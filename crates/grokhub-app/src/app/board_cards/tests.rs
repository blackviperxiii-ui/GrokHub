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

use super::compact::{
    card_desc, elapsed_label, one_line, paint_compact_card, status_icon, CompactRow, StatusIcon,
    DESC_MAX_CHARS,
};
use eframe::egui;
use grokhub_core::{BoardCard, BoardStatus};

#[test]
fn timer_reads_mm_ss_under_an_hour_then_hours_then_days() {
    assert_eq!(elapsed_label(0), "00:00");
    assert_eq!(elapsed_label(999), "00:00");
    assert_eq!(elapsed_label(247_000), "04:07");
    assert_eq!(elapsed_label(3_599_000), "59:59");
    assert_eq!(elapsed_label(3_600_000), "1h 0m");
    assert_eq!(elapsed_label(3_840_000), "1h 4m");
    assert_eq!(elapsed_label(86_399_000), "23h 59m");
    assert_eq!(elapsed_label(86_400_000), "1d");
    assert_eq!(elapsed_label(3 * 86_400_000 + 7_200_000), "3d");
}

#[test]
fn description_is_one_flat_line_cut_with_an_ellipsis() {
    assert_eq!(one_line(""), "");
    assert_eq!(
        one_line("\n  \n  Swap   the\tbulb  \nsecond line"),
        "Swap the bulb"
    );
    let long = "word ".repeat(60);
    let cut = one_line(&long);
    assert_eq!(cut.chars().count(), 140);
    assert!(cut.ends_with("word…"), "{cut}");
    let exact = "x".repeat(DESC_MAX_CHARS);
    assert_eq!(one_line(&exact), exact, "a line at the cap stays whole");

    let mut card = BoardCard::new("Nightly ledger", "", "");
    card.report = "Two invoices are overdue.\nDetails follow.".into();
    assert_eq!(card_desc(&card), "Two invoices are overdue.");
    card.detail = "Check the ledger".into();
    assert_eq!(
        card_desc(&card),
        "Check the ledger",
        "detail wins over the report"
    );
}

#[test]
fn each_status_has_its_icon() {
    assert_eq!(status_icon(BoardStatus::InProgress), StatusIcon::Spinner);
    assert_eq!(status_icon(BoardStatus::Todo), StatusIcon::Dot);
    assert_eq!(status_icon(BoardStatus::Proposed), StatusIcon::Dot);
    assert_eq!(status_icon(BoardStatus::Approved), StatusIcon::Dot);
    assert_eq!(status_icon(BoardStatus::Staged), StatusIcon::Dot);
    assert_eq!(status_icon(BoardStatus::Dismissed), StatusIcon::Dot);
    assert_eq!(status_icon(BoardStatus::Done), StatusIcon::Check);
    assert_eq!(status_icon(BoardStatus::Blocked), StatusIcon::Alert);
    assert_eq!(status_icon(BoardStatus::FollowUp), StatusIcon::Retry);
}

fn walk(shape: &egui::Shape, texts: &mut Vec<String>, fills: &mut Vec<Color32>) {
    match shape {
        egui::Shape::Text(t) => texts.push(t.galley.text().to_string()),
        egui::Shape::Rect(r) => fills.push(r.fill),
        egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, texts, fills)),
        _ => {}
    }
}

/// One headless frame of a compact card in a 360 px column.
fn render(dark: bool, row: &CompactRow) -> (egui::Rect, f32, Vec<String>, Vec<Color32>) {
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(360.0, 400.0),
        )),
        ..Default::default()
    };
    let (mut rect, mut avail) = (egui::Rect::NOTHING, 0.0);
    let out = crate::theme::test_pass(&ctx, input, |ui| {
        ui.visuals_mut().dark_mode = dark;
        avail = ui.available_width();
        rect = paint_compact_card(ui, row).rect;
    });
    let (mut texts, mut fills) = (Vec::new(), Vec::new());
    for c in &out.shapes {
        walk(&c.shape, &mut texts, &mut fills);
    }
    (rect, avail, texts, fills)
}

#[test]
fn a_compact_card_paints_one_56px_row_in_both_modes() {
    let mut card = BoardCard::new("Harbor lamp", "Swap the bulb on the north pier.", "");
    card.id = "card-a".into();
    card.status = BoardStatus::InProgress;
    card.updated_ms = 1_000_000;
    let chips = [('C', Color32::BLACK, Color32::WHITE)];
    let row = CompactRow {
        card: &card,
        model: Some("grok-4-fast"),
        live: false,
        dragging: false,
        now_ms: 1_000_000 + 247_000,
        chips: &chips,
    };
    for dark in [true, false] {
        let (rect, avail, texts, fills) = render(dark, &row);
        assert_eq!(rect.height(), 56.0, "dark={dark}");
        assert!(avail > 300.0, "a real column width: {avail}");
        assert_eq!(rect.width(), avail, "full row width, dark={dark}");
        for want in [
            "Harbor lamp",
            "04:07",
            "Swap the bulb on the north pier.",
            "grok-4-fast",
            "C",
        ] {
            assert!(
                texts.iter().any(|t| t == want),
                "{want} dark={dark}: {texts:?}"
            );
        }
        assert!(
            fills.contains(&card_tokens(dark).surface),
            "resting fill is the mode's surface token, dark={dark}: {fills:?}"
        );
    }
}

#[test]
fn a_card_with_no_chat_or_update_shows_no_model_or_timer() {
    let mut card = BoardCard::new("Tide chart", "", "");
    card.id = "card-b".into();
    let row = CompactRow {
        card: &card,
        model: None,
        live: false,
        dragging: false,
        now_ms: 5_000,
        chips: &[],
    };
    let (_, _, texts, _) = render(true, &row);
    assert_eq!(texts, vec!["Tide chart".to_string()]);
}
