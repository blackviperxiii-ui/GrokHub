//! Pulse Ideas the model writes in the background, Muse style: personal
//! first, one fixed category each, an emoji, a first-person "I can …" line
//! and the exact action to run. They come from the person's goals, interest
//! topics, recent chats and recent feed posts, at most once every six hours,
//! and retire after two weeks (`pulse::IDEA_TTL_MS`).
//!
//! More like this is remembered for the next run; Not interested is
//! remembered as turned down so the same idea does not come back.

use serde::{Deserialize, Serialize};

use crate::ideas::same_topic;
use crate::pulse::{PulseCategory, IDEA_TTL_MS};
use crate::redact::is_plain_text;
use crate::update_feed::{idea_card, post_update, UpdateCard, UpdateKind};

/// Source id prefix on the cards this job writes.
pub const IDEA_SOURCE: &str = "pulse:idea";
pub const STATE_FILE: &str = "pulse-ideas.json";
/// At most one run this often.
pub const EVERY_MS: u64 = 6 * 60 * 60 * 1000;
/// At most this many ideas per run.
pub const PER_RUN: usize = 4;
/// Titles kept for More like this (the next run reads them).
pub const MORE_LIKE_MAX: usize = 5;
/// Titles kept as turned down.
pub const TURNED_DOWN_MAX: usize = 30;
/// The emoji on an idea that came without one.
pub const DEFAULT_ICON: &str = "💡";

/// The categories an idea may take, in the Ideas view's order.
pub const CATEGORIES: [PulseCategory; 5] = [
    PulseCategory::Productivity,
    PulseCategory::Financial,
    PulseCategory::Shopping,
    PulseCategory::Relationships,
    PulseCategory::Health,
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IdeasState {
    pub last_run_ms: u64,
    /// Ideas the person asked for more of, newest first.
    pub more_like: Vec<String>,
    /// Ideas the person was not interested in, newest first.
    pub turned_down: Vec<String>,
}

fn push_front(list: &mut Vec<String>, title: &str, max: usize) {
    let title = title.trim();
    if title.is_empty() {
        return;
    }
    list.retain(|t| !t.eq_ignore_ascii_case(title));
    list.insert(0, title.to_string());
    list.truncate(max);
}

impl IdeasState {
    /// The six-hour gate.
    pub fn due(&self, now_ms: u64) -> bool {
        self.last_run_ms == 0 || now_ms.saturating_sub(self.last_run_ms) >= EVERY_MS
    }

    pub fn note_more_like(&mut self, title: &str) {
        push_front(&mut self.more_like, title, MORE_LIKE_MAX);
    }

    pub fn note_not_interested(&mut self, title: &str) {
        self.more_like
            .retain(|t| !t.eq_ignore_ascii_case(title.trim()));
        push_front(&mut self.turned_down, title, TURNED_DOWN_MAX);
    }
}

pub fn load_state(path: &std::path::Path) -> IdeasState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_state(path: &std::path::Path, state: &IdeasState) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(path, &raw)
}

static RULES: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    crate::brand::filled("You suggest ideas for one person's Pulse in {product}, a desktop assistant on their own computer. \
{product} can run commands and edit files, search the web, schedule automations, keep notes and drafts, and make images.

Write at most 4 ideas, personal first: each one must come from their goals, topics, recent chats or feed posts below. \
No generic tips. If nothing fits, return [].

Reply with only a JSON array. Each item:
{\"title\": short name of the idea, at most 60 characters,
 \"description\": one line in your own voice that starts with \"I can\", at most 140 characters,
 \"emoji\": one emoji for its topic,
 \"category\": one of productivity, financial, shopping, relationships, health,
 \"action\": the exact message to send GrokHub to do it}

Put the most personal ideas first. Do not repeat or overlap anything under Existing ideas or Turned down. \
When More like this lists ideas, write new ones in the same spirit.")
});

/// What one run may look at. Every list is optional.
#[derive(Debug, Clone, Default)]
pub struct IdeaInput<'a> {
    pub goals: &'a [String],
    /// The interest topics section (`InterestProfile::prompt_section`).
    pub topics: &'a str,
    pub chats: &'a [String],
    pub posts: &'a [String],
    pub more_like: &'a [String],
    pub turned_down: &'a [String],
    pub existing: &'a [String],
}

fn push_list(out: &mut String, head: &str, items: &[String], max: usize) {
    let picked: Vec<&str> = items
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && is_plain_text(s))
        .take(max)
        .collect();
    if picked.is_empty() {
        return;
    }
    out.push_str(&format!("\n{head}:\n"));
    for item in picked {
        let line: String = item.chars().take(200).collect();
        out.push_str(&format!("- {line}\n"));
    }
}

/// The system rules and the person's context, or None when there is
/// nothing personal to go on.
pub fn idea_messages(input: &IdeaInput) -> Option<Vec<(String, String)>> {
    let mut user = String::new();
    push_list(&mut user, "Their goals", input.goals, 8);
    let topics = input.topics.trim();
    if !topics.is_empty() && is_plain_text(topics) {
        user.push_str(&format!("\n{topics}\n"));
    }
    push_list(&mut user, "What they said in chat lately", input.chats, 10);
    push_list(&mut user, "Recent feed posts", input.posts, 10);
    if user.trim().is_empty() {
        return None;
    }
    push_list(&mut user, "More like this", input.more_like, MORE_LIKE_MAX);
    push_list(&mut user, "Existing ideas", input.existing, 20);
    push_list(&mut user, "Turned down", input.turned_down, TURNED_DOWN_MAX);
    Some(vec![
        ("system".into(), (*RULES).into()),
        ("user".into(), user.trim_start().to_string()),
    ])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuseIdea {
    pub title: String,
    pub description: String,
    pub emoji: String,
    pub category: PulseCategory,
    pub action: String,
}

#[derive(Deserialize)]
struct RawIdea {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    emoji: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    action: String,
}

/// One of the five categories, by key or by its label's first word
/// ("Health & Fitness" is health). Anything else is refused.
pub fn category_of(raw: &str) -> Option<PulseCategory> {
    let lower = raw.trim().to_ascii_lowercase();
    let first = lower.split([' ', '&', '/']).next().unwrap_or("");
    CATEGORIES.into_iter().find(|c| c.key() == first)
}

fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The JSON array in the reply. Items that break a rule are dropped: no
/// "I can" line, a category outside the five, a title out of range, a
/// secret-looking value, or the same topic as an existing or turned-down
/// idea or one earlier in the reply.
pub fn parse_ideas(raw: &str, existing: &[String], turned_down: &[String]) -> Vec<MuseIdea> {
    let (Some(start), Some(end)) = (raw.find('['), raw.rfind(']')) else {
        return Vec::new();
    };
    if end < start {
        return Vec::new();
    }
    let Ok(items) = serde_json::from_str::<Vec<RawIdea>>(&raw[start..=end]) else {
        return Vec::new();
    };
    let mut out: Vec<MuseIdea> = Vec::new();
    for item in items {
        if out.len() >= PER_RUN {
            break;
        }
        let title = clean(item.title.trim_matches(['"', '*']));
        let description = clean(&item.description);
        let action = clean(&item.action);
        let Some(category) = category_of(&item.category) else {
            continue;
        };
        if !(4..=60).contains(&title.chars().count())
            || !(12..=140).contains(&description.chars().count())
            || !(8..=400).contains(&action.chars().count())
            || !description.to_ascii_lowercase().starts_with("i can ")
        {
            continue;
        }
        if ![&title, &description, &action]
            .iter()
            .all(|s| is_plain_text(s))
        {
            continue;
        }
        let text = format!("{title} {description}");
        let taken = existing
            .iter()
            .chain(turned_down)
            .chain(out.iter().map(|i| &i.title))
            .any(|t| t.trim().eq_ignore_ascii_case(&title) || same_topic(t, &text));
        if taken {
            continue;
        }
        let emoji = item.emoji.trim();
        let emoji = if emoji.is_empty() || emoji.chars().count() > 8 || emoji.is_ascii() {
            DEFAULT_ICON.to_string()
        } else {
            emoji.to_string()
        };
        out.push(MuseIdea {
            title,
            description,
            emoji,
            category,
            action,
        });
    }
    out
}

/// One idea as a card: its category and emoji stored, its action as the
/// prompt, and two weeks to live.
pub fn muse_idea_card(idea: &MuseIdea, now_ms: u64) -> UpdateCard {
    let source = format!(
        "{IDEA_SOURCE}-{}",
        crate::cabin_engine::engine_slug(&idea.title)
    );
    let mut card = idea_card(&source, &idea.title, &idea.description, now_ms);
    card.prompt = Some(idea.action.clone());
    card.pulse.icon = Some(idea.emoji.clone());
    card.pulse.category = Some(idea.category.key().to_string());
    card.expires_at = Some(now_ms + IDEA_TTL_MS);
    card
}

/// Post a run's ideas. Returns how many landed.
pub fn post_ideas(cards: &mut Vec<UpdateCard>, ideas: &[MuseIdea], now_ms: u64) -> usize {
    ideas
        .iter()
        .filter(|idea| post_update(cards, muse_idea_card(idea, now_ms)))
        .count()
}

/// A card this job wrote.
pub fn is_muse_idea(card: &UpdateCard) -> bool {
    card.kind == UpdateKind::Idea && card.source_id.starts_with(IDEA_SOURCE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = r#"Here you go:
[
 {"title":"Cheaper groceries this week","description":"I can plan a week of meals around what's on sale near you.","emoji":"🛒","category":"shopping","action":"plan a week of meals from this week's grocery sales"},
 {"title":"Rust release notes","description":"I can sum up the Rust 1.99 release notes for your crates.","emoji":"🦀","category":"Productivity","action":"summarize the Rust 1.99 release notes for my crates"},
 {"title":"Budget check","description":"Let me look at your budget.","emoji":"💰","category":"financial","action":"review my budget"},
 {"title":"Trivia night","description":"I can set up a trivia night for your friends.","emoji":"🎉","category":"fun","action":"plan a trivia night"},
 {"title":"Cheaper groceries again","description":"I can plan meals around what's on sale near you.","emoji":"🛒","category":"shopping","action":"plan meals from grocery sales"},
 {"title":"Morning run plan","description":"I can build a 5k plan around your mornings.","emoji":"","category":"Health & Fitness","action":"build me a 5k plan for weekday mornings"}
]"#;

    #[test]
    fn the_parser_keeps_personal_categorized_i_can_ideas() {
        let got = parse_ideas(REPLY, &[], &[]);
        let rows: Vec<(&str, PulseCategory, &str)> = got
            .iter()
            .map(|i| (i.title.as_str(), i.category, i.emoji.as_str()))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("Cheaper groceries this week", PulseCategory::Shopping, "🛒"),
                ("Rust release notes", PulseCategory::Productivity, "🦀"),
                ("Morning run plan", PulseCategory::Health, DEFAULT_ICON),
            ],
            "no I can line, an unknown category and a repeat topic are dropped"
        );
        assert_eq!(
            got[0].action,
            "plan a week of meals from this week's grocery sales"
        );
        assert!(parse_ideas("no ideas today", &[], &[]).is_empty());
        assert!(parse_ideas("[]", &[], &[]).is_empty());
    }

    #[test]
    fn existing_and_turned_down_ideas_do_not_come_back() {
        let got = parse_ideas(
            REPLY,
            &["Rust release notes".to_string()],
            &["Cheaper groceries this week".to_string()],
        );
        let titles: Vec<&str> = got.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, vec!["Morning run plan"]);
    }

    #[test]
    fn categories_take_keys_or_labels_and_only_the_five() {
        assert_eq!(category_of("financial"), Some(PulseCategory::Financial));
        assert_eq!(
            category_of("Financial Management"),
            Some(PulseCategory::Financial)
        );
        assert_eq!(category_of("Health & Fitness"), Some(PulseCategory::Health));
        assert_eq!(category_of("more"), None);
        assert_eq!(category_of(""), None);
    }

    #[test]
    fn a_card_carries_its_category_emoji_action_and_two_weeks() {
        let idea = &parse_ideas(REPLY, &[], &[])[0];
        let card = muse_idea_card(idea, 1_000);
        assert_eq!(card.kind, UpdateKind::Idea);
        assert_eq!(card.title, "Cheaper groceries this week");
        assert_eq!(
            card.body.as_deref(),
            Some("I can plan a week of meals around what's on sale near you.")
        );
        assert_eq!(card.pulse.category.as_deref(), Some("shopping"));
        assert_eq!(card.pulse.icon.as_deref(), Some("🛒"));
        assert_eq!(
            card.idea_action(),
            "plan a week of meals from this week's grocery sales"
        );
        assert_eq!(card.expires_at, Some(1_000 + 14 * 24 * 60 * 60 * 1000));
        assert!(is_muse_idea(&card));
        let mut cards = Vec::new();
        assert_eq!(
            post_ideas(&mut cards, &parse_ideas(REPLY, &[], &[]), 1_000),
            3
        );
    }

    #[test]
    fn more_like_this_is_kept_for_the_next_run_and_not_interested_is_turned_down() {
        let mut s = IdeasState::default();
        assert!(s.due(5));
        s.last_run_ms = 1_000;
        assert!(!s.due(1_000 + EVERY_MS - 1));
        assert!(s.due(1_000 + EVERY_MS));
        s.note_more_like("Morning run plan");
        s.note_more_like("Trivia night");
        s.note_more_like("Morning run plan");
        assert_eq!(s.more_like, vec!["Morning run plan", "Trivia night"]);
        s.note_not_interested("Trivia night");
        assert_eq!(s.more_like, vec!["Morning run plan"]);
        assert_eq!(s.turned_down, vec!["Trivia night"]);
        let msgs = idea_messages(&IdeaInput {
            chats: &["I'm training for a 5k".to_string()],
            more_like: &s.more_like,
            turned_down: &s.turned_down,
            ..Default::default()
        })
        .unwrap();
        assert!(msgs[0]
            .1
            .starts_with("You suggest ideas for one person's Pulse"));
        assert_eq!(
            msgs[1].1,
            "What they said in chat lately:\n- I'm training for a 5k\n\nMore like this:\n- Morning run plan\n\nTurned down:\n- Trivia night\n"
        );
    }

    #[test]
    fn nothing_personal_means_no_call() {
        assert_eq!(
            idea_messages(&IdeaInput {
                existing: &["Old idea".to_string()],
                more_like: &["Old idea".to_string()],
                ..Default::default()
            }),
            None
        );
    }
}
