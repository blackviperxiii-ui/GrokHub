//! Read-only import of chats from the Grok Build CLI days: the session
//! folders and markdown dumps under `~/.grok`. Nothing here runs the CLI or
//! reads its credentials.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

/// The user's old Grok Build folder (`~/.grok`), read only for chat history.
pub fn grok_home() -> Option<PathBuf> {
    Some(crate::user_home()?.join(".grok"))
}

pub fn session_id_in_home(home: &Path, id: &str) -> bool {
    let id = id.trim();
    if id.is_empty() || !looks_like_session_id(id) {
        return false;
    }
    dir_has_named_session(&home.join("sessions"), id, 0)
}

fn dir_has_named_session(dir: &Path, id: &str, depth: u8) -> bool {
    if depth > 6 {
        return false;
    }
    if dir.join(id).is_dir() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() && dir_has_named_session(&path, id, depth + 1) {
            return true;
        }
    }
    false
}

fn looks_like_session_id(id: &str) -> bool {
    if id.len() < 8 {
        return false;
    }
    let dashes = id.bytes().filter(|b| *b == b'-').count();
    if dashes >= 2 && id.len() >= 16 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return true;
    }
    if id.starts_with("sess_") {
        return id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    }
    false
}

pub fn is_placeholder_session_title(s: &str) -> bool {
    let t = s.trim();
    t.is_empty()
        || t.eq_ignore_ascii_case("(no summary)")
        || t.eq_ignore_ascii_case("(no label)")
        || t.eq_ignore_ascii_case("session")
        || t.eq_ignore_ascii_case("plan")
}

/// Selecting Plan switches the session mode. The thread title is unchanged.
pub fn title_after_selecting_plan(title: &str) -> String {
    title.to_string()
}

/// History row text after Plan. Keep the label already on screen.
pub fn history_label_after_plan(shown: &str, grok_title: &str) -> String {
    let shown = shown.trim();
    if shown.is_empty() {
        grok_title.to_string()
    } else {
        shown.to_string()
    }
}

/// Cabin History label: Grok Build session name unless the user renamed the tab.
pub fn preferred_history_title(
    cabin_title: &str,
    title_locked: bool,
    grok_title: Option<&str>,
    grok_id: Option<&str>,
) -> String {
    if !title_locked {
        if let Some(title) = grok_title.map(str::trim).filter(|t| !t.is_empty()) {
            let id = grok_id.unwrap_or("").trim();
            if !is_placeholder_session_title(title) && title != id {
                return title.to_string();
            }
        }
    }
    cabin_title.to_string()
}

/// First real `<user_query>` line from a Grok Build `chat_history.jsonl`.
pub fn session_title_from_chat_history(text: &str) -> Option<String> {
    if let Some(t) = title_from_jsonl_records(text) {
        return Some(t);
    }
    title_from_user_query_blocks(text)
}

fn title_from_jsonl_records(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        for blob in json_user_blobs(&v) {
            if let Some(t) = title_from_user_query_blocks(&blob) {
                return Some(t);
            }
        }
    }
    None
}

fn json_user_blobs(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(c) = v.get("content") {
        collect_json_text(c, &mut out);
    }
    out
}

fn collect_json_text(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => {
            for x in a {
                collect_json_text(x, out);
            }
        }
        Value::Object(m) => {
            if let Some(Value::String(s)) = m.get("text") {
                out.push(s.clone());
            } else if let Some(c) = m.get("content") {
                collect_json_text(c, out);
            }
        }
        _ => {}
    }
}

fn title_from_user_query_blocks(text: &str) -> Option<String> {
    for chunk in text.split("<user_query>").skip(1) {
        if !chunk.contains("</user_query>") {
            continue;
        }
        let Some(inner) = chunk.split("</user_query>").next() else {
            continue;
        };
        if inner.contains("<work_policy>")
            || inner.contains("<user_info>")
            || inner.contains("<system-reminder>")
        {
            continue;
        }
        let normalized = inner.replace("\\n", "\n");
        let Some(line) = normalized.lines().map(str::trim).find(|l| !l.is_empty()) else {
            continue;
        };
        if line.eq_ignore_ascii_case("tag.") {
            continue;
        }
        return Some(line.chars().take(80).collect());
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokSession {
    pub id: String,
    pub title: String,
    pub path: Option<PathBuf>,
    /// Worktree `grok sessions list` used when this row came from the CLI.
    pub cwd: Option<PathBuf>,
    /// True when the session files live under cabin GROK_HOME (safe to `--resume`).
    pub cabin: bool,
}

/// Transcript turns from a Grok session markdown dump.
pub fn parse_session_markdown(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut role = String::new();
    let mut body = String::new();
    let flush = |role: &mut String, body: &mut String, out: &mut Vec<(String, String)>| {
        let t = body.trim();
        if !role.is_empty() && !t.is_empty() {
            out.push((role.clone(), t.to_string()));
        }
        role.clear();
        body.clear();
    };
    for line in text.lines() {
        let t = line.trim();
        let lower = t.to_ascii_lowercase();
        let heading = t.trim_start_matches('#').trim();
        let heading_l = heading.to_ascii_lowercase();
        let next = if heading_l == "user" || heading_l == "human" || lower.starts_with("user:") || lower.starts_with("**user**")
        {
            Some("user")
        } else if heading_l == "assistant"
            || heading_l == "grok"
            || lower.starts_with("assistant:")
            || lower.starts_with("grok:")
            || lower.starts_with("**assistant**")
            || lower.starts_with("**grok**")
        {
            Some("assistant")
        } else {
            None
        };
        if let Some(r) = next {
            flush(&mut role, &mut body, &mut out);
            role = r.into();
            if let Some((_, rest)) = t.split_once(':') {
                let rest = rest.trim().trim_matches('*').trim();
                if !rest.is_empty() && !rest.eq_ignore_ascii_case("user") && !rest.eq_ignore_ascii_case("assistant") {
                    body.push_str(rest);
                    body.push('\n');
                }
            }
            continue;
        }
        if t.starts_with('#') {
            continue;
        }
        if !role.is_empty() {
            body.push_str(line);
            body.push('\n');
        }
    }
    flush(&mut role, &mut body, &mut out);
    out
}

fn session_title_from_markdown(text: &str, fallback: &str) -> String {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("# ") {
            let name = rest.trim();
            if !name.is_empty()
                && !name.eq_ignore_ascii_case("user")
                && !name.eq_ignore_ascii_case("assistant")
                && !name.eq_ignore_ascii_case("session")
            {
                return name.chars().take(80).collect();
            }
        }
    }
    fallback.to_string()
}

/// On-disk Grok Build sessions (`~/.grok/sessions/<id>/`, `~/.grok/memory/*/sessions/*.md`).
pub fn discover_session_files() -> Vec<GrokSession> {
    let Some(home) = grok_home() else {
        return Vec::new();
    };
    discover_session_files_in(&home)
}

/// Read Grok Build 1.0.12 `signals.json` (context window + used tokens).
pub fn load_session_signals(home: &Path, session_id: &str) -> Option<crate::wire::GrokUsage> {
    let id = session_id.trim();
    if id.is_empty() {
        return None;
    }
    find_signals_json(&home.join("sessions"), id, 0)
}

fn find_signals_json(dir: &Path, id: &str, depth: u8) -> Option<crate::wire::GrokUsage> {
    if depth > 4 {
        return None;
    }
    let direct = dir.join(id).join("signals.json");
    if direct.is_file() {
        let raw = std::fs::read_to_string(&direct).ok()?;
        return crate::wire::parse_signals_json(&raw);
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(u) = find_signals_json(&p, id, depth + 1) {
                return Some(u);
            }
        }
    }
    None
}

pub fn discover_session_files_in(home: &Path) -> Vec<GrokSession> {
    let mut out = Vec::new();
    let roots = [
        home.join("memory"),
        home.join("sessions"),
        home.join("worktrees"),
    ];
    for root in roots {
        walk_session_md(&root, 0, &mut out);
    }
    walk_session_dirs(&home.join("sessions"), 0, &mut out);
    out
}

fn walk_session_dirs(dir: &Path, depth: u8, out: &mut Vec<GrokSession>) {
    if depth > 6 || out.len() >= 80 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if looks_like_session_id(name) {
            if out.iter().any(|s| s.id == name) {
                continue;
            }
            let title = session_title_from_dir(&path, name);
            out.push(GrokSession {
                id: name.to_string(),
                title,
                path: Some(path.join("chat_history.jsonl")),
                cwd: None,
                cabin: false,
            });
            continue;
        }
        walk_session_dirs(&path, depth + 1, out);
    }
}

fn session_title_from_dir(dir: &Path, fallback: &str) -> String {
    let raw = read_file_capped(&dir.join("summary.json"), SESSION_MD_CAP);
    if let Ok(v) = serde_json::from_str::<Value>(&raw) {
        if let Some(t) = v
            .get("session_summary")
            .or_else(|| v.get("title"))
            .or_else(|| v.pointer("/info/title"))
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|t| !is_placeholder_session_title(t))
        {
            return t.chars().take(80).collect();
        }
    }
    if let Some(t) = session_title_from_jsonl_path(&dir.join("chat_history.jsonl")) {
        return t;
    }
    fallback.to_string()
}

fn session_title_from_jsonl_path(path: &Path) -> Option<String> {
    let f = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(f);
    let mut seen = 0usize;
    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        seen = seen.saturating_add(line.len());
        if let Some(t) = session_title_from_chat_history(&line) {
            return Some(t);
        }
        if seen > 256 * 1024 {
            break;
        }
    }
    None
}

fn walk_session_md(dir: &Path, depth: u8, out: &mut Vec<GrokSession>) {
    if depth > 6 || out.len() >= 80 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            walk_session_md(&path, depth + 1, out);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if stem.eq_ignore_ascii_case("readme")
            || stem.eq_ignore_ascii_case("skill")
            || stem.is_empty()
        {
            continue;
        }
        let id = if looks_like_session_id(&stem) {
            stem.clone()
        } else if let Some(parent) = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
        {
            if looks_like_session_id(parent) {
                parent.to_string()
            } else {
                continue;
            }
        } else {
            continue;
        };
        if out.iter().any(|s| s.id == id && s.path.is_some()) {
            continue;
        }
        let text = read_file_capped(&path, SESSION_MD_CAP);
        out.push(GrokSession {
            id: id.clone(),
            title: session_title_from_markdown(&text, &id),
            path: Some(path),
            cwd: None,
            cabin: false,
        });
    }
}

const SESSION_MD_CAP: usize = 8 * 1024;

fn read_file_capped(path: &Path, cap: usize) -> String {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut buf = vec![0u8; cap];
    let n = match f.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return String::new(),
    };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}
