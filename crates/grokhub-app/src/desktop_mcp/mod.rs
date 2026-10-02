//! Headless `grokhub --mcp-desktop` stdio server.
//! Stdout is JSON-RPC only. Logs go to stderr.

mod keys;
#[cfg(any(unix, test))]
mod outputs;
#[cfg(any(unix, test))]
mod pixels;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
mod x11;
#[cfg(unix)]
mod wayland;

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};

use grokhub_core::desktop_mcp::{
    stamp_halts, CallGate, DesktopBackend, DesktopServer,
};

const LINE_CAP: usize = 1 << 20;
const PNG_JPEG_AT: usize = 1_400_000;

static REG_BUSY: AtomicBool = AtomicBool::new(false);
static REG_RX: Mutex<Option<mpsc::Receiver<String>>> = Mutex::new(None);

pub fn run_stdio() -> i32 {
    #[cfg(windows)]
    windows::enable_per_monitor_dpi();
    let started = grokhub_core::now_ms();
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
            enabled: crate::config::load().desktop_control,
            halted: read_halt_stamp().is_some_and(|ms| stamp_halts(ms, started)),
        };
        let outcome = server.handle_line(&line, gate);
        if let Some(reply) = outcome.reply {
            emit(&reply);
        }
        if outcome.exit {
            break;
        }
    }
    0
}

fn emit(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn read_line_capped(reader: &mut impl BufRead, cap: usize) -> Result<Option<String>, String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = reader
            .read(&mut byte)
            .map_err(|e| format!("stdin: {e}"))?;
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

pub(crate) fn encode_rgba(rgba: &[u8], w: u32, h: u32) -> Result<(Vec<u8>, String, u32, u32), String> {
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
        let img = image::RgbaImage::from_raw(nw, nh, owned)
            .ok_or_else(|| "rgba".to_string())?;
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        let mut jpeg = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85);
        image::ImageEncoder::write_image(enc, rgb.as_raw(), nw, nh, image::ExtendedColorType::Rgb8)
            .map_err(|e| format!("jpeg: {e}"))?;
        return Ok((jpeg, "image/jpeg".into(), nw, nh));
    }
    Ok((png, "image/png".into(), nw, nh))
}

pub(crate) fn write_halt_stamp() {
    let path = crate::config::config_dir().join("desktop-halt.stamp");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, grokhub_core::now_ms().to_string());
}

pub(crate) fn read_halt_stamp() -> Option<u64> {
    let path = crate::config::config_dir().join("desktop-halt.stamp");
    let text = std::fs::read_to_string(path).ok()?;
    text.trim().parse().ok()
}

pub(crate) fn reg_busy() -> bool {
    REG_BUSY.load(Ordering::SeqCst)
}

pub(crate) fn take_reg_status() -> Option<String> {
    let mut guard = REG_RX.lock().ok()?;
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

pub(crate) fn spawn_register(on: bool) {
    start_register(on, true);
}

#[cfg(not(test))]
pub(crate) fn maybe_register_on_start(enabled: bool) {
    if !enabled || cabin_desktop_registered() {
        return;
    }
    start_register(true, false);
}

fn start_register(on: bool, announce: bool) {
    if REG_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = mpsc::channel();
    if let Ok(mut slot) = REG_RX.lock() {
        *slot = Some(rx);
    }
    std::thread::spawn(move || {
        let msg = run_register(on);
        if announce || msg.starts_with("Could not") {
            let _ = tx.send(msg);
        }
        REG_BUSY.store(false, Ordering::SeqCst);
    });
}

fn run_register(on: bool) -> String {
    let Some(bin) = grokhub_acp::find_grok() else {
        return "Could not register desktop tools: Grok Build CLI is not installed.".into();
    };
    let cwd = crate::config::config_dir();
    let result = if on {
        match std::env::current_exe() {
            Ok(exe) => grokhub_acp::register_desktop_mcp(&bin, &cwd, &exe),
            Err(e) => Err(e.to_string()),
        }
    } else {
        grokhub_acp::unregister_desktop_mcp(&bin, &cwd)
    };
    match result {
        Ok(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                if on {
                    "Desktop tools registered.".into()
                } else {
                    "Desktop tools removed.".into()
                }
            } else {
                trimmed.chars().take(240).collect()
            }
        }
        Err(e) => format!("Could not register desktop tools: {e}"),
    }
}

#[cfg(not(test))]
fn cabin_desktop_registered() -> bool {
    let Some(home) = grokhub_acp::cabin_grok_home() else {
        return false;
    };
    // An update can move the exe. Re-register unless this exact binary is listed.
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let raw = exe.display().to_string();
    // TOML basic strings double the backslashes in a Windows path; literal strings do not.
    let escaped = raw.replace('\\', "\\\\");
    std::fs::read_to_string(home.join("config.toml"))
        .ok()
        .is_some_and(|text| {
            text.contains(grokhub_core::DESKTOP_MCP_SERVER)
                && (text.contains(&raw) || text.contains(&escaped))
        })
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
            return Ok(Box::new(wayland::WaylandBackend::new()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_mcp_png_encode_and_jpeg_api() {
        let rgba = vec![255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
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
        let root = crate::config::test_config_root("desktop-mcp-halt");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        assert!(read_halt_stamp().is_none());
        write_halt_stamp();
        let ms = read_halt_stamp().expect("stamp");
        assert!(stamp_halts(ms, ms.saturating_sub(1)));
        assert!(!stamp_halts(ms, ms));
        let _ = std::fs::remove_dir_all(&root);
    }
}
