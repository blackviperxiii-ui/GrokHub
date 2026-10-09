//! Spike-6b in the cabin: GrokHub does a small, safe, undoable thing on its
//! own when it's sure you won't mind, then says so on a Done-for-you card.
//!
//! - `tick_auto_act` (inside `tick_anticipate`) takes one candidate, reads
//!   its class from the harness, and fills the autonomy ceiling from cabin
//!   state: scope grant, Access, the permission pill, MindCheck over the
//!   proactive span log, quiet hours, busy, Halt, and today's auto budget.
//! - Admitted: `hx::run_auto_act` asks `harness::decide` and runs the step
//!   through the native dispatch (path E, origin proactive). The ledger line
//!   it wrote is the card's Undo.
//! - Missed: an `ask` span names the `CeilingMiss`, and the step becomes a
//!   Spike-6a card under its card budget: a one-tap suggestion when MindCheck is
//!   why, "I can …" otherwise. Quiet hours queue it; busy drops it.
//! - Hard class: prepare, don't do. The prepared step parks a hard card,
//!   whatever Access, the pill or the hour; an unattended park times out to
//!   a Deny span.
//!
//! `AutoState::queue` is where auto-act candidates come in. The 6a engine's
//! sources carry no tool arguments yet, so nothing fills it in the running
//! app; `AutoBudget` (5 a day) sits beside 6a's card budget.

use std::collections::VecDeque;
use std::sync::Arc;

use super::*;
use grokhub_agent::harness::{self as hx, ChangeKind};
use grokhub_agent::{AccessMode, HardClass};
use grokhub_core::proactive as pro;
use grokhub_core::UpdateKind;
use grokhub_core::{AccessTier, AutoAct, AutoBudget, AutoCandidate, CeilingCtx, CeilingMiss, DoneForYou, PillMode};

#[derive(Debug, Default)]
pub(super) struct AutoState {
    /// Candidates waiting for the next tick, oldest first.
    pub queue: VecDeque<AutoCandidate>,
    /// Auto-acts spent today.
    pub budget: AutoBudget,
    /// Ledger lines auto-acts wrote. Their Done-for-you card stands in for
    /// the Work-tree row and the Changed update.
    pub auto_lines: Vec<(ChangeKind, u64)>,
}

fn tier(access: AccessMode) -> AccessTier {
    match access {
        AccessMode::Readonly => AccessTier::Readonly,
        AccessMode::Supervised => AccessTier::Supervised,
        AccessMode::Full => AccessTier::Full,
    }
}

fn pill(mode: PermissionMode) -> PillMode {
    match mode {
        PermissionMode::Ask => PillMode::Ask,
        PermissionMode::Auto => PillMode::Auto,
        PermissionMode::AlwaysApprove => PillMode::Always,
    }
}

impl Cabin {
    /// The ceiling's inputs for one candidate, from cabin state.
    pub(super) fn ceiling_ctx(&mut self, candidate: &AutoCandidate, mind: &hx::MindCheck) -> CeilingCtx {
        let dir = config::config_dir();
        let in_scope = candidate
            .scope
            .as_deref()
            .and_then(hx::Scope::parse)
            .is_some_and(|scope| hx::ConsentLedger::load(&dir).scope_grant(&scope).is_some());
        let quiet = self.quiet_now();
        let busy = self.heartbeat_busy();
        let halted = self.heartbeat_halted(now_ms());
        let route = mind.mind_route(&hx::Candidate { key: &candidate.key, hard: None });
        CeilingCtx {
            in_scope,
            access: tier(self.access_mode()),
            pill: pill(self.permission_mode),
            p_mind: mind.prior(&candidate.key).map(|p| p.p_mind()),
            mind_asks: route == hx::MindRoute::Ask,
            quiet,
            busy,
            halted,
            budget_left: self.auto_act.budget.left(&Self::local_day(), quiet, busy),
        }
    }

    /// One candidate per tick: hard class parks, the ceiling admits or sends
    /// it to an ask card, and an admitted one runs through the harness.
    pub(super) fn tick_auto_act(&mut self) {
        self.expire_hard_park();
        let Some(mut c) = self.auto_act.queue.pop_front() else {
            return;
        };
        let dir = config::config_dir();
        // The harness decides the class and the key, not the candidate's source.
        c.hard = hx::step_class(&c.tool, &c.arguments);
        c.key = hx::proactive_key(&c.tool, &c.arguments);
        let by_hand = c.reversibility > 0.0;
        if hx::ledger_target(&c.tool, &c.arguments).is_none() {
            c.reversibility = 0.0;
        }
        let access = self.access_mode();
        if let Some(class) = c.hard.clone() {
            self.prepare_hard(&c, &class, access);
            return;
        }
        let mind = hx::proactive_mind(&dir, Arc::new(hx::SystemClock));
        let ctx = self.ceiling_ctx(&c, &mind);
        let act = match AutoAct::admit(c.clone(), &ctx) {
            Ok(act) => act,
            Err(miss) => return self.send_to_ask(c, miss, access, by_hand),
        };
        match hx::run_auto_act(&dir, &self.native_workspace(), &act, &mind, access) {
            hx::AutoRun::Done { kind, change } => {
                self.auto_act.budget.spend(&Self::local_day());
                self.auto_act.auto_lines.push((kind, change.seq));
                let done = DoneForYou {
                    kind: kind.as_str().into(),
                    target: change.id.clone(),
                    undo_ref: change.seq,
                    mind_key: c.key.clone(),
                    answered: false,
                };
                self.post_feed_card(grokhub_core::done_for_you_card(&c.summary, &c.why, done, now_ms()));
            }
            // The gate disagreed with the ceiling: it asks.
            hx::AutoRun::Asked(_) => self.offer_card(&c, CeilingMiss::MindCheck, by_hand),
            hx::AutoRun::NoUndo => self.offer_card(&c, CeilingMiss::Reversibility, by_hand),
            hx::AutoRun::Failed(why) => self.status = format!("GrokHub tried {} on its own and stopped: {why}", c.tool),
        }
    }

    fn send_to_ask(&mut self, c: AutoCandidate, miss: CeilingMiss, access: AccessMode, by_hand: bool) {
        let span = hx::proactive_span(&c.tool, &c.arguments, hx::DECISION_ASK, miss.as_str(), access);
        hx::note_proactive(&config::config_dir(), &span);
        self.offer_card(&c, miss, by_hand);
    }

    /// A step the ceiling did not admit becomes a Spike-6a card under its
    /// card budget: a one-tap suggestion when MindCheck is why, "I can …"
    /// otherwise. A click on it meets `harness::decide` like any proactive
    /// card. A class you said "Don't do this again" to is never offered.
    fn offer_card(&mut self, c: &AutoCandidate, miss: CeilingMiss, by_hand: bool) {
        let mind = hx::proactive_mind(&config::config_dir(), Arc::new(hx::SystemClock));
        if mind.prior(&c.key).is_some_and(|p| p.never) {
            return;
        }
        let now = now_ms();
        let mut cand = pro::Candidate::soft(pro::CandidateSource::SystemState, &c.offer, &c.offer, c.value, c.confidence, &c.why);
        cand.scope = c.scope.clone();
        cand.tool = c.tool.clone();
        cand.reversible = match (c.reversibility >= 1.0, by_hand) {
            (true, _) => pro::Reversibility::Ledger,
            (false, true) => pro::Reversibility::ByHand,
            (false, false) => pro::Reversibility::Irreversible,
        };
        let route = match miss {
            CeilingMiss::PMind | CeilingMiss::MindCheck | CeilingMiss::NoHistory => pro::ProactiveRoute::Suggest,
            _ => pro::ProactiveRoute::ICan,
        };
        let (quiet, busy) = (self.quiet_now(), self.heartbeat_busy());
        let out = self.proactive.surface(vec![cand], now, quiet, busy);
        for cand in &out {
            grokhub_core::post_update(&mut self.updates, pro::proactive_card(cand, route, now));
        }
        if !out.is_empty() {
            self.persist_updates();
        }
        self.save_proactive();
    }

    /// Hard class: prepare, don't do. The prepared step waits on a hard card
    /// (no Always, Enter never approves, Esc and TTL deny). A floor hit is refused.
    fn prepare_hard(&mut self, c: &AutoCandidate, class: &str, access: AccessMode) {
        let dir = config::config_dir();
        let Some(class) = HardClass::parse(class) else {
            let span = hx::Span::deny(hx::PROACTIVE_TRACE, &c.tool, &c.arguments, "hard floor", "floor")
                .on_path("E")
                .from_origin(hx::Origin::Proactive);
            hx::note_proactive(&dir, &span);
            return;
        };
        let mut span = hx::Span::hard_park(hx::PROACTIVE_TRACE, &c.tool, &c.arguments, class)
            .on_path("E")
            .from_origin(hx::Origin::Proactive);
        span.access = access.as_str().into();
        hx::note_proactive(&dir, &span);
        self.park_proactive(class, c.tool.clone(), c.arguments.clone(), c.summary.clone());
    }

    /// Ledger lines an auto-act wrote: this run's, plus those Done-for-you
    /// cards point at (after a restart).
    pub(super) fn auto_lines(&self) -> Vec<(ChangeKind, u64)> {
        let mut out = self.auto_act.auto_lines.clone();
        out.extend(
            self.updates
                .iter()
                .filter_map(|c| c.done_for_you.as_ref())
                .filter_map(|d| ChangeKind::parse(&d.kind).map(|k| (k, d.undo_ref))),
        );
        out
    }

    fn done_card(&self, id: &str) -> Option<DoneForYou> {
        self.updates
            .iter()
            .find(|c| c.id == id && c.kind == UpdateKind::DoneForYou)
            .and_then(|c| c.done_for_you.clone())
            .filter(|d| !d.answered)
    }

    fn mark_answered(&mut self, id: &str) {
        if let Some(d) = self.updates.iter_mut().find(|c| c.id == id).and_then(|c| c.done_for_you.as_mut()) {
            d.answered = true;
        }
        let _ = grokhub_core::mark_update_opened(&mut self.updates, id);
        self.persist_updates();
    }

    /// Undo on a Done-for-you card. Click only: the card's pill is the one
    /// caller, and it builds the `UndoAsk` from that click.
    pub(super) fn done_for_you_undo(&mut self, id: &str) {
        let Some(done) = self.done_card(id) else {
            return;
        };
        let Some(kind) = ChangeKind::parse(&done.kind) else {
            return;
        };
        let row = super::change_undo::ChangeRow {
            kind,
            label: format!("Grok changed {} {}", kind.as_str(), done.target),
            id: done.target.clone(),
            keep: false,
        };
        if !self.change_row_clicked(&row, super::change_undo::ChangeAct::Undo) {
            return;
        }
        let span = hx::answer_span(&done.mind_key, hx::DECISION_UNDO, self.access_mode());
        hx::note_proactive(&config::config_dir(), &span);
        self.mark_answered(id);
        self.status = "Undone. GrokHub will ask before doing this again.".into();
    }

    /// "Don't do this again": `p_mind` 1.0 for this class. It never acts on
    /// its own again and asks if it's ever suggested.
    pub(super) fn done_for_you_never(&mut self, id: &str) {
        let Some(done) = self.done_card(id) else {
            return;
        };
        let span = hx::answer_span(&done.mind_key, hx::DECISION_NEVER, self.access_mode());
        hx::note_proactive(&config::config_dir(), &span);
        self.mark_answered(id);
        self.status = "GrokHub won't do or suggest this again.".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const NOTES: &str = r#"{"name":"notes"}"#;

    struct Rig {
        root: std::path::PathBuf,
        _pin: crate::config::TestConfigDir,
        cabin: Cabin,
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            std::env::remove_var("GROKHUB_CONFIG");
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Full, Auto pill, quiet hours off, the system-state scope granted, and
    /// `servers` in the cabin's own `mcp.json` (written the way the ledger
    /// writes it, so Undo can be checked byte for byte).
    fn rig(label: &str, servers: &[&str]) -> Rig {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("config root");
        std::env::set_var("GROKHUB_CONFIG", &root);
        let pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_control = true;
        cabin.harness.access_full = true;
        cabin.permission_mode = PermissionMode::Auto;
        cabin.cfg.quiet_start = "00:00".into();
        cabin.cfg.quiet_end = "00:00".into();
        hx::grant_scope(&root, &hx::Scope::SystemState, None, hx::UserClick::from_click()).expect("grant");
        let mut map = serde_json::Map::new();
        for name in servers {
            map.insert((*name).into(), serde_json::json!({ "command": format!("{name}-mcp") }));
        }
        let text = serde_json::to_string_pretty(&serde_json::json!({ "mcpServers": map })).unwrap();
        std::fs::write(grokhub_agent::mcp::config_file(), text).expect("mcp.json");
        Rig { root, _pin: pin, cabin }
    }

    fn approve_history(root: &std::path::Path, n: usize) {
        for _ in 0..n {
            let span = hx::proactive_span("connection_disable", NOTES, "approve", "", AccessMode::Full);
            hx::note_proactive(root, &span);
        }
    }

    fn turn_off(name: &str) -> AutoCandidate {
        let args = serde_json::json!({ "name": name, "reason": "it failed every start this week" }).to_string();
        AutoCandidate {
            key: String::new(),
            tool: "connection_disable".into(),
            arguments: args,
            hard: None,
            scope: Some("system_state".into()),
            desktop: false,
            value: 0.8,
            confidence: 0.9,
            reversibility: 1.0,
            summary: format!("Turned off the {name} connection"),
            offer: format!("turn off the {name} connection"),
            why: "It failed every start this week.".into(),
        }
    }

    fn reply_to_sam() -> AutoCandidate {
        AutoCandidate {
            key: String::new(),
            tool: "mail_send".into(),
            arguments: r#"{"to":"sam@example.com","body":"Thanks Sam, Thursday at 3 works."}"#.into(),
            hard: None,
            scope: Some("system_state".into()),
            desktop: false,
            value: 0.9,
            confidence: 0.95,
            reversibility: 1.0,
            summary: "Reply to Sam: Thanks Sam, Thursday at 3 works.".into(),
            offer: "reply to Sam".into(),
            why: "Sam asked if Thursday works.".into(),
        }
    }

    fn mcp_bytes() -> Vec<u8> {
        std::fs::read(grokhub_agent::mcp::config_file()).expect("mcp.json")
    }

    fn proactive_spans(root: &std::path::Path) -> Vec<hx::Span> {
        hx::read_spans(root, hx::PROACTIVE_TRACE).unwrap_or_default()
    }

    /// Spike-6a cards a missed step became: (tool, route).
    fn offer_cards(cabin: &Cabin) -> Vec<(String, pro::ProactiveRoute)> {
        cabin
            .updates
            .iter()
            .filter_map(|c| c.pulse.proactive.as_ref())
            .map(|m| (m.tool.clone(), m.route))
            .collect()
    }

    fn last_ask(root: &std::path::Path) -> String {
        proactive_spans(root).into_iter().rfind(|s| s.decision == "ask").map(|s| s.claim).unwrap_or_default()
    }

    fn done_cards(cabin: &Cabin) -> Vec<grokhub_core::UpdateCard> {
        cabin.updates.iter().filter(|c| c.kind == UpdateKind::DoneForYou).cloned().collect()
    }

    /// Acceptance 1: {soft, reversible 1.0, value .8, conf .9, p_mind 0 with
    /// history} in Full and in scope auto-acts, posts a Done-for-you card
    /// with Undo, and Undo puts `mcp.json` back byte for byte.
    #[test]
    fn a_soft_reversible_candidate_auto_acts_and_undo_restores_the_bytes() {
        let _g = crate::config::hold_test_config();
        let mut r = rig("dfy-soft", &["notes", "calendar"]);
        approve_history(&r.root, 1);
        let before = mcp_bytes();
        r.cabin.auto_act.queue.push_back(turn_off("notes"));
        r.cabin.tick_auto_act();

        assert!(offer_cards(&r.cabin).is_empty());
        let after = String::from_utf8(mcp_bytes()).unwrap();
        assert!(after.contains("\"enabled\": false"), "{after}");
        let ledger = hx::ChangeLedger::load_kind(&r.root, ChangeKind::Connection);
        let line = ledger.all().last().cloned().expect("ledger line");
        assert_eq!(line.id, "notes");
        let cards = done_cards(&r.cabin);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "Turned off the notes connection");
        assert_eq!(cards[0].body.as_deref(), Some("It failed every start this week."));
        assert_eq!(
            cards[0].done_for_you,
            Some(DoneForYou {
                kind: "connection".into(),
                target: "notes".into(),
                undo_ref: line.seq,
                mind_key: "proactive:connection_disable".into(),
                answered: false,
            })
        );
        let auto: Vec<hx::Span> = proactive_spans(&r.root).into_iter().filter(|s| s.decision == "auto").collect();
        assert_eq!(auto.len(), 1);
        assert_eq!(auto[0].undo_ref, format!("connection:{}", line.seq));
        assert_eq!(auto[0].origin, hx::Origin::Proactive);
        assert_eq!(auto[0].path, "E");
        assert_eq!(auto[0].access, "full");
        // The card stands in for the Work-tree row and the Changed update.
        r.cabin.poll_self_changes();
        assert!(r.cabin.harness.work_rows.is_empty());
        assert!(r.cabin.updates.iter().all(|c| c.kind != UpdateKind::SelfChange));

        let id = cards[0].id.clone();
        r.cabin.done_for_you_undo(&id);
        assert_eq!(mcp_bytes(), before, "Undo restores the fixture byte for byte");
        assert_eq!(r.cabin.status, "Undone. GrokHub will ask before doing this again.");
        assert_eq!(done_cards(&r.cabin)[0].done_for_you.as_ref().map(|d| d.answered), Some(true));
        let mind = hx::proactive_mind(&r.root, Arc::new(hx::SystemClock));
        assert_eq!(mind.mind_prior("proactive:connection_disable"), 0.6);
        // A second click on the answered card does nothing.
        r.cabin.done_for_you_undo(&id);
        assert_eq!(mcp_bytes(), before);
    }

    /// Each ceiling term off in turn: no auto-act, the right miss, nothing written.
    #[test]
    fn each_ceiling_term_off_sends_it_to_an_ask_card() {
        let _g = crate::config::hold_test_config();
        type Setup = fn(&mut Rig) -> AutoCandidate;
        let cases: Vec<(&str, Setup, CeilingMiss)> = vec![
            ("scope", |r| AutoCandidate { scope: Some("calendar".into()), ..turn_off_with(r) }, CeilingMiss::NotInScope),
            (
                "watch",
                |r| {
                    r.cabin.cfg.desktop_control = false;
                    turn_off_with(r)
                },
                CeilingMiss::Access,
            ),
            (
                "pill",
                |r| {
                    r.cabin.permission_mode = PermissionMode::Ask;
                    turn_off_with(r)
                },
                CeilingMiss::Pill,
            ),
            (
                "quiet",
                |r| {
                    let c = Cabin::local_clock();
                    let now = c.hour * 60 + c.minute;
                    let hm = |m: u32| format!("{:02}:{:02}", (m % 1440) / 60, m % 60);
                    r.cabin.cfg.quiet_start = hm(now + 1440 - 60);
                    r.cabin.cfg.quiet_end = hm(now + 60);
                    turn_off_with(r)
                },
                CeilingMiss::QuietHours,
            ),
            (
                "budget",
                |r| {
                    for _ in 0..grokhub_core::AUTO_PER_DAY {
                        r.cabin.auto_act.budget.spend(&Cabin::local_day());
                    }
                    turn_off_with(r)
                },
                CeilingMiss::Budget,
            ),
            (
                "p_mind",
                |r| {
                    let undo = hx::proactive_span("connection_disable", NOTES, hx::DECISION_UNDO, "", AccessMode::Full);
                    hx::note_proactive(&r.root, &undo);
                    approve_history(&r.root, 8);
                    turn_off("notes")
                },
                CeilingMiss::PMind,
            ),
            ("history", |_| turn_off("notes"), CeilingMiss::NoHistory),
            ("confidence", |r| AutoCandidate { confidence: 0.79, ..turn_off_with(r) }, CeilingMiss::Confidence),
            ("reversibility", |r| AutoCandidate { reversibility: 0.5, ..turn_off_with(r) }, CeilingMiss::Reversibility),
            (
                "no ledger",
                |r| {
                    let args = r#"{"path":"a.txt","contents":"x"}"#;
                    hx::note_proactive(&r.root, &hx::proactive_span("write", args, "approve", "", AccessMode::Full));
                    AutoCandidate { tool: "write".into(), arguments: args.into(), ..turn_off("notes") }
                },
                CeilingMiss::Reversibility,
            ),
        ];
        for (label, setup, miss) in cases {
            let mut r = rig(&format!("dfy-miss-{}", label.replace(' ', "-")), &["notes"]);
            let before = mcp_bytes();
            let c = setup(&mut r);
            let tool = c.tool.clone();
            r.cabin.auto_act.queue.push_back(c);
            r.cabin.tick_auto_act();
            assert_eq!(last_ask(&r.root), miss.as_str(), "{label}");
            assert!(done_cards(&r.cabin).is_empty(), "{label}");
            assert_eq!(mcp_bytes(), before, "{label}: nothing ran");
            assert!(proactive_spans(&r.root).iter().all(|s| s.decision != "auto"), "{label}");
            let route = match miss {
                CeilingMiss::PMind | CeilingMiss::NoHistory => pro::ProactiveRoute::Suggest,
                _ => pro::ProactiveRoute::ICan,
            };
            if miss == CeilingMiss::QuietHours {
                assert!(offer_cards(&r.cabin).is_empty(), "quiet hours hold the card");
                assert_eq!(r.cabin.proactive.queue.len(), 1, "it waits in 6a's quiet queue");
            } else {
                assert_eq!(offer_cards(&r.cabin), vec![(tool, route)], "{label}");
            }
        }
    }

    fn turn_off_with(r: &mut Rig) -> AutoCandidate {
        approve_history(&r.root, 1);
        turn_off("notes")
    }

    /// The sixth auto-act in one day does not run.
    #[test]
    fn the_sixth_auto_act_in_a_day_waits_for_tomorrow() {
        let _g = crate::config::hold_test_config();
        let names = ["n1", "n2", "n3", "n4", "n5", "n6"];
        let mut r = rig("dfy-sixth", &names);
        approve_history(&r.root, 1);
        for name in names {
            r.cabin.auto_act.queue.push_back(turn_off(name));
            r.cabin.tick_auto_act();
        }
        assert_eq!(done_cards(&r.cabin).len(), 5);
        assert_eq!(last_ask(&r.root), "budget");
        assert_eq!(offer_cards(&r.cabin), vec![("connection_disable".to_string(), pro::ProactiveRoute::ICan)]);
        let offer = r.cabin.updates.iter().find(|c| c.pulse.proactive.is_some()).expect("offer card");
        assert_eq!(grokhub_core::pulse::i_can_title(offer), "I can turn off the n6 connection");
        let text = String::from_utf8(mcp_bytes()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["mcpServers"]["n6"], serde_json::json!({ "command": "n6-mcp" }), "the sixth never ran");
    }

    /// "Don't do this again" sets p_mind 1.0; the next same-class candidate
    /// goes to an ask card instead of acting.
    #[test]
    fn dont_do_this_again_turns_the_next_one_into_an_ask() {
        let _g = crate::config::hold_test_config();
        let mut r = rig("dfy-never", &["notes", "calendar"]);
        approve_history(&r.root, 1);
        r.cabin.auto_act.queue.push_back(turn_off("notes"));
        r.cabin.tick_auto_act();
        let id = done_cards(&r.cabin)[0].id.clone();
        r.cabin.done_for_you_never(&id);
        assert_eq!(r.cabin.status, "GrokHub won't do or suggest this again.");
        let mind = hx::proactive_mind(&r.root, Arc::new(hx::SystemClock));
        assert_eq!(mind.mind_prior("proactive:connection_disable"), 1.0);
        let never = proactive_spans(&r.root).into_iter().rfind(|s| s.decision == "never").expect("never span");
        assert_eq!(never.args_redacted, "{}", "no step arguments on the answer span");

        let before = mcp_bytes();
        approve_history(&r.root, 40);
        r.cabin.auto_act.queue.push_back(turn_off("calendar"));
        r.cabin.tick_auto_act();
        assert_eq!(last_ask(&r.root), "p_mind");
        assert!(offer_cards(&r.cabin).is_empty(), "a class you said never to is not suggested either");
        assert_eq!(done_cards(&r.cabin).len(), 1);
        assert_eq!(mcp_bytes(), before);
    }

    /// Acceptance 2 and the 100-approvals rule: "reply to Sam's email" is a
    /// prepared draft on a hard Send card under Always + Full, never sent.
    #[test]
    fn reply_to_sam_is_a_prepared_draft_on_a_hard_card_even_under_always_and_full() {
        let _g = crate::config::hold_test_config();
        let mut r = rig("dfy-sam", &["notes"]);
        r.cabin.permission_mode = PermissionMode::AlwaysApprove;
        approve_history(&r.root, 100);
        r.cabin.auto_act.queue.push_back(reply_to_sam());
        r.cabin.tick_auto_act();

        let park = r.cabin.harness.park.clone().expect("hard card");
        assert_eq!(park.class, HardClass::Send);
        assert_eq!(park.tool, "mail_send");
        assert_eq!(park.action, "Reply to Sam: Thanks Sam, Thursday at 3 works.");
        assert_eq!(park.path, "E");
        assert!(matches!(park.source, harness_ui::ParkSource::AutoPrepared(_)));
        assert!(done_cards(&r.cabin).is_empty());
        assert!(offer_cards(&r.cabin).is_empty());
        let spans = proactive_spans(&r.root);
        let parked = spans.iter().rfind(|s| s.decision == "park").expect("park span");
        assert_eq!(parked.approval_class, "send");
        assert_eq!(parked.origin, hx::Origin::Proactive);
        assert!(spans.iter().all(|s| s.decision != "auto"));
        assert_eq!(hx::hard_card_key(true, false, false), None, "Enter never approves");
    }

    /// Acceptance 3: an unattended heartbeat with a hard candidate only parks;
    /// past the TTL the next tick denies it with a span.
    #[test]
    fn an_unattended_hard_candidate_parks_and_times_out_to_a_deny_span() {
        let _g = crate::config::hold_test_config();
        let mut r = rig("dfy-ttl", &["notes"]);
        approve_history(&r.root, 1);
        r.cabin.auto_act.queue.push_back(reply_to_sam());
        r.cabin.tick_anticipate();
        assert!(r.cabin.harness.park.is_some());
        if let Some(p) = r.cabin.harness.park.as_mut() {
            p.parked_at = Instant::now() - hx::APPROVAL_TTL - Duration::from_secs(1);
        }
        r.cabin.tick_anticipate();
        assert!(r.cabin.harness.park.is_none());
        let deny = proactive_spans(&r.root).into_iter().rfind(|s| s.decision == "deny").expect("deny span");
        assert_eq!(deny.result, "timed out — fail-closed Deny");
        assert_eq!(deny.approval_class, "send");
        assert_eq!(deny.origin, hx::Origin::Proactive);
        assert_eq!(deny.path, "E");
    }
}
