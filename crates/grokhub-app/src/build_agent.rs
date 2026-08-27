//! Spawn and talk to the Grok Build CLI over ACP.

use grokhub_acp::{connect, find_grok, AcpHandle, PermissionMode, SessionMode, SpawnOpts};
use std::path::PathBuf;

pub fn can_agent(_has_auth: bool) -> bool {
    find_grok().is_some()
}

pub fn spawn_session(
    cwd: PathBuf,
    api_key: Option<String>,
    xai_api_key: Option<String>,
    perm: PermissionMode,
    mode: SessionMode,
    reasoning_effort: Option<String>,
    resume: Option<String>,
    skip_cabin_home: bool,
    worktree: bool,
) -> Result<AcpHandle, String> {
    let yolo = perm == PermissionMode::AlwaysApprove;
    let auto = perm == PermissionMode::Auto;
    let mut opts = SpawnOpts::grok(cwd, api_key, yolo, auto, mode, reasoning_effort)?;
    opts.skip_cabin_home = skip_cabin_home;
    opts.worktree = worktree;
    opts = opts.with_xai_api_key(xai_api_key).with_resume(resume);
    let name = opts
        .program
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if name.contains("fake-acp") {
        opts.args.clear();
    }
    connect(opts)
}

pub fn grok_banner() -> String {
    match find_grok() {
        Some(p) => grokhub_acp::doctor_grok_line(Some(&p)).1,
        None => grokhub_acp::doctor_grok_line(None).1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_needs_cli() {
        assert_eq!(can_agent(true), find_grok().is_some());
        assert_eq!(can_agent(false), find_grok().is_some());
    }

    #[test]
    fn banner_mentions_install_or_version() {
        let t = grok_banner();
        assert!(t.contains("Grok Build") || t.contains("x.ai/cli"), "{t}");
    }
}
