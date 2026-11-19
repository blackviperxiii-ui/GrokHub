//! Messaging channels, part 1: the trust rules every channel shares, before
//! any adapter talks to a network.
//!
//! - [`DmTrust`]: a direct message from a sender nobody paired is never
//!   handed to a model. The sender gets a pairing code; the user approves
//!   that code in the app, and only then does the sender count as trusted.
//! - [`verify_webhook`]: an inbound webhook must carry an HMAC-SHA256
//!   signature over `"{timestamp}.{body}"` from a fresh timestamp.
//! - [`Filter`]: declarative pre-filters drop routine webhook noise before
//!   any model is called.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::pair::{ct_eq, make_pair_code, normalize_code};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    Telegram,
    Discord,
}

/// How long a pairing code stays good.
pub const PAIR_CODE_TTL_MS: u64 = 60 * 60 * 1000;
/// Codes waiting per channel; more unknown senders are told to wait.
pub const MAX_PENDING: usize = 3;
/// Wrong codes typed in the app before every waiting code is dropped.
pub const MAX_WRONG: u32 = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub channel: ChannelKind,
    /// The channel's own id for the sender (a Telegram chat id, a Discord user id).
    pub sender: String,
    /// What the channel calls them, for the approval row only.
    pub label: String,
    pub code: String,
    pub expires_ms: u64,
}

/// Paired senders per channel, and the codes waiting for the user.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DmTrust {
    pub paired: BTreeMap<ChannelKind, BTreeSet<String>>,
    pub pending: Vec<Pending>,
    pub wrong: u32,
}

/// What to do with one inbound direct message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inbound {
    /// A paired sender: hand the message on.
    Trusted,
    /// Reply with this code and drop the message. `fresh` is false when the
    /// sender already had a live code, so a channel can avoid repeating it.
    Pair { code: String, fresh: bool },
    /// Too many senders are waiting: drop the message, say nothing new.
    Busy,
}

impl DmTrust {
    pub fn is_paired(&self, channel: ChannelKind, sender: &str) -> bool {
        self.paired
            .get(&channel)
            .is_some_and(|set| set.contains(sender))
    }

    fn sweep(&mut self, now_ms: u64) {
        self.pending.retain(|p| p.expires_ms > now_ms);
    }

    /// A direct message from `sender` on `channel` arrived.
    pub fn inbound(
        &mut self,
        channel: ChannelKind,
        sender: &str,
        label: &str,
        now_ms: u64,
    ) -> Inbound {
        if self.is_paired(channel, sender) {
            return Inbound::Trusted;
        }
        self.sweep(now_ms);
        if let Some(p) = self
            .pending
            .iter()
            .find(|p| p.channel == channel && p.sender == sender)
        {
            return Inbound::Pair {
                code: p.code.clone(),
                fresh: false,
            };
        }
        if self.pending.iter().filter(|p| p.channel == channel).count() >= MAX_PENDING {
            return Inbound::Busy;
        }
        let code = make_pair_code();
        self.pending.push(Pending {
            channel,
            sender: sender.to_string(),
            label: label.chars().take(64).collect(),
            code: code.clone(),
            expires_ms: now_ms + PAIR_CODE_TTL_MS,
        });
        Inbound::Pair { code, fresh: true }
    }

    /// The user typed `code` in the app. The sender it belongs to is paired.
    pub fn approve(&mut self, code: &str, now_ms: u64) -> Result<Pending, String> {
        self.sweep(now_ms);
        let typed = normalize_code(code);
        let hit = self
            .pending
            .iter()
            .position(|p| ct_eq(normalize_code(&p.code).as_bytes(), typed.as_bytes()));
        let Some(i) = hit else {
            self.wrong += 1;
            if self.wrong >= MAX_WRONG {
                self.pending.clear();
                self.wrong = 0;
                return Err("Too many wrong codes, so every waiting code was dropped. Ask the sender to message again.".into());
            }
            return Err("That code doesn't match anyone waiting. Codes last an hour.".into());
        };
        self.wrong = 0;
        let p = self.pending.remove(i);
        self.paired
            .entry(p.channel)
            .or_default()
            .insert(p.sender.clone());
        Ok(p)
    }

    /// Unpairs `sender`. True when they were paired.
    pub fn revoke(&mut self, channel: ChannelKind, sender: &str) -> bool {
        self.paired
            .get_mut(&channel)
            .is_some_and(|set| set.remove(sender))
    }

    pub fn path(config_dir: &Path) -> PathBuf {
        config_dir.join("channels").join("trust.json")
    }

    pub fn load(config_dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(config_dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::pulse_writer::write_atomic(&Self::path(config_dir), &text)
    }
}

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| k.map(|b| b ^ byte);
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(msg)
        .finalize();
    Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

/// How far a webhook's timestamp may be from now.
pub const WEBHOOK_SKEW_MS: u64 = 5 * 60 * 1000;

/// Checks an inbound webhook: `signature` is `sha256=<hex>` of
/// HMAC-SHA256(`secret`, `"{timestamp}.{body}"`), and `timestamp` (Unix
/// seconds) is within [`WEBHOOK_SKEW_MS`] of `now_ms`, so an old request
/// can't be replayed.
pub fn verify_webhook(
    secret: &[u8],
    body: &[u8],
    signature: &str,
    timestamp: &str,
    now_ms: u64,
) -> Result<(), String> {
    if secret.len() < 16 {
        return Err("the webhook secret is too short".into());
    }
    let secs: u64 = timestamp
        .trim()
        .parse()
        .map_err(|_| "the webhook timestamp isn't a number".to_string())?;
    let sent_ms = secs.saturating_mul(1000);
    if sent_ms.abs_diff(now_ms) > WEBHOOK_SKEW_MS {
        return Err("the webhook timestamp is too old or too far ahead".into());
    }
    let given = signature
        .trim()
        .strip_prefix("sha256=")
        .and_then(|h| hex::decode(h).ok())
        .ok_or_else(|| "the webhook signature isn't sha256=<hex>".to_string())?;
    let mut signed = format!("{}.", timestamp.trim()).into_bytes();
    signed.extend_from_slice(body);
    if !ct_eq(&hmac_sha256(secret, &signed), &given) {
        return Err("the webhook signature doesn't match".into());
    }
    Ok(())
}

/// One declarative check on a webhook's JSON body. `path` is dot-separated
/// (`pull_request.state`); array items are numbers (`commits.0.id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub path: String,
    #[serde(flatten)]
    pub test: Test,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "op", content = "value")]
pub enum Test {
    Exists,
    Equals(String),
    Contains(String),
    OneOf(Vec<String>),
}

fn at<'a>(body: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .filter(|s| !s.is_empty())
        .try_fold(body, |v, key| match v {
            Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => v.get(key),
        })
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl Filter {
    pub fn passes(&self, body: &Value) -> bool {
        let Some(v) = at(body, &self.path) else {
            return false;
        };
        match &self.test {
            Test::Exists => !v.is_null(),
            Test::Equals(want) => text_of(v) == *want,
            Test::Contains(part) => text_of(v).contains(part.as_str()),
            Test::OneOf(list) => list.contains(&text_of(v)),
        }
    }
}

/// Every filter passes (no filters: everything passes).
pub fn keep(filters: &[Filter], body: &Value) -> bool {
    filters.iter().all(|f| f.passes(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hmac_matches_rfc_4231() {
        // Test case 2.
        assert_eq!(
            hex::encode(hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 6: a key longer than the block.
        assert_eq!(
            hex::encode(hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    const SECRET: &[u8] = b"whsec-abcdefghijklmnopqrstuv";
    const NOW: u64 = 1_763_550_000_000;

    fn sign(ts: &str, body: &[u8]) -> String {
        let mut signed = format!("{ts}.").into_bytes();
        signed.extend_from_slice(body);
        format!("sha256={}", hex::encode(hmac_sha256(SECRET, &signed)))
    }

    #[test]
    fn a_signed_fresh_webhook_passes_and_anything_else_is_refused() {
        let body = br#"{"action":"opened"}"#;
        let ts = "1763550000";
        assert_eq!(
            verify_webhook(SECRET, body, &sign(ts, body), ts, NOW),
            Ok(())
        );
        assert_eq!(
            verify_webhook(SECRET, br#"{"action":"closed"}"#, &sign(ts, body), ts, NOW),
            Err("the webhook signature doesn't match".into())
        );
        assert_eq!(
            verify_webhook(b"whsec-other-secret-xyz", body, &sign(ts, body), ts, NOW),
            Err("the webhook signature doesn't match".into())
        );
        // A replay ten minutes later.
        assert_eq!(
            verify_webhook(SECRET, body, &sign(ts, body), ts, NOW + 10 * 60 * 1000),
            Err("the webhook timestamp is too old or too far ahead".into())
        );
        // The timestamp is signed too: a fresh one can't be swapped in.
        assert_eq!(
            verify_webhook(SECRET, body, &sign(ts, body), "1763550100", NOW + 100_000),
            Err("the webhook signature doesn't match".into())
        );
        assert_eq!(
            verify_webhook(SECRET, body, "md5=abc", ts, NOW),
            Err("the webhook signature isn't sha256=<hex>".into())
        );
        assert_eq!(
            verify_webhook(b"short", body, &sign(ts, body), ts, NOW),
            Err("the webhook secret is too short".into())
        );
    }

    #[test]
    fn filters_drop_noise_before_any_model() {
        let push = json!({"ref": "refs/heads/main", "commits": [{"id": "abc"}], "sender": {"type": "Bot"}});
        let main_only = Filter {
            path: "ref".into(),
            test: Test::Equals("refs/heads/main".into()),
        };
        let humans = Filter {
            path: "sender.type".into(),
            test: Test::OneOf(vec!["User".into()]),
        };
        let has_commit = Filter {
            path: "commits.0.id".into(),
            test: Test::Exists,
        };
        assert!(keep(&[main_only.clone(), has_commit.clone()], &push));
        assert!(!keep(&[main_only.clone(), humans], &push));
        assert!(!keep(
            &[Filter {
                path: "missing.path".into(),
                test: Test::Exists
            }],
            &push
        ));
        assert!(keep(&[], &push));
        // The stored form people write.
        let parsed: Filter =
            serde_json::from_value(json!({"path": "ref", "op": "contains", "value": "main"}))
                .unwrap();
        assert_eq!(
            parsed,
            Filter {
                path: "ref".into(),
                test: Test::Contains("main".into())
            }
        );
        assert!(parsed.passes(&push));
    }

    #[test]
    fn an_unpaired_sender_gets_a_code_and_only_the_users_ok_pairs_them() {
        let mut trust = DmTrust::default();
        let Inbound::Pair { code, fresh: true } =
            trust.inbound(ChannelKind::Telegram, "1001", "user", NOW)
        else {
            panic!("a new sender gets a code");
        };
        assert_eq!(
            trust.inbound(ChannelKind::Telegram, "1001", "user", NOW + 1),
            Inbound::Pair {
                code: code.clone(),
                fresh: false
            }
        );
        // The same id on another channel is someone else.
        assert!(!trust.is_paired(ChannelKind::Discord, "1001"));
        assert_eq!(
            trust.approve("ZZZ-ZZZ", NOW + 2),
            Err("That code doesn't match anyone waiting. Codes last an hour.".into())
        );
        let paired = trust
            .approve(&code.to_lowercase().replace('-', " "), NOW + 3)
            .unwrap();
        assert_eq!(
            (paired.channel, paired.sender.as_str()),
            (ChannelKind::Telegram, "1001")
        );
        assert_eq!(
            trust.inbound(ChannelKind::Telegram, "1001", "user", NOW + 4),
            Inbound::Trusted
        );
        assert!(trust.pending.is_empty());
        assert!(trust.revoke(ChannelKind::Telegram, "1001"));
        assert!(!trust.is_paired(ChannelKind::Telegram, "1001"));
    }

    #[test]
    fn codes_expire_waiting_senders_are_capped_and_guessing_burns_them() {
        let mut trust = DmTrust::default();
        for id in ["1", "2", "3"] {
            assert!(matches!(
                trust.inbound(ChannelKind::Discord, id, "", NOW),
                Inbound::Pair { fresh: true, .. }
            ));
        }
        assert_eq!(
            trust.inbound(ChannelKind::Discord, "4", "", NOW),
            Inbound::Busy
        );
        // Another channel has its own room.
        assert!(matches!(
            trust.inbound(ChannelKind::Telegram, "4", "", NOW),
            Inbound::Pair { .. }
        ));
        // An hour on, the old codes are gone and a new sender gets in.
        let later = NOW + PAIR_CODE_TTL_MS;
        assert!(matches!(
            trust.inbound(ChannelKind::Discord, "4", "", later),
            Inbound::Pair { fresh: true, .. }
        ));
        assert_eq!(trust.pending.len(), 1);
        for _ in 0..MAX_WRONG - 1 {
            assert!(trust.approve("AAA-AAA", later).is_err());
        }
        assert_eq!(
            trust.approve("AAA-AAA", later),
            Err("Too many wrong codes, so every waiting code was dropped. Ask the sender to message again.".into())
        );
        assert!(trust.pending.is_empty());
    }

    #[test]
    fn the_trust_store_round_trips() {
        let dir = std::env::temp_dir().join(format!("grokhub-dmtrust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(DmTrust::load(&dir), DmTrust::default());
        let mut trust = DmTrust::default();
        trust
            .paired
            .entry(ChannelKind::Discord)
            .or_default()
            .insert("42".into());
        trust.save(&dir).unwrap();
        assert_eq!(DmTrust::load(&dir), trust);
        let text = std::fs::read_to_string(DmTrust::path(&dir)).unwrap();
        assert!(text.contains("\"discord\""), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
