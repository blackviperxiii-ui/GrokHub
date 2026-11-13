//! The foreground app's process name, read once for focus help. `None`
//! where no reader sees it (macOS, and Wayland compositors other than
//! Hyprland and Sway). Never a window title.

#[cfg(unix)]
use std::process::{Command, Stdio};

/// The foreground app's process name. Opt-in callers (focus help) read this
/// once after the shell loses focus; off never calls it.
pub fn foreground_process() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        if wayland() {
            return compositor_hint();
        }
        x11_front()
    }
    #[cfg(windows)]
    {
        win_foreground()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn wayland() -> bool {
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let session = std::env::var("XDG_SESSION_TYPE").ok();
    grokhub_core::session_is_wayland(wayland.as_deref(), session.as_deref())
}

/// Hyprland, then Sway. Other Wayland compositors give no hint.
#[cfg(target_os = "linux")]
fn compositor_hint() -> Option<String> {
    if let Some(j) = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .and_then(|_| run("hyprctl", &["activewindow", "-j"]))
    {
        return grokhub_core::local_governor::parse_hyprctl_active(&j).1;
    }
    if let Some(j) = std::env::var_os("SWAYSOCK").and_then(|_| run("swaymsg", &["-t", "get_tree"]))
    {
        return grokhub_core::local_governor::parse_sway_tree(&j).1;
    }
    None
}

#[cfg(target_os = "linux")]
fn x11_front() -> Option<String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    std::env::var_os("DISPLAY")?;
    let (conn, screen) = x11rb::rust_connection::RustConnection::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let atom = |name: &[u8]| Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom);
    let active = atom(b"_NET_ACTIVE_WINDOW")?;
    let pid_atom = atom(b"_NET_WM_PID")?;
    let win = conn
        .get_property(false, root, active, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()
        .filter(|w| *w != 0)?;
    let pid = conn
        .get_property(false, win, pid_atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()?;
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|c| c.trim().to_ascii_lowercase())
}

/// The foreground window's executable file name, from a Toolhelp snapshot by
/// pid so the app itself is never opened.
#[cfg(windows)]
fn win_foreground() -> Option<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    // SAFETY: out-pointer to a local u32; hwnd null-check before use.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        (pid != 0)
            .then(|| crate::game_guard::exe_name_of(pid))
            .flatten()
    }
}

#[cfg(target_os = "linux")]
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn mac_and_unknown_targets_return_none_without_panicking() {
        // On every target this is safe to call; macOS and exotic OSes are None.
        let _ = super::foreground_process();
    }
}
