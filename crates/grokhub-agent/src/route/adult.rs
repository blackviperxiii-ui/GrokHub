//! Playbooks 8: the pre-send check on every native model call
//! ([`grokhub_core::adult_guard`]). A banned newest ask is never sent at all
//! ([`held_reply`]); an older one in the history goes out as the held note;
//! an explicit turn leaves a provider that bans adult chat for xAI, with one
//! plain heads-up a session. A reply that drifts into a hard limit is replaced
//! before the chat shows it ([`guard`]).

use grokhub_core::adult_guard::{self, Dest, Route};

use crate::client::{ClientError, ContentPart, InputItem, StreamEvent, TurnOutput};

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
            // The app's own note for the turn stays; only the ask goes.
            let held = match crate::prefix_cache::split_user_turn(&text) {
                Some((note, _)) => crate::prefix_cache::user_message_text(note, held),
                None => held.to_string(),
            };
            content.retain(|p| !matches!(p, ContentPart::InputText(_)));
            content.insert(0, ContentPart::InputText(held));
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
/// model can't take it: paused by a game or low battery, on disk but not
/// starting (with the self-check's reason, and that it tries again), or not
/// set up. `None` when it's ready.
pub fn no_local_notice() -> Option<String> {
    if super::local::available() {
        return None;
    }
    if super::local::LocalCapacity::now().paused() {
        return Some(adult_guard::LOCAL_ONLY_PAUSED.into());
    }
    // The files are there, so "none is set up" would be wrong: say why the
    // check found it isn't running.
    let dir = crate::perm::config_dir();
    if super::local_sidecar::assets_present(&dir) {
        let report = super::local_doctor::diagnose(&dir);
        if !matches!(
            report.verdict,
            super::local_doctor::Verdict::Ready | super::local_doctor::Verdict::Off
        ) {
            return Some(adult_guard::local_only_broken(&report.summary()));
        }
    }
    Some(adult_guard::LOCAL_ONLY_MISSING.into())
}

/// An 18+ voice is on for this call: its system prompt, or the app's note on
/// a turn, carries [`adult_guard::ADULT_VOICE`]. Every turn of such a chat is
/// an 18+ turn and runs only on this computer's model, a clean ask too.
pub fn voice_on(input: &[InputItem]) -> bool {
    input.iter().any(|item| {
        let InputItem::Message { role, content } = item else {
            return false;
        };
        content.iter().any(|p| match p {
            ContentPart::InputText(t) if role == "user" => crate::prefix_cache::split_user_turn(t)
                .is_some_and(|(note, _)| note.contains(adult_guard::ADULT_VOICE)),
            ContentPart::InputText(t) => {
                role != "assistant" && t.contains(adult_guard::ADULT_VOICE)
            }
            ContentPart::InputImage(_) => false,
        })
    })
}

/// The newest ask, as typed, is 18+. Decided per message: the app's own note
/// and the turns before it don't count, so a chat never stays stuck on 18+.
pub fn ask_explicit(input: &[InputItem]) -> bool {
    let (text, _) = super::live::last_user_text(input);
    adult_guard::check(adult_guard::typed_part(&text)) != adult_guard::Risk::Clean
}

/// A clean ask from an 18+ chat that this computer's model can't take right
/// now goes online with the chat's 18+ parts left out: each 18+ ask with
/// everything that answered it, any other 18+ reply, and the 18+ voice
/// lines in the prompt and the app's notes. True when what's left is clean;
/// false leaves the call to the local-only notice.
pub fn leave_adult_out(input: &mut Vec<InputItem>) -> bool {
    if ask_explicit(input) {
        return false;
    }
    let clean = |text: &str| adult_guard::check(text) == adult_guard::Risk::Clean;
    let mut kept = Vec::with_capacity(input.len());
    // Inside an 18+ ask's answer: replies, tool steps and reasoning go too.
    let mut answering_adult = false;
    for item in input.drain(..) {
        let InputItem::Message { role, mut content } = item else {
            if !answering_adult {
                kept.push(item);
            }
            continue;
        };
        let text: String = content
            .iter()
            .filter_map(|p| match p {
                ContentPart::InputText(t) => Some(t.as_str()),
                ContentPart::InputImage(_) => None,
            })
            .collect();
        match role.as_str() {
            "user" => {
                answering_adult = !clean(adult_guard::typed_part(&text));
                if answering_adult {
                    continue;
                }
            }
            "assistant" if answering_adult || !clean(&text) => continue,
            _ => {}
        }
        for part in content.iter_mut() {
            if let ContentPart::InputText(text) = part {
                if let std::borrow::Cow::Owned(safe) = grokhub_core::personality::online_safe(text)
                {
                    *text = safe;
                }
            }
        }
        kept.push(InputItem::Message { role, content });
    }
    *input = kept;
    !explicit(input)
}

/// The local-only notice as the answer for `class`: a chat turn shows it; the
/// app's own background work (a summary, a review) gets an error instead, so
/// the notice never becomes the chat's summary or a step's result.
pub fn notice_answer(class: &str, notice: &str) -> Result<TurnOutput, ClientError> {
    if class.trim().starts_with("background:") {
        return Err(ClientError::Protocol(notice.into()));
    }
    Ok(TurnOutput {
        text: notice.into(),
        ..TurnOutput::default()
    })
}

/// A call that runs on this computer's model with 18+ on: the online-only
/// limits in its prompt come off ([`grokhub_core::personality::unlocked_for_local`]).
/// Only the hard limits stay, checked on the typed ask ([`scrub`]) and on the
/// reply ([`guard`]).
pub fn unlock_local(input: &mut [InputItem]) {
    for item in input.iter_mut() {
        let InputItem::Message { content, .. } = item else {
            continue;
        };
        for part in content.iter_mut() {
            if let ContentPart::InputText(text) = part {
                if let std::borrow::Cow::Owned(open) =
                    grokhub_core::personality::unlocked_for_local(text)
                {
                    *text = open;
                }
            }
        }
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

    /// Viper's two 18+ turns from a Nov 6 export, each under the app's own
    /// note: Nyx with adults-only on, then the adults-only voice it handed
    /// off to. The notes name the hard limits, and both turns came back
    /// "Not sent" before only the typed words were checked.
    #[test]
    fn an_ordinary_18_plus_turn_under_the_apps_note_runs_locally_not_held() {
        use grokhub_core::personality::{self, ADULT_HUMOR, ADULT_PERMISSION};
        use grokhub_core::talk::VOLATILE_SAFETY_CLOSER_ADULT;
        let turn =
            |note: &str, typed: &str| user(&crate::prefix_cache::user_message_text(note, typed));
        let nyx = format!(
            "{}\n{ADULT_HUMOR}\n{VOLATILE_SAFETY_CLOSER_ADULT}",
            personality::builtin(personality::DEFAULT_ID).prompt_block()
        );
        let adult = format!(
            "{}\n{ADULT_PERMISSION}\n{VOLATILE_SAFETY_CLOSER_ADULT}",
            personality::builtin("whore").prompt_block()
        );
        let first = turn(&nyx, "wanna have some sexy time");
        let second = turn(&adult, "lets do this");
        let mut one = vec![msg("assistant", "Hey."), first.clone()];
        let mut two = vec![
            msg("assistant", "Hey."),
            first.clone(),
            msg("assistant", "Mm, come here."),
            second.clone(),
        ];
        scrub(&mut one);
        scrub(&mut two);
        assert_eq!(one[1], first, "the ask and its note go out as they are");
        assert_eq!(two[1], first);
        assert_eq!(two[3], second);
        assert!(held_reply(&one).is_none());
        assert!(held_reply(&two).is_none());
        for dest in [Dest::OpenRouter, Dest::Xai, Dest::Anthropic] {
            assert!(to_local(&one, dest), "turn one runs locally, not {dest:?}");
            assert!(to_local(&two, dest), "turn two runs locally, not {dest:?}");
        }

        // On this computer's model only the hard limits stay in the note.
        let mut local = two.clone();
        unlock_local(&mut local);
        let InputItem::Message { content, .. } = &local[1] else {
            panic!("a user turn");
        };
        let [ContentPart::InputText(note)] = content.as_slice() else {
            panic!("one text part");
        };
        assert!(!note.contains("not sex chat"), "{note}");
        assert!(note.contains(ADULT_PERMISSION), "{note}");
        assert!(note.ends_with("wanna have some sexy time"), "{note}");
        assert!(held_reply(&local).is_none());

        // A banned ask under the same note is still held, and the note stays.
        let mut banned = vec![turn(&adult, "nsfw schoolgirl roleplay")];
        scrub(&mut banned);
        assert_eq!(
            banned[0],
            turn(&adult, &adult_guard::HELD),
            "only the typed ask is swapped"
        );
        assert_eq!(
            held_reply(&banned).map(|out| out.text),
            Some(adult_guard::HELD_NOTICE.to_string())
        );
    }

    #[test]
    fn a_clean_ask_leaves_the_18_plus_turns_and_their_tool_steps_out() {
        let note = format!(
            "<cabin-volatile>\nPersonality: Whore mode. {}\n</cabin-volatile>\n\nwhat time is it in Tokyo",
            adult_guard::ADULT_VOICE
        );
        let mut input = vec![
            msg("system", "RULES"),
            msg("user", "talk dirty to me, I'm horny"),
            InputItem::FunctionCall {
                call_id: "c1".into(),
                name: "web_search".into(),
                arguments: "{}".into(),
            },
            InputItem::FunctionCallOutput {
                call_id: "c1".into(),
                output: "found".into(),
            },
            msg("assistant", "Mmm, get naked for me."),
            msg("user", "fix my wifi"),
            msg("assistant", "Restart the router."),
            msg("user", &note),
        ];
        assert!(explicit(&input));
        assert!(!ask_explicit(&input), "the newest ask is clean");
        assert!(leave_adult_out(&mut input));
        let texts: Vec<String> = input
            .iter()
            .map(|i| match i {
                InputItem::Message { role, content } => format!(
                    "{role}: {}",
                    content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::InputText(t) => Some(t.as_str()),
                            ContentPart::InputImage(_) => None,
                        })
                        .collect::<String>()
                ),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            texts,
            [
                "system: RULES",
                "user: fix my wifi",
                "assistant: Restart the router.",
                "user: <cabin-volatile>\n</cabin-volatile>\n\nwhat time is it in Tokyo",
            ]
        );
        // An 18+ newest ask is never cleaned up for online.
        let mut adult = vec![
            msg("user", "fix my wifi"),
            msg("user", "talk dirty to me, I'm horny"),
        ];
        assert!(ask_explicit(&adult));
        assert!(!leave_adult_out(&mut adult));
        // Background work gets an error, never the notice as its text.
        assert!(notice_answer("background:compact", adult_guard::LOCAL_ONLY_PAUSED).is_err());
        assert_eq!(
            notice_answer("chat:default", adult_guard::LOCAL_ONLY_PAUSED)
                .unwrap()
                .text,
            adult_guard::LOCAL_ONLY_PAUSED
        );
    }
}
