use super::*;
use crate::game_profile::{live_match, MATCH_RULES};

#[test]
fn gameplay_asks_are_advice() {
    for ask in [
        "how do I beat Yasuo mid?",
        "what should I build on Jinx",
        "should I buy or save this round",
        "best way to retake B site",
        "tips for laning against Darius",
        "where should I ward",
        "how do I counter their comp?",
        "Is it time to push top?",
    ] {
        assert!(gameplay_advice(ask), "{ask}");
    }
}

#[test]
fn settings_post_game_and_other_asks_go_through() {
    for ask in [
        "what sensitivity should I use",
        "my fps drops in teamfights, how do I fix it",
        "best crosshair settings for VALORANT",
        "review my last game, why did we lose that fight",
        "what are the patch notes for the new build",
        "remind me to stretch after this",
        "what's the weather tomorrow",
        "push the branch to beta",
        "",
    ] {
        assert!(!gameplay_advice(ask), "{ask}");
    }
}

#[test]
fn the_refusal_names_the_game_and_what_stays_open() {
    let m = live_match(&["League of Legends.exe"]).unwrap();
    let line = refusal(&m);
    assert!(
        line.starts_with("Riot Games doesn't allow live coaching during a League of Legends match")
    );
    assert!(line.contains("Settings help"));
    assert!(!line.contains('\n'));
    assert_eq!(MATCH_RULES[0].publisher, "Riot Games");
}
