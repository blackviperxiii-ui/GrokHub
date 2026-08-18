use crate::host_plan::{explain_host_risk, host_risk, HostPlanStep, HostRisk};
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn git_head_branch(source: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(source)
        .args(["symbolic-ref", "-q", "--short", "HEAD"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err("source clone is not on a branch — checkout main, then Update".into());
    }
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if branch.is_empty() {
        return Err("source clone is not on a branch — checkout main, then Update".into());
    }
    Ok(branch)
}

fn git_has_origin(source: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(source)
        .args(["remote", "get-url", "origin"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err("source clone has no origin — add origin, then Update".into());
    }
    Ok(())
}

pub fn update_cmds(source: &Path) -> Result<Vec<String>, String> {
    if !is_grokhub_source(source) {
        return Err("not a GrokHub source tree — set Settings → source or GROKHUB_SRC".into());
    }
    let branch = git_head_branch(source)?;
    if branch != "main" {
        return Err(format!(
            "source clone is on {branch} — checkout main, then Update"
        ));
    }
    git_has_origin(source)?;
    let src = sh_quote(&source.display().to_string());
    Ok(vec![
        format!("git -C {src} pull --ff-only origin main"),
        format!("{src}/scripts/install.sh --user"),
    ])
}

pub fn update_plan_steps(cmds: Vec<String>) -> Vec<HostPlanStep> {
    cmds.into_iter()
        .map(|cmd| {
            let explain = if cmd.contains("pull --ff-only") {
                "fast-forward origin/main — config stays".into()
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
    if cmd.contains("pull --ff-only") {
        "Pulling origin/main…"
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

    #[test]
    fn update_requires_main_and_pulls_origin_main() {
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
        let no_origin = update_cmds(&root).unwrap_err();
        assert!(no_origin.contains("origin"), "{no_origin}");
        std::process::Command::new("git")
            .args(["remote", "add", "origin", "https://example.invalid/grokhub.git"])
            .current_dir(&root)
            .status()
            .unwrap();
        let cmds = update_cmds(&root).unwrap();
        assert!(cmds[0].contains("pull --ff-only origin main"), "{cmds:?}");
        assert!(cmds[1].ends_with("--user"));
        assert!(!update_wipes_config(&cmds));
        let plan = update_plan_steps(cmds);
        assert!(plan[0].explain.contains("origin/main"), "{plan:?}");
        assert!(plan[1].explain.contains("overlay"), "{plan:?}");
        assert_ne!(plan[0].explain, "read-only");
        let _ = fs::remove_dir_all(&root);
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
            update_step_label("git pull --ff-only origin main"),
        );
        assert_eq!(pull.pct, 50);
        assert!(pull.running);
        assert!(!pull.posts_chat);
        assert!(pull.stay_on_update);
        assert!(!pull.can_restart);
        assert!(pull.status.contains("Pulling"));

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
    fn overlay_step_labels_name_pull_and_install() {
        assert_eq!(
            update_step_label("git -C '/x' pull --ff-only origin main"),
            "Pulling origin/main…"
        );
        assert_eq!(
            update_step_label("'/x/scripts/install.sh' --user"),
            "Installing overlay…"
        );
        assert_eq!(update_step_label("echo hi"), "Updating…");
    }
}
