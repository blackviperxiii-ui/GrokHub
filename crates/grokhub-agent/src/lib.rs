//! Native engine. Sync HTTP, no CLI, no credential files.

mod auto_review;
pub mod brain;
mod checkpoint;
mod client;
mod close_check;
mod compact;
mod dream_model;
mod drive;
pub mod episode;
mod eval;
mod events;
mod file_checkpoint;
mod gate;
pub mod harness;
mod hooks;
pub mod identity;
mod image_budget;
pub mod indexers;
mod loopback;
pub mod mcp;
mod memory;
mod models;
pub mod perm;
mod plan_skills;
pub mod plugins;
mod prefix_cache;
pub mod prefix_guard;
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
pub use dream_model::sleep_dream;
pub use eval::efficiency::{render_bench, run_bench, BenchRow};
pub use eval::memory_index::{render_memory_index_bench, run_memory_index_bench, MemoryIndexBench};
pub use eval::turn_endings::{generic_closing, last_sentence};
pub use eval::{
    load_tasks, parse_args, reject_live, render_markdown, render_report, run_suite,
    run_technique_matrix, ItemResult, Opts, Recommendation, Technique, TechniqueArm,
    TechniqueMatrixReport, SUITE_ITEMS,
};
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
pub use identity::{identity_blocks, SOUL_BYTE_CAP, USER_BYTE_CAP};
pub use mcp::{
    alias_elicit, attach_elicit, configured, detach_elicit, doctor, import_documents, import_paths,
    restart, set_workspace, shutdown_all, unalias_elicit, DoctorRow, ElicitInbox, ElicitNote,
    ElicitView,
};
pub use memory::{
    amr_enabled, amr_store, dream, dream_model_enabled, first_turn_injection, flush_pending,
    global_memory_path, is_dream_command, is_flush_command, memory_writes_enabled,
    queue_memory_status, remember, remember_note_text, search as search_memory, set_memory_writes,
    take_memory_status, workspace_memory_path, Hit as MemoryHit, INJECT_BYTE_CAP, INJECT_TOKEN_CAP,
};
pub use models::{
    context_length_for, parse_listed_models, parse_xai_models, pick_model, ListedModel,
    GROK_47_CONTEXT_LENGTH, XAI_MODELS_URL,
};
pub use plan_skills::{
    is_av_change, is_multi_step, is_ship_or_release, moments_for,
    prompt_block as plan_skills_prompt, skills_for, skills_for_context,
    standing_reminder as plan_skills_standing, PlanSkillCtx, SkillMoment, AV_QA_WALKTHROUGH,
    CREATE_VERIFICATION, FOLLOW_THROUGH, MAINTAIN_VERIFICATION, QA_WALKTHROUGH,
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
    bind_job_tool_set, clear_job_tool_set, run_loop, HaltCheck, JobToolSetGuard, LoopEvent, LoopIn,
    LoopOut, SteerQueue, StopReason, REPEAT_REPLAN, TRUNCATED_TOOL,
};
pub use session::{
    attach_run, cancel_session, delete_forks_of, delete_session, export_markdown, fork_session,
    format_cost_ticks, history_generation, list_sessions, load_session, local_title, merge_history,
    record_compaction, record_turn, rename_session, resume_input, session_file, session_intact,
    transcript_pairs, usage_label, HistoryRow, RunGuard, SessionInfo,
};
pub use session_tools::{plan_on, set_plan_session, todos_for, TodoItem};
pub use skills::{
    builtin_names, discover as discover_skills, Skill, SkillSource, BUILTIN_SKILL_BODIES,
};
pub use slash_parity::{slash_parity, unparsed_native_slash, SlashParity, UnparsedSlash};
pub use sse::SseParser;
pub use subagent::{drain_side_events, requeue_side_events, SideEvent};
pub use tasks::{
    forget_session, halt_all_sessions, halt_session, halt_tree, hub_for, link_child,
    session_is_live, watch_cancel, TaskRow, TaskState,
};
pub use tools::control::{take_automation_changes, AutomationChange};
pub use tools::{
    brave_key_present, deferred_tools_enabled, deferred_tools_settings, dispatch_for_job, execute,
    imagine_client, is_readonly, public_only_resolve, run_media_tool, schemas_for, schemas_for_job,
    seal_brave_key, tool_schemas, tool_set_for_kind, tool_set_refusal, DeferredToolsSettings,
    DesktopOps, ImagineApi, ToolOutput, ToolSet, WebSearchBackend, WebSearchSettings,
    READ_ONLY_PHASE, TOOL_SET_OUTSIDE,
};
pub use unattended::{run_unattended, UnattendedDone, UnattendedRun};
