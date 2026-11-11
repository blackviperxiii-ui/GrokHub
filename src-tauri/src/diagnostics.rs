//! Settings → Labs → Diagnostics mode (cabin parity): one click builds a
//! redacted debug bundle, the page previews it, Save writes the zip to the
//! Diagnostics folder (never uploaded), and Copy summary gives the same
//! redaction line the cabin shows. Everything is redacted by
//! `grokhub_core::redact_for_diagnostics`, the cabin's own redactor; this file
//! has none of its own. The two switches are the cabin's `app.json` keys
//! (`diagnosticsMode`, `diagnosticsIncludeLogs`). Chat text goes in only when
//! a chat is picked; with none picked the bundle holds the app facts and
//! service status alone.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use grokhub_core::{
    diagnostics_markdown, diagnostics_preview_lines, diagnostics_redaction_summary,
    redact_for_diagnostics, redact_turn, redacted_log_tail, DiagnosticsExport, DiagnosticsToolCall,
    DiagnosticsTurn,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::history::{self, Role};

const MODE_KEY: &str = "diagnosticsMode";
const LOGS_KEY: &str = "diagnosticsIncludeLogs";
/// A bigger `app.json` than this is not read or rewritten.
const APP_JSON_CAP: u64 = 1024 * 1024;
/// Recent app logs: the last this many characters, as in the cabin.
const LOG_TAIL: usize = 4_000;

pub const MODE_OFF: &str = "Turn on Diagnostics mode in Settings → Labs first";
pub const NO_TURNS: &str = "No turns to export";
pub const NOTHING_PENDING: &str = "Make a bundle first; there is nothing to save.";

/// The bundle last built, waiting for Save: Save writes exactly what the
/// preview showed.
static PENDING: Mutex<Vec<(PathBuf, DiagnosticsExport)>> = Mutex::new(Vec::new());

fn pending_take(config_dir: &Path) -> Option<DiagnosticsExport> {
    let mut all = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let at = all.iter().position(|(dir, _)| dir == config_dir)?;
    Some(all.remove(at).1)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub mode: bool,
    pub include_logs: bool,
    /// What each Save includes, the cabin's own lines.
    pub preview: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// "Redacted: 1 key, 0 emails, …", what Copy summary copies.
    pub summary: String,
    /// The redacted bundle as readable text.
    pub markdown: String,
    pub turns: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub path: String,
    pub status: String,
}

fn read_app(config_dir: &Path) -> Value {
    let path = config_dir.join("app.json");
    if !std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= APP_JSON_CAP) {
        return json!({});
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

pub fn page(config_dir: &Path) -> Page {
    let app = read_app(config_dir);
    let flag = |key: &str| app.get(key).and_then(Value::as_bool) == Some(true);
    let include_logs = flag(LOGS_KEY);
    Page {
        mode: flag(MODE_KEY),
        include_logs,
        preview: diagnostics_preview_lines(include_logs),
    }
}

/// Save the two switches into `app.json`, keeping every other key. A file
/// that doesn't parse is left alone.
pub fn set(config_dir: &Path, mode: bool, include_logs: bool) -> Result<Page, String> {
    let path = config_dir.join("app.json");
    let mut app = match std::fs::read_to_string(&path) {
        Ok(raw) if raw.len() as u64 > APP_JSON_CAP => {
            return Err("The settings file is too big to change here.".into());
        }
        Ok(raw) if raw.trim().is_empty() => json!({}),
        Ok(raw) => serde_json::from_str::<Value>(&raw)
            .ok()
            .filter(Value::is_object)
            .ok_or("The settings file can't be read, so it was left as it is.")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e.to_string()),
    };
    let map = app
        .as_object_mut()
        .ok_or("The settings file isn't an object.")?;
    map.insert(MODE_KEY.into(), json!(mode));
    map.insert(LOGS_KEY.into(), json!(include_logs));
    let body = serde_json::to_string_pretty(&app).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&path, &body)?;
    if !mode {
        pending_take(config_dir);
    }
    Ok(page(config_dir))
}

fn clip_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// One ask with its replies.
#[derive(Debug, PartialEq, Eq)]
struct Group {
    /// 1-based among visible asks.
    turn: u32,
    /// The raw `user` row numbers this ask used (spans count every one).
    raw_first: u32,
    raw_last: u32,
    user: String,
    assistant: String,
    tools: Vec<String>,
}

/// Visible asks with their replies, as the cabin groups them; a hidden
/// workload row belongs to the ask before it.
fn group_turns(lines: &[history::Line]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    let mut raw = 0u32;
    for line in lines {
        match line.role {
            Role::User => {
                raw += 1;
                if !grokhub_core::is_workload_user(&line.text) {
                    out.push(Group {
                        turn: out.len() as u32 + 1,
                        raw_first: raw,
                        raw_last: raw,
                        user: line.text.clone(),
                        assistant: String::new(),
                        tools: Vec::new(),
                    });
                } else if let Some(last) = out.last_mut() {
                    last.raw_last = raw;
                }
            }
            Role::Assistant => {
                if let Some(last) = out.last_mut() {
                    if !last.assistant.is_empty() {
                        last.assistant.push_str("\n\n");
                    }
                    last.assistant.push_str(&line.text);
                }
            }
            Role::Tools => {
                if let Some(last) = out.last_mut() {
                    last.tools.push(line.text.clone());
                }
            }
        }
    }
    out
}

fn service_status(has_key: bool, config_dir: &Path) -> Vec<String> {
    let mut doctor = grokhub_core::doctor_lines(
        has_key,
        config_dir.join("memory").is_dir(),
        grokhub_core::HUB_KIND,
    );
    doctor.extend(
        grokhub_agent::config_warnings()
            .into_iter()
            .map(|text| grokhub_core::DoctorLine { ok: false, text }),
    );
    doctor
        .into_iter()
        .map(|l| format!("{} — {}", if l.ok { "ok" } else { "fail" }, l.text))
        .collect()
}

fn recent_logs(config_dir: &Path, home: Option<&str>) -> String {
    for path in [
        config_dir.join("logs").join("cabin.log"),
        config_dir.join("cabin.log"),
        config_dir.join("hub.log"),
    ] {
        if let Some(tail) = redacted_log_tail(&path, home, LOG_TAIL) {
            return tail;
        }
    }
    String::new()
}

/// Builds the redacted export. `chat` is the one chat whose text goes in;
/// `None` leaves every chat out. Nothing is written.
pub fn build(
    config_dir: &Path,
    version: &str,
    has_key: bool,
    chat: Option<&str>,
    home: Option<&str>,
) -> Result<DiagnosticsExport, String> {
    let include_logs = page(config_dir).include_logs;
    let (id, title, model, lines) = match chat {
        Some(id) => {
            let info = history::saved(config_dir, id)?;
            let lines = history::lines(&info);
            (info.id, info.title, info.model, lines)
        }
        None => (String::new(), String::new(), String::new(), Vec::new()),
    };
    let mut export = DiagnosticsExport::with_defaults(version, &id, &title);
    export.service_status = service_status(has_key, config_dir);
    export.preview = diagnostics_preview_lines(include_logs);
    if include_logs {
        export.recent_logs = recent_logs(config_dir, home);
    }
    let spans = if id.is_empty() {
        Vec::new()
    } else {
        grokhub_agent::harness::read_spans(config_dir, &id).unwrap_or_default()
    };
    for group in group_turns(&lines) {
        let raw = group.raw_first..=group.raw_last;
        let mut turn = DiagnosticsTurn {
            turn: group.turn,
            user_text: group.user.clone(),
            final_reply: grokhub_core::turn_says(&group.assistant)
                .unwrap_or_else(|| group.assistant.clone()),
            raw_saved_turn: group.assistant.clone(),
            // The model the chat was saved with. Which provider answered a
            // turn is not saved, so none is named.
            model: model.clone(),
            ..DiagnosticsTurn::default()
        };
        let (mut started, mut finished) = (u64::MAX, 0u64);
        for span in spans.iter().filter(|s| raw.contains(&s.turn)) {
            started = started.min(span.ts_ms);
            finished = finished.max(span.ts_ms);
            turn.tool_calls.push(DiagnosticsToolCall {
                tool: span.tool.clone(),
                decision: span.decision.clone(),
                approval_class: span.approval_class.clone(),
                result_summary: clip_chars(&redact_for_diagnostics(&span.result, home), 160),
                hard_approved: span.hard_approved,
            });
            if span.decision == "park" || span.approval_class == "ask" {
                turn.approvals_asked
                    .push(format!("{} ({})", span.tool, span.approval_class));
            }
            if span.hard_approved || span.decision == "approve" {
                turn.approvals_granted
                    .push(format!("{} ({})", span.tool, span.decision));
            }
            if span.result == "error"
                || span.result.starts_with("error")
                || span.claim.to_ascii_lowercase().contains("error")
            {
                turn.errors
                    .push(clip_chars(&redact_for_diagnostics(&span.claim, home), 200));
            }
        }
        if started != u64::MAX {
            turn.started_ms = started;
            turn.finished_ms = finished;
            turn.wall_ms = finished.saturating_sub(started);
        }
        if group.assistant.trim_start().starts_with("Error:") {
            turn.errors.push(clip_chars(
                &redact_for_diagnostics(&group.assistant, home),
                240,
            ));
        }
        export.turns.push(redact_turn(turn, home));
    }
    if chat.is_some() && export.turns.is_empty() {
        return Err(NO_TURNS.into());
    }
    export.chat_title = redact_for_diagnostics(&export.chat_title, home);
    export.recent_logs = redact_for_diagnostics(&export.recent_logs, home);
    export.service_status = export
        .service_status
        .into_iter()
        .map(|s| redact_for_diagnostics(&s, home))
        .collect();
    Ok(export)
}

/// Builds the bundle, keeps it for Save and returns the preview.
pub fn preview(
    config_dir: &Path,
    version: &str,
    has_key: bool,
    chat: Option<&str>,
) -> Result<Preview, String> {
    let home = grokhub_core::user_home().and_then(|p| p.into_os_string().into_string().ok());
    preview_for(config_dir, version, has_key, chat, home.as_deref())
}

/// [`preview`] with the home folder to hide named, so a test can name its own.
fn preview_for(
    config_dir: &Path,
    version: &str,
    has_key: bool,
    chat: Option<&str>,
    home: Option<&str>,
) -> Result<Preview, String> {
    if !page(config_dir).mode {
        return Err(MODE_OFF.into());
    }
    let export = build(config_dir, version, has_key, chat, home)?;
    let markdown = diagnostics_markdown(&export);
    let out = Preview {
        summary: diagnostics_redaction_summary(&markdown),
        markdown,
        turns: export.turns.len(),
    };
    let mut all = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    all.retain(|(dir, _)| dir != config_dir);
    all.push((config_dir.to_path_buf(), export));
    Ok(out)
}

/// Writes exactly the bundle the preview showed.
pub fn save(config_dir: &Path) -> Result<Saved, String> {
    let export = pending_take(config_dir).ok_or(NOTHING_PENDING)?;
    let path = grokhub_core::diagnostics_zip_path_in(config_dir, SystemTime::now())?;
    let path = grokhub_core::write_diagnostics_zip(&path, &export)?;
    let place = grokhub_core::diagnostics_place(config_dir, &path);
    Ok(Saved {
        status: grokhub_core::diagnostics_saved_status(&path, place),
        path: path.display().to_string(),
    })
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
