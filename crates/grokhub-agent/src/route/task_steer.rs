//! Words that steer a chat's tasks to a model or an effort: "use Opus for the
//! refactor", "use high effort for debugging", "use grok 4.3 for everything".
//! There is no picker: the person says it in chat, the ask is saved for that
//! chat (so it sticks after a restart) and every helper of the chat reads it
//! too. "Back to auto" clears the chat's asks.
//!
//! The router still decides. A named model only narrows Auto's candidates to
//! the models whose id carries that name and that can take the step; when
//! none can, Auto picks as before and the route record says so. A named
//! effort is held inside the class's floor and ceiling.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::ask::AskKind;

/// The file under the config folder.
pub const FILE: &str = "chat-task-steer.json";
/// The rule a narrowed model pick adds to the route record.
pub const MODEL_RULE: &str = "words:model";
/// The rule when no model by the asked name could take the step.
pub const MODEL_MISSING_RULE: &str = "words:model:missing";
/// The rule a held effort adds.
pub const EFFORT_RULE: &str = "words:effort";

const BACK_TO_AUTO: &[&str] = &["back to auto", "go back to auto", "let auto pick"];

/// Which tasks an ask covers, as the words name them. Each maps onto the
/// kind [`super::ask::read`] gives a task.
const KIND_WORDS: &[(&str, AskKind)] = &[
    ("refactors", AskKind::CodeMulti),
    ("refactor", AskKind::CodeMulti),
    ("refactoring", AskKind::CodeMulti),
    ("multi-file changes", AskKind::CodeMulti),
    ("big changes", AskKind::CodeMulti),
    ("migrations", AskKind::CodeMulti),
    ("small fixes", AskKind::CodeSmall),
    ("quick fixes", AskKind::CodeSmall),
    ("small edits", AskKind::CodeSmall),
    ("fixes", AskKind::CodeSmall),
    ("tests", AskKind::CodeSmall),
    ("debugging", AskKind::Debug),
    ("debug", AskKind::Debug),
    ("bugs", AskKind::Debug),
    ("ui", AskKind::Design),
    ("design", AskKind::Design),
    ("planning", AskKind::Plan),
    ("plans", AskKind::Plan),
    ("research", AskKind::Research),
    ("docs", AskKind::Explain),
    ("explanations", AskKind::Explain),
];

/// Words that cover every task of the chat.
const EVERY_TASK: &[&str] = &[
    "everything",
    "every task",
    "all tasks",
    "this chat",
    "the rest of this chat",
];

/// Effort words, as the person says them, to rungs on the ladder.
const EFFORT_WORDS: &[(&str, &str)] = &[
    ("extra high", "xhigh"),
    ("xhigh", "xhigh"),
    ("max", "xhigh"),
    ("high", "high"),
    ("medium", "medium"),
    ("low", "low"),
];

/// Words a model's name carries. "Use pytest for tests" names a tool, not a
/// model, so it asks nothing.
const MODEL_FAMILIES: &[&str] = &[
    "grok", "claude", "opus", "sonnet", "haiku", "fable", "gpt", "llama", "qwen", "mistral",
    "deepseek", "gemma", "glimmer", "local",
];

/// One ask: which tasks it covers (`None`: every task) and the model name or
/// effort it asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAsk {
    /// An [`AskKind::id`]; absent for every task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// The model's name as a lowercase, dash-joined id part (`opus`,
    /// `grok-4.3`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A rung name on the effort ladder (`high`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// What a chat message says about the chat's task asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    Set(TaskAsk),
    Clear,
}

/// Reads one message. `None` when it asks nothing of the kind.
pub fn read(text: &str) -> Option<Heard> {
    let words = super::intent::person_words(text).to_lowercase();
    if BACK_TO_AUTO.iter().any(|p| words.contains(p)) {
        return Some(Heard::Clear);
    }
    let rest = &words[find_word(&words, "use")? + "use".len()..];
    let at = rest.find(" for ")?;
    let what = rest[..at].trim();
    let target = rest[at + " for ".len()..]
        .split(['.', '!', '?', ',', ';', '\n'])
        .next()
        .unwrap_or("")
        .trim();
    let target = ["the ", "all ", "every ", "my ", "any "]
        .iter()
        .find_map(|p| target.strip_prefix(p))
        .unwrap_or(target)
        .trim();
    let kind = if EVERY_TASK.contains(&target) {
        None
    } else {
        Some(
            KIND_WORDS
                .iter()
                .find(|(w, _)| *w == target)?
                .1
                .id()
                .to_string(),
        )
    };
    let effort_of = |s: &str| {
        let s = s.strip_suffix(" effort").unwrap_or(s).trim();
        EFFORT_WORDS
            .iter()
            .find(|(w, _)| *w == s)
            .map(|(_, rung)| (*rung).to_string())
    };
    let (model, effort) = match effort_of(what) {
        Some(e) => (None, Some(e)),
        None => (Some(model_name(what)?), None),
    };
    Some(Heard::Set(TaskAsk {
        kind,
        model,
        effort,
    }))
}

/// The first place `word` stands alone in `text`.
fn find_word(text: &str, word: &str) -> Option<usize> {
    text.match_indices(word).map(|(i, _)| i).find(|&i| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// A model name as id text: "Grok 4.3" is `grok-4.3`. More than four words,
/// or no model family in it ("a hash map", "pytest"), is `None`.
fn model_name(what: &str) -> Option<String> {
    let what = what
        .strip_prefix("the ")
        .unwrap_or(what)
        .trim_end_matches(" model")
        .trim();
    let parts: Vec<&str> = what.split([' ', '_']).filter(|p| !p.is_empty()).collect();
    let named = parts
        .iter()
        .any(|p| MODEL_FAMILIES.iter().any(|f| p.starts_with(f)));
    (named && parts.len() <= 4).then(|| parts.join("-"))
}

/// True when model `id` carries the asked `name`.
pub fn names(id: &str, name: &str) -> bool {
    let id = id.to_ascii_lowercase().replace(['_', ' '], "-");
    !name.is_empty() && id.contains(name)
}

/// The chat a conversation belongs to: a helper's id (`chat/child`) and a
/// side answer's (`chat~side-1`) read their chat's asks.
pub fn chat_of(conversation_id: &str) -> &str {
    let id = conversation_id.split('/').next().unwrap_or(conversation_id);
    id.split(super::local_scale::SIDE_MARK).next().unwrap_or(id)
}

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

type Store = BTreeMap<String, Vec<TaskAsk>>;

fn load(config_dir: &Path) -> Store {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The asks chat `chat` has saved.
pub fn asks(config_dir: &Path, chat: &str) -> Vec<TaskAsk> {
    load(config_dir).remove(chat_of(chat)).unwrap_or_default()
}

/// Saves what a message of chat `chat` said. A new ask replaces the chat's
/// old ask for the same tasks and the same thing (model or effort). True
/// when the message said something.
pub fn hear(config_dir: &Path, chat: &str, text: &str) -> Result<bool, String> {
    let Some(heard) = read(text) else {
        return Ok(false);
    };
    let chat = chat_of(chat);
    if chat.is_empty() {
        return Ok(false);
    }
    let mut store = load(config_dir);
    match heard {
        Heard::Clear => {
            if store.remove(chat).is_none() {
                return Ok(true);
            }
        }
        Heard::Set(ask) => {
            let list = store.entry(chat.to_string()).or_default();
            list.retain(|a| {
                a.kind != ask.kind
                    || a.model.is_some() != ask.model.is_some()
                    || a.effort.is_some() != ask.effort.is_some()
            });
            list.push(ask);
        }
    }
    let text = serde_json::to_string_pretty(&store).map_err(|e| e.to_string())?;
    crate::harness::private_write(&path(config_dir), text.as_bytes())?;
    Ok(true)
}

/// The model name and effort the chat asked for a task of `kind`: an ask for
/// that kind wins over one for every task.
pub fn for_kind(asks: &[TaskAsk], kind: AskKind) -> (Option<String>, Option<String>) {
    let pick = |f: fn(&TaskAsk) -> Option<&String>| {
        asks.iter()
            .rev()
            .find(|a| a.kind.as_deref() == Some(kind.id()) && f(a).is_some())
            .or_else(|| {
                asks.iter()
                    .rev()
                    .find(|a| a.kind.is_none() && f(a).is_some())
            })
            .and_then(f)
            .cloned()
    };
    (pick(|a| a.model.as_ref()), pick(|a| a.effort.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(kind: Option<&str>, model: Option<&str>, effort: Option<&str>) -> Option<Heard> {
        Some(Heard::Set(TaskAsk {
            kind: kind.map(str::to_string),
            model: model.map(str::to_string),
            effort: effort.map(str::to_string),
        }))
    }

    #[test]
    fn words_name_a_model_or_an_effort_for_a_kind_of_task() {
        assert_eq!(
            read("use Opus for the refactor"),
            set(Some("code_multi"), Some("opus"), None)
        );
        assert_eq!(
            read("Please use Grok 4.3 for tests, thanks"),
            set(Some("code_small"), Some("grok-4.3"), None)
        );
        assert_eq!(
            read("use high effort for debugging"),
            set(Some("debug"), None, Some("high"))
        );
        assert_eq!(
            read("use extra high for all refactors"),
            set(Some("code_multi"), None, Some("xhigh"))
        );
        assert_eq!(
            read("use sonnet for everything"),
            set(None, Some("sonnet"), None)
        );
        assert_eq!(read("ok, back to auto"), Some(Heard::Clear));
    }

    #[test]
    fn other_uses_of_use_are_not_asks() {
        for text in [
            "use a hash map for the cache",
            "use a hash map for the tests",
            "use pytest for tests",
            "we use this for testing the parser",
            "users for the refactor",
            "what should I use?",
            "use opus",
            "",
        ] {
            assert_eq!(read(text), None, "{text}");
        }
    }

    #[test]
    fn a_named_model_matches_ids_that_carry_the_name() {
        assert!(names("anthropic/claude-opus-4.1", "opus"));
        assert!(names("grok-4.3-fast", "grok-4.3"));
        assert!(!names("grok-4.7", "grok-4.3"));
        assert!(!names("grok-4.7", ""));
    }

    #[test]
    fn helpers_and_side_answers_read_their_chats_asks() {
        assert_eq!(chat_of("c1/s2"), "c1");
        assert_eq!(chat_of("c1~side-3"), "c1");
        assert_eq!(chat_of("c1"), "c1");
    }

    #[test]
    fn asks_stick_per_chat_and_a_kinds_ask_wins_over_every_task() {
        let dir = crate::harness::test_dir("task-steer");
        assert_eq!(hear(&dir, "c1", "use sonnet for everything"), Ok(true));
        assert_eq!(hear(&dir, "c1", "use opus for the refactor"), Ok(true));
        assert_eq!(hear(&dir, "c1", "use high effort for refactors"), Ok(true));
        assert_eq!(hear(&dir, "c1", "use grok 4.3 for refactors"), Ok(true));
        assert_eq!(hear(&dir, "c1", "how is it going?"), Ok(false));
        let asks = asks(&dir, "c1/s4");
        assert_eq!(asks.len(), 3, "the newer refactor model replaced opus");
        assert_eq!(
            for_kind(&asks, AskKind::CodeMulti),
            (Some("grok-4.3".into()), Some("high".into()))
        );
        assert_eq!(
            for_kind(&asks, AskKind::Debug),
            (Some("sonnet".into()), None)
        );
        assert_eq!(super::asks(&dir, "c2"), Vec::new());
        assert_eq!(hear(&dir, "c1", "back to auto"), Ok(true));
        assert_eq!(super::asks(&dir, "c1"), Vec::new());
        let _ = std::fs::remove_dir_all(dir);
    }
}
