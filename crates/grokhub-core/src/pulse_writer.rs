//! Pulse Feed writer: each Feed post is one model call that turns this run's
//! free-source research into a short briefing with inline cites. Rules pick
//! the research, check every cite and decide when to run; the model only
//! writes. A short taste summary, rebuilt from what the person loved,
//! discussed and said in chat, steers the writer instead of vote counts.

use serde::{Deserialize, Serialize};

use crate::pulse::{feed_instructions_or_default, source_host, FEED_INSTRUCTIONS_CAP};
use crate::pulse_sources::{canonical_url, check_due, title_similarity, Ranked, SAME_STORY_PCT};
use crate::update_feed::{digest_card, UpdateCard};

/// Source id prefix on every written post.
pub const WRITER_SOURCE: &str = "pulse:writer";
/// Writer state in the config folder: clock, queue, prompt, taste events.
pub const STATE_FILE: &str = "pulse-writer.json";
/// The taste summary the writer reads, rebuilt when new taste events land.
pub const TASTE_FILE: &str = "pulse-taste.md";
/// Titles of the newest posts the writer must not repeat.
pub const RECENT_TITLES: usize = 20;
/// Research items one post may draw on.
pub const RESEARCH_MAX: usize = 5;
pub const SNIPPET_CHARS: usize = 600;
pub const WHY_CHARS: usize = 240;
pub const TASTE_CHARS: usize = 1200;
/// Taste events kept for the next rebuild, newest last.
pub const TASTE_EVENTS_MAX: usize = 30;
/// Characters kept from one taste event's text.
pub const TASTE_EVENT_CHARS: usize = 200;
pub const DEFAULT_ICON: &str = "📰";

/// Minutes between posts when nothing changes.
pub const CADENCE_BASE_MINS: u32 = 120;
/// When the person opened Pulse, loved or discussed a post in the last day.
pub const CADENCE_ENGAGED_MINS: u32 = 90;
/// After three days without any of that.
pub const CADENCE_IDLE_MINS: u32 = 240;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const IDLE_DAYS: u64 = 3;

/// The quiet window the shell keeps until it has its own setting (the cabin's default).
pub const QUIET_START: &str = "22:00";
pub const QUIET_END: &str = "07:00";

// cadence

/// The system cadence. Opening Pulse, Love and Discuss count as engagement;
/// a person who never engaged gets the base clock.
pub fn cadence_mins(last_engaged_ms: u64, now_ms: u64) -> u32 {
    if last_engaged_ms == 0 {
        return CADENCE_BASE_MINS;
    }
    let since = now_ms.saturating_sub(last_engaged_ms);
    if since < DAY_MS {
        CADENCE_ENGAGED_MINS
    } else if since >= IDLE_DAYS * DAY_MS {
        CADENCE_IDLE_MINS
    } else {
        CADENCE_BASE_MINS
    }
}

/// Should the writer run now? A queued post (the prompt was just saved) runs
/// at once; otherwise never in quiet hours, and on the cadence clock with the
/// open debounce.
pub fn writer_due(state: &WriterState, now_ms: u64, just_opened: bool, quiet: bool) -> bool {
    if state.queued {
        return true;
    }
    if quiet {
        return false;
    }
    let every = u64::from(cadence_mins(state.last_engaged_ms, now_ms)) * 60 * 1000;
    check_due(state.last_run_ms, now_ms, every, just_opened)
}

/// Minutes east of UTC, from the local clock a page reported, in 15-minute steps.
pub fn utc_offset_min(hour: u32, minute: u32, now_ms: u64) -> i32 {
    let local = i64::from(hour.min(23)) * 60 + i64::from(minute.min(59));
    let utc = ((now_ms / 60_000) % 1440) as i64;
    let mut diff = local - utc;
    if diff > 840 {
        diff -= 1440;
    } else if diff < -720 {
        diff += 1440;
    }
    let step = (diff as f64 / 15.0).round() as i64 * 15;
    step as i32
}

/// Local "HH:MM" at `now_ms` for a stored offset.
pub fn local_hm(now_ms: u64, offset_min: i32) -> String {
    let utc = ((now_ms / 60_000) % 1440) as i64;
    let local = (utc + i64::from(offset_min)).rem_euclid(1440);
    format!("{:02}:{:02}", local / 60, local % 60)
}

// state

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TasteKind {
    Love,
    Discuss,
    /// Something the person said in the main chat.
    Chat,
    /// Opened or scrolled past a post. Never a taste signal.
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TasteEvent {
    pub kind: TasteKind,
    pub text: String,
    pub at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriterState {
    #[serde(default)]
    pub last_run_ms: u64,
    /// One post owed right away (the feed prompt was saved).
    #[serde(default)]
    pub queued: bool,
    /// Last time the person opened Pulse, loved or discussed a post.
    #[serde(default)]
    pub last_engaged_ms: u64,
    /// The feed prompt. Empty means the default.
    #[serde(default)]
    pub prompt: String,
    /// Minutes east of UTC, from the last page that reported its clock.
    #[serde(default)]
    pub utc_offset_min: Option<i32>,
    #[serde(default)]
    pub taste_events: Vec<TasteEvent>,
    /// When the newest taste event landed.
    #[serde(default)]
    pub taste_event_ms: u64,
    /// When the taste summary was last rebuilt.
    #[serde(default)]
    pub taste_built_ms: u64,
    /// What the last run did, for the page.
    #[serde(default)]
    pub note: String,
}

impl WriterState {
    pub fn prompt_or_default(&self) -> &str {
        feed_instructions_or_default(&self.prompt)
    }

    /// Save the feed prompt and owe exactly one post for it.
    pub fn save_prompt(&mut self, text: &str) {
        self.prompt = text.trim().chars().take(FEED_INSTRUCTIONS_CAP).collect();
        self.queued = true;
    }

    /// The owed post, once.
    pub fn take_queued(&mut self) -> bool {
        std::mem::replace(&mut self.queued, false)
    }

    pub fn note_engaged(&mut self, now_ms: u64) {
        self.last_engaged_ms = self.last_engaged_ms.max(now_ms);
    }

    /// Record a taste event. Reads are not taste and change nothing. Returns
    /// whether the summary is now due a rebuild.
    pub fn note_taste(&mut self, kind: TasteKind, text: &str, now_ms: u64) -> bool {
        let text: String = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(TASTE_EVENT_CHARS)
            .collect();
        if kind == TasteKind::Read || text.is_empty() || !crate::is_plain_text(&text) {
            return false;
        }
        self.taste_events.push(TasteEvent {
            kind,
            text,
            at_ms: now_ms,
        });
        let over = self.taste_events.len().saturating_sub(TASTE_EVENTS_MAX);
        self.taste_events.drain(..over);
        self.taste_event_ms = self.taste_event_ms.max(now_ms);
        true
    }

    pub fn taste_stale(&self) -> bool {
        self.taste_event_ms > self.taste_built_ms
    }
}

pub fn load_state(path: &std::path::Path) -> WriterState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Through a temp file and a rename, so a crash never leaves half a file.
pub fn save_state(path: &std::path::Path, state: &WriterState) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    write_atomic(path, &raw)
}

pub fn write_atomic(path: &std::path::Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

// taste summary

const TASTE_RULES: &str = "You keep a short note on what one person likes to read in their news feed. Rewrite the note from their current note and what they did below. Say what they care about and what they skip, in plain sentences. Keep it under 1000 characters. Never quote secrets, numbers or names that are not topics. Reply with the note only.";

/// The rebuild call, or `None` when there is nothing to learn from.
pub fn taste_messages(current: &str, events: &[TasteEvent]) -> Option<Vec<(String, String)>> {
    let mut lines = String::new();
    for e in events {
        let label = match e.kind {
            TasteKind::Love => "Loved a post",
            TasteKind::Discuss => "Asked to discuss",
            TasteKind::Chat => "Said in chat",
            TasteKind::Read => continue,
        };
        lines.push_str(&format!("- {label}: {}\n", e.text));
    }
    if lines.is_empty() {
        return None;
    }
    let current = current.trim();
    let current = if current.is_empty() {
        "(none yet)"
    } else {
        current
    };
    Some(vec![
        ("system".into(), TASTE_RULES.into()),
        (
            "user".into(),
            format!("Current note:\n{current}\n\nWhat they did:\n{lines}"),
        ),
    ])
}

/// The model's note, cleaned. `None` keeps the current one.
pub fn parse_taste(reply: &str) -> Option<String> {
    let text = strip_fence(reply);
    if text.chars().count() < 10 {
        return None;
    }
    Some(text.chars().take(TASTE_CHARS).collect())
}

fn strip_fence(reply: &str) -> &str {
    let mut text = reply.trim();
    if let Some(inner) = text.strip_prefix("```") {
        let inner = inner.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
        text = inner.trim_end().strip_suffix("```").unwrap_or(inner).trim();
    }
    text
}

// writer

/// One thing fetched this run, offered to the writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Research {
    pub title: String,
    pub url: String,
    pub source: String,
    pub text: String,
}

pub fn research_from(picks: &[Ranked]) -> Vec<Research> {
    picks
        .iter()
        .take(RESEARCH_MAX)
        .map(|r| Research {
            title: r.item.title.clone(),
            url: r.item.url.clone(),
            source: r.item.source_name.clone(),
            text: r.item.summary.clone(),
        })
        .collect()
}

pub struct WriterInput<'a> {
    pub prompt: &'a str,
    pub taste: &'a str,
    /// The interest and memory topics block.
    pub topics: &'a str,
    pub recent_titles: &'a [String],
    pub research: &'a [Research],
}

const WRITER_RULES: &str = "You write one post for a person's news feed, in the style of a short briefing. Pick the single item below that fits them best, or combine items that are the same story. \
Reply with JSON only: {\"title\":\"...\",\"snippet\":\"...\",\"icon\":\"...\",\"why\":\"...\"}. \
title: under 70 characters, plain, no clickbait. \
snippet: one to three short sentences of markdown that say what happened and why it matters, citing sources inline as [site](url). Use only URLs from the list below, exactly as written; never invent a link, fact, name or number. \
icon: one emoji for the topic. \
why: one or two sentences to the reader, starting with \"You\", on why this is in their feed. \
Never repeat a story from the recent titles. If nothing below is worth a post, reply {\"skip\":true}.";

pub fn writer_messages(input: &WriterInput) -> Vec<(String, String)> {
    let mut user = format!(
        "Feed prompt from the person (follow it):\n{}\n",
        input.prompt.trim()
    );
    let taste = input.taste.trim();
    if !taste.is_empty() {
        user.push_str(&format!("\nWhat they like (taste summary):\n{taste}\n"));
    }
    let topics = input.topics.trim();
    if !topics.is_empty() {
        user.push_str(&format!("\n{topics}\n"));
    }
    if !input.recent_titles.is_empty() {
        user.push_str("\nRecent titles (do not repeat):\n");
        for t in input.recent_titles {
            user.push_str(&format!("- {t}\n"));
        }
    }
    user.push_str("\nFetched this run:\n");
    for (n, r) in input.research.iter().enumerate() {
        user.push_str(&format!(
            "{}. {} ({})\n   {}\n   {}\n",
            n + 1,
            r.title.trim(),
            r.source.trim(),
            r.url.trim(),
            r.text.trim()
        ));
    }
    vec![
        ("system".into(), WRITER_RULES.into()),
        ("user".into(), user),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub title: String,
    /// Markdown with only checked links left in.
    pub snippet: String,
    pub icon: String,
    pub why: String,
    /// The fetched URLs the snippet cites, in order.
    pub cites: Vec<String>,
}

pub const SKIPPED: &str = "Nothing new was worth a post.";
pub const NO_SOURCE: &str = "The post cited no page fetched this run, so it was dropped.";
pub const REPEAT: &str = "The post repeated a recent one, so it was dropped.";
pub const UNREADABLE: &str = "The writer's reply was not a post.";

/// Parse the writer's reply. Links to pages not fetched this run are cut down
/// to their text; a post left with no real cite, or that repeats a recent
/// title, is dropped.
pub fn parse_written(
    reply: &str,
    research: &[Research],
    recent_titles: &[String],
) -> Result<Written, String> {
    let text = strip_fence(reply);
    let json = match (text.find('{'), text.rfind('}')) {
        (Some(a), Some(b)) if a < b => &text[a..=b],
        _ => return Err(UNREADABLE.into()),
    };
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| UNREADABLE.to_string())?;
    if v.get("skip").and_then(|s| s.as_bool()) == Some(true) {
        return Err(SKIPPED.into());
    }
    let field = |k: &str| {
        v.get(k)
            .and_then(|s| s.as_str())
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    };
    let title = clip(&field("title"), 72);
    let snippet = field("snippet");
    if title.is_empty() || snippet.is_empty() {
        return Err(UNREADABLE.into());
    }
    if is_repeat(&title, recent_titles) {
        return Err(REPEAT.into());
    }
    let allowed: Vec<String> = research.iter().map(|r| canonical_url(&r.url)).collect();
    let (snippet, cites) = check_cites(&snippet, &allowed);
    if cites.is_empty() {
        return Err(NO_SOURCE.into());
    }
    Ok(Written {
        title,
        snippet: clip(&snippet, SNIPPET_CHARS),
        icon: icon_or_default(&field("icon")),
        why: clip(&field("why"), WHY_CHARS),
        cites,
    })
}

/// Keep `[text](url)` links whose URL was fetched; cut the rest to their text.
pub fn check_cites(markdown: &str, allowed: &[String]) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut cites: Vec<String> = Vec::new();
    let mut rest = markdown;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            let url_part = &after[close + 2..];
            url_part
                .find(')')
                .map(|end| (&after[..close], &url_part[..end], close + 2 + end + 1))
        });
        let Some((label, url, used)) = link else {
            out.push_str(&rest[..open + 1]);
            rest = after;
            continue;
        };
        out.push_str(&rest[..open]);
        let url = url.trim();
        let ok = (url.starts_with("https://") || url.starts_with("http://"))
            && allowed.iter().any(|a| *a == canonical_url(url));
        if ok {
            out.push_str(&format!("[{label}]({url})"));
            if !cites.iter().any(|c| c == url) {
                cites.push(url.to_string());
            }
        } else {
            out.push_str(label);
        }
        rest = &after[used..];
    }
    out.push_str(rest);
    (out, cites)
}

/// Same title, or the same story in other words.
pub fn is_repeat(title: &str, recent_titles: &[String]) -> bool {
    recent_titles.iter().any(|r| {
        r.trim().eq_ignore_ascii_case(title.trim()) || title_similarity(title, r) >= SAME_STORY_PCT
    })
}

/// The newest written posts' titles, newest first.
pub fn recent_titles(cards: &[UpdateCard]) -> Vec<String> {
    let mut mine: Vec<&UpdateCard> = cards
        .iter()
        .filter(|c| c.source_id.starts_with(WRITER_SOURCE))
        .collect();
    mine.sort_by_key(|c| std::cmp::Reverse(c.created_at));
    mine.into_iter()
        .take(RECENT_TITLES)
        .map(|c| c.title.clone())
        .collect()
}

fn icon_or_default(icon: &str) -> String {
    let icon = icon.trim();
    let n = icon.chars().count();
    if n == 0 || n > 4 || icon.chars().any(|c| c.is_ascii()) {
        DEFAULT_ICON.to_string()
    } else {
        icon.to_string()
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    if let Some(cut) = out.rfind(' ') {
        out.truncate(cut);
    }
    out.push('…');
    out
}

/// The Feed card for a written post. Images are added after the gallery step.
pub fn post_card(written: &Written, research: &[Research], now_ms: u64) -> UpdateCard {
    let mut card = digest_card(
        &format!("{WRITER_SOURCE}:{now_ms}"),
        &written.title,
        &written.snippet,
        now_ms,
    );
    card.citations = written.cites.clone();
    let first = written.cites.first().map(String::as_str).unwrap_or("");
    card.pulse.source_name = research
        .iter()
        .find(|r| canonical_url(&r.url) == canonical_url(first))
        .map(|r| r.source.clone())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| source_host(first));
    card.pulse.icon = Some(written.icon.clone());
    card.why = (!written.why.is_empty()).then(|| written.why.clone());
    card
}

/// What Discuss puts in the chat box: the post itself, so the chat starts from it.
pub fn discuss_seed(card: &UpdateCard) -> String {
    let mut seed = format!(
        "Let's talk about this post from my Pulse feed.\n\n**{}**\n",
        card.title.trim()
    );
    if let Some(body) = card.body.as_deref().filter(|b| !b.trim().is_empty()) {
        seed.push_str(body.trim());
        seed.push('\n');
    }
    if let Some(link) = card.citations.first() {
        seed.push_str(&format!("\nSource: {link}\n"));
    }
    seed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pulse_sources::{SourceItem, SourceKind};

    const NOW: u64 = 1_800_000_000_000;
    const MIN: u64 = 60_000;

    fn research() -> Vec<Research> {
        vec![
            Research {
                title: "Rust 1.99 ships async closures".into(),
                url: "https://blog.rust-lang.org/2026/10/20/Rust-1.99.html".into(),
                source: "Rust Blog".into(),
                text: "Async closures are stable.".into(),
            },
            Research {
                title: "Tauri 2.5 released".into(),
                url: "https://tauri.app/blog/tauri-2-5/".into(),
                source: "tauri.app".into(),
                text: "Faster builds on Windows.".into(),
            },
        ]
    }

    #[test]
    fn a_writer_reply_parses_into_a_post_with_checked_cites() {
        let reply = r#"```json
{"title":"Rust 1.99 makes async closures stable","snippet":"Async closures are now stable in [Rust 1.99](https://blog.rust-lang.org/2026/10/20/Rust-1.99.html), and [Tauri](https://tauri.app/blog/tauri-2-5/) builds got faster.","icon":"🦀","why":"You build a Rust app, so this changes code you write."}
```"#;
        let post = parse_written(reply, &research(), &[]).expect("post");
        assert_eq!(post.title, "Rust 1.99 makes async closures stable");
        assert_eq!(
            post.snippet,
            "Async closures are now stable in [Rust 1.99](https://blog.rust-lang.org/2026/10/20/Rust-1.99.html), and [Tauri](https://tauri.app/blog/tauri-2-5/) builds got faster."
        );
        assert_eq!(post.icon, "🦀");
        assert_eq!(
            post.why,
            "You build a Rust app, so this changes code you write."
        );
        assert_eq!(
            post.cites,
            vec![
                "https://blog.rust-lang.org/2026/10/20/Rust-1.99.html".to_string(),
                "https://tauri.app/blog/tauri-2-5/".to_string()
            ]
        );
    }

    #[test]
    fn invented_cite_urls_are_cut_to_their_text() {
        let reply = r#"{"title":"Async closures land","snippet":"Stable now ([blog](https://blog.rust-lang.org/2026/10/20/Rust-1.99.html)), says [a fake site](https://made-up.example/news).","icon":"🦀","why":"You use Rust."}"#;
        let post = parse_written(reply, &research(), &[]).expect("post");
        assert_eq!(
            post.snippet,
            "Stable now ([blog](https://blog.rust-lang.org/2026/10/20/Rust-1.99.html)), says a fake site."
        );
        assert_eq!(
            post.cites,
            vec!["https://blog.rust-lang.org/2026/10/20/Rust-1.99.html".to_string()]
        );
    }

    #[test]
    fn a_post_with_only_invented_cites_is_dropped() {
        let reply = r#"{"title":"Big news","snippet":"Read [this](https://made-up.example/a).","icon":"📰","why":"You like news."}"#;
        assert_eq!(
            parse_written(reply, &research(), &[]),
            Err(NO_SOURCE.to_string())
        );
        let bare = r#"{"title":"Big news","snippet":"No links at all.","icon":"📰","why":"x"}"#;
        assert_eq!(
            parse_written(bare, &research(), &[]),
            Err(NO_SOURCE.to_string())
        );
    }

    #[test]
    fn skip_and_junk_replies_post_nothing() {
        assert_eq!(
            parse_written(r#"{"skip":true}"#, &research(), &[]),
            Err(SKIPPED.to_string())
        );
        assert_eq!(
            parse_written("Sorry, I can't.", &research(), &[]),
            Err(UNREADABLE.to_string())
        );
    }

    #[test]
    fn a_title_from_the_last_twenty_is_a_repeat() {
        let recent: Vec<String> = (0..19)
            .map(|n| format!("Unrelated story number {n}"))
            .chain(["Rust 1.99 makes async closures stable".to_string()])
            .collect();
        assert_eq!(recent.len(), RECENT_TITLES);
        let reply = r#"{"title":"rust 1.99 makes async closures stable","snippet":"See [blog](https://blog.rust-lang.org/2026/10/20/Rust-1.99.html).","icon":"🦀","why":"You use Rust."}"#;
        assert_eq!(
            parse_written(reply, &research(), &recent),
            Err(REPEAT.to_string())
        );
        // The same story in other words is a repeat too.
        assert!(is_repeat("Async closures stable in Rust 1.99", &recent));
        assert!(!is_repeat("Tauri 2.5 speeds up Windows builds", &recent));
    }

    #[test]
    fn recent_titles_are_the_twenty_newest_written_posts() {
        let mut cards: Vec<UpdateCard> = (0..25u64)
            .map(|n| {
                let w = Written {
                    title: format!("Post {n}"),
                    snippet: "x".into(),
                    icon: DEFAULT_ICON.into(),
                    why: String::new(),
                    cites: vec!["https://tauri.app/blog/tauri-2-5/".into()],
                };
                post_card(&w, &research(), NOW + n * MIN)
            })
            .collect();
        cards.push(digest_card(
            "source:hn:x",
            "Not mine",
            "body",
            NOW + 99 * MIN,
        ));
        let titles = recent_titles(&cards);
        assert_eq!(titles.len(), 20);
        assert_eq!(titles[0], "Post 24");
        assert_eq!(titles[19], "Post 5");
    }

    #[test]
    fn writer_messages_carry_prompt_taste_topics_titles_and_research() {
        let titles = vec!["Yesterday's post".to_string()];
        let r = research();
        let msgs = writer_messages(&WriterInput {
            prompt: "Mostly Rust news.",
            taste: "They love Rust release notes.",
            topics: "Interests: Rust, Tauri",
            recent_titles: &titles,
            research: &r,
        });
        assert_eq!(msgs[0].0, "system");
        assert!(msgs[0].1.contains("never invent a link"), "{}", msgs[0].1);
        let user = &msgs[1].1;
        assert!(user.contains("Feed prompt from the person (follow it):\nMostly Rust news.\n"));
        assert!(user.contains("What they like (taste summary):\nThey love Rust release notes.\n"));
        assert!(user.contains("Interests: Rust, Tauri"));
        assert!(user.contains("Recent titles (do not repeat):\n- Yesterday's post\n"));
        assert!(user.contains(
            "1. Rust 1.99 ships async closures (Rust Blog)\n   https://blog.rust-lang.org/2026/10/20/Rust-1.99.html\n"
        ));
        assert!(user.contains("2. Tauri 2.5 released (tauri.app)"));
    }

    #[test]
    fn cadence_is_90_120_or_240_minutes() {
        assert_eq!(cadence_mins(0, NOW), 120);
        assert_eq!(cadence_mins(NOW - 30 * MIN, NOW), 90);
        assert_eq!(cadence_mins(NOW - DAY_MS + MIN, NOW), 90);
        assert_eq!(cadence_mins(NOW - DAY_MS, NOW), 120);
        assert_eq!(cadence_mins(NOW - 3 * DAY_MS + MIN, NOW), 120);
        assert_eq!(cadence_mins(NOW - 3 * DAY_MS, NOW), 240);
    }

    #[test]
    fn the_writer_runs_on_the_cadence_and_never_in_quiet_hours() {
        let mut s = WriterState {
            last_run_ms: NOW - 100 * MIN,
            last_engaged_ms: NOW - 60 * MIN,
            ..Default::default()
        };
        // Engaged today: 90 minutes, so 100 minutes later it is due.
        assert!(writer_due(&s, NOW, false, false));
        assert!(!writer_due(&s, NOW, false, true), "quiet hours hold it");
        // Not engaged for two days: 120 minutes, not due at 100.
        s.last_engaged_ms = NOW - 2 * DAY_MS;
        assert!(!writer_due(&s, NOW, false, false));
        // Opening the app after the debounce runs it.
        assert!(writer_due(&s, NOW, true, false));
        // Idle for three days: 240 minutes.
        s.last_engaged_ms = NOW - 4 * DAY_MS;
        s.last_run_ms = NOW - 200 * MIN;
        assert!(!writer_due(&s, NOW, false, false));
        assert!(writer_due(&s, NOW + 40 * MIN, false, false));
    }

    #[test]
    fn saving_the_prompt_queues_exactly_one_post() {
        let mut s = WriterState {
            last_run_ms: NOW - MIN,
            ..Default::default()
        };
        assert!(!writer_due(&s, NOW, false, false));
        s.save_prompt("  Only Rust news.  ");
        s.save_prompt("Only Rust and Tauri news.");
        assert_eq!(s.prompt, "Only Rust and Tauri news.");
        // Queued runs even in quiet hours: the person just asked.
        assert!(writer_due(&s, NOW, false, true));
        assert!(s.take_queued());
        assert!(!s.take_queued());
        assert!(!writer_due(&s, NOW, false, false));
    }

    #[test]
    fn the_taste_summary_ignores_reads() {
        let mut s = WriterState::default();
        assert!(!s.note_taste(TasteKind::Read, "Opened a post", NOW));
        assert!(!s.taste_stale());
        assert!(s.taste_events.is_empty());
        assert_eq!(taste_messages("", &s.taste_events), None);

        assert!(s.note_taste(TasteKind::Love, "Rust 1.99 ships", NOW));
        assert!(s.note_taste(TasteKind::Discuss, "Tauri 2.5 released", NOW + 1));
        assert!(s.note_taste(TasteKind::Chat, "help me  port\nthe shell", NOW + 2));
        assert!(!s.note_taste(TasteKind::Chat, "key sk-abcdefghijklmnopqrstuv", NOW + 3));
        assert!(s.taste_stale());
        // A read event slipped into the list is still left out.
        let mut events = s.taste_events.clone();
        events.push(TasteEvent {
            kind: TasteKind::Read,
            text: "Scrolled past".into(),
            at_ms: NOW + 4,
        });
        let msgs = taste_messages("They like Rust.", &events).expect("call");
        assert_eq!(
            msgs[1].1,
            "Current note:\nThey like Rust.\n\nWhat they did:\n- Loved a post: Rust 1.99 ships\n- Asked to discuss: Tauri 2.5 released\n- Said in chat: help me port the shell\n"
        );
        assert_eq!(
            parse_taste("```\nThey like Rust and Tauri.\n```").as_deref(),
            Some("They like Rust and Tauri.")
        );
        assert_eq!(parse_taste("ok"), None);
    }

    #[test]
    fn taste_events_keep_the_newest_thirty() {
        let mut s = WriterState::default();
        for n in 0..35u64 {
            s.note_taste(TasteKind::Love, &format!("post {n}"), NOW + n);
        }
        assert_eq!(s.taste_events.len(), TASTE_EVENTS_MAX);
        assert_eq!(s.taste_events[0].text, "post 5");
        assert_eq!(s.taste_event_ms, NOW + 34);
    }

    #[test]
    fn a_written_post_becomes_a_feed_card_with_icon_why_and_source() {
        let w = Written {
            title: "Tauri 2.5 speeds up Windows builds".into(),
            snippet: "Faster [builds](https://tauri.app/blog/tauri-2-5/).".into(),
            icon: "⚡".into(),
            why: "You ship a Tauri app.".into(),
            cites: vec!["https://tauri.app/blog/tauri-2-5/".into()],
        };
        let card = post_card(&w, &research(), NOW);
        assert_eq!(card.source_id, format!("pulse:writer:{NOW}"));
        assert_eq!(card.title, "Tauri 2.5 speeds up Windows builds");
        assert_eq!(
            card.body.as_deref(),
            Some("Faster [builds](https://tauri.app/blog/tauri-2-5/).")
        );
        assert_eq!(
            card.citations,
            vec!["https://tauri.app/blog/tauri-2-5/".to_string()]
        );
        assert_eq!(card.pulse.source_name.as_deref(), Some("tauri.app"));
        assert_eq!(card.pulse.icon.as_deref(), Some("⚡"));
        assert_eq!(card.why.as_deref(), Some("You ship a Tauri app."));
        assert!(crate::pulse::is_feed_card(&card));
        assert_eq!(
            discuss_seed(&card),
            "Let's talk about this post from my Pulse feed.\n\n**Tauri 2.5 speeds up Windows builds**\nFaster [builds](https://tauri.app/blog/tauri-2-5/).\n\nSource: https://tauri.app/blog/tauri-2-5/\n"
        );
    }

    #[test]
    fn a_bad_icon_falls_back_and_research_takes_at_most_five() {
        assert_eq!(icon_or_default("rust"), DEFAULT_ICON);
        assert_eq!(icon_or_default(""), DEFAULT_ICON);
        assert_eq!(icon_or_default("🦀"), "🦀");
        let picks: Vec<Ranked> = (0..7)
            .map(|n| Ranked {
                item: SourceItem {
                    title: format!("Item {n}"),
                    url: format!("https://news.example.com/{n}"),
                    kind: SourceKind::HackerNews,
                    source_name: "Hacker News".into(),
                    published_ms: NOW,
                    summary: "text".into(),
                    image: None,
                    topic: None,
                    engagement: 0,
                },
                score: 50,
                because: None,
            })
            .collect();
        let r = research_from(&picks);
        assert_eq!(r.len(), RESEARCH_MAX);
        assert_eq!(r[4].url, "https://news.example.com/4");
    }

    #[test]
    fn the_local_clock_comes_from_the_page() {
        // 12:00 UTC on the test day.
        let noon = NOW - NOW % DAY_MS + 12 * 60 * MIN;
        assert_eq!(utc_offset_min(14, 0, noon), 120);
        assert_eq!(utc_offset_min(7, 0, noon), -300);
        assert_eq!(utc_offset_min(17, 31, noon), 330);
        assert_eq!(local_hm(noon, -300), "07:00");
        assert_eq!(local_hm(noon + 11 * 60 * MIN, 120), "01:00");
        assert!(crate::quiet_hours_active(
            &local_hm(noon + 11 * 60 * MIN, 120),
            QUIET_START,
            QUIET_END
        ));
    }

    #[test]
    fn writer_state_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("grokhub-pulse-writer-{}", std::process::id()));
        let path = dir.join(STATE_FILE);
        let mut s = WriterState::default();
        s.save_prompt("Rust news");
        s.note_engaged(NOW);
        save_state(&path, &s).expect("save");
        assert_eq!(load_state(&path), s);
        assert_eq!(
            load_state(&dir.join("missing.json")),
            WriterState::default()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
