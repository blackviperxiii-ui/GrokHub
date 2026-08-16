use eframe::egui;

pub fn apply_tray_window(ctx: &egui::Context, w: crate::tray::TrayWindow) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(w.visible));
    if w.visible {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(w.minimized));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

pub fn titlebar_chrome_size() -> egui::Vec2 {
    egui::vec2(crate::theme::HIT.max(36.0), crate::theme::TITLEBAR_H)
}

/// egui ignores a click held longer than 0.8s (`max_click_duration`). The
/// titlebar × is a close control — release over it must still hide to tray.
pub fn chrome_activated(clicked: bool, released_over: bool) -> bool {
    clicked || released_over
}

pub fn titlebar_chrome_hit(resp: &egui::Response) -> bool {
    chrome_activated(
        resp.clicked(),
        resp.contains_pointer() && resp.ctx.input(|i| i.pointer.primary_released()),
    )
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
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(16.0),
        color,
    );
    resp
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
            "egui drops clicks held longer than 0.8s — titlebar × must still hide to tray"
        );
        assert!(!chrome_activated(false, false));
    }
}
