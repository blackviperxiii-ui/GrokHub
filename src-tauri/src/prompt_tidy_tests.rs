use super::*;

fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "grokhub-shell-tidy-{tag}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

fn app_json(d: &Path) -> String {
    std::fs::read_to_string(d.join("app.json")).unwrap_or_default()
}

#[test]
fn the_hint_leads_with_on_or_off() {
    assert_eq!(
        hint(false, true, true),
        "Off. Your messages go to the model exactly as you type them."
    );
    assert_eq!(
        hint(true, true, true),
        "On. While the on-device model is loaded, it fixes typos first (up to 0.3 s); Keep loaded tidies every message. Your chat keeps what you typed; numbers, names, paths and links are never changed."
    );
    assert_eq!(
        hint(true, false, true),
        "On, but paused. Set up a local engine and model first."
    );
    assert_eq!(
        hint(false, false, false),
        "Off. Set up a local engine and model first."
    );
    assert_eq!(
        hint(false, true, false),
        "Off. An older version of the app switched Local AI off; turning this on switches it back on."
    );
}

#[test]
fn off_and_unavailable_until_a_local_model_is_set_up() {
    let d = dir("fresh");
    assert_eq!(
        view_with(&d, false),
        TidyView {
            on: false,
            available: false,
            hint: "Off. Set up a local engine and model first.".into(),
        }
    );
    assert_eq!(set_with(&d, true, false), Err(NOT_SET_UP.to_string()));
    assert!(!d.join("app.json").exists(), "nothing is written");
}

#[test]
fn an_older_local_model_off_is_switched_back_on_with_the_tidy() {
    let d = dir("local-off");
    std::fs::write(
        d.join("app.json"),
        r#"{"localModel":false,"promptCleanup":true,"focusHelp":true}"#,
    )
    .unwrap();
    assert_eq!(
        view_with(&d, true),
        TidyView {
            on: false,
            available: true,
            hint: "Off. An older version of the app switched Local AI off; turning this on switches it back on.".into(),
        }
    );
    assert_eq!(set_with(&d, true, true).map(|v| v.on), Ok(true));
    let saved: serde_json::Value = serde_json::from_str(&app_json(&d)).unwrap();
    assert_eq!(
        saved,
        json!({ "localModel": true, "promptCleanup": true, "focusHelp": true })
    );
    let routing = grokhub_agent::route::host::read(&d);
    assert!(routing.local_model && routing.prompt_cleanup);
}

#[test]
fn a_saved_on_shows_paused_and_still_turns_off_without_a_model() {
    let d = dir("paused");
    std::fs::write(d.join("app.json"), r#"{"promptCleanup":true}"#).unwrap();
    assert_eq!(
        view_with(&d, false),
        TidyView {
            on: true,
            available: false,
            hint: "On, but paused. Set up a local engine and model first.".into(),
        }
    );
    assert_eq!(
        set_with(&d, false, false),
        Ok(TidyView {
            on: false,
            available: false,
            hint: "Off. Set up a local engine and model first.".into(),
        })
    );
    let saved: serde_json::Value = serde_json::from_str(&app_json(&d)).unwrap();
    assert_eq!(
        saved,
        json!({ "promptCleanup": false }),
        "no localModel written"
    );
}

#[test]
fn turning_it_on_reaches_the_router_at_once_and_keeps_other_settings() {
    let d = dir("on");
    std::fs::write(d.join("app.json"), r#"{"focusHelp":true}"#).unwrap();
    assert_eq!(
        set_with(&d, true, true),
        Ok(TidyView {
            on: true,
            available: true,
            hint: hint(true, true, true).into(),
        })
    );
    let saved: serde_json::Value = serde_json::from_str(&app_json(&d)).unwrap();
    assert_eq!(saved, json!({ "focusHelp": true, "promptCleanup": true }));
    assert!(grokhub_agent::route::host::read(&d).prompt_cleanup);
    assert!(grokhub_agent::route::local::prompt_cleanup(), "applied now");
    assert_eq!(set_with(&d, false, true).map(|v| v.on), Ok(false));
    assert!(!grokhub_agent::route::local::prompt_cleanup());
}

#[test]
fn an_unreadable_app_json_is_left_alone() {
    let d = dir("bad");
    std::fs::write(d.join("app.json"), "not json").unwrap();
    assert!(set_with(&d, true, true).is_err());
    assert_eq!(app_json(&d), "not json");
}

#[test]
fn a_file_with_a_byte_order_mark_still_turns_off() {
    let d = dir("bom");
    std::fs::write(d.join("app.json"), "\u{feff}{\"promptCleanup\":true}").unwrap();
    assert!(view_with(&d, true).on);
    assert_eq!(set_with(&d, false, true).map(|v| v.on), Ok(false));
    assert!(!grokhub_agent::route::host::read(&d).prompt_cleanup);
}
