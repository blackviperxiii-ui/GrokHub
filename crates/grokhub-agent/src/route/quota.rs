//! Quota detection: the wire half of `grokhub_core::model_registry::quota`.
//!
//! [`run_quota_checks`] runs inside the registry refresh: each route that
//! is due (never checked, an error stamped a re-read, or the last check
//! aged past ten minutes) is re-read from its own account endpoint —
//! OpenRouter's `/api/v1/key`, xAI's `/v1/api-key` — and the parsed state
//! lands in `models/quota.json`. Anthropic has no account endpoint for
//! normal keys, so its section is fed by the note functions below, which
//! `providers::call_model` calls with every reply in hand: rate-limit
//! headers and usage on a success, the bench rules on a failure. Native
//! xAI failures reach [`note_native_error`] from the route log, so a 402,
//! 403 or 429 anywhere re-reads that route at the next refresh instead of
//! waiting out the ten minutes.
//!
//! Detection is silent and automatic: no settings, no cards, no spans
//! with content. A failed check keeps the last known state.

use std::path::Path;
use std::sync::Mutex;

use grokhub_core::model_registry::quota::{
    anthropic_call_ticks, parse_openrouter_key, parse_xai_api_key, QuotaLedger,
};
use grokhub_core::model_registry::store::{load_quota, save_quota};
use grokhub_core::model_registry::Prices;

use crate::client::Usage;

use super::providers::{self, Provider, ProviderKind};
use super::sources;

/// Run every due route's check and persist what changed. Called by the
/// registry refresh with the sign-in's bearer (a key or plan token; the
/// xAI check is skipped without one). Never fails the refresh.
/// One writer at a time for `quota.json`. Chats, helpers and background
/// runs note replies at once; a read-modify-write without this lets one
/// note overwrite another's, and both share the save's temp file.
static WRITER: Mutex<()> = Mutex::new(());

/// Load, change and save the ledger as one step. Returns the saved ledger.
fn update(config_dir: &Path, change: impl FnOnce(&mut QuotaLedger) -> bool) -> QuotaLedger {
    let _one = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut ledger = load_quota(config_dir);
    if change(&mut ledger) {
        let _ = save_quota(config_dir, &ledger);
    }
    ledger
}

pub fn run_quota_checks(config_dir: &Path, xai_bearer: Option<&str>, now_ms: u64) -> QuotaLedger {
    // The account reads go out without the writer held, so a slow endpoint
    // never holds up a reply's note; what they found is applied under it.
    let before = load_quota(config_dir);
    let mut openrouter = None;
    if before.openrouter_due(now_ms) {
        for p in providers::load_providers(config_dir) {
            if !p.is_openrouter() || !providers::has_key(config_dir, &p.id) {
                continue;
            }
            if let Ok(v) = providers::fetch_openrouter_key(config_dir, &p) {
                openrouter = Some(parse_openrouter_key(&v, now_ms));
            }
        }
    }
    let mut xai = None;
    if before.xai_due(now_ms) {
        if let Some(bearer) = xai_bearer.filter(|b| !b.trim().is_empty()) {
            if let Ok(v) = sources::fetch_xai_api_key(config_dir, bearer) {
                xai = Some(parse_xai_api_key(&v, now_ms));
            }
        }
    }
    update(config_dir, |ledger| {
        let changed = openrouter.is_some() || xai.is_some();
        if let Some(read) = openrouter {
            ledger.openrouter = read;
        }
        if let Some(read) = xai {
            ledger.xai = read;
        }
        changed
    })
}

/// A provider call failed. OpenRouter account errors stamp a re-read
/// (and a free model's 402 stands the account Negative); Anthropic's
/// spend-limit and out-of-credit answers stamp their bench. Other
/// statuses are the health fold's business, not the ledger's.
pub fn note_provider_error(
    config_dir: &Path,
    p: &Provider,
    model: &str,
    status: u16,
    body: &str,
    headers: &[(String, String)],
    now_ms: u64,
) {
    if !matches!(status, 400 | 402 | 403 | 429) {
        return;
    }
    if !p.is_openrouter() && p.kind != ProviderKind::Anthropic {
        return;
    }
    update(config_dir, |ledger| {
        if p.is_openrouter() {
            ledger.note_openrouter_error(providers::is_free_model(model), status, now_ms);
        } else {
            let retry_after = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
                .map(|(_, v)| v.as_str());
            ledger.note_anthropic_error(status, body, retry_after, now_ms);
        }
        true
    });
}

/// An Anthropic call answered: keep its rate-limit headers and fold its
/// usage into the spend ledger, priced from the registry's table when
/// the model has one.
pub fn note_anthropic_reply(
    config_dir: &Path,
    prices: Option<&Prices>,
    usage: &Usage,
    cache_write_tokens: u64,
    headers: &[(String, String)],
    now_ms: u64,
) {
    let ticks = prices.and_then(|p| {
        anthropic_call_ticks(
            p,
            usage.input_tokens,
            usage.output_tokens,
            usage.cached_tokens,
            cache_write_tokens,
        )
    });
    update(config_dir, |ledger| {
        ledger.note_anthropic_reply(
            headers,
            ticks,
            usage
                .input_tokens
                .saturating_add(usage.cached_tokens)
                .saturating_add(cache_write_tokens),
            usage.output_tokens,
            now_ms,
        );
        true
    });
}

/// A native (non-provider) call failed with an account-shaped status:
/// the xAI route re-reads its key state at the next refresh. Provider
/// models are skipped — `providers::call_model` already noted those.
pub fn note_native_error(config_dir: &Path, model: &str, status: u16, now_ms: u64) {
    if !matches!(status, 402 | 403 | 429) {
        return;
    }
    if let Some((pid, _)) = grokhub_core::model_registry::split_provider_model(model) {
        if providers::provider(config_dir, pid).is_some() {
            return;
        }
    }
    update(config_dir, |ledger| {
        ledger.note_xai_error(status, now_ms);
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use grokhub_core::model_registry::quota::{AnthropicBench, OpenRouterState};
    use serde_json::json;

    use crate::harness::{grant_destination, test_dir, UserClick};
    use crate::route::providers::{
        add_provider, use_transport_for, use_vault_for, HttpReply, MemoryVault, ProviderTransport,
        CALL_DATA,
    };

    const NOON: u64 = 1_794_052_800_000;
    /// An obvious fake, never a real key.
    const KEY: &str = "<redacted>";

    /// Answers `/key` with a free-tier payload and counts its GETs.
    struct KeyWire {
        gets: std::sync::Mutex<u32>,
        body: String,
    }

    impl ProviderTransport for KeyWire {
        fn get(&self, url: &str, _headers: &[(String, String)]) -> Result<HttpReply, String> {
            *self.gets.lock().unwrap() += 1;
            assert!(url.ends_with("/key"), "unexpected GET {url}");
            Ok(HttpReply {
                status: 200,
                body: self.body.clone(),
                headers: Vec::new(),
            })
        }

        fn post(
            &self,
            _url: &str,
            _headers: &[(String, String)],
            _body: &str,
        ) -> Result<HttpReply, String> {
            Err("no posts in this test".into())
        }
    }

    fn openrouter_setup(label: &str) -> (std::path::PathBuf, Arc<KeyWire>) {
        let dir = test_dir(label);
        use_vault_for(&dir, Arc::new(MemoryVault::default()));
        add_provider(&dir, "https://openrouter.ai/api/v1", KEY, NOON).unwrap();
        // The /key read goes through the destination grant, like the listing.
        grant_destination(&dir, "openrouter.ai", CALL_DATA, UserClick::from_click()).unwrap();
        let wire = Arc::new(KeyWire {
            gets: std::sync::Mutex::new(0),
            body: json!({"data": {
                "is_free_tier": true,
                "free_model_daily_requests": {"used": 5, "limit": 50, "remaining": 45}
            }})
            .to_string(),
        });
        use_transport_for(&dir, wire.clone());
        (dir, wire)
    }

    #[test]
    fn checks_poll_the_key_once_then_wait_out_the_ten_minutes() {
        let (dir, wire) = openrouter_setup("poll");
        let ledger = run_quota_checks(&dir, None, NOON);
        assert_eq!(ledger.openrouter_state(), OpenRouterState::FreeOnly);
        assert_eq!(ledger.openrouter_free_remaining(), Some(45));
        assert_eq!(*wire.gets.lock().unwrap(), 1);
        // The saved ledger carries the same state.
        assert_eq!(
            load_quota(&dir).openrouter_state(),
            OpenRouterState::FreeOnly
        );
        // Nine minutes later: still fresh, no second read.
        let _ = run_quota_checks(&dir, None, NOON + 9 * 60 * 1000);
        assert_eq!(*wire.gets.lock().unwrap(), 1);
        // An error stamps a re-read, and the next run takes it at once.
        let p = providers::load_providers(&dir).remove(0);
        note_provider_error(
            &dir,
            &p,
            "openrouter/vendor/cheap",
            429,
            "{}",
            &[],
            NOON + 1,
        );
        let _ = run_quota_checks(&dir, None, NOON + 60 * 1000);
        assert_eq!(*wire.gets.lock().unwrap(), 2);
        // A native failure stamps the xAI re-read the same way.
        note_native_error(&dir, "grok-4.7", 429, NOON + 2);
        assert_eq!(load_quota(&dir).xai.recheck_at_ms, NOON + 2);
    }

    #[test]
    fn replies_noted_at_once_from_many_chats_all_count() {
        let dir = test_dir("quota-writers");
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        note_anthropic_reply(&dir, None, &Usage::default(), 0, &[], NOON);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(load_quota(&dir).anthropic.spend.day_requests, 80);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn provider_error_notes_openrouter_and_anthropic() {
        let dir = test_dir("notes");
        let p = Provider {
            id: "openrouter".into(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: "https://openrouter.ai/api/v1".into(),
            added_at: 0,
        };
        // Seed a FreeOnly read, then a free model's 402 stands it Negative.
        let seed = QuotaLedger {
            openrouter: parse_openrouter_key(&json!({"data": {"is_free_tier": true}}), NOON),
            ..Default::default()
        };
        save_quota(&dir, &seed).unwrap();
        note_provider_error(
            &dir,
            &p,
            "openrouter/nvidia/nemotron:free",
            402,
            "{}",
            &[],
            NOON + 1,
        );
        assert_eq!(
            load_quota(&dir).openrouter_state(),
            OpenRouterState::Negative
        );

        // Anthropic out of credit benches the route.
        let anthropic_p = Provider {
            id: "anthropic".into(),
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            added_at: 0,
        };
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API."}}"#;
        note_provider_error(
            &dir,
            &anthropic_p,
            "anthropic/claude-x",
            400,
            body,
            &[],
            NOON,
        );
        let ledger = load_quota(&dir);
        assert_eq!(ledger.anthropic.bench, AnthropicBench::OutOfCredit);
        assert!(ledger.anthropic_benched(NOON + 1));
    }

    #[test]
    fn anthropic_reply_notes_headers_and_spend() {
        let dir = test_dir("reply");
        let prices = Prices {
            prompt: Some(300),
            cached: Some(30),
            completion: Some(1500),
            ..Default::default()
        };
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cached_tokens: 1_000_000,
            ..Usage::default()
        };
        let headers = vec![(
            "anthropic-ratelimit-requests-remaining".to_string(),
            "42".to_string(),
        )];
        note_anthropic_reply(&dir, Some(&prices), &usage, 1_000_000, &headers, NOON);
        let ledger = load_quota(&dir);
        assert_eq!(ledger.anthropic.rate.requests.remaining, Some(42));
        // Day input counts fresh + cache read + cache write tokens.
        assert_eq!(ledger.anthropic.spend.day_input_tokens, 3_000_000);
        assert_eq!(
            ledger.anthropic.spend.day_ticks,
            1_000_000 * 300 + 1_000_000 * 30 + 1_000_000 * 300 + 1_000_000 * 1500
        );
    }
}
