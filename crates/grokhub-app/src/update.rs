use crate::config;
use crate::host::run_host;
use grokhub_core::{discover_source, forbidden_reason, update_cmds, update_wipes_config};
use std::env;
use std::path::PathBuf;
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
    if update_wipes_config(cmds) {
        return Err("refusing an update that would wipe config".into());
    }
    let mut out = String::new();
    for c in cmds {
        if let Some(why) = forbidden_reason(c) {
            return Err(why.to_string());
        }
        let chunk = run_host(c, Duration::from_secs(900));
        out.push_str(&chunk);
        out.push('\n');
        if host_receipt_failed(&chunk) {
            return Err(out);
        }
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
}
