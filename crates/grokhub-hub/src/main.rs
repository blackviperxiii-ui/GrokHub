//! LAN hub — same /v1 contract as grokhub --hub.

use grokhub_core::DEFAULT_PORT;
use grokhub_hub::{parse_args, Launch};
use std::env;
use std::path::PathBuf;

const STATE_FILE: &str = "hub-state.json";

/// The hub's state lives in the same app folder as everything else
/// (`%APPDATA%\GrokHub` on Windows). Older hubs ignored `%APPDATA%` and wrote
/// under `$HOME/.config/GrokHub`; that copy is brought over once and kept.
fn persist_path() -> PathBuf {
    let path = grokhub_core::paths::config_dir().join(STATE_FILE);
    if grokhub_core::paths::env_config_dir().is_none() {
        let legacy = env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join(".config/GrokHub").join(STATE_FILE));
        adopt_legacy_state(&path, legacy.as_deref());
    }
    path
}

/// Copy `legacy` to `path` when only the old file exists. Never moves or
/// deletes the old one.
fn adopt_legacy_state(path: &std::path::Path, legacy: Option<&std::path::Path>) {
    let Some(legacy) = legacy else { return };
    if legacy == path || path.exists() || !legacy.is_file() {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::copy(legacy, path) {
        eprintln!("hub: couldn't bring over {}: {e}", legacy.display());
    }
}

fn main() {
    match parse_args(&env::args().collect::<Vec<_>>()) {
        Launch::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Launch::Help => {
            eprint!(
                "grokhub-hub {} — LAN /v1 hub\n\n  grokhub-hub           listen on GROKHUB_HUB_PORT or :18766\n  grokhub-hub --version\n  grokhub-hub --help\n",
                env!("CARGO_PKG_VERSION")
            );
        }
        Launch::Serve => {
            let port = env::var("GROKHUB_HUB_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_PORT);
            let banner = format!("grokhub-hub {}", env!("CARGO_PKG_VERSION"));
            if let Err(e) = grokhub_hub::run(port, persist_path(), &banner) {
                eprintln!("hub failed: {e}");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hub-state-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn old_hub_state_is_copied_once_and_kept() {
        let root = temp("adopt");
        let legacy = root.join("old/hub-state.json");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "{\"old\":1}").unwrap();
        let path = root.join("new/hub-state.json");
        adopt_legacy_state(&path, Some(&legacy));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"old\":1}");
        assert_eq!(std::fs::read_to_string(&legacy).unwrap(), "{\"old\":1}");
        std::fs::write(&path, "{\"new\":2}").unwrap();
        adopt_legacy_state(&path, Some(&legacy));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"new\":2}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
