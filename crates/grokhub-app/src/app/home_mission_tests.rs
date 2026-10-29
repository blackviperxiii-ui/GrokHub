//! Mission Control Home: Send/chips leave Home for Chat.

use super::*;

#[test]
fn send_home_to_chat_leaves_home_for_chat() {
    let mut quiet = QuietCabin::boot("home-send-chat");
    let cabin = &mut quiet.cabin;
    cabin.open_mission_home();
    assert_eq!(cabin.nav, Nav::Home, "start on Mission Control Home");

    cabin.send_home_to_chat("Fix my stutter".into());

    assert_eq!(
        cabin.nav,
        Nav::Chat,
        "Send from Home must open Chat (new tab), not stay on Home"
    );
    // No Grok binary in QuietCabin — the prompt is refused, but nav still moved.
    assert_eq!(cabin.status, "Sign in with Grok or add an API key.");
    quiet.settle();
}

#[test]
fn home_deck_shot_paints_the_same_module_grid() {
    let src = include_str!("home/page.rs");
    let body = src
        .split("fn ui_home_mission(")
        .nth(1)
        .and_then(|s| s.split("fn home_header(").next())
        .expect("ui_home_mission");
    assert!(
        body.contains("place_slots") && body.contains("paint_module(self, ui, id, slot.size)"),
        "Home paints the module grid: {body}"
    );
    assert!(
        !body.contains("deck_home_mc_shot") && !body.contains("paint_mission_control_body"),
        "a deck shot must not swap in a layout real users never get: {body}"
    );
}

#[test]
fn objectives_is_the_details_page_without_made_up_stats() {
    let page = include_str!("mission_page.rs");
    let paint = include_str!("home/paint.rs");
    for src in [page, paint] {
        assert!(!src.contains("+12%"), "no hardcoded win rate");
        assert!(!src.contains("Coach:"), "no coach line without a real stat");
    }
    let control = page
        .split("fn ui_mission_control(")
        .nth(1)
        .and_then(|s| s.split("fn mission_header(").next())
        .expect("ui_mission_control");
    assert!(
        control.contains("mission_objectives") && !control.contains("Up next"),
        "Objectives shows the checklist, not a second Home: {control}"
    );
    assert!(
        !page.contains("fn mission_pc(") && !page.contains("fn mission_compose("),
        "the PC card and composer live on Home only"
    );
    let objectives = paint
        .split("fn paint_objectives(")
        .nth(1)
        .and_then(|s| s.split("\nfn ").next())
        .expect("paint_objectives");
    assert!(
        objectives.contains("ModuleAction::Nav(\"objectives\")"),
        "Home's objectives card opens the Objectives details: {objectives}"
    );
    let details = objectives.find("\"Details\"").expect("Details button");
    let list = objectives
        .find("paint_objective_list(")
        .expect("objective list");
    assert!(
        details < list,
        "Details sits above the checklist so a long list in a clipped slot can't hide it"
    );
}

#[test]
fn deck_home_mc_keeps_activity_dock_collapsed() {
    let src = include_str!("activity_dock.rs");
    assert!(
        src.contains("deck_nav_home_pinned"),
        "deck home-mc must not open the Activity popover over Mission Control"
    );
}

#[test]
fn home_chip_labels_match_mission_control_preset() {
    let src = include_str!("home/page.rs");
    let chips = src
        .split("HOME_CHIPS:")
        .nth(1)
        .and_then(|s| s.split("];").next())
        .expect("HOME_CHIPS array");
    for label in [
        "Fix my stutter",
        "Find a duo tonight",
        "Free up storage",
        "Plan my week",
    ] {
        assert!(
            chips.contains(label),
            "Mission Control chips must include {label}: {chips}"
        );
    }
}

/// Every text on one Home frame of a fresh profile: (text, on-screen rect, clip).
/// With `wheel`, the mouse wheel then scrolls down over the module grid.
fn home_texts(w: f32, h: f32, wheel: bool) -> Vec<(String, egui::Rect, egui::Rect)> {
    fn walk(
        shape: &egui::Shape,
        clip: egui::Rect,
        out: &mut Vec<(String, egui::Rect, egui::Rect)>,
    ) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.job.text.clone(),
                shape.visual_bounding_rect(),
                clip,
            )),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, clip, out)),
            _ => {}
        }
    }
    let mut quiet = QuietCabin::boot("home-grid-fit");
    let cabin = &mut quiet.cabin;
    cabin.home_layout = grokhub_core::home::HomeLayout::default();
    let _paint = crate::theme::hold_paint_test();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w, h),
        )),
        ..Default::default()
    };
    let mut texts = Vec::new();
    // The first frame measures the composer; the next ones lay out with it.
    for frame in 0..24 {
        if frame == 3 && !wheel {
            break;
        }
        let mut raw = raw.clone();
        raw.time = Some(frame as f64 / 10.0);
        if frame == 3 {
            let over_grid = egui::pos2(w / 2.0, 200.0);
            raw.events = vec![
                egui::Event::PointerMoved(over_grid),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -300.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
        }
        texts.clear();
        let out = crate::theme::test_pass(&ctx, raw, |ui| cabin.ui_home_mission(ui));
        for c in &out.shapes {
            walk(&c.shape, c.clip_rect, &mut texts);
        }
    }
    quiet.settle();
    texts
}

fn fully_shown(texts: &[(String, egui::Rect, egui::Rect)], text: &str, h: f32) -> egui::Rect {
    let (_, rect, clip) = texts
        .iter()
        .find(|(t, _, _)| t == text)
        .unwrap_or_else(|| panic!("{text:?} not painted: {texts:?}"));
    assert!(
        clip.contains_rect(*rect) && rect.max.y <= h,
        "{text:?} is clipped: {rect:?} in {clip:?}, screen height {h}"
    );
    *rect
}

#[test]
fn fresh_home_fits_without_clipping_at_both_qa_sizes() {
    // Content area of a 1280×800 and a 900×700 cabin (rail and title bar off).
    for (w, h) in [(1060.0, 740.0), (700.0, 640.0)] {
        let texts = home_texts(w, h, false);
        let hint = fully_shown(&texts, "Assign an objective or ask anything…", h);
        assert!(
            hint.max.y <= h - 24.0,
            "composer sits above the page padding at {w}x{h}"
        );
        fully_shown(&texts, "UP NEXT", h);
        fully_shown(&texts, "Games you play with Steam show up here.", h);
        fully_shown(&texts, "WEEKLY STREAK", h);
        fully_shown(&texts, "Finish an objective today to start your streak.", h);
        let pc = fully_shown(&texts, "PC LOADOUT", h);
        // The grid fits, so the wheel has nothing to scroll (no stray scrollbar).
        let scrolled = home_texts(w, h, true);
        assert_eq!(
            fully_shown(&scrolled, "PC LOADOUT", h),
            pc,
            "grid scrolled at {w}x{h}"
        );
    }
}
