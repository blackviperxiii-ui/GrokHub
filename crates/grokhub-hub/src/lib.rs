//! In-process LAN hub. The native app embeds this. `grokhub-hub` CLI is a thin main.

mod server;

pub use server::{serve, serve_background, serve_lan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    Serve,
    Version,
    Help,
}

pub fn parse_args(args: &[String]) -> Launch {
    for a in args.iter().skip(1) {
        match a.as_str() {
            "--version" | "-V" => return Launch::Version,
            "-h" | "--help" => return Launch::Help,
            _ => {}
        }
    }
    Launch::Serve
}

use grokhub_core::{load_hub_state, now_ms, save_hub_state, start_hub_rotates_pair, HubState, HUB_KIND};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn boot_state(mut state: HubState, port: u16) -> HubState {
    state.port = port;
    state.sharing = true;
    tick_running_pair(&mut state, now_ms());
    state
}

fn tick_running_pair(state: &mut HubState, now: u64) -> bool {
    if start_hub_rotates_pair(state.pair.as_ref().map(|p| p.expires_at), now) {
        state.rotate_pair();
        true
    } else {
        false
    }
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
            if let Ok(mut g) = st.lock() {
                tick_running_pair(&mut g, now_ms());
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

    #[test]
    fn standalone_hub_mints_again_after_ttl_not_only_at_boot() {
        let src = include_str!("lib.rs");
        let persist = src
            .split("pub fn run(")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .and_then(|s| s.split("thread::spawn").nth(1))
            .expect("persist");
        assert!(
            persist.contains("tick_running_pair") || persist.contains("start_hub_rotates_pair"),
            "long-lived grokhub-hub must mint after PAIR_TTL, not only at boot: {persist}"
        );
        let mut dead = HubState::empty();
        dead.rotate_pair();
        dead.pair.as_mut().unwrap().expires_at = 1;
        let old = dead.pair.as_ref().unwrap().code.clone();
        assert!(tick_running_pair(&mut dead, now_ms()));
        assert_ne!(
            dead.pair.as_ref().map(|p| p.code.as_str()),
            Some(old.as_str()),
            "expired pair must not stay the live code across persist ticks"
        );
        let mut live = HubState::empty();
        let code = live.rotate_pair().code;
        assert!(!tick_running_pair(&mut live, now_ms()));
        assert_eq!(live.pair.as_ref().map(|p| p.code.as_str()), Some(code.as_str()));
    }

    #[test]
    fn standalone_hub_version_and_help_must_not_bind() {
        let main = include_str!("main.rs");
        let before_run = main.split("grokhub_hub::run(").next().expect("run");
        assert!(
            before_run.contains("--version") && before_run.contains("Launch::Version"),
            "grokhub-hub --version must print and exit before bind: {before_run}"
        );
        assert!(
            (before_run.contains("--help") || before_run.contains("\"-h\""))
                && before_run.contains("Launch::Help"),
            "grokhub-hub --help must print usage before bind: {before_run}"
        );
        assert_eq!(parse_args(&["grokhub-hub".into()]), Launch::Serve);
        assert_eq!(
            parse_args(&["grokhub-hub".into(), "--version".into()]),
            Launch::Version
        );
        assert_eq!(
            parse_args(&["grokhub-hub".into(), "-V".into()]),
            Launch::Version
        );
        assert_eq!(
            parse_args(&["grokhub-hub".into(), "--help".into()]),
            Launch::Help
        );
        assert_eq!(parse_args(&["grokhub-hub".into(), "-h".into()]), Launch::Help);
    }
}
