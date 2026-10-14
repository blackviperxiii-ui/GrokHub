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

use super::defs::{deferred_desktop_tail, deferred_native_defs, eager_defs, find_def};
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
    let mut stable = Vec::new();
    for def in eager_defs() {
        if (def.visible)(gate) {
            stable.push((def.schema)());
        }
    }
    stable.extend(crate::session_tools::schemas());
    stable.push(crate::subagent::spawn_schema());
    if gate.readonly_session {
        stable.push(load_tool_schema_schema(index_lines(gate)));
        return Offer {
            stable,
            volatile: Vec::new(),
        };
    }
    // Self-manage stays on the stable list (same as legacy). Deferring them into
    // the index left full schemas missing after load_tool_schema because find_def
    // does not know those names.
    stable.extend(crate::self_manage::native_schemas());
    stable.push(load_tool_schema_schema(index_lines(gate)));
    let session = current_session().unwrap_or_default();
    let loaded = loaded_names(&session);
    let mut volatile = Vec::new();
    for name in &loaded {
        if let Some(def) = find_def(name) {
            if (def.visible)(gate) {
                volatile.push((def.schema)());
            }
        }
    }
    let desk = if gate.desktop {
        desktop::schemas()
    } else {
        Vec::new()
    };
    let native = desk.len();
    stable.extend(crate::mcp::schema_tools(native));
    Offer { stable, volatile }
}

fn legacy_schemas(gate: &Gate) -> Vec<Value> {
    let mut tools = Vec::new();
    for def in eager_defs() {
        if (def.visible)(gate) {
            tools.push((def.schema)());
        }
    }
    tools.extend(crate::session_tools::schemas());
    tools.push(crate::subagent::spawn_schema());
    if gate.readonly_session {
        return tools;
    }
    for def in deferred_native_defs()
        .iter()
        .chain(deferred_desktop_tail().iter())
    {
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

fn index_lines(gate: &Gate) -> String {
    let mut lines = Vec::new();
    for def in deferred_native_defs().iter().chain(deferred_desktop_tail()) {
        if (def.visible)(gate) {
            lines.push(format!("{} — {}", def.name, def.blurb));
        }
    }
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
        return ToolOutput::err(format!(
            "`{name}` is always available with its full schema; load_tool_schema is only for deferred tools"
        ));
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
    fn enabled_defers_write_until_load_tool_schema() {
        let gate = chat(false, false);
        begin_turn("conv-defer");
        let stable = stable_schemas(&gate, ON);
        assert!(
            !stable.iter().any(|t| t["name"] == "write"),
            "write must not be in the stable list"
        );
        assert!(
            stable.iter().any(|t| t["name"] == LOAD_TOOL_SCHEMA),
            "meta loader must stay in the stable list"
        );
        assert!(volatile_schemas(&gate, ON).is_empty());
        let out = run_load_tool_schema(&json!({"name": "write"}));
        assert!(!out.failed);
        assert!(out.text.contains("\"write\""));
        let volatile = volatile_schemas(&gate, ON);
        assert_eq!(volatile.len(), 1);
        assert_eq!(volatile[0]["name"], "write");
        let full = schemas_for_settings(&gate, ON);
        assert!(full.iter().any(|t| t["name"] == "write"));
    }

    #[test]
    fn stable_prefix_unchanged_when_only_volatile_grows() {
        let gate = chat(false, false);
        begin_turn("conv-stable");
        let before = serde_json::to_string(&stable_schemas(&gate, ON)).unwrap();
        run_load_tool_schema(&json!({"name": "write"}));
        run_load_tool_schema(&json!({"name": "run_terminal_command"}));
        let after = serde_json::to_string(&stable_schemas(&gate, ON)).unwrap();
        assert_eq!(before, after, "stable tool prefix must stay byte-identical");
        assert_eq!(volatile_schemas(&gate, ON).len(), 2);
    }

    #[test]
    fn begin_turn_clears_loaded_schemas() {
        let gate = chat(false, false);
        begin_turn("conv-clear-a");
        run_load_tool_schema(&json!({"name": "write"}));
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
    fn deferred_mode_keeps_self_manage_on_stable_list() {
        let gate = chat(false, false);
        begin_turn("conv-self-manage");
        let stable = stable_schemas(&gate, ON);
        assert!(
            stable.iter().any(|t| t["name"] == "skill_list"),
            "skill_list must stay on the stable list when deferred tools are on"
        );
        assert!(
            stable.iter().any(|t| t["name"] == "automation_create"),
            "automation_create must stay on the stable list when deferred tools are on"
        );
        let index = stable
            .iter()
            .find(|t| t["name"] == LOAD_TOOL_SCHEMA)
            .and_then(|t| t["description"].as_str())
            .unwrap_or("");
        assert!(
            !index.contains("skill_list —"),
            "self-manage must not be deferred into the load_tool_schema index"
        );
        let out = run_load_tool_schema(&json!({"name": "skill_list"}));
        assert!(out.failed, "{}", out.text);
        assert!(out.text.contains("always available"), "{}", out.text);
    }
}
