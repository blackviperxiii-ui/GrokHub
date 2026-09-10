//! Windows-native cabin chrome: real hide, toasts, sleep inhibit, DWM polish.

#![cfg(windows)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
};
use windows_sys::Win32::UI::Shell::{
    SetCurrentProcessExplicitAppUserModelID, ShellExecuteW,
};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITOR_DEFAULTTONEAREST, MONITORINFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SetForegroundWindow, SetWindowPos, ShowWindow, SystemParametersInfoW, HWND_TOP, SWP_SHOWWINDOW,
    SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL, SPI_GETWORKAREA,
};

const APP_USER_MODEL_ID: &str = "GrokHub.Cabin";
static KEEP_AWAKE: AtomicBool = AtomicBool::new(false);

// DWM attribute ids (windows-sys may omit the Win11 names on older feature sets).
const DWMWA_CLOAK: u32 = 13;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
/// DWMSBT_MAINWINDOW — Mica-like backdrop when the compositor supports it.
const DWMSBT_MAINWINDOW: u32 = 2;
/// DWMWCP_ROUND — Windows 11 rounded corners on an undecorated cabin.
const DWMWCP_ROUND: u32 = 2;

struct FindState {
    pid: u32,
    hwnd: HWND,
}

struct StubState {
    pid: u32,
    cabin: HWND,
}

unsafe extern "system" fn enum_cabin(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam as *mut FindState);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != state.pid {
        return TRUE;
    }
    let mut buf = [0u16; 64];
    let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if n <= 0 {
        return TRUE;
    }
    let title = String::from_utf16_lossy(&buf[..n as usize]);
    if title == "GrokHub" {
        state.hwnd = hwnd;
        return 0; // stop
    }
    TRUE
}

unsafe extern "system" fn enum_hide_stubs(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &*(lparam as *const StubState);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != state.pid || hwnd == state.cabin {
        return TRUE;
    }
    if IsWindowVisible(hwnd) == 0 {
        return TRUE;
    }
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if GetWindowRect(hwnd, &mut rc) == 0 {
        return TRUE;
    }
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    // egui Visible(false) spawns a ~14×14 untitled ghost; hide anything tiny.
    if w > 0 && h > 0 && (w < 200 || h < 200) {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    TRUE
}

struct LargestState {
    pid: u32,
    hwnd: HWND,
    best_area: i32,
}

fn cabin_hwnd() -> Option<HWND> {
    let pid = std::process::id();
    let mut state = FindState {
        pid,
        hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(enum_cabin), &mut state as *mut _ as LPARAM);
    }
    if !state.hwnd.is_null() {
        return Some(state.hwnd);
    }
    // Title may lag a frame after create — fall back to the largest top-level window.
    let mut largest = LargestState {
        pid,
        hwnd: std::ptr::null_mut(),
        best_area: 0,
    };
    unsafe {
        EnumWindows(Some(enum_largest), &mut largest as *mut _ as LPARAM);
    }
    if largest.hwnd.is_null() {
        None
    } else {
        Some(largest.hwnd)
    }
}

unsafe extern "system" fn enum_largest(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam as *mut LargestState);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != state.pid {
        return TRUE;
    }
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if GetWindowRect(hwnd, &mut rc) == 0 {
        return TRUE;
    }
    let area = (rc.right - rc.left).saturating_mul(rc.bottom - rc.top);
    if area > state.best_area && area >= 200 * 200 {
        state.best_area = area;
        state.hwnd = hwnd;
    }
    TRUE
}

/// Hide egui's leftover 14×14 ghosts so Alt-Tab / taskbar stay clean.
fn hide_stub_windows(cabin: Option<HWND>) {
    let state = StubState {
        pid: std::process::id(),
        cabin: cabin.unwrap_or(std::ptr::null_mut()),
    };
    unsafe {
        EnumWindows(Some(enum_hide_stubs), &state as *const _ as LPARAM);
    }
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Taskbar / toast identity so Explorer groups GrokHub as one app.
pub fn set_app_user_model_id() {
    let id = wide(APP_USER_MODEL_ID);
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
    }
}

/// Round corners + dark mode + Mica backdrop when DWM allows it.
pub fn polish_hwnd(dark: bool) {
    let Some(hwnd) = cabin_hwnd() else {
        return;
    };
    unsafe {
        let corner: u32 = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
        let dark_i: i32 = if dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_i as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        );
        let backdrop: u32 = DWMSBT_MAINWINDOW;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

/// Sweep egui 14×14 ghosts without touching the titled cabin visibility.
pub fn hide_cabin_stubs_only() {
    hide_stub_windows(cabin_hwnd());
}

fn set_cloaked(hwnd: HWND, cloak: bool) {
    let v: i32 = if cloak { 1 } else { 0 };
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK,
            &v as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// Hide without freezing winit: DWM cloak keeps the message pump alive.
/// `SW_HIDE` stops egui timers so cabin.raise / tray Show never run.
pub fn hide_cabin() -> bool {
    let hwnd = cabin_hwnd();
    let ok = if let Some(hwnd) = hwnd {
        set_cloaked(hwnd, true);
        true
    } else {
        false
    };
    hide_stub_windows(hwnd);
    ok
}

/// Monitor work area in physical pixels. Prefers the cabin's monitor; falls
/// back to the primary work area so maximize works before the HWND is titled.
pub fn work_area() -> Option<(i32, i32, i32, i32)> {
    if let Some(hwnd) = cabin_hwnd() {
        unsafe {
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            if !mon.is_null() {
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    rcMonitor: RECT {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    },
                    rcWork: RECT {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    },
                    dwFlags: 0,
                };
                if GetMonitorInfoW(mon, &mut info) != 0 {
                    let r = info.rcWork;
                    let w = r.right - r.left;
                    let h = r.bottom - r.top;
                    if w >= 640 && h >= 480 {
                        return Some((r.left, r.top, w, h));
                    }
                }
            }
        }
    }
    primary_work_area()
}

fn primary_work_area() -> Option<(i32, i32, i32, i32)> {
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        let ok = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut rc as *mut RECT as *mut _,
            0,
        );
        if ok == 0 {
            return None;
        }
    }
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    if w >= 640 && h >= 480 {
        Some((rc.left, rc.top, w, h))
    } else {
        None
    }
}

fn maximize_undecorated(hwnd: HWND) {
    let Some((x, y, w, h)) = work_area() else {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        return;
    };
    unsafe {
        let _ = SetWindowPos(hwnd, HWND_TOP, x, y, w, h, SWP_SHOWWINDOW);
    }
}

/// Show + restore size. Call after egui Visible(true) / apply_saved_geom.
pub fn show_cabin(x: i32, y: i32, w: i32, h: i32, maximized: bool) -> bool {
    let Some(hwnd) = cabin_hwnd() else {
        return false;
    };
    set_cloaked(hwnd, false);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        if maximized {
            // Undecorated egui windows ignore SW_MAXIMIZE — fill the monitor work area.
            maximize_undecorated(hwnd);
        } else {
            if IsIconic(hwnd) != 0 {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let ww = w.max(720);
            let hh = h.max(480);
            let _ = SetWindowPos(hwnd, HWND_TOP, x, y, ww, hh, SWP_SHOWWINDOW);
        }
        let _ = SetForegroundWindow(hwnd);
    }
    hide_stub_windows(Some(hwnd));
    true
}

/// Re-apply size if the compositor left a stub after Visible(true).
pub fn repair_stub_if_needed(x: i32, y: i32, w: i32, h: i32, maximized: bool) -> bool {
    let Some(hwnd) = cabin_hwnd() else {
        return false;
    };
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        if GetWindowRect(hwnd, &mut rc) == 0 {
            return false;
        }
        let cw = rc.right - rc.left;
        let ch = rc.bottom - rc.top;
        let visible = IsWindowVisible(hwnd) != 0;
        if !visible || cw < 200 || ch < 200 {
            return show_cabin(x, y, w, h, maximized);
        }
    }
    false
}

pub fn keep_awake(on: bool) {
    KEEP_AWAKE.store(on, Ordering::SeqCst);
    unsafe {
        if on {
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        } else {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

/// Windows toast via WinRT (no notify-send). Best-effort; failures are silent.
pub fn toast(title: &str, body: &str) {
    let title = sanitize_ps(title);
    let body = sanitize_ps(body);
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
[Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
$xml = New-Object Windows.Data.Xml.Dom.XmlDocument
$xml.LoadXml(@'
<toast><visual><binding template="ToastGeneric"><text>{title}</text><text>{body}</text></binding></visual></toast>
'@)
$toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{APP_USER_MODEL_ID}').Show($toast)
"#
    );
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-NoLogo",
        "-WindowStyle",
        "Hidden",
        "-Command",
        &script,
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());
    grokhub_acp::hide_windows_console(&mut cmd);
    let _ = cmd.spawn();
}

/// Open a file, folder, or http(s) URL with the Windows shell.
pub fn open_path(path: &str) -> Result<(), String> {
    let verb = wide("open");
    let target = wide(path);
    let rc = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL as i32,
        )
    };
    if (rc as usize) <= 32 {
        return Err(format!("ShellExecute failed ({})", rc as usize));
    }
    Ok(())
}

fn file_dialog(save: bool, suggested: &str) -> Option<std::path::PathBuf> {
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, GetSaveFileNameW, OPENFILENAMEW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
        OFN_HIDEREADONLY, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST,
    };
    let mut buf = [0u16; 1024];
    if !suggested.is_empty() {
        let w = wide(suggested);
        let n = w.len().saturating_sub(1).min(buf.len().saturating_sub(1));
        buf[..n].copy_from_slice(&w[..n]);
    }
    let filter = wide("All files\0*.*\0\0");
    let title = wide(if save { "Save" } else { "Open" });
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: cabin_hwnd().unwrap_or(ptr::null_mut()),
        hInstance: ptr::null_mut(),
        lpstrFilter: filter.as_ptr(),
        lpstrCustomFilter: ptr::null_mut(),
        nMaxCustFilter: 0,
        nFilterIndex: 1,
        lpstrFile: buf.as_mut_ptr(),
        nMaxFile: buf.len() as u32,
        lpstrFileTitle: ptr::null_mut(),
        nMaxFileTitle: 0,
        lpstrInitialDir: ptr::null(),
        lpstrTitle: title.as_ptr(),
        Flags: OFN_EXPLORER
            | OFN_HIDEREADONLY
            | OFN_PATHMUSTEXIST
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            },
        nFileOffset: 0,
        nFileExtension: 0,
        lpstrDefExt: ptr::null(),
        lCustData: 0,
        lpfnHook: None,
        lpTemplateName: ptr::null(),
        pvReserved: ptr::null_mut(),
        dwReserved: 0,
        FlagsEx: 0,
    };
    let ok = unsafe {
        if save {
            GetSaveFileNameW(&mut ofn)
        } else {
            GetOpenFileNameW(&mut ofn)
        }
    };
    if ok == 0 {
        return None;
    }
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    let path = String::from_utf16_lossy(&buf[..end]);
    if path.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(path))
    }
}

pub fn open_file_dialog() -> Option<std::path::PathBuf> {
    file_dialog(false, "")
}

pub fn save_file_dialog(suggested: &str) -> Option<std::path::PathBuf> {
    file_dialog(true, suggested)
}

fn sanitize_ps(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\'' | '"' | '`' | '$' | '\n' | '\r' | '<' | '>' | '&' => ' ',
            _ => c,
        })
        .take(180)
        .collect()
}

/// Registry AppsUseLightTheme: 0 = dark, 1 = light. None if unset/unreadable.
pub fn apps_use_light_theme() -> Option<bool> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    };
    let sub = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("AppsUseLightTheme");
    unsafe {
        let mut key = std::ptr::null_mut();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_READ,
            &mut key,
        ) != 0
        {
            return None;
        }
        let mut ty = 0u32;
        let mut data = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        let status = RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            &mut data as *mut u32 as *mut u8,
            &mut len,
        );
        let _ = RegCloseKey(key);
        if status != 0 || ty != REG_DWORD {
            return None;
        }
        Some(data != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_sanitizes_injection_chars() {
        let s = sanitize_ps("hi'; $(calc) <b>");
        assert!(!s.contains('\''));
        assert!(!s.contains('$'));
        assert!(!s.contains('<'));
        let src = include_str!("win_native.rs");
        let toast = src
            .split("pub fn toast(")
            .nth(1)
            .and_then(|s| s.split("pub fn open_path(").next())
            .expect("toast");
        assert!(
            toast.contains("hide_windows_console"),
            "WinRT toast PowerShell must not flash a console: {toast}"
        );
    }

    #[test]
    fn app_user_model_id_is_stable() {
        assert_eq!(APP_USER_MODEL_ID, "GrokHub.Cabin");
    }
}
