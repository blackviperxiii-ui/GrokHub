//! The person's interest profile: topics Pulse looks for, with a weight each.
//! Seeded once from USER.md, MEMORY.md, recent chats and liked cards; edited in
//! Settings → Pulse; nudged by what they open, save and skip.

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "interests.json";
/// Longest topic kept, in chars.
pub const TOPIC_CHARS: usize = 40;
/// Most topics a profile holds. Adding past this drops the weakest unpinned one.
pub const TOPICS_MAX: usize = 40;
/// Most topics one seeding pass adds.
pub const SEED_MAX: usize = 12;
/// Topics named in the lookup prompt, strongest first.
pub const STEER_MAX: usize = 6;
pub const WEIGHT_MIN: u8 = 5;
pub const WEIGHT_MAX: u8 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterestSource {
    /// From USER.md or MEMORY.md.
    Memory,
    /// From a recent chat title.
    Chats,
    /// From a card they liked or saved.
    Saved,
    /// Typed in Settings.
    Explicit,
}

impl InterestSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Memory => "From memory",
            Self::Chats => "From chats",
            Self::Saved => "From saved",
            Self::Explicit => "Added by you",
        }
    }

    /// Where a new topic starts.
    pub fn start_weight(self) -> u8 {
        match self {
            Self::Explicit => 70,
            Self::Memory => 60,
            Self::Saved => 55,
            Self::Chats => 40,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interest {
    pub topic: String,
    pub weight: u8,
    pub source: InterestSource,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub muted: bool,
    /// When it was added, or last named by a card the person opened, saved or skipped.
    #[serde(default)]
    pub last_seen_ms: u64,
}

/// What the person did with a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterestSignal {
    Opened,
    Saved,
    Accepted,
    Dismissed,
    NotInterested,
    /// More like this on a Pulse post.
    MoreLikeThis,
}

impl InterestSignal {
    pub fn delta(self) -> i16 {
        match self {
            Self::Opened => 4,
            Self::Saved => 10,
            Self::Accepted => 8,
            Self::Dismissed => -6,
            Self::NotInterested => -15,
            Self::MoreLikeThis => 12,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterestProfile {
    #[serde(default)]
    pub topics: Vec<Interest>,
    /// Seeding ran once; later passes leave the list to the person.
    #[serde(default)]
    pub seeded: bool,
    /// Lowercased topics the person removed. Seeding never brings them back.
    #[serde(default)]
    pub removed: Vec<String>,
}

/// Trimmed, single-spaced, no bullet or trailing period, at most TOPIC_CHARS.
pub fn clean_topic(raw: &str) -> String {
    let words: Vec<&str> = raw
        .trim()
        .trim_start_matches(['-', '*', '•'])
        .trim_end_matches(['.', '!', ';', ','])
        .split_whitespace()
        .collect();
    let joined = words.join(" ");
    let mut out: String = joined.chars().take(TOPIC_CHARS).collect();
    if out.chars().count() < joined.chars().count() {
        if let Some(cut) = out.rfind(' ') {
            out.truncate(cut);
        }
    }
    out.trim().to_string()
}

fn key(topic: &str) -> String {
    clean_topic(topic).to_lowercase()
}

impl InterestProfile {
    pub fn find(&self, topic: &str) -> Option<&Interest> {
        let k = key(topic);
        self.topics.iter().find(|t| t.topic.to_lowercase() == k)
    }

    fn find_mut(&mut self, topic: &str) -> Option<&mut Interest> {
        let k = key(topic);
        self.topics.iter_mut().find(|t| t.topic.to_lowercase() == k)
    }

    /// Add a topic. The person's own add always lands and clears an earlier
    /// removal; anything else skips topics they removed. False when nothing changed.
    pub fn add(&mut self, topic: &str, source: InterestSource, now_ms: u64) -> bool {
        let topic = clean_topic(topic);
        if topic.is_empty() {
            return false;
        }
        let k = topic.to_lowercase();
        if source == InterestSource::Explicit {
            self.removed.retain(|r| *r != k);
        } else if self.removed.contains(&k) {
            return false;
        }
        if let Some(have) = self.find_mut(&topic) {
            if source == InterestSource::Explicit && have.source != InterestSource::Explicit {
                have.source = InterestSource::Explicit;
                have.weight = have.weight.max(InterestSource::Explicit.start_weight());
                return true;
            }
            return false;
        }
        if self.topics.len() >= TOPICS_MAX && !self.drop_weakest() {
            return false;
        }
        self.topics.push(Interest {
            topic,
            weight: source.start_weight(),
            source,
            pinned: false,
            muted: false,
            last_seen_ms: now_ms,
        });
        true
    }

    fn drop_weakest(&mut self) -> bool {
        let weakest = self
            .topics
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.pinned && t.source != InterestSource::Explicit)
            .min_by_key(|(_, t)| (t.weight, t.last_seen_ms))
            .map(|(i, _)| i);
        match weakest {
            Some(i) => {
                self.topics.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, topic: &str) -> bool {
        let k = key(topic);
        let before = self.topics.len();
        self.topics.retain(|t| t.topic.to_lowercase() != k);
        if self.topics.len() == before {
            return false;
        }
        if !self.removed.contains(&k) {
            self.removed.push(k);
        }
        true
    }

    pub fn set_pinned(&mut self, topic: &str, pinned: bool) -> bool {
        match self.find_mut(topic) {
            Some(t) if t.pinned != pinned => {
                t.pinned = pinned;
                if pinned {
                    t.muted = false;
                }
                true
            }
            _ => false,
        }
    }

    pub fn set_muted(&mut self, topic: &str, muted: bool) -> bool {
        match self.find_mut(topic) {
            Some(t) if t.muted != muted => {
                t.muted = muted;
                if muted {
                    t.pinned = false;
                }
                true
            }
            _ => false,
        }
    }

    /// Nudge every topic the card's text names. A pinned topic never drops.
    /// Returns the topics that moved.
    pub fn feedback(&mut self, text: &str, signal: InterestSignal, now_ms: u64) -> Vec<String> {
        let delta = signal.delta();
        let mut moved = Vec::new();
        for t in &mut self.topics {
            if !names_topic(text, &t.topic) {
                continue;
            }
            t.last_seen_ms = now_ms;
            if delta < 0 && t.pinned {
                continue;
            }
            let next =
                (i16::from(t.weight) + delta).clamp(i16::from(WEIGHT_MIN), i16::from(WEIGHT_MAX));
            let next = u8::try_from(next).unwrap_or(WEIGHT_MIN);
            if next != t.weight {
                t.weight = next;
                moved.push(t.topic.clone());
            }
        }
        moved
    }

    /// Unmuted topics, pinned first, then by weight.
    pub fn ranked(&self) -> Vec<&Interest> {
        let mut out: Vec<&Interest> = self.topics.iter().filter(|t| !t.muted).collect();
        out.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.weight.cmp(&a.weight))
                .then(a.topic.cmp(&b.topic))
        });
        out
    }

    /// The prompt section the lookup follows. Empty when there is nothing to say.
    pub fn prompt_section(&self) -> String {
        let top: Vec<&str> = self
            .ranked()
            .into_iter()
            .take(STEER_MAX)
            .map(|t| t.topic.as_str())
            .collect();
        let muted: Vec<&str> = self
            .topics
            .iter()
            .filter(|t| t.muted)
            .map(|t| t.topic.as_str())
            .collect();
        let mut out = String::new();
        if !top.is_empty() {
            out.push_str(&format!(
                "Their interests, strongest first: {}.",
                top.join(", ")
            ));
        }
        if !muted.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&format!("Never about: {}.", muted.join(", ")));
        }
        out
    }

    /// One seeding pass from what the app already knows. Runs once.
    /// Returns how many topics it added.
    pub fn seed(&mut self, from: &SeedFrom<'_>, now_ms: u64) -> usize {
        if self.seeded {
            return 0;
        }
        self.seeded = true;
        let mut added = 0;
        let mut offer = |p: &mut Self, topic: &str, source: InterestSource| {
            if added < SEED_MAX && p.add(topic, source, now_ms) {
                added += 1;
            }
        };
        for raw in [from.user_md, from.memory_md] {
            for topic in topics_in_memory(raw) {
                offer(self, &topic, InterestSource::Memory);
            }
        }
        for title in from.saved_titles {
            if let Some(topic) = short_title(title) {
                offer(self, &topic, InterestSource::Saved);
            }
        }
        for title in from.chat_titles {
            if let Some(topic) = short_title(title) {
                offer(self, &topic, InterestSource::Chats);
            }
        }
        added
    }
}

/// What seeding reads. Chat titles newest first.
pub struct SeedFrom<'a> {
    pub user_md: &'a str,
    pub memory_md: &'a str,
    pub chat_titles: &'a [String],
    pub saved_titles: &'a [String],
}

/// Most chat titles seeding reads.
pub const SEED_CHATS: usize = 5;

/// Whole-word, case-insensitive: "rust" names "Rust 1.99 is out", not "trust".
pub fn names_topic(text: &str, topic: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let hay = words(text);
    let needle = words(topic);
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    hay.windows(needle.len()).any(|w| w == needle.as_slice())
}

const MEMORY_MARKERS: &[&str] = &[
    "interests:",
    "interested in",
    "into:",
    "hobbies:",
    "topics:",
    "i follow",
    "follows:",
    "cares about",
    "care about",
];

/// Topics from lines like "Interests: Rust, air compressors and xAI".
/// Pulse ledger lines and headings are skipped.
pub fn topics_in_memory(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.contains("] pulse: ") {
            continue;
        }
        let lower = line.to_lowercase();
        let Some((at, marker)) = MEMORY_MARKERS
            .iter()
            .filter_map(|m| lower.find(m).map(|at| (at, *m)))
            .min_by_key(|(at, _)| *at)
        else {
            continue;
        };
        let rest = &line[at + marker.len()..];
        let rest = rest.trim_start_matches([':', ' ']);
        for piece in rest.split([',', ';', '/']).flat_map(|p| p.split(" and ")) {
            let topic = clean_topic(piece);
            let words = topic.split_whitespace().count();
            if (1..=4).contains(&words) && !out.iter().any(|t| t.eq_ignore_ascii_case(&topic)) {
                out.push(topic);
            }
        }
    }
    out
}

const GENERIC_TITLES: &[&str] = &["new chat", "untitled", "chat", "scratch", "new session"];

/// A chat or card title short enough to be a topic, else None.
fn short_title(title: &str) -> Option<String> {
    let topic = clean_topic(title);
    let words = topic.split_whitespace().count();
    if !(1..=4).contains(&words) || GENERIC_TITLES.contains(&topic.to_lowercase().as_str()) {
        return None;
    }
    Some(topic)
}

pub fn load(path: &std::path::Path) -> InterestProfile {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &std::path::Path, profile: &InterestProfile) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Interests: {e}"))?;
    }
    let text = serde_json::to_string_pretty(profile).map_err(|e| format!("Interests: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("Interests: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Interests: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn seeding_reads_memory_lines_saved_cards_and_short_chat_titles() {
        let user = "# About me\nInterests: Rust, air compressors and xAI.\nWorks nights.";
        let memory = "- [2026-10-09] pulse: liked \"Tokio 2.0\" reason=liked\nI follow Formula 1";
        let chats = titles(&[
            "New chat",
            "Fix the cabin build on Windows please now",
            "Home espresso",
        ]);
        let saved = titles(&["Bambu printers"]);
        let mut p = InterestProfile::default();
        let n = p.seed(
            &SeedFrom {
                user_md: user,
                memory_md: memory,
                chat_titles: &chats,
                saved_titles: &saved,
            },
            7,
        );
        let got: Vec<(&str, InterestSource, u8)> = p
            .topics
            .iter()
            .map(|t| (t.topic.as_str(), t.source, t.weight))
            .collect();
        assert_eq!(
            got,
            vec![
                ("Rust", InterestSource::Memory, 60),
                ("air compressors", InterestSource::Memory, 60),
                ("xAI", InterestSource::Memory, 60),
                ("Formula 1", InterestSource::Memory, 60),
                ("Bambu printers", InterestSource::Saved, 55),
                ("Home espresso", InterestSource::Chats, 40),
            ]
        );
        assert_eq!(n, 6);
        assert!(p.seeded);
        assert_eq!(
            p.seed(
                &SeedFrom {
                    user_md: "Interests: Go",
                    memory_md: "",
                    chat_titles: &[],
                    saved_titles: &[]
                },
                8
            ),
            0
        );
        assert!(p.find("Go").is_none());
    }

    #[test]
    fn a_removed_topic_stays_gone_until_the_person_adds_it_back() {
        let mut p = InterestProfile::default();
        assert!(p.add("Rust", InterestSource::Memory, 1));
        assert!(p.remove("rust"));
        assert_eq!(p.removed, vec!["rust".to_string()]);
        assert!(!p.add("Rust", InterestSource::Chats, 2));
        assert!(p.add("Rust", InterestSource::Explicit, 3));
        assert_eq!(
            p.find("RUST").map(|t| (t.weight, t.source)),
            Some((70, InterestSource::Explicit))
        );
        assert!(p.removed.is_empty());
    }

    #[test]
    fn feedback_moves_matching_topics_and_spares_pinned_ones() {
        let mut p = InterestProfile::default();
        p.add("Rust", InterestSource::Memory, 0);
        p.add("air compressors", InterestSource::Chats, 0);
        p.add("xAI", InterestSource::Explicit, 0);
        p.set_pinned("xAI", true);
        assert_eq!(
            p.feedback("Rust 1.99 ships async closures", InterestSignal::Saved, 50),
            vec!["Rust"]
        );
        assert_eq!(
            p.find("Rust").map(|t| (t.weight, t.last_seen_ms)),
            Some((70, 50))
        );
        // "trust" is not "Rust".
        assert!(p
            .feedback("Why trust matters", InterestSignal::Saved, 60)
            .is_empty());
        p.feedback(
            "New air compressors for home shops",
            InterestSignal::NotInterested,
            70,
        );
        assert_eq!(p.find("air compressors").map(|t| t.weight), Some(25));
        assert!(p
            .feedback("xAI raises again", InterestSignal::NotInterested, 80)
            .is_empty());
        assert_eq!(
            p.find("xAI").map(|t| (t.weight, t.last_seen_ms)),
            Some((70, 80))
        );
        for _ in 0..10 {
            p.feedback("air compressors", InterestSignal::NotInterested, 90);
        }
        assert_eq!(
            p.find("air compressors").map(|t| t.weight),
            Some(WEIGHT_MIN)
        );
    }

    #[test]
    fn prompt_section_puts_pinned_first_and_names_muted_topics() {
        let mut p = InterestProfile::default();
        p.add("Rust", InterestSource::Memory, 0);
        p.add("xAI", InterestSource::Chats, 0);
        p.add("crypto", InterestSource::Memory, 0);
        p.set_pinned("xAI", true);
        p.set_muted("crypto", true);
        assert_eq!(
            p.prompt_section(),
            "Their interests, strongest first: xAI, Rust. Never about: crypto."
        );
        assert_eq!(InterestProfile::default().prompt_section(), "");
        // Pin and mute exclude each other.
        p.set_pinned("crypto", true);
        assert_eq!(
            p.find("crypto").map(|t| (t.pinned, t.muted)),
            Some((true, false))
        );
    }

    #[test]
    fn a_full_profile_drops_its_weakest_learned_topic_for_a_new_one() {
        let mut p = InterestProfile::default();
        for i in 0..TOPICS_MAX {
            p.add(&format!("topic {i}"), InterestSource::Explicit, 0);
        }
        assert!(!p.add("one more", InterestSource::Chats, 1));
        p.topics[3].source = InterestSource::Chats;
        p.topics[3].weight = 10;
        assert!(p.add("one more", InterestSource::Chats, 1));
        assert_eq!(p.topics.len(), TOPICS_MAX);
        assert!(p.find("topic 3").is_none());
    }

    #[test]
    fn clean_topic_trims_bullets_and_cuts_on_a_word() {
        assert_eq!(clean_topic("  - Rust   lang.  "), "Rust lang");
        assert_eq!(
            clean_topic("an extremely long topic name that keeps going and going"),
            "an extremely long topic name that keeps"
        );
    }

    #[test]
    fn the_profile_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("grokhub-interests-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(FILE_NAME);
        assert_eq!(load(&path), InterestProfile::default());
        let mut p = InterestProfile::default();
        p.add("Rust", InterestSource::Explicit, 9);
        p.set_muted("Rust", true);
        save(&path, &p).expect("save");
        assert_eq!(load(&path), p);
        std::fs::write(&path, "not json").expect("write");
        assert_eq!(load(&path), InterestProfile::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
