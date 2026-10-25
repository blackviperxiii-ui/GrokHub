use super::*;
use crate::route::local_sidecar::{busy, ensure_global, in_flight, tick_governor};

#[test]
fn levels_map_to_launch_states_and_unload_never_launches() {
    assert_eq!(gov_state(Level::Unload), None);
    assert_eq!(gov_state(Level::Throttle), Some(GovState::Throttle));
    assert_eq!(gov_state(Level::Normal), Some(GovState::Normal));
    assert_eq!(gov_state(Level::RampUp), Some(GovState::RampUp));
}

#[test]
fn a_change_between_running_levels_restarts_and_unload_pauses() {
    reset_for_test();
    apply(Level::Throttle, "a game is running");
    assert_eq!(autoconfig::state(), GovState::Throttle);
    assert!(restart_due());
    assert!(!paused());
    assert_eq!(reason(), "a game is running");
    clear_restart();

    apply(Level::Unload, "the battery is low");
    assert!(paused());
    assert!(!restart_due());
    // The launch state stays at the last running level.
    assert_eq!(autoconfig::state(), GovState::Throttle);
    let dir = std::env::temp_dir();
    assert_eq!(
        ensure_global(&dir),
        Err("the on-device model is paused because the battery is low".into())
    );

    apply(Level::Normal, "normal use");
    assert!(!paused());
    assert!(
        !restart_due(),
        "out of Unload starts fresh, nothing to restart"
    );
    assert_eq!(autoconfig::state(), GovState::Normal);

    // The same level again only refreshes the reason.
    apply(Level::Normal, "idle on AC power");
    assert!(!restart_due());
    assert_eq!(reason(), "idle on AC power");
    reset_for_test();
}

#[test]
fn restarts_wait_for_requests_in_flight() {
    reset_for_test();
    apply(Level::RampUp, "idle on AC power");
    assert!(restart_due());
    let guard = busy();
    assert_eq!(in_flight(), 1);
    tick_governor();
    assert!(restart_due(), "a request is in flight, so nothing restarts");
    drop(guard);
    assert_eq!(in_flight(), 0);
    tick_governor();
    assert!(!restart_due());
    reset_for_test();
}

#[test]
fn three_failed_starts_open_the_crash_breaker() {
    reset_for_test();
    assert_eq!(may_start(0), Ok(()));
    start_failed(0);
    start_failed(1_000);
    assert_eq!(may_start(2_000), Ok(()));
    start_failed(2_000);
    assert_eq!(
        may_start(3_000),
        Err("the local model server kept stopping, so GrokHub paused it for 30 minutes".into())
    );
    assert_eq!(may_start(2_000 + 1_800_000), Ok(()));
    reset_for_test();
}

#[cfg(unix)]
#[test]
fn the_breaker_stops_starting_a_server_that_keeps_dying_without_leaking_a_request() {
    use std::os::unix::fs::PermissionsExt;
    reset_for_test();
    let dir = crate::harness::test_dir("gov-breaker");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::create_dir_all(dir.join("models")).unwrap();
    let bin = dir.join("bin").join("llama-server");
    std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("models").join("m.gguf"), b"x").unwrap();
    for _ in 0..3 {
        assert_eq!(
            crate::route::local_sidecar::acquire(&dir).err(),
            Some("local model server stopped before it was ready".into())
        );
    }
    assert_eq!(
        crate::route::local_sidecar::acquire(&dir).err(),
        Some("the local model server kept stopping, so GrokHub paused it for 30 minutes".into())
    );
    assert_eq!(in_flight(), 0, "a failed start holds no request guard");
    assert_eq!(crate::route::local_sidecar::global_pid(), None);
    reset_for_test();
}
