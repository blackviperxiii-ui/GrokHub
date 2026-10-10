//! Headless `grokhub --mcp-self` stdio server (Spike-5c). Grok Build sees it
//! as `grokhub-self`: Grok's own tools for its skills, connections, and
//! automations. Stdout is JSON-RPC only. Logs go to stderr.
//!
//! Path A: every `tools/call` asks `harness::decide` first, on every OS and
//! under Always too. Soft calls run (Grok Build's own pill already asked).
//! Delete and credentials park a hard card through the same park files as
//! `--mcp-desktop` and wait (TTL, halt, or a closed cabin ⇒ Deny). A target
//! under harness policy, consent, egress, or Access is refused with a finding
//! before any ledger is touched. A secret comes only from the user's elicit
//! card; the model sees `%secret%`.

use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use grokhub_agent::harness::{self as hx, GateOutcome, HardClass, Origin};
use grokhub_agent::self_manage::{self as sm, SELF_MCP_SERVER};
use serde_json::{json, Value};

/// Span trace when no chat is open.
pub(crate) const SELF_TRACE: &str = "grokhub-self";
const PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const LINE_CAP: usize = 1 << 20;

/// What the server needs from the world: the park handoff, the elicit card,
/// and Halt. Tests swap in fakes.
pub(crate) trait SelfIo {
    /// Post a park file and wait. True only on the user's Approve.
    fn park(&mut self, req: &hx::ParkRequest) -> bool;
    /// Send `elicitation/create` and wait for its result object.
    fn elicit(&mut self, params: Value) -> Option<Value>;
    fn halted(&mut self) -> bool;
}

pub(crate) struct SelfServer<'a> {
    config_dir: &'a Path,
    /// The client said it can show an elicitation form.
    elicit_ok: bool,
}

impl<'a> SelfServer<'a> {
    pub(crate) fn new(config_dir: &'a Path) -> Self {
        Self { config_dir, elicit_ok: false }
    }

    /// One request line in, at most one reply line out.
    pub(crate) fn handle_line(&mut self, line: &str, io: &mut dyn SelfIo) -> Option<String> {
        let v: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(_) => return Some(rpc_error(&Value::Null, -32700, "Parse error")),
        };
        let id = v.get("id").cloned().filter(|i| !i.is_null())?;
        let method = v.get("method").and_then(|m| m.as_str())?;
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        Some(match method {
            "initialize" => {
                self.elicit_ok = params.pointer("/capabilities/elicitation").is_some();
                let asked = params.get("protocolVersion").and_then(|p| p.as_str()).unwrap_or("");
                let version = if PROTOCOLS.contains(&asked) { asked } else { PROTOCOLS[0] };
                rpc_result(
                    &id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": { "name": SELF_MCP_SERVER, "version": env!("CARGO_PKG_VERSION") },
                    }),
                )
            }
            "ping" => rpc_result(&id, json!({})),
            "tools/list" => rpc_result(&id, json!({ "tools": sm::mcp_tools() })),
            "tools/call" => {
                let tool = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
                let (ok, text) = self.call(tool, &args, io);
                tool_reply(&id, ok, &text)
            }
            _ => rpc_error(&id, -32601, "Method not found"),
        })
    }

    /// Gate, park, ask for secrets, run, and write the spans.
    fn call(&mut self, tool: &str, args: &Value, io: &mut dyn SelfIo) -> (bool, String) {
        let mcp = format!("{SELF_MCP_SERVER}__{tool}");
        let Some(class) = sm::self_class(&mcp, args) else {
            return (false, format!("unknown tool `{tool}`"));
        };
        let span_args = safe_args(args);
        if io.halted() {
            let why = "Halted. Nothing ran.".to_string();
            self.span(hx::Span::deny("", &mcp, &span_args, &why, class.as_str()));
            return (false, why);
        }
        let arguments = args.to_string();
        let hard = match hx::decide(hx::Step::Tool { name: &mcp, arguments: &arguments }) {
            GateOutcome::Allow => None,
            GateOutcome::Refuse { reason } => {
                self.span(hx::Span::deny("", &mcp, &span_args, &reason, class.as_str()));
                return (false, reason);
            }
            GateOutcome::Park { reason, hard, .. } => {
                let class = hard.unwrap_or(HardClass::Delete);
                if !self.park(&mcp, args, class, io) {
                    let why = format!("Denied: {reason}. Jeremy did not approve it.");
                    self.span(hx::Span::deny("", &mcp, &span_args, &why, class.as_str()));
                    return (false, why);
                }
                self.span(hx::Span::hard_approve("", &mcp, &span_args, class));
                Some(class)
            }
        };
        let ctx = sm::SelfCtx::new(self.config_dir);
        let elicit_ok = self.elicit_ok;
        let mut ask = |message: &str| ask_token(elicit_ok, message, io);
        let out = sm::run(&ctx, &mcp, args, &mut ask);
        let mut span = hx::Span::soft_allow("", &mcp, &span_args, &out.text, "", hx::AccessMode::Supervised, "grokhub-self");
        span.approval_class = class.as_str().into();
        span.hard_approved = hard.is_some();
        if out.failed {
            span.decision = "deny".into();
        }
        self.span(span);
        (!out.failed, out.text)
    }

    fn park(&self, mcp: &str, args: &Value, class: HardClass, io: &mut dyn SelfIo) -> bool {
        static N: AtomicU64 = AtomicU64::new(0);
        let req = hx::ParkRequest {
            id: format!("self-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)),
            path: "A".into(),
            tool: mcp.into(),
            action: hx::redact_args(&park_action(mcp, args)),
            class: class.as_str().into(),
            ts_ms: grokhub_core::now_ms(),
        };
        self.span(hx::Span::hard_park("", mcp, &safe_args(args), class));
        io.park(&req)
    }

    fn span(&self, span: hx::Span) {
        let ctx = hx::read_turn_context(self.config_dir);
        let session = if ctx.chat_id.is_empty() { SELF_TRACE } else { ctx.chat_id.as_str() };
        let mut span = span.from_origin(Origin::SelfManage).on_path("A").in_turn(&ctx.chat_id, ctx.turn);
        span.session_id = session.into();
        if !ctx.access.is_empty() {
            span.access = ctx.access;
        }
        let _ = hx::append_span(self.config_dir, &span);
    }
}

/// The masked token card over MCP elicitation, the same shape Spike-5b's
/// native card uses. The value goes straight to the sealed store; it is
/// never echoed, logged, or returned to the model. No elicitation support
/// in the client means no token (fail closed).
fn ask_token(elicit_ok: bool, message: &str, io: &mut dyn SelfIo) -> Option<String> {
    if !elicit_ok {
        return None;
    }
    let params = json!({
        "message": message,
        "requestedSchema": {
            "type": "object",
            "properties": { "token": { "type": "string", "title": "Token", "format": "password", "writeOnly": true } },
            "required": ["token"],
        },
    });
    let result = io.elicit(params)?;
    if result.get("action").and_then(|a| a.as_str()) != Some("accept") {
        return None;
    }
    result
        .pointer("/content/token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Args as a span may keep them: secret-shaped values redacted, and anything
/// under `env` or `headers` dropped (a model can try to pass a value there).
pub(crate) fn safe_args(args: &Value) -> String {
    let mut a = args.clone();
    if let Some(m) = a.as_object_mut() {
        for k in ["env", "headers", "value", "token"] {
            if m.contains_key(k) {
                m.insert(k.into(), json!("%secret%"));
            }
        }
    }
    hx::redact_args(&grokhub_core::redact_secrets(&a.to_string()))
}

/// The line the hard card shows.
fn park_action(mcp: &str, args: &Value) -> String {
    let leaf = mcp.rsplit("__").next().unwrap_or(mcp);
    let what = args
        .get("name")
        .or_else(|| args.get("id"))
        .and_then(|n| n.as_str())
        .unwrap_or("?");
    let token = if sm::needs_token(args) { " with a token you type" } else { "" };
    format!("{} {what}{token}", leaf.replace('_', " "))
}

fn rpc_result(id: &Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn rpc_error(id: &Value, code: i64, msg: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } }).to_string()
}

fn tool_reply(id: &Value, ok: bool, text: &str) -> String {
    rpc_result(id, json!({ "content": [{ "type": "text", "text": text }], "isError": !ok }))
}

/// The real world: park files, stdio elicitation, the halt stamp.
struct LiveIo<'a> {
    config_dir: &'a Path,
    started: u64,
    pending: &'a mut VecDeque<String>,
    reader: &'a mut dyn std::io::BufRead,
}

impl SelfIo for LiveIo<'_> {
    fn park(&mut self, req: &hx::ParkRequest) -> bool {
        if hx::post_park(self.config_dir, req).is_err() {
            return false;
        }
        let started = self.started;
        let mut halted = || stamp_halted(started);
        hx::wait_park(self.config_dir, &req.id, hx::APPROVAL_TTL, Duration::from_millis(200), &mut halted)
    }

    fn elicit(&mut self, params: Value) -> Option<Value> {
        static N: AtomicU64 = AtomicU64::new(0);
        let id = format!("self-elicit-{}", N.fetch_add(1, Ordering::Relaxed));
        emit(&json!({ "jsonrpc": "2.0", "id": id, "method": "elicitation/create", "params": params }).to_string());
        loop {
            let line = crate::desktop_mcp::read_line_capped(&mut self.reader, LINE_CAP).ok()??;
            let v: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
            if v.get("id").and_then(|i| i.as_str()) == Some(id.as_str()) && v.get("method").is_none() {
                return v.get("result").cloned();
            }
            // Anything else waits its turn after this call.
            self.pending.push_back(line);
        }
    }

    fn halted(&mut self) -> bool {
        stamp_halted(self.started)
    }
}

fn stamp_halted(started: u64) -> bool {
    crate::desktop_mcp::read_halt_stamp().is_some_and(|ms| grokhub_core::desktop_mcp::stamp_halts(ms, started))
}

fn emit(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

pub fn run_stdio() -> i32 {
    let started = crate::desktop_mcp::process_started_ms();
    let dir = crate::config::config_dir();
    let mut server = SelfServer::new(&dir);
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut pending = VecDeque::new();
    loop {
        let line = match pending.pop_front() {
            Some(l) => l,
            None => match crate::desktop_mcp::read_line_capped(&mut reader, LINE_CAP) {
                Ok(None) => break,
                Ok(Some(l)) => l,
                Err(msg) => {
                    eprintln!("self-mcp: {msg}");
                    emit(&rpc_error(&Value::Null, -32700, "Parse error"));
                    continue;
                }
            },
        };
        if line.trim().is_empty() {
            continue;
        }
        let mut io = LiveIo { config_dir: &dir, started, pending: &mut pending, reader: &mut reader };
        if let Some(reply) = server.handle_line(&line, &mut io) {
            emit(&reply);
        }
    }
    0
}

#[cfg(test)]
pub(crate) mod tests;
