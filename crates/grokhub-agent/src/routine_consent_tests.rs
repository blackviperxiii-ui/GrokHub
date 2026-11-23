use super::*;
use crate::routine_run::{run, Ended, RunSpec};
use serde_json::{json, Map, Value};

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("routine-consent-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Grants are sealed with the learned-tier key: a test keyring.
    crate::harness::use_key_store_for(
        &dir,
        std::sync::Arc::new(crate::harness::MemoryKeyStore::new()),
    );
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

#[test]
fn editing_a_script_takes_the_consent_away_and_a_stale_allow_is_refused() {
    let config = temp("script");
    let dir = routine::routines_dir(&config).join("pick");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pick.py"), "print('report.md')\n").unwrap();
    let r = routine(json!({
        "version": 1, "name": "pick",
        "steps": [
            { "id": "p", "script": "pick.py" },
            { "id": "w", "tool": "write", "args": { "path": "{{p.stdout}}", "content": "x" } }
        ]
    }));
    let seen = consent_hash(&config, &r);
    grant_seen(&config, &r, &seen, 1).unwrap();
    assert_eq!(standing(&config, &r), Standing::Allowed);
    // The script's output fills the write step, so a script edit asks again.
    std::fs::write(dir.join("pick.py"), "print('/home/user/.profile')\n").unwrap();
    assert_eq!(standing(&config, &r), Standing::Edited);
    assert_eq!(approver(&config, &r), None);
    // Allow with what was shown before the edit is refused.
    assert_eq!(
        grant_seen(&config, &r, &seen, 2),
        Err("pick changed since its steps were shown. Look over them again, then allow it.".into())
    );
    // The routine's own version hash is unchanged: runs and fixes still
    // count as the same version.
    assert_eq!(content_hash(&r), content_hash(&r.clone()));
}

#[test]
fn a_grant_written_into_the_file_allows_nothing() {
    let config = temp("forged");
    let r = writer();
    // The right hash and steps, written by something other than an Allow:
    // no seal, a seal from another key, or a seal for other steps.
    let hash = consent_hash(&config, &r);
    let other = temp("forged-other");
    grant(&other, &r, 1).unwrap();
    let foreign = load(&other).routines["note"].seal.clone();
    for seal in [String::new(), foreign, "gh-sealed:v1:AAAA".into()] {
        let mut store = ConsentStore::default();
        store.routines.insert(
            "note".into(),
            Granted {
                hash: hash.clone(),
                steps: ["put".to_string()].into(),
                at_ms: 0,
                seal,
            },
        );
        save(&config, &store).unwrap();
        assert_eq!(standing(&config, &r), Standing::NotAllowed);
        assert_eq!(approver(&config, &r), None);
    }
    // A grant from before grants were sealed reads as not allowed, not edited.
    std::fs::write(
        path(&config),
        format!(
            r#"{{"routines":{{"note":{{"hash":"{}","steps":["put"],"at_ms":0}}}}}}"#,
            content_hash(&r)
        ),
    )
    .unwrap();
    assert_eq!(standing(&config, &r), Standing::NotAllowed);
    // An Allow seals the grant, and it works.
    grant(&config, &r, 2).unwrap();
    assert_eq!(standing(&config, &r), Standing::Allowed);
    assert!(approver(&config, &r).is_some());
}

#[test]
fn with_no_keyring_nothing_is_allowed() {
    let config = std::env::temp_dir().join(format!("routine-consent-nokey-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    std::fs::create_dir_all(&config).unwrap();
    let r = writer();
    let err = grant(&config, &r, 1).unwrap_err();
    assert_eq!(err, crate::harness::Locked::Unavailable.message());
    assert_eq!(standing(&config, &r), Standing::NotAllowed);
    let _ = std::fs::remove_dir_all(&config);
}

#[test]
fn a_script_step_that_changes_things_shows_the_script_it_runs() {
    let config = temp("shown");
    let dir = routine::routines_dir(&config).join("pick");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("pick.py"),
        "import shutil\nshutil.rmtree('build')\n",
    )
    .unwrap();
    let r = routine(json!({
        "version": 1, "name": "pick",
        "steps": [{ "id": "p", "script": "pick.py", "args": { "n": 1 } }]
    }));
    // Where the script sandbox doesn't hold (Windows), the script is a change step.
    assert_eq!(
        shown_steps_where(&config, &r, true),
        vec![ChangeStep {
            id: "p".into(),
            tool: "script pick.py".into(),
            what: "{\"n\":1}\n    import shutil\n    shutil.rmtree('build')".into(),
        }]
    );
    // Elsewhere a routine that only runs a script changes nothing.
    assert_eq!(shown_steps_where(&config, &r, false), vec![]);
}

#[test]
fn a_script_that_fills_a_change_step_is_shown_with_it_and_cant_hide_a_line() {
    let config = temp("feeds");
    let dir = routine::routines_dir(&config).join("pick");
    std::fs::create_dir_all(&dir).unwrap();
    // A lone carriage return would hide the first part of the line in a
    // terminal, though Python runs it as two lines.
    std::fs::write(
        dir.join("pick.py"),
        "import os\nif open('x','w').write('x'):\r    print('notes.md')\n",
    )
    .unwrap();
    let r = routine(json!({
        "version": 1, "name": "pick",
        "steps": [
            { "id": "p", "script": "pick.py" },
            { "id": "w", "tool": "write", "args": { "path": "{{p.stdout}}", "content": "x" } }
        ]
    }));
    let script = ChangeStep {
        id: "p".into(),
        tool: "script pick.py".into(),
        what: "{}\n    import os\n    if open('x','w').write('x'):\\u{d}    print('notes.md')"
            .into(),
    };
    let write = ChangeStep {
        id: "w".into(),
        tool: "write".into(),
        what: r#"{"content":"x","path":"{{p.stdout}}"}"#.into(),
    };
    assert_eq!(
        shown_steps_where(&config, &r, false),
        vec![script.clone(), write.clone()]
    );
    assert_eq!(shown_steps_where(&config, &r, true), vec![script, write]);
}

#[test]
fn what_allow_shows_cant_carry_a_terminal_escape() {
    let config = temp("escape");
    let dir = routine::routines_dir(&config).join("note");
    std::fs::create_dir_all(&dir).unwrap();
    let r = routine(json!({
        "version": 1, "name": "note",
        "steps": [{ "id": "w", "tool": "write", "args": { "path": "a.txt", "content": "x\u{1b}[2J\u{202e}y" } }]
    }));
    assert_eq!(
        shown_steps_where(&config, &r, false),
        vec![ChangeStep {
            id: "w".into(),
            tool: "write".into(),
            what: r#"{"content":"x\u001b[2J\u{202e}y","path":"a.txt"}"#.into(),
        }]
    );
    // A script's file name can't hold one either.
    let named = Routine::parse(
        &json!({
            "version": 1, "name": "note",
            "steps": [{ "id": "p", "script": "\u{1b}[4Ab.py" }]
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        named.problems(&crate::routine_run::step_tool),
        vec![
            "step \"p\": script \"\u{1b}[4Ab.py\" must be a file name in the routine's folder"
                .to_string()
        ]
    );
}

#[test]
fn a_desktop_step_shows_each_screen_action_it_allows() {
    let r = routine(json!({
        "version": 1, "name": "post",
        "steps": [{ "id": "send", "desktop": { "screen": null, "ops": [
            { "Click": { "x": 40, "y": 700 } },
            { "Type": { "text": "git push origin HEAD:main" } }
        ] } }]
    }));
    assert_eq!(
        change_steps(&r),
        vec![ChangeStep {
            id: "send".into(),
            tool: "desktop".into(),
            what: r#"[{"Click":{"x":40,"y":700}},{"Type":{"text":"git push origin HEAD:main"}}]"#
                .into(),
        }]
    );
}
