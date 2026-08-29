//! Grok Build ACP client. The cabin talks to `grok agent stdio`, not the TUI.

mod catalog;
mod client;
mod locate;
mod stream;
pub mod protocol;

pub use catalog::{
    load_grok_catalog, parse_inspect_skills, parse_mcp_list, parse_models_list, parse_plugin_list,
    parse_workflows, skill_source_label,
    GrokCatalog, GrokMcpRow, GrokPluginRow, GrokSkillRow, GrokWorkflowRow,
};
pub use client::{
    cabin_has_session, connect, delete_session, discover_session_files, discover_session_files_in,
    ensure_session_cwd, load_session_signals, session_id_in_home, session_resume_is_missing,
    explain_handshake_error, inspect_json, is_placeholder_session_title, is_session_cwd_error,
    is_sigterm_status, jsonrpc_error_text, list_sessions, merge_grok_sessions, parse_session_list,
    parse_session_markdown, parse_single_turn, preferred_history_title, run_single_turn,
    run_single_turn_full, spawn_grok_p_stream,
    session_title_from_chat_history,
    show_session, split_session_row, wait_event, AcpHandle, GrokSession, SingleTurn, SpawnOpts,
};
pub use locate::{
    agent_args, agent_args_resume, doctor_grok_line, doctor_line_busy, find_grok, grok_auth_path,
    cabin_grok_home, cabin_leader_socket, grok_cli_key, grok_home, grok_stdout, grok_stdout_timeout,
    grok_user_stdout_timeout, grok_version, parse_grok_auth_key, prepare_cabin_grok_home,
    single_turn_args, single_turn_args_full, which,
};
pub use protocol::{
    merge_tool_card, AcpEvent, PermissionAsk, PermissionMode, SessionMode, ToolCard,
    PROTOCOL_VERSION,
};
pub use stream::{
    fold_stream, grok_context_line, grok_usage_line, kill_pid, parse_signals_json, parse_stream_line,
    parse_usage, prompt_json, rewrite_truncation_error, turn_footer, classify_stream_error,
    StreamErrorKind, GrokPEvent, GrokUsage,
};
