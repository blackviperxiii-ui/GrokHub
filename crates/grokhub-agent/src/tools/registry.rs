//! Tool registry (Reason harness R4): optional deferred native schemas.
//!
//! Off by default (`{config_dir}/deferred_tools.toml`). When enabled, rarely used
//! native tools are listed in a compact index; the model calls `load_tool_schema`
//! to pull a full schema into the volatile tail of the tools list so the stable
//! prefix stays byte-identical across turns when only loaded tools change.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use crate::gate::Gate;

use super::defs::{action_defs, all_defs, desktop_tail_defs, find_def, read_defs, ToolDef};
use super::ToolOutput;

const CONFIG_NAME: &str = "deferred_tools.toml";

pub const LOAD_TOOL_SCHEMA: &str = "load_tool_schema";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    pub enabled: bool,
}

thread_local! {
    static SESSION: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn loaded_by_session() -> &'static Mutex<BTreeMap<String, BTreeSet<String>>> {
    static MAP: OnceLock<Mutex<BTreeMap<String, BTreeSet<String>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn config_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(CONFIG_NAME)
}

pub fn settings(config_dir: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(config_path(config_dir)) else {
        return Settings::default();
    };
    parse_file(&text)
}

fn parse_file(text: &str) -> Settings {
    let mut enabled = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("enabled") && value.trim().eq_ignore_ascii_case("true") {
            enabled = true;
        }
    }
    Settings { enabled }
}

/// Bind the conversation id for deferred schema state on this thread.
pub fn bind_session(session: &str) {
    SESSION.with(|cell| {
        *cell.borrow_mut() = Some(session.to_string());
    });
}

/// Clear loaded schemas for a new user turn (same chat, fresh tool list).
pub fn begin_turn(session: &str) {
    bind_session(session);
    loaded_by_session()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(session.to_string(), BTreeSet::new());
}

fn current_session() -> Option<String> {
    SESSION.with(|cell| cell.borrow().clone())
}

fn loaded_names(session: &str) -> BTreeSet<String> {
    loaded_by_session()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
        .cloned()
        .unwrap_or_default()
}

fn remember_loaded(session: &str, name: &str) {
    loaded_by_session()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(session.to_string())
        .or_default()
        .insert(name.to_string());
}

/// Stable tool schemas (eager natives, session tools, spawn, meta, MCP) for prefix-cache tests.
#[cfg(test)]
fn stable_schemas(gate: &Gate, cfg: Settings) -> Vec<Value> {
    offer(gate, cfg).stable
}

/// Volatile tail: full schemas loaded this turn via [`LOAD_TOOL_SCHEMA`].
#[cfg(test)]
fn volatile_schemas(gate: &Gate, cfg: Settings) -> Vec<Value> {
    offer(gate, cfg).volatile
}

pub fn schemas_for(gate: &Gate) -> Vec<Value> {
    schemas_for_settings(gate, settings(&crate::perm::config_dir()))
}

fn schemas_for_settings(gate: &Gate, cfg: Settings) -> Vec<Value> {
    let parts = offer(gate, cfg);
    parts.stable.into_iter().chain(parts.volatile).collect()
}

struct Offer {
    stable: Vec<Value>,
    volatile: Vec<Value>,
}

fn offer(gate: &Gate, cfg: Settings) -> Offer {
    if !cfg.enabled {
        return Offer {
            stable: legacy_schemas(gate),
            volatile: Vec::new(),
        };
    }
    let rows = offered_defs(gate);
    let mut stable: Vec<Value> = rows
        .iter()
        .filter(|def| !def.defer)
        .map(|def| (def.schema)())
        .collect();
    stable.extend(crate::session_tools::schemas());
    stable.push(crate::subagent::spawn_schema());
    let selfs = if gate.readonly_session {
        Vec::new()
    } else {
        self_rows()
    };
    // Self-manage reads (`*_list`) stay eager; its writes are deferred like the rest.
    stable.extend(selfs.iter().filter(|r| r.read).map(|r| r.schema.clone()));
    stable.push(load_tool_schema_schema(index_lines(&rows, &selfs)));
    let session = current_session().unwrap_or_default();
    let volatile = loaded_names(&session)
        .iter()
        .filter_map(|name| {
            if let Some(def) = rows.iter().find(|d| d.defer && d.name == name) {
                return Some((def.schema)());
            }
            selfs
                .iter()
                .find(|r| !r.read && r.name == *name)
                .map(|r| r.schema.clone())
        })
        .collect();
    if !gate.readonly_session {
        let native = if gate.desktop {
            desktop::schemas().len()
        } else {
            0
        };
        stable.extend(crate::mcp::schema_tools(native));
    }
    Offer { stable, volatile }
}

fn legacy_schemas(gate: &Gate) -> Vec<Value> {
    let mut tools = Vec::new();
    for def in read_defs() {
        if (def.visible)(gate) {
            tools.push((def.schema)());
        }
    }
    tools.extend(crate::session_tools::schemas());
    tools.push(crate::subagent::spawn_schema());
    if gate.readonly_session {
        return tools;
    }
    for def in action_defs().iter().chain(desktop_tail_defs().iter()) {
        if (def.visible)(gate) {
            tools.push((def.schema)());
        }
    }
    tools.extend(crate::self_manage::native_schemas());
    let native = if gate.desktop {
        desktop::schemas().len()
    } else {
        0
    };
    tools.extend(crate::mcp::schema_tools(native));
    tools
}

/// Native rows the gate offers: read-only sessions get the read rows only, as in legacy.
fn offered_defs(gate: &Gate) -> Vec<&'static ToolDef> {
    let rows: Vec<&'static ToolDef> = if gate.readonly_session {
        read_defs().iter().collect()
    } else {
        all_defs().collect()
    };
    rows.into_iter().filter(|def| (def.visible)(gate)).collect()
}

/// One self-manage tool as the native engine offers it.
struct SelfRow {
    name: String,
    blurb: String,
    schema: Value,
    read: bool,
}

fn self_rows() -> Vec<SelfRow> {
    crate::self_manage::native_schemas()
        .into_iter()
        .map(|schema| {
            let name = schema["name"].as_str().unwrap_or("").to_string();
            let desc = schema["description"].as_str().unwrap_or("");
            let blurb = desc.split('.').next().unwrap_or(desc).trim().to_string();
            SelfRow {
                read: crate::self_manage::is_read(&name),
                name,
                blurb,
                schema,
            }
        })
        .collect()
}

/// One `name — blurb` line per deferred tool the gate shows, sorted by name so the
/// index (and the stable prefix it sits in) is byte-identical from build to build.
fn index_lines(rows: &[&ToolDef], selfs: &[SelfRow]) -> String {
    let native = rows
        .iter()
        .filter(|def| def.defer)
        .map(|def| format!("{} — {}", def.name, def.blurb));
    let own = selfs
        .iter()
        .filter(|r| !r.read)
        .map(|r| format!("{} — {}", r.name, r.blurb));
    let mut lines: Vec<String> = native.chain(own).collect();
    lines.sort();
    lines.join("\n")
}

fn load_tool_schema_schema(index: String) -> Value {
    json!({
        "type": "function",
        "name": LOAD_TOOL_SCHEMA,
        "description": format!(
            "Load the full JSON schema for a deferred native tool by name. \
             Loaded schemas are added to the volatile tail of the tools list for this chat turn. \
             Deferred tools (index):\n{index}"
        ),
        "parameters": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Native tool name from the index."
                }
            },
            "required": ["name"],
            "additionalProperties": false
        }
    })
}

pub fn run_load_tool_schema(args: &Value) -> ToolOutput {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return ToolOutput::err("name is required");
    }
    if crate::self_manage::self_tool(name).is_some() {
        if crate::self_manage::is_read(name) {
            return ToolOutput::err(format!(
                "`{name}` is always available with its full schema; load_tool_schema is only for deferred tools"
            ));
        }
        return match self_rows().into_iter().find(|r| r.name == name) {
            Some(row) => finish_load(name, row.schema),
            None => ToolOutput::err(format!("unknown tool `{name}`")),
        };
    }
    let Some(def) = find_def(name) else {
        return ToolOutput::err(format!("unknown tool `{name}`"));
    };
    if !def.defer {
        return ToolOutput::err(format!(
            "`{name}` is always available with its full schema; load_tool_schema is only for deferred tools"
        ));
    }
    finish_load(name, (def.schema)())
}

fn finish_load(name: &str, schema: Value) -> ToolOutput {
    let session = current_session().filter(|s| !s.is_empty());
    if let Some(session) = session {
        remember_loaded(&session, name);
    }
    let text = serde_json::to_string_pretty(&schema).unwrap_or_else(|_| schema.to_string());
    ToolOutput::ok(text)
}

use super::desktop;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{Gate, PermMode};

    fn chat(readonly: bool, desktop: bool) -> Gate {
        Gate {
            mode: PermMode::Always,
            readonly_session: readonly,
            attended: true,
            desktop,
        }
    }

    const ON: Settings = Settings { enabled: true };
    const OFF: Settings = Settings { enabled: false };

    #[test]
    fn settings_default_off_and_file_enables() {
        assert!(!parse_file("").enabled);
        assert!(parse_file("enabled = true").enabled);
        let dir = crate::harness::test_dir("deferred-tools-default");
        let _ = std::fs::remove_file(config_path(&dir));
        assert!(!settings(&dir).enabled);
        std::fs::write(config_path(&dir), "enabled = true\n").unwrap();
        assert!(settings(&dir).enabled);
    }

    #[test]
    fn disabled_matches_legacy_tool_list() {
        let gate = chat(false, false);
        let legacy = legacy_schemas(&gate);
        let via = schemas_for_settings(&gate, OFF);
        assert_eq!(legacy.len(), via.len());
        let legacy_names: Vec<_> = legacy.iter().filter_map(|t| t["name"].as_str()).collect();
        let via_names: Vec<_> = via.iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(legacy_names, via_names);
    }

    #[test]
    fn enabled_defers_web_fetch_until_load_tool_schema() {
        let gate = chat(false, false);
        begin_turn("conv-defer");
        let stable = stable_schemas(&gate, ON);
        assert!(
            !stable.iter().any(|t| t["name"] == "web_fetch"),
            "web_fetch must not be in the stable list"
        );
        assert!(
            stable.iter().any(|t| t["name"] == LOAD_TOOL_SCHEMA),
            "meta loader must stay in the stable list"
        );
        assert!(volatile_schemas(&gate, ON).is_empty());
        let out = run_load_tool_schema(&json!({"name": "web_fetch"}));
        assert!(!out.failed);
        assert!(out.text.contains("\"web_fetch\""));
        let volatile = volatile_schemas(&gate, ON);
        assert_eq!(volatile.len(), 1);
        assert_eq!(volatile[0]["name"], "web_fetch");
        let full = schemas_for_settings(&gate, ON);
        assert!(full.iter().any(|t| t["name"] == "web_fetch"));
    }

    #[test]
    fn stable_prefix_unchanged_when_only_volatile_grows() {
        let gate = chat(false, false);
        begin_turn("conv-stable");
        let before = serde_json::to_string(&stable_schemas(&gate, ON)).unwrap();
        run_load_tool_schema(&json!({"name": "web_fetch"}));
        run_load_tool_schema(&json!({"name": "scheduler_list"}));
        let after = serde_json::to_string(&stable_schemas(&gate, ON)).unwrap();
        assert_eq!(before, after, "stable tool prefix must stay byte-identical");
        assert_eq!(volatile_schemas(&gate, ON).len(), 2);
    }

    #[test]
    fn begin_turn_clears_loaded_schemas() {
        let gate = chat(false, false);
        begin_turn("conv-clear-a");
        run_load_tool_schema(&json!({"name": "web_fetch"}));
        assert_eq!(volatile_schemas(&gate, ON).len(), 1);
        begin_turn("conv-clear-b");
        assert!(volatile_schemas(&gate, ON).is_empty());
    }

    #[test]
    fn load_tool_schema_refuses_eager_tools() {
        begin_turn("conv-eager");
        let out = run_load_tool_schema(&json!({"name": "read_file"}));
        assert!(out.failed);
        assert!(out.text.contains("always available"));
    }

    #[test]
    fn deferred_mode_keeps_self_manage_reads_eager_and_loads_its_writes() {
        let gate = chat(false, false);
        begin_turn("conv-self-manage");
        let stable = stable_schemas(&gate, ON);
        assert!(
            stable.iter().any(|t| t["name"] == "skill_list"),
            "skill_list must stay on the stable list when deferred tools are on"
        );
        assert!(
            !stable.iter().any(|t| t["name"] == "automation_create"),
            "automation_create is deferred"
        );
        let index = index_of(&stable);
        assert!(!index.contains("skill_list —"), "{index}");
        assert!(
            index.contains("automation_create — Create a Loadout automation\n"),
            "{index}"
        );
        let out = run_load_tool_schema(&json!({"name": "skill_list"}));
        assert!(out.failed, "{}", out.text);
        assert!(out.text.contains("always available"), "{}", out.text);
        // The schema a load returns is the one the next request carries in full.
        let out = run_load_tool_schema(&json!({"name": "automation_create"}));
        assert!(!out.failed, "{}", out.text);
        let volatile = volatile_schemas(&gate, ON);
        assert_eq!(volatile.len(), 1);
        assert_eq!(volatile[0]["name"], "automation_create");
        assert_eq!(
            volatile[0]["parameters"]["required"],
            json!(["instructions", "every", "reason"])
        );
    }

    fn index_of(stable: &[Value]) -> String {
        stable
            .iter()
            .find(|t| t["name"] == LOAD_TOOL_SCHEMA)
            .and_then(|t| t["description"].as_str())
            .unwrap_or("")
            .to_string()
    }

    fn profile(mode: PermMode, readonly: bool, attended: bool) -> Gate {
        Gate {
            mode,
            readonly_session: readonly,
            attended,
            desktop: false,
        }
    }

    /// Native eager payload one profile sends with deferred tools on (MCP tools
    /// come from the user's config, so they are left out of the budget).
    fn eager_payload(gate: &Gate, set: &super::super::set::ToolSet) -> Vec<Value> {
        begin_turn("conv-budget");
        stable_schemas(gate, ON)
            .into_iter()
            .filter(|t| {
                let name = t["name"].as_str().unwrap_or("");
                set.allows(name)
                    && name != "search_tool"
                    && name != "use_tool"
                    && !name.contains("__")
            })
            .collect()
    }

    fn names(tools: &[Value]) -> Vec<&str> {
        tools.iter().filter_map(|t| t["name"].as_str()).collect()
    }

    /// Schema byte budget per profile with deferred tools on. These are the sizes
    /// after the lean eager set (before it: Chat/Agent 13736, Plan/Ask 6900,
    /// background 6954). To raise one, add the tool to the right list in
    /// `defs.rs`, rerun this test, and put the new size and the reason in the PR.
    /// The deferred `local_ai_status` / `local_ai_selftest` index lines added
    /// 143 bytes to Chat/Agent and background, and 74 to Plan/Ask. The deferred
    /// `pulse_steam` index line added 53 to Chat/Agent and background, and the
    /// eager read `know_me_tags` (the agent's only view of Know me) 184.
    /// Eager `web_search` (OpenRouter / local always see the schema) added 472
    /// to Chat/Agent (10_503 → 10_975) and background (9_323 → 9_795).
    #[test]
    fn eager_schema_payload_stays_within_budget_per_profile() {
        use super::super::set::ToolSet;
        let full = ToolSet::FULL;
        let cases = [
            (
                "chat",
                profile(PermMode::Ask, false, true),
                full.clone(),
                10_975,
            ),
            (
                "agent",
                profile(PermMode::Always, false, true),
                full.clone(),
                10_975,
            ),
            ("plan/ask", profile(PermMode::Ask, true, true), full, 6_824),
            (
                "background",
                profile(PermMode::Auto, false, false),
                ToolSet::background(),
                9_795,
            ),
        ];
        for (label, gate, set, budget) in cases {
            let bytes = serde_json::to_string(&eager_payload(&gate, &set))
                .unwrap()
                .len();
            assert!(
                bytes <= budget,
                "{label}: {bytes} schema bytes > budget {budget}"
            );
        }
    }

    #[test]
    fn eager_set_is_the_general_coding_tools() {
        let chat_tools = eager_payload(
            &profile(PermMode::Ask, false, true),
            &super::super::set::ToolSet::FULL,
        );
        assert_eq!(
            names(&chat_tools),
            vec![
                "read_file",
                "list_dir",
                "grep",
                "glob",
                "get_command_or_subagent_output",
                "skill",
                "diagnose",
                "write",
                "search_replace",
                "run_terminal_command",
                "web_search",
                "todo_write",
                "ask_user_question",
                "report_findings",
                "enter_plan_mode",
                "exit_plan_mode",
                "spawn_subagent",
                "skill_list",
                "connection_list",
                "automation_list",
                "know_me_tags",
                LOAD_TOOL_SCHEMA,
            ]
        );
        let plan_tools = eager_payload(
            &profile(PermMode::Ask, true, true),
            &super::super::set::ToolSet::FULL,
        );
        assert!(!names(&plan_tools).contains(&"write"));
        assert!(!names(&plan_tools).contains(&"run_terminal_command"));
    }

    #[test]
    fn deferred_index_is_sorted_and_byte_stable() {
        let gate = chat(false, true);
        let first = index_of(&stable_schemas(&gate, ON));
        let second = index_of(&stable_schemas(&gate, ON));
        assert_eq!(first, second);
        let lines: Vec<&str> = first
            .split_once("Deferred tools (index):\n")
            .map(|(_, list)| list)
            .unwrap_or("")
            .lines()
            .collect();
        let mut sorted = lines.clone();
        sorted.sort();
        assert_eq!(lines, sorted);
        assert_eq!(
            lines.first(),
            Some(&"automation_create — Create a Loadout automation")
        );
        assert!(lines.contains(&"scheduler_list — List scheduled automations"));
        assert!(lines.contains(&"screenshot — Capture the desktop screen"));
        assert!(lines.contains(
            &"connection_add — Add a connection (MCP server) to the cabin's own MCP config"
        ));
        assert!(lines
            .iter()
            .all(|l| l.split_once(" — ").is_some() && !l.contains('\n')));
    }

    #[test]
    fn moved_tools_load_and_keep_their_gate_decisions() {
        use crate::gate::{decide, Decision};
        let desk = chat(false, true);
        for name in [
            "scheduler_list",
            "scheduler_create",
            "image_generate",
            "screenshot",
            "connection_delete",
            "connection_add",
            "skill_create",
        ] {
            begin_turn("conv-moved");
            let out = run_load_tool_schema(&json!({"name": name}));
            assert!(!out.failed, "{name}: {}", out.text);
            let volatile = volatile_schemas(&desk, ON);
            assert_eq!(names(&volatile), vec![name]);
        }
        let ro = chat(true, false);
        assert_eq!(decide(&ro, "scheduler_list", false, None), Decision::Run);
        assert_eq!(decide(&ro, "connection_list", false, None), Decision::Run);
        for name in [
            "scheduler_create",
            "image_generate",
            "connection_add",
            "write",
        ] {
            assert_eq!(
                decide(&ro, name, false, None),
                Decision::Refuse(crate::gate::readonly_refusal(name)),
                "{name}"
            );
        }
        // A read-only session can load a deferred read tool and gets its schema back.
        begin_turn("conv-moved-ro");
        assert!(!run_load_tool_schema(&json!({"name": "scheduler_list"})).failed);
        assert_eq!(names(&volatile_schemas(&ro, ON)), vec!["scheduler_list"]);
    }

    #[test]
    fn a_loaded_scheduler_list_dispatches() {
        let dir = crate::harness::test_dir("deferred-scheduler-list");
        let ctx = crate::tools::ToolCtx {
            workspace: &dir,
            desktop: None,
            stop: &|| false,
            tasks: None,
            owner: None,
            session: Some("conv-dispatch"),
        };
        begin_turn("conv-dispatch");
        let loaded = crate::tools::dispatch(&ctx, LOAD_TOOL_SCHEMA, r#"{"name":"scheduler_list"}"#);
        assert!(!loaded.failed);
        let listed = crate::tools::dispatch(&ctx, "scheduler_list", "{}");
        assert!(!listed.failed);
    }
}
