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
