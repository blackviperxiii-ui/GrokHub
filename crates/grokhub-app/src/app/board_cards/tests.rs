use super::tokens::*;
use crate::theme::contrast_ratio;
use eframe::egui::Color32;

#[test]
fn dark_tokens_match_the_spec() {
    let t = card_tokens(true);
    assert_eq!(t.surface, Color32::from_rgb(0x16, 0x18, 0x1c));
    assert_eq!(t.hover, Color32::from_rgb(0x1c, 0x1f, 0x23));
    assert_eq!(t.border, Color32::from_rgb(0x2f, 0x33, 0x36));
    assert_eq!(t.title, Color32::from_rgb(0xe7, 0xe9, 0xea));
    assert_eq!(t.desc, Color32::from_rgb(0x8b, 0x90, 0x96));
    assert_eq!(t.meta, Color32::from_rgb(0x71, 0x76, 0x7b));
    assert_eq!(t.live, Color32::from_rgb(0x22, 0xc5, 0x5e));
    assert_eq!(t.warn, Color32::from_rgb(0xea, 0xb3, 0x08));
    assert_eq!(t.danger, Color32::from_rgb(0xf4, 0x21, 0x2e));
}

#[test]
fn light_tokens_match_the_spec() {
    let t = card_tokens(false);
    assert_eq!(t.surface, Color32::from_rgb(0xff, 0xff, 0xff));
    assert_eq!(t.hover, Color32::from_rgb(0xf4, 0xf4, 0xf5));
    assert_eq!(t.border, Color32::from_rgb(0xe4, 0xe4, 0xe7));
    assert_eq!(t.title, Color32::from_rgb(0x0f, 0x14, 0x19));
    assert_eq!(t.desc, Color32::from_rgb(0x53, 0x64, 0x71));
    assert_eq!(t.meta, Color32::from_rgb(0x71, 0x76, 0x7b));
    assert_eq!(t.live, Color32::from_rgb(0x15, 0x80, 0x3d));
    assert_eq!(t.warn, Color32::from_rgb(0xa1, 0x62, 0x07));
    assert_eq!(t.danger, Color32::from_rgb(0xb9, 0x1c, 0x1c));
}

#[test]
fn modes_differ_in_every_surface_and_text_token() {
    let (d, l) = (card_tokens(true), card_tokens(false));
    assert_ne!(d.surface, l.surface);
    assert_ne!(d.hover, l.hover);
    assert_ne!(d.border, l.border);
    assert_ne!(d.title, l.title);
    assert_ne!(d.desc, l.desc);
}

#[test]
fn title_and_desc_are_readable_on_the_card_in_both_modes() {
    for dark in [true, false] {
        let t = card_tokens(dark);
        for bg in [t.surface, t.hover] {
            assert!(contrast_ratio(t.title, bg) >= 4.5, "title dark={dark}");
            assert!(contrast_ratio(t.desc, bg) >= 4.5, "desc dark={dark}");
        }
    }
}

#[test]
fn spec_timings_and_geometry() {
    assert_eq!(
        (COMPACT_H, CARD_RADIUS, PILL_H, ACTION_H),
        (56.0, 12.0, 36.0, 28.0)
    );
    assert_eq!(
        (EXPAND_SECS, COLLAPSE_SECS, EXPAND_FADE_DELAY_SECS),
        (0.22, 0.18, 0.06)
    );
    assert_eq!((HOVER_FILL_SECS, ACTION_MORPH_SECS), (0.12, 0.16));
    assert_eq!(
        (DRAG_SHRINK_SECS, DRAG_RESTORE_SECS, DRAG_SCALE),
        (0.12, 0.14, 0.97)
    );
    assert_eq!(
        (
            EMPTY_FADE_IN_SECS,
            EMPTY_FADE_DELAY_SECS,
            EMPTY_FADE_OUT_SECS
        ),
        (0.24, 0.15, 0.12)
    );
    assert_eq!(
        (
            LAYOUT_SWITCH_SECS,
            LAYOUT_STAGGER_SECS,
            LAYOUT_STAGGER_MAX_SECS
        ),
        (0.26, 0.02, 0.2)
    );
}
