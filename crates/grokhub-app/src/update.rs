use crate::config;
use crate::host::run_host;
use grokhub_core::{
    discover_source, forbidden_reason, restart_acts, restart_bin, systemd_user_restart_args,
    systemd_user_stop_args, update_cmds, update_progress_pct, update_step_label,
    update_wipes_config, RestartAct,
};
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
        let chunk = run_host(c, Duration::from_secs(900));
        out.push_str(&chunk);
        out.push('\n');
        if host_receipt_failed(&chunk) {
            return Err(out);
        }
        on_progress(update_progress_pct(i + 1, total), update_step_label(c));
    }
    Ok(out)
}

pub fn run_update(source: &std::path::Path) -> Result<String, String> {
    let cmds = update_cmds(source)?;
    remember_source(source);
    run_update_cmds(&cmds)
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
            assert_eq!(cmds.last().map(String::as_str), Some("grok update --alpha"));
            fs::write(
                root.join("scripts/install-windows.ps1"),
                "Set-Content -Path (Join-Path $PSScriptRoot '..\\overlay.ok') -Value overlay-ok\n",
            )
            .unwrap();
        }
        #[cfg(unix)]
        assert_eq!(cmds.last().map(String::as_str), Some("grok update"));
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
            let bin = home.join(".local/bin/grokhub");
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
}
