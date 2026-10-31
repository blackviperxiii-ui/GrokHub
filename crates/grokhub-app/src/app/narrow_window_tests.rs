//! The cabin at its 420 px minimum width, the rail while Settings covers a
//! page, and the ledger tickers' in-memory cadence (QA batch C). Wired via
//! `#[path]` from `tests.rs` like the other stage-5 files.

use super::*;
use eframe::egui;

/// The narrowest the window goes (`minWidth` in the Tauri shell).
const MIN_W: f32 = 420.0;

/// One headless pass of `paint` at `w` px wide, run twice so centred
/// layouts settle, with the text shapes it left and where each one sits.
fn pass_texts(w: f32, mut paint: impl FnMut(&mut egui::Ui)) -> Vec<(String, egui::Rect)> {
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w, 640.0),
        )),
        ..Default::default()
    };
    let mut texts = Vec::new();
    for _ in 0..2 {
        texts.clear();
        let out = crate::theme::test_pass(&ctx, raw.clone(), &mut paint);
        for clipped in &out.shapes {
            text_rects(&clipped.shape, &mut texts);
        }
    }
    texts
}

fn text_rects(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => out.push((
            text.galley.job.text.clone(),
            text.galley.rect.translate(text.pos.to_vec2()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                text_rects(shape, out);
            }
        }
        _ => {}
    }
}

#[test]
fn the_window_controls_stay_on_screen_at_the_minimum_width() {
    let _paint = crate::theme::hold_paint_test();
    crate::theme::set_paint_palette(crate::theme::PaintPalette::OpsDark);
    let (root, mut cabin) = isolated_cabin("narrow-titlebar");
    let wide = pass_texts(1280.0, |ui| cabin.ui_titlebar(ui));
    let (bar_w, chrome_left, left_right) = super::sidebar::TITLEBAR_FIT
        .with(|f| f.get())
        .expect("the titlebar painted");
    assert_eq!(bar_w, 1280.0);
    assert!(chrome_left >= 900.0, "controls on the right: {chrome_left}");
    assert!(
        left_right >= 100.0 && left_right <= chrome_left,
        "{left_right}"
    );
    assert!(
        wide.iter().any(|(t, _)| t.starts_with("PERSONALITY · ")),
        "the pills paint when there is room: {wide:?}"
    );

    let narrow = pass_texts(MIN_W, |ui| cabin.ui_titlebar(ui));
    let (bar_w, chrome_left, left_right) = super::sidebar::TITLEBAR_FIT
        .with(|f| f.get())
        .expect("the titlebar painted");
    assert_eq!(bar_w, MIN_W);
    // Close, maximize and minimize are 46 px each and sit at the right edge.
    assert!(
        (0.0..=MIN_W - 3.0 * 46.0).contains(&chrome_left),
        "the three window controls fit: {chrome_left}"
    );
    assert!(
        left_right <= chrome_left,
        "the brand never runs under the controls: {left_right} > {chrome_left}"
    );
    assert!(
        !narrow.iter().any(|(t, _)| t.starts_with("PERSONALITY · ")),
        "a pill with no room is left out, not clipped: {narrow:?}"
    );
    for (text, rect) in &narrow {
        assert!(
            rect.left() >= 0.0 && rect.right() <= MIN_W,
            "{text:?} at {rect:?} leaves the window"
        );
    }
    release_isolated(&root, cabin);
}

#[test]
fn the_sign_in_card_wraps_inside_the_minimum_width() {
    let _paint = crate::theme::hold_paint_test();
    let (root, mut cabin) = isolated_cabin("narrow-get-started");
    cabin.cfg.get_started_done = false;
    assert!(cabin.ui_wants_get_started());
    let texts = pass_texts(MIN_W, |ui| {
        assert!(cabin.ui_get_started(ui), "the card took the page");
    });
    assert!(texts.iter().any(|(t, _)| t == "Get Started"), "{texts:?}");
    let body = texts
        .iter()
        .find(|(t, _)| t.starts_with("Connect your Super Grok account."))
        .expect("the connect line");
    assert!(
        body.1.left() >= 24.0 && body.1.right() <= MIN_W - 24.0,
        "the connect line wraps inside the card: {:?}",
        body.1
    );
    assert!(
        body.1.height() > 20.0,
        "at 420 px the line wraps onto more than one row: {:?}",
        body.1
    );
    for (text, rect) in &texts {
        assert!(
            rect.left() >= 0.0 && rect.right() <= MIN_W,
            "{text:?} at {rect:?} leaves the window"
        );
    }
    release_isolated(&root, cabin);
}

#[test]
fn the_rail_lights_the_gear_while_settings_covers_a_page() {
    let (root, mut cabin) = isolated_cabin("rail-settings");
    cabin.nav = Nav::Chat;
    assert_eq!(cabin.rail_active_id(), "chat");
    cabin.set_nav_id("settings");
    assert_eq!(cabin.nav, Nav::Settings);
    assert_eq!(
        cabin.nav_id(),
        "chat",
        "the page under Settings is still Chat"
    );
    assert_eq!(
        cabin.rail_active_id(),
        "settings",
        "but the rail lights the gear"
    );
    cabin.settings_menu_open = true;
    assert_eq!(
        cabin.rail_active_id(),
        "profile",
        "the avatar menu keeps its row"
    );
    cabin.settings_menu_open = false;
    cabin.nav = Nav::Pulse;
    assert_eq!(cabin.rail_active_id(), "pulse");
    release_isolated(&root, cabin);
}

#[test]
fn a_tick_before_its_cadence_reads_no_ledger() {
    let (root, mut cabin) = isolated_cabin("ticker-cadence");
    let reads = super::ledger_store::reads;
    // The first tick of each reads its ledger once (and may start a worker).
    let before = reads();
    cabin.tick_nudge_watchers();
    assert_eq!(
        reads(),
        before + 1,
        "nudge watchers: one read on the first tick"
    );
    cabin.tick_ask_offers();
    assert_eq!(
        reads(),
        before + 2,
        "ask offers: one read on the first tick"
    );
    cabin.tick_screenshot_offer();
    assert_eq!(
        reads(),
        before + 3,
        "screenshot offer: one read on the first tick"
    );
    cabin.tick_hobby_paths();
    let hobby = root.join(grokhub_core::hobby_paths::FILE_NAME);
    assert!(
        hobby.is_file(),
        "hobby paths: the first tick saved its ledger"
    );
    std::fs::remove_file(&hobby).unwrap();
    // The next frame is inside every cadence: no file is opened.
    let settled = reads();
    for _ in 0..3 {
        cabin.tick_nudge_watchers();
        cabin.tick_ask_offers();
        cabin.tick_screenshot_offer();
        cabin.tick_hobby_paths();
    }
    assert_eq!(reads(), settled, "a frame before the cadence reads nothing");
    assert!(
        !hobby.exists(),
        "hobby paths: a frame before the cadence writes nothing either"
    );
    // The week recap re-reads its file once a minute, not every housekeep.
    cabin.tick_week_recap();
    let after_first = reads();
    cabin.tick_week_recap();
    assert_eq!(reads(), after_first, "week recap: one read per minute");
    release_isolated(&root, cabin);
}
