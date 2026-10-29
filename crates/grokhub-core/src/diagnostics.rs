//! One-click diagnostics. No secrets.
//!
//! Labs Diagnostics mode exports a chat/session debug bundle to the Desktop
//! as `loadout-diagnostics-DATE-TIME.zip` (JSON + readable Markdown). Paths
//! under the user's home become `/home/user`; tokens, emails, and credential-
//! like strings are redacted before anything is written.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::redact::redact_secrets;
use crate::user_home;

/// Clipboard About → Diagnostics (unchanged path).
pub fn diagnostics_bundle(
    version: &str,
    auth: bool,
    hub_kind: &str,
    skill_count: usize,
    last_receipt: Option<bool>,
    workboard_open: usize,
    notes: &str,
) -> String {
    let receipt = match last_receipt {
        Some(true) => "ok",
        Some(false) => "failed",
        None => "none",
    };
    // The status line can echo a pasted key; it never leaves in the bundle.
    let notes = redact_secrets(&notes.chars().take(400).collect::<String>());
    format!(
        "app GrokHub\nversion {version}\nos {} {}\nauth {}\nhub {hub_kind}\nskills {skill_count}\nlastHost {receipt}\nworkboardOpen {workboard_open}\nnotes {notes}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        if auth { "present" } else { "missing" },
    )
}

/// What a Labs Diagnostics export includes (shown before / with the save).
pub const DIAGNOSTICS_PREVIEW_LINES: &[&str] = &[
    "Per turn: user text and final reply (secrets redacted)",
    "Raw saved turn with TURN_* labels (Thought / Tool / Say)",
    "Classifier: intent, care, domain, why chat vs task",
    "Selected playbook and why, active personality + content level",
    "Brain policy row, length cap, model/provider",
    "Tool calls + results summary, approvals, errors, timings",
    "App version and OS",
    "Optional: recent app logs and service status",
    "Nothing leaves this machine unless you copy or share the file",
];

/// Classifier slice for one turn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsClassifier {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub intent: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub care: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub domain: String,
    /// Why the turn was chat vs task (or another intent).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why_chat_vs_task: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
}

/// Playbook pick for one turn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsPlaybook {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
}

/// Brain policy row applied to the turn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsPolicy {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub row: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goal: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub talk: String,
}

/// One tool step summary (args already redacted at source).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsToolCall {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub decision: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub approval_class: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub result_summary: String,
    #[serde(default)]
    pub hard_approved: bool,
}

/// One turn in a Labs Diagnostics export.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsTurn {
    pub turn: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub final_reply: String,
    /// Raw saved assistant turn; TURN_* labels stay visible here only.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw_saved_turn: String,
    #[serde(default)]
    pub classifier: DiagnosticsClassifier,
    #[serde(default)]
    pub playbook: DiagnosticsPlaybook,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub personality: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain_policy: Option<DiagnosticsPolicy>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub length_cap: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<DiagnosticsToolCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals_asked: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals_granted: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    #[serde(default)]
    pub wall_ms: u64,
    #[serde(default)]
    pub started_ms: u64,
    #[serde(default)]
    pub finished_ms: u64,
}

/// Full Labs Diagnostics export payload (JSON root + Markdown render).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsExport {
    pub app: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chat_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chat_title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub exported_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turns: Vec<DiagnosticsTurn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preview: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service_status: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recent_logs: String,
}

impl DiagnosticsExport {
    pub fn with_defaults(version: &str, chat_id: &str, chat_title: &str) -> Self {
        Self {
            app: "GrokHub".into(),
            version: version.into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            chat_id: chat_id.into(),
            chat_title: chat_title.into(),
            exported_at: export_stamp(SystemTime::now()),
            turns: Vec::new(),
            preview: DIAGNOSTICS_PREVIEW_LINES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            service_status: Vec::new(),
            recent_logs: String::new(),
        }
    }
}

/// The user's Desktop: the platform's own location (Windows known folder,
/// which OneDrive can redirect; XDG `user-dirs.dirs` on Linux), else
/// `<home>/Desktop`. The platform lookup only counts for the real home, so a
/// `HOME`/`USERPROFILE` override (tests, portable runs) keeps its own Desktop.
pub fn desktop_dir() -> Option<PathBuf> {
    let home = user_home()?;
    let platform = dirs::desktop_dir()
        .filter(|_| dirs::home_dir().as_deref() == Some(home.as_path()))
        .filter(|d| d != &home);
    Some(platform.unwrap_or_else(|| home.join("Desktop")))
}

/// `loadout-diagnostics-YYYYMMDD-HHMMSS.zip` from a wall-clock stamp.
pub fn diagnostics_zip_name(at: SystemTime) -> String {
    format!("loadout-diagnostics-{}.zip", file_stamp(at))
}

/// Destination path on the Desktop for a new export.
pub fn diagnostics_zip_path(at: SystemTime) -> Result<PathBuf, String> {
    let dir = desktop_dir().ok_or_else(|| "HOME is unset; cannot find Desktop".to_string())?;
    Ok(dir.join(diagnostics_zip_name(at)))
}

/// Redact secrets, emails/PII, credential assignments (`DB_PASSWORD=…`,
/// `"api_key": "…"`), and home-rooted paths for a diagnostics export.
pub fn redact_for_diagnostics(input: &str, home: Option<&str>) -> String {
    let (s, _) = crate::pii::redact_recall(input);
    let mut s = redact_credential_values(&s);
    if let Some(home) = home.map(str::trim).filter(|h| !h.is_empty()) {
        s = redact_home_paths(&s, home);
    }
    s
}

/// Apply [`redact_for_diagnostics`] to every string field on a turn,
/// metadata included (custom playbook names, policy reasons, tool names).
pub fn redact_turn(mut turn: DiagnosticsTurn, home: Option<&str>) -> DiagnosticsTurn {
    let r = |s: &mut String| *s = redact_for_diagnostics(s, home);
    r(&mut turn.user_text);
    r(&mut turn.final_reply);
    r(&mut turn.raw_saved_turn);
    let c = &mut turn.classifier;
    for s in [
        &mut c.intent,
        &mut c.care,
        &mut c.domain,
        &mut c.why_chat_vs_task,
        &mut c.by,
    ] {
        r(s);
    }
    let p = &mut turn.playbook;
    for s in [&mut p.name, &mut p.id, &mut p.why] {
        r(s);
    }
    if let Some(p) = &mut turn.brain_policy {
        for s in [&mut p.row, &mut p.goal, &mut p.reason, &mut p.talk] {
            r(s);
        }
    }
    for s in [
        &mut turn.personality,
        &mut turn.content_level,
        &mut turn.length_cap,
        &mut turn.model,
        &mut turn.provider,
    ] {
        r(s);
    }
    for t in &mut turn.tool_calls {
        for s in [
            &mut t.tool,
            &mut t.decision,
            &mut t.approval_class,
            &mut t.result_summary,
        ] {
            r(s);
        }
    }
    for s in turn
        .errors
        .iter_mut()
        .chain(turn.approvals_asked.iter_mut())
        .chain(turn.approvals_granted.iter_mut())
    {
        r(s);
    }
    turn
}

/// Key names whose value is a credential, matched inside the key
/// (`DATABASE_PASSWORD`, `x-api-key`, `client_secret`).
const CREDENTIAL_KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "apikey",
    "api_key",
    "api-key",
    "access_key",
    "accesskey",
    "private_key",
    "privatekey",
    "credential",
    "authorization",
];

fn is_key_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

fn is_credential_key(key: &str) -> bool {
    // `max_tokens` / `tokens_used` count tokens; they are not a token.
    let key = key.to_ascii_lowercase().replace("tokens", "");
    CREDENTIAL_KEYS.iter().any(|k| key.contains(k))
}

/// Mask the value in `KEY=value`, `KEY: value` and `"key": "value"` when the
/// key names a credential, numbers and escaped quotes included.
fn redact_credential_values(input: &str) -> String {
    let b = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < b.len() {
        if !is_key_byte(b[i]) {
            let ch = input[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let start = i;
        while i < b.len() && is_key_byte(b[i]) {
            i += 1;
        }
        out.push_str(&input[start..i]);
        if !is_credential_key(&input[start..i]) {
            continue;
        }
        let mut j = i;
        if j < b.len() && matches!(b[j], b'"' | b'\'') {
            j += 1;
        }
        while j < b.len() && matches!(b[j], b' ' | b'\t') {
            j += 1;
        }
        if j >= b.len() || !matches!(b[j], b'=' | b':') {
            continue;
        }
        j += 1;
        while j < b.len() && matches!(b[j], b' ' | b'\t') {
            j += 1;
        }
        let quote = (j < b.len() && matches!(b[j], b'"' | b'\'')).then(|| b[j]);
        if quote.is_some() {
            j += 1;
        }
        let vstart = j;
        while j < b.len()
            && match quote {
                Some(q) => b[j] != q && b[j] != b'\n',
                None => !b[j].is_ascii_whitespace() && !matches!(b[j], b',' | b';' | b'}'),
            }
        {
            // `\"` inside a quoted value is part of it.
            if quote.is_some() && b[j] == b'\\' && j + 1 < b.len() && b[j + 1] != b'\n' {
                j += 1;
            }
            j += 1;
        }
        let value = &input[vstart..j];
        if value.is_empty() || value == "[redacted]" {
            continue;
        }
        out.push_str(&input[i..vstart]);
        out.push_str("[redacted]");
        i = j;
    }
    out
}

/// Short reason line for intent (chat vs task and friends).
pub fn why_chat_vs_task(intent: &str, by: &str) -> String {
    let intent = intent.trim().to_ascii_lowercase();
    let by = by.trim();
    let base = match intent.as_str() {
        "chat" => "chat: short greeting or casual talk, not an action ask",
        "task" => "task: action verb, files, or tools — not casual chat",
        "question" => "question: asking for an answer, not a change",
        "repair" => "repair: something broken or failing",
        "research" => "research: look up / summarize sources",
        "creative" => "creative: draft or write",
        "background" => "background: scheduled or headless job",
        other if !other.is_empty() => return format!("{other}: classified as {other}"),
        _ => "unclassified",
    };
    if by.is_empty() {
        base.into()
    } else {
        format!("{base} (by {by})")
    }
}

/// Content level label for the active personality gate.
pub fn content_level_label(explicit_personalities: bool, personality_explicit: bool) -> String {
    if explicit_personalities && personality_explicit {
        "adults-only".into()
    } else if explicit_personalities {
        "adults-only allowed (standard voice)".into()
    } else {
        "standard".into()
    }
}

/// Readable Markdown for a diagnostics export.
pub fn diagnostics_markdown(export: &DiagnosticsExport) -> String {
    let mut out = String::new();
    out.push_str(crate::brand::filled("# {product} diagnostics\n\n"));
    out.push_str(&format!(
        "- App: {} {}\n- OS: {} {}\n- Chat: {} ({})\n- Exported: {}\n\n",
        export.app,
        export.version,
        export.os,
        export.arch,
        if export.chat_title.is_empty() {
            "(untitled)"
        } else {
            &export.chat_title
        },
        export.chat_id,
        export.exported_at
    ));
    if !export.preview.is_empty() {
        out.push_str("## What's included\n\n");
        for line in &export.preview {
            out.push_str(&format!("- {line}\n"));
        }
        out.push('\n');
    }
    if !export.service_status.is_empty() {
        out.push_str("## Service status\n\n");
        for line in &export.service_status {
            out.push_str(&format!("- {line}\n"));
        }
        out.push('\n');
    }
    for turn in &export.turns {
        out.push_str(&format!("## Turn {}\n\n", turn.turn));
        out.push_str("### User\n\n");
        out.push_str(&fence(&turn.user_text));
        out.push_str("\n### Final reply\n\n");
        out.push_str(&fence(&turn.final_reply));
        if !turn.raw_saved_turn.is_empty() {
            out.push_str("\n### Raw saved turn (TURN_* labels)\n\n");
            out.push_str(&fence(&turn.raw_saved_turn));
        }
        out.push_str("\n### Classifier\n\n");
        out.push_str(&format!(
            "- Intent: {}\n- Care: {}\n- Domain: {}\n- Why chat vs task: {}\n- By: {}\n",
            empty_dash(&turn.classifier.intent),
            empty_dash(&turn.classifier.care),
            empty_dash(&turn.classifier.domain),
            empty_dash(&turn.classifier.why_chat_vs_task),
            empty_dash(&turn.classifier.by)
        ));
        out.push_str("\n### Playbook\n\n");
        out.push_str(&format!(
            "- Name: {}\n- Id: {}\n- Why: {}\n",
            empty_dash(&turn.playbook.name),
            empty_dash(&turn.playbook.id),
            empty_dash(&turn.playbook.why)
        ));
        out.push_str(&format!(
            "\n### Personality / content\n\n- Personality: {}\n- Content level: {}\n",
            empty_dash(&turn.personality),
            empty_dash(&turn.content_level)
        ));
        if let Some(p) = &turn.brain_policy {
            out.push_str(&format!(
                "\n### Brain policy\n\n- Row: {}\n- Goal: {}\n- Reason: {}\n- Talk: {}\n",
                empty_dash(&p.row),
                empty_dash(&p.goal),
                empty_dash(&p.reason),
                empty_dash(&p.talk)
            ));
        }
        out.push_str(&format!(
            "\n### Length cap / model\n\n- Length cap: {}\n- Model: {}\n- Provider: {}\n",
            empty_dash(&turn.length_cap),
            empty_dash(&turn.model),
            empty_dash(&turn.provider)
        ));
        if !turn.tool_calls.is_empty() {
            out.push_str("\n### Tool calls\n\n");
            for t in &turn.tool_calls {
                out.push_str(&format!(
                    "- {} — decision {}, class {}, hard_approved={}, result: {}\n",
                    empty_dash(&t.tool),
                    empty_dash(&t.decision),
                    empty_dash(&t.approval_class),
                    t.hard_approved,
                    empty_dash(&t.result_summary)
                ));
            }
        }
        if !turn.approvals_asked.is_empty() || !turn.approvals_granted.is_empty() {
            out.push_str("\n### Approvals\n\n");
            for a in &turn.approvals_asked {
                out.push_str(&format!("- Asked: {a}\n"));
            }
            for a in &turn.approvals_granted {
                out.push_str(&format!("- Granted: {a}\n"));
            }
        }
        if !turn.errors.is_empty() {
            out.push_str("\n### Errors\n\n");
            for e in &turn.errors {
                out.push_str(&format!("- {e}\n"));
            }
        }
        out.push_str(&format!(
            "\n### Timings\n\n- Wall ms: {}\n- Started ms: {}\n- Finished ms: {}\n\n",
            turn.wall_ms, turn.started_ms, turn.finished_ms
        ));
    }
    if !export.recent_logs.is_empty() {
        out.push_str("## Recent app logs\n\n");
        out.push_str(&fence(&export.recent_logs));
        out.push('\n');
    }
    out.push_str(
        "\n---\nLocal only. Secrets, tokens, emails, and home paths were redacted before write.\n",
    );
    out
}

/// Write `diagnostics.json` + `diagnostics.md` into a new zip at `path`, or
/// at `name-2.zip`, `name-3.zip`… when that file exists. Never overwrites an
/// earlier bundle. Returns the path written.
pub fn write_diagnostics_zip(path: &Path, export: &DiagnosticsExport) -> Result<PathBuf, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Desktop folder: {e}"))?;
    }
    let json =
        serde_json::to_string_pretty(export).map_err(|e| format!("diagnostics json: {e}"))?;
    let md = diagnostics_markdown(export);
    let (file, path) = create_new_numbered(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("diagnostics.json", opts)
        .map_err(|e| format!("zip json: {e}"))?;
    zip.write_all(json.as_bytes())
        .map_err(|e| format!("zip json write: {e}"))?;
    zip.start_file("diagnostics.md", opts)
        .map_err(|e| format!("zip md: {e}"))?;
    zip.write_all(md.as_bytes())
        .map_err(|e| format!("zip md write: {e}"))?;
    zip.finish().map_err(|e| format!("zip finish: {e}"))?;
    Ok(path)
}

/// Create `path` only if it doesn't exist, else the first free `stem-N.ext`.
fn create_new_numbered(path: &Path) -> Result<(std::fs::File, PathBuf), String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("diagnostics")
        .to_string();
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("zip");
    for n in 1u32.. {
        let candidate = if n == 1 {
            path.to_path_buf()
        } else {
            path.with_file_name(format!("{stem}-{n}.{ext}"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => return Ok((f, candidate)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("create zip: {e}")),
        }
    }
    unreachable!("an unbounded name search returns or errors")
}

/// Status line after a successful Desktop write (triggers a Saved toast;
/// the chat's notice carries the Open folder action).
pub fn diagnostics_saved_status(path: &Path) -> String {
    format!(
        "Saved {} to your Desktop",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("diagnostics.zip")
    )
}

fn empty_dash(s: &str) -> &str {
    if s.trim().is_empty() {
        "—"
    } else {
        s
    }
}

fn fence(body: &str) -> String {
    if body.trim().is_empty() {
        "_(empty)_\n".into()
    } else {
        format!("```\n{body}\n```\n")
    }
}

fn file_stamp(at: SystemTime) -> String {
    let secs = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    // UTC YYYYMMDD-HHMMSS without chrono.
    let days = secs / 86_400;
    let day_secs = secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let hh = day_secs / 3600;
    let mm = (day_secs % 3600) / 60;
    let ss = day_secs % 60;
    format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}")
}

fn export_stamp(at: SystemTime) -> String {
    let stamp = file_stamp(at);
    // 20261028-041530 → 2026-10-28T04:15:30Z
    format!(
        "{}-{}-{}T{}:{}:{}Z",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[9..11],
        &stamp[11..13],
        &stamp[13..15]
    )
}

/// Days since Unix epoch → (year, month, day). Proleptic Gregorian.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    // Howard Hinnant civil_from_days.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

fn redact_home_paths(input: &str, home: &str) -> String {
    let home = home.trim_end_matches(['/', '\\']);
    if home.is_empty() {
        return input.to_string();
    }
    let mut s = input.replace(home, "/home/user");
    // Windows-style separators if home used backslashes.
    let home_slash = home.replace('\\', "/");
    if home_slash != home {
        s = s.replace(&home_slash, "/home/user");
    }
    let home_backslash = home.replace('/', "\\");
    if home_backslash != home {
        s = s.replace(&home_backslash, "/home/user");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn no_secret_leak() {
        let t = diagnostics_bundle(
            "2.0.0",
            true,
            "grokhub-hub-v1",
            2,
            Some(true),
            1,
            "token sk-nope",
        );
        assert!(t.contains("version 2.0.0"));
        assert!(t.contains("skills 2"));
        assert!(!t.contains("sk-nope") || t.contains("notes"));
    }

    #[test]
    fn bundle_names_the_os_and_redacts_a_key_in_the_notes() {
        let t = diagnostics_bundle(
            "2.10.92-beta (beta @ abc1234)",
            false,
            "grokhub-hub-v1",
            0,
            None,
            0,
            "Saved key sk-abcdefghijklmnopqrstuv",
        );
        assert!(t.contains("version 2.10.92-beta (beta @ abc1234)\n"), "{t}");
        assert!(
            t.contains(&format!(
                "\nos {} {}\n",
                std::env::consts::OS,
                std::env::consts::ARCH
            )),
            "{t}"
        );
        assert!(t.contains("notes Saved key [redacted]\n"), "{t}");
        assert!(!t.contains("sk-abcdefghijklmnopqrstuv"), "{t}");
        assert!(
            t.contains("auth missing\nhub grokhub-hub-v1\nskills 0\nlastHost none\n"),
            "{t}"
        );
    }

    #[test]
    fn redact_for_diagnostics_strips_secret_email_and_home() {
        let home = "/home/alice";
        let raw = format!(
            "key sk-abcdefghijklmnopqrstuv mail user@example.com path {home}/secret/token.env"
        );
        let out = redact_for_diagnostics(&raw, Some(home));
        assert!(!out.contains("sk-abcdefghijklmnopqrstuv"), "{out}");
        assert!(!out.contains("user@example.com"), "{out}");
        assert!(
            out.contains("[email]") || out.contains("[redacted]"),
            "{out}"
        );
        assert!(!out.contains("/home/alice"), "{out}");
        assert!(out.contains("/home/user/secret/token.env"), "{out}");
    }

    #[test]
    fn why_chat_vs_task_names_chat_and_task() {
        assert!(why_chat_vs_task("chat", "rules").contains("chat:"));
        assert!(why_chat_vs_task("task", "rules").contains("task:"));
        assert!(why_chat_vs_task("task", "rules").contains("by rules"));
    }

    #[test]
    fn zip_name_and_write_round_trip() {
        let at = UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let name = diagnostics_zip_name(at);
        assert_eq!(name, "loadout-diagnostics-20231114-221320.zip");

        let dir = std::env::temp_dir().join(format!("gh-diag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(&name);
        let mut export = DiagnosticsExport::with_defaults("9.9.9-test", "thr-1", "Hangout");
        export.turns.push(redact_turn(
            DiagnosticsTurn {
                turn: 1,
                user_text: "hi sk-abcdefghijklmnopqrstuv".into(),
                final_reply: "hey".into(),
                raw_saved_turn: "TURN_SAY:\nhey\n".into(),
                classifier: DiagnosticsClassifier {
                    intent: "chat".into(),
                    care: "casual".into(),
                    domain: "general".into(),
                    why_chat_vs_task: why_chat_vs_task("chat", "rules"),
                    by: "rules".into(),
                },
                playbook: DiagnosticsPlaybook {
                    name: "hangout".into(),
                    id: "hangout".into(),
                    why: "matched chat × casual".into(),
                },
                personality: "nyx".into(),
                content_level: "standard".into(),
                brain_policy: Some(DiagnosticsPolicy {
                    row: "chat × casual".into(),
                    goal: "rapport".into(),
                    reason: "casual talk".into(),
                    talk: "short".into(),
                }),
                length_cap: "short, under 60 words".into(),
                model: "grok-4".into(),
                provider: "xai".into(),
                tool_calls: vec![],
                approvals_asked: vec![],
                approvals_granted: vec![],
                errors: vec![],
                wall_ms: 120,
                started_ms: 1,
                finished_ms: 121,
            },
            Some("/home/alice"),
        ));
        assert_eq!(write_diagnostics_zip(&path, &export).unwrap(), path);
        assert!(path.is_file());
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() > 40, "zip should be non-empty");

        let file = std::fs::File::open(&path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let mut json = String::new();
        zip.by_name("diagnostics.json")
            .unwrap()
            .read_to_string(&mut json)
            .unwrap();
        let mut md = String::new();
        zip.by_name("diagnostics.md")
            .unwrap()
            .read_to_string(&mut md)
            .unwrap();
        assert!(json.contains("\"intent\": \"chat\""), "{json}");
        assert!(!json.contains("sk-abcdefghijklmnopqrstuv"), "{json}");
        assert!(md.contains("TURN_SAY:"), "{md}");
        assert!(md.contains("Length cap:"), "{md}");
        assert!(md.contains("Why chat vs task:"), "{md}");
        assert_eq!(
            diagnostics_saved_status(&path),
            "Saved loadout-diagnostics-20231114-221320.zip to your Desktop"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn credential_assignments_are_redacted_in_env_json_and_yaml_forms() {
        let raw = "DATABASE_PASSWORD=not-a-real-password\n\
                   {\"api_key\": \"abc123xyz\", \"name\": \"harbor\"}\n\
                   client_secret: hunter2\n\
                   PIN_TOKEN=12345\n\
                   api_key: \"abc\\\"tail-secret\" next\n\
                   max_tokens: 4000\n\
                   author: Pat";
        assert_eq!(
            redact_for_diagnostics(raw, None),
            "DATABASE_PASSWORD=[redacted]\n\
             {\"api_key\": \"[redacted]\", \"name\": \"harbor\"}\n\
             client_secret: [redacted]\n\
             PIN_TOKEN=[redacted]\n\
             api_key: \"[redacted]\" next\n\
             max_tokens: 4000\n\
             author: Pat"
        );
    }

    #[test]
    fn redact_turn_covers_playbook_policy_and_tool_metadata() {
        let turn = redact_turn(
            DiagnosticsTurn {
                turn: 1,
                playbook: DiagnosticsPlaybook {
                    name: "mail pat@example.com".into(),
                    id: "custom".into(),
                    why: String::new(),
                },
                brain_policy: Some(DiagnosticsPolicy {
                    row: "task × careful".into(),
                    goal: "deploy".into(),
                    reason: "token=abc123xyz".into(),
                    talk: "short".into(),
                }),
                tool_calls: vec![DiagnosticsToolCall {
                    tool: "bash /home/alice/bin/run".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            Some("/home/alice"),
        );
        assert_eq!(turn.playbook.name, "mail [email]");
        assert_eq!(turn.brain_policy.unwrap().reason, "token=[redacted]");
        assert_eq!(turn.tool_calls[0].tool, "bash /home/user/bin/run");
    }

    #[test]
    fn a_second_save_with_the_same_name_gets_a_new_file() {
        let dir = std::env::temp_dir().join(format!("gh-diag-clobber-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("loadout-diagnostics-20231114-221320.zip");
        let first = DiagnosticsExport::with_defaults("1.0.0", "thr-1", "First");
        let second = DiagnosticsExport::with_defaults("1.0.0", "thr-2", "Second");
        assert_eq!(write_diagnostics_zip(&path, &first).unwrap(), path);
        let next = write_diagnostics_zip(&path, &second).unwrap();
        assert_eq!(next, dir.join("loadout-diagnostics-20231114-221320-2.zip"));
        let read_title = |p: &Path| {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(p).unwrap()).unwrap();
            let mut json = String::new();
            zip.by_name("diagnostics.json")
                .unwrap()
                .read_to_string(&mut json)
                .unwrap();
            serde_json::from_str::<DiagnosticsExport>(&json)
                .unwrap()
                .chat_title
        };
        assert_eq!(read_title(&path), "First");
        assert_eq!(read_title(&next), "Second");
        // No cap on the suffix.
        for n in 3..=100 {
            std::fs::write(
                dir.join(format!("loadout-diagnostics-20231114-221320-{n}.zip")),
                b"",
            )
            .unwrap();
        }
        assert_eq!(
            write_diagnostics_zip(&path, &first).unwrap(),
            dir.join("loadout-diagnostics-20231114-221320-101.zip")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A localized or moved Desktop from `user-dirs.dirs` wins over
    /// `~/Desktop`; without one the bundle goes to `~/Desktop`.
    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_follows_xdg_user_dirs() {
        let home = std::env::temp_dir().join(format!("gh-diag-xdg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".config")).unwrap();
        let old_home = std::env::var_os("HOME");
        let old_xdg = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("HOME", &home);
        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(desktop_dir(), Some(home.join("Desktop")));
        std::fs::write(
            home.join(".config/user-dirs.dirs"),
            "XDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\n",
        )
        .unwrap();
        assert_eq!(desktop_dir(), Some(home.join("Schreibtisch")));
        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        if let Some(v) = old_xdg {
            std::env::set_var("XDG_CONFIG_HOME", v);
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn content_level_labels() {
        assert_eq!(content_level_label(false, false), "standard");
        assert_eq!(content_level_label(false, true), "standard");
        assert_eq!(
            content_level_label(true, false),
            "adults-only allowed (standard voice)"
        );
        assert_eq!(content_level_label(true, true), "adults-only");
    }
}
