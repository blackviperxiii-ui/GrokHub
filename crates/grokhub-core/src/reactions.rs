//! How the user reacts to suggestions: each Pulse idea, Feed post and
//! proactive card that is tapped, accepted, dismissed, snoozed or ignored
//! moves a score for its category. The score lifts or lowers how often that
//! kind shows up in Pulse ranking and in the proactive card picker. Three
//! ignores in a row quiet the category until the user asks for more, and the
//! cabin says so once.
//!
//! Stored locally in `{config}/reactions.json`: the outcome, the category and
//! a short source tag. Never card text or message content.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::proactive::CandidateSource;
use crate::pulse::{card_category, is_idea_card};
use crate::pulse_sources::is_source_post;
use crate::update_feed::{UpdateCard, UpdateStatus};

pub const FILE_NAME: &str = "reactions.json";
/// A category's score stays in `-SCORE_MAX..=SCORE_MAX`.
pub const SCORE_MAX: i32 = 10;
/// Ignored this many times in a row quiets the category.
pub const QUIET_AFTER_IGNORES: u32 = 3;
/// Pulse rank points per score point.
pub const RANK_PER_POINT: i64 = 20;
/// A card shown this long without a tap counts as ignored.
pub const IGNORED_AFTER_MS: u64 = 24 * 60 * 60 * 1000;
/// Outcome rows kept for Settings.
pub const LOG_MAX: usize = 200;
/// Card ids already counted as ignored, so one card counts once.
const IGNORED_IDS_MAX: usize = 500;

/// What the user did with one suggestion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Tapped,
    Accepted,
    Dismissed,
    Snoozed,
    Ignored,
}

impl Outcome {
    /// How far one outcome moves the category's score.
    pub fn delta(self) -> i32 {
        match self {
            Self::Accepted => 2,
            Self::Tapped => 1,
            Self::Snoozed | Self::Ignored => -1,
            Self::Dismissed => -2,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryScore {
    #[serde(default)]
    pub score: i32,
    /// Ignores in a row; any other outcome resets it.
    #[serde(default)]
    pub ignored_run: u32,
    /// Quieted: nothing of this kind surfaces until More like this or a reset.
    #[serde(default)]
    pub quiet: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub at_ms: u64,
    pub outcome: Outcome,
    pub category: String,
    pub tag: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reactions {
    #[serde(default)]
    pub categories: BTreeMap<String, CategoryScore>,
    #[serde(default)]
    pub log: Vec<Reaction>,
    #[serde(default)]
    pub ignored_ids: BTreeSet<String>,
}

impl Reactions {
    /// Records one outcome. Returns the category when this outcome just
    /// quieted it, so the cabin can say so once.
    pub fn record(
        &mut self,
        outcome: Outcome,
        category: &str,
        tag: &str,
        now_ms: u64,
    ) -> Option<String> {
        let entry = self.categories.entry(category.to_string()).or_default();
        entry.score = (entry.score + outcome.delta()).clamp(-SCORE_MAX, SCORE_MAX);
        let mut quieted = None;
        if outcome == Outcome::Ignored {
            entry.ignored_run += 1;
            if entry.ignored_run >= QUIET_AFTER_IGNORES && !entry.quiet {
                entry.quiet = true;
                quieted = Some(category.to_string());
            }
        } else {
            entry.ignored_run = 0;
        }
        self.log.push(Reaction {
            at_ms: now_ms,
            outcome,
            category: category.to_string(),
            tag: tag.to_string(),
        });
        if self.log.len() > LOG_MAX {
            let extra = self.log.len() - LOG_MAX;
            self.log.drain(..extra);
        }
        quieted
    }

    pub fn score(&self, category: &str) -> i32 {
        self.categories.get(category).map_or(0, |c| c.score)
    }

    pub fn quiet(&self, category: &str) -> bool {
        self.categories.get(category).is_some_and(|c| c.quiet)
    }

    /// Pulse rank points for a category: `score × RANK_PER_POINT`.
    pub fn rank_boost(&self, category: &str) -> i64 {
        i64::from(self.score(category)) * RANK_PER_POINT
    }

    /// The factor a proactive candidate's value is scaled by: 0 when quiet,
    /// else `1 + score / 20`, so 0.5 to 1.5.
    pub fn weight(&self, category: &str) -> f64 {
        if self.quiet(category) {
            return 0.0;
        }
        1.0 + f64::from(self.score(category)) / (2.0 * f64::from(SCORE_MAX))
    }

    /// More like this: lifts the score and takes the category out of quiet.
    pub fn more(&mut self, category: &str) {
        let entry = self.categories.entry(category.to_string()).or_default();
        entry.score = (entry.score + 3).clamp(-SCORE_MAX, SCORE_MAX);
        entry.quiet = false;
        entry.ignored_run = 0;
    }

    /// Less like this: lowers the score. Quiet stays the user's own call.
    pub fn less(&mut self, category: &str) {
        let entry = self.categories.entry(category.to_string()).or_default();
        entry.score = (entry.score - 3).clamp(-SCORE_MAX, SCORE_MAX);
    }

    /// Settings → Reset: everything learned is gone.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Cards shown at least [`IGNORED_AFTER_MS`] and never opened or acted
    /// on, each counted once. Returns the categories this quieted.
    pub fn sweep_ignored(&mut self, cards: &[UpdateCard], now_ms: u64) -> Vec<String> {
        let mut quieted = Vec::new();
        for card in cards {
            let untouched = card.status == UpdateStatus::Unread
                && card.reaction.is_none()
                && card.pulse.snoozed_until == 0
                && !card.held;
            if !untouched
                || now_ms.saturating_sub(card.created_at) < IGNORED_AFTER_MS
                || self.ignored_ids.contains(&card.id)
            {
                continue;
            }
            let Some((category, tag)) = card_category_tag(card) else {
                continue;
            };
            self.ignored_ids.insert(card.id.clone());
            quieted.extend(self.record(Outcome::Ignored, &category, &tag, now_ms));
        }
        while self.ignored_ids.len() > IGNORED_IDS_MAX {
            let first = self.ignored_ids.iter().next().cloned();
            if let Some(id) = first {
                self.ignored_ids.remove(&id);
            }
        }
        quieted
    }
}

/// The category a proactive candidate's source falls under.
pub fn source_category(source: CandidateSource) -> &'static str {
    match source {
        CandidateSource::Pulse => "pulse follow-ups",
        CandidateSource::Workboard => "coding next step",
        CandidateSource::AutomationHealth => "automation health",
        CandidateSource::Calendar => "calendar",
        CandidateSource::Mail => "mail",
        CandidateSource::SystemState => "system",
        CandidateSource::Spans => "unfinished turns",
        CandidateSource::UserModel => "your routines",
    }
}

/// `(category, source tag)` for a card that learns, or None for cards that
/// are not suggestions (run reports, schedule notes, self changes).
pub fn card_category_tag(card: &UpdateCard) -> Option<(String, String)> {
    if let Some(p) = &card.pulse.proactive {
        let category = p
            .source
            .map(source_category)
            .unwrap_or("suggestions")
            .to_string();
        return Some((category, p.topic.clone()));
    }
    if is_source_post(card) {
        let source = card.pulse.source_name.clone().unwrap_or_default();
        let category = if source.ends_with(" on Steam") {
            "game news".to_string()
        } else if let Some(kind) = feed_kind(&card.source_id) {
            format!("{kind} posts")
        } else {
            "feed posts".to_string()
        };
        let tag = if source.is_empty() {
            "feed".to_string()
        } else {
            source
        };
        return Some((category, tag));
    }
    is_idea_card(card).then(|| {
        (
            format!("{} ideas", card_category(card).key()),
            "Pulse ideas".to_string(),
        )
    })
}

/// `source:<kind>:<url>` → the kind's key ("hn", "reddit", …).
fn feed_kind(source_id: &str) -> Option<&str> {
    let rest = source_id.strip_prefix("source:")?;
    rest.split(':').next().filter(|k| !k.is_empty())
}

/// The "Why am I seeing this?" line: source tag, category and its score.
pub fn why_line(reactions: &Reactions, category: &str, tag: &str) -> String {
    let score = reactions.score(category);
    let sign = if score > 0 { "+" } else { "" };
    format!("From {tag} · {category} {sign}{score}")
}

pub fn load(path: &Path) -> Reactions {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, state: &Reactions) -> Result<(), String> {
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update_feed::{blank_card, UpdateKind};

    const T0: u64 = 1_800_000_000_000;

    #[test]
    fn scores_move_up_on_accept_and_down_on_dismiss() {
        let mut r = Reactions::default();
        r.record(Outcome::Accepted, "game news", "Dota 2 on Steam", T0);
        r.record(Outcome::Tapped, "game news", "Dota 2 on Steam", T0);
        assert_eq!(r.score("game news"), 3);
        assert_eq!(r.rank_boost("game news"), 60);
        assert!((r.weight("game news") - 1.15).abs() < 1e-9);
        r.record(Outcome::Dismissed, "disk", "low disk", T0);
        r.record(Outcome::Snoozed, "disk", "low disk", T0);
        assert_eq!(r.score("disk"), -3);
        assert_eq!(r.rank_boost("disk"), -60);
        assert!((r.weight("disk") - 0.85).abs() < 1e-9);
        assert_eq!(r.score("never seen"), 0);
        assert_eq!(r.weight("never seen"), 1.0);
    }

    #[test]
    fn scores_stay_inside_the_cap() {
        let mut r = Reactions::default();
        for _ in 0..20 {
            r.record(Outcome::Dismissed, "mail", "inbox", T0);
        }
        assert_eq!(r.score("mail"), -SCORE_MAX);
        assert_eq!(r.weight("mail"), 0.5);
    }

    #[test]
    fn three_ignores_in_a_row_quiet_the_category_once_and_more_undoes_it() {
        let mut r = Reactions::default();
        assert_eq!(
            r.record(Outcome::Ignored, "hn posts", "Hacker News", T0),
            None
        );
        assert_eq!(
            r.record(Outcome::Ignored, "hn posts", "Hacker News", T0),
            None
        );
        // A tap in between starts the run over.
        r.record(Outcome::Tapped, "hn posts", "Hacker News", T0);
        assert_eq!(
            r.record(Outcome::Ignored, "hn posts", "Hacker News", T0),
            None
        );
        assert_eq!(
            r.record(Outcome::Ignored, "hn posts", "Hacker News", T0),
            None
        );
        assert_eq!(
            r.record(Outcome::Ignored, "hn posts", "Hacker News", T0),
            Some("hn posts".to_string())
        );
        assert!(r.quiet("hn posts"));
        assert_eq!(r.weight("hn posts"), 0.0);
        // Said once: a fourth ignore does not announce again.
        assert_eq!(
            r.record(Outcome::Ignored, "hn posts", "Hacker News", T0),
            None
        );
        r.more("hn posts");
        assert!(!r.quiet("hn posts"));
        // Six ignores and a tap left -5; more lifts it by 3.
        assert_eq!(r.score("hn posts"), -2);
    }

    #[test]
    fn reset_clears_scores_quiet_and_log() {
        let mut r = Reactions::default();
        for _ in 0..3 {
            r.record(Outcome::Ignored, "reddit posts", "r/rust", T0);
        }
        r.less("game news");
        assert!(r.quiet("reddit posts"));
        r.reset();
        assert_eq!(r, Reactions::default());
        assert!(!r.quiet("reddit posts"));
        assert_eq!(r.score("game news"), 0);
    }

    fn feed_post(id: &str, source_id: &str, source_name: &str, created: u64) -> UpdateCard {
        let mut c = blank_card(
            id.to_string(),
            UpdateKind::Digest,
            "A post".into(),
            None,
            created,
        );
        c.source_id = source_id.into();
        c.pulse.source_name = Some(source_name.into());
        c
    }

    #[test]
    fn cards_map_to_a_category_and_a_source_tag() {
        let steam = feed_post(
            "a",
            "source:rss:https://store.steampowered.com/feeds/news/app/570",
            "Dota 2 on Steam",
            T0,
        );
        assert_eq!(
            card_category_tag(&steam),
            Some(("game news".into(), "Dota 2 on Steam".into()))
        );
        let hn = feed_post("b", "source:hn:https://example.com/x", "Hacker News", T0);
        assert_eq!(
            card_category_tag(&hn),
            Some(("hn posts".into(), "Hacker News".into()))
        );
        let report = blank_card(
            "c".into(),
            UpdateKind::AutomationDone,
            "Nightly ran".into(),
            None,
            T0,
        );
        assert_eq!(card_category_tag(&report), None);
        let mut cand = crate::proactive::Candidate::soft(
            CandidateSource::Workboard,
            "pick up the parser fix",
            "parser fix",
            0.8,
            0.9,
            "blocked card",
        );
        cand.reversible = crate::proactive::Reversibility::Ledger;
        let card =
            crate::proactive::proactive_card(&cand, crate::proactive::ProactiveRoute::ICan, T0);
        assert_eq!(
            card_category_tag(&card),
            Some(("coding next step".into(), "parser fix".into()))
        );
    }

    #[test]
    fn an_untouched_card_counts_as_ignored_once_after_a_day() {
        let mut r = Reactions::default();
        let cards = vec![
            feed_post("old", "source:hn:https://example.com/1", "Hacker News", T0),
            feed_post(
                "new",
                "source:hn:https://example.com/2",
                "Hacker News",
                T0 + IGNORED_AFTER_MS,
            ),
        ];
        assert!(r.sweep_ignored(&cards, T0 + IGNORED_AFTER_MS).is_empty());
        assert_eq!(r.score("hn posts"), -1);
        assert_eq!(r.log.len(), 1);
        // Swept again: the same card is not counted twice.
        r.sweep_ignored(&cards, T0 + IGNORED_AFTER_MS + 1);
        assert_eq!(r.score("hn posts"), -1);
        // An opened card is never ignored.
        let mut opened = feed_post("seen", "source:hn:https://example.com/3", "Hacker News", T0);
        opened.status = UpdateStatus::Opened;
        r.sweep_ignored(&[opened], T0 + 2 * IGNORED_AFTER_MS);
        assert_eq!(r.score("hn posts"), -1);
    }

    #[test]
    fn why_line_names_the_tag_category_and_score() {
        let mut r = Reactions::default();
        assert_eq!(
            why_line(&r, "game news", "Dota 2 on Steam"),
            "From Dota 2 on Steam · game news 0"
        );
        r.record(Outcome::Accepted, "game news", "Dota 2 on Steam", T0);
        assert_eq!(
            why_line(&r, "game news", "Dota 2 on Steam"),
            "From Dota 2 on Steam · game news +2"
        );
        r.less("game news");
        assert_eq!(
            why_line(&r, "game news", "Dota 2 on Steam"),
            "From Dota 2 on Steam · game news -1"
        );
    }

    #[test]
    fn saved_state_round_trips_without_card_text() {
        let dir = std::env::temp_dir().join(format!("grokhub-reactions-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        let mut r = Reactions::default();
        r.record(Outcome::Accepted, "game news", "Dota 2 on Steam", T0);
        save(&path, &r).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("A post"));
        assert_eq!(load(&path), r);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
