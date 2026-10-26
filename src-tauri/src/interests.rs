//! Settings → Pulse → Interests (shell parity): the interest profile in
//! `{config}/interests.json` that steers every Pulse lookup, the same file
//! the cabin learns from. Pinned topics come first; muted ones never show.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use grokhub_core::interests::{self, Interest, InterestProfile, InterestSource};
use serde::{Deserialize, Serialize};

/// One read-change-write at a time from this app.
static STORE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub topic: String,
    pub pinned: bool,
    pub muted: bool,
    /// "From memory", "From chats", "From saved" or "Added by you".
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// Pinned, then the rest by weight, then muted at the bottom.
    pub topics: Vec<Topic>,
    /// What the last change did.
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Act {
    Pin { on: bool },
    Mute { on: bool },
    Remove,
}

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(interests::FILE_NAME)
}

fn page(profile: &InterestProfile, note: String) -> Page {
    let mut rows: Vec<&Interest> = profile.topics.iter().collect();
    rows.sort_by(|a, b| {
        a.muted
            .cmp(&b.muted)
            .then(b.pinned.cmp(&a.pinned))
            .then(b.weight.cmp(&a.weight))
            .then(a.topic.cmp(&b.topic))
    });
    Page {
        topics: rows
            .into_iter()
            .map(|t| Topic {
                topic: t.topic.clone(),
                pinned: t.pinned,
                muted: t.muted,
                source: t.source.label().to_string(),
            })
            .collect(),
        note,
    }
}

pub fn load(config_dir: &Path) -> Page {
    page(&interests::load(&path(config_dir)), String::new())
}

pub fn add(config_dir: &Path, text: &str) -> Result<Page, String> {
    let topic = interests::clean_topic(text);
    if topic.is_empty() {
        return Err("Type a topic first.".into());
    }
    let _held = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let mut profile = interests::load(&path(config_dir));
    let note = if profile.add(&topic, InterestSource::Explicit, grokhub_core::now_ms()) {
        interests::save(&path(config_dir), &profile)?;
        format!("Pulse will look for {topic}.")
    } else {
        format!("{topic} is already on your list.")
    };
    Ok(page(&profile, note))
}

/// A Pulse idea's More like this or Not interested moves every topic its
/// text names (a pinned topic never drops). Returns the topics that moved.
pub fn feedback(
    config_dir: &Path,
    text: &str,
    signal: interests::InterestSignal,
    now_ms: u64,
) -> Result<Vec<String>, String> {
    let _held = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let mut profile = interests::load(&path(config_dir));
    let moved = profile.feedback(text, signal, now_ms);
    if !moved.is_empty() {
        interests::save(&path(config_dir), &profile)?;
    }
    Ok(moved)
}

pub fn act(config_dir: &Path, topic: &str, act: Act) -> Result<Page, String> {
    let _held = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let mut profile = interests::load(&path(config_dir));
    let (changed, note) = match act {
        Act::Pin { on } => (
            profile.set_pinned(topic, on),
            if on {
                format!("Pinned {topic}.")
            } else {
                format!("Unpinned {topic}.")
            },
        ),
        Act::Mute { on } => (
            profile.set_muted(topic, on),
            if on {
                format!("Muted {topic}.")
            } else {
                format!("Unmuted {topic}.")
            },
        ),
        Act::Remove => (
            profile.remove(topic),
            format!("Removed {topic}. It won't come back."),
        ),
    };
    if !changed {
        return Ok(page(&profile, String::new()));
    }
    interests::save(&path(config_dir), &profile)?;
    Ok(page(&profile, note))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gh-shell-interests-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(page: &Page) -> Vec<&str> {
        page.topics.iter().map(|t| t.topic.as_str()).collect()
    }

    #[test]
    fn add_pin_mute_and_remove_land_in_interests_json() {
        let cfg = scratch("flow");
        assert_eq!(load(&cfg).topics, Vec::new());
        let page = add(&cfg, "  air compressors ").unwrap();
        assert_eq!(page.note, "Pulse will look for air compressors.");
        assert_eq!(
            page.topics,
            vec![Topic {
                topic: "air compressors".into(),
                pinned: false,
                muted: false,
                source: "Added by you".into(),
            }]
        );
        assert_eq!(
            add(&cfg, "air compressors").unwrap().note,
            "air compressors is already on your list."
        );
        add(&cfg, "sourdough").unwrap();

        let page = act(&cfg, "sourdough", Act::Pin { on: true }).unwrap();
        assert_eq!(page.note, "Pinned sourdough.");
        assert_eq!(names(&page), vec!["sourdough", "air compressors"]);

        let page = act(&cfg, "sourdough", Act::Mute { on: true }).unwrap();
        assert_eq!(page.note, "Muted sourdough.");
        assert_eq!(names(&page), vec!["air compressors", "sourdough"]);
        assert!(page.topics[1].muted);

        let page = act(&cfg, "air compressors", Act::Remove).unwrap();
        assert_eq!(page.note, "Removed air compressors. It won't come back.");
        assert_eq!(names(&page), vec!["sourdough"]);
        let saved = interests::load(&path(&cfg));
        assert_eq!(saved.topics.len(), 1);
        assert!(saved.topics[0].muted);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn an_empty_topic_is_refused_and_an_unknown_one_changes_nothing() {
        let cfg = scratch("empty");
        assert_eq!(add(&cfg, "   "), Err("Type a topic first.".to_string()));
        let page = act(&cfg, "nothing", Act::Remove).unwrap();
        assert_eq!((page.topics.len(), page.note.as_str()), (0, ""));
        assert!(!path(&cfg).exists());
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn acts_read_their_kind() {
        let acts: Vec<Act> = serde_json::from_str(
            r#"[{"kind":"pin","on":true},{"kind":"mute","on":false},{"kind":"remove"}]"#,
        )
        .unwrap();
        assert_eq!(
            acts,
            vec![Act::Pin { on: true }, Act::Mute { on: false }, Act::Remove]
        );
    }
}
