//! Journal (shell parity): the cabin's decision journal from the same
//! `decisions.json`, changed through the same `grokhub_core::journal` rules,
//! so a question added in either shows in both. To mission links a chat to
//! the question and drafts its first message; nothing is sent for you.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use grokhub_core::{JournalEntry, JournalStatus};
use serde::{Deserialize, Serialize};

const FILE: &str = "decisions.json";
/// The cabin's own cap for its JSON stores.
const STORE_CAP: u64 = 32 * 1024 * 1024;

/// One writer at a time in this process; the temp file is this process's own.
static STORE: Mutex<()> = Mutex::new(());

fn store() -> MutexGuard<'static, ()> {
    STORE.lock().unwrap_or_else(|err| err.into_inner())
}

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub id: String,
    pub question: String,
    /// "Open", "Investigating", … as the cabin labels it.
    pub status: String,
    pub active: bool,
    pub options: Vec<Choice>,
    /// The id of the option it leans to, if that option is still there.
    pub leaning: Option<String>,
    /// The option picked when it was decided.
    pub chosen: Option<String>,
    /// The chat its mission runs in.
    pub mission: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// Active questions, newest first.
    pub active: Vec<Question>,
    /// Decided and abandoned ones, newest first.
    pub history: Vec<Question>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Act {
    AddOption {
        label: String,
    },
    Lean {
        option_id: String,
    },
    Defer,
    Abandon,
    /// Back to Open from Deferred.
    Reopen,
    Decide {
        option_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mission {
    /// The chat to open; the same one on a second press.
    pub conversation_id: String,
    pub title: String,
    /// The first message, put in the chat box for you to send.
    pub prompt: String,
}

fn load_unlocked(config_dir: &Path) -> Result<Vec<JournalEntry>, String> {
    let path = path(config_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len > STORE_CAP {
        return Err(format!("{FILE} is too large to be a journal."));
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw).map_err(|e| format!("{FILE}: {e}"))
}

/// Writes through a temp file and a rename, like the cabin. Callers hold
/// [`STORE`].
fn save(config_dir: &Path, entries: &[JournalEntry]) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.shell-{}.tmp", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

fn label_of(entry: &JournalEntry, option_id: &str) -> Option<String> {
    entry
        .options
        .iter()
        .find(|o| o.id == option_id)
        .map(|o| o.label.clone())
}

fn question(entry: &JournalEntry) -> Question {
    Question {
        id: entry.id.clone(),
        question: entry.question.clone(),
        status: entry.status.label().to_string(),
        active: entry.status.is_active(),
        options: entry
            .options
            .iter()
            .map(|o| Choice {
                id: o.id.clone(),
                label: o.label.clone(),
            })
            .collect(),
        leaning: entry
            .leaning
            .as_ref()
            .filter(|l| label_of(entry, &l.option_id).is_some())
            .map(|l| l.option_id.clone()),
        chosen: entry
            .outcome
            .as_ref()
            .and_then(|o| label_of(entry, &o.chosen_option_id)),
        mission: entry.mission_thread.clone(),
    }
}

fn page(entries: &[JournalEntry]) -> Page {
    let (active, history) = entries
        .iter()
        .partition::<Vec<_>, _>(|e| e.status.is_active());
    Page {
        active: active.into_iter().map(question).collect(),
        history: history.into_iter().map(question).collect(),
    }
}

pub fn load(config_dir: &Path) -> Result<Page, String> {
    let _held = store();
    Ok(page(&load_unlocked(config_dir)?))
}

/// Adds an open question at the top.
pub fn add(config_dir: &Path, text: &str) -> Result<Page, String> {
    let _held = store();
    let mut entries = load_unlocked(config_dir)?;
    grokhub_core::create_entry(&mut entries, text).ok_or("Type the question first.")?;
    save(config_dir, &entries)?;
    Ok(page(&entries))
}

pub fn act(config_dir: &Path, id: &str, act: Act) -> Result<Page, String> {
    let _held = store();
    let mut entries = load_unlocked(config_dir)?;
    let entry = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or("That question is gone. It may have been removed in the cabin.")?;
    let active = entry.status.is_active();
    let changed = match act {
        Act::AddOption { label } => {
            if entry.options.len() >= 5 {
                return Err("A question can have five options at most.".into());
            }
            grokhub_core::add_option(&mut entries, id, &label).is_some()
        }
        Act::Lean { option_id } => grokhub_core::set_leaning(&mut entries, id, &option_id, ""),
        Act::Defer if active => grokhub_core::defer_entry(&mut entries, id),
        Act::Abandon if active => grokhub_core::abandon_entry(&mut entries, id),
        Act::Reopen if entry.status == JournalStatus::Deferred => {
            grokhub_core::set_status(&mut entries, id, JournalStatus::Open)
        }
        Act::Decide { option_id } if active => {
            grokhub_core::decide_entry(&mut entries, id, option_id.as_deref(), "")
        }
        _ => false,
    };
    if !changed {
        return Err("Nothing changed.".into());
    }
    save(config_dir, &entries)?;
    Ok(page(&entries))
}

/// Links an active question to a chat and drafts its first message. A second
/// press keeps the chat it already has; `new_conversation` is used only for
/// the first.
pub fn to_mission(config_dir: &Path, id: &str, new_conversation: &str) -> Result<Mission, String> {
    let _held = store();
    let mut entries = load_unlocked(config_dir)?;
    let entry = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or("That question is gone. It may have been removed in the cabin.")?;
    let conversation = entry
        .mission_thread
        .clone()
        .unwrap_or_else(|| new_conversation.trim().to_string());
    let start = grokhub_core::promote_to_mission(&mut entries, id, &conversation)
        .ok_or("Only an open question can become a mission.")?;
    save(config_dir, &entries)?;
    Ok(Mission {
        conversation_id: conversation,
        title: start.title,
        prompt: start.prompt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let config = std::env::temp_dir().join(format!(
            "grokhub-shell-journal-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&config);
        std::fs::create_dir_all(&config).expect("cfg");
        config
    }

    fn on_disk(config: &Path) -> Vec<JournalEntry> {
        serde_json::from_str(&std::fs::read_to_string(config.join(FILE)).expect("file"))
            .expect("json")
    }

    #[test]
    fn no_file_is_an_empty_journal_and_a_blank_question_is_refused() {
        let config = dir("empty");
        assert_eq!(
            load(&config).unwrap(),
            Page {
                active: vec![],
                history: vec![]
            }
        );
        assert_eq!(add(&config, "   ").unwrap_err(), "Type the question first.");
        assert!(!config.join(FILE).exists());
    }

    #[test]
    fn a_question_gets_options_a_leaning_and_a_decision_in_the_cabins_file() {
        let config = dir("decide");
        let page = add(&config, "  Ship the shell before the cabin?  ").unwrap();
        assert_eq!(page.active.len(), 1);
        let q = &page.active[0];
        assert_eq!(q.question, "Ship the shell before the cabin?");
        assert_eq!(q.status, "Open");
        let id = q.id.clone();

        act(
            &config,
            &id,
            Act::AddOption {
                label: "Yes".into(),
            },
        )
        .unwrap();
        let page = act(
            &config,
            &id,
            Act::AddOption {
                label: "Wait".into(),
            },
        )
        .unwrap();
        let labels: Vec<_> = page.active[0]
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect();
        // A new question starts with the cabin's two default options.
        assert_eq!(
            labels,
            ["Do nothing", "Try a small experiment", "Yes", "Wait"]
        );
        let wait = page.active[0].options[3].id.clone();

        let page = act(
            &config,
            &id,
            Act::Lean {
                option_id: wait.clone(),
            },
        )
        .unwrap();
        assert_eq!(page.active[0].leaning.as_deref(), Some(wait.as_str()));

        let page = act(
            &config,
            &id,
            Act::Decide {
                option_id: Some(wait.clone()),
            },
        )
        .unwrap();
        assert!(page.active.is_empty());
        assert_eq!(page.history.len(), 1);
        assert_eq!(page.history[0].status, "Decided");
        assert_eq!(page.history[0].chosen.as_deref(), Some("Wait"));

        let saved = on_disk(&config);
        assert_eq!(saved[0].status, JournalStatus::Decided);
        assert_eq!(saved[0].outcome.as_ref().unwrap().chosen_option_id, wait);
        assert_eq!(
            act(&config, &id, Act::Defer).unwrap_err(),
            "Nothing changed."
        );
    }

    #[test]
    fn defer_reopen_and_abandon_move_it_between_lists() {
        let config = dir("defer");
        let id = add(&config, "Keep Imagine?").unwrap().active[0].id.clone();
        let page = act(&config, &id, Act::Defer).unwrap();
        assert_eq!(page.active[0].status, "Deferred");
        let page = act(&config, &id, Act::Reopen).unwrap();
        assert_eq!(page.active[0].status, "Open");
        assert_eq!(
            act(&config, &id, Act::Reopen).unwrap_err(),
            "Nothing changed."
        );
        let page = act(&config, &id, Act::Abandon).unwrap();
        assert_eq!(page.history[0].status, "Abandoned");
        assert_eq!(
            act(&config, "nope", Act::Defer).unwrap_err(),
            "That question is gone. It may have been removed in the cabin."
        );
    }

    #[test]
    fn to_mission_drafts_the_message_and_reuses_its_chat() {
        let config = dir("mission");
        let id = add(&config, "Which phone ships first?").unwrap().active[0]
            .id
            .clone();
        act(
            &config,
            &id,
            Act::AddOption {
                label: "Android".into(),
            },
        )
        .unwrap();

        let first = to_mission(&config, &id, "chat-1").unwrap();
        assert_eq!(first.conversation_id, "chat-1");
        assert_eq!(first.title, "Which phone ships first?");
        assert_eq!(
            first.prompt,
            "Mission from my decision journal: Which phone ships first?\
             \nOptions: Do nothing · Try a small experiment · Android\
             \nNext experiment: pick the cheapest one that would change the call.\
             \n\nRun that experiment, show the evidence, and say which option it supports. Leave the decision to me."
        );
        let page = load(&config).unwrap();
        assert_eq!(page.active[0].status, "Investigating");
        assert_eq!(page.active[0].mission.as_deref(), Some("chat-1"));

        let again = to_mission(&config, &id, "chat-2").unwrap();
        assert_eq!(again.conversation_id, "chat-1");

        act(&config, &id, Act::Abandon).unwrap();
        assert_eq!(
            to_mission(&config, &id, "chat-3").unwrap_err(),
            "Only an open question can become a mission."
        );
    }

    #[test]
    fn a_sixth_option_is_refused() {
        let config = dir("five");
        let id = add(&config, "Pick a color").unwrap().active[0].id.clone();
        // Two defaults plus three.
        for label in ["a", "b", "c"] {
            act(
                &config,
                &id,
                Act::AddOption {
                    label: label.into(),
                },
            )
            .unwrap();
        }
        assert_eq!(
            act(&config, &id, Act::AddOption { label: "f".into() }).unwrap_err(),
            "A question can have five options at most."
        );
        assert_eq!(on_disk(&config)[0].options.len(), 5);
    }
}
