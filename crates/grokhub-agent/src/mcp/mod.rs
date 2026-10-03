//! MCP client. Sync JSON-RPC over stdio, streamable HTTP, and legacy SSE.
//! One worker thread per stdio child or HTTP read. No tokio.

mod config;
mod elicit;
mod http;
mod names;
mod rpc;
mod stdio;

pub use config::{
    config_file, import_documents, import_paths, load_servers, read_mcp_text, target_label,
    ImportReport, ServerDef,
};
pub(crate) use elicit::wait_elicit;
pub use elicit::{attach_elicit, detach_elicit, ElicitInbox, ElicitNote, ElicitView};

pub(crate) use elicit::with_elicit;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::gate::{self, Decision, Gate, PermMode};
use crate::perm::{self, Action, Policy};
use crate::tools::ToolOutput;

use config::TransportDef;
use rpc::RawTool;

/// Individual tool schemas at this count. Above it, `search_tool` and `use_tool`.
const DEFER_AFTER: usize = 40;

static GEN: AtomicU64 = AtomicU64::new(0);

pub fn invalidate() {
    GEN.fetch_add(1, Ordering::Relaxed);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorRow {
    pub name: String,
    pub status: String,
    pub tool_count: usize,
    pub last_error: String,
    pub detail: String,
}

#[derive(Clone)]
struct ToolRec {
    server: String,
    raw: String,
    qualified: String,
    description: String,
    schema: Value,
    read_only: bool,
}

struct SlotInner {
    name: String,
    def: ServerDef,
    raw: Vec<RawTool>,
    tools: Vec<ToolRec>,
    last_error: Option<String>,
    tried: bool,
    conn: Option<Conn>,
}

impl SlotInner {
    fn new(name: String, def: ServerDef) -> Self {
        Self {
            name,
            def,
            raw: Vec::new(),
            tools: Vec::new(),
            last_error: None,
            tried: false,
            conn: None,
        }
    }
}

enum Conn {
    Stdio(stdio::StdioConn),
    Http(http::HttpConn),
}

impl Conn {
    fn call(
        &mut self,
        name: &str,
        args: &Value,
        timeout: std::time::Duration,
    ) -> Result<(String, bool), String> {
        match self {
            Conn::Stdio(conn) => conn.call(name, args, timeout),
            Conn::Http(conn) => conn.call(name, args, timeout),
        }
    }

    fn ping(&mut self, timeout: std::time::Duration) -> Result<(), String> {
        match self {
            Conn::Stdio(conn) => conn.ping(timeout),
            Conn::Http(conn) => conn.ping(timeout),
        }
    }
}

struct State {
    path: PathBuf,
    gen: u64,
    mtime: Option<SystemTime>,
    loaded: bool,
    workspace: PathBuf,
    slots: BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>>,
}

impl State {
    fn empty() -> Self {
        Self {
            path: PathBuf::new(),
            gen: 0,
            mtime: None,
            loaded: false,
            workspace: PathBuf::new(),
            slots: BTreeMap::new(),
        }
    }
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::empty()))
}

fn lock_state() -> MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|err| err.into_inner())
}

fn lock_slot(slot: &Mutex<SlotInner>) -> MutexGuard<'_, SlotInner> {
    slot.lock().unwrap_or_else(|err| err.into_inner())
}

pub fn set_workspace(path: &Path) {
    lock_state().workspace = path.to_path_buf();
}

pub fn shutdown_all() {
    let mut held = lock_state();
    for slot in held.slots.values() {
        lock_slot(slot).conn.take();
    }
    held.slots.clear();
    held.loaded = false;
}

pub fn configured() -> Vec<DoctorRow> {
    config::load_servers()
        .into_iter()
        .map(|(name, def)| DoctorRow {
            name,
            status: if def.enabled {
                "not checked"
            } else {
                "disabled"
            }
            .into(),
            tool_count: 0,
            last_error: String::new(),
            detail: config::target_label(&def),
        })
        .collect()
}

pub fn doctor() -> Vec<DoctorRow> {
    let _ = current_tools();
    let slots = slots_now();
    let mut rows = Vec::new();
    for (name, slot) in slots {
        let mut inner = lock_slot(&slot);
        if !inner.def.enabled {
            rows.push(DoctorRow {
                name,
                status: "disabled".into(),
                tool_count: 0,
                last_error: String::new(),
                detail: config::target_label(&inner.def),
            });
            continue;
        }
        let ping_for = std::time::Duration::from_secs(5).min(inner.def.tool_timeout);
        if let Some(conn) = inner.conn.as_mut() {
            if let Err(err) = conn.ping(ping_for) {
                inner.last_error = Some(clip_err(&err));
                inner.conn.take();
            }
        }
        let status = if inner.conn.is_some() && inner.last_error.is_none() {
            "connected"
        } else if inner.last_error.is_some() {
            "error"
        } else {
            "stopped"
        };
        rows.push(DoctorRow {
            name,
            status: status.into(),
            tool_count: inner.tools.len(),
            last_error: inner.last_error.clone().unwrap_or_default(),
            detail: config::target_label(&inner.def),
        });
    }
    rows
}

pub fn restart(name: &str) -> Result<(), String> {
    let (slots, workspace) = load_slots();
    let Some(slot) = slots.get(name).cloned() else {
        return Err(format!("MCP server `{name}` is not configured"));
    };
    {
        let mut inner = lock_slot(&slot);
        if !inner.def.enabled {
            return Err(format!("MCP server `{name}` is disabled"));
        }
        inner.conn.take();
        inner.tried = false;
        inner.last_error = None;
        inner.raw.clear();
        inner.tools.clear();
    }
    connect_if_needed(&slot, &workspace);
    requalify(&slots);
    let inner = lock_slot(&slot);
    if inner.conn.is_none() {
        return Err(inner
            .last_error
            .clone()
            .unwrap_or_else(|| format!("MCP server `{name}` stopped")));
    }
    Ok(())
}

pub(crate) fn schema_tools() -> Vec<Value> {
    let tools = current_tools();
    if tools.len() > DEFER_AFTER {
        return vec![search_schema(), use_schema()];
    }
    tools.iter().map(one_schema).collect()
}

pub(crate) fn try_dispatch(name: &str, args: &Value) -> Option<ToolOutput> {
    if name == "search_tool" {
        return Some(search_output(args));
    }
    if name != "use_tool" && !name.contains("__") {
        return None;
    }
    let tools = current_tools();
    if name == "use_tool" {
        if tools.len() <= DEFER_AFTER {
            return None;
        }
        return Some(use_output(&tools, args));
    }
    let rec = tools.into_iter().find(|tool| tool.qualified == name)?;
    #[cfg(test)]
    if test_snapshot().is_some() {
        return Some(ToolOutput::ok(format!("test-call {}", rec.qualified)));
    }
    Some(call_qualified(&rec, args))
}

pub(crate) fn search_output(args: &Value) -> ToolOutput {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let tools = current_tools();
    let mut lines = Vec::new();
    for tool in &tools {
        let hay = format!("{} {}", tool.qualified, tool.description).to_ascii_lowercase();
        if query.is_empty() || hay.contains(&query) {
            lines.push(format!("{} — {}", tool.qualified, tool.description));
        }
        if lines.len() == 50 {
            break;
        }
    }
    if lines.is_empty() {
        ToolOutput::ok("No matching MCP tools")
    } else {
        ToolOutput::ok(lines.join("\n"))
    }
}

enum Resolved {
    Known(String),
    Missing(String),
}

pub(crate) fn permission(
    gate: &Gate,
    name: &str,
    arguments: &str,
    latched_always: bool,
    policy: Option<&Policy>,
) -> Option<Decision> {
    match resolve(name, arguments)? {
        Resolved::Missing(target) => Some(Decision::Refuse(format!("unknown tool `{target}`"))),
        Resolved::Known(target) => Some(decide_mcp(gate, &target, latched_always, policy)),
    }
}

pub(crate) fn is_mcp_call(name: &str, arguments: &str) -> bool {
    matches!(resolve(name, arguments), Some(Resolved::Known(_)))
}

pub(crate) fn has_ask_rule(policy: &Policy, name: &str, arguments: &str) -> bool {
    let Some(Resolved::Known(target)) = resolve(name, arguments) else {
        return false;
    };
    rule(policy, &target, Action::Ask)
}

fn decide_mcp(gate: &Gate, name: &str, latched_always: bool, policy: Option<&Policy>) -> Decision {
    // A server `readOnlyHint` does not grant a run. An allow rule does.
    if gate.readonly_session {
        return Decision::Refuse(gate::readonly_refusal(name));
    }
    if let Some(policy) = policy {
        if rule(policy, name, Action::Deny) {
            return Decision::Refuse(gate::mcp_policy_deny(name));
        }
        if rule(policy, name, Action::Ask) {
            return if gate.attended {
                Decision::Ask
            } else {
                Decision::Refuse(gate::mcp_policy_deny(name))
            };
        }
        if rule(policy, name, Action::Allow) {
            return Decision::Run;
        }
    }
    if gate.mode == PermMode::Always || latched_always {
        return Decision::Run;
    }
    if gate.attended {
        return Decision::Ask;
    }
    Decision::Refuse(gate::mcp_policy_deny(name))
}

fn rule(policy: &Policy, name: &str, action: Action) -> bool {
    policy
        .rules
        .iter()
        .any(|item| item.action == action && perm::mcp_matches(item, name))
}

fn resolve(name: &str, arguments: &str) -> Option<Resolved> {
    if name != "use_tool" && !name.contains("__") {
        return None;
    }
    let tools = current_tools();
    if name == "use_tool" {
        if tools.len() <= DEFER_AFTER {
            return None;
        }
        let target = arg_name(arguments);
        if target.is_empty() {
            return Some(Resolved::Missing("use_tool".into()));
        }
        if tools.iter().any(|tool| tool.qualified == target) {
            return Some(Resolved::Known(target));
        }
        return Some(Resolved::Missing(target));
    }
    if tools.iter().any(|tool| tool.qualified == name) {
        Some(Resolved::Known(name.to_string()))
    } else {
        None
    }
}

fn arg_name(arguments: &str) -> String {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|v| {
            v.get("name")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn use_output(tools: &[ToolRec], args: &Value) -> ToolOutput {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return ToolOutput::err("unknown tool `use_tool`");
    }
    let Some(rec) = tools.iter().find(|tool| tool.qualified == name) else {
        return ToolOutput::err(format!("unknown tool `{name}`"));
    };
    let call_args = normalize_args(args.get("arguments"));
    #[cfg(test)]
    if test_snapshot().is_some() {
        return ToolOutput::ok(format!("test-call {}", rec.qualified));
    }
    call_qualified(rec, &call_args)
}

fn normalize_args(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return json!({});
    };
    if let Some(text) = value.as_str() {
        return serde_json::from_str(text).unwrap_or_else(|_| json!({}));
    }
    if value.is_object() {
        value.clone()
    } else {
        json!({})
    }
}

fn call_qualified(rec: &ToolRec, args: &Value) -> ToolOutput {
    let slots = slots_now();
    let Some(slot) = slots.get(&rec.server).cloned() else {
        return ToolOutput::err(format!("MCP server `{}` stopped", rec.server));
    };
    let mut inner = lock_slot(&slot);
    let timeout = inner.def.tool_timeout;
    let Some(conn) = inner.conn.as_mut() else {
        let err = inner
            .last_error
            .clone()
            .unwrap_or_else(|| format!("MCP server `{}` stopped", rec.server));
        return ToolOutput::err(err);
    };
    match conn.call(&rec.raw, args, timeout) {
        Ok((text, true)) => ToolOutput::err(text),
        Ok((text, false)) => ToolOutput::ok(text),
        Err(err) => {
            inner.conn.take();
            inner.last_error = Some(clip_err(&err));
            ToolOutput::err(err)
        }
    }
}

fn one_schema(tool: &ToolRec) -> Value {
    let mut description = clip_chars(&tool.description, 1024);
    if tool.read_only {
        if !description.is_empty() {
            description.push(' ');
        }
        description.push_str("(server hint: read-only)");
    }
    let parameters = if tool.schema.is_object() {
        tool.schema.clone()
    } else {
        json!({"type": "object", "properties": {}})
    };
    json!({
        "type": "function",
        "name": tool.qualified,
        "description": description,
        "parameters": parameters,
    })
}

fn search_schema() -> Value {
    json!({
        "type": "function",
        "name": "search_tool",
        "description": "Search connected MCP tools by name or description. Returns matching server__tool names. Call use_tool with one name.",
        "parameters": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Case-insensitive substring."}
            },
            "required": ["query"],
            "additionalProperties": false
        }
    })
}

fn use_schema() -> Value {
    json!({
        "type": "function",
        "name": "use_tool",
        "description": "Call one MCP tool by its server__tool name. arguments is a JSON object or a JSON string.",
        "parameters": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "arguments": {"description": "JSON object, or a string of JSON."}
            },
            "required": ["name"],
            "additionalProperties": false
        }
    })
}

fn current_tools() -> Vec<ToolRec> {
    #[cfg(test)]
    if let Some(tools) = test_snapshot() {
        return tools;
    }
    let (slots, workspace) = load_slots();
    for slot in slots.values() {
        connect_if_needed(slot, &workspace);
    }
    requalify(&slots);
    flatten(&slots)
}

fn slots_now() -> BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>> {
    let (slots, _) = load_slots();
    slots
}

fn load_slots() -> (BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>>, PathBuf) {
    let mut held = lock_state();
    let path = config::config_file();
    let (gen, mtime) = fingerprint(&path);
    if !held.loaded || held.path != path || held.gen != gen || held.mtime != mtime {
        for slot in held.slots.values() {
            lock_slot(slot).conn.take();
        }
        held.slots.clear();
        for (name, def) in config::load_servers() {
            held.slots.insert(
                name.clone(),
                std::sync::Arc::new(Mutex::new(SlotInner::new(name, def))),
            );
        }
        held.loaded = true;
        held.path = path;
        held.gen = gen;
        held.mtime = mtime;
    }
    let workspace = if held.workspace.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir())
    } else {
        held.workspace.clone()
    };
    (held.slots.clone(), workspace)
}

fn fingerprint(path: &Path) -> (u64, Option<SystemTime>) {
    let gen = GEN.load(Ordering::Relaxed);
    let mtime = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok();
    (gen, mtime)
}

fn connect_if_needed(slot: &Mutex<SlotInner>, workspace: &Path) {
    let mut inner = lock_slot(slot);
    if !inner.def.enabled || inner.tried || inner.conn.is_some() {
        return;
    }
    inner.tried = true;
    let name = inner.name.clone();
    let def = inner.def.clone();
    match open_def(&name, &def, workspace) {
        Ok((conn, raw)) => {
            inner.raw = raw;
            inner.conn = Some(conn);
            inner.last_error = None;
        }
        Err(err) => {
            inner.raw.clear();
            inner.conn = None;
            inner.last_error = Some(clip_err(&err));
        }
    }
}

fn open_def(name: &str, def: &ServerDef, workspace: &Path) -> Result<(Conn, Vec<RawTool>), String> {
    match &def.transport {
        TransportDef::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let dir = stdio::workspace_or(cwd.clone(), workspace);
            let (conn, tools) =
                stdio::connect(name, command, args, env, &dir, def.startup_timeout)?;
            Ok((Conn::Stdio(conn), tools))
        }
        TransportDef::Http { url, sse } => {
            let (conn, tools) = http::connect(
                name,
                url,
                *sse,
                &def.headers,
                def.startup_timeout,
                def.tool_timeout,
            )?;
            Ok((Conn::Http(conn), tools))
        }
    }
}

fn requalify(slots: &BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>>) {
    let mut guards = Vec::new();
    let mut pairs = Vec::new();
    for slot in slots.values() {
        let guard = lock_slot(slot);
        for tool in &guard.raw {
            pairs.push((guard.name.clone(), tool.name.clone()));
        }
        guards.push(guard);
    }
    let qualified = names::qualify_all(&pairs);
    let mut map = std::collections::HashMap::new();
    for item in qualified {
        map.insert((item.server, item.tool), item.name);
    }
    for guard in &mut guards {
        guard.tools.clear();
        let server = guard.name.clone();
        let raw: Vec<(String, String, Value, bool)> = guard
            .raw
            .iter()
            .map(|tool| {
                (
                    tool.name.clone(),
                    clip_chars(&tool.description, 4000),
                    tool.input_schema.clone(),
                    tool.read_only,
                )
            })
            .collect();
        for (name, description, schema, read_only) in raw {
            let Some(qname) = map.get(&(server.clone(), name.clone())) else {
                continue;
            };
            guard.tools.push(ToolRec {
                server: server.clone(),
                raw: name,
                qualified: qname.clone(),
                description,
                schema,
                read_only,
            });
        }
    }
}

fn flatten(slots: &BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>>) -> Vec<ToolRec> {
    let mut out = Vec::new();
    for slot in slots.values() {
        out.extend(lock_slot(slot).tools.iter().cloned());
    }
    out
}

fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

fn clip_err(err: &str) -> String {
    clip_chars(err.trim(), 300)
}

#[cfg(test)]
thread_local! {
    static TEST_TOOLS: std::cell::RefCell<Option<Vec<ToolRec>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_snapshot() -> Option<Vec<ToolRec>> {
    TEST_TOOLS.with(|slot| slot.borrow().clone())
}

#[cfg(test)]
pub(crate) struct TestGuard {
    prev: Option<Vec<ToolRec>>,
}

#[cfg(test)]
impl Drop for TestGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        TEST_TOOLS.with(|slot| *slot.borrow_mut() = prev);
    }
}

#[cfg(test)]
pub(crate) fn install_test_tools(rows: &[(&str, &str, &str, bool)]) -> TestGuard {
    let pairs: Vec<(String, String)> = rows
        .iter()
        .map(|(server, tool, _, _)| ((*server).to_string(), (*tool).to_string()))
        .collect();
    let qualified = names::qualify_all(&pairs);
    let mut tools = Vec::new();
    for item in qualified {
        let read_only = rows
            .iter()
            .find(|(server, tool, _, _)| *server == item.server && *tool == item.tool)
            .map(|(_, _, _, flag)| *flag)
            .unwrap_or(false);
        let description = rows
            .iter()
            .find(|(server, tool, _, _)| *server == item.server && *tool == item.tool)
            .map(|(_, _, description, _)| (*description).to_string())
            .unwrap_or_default();
        tools.push(ToolRec {
            server: item.server,
            raw: item.tool,
            qualified: item.name,
            description,
            schema: json!({"type": "object", "properties": {}}),
            read_only,
        });
    }
    let prev = TEST_TOOLS.with(|slot| slot.borrow_mut().replace(tools));
    TestGuard { prev }
}

#[cfg(test)]
mod tests {
    use super::{
        import_documents, install_test_tools, read_mcp_text, schema_tools, search_output,
        try_dispatch,
    };
    use crate::gate::{decide, decide_with, mcp_policy_deny, Decision, Gate, PermMode};
    use crate::perm::ConfigGuard;
    use crate::perm::{parse_rule, Action, Policy};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    fn gate(mode: PermMode, readonly: bool, attended: bool) -> Gate {
        Gate {
            mode,
            readonly_session: readonly,
            attended,
            desktop: false,
        }
    }

    fn policy(rules: &[(&str, Action)]) -> Policy {
        Policy {
            rules: rules
                .iter()
                .map(|(text, action)| parse_rule(text, *action).unwrap())
                .collect(),
            grants: Vec::new(),
        }
    }

    fn temp_cfg(label: &str) -> (PathBuf, ConfigGuard) {
        let dir = std::env::temp_dir().join(format!("gh-mcp-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let guard = ConfigGuard::set(&dir);
        (dir, guard)
    }

    #[test]
    fn qualify_sanitizes_caps_and_dedupes() {
        let rows = super::names::qualify_all(&[
            ("a b".into(), "t".into()),
            ("a_b".into(), "t".into()),
            ("9box".into(), "echo".into()),
            ("box".into(), "1abc".into()),
            ("read".into(), "file".into()),
            ("x".repeat(80), "y".repeat(80)),
        ]);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert!(names
            .iter()
            .all(|name| name.len() <= super::names::NAME_MAX));
        assert!(names.iter().all(|name| name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')));
        assert!(names.iter().all(|name| name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')));
        assert_eq!(
            names.iter().filter(|name| name.contains("__")).count(),
            names.len()
        );
        assert!(names.contains(&"s9box__echo"));
        assert!(names.contains(&"box__1abc"));
        assert!(names.contains(&"read__file"));
        assert!(names
            .iter()
            .any(|name| *name == "a_b__t" || name.starts_with("a_b__t_")));
        assert_eq!(
            names.len(),
            names.iter().collect::<std::collections::HashSet<_>>().len()
        );
        assert!(!names
            .iter()
            .any(|name| *name == "search_tool" || *name == "use_tool" || *name == "read_file"));
    }

    #[test]
    fn import_copies_server_definitions_only() {
        let (dir, _guard) = temp_cfg("import");
        let doc = r#"{
            "token": "super-secret-token",
            "access_token": "nope",
            "mcpServers": {
                "echo": {"command":"echo","args":["hi"],"env":{"VISIBLE":"ok","API_TOKEN":"in-entry"},"access_token":"drop-me"},
                "web": {"url":"http://127.0.0.1:9/mcp","headers":{"Authorization":"Bearer in-entry"}}
            }
        }"#;
        let report = import_documents(&[doc.to_string()]).unwrap();
        assert_eq!(report.added, vec!["echo".to_string(), "web".to_string()]);
        let saved = std::fs::read_to_string(dir.join("mcp.json")).unwrap();
        assert!(saved.contains("echo"));
        assert!(saved.contains("VISIBLE"));
        assert!(saved.contains("API_TOKEN"));
        assert!(saved.contains("in-entry"));
        assert!(saved.contains("Authorization"));
        assert!(!saved.contains("super-secret-token"));
        assert!(!saved.contains("nope"));
        assert!(!saved.contains("drop-me"));
        let again = import_documents(&[doc.to_string()]).unwrap();
        assert!(again.added.is_empty());
        assert_eq!(again.skipped.len(), 2);
        let cred = dir.join(concat!("auth", ".json"));
        std::fs::write(&cred, "marker").unwrap();
        let err = read_mcp_text(&cred).unwrap_err();
        assert!(err.contains("refusing"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcptool_rules_deny_ask_allow_and_unattended_denies() {
        let _tools = install_test_tools(&[
            ("box", "echo", "Echo", false),
            ("box", "peek", "Peek", true),
        ]);
        let ws = Path::new(".");
        let deny = policy(&[
            ("MCPTool(box__echo)", Action::Deny),
            ("MCPTool(box__*)", Action::Allow),
        ]);
        let decision = decide_with(
            &gate(PermMode::Ask, false, true),
            "box__echo",
            "{}",
            false,
            None,
            ws,
            Some(&deny),
        );
        assert!(matches!(decision, Decision::Refuse(ref msg) if msg.contains("deny rule on mcp")));

        let ask = policy(&[("MCPTool(box__echo)", Action::Ask)]);
        assert_eq!(
            decide_with(
                &gate(PermMode::Always, false, true),
                "box__echo",
                "{}",
                true,
                None,
                ws,
                Some(&ask)
            ),
            Decision::Ask
        );
        let away = decide_with(
            &gate(PermMode::Ask, false, false),
            "box__echo",
            "{}",
            false,
            None,
            ws,
            Some(&ask),
        );
        assert!(
            matches!(away, Decision::Refuse(ref msg) if msg.contains("deny rule on mcp") && !msg.contains("deny rule on edit"))
        );

        let allow = policy(&[("MCPTool(box__echo)", Action::Allow)]);
        assert_eq!(
            decide_with(
                &gate(PermMode::Ask, false, false),
                "box__echo",
                "{}",
                false,
                None,
                ws,
                Some(&allow)
            ),
            Decision::Run
        );

        let open = decide_with(
            &gate(PermMode::Ask, false, true),
            "box__echo",
            "{}",
            false,
            None,
            ws,
            None,
        );
        assert_eq!(open, Decision::Ask);
        let unattended = decide_with(
            &gate(PermMode::Ask, false, false),
            "box__echo",
            "{}",
            false,
            None,
            ws,
            None,
        );
        assert!(
            matches!(unattended, Decision::Refuse(ref msg) if msg.contains("deny rule on mcp"))
        );
        assert_eq!(
            decide_with(
                &gate(PermMode::Always, false, false),
                "box__echo",
                "{}",
                false,
                None,
                ws,
                None
            ),
            Decision::Run
        );
        let plan = decide_with(
            &gate(PermMode::Ask, true, true),
            "box__echo",
            "{}",
            false,
            None,
            ws,
            Some(&allow),
        );
        assert!(matches!(plan, Decision::Refuse(ref msg) if msg.contains("read-only")));

        assert_eq!(
            decide(
                &gate(PermMode::Ask, false, false),
                "search_tool",
                false,
                None
            ),
            Decision::Run
        );
        let (dir, _guard) = temp_cfg("empty-use");
        let unknown = decide_with(
            &gate(PermMode::Ask, false, true),
            "use_tool",
            "{}",
            false,
            None,
            ws,
            None,
        );
        assert!(matches!(unknown, Decision::Refuse(ref msg) if msg.contains("unknown tool")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_only_hint_does_not_skip_the_gate() {
        let _tools = install_test_tools(&[("box", "peek", "Peek a file", true)]);
        let ws = Path::new(".");
        let asked = decide_with(
            &gate(PermMode::Ask, false, true),
            "box__peek",
            "{}",
            false,
            None,
            ws,
            None,
        );
        assert_eq!(asked, Decision::Ask);
        let allow = policy(&[("MCPTool(box__peek)", Action::Allow)]);
        assert_eq!(
            decide_with(
                &gate(PermMode::Ask, false, false),
                "box__peek",
                "{}",
                false,
                None,
                ws,
                Some(&allow)
            ),
            Decision::Run
        );
        let schemas = schema_tools();
        let hint = schemas
            .iter()
            .find(|tool| tool["name"] == "box__peek")
            .unwrap();
        assert!(hint["description"].as_str().unwrap().contains("read-only"));
    }

    #[test]
    fn deferred_discovery_switches_above_forty() {
        let mut rows = Vec::new();
        for n in 0..40 {
            rows.push((
                "box",
                format!("t{n}"),
                if n == 0 { "widget alpha" } else { "other" },
                false,
            ));
        }
        let owned: Vec<(&str, String, &str, bool)> = rows
            .iter()
            .map(|(server, tool, description, flag)| (*server, tool.clone(), *description, *flag))
            .collect();
        // install_test_tools wants &str. Leak the formatted names for the test process.
        let leaked: Vec<(&str, &str, &str, bool)> = owned
            .iter()
            .map(|(server, tool, description, flag)| (*server, tool.as_str(), *description, *flag))
            .collect();
        {
            let _guard = install_test_tools(&leaked);
            let schemas = schema_tools();
            assert!(schemas.iter().any(|tool| tool["name"] == "box__t0"));
            assert!(schemas
                .iter()
                .all(|tool| tool["name"] != "search_tool" && tool["name"] != "use_tool"));
            assert_eq!(schemas.len(), 40);
            let found = search_output(&json!({"query": "widget"}));
            assert!(found.text.contains("box__t0"));
            assert!(found.text.contains("widget alpha"));
        }
        let mut more = leaked.clone();
        let extra = "t40".to_string();
        more.push(("box", extra.as_str(), "widget beta", false));
        let _guard = install_test_tools(&more);
        let schemas = schema_tools();
        assert_eq!(schemas.len(), 2);
        assert!(schemas.iter().any(|tool| tool["name"] == "search_tool"));
        assert!(schemas.iter().any(|tool| tool["name"] == "use_tool"));
        assert!(schemas.iter().all(|tool| tool["name"] != "box__t0"));
        let found = search_output(&json!({"query": "beta"}));
        assert!(found.text.contains("box__t40"));
        let ws = Path::new(".");
        let missing = decide_with(
            &gate(PermMode::Ask, false, true),
            "use_tool",
            r#"{"name":"box__missing"}"#,
            false,
            None,
            ws,
            None,
        );
        assert!(
            matches!(missing, Decision::Refuse(ref msg) if msg.contains("unknown tool `box__missing`"))
        );
        let allow = policy(&[("MCPTool(box__t0)", Action::Allow)]);
        assert_eq!(
            decide_with(
                &gate(PermMode::Ask, false, false),
                "use_tool",
                r#"{"name":"box__t0","arguments":{"q":"1"}}"#,
                false,
                None,
                ws,
                Some(&allow),
            ),
            Decision::Run
        );
        let called = try_dispatch(
            "use_tool",
            &json!({"name": "box__t0", "arguments": {"q": "1"}}),
        )
        .unwrap();
        assert_eq!(called.text, "test-call box__t0");
        let denied = decide_with(
            &gate(PermMode::Ask, false, false),
            "use_tool",
            r#"{"name":"box__t40"}"#,
            false,
            None,
            ws,
            None,
        );
        assert!(matches!(denied, Decision::Refuse(ref msg) if msg.contains("deny rule on mcp")));
    }

    #[test]
    fn auto_review_fail_closed_for_mcp_and_keeps_explicit_ask() {
        let _tools = install_test_tools(&[("box", "echo", "Echo", false)]);
        let ws = std::env::temp_dir();
        let cancel = crate::CancelToken::new();
        let halt = || false;
        let ask_policy = policy(&[("MCPTool(box__echo)", Action::Ask)]);
        let quiet = Scripted {
            text: "garbage".into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let kept = crate::auto_review::review(&crate::auto_review::ReviewIn {
            client: &quiet,
            cancel: &cancel,
            halt: &halt,
            gate: &gate(PermMode::Auto, false, true),
            name: "box__echo",
            arguments: "{}",
            workspace: &ws,
            policy: Some(&ask_policy),
            latched_always: false,
            desk: None,
            history: &[],
            conversation_id: "s",
            base: Decision::Ask,
            timeout: std::time::Duration::from_secs(2),
        });
        assert_eq!(kept.decision, Decision::Ask);
        assert_eq!(quiet.calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        let judge = Scripted {
            text: "sure, allow it".into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let attended = crate::auto_review::review(&crate::auto_review::ReviewIn {
            client: &judge,
            cancel: &cancel,
            halt: &halt,
            gate: &gate(PermMode::Auto, false, true),
            name: "box__echo",
            arguments: "{}",
            workspace: &ws,
            policy: None,
            latched_always: false,
            desk: None,
            history: &[],
            conversation_id: "s",
            base: Decision::Ask,
            timeout: std::time::Duration::from_secs(2),
        });
        assert_eq!(attended.decision, Decision::Ask);
        assert_eq!(attended.ask_reason, crate::auto_review::FAIL_CLOSED_REASON);
        assert!(judge.calls.load(std::sync::atomic::Ordering::SeqCst) >= 1);

        let away_client = Scripted {
            text: "garbage".into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let away = crate::auto_review::review(&crate::auto_review::ReviewIn {
            client: &away_client,
            cancel: &cancel,
            halt: &halt,
            gate: &gate(PermMode::Auto, false, false),
            name: "box__echo",
            arguments: "{}",
            workspace: &ws,
            policy: None,
            latched_always: false,
            desk: None,
            history: &[],
            conversation_id: "s",
            base: Decision::Refuse(mcp_policy_deny("box__echo")),
            timeout: std::time::Duration::from_secs(2),
        });
        assert!(
            matches!(away.decision, Decision::Refuse(ref msg) if msg.contains("Denied by permission policy"))
        );
    }

    struct Scripted {
        text: String,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl crate::ModelClient for Scripted {
        fn stream(
            &self,
            req: &crate::ResponsesRequest,
            _cancel: &crate::CancelToken,
            _sink: &mut dyn FnMut(crate::StreamEvent),
        ) -> Result<crate::TurnOutput, crate::ClientError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let joined = format!("{req:?}");
            assert!(joined.contains(crate::auto_review::JUDGE_MARK) || req.input.iter().any(|item| {
                matches!(item, crate::InputItem::Message { content, .. } if content.iter().any(|part| {
                    matches!(part, crate::ContentPart::InputText(text) if text.contains(crate::auto_review::JUDGE_MARK))
                }))
            }));
            Ok(crate::TurnOutput {
                text: self.text.clone(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: crate::Usage::default(),
            })
        }
    }
}
