//! LAN hub — same /v1 contract as grokhub --hub.

use grokhub_core::DEFAULT_PORT;
use grokhub_hub::{parse_args, Launch};
use std::env;
use std::path::PathBuf;

fn persist_path() -> PathBuf {
    if let Ok(p) = env::var("GROKHUB_CONFIG") {
        return PathBuf::from(p).join("hub-state.json");
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".config/GrokHub/hub-state.json");
    }
    PathBuf::from("hub-state.json")
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
