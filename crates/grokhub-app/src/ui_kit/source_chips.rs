//! A web-search reply's Sources as compact chips with the site name.

use eframe::egui::{self, Color32, FontId, Sense, Stroke};
use grokhub_core::reply_sources::ReplySource;

/// Inside padding of a web source chip under a reply.
const SOURCE_CHIP_PAD: egui::Vec2 = egui::vec2(8.0, 3.0);
const SOURCE_CHIP_GAP: f32 = 6.0;

fn source_chip_galley(ui: &egui::Ui, site: &str) -> std::sync::Arc<egui::Galley> {
    ui.fonts_mut(|f| {
        f.layout_no_wrap(
            site.to_owned(),
            FontId::proportional(crate::theme::TYPE_CAPTION),
            Color32::PLACEHOLDER,
        )
    })
}

/// Width of the chip row on one line, for sizing the reply bubble.
pub fn source_chips_width(ui: &egui::Ui, sources: &[ReplySource]) -> f32 {
    sources
        .iter()
        .map(|s| source_chip_galley(ui, &s.site).size().x + SOURCE_CHIP_PAD.x * 2.0)
        .sum::<f32>()
        + SOURCE_CHIP_GAP * sources.len().saturating_sub(1) as f32
}

/// A reply's web sources as compact chips with the site name. Hover shows
/// the full link; a click opens it. Returns the clicked URL.
pub fn source_chips(ui: &mut egui::Ui, sources: &[ReplySource]) -> Option<String> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(SOURCE_CHIP_GAP, SOURCE_CHIP_GAP);
        for source in sources {
            let galley = source_chip_galley(ui, &source.site);
            let (rect, resp) =
                ui.allocate_exact_size(galley.size() + SOURCE_CHIP_PAD * 2.0, Sense::click());
            let radius = rect.height() * 0.5;
            let hovered = resp.hovered();
            ui.painter().rect_filled(
                rect,
                radius,
                if hovered {
                    crate::theme::surface_hover()
                } else {
                    crate::theme::elevated()
                },
            );
            ui.painter().rect_stroke(
                rect,
                radius,
                Stroke::new(1.0_f32, crate::theme::border()),
                egui::StrokeKind::Inside,
            );
            ui.painter().galley(
                rect.min + SOURCE_CHIP_PAD,
                galley,
                if hovered {
                    crate::theme::fg()
                } else {
                    crate::theme::muted()
                },
            );
            crate::theme::paint_focus_ring(ui, rect, radius, &resp);
            if crate::theme::pointing(resp.on_hover_text(&source.url)).clicked() {
                clicked = Some(source.url.clone());
            }
        }
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_sources_paint_as_site_chips_not_raw_links() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let (_, sources) = grokhub_core::reply_sources::split_sources(
            "Rust 1.92 is out.\n\nSources:\n1. https://blog.rust-lang.org/2026/10/30/Rust-1.92.html\n2. https://www.phoronix.com/news/Rust-1.92",
        )
        .expect("sources");
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(480.0, 200.0),
            )),
            ..Default::default()
        };
        let mut width = 0.0;
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                width = source_chips_width(ui, &sources);
                source_chips(ui, &sources);
            });
        });
        let texts: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["blog.rust-lang.org", "phoronix.com"]);
        assert!(width > 0.0 && width < 260.0, "compact row: {width}");
    }
}
