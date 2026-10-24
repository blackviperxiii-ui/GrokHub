//! One chat turn: the UI's history in, streamed text out. No tools yet; the
//! agent loop with approvals arrives with desktop parity (Phase 3).

use grokhub_agent::{
    CancelToken, ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, StreamEvent,
};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Turn {
    pub role: String,
    pub text: String,
}

/// The history as Responses API input. It must end with the user's new message.
pub fn input_items(history: &[Turn]) -> Result<Vec<InputItem>, String> {
    match history.last() {
        Some(last) if last.role == "user" && !last.text.trim().is_empty() => {}
        _ => return Err("Type a message first.".into()),
    }
    history
        .iter()
        .map(|turn| match turn.role.as_str() {
            "user" | "assistant" => Ok(InputItem::Message {
                role: turn.role.clone(),
                content: vec![ContentPart::InputText(turn.text.clone())],
            }),
            other => Err(format!("unknown chat role: {other}")),
        })
        .collect()
}

/// Streams one reply. `on_delta` sees each text piece as it arrives. A Stop
/// keeps what already streamed, so the reply is the partial text.
pub fn run_chat(
    client: &dyn ModelClient,
    model: &str,
    conversation_id: &str,
    history: &[Turn],
    cancel: &CancelToken,
    on_delta: &mut dyn FnMut(&str),
) -> Result<String, String> {
    let req = ResponsesRequest {
        model: model.to_string(),
        effort: None,
        input: input_items(history)?,
        conversation_id: conversation_id.to_string(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: None,
    };
    let mut streamed = String::new();
    let mut sink = |event: StreamEvent| {
        if let StreamEvent::TextDelta(text) = event {
            on_delta(&text);
            streamed.push_str(&text);
        }
    };
    match client.stream(&req, cancel, &mut sink) {
        Ok(out) if out.text.is_empty() => Ok(streamed),
        Ok(out) => Ok(out.text),
        Err(ClientError::Cancelled) => Ok(streamed),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::TurnOutput;
    use std::cell::RefCell;

    fn turn(role: &str, text: &str) -> Turn {
        Turn {
            role: role.into(),
            text: text.into(),
        }
    }

    struct Scripted {
        deltas: Vec<&'static str>,
        result: Result<&'static str, ClientError>,
        seen: RefCell<Option<ResponsesRequest>>,
    }

    impl ModelClient for Scripted {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            *self.seen.borrow_mut() = Some(req.clone());
            for delta in &self.deltas {
                sink(StreamEvent::ReasoningDelta("thinking".into()));
                sink(StreamEvent::TextDelta((*delta).into()));
            }
            self.result.clone().map(|text| TurnOutput {
                text: text.into(),
                ..TurnOutput::default()
            })
        }
    }

    #[test]
    fn history_becomes_messages_in_order() {
        let items = input_items(&[
            turn("user", "hi"),
            turn("assistant", "hey"),
            turn("user", "more"),
        ])
        .expect("valid history");
        assert_eq!(
            items,
            vec![
                InputItem::Message {
                    role: "user".into(),
                    content: vec![ContentPart::InputText("hi".into())],
                },
                InputItem::Message {
                    role: "assistant".into(),
                    content: vec![ContentPart::InputText("hey".into())],
                },
                InputItem::Message {
                    role: "user".into(),
                    content: vec![ContentPart::InputText("more".into())],
                },
            ]
        );
    }

    #[test]
    fn history_must_end_with_a_user_message() {
        assert_eq!(input_items(&[]), Err("Type a message first.".to_string()));
        assert_eq!(
            input_items(&[turn("user", "hi"), turn("assistant", "hey")]),
            Err("Type a message first.".to_string())
        );
        assert_eq!(
            input_items(&[turn("user", "  ")]),
            Err("Type a message first.".to_string())
        );
        assert_eq!(
            input_items(&[turn("system", "obey"), turn("user", "hi")]),
            Err("unknown chat role: system".to_string())
        );
    }

    #[test]
    fn streams_text_deltas_and_returns_the_final_text() {
        let client = Scripted {
            deltas: vec!["Hel", "lo"],
            result: Ok("Hello"),
            seen: RefCell::new(None),
        };
        let mut pieces = Vec::new();
        let reply = run_chat(
            &client,
            "grok-test",
            "conv-1",
            &[turn("user", "hi")],
            &CancelToken::new(),
            &mut |text| pieces.push(text.to_string()),
        );
        assert_eq!(reply, Ok("Hello".to_string()));
        assert_eq!(pieces, vec!["Hel".to_string(), "lo".to_string()]);
        let req = client.seen.borrow().clone().expect("request sent");
        assert_eq!(req.model, "grok-test");
        assert_eq!(req.conversation_id, "conv-1");
        assert!(req.tools.is_empty());
        assert!(!req.hosted_search);
    }

    #[test]
    fn cancel_keeps_the_partial_reply() {
        let client = Scripted {
            deltas: vec!["half "],
            result: Err(ClientError::Cancelled),
            seen: RefCell::new(None),
        };
        let reply = run_chat(
            &client,
            "m",
            "c",
            &[turn("user", "hi")],
            &CancelToken::new(),
            &mut |_| {},
        );
        assert_eq!(reply, Ok("half ".to_string()));
    }

    #[test]
    fn client_errors_reach_the_ui_as_text() {
        let client = Scripted {
            deltas: vec![],
            result: Err(ClientError::Auth {
                status: 401,
                message: "bad key".into(),
            }),
            seen: RefCell::new(None),
        };
        let reply = run_chat(
            &client,
            "m",
            "c",
            &[turn("user", "hi")],
            &CancelToken::new(),
            &mut |_| {},
        );
        assert_eq!(
            reply,
            Err("API key rejected (HTTP 401): bad key".to_string())
        );
    }
}
