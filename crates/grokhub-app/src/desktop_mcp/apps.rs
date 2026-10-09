//! Hermes #1 and #5 OS routes for path A: open an app, focus a window, list
//! windows, and the trash / Recycle Bin (Spike-1b).
//!
//! Linux: `gtk-launch` (then `gio launch`) for a desktop id, EWMH over the
//! existing X11 connection for focus and the window list, and a one-shot KWin
//! script on KDE Wayland. Windows: `ShellExecuteW` plus `SetForegroundWindow`,
//! and `SHFileOperationW` with undo for the Recycle Bin. No UIA (D3).
//! Window geometry (card 12): X11 `GetGeometry` and `_NET_MOVERESIZE_WINDOW`,
//! a one-shot KWin script to set it on KDE Wayland (reading needs X11), and
//! `GetWindowRect` / `SetWindowPos` on Windows.
//! The harness gate runs before any of these (`harness_gate::precheck`).

#[cfg(not(any(target_os = "linux", windows)))]
use std::path::PathBuf;

use grokhub_core::desktop_mcp::DesktopWindows;
#[cfg(not(any(target_os = "linux", windows)))]
use grokhub_core::desktop_mcp::{WindowGeom, APPS_MSG, GEOMETRY_MSG, TRASH_MSG};

/// First window (in list order) whose title holds `want`, any case.
pub(crate) fn pick_title<'a>(titles: impl IntoIterator<Item = &'a str>, want: &str) -> Option<&'a str> {
    let want = want.trim().to_lowercase();
    if want.is_empty() {
        return None;
    }
    titles.into_iter().find(|t| t.to_lowercase().contains(&want))
}

/// Did `open_app` or `focus_window` change the desktop? A new window, a gone
/// one, or a different focused window counts.
pub(crate) fn windows_changed(before: &DesktopWindows, after: &DesktopWindows) -> bool {
    let mut a = before.titles.clone();
    let mut b = after.titles.clone();
    a.sort();
    b.sort();
    a != b || before.active != after.active
}

#[cfg(target_os = "linux")]
pub(crate) use linux::{focus_window, list_windows, open_app, set_window_geometry, trash, window_geometry};
#[cfg(windows)]
pub(crate) use win::{focus_window, list_windows, no_recycle_bin, open_app, set_window_geometry, trash, window_geometry};

/// Outside Windows a trash move with no trash fails instead of deleting.
#[cfg(not(windows))]
pub(crate) fn no_recycle_bin(_path: &std::path::Path) -> bool {
    false
}

#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) fn open_app(_app: &str) -> Result<(), String> {
    Err(APPS_MSG.into())
}
#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) fn focus_window(_title: &str) -> Result<String, String> {
    Err(APPS_MSG.into())
}
#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) fn list_windows() -> Result<DesktopWindows, String> {
    Err(APPS_MSG.into())
}
#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) fn trash(_paths: &[PathBuf]) -> Result<(), String> {
    Err(TRASH_MSG.into())
}
#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) fn window_geometry(_title: &str) -> Result<WindowGeom, String> {
    Err(GEOMETRY_MSG.into())
}
#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) fn set_window_geometry(_title: &str, _geom: &WindowGeom) -> Result<String, String> {
    Err(GEOMETRY_MSG.into())
}

/// The KWin script body that moves and resizes the first window whose
/// caption holds `title` (any case).
#[cfg(target_os = "linux")]
pub(crate) fn kwin_geometry_script(title: &str, g: &grokhub_core::desktop_mcp::WindowGeom) -> Result<String, String> {
    let want = serde_json::to_string(&title.to_lowercase()).map_err(|e| e.to_string())?;
    Ok(format!(
        "const want = {want};\n\
         const list = workspace.windowList ? workspace.windowList() : workspace.clientList();\n\
         for (const w of list) {{\n\
           if ((w.caption || \"\").toLowerCase().includes(want)) {{\n\
             w.frameGeometry = {{ x: {}, y: {}, width: {}, height: {} }};\n\
             break;\n\
           }}\n\
         }}\n",
        g.x, g.y, g.width, g.height
    ))
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use grokhub_core::desktop_mcp::{DesktopWindows, WindowGeom, APPS_MSG, GEOMETRY_MSG, TRASH_MSG};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window};
    use x11rb::rust_connection::RustConnection;

    const LAUNCH_WAIT: Duration = Duration::from_secs(5);

    fn x11_session() -> bool {
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        let session = std::env::var("XDG_SESSION_TYPE").ok();
        !grokhub_core::session_is_wayland(wayland.as_deref(), session.as_deref()) && std::env::var_os("DISPLAY").is_some()
    }

    fn kde_wayland() -> bool {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
        !x11_session() && grokhub_core::desktop_mcp::current_desktop_is_kde(desktop.as_deref())
    }

    /// Run one launcher with no shell. `Ok(None)` when the program is missing.
    fn run(program: &str, args: &[&str]) -> Result<Option<()>, String> {
        let mut child = match Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{program}: {e}")),
        };
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(Some(())),
                Ok(Some(_)) => {
                    let mut err = String::new();
                    if let Some(mut pipe) = child.stderr.take() {
                        let _ = std::io::Read::read_to_string(&mut pipe, &mut err);
                    }
                    let line = err.lines().next().unwrap_or("failed").trim().to_string();
                    return Err(format!("{program}: {line}"));
                }
                Ok(None) if start.elapsed() < LAUNCH_WAIT => std::thread::sleep(Duration::from_millis(50)),
                // Still running: the launcher handed off and is waiting on the app.
                Ok(None) => return Ok(Some(())),
                Err(e) => return Err(format!("{program}: {e}")),
            }
        }
    }

    /// `{data dir}/applications/{id}.desktop` for each XDG data dir.
    pub(super) fn desktop_file(id: &str) -> Option<PathBuf> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".local").join("share")));
        let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        data_home
            .into_iter()
            .chain(dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from))
            .map(|d| d.join("applications").join(format!("{id}.desktop")))
            .find(|p| p.is_file())
    }

    pub(crate) fn open_app(id: &str) -> Result<(), String> {
        if run("gtk-launch", &[id])?.is_some() {
            return Ok(());
        }
        let file = desktop_file(id).ok_or_else(|| format!("No app with the desktop id {id}."))?;
        let path = file.to_string_lossy().into_owned();
        match run("gio", &["launch", &path])? {
            Some(()) => Ok(()),
            None => Err("Install gtk-launch or gio to open apps.".into()),
        }
    }

    struct X11Windows {
        conn: RustConnection,
        root: Window,
        net_active: u32,
        rows: Vec<(Window, String, String)>,
        active: Window,
    }

    fn atom(conn: &RustConnection, name: &str) -> Result<u32, String> {
        Ok(conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| format!("X11: {e}"))?
            .reply()
            .map_err(|e| format!("X11: {e}"))?
            .atom)
    }

    fn prop_bytes(conn: &RustConnection, win: Window, prop: u32, ty: u32) -> Vec<u8> {
        conn.get_property(false, win, prop, ty, 0, 4096)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.value)
            .unwrap_or_default()
    }

    fn prop_windows(conn: &RustConnection, win: Window, prop: u32) -> Vec<Window> {
        conn.get_property(false, win, prop, AtomEnum::WINDOW, 0, 4096)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    fn x11_windows() -> Result<X11Windows, String> {
        let (conn, screen) = RustConnection::connect(None).map_err(|e| format!("X11: {e}"))?;
        let root = conn.setup().roots[screen].root;
        let client_list = atom(&conn, "_NET_CLIENT_LIST")?;
        let net_active = atom(&conn, "_NET_ACTIVE_WINDOW")?;
        let net_name = atom(&conn, "_NET_WM_NAME")?;
        let utf8 = atom(&conn, "UTF8_STRING")?;
        let mut rows = Vec::new();
        for w in prop_windows(&conn, root, client_list) {
            let mut title = String::from_utf8_lossy(&prop_bytes(&conn, w, net_name, utf8)).into_owned();
            if title.is_empty() {
                title = String::from_utf8_lossy(&prop_bytes(&conn, w, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into())).into_owned();
            }
            let class = prop_bytes(&conn, w, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into());
            let class = String::from_utf8_lossy(&class).split('\0').filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
            rows.push((w, title, class));
        }
        let active = prop_windows(&conn, root, net_active).first().copied().unwrap_or(0);
        Ok(X11Windows { conn, root, net_active, rows, active })
    }

    fn as_list(x: &X11Windows) -> DesktopWindows {
        let active = x
            .rows
            .iter()
            .find(|(w, _, _)| *w == x.active)
            .map(|(_, t, c)| format!("{c} {t}").trim().to_string())
            .unwrap_or_default();
        DesktopWindows { titles: x.rows.iter().map(|(_, t, _)| t.clone()).collect(), active }
    }

    pub(crate) fn list_windows() -> Result<DesktopWindows, String> {
        if !x11_session() {
            return Err(APPS_MSG.into());
        }
        x11_windows().map(|x| as_list(&x))
    }

    pub(crate) fn focus_window(title: &str) -> Result<String, String> {
        if kde_wayland() {
            return kwin_focus(title).map(|()| title.to_string());
        }
        if !x11_session() {
            return Err(APPS_MSG.into());
        }
        let x = x11_windows()?;
        let got = super::pick_title(x.rows.iter().map(|(_, t, _)| t.as_str()), title)
            .ok_or_else(|| format!("No window title has \"{title}\" in it."))?
            .to_string();
        let win = x.rows.iter().find(|(_, t, _)| *t == got).map(|(w, _, _)| *w).unwrap_or(0);
        // EWMH: source 2 (pager) asks the window manager to raise and focus it.
        let ev = ClientMessageEvent::new(32, win, x.net_active, [2u32, 0, 0, 0, 0]);
        x.conn
            .send_event(false, x.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev)
            .map_err(|e| format!("X11: {e}"))?;
        x.conn.flush().map_err(|e| format!("X11: {e}"))?;
        Ok(got)
    }

    /// The window whose title holds `title`, with its full title.
    fn find_window(x: &X11Windows, title: &str) -> Result<(Window, String), String> {
        let got = super::pick_title(x.rows.iter().map(|(_, t, _)| t.as_str()), title)
            .ok_or_else(|| format!("No window title has \"{title}\" in it."))?
            .to_string();
        let win = x.rows.iter().find(|(_, t, _)| *t == got).map(|(w, _, _)| *w).unwrap_or(0);
        Ok((win, got))
    }

    /// The client area's place on the root window and its size.
    pub(crate) fn window_geometry(title: &str) -> Result<WindowGeom, String> {
        if !x11_session() {
            return Err(GEOMETRY_MSG.into());
        }
        let x = x11_windows()?;
        let (win, _) = find_window(&x, title)?;
        let geo = x.conn.get_geometry(win).map_err(|e| format!("X11: {e}"))?.reply().map_err(|e| format!("X11: {e}"))?;
        let at = x
            .conn
            .translate_coordinates(win, x.root, 0, 0)
            .map_err(|e| format!("X11: {e}"))?
            .reply()
            .map_err(|e| format!("X11: {e}"))?;
        // `_NET_MOVERESIZE_WINDOW` places the frame's top-left, so report it
        // there too (client corner minus the left and top borders) and a
        // set of what get returned leaves the window where it was.
        let extents = atom(&x.conn, "_NET_FRAME_EXTENTS")?;
        let frame: Vec<u32> = x
            .conn
            .get_property(false, win, extents, AtomEnum::CARDINAL, 0, 4)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default();
        let (left, top) = match frame.as_slice() {
            [l, _, t, _] => (i32::try_from(*l).unwrap_or(0), i32::try_from(*t).unwrap_or(0)),
            _ => (0, 0),
        };
        Ok(WindowGeom {
            x: i32::from(at.dst_x) - left,
            y: i32::from(at.dst_y) - top,
            width: geo.width.into(),
            height: geo.height.into(),
        })
    }

    pub(crate) fn set_window_geometry(title: &str, g: &WindowGeom) -> Result<String, String> {
        if kde_wayland() {
            let body = super::kwin_geometry_script(title, g)?;
            return kwin_script(&body, "geometry").map(|()| title.to_string());
        }
        if !x11_session() {
            return Err(GEOMETRY_MSG.into());
        }
        let x = x11_windows()?;
        let (win, got) = find_window(&x, title)?;
        let net_move = atom(&x.conn, "_NET_MOVERESIZE_WINDOW")?;
        // EWMH: gravity 0 (the window's own), x/y/width/height all set (bits
        // 8-11), source 2 (pager), so the window manager places the frame.
        let flags = (1u32 << 8) | (1 << 9) | (1 << 10) | (1 << 11) | (2 << 12);
        let ev = ClientMessageEvent::new(32, win, net_move, [flags, g.x as u32, g.y as u32, g.width, g.height]);
        x.conn
            .send_event(false, x.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev)
            .map_err(|e| format!("X11: {e}"))?;
        x.conn.flush().map_err(|e| format!("X11: {e}"))?;
        Ok(got)
    }

    /// KDE Wayland: load a one-line KWin script that activates the first
    /// window whose caption holds `title`, run it, and unload it.
    fn kwin_focus(title: &str) -> Result<(), String> {
        let want = serde_json::to_string(&title.to_lowercase()).map_err(|e| e.to_string())?;
        let body = format!(
            "const want = {want};\n\
             const list = workspace.windowList ? workspace.windowList() : workspace.clientList();\n\
             for (const w of list) {{\n\
               if ((w.caption || \"\").toLowerCase().includes(want)) {{\n\
                 if (\"activeWindow\" in workspace) {{ workspace.activeWindow = w; }} else {{ workspace.activeClient = w; }}\n\
                 break;\n\
               }}\n\
             }}\n"
        );
        kwin_script(&body, "focus")
    }

    /// Write a KWin script to a temp file, run it once and remove it.
    fn kwin_script(body: &str, what: &str) -> Result<(), String> {
        let name = format!("grokhub-{what}-{}", std::process::id());
        let file = std::env::temp_dir().join(format!("{name}.js"));
        std::fs::write(&file, body).map_err(|e| format!("KWin script: {e}"))?;
        let out = kwin_run(&file, &name);
        let _ = std::fs::remove_file(&file);
        out
    }

    fn kwin_run(file: &Path, name: &str) -> Result<(), String> {
        let conn = zbus::blocking::Connection::session().map_err(|e| format!("KWin: {e}"))?;
        let path = file.to_string_lossy().into_owned();
        let reply = conn
            .call_method(Some("org.kde.KWin"), "/Scripting", Some("org.kde.kwin.Scripting"), "loadScript", &(path.as_str(), name))
            .map_err(|e| format!("KWin: {e}"))?;
        let id: i32 = reply.body().deserialize().map_err(|e| format!("KWin: {e}"))?;
        // KWin 6 names the object /Scripting/Script{id}; KWin 5 used /{id}.
        let ran = [format!("/Scripting/Script{id}"), format!("/{id}")].iter().any(|obj| {
            conn.call_method(Some("org.kde.KWin"), obj.as_str(), Some("org.kde.kwin.Script"), "run", &())
                .is_ok()
        });
        let _ = conn.call_method(Some("org.kde.KWin"), "/Scripting", Some("org.kde.kwin.Scripting"), "unloadScript", &(name,));
        if ran {
            Ok(())
        } else {
            Err(format!("KWin did not run the {} script.", name.split('-').nth(1).unwrap_or("window")))
        }
    }

    pub(crate) fn trash(paths: &[PathBuf]) -> Result<(), String> {
        let list: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let mut args: Vec<&str> = vec!["trash", "--"];
        args.extend(list.iter().map(String::as_str));
        if run("gio", &args)?.is_some() {
            return Ok(());
        }
        let mut args: Vec<&str> = vec!["--"];
        args.extend(list.iter().map(String::as_str));
        match run("trash-put", &args)? {
            Some(()) => Ok(()),
            None => Err(TRASH_MSG.into()),
        }
    }
}

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;

    use grokhub_core::desktop_mcp::{DesktopWindows, WindowGeom};
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, ShellExecuteW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextW, IsIconic, IsWindowVisible,
        SetForegroundWindow, SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOZORDER, SW_RESTORE, SW_SHOWNORMAL,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn text_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn class_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam as *mut Vec<HWND>);
        if IsWindowVisible(hwnd) != 0 && !text_of(hwnd).is_empty() {
            out.push(hwnd);
        }
        1
    }

    fn windows() -> Vec<HWND> {
        let mut out: Vec<HWND> = Vec::new();
        unsafe {
            EnumWindows(Some(collect), &mut out as *mut Vec<HWND> as LPARAM);
        }
        out
    }

    pub(crate) fn open_app(id: &str) -> Result<(), String> {
        let verb = wide("open");
        let file = wide(id);
        let code = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        // ShellExecute returns a value above 32 on success.
        if code > 32 {
            Ok(())
        } else {
            Err(format!("Windows could not open {id} (code {code})."))
        }
    }

    pub(crate) fn list_windows() -> Result<DesktopWindows, String> {
        let fg = unsafe { GetForegroundWindow() };
        let all = windows();
        let active = if fg.is_null() { String::new() } else { format!("{} {}", class_of(fg), text_of(fg)) };
        Ok(DesktopWindows { titles: all.into_iter().map(text_of).collect(), active })
    }

    /// The window whose title holds `title`, with its full title.
    fn find(title: &str) -> Result<(HWND, String), String> {
        let all: Vec<(HWND, String)> = windows().into_iter().map(|h| (h, text_of(h))).collect();
        let got = super::pick_title(all.iter().map(|(_, t)| t.as_str()), title)
            .ok_or_else(|| format!("No window title has \"{title}\" in it."))?
            .to_string();
        let hwnd = all.iter().find(|(_, t)| *t == got).map(|(h, _)| *h).unwrap_or(std::ptr::null_mut());
        Ok((hwnd, got))
    }

    pub(crate) fn window_geometry(title: &str) -> Result<WindowGeom, String> {
        let (hwnd, got) = find(title)?;
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if unsafe { GetWindowRect(hwnd, &mut rc) } == 0 {
            return Err(format!("Windows did not say where \"{got}\" is."));
        }
        Ok(WindowGeom {
            x: rc.left,
            y: rc.top,
            width: (rc.right - rc.left).max(0) as u32,
            height: (rc.bottom - rc.top).max(0) as u32,
        })
    }

    pub(crate) fn set_window_geometry(title: &str, g: &WindowGeom) -> Result<String, String> {
        let (hwnd, got) = find(title)?;
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            let ok = SetWindowPos(hwnd, std::ptr::null_mut(), g.x, g.y, g.width as i32, g.height as i32, SWP_NOZORDER | SWP_NOACTIVATE);
            if ok == 0 {
                return Err(format!("Windows did not move \"{got}\"."));
            }
        }
        Ok(got)
    }

    pub(crate) fn focus_window(title: &str) -> Result<String, String> {
        let (hwnd, got) = find(title)?;
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            if SetForegroundWindow(hwnd) == 0 {
                return Err(format!("Windows did not bring \"{got}\" to the front."));
            }
        }
        Ok(got)
    }

    /// Only a fixed drive has a Recycle Bin. On a network share, a removable
    /// or unknown drive, `SHFileOperationW` with undo deletes for good (G2).
    pub(crate) fn no_recycle_bin(path: &std::path::Path) -> bool {
        use std::path::{Component, Prefix};
        let root = match path.components().next() {
            Some(Component::Prefix(p)) => match p.kind() {
                Prefix::Disk(d) | Prefix::VerbatimDisk(d) => format!("{}:\\", d as char),
                _ => return true,
            },
            _ => return true,
        };
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        // DRIVE_FIXED
        unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(wide.as_ptr()) != 3 }
    }

    /// Recycle Bin: `SHFileOperationW` delete with undo, no dialogs.
    pub(crate) fn trash(paths: &[PathBuf]) -> Result<(), String> {
        // A double-NUL-terminated list of NUL-separated paths.
        let mut from: Vec<u16> = Vec::new();
        for p in paths {
            from.extend(p.as_os_str().encode_wide());
            from.push(0);
        }
        from.push(0);
        let mut op = SHFILEOPSTRUCTW {
            hwnd: std::ptr::null_mut(),
            wFunc: FO_DELETE,
            pFrom: from.as_ptr(),
            pTo: std::ptr::null(),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as u16,
            fAnyOperationsAborted: 0,
            hNameMappings: std::ptr::null_mut(),
            lpszProgressTitle: std::ptr::null(),
        };
        let code = unsafe { SHFileOperationW(&mut op) };
        if code == 0 && op.fAnyOperationsAborted == 0 {
            Ok(())
        } else {
            Err(format!("Windows could not move them to the Recycle Bin (code {code})."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_title_is_first_match_any_case() {
        let titles = ["GrokHub", "notes.txt — Kate", "Notes — Firefox"];
        assert_eq!(pick_title(titles, "NOTES"), Some("notes.txt — Kate"));
        assert_eq!(pick_title(titles, "firefox"), Some("Notes — Firefox"));
        assert_eq!(pick_title(titles, "  "), None);
        assert_eq!(pick_title(titles, "Dolphin"), None);
    }

    #[test]
    fn windows_changed_sees_new_gone_and_focus() {
        let before = DesktopWindows { titles: vec!["A".into(), "B".into()], active: "x A".into() };
        let same = DesktopWindows { titles: vec!["B".into(), "A".into()], active: "x A".into() };
        assert!(!windows_changed(&before, &same), "order alone is not a change");
        let opened = DesktopWindows { titles: vec!["A".into(), "B".into(), "C".into()], active: "x A".into() };
        assert!(windows_changed(&before, &opened));
        let focused = DesktopWindows { titles: vec!["A".into(), "B".into()], active: "y B".into() };
        assert!(windows_changed(&before, &focused));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_kwin_geometry_script_matches_the_caption_and_sets_the_frame() {
        let g = grokhub_core::desktop_mcp::WindowGeom { x: -20, y: 40, width: 800, height: 600 };
        let body = kwin_geometry_script("Notes \"draft\"", &g).unwrap();
        assert!(body.starts_with("const want = \"notes \\\"draft\\\"\";\n"), "{body}");
        assert!(body.contains("w.frameGeometry = { x: -20, y: 40, width: 800, height: 600 };"), "{body}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_ids_resolve_under_xdg_data_dirs() {
        let root = crate::config::test_config_root("apps-xdg");
        let apps = root.join("share").join("applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("org.example.Viewer.desktop"), "[Desktop Entry]\nName=Viewer\n").unwrap();
        let old_home = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", root.join("share"));
        let found = linux::desktop_file("org.example.Viewer");
        let missing = linux::desktop_file("org.example.Nope");
        match old_home {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        assert_eq!(found, Some(apps.join("org.example.Viewer.desktop")));
        assert_eq!(missing, None);
        let _ = std::fs::remove_dir_all(root);
    }
}
