//! Telephony and phone-number agent protocol, consent policies, and privacy/retention.
//!
//! Credentials (provider API key and inbound webhook secret) live in the OS
//! keychain (`grokhub_agent::telephony_net`), never in this config file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

pub const TELEPHONY_DIR: &str = "telephony";
pub const CONFIG_FILE: &str = "config.json";
pub const TRANSCRIPTS_DIR: &str = "transcripts";
pub const DEFAULT_RETENTION_DAYS: u16 = 30;
pub const MAX_RETENTION_DAYS: u16 = 365;
pub const DEFAULT_VAPI_API: &str = "https://api.vapi.ai";
pub const DEFAULT_TWILIO_API: &str = "https://api.twilio.com";
const DAY_MS: u64 = 86_400_000;

/// Supported telephony / phone-agent provider backends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelephonyProvider {
    #[default]
    Vapi,
    Twilio,
    CustomWebhook,
}

impl TelephonyProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vapi => "vapi",
            Self::Twilio => "twilio",
            Self::CustomWebhook => "custom_webhook",
        }
    }

    pub fn default_api_base(self) -> &'static str {
        match self {
            Self::Vapi => DEFAULT_VAPI_API,
            Self::Twilio => DEFAULT_TWILIO_API,
            Self::CustomWebhook => "",
        }
    }
}

/// Consent rules for incoming phone calls.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InboundPolicy {
    pub allow_inbound: bool,
    pub allow_unknown_callers: bool,
    pub allowed_callers: Vec<String>,
}

/// Consent rules for outgoing phone calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutboundPolicy {
    pub allow_outbound: bool,
    pub require_per_call_confirm: bool,
    pub allowed_destinations: Vec<String>,
}

impl Default for OutboundPolicy {
    fn default() -> Self {
        Self {
            allow_outbound: false,
            require_per_call_confirm: true,
            allowed_destinations: Vec::new(),
        }
    }
}

/// Call audio, disclosure, redaction, and retention controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CallPrivacyPolicy {
    pub announce_ai_disclosure: bool,
    pub record_audio: bool,
    pub store_transcripts: bool,
    pub transcript_retention_days: u16,
    pub redact_pii_in_transcripts: bool,
}

impl Default for CallPrivacyPolicy {
    fn default() -> Self {
        Self {
            announce_ai_disclosure: true,
            record_audio: false,
            store_transcripts: true,
            transcript_retention_days: DEFAULT_RETENTION_DAYS,
            redact_pii_in_transcripts: true,
        }
    }
}

/// Persisted user telephony configuration (no secret tokens stored here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelephonyConfig {
    pub enabled: bool,
    pub provider: TelephonyProvider,
    pub phone_number: String,
    pub assistant_id: String,
    pub api_base: String,
    pub inbound: InboundPolicy,
    pub outbound: OutboundPolicy,
    pub privacy: CallPrivacyPolicy,
}

impl Default for TelephonyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: TelephonyProvider::Vapi,
            phone_number: String::new(),
            assistant_id: String::new(),
            api_base: DEFAULT_VAPI_API.to_string(),
            inbound: InboundPolicy::default(),
            outbound: OutboundPolicy::default(),
            privacy: CallPrivacyPolicy::default(),
        }
    }
}

impl TelephonyConfig {
    /// Returns the trimmed `api_base`, or the provider's default HTTPS base URL.
    pub fn effective_api_base(&self) -> &str {
        let trimmed = self.api_base.trim();
        if trimmed.is_empty() {
            self.provider.default_api_base()
        } else {
            trimmed
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallStatus {
    Queued,
    Ringing,
    InProgress,
    Ended,
    Failed,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallDecision {
    Allow,
    NeedsConfirm { reason: String },
    Deny { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub role: String,
    pub text: String,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallTranscript {
    pub call_id: String,
    pub provider: TelephonyProvider,
    pub direction: CallDirection,
    pub peer_number: String,
    pub started_ms: u64,
    pub ended_ms: u64,
    pub audio_recorded: bool,
    pub ai_disclosed: bool,
    pub summary: String,
    pub entries: Vec<TranscriptEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundWebhookEvent {
    CallStarted {
        call_id: String,
        from: String,
    },
    TranscriptTurn {
        call_id: String,
        role: String,
        text: String,
        ts_ms: u64,
    },
    EndOfCallReport {
        transcript: CallTranscript,
    },
}

/// Validates an E.164 phone number (`+` followed by 8..=15 digits, non-zero lead).
pub fn valid_e164(number: &str) -> bool {
    let trimmed = number.trim();
    let Some(digits) = trimmed.strip_prefix('+') else {
        return false;
    };
    let len = digits.len();
    (8..=15).contains(&len)
        && digits
            .as_bytes()
            .first()
            .is_some_and(|b| matches!(b, b'1'..=b'9'))
        && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Validates a provider endpoint URL: HTTPS anywhere, or HTTP only on loopback.
pub fn valid_api_base(raw: &str) -> bool {
    let Ok(parsed) = Url::parse(raw.trim()) else {
        return false;
    };
    match parsed.scheme() {
        "https" => parsed.host_str().is_some_and(|h| !h.is_empty()),
        "http" => parsed.host_str().is_some_and(is_loopback_host),
        _ => false,
    }
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// Validates a safe call identifier (alphanumeric, `-`, `_`, up to 96 bytes).
pub fn valid_call_id(id: &str) -> bool {
    let trimmed = id.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 96
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Validates telephony configuration before saving or enabling calls.
pub fn validate_config(cfg: &TelephonyConfig) -> Result<(), String> {
    if cfg.privacy.transcript_retention_days > MAX_RETENTION_DAYS {
        return Err(format!(
            "Transcript retention days cannot exceed {MAX_RETENTION_DAYS}."
        ));
    }
    let base = cfg.effective_api_base();
    if cfg.enabled || !base.is_empty() {
        if !valid_api_base(base) {
            return Err(
                "Telephony API base URL must use HTTPS (or loopback HTTP for local testing)."
                    .into(),
            );
        }
    }
    if cfg.enabled && !valid_e164(&cfg.phone_number) {
        return Err(
            "Configured agent phone number must be in E.164 format (e.g. +14155550100).".into(),
        );
    }
    if !cfg.phone_number.trim().is_empty() && !valid_e164(&cfg.phone_number) {
        return Err("Agent phone number must be in E.164 format (e.g. +14155550100).".into());
    }
    for caller in &cfg.inbound.allowed_callers {
        if !valid_e164(caller) {
            return Err(format!("Allowed caller '{caller}' is not valid E.164."));
        }
    }
    for dest in &cfg.outbound.allowed_destinations {
        if !valid_e164(dest) {
            return Err(format!("Allowed destination '{dest}' is not valid E.164."));
        }
    }
    Ok(())
}

pub fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join(TELEPHONY_DIR).join(CONFIG_FILE)
}

pub fn transcripts_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(TELEPHONY_DIR).join(TRANSCRIPTS_DIR)
}

pub fn load_config(config_dir: &Path) -> TelephonyConfig {
    let path = config_path(config_dir);
    let Ok(raw) = fs::read_to_string(path) else {
        return TelephonyConfig::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_config(config_dir: &Path, cfg: &TelephonyConfig) -> Result<(), String> {
    validate_config(cfg)?;
    let path = config_path(config_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed creating telephony dir: {e}"))?;
    }
    let body = serde_json::to_string_pretty(cfg)
        .map_err(|e| format!("Failed serializing telephony config: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body).map_err(|e| format!("Failed writing telephony config: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("Failed replacing telephony config: {e}"))
}

/// Checks consent and allowlist policies before placing an outbound call.
pub fn authorize_outbound(
    cfg: &TelephonyConfig,
    to_number: &str,
    user_confirmed: bool,
) -> CallDecision {
    if !cfg.enabled {
        return CallDecision::Deny {
            reason: "Telephony integration is disabled.".into(),
        };
    }
    if !cfg.outbound.allow_outbound {
        return CallDecision::Deny {
            reason: "Outbound phone calls are disabled by policy.".into(),
        };
    }
    if !valid_e164(&cfg.phone_number) {
        return CallDecision::Deny {
            reason: "No valid E.164 agent phone number is configured.".into(),
        };
    }
    let dest = to_number.trim();
    if !valid_e164(dest) {
        return CallDecision::Deny {
            reason: "Destination phone number must be in E.164 format.".into(),
        };
    }
    if !cfg.outbound.allowed_destinations.is_empty()
        && !cfg
            .outbound
            .allowed_destinations
            .iter()
            .any(|d| d.trim() == dest)
    {
        return CallDecision::Deny {
            reason: format!("Destination {dest} is not on the allowed outbound list."),
        };
    }
    if cfg.outbound.require_per_call_confirm && !user_confirmed {
        return CallDecision::NeedsConfirm {
            reason: format!("Explicit user confirmation required before calling {dest}."),
        };
    }
    CallDecision::Allow
}

/// Checks consent and caller allowlist before accepting an inbound call event.
pub fn authorize_inbound(cfg: &TelephonyConfig, from_number: &str) -> CallDecision {
    if !cfg.enabled {
        return CallDecision::Deny {
            reason: "Telephony integration is disabled.".into(),
        };
    }
    if !cfg.inbound.allow_inbound {
        return CallDecision::Deny {
            reason: "Inbound phone calls are disabled by policy.".into(),
        };
    }
    let caller = from_number.trim();
    if !valid_e164(caller) {
        return CallDecision::Deny {
            reason: "Caller number is missing or not valid E.164.".into(),
        };
    }
    let allowlisted = cfg
        .inbound
        .allowed_callers
        .iter()
        .any(|c| c.trim() == caller);
    if !allowlisted && !cfg.inbound.allow_unknown_callers {
        return CallDecision::Deny {
            reason: format!("Caller {caller} is not on the inbound allowlist."),
        };
    }
    CallDecision::Allow
}

/// Verifies a shared webhook secret in constant time.
pub fn verify_webhook_secret(expected: &str, provided: &str) -> bool {
    let e = expected.trim();
    let p = provided.trim();
    !e.is_empty() && !p.is_empty() && crate::pair::ct_eq(e.as_bytes(), p.as_bytes())
}

/// Parses an inbound webhook payload from Vapi or a generic custom webhook.
pub fn parse_inbound_webhook(
    body: &str,
    provider: TelephonyProvider,
    now_ms: u64,
) -> Result<InboundWebhookEvent, String> {
    let root: Value =
        serde_json::from_str(body).map_err(|e| format!("Invalid telephony webhook JSON: {e}"))?;
    let msg = root.get("message").unwrap_or(&root);
    let kind = msg
        .get("type")
        .or_else(|| msg.get("event"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let call_obj = msg.get("call").unwrap_or(&root);
    let call_id = call_obj
        .get("id")
        .or_else(|| msg.get("call_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !valid_call_id(&call_id) {
        return Err("Webhook missing valid call id.".into());
    }

    match kind {
        "call-started" | "status-update" | "call.started" => {
            let from = call_obj
                .get("customer")
                .and_then(|c| c.get("number"))
                .or_else(|| msg.get("from"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            Ok(InboundWebhookEvent::CallStarted { call_id, from })
        }
        "transcript" => {
            let role = msg
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user")
                .trim()
                .to_string();
            let text = msg
                .get("transcript")
                .or_else(|| msg.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let ts_ms = msg.get("ts_ms").and_then(Value::as_u64).unwrap_or(now_ms);
            Ok(InboundWebhookEvent::TranscriptTurn {
                call_id,
                role,
                text,
                ts_ms,
            })
        }
        "end-of-call-report" | "call.ended" => {
            let direction = match call_obj
                .get("type")
                .or_else(|| msg.get("direction"))
                .and_then(Value::as_str)
                .unwrap_or("")
            {
                "outboundPhoneCall" | "outbound" => CallDirection::Outbound,
                _ => CallDirection::Inbound,
            };
            let peer_number = call_obj
                .get("customer")
                .and_then(|c| c.get("number"))
                .or_else(|| msg.get("peer_number"))
                .or_else(|| msg.get("from"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let started_ms = msg
                .get("started_ms")
                .and_then(Value::as_u64)
                .unwrap_or(now_ms);
            let ended_ms = msg
                .get("ended_ms")
                .and_then(Value::as_u64)
                .unwrap_or(now_ms);
            let audio_recorded = msg
                .get("recordingUrl")
                .or_else(|| msg.get("artifact").and_then(|a| a.get("recordingUrl")))
                .and_then(Value::as_str)
                .is_some_and(|u| !u.trim().is_empty());
            let summary = msg
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let raw_entries = msg
                .get("entries")
                .or_else(|| msg.get("artifact").and_then(|a| a.get("messages")))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let entries = raw_entries
                .into_iter()
                .filter_map(|item| {
                    let text = item
                        .get("message")
                        .or_else(|| item.get("text"))
                        .and_then(Value::as_str)?
                        .trim()
                        .to_string();
                    if text.is_empty() {
                        return None;
                    }
                    let role = item
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .trim()
                        .to_string();
                    let ts_ms = item.get("ts_ms").and_then(Value::as_u64).unwrap_or(now_ms);
                    Some(TranscriptEntry { role, text, ts_ms })
                })
                .collect();
            Ok(InboundWebhookEvent::EndOfCallReport {
                transcript: CallTranscript {
                    call_id,
                    provider,
                    direction,
                    peer_number,
                    started_ms,
                    ended_ms,
                    audio_recorded,
                    ai_disclosed: true,
                    summary,
                    entries,
                },
            })
        }
        other => Err(format!("Unsupported telephony webhook event '{other}'.")),
    }
}

/// Redacts sensitive PII (SSN, card numbers, emails, phone numbers) from call text.
pub fn redact_transcript_text(text: &str) -> String {
    static RE_SSN: OnceLock<Option<Regex>> = OnceLock::new();
    static RE_CARD: OnceLock<Option<Regex>> = OnceLock::new();
    static RE_EMAIL: OnceLock<Option<Regex>> = OnceLock::new();
    static RE_PHONE: OnceLock<Option<Regex>> = OnceLock::new();

    let ssn = RE_SSN.get_or_init(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").ok());
    let card = RE_CARD.get_or_init(|| Regex::new(r"\b(?:\d[ -]?){13,19}\b").ok());
    let email = RE_EMAIL
        .get_or_init(|| Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").ok());
    let phone = RE_PHONE
        .get_or_init(|| Regex::new(r"(?:\+[1-9]\d{7,14}\b|\b\d{3}[-.\s]\d{3}[-.\s]\d{4}\b)").ok());

    let mut out = text.to_string();
    if let Some(re) = ssn {
        out = re.replace_all(&out, "[REDACTED-SSN]").into_owned();
    }
    if let Some(re) = card {
        out = re.replace_all(&out, "[REDACTED-CARD]").into_owned();
    }
    if let Some(re) = email {
        out = re.replace_all(&out, "[REDACTED-EMAIL]").into_owned();
    }
    if let Some(re) = phone {
        out = re.replace_all(&out, "[REDACTED-PHONE]").into_owned();
    }
    out
}

/// Persists a transcript when allowed by `privacy` and prunes expired records.
/// Returns `Ok(true)` if written, `Ok(false)` when storage/retention is disabled.
pub fn save_transcript(
    config_dir: &Path,
    privacy: &CallPrivacyPolicy,
    transcript: &CallTranscript,
    now_ms: u64,
) -> Result<bool, String> {
    if !privacy.store_transcripts || privacy.transcript_retention_days == 0 {
        return Ok(false);
    }
    if !valid_call_id(&transcript.call_id) {
        return Err("Invalid transcript call_id.".into());
    }
    let dir = transcripts_dir(config_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed creating transcripts dir: {e}"))?;

    let mut stored = transcript.clone();
    if privacy.redact_pii_in_transcripts {
        stored.summary = redact_transcript_text(&stored.summary);
        for entry in &mut stored.entries {
            entry.text = redact_transcript_text(&entry.text);
        }
    }

    let path = dir.join(format!("{}.json", stored.call_id));
    let body = serde_json::to_string_pretty(&stored)
        .map_err(|e| format!("Failed serializing transcript: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body).map_err(|e| format!("Failed writing transcript: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("Failed finalizing transcript: {e}"))?;

    let _ = purge_expired_transcripts(config_dir, privacy.transcript_retention_days, now_ms);
    Ok(true)
}

/// Removes saved transcripts older than `retention_days` relative to `now_ms`.
pub fn purge_expired_transcripts(config_dir: &Path, retention_days: u16, now_ms: u64) -> usize {
    let dir = transcripts_dir(config_dir);
    let Ok(read_dir) = fs::read_dir(dir) else {
        return 0;
    };
    let cutoff = if retention_days == 0 {
        u64::MAX
    } else {
        now_ms.saturating_sub(u64::from(retention_days).saturating_mul(DAY_MS))
    };
    let mut removed = 0usize;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(t) = serde_json::from_str::<CallTranscript>(&raw) else {
            continue;
        };
        let stamp = t.ended_ms.max(t.started_ms);
        if stamp < cutoff && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Reads all currently retained transcripts sorted newest-first.
pub fn list_transcripts(config_dir: &Path) -> Vec<CallTranscript> {
    let dir = transcripts_dir(config_dir);
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<CallTranscript> = read_dir
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                return None;
            }
            let raw = fs::read_to_string(path).ok()?;
            serde_json::from_str::<CallTranscript>(&raw).ok()
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.started_ms));
    out
}

/// Deletes a single transcript by call ID.
pub fn delete_transcript(config_dir: &Path, call_id: &str) -> Result<bool, String> {
    if !valid_call_id(call_id) {
        return Err("Invalid transcript call_id.".into());
    }
    let path = transcripts_dir(config_dir).join(format!("{}.json", call_id.trim()));
    if !path.is_file() {
        return Ok(false);
    }
    fs::remove_file(path)
        .map(|()| true)
        .map_err(|e| format!("Failed deleting transcript: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-telephony-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn enabled_cfg() -> TelephonyConfig {
        TelephonyConfig {
            enabled: true,
            provider: TelephonyProvider::Vapi,
            phone_number: "+14155550100".into(),
            assistant_id: "asst-1".into(),
            api_base: DEFAULT_VAPI_API.into(),
            inbound: InboundPolicy {
                allow_inbound: true,
                allow_unknown_callers: false,
                allowed_callers: vec!["+14155550111".into()],
            },
            outbound: OutboundPolicy {
                allow_outbound: true,
                require_per_call_confirm: true,
                allowed_destinations: vec!["+14155550222".into()],
            },
            privacy: CallPrivacyPolicy::default(),
        }
    }

    #[test]
    fn e164_api_base_and_call_id_validation() {
        assert!(valid_e164("+14155550100"));
        assert!(!valid_e164("14155550100"));
        assert!(!valid_e164("+0155550100"));
        assert!(!valid_e164("+1234567"));

        assert!(valid_api_base("https://api.vapi.ai"));
        assert!(valid_api_base("http://127.0.0.1:8080"));
        assert!(!valid_api_base("http://api.vapi.ai"));
        assert!(!valid_api_base("file:///tmp/x"));

        assert!(valid_call_id("call_123-abc"));
        assert!(!valid_call_id("../etc/passwd"));
        assert!(!valid_call_id(""));
    }

    #[test]
    fn privacy_defaults_disable_recording_and_require_disclosure() {
        let cfg = TelephonyConfig::default();
        assert!(!cfg.enabled);
        assert!(!cfg.privacy.record_audio);
        assert!(cfg.privacy.announce_ai_disclosure);
        assert!(cfg.privacy.redact_pii_in_transcripts);
        assert!(cfg.outbound.require_per_call_confirm);
    }

    #[test]
    fn outbound_consent_enforces_confirm_and_allowlist() {
        let mut cfg = enabled_cfg();
        assert!(matches!(
            authorize_outbound(&cfg, "+14155550222", false),
            CallDecision::NeedsConfirm { .. }
        ));
        assert_eq!(
            authorize_outbound(&cfg, "+14155550222", true),
            CallDecision::Allow
        );
        assert!(matches!(
            authorize_outbound(&cfg, "+14155559999", true),
            CallDecision::Deny { .. }
        ));
        cfg.enabled = false;
        assert!(matches!(
            authorize_outbound(&cfg, "+14155550222", true),
            CallDecision::Deny { .. }
        ));
    }

    #[test]
    fn inbound_consent_blocks_unknown_callers_unless_allowed() {
        let mut cfg = enabled_cfg();
        assert_eq!(authorize_inbound(&cfg, "+14155550111"), CallDecision::Allow);
        assert!(matches!(
            authorize_inbound(&cfg, "+14155550999"),
            CallDecision::Deny { .. }
        ));
        cfg.inbound.allow_unknown_callers = true;
        assert_eq!(authorize_inbound(&cfg, "+14155550999"), CallDecision::Allow);
    }

    #[test]
    fn transcript_redaction_and_retention_pruning_work() {
        let dir = temp_dir("retention");
        let now = 1_800_000_000_000u64;
        let old_ts = now - (35 * DAY_MS);

        let old = CallTranscript {
            call_id: "call-old".into(),
            provider: TelephonyProvider::Vapi,
            direction: CallDirection::Inbound,
            peer_number: "+14155550111".into(),
            started_ms: old_ts,
            ended_ms: old_ts,
            audio_recorded: false,
            ai_disclosed: true,
            summary: "Call from +14155550111".into(),
            entries: vec![],
        };
        let recent = CallTranscript {
            call_id: "call-new".into(),
            provider: TelephonyProvider::Vapi,
            direction: CallDirection::Inbound,
            peer_number: "+14155550111".into(),
            started_ms: now - 10_000,
            ended_ms: now,
            audio_recorded: false,
            ai_disclosed: true,
            summary: "Email alice@example.com SSN 123-45-6789".into(),
            entries: vec![TranscriptEntry {
                role: "user".into(),
                text: "Call me at +14155550111 or 4111-1111-1111-1111".into(),
                ts_ms: now,
            }],
        };

        let privacy = CallPrivacyPolicy::default();
        assert!(save_transcript(&dir, &privacy, &old, old_ts).unwrap());
        assert!(save_transcript(&dir, &privacy, &recent, now).unwrap());

        let saved = list_transcripts(&dir);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].call_id, "call-new");
        assert!(saved[0].summary.contains("[REDACTED-EMAIL]"));
        assert!(saved[0].summary.contains("[REDACTED-SSN]"));
        assert!(saved[0].entries[0].text.contains("[REDACTED-PHONE]"));
        assert!(saved[0].entries[0].text.contains("[REDACTED-CARD]"));

        // Ephemeral retention writes nothing.
        let mut ephemeral = privacy.clone();
        ephemeral.transcript_retention_days = 0;
        let eph = CallTranscript {
            call_id: "call-eph".into(),
            ..recent
        };
        assert!(!save_transcript(&dir, &ephemeral, &eph, now).unwrap());
        assert!(delete_transcript(&dir, "call-new").unwrap());
        assert!(list_transcripts(&dir).is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn webhook_secret_and_vapi_report_parsing() {
        assert!(verify_webhook_secret("top-secret", "top-secret"));
        assert!(!verify_webhook_secret("", ""));
        assert!(!verify_webhook_secret("top-secret", "wrong"));

        let body = r#"{
            "message": {
                "type": "end-of-call-report",
                "summary": "Scheduled follow up",
                "call": { "id": "vapi-99", "type": "inboundPhoneCall", "customer": { "number": "+14155550111" } },
                "artifact": { "messages": [ { "role": "user", "message": "Hi there" } ] }
            }
        }"#;
        let ev = parse_inbound_webhook(body, TelephonyProvider::Vapi, 5_000).unwrap();
        match ev {
            InboundWebhookEvent::EndOfCallReport { transcript } => {
                assert_eq!(transcript.call_id, "vapi-99");
                assert_eq!(transcript.direction, CallDirection::Inbound);
                assert_eq!(transcript.peer_number, "+14155550111");
                assert_eq!(transcript.entries.len(), 1);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
