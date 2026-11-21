use super::*;
use crate::client::{ContentPart, InputItem};
use std::path::PathBuf;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("save-routine-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn turn() -> Vec<InputItem> {
    vec![
        InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText("write the note".into())],
        },
        InputItem::FunctionCall {
            call_id: "a".into(),
            name: "list_dir".into(),
            arguments: r#"{"target_directory":"."}"#.into(),
        },
        InputItem::FunctionCallOutput {
            call_id: "a".into(),
            output: "notes/".into(),
        },
        InputItem::FunctionCall {
            call_id: "b".into(),
            name: "write".into(),
            arguments: r#"{"path":"notes/today.txt","content":"buy milk"}"#.into(),
        },
        InputItem::FunctionCallOutput {
            call_id: "b".into(),
            output: "wrote notes/today.txt".into(),
        },
    ]
}

#[test]
fn saves_and_says_which_steps_still_need_an_ok() {
    let config = temp("ok");
    let args = json!({
        "name": "note", "about": "Write today's note",
        "inputs": [{ "name": "text", "value": "buy milk", "about": "what to write" }]
    });
    let out = save_from(&config, &turn(), &args);
    assert_eq!(
        (out.failed, out.text.as_str()),
        (
            false,
            "Saved the routine note with 2 steps: list, write. Its steps that change something (write) are refused when it runs on its own until the user allows them. Undo takes it back from the change history."
        )
    );
    let again = save_from(&config, &turn(), &args);
    assert!(again
        .text
        .starts_with("The routine note was already saved exactly so."));
    // It is a chat routine now.
    let listed: Vec<String> = crate::routine_triggers::for_chat(&config)
        .into_iter()
        .map(|(_, r)| r.name)
        .collect();
    assert_eq!(listed, vec!["note".to_string()]);
}

#[test]
fn a_bad_draft_is_not_saved() {
    let config = temp("bad");
    let out = save_from(
        &config,
        &turn(),
        &json!({ "name": "note", "about": "x", "inputs": [{ "name": "text", "value": "eggs" }] }),
    );
    assert_eq!(
        (out.failed, out.text.as_str()),
        (
            true,
            "Not saved: \"eggs\" for inputs.text isn't in any step."
        )
    );
    assert!(crate::routine_triggers::routines(&config).is_empty());
}
