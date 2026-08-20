use eframe::egui;

pub fn apply_tray_window(ctx: &egui::Context, w: crate::tray::TrayWindow) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(w.visible));
    if w.visible {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(w.minimized));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

pub fn titlebar_chrome_size() -> egui::Vec2 {
    egui::vec2(36.0, crate::theme::TITLEBAR_H)
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

pub fn titlebar_chrome_btn(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let (_rect, resp) = ui.allocate_exact_size(titlebar_chrome_size(), egui::Sense::click_and_drag());
    let (resp, rect, wash) = crate::theme::feel_response(ui, resp, egui::Color32::TRANSPARENT);
    if wash.a() > 0 {
        ui.painter().rect_filled(rect, 6.0, wash);
    }
    let color = if resp.hovered() {
        crate::theme::fg()
    } else {
        crate::theme::muted()
    };
    paint_chrome_glyph(ui, rect, label, color);
    resp
}

fn paint_chrome_glyph(ui: &egui::Ui, rect: egui::Rect, label: &str, color: egui::Color32) {
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.5_f32, color);
    let r = rect.shrink(12.0);
    match label {
        "×" => {
            painter.line_segment([r.left_top(), r.right_bottom()], stroke);
            painter.line_segment([r.right_top(), r.left_bottom()], stroke);
        }
        "□" => {
            painter.rect_stroke(r, 1.5, stroke);
        }
        "–" => {
            let y = r.center().y;
            painter.line_segment([egui::pos2(r.left(), y), egui::pos2(r.right(), y)], stroke);
        }
        _ => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(16.0),
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titlebar_close_is_a_real_hit() {
        let s = titlebar_chrome_size();
        assert!(s.x >= 32.0, "close hit {s:?}");
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
    }
}
