// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! `server__tool` names for the Responses tool charset.
//! Segments keep letters, digits, `_`, and `-`. The qualified name is capped
//! and de-duplicated in sorted order so the same catalog always maps the same way.

use std::collections::HashSet;

use crate::skills::clip_chars;

pub const NAME_MAX: usize = 64;
const DELIM: &str = "__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Qualified {
    pub server: String,
    pub tool: String,
    pub name: String,
}

pub fn qualify_all(pairs: &[(String, String)]) -> Vec<Qualified> {
    let mut rows = pairs.to_vec();
    rows.sort();
    rows.dedup();
    let mut used = HashSet::new();
    let mut out = Vec::with_capacity(rows.len());
    for (server, tool) in rows {
        let name = unique_name(&sanitize(&server, true), &sanitize(&tool, false), &mut used);
        out.push(Qualified { server, tool, name });
    }
    out
}

fn unique_name(server: &str, tool: &str, used: &mut HashSet<String>) -> String {
    let first = fit(server, tool, NAME_MAX);
    if claim(&first, used) {
        return first;
    }
    for n in 2..10_000u32 {
        let suffix = format!("_{n}");
        let room = NAME_MAX.saturating_sub(suffix.len()).max(DELIM.len() + 2);
        let base = fit(server, tool, room);
        let candidate = format!("{base}{suffix}");
        let candidate = clip_chars(&candidate, NAME_MAX);
        if claim(&candidate, used) {
            return candidate;
        }
    }
    let fallback = format!("mcp_{}", used.len());
    let fallback = clip_chars(&fallback, NAME_MAX);
    used.insert(fallback.clone());
    fallback
}

fn claim(name: &str, used: &mut HashSet<String>) -> bool {
    if name.is_empty() || reserved(name) || used.contains(name) {
        return false;
    }
    used.insert(name.to_string());
    true
}

fn fit(server: &str, tool: &str, max: usize) -> String {
    let mut server = server.to_string();
    let mut tool = tool.to_string();
    if server.is_empty() {
        server = "server".into();
    }
    if tool.is_empty() {
        tool = "tool".into();
    }
    while server.len() + DELIM.len() + tool.len() > max {
        if tool.len() > 1 {
            tool.pop();
        } else if server.len() > 1 {
            server.pop();
        } else {
            break;
        }
    }
    let mut name = format!("{server}{DELIM}{tool}");
    if name.len() > max {
        name = clip_chars(&name, max);
    }
    if !starts_ok(&name) {
        name = format!("m{name}");
        name = clip_chars(&name, max);
    }
    name
}

fn sanitize(raw: &str, server: bool) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        return if server {
            "server".into()
        } else {
            "tool".into()
        };
    }
    if server && !starts_ok(&out) {
        return format!("s{out}");
    }
    out
}

fn starts_ok(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

pub fn reserved(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "list_dir"
            | "grep"
            | "glob"
            | "get_command_or_subagent_output"
            | "scheduler_list"
            | "write"
            | "search_replace"
            | "run_terminal_command"
            | "kill_command_or_subagent"
            | "monitor"
            | "scheduler_create"
            | "scheduler_delete"
            | "connection_add"
            | "connection_disable"
            | "connection_delete"
            | "screenshot"
            | "click"
            | "move"
            | "drag"
            | "scroll"
            | "type"
            | "key"
            | "search_tool"
            | "skill"
            | "use_tool"
    )
}
