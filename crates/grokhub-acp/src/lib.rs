//! Grok Build ACP client. The cabin talks to `grok agent stdio`, not the TUI.

mod catalog;
mod client;
mod install;
mod locate;
mod stream;
pub mod protocol;

pub use catalog::{
    inspect_advisory, load_grok_catalog, parse_inspect_skills, parse_mcp_list, parse_models_list,
    parse_plugin_list, parse_workflows, skill_source_label,
    GrokCatalog, GrokMcpRow, GrokPluginRow, GrokSkillRow, GrokWorkflowRow,
};
pub use client::{
    cabin_has_session, connect, delete_session, discover_session_files, discover_session_files_in,
    use_user_grok_home, HANDSHAKE_TIMEOUT,
    ensure_session_cwd, load_session_signals, session_id_in_home, session_resume_is_missing,
    explain_handshake_error, inspect_json, is_placeholder_session_title, is_session_cwd_error,
    is_sigterm_status, jsonrpc_error_text, list_sessions, merge_grok_sessions, parse_session_list,
    history_label_after_plan, parse_session_markdown, parse_single_turn, preferred_history_title,
    run_single_turn, title_after_selecting_plan,
    run_single_turn_full, spawn_grok_p_stream, session_usage,
    session_title_from_chat_history,
    show_session, split_session_row, wait_event, AcpHandle, GrokSession, SingleTurn, SpawnOpts,
};
pub use install::{
    begin_ensure_grok_alpha, begin_grok_install, begin_grok_install_force, begin_keep_cli_alpha,
    grok_cli_install_cmd,
    install_grok_blocking, install_grok_blocking_force, keep_cli_alpha_blocking,
    prepend_dir_to_path, prepend_grok_bin_to_process_path,
};
pub use locate::{
    agent_args, agent_args_resume, cabin_grok_home, cabin_leader_socket, clear_grok_unusable,
    cli_install_should_skip, doctor_broken_hint, doctor_grok_line, doctor_grok_line_blocking,
    doctor_line_busy, doctor_missing_hint, find_grok, grok_auth_path, grok_bin_looks_complete,
    grok_cli_channel, grok_cli_is_runnable, grok_cli_key, grok_cli_known_good, grok_home,
    grok_marked_unusable,
    grok_stdout, grok_stdout_timeout, grok_user_stdout_timeout, grok_user_stdout_wait, grok_version,
    hide_windows_console,
    invalidate_grok_bin_cache, invalidate_grok_key_cache, is_cli_hard_failure, mark_grok_unusable,
    parse_grok_auth_key, prepare_cabin_grok_home, silence_windows_hard_errors, single_turn_args,
    single_turn_args_full, which, write_cli_auth_if_needed,
};
pub use protocol::{
    ask_denied_without_acp, merge_tool_card, AcpEvent, ElicitAsk, PermissionAsk, PermissionMode,
    SessionMode, ToolCard, ASK_ACP_DOWN, PROTOCOL_VERSION,
};
pub use stream::{
    fold_stream, grok_context_line, grok_usage_line, kill_pid, parse_signals_json, parse_stream_line,
    parse_usage, prompt_json, rewrite_truncation_error, retry_status_line, turn_footer,
    classify_stream_error, StreamErrorKind, GrokPEvent, GrokUsage,
};
