//! Path A: the cabin pre-check in front of every `grokhub-desktop` tool call.
//!
//! Same code on Linux and Windows (D3): `run_stdio` calls [`precheck`] before
//! the server dispatches, so X11, Wayland, and the Windows SendInput tools are
//! all gated by `harness::desk_decide`. The server keeps its own switch, halt,
//! and lock checks. Hard floor ⇒ refuse. Hard class ⇒ park a cabin card and
//! wait (TTL / halt ⇒ Deny). Runs under Always too.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use grokhub_agent::harness::{self as hx, GateOutcome};
use grokhub_agent::AccessMode;
use grokhub_core::desktop_mcp::CallGate;
use serde_json::{json, Value};

/// One `tools/call` line: JSON-RPC id, tool name, arguments.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeskCallLine {
    pub id: Value,
    pub tool: String,
    pub args: Value,
}

pub(crate) fn parse_call(line: &str) -> Option<DeskCallLine> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("method").and_then(|m| m.as_str()) != Some("tools/call") {
        return None;
    }
    let id = v.get("id").cloned().filter(|i| !i.is_null())?;
    let params = v.get("params")?;
    let tool = params.get("name").and_then(|n| n.as_str())?.to_string();
    let args = params
        .get("arguments")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    Some(DeskCallLine { id, tool, args })
}

/// What `run_stdio` does with a call before the server sees it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Precheck {
    /// Not a tool call, or the server's own gates answer it, or soft.
    Pass,
    Refuse(String),
    Park { class: String, reason: String },
}

/// Switch off or halted: the server refuses on its own (existing behavior).
pub(crate) fn precheck(call: &DeskCallLine, gate: CallGate) -> Precheck {
    if !gate.enabled || gate.halted {
        return Precheck::Pass;
    }
    match hx::decide(hx::Step::Desk { tool: &call.tool, args: &call.args }) {
        GateOutcome::Allow => Precheck::Pass,
        GateOutcome::Refuse { reason } => Precheck::Refuse(reason),
        GateOutcome::Park { reason, hard, .. } => Precheck::Park {
            class: hard.map(|c| c.as_str()).unwrap_or("irreversible_os").into(),
            reason,
        },
    }
}

/// MCP tool error the model reads. Same shape as the server's own refusals.
pub(crate) fn refusal_reply(id: &Value, msg: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "content": [{ "type": "text", "text": msg }], "isError": true },
    })
    .to_string()
}

/// `(ok, first text)` from the server's reply.
pub(crate) fn reply_result(reply: &str) -> (bool, String) {
    let v: Value = serde_json::from_str(reply).unwrap_or(Value::Null);
    let r = &v["result"];
    let ok = !r["isError"].as_bool().unwrap_or(true) && v.get("error").is_none();
    let text = r["content"][0]["text"]
        .as_str()
        .or_else(|| v["error"]["message"].as_str())
        .unwrap_or("")
        .to_string();
    (ok, text)
}

pub(crate) fn access_now(config_dir: &Path, enabled: bool) -> AccessMode {
    if !enabled {
        return AccessMode::Readonly;
    }
    match AccessMode::parse(&hx::read_turn_context(config_dir).access) {
        Some(AccessMode::Full) => AccessMode::Full,
        _ => AccessMode::Supervised,
    }
}

/// Post a park for the cabin and wait. True only on Jeremy's Approve.
pub(crate) fn park_and_wait(
    config_dir: &Path,
    call: &DeskCallLine,
    class: &str,
    halted: &mut dyn FnMut() -> bool,
) -> bool {
    static N: AtomicU64 = AtomicU64::new(0);
    let id = format!("{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
    let action = match call.tool.as_str() {
        "type" => call.args["text"].as_str().unwrap_or("").to_string(),
        "key" => call.args["keys"].as_str().unwrap_or("").to_string(),
        _ => hx::desk_args(&call.tool, &call.args),
    };
    let req = hx::ParkRequest {
        id: id.clone(),
        path: "A".into(),
        tool: call.tool.clone(),
        action: hx::redact_args(&action),
        class: class.into(),
        ts_ms: grokhub_core::now_ms(),
    };
    if hx::post_park(config_dir, &req).is_err() {
        return false;
    }
    let ctx = hx::read_turn_context(config_dir);
    let park_span = hx::Span::hard_park(
        if ctx.chat_id.is_empty() { hx::CU_TRACE } else { &ctx.chat_id },
        &call.tool,
        &hx::desk_args(&call.tool, &call.args),
        hx::HardClass::parse(class).unwrap_or(hx::HardClass::IrreversibleOs),
    )
    .on_path("A")
    .in_turn(&ctx.chat_id, ctx.turn);
    let mut park_span = park_span;
    park_span.access = ctx.access.clone();
    let _ = hx::append_span(config_dir, &park_span);
    hx::wait_park(config_dir, &id, hx::APPROVAL_TTL, Duration::from_millis(200), halted)
}

/// Span for a call the pre-check or the server answered.
pub(crate) fn write_span(
    config_dir: &Path,
    call: &DeskCallLine,
    ok: bool,
    result: &str,
    access: AccessMode,
    ui_changed: Option<bool>,
    parked: Option<(hx::HardClass, bool)>,
) {
    let ctx = hx::read_turn_context(config_dir);
    let span = hx::desk_span(
        &hx::DeskCall {
            tool: &call.tool,
            args: &call.args,
            ok,
            result,
            access,
            ui_changed,
            parked,
        },
        &ctx.chat_id,
        ctx.turn,
    );
    if let Some(span) = span {
        let _ = hx::append_span(config_dir, &span);
    }
}

/// Stable hash of a screenshot's bytes, for `ui_changed`.
pub(crate) fn shot_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: CallGate = CallGate {
        enabled: true,
        halted: false,
    };

    fn line(tool: &str, args: Value) -> DeskCallLine {
        let raw = json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": tool, "arguments": args },
        })
        .to_string();
        parse_call(&raw).expect("tools/call")
    }

    #[test]
    fn same_gate_for_every_platform_dispatch() {
        let typed = line("type", json!({ "text": "rm -f disposable.txt" }));
        assert_eq!(
            precheck(&typed, ON),
            Precheck::Park {
                class: "delete".into(),
                reason: "hard-class delete: Delete — Always cannot skip".into(),
            }
        );
        let floor = line("type", json!({ "text": "rm -rf /" }));
        assert_eq!(precheck(&floor, ON), Precheck::Refuse("hard floor: rm -rf /".into()));
        let click = line("click", json!({ "x": 12, "y": 34 }));
        assert_eq!(precheck(&click, ON), Precheck::Pass);
        let off = CallGate {
            enabled: false,
            halted: false,
        };
        assert_eq!(precheck(&floor, off), Precheck::Pass, "the server's switch refusal answers");
        assert_eq!(parse_call(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#), None);
    }

    #[test]
    fn refusal_and_reply_shapes() {
        let r = refusal_reply(&json!(4), "hard floor: rm -rf /");
        assert_eq!(
            r,
            r#"{"id":4,"jsonrpc":"2.0","result":{"content":[{"text":"hard floor: rm -rf /","type":"text"}],"isError":true}}"#
        );
        assert_eq!(reply_result(&r), (false, "hard floor: rm -rf /".into()));
        let ok = r#"{"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"text","text":"clicked"}],"isError":false}}"#;
        assert_eq!(reply_result(ok), (true, "clicked".into()));
        assert_eq!(shot_hash(b"abc"), shot_hash(b"abc"));
        assert_ne!(shot_hash(b"abc"), shot_hash(b"abd"));
    }

    #[test]
    fn park_waits_for_the_cabin_and_halt_denies() {
        let dir = crate::config::test_config_root("desk-park");
        let _ = std::fs::create_dir_all(&dir);
        let typed = line("type", json!({ "text": "rm -f disposable.txt" }));
        assert!(!park_and_wait(&dir, &typed, "delete", &mut || true));
        let spans = hx::read_spans(&dir, hx::CU_TRACE).unwrap();
        assert_eq!(spans[0].decision, "park");
        assert_eq!(spans[0].path, "A");
        assert_eq!(spans[0].args_redacted, r#"{"chars":20}"#);
        assert!(hx::pending_parks(&dir).is_empty());
        assert_eq!(access_now(&dir, false), AccessMode::Readonly);
        assert_eq!(access_now(&dir, true), AccessMode::Supervised);
        let _ = std::fs::remove_dir_all(dir);
    }
}
