//! Spike-2a: Cua Driver as an optional second pair of hands on Linux.
//!
//! `grokhub --mcp-cua` is a cabin MCP server that Grok Build registers as
//! `grokhub-cua` (the cabin Grok home only). It starts the pinned MIT
//! `cua-driver mcp` as its own child in `bounded` mode with a cabin-written
//! manifest, and runs every `tools/call` through [`decide`] before it is
//! forwarded: the floor refuses, hard class parks the same card as path A,
//! and the desktop switch, the `cuaDriver` flag, and Halt refuse. Grok Build
//! never sees Cua's socket. Spans carry `driver:"cua"` and store typed text
//! as its length only.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::harness::approval::{decide, GateOutcome, Step};
use crate::harness::backend::{desk_access, desk_span, park_desk_call, ComputerUseBackend, DeskCall};
use crate::harness::hard::{HardClass, TARGET_HINT};
use crate::harness::span::{append_span, read_turn_context};

/// Pinned `cua-driver-rs` release (github.com/trycua/cua). Every
/// `cua-driver-rs-v*` release is marked pre-release on GitHub; this is the
/// newest one at build time and the first with the six cursor motions.
pub const CUA_DRIVER_VERSION: &str = "0.34.0";
pub const CUA_DRIVER_TAG: &str = "cua-driver-rs-v0.34.0";
/// The Linux x86_64 release asset and its sha256 from the release's `SHA256SUMS`.
pub const CUA_LINUX_ASSET: &str = "cua-driver-rs-0.34.0-linux-x86_64.tar.gz";
pub const CUA_LINUX_SHA256: &str = "8851c028e5d88a3b779ad2a8d49ed6f4b92aa94de2edf5c2851cee63bcc274c5";
/// The Driver is MIT. The AGPL `cua-perception` extension is never used.
pub const CUA_DRIVER_LICENSE: &str = "MIT";
/// Executable name looked up on PATH when `app.json` names no path.
pub const CUA_BIN: &str = "cua-driver";

/// The only mode the cabin runs. Never `standard`, never `unrestricted`.
pub const CUA_PERMISSION_MODE: &str = "bounded";
/// Inherited variables that could loosen the child; always removed.
pub const CUA_ENV_REMOVE: &[&str] = &["CUA_DRIVER_DANGEROUSLY_BYPASS_APPROVALS", "CUA_DRIVER_POLICY_FILE"];

/// Tools the manifest allows and the proxy forwards: look (AX tree first,
/// screenshot as fallback), act (named control or pixel), type, keys,
/// scroll, drag, apps and windows, and wait.
pub const CUA_TOOLS: &[&str] = &[
    "start_session",
    "end_session",
    "list_apps",
    "list_windows",
    "launch_app",
    "get_window_state",
    "screenshot",
    "click",
    "type_text",
    "set_value",
    "press_key",
    "hotkey",
    "scroll",
    "drag",
    "wait",
];
/// Denied in the manifest as well as left out of `CUA_TOOLS`.
pub const CUA_DENY_TOOLS: &[&str] = &["shell_execute", "page", "browser_download", "browser_set_input_files"];
/// App scope for this spike: a disposable text editor (the measured task).
pub const CUA_APPS: &[&str] = &[
    "org.gnome.TextEditor",
    "org.gnome.gedit",
    "org.kde.kate",
    "org.kde.kwrite",
    "org.xfce.mousepad",
];

/// Typed placeholder for a credential. The model sees only this.
pub const PASSWORD_SLOT: &str = "%password%";

pub const CUA_OFF_MSG: &str = "Cua Driver is off. GrokHub only uses it while the cuaDriver spike flag is on.";
pub const CUA_LINUX_ONLY_MSG: &str = "Cua Driver runs on Linux only in this GrokHub build.";
pub const CUA_MISSING_MSG: &str =
    "Cua Driver is not installed. Install cua-driver-rs 0.34.0, or set cuaDriverPath in app.json.";
pub const CUA_NOT_IN_MANIFEST: &str = "That Cua tool is not in GrokHub's Cua manifest, so it was not run.";
pub const CUA_NO_PASSWORD_MSG: &str =
    "GrokHub has no saved password for that field, so nothing was typed. Type it yourself.";

/// How long the proxy waits for the child's answer to one request.
const REPLY_WAIT: Duration = Duration::from_secs(60);
/// How long `cua-driver --version` may take.
const VERSION_WAIT: Duration = Duration::from_secs(5);

/// `{config_dir}/cua`: the child's socket and the manifest. Cabin-private.
pub fn cua_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("cua")
}

pub fn cua_socket_path(config_dir: &Path) -> PathBuf {
    cua_dir(config_dir).join("driver.sock")
}

pub fn cua_manifest_path(config_dir: &Path) -> PathBuf {
    cua_dir(config_dir).join("manifest.yaml")
}

fn yaml_list(indent: &str, items: &[&str]) -> String {
    items.iter().map(|i| format!("{indent}- {i}\n")).collect()
}

/// The bounded capability manifest: only the tools and apps this spike needs.
pub fn cua_manifest() -> String {
    format!(
        "version: 1\nmode: bounded\nresources:\n  applications:\n{}allow:\n  tools:\n{}deny:\n  tools:\n{}",
        CUA_APPS.iter().map(|a| format!("    - bundle_id: {a}\n")).collect::<String>(),
        yaml_list("    ", CUA_TOOLS),
        yaml_list("    ", CUA_DENY_TOOLS),
    )
}

/// `cua-driver mcp --socket <cabin-private socket>`
pub fn cua_spawn_args(socket: &Path) -> Vec<String> {
    vec!["mcp".into(), "--socket".into(), socket.display().to_string()]
}

/// The child's permission env: bounded, the cabin manifest, and its approval.
pub fn cua_spawn_env(manifest: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("CUA_DRIVER_PERMISSION_MODE", CUA_PERMISSION_MODE.into()),
        ("CUA_DRIVER_CAPABILITY_MANIFEST_FILE", manifest.display().to_string()),
        ("CUA_DRIVER_CAPABILITY_MANIFEST_APPROVED", "1".into()),
    ]
}

/// Read `cua-driver --version` output. Only the pinned MIT release passes.
pub fn check_cua_version(output: &str) -> Result<(), String> {
    let lower = output.to_ascii_lowercase();
    if lower.contains("agpl") || lower.contains("perception") {
        return Err(format!(
            "This Cua Driver build includes the AGPL perception extension. GrokHub only runs the {CUA_DRIVER_LICENSE} Driver."
        ));
    }
    let found = output
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '(' | ')'))
        .map(|w| w.trim_start_matches('v'))
        .find(|w| {
            let parts: Vec<&str> = w.split('.').collect();
            parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        });
    match found {
        Some(v) if v == CUA_DRIVER_VERSION => Ok(()),
        Some(v) => Err(format!(
            "Cua Driver {v} is installed, but GrokHub only runs the pinned {CUA_DRIVER_VERSION} ({CUA_DRIVER_TAG}). Install that release."
        )),
        None => Err("GrokHub could not read the Cua Driver version, so it did not start it.".into()),
    }
}

/// `cua-driver` from `app.json` (`cuaDriverPath`) or PATH.
pub fn find_cua_driver(override_path: &str) -> Option<PathBuf> {
    let p = override_path.trim();
    if !p.is_empty() {
        let path = PathBuf::from(p);
        return path.is_file().then_some(path);
    }
    let name = if cfg!(windows) { format!("{CUA_BIN}.exe") } else { CUA_BIN.to_string() };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&name))
        .find(|p| p.is_file())
}

/// Run `<bin> --version` and check it against the pin.
pub fn verify_cua_driver(bin: &Path) -> Result<(), String> {
    let mut cmd = Command::new(bin);
    cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    grokhub_acp::hide_windows_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("GrokHub could not run Cua Driver: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > VERSION_WAIT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Cua Driver did not answer --version, so GrokHub did not start it.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("GrokHub could not run Cua Driver: {e}")),
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    check_cua_version(&text)
}

/// A Cua call in the `grokhub-desktop` shape `desk_classify`, the park card
/// and the span read. `None` for anything that is not a Cua act. A
/// `%password%` slot marks the call secret, so it parks as credentials.
pub fn cua_as_desk(tool: &str, args: &Value) -> Option<(&'static str, Value)> {
    let mut a = args.clone();
    if !a.is_object() {
        a = json!({});
    }
    let shape = match tool {
        "double_click" | "right_click" => "click",
        "type_text" => "type",
        "set_value" => {
            let value = args.get("value").cloned().unwrap_or(Value::Null);
            a["text"] = value;
            "type"
        }
        "press_key" => {
            a["keys"] = args.get("key").cloned().unwrap_or(Value::Null);
            "key"
        }
        "hotkey" => {
            let keys = match args.get("keys") {
                Some(Value::Array(k)) => k.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("+"),
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            a["keys"] = Value::String(keys);
            "key"
        }
        "launch_app" => {
            let app = ["bundle_id", "app", "name"]
                .iter()
                .find_map(|k| args.get(*k).and_then(Value::as_str))
                .unwrap_or("");
            a["app"] = Value::String(app.into());
            "open_app"
        }
        _ => return None,
    };
    if shape == "type" && a["text"].as_str().is_some_and(|t| t.contains(PASSWORD_SLOT)) {
        a["secret"] = Value::Bool(true);
    }
    Some((shape, a))
}

/// The desk tool name and args a Cua call is gated, parked, and logged as.
fn desk_shape(tool: &str, args: &Value) -> (String, Value) {
    match cua_as_desk(tool, args) {
        Some((t, a)) => (t.to_string(), a),
        None => (tool.to_string(), args.clone()),
    }
}

/// What the proxy checks before any call reaches the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CuaGate {
    /// Settings → Let Grok control the desktop.
    pub enabled: bool,
    /// `app.json` `cuaDriver`.
    pub flag: bool,
    /// Halt (Ctrl+Alt+H) since this process started.
    pub halted: bool,
}

/// The `cua-driver mcp` child as the proxy talks to it.
pub trait CuaChild {
    /// Send one line and return the reply whose `id` matches.
    fn request(&mut self, line: &str, id: &Value) -> Result<String, String>;
    /// Send a notification (no reply).
    fn notify(&mut self, line: &str);
    fn kill(&mut self);
}

/// The real child: line-delimited JSON-RPC on its stdin/stdout.
pub struct StdioChild {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<String>,
}

/// Write the manifest and start `cua-driver mcp` with the bounded env.
pub fn spawn_cua_child(bin: &Path, config_dir: &Path) -> Result<StdioChild, String> {
    let dir = cua_dir(config_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("GrokHub could not make {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let manifest = cua_manifest_path(config_dir);
    std::fs::write(&manifest, cua_manifest()).map_err(|e| format!("GrokHub could not write the Cua manifest: {e}"))?;
    let mut cmd = Command::new(bin);
    cmd.args(cua_spawn_args(&cua_socket_path(config_dir)))
        .envs(cua_spawn_env(&manifest))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    for k in CUA_ENV_REMOVE {
        cmd.env_remove(k);
    }
    grokhub_acp::hide_windows_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("GrokHub could not start Cua Driver: {e}"))?;
    let stdin = child.stdin.take().ok_or("Cua Driver has no stdin")?;
    let stdout = child.stdout.take().ok_or("Cua Driver has no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Ok(StdioChild { child, stdin, rx })
}

impl CuaChild for StdioChild {
    fn request(&mut self, line: &str, id: &Value) -> Result<String, String> {
        writeln!(self.stdin, "{line}").and_then(|_| self.stdin.flush()).map_err(|e| format!("Cua Driver stopped: {e}"))?;
        let deadline = Instant::now() + REPLY_WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let got = self.rx.recv_timeout(left).map_err(|_| "Cua Driver did not answer.".to_string())?;
            let v: Value = serde_json::from_str(&got).unwrap_or(Value::Null);
            if v.get("id") == Some(id) {
                return Ok(got);
            }
            // The child's own notifications and log lines are not forwarded.
        }
    }

    fn notify(&mut self, line: &str) {
        let _ = writeln!(self.stdin, "{line}").and_then(|_| self.stdin.flush());
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for StdioChild {
    fn drop(&mut self) {
        self.kill();
    }
}

/// The gate proxy. The child starts on the first call that may use it, so
/// with the flag or the switch off no child is ever started.
pub struct CuaProxy<C: CuaChild> {
    child: Option<C>,
    start: Box<dyn FnMut() -> Result<C, String>>,
    start_err: Option<String>,
    halted: bool,
    observe_n: u64,
    /// The proxy answered `initialize` itself (flag or switch off then), so a
    /// child started later never saw Grok Build's handshake.
    answered_init: bool,
}

impl<C: CuaChild> CuaProxy<C> {
    pub fn new(start: Box<dyn FnMut() -> Result<C, String>>) -> Self {
        Self { child: None, start, start_err: None, halted: false, observe_n: 0, answered_init: false }
    }

    /// Halt: kill the child. It is not started again in this process.
    pub fn halt(&mut self) {
        self.halted = true;
        if let Some(mut c) = self.child.take() {
            c.kill();
        }
    }

    fn child(&mut self) -> Result<&mut C, String> {
        if self.child.is_none() {
            if let Some(e) = &self.start_err {
                return Err(e.clone());
            }
            match (self.start)() {
                Ok(mut c) => {
                    if self.answered_init {
                        // Turned on mid-session: give the child the handshake
                        // an MCP server expects before its first request.
                        let id = json!("grokhub-cua-init");
                        let init = json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{
                            "protocolVersion":"2025-06-18",
                            "capabilities":{},
                            "clientInfo":{"name":grokhub_core::CUA_MCP_SERVER,"version":CUA_DRIVER_VERSION}}});
                        if let Err(e) = c.request(&init.to_string(), &id) {
                            c.kill();
                            return Err(e);
                        }
                        c.notify(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
                    }
                    self.child = Some(c);
                }
                Err(e) => {
                    self.start_err = Some(e.clone());
                    return Err(e);
                }
            }
        }
        self.child.as_mut().ok_or_else(|| CUA_OFF_MSG.to_string())
    }

    /// One stdin line from Grok Build. Returns what to print, if anything.
    pub fn handle_line(&mut self, line: &str, gate: CuaGate, dir: &Path, halted: &mut dyn FnMut() -> bool) -> Option<String> {
        if gate.halted {
            self.halt();
        }
        let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else {
            return Some(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}).to_string());
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let Some(id) = msg.get("id").cloned().filter(|i| !i.is_null()) else {
            if let Some(c) = self.child.as_mut() {
                c.notify(line.trim());
            }
            return None;
        };
        let live = gate.enabled && gate.flag && !self.halted;
        match method {
            "initialize" if !live => Some({
                self.answered_init = true;
                json!({"jsonrpc":"2.0","id":id,"result":{
                    "protocolVersion":"2025-06-18",
                    "capabilities":{"tools":{}},
                    "serverInfo":{"name":grokhub_core::CUA_MCP_SERVER,"version":CUA_DRIVER_VERSION}}})
                .to_string()
            }),
            "tools/list" if !live => Some(json!({"jsonrpc":"2.0","id":id,"result":{"tools":[]}}).to_string()),
            "tools/list" => Some(match self.child().and_then(|c| c.request(line.trim(), &id)) {
                Ok(reply) => manifest_tools_only(&reply),
                Err(e) => error_reply(&id, &e),
            }),
            "tools/call" => Some(self.call(&msg, id, gate, dir, halted)),
            _ if !live => Some(error_reply(&id, &self.off_reason(gate))),
            _ => Some(match self.child().and_then(|c| c.request(line.trim(), &id)) {
                Ok(reply) => reply,
                Err(e) => error_reply(&id, &e),
            }),
        }
    }

    fn off_reason(&self, gate: CuaGate) -> String {
        if self.halted {
            grokhub_core::desktop_mcp::HALT_MSG.into()
        } else if !gate.enabled {
            grokhub_core::desktop_mcp::OFF_MSG.into()
        } else {
            CUA_OFF_MSG.into()
        }
    }

    fn call(&mut self, msg: &Value, id: Value, gate: CuaGate, dir: &Path, halted: &mut dyn FnMut() -> bool) -> String {
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let tool = params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let args = params.get("arguments").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
        let (desk_tool, mut desk) = desk_shape(&tool, &args);
        let live = gate.enabled && gate.flag && !self.halted;
        let access = desk_access(dir, gate.enabled);
        let in_manifest = CUA_TOOLS.contains(&tool.as_str());
        let probe = tool == "click" || tool == "double_click" || tool == "right_click";
        // Spike-2b: look first. The AX tree names the control, so the gate
        // knows what the click will do before it runs.
        let before = if live && in_manifest && probe { self.observe(&args) } else { None };
        if live && in_manifest && probe {
            desk[TARGET_HINT] = cua_target(before.as_ref().map(|(_, state)| state), &args);
        }
        // Card 18: Enter, a paste or a typed newline is read with the window it
        // lands in. A step already hard without it sends the child nothing first.
        let soft_so_far = || decide(Step::Desk { tool: &desk_tool, args: &desk }).is_allow();
        if live && in_manifest && crate::harness::needs_window(&desk_tool, &desk) && soft_so_far() {
            if let Some(w) = self.observe(&args).and_then(|(_, state)| window_title(&state)) {
                desk["window"] = Value::String(w);
            }
        }
        let refuse = |reason: &str, parked: Option<(HardClass, bool)>| {
            let call = DeskCall { tool: &desk_tool, args: &desk, ok: false, result: reason, access, ui_changed: None, parked };
            write_cua_span(dir, &tool, &call);
            refusal_reply(&id, reason)
        };
        if !live {
            return refuse(&self.off_reason(gate), None);
        }
        if !in_manifest {
            return refuse(CUA_NOT_IN_MANIFEST, None);
        }
        let mut parked = None;
        match decide(Step::Desk { tool: &desk_tool, args: &desk }) {
            GateOutcome::Allow => {}
            GateOutcome::Refuse { reason } => return refuse(&reason, None),
            GateOutcome::Park { reason, hard, .. } => {
                let class = hard.unwrap_or(HardClass::IrreversibleOs);
                let approved = park_desk_call(dir, &desk_tool, &desk, class.as_str(), ComputerUseBackend::CuaDriver, halted);
                if halted() {
                    self.halt();
                }
                if !approved {
                    return refuse(&format!("Denied: {reason}. Jeremy did not approve it."), Some((class, false)));
                }
                parked = Some((class, true));
            }
        }
        if desk["text"].as_str().is_some_and(|t| t.contains(PASSWORD_SLOT)) {
            // No credential store yet: never type the slot or a guessed value.
            return refuse(CUA_NO_PASSWORD_MSG, parked);
        }
        let sent = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":tool,"arguments":args}}).to_string();
        let reply = match self.child().and_then(|c| c.request(&sent, &id)) {
            Ok(r) => r,
            Err(e) => return refuse(&e, parked),
        };
        let (ok, text) = reply_result(&reply);
        let ui_changed = match (before, ok) {
            (Some((pre, _)), true) => self.observe(&args).map(|(post, _)| post != pre),
            _ => None,
        };
        let call = DeskCall { tool: &desk_tool, args: &desk, ok, result: &text, access, ui_changed, parked };
        write_cua_span(dir, &tool, &call);
        reply
    }

    /// AX tree first, screenshot as fallback: a hash of what the window
    /// shows, and the reply's `result` (the tree the click target is read from).
    fn observe(&mut self, args: &Value) -> Option<(u64, Value)> {
        let mut scope = json!({});
        for k in ["pid", "window_id"] {
            if let Some(v) = args.get(k) {
                scope[k] = v.clone();
            }
        }
        for look in ["get_window_state", "screenshot"] {
            self.observe_n += 1;
            let id = Value::String(format!("grokhub-observe-{}", self.observe_n));
            let line = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":look,"arguments":scope}}).to_string();
            let Ok(reply) = self.child().and_then(|c| c.request(&line, &id)) else {
                return None;
            };
            let v: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
            if reply_result(&reply).0 {
                return Some((content_hash(&v["result"]["content"].to_string()), v["result"].clone()));
            }
        }
        None
    }
}

/// Spike-2b: the AX label and role of the control a Cua click names
/// (`element_index`), read from the `get_window_state` result taken just
/// before it, plus the window title for the card. `{"unknown":true}` when the
/// click names no element or the tree does not list it. The tree shape
/// (structured `elements` or `[N] role "label"` lines) is not yet checked
/// against a live 0.34.0 driver.
fn cua_target(state: Option<&Value>, args: &Value) -> Value {
    let index = ["element_index", "element", "index"].iter().find_map(|k| args.get(*k).and_then(Value::as_u64));
    let mut target = match state.zip(index).and_then(|(s, i)| tree_element(s, i)) {
        Some(el) => el,
        None => json!({"unknown": true}),
    };
    if let Some(w) = state.and_then(window_title) {
        target["window"] = Value::String(w);
    }
    target
}

/// One element of a window-state result by its index: role, label, effect.
fn tree_element(state: &Value, index: u64) -> Option<Value> {
    if let Some(el) = state.get("structuredContent").and_then(|s| structured_element(s, index)) {
        return Some(el);
    }
    let tag = format!("[{index}]");
    let texts = state["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str());
    for line in texts.flat_map(str::lines) {
        let Some(at) = line.find(&tag) else { continue };
        let rest = line[at + tag.len()..].trim();
        let role = rest.split_whitespace().next().unwrap_or("").trim_end_matches(':');
        let after = rest[role.len()..].trim().trim_start_matches(':').trim();
        let label = after.split('"').nth(1).unwrap_or(after);
        return Some(json!({"label": label, "role": role}));
    }
    None
}

fn structured_element(v: &Value, index: u64) -> Option<Value> {
    match v {
        Value::Object(m) => {
            let at = ["element_index", "index"].iter().find_map(|k| m.get(*k).and_then(Value::as_u64));
            if at == Some(index) {
                let pick = |keys: &[&str]| keys.iter().find_map(|k| m.get(*k).and_then(Value::as_str)).unwrap_or("");
                let mut el = json!({"label": pick(&["label", "title", "name", "description"]), "role": pick(&["role", "ax_role"])});
                if let Some(e) = m.get("effect").and_then(Value::as_str) {
                    el["effect"] = Value::String(e.into());
                }
                return Some(el);
            }
            m.values().find_map(|c| structured_element(c, index))
        }
        Value::Array(a) => a.iter().find_map(|c| structured_element(c, index)),
        _ => None,
    }
}

/// The window or app title a window-state result names, for the card.
fn window_title(state: &Value) -> Option<String> {
    let s = &state["structuredContent"];
    let structured = ["window_title", "title", "app_name", "app"].iter().find_map(|k| s.get(*k).and_then(Value::as_str));
    let text = || {
        state["content"].as_array()?.iter().filter_map(|c| c["text"].as_str()).flat_map(str::lines).find_map(|l| {
            let l = l.trim();
            l.strip_prefix("Window:").or_else(|| l.strip_prefix("window:")).map(str::trim)
        })
    };
    structured.or_else(text).filter(|t| !t.is_empty()).map(|t| t.chars().take(60).collect())
}

/// Keep only manifest tools in the child's `tools/list` reply.
fn manifest_tools_only(reply: &str) -> String {
    let mut v: Value = serde_json::from_str(reply).unwrap_or(Value::Null);
    if let Some(tools) = v.pointer_mut("/result/tools").and_then(Value::as_array_mut) {
        tools.retain(|t| t.get("name").and_then(Value::as_str).is_some_and(|n| CUA_TOOLS.contains(&n)));
    }
    v.to_string()
}

fn content_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn refusal_reply(id: &Value, msg: &str) -> String {
    json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":msg}],"isError":true}}).to_string()
}

fn error_reply(id: &Value, msg: &str) -> String {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":msg}}).to_string()
}

/// `(ok, first text)` of a reply.
fn reply_result(reply: &str) -> (bool, String) {
    let v: Value = serde_json::from_str(reply).unwrap_or(Value::Null);
    let r = &v["result"];
    let ok = v.get("error").is_none() && r.is_object() && !r["isError"].as_bool().unwrap_or(false);
    let text = r["content"][0]["text"].as_str().or_else(|| v["error"]["message"].as_str()).unwrap_or("").to_string();
    (ok, text)
}

/// The path A span for a Cua call, with `driver:"cua"` and the Cua tool in the claim.
fn write_cua_span(dir: &Path, tool: &str, call: &DeskCall<'_>) {
    let ctx = read_turn_context(dir);
    if let Some(mut span) = desk_span(call, &ctx.chat_id, ctx.turn) {
        span.driver = ComputerUseBackend::CuaDriver.as_str().into();
        span.claim = format!("cua {tool}");
        let _ = append_span(dir, &span.in_episode(&ctx.episode));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{answer_park, pending_parks, read_spans, span_path, test_dir, write_turn_context, TurnContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    const ON: CuaGate = CuaGate { enabled: true, flag: true, halted: false };

    /// Records every line the child receives; answers every call `ok`,
    /// and its window state changes once per click.
    #[derive(Clone, Default)]
    struct Fake {
        got: Rc<RefCell<Vec<Value>>>,
        killed: Rc<RefCell<bool>>,
        /// Window-state text the fake shows above its click count.
        tree: Rc<RefCell<String>>,
    }

    impl CuaChild for Fake {
        fn request(&mut self, line: &str, id: &Value) -> Result<String, String> {
            let v: Value = serde_json::from_str(line).unwrap();
            self.got.borrow_mut().push(v.clone());
            let clicks = self.got.borrow().iter().filter(|m| m["params"]["name"] == "click").count();
            let text = match v["params"]["name"].as_str() {
                Some("get_window_state") => format!("{}tree after {clicks} clicks", self.tree.borrow()),
                Some(t) => format!("{t} done"),
                None => "ok".into(),
            };
            Ok(json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":text}],"isError":false}}).to_string())
        }
        fn notify(&mut self, line: &str) {
            self.got.borrow_mut().push(serde_json::from_str(line).unwrap());
        }
        fn kill(&mut self) {
            *self.killed.borrow_mut() = true;
        }
    }

    fn proxy(fake: &Fake, starts: Rc<RefCell<u32>>) -> CuaProxy<Fake> {
        let f = fake.clone();
        CuaProxy::new(Box::new(move || {
            *starts.borrow_mut() += 1;
            Ok(f.clone())
        }))
    }

    fn call(tool: &str, args: Value) -> String {
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":tool,"arguments":args}}).to_string()
    }

    fn turn(dir: &Path) {
        write_turn_context(dir, &TurnContext { chat_id: "chat-c".into(), turn: 2, access: "supervised".into(), ..Default::default() }).unwrap();
    }

    fn text_of(reply: &str) -> String {
        reply_result(reply).1
    }

    fn sent_tools(fake: &Fake) -> Vec<String> {
        fake.got.borrow().iter().map(|m| m["params"]["name"].as_str().unwrap_or("").to_string()).collect()
    }

    fn cabin_answers(dir: &Path, approve: bool) -> std::thread::JoinHandle<Option<crate::harness::ParkRequest>> {
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            for _ in 0..500 {
                if let Some(req) = pending_parks(&dir).pop() {
                    let _ = answer_park(&dir, &req.id, approve);
                    return Some(req);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            None
        })
    }

    #[test]
    fn flag_off_starts_no_child_and_refuses_every_call() {
        let dir = test_dir("cua-flag-off");
        turn(&dir);
        let fake = Fake::default();
        let starts = Rc::new(RefCell::new(0));
        let mut p = proxy(&fake, starts.clone());
        let off = CuaGate { enabled: true, flag: false, halted: false };
        let out = p.handle_line(&call("click", json!({"x":10,"y":20})), off, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), CUA_OFF_MSG);
        let list = p.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, off, &dir, &mut || false).unwrap();
        assert_eq!(list, r#"{"id":1,"jsonrpc":"2.0","result":{"tools":[]}}"#);
        // Flag on, desktop switch off: still refused, still no child.
        let toggle_off = CuaGate { enabled: false, flag: true, halted: false };
        let out = p.handle_line(&call("click", json!({"x":10,"y":20})), toggle_off, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), grokhub_core::desktop_mcp::OFF_MSG);
        assert_eq!(*starts.borrow(), 0, "no child was started");
        assert!(fake.got.borrow().is_empty());
        let spans = read_spans(&dir, "chat-c").unwrap();
        let got: Vec<_> = spans.iter().map(|s| (s.decision.as_str(), s.driver.as_str(), s.access.as_str())).collect();
        assert_eq!(got, vec![("deny", "cua", "supervised"), ("deny", "cua", "readonly")]);
        assert_eq!(ComputerUseBackend::selected(false, true), ComputerUseBackend::GrokBuild);
        assert_eq!(ComputerUseBackend::selected(true, false), ComputerUseBackend::GrokBuild);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_child_started_after_the_switch_turns_on_gets_the_handshake_first() {
        let dir = test_dir("cua-late-init");
        turn(&dir);
        let fake = Fake::default();
        let starts = Rc::new(RefCell::new(0));
        let mut p = proxy(&fake, starts.clone());
        let off = CuaGate { enabled: false, flag: true, halted: false };
        let init = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}"#;
        assert!(p.handle_line(init, off, &dir, &mut || false).is_some());
        assert_eq!(*starts.borrow(), 0);
        // The switch goes on; the first call starts the child.
        let out = p.handle_line(&call("click", json!({"pid":42,"window_id":7,"x":10,"y":20})), ON, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), "click done");
        let methods: Vec<String> = fake.got.borrow().iter().map(|m| m["method"].as_str().unwrap_or("").to_string()).collect();
        assert_eq!(&methods[..3], &["initialize", "notifications/initialized", "tools/call"]);
        assert_eq!(*starts.borrow(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn soft_click_is_forwarded_once_with_a_cua_span_and_ui_changed() {
        let dir = test_dir("cua-click");
        turn(&dir);
        let fake = Fake::default();
        let starts = Rc::new(RefCell::new(0));
        let mut p = proxy(&fake, starts.clone());
        let out = p.handle_line(&call("click", json!({"pid":42,"window_id":7,"x":10,"y":20})), ON, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), "click done");
        assert_eq!(sent_tools(&fake), vec!["get_window_state", "click", "get_window_state"]);
        assert_eq!(fake.got.borrow()[1]["params"]["arguments"], json!({"pid":42,"window_id":7,"x":10,"y":20}));
        // The observe calls are scoped to the clicked window.
        assert_eq!(fake.got.borrow()[0]["params"]["arguments"], json!({"pid":42,"window_id":7}));
        let spans = read_spans(&dir, "chat-c").unwrap();
        assert_eq!(spans.len(), 1);
        let s = &spans[0];
        assert_eq!(
            (s.tool.as_str(), s.decision.as_str(), s.driver.as_str(), s.path.as_str(), s.claim.as_str(), s.ui_changed),
            ("click", "allow", "cua", "A", "cua click", Some(true))
        );
        assert_eq!(s.args_redacted, r#"{"pid":42,"window_id":7,"x":10,"y":20}"#);
        assert_eq!(*starts.borrow(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn typing_logs_only_the_length_and_a_credential_field_parks_before_forwarding() {
        let dir = test_dir("cua-type");
        turn(&dir);
        let fake = Fake::default();
        let mut p = proxy(&fake, Rc::new(RefCell::new(0)));
        let out = p.handle_line(&call("type_text", json!({"text":"quarterly notes"})), ON, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), "type_text done");
        let waiter = cabin_answers(&dir, false);
        let pin = call("type_text", json!({"text":"4821","element":{"role":"AXSecureTextField","label":"PIN"}}));
        let out = p.handle_line(&pin, ON, &dir, &mut || false).unwrap();
        let req = waiter.join().unwrap().expect("credential park posted");
        assert_eq!((req.class.as_str(), req.path.as_str(), req.tool.as_str()), ("credentials", "A", "type"));
        assert!(!req.action.contains("4821"), "{}", req.action);
        assert!(text_of(&out).starts_with("Denied: hard-class credentials"), "{}", text_of(&out));
        assert_eq!(sent_tools(&fake), vec!["type_text"], "the PIN never reached the child");
        let spans = read_spans(&dir, "chat-c").unwrap();
        let got: Vec<_> = spans.iter().map(|s| (s.decision.as_str(), s.approval_class.as_str(), s.args_redacted.as_str(), s.driver.as_str())).collect();
        assert_eq!(
            got,
            vec![
                ("allow", "soft", r#"{"chars":15}"#, "cua"),
                ("park", "credentials", r#"{"chars":4}"#, "cua"),
                ("deny", "credentials", r#"{"chars":4}"#, "cua"),
            ]
        );
        let trace = std::fs::read_to_string(span_path(&dir, "chat-c")).unwrap();
        assert!(!trace.contains("quarterly") && !trace.contains("4821"), "{trace}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn session_ending_keys_park_and_halt_or_ttl_send_nothing() {
        let dir = test_dir("cua-keys");
        turn(&dir);
        let fake = Fake::default();
        let mut p = proxy(&fake, Rc::new(RefCell::new(0)));
        // A halt while parked denies, kills the child, and refuses after.
        let out = p.handle_line(&call("hotkey", json!({"keys":["ctrl","alt","backspace"]})), ON, &dir, &mut || true).unwrap();
        assert!(text_of(&out).starts_with("Denied: hard-class irreversible_os"), "{}", text_of(&out));
        assert!(pending_parks(&dir).is_empty());
        let after = p.handle_line(&call("click", json!({"x":1,"y":1})), ON, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&after), grokhub_core::desktop_mcp::HALT_MSG);
        assert!(fake.got.borrow().is_empty(), "the child received nothing: {:?}", fake.got.borrow());
        let spans = read_spans(&dir, "chat-c").unwrap();
        let got: Vec<_> = spans.iter().map(|s| (s.tool.as_str(), s.decision.as_str(), s.approval_class.as_str())).collect();
        assert_eq!(got, vec![("key", "park", "irreversible_os"), ("key", "deny", "irreversible_os"), ("click", "deny", "soft")]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn deny_sends_nothing_approve_forwards_once_and_the_password_slot_is_never_typed() {
        let dir = test_dir("cua-deny");
        turn(&dir);
        let fake = Fake::default();
        let mut p = proxy(&fake, Rc::new(RefCell::new(0)));
        let waiter = cabin_answers(&dir, false);
        let out = p.handle_line(&call("press_key", json!({"key":"ctrl+alt+delete"})), ON, &dir, &mut || false).unwrap();
        assert!(waiter.join().unwrap().is_some());
        assert!(text_of(&out).starts_with("Denied:"));
        assert!(fake.got.borrow().is_empty());
        let waiter = cabin_answers(&dir, true);
        let out = p.handle_line(&call("press_key", json!({"key":"ctrl+alt+delete"})), ON, &dir, &mut || false).unwrap();
        assert!(waiter.join().unwrap().is_some());
        assert_eq!(text_of(&out), "press_key done");
        assert_eq!(sent_tools(&fake), vec!["press_key"]);
        // The model only ever sees the slot; with no saved password nothing is typed.
        let waiter = cabin_answers(&dir, true);
        let out = p.handle_line(&call("type_text", json!({"text":"%password%"})), ON, &dir, &mut || false).unwrap();
        let req = waiter.join().unwrap().expect("slot parks as credentials");
        assert_eq!(req.class, "credentials");
        assert_eq!(text_of(&out), CUA_NO_PASSWORD_MSG);
        assert_eq!(sent_tools(&fake), vec!["press_key"]);
        // Floor and manifest refusals never reach the child either.
        let out = p.handle_line(&call("type_text", json!({"text":"rm -rf /"})), ON, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), "hard floor: rm -rf /");
        let out = p.handle_line(&call("shell_execute", json!({"command":"ls"})), ON, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), CUA_NOT_IN_MANIFEST);
        assert_eq!(sent_tools(&fake), vec!["press_key"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Hermes #3, #4, #8, #9 through the fake Cua child under Full: fields,
    /// drafts, toggles and local saves stay soft; Pay, Send, Reset and Upload
    /// park before the click is sent, and Deny or Halt never clicks.
    #[test]
    fn hermes_clicks_park_by_their_ax_label_before_the_click_is_sent() {
        let dir = test_dir("cua-hermes");
        write_turn_context(&dir, &TurnContext { chat_id: "chat-h".into(), turn: 1, access: "full".into(), ..Default::default() }).unwrap();
        let fake = Fake::default();
        *fake.tree.borrow_mut() = [
            "Window: Checkout — Shop",
            "[1] AXTextField \"Name on card\"",
            "[2] AXTextField \"Address\"",
            "[3] AXButton \"Pay now\"",
            "[4] AXTextArea \"Message body\"",
            "[5] AXButton \"Send\"",
            "[6] AXSwitch \"Dark mode\"",
            "[7] AXButton \"Reset all settings\"",
            "[8] AXButton \"Save\"",
            "[9] AXButton \"Upload screenshot\"",
            "",
        ]
        .join("\n");
        let mut p = proxy(&fake, Rc::new(RefCell::new(0)));
        let click = |i: u64| call("click", json!({"pid": 42, "window_id": 7, "element_index": i}));
        let parked = |p: &mut CuaProxy<Fake>, i: u64, approve: bool| {
            let waiter = cabin_answers(&dir, approve);
            let out = p.handle_line(&click(i), ON, &dir, &mut || false).unwrap();
            (waiter.join().unwrap().expect("park posted"), text_of(&out))
        };
        // #3: typed fields are soft; Pay parks as money.
        for (i, text) in [(1, "Jeremy Example"), (2, "1 Example Road")] {
            let out = p.handle_line(&call("type_text", json!({"element_index": i, "text": text})), ON, &dir, &mut || false).unwrap();
            assert_eq!(text_of(&out), "type_text done");
        }
        let (req, out) = parked(&mut p, 3, false);
        assert_eq!((req.class.as_str(), req.action.as_str()), ("money", "Grok wants to click Pay in Checkout — Shop"));
        assert!(out.starts_with("Denied: hard-class money"), "{out}");
        // #4: the draft is soft; Send parks as send.
        let out = p.handle_line(&call("type_text", json!({"element_index": 4, "text": "See you at 5"})), ON, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), "type_text done");
        let (req, _) = parked(&mut p, 5, false);
        assert_eq!((req.class.as_str(), req.action.as_str()), ("send", "Grok wants to click Send in Checkout — Shop"));
        // #8: the toggle is soft and its change is seen; Reset parks, and Halt denies it.
        let out = p.handle_line(&click(6), ON, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), "click done");
        let out = p.handle_line(&click(7), ON, &dir, &mut || true).unwrap();
        assert!(text_of(&out).starts_with("Denied: hard-class irreversible_os"), "{}", text_of(&out));
        let mut p = proxy(&fake, Rc::new(RefCell::new(0)));
        // #9: Save is soft; Upload parks as send.
        let out = p.handle_line(&click(8), ON, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), "click done");
        let (req, _) = parked(&mut p, 9, false);
        assert_eq!((req.class.as_str(), req.action.as_str()), ("send", "Grok wants to click Upload in Checkout — Shop"));
        // Only the soft clicks ever reached the child.
        let clicked: Vec<Value> = fake
            .got
            .borrow()
            .iter()
            .filter(|m| m["params"]["name"] == "click")
            .map(|m| m["params"]["arguments"]["element_index"].clone())
            .collect();
        assert_eq!(clicked, vec![json!(6), json!(8)]);
        let spans = read_spans(&dir, "chat-h").unwrap();
        let clicks: Vec<_> = spans
            .iter()
            .filter(|s| s.tool == "click")
            .map(|s| (s.decision.as_str(), s.target.as_str(), s.target_rule.as_str(), s.access.as_str()))
            .collect();
        assert_eq!(
            clicks,
            vec![
                ("park", "ax", "money:Pay", "full"),
                ("deny", "ax", "money:Pay", "full"),
                ("park", "ax", "send:Send", "full"),
                ("deny", "ax", "send:Send", "full"),
                ("allow", "ax", "", "full"),
                ("park", "ax", "irreversible_os:Reset", "full"),
                ("deny", "ax", "irreversible_os:Reset", "full"),
                ("allow", "ax", "", "full"),
                ("park", "ax", "send:Upload", "full"),
                ("deny", "ax", "send:Upload", "full"),
            ]
        );
        let toggle = spans.iter().find(|s| s.tool == "click" && s.decision == "allow").unwrap();
        assert_eq!(toggle.ui_changed, Some(true), "the toggle is verified by a fresh look");
        let trace = std::fs::read_to_string(span_path(&dir, "chat-h")).unwrap();
        for never in ["Pay now", "Upload screenshot", "Reset all settings", "Checkout — Shop", "Jeremy Example", "See you at 5"] {
            assert!(!trace.contains(never), "{never} reached a span: {trace}");
        }
        // Enter never approves a hard card; Esc denies.
        assert_eq!(crate::harness::hard_card_key(true, false, false), None);
        assert_eq!(crate::harness::hard_card_key(false, true, false), Some(crate::harness::HardAnswer::Deny));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_click_the_tree_does_not_list_is_soft_with_an_unknown_target() {
        let dir = test_dir("cua-unknown");
        turn(&dir);
        let fake = Fake::default();
        let mut p = proxy(&fake, Rc::new(RefCell::new(0)));
        let out = p.handle_line(&call("click", json!({"element_index": 99})), ON, &dir, &mut || false).unwrap();
        assert_eq!(text_of(&out), "click done");
        let s = read_spans(&dir, "chat-c").unwrap().pop().unwrap();
        assert_eq!((s.decision.as_str(), s.target.as_str(), s.target_rule.as_str()), ("allow", "unknown", ""));
        // A structured tree names the control and its declared effect.
        let state = json!({"structuredContent": {"title": "Mail", "elements": [{"element_index": 3, "role": "AXButton", "label": "Go", "effect": "send"}]}});
        assert_eq!(
            cua_target(Some(&state), &json!({"element_index": 3})),
            json!({"label": "Go", "role": "AXButton", "effect": "send", "window": "Mail"})
        );
        assert_eq!(cua_target(Some(&state), &json!({"x": 1, "y": 2})), json!({"unknown": true, "window": "Mail"}));
        assert_eq!(cua_target(None, &json!({"element_index": 3})), json!({"unknown": true}));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tools_list_is_cut_to_the_manifest() {
        let reply = r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[{"name":"click"},{"name":"shell_execute"},{"name":"page"}]}}"#;
        assert_eq!(manifest_tools_only(reply), r#"{"id":3,"jsonrpc":"2.0","result":{"tools":[{"name":"click"}]}}"#);
    }

    #[test]
    fn spawn_env_is_bounded_with_an_approved_cabin_manifest() {
        let manifest = Path::new("cfg").join("cua").join("manifest.yaml");
        let env = cua_spawn_env(&manifest);
        assert_eq!(
            env,
            vec![
                ("CUA_DRIVER_PERMISSION_MODE", "bounded".to_string()),
                ("CUA_DRIVER_CAPABILITY_MANIFEST_FILE", manifest.display().to_string()),
                ("CUA_DRIVER_CAPABILITY_MANIFEST_APPROVED", "1".to_string()),
            ]
        );
        assert!(!env.iter().any(|(_, v)| v == "standard" || v == "unrestricted"));
        assert_eq!(CUA_ENV_REMOVE, ["CUA_DRIVER_DANGEROUSLY_BYPASS_APPROVALS", "CUA_DRIVER_POLICY_FILE"]);
        let sock = Path::new("cfg").join("cua").join("driver.sock");
        assert_eq!(cua_socket_path(Path::new("cfg")), sock);
        assert_eq!(cua_spawn_args(&sock), vec!["mcp".to_string(), "--socket".into(), sock.display().to_string()]);
        let m = cua_manifest();
        assert!(m.starts_with("version: 1\nmode: bounded\nresources:\n  applications:\n    - bundle_id: org.gnome.TextEditor\n"), "{m}");
        assert!(m.contains("allow:\n  tools:\n    - start_session\n"), "{m}");
        assert!(m.ends_with("deny:\n  tools:\n    - shell_execute\n    - page\n    - browser_download\n    - browser_set_input_files\n"), "{m}");
    }

    #[test]
    fn only_the_pinned_mit_release_passes() {
        assert_eq!(check_cua_version("cua-driver 0.34.0"), Ok(()));
        assert_eq!(check_cua_version("cua-driver-rs v0.34.0 (abc123)\n"), Ok(()));
        assert_eq!(
            check_cua_version("cua-driver 0.33.4"),
            Err("Cua Driver 0.33.4 is installed, but GrokHub only runs the pinned 0.34.0 (cua-driver-rs-v0.34.0). Install that release.".into())
        );
        assert_eq!(
            check_cua_version("cua-driver 0.34.0 +perception (AGPL-3.0-only)"),
            Err("This Cua Driver build includes the AGPL perception extension. GrokHub only runs the MIT Driver.".into())
        );
        assert_eq!(check_cua_version("hello"), Err("GrokHub could not read the Cua Driver version, so it did not start it.".into()));
        assert_eq!(CUA_LINUX_SHA256.len(), 64);
        assert_eq!(CUA_LINUX_ASSET, "cua-driver-rs-0.34.0-linux-x86_64.tar.gz");
        assert_eq!(find_cua_driver(&Path::new("no-such-dir").join("cua-driver").display().to_string()), None);
    }

    #[test]
    fn cua_names_map_to_the_desk_shapes() {
        assert_eq!(cua_as_desk("hotkey", &json!({"keys":["ctrl","alt","delete"]})).unwrap(), ("key", json!({"keys":"ctrl+alt+delete"})));
        assert_eq!(cua_as_desk("press_key", &json!({"key":"Return"})).unwrap(), ("key", json!({"key":"Return","keys":"Return"})));
        assert_eq!(
            cua_as_desk("launch_app", &json!({"bundle_id":"org.kde.kate"})).unwrap(),
            ("open_app", json!({"bundle_id":"org.kde.kate","app":"org.kde.kate"}))
        );
        assert_eq!(cua_as_desk("set_value", &json!({"value":"x"})).unwrap(), ("type", json!({"value":"x","text":"x"})));
        assert_eq!(cua_as_desk("type_text", &json!({"text":"a %password% b"})).unwrap().1["secret"], json!(true));
        assert_eq!(cua_as_desk("get_window_state", &json!({})), None);
        assert_eq!(cua_as_desk("click", &json!({"x":1})), None);
    }
}
