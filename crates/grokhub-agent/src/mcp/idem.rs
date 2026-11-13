//! Idempotency keys for MCP calls that can change things (round 2 item 24).
//!
//! A tool the server doesn't mark read-only or idempotent gets a key in
//! `params._meta["idempotency-key"]`. When a call ends with no answer (a
//! timeout, a dropped connection), its key is kept for [`WINDOW`]: the same
//! call sent again in that time carries the same key, so a server or proxy
//! that honors keys runs it once (arXiv 2608.02645). An answered call frees
//! its key, so asking for the same thing again on purpose is a new call. The
//! MCP spec names no key field yet; `idempotency-key` is the one the
//! deduplicating proxies read, and servers that don't know it ignore `_meta`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::rpc::ToolHints;

/// How long a key waits for its resend.
pub(crate) const WINDOW: Duration = Duration::from_secs(600);

/// Added to a keyed call's error when the server may have run it anyway.
pub(crate) const MAYBE_RAN: &str = "\nThe server may have run this before the answer was lost. Check whether it took effect before you send it again; a resend carries the same idempotency key.";

struct Open {
    key: String,
    at: Instant,
}

fn open() -> &'static Mutex<HashMap<String, Open>> {
    static OPEN: OnceLock<Mutex<HashMap<String, Open>>> = OnceLock::new();
    OPEN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Read-only and idempotent tools need no key.
pub(crate) fn needs_key(hints: &ToolHints) -> bool {
    !hints.read_only && hints.idempotent != Some(true)
}

/// One call's identity: server, tool and arguments with their keys sorted.
pub(crate) fn fingerprint(server: &str, tool: &str, args: &Value) -> String {
    let mut h = Sha256::new();
    h.update(server.as_bytes());
    h.update([0]);
    h.update(tool.as_bytes());
    h.update([0]);
    h.update(canonical(args).as_bytes());
    hex(&h.finalize())
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), canonical(&map[k])))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(items) => {
            format!(
                "[{}]",
                items.iter().map(canonical).collect::<Vec<_>>().join(",")
            )
        }
        other => other.to_string(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The key for a call about to go out: the one its unanswered try used, or a
/// new one.
pub(crate) fn begin(print: &str) -> String {
    let mut open = open().lock().unwrap_or_else(|e| e.into_inner());
    open.retain(|_, o| o.at.elapsed() < WINDOW);
    if let Some(o) = open.get_mut(print) {
        o.at = Instant::now();
        return o.key.clone();
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut h = Sha256::new();
    h.update(print.as_bytes());
    h.update(std::process::id().to_le_bytes());
    h.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    h.update(
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_le_bytes(),
    );
    let key = hex(&h.finalize()[..16]);
    open.insert(
        print.to_string(),
        Open {
            key: key.clone(),
            at: Instant::now(),
        },
    );
    key
}

/// After the call: an answer (even an error result) frees the key; no answer
/// keeps it for the resend.
pub(crate) fn end(print: &str, answered: bool) {
    if answered {
        open()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(print);
    }
}

/// An error after which the call may still have run on the server.
pub(crate) fn maybe_ran(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    ["timed out", "timeout", "stopped", "reset", "closed", "broken pipe", "eof"]
        .iter()
        .any(|w| e.contains(w))
        // A 4xx is the server's own answer: it refused, it didn't run.
        && !e.contains("http 4")
        && !super::http::is_unauthorized(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_resend_after_no_answer_keeps_its_key_and_an_answer_frees_it() {
        let send = fingerprint(
            "chat",
            "send_message",
            &json!({"to": "#ops", "text": "deploy done"}),
        );
        // The same call with its keys in another order is the same call.
        assert_eq!(
            send,
            fingerprint(
                "chat",
                "send_message",
                &json!({"text": "deploy done", "to": "#ops"})
            )
        );
        assert_ne!(
            send,
            fingerprint(
                "chat",
                "send_message",
                &json!({"to": "#ops", "text": "deploy"})
            )
        );

        let first = begin(&send);
        end(&send, false); // timed out
        let again = begin(&send);
        assert_eq!(first, again, "the resend reuses the key");
        end(&send, true); // answered
        let fresh = begin(&send);
        assert_ne!(again, fresh, "asking again on purpose is a new call");
        end(&send, true);
        assert_eq!(first.len(), 32);
    }

    #[test]
    fn only_calls_that_can_change_things_get_a_key() {
        assert!(needs_key(&ToolHints::default()));
        assert!(!needs_key(&ToolHints {
            read_only: true,
            ..ToolHints::default()
        }));
        assert!(!needs_key(&ToolHints {
            idempotent: Some(true),
            ..ToolHints::default()
        }));
        assert!(maybe_ran("MCP server `chat` timed out"));
        assert!(!maybe_ran("MCP server `chat` stopped: HTTP 401: sign in"));
        assert!(!maybe_ran(
            "MCP server `chat` sent a result of type partial"
        ));
        assert!(!maybe_ran("MCP server `box` stopped: HTTP 404: not here"));
        assert!(maybe_ran("MCP server `box` stopped: HTTP 502: bad gateway"));
    }
}
