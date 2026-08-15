//! StatusNotifierItem tray. Close hides the cabin; the process keeps working.

use ksni::blocking::TrayMethods;
use ksni::menu::*;
use std::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCmd {
    Show,
    Halt,
    Quit,
}

pub fn tray_wanted() -> bool {
    std::env::var("GROKHUB_TRAY").ok().as_deref() != Some("0")
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
}
