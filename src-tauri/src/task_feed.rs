//! Which chats' Tasks panels are out of date. The agent marks a chat when a
//! helper, command, monitor or to-do changes; the shell marks one here for
//! what the agent cannot see (an ask answered, a turn starting or ending).
//! The helper-ask loop sends one `tasks://changed` per chat per tick, and
//! the page reads the queue again.

use std::collections::BTreeSet;
use std::sync::Mutex;

use serde::Serialize;

/// The page's nudge to read a chat's queue again. It carries no rows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksChanged<'a> {
    pub conversation_id: &'a str,
}

/// The chats that changed since the last read.
#[derive(Default)]
pub struct Feed {
    chats: Mutex<BTreeSet<String>>,
}

impl Feed {
    /// The chat's Tasks panel is out of date.
    pub fn touch(&self, chat: &str) {
        let mut chats = self.chats.lock().unwrap_or_else(|e| e.into_inner());
        if !chats.contains(chat) {
            chats.insert(chat.to_string());
        }
    }

    /// The chats touched since the last call, sorted, each once.
    pub fn take(&self) -> Vec<String> {
        let mut chats = self.chats.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *chats).into_iter().collect()
    }
}

fn feed() -> &'static Feed {
    static FEED: std::sync::OnceLock<Feed> = std::sync::OnceLock::new();
    FEED.get_or_init(Feed::default)
}

/// The chat's Tasks panel is out of date.
pub fn touch(chat: &str) {
    feed().touch(chat);
}

/// The chats to nudge this tick: the shell's own and the agent's, sorted,
/// each once.
pub fn take() -> Vec<String> {
    merge(feed().take(), grokhub_agent::drain_task_changes())
}

fn merge(shell: Vec<String>, agent: Vec<String>) -> Vec<String> {
    let all: BTreeSet<String> = shell.into_iter().chain(agent).collect();
    all.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chat_touched_twice_is_taken_once_in_order() {
        let feed = Feed::default();
        feed.touch("b");
        feed.touch("a");
        feed.touch("b");
        assert_eq!(feed.take(), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(feed.take(), Vec::<String>::new());
    }

    #[test]
    fn the_shells_and_the_agents_chats_merge_without_repeats() {
        let merged = merge(
            vec!["shell-c2".into(), "shell-c3".into()],
            vec!["shell-c1".into(), "shell-c2".into()],
        );
        assert_eq!(
            merged,
            vec![
                "shell-c1".to_string(),
                "shell-c2".to_string(),
                "shell-c3".to_string()
            ]
        );
    }

    #[test]
    fn the_event_carries_only_the_chat_id() {
        let json = serde_json::to_string(&TasksChanged {
            conversation_id: "shell-c1",
        })
        .unwrap();
        assert_eq!(json, r#"{"conversationId":"shell-c1"}"#);
    }
}
