use super::*;
use crate::client::ContentPart;
use serde_json::json;
use std::path::PathBuf;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("routine-draft-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn user(text: &str) -> InputItem {
    InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText(text.into())],
    }
}

fn call(id: &str, name: &str, args: Value) -> InputItem {
    InputItem::FunctionCall {
        call_id: id.into(),
        name: name.into(),
        arguments: args.to_string(),
    }
}

fn out(id: &str, text: &str) -> InputItem {
    InputItem::FunctionCallOutput {
        call_id: id.into(),
        output: text.into(),
    }
}

/// An earlier turn, then one that checked the disk, had a step refused,
/// read a note and planned.
fn chat() -> Vec<InputItem> {
    vec![
        user("hi"),
        call("a", "read_file", json!({ "target_file": "old.txt" })),
        out("a", "1|old"),
        user("how full is /home?"),
        call(
            "b",
            "run_terminal_command",
            json!({ "command": "df -h /home" }),
        ),
        out("b", "exit 0\n/dev/sda1  50G  45G  90% /home"),
        call(
            "c",
            "run_terminal_command",
            json!({ "command": "rm -rf /tmp/x" }),
        ),
        out(
            "c",
            "User rejected the execution for tool `run_terminal_command`",
        ),
        call("d", "run_terminal_command", json!({ "command": "false" })),
        out("d", "exit 1\n"),
        call("e", "todo_write", json!({ "todos": [] })),
        out("e", "ok"),
        call("f", "read_file", json!({ "target_file": "notes/home.txt" })),
        out("f", "1|watch /home"),
        call(
            "g",
            "run_terminal_command",
            json!({ "command": "du -sh /home" }),
        ),
        out("g", "exit 0\n40G /home"),
    ]
}

fn naming(inputs: &[(&str, &str)]) -> Naming {
    Naming {
        name: "home-disk".into(),
        about: "How full /home is".into(),
        inputs: inputs
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string(), String::new()))
            .collect(),
    }
}

#[test]
fn only_the_last_turns_working_steps_are_kept() {
    let calls = last_turn_calls(&chat());
    assert_eq!(
        calls,
        vec![
            Call {
                tool: "run_terminal_command".into(),
                args: json!({ "command": "df -h /home" })
                    .as_object()
                    .unwrap()
                    .clone()
            },
            Call {
                tool: "read_file".into(),
                args: json!({ "target_file": "notes/home.txt" })
                    .as_object()
                    .unwrap()
                    .clone()
            },
            Call {
                tool: "run_terminal_command".into(),
                args: json!({ "command": "du -sh /home" })
                    .as_object()
                    .unwrap()
                    .clone()
            },
        ]
    );
}

#[test]
fn named_values_become_inputs_with_their_defaults() {
    let r = draft(&last_turn_calls(&chat()), &naming(&[("folder", "/home")])).unwrap();
    assert_eq!(
        serde_json::to_value(&r).unwrap(),
        json!({
            "version": 1,
            "name": "home-disk",
            "about": "How full /home is",
            "triggers": ["chat"],
            "inputs": { "folder": { "type": "string", "default": "/home" } },
            "steps": [
                { "id": "run", "tool": "run_terminal_command", "args": { "command": "df -h {{inputs.folder}}" } },
                { "id": "read", "tool": "read_file", "args": { "target_file": "notes{{inputs.folder}}.txt" } },
                { "id": "run_2", "tool": "run_terminal_command", "args": { "command": "du -sh {{inputs.folder}}" } }
            ],
            "result": "{{run_2.stdout}}"
        })
    );
}

#[test]
fn a_draft_says_why_it_cant_be_made() {
    let calls = last_turn_calls(&chat());
    assert_eq!(
        draft(&calls, &naming(&[("place", "/srv")])),
        Err("\"/srv\" for inputs.place isn't in any step".into())
    );
    let mut bad = naming(&[]);
    bad.name = "Home Disk".into();
    assert_eq!(
        draft(&calls, &bad).unwrap_err(),
        "name \"Home Disk\": use lowercase letters, digits and dashes"
    );
    assert_eq!(
        draft(&[], &naming(&[])),
        Err(
            "the last turn ran no steps a routine can keep (files, commands, edits, web reads)"
                .into()
        )
    );
    let braces = vec![Call {
        tool: "run_terminal_command".into(),
        args: json!({ "command": "echo {{x}}" })
            .as_object()
            .unwrap()
            .clone(),
    }];
    assert_eq!(
        draft(&braces, &naming(&[])),
        Err("a step's text holds \"{{\", which a routine would read as a blank".into())
    );
}

#[test]
fn a_short_value_only_replaces_a_whole_argument() {
    let calls = vec![Call {
        tool: "list_dir".into(),
        args: json!({ "target_directory": "/" })
            .as_object()
            .unwrap()
            .clone(),
    }];
    let r = draft(&calls, &naming(&[("root", "/")])).unwrap();
    assert_eq!(
        r.steps[0].kind,
        StepKind::Tool {
            tool: "list_dir".into(),
            args: json!({ "target_directory": "{{inputs.root}}" })
                .as_object()
                .unwrap()
                .clone()
        }
    );
    let calls = vec![Call {
        tool: "run_terminal_command".into(),
        args: json!({ "command": "ls /" }).as_object().unwrap().clone(),
    }];
    assert_eq!(
        draft(&calls, &naming(&[("root", "/")])),
        Err("\"/\" for inputs.root isn't in any step".into())
    );
}

#[test]
fn saving_goes_through_the_ledger_and_undo_takes_it_back() {
    let config = temp("save");
    let first = draft(&last_turn_calls(&chat()), &naming(&[])).unwrap();
    let change = save(&config, &first, "saved from chat").unwrap().unwrap();
    assert_eq!(
        (change.kind.as_str(), change.id.as_str()),
        ("routine", "home-disk")
    );
    assert_eq!(save(&config, &first, "again").unwrap(), None);
    let dir = routine::routines_dir(&config).join("home-disk");
    assert_eq!(
        Routine::load(&dir, &crate::routine_run::step_tool),
        Ok(first.clone())
    );

    let second = draft(&last_turn_calls(&chat()), &naming(&[("folder", "/home")])).unwrap();
    save(&config, &second, "named an input").unwrap().unwrap();
    assert_eq!(
        undo(&config, "home-disk", harness::UndoAsk::from_typing()),
        Ok("Undid the newest change to the routine home-disk.".into())
    );
    assert_eq!(
        Routine::load(&dir, &crate::routine_run::step_tool),
        Ok(first)
    );
    assert_eq!(
        undo(&config, "home-disk", harness::UndoAsk::from_typing()),
        Ok("Removed the routine home-disk. Its text is kept in the change history.".into())
    );
    assert!(!dir.exists());
}
