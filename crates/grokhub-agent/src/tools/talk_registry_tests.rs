//! LO-45: the tool list a Talk chat sends, its one upgrade, and loads.

use serde_json::{json, Value};

use super::*;
use crate::brain::{Brief, Intent};
use crate::gate::{Gate, PermMode};

const ON: Settings = Settings { enabled: true };
const OFF: Settings = Settings { enabled: false };

/// Pins deferred tools on for this thread so `load_tool_schema` reads them as on.
struct LazyOn;

impl LazyOn {
    fn set() -> Self {
        set_enabled_for_tests(Some(true));
        Self
    }
}

impl Drop for LazyOn {
    fn drop(&mut self) {
        set_enabled_for_tests(None);
    }
}

fn gate(readonly: bool) -> Gate {
    Gate {
        mode: PermMode::Ask,
        readonly_session: readonly,
        attended: true,
        desktop: false,
    }
}

fn chat() -> Brief {
    Brief {
        intent: Intent::Chat,
        domain: "general".into(),
        confidence: 100,
        ..Default::default()
    }
}

fn task() -> Brief {
    Brief {
        intent: Intent::Task,
        domain: "files".into(),
        confidence: 100,
        ..Default::default()
    }
}

/// One user turn in `session`: its Brief, then the list the model is sent.
fn turn(session: &str, brief: &Brief, cfg: Settings) -> Vec<Value> {
    note_brief(session, brief);
    begin_turn(session);
    schemas_for_settings(&gate(false), cfg)
}

fn wire(tools: &[Value]) -> String {
    serde_json::to_string(tools).unwrap()
}

fn names(tools: &[Value]) -> Vec<String> {
    tools
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect()
}

fn index(tools: &[Value]) -> String {
    tools
        .iter()
        .find(|t| t["name"] == LOAD_TOOL_SCHEMA)
        .and_then(|t| t["description"].as_str())
        .unwrap_or("")
        .to_string()
}

#[test]
fn talk_chat_indexes_the_workspace_tools_instead_of_sending_them() {
    let tools = turn("talk-reg-names", &chat(), ON);
    let sent = names(&tools);
    for name in ["write", "read_file", "run_terminal_command"] {
        assert!(!sent.iter().any(|n| n == name), "{name} sent: {sent:?}");
    }
    for name in TALK_DEMOTED {
        assert!(!sent.iter().any(|n| n == name), "{name} sent: {sent:?}");
    }
    assert!(sent.iter().any(|n| n == "web_search"), "web_search stays");
    let index = index(&tools);
    assert!(
        index.contains("\nread_file — Read a workspace file\n"),
        "{index}"
    );
    assert!(
        index.contains("run_terminal_command — Run a shell command"),
        "{index}"
    );
    let lines: Vec<&str> = index
        .split_once("Deferred tools (index):\n")
        .map(|(_, l)| l)
        .unwrap_or("")
        .lines()
        .collect();
    let mut sorted = lines.clone();
    sorted.sort();
    assert_eq!(lines, sorted, "the index stays sorted");
}

#[test]
fn full_chat_sends_todays_list() {
    begin_turn("talk-reg-today");
    let today = schemas_for_settings(&gate(false), ON);
    let full = turn("talk-reg-full", &task(), ON);
    assert_eq!(wire(&full), wire(&today));
    assert!(names(&full).iter().any(|n| n == "read_file"));
}

#[test]
fn talk_then_task_appends_the_demoted_list_once_then_holds() {
    let s = "talk-reg-upgrade";
    let first = turn(s, &chat(), ON);
    let second = turn(s, &task(), ON);
    let head = wire(&first);
    let grown = wire(&second);
    assert!(
        grown.starts_with(head.trim_end_matches(']')),
        "the upgrade only appends"
    );
    let added: Vec<String> = names(&second)[first.len()..].to_vec();
    assert_eq!(added, TALK_DEMOTED.map(str::to_string));
    let third = turn(s, &chat(), ON);
    assert_eq!(wire(&third), grown, "byte-identical after the upgrade");
    let fourth = turn(s, &task(), ON);
    assert_eq!(wire(&fourth), grown);
}

#[test]
fn task_then_chat_stays_full() {
    begin_turn("talk-reg-today-2");
    let today = wire(&schemas_for_settings(&gate(false), ON));
    let s = "talk-reg-task-first";
    assert_eq!(wire(&turn(s, &task(), ON)), today);
    assert_eq!(wire(&turn(s, &chat(), ON)), today);
}

#[test]
fn load_read_file_upgrades_a_talk_chat_and_still_fails_on_full() {
    let _lazy = LazyOn::set();
    let s = "talk-reg-load";
    let before = turn(s, &chat(), ON);
    let out = run_load_tool_schema(&json!({"name": "read_file"}));
    assert!(!out.failed, "{}", out.text);
    assert!(out.text.contains("\"read_file\""), "{}", out.text);
    let after = schemas_for_settings(&gate(false), ON);
    let added: Vec<String> = names(&after)[before.len()..].to_vec();
    assert_eq!(added, TALK_DEMOTED.map(str::to_string), "all at once");
    // Loading another demoted tool now changes nothing.
    assert!(!run_load_tool_schema(&json!({"name": "write"})).failed);
    assert_eq!(wire(&schemas_for_settings(&gate(false), ON)), wire(&after));

    turn("talk-reg-load-full", &task(), ON);
    let out = run_load_tool_schema(&json!({"name": "read_file"}));
    assert!(out.failed);
    assert!(out.text.contains("always available"), "{}", out.text);
}

#[test]
fn talk_leaves_lazy_off_and_job_sets_as_they_were() {
    begin_turn("talk-reg-legacy-none");
    let legacy = wire(&schemas_for_settings(&gate(false), OFF));
    assert_eq!(wire(&turn("talk-reg-legacy", &chat(), OFF)), legacy);

    let s = "talk-reg-job";
    let _job = crate::run::bind_job_tool_set(s, crate::tools::ToolSet::background());
    begin_turn("talk-reg-job-none");
    let today = wire(&schemas_for_settings(&gate(false), ON));
    assert_eq!(wire(&turn(s, &chat(), ON)), today);
    // A child of a job session inherits the job, so it stays Full too.
    note_brief(&format!("{s}/child"), &chat());
    begin_turn(&format!("{s}/child"));
    assert_eq!(wire(&schemas_for_settings(&gate(false), ON)), today);
    crate::run::clear_job_tool_set(s);
}

#[test]
fn read_only_talk_chat_upgrades_to_the_read_tools_it_may_see() {
    let s = "talk-reg-readonly";
    note_brief(s, &chat());
    begin_turn(s);
    let ro = gate(true);
    let first = schemas_for_settings(&ro, ON);
    assert!(!names(&first).iter().any(|n| n == "read_file"));
    assert!(
        !index(&first).contains("write —"),
        "write is hidden read-only"
    );
    note_brief(s, &task());
    let after = schemas_for_settings(&ro, ON);
    let added: Vec<String> = names(&after)[first.len()..].to_vec();
    assert_eq!(
        added,
        [
            "read_file",
            "list_dir",
            "grep",
            "glob",
            "get_command_or_subagent_output",
            "diagnose"
        ]
    );
}
