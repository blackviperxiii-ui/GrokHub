//! LAN hub — same /v1 contract as grokhub --hub — plus the headless tools
//! the desktop shell has no console for (`--mcp-self`, `--askpass`, `bench local`).

use grokhub_core::DEFAULT_PORT;
use grokhub_hub::{askpass_socket, parse_args, Launch};
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

/// What `grokhub-hub --help` prints.
fn help_text(version: &str) -> String {
    format!(
        "grokhub-hub {version} — LAN /v1 hub and headless tools\n\n  \
grokhub-hub              listen on GROKHUB_HUB_PORT or :18766 (also --hub)\n  \
grokhub-hub --mcp-self   the agent's skill, connection and automation tools on stdin\n  \
grokhub-hub bench local  on-device speed + eval\n  \
grokhub-hub --askpass <socket>  answer a sudo prompt (the app runs this)\n  \
grokhub-hub --version\n  \
grokhub-hub --help\n"
    )
}

fn main() {
    let args: Vec<String> = env::args().collect();
    // `sudo`'s helper: answer it and leave before anything else starts.
    if let Some(socket) = askpass_socket(&args) {
        std::process::exit(grokhub_agent::sudo_pass::client(socket));
    }
    match parse_args(&args) {
        Launch::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Launch::Help => {
            eprint!("{}", help_text(env!("CARGO_PKG_VERSION")));
        }
        Launch::McpSelf => {
            use_os_stores();
            let dir = grokhub_core::paths::config_dir();
            std::process::exit(grokhub_agent::self_manage::mcp::run_stdio(&dir));
        }
        Launch::BenchLocal => {
            use_os_stores();
            std::process::exit(grokhub_agent::route::local_bench::run_cli());
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

/// The same OS keyring and provider vault the apps use, so a token the
/// tools save is one the app can read.
fn use_os_stores() {
    grokhub_agent::harness::use_os_keyring();
    grokhub_agent::route::providers::use_os_vault();
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
    fn help_lists_every_tool() {
        let help = help_text("3.5.0");
        assert!(
            help.starts_with("grokhub-hub 3.5.0 — LAN /v1 hub and headless tools\n"),
            "{help}"
        );
        for line in [
            "  grokhub-hub              listen on GROKHUB_HUB_PORT or :18766 (also --hub)\n",
            "  grokhub-hub --mcp-self   the agent's skill, connection and automation tools on stdin\n",
            "  grokhub-hub bench local  on-device speed + eval\n",
            "  grokhub-hub --askpass <socket>  answer a sudo prompt (the app runs this)\n",
        ] {
            assert!(help.contains(line), "{line} missing from {help}");
        }
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
