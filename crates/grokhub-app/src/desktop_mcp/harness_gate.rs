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
use grokhub_core::desktop_mcp::{CallGate, DesktopBackend, DesktopServer, DesktopWindows, RpcOutcome};
use serde_json::{json, Value};

/// How long `open_app` / `focus_window` may take to show a change.
const WINDOW_WAIT: Duration = Duration::from_secs(3);
/// One look after a click or typing, as before.
const SHOT_WAIT: Duration = Duration::from_millis(150);
const PROBE_STEP: Duration = Duration::from_millis(150);

/// One stdin line through path A: the focused window for a Delete key, the
/// pre-check (`harness::decide`), a park when hard, the server, the
/// before/after check, and the span. Returns what `run_stdio` prints.
pub(crate) fn handle_desk_line<B: DesktopBackend>(
    server: &mut DesktopServer<B>,
    line: &str,
    gate: CallGate,
    dir: &Path,
    halted: &mut dyn FnMut() -> bool,
) -> RpcOutcome {
    let mut call = parse_call(line);
    let live = gate.enabled && !gate.halted;
    let mut parked = None;
    if let Some(c) = call.as_mut() {
        if live && c.tool == "key" && delete_key(&c.args) {
            // `decide` can only call a Delete key hard when it knows a file
            // manager has focus.
            if let Ok(w) = server.backend_mut().list_windows() {
                c.args["window"] = Value::String(w.active);
            }
        }
        let access = access_now(dir, gate.enabled);
        let refused = match precheck(c, gate) {
            Precheck::Pass => None,
            Precheck::Refuse(reason) => Some(reason),
            Precheck::Park { class, reason } => {
                let approved = park_and_wait(dir, c, &class, halted);
                parked = hx::HardClass::parse(&class).map(|h| (h, approved));
                (!approved).then(|| format!("Denied: {reason}. Jeremy did not approve it."))
            }
        };
        if let Some(reason) = refused {
            write_span(dir, c, false, &reason, access, None, parked);
            return RpcOutcome { reply: Some(refusal_reply(&c.id, &reason)), exit: false };
        }
    }
    let probe = call.as_ref().filter(|_| live).map_or(Probe::None, |c| probe_for(&c.tool));
    let before = observe(server.backend_mut(), probe);
    // The server answers the call it was sent; the `window` hint is ours.
    let outcome = match &call {
        Some(c) => {
            let sent = json!({
                "jsonrpc": "2.0", "id": c.id, "method": "tools/call",
                "params": { "name": c.tool, "arguments": strip_window(&c.args) },
            });
            server.handle_line(&sent.to_string(), gate)
        }
        None => server.handle_line(line, gate),
    };
    if let (Some(c), Some(reply)) = (&call, outcome.reply.as_deref()) {
        let (ok, text) = reply_result(reply);
        let ui_changed = match (&before, ok) {
            (Some(pre), true) => changed_after(server.backend_mut(), pre),
            _ => None,
        };
        let access = access_now(dir, gate.enabled);
        write_span(dir, c, ok, &text, access, ui_changed, parked);
    }
    outcome
}

fn delete_key(args: &Value) -> bool {
    let keys = args["keys"].as_str().unwrap_or("").to_ascii_lowercase().replace(' ', "");
    matches!(keys.as_str(), "delete" | "del" | "shift+delete" | "shift+del")
}

fn strip_window(args: &Value) -> Value {
    let mut a = args.clone();
    if let Some(m) = a.as_object_mut() {
        m.remove("window");
    }
    a
}

/// What `ui_changed` is measured with for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Probe {
    None,
    /// Screenshot hash: clicks and typing.
    Shot,
    /// The window list (focus and the set of windows), else the screenshot.
    Windows,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Seen {
    Shot(u64),
    Windows(DesktopWindows),
}

pub(crate) fn probe_for(tool: &str) -> Probe {
    match tool {
        "click" | "type" => Probe::Shot,
        "open_app" | "focus_window" => Probe::Windows,
        _ => Probe::None,
    }
}

pub(crate) fn observe<B: DesktopBackend + ?Sized>(b: &mut B, probe: Probe) -> Option<Seen> {
    match probe {
        Probe::None => None,
        Probe::Shot => b.screenshot("all").ok().map(|s| Seen::Shot(shot_hash(&s.bytes))),
        Probe::Windows => match b.list_windows() {
            Ok(w) => Some(Seen::Windows(w)),
            Err(_) => observe(b, Probe::Shot),
        },
    }
}

/// Look again until something changed or the wait is up. A window opening
/// can take a moment; a click or keystroke gets one look.
pub(crate) fn changed_after<B: DesktopBackend + ?Sized>(b: &mut B, pre: &Seen) -> Option<bool> {
    let wait = match pre {
        Seen::Windows(_) => WINDOW_WAIT,
        Seen::Shot(_) => SHOT_WAIT,
    };
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(PROBE_STEP);
        let now = match pre {
            Seen::Windows(_) => b.list_windows().ok().map(Seen::Windows),
            Seen::Shot(_) => observe(b, Probe::Shot),
        }?;
        let changed = match (pre, &now) {
            (Seen::Windows(a), Seen::Windows(z)) => super::apps::windows_changed(a, z),
            (a, z) => a != z,
        };
        if changed || start.elapsed() >= wait {
            return Some(changed);
        }
    }
}

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
    // A credential field's value never reaches the park file, the card, or a span.
    let action = match call.tool.as_str() {
        _ if class == hx::HardClass::Credentials.as_str() => hx::credential_action(&call.args),
        "type" => call.args["text"].as_str().unwrap_or("").to_string(),
        // The file manager does not say which files are selected.
        "key" if class == hx::HardClass::Delete.as_str() => format!(
            "press {} on the files selected in {} (the cabin can't see which)",
            call.args["keys"].as_str().unwrap_or(""),
            call.args["window"].as_str().unwrap_or("a file manager")
        ),
        "key" => call.args["keys"].as_str().unwrap_or("").to_string(),
        "delete_files" => hx::delete_files_action(&call.args),
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
        let pin = line("type", json!({ "text": "4821", "label": "PIN" }));
        assert_eq!(
            precheck(&pin, ON),
            Precheck::Park {
                class: "credentials".into(),
                reason: "hard-class credentials: Credentials / secrets — Always cannot skip".into(),
            }
        );
        // The cabin sees the park file while the MCP waits; Esc / Deny answers it.
        let cabin_dir = dir.clone();
        let waiter = std::thread::spawn(move || {
            for _ in 0..200 {
                if let Some(req) = hx::pending_parks(&cabin_dir).pop() {
                    let _ = hx::answer_park(&cabin_dir, &req.id, false);
                    return Some(req);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            None
        });
        assert!(!park_and_wait(&dir, &pin, "credentials", &mut || false));
        let req = waiter.join().unwrap().expect("park file posted");
        assert_eq!(req.action, "type into PIN (4 chars hidden)");
        assert_eq!(req.class, "credentials");
        let spans = hx::read_spans(&dir, hx::CU_TRACE).unwrap();
        let last = spans.last().unwrap();
        assert_eq!((last.decision.as_str(), last.approval_class.as_str()), ("park", "credentials"));
        assert_eq!(last.args_redacted, r#"{"chars":4}"#);
        let trace = std::fs::read_to_string(hx::span_path(&dir, hx::CU_TRACE)).unwrap();
        assert!(!trace.contains("4821"), "the typed PIN never reaches a span");
        assert_eq!(access_now(&dir, false), AccessMode::Readonly);
        assert_eq!(access_now(&dir, true), AccessMode::Supervised);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A screen whose pixels change on each typed string and whose window list
    /// gains a window when an app opens or changes focus.
    #[derive(Default)]
    struct Screen {
        typed: Vec<String>,
        opened: Vec<String>,
        frame: u8,
        windows: DesktopWindows,
        keys: usize,
    }

    impl DesktopBackend for Screen {
        fn list_monitors(&mut self) -> Result<Vec<grokhub_core::desktop_mcp::MonitorGeom>, String> {
            Ok(vec![grokhub_core::desktop_mcp::MonitorGeom {
                id: "m".into(),
                name: "m".into(),
                x: 0,
                y: 0,
                width: 100,
                height: 50,
                scale_factor: 1.0,
                primary: true,
            }])
        }
        fn screenshot(&mut self, _monitor: &str) -> Result<grokhub_core::desktop_mcp::CapturedShot, String> {
            let mon = &self.list_monitors()?[0];
            Ok(grokhub_core::desktop_mcp::CapturedShot {
                bytes: vec![self.frame],
                mime: "image/png".into(),
                geom: grokhub_core::desktop_mcp::ShotGeom::native(mon),
            })
        }
        fn move_abs(&mut self, _x: i32, _y: i32) -> Result<(), String> {
            Ok(())
        }
        fn button(&mut self, _b: grokhub_core::desktop_mcp::MouseButton, _down: bool) -> Result<(), String> {
            Ok(())
        }
        fn scroll(&mut self, _dx: i32, _dy: i32) -> Result<(), String> {
            Ok(())
        }
        fn type_text(&mut self, text: &str) -> Result<(), String> {
            self.typed.push(text.into());
            self.frame = self.frame.wrapping_add(1);
            Ok(())
        }
        fn key_combo(&mut self, _combo: &grokhub_core::desktop_mcp::KeyCombo) -> Result<(), String> {
            self.keys += 1;
            Ok(())
        }
        fn is_locked(&mut self) -> bool {
            false
        }
        fn open_app(&mut self, app: &str) -> Result<(), String> {
            self.opened.push(app.into());
            self.windows.titles.push(format!("{app} window"));
            self.windows.active = format!("{app} {app} window");
            Ok(())
        }
        fn focus_window(&mut self, title: &str) -> Result<String, String> {
            let got = super::super::apps::pick_title(self.windows.titles.iter().map(String::as_str), title)
                .ok_or("no window")?
                .to_string();
            self.windows.active = format!("x {got}");
            Ok(got)
        }
        fn list_windows(&mut self) -> Result<DesktopWindows, String> {
            Ok(self.windows.clone())
        }
    }

    fn desk() -> DesktopServer<Screen> {
        let screen = Screen {
            windows: DesktopWindows {
                titles: vec!["GrokHub".into(), "Downloads — Dolphin".into()],
                active: "grokhub GrokHub".into(),
            },
            ..Screen::default()
        };
        DesktopServer::new("t", screen)
    }

    fn rpc(tool: &str, args: Value) -> String {
        json!({
            "jsonrpc": "2.0", "id": 8, "method": "tools/call",
            "params": { "name": tool, "arguments": args },
        })
        .to_string()
    }

    fn reply_text(out: &RpcOutcome) -> String {
        reply_result(out.reply.as_deref().unwrap()).1
    }

    fn turn(dir: &Path, access: &str) {
        let _ = std::fs::create_dir_all(dir);
        hx::write_turn_context(dir, &hx::TurnContext { chat_id: "chat-1".into(), turn: 3, access: access.into() }).unwrap();
    }

    fn spans(dir: &Path) -> Vec<hx::Span> {
        hx::read_spans(dir, "chat-1").unwrap()
    }

    /// The cabin's half: answer the first park the MCP posts.
    fn cabin_answers(dir: &Path, approve: bool) -> std::thread::JoinHandle<Option<hx::ParkRequest>> {
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            for _ in 0..500 {
                if let Some(req) = hx::pending_parks(&dir).pop() {
                    let _ = hx::answer_park(&dir, &req.id, approve);
                    return Some(req);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            None
        })
    }

    #[test]
    fn open_and_focus_are_soft_and_record_ui_changed() {
        let dir = crate::config::test_config_root("desk-open");
        turn(&dir, "supervised");
        let mut s = desk();
        let out = handle_desk_line(&mut s, &rpc("open_app", json!({ "app": "org.kde.kate" })), ON, &dir, &mut || false);
        assert_eq!(reply_text(&out), "opened org.kde.kate");
        let out = handle_desk_line(&mut s, &rpc("focus_window", json!({ "title": "dolphin" })), ON, &dir, &mut || false);
        assert_eq!(reply_text(&out), "focused Downloads — Dolphin");
        // Focusing the window that already has focus changes nothing.
        let out = handle_desk_line(&mut s, &rpc("focus_window", json!({ "title": "dolphin" })), ON, &dir, &mut || false);
        assert_eq!(reply_text(&out), "focused Downloads — Dolphin");
        let got: Vec<_> = spans(&dir)
            .iter()
            .map(|s| (s.tool.clone(), s.decision.clone(), s.approval_class.clone(), s.path.clone(), s.ui_changed))
            .collect();
        let row = |t: &str, ui: bool| (t.to_string(), "allow".to_string(), "soft".to_string(), "A".to_string(), Some(ui));
        assert_eq!(got, vec![row("open_app", true), row("focus_window", true), row("focus_window", false)]);
        assert!(hx::pending_parks(&dir).is_empty(), "open and focus never park");
        assert_eq!(s.backend_mut().opened, vec!["org.kde.kate"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_and_focus_are_refused_with_desktop_control_off() {
        let dir = crate::config::test_config_root("desk-open-off");
        turn(&dir, "readonly");
        let mut s = desk();
        let off = CallGate { enabled: false, halted: false };
        let out = handle_desk_line(&mut s, &rpc("open_app", json!({ "app": "firefox" })), off, &dir, &mut || false);
        assert_eq!(reply_text(&out), grokhub_core::desktop_mcp::OFF_MSG);
        let out = handle_desk_line(&mut s, &rpc("focus_window", json!({ "title": "GrokHub" })), off, &dir, &mut || false);
        assert_eq!(reply_text(&out), grokhub_core::desktop_mcp::OFF_MSG);
        assert!(s.backend_mut().opened.is_empty());
        assert_eq!(s.backend_mut().windows.active, "grokhub GrokHub", "nothing was focused");
        let got: Vec<_> = spans(&dir).iter().map(|s| (s.tool.clone(), s.decision.clone(), s.access.clone())).collect();
        assert_eq!(
            got,
            vec![
                ("open_app".to_string(), "deny".to_string(), "readonly".to_string()),
                ("focus_window".to_string(), "deny".to_string(), "readonly".to_string()),
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn typing_is_soft_keeps_only_the_length_and_the_pin_field_still_parks() {
        let dir = crate::config::test_config_root("desk-type");
        turn(&dir, "full");
        let mut s = desk();
        let out = handle_desk_line(&mut s, &rpc("type", json!({ "text": "quarterly notes" })), ON, &dir, &mut || false);
        assert_eq!(reply_text(&out), "typed");
        let first = spans(&dir).pop().unwrap();
        assert_eq!(
            (first.decision.as_str(), first.approval_class.as_str(), first.args_redacted.as_str(), first.ui_changed),
            ("allow", "soft", r#"{"chars":15}"#, Some(true))
        );
        assert_eq!(first.access, "full");
        // Spike-1a regression: a PIN field parks hard even under Full, and Deny types nothing.
        let waiter = cabin_answers(&dir, false);
        let pin = rpc("type", json!({ "text": "4821", "label": "PIN" }));
        let out = handle_desk_line(&mut s, &pin, ON, &dir, &mut || false);
        let req = waiter.join().unwrap().expect("PIN park posted");
        assert_eq!(req.action, "type into PIN (4 chars hidden)");
        assert_eq!(req.class, "credentials");
        assert!(reply_text(&out).starts_with("Denied: hard-class credentials"), "{}", reply_text(&out));
        assert_eq!(s.backend_mut().typed, vec!["quarterly notes"], "the PIN was never typed");
        let tail: Vec<_> = spans(&dir).iter().skip(1).map(|s| (s.decision.clone(), s.approval_class.clone())).collect();
        assert_eq!(
            tail,
            vec![("park".to_string(), "credentials".to_string()), ("deny".to_string(), "credentials".to_string())]
        );
        let trace = std::fs::read_to_string(hx::span_path(&dir, "chat-1")).unwrap();
        assert!(!trace.contains("quarterly notes") && !trace.contains("4821"), "typed values stay out: {trace}");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn fixture(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let files = dir.join("files");
        std::fs::create_dir_all(&files).unwrap();
        let a = files.join("report.txt");
        let b = files.join("draft.txt");
        let keep = files.join("keep.txt");
        std::fs::write(&a, b"fixture report\n").unwrap();
        std::fs::write(&b, b"fixture draft\n").unwrap();
        std::fs::write(&keep, b"keep me\n").unwrap();
        (a, b, keep)
    }

    fn path_str(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn delete_parks_hard_under_full_and_deny_or_halt_leaves_the_files() {
        let dir = crate::config::test_config_root("desk-delete-deny");
        turn(&dir, "full");
        let (a, b, _) = fixture(&dir);
        let mut s = desk();
        let del = rpc("delete_files", json!({ "paths": [path_str(&a), path_str(&b)] }));
        let waiter = cabin_answers(&dir, false);
        let out = handle_desk_line(&mut s, &del, ON, &dir, &mut || false);
        let req = waiter.join().unwrap().expect("delete park posted");
        assert_eq!(req.class, "delete");
        assert_eq!(req.action, hx::redact_args(&format!("delete 2 paths: {}, {}", path_str(&a), path_str(&b))));
        assert!(reply_text(&out).starts_with("Denied: hard-class delete"));
        assert_eq!(std::fs::read(&a).unwrap(), b"fixture report\n", "byte-identical after Deny");
        assert_eq!(std::fs::read(&b).unwrap(), b"fixture draft\n");
        // Halt fails closed the same way (the TTL runs out through the same branch).
        let trash = rpc("delete_files", json!({ "paths": [path_str(&a)], "to_trash": true }));
        let out = handle_desk_line(&mut s, &trash, ON, &dir, &mut || true);
        assert!(reply_text(&out).starts_with("Denied: hard-class delete"));
        assert_eq!(std::fs::read(&a).unwrap(), b"fixture report\n");
        assert!(hx::pending_parks(&dir).is_empty());
        let got: Vec<_> = spans(&dir).iter().map(|s| (s.tool.clone(), s.decision.clone(), s.approval_class.clone())).collect();
        let row = |d: &str| ("delete_files".to_string(), d.to_string(), "delete".to_string());
        assert_eq!(got, vec![row("park"), row("deny"), row("park"), row("deny")]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn approve_deletes_exactly_the_listed_paths_once() {
        let dir = crate::config::test_config_root("desk-delete-ok");
        turn(&dir, "full");
        let (a, b, keep) = fixture(&dir);
        let mut s = desk();
        let del = rpc("delete_files", json!({ "paths": [path_str(&a), path_str(&b)] }));
        let waiter = cabin_answers(&dir, true);
        let out = handle_desk_line(&mut s, &del, ON, &dir, &mut || false);
        assert!(waiter.join().unwrap().is_some());
        assert_eq!(reply_text(&out), "deleted 2 paths");
        assert!(!a.exists() && !b.exists());
        assert_eq!(std::fs::read(&keep).unwrap(), b"keep me\n", "an unlisted file stays");
        let last = spans(&dir).pop().unwrap();
        assert_eq!((last.decision.as_str(), last.approval_class.as_str(), last.hard_approved), ("allow", "delete", true));
        // Once: the same call again parks again, and with no answer it is denied.
        let out = handle_desk_line(&mut s, &del, ON, &dir, &mut || true);
        assert!(reply_text(&out).starts_with("Denied:"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn delete_key_parks_only_on_a_file_manager() {
        let dir = crate::config::test_config_root("desk-delete-key");
        turn(&dir, "full");
        let mut s = desk();
        let del = rpc("key", json!({ "keys": "Delete" }));
        // GrokHub has focus: a Delete key is a plain key.
        let out = handle_desk_line(&mut s, &del, ON, &dir, &mut || false);
        assert_eq!(reply_text(&out), "keyed");
        s.backend_mut().windows.active = "org.kde.dolphin Downloads — Dolphin".into();
        let waiter = cabin_answers(&dir, false);
        let out = handle_desk_line(&mut s, &del, ON, &dir, &mut || false);
        let req = waiter.join().unwrap().expect("park posted");
        assert_eq!(req.class, "delete");
        assert_eq!(
            req.action,
            "press Delete on the files selected in org.kde.dolphin Downloads — Dolphin (the cabin can't see which)"
        );
        assert!(reply_text(&out).starts_with("Denied: hard-class delete"));
        assert_eq!(s.backend_mut().keys, 1, "the denied Delete was never pressed");
        let _ = std::fs::remove_dir_all(dir);
    }
}
