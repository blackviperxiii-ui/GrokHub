//! Badges and chips for meta labels and filters.

use eframe::egui::{self, Color32, FontId, Response, Sense, Stroke, Ui, Vec2};

/// Badge color intent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeTone {
    Neutral,
    Accent,
    Warning,
}

/// Small status pill.
pub fn badge(ui: &mut Ui, label: &str, tone: BadgeTone) -> Response {
    let (fill, text) = badge_colors(tone);
    paint_pill(ui, label, fill, text, crate::theme::RADIUS_SM, false, true)
}

/// Filter / tag chip. `enabled` drives hover and focus; disabled chips are flat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipVariant {
    Default,
    Outlined,
}

pub fn chip(ui: &mut Ui, label: &str, variant: ChipVariant, enabled: bool) -> Response {
    let (fill, stroke, text) = match variant {
        ChipVariant::Default => (
            if enabled {
                crate::theme::surface()
            } else {
                crate::theme::blend_color(crate::theme::surface(), crate::theme::bg(), 0.4)
            },
            Stroke::NONE,
            crate::theme::fg(),
        ),
        ChipVariant::Outlined => (
            Color32::TRANSPARENT,
            Stroke::new(1.0, crate::theme::border()),
            crate::theme::fg(),
        ),
    };
    let id = ui.id().with(("ui-kit-chip", label));
    let font = FontId::proportional(crate::theme::TYPE_CAPTION);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, text));
    let pad = egui::vec2(crate::theme::SPACING_8, crate::theme::SPACING_4);
    let size = galley.size() + pad * 2.0;
    let min = Vec2::new(
        size.x.max(crate::theme::MIN_HIT * 0.75),
        size.y.max(crate::theme::MIN_HIT * 0.65),
    );
    let (rect, resp) = ui.allocate_exact_size(min, if enabled { Sense::click() } else { Sense::hover() });
    let resp = if enabled {
        ui.interact(rect, id, Sense::click())
    } else {
        resp
    };
    let (resp, felt, painted) = if enabled {
        crate::theme::feel_response(ui, resp, fill)
    } else {
        (resp, rect, fill)
    };
    if stroke.width > 0.0 {
        ui.painter()
            .rect_stroke(felt, crate::theme::RADIUS_MD, stroke, egui::StrokeKind::Inside);
    }
    ui.painter().rect_filled(felt, crate::theme::RADIUS_MD, painted);
    crate::theme::paint_focus_ring(ui, felt, crate::theme::RADIUS_MD, &resp);
    ui.painter().galley(
        egui::pos2(
            felt.min.x + pad.x,
            felt.center().y - galley.size().y * 0.5,
        ),
        galley,
        text,
    );
    resp
}

fn badge_colors(tone: BadgeTone) -> (Color32, Color32) {
    match tone {
        BadgeTone::Neutral => (crate::theme::surface(), crate::theme::fg()),
        BadgeTone::Accent => (crate::theme::selection(), crate::theme::fg()),
        BadgeTone::Warning => (
            crate::theme::blend_color(crate::theme::setup(), crate::theme::surface(), 0.35),
            crate::theme::fg(),
        ),
    }
}

fn paint_pill(
    ui: &mut Ui,
    label: &str,
    fill: Color32,
    text: Color32,
    radius: f32,
    interactive: bool,
    enabled: bool,
) -> Response {
    let font = FontId::proportional(crate::theme::TYPE_CAPTION);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, text));
    let pad = egui::vec2(crate::theme::SPACING_8, crate::theme::SPACING_4);
    let size = galley.size() + pad * 2.0;
    let min = Vec2::new(size.x, size.y.max(crate::theme::MIN_HIT * 0.55));
    let sense = if interactive && enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(min, sense);
    ui.painter().rect_filled(rect, radius, fill);
    ui.painter().galley(
        egui::pos2(rect.min.x + pad.x, rect.center().y - galley.size().y * 0.5),
        galley,
        text,
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{contrast_ratio, fg, hold_paint_test, set_paint_dark};
    use eframe::egui::{self, Pos2, Rect};

    #[test]
    fn badge_and_chip_paint_labels() {
        let ctx = crate::ui_kit::test_ctx(true);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(320.0, 120.0))),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            badge(ui, "New", BadgeTone::Accent);
            chip(ui, "Tag", ChipVariant::Default, true);
        });
        let text = out
            .shapes
            .iter()
            .filter_map(|c| {
                if let egui::Shape::Text(t) = &c.shape {
                    Some(t.galley.text().to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert!(text.iter().any(|t| t == "New"));
        assert!(text.iter().any(|t| t == "Tag"));
    }

    #[test]
    fn badge_text_contrast() {
        let _paint = hold_paint_test();
        for dark in [true, false] {
            set_paint_dark(dark);
            let (fill, text) = badge_colors(BadgeTone::Neutral);
            assert!(
                contrast_ratio(text, fill) >= 4.5,
                "badge contrast dark={dark}"
            );
            assert!(contrast_ratio(fg(), fill) >= 3.0);
        }
    }
}
