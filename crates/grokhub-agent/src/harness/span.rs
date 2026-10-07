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
    /// Who started the step (harness design §12.0 rule 1). Old lines read as `user`.
    #[serde(default)]
    pub origin: Origin,
    /// The consent grant that allowed this step (`g-…`, or `approved-once`). Empty when none applied.
    #[serde(default)]
    pub consent_ref: String,
    /// Tokens and cost of a model call (Spike-4c router). Absent on every other step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ModelUsage>,
}

/// What one model call used, as the provider reported it. Counts only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cost_in_usd_ticks: i64,
}

/// Who started a step. Every origin goes through `harness::decide`; none skips it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    User,
    Proactive,
    Automation,
    SelfManage,
    Repair,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Proactive => "proactive",
            Self::Automation => "automation",
            Self::SelfManage => "self_manage",
            Self::Repair => "repair",
        }
    }
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
            origin: Origin::User,
            consent_ref: String::new(),
            usage: None,
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
            origin: Origin::User,
            consent_ref: String::new(),
            usage: None,
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
            origin: Origin::User,
            consent_ref: String::new(),
            usage: None,
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
            origin: Origin::User,
            consent_ref: String::new(),
            usage: None,
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

    pub fn from_origin(mut self, origin: Origin) -> Self {
        self.origin = origin;
        self
    }

    /// Tag the consent grant that allowed the step.
    pub fn with_consent(mut self, grant_id: &str) -> Self {
        self.consent_ref = grant_id.into();
        self
    }

    /// What the reply told the user at the end of a turn (Spike-1a), so the
    /// detectors can check a claim against the steps. Secret-shaped strings and
    /// `held` values (secrets the user typed this session) are redacted, and
    /// only the first [`CLAIM_CAP`] chars are kept.
    pub fn reply(session_id: &str, text: &str, held: &[String]) -> Self {
        let clean = grokhub_core::redact_held_secrets(&grokhub_core::redact_secrets(text), held);
        let claim: String = clean.trim().chars().take(CLAIM_CAP).collect();
        let mut s = Self::deny(session_id, REPLY_TOOL, "{}", "", "soft");
        s.claim = claim;
        s.decision = "say".into();
        s
    }

    /// `{session}:{ts_ms}`, how `egress.jsonl` points at a span.
    pub fn span_ref(&self) -> String {
        format!("{}:{}", self.session_id, self.ts_ms)
    }
}

/// Span tool for a reply's claim (decision `say`).
pub const REPLY_TOOL: &str = "reply";
/// Span tool for a verify script run (`result` is `pass` or `fail`).
pub const VERIFY_TOOL: &str = "verify_script";
/// How much of a reply a `say` span keeps.
pub const CLAIM_CAP: usize = 400;

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
        "passcode",
        "verification_code",
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
    /// Who started the turn (Spike-4c). Old files read as `user`.
    #[serde(default)]
    pub origin: Origin,
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
    fn reply_span_redacts_shaped_and_held_secrets_and_caps_the_claim() {
        let s = Span::reply(
            "chat-r",
            "Logged in with hunter2222 and key sk-abcdefghijklmnopqrstuv, then done.",
            &["hunter2222".into()],
        );
        assert_eq!(s.claim, "Logged in with [redacted] and key [redacted], then done.");
        assert_eq!((s.tool.as_str(), s.decision.as_str(), s.args_redacted.as_str()), (REPLY_TOOL, "say", "{}"));
        let long = Span::reply("chat-r", &"a".repeat(CLAIM_CAP + 50), &[]);
        assert_eq!(long.claim.chars().count(), CLAIM_CAP);
        let r = redact_args(r#"{"passcode":"9911","verification_code":"424242"}"#);
        assert!(!r.contains("9911") && !r.contains("424242"), "{r}");
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
            origin: Origin::Proactive,
        };
        write_turn_context(&dir, &ctx).unwrap();
        assert_eq!(read_turn_context(&dir), ctx);
        // A turn file from before Spike-4c has no origin: it reads as the user's.
        fs::write(turn_context_path(&dir), r#"{"chat_id":"chat-1","turn":2,"access":"supervised"}"#).unwrap();
        assert_eq!(read_turn_context(&dir).origin, Origin::User);
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

    #[test]
    fn old_span_files_still_read_with_origin_user_and_no_consent_ref() {
        let (dir, _guard) = temp_dir_named("old-origin");
        let path = span_path(&dir, "chat-old");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A Spike-0 line (path / chat_id / turn / ui_changed) and a pre-Spike-0 line.
        let spike0 = r#"{"session_id":"chat-old","ts_ms":5,"tool":"click","args_redacted":"{}","result":"ok","claim":"c","access":"supervised","approval_class":"soft","decision":"allow","driver":"grok_build","hard_approved":false,"path":"A","chat_id":"chat-old","turn":2,"ui_changed":true}"#;
        let older = r#"{"session_id":"chat-old","ts_ms":6,"tool":"t","args_redacted":"{}","result":"","claim":"","access":"","approval_class":"soft","decision":"allow","driver":"none"}"#;
        std::fs::write(&path, format!("{spike0}\n{older}\n")).unwrap();
        let got = read_spans(&dir, "chat-old").unwrap();
        assert_eq!(got.len(), 2);
        for s in &got {
            assert_eq!(s.origin, Origin::User);
            assert_eq!(s.consent_ref, "");
        }
        assert_eq!(got[0].path, "A");
        assert_eq!(got[0].ui_changed, Some(true));

        let tagged = Span::soft_allow("chat-old", "hub_sync", "{}", "sent", "c", AccessMode::Readonly, "none")
            .from_origin(Origin::Automation)
            .with_consent("g-0123456789ab");
        let line = serde_json::to_string(&tagged).unwrap();
        assert!(line.ends_with(r#","origin":"automation","consent_ref":"g-0123456789ab"}"#), "{line}");
        let back: Span = serde_json::from_str(&line).unwrap();
        assert_eq!(back, tagged);
        assert_eq!(tagged.span_ref(), format!("chat-old:{}", tagged.ts_ms));
    }
}
