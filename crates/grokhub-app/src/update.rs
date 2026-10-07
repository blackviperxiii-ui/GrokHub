use crate::config;
use crate::host::run_host;
use grokhub_core::{
    auto_off_target, channel_status_line, channel_switch_fail_hint, discover_source,
    fetch_channel_tips, forbidden_reason, parse_github_latest_tag, switch_clone_to_main,
    update_fail_hint,
    parse_installed_cli_version, parse_published_cli_alpha, restart_acts,
    restart_bin, systemd_user_restart_args, systemd_user_stop_args, update_progress_pct,
    update_step_label, update_wipes_config, Channel, RestartAct, CHANNEL_AUTO_OFF_NOTE,
    CHANNEL_RECEIPT, CLI_ALPHA_VERSION_FALLBACK, CLI_ALPHA_VERSION_URL, GITHUB_LATEST_API,
    TEXT_FILE_CAP,
};
#[cfg(any(test, windows))]
use grokhub_core::CHANNEL_WINDOWS_NOTE;
#[cfg(not(windows))]
use grokhub_core::{channel_switch_preflight, channel_switch_shell};
use std::io::Read;
use std::env;
use std::process::{Command, Stdio};
#[cfg(all(test, unix))]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

fn expand_source_hint(raw: &str) -> PathBuf {
    PathBuf::from(grokhub_core::expand_project_root(
        raw,
        grokhub_core::user_home()
            .as_ref()
            .and_then(|p| p.to_str()),
    ))
}

pub fn resolve_source(cfg_source: &str) -> Option<PathBuf> {
    let mut hints = Vec::new();
    if let Ok(e) = env::var("GROKHUB_SRC") {
        hints.push(expand_source_hint(&e));
    }
    let trimmed = cfg_source.trim();
    if !trimmed.is_empty() {
        hints.push(expand_source_hint(trimmed));
    }
    let marker = config::config_dir().join("source");
    if let Ok(p) = std::fs::read_to_string(&marker) {
        let p = p.trim();
        if !p.is_empty() {
            hints.push(expand_source_hint(p));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        hints.push(cwd);
    }
    if let Some(home) = grokhub_core::user_home() {
        hints.push(home.join("Grok-Hub"));
        hints.push(home.join("GrokHub"));
    }
    discover_source(&hints)
}

pub fn remember_source(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(config::config_dir());
    let _ = std::fs::write(config::config_dir().join("source"), dir.display().to_string());
}

/// The install channel from the `channel` receipt that `scripts/install.sh`
/// writes next to `source`. Missing or unknown reads as stable. On Windows a
/// stray `beta` receipt reads as stable too: beta is Linux-only (built from source).
pub fn installed_channel() -> Channel {
    channel_for_host(receipt_channel(), cfg!(windows))
}

/// The raw receipt, before the Windows guard.
fn receipt_channel() -> Channel {
    std::fs::read_to_string(config::config_dir().join(CHANNEL_RECEIPT))
        .map(|t| Channel::from_receipt(&t))
        .unwrap_or_default()
}

/// Windows has no beta channel yet, so any receipt there means stable.
pub fn channel_for_host(receipt: Channel, windows: bool) -> Channel {
    if windows {
        Channel::Stable
    } else {
        receipt
    }
}

pub const BETA_LINUX_ONLY_NOTE: &str = "Beta is Linux-only for now, so GrokHub updated to stable.";

/// Update note for a stray beta receipt on Windows (Update runs as stable).
pub fn beta_linux_only_note(receipt: Channel, windows: bool) -> Option<&'static str> {
    (windows && receipt == Channel::Beta).then_some(BETA_LINUX_ONLY_NOTE)
}

/// [`beta_linux_only_note`] for this host and its receipt.
pub fn stray_beta_receipt_note() -> Option<&'static str> {
    beta_linux_only_note(receipt_channel(), cfg!(windows))
}

/// The channel this binary was built for (`GROKHUB_BUILD_CHANNEL` from build.rs).
pub fn build_channel() -> Channel {
    Channel::parse(env!("GROKHUB_BUILD_CHANNEL")).unwrap_or_default()
}

/// `grokhub --version`: `GrokHub 2.10.92-beta (beta @ abc1234)` on beta.
pub fn build_version_line() -> String {
    grokhub_core::version_line(
        env!("CARGO_PKG_VERSION"),
        build_channel(),
        env!("GROKHUB_BUILD_BRANCH"),
        env!("GROKHUB_BUILD_SHA"),
    )
}

/// Labs line for the Beta channel toggle: receipt channel + build branch/SHA.
pub fn channel_labs_status() -> String {
    channel_status_line(
        installed_channel(),
        env!("GROKHUB_BUILD_BRANCH"),
        env!("GROKHUB_BUILD_SHA"),
    )
}

/// One host command: backup binaries, `install.sh --user --channel`, restore on fail.
/// Linux only — Windows Labs keeps the toggle disabled (see [`CHANNEL_WINDOWS_NOTE`]).
#[cfg(not(windows))]
pub fn channel_switch_cmds(source: &std::path::Path, target: Channel) -> Result<Vec<String>, String> {
    channel_switch_preflight(false, Some(source))?;
    let home = grokhub_core::user_home()
        .ok_or_else(|| "No home directory — cannot install".to_string())?;
    let home = home
        .to_str()
        .ok_or_else(|| "Home path is not UTF-8".to_string())?;
    Ok(vec![channel_switch_shell(
        &source.display().to_string(),
        target,
        home,
    )])
}

/// Plain-words hint for a failed Labs channel switch. Linux only, like its
/// sole caller `queue_channel_switch`; on Windows it would be dead code and
/// `clippy -D warnings` fails the windows job.
#[cfg(not(windows))]
pub fn map_channel_switch_error(raw: &str) -> String {
    channel_switch_fail_hint(raw).to_string()
}

/// Windows Labs disabled-toggle copy. Compiled only on Windows (Linux never paints it).
#[cfg(windows)]
pub fn channel_windows_note() -> &'static str {
    CHANNEL_WINDOWS_NOTE
}

pub fn channel_auto_off_note() -> &'static str {
    CHANNEL_AUTO_OFF_NOTE
}

/// Write the install channel receipt (`channel` next to `source`).
pub fn write_installed_channel(channel: Channel) -> Result<(), String> {
    let dir = config::config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("channel receipt: {e}"))?;
    std::fs::write(dir.join(CHANNEL_RECEIPT), channel.receipt())
        .map_err(|e| format!("channel receipt: {e}"))
}

/// When on Beta and beta has caught up to main — `origin/beta` and
/// `origin/main` have the same tree (squash promote + merge-commit sync) or the
/// same tip — switch back to stable for real: check the clone out on `main`
/// at `origin/main` (no rebuild — the trees match, so the next Update builds
/// main), then write `stable` so the Labs toggle reads off. Linux only;
/// Windows no-ops (channels not supported yet — see [`CHANNEL_WINDOWS_NOTE`]).
///
/// Fetches `beta` and `main` from `origin` first. A failed fetch (offline,
/// timeout) or missing ref fails safe: stay on beta and return `None`, so the
/// cabin says nothing. When the clone can't move to main (uncommitted changes,
/// another branch, a local-only main commit) it stays on beta and the receipt
/// is untouched; the returned line says why. Returns `None` when nothing changed.
pub fn try_auto_off_beta_channel(source: Option<&std::path::Path>) -> Option<String> {
    if cfg!(windows) {
        return None;
    }
    let current = installed_channel();
    if current != Channel::Beta {
        return None;
    }
    let source = source?;
    let tips = fetch_channel_tips(source).ok()?;
    let Some(Channel::Stable) = auto_off_target(current, &tips) else {
        return None;
    };
    if let Err(e) = switch_clone_to_main(source) {
        let why = match update_fail_hint(&e) {
            "Update failed" => e,
            hint => hint.to_string(),
        };
        return Some(format!(
            "Beta caught up to main, but GrokHub couldn't switch back to stable: {why}"
        ));
    }
    write_installed_channel(Channel::Stable).ok()?;
    Some(
        "Beta caught up to main — switched to stable. Re-enable Labs Beta anytime.".into(),
    )
}

/// A Labs channel switch runs `install.sh --user --channel …`.
pub fn is_channel_switch_cmd(cmd: &str) -> bool {
    cmd.contains("install.sh") && cmd.contains("--channel ")
}

/// Host timeout per update step. A channel switch can be a cold release build,
/// so it gets 40 minutes; every other step keeps 15.
pub fn host_timeout_for(cmd: &str) -> Duration {
    if is_channel_switch_cmd(cmd) {
        Duration::from_secs(2400)
    } else {
        Duration::from_secs(900)
    }
}

/// Update / channel-switch attempts, newest last, in the config dir.
pub const UPDATE_LOG: &str = "update.log";
const UPDATE_LOG_OLD: &str = "update.1.log";
const UPDATE_LOG_CAP: u64 = 256 * 1024;
const UPDATE_LOG_TAIL: usize = 60;

pub fn update_log_path() -> PathBuf {
    config::config_dir().join(UPDATE_LOG)
}

/// One log entry: UTC time, channel, kind and result, a redacted summary of
/// each command, then the last 60 lines of host output with secrets redacted.
pub fn update_log_entry(
    now_ms: u64,
    channel: Channel,
    cmds: &[String],
    ok: bool,
    output: &str,
) -> String {
    let kind = if cmds.iter().any(|c| is_channel_switch_cmd(c)) {
        "channel switch"
    } else {
        "update"
    };
    let mut entry = format!(
        "== {} · channel {} · {kind} · {}\n",
        grokhub_core::unix_ms_to_rfc3339(now_ms),
        channel.as_str(),
        if ok { "ok" } else { "failed" }
    );
    for c in cmds {
        let cmd: String = c.chars().take(240).collect();
        entry.push_str(&format!(
            "$ {}: {}\n",
            grokhub_core::update_step_label(c),
            grokhub_core::redact_secrets(&cmd)
        ));
    }
    let lines: Vec<&str> = output.lines().collect();
    let tail = lines[lines.len().saturating_sub(UPDATE_LOG_TAIL)..].join("\n");
    entry.push_str(&grokhub_core::redact_secrets(&tail));
    entry.push_str("\n\n");
    entry
}

/// Append to `dir/update.log`. Past ~256 KB the log rotates once to
/// `update.1.log` (replacing the older one) and starts fresh.
pub fn append_update_log_in(dir: &std::path::Path, entry: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(UPDATE_LOG);
    let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if len > 0 && len + entry.len() as u64 > UPDATE_LOG_CAP {
        std::fs::rename(&path, dir.join(UPDATE_LOG_OLD))?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?
        .write_all(entry.as_bytes())
}

/// Log one Update or channel-switch attempt. Best effort: a log error never
/// changes the result.
pub fn log_update_attempt(channel: Channel, cmds: &[String], ok: bool, output: &str) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let entry = update_log_entry(now_ms, channel, cmds, ok, output);
    let _ = append_update_log_in(&config::config_dir(), &entry);
}

/// Status after a failed Update or channel switch: the specific hint from the
/// host output (dirty clone, Rust too old, no clone, clone on the other
/// branch, build failed), then where the full log is.
pub fn update_failure_status(output: &str, channel_switch: bool, log: &std::path::Path) -> String {
    let hint = if channel_switch {
        channel_switch_fail_hint(output)
    } else {
        update_fail_hint(output)
    };
    let sep = if hint.ends_with('.') { " " } else { ". " };
    format!("{hint}{sep}Details: {}", log.display())
}

pub fn host_receipt_failed(receipt: &str) -> bool {
    if receipt.contains("HOST_RECEIPT: timed out")
        || receipt.contains("HOST_RECEIPT: halted")
        || receipt.contains("spawn failed")
        || receipt.contains("thread panicked")
    {
        return true;
    }
    receipt
        .lines()
        .any(|l| l.starts_with("exit ") && !l.starts_with("exit 0"))
}

pub fn run_update_cmds(cmds: &[String]) -> Result<String, String> {
    run_update_cmds_with_progress(cmds, |_, _| {})
}

pub fn run_update_cmds_with_progress(
    cmds: &[String],
    mut on_progress: impl FnMut(u8, &str),
) -> Result<String, String> {
    if update_wipes_config(cmds) {
        return Err("refusing an update that would wipe config".into());
    }
    let total = cmds.len();
    let mut out = String::new();
    on_progress(update_progress_pct(0, total), "Updating…");
    for (i, c) in cmds.iter().enumerate() {
        if let Some(why) = forbidden_reason(c) {
            return Err(why.to_string());
        }
        on_progress(update_progress_pct(i, total), update_step_label(c));
        let chunk = run_host(c, host_timeout_for(c));
        out.push_str(&chunk);
        out.push('\n');
        if host_receipt_failed(&chunk) {
            return Err(out);
        }
        on_progress(update_progress_pct(i + 1, total), update_step_label(c));
    }
    Ok(out)
}

pub fn fetch_github_latest_tag() -> Result<String, String> {
    let resp = match ureq::get(GITHUB_LATEST_API)
        .set("user-agent", "GrokHub")
        .set("accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(8))
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let mut buf = Vec::new();
            let _ = r
                .into_reader()
                .take(TEXT_FILE_CAP as u64)
                .read_to_end(&mut buf);
            return Err(format!(
                "GitHub Latest {code}: {}",
                String::from_utf8_lossy(&buf).chars().take(120).collect::<String>()
            ));
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut buf = Vec::new();
    if resp
        .into_reader()
        .take(TEXT_FILE_CAP as u64 + 1)
        .read_to_end(&mut buf)
        .is_err()
    {
        return Err("GitHub Latest response read failed".into());
    }
    if buf.len() > TEXT_FILE_CAP {
        return Err("GitHub Latest response too large".into());
    }
    let body = String::from_utf8_lossy(&buf);
    parse_github_latest_tag(&body).ok_or_else(|| "GitHub Latest has no tag_name".into())
}

pub struct UpdateProbe {
    pub cabin_tag: Option<String>,
    pub cli_alpha: Option<String>,
    pub cli_installed: Option<String>,
}

pub fn fetch_cli_alpha_version() -> Result<String, String> {
    let mut last = "Grok Build CLI alpha version unavailable".to_string();
    for url in [CLI_ALPHA_VERSION_URL, CLI_ALPHA_VERSION_FALLBACK] {
        match fetch_text_capped(url) {
            Ok(body) => match parse_published_cli_alpha(&body) {
                Some(v) => return Ok(v),
                None => last = "Grok Build CLI alpha version was not a semver".into(),
            },
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn fetch_text_capped(url: &str) -> Result<String, String> {
    let resp = match ureq::get(url)
        .set("user-agent", "GrokHub")
        .timeout(Duration::from_secs(8))
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let mut buf = Vec::new();
            let _ = r
                .into_reader()
                .take(TEXT_FILE_CAP as u64)
                .read_to_end(&mut buf);
            return Err(format!(
                "CLI alpha version {code}: {}",
                String::from_utf8_lossy(&buf).chars().take(120).collect::<String>()
            ));
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut buf = Vec::new();
    if resp
        .into_reader()
        .take(TEXT_FILE_CAP as u64 + 1)
        .read_to_end(&mut buf)
        .is_err()
    {
        return Err("CLI alpha version response read failed".into());
    }
    if buf.len() > TEXT_FILE_CAP {
        return Err("CLI alpha version response too large".into());
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn installed_cli_version() -> Option<String> {
    let bin = grokhub_acp::find_grok()?;
    let text = grokhub_acp::grok_version(&bin).ok()?;
    parse_installed_cli_version(&text)
}

/// Background check for GitHub Latest and the published CLI alpha.
/// `grok --version` stays off the UI thread.
pub fn begin_update_probe() -> std::sync::mpsc::Receiver<UpdateProbe> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(blocking_update_probe());
    });
    rx
}

/// Same three checks as the in-app probe. `grokhub --update` has no UI cache.
pub fn blocking_update_probe() -> UpdateProbe {
    UpdateProbe {
        cabin_tag: fetch_github_latest_tag().ok(),
        cli_alpha: fetch_cli_alpha_version().ok(),
        cli_installed: installed_cli_version(),
    }
}

fn unit_is_active(unit: &str) -> bool {
    let mut cmd = Command::new("systemctl");
    cmd.args(["--user", "is-active", "--quiet", unit]);
    crate::desktop::run_limited(cmd, Duration::from_millis(1500)).is_some_and(|o| o.status.success())
}

pub fn stop_user_unit(unit: &str) -> bool {
    let args = systemd_user_stop_args(unit);
    let mut cmd = Command::new("systemctl");
    cmd.args(&args);
    crate::desktop::run_limited(cmd, Duration::from_secs(3)).is_some_and(|o| o.status.success())
}

fn spawn_detached(argv: &[String]) -> Result<(), String> {
    let (bin, args) = argv.split_first().ok_or("restart argv empty")?;
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::host::hide_windows_console(&mut cmd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(|e| format!("restart spawn: {e}"))?;
    Ok(())
}

/// Drop the pid lock, start a new cabin, then exit this process.
/// `exec` would keep the old display connection and look like a partial restart.
fn replace_process(argv: &[String]) -> Result<(), String> {
    crate::tray::release_cabin_claim();
    if let Err(e) = spawn_detached(argv) {
        let _ = crate::tray::try_claim_cabin();
        return Err(e);
    }
    std::process::exit(0);
}

/// Relaunch hub/hands, then a new cabin process. Caller must persist first.
pub fn restart_system(hidden: bool) -> Result<(), String> {
    let home = grokhub_core::user_home().and_then(|p| p.into_os_string().into_string().ok());
    let current = env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let exe = restart_bin(home.as_deref(), current.as_deref());
    let acts = restart_acts(
        unit_is_active("grokhub-hub.service"),
        unit_is_active("ydotoold.service"),
        &exe,
        hidden,
    );
    for act in acts {
        match act {
            RestartAct::Systemd { units } => {
                let args = systemd_user_restart_args(&units);
                let mut cmd = Command::new("systemctl");
                cmd.args(&args);
                match crate::desktop::run_limited(cmd, Duration::from_secs(3)) {
                    Some(out) if out.status.success() => {}
                    Some(_) => return Err("systemctl --user restart failed".into()),
                    None => return Err("systemctl --user restart timed out".into()),
                }
            }
            RestartAct::Spawn { argv } => replace_process(&argv)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_source_expands_tilde() {
        let _g = crate::config::hold_test_config();
        let home = grokhub_core::user_home()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/tmp".into());
        let root = PathBuf::from(&home).join(format!(
            "grokhub-src-tilde-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("crates/grokhub-app")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("scripts/install.sh"), "#!/bin/sh\n").unwrap();
        let prev_src = env::var("GROKHUB_SRC").ok();
        env::remove_var("GROKHUB_SRC");
        let prev_cfg = env::var("GROKHUB_CONFIG").ok();
        let cfg = crate::config::test_config_root("src-tilde-cfg");
        let _ = fs::remove_dir_all(&cfg);
        env::set_var("GROKHUB_CONFIG", &cfg);
        let rest = root
            .strip_prefix(&home)
            .map(|p| p.to_string_lossy().trim_start_matches(['/', '\\']).to_string())
            .unwrap_or_else(|_| root.to_string_lossy().into_owned());
        let found = resolve_source(&format!("~/{rest}"));
        match prev_src {
            Some(v) => env::set_var("GROKHUB_SRC", v),
            None => env::remove_var("GROKHUB_SRC"),
        }
        match prev_cfg {
            Some(v) => env::set_var("GROKHUB_CONFIG", v),
            None => env::remove_var("GROKHUB_CONFIG"),
        }
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&cfg);
        assert_eq!(
            found,
            Some(root),
            "Settings source ~/… must expand before discover"
        );
    }

    #[test]
    fn resolve_prefers_grokhub_src() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("src-hint");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("crates/grokhub-app")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("scripts/install.sh"), "#!/bin/sh\n").unwrap();
        let prev = env::var("GROKHUB_SRC").ok();
        env::set_var("GROKHUB_SRC", &root);
        let found = resolve_source("");
        match prev {
            Some(v) => env::set_var("GROKHUB_SRC", v),
            None => env::remove_var("GROKHUB_SRC"),
        }
        assert_eq!(found, Some(root.clone()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_channel_receipt_defaults_to_stable_round_trips_and_steers_update_to_origin_beta() {
        let _g = crate::config::hold_test_config();
        let cfg = crate::config::test_config_root("channel-receipt");
        let _ = fs::remove_dir_all(&cfg);
        let _pin = crate::config::TestConfigDir::set(cfg.clone());
        assert_eq!(installed_channel(), Channel::Stable);
        fs::create_dir_all(&cfg).unwrap();
        fs::write(cfg.join(CHANNEL_RECEIPT), Channel::Beta.receipt()).unwrap();
        assert_eq!(fs::read_to_string(cfg.join("channel")).unwrap(), "beta\n");
        assert_eq!(receipt_channel(), Channel::Beta);
        assert_eq!(installed_channel(), channel_for_host(Channel::Beta, cfg!(windows)));
        let root = cfg.join("src");
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("crates/grokhub-app")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("scripts/install.sh"), "#!/bin/sh\n").unwrap();
        for args in [
            &["init", "-q", "-b", "beta"][..],
            &["config", "user.email", "cabin@test"],
            &["config", "user.name", "Cabin"],
            &["add", "."],
            &["commit", "-q", "-m", "seed"],
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/blackviperxiii-ui/GrokHub.git",
            ],
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success());
        }
        let plan = grokhub_core::combined_update_cmds_in(
            Some(&root),
            grokhub_core::UpdatePending::Cabin,
            receipt_channel(),
        )
        .expect("beta plan");
        assert!(
            plan.cmds[0].ends_with(" pull --ff-only origin beta"),
            "{:?}",
            plan.cmds
        );
        assert!(
            !plan.cmds.iter().any(|c| c.contains("origin main")),
            "{:?}",
            plan.cmds
        );
        fs::write(cfg.join(CHANNEL_RECEIPT), Channel::Stable.receipt()).unwrap();
        assert_eq!(fs::read_to_string(cfg.join("channel")).unwrap(), "stable\n");
        assert_eq!(installed_channel(), Channel::Stable);
        drop(_pin);
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn version_line_carries_the_build_channel_branch_and_sha() {
        let line = build_version_line();
        let parsed = grokhub_core::parse_version_line(&line).expect("parses");
        assert_eq!(parsed.channel, build_channel());
        let base = parsed.version.trim_end_matches("-beta");
        assert_eq!(base, env!("CARGO_PKG_VERSION"));
        assert!(line.starts_with("GrokHub 2.10."), "{line}");
        assert_eq!(parsed.branch, env!("GROKHUB_BUILD_BRANCH"));
        assert_eq!(parsed.sha, env!("GROKHUB_BUILD_SHA"));
    }

    #[test]
    fn receipt_fail_stops_overlay() {
        assert!(!host_receipt_failed("$ echo\nexit 0 · 3ms\nok\n"));
        assert!(host_receipt_failed("$ git\nexit 1 · 10ms\nfatal\n"));
        assert!(host_receipt_failed("$ x\nHOST_RECEIPT: timed out"));
        assert!(
            host_receipt_failed("$ c\nHOST_RECEIPT: halted\n"),
            "a halted host batch is not success"
        );
        assert!(run_update_cmds(&["rm -rf ~/.config/GrokHub".into()]).is_err());
    }

    #[test]
    fn run_update_pulls_main_then_overlay() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("upd-src");
        let bare = crate::config::test_config_root("upd-bare").with_extension("git");
        let prev_cfg = env::var("GROKHUB_CONFIG").ok();
        let cfg = crate::config::test_config_root("upd-cfg");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&bare);
        let _ = fs::remove_dir_all(&cfg);
        env::set_var("GROKHUB_CONFIG", &cfg);
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("crates/grokhub-app")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        let install = root.join("scripts/install.sh");
        fs::write(
            &install,
            "#!/bin/sh\nset -e\necho overlay-ok > \"$(dirname \"$0\")/../overlay.ok\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            let mut perm = fs::metadata(&install).unwrap().permissions();
            perm.set_mode(0o755);
            fs::set_permissions(&install, perm).unwrap();
        }
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success(),
                "{args:?}"
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "cabin@test"]);
        git(&["config", "user.name", "Cabin"]);
        git(&["add", "."]);
        git(&["commit", "-m", "seed"]);
        assert!(Command::new("git")
            .args(["init", "--bare"])
            .arg(&bare)
            .status()
            .unwrap()
            .success());
        git(&["remote", "add", "origin", &bare.display().to_string()]);
        git(&["push", "-u", "origin", "main"]);
        remember_source(&root);
        let mut cmds = grokhub_core::update_cmds(&root).expect("cmds");
        #[cfg(windows)]
        {
            assert!(
                cmds.last().is_some_and(|c| c.contains("grok update --alpha")),
                "{cmds:?}"
            );
            fs::write(
                root.join("scripts/install-windows.ps1"),
                "Set-Content -Path (Join-Path $PSScriptRoot '..\\overlay.ok') -Value overlay-ok\n",
            )
            .unwrap();
        }
        #[cfg(unix)]
        assert_eq!(
            cmds.last().map(String::as_str),
            Some(grokhub_core::unix_grok_update_cmd())
        );
        cmds.pop();
        cmds.retain(|c| !c.contains("remote set-url") && !c.contains("remote add"));
        let out = run_update_cmds(&cmds).expect("update");
        assert!(out.contains("exit 0"), "{out}");
        assert!(root.join("overlay.ok").is_file(), "{out}");
        match prev_cfg {
            Some(v) => env::set_var("GROKHUB_CONFIG", v),
            None => env::remove_var("GROKHUB_CONFIG"),
        }
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&bare);
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn overlay_reports_percent_without_chat_text() {
        let cmds = if cfg!(windows) {
            vec!["$true".into(), "$true".into()]
        } else {
            vec!["true".into(), "true".into()]
        };
        let mut ticks = Vec::new();
        let out = run_update_cmds_with_progress(&cmds, |pct, msg| {
            ticks.push((pct, msg.to_string()));
        })
        .expect("ok");
        assert!(!out.contains("HOST_RESULT"), "{out}");
        let pcts: Vec<u8> = ticks.iter().map(|(p, _)| *p).collect();
        let mut uniq = pcts.clone();
        uniq.dedup();
        assert_eq!(uniq, vec![0, 50, 100], "{ticks:?}");
        assert_eq!(*pcts.first().unwrap(), 0);
        assert_eq!(*pcts.last().unwrap(), 100);
        assert!(ticks.iter().all(|(_, m)| !m.contains("HOST_RESULT")), "{ticks:?}");
    }

    #[test]
    fn restart_system_plan_uses_overlay_when_present() {
        let home = crate::config::test_config_root("restart-home");
        let _ = std::fs::remove_dir_all(&home);
        #[cfg(unix)]
        {
            let bin = home.join(".local").join("bin").join("grokhub");
            std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
            std::fs::write(&bin, "#!/bin/sh\n").unwrap();
            assert_eq!(
                grokhub_core::restart_bin(Some(home.to_str().unwrap()), Some("/old/grokhub")),
                bin.to_string_lossy()
            );
        }
        #[cfg(windows)]
        {
            let prev = env::var_os("LOCALAPPDATA");
            env::set_var("LOCALAPPDATA", &home);
            let bin = home.join("Programs").join("GrokHub").join("grokhub.exe");
            std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
            std::fs::write(&bin, b"MZ").unwrap();
            assert_eq!(
                grokhub_core::restart_bin(None, Some(r"C:\old\grokhub.exe")),
                bin.to_string_lossy()
            );
            match prev {
                Some(v) => env::set_var("LOCALAPPDATA", v),
                None => env::remove_var("LOCALAPPDATA"),
            }
        }
        let acts = grokhub_core::restart_acts(false, false, "/opt/grokhub", true);
        assert_eq!(
            acts,
            vec![grokhub_core::RestartAct::Spawn {
                argv: vec!["/opt/grokhub".into(), "--agent".into()]
            }]
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn restart_system_spawns_then_exits() {
        let src = include_str!("update.rs");
        let start = src.find("pub fn restart_system").expect("restart_system");
        let slice = &src[start..start + 1600];
        assert!(slice.contains("replace_process"), "{slice}");
        assert!(src.contains("release_cabin_claim"), "{src}");
        assert!(src.contains("spawn_detached"), "{src}");
        assert!(src.contains("process::exit"), "{src}");
        assert!(
            !slice.contains("unit_is_active(\"grokhub.service\")"),
            "must not systemctl-restart the running cabin: {slice}"
        );
        let unit = src
            .split("fn unit_is_active(")
            .nth(1)
            .and_then(|s| s.split("\nfn spawn_detached").next())
            .expect("unit_is_active");
        assert!(
            unit.contains("run_limited(") && !unit.contains(".status()"),
            "systemctl is-active must not freeze update restart: {unit}"
        );
        assert!(
            slice.contains("run_limited("),
            "systemctl --user restart must not freeze the UI: {slice}"
        );
        let prod = src.split("#[cfg(test)]").next().expect("prod");
        assert!(
            !prod.contains("pgrep "),
            "cabin restart must never pgrep: {prod}"
        );
    }

    #[test]
    fn latest_fetch_is_capped_github_api() {
        let src = include_str!("update.rs");
        let fetch = src
            .split("pub fn fetch_github_latest_tag(")
            .nth(1)
            .and_then(|s| s.split("pub struct UpdateProbe").next())
            .expect("fetch_github_latest_tag");
        assert!(
            fetch.contains("GITHUB_LATEST_API")
                && fetch.contains("user-agent")
                && fetch.contains("GrokHub")
                && fetch.contains(".take(")
                && fetch.contains("TEXT_FILE_CAP")
                && fetch.contains("parse_github_latest_tag")
                && !fetch.contains("into_string()")
                && !fetch.contains("into_json()"),
            "Latest check must cap the GitHub body and stay in-app: {fetch}"
        );
    }

    #[test]
    fn update_probe_checks_cabin_and_cli_alpha_off_the_ui_thread() {
        let src = include_str!("update.rs");
        let probe = src
            .split("pub fn begin_update_probe(")
            .nth(1)
            .and_then(|s| s.split("fn unit_is_active(").next())
            .expect("begin_update_probe");
        assert!(
            probe.contains("thread::spawn")
                && probe.contains("fetch_github_latest_tag")
                && probe.contains("fetch_cli_alpha_version")
                && probe.contains("installed_cli_version"),
            "the 2h probe must check cabin Latest and CLI alpha off the UI thread: {probe}"
        );
        let alpha = src
            .split("pub fn fetch_cli_alpha_version(")
            .nth(1)
            .and_then(|s| s.split("fn fetch_text_capped(").next())
            .expect("fetch_cli_alpha_version");
        assert!(
            alpha.contains("CLI_ALPHA_VERSION_URL")
                && alpha.contains("CLI_ALPHA_VERSION_FALLBACK")
                && alpha.contains("parse_published_cli_alpha")
                && !alpha.contains("--stable")
                && !alpha.contains("x.ai/cli/stable"),
            "CLI notify must read the alpha version, never stable: {alpha}"
        );
        let capped = src
            .split("fn fetch_text_capped(")
            .nth(1)
            .and_then(|s| s.split("fn installed_cli_version(").next())
            .expect("fetch_text_capped");
        assert!(
            capped.contains(".take(") && capped.contains("TEXT_FILE_CAP"),
            "CLI alpha fetch must cap the body: {capped}"
        );
        let installed = src
            .split("fn installed_cli_version(")
            .nth(1)
            .and_then(|s| s.split("pub fn begin_update_probe(").next())
            .expect("installed_cli_version");
        assert!(
            installed.contains("grok_version") && installed.contains("parse_installed_cli_version"),
            "installed CLI version comes from grok --version: {installed}"
        );
    }

    #[test]
    fn channel_switch_cmds_name_install_sh_and_backup() {
        #[cfg(windows)]
        {
            // channel_switch_cmds is Linux-only; Windows Labs shows CHANNEL_WINDOWS_NOTE.
            assert_eq!(
                grokhub_core::channel_switch_preflight(true, Some(std::path::Path::new(".")))
                    .unwrap_err(),
                CHANNEL_WINDOWS_NOTE
            );
        }
        #[cfg(not(windows))]
        {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let cmds = channel_switch_cmds(&root, Channel::Beta).expect("linux cmds");
            assert_eq!(cmds.len(), 1);
            assert!(cmds[0].contains("--channel beta"), "{}", cmds[0]);
            assert!(cmds[0].contains("scripts/install.sh"), "{}", cmds[0]);
            assert!(cmds[0].contains("channel-bak"), "{}", cmds[0]);
            let stable = channel_switch_cmds(&root, Channel::Stable).expect("stable");
            assert!(stable[0].contains("--channel stable"), "{}", stable[0]);
        }
    }

    #[test]
    fn channel_labs_status_includes_receipt_channel() {
        let line = channel_labs_status();
        assert!(
            line.starts_with("stable") || line.starts_with("beta"),
            "unexpected labs status: {line}"
        );
        #[cfg(not(windows))]
        assert_eq!(
            map_channel_switch_error("error: could not compile `grokhub-app`"),
            "Build failed — previous install kept."
        );
        // Every platform: the shared hint the Linux wrapper forwards.
        assert_eq!(
            channel_switch_fail_hint("error: could not compile `grokhub-app`"),
            "Build failed — previous install kept."
        );
        assert!(!CHANNEL_WINDOWS_NOTE.is_empty());
        #[cfg(windows)]
        assert_eq!(channel_windows_note(), CHANNEL_WINDOWS_NOTE);
    }

    /// Body of `fn <name>` in `src` (from its signature to the matching close brace).
    fn fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
        let start = src.find(sig).unwrap_or_else(|| panic!("missing {sig}"));
        let open = start + src[start..].find('{').expect("fn body");
        let mut depth = 0usize;
        for (i, c) in src[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &src[start..open + i + 1];
                    }
                }
                _ => {}
            }
        }
        panic!("unbalanced body for {sig}");
    }

    /// Simulates the Windows build: a helper whose only callers sit in a
    /// `#[cfg(not(windows))]` fn must carry the same cfg, or it is dead code on
    /// Windows and `clippy -D warnings` fails the windows job (#529).
    /// CRLF-normalized so a Windows checkout reads the same source.
    #[test]
    fn linux_only_channel_helpers_are_cfg_gated_like_their_callers() {
        assert_linux_only_helpers_gated(include_str!("update.rs"), include_str!("app/mod.rs"));
    }

    fn assert_linux_only_helpers_gated(update: &str, app: &str) {
        // A Windows checkout (autocrlf) has CRLF sources; read them the same.
        let update = &update.replace("\r\n", "\n");
        let app = &app.replace("\r\n", "\n");
        let caller_sig = "fn queue_channel_switch(";
        let at = app.find(caller_sig).expect("queue_channel_switch");
        let attrs = &app[..at];
        let attrs = attrs.trim_end().rsplit_once("\n\n").map_or(attrs, |(_, a)| a);
        assert!(
            attrs.contains("#[cfg(not(windows))]"),
            "queue_channel_switch must stay Linux-only"
        );
        let caller = fn_body(app, caller_sig);
        for helper in ["map_channel_switch_error", "channel_switch_cmds"] {
            let call = format!("crate::update::{helper}(");
            let uses = app.matches(&call).count();
            assert!(uses > 0, "{helper} has no caller in app/mod.rs");
            assert_eq!(
                caller.matches(&call).count(),
                uses,
                "{helper} is called outside the Linux-only queue_channel_switch; drop its cfg"
            );
            let def = format!("pub fn {helper}(");
            let def_at = update.find(&def).unwrap_or_else(|| panic!("missing {def}"));
            let before: Vec<&str> = update[..def_at]
                .lines()
                .rev()
                .take_while(|l| l.trim_start().starts_with("#[") || l.trim_start().starts_with("///"))
                .collect();
            assert!(
                before.iter().any(|l| l.trim() == "#[cfg(not(windows))]"),
                "{helper} is only called from Linux-only code; gate it with #[cfg(not(windows))]"
            );
        }
    }

    #[test]
    fn linux_only_helper_gate_check_catches_an_ungated_helper() {
        let app = "    #[cfg(not(windows))]\n    fn queue_channel_switch(&mut self) {\n        crate::update::map_channel_switch_error(\"x\");\n        crate::update::channel_switch_cmds(1);\n    }\n";
        let gated = "#[cfg(not(windows))]\npub fn channel_switch_cmds(a) {}\n\n/// doc\n#[cfg(not(windows))]\npub fn map_channel_switch_error(raw: &str) {}\n";
        assert_linux_only_helpers_gated(gated, app);
        // Same sources as a Windows (CRLF) checkout sees them.
        assert_linux_only_helpers_gated(&gated.replace('\n', "\r\n"), &app.replace('\n', "\r\n"));
        let ungated = gated.replace("/// doc\n#[cfg(not(windows))]\n", "/// doc\n");
        let r = std::panic::catch_unwind(|| assert_linux_only_helpers_gated(&ungated, app));
        assert!(r.is_err(), "an ungated Linux-only helper must fail the check");
    }

    #[test]
    fn write_installed_channel_flips_receipt_to_stable() {
        let _g = crate::config::hold_test_config();
        let cfg = crate::config::test_config_root("channel-auto-off");
        let _ = std::fs::remove_dir_all(&cfg);
        let _pin = crate::config::TestConfigDir::set(cfg.clone());
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join(CHANNEL_RECEIPT), Channel::Beta.receipt()).unwrap();
        assert_eq!(receipt_channel(), Channel::Beta);
        write_installed_channel(Channel::Stable).unwrap();
        assert_eq!(installed_channel(), Channel::Stable);
        assert_eq!(
            std::fs::read_to_string(cfg.join(CHANNEL_RECEIPT)).unwrap(),
            "stable\n"
        );
    }

    #[test]
    fn try_auto_off_beta_noops_when_stable_or_windows_path() {
        let _g = crate::config::hold_test_config();
        let cfg = crate::config::test_config_root("channel-auto-off-stable");
        let _ = std::fs::remove_dir_all(&cfg);
        let _pin = crate::config::TestConfigDir::set(cfg.clone());
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join(CHANNEL_RECEIPT), Channel::Stable.receipt()).unwrap();
        // Not on beta → None even with a real tree.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(try_auto_off_beta_channel(Some(&root)).is_none());
        assert!(try_auto_off_beta_channel(None).is_none());
        assert_eq!(channel_auto_off_note(), CHANNEL_AUTO_OFF_NOTE);
    }

    #[test]
    fn auto_off_target_from_core_drives_receipt_policy() {
        let tips = |b: &str, m: &str, bt: &str, mt: &str| grokhub_core::ChannelTips {
            beta_sha: b.into(),
            main_sha: m.into(),
            beta_tree: bt.into(),
            main_tree: mt.into(),
        };
        let tree = "47fd2b27aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(
            auto_off_target(Channel::Beta, &tips("abcdef0", "abcdef0", "", "")),
            Some(Channel::Stable)
        );
        assert_eq!(
            auto_off_target(Channel::Beta, &tips("e79aa50f", "7266ae6a", tree, tree)),
            Some(Channel::Stable)
        );
        assert!(auto_off_target(Channel::Beta, &tips("aaaaaaa", "bbbbbbb", "", "")).is_none());
    }

    fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
        let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Bare `origin` + a GrokHub-shaped `client` clone on beta. Then, in
    /// `work`, beta gets a feature, main gets it by squash, and beta gets main
    /// back by merge commit: same tree, different SHAs, client refs stale.
    fn same_tree_clone(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir()
            .join(format!("grokhub-auto-off-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let work = base.join("work");
        fs::create_dir_all(work.join("scripts")).unwrap();
        fs::create_dir_all(work.join("crates/grokhub-app")).unwrap();
        fs::write(work.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(work.join("scripts/install.sh"), "#!/bin/sh\n").unwrap();
        fs::write(work.join("crates/grokhub-app/lib.rs"), "\n").unwrap();
        git_in(&base, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git_in(&work, &["init", "-q", "-b", "main"]);
        git_in(&work, &["config", "user.email", "cabin@test"]);
        git_in(&work, &["config", "user.name", "Cabin"]);
        git_in(&work, &["config", "commit.gpgsign", "false"]);
        git_in(&work, &["add", "."]);
        git_in(&work, &["commit", "-q", "-m", "seed"]);
        git_in(&work, &["remote", "add", "origin", &base.join("origin.git").display().to_string()]);
        git_in(&work, &["push", "-q", "origin", "main", "main:beta"]);
        git_in(&base, &["clone", "-q", "-b", "beta", "origin.git", "client"]);
        git_in(&work, &["checkout", "-q", "-b", "beta"]);
        fs::write(work.join("feature.txt"), "feature\n").unwrap();
        git_in(&work, &["add", "."]);
        git_in(&work, &["commit", "-q", "-m", "feature"]);
        git_in(&work, &["checkout", "-q", "main"]);
        git_in(&work, &["merge", "-q", "--squash", "beta"]);
        git_in(&work, &["commit", "-q", "-m", "promote (squash)"]);
        git_in(&work, &["checkout", "-q", "beta"]);
        git_in(&work, &["merge", "-q", "--no-ff", "main", "-m", "sync main into beta"]);
        git_in(&work, &["push", "-q", "origin", "main", "beta"]);
        (base.clone(), base.join("client"))
    }

    #[test]
    fn try_auto_off_beta_switches_clone_to_main_on_same_tree() {
        let _g = crate::config::hold_test_config();
        let cfg = crate::config::test_config_root("channel-auto-off-tree");
        let _ = fs::remove_dir_all(&cfg);
        let _pin = crate::config::TestConfigDir::set(cfg.clone());
        fs::create_dir_all(&cfg).unwrap();
        let (base, client) = same_tree_clone("flip");
        fs::write(cfg.join(CHANNEL_RECEIPT), Channel::Beta.receipt()).unwrap();
        let msg = try_auto_off_beta_channel(Some(&client));
        if cfg!(windows) {
            // Windows stays disabled: no fetch, no checkout, receipt untouched.
            assert!(msg.is_none());
            assert_eq!(receipt_channel(), Channel::Beta);
            assert_eq!(git_in(&client, &["symbolic-ref", "--short", "HEAD"]), "beta");
        } else {
            assert_eq!(
                msg.as_deref(),
                Some("Beta caught up to main — switched to stable. Re-enable Labs Beta anytime.")
            );
            assert_eq!(installed_channel(), Channel::Stable);
            // Really back on stable: the clone is on main, so the next Update
            // (stable plan) accepts it instead of "source clone is on beta".
            assert_eq!(git_in(&client, &["symbolic-ref", "--short", "HEAD"]), "main");
            assert_eq!(
                git_in(&client, &["rev-parse", "HEAD"]),
                git_in(&client, &["rev-parse", "origin/main"])
            );
            assert!(grokhub_core::update_cmds_in(&client, Channel::Stable).is_ok());
        }
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn try_auto_off_beta_fails_safe_offline_and_on_a_dirty_clone() {
        let _g = crate::config::hold_test_config();
        let cfg = crate::config::test_config_root("channel-auto-off-safe");
        let _ = fs::remove_dir_all(&cfg);
        let _pin = crate::config::TestConfigDir::set(cfg.clone());
        fs::create_dir_all(&cfg).unwrap();
        let (base, client) = same_tree_clone("safe");
        fs::write(cfg.join(CHANNEL_RECEIPT), Channel::Beta.receipt()).unwrap();
        // Dirty clone: stays on beta, edit kept, receipt untouched, says why.
        fs::write(client.join("Cargo.toml"), "[workspace] # wip\n").unwrap();
        let msg = try_auto_off_beta_channel(Some(&client));
        if cfg!(windows) {
            assert!(msg.is_none());
        } else {
            assert_eq!(
                msg.as_deref(),
                Some("Beta caught up to main, but GrokHub couldn't switch back to stable: Your GrokHub source folder has unsaved code changes. Save or undo them (git commit or git stash), then try again.")
            );
        }
        assert_eq!(receipt_channel(), Channel::Beta);
        assert_eq!(git_in(&client, &["symbolic-ref", "--short", "HEAD"]), "beta");
        assert_eq!(fs::read_to_string(client.join("Cargo.toml")).unwrap(), "[workspace] # wip\n");
        // Offline / fetch error: stay on beta and say nothing.
        git_in(&client, &["checkout", "-q", "--", "Cargo.toml"]);
        let gone = base.join("gone.git").display().to_string();
        git_in(&client, &["remote", "set-url", "origin", &gone]);
        assert!(try_auto_off_beta_channel(Some(&client)).is_none());
        assert_eq!(receipt_channel(), Channel::Beta);
        assert_eq!(git_in(&client, &["symbolic-ref", "--short", "HEAD"]), "beta");
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn windows_reads_a_stray_beta_receipt_as_stable_and_says_beta_is_linux_only() {
        assert_eq!(channel_for_host(Channel::Beta, true), Channel::Stable);
        assert_eq!(channel_for_host(Channel::Stable, true), Channel::Stable);
        assert_eq!(channel_for_host(Channel::Beta, false), Channel::Beta);
        assert_eq!(
            beta_linux_only_note(Channel::Beta, true),
            Some("Beta is Linux-only for now, so GrokHub updated to stable.")
        );
        assert_eq!(beta_linux_only_note(Channel::Stable, true), None);
        assert_eq!(beta_linux_only_note(Channel::Beta, false), None);
        let _g = crate::config::hold_test_config();
        let cfg = crate::config::test_config_root("channel-windows-guard");
        let _ = fs::remove_dir_all(&cfg);
        let _pin = crate::config::TestConfigDir::set(cfg.clone());
        fs::create_dir_all(&cfg).unwrap();
        fs::write(cfg.join(CHANNEL_RECEIPT), Channel::Beta.receipt()).unwrap();
        if cfg!(windows) {
            assert_eq!(installed_channel(), Channel::Stable);
            assert_eq!(stray_beta_receipt_note(), Some(BETA_LINUX_ONLY_NOTE));
        } else {
            assert_eq!(installed_channel(), Channel::Beta);
            assert_eq!(stray_beta_receipt_note(), None);
        }
        let _ = fs::remove_dir_all(&cfg);
        // Update shows the note instead of failing on a stray Windows receipt.
        let app = include_str!("app/mod.rs");
        assert!(app.contains("crate::update::stray_beta_receipt_note()"));
        let main = include_str!("main.rs");
        assert!(main.contains("update::stray_beta_receipt_note()"));
    }

    #[test]
    fn channel_switch_gets_2400s_and_other_steps_keep_900s() {
        let switch = grokhub_core::channel_switch_shell("/repo", Channel::Stable, "/home/u");
        assert!(is_channel_switch_cmd(&switch));
        assert_eq!(host_timeout_for(&switch), Duration::from_secs(2400));
        assert_eq!(
            host_timeout_for("git -C '/repo' pull --ff-only origin main"),
            Duration::from_secs(900)
        );
        assert_eq!(host_timeout_for("bash '/repo/scripts/install.sh' --user"), Duration::from_secs(900));
        let src = include_str!("update.rs");
        assert!(src.contains("let chunk = run_host(c, host_timeout_for(c));"));
    }

    #[test]
    fn failed_update_status_names_the_cause_and_the_log() {
        let log = std::path::Path::new("/home/u/.config/GrokHub/update.log");
        let dirty = "$ bash install.sh --user --channel stable\nexit 1 · 9ms\nerror: /src has uncommitted changes; commit or stash them before --channel stable";
        assert_eq!(
            update_failure_status(dirty, true, log),
            "Your GrokHub source folder has unsaved code changes. Save or undo them (git commit or git stash), then try again. Details: /home/u/.config/GrokHub/update.log"
        );
        let rustc = "exit 101 · 3s\nerror: package `eframe v0.36.2` cannot be built because it requires rustc 1.88 or newer";
        assert_eq!(
            update_failure_status(rustc, false, log),
            "GrokHub needs a newer Rust to build. Run rustup update, then try again. Details: /home/u/.config/GrokHub/update.log"
        );
        let on_main = "error: /src is on main but the beta channel builds beta; run with --channel stable to switch, or git checkout beta";
        assert!(update_failure_status(on_main, false, log).starts_with("Your GrokHub source folder is on stable, but Labs Beta is on."));
        assert_eq!(
            update_failure_status("exit 1 · 5ms\nfatal: unable to access", false, log),
            "Update failed. Details: /home/u/.config/GrokHub/update.log"
        );
        assert_eq!(
            update_failure_status("exit 1 · 5ms\nfatal: unable to access", true, log),
            "Channel switch failed — previous install kept. Details: /home/u/.config/GrokHub/update.log"
        );
        // The handler feeds the real host output, not the fixed "Update failed".
        let jobs = include_str!("app/jobs.rs");
        assert!(jobs.contains("JobOut::UpdateDone { ok, output }"));
        assert!(jobs.contains("crate::update::update_failure_status("));
        assert!(!jobs.contains("map_channel_switch_error(&view.status)"));
        let app = include_str!("app/mod.rs");
        assert!(app.contains("JobOut::UpdateDone { ok: false, output }"));
        assert!(app.contains("crate::update::log_update_attempt(channel, &cmds, false, e)"));
    }

    #[test]
    fn update_log_keeps_a_redacted_60_line_tail_and_rotates_once() {
        let mut out = String::new();
        for i in 0..100 {
            out.push_str(&format!("line {i}\n"));
        }
        out.push_str("token ghp_abcdefghijklmnop leaked\n");
        let cmds = vec![
            "git -C '/src' pull --ff-only origin beta".to_string(),
            "bash '/src/scripts/install.sh' --user".to_string(),
        ];
        let entry = update_log_entry(0, Channel::Beta, &cmds, false, &out);
        assert!(entry.starts_with("== 1970-01-01T00:00:00Z · channel beta · update · failed\n"), "{entry}");
        assert!(entry.contains("$ Pulling origin/beta…: git -C '/src' pull --ff-only origin beta\n"));
        assert!(entry.contains("$ Installing overlay…: bash '/src/scripts/install.sh' --user\n"));
        assert!(!entry.contains("line 40\n") && entry.contains("line 41\n") && entry.contains("line 99\n"));
        assert!(!entry.contains("ghp_abcdefghijklmnop") && entry.contains("[redacted]"), "{entry}");
        let switch = vec![grokhub_core::channel_switch_shell("/src", Channel::Stable, "/home/u")];
        assert!(update_log_entry(0, Channel::Beta, &switch, true, "ok").contains("· channel switch · ok"));

        let dir = std::env::temp_dir().join(format!("grokhub-update-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let big = "x".repeat(100 * 1024);
        for _ in 0..2 {
            append_update_log_in(&dir, &big).unwrap();
        }
        assert_eq!(fs::metadata(dir.join(UPDATE_LOG)).unwrap().len(), 200 * 1024);
        assert!(!dir.join("update.1.log").exists());
        append_update_log_in(&dir, &big).unwrap(); // would pass 256 KB → rotate
        assert_eq!(fs::metadata(dir.join("update.1.log")).unwrap().len(), 200 * 1024);
        assert_eq!(fs::metadata(dir.join(UPDATE_LOG)).unwrap().len(), 100 * 1024);
        append_update_log_in(&dir, &big).unwrap();
        append_update_log_in(&dir, &big).unwrap(); // rotates again, replacing the old one
        assert_eq!(fs::metadata(dir.join("update.1.log")).unwrap().len(), 200 * 1024);
        assert_eq!(fs::metadata(dir.join(UPDATE_LOG)).unwrap().len(), 100 * 1024);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2, "rotates once, never more files");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_off_stays_disabled_on_windows_before_any_fetch() {
        let src = include_str!("update.rs");
        let body = src
            .split("pub fn try_auto_off_beta_channel(")
            .nth(1)
            .and_then(|s| s.split("pub fn host_receipt_failed(").next())
            .expect("try_auto_off_beta_channel");
        let win = body.find("if cfg!(windows)").expect("windows early return");
        let fetch = body.find("fetch_channel_tips(").expect("fetch");
        assert!(win < fetch, "Windows must return before fetching: {body}");
        let app = include_str!("app/mod.rs");
        let maybe = app
            .split("fn maybe_auto_off_beta_channel(")
            .nth(1)
            .expect("maybe_auto_off_beta_channel");
        let win = maybe.find("if cfg!(windows)").expect("windows early return");
        assert!(
            win < maybe.find("installed_channel()").expect("receipt")
                && win < maybe.find("try_auto_off_beta_channel(").expect("try"),
            "maybe_auto_off_beta_channel must return first on Windows"
        );
        let settings = include_str!("app/settings.rs");
        assert!(
            settings.contains("ui.add_enabled_ui(false, |ui| {")
                && settings.contains("crate::update::channel_windows_note()"),
            "Windows Labs keeps the Beta toggle disabled with its note"
        );
    }
}
