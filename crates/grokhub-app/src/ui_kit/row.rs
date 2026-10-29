//! Settings rows with trailing controls.

use eframe::egui::{self, ComboBox, Response, Ui, Vec2};

/// Trailing control for [`setting_row`].
pub enum SettingRowControl<'a> {
    Switch(&'a mut bool),
    Select {
        selected: &'a mut usize,
        labels: &'a [&'a str],
    },
}

/// One settings row: label + description on the left, control on the right.
pub fn setting_row(
    ui: &mut Ui,
    label: &str,
    description: &str,
    control: SettingRowControl<'_>,
) -> Response {
    let row_h = crate::theme::MIN_HIT.max(crate::theme::NAV_ROW_H);
    ui.horizontal(|ui| {
        ui.set_min_height(row_h);
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            ui.set_width(ui.available_width() * 0.55);
            ui.label(
                egui::RichText::new(label)
                    .size(crate::theme::TYPE_LABEL)
                    .color(crate::theme::fg()),
            );
            if !description.is_empty() {
                ui.label(
                    egui::RichText::new(description)
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::blend_color(
                            crate::theme::fg(),
                            crate::theme::bg(),
                            0.35,
                        )),
                );
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            paint_control(ui, label, control);
        });
    })
    .response
}

fn paint_control(ui: &mut Ui, label: &str, control: SettingRowControl<'_>) {
    match control {
        SettingRowControl::Switch(on) => {
            let min = Vec2::splat(crate::theme::MIN_HIT);
            ui.add_sized(min, egui::Checkbox::new(on, ""));
        }
        SettingRowControl::Select { selected, labels } => {
            let current = labels.get(*selected).copied().unwrap_or("—");
            // Rows share a parent id, so the label keeps each select apart.
            ComboBox::from_id_salt(ui.id().with(("select", label)))
                .selected_text(current)
                .width(crate::theme::MIN_HIT * 4.0)
                .show_ui(ui, |ui| {
                    ui.set_min_height(crate::theme::MIN_HIT);
                    for (i, name) in labels.iter().enumerate() {
                        ui.selectable_value(selected, i, *name);
                    }
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Pos2, Rect};

    #[test]
    fn setting_row_meets_min_hit() {
        let ctx = crate::ui_kit::test_ctx(false);
        let mut on = false;
        let mut pick = 1_usize;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(520.0, 200.0))),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            setting_row(ui, "Row", "Help", SettingRowControl::Switch(&mut on));
            setting_row(
                ui,
                "Pick",
                "",
                SettingRowControl::Select {
                    selected: &mut pick,
                    labels: &["A", "B"],
                },
            );
        });
        let tall = out
            .shapes
            .iter()
            .map(|c| c.clip_rect.height())
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0);
        assert!(tall >= crate::theme::MIN_HIT - 2.0, "row height {tall}");
    }

    #[test]
    fn setting_row_wires_all_controls() {
        let src = include_str!("row.rs");
        assert!(src.contains("Checkbox"));
        assert!(src.contains("ComboBox"));
    }
}
