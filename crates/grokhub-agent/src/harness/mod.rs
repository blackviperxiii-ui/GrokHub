//! Spike-0 agentic harness kernel.
//!
//! D1: a stricter layer on top of Grok Build. GB keeps its permission prompts
//! (Ask / Auto / Always) and computer use. The cabin only adds a pre-check in
//! front of GB tool execution: hard floor deny, hard-class park (even under
//! Always), and the desktop switch as Access. It never loosens GB.
//! D2: Readonly / Supervised is the Settings switch "Let Grok control the
//! desktop"; Full is one inline Work-tree card. No new chrome.
//! D3: Windows runs the same gate on the shipped `grokhub-desktop` tools.
//! No new cabin-native CU. Spike-2a: Cua Driver (MIT, `bounded`, pinned) is
//! an optional second pair of hands on Linux behind `grokhub --mcp-cua`
//! (`cua`), off by default; its calls go through the same `decide`.
//!
//! Paths: A `grokhub-desktop` dispatch, B ACP ask, C headless `--deny` rules,
//! D Grok Build's own computer use (`path_d`: `--deny` rules plus a watchdog
//! on frames nobody asked about), E native Lab engine (`gate.rs`). Every path
//! writes the same span.
//!
//! Spike-4a trust floor: `consent` (ConsentLedger, scopes all off) and
//! `egress` (EgressGuard + `egress.jsonl`) answer through the same `decide`
//! (`Step::Egress`, `Step::Scope`). Grok Build's own traffic stays outside (D1).
//! Spike-4b: `at_rest` seals the learned tier (consent and egress logs,
//! personal and sensitive AMR nodes) with a key held in the OS keyring, and
//! fails closed without it.
//!
//! Spike-5: `changes` (ChangeLedger for skills, connections, and automations)
//! keeps every version a self-managed write replaces; undo, restore, and Keep
//! need a user's typing or click (`UndoAsk`). `self_manage` holds the
//! connection and automation targets and the new-automation cap (Spike-5b).
//!
//! Spike-5a: `mindcheck` folds denies, undos, approves and Pulse dismisses
//! into a prior per action class ("if unsure whether you'd be upset, ask").
//! It is a soft-path input to `decide` (`Step::Proactive`), never a second
//! gate, and never touches hard class. An agent-started memory forget is
//! hard class Delete (`agent_forget`).
//!
//! Spike-6b: `proactive` runs one auto-act the autonomy ceiling
//! (`grokhub_core::AutoAct`) admitted, through `decide` (`Step::Proactive`)
//! and the native dispatch, only when a change-ledger target exists, and
//! links the ledger line on its span (`undo_ref`).
//!

//! Spike-1a safety loop: `detect` catches failed or looping actions and
//! unbacked claims, `audit` runs them in two cheap passes, and `ladder`
//! recovers (retry once, backtrack) or pauses for the user. Hard class is
//! never retried. Typing into a credential field is hard class credentials.
//!
//! Spike-2b: a click is classified by the control it lands on (`hard.rs`
//! `click_target_class`): a Send, Pay, Delete or Reset button parks before
//! the click runs, on every path, and spans keep only the matched rule.
//!
//! Spike-3a (AMR M4): `trail` turns one turn's spans into one AMR `trail`
//! node at turn end, from already-redacted span fields only; `span_search`
//! reads `spans/*.jsonl` for History and palette search. Both only read
//! spans and never execute anything.

mod access;
mod approval;
mod at_rest;
mod audit;
mod backend;
mod changes;
mod consent;
mod cua;
mod detect;
mod egress;
#[cfg(test)]
mod egress_coverage;
mod hard;
mod ladder;
mod mindcheck;
mod park;
mod path_d;
mod proactive;
mod self_improve;
mod self_manage;
mod span;
mod span_search;
mod trail;

pub use access::{always_does_not_imply_full, AccessMode};
pub use approval::{
    always_keeps_access, apply_access, decide, decide_harness, hard_card_key, resolve_park, GateOutcome,
    HardAnswer, HardPark, Step, APPROVAL_TTL,
};
pub use at_rest::{
    has_sealed_data, keyring_name, keyring_name_for, read_key, recheck_keyring, set_default_key_store, use_key_store_for, use_os_keyring,
    KeyStore, KeyringOs, LearnedVault, Locked, MemoryKeyStore, OsKeyring, KEY_ACCOUNT, KEY_ID_FILE, KEY_SERVICE,
    SEALED_PREFIX,
};
pub use backend::{
    computer_tool_names, desk_access, desk_args, desk_decide, desk_span, grok_build_click, park_desk_call, ClickOutcome,
    ClickRequest, ComputerUseBackend, DeskCall, CU_TRACE,
};
pub use cua::{
    check_cua_version, cua_as_desk, cua_manifest, cua_manifest_path, cua_socket_path, cua_spawn_args, cua_spawn_env,
    find_cua_driver, spawn_cua_child, verify_cua_driver, CuaChild, CuaGate, CuaProxy, StdioChild, CUA_DRIVER_LICENSE,
    CUA_DRIVER_TAG, CUA_DRIVER_VERSION, CUA_ENV_REMOVE, CUA_LINUX_ASSET, CUA_LINUX_SHA256, CUA_LINUX_ONLY_MSG,
    CUA_MISSING_MSG, CUA_OFF_MSG, CUA_PERMISSION_MODE, CUA_TOOLS,
};
pub use changes::{
    accept_change, change_id, note_proposal_finding, content_hash, entry_id, findings_path, history_dir, ledger_path, read_scope_findings,
    record_change, record_skill_change, restore_change, restore_skill, scope_guard, skill_history_dir,
    skill_ledger_path, take_self_changes, undo_change, undo_skill_change, Change, ChangeKind, ChangeLedger,
    ChangeOp, ChangeTarget, Reverted, ScopeFinding, SkillTarget, UndoAsk, AUTOMATION_LEDGER_FILE, CHANGES_DIR,
    CONNECTION_LEDGER_FILE, FINDINGS_FILE, HISTORY_CAP, LEDGER_LINE_CAP, LEDGER_SCOPE_VIOLATION, MODEL_LEDGER_FILE, SKILL_LEDGER_FILE,
};
pub(crate) use changes::private_write;
pub use self_improve::{
    last_recorded_run, outcome_from_spans, patch_marks, reject_live_replay, replay_gate, replay_patch, run_weekly,
    spans_for, ReplayOpts, WeeklyPass, REPLAY_LIVE_REFUSED, REPLAY_TOOL, SELF_REVIEW_SESSION, SELF_REVIEW_TOOL,
};
pub use self_manage::{
    automation_cap_refusal, open_connection_token, tool_origin, undo_connection, AutomationsFile, McpFile,
    SELF_AUTOMATION_WEEK_CAP, SELF_MANAGE_TOOLS, WEEK_MS,
};
pub(crate) use self_manage::{forget_connection_token, seal_connection_token};
pub use consent::{
    consent_path, grant_destination, grant_scope, revoke_grant, scope_excluded, scope_refusal,
    ConsentLedger, Grant, Scope, UserClick, CONSENT_FILE, SCOPE_HARD_EXCLUDES, SCOPE_KINDS,
};
pub use egress::{
    append_egress, current_origin, egress_dest, egress_path, guard_egress, guard_or_park, guard_quiet,
    is_local_dest, is_model_host, model_text_classes, read_egress, read_egress_report, record_approved_once,
    DataClass, EgressBasis, EgressLine, EgressRead, EgressReq, OriginScope, RecallScope, EGRESS_FILE,
    HUB_DEST, HUB_SYNC_DATA,
};
pub use audit::{audit_file, audit_spans, pass1, pass2, summarize, Audit, SpanDigest, Window, WindowAudit, WindowSummary};
pub use detect::{
    action_loop, approval_gate_violation, claimed_click_no_change, claims_done, claims_success, done_without_criteria,
    fixture_action_loop, fixture_claimed_click_no_change, fixture_done_without_criteria, fixture_hard_allow_without_approve,
    fixture_hard_with_approve, fixture_span, fixture_unsupported_assurance, span_kind, step_hash, unsupported_assurance,
    Evidence, Finding, SpanKind, ACTION_LOOP, APPROVAL_GATE_VIOLATION, CLAIMED_CLICK_NO_CHANGE, DONE_WITHOUT_CRITERIA,
    UNSUPPORTED_ASSURANCE,
};
pub use hard::{
    classify, classify_ask, click_action, click_rule, click_target, click_target_class, click_target_in, credential_action,
    credential_field, credential_hint, delete_files_action, delete_targets, desk_classify, hard_class, hard_floor,
    ClickRule, ClickTarget, HardClass, HardFloor, HardHit, BUILTIN_CU_DENY, GB_DENY_GAPS, HEADLESS_DENY_RULES,
    TARGET_HINT,
};
pub use path_d::{builtin_cu, cu_look_only, decide_unasked, unasked_action, unasked_title};
pub use ladder::{hard_target, ladder_span, Ladder, LadderStep, Rung, RECOVERY_TOOL};
pub use mindcheck::{
    agent_forget, learn, mind_key, note_prior, signals_from_cards, signals_from_changes, signals_from_spans, Candidate,
    Clock, MindCheck, MindEvent, MindRoute, MindSignal, Prior, SystemClock, AGENT_FORGET_TOOL, MIND_APPROVE_STEP,
    MIND_ASK_AT, MIND_ASK_FIRST_MS, MIND_DENY, MIND_DISMISS_STEP, SKILL_CHANGE_KEY,
};
pub use proactive::{
    answer_span, ledger_target, note_proactive, proactive_key, proactive_mind, proactive_span, run_approved_once, run_auto_act, step_class,
    AutoRun, DECISION_ASK, DECISION_AUTO, DECISION_NEVER, DECISION_UNDO, PROACTIVE_TRACE,
};
pub use park::{
    answer_park, clear_park, park_dir, pending_parks, post_park, take_answer, wait_park,
    ParkRequest,
};
pub use span::{
    append_span, read_spans, read_spans_tail, read_turn_context, redact_args, span_path, turn_context_path,
    write_turn_context, ModelUsage, Origin, Span, TurnContext, CLAIM_CAP, REPLY_TOOL, SPAN_TAIL_BYTES, VERIFY_TOOL,
};
pub use span_search::{search_spans, SpanHit, SpanSearch, SPAN_SEARCH_FILES, SPAN_SEARCH_HITS, SPAN_SEARCH_LINES};
pub use trail::{link_learned, trail_body, trail_draft, write_trail, TrailWrite, TRAIL_BODY_CAP};

/// Scratch dir for harness tests, under the workspace `target/` (not the
/// shared system temp dir). It gets its own in-memory keyring, so no test
/// reaches the OS keyring.
#[cfg(test)]
pub(crate) fn test_dir(label: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/harness-tests")
        .join(format!("{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("harness test dir");
    use_key_store_for(&p, std::sync::Arc::new(MemoryKeyStore::new()));
    p
}

/// Test stand-in for the cabin's click: answer the first park that shows up
/// under `dir` (Approve or Deny) and hand back what the card would show.
#[cfg(test)]
pub(crate) fn answer_next_park(dir: std::path::PathBuf, approve: bool) -> std::thread::JoinHandle<Option<ParkRequest>> {
    std::thread::spawn(move || {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < until {
            if let Some(req) = pending_parks(&dir).into_iter().next() {
                answer_park(&dir, &req.id, approve).expect("answer park");
                return Some(req);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    })
}
