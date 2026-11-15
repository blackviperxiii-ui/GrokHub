use super::*;

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
fn a_new_install_is_on_and_writes_nothing() {
    let d = dir("fresh");
    assert_eq!(
        view(&d),
        LocalUseView {
            on: true,
            hint: "On. Background jobs such as sorting, summaries and hiding private details can run on this computer once a local engine and model are set up.".into(),
        }
    );
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
    assert_eq!(
        view(&d),
        LocalUseView {
            on: false,
            hint: "Off. Nothing runs on the on-device model, and typo tidy is paused.".into(),
        }
    );
    assert_eq!(set(&d, true).map(|v| v.on), Ok(true));
    assert_eq!(
        saved(&d),
        json!({ "localModel": true, "promptCleanup": true, "focusHelp": true })
    );
    let routing = grokhub_agent::route::host::read(&d);
    assert!(routing.local_model && routing.prompt_cleanup);
    assert!(grokhub_agent::route::local::switched_on(), "applied now");
    assert!(grokhub_agent::route::local::prompt_cleanup());
}

#[test]
fn turning_it_off_reaches_the_router_at_once_and_pauses_the_tidy() {
    let d = dir("off");
    std::fs::write(d.join("app.json"), r#"{"promptCleanup":true}"#).unwrap();
    assert_eq!(set(&d, false).map(|v| v.on), Ok(false));
    assert_eq!(
        saved(&d),
        json!({ "localModel": false, "promptCleanup": true })
    );
    assert!(!grokhub_agent::route::local::switched_on());
    assert!(!grokhub_agent::route::local::prompt_cleanup());
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
}
