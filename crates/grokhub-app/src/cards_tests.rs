//! `cards.rs` tests, moved out whole (`#[path]` keeps `cards.rs` the parent).

use super::*;

/// SY-10: a group heading gets 8–12px more room above it than a plain
/// label, at the same size.
#[test]
fn catalog_chrome_wires_ui_kit_frames() {
    let src = include_str!("cards.rs");
    assert!(
        src.contains("use crate::ui_kit::{card_frame, CardVariant}"),
        "cards must import the U1 kit"
    );
    assert!(
        src.contains("card_frame(ui, CardVariant::Outlined")
            && src.contains("kit_shadow(crate::theme::ShadowLevel::Lift)"),
        "catalog surfaces must use kit card_frame and shadow tokens"
    );
    assert!(
        src.contains("pub fn status_chip(")
            && src.contains("pub fn grok_tile(")
            && src.contains("pub fn search_bar("),
        "primary card entry points stay in cards.rs"
    );
}

#[test]
fn section_heading_adds_room_above_at_the_same_size() {
    assert!(
        (8.0..=12.0).contains(&SECTION_HEAD_GAP),
        "{SECTION_HEAD_GAP}"
    );
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let tops = |heading: bool| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(480.0, 400.0),
            )),
            ..Default::default()
        };
        let mut top = 0.0_f32;
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                top = ui.cursor().min.y;
                if heading {
                    section_heading(ui, "Command rules");
                } else {
                    section_label(ui, "Command rules");
                }
            });
        });
        let mut at = None;
        for clipped in &out.shapes {
            if let egui::Shape::Text(t) = &clipped.shape {
                if t.galley.text() == "Command rules" {
                    at = Some((t.pos.y - top, t.galley.rect.height()));
                }
            }
        }
        at.expect("heading painted")
    };
    let (plain_y, plain_h) = tops(false);
    let (head_y, head_h) = tops(true);
    assert!(
        (head_y - plain_y - SECTION_HEAD_GAP).abs() < 0.5,
        "{plain_y} -> {head_y}"
    );
    assert!(
        (head_h - plain_h).abs() < 0.01,
        "the heading size stays: {plain_h} vs {head_h}"
    );
}

#[test]
fn appearance_swatch_has_a_contrasting_outline() {
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(240.0, 160.0),
        )),
        ..Default::default()
    };
    let out = crate::theme::test_pass(&ctx, input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            appearance_card(ui, "Ops Dark", true, crate::theme::bg());
        });
    });
    assert!(out.shapes.iter().any(|shape| match &shape.shape {
        egui::Shape::Rect(rect) => {
            rect.rect.size() == egui::vec2(88.0, 56.0) && rect.stroke.color == crate::theme::fg()
        }
        _ => false,
    }));
}

/// Spike-4b: a focused Allow answers Enter/Space as a plain pill would, but
/// a grant row's Allow only counts a pointer click.
#[test]
fn grant_pill_ignores_the_keyboard_and_takes_a_pointer_click() {
    let run = |grant: bool| {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
        let pass = |events: Vec<egui::Event>| {
            let mut hit = false;
            let mut at = egui::Pos2::ZERO;
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let out = crate::theme::test_pass(&ctx, input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    hit = if grant {
                        settings_grant_row(
                            ui,
                            "Row",
                            "",
                            None,
                            GrantPill::Allow,
                            None,
                            false,
                            |_| {},
                        )
                    } else {
                        white_pill(ui, "Allow")
                    };
                });
            });
            for clipped in &out.shapes {
                if let egui::Shape::Text(t) = &clipped.shape {
                    at = t.pos + t.galley.rect.center().to_vec2();
                }
            }
            (hit, at)
        };
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let (_, at) = pass(vec![]);
        pass(vec![key(egui::Key::Tab)]);
        let by_keys = pass(vec![key(egui::Key::Enter)]).0 | pass(vec![key(egui::Key::Space)]).0;
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        pass(vec![egui::Event::PointerMoved(at)]);
        pass(vec![press(true)]);
        let by_click = pass(vec![press(false)]).0;
        (by_keys, by_click)
    };
    assert_eq!(
        run(false),
        (true, true),
        "the plain pill takes keys: the test reaches the button"
    );
    assert_eq!(
        run(true),
        (false, true),
        "the grant pill takes the click only"
    );
}

#[test]
fn settings_toggle_hint_wraps_before_the_switch() {
    assert_eq!(settings_toggle_text_width(600.0), 544.0);
    assert_eq!(settings_toggle_text_width(30.0), 0.0);
    let hint = "Grok can see the screen and use the mouse and keyboard through Loadout. Ask still asks first. Deletes, sends, money and credentials always ask.";
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(480.0, 400.0),
        )),
        ..Default::default()
    };
    let mut row_right = 0.0_f32;
    let out = crate::theme::test_pass(&ctx, input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            row_right = ui.max_rect().right();
            let mut on = true;
            assert!(!settings_toggle(
                ui,
                "Let Grok control the desktop",
                hint,
                &mut on
            ));
        });
    });
    let mut hint_right = None;
    for clipped in &out.shapes {
        if let egui::Shape::Text(t) = &clipped.shape {
            if t.galley.text().starts_with("Grok can see") {
                hint_right = Some(t.pos.x + t.galley.rect.right());
            }
        }
    }
    let hint_right = hint_right.expect("hint painted");
    assert!(
        hint_right <= row_right - SETTINGS_SWITCH_W - SETTINGS_TOGGLE_GAP + 0.5,
        "hint runs under the switch: {hint_right} vs row {row_right}"
    );
}

#[test]
fn help_text_paints_backticks_as_code_not_literals() {
    let job = inline_code_job("run `/loop 30m` now", 12.0, Color32::GRAY, 400.0);
    assert!(
        !job.text.contains('`'),
        "backticks must not paint: {}",
        job.text
    );
    assert_eq!(job.text, "run /loop 30m now");
    let fonts: Vec<_> = job
        .sections
        .iter()
        .map(|s| s.format.font_id.family.clone())
        .collect();
    assert_eq!(
        fonts,
        vec![
            egui::FontFamily::Proportional,
            egui::FontFamily::Monospace,
            egui::FontFamily::Proportional
        ]
    );
    // An unpaired backtick is prose, not the start of a code span.
    let open = inline_code_job("it's `open", 12.0, Color32::GRAY, 400.0);
    assert!(open
        .sections
        .iter()
        .all(|s| s.format.font_id.family == egui::FontFamily::Proportional));
}

#[test]
fn titlebar_chip_does_not_stretch_to_the_bar() {
    let src = include_str!("cards.rs");
    let chip = src
        .split("pub fn titlebar_update_chip(")
        .nth(1)
        .and_then(|s| s.split("\npub fn ").next())
        .expect("titlebar_update_chip");
    assert!(
        chip.contains("allocate_exact_size") && !chip.contains("Frame::NONE"),
        "a Frame in the centred titlebar row fills the whole 40px bar: {chip}"
    );
}
use grokhub_core::parse_loop_line;

fn with_fonts(mut check: impl FnMut(&mut egui::epaint::text::FontsView<'_>)) {
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.fonts_mut(|fonts| check(fonts));
        });
    });
}

#[test]
fn grok_tile_body_does_not_clip_mid_word() {
    with_fonts(|fonts| {
        assert_eq!(tile_body_text(fonts, "short", 220.0), "short");
        assert_eq!(tile_body_text(fonts, "  ", 220.0), "");
        let long = "Built-in — Required reading before you start, watch, or wait on anything that remains after the first eighty glyphs of this description so wrap is the path and the card stays three lines.";
        let width = 160.0;
        let painted = tile_body_text(fonts, long, width);
        assert!(painted.ends_with('…'), "{painted}");
        assert!(!painted.ends_with(" re"), "{painted}");
        assert!(!painted.contains("take(80)"), "{painted}");
        let stem = painted.trim_end_matches('…');
        let orig: Vec<_> = long.split_whitespace().collect();
        let got: Vec<_> = stem.split_whitespace().collect();
        assert!(!got.is_empty() && got.len() < orig.len(), "{painted}");
        assert_eq!(&orig[..got.len()], got.as_slice(), "{painted}");
        let clamped = tile_body_galley(fonts, &painted, width);
        assert!(
            clamped.rows.len() <= TILE_BODY_MAX_LINES,
            "rows {} text {painted}",
            clamped.rows.len()
        );
        let full = tile_body_galley(fonts, long, width);
        assert!(
            full.rows.len() > TILE_BODY_MAX_LINES,
            "uncut copy must exceed three lines, got {}",
            full.rows.len()
        );
        let wide = tile_body_text(fonts, "one two", 400.0);
        assert_eq!(wide, "one two");
        assert!(!wide.contains('…'));
    });
}

fn paint_tile_grid(ctx: &egui::Context, tiles: &[(&str, &str, Option<&str>)]) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "inter-bold".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Name("inter-bold".into()),
        vec!["inter-bold".into()],
    );
    ctx.set_fonts(fonts);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(960.0, 900.0),
        )),
        ..Default::default()
    };
    let _ = crate::theme::test_pass(ctx, raw, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.set_width(920.0);
            ui.set_min_width(920.0);
            tile_row(ui, tiles.len(), |ui, i| {
                let (title, body, action) = tiles[i];
                let _ = grok_tile(ui, TileIcon::Bolt, title, body, action, false);
            });
        });
    });
}

fn action_gap(metric: &TileMetric) -> f32 {
    metric.bottom - metric.button_bottom.expect("action")
}

#[test]
fn grok_tile_row_shares_height_and_pins_action() {
    let _paint = crate::theme::hold_paint_test();
    let ctx = egui::Context::default();
    let long_body = "word ".repeat(80);
    let long_title = "Workspace memory and nightly review suggestions for the cabin skill shelf that wrap the title";
    let mixed = [
        ("Add", long_body.as_str(), Some("Use in chat")),
        (long_title, "", Some("Use in chat")),
    ];
    let _ = take_tile_metrics();
    for _ in 0..3 {
        let _ = take_tile_metrics();
        paint_tile_grid(&ctx, &mixed);
    }
    let mixed_metrics = take_tile_metrics();
    assert_eq!(mixed_metrics.len(), 2, "{mixed_metrics:?}");
    let (a, b) = (mixed_metrics[0], mixed_metrics[1]);
    assert!(
        (a.bottom - b.bottom).abs() < 2.0,
        "both cards must share a row: {mixed_metrics:?}"
    );
    assert!(
        (a.height - b.height).abs() < 1.5,
        "taller neighbor wins: {mixed_metrics:?}"
    );
    assert!(
        (a.body_h - b.body_h).abs() < 0.5 && a.body_h > 30.0,
        "empty and long bodies share the 3-line band: {mixed_metrics:?}"
    );
    let gap_a = action_gap(&a);
    let gap_b = action_gap(&b);
    assert!(
        (gap_a - gap_b).abs() < 1.5 && gap_a < 22.0 && gap_b < 22.0,
        "Use in chat pins to the card bottom: gaps {gap_a} {gap_b} {mixed_metrics:?}"
    );

    let stubs = [
        ("Alpha", "", Some("Use in chat")),
        ("Beta", long_body.as_str(), Some("Add")),
    ];
    let _ = take_tile_metrics();
    for _ in 0..2 {
        let _ = take_tile_metrics();
        paint_tile_grid(&ctx, &stubs);
    }
    let stubs_metrics = take_tile_metrics();
    assert_eq!(stubs_metrics.len(), 2, "{stubs_metrics:?}");
    assert!(
        (stubs_metrics[0].height - stubs_metrics[1].height).abs() < 1.5,
        "copy length must not change card height: {stubs_metrics:?}"
    );
    assert!((stubs_metrics[0].body_h - stubs_metrics[1].body_h).abs() < 0.5);
    assert!((action_gap(&stubs_metrics[0]) - action_gap(&stubs_metrics[1])).abs() < 1.5);
}

#[test]
fn card_and_wall_hover_stay_in_slot() {
    let src = include_str!("cards.rs");
    let card = src
        .split("fn paint_slot_card(")
        .nth(1)
        .and_then(|s| s.split("pub fn grok_tile(").next())
        .expect("paint_slot_card");
    assert!(
        card.contains("feel_response_in_slot") && card.contains("veil_over"),
        "skill and automation cards must tint inside the slot: {card}"
    );
    assert!(
        !card.contains("rect_filled"),
        "hover plate must not cover the title: {card}"
    );
    let tile = src
        .split("pub fn grok_tile(")
        .nth(1)
        .and_then(|s| s.split("pub fn tile_row(").next())
        .expect("grok_tile");
    assert!(
        tile.contains("paint_slot_card")
            && tile.contains("card_title_font")
            && tile.contains("tile_body_text")
            && tile.contains("TILE_BODY_MAX_LINES")
            && tile.contains("tile_action_gap")
            && tile.contains(".wrap()")
            && !tile.contains("take(80)")
            && !tile.contains("right_to_left"),
        "Use in chat pins under a 3-line body, never clip mid-word: {tile}"
    );
    let row = src
        .split("pub fn tile_row(")
        .nth(1)
        .and_then(|s| s.split("fn still_jpeg(").next())
        .expect("tile_row");
    assert!(
        row.contains("tile_pass_id")
            && row.contains("TileRowPass")
            && row.contains("request_repaint"),
        "a tile row must stretch short cards up to the tallest neighbor: {row}"
    );
    let pages = include_str!("app/pages.rs");
    let skills = pages
        .split("fn ui_skills(")
        .nth(1)
        .and_then(|s| s.split("fn tick_history_search(").next())
        .expect("ui_skills");
    assert!(
        !skills.contains("tab_pill(ui, \"Connectors\"")
            && skills.contains("self.open_connectors()"),
        "Skills has no Connectors tab; its Connectors pill opens Settings: {skills}"
    );
    assert!(skills.contains("grok_tile("), "skill cards stay grok_tile");
    let empty = src
        .split("pub fn empty_prompt_tile(")
        .nth(1)
        .and_then(|s| s.split("#[cfg(test)]").next())
        .expect("empty_prompt_tile");
    assert!(
        empty.contains("paint_slot_card") && !empty.contains("rect_filled"),
        "{empty}"
    );
    let wall = src
        .split("fn wall_slot_feel(")
        .nth(1)
        .and_then(|s| s.split("pub fn empty_prompt_tile(").next())
        .expect("wall_slot_feel");
    assert!(
        wall.contains("feel_response_in_slot") && !wall.contains("rect_filled"),
        "{wall}"
    );
    let photo = src
        .split("fn imagine_photo_tile(")
        .nth(1)
        .and_then(|s| s.split("fn imagine_disk_tex(").next())
        .expect("imagine_photo_tile");
    assert!(
        photo.contains("wall_slot_feel") && !photo.contains("wash"),
        "{photo}"
    );
    let disk = src
        .split("fn imagine_disk_tile(")
        .nth(1)
        .and_then(|s| s.split("fn wall_slot_feel(").next())
        .expect("imagine_disk_tile");
    assert!(
        disk.contains("wall_slot_feel") && !disk.contains("wash"),
        "{disk}"
    );
}

#[test]
fn the_selected_tab_label_is_readable_on_its_pill() {
    let src = include_str!("cards.rs");
    let tab = src
        .split("pub fn felt_tab(")
        .nth(1)
        .and_then(|s| s.split("pub fn felt_menu_row(").next())
        .expect("felt_tab");
    let layout = tab.find("layout_no_wrap").expect("tab galley");
    let paint = tab.find(".galley(").expect("tab paint");
    assert!(
        tab[layout..paint].contains("Color32::PLACEHOLDER"),
        "a galley laid out in fg() ignores the paint colour, so the selected tab \
         paints a white label on a white pill: {tab}"
    );
    assert!(
        tab.contains("blend_color(crate::theme::muted(), crate::theme::bg()"),
        "the selected tab reads in the background colour: {tab}"
    );
}

#[test]
fn chip_row_act_is_apply_or_dismiss() {
    assert_ne!(ChipRowAct::Apply(0), ChipRowAct::Dismiss(0));
    match ChipRowAct::Apply(4) {
        ChipRowAct::Apply(i) => assert_eq!(i, 4),
        ChipRowAct::Dismiss(_) => panic!("apply is not dismiss"),
    }
    assert_eq!(
        chip_paint_label("Continue Night cabin"),
        "Continue Night cabin"
    );
    let long = chip_paint_label(
        "Continue the work from the chat \"Night cabin\". Last ask: paint the wall.",
    );
    assert!(
        long.contains("Night cabin") && long.contains("paint the wall"),
        "{long}"
    );
    assert!(!long.contains('…'), "{long}");
    for fail in [
        "you should already have mcp con…",
        "check doosan for information on…",
        "can you search the web and veri…",
    ] {
        let paint = chip_paint_label(fail);
        assert!(!paint.contains('…'), "{paint}");
        assert!(paint.split_whitespace().count() >= 3, "{paint}");
    }
}

#[test]
fn composer_hover_copy_covers_the_real_pills() {
    let modes: Vec<_> = composer_modes().iter().map(|(id, _)| *id).collect();
    assert_eq!(modes, ["chat", "plan", "ask"]);
    for id in modes {
        let (title, body) = composer_session_tip(id).expect(id);
        assert!(!title.is_empty(), "{id}");
        assert!(body.len() < 160, "{id} tip too long: {body}");
    }
    let (chat_title, chat) = composer_session_tip("chat").unwrap();
    assert_eq!(chat_title, "Chat");
    assert!(chat.contains("Normal chat"), "{chat}");
    let (plan_title, plan) = composer_session_tip("plan").unwrap();
    assert_eq!(plan_title, "Plan");
    assert!(plan.contains("plan") && plan.contains("/plan"), "{plan}");
    let (look_title, look) = composer_session_tip("ask").unwrap();
    assert_eq!(look_title, "btw");
    assert_eq!(composer_modes()[2], ("ask", "btw"));
    assert_ne!(composer_modes()[2].1, permission_modes()[0].1);
    const { assert!(SEG_INSET_X >= 8.0) };
    const { assert!(SEG_INSET_Y >= 6.0) };
    let styled = include_str!("cards.rs")
        .split("fn felt_segment_styled(")
        .nth(1)
        .and_then(|s| s.split("pub fn felt_tab(").next())
        .expect("felt_segment_styled");
    assert!(
        styled.contains("SEG_INSET_X")
            && styled.contains("layout_no_wrap")
            && styled.contains("galley"),
        "Questions must size to the label plus inset, not a 52px jam: {styled}"
    );
    assert!(look.contains("does not stop"), "{look}");
    assert!(look.contains("/btw"), "{look}");
    assert!(look.contains("look-safe"), "{look}");
    assert!(!look.contains("Ask"), "{look}");
    assert!(!look_title.contains("Read"), "{look_title}");
    assert!(composer_session_tip("always-approve").is_none());
    assert!(composer_session_tip("auto").is_none());

    let perms: Vec<_> = permission_modes().iter().map(|(id, _)| *id).collect();
    assert_eq!(perms, ["ask", "auto", "always-approve"]);
    for id in perms {
        let (title, body) = composer_perm_tip(id).expect(id);
        assert!(!title.is_empty(), "{id}");
        assert!(body.len() < 160, "{id} tip too long: {body}");
    }
    let (ask_p, ask_body) = composer_perm_tip("ask").unwrap();
    assert_eq!(ask_p, "Ask");
    assert!(
        ask_body.contains("Allow / Deny") && ask_body.contains("denied"),
        "{ask_body}"
    );
    let (auto_t, auto) = composer_perm_tip("auto").unwrap();
    assert_eq!(auto_t, "Auto");
    assert!(
        auto.contains("Safe tools") && auto.contains("/auto"),
        "{auto}"
    );
    let (always_t, always) = composer_perm_tip("always-approve").unwrap();
    assert_eq!(always_t, "Always");
    assert!(
        always.contains("Skip every tool") && always.contains("/always-approve"),
        "{always}"
    );
    assert!(composer_perm_tip("chat").is_none());
    assert!(composer_perm_tip("plan").is_none());

    let chip = include_str!("cards.rs")
        .split("pub fn auto_chip(")
        .nth(1)
        .and_then(|s| s.split("pub struct SessionRowOut").next())
        .expect("auto_chip");
    assert!(
        chip.contains("AUTO_CHIP_TIP") && chip.contains("reason") && !chip.contains("Popup"),
        "the Auto chip shows the reason on hover and opens no dropdown: {chip}"
    );
}

#[test]
fn mode_pill_fits_the_composer_cluster() {
    assert_eq!(composer_go_cluster_w(), 22.0 + 28.0 + 8.0 * 3.0 + 12.0);
    assert_eq!(composer_modes().len(), 3);
    assert_eq!(permission_modes().len(), 3);
    let session = include_str!("cards.rs")
        .split("pub fn session_row(")
        .nth(1)
        .and_then(|s| s.split("pub fn voice_mode_row(").next())
        .expect("session_row");
    assert!(
        session.contains("felt_segment")
            && session.contains("felt_perm_segment")
            && session.contains("out.why = auto_chip("),
        "composer session row ends with the read-only Auto chip: {session}"
    );
    assert_eq!(
        permission_risk_stroke_w("always-approve", false),
        permission_risk_stroke_w("ask", false)
    );
    assert_eq!(
        permission_risk_stroke_w("always-approve", false),
        permission_risk_stroke_w("auto", false)
    );
    assert_eq!(
        permission_risk_stroke_color("always-approve", false),
        permission_risk_stroke_color("ask", false)
    );
    assert_eq!(
        permission_risk_stroke_color("always-approve", false),
        crate::theme::border()
    );
    let _paint = crate::theme::hold_paint_test();
    crate::theme::set_paint_dark(true);
    assert_eq!(
        permission_risk_stroke_color("always-approve", true),
        crate::theme::ALWAYS_AMBER_DARK
    );
    crate::theme::set_paint_dark(false);
    assert_eq!(
        permission_risk_stroke_color("always-approve", true),
        crate::theme::ALWAYS_AMBER_LIGHT
    );
    crate::theme::set_paint_dark(true);
    assert_ne!(
        permission_risk_stroke_color("always-approve", true),
        crate::theme::setup()
    );
    assert_ne!(
        permission_risk_stroke_color("always-approve", false),
        crate::theme::always_amber()
    );
    assert_eq!(permission_risk_stroke_w("always-approve", true), 2.0);
    assert_eq!(permission_risk_stroke_w("auto", true), 1.0);
    assert_eq!(permission_risk_stroke_w("ask", true), 1.0);
    assert_eq!(permission_risk_fill(true), crate::theme::nav_active());
    assert_eq!(permission_risk_fill(false), Color32::TRANSPARENT);
    assert_ne!(permission_risk_fill(true), crate::theme::always_amber());
    assert!(permission_risk_strong("always-approve", true));
    assert!(!permission_risk_strong("always-approve", false));
    assert!(!permission_risk_strong("ask", true));
    assert!(!permission_risk_strong("auto", true));

    let styled = include_str!("cards.rs")
        .split("fn felt_segment_styled(")
        .nth(1)
        .and_then(|t| t.split("pub fn felt_tab(").next())
        .expect("felt_segment_styled");
    assert!(
        styled.contains("0.98 + 0.02 * on_t")
            && styled.contains("ALWAYS_SETTLE_SECS")
            && styled.contains("idle.width + (stroke.width - idle.width) * on_t"),
        "Always must settle scale+stroke over ALWAYS_SETTLE_SECS: {styled}"
    );

    assert!(
        session.contains("composer_session_tip")
            && session.contains("composer_perm_tip")
            && session.contains("with_composer_tip"),
        "composer pills must show hover help: {session}"
    );
    let seg = include_str!("cards.rs")
        .split("pub fn felt_segment(")
        .nth(1)
        .and_then(|s| s.split("pub fn permission_risk_stroke_w(").next())
        .expect("felt_segment");
    assert!(
        seg.contains("FONT_BODY") && seg.contains("None"),
        "session pills stay quieter than permission: {seg}"
    );
    let perm_seg = include_str!("cards.rs")
        .split("pub fn felt_perm_segment(")
        .nth(1)
        .and_then(|s| s.split("fn felt_segment_styled(").next())
        .expect("felt_perm_segment");
    assert!(
        perm_seg.contains("FONT_CHROME") && perm_seg.contains("permission_risk_stroke_w"),
        "permission row keeps a designed stroke: {perm_seg}"
    );
    let voice = include_str!("cards.rs")
        .split("pub fn voice_mode_row(")
        .nth(1)
        .and_then(|s| s.split("pub enum ChipRowAct").next())
        .expect("voice_mode_row");
    assert!(
        voice.contains("theme::live()") && voice.contains("ghost_pill(ui, \"Stop\")"),
        "voice mode must show a live indicator and Stop: {voice}"
    );
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            assert!(!voice_mode_row(ui, "Listening"));
        });
    });
    let switch = include_str!("cards.rs")
        .split("pub fn settings_switch(")
        .nth(1)
        .and_then(|s| s.split("pub fn settings_dropdown(").next())
        .expect("settings_switch");
    assert!(
        switch.contains("animate_selection") && switch.contains("lerp_f32"),
        "settings switch must slide the knob: {switch}"
    );
    let dropdown = include_str!("cards.rs")
        .split("pub fn settings_dropdown(")
        .nth(1)
        .and_then(|s| s.split("pub fn settings_field(").next())
        .expect("settings_dropdown");
    assert!(
        dropdown.contains("ComboBox") && dropdown.contains("selectable_label"),
        "settings dropdown must be one ComboBox: {dropdown}"
    );
    let pills = include_str!("cards.rs");
    assert!(
        pills.contains("pub fn felt_pill(")
            && pills.contains("felt_pill(ui, label, PillStyle::Solid)"),
        "white_pill must delegate to felt_pill"
    );
    assert_eq!(chip_tone_color(ChipTone::Offline), crate::theme::offline());
}

#[test]
fn first_quick_chip_is_inline_not_selected() {
    assert_ne!(quick_chip_fill(true), quick_chip_fill(false));
    assert_ne!(quick_chip_stroke(true), quick_chip_stroke(false));
    assert!(quick_chip_stroke_w(true) > quick_chip_stroke_w(false));
    assert!(quick_chip_strong(true));
    assert!(!quick_chip_strong(false));
    assert_eq!(quick_chip_fg(true), crate::theme::fg());
    assert_eq!(quick_chip_fg(false), crate::theme::muted());
    assert!(chip_why_tip("Last slash", "/plan").starts_with("Why this?"));
    assert_eq!(chip_why_tip("", "Continue"), "Why this?\nContinue");
    assert_eq!(
        chip_why_tip("New here", "Start here"),
        "Suggested because you're new here"
    );
    let max_w = chip_row_width_lock(640.0);
    assert_eq!(max_w, 640.0);
    assert_ne!(max_w, 0.0);
    assert_eq!(
        chip_row_width_lock(2000.0),
        crate::theme::CHAT_COL_W,
        "inflated pane width must not stretch chips past Ask anything"
    );
    let src = include_str!("app/chips.rs");
    let slice = src
        .split("pub fn quick_chip_row(")
        .nth(1)
        .and_then(|s| s.split("// quick_chip_row_end").next())
        .expect("chip row");
    assert!(
        slice.contains("left_to_right(egui::Align::Center)") && slice.contains("CHIP_ROW_H"),
        "chips share one fixed midline height: {slice}"
    );
    assert!(
        slice.contains("with_main_wrap(false)")
            && slice.contains("fluid_chip_count")
            && slice.contains("chip_row_visible_w")
            && !slice.contains("with_main_wrap(true)")
            && !slice.contains("CHIP_CLUSTER_H")
            && !slice.contains("CHIP_VISIBLE_MAX"),
        "chips fit the row and drop overflow; no second wrap line and no fixed visible count: {slice}"
    );
    assert!(
        slice.contains("layout_chip_label") && slice.contains("CHIP_PAD_X"),
        "long labels ellipsize on one line inside the pad: {slice}"
    );
    let layout = src
        .split("pub fn layout_chip_label(")
        .nth(1)
        .and_then(|s| s.split("fn paint_chip_pill(").next())
        .expect("layout_chip_label");
    assert!(
        layout.contains("truncate_at_width") && layout.contains("break_on_newline = false"),
        "chip labels are one line with an ellipsis: {layout}"
    );
    assert!(
        !slice.contains("set_width(max_w)") && !slice.contains("set_min_width"),
        "chip cluster must shrink-wrap, not fill the pill: {slice}"
    );
    assert!(
        slice.contains("allocate_ui_with_layout") && slice.contains("CHIP_ROW_H"),
        "chip row stays one fixed height: {slice}"
    );
    const {
        assert!(CHIP_ROW_H >= 13.0 + CHIP_PAD_Y * 2.0);
        assert!(CHIP_LABEL_MAX_W >= 48.0);
    }
    assert!(
        !slice.contains("ui.with_layout("),
        "with_layout eats remaining height and drops chips to the bottom: {slice}"
    );
    assert_eq!(CHIP_EMPTY_LABEL, "Nothing queued");
    assert!(
        slice.contains("paint_empty_chip_state(ui)"),
        "empty ranking must paint a muted placeholder, not a blank row: {slice}"
    );
    let empty = include_str!("app/chips.rs")
        .split("pub fn paint_empty_chip_state(")
        .nth(1)
        .and_then(|s| s.split("pub fn quick_chip_row(").next())
        .expect("paint_empty_chip_state");
    assert!(
        empty.contains("CHIP_EMPTY_LABEL")
            && empty.contains("quick_chip_fill(false)")
            && empty.contains("paint_chip_pill")
            && empty.contains("CHIP_ROW_H")
            && empty.contains("Sense::hover()")
            && empty.contains("layout_chip_label"),
        "empty state is one muted single-line chip, not a ranked action: {empty}"
    );
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            assert!(quick_chip_row(ui, &[]).is_none());
        });
    });
}

fn sample_chip(label: &str, primary: bool) -> grokhub_core::QuickChip {
    grokhub_core::QuickChip {
        id: label.into(),
        label: label.into(),
        value: format!("send {label}"),
        kind: grokhub_core::ChipKind::Chat,
        score: 1.0,
        hint: String::new(),
        primary,
    }
}

#[test]
fn quick_chips_are_one_line_and_fluid() {
    let gap = CHIP_GAP;
    let w = 100.0;
    assert_eq!(fluid_chip_count(&[w, w, w, w, w], w, gap), 1);
    assert_eq!(fluid_chip_count(&[w, w, w, w, w], w + gap + w, gap), 2);
    assert_eq!(
        fluid_chip_count(&[w, w, w, w, w], w + gap + w + 1.0, gap),
        2,
        "a chip that does not fit is dropped, not wrapped"
    );
    assert_eq!(fluid_chip_count(&[w; 5], 10_000.0, gap), 5);
    assert_eq!(fluid_chip_count(&[], 400.0, gap), 0);
    assert_eq!(
        fluid_chip_count(&[200.0], 100.0, gap),
        0,
        "a pill wider than the row is omitted, not sliced"
    );
    // Owner shot 2026-09-23: last chip starts on screen and is cut
    // ("control my mou" / "close one"). Drop that whole pill.
    let owner_w = [72.0, 48.0, 168.0, 176.0, 150.0];
    let partial = 72.0 + gap + 48.0 + gap + 168.0 + gap + 176.0 + 40.0;
    assert!(partial < owner_w.iter().sum::<f32>() + gap * 4.0);
    assert_eq!(
        fluid_chip_count(&owner_w, partial, gap),
        4,
        "do not paint a chip the viewport would cut"
    );
    assert_eq!(chip_label_budget(4_000.0), CHIP_LABEL_MAX_W);
    let narrow = chip_label_budget(120.0);
    assert!(narrow < CHIP_LABEL_MAX_W);
    assert!(chip_pill_width(narrow) <= 120.5);

    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            let font = FontId::proportional(13.0);
            let color = Color32::WHITE;
            let long = layout_chip_label(
                ui,
                "you should already have mcp configured",
                font.clone(),
                color,
                CHIP_LABEL_MAX_W,
            );
            let cut = layout_chip_label(
                ui,
                "how do you feel about the new grok",
                font.clone(),
                color,
                CHIP_LABEL_MAX_W,
            );
            let short = layout_chip_label(ui, "Continue", font.clone(), color, CHIP_LABEL_MAX_W);
            let desc = layout_chip_label(ui, "gypq", font, color, CHIP_LABEL_MAX_W);
            for galley in [&long, &cut, &short, &desc] {
                assert_eq!(
                    galley.rows.len(),
                    1,
                    "single line: {}",
                    galley.rows[0].text()
                );
                assert!(
                    galley.size().y + CHIP_PAD_Y * 2.0 <= CHIP_ROW_H + 0.5,
                    "descenders must fit the fixed height: h={} row={}",
                    galley.size().y,
                    CHIP_ROW_H
                );
            }
            let natural = ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    "you should already have mcp configured".into(),
                    FontId::proportional(13.0),
                    color,
                )
                .size()
                .x
            });
            assert!(
                natural > CHIP_LABEL_MAX_W,
                "owner-length label must overflow the cap, got {natural}"
            );
            assert!(
                long.elided,
                "long label must ellipsize: {}",
                long.rows[0].text()
            );
            assert!(
                long.rows[0].text().ends_with('…'),
                "ellipsis, not a hard clip: {}",
                long.rows[0].text()
            );
            assert!(
                cut.elided && cut.rows[0].text().ends_with('…'),
                "clipped owner label must ellipsize: {}",
                cut.rows[0].text()
            );
            assert!(long.size().x <= CHIP_LABEL_MAX_W + 0.5);
            assert!(
                !short.elided,
                "short label stays whole: {}",
                short.rows[0].text()
            );
            assert_eq!(cut.rows.len(), 1);
            // Owner shot: these wrapped to two lines and the last was cut
            // mid-glyph. Each stays one line; overflow is an ellipsis.
            for label in [
                "hey that was super close that time",
                "lets try this again using only",
                "control my mouse close one",
            ] {
                let galley = layout_chip_label(
                    ui,
                    label,
                    FontId::proportional(13.0),
                    color,
                    CHIP_LABEL_MAX_W,
                );
                assert_eq!(galley.rows.len(), 1, "owner wrap: {label}");
                let natural = ui.fonts_mut(|f| {
                    f.layout_no_wrap(label.to_owned(), FontId::proportional(13.0), color)
                        .size()
                        .x
                });
                if natural > CHIP_LABEL_MAX_W {
                    assert!(
                        galley.elided && galley.rows[0].text().ends_with('…'),
                        "owner clip must be an ellipsis: {label} -> {}",
                        galley.rows[0].text()
                    );
                } else {
                    assert!(!galley.elided, "short owner label stays whole: {label}");
                }
                assert!(galley.size().y + CHIP_PAD_Y * 2.0 <= CHIP_ROW_H + 0.5);
            }
            let continue_g = layout_chip_label(
                ui,
                "continue",
                FontId::proportional(13.0),
                color,
                CHIP_LABEL_MAX_W,
            );
            let test_g = layout_chip_label(
                ui,
                "test",
                FontId::proportional(13.0),
                color,
                CHIP_LABEL_MAX_W,
            );
            assert!(!continue_g.elided && continue_g.rows.len() == 1);
            assert!(!test_g.elided && test_g.rows.len() == 1);
            let owner = vec![
                sample_chip("continue", false),
                sample_chip("test", false),
                sample_chip("hey that was super close that time", false),
                sample_chip("lets try this again using only", false),
                sample_chip("control my mouse close one", false),
            ];
            ui.spacing_mut().item_spacing.y = 0.0;
            let y_owner = ui.cursor().min.y;
            let _ = quick_chip_row(ui, &owner);
            let owner_h = ui.cursor().min.y - y_owner;
            assert!(
                owner_h + 0.5 >= CHIP_ROW_H && owner_h < CHIP_ROW_H + 8.0,
                "owner row must stay one fixed line, got {owner_h}"
            );

            let mixed = vec![
                sample_chip("Continue", false),
                sample_chip("you should already have mcp configured", false),
                sample_chip("Fix", false),
                sample_chip("how do you feel about the new grok", false),
                sample_chip("Skills", false),
            ];
            ui.spacing_mut().item_spacing.y = 0.0;
            let y0 = ui.cursor().min.y;
            let _ = quick_chip_row(ui, &mixed);
            let mixed_h = ui.cursor().min.y - y0;
            let y1 = ui.cursor().min.y;
            let _ = quick_chip_row(ui, &[sample_chip("Fix", false)]);
            let short_h = ui.cursor().min.y - y1;
            assert!(
                (mixed_h - CHIP_ROW_H).abs() < 1.0,
                "mixed row height {mixed_h} != {CHIP_ROW_H}"
            );
            assert!(
                (short_h - mixed_h).abs() < 0.5,
                "short and long rows must match: {short_h} vs {mixed_h}"
            );
        });
    });
}

fn install_chip_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "inter-bold".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Name("inter-bold".into()),
        vec!["inter-bold".into()],
    );
    ctx.set_fonts(fonts);
}

fn chip_frame(
    ctx: &egui::Context,
    time: f64,
    pointer: Option<egui::Pos2>,
    press: Option<bool>,
    chips: &[grokhub_core::QuickChip],
) -> (Option<ChipRowAct>, egui::CursorIcon, Vec<egui::Rect>) {
    install_chip_fonts(ctx);
    let mut events = Vec::new();
    if let Some(pos) = pointer {
        events.push(egui::Event::PointerMoved(pos));
        if let Some(pressed) = press {
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 400.0),
        )),
        time: Some(time),
        events,
        ..Default::default()
    };
    let mut act = None;
    let out = crate::theme::test_pass(ctx, raw, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            act = quick_chip_row(ui, chips);
        });
    });
    let hits = ctx
        .data(|d| d.get_temp::<Vec<egui::Rect>>(egui::Id::new("qchip-dismiss-hits")))
        .unwrap_or_default();
    (act, out.platform_output.cursor_icon, hits)
}

#[test]
fn chip_dismiss_hit_stays_under_the_pointer() {
    let pill = egui::Rect::from_min_size(egui::pos2(8.0, 12.0), egui::vec2(140.0, CHIP_ROW_H));
    let slot = chip_dismiss_rect(pill);
    assert!(pill.contains_rect(slot));
    assert!((slot.width() - CHIP_DISMISS_SLOT).abs() < 0.01);
    assert!((pill.right() - slot.right() - CHIP_PAD_X).abs() < 0.01);
    assert_eq!(chip_dismiss_rect(pill), slot);
    assert_eq!(chip_hit_act(1, slot, slot.center()), ChipRowAct::Dismiss(1));
    assert_eq!(
        chip_hit_act(1, slot, slot.left_center() - egui::vec2(12.0, 0.0)),
        ChipRowAct::Apply(1)
    );

    let chips = vec![sample_chip("Continue", true), sample_chip("Fix", false)];
    let ctx = egui::Context::default();
    let (_, _, rest) = chip_frame(&ctx, 0.0, None, None, &chips);
    assert_eq!(rest.len(), 2, "both chips reserve a dismiss hit: {rest:?}");
    let parked = rest[0].center();
    assert!(rest[0].contains(parked));
    let mut cursors = Vec::new();
    for step in 1..=16 {
        let time = step as f64 * 0.025;
        let (_, cursor, hits) = chip_frame(&ctx, time, Some(parked), None, &chips);
        assert_eq!(hits.len(), 2, "hover must not drop a dismiss hit: {hits:?}");
        assert_eq!(
            hits, rest,
            "hover must not move the dismiss slot off the pointer at t={time}: {hits:?} vs {rest:?}"
        );
        assert!(
            hits[0].contains(parked),
            "pointer left the × at t={time}: {hits:?}"
        );
        cursors.push(cursor);
    }
    assert!(
        cursors.iter().all(|c| *c == egui::CursorIcon::PointingHand),
        "the pointer must stay a hand on the ×, not flash: {cursors:?}"
    );

    let (pressed, _, _) = chip_frame(&ctx, 0.50, Some(parked), Some(true), &chips);
    assert!(pressed.is_none(), "press alone is not a click: {pressed:?}");
    let (clicked, cursor, hits) = chip_frame(&ctx, 0.54, Some(parked), Some(false), &chips);
    assert_eq!(hits, rest, "the click must not shove the ×: {hits:?}");
    assert!(hits[0].contains(parked));
    assert_eq!(cursor, egui::CursorIcon::PointingHand);
    assert_eq!(clicked, Some(ChipRowAct::Dismiss(0)));

    let label = egui::pos2(rest[0].left() - 28.0, rest[0].center().y);
    let ctx = egui::Context::default();
    let (_, _, label_rest) = chip_frame(&ctx, 0.0, None, None, &chips);
    assert!(
        !label_rest[0].contains(label),
        "label probe must miss the ×: {label:?} {label_rest:?}"
    );
    for step in 1..=16 {
        let (_, _, hits) = chip_frame(&ctx, step as f64 * 0.025, Some(label), None, &chips);
        assert_eq!(hits, label_rest, "label hover must not move the ×");
    }
    let _ = chip_frame(&ctx, 0.50, Some(label), Some(true), &chips);
    let (applied, _, _) = chip_frame(&ctx, 0.54, Some(label), Some(false), &chips);
    assert_eq!(applied, Some(ChipRowAct::Apply(0)));
}

#[test]
fn chip_cluster_lead_centers_within_a_pixel() {
    assert_eq!(chip_cluster_w(&[100.0, 80.0, 60.0], 2, CHIP_GAP), 186.0);
    assert_eq!(chip_cluster_w(&[100.0, 80.0, 60.0], 3, CHIP_GAP), 252.0);
    assert_eq!(chip_cluster_w(&[100.0], 5, CHIP_GAP), 100.0);
    assert_eq!(chip_cluster_w(&[], 0, CHIP_GAP), 0.0);
    assert_eq!(chip_cluster_lead(600.0, 252.0, 1.0), 174.0);
    assert_eq!(chip_cluster_lead(601.0, 252.0, 1.0), 175.0);
    assert_eq!(chip_cluster_lead(601.0, 252.0, 2.0), 174.5);
    assert_eq!(chip_cluster_lead(300.0, 420.0, 1.0), 0.0);
    assert_eq!(chip_cluster_lead(500.0, 100.0, 0.0), 200.0);
    for (row_w, cluster_w) in [
        (360.0, 117.0),
        (480.5, 301.25),
        (760.0, 433.0),
        (1033.0, 77.7),
    ] {
        for ppp in [1.0f32, 1.25, 1.5, 2.0] {
            let lead = chip_cluster_lead(row_w, cluster_w, ppp);
            let off_px = (lead + cluster_w * 0.5 - row_w * 0.5).abs() * ppp;
            assert!(
                off_px <= 1.0,
                "row {row_w} cluster {cluster_w} at {ppp}x: lead {lead} is {off_px} px off center"
            );
            assert_eq!((lead * ppp).fract(), 0.0, "lead lands on a physical pixel");
        }
    }
}

fn chip_pills_in_column(
    screen_w: f32,
    ppp: f32,
    chips: &[grokhub_core::QuickChip],
) -> (egui::Rect, Vec<egui::Rect>) {
    let ctx = egui::Context::default();
    install_chip_fonts(&ctx);
    ctx.set_pixels_per_point(ppp);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(screen_w, 400.0),
        )),
        ..Default::default()
    };
    let mut col = egui::Rect::NOTHING;
    for _ in 0..2 {
        let _ = crate::theme::test_pass(&ctx, raw.clone(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.set_max_width(composer_pill_w(ui.ctx().content_rect().width()));
                    col = ui.max_rect();
                    let _ = quick_chip_row(ui, chips);
                });
            });
        });
    }
    let pills = ctx
        .data(|d| d.get_temp::<Vec<egui::Rect>>(egui::Id::new("qchip-pills")))
        .unwrap_or_default();
    (col, pills)
}

#[test]
fn quick_chips_sit_centered_under_the_chat_box() {
    let chips = vec![
        sample_chip("Continue", true),
        sample_chip("Fix the failing test", false),
        sample_chip("Plan", false),
    ];
    for screen_w in [700.0f32, 900.0, 1400.0, 1920.0] {
        for ppp in [1.0f32, 1.25, 1.5, 2.0] {
            let (col, pills) = chip_pills_in_column(screen_w, ppp, &chips);
            assert_eq!(
                pills.len(),
                3,
                "all three chips fit at {screen_w}px {ppp}x: {pills:?}"
            );
            let left = pills.first().unwrap().left();
            let right = pills.last().unwrap().right();
            let mid = (left + right) * 0.5;
            assert!(
                (mid - col.center().x).abs() <= 1.0,
                "chips centered on the chat box at {screen_w}px {ppp}x: cluster {left}..{right}, box {col:?}"
            );
        }
    }
    let (col, empty) = chip_pills_in_column(900.0, 1.5, &[]);
    assert_eq!(empty.len(), 1, "the empty state paints one pill: {empty:?}");
    assert!(
        (empty[0].center().x - col.center().x).abs() <= 1.0,
        "the Nothing queued pill is centered too: {:?} vs {col:?}",
        empty[0]
    );
}

#[test]
fn suggested_autos_parse() {
    assert_eq!(SUGGESTED_AUTOS.len(), 7);
    for s in SUGGESTED_AUTOS {
        let (iv, prompt) = parse_loop_line(s.seed).expect(s.title);
        assert!(!iv.is_empty());
        assert!(!prompt.is_empty());
        assert!(s.seed.contains("/loop"));
        let _ = s.icon;
    }
}

#[test]
fn learned_tiles_lead_static_fallback() {
    let learned_auto = LearnedSuggestion {
        kind: SuggestionKind::Auto,
        title: "Night wrap".into(),
        body: "Close the day".into(),
        seed: Some("every day at 21, say good night".into()),
        name: None,
        trigger: None,
        instructions: None,
        provider: None,
        tool: None,
    };
    let autos = merge_suggested_autos(&[learned_auto], &[]);
    assert_eq!(autos[0].1, "Night wrap");
    assert!(autos.iter().any(|t| t.1 == "Morning brief"));
    let hidden = merge_suggested_autos(&[], &["Morning brief".into()]);
    assert!(!hidden.iter().any(|t| t.1 == "Morning brief"));
    let by_prompt = merge_suggested_autos(
        &[],
        &["summarize the workboard and last host receipt".into()],
    );
    assert!(
        !by_prompt.iter().any(|t| t.1 == "Morning brief"),
        "adding a /loop seed must hide the matching Suggested tile: {by_prompt:?}"
    );
    assert_eq!(GITHUB_TILES.len(), 2);
    assert_eq!(GITHUB_TILES[0].2, "user");
    assert_eq!(GITHUB_TILES[1].2, "list_repos");
}

#[test]
fn catalog_is_cabin_real() {
    let forbidden = [
        "outlook",
        "gmail",
        "stock",
        "ticker",
        "docx",
        "xlsx",
        "pptx",
        "powerpoint",
        "spreadsheet",
        "word document",
        "pdf",
        "video",
    ];
    for s in SUGGESTED_AUTOS {
        let blob = format!("{} {} {}", s.title, s.body, s.seed).to_ascii_lowercase();
        for w in forbidden {
            assert!(!blob.contains(w), "auto {} mentions {w}", s.title);
        }
    }
    assert_eq!(IMAGINE_SCENES.len(), 9);
    assert_eq!(imagine_word(0), "the cabin");
    assert_eq!(imagine_word(2800), "the night");
    assert_eq!(grokhub_core::imagine_aspect_label(0), "2:3");
    assert_eq!(grokhub_core::imagine_aspect_label(4), "16:9");
    assert_eq!(grokhub_core::imagine_aspect_name(0), "Tall");
    assert_eq!(
        imagine_send_cluster_w(),
        crate::theme::IMAGINE_HIT * 2.0 + 12.0
    );
    assert!(
        composer_go_cluster_w() >= 22.0 + 28.0,
        "mic + Stop disc stay inside the bar after session pills moved above"
    );
    assert_eq!(
        composer_pill_w(900.0),
        crate::theme::CHAT_COL_W,
        "900-wide cabin minus the 40px rail hits the Grok column cap"
    );
    assert_eq!(
        composer_pill_w(1400.0),
        crate::theme::CHAT_COL_W,
        "wide cabins lock to the Grok conversation column"
    );
    assert_eq!(
        composer_pill_w(3440.0),
        crate::theme::CHAT_COL_W,
        "ultrawide composer stays a centered Grok column, got {}",
        composer_pill_w(3440.0)
    );
    assert_eq!(
        composer_pill_w(3440.0),
        composer_pill_w(1920.0),
        "past the column cap the pill does not keep growing"
    );
    assert!(
        composer_pill_w(900.0) > composer_go_cluster_w() + 80.0,
        "Stop cluster must fit inside a 900-wide cabin pill"
    );
    let inner = composer_pill_w(900.0) - 16.0;
    assert_eq!(
        22.0 + 8.0 + composer_mid_w(inner) + 8.0 + composer_go_hit_w(),
        inner,
        "Plus + mid + Stop must fill the frame inner, not overflow it"
    );
    // grok.com/imagine `.query-bar` measured height (94) minus its padding.
    let bar_inner = 94.0 - 20.0;
    assert_eq!(imagine_prompt_h(), 32.0);
    assert_eq!(imagine_prompt_chip_gap(), 8.0);
    assert_eq!(
        imagine_chip_stack_h(),
        (crate::theme::IMAGINE_HIT + 8.0) * 2.0
    );
    assert!(
        imagine_prompt_h() < bar_inner,
        "prompt must be pinned, not stretched to bar min-height {bar_inner}"
    );
    let chip_top = imagine_prompt_h() + imagine_prompt_chip_gap();
    assert!(
        bar_inner > chip_top,
        "a stretching prompt of {bar_inner}px would cover chips starting at {chip_top}"
    );
    let stage = include_str!("cards.rs");
    let stage = stage
        .split("pub fn imagine_stage(")
        .nth(1)
        .and_then(|s| s.split("pub fn imagine_result_hero(").next())
        .expect("imagine_stage");
    assert!(
        stage.contains("Imagining…")
            && stage.contains("Imagining video…")
            && stage.contains("Imagine failed")
            && stage.contains("ghost_pill(ui, \"Retry\")")
            && stage.contains("hit.retry = true")
            && stage.contains("Expand")
            && stage.contains("Play")
            && stage.contains("Save")
            && stage.contains("Open")
            && stage.contains("imagine_media_click")
            && stage.contains("ImagineMediaClick::Play"),
        "generating box must be interactive: {stage}"
    );
    assert!(
        stage.contains("ImagineMediaClick::Play => hit.play = true")
            && stage.contains("ghost_pill(ui, \"Play\")"),
        "a video click must play, not only expand: {stage}"
    );
    let hero = include_str!("cards.rs");
    let hero = hero
        .split("fn imagine_video_hero(")
        .nth(1)
        .and_then(|s| s.split("pub fn imagine_masonry(").next())
        .expect("imagine_video_hero");
    assert!(
        hero.contains("video_poster_ready")
            && hero.contains("kick_video_poster")
            && hero.contains("thread::spawn")
            && !hero.contains("g.remove(&path)"),
        "video poster extract must leave the UI thread and attempt a clip once: {hero}"
    );
    assert!(
        stage.contains("!fail.is_empty()")
            && !stage.contains("!fail.is_empty() && path.is_empty()"),
        "on-stage error must win over leftover imagine_last: {stage}"
    );
    assert_eq!(imagine_kind_label(ImagineKind::Image), "Image");
    assert_eq!(imagine_kind_label(ImagineKind::Video), "Video");
    assert_eq!(imagine_kind_label(ImagineKind::Agent), "Agent");
    assert_eq!(imagine_quality_label(false), "Speed");
    assert_eq!(imagine_quality_label(true), "Quality (v2.0)");
    let fallback = imagine_still_rgba(b"not-a-jpeg");
    assert_eq!((fallback.width(), fallback.height()), (1, 1));
    for s in IMAGINE_SCENES {
        let blob = format!("{} {}", s.title, s.prompt).to_ascii_lowercase();
        for w in forbidden {
            assert!(!blob.contains(w), "imagine {} mentions {w}", s.title);
        }
        assert!(
            blob.contains("still") || blob.contains("cabin") || blob.contains("desk"),
            "imagine {} is not a still",
            s.title
        );
        assert!(!blob.contains("video"));
        assert!(!blob.contains("photo edit"));
        let _ = s.icon;
        let _ = s.tall;
        assert!(
            s.frames.len() >= 2,
            "imagine {} needs two frames to live like a cover GIF",
            s.title
        );
        for key in s.frames {
            let bytes = still_jpeg(key);
            assert!(bytes.len() > 1000, "imagine still {key} is empty");
            let img = image::load_from_memory(bytes).expect(key);
            assert!(img.width() >= 256);
            assert!(img.height() >= 256);
        }
        let (a, _, _) = imagine_frame_pair(s, 0);
        let (b, _, _) = imagine_frame_pair(s, crate::theme::IMAGINE_FRAME_MS);
        assert_ne!(a, b, "imagine {} cover must change", s.title);
        let (_, _, fade0) = imagine_frame_pair(s, 0);
        let (_, _, fade1) = imagine_frame_pair(s, crate::theme::IMAGINE_FRAME_MS - 1);
        assert!(fade0 < 0.05);
        assert!(fade1 > 0.9);
    }
    let uv = cover_uv(768.0, 512.0, 345.0, 230.0);
    assert!(uv.width() > 0.4 && uv.height() > 0.9);
}

#[test]
fn composer_pill_tracks_the_monitor() {
    assert_eq!(
        composer_pill_w(700.0),
        620.0,
        "narrow cabin minus 40px rail"
    );
    assert_eq!(composer_pill_w(900.0), crate::theme::CHAT_COL_W);
    assert_eq!(composer_pill_w(1400.0), crate::theme::CHAT_COL_W);
    assert_eq!(
        composer_pill_w(3440.0),
        crate::theme::CHAT_COL_W,
        "ultrawide composer stays a centered Grok column, got {}",
        composer_pill_w(3440.0)
    );
    assert_eq!(composer_pill_w(3440.0), composer_pill_w(1920.0));
    assert!(composer_pill_w(900.0) > composer_pill_w(700.0));
}

#[test]
fn imagine_still_tex_decodes_off_the_ui_thread() {
    let src = include_str!("cards.rs");
    let tex = src
        .split("fn imagine_still_tex(")
        .nth(1)
        .and_then(|s| s.split("fn cover_uv(").next())
        .expect("imagine_still_tex");
    let spawn = tex
        .find("thread::spawn")
        .expect("decode must leave the UI thread");
    let decode = tex.find("imagine_still_rgba").expect("bundled JPEG decode");
    assert!(
        spawn < decode && tex.contains("inflight"),
        "stock Imagine stills must not JPEG-decode on the first paint: {tex}"
    );
}

#[test]
fn imagine_disk_tex_rejects_a_huge_file() {
    let src = include_str!("cards.rs");
    let tex = src
        .split("fn imagine_disk_tex(")
        .nth(1)
        .and_then(|s| s.split("fn imagine_disk_tile(").next())
        .expect("imagine_disk_tex");
    let meta = tex.find("metadata").expect("size check before decode");
    let read = tex.find("std::fs::read").expect("read image");
    let spawn = tex
        .find("thread::spawn")
        .expect("decode must leave the UI thread");
    assert!(
        spawn < read && meta < read && tex.contains("IMAGE_FILE_CAP"),
        "a huge wall still must not decode on the UI thread: {tex}"
    );
    assert!(
        tex.contains("image_pixels_ok") || tex.contains("IMAGE_PIXEL_CAP"),
        "a tiny wall still with huge pixels must not decode on the UI thread: {tex}"
    );
}

#[test]
fn run_pulse_is_a_labeled_live_dot() {
    let src = include_str!("cards.rs");
    let pulse = src
        .split("pub fn paint_run_pulse(")
        .nth(1)
        .and_then(|s| s.split("pub fn titlebar_update_chip(").next())
        .expect("paint_run_pulse");
    assert!(
        pulse.contains("theme::live()")
            && pulse.contains("circle_filled")
            && pulse.contains("chat_run_dot_alpha")
            && pulse.contains("on_hover_text")
            && !pulse.contains("ghost_pill")
            && !pulse.contains("Stop"),
        "a chat turn shows a pulsing live dot and label, and the row has no Stop: {pulse}"
    );
    assert!(
        !pulse.contains("vec2(2.0, 16.0)") && !pulse.contains("rect_filled"),
        "the running cue is not a blinking caret: {pulse}"
    );
}

#[test]
fn run_pulse_ticks_about_thirty_times_a_second() {
    let src = include_str!("cards.rs");
    let pulse = src
        .split("pub fn paint_run_pulse(")
        .nth(1)
        .and_then(|s| s.split("pub fn titlebar_update_chip(").next())
        .expect("paint_run_pulse");
    assert!(
        pulse.contains("request_repaint_after") && pulse.contains("33"),
        "the live dot must not repaint every frame: {pulse}"
    );
    assert!(
        !pulse.contains("request_repaint()"),
        "request_repaint() would run at the monitor rate: {pulse}"
    );
}

#[test]
fn run_pulse_paints_a_labeled_row() {
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            let _paint = crate::theme::hold_paint_test();
            crate::theme::set_paint_dark(true);
            paint_run_pulse(ui, "Running", "run_terminal_cmd");
            paint_run_pulse(ui, "", "hidden");
        });
    });
}

#[test]
fn get_started_copy_and_connect() {
    let src = include_str!("cards.rs");
    assert!(
        src.contains("Get Started")
            && src.contains("Connect your Super Grok account")
            && src.contains("Connect Super Grok"),
        "{src}"
    );
    let app = concat!(
        include_str!("app/mod.rs"),
        include_str!("app/pages.rs"),
        include_str!("app/oauth.rs"),
        include_str!("app/settings.rs"),
    );
    assert!(
        app.contains("ui_get_started")
            && app.contains("ui_wants_get_started")
            && app.contains("start_oauth")
            && app.contains("oauth_err"),
        "Get Started must use cabin device-code OAuth and surface errors: {app}"
    );
    assert!(
        app.contains("keeps this sign-in to itself.")
            && !app.contains("Also signs in the Grok Build CLI"),
        "settings Connect must say the sign-in stays with GrokHub: {app}"
    );
}

#[test]
fn titlebar_session_menu_opens_below_its_button() {
    let session = include_str!("app/chat_ui.rs");
    let menu = session
        .split("fn paint_session_actions_menu(")
        .nth(1)
        .and_then(|s| s.split("fn ui_composer_stack(").next())
        .unwrap_or("");
    assert!(
        menu.contains("RectAlign::BOTTOM_START") && menu.contains("align_alternatives(&[])"),
        "the titlebar session menu stays below its button"
    );
}

#[test]
fn switch_and_segments_expose_checked_state() {
    assert_eq!(switch_access_name(true), "On");
    assert_eq!(switch_access_name(false), "Off");
    let src = include_str!("cards.rs");
    let switch = src
        .split("pub fn settings_switch(")
        .nth(1)
        .and_then(|s| s.split("pub fn switch_access_name(").next())
        .expect("settings_switch");
    assert!(
        switch.contains("WidgetType::Checkbox") && switch.contains("switch_access_name"),
        "the settings switch is a named checkbox: {switch}"
    );
    let seg = src
        .split("fn felt_segment_styled(")
        .nth(1)
        .and_then(|s| s.split("pub fn felt_tab(").next())
        .expect("segment");
    assert!(
        seg.contains("WidgetInfo::selected") && seg.contains("selected"),
        "a segment reports whether it is selected: {seg}"
    );
}

mod native_media_cards {
    use super::media_tool_cards;

    #[test]
    fn media_tool_cards_use_imagine_hero() {
        let image = media_tool_cards(
            "image_generate",
            "IMAGINE: /tmp/cabin.png\nIMAGINE: /tmp/b.png",
        )
        .unwrap();
        assert_eq!(image.len(), 2);
        assert!(!image[0].video);
        assert_eq!(image[0].path, "/tmp/cabin.png");
        let video = media_tool_cards("video_extend", "note\nIMAGINE: /tmp/clip.mp4").unwrap();
        assert!(video[0].video);
        assert_eq!(video[0].path, "/tmp/clip.mp4");
        assert!(media_tool_cards("web_fetch", "IMAGINE: /tmp/cabin.png").is_none());
        assert!(media_tool_cards("image_edit", "still working").is_none());
        let cards = include_str!("cards.rs");
        assert!(cards.contains("fn paint_native_media_card"));
        assert!(cards.contains("imagine_result_hero"));
        let app = include_str!("app/chat_paint.rs");
        let body = app
            .split("fn paint_tool_card_body(")
            .nth(1)
            .and_then(|s| s.split("fn eyes_frame_tex(").next())
            .expect("paint_tool_card_body");
        assert!(body.contains("paint_native_media_card"), "{body}");
        let rows = app
            .split("fn paint_tool_rows(")
            .nth(1)
            .and_then(|s| s.split("fn paint_tool_card_body(").next())
            .expect("paint_tool_rows");
        assert!(rows.contains("paint_native_media_card"), "{rows}");
    }
}
