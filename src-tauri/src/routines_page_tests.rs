use super::*;
use std::path::PathBuf;

use serde_json::{json, Value};

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("routines-page-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(config: &Path, v: Value) {
    let dir = grokhub_core::routine::routines_dir(config).join(v["name"].as_str().unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(grokhub_core::routine::FILE), v.to_string()).unwrap();
}

fn no_route() -> Result<Route, String> {
    Err("no key".into())
}

fn greeter(config: &Path) {
    save(
        config,
        json!({
            "version": 1, "name": "greet", "about": "Says hi",
            "triggers": [{ "timer": "weekdays 07:30" }, { "webhook": "pings" }, "chat"],
            "steps": [{ "id": "say", "tool": "run_terminal_command", "args": { "command": "echo hi" } }],
            "result": "{{say.stdout}}"
        }),
    );
}

fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter().find(|r| r.name == name).unwrap()
}

#[test]
fn rows_show_triggers_changes_and_broken_routines() {
    let config = temp("rows");
    greeter(&config);
    save(
        &config,
        json!({
            "version": 1, "name": "inbox",
            "triggers": [{ "file": { "folder": "/home/user/in", "glob": "*.csv" } }],
            "steps": [{ "id": "look", "tool": "list_dir", "args": { "target_directory": "/home/user/in" } }]
        }),
    );
    save(
        &config,
        json!({ "version": 1, "name": "broken", "triggers": ["chat"], "steps": [{ "id": "x", "tool": "no_such_tool", "args": {} }] }),
    );
    let rows = rows(&config);
    assert_eq!(
        rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        vec!["broken", "greet", "inbox"]
    );
    assert_eq!(
        row(&rows, "greet"),
        &Row {
            name: "greet".into(),
            about: "Says hi".into(),
            triggers: vec![
                "weekdays 07:30".into(),
                "webhook pings".into(),
                "chat".into()
            ],
            steps: 1,
            changes: vec!["say: run_terminal_command".into()],
            consent: Some(Consent::NotAllowed),
            on: true,
            runs: 0,
            model_calls: 0,
            last_at_ms: None,
            last_ok: None,
            last_line: None,
            problems: Vec::new(),
        }
    );
    let inbox = row(&rows, "inbox");
    assert_eq!(
        inbox.triggers,
        vec!["new *.csv in /home/user/in".to_string()]
    );
    assert_eq!(inbox.consent, Some(Consent::ReadOnly));
    let broken = row(&rows, "broken");
    assert_eq!(broken.consent, None);
    assert_eq!(broken.problems.len(), 1);
    assert!(
        broken.problems[0].contains("no_such_tool"),
        "{:?}",
        broken.problems
    );
}

#[test]
fn allow_revoke_and_off_show_on_the_row() {
    let config = temp("allow");
    greeter(&config);
    allow(&config, "greet").unwrap();
    assert_eq!(row(&rows(&config), "greet").consent, Some(Consent::Allowed));
    revoke(&config, "greet").unwrap();
    assert_eq!(
        row(&rows(&config), "greet").consent,
        Some(Consent::NotAllowed)
    );
    grokhub_agent::routine_triggers::set_on(&config, "greet", false).unwrap();
    assert!(!row(&rows(&config), "greet").on);
    assert_eq!(
        allow(&config, "nope"),
        Err("No routine named nope".to_string())
    );
}

#[test]
fn run_now_counts_on_the_row_and_refuses_until_allowed() {
    let config = temp("run");
    greeter(&config);
    let refused = run_now(&config, "greet", &no_route).unwrap();
    assert_eq!(refused.stop, "error");
    assert!(
        refused.error.starts_with("Stopped at say:"),
        "{}",
        refused.error
    );

    allow(&config, "greet").unwrap();
    let ran = run_now(&config, "greet", &no_route).unwrap();
    assert_eq!(ran.stop, crate::routine_runs::FINISHED, "{}", ran.error);
    assert_eq!(ran.text, "say: hi\n\nhi");

    let greet = rows(&config)
        .into_iter()
        .find(|r| r.name == "greet")
        .unwrap();
    assert_eq!((greet.runs, greet.model_calls), (2, 0));
    assert_eq!(greet.last_ok, Some(true));
    assert_eq!(greet.last_line.as_deref(), Some("hi"));
    assert!(greet.last_at_ms.is_some());
    assert_eq!(
        run_now(&config, "nope", &no_route),
        Err("No routine named nope".to_string())
    );
}

#[test]
fn a_routine_already_running_is_not_started_twice() {
    let config = temp("twice");
    greeter(&config);
    let _held = crate::automations::Running::claim("routine:greet").unwrap();
    let second = run_now(&config, "greet", &no_route).unwrap();
    assert_eq!(second.error, "The routine greet is already running.");
    assert_eq!(grokhub_agent::routine_log::read(&config), Vec::new());
}

#[test]
fn undo_takes_back_a_saved_routine() {
    let config = temp("undo");
    let r: Routine = serde_json::from_value(json!({
        "version": 1, "name": "saved", "triggers": ["chat"],
        "steps": [{ "id": "look", "tool": "list_dir", "args": { "target_directory": "." } }]
    }))
    .unwrap();
    grokhub_agent::routine_draft::save(&config, &r, "test").unwrap();
    assert_eq!(rows(&config).len(), 1);
    assert_eq!(
        undo(&config, "saved").unwrap(),
        "Removed the routine saved. Its text is kept in the change history."
    );
    assert_eq!(rows(&config), Vec::new());
}
