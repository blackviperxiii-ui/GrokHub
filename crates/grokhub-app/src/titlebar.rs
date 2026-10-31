use eframe::egui;

pub fn apply_tray_window(ctx: &egui::Context, w: crate::tray::TrayWindow) {
    // winit Visible(false) is SW_HIDE on Windows — that freezes egui timers
    // so tray Quit / Show never run. Linux still unmaps. Windows cloaks and
    // drops the taskbar stub (TOOLWINDOW) so × is unmap-to-tray, not minimize.
    #[cfg(not(windows))]
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(w.visible));
    #[cfg(windows)]
    if w.visible {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    }
    if w.visible {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(w.minimized));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

pub fn titlebar_chrome_size() -> egui::Vec2 {
    // Win11 caption is ~46×32; our bar is 40px so keep the hit 46×40.
    // Glyphs use a centered square — shrink() on a 36×40 rect stretched ×/□.
    egui::vec2(46.0, crate::theme::TITLEBAR_H)
}

#[derive(Clone, Copy)]
pub enum ChromeBtn {
    Close,
    Maximize,
    Restore,
    Minimize,
    /// Three-bar menu for Compact, Copy session, and Export.
    Menu,
}

/// egui ignores a click held longer than 0.8s (`max_click_duration`). The
/// titlebar × is a close control — a drag that started on it must still hide.
/// A release that started elsewhere (titlebar drag, text select) must not.
pub fn chrome_activated(clicked: bool, drag_stopped: bool) -> bool {
    clicked || drag_stopped
}

pub fn titlebar_chrome_hit(resp: &egui::Response) -> bool {
    chrome_activated(resp.clicked(), resp.drag_stopped())
}

/// Undecorated cabin: the titlebar body moves the window.
pub fn titlebar_should_start_drag(drag_started: bool) -> bool {
    drag_started
}

pub fn titlebar_chrome_btn(ui: &mut egui::Ui, kind: ChromeBtn) -> egui::Response {
    let (_rect, resp) =
        ui.allocate_exact_size(titlebar_chrome_size(), egui::Sense::click_and_drag());
    let (resp, rect, wash) = crate::theme::feel_button(ui, resp, egui::Color32::TRANSPARENT);
    if wash.a() > 0 {
        ui.painter().rect_filled(rect, 6.0, wash);
    }
    let color = if resp.hovered() {
        crate::theme::fg()
    } else {
        crate::theme::muted()
    };
    let cue = crate::motion::icon::Cue::of(&resp, false);
    let pose = crate::motion::icon::pose(ui, resp.id, cue, chrome_kick(kind));
    paint_chrome_glyph(ui, rect, kind, color, pose);
    let name = titlebar_chrome_tip(kind, false);
    let enabled = resp.enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    resp
}

/// How each chrome glyph answers the pointer: the × turns a quarter (it
/// lands looking the same), minimize dips, the others rise.
fn chrome_kick(kind: ChromeBtn) -> crate::motion::icon::Kick {
    use crate::motion::icon::Kick;
    match kind {
        ChromeBtn::Close => Kick::Turn(std::f32::consts::FRAC_PI_2),
        ChromeBtn::Minimize => Kick::Nudge(egui::vec2(0.0, 1.0)),
        ChromeBtn::Menu | ChromeBtn::Maximize | ChromeBtn::Restore => Kick::Lift,
    }
}

fn paint_chrome_glyph(
    ui: &egui::Ui,
    rect: egui::Rect,
    kind: ChromeBtn,
    color: egui::Color32,
    pose: crate::motion::icon::Pose,
) {
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.25_f32, color);
    let side = 10.0_f32;
    let r = egui::Rect::from_center_size(rect.center(), egui::vec2(side, side));
    let c = r.center();
    let m = |p: egui::Pos2| pose.map(c, p);
    // A square outline through the pose, so it turns and scales with the rest.
    let square = |sq: egui::Rect| {
        let pts = [
            sq.left_top(),
            sq.right_top(),
            sq.right_bottom(),
            sq.left_bottom(),
        ];
        pts.map(m).to_vec()
    };
    match kind {
        ChromeBtn::Close => {
            painter.line_segment([m(r.left_top()), m(r.right_bottom())], stroke);
            painter.line_segment([m(r.right_top()), m(r.left_bottom())], stroke);
        }
        ChromeBtn::Maximize => {
            painter.add(egui::Shape::closed_line(square(r), stroke));
        }
        ChromeBtn::Restore => {
            let inset = 2.5_f32;
            let back = egui::Rect::from_min_max(
                egui::pos2(r.left() + inset, r.top()),
                egui::pos2(r.right(), r.bottom() - inset),
            );
            let front = egui::Rect::from_min_max(
                egui::pos2(r.left(), r.top() + inset),
                egui::pos2(r.right() - inset, r.bottom()),
            );
            painter.add(egui::Shape::closed_line(square(back), stroke));
            painter.add(egui::Shape::convex_polygon(
                square(front),
                crate::theme::bg(),
                stroke,
            ));
        }
        ChromeBtn::Minimize => {
            let y = r.center().y;
            painter.line_segment(
                [m(egui::pos2(r.left(), y)), m(egui::pos2(r.right(), y))],
                stroke,
            );
        }
        ChromeBtn::Menu => {
            let y0 = r.top() + 1.0;
            let y1 = r.center().y;
            let y2 = r.bottom() - 1.0;
            painter.line_segment(
                [m(egui::pos2(r.left(), y0)), m(egui::pos2(r.right(), y0))],
                stroke,
            );
            painter.line_segment(
                [m(egui::pos2(r.left(), y1)), m(egui::pos2(r.right(), y1))],
                stroke,
            );
            painter.line_segment(
                [m(egui::pos2(r.left(), y2)), m(egui::pos2(r.right(), y2))],
                stroke,
            );
        }
    }
}

/// Hover name for a titlebar chrome button. Close hides when the tray owns ×.
pub fn titlebar_chrome_tip(kind: ChromeBtn, hide_on_close: bool) -> &'static str {
    match kind {
        ChromeBtn::Minimize => "Minimize",
        ChromeBtn::Maximize => "Maximize",
        ChromeBtn::Restore => "Restore",
        ChromeBtn::Close if hide_on_close => "Hide to tray",
        ChromeBtn::Close => "Close",
        ChromeBtn::Menu => "Session menu",
    }
}

/// How far in from the window edge a resize grab reaches, and how long the
/// corner grabs run along each side.
pub const EDGE_GRIP: f32 = 5.0;
const CORNER_GRIP: f32 = 12.0;

/// The undecorated cabin has no OS frame to drag, so these strips along its
/// edges and corners start the OS resize instead.
pub fn edge_grips(screen: egui::Rect) -> [(egui::ResizeDirection, egui::Rect); 8] {
    use egui::ResizeDirection as D;
    let (l, r, t, b) = (screen.left(), screen.right(), screen.top(), screen.bottom());
    let (g, c) = (EDGE_GRIP, CORNER_GRIP);
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1))
    };
    [
        (D::NorthWest, rect(l, t, l + c, t + c)),
        (D::NorthEast, rect(r - c, t, r, t + c)),
        (D::SouthWest, rect(l, b - c, l + c, b)),
        (D::SouthEast, rect(r - c, b - c, r, b)),
        (D::North, rect(l + c, t, r - c, t + g)),
        (D::South, rect(l + c, b - g, r - c, b)),
        (D::West, rect(l, t + c, l + g, b - c)),
        (D::East, rect(r - g, t + c, r, b - c)),
    ]
}

fn resize_cursor(dir: egui::ResizeDirection) -> egui::CursorIcon {
    use egui::{CursorIcon as C, ResizeDirection as D};
    match dir {
        D::North => C::ResizeNorth,
        D::South => C::ResizeSouth,
        D::East => C::ResizeEast,
        D::West => C::ResizeWest,
        D::NorthEast => C::ResizeNorthEast,
        D::NorthWest => C::ResizeNorthWest,
        D::SouthEast => C::ResizeSouthEast,
        D::SouthWest => C::ResizeSouthWest,
    }
}

/// Grabs along the window edges that resize it. Skipped while maximized, when
/// the edges sit against the screen and a drag there should not shrink it.
pub fn edge_resize(ctx: &egui::Context, maximized: bool) {
    if maximized {
        return;
    }
    for (dir, rect) in edge_grips(ctx.content_rect()) {
        egui::Area::new(egui::Id::new(("edge-resize", format!("{dir:?}"))))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                let (_, resp) = ui.allocate_exact_size(rect.size(), egui::Sense::drag());
                let resp = resp.on_hover_cursor(resize_cursor(dir));
                if resp.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titlebar_close_is_a_real_hit() {
        let s = titlebar_chrome_size();
        assert!(s.x >= 40.0, "close hit {s:?}");
        assert_eq!(s.y, crate::theme::TITLEBAR_H);
    }

    #[test]
    fn titlebar_close_fires_after_a_held_press() {
        assert!(chrome_activated(true, false), "a normal click still closes");
        assert!(
            chrome_activated(false, true),
            "egui drops clicks held longer than 0.8s — drag_stopped on × must still hide to tray"
        );
        assert!(!chrome_activated(false, false));
    }

    #[test]
    fn titlebar_body_starts_a_window_drag() {
        assert!(titlebar_should_start_drag(true));
        assert!(!titlebar_should_start_drag(false));
    }

    #[test]
    fn titlebar_chrome_tips_name_each_button() {
        assert_eq!(titlebar_chrome_tip(ChromeBtn::Minimize, false), "Minimize");
        assert_eq!(titlebar_chrome_tip(ChromeBtn::Maximize, false), "Maximize");
        assert_eq!(titlebar_chrome_tip(ChromeBtn::Restore, false), "Restore");
        assert_eq!(titlebar_chrome_tip(ChromeBtn::Close, false), "Close");
        assert_eq!(titlebar_chrome_tip(ChromeBtn::Close, true), "Hide to tray");
        assert_eq!(titlebar_chrome_tip(ChromeBtn::Menu, false), "Session menu");
        assert_eq!(
            titlebar_chrome_tip(ChromeBtn::Menu, true),
            "Session menu",
            "only Close changes when the tray owns the window"
        );
    }

    #[test]
    fn titlebar_menu_is_three_bars() {
        let src = include_str!("titlebar.rs");
        let menu = src.split("ChromeBtn::Menu =>").nth(1).expect("menu glyph");
        let arm = menu.split("\n    }").next().unwrap_or(menu);
        let bars = arm.matches("painter.line_segment").count();
        assert_eq!(
            bars, 3,
            "session menu must be a 3-bar hamburger, got {bars} strokes: {arm}"
        );
    }

    #[test]
    fn titlebar_chrome_paints_strokes_not_glyphs() {
        let src = include_str!("titlebar.rs");
        assert!(
            src.contains("paint_chrome_glyph") && src.contains("line_segment"),
            "window chrome must be strokes, not 16px letterforms"
        );
        assert!(
            src.contains("from_center_size") && src.contains("ChromeBtn::Restore"),
            "40px titlebar must keep square ×/□/restore, not shrink() a tall hit: {src}"
        );
    }

    #[test]
    fn windows_hide_to_tray_does_not_sw_hide() {
        let src = include_str!("titlebar.rs");
        assert!(
            src.contains("cfg(not(windows))") && src.contains("Visible(w.visible)"),
            "Windows must cloak + leave the taskbar, not winit Visible(false)/SW_HIDE, or tray Quit never runs: {src}"
        );
        assert!(
            src.contains("cfg(windows)") && src.contains("Visible(true)"),
            "Show from tray must still map the cloaked cabin: {src}"
        );
    }

    #[test]
    fn the_window_edges_and_corners_each_resize_their_own_way() {
        use egui::ResizeDirection as D;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let at = |x: f32, y: f32| {
            let hits: Vec<_> = edge_grips(screen)
                .into_iter()
                .filter(|(_, r)| r.contains(egui::pos2(x, y)))
                .map(|(d, _)| d)
                .collect();
            hits
        };
        assert_eq!(at(2.0, 2.0), vec![D::NorthWest]);
        assert_eq!(at(998.0, 698.0), vec![D::SouthEast]);
        assert_eq!(at(500.0, 2.0), vec![D::North]);
        assert_eq!(at(500.0, 698.0), vec![D::South]);
        assert_eq!(at(2.0, 350.0), vec![D::West]);
        assert_eq!(at(998.0, 350.0), vec![D::East]);
        assert!(at(500.0, 20.0).is_empty(), "the titlebar body still drags");
        assert!(at(500.0, 350.0).is_empty());
    }
}
