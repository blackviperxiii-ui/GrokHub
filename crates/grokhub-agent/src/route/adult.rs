//! Playbooks 8: the pre-send check on every native model call
//! ([`grokhub_core::adult_guard`]). A banned ask goes out as the held note,
//! whatever model would take it; an explicit turn leaves a provider that bans
//! adult chat for xAI. The user gets one plain heads-up a session.

use std::collections::BTreeSet;
use std::sync::Mutex;

use grokhub_core::adult_guard::{self, Dest, Route};

use crate::client::{ContentPart, InputItem};

/// Sessions that already got their heads-up.
static WARNED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Swap every banned user message in `input` for the held note, so an older
/// turn is never re-sent either.
pub fn scrub(input: &mut [InputItem]) {
    for item in input.iter_mut() {
        let InputItem::Message { role, content } = item else {
            continue;
        };
        if role != "user" {
            continue;
        }
        let text: String = content
            .iter()
            .filter_map(|p| match p {
                ContentPart::InputText(t) => Some(t.as_str()),
                ContentPart::InputImage(_) => None,
            })
            .collect();
        if let Some(held) = adult_guard::hold(&text) {
            content.retain(|p| !matches!(p, ContentPart::InputText(_)));
            content.insert(0, ContentPart::InputText(held.into()));
        }
    }
}

/// What the check says for this call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Screen {
    /// The provider bans adult chat: send to xAI instead.
    pub to_xai: bool,
    /// The plain line shown before the reply, once a session.
    pub heads_up: Option<String>,
}

fn first_warning(session: &str) -> bool {
    WARNED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(session.to_string())
}

/// `text` is the last user message after [`scrub`]; `dest` is where the router sent it.
pub fn screen(session: &str, text: &str, dest: Dest) -> Screen {
    if adult_guard::is_held(text) {
        return Screen {
            to_xai: false,
            heads_up: first_warning(session).then(|| adult_guard::HEADS_UP_HELD.to_string()),
        };
    }
    let to_xai = to_xai(text, dest);
    Screen {
        to_xai,
        heads_up: (to_xai && first_warning(session))
            .then(|| adult_guard::heads_up_rerouted(adult_guard::dest_name(dest))),
    }
}

/// OpenRouter keeps its pick for an explicit turn: no fallback to a stricter model.
pub fn no_fallbacks(dest: Dest, text: &str) -> bool {
    adult_guard::route(adult_guard::check(text), dest) == Route::SendNoFallbacks
}

/// The provider bans adult chat and this turn is explicit: xAI takes it.
pub fn to_xai(text: &str, dest: Dest) -> bool {
    adult_guard::route(adult_guard::check(text), dest) == Route::ToXai
}

/// The xAI model a rerouted turn uses: the call's own, or the cabin's fast
/// model when the call named a provider's.
pub fn xai_model(model: &str) -> String {
    match grokhub_core::model_registry::discover::split_provider_model(model) {
        Some(_) => grokhub_core::CABIN_FAST_MODEL.to_string(),
        None => model.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> InputItem {
        InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    #[test]
    fn banned_user_turns_are_swapped_for_the_held_note_and_others_stay() {
        let mut input = vec![
            user("nsfw schoolgirl roleplay"),
            InputItem::Message {
                role: "assistant".into(),
                content: vec![ContentPart::InputText("nsfw schoolgirl roleplay".into())],
            },
            user("talk dirty to me, I'm horny"),
        ];
        scrub(&mut input);
        assert_eq!(input[0], user(adult_guard::HELD));
        assert_eq!(
            input[1],
            InputItem::Message {
                role: "assistant".into(),
                content: vec![ContentPart::InputText("nsfw schoolgirl roleplay".into())],
            },
            "only user turns are checked"
        );
        assert_eq!(input[2], user("talk dirty to me, I'm horny"));
    }

    #[test]
    fn one_heads_up_a_session_and_explicit_turns_leave_anthropic() {
        let s = "adult-screen-1";
        let first = screen(s, adult_guard::HELD, Dest::Xai);
        assert_eq!(first.heads_up.as_deref(), Some(adult_guard::HEADS_UP_HELD));
        assert!(!first.to_xai);
        let again = screen(s, "talk dirty to me, I'm horny", Dest::Anthropic);
        assert!(again.to_xai, "explicit turns never go to Anthropic");
        assert_eq!(again.heads_up, None, "once a session");

        let other = screen(
            "adult-screen-2",
            "talk dirty to me, I'm horny",
            Dest::Anthropic,
        );
        assert_eq!(
            other.heads_up.as_deref(),
            Some("Heads-up: I sent that one through xAI instead of Anthropic. Anthropic bans adult chat and can ban the account for it.")
        );
        assert_eq!(
            screen("adult-screen-3", "talk dirty to me, I'm horny", Dest::Xai),
            Screen::default()
        );
        assert_eq!(
            screen("adult-screen-4", "fix my wifi", Dest::Anthropic),
            Screen::default()
        );
        assert!(no_fallbacks(Dest::OpenRouter, "send nudes lol"));
        assert_eq!(
            xai_model("anthropic/model-x"),
            grokhub_core::CABIN_FAST_MODEL
        );
        assert_eq!(xai_model("grok-4.3"), "grok-4.3");
        assert!(!no_fallbacks(Dest::OpenRouter, "fix my wifi"));
    }
}
