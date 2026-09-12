use eframe::egui;

pub fn apply_tray_window(ctx: &egui::Context, w: crate::tray::TrayWindow) {
    // winit Visible(false) is SW_HIDE on Windows — that freezes egui timers
    // so tray Quit / Show never run. Linux still unmaps; Windows cloaks.
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
    let (_rect, resp) = ui.allocate_exact_size(titlebar_chrome_size(), egui::Sense::click_and_drag());
    let (resp, rect, wash) = crate::theme::feel_response(ui, resp, egui::Color32::TRANSPARENT);
    if wash.a() > 0 {
        ui.painter().rect_filled(rect, 0.0, wash);
    }
    let color = if resp.hovered() {
        crate::theme::fg()
    } else {
        crate::theme::muted()
    };
    paint_chrome_glyph(ui, rect, kind, color);
    resp
}

fn paint_chrome_glyph(ui: &egui::Ui, rect: egui::Rect, kind: ChromeBtn, color: egui::Color32) {
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.25_f32, color);
    let side = 10.0_f32;
    let r = egui::Rect::from_center_size(rect.center(), egui::vec2(side, side));
    match kind {
        ChromeBtn::Close => {
            painter.line_segment([r.left_top(), r.right_bottom()], stroke);
            painter.line_segment([r.right_top(), r.left_bottom()], stroke);
        }
        ChromeBtn::Maximize => {
            painter.rect_stroke(r, 0.0, stroke);
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
            painter.rect_stroke(back, 0.0, stroke);
            painter.rect_filled(front, 0.0, crate::theme::bg());
            painter.rect_stroke(front, 0.0, stroke);
        }
        ChromeBtn::Minimize => {
            let y = r.center().y;
            painter.line_segment([egui::pos2(r.left(), y), egui::pos2(r.right(), y)], stroke);
        }
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
            "Windows must cloak, not winit Visible(false)/SW_HIDE, or tray Quit never runs: {src}"
        );
        assert!(
            src.contains("cfg(windows)") && src.contains("Visible(true)"),
            "Show from tray must still map the cloaked cabin: {src}"
        );
    }
}
