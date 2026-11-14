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

use grokhub_core::learn::Undo;
use grokhub_core::AutoAct;

use crate::harness::access::AccessMode;
use crate::harness::approval::{decide, GateOutcome, Step};
use crate::harness::changes::{change_id, entry_id, Change, ChangeKind, ChangeLedger};
use crate::harness::hard::{classify, HardHit};
use crate::harness::mindcheck::{mind_key, signals_from_spans, Clock, MindCheck};
use crate::harness::span::{append_span, read_spans, Origin, Span};

/// Span session for proactive steps and their answers.
pub const PROACTIVE_TRACE: &str = "proactive";
/// Decision on a span for a step GrokHub did on its own.
pub const DECISION_AUTO: &str = "auto";
/// Decision on a span for a candidate the ceiling or the gate sent to a card.
pub const DECISION_ASK: &str = "ask";
/// Decision on a span for a step the user ran from its card (Pause it).
/// MindCheck reads it as an approval of that kind of step.
pub const DECISION_APPROVE: &str = "approve";
/// Undo on a Done-for-you card.
pub const DECISION_UNDO: &str = "undo";
/// "Don't do this again" on a Done-for-you card.
pub const DECISION_NEVER: &str = "never";

/// A proactive span for `tool` (path E, origin proactive).
pub fn proactive_span(
    tool: &str,
    arguments: &str,
    decision: &str,
    claim: &str,
    access: AccessMode,
) -> Span {
    let mut span = Span::soft_allow(
        PROACTIVE_TRACE,
        tool,
        arguments,
        "",
        claim,
        access,
        "native",
    )
    .on_path("E")
    .from_origin(Origin::Proactive);
    span.decision = decision.into();
    span
}

/// The MindCheck key a proactive step is read back under. Built from the
/// span it would write, so the key and the log always agree.
pub fn proactive_key(tool: &str, arguments: &str) -> String {
    mind_key(&proactive_span(
        tool,
        arguments,
        DECISION_AUTO,
        "",
        AccessMode::Readonly,
    ))
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
    proactive_span(
        tool,
        &args,
        decision,
        "user answer on a Done-for-you card",
        access,
    )
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
/// Only these steps can be put back from the ledger, so only these may
/// auto-act. [`step_undo`] says which rung of the ceiling each one is on.
pub fn ledger_target(tool: &str, arguments: &str) -> Option<(ChangeKind, String)> {
    let (kind, field) = match crate::self_manage::self_tool(tool)? {
        "connection_disable" | "connection_add" => (ChangeKind::Connection, "name"),
        "skill_modify" => (ChangeKind::Skill, "name"),
        "automation_disable" => (ChangeKind::Automation, "id"),
        _ => return None,
    };
    let args: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let raw = args.get(field).and_then(|v| v.as_str())?;
    let name = raw.trim();
    match kind {
        // A skill's ledger id is its folder name; a name that is not already
        // one would write a different skill than the card names.
        ChangeKind::Skill => {
            (!name.is_empty() && change_id(name) == name).then(|| (kind, name.to_string()))
        }
        // The tool looks a job up by its exact id, so the ledger id must be
        // that id, not a trimmed copy of it.
        ChangeKind::Automation => entry_id(raw)
            .ok()
            .filter(|id| id == raw)
            .map(|id| (kind, id)),
        _ => entry_id(name).ok().map(|id| (kind, id)),
    }
}

/// How a step is taken back, for the autonomy ceiling. Switching a
/// connection or turning an automation back on is one click. A patch to a skill changes what the agent does
/// from then on, so it is a bigger change with the higher bar. Anything with
/// no ledger target can't be put back and never acts on its own.
pub fn step_undo(tool: &str, arguments: &str) -> Undo {
    match ledger_target(tool, arguments) {
        Some((ChangeKind::Connection | ChangeKind::Automation, _)) => Undo::OneClick,
        Some((ChangeKind::Skill, _)) => Undo::Restore,
        _ => Undo::Never,
    }
}

/// MindCheck over the proactive span log.
pub fn proactive_mind(config_dir: &Path, clock: Arc<dyn Clock>) -> MindCheck {
    let mut mind = MindCheck::new(clock);
    mind.extend(signals_from_spans(
        &read_spans(config_dir, PROACTIVE_TRACE).unwrap_or_default(),
    ));
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
    Done {
        kind: ChangeKind,
        change: Box<Change>,
    },
    /// The gate did not allow it: a hard park or a MindCheck ask.
    Asked(GateOutcome),
    /// No ledger target: it can't be undone, so it doesn't run.
    NoUndo,
    /// The tool ran and failed, or changed nothing.
    Failed(String),
}

/// Run one admitted auto-act: `decide` first, then the native dispatch, then
/// the span with its `undo_ref`.
pub fn run_auto_act(
    config_dir: &Path,
    workspace: &Path,
    act: &AutoAct,
    mind: &MindCheck,
    access: AccessMode,
) -> AutoRun {
    let c = act.candidate();
    let Some((kind, id)) = ledger_target(&c.tool, &c.arguments) else {
        return AutoRun::NoUndo;
    };
    let outcome = decide(Step::Proactive {
        name: &c.tool,
        arguments: &c.arguments,
        key: &c.key,
        mind,
    });
    if !outcome.is_allow() {
        let mut span = proactive_span(&c.tool, &c.arguments, DECISION_ASK, "gate asked", access);
        if let GateOutcome::Park {
            hard: Some(class), ..
        } = &outcome
        {
            span.approval_class = class.as_str().into();
        }
        note_proactive(config_dir, &span);
        return AutoRun::Asked(outcome);
    }
    let ran = dispatch_ledgered(config_dir, workspace, &c.tool, &c.arguments, kind, &id);
    let mut span = proactive_span(&c.tool, &c.arguments, DECISION_AUTO, &c.summary, access);
    let change = match ran {
        Ok(change) => change,
        Err(Ran { failed, text }) => {
            span.result = if failed {
                "failed".into()
            } else {
                "no change".into()
            };
            note_proactive(config_dir, &span);
            return AutoRun::Failed(text);
        }
    };
    span.result = "done".into();
    span.undo_ref = format!("{}:{}", kind.as_str(), change.seq);
    note_proactive(config_dir, &span);
    AutoRun::Done {
        kind,
        change: Box::new(change),
    }
}

/// A step the user ran from its card (Pause it): the same native dispatch
/// as an auto-act, with no gate in between, since the tap is the approval.
/// A hard-class step is refused before anything runs, and a step with no
/// ledger target never runs. Only a step that wrote a new ledger line counts:
/// it gets one `approve` span linking that line, so MindCheck learns from
/// it. Anything else writes no span.
pub fn run_offered_step(
    config_dir: &Path,
    workspace: &Path,
    tool: &str,
    arguments: &str,
    access: AccessMode,
) -> AutoRun {
    let Some((kind, id)) = ledger_target(tool, arguments) else {
        return AutoRun::NoUndo;
    };
    if step_class(tool, arguments).is_some() {
        return AutoRun::Failed("That step always asks.".into());
    }
    let change = match dispatch_ledgered(config_dir, workspace, tool, arguments, kind, &id) {
        Ok(change) => change,
        Err(Ran { text, .. }) => return AutoRun::Failed(text),
    };
    let mut span = proactive_span(
        tool,
        arguments,
        DECISION_APPROVE,
        "ran from its card",
        access,
    );
    span.result = "done".into();
    span.undo_ref = format!("{}:{}", kind.as_str(), change.seq);
    note_proactive(config_dir, &span);
    AutoRun::Done {
        kind,
        change: Box::new(change),
    }
}

/// Why a dispatched step did not count: the tool failed, or it changed
/// nothing on the ledger. `text` is the tool's reply with secrets redacted.
struct Ran {
    failed: bool,
    text: String,
}

/// Run `tool` through the native dispatch and return the new ledger line it
/// wrote on `id`, if any.
fn dispatch_ledgered(
    config_dir: &Path,
    workspace: &Path,
    tool: &str,
    arguments: &str,
    kind: ChangeKind,
    id: &str,
) -> Result<Change, Ran> {
    let last_seq = ChangeLedger::load_kind(config_dir, kind)
        .all()
        .last()
        .map(|c| c.seq);
    let stop = || false;
    let ctx = crate::tools::ToolCtx {
        workspace,
        desktop: None,
        stop: &stop,
        tasks: None,
        owner: None,
        session: None,
    };
    let out = crate::tools::dispatch(&ctx, tool, arguments);
    let wrote = ChangeLedger::load_kind(config_dir, kind)
        .all()
        .iter()
        .rev()
        .find(|line| line.id == id && Some(line.seq) > last_seq)
        .cloned();
    match (out.failed, wrote) {
        (false, Some(change)) => Ok(change),
        (failed, _) => Err(Ran {
            failed,
            text: grokhub_core::redact_secrets(&out.text),
        }),
    }
}

/// A parked proactive hard card the user approved: the step runs once
/// through the same native dispatch. Returns the tool's text and whether it failed.
pub fn run_approved_once(workspace: &Path, tool: &str, arguments: &str) -> (String, bool) {
    let stop = || false;
    let ctx = crate::tools::ToolCtx {
        workspace,
        desktop: None,
        stop: &stop,
        tasks: None,
        owner: None,
        session: None,
    };
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
            undo: step_undo(tool, args),
            evidence: 0.0,
            summary: "Turned off the notes connection".into(),
            offer: "turn off the notes connection".into(),
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
            restore_left: grokhub_core::AUTO_RESTORE_PER_DAY,
        }
    }

    #[test]
    fn keys_and_classes_come_from_the_same_span_and_classify() {
        assert_eq!(
            proactive_key("connection_disable", r#"{"name":"notes"}"#),
            "proactive:connection_disable"
        );
        assert_eq!(
            step_class("connection_disable", r#"{"name":"notes"}"#),
            None
        );
        assert_eq!(
            step_class("mail_send", r#"{"to":"sam@example.com"}"#).as_deref(),
            Some("send")
        );
        assert_eq!(
            ledger_target("connection_disable", r#"{"name":"notes"}"#),
            Some((ChangeKind::Connection, "notes".into()))
        );
        for key in ["proactive:connection_disable", "proactive:type@browser"] {
            assert_eq!(
                mind_key(&answer_span(key, DECISION_NEVER, AccessMode::Full)),
                key
            );
        }
        assert_eq!(ledger_target("write", r#"{"path":"a.txt"}"#), None);
        assert_eq!(ledger_target("click", r#"{"x":1,"y":2}"#), None);
    }

    #[test]
    fn another_servers_tool_of_the_same_name_has_no_ledger_target() {
        let foo = r#"{"name":"foo"}"#;
        let notes = r#"{"name":"notes"}"#;
        assert_eq!(ledger_target("mcp__x__skill_modify", foo), None);
        assert_eq!(ledger_target("notes__connection_add", notes), None);
        assert_eq!(step_undo("notes__connection_disable", notes), Undo::Never);
        assert_eq!(
            ledger_target("grokhub-self__skill_modify", foo),
            Some((ChangeKind::Skill, "foo".into()))
        );
        assert_eq!(
            step_undo("grokhub-self__connection_disable", notes),
            Undo::OneClick
        );
    }

    #[test]
    fn the_harness_names_the_rung_from_the_tool() {
        let notes = r#"{"name":"notes"}"#;
        assert_eq!(step_undo("connection_disable", notes), Undo::OneClick);
        assert_eq!(step_undo("connection_add", notes), Undo::OneClick);
        let patch = r#"{"name":"patch-notes","instructions":"Check the date first."}"#;
        assert_eq!(step_class("skill_modify", patch), None);
        assert_eq!(
            ledger_target("skill_modify", patch),
            Some((ChangeKind::Skill, "patch-notes".into()))
        );
        assert_eq!(step_undo("skill_modify", patch), Undo::Restore);
        // A name that is not the skill's own folder name has no target.
        assert_eq!(
            ledger_target("skill_modify", r#"{"name":"Patch Notes"}"#),
            None
        );
        assert_eq!(step_undo("skill_modify", r#"{"name":"../x"}"#), Undo::Never);
        assert_eq!(step_undo("skill_modify", "{}"), Undo::Never);
        // Deleting, creating and anything off the ledger stay off both rungs.
        for tool in [
            "skill_delete",
            "skill_create",
            "automation_create",
            "write",
            "mail_send",
        ] {
            assert_eq!(step_undo(tool, patch), Undo::Never, "{tool}");
        }
    }

    #[test]
    fn pausing_an_automation_is_a_one_click_ledger_step_and_nothing_else_moves() {
        let pause = r#"{"id":"auto-abc","reason":"x"}"#;
        assert_eq!(
            ledger_target("automation_disable", pause),
            Some((ChangeKind::Automation, "auto-abc".into()))
        );
        assert_eq!(step_undo("automation_disable", pause), Undo::OneClick);
        assert_eq!(step_class("automation_disable", pause), None);
        assert_eq!(
            proactive_key("automation_disable", pause),
            "proactive:automation_disable"
        );
        // An id the tool would not find as written has no target.
        assert_eq!(
            ledger_target("automation_disable", r#"{"id":" auto-abc"}"#),
            None
        );
        assert_eq!(
            ledger_target("automation_disable", r#"{"id":"../x"}"#),
            None
        );
        assert_eq!(
            ledger_target("automation_disable", r#"{"name":"auto-abc"}"#),
            None
        );
        for tool in ["automation_delete", "automation_modify", "skill_disable"] {
            assert_eq!(ledger_target(tool, pause), None, "{tool}");
            assert_eq!(step_undo(tool, pause), Undo::Never, "{tool}");
        }
        assert_eq!(
            ledger_target("connection_disable", r#"{"name":"notes"}"#),
            Some((ChangeKind::Connection, "notes".into()))
        );
    }

    fn write_automation(dir: &Path, id: &str, enabled: bool) {
        let row = serde_json::json!([{
            "id": id,
            "name": "Sync notes",
            "schedule": "daily",
            "time": "09:00",
            "instructions": "sync my notes",
            "enabled": enabled,
            "health": {"outcome": "failed", "error": "disk full", "failStreak": 5}
        }]);
        std::fs::write(dir.join("automations.json"), row.to_string()).expect("write automations");
    }

    #[test]
    fn pause_it_from_a_card_turns_the_job_off_once_and_records_the_approval() {
        let dir = crate::harness::test_dir("proactive-offered");
        let _pin = crate::perm::ConfigGuard::set(&dir);
        write_automation(&dir, "auto-abc", true);
        let args = r#"{"id":"auto-abc","reason":"failed 5 runs in a row"}"#;

        let run = run_offered_step(
            &dir,
            &dir,
            "automation_disable",
            args,
            AccessMode::Supervised,
        );
        let AutoRun::Done { kind, change } = run else {
            panic!("expected Done, got {run:?}");
        };
        assert_eq!(kind, ChangeKind::Automation);
        assert_eq!(change.id, "auto-abc");
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("automations.json")).unwrap())
                .unwrap();
        assert_eq!(saved[0]["enabled"], serde_json::json!(false));
        let ledger = ChangeLedger::load_kind(&dir, ChangeKind::Automation);
        let ids: Vec<&str> = ledger.all().iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["auto-abc"]);
        let spans = read_spans(&dir, PROACTIVE_TRACE).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].decision, "approve");
        assert_eq!(spans[0].result, "done");
        assert_eq!(spans[0].undo_ref, format!("automation:{}", change.seq));
        assert_eq!(mind_key(&spans[0]), "proactive:automation_disable");

        // Already off: the tool changes nothing, so no span and no ledger line.
        let again = run_offered_step(
            &dir,
            &dir,
            "automation_disable",
            args,
            AccessMode::Supervised,
        );
        assert!(matches!(again, AutoRun::Failed(_)), "{again:?}");
        assert_eq!(read_spans(&dir, PROACTIVE_TRACE).unwrap().len(), 1);
        assert_eq!(
            ChangeLedger::load_kind(&dir, ChangeKind::Automation)
                .all()
                .len(),
            1
        );

        // A job that is not there fails the same way.
        let gone = run_offered_step(
            &dir,
            &dir,
            "automation_disable",
            r#"{"id":"auto-none"}"#,
            AccessMode::Supervised,
        );
        assert!(matches!(gone, AutoRun::Failed(_)), "{gone:?}");
        assert_eq!(read_spans(&dir, PROACTIVE_TRACE).unwrap().len(), 1);
        let _ = crate::tools::control::take_automation_changes();
        let _ = crate::harness::take_self_changes(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hard_step_from_a_card_is_refused_before_it_runs() {
        let dir = crate::harness::test_dir("proactive-offered-hard");
        let _pin = crate::perm::ConfigGuard::set(&dir);
        let args = r#"{"name":"crm","url":"http://127.0.0.1:9/mcp","needs_token":true}"#;
        assert!(step_class("connection_add", args).is_some());
        let run = run_offered_step(&dir, &dir, "connection_add", args, AccessMode::Supervised);
        assert_eq!(run, AutoRun::Failed("That step always asks.".into()));
        assert!(!dir.join("mcp.json").exists());
        assert!(read_spans(&dir, PROACTIVE_TRACE).unwrap().is_empty());
        // And a step with no ledger target never runs.
        assert_eq!(
            run_offered_step(
                &dir,
                &dir,
                "write",
                r#"{"path":"a.txt"}"#,
                AccessMode::Supervised
            ),
            AutoRun::NoUndo
        );
        assert!(!dir.join("a.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hundred_soft_approvals_never_let_a_hard_send_auto_act() {
        let dir = crate::harness::test_dir("proactive-hundred");
        let soft_args = r#"{"name":"notes"}"#;
        for _ in 0..100 {
            note_proactive(
                &dir,
                &proactive_span(
                    "connection_disable",
                    soft_args,
                    "approve",
                    "",
                    AccessMode::Full,
                ),
            );
        }
        let mind = proactive_mind(&dir, Arc::new(SystemClock));
        assert_eq!(mind.mind_prior("proactive:connection_disable"), 0.0);
        let send = candidate(
            "mail_send",
            r#"{"to":"sam@example.com","body":"See you at 3"}"#,
        );
        let ctx = CeilingCtx {
            p_mind: Some(mind.mind_prior(&send.key)),
            ..open()
        };
        assert_eq!(
            AutoAct::admit(send.clone(), &ctx),
            Err(grokhub_core::CeilingMiss::HardClass)
        );
        assert_eq!(
            decide(Step::Proactive {
                name: &send.tool,
                arguments: &send.arguments,
                key: &send.key,
                mind: &mind
            }),
            GateOutcome::Park {
                reason: "hard-class send: Send — Always cannot skip".into(),
                hard: Some(HardClass::Send),
                needs_user: true,
            }
        );
    }

    #[test]
    fn undo_and_never_spans_raise_the_prior() {
        let dir = crate::harness::test_dir("proactive-never");
        let args = r#"{"name":"notes"}"#;
        note_proactive(
            &dir,
            &proactive_span("connection_disable", args, "approve", "", AccessMode::Full),
        );
        assert_eq!(
            proactive_mind(&dir, Arc::new(SystemClock)).mind_prior("proactive:connection_disable"),
            0.0
        );
        note_proactive(
            &dir,
            &proactive_span(
                "connection_disable",
                args,
                DECISION_UNDO,
                "",
                AccessMode::Full,
            ),
        );
        assert_eq!(
            proactive_mind(&dir, Arc::new(SystemClock)).mind_prior("proactive:connection_disable"),
            0.6
        );
        note_proactive(
            &dir,
            &proactive_span(
                "connection_disable",
                args,
                DECISION_NEVER,
                "",
                AccessMode::Full,
            ),
        );
        let mind = proactive_mind(&dir, Arc::new(SystemClock));
        assert_eq!(mind.mind_prior("proactive:connection_disable"), 1.0);
        for _ in 0..30 {
            note_proactive(
                &dir,
                &proactive_span("connection_disable", args, "approve", "", AccessMode::Full),
            );
        }
        let later = proactive_mind(&dir, Arc::new(SystemClock));
        assert_eq!(later.mind_prior("proactive:connection_disable"), 1.0);
        let c = candidate("connection_disable", args);
        assert_eq!(
            later.mind_route(&crate::harness::Candidate {
                key: &c.key,
                hard: None
            }),
            crate::harness::MindRoute::Ask,
            "never stays ask-first however many approvals follow"
        );
    }
}
