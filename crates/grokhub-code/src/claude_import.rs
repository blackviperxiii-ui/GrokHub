//! Read-only import of Claude Code session context for the Code tab.
//!
//! Reads local transcripts under `~/.claude/projects/` (or a user-picked
//! `.jsonl` file). Never writes under `.claude`, and never spawns `claude` or
//! `grok`. Secrets and oversized base64 blobs are stripped before the context
//! is handed to a Loadout Code thread.

use grokhub_core::claude_usage::claude_dir;
use grokhub_core::redact::redact_for_share;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// How many recent conversational turns land in the import.
pub const MAX_TURNS: usize = 24;
/// Characters kept per turn after sanitize.
pub const MAX_TURN_CHARS: usize = 4_000;
/// Cap on the whole prefill block.
pub const MAX_CONTEXT_CHARS: usize = 24_000;
/// Touched file paths kept.
pub const MAX_FILES: usize = 40;
/// Rows the picker lists.
pub const MAX_SESSIONS: usize = 40;
/// Longest line scanned while listing (skips huge tool dumps cheaply).
const LIST_LINE_CAP: usize = 64 * 1024;
/// Base64-looking runs at or above this length are replaced.
pub const BASE64_BLOB_MIN: usize = 200;
/// Bytes read from a session file when importing (hard ceiling).
const IMPORT_FILE_CAP: u64 = 8 * 1024 * 1024;

const MISSING: &str = "That Claude session file wasn't found.";
const NOT_JSONL: &str = "Pick a Claude Code session .jsonl file.";
const CORRUPT: &str = "That Claude session file couldn't be read.";
const EMPTY: &str = "That Claude session has no turns to import.";
const OUTSIDE_PROJECTS: &str = "Claude sessions can only be imported from ~/.claude/projects/.";

/// One recent session for the Import Claude session picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSessionRow {
    pub id: String,
    pub title: String,
    pub cwd: Option<String>,
    pub path: String,
    pub turn_count: u32,
    pub updated_ms: u64,
    pub matches_workspace: bool,
}

/// One sanitized turn ready for the Code thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeImportTurn {
    pub role: String,
    pub text: String,
}

/// Parsed, sanitized session context for a Loadout Code thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeImport {
    pub title: String,
    pub cwd: Option<String>,
    pub turns: Vec<ClaudeImportTurn>,
    pub files: Vec<String>,
    /// Prefill text for a new or active Code thread.
    pub context: String,
}

/// Claude Code's folder: `CLAUDE_CONFIG_DIR`, or `~/.claude`.
pub fn default_claude_dir() -> Option<PathBuf> {
    let home = grokhub_core::user_home()?;
    let env = std::env::var("CLAUDE_CONFIG_DIR").ok();
    Some(claude_dir(env.as_deref(), &home))
}

/// Encode a workspace path the way Claude Code names project folders:
/// every non-alphanumeric character becomes `-` (same as
/// `cwd.replace(/[^a-zA-Z0-9]/g, "-")`).
pub fn encode_project_dir(cwd: &str) -> String {
    let trimmed = cwd.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

/// Whether a session belongs to `workspace` (encoded folder name or cwd).
pub fn workspace_matches(workspace: &str, session_cwd: Option<&str>, project_dir: &str) -> bool {
    let ws = workspace.trim().trim_end_matches(['/', '\\']);
    if ws.is_empty() {
        return false;
    }
    let encoded = encode_project_dir(ws);
    if !encoded.is_empty()
        && (project_dir == encoded || project_dir.starts_with(&format!("{encoded}-")))
    {
        return true;
    }
    if let Some(cwd) = session_cwd.map(str::trim).filter(|c| !c.is_empty()) {
        let a = Path::new(ws);
        let b = Path::new(cwd.trim_end_matches(['/', '\\']));
        if a == b {
            return true;
        }
        if encode_project_dir(cwd) == encoded {
            return true;
        }
    }
    false
}

/// Recent sessions under `claude_dir/projects`, preferring ones that match
/// `workspace`. Unrelated sessions are omitted unless `include_unrelated`.
pub fn list_sessions(
    claude_dir: &Path,
    workspace: &str,
    include_unrelated: bool,
) -> Vec<ClaudeSessionRow> {
    let projects = claude_dir.join("projects");
    let Ok(entries) = std::fs::read_dir(&projects) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() {
            continue;
        }
        let project = entry.path();
        if is_link(&project) {
            continue;
        }
        let project_name = project
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let Ok(files) = std::fs::read_dir(&project) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if is_link(&path) {
                continue;
            }
            if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let Ok(meta) = file.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let updated_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
            let skim = skim_session(&path);
            let matches = workspace_matches(workspace, skim.cwd.as_deref(), &project_name);
            if !matches && !include_unrelated {
                continue;
            }
            rows.push(ClaudeSessionRow {
                id,
                title: skim.title,
                cwd: skim.cwd,
                path: path.to_string_lossy().into_owned(),
                turn_count: skim.turn_count,
                updated_ms,
                matches_workspace: matches,
            });
        }
    }
    rows.sort_by(|a, b| {
        b.matches_workspace
            .cmp(&a.matches_workspace)
            .then(b.updated_ms.cmp(&a.updated_ms))
            .then(a.id.cmp(&b.id))
    });
    rows.truncate(MAX_SESSIONS);
    rows
}

/// Import one session file. Path must be a `.jsonl` under the Claude projects
/// tree (or, in tests, any readable `.jsonl` when `allow_any` is true).
pub fn import_session(path: &Path, claude_dir: &Path) -> Result<ClaudeImport, String> {
    if path.extension().is_none_or(|e| e != "jsonl") {
        return Err(NOT_JSONL.into());
    }
    if is_link(path) {
        return Err(MISSING.into());
    }
    let meta = std::fs::metadata(path).map_err(|_| MISSING.to_string())?;
    if !meta.is_file() {
        return Err(MISSING.into());
    }
    let projects = claude_dir.join("projects");
    if !path_under(&projects, path) {
        return Err(OUTSIDE_PROJECTS.into());
    }
    if meta.len() > IMPORT_FILE_CAP {
        return Err(format!(
            "That Claude session is too large to import (limit {} MB).",
            IMPORT_FILE_CAP / (1024 * 1024)
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|_| CORRUPT.to_string())?;
    parse_session_jsonl(&text)
}

/// Parse session JSONL text into a sanitized import payload.
pub fn parse_session_jsonl(text: &str) -> Result<ClaudeImport, String> {
    if text.trim().is_empty() {
        return Err(EMPTY.into());
    }
    let mut title: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut turns: Vec<ClaudeImportTurn> = Vec::new();
    let mut files: BTreeSet<String> = BTreeSet::new();
    let mut saw_json = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        saw_json = true;
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if cwd.is_none() {
            if let Some(c) = v.get("cwd").and_then(|c| c.as_str()).map(str::trim) {
                if !c.is_empty() {
                    cwd = Some(c.to_string());
                }
            }
        }
        match kind {
            "summary" => {
                if let Some(s) = v
                    .get("summary")
                    .and_then(|s| s.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    title = Some(s.chars().take(80).collect());
                }
            }
            "custom-title" => {
                if let Some(s) = v
                    .get("customTitle")
                    .or_else(|| v.get("title"))
                    .and_then(|s| s.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    title = Some(s.chars().take(80).collect());
                }
            }
            "user" => {
                if let Some(body) = user_prompt_text(&v) {
                    let text = sanitize_text(&body);
                    if !text.is_empty() {
                        if title.is_none() {
                            title = Some(text.chars().take(80).collect());
                        }
                        turns.push(ClaudeImportTurn {
                            role: "user".into(),
                            text: truncate_chars(&text, MAX_TURN_CHARS),
                        });
                    }
                }
                collect_paths_from_value(&v, &mut files);
            }
            "assistant" => {
                if let Some(body) = assistant_text(&v) {
                    let text = sanitize_text(&body);
                    if !text.is_empty() {
                        turns.push(ClaudeImportTurn {
                            role: "assistant".into(),
                            text: truncate_chars(&text, MAX_TURN_CHARS),
                        });
                    }
                }
                collect_paths_from_value(&v, &mut files);
            }
            _ => {
                collect_paths_from_value(&v, &mut files);
            }
        }
    }

    if !saw_json {
        return Err(CORRUPT.into());
    }
    if turns.is_empty() {
        return Err(EMPTY.into());
    }
    if turns.len() > MAX_TURNS {
        turns = turns.split_off(turns.len() - MAX_TURNS);
    }
    let files: Vec<String> = files.into_iter().take(MAX_FILES).collect();
    let title = title.unwrap_or_else(|| "Claude Code session".into());
    let mut imported = ClaudeImport {
        title,
        cwd,
        turns,
        files,
        context: String::new(),
    };
    imported.context = format_context(&imported);
    Ok(imported)
}

/// Strip secrets and oversized base64-looking blobs.
pub fn sanitize_text(input: &str) -> String {
    let stripped = strip_base64_blobs(input);
    redact_for_share(&stripped)
}

fn format_context(imp: &ClaudeImport) -> String {
    let mut out = String::from("## Imported Claude Code session\n\n");
    out.push_str(&format!("Title: {}\n", imp.title));
    if let Some(cwd) = &imp.cwd {
        out.push_str(&format!("Cwd: {}\n", cwd));
    }
    out.push_str("\n### Recent turns\n");
    for turn in &imp.turns {
        out.push_str(&format!("\n**{}:**\n{}\n", turn.role, turn.text));
    }
    if !imp.files.is_empty() {
        out.push_str("\n### Touched files\n");
        for f in &imp.files {
            out.push_str(&format!("- {}\n", f));
        }
    }
    out.push_str(
        "\n(Read-only import from a local Claude Code transcript. No Claude or Grok CLI was run.)\n",
    );
    let sanitized = sanitize_text(&out);
    truncate_chars(&sanitized, MAX_CONTEXT_CHARS)
}

#[derive(Default)]
struct Skim {
    title: String,
    cwd: Option<String>,
    turn_count: u32,
}

fn skim_session(path: &Path) -> Skim {
    let mut skim = Skim {
        title: path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Claude session")
            .chars()
            .take(80)
            .collect(),
        ..Default::default()
    };
    let Ok(file) = std::fs::File::open(path) else {
        return skim;
    };
    let mut first_user: Option<String> = None;
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        if line.len() > LIST_LINE_CAP {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if skim.cwd.is_none() {
            if let Some(c) = v.get("cwd").and_then(|c| c.as_str()).map(str::trim) {
                if !c.is_empty() {
                    skim.cwd = Some(c.to_string());
                }
            }
        }
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match kind {
            "summary" => {
                if let Some(s) = v
                    .get("summary")
                    .and_then(|s| s.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    skim.title = s.chars().take(80).collect();
                }
            }
            "custom-title" => {
                if let Some(s) = v
                    .get("customTitle")
                    .or_else(|| v.get("title"))
                    .and_then(|s| s.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    skim.title = s.chars().take(80).collect();
                }
            }
            "user" => {
                if let Some(body) = user_prompt_text(&v) {
                    skim.turn_count = skim.turn_count.saturating_add(1);
                    if first_user.is_none() {
                        first_user = Some(body.chars().take(80).collect());
                    }
                }
            }
            "assistant" if assistant_text(&v).is_some() => {
                skim.turn_count = skim.turn_count.saturating_add(1);
            }
            _ => {}
        }
    }
    if skim.title == path.file_stem().and_then(|s| s.to_str()).unwrap_or("") {
        if let Some(u) = first_user {
            skim.title = u;
        }
    }
    skim
}

fn user_prompt_text(v: &Value) -> Option<String> {
    let message = v.get("message")?;
    let content = message.get("content")?;
    match content {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Value::Array(parts) => {
            // Tool-result user lines are not conversational prompts.
            if parts
                .iter()
                .any(|p| p.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
            {
                return None;
            }
            let mut out = String::new();
            for part in parts {
                if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(t.trim());
                    }
                }
            }
            let t = out.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        _ => None,
    }
}

fn assistant_text(v: &Value) -> Option<String> {
    let content = v.pointer("/message/content")?;
    match content {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Value::Array(parts) => {
            let mut out = String::new();
            for part in parts {
                let kind = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if kind == "text" {
                    if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(t.trim());
                    }
                }
            }
            let t = out.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        _ => None,
    }
}

fn collect_paths_from_value(v: &Value, files: &mut BTreeSet<String>) {
    if files.len() >= MAX_FILES {
        return;
    }
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                let key = k.as_str();
                if matches!(
                    key,
                    "file_path" | "filePath" | "path" | "filename" | "file_name"
                ) {
                    if let Some(p) = child.as_str().map(str::trim).filter(|p| !p.is_empty()) {
                        if looks_like_path(p) {
                            files.insert(p.to_string());
                            if files.len() >= MAX_FILES {
                                return;
                            }
                        }
                    }
                }
                collect_paths_from_value(child, files);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_paths_from_value(child, files);
                if files.len() >= MAX_FILES {
                    return;
                }
            }
        }
        _ => {}
    }
}

fn looks_like_path(p: &str) -> bool {
    if p.len() < 2 || p.len() > 512 {
        return false;
    }
    if p.contains('\n') || p.contains('\0') {
        return false;
    }
    p.contains('/') || p.contains('\\') || p.contains('.')
}

/// Replace long base64-looking runs with a short marker.
pub fn strip_base64_blobs(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut run = String::new();
    let flush_run = |run: &mut String, out: &mut String| {
        if run.chars().count() >= BASE64_BLOB_MIN {
            out.push_str("[omitted binary/base64 blob]");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for ch in input.chars() {
        if is_b64_char(ch) {
            run.push(ch);
        } else {
            flush_run(&mut run, &mut out);
            out.push(ch);
        }
    }
    flush_run(&mut run, &mut out);
    out
}

fn is_b64_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '-' | '_')
}

fn truncate_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn path_under(root: &Path, path: &Path) -> bool {
    let Ok(root) = std::fs::canonicalize(root) else {
        return false;
    };
    let Ok(path) = std::fs::canonicalize(path) else {
        return false;
    };
    path.starts_with(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_jsonl() -> String {
        let blob = "A".repeat(BASE64_BLOB_MIN + 40);
        format!(
            r#"{{"type":"summary","summary":"Fix the auth bug"}}
{{"type":"user","cwd":"/home/user/my-app","timestamp":"2026-11-20T12:00:00Z","message":{{"role":"user","content":"fix the failing auth test sk-abcdefghijklmnopqrstuv"}}}}
{{"type":"assistant","cwd":"/home/user/my-app","timestamp":"2026-11-20T12:00:05Z","message":{{"role":"assistant","content":[{{"type":"text","text":"I'll open the auth module."}},{{"type":"tool_use","id":"t1","name":"Read","input":{{"file_path":"/home/user/my-app/src/auth.rs"}}}}]}}}}
{{"type":"user","cwd":"/home/user/my-app","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"ok"}}]}}}}
{{"type":"assistant","cwd":"/home/user/my-app","message":{{"role":"assistant","content":[{{"type":"text","text":"Here is a dump: {blob}"}},{{"type":"tool_use","id":"t2","name":"Edit","input":{{"file_path":"/home/user/my-app/src/auth.rs"}}}}]}}}}
"#
        )
    }

    #[test]
    fn encode_project_dir_replaces_non_alnum_with_dashes() {
        assert_eq!(encode_project_dir("/home/user/my-app"), "-home-user-my-app");
        assert_eq!(
            encode_project_dir("C:\\Users\\user\\app"),
            "C--Users-user-app"
        );
        assert_eq!(encode_project_dir(""), "");
    }

    #[test]
    fn workspace_matching_uses_encoded_folder_and_cwd() {
        assert!(workspace_matches(
            "/home/user/my-app",
            Some("/home/user/my-app"),
            "-home-user-my-app"
        ));
        assert!(workspace_matches(
            "/home/user/my-app",
            None,
            "-home-user-my-app"
        ));
        assert!(!workspace_matches(
            "/home/user/my-app",
            Some("/home/user/other"),
            "-home-user-other"
        ));
    }

    #[test]
    fn parse_fixture_extracts_title_turns_files_and_strips_secrets_and_blobs() {
        let imp = parse_session_jsonl(&fixture_jsonl()).expect("parse");
        assert_eq!(imp.title, "Fix the auth bug");
        assert_eq!(imp.cwd.as_deref(), Some("/home/user/my-app"));
        assert_eq!(imp.turns.len(), 3, "tool_result user lines are skipped");
        assert_eq!(imp.turns[0].role, "user");
        assert!(imp.turns[0].text.contains("failing auth test"));
        assert!(
            !imp.turns[0].text.contains("sk-abcdefghijklmnopqrstuv"),
            "api key stripped: {}",
            imp.turns[0].text
        );
        assert!(imp.turns[2].text.contains("[omitted binary/base64 blob]"));
        assert!(!imp.turns[2].text.contains(&"A".repeat(BASE64_BLOB_MIN)));
        assert_eq!(imp.files, vec!["/home/user/my-app/src/auth.rs".to_string()]);
        assert!(imp.context.contains("Imported Claude Code session"));
        assert!(imp.context.contains("Touched files"));
        assert!(!imp.context.contains("sk-abcdefghijklmnopqrstuv"));
    }

    #[test]
    fn oversized_blob_truncation_on_sanitize() {
        let blob = format!("prefix {} suffix", "B".repeat(BASE64_BLOB_MIN + 10));
        let out = sanitize_text(&blob);
        assert!(out.contains("[omitted binary/base64 blob]"));
        assert!(out.contains("prefix"));
        assert!(out.contains("suffix"));
        assert!(!out.contains(&"B".repeat(BASE64_BLOB_MIN)));
    }

    #[test]
    fn missing_and_corrupt_files_error_clearly() {
        assert_eq!(parse_session_jsonl("").unwrap_err(), EMPTY);
        assert_eq!(
            parse_session_jsonl("not json at all\n").unwrap_err(),
            CORRUPT
        );
        assert_eq!(
            parse_session_jsonl(r#"{"type":"summary","summary":"only meta"}"#).unwrap_err(),
            EMPTY
        );
        let dir = std::env::temp_dir().join(format!("claude-import-miss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("projects")).unwrap();
        let err = import_session(&dir.join("projects/nope.jsonl"), &dir).unwrap_err();
        assert_eq!(err, MISSING);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_sessions_filters_to_workspace_and_reports_turn_count() {
        let dir = std::env::temp_dir().join(format!("claude-import-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mine = dir.join("projects/-home-user-my-app");
        let other = dir.join("projects/-home-user-other");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(mine.join("sess-a.jsonl"), fixture_jsonl()).unwrap();
        std::fs::write(
            other.join("sess-b.jsonl"),
            r#"{"type":"user","cwd":"/home/user/other","message":{"role":"user","content":"unrelated"}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}
"#,
        )
        .unwrap();

        let matched = list_sessions(&dir, "/home/user/my-app", false);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].id, "sess-a");
        assert_eq!(matched[0].title, "Fix the auth bug");
        assert!(matched[0].matches_workspace);
        assert_eq!(matched[0].turn_count, 3);
        assert!(matched[0].updated_ms > 0);

        let all = list_sessions(&dir, "/home/user/my-app", true);
        assert_eq!(all.len(), 2);

        let imported = import_session(Path::new(&matched[0].path), &dir).expect("import");
        assert_eq!(imported.title, "Fix the auth bug");

        let outside = import_session(Path::new("/tmp/not-under-projects.jsonl"), &dir);
        assert!(outside.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
