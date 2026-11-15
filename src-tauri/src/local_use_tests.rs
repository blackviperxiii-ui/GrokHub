use super::*;

use grokhub_agent::route::local;

fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "grokhub-shell-local-use-{tag}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

fn saved(d: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(d.join("app.json")).unwrap()).unwrap()
}

#[test]
fn the_hint_leads_with_on_or_off_and_says_what_runs_here() {
    assert_eq!(
        view_of(true, true).hint,
        "On. Background jobs run on this computer, and so do some chats: short ones while the model is loaded, and any while you're offline or signed out of Grok."
    );
    assert_eq!(
        view_of(true, false).hint,
        "On. Set up a local engine and model below and it starts helping."
    );
    for set_up in [true, false] {
        assert_eq!(
            view_of(false, set_up),
            LocalUseView {
                on: false,
                hint: "Off. Nothing runs on the on-device model.".into(),
            }
        );
    }
}

#[test]
fn a_new_install_is_on_and_writes_nothing() {
    let d = dir("fresh");
    assert!(is_on(&d));
    assert_eq!(view(&d), view_of(true, set_up(&d)));
    assert!(!d.join("app.json").exists());
}

#[test]
fn an_older_apps_saved_off_can_be_switched_back_on() {
    let d = dir("old-off");
    std::fs::write(
        d.join("app.json"),
        r#"{"localModel":false,"promptCleanup":true,"focusHelp":true}"#,
    )
    .unwrap();
    grokhub_agent::route::host::sync(&d);
    assert!(!local::switched_on(), "the old off reaches the router");
    assert!(!local::prompt_cleanup());
    assert_eq!(view(&d), view_of(false, set_up(&d)));
    assert_eq!(set(&d, true), Ok(view_of(true, set_up(&d))));
    assert_eq!(
        saved(&d),
        json!({ "localModel": true, "promptCleanup": true, "focusHelp": true })
    );
    assert!(local::switched_on(), "applied now");
    assert!(local::prompt_cleanup(), "the saved tidy comes back with it");
}

#[test]
fn turning_it_off_reaches_the_router_at_once_and_pauses_the_tidy() {
    let d = dir("off");
    std::fs::write(d.join("app.json"), r#"{"promptCleanup":true}"#).unwrap();
    assert_eq!(set(&d, true).map(|v| v.on), Ok(true));
    assert!(local::switched_on() && local::prompt_cleanup());
    assert_eq!(set(&d, false), Ok(view_of(false, set_up(&d))));
    assert_eq!(
        saved(&d),
        json!({ "localModel": false, "promptCleanup": true })
    );
    assert!(!local::switched_on());
    assert!(!local::prompt_cleanup());
}

#[test]
fn off_stops_the_model_after_the_answer_in_flight_and_on_keeps_it() {
    let d = dir("stop");
    let held = local_sidecar::busy();
    set(&d, false).unwrap();
    assert!(
        local_sidecar::stop_pending(),
        "an answer in flight is never cut off"
    );
    set(&d, true).unwrap();
    assert!(!local_sidecar::stop_pending(), "back on: no stop");
    set(&d, false).unwrap();
    drop(held);
    assert!(
        !local_sidecar::stop_pending(),
        "the stop runs as soon as the last answer finishes"
    );
}

#[test]
fn an_unreadable_app_json_is_left_alone() {
    let d = dir("bad");
    std::fs::write(d.join("app.json"), "not json").unwrap();
    assert!(set(&d, false).is_err());
    assert_eq!(
        std::fs::read_to_string(d.join("app.json")).unwrap(),
        "not json"
    );
    assert!(
        !local_sidecar::stop_pending(),
        "a failed save stops nothing"
    );
}
