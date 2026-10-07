//! Spike-5a MindCheck: "if unsure whether you'd be upset, ask".
//!
//! A prior per action class plus context (`proactive:tidy_downloads`,
//! `desktop:type@browser`), folded from the span log, the change ledger and
//! card signals in time order:
//! - a deny or an Undo sets `p_mind = max(p_mind, 0.6)` and asks first for 30 days;
//! - each later approve lowers it by 0.05, floor 0;
//! - a Pulse Dismiss or "Not this" raises it by 0.1, cap 1.
//!
//! `p_mind >= 0.2`, an open ask-first window, or no history at all routes to
//! [`MindRoute::Ask`] (a soft card), never auto. Priors never touch hard
//! class: hard spans are not read, and a hard candidate is always Ask (the
//! gate parks it anyway). MindCheck is a soft-path input to
//! [`decide`](crate::harness::decide) through [`Step::Proactive`](crate::harness::Step),
//! never a second gate. Nothing auto-acts on it yet (Spike-6 will).

use std::sync::Arc;

use grokhub_core::amr::{line_id, AmrError, AmrStore, NodeDraft, NodeType, Noted, Sensitivity};
use grokhub_core::card_signals::{CardEvent, CardSignal};

use crate::harness::approval::{decide, GateOutcome, Step};
use crate::harness::changes::{Change, ChangeOp};
use crate::harness::hard::HardClass;
use crate::harness::span::Span;

/// Prior after a deny or an Undo, in hundredths.
pub const MIND_DENY: u32 = 60;
/// Each later approve with no undo lowers the prior by this, in hundredths.
pub const MIND_APPROVE_STEP: u32 = 5;
/// A Pulse Dismiss or "Not this" raises the prior by this, in hundredths.
pub const MIND_DISMISS_STEP: u32 = 10;
/// At or over this (hundredths) the candidate asks.
pub const MIND_ASK_AT: u32 = 20;
/// How long a deny or Undo means ask-first, whatever the prior.
pub const MIND_ASK_FIRST_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// The skill-patch class an Undo in `changes/skills.jsonl` counts against.
pub const SKILL_CHANGE_KEY: &str = "self_manage:skill_change";
/// Tool name an agent-started forget is checked as: `delete` makes it hard
/// class Delete, so it parks a card no prior or Always can skip.
pub const AGENT_FORGET_TOOL: &str = "memory_delete";

/// What happened, as MindCheck reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MindEvent {
    /// A deny, a TTL deny, or a Halt.
    Deny,
    /// The user undid what the action did.
    Undo,
    /// The user approved the action.
    Approve,
    /// A Pulse Dismiss or "Not this".
    Dismiss,
}

/// One event for one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MindSignal {
    pub key: String,
    pub at_ms: u64,
    pub event: MindEvent,
    /// `span:<session>:<ms>`, `ledger:<seq>` or `pulse:card:<id>`: the "why".
    pub source: String,
}

/// The clock MindCheck reads. Tests inject a fixed one.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// Wall clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// One key's folded state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Prior {
    /// `p_mind` in hundredths (0..=100).
    pub hundredths: u32,
    /// Ask first until this time.
    pub ask_until_ms: u64,
    /// Events folded in.
    pub events: usize,
}

impl Prior {
    pub fn p_mind(&self) -> f64 {
        f64::from(self.hundredths) / 100.0
    }
}

/// A candidate action. `hard` is the gate's class for it, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate<'a> {
    pub key: &'a str,
    pub hard: Option<HardClass>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MindRoute {
    /// Show a soft card first.
    Ask,
    /// The prior allows acting without a card (Spike-6 decides whether to).
    MayAuto,
}

/// Priors per key over an injected clock.
pub struct MindCheck {
    signals: Vec<MindSignal>,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for MindCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MindCheck").field("signals", &self.signals.len()).finish()
    }
}

impl MindCheck {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { signals: Vec::new(), clock }
    }

    pub fn record(&mut self, signal: MindSignal) {
        self.signals.push(signal);
    }

    pub fn extend(&mut self, signals: impl IntoIterator<Item = MindSignal>) {
        self.signals.extend(signals);
    }

    /// The folded prior for `key` as of the clock, or `None` with no history.
    pub fn prior(&self, key: &str) -> Option<Prior> {
        let now = self.clock.now_ms();
        let mut mine: Vec<&MindSignal> = self.signals.iter().filter(|s| s.key == key && s.at_ms <= now).collect();
        if mine.is_empty() {
            return None;
        }
        mine.sort_by_key(|s| s.at_ms);
        let mut prior = Prior::default();
        for signal in mine {
            prior.events += 1;
            match signal.event {
                MindEvent::Deny | MindEvent::Undo => {
                    prior.hundredths = prior.hundredths.max(MIND_DENY);
                    prior.ask_until_ms = prior.ask_until_ms.max(signal.at_ms.saturating_add(MIND_ASK_FIRST_MS));
                }
                MindEvent::Approve => prior.hundredths = prior.hundredths.saturating_sub(MIND_APPROVE_STEP),
                MindEvent::Dismiss => prior.hundredths = (prior.hundredths + MIND_DISMISS_STEP).min(100),
            }
        }
        Some(prior)
    }

    /// `p_mind` for `key`; 0 with no history (which still routes to Ask).
    pub fn mind_prior(&self, key: &str) -> f64 {
        self.prior(key).map(|p| p.p_mind()).unwrap_or(0.0)
    }

    /// Ask or MayAuto. Hard class, no history, an open ask-first window, or
    /// `p_mind >= 0.2` all ask.
    pub fn mind_route(&self, candidate: &Candidate<'_>) -> MindRoute {
        if candidate.hard.is_some() {
            return MindRoute::Ask;
        }
        let Some(prior) = self.prior(candidate.key) else {
            return MindRoute::Ask;
        };
        if self.clock.now_ms() < prior.ask_until_ms || prior.hundredths >= MIND_ASK_AT {
            return MindRoute::Ask;
        }
        MindRoute::MayAuto
    }
}

/// The action class plus context for a span: the origin for anything the
/// user didn't start (`proactive:tidy_downloads`), `desktop:` on path A, else
/// `tool:`. An `app` argument adds `@<app>` (`desktop:type@browser`).
pub fn mind_key(span: &Span) -> String {
    let class = match (span.origin.as_str(), span.path.as_str()) {
        ("user", "A") => "desktop",
        ("user", _) => "tool",
        (origin, _) => origin,
    };
    let tool = span.tool.rsplit("__").next().unwrap_or(&span.tool);
    let app = serde_json::from_str::<serde_json::Value>(&span.args_redacted)
        .ok()
        .and_then(|v| v.get("app").and_then(|a| a.as_str()).map(|a| a.trim().to_ascii_lowercase()))
        .filter(|a| !a.is_empty());
    match app {
        Some(app) => format!("{class}:{tool}@{app}"),
        None => format!("{class}:{tool}"),
    }
}

/// Deny and approve spans as signals. Hard-class spans are skipped: priors
/// never touch hard class. A TTL deny and a Halt are deny spans too.
pub fn signals_from_spans(spans: &[Span]) -> Vec<MindSignal> {
    spans
        .iter()
        .filter(|s| HardClass::parse(&s.approval_class).is_none())
        .filter_map(|s| {
            let event = match s.decision.as_str() {
                "deny" => MindEvent::Deny,
                "approve" => MindEvent::Approve,
                _ => return None,
            };
            Some(MindSignal {
                key: mind_key(s),
                at_ms: s.ts_ms,
                event,
                source: format!("span:{}", s.span_ref()),
            })
        })
        .collect()
}

/// Undo lines in the change ledger, against [`SKILL_CHANGE_KEY`].
pub fn signals_from_changes(changes: &[Change]) -> Vec<MindSignal> {
    changes
        .iter()
        .filter(|c| c.op == ChangeOp::Undo)
        .map(|c| MindSignal {
            key: SKILL_CHANGE_KEY.into(),
            at_ms: c.at,
            event: MindEvent::Undo,
            source: format!("ledger:{}", c.seq),
        })
        .collect()
}

/// Dismissed and rejected cards, keyed `proactive:<group>`.
pub fn signals_from_cards(cards: &[CardSignal]) -> Vec<MindSignal> {
    cards
        .iter()
        .filter(|c| matches!(c.event, CardEvent::Dismissed | CardEvent::Rejected))
        .map(|c| MindSignal {
            key: format!("proactive:{}", c.group),
            at_ms: c.ts,
            event: MindEvent::Dismiss,
            source: format!("pulse:card:{}", c.card_id),
        })
        .collect()
}

/// A deny or an Undo becomes one `mind_prior` node per key ("ask first"),
/// so `/memory` shows it with its source. Other events write nothing.
pub fn note_prior(store: &AmrStore, signal: &MindSignal) -> Result<Noted, AmrError> {
    if !matches!(signal.event, MindEvent::Deny | MindEvent::Undo) {
        return Ok(Noted::Skipped("not a deny or undo"));
    }
    let stamp = grokhub_core::oauth::unix_ms_to_rfc3339(signal.at_ms);
    let text = format!("Ask first before {}.", signal.key);
    let draft = NodeDraft {
        id: line_id(&text),
        node_type: NodeType::MindPrior,
        created: stamp.clone(),
        updated: stamp,
        source: signal.source.clone(),
        confidence: MIND_DENY as f32 / 100.0,
        tags: vec!["mind_prior".into(), signal.key.clone()],
        body: format!("{text}\n"),
        sensitivity: Sensitivity::Plain,
        consent_ref: String::new(),
    };
    match store.remember(&draft) {
        Ok(id) => Ok(Noted::New(id.as_str().to_string())),
        Err(AmrError::DuplicateId(id)) => Ok(Noted::Known(id)),
        Err(err) => Err(err),
    }
}

/// Fold the signals in and write a prior node for each deny or Undo.
pub fn learn(store: &AmrStore, mind: &mut MindCheck, signals: Vec<MindSignal>) -> Result<Vec<Noted>, AmrError> {
    let mut out = Vec::new();
    for signal in &signals {
        out.push(note_prior(store, signal)?);
    }
    mind.extend(signals);
    Ok(out)
}

/// An agent-started forget of `count` learned notes: always a hard delete
/// card. The user's own Forget click never comes here.
pub fn agent_forget(count: usize) -> GateOutcome {
    let action = format!("forget {count} learned notes");
    decide(Step::Ask { title: AGENT_FORGET_TOOL, action: &action })
}

/// The soft-path verdict for a proactive candidate: [`decide`] with
/// [`Step::Proactive`].
pub(crate) fn proactive_outcome(name: &str, key: &str, mind: &MindCheck) -> GateOutcome {
    match mind.mind_route(&Candidate { key, hard: None }) {
        MindRoute::MayAuto => GateOutcome::Allow,
        MindRoute::Ask => GateOutcome::Park {
            reason: format!("ask first: {name} ({key}, p_mind {:.2})", mind.mind_prior(key)),
            hard: None,
            needs_jeremy: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::harness::span::Origin;
    use crate::harness::{hard_card_key, HardAnswer};

    const DAY: u64 = 24 * 60 * 60 * 1000;
    const T0: u64 = 1_791_374_400_000;

    struct TestClock(AtomicU64);

    impl Clock for TestClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn clock(at: u64) -> Arc<TestClock> {
        Arc::new(TestClock(AtomicU64::new(at)))
    }

    fn tidy(decision: &str, ts: u64) -> Span {
        let mut s = Span::deny("s1", "tidy_downloads", "{}", "denied", "soft").from_origin(Origin::Proactive);
        s.decision = decision.into();
        s.ts_ms = ts;
        s
    }

    #[test]
    fn one_deny_sets_the_prior_to_0_6_and_the_next_candidate_asks() {
        let c = clock(T0 + 1);
        let mut mind = MindCheck::new(c.clone());
        let signals = signals_from_spans(&[tidy("deny", T0)]);
        assert_eq!(
            signals,
            vec![MindSignal {
                key: "proactive:tidy_downloads".into(),
                at_ms: T0,
                event: MindEvent::Deny,
                source: format!("span:s1:{T0}"),
            }]
        );
        mind.extend(signals);
        assert_eq!(mind.mind_prior("proactive:tidy_downloads"), 0.6);
        let candidate = Candidate { key: "proactive:tidy_downloads", hard: None };
        assert_eq!(mind.mind_route(&candidate), MindRoute::Ask);
        assert_eq!(
            decide(Step::Proactive { name: "tidy_downloads", arguments: "{}", key: "proactive:tidy_downloads", mind: &mind }),
            GateOutcome::Park {
                reason: "ask first: tidy_downloads (proactive:tidy_downloads, p_mind 0.60)".into(),
                hard: None,
                needs_jeremy: false,
            }
        );
    }

    #[test]
    fn no_history_asks_and_approvals_lower_the_prior_to_a_floor_of_zero() {
        let c = clock(T0 + 40 * DAY);
        let mut mind = MindCheck::new(c.clone());
        assert_eq!(mind.mind_route(&Candidate { key: "proactive:new", hard: None }), MindRoute::Ask);
        assert_eq!(mind.mind_prior("proactive:new"), 0.0);
        let mut spans = vec![tidy("deny", T0)];
        spans.extend((1..=7).map(|i| tidy("approve", T0 + 31 * DAY + i)));
        mind.extend(signals_from_spans(&spans));
        // 0.60 - 7 * 0.05 = 0.25: still asks.
        assert_eq!(mind.mind_prior("proactive:tidy_downloads"), 0.25);
        let candidate = Candidate { key: "proactive:tidy_downloads", hard: None };
        assert_eq!(mind.mind_route(&candidate), MindRoute::Ask);
        mind.extend(signals_from_spans(&[tidy("approve", T0 + 32 * DAY)]));
        assert_eq!(mind.mind_prior("proactive:tidy_downloads"), 0.2);
        assert_eq!(mind.mind_route(&candidate), MindRoute::Ask, "0.2 still asks");
        mind.extend(signals_from_spans(&[tidy("approve", T0 + 33 * DAY)]));
        assert_eq!(mind.mind_prior("proactive:tidy_downloads"), 0.15);
        assert_eq!(mind.mind_route(&candidate), MindRoute::MayAuto);
        mind.extend((0..10).map(|i| MindSignal {
            key: "proactive:tidy_downloads".into(),
            at_ms: T0 + 34 * DAY + i,
            event: MindEvent::Approve,
            source: "span:s1:0".into(),
        }));
        assert_eq!(mind.mind_prior("proactive:tidy_downloads"), 0.0);
    }

    #[test]
    fn the_thirty_day_ask_first_window_expires_on_the_clock() {
        let c = clock(T0);
        let mut mind = MindCheck::new(c.clone());
        mind.extend(signals_from_spans(&[tidy("deny", T0)]));
        // Twelve approvals bring p_mind to 0, but the window still asks.
        mind.extend((1..=12).map(|i| MindSignal {
            key: "proactive:tidy_downloads".into(),
            at_ms: T0 + i,
            event: MindEvent::Approve,
            source: "span:s1:0".into(),
        }));
        let candidate = Candidate { key: "proactive:tidy_downloads", hard: None };
        c.0.store(T0 + 12, Ordering::SeqCst);
        assert_eq!(mind.mind_prior("proactive:tidy_downloads"), 0.0);
        assert_eq!(mind.mind_route(&candidate), MindRoute::Ask);
        c.0.store(T0 + MIND_ASK_FIRST_MS - 1, Ordering::SeqCst);
        assert_eq!(mind.mind_route(&candidate), MindRoute::Ask);
        c.0.store(T0 + MIND_ASK_FIRST_MS, Ordering::SeqCst);
        assert_eq!(mind.mind_route(&candidate), MindRoute::MayAuto);
        // A signal later than the clock is not read yet.
        mind.record(MindSignal {
            key: "proactive:tidy_downloads".into(),
            at_ms: T0 + MIND_ASK_FIRST_MS + 5,
            event: MindEvent::Undo,
            source: "ledger:4".into(),
        });
        assert_eq!(mind.mind_route(&candidate), MindRoute::MayAuto);
        c.0.store(T0 + MIND_ASK_FIRST_MS + 5, Ordering::SeqCst);
        assert_eq!(mind.mind_prior("proactive:tidy_downloads"), 0.6);
        assert_eq!(mind.mind_route(&candidate), MindRoute::Ask);
    }

    #[test]
    fn dismiss_raises_by_a_tenth_and_undo_and_cards_feed_the_prior() {
        let c = clock(T0 + 100);
        let mut mind = MindCheck::new(c.clone());
        let card = CardSignal {
            ts: T0,
            card_id: "c9".into(),
            kind: "automate_offer".into(),
            group: "offer:downloads".into(),
            source_id: String::new(),
            event: CardEvent::Dismissed,
            after_open: None,
        };
        let opened = CardSignal { event: CardEvent::Opened, ..card.clone() };
        mind.extend(signals_from_cards(&[card.clone(), opened, CardSignal { ts: T0 + 1, ..card }]));
        assert_eq!(mind.mind_prior("proactive:offer:downloads"), 0.2);
        let undo = Change {
            seq: 4,
            at: T0 + 2,
            kind: "skill".into(),
            id: "inbox".into(),
            op: ChangeOp::Undo,
            origin: crate::harness::span::Origin::User,
            reason: String::new(),
            before_hash: String::new(),
            after_hash: String::new(),
            undoes: Some(3),
        };
        let modify = Change { op: ChangeOp::Modify, undoes: None, ..undo.clone() };
        let signals = signals_from_changes(&[modify, undo]);
        assert_eq!(
            signals,
            vec![MindSignal { key: SKILL_CHANGE_KEY.into(), at_ms: T0 + 2, event: MindEvent::Undo, source: "ledger:4".into() }]
        );
        mind.extend(signals);
        assert_eq!(mind.mind_prior(SKILL_CHANGE_KEY), 0.6);
    }

    #[test]
    fn a_hundred_send_approvals_never_soften_the_hard_card() {
        let c = clock(T0 + 1_000);
        let mut mind = MindCheck::new(c.clone());
        let approvals: Vec<Span> = (0..100)
            .map(|i| {
                let mut s = Span::hard_approve("s1", "send_email", "{}", HardClass::Send).from_origin(Origin::Proactive);
                s.ts_ms = T0 + i;
                s
            })
            .collect();
        assert_eq!(signals_from_spans(&approvals), Vec::new(), "priors never read hard spans");
        mind.extend(signals_from_spans(&approvals));
        assert_eq!(mind.prior("proactive:send_email"), None);
        let candidate = Candidate { key: "proactive:send_email", hard: Some(HardClass::Send) };
        assert_eq!(mind.mind_route(&candidate), MindRoute::Ask);
        // Even a prior that says auto cannot reach a hard call.
        mind.extend((0..100).map(|i| MindSignal {
            key: "proactive:send_email".into(),
            at_ms: T0 - 40 * DAY + i,
            event: MindEvent::Approve,
            source: "span:s1:0".into(),
        }));
        assert_eq!(mind.mind_route(&Candidate { key: "proactive:send_email", hard: None }), MindRoute::MayAuto);
        assert_eq!(
            decide(Step::Proactive { name: "send_email", arguments: "{}", key: "proactive:send_email", mind: &mind }),
            GateOutcome::Park {
                reason: "hard-class send: Send — Always cannot skip".into(),
                hard: Some(HardClass::Send),
                needs_jeremy: true,
            }
        );
        assert_eq!(hard_card_key(true, false, false), None, "Enter never approves a hard card");
    }

    #[test]
    fn keys_carry_the_class_and_the_app() {
        let mut desk = Span::deny("s", "grokhub-desktop__type", r#"{"app":"Browser","text":"hi"}"#, "", "soft");
        desk.path = "A".into();
        assert_eq!(mind_key(&desk), "desktop:type@browser");
        let plain = Span::deny("s", "read_file", "{}", "", "soft");
        assert_eq!(mind_key(&plain), "tool:read_file");
        assert_eq!(mind_key(&tidy("deny", 1)), "proactive:tidy_downloads");
    }

    #[test]
    fn agent_forget_parks_a_hard_delete_card() {
        assert_eq!(
            agent_forget(3),
            GateOutcome::Park {
                reason: "hard-class delete: Delete — Always cannot skip".into(),
                hard: Some(HardClass::Delete),
                needs_jeremy: true,
            }
        );
        assert_eq!(hard_card_key(true, false, false), None);
        assert_eq!(hard_card_key(false, true, false), Some(HardAnswer::Deny));
    }

    #[test]
    fn a_deny_writes_one_mind_prior_node_with_its_span_source() {
        let dir = crate::harness::test_dir("mind-prior-node");
        let store = AmrStore::at(dir.join("amr"));
        store.init().unwrap();
        let c = clock(T0 + 10);
        let mut mind = MindCheck::new(c);
        let spans = [tidy("deny", T0), tidy("deny", T0 + 5), tidy("approve", T0 + 6)];
        let noted = learn(&store, &mut mind, signals_from_spans(&spans)).unwrap();
        let id = noted[0].id().unwrap().to_string();
        assert_eq!(noted, vec![Noted::New(id.clone()), Noted::Known(id.clone()), Noted::Skipped("not a deny or undo")]);
        assert_eq!(mind.mind_prior("proactive:tidy_downloads"), 0.55);
        let (rows, _) = grokhub_core::amr::memory_rows(&store);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].node_type, NodeType::MindPrior);
        assert_eq!(
            rows[0].line(),
            format!("- Ask first before proactive:tidy_downloads. · why: [from what you approved, denied or undid](span:s1:{T0})")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
