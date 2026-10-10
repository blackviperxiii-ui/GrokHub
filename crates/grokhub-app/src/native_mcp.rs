//! The cabin's own MCP servers (Settings → Connectors). Import, status,
//! sign-in and Disconnect run off the UI thread.

use std::path::{Component, Path};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Map, Value};

struct PaintState {
    rows: Vec<grokhub_agent::mcp::DoctorRow>,
    note: String,
    busy: bool,
    seeded: bool,
}

fn paint_state() -> &'static Mutex<PaintState> {
    static STATE: OnceLock<Mutex<PaintState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(PaintState {
            rows: Vec::new(),
            note: String::new(),
            busy: false,
            seeded: false,
        })
    })
}

/// The last checked MCP servers for `/health`: how many are on, and each
/// one in error by name. Reads the Settings cache; pings nothing.
pub fn health_rows() -> (usize, Vec<String>) {
    let held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
    servers_health(&held.rows)
}

fn servers_health(rows: &[grokhub_agent::mcp::DoctorRow]) -> (usize, Vec<String>) {
    let on = rows.iter().filter(|r| r.status != "disabled").count();
    let failed = rows
        .iter()
        .filter(|r| r.status == "error")
        .map(|r| match r.last_error.trim() {
            "" => format!("MCP {}: error", r.name),
            why => format!("MCP {}: error, {why}", r.name),
        })
        .collect();
    (on, failed)
}

/// Work on the cabin's own MCP servers, run off the UI thread.
pub enum Job {
    Import,
    Doctor,
    Restart(String),
    SignIn(String),
    SignOut(String),
    /// Settings → Connectors Disconnect, after the confirm sheet.
    Disconnect(String),
    /// Marketplace Install: write `entry` under `name` (`label` is the
    /// catalog name for the note), then open its browser sign-in when
    /// `sign_in` is set.
    Install {
        name: String,
        label: String,
        entry: serde_json::Value,
        sign_in: bool,
    },
    /// Seal a key the user typed, after the confirm sheet approved it.
    SaveKey { name: String, key: String },
}

/// What Settings → Connectors paints: the last rows, the last job's note,
/// and whether a job is still running.
pub struct Snapshot {
    pub rows: Vec<grokhub_agent::mcp::DoctorRow>,
    pub note: String,
    pub busy: bool,
}

/// The cached rows (read from the config the first time; nothing is pinged).
pub fn snapshot() -> Snapshot {
    let mut held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
    if !held.seeded {
        held.rows = grokhub_agent::mcp::configured();
        held.seeded = true;
    }
    Snapshot {
        rows: held.rows.clone(),
        note: held.note.clone(),
        busy: held.busy,
    }
}

/// Run `job` on a thread. One at a time: a second job while one runs is dropped.
pub fn spawn(job: Job) {
    {
        let mut held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
        if held.busy {
            return;
        }
        held.busy = true;
        match &job {
            Job::SignIn(name) => held.note = format!("Finish signing in to {name} in your browser"),
            Job::Install { label, .. } => held.note = format!("Installing {label}"),
            _ => {}
        }
    }
    std::thread::spawn(move || {
        let (rows, note) = match job {
            Job::Import => {
                let note = match import_from_cabin() {
                    Ok(text) => text,
                    Err(err) => err,
                };
                (grokhub_agent::mcp::configured(), note)
            }
            Job::Doctor => (grokhub_agent::mcp::doctor(), String::new()),
            Job::Restart(name) => {
                let note = match grokhub_agent::mcp::restart(&name) {
                    Ok(()) => format!("Reconnected {name}"),
                    Err(err) => err,
                };
                (grokhub_agent::mcp::doctor(), note)
            }
            Job::SignIn(name) => {
                let note = match grokhub_agent::mcp::sign_in(&name, &|url| crate::oauth::open_browser(url)) {
                    Ok(line) => line,
                    Err(err) => err,
                };
                (grokhub_agent::mcp::doctor(), note)
            }
            Job::SignOut(name) => {
                let note = grokhub_agent::mcp::sign_out(&name);
                (grokhub_agent::mcp::configured(), note)
            }
            Job::Disconnect(name) => {
                let note = match grokhub_agent::mcp::disconnect(&name) {
                    Ok(line) => line,
                    Err(err) => err,
                };
                (grokhub_agent::mcp::configured(), note)
            }
            Job::Install { name, label, entry, sign_in } => {
                let note = match grokhub_agent::mcp::install(&name, &entry) {
                    Ok(done) if sign_in => {
                        set_note(&format!("{}. Finish signing in to {label} in your browser", install_line(&label, &done)));
                        match grokhub_agent::mcp::sign_in(&name, &|url| crate::oauth::open_browser(url)) {
                            Ok(line) => format!("{}. {line}", install_line(&label, &done)),
                            Err(err) => format!("{}. {err}", install_line(&label, &done)),
                        }
                    }
                    Ok(done) => install_line(&label, &done),
                    Err(err) => err,
                };
                (grokhub_agent::mcp::configured(), note)
            }
            Job::SaveKey { name, key } => {
                let note = match grokhub_agent::mcp::save_key(&name, &key) {
                    Ok(line) => line,
                    Err(err) => err,
                };
                (grokhub_agent::mcp::configured(), note)
            }
        };
        let mut held = paint_state().lock().unwrap_or_else(|err| err.into_inner());
        held.rows = rows;
        held.note = note;
        held.busy = false;
        held.seeded = true;
    });
}

fn set_note(note: &str) {
    paint_state().lock().unwrap_or_else(|err| err.into_inner()).note = note.to_string();
}

/// "Installed Playwright" or "Playwright was already installed".
pub fn install_line(label: &str, done: &grokhub_agent::mcp::Installed) -> String {
    match done {
        grokhub_agent::mcp::Installed::Added => format!("Installed {label}"),
        grokhub_agent::mcp::Installed::AlreadyThere => format!("{label} was already installed"),
    }
}

pub fn import_from_cabin() -> Result<String, String> {
    import_from_dir(&crate::config::cabin_grok_home())
}

/// Copy `mcpServers` from JSON files and `[mcp_servers.*]` tables in the cabin
/// settings file. A path that contains a `.grok` component is refused.
pub fn import_from_dir(home: &Path) -> Result<String, String> {
    if home
        .components()
        .any(|part| matches!(part, Component::Normal(name) if name == ".grok"))
    {
        return Err("refusing a CLI home".into());
    }
    let mut docs = Vec::new();
    for name in ["mcp.json", ".mcp.json"] {
        let path = home.join(name);
        if path.is_file() {
            docs.push(grokhub_agent::mcp::read_mcp_text(&path)?);
        }
    }
    let settings = home.join("config.toml");
    if settings.is_file() {
        let text = std::fs::read_to_string(&settings).map_err(|err| err.to_string())?;
        let value = mcp_servers_from_settings(&text);
        let count = value
            .get("mcpServers")
            .and_then(|item| item.as_object())
            .map(|map| map.len())
            .unwrap_or(0);
        if count > 0 {
            docs.push(value.to_string());
        }
    }
    Ok(grokhub_agent::mcp::import_documents(&docs)?.summary())
}

fn mcp_servers_from_settings(text: &str) -> Value {
    let mut servers = Map::new();
    let mut current: Option<String> = None;
    let mut sub: Option<String> = None;
    for raw in text.lines() {
        let line = strip_comment(raw);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let inside = line[1..line.len() - 1].trim().trim_matches('"');
            match table_target(inside) {
                Some((name, child)) => {
                    servers.entry(name.clone()).or_insert_with(|| json!({}));
                    current = Some(name);
                    sub = child;
                }
                None => {
                    current = None;
                    sub = None;
                }
            }
            continue;
        }
        let Some(server) = current.clone() else {
            continue;
        };
        let Some((key, raw_value)) = split_eq(line) else {
            continue;
        };
        let parsed = parse_toml_value(raw_value.trim());
        let Some(obj) = servers
            .get_mut(&server)
            .and_then(|item| item.as_object_mut())
        else {
            continue;
        };
        if let Some(child) = &sub {
            let entry = obj.entry(child.clone()).or_insert_with(|| json!({}));
            if let Some(map) = entry.as_object_mut() {
                map.insert(key, parsed);
            }
        } else {
            obj.insert(key, parsed);
        }
    }
    json!({ "mcpServers": servers })
}

fn table_target(inside: &str) -> Option<(String, Option<String>)> {
    let parts: Vec<&str> = inside
        .split('.')
        .map(|part| part.trim().trim_matches('"'))
        .filter(|part| !part.is_empty())
        .collect();
    if parts.first().copied() != Some("mcp_servers") || parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let name = parts[1].to_string();
    if name.is_empty() {
        return None;
    }
    let child = parts.get(2).map(|part| (*part).to_string());
    Some((name, child))
}

fn strip_comment(line: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    for ch in line.chars() {
        if let Some(mark) = quote {
            out.push(ch);
            if ch == mark {
                quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            out.push(ch);
            continue;
        }
        if ch == '#' {
            break;
        }
        out.push(ch);
    }
    out
}

fn split_eq(line: &str) -> Option<(String, &str)> {
    let mut quote: Option<char> = None;
    for (idx, ch) in line.char_indices() {
        if let Some(mark) = quote {
            if ch == mark {
                quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            continue;
        }
        if ch == '=' {
            let key = line[..idx].trim().trim_matches('"').to_string();
            if key.is_empty() {
                return None;
            }
            return Some((key, &line[idx + 1..]));
        }
    }
    None
}

fn parse_toml_value(text: &str) -> Value {
    let text = text.trim().trim_end_matches(',');
    if text == "true" {
        return json!(true);
    }
    if text == "false" {
        return json!(false);
    }
    if let Some(rest) = text.strip_prefix('"') {
        return json!(unquote(rest, '"'));
    }
    if let Some(rest) = text.strip_prefix('\'') {
        return json!(unquote(rest, '\''));
    }
    if text.starts_with('[') && text.ends_with(']') {
        let inner = &text[1..text.len() - 1];
        let items = split_top(inner, ',')
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .map(|part| parse_toml_value(part.trim()))
            .collect::<Vec<_>>();
        return json!(items);
    }
    if text.starts_with('{') && text.ends_with('}') {
        let inner = &text[1..text.len() - 1];
        let mut map = Map::new();
        for part in split_top(inner, ',') {
            if let Some((key, value)) = split_eq(part.trim()) {
                map.insert(key, parse_toml_value(value.trim()));
            }
        }
        return Value::Object(map);
    }
    if let Ok(number) = text.parse::<u64>() {
        return json!(number);
    }
    json!(text)
}

fn unquote(text: &str, mark: char) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == mark {
            break;
        }
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => {}
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn split_top(text: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (idx, ch) in text.char_indices() {
        if let Some(mark) = quote {
            if ch == mark {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            ch if ch == sep && depth == 0 => {
                out.push(&text[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_counts_servers_that_are_on_and_names_each_one_in_error() {
        use grokhub_agent::mcp::{DoctorRow, SignIn};
        let row = |name: &str, status: &str, err: &str| DoctorRow {
            name: name.into(),
            status: status.into(),
            tool_count: 0,
            last_error: err.into(),
            detail: String::new(),
            sign_in: SignIn::NotOffered,
        };
        let rows = vec![
            row("github", "error", "timed out after 5s"),
            row("linear", "connected", ""),
            row("old", "disabled", ""),
            row("files", "error", " "),
        ];
        assert_eq!(
            servers_health(&rows),
            (3, vec!["MCP github: error, timed out after 5s".to_string(), "MCP files: error".to_string()])
        );
        assert_eq!(servers_health(&[]), (0, Vec::new()));
    }

    #[test]
    fn native_mcp_import_copies_server_definitions_only() {
        let cfg = std::env::temp_dir().join(format!("gh-native-mcp-cfg-{}", std::process::id()));
        let home = std::env::temp_dir().join(format!("gh-native-mcp-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cfg);
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let _guard = grokhub_agent::perm::ConfigGuard::set(&cfg);
        std::fs::write(
            home.join("config.toml"),
            r#"
api_key = "top-level-marker"
[mcp_servers.echo]
command = "echo"
args = ["hi"]
env = { VISIBLE = "ok", API_TOKEN = "in-entry" }
access_token = "drop-me"
[mcp_servers.web]
url = "http://127.0.0.1:9/mcp"
type = "http"
[mcp_servers.web.headers]
Authorization = "Bearer in-entry"
"#,
        )
        .unwrap();
        std::fs::write(home.join(concat!("auth", ".json")), "auth-file-marker").unwrap();
        std::fs::write(
            home.join("mcp.json"),
            r#"{"token":"json-token-marker","mcpServers":{"fromjson":{"command":"true"}}}"#,
        )
        .unwrap();
        let summary = import_from_dir(&home).unwrap();
        assert!(summary.contains("Imported"), "{summary}");
        let saved = std::fs::read_to_string(cfg.join("mcp.json")).unwrap();
        assert!(saved.contains("echo"), "{saved}");
        assert!(saved.contains("fromjson"), "{saved}");
        assert!(saved.contains("VISIBLE"), "{saved}");
        assert!(saved.contains("API_TOKEN"), "{saved}");
        assert!(saved.contains("in-entry"), "{saved}");
        assert!(saved.contains("Authorization"), "{saved}");
        assert!(!saved.contains("top-level-marker"), "{saved}");
        assert!(!saved.contains("drop-me"), "{saved}");
        assert!(!saved.contains("auth-file-marker"), "{saved}");
        assert!(!saved.contains("json-token-marker"), "{saved}");
        let cli = home.join(".grok");
        std::fs::create_dir_all(&cli).unwrap();
        std::fs::write(
            cli.join("mcp.json"),
            r#"{"mcpServers":{"secretcli":{"command":"nope"}}}"#,
        )
        .unwrap();
        assert!(import_from_dir(&cli).is_err());
        let saved = std::fs::read_to_string(cfg.join("mcp.json")).unwrap();
        assert!(!saved.contains("secretcli"), "{saved}");
        let _ = std::fs::remove_dir_all(&cfg);
        let _ = std::fs::remove_dir_all(&home);
    }
}
