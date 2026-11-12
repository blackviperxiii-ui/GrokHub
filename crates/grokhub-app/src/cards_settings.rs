//! Settings' own big buttons: the category row and the way out. Kept out of
//! cards.rs to hold its line budget.

use eframe::egui::{self, Color32, FontId, RichText, Sense, Stroke};

/// A big category button in Settings' top row: larger than a [`felt_tab`],
/// so the categories read at a glance.
pub fn settings_tab(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let font = FontId::proportional(crate::theme::TYPE_TITLE);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER));
    let size = egui::vec2((galley.size().x + 36.0).max(96.0), 40.0);
    let (_rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let on_t = crate::theme::animate_selection(ui, resp.id.with("settings-tab"), active);
    let (resp, rect, ink) = if crate::theme::is_loadout_paint() {
        use crate::ui_kit::recipe::{look, paint, state, Part};
        let (resp, rect, _) = crate::theme::feel_button(ui, resp, Color32::TRANSPARENT);
        let look = look(Part::Tab, state(resp.hovered(), active, false));
        paint(ui.painter(), rect, &look);
        (resp, rect, look.ink)
    } else {
        let fill = crate::theme::blend_color(Color32::TRANSPARENT, crate::theme::fg(), on_t);
        let ink = crate::theme::blend_color(crate::theme::muted(), crate::theme::bg(), on_t);
        let stroke = crate::theme::blend_color(crate::theme::border(), crate::theme::fg(), on_t);
        let (resp, rect, fill) = crate::theme::feel_button(ui, resp, fill);
        ui.painter().rect_filled(rect, 10.0, fill);
        ui.painter().rect_stroke(
            rect,
            10.0,
            Stroke::new(1.0_f32, stroke),
            egui::StrokeKind::Middle,
        );
        (resp, rect, ink)
    };
    ui.painter()
        .galley(rect.center() - galley.size() * 0.5, galley, ink);
    let enabled = resp.enabled();
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, active, label));
    resp.clicked()
}

/// Settings' way out: a large, labelled button ("← Back to Chat"), not a
/// small ×.
pub fn settings_back(ui: &mut egui::Ui, label: &str) -> bool {
    let id = Some(egui::Id::unique("settings-back"));
    let size = egui::vec2(150.0, 40.0);
    if crate::theme::is_loadout_paint() {
        return crate::ui_kit::recipe::button(ui, id, label, true, size, true).clicked();
    }
    crate::theme::felt_label_button_with(
        ui,
        id,
        label,
        crate::theme::fg(),
        crate::theme::bg(),
        10.0,
        size,
        None,
        true,
    )
    .clicked()
}

/// A muted line under a Settings row.
pub fn settings_note(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(crate::theme::TYPE_BODY)
            .color(crate::theme::muted()),
    );
    ui.add_space(8.0);
}
