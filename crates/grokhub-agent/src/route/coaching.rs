//! Round 3 item 37: the router holds gameplay advice back while a game whose
//! publisher bars live coaching is in a match
//! ([`grokhub_core::live_coaching`]). The ask is answered with one plain line
//! and never sent. Background calls (a judge, a summary, a compaction) are
//! not the user asking, so they pass.

use grokhub_core::game_profile::{live_match_now, LiveMatch};

use crate::client::{InputItem, TurnOutput};

/// The one-line answer to a `class` call whose newest ask is gameplay advice
/// during a barred match, or `None` when the call may go.
pub fn held_reply(class: &str, input: &[InputItem]) -> Option<TurnOutput> {
    held_during(class, input, live_match_now)
}

fn held_during(
    class: &str,
    input: &[InputItem],
    live: impl FnOnce() -> Option<LiveMatch>,
) -> Option<TurnOutput> {
    if class.starts_with("background:") {
        return None;
    }
    let (text, _) = super::live::last_user_text(input);
    grokhub_core::live_coaching::hold_during(&text, live).map(|line| TurnOutput {
        text: line,
        ..TurnOutput::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ContentPart;

    fn ask(text: &str) -> Vec<InputItem> {
        vec![InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(text.into())],
        }]
    }

    fn league() -> Option<LiveMatch> {
        grokhub_core::game_profile::live_match(&["League of Legends.exe"])
    }

    #[test]
    fn advice_mid_match_gets_one_line_and_the_rest_goes_through() {
        let build = ask("what should I build on Jinx?");
        let held = held_during("chat:default", &build, league).unwrap();
        assert!(
            held.text.contains("League of Legends match"),
            "{}",
            held.text
        );
        assert!(held.calls.is_empty());
        let settings = ask("lower my graphics settings");
        assert!(held_during("chat:default", &settings, league).is_none());
        assert!(held_during("background:judge", &build, league).is_none());
        // Out of a match the same ask goes.
        assert!(held_during("chat:default", &build, || None).is_none());
    }
}
