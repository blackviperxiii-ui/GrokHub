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
