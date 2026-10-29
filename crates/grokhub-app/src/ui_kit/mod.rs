//! GrokHub UI kit — shared settings, catalog, feed, board, and panel primitives
//! (light + dark). Callers import from here; keep new shared chrome in this module.

mod card;
mod chip;
pub mod recipe;
mod row;
mod source_chips;

pub use card::{card_frame, CardVariant};
pub use chip::{badge, chip, BadgeTone};
pub use row::{setting_row, SettingRowControl};
pub use source_chips::{source_chips, source_chips_width};

use eframe::egui::{self, RichText, Ui, Vec2};

/// Extra room above a kit section header (matches settings group rhythm).
pub const SECTION_HEADER_GAP: f32 = crate::theme::SPACING_12;

/// Section title for grouped settings. Muted optional description sits underneath.
pub fn section_header(ui: &mut Ui, title: &str, description: Option<&str>) {
    ui.add_space(SECTION_HEADER_GAP);
    ui.label(
        RichText::new(title)
            .font(crate::theme::title_font(crate::theme::TYPE_TITLE))
            .color(crate::theme::fg()),
    );
    if let Some(desc) = description {
        ui.add_space(crate::theme::SPACING_4);
        ui.label(RichText::new(desc).size(crate::theme::TYPE_BODY).color(
            crate::theme::blend_color(crate::theme::fg(), crate::theme::bg(), 0.35),
        ));
    }
    ui.add_space(crate::theme::SPACING_8);
}

/// Centered empty state with optional action button.
pub fn empty_state(ui: &mut Ui, title: &str, message: &str, action_label: Option<&str>) -> bool {
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.add_space(crate::theme::SPACING_24);
        ui.label(
            RichText::new(title)
                .font(crate::theme::title_font(crate::theme::TYPE_HEADLINE))
                .color(crate::theme::fg()),
        );
        ui.add_space(crate::theme::SPACING_8);
        ui.label(RichText::new(message).size(crate::theme::TYPE_BODY).color(
            crate::theme::blend_color(crate::theme::fg(), crate::theme::bg(), 0.35),
        ));
        if let Some(label) = action_label {
            ui.add_space(crate::theme::SPACING_16);
            let min = Vec2::new(crate::theme::MIN_HIT * 2.0, crate::theme::MIN_HIT);
            clicked = crate::theme::felt_label_button(
                ui,
                label,
                crate::theme::surface(),
                crate::theme::fg(),
                crate::theme::RADIUS_MD,
                min,
                Some(egui::Stroke::new(1.0, crate::theme::border())),
                false,
            )
            .clicked();
        }
        ui.add_space(crate::theme::SPACING_24);
    });
    clicked
}

/// Paint every kit piece for snapshot tests and design review.
#[cfg(test)]
pub fn paint_kit_gallery(ui: &mut Ui) {
    ui.set_width(420.0);
    section_header(ui, "Appearance", Some("Shared primitives"));
    for variant in [
        CardVariant::Default,
        CardVariant::Subtle,
        CardVariant::Outlined,
        CardVariant::Interactive,
    ] {
        card_frame(ui, variant, true, |ui| {
            ui.label(format!("Card — {:?}", variant));
        });
        ui.add_space(crate::theme::SPACING_8);
    }
    ui.horizontal(|ui| {
        badge(ui, "Beta", BadgeTone::Accent);
        ui.add_space(crate::theme::SPACING_8);
        badge(ui, "Offline", BadgeTone::Warning);
        ui.add_space(crate::theme::SPACING_8);
        chip(ui, "Filters", false, true);
        ui.add_space(crate::theme::SPACING_8);
        chip(ui, "Disabled", true, false);
    });
    ui.add_space(crate::theme::SPACING_12);
    let mut on = true;
    let mut pick = 0_usize;
    setting_row(
        ui,
        "Notifications",
        "Desktop alerts for agent replies.",
        SettingRowControl::Switch(&mut on),
    );
    setting_row(
        ui,
        "Theme",
        "Follow system or stay fixed.",
        SettingRowControl::Select {
            selected: &mut pick,
            labels: &["System", "Dark", "Light"],
        },
    );
    ui.add_space(crate::theme::SPACING_16);
    let _ = empty_state(ui, "Nothing here", "Try another filter.", Some("Reset"));
}

#[cfg(test)]
pub(crate) fn test_ctx(dark: bool) -> egui::Context {
    use crate::theme::{apply, install_fonts_on};
    let ctx = egui::Context::default();
    install_fonts_on(&ctx);
    apply(&ctx, dark);
    ctx.all_styles_mut(|s| s.animation_time = 0.0);
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{contrast_ratio, fg, hold_paint_test, set_paint_dark};
    use eframe::egui::{self, Pos2, Rect};

    fn gallery_shapes(dark: bool) -> Vec<egui::epaint::ClippedShape> {
        let ctx = test_ctx(dark);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(480.0, 900.0))),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, paint_kit_gallery);
        });
        out.shapes
    }

    #[test]
    fn section_header_and_empty_state_paint_copy() {
        let shapes = gallery_shapes(true);
        let text: Vec<String> = shapes
            .iter()
            .filter_map(|c| {
                if let egui::Shape::Text(t) = &c.shape {
                    Some(t.galley.text().to_string())
                } else {
                    None
                }
            })
            .collect();
        assert!(text.iter().any(|t| t.contains("Appearance")));
        assert!(text.iter().any(|t| t.contains("Nothing here")));
        assert!(text.iter().any(|t| t.contains("Notifications")));
    }

    #[test]
    fn kit_text_contrast_in_both_themes() {
        let _paint = hold_paint_test();
        for dark in [true, false] {
            set_paint_dark(dark);
            let canvas = if dark {
                crate::theme::BG
            } else {
                crate::theme::LIGHT_BG
            };
            assert!(
                contrast_ratio(fg(), canvas) >= 4.5,
                "primary kit copy on canvas dark={dark}"
            );
        }
    }

    #[test]
    fn interactive_card_wires_focus_ring() {
        let card = include_str!("card.rs");
        assert!(
            card.contains("Sense::click()") && card.contains("paint_focus_ring"),
            "interactive cards must be clickable and paint a keyboard ring"
        );
        let theme = include_str!("../theme.rs");
        assert!(
            theme.contains("pub fn paint_focus_ring") && theme.contains("link()"),
            "focus ring uses the link stroke"
        );
    }

    #[test]
    fn gallery_module_wires_every_piece() {
        let src = include_str!("mod.rs");
        assert!(src.contains("paint_kit_gallery"));
        assert!(src.contains("section_header"));
        assert!(src.contains("empty_state"));
        assert!(src.contains("card_frame"));
        assert!(src.contains("badge"));
        assert!(src.contains("chip"));
        assert!(src.contains("setting_row"));
    }
}
