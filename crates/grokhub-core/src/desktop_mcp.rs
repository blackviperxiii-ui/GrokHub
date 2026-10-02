//! Pure desktop MCP: geometry, key combos, JSON-RPC, and the permission argv.
//!
//! The cabin process owns the screen. This module never opens a display.

use std::collections::HashMap;

use serde_json::{json, Value};

/// MCP server name. grok exposes tools as `grokhub-desktop__<tool>`.
pub const DESKTOP_MCP_SERVER: &str = "grokhub-desktop";

/// grok permission rule for every desktop tool.
pub const DESKTOP_MCP_RULE: &str = "MCPTool(grokhub-desktop__*)";

const PREFERRED_PROTOCOL: &str = "2025-06-18";
const PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const OFF_MSG: &str = "Desktop control is off. Turn on Settings → Let Grok control the desktop.";
const HALT_MSG: &str =
    "Desktop control was halted (Ctrl+Alt+H). Open GrokHub again to use the screen.";
const LOCK_MSG: &str = "The lock screen is up. Unlock this computer, then try again.";

const DRAG_STEPS: i32 = 8;

const COORD_NOTE: &str = "Coordinates are pixels in the last screenshot of that monitor (or \"all\"), not physical screen pixels. With no screenshot yet, they are the monitor's native pixels.";

/// One monitor in physical pixels. Origins may be negative on a virtual desktop.
#[derive(Clone, Debug, PartialEq)]
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
#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
            return tool_fail(id, OFF_MSG, false);
        }
        if gate.halted {
            return tool_fail(id, HALT_MSG, true);
        }
        if is_input_tool(name) && self.backend.is_locked() {
            return tool_fail(id, LOCK_MSG, false);
        }
        let result = match name {
            "list_monitors" => self.tool_list_monitors(),
            "screenshot" => self.tool_screenshot(&args),
            "click" => self.tool_click(&args),
            "move" => self.tool_move(&args),
            "drag" => self.tool_drag(&args),
            "scroll" => self.tool_scroll(&args),
            "type" => self.tool_type(&args),
            "key" => self.tool_key(&args),
            other => Err(format!("Unknown tool \"{other}\".")),
        };
        match result {
            Ok(body) => RpcOutcome {
                reply: Some(rpc_result(id, body)),
                exit: false,
            },
            Err(msg) => tool_fail(id, &msg, false),
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

    fn tool_click(&mut self, args: &Value) -> Result<Value, String> {
        let (x, y) = require_xy(args, "x", "y")?;
        let button = button_arg(args)?;
        let double = args.get("double").and_then(|v| v.as_bool()).unwrap_or(false);
        let (sx, sy) = self.map_xy(&monitor_arg(args), x, y)?;
        self.backend.move_abs(sx, sy)?;
        self.backend.button(button, true)?;
        self.backend.button(button, false)?;
        if double {
            self.backend.pace();
            self.backend.button(button, true)?;
            self.backend.button(button, false)?;
        }
        Ok(text_ok("clicked"))
    }

    fn tool_move(&mut self, args: &Value) -> Result<Value, String> {
        let (x, y) = require_xy(args, "x", "y")?;
        let (sx, sy) = self.map_xy(&monitor_arg(args), x, y)?;
        self.backend.move_abs(sx, sy)?;
        Ok(text_ok("moved"))
    }

    fn tool_drag(&mut self, args: &Value) -> Result<Value, String> {
        let (x0, y0) = require_xy(args, "from_x", "from_y")?;
        let (x1, y1) = require_xy(args, "to_x", "to_y")?;
        let button = button_arg(args)?;
        let which = monitor_arg(args);
        let (sx0, sy0) = self.map_xy(&which, x0, y0)?;
        let (sx1, sy1) = self.map_xy(&which, x1, y1)?;
        self.backend.move_abs(sx0, sy0)?;
        self.backend.button(button, true)?;
        let mut moved = Ok(());
        for step in 1..=DRAG_STEPS {
            let t = step as f64 / DRAG_STEPS as f64;
            let x = round_i32(sx0 as f64 + (sx1 - sx0) as f64 * t);
            let y = round_i32(sy0 as f64 + (sy1 - sy0) as f64 * t);
            moved = self.backend.move_abs(x, y);
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
        Ok(text_ok("dragged"))
    }

    fn tool_scroll(&mut self, args: &Value) -> Result<Value, String> {
        let (x, y) = require_xy(args, "x", "y")?;
        let dx = args.get("dx").and_then(Value::as_f64).unwrap_or(0.0);
        let dy = args.get("dy").and_then(Value::as_f64).unwrap_or(0.0);
        let (sx, sy) = self.map_xy(&monitor_arg(args), x, y)?;
        self.backend.move_abs(sx, sy)?;
        self.backend.scroll(round_i32(dx), round_i32(dy))?;
        Ok(text_ok("scrolled"))
    }

    fn tool_type(&mut self, args: &Value) -> Result<Value, String> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "type needs text.".to_string())?;
        self.backend.type_text(text)?;
        Ok(text_ok("typed"))
    }

    fn tool_key(&mut self, args: &Value) -> Result<Value, String> {
        let keys = args
            .get("keys")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "key needs keys.".to_string())?;
        let combo = parse_key_combo(keys)?;
        self.backend.key_combo(&combo)?;
        Ok(text_ok("keyed"))
    }

    fn map_xy(&mut self, monitor: &str, x: f64, y: f64) -> Result<(i32, i32), String> {
        let geom = self.shot_for(monitor)?;
        Ok(map_screenshot_point(&geom, x, y))
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
    matches!(name, "click" | "move" | "drag" | "scroll" | "type" | "key")
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
}
