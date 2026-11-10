use super::*;
use grokhub_core::app_lock::Owner;

fn held_by(app: AppKind) -> Held {
    Held {
        other: Some(Owner { app, pid: 7 }),
    }
}

#[test]
fn a_free_profile_starts() {
    assert_eq!(lock_start(None, false), LockStart::Go);
    assert_eq!(lock_start(None, true), LockStart::Go);
}

#[test]
fn a_login_start_on_a_taken_profile_leaves_quietly() {
    assert_eq!(
        lock_start(Some(&held_by(AppKind::Cabin)), true),
        LockStart::ExitQuietly
    );
}

#[test]
fn an_asked_for_start_says_which_app_has_the_profile() {
    assert_eq!(
        lock_start(Some(&held_by(AppKind::Cabin)), false),
        LockStart::Tell(
            "The other Loadout app is already open with this profile. Quit it from its tray, then try again."
                .into()
        )
    );
    assert_eq!(
        lock_start(Some(&Held { other: None }), false),
        LockStart::Tell(
            "Another copy of Loadout is already open with this profile. Quit it from its tray, then try again."
                .into()
        )
    );
}

#[test]
fn setup_takes_the_profile_before_any_window() {
    let src = include_str!("lib.rs").replace("\r\n", "\n");
    let setup = src
        .split("pub fn run() {")
        .nth(1)
        .and_then(|s| s.split(".on_window_event(").next())
        .expect("run setup");
    let take = setup
        .find("app_guard::take_profile(")
        .expect("take_profile in setup");
    assert!(
        take < setup.find("build_main_window(app)").expect("window"),
        "{setup}"
    );
    assert!(
        take < setup.find("app.manage(ShellState").expect("state"),
        "{setup}"
    );
    assert!(
        setup
            .find("os_extras::plugins(builder)")
            .expect("single instance")
            < take,
        "the single-instance hand-off runs first"
    );
    assert_eq!(src.matches("app_guard::take_profile(").count(), 1);
}
