//! Questions from background helpers. A helper that runs after its chat's
//! reply ended (a background subagent) asks through the agent's side queue,
//! not through the reply's own event stream. This drains that queue, sends
//! each ask to the page as a chat event marked `helper`, and keeps it until
//! it is answered, so a chat opened later still shows it.

use std::collections::HashMap;
use std::sync::Mutex;

use grokhub_agent::SideEvent;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::agent::UiEvent;

/// How often the side queue is read.
pub const TICK: std::time::Duration = std::time::Duration::from_millis(250);

/// A chat event from a helper. The page shows it whether or not that chat's
/// reply is still running, since the helper outlives the reply.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HelperEvent<'a> {
    conversation_id: &'a str,
    event: &'a UiEvent,
    helper: bool,
}

fn pending() -> &'static Pending {
    static PENDING: std::sync::OnceLock<Pending> = std::sync::OnceLock::new();
    PENDING.get_or_init(Pending::default)
}

/// Reads the side queue for the life of the app and sends each helper's
/// question to the page, with a desktop notice like the chat's own.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || loop {
        for (chat, event) in pending().take(grokhub_agent::drain_side_events()) {
            #[cfg(desktop)]
            crate::os_extras::on_chat_event(&app, &chat, &event);
            let _ = app.emit(
                crate::CHAT_EVENT,
                HelperEvent {
                    conversation_id: &chat,
                    event: &event,
                    helper: true,
                },
            );
        }
        std::thread::sleep(TICK);
    });
}

/// The helper questions a chat still waits on, for a chat opened later.
pub fn waiting(chat: &str) -> Vec<UiEvent> {
    pending().waiting(chat)
}

/// Every helper question still waiting, by chat: the decision inbox's
/// helper rows.
pub fn all_waiting() -> Vec<(String, UiEvent)> {
    pending().all_waiting()
}

/// The question `id` in `chat` was answered.
pub fn answered(chat: &str, id: &str) {
    pending().answered(chat, id);
}

/// The chat's worker ended, and its helpers with it: nothing waits now.
pub fn chat_ended(chat: &str) {
    pending().chat_ended(chat);
}

/// Asks waiting for an answer, by chat.
#[derive(Default)]
pub struct Pending {
    asks: Mutex<HashMap<String, Vec<UiEvent>>>,
}

impl Pending {
    /// Turns side events into page events and keeps the asks. Task and plan
    /// rows are dropped: the page has no row for them yet.
    pub fn take(&self, events: Vec<SideEvent>) -> Vec<(String, UiEvent)> {
        let mut out = Vec::new();
        let mut asks = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        for event in events {
            let chat = event.session().to_string();
            let Some(ui) = ask_event(event) else {
                continue;
            };
            asks.entry(chat.clone()).or_default().push(ui.clone());
            out.push((chat, ui));
        }
        out
    }

    /// The asks a chat still owes an answer to, oldest first.
    pub fn waiting(&self, chat: &str) -> Vec<UiEvent> {
        self.asks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat)
            .cloned()
            .unwrap_or_default()
    }

    /// Every chat's asks, chats in id order so the inbox's rows don't
    /// shuffle between reads.
    pub fn all_waiting(&self) -> Vec<(String, UiEvent)> {
        let asks = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        let mut chats: Vec<&String> = asks.keys().collect();
        chats.sort();
        chats
            .into_iter()
            .flat_map(|chat| asks[chat].iter().map(move |ui| (chat.clone(), ui.clone())))
            .collect()
    }

    /// The ask `id` was answered (or its chat closed it).
    pub fn answered(&self, chat: &str, id: &str) {
        let mut asks = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(list) = asks.get_mut(chat) {
            list.retain(|ui| ask_id(ui) != Some(id));
            if list.is_empty() {
                asks.remove(chat);
            }
        }
    }

    /// The chat's helpers stopped with its worker.
    pub fn chat_ended(&self, chat: &str) {
        self.asks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(chat);
    }
}

fn ask_id(ui: &UiEvent) -> Option<&str> {
    match ui {
        UiEvent::Permission { id, .. } | UiEvent::Elicit { id, .. } => Some(id),
        _ => None,
    }
}

fn ask_event(event: SideEvent) -> Option<UiEvent> {
    match event {
        SideEvent::Permission(ask) => Some(UiEvent::Permission {
            id: ask.tool_call_id,
            title: ask.title,
            action: ask.action,
            reason: ask.reason,
        }),
        SideEvent::Elicit(ask) => Some(UiEvent::Elicit {
            id: ask.tool_call_id,
            server: ask.server_name,
            message: ask.message,
            mode: ask.mode,
            url: ask.url,
            field_name: ask.field_name,
            field_title: ask.field_title,
            secret: ask.secret,
            options: ask.options,
            chips: Vec::new(),
        }),
        SideEvent::Task { .. } | SideEvent::Plan { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::wire::PermissionAsk;

    fn permission(chat: &str, id: &str) -> SideEvent {
        SideEvent::Permission(PermissionAsk {
            rpc_id: serde_json::Value::String(id.into()),
            session_id: chat.into(),
            title: "run_command".into(),
            tool_call_id: id.into(),
            action: "cargo test".into(),
            reason: "check the fix".into(),
            reject_option: Some("denied".into()),
        })
    }

    #[test]
    fn a_helper_ask_is_kept_until_answered_and_tasks_are_dropped() {
        let pending = Pending::default();
        let sent = pending.take(vec![
            permission("chat-a", "c1"),
            SideEvent::Task {
                session: "chat-a".into(),
                id: "a1".into(),
                title: "look".into(),
                done: false,
            },
            permission("chat-b", "c2"),
        ]);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, "chat-a");
        assert_eq!(
            sent[0].1,
            UiEvent::Permission {
                id: "c1".into(),
                title: "run_command".into(),
                action: "cargo test".into(),
                reason: "check the fix".into(),
            }
        );
        assert_eq!(pending.waiting("chat-a").len(), 1);
        pending.answered("chat-a", "c1");
        assert!(pending.waiting("chat-a").is_empty());
        assert_eq!(pending.waiting("chat-b").len(), 1);
        pending.answered("chat-b", "nope");
        assert_eq!(pending.waiting("chat-b").len(), 1);
        pending.chat_ended("chat-b");
        assert!(pending.waiting("chat-b").is_empty());
    }
}
