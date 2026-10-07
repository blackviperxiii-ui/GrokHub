//! Spike-5c: Grok's own tools for its skills, connections, and automations.
//!
//! One static table ([`SELF_TOOLS`]) names every tool and its class. Both
//! callers read it: the `grokhub --mcp-self` server that Grok Build sees as
//! `grokhub-self` (path A, cabin MCP dispatch), and the native Lab engine
//! (path E). Neither adds a gate: `harness::decide` classifies these tools
//! through `hard_class` / `hard_floor` like any other call, so delete and
//! credentials park a hard card even under Always, and a target under harness
//! policy, consent, egress, or Access is refused before any ledger is touched
//! ([`scope_guard`]).
//!
//! Rule 4: the agent never widens its own permissions. These tools reach
//! skills, connections, automations, and nothing else.

mod ops;
pub mod secrets;

use serde_json::{json, Value};

pub use ops::{run, skill_creates_today, SelfCtx};

/// The MCP server name Grok Build registers (tools show as `grokhub-self__<tool>`).
pub use grokhub_core::SELF_MCP_SERVER;

/// At most this many new skills a day from these tools (flagged for Jeremy).
pub const SKILL_CREATE_DAY_CAP: usize = 5;

/// How a self-manage tool is gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfClass {
    /// Lists only. Never parks.
    Read,
    /// Create, modify, disable, enable: logged with Undo, follows the GB pill.
    Soft,
    /// Hard class delete: always parks a card.
    Delete,
    /// Hard class credentials: a connection that needs a secret.
    Credentials,
}

impl SelfClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Soft => "soft",
            Self::Delete => "delete",
            Self::Credentials => "credentials",
        }
    }

    /// The harness hard class, when there is one.
    pub fn hard(self) -> Option<crate::harness::HardClass> {
        match self {
            Self::Read | Self::Soft => None,
            Self::Delete => Some(crate::harness::HardClass::Delete),
            Self::Credentials => Some(crate::harness::HardClass::Credentials),
        }
    }
}

/// Every self-manage tool and its base class. `connection_add` and
/// `connection_modify` become [`SelfClass::Credentials`] when they name a
/// secret ([`self_class`]).
pub const SELF_TOOLS: &[(&str, SelfClass)] = &[
    ("skill_list", SelfClass::Read),
    ("skill_create", SelfClass::Soft),
    ("skill_modify", SelfClass::Soft),
    ("skill_disable", SelfClass::Soft),
    ("skill_enable", SelfClass::Soft),
    ("skill_delete", SelfClass::Delete),
    ("connection_list", SelfClass::Read),
    ("connection_add", SelfClass::Soft),
    ("connection_modify", SelfClass::Soft),
    ("connection_disable", SelfClass::Soft),
    ("connection_remove", SelfClass::Delete),
    ("automation_list", SelfClass::Read),
    ("automation_create", SelfClass::Soft),
    ("automation_modify", SelfClass::Soft),
    ("automation_disable", SelfClass::Soft),
    ("automation_delete", SelfClass::Delete),
];

/// The table name for a native name (`skill_create`) or a Grok Build MCP name
/// (`grokhub-self__skill_create`). Another server's tool of the same leaf is
/// not ours.
pub fn self_tool(name: &str) -> Option<&'static str> {
    let leaf = match name.split_once("__") {
        Some((server, leaf)) if server == SELF_MCP_SERVER => leaf,
        Some(_) => return None,
        None => name,
    };
    SELF_TOOLS.iter().find(|(n, _)| *n == leaf).map(|(n, _)| *n)
}

/// Path E: run one call on the native engine. A credentials call asks for
/// each secret on the same masked card an MCP server's elicitation uses.
pub(crate) fn run_native(name: &str, args: &Value) -> crate::tools::ToolOutput {
    let dir = crate::perm::config_dir();
    let mut ctx = SelfCtx::new(&dir);
    if self_class(name, args) == Some(SelfClass::Credentials) {
        let conn = args.get("name").and_then(|n| n.as_str()).unwrap_or("");
        for var in secret_names(args) {
            let msg = secret_prompt(conn, &var);
            match crate::mcp::ask_secret(SELF_MCP_SERVER, &var, &msg) {
                Some(v) => ctx.secrets.push((var, v)),
                None => return crate::tools::ToolOutput::err(secret_missing(&var)),
            }
        }
    }
    run(&ctx, name, args)
}

/// What the masked card says.
pub fn secret_prompt(conn: &str, var: &str) -> String {
    format!("Grok is adding the connection {conn}. Paste {var} here; Grok never sees it.")
}

/// What the model reads when no secret came back.
pub fn secret_missing(var: &str) -> String {
    format!("The secret for {var} was not given. Nothing changed.")
}

/// A list tool: runs like the read-only tools.
pub fn is_read(name: &str) -> bool {
    self_tool(name).is_some_and(|t| SELF_TOOLS.iter().any(|(n, c)| *n == t && *c == SelfClass::Read))
}

/// Class for one call. A connection write that names a secret is credentials.
pub fn self_class(name: &str, args: &Value) -> Option<SelfClass> {
    let tool = self_tool(name)?;
    let base = SELF_TOOLS.iter().find(|(n, _)| *n == tool).map(|(_, c)| *c)?;
    if matches!(tool, "connection_add" | "connection_modify") && !secret_names(args).is_empty() {
        return Some(SelfClass::Credentials);
    }
    Some(base)
}

/// Env var names a connection asks for (`secrets: ["GITHUB_TOKEN"]`). Values
/// never travel in args: the user types them on the elicit card.
pub fn secret_names(args: &Value) -> Vec<String> {
    args.get("secrets")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// What a scope-guard refusal names. The words are matched on any string in
/// the args (a name, a path, a command, instructions), lowercased with `\` as
/// `/`. Harness policy, consent, egress, and Access are not self-manage.
const GUARDED: &[(&str, &str)] = &[
    ("consent.jsonl", "the consent ledger"),
    ("egress.jsonl", "the egress log"),
    ("egress.1.jsonl", "the egress log"),
    ("permission-rules.json", "harness policy"),
    ("permission-grants.json", "harness policy"),
    ("harness/", "harness policy"),
    ("hard.rs", "the hard-class list"),
    ("headless_deny_rules", "the hard-class list"),
    ("learned-key", "the learned-tier key"),
    ("app.json", "Access (app settings)"),
    ("secrets.json", "the secrets store"),
    ("changes/", "the change ledger"),
    ("access_mode", "Access"),
    ("desktop_control", "Access"),
];

/// Name parts a self-made skill, connection, or automation can't carry, and
/// whole names it can't take: they would read as the cabin's own policy.
const GUARDED_NAME_PARTS: &[&str] = &["consent", "egress", "harness"];
const GUARDED_NAMES: &[&str] = &["access", "access-mode", "permissions", "policy", "hard-class"];

/// A refused target, as a finding the cabin and the model can read.
pub fn scope_guard(name: &str, args: &Value) -> Option<crate::harness::Finding> {
    self_tool(name)?;
    let mut hit = None;
    walk_strings(args, &mut |key, s| {
        if hit.is_some() {
            return;
        }
        let low = s.replace('\\', "/").to_ascii_lowercase();
        if let Some((word, what)) = GUARDED.iter().find(|(w, _)| low.contains(w)) {
            hit = Some((key.to_string(), word.to_string(), *what));
            return;
        }
        if key == "name" {
            let slug = grokhub_core::skill_dir_name(s);
            let part = GUARDED_NAME_PARTS.iter().find(|w| slug.split('-').any(|p| p == **w));
            if let Some(w) = part.or_else(|| GUARDED_NAMES.iter().find(|w| slug == **w)) {
                hit = Some((key.to_string(), (*w).to_string(), "a cabin policy name"));
            }
        }
    });
    let (field, word, what) = hit?;
    Some(crate::harness::Finding {
        detector: SCOPE_GUARD.into(),
        tool: name.into(),
        detail: format!("{name} refused: `{field}` reaches {what} (`{word}`). Self-manage covers skills, connections, and automations only."),
        evidence: Vec::new(),
    })
}

/// Detector name on a scope-guard finding.
pub const SCOPE_GUARD: &str = "self_manage_scope";

fn walk_strings(v: &Value, f: &mut dyn FnMut(&str, &str)) {
    fn go(key: &str, v: &Value, f: &mut dyn FnMut(&str, &str)) {
        match v {
            Value::String(s) => f(key, s),
            Value::Array(a) => a.iter().for_each(|x| go(key, x, f)),
            Value::Object(m) => m.iter().for_each(|(k, x)| go(k, x, f)),
            _ => {}
        }
    }
    go("", v, f);
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

fn tool(name: &str, description: &str, input: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input })
}

/// MCP `tools/list` entries, one per [`SELF_TOOLS`] row.
pub fn mcp_tools() -> Vec<Value> {
    let reason = json!({ "type": "string", "description": "One short line on why. Shown on the Work-tree row." });
    let name = json!({ "type": "string" });
    let named = |extra: Value| {
        let mut props = json!({ "name": name, "reason": reason });
        if let (Some(p), Some(e)) = (props.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        props
    };
    let skill_body = json!({
        "description": { "type": "string" },
        "trigger": { "type": "string" },
        "instructions": { "type": "string", "description": "The steps. Never a secret." },
    });
    let conn_body = json!({
        "command": { "type": "string", "description": "stdio server command" },
        "args": { "type": "array", "items": { "type": "string" } },
        "url": { "type": "string", "description": "http server URL (instead of command)" },
        "secrets": { "type": "array", "items": { "type": "string" }, "description": "Env var names the server needs (GITHUB_TOKEN). Never the value: the user types it on a card, and you see %secret%." },
    });
    let auto_body = json!({
        "instructions": { "type": "string" },
        "schedule": { "type": "string", "description": "daily, weekdays, or a weekday name" },
        "time": { "type": "string", "description": "HH:MM" },
    });
    vec![
        tool("skill_list", "List GrokHub skills.", obj(json!({}), &[])),
        tool("skill_create", "Create a GrokHub skill. Logged with Undo. At most 5 new skills a day.", obj(named(skill_body.clone()), &["name", "instructions", "reason"])),
        tool("skill_modify", "Change a GrokHub skill. Logged with Undo.", obj(named(skill_body), &["name", "reason"])),
        tool("skill_disable", "Turn a GrokHub skill off. Logged with Undo.", obj(named(json!({})), &["name", "reason"])),
        tool("skill_enable", "Turn a disabled GrokHub skill back on. Logged with Undo.", obj(named(json!({})), &["name", "reason"])),
        tool("skill_delete", "Delete a GrokHub skill. Needs the user's click.", obj(named(json!({})), &["name", "reason"])),
        tool("connection_list", "List connections (MCP servers) in the cabin Grok home.", obj(json!({}), &[])),
        tool("connection_add", "Add a connection (MCP server) to the cabin Grok home. Logged with Undo. Naming a secret needs the user's click.", obj(named(conn_body.clone()), &["name", "reason"])),
        tool("connection_modify", "Change a connection. Logged with Undo. Naming a secret needs the user's click.", obj(named(conn_body), &["name", "reason"])),
        tool("connection_disable", "Turn a connection off. Logged with Undo.", obj(named(json!({})), &["name", "reason"])),
        tool("connection_remove", "Remove a connection. Needs the user's click.", obj(named(json!({})), &["name", "reason"])),
        tool("automation_list", "List GrokHub automations.", obj(json!({}), &[])),
        tool("automation_create", "Create a GrokHub automation. Logged with Undo. At most 2 a week until the user accepts one.", obj(named(auto_body.clone()), &["name", "instructions", "reason"])),
        tool("automation_modify", "Change a GrokHub automation. Logged with Undo.", obj(named(auto_body), &["name", "reason"])),
        tool("automation_disable", "Turn a GrokHub automation off. Logged with Undo.", obj(named(json!({})), &["name", "reason"])),
        tool("automation_delete", "Delete a GrokHub automation. Needs the user's click.", obj(named(json!({})), &["name", "reason"])),
    ]
}

/// Native Responses API function schemas (path E), same rows as [`mcp_tools`].
pub fn native_schemas() -> Vec<Value> {
    mcp_tools()
        .into_iter()
        .map(|t| {
            json!({
                "type": "function",
                "name": t["name"],
                "description": t["description"],
                "parameters": t["inputSchema"],
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
