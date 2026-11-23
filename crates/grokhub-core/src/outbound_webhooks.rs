//! Outbound signed lifecycle webhooks.
//!
//! A saved endpoint receives POSTs for session, turn, and tool events. Each
//! body is signed the same way inbound webhooks are verified
//! ([`crate::msg_channels::verify_webhook`]): HMAC-SHA256 over
//! `"{timestamp}.{body}"`, sent as `X-Webhook-Signature: sha256=<hex>` with
//! `X-Webhook-Timestamp`. Secrets live under `channels/outbound.json` and
//! are never returned after create.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::msg_channels::{hmac_sha256, verify_webhook};
use crate::update_feed::public_http_url;

/// Lifecycle kind a receiver can subscribe to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleKind {
    Session,
    Turn,
    Tool,
}

impl LifecycleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Turn => "turn",
            Self::Tool => "tool",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "session" => Some(Self::Session),
            "turn" => Some(Self::Turn),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }
}

/// One outbound endpoint the user set up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboundHook {
    pub name: String,
    /// HTTPS URL that receives the POST.
    pub url: String,
    /// Shared HMAC secret. Kept on disk; handed back only when created.
    pub secret: String,
    #[serde(default)]
    pub kinds: BTreeSet<LifecycleKind>,
    #[serde(default = "on")]
    pub enabled: bool,
}

fn on() -> bool {
    true
}

pub fn outbound_path(config_dir: &Path) -> PathBuf {
    config_dir.join("channels").join("outbound.json")
}

pub fn load_outbound(config_dir: &Path) -> Vec<OutboundHook> {
    std::fs::read_to_string(outbound_path(config_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_private(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_file_name(format!(".outbound.json.{}.tmp", std::process::id()));
    crate::state::write_private_synced(&tmp, text.as_bytes()).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(windows)]
    crate::win_acl::restrict_user_only(path);
    Ok(())
}

pub fn save_outbound(config_dir: &Path, hooks: &[OutboundHook]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(hooks).map_err(|e| e.to_string())?;
    write_private(&outbound_path(config_dir), &text)
}

fn valid_name(name: &str) -> bool {
    (1..=40).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Sign an outbound body the way [`crate::msg_channels::verify_webhook`] checks.
/// `timestamp` is Unix seconds as a decimal string.
pub fn sign_webhook(secret: &[u8], body: &[u8], timestamp: &str) -> Result<String, String> {
    if secret.len() < 16 {
        return Err("the webhook secret is too short".into());
    }
    let _: u64 = timestamp
        .trim()
        .parse()
        .map_err(|_| "the webhook timestamp isn't a number".to_string())?;
    let mut signed = format!("{}.", timestamp.trim()).into_bytes();
    signed.extend_from_slice(body);
    Ok(format!("sha256={}", hex::encode(hmac_sha256(secret, &signed))))
}

/// Headers a receiver needs, plus the Unix-seconds timestamp string.
pub fn signed_headers(
    secret: &[u8],
    body: &[u8],
    now_ms: u64,
) -> Result<(String, String), String> {
    let timestamp = (now_ms / 1000).to_string();
    let signature = sign_webhook(secret, body, &timestamp)?;
    Ok((signature, timestamp))
}

/// JSON body for one lifecycle event.
pub fn event_body(
    kind: LifecycleKind,
    event: &str,
    session: &str,
    detail: &Value,
    now_ms: u64,
) -> Value {
    json!({
        "kind": kind.as_str(),
        "event": event,
        "session": session,
        "detail": detail,
        "sentMs": now_ms,
    })
}

/// Endpoints that should receive `kind` right now.
pub fn matching(config_dir: &Path, kind: LifecycleKind) -> Vec<OutboundHook> {
    load_outbound(config_dir)
        .into_iter()
        .filter(|h| h.enabled && (h.kinds.is_empty() || h.kinds.contains(&kind)))
        .collect()
}

/// Add an outbound hook. `kinds` empty means every kind. The secret is shown
/// once in the returned hook.
pub fn add_outbound(
    config_dir: &Path,
    name: &str,
    url: &str,
    kinds: &[LifecycleKind],
) -> Result<OutboundHook, String> {
    let name = name.trim().to_ascii_lowercase();
    if !valid_name(&name) {
        return Err("An outbound webhook name is 1 to 40 of a-z, 0-9 and -.".into());
    }
    let url = url.trim();
    if !public_http_url(url) || !url.to_ascii_lowercase().starts_with("https://") {
        return Err("An outbound webhook URL must be a public https address.".into());
    }
    let mut hooks = load_outbound(config_dir);
    if hooks.iter().any(|h| h.name == name) {
        return Err(format!("There's already an outbound webhook named {name}."));
    }
    let mut secret = [0u8; 32];
    crate::fill_random(&mut secret);
    let kinds: BTreeSet<LifecycleKind> = kinds.iter().copied().collect();
    let hook = OutboundHook {
        name,
        url: url.to_string(),
        secret: hex::encode(secret),
        kinds,
        enabled: true,
    };
    hooks.push(hook.clone());
    save_outbound(config_dir, &hooks)?;
    Ok(hook)
}

/// Parse kind names from user text (`session,turn`).
pub fn parse_kinds(raw: &str) -> Result<Vec<LifecycleKind>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for part in trimmed.split([',', ' ', '|']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some(kind) = LifecycleKind::parse(part) else {
            return Err(format!(
                "Unknown lifecycle kind {part:?}. Use session, turn, or tool."
            ));
        };
        if !out.contains(&kind) {
            out.push(kind);
        }
    }
    Ok(out)
}

/// True when an outbound hook by that name was removed.
pub fn remove_outbound(config_dir: &Path, name: &str) -> Result<bool, String> {
    let mut hooks = load_outbound(config_dir);
    let before = hooks.len();
    hooks.retain(|h| h.name != name);
    if hooks.len() == before {
        return Ok(false);
    }
    save_outbound(config_dir, &hooks)?;
    Ok(true)
}

/// Build the signed POST parts for one hook. Does not send.
pub fn prepare_post(
    hook: &OutboundHook,
    kind: LifecycleKind,
    event: &str,
    session: &str,
    detail: &Value,
    now_ms: u64,
) -> Result<(String, Vec<(String, String)>, Vec<u8>), String> {
    let body = event_body(kind, event, session, detail, now_ms);
    let bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let (signature, timestamp) = signed_headers(hook.secret.as_bytes(), &bytes, now_ms)?;
    // Self-check: a receiver using verify_webhook accepts this.
    verify_webhook(
        hook.secret.as_bytes(),
        &bytes,
        &signature,
        &timestamp,
        now_ms,
    )?;
    let headers = vec![
        ("Content-Type".into(), "application/json".into()),
        ("X-Webhook-Signature".into(), signature),
        ("X-Webhook-Timestamp".into(), timestamp),
        ("User-Agent".into(), "GrokHub-OutboundWebhook/1".into()),
    ];
    Ok((hook.url.clone(), headers, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_700_000_000_000;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-out-hook-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sign_round_trips_through_verify_webhook() {
        let secret = b"0123456789abcdef0123456789abcdef";
        let body = br#"{"kind":"session","event":"SessionStart"}"#;
        let ts = (NOW / 1000).to_string();
        let sig = sign_webhook(secret, body, &ts).unwrap();
        assert!(sig.starts_with("sha256="));
        verify_webhook(secret, body, &sig, &ts, NOW).unwrap();
        assert_eq!(
            verify_webhook(
                secret,
                body,
                &sig,
                &ts,
                NOW + crate::msg_channels::WEBHOOK_SKEW_MS + 1
            )
            .unwrap_err(),
            "the webhook timestamp is too old or too far ahead"
        );
    }

    #[test]
    fn prepare_post_is_receiver_verifiable() {
        let hook = OutboundHook {
            name: "audit".into(),
            url: "https://hooks.example.com/life".into(),
            secret: "0123456789abcdef0123456789abcdef".into(),
            kinds: BTreeSet::from([LifecycleKind::Turn]),
            enabled: true,
        };
        let (url, headers, body) = prepare_post(
            &hook,
            LifecycleKind::Turn,
            "PreLlmCall",
            "chat-1",
            &json!({"model": "grok"}),
            NOW,
        )
        .unwrap();
        assert_eq!(url, hook.url);
        let sig = headers
            .iter()
            .find(|(k, _)| k == "X-Webhook-Signature")
            .map(|(_, v)| v.as_str())
            .unwrap();
        let ts = headers
            .iter()
            .find(|(k, _)| k == "X-Webhook-Timestamp")
            .map(|(_, v)| v.as_str())
            .unwrap();
        verify_webhook(hook.secret.as_bytes(), &body, sig, ts, NOW).unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["kind"], "turn");
        assert_eq!(parsed["event"], "PreLlmCall");
        assert_eq!(parsed["session"], "chat-1");
        assert_eq!(parsed["detail"]["model"], "grok");
    }

    #[test]
    fn outbound_hooks_are_added_and_matched_by_kind() {
        let dir = scratch("add");
        let hook = add_outbound(
            &dir,
            " Deploys ",
            "https://hooks.example.com/x",
            &[LifecycleKind::Session, LifecycleKind::Tool],
        )
        .unwrap();
        assert_eq!(hook.name, "deploys");
        assert_eq!(hook.secret.len(), 64);
        assert_eq!(
            matching(&dir, LifecycleKind::Session)
                .iter()
                .map(|h| h.name.as_str())
                .collect::<Vec<_>>(),
            vec!["deploys"]
        );
        assert!(matching(&dir, LifecycleKind::Turn).is_empty());
        assert_eq!(
            add_outbound(&dir, "deploys", "https://hooks.example.com/y", &[])
                .unwrap_err(),
            "There's already an outbound webhook named deploys."
        );
        assert_eq!(
            add_outbound(&dir, "bad", "http://hooks.example.com/x", &[]).unwrap_err(),
            "An outbound webhook URL must be a public https address."
        );
        assert!(remove_outbound(&dir, "deploys").unwrap());
        assert!(!remove_outbound(&dir, "deploys").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_kinds_match_every_lifecycle() {
        let dir = scratch("all");
        add_outbound(&dir, "all", "https://hooks.example.com/all", &[]).unwrap();
        assert_eq!(matching(&dir, LifecycleKind::Session).len(), 1);
        assert_eq!(matching(&dir, LifecycleKind::Turn).len(), 1);
        assert_eq!(matching(&dir, LifecycleKind::Tool).len(), 1);
        assert_eq!(
            parse_kinds("session, tool").unwrap(),
            vec![LifecycleKind::Session, LifecycleKind::Tool]
        );
        assert!(parse_kinds("nope").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
