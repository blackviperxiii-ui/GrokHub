//! KWin `org.kde.KWin.ScreenShot2` capture. The pipe is an input fd: KWin writes
//! ARGB32-premultiplied pixels while the call is in flight, so the reader runs
//! alongside the D-Bus method.

use std::collections::HashMap;
use std::io::Read;
use std::sync::mpsc;
use std::time::Duration;

use grokhub_core::desktop_mcp::{argb32_premul_to_rgba, parse_screenshot2_metadata, ScreenShot2Meta};
use zbus::zvariant::{Fd, OwnedValue, Value};

const PATH: &str = "/org/kde/KWin/ScreenShot2";
const DESTINATIONS: &[&str] = &["org.kde.KWin", "org.kde.KWin.ScreenShot2"];
const PIXEL_CAP: usize = 80 * 1024 * 1024;

#[zbus::proxy(
    interface = "org.kde.KWin.ScreenShot2",
    default_service = "org.kde.KWin",
    default_path = "/org/kde/KWin/ScreenShot2"
)]
trait ScreenShot2 {
    fn capture_screen(
        &self,
        name: &str,
        options: HashMap<&str, Value<'_>>,
        pipe: Fd<'_>,
    ) -> zbus::Result<HashMap<String, OwnedValue>>;

    fn capture_workspace(
        &self,
        options: HashMap<&str, Value<'_>>,
        pipe: Fd<'_>,
    ) -> zbus::Result<HashMap<String, OwnedValue>>;
}

/// `org.kde.KWin` owns the screenshot interface. Skip the probe when there is
/// no session bus so CI never tries to start one.
pub(crate) fn kwin_running() -> bool {
    if bus_address().is_none() {
        return false;
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let found = (|| {
            let conn = zbus::blocking::Connection::session().ok()?;
            name_is_owned(&conn, "org.kde.KWin")
        })();
        let _ = tx.send(found.unwrap_or(false));
    });
    rx.recv_timeout(Duration::from_millis(400)).unwrap_or(false)
}

/// `screen` is one output name. `None` captures the whole workspace.
pub(crate) fn capture(screen: Option<&str>) -> Result<(Vec<u8>, ScreenShot2Meta), String> {
    let conn = session_bus()?;
    let proxy = screenshot_proxy(&conn)?;
    let options = shot_options();
    let (reader, writer) = std::io::pipe().map_err(|err| format!("screenshot pipe: {err}"))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read_pipe(reader));
    });
    let meta = if let Some(name) = screen {
        proxy.capture_screen(name, options, Fd::from(&writer))
    } else {
        proxy.capture_workspace(options, Fd::from(&writer))
    };
    drop(writer);
    let pixels = match rx.recv_timeout(Duration::from_secs(8)) {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) => return Err(err),
        Err(_) => return Err("ScreenShot2 pipe timed out.".into()),
    };
    let meta = meta.map_err(|err| format!("ScreenShot2: {err}"))?;
    let value = dbus_meta(&meta);
    let parsed = parse_screenshot2_metadata(&value)?;
    let rgba = argb32_premul_to_rgba(&pixels, parsed.width, parsed.height, parsed.stride)?;
    Ok((rgba, parsed))
}

fn bus_address() -> Option<String> {
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok()?;
    if addr.trim().is_empty() {
        None
    } else {
        Some(addr)
    }
}

fn session_bus() -> Result<zbus::blocking::Connection, String> {
    if bus_address().is_none() {
        return Err("No D-Bus session bus.".into());
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(
            zbus::blocking::Connection::session().map_err(|err| format!("D-Bus session bus: {err}")),
        );
    });
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(_) => Err("D-Bus session bus timed out.".into()),
    }
}

fn screenshot_proxy(
    conn: &zbus::blocking::Connection,
) -> Result<ScreenShot2ProxyBlocking<'_>, String> {
    for dest in DESTINATIONS {
        if name_is_owned(conn, dest) != Some(true) {
            continue;
        }
        return ScreenShot2ProxyBlocking::builder(conn)
            .destination(*dest)
            .map_err(|err| format!("ScreenShot2: {err}"))?
            .path(PATH)
            .map_err(|err| format!("ScreenShot2: {err}"))?
            .build()
            .map_err(|err| format!("ScreenShot2: {err}"));
    }
    Err("KWin ScreenShot2 is not on the session bus.".into())
}

fn name_is_owned(conn: &zbus::blocking::Connection, dest: &str) -> Option<bool> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(conn).ok()?;
    let name: zbus::names::BusName<'_> = dest.try_into().ok()?;
    dbus.name_has_owner(name).ok()
}

fn shot_options() -> HashMap<&'static str, Value<'static>> {
    let mut options = HashMap::new();
    options.insert("native-resolution", Value::Bool(true));
    options
}

fn read_pipe(mut reader: std::io::PipeReader) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|err| format!("ScreenShot2 pipe: {err}"))?;
        if n == 0 {
            return Ok(buf);
        }
        if buf.len().saturating_add(n) > PIXEL_CAP {
            return Err("ScreenShot2 image is larger than 80 MiB.".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn dbus_meta(map: &HashMap<String, OwnedValue>) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    for (key, value) in map {
        if let Some(json) = dbus_json(value) {
            obj.insert(key.clone(), json);
        }
    }
    serde_json::Value::Object(obj)
}

fn dbus_json(value: &OwnedValue) -> Option<serde_json::Value> {
    let flat = match &**value {
        Value::Value(inner) => inner.as_ref(),
        other => other,
    };
    match flat {
        Value::Bool(v) => Some(serde_json::Value::Bool(*v)),
        Value::U8(v) => Some(serde_json::json!(*v)),
        Value::U16(v) => Some(serde_json::json!(*v)),
        Value::U32(v) => Some(serde_json::json!(*v)),
        Value::U64(v) => Some(serde_json::json!(*v)),
        Value::I16(v) => Some(serde_json::json!(*v)),
        Value::I32(v) => Some(serde_json::json!(*v)),
        Value::I64(v) => Some(serde_json::json!(*v)),
        Value::F64(v) => Some(serde_json::json!(*v)),
        Value::Str(v) => Some(serde_json::Value::String(v.to_string())),
        _ => None,
    }
}
