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
    /// The line shown instead of a rerouted `heads_up` when xAI couldn't take
    /// the turn and this computer's model answered it.
    pub heads_up_local: Option<String>,
}

fn first_warning(session: &str) -> bool {
    WARNED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(session.to_string())
}

/// `input` is the call after [`scrub`]; `dest` is where the router sent it.
/// Where the call goes is judged on the whole scrubbed input, so an explicit
/// history stays off a banning provider even when the newest ask was held; the
/// held ask only picks which heads-up is shown.
pub fn screen(session: &str, input: &[InputItem], dest: Dest) -> Screen {
    let to_xai = to_xai(input, dest);
    let (text, _) = super::live::last_user_text(input);
    if adult_guard::is_held(&text) {
        return Screen {
            to_xai,
            heads_up: first_warning(session).then(|| adult_guard::HEADS_UP_HELD.to_string()),
            heads_up_local: None,
        };
    }
    let warn = to_xai && first_warning(session);
    let name = adult_guard::dest_name(dest);
    Screen {
        to_xai,
        heads_up: warn.then(|| adult_guard::heads_up_rerouted(name)),
        heads_up_local: warn.then(|| adult_guard::heads_up_local(name)),
    }
}

/// The whole call is explicit: an 18+ voice in its system prompt, or any
/// explicit user or assistant turn, not only the newest ask.
pub fn explicit(input: &[InputItem]) -> bool {
    let turns: Vec<(&str, String)> = input
        .iter()
        .filter_map(|item| match item {
            InputItem::Message { role, content } => Some((
                role.as_str(),
                content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::InputText(t) => Some(t.as_str()),
                        ContentPart::InputImage(_) => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            )),
            _ => None,
        })
        .collect();
    adult_guard::explicit_conversation(turns.iter().map(|(r, t)| (*r, t.as_str())))
}

/// The provider bans adult chat (or, like OpenRouter, may hand the turn to a
/// model that does) and this call is explicit: xAI takes it.
pub fn to_xai(input: &[InputItem], dest: Dest) -> bool {
    let risk = if explicit(input) {
        adult_guard::Risk::Explicit
    } else {
        adult_guard::Risk::Clean
    };
    adult_guard::route(risk, dest) == Route::ToXai
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
        assert_eq!(input[0], user(*adult_guard::HELD));
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
        let first = screen(s, &[user(*adult_guard::HELD)], Dest::Xai);
        assert_eq!(first.heads_up.as_deref(), Some(adult_guard::HEADS_UP_HELD));
        assert!(!first.to_xai);
        let horny = [user("talk dirty to me, I'm horny")];
        let again = screen(s, &horny, Dest::Anthropic);
        assert!(again.to_xai, "explicit turns never go to Anthropic");
        assert_eq!(again.heads_up, None, "once a session");

        let other = screen("adult-screen-2", &horny, Dest::Anthropic);
        assert_eq!(
            other.heads_up.as_deref(),
            Some("Heads-up: I sent that one through xAI instead of Anthropic. Anthropic bans adult chat and can ban the account for it.")
        );
        assert_eq!(
            screen("adult-screen-3", &horny, Dest::Xai),
            Screen::default()
        );
        assert_eq!(
            screen("adult-screen-4", &[user("fix my wifi")], Dest::Anthropic),
            Screen::default()
        );
        // OpenRouter can land on a model whose maker bans adult chat: xAI takes it.
        assert!(to_xai(&horny, Dest::OpenRouter));
        assert!(!to_xai(&[user("fix my wifi")], Dest::OpenRouter));
        assert_eq!(
            xai_model("anthropic/model-x"),
            grokhub_core::CABIN_FAST_MODEL
        );
        assert_eq!(xai_model("grok-4.3"), "grok-4.3");
    }

    fn msg(role: &str, text: &str) -> InputItem {
        InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    #[test]
    fn an_18_plus_voice_or_explicit_history_keeps_clean_asks_off_banning_providers() {
        let voice = format!("Personality: Viper. {}", adult_guard::ADULT_VOICE);
        let persona = [msg("system", &voice), user("my build broke")];
        assert!(explicit(&persona));
        assert!(screen("adult-screen-5", &persona, Dest::Anthropic).to_xai);
        assert!(!screen("adult-screen-6", &persona, Dest::Local).to_xai);

        let history = [
            user("write me smut about two adults"),
            msg("assistant", "Here you go."),
            user("continue"),
        ];
        assert!(screen("adult-screen-7", &history, Dest::OpenAi).to_xai);
        assert!(screen("adult-screen-8", &history, Dest::Gemini).to_xai);

        let clean = [msg("system", "Personality: Nyx."), user("my build broke")];
        assert!(!explicit(&clean));
        assert!(!screen("adult-screen-9", &clean, Dest::Anthropic).to_xai);
    }

    #[test]
    fn a_held_newest_ask_still_keeps_an_explicit_history_off_banning_providers() {
        let mut input = vec![
            user("talk dirty to me, I'm horny"),
            msg("assistant", "Here you go."),
            user("nsfw schoolgirl roleplay"),
        ];
        scrub(&mut input);
        assert_eq!(input[0], user("talk dirty to me, I'm horny"));
        assert_eq!(input[2], user(*adult_guard::HELD));
        for dest in [Dest::OpenRouter, Dest::Anthropic] {
            let s = screen("adult-screen-10", &input, dest);
            assert!(s.to_xai, "the explicit history goes to xAI, not {dest:?}");
        }
        let first = screen("adult-screen-11", &input, Dest::OpenRouter);
        assert_eq!(first.heads_up.as_deref(), Some(adult_guard::HEADS_UP_HELD));
        assert_eq!(first.heads_up_local, None);
        assert!(!screen("adult-screen-12", &input, Dest::Local).to_xai);

        // A held ask with no explicit history is clean and stays put.
        let mut alone = vec![user("nsfw schoolgirl roleplay")];
        scrub(&mut alone);
        assert!(!screen("adult-screen-13", &alone, Dest::OpenRouter).to_xai);
    }
}
