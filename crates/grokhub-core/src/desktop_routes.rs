// Desktop route helpers: gates, fallback order, uinput mapping, capture crops,
// and the desk broker codec. Included into `desktop_mcp`; free of D-Bus, evdev, and sockets.

use std::path::{Path, PathBuf};

/// Wayland input after the compositor session is up. Order is the fallback order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputRouteId {
    Libei,
    Notify,
    Uinput,
    Ydotool,
}

/// Capture after a KDE session. Grim stays the wlroots path and is not in this list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureRouteId {
    ScreenShot2,
    Spectacle,
    PortalScreenshot,
}

pub fn input_route_order() -> &'static [InputRouteId] {
    &[
        InputRouteId::Libei,
        InputRouteId::Notify,
        InputRouteId::Uinput,
        InputRouteId::Ydotool,
    ]
}

pub fn capture_route_order() -> &'static [CaptureRouteId] {
    &[
        CaptureRouteId::ScreenShot2,
        CaptureRouteId::Spectacle,
        CaptureRouteId::PortalScreenshot,
    ]
}

pub fn input_route_label(route: InputRouteId) -> &'static str {
    match route {
        InputRouteId::Libei => "RemoteDesktop portal (libei)",
        InputRouteId::Notify => "RemoteDesktop portal (Notify)",
        InputRouteId::Uinput => "absolute uinput",
        InputRouteId::Ydotool => "ydotool (imprecise)",
    }
}

pub fn capture_route_label(route: CaptureRouteId) -> &'static str {
    match route {
        CaptureRouteId::ScreenShot2 => "KWin ScreenShot2",
        CaptureRouteId::Spectacle => "spectacle",
        CaptureRouteId::PortalScreenshot => "portal Screenshot",
    }
}

pub fn input_route_is_imprecise(route: InputRouteId) -> bool {
    matches!(route, InputRouteId::Ydotool)
}

/// Settings → Desktop control. Two lines, one backend each.
pub fn desktop_control_status(input: &str, capture: &str) -> String {
    format!("Input: {input}\nCapture: {capture}")
}

pub fn imprecise_fallback_note(failures: &[String]) -> String {
    let why = if failures.is_empty() {
        "Wayland input could not use the portal.".to_string()
    } else {
        failures.join(" ")
    };
    format!("{why} Pointer and keyboard input fell back to ydotool and is imprecise.")
}

pub fn uinput_access_denied_message() -> &'static str {
    "Absolute uinput needs write access to /dev/uinput. Install packaging/udev/60-grokhub-uinput.rules (TAG+=\"uaccess\") (mode 0660, seat user only), then re-login. Do not add this user to the input group: that also exposes every keyboard."
}

/// Inclusive absolute axis. `maximum - minimum + 1` is the pixel span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbsAxisRange {
    pub minimum: i32,
    pub maximum: i32,
}

/// Tablet-style absolute pointer sized to the union of outputs.
/// Axis 0 is the top-left of that union, so a negative monitor origin maps
/// into a non-negative axis value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UinputAbsDescriptor {
    pub name: &'static str,
    pub abs_x: AbsAxisRange,
    pub abs_y: AbsAxisRange,
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn uinput_abs_descriptor(monitors: &[MonitorGeom]) -> Result<UinputAbsDescriptor, String> {
    let union = union_monitor(monitors).ok_or_else(|| "No monitors for the uinput device.".to_string())?;
    if union.physical_w == 0 || union.physical_h == 0 {
        return Err("The output union is empty.".into());
    }
    let max_x = i32::try_from(union.physical_w - 1).unwrap_or(i32::MAX);
    let max_y = i32::try_from(union.physical_h - 1).unwrap_or(i32::MAX);
    Ok(UinputAbsDescriptor {
        name: "GrokHub absolute pointer",
        abs_x: AbsAxisRange {
            minimum: 0,
            maximum: max_x,
        },
        abs_y: AbsAxisRange {
            minimum: 0,
            maximum: max_y,
        },
        origin_x: union.x,
        origin_y: union.y,
        width: union.physical_w,
        height: union.physical_h,
    })
}

/// Global physical pixel → absolute axis value. Negative origins shift into range.
pub fn map_point_to_uinput(desc: &UinputAbsDescriptor, x: i32, y: i32) -> (i32, i32) {
    let ax = x.saturating_sub(desc.origin_x);
    let ay = y.saturating_sub(desc.origin_y);
    (
        ax.clamp(desc.abs_x.minimum, desc.abs_x.maximum),
        ay.clamp(desc.abs_y.minimum, desc.abs_y.maximum),
    )
}

/// `spectacle -b -n -f -o <path>`. `screen` adds `-s <index>` when the installed
/// spectacle accepts it; otherwise the caller captures the full desktop and crops.
pub fn spectacle_argv(output: &str, screen: Option<u32>) -> Vec<String> {
    let mut args = vec![
        "spectacle".to_string(),
        "-b".into(),
        "-n".into(),
        "-f".into(),
        "-o".into(),
        output.to_string(),
    ];
    if let Some(index) = screen {
        args.push("-s".into());
        args.push(index.to_string());
    }
    args
}

pub fn spectacle_help_supports_screen(help: &str) -> bool {
    help.split_whitespace().any(|token| {
        let token = token.trim_matches(|c: char| matches!(c, ',' | '[' | ']' | '(' | ')'));
        token == "-s" || token == "--screen"
    })
}

pub fn monitor_index(monitors: &[MonitorGeom], monitor: &str) -> Option<u32> {
    if monitor.is_empty() || monitor == "all" {
        return None;
    }
    monitors
        .iter()
        .position(|mon| mon.id == monitor || mon.name == monitor)
        .and_then(|index| u32::try_from(index).ok())
}

pub fn monitor_center(mon: &MonitorGeom) -> (i32, i32) {
    (
        mon.x.saturating_add((mon.width / 2) as i32),
        mon.y.saturating_add((mon.height / 2) as i32),
    )
}

/// Crop rectangle of one monitor inside a full-desktop image.
/// The desktop geom is the output union in physical pixels. The image may be
/// a different size; the rect is scaled by image/physical and clamped.
pub fn monitor_crop_rect(
    desktop: &ShotGeom,
    image_w: u32,
    image_h: u32,
    mon: &MonitorGeom,
) -> Result<(u32, u32, u32, u32), String> {
    if image_w == 0 || image_h == 0 || desktop.physical_w == 0 || desktop.physical_h == 0 {
        return Err("Cannot crop an empty desktop image.".into());
    }
    let scale_x = image_w as f64 / desktop.physical_w as f64;
    let scale_y = image_h as f64 / desktop.physical_h as f64;
    let x = ((mon.x.saturating_sub(desktop.x) as f64) * scale_x).round();
    let y = ((mon.y.saturating_sub(desktop.y) as f64) * scale_y).round();
    let w = (mon.width as f64 * scale_x).round();
    let h = (mon.height as f64 * scale_y).round();
    let x = x.clamp(0.0, image_w as f64) as u32;
    let y = y.clamp(0.0, image_h as f64) as u32;
    let w = (w as u32).min(image_w.saturating_sub(x)).max(1);
    let h = (h as u32).min(image_h.saturating_sub(y)).max(1);
    if x >= image_w || y >= image_h {
        return Err(format!("Monitor {} is outside the desktop image.", mon.name));
    }
    Ok((x, y, w, h))
}

pub fn crop_rgba(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> Result<Vec<u8>, String> {
    let need = (src_w as usize).saturating_mul(src_h as usize).saturating_mul(4);
    if src.len() < need || src_w == 0 || src_h == 0 {
        return Err("Screenshot buffer does not match its size.".into());
    }
    if w == 0 || h == 0 || x.saturating_add(w) > src_w || y.saturating_add(h) > src_h {
        return Err("Crop rectangle is outside the screenshot.".into());
    }
    let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for row in y..y + h {
        let start = ((row as usize) * (src_w as usize) + (x as usize)) * 4;
        let end = start + (w as usize) * 4;
        out.extend_from_slice(&src[start..end]);
    }
    Ok(out)
}

/// One ScreenCast stream in compositor coordinates. `node` is the PipeWire id
/// `NotifyPointerMotionAbsolute` expects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CastStream {
    pub node: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Point in the same space as the stream origin → `(node, local_x, local_y)`.
/// A point in a gap uses the nearest stream and clamps to its far edge.
pub fn cast_stream_for_point(streams: &[CastStream], x: i32, y: i32) -> Option<(u32, f64, f64)> {
    let stream = streams
        .iter()
        .find(|stream| stream_contains(stream, x, y))
        .or_else(|| nearest_stream(streams, x, y))?;
    let local_x = x.saturating_sub(stream.x) as f64;
    let local_y = y.saturating_sub(stream.y) as f64;
    let max_x = stream.width.saturating_sub(1) as f64;
    let max_y = stream.height.saturating_sub(1) as f64;
    Some((stream.node, local_x.clamp(0.0, max_x), local_y.clamp(0.0, max_y)))
}

fn stream_contains(stream: &CastStream, x: i32, y: i32) -> bool {
    let x1 = stream.x.saturating_add(stream.width as i32);
    let y1 = stream.y.saturating_add(stream.height as i32);
    x >= stream.x && y >= stream.y && x < x1 && y < y1
}

fn nearest_stream(streams: &[CastStream], x: i32, y: i32) -> Option<&CastStream> {
    streams.iter().min_by(|a, b| {
        stream_dist2(a, x, y)
            .partial_cmp(&stream_dist2(b, x, y))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn stream_dist2(stream: &CastStream, x: i32, y: i32) -> f64 {
    let x0 = stream.x as f64;
    let y0 = stream.y as f64;
    let x1 = x0 + stream.width as f64;
    let y1 = y0 + stream.height as f64;
    let px = x as f64;
    let py = y as f64;
    let dx = px - px.clamp(x0, x1);
    let dy = py - py.clamp(y0, y1);
    dx * dx + dy * dy
}

pub fn desk_socket_path(runtime_dir: &str) -> Result<PathBuf, String> {
    desk_runtime_file(runtime_dir, "desk.sock")
}

pub fn desk_lock_path(runtime_dir: &str) -> Result<PathBuf, String> {
    desk_runtime_file(runtime_dir, "desk.lock")
}

fn desk_runtime_file(runtime_dir: &str, name: &str) -> Result<PathBuf, String> {
    let runtime_dir = runtime_dir.trim();
    if runtime_dir.is_empty() {
        return Err("XDG_RUNTIME_DIR is unset, so the desktop broker has no socket directory.".into());
    }
    Ok(Path::new(runtime_dir).join("grokhub").join(name))
}

/// Same-uid peer credentials. A mismatched uid is refused before the request runs.
pub fn peer_uid_allowed(peer_uid: u32, self_uid: u32) -> bool {
    peer_uid == self_uid
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeskRequest {
    ListMonitors {
        id: u64,
    },
    Screenshot {
        id: u64,
        monitor: String,
    },
    MoveAbs {
        id: u64,
        x: i32,
        y: i32,
    },
    MoveOn {
        id: u64,
        geom: ShotGeom,
        x: i32,
        y: i32,
    },
    Button {
        id: u64,
        button: MouseButton,
        down: bool,
    },
    Scroll {
        id: u64,
        dx: i32,
        dy: i32,
    },
    TypeText {
        id: u64,
        text: String,
    },
    Key {
        id: u64,
        keys: String,
    },
    Release {
        id: u64,
    },
    Locked {
        id: u64,
    },
    Status {
        id: u64,
    },
    Probe {
        id: u64,
    },
}

impl DeskRequest {
    pub fn id(&self) -> u64 {
        match self {
            Self::ListMonitors { id }
            | Self::Screenshot { id, .. }
            | Self::MoveAbs { id, .. }
            | Self::MoveOn { id, .. }
            | Self::Button { id, .. }
            | Self::Scroll { id, .. }
            | Self::TypeText { id, .. }
            | Self::Key { id, .. }
            | Self::Release { id }
            | Self::Locked { id }
            | Self::Status { id }
            | Self::Probe { id } => *id,
        }
    }

    pub fn is_input(&self) -> bool {
        matches!(
            self,
            Self::MoveAbs { .. }
                | Self::MoveOn { .. }
                | Self::Button { .. }
                | Self::Scroll { .. }
                | Self::TypeText { .. }
                | Self::Key { .. }
                | Self::Probe { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeskResponse {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exit: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitors: Option<Vec<MonitorGeom>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geom: Option<ShotGeom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
}

impl DeskResponse {
    pub fn fail(id: u64, error: impl Into<String>, exit: bool) -> Self {
        Self {
            id,
            ok: false,
            error: Some(error.into()),
            exit,
            monitors: None,
            image_b64: None,
            mime: None,
            geom: None,
            locked: None,
            input: None,
            capture: None,
            note: None,
            report: None,
        }
    }

    pub fn bare(id: u64) -> Self {
        Self {
            id,
            ok: true,
            error: None,
            exit: false,
            monitors: None,
            image_b64: None,
            mime: None,
            geom: None,
            locked: None,
            input: None,
            capture: None,
            note: None,
            report: None,
        }
    }
}

pub fn encode_desk_request(req: &DeskRequest) -> Result<String, String> {
    serde_json::to_string(req).map_err(|err| format!("broker request: {err}"))
}

pub fn decode_desk_request(line: &str) -> Result<DeskRequest, String> {
    serde_json::from_str(line.trim()).map_err(|err| format!("broker request: {err}"))
}

pub fn encode_desk_response(resp: &DeskResponse) -> Result<String, String> {
    serde_json::to_string(resp).map_err(|err| format!("broker response: {err}"))
}

pub fn decode_desk_response(line: &str) -> Result<DeskResponse, String> {
    serde_json::from_str(line.trim()).map_err(|err| format!("broker response: {err}"))
}

pub fn desk_b64_decode(text: &str) -> Result<Vec<u8>, String> {
    fn val(byte: u8) -> Result<u8, String> {
        match byte {
            b'A'..=b'Z' => Ok(byte - b'A'),
            b'a'..=b'z' => Ok(byte - b'a' + 26),
            b'0'..=b'9' => Ok(byte - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("broker image is not base64".into()),
        }
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("broker image is not base64".into());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut i = 0;
    while i < bytes.len() {
        let pad = (bytes[i + 2] == b'=') as usize + (bytes[i + 3] == b'=') as usize;
        let a = val(bytes[i])?;
        let b = val(bytes[i + 1])?;
        let c = if bytes[i + 2] == b'=' { 0 } else { val(bytes[i + 2])? };
        let d = if bytes[i + 3] == b'=' { 0 } else { val(bytes[i + 3])? };
        let n = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | (d as u32);
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
        i += 4;
    }
    Ok(out)
}

fn attach_note(resp: &mut DeskResponse, backend: &mut dyn DesktopBackend) {
    if let Some(note) = backend.status_note() {
        if !note.is_empty() {
            resp.note = Some(note);
        }
    }
}

/// Run one broker request. The gate is the owner's switch, halt stamp, and lock.
/// The request cannot override it. Halt, a switch that is off, and the lock
/// screen close the session before the refusal is returned.
pub fn apply_desk_request(
    backend: &mut dyn DesktopBackend,
    gate: CallGate,
    req: &DeskRequest,
) -> DeskResponse {
    let id = req.id();
    if !gate.enabled {
        backend.suspend_input();
        return DeskResponse::fail(id, OFF_MSG, false);
    }
    if gate.halted {
        backend.release_input();
        return DeskResponse::fail(id, HALT_MSG, true);
    }
    if req.is_input() && backend.is_locked() {
        backend.release_input();
        return DeskResponse::fail(id, LOCK_MSG, false);
    }
    match req {
        DeskRequest::ListMonitors { .. } => match backend.list_monitors() {
            Ok(monitors) => DeskResponse {
                monitors: Some(monitors),
                ..DeskResponse::bare(id)
            },
            Err(err) => DeskResponse::fail(id, err, false),
        },
        DeskRequest::Screenshot { monitor, .. } => match backend.screenshot(monitor) {
            Ok(shot) => DeskResponse {
                image_b64: Some(b64(&shot.bytes)),
                mime: Some(shot.mime),
                geom: Some(shot.geom),
                ..DeskResponse::bare(id)
            },
            Err(err) => DeskResponse::fail(id, err, false),
        },
        DeskRequest::MoveAbs { x, y, .. } => match backend.move_abs(*x, *y) {
            Ok(()) => {
                let mut resp = DeskResponse::bare(id);
                attach_note(&mut resp, backend);
                resp
            }
            Err(err) => DeskResponse::fail(id, err, false),
        },
        DeskRequest::MoveOn { geom, x, y, .. } => match backend.move_abs_on(geom, *x, *y) {
            Ok(()) => {
                let mut resp = DeskResponse::bare(id);
                attach_note(&mut resp, backend);
                resp
            }
            Err(err) => DeskResponse::fail(id, err, false),
        },
        DeskRequest::Button { button, down, .. } => match backend.button(*button, *down) {
            Ok(()) => DeskResponse::bare(id),
            Err(err) => DeskResponse::fail(id, err, false),
        },
        DeskRequest::Scroll { dx, dy, .. } => match backend.scroll(*dx, *dy) {
            Ok(()) => DeskResponse::bare(id),
            Err(err) => DeskResponse::fail(id, err, false),
        },
        DeskRequest::TypeText { text, .. } => match backend.type_text(text) {
            Ok(()) => DeskResponse::bare(id),
            Err(err) => DeskResponse::fail(id, err, false),
        },
        DeskRequest::Key { keys, .. } => match parse_key_combo(keys).and_then(|combo| backend.key_combo(&combo)) {
            Ok(()) => DeskResponse::bare(id),
            Err(err) => DeskResponse::fail(id, err, false),
        },
        DeskRequest::Release { .. } => {
            backend.release_input();
            DeskResponse::bare(id)
        }
        DeskRequest::Locked { .. } => DeskResponse {
            locked: Some(backend.is_locked()),
            ..DeskResponse::bare(id)
        },
        DeskRequest::Status { .. } => {
            let mut resp = DeskResponse::bare(id);
            attach_note(&mut resp, backend);
            resp
        }
        DeskRequest::Probe { .. } => match probe_desktop(backend, gate) {
            Ok(report) => DeskResponse {
                report: Some(report),
                ..DeskResponse::bare(id)
            },
            Err(err) => DeskResponse::fail(id, err, gate.halted),
        },
    }
}

pub fn format_probe_line(
    name: &str,
    cx: i32,
    cy: i32,
    moved: &Result<(), String>,
    captured: &Result<(), String>,
    offset: Option<(i32, i32)>,
) -> String {
    let move_text = match moved {
        Ok(()) => "moved".to_string(),
        Err(err) => format!("move failed ({err})"),
    };
    let shot_text = match captured {
        Ok(()) => "captured".to_string(),
        Err(err) => format!("capture failed ({err})"),
    };
    let offset_text = match offset {
        Some((dx, dy)) => format!("offset {dx},{dy}"),
        None => "offset not measured".to_string(),
    };
    format!("{name} center ({cx}, {cy}): {move_text}, {shot_text}, {offset_text}")
}

/// Move to each monitor center and capture. Offset is reported only when the
/// backend can see where the pointer landed. Halt and a switch that is off
/// close the session and refuse.
pub fn probe_desktop(backend: &mut dyn DesktopBackend, gate: CallGate) -> Result<String, String> {
    if !gate.enabled {
        backend.suspend_input();
        return Err(OFF_MSG.to_string());
    }
    if gate.halted {
        backend.release_input();
        return Err(HALT_MSG.to_string());
    }
    if backend.is_locked() {
        backend.release_input();
        return Err(LOCK_MSG.to_string());
    }
    let monitors = backend.list_monitors()?;
    if monitors.is_empty() {
        return Err("No monitors.".into());
    }
    let mut lines = Vec::with_capacity(monitors.len());
    for mon in &monitors {
        let (cx, cy) = monitor_center(mon);
        let moved = backend.move_abs(cx, cy);
        let captured = backend.screenshot(&mon.id).map(|_| ());
        let offset = backend
            .landed_at()
            .map(|(x, y)| (x.saturating_sub(cx), y.saturating_sub(cy)));
        lines.push(format_probe_line(&mon.name, cx, cy, &moved, &captured, offset));
    }
    Ok(lines.join("\n"))
}
