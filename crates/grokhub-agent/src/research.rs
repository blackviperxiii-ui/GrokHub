//! `/deep-research <query>` is one normal turn that works to a research
//! recipe. The person's line stays in the chat as they typed it, so their
//! words stay theirs; the recipe rides in that turn's notes only.

const COMMAND: &str = "/deep-research";

pub(crate) const RECIPE: &str =
    "Deep research: the person's message is /deep-research followed by \
what to research. Work in this order, then answer.\n\
\n\
1. Plan a few distinct questions that cover the query. Each question needs its own evidence.\n\
2. Research those questions with the search tools you were offered: web_search, x_search when \
listed, and web_fetch. Open the pages you rely on with web_fetch. Do not call a tool that is \
not in your tool list.\n\
3. Check claims against more than one source before you keep them.\n\
4. Write a cited report. Mark each claim with its source and list the sources at the end.";

/// The text after `/deep-research`, in any letter case, when `line` is that
/// command. `/deep-research-extra` is not.
fn command_query(line: &str) -> Option<&str> {
    let head = line.get(..COMMAND.len())?;
    if !head.eq_ignore_ascii_case(COMMAND) {
        return None;
    }
    let rest = &line[COMMAND.len()..];
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// The recipe for a turn whose message is `/deep-research <query>`, read past
/// a card's carried context. `None` for any other message, or with nothing
/// to research.
pub fn turn_note(user_text: &str) -> Option<&'static str> {
    let words =
        grokhub_core::card_report::split_card_carry(user_text).map_or(user_text, |(_, user)| user);
    let query = command_query(words.trim())?;
    (!query.is_empty()).then_some(RECIPE)
}

/// Whether `line` asks for deep research.
pub fn is_command(line: &str) -> bool {
    turn_note(line).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_command_with_a_query_gets_the_recipe() {
        assert_eq!(turn_note("/deep-research cats"), Some(RECIPE));
        assert_eq!(turn_note("  /Deep-Research\tharbor tides "), Some(RECIPE));
        assert_eq!(turn_note("/deep-research"), None);
        assert_eq!(turn_note("/deep-research   "), None);
        assert_eq!(turn_note("/deep-research-extra cats"), None);
        assert_eq!(turn_note("deep-research cats"), None);
        assert_eq!(turn_note("please /deep-research cats"), None);
        assert_eq!(turn_note("/déep"), None);
        assert!(is_command("/deep-research cats"));
        assert!(!is_command("cats"));
    }

    #[test]
    fn a_card_chat_still_gets_the_recipe() {
        let carried = format!(
            "{}/deep-research why it crashed",
            grokhub_core::card_report::card_carry("Pulse card: bluetoothd crashed")
        );
        assert_eq!(turn_note(&carried), Some(RECIPE));
    }

    struct Done;
    impl crate::client::ModelClient for Done {
        fn stream(
            &self,
            _req: &crate::client::ResponsesRequest,
            _cancel: &crate::client::CancelToken,
            _sink: &mut dyn FnMut(crate::client::StreamEvent),
        ) -> Result<crate::client::TurnOutput, crate::client::ClientError> {
            Ok(crate::client::TurnOutput {
                text: "done".into(),
                ..Default::default()
            })
        }
    }

    struct Never;
    impl crate::run::HaltCheck for Never {
        fn halted(&self) -> bool {
            false
        }
    }

    /// The newest user turn after a run of each ask in one chat: its notes
    /// and the words shown.
    fn newest_turn(asks: &[&str], name: &str) -> (String, String) {
        let dir = std::env::temp_dir().join(format!(
            "gh-research-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let config = dir.join("config");
        std::fs::create_dir_all(&config).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&config);
        let conv = format!("research-{name}-{}", std::process::id());
        let input = crate::run::LoopIn {
            client: &Done,
            workspace: &dir,
            model: "test/model",
            effort: None,
            system: "",
            conversation_id: &conv,
            max_turns: 0,
            usage_base: crate::client::Usage::default(),
            cancel: &crate::client::CancelToken::new(),
            steer: &crate::run::SteerQueue::new(),
            halt: &Never,
            gate: crate::gate::Gate::phase_readonly(),
            desktop: None,
            permits: &crate::gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut history = Vec::new();
        for ask in asks {
            crate::run_loop(&input, &mut history, ask, None, &mut |_| {});
        }
        let text = history
            .iter()
            .rev()
            .find(|item| {
                matches!(item, crate::client::InputItem::Message { role, .. } if role == "user")
            })
            .map(crate::compact::message_text)
            .expect("a user turn");
        let _ = std::fs::remove_dir_all(&dir);
        match crate::prefix_cache::split_user_turn(&text) {
            Some((notes, words)) => (notes.to_string(), words.to_string()),
            None => (String::new(), text),
        }
    }

    #[test]
    fn a_research_turn_carries_the_recipe_and_keeps_the_line_as_typed() {
        let (notes, words) = newest_turn(&["/deep-research harbor tides"], "on");
        assert!(notes.contains(RECIPE), "{notes}");
        assert_eq!(words, "/deep-research harbor tides");
        let (plain, words) = newest_turn(&["what are harbor tides"], "off");
        assert!(!plain.contains("Deep research:"), "{plain}");
        assert_eq!(words, "what are harbor tides");
    }

    #[test]
    fn the_turn_after_research_says_the_recipe_is_old() {
        let (notes, words) = newest_turn(&["/deep-research harbor tides", "thanks"], "after");
        assert!(
            notes.starts_with(crate::prefix_cache::SUPERSEDES_NOTE),
            "{notes}"
        );
        assert!(!notes.contains(RECIPE), "{notes}");
        assert_eq!(words, "thanks");
    }

    #[test]
    fn a_research_line_never_runs_as_a_desktop_episode() {
        assert!(!crate::events::runs_as_episode(
            true,
            "/deep-research harbor tides"
        ));
        assert!(crate::events::runs_as_episode(
            true,
            "open the settings app"
        ));
        assert!(!crate::events::runs_as_episode(
            false,
            "open the settings app"
        ));
    }

    #[test]
    fn the_recipe_names_the_steps_and_the_tools() {
        for want in [
            "Plan a few distinct questions",
            "web_search",
            "x_search when listed",
            "web_fetch",
            "Do not call a tool that is not in your tool list.",
            "more than one source",
            "cited report",
        ] {
            assert!(RECIPE.contains(want), "{want}");
        }
    }
}
