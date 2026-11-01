//! The old pages and tiles are on the one type scale and the Loadout card look (LO-12).

/// Sizes written as numbers on the old pages (Command, Queue, Devices,
/// Memory, History, Journal, Night), not as steps of the one type scale.
fn raw_sizes(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        for call in [".size(", "title_font(", "FontId::proportional("] {
            let mut rest = line;
            while let Some(at) = rest.find(call) {
                rest = &rest[at + call.len()..];
                if rest.starts_with(|c: char| c.is_ascii_digit()) {
                    out.push(format!("{}: {}", i + 1, line.trim()));
                }
            }
        }
        if line.contains("Margin::same(24)") {
            out.push(format!("{}: {}", i + 1, line.trim()));
        }
    }
    out
}

#[test]
fn old_pages_take_sizes_and_margins_from_the_one_scale() {
    for (name, src) in [
        ("pages.rs", include_str!("app/pages.rs")),
        ("journal_ui.rs", include_str!("app/journal_ui.rs")),
        ("night.rs", include_str!("app/night.rs")),
    ] {
        let raw = raw_sizes(src);
        assert!(raw.is_empty(), "{name} sizes text by number: {raw:#?}");
    }
    // The page title is the kit headline, not the old 28pt title.
    let header = include_str!("cards.rs")
        .split("pub fn page_header(")
        .nth(1)
        .and_then(|s| s.split("pub fn white_pill(").next())
        .expect("page_header");
    assert!(header.contains("TYPE_HEADLINE"), "{header}");
    assert!(raw_sizes(header).is_empty(), "{header}");
    // The scanner itself catches a numbered size.
    assert_eq!(
        raw_sizes("x.size(12.0)\ny.size(crate::theme::TYPE_BODY)").len(),
        1
    );
}

#[test]
fn tiles_wear_the_loadout_card_rim() {
    let src = include_str!("cards.rs");
    let slot = src
        .split("fn paint_slot_card(")
        .nth(1)
        .and_then(|s| s.split("/// Description lines reserved").next())
        .expect("paint_slot_card");
    assert!(
        slot.contains("recipe::look") && slot.contains("Part::Card"),
        "tile rims come from the Loadout card recipe: {slot}"
    );
    let frame = src
        .split("fn tile_frame()")
        .nth(1)
        .and_then(|s| s.split("fn paint_slot_card(").next())
        .expect("tile_frame");
    assert!(frame.contains("Part::Card"), "{frame}");
    let grok = src
        .split("pub fn grok_tile(")
        .nth(1)
        .and_then(|s| s.split("pub fn tile_row(").next())
        .expect("grok_tile");
    assert!(grok.contains("tile_frame()"), "{grok}");
}
