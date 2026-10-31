//! "Ask about this match" for the in-game watcher (design:
//! `docs/design/game-watcher/`). The question goes out with the match
//! summary from `grokhub_core::game_watch_store::match_summary`, never the raw
//! feed, to the model on this computer first.

use grokhub_core::game_watch_store::ask_messages;

use super::local;
use crate::client::CancelToken;

/// Where an ask went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAskRoute {
    /// The model on this computer. Nothing left the machine.
    Local,
    /// The model the user picked for chats. The summary went to its provider.
    Chosen,
}

/// Where an ask would go right now: [`MatchAskRoute::Local`] while Local AI is
/// available ([`local::available`]: the Settings toggle is on, a runtime is
/// installed and the governor isn't holding it, as during a game), else
/// [`MatchAskRoute::Chosen`]. The chip reads this to say which one answers.
pub fn match_ask_route() -> MatchAskRoute {
    if local::available() {
        MatchAskRoute::Local
    } else {
        MatchAskRoute::Chosen
    }
}

/// A reply to "Ask about this match" and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchAnswer {
    pub text: String,
    pub route: MatchAskRoute,
}

/// Ask one question about a finished match. Off the UI thread.
///
/// Picks the local model first: while Local AI is available the question and
/// `summary` go to the on-device model through [`local::serve`] and stay on
/// this computer. Otherwise `chosen` sends them to the model the user picked
/// for chats (the cabin passes its usual routed Grok or provider call), and
/// that cloud model sees the summary. A local failure is returned as an
/// error, never retried in the cloud, so the summary only leaves the machine
/// when the chip said it would. An empty question asks the default one.
pub fn ask_about_match(
    summary: &str,
    question: &str,
    chosen: impl FnOnce(&[(String, String)]) -> Result<String, String>,
    cancel: &CancelToken,
) -> Result<MatchAnswer, String> {
    let messages = ask_messages(summary, question);
    match match_ask_route() {
        MatchAskRoute::Local => {
            // The local model takes one user turn: the rules ride in front.
            let text = messages
                .into_iter()
                .map(|(_, t)| t)
                .collect::<Vec<_>>()
                .join("\n\n");
            local::serve("chat:quick", &text, cancel)
                .map(|out| MatchAnswer {
                    text: out.text,
                    route: MatchAskRoute::Local,
                })
                .map_err(|e| e.to_string())
        }
        MatchAskRoute::Chosen => chosen(&messages).map(|text| MatchAnswer {
            text,
            route: MatchAskRoute::Chosen,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};

    use super::*;
    use crate::route::local::{ChatMessage, ForceOn, LocalRuntime};

    /// Keeps what the local model was sent and answers with a fixed line.
    struct FakeRt(Arc<Mutex<Vec<String>>>);

    impl LocalRuntime for FakeRt {
        fn classify(&self, _text: &str, labels: &[&str]) -> Result<Value, String> {
            Ok(json!({ "label": labels[0], "score": 0.9 }))
        }
        fn summarize(&self, _text: &str, _max_words: u32) -> Result<Value, String> {
            Ok(json!({ "summary": "two lines" }))
        }
        fn difficulty(&self, _text: &str) -> Result<Value, String> {
            Ok(json!({ "d": 0.4 }))
        }
        fn chat_stream(
            &self,
            messages: &[ChatMessage],
            _on_delta: &mut dyn FnMut(&str),
        ) -> Result<String, String> {
            self.0.lock().unwrap().extend(
                messages
                    .iter()
                    .map(|m| format!("{}: {}", m.role, m.content)),
            );
            Ok("Trade less early.".into())
        }
    }

    const SUMMARY: &str = "Score: 13-9, a win.";

    #[test]
    fn local_ai_answers_first_and_the_chosen_model_is_never_called() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let _on = ForceOn::with(Some(Arc::new(FakeRt(seen.clone()))));
        assert_eq!(match_ask_route(), MatchAskRoute::Local);
        let got = ask_about_match(
            SUMMARY,
            "What went wrong?",
            |_| panic!("the chosen model must not be called while Local AI is available"),
            &CancelToken::new(),
        );
        assert_eq!(
            got,
            Ok(MatchAnswer {
                text: "Trade less early.".into(),
                route: MatchAskRoute::Local,
            })
        );
        let rules = &ask_messages(SUMMARY, "")[0].1;
        assert_eq!(
            *seen.lock().unwrap(),
            vec![format!(
                "user: {rules}\n\nMatch summary:\nScore: 13-9, a win.\n\nQuestion: What went wrong?"
            )]
        );
    }

    #[test]
    fn without_local_ai_the_chosen_model_gets_the_summary() {
        assert_eq!(match_ask_route(), MatchAskRoute::Chosen);
        let mut sent = Vec::new();
        let got = ask_about_match(
            SUMMARY,
            "",
            |m| {
                sent = m.to_vec();
                Ok("Good game.".into())
            },
            &CancelToken::new(),
        );
        assert_eq!(
            got,
            Ok(MatchAnswer {
                text: "Good game.".into(),
                route: MatchAskRoute::Chosen,
            })
        );
        assert_eq!(sent, ask_messages(SUMMARY, ""));
        assert_eq!(
            sent[1].1,
            "Match summary:\nScore: 13-9, a win.\n\nQuestion: How did this match go, and what should I work on next?"
        );
        // The chosen model's error comes back as is.
        let failed = ask_about_match(
            SUMMARY,
            "?",
            |_| Err("Connect Grok in Settings".into()),
            &CancelToken::new(),
        );
        assert_eq!(failed, Err("Connect Grok in Settings".to_string()));
    }
}
