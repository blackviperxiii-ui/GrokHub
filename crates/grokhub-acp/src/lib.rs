//! Grok Build ACP client. The cabin talks to `grok agent stdio`, not the TUI.

mod client;
mod locate;
pub mod protocol;

pub use client::{
    connect, inspect_json, list_sessions, parse_session_list, wait_event, AcpHandle, SpawnOpts,
};
pub use locate::{
    agent_args, doctor_grok_line, find_grok, grok_stdout, grok_version, which,
};
pub use protocol::{
    AcpEvent, PermissionAsk, PermissionMode, SessionMode, ToolCard, PROTOCOL_VERSION,
};
