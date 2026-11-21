use super::*;
use crate::routine_run::{run, Ended, RunSpec};
use serde_json::{json, Map, Value};

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("routine-consent-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn routine(v: Value) -> Routine {
    let r = Routine::parse(&v.to_string()).unwrap();
    assert_eq!(
        r.problems(&crate::routine_run::step_tool),
        Vec::<String>::new()
    );
    r
}

fn writer() -> Routine {
    routine(json!({
        "version": 1, "name": "note",
        "inputs": { "text": { "type": "string", "default": "hello" } },
        "steps": [
            { "id": "look", "tool": "list_dir", "args": { "target_directory": "." } },
            { "id": "put", "tool": "write", "args": { "path": "note.txt", "content": "{{inputs.text}}" } }
        ]
    }))
}

fn go(r: &Routine, work: &Path, approve: Option<&dyn Approve>) -> Ended {
    let stop = || false;
    run(&RunSpec {
        routine: r,
        dir: work,
        workspace: work,
        inputs: Map::new(),
        dry_run: false,
        approve,
        desktop: None,
        stop: &stop,
        brain: None,
        words: None,
    })
    .ended
}

#[test]
fn only_change_steps_need_consent() {
    let steps = change_steps(&writer());
    assert_eq!(
        steps,
        vec![ChangeStep {
            id: "put".into(),
            tool: "write".into(),
            what: r#"{"content":"{{inputs.text}}","path":"note.txt"}"#.into(),
        }]
    );
    let config = temp("standing");
    let reader = routine(json!({
        "version": 1, "name": "peek",
        "steps": [{ "id": "look", "tool": "list_dir", "args": { "target_directory": "." } }]
    }));
    assert_eq!(standing(&config, &reader), Standing::ReadOnly);
    assert_eq!(standing(&config, &writer()), Standing::NotAllowed);
}

#[test]
fn a_granted_routine_runs_its_change_steps_unattended() {
    let (config, work) = (temp("grant-cfg"), temp("grant-work"));
    let r = writer();
    assert_eq!(approver(&config, &r), None);
    grant(&config, &r, 7).unwrap();
    assert_eq!(standing(&config, &r), Standing::Allowed);
    assert_eq!(load(&config).routines["note"].at_ms, 7);
    let consented = approver(&config, &r).unwrap();
    assert_eq!(go(&r, &work, Some(&consented)), Ended::Finished);
    assert_eq!(
        std::fs::read_to_string(work.join("note.txt")).unwrap(),
        "hello"
    );
}

#[test]
fn an_edit_takes_the_consent_away_until_allowed_again() {
    let (config, work) = (temp("edit-cfg"), temp("edit-work"));
    let r = writer();
    grant(&config, &r, 1).unwrap();
    let mut edited = r.clone();
    edited.steps[1].kind = StepKind::Tool {
        tool: "write".into(),
        args: json!({ "path": "../elsewhere.txt", "content": "x" })
            .as_object()
            .unwrap()
            .clone(),
    };
    assert_ne!(content_hash(&edited), content_hash(&r));
    assert_eq!(standing(&config, &edited), Standing::Edited);
    assert_eq!(approver(&config, &edited), None);
    // Unattended with no consent: the change step is refused.
    match go(&edited, &work, None) {
        Ended::Stopped { at, .. } => assert_eq!(at, "put"),
        other => panic!("{other:?}"),
    }
    assert!(!work.join("note.txt").exists());
    assert_eq!(revoke(&config, "note"), Ok(true));
    assert_eq!(revoke(&config, "note"), Ok(false));
    assert_eq!(standing(&config, &r), Standing::NotAllowed);
}

#[test]
fn consent_never_covers_a_hard_class_or_an_unlisted_step() {
    let r = routine(json!({
        "version": 1, "name": "tidy",
        "steps": [{ "id": "clear", "tool": "run_terminal_command", "args": { "command": "rm old.log" } }]
    }));
    let config = temp("hard");
    grant(&config, &r, 1).unwrap();
    let consented = approver(&config, &r).unwrap();
    assert!(!consented.approve(
        "clear",
        "run_terminal_command",
        r#"{"command":"rm old.log"}"#
    ));
    assert!(consented.approve("clear", "run_terminal_command", r#"{"command":"echo hi"}"#));
    assert!(!consented.approve("other", "run_terminal_command", r#"{"command":"echo hi"}"#));
}
