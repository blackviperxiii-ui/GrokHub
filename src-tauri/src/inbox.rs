//! The decision inbox: every ask that waits on the person, in one list —
//! the shell's side of the cabin's Pactwatch inbox (Spike-1b). Permission
//! and elicit asks are noted as they cross the chat event path, helper asks
//! come from [`crate::helper_asks`], and harness parks are read from disk
//! when the list is built. Answering goes through the same paths as
//! answering the card itself (see `answer_inbox` in the shell's commands):
//! a permission goes to the session, an elicit's Approve opens its chat
//! (its Deny cancels), and a park is answered in the harness park store.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;

use crate::agent::UiEvent;

/// One decision waiting, in the order the cabin's inbox lists them:
/// hard parks first, then permission asks, then elicit asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    /// The ask's id: a tool call id, an elicit id, or a park id.
    pub id: String,
    /// "permission", "elicit" or "park".
    pub kind: String,
    /// The chat the ask belongs to. Empty for a harness park, which belongs
    /// to a desktop session rather than a chat.
    pub chat_id: String,
    /// The decision in plain words, cut to one row.
    pub what: String,
    /// A hard stop. Hard rows never offer Always, and Enter never approves
    /// them: a pointer click on the row's own buttons only.
    pub hard: bool,
}

/// Row words are cut here, as the cabin cuts its inbox rows.
const WHAT_MAX: usize = 120;

fn clip(text: &str) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= WHAT_MAX {
        return one;
    }
    let cut: String = one.chars().take(WHAT_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

/// The plain-words line for one ask, mirroring the cabin inbox's words.
fn ask_words(ui: &UiEvent) -> Option<(String, bool)> {
    match ui {
        UiEvent::Permission { title, action, .. } => Some((
            if action.trim().is_empty() {
                title.clone()
            } else {
                format!("{title}: {action}")
            },
            false,
        )),
        UiEvent::Elicit {
            server, message, ..
        } => Some((
            if server == "Plan" {
                "A plan is ready for your OK".to_string()
            } else if server == "Question" {
                message
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or(message)
                    .to_string()
            } else {
                format!("{server} wants input")
            },
            false,
        )),
        _ => None,
    }
}

/// Asks noted from the chat event path, by chat. An ask leaves when it is
/// answered (here, on its card, or on its notification) or its chat ends.
#[derive(Default)]
pub struct Noted {
    asks: Mutex<HashMap<String, Vec<UiEvent>>>,
}

impl Noted {
    /// Keeps a chat's permission and elicit asks; an `Answered` event (a
    /// notification answered the ask) retires it. Other events pass by.
    pub fn note(&self, chat: &str, event: &UiEvent) {
        if let UiEvent::Answered { id, .. } = event {
            self.answered(chat, id);
            return;
        }
        if ask_words(event).is_none() {
            return;
        }
        let id = match event {
            UiEvent::Permission { id, .. } | UiEvent::Elicit { id, .. } => id.clone(),
            _ => return,
        };
        let mut asks = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        let list = asks.entry(chat.to_string()).or_default();
        match list.iter_mut().find(|ui| matches!(ui, UiEvent::Permission { id: seen, .. } | UiEvent::Elicit { id: seen, .. } if *seen == id)) {
            Some(kept) => *kept = event.clone(),
            None => list.push(event.clone()),
        }
    }

    /// The ask `id` in `chat` was answered, wherever the answer happened.
    pub fn answered(&self, chat: &str, id: &str) {
        let mut asks = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(list) = asks.get_mut(chat) {
            list.retain(
                |ui| !matches!(ui, UiEvent::Permission { id: seen, .. } | UiEvent::Elicit { id: seen, .. } if seen == id),
            );
            if list.is_empty() {
                asks.remove(chat);
            }
        }
    }

    /// The chat ended; its asks end with it.
    pub fn chat_ended(&self, chat: &str) {
        self.asks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(chat);
    }

    /// Every noted ask, chats in id order so rows don't shuffle.
    pub fn all(&self) -> Vec<(String, UiEvent)> {
        let asks = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        let mut chats: Vec<&String> = asks.keys().collect();
        chats.sort();
        chats
            .into_iter()
            .flat_map(|chat| asks[chat].iter().map(move |ui| (chat.clone(), ui.clone())))
            .collect()
    }
}

fn noted() -> &'static Noted {
    static NOTED: std::sync::OnceLock<Noted> = std::sync::OnceLock::new();
    NOTED.get_or_init(Noted::default)
}

/// Notes one event crossing a chat's event path (the shell's send route).
pub fn note(chat: &str, event: &UiEvent) {
    noted().note(chat, event);
}

/// The ask was answered: it leaves the inbox and the helper store alike.
pub fn answered(chat: &str, id: &str) {
    noted().answered(chat, id);
    crate::helper_asks::answered(chat, id);
}

/// The chat ended; its noted asks end with it.
pub fn chat_ended(chat: &str) {
    noted().chat_ended(chat);
}

/// Every decision waiting now, in the cabin inbox's order.
pub fn decisions(config_dir: &Path) -> Vec<Decision> {
    build(config_dir, noted().all(), crate::helper_asks::all_waiting())
}

fn build(
    config_dir: &Path,
    live: Vec<(String, UiEvent)>,
    helper: Vec<(String, UiEvent)>,
) -> Vec<Decision> {
    let mut rows: Vec<Decision> = grokhub_agent::harness::pending_parks(config_dir)
        .iter()
        .map(|park| Decision {
            id: park.id.clone(),
            kind: "park".to_string(),
            chat_id: String::new(),
            what: clip(if park.action.trim().is_empty() {
                &park.tool
            } else {
                &park.action
            }),
            hard: true,
        })
        .collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut permissions: Vec<Decision> = Vec::new();
    let mut elicits: Vec<Decision> = Vec::new();
    for (chat, ui) in live.into_iter().chain(helper) {
        let (UiEvent::Permission { id, .. } | UiEvent::Elicit { id, .. }) = &ui else {
            continue;
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some((words, hard)) = ask_words(&ui) else {
            continue;
        };
        let row = Decision {
            id: id.clone(),
            kind: if matches!(ui, UiEvent::Permission { .. }) {
                "permission"
            } else {
                "elicit"
            }
            .to_string(),
            chat_id: chat,
            what: clip(&words),
            hard,
        };
        if row.kind == "permission" {
            permissions.push(row);
        } else {
            elicits.push(row);
        }
    }
    rows.extend(permissions);
    rows.extend(elicits);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::harness::{answer_park, post_park, ParkRequest};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-inbox-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn permission(id: &str) -> UiEvent {
        UiEvent::Permission {
            id: id.into(),
            title: "run_command".into(),
            action: "cargo test".into(),
            reason: "check the fix".into(),
        }
    }

    #[test]
    fn a_fixture_decision_lists_and_answering_it_clears_it() {
        let config = temp_dir("fixture");
        let noted = Noted::default();
        noted.note("chat-1", &permission("c1"));
        noted.note("chat-1", &UiEvent::Text { text: "hi".into() });
        let rows = build(&config, noted.all(), Vec::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "permission");
        assert_eq!(rows[0].chat_id, "chat-1");
        assert_eq!(rows[0].what, "run_command: cargo test");
        assert!(!rows[0].hard);
        noted.answered("chat-1", "c1");
        assert!(build(&config, noted.all(), Vec::new()).is_empty());
    }

    #[test]
    fn an_answered_event_retires_the_ask_and_renoting_replaces_it() {
        let noted = Noted::default();
        noted.note("chat-1", &permission("c1"));
        noted.note("chat-1", &permission("c1"));
        assert_eq!(noted.all().len(), 1);
        noted.note(
            "chat-1",
            &UiEvent::Answered {
                id: "c1".into(),
                answer: "deny".into(),
            },
        );
        assert!(noted.all().is_empty());
    }

    #[test]
    fn parks_list_first_as_hard_rows_and_answering_clears_them() {
        let config = temp_dir("parks");
        post_park(
            &config,
            &ParkRequest {
                id: "park-1".into(),
                path: "/tmp".into(),
                tool: "computer".into(),
                action: "Delete the file report.txt".into(),
                class: "hard".into(),
                ts_ms: 1,
            },
        )
        .expect("park posts");
        let noted = Noted::default();
        noted.note("chat-1", &permission("c1"));
        let rows = build(&config, noted.all(), Vec::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, "park");
        assert!(rows[0].hard);
        assert_eq!(rows[0].what, "Delete the file report.txt");
        assert_eq!(rows[1].kind, "permission");
        answer_park(&config, "park-1", false).expect("park answers");
        let rows = build(&config, noted.all(), Vec::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "permission");
    }
}
