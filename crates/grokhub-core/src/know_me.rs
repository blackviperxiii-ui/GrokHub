//! Optional "Know me" tags (Command Centre card 41). Nudge watchers read tags
//! only through this module so they keep working when the scan has not landed yet.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One tag from a Know me scan. Metadata only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowMeTag {
    pub label: String,
    #[serde(default)]
    pub topic: String,
}

/// Tags from disk, or empty when Know me is absent or unreadable.
pub fn load_tags(config_dir: &Path) -> Vec<KnowMeTag> {
    let path = config_dir.join("know-me.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Topic strings for watchers that want to match interests. Empty is fine.
pub fn tag_topics(tags: &[KnowMeTag]) -> Vec<String> {
    tags.iter()
        .map(|t| {
            let topic = t.topic.trim();
            if topic.is_empty() {
                t.label.trim()
            } else {
                topic
            }
        })
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

/// True when any tag mentions one of the words (whole-token-ish match).
pub fn tags_mention(tags: &[KnowMeTag], words: &[&str]) -> bool {
    if tags.is_empty() || words.is_empty() {
        return false;
    }
    let topics = tag_topics(tags);
    words.iter().any(|w| {
        let w = w.to_ascii_lowercase();
        topics.iter().any(|t| t.contains(&w) || w.contains(t.as_str()))
    })
}
