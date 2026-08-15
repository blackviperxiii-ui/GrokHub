use crate::config;
use crate::host::run_host;
use grokhub_core::{
    discover_source, forbidden_reason, update_cmds, update_progress_pct, update_step_label,
    update_wipes_config,
};
use std::env;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

pub fn resolve_source(cfg_source: &str) -> Option<PathBuf> {
    let mut hints = Vec::new();
    if let Ok(e) = env::var("GROKHUB_SRC") {
        hints.push(PathBuf::from(e));
    }
    let trimmed = cfg_source.trim();
    if !trimmed.is_empty() {
        hints.push(PathBuf::from(trimmed));
    }
    let marker = config::config_dir().join("source");
    if let Ok(p) = std::fs::read_to_string(&marker) {
        let p = p.trim();
        if !p.is_empty() {
            hints.push(PathBuf::from(p));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        hints.push(cwd);
    }
    if let Ok(home) = env::var("HOME") {
        hints.push(PathBuf::from(&home).join("Grok-Hub"));
        hints.push(PathBuf::from(&home).join("GrokHub"));
    }
    discover_source(&hints)
}

pub fn remember_source(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(config::config_dir());
    let _ = std::fs::write(config::config_dir().join("source"), dir.display().to_string());
}

pub fn host_receipt_failed(receipt: &str) -> bool {
    if receipt.contains("HOST_RECEIPT: timed out")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TEST_CONFIG_LOCK;
    use std::fs;

    #[test]
    fn resolve_prefers_grokhub_src() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-src-hint-{}", std::process::id()));
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
        assert!(run_update_cmds(&["rm -rf ~/.config/GrokHub".into()]).is_err());
    }

    #[test]
    fn run_update_pulls_main_then_overlay() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let pid = std::process::id();
        let root = std::env::temp_dir().join(format!("grokhub-upd-src-{pid}"));
        let bare = std::env::temp_dir().join(format!("grokhub-upd-bare-{pid}.git"));
        let prev_cfg = env::var("GROKHUB_CONFIG").ok();
        let cfg = std::env::temp_dir().join(format!("grokhub-upd-cfg-{pid}"));
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
        let out = run_update(&root).expect("update");
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
        let cmds = vec!["true".into(), "true".into()];
        let mut ticks = Vec::new();
        let out = run_update_cmds_with_progress(&cmds, |pct, msg| {
            ticks.push((pct, msg.to_string()));
        })
        .expect("ok");
        assert!(!out.contains("HOST_RESULT"), "{out}");
        let pcts: Vec<u8> = ticks.iter().map(|(p, _)| *p).collect();
        assert_eq!(pcts, vec![0, 50, 100], "{ticks:?}");
        assert!(ticks.iter().all(|(_, m)| !m.contains("HOST_RESULT")), "{ticks:?}");
    }
}
