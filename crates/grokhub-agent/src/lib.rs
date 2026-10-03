//! Native engine. Sync HTTP, no CLI, no credential files.

mod auto_review;
mod client;
mod compact;
mod events;
mod gate;
mod image_budget;
mod models;
pub mod perm;
mod prompt;
mod retry;
mod run;
mod scan;
mod session;
mod sse;
pub mod tasks;
pub mod tokens;
mod tools;

pub use client::{
    map_http_status, request_headers, responses_body, AuthKind, CancelToken, ClientError,
    ContentPart, FunctionCall, InputItem, ModelClient, ResponsesRequest, StreamEvent, TurnOutput,
    Usage, XaiClient, DEFAULT_MODEL, RESPONSES_URL, USER_AGENT,
};
pub use compact::{estimate_input_tokens, manual_compact_targets_native, message_text};
pub use events::{meter_for, Engine, EngineParts, NativeEngine, StampHalt};
pub use gate::{ClosedPermits, Gate, PermAnswer, PermMode, PermitInbox, PermitNote, PermitWait};
pub use models::{
    context_length_for, parse_listed_models, parse_xai_models, pick_model, ListedModel,
    GROK_47_CONTEXT_LENGTH, XAI_MODELS_URL,
};
pub use prompt::system_prompt;
pub use retry::{
    decide_retry, jitter_backoff, resolve_max_retries_with_env, retry_after_or_backoff,
    retry_backoff_with_jitter, DEFAULT_MAX_RETRIES, MAX_RETRY_BACKOFF, RATE_LIMIT_RETRY_THRESHOLD,
    TRANSPORT_REBUILD_BACKOFF,
};
pub use run::{
    run_loop, HaltCheck, LoopEvent, LoopIn, LoopOut, SteerQueue, StopReason, DEFAULT_MAX_TURNS,
};
pub use session::{
    attach_run, cancel_session, delete_session, export_markdown, fork_session, format_cost_ticks,
    history_generation, list_sessions, load_session, local_title, merge_history, record_compaction,
    record_turn, rename_session, resume_input, session_file, transcript_pairs, usage_label,
    HistoryRow, RunGuard, SessionInfo,
};
pub use sse::SseParser;
pub use tasks::{forget_session, halt_all_sessions, halt_session, halt_tree, hub_for, link_child, watch_cancel};
pub use tools::control::{take_automation_changes, AutomationChange};
pub use tools::{
    execute, is_readonly, schemas_for, tool_schemas, DesktopOps, ToolOutput, READ_ONLY_PHASE,
};
