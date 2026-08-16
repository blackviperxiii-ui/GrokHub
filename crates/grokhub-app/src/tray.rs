//! StatusNotifierItem tray. Close hides the cabin; the process keeps working.

use ksni::blocking::TrayMethods;
use ksni::menu::*;
use std::sync::mpsc;
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCmd {
    Show,
    Halt,
    Quit,
}

pub fn tray_wanted() -> bool {
    std::env::var("GROKHUB_TRAY").ok().as_deref() != Some("0")
}

/// SNI while the cabin is visible shows up as a persistent desktop notification
/// on GNOME. Only register the icon when the window starts hidden (`--agent`).
pub fn tray_needed_at_launch(window_hidden: bool) -> bool {
    window_hidden && tray_wanted()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideAction {
    Skip,
    Hide,
    HideAndPing,
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

pub fn show_from_tray_window() -> TrayWindow {
    TrayWindow {
        visible: true,
        minimized: false,
    }
}

/// winit Wayland cannot unmap. Prefer X11 when DISPLAY exists so close can hide.
pub fn prefer_x11_backend(existing: Option<&str>, has_display: bool) -> Option<&'static str> {
    if existing.is_some() {
        None
    } else if has_display {
        Some("x11")
    } else {
        None
    }
}

pub struct TrayHost {
    rx: mpsc::Receiver<TrayCmd>,
    _keep: ksni::blocking::Handle<GrokTray>,
}

impl TrayHost {
    pub fn try_recv(&self) -> Option<TrayCmd> {
        self.rx.try_recv().ok()
    }
}

struct GrokTray {
    tx: mpsc::Sender<TrayCmd>,
}

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
                label: "Halt hands".into(),
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
    spawn_worker(spawn)
}

pub fn take_spawn_result<T>(rx: &mpsc::Receiver<T>) -> Option<T> {
    match rx.try_recv() {
        Ok(v) => Some(v),
        Err(mpsc::TryRecvError::Empty) => None,
        Err(mpsc::TryRecvError::Disconnected) => None,
    }
}

/// SNI while the cabin is visible looks like a persistent notification.
pub fn keep_if_hidden<T: Send + 'static>(hidden: bool, host: T) -> Option<T> {
    if hidden {
        Some(host)
    } else {
        drop_off_thread(host);
        None
    }
}

pub fn drop_off_thread<T: Send + 'static>(value: T) {
    let _ = thread::Builder::new()
        .name("grokhub-tray-drop".into())
        .spawn(move || drop(value));
}

impl Drop for TrayHost {
    fn drop(&mut self) {
        let _ = self._keep.shutdown();
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
    fn tray_hide_prefers_x11_when_display_exists() {
        assert_eq!(prefer_x11_backend(None, true), Some("x11"));
        assert_eq!(prefer_x11_backend(Some("wayland"), true), None);
        assert_eq!(prefer_x11_backend(None, false), None);
    }

    #[test]
    fn already_hidden_cabin_does_not_hide_again() {
        assert_eq!(hide_action(false, false), HideAction::Skip);
        assert_eq!(hide_action(false, true), HideAction::Skip);
        assert_eq!(hide_action(true, false), HideAction::HideAndPing);
        assert_eq!(hide_action(true, true), HideAction::Hide);
    }

    #[test]
    fn visible_launch_does_not_need_a_tray_icon() {
        let prev = std::env::var("GROKHUB_TRAY").ok();
        std::env::remove_var("GROKHUB_TRAY");
        assert!(
            !tray_needed_at_launch(false),
            "A visible cabin must not register StatusNotifierItem"
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
    fn visible_cabin_discards_a_late_tray() {
        assert_eq!(keep_if_hidden(true, 1), Some(1));
        assert!(keep_if_hidden(false, 1).is_none());
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
}
