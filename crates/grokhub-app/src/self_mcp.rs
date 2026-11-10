//! `grokhub --mcp-self`: the `grokhub-self` stdio server now lives in
//! `grokhub_agent::self_manage::mcp`, so the headless `grokhub-hub` binary
//! serves the same tools. The cabin keeps its flag and passes its config dir.

pub fn run_stdio() -> i32 {
    grokhub_agent::self_manage::mcp::run_stdio(&crate::config::config_dir())
}
