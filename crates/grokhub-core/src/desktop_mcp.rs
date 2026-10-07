//! Pure desktop MCP: geometry, key combos, JSON-RPC, and the permission argv.
//!
//! The cabin process owns the screen. This module never opens a display.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// MCP server name. grok exposes tools as `grokhub-desktop__<tool>`.
pub const DESKTOP_MCP_SERVER: &str = "grokhub-desktop";

/// grok permission rule for every desktop tool.
pub const DESKTOP_MCP_RULE: &str = "MCPTool(grokhub-desktop__*)";

/// Spike-2a: the cabin's gate proxy in front of Cua Driver (`grokhub --mcp-cua`).
pub const CUA_MCP_SERVER: &str = "grokhub-cua";

/// grok permission rule for every Cua proxy tool. Denied wherever the desktop rule is.
pub const CUA_MCP_RULE: &str = "MCPTool(grokhub-cua__*)";

/// The cabin's own computer-use MCP servers. Both gate every call in the cabin.
pub const CABIN_CU_SERVERS: &[&str] = &[DESKTOP_MCP_SERVER, CUA_MCP_SERVER];

const PREFERRED_PROTOCOL: &str = "2025-06-18";
const PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

pub const OFF_MSG: &str = "Desktop control is off. Turn on Settings → Let Grok control the desktop.";
pub const HALT_MSG: &str =
    "Desktop control was halted (Ctrl+Alt+H). Open GrokHub again to use the screen.";
pub const LOCK_MSG: &str = "The lock screen is up. Unlock this computer, then try again.";

const DRAG_STEPS: i32 = 8;

/// What a backend says when it cannot open, focus, or list windows here.
pub const APPS_MSG: &str = "Opening and focusing apps is not available on this desktop.";
/// What a backend says when it has no trash or Recycle Bin route.
pub const TRASH_MSG: &str = "Moving to the trash is not available on this desktop.";
/// Most paths one `delete_files` call may name.
pub const DELETE_FILES_CAP: usize = 100;

const COORD_NOTE: &str = "Coordinates are pixels in the last screenshot of that monitor (or \"all\"), not physical screen pixels. With no screenshot yet, they are the monitor's native pixels.";

/// One monitor in physical pixels. Origins may be negative on a virtual desktop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonitorGeom {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub primary: bool,
}

/// Last screenshot of one monitor, or of the whole desktop (`id` `"all"`).
/// `scale_factor` is DPI. Mapping uses physical size over image size, not DPI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShotGeom {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub physical_w: u32,
    pub physical_h: u32,
    pub scale_factor: f64,
    pub image_w: u32,
    pub image_h: u32,
}

impl ShotGeom {
    pub fn native(mon: &MonitorGeom) -> Self {
        Self {
            id: mon.id.clone(),
            x: mon.x,
            y: mon.y,
            physical_w: mon.width,
            physical_h: mon.height,
            scale_factor: mon.scale_factor,
            image_w: mon.width,
            image_h: mon.height,
        }
    }

    /// Physical pixels per image pixel. 1 when the shot was not downscaled.
    pub fn scale(&self) -> f64 {
        let img = if self.image_w == 0 {
            self.physical_w
        } else {
            self.image_w
        };
        if img == 0 {
            1.0
        } else {
            self.physical_w as f64 / img as f64
        }
    }
}

/// Bounding box of every monitor. Image size matches the physical box.
pub fn union_monitor(mons: &[MonitorGeom]) -> Option<ShotGeom> {
    let first = mons.first()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x.saturating_add(first.width as i32);
    let mut max_y = first.y.saturating_add(first.height as i32);
    let mut scale = first.scale_factor;
    for m in mons {
        min_x = min_x.min(m.x);
        min_y = min_y.min(m.y);
        max_x = max_x.max(m.x.saturating_add(m.width as i32));
        max_y = max_y.max(m.y.saturating_add(m.height as i32));
        if m.primary {
            scale = m.scale_factor;
        }
    }
    let w = max_x.saturating_sub(min_x).max(0) as u32;
    let h = max_y.saturating_sub(min_y).max(0) as u32;
    Some(ShotGeom {
        id: "all".into(),
        x: min_x,
        y: min_y,
        physical_w: w,
        physical_h: h,
        scale_factor: if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        },
        image_w: w,
        image_h: h,
    })
}

/// `screen = origin + round((s + 0.5) * phys / img - 0.5)`, clamped to the monitor.
/// A zero image size is treated as native pixels. DPI is not part of this.
pub fn map_screenshot_point(geom: &ShotGeom, sx: f64, sy: f64) -> (i32, i32) {
    let x = map_axis(geom.x, sx, geom.physical_w, geom.image_w);
    let y = map_axis(geom.y, sy, geom.physical_h, geom.image_h);
    (x, y)
}

fn map_axis(origin: i32, s: f64, phys: u32, img: u32) -> i32 {
    if phys == 0 {
        return origin;
    }
    let img = if img == 0 { phys } else { img };
    let raw = (s + 0.5) * (phys as f64) / (img as f64) - 0.5;
    let off = round_i32(raw);
    let screen = origin.saturating_add(off);
    let hi = origin.saturating_add(phys as i32).saturating_sub(1);
    screen.clamp(origin.min(hi), origin.max(hi))
}

fn round_i32(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    let r = v.round();
    if r >= i32::MAX as f64 {
        i32::MAX
    } else if r <= i32::MIN as f64 {
        i32::MIN
    } else {
        r as i32
    }
}

/// Fit inside 1920 on the long side and 2_400_000 pixels. Already-small stays.
/// A zero side stays zero. Rounding that still exceeds a cap shrinks the longer side.
pub fn fit_downscale(w: u32, h: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (w, h);
    }
    let long = w.max(h) as f64;
    let pixels = (w as f64) * (h as f64);
    let by_long = 1920.0 / long;
    let by_px = (2_400_000.0 / pixels).sqrt();
    let scale = 1.0_f64.min(by_long).min(by_px);
    if scale >= 1.0 {
        return (w, h);
    }
    let mut nw = ((w as f64) * scale).floor().max(1.0) as u32;
    let mut nh = ((h as f64) * scale).floor().max(1.0) as u32;
    if nw == 0 {
        nw = 1;
    }
    if nh == 0 {
        nh = 1;
    }
    while nw.max(nh) > 1920 || (nw as u64) * (nh as u64) > 2_400_000 {
        if nw >= nh && nw > 1 {
            nw -= 1;
        } else if nh > 1 {
            nh -= 1;
        } else {
            break;
        }
    }
    (nw, nh)
}

/// A halt stamp newer than the server's start refuses tools and ends the process.
/// An equal stamp is not a halt (the server may have written nothing itself).
pub fn stamp_halts(stamp_ms: u64, started_ms: u64) -> bool {
    stamp_ms > started_ms
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyName {
    Char(char),
    Return,
    Escape,
    Backspace,
    Tab,
    Space,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
    F(u8),
    Super,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: KeyName,
}

/// `"Return"`, `"ctrl+shift+t"`, `"alt+F4"`, `"super"`, arrows, F1–F24, one character.
/// An uppercase ASCII letter implies Shift. A lone Super is the key, not a modifier.
pub fn parse_key_combo(raw: &str) -> Result<KeyCombo, String> {
    let parts: Vec<&str> = raw
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Err("Key combo is empty.".into());
    }
    if parts.len() == 1 {
        let (key, shift) = parse_key_token(parts[0])?;
        return Ok(KeyCombo {
            ctrl: false,
            alt: false,
            shift,
            super_key: false,
            key,
        });
    }
    let mut combo = KeyCombo {
        ctrl: false,
        alt: false,
        shift: false,
        super_key: false,
        key: KeyName::Space,
    };
    for p in &parts[..parts.len() - 1] {
        match modifier_kind(p) {
            Some(ModKind::Ctrl) => combo.ctrl = true,
            Some(ModKind::Alt) => combo.alt = true,
            Some(ModKind::Shift) => combo.shift = true,
            Some(ModKind::Super) => combo.super_key = true,
            None => return Err(format!("Unknown modifier \"{p}\".")),
        }
    }
    let last = parts[parts.len() - 1];
    if modifier_kind(last) == Some(ModKind::Super) {
        combo.key = KeyName::Super;
        return Ok(combo);
    }
    if modifier_kind(last).is_some() {
        return Err("Key combo needs a key.".into());
    }
    let (key, implied) = parse_key_token(last)?;
    if implied {
        combo.shift = true;
    }
    combo.key = key;
    Ok(combo)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModKind {
    Ctrl,
    Alt,
    Shift,
    Super,
}

fn modifier_kind(token: &str) -> Option<ModKind> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "control_l" | "ctrl_l" => Some(ModKind::Ctrl),
        "alt" | "alt_l" | "option" => Some(ModKind::Alt),
        "shift" | "shift_l" => Some(ModKind::Shift),
        "super" | "super_l" | "meta" | "win" | "cmd" => Some(ModKind::Super),
        _ => None,
    }
}

fn parse_key_token(token: &str) -> Result<(KeyName, bool), String> {
    let lower = token.to_ascii_lowercase();
    let named = match lower.as_str() {
        "return" | "enter" | "kp_enter" => Some(KeyName::Return),
        "esc" | "escape" => Some(KeyName::Escape),
        "backspace" | "bs" => Some(KeyName::Backspace),
        "tab" => Some(KeyName::Tab),
        "space" | "spacebar" => Some(KeyName::Space),
        "delete" | "del" => Some(KeyName::Delete),
        "insert" | "ins" => Some(KeyName::Insert),
        "home" => Some(KeyName::Home),
        "end" => Some(KeyName::End),
        "pageup" | "page_up" | "pgup" | "prior" => Some(KeyName::PageUp),
        "pagedown" | "page_down" | "pgdn" | "next" => Some(KeyName::PageDown),
        "left" | "arrow_left" => Some(KeyName::Left),
        "right" | "arrow_right" => Some(KeyName::Right),
        "up" | "arrow_up" => Some(KeyName::Up),
        "down" | "arrow_down" => Some(KeyName::Down),
        "super" | "super_l" | "meta" | "win" | "cmd" => Some(KeyName::Super),
        _ => None,
    };
    if let Some(key) = named {
        return Ok((key, false));
    }
    if let Some(rest) = lower.strip_prefix('f') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            let n: u32 = rest.parse().unwrap_or(0);
            if (1..=24).contains(&n) {
                return Ok((KeyName::F(n as u8), false));
            }
            return Err("F keys only go from F1 to F24.".into());
        }
    }
    let mut chars = token.chars();
    let ch = chars.next().ok_or_else(|| format!("Unknown key \"{token}\"."))?;
    if chars.next().is_some() {
        return Err(format!("Unknown key \"{token}\"."));
    }
    if ch.is_ascii_alphabetic() && ch.is_ascii_uppercase() {
        return Ok((KeyName::Char(ch.to_ascii_lowercase()), true));
    }
    Ok((KeyName::Char(ch), false))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Encoded image plus the geometry the next click maps through.
#[derive(Clone, Debug, PartialEq)]
pub struct CapturedShot {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub geom: ShotGeom,
}

/// Screen driver. Tests pass a fake. The app process supplies X11, Wayland, or Windows.
pub trait DesktopBackend {
    fn list_monitors(&mut self) -> Result<Vec<MonitorGeom>, String>;
    fn screenshot(&mut self, monitor: &str) -> Result<CapturedShot, String>;
    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String>;
    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String>;
    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String>;
    fn type_text(&mut self, text: &str) -> Result<(), String>;
    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String>;
    fn is_locked(&mut self) -> bool;
    /// Pause between a double-click's halves and between drag steps.
    fn pace(&mut self) {}
    /// Close a portal session and latch it shut. Halt and the lock screen call this.
    /// The default does nothing so X11, Windows, and fakes stay quiet.
    fn release_input(&mut self) {}
    /// Close a session without latching it. The desktop switch uses this so
    /// turning the switch back on can open a new session.
    fn suspend_input(&mut self) {}
    /// Move using the screenshot geometry. The default ignores `geom` and
    /// calls [`move_abs`](DesktopBackend::move_abs) with the mapped point.
    fn move_abs_on(&mut self, geom: &ShotGeom, x: i32, y: i32) -> Result<(), String> {
        let _ = geom;
        self.move_abs(x, y)
    }
    /// Sticky note for the next input reply (imprecise fallback, portal retry).
    fn status_note(&mut self) -> Option<String> {
        None
    }
    /// Where the pointer landed, when the backend can see it. `None` means
    /// the Test button must say the offset was not measured.
    fn landed_at(&mut self) -> Option<(i32, i32)> {
        None
    }
    /// Launch an app by the id [`app_id`] returns.
    fn open_app(&mut self, app: &str) -> Result<(), String> {
        let _ = app;
        Err(APPS_MSG.into())
    }
    /// Bring the first window whose title contains `title` (any case) to the
    /// front. Returns that window's title.
    fn focus_window(&mut self, title: &str) -> Result<String, String> {
        let _ = title;
        Err(APPS_MSG.into())
    }
    fn list_windows(&mut self) -> Result<DesktopWindows, String> {
        Err(APPS_MSG.into())
    }
    /// Move these paths to the trash (Linux) or the Recycle Bin (Windows).
    fn trash(&mut self, paths: &[std::path::PathBuf]) -> Result<(), String> {
        let _ = paths;
        Err(TRASH_MSG.into())
    }
    /// Spike-2b: the accessible control at a screen point, read-only. `None`
    /// means unknown (no reader, a timeout, or no control there). The
    /// default (Windows, fakes) is unknown: no UIA reader (D3).
    fn target_at(&mut self, x: i32, y: i32) -> Option<ClickTarget> {
        let _ = (x, y);
        None
    }
}

/// The accessible label and role of the control under a click (Spike-2b).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClickTarget {
    pub label: String,
    pub role: String,
}

/// Top-level windows and the focused one, for the before/after check of
/// `open_app` and `focus_window`. `active` is the window class and title.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopWindows {
    pub titles: Vec<String>,
    pub active: String,
}

/// An app id `open_app` takes: a Linux desktop id (`org.kde.dolphin`, with or
/// without `.desktop`) or a Windows app name (`notepad`). Letters, digits,
/// `.`, `_`, and `-` only, so it can never be read as a flag, a path, or a
/// second command. Returns the id without `.desktop`.
pub fn app_id(raw: &str) -> Result<String, String> {
    let id = raw.trim();
    let id = id.strip_suffix(".desktop").unwrap_or(id);
    let ok = !id.is_empty()
        && id.len() <= 128
        && !id.starts_with(['-', '.'])
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(id.to_string())
    } else {
        Err(format!("\"{}\" is not an app id. Use a desktop id like org.kde.dolphin or an app name like notepad.", raw.trim()))
    }
}

/// Re-read on every `tools/call`. Halt also ends the process after the reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallGate {
    pub enabled: bool,
    pub halted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcOutcome {
    pub reply: Option<String>,
    pub exit: bool,
}

pub struct DesktopServer<B: DesktopBackend> {
    version: String,
    backend: B,
    shots: HashMap<String, ShotGeom>,
}

impl<B: DesktopBackend> DesktopServer<B> {
    pub fn new(version: impl Into<String>, backend: B) -> Self {
        Self {
            version: version.into(),
            backend,
            shots: HashMap::new(),
        }
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn handle_line(&mut self, line: &str, gate: CallGate) -> RpcOutcome {
        let line = line.trim();
        if line.is_empty() {
            return RpcOutcome {
                reply: None,
                exit: false,
            };
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                return RpcOutcome {
                    reply: Some(rpc_error(Value::Null, -32700, "Parse error")),
                    exit: false,
                };
            }
        };
        let Some(obj) = value.as_object() else {
            return RpcOutcome {
                reply: Some(rpc_error(Value::Null, -32602, "Invalid params")),
                exit: false,
            };
        };
        let id = obj.get("id").cloned().filter(|v| !v.is_null());
        let method = obj.get("method").and_then(|m| m.as_str());
        let Some(method) = method else {
            if id.is_none() {
                return RpcOutcome {
                    reply: None,
                    exit: false,
                };
            }
            return RpcOutcome {
                reply: Some(rpc_error(id.unwrap_or(Value::Null), -32602, "Invalid params")),
                exit: false,
            };
        };
        if method.starts_with("notifications/") || id.is_none() {
            return RpcOutcome {
                reply: None,
                exit: false,
            };
        }
        let id = id.unwrap_or(Value::Null);
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        match method {
            "initialize" => self.initialize(&id, &params),
            "ping" => RpcOutcome {
                reply: Some(rpc_result(&id, json!({}))),
                exit: false,
            },
            "tools/list" => RpcOutcome {
                reply: Some(rpc_result(&id, json!({ "tools": tool_schemas() }))),
                exit: false,
            },
            "tools/call" => self.tools_call(&id, &params, gate),
            _ => RpcOutcome {
                reply: Some(rpc_error(id, -32601, "Method not found")),
                exit: false,
            },
        }
    }

    fn initialize(&self, id: &Value, params: &Value) -> RpcOutcome {
        let Some(obj) = params.as_object() else {
            return RpcOutcome {
                reply: Some(rpc_error(id.clone(), -32602, "Invalid params")),
                exit: false,
            };
        };
        let Some(version) = obj.get("protocolVersion").and_then(|v| v.as_str()) else {
            return RpcOutcome {
                reply: Some(rpc_error(id.clone(), -32602, "Invalid params")),
                exit: false,
            };
        };
        let negotiated = if PROTOCOLS.contains(&version) {
            version
        } else {
            PREFERRED_PROTOCOL
        };
        RpcOutcome {
            reply: Some(rpc_result(
                id,
                json!({
                    "protocolVersion": negotiated,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": DESKTOP_MCP_SERVER, "version": self.version },
                }),
            )),
            exit: false,
        }
    }

    fn tools_call(&mut self, id: &Value, params: &Value, gate: CallGate) -> RpcOutcome {
        let Some(obj) = params.as_object() else {
            return RpcOutcome {
                reply: Some(rpc_error(id.clone(), -32602, "Invalid params")),
                exit: false,
            };
        };
        let Some(name) = obj.get("name").and_then(|v| v.as_str()) else {
            return RpcOutcome {
                reply: Some(rpc_error(id.clone(), -32602, "Invalid params")),
                exit: false,
            };
        };
        let args = match obj.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(v) if v.is_object() => v.clone(),
            Some(_) => {
                return tool_fail(id, "arguments must be an object", false);
            }
        };
        if !gate.enabled {
            self.backend.suspend_input();
            return tool_fail(id, OFF_MSG, false);
        }
        if gate.halted {
            self.backend.release_input();
            return tool_fail(id, HALT_MSG, true);
        }
        if is_input_tool(name) && self.backend.is_locked() {
            self.backend.release_input();
            return tool_fail(id, LOCK_MSG, false);
        }
        match self.invoke(name, &args) {
            Ok(body) => RpcOutcome {
                reply: Some(rpc_result(id, body)),
                exit: false,
            },
            Err(msg) => tool_fail(id, &msg, false),
        }
    }

    /// Run one desktop tool. Callers apply the switch, halt, and lock gates first.
    pub fn invoke(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "list_monitors" => self.tool_list_monitors(),
            "screenshot" => self.tool_screenshot(args),
            "click" => self.tool_click(args),
            "move" => self.tool_move(args),
            "drag" => self.tool_drag(args),
            "scroll" => self.tool_scroll(args),
            "type" => self.tool_type(args),
            "key" => self.tool_key(args),
            "open_app" => self.tool_open_app(args),
            "focus_window" => self.tool_focus_window(args),
            "delete_files" => self.tool_delete_files(args),
            other => Err(format!("Unknown tool \"{other}\".")),
        }
    }

    fn tool_list_monitors(&mut self) -> Result<Value, String> {
        let mons = self.backend.list_monitors()?;
        let rows: Vec<Value> = mons.iter().map(monitor_json).collect();
        let text = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into());
        Ok(json!({
            "content": [{ "type": "text", "text": text }],
            "structuredContent": { "monitors": rows },
            "isError": false,
        }))
    }

    fn tool_screenshot(&mut self, args: &Value) -> Result<Value, String> {
        let which = monitor_arg(args);
        let shot = self.backend.screenshot(&which)?;
        let key = if which.is_empty() { "all" } else { which.as_str() };
        self.shots.insert(key.to_string(), shot.geom.clone());
        if shot.geom.id != key {
            self.shots.insert(shot.geom.id.clone(), shot.geom.clone());
        }
        let geom = geom_json(&shot.geom);
        let text = serde_json::to_string(&geom).unwrap_or_else(|_| "{}".into());
        Ok(json!({
            "content": [
                {
                    "type": "image",
                    "data": b64(&shot.bytes),
                    "mimeType": shot.mime,
                },
                { "type": "text", "text": text },
            ],
            "structuredContent": geom,
            "isError": false,
        }))
    }

    /// Spike-2b: the control a `click` with these args would land on, read
    /// before it runs. `None` when the point or monitor is bad or the
    /// backend can't tell.
    pub fn click_target(&mut self, args: &Value) -> Option<ClickTarget> {
        let (x, y) = require_xy(args, "x", "y").ok()?;
        let geom = self.shot_for(&monitor_arg(args)).ok()?;
        let (sx, sy) = map_screenshot_point(&geom, x, y);
        self.backend.target_at(sx, sy)
    }

    fn tool_click(&mut self, args: &Value) -> Result<Value, String> {
        let (x, y) = require_xy(args, "x", "y")?;
        let button = button_arg(args)?;
        let double = args.get("double").and_then(|v| v.as_bool()).unwrap_or(false);
        let which = monitor_arg(args);
        let geom = self.shot_for(&which)?;
        let (sx, sy) = map_screenshot_point(&geom, x, y);
        self.backend.move_abs_on(&geom, sx, sy)?;
        self.backend.button(button, true)?;
        self.backend.button(button, false)?;
        if double {
            self.backend.pace();
            self.backend.button(button, true)?;
            self.backend.button(button, false)?;
        }
        Ok(self.input_ok("clicked"))
    }

    fn tool_move(&mut self, args: &Value) -> Result<Value, String> {
        let (x, y) = require_xy(args, "x", "y")?;
        let which = monitor_arg(args);
        let geom = self.shot_for(&which)?;
        let (sx, sy) = map_screenshot_point(&geom, x, y);
        self.backend.move_abs_on(&geom, sx, sy)?;
        Ok(self.input_ok("moved"))
    }

    fn tool_drag(&mut self, args: &Value) -> Result<Value, String> {
        let (x0, y0) = require_xy(args, "from_x", "from_y")?;
        let (x1, y1) = require_xy(args, "to_x", "to_y")?;
        let button = button_arg(args)?;
        let which = monitor_arg(args);
        let geom = self.shot_for(&which)?;
        let (sx0, sy0) = map_screenshot_point(&geom, x0, y0);
        let (sx1, sy1) = map_screenshot_point(&geom, x1, y1);
        self.backend.move_abs_on(&geom, sx0, sy0)?;
        self.backend.button(button, true)?;
        let mut moved = Ok(());
        for step in 1..=DRAG_STEPS {
            let t = step as f64 / DRAG_STEPS as f64;
            let x = round_i32(sx0 as f64 + (sx1 - sx0) as f64 * t);
            let y = round_i32(sy0 as f64 + (sy1 - sy0) as f64 * t);
            moved = self.backend.move_abs_on(&geom, x, y);
            if moved.is_err() {
                break;
            }
            if step != DRAG_STEPS {
                self.backend.pace();
            }
        }
        // Let go even when a move failed, so the user's button is not left held.
        let up = self.backend.button(button, false);
        moved?;
        up?;
        Ok(self.input_ok("dragged"))
    }

    fn tool_scroll(&mut self, args: &Value) -> Result<Value, String> {
        let (x, y) = require_xy(args, "x", "y")?;
        let dx = args.get("dx").and_then(Value::as_f64).unwrap_or(0.0);
        let dy = args.get("dy").and_then(Value::as_f64).unwrap_or(0.0);
        let which = monitor_arg(args);
        let geom = self.shot_for(&which)?;
        let (sx, sy) = map_screenshot_point(&geom, x, y);
        self.backend.move_abs_on(&geom, sx, sy)?;
        self.backend.scroll(round_i32(dx), round_i32(dy))?;
        Ok(self.input_ok("scrolled"))
    }

    fn tool_type(&mut self, args: &Value) -> Result<Value, String> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "type needs text.".to_string())?;
        self.backend.type_text(text)?;
        Ok(self.input_ok("typed"))
    }

    fn tool_key(&mut self, args: &Value) -> Result<Value, String> {
        let keys = args
            .get("keys")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "key needs keys.".to_string())?;
        let combo = parse_key_combo(keys)?;
        self.backend.key_combo(&combo)?;
        Ok(self.input_ok("keyed"))
    }

    fn tool_open_app(&mut self, args: &Value) -> Result<Value, String> {
        let raw = args
            .get("app")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "open_app needs app.".to_string())?;
        let id = app_id(raw)?;
        self.backend.open_app(&id)?;
        Ok(text_ok(&format!("opened {id}")))
    }

    fn tool_focus_window(&mut self, args: &Value) -> Result<Value, String> {
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| "focus_window needs title.".to_string())?;
        let got = self.backend.focus_window(title)?;
        Ok(text_ok(&format!("focused {got}")))
    }

    /// The real delete behind a hard card (Spike-1b). Every path is checked
    /// before anything is touched, so a bad path deletes nothing. Folders must
    /// be empty: the card names each path, never what is inside one.
    fn tool_delete_files(&mut self, args: &Value) -> Result<Value, String> {
        let paths = delete_paths(args)?;
        let n = paths.len();
        let noun = if n == 1 { "path" } else { "paths" };
        if args.get("to_trash").and_then(|v| v.as_bool()) == Some(true) {
            self.backend.trash(&paths)?;
            return Ok(text_ok(&format!("moved {n} {noun} to the trash")));
        }
        for p in &paths {
            let meta = std::fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let gone = if meta.is_dir() { std::fs::remove_dir(p) } else { std::fs::remove_file(p) };
            gone.map_err(|e| format!("{}: {e}", p.display()))?;
        }
        Ok(text_ok(&format!("deleted {n} {noun}")))
    }

    fn input_ok(&mut self, verb: &str) -> Value {
        match self.backend.status_note() {
            Some(note) if !note.is_empty() => text_ok(&format!("{verb}. {note}")),
            _ => text_ok(verb),
        }
    }

    fn shot_for(&mut self, monitor: &str) -> Result<ShotGeom, String> {
        let key = if monitor.is_empty() { "all" } else { monitor };
        if let Some(g) = self.shots.get(key) {
            return Ok(g.clone());
        }
        let mons = self.backend.list_monitors()?;
        if key == "all" {
            return union_monitor(&mons).ok_or_else(|| "No monitors.".to_string());
        }
        mons.iter()
            .find(|m| m.id == key || m.name == key)
            .map(ShotGeom::native)
            .ok_or_else(|| format!("No monitor \"{key}\"."))
    }
}

fn is_input_tool(name: &str) -> bool {
    matches!(name, "click" | "move" | "drag" | "scroll" | "type" | "key" | "open_app" | "focus_window")
}

/// `delete_files` paths: 1 to [`DELETE_FILES_CAP`], absolute, no repeats, each
/// one there, and a folder only when empty.
fn delete_paths(args: &Value) -> Result<Vec<std::path::PathBuf>, String> {
    let list = args
        .get("paths")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "delete_files needs paths.".to_string())?;
    if list.is_empty() || list.len() > DELETE_FILES_CAP {
        return Err(format!("delete_files takes 1 to {DELETE_FILES_CAP} paths."));
    }
    let mut out: Vec<std::path::PathBuf> = Vec::with_capacity(list.len());
    for v in list {
        let raw = v.as_str().ok_or_else(|| "paths must be strings.".to_string())?;
        let p = std::path::PathBuf::from(raw);
        if !p.is_absolute() {
            return Err(format!("{raw}: use the full path."));
        }
        if out.contains(&p) {
            return Err(format!("{raw} is listed twice."));
        }
        let meta = std::fs::symlink_metadata(&p).map_err(|e| format!("{raw}: {e}"))?;
        if meta.is_dir() && std::fs::read_dir(&p).map_err(|e| format!("{raw}: {e}"))?.next().is_some() {
            return Err(format!("{raw} is a folder with files in it. List the files to delete."));
        }
        out.push(p);
    }
    Ok(out)
}

fn monitor_arg(args: &Value) -> String {
    match args.get("monitor") {
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                "all".into()
            } else {
                t.to_string()
            }
        }
        Some(Value::Number(n)) => n.to_string(),
        _ => "all".into(),
    }
}

fn require_xy(args: &Value, xk: &str, yk: &str) -> Result<(f64, f64), String> {
    let x = args
        .get(xk)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{xk} is required."))?;
    let y = args
        .get(yk)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{yk} is required."))?;
    if !x.is_finite() || !y.is_finite() {
        return Err(format!("{xk} and {yk} must be numbers."));
    }
    Ok((x, y))
}

fn button_arg(args: &Value) -> Result<MouseButton, String> {
    match args.get("button").and_then(|v| v.as_str()).unwrap_or("left") {
        "left" => Ok(MouseButton::Left),
        "right" => Ok(MouseButton::Right),
        "middle" => Ok(MouseButton::Middle),
        other => Err(format!("button must be left, right, or middle (got \"{other}\").")),
    }
}

fn text_ok(text: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false,
    })
}

fn tool_fail(id: &Value, msg: &str, exit: bool) -> RpcOutcome {
    RpcOutcome {
        reply: Some(rpc_result(
            id,
            json!({
                "content": [{ "type": "text", "text": msg }],
                "isError": true,
            }),
        )),
        exit,
    }
}

fn rpc_result(id: &Value, result: Value) -> String {
    let mut body = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    if let Some(obj) = body.as_object_mut() {
        obj.insert("id".into(), id.clone());
    }
    body.to_string()
}

fn rpc_error(id: Value, code: i32, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

/// JSON-RPC parse error for a line the stdio reader could not keep.
pub fn rpc_parse_error() -> String {
    rpc_error(Value::Null, -32700, "Parse error")
}

fn monitor_json(m: &MonitorGeom) -> Value {
    json!({
        "id": m.id,
        "name": m.name,
        "x": m.x,
        "y": m.y,
        "width": m.width,
        "height": m.height,
        "scale_factor": m.scale_factor,
        "primary": m.primary,
    })
}

fn geom_json(g: &ShotGeom) -> Value {
    json!({
        "id": g.id,
        "x": g.x,
        "y": g.y,
        "physical_w": g.physical_w,
        "physical_h": g.physical_h,
        "scale_factor": g.scale_factor,
        "image_w": g.image_w,
        "image_h": g.image_h,
        "scale": g.scale(),
    })
}

fn tool_schemas() -> Vec<Value> {
    let monitor = json!({
        "type": "string",
        "description": "Monitor id from list_monitors, or \"all\" for the whole desktop. Defaults to \"all\".",
    });
    let button = json!({
        "type": "string",
        "enum": ["left", "right", "middle"],
        "description": "Mouse button. Defaults to left.",
    });
    vec![
        tool(
            "list_monitors",
            "List monitors. Each has id, name, origin x/y, physical width and height, and DPI scale_factor. Click coordinates are not these physical pixels; they are pixels in the last screenshot.",
            json!({}),
            &[],
        ),
        tool(
            "screenshot",
            &format!("Capture the screen as PNG, or JPEG when the PNG is large. {COORD_NOTE} The text block is geometry: monitor id, origin, physical size, scale_factor, image size, and scale (physical/image)."),
            json!({ "monitor": monitor }),
            &[],
        ),
        tool(
            "click",
            &format!("Click. {COORD_NOTE} double repeats the click."),
            json!({
                "x": { "type": "number" },
                "y": { "type": "number" },
                "button": button,
                "double": { "type": "boolean", "description": "Click twice." },
                "monitor": monitor,
            }),
            &["x", "y"],
        ),
        tool(
            "move",
            &format!("Move the pointer. {COORD_NOTE}"),
            json!({
                "x": { "type": "number" },
                "y": { "type": "number" },
                "monitor": monitor,
            }),
            &["x", "y"],
        ),
        tool(
            "drag",
            &format!("Press, move in steps, and release. {COORD_NOTE}"),
            json!({
                "from_x": { "type": "number" },
                "from_y": { "type": "number" },
                "to_x": { "type": "number" },
                "to_y": { "type": "number" },
                "button": button,
                "monitor": monitor,
            }),
            &["from_x", "from_y", "to_x", "to_y"],
        ),
        tool(
            "scroll",
            &format!("Move the pointer, then scroll by wheel notches. Positive dy is up. Positive dx is right. {COORD_NOTE}"),
            json!({
                "x": { "type": "number" },
                "y": { "type": "number" },
                "dx": { "type": "number", "description": "Horizontal notches. Positive is right." },
                "dy": { "type": "number", "description": "Vertical notches. Positive is up." },
                "monitor": monitor,
            }),
            &["x", "y"],
        ),
        tool(
            "type",
            "Type Unicode text by injecting keystrokes. Does not read or paste the clipboard.",
            json!({ "text": { "type": "string" } }),
            &["text"],
        ),
        tool(
            "key",
            "Press a key combo, for example Return, ctrl+shift+t, alt+F4, super, Page_Down, or an arrow.",
            json!({ "keys": { "type": "string" } }),
            &["keys"],
        ),
        tool(
            "open_app",
            "Open an app. On Linux pass its desktop id (org.kde.dolphin, firefox); on Windows its app name (notepad, msedge).",
            json!({ "app": { "type": "string" } }),
            &["app"],
        ),
        tool(
            "focus_window",
            "Bring the first window whose title contains this text to the front.",
            json!({ "title": { "type": "string" } }),
            &["title"],
        ),
        tool(
            "delete_files",
            "Delete files or empty folders by full path, or move them to the trash / Recycle Bin with to_trash. Always asks the user first, naming every path.",
            json!({
                "paths": { "type": "array", "items": { "type": "string" } },
                "to_trash": { "type": "boolean", "description": "Move to the trash or Recycle Bin instead of deleting." },
            }),
            &["paths"],
        ),
    ]
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        },
    })
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = data.len() - i;
    if rem == 1 {
        let n = (data[i] as u32) << 16;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopPermMode {
    Ask,
    Auto,
    Always,
}

/// Extra grok argv for one spawn. Attended Ask adds nothing.
/// Disabled, and unattended Ask, deny the desktop rule. Auto and Always allow it.
pub fn desktop_mcp_args(mode: DesktopPermMode, attended: bool, enabled: bool) -> Vec<String> {
    apply_desktop_mcp_args(Vec::new(), mode, attended, enabled)
}

/// Deny wins: a `--deny` of the desktop rule is never paired with `--allow`.
/// An attended Ask chat is left unchanged, so ACP can still ask.
pub fn apply_desktop_mcp_args(
    mut args: Vec<String>,
    mode: DesktopPermMode,
    attended: bool,
    enabled: bool,
) -> Vec<String> {
    if enabled && matches!(mode, DesktopPermMode::Ask) && attended {
        return args;
    }
    let want_deny = !enabled || matches!(mode, DesktopPermMode::Ask);
    if want_deny || has_rule_flag(&args, "--deny") {
        strip_rule_flag(&mut args, "--allow");
        if !has_rule_flag(&args, "--deny") {
            args.push("--deny".into());
            args.push(DESKTOP_MCP_RULE.into());
        }
        return args;
    }
    if !has_rule_flag(&args, "--allow") {
        args.push("--allow".into());
        args.push(DESKTOP_MCP_RULE.into());
    }
    args
}

fn has_rule_flag(args: &[String], flag: &str) -> bool {
    args.windows(2)
        .any(|w| w[0] == flag && w[1] == DESKTOP_MCP_RULE)
}

fn strip_rule_flag(args: &mut Vec<String>, flag: &str) {
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i] == flag && args[i + 1] == DESKTOP_MCP_RULE {
            args.remove(i);
            args.remove(i);
            continue;
        }
        i += 1;
    }
}

/// One libei absolute region, in logical pixels. Origins may be negative.
#[derive(Clone, Debug, PartialEq)]
pub struct EisRegion {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Absolute pointer position in the same logical space as the EIS region
/// offset. `motion_absolute` must fall inside a region, so the far edge clamps
/// to `offset + size - 1` and a shared edge belongs to the region that starts
/// there. Negative origins stay negative.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EisAbsolute {
    pub region: usize,
    pub x: f32,
    pub y: f32,
}

fn positive_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// Image pixel → physical (the existing formula, origin 0) → logical (`÷ scale`)
/// → output logical origin → the EIS region that contains the point.
/// `motion_absolute` uses that same logical space, clamped into the region:
/// the far edge is `offset + size - 1`, and a shared edge belongs to the
/// region that starts there. Negative origins stay negative.
pub fn map_image_px_to_eis(
    geom: &ShotGeom,
    regions: &[EisRegion],
    sx: f64,
    sy: f64,
) -> Option<EisAbsolute> {
    if regions.is_empty() {
        return None;
    }
    let scale = positive_scale(geom.scale_factor);
    let phys_x = map_axis(0, sx, geom.physical_w, geom.image_w);
    let phys_y = map_axis(0, sy, geom.physical_h, geom.image_h);
    let global_x = geom.x as f64 + (phys_x as f64) / scale;
    let global_y = geom.y as f64 + (phys_y as f64) / scale;
    Some(pick_eis_region(regions, global_x, global_y))
}

/// Mapped screen point (`origin + physical pixel`) → EIS absolute.
pub fn mapped_point_to_eis(
    geom: &ShotGeom,
    regions: &[EisRegion],
    x: i32,
    y: i32,
) -> Option<EisAbsolute> {
    if regions.is_empty() {
        return None;
    }
    let scale = positive_scale(geom.scale_factor);
    let phys_x = x.saturating_sub(geom.x);
    let phys_y = y.saturating_sub(geom.y);
    let global_x = geom.x as f64 + (phys_x as f64) / scale;
    let global_y = geom.y as f64 + (phys_y as f64) / scale;
    Some(pick_eis_region(regions, global_x, global_y))
}

/// Half-open region pick. An empty list is not called by the mappers.
pub fn pick_eis_region(regions: &[EisRegion], x: f64, y: f64) -> EisAbsolute {
    if let Some((index, region)) = regions
        .iter()
        .enumerate()
        .find(|(_, region)| region_contains(region, x, y))
    {
        return region_local(index, region, x, y);
    }
    let Some((index, region)) = regions.iter().enumerate().min_by(|(_, a), (_, b)| {
        region_dist2(a, x, y)
            .partial_cmp(&region_dist2(b, x, y))
            .unwrap_or(std::cmp::Ordering::Equal)
    }) else {
        return EisAbsolute {
            region: 0,
            x: 0.0,
            y: 0.0,
        };
    };
    region_local(index, region, x, y)
}

fn region_contains(region: &EisRegion, x: f64, y: f64) -> bool {
    let x0 = region.x as f64;
    let y0 = region.y as f64;
    let x1 = x0 + region.width as f64;
    let y1 = y0 + region.height as f64;
    x >= x0 && x < x1 && y >= y0 && y < y1
}

fn region_dist2(region: &EisRegion, x: f64, y: f64) -> f64 {
    let x0 = region.x as f64;
    let y0 = region.y as f64;
    let x1 = x0 + region.width as f64;
    let y1 = y0 + region.height as f64;
    let dx = x - x.clamp(x0, x1);
    let dy = y - y.clamp(y0, y1);
    dx * dx + dy * dy
}

fn region_local(index: usize, region: &EisRegion, x: f64, y: f64) -> EisAbsolute {
    EisAbsolute {
        region: index,
        x: clamp_into_region(x, region.x, region.width),
        y: clamp_into_region(y, region.y, region.height),
    }
}

/// Keep a logical coordinate inside `[origin, origin + size - 1]`.
fn clamp_into_region(value: f64, origin: i32, dim: u32) -> f32 {
    if !value.is_finite() {
        return origin as f32;
    }
    let lo = origin as f64;
    if dim == 0 {
        return lo as f32;
    }
    let hi = lo + (dim as f64) - 1.0;
    value.clamp(lo, hi) as f32
}

/// How this process should drive the desktop. A pure function of the session
/// environment and probes, so CI can cover KDE, wlroots, and X11 without a bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopInputKind {
    Portal,
    Ydotool,
    X11rb,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopCaptureKind {
    ScreenShot2,
    Grim,
    X11rb,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopMonitorKind {
    Kscreen,
    Wlroots,
    X11Randr,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopStack {
    pub input: DesktopInputKind,
    pub capture: DesktopCaptureKind,
    pub monitors: DesktopMonitorKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesktopSessionEnv {
    pub wayland_display: Option<String>,
    pub session_type: Option<String>,
    pub current_desktop: Option<String>,
    pub display: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DesktopProbes {
    pub kwin: bool,
}

pub fn current_desktop_is_kde(desktop: Option<&str>) -> bool {
    desktop.is_some_and(|value| {
        value
            .split(':')
            .any(|part| part.trim().eq_ignore_ascii_case("kde"))
    })
}

pub fn select_desktop_stack(env: &DesktopSessionEnv, probes: &DesktopProbes) -> DesktopStack {
    let wayland = crate::session_is_wayland(
        env.wayland_display.as_deref(),
        env.session_type.as_deref(),
    );
    if wayland && (current_desktop_is_kde(env.current_desktop.as_deref()) || probes.kwin) {
        return DesktopStack {
            input: DesktopInputKind::Portal,
            capture: DesktopCaptureKind::ScreenShot2,
            monitors: DesktopMonitorKind::Kscreen,
        };
    }
    if wayland {
        return DesktopStack {
            input: DesktopInputKind::Ydotool,
            capture: DesktopCaptureKind::Grim,
            monitors: DesktopMonitorKind::Wlroots,
        };
    }
    if env
        .display
        .as_deref()
        .is_some_and(|display| !display.trim().is_empty())
    {
        return DesktopStack {
            input: DesktopInputKind::X11rb,
            capture: DesktopCaptureKind::X11rb,
            monitors: DesktopMonitorKind::X11Randr,
        };
    }
    DesktopStack {
        input: DesktopInputKind::None,
        capture: DesktopCaptureKind::None,
        monitors: DesktopMonitorKind::None,
    }
}

/// KWin `org.kde.KWin.ScreenShot2` result dictionary.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenShot2Meta {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub scale: f64,
}

pub fn parse_screenshot2_metadata(value: &Value) -> Result<ScreenShot2Meta, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "ScreenShot2 metadata is not an object.".to_string())?;
    let width = json_u32(obj.get("width")).ok_or_else(|| "ScreenShot2 metadata has no width.".to_string())?;
    let height = json_u32(obj.get("height")).ok_or_else(|| "ScreenShot2 metadata has no height.".to_string())?;
    if width == 0 || height == 0 {
        return Err("ScreenShot2 returned an empty image.".into());
    }
    let stride = json_u32(obj.get("stride")).unwrap_or_else(|| width.saturating_mul(4));
    if (stride as u64) < (width as u64) * 4 {
        return Err("ScreenShot2 stride is shorter than one row.".into());
    }
    let scale = json_f64(obj.get("scale")).unwrap_or(1.0);
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    Ok(ScreenShot2Meta {
        width,
        height,
        stride,
        scale,
    })
}

/// Qt `Format_ARGB32_Premultiplied` is little-endian B,G,R,A with the colors
/// already multiplied by alpha. Straight RGBA un-premultiplies each channel.
pub fn argb32_premul_to_rgba(
    src: &[u8],
    width: u32,
    height: u32,
    stride: u32,
) -> Result<Vec<u8>, String> {
    let row = (width as usize).saturating_mul(4);
    if (stride as usize) < row {
        return Err("ScreenShot2 stride is shorter than one row.".into());
    }
    let need = (stride as usize).saturating_mul(height as usize);
    if src.len() < need {
        return Err("ScreenShot2 buffer is shorter than width, height, and stride.".into());
    }
    let mut out = vec![0u8; row.saturating_mul(height as usize)];
    for y in 0..height as usize {
        let src_row = &src[y * stride as usize..];
        let dst_off = y * row;
        for x in 0..width as usize {
            let i = x * 4;
            let b = src_row[i] as u32;
            let g = src_row[i + 1] as u32;
            let r = src_row[i + 2] as u32;
            let a = src_row[i + 3] as u32;
            let (r, g, b) = if a == 0 {
                (0, 0, 0)
            } else {
                (unpremultiply(r, a), unpremultiply(g, a), unpremultiply(b, a))
            };
            out[dst_off + i] = r;
            out[dst_off + i + 1] = g;
            out[dst_off + i + 2] = b;
            out[dst_off + i + 3] = a as u8;
        }
    }
    Ok(out)
}

fn unpremultiply(channel: u32, alpha: u32) -> u8 {
    let value = (channel.saturating_mul(255) + alpha / 2) / alpha;
    value.min(255) as u8
}

/// `kscreen-doctor -j`. Disabled, disconnected, and zero-size outputs are skipped.
/// `pos` is the logical origin. `size` is the oriented pixel size KScreen serializes.
pub fn parse_kscreen_doctor(text: &str) -> Result<Vec<MonitorGeom>, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("kscreen-doctor: {e}"))?;
    let outputs = value
        .get("outputs")
        .and_then(Value::as_array)
        .ok_or_else(|| "kscreen-doctor JSON has no outputs array.".to_string())?;
    let mut monitors = Vec::new();
    for output in outputs {
        let Some(obj) = output.as_object() else {
            continue;
        };
        if obj.get("connected").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        if obj.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        let Some((width, height)) = json_size(obj.get("size")) else {
            continue;
        };
        if width == 0 || height == 0 {
            continue;
        }
        let (x, y) = json_pos(obj.get("pos")).unwrap_or((0, 0));
        let scale = positive_scale(json_f64(obj.get("scale")).unwrap_or(1.0));
        let priority = obj.get("priority").and_then(json_i64).unwrap_or(0);
        let primary = obj.get("primary").and_then(Value::as_bool).unwrap_or(false) || priority == 1;
        let id = match obj.get("id") {
            Some(Value::String(id)) => id.clone(),
            Some(Value::Number(id)) => id.to_string(),
            _ => name.to_string(),
        };
        monitors.push(MonitorGeom {
            id,
            name: name.to_string(),
            x,
            y,
            width,
            height,
            scale_factor: scale,
            primary,
        });
    }
    if monitors.is_empty() {
        return Err("kscreen-doctor listed no enabled outputs.".into());
    }
    Ok(monitors)
}

/// EIS regions win for logical origin. Physical size and scale stay with kscreen
/// when the output name matches. Used once the portal session is up.
pub fn join_kscreen_with_eis(monitors: &[MonitorGeom], regions: &[EisRegion]) -> Vec<MonitorGeom> {
    if regions.is_empty() {
        return monitors.to_vec();
    }
    regions
        .iter()
        .enumerate()
        .map(|(index, region)| {
            let matched = monitors.iter().find(|monitor| {
                !region.name.is_empty()
                    && (monitor.name == region.name || monitor.id == region.name)
            });
            let scale = matched.map(|monitor| monitor.scale_factor).unwrap_or(1.0);
            let scale = positive_scale(scale);
            let (width, height) = matched
                .map(|monitor| (monitor.width, monitor.height))
                .unwrap_or_else(|| {
                    (
                        ((region.width as f64) * scale).round().max(1.0) as u32,
                        ((region.height as f64) * scale).round().max(1.0) as u32,
                    )
                });
            MonitorGeom {
                id: matched
                    .map(|monitor| monitor.id.clone())
                    .unwrap_or_else(|| (index + 1).to_string()),
                name: if region.name.is_empty() {
                    matched
                        .map(|monitor| monitor.name.clone())
                        .unwrap_or_else(|| format!("output-{index}"))
                } else {
                    region.name.clone()
                },
                x: region.x,
                y: region.y,
                width,
                height,
                scale_factor: scale,
                primary: matched.map(|monitor| monitor.primary).unwrap_or(index == 0),
            }
        })
        .collect()
}

/// Prefer EIS `TEXT`. Otherwise each character needs a keycode. A missing
/// character is an error (route 2 keysyms are out of scope).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EisTextPlan {
    Text,
    Keycodes(Vec<u16>),
}

pub fn plan_eis_text(
    text_offered: bool,
    text: &str,
    mut keycode: impl FnMut(char) -> Option<u16>,
) -> Result<EisTextPlan, String> {
    if text_offered {
        return Ok(EisTextPlan::Text);
    }
    let mut codes = Vec::new();
    for ch in text.chars() {
        match keycode(ch) {
            Some(code) => codes.push(code),
            None => {
                return Err(format!(
                    "Character {ch:?} is missing from the keymap. EIS TEXT is not offered."
                ));
            }
        }
    }
    Ok(EisTextPlan::Keycodes(codes))
}

include!("desktop_routes.rs");

fn json_u32(value: Option<&Value>) -> Option<u32> {
    let value = value?;
    if let Some(n) = value.as_u64() {
        return u32::try_from(n).ok();
    }
    if let Some(n) = value.as_i64() {
        return u32::try_from(n).ok();
    }
    if let Some(n) = value.as_f64() {
        if n.is_finite() && n >= 0.0 && n <= u32::MAX as f64 {
            return Some(n.round() as u32);
        }
    }
    None
}

fn json_f64(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

fn json_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| value.as_f64().and_then(|n| if n.is_finite() { Some(n.round() as i64) } else { None }))
}

fn json_size(value: Option<&Value>) -> Option<(u32, u32)> {
    let obj = value?.as_object()?;
    Some((json_u32(obj.get("width"))?, json_u32(obj.get("height"))?))
}

fn json_pos(value: Option<&Value>) -> Option<(i32, i32)> {
    let obj = value?.as_object()?;
    let x = json_i64(obj.get("x")?)?;
    let y = json_i64(obj.get("y")?)?;
    Some((i32::try_from(x).unwrap_or(0), i32::try_from(y).unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: &str, x: i32, y: i32, w: u32, h: u32, scale: f64, primary: bool) -> MonitorGeom {
        MonitorGeom {
            id: id.into(),
            name: id.into(),
            x,
            y,
            width: w,
            height: h,
            scale_factor: scale,
            primary,
        }
    }

    fn shot(x: i32, y: i32, pw: u32, ph: u32, iw: u32, ih: u32, dpi: f64) -> ShotGeom {
        ShotGeom {
            id: "m".into(),
            x,
            y,
            physical_w: pw,
            physical_h: ph,
            scale_factor: dpi,
            image_w: iw,
            image_h: ih,
        }
    }

    #[test]
    fn map_identity_and_edges() {
        let g = shot(0, 0, 100, 80, 100, 80, 1.0);
        assert_eq!(map_screenshot_point(&g, 0.0, 0.0), (0, 0));
        assert_eq!(map_screenshot_point(&g, 50.0, 40.0), (50, 40));
        assert_eq!(map_screenshot_point(&g, 99.0, 79.0), (99, 79));
    }

    #[test]
    fn map_two_x_downscale_rounds_half_away_from_zero() {
        let g = shot(0, 0, 200, 200, 100, 100, 1.0);
        assert_eq!(map_screenshot_point(&g, 0.0, 0.0), (1, 1));
        assert_eq!(map_screenshot_point(&g, 1.0, 1.0), (3, 3));
        assert_eq!(map_screenshot_point(&g, 99.0, 99.0), (199, 199));
    }

    #[test]
    fn map_odd_image_and_clamps() {
        let g = shot(10, 20, 100, 50, 33, 17, 2.0);
        let (x, y) = map_screenshot_point(&g, 0.0, 0.0);
        assert!((10..110).contains(&x), "{x}");
        assert!((20..70).contains(&y), "{y}");
        assert_eq!(map_screenshot_point(&g, -40.0, 10_000.0), (10, 69));
        assert_eq!(map_screenshot_point(&g, 10_000.0, -5.0), (109, 20));
        let zero = shot(5, 6, 0, 40, 10, 0, 1.5);
        assert_eq!(map_screenshot_point(&zero, 3.0, 3.0).0, 5);
    }

    #[test]
    fn map_negative_origin_second_monitor() {
        let g = shot(-1920, 0, 1920, 1080, 960, 540, 1.0);
        assert_eq!(map_screenshot_point(&g, 0.0, 0.0).0, -1919);
        let last = map_screenshot_point(&g, 959.0, 539.0);
        assert_eq!(last.0, -1);
        assert!(last.1 <= 1079 && last.1 >= 0);
        assert_eq!(map_screenshot_point(&g, -8.0, 0.0).0, -1920);
    }

    #[test]
    fn map_ignores_dpi_scale_factor() {
        for dpi in [1.25_f64, 1.5, 2.0] {
            let g = shot(0, 0, 2000, 1000, 1000, 500, dpi);
            assert!((g.scale() - 2.0).abs() < 1e-9);
            assert_eq!(g.scale_factor, dpi);
            assert_eq!(map_screenshot_point(&g, 0.0, 0.0), (1, 1));
            assert_eq!(map_screenshot_point(&g, 999.0, 499.0), (1999, 999));
        }
    }

    #[test]
    fn union_keeps_negative_origin() {
        let mons = vec![
            mon("left", -1920, 0, 1920, 1080, 1.0, false),
            mon("main", 0, 0, 1920, 1080, 1.5, true),
        ];
        let u = union_monitor(&mons).unwrap();
        assert_eq!(u.x, -1920);
        assert_eq!(u.physical_w, 3840);
        assert_eq!(u.physical_h, 1080);
        assert_eq!(u.image_w, 3840);
        assert!((u.scale_factor - 1.5).abs() < 1e-9);
        assert!(union_monitor(&[]).is_none());
    }

    #[test]
    fn downscale_fit_identity_4k_square_and_zero() {
        assert_eq!(fit_downscale(100, 80), (100, 80));
        assert_eq!(fit_downscale(1920, 1080), (1920, 1080));
        assert_eq!(fit_downscale(3840, 2160), (1920, 1080));
        let square = fit_downscale(2000, 2000);
        assert_eq!(square, (1549, 1549));
        assert!((square.0 as u64) * (square.1 as u64) <= 2_400_000);
        assert_eq!(fit_downscale(0, 100), (0, 100));
        assert_eq!(fit_downscale(100, 0), (100, 0));
        let (w, h) = fit_downscale(1921, 1081);
        assert!(w <= 1920 && h <= 1920);
        assert!((w as u64) * (h as u64) <= 2_400_000);
        assert!(w >= 1 && h >= 1);
    }

    #[test]
    fn key_combos_parse() {
        assert_eq!(parse_key_combo("Return").unwrap().key, KeyName::Return);
        assert_eq!(parse_key_combo("enter").unwrap().key, KeyName::Return);
        let t = parse_key_combo("ctrl+shift+t").unwrap();
        assert!(t.ctrl && t.shift && !t.alt);
        assert_eq!(t.key, KeyName::Char('t'));
        let upper = parse_key_combo("ctrl+T").unwrap();
        assert!(upper.ctrl && upper.shift);
        assert_eq!(upper.key, KeyName::Char('t'));
        let lower = parse_key_combo("ctrl+t").unwrap();
        assert!(lower.ctrl && !lower.shift);
        let f4 = parse_key_combo("alt+F4").unwrap();
        assert!(f4.alt && !f4.shift);
        assert_eq!(f4.key, KeyName::F(4));
        assert_eq!(parse_key_combo("super").unwrap().key, KeyName::Super);
        assert!(!parse_key_combo("super").unwrap().super_key);
        let sup = parse_key_combo("super+a").unwrap();
        assert!(sup.super_key);
        assert_eq!(sup.key, KeyName::Char('a'));
        assert_eq!(parse_key_combo("Page_Down").unwrap().key, KeyName::PageDown);
        assert_eq!(parse_key_combo("pgdn").unwrap().key, KeyName::PageDown);
        assert_eq!(parse_key_combo("arrow_left").unwrap().key, KeyName::Left);
        assert_eq!(parse_key_combo("F24").unwrap().key, KeyName::F(24));
        assert_eq!(parse_key_combo("F1").unwrap().key, KeyName::F(1));
        let letter = parse_key_combo("T").unwrap();
        assert!(letter.shift);
        assert_eq!(letter.key, KeyName::Char('t'));
        assert_eq!(parse_key_combo("a").unwrap().key, KeyName::Char('a'));
        assert!(parse_key_combo("ctrl+shift").is_err());
        assert!(parse_key_combo("F0").is_err());
        assert!(parse_key_combo("F25").is_err());
        assert!(parse_key_combo("").is_err());
        assert!(parse_key_combo("nope").is_err());
    }

    #[test]
    fn base64_one_byte() {
        assert_eq!(b64(&[0xff]), "/w==");
        assert_eq!(b64(&[]), "");
        assert_eq!(b64(b"Man"), "TWFu");
    }

    #[derive(Default)]
    struct Fake {
        monitors: Vec<MonitorGeom>,
        locked: bool,
        fail: Option<String>,
        log: Vec<String>,
        fail_moves_while_down: bool,
        down: bool,
    }

    impl DesktopBackend for Fake {
        fn list_monitors(&mut self) -> Result<Vec<MonitorGeom>, String> {
            self.log.push("list".into());
            if let Some(e) = &self.fail {
                return Err(e.clone());
            }
            Ok(self.monitors.clone())
        }
        fn screenshot(&mut self, monitor: &str) -> Result<CapturedShot, String> {
            self.log.push(format!("shot:{monitor}"));
            if let Some(e) = &self.fail {
                return Err(e.clone());
            }
            let geom = if monitor == "all" {
                union_monitor(&self.monitors).unwrap_or_else(|| shot(0, 0, 200, 100, 100, 50, 1.25))
            } else {
                self.monitors
                    .iter()
                    .find(|m| m.id == monitor)
                    .map(|m| {
                        let mut g = ShotGeom::native(m);
                        g.image_w = m.width / 2;
                        g.image_h = m.height / 2;
                        g
                    })
                    .unwrap_or_else(|| shot(0, 0, 200, 100, 100, 50, 1.5))
            };
            Ok(CapturedShot {
                bytes: vec![0xff],
                mime: "image/png".into(),
                geom,
            })
        }
        fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
            self.log.push(format!("move:{x},{y}"));
            if self.fail_moves_while_down && self.down {
                return Err("input desktop changed".into());
            }
            Ok(())
        }
        fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
            self.log.push(format!("btn:{button:?}:{down}"));
            self.down = down;
            Ok(())
        }
        fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
            self.log.push(format!("scroll:{dx},{dy}"));
            Ok(())
        }
        fn type_text(&mut self, text: &str) -> Result<(), String> {
            self.log.push(format!("type:{text}"));
            Ok(())
        }
        fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
            self.log.push(format!("key:{combo:?}"));
            Ok(())
        }
        fn is_locked(&mut self) -> bool {
            self.locked
        }
        fn pace(&mut self) {
            self.log.push("pace".into());
        }
        fn open_app(&mut self, app: &str) -> Result<(), String> {
            self.log.push(format!("open:{app}"));
            Ok(())
        }
        fn focus_window(&mut self, title: &str) -> Result<String, String> {
            self.log.push(format!("focus:{title}"));
            Ok(format!("{title} — Editor"))
        }
        fn trash(&mut self, paths: &[std::path::PathBuf]) -> Result<(), String> {
            self.log.push(format!("trash:{}", paths.len()));
            Ok(())
        }
    }

    fn server() -> DesktopServer<Fake> {
        let fake = Fake {
            monitors: vec![
                mon("main", 0, 0, 200, 100, 1.25, true),
                mon("left", -1920, 0, 1920, 1080, 1.0, false),
            ],
            ..Fake::default()
        };
        DesktopServer::new("2.10.61", fake)
    }

    fn on() -> CallGate {
        CallGate {
            enabled: true,
            halted: false,
        }
    }

    fn reply_of(out: &RpcOutcome) -> Value {
        serde_json::from_str(out.reply.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn initialize_negotiates_and_names_the_server() {
        let mut s = server();
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
        let out = s.handle_line(line, on());
        let v = reply_of(&out);
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(v["result"]["serverInfo"]["name"], "grokhub-desktop");
        assert_eq!(v["result"]["serverInfo"]["version"], "2.10.61");
        assert_eq!(v["result"]["capabilities"]["tools"]["listChanged"], false);
        let old = s.handle_line(
            r#"{"jsonrpc":"2.0","id":"abc","method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
            on(),
        );
        let oldv = reply_of(&old);
        assert_eq!(oldv["id"], "abc");
        assert_eq!(oldv["result"]["protocolVersion"], "2024-11-05");
        let unknown = s.handle_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
            on(),
        );
        assert_eq!(reply_of(&unknown)["result"]["protocolVersion"], "2025-06-18");
        let bad = s.handle_line(
            r#"{"jsonrpc":"2.0","id":4,"method":"initialize","params":{}}"#,
            on(),
        );
        assert_eq!(reply_of(&bad)["error"]["code"], -32602);
        let noted = s.handle_line(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            on(),
        );
        assert!(noted.reply.is_none());
        assert!(!noted.exit);
        let ping = s.handle_line(r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#, on());
        assert!(reply_of(&ping)["result"].is_object());
    }

    #[test]
    fn tools_list_schemas_and_call_shapes() {
        let mut s = server();
        let listed = s.handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, on());
        let tools = reply_of(&listed)["result"]["tools"].as_array().unwrap().clone();
        let names = [
            "list_monitors",
            "screenshot",
            "click",
            "move",
            "drag",
            "scroll",
            "type",
            "key",
            "open_app",
            "focus_window",
            "delete_files",
        ];
        assert_eq!(tools.len(), names.len());
        for (tool, name) in tools.iter().zip(names) {
            assert_eq!(tool["name"], name);
            let desc = tool["description"].as_str().unwrap();
            assert!(!desc.is_empty());
            if matches!(name, "screenshot" | "click" | "move" | "drag" | "scroll") {
                assert!(
                    desc.contains("screenshot") || desc.contains("Coordinates are pixels"),
                    "{name}: {desc}"
                );
            }
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert!(tool["inputSchema"]["required"].is_array());
        }
        let shot = s.handle_line(
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"screenshot","arguments":{"monitor":"all"}}}"#,
            on(),
        );
        let body = reply_of(&shot);
        assert_eq!(body["result"]["isError"], false);
        let content = body["result"]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["mimeType"], "image/png");
        assert_eq!(content[0]["data"], "/w==");
        let geom: Value = serde_json::from_str(content[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(geom["id"], "all");
        assert!(geom.get("scale_factor").is_some());
        assert!(geom.get("physical_w").is_some());
        assert!(geom.get("image_w").is_some());
        assert!(geom.get("scale").is_some());
        assert_eq!(body["result"]["structuredContent"]["id"], "all");
        let oops = s.handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#,
            on(),
        );
        assert_eq!(reply_of(&oops)["error"]["code"], -32601);
        let bad = s.handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#,
            on(),
        );
        assert_eq!(reply_of(&bad)["error"]["code"], -32602);
        let parse = s.handle_line("{", on());
        assert_eq!(reply_of(&parse)["error"]["code"], -32700);
        assert!(reply_of(&parse)["id"].is_null());
        s.backend_mut().fail = Some("capture failed".into());
        let err = s.handle_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_monitors"}}"#,
            on(),
        );
        let ev = reply_of(&err);
        assert_eq!(ev["result"]["isError"], true);
        assert!(ev["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("capture failed"));
        assert!(ev.get("error").is_none());
    }

    #[test]
    fn click_drag_and_double_use_screenshot_space() {
        let mut s = server();
        let _ = s.handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"screenshot","arguments":{"monitor":"main"}}}"#,
            on(),
        );
        s.backend_mut().log.clear();
        let click = s.handle_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"click","arguments":{"x":0,"y":0,"double":true,"monitor":"main"}}}"#,
            on(),
        );
        assert_eq!(reply_of(&click)["result"]["isError"], false);
        let log = s.backend_mut().log.clone();
        assert!(log.iter().any(|e| e == "move:1,1"), "{log:?}");
        assert_eq!(log.iter().filter(|e| e.starts_with("btn:")).count(), 4);
        assert!(log.iter().any(|e| e == "pace"), "{log:?}");
        s.backend_mut().log.clear();
        let _ = s.handle_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"drag","arguments":{"from_x":0,"from_y":0,"to_x":10,"to_y":0,"monitor":"main"}}}"#,
            on(),
        );
        let log = &s.backend_mut().log;
        assert!(log.iter().filter(|e| e.starts_with("move:")).count() >= 8, "{log:?}");
        assert!(log.iter().any(|e| e.contains("btn:Left:true")));
        assert!(log.last().unwrap().contains("btn:Left:false"));
    }

    #[test]
    fn drag_lets_go_when_a_move_fails() {
        let mut s = server();
        let _ = s.handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"screenshot","arguments":{"monitor":"main"}}}"#,
            on(),
        );
        s.backend_mut().fail_moves_while_down = true;
        s.backend_mut().log.clear();
        let drag = s.handle_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"drag","arguments":{"from_x":0,"from_y":0,"to_x":10,"to_y":0,"monitor":"main"}}}"#,
            on(),
        );
        assert_eq!(reply_of(&drag)["result"]["isError"], true);
        let log = &s.backend_mut().log;
        assert!(log.last().unwrap().contains("btn:Left:false"), "{log:?}");
        assert!(!s.backend_mut().down);
    }

    #[test]
    fn gate_refuses_off_halted_and_locked() {
        let mut s = server();
        let off = s.handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"screenshot"}}"#,
            CallGate {
                enabled: false,
                halted: false,
            },
        );
        assert!(!off.exit);
        assert!(reply_of(&off)["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Desktop control is off"));
        assert!(s.backend_mut().log.is_empty());
        let halted = s.handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_monitors"}}"#,
            CallGate {
                enabled: true,
                halted: true,
            },
        );
        assert!(halted.exit);
        assert!(reply_of(&halted)["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("halted"));
        let still = s.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#, CallGate {
            enabled: true,
            halted: true,
        });
        assert!(!still.exit);
        assert!(reply_of(&still).get("result").is_some());
        s.backend_mut().locked = true;
        let shot = s.handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"screenshot"}}"#,
            on(),
        );
        assert_eq!(reply_of(&shot)["result"]["isError"], false);
        let click = s.handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"click","arguments":{"x":1,"y":1}}}"#,
            on(),
        );
        assert!(!click.exit);
        assert!(reply_of(&click)["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("lock screen"));
        assert!(!s.backend_mut().log.iter().any(|e| e.starts_with("move:")));
    }

    #[test]
    fn stamp_equal_is_not_halted() {
        assert!(!stamp_halts(10, 10));
        assert!(stamp_halts(11, 10));
        assert!(!stamp_halts(9, 10));
    }

    #[test]
    fn permission_args_cover_mode_attendance_and_the_switch() {
        let modes = [
            DesktopPermMode::Ask,
            DesktopPermMode::Auto,
            DesktopPermMode::Always,
        ];
        for mode in modes {
            for attended in [false, true] {
                for enabled in [false, true] {
                    let args = desktop_mcp_args(mode, attended, enabled);
                    let deny = args.windows(2).any(|w| w[0] == "--deny" && w[1] == DESKTOP_MCP_RULE);
                    let allow = args
                        .windows(2)
                        .any(|w| w[0] == "--allow" && w[1] == DESKTOP_MCP_RULE);
                    let want_deny = !enabled || (matches!(mode, DesktopPermMode::Ask) && !attended);
                    let want_allow = enabled
                        && matches!(mode, DesktopPermMode::Auto | DesktopPermMode::Always);
                    let want_none = enabled && matches!(mode, DesktopPermMode::Ask) && attended;
                    assert_eq!(deny, want_deny, "{mode:?} att={attended} en={enabled} {args:?}");
                    assert_eq!(allow, want_allow, "{mode:?} att={attended} en={enabled} {args:?}");
                    assert_eq!(args.is_empty(), want_none, "{mode:?} att={attended} en={enabled}");
                    assert!(!(deny && allow), "deny wins {args:?}");
                }
            }
        }
        let both = vec![
            "--allow".into(),
            DESKTOP_MCP_RULE.into(),
            "--deny".into(),
            DESKTOP_MCP_RULE.into(),
        ];
        let kept = apply_desktop_mcp_args(both, DesktopPermMode::Auto, false, true);
        assert!(kept.windows(2).any(|w| w[0] == "--deny" && w[1] == DESKTOP_MCP_RULE));
        assert!(!kept.windows(2).any(|w| w[0] == "--allow" && w[1] == DESKTOP_MCP_RULE));
        assert_eq!(
            kept.iter().filter(|a| a.as_str() == DESKTOP_MCP_RULE).count(),
            1
        );
        let already = apply_desktop_mcp_args(
            vec!["--deny".into(), DESKTOP_MCP_RULE.into()],
            DesktopPermMode::Ask,
            false,
            false,
        );
        assert_eq!(already.iter().filter(|a| *a == "--deny").count(), 1);
        let ask = vec!["--allow".into(), DESKTOP_MCP_RULE.into()];
        let left = apply_desktop_mcp_args(ask.clone(), DesktopPermMode::Ask, true, true);
        assert_eq!(left, ask);
    }

    fn eis(name: &str, x: i32, y: i32, w: u32, h: u32) -> EisRegion {
        EisRegion {
            name: name.into(),
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn shot_at(x: i32, y: i32, w: u32, h: u32, scale: f64) -> ShotGeom {
        ShotGeom {
            id: "m".into(),
            x,
            y,
            physical_w: w,
            physical_h: h,
            scale_factor: scale,
            image_w: w,
            image_h: h,
        }
    }

    fn near(got: f32, want: f32) {
        assert!((got - want).abs() < 0.02, "{got} vs {want}");
    }

    #[test]
    fn maps_image_pixels_to_eis_regions_at_common_scales() {
        let span = [
            eis("HDMI-A-2", 0, 0, 2560, 1440),
            eis("DP-1", 2560, 0, 2800, 1440),
            eis("DP-2", 5360, 0, 1920, 1440),
        ];
        let all = shot_at(0, 0, 7280, 1440, 1.0);
        let left = map_image_px_to_eis(&all, &span, 0.0, 0.0).unwrap();
        assert_eq!(left.region, 0);
        near(left.x, 0.0);
        near(left.y, 0.0);
        let edge = map_image_px_to_eis(&all, &span, 2560.0, 10.0).unwrap();
        assert_eq!(edge.region, 1);
        near(edge.x, 2560.0);
        near(edge.y, 10.0);
        let right = map_image_px_to_eis(&all, &span, 5360.0, 100.0).unwrap();
        assert_eq!(right.region, 2);
        near(right.x, 5360.0);
        let past = map_image_px_to_eis(&all, &span, 7285.0, 0.0).unwrap();
        assert_eq!(past.region, 2);
        near(past.x, 7279.0);

        let s2 = shot_at(0, 0, 200, 100, 2.0);
        let p = map_image_px_to_eis(&s2, &[eis("a", 0, 0, 100, 50)], 10.0, 8.0).unwrap();
        near(p.x, 5.0);
        near(p.y, 4.0);

        let s125 = shot_at(100, 40, 2500, 1250, 1.25);
        let p = map_image_px_to_eis(&s125, &[eis("a", 100, 40, 2000, 1000)], 125.0, 25.0).unwrap();
        near(p.x, 200.0);
        near(p.y, 60.0);

        let s15 = shot_at(0, 0, 300, 150, 1.5);
        let p = map_image_px_to_eis(&s15, &[eis("a", 0, 0, 200, 100)], 3.0, 6.0).unwrap();
        near(p.x, 2.0);
        near(p.y, 4.0);

        let neg = shot_at(-1920, -120, 1920, 1080, 1.0);
        let region = [eis("left", -1920, -120, 1920, 1080)];
        let origin = map_image_px_to_eis(&neg, &region, 0.0, 0.0).unwrap();
        near(origin.x, -1920.0);
        near(origin.y, -120.0);
        let inside = map_image_px_to_eis(&neg, &region, 10.0, 20.0).unwrap();
        near(inside.x, -1910.0);
        near(inside.y, -100.0);
        let before = map_image_px_to_eis(&neg, &region, -40.0, -80.0).unwrap();
        near(before.x, -1920.0);
        near(before.y, -120.0);
    }

    #[test]
    fn region_pick_at_output_edges() {
        let span = [
            eis("HDMI-A-2", 0, 0, 2560, 1440),
            eis("DP-1", 2560, 0, 2800, 1440),
            eis("DP-2", 5360, 0, 1920, 1440),
        ];
        let at = |x: f64, y: f64| pick_eis_region(&span, x, y);
        assert_eq!(at(2559.0, 0.0).region, 0);
        near(at(2559.0, 0.0).x, 2559.0);
        assert_eq!(at(2560.0, 0.0).region, 1);
        near(at(2560.0, 0.0).x, 2560.0);
        assert_eq!(at(5359.0, 1439.0).region, 1);
        assert_eq!(at(5360.0, 0.0).region, 2);
        near(at(5360.0, 0.0).x, 5360.0);
        assert_eq!(at(7279.0, 1439.0).region, 2);
        near(at(7279.0, 1439.0).x, 7279.0);
        near(at(7279.0, 1439.0).y, 1439.0);
        let far = at(7280.0, 1440.0);
        assert_eq!(far.region, 2);
        near(far.x, 7279.0);
        near(far.y, 1439.0);
        let beyond = at(7285.0, -4.0);
        assert_eq!(beyond.region, 2);
        near(beyond.x, 7279.0);
        near(beyond.y, 0.0);
        let neg = [eis("left", -1920, -100, 1920, 1080), eis("main", 0, 0, 2560, 1440)];
        assert_eq!(pick_eis_region(&neg, -1920.0, -100.0).region, 0);
        near(pick_eis_region(&neg, -1920.0, -100.0).x, -1920.0);
        assert_eq!(pick_eis_region(&neg, 0.0, 0.0).region, 1);
        near(pick_eis_region(&neg, -1921.0, -100.0).x, -1920.0);
        assert_eq!(pick_eis_region(&neg, -1921.0, -100.0).region, 0);
    }

    #[test]
    fn backend_selection_matrix() {
        let kde = select_desktop_stack(
            &DesktopSessionEnv {
                wayland_display: Some("wayland-0".into()),
                current_desktop: Some("KDE".into()),
                ..DesktopSessionEnv::default()
            },
            &DesktopProbes { kwin: false },
        );
        assert_eq!(kde.input, DesktopInputKind::Portal);
        assert_eq!(kde.capture, DesktopCaptureKind::ScreenShot2);
        assert_eq!(kde.monitors, DesktopMonitorKind::Kscreen);

        let plasma = select_desktop_stack(
            &DesktopSessionEnv {
                session_type: Some("wayland".into()),
                current_desktop: Some("KDE:plasma".into()),
                ..DesktopSessionEnv::default()
            },
            &DesktopProbes::default(),
        );
        assert_eq!(plasma.input, DesktopInputKind::Portal);

        let gnome_kwin = select_desktop_stack(
            &DesktopSessionEnv {
                wayland_display: Some("wayland-0".into()),
                current_desktop: Some("ubuntu:GNOME".into()),
                ..DesktopSessionEnv::default()
            },
            &DesktopProbes { kwin: true },
        );
        assert_eq!(gnome_kwin.input, DesktopInputKind::Portal);
        assert_eq!(gnome_kwin.capture, DesktopCaptureKind::ScreenShot2);

        let sway = select_desktop_stack(
            &DesktopSessionEnv {
                wayland_display: Some("wayland-1".into()),
                current_desktop: Some("sway".into()),
                ..DesktopSessionEnv::default()
            },
            &DesktopProbes { kwin: false },
        );
        assert_eq!(sway.input, DesktopInputKind::Ydotool);
        assert_eq!(sway.capture, DesktopCaptureKind::Grim);
        assert_eq!(sway.monitors, DesktopMonitorKind::Wlroots);

        let x11 = select_desktop_stack(
            &DesktopSessionEnv {
                display: Some(":0".into()),
                current_desktop: Some("KDE".into()),
                ..DesktopSessionEnv::default()
            },
            &DesktopProbes { kwin: true },
        );
        assert_eq!(x11.input, DesktopInputKind::X11rb);
        assert_eq!(x11.capture, DesktopCaptureKind::X11rb);
        assert_eq!(x11.monitors, DesktopMonitorKind::X11Randr);

        let none = select_desktop_stack(&DesktopSessionEnv::default(), &DesktopProbes::default());
        assert_eq!(none.input, DesktopInputKind::None);
        assert_eq!(none.capture, DesktopCaptureKind::None);
        assert_eq!(none.monitors, DesktopMonitorKind::None);
    }

    #[test]
    fn screenshot2_metadata_and_premultiplied_argb_to_rgba() {
        let meta = parse_screenshot2_metadata(&json!({
            "type": "raw",
            "format": "argb32",
            "width": 3,
            "height": 1,
            "stride": 16,
            "scale": 1.5
        }))
        .unwrap();
        assert_eq!(meta.width, 3);
        assert_eq!(meta.height, 1);
        assert_eq!(meta.stride, 16);
        assert_eq!(meta.scale, 1.5);
        assert!(parse_screenshot2_metadata(&json!({"width": 0, "height": 1})).is_err());

        let mut src = vec![0u8; 16];
        src[0..4].copy_from_slice(&[0, 0, 128, 128]);
        src[4..8].copy_from_slice(&[255, 0, 0, 255]);
        src[8..12].copy_from_slice(&[9, 8, 7, 0]);
        let rgba = argb32_premul_to_rgba(&src, 3, 1, 16).unwrap();
        assert_eq!(&rgba[0..4], &[255, 0, 0, 128]);
        assert_eq!(&rgba[4..8], &[0, 0, 255, 255]);
        assert_eq!(&rgba[8..12], &[0, 0, 0, 0]);
    }

    #[test]
    fn parses_kscreen_doctor_outputs() {
        let text = r#"{
            "outputs": [
                {
                    "id": 42,
                    "name": "HDMI-A-2",
                    "pos": {"x": 0, "y": 0},
                    "size": {"width": 2560, "height": 1440},
                    "scale": 1.0,
                    "rotation": 1,
                    "connected": true,
                    "enabled": true,
                    "priority": 1
                },
                {
                    "id": 7,
                    "name": "DP-1",
                    "pos": {"x": 2560, "y": 0},
                    "size": {"width": 2800, "height": 1440},
                    "scale": 1.25,
                    "rotation": 1,
                    "connected": true,
                    "enabled": true,
                    "priority": 2
                },
                {
                    "id": 8,
                    "name": "DP-2",
                    "pos": {"x": -1920, "y": -100},
                    "size": {"width": 1440, "height": 2560},
                    "scale": 2,
                    "rotation": 2,
                    "connected": true,
                    "enabled": false,
                    "priority": 3
                },
                {
                    "id": 9,
                    "name": "eDP-1",
                    "connected": false,
                    "enabled": true,
                    "pos": {"x": 0, "y": 0},
                    "size": {"width": 1920, "height": 1080},
                    "scale": 1
                },
                {
                    "name": "VGA-1",
                    "connected": true,
                    "enabled": true,
                    "pos": {"x": 0, "y": 0},
                    "size": {"width": 0, "height": 0},
                    "scale": 1
                }
            ]
        }"#;
        let mons = parse_kscreen_doctor(text).unwrap();
        assert_eq!(mons.len(), 2);
        assert_eq!(mons[0].name, "HDMI-A-2");
        assert_eq!(mons[0].id, "42");
        assert!(mons[0].primary);
        assert_eq!((mons[0].width, mons[0].height), (2560, 1440));
        assert_eq!(mons[1].name, "DP-1");
        assert_eq!(mons[1].x, 2560);
        assert!((mons[1].scale_factor - 1.25).abs() < f64::EPSILON);
        assert!(!mons[1].primary);
        let joined = join_kscreen_with_eis(
            &mons,
            &[eis("DP-1", 2000, 10, 2240, 1152), eis("HDMI-A-2", 0, 0, 2560, 1440)],
        );
        assert_eq!(joined[0].name, "DP-1");
        assert_eq!(joined[0].x, 2000);
        assert_eq!(joined[0].width, 2800);
        assert!((joined[0].scale_factor - 1.25).abs() < f64::EPSILON);
        assert_eq!(joined[1].name, "HDMI-A-2");
        assert!(joined[1].primary);
        assert_eq!(
            plan_eis_text(true, "é", |_| None).unwrap(),
            EisTextPlan::Text
        );
        assert_eq!(
            plan_eis_text(false, "ab", |ch| match ch {
                'a' => Some(30),
                'b' => Some(48),
                _ => None,
            })
            .unwrap(),
            EisTextPlan::Keycodes(vec![30, 48])
        );
        assert!(plan_eis_text(false, "é", |_| None).unwrap_err().contains("missing"));
    }

    #[test]
    fn spectacle_argv() {
        assert_eq!(
            super::spectacle_argv("/tmp/grokhub-shot.png", None),
            vec![
                "spectacle".to_string(),
                "-b".into(),
                "-n".into(),
                "-f".into(),
                "-o".into(),
                "/tmp/grokhub-shot.png".into(),
            ]
        );
        assert_eq!(
            super::spectacle_argv("/tmp/out.png", Some(2)),
            vec![
                "spectacle", "-b", "-n", "-f", "-o", "/tmp/out.png", "-s", "2"
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
        assert!(spectacle_help_supports_screen(
            "Usage: spectacle -b -n -f -o file -s, --screen <index>"
        ));
        assert!(!spectacle_help_supports_screen("Usage: spectacle -b -n -f -o file"));
        assert_eq!(
            monitor_index(
                &[mon("HDMI-A-2", 0, 0, 100, 100, 1.0, true), mon("DP-1", 100, 0, 100, 100, 1.0, false)],
                "DP-1"
            ),
            Some(1)
        );
        assert_eq!(monitor_index(&[mon("a", 0, 0, 1, 1, 1.0, true)], "all"), None);
    }

    #[test]
    fn uinput_abs_device_spans_the_output_union() {
        let mons = [
            mon("HDMI-A-2", 0, 0, 2560, 1440, 1.0, true),
            mon("DP-1", 2560, 0, 2800, 1440, 1.0, false),
            mon("DP-2", 5360, 0, 1920, 1440, 1.0, false),
        ];
        let desc = uinput_abs_descriptor(&mons).unwrap();
        assert_eq!(desc.name, "GrokHub absolute pointer");
        assert_eq!(desc.width, 7280);
        assert_eq!(desc.height, 1440);
        assert_eq!(desc.origin_x, 0);
        assert_eq!(desc.origin_y, 0);
        assert_eq!(desc.abs_x.minimum, 0);
        assert_eq!(desc.abs_x.maximum, 7279);
        assert_eq!(desc.abs_y.maximum, 1439);
        assert_eq!(
            (desc.abs_x.maximum - desc.abs_x.minimum + 1) as u32,
            desc.width
        );
        assert_eq!(map_point_to_uinput(&desc, 10, 20), (10, 20));
        assert_eq!(map_point_to_uinput(&desc, 2560, 0), (2560, 0));
        assert_eq!(map_point_to_uinput(&desc, 5360 + 100, 10), (5460, 10));
        assert_eq!(map_point_to_uinput(&desc, 8000, 2000), (7279, 1439));

        let shifted = [
            mon("left", -1920, -100, 1920, 1080, 1.0, false),
            mon("main", 0, 0, 2560, 1440, 1.0, true),
        ];
        let desc = uinput_abs_descriptor(&shifted).unwrap();
        assert_eq!(desc.origin_x, -1920);
        assert_eq!(desc.origin_y, -100);
        assert_eq!(map_point_to_uinput(&desc, -1920, -100), (0, 0));
        assert_eq!(map_point_to_uinput(&desc, 0, 0), (1920, 100));
        assert!(uinput_access_denied_message().contains("TAG+=\"uaccess\""));
        assert!(uinput_access_denied_message().contains("/dev/uinput"));
    }

    #[test]
    fn broker_protocol_round_trip() {
        let geom = ShotGeom {
            id: "m".into(),
            x: -1920,
            y: 0,
            physical_w: 1920,
            physical_h: 1080,
            scale_factor: 1.0,
            image_w: 1920,
            image_h: 1080,
        };
        let requests = vec![
            DeskRequest::ListMonitors { id: 1 },
            DeskRequest::Screenshot {
                id: 2,
                monitor: "HDMI-A-2".into(),
            },
            DeskRequest::MoveAbs { id: 3, x: 4, y: 5 },
            DeskRequest::MoveOn {
                id: 4,
                geom: geom.clone(),
                x: 8,
                y: 9,
            },
            DeskRequest::Button {
                id: 5,
                button: MouseButton::Right,
                down: true,
            },
            DeskRequest::Scroll { id: 6, dx: -1, dy: 2 },
            DeskRequest::TypeText {
                id: 7,
                text: "hi".into(),
            },
            DeskRequest::Key {
                id: 8,
                keys: "ctrl+a".into(),
            },
            DeskRequest::Release { id: 9 },
            DeskRequest::Locked { id: 10 },
            DeskRequest::Status { id: 11 },
            DeskRequest::Probe { id: 12 },
        ];
        for req in &requests {
            let line = encode_desk_request(req).unwrap();
            assert!(!line.contains("enabled"), "{line}");
            assert!(!line.contains('\n'));
            assert_eq!(&decode_desk_request(&line).unwrap(), req);
        }
        let full = DeskResponse {
            id: 3,
            ok: true,
            error: Some("nope".into()),
            exit: true,
            monitors: Some(vec![mon("HDMI-A-2", 0, 0, 10, 10, 1.0, true)]),
            image_b64: Some(b64(&[1, 2, 3])),
            mime: Some("image/png".into()),
            geom: Some(geom),
            locked: Some(false),
            input: Some(input_route_label(InputRouteId::Libei).into()),
            capture: Some(capture_route_label(CaptureRouteId::ScreenShot2).into()),
            note: Some("imprecise".into()),
            report: Some("offset not measured".into()),
        };
        let encoded = encode_desk_response(&full).unwrap();
        assert_eq!(decode_desk_response(&encoded).unwrap(), full);
        let bare = DeskResponse::bare(9);
        let bare_line = encode_desk_response(&bare).unwrap();
        assert!(!bare_line.contains("image_b64"));
        assert_eq!(decode_desk_response(&bare_line).unwrap(), bare);
        assert_eq!(desk_b64_decode(&b64(b"Man")).unwrap(), b"Man");
        assert!(desk_b64_decode("****").is_err());
        assert!(peer_uid_allowed(1000, 1000));
        assert!(!peer_uid_allowed(1000, 0));
        assert_eq!(
            input_route_order(),
            &[
                InputRouteId::Libei,
                InputRouteId::Notify,
                InputRouteId::Uinput,
                InputRouteId::Ydotool
            ]
        );
        assert_eq!(
            capture_route_order(),
            &[
                CaptureRouteId::ScreenShot2,
                CaptureRouteId::Spectacle,
                CaptureRouteId::PortalScreenshot
            ]
        );
        assert_eq!(
            desktop_control_status(
                input_route_label(InputRouteId::Libei),
                capture_route_label(CaptureRouteId::ScreenShot2)
            ),
            "Input: RemoteDesktop portal (libei)\nCapture: KWin ScreenShot2"
        );
    }

    struct GateFake {
        locked: bool,
        released: u32,
        suspended: u32,
        moves: u32,
        shots: u32,
        lists: u32,
    }

    impl DesktopBackend for GateFake {
        fn list_monitors(&mut self) -> Result<Vec<MonitorGeom>, String> {
            self.lists += 1;
            Ok(vec![mon("main", 0, 0, 200, 100, 1.0, true)])
        }
        fn screenshot(&mut self, _monitor: &str) -> Result<CapturedShot, String> {
            self.shots += 1;
            Ok(CapturedShot {
                bytes: vec![9],
                mime: "image/png".into(),
                geom: shot(0, 0, 200, 100, 200, 100, 1.0),
            })
        }
        fn move_abs(&mut self, _x: i32, _y: i32) -> Result<(), String> {
            self.moves += 1;
            Ok(())
        }
        fn button(&mut self, _button: MouseButton, _down: bool) -> Result<(), String> {
            Ok(())
        }
        fn scroll(&mut self, _dx: i32, _dy: i32) -> Result<(), String> {
            Ok(())
        }
        fn type_text(&mut self, _text: &str) -> Result<(), String> {
            Ok(())
        }
        fn key_combo(&mut self, _combo: &KeyCombo) -> Result<(), String> {
            Ok(())
        }
        fn is_locked(&mut self) -> bool {
            self.locked
        }
        fn release_input(&mut self) {
            self.released += 1;
        }
        fn suspend_input(&mut self) {
            self.suspended += 1;
        }
    }

    #[test]
    fn broker_refuses_when_halted_or_switch_off() {
        let mut fake = GateFake {
            locked: false,
            released: 0,
            suspended: 0,
            moves: 0,
            shots: 0,
            lists: 0,
        };
        let off = apply_desk_request(
            &mut fake,
            CallGate {
                enabled: false,
                halted: false,
            },
            &DeskRequest::Screenshot {
                id: 1,
                monitor: "all".into(),
            },
        );
        assert!(!off.ok);
        assert!(!off.exit);
        assert_eq!(off.error.as_deref(), Some(OFF_MSG));
        assert_eq!(fake.suspended, 1);
        assert_eq!(fake.shots, 0);

        let halted = apply_desk_request(
            &mut fake,
            CallGate {
                enabled: true,
                halted: true,
            },
            &DeskRequest::MoveAbs { id: 2, x: 1, y: 1 },
        );
        assert!(!halted.ok);
        assert!(halted.exit);
        assert_eq!(halted.error.as_deref(), Some(HALT_MSG));
        assert_eq!(fake.released, 1);
        assert_eq!(fake.moves, 0);

        fake.locked = true;
        let locked = apply_desk_request(
            &mut fake,
            CallGate {
                enabled: true,
                halted: false,
            },
            &DeskRequest::Probe { id: 3 },
        );
        assert_eq!(locked.error.as_deref(), Some(LOCK_MSG));
        assert_eq!(fake.released, 2);
        assert_eq!(fake.moves, 0);

        let shot = apply_desk_request(
            &mut fake,
            CallGate {
                enabled: true,
                halted: false,
            },
            &DeskRequest::ListMonitors { id: 4 },
        );
        assert!(shot.ok);
        assert_eq!(fake.lists, 1);
        assert_eq!(fake.released, 2);
    }

    fn call(s: &mut DesktopServer<Fake>, gate: CallGate, tool: &str, args: Value) -> Value {
        let line = json!({
            "jsonrpc": "2.0", "id": 21, "method": "tools/call",
            "params": { "name": tool, "arguments": args },
        })
        .to_string();
        reply_of(&s.handle_line(&line, gate))["result"].clone()
    }

    #[test]
    fn open_and_focus_run_only_with_desktop_control_on() {
        let mut s = server();
        let opened = call(&mut s, on(), "open_app", json!({ "app": "org.kde.dolphin.desktop" }));
        assert_eq!(opened["isError"], false);
        assert_eq!(opened["content"][0]["text"], "opened org.kde.dolphin");
        let focused = call(&mut s, on(), "focus_window", json!({ "title": "notes" }));
        assert_eq!(focused["content"][0]["text"], "focused notes — Editor");
        assert_eq!(s.backend_mut().log, vec!["open:org.kde.dolphin", "focus:notes"]);
        let off = CallGate { enabled: false, halted: false };
        let refused = call(&mut s, off, "open_app", json!({ "app": "firefox" }));
        assert_eq!(refused["isError"], true);
        assert_eq!(refused["content"][0]["text"], OFF_MSG);
        assert_eq!(s.backend_mut().log.len(), 2, "nothing ran with the switch off");
        s.backend_mut().locked = true;
        let locked = call(&mut s, on(), "focus_window", json!({ "title": "notes" }));
        assert_eq!(locked["content"][0]["text"], LOCK_MSG);
    }

    #[test]
    fn app_ids_are_ids_not_commands() {
        assert_eq!(app_id("firefox"), Ok("firefox".into()));
        assert_eq!(app_id(" org.kde.dolphin.desktop "), Ok("org.kde.dolphin".into()));
        assert_eq!(app_id("notepad"), Ok("notepad".into()));
        for bad in ["", "-x", ".hidden", "rm -rf ~", "/usr/bin/xterm", "C:\\x.exe", "a;b", "$(id)", "calc&"] {
            assert!(app_id(bad).is_err(), "{bad}");
        }
        assert_eq!(
            app_id("a;b"),
            Err("\"a;b\" is not an app id. Use a desktop id like org.kde.dolphin or an app name like notepad.".into())
        );
    }

    fn scratch(label: &str) -> std::path::PathBuf {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("desktop-mcp-tests")
            .join(format!("{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn delete_files_deletes_exactly_the_listed_paths_or_nothing() {
        let dir = scratch("delete");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        let keep = dir.join("keep.txt");
        let empty = dir.join("empty");
        let full = dir.join("full");
        for f in [&a, &b, &keep] {
            std::fs::write(f, b"fixture bytes").unwrap();
        }
        std::fs::create_dir(&empty).unwrap();
        std::fs::create_dir(&full).unwrap();
        std::fs::write(full.join("inner.txt"), b"x").unwrap();
        let s_path = |p: &std::path::Path| p.to_string_lossy().into_owned();
        let mut s = server();
        // One bad path: nothing is touched.
        let bad = call(&mut s, on(), "delete_files", json!({ "paths": [s_path(&a), s_path(&full)] }));
        assert_eq!(bad["isError"], true);
        assert!(a.exists() && full.join("inner.txt").exists());
        let rel = call(&mut s, on(), "delete_files", json!({ "paths": ["a.txt"] }));
        assert_eq!(rel["content"][0]["text"], "a.txt: use the full path.");
        let twice = call(&mut s, on(), "delete_files", json!({ "paths": [s_path(&a), s_path(&a)] }));
        assert_eq!(twice["isError"], true);
        assert!(a.exists());
        let ok = call(&mut s, on(), "delete_files", json!({ "paths": [s_path(&a), s_path(&b), s_path(&empty)] }));
        assert_eq!(ok["content"][0]["text"], "deleted 3 paths");
        assert!(!a.exists() && !b.exists() && !empty.exists());
        assert_eq!(std::fs::read(&keep).unwrap(), b"fixture bytes", "an unlisted file is untouched");
        let trash = call(&mut s, on(), "delete_files", json!({ "paths": [s_path(&keep)], "to_trash": true }));
        assert_eq!(trash["content"][0]["text"], "moved 1 path to the trash");
        assert_eq!(s.backend_mut().log.last().map(String::as_str), Some("trash:1"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
