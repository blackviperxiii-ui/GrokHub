//! Values the person typed into a secret field on a chat's form card (a
//! connector token, a password a server asked for), held for that chat while
//! the app runs. Whatever the model says or thinks after that has them
//! replaced with `[redacted]`: live on the page, and in the text and thoughts
//! the chat saves. They are never written anywhere.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::run::LoopEvent;

/// Values shorter than this aren't held: replacing them would mangle words.
const MIN_CHARS: usize = 4;

fn chats() -> &'static Mutex<HashMap<String, Vec<String>>> {
    static MAP: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Hold `value` for chat `session`. Short and repeated values are skipped.
pub fn hold_secret(session: &str, value: &str) {
    let value = value.trim();
    if session.is_empty() || value.chars().count() < MIN_CHARS {
        return;
    }
    let mut map = chats().lock().unwrap_or_else(|err| err.into_inner());
    let held = map.entry(session.to_string()).or_default();
    if !held.iter().any(|v| v == value) {
        held.push(value.to_string());
    }
}

/// The values held for chat `session`, oldest first.
pub fn held_secrets(session: &str) -> Vec<String> {
    let map = chats().lock().unwrap_or_else(|err| err.into_inner());
    map.get(session).cloned().unwrap_or_default()
}

/// `text` with every value held for `session` replaced.
pub(crate) fn redact(session: &str, text: &str) -> String {
    let held = held_secrets(session);
    if held.is_empty() {
        return text.to_string();
    }
    grokhub_core::redact_held_secrets(text, &held)
}

/// Redacts a stream of text deltas. A held value can arrive split across
/// two deltas, so while any value is held the end of the text that could
/// still begin one waits for the next delta, or for [`Stream::flush`].
#[derive(Debug, Default)]
pub(crate) struct Stream {
    pending: String,
}

impl Stream {
    /// What can go out now of `delta`, redacted.
    pub(crate) fn push(&mut self, held: &[String], delta: &str) -> String {
        self.pending.push_str(delta);
        if held.is_empty() {
            return std::mem::take(&mut self.pending);
        }
        let clean = grokhub_core::redact_held_secrets(&self.pending, held);
        let keep = waiting_tail(&clean, held);
        let cut = clean.len() - keep;
        self.pending = clean[cut..].to_string();
        clean[..cut].to_string()
    }

    /// The rest of the stream, redacted.
    pub(crate) fn flush(&mut self, held: &[String]) -> String {
        let rest = std::mem::take(&mut self.pending);
        grokhub_core::redact_held_secrets(&rest, held)
    }
}

/// Bytes at the end of `text` that are the start of a held value, the
/// longest such run.
fn waiting_tail(text: &str, held: &[String]) -> usize {
    held.iter()
        .map(|value| value.trim())
        .filter(|value| value.chars().count() >= MIN_CHARS)
        .map(|value| {
            value
                .char_indices()
                .skip(1)
                .map(|(i, _)| &value[..i])
                .filter(|start| text.ends_with(start))
                .map(str::len)
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0)
}

/// One prompt's events on their way to the page: text, thoughts and tool
/// cards lose the values held for the chat, and the thoughts are kept, with
/// how many tool calls and runs of reply text came before each, for the
/// turn's save.
pub(crate) struct Shown {
    session: String,
    text: Stream,
    thought: Stream,
    calls: std::collections::HashSet<String>,
    /// Runs of reply text so far, and whether one is open: a thought, a tool
    /// card or the end of a model step closes it.
    texts: usize,
    in_text: bool,
    pub(crate) thoughts: Vec<crate::session::TurnThought>,
}

impl Shown {
    pub(crate) fn new(session: &str) -> Self {
        Self {
            session: session.to_string(),
            text: Stream::default(),
            thought: Stream::default(),
            calls: std::collections::HashSet::new(),
            texts: 0,
            in_text: false,
            thoughts: Vec::new(),
        }
    }

    /// A model step ended: the next reply text is a new run.
    pub(crate) fn end_step(&mut self) {
        self.in_text = false;
    }

    /// What goes to the page for `event`: a text or thought delta waits on
    /// the end of a held value, and anything else first sends what waited.
    pub(crate) fn pass(&mut self, event: LoopEvent) -> Vec<LoopEvent> {
        let held = held_secrets(&self.session);
        let mut out = Vec::new();
        match event {
            LoopEvent::Text(delta) => {
                if !self.in_text {
                    self.texts += 1;
                    self.in_text = true;
                }
                self.flush_thought(&held, &mut out);
                let text = self.text.push(&held, &delta);
                if !text.is_empty() {
                    out.push(LoopEvent::Text(text));
                }
            }
            LoopEvent::Thought(delta) => {
                self.in_text = false;
                self.flush_text(&held, &mut out);
                self.keep_thought(&delta);
                let text = self.thought.push(&held, &delta);
                if !text.is_empty() {
                    out.push(LoopEvent::Thought(text));
                }
            }
            LoopEvent::Tool {
                id,
                name,
                status,
                detail,
                image,
            } => {
                self.in_text = false;
                self.flush_all(&held, &mut out);
                self.calls.insert(id.clone());
                out.push(LoopEvent::Tool {
                    id,
                    name: grokhub_core::redact_held_secrets(&name, &held),
                    status,
                    detail: grokhub_core::redact_held_secrets(&detail, &held),
                    image,
                });
            }
            other => {
                self.flush_all(&held, &mut out);
                out.push(other);
            }
        }
        out
    }

    /// What still waits at the end of the prompt.
    pub(crate) fn finish(&mut self) -> Vec<LoopEvent> {
        let held = held_secrets(&self.session);
        let mut out = Vec::new();
        self.flush_all(&held, &mut out);
        out
    }

    fn keep_thought(&mut self, delta: &str) {
        let (calls, texts) = (self.calls.len(), self.texts);
        match self.thoughts.last_mut() {
            Some(last) if (last.calls, last.texts) == (calls, texts) => last.text.push_str(delta),
            _ => self.thoughts.push(crate::session::TurnThought {
                calls,
                texts,
                text: delta.to_string(),
            }),
        }
    }

    fn flush_all(&mut self, held: &[String], out: &mut Vec<LoopEvent>) {
        self.flush_thought(held, out);
        self.flush_text(held, out);
    }

    fn flush_text(&mut self, held: &[String], out: &mut Vec<LoopEvent>) {
        let rest = self.text.flush(held);
        if !rest.is_empty() {
            out.push(LoopEvent::Text(rest));
        }
    }

    fn flush_thought(&mut self, held: &[String], out: &mut Vec<LoopEvent>) {
        let rest = self.thought.flush(held);
        if !rest.is_empty() {
            out.push(LoopEvent::Thought(rest));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "sk-abcdefghijklmnopqrstuv";

    #[test]
    fn a_held_value_is_kept_once_and_short_ones_are_skipped() {
        let chat = "held-keeps-once";
        hold_secret(chat, "abc");
        hold_secret(chat, "  ");
        assert_eq!(held_secrets(chat), Vec::<String>::new());
        hold_secret(chat, &format!(" {TOKEN} "));
        hold_secret(chat, TOKEN);
        assert_eq!(held_secrets(chat), vec![TOKEN.to_string()]);
        assert_eq!(held_secrets("held-other-chat"), Vec::<String>::new());
        assert_eq!(
            redact(chat, &format!("Saved {TOKEN} for you.")),
            "Saved [redacted] for you."
        );
        assert_eq!(redact("held-other-chat", TOKEN), TOKEN);
    }

    #[test]
    fn a_value_split_across_deltas_never_goes_out() {
        let held = vec![TOKEN.to_string()];
        let mut stream = Stream::default();
        let parts = [
            "Your key is sk-abc",
            "defghijklm",
            "nopqrstuv and it",
            " works.",
        ];
        let sent: Vec<String> = parts.iter().map(|d| stream.push(&held, d)).collect();
        assert_eq!(
            sent,
            vec![
                "Your key is ".to_string(),
                String::new(),
                "[redacted] and it".to_string(),
                " works.".to_string(),
            ]
        );
        assert_eq!(stream.flush(&held), "");
    }

    fn tool(id: &str, detail: &str) -> LoopEvent {
        LoopEvent::Tool {
            id: id.into(),
            name: "connection_add".into(),
            status: "completed".into(),
            detail: detail.into(),
            image: None,
        }
    }

    #[test]
    fn a_prompts_events_lose_held_values_and_keep_its_thoughts_by_tool_call() {
        let chat = "held-shown-prompt";
        hold_secret(chat, TOKEN);
        let mut shown = Shown::new(chat);
        let mut page = Vec::new();
        for event in [
            LoopEvent::Thought("I'll save sk-abcdefghij".into()),
            LoopEvent::Thought("klmnopqrstuv now.".into()),
            tool("call-1", &format!("token {TOKEN}")),
            tool("call-1", "saved"),
            LoopEvent::Thought("Saved. ".into()),
            LoopEvent::Text("Done with sk-abc".into()),
        ] {
            page.extend(shown.pass(event));
        }
        page.extend(shown.finish());
        let page: Vec<String> = page
            .iter()
            .map(|event| match event {
                LoopEvent::Text(t) => format!("text:{t}"),
                LoopEvent::Thought(t) => format!("thought:{t}"),
                LoopEvent::Tool { detail, .. } => format!("tool:{detail}"),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            page,
            vec![
                "thought:I'll save ",
                "thought:[redacted] now.",
                "tool:token [redacted]",
                "tool:saved",
                "thought:Saved. ",
                "text:Done with ",
                "text:sk-abc",
            ]
        );
        let kept: Vec<(usize, usize, &str)> = shown
            .thoughts
            .iter()
            .map(|t| (t.calls, t.texts, t.text.as_str()))
            .collect();
        assert_eq!(
            kept,
            vec![
                (0, 0, "I'll save sk-abcdefghijklmnopqrstuv now."),
                (1, 0, "Saved. "),
            ],
            "kept as streamed; the save redacts them"
        );
    }

    #[test]
    fn each_model_steps_thought_is_kept_after_the_text_before_it() {
        let mut shown = Shown::new("held-steps");
        for event in [
            LoopEvent::Thought("Plan.".into()),
            LoopEvent::Text("First try.".into()),
        ] {
            shown.pass(event);
        }
        shown.end_step();
        shown.pass(LoopEvent::Text(" Still first.".into()));
        shown.end_step();
        for event in [
            LoopEvent::Thought("Again.".into()),
            LoopEvent::Text("Second try.".into()),
        ] {
            shown.pass(event);
        }
        let kept: Vec<(usize, usize, &str)> = shown
            .thoughts
            .iter()
            .map(|t| (t.calls, t.texts, t.text.as_str()))
            .collect();
        assert_eq!(kept, vec![(0, 0, "Plan."), (0, 2, "Again.")]);
    }

    #[test]
    fn a_tail_that_only_looks_like_a_value_goes_out_on_flush() {
        let held = vec![TOKEN.to_string()];
        let mut stream = Stream::default();
        assert_eq!(stream.push(&held, "ends with sk-"), "ends with ");
        assert_eq!(stream.flush(&held), "sk-");
        let mut plain = Stream::default();
        assert_eq!(plain.push(&[], "sk-"), "sk-");
        assert_eq!(plain.flush(&[]), "");
    }
}
