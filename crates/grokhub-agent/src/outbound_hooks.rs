//! Deliver outbound signed lifecycle webhooks.
//!
//! Config and signing live in `grokhub_core::outbound_webhooks`. This module
//! POSTs matching endpoints after Offline only, public-URL, and quiet egress
//! checks. Failures are logged and never raise into the agent loop.

use std::path::Path;
use std::time::Duration;

use grokhub_core::http;
use grokhub_core::outbound_webhooks::{matching, prepare_post, LifecycleKind, OutboundHook};
use grokhub_core::{now_ms, public_http_url};
use serde_json::Value;

use crate::harness::{guard_quiet, EgressReq};

const CONNECT: Duration = Duration::from_secs(5);
const OVERALL: Duration = Duration::from_secs(10);

/// Fire every matching outbound hook for one lifecycle event. Best-effort.
pub fn emit(config_dir: &Path, kind: LifecycleKind, event: &str, session: &str, detail: &Value) {
    let hooks = matching(config_dir, kind);
    if hooks.is_empty() {
        return;
    }
    let now = now_ms();
    for hook in hooks {
        if let Err(err) = post_one(config_dir, &hook, kind, event, session, detail, now) {
            eprintln!("outbound webhook {}: {err}", hook.name);
        }
    }
}

fn post_one(
    config_dir: &Path,
    hook: &OutboundHook,
    kind: LifecycleKind,
    event: &str,
    session: &str,
    detail: &Value,
    now_ms: u64,
) -> Result<(), String> {
    if grokhub_core::offline::refuse().is_err() {
        return Err("offline only".into());
    }
    if !public_http_url(&hook.url) || !hook.url.to_ascii_lowercase().starts_with("https://") {
        return Err("url is not a public https address".into());
    }
    guard_quiet(config_dir, &EgressReq::new(&hook.url, &[]))?;
    let (url, headers, body) = prepare_post(hook, kind, event, session, detail, now_ms)?;
    let agent = http::agent(http::Opts {
        connect: CONNECT,
        overall: Some(OVERALL),
        redirects: 0,
        public_only: true,
        ..Default::default()
    });
    let mut req = agent.post(&url);
    for (key, value) in &headers {
        req = req.header(key, value);
    }
    let resp = req.send_bytes(&body).map_err(|e| e.to_string())?;
    let status = resp.status();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    Ok(())
}

/// Convenience: session lifecycle.
pub fn emit_session(config_dir: &Path, event: &str, session: &str, detail: Value) {
    emit(config_dir, LifecycleKind::Session, event, session, &detail);
}

/// Convenience: turn lifecycle.
pub fn emit_turn(config_dir: &Path, event: &str, session: &str, detail: Value) {
    emit(config_dir, LifecycleKind::Turn, event, session, &detail);
}

/// Convenience: tool lifecycle.
pub fn emit_tool(config_dir: &Path, event: &str, session: &str, detail: Value) {
    emit(config_dir, LifecycleKind::Tool, event, session, &detail);
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::outbound_webhooks::{add_outbound, remove_outbound};
    use serde_json::json;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-out-deliv-{tag}-{}-{}",
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
    fn emit_with_no_hooks_is_a_no_op() {
        let dir = scratch("empty");
        emit(
            &dir,
            LifecycleKind::Session,
            "SessionStart",
            "s1",
            &json!({}),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prepare_then_emit_with_no_network_still_returns() {
        let dir = scratch("offline");
        let hook = add_outbound(
            &dir,
            "audit",
            "https://hooks.example.com/life",
            &[LifecycleKind::Session],
        )
        .unwrap();
        // Signing stays local; emit is best-effort and must not panic when the
        // host is unreachable or Offline only refuses.
        let prepared = grokhub_core::outbound_webhooks::prepare_post(
            &hook,
            LifecycleKind::Session,
            "SessionStart",
            "s1",
            &json!({"source": "startup"}),
            grokhub_core::now_ms(),
        )
        .unwrap();
        assert!(prepared.0.starts_with("https://"));
        emit_session(&dir, "SessionStart", "s1", json!({"source": "startup"}));
        assert!(remove_outbound(&dir, "audit").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
