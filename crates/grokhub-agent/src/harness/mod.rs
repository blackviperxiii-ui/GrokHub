//! Spike-0 agentic harness kernel.
//!
//! D1: a stricter layer on top of Grok Build. GB keeps its permission prompts
//! (Ask / Auto / Always) and computer use. The cabin only adds a pre-check in
//! front of GB tool execution: hard floor deny, hard-class park (even under
//! Always), and the desktop switch as Access. It never loosens GB.
//! D2: Readonly / Supervised is the Settings switch "Let Grok control the
//! desktop"; Full is one inline Work-tree card. No new chrome.
//! D3: Windows runs the same gate on the shipped `grokhub-desktop` tools.
//! No Cua, no new cabin-native CU.
//!
//! Paths: A `grokhub-desktop` dispatch, B ACP ask, C headless `--deny` rules,
//! E native Lab engine (`gate.rs`). Every path writes the same span.
//!
//! Spike-4a trust floor: `consent` (ConsentLedger, scopes all off) and
//! `egress` (EgressGuard + `egress.jsonl`) answer through the same `decide`
//! (`Step::Egress`, `Step::Scope`). Grok Build's own traffic stays outside (D1).
//!
//! Spike-5 slice: `changes` (ChangeLedger for skills) keeps every version a
//! self-managed skill write replaces; undo and restore need a user's typing
//! or click (`UndoAsk`).

mod access;
mod approval;
mod backend;
mod changes;
mod consent;
mod detect;
mod egress;
mod hard;
mod park;
mod span;

pub use access::{always_does_not_imply_full, AccessMode};
pub use approval::{
    always_keeps_access, apply_access, decide, decide_harness, hard_card_key, resolve_park, GateOutcome,
    HardAnswer, HardPark, Step, APPROVAL_TTL,
};
pub use backend::{
    computer_tool_names, desk_args, desk_decide, desk_span, grok_build_click, ClickOutcome,
    ClickRequest, ComputerUseBackend, DeskCall, CU_TRACE,
};
pub use changes::{
    change_id, content_hash, record_skill_change, restore_skill, skill_history_dir, skill_ledger_path,
    undo_skill_change, Change, ChangeLedger, ChangeOp, Reverted, UndoAsk, CHANGES_DIR, HISTORY_CAP,
    LEDGER_LINE_CAP, SKILL_LEDGER_FILE,
};
pub use consent::{
    consent_path, grant_destination, grant_scope, revoke_grant, scope_excluded, scope_refusal,
    ConsentLedger, Grant, Scope, UserClick, CONSENT_FILE, SCOPE_HARD_EXCLUDES, SCOPE_KINDS,
};
pub use egress::{
    append_egress, egress_dest, egress_path, guard_egress, is_local_dest, is_model_host,
    read_egress, record_approved_once, DataClass, EgressBasis, EgressLine, EgressReq, EGRESS_FILE,
    HUB_DEST, HUB_SYNC_DATA,
};
pub use detect::{
    approval_gate_violation, fixture_hard_allow_without_approve, fixture_hard_with_approve,
    Finding, APPROVAL_GATE_VIOLATION,
};
pub use hard::{
    classify, classify_ask, desk_classify, hard_class, hard_floor, HardClass, HardFloor, HardHit,
    HEADLESS_DENY_RULES,
};
pub use park::{
    answer_park, clear_park, park_dir, pending_parks, post_park, take_answer, wait_park,
    ParkRequest,
};
pub use span::{
    append_span, read_spans, read_turn_context, redact_args, span_path, turn_context_path,
    write_turn_context, Origin, Span, TurnContext,
};

/// Scratch dir for harness tests, under the workspace `target/` (not the
/// shared system temp dir).
#[cfg(test)]
pub(crate) fn test_dir(label: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/harness-tests")
        .join(format!("{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("harness test dir");
    p
}
