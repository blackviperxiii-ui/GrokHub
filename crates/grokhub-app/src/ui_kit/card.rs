//! Card frame primitives for settings and catalog surfaces.

use eframe::egui::{Color32, Frame, Margin, Response, Sense, Stroke, Ui};

use super::recipe::{Part, State};

/// Visual treatment for [`card_frame`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardVariant {
    Default,
    Subtle,
    Outlined,
    Interactive,
    /// Loadout card: raised fill with a dim red rim.
    Signal,
    /// Loadout card, hovered or focused: lifted fill with a signal-red rim.
    SignalHot,
}

/// The rim each variant paints.
fn card_stroke(variant: CardVariant) -> Stroke {
    match variant {
        CardVariant::Outlined => Stroke::new(1.0, crate::theme::border()),
        CardVariant::Signal => super::recipe::look(Part::Card, State::Default).stroke,
        CardVariant::SignalHot => super::recipe::look(Part::Card, State::Hover).stroke,
        CardVariant::Default | CardVariant::Subtle | CardVariant::Interactive => Stroke::NONE,
    }
}

/// Paint a rounded card with kit spacing, shadow, and interaction states.
pub fn card_frame(
    ui: &mut Ui,
    variant: CardVariant,
    enabled: bool,
    add_contents: impl FnOnce(&mut Ui),
) -> Response {
    let base_fill = card_fill(variant, enabled);
    let stroke = card_stroke(variant);
    let shadow = match variant {
        CardVariant::Default => crate::theme::kit_shadow(crate::theme::ShadowLevel::Lift),
        CardVariant::Interactive => crate::theme::kit_shadow(crate::theme::ShadowLevel::Soft),
        CardVariant::Subtle
        | CardVariant::Outlined
        | CardVariant::Signal
        | CardVariant::SignalHot => crate::theme::kit_shadow(crate::theme::ShadowLevel::None),
    };
    let radius = match variant {
        CardVariant::Signal | CardVariant::SignalHot => super::recipe::CARD_RADIUS,
        _ => crate::theme::RADIUS_LG,
    };
    let mut prepared = Frame {
        fill: base_fill,
        stroke,
        shadow,
        inner_margin: Margin::same(12),
        outer_margin: Margin::ZERO,
        corner_radius: radius.into(),
    }
    .begin(ui);
    {
        let ui = &mut prepared.content_ui;
        ui.set_min_height(crate::theme::MIN_HIT);
        ui.add_enabled_ui(enabled, add_contents);
    }
    let sense = if variant == CardVariant::Interactive && enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = prepared.allocate_space(ui).interact(sense);
    let resting = prepared.frame.fill;
    let (resp, slot, veil) = if enabled {
        crate::theme::feel_response_in_slot(ui, resp, Color32::TRANSPARENT)
    } else {
        let rect = resp.rect;
        (resp, rect, disabled_fill(resting))
    };
    prepared.frame.fill = if enabled {
        crate::theme::veil_over(resting, veil)
    } else {
        disabled_fill(resting)
    };
    prepared.frame.stroke = card_stroke(variant);
    prepared.paint(ui);
    crate::theme::paint_focus_ring(ui, slot, radius, &resp);
    resp
}

fn card_fill(variant: CardVariant, enabled: bool) -> Color32 {
    let base = match variant {
        CardVariant::Default | CardVariant::Interactive => crate::theme::elevated(),
        CardVariant::Subtle => crate::theme::surface(),
        CardVariant::Outlined => crate::theme::elevated(),
        CardVariant::Signal => super::recipe::look(Part::Card, State::Default).fill,
        CardVariant::SignalHot => super::recipe::look(Part::Card, State::Hover).fill,
    };
    if enabled {
        base
    } else {
        disabled_fill(base)
    }
}

fn disabled_fill(base: Color32) -> Color32 {
    crate::theme::blend_color(base, crate::theme::bg(), 0.35)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Pos2, Rect};

    #[test]
    fn card_variants_allocate_minimum_height() {
        for variant in [
            CardVariant::Default,
            CardVariant::Subtle,
            CardVariant::Outlined,
            CardVariant::Interactive,
            CardVariant::Signal,
            CardVariant::SignalHot,
        ] {
            let ctx = crate::ui_kit::test_ctx(true);
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(320.0, 240.0))),
                ..Default::default()
            };
            let out = crate::theme::test_pass(&ctx, input, |ui| {
                card_frame(ui, variant, true, |ui| {
                    ui.label(format!("{variant:?}"));
                });
            });
            let max_h = out
                .shapes
                .iter()
                .map(|c| c.clip_rect.height())
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(0.0);
            assert!(
                max_h >= crate::theme::MIN_HIT - 1.0,
                "{variant:?} height {max_h}"
            );
        }
    }

    #[test]
    fn signal_cards_paint_the_loadout_rims() {
        let _paint = crate::theme::hold_paint_test();
        crate::theme::set_paint_palette(crate::theme::PaintPalette::OpsDark);
        assert_eq!(
            card_stroke(CardVariant::Signal),
            Stroke::new(1.5, crate::theme::OPS_SIGNAL_RED)
        );
        assert_eq!(
            card_stroke(CardVariant::SignalHot),
            Stroke::new(1.5, crate::theme::OPS_SIGNAL_RED)
        );
        assert_eq!(
            card_fill(CardVariant::Signal, true),
            crate::theme::OPS_RAISED
        );
        assert_eq!(
            card_fill(CardVariant::SignalHot, true),
            crate::theme::OPS_HOVER
        );
        assert_eq!(card_stroke(CardVariant::Default), Stroke::NONE);
    }

    #[test]
    fn disabled_card_uses_muted_fill() {
        let enabled = card_fill(CardVariant::Default, true);
        let disabled = card_fill(CardVariant::Default, false);
        assert_ne!(enabled, disabled);
    }

    #[test]
    fn card_frame_source_covers_states() {
        let src = include_str!("card.rs");
        assert!(src.contains("feel_response_in_slot"));
        assert!(src.contains("Sense::click()"));
        assert!(src.contains("paint_focus_ring"));
        assert!(src.contains("disabled_fill"));
    }
}
