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
fn home_deck_mc_uses_slide_nine_layout() {
    let src = include_str!("home/page.rs");
    let body = src
        .split("home-mission-body")
        .nth(1)
        .expect("home mission scroll body");
    assert!(
        body.contains("deck_home_mc_shot") && body.contains("paint_mission_control_body"),
        "deck home-mc must paint slide 09 layout A, not the modular grid"
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
