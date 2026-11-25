//! Native engine. Sync HTTP, no CLI, no credential files.

mod agent_message;
mod auto_review;
pub mod batch;
pub mod brain;
mod builtin_skills;
pub mod cache_watch;
pub mod channels_net;
pub mod check_decide;
mod checkpoint;
pub mod claude_plan;
mod client;
mod close_check;
pub mod code_guard;
mod compact;
mod config_file;
mod denials;
pub mod device;
mod diet;
pub mod dream_gate;
mod dream_model;
mod drive;
pub mod elevate;
pub mod episode;
mod eval;
mod events;
mod file_checkpoint;
mod gate;
pub mod harness;
mod held;
mod hooks;
pub mod identity;
mod image_budget;
pub mod imagine_keychain;
pub mod indexers;
pub mod keyring_blob;
mod lead;
mod loopback;
mod lru_map;
pub mod mcp;
mod memory;
mod memory_embed;
mod models;
pub mod mods_net;
pub mod onboard;
pub mod outbound_hooks;
mod pace;
pub mod package_age;
pub mod perm;
pub mod plan_room;
mod plan_skills;
pub mod plugins;
mod post_check;
mod prefix_cache;
pub mod prefix_guard;
mod prompt;
mod recovery;
pub mod repair;
mod repo_rules;
pub mod research;
mod retry;
pub mod review;
pub mod route;
pub mod routine_consent;
pub mod routine_draft;
pub mod routine_fix;
pub mod routine_log;
pub mod routine_run;
pub mod routine_triggers;
pub mod routine_wake;
mod run;
pub mod sandbox;
mod scan;
pub mod scratch;
pub mod self_manage;
mod session;
mod session_tools;
pub mod ship_tree;
pub mod skill_write;
mod skills;
mod sse;
mod stall;
mod subagent;
mod subagent_policy;
pub mod sudo_pass;
pub mod tasks;
pub mod timing;
pub mod tokens;
mod tools;
mod turn_budget;
mod unattended;
pub mod voice_session;
pub mod vstack_sync;
mod work;
mod ws_mode;
pub mod xai_keychain;

pub use client::{
    map_http_status, request_headers, responses_body, AuthKind, CancelToken, ClientError,
    ContentPart, FunctionCall, InputItem, ModelClient, ResponsesRequest, StreamEvent, TextFormat,
    TurnOutput, Usage, XaiClient, DEFAULT_MODEL, RESPONSES_URL, USER_AGENT,
};
pub use compact::{estimate_input_tokens, message_text};
pub use config_file::config_warnings;
pub use dream_model::sleep_dream;
pub use eval::efficiency::{render_bench, run_bench, BenchRow};
pub use eval::live::{
    ab_flag as live_ab_flag, ab_passed as live_ab_passed, dream_lessons_passed,
    parse_budget as parse_live_budget, read_progress as read_live_progress, render_live, run_live,
    run_live_with, write_atomic as write_live_atomic, write_progress as write_live_progress,
    LiveOpts, LiveReport, LocalClient as LiveLocalClient, OpenRouterClient as LiveOpenRouterClient,
    Provider as LiveProvider, RetryPolicy as LiveRetryPolicy, AB_FLAGS as LIVE_AB_FLAGS,
    DEFAULT_K as LIVE_DEFAULT_K,
};
pub use eval::memory_index::{render_memory_index_bench, run_memory_index_bench, MemoryIndexBench};
pub use eval::turn_endings::{generic_closing, last_sentence};
pub use eval::{
    live_budget_ticks, load_tasks, parse_args, reject_live, render_markdown, render_report,
    render_report_with, run_live_suite, run_live_suite_with, run_suite, run_technique_matrix,
    ItemResult, LiveSpend, LiveSuite, Opts, Recommendation, Technique, TechniqueArm,
    TechniqueMatrixReport, LIVE_BUDGET_CAP_CENTS, SUITE_ITEMS,
};
pub use events::{Engine, EngineParts, EpisodeSeed, NativeEngine, StampHalt};
pub use file_checkpoint::{
    before_write, begin_step, format_steps, list_steps, restore_step, StepInfo, MAX_BYTES, MAX_RUNS,
};
pub use gate::{ClosedPermits, Gate, PermAnswer, PermMode, PermitInbox, PermitNote, PermitWait};
pub use harness::{
    AccessMode, ComputerUseBackend, GateOutcome, HardClass, APPROVAL_GATE_VIOLATION,
};
pub use held::{held_secrets, hold_secret};
pub use hooks::{
    discover_hooks, folder_trusted, hooks_by_trust, on_subagent_start, on_subagent_stop,
    set_folder_trust, HookInfo, HookOrigin, HookRun,
};
pub use identity::{identity_blocks, SOUL_BYTE_CAP, USER_BYTE_CAP};
pub use lead::{lead_on, set_lead_session, LeadPhase};
pub use mcp::{
    alias_elicit, app_call_tool, app_host_initialize, attach_elicit, configured, detach_elicit,
    doctor, import_documents, import_paths, restart, set_workspace, shutdown_all, unalias_elicit,
    DoctorRow, ElicitInbox, ElicitNote, ElicitView, McpAppView,
};
pub use memory::{
    amr_enabled, amr_store, dream, dream_model_enabled, drop_global_copies, first_turn_injection,
    flush_pending, forget_session_topic, global_memory_path, is_dream_command, is_flush_command,
    memory_writes_enabled, queue_memory_status, remember, remember_note_text,
    search as search_memory, session_lines_about, set_memory_writes, take_memory_status,
    workspace_memory_path, Hit as MemoryHit, INJECT_BYTE_CAP, INJECT_TOKEN_CAP,
};
pub use memory_embed::embedder as text_embedder;
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
pub use prompt::{
    chat_volatile_tail, prompt_need_from_job, system_prompt, system_prompt_parts,
    system_prompt_with, tool_set_from_job, ChatTail, PromptNeed, PromptProfile,
};
pub use recovery::{
    project as project_recovery, state_for_rung, DriveFact, RecoveryFacts, RecoverySnapshot,
    RecoveryState,
};
pub use repo_rules::{
    discover as discover_repo_rules, resolve_preferred_branch,
    task_prefix as repo_rules_task_prefix, RepoRulesBundle, RepoRulesInput, RepoSync, RuleFile,
    RuleKind, PROMPT_CAP as REPO_RULES_PROMPT_CAP,
};
pub use retry::{
    decide_retry, jitter_backoff, resolve_max_retries_with_env, retry_after_or_backoff,
    retry_backoff_with_jitter, DEFAULT_MAX_RETRIES, MAX_RETRY_BACKOFF, RATE_LIMIT_RETRY_THRESHOLD,
    TRANSPORT_REBUILD_BACKOFF,
};
pub use run::{
    bind_job_tool_set, clear_job_tool_set, run_loop, HaltCheck, JobToolSetGuard, LoopEvent, LoopIn,
    LoopOut, SteerQueue, StopReason, TurnKept, BUSY_GAVE_UP, REPEAT_REPLAN, TRUNCATED_TOOL,
};
pub use session::{
    attach_run, cancel_session, compacted_messages, delete_forks_of, delete_session,
    export_markdown, forget_message, fork_session, format_cost_ticks, history_generation,
    list_session_heads, list_sessions, load_session, local_title, merge_history, message_pairs,
    record_compaction, record_turn, rename_session, resume_input, session_file, session_intact,
    shown_pairs, transcript_pairs, usage_label, user_lines, Forget, Forgotten, HistoryRow,
    RunGuard, SessionHead, SessionInfo, TurnThought, UserKind, UserLine, FORGET_BUSY,
};
pub use session_tools::{plan_on, set_plan_session, todos_for, TodoItem};
pub use skills::{
    active_always_on, active_builtin_bodies, builtin_names, builtin_text,
    discover as discover_skills, discover_all as discover_all_skills, invalidate_discover_cache,
    set_skill_off_in, skill_is_off_in, skill_name_key, skill_quarantine_in, skills_off_in, Skill,
    SkillSource, BUILTIN_ALWAYS_ON, BUILTIN_SKILL_BODIES,
};
pub use sse::SseParser;
pub use subagent::{drain_side_events, requeue_side_events, SideEvent};
pub use tasks::{
    drain_task_changes, forget_session, halt_all_sessions, halt_session, halt_tree, hub_for,
    link_child, session_is_live, set_task_watch, watch_cancel, TaskPeek, TaskProgress, TaskRow,
    TaskState,
};
pub use tools::control::{take_automation_changes, AutomationChange};
pub use tools::home_assistant::{
    available as home_assistant_available, clear_host as clear_home_assistant_host,
    set_host as set_home_assistant_host, HomeAssistantOps, OFF_MSG as HOME_ASSISTANT_OFF,
};
pub use tools::mods::{set_host as set_mods_host, ModsOps};
pub use tools::{
    brave_key_present, deferred_tools_enabled, deferred_tools_settings, dispatch_for_job, execute,
    generative_ui_enabled, imagine_client, is_readonly, public_only_resolve, run_media_tool,
    schemas_for, schemas_for_job, seal_brave_key, searxng_url_problem, tool_schemas,
    tool_set_for_kind, tool_set_refusal, DeferredToolsSettings, DesktopOps, ImagineApi,
    ImagineSource, ToolOutput, ToolSet, WebSearchBackend, WebSearchSettings, READ_ONLY_PHASE,
    TOOL_SET_OUTSIDE, WEB_SEARCH_BACKENDS,
};
pub use unattended::{run_unattended, UnattendedDone, UnattendedRun};
pub use work::{
    discard_work, how_ended, merge_work, resume_work, settled, why_held, work_for, By, Counts,
    Ended, Held, Landing, Resume, Work, WorkState,
};
/// The TLS config the agent's sockets use, for the shell's speech stream.
pub use ws_mode::tls_config as ws_tls_config;
