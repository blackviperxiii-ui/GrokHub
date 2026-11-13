//! One auto setup pass: the one function both apps call.
//!
//! It takes over settings the user changed, settles each setting between the
//! rules that want it, and sorts the winners: applied now (sure, and allowed
//! to change it, and within the change budget), offered as a suggestion, or
//! left alone. The caller writes the applied changes through its
//! [`super::settings::Target`], then records them with [`apply`].

use std::collections::BTreeMap;

use super::questions::{answered_by_device, bank, fits_device, Pace, Qid, Question};
use super::rules::{propose, Proposal, Signals};
use super::settings::{Key, Value};
use super::store::{AutoStore, JournalLine, Source};

/// Sure enough to change it without asking.
pub const ACT_AT: f32 = 0.8;
/// Sure enough to offer it.
pub const SUGGEST_AT: f32 = 0.5;

/// When a pass runs. Visible changes wait for a launch or a return from
/// idle, so nothing moves under the user's hands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    /// The first-run setup: everything it is sure of, no budget.
    FirstRun,
    Launch,
    BackFromIdle,
    /// Power, screen or OS look changed while in use.
    Event,
}

/// A change to make or offer.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub key: Key,
    pub from: Option<Value>,
    pub to: Value,
    pub rule: &'static str,
    pub asked: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Make these now, then call [`apply`].
    pub apply: Vec<Change>,
    /// Offer these with a Yes and a No.
    pub suggest: Vec<Change>,
    /// History lines for settings the user took over (already in the store).
    pub took_over: Vec<JournalLine>,
}

/// The one proposal a setting settles on: an answer beats a guess, then the
/// surer rule, then the value that costs less if wrong.
pub fn settle(props: Vec<Proposal>) -> BTreeMap<Key, Proposal> {
    let mut best: BTreeMap<Key, Proposal> = BTreeMap::new();
    for p in props {
        let keep = match best.get(&p.key) {
            None => true,
            Some(cur) => beats(&p, cur),
        };
        if keep {
            best.insert(p.key, p);
        }
    }
    best
}

fn beats(a: &Proposal, b: &Proposal) -> bool {
    if a.asked != b.asked {
        return a.asked;
    }
    if (a.confidence - b.confidence).abs() > f32::EPSILON {
        return a.confidence > b.confidence;
    }
    let safer = a.key.spec().safer.map(|f| f());
    safer.is_some_and(|s| a.value == s && b.value != s)
}

/// Work out the pass. Updates the store for settings the user took over;
/// nothing else changes until [`apply`].
pub fn plan_pass(s: &Signals, store: &mut AutoStore, moment: Moment, now_ms: u64) -> Plan {
    let took_over = store.take_over_changed(&s.current, now_ms);
    let mut plan = Plan {
        took_over,
        ..Plan::default()
    };
    let mut budget = match moment {
        Moment::FirstRun => usize::MAX,
        Moment::Launch | Moment::BackFromIdle => store.visible_left(now_ms),
        Moment::Event => 0,
    };
    for (key, p) in settle(propose(s)) {
        if store.locked(key) || store.rules_off.contains(p.rule) {
            continue;
        }
        let from = s.current.get(&key).cloned();
        // An app without this setting can't take it.
        let Some(now) = from.clone() else {
            continue;
        };
        if now == p.value {
            continue;
        }
        // Already put there once by this rule and since changed by the user
        // is covered by the lock; an entry with this very value means a
        // previous pass already did it.
        if store
            .entries
            .get(&key)
            .is_some_and(|e| e.applied == p.value)
        {
            continue;
        }
        let change = Change {
            key,
            from,
            to: p.value.clone(),
            rule: p.rule,
            asked: p.asked,
            reason: p.reason.clone(),
        };
        let spec = key.spec();
        let sure = p.asked || (spec.auto && p.confidence >= ACT_AT);
        if sure {
            if !spec.visible || (p.asked && moment == Moment::FirstRun) {
                plan.apply.push(change);
            } else if budget > 0 {
                budget -= 1;
                plan.apply.push(change);
            } else if moment == Moment::Event {
                // Wait for the next launch or return from idle.
            } else {
                plan.suggest.push(change);
            }
        } else if p.confidence >= SUGGEST_AT && !store.resting(p.rule, now_ms) {
            plan.suggest.push(change);
        }
    }
    plan
}

/// Record changes the caller made (only the ones its config accepted). A
/// visible change auto setup made on its own, outside first run, spends the
/// change budget.
pub fn apply(
    store: &mut AutoStore,
    made: &[Change],
    moment: Moment,
    now_ms: u64,
) -> Vec<JournalLine> {
    made.iter()
        .map(|c| {
            if c.key.spec().visible && !c.asked && moment != Moment::FirstRun {
                store.spend(now_ms);
            }
            let source = if c.asked {
                Source::Asked {
                    rule: c.rule.to_string(),
                }
            } else {
                Source::Inferred {
                    rule: c.rule.to_string(),
                }
            };
            store.record(
                c.key,
                c.from.clone(),
                c.to.clone(),
                source,
                &c.reason,
                now_ms,
            )
        })
        .collect()
}

/// The user said yes to a suggestion: record it as theirs.
pub fn accept(store: &mut AutoStore, c: &Change, now_ms: u64) -> JournalLine {
    crate::learn::hub::setup_outcome(c.rule, crate::learn::decide::Outcome::Accepted, now_ms);
    store.record(
        c.key,
        c.from.clone(),
        c.to.clone(),
        Source::Asked {
            rule: c.rule.to_string(),
        },
        &c.reason,
        now_ms,
    )
}

/// A setting asked for in chat or set by hand through auto setup's page:
/// the user's from now on, with Undo to what it was.
pub fn user_set(
    store: &mut AutoStore,
    key: Key,
    from: Option<Value>,
    to: Value,
    now_ms: u64,
) -> JournalLine {
    store.record(
        key,
        from,
        to,
        Source::UserSet,
        "You asked for this.",
        now_ms,
    )
}

/// The next first-run question worth asking, or `None` when done: never one
/// the PC already answered, one this PC can't use, or one whose every answer
/// would leave the settings as they are. At most [`Pace::limit`] questions.
pub fn next_question(s: &Signals, asked: &[Qid], pace: Pace) -> Option<&'static Question> {
    if asked.len() >= pace.limit() {
        return None;
    }
    let base = settle(propose(s));
    bank().iter().find(|q| {
        !asked.contains(&q.id)
            && !s.answers.contains_key(&q.id)
            && answered_by_device(q.id, &s.device).is_none()
            && fits_device(q.id, &s.device)
            && q.choices.iter().any(|c| {
                let mut with = s.clone();
                with.answers.insert(q.id, c.value.to_string());
                settle(propose(&with)).iter().any(|(k, p)| {
                    let now = s.current.get(k);
                    now.is_some()
                        && now != Some(&p.value)
                        // A new value, or a guess the answer makes sure.
                        && base
                            .get(k)
                            .is_none_or(|b| b.value != p.value || b.asked != p.asked)
                })
            })
    })
}

#[cfg(test)]
mod tests {
    use super::super::device::tests::{beast, surface_pro_7};
    use super::super::device::OsLook;
    use super::super::questions::Qid;
    use super::super::rules::Usage;
    use super::*;

    const T: u64 = 1_800_000_000_000;
    const DAY: u64 = 24 * 60 * 60 * 1_000;

    /// A fresh native app's settings.
    fn fresh() -> BTreeMap<Key, Value> {
        let mut m = BTreeMap::new();
        m.insert(Key::TextSize, Value::Pct(100));
        m.insert(Key::ReduceMotion, Value::Bool(false));
        m.insert(Key::Theme, Value::pick("dark"));
        m.insert(Key::Effects, Value::Bool(true));
        m.insert(Key::CloseToTray, Value::Bool(true));
        m.insert(Key::LocalModel, Value::Bool(false));
        m.insert(Key::GameCompanion, Value::Bool(false));
        m.insert(Key::GameNudges, Value::Bool(false));
        m.insert(Key::QuietHours, Value::Hours(22, 7));
        m.insert(Key::DreamHour, Value::Hour(21));
        m.insert(Key::Personality, Value::pick("friendly"));
        m.insert(Key::CollapseThoughts, Value::Bool(false));
        m
    }

    fn surface_150() -> Signals {
        let mut device = surface_pro_7();
        device.look = OsLook {
            text_scale_pct: Some(150),
            ..OsLook::default()
        };
        Signals {
            device,
            current: fresh(),
            ..Signals::default()
        }
    }

    /// Write a plan's changes into `current`, as an app would.
    fn make(s: &mut Signals, store: &mut AutoStore, plan: &Plan, moment: Moment, at: u64) {
        for c in &plan.apply {
            s.current.insert(c.key, c.to.clone());
        }
        apply(store, &plan.apply, moment, at);
    }

    #[test]
    fn first_run_on_a_surface_applies_what_it_is_sure_of_and_offers_the_rest() {
        let mut s = surface_150();
        let mut store = AutoStore::default();
        let plan = plan_pass(&s, &mut store, Moment::FirstRun, T);
        let applied: Vec<(Key, Value)> = plan.apply.iter().map(|c| (c.key, c.to.clone())).collect();
        assert_eq!(
            applied,
            vec![
                (Key::TextSize, Value::Pct(150)),
                (Key::ReduceMotion, Value::Bool(true)),
                (Key::Effects, Value::Bool(false)),
            ]
        );
        let offered: Vec<Key> = plan.suggest.iter().map(|c| c.key).collect();
        assert_eq!(
            offered,
            vec![Key::CloseToTray],
            "not sure enough to quit on close"
        );
        make(&mut s, &mut store, &plan, Moment::FirstRun, T);
        // Running again changes nothing: no flip-flop.
        let again = plan_pass(&s, &mut store, Moment::Launch, T + DAY);
        assert!(again.apply.is_empty(), "{:?}", again.apply);
        assert!(again.took_over.is_empty());
    }

    #[test]
    fn a_later_launch_spends_one_visible_change_a_day() {
        let mut s = surface_150();
        let mut store = AutoStore::default();
        let plan = plan_pass(&s, &mut store, Moment::Launch, T);
        assert_eq!(plan.apply.len(), 1, "one visible change on a launch");
        assert_eq!(plan.apply[0].key, Key::TextSize);
        assert_eq!(
            plan.suggest.iter().map(|c| c.key).collect::<Vec<_>>(),
            vec![Key::ReduceMotion, Key::Effects, Key::CloseToTray],
            "the rest wait as suggestions"
        );
        make(&mut s, &mut store, &plan, Moment::Launch, T);
        let next = plan_pass(&s, &mut store, Moment::BackFromIdle, T + DAY / 2);
        assert!(next.apply.is_empty());
        let tomorrow = plan_pass(&s, &mut store, Moment::Launch, T + DAY);
        assert_eq!(tomorrow.apply.len(), 1);
        assert_eq!(tomorrow.apply[0].key, Key::ReduceMotion);
        // In use, nothing visible moves.
        let mut store2 = AutoStore::default();
        let ev = plan_pass(&surface_150(), &mut store2, Moment::Event, T);
        assert!(ev.apply.is_empty());
        assert_eq!(ev.suggest.len(), 1, "only the suggestion-band rule");
    }

    #[test]
    fn what_the_user_changes_stays_theirs_and_two_undos_turn_a_rule_off() {
        let mut s = surface_150();
        let mut store = AutoStore::default();
        let plan = plan_pass(&s, &mut store, Moment::FirstRun, T);
        make(&mut s, &mut store, &plan, Moment::FirstRun, T);
        // They set text to 115 themselves.
        s.current.insert(Key::TextSize, Value::Pct(115));
        let p = plan_pass(&s, &mut store, Moment::Launch, T + DAY);
        assert_eq!(p.took_over.len(), 1);
        assert!(p.apply.is_empty());
        assert!(store.locked(Key::TextSize));
        // They undo the effects change twice across a hand-back: rule off.
        let (back, _) = store.undo(Key::Effects, T + 2 * DAY).expect("undo");
        s.current.insert(Key::Effects, back);
        store.manage_again(Key::Effects);
        let p = plan_pass(&s, &mut store, Moment::Launch, T + 3 * DAY);
        assert_eq!(
            p.apply.iter().map(|c| c.key).collect::<Vec<_>>(),
            vec![Key::Effects]
        );
        make(&mut s, &mut store, &p, Moment::Launch, T + 3 * DAY);
        let (back, _) = store.undo(Key::Effects, T + 4 * DAY).expect("undo");
        s.current.insert(Key::Effects, back);
        store.entries.remove(&Key::Effects);
        let p = plan_pass(&s, &mut store, Moment::Launch, T + 5 * DAY);
        assert!(p.apply.iter().all(|c| c.key != Key::Effects));
        assert!(p.suggest.iter().all(|c| c.key != Key::Effects));
    }

    #[test]
    fn the_beast_only_gets_offers_and_never_loses_its_local_model() {
        let mut device = beast();
        device.free_disk_mb = 400_000;
        let mut current = fresh();
        let mut s = Signals {
            device: device.clone(),
            current: current.clone(),
            ..Signals::default()
        };
        let mut store = AutoStore::default();
        let plan = plan_pass(&s, &mut store, Moment::FirstRun, T);
        assert!(plan.apply.is_empty(), "nothing it isn't asked about");
        assert_eq!(
            plan.suggest.iter().map(|c| c.key).collect::<Vec<_>>(),
            vec![Key::LocalModel, Key::GameCompanion, Key::GameNudges]
        );
        // A model already running, and "Not now" to the local AI question.
        current.insert(Key::LocalModel, Value::Bool(true));
        s.current = current;
        s.answers.insert(Qid::LocalAi, "no".into());
        let plan = plan_pass(&s, &mut store, Moment::FirstRun, T);
        assert!(plan.apply.iter().all(|c| c.key != Key::LocalModel));
        assert!(plan.suggest.iter().all(|c| c.key != Key::LocalModel));
    }

    #[test]
    fn answers_apply_at_first_run_even_when_visible() {
        let mut s = Signals {
            device: beast(),
            current: fresh(),
            ..Signals::default()
        };
        s.answers.insert(Qid::Talk, "direct".into());
        s.answers.insert(Qid::Sleep, "23".into());
        s.answers.insert(Qid::Games, "yes".into());
        let mut store = AutoStore::default();
        let plan = plan_pass(&s, &mut store, Moment::FirstRun, T);
        let applied: Vec<Key> = plan.apply.iter().map(|c| c.key).collect();
        assert_eq!(
            applied,
            vec![
                Key::QuietHours,
                Key::DreamHour,
                Key::GameCompanion,
                Key::GameNudges,
                Key::Personality
            ]
        );
        let lines = apply(&mut store, &plan.apply, Moment::FirstRun, T);
        assert!(store.visible_at.is_empty(), "first run spends no budget");
        assert_eq!(lines.len(), 5);
        assert!(matches!(
            store.entries[&Key::Personality].source,
            Source::Asked { .. }
        ));
        assert_eq!(
            store.entries[&Key::Personality].before,
            Some(Value::pick("friendly"))
        );
    }

    #[test]
    fn a_setting_this_app_lacks_is_never_planned() {
        let mut current = fresh();
        current.remove(&Key::Effects);
        let s = Signals {
            current,
            ..surface_150()
        };
        let plan = plan_pass(&s, &mut AutoStore::default(), Moment::FirstRun, T);
        assert!(plan
            .apply
            .iter()
            .chain(&plan.suggest)
            .all(|c| c.key != Key::Effects));
    }

    #[test]
    fn a_dismissed_suggestion_rests_and_usage_rules_apply_when_sure() {
        let mut s = Signals {
            device: beast(),
            current: fresh(),
            usage: Usage {
                thoughts_closed_streak: 7,
                ..Usage::default()
            },
            ..Signals::default()
        };
        s.device.free_disk_mb = 400_000;
        let mut store = AutoStore::default();
        store.dismiss("pc_runs_local_ai", T);
        let plan = plan_pass(&s, &mut store, Moment::Launch, T + DAY);
        assert!(plan.suggest.iter().all(|c| c.key != Key::LocalModel));
        assert_eq!(
            plan.apply.iter().map(|c| c.key).collect::<Vec<_>>(),
            vec![Key::CollapseThoughts]
        );
        assert_eq!(
            plan.apply[0].reason,
            "Thinking now starts collapsed, because you close it every time."
        );
    }

    #[test]
    fn questions_skip_what_the_pc_says_and_stop_at_the_pace_limit() {
        let mut current = fresh();
        current.insert(Key::Notifications, Value::pick("all"));
        current.insert(Key::Approvals, Value::pick("ask"));
        let mut s = Signals {
            device: beast(),
            current,
            ..Signals::default()
        };
        let mut asked = Vec::new();
        while let Some(q) = next_question(&s, &asked, Pace::Detailed) {
            asked.push(q.id);
            // The last choice each time.
            let pick = q.choices[q.choices.len() - 1].value;
            s.answers.insert(q.id, pick.into());
        }
        // Steam answers Games; no code folders setting here, no local AI
        // offer without free disk.
        assert_eq!(
            asked,
            vec![
                Qid::Talk,
                Qid::Approvals,
                Qid::Notifications,
                Qid::Sleep,
                Qid::Background
            ]
        );
        let mut quick = Signals {
            answers: Default::default(),
            ..s.clone()
        };
        let mut asked = Vec::new();
        while let Some(q) = next_question(&quick, &asked, Pace::Quick) {
            asked.push(q.id);
            quick.answers.insert(q.id, q.choices[0].value.into());
        }
        assert_eq!(asked.len(), 3);
        // A PC with Steam and code folders and room for local AI.
        let mut device = beast();
        device.free_disk_mb = 400_000;
        device.dev_folders = true;
        let mut current = fresh();
        current.insert(Key::ResumeCode, Value::Bool(false));
        let s = Signals {
            device,
            current,
            ..Signals::default()
        };
        assert_eq!(
            next_question(&s, &[], Pace::Detailed).map(|q| q.id),
            Some(Qid::Talk)
        );
        let mut asked = vec![
            Qid::Talk,
            Qid::Approvals,
            Qid::Notifications,
            Qid::Sleep,
            Qid::Background,
        ];
        assert_eq!(
            next_question(&s, &asked, Pace::Detailed).map(|q| q.id),
            Some(Qid::LocalAi)
        );
        asked.push(Qid::LocalAi);
        assert_eq!(
            next_question(&s, &asked, Pace::Detailed).map(|q| q.id),
            None,
            "Coding: the folders said"
        );
    }

    #[test]
    fn ties_settle_on_the_safer_value() {
        let a = Proposal {
            key: Key::Effects,
            value: Value::Bool(true),
            confidence: 0.7,
            asked: false,
            rule: "x",
            reason: String::new(),
        };
        let b = Proposal {
            value: Value::Bool(false),
            rule: "y",
            ..a.clone()
        };
        assert_eq!(settle(vec![a.clone(), b.clone()])[&Key::Effects].rule, "y");
        assert_eq!(settle(vec![b, a])[&Key::Effects].rule, "y");
    }

    #[test]
    fn accept_and_user_set_record_with_undo() {
        let mut store = AutoStore::default();
        let c = Change {
            key: Key::ChatObjectives,
            from: Some(Value::Bool(false)),
            to: Value::Bool(true),
            rule: "wide_screen",
            asked: false,
            reason: "wide".into(),
        };
        accept(&mut store, &c, T);
        assert!(!store.locked(Key::ChatObjectives));
        user_set(
            &mut store,
            Key::TextSize,
            Some(Value::Pct(100)),
            Value::Pct(130),
            T + 1,
        );
        assert!(store.locked(Key::TextSize));
        // "Undo that" after a chat ask undoes the ask.
        assert_eq!(store.last_undoable(), Some(Key::TextSize));
        store.undo(Key::TextSize, T + 2).expect("undo");
        assert_eq!(store.last_undoable(), Some(Key::ChatObjectives));
    }
}
