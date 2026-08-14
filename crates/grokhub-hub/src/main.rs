//! LAN hub — same /v1 contract as grokhub --hub.

use grokhub_core::{load_hub_state, save_hub_state, HubState, DEFAULT_PORT, HUB_KIND};
use grokhub_hub::serve;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    let port = env::var("GROKHUB_HUB_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let path = persist_path();
    let mut state = load_hub_state(&path).unwrap_or_else(HubState::empty);
    state.port = port;
    state.sharing = true;
    if state.pair.is_none() {
        state.rotate_pair();
    }
    let code = state
        .pair
        .as_ref()
        .map(|p| p.code.clone())
        .unwrap_or_default();
    eprintln!(
        "grokhub-hub {} on :{port}  pair {code}  kind {HUB_KIND}",
        env!("CARGO_PKG_VERSION")
    );
    let shared = Arc::new(Mutex::new(state));
    {
        let st = shared.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if let Ok(g) = st.lock() {
                let _ = save_hub_state(&path, &g);
            }
        });
    }
    if let Err(e) = serve(shared, port) {
        eprintln!("hub failed: {e}");
        std::process::exit(1);
    }
}
