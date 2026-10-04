//! Desktop broker. The cabin holds the portal session and the lock.
//! `grokhub --mcp-desktop` speaks line-delimited JSON on `desk.sock`.
//! A second process cannot open another session while the lock is held.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
#[cfg(not(test))]
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::Path;
#[cfg(not(test))]
use std::path::PathBuf;
#[cfg(not(test))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(test))]
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_core::desktop_mcp::{
    decode_desk_response, desk_b64_decode, desk_lock_path, desk_socket_path, encode_desk_request,
    CapturedShot, DeskRequest, DeskResponse, DesktopBackend, KeyCombo, MonitorGeom, MouseButton,
    ShotGeom,
};
#[cfg(not(test))]
use grokhub_core::desktop_mcp::{
    apply_desk_request, peer_uid_allowed, probe_desktop, stamp_halts, CallGate,
};

use super::wayland::WaylandBackend;

const LINE_CAP: usize = 16 << 20;

pub(crate) struct SessionLock {
    file: File,
}

impl SessionLock {
    pub(crate) fn try_acquire(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("desktop lock: {err}"))?;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|err| format!("desktop lock: {err}"))?;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| "Another GrokHub process holds the desktop session.".to_string())?;
        Ok(Self { file })
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = rustix::fs::flock(&self.file, rustix::fs::FlockOperation::Unlock);
    }
}

#[cfg(not(test))]
struct Hub {
    backend: Mutex<WaylandBackend>,
    stop: AtomicBool,
}

#[cfg(not(test))]
static HUB: Mutex<Option<Arc<Hub>>> = Mutex::new(None);

#[cfg(not(test))]
pub(crate) fn note_halt() {
    if let Some(hub) = hub() {
        hub.backend.lock().unwrap_or_else(|err| err.into_inner()).release_input();
    }
}

#[cfg(not(test))]
pub(crate) fn set_enabled(on: bool) {
    if on {
        if let Err(err) = ensure_started() {
            eprintln!("desktop-mcp: {err}");
        }
    } else if let Some(hub) = hub() {
        hub.backend.lock().unwrap_or_else(|err| err.into_inner()).suspend_input();
    }
}

#[cfg(not(test))]
fn hub() -> Option<Arc<Hub>> {
    HUB.lock().unwrap_or_else(|err| err.into_inner()).clone()
}

#[cfg(not(test))]
pub(crate) fn ensure_started() -> Result<(), String> {
    if hub().is_some() {
        return Ok(());
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .map_err(|_| "XDG_RUNTIME_DIR is unset, so the desktop broker has no socket directory.".to_string())?;
    let lock_path = desk_lock_path(&runtime)?;
    let sock_path = desk_socket_path(&runtime)?;
    let lock = SessionLock::try_acquire(Path::new(&lock_path))?;
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("desktop socket: {err}"))?;
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }
    if sock_path.exists() {
        let _ = std::fs::remove_file(&sock_path);
    }
    let listener = UnixListener::bind(&sock_path).map_err(|err| format!("desktop socket: {err}"))?;
    let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600));
    listener.set_nonblocking(true).map_err(|err| format!("desktop socket: {err}"))?;
    let hub = Arc::new(Hub {
        backend: Mutex::new(WaylandBackend::new()),
        stop: AtomicBool::new(false),
    });
    *HUB.lock().unwrap_or_else(|err| err.into_inner()) = Some(Arc::clone(&hub));
    std::thread::Builder::new()
        .name("grokhub-desk".into())
        .spawn(move || serve(listener, sock_path, hub, lock))
        .map_err(|err| format!("desktop broker: {err}"))?;
    Ok(())
}

#[cfg(not(test))]
fn serve(listener: UnixListener, sock_path: PathBuf, hub: Arc<Hub>, lock: SessionLock) {
    while !hub.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => handle_client(stream, &hub),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(40)),
        }
    }
    let _ = std::fs::remove_file(sock_path);
    drop(lock);
}

#[cfg(not(test))]
fn handle_client(stream: UnixStream, hub: &Hub) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let cred = rustix::net::sockopt::socket_peercred(&stream);
    let allowed = match cred {
        Ok(cred) => peer_uid_allowed(cred.uid.as_raw(), rustix::process::getuid().as_raw()),
        Err(_) => false,
    };
    let mut write = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    loop {
        if hub.stop.load(Ordering::SeqCst) {
            break;
        }
        let line = match read_line(&mut reader) {
            Ok(None) => break,
            Ok(Some(line)) => line,
            Err(err) => {
                let _ = write_response(&mut write, &DeskResponse::fail(0, err, false));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        if !allowed {
            let id = decode_id(&line);
            let _ = write_response(
                &mut write,
                &DeskResponse::fail(id, "The desktop broker refused a different user.", false),
            );
            break;
        }
        let req = match grokhub_core::desktop_mcp::decode_desk_request(&line) {
            Ok(req) => req,
            Err(err) => {
                let _ = write_response(&mut write, &DeskResponse::fail(0, err, false));
                continue;
            }
        };
        let gate = current_gate();
        let mut backend = hub.backend.lock().unwrap_or_else(|err| err.into_inner());
        let resp = apply_desk_request(&mut *backend, gate, &req);
        drop(backend);
        if write_response(&mut write, &resp).is_err() {
            break;
        }
    }
}

#[cfg(not(test))]
fn current_gate() -> CallGate {
    let started = super::process_started_ms();
    CallGate {
        enabled: crate::config::load().desktop_control,
        halted: super::read_halt_stamp().is_some_and(|ms| stamp_halts(ms, started)),
    }
}

#[cfg(not(test))]
fn decode_id(line: &str) -> u64 {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|value| value.get("id").and_then(|id| id.as_u64()))
        .unwrap_or(0)
}

#[cfg(not(test))]
fn read_line(reader: &mut impl BufRead) -> Result<Option<String>, String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = reader.read(&mut byte).map_err(|err| format!("broker read: {err}"))?;
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
        if buf.len() >= LINE_CAP {
            return Err("broker line is too long".into());
        }
        buf.push(byte[0]);
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

#[cfg(not(test))]
fn write_response(stream: &mut UnixStream, resp: &DeskResponse) -> Result<(), String> {
    let line = grokhub_core::desktop_mcp::encode_desk_response(resp)?;
    stream.write_all(line.as_bytes()).map_err(|err| format!("broker write: {err}"))?;
    stream.write_all(b"\n").map_err(|err| format!("broker write: {err}"))?;
    stream.flush().map_err(|err| format!("broker write: {err}"))?;
    Ok(())
}

#[cfg(not(test))]
pub(crate) fn run_probe() -> Result<String, String> {
    let hub = hub().ok_or_else(|| "Desktop control is not running.".to_string())?;
    let mut backend = hub.backend.lock().unwrap_or_else(|err| err.into_inner());
    probe_desktop(&mut *backend, current_gate())
}

/// MCP client. It does not send a gate; the broker reads the switch, halt, and lock.
pub(crate) struct BrokerClient {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl BrokerClient {
    pub(crate) fn connect(path: &Path) -> Result<Self, String> {
        let stream = UnixStream::connect(path).map_err(|err| format!("desktop broker: {err}"))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let reader = BufReader::new(stream.try_clone().map_err(|err| format!("desktop broker: {err}"))?);
        Ok(Self { stream, reader, next_id: 1 })
    }

    pub(crate) fn connect_runtime() -> Result<Self, String> {
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
        let path = desk_socket_path(&runtime)?;
        Self::connect(Path::new(&path))
    }

    fn call(&mut self, build: impl FnOnce(u64) -> DeskRequest) -> Result<DeskResponse, String> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let line = encode_desk_request(&build(id))?;
        self.stream.write_all(line.as_bytes()).map_err(|err| format!("desktop broker: {err}"))?;
        self.stream.write_all(b"\n").map_err(|err| format!("desktop broker: {err}"))?;
        self.stream.flush().map_err(|err| format!("desktop broker: {err}"))?;
        let mut buf = String::new();
        self.reader.read_line(&mut buf).map_err(|err| format!("desktop broker: {err}"))?;
        if buf.is_empty() {
            return Err("The desktop broker closed the socket.".into());
        }
        let resp = decode_desk_response(&buf)?;
        if resp.ok {
            Ok(resp)
        } else {
            Err(resp.error.unwrap_or_else(|| "The desktop broker refused the request.".into()))
        }
    }
}

impl DesktopBackend for BrokerClient {
    fn list_monitors(&mut self) -> Result<Vec<MonitorGeom>, String> {
        Ok(self.call(|id| DeskRequest::ListMonitors { id })?.monitors.unwrap_or_default())
    }

    fn screenshot(&mut self, monitor: &str) -> Result<CapturedShot, String> {
        let resp = self.call(|id| DeskRequest::Screenshot {
            id,
            monitor: monitor.to_string(),
        })?;
        let bytes = desk_b64_decode(resp.image_b64.as_deref().unwrap_or(""))?;
        Ok(CapturedShot {
            bytes,
            mime: resp.mime.unwrap_or_else(|| "image/png".into()),
            geom: resp.geom.ok_or_else(|| "The broker screenshot has no geometry.".to_string())?,
        })
    }

    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.call(|id| DeskRequest::MoveAbs { id, x, y })?;
        Ok(())
    }

    fn move_abs_on(&mut self, geom: &ShotGeom, x: i32, y: i32) -> Result<(), String> {
        self.call(|id| DeskRequest::MoveOn {
            id,
            geom: geom.clone(),
            x,
            y,
        })?;
        Ok(())
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
        self.call(|id| DeskRequest::Button { id, button, down })?;
        Ok(())
    }

    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        self.call(|id| DeskRequest::Scroll { id, dx, dy })?;
        Ok(())
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        let text = text.to_string();
        self.call(|id| DeskRequest::TypeText { id, text })?;
        Ok(())
    }

    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
        let keys = format_combo(combo);
        self.call(|id| DeskRequest::Key { id, keys })?;
        Ok(())
    }

    fn is_locked(&mut self) -> bool {
        self.call(|id| DeskRequest::Locked { id })
            .ok()
            .and_then(|resp| resp.locked)
            .unwrap_or(false)
    }

    fn pace(&mut self) {
        std::thread::sleep(Duration::from_millis(15));
    }

    fn release_input(&mut self) {
        let _ = self.call(|id| DeskRequest::Release { id });
    }

    fn suspend_input(&mut self) {
        self.release_input();
    }

    fn status_note(&mut self) -> Option<String> {
        self.call(|id| DeskRequest::Status { id }).ok().and_then(|resp| resp.note)
    }
}

fn format_combo(combo: &KeyCombo) -> String {
    let mut parts = Vec::new();
    if combo.ctrl {
        parts.push("ctrl".to_string());
    }
    if combo.alt {
        parts.push("alt".into());
    }
    if combo.shift {
        parts.push("shift".into());
    }
    if combo.super_key {
        parts.push("super".into());
    }
    parts.push(key_token(&combo.key));
    parts.join("+")
}

fn key_token(key: &grokhub_core::desktop_mcp::KeyName) -> String {
    use grokhub_core::desktop_mcp::KeyName;
    match key {
        KeyName::Return => "Return".into(),
        KeyName::Escape => "Escape".into(),
        KeyName::Backspace => "Backspace".into(),
        KeyName::Tab => "Tab".into(),
        KeyName::Space => "space".into(),
        KeyName::Delete => "Delete".into(),
        KeyName::Insert => "Insert".into(),
        KeyName::Home => "Home".into(),
        KeyName::End => "End".into(),
        KeyName::PageUp => "PageUp".into(),
        KeyName::PageDown => "PageDown".into(),
        KeyName::Left => "Left".into(),
        KeyName::Up => "Up".into(),
        KeyName::Right => "Right".into(),
        KeyName::Down => "Down".into(),
        KeyName::Super => "Super".into(),
        KeyName::F(n) => format!("F{n}"),
        KeyName::Char(ch) => ch.to_string(),
    }
}

pub(crate) fn connect_or_own() -> Result<Box<dyn DesktopBackend>, String> {
    if let Ok(client) = BrokerClient::connect_runtime() {
        return Ok(Box::new(client));
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    let lock_path = desk_lock_path(&runtime)?;
    match SessionLock::try_acquire(Path::new(&lock_path)) {
        Ok(lock) => {
            if let Ok(client) = BrokerClient::connect_runtime() {
                drop(lock);
                return Ok(Box::new(client));
            }
            Ok(Box::new(OwnedSession {
                backend: WaylandBackend::new(),
                _lock: lock,
            }))
        }
        Err(err) => {
            for _ in 0..20 {
                if let Ok(client) = BrokerClient::connect_runtime() {
                    return Ok(Box::new(client));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err)
        }
    }
}

struct OwnedSession {
    backend: WaylandBackend,
    _lock: SessionLock,
}

impl DesktopBackend for OwnedSession {
    fn list_monitors(&mut self) -> Result<Vec<MonitorGeom>, String> {
        self.backend.list_monitors()
    }
    fn screenshot(&mut self, monitor: &str) -> Result<CapturedShot, String> {
        self.backend.screenshot(monitor)
    }
    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.backend.move_abs(x, y)
    }
    fn move_abs_on(&mut self, geom: &ShotGeom, x: i32, y: i32) -> Result<(), String> {
        self.backend.move_abs_on(geom, x, y)
    }
    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
        self.backend.button(button, down)
    }
    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        self.backend.scroll(dx, dy)
    }
    fn type_text(&mut self, text: &str) -> Result<(), String> {
        self.backend.type_text(text)
    }
    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
        self.backend.key_combo(combo)
    }
    fn is_locked(&mut self) -> bool {
        self.backend.is_locked()
    }
    fn pace(&mut self) {
        self.backend.pace();
    }
    fn release_input(&mut self) {
        self.backend.release_input();
    }
    fn suspend_input(&mut self) {
        self.backend.suspend_input();
    }
    fn status_note(&mut self) -> Option<String> {
        self.backend.status_note()
    }
    fn landed_at(&mut self) -> Option<(i32, i32)> {
        self.backend.landed_at()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_lock_prevents_double_sessions() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-desk-lock-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("grokhub").join("desk.lock");
        let first = SessionLock::try_acquire(&path).unwrap();
        let second = SessionLock::try_acquire(&path);
        assert!(second.is_err(), "a second holder must be refused");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "lock file mode {mode:o}");
        let dir_mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "runtime dir mode {dir_mode:o}");
        drop(first);
        assert!(path.exists(), "dropping the lock must not delete the file");
        let third = SessionLock::try_acquire(&path).unwrap();
        drop(third);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
