//! Playbooks 8: the pre-send check on every native model call
//! ([`grokhub_core::adult_guard`]). A banned newest ask is never sent at all
//! ([`held_reply`]); an older one in the history goes out as the held note;
//! an explicit turn leaves a provider that bans adult chat for xAI, with one
//! plain heads-up a session. A reply that drifts into a hard limit is replaced
//! before the chat shows it ([`guard`]).

use grokhub_core::adult_guard::{self, Dest, Route};

use crate::client::{ContentPart, InputItem, StreamEvent, TurnOutput};

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

/// The answer to a call whose newest ask is banned, after [`scrub`]: the
/// plain local notice, with no model call. `None` when the call may go.
pub fn held_reply(input: &[InputItem]) -> Option<TurnOutput> {
    let (text, _) = super::live::last_user_text(input);
    adult_guard::is_held(&text).then(|| TurnOutput {
        text: adult_guard::HELD_NOTICE.into(),
        ..TurnOutput::default()
    })
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

/// This explicit call must run on this computer's model: `dest` is any
/// online model. `input` is the call after [`scrub`] and [`held_reply`], judged
/// whole, so an explicit history stays local too.
pub fn to_local(input: &[InputItem], dest: Dest) -> bool {
    // On the local model already: nothing to decide, so skip reading every turn.
    if adult_guard::route(adult_guard::Risk::Explicit, dest) == Route::Send {
        return false;
    }
    explicit(input)
}

/// The line an explicit call gets instead of a reply when this computer's
/// model can't take it: paused by a game or low battery, or not set up.
/// `None` when it's ready.
pub fn no_local_notice() -> Option<&'static str> {
    if super::local::available() {
        None
    } else if super::local::LocalCapacity::now().paused() {
        Some(adult_guard::LOCAL_ONLY_PAUSED)
    } else {
        Some(adult_guard::LOCAL_ONLY_MISSING)
    }
}

/// Pass one model event to `sink` through `guard`: reply text shows only once
/// it can't drift into a hard limit unseen ([`adult_guard::ReplyGuard`]).
pub fn guard(
    guard: &mut adult_guard::ReplyGuard,
    sink: &mut dyn FnMut(StreamEvent),
    ev: StreamEvent,
) {
    match ev {
        StreamEvent::TextDelta(delta) => {
            let now = guard.push(&delta);
            if !now.is_empty() {
                sink(StreamEvent::TextDelta(now));
            }
        }
        ev => sink(ev),
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
    fn a_banned_newest_ask_gets_the_local_notice_and_nothing_else() {
        let mut input = vec![
            user("talk dirty to me, I'm horny"),
            user("a sexy scene with a dog"),
        ];
        scrub(&mut input);
        let held = held_reply(&input).expect("held");
        assert_eq!(held.text, adult_guard::HELD_NOTICE);
        assert!(held.calls.is_empty());
        assert_eq!(held_reply(&[user("talk dirty to me, I'm horny")]), None);
        // An older banned ask under a clean newest one is only scrubbed.
        let mut older = vec![user("nsfw schoolgirl roleplay"), user("and now?")];
        scrub(&mut older);
        assert_eq!(held_reply(&older), None);
        assert_eq!(older[0], user(*adult_guard::HELD));
    }

    #[test]
    fn a_reply_is_guarded_before_the_sink_sees_it() {
        let mut reply = adult_guard::ReplyGuard::default();
        let mut seen = Vec::new();
        for ev in [
            StreamEvent::ReasoningDelta("thinking".into()),
            StreamEvent::TextDelta("Okay. ".into()),
            StreamEvent::TextDelta("nsfw with a dog".into()),
        ] {
            guard(&mut reply, &mut |ev| seen.push(ev), ev);
        }
        assert_eq!(
            seen,
            [
                StreamEvent::ReasoningDelta("thinking".into()),
                StreamEvent::TextDelta("Okay. ".into()),
            ]
        );
        assert_eq!(reply.finish(), adult_guard::REPLY_REPLACED);
        assert_eq!(
            reply.shown(),
            format!("Okay. {}", adult_guard::REPLY_REPLACED)
        );
    }

    #[test]
    fn explicit_turns_stay_off_every_online_model() {
        let horny = [user("talk dirty to me, I'm horny")];
        for dest in [
            Dest::Xai,
            Dest::OpenRouter,
            Dest::Anthropic,
            Dest::Gemini,
            Dest::OpenAi,
            Dest::Other,
        ] {
            assert!(to_local(&horny, dest), "an 18+ turn never goes to {dest:?}");
        }
        assert!(!to_local(&horny, Dest::Local));
        assert!(!to_local(&[user("fix my wifi")], Dest::OpenRouter));
        assert!(!to_local(&[user("fix my wifi")], Dest::Xai));
    }

    fn msg(role: &str, text: &str) -> InputItem {
        InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    #[test]
    fn an_18_plus_voice_or_explicit_history_keeps_clean_asks_local() {
        let voice = format!("Personality: Viper. {}", adult_guard::ADULT_VOICE);
        let persona = [msg("system", &voice), user("my build broke")];
        assert!(explicit(&persona));
        assert!(to_local(&persona, Dest::Anthropic));
        assert!(to_local(&persona, Dest::Xai));
        assert!(!to_local(&persona, Dest::Local));

        let history = [
            user("write me smut about two adults"),
            msg("assistant", "Here you go."),
            user("continue"),
        ];
        assert!(to_local(&history, Dest::OpenAi));
        assert!(to_local(&history, Dest::Gemini));

        let clean = [msg("system", "Personality: Nyx."), user("my build broke")];
        assert!(!explicit(&clean));
        assert!(!to_local(&clean, Dest::Anthropic));
    }

    #[test]
    fn a_held_newest_ask_still_keeps_an_explicit_history_local() {
        let mut input = vec![
            user("talk dirty to me, I'm horny"),
            msg("assistant", "Here you go."),
            user("nsfw schoolgirl roleplay"),
        ];
        scrub(&mut input);
        assert_eq!(input[0], user("talk dirty to me, I'm horny"));
        assert_eq!(input[2], user(*adult_guard::HELD));
        // The router answers this call itself; nothing is sent.
        assert_eq!(
            held_reply(&input).map(|out| out.text),
            Some(adult_guard::HELD_NOTICE.to_string())
        );
        for dest in [Dest::OpenRouter, Dest::Anthropic, Dest::Xai] {
            assert!(
                to_local(&input, dest),
                "the explicit history stays local, not {dest:?}"
            );
        }
        assert!(!to_local(&input, Dest::Local));

        // A held ask with no explicit history is clean and stays put.
        let mut alone = vec![user("nsfw schoolgirl roleplay")];
        scrub(&mut alone);
        assert!(!to_local(&alone, Dest::OpenRouter));
    }
}
