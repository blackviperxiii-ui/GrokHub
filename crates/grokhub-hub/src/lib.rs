//! In-process LAN hub. The native app embeds this. `grokhub-hub` CLI is a thin main.

mod server;

pub use server::{serve, serve_background, serve_lan};

use grokhub_core::{load_hub_state, now_ms, save_hub_state, start_hub_rotates_pair, HubState, HUB_KIND};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn boot_state(mut state: HubState, port: u16) -> HubState {
    state.port = port;
    state.sharing = true;
    if start_hub_rotates_pair(state.pair.as_ref().map(|p| p.expires_at), now_ms()) {
        state.rotate_pair();
    }
    state
}

/// Shared hub bootstrap for `grokhub --hub` and `grokhub-hub`.
pub fn run(port: u16, path: PathBuf, banner: &str) -> Result<(), String> {
    let state = load_hub_state(&path).unwrap_or_else(HubState::empty);
    let state = boot_state(state, port);
    eprintln!("{banner} on :{port}  kind {HUB_KIND}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_hub_rotates_expired_and_does_not_print_the_code() {
        let src = include_str!("lib.rs");
        let run = src
            .split("pub fn run(")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("run");
        assert!(
            run.contains("boot_state") && src.contains("start_hub_rotates_pair"),
            "an expired leftover pair must rotate when grokhub-hub starts: {run}"
        );
        assert!(
            !run.contains("pair {code}"),
            "journal must not print the live pair code: {run}"
        );
        let mut st = HubState::empty();
        st.rotate_pair();
        st.pair.as_mut().unwrap().expires_at = 1;
        let old = st.pair.as_ref().unwrap().code.clone();
        let booted = boot_state(st, 18766);
        assert_ne!(
            booted.pair.as_ref().map(|p| p.code.as_str()),
            Some(old.as_str()),
            "expired leftover must not stay the live code"
        );
        let mut live = HubState::empty();
        let code = live.rotate_pair().code;
        let kept = boot_state(live, 18766);
        assert_eq!(kept.pair.as_ref().map(|p| p.code.as_str()), Some(code.as_str()));
    }
}
