//! In-process LAN hub. The native app embeds this. `grokhub-hub` CLI is a thin main.

mod server;

pub use server::{serve, serve_background, serve_lan};

use grokhub_core::{load_hub_state, save_hub_state, HubState, HUB_KIND};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Shared hub bootstrap for `grokhub --hub` and `grokhub-hub`.
pub fn run(port: u16, path: PathBuf, banner: &str) -> Result<(), String> {
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
    eprintln!("{banner} on :{port}  pair {code}  kind {HUB_KIND}");
    let shared = Arc::new(Mutex::new(state));
    {
        let st = shared.clone();
        let path = path.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if let Ok(g) = st.lock() {
                let _ = save_hub_state(&path, &g);
            }
        });
    }
    serve(shared, port).map_err(|e| e.to_string())
}
