use crate::host_plan::{explain_host_risk, host_risk, HostPlanStep, HostRisk};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

fn host_quote(s: &str) -> String {
    if cfg!(windows) {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn git_probe_timeout() -> Duration {
    // Windows Defender / cold git.exe often exceeds 2s on the first probe.
    if cfg!(windows) {
        Duration::from_secs(8)
    } else {
        Duration::from_secs(2)
    }
}

fn git_stdout(source: &Path, args: &[&str]) -> Result<(bool, String), String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(source)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("git: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let mut out = String::new();
                if let Some(stdout) = child.stdout.take() {
                    let _ = stdout.take(4096).read_to_string(&mut out);
                }
                return Ok((st.success(), out));
            }
            Ok(None) if start.elapsed() > git_probe_timeout() => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("git timed out — is the source clone reachable?".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(format!("git: {e}")),
        }
    }
}

fn git_head_branch(source: &Path) -> Result<String, String> {
    let (ok, out) = git_stdout(source, &["symbolic-ref", "-q", "--short", "HEAD"])?;
    if !ok {
        return Err("source clone is not on a branch — checkout main, then Update".into());
    }
    let branch = out.trim().to_string();
    if branch.is_empty() {
        return Err("source clone is not on a branch — checkout main, then Update".into());
    }
    Ok(branch)
}

fn git_origin_url(source: &Path) -> Result<String, String> {
    let (ok, out) = git_stdout(source, &["remote", "get-url", "origin"])?;
    if !ok {
        return Err("source clone has no origin — add origin, then Update".into());
    }
    Ok(out.trim().to_string())
}

/// Overlay pulls this GitHub remote until Cursor Origin is live.
pub const GITHUB_REMOTE_URL: &str = "https://github.com/blackviperxiii-ui/GrokHub.git";
/// Leftover Cursor Origin clone — retarget to GitHub.
pub const ORIGIN_REMOTE_URL: &str = "https://origin.cursor.com/viperxiii/GrokHub.git";

fn origin_norm(url: &str) -> String {
    let mut u = url.trim().trim_end_matches('/').to_ascii_lowercase();
    if let Some(stripped) = u.strip_suffix(".git") {
        u = stripped.to_string();
    }
    if let Some(rest) = u.strip_prefix("git@") {
        rest.replace(':', "/")
    } else {
        u
    }
}

/// Same repo as `GITHUB_REMOTE_URL` (https or ssh). Not GrokHub-Windows.
pub fn canonical_github_origin(url: &str) -> bool {
    let u = origin_norm(url);
    u.ends_with("github.com/blackviperxiii-ui/grokhub")
}

/// Any origin that is not this repo — pin before pull so overlay never
/// fetches an unvalidated remote and then runs its install script.
pub fn origin_needs_retarget(url: &str) -> bool {
    !canonical_github_origin(url)
}

/// Alias used by older call sites — same as `origin_needs_retarget`.
pub fn stale_github_origin(url: &str) -> bool {
    origin_needs_retarget(url)
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
    let src = host_quote(&source.display().to_string());
    let mut cmds = Vec::new();
    match git_origin_url(source) {
        Ok(origin) if origin_needs_retarget(&origin) => {
            cmds.push(format!(
                "git -C {src} remote set-url origin {GITHUB_REMOTE_URL}"
            ));
        }
        Ok(_) => {}
        Err(_) => {
            cmds.push(format!(
                "git -C {src} remote add origin {GITHUB_REMOTE_URL}"
            ));
        }
    }
    cmds.push(format!("git -C {src} pull --ff-only origin main"));
    cmds.push(overlay_install_cmd(source, &src));
    cmds.push(overlay_grok_update_cmd().into());
    Ok(cmds)
}

/// Windows Setup users have no clone and no cargo. Download the latest zip.
pub fn update_cmds_for(source: Option<&Path>) -> Result<Vec<String>, String> {
    match source {
        Some(src) => update_cmds(src),
        None if cfg!(windows) => Ok(windows_release_update_cmds()),
        None => Err("not a GrokHub source tree — set Settings → source or GROKHUB_SRC".into()),
    }
}

pub fn windows_release_update_cmds() -> Vec<String> {
    vec![
        windows_release_overlay_cmd().into(),
        windows_grok_update_cmd().into(),
    ]
}

/// Embedded so a Setup.exe install can update without a source tree.
pub fn windows_release_overlay_cmd() -> &'static str {
    concat!(
        "$ErrorActionPreference='Stop'; ",
        "$dest = Join-Path $env:LOCALAPPDATA 'Programs\\GrokHub'; ",
        "New-Item -ItemType Directory -Path $dest -Force | Out-Null; ",
        "$rel = Invoke-RestMethod -Uri 'https://api.github.com/repos/blackviperxiii-ui/GrokHub/releases/latest' -Headers @{ 'User-Agent'='GrokHub' }; ",
        "$asset = @($rel.assets | Where-Object { $_.name -like 'grokhub-windows-v*.zip' })[0]; ",
        "if (-not $asset) { throw 'no grokhub-windows zip on latest GitHub Release' }; ",
        "$zip = Join-Path $env:TEMP $asset.name; ",
        "Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip; ",
        "$stage = Join-Path $env:TEMP 'grokhub-windows-overlay'; ",
        "if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }; ",
        "Expand-Archive -Path $zip -DestinationPath $stage -Force; ",
        "$exe = Get-ChildItem -Path $stage -Recurse -Filter grokhub.exe | Select-Object -First 1; ",
        "$hub = Get-ChildItem -Path $stage -Recurse -Filter grokhub-hub.exe | Select-Object -First 1; ",
        "if (-not $exe -or -not $hub) { throw 'zip missing grokhub.exe' }; ",
        "Copy-Item $exe.FullName (Join-Path $dest 'grokhub.exe') -Force; ",
        "Copy-Item $hub.FullName (Join-Path $dest 'grokhub-hub.exe') -Force; ",
        "Write-Output \"overlay $dest\""
    )
}

pub fn windows_grok_update_cmd() -> &'static str {
    r#"$env:PATH = "$env:USERPROFILE\.grok\bin;$env:PATH"; grok update --alpha"#
}

pub fn settings_update_note() -> &'static str {
    if cfg!(windows) {
        "Downloads the latest Windows zip from GitHub into %LOCALAPPDATA%\\Programs\\GrokHub, then runs grok update --alpha. A source clone on main overlays with install-windows.ps1 instead. Does not wipe %APPDATA%\\GrokHub."
    } else {
        "Pulls origin/main, overlays the GUI, then runs grok update on the current channel (does not switch alpha/stable). The clone must be on main. Does not wipe ~/.config/GrokHub."
    }
}

pub fn settings_update_action_hint() -> &'static str {
    if cfg!(windows) {
        "Latest GitHub zip, or overlay a source clone, then grok update --alpha."
    } else {
        "Pulls this clone, overlays the GUI, and updates grok."
    }
}

fn overlay_install_cmd(source: &Path, src_quoted: &str) -> String {
    if cfg!(windows) {
        // run_host is already PowerShell -Command. Do not nest powershell.exe -File
        // or the script's exit code is lost.
        let install = host_quote(
            &source
                .join("scripts")
                .join("install-windows.ps1")
                .display()
                .to_string(),
        );
        format!("& {install}")
    } else {
        format!("{src_quoted}/scripts/install.sh --user")
    }
}

fn overlay_grok_update_cmd() -> &'static str {
    // Linux overlay stays on the current channel. Windows first-run / installer
    // may still vendor alpha; do not force --alpha here on Unix.
    if cfg!(windows) {
        windows_grok_update_cmd()
    } else {
        "grok update"
    }
}

pub fn update_plan_steps(cmds: Vec<String>) -> Vec<HostPlanStep> {
    cmds.into_iter()
        .map(|cmd| {
            let explain = if cmd.contains("pull --ff-only") {
                "fast-forward origin/main — config stays".into()
            } else if cmd.contains("remote set-url") || cmd.contains("remote add") {
                "point origin at GitHub — Cursor Origin is not live yet".into()
            } else if cmd.contains("releases/latest") || cmd.contains("grokhub-windows-v") {
                "download latest Windows cabin from GitHub — does not wipe config".into()
            } else if cmd.contains("install.sh") || cmd.contains("install-windows.ps1") {
                if cfg!(windows) {
                    "overlay %LOCALAPPDATA%\\Programs\\GrokHub — does not wipe config".into()
                } else {
                    "overlay ~/.local/bin — does not wipe config".into()
                }
            } else if grok_cli_update_cmd(&cmd) {
                if cmd.contains("--alpha") {
                    "update Grok Build CLI on the alpha channel".into()
                } else {
                    "update Grok Build CLI on the current channel".into()
                }
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

pub fn grok_cli_update_cmd(cmd: &str) -> bool {
    let t = cmd.trim();
    t == "grok update"
        || t.starts_with("grok update ")
        || t.ends_with("/grok update")
        || t.contains("grok update --alpha")
}

pub fn update_step_label(cmd: &str) -> &'static str {
    if cmd.contains("pull --ff-only") {
        "Pulling origin/main…"
    } else if cmd.contains("remote set-url") || cmd.contains("remote add") {
        "Retargeting origin…"
    } else if cmd.contains("releases/latest") || cmd.contains("grokhub-windows-v") {
        "Downloading latest Windows cabin…"
    } else if cmd.contains("install.sh") || cmd.contains("install-windows.ps1") {
        "Installing overlay…"
    } else if grok_cli_update_cmd(cmd) {
        "Updating Grok Build CLI…"
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
    if cfg!(windows) {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let overlay = std::path::Path::new(&local)
                .join("Programs")
                .join("GrokHub")
                .join("grokhub.exe");
            if overlay.is_file() {
                return overlay.to_string_lossy().into_owned();
            }
        }
    } else if let Some(home) = home.map(str::trim).filter(|s| !s.is_empty()) {
        let overlay = std::path::Path::new(home)
            .join(".local")
            .join("bin")
            .join("grokhub");
        if overlay.is_file() {
            return overlay.to_string_lossy().into_owned();
        }
    }
    current_exe
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(if cfg!(windows) { "grokhub.exe" } else { "grokhub" })
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

/// Restart hub if live, then spawn a new cabin. Grok Build owns computer-use;
/// do not restart ydotoold.
pub fn restart_acts(hub_unit: bool, _hands_unit: bool, exe: &str, hidden: bool) -> Vec<RestartAct> {
    let mut acts = Vec::new();
    let mut units = Vec::new();
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

/// Overlay stop targets: systemd MainPID of grokhub.service, then leftover cabin.pid.
/// Do not glob the process table by name — that matches this process.
pub fn overlay_stop_targets(main_pid: Option<u32>, cabin_pid: Option<u32>) -> Vec<u32> {
    let mut out = Vec::new();
    if let Some(pid) = main_pid.filter(|p| *p != 0) {
        out.push(pid);
    }
    if let Some(pid) = cabin_pid.filter(|p| *p != 0 && !out.contains(p)) {
        out.push(pid);
    }
    out
}

pub fn systemd_user_restart_args(units: &[String]) -> Vec<String> {
    let mut args = vec!["--user".into(), "restart".into()];
    args.extend(units.iter().cloned());
    args
}

pub fn systemd_user_stop_args(unit: &str) -> Vec<String> {
    vec!["--user".into(), "stop".into(), unit.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn assert_overlay_install(cmd: &str) {
        #[cfg(windows)]
        {
            assert!(
                cmd.contains("install-windows.ps1")
                    && cmd.trim_start().starts_with('&')
                    && !cmd.contains("powershell.exe"),
                "Windows overlay must invoke the ps1 inside host PowerShell: {cmd}"
            );
        }
        #[cfg(unix)]
        {
            assert!(
                cmd.contains("install.sh") && cmd.contains("--user"),
                "{cmd}"
            );
        }
    }

    fn assert_overlay_grok(cmd: &str) {
        #[cfg(windows)]
        {
            assert_eq!(cmd, windows_grok_update_cmd());
            assert!(cmd.contains("grok update --alpha"), "{cmd}");
        }
        #[cfg(unix)]
        assert_eq!(cmd, "grok update");
    }

    #[test]
    fn overlay_stop_uses_mainpid_and_cabin_pid_never_pgrep() {
        assert_eq!(overlay_stop_targets(None, None), Vec::<u32>::new());
        assert_eq!(overlay_stop_targets(Some(0), Some(0)), Vec::<u32>::new());
        assert_eq!(overlay_stop_targets(Some(42), None), vec![42]);
        assert_eq!(overlay_stop_targets(None, Some(7)), vec![7]);
        assert_eq!(overlay_stop_targets(Some(42), Some(42)), vec![42]);
        assert_eq!(overlay_stop_targets(Some(42), Some(7)), vec![42, 7]);
        let prod = include_str!("update.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("prod");
        assert!(
            !prod.contains("pgrep "),
            "overlay restart must never pgrep: {prod}"
        );
        let install = include_str!("../../../scripts/install.sh");
        assert!(
            !install.contains("pgrep grokhub"),
            "install.sh must never pgrep grokhub: {install}"
        );
    }

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

    #[test]
    fn git_probes_must_time_out() {
        let src = include_str!("update.rs");
        let git = src
            .split("fn git_stdout(")
            .nth(1)
            .and_then(|s| s.split("fn git_head_branch(").next())
            .expect("git_stdout");
        assert!(
            git.contains("try_wait") && !git.contains(".output()"),
            "Settings Update git must not hang the cabin: {git}"
        );
        assert!(
            git.contains("CREATE_NO_WINDOW") && git.contains("creation_flags"),
            "Windows git probes must not flash a console: {git}"
        );
        let head = src
            .split("fn git_head_branch(")
            .nth(1)
            .and_then(|s| s.split("fn git_origin_url(").next())
            .expect("git_head_branch");
        assert!(
            head.contains("git_stdout(") && !head.contains(".output()"),
            "git HEAD probe must time out: {head}"
        );
        let origin = src
            .split("fn git_origin_url(")
            .nth(1)
            .and_then(|s| s.split("pub const GITHUB_REMOTE_URL").next())
            .expect("git_origin_url");
        assert!(
            origin.contains("git_stdout(") && !origin.contains(".output()"),
            "git origin probe must time out: {origin}"
        );
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
        let no_origin = update_cmds(&root).unwrap();
        assert!(
            no_origin[0].contains("remote add origin https://github.com/blackviperxiii-ui/GrokHub.git"),
            "{no_origin:?}"
        );
        assert!(no_origin[1].contains("pull --ff-only origin main"), "{no_origin:?}");
        assert_overlay_install(&no_origin[2]);
        assert_overlay_grok(no_origin.last().unwrap());
        std::process::Command::new("git")
            .args(["remote", "add", "origin", "https://example.invalid/grokhub.git"])
            .current_dir(&root)
            .status()
            .unwrap();
        let cmds = update_cmds(&root).unwrap();
        assert!(
            cmds[0].contains("remote set-url origin https://github.com/blackviperxiii-ui/GrokHub.git"),
            "{cmds:?}"
        );
        assert!(cmds[1].contains("pull --ff-only origin main"), "{cmds:?}");
        assert_overlay_install(&cmds[2]);
        assert_overlay_grok(cmds.last().unwrap());
        #[cfg(unix)]
        assert!(!cmds.iter().any(|c| c.contains("--alpha") || c.contains("--stable")), "{cmds:?}");
        assert!(!update_wipes_config(&cmds));
        let plan = update_plan_steps(cmds);
        assert!(plan[0].explain.contains("GitHub"), "{plan:?}");
        assert!(plan[1].explain.contains("origin/main"), "{plan:?}");
        assert!(plan[2].explain.contains("overlay"), "{plan:?}");
        assert!(plan.last().unwrap().explain.contains("Grok Build CLI"), "{plan:?}");
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
        let root = std::env::temp_dir().join(format!(
            "grokhub-restart-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        #[cfg(unix)]
        {
            let bin = root.join(".local").join("bin").join("grokhub");
            fs::create_dir_all(bin.parent().unwrap()).unwrap();
            fs::write(&bin, "#!/bin/sh\n").unwrap();
            assert_eq!(
                restart_bin(Some(root.to_str().unwrap()), Some("/old/grokhub")),
                bin.to_string_lossy().to_string()
            );
            assert_eq!(restart_bin(None, Some("/opt/grokhub")), "/opt/grokhub");
        }
        #[cfg(windows)]
        {
            let prev = std::env::var_os("LOCALAPPDATA");
            std::env::set_var("LOCALAPPDATA", &root);
            let bin = root.join("Programs").join("GrokHub").join("grokhub.exe");
            fs::create_dir_all(bin.parent().unwrap()).unwrap();
            fs::write(&bin, b"MZ").unwrap();
            assert_eq!(
                restart_bin(None, Some(r"C:\old\grokhub.exe")),
                bin.to_string_lossy().to_string()
            );
            assert_eq!(
                restart_bin(None, Some(r"C:\opt\grokhub.exe")),
                bin.to_string_lossy().to_string(),
                "Windows overlay must win over current_exe"
            );
            match prev {
                Some(v) => std::env::set_var("LOCALAPPDATA", v),
                None => std::env::remove_var("LOCALAPPDATA"),
            }
        }
        assert_eq!(restart_argv("/opt/grokhub", false), vec!["/opt/grokhub".to_string()]);
        assert_eq!(
            restart_argv("/opt/grokhub", true),
            vec!["/opt/grokhub".to_string(), "--agent".into()]
        );
        assert_eq!(
            restart_acts(true, true, "/opt/grokhub", false),
            vec![
                RestartAct::Systemd {
                    units: vec!["grokhub-hub.service".into()]
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
        assert_eq!(
            systemd_user_stop_args("grokhub-hub.service"),
            vec!["--user".to_string(), "stop".into(), "grokhub-hub.service".into()]
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
        assert_eq!(update_step_label("grok update"), "Updating Grok Build CLI…");
        assert_eq!(
            update_step_label(windows_release_overlay_cmd()),
            "Downloading latest Windows cabin…"
        );
        assert!(grok_cli_update_cmd("grok update"));
        assert!(grok_cli_update_cmd(windows_grok_update_cmd()));
        assert!(!grok_cli_update_cmd("echo grok update"));
        assert_eq!(update_step_label("echo hi"), "Updating…");
        assert_eq!(
            update_step_label("git -C '/x' remote set-url origin https://github.com/blackviperxiii-ui/GrokHub.git"),
            "Retargeting origin…"
        );
    }

    #[test]
    fn overlay_retargets_origin_clone_to_github() {
        assert!(origin_needs_retarget(
            "https://github.com/blackviperxiii-ui/Grok-Hub.git"
        ));
        assert!(origin_needs_retarget(ORIGIN_REMOTE_URL));
        assert!(!origin_needs_retarget(GITHUB_REMOTE_URL));
        assert!(!origin_needs_retarget(
            "git@github.com:blackviperxiii-ui/GrokHub.git"
        ));
        assert!(origin_needs_retarget(
            "https://github.com/blackviperxiii-ui/GrokHub-Windows.git"
        ));
        assert!(!canonical_github_origin(
            "https://github.com/blackviperxiii-ui/GrokHub-Windows.git"
        ));
        assert!(origin_needs_retarget("https://example.invalid/grokhub.git"));
        assert!(canonical_github_origin(GITHUB_REMOTE_URL));
        assert!(canonical_github_origin(
            "git@github.com:blackviperxiii-ui/GrokHub.git"
        ));

        let root = std::env::temp_dir().join(format!("grokhub-src-gh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        seed_git_source(&root, "main");
        std::process::Command::new("git")
            .args(["remote", "add", "origin", ORIGIN_REMOTE_URL])
            .current_dir(&root)
            .status()
            .unwrap();
        let cmds = update_cmds(&root).unwrap();
        assert!(
            cmds[0].contains("remote set-url origin https://github.com/blackviperxiii-ui/GrokHub.git"),
            "{cmds:?}"
        );
        assert!(cmds[1].contains("pull --ff-only origin main"), "{cmds:?}");
        assert_overlay_install(&cmds[2]);
        assert_overlay_grok(cmds.last().unwrap());
        let plan = update_plan_steps(cmds);
        assert!(plan[0].explain.contains("GitHub"), "{plan:?}");

        std::process::Command::new("git")
            .args([
                "remote",
                "set-url",
                "origin",
                "https://github.com/blackviperxiii-ui/GrokHub-Windows.git",
            ])
            .current_dir(&root)
            .status()
            .unwrap();
        let from_windows = update_cmds(&root).unwrap();
        assert!(
            from_windows[0].contains(
                "remote set-url origin https://github.com/blackviperxiii-ui/GrokHub.git"
            ),
            "{from_windows:?}"
        );
        assert!(!from_windows
            .iter()
            .any(|c| c.contains("GrokHub-Windows.git")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn windows_can_update_without_a_clone() {
        let none = update_cmds_for(None);
        #[cfg(windows)]
        {
            let cmds = none.expect("windows release plan");
            assert!(
                cmds[0].contains("releases/latest") && cmds[0].contains("grokhub-windows-v"),
                "{cmds:?}"
            );
            assert_overlay_grok(cmds.last().unwrap());
            assert!(!update_wipes_config(&cmds));
            let plan = update_plan_steps(cmds);
            assert!(
                plan[0].explain.contains("GitHub") && plan[0].explain.contains("Windows"),
                "{plan:?}"
            );
            assert!(plan.last().unwrap().explain.contains("alpha"), "{plan:?}");
        }
        #[cfg(unix)]
        {
            let err = none.unwrap_err();
            assert!(
                err.contains("source") || err.contains("GROKHUB_SRC"),
                "{err}"
            );
        }
        let rel = windows_release_overlay_cmd();
        assert!(
            rel.contains("api.github.com/repos/blackviperxiii-ui/GrokHub/releases/latest")
                && rel.contains("grokhub-windows-v*.zip")
                && rel.contains("LOCALAPPDATA")
                && rel.contains("Programs\\GrokHub")
                && rel.contains("Expand-Archive")
                && rel.contains("grokhub.exe")
                && !rel.contains("cargo build")
                && !rel.contains("GROK_CHANNEL=alpha"),
            "{rel}"
        );
        assert_eq!(
            windows_grok_update_cmd(),
            r#"$env:PATH = "$env:USERPROFILE\.grok\bin;$env:PATH"; grok update --alpha"#
        );
        assert!(settings_update_note().contains(if cfg!(windows) {
            "grok update --alpha"
        } else {
            "current channel"
        }));
        #[cfg(unix)]
        assert!(
            !settings_update_note().contains("--alpha"),
            "{}",
            settings_update_note()
        );
        let unix = include_str!("update.rs")
            .split("fn overlay_grok_update_cmd(")
            .nth(1)
            .and_then(|s| s.split("pub fn update_plan_steps(").next())
            .expect("overlay_grok_update_cmd");
        assert!(
            unix.contains("\"grok update\"")
                && unix.contains("do not force --alpha here on Unix"),
            "Linux cabin /update must stay current-channel: {unix}"
        );
    }
}
