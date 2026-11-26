//! Telephony network calls, OS-keychain credentials, and webhook handling
//! (Order 974).
//!
//! API keys and webhook secrets stay in the OS keychain (`keyring_blob`).
//! Outbound calls fail closed on disabled policy, missing per-call user
//! confirmation, Offline only, or egress guard refusal.

use std::path::Path;
use std::time::Duration;

use grokhub_core::http;
use grokhub_core::telephony::{
    self, CallDecision, CallDirection, CallStatus, InboundWebhookEvent, TelephonyConfig,
    TelephonyProvider,
};
use serde_json::{json, Value};

use crate::harness::{guard_quiet, DataClass, EgressReq};
use crate::keyring_blob::{self, Backend};

pub const API_KEY_ACCOUNT: &str = "telephony-api-key";
pub const WEBHOOK_SECRET_ACCOUNT: &str = "telephony-webhook-secret";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const OVERALL_TIMEOUT: Duration = Duration::from_secs(30);

fn keychain_message(err: keyring::Error) -> String {
    format!("the system keychain: {err}")
}

/// System keychain store for telephony credentials.
pub fn os_keychain() -> keyring_blob::OsBackend {
    keyring_blob::OsBackend::new(grokhub_core::brand::INTERNAL_ID, keychain_message)
}

pub fn save_api_key(store: &dyn Backend, key: &str) -> Result<(), String> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("Telephony API key cannot be empty.".into());
    }
    store.set(API_KEY_ACCOUNT, trimmed)
}

pub fn load_api_key(store: &dyn Backend) -> Result<Option<String>, String> {
    store.get(API_KEY_ACCOUNT)
}

pub fn delete_api_key(store: &dyn Backend) -> Result<(), String> {
    store.delete(API_KEY_ACCOUNT)
}

pub fn save_webhook_secret(store: &dyn Backend, secret: &str) -> Result<(), String> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return Err("Telephony webhook secret cannot be empty.".into());
    }
    store.set(WEBHOOK_SECRET_ACCOUNT, trimmed)
}

pub fn load_webhook_secret(store: &dyn Backend) -> Result<Option<String>, String> {
    store.get(WEBHOOK_SECRET_ACCOUNT)
}

pub fn delete_webhook_secret(store: &dyn Backend) -> Result<(), String> {
    store.delete(WEBHOOK_SECRET_ACCOUNT)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedCall {
    pub call_id: String,
    pub provider: TelephonyProvider,
    pub to_number: String,
    pub status: CallStatus,
    pub ai_disclosed: bool,
    pub audio_recorded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookOutcome {
    Accepted {
        event: InboundWebhookEvent,
        transcript_saved: bool,
    },
    Denied {
        reason: String,
    },
}

/// Builds the provider endpoint URL and JSON payload honoring privacy policy.
pub fn build_outbound_request(
    cfg: &TelephonyConfig,
    to_number: &str,
    first_message: Option<&str>,
) -> Result<(String, Value), String> {
    telephony::validate_config(cfg)?;
    let base = cfg.effective_api_base().trim_end_matches('/');
    if base.is_empty() {
        return Err("Telephony API base URL is not configured.".into());
    }
    let opening = match first_message.map(str::trim).filter(|s| !s.is_empty()) {
        Some(msg) if cfg.privacy.announce_ai_disclosure => {
            format!("Automated AI assistant calling: {msg}")
        }
        Some(msg) => msg.to_string(),
        None if cfg.privacy.announce_ai_disclosure => {
            "Hello, this is an automated AI assistant calling on behalf of the user.".to_string()
        }
        None => String::new(),
    };
    let (path, mut payload) = match cfg.provider {
        TelephonyProvider::Vapi => (
            "/call",
            json!({
                "phoneNumberId": cfg.phone_number,
                "assistantId": cfg.assistant_id,
                "customer": { "number": to_number.trim() },
                "recordingEnabled": cfg.privacy.record_audio,
            }),
        ),
        TelephonyProvider::Twilio | TelephonyProvider::CustomWebhook => (
            "/calls",
            json!({
                "from": cfg.phone_number,
                "to": to_number.trim(),
                "record": cfg.privacy.record_audio,
            }),
        ),
    };
    if !opening.is_empty() {
        payload["firstMessage"] = Value::String(opening);
    }
    Ok((format!("{base}{path}"), payload))
}

/// Places an outbound phone call through `post` after consent, keychain,
/// offline, and egress checks pass.
pub fn place_call_with<F>(
    config_dir: &Path,
    store: &dyn Backend,
    cfg: &TelephonyConfig,
    to_number: &str,
    first_message: Option<&str>,
    user_confirmed: bool,
    mut post: F,
) -> Result<PlacedCall, String>
where
    F: FnMut(&str, &str, &Value) -> Result<Value, String>,
{
    match telephony::authorize_outbound(cfg, to_number, user_confirmed) {
        CallDecision::Allow => {}
        CallDecision::NeedsConfirm { reason } | CallDecision::Deny { reason } => {
            return Err(reason);
        }
    }
    let api_key = load_api_key(store)?
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| "No telephony API key is saved in the OS keychain.".to_string())?;
    let (url, payload) = build_outbound_request(cfg, to_number, first_message)?;
    if grokhub_core::offline::refuse().is_err() {
        return Err("Offline only is enabled.".into());
    }
    guard_quiet(config_dir, &EgressReq::new(&url, &[DataClass::Chat]))?;

    let resp = post(&url, &api_key, &payload)?;
    let call_id = resp
        .get("id")
        .or_else(|| resp.get("sid"))
        .and_then(Value::as_str)
        .unwrap_or("call-pending")
        .trim()
        .to_string();

    Ok(PlacedCall {
        call_id,
        provider: cfg.provider,
        to_number: to_number.trim().to_string(),
        status: CallStatus::Queued,
        ai_disclosed: cfg.privacy.announce_ai_disclosure,
        audio_recorded: cfg.privacy.record_audio,
    })
}

/// Places an outbound phone call over HTTPS using `grokhub_core::http::agent`.
pub fn place_call(
    config_dir: &Path,
    store: &dyn Backend,
    cfg: &TelephonyConfig,
    to_number: &str,
    first_message: Option<&str>,
    user_confirmed: bool,
) -> Result<PlacedCall, String> {
    place_call_with(
        config_dir,
        store,
        cfg,
        to_number,
        first_message,
        user_confirmed,
        |url, key, payload| {
            let body = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
            let public_only = grokhub_core::public_http_url(url);
            let agent = http::agent(http::Opts {
                connect: CONNECT_TIMEOUT,
                overall: Some(OVERALL_TIMEOUT),
                redirects: 0,
                public_only,
                ..Default::default()
            });
            let resp = agent
                .post(url)
                .header("Authorization", &format!("Bearer {key}"))
                .header("Content-Type", "application/json")
                .send_bytes(&body)
                .map_err(|e| e.to_string())?;
            let status = resp.status();
            let text = resp.into_string().map_err(|e| e.to_string())?;
            if !(200..300).contains(&status) {
                return Err(format!("Telephony provider returned HTTP {status}: {text}"));
            }
            serde_json::from_str::<Value>(&text)
                .map_err(|e| format!("Invalid telephony provider response JSON: {e}"))
        },
    )
}

/// Authenticates an inbound telephony webhook, enforces inbound consent and
/// caller allowlist, and saves redacted transcripts within retention limits.
pub fn handle_inbound_webhook(
    config_dir: &Path,
    store: &dyn Backend,
    cfg: &TelephonyConfig,
    provided_secret: &str,
    body: &str,
    now_ms: u64,
) -> Result<WebhookOutcome, String> {
    let expected = load_webhook_secret(store)?
        .ok_or_else(|| "No telephony webhook secret is saved in the OS keychain.".to_string())?;
    if !telephony::verify_webhook_secret(&expected, provided_secret) {
        return Err("Telephony webhook signature/secret rejected.".into());
    }
    let event = telephony::parse_inbound_webhook(body, cfg.provider, now_ms)?;
    match &event {
        InboundWebhookEvent::CallStarted { from, .. } => {
            if let CallDecision::Deny { reason } | CallDecision::NeedsConfirm { reason } =
                telephony::authorize_inbound(cfg, from)
            {
                return Ok(WebhookOutcome::Denied { reason });
            }
            Ok(WebhookOutcome::Accepted {
                event,
                transcript_saved: false,
            })
        }
        InboundWebhookEvent::TranscriptTurn { .. } => {
            if !cfg.enabled || !cfg.inbound.allow_inbound {
                return Ok(WebhookOutcome::Denied {
                    reason: "Inbound phone calls are disabled by policy.".into(),
                });
            }
            Ok(WebhookOutcome::Accepted {
                event,
                transcript_saved: false,
            })
        }
        InboundWebhookEvent::EndOfCallReport { transcript } => {
            if transcript.direction == CallDirection::Inbound {
                if let CallDecision::Deny { reason } | CallDecision::NeedsConfirm { reason } =
                    telephony::authorize_inbound(cfg, &transcript.peer_number)
                {
                    return Ok(WebhookOutcome::Denied { reason });
                }
            }
            let saved = telephony::save_transcript(config_dir, &cfg.privacy, transcript, now_ms)?;
            Ok(WebhookOutcome::Accepted {
                event,
                transcript_saved: saved,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::telephony::{CallPrivacyPolicy, InboundPolicy, OutboundPolicy};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemStore(Mutex<HashMap<String, String>>);

    impl Backend for MemStore {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, value: &str) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(account.to_string(), value.to_string());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-telephony-net-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_cfg() -> TelephonyConfig {
        TelephonyConfig {
            enabled: true,
            provider: TelephonyProvider::Vapi,
            phone_number: "+14155550100".into(),
            assistant_id: "asst-test".into(),
            api_base: "http://127.0.0.1:9999".into(),
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
    fn keychain_roundtrip_for_api_key_and_webhook_secret() {
        let store = MemStore::default();
        assert!(save_api_key(&store, "   ").is_err());
        save_api_key(&store, "vapi-secret").unwrap();
        save_webhook_secret(&store, "hook-secret").unwrap();
        assert_eq!(
            load_api_key(&store).unwrap().as_deref(),
            Some("vapi-secret")
        );
        assert_eq!(
            load_webhook_secret(&store).unwrap().as_deref(),
            Some("hook-secret")
        );
        delete_api_key(&store).unwrap();
        delete_webhook_secret(&store).unwrap();
        assert_eq!(load_api_key(&store).unwrap(), None);
    }

    #[test]
    fn outbound_call_requires_confirmation_and_preserves_privacy_flags() {
        let dir = temp_dir("outbound");
        let store = MemStore::default();
        save_api_key(&store, "vapi-key").unwrap();
        let cfg = test_cfg();

        let mut called = 0usize;
        let denied = place_call_with(
            &dir,
            &store,
            &cfg,
            "+14155550222",
            Some("Check status"),
            false,
            |_u, _k, _p| {
                called += 1;
                Ok(json!({ "id": "should-not-run" }))
            },
        );
        assert!(denied.is_err());
        assert_eq!(called, 0);

        let mut seen_payload = Value::Null;
        let placed = place_call_with(
            &dir,
            &store,
            &cfg,
            "+14155550222",
            Some("Check status"),
            true,
            |url, key, payload| {
                called += 1;
                assert_eq!(url, "http://127.0.0.1:9999/call");
                assert_eq!(key, "vapi-key");
                seen_payload = payload.clone();
                Ok(json!({ "id": "call-77" }))
            },
        )
        .unwrap();
        assert_eq!(called, 1);
        assert_eq!(placed.call_id, "call-77");
        assert!(placed.ai_disclosed);
        assert!(!placed.audio_recorded);
        assert_eq!(seen_payload["recordingEnabled"], json!(false));
        assert!(seen_payload["firstMessage"]
            .as_str()
            .is_some_and(|m| m.starts_with("Automated AI assistant")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbound_webhook_verifies_secret_caller_and_saves_redacted_transcript() {
        let dir = temp_dir("inbound");
        let store = MemStore::default();
        save_webhook_secret(&store, "hook-123").unwrap();
        let cfg = test_cfg();
        let body = r#"{
            "message": {
                "type": "end-of-call-report",
                "summary": "Caller +14155550111 asked for support",
                "call": { "id": "call-in-1", "type": "inboundPhoneCall", "customer": { "number": "+14155550111" } },
                "artifact": { "messages": [ { "role": "user", "message": "Email me at user@example.com" } ] }
            }
        }"#;

        assert!(handle_inbound_webhook(&dir, &store, &cfg, "wrong", body, 1_000).is_err());

        let unknown_body = body.replace("+14155550111", "+14155550999");
        let denied =
            handle_inbound_webhook(&dir, &store, &cfg, "hook-123", &unknown_body, 1_000).unwrap();
        assert!(matches!(denied, WebhookOutcome::Denied { .. }));
        assert!(telephony::list_transcripts(&dir).is_empty());

        let accepted = handle_inbound_webhook(&dir, &store, &cfg, "hook-123", body, 1_000).unwrap();
        assert!(matches!(
            accepted,
            WebhookOutcome::Accepted {
                transcript_saved: true,
                ..
            }
        ));
        let saved = telephony::list_transcripts(&dir);
        assert_eq!(saved.len(), 1);
        assert!(saved[0].summary.contains("[REDACTED-PHONE]"));
        assert!(saved[0].entries[0].text.contains("[REDACTED-EMAIL]"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
