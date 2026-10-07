//! Cabin-local span store (v0). Lives next to History under the cabin config dir.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::harness::access::AccessMode;
use crate::harness::hard::HardClass;

/// One tool step in a session trace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Span {
    pub session_id: String,
    pub ts_ms: u64,
    pub tool: String,
    /// Args with secrets redacted.
    pub args_redacted: String,
    pub result: String,
    pub claim: String,
    pub access: String,
    pub approval_class: String,
    /// "allow" | "deny" | "park" | "approve"
    pub decision: String,
    pub driver: String,
    /// True when a hard-class action was approved by Jeremy.
    #[serde(default)]
    pub hard_approved: bool,
    /// Engine path that checked the call: A desktop MCP, B ACP ask, C headless, E native.
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub chat_id: String,
    #[serde(default)]
    pub turn: u32,
    /// Pre/post screenshot hash differed. `None` when not measured.
    #[serde(default)]
    pub ui_changed: Option<bool>,
}

impl Span {
    pub fn soft_allow(
        session_id: &str,
        tool: &str,
        args: &str,
        result: &str,
        claim: &str,
        access: AccessMode,
        driver: &str,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            ts_ms: now_ms(),
            tool: tool.into(),
            args_redacted: redact_args(args),
            result: result.into(),
            claim: claim.into(),
            access: access.as_str().into(),
            approval_class: "soft".into(),
            decision: "allow".into(),
            driver: driver.into(),
            hard_approved: false,
            path: String::new(),
            chat_id: String::new(),
            turn: 0,
            ui_changed: None,
        }
    }

    pub fn hard_park(session_id: &str, tool: &str, args: &str, class: HardClass) -> Self {
        Self {
            session_id: session_id.into(),
            ts_ms: now_ms(),
            tool: tool.into(),
            args_redacted: redact_args(args),
            result: "parked".into(),
            claim: format!("needs Jeremy approve ({})", class.label()),
            access: String::new(),
            approval_class: class.as_str().into(),
            decision: "park".into(),
            driver: "none".into(),
            hard_approved: false,
            path: String::new(),
            chat_id: String::new(),
            turn: 0,
            ui_changed: None,
        }
    }

    pub fn hard_approve(session_id: &str, tool: &str, args: &str, class: HardClass) -> Self {
        Self {
            session_id: session_id.into(),
            ts_ms: now_ms(),
            tool: tool.into(),
            args_redacted: redact_args(args),
            result: "approved".into(),
            claim: format!("Jeremy approved ({})", class.label()),
            access: String::new(),
            approval_class: class.as_str().into(),
            decision: "approve".into(),
            driver: "none".into(),
            hard_approved: true,
            path: String::new(),
            chat_id: String::new(),
            turn: 0,
            ui_changed: None,
        }
    }

    pub fn deny(session_id: &str, tool: &str, args: &str, reason: &str, class: &str) -> Self {
        Self {
            session_id: session_id.into(),
            ts_ms: now_ms(),
            tool: tool.into(),
            args_redacted: redact_args(args),
            result: reason.into(),
            claim: "denied".into(),
            access: String::new(),
            approval_class: class.into(),
            decision: "deny".into(),
            driver: "none".into(),
            hard_approved: false,
            path: String::new(),
            chat_id: String::new(),
            turn: 0,
            ui_changed: None,
        }
    }

    /// Tag the engine path (`"A"`, `"B"`, `"C"`, `"E"`).
    pub fn on_path(mut self, path: &str) -> Self {
        self.path = path.into();
        self
    }

    /// Tag the chat and user turn the step belongs to.
    pub fn in_turn(mut self, chat_id: &str, turn: u32) -> Self {
        self.chat_id = chat_id.into();
        self.turn = turn;
        self
    }

    pub fn with_ui_changed(mut self, changed: Option<bool>) -> Self {
        self.ui_changed = changed;
        self
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Redact common secret-shaped values in tool args before span write.
pub fn redact_args(raw: &str) -> String {
    let mut out = raw.to_string();
    for key in [
        "password",
        "passwd",
        "api_key",
        "apikey",
        "secret",
        "token",
        "authorization",
        "private_key",
    ] {
        // Crude JSON string value wipe: "password":"…value…" → "password":"%redacted%"
        let patterns = [
            format!("\"{key}\":\""),
            format!("\"{key}\": \""),
            format!("{key}="),
        ];
        for pat in patterns {
            if let Some(i) = out.to_ascii_lowercase().find(&pat.to_ascii_lowercase()) {
                let start = i + pat.len();
                let rest = &out[start..];
                let end = if pat.ends_with('=') {
                    rest.find(|c: char| c.is_whitespace() || c == '&' || c == '"')
                        .unwrap_or(rest.len())
                } else {
                    rest.find('"').unwrap_or(rest.len())
                };
                out.replace_range(start..start + end, "%redacted%");
            }
        }
    }
    out
}

/// Chat turn the cabin is on, shared with the `--mcp-desktop` process so its
/// spans land in the same file. `{config_dir}/harness/turn.json`
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnContext {
    pub chat_id: String,
    pub turn: u32,
    pub access: String,
}

pub fn turn_context_path(config_dir: &Path) -> PathBuf {
    config_dir.join("harness").join("turn.json")
}

pub fn write_turn_context(config_dir: &Path, ctx: &TurnContext) -> Result<(), String> {
    let path = turn_context_path(config_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string(ctx).map_err(|e| e.to_string())?;
    fs::write(path, body).map_err(|e| e.to_string())
}

pub fn read_turn_context(config_dir: &Path) -> TurnContext {
    fs::read_to_string(turn_context_path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// `{config_dir}/spans/{session_id}.jsonl`
pub fn span_path(config_dir: &Path, session_id: &str) -> PathBuf {
    let safe: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    config_dir.join("spans").join(format!("{safe}.jsonl"))
}

pub fn append_span(config_dir: &Path, span: &Span) -> Result<PathBuf, String> {
    let path = span_path(config_dir, &span.session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let line = serde_json::to_string(span).map_err(|e| e.to_string())?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn read_spans(config_dir: &Path, session_id: &str) -> Result<Vec<Span>, String> {
    let path = span_path(config_dir, session_id);
    if !path.exists() {
        return Ok(vec![]);
    }
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let span: Span = serde_json::from_str(line).map_err(|e| e.to_string())?;
        out.push(span);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::access::AccessMode;

    struct DirGuard(PathBuf);
    impl Drop for DirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir() -> (PathBuf, DirGuard) {
        temp_dir_named("rt")
    }

    fn temp_dir_named(label: &str) -> (PathBuf, DirGuard) {
        let p = crate::harness::test_dir(&format!("span-{label}"));
        (p.clone(), DirGuard(p))
    }

    #[test]
    fn redact_password() {
        let raw = r#"{"password":"s3cret","x":1}"#;
        let r = redact_args(raw);
        assert!(r.contains("%redacted%"), "{r}");
        assert!(!r.contains("s3cret"), "{r}");
    }

    #[test]
    fn append_and_read_roundtrip() {
        let (dir, _guard) = temp_dir();
        let span = Span::soft_allow(
            "sess-1",
            "click",
            r#"{"x":1,"y":2}"#,
            "ok",
            "clicked button",
            AccessMode::Supervised,
            "grok_build",
        );
        append_span(&dir, &span).unwrap();
        let got = read_spans(&dir, "sess-1").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tool, "click");
        assert_eq!(got[0].driver, "grok_build");
        assert_eq!(got[0].decision, "allow");
        assert_eq!(got[0].path, "");
    }

    #[test]
    fn turn_context_round_trip_defaults_when_missing() {
        let (dir, _guard) = temp_dir_named("turn");
        assert_eq!(read_turn_context(&dir), TurnContext::default());
        let ctx = TurnContext {
            chat_id: "chat-9".into(),
            turn: 4,
            access: "full".into(),
        };
        write_turn_context(&dir, &ctx).unwrap();
        assert_eq!(read_turn_context(&dir), ctx);
    }

    #[test]
    fn span_carries_path_chat_turn_and_ui_changed() {
        let span = Span::soft_allow(
            "chat-7",
            "click",
            r#"{"x":10,"y":20}"#,
            "clicked",
            "click",
            AccessMode::Supervised,
            "grok_build",
        )
        .on_path("A")
        .in_turn("chat-7", 3)
        .with_ui_changed(Some(true));
        let line = serde_json::to_string(&span).unwrap();
        assert!(line.contains(r#""path":"A""#), "{line}");
        assert!(line.contains(r#""chat_id":"chat-7""#), "{line}");
        assert!(line.contains(r#""turn":3"#), "{line}");
        assert!(line.contains(r#""ui_changed":true"#), "{line}");
        let old = r#"{"session_id":"s","ts_ms":1,"tool":"t","args_redacted":"{}","result":"","claim":"","access":"","approval_class":"soft","decision":"allow","driver":"none"}"#;
        let back: Span = serde_json::from_str(old).unwrap();
        assert_eq!(back.ui_changed, None);
        assert_eq!(back.turn, 0);
    }
}
