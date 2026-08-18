use crate::host_plan::{explain_host_risk, host_risk, HostPlanStep, HostRisk};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const GROKHUB_ORIGIN_REMOTE: &str = "https://origin.cursor.com/blackviperxiii-ui/grok-hub.git";
pub const GROKHUB_ORIGIN_BROWSE: &str = "https://cursor.com/codebase/blackviperxiii-ui/grok-hub";
pub const ORIGIN_AUTH_HINT: &str =
    "Origin git needs sign-in — run origin auth login, then Update";

/// Overlay update only. Never wipes `~/.config/GrokHub`.
pub fn is_grokhub_source(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file()
        && dir.join("scripts/install.sh").is_file()
        && dir.join("crates/grokhub-app").is_dir()
}

pub fn walk_up_source(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        if is_grokhub_source(&cur) {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

pub fn discover_source(hints: &[PathBuf]) -> Option<PathBuf> {
    for h in hints {
        let p = h.as_path();
        if p.as_os_str().is_empty() {
            continue;
        }
        if is_grokhub_source(p) {
            return Some(p.to_path_buf());
        }
        if let Some(found) = walk_up_source(p) {
            return Some(found);
        }
    }
    None
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn git_output(source: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(source)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))
}

fn git_fail_message(args: &[&str], out: &Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{err}\n{}", String::from_utf8_lossy(&out.stdout));
    if looks_like_origin_auth_error(&combined) {
        return ORIGIN_AUTH_HINT.to_string();
    }
    let msg = err.trim();
    if msg.is_empty() {
        format!("git {} failed", args.join(" "))
    } else {
        msg.to_string()
    }
}

fn git_stdout(source: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_output(source, args)?;
    if !out.status.success() {
        return Err(git_fail_message(args, &out));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_run(source: &Path, args: &[&str]) -> Result<(), String> {
    git_stdout(source, args).map(|_| ())
}

fn git_head_branch(source: &Path) -> Result<String, String> {
    let out = git_output(source, &["symbolic-ref", "-q", "--short", "HEAD"])?;
    if !out.status.success() {
        return Err("source clone is not on a branch — checkout main, then Update".into());
    }
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if branch.is_empty() {
        return Err("source clone is not on a branch — checkout main, then Update".into());
    }
    Ok(branch)
}

pub fn is_legacy_github_remote(url: &str) -> bool {
    let lowered = url.trim().to_ascii_lowercase();
    let stripped = lowered.trim_end_matches(".git");
    let after_auth = stripped.rsplit_once('@').map(|(_, rest)| rest).unwrap_or(stripped);
    let after_scheme = after_auth
        .strip_prefix("https://")
        .or_else(|| after_auth.strip_prefix("http://"))
        .or_else(|| after_auth.strip_prefix("ssh://git@"))
        .or_else(|| after_auth.strip_prefix("ssh://"))
        .or_else(|| after_auth.strip_prefix("git@"))
        .unwrap_or(after_auth);
    let normalized = after_scheme.replace(':', "/");
    normalized == "github.com/blackviperxiii-ui/grok-hub"
        || normalized.starts_with("github.com/blackviperxiii-ui/grok-hub/")
}

pub fn looks_like_origin_auth_error(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    l.contains("authentication failed")
        || l.contains("could not read username")
        || l.contains("permission denied")
        || l.contains("access rights")
        || l.contains("authorization")
        || l.contains("http basic")
        || l.contains("401")
        || l.contains("403")
        || (l.contains("origin.cursor.com")
            && (l.contains("denied") || l.contains("unauthorized") || l.contains("login")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginRemoteAct {
    Unchanged,
    Added,
    Retargeted,
}

pub fn source_origin_url(source: &Path) -> Option<String> {
    git_stdout(source, &["remote", "get-url", "origin"]).ok().filter(|s| !s.is_empty())
}

pub fn ensure_origin_remote(source: &Path) -> Result<OriginRemoteAct, String> {
    match git_stdout(source, &["remote", "get-url", "origin"]) {
        Ok(url) if is_legacy_github_remote(&url) => {
            git_run(source, &["remote", "set-url", "origin", GROKHUB_ORIGIN_REMOTE])?;
            Ok(OriginRemoteAct::Retargeted)
        }
        Ok(_) => Ok(OriginRemoteAct::Unchanged),
        Err(_) => {
            git_run(source, &["remote", "add", "origin", GROKHUB_ORIGIN_REMOTE])?;
            Ok(OriginRemoteAct::Added)
        }
    }
}

fn git_dirty(source: &Path) -> Result<bool, String> {
    let out = git_stdout(source, &["status", "--porcelain"])?;
    Ok(!out.is_empty())
}

fn require_source_on_main(source: &Path) -> Result<(), String> {
    if !is_grokhub_source(source) {
        return Err("not a GrokHub source tree — set Settings → source or GROKHUB_SRC".into());
    }
    let branch = git_head_branch(source)?;
    if branch != "main" {
        return Err(format!(
            "source clone is on {branch} — checkout main, then Update"
        ));
    }
    Ok(())
}

pub fn prepare_update(source: &Path) -> Result<OriginRemoteAct, String> {
    require_source_on_main(source)?;
    if git_dirty(source)? {
        return Err("source clone has local changes — commit or stash, then Update".into());
    }
    ensure_origin_remote(source)
}

fn fetch_origin_main(source: &Path) -> Result<(), String> {
    git_run(source, &["fetch", "origin", "main"])
}

fn tip_sha(source: &Path) -> Result<String, String> {
    match git_stdout(source, &["rev-parse", "origin/main"]) {
        Ok(sha) if !sha.is_empty() => Ok(sha),
        _ => git_stdout(source, &["rev-parse", "FETCH_HEAD"]),
    }
}

fn heads_match(source: &Path) -> Result<bool, String> {
    let head = git_stdout(source, &["rev-parse", "HEAD"])?;
    let tip = tip_sha(source)?;
    Ok(head == tip)
}

fn already_current_status() -> String {
    format!("Already current — v{}", env!("CARGO_PKG_VERSION"))
}

fn overlay_cmds(source: &Path) -> Vec<String> {
    let src = sh_quote(&source.display().to_string());
    vec![
        format!("git -C {src} merge --ff-only origin/main"),
        format!("{src}/scripts/install.sh --user"),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePlan {
    Current { status: String },
    Overlay { cmds: Vec<String> },
}

pub fn plan_update(source: &Path) -> Result<UpdatePlan, String> {
    prepare_update(source)?;
    fetch_origin_main(source)?;
    if heads_match(source)? {
        return Ok(UpdatePlan::Current {
            status: already_current_status(),
        });
    }
    Ok(UpdatePlan::Overlay {
        cmds: overlay_cmds(source),
    })
}

pub fn update_cmds(source: &Path) -> Result<Vec<String>, String> {
    match plan_update(source)? {
        UpdatePlan::Current { .. } => Ok(vec![]),
        UpdatePlan::Overlay { cmds } => Ok(cmds),
    }
}

pub fn update_settings_note(version: &str) -> String {
    format!(
        "Installed v{version}. Overlay only — fetch Origin main, fast-forward when behind, then install.sh --user. The clone must be on main with a clean worktree. Origin git may need `origin auth login`. Does not wipe ~/.config/GrokHub."
    )
}

pub fn update_plan_steps(cmds: Vec<String>) -> Vec<HostPlanStep> {
    cmds.into_iter()
        .map(|cmd| {
            let explain = if cmd.contains("merge --ff-only") || cmd.contains("pull --ff-only") {
                "fast-forward origin/main — config stays".into()
            } else if cmd.contains("fetch origin") {
                "fetch origin/main — config stays".into()
            } else if cmd.contains("install.sh") {
                "overlay ~/.local/bin — does not wipe config".into()
            } else {
                explain_host_risk(&cmd, host_risk(&cmd))
            };
            HostPlanStep {
                cmd,
                risk: HostRisk::Moderate,
                explain,
                checked: true,
            }
        })
        .collect()
}

pub fn update_wipes_config(cmds: &[String]) -> bool {
    cmds.iter().any(|c| {
        let l = c.to_ascii_lowercase();
        l.contains(".config/grokhub") && (l.contains("rm ") || l.contains("rm\t") || l.contains("rm -"))
    })
}

pub fn update_progress_pct(done_cmds: usize, total_cmds: usize) -> u8 {
    if total_cmds == 0 {
        return 100;
    }
    let pct = done_cmds.saturating_mul(100) / total_cmds;
    pct.min(100) as u8
}

pub fn update_step_label(cmd: &str) -> &'static str {
    if cmd.contains("remote set-url") || cmd.contains("remote add") {
        "Pointing origin at Origin…"
    } else if cmd.contains("fetch origin") {
        "Fetching origin/main…"
    } else if cmd.contains("merge --ff-only") || cmd.contains("pull --ff-only") {
        "Updating origin/main…"
    } else if cmd.contains("install.sh") {
        "Installing overlay…"
    } else {
        "Updating…"
    }
}

pub struct OverlayUpdateView {
    pub pct: u8,
    pub status: String,
    pub running: bool,
    pub posts_chat: bool,
    pub stay_on_update: bool,
    pub can_restart: bool,
}

fn overlay_view(pct: u8, status: String, running: bool, can_restart: bool) -> OverlayUpdateView {
    OverlayUpdateView {
        pct,
        status,
        running,
        posts_chat: false,
        stay_on_update: true,
        can_restart,
    }
}

pub fn overlay_update_begin(total_cmds: usize) -> OverlayUpdateView {
    overlay_view(
        update_progress_pct(0, total_cmds),
        "Updating…".into(),
        true,
        false,
    )
}

pub fn overlay_update_progress(
    done_cmds: usize,
    total_cmds: usize,
    label: &str,
) -> OverlayUpdateView {
    overlay_view(
        update_progress_pct(done_cmds, total_cmds),
        label.to_string(),
        true,
        false,
    )
}

pub fn overlay_update_finish(ok: bool, last_pct: u8) -> OverlayUpdateView {
    if ok {
        overlay_view(100, "Update finished — restart GrokHub".into(), false, true)
    } else {
        overlay_view(last_pct, "Update failed".into(), false, false)
    }
}

pub fn overlay_update_current(status: &str) -> OverlayUpdateView {
    overlay_view(100, status.to_string(), false, false)
}

pub fn overlay_update_can_restart(finished_ok: bool, running: bool) -> bool {
    finished_ok && !running
}

/// Prefer the user overlay binary so a running (deleted) inode is not relaunched.
pub fn restart_bin(home: Option<&str>, current_exe: Option<&str>) -> String {
    if let Some(home) = home.map(str::trim).filter(|s| !s.is_empty()) {
        let overlay = std::path::Path::new(home).join(".local/bin/grokhub");
        if overlay.is_file() {
            return overlay.to_string_lossy().into_owned();
        }
    }
    current_exe
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("grokhub")
        .to_string()
}

pub fn restart_argv(exe: &str, hidden: bool) -> Vec<String> {
    if hidden {
        vec![exe.to_string(), "--agent".into()]
    } else {
        vec![exe.to_string()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartAct {
    Systemd { units: Vec<String> },
    Spawn { argv: Vec<String> },
}

/// Restart hands and hub units if they are live, then spawn a new cabin.
///
/// Never `systemctl restart grokhub.service` from inside the cabin: that
/// deadlocks (systemd waits for us, we wait for systemctl). The running
/// process must drop `cabin.pid` before the spawn, then exit, or the child
/// only raises the old window.
pub fn restart_acts(hub_unit: bool, hands_unit: bool, exe: &str, hidden: bool) -> Vec<RestartAct> {
    let mut acts = Vec::new();
    let mut units = Vec::new();
    if hands_unit {
        units.push("ydotoold.service".into());
    }
    if hub_unit {
        units.push("grokhub-hub.service".into());
    }
    if !units.is_empty() {
        acts.push(RestartAct::Systemd { units });
    }
    acts.push(RestartAct::Spawn {
        argv: restart_argv(exe, hidden),
    });
    acts
}

pub fn systemd_user_restart_args(units: &[String]) -> Vec<String> {
    let mut args = vec!["--user".into(), "restart".into()];
    args.extend(units.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn source_and_overlay_plan() {
        let root = std::env::temp_dir().join(format!("grokhub-src-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("crates/grokhub-app")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("scripts/install.sh"), "#!/bin/sh\n").unwrap();
        assert!(is_grokhub_source(&root));
        assert!(!is_grokhub_source(&std::env::temp_dir()));
        assert_eq!(walk_up_source(&root.join("crates/grokhub-app")), Some(root.clone()));
        assert_eq!(discover_source(&[root.join("crates")]), Some(root.clone()));
        let err = update_cmds(&root).unwrap_err();
        assert!(err.contains("not on a branch") || err.contains("main"), "{err}");
        assert!(update_wipes_config(&[
            "rm -rf ~/.config/GrokHub".into()
        ]));
        assert!(update_cmds(Path::new("/tmp/not-grokhub")).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    fn seed_git_source(root: &std::path::Path, branch: &str) {
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("crates/grokhub-app")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("scripts/install.sh"), "#!/bin/sh\n").unwrap();
        let run = |args: &[&str]| {
            let st = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .status()
                .unwrap();
            assert!(st.success(), "git {args:?}");
        };
        run(&["init", "-b", branch]);
        run(&["config", "user.email", "cabin@test"]);
        run(&["config", "user.name", "Cabin"]);
        run(&["add", "."]);
        run(&["commit", "-m", "seed"]);
    }

    fn git_in(root: &std::path::Path, args: &[&str]) {
        let st = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    fn attach_bare_origin(root: &std::path::Path, bare: &std::path::Path) {
        let _ = fs::remove_dir_all(bare);
        assert!(Command::new("git")
            .args(["init", "--bare"])
            .arg(bare)
            .status()
            .unwrap()
            .success());
        git_in(root, &["remote", "add", "origin", &bare.display().to_string()]);
        git_in(root, &["push", "-u", "origin", "main"]);
        assert!(Command::new("git")
            .args(["-C", &bare.display().to_string(), "symbolic-ref", "HEAD", "refs/heads/main"])
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn legacy_github_urls_match_this_repo_only() {
        assert!(is_legacy_github_remote(
            "https://github.com/blackviperxiii-ui/Grok-Hub.git"
        ));
        assert!(is_legacy_github_remote(
            "https://github.com/blackviperxiii-ui/grok-hub"
        ));
        assert!(is_legacy_github_remote(
            "git@github.com:blackviperxiii-ui/Grok-Hub.git"
        ));
        assert!(is_legacy_github_remote(
            "https://x-access-token:secret@github.com/blackviperxiii-ui/grok-hub.git"
        ));
        assert!(!is_legacy_github_remote(
            "https://github.com/other/Grok-Hub.git"
        ));
        assert!(!is_legacy_github_remote(GROKHUB_ORIGIN_REMOTE));
        assert!(!is_legacy_github_remote("https://example.invalid/grokhub.git"));
    }

    #[test]
    fn auth_errors_map_to_origin_login() {
        assert!(looks_like_origin_auth_error("fatal: Authentication failed"));
        assert!(looks_like_origin_auth_error("HTTP 401"));
        assert!(looks_like_origin_auth_error("remote: 403 Forbidden"));
        assert!(looks_like_origin_auth_error(
            "fatal: could not read Username for 'https://origin.cursor.com'"
        ));
        assert!(looks_like_origin_auth_error("Permission denied (publickey)"));
        assert!(!looks_like_origin_auth_error("Already up to date."));
    }

    #[test]
    fn update_requires_main_and_refuses_dirty() {
        let root = std::env::temp_dir().join(format!("grokhub-src-main-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        seed_git_source(&root, "dev");
        let err = update_cmds(&root).unwrap_err();
        assert!(err.contains("main"), "{err}");
        assert!(err.contains("dev"), "{err}");
        std::process::Command::new("git")
            .args(["checkout", "-B", "main"])
            .current_dir(&root)
            .status()
            .unwrap();
        fs::write(root.join("dirty.txt"), "nope\n").unwrap();
        let dirty = plan_update(&root).unwrap_err();
        assert!(dirty.contains("local changes"), "{dirty}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn origin_remote_is_added_or_retargeted() {
        let root = std::env::temp_dir().join(format!("grokhub-src-remote-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        seed_git_source(&root, "main");
        assert_eq!(
            ensure_origin_remote(&root).unwrap(),
            OriginRemoteAct::Added
        );
        assert_eq!(
            source_origin_url(&root).as_deref(),
            Some(GROKHUB_ORIGIN_REMOTE)
        );
        assert_eq!(
            ensure_origin_remote(&root).unwrap(),
            OriginRemoteAct::Unchanged
        );

        git_in(
            &root,
            &[
                "remote",
                "set-url",
                "origin",
                "https://github.com/blackviperxiii-ui/Grok-Hub.git",
            ],
        );
        assert_eq!(
            ensure_origin_remote(&root).unwrap(),
            OriginRemoteAct::Retargeted
        );
        assert_eq!(
            source_origin_url(&root).as_deref(),
            Some(GROKHUB_ORIGIN_REMOTE)
        );

        git_in(
            &root,
            &["remote", "set-url", "origin", "https://example.invalid/fork.git"],
        );
        assert_eq!(
            ensure_origin_remote(&root).unwrap(),
            OriginRemoteAct::Unchanged
        );
        assert_eq!(
            source_origin_url(&root).as_deref(),
            Some("https://example.invalid/fork.git")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_skips_overlay_when_already_current() {
        let pid = std::process::id();
        let root = std::env::temp_dir().join(format!("grokhub-src-tip-{pid}"));
        let bare = std::env::temp_dir().join(format!("grokhub-src-tip-{pid}.git"));
        let _ = fs::remove_dir_all(&root);
        seed_git_source(&root, "main");
        attach_bare_origin(&root, &bare);
        let plan = plan_update(&root).expect("plan");
        match plan {
            UpdatePlan::Current { status } => {
                assert!(status.contains("Already current"), "{status}");
                assert!(status.contains(env!("CARGO_PKG_VERSION")), "{status}");
            }
            UpdatePlan::Overlay { cmds } => panic!("expected current, got {cmds:?}"),
        }
        let cmds = update_cmds(&root).expect("cmds");
        assert!(cmds.is_empty(), "{cmds:?}");
        assert!(!cmds.iter().any(|c| c.contains("install.sh")));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&bare);
    }

    #[test]
    fn plan_overlays_when_behind_origin_main() {
        let pid = std::process::id();
        let root = std::env::temp_dir().join(format!("grokhub-src-behind-{pid}"));
        let bare = std::env::temp_dir().join(format!("grokhub-src-behind-{pid}.git"));
        let other = std::env::temp_dir().join(format!("grokhub-src-behind-other-{pid}"));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&other);
        seed_git_source(&root, "main");
        attach_bare_origin(&root, &bare);
        assert!(Command::new("git")
            .args([
                "clone",
                "-b",
                "main",
                &bare.display().to_string(),
                &other.display().to_string(),
            ])
            .status()
            .unwrap()
            .success());
        git_in(&other, &["config", "user.email", "cabin@test"]);
        git_in(&other, &["config", "user.name", "Cabin"]);
        fs::write(other.join("ahead.txt"), "tip\n").unwrap();
        git_in(&other, &["add", "ahead.txt"]);
        git_in(&other, &["commit", "-m", "ahead"]);
        git_in(&other, &["push", "origin", "main"]);

        let plan = plan_update(&root).expect("plan");
        let UpdatePlan::Overlay { cmds } = plan else {
            panic!("expected overlay when behind");
        };
        assert!(cmds[0].contains("merge --ff-only origin/main"), "{cmds:?}");
        assert!(cmds[1].ends_with("--user"), "{cmds:?}");
        assert!(!update_wipes_config(&cmds));
        let steps = update_plan_steps(cmds);
        assert!(steps[0].explain.contains("origin/main"), "{steps:?}");
        assert!(steps[1].explain.contains("overlay"), "{steps:?}");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&other);
        let _ = fs::remove_dir_all(&bare);
    }

    #[test]
    fn overlay_progress_is_cmd_share() {
        assert_eq!(update_progress_pct(0, 2), 0);
        assert_eq!(update_progress_pct(1, 2), 50);
        assert_eq!(update_progress_pct(2, 2), 100);
        assert_eq!(update_progress_pct(3, 2), 100);
        assert_eq!(update_progress_pct(0, 0), 100);
        assert_eq!(update_progress_pct(1, 3), 33);
    }

    #[test]
    fn overlay_update_stays_on_settings_and_skips_chat() {
        let start = overlay_update_begin(2);
        assert_eq!(start.pct, 0);
        assert!(start.running);
        assert!(!start.posts_chat);
        assert!(start.stay_on_update);
        assert!(!start.can_restart);
        assert!(start.status.contains("Updating"));

        let pull = overlay_update_progress(
            1,
            2,
            update_step_label("git merge --ff-only origin/main"),
        );
        assert_eq!(pull.pct, 50);
        assert!(pull.running);
        assert!(!pull.posts_chat);
        assert!(pull.stay_on_update);
        assert!(!pull.can_restart);
        assert!(pull.status.contains("Updating"));

        let ok = overlay_update_finish(true, 50);
        assert_eq!(ok.pct, 100);
        assert!(!ok.running);
        assert!(!ok.posts_chat);
        assert!(ok.stay_on_update);
        assert!(ok.can_restart);
        assert!(ok.status.contains("restart"));
        assert!(!ok.status.contains("HOST_RESULT"));
        assert!(overlay_update_can_restart(true, false));
        assert!(!overlay_update_can_restart(true, true));
        assert!(!overlay_update_can_restart(false, false));

        let fail = overlay_update_finish(false, 50);
        assert_eq!(fail.pct, 50);
        assert!(!fail.running);
        assert!(!fail.posts_chat);
        assert!(fail.stay_on_update);
        assert!(!fail.can_restart);
        assert!(fail.status.contains("failed"));
        assert!(!fail.status.contains("HOST_RESULT"));

        let current = overlay_update_current("Already current — v2.6.30");
        assert_eq!(current.pct, 100);
        assert!(!current.running);
        assert!(!current.can_restart);
        assert!(current.status.contains("Already current"));
        assert!(update_settings_note("2.6.30").contains("Installed v2.6.30"));
        assert!(update_settings_note("2.6.30").contains("origin auth login"));
    }

    #[test]
    fn restart_prefers_overlay_bin_and_restarts_hub_then_cabin() {
        let root = std::env::temp_dir().join(format!("grokhub-restart-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let bin = root.join(".local/bin/grokhub");
        fs::create_dir_all(bin.parent().unwrap()).unwrap();
        fs::write(&bin, "#!/bin/sh\n").unwrap();
        assert_eq!(restart_bin(Some(root.to_str().unwrap()), Some("/old/grokhub")), bin.to_string_lossy().to_string());
        assert_eq!(restart_bin(None, Some("/opt/grokhub")), "/opt/grokhub");
        assert_eq!(restart_argv("/opt/grokhub", false), vec!["/opt/grokhub".to_string()]);
        assert_eq!(
            restart_argv("/opt/grokhub", true),
            vec!["/opt/grokhub".to_string(), "--agent".into()]
        );
        assert_eq!(
            restart_acts(true, true, "/opt/grokhub", false),
            vec![
                RestartAct::Systemd {
                    units: vec!["ydotoold.service".into(), "grokhub-hub.service".into()]
                },
                RestartAct::Spawn {
                    argv: vec!["/opt/grokhub".into()]
                }
            ]
        );
        assert_eq!(
            restart_acts(true, false, "/opt/grokhub", true),
            vec![
                RestartAct::Systemd {
                    units: vec!["grokhub-hub.service".into()]
                },
                RestartAct::Spawn {
                    argv: vec!["/opt/grokhub".into(), "--agent".into()]
                }
            ]
        );
        assert_eq!(
            restart_acts(false, false, "/opt/grokhub", false),
            vec![RestartAct::Spawn {
                argv: vec!["/opt/grokhub".into()]
            }]
        );
        assert!(
            !restart_acts(true, true, "/opt/grokhub", false)
                .iter()
                .any(|a| match a {
                    RestartAct::Systemd { units } => units.iter().any(|u| u == "grokhub.service"),
                    RestartAct::Spawn { .. } => false,
                }),
            "cabin must spawn a new process, not systemctl restart grokhub.service"
        );
        assert_eq!(
            systemd_user_restart_args(&["ydotoold.service".into(), "grokhub-hub.service".into()]),
            vec![
                "--user",
                "restart",
                "ydotoold.service",
                "grokhub-hub.service"
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn overlay_step_labels_name_fetch_merge_and_install() {
        assert_eq!(
            update_step_label("git -C '/x' merge --ff-only origin/main"),
            "Updating origin/main…"
        );
        assert_eq!(
            update_step_label("git -C '/x' fetch origin main"),
            "Fetching origin/main…"
        );
        assert_eq!(
            update_step_label("git remote set-url origin https://origin.cursor.com/x"),
            "Pointing origin at Origin…"
        );
        assert_eq!(
            update_step_label("'/x/scripts/install.sh' --user"),
            "Installing overlay…"
        );
        assert_eq!(update_step_label("echo hi"), "Updating…");
    }
}
