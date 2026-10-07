//! Spike-6b: one admitted auto-act, run through path E.
//!
//! The cabin builds a [`grokhub_core::AutoAct`] (the autonomy ceiling), then
//! [`run_auto_act`] asks [`decide`] with [`Step::Proactive`] (hard class
//! first, then MindCheck). Only an `Allow` runs, through the native tool
//! dispatch every path-E step uses, with origin `proactive`. No new
//! executor, no side path. A step must name a change-ledger target before
//! it runs (no ledger, no auto), and the span links the ledger line it
//! wrote (`undo_ref`) so Undo and the "why" can find it.
//!
//! The proactive span log (`spans/proactive.jsonl`) is also where MindCheck
//! reads this class back: approvals, Done-for-you Undos (`undo`) and
//! "Don't do this again" (`never`).

use std::path::Path;
use std::sync::Arc;

use grokhub_core::AutoAct;

use crate::harness::access::AccessMode;
use crate::harness::approval::{decide, GateOutcome, Step};
use crate::harness::changes::{entry_id, Change, ChangeKind, ChangeLedger};
use crate::harness::hard::{classify, HardHit};
use crate::harness::mindcheck::{mind_key, signals_from_spans, Clock, MindCheck};
use crate::harness::span::{append_span, read_spans, Origin, Span};

/// Span session for proactive steps and their answers.
pub const PROACTIVE_TRACE: &str = "proactive";
/// Decision on a span for a step GrokHub did on its own.
pub const DECISION_AUTO: &str = "auto";
/// Decision on a span for a candidate the ceiling or the gate sent to a card.
pub const DECISION_ASK: &str = "ask";
/// Undo on a Done-for-you card.
pub const DECISION_UNDO: &str = "undo";
/// "Don't do this again" on a Done-for-you card.
pub const DECISION_NEVER: &str = "never";

/// A proactive span for `tool` (path E, origin proactive).
pub fn proactive_span(tool: &str, arguments: &str, decision: &str, claim: &str, access: AccessMode) -> Span {
    let mut span = Span::soft_allow(PROACTIVE_TRACE, tool, arguments, "", claim, access, "native")
        .on_path("E")
        .from_origin(Origin::Proactive);
    span.decision = decision.into();
    span
}

/// The MindCheck key a proactive step is read back under. Built from the
/// span it would write, so the key and the log always agree.
pub fn proactive_key(tool: &str, arguments: &str) -> String {
    mind_key(&proactive_span(tool, arguments, DECISION_AUTO, "", AccessMode::Readonly))
}

/// The span for the user's answer on a Done-for-you card (`undo`, `never`),
/// built back from the MindCheck key so it folds into the same prior. Holds
/// the tool name and app only, never the step's arguments.
pub fn answer_span(key: &str, decision: &str, access: AccessMode) -> Span {
    let rest = key.strip_prefix("proactive:").unwrap_or(key);
    let (tool, args) = match rest.split_once('@') {
        Some((tool, app)) => (tool, serde_json::json!({ "app": app }).to_string()),
        None => (rest, "{}".to_string()),
    };
    proactive_span(tool, &args, decision, "user answer on a Done-for-you card", access)
}

/// The harness class of a step: `send`, `delete`, …, `floor`, or `None`
/// when it is soft. Same `classify` the gate runs.
pub fn step_class(tool: &str, arguments: &str) -> Option<String> {
    match classify(tool, arguments) {
        HardHit::None => None,
        HardHit::Class(class) => Some(class.as_str().into()),
        HardHit::Floor(_) => Some("floor".into()),
    }
}

/// The change-ledger target a step writes, if it writes through one.
/// Only these steps can be undone in one click, so only these may auto-act.
pub fn ledger_target(tool: &str, arguments: &str) -> Option<(ChangeKind, String)> {
    let leaf = tool.rsplit("__").next().unwrap_or(tool);
    if !matches!(leaf, "connection_disable" | "connection_add") {
        return None;
    }
    let args: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let name = args.get("name").and_then(|v| v.as_str())?;
    entry_id(name).ok().map(|id| (ChangeKind::Connection, id))
}

/// MindCheck over the proactive span log.
pub fn proactive_mind(config_dir: &Path, clock: Arc<dyn Clock>) -> MindCheck {
    let mut mind = MindCheck::new(clock);
    mind.extend(signals_from_spans(&read_spans(config_dir, PROACTIVE_TRACE).unwrap_or_default()));
    mind
}

/// Write one proactive span.
pub fn note_proactive(config_dir: &Path, span: &Span) {
    let _ = append_span(config_dir, span);
}

/// What one auto-act did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoRun {
    /// It ran and wrote this ledger line.
    Done { kind: ChangeKind, change: Box<Change> },
    /// The gate did not allow it: a hard park or a MindCheck ask.
    Asked(GateOutcome),
    /// No ledger target: it can't be undone, so it doesn't run.
    NoUndo,
    /// The tool ran and failed, or changed nothing.
    Failed(String),
}

/// Run one admitted auto-act: `decide` first, then the native dispatch, then
/// the span with its `undo_ref`.
pub fn run_auto_act(config_dir: &Path, workspace: &Path, act: &AutoAct, mind: &MindCheck, access: AccessMode) -> AutoRun {
    let c = act.candidate();
    let Some((kind, id)) = ledger_target(&c.tool, &c.arguments) else {
        return AutoRun::NoUndo;
    };
    let outcome = decide(Step::Proactive { name: &c.tool, arguments: &c.arguments, key: &c.key, mind });
    if !outcome.is_allow() {
        let mut span = proactive_span(&c.tool, &c.arguments, DECISION_ASK, "gate asked", access);
        if let GateOutcome::Park { hard: Some(class), .. } = &outcome {
            span.approval_class = class.as_str().into();
        }
        note_proactive(config_dir, &span);
        return AutoRun::Asked(outcome);
    }
    let last_seq = ChangeLedger::load_kind(config_dir, kind).all().last().map(|c| c.seq);
    let stop = || false;
    let ctx = crate::tools::ToolCtx { workspace, desktop: None, stop: &stop, tasks: None, owner: None };
    let out = crate::tools::dispatch(&ctx, &c.tool, &c.arguments);
    let mut span = proactive_span(&c.tool, &c.arguments, DECISION_AUTO, &c.summary, access);
    let wrote = ChangeLedger::load_kind(config_dir, kind)
        .all()
        .iter()
        .rev()
        .find(|line| line.id == id && Some(line.seq) > last_seq)
        .cloned();
    let change = match (out.failed, wrote) {
        (false, Some(change)) => change,
        (failed, _) => {
            span.result = if failed { "failed".into() } else { "no change".into() };
            note_proactive(config_dir, &span);
            return AutoRun::Failed(grokhub_core::redact_secrets(&out.text));
        }
    };
    span.result = "done".into();
    span.undo_ref = format!("{}:{}", kind.as_str(), change.seq);
    note_proactive(config_dir, &span);
    AutoRun::Done { kind, change: Box::new(change) }
}

/// A parked proactive hard card the user approved: the step runs once
/// through the same native dispatch. Returns the tool's text and whether it failed.
pub fn run_approved_once(workspace: &Path, tool: &str, arguments: &str) -> (String, bool) {
    let stop = || false;
    let ctx = crate::tools::ToolCtx { workspace, desktop: None, stop: &stop, tasks: None, owner: None };
    let out = crate::tools::dispatch(&ctx, tool, arguments);
    (out.text, out.failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::mindcheck::SystemClock;
    use crate::harness::HardClass;
    use grokhub_core::{AccessTier, AutoCandidate, CeilingCtx, PillMode, AUTO_PER_DAY};

    fn candidate(tool: &str, args: &str) -> AutoCandidate {
        AutoCandidate {
            key: proactive_key(tool, args),
            tool: tool.into(),
            arguments: args.into(),
            hard: step_class(tool, args),
            scope: Some("system_state".into()),
            desktop: false,
            value: 0.8,
            confidence: 0.9,
            reversibility: 1.0,
            summary: "Turned off the notes connection".into(),
            why: String::new(),
        }
    }

    fn open() -> CeilingCtx {
        CeilingCtx {
            in_scope: true,
            access: AccessTier::Full,
            pill: PillMode::Always,
            p_mind: Some(0.0),
            mind_asks: false,
            quiet: false,
            busy: false,
            halted: false,
            budget_left: AUTO_PER_DAY,
        }
    }

    #[test]
    fn keys_and_classes_come_from_the_same_span_and_classify() {
        assert_eq!(proactive_key("connection_disable", r#"{"name":"notes"}"#), "proactive:connection_disable");
        assert_eq!(step_class("connection_disable", r#"{"name":"notes"}"#), None);
        assert_eq!(step_class("mail_send", r#"{"to":"sam@example.com"}"#).as_deref(), Some("send"));
        assert_eq!(
            ledger_target("connection_disable", r#"{"name":"notes"}"#),
            Some((ChangeKind::Connection, "notes".into()))
        );
        for key in ["proactive:connection_disable", "proactive:type@browser"] {
            assert_eq!(mind_key(&answer_span(key, DECISION_NEVER, AccessMode::Full)), key);
        }
        assert_eq!(ledger_target("write", r#"{"path":"a.txt"}"#), None);
        assert_eq!(ledger_target("click", r#"{"x":1,"y":2}"#), None);
    }

    #[test]
    fn a_hundred_soft_approvals_never_let_a_hard_send_auto_act() {
        let dir = crate::harness::test_dir("proactive-hundred");
        let soft_args = r#"{"name":"notes"}"#;
        for _ in 0..100 {
            note_proactive(&dir, &proactive_span("connection_disable", soft_args, "approve", "", AccessMode::Full));
        }
        let mind = proactive_mind(&dir, Arc::new(SystemClock));
        assert_eq!(mind.mind_prior("proactive:connection_disable"), 0.0);
        let send = candidate("mail_send", r#"{"to":"sam@example.com","body":"See you at 3"}"#);
        let ctx = CeilingCtx { p_mind: Some(mind.mind_prior(&send.key)), ..open() };
        assert_eq!(AutoAct::admit(send.clone(), &ctx), Err(grokhub_core::CeilingMiss::HardClass));
        assert_eq!(
            decide(Step::Proactive { name: &send.tool, arguments: &send.arguments, key: &send.key, mind: &mind }),
            GateOutcome::Park {
                reason: "hard-class send: Send — Always cannot skip".into(),
                hard: Some(HardClass::Send),
                needs_jeremy: true,
            }
        );
    }

    #[test]
    fn undo_and_never_spans_raise_the_prior() {
        let dir = crate::harness::test_dir("proactive-never");
        let args = r#"{"name":"notes"}"#;
        note_proactive(&dir, &proactive_span("connection_disable", args, "approve", "", AccessMode::Full));
        assert_eq!(proactive_mind(&dir, Arc::new(SystemClock)).mind_prior("proactive:connection_disable"), 0.0);
        note_proactive(&dir, &proactive_span("connection_disable", args, DECISION_UNDO, "", AccessMode::Full));
        assert_eq!(proactive_mind(&dir, Arc::new(SystemClock)).mind_prior("proactive:connection_disable"), 0.6);
        note_proactive(&dir, &proactive_span("connection_disable", args, DECISION_NEVER, "", AccessMode::Full));
        let mind = proactive_mind(&dir, Arc::new(SystemClock));
        assert_eq!(mind.mind_prior("proactive:connection_disable"), 1.0);
        for _ in 0..30 {
            note_proactive(&dir, &proactive_span("connection_disable", args, "approve", "", AccessMode::Full));
        }
        let later = proactive_mind(&dir, Arc::new(SystemClock));
        assert_eq!(later.mind_prior("proactive:connection_disable"), 1.0);
        let c = candidate("connection_disable", args);
        assert_eq!(
            later.mind_route(&crate::harness::Candidate { key: &c.key, hard: None }),
            crate::harness::MindRoute::Ask,
            "never stays ask-first however many approvals follow"
        );
    }
}
