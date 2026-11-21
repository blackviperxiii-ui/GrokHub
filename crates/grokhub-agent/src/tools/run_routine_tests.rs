use super::*;
use std::path::PathBuf;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("run-routine-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(config: &Path, v: Value) {
    let dir = grokhub_core::routine::routines_dir(config).join(v["name"].as_str().unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(grokhub_core::routine::FILE), v.to_string()).unwrap();
}

fn config() -> (PathBuf, PathBuf) {
    let config = temp("cfg");
    let work = temp("work");
    std::fs::write(work.join("notes.txt"), "alpha\nbeta\n").unwrap();
    save(
        &config,
        json!({
            "version": 1, "name": "read-notes", "about": "Reads a notes file",
            "triggers": ["chat"],
            "inputs": { "file": { "type": "string", "default": "notes.txt" }, "lines": { "type": "integer" } },
            "steps": [{ "id": "read", "tool": "read_file", "args": { "target_file": "{{inputs.file}}" } }],
            "result": "{{read.stdout}}"
        }),
    );
    save(
        &config,
        json!({
            "version": 1, "name": "write-note", "triggers": ["chat"],
            "steps": [{ "id": "put", "tool": "write", "args": { "file_path": "new.txt", "contents": "x" } }]
        }),
    );
    // No chat trigger: not the tool's to run.
    save(
        &config,
        json!({
            "version": 1, "name": "nightly", "triggers": [{ "timer": "daily 02:00" }],
            "steps": [{ "id": "read", "tool": "read_file", "args": { "target_file": "notes.txt" } }]
        }),
    );
    (config, work)
}

fn call(config: &Path, work: &Path, args: Value) -> ToolOutput {
    let stop = || false;
    let ctx = ToolCtx {
        workspace: work,
        desktop: None,
        stop: &stop,
        tasks: None,
        owner: None,
        session: None,
    };
    run_in(config, &ctx, &args)
}

#[test]
fn lists_only_chat_routines() {
    let (config, _) = config();
    let schema = schema_in(&config);
    assert_eq!(
        schema["parameters"]["properties"]["name"]["enum"],
        json!(["read-notes", "write-note"])
    );
    let about = schema["description"].as_str().unwrap();
    assert!(
        about.ends_with(
            "Routines:\n- read-notes: Reads a notes file (inputs: file string = \"notes.txt\", lines integer)\n- write-note"
        ),
        "{about}"
    );
    assert!(visible(&config));
    assert!(!visible(&temp("empty")));
}

#[test]
fn runs_a_read_only_routine_to_its_result() {
    let (config, work) = config();
    let out = call(
        &config,
        &work,
        json!({ "name": "read-notes", "inputs": { "lines": "2" } }),
    );
    assert!(!out.failed, "{}", out.text);
    assert!(
        out.text.starts_with("<<<UNTRUSTED source=routine "),
        "{}",
        out.text
    );
    // The step's line, then the result: the file as read_file numbers it.
    assert!(
        out.text
            .contains("read: 1|alpha\n2|beta\n\n1|alpha\n2|beta"),
        "{}",
        out.text
    );
}

#[test]
fn a_change_step_is_refused_and_nothing_is_written() {
    let (config, work) = config();
    let out = call(&config, &work, json!({ "name": "write-note" }));
    assert!(out.failed);
    assert!(out.text.contains("Stopped at put:"), "{}", out.text);
    assert!(!work.join("new.txt").exists());
}

#[test]
fn bad_names_and_inputs_say_why() {
    let (config, work) = config();
    let out = call(&config, &work, json!({ "name": "nightly" }));
    assert_eq!(
        (out.failed, out.text.as_str()),
        (
            true,
            "No routine named \"nightly\". Routines: read-notes, write-note."
        )
    );
    let out = call(
        &config,
        &work,
        json!({ "name": "read-notes", "inputs": { "lines": "two" } }),
    );
    assert_eq!(
        (out.failed, out.text.as_str()),
        (true, "inputs.lines must be an integer")
    );
    let out = call(
        &config,
        &work,
        json!({ "name": "read-notes", "inputs": { "mode": "x" } }),
    );
    assert_eq!(
        (out.failed, out.text.as_str()),
        (true, "read-notes has no input named mode")
    );
}

#[test]
fn a_routine_may_not_start_a_routine() {
    let config = temp("nested");
    save(
        &config,
        json!({
            "version": 1, "name": "loop", "triggers": ["chat"],
            "steps": [{ "id": "again", "tool": "run_routine", "args": { "name": "loop" } }]
        }),
    );
    assert_eq!(crate::routine_triggers::for_chat(&config), Vec::new());
    assert!(!crate::routine_run::step_tool(NAME));
    assert!(crate::routine_run::step_tool("read_file"));
}
