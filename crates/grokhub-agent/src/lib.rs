//! Native engine. Sync HTTP, no CLI, no credential files.

mod auto_review;
pub mod brain;
mod checkpoint;
mod client;
mod compact;
mod drive;
pub mod episode;
mod eval;
mod events;
mod file_checkpoint;
mod gate;
pub mod harness;
mod hooks;
mod image_budget;
pub mod indexers;
mod loopback;
pub mod mcp;
mod memory;
mod models;
pub mod perm;
pub mod plugins;
mod prefix_cache;
mod prompt;
mod recovery;
pub mod repair;
mod research;
mod retry;
pub mod route;
mod run;
mod scan;
pub mod self_manage;
mod session;
mod session_tools;
mod skills;
mod slash_parity;
mod sse;
mod subagent;
pub mod sudo_pass;
pub mod tasks;
pub mod timing;
pub mod tokens;
mod tools;
mod unattended;

pub use client::{
    map_http_status, request_headers, responses_body, AuthKind, CancelToken, ClientError,
    ContentPart, FunctionCall, InputItem, ModelClient, ResponsesRequest, StreamEvent, TurnOutput,
    Usage, XaiClient, DEFAULT_MODEL, RESPONSES_URL, USER_AGENT,
};
pub use compact::{estimate_input_tokens, message_text};
pub use eval::{parse_args, reject_live, render_report, run_suite, ItemResult, Opts, SUITE_ITEMS};
pub use events::{Engine, EngineParts, EpisodeSeed, NativeEngine, StampHalt};
pub use file_checkpoint::{
    before_write, begin_step, format_steps, list_steps, restore_step, StepInfo, MAX_BYTES, MAX_RUNS,
};
pub use gate::{ClosedPermits, Gate, PermAnswer, PermMode, PermitInbox, PermitNote, PermitWait};
pub use harness::{
    AccessMode, ComputerUseBackend, GateOutcome, HardClass, APPROVAL_GATE_VIOLATION,
};
pub use hooks::{
    discover_hooks, folder_trusted, on_subagent_start, on_subagent_stop, set_folder_trust,
    HookInfo, HookOrigin,
};
pub use mcp::{
    alias_elicit, attach_elicit, configured, detach_elicit, doctor, import_documents, import_paths,
    restart, set_workspace, shutdown_all, unalias_elicit, DoctorRow, ElicitInbox, ElicitNote,
    ElicitView,
};
pub use memory::{
    amr_enabled, amr_store, dream, first_turn_injection, flush_pending, is_dream_command,
    is_flush_command, queue_memory_status, remember, remember_note_text, take_memory_status,
    INJECT_BYTE_CAP, INJECT_TOKEN_CAP,
};
pub use models::{
    context_length_for, parse_listed_models, parse_xai_models, pick_model, ListedModel,
    GROK_47_CONTEXT_LENGTH, XAI_MODELS_URL,
};
pub use prefix_cache::{
    enabled as prefix_cache_enabled, settings as prefix_cache_settings,
    Settings as PrefixCacheSettings,
};
pub use prompt::{
    prompt_need_from_job, system_prompt, system_prompt_parts, system_prompt_with,
    tool_set_from_job, PromptNeed, PromptProfile,
};
pub use recovery::{
    project as project_recovery, state_for_rung, DriveFact, RecoveryFacts, RecoverySnapshot,
    RecoveryState,
};
pub use research::native_deep_research_prompt;
pub use retry::{
    decide_retry, jitter_backoff, resolve_max_retries_with_env, retry_after_or_backoff,
    retry_backoff_with_jitter, DEFAULT_MAX_RETRIES, MAX_RETRY_BACKOFF, RATE_LIMIT_RETRY_THRESHOLD,
    TRANSPORT_REBUILD_BACKOFF,
};
pub use run::{
    run_loop, HaltCheck, LoopEvent, LoopIn, LoopOut, SteerQueue, StopReason, REPEAT_REPLAN,
    TRUNCATED_TOOL,
};
pub use session::{
    attach_run, cancel_session, delete_session, export_markdown, fork_session, format_cost_ticks,
    history_generation, list_sessions, load_session, local_title, merge_history, record_compaction,
    record_turn, rename_session, resume_input, session_file, transcript_pairs, usage_label,
    HistoryRow, RunGuard, SessionInfo,
};
pub use session_tools::{plan_on, set_plan_session, todos_for, TodoItem};
pub use skills::{discover as discover_skills, Skill, SkillSource};
pub use slash_parity::{slash_parity, unparsed_native_slash, SlashParity, UnparsedSlash};
pub use sse::SseParser;
pub use subagent::{drain_side_events, requeue_side_events, SideEvent};
pub use tasks::{
    forget_session, halt_all_sessions, halt_session, halt_tree, hub_for, link_child,
    session_is_live, watch_cancel,
};
pub use tools::control::{take_automation_changes, AutomationChange};
pub use tools::{
    deferred_tools_enabled, deferred_tools_settings, dispatch_for_job, execute, is_readonly,
    public_only_resolve, schemas_for, schemas_for_job, tool_schemas, tool_set_for_kind,
    tool_set_refusal, DeferredToolsSettings, DesktopOps, ToolOutput, ToolSet, READ_ONLY_PHASE,
    TOOL_SET_OUTSIDE,
};
pub use unattended::{run_unattended, UnattendedDone, UnattendedRun};
