//! The one Grok Build CLI exception: when the CLI is already installed, the
//! Marketplace points Google's MCP connectors at it. This module only looks
//! for the binary. It never runs, installs or updates the CLI, and never reads
//! its files or credentials. The CLI guard test allows CLI lookups here only.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const RECHECK: Duration = Duration::from_secs(5);

/// Is the Grok Build CLI on this machine? Rechecked at most every few seconds.
pub fn cli_installed() -> bool {
    static SEEN: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut held = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, found)) = *held {
        if at.elapsed() < RECHECK {
            return found;
        }
    }
    let found = locate().is_some();
    *held = Some((Instant::now(), found));
    found
}

fn bin_name() -> &'static str {
    if cfg!(windows) {
        "grok.exe"
    } else {
        "grok"
    }
}

fn usable(path: PathBuf) -> Option<PathBuf> {
    std::fs::metadata(&path)
        .is_ok_and(|m| m.is_file() && m.len() > 0)
        .then_some(path)
}

/// `GROKHUB_GROK` (an explicit path wins, even when it is missing), then PATH,
/// then the CLI's own install folders.
fn locate() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("GROKHUB_GROK") {
        return usable(PathBuf::from(p));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        if let Some(p) = std::env::split_paths(&paths).find_map(|dir| usable(dir.join(bin_name()))) {
            return Some(p);
        }
    }
    let home = grokhub_core::user_home()?;
    let dirs: &[&str] = if cfg!(windows) { &[".grok/bin"] } else { &[".local/bin", ".grok/bin"] };
    dirs.iter().find_map(|rel| usable(Path::new(&home).join(rel).join(bin_name())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_path_decides_and_nothing_is_run() {
        let _lock = crate::config::hold_test_config();
        let dir = std::env::temp_dir().join(format!("gh-gcli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join(bin_name());
        let prev = std::env::var_os("GROKHUB_GROK");
        std::env::set_var("GROKHUB_GROK", dir.join("missing"));
        assert_eq!(locate(), None, "a missing explicit path does not fall back to PATH");
        std::fs::write(&bin, "not a program; reading it would fail").unwrap();
        std::env::set_var("GROKHUB_GROK", &bin);
        assert_eq!(locate(), Some(bin.clone()));
        match prev {
            Some(v) => std::env::set_var("GROKHUB_GROK", v),
            None => std::env::remove_var("GROKHUB_GROK"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        let src = include_str!("google_mcp_via_cli.rs").replace("\r\n", "\n");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(!code.contains("Command::new") && !code.contains("read_to_string"), "{code}");
    }
}
