//! The desktop tools (screenshots, pointer, keys, windows) behind the hard
//! gate, shared by the cabin and the Tauri shell. Also the headless
//! `grokhub --mcp-desktop` stdio server: stdout is JSON-RPC only, logs go to
//! stderr. The hosting app installs a [`host::Host`] first.

mod apps;
#[cfg(target_os = "linux")]
mod broker;
#[cfg(target_os = "linux")]
mod capture;
#[cfg(target_os = "linux")]
mod fallback;
mod harness_gate;
pub mod host;
mod keys;
#[cfg(target_os = "linux")]
mod kwin_shot;
#[cfg(any(unix, test))]
mod outputs;
#[cfg(any(unix, test))]
mod pixels;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "linux")]
pub mod screen;
#[cfg(target_os = "linux")]
mod uinput_dev;
#[cfg(unix)]
mod wayland;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod x11;

use std::io::{BufRead, Write};
use std::sync::{mpsc, Mutex};

use grokhub_core::desktop_mcp::{stamp_halts, CallGate, DesktopBackend, DesktopServer};

const LINE_CAP: usize = 1 << 20;
const PNG_JPEG_AT: usize = 1_400_000;

pub fn run_stdio() -> i32 {
    #[cfg(windows)]
    windows::enable_per_monitor_dpi();
    let started = process_started_ms();
    let mut server = DesktopServer::new(env!("CARGO_PKG_VERSION"), LiveBackend::new());
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    loop {
        let line = match read_line_capped(&mut reader, LINE_CAP) {
            Ok(None) => break,
            Ok(Some(line)) => line,
            Err(msg) => {
                eprintln!("desktop-mcp: {msg}");
                emit(&grokhub_core::desktop_mcp::rpc_parse_error());
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let gate = CallGate {
            enabled: crate::host::settings().desktop_control,
            halted: read_halt_stamp().is_some_and(|ms| stamp_halts(ms, started)),
        };
        // Path A: the cabin pre-check runs before every desktop tool, on every OS.
        let dir = crate::host::config_dir();
        let mut halted = || read_halt_stamp().is_some_and(|ms| stamp_halts(ms, started));
        let outcome = harness_gate::handle_desk_line(&mut server, &line, gate, &dir, &mut halted);
        if let Some(reply) = outcome.reply {
            emit(&reply);
        }
        if outcome.exit {
            break;
        }
    }
    0
}

/// Spike-2a: `grokhub --mcp-cua`. The cabin's gate proxy in front of a
/// `cua-driver mcp` child (Linux only, `cuaDriver` flag). Every call goes
/// through `harness::decide` before it is forwarded; the child starts on the
/// first call that may use it, so with the flag or the switch off none starts.
pub fn run_cua_stdio() -> i32 {
    use grokhub_agent::harness as hx;
    let started = process_started_ms();
    let dir = crate::host::config_dir();
    let child_dir = dir.clone();
    let mut proxy: hx::CuaProxy<hx::StdioChild> =
        hx::CuaProxy::new(Box::new(move || start_cua_child(&child_dir)));
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    loop {
        let line = match read_line_capped(&mut reader, LINE_CAP) {
            Ok(None) => break,
            Ok(Some(line)) => line,
            Err(msg) => {
                eprintln!("cua-mcp: {msg}");
                emit(&grokhub_core::desktop_mcp::rpc_parse_error());
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let cfg = crate::host::settings();
        let mut halted = || read_halt_stamp().is_some_and(|ms| stamp_halts(ms, started));
        let gate = hx::CuaGate {
            enabled: cfg.desktop_control,
            flag: cfg.cua_driver,
            halted: halted(),
        };
        if let Some(reply) = proxy.handle_line(&line, gate, &dir, &mut halted) {
            emit(&reply);
        }
    }
    0
}

/// Linux only, the pinned release only, from `cuaDriverPath` or PATH.
fn start_cua_child(dir: &std::path::Path) -> Result<grokhub_agent::harness::StdioChild, String> {
    use grokhub_agent::harness as hx;
    if !cfg!(target_os = "linux") {
        return Err((*hx::CUA_LINUX_ONLY_MSG).into());
    }
    let bin = hx::find_cua_driver(&crate::host::settings().cua_driver_path)
        .ok_or_else(|| hx::CUA_MISSING_MSG.to_string())?;
    hx::verify_cua_driver(&bin)?;
    hx::spawn_cua_child(&bin, dir)
}

/// The Cua proxy is registered only while desktop control and the `cuaDriver`
/// flag are both on (and only on Linux).
pub fn cua_wanted(desktop_control: bool, cua_flag: bool) -> bool {
    grokhub_agent::harness::ComputerUseBackend::selected(cua_flag, desktop_control)
        == grokhub_agent::harness::ComputerUseBackend::CuaDriver
}

fn emit(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

pub fn read_line_capped(reader: &mut impl BufRead, cap: usize) -> Result<Option<String>, String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = reader.read(&mut byte).map_err(|e| format!("stdin: {e}"))?;
        if n == 0 {
            if buf.is_empty() {
                return Ok(None);
            }
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        if byte[0] == b'\r' {
            continue;
        }
        if buf.len() >= cap {
            loop {
                let n = reader.read(&mut byte).map_err(|e| format!("stdin: {e}"))?;
                if n == 0 || byte[0] == b'\n' {
                    break;
                }
            }
            return Err("line longer than 1 MiB".into());
        }
        buf.push(byte[0]);
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

pub fn encode_rgba(rgba: &[u8], w: u32, h: u32) -> Result<(Vec<u8>, String, u32, u32), String> {
    if w == 0 || h == 0 {
        return Err("empty screenshot".into());
    }
    if rgba.len() < (w as usize) * (h as usize) * 4 {
        return Err("screenshot buffer does not match its size".into());
    }
    let (nw, nh) = grokhub_core::desktop_mcp::fit_downscale(w, h);
    let owned = if nw == w && nh == h {
        rgba[..(w as usize) * (h as usize) * 4].to_vec()
    } else {
        let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
            .ok_or_else(|| "screenshot buffer does not match its size".to_string())?;
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::CatmullRom).into_raw()
    };
    let mut png = Vec::new();
    {
        let enc = image::codecs::png::PngEncoder::new(&mut png);
        image::ImageEncoder::write_image(enc, &owned, nw, nh, image::ExtendedColorType::Rgba8)
            .map_err(|e| format!("png: {e}"))?;
    }
    if png.len() > PNG_JPEG_AT {
        let img = image::RgbaImage::from_raw(nw, nh, owned).ok_or_else(|| "rgba".to_string())?;
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        let mut jpeg = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85);
        image::ImageEncoder::write_image(enc, rgb.as_raw(), nw, nh, image::ExtendedColorType::Rgb8)
            .map_err(|e| format!("jpeg: {e}"))?;
        return Ok((jpeg, "image/jpeg".into(), nw, nh));
    }
    Ok((png, "image/png".into(), nw, nh))
}

pub fn write_halt_stamp() {
    let path = crate::host::config_dir().join("desktop-halt.stamp");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, grokhub_core::now_ms().to_string());
}

pub fn read_halt_stamp() -> Option<u64> {
    let path = crate::host::config_dir().join("desktop-halt.stamp");
    let text = std::fs::read_to_string(path).ok()?;
    text.trim().parse().ok()
}

/// A program on PATH or in the usual install places.
pub fn resolve_bin(name: &str) -> Option<std::path::PathBuf> {
    grokhub_core::resolve_bin_in(
        name,
        std::env::var("PATH").ok().as_deref(),
        grokhub_core::user_home().as_ref().and_then(|p| p.to_str()),
    )
}

/// Move files to the trash (Linux) or the Recycle Bin (Windows). Refuses
/// on Windows when a path's Recycle Bin is missing or turned off, where it
/// would delete for good.
pub fn trash_paths(paths: &[std::path::PathBuf]) -> Result<(), String> {
    trash_guarded(paths, apps::no_recycle_bin, apps::trash)
}

const NO_BIN_MSG: &str = "That drive's Recycle Bin is missing or turned off, so nothing was moved.";

fn trash_guarded(
    paths: &[std::path::PathBuf],
    no_bin: impl Fn(&std::path::Path) -> bool,
    trash: impl FnOnce(&[std::path::PathBuf]) -> Result<(), String>,
) -> Result<(), String> {
    if paths.iter().any(|p| no_bin(p)) {
        return Err(NO_BIN_MSG.into());
    }
    trash(paths)
}

pub fn which(name: &str) -> bool {
    resolve_bin(name).is_some()
}

static PROCESS_STARTED_MS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

pub fn process_started_ms() -> u64 {
    *PROCESS_STARTED_MS.get_or_init(grokhub_core::now_ms)
}

#[cfg(target_os = "linux")]
static INPUT_BACKEND: Mutex<Option<String>> = Mutex::new(None);
#[cfg(target_os = "linux")]
static CAPTURE_BACKEND: Mutex<Option<String>> = Mutex::new(None);

#[cfg(target_os = "linux")]
pub(crate) fn publish_input_backend(label: &str) {
    *INPUT_BACKEND.lock().unwrap_or_else(|err| err.into_inner()) = Some(label.to_string());
}

#[cfg(target_os = "linux")]
pub(crate) fn publish_capture_backend(label: &str) {
    *CAPTURE_BACKEND
        .lock()
        .unwrap_or_else(|err| err.into_inner()) = Some(label.to_string());
}

pub fn desktop_panel_lines() -> String {
    grokhub_core::desktop_mcp::desktop_control_status(
        &active_input_label(),
        &active_capture_label(),
    )
}

fn active_input_label() -> String {
    #[cfg(target_os = "linux")]
    if let Some(label) = INPUT_BACKEND
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
    {
        return label;
    }
    predict_input_label()
}

fn active_capture_label() -> String {
    #[cfg(target_os = "linux")]
    if let Some(label) = CAPTURE_BACKEND
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
    {
        return label;
    }
    predict_capture_label()
}

fn predict_input_label() -> String {
    #[cfg(windows)]
    {
        "Windows".into()
    }
    #[cfg(unix)]
    {
        match session_stack().input {
            grokhub_core::desktop_mcp::DesktopInputKind::Portal => {
                "RemoteDesktop portal (libei)".into()
            }
            grokhub_core::desktop_mcp::DesktopInputKind::Ydotool => "ydotool (imprecise)".into(),
            grokhub_core::desktop_mcp::DesktopInputKind::X11rb => "X11".into(),
            grokhub_core::desktop_mcp::DesktopInputKind::None => "unavailable".into(),
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        "unavailable".into()
    }
}

fn predict_capture_label() -> String {
    #[cfg(windows)]
    {
        "Windows".into()
    }
    #[cfg(unix)]
    {
        match session_stack().capture {
            grokhub_core::desktop_mcp::DesktopCaptureKind::ScreenShot2 => "KWin ScreenShot2".into(),
            grokhub_core::desktop_mcp::DesktopCaptureKind::Grim => "grim".into(),
            grokhub_core::desktop_mcp::DesktopCaptureKind::X11rb => "X11".into(),
            grokhub_core::desktop_mcp::DesktopCaptureKind::None => "unavailable".into(),
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        "unavailable".into()
    }
}

#[cfg(unix)]
fn session_stack() -> grokhub_core::desktop_mcp::DesktopStack {
    let env = grokhub_core::desktop_mcp::DesktopSessionEnv {
        wayland_display: std::env::var("WAYLAND_DISPLAY").ok(),
        session_type: std::env::var("XDG_SESSION_TYPE").ok(),
        current_desktop: std::env::var("XDG_CURRENT_DESKTOP").ok(),
        display: std::env::var("DISPLAY").ok(),
    };
    grokhub_core::desktop_mcp::select_desktop_stack(
        &env,
        &grokhub_core::desktop_mcp::DesktopProbes { kwin: false },
    )
}

static TEST_RX: Mutex<Option<mpsc::Receiver<String>>> = Mutex::new(None);

pub fn request_desktop_test(enabled: bool) -> String {
    if !enabled {
        set_desktop_enabled(false);
        return grokhub_core::desktop_mcp::OFF_MSG.to_string();
    }
    if read_halt_stamp().is_some_and(|ms| stamp_halts(ms, process_started_ms())) {
        note_halt();
        return (*grokhub_core::desktop_mcp::HALT_MSG).to_string();
    }
    #[cfg(all(target_os = "linux", not(test)))]
    {
        spawn_desktop_probe()
    }
    #[cfg(not(all(target_os = "linux", not(test))))]
    {
        "Desktop test did not open a session.".into()
    }
}

#[cfg(all(target_os = "linux", not(test)))]
fn spawn_desktop_probe() -> String {
    if let Err(err) = broker::ensure_started() {
        return err;
    }
    let (tx, rx) = mpsc::channel();
    if let Ok(mut slot) = TEST_RX.lock() {
        *slot = Some(rx);
    }
    std::thread::spawn(move || {
        let msg = match broker::run_probe() {
            Ok(report) => report,
            Err(err) => err,
        };
        let _ = tx.send(msg);
    });
    "Testing each monitor...".into()
}

pub fn take_desktop_test_status() -> Option<String> {
    let mut guard = TEST_RX.lock().ok()?;
    let rx = guard.as_mut()?;
    match rx.try_recv() {
        Ok(msg) => {
            *guard = None;
            Some(msg)
        }
        Err(mpsc::TryRecvError::Empty) => None,
        Err(mpsc::TryRecvError::Disconnected) => {
            *guard = None;
            None
        }
    }
}

pub fn set_desktop_enabled(on: bool) {
    #[cfg(all(target_os = "linux", not(test)))]
    broker::set_enabled(on);
    #[cfg(not(all(target_os = "linux", not(test))))]
    let _ = on;
}

pub fn note_halt() {
    #[cfg(all(target_os = "linux", not(test)))]
    broker::note_halt();
}

struct LiveBackend {
    inner: Option<Box<dyn DesktopBackend>>,
    error: Option<String>,
    tried: bool,
}

impl LiveBackend {
    fn new() -> Self {
        Self {
            inner: None,
            error: None,
            tried: false,
        }
    }

    fn ensure(&mut self) -> Result<&mut dyn DesktopBackend, String> {
        if self.inner.is_none() && !self.tried {
            self.tried = true;
            match connect_backend() {
                Ok(backend) => self.inner = Some(backend),
                Err(e) => {
                    eprintln!("desktop-mcp: {e}");
                    self.error = Some(e);
                }
            }
        }
        if let Some(backend) = self.inner.as_mut() {
            return Ok(backend.as_mut());
        }
        Err(self
            .error
            .clone()
            .unwrap_or_else(|| "No desktop backend.".into()))
    }
}

impl DesktopBackend for LiveBackend {
    fn list_monitors(&mut self) -> Result<Vec<grokhub_core::desktop_mcp::MonitorGeom>, String> {
        self.ensure()?.list_monitors()
    }
    fn screenshot(
        &mut self,
        monitor: &str,
    ) -> Result<grokhub_core::desktop_mcp::CapturedShot, String> {
        self.ensure()?.screenshot(monitor)
    }
    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.ensure()?.move_abs(x, y)
    }
    fn button(
        &mut self,
        button: grokhub_core::desktop_mcp::MouseButton,
        down: bool,
    ) -> Result<(), String> {
        self.ensure()?.button(button, down)
    }
    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        self.ensure()?.scroll(dx, dy)
    }
    fn type_text(&mut self, text: &str) -> Result<(), String> {
        self.ensure()?.type_text(text)
    }
    fn key_combo(&mut self, combo: &grokhub_core::desktop_mcp::KeyCombo) -> Result<(), String> {
        self.ensure()?.key_combo(combo)
    }
    fn is_locked(&mut self) -> bool {
        self.ensure().map(|b| b.is_locked()).unwrap_or(false)
    }
    fn pace(&mut self) {
        if let Ok(b) = self.ensure() {
            b.pace();
        }
    }
    fn release_input(&mut self) {
        if let Some(backend) = self.inner.as_mut() {
            backend.release_input();
        }
    }
    fn suspend_input(&mut self) {
        if let Some(backend) = self.inner.as_mut() {
            backend.suspend_input();
        }
    }
    fn move_abs_on(
        &mut self,
        geom: &grokhub_core::desktop_mcp::ShotGeom,
        x: i32,
        y: i32,
    ) -> Result<(), String> {
        self.ensure()?.move_abs_on(geom, x, y)
    }
    fn status_note(&mut self) -> Option<String> {
        self.ensure().ok().and_then(|backend| backend.status_note())
    }
    // Open, focus, list, and trash go straight to the OS, not the input
    // backend, so they work on every session the input route supports.
    fn open_app(&mut self, app: &str) -> Result<(), String> {
        apps::open_app(app)
    }
    fn focus_window(&mut self, title: &str) -> Result<String, String> {
        apps::focus_window(title)
    }
    fn list_windows(&mut self) -> Result<grokhub_core::desktop_mcp::DesktopWindows, String> {
        apps::list_windows()
    }
    fn window_geometry(
        &mut self,
        title: &str,
    ) -> Result<grokhub_core::desktop_mcp::WindowGeom, String> {
        apps::window_geometry(title)
    }
    fn set_window_geometry(
        &mut self,
        title: &str,
        geom: &grokhub_core::desktop_mcp::WindowGeom,
    ) -> Result<String, String> {
        apps::set_window_geometry(title, geom)
    }
    fn trash(&mut self, paths: &[std::path::PathBuf]) -> Result<(), String> {
        apps::trash(paths)
    }
    // Spike-2b: AT-SPI on Linux. Windows keeps the default (unknown): no UIA reader (D3).
    #[cfg(target_os = "linux")]
    fn target_at(&mut self, x: i32, y: i32) -> Option<grokhub_core::desktop_mcp::ClickTarget> {
        crate::host::target_at(x, y)
    }
}

pub struct NativeDesktop {
    inner: Mutex<DesktopServer<LiveBackend>>,
}

impl NativeDesktop {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(DesktopServer::new(
                env!("CARGO_PKG_VERSION"),
                LiveBackend::new(),
            )),
        }
    }
}

impl Default for NativeDesktop {
    fn default() -> Self {
        Self::new()
    }
}

impl grokhub_agent::DesktopOps for NativeDesktop {
    fn halted(&self) -> bool {
        read_halt_stamp().is_some_and(|ms| stamp_halts(ms, process_started_ms()))
    }

    fn locked(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .backend_mut()
            .is_locked()
    }

    fn call(&self, name: &str, args: &serde_json::Value) -> grokhub_agent::ToolOutput {
        let mut server = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        match server.invoke(name, args) {
            Ok(body) => mcp_tool_output(body),
            Err(err) => grokhub_agent::ToolOutput::err(err),
        }
    }

    fn hint(&self, name: &str, args: &serde_json::Value) -> serde_json::Value {
        let mut hinted = args.clone();
        let mut server = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        harness_gate::add_hints(&mut server, name, &mut hinted);
        hinted
    }
}

fn mcp_tool_output(body: serde_json::Value) -> grokhub_agent::ToolOutput {
    let failed = body
        .get("isError")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let mut text = String::new();
    let mut image = None;
    if let Some(parts) = body.get("content").and_then(|value| value.as_array()) {
        for part in parts {
            match part.get("type").and_then(|value| value.as_str()) {
                Some("text") => {
                    if let Some(line) = part.get("text").and_then(|value| value.as_str()) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(line);
                    }
                }
                Some("image") => {
                    let data = part
                        .get("data")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    let mime = part
                        .get("mimeType")
                        .and_then(|value| value.as_str())
                        .unwrap_or("image/png");
                    if !data.is_empty() {
                        image = Some(format!("data:{mime};base64,{data}"));
                    }
                }
                _ => {}
            }
        }
    }
    grokhub_agent::ToolOutput {
        text,
        image_data_url: image,
        failed,
    }
}

/// Full-screen stills for "Record my screen", one per call, as `(bytes, mime)`.
/// Connects to the desktop backend on the first call and keeps it.
pub fn screen_grabber() -> impl FnMut() -> Result<(Vec<u8>, String), String> {
    let mut backend: Option<Box<dyn DesktopBackend>> = None;
    move || {
        if backend.is_none() {
            backend = Some(connect_backend()?);
        }
        let shot = backend
            .as_mut()
            .ok_or_else(|| "no desktop backend".to_string())?
            .screenshot("all")?;
        Ok((shot.bytes, shot.mime))
    }
}

fn connect_backend() -> Result<Box<dyn DesktopBackend>, String> {
    #[cfg(windows)]
    {
        Ok(Box::new(windows::WindowsBackend::new()))
    }
    #[cfg(unix)]
    {
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        let session = std::env::var("XDG_SESSION_TYPE").ok();
        if grokhub_core::session_is_wayland(wayland.as_deref(), session.as_deref()) {
            #[cfg(target_os = "linux")]
            {
                return broker::connect_or_own();
            }
            #[cfg(not(target_os = "linux"))]
            {
                return Ok(Box::new(wayland::WaylandBackend::new()));
            }
        }
        if std::env::var_os("DISPLAY").is_some() {
            return x11::X11Backend::connect().map(|b| Box::new(b) as Box<dyn DesktopBackend>);
        }
        Err(
            "No display. Log into an X11 session, or a Wayland session with grim and ydotool."
                .into(),
        )
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err("Desktop control is not available on this system.".into())
    }
}

/// COMPUTER_CMD hands on a [`DesktopBackend`]: click, double-click, move,
/// type, key and scroll. `None` for the ops that read the screen instead
/// (act, wait_for, tabs, cursor, move to a monitor).
#[cfg(any(windows, test))]
pub(crate) fn drive_hands<B: DesktopBackend>(
    b: &mut B,
    op: &grokhub_core::recipe::ComputerOp,
) -> Option<Result<String, String>> {
    use grokhub_core::desktop_mcp::{parse_key_combo, MouseButton};
    use grokhub_core::recipe::ComputerOp;
    fn click<B: DesktopBackend>(b: &mut B, x: i32, y: i32, times: u8) -> Result<(), String> {
        b.move_abs(x, y)?;
        for _ in 0..times {
            b.button(MouseButton::Left, true)?;
            b.button(MouseButton::Left, false)?;
        }
        Ok(())
    }
    Some(match op {
        ComputerOp::Click { x, y } => click(b, *x, *y, 1).map(|()| format!("clicked {x},{y}")),
        ComputerOp::DoubleClick { x, y } => {
            click(b, *x, *y, 2).map(|()| format!("double-clicked {x},{y}"))
        }
        ComputerOp::Move { x, y } => b.move_abs(*x, *y).map(|()| format!("moved {x},{y}")),
        ComputerOp::Type { text } => b
            .type_text(text)
            .map(|()| format!("typed {} chars", text.chars().count())),
        ComputerOp::Key { name } => parse_key_combo(name)
            .and_then(|c| b.key_combo(&c))
            .map(|()| format!("key {name}")),
        // COMPUTER_CMD scroll: a positive count is up, as a wheel notch is.
        ComputerOp::Scroll { dy } => b.scroll(0, *dy).map(|()| format!("scrolled {dy}")),
        ComputerOp::Act { .. }
        | ComputerOp::WaitFor { .. }
        | ComputerOp::Tab { .. }
        | ComputerOp::Cursor
        | ComputerOp::MoveMonitor { .. } => return None,
    })
}

/// Windows Eyes: the primary monitor through xcap, the grabber the desktop
/// tools' screenshot uses, since the Linux capture tools are not there.
#[cfg(windows)]
pub fn windows_primary_frame() -> Result<image::RgbaImage, String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let primary = monitors
        .iter()
        .position(|m| m.is_primary().unwrap_or(false))
        .unwrap_or(0);
    let mon = monitors.get(primary).ok_or("no monitor")?;
    mon.capture_image().map_err(|e| e.to_string())
}

/// Windows COMPUTER_CMD hands: SendInput through the desktop tools' own
/// backend, since xdotool and ydotool are Linux tools.
#[cfg(windows)]
pub fn windows_hands(op: &grokhub_core::recipe::ComputerOp) -> Option<Result<String, String>> {
    drive_hands(&mut windows::WindowsBackend::new(), op)
}

#[cfg(test)]
pub(crate) fn test_root(label: &str) -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("grokhub-{label}-{}-{n}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_volume_without_a_usable_recycle_bin_moves_nothing() {
        let paths = vec![
            std::path::PathBuf::from(r"C:\Users\user\Downloads\a.zip"),
            std::path::PathBuf::from(r"D:\Downloads\b.iso"),
        ];
        let mut called = false;
        let got = trash_guarded(
            &paths,
            |p| p.to_string_lossy().starts_with('D'),
            |_| {
                called = true;
                Ok(())
            },
        );
        assert_eq!(got, Err(NO_BIN_MSG.to_string()));
        assert!(!called, "nothing reaches the Recycle Bin call");
        let mut moved = Vec::new();
        let got = trash_guarded(
            &paths,
            |_| false,
            |p| {
                moved = p.to_vec();
                Ok(())
            },
        );
        assert_eq!(got, Ok(()));
        assert_eq!(moved, paths);
    }

    #[test]
    fn cua_proxy_is_wanted_only_with_the_flag_and_the_switch_on_linux() {
        assert!(
            !cua_wanted(true, false),
            "flag off (the default): Grok Build only"
        );
        assert!(!cua_wanted(false, true), "switch off: not registered");
        assert!(!cua_wanted(false, false));
        assert_eq!(cua_wanted(true, true), cfg!(target_os = "linux"));
    }

    #[test]
    fn desktop_mcp_png_encode_and_jpeg_api() {
        let rgba = vec![
            255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let (bytes, mime, w, h) = encode_rgba(&rgba, 2, 2).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!((w, h), (2, 2));
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let rgb = vec![0u8; 8 * 8 * 3];
        let mut jpeg = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85);
        image::ImageEncoder::write_image(enc, &rgb, 8, 8, image::ExtendedColorType::Rgb8).unwrap();
        assert!(jpeg.len() > 10);
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn desktop_mcp_halt_stamp_round_trip() {
        let root = test_root("desktop-mcp-halt");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let _pin = grokhub_agent::perm::ConfigGuard::set(&root);
        assert!(read_halt_stamp().is_none());
        write_halt_stamp();
        let ms = read_halt_stamp().expect("stamp");
        assert!(stamp_halts(ms, ms.saturating_sub(1)));
        assert!(!stamp_halts(ms, ms));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_xdotool_on_wayland() {
        let wayland = include_str!("wayland.rs");
        let portal = include_str!("portal.rs");
        assert!(
            !wayland.contains("xdotool"),
            "wayland input must not name the xwayland tool"
        );
        assert!(
            !portal.contains("xdotool"),
            "portal input must not name the xwayland tool"
        );
    }

    #[test]
    fn udev_rule_tags_uaccess() {
        let text = include_str!("../../../packaging/udev/60-grokhub-uinput.rules");
        assert!(text.contains("KERNEL==\"uinput\""));
        assert!(text.contains("GROUP=\"input\""));
        assert!(text.contains("MODE=\"0660\""));
        assert!(text.contains("TAG+=\"uaccess\""));
        assert!(text.contains("OPTIONS+=\"static_node=uinput\""));
        assert!(
            !text.contains("0666") && !text.contains("0777"),
            "uinput must never be world-writable"
        );
        assert_eq!(
            text.lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                .count(),
            1,
            "one uinput rule only"
        );
    }

    #[test]
    fn desktop_entry_allows_kwin_screenshot2() {
        let text = include_str!("../../../packaging/grokhub.desktop");
        assert!(text.lines().any(|line| line == "Version=1.5"));
        assert!(text.lines().any(|line| line == "Exec=grokhub"));
        assert!(text
            .lines()
            .any(|line| line == "X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"));
    }
    /// Records what COMPUTER_CMD hands sent to the backend.
    #[derive(Default)]
    struct Hands {
        sent: Vec<String>,
    }

    impl DesktopBackend for Hands {
        fn list_monitors(&mut self) -> Result<Vec<grokhub_core::desktop_mcp::MonitorGeom>, String> {
            Ok(vec![])
        }
        fn screenshot(
            &mut self,
            _monitor: &str,
        ) -> Result<grokhub_core::desktop_mcp::CapturedShot, String> {
            Err("no screen".into())
        }
        fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
            self.sent.push(format!("move {x} {y}"));
            Ok(())
        }
        fn button(
            &mut self,
            b: grokhub_core::desktop_mcp::MouseButton,
            down: bool,
        ) -> Result<(), String> {
            self.sent
                .push(format!("{b:?} {}", if down { "down" } else { "up" }));
            Ok(())
        }
        fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
            self.sent.push(format!("scroll {dx} {dy}"));
            Ok(())
        }
        fn type_text(&mut self, text: &str) -> Result<(), String> {
            self.sent.push(format!("type {text}"));
            Ok(())
        }
        fn key_combo(&mut self, c: &grokhub_core::desktop_mcp::KeyCombo) -> Result<(), String> {
            self.sent.push(format!("key ctrl={} {:?}", c.ctrl, c.key));
            Ok(())
        }
        fn is_locked(&mut self) -> bool {
            false
        }
    }

    #[test]
    fn computer_cmd_hands_drive_the_desktop_backend() {
        use grokhub_core::recipe::ComputerOp;
        let mut b = Hands::default();
        let got = drive_hands(&mut b, &ComputerOp::DoubleClick { x: 40, y: 7 });
        assert_eq!(got, Some(Ok("double-clicked 40,7".to_string())));
        assert_eq!(
            b.sent,
            ["move 40 7", "Left down", "Left up", "Left down", "Left up"]
        );

        let mut b = Hands::default();
        assert_eq!(
            drive_hands(
                &mut b,
                &ComputerOp::Key {
                    name: "ctrl+l".into()
                }
            ),
            Some(Ok("key ctrl+l".to_string()))
        );
        assert_eq!(
            drive_hands(
                &mut b,
                &ComputerOp::Type {
                    text: "héllo".into()
                }
            ),
            Some(Ok("typed 5 chars".to_string()))
        );
        assert_eq!(
            drive_hands(&mut b, &ComputerOp::Scroll { dy: 3 }),
            Some(Ok("scrolled 3".to_string()))
        );
        assert_eq!(
            b.sent,
            ["key ctrl=true Char('l')", "type héllo", "scroll 0 3"]
        );

        let mut b = Hands::default();
        assert!(drive_hands(
            &mut b,
            &ComputerOp::Key {
                name: "nosuchkey".into()
            }
        )
        .unwrap()
        .is_err());
        assert_eq!(
            drive_hands(&mut b, &ComputerOp::Cursor),
            None,
            "reads stay on the shared path"
        );
        assert!(b.sent.is_empty());
    }
}
