//! StatusNotifierItem tray. Close hides the cabin; the process keeps working.

#[cfg(unix)]
use ksni::blocking::TrayMethods;
#[cfg(unix)]
use ksni::menu::*;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCmd {
    Show,
    Halt,
    Quit,
}

pub fn tray_wanted() -> bool {
    env::var("GROKHUB_TRAY").ok().as_deref() != Some("0")
}

/// zbus/ksni cannot connect to libdbus `autolaunch:`. Without a unix path the
/// titlebar × unmaps the cabin and the tray icon never appears.
pub fn session_bus_is_usable(addr: &str) -> bool {
    let addr = addr.trim();
    if addr.is_empty() || addr == "autolaunch:" || addr.starts_with("autolaunch:") {
        return false;
    }
    addr.starts_with("unix:") || addr.starts_with("tcp:") || addr.starts_with("nonce-tcp:")
}

pub fn parse_session_bus_file(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix("DBUS_SESSION_BUS_ADDRESS=") else {
            continue;
        };
        let rest = rest.trim().trim_matches('\'').trim_matches('"');
        if session_bus_is_usable(rest) {
            return Some(rest.to_string());
        }
    }
    None
}

pub fn dbus_display_slot(display: &str) -> Option<String> {
    let rest = display.trim().strip_prefix(':').unwrap_or(display.trim());
    let slot: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if slot.is_empty() {
        None
    } else {
        Some(slot)
    }
}

pub fn session_bus_file_path(home: &Path, machine_id: &str, display: &str) -> Option<PathBuf> {
    let slot = dbus_display_slot(display)?;
    let id = machine_id.trim();
    if id.is_empty() {
        return None;
    }
    Some(home.join(".dbus/session-bus").join(format!("{id}-{slot}")))
}

pub fn resolved_session_bus(
    env_addr: Option<&str>,
    runtime_bus: Option<&Path>,
    session_file: Option<&str>,
) -> Option<String> {
    if let Some(addr) = env_addr {
        if session_bus_is_usable(addr) {
            return Some(addr.trim().to_string());
        }
    }
    if let Some(path) = runtime_bus {
        return Some(format!("unix:path={}", path.display()));
    }
    session_file.and_then(parse_session_bus_file)
}

fn read_legacy_session_bus_file() -> Option<String> {
    let home = env::var("HOME").ok().map(PathBuf::from)?;
    let machine = fs::read_to_string("/var/lib/dbus/machine-id")
        .or_else(|_| fs::read_to_string("/etc/machine-id"))
        .ok()?;
    let display = env::var("DISPLAY").unwrap_or_default();
    let path = session_bus_file_path(&home, &machine, &display)?;
    fs::read_to_string(path).ok()
}

/// Replace `autolaunch:` so StatusNotifierItem can actually register.
pub fn pin_session_bus() {
    let env_addr = env::var("DBUS_SESSION_BUS_ADDRESS").ok();
    let runtime_bus = env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(|d| PathBuf::from(d).join("bus"))
        .filter(|p| p.exists());
    let session_file = read_legacy_session_bus_file();
    let Some(addr) = resolved_session_bus(
        env_addr.as_deref(),
        runtime_bus.as_deref(),
        session_file.as_deref(),
    ) else {
        return;
    };
    if env_addr.as_deref() != Some(addr.as_str()) {
        env::set_var("DBUS_SESSION_BUS_ADDRESS", addr);
    }
}

/// The tray icon belongs in the tray from launch. × then hides the cabin;
/// it must not be the first time the icon appears.
pub fn tray_needed_at_launch(_window_hidden: bool) -> bool {
    tray_wanted()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideAction {
    Skip,
    Hide,
    HideAndPing,
}

/// Whether this close should toast, and whether that toast counts as seen.
/// Quiet hours suppress the toast and leave the flag clear so the first close
/// outside quiet hours can still tip once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayTip {
    pub show: bool,
    pub mark_seen: bool,
}

pub fn tray_tip_on_hide(already_seen: bool, quiet: bool) -> TrayTip {
    if already_seen {
        TrayTip {
            show: false,
            mark_seen: true,
        }
    } else if !crate::notify::allow_ping(quiet) {
        TrayTip {
            show: false,
            mark_seen: false,
        }
    } else {
        TrayTip {
            show: true,
            mark_seen: true,
        }
    }
}

/// Close-to-tray can fire every frame while `close_requested` sticks. Do not
/// re-unmap or re-ping once the cabin is already hidden.
pub fn hide_action(window_visible: bool, already_told: bool) -> HideAction {
    if !window_visible {
        HideAction::Skip
    } else if already_told {
        HideAction::Hide
    } else {
        HideAction::HideAndPing
    }
}

/// A pinned taskbar click maps the existing cabin. `close_requested` can still
/// be true from the earlier × — cancel it so hide does not run again.
pub fn ignore_close_while_hidden(window_visible: bool, close_requested: bool) -> bool {
    !window_visible && close_requested
}

/// Tray Quit / restart set `want_quit` then Close. A hidden cabin must not
/// CancelClose that — otherwise Quit leaves a process with no window.
pub fn ignore_close_request(
    window_visible: bool,
    close_requested: bool,
    want_quit: bool,
) -> bool {
    !want_quit && ignore_close_while_hidden(window_visible, close_requested)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenTick {
    Raise,
    StayHidden,
}

/// Taskbar activate focuses an unmapped cabin. Titlebar × hides after the
/// raise check, and winit still reports focused on the next frame — do not
/// raise until the withdrawn cabin actually lost focus.
pub fn hidden_window_tick(
    hidden: bool,
    focused: bool,
    just_hid: bool,
    saw_unfocused: bool,
) -> HiddenTick {
    if hidden && focused && !just_hid && saw_unfocused {
        HiddenTick::Raise
    } else {
        HiddenTick::StayHidden
    }
}

/// × / WM close can bounce FocusLost then FocusGained in the same beat.
/// Ignore that bounce so the cabin does not flash back out of the tray.
pub const HIDE_RAISE_GRACE_MS: u64 = 400;

pub fn hidden_raise_ready(elapsed_ms: u64) -> bool {
    elapsed_ms >= HIDE_RAISE_GRACE_MS
}

/// Re-send unmap only while leftover focus is still on the withdrawn cabin.
/// Spamming Visible(false) every pulse after FocusLost can flash it back.
pub fn reapply_unmap(hidden: bool, focused: bool) -> bool {
    hidden && focused
}

/// Latch: a hide that never saw FocusLost must not treat leftover focus as a raise.
pub fn remember_hidden_unfocus(focused: bool, already: bool) -> bool {
    already || !focused
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CabinClaim {
    ThisProcess,
    AlreadyRunning,
}

pub fn cabin_pid_path(dir: &Path) -> PathBuf {
    dir.join("cabin.pid")
}

pub fn cabin_raise_path(dir: &Path) -> PathBuf {
    dir.join("cabin.raise")
}

pub fn parse_cabin_pid(text: &str) -> Option<u32> {
    text.trim().parse().ok().filter(|&pid| pid != 0)
}

pub fn cabin_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(windows)]
    {
        windows_pid_alive(pid)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        true
    }
}

#[cfg(windows)]
fn windows_pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}

/// Second launch writes `cabin.raise` and exits so the running cabin can show.
pub fn claim_cabin_at(dir: &Path, this_pid: u32, alive: impl Fn(u32) -> bool) -> CabinClaim {
    let _ = fs::create_dir_all(dir);
    let path = cabin_pid_path(dir);
    if let Ok(text) = fs::read_to_string(&path) {
        if let Some(pid) = parse_cabin_pid(&text) {
            if pid != this_pid && alive(pid) {
                request_cabin_raise_at(dir);
                return CabinClaim::AlreadyRunning;
            }
        }
    }
    let _ = fs::write(&path, this_pid.to_string());
    let _ = fs::remove_file(cabin_raise_path(dir));
    CabinClaim::ThisProcess
}

pub fn try_claim_cabin() -> bool {
    matches!(
        claim_cabin_at(
            &crate::config::config_dir(),
            std::process::id(),
            cabin_pid_alive
        ),
        CabinClaim::ThisProcess
    )
}

/// Restart must drop our pid file before spawning or the child only raises us.
pub fn release_cabin_claim_at(dir: &Path, this_pid: u32) {
    let path = cabin_pid_path(dir);
    if let Ok(text) = fs::read_to_string(&path) {
        if parse_cabin_pid(&text) == Some(this_pid) {
            let _ = fs::remove_file(&path);
        }
    }
    let _ = fs::remove_file(cabin_raise_path(dir));
}

pub fn release_cabin_claim() {
    release_cabin_claim_at(&crate::config::config_dir(), std::process::id());
}

pub fn request_cabin_raise_at(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let _ = fs::write(cabin_raise_path(dir), b"1");
}

pub fn take_cabin_raise_at(dir: &Path) -> bool {
    let path = cabin_raise_path(dir);
    if !path.exists() {
        return false;
    }
    let _ = fs::remove_file(&path);
    true
}

pub fn take_cabin_raise() -> bool {
    take_cabin_raise_at(&crate::config::config_dir())
}

/// A sibling spawn writes `cabin.raise` before it exits. Restart must not
/// honor that and CancelClose — that keeps the old process alive.
pub fn honor_cabin_raise(want_quit: bool) -> bool {
    !want_quit
}

pub fn should_hide_on_close(close_to_tray: bool, tray_alive: bool) -> bool {
    close_to_tray && (tray_alive || tray_wanted())
}

/// What the cabin window should be after close-to-tray or Show cabin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayWindow {
    pub visible: bool,
    pub minimized: bool,
}

/// Close to tray unmaps the cabin. Iconify is minimize — that leaves a taskbar stub.
pub fn hide_to_tray_window() -> TrayWindow {
    TrayWindow {
        visible: false,
        minimized: false,
    }
}

/// Win32 `WS_EX_TOOLWINDOW` — hidden from the taskbar and Alt-Tab.
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
/// Win32 `WS_EX_APPWINDOW` — forced onto the taskbar.
pub const WS_EX_APPWINDOW: u32 = 0x0004_0000;

/// × / close-to-tray: keep the HWND so winit timers live, drop the taskbar stub.
pub fn windows_unmap_exstyle(ex: u32) -> u32 {
    (ex | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW
}

/// Show cabin: put the cabin back on the taskbar.
pub fn windows_map_exstyle(ex: u32) -> u32 {
    (ex | WS_EX_APPWINDOW) & !WS_EX_TOOLWINDOW
}

pub fn show_from_tray_window() -> TrayWindow {
    TrayWindow {
        visible: true,
        minimized: false,
    }
}

/// winit 0.30 ignores `WINIT_UNIX_BACKEND`. It picks Wayland whenever
/// `WAYLAND_DISPLAY` / `WAYLAND_SOCKET` is set, and Wayland cannot unmap on ×.
/// Clear those and keep X11 when a DISPLAY exists so close-to-tray works.
pub fn should_clear_wayland(has_display: bool, wayland_set: bool) -> bool {
    has_display && wayland_set
}

pub fn prefer_x11_backend(existing: Option<&str>, has_display: bool) -> Option<&'static str> {
    if existing.is_some() {
        None
    } else if has_display {
        Some("x11")
    } else {
        None
    }
}

/// Apply the X11 backend so `Visible(false)` actually unmaps the cabin.
pub fn force_x11_for_close_to_tray(has_display: bool, wayland_set: bool) {
    if !has_display {
        return;
    }
    if should_clear_wayland(has_display, wayland_set) {
        env::remove_var("WAYLAND_DISPLAY");
        env::remove_var("WAYLAND_SOCKET");
    }
    if prefer_x11_backend(env::var("WINIT_UNIX_BACKEND").ok().as_deref(), has_display).is_some() {
        env::set_var("WINIT_UNIX_BACKEND", "x11");
    }
}

pub struct TrayHost {
    rx: mpsc::Receiver<TrayCmd>,
    #[cfg(unix)]
    _keep: ksni::blocking::Handle<GrokTray>,
    #[cfg(windows)]
    _keep: tray_icon::TrayIcon,
    #[cfg(windows)]
    _menu_items: Vec<tray_icon::menu::MenuItem>,
}

impl TrayHost {
    pub fn try_recv(&self) -> Option<TrayCmd> {
        self.rx.try_recv().ok()
    }
}

#[cfg(unix)]
struct GrokTray {
    tx: mpsc::Sender<TrayCmd>,
}

#[cfg(unix)]
impl ksni::Tray for GrokTray {
    fn id(&self) -> String {
        "grokhub".into()
    }

    fn title(&self) -> String {
        "GrokHub".into()
    }

    fn icon_name(&self) -> String {
        "grokhub".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![cabin_icon()]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: "grokhub".into(),
            icon_pixmap: vec![cabin_icon()],
            title: "GrokHub".into(),
            description: "Cabin — close stays in the tray".into(),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(TrayCmd::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Show cabin".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(TrayCmd::Show);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Halt".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(TrayCmd::Halt);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send(TrayCmd::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        true
    }
}

#[cfg(unix)]
fn cabin_icon() -> ksni::Icon {
    let w = 22i32;
    let h = 22i32;
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            let (a, r, g, b) = if edge {
                (255, 40, 28, 22)
            } else {
                (255, 232, 168, 96)
            };
            data.extend_from_slice(&[a, r, g, b]);
        }
    }
    ksni::Icon {
        width: w,
        height: h,
        data,
    }
}

#[cfg(unix)]
pub fn spawn() -> Option<TrayHost> {
    if !tray_wanted() {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    let tray = GrokTray { tx };
    match tray.assume_sni_available(true).spawn() {
        Ok(handle) => Some(TrayHost { rx, _keep: handle }),
        Err(_) => None,
    }
}

/// Linux installs `hicolor` + `icon_name = grokhub`. Windows has no icon
/// theme — the tray must carry the same cabin PNG as RGBA.
pub fn cabin_tray_png() -> &'static [u8] {
    include_bytes!("../../../packaging/icons/hicolor/32x32/apps/grokhub.png")
}

pub fn cabin_tray_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(cabin_tray_png()).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 || width != height {
        return None;
    }
    Some((img.into_raw(), width, height))
}

#[cfg(windows)]
fn windows_tray_icon() -> Option<tray_icon::Icon> {
    let (rgba, width, height) = cabin_tray_rgba()?;
    tray_icon::Icon::from_rgba(rgba, width, height).ok()
}

/// Build the Win32 tray on the caller (UI) thread. A worker `GetMessageW`
/// loop is what closed the right-click menu in about a second — the popup
/// was owned by a thread that was not the foreground cabin.
#[cfg(windows)]
fn windows_tray_attach() -> Option<TrayHost> {
    if !tray_wanted() {
        return None;
    }
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let (tx, rx) = mpsc::channel();
    let show = MenuItem::new("Show cabin", true, None);
    let halt = MenuItem::new("Halt", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    let _ = menu.append(&show);
    let _ = menu.append(&halt);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit);
    let icon = windows_tray_icon()?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("GrokHub")
        .with_icon(icon)
        .build()
        .ok()?;
    let show_id = show.id().clone();
    let halt_id = halt.id().clone();
    let quit_id = quit.id().clone();
    let menu_tx = tx.clone();
    std::thread::spawn(move || {
        while let Ok(ev) = MenuEvent::receiver().recv() {
            let cmd = if ev.id == show_id {
                TrayCmd::Show
            } else if ev.id == halt_id {
                TrayCmd::Halt
            } else if ev.id == quit_id {
                TrayCmd::Quit
            } else {
                continue;
            };
            if menu_tx.send(cmd).is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        while let Ok(ev) = TrayIconEvent::receiver().recv() {
            let show = matches!(
                ev,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            );
            if show && tx.send(TrayCmd::Show).is_err() {
                break;
            }
        }
    });
    Some(TrayHost {
        rx,
        _keep: tray,
        _menu_items: vec![show, halt, quit],
    })
}

/// ksni `spawn()` `block_on`s session-bus setup on the caller. Never do that
/// on the UI thread — a missing bus hangs close/quit for tens of seconds.
pub fn spawn_worker<F, T>(f: F) -> mpsc::Receiver<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let _ = thread::Builder::new()
        .name("grokhub-tray".into())
        .spawn(move || {
            let _ = tx.send(f());
        });
    rx
}

pub fn begin_tray_spawn() -> mpsc::Receiver<Option<TrayHost>> {
    #[cfg(unix)]
    {
        spawn_worker(spawn)
    }
    #[cfg(windows)]
    {
        // Attach on this thread so winit pumps the tray HWND. A worker
        // GetMessage loop owned the popup and Windows dismissed it in ~1s.
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(windows_tray_attach());
        rx
    }
    #[cfg(not(any(unix, windows)))]
    {
        spawn_worker(|| None)
    }
}

pub fn take_spawn_result<T>(rx: &mpsc::Receiver<T>) -> Option<T> {
    match rx.try_recv() {
        Ok(v) => Some(v),
        Err(mpsc::TryRecvError::Empty) => None,
        Err(mpsc::TryRecvError::Disconnected) => None,
    }
}

/// Keep the StatusNotifierItem whether the cabin is visible or hidden.
/// No `Send` bound — Windows `TrayIcon` is `Rc` / `!Send`.
pub fn keep_if_hidden<T>(_hidden: bool, host: T) -> Option<T> {
    Some(host)
}

pub fn drop_off_thread<T: Send + 'static>(value: T) {
    let _ = thread::Builder::new()
        .name("grokhub-tray-drop".into())
        .spawn(move || drop(value));
}

/// Tear down the tray. Linux D-Bus shutdown is slow — do that off-thread.
/// Windows `TrayIcon` is `!Send` and must die on the UI thread that created it
/// (a drop worker leaves a ghost icon and does not compile).
pub fn drop_tray(host: TrayHost) {
    #[cfg(unix)]
    drop_off_thread(host);
    #[cfg(not(unix))]
    drop(host);
}

impl Drop for TrayHost {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = self._keep.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_needs_a_live_tray() {
        assert!(should_hide_on_close(true, true));
        assert!(!should_hide_on_close(false, true));
        let prev = std::env::var("GROKHUB_TRAY").ok();
        std::env::remove_var("GROKHUB_TRAY");
        assert!(
            should_hide_on_close(true, false),
            "Close to tray still hides when the icon failed to spawn"
        );
        std::env::set_var("GROKHUB_TRAY", "0");
        assert!(!should_hide_on_close(true, false));
        match prev {
            Some(v) => std::env::set_var("GROKHUB_TRAY", v),
            None => std::env::remove_var("GROKHUB_TRAY"),
        }
    }

    #[test]
    fn close_to_tray_unmaps_and_does_not_minimize() {
        let hide = hide_to_tray_window();
        assert!(!hide.visible);
        assert!(
            !hide.minimized,
            "Minimized(true) parks a taskbar stub instead of the tray"
        );
        let show = show_from_tray_window();
        assert!(show.visible);
        assert!(!show.minimized);
    }

    #[test]
    fn windows_close_unmaps_the_taskbar_stub() {
        assert_eq!(windows_unmap_exstyle(0), WS_EX_TOOLWINDOW);
        assert_eq!(windows_unmap_exstyle(WS_EX_APPWINDOW), WS_EX_TOOLWINDOW);
        assert_eq!(
            windows_unmap_exstyle(WS_EX_APPWINDOW | 0x200),
            WS_EX_TOOLWINDOW | 0x200
        );
        assert_eq!(windows_map_exstyle(WS_EX_TOOLWINDOW), WS_EX_APPWINDOW);
        assert_eq!(
            windows_map_exstyle(windows_unmap_exstyle(WS_EX_APPWINDOW)),
            WS_EX_APPWINDOW
        );
        let src = include_str!("win_native.rs");
        assert!(
            src.contains("windows_unmap_exstyle") && src.contains("SWP_NOACTIVATE"),
            "hide_cabin must drop the taskbar stub without activating (that closed the tray menu): {src}"
        );
        assert!(
            src.contains("windows_map_exstyle"),
            "Show cabin must put the cabin back on the taskbar: {src}"
        );
    }

    #[test]
    fn windows_tray_menu_attaches_on_the_ui_thread() {
        let src = include_str!("tray.rs");
        let begin = src
            .split("pub fn begin_tray_spawn")
            .nth(1)
            .and_then(|s| s.split("pub fn take_spawn_result").next())
            .expect("begin_tray_spawn");
        assert!(
            begin.contains("windows_tray_attach") && !begin.contains("GetMessageW"),
            "Windows tray must attach on the UI thread or the right-click menu self-closes: {begin}"
        );
        let attach = src
            .split("fn windows_tray_attach")
            .nth(1)
            .and_then(|s| s.split("/// ksni").next())
            .expect("windows_tray_attach");
        assert!(
            !attach.contains("GetMessageW") && attach.contains("with_menu"),
            "a worker GetMessage loop is what closed the tray menu in ~1s: {attach}"
        );
        let drop = src
            .split("pub fn drop_tray")
            .nth(1)
            .and_then(|s| s.split("impl Drop for TrayHost").next())
            .expect("drop_tray");
        assert!(
            drop.contains("cfg(not(unix))") && drop.contains("drop(host)"),
            "Windows TrayIcon is !Send — drop on the UI thread: {drop}"
        );
        let app = concat!(
            include_str!("app/mod.rs"),
            include_str!("app/acp.rs"),
            include_str!("app/chat_kick.rs"),
            include_str!("app/night.rs"),
        );
        assert_eq!(
            app.matches("drop_off_thread(tray)").count(),
            0,
            "TrayHost must not go through drop_off_thread (Send)"
        );
        assert!(
            app.contains("drop_tray(tray)"),
            "Quit / restart / on_exit must drop the tray via drop_tray"
        );
    }

    #[test]
    fn tray_hide_prefers_x11_when_display_exists() {
        assert_eq!(prefer_x11_backend(None, true), Some("x11"));
        assert_eq!(prefer_x11_backend(Some("wayland"), true), None);
        assert_eq!(prefer_x11_backend(None, false), None);
        assert!(
            should_clear_wayland(true, true),
            "winit 0.30 picks Wayland first; × cannot unmap until WAYLAND_DISPLAY is cleared"
        );
        assert!(!should_clear_wayland(true, false));
        assert!(!should_clear_wayland(false, true));
    }

    #[test]
    fn already_hidden_cabin_does_not_hide_again() {
        assert_eq!(hide_action(false, false), HideAction::Skip);
        assert_eq!(hide_action(false, true), HideAction::Skip);
        assert_eq!(hide_action(true, false), HideAction::HideAndPing);
        assert_eq!(hide_action(true, true), HideAction::Hide);
        let first = tray_tip_on_hide(false, false);
        assert!(first.show && first.mark_seen);
        let quiet = tray_tip_on_hide(false, true);
        assert!(!quiet.show && !quiet.mark_seen);
        let again = tray_tip_on_hide(true, false);
        assert!(!again.show && again.mark_seen);
        let hide = include_str!("app/mod.rs")
            .split("fn hide_to_tray")
            .nth(1)
            .and_then(|s| s.split("fn unmap_to_tray").next())
            .expect("hide_to_tray");
        assert!(
            hide.contains("close_to_tray_tip_seen")
                && hide.contains("tray_tip_on_hide")
                && hide.contains("if tip.show")
                && hide.contains("if tip.mark_seen")
                && hide.contains("Still running in the tray")
                && hide.contains("In the tray — Show cabin to sit down"),
            "first tip persists only when shown; cabin status stays: {hide}"
        );
    }

    #[test]
    fn hidden_cabin_cancels_a_sticky_close() {
        assert!(ignore_close_while_hidden(false, true));
        assert!(!ignore_close_while_hidden(false, false));
        assert!(!ignore_close_while_hidden(true, true));
        assert!(
            ignore_close_request(false, true, false),
            "a leftover close after × must not hide again"
        );
        assert!(
            !ignore_close_request(false, true, true),
            "tray Quit on a hidden cabin must not CancelClose"
        );
        assert!(!ignore_close_request(true, true, true));
        assert!(!ignore_close_request(true, true, false));
    }

    #[test]
    fn taskbar_focus_raises_a_hidden_cabin() {
        assert_eq!(
            hidden_window_tick(true, true, false, true),
            HiddenTick::Raise,
            "pinned taskbar maps + focuses the unmapped cabin after it lost focus"
        );
        assert_eq!(
            hidden_window_tick(true, true, true, false),
            HiddenTick::StayHidden,
            "the hide frame is still focused — raising it flashes the window back"
        );
        assert_eq!(
            hidden_window_tick(true, true, false, false),
            HiddenTick::StayHidden,
            "titlebar × hides after the raise check; next frame is still focused"
        );
        assert_eq!(hidden_window_tick(true, false, false, false), HiddenTick::StayHidden);
        assert_eq!(hidden_window_tick(false, true, false, true), HiddenTick::StayHidden);
        assert!(
            !remember_hidden_unfocus(true, false),
            "a hide that never lost focus must not arm a taskbar raise"
        );
        assert!(remember_hidden_unfocus(false, false));
        assert!(remember_hidden_unfocus(true, true));
        assert!(!hidden_raise_ready(0));
        assert!(!hidden_raise_ready(HIDE_RAISE_GRACE_MS - 1));
        assert!(hidden_raise_ready(HIDE_RAISE_GRACE_MS));
        assert!(
            reapply_unmap(true, true),
            "leftover focus after × must keep the cabin unmapped"
        );
        assert!(
            !reapply_unmap(true, false),
            "an unfocused tray cabin must not be sent Visible(false) every pulse"
        );
        assert!(!reapply_unmap(false, true));
    }

    #[test]
    fn second_launch_asks_the_running_cabin_to_show() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-cabin-claim-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            claim_cabin_at(&dir, 11, |_| false),
            CabinClaim::ThisProcess
        );
        assert_eq!(fs::read_to_string(cabin_pid_path(&dir)).unwrap().trim(), "11");
        assert!(!cabin_raise_path(&dir).exists());
        assert_eq!(
            claim_cabin_at(&dir, 22, |pid| pid == 11),
            CabinClaim::AlreadyRunning
        );
        assert!(take_cabin_raise_at(&dir));
        assert!(!take_cabin_raise_at(&dir));
        assert_eq!(
            claim_cabin_at(&dir, 33, |_| false),
            CabinClaim::ThisProcess,
            "a dead pid file must not block a new cabin"
        );
        assert_eq!(
            claim_cabin_at(&dir, 44, |_| false),
            CabinClaim::ThisProcess
        );
        release_cabin_claim_at(&dir, 99);
        assert_eq!(
            fs::read_to_string(cabin_pid_path(&dir)).unwrap().trim(),
            "44",
            "another pid must not drop our claim"
        );
        release_cabin_claim_at(&dir, 44);
        assert!(
            !cabin_pid_path(&dir).exists(),
            "restart must free the lock so the new cabin can claim"
        );
        assert_eq!(parse_cabin_pid(" 42\n"), Some(42));
        assert_eq!(parse_cabin_pid("0"), None);
        assert!(!cabin_pid_alive(0));
        assert!(honor_cabin_raise(false));
        assert!(
            !honor_cabin_raise(true),
            "restart/quit must not raise the old cabin after a sibling spawn"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_tray_uses_the_linux_cabin_mark() {
        let src = include_str!("tray.rs");
        let win = src
            .split("fn windows_tray_attach")
            .nth(1)
            .and_then(|s| s.split("/// ksni").next())
            .expect("windows_tray_attach");
        assert!(
            win.contains("windows_tray_icon") && !win.contains("from_rgba(vec!"),
            "Windows tray must load the cabin PNG, not a fill square: {win}"
        );
        assert!(
            src.contains("hicolor/32x32/apps/grokhub.png"),
            "Windows tray must embed the Linux 32px cabin PNG: {src}"
        );
        let (rgba, w, h) = cabin_tray_rgba().expect("cabin tray png");
        assert_eq!((w, h), (32, 32));
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let linux = image::load_from_memory(include_bytes!(
            "../../../packaging/icons/hicolor/32x32/apps/grokhub.png"
        ))
        .unwrap()
        .into_rgba8();
        assert_eq!(rgba, linux.into_raw());
        let cx = ((16 * 32 + 16) * 4) as usize;
        assert_ne!(
            &rgba[cx..cx + 3],
            &[232, 168, 96],
            "cabin well must not be the old orange fill"
        );
    }

    #[test]
    fn windows_ico_matches_linux_hicolor() {
        let ico = image::load_from_memory(include_bytes!(
            "../../../packaging/windows/grokhub.ico"
        ))
        .expect("grokhub.ico")
        .into_rgba8();
        let linux = image::load_from_memory(include_bytes!(
            "../../../packaging/icons/hicolor/256x256/apps/grokhub.png"
        ))
        .unwrap()
        .into_rgba8();
        assert_eq!(
            ico.dimensions(),
            linux.dimensions(),
            "winresource / Inno ICO must carry the Linux 256px cabin"
        );
        assert_eq!(
            ico.as_raw(),
            linux.as_raw(),
            "Windows ICO pixels must match packaging/icons/hicolor/256x256"
        );
    }

    #[test]
    fn visible_launch_registers_the_tray_icon() {
        let prev = std::env::var("GROKHUB_TRAY").ok();
        std::env::remove_var("GROKHUB_TRAY");
        assert!(
            tray_needed_at_launch(false),
            "A visible cabin must already have a tray icon"
        );
        assert!(tray_needed_at_launch(true), "--agent starts hidden with a tray");
        std::env::set_var("GROKHUB_TRAY", "0");
        assert!(!tray_needed_at_launch(true));
        match prev {
            Some(v) => std::env::set_var("GROKHUB_TRAY", v),
            None => std::env::remove_var("GROKHUB_TRAY"),
        }
    }

    #[test]
    fn spawn_worker_returns_before_the_job_finishes() {
        let (block_tx, block_rx) = mpsc::channel::<()>();
        let started = std::time::Instant::now();
        let rx = spawn_worker(move || {
            block_rx.recv().unwrap();
            9
        });
        assert!(
            started.elapsed() < std::time::Duration::from_millis(80),
            "Hide/quit must not wait for StatusNotifierItem D-Bus setup"
        );
        assert!(
            take_spawn_result(&rx).is_none(),
            "A blocked worker must not look ready"
        );
        block_tx.send(()).unwrap();
        let t0 = std::time::Instant::now();
        loop {
            if let Some(v) = take_spawn_result(&rx) {
                assert_eq!(v, 9);
                break;
            }
            assert!(
                t0.elapsed() < std::time::Duration::from_secs(2),
                "worker should finish after unblock"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn take_spawn_result_does_not_block_when_empty() {
        let (_tx, rx) = mpsc::sync_channel::<i32>(1);
        let started = std::time::Instant::now();
        assert!(take_spawn_result(&rx).is_none());
        assert!(started.elapsed() < std::time::Duration::from_millis(50));
    }

    #[test]
    fn visible_cabin_keeps_the_tray() {
        assert_eq!(keep_if_hidden(true, 1), Some(1));
        assert_eq!(keep_if_hidden(false, 1), Some(1));
    }

    #[test]
    fn drop_off_thread_returns_before_destructor_finishes() {
        struct Slow(mpsc::Sender<()>);
        impl Drop for Slow {
            fn drop(&mut self) {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let _ = self.0.send(());
            }
        }
        let (tx, rx) = mpsc::channel();
        let started = std::time::Instant::now();
        drop_off_thread(Slow(tx));
        assert!(
            started.elapsed() < std::time::Duration::from_millis(80),
            "Quit must not wait for tray D-Bus teardown on the UI thread"
        );
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .expect("destructor should still run off-thread");
    }

    #[test]
    fn autolaunch_session_bus_cannot_host_the_tray() {
        assert!(
            !session_bus_is_usable("autolaunch:"),
            "zbus/ksni never connect to autolaunch: — × then unmaps with no icon"
        );
        assert!(!session_bus_is_usable(""));
        assert!(!session_bus_is_usable("  "));
        assert!(session_bus_is_usable(
            "unix:path=/tmp/dbus-isXxqaqqzS,guid=c6bdfebad6fd38852379ce546a7ea1c1"
        ));
        assert!(session_bus_is_usable("unix:abstract=/tmp/dbus-foo"));
        assert!(session_bus_is_usable("tcp:host=127.0.0.1,port=1234"));
    }

    #[test]
    fn session_bus_file_pins_unix_path_when_env_is_autolaunch() {
        let file = "\
# comment
DBUS_SESSION_BUS_ADDRESS='unix:path=/tmp/dbus-isXxqaqqzS,guid=abc'
DBUS_SESSION_BUS_PID=1330
";
        assert_eq!(
            parse_session_bus_file(file).as_deref(),
            Some("unix:path=/tmp/dbus-isXxqaqqzS,guid=abc")
        );
        assert_eq!(
            resolved_session_bus(Some("autolaunch:"), None, Some(file)).as_deref(),
            Some("unix:path=/tmp/dbus-isXxqaqqzS,guid=abc")
        );
        assert_eq!(
            resolved_session_bus(Some("unix:path=/run/user/1000/bus"), None, Some(file)).as_deref(),
            Some("unix:path=/run/user/1000/bus"),
            "a real env address must win"
        );
        let runtime = std::path::Path::new("/run/user/1000/bus");
        assert_eq!(
            resolved_session_bus(Some("autolaunch:"), Some(runtime), Some(file)).as_deref(),
            Some("unix:path=/run/user/1000/bus")
        );
        assert_eq!(dbus_display_slot(":1"), Some("1".into()));
        assert_eq!(dbus_display_slot(":1.0"), Some("1".into()));
        assert_eq!(
            session_bus_file_path(
                std::path::Path::new("/home/viper"),
                "44cb5599dcc24b78a2ed9dac18a9b2a5\n",
                ":1.0"
            ),
            Some(std::path::PathBuf::from(
                "/home/viper/.dbus/session-bus/44cb5599dcc24b78a2ed9dac18a9b2a5-1"
            ))
        );
    }
}
