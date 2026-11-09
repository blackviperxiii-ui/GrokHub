//! Pulse: one page for what the cabin would do next (For You) and what it noticed
//! (Feed). Rules pick and order the cards; a model only ever writes sentences.
//! Same inputs give the same order. Feedback lands in MEMORY.md as one
//! structured ledger line per choice, and the ledger feeds the next ranking and
//! the feed instructions rewrite.

use serde::{Deserialize, Serialize};

use crate::ideas::{same_topic_words, topic_words};
use crate::update_feed::{
    post_update, public_http_url, CardReaction, FeedPulse, UpdateCard, UpdateKind, UpdateStatus,
};

/// Pulse fields on a feed card. Every one defaults, so older `updates.json`
/// files load unchanged and older builds ignore them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseMeta {
    /// Category key (`financial`, `productivity`, …). Set once by the migration
    /// or when the card is posted, so a card never jumps between headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Snoozed: hidden until this unix ms.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub snoozed_until: u64,
    /// A real due time the source gave (a bill, a renewal). Never guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<u64>,
    /// Images from the source itself (its og:image or thumbnail). Never made up.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_urls: Vec<String>,
    /// Where a Feed post came from, shown above its headline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    /// The interest a source post matched, painted as "Why: …" under its summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub because: Option<String>,
    /// Released from quiet hours inside one digest card: no ping of its own.
    #[serde(default, skip_serializing_if = "is_false")]
    pub quiet_batched: bool,
    /// Closed on the main window's deck. Pulse still shows it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub off_home: bool,
    /// A written post's topic emoji.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// A Spike-6a proactive offer: its help score, route and topic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proactive: Option<crate::proactive::ProactiveMeta>,
    /// Command Centre nudge watcher that posted this card (card 42).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nudge_watcher: Option<crate::nudge_watchers::NudgeWatcherMeta>,
    /// LAN 3D printer alert (Command Centre card 60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print_alert: Option<crate::print_alerts::PrintAlertMeta>,
    /// Bite-sized hobby learning-path tip (Know me opt-in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hobby_path: Option<crate::hobby_paths::HobbyPathMeta>,
    /// One line on what the card's numbers mean, painted above its detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<PulseVerdict>,
    /// What the card found: headline, summary and the full report it opens to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<crate::card_report::CardReport>,
    /// Generic `kind:value` tags (topic, subject, tone…) that taste learning
    /// reads (`card_taste`). Empty on cards nobody tagged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traits: Vec<String>,
}

/// How a verdict reads: normal, worth watching, or worth a look.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerdictTone {
    #[default]
    Fine,
    Watch,
    Act,
}

/// A card's one-line interpretation, from rules over the user's own history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseVerdict {
    pub tone: VerdictTone,
    pub line: String,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn is_false(v: &bool) -> bool {
    !*v
}

pub const PULSE_TITLE: &str = "Pulse";
pub static PULSE_SUBTITLE: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    crate::brand::filled("What I'd do next, from how you use {product}.")
});
/// Pulse with no ideas, no posts, and nothing running.
pub const PULSE_EMPTY: &str =
    "Nothing on Pulse yet. I'll add ideas and posts when I spot something worth doing.";
/// Pulse while a suggestion call is running: shown with placeholder rows.
pub const IDEAS_LOADING: &str = "Looking for ideas in your recent work…";
/// Suggest for you pressed with no Grok sign-in or key.
pub const IDEAS_SIGN_IN: &str = "Sign in to Grok to get ideas.";
/// The longest bold line on a For You row.
pub const I_CAN_MAX: usize = 70;

/// "Last pulse 2m ago. Nothing to do." for the silence line.
pub fn silence_line(last_pulse_ms: u64, now_ms: u64) -> String {
    if last_pulse_ms == 0 {
        return "No pulse yet. Nothing to do.".into();
    }
    format!(
        "Last pulse {}. Nothing to do.",
        ago_label(last_pulse_ms, now_ms)
    )
}

/// "just now", "2m ago", "3h ago", "2d ago".
pub fn ago_label(at_ms: u64, now_ms: u64) -> String {
    let s = now_ms.saturating_sub(at_ms) / 1000;
    if s < 60 {
        "just now".into()
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else if s < 86_400 {
        format!("{}h ago", s / 3600)
    } else {
        format!("{}d ago", s / 86_400)
    }
}

// categories

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PulseCategory {
    Games,
    Pc,
    Mods,
    Deals,
    Automations,
    More,
}

impl PulseCategory {
    /// Header order on the Ideas view: a player's own things first. More
    /// holds cards whose words match no category.
    pub const ORDER: [PulseCategory; 6] = [
        Self::Games,
        Self::Pc,
        Self::Mods,
        Self::Deals,
        Self::Automations,
        Self::More,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Games => "Games",
            Self::Pc => "PC",
            Self::Mods => "Mods & builds",
            Self::Deals => "Deals",
            Self::Automations => "Automations",
            Self::More => "More for you",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Games => "games",
            Self::Pc => "pc",
            Self::Mods => "mods",
            Self::Deals => "deals",
            Self::Automations => "automations",
            Self::More => "more",
        }
    }

    /// A stored key. Keys from the older consumer categories map over:
    /// shopping is Deals, productivity is Automations, the rest are More.
    pub fn parse(key: &str) -> Option<Self> {
        let key = key.trim();
        Self::ORDER
            .into_iter()
            .find(|c| c.key() == key)
            .or(match key {
                "shopping" => Some(Self::Deals),
                "productivity" => Some(Self::Automations),
                "financial" | "relationships" | "health" => Some(Self::More),
                _ => None,
            })
    }
}

const MODS: &[&str] = &[
    "mod",
    "mods",
    "modded",
    "modding",
    "modlist",
    "mod list",
    "load order",
    "nexus",
    "workshop",
    "shader",
    "shaders",
    "reshade",
    "texture pack",
    "loadout",
    "loadouts",
    "build guide",
    "builds",
];
const PC: &[&str] = &[
    "pc",
    "gpu",
    "cpu",
    "driver",
    "drivers",
    "fps",
    "temps",
    "temperature",
    "fan",
    "fans",
    "overclock",
    "benchmark",
    "ssd",
    "disk",
    "drive space",
    "ram",
    "memory leak",
    "crash",
    "crashes",
    "stutter",
    "windows update",
    "startup",
    "backup",
    "backups",
    "health check",
];
const GAMES: &[&str] = &[
    "game",
    "games",
    "gaming",
    "steam",
    "patch",
    "patch notes",
    "season",
    "raid",
    "ranked",
    "match",
    "matches",
    "achievement",
    "achievements",
    "playtime",
    "speedrun",
    "boss",
    "quest",
    "dlc",
    "server",
    "servers",
    "squad",
    "co-op",
    "spoiler-free",
];
const DEALS: &[&str] = &[
    "sale",
    "sales",
    "deal",
    "deals",
    "discount",
    "price",
    "prices",
    "price drop",
    "cheaper",
    "wishlist",
    "bundle",
    "free weekend",
    "coupon",
    "buy",
    "shopping",
    "compare",
    "cart",
    "order",
];
const AUTOMATIONS: &[&str] = &[
    "every",
    "daily",
    "weekly",
    "weekday",
    "weekdays",
    "morning",
    "nightly",
    "schedule",
    "remind",
    "reminder",
    "automate",
    "automation",
    "watch",
    "notify",
    "track",
    "summarize",
    "summary",
    "brief",
    "tidy",
    "organize",
    "workboard",
    "inbox",
    "email",
    "calendar",
    "folder",
    "files",
];

/// Category from the card's words. Fixed keyword table, first match in this
/// order: Mods, PC, Deals, Games, Automations, else More. The narrower lists
/// go first so a mod for a game is a mod, a GPU driver for a game is PC and a
/// Steam sale is a deal.
pub fn categorize(text: &str) -> PulseCategory {
    let lower = text.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .collect();
    let hit = |list: &[&str]| {
        list.iter().any(|k| {
            let k = k.trim();
            if k.contains(' ') {
                lower.contains(k)
            } else {
                words.contains(&k)
            }
        })
    };
    [
        (MODS, PulseCategory::Mods),
        (PC, PulseCategory::Pc),
        (DEALS, PulseCategory::Deals),
        (GAMES, PulseCategory::Games),
        (AUTOMATIONS, PulseCategory::Automations),
    ]
    .into_iter()
    .find(|(list, _)| hit(list))
    .map_or(PulseCategory::More, |(_, cat)| cat)
}

/// The stored category, or the keyword pick for a card that has none yet.
pub fn card_category(card: &UpdateCard) -> PulseCategory {
    card.pulse
        .category
        .as_deref()
        .and_then(PulseCategory::parse)
        .unwrap_or_else(|| categorize(&card_text(card)))
}

fn card_text(card: &UpdateCard) -> String {
    let mut text = card.title.clone();
    for part in [card.body.as_deref(), card.prompt.as_deref()]
        .into_iter()
        .flatten()
    {
        text.push(' ');
        text.push_str(part);
    }
    text
}

// card types

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseType {
    /// One action, launched as `/bg`. Never takes the composer.
    Do,
    /// Something changed.
    Watch,
    /// A hypothesis about you: Accept or Wrong.
    Learn,
    /// "You did this 3 times, make it a loop."
    Automate,
    /// Below the threshold: no card, only the last pulse time.
    Quiet,
}

impl PulseType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Do => "Do",
            Self::Watch => "Watch",
            Self::Learn => "Learn",
            Self::Automate => "Automate",
            Self::Quiet => "Quiet",
        }
    }

    /// Opened-card section heading for this type (PI-03). One source with the row.
    pub fn apply_heading(self) -> &'static str {
        match self {
            Self::Learn => "What I'll learn",
            Self::Automate => "What Apply will schedule",
            // Do / Watch / Quiet: one-shot Apply.
            _ => "What Apply will do",
        }
    }
}

/// The type a card is before scoring. Quiet only comes from the score.
pub fn pulse_type(card: &UpdateCard) -> PulseType {
    match card.kind {
        UpdateKind::AutomationDone
        | UpdateKind::ScheduleCreated
        | UpdateKind::Digest
        | UpdateKind::SelfChange
        | UpdateKind::DoneForYou => PulseType::Watch,
        UpdateKind::AutomateOffer => PulseType::Automate,
        // The quiet-hours digest only reports what happened.
        UpdateKind::Suggestion if card.source_id == QUIET_DIGEST_SOURCE => PulseType::Watch,
        UpdateKind::Suggestion => PulseType::Do,
        // Same source as the opened card and Apply: a reminder is scheduled, so
        // it reads Automate on the row too.
        UpdateKind::Idea => match card.idea_type_label() {
            "Skill" => PulseType::Learn,
            "Automation" => PulseType::Automate,
            _ => PulseType::Do,
        },
    }
}

/// Ideas view: things to do. Feed view: things that happened or were found.
/// The quiet-hours digest is news about what happened, so it reads on the Feed.
/// A learning-path tip reads there too: its Done / Skip / Too easy / Too hard
/// controls are painted on Feed posts.
pub fn is_idea_card(card: &UpdateCard) -> bool {
    matches!(
        card.kind,
        UpdateKind::Idea | UpdateKind::Suggestion | UpdateKind::AutomateOffer
    ) && card.source_id != QUIET_DIGEST_SOURCE
        && card.pulse.hobby_path.is_none()
}

/// The bold line on an Ideas row, always in the cabin's own voice and at most
/// `I_CAN_MAX` characters. Two kinds keep their own title as written, bounded
/// by the card's title limit instead: a written idea, and a repeated-ask offer,
/// whose title is the user's question and must not be cut mid-sentence.
/// The model writes it as the idea's title ("I can …");
/// a card without one gets a plain line from its action: Do "I can {action}",
/// Automate "I can {action} {cadence}", Learn "I can learn how you {action}".
pub fn i_can_title(card: &UpdateCard) -> String {
    // A card that found something leads with what it found.
    if let Some(found) = card
        .pulse
        .report
        .as_ref()
        .map(|r| r.found.trim())
        .filter(|found| !found.is_empty())
    {
        return found.to_string();
    }
    let title = card.title.trim();
    // A written idea has its own short title; its "I can" line is the body.
    if crate::pulse_ideas::is_muse_idea(card) {
        return title.to_string();
    }
    // A repeated-ask offer's title is its question ("You ask this most
    // evenings around 7 PM, …"); the schedule line is in its preview.
    if crate::ask_offers::is_ask_offer_card(card) {
        return title.to_string();
    }
    let lower = title.to_ascii_lowercase();
    if lower.starts_with("i can ") && title.chars().count() <= I_CAN_MAX {
        return title.to_string();
    }
    // A suggestion's title already names its item: a proactive one is the
    // step ("Tidy the board"), a situation, router or review one is its own
    // line ("Paused: Fix the tray icon"). "I can paused: …" read as nonsense.
    let suggests = card.kind == UpdateKind::Suggestion
        && card
            .pulse
            .proactive
            .as_ref()
            .is_none_or(|p| p.route == crate::proactive::ProactiveRoute::Suggest);
    if suggests {
        return clip_words(title, I_CAN_MAX);
    }
    let action = card.idea_action();
    let action = one_line(&action);
    let what = if action.is_empty() || action.starts_with('/') {
        lower_first(title.trim_end_matches('.'))
    } else {
        lower_first(&action)
    };
    let mut lines: Vec<String> = Vec::new();
    match pulse_type(card) {
        PulseType::Automate => {
            if let Some((cadence, rest)) = split_cadence(&what) {
                lines.push(format!("I can {} {cadence}", toward_you(&rest)));
                if let Some((head, _)) = rest.split_once(" and ") {
                    lines.push(format!("I can {} {cadence}", toward_you(head)));
                }
            }
            // A schedule with no comma can't be split: the title says what it does.
            let plain = if schedule_led(&what) {
                lower_first(title.trim_end_matches('.'))
            } else {
                what.clone()
            };
            lines.push(format!("I can {} automatically", toward_you(&plain)));
        }
        PulseType::Learn => {
            let t = lower_first(title.trim_end_matches('.'));
            if !action.is_empty() {
                lines.push(format!("I can learn how you {}", toward_you(&what)));
            } else if t.split_whitespace().count() >= 3 {
                lines.push(format!("I can learn how you {}", toward_you(&t)));
            }
            lines.push(format!("I can learn your {}", toward_you(&t)));
        }
        _ => {
            lines.push(format!("I can {}", toward_you(&what)));
            if let Some((head, _)) = what.split_once(" and ") {
                lines.push(format!("I can {}", toward_you(head)));
            }
        }
    }
    let first = lines[0].clone();
    lines
        .into_iter()
        .find(|l| l.chars().count() <= I_CAN_MAX)
        .unwrap_or_else(|| clip_words(&first, I_CAN_MAX))
}

fn one_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches('.')
        .to_string()
}

fn schedule_led(action: &str) -> bool {
    let l = action.to_ascii_lowercase();
    ["every ", "each ", "daily", "weekdays", "on weekdays"]
        .iter()
        .any(|p| l.starts_with(p))
}

/// "every weekday at 8, run the tests" → ("every weekday at 8", "run the tests").
fn split_cadence(action: &str) -> Option<(String, String)> {
    if !schedule_led(action) {
        return None;
    }
    let (cadence, rest) = action.split_once(',')?;
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    Some((cadence.trim().to_string(), lower_first(rest)))
}

/// Cut at a word so the line plus "…" fits in `max` characters.
fn clip_words(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out = String::new();
    for word in text.split(' ') {
        let next = if out.is_empty() {
            word.to_string()
        } else {
            format!("{out} {word}")
        };
        if next.chars().count() + 1 > max {
            break;
        }
        out = next;
    }
    out.push('…');
    out
}

/// The Snooze label for the time it is now: today before nine, else tomorrow.
pub fn snooze_label(hour: u32) -> &'static str {
    if hour < 9 {
        "Snooze until 9 AM"
    } else {
        "Snooze until tomorrow 9 AM"
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        // Keep acronyms ("PR", "CI") as written.
        Some(_) if chars.clone().next().is_some_and(|n| n.is_uppercase()) => text.to_string(),
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn toward_you(text: &str) -> String {
    text.split(' ')
        .map(|w| match w {
            "my" => "your",
            "me" => "you",
            "mine" => "yours",
            "I" => "you",
            _ => w,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn is_feed_card(card: &UpdateCard) -> bool {
    !is_idea_card(card)
}

// ledger

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerReason {
    NotThis,
    Dismiss,
    /// Used or engaged, then dismissed — neutral-complete, never a bury.
    Used,
    Accepted,
    Wrong,
    Liked,
}

impl LedgerReason {
    pub fn key(self) -> &'static str {
        match self {
            Self::NotThis => "not-this",
            Self::Dismiss => "dismiss",
            Self::Used => "used",
            Self::Accepted => "accepted",
            Self::Wrong => "wrong",
            Self::Liked => "liked",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Self::NotThis | Self::Dismiss => "dismissed",
            Self::Used => "used",
            Self::Accepted => "accepted",
            Self::Wrong => "rejected",
            Self::Liked => "liked",
        }
    }

    fn parse(key: &str) -> Option<Self> {
        [
            Self::NotThis,
            Self::Dismiss,
            Self::Used,
            Self::Accepted,
            Self::Wrong,
            Self::Liked,
        ]
        .into_iter()
        .find(|r| r.key() == key.trim())
    }

    /// Likes and dislikes the feed instructions rewrite learns from.
    pub fn is_taste(self) -> bool {
        matches!(
            self,
            Self::NotThis | Self::Wrong | Self::Liked | Self::Accepted
        )
    }

    pub fn is_dislike(self) -> bool {
        matches!(self, Self::NotThis | Self::Wrong | Self::Dismiss)
    }

    /// True when this reason should move interest / reaction scores down.
    pub fn is_negative_feedback(self) -> bool {
        self.is_dislike()
    }
}

/// How a Dismiss should be recorded for learning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DismissKind {
    /// Never engaged — "not interested", negative.
    NotInterested,
    /// Used / liked / discussed then dismissed — neutral-complete.
    UsedDone,
}

impl DismissKind {
    pub fn ledger_reason(self) -> LedgerReason {
        match self {
            Self::NotInterested => LedgerReason::Dismiss,
            Self::UsedDone => LedgerReason::Used,
        }
    }

    /// True when reactions / interests / topic bury should treat this as a no.
    pub fn counts_negative(self) -> bool {
        matches!(self, Self::NotInterested)
    }
}

/// For You card: selected, ran, opened into a task, or completed.
/// `UpdateCard.runs` defaults to 1 on every new card, so only `runs > 1`
/// means it actually ran; prefer `used_depth` / Opened / modified.
pub fn idea_was_used(card: &UpdateCard, used_depth: u8) -> bool {
    card.status == UpdateStatus::Opened
        || card.built
        || card.discuss_thread.is_some()
        || used_depth > 0
        || card.runs > 1
        || card.modified
}

/// Feed card: liked or Discuss clicked.
pub fn feed_was_engaged(card: &UpdateCard) -> bool {
    card.reaction == Some(CardReaction::Up) || card.discuss_thread.is_some()
}

/// Whether the card was engaged before Dismiss, so the dismiss is not a bury.
pub fn card_engaged_before_dismiss(card: &UpdateCard, used_depth: u8) -> bool {
    if is_idea_card(card) {
        idea_was_used(card, used_depth)
    } else {
        feed_was_engaged(card)
    }
}

pub fn dismiss_kind(card: &UpdateCard, used_depth: u8) -> DismissKind {
    if card_engaged_before_dismiss(card, used_depth) {
        DismissKind::UsedDone
    } else {
        DismissKind::NotInterested
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub date: String,
    pub title: String,
    pub reason: LedgerReason,
}

/// The ledger's own file in `memory/`, so MEMORY.md keeps only long-term notes.
pub const LEDGER_FILE: &str = "pulse-ledger.md";

/// Split MEMORY.md into the body it keeps and the ledger lines that move to
/// [`LEDGER_FILE`], oldest first. Lines that are not ledger lines stay as written.
pub fn split_ledger(memory_md: &str) -> (String, Vec<String>) {
    let mut kept = String::with_capacity(memory_md.len());
    let mut moved = Vec::new();
    for line in memory_md.split_inclusive('\n') {
        if parse_ledger_line(line).is_some() {
            moved.push(line.trim().to_string());
        } else {
            kept.push_str(line);
        }
    }
    (kept, moved)
}

/// The ledger file with `older` lines in front, skipping any it already holds.
pub fn merge_ledger(ledger: &str, older: &[String]) -> String {
    let have: std::collections::HashSet<&str> = ledger.lines().map(str::trim).collect();
    let mut seen = std::collections::HashSet::new();
    let mut out = String::new();
    for line in older {
        if !have.contains(line.as_str()) && seen.insert(line.as_str()) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str(ledger);
    out
}

/// One ledger line: `- [2026-10-04] pulse: dismissed "summarize workboard" reason=not-this`.
pub fn ledger_line(date: &str, title: &str, reason: LedgerReason) -> String {
    let title: String = title
        .chars()
        .filter(|c| !matches!(c, '"' | '\n' | '\r'))
        .take(120)
        .collect();
    format!(
        "- [{}] pulse: {} \"{}\" reason={}",
        date.trim(),
        reason.verb(),
        title.trim(),
        reason.key()
    )
}

/// Every pulse ledger line in `text`, oldest first. Other lines are ignored.
pub fn parse_ledger(memory_md: &str) -> Vec<LedgerEntry> {
    memory_md.lines().filter_map(parse_ledger_line).collect()
}

fn parse_ledger_line(line: &str) -> Option<LedgerEntry> {
    let rest = line.trim().strip_prefix("- [")?;
    let (date, rest) = rest.split_once(']')?;
    let rest = rest.trim_start().strip_prefix("pulse:")?.trim_start();
    let (_, rest) = rest.split_once('"')?;
    let (title, rest) = rest.split_once('"')?;
    let reason = rest.trim().strip_prefix("reason=")?;
    Some(LedgerEntry {
        date: date.trim().to_string(),
        title: title.to_string(),
        reason: LedgerReason::parse(reason)?,
    })
}

// ranking

/// Rule weights, in the brief's order. Deterministic: no clock noise, no model.
pub const SCORE_SKILL_ASK: i64 = 1000;
pub const SCORE_DUE: i64 = 500;
pub const SCORE_UNFINISHED: i64 = 250;
pub const SCORE_REPEAT: i64 = 120;
pub const SCORE_LEARN_BOOST: i64 = 80;
/// A Like on a card with the same title or topic.
pub const SCORE_LIKED: i64 = 60;
pub const PENALTY_NOT_THIS: i64 = 400;
pub const PENALTY_DISMISS: i64 = 200;
/// A card below this is Quiet: nothing is shown for it.
pub const PULSE_THRESHOLD: i64 = 100;
/// How far under the threshold a quieted category's card lands.
pub const REACTIONS_QUIET_PENALTY: i64 = 1000;
/// The Ideas view opens with this many ideas under no header: the most
/// personal ones, by rank.
pub const IDEAS_LEAD: usize = 3;
/// An idea retires this long after it was posted, unless it was edited.
pub const IDEA_TTL_MS: u64 = 14 * 24 * 60 * 60 * 1000;
pub const DUE_WINDOW_MS: u64 = 24 * 60 * 60 * 1000;

/// Starting score per type, before any rule. Every type clears the threshold
/// until feedback pushes it under.
pub fn base_score(kind: PulseType) -> i64 {
    match kind {
        PulseType::Do => 150,
        PulseType::Automate => 140,
        PulseType::Learn => 130,
        PulseType::Watch => 110,
        PulseType::Quiet => 0,
    }
}

/// What the rules look at besides the card itself.
#[derive(Debug, Clone, Default)]
pub struct PulseInputs<'a> {
    pub ledger: &'a [LedgerEntry],
    /// Saved skill names and triggers.
    pub skills: &'a [String],
    /// Titles of open workboard cards.
    pub open_work: &'a [String],
    pub now_ms: u64,
    /// Learned reactions per category; None leaves every score as it was.
    pub reactions: Option<&'a crate::reactions::Reactions>,
}

fn asks_explicitly(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "remind me",
        "need to",
        "don't forget",
        "dont forget",
        "remember to",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

/// The inputs' topic words, worked out once per ranking. Pulse ranks every
/// frame, and comparing each card with each ledger line, skill and open card
/// re-split both sides every time.
struct InputTopics {
    ledger: Vec<Vec<String>>,
    skills: Vec<Vec<String>>,
    open_work: Vec<Vec<String>>,
}

impl InputTopics {
    fn of(inputs: &PulseInputs) -> Self {
        let words = |lines: &mut dyn Iterator<Item = &str>| lines.map(topic_words).collect();
        Self {
            ledger: words(&mut inputs.ledger.iter().map(|e| e.title.as_str())),
            skills: words(&mut inputs.skills.iter().map(String::as_str)),
            open_work: words(&mut inputs.open_work.iter().map(String::as_str)),
        }
    }
}

/// The card's score: base by type, plus each rule that fires.
pub fn pulse_score(card: &UpdateCard, inputs: &PulseInputs) -> i64 {
    score_with(card, inputs, &InputTopics::of(inputs))
}

fn score_with(card: &UpdateCard, inputs: &PulseInputs, topics: &InputTopics) -> i64 {
    let text = card_text(card);
    let title_words = topic_words(&card.title);
    let mut score = base_score(pulse_type(card));
    // 1. An explicit remind-me / need-to that a saved skill covers.
    if asks_explicitly(&text) {
        let text_words = topic_words(&text);
        if topics
            .skills
            .iter()
            .any(|s| same_topic_words(s, &text_words))
        {
            score += SCORE_SKILL_ASK;
        }
    }
    // 2. Due inside a day, or overdue.
    if card
        .pulse
        .due_at
        .is_some_and(|at| at <= inputs.now_ms.saturating_add(DUE_WINDOW_MS))
    {
        score += SCORE_DUE;
    }
    // 3. Unfinished work: you started it, kept it pinned on the old Home deck,
    // or an open board card is about it.
    if card.modified
        || card.feed_pin
        || card.draft.is_some()
        || topics
            .open_work
            .iter()
            .any(|w| same_topic_words(w, &title_words))
    {
        score += SCORE_UNFINISHED;
    }
    // 4. Something you did three times.
    if card.runs >= 3 || card.kind == UpdateKind::AutomateOffer {
        score += SCORE_REPEAT;
    }
    // 5 and 6. The ledger: accepted Learn cards and likes lift a topic, Not
    // this and unused Dismiss push it down. Used-then-dismissed is neutral. Dismiss only
    // buries ideas: a dismissed Feed post is gone, but tomorrow's run report or
    // quiet digest with that title still posts.
    let idea = is_idea_card(card);
    for (entry, words) in inputs.ledger.iter().zip(&topics.ledger) {
        let hit = entry.title.eq_ignore_ascii_case(card.title.trim())
            || same_topic_words(words, &title_words);
        if !hit {
            continue;
        }
        score += match entry.reason {
            LedgerReason::Accepted => SCORE_LEARN_BOOST,
            LedgerReason::Liked => SCORE_LIKED,
            LedgerReason::NotThis | LedgerReason::Wrong => -PENALTY_NOT_THIS,
            LedgerReason::Dismiss if idea => -PENALTY_DISMISS,
            LedgerReason::Dismiss | LedgerReason::Used => 0,
        };
    }
    // 7. A proactive offer's help (value × confidence × reversibility, × 1000).
    // Cards without candidate data add nothing, so their order is unchanged.
    if let Some(p) = &card.pulse.proactive {
        score += i64::from(p.help);
    }
    // 8. How you react to this kind of card: a quiet category drops under
    // the threshold, the rest move by their learned score.
    if let Some(reactions) = inputs.reactions {
        if let Some((category, _)) = crate::reactions::card_category_tag(card) {
            if reactions.quiet(&category) {
                return score.min(PULSE_THRESHOLD - 1) - REACTIONS_QUIET_PENALTY;
            }
            score += reactions.rank_boost(&category);
        }
    }
    score
}

/// A card whose category learned reactions have quieted.
fn reaction_quiet(card: &UpdateCard, inputs: &PulseInputs) -> bool {
    inputs.reactions.is_some_and(|r| {
        crate::reactions::card_category_tag(card).is_some_and(|(cat, _)| r.quiet(&cat))
    })
}

/// On the page now: not dismissed, not held by quiet hours, not snoozed.
pub fn pulse_visible(card: &UpdateCard, now_ms: u64) -> bool {
    card.status != UpdateStatus::Dismissed && !card.held && card.pulse.snoozed_until <= now_ms
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranked {
    pub id: String,
    pub score: i64,
    pub kind: PulseType,
    pub category: PulseCategory,
}

/// Visible cards, best first: score, then newer, then id. Cards under the
/// threshold are kept with kind Quiet so the page can say why it is silent.
pub fn rank_pulse(cards: &[UpdateCard], inputs: &PulseInputs) -> Vec<Ranked> {
    let topics = InputTopics::of(inputs);
    let mut out: Vec<(Ranked, u64)> = cards
        .iter()
        .filter(|c| pulse_visible(c, inputs.now_ms))
        .map(|c| {
            let score = score_with(c, inputs, &topics);
            let kind = if score < PULSE_THRESHOLD {
                PulseType::Quiet
            } else {
                pulse_type(c)
            };
            (
                Ranked {
                    id: c.id.clone(),
                    score,
                    kind,
                    category: card_category(c),
                },
                c.created_at,
            )
        })
        .collect();
    out.sort_by(|(a, at), (b, bt)| b.score.cmp(&a.score).then(bt.cmp(at)).then(a.id.cmp(&b.id)));
    out.into_iter().map(|(r, _)| r).collect()
}

/// A written idea retires at its `expires_at`, two weeks after it was
/// posted (`IDEA_TTL_MS`). One the person edited stays.
pub fn idea_retired(card: &UpdateCard, now_ms: u64) -> bool {
    card.kind == UpdateKind::Idea
        && !card.modified
        && card.expires_at.is_some_and(|at| now_ms >= at)
}

/// The Ideas view: the first `IDEAS_LEAD` ideas by rank under no header
/// (`None`), then one group per category in `PulseCategory::ORDER`. Quiet
/// and retired cards and empty groups are left out.
pub fn group_ideas(
    cards: &[UpdateCard],
    inputs: &PulseInputs,
) -> Vec<(Option<PulseCategory>, Vec<Ranked>)> {
    let ranked: Vec<Ranked> = rank_pulse(cards, inputs)
        .into_iter()
        .filter(|r| r.kind != PulseType::Quiet)
        .filter(|r| {
            cards
                .iter()
                .any(|c| c.id == r.id && is_idea_card(c) && !idea_retired(c, inputs.now_ms))
        })
        .collect();
    let top: Vec<Ranked> = ranked.iter().take(IDEAS_LEAD).cloned().collect();
    let mut groups = Vec::new();
    if !top.is_empty() {
        groups.push((None, top.clone()));
    }
    for cat in PulseCategory::ORDER {
        let rows: Vec<Ranked> = ranked
            .iter()
            .filter(|r| r.category == cat && !top.iter().any(|t| t.id == r.id))
            .cloned()
            .collect();
        if !rows.is_empty() {
            groups.push((Some(cat), rows));
        }
    }
    groups
}

/// A fetched or written post leaves the feed this long after it was posted,
/// unless it was kept: news is stale in a day or two, patch notes in about three.
pub const POST_TTL_MS: u64 = 3 * 24 * 60 * 60 * 1000;
/// Posts newer than this make up today's set.
pub const TODAY_WINDOW_MS: u64 = 24 * 60 * 60 * 1000;
/// Today's set holds at most this many posts.
pub const TODAY_MAX: usize = 12;
/// And at most this many from one source (one game, one subreddit, one site).
pub const TODAY_PER_SOURCE: usize = 2;

/// Loved, saved, pinned, edited or discussed: the person wants it kept.
pub fn card_kept(card: &UpdateCard) -> bool {
    card.modified
        || card.feed_pin
        || card.feed_kept
        || card.reaction == Some(crate::update_feed::CardReaction::Up)
        || card.discuss_thread.is_some()
}

/// A post from a followed source or the writer, older than `POST_TTL_MS`
/// and not kept. Run reports, digests and other feed cards keep their own rules.
pub fn post_retired(card: &UpdateCard, now_ms: u64) -> bool {
    let fetched = card.source_id.starts_with("source:")
        || card
            .source_id
            .starts_with(crate::pulse_writer::WRITER_SOURCE);
    fetched && !card_kept(card) && now_ms >= card.created_at.saturating_add(POST_TTL_MS)
}

fn post_source(card: &UpdateCard) -> &str {
    card.pulse
        .source_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(card.source_id.as_str())
}

/// Why a fetched post is in the feed, in plain words: "You play Helldivers 2",
/// "Matches your interest in Rust", "From r/buildapc, a source you follow".
/// None for cards that are not fetched posts.
pub fn plain_why(card: &UpdateCard) -> Option<String> {
    if !crate::pulse_sources::is_source_post(card) {
        return None;
    }
    let source = card
        .pulse
        .source_name
        .as_deref()
        .map(str::trim)
        .unwrap_or("");
    let because = card
        .pulse
        .because
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty());
    Some(match because {
        Some(game) if source == format!("{game} on Steam") => format!("You play {game}"),
        Some(topic) => format!("Matches your interest in {topic}"),
        None if !source.is_empty() => format!("From {source}, a source you follow"),
        None => "From a source you follow".to_string(),
    })
}

/// The Feed view split in two: today's set (posted in the last
/// `TODAY_WINDOW_MS`, newest first, at most `TODAY_PER_SOURCE` per source and
/// `TODAY_MAX` in all), then everything else, newest first. Retired posts are
/// left out. No score gate: the writer already chose what is worth posting.
pub fn feed_day(cards: &[UpdateCard], inputs: &PulseInputs) -> (Vec<UpdateCard>, Vec<UpdateCard>) {
    let now = inputs.now_ms;
    let mut posts: Vec<UpdateCard> = cards
        .iter()
        .filter(|c| is_feed_card(c) && pulse_visible(c, now) && !post_retired(c, now))
        .filter(|c| !reaction_quiet(c, inputs))
        .cloned()
        .collect();
    posts.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    let mut today = Vec::new();
    let mut older = Vec::new();
    for card in posts {
        let fresh = now < card.created_at.saturating_add(TODAY_WINDOW_MS);
        let from_source = today
            .iter()
            .filter(|t: &&UpdateCard| post_source(t) == post_source(&card))
            .count();
        if fresh && today.len() < TODAY_MAX && from_source < TODAY_PER_SOURCE {
            today.push(card);
        } else {
            older.push(card);
        }
    }
    (today, older)
}

/// The Feed view as one list: today's set first, then the older posts.
pub fn feed_posts(cards: &[UpdateCard], inputs: &PulseInputs) -> Vec<UpdateCard> {
    let (mut today, older) = feed_day(cards, inputs);
    today.extend(older);
    today
}

// pings

/// Pulse may raise at most this many OS notifications a day; the rest wait
/// on the page.
pub const PINGS_PER_DAY: u32 = 3;

/// What a Pulse ping asks about the moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PingAsk<'a> {
    /// Due within the hour, or a watch that just hit. Anything else waits on
    /// the page.
    pub time_sensitive: bool,
    /// A game is running: never interrupt it.
    pub game_running: bool,
    pub quiet_hours: bool,
    /// The person's local day, like "2026-11-06".
    pub day: &'a str,
}

/// The one gate every Pulse-origin OS notification goes through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PingGate {
    day: String,
    sent: u32,
}

impl PingGate {
    pub const fn new() -> Self {
        Self {
            day: String::new(),
            sent: 0,
        }
    }

    pub fn sent_on(&self, day: &str) -> u32 {
        if self.day == day {
            self.sent
        } else {
            0
        }
    }

    pub fn may_ping(&self, ask: &PingAsk) -> bool {
        ask.time_sensitive
            && !ask.game_running
            && !ask.quiet_hours
            && self.sent_on(ask.day) < PINGS_PER_DAY
    }

    /// Count a ping that the gate lets through. False means stay quiet.
    pub fn take(&mut self, ask: &PingAsk) -> bool {
        if !self.may_ping(ask) {
            return false;
        }
        self.sent = self.sent_on(ask.day) + 1;
        self.day = ask.day.to_string();
        true
    }
}

// buttons

/// Run: the idea's own action as a background task. Never the composer.
pub fn run_line(card: &UpdateCard) -> String {
    let action = card.idea_action();
    let what = if action.is_empty() {
        card.title.trim().to_string()
    } else {
        action
    };
    format!("/bg {what}")
}

/// Snooze: the next 09:00 local after now (later today before nine, else tomorrow).
pub fn snooze_until(now_ms: u64, hour: u32, minute: u32) -> u64 {
    let minute_start = now_ms - now_ms % 60_000;
    let since_midnight = u64::from(hour.min(23)) * 60 + u64::from(minute.min(59));
    let midnight = minute_start.saturating_sub(since_midnight * 60_000);
    let nine = 9 * 60;
    let target = if since_midnight < nine {
        nine
    } else {
        nine + 24 * 60
    };
    midnight + target * 60_000
}

/// × on the main window's deck: the card leaves that deck only. Pulse keeps it.
pub fn hide_from_home(cards: &mut [UpdateCard], id: &str) -> bool {
    match cards
        .iter_mut()
        .find(|c| c.id == id && c.status != UpdateStatus::Dismissed && !c.pulse.off_home)
    {
        Some(card) => {
            card.pulse.off_home = true;
            true
        }
        None => false,
    }
}

pub fn snooze_card(cards: &mut [UpdateCard], id: &str, until: u64) -> bool {
    match cards.iter_mut().find(|c| c.id == id) {
        Some(card) => {
            card.pulse.snoozed_until = until;
            true
        }
        None => false,
    }
}

// feed instructions

/// Our own default wording. The person can rewrite all of it.
pub const DEFAULT_FEED_INSTRUCTIONS: &str = "Keep my feed short and easy to skim. Mostly news and stories about the things I work on and care about, each explained plainly in two or three sentences, with a real source and a real image when the source has one. No clickbait, no filler, no repeats.

Show more of:
- Updates on the projects, tools, and topics from my recent chats.

Show less of:
- Anything I dismissed or marked Not this.";

/// The cap the rewrite and the editor keep to.
pub const FEED_INSTRUCTIONS_CAP: usize = 2000;
/// Taste lines (likes and dislikes) that trigger a rewrite.
pub const REWRITE_AFTER: u32 = 3;

pub fn feed_instructions_or_default(text: &str) -> &str {
    if text.trim().is_empty() {
        DEFAULT_FEED_INSTRUCTIONS
    } else {
        text
    }
}

/// An older build's one-line digest brief becomes a Focus line in the instructions.
pub fn instructions_from_brief(brief: &str) -> String {
    let brief = brief.trim();
    if brief.is_empty() {
        return DEFAULT_FEED_INSTRUCTIONS.to_string();
    }
    format!("{DEFAULT_FEED_INSTRUCTIONS}\n\nFocus: {brief}")
}

/// The digest lookup with the person's interests and instructions. They decide what belongs.
pub fn feed_prompt(steer: &str, interests: &str, instructions: &str) -> String {
    let mut base = crate::update_feed::digest_lookup_prompt(steer);
    let interests = interests.trim();
    if !interests.is_empty() {
        base.push_str("\n\n");
        base.push_str(interests);
    }
    let text: String = feed_instructions_or_default(instructions)
        .trim()
        .chars()
        .take(FEED_INSTRUCTIONS_CAP)
        .collect();
    format!("{base}\n\nFeed instructions from the person (follow them; they apply to this and future posts):\n{text}")
}

/// Ask the model to rewrite the instructions from what was liked and disliked.
pub fn rewrite_prompt(current: &str, taste: &[LedgerEntry]) -> String {
    let mut lines = String::new();
    for entry in taste {
        let label = if entry.reason.is_dislike() {
            "Disliked"
        } else {
            "Liked"
        };
        lines.push_str(&format!("- {label}: {}\n", entry.title));
    }
    format!(
        "Rewrite these feed instructions so future posts fit this person better. Keep their own wording and every rule they wrote. \
Fold in what they liked and disliked below: add or sharpen \"Show more of\" and \"Show less of\" lines. Plain text only, under {FEED_INSTRUCTIONS_CAP} characters. \
Reply with the full new instructions and nothing else.\n\nCurrent instructions:\n{}\n\nWhat they did:\n{lines}",
        feed_instructions_or_default(current).trim()
    )
}

/// The model's rewrite, cleaned. `None` keeps the current text.
pub fn parse_rewrite(reply: &str) -> Option<String> {
    let mut text = reply.trim();
    if let Some(inner) = text.strip_prefix("```") {
        let inner = inner.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
        text = inner.trim_end().strip_suffix("```").unwrap_or(inner).trim();
    }
    if text.len() < 20 || text.eq_ignore_ascii_case("none") {
        return None;
    }
    Some(text.chars().take(FEED_INSTRUCTIONS_CAP).collect())
}

/// The last `REWRITE_AFTER` taste lines, the ones a rewrite folds in.
pub fn recent_taste(ledger: &[LedgerEntry], n: usize) -> Vec<LedgerEntry> {
    let taste: Vec<&LedgerEntry> = ledger.iter().filter(|e| e.reason.is_taste()).collect();
    taste[taste.len().saturating_sub(n)..]
        .iter()
        .map(|e| (*e).clone())
        .collect()
}

// quiet hours

pub const QUIET_DIGEST_TITLE: &str = "While you were in quiet hours";
pub const QUIET_DIGEST_SOURCE: &str = "pulse:quiet";

/// Quiet hours ended: release every held card. Two or more become one digest
/// card (and ping nothing on their own); a single card is released as is.
/// Returns how many were released and the digest id, if one was posted.
pub fn release_quiet_batch(cards: &mut Vec<UpdateCard>, now_ms: u64) -> (usize, Option<String>) {
    let mut titles = Vec::new();
    let mut ids = Vec::new();
    let mut items = Vec::new();
    for card in cards.iter_mut().filter(|c| c.held) {
        card.held = false;
        titles.push(card.title.clone());
        ids.push(card.id.clone());
        items.push(crate::card_report::ReportItem {
            card_id: card.id.clone(),
            found: crate::card_report::card_headline(card),
            summary: crate::card_report::card_summary(card),
        });
    }
    let n = titles.len();
    if n < 2 {
        return (n, None);
    }
    for card in cards.iter_mut().filter(|c| ids.contains(&c.id)) {
        card.pulse.quiet_batched = true;
    }
    // Yesterday's dismissed digest must not swallow today's.
    cards.retain(|c| !(c.source_id == QUIET_DIGEST_SOURCE && c.status == UpdateStatus::Dismissed));
    let shown: Vec<&str> = titles.iter().take(3).map(String::as_str).collect();
    let mut body = format!("{n} updates: {}", shown.join(" · "));
    if n > 3 {
        body.push_str(&format!(" · and {} more", n - 3));
    }
    let id = format!("pulse-quiet-{now_ms}");
    let mut digest =
        crate::update_feed::suggestion_card(QUIET_DIGEST_SOURCE, QUIET_DIGEST_TITLE, &body, now_ms);
    digest.id = id.clone();
    digest.source_id = QUIET_DIGEST_SOURCE.to_string();
    digest.pulse.source_name = Some("Quiet hours".into());
    digest.pulse.report = Some(crate::card_report::CardReport {
        found: format!("{n} updates came in during quiet hours"),
        summary: body.clone(),
        items,
        ..Default::default()
    });
    post_update(cards, digest);
    (n, Some(id))
}

// real images

/// The page's own preview image: `og:image`, else `twitter:image`. Relative
/// paths resolve against the page. Only public http(s) URLs.
pub fn og_image(html: &str, page_url: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    for key in ["og:image:secure_url", "og:image", "twitter:image"] {
        let mut from = 0;
        while let Some(pos) = lower[from..].find("<meta") {
            let start = from + pos;
            let end = lower[start..]
                .find('>')
                .map(|e| start + e)
                .unwrap_or(lower.len());
            let tag = &html[start..end];
            let tag_lower = &lower[start..end];
            from = end;
            let names = [
                attr(tag, tag_lower, "property"),
                attr(tag, tag_lower, "name"),
            ];
            if !names.iter().flatten().any(|n| n.eq_ignore_ascii_case(key)) {
                continue;
            }
            let Some(content) = attr(tag, tag_lower, "content") else {
                continue;
            };
            match resolve_url(content.trim(), page_url) {
                Some(url) if public_http_url(&url) => return Some(url),
                _ => continue,
            }
        }
    }
    None
}

/// Images a post's gallery may show: the page's own preview image, then up
/// to two more large in-page images. Logos, icons, avatars, tracking pixels
/// and anything marked smaller than `GALLERY_MIN_PX` are skipped.
pub fn gallery_images(html: &str, page_url: &str) -> Vec<String> {
    let mut out: Vec<String> = og_image(html, page_url).into_iter().collect();
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while out.len() < GALLERY_MAX {
        let Some(pos) = lower[from..].find("<img") else {
            break;
        };
        let start = from + pos;
        let end = lower[start..]
            .find('>')
            .map(|e| start + e)
            .unwrap_or(lower.len());
        let (tag, tag_lower) = (&html[start..end], &lower[start..end]);
        from = end;
        let Some(src) = attr(tag, tag_lower, "src") else {
            continue;
        };
        let small = ["width", "height"].iter().any(|k| {
            attr(tag, tag_lower, k)
                .and_then(|v| v.trim().trim_end_matches("px").parse::<u32>().ok())
                .is_some_and(|px| px < GALLERY_MIN_PX)
        });
        let alt = attr(tag, tag_lower, "alt")
            .unwrap_or("")
            .to_ascii_lowercase();
        let class = attr(tag, tag_lower, "class")
            .unwrap_or("")
            .to_ascii_lowercase();
        let path = src.to_ascii_lowercase();
        let junk = small
            || path.ends_with(".svg")
            || path.ends_with(".gif")
            || path.starts_with("data:")
            || GALLERY_SKIP
                .iter()
                .any(|w| path.contains(w) || alt.contains(w) || class.contains(w));
        if junk {
            continue;
        }
        let Some(url) = resolve_url(src.trim(), page_url).filter(|u| public_http_url(u)) else {
            continue;
        };
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// Most images on one post.
pub const GALLERY_MAX: usize = 3;
/// An image marked narrower or shorter than this is not a content image.
pub const GALLERY_MIN_PX: u32 = 200;
/// Smallest image file worth keeping: tiny files are icons or spacers.
pub const IMAGE_MIN_BYTES: usize = 8 * 1024;
/// Largest image file kept in the local cache.
pub const IMAGE_MAX_BYTES: usize = 4 * 1024 * 1024;
const GALLERY_SKIP: &[&str] = &[
    "logo", "icon", "avatar", "sprite", "badge", "pixel", "spacer", "emoji", "favicon", "button",
];

/// A downloaded image worth caching: a real JPEG, PNG or WebP, not tiny, not huge.
pub fn image_bytes_ok(bytes: &[u8]) -> bool {
    let real = bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(&[0x89, b'P', b'N', b'G'])
        || (bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP");
    real && (IMAGE_MIN_BYTES..=IMAGE_MAX_BYTES).contains(&bytes.len())
}

fn attr<'a>(tag: &'a str, tag_lower: &str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(pos) = tag_lower[from..].find(name) {
        let at = from + pos;
        from = at + name.len();
        let before_ok = at == 0 || tag_lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = tag_lower[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let value_at = tag_lower.len() - rest.len() + 1;
        let value = tag[value_at..].trim_start();
        let offset = tag.len() - value.len();
        let quote = value.chars().next()?;
        return if quote == '"' || quote == '\'' {
            let inner = &tag[offset + 1..];
            inner.find(quote).map(|e| &inner[..e])
        } else {
            let e = value
                .find(|c: char| c.is_whitespace() || c == '/')
                .unwrap_or(value.len());
            Some(&value[..e])
        };
    }
    None
}

fn resolve_url(url: &str, page_url: &str) -> Option<String> {
    let url = url.replace("&amp;", "&");
    if url.starts_with("https://") || url.starts_with("http://") {
        return Some(url);
    }
    let scheme_end = page_url.find("://")? + 3;
    let host_end = page_url[scheme_end..]
        .find('/')
        .map(|e| scheme_end + e)
        .unwrap_or(page_url.len());
    if let Some(rest) = url.strip_prefix("//") {
        return Some(format!("{}{}", &page_url[..scheme_end], rest));
    }
    if url.starts_with('/') {
        return Some(format!("{}{}", &page_url[..host_end], url));
    }
    None
}

/// Cache file name for an image URL: a stable hash plus the URL's extension.
pub fn image_cache_name(url: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let ext = [".jpg", ".jpeg", ".png", ".webp"]
        .into_iter()
        .find(|e| path.ends_with(e))
        .map(|e| if e == ".jpeg" { ".jpg" } else { e })
        .unwrap_or(".img");
    format!("{h:016x}{ext}")
}

/// Site name for a post's source line: `www.theverge.com/…` → `theverge.com`.
pub fn source_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest
        .split(['/', '?', '#', ':'])
        .next()?
        .to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    (!host.is_empty()).then_some(host)
}

// migration

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PulseMigration {
    /// Live cards now on the Ideas view.
    pub ideas: usize,
    /// Live cards now on the Feed view.
    pub feed: usize,
    /// Cards pinned to the old Home deck. They keep the pin and rank in
    /// Pulse's top group (rule 3, unfinished work).
    pub pinned: usize,
    /// Every card kept, live or dismissed.
    pub kept: usize,
}

/// One-time move of the Home deck and the Ideas board into Pulse. Keeps every
/// card and every pin; stores a category on each idea; marks the feed
/// config so it never runs twice. `None` when it already ran.
pub fn migrate_to_pulse(cards: &mut [UpdateCard], pulse: &mut FeedPulse) -> Option<PulseMigration> {
    if pulse.pulse_v1 {
        return None;
    }
    let mut out = PulseMigration {
        kept: cards.len(),
        ..PulseMigration::default()
    };
    for card in cards.iter_mut() {
        if is_idea_card(card) && card.pulse.category.is_none() {
            card.pulse.category = Some(categorize(&card_text(card)).key().to_string());
        }
        if card.feed_pin {
            out.pinned += 1;
        }
        if card.pulse.source_name.is_none() {
            card.pulse.source_name = card.citations.first().and_then(|u| source_host(u));
        }
        if card.status == UpdateStatus::Dismissed {
            continue;
        }
        if is_idea_card(card) {
            out.ideas += 1;
        } else {
            out.feed += 1;
        }
    }
    pulse.pulse_v1 = true;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ideas::IdeaKind;
    use crate::update_feed::{
        automate_offer_card, automation_done_card, digest_card, idea_card, suggestion_card,
    };

    fn idea(id: &str, title: &str, body: &str, at: u64) -> UpdateCard {
        let mut c = idea_card("gen", title, body, at);
        c.id = id.into();
        c
    }

    #[test]
    fn categories_follow_the_keyword_table_and_the_fixed_order() {
        let cases = [
            (
                "I can watch Helldivers 2 patch notes for you",
                PulseCategory::Games,
            ),
            (
                "I can map a spoiler-free path through Witcher 3",
                PulseCategory::Games,
            ),
            (
                "I can check your GPU temps after each session",
                PulseCategory::Pc,
            ),
            (
                "I can find the newest Nvidia driver for your game",
                PulseCategory::Pc,
            ),
            (
                "I can check your Skyrim load order for mod conflicts",
                PulseCategory::Mods,
            ),
            ("I can watch your wishlist for a sale", PulseCategory::Deals),
            (
                "I can tell you when the Steam sale starts",
                PulseCategory::Deals,
            ),
            (
                "Tell me the item and I'll compare prices",
                PulseCategory::Deals,
            ),
            (
                "I can tidy your workboard every Friday",
                PulseCategory::Automations,
            ),
            (
                "I can tailor your resume for entry-level jobs",
                PulseCategory::More,
            ),
        ];
        for (text, want) in cases {
            assert_eq!(categorize(text), want, "{text}");
        }
        let labels: Vec<&str> = PulseCategory::ORDER.iter().map(|c| c.label()).collect();
        assert_eq!(
            labels,
            vec![
                "Games",
                "PC",
                "Mods & builds",
                "Deals",
                "Automations",
                "More for you"
            ]
        );
        let mut stored = idea("i1", "Witcher 3 route", "", 1);
        stored.pulse.category = Some("deals".into());
        assert_eq!(
            card_category(&stored),
            PulseCategory::Deals,
            "a stored category wins"
        );
        let old = |key: &str| PulseCategory::parse(key);
        assert_eq!(old("shopping"), Some(PulseCategory::Deals));
        assert_eq!(old("productivity"), Some(PulseCategory::Automations));
        assert_eq!(old("financial"), Some(PulseCategory::More));
        assert_eq!(old("relationships"), Some(PulseCategory::More));
        assert_eq!(old("health"), Some(PulseCategory::More));
        assert_eq!(old("fun"), None);
    }

    #[test]
    fn card_types_map_from_the_feed_kinds() {
        let mut auto = idea("a", "Weekly report", "", 1);
        auto.idea_kind = Some(IdeaKind::Automation);
        let mut remind = idea("r", "Renew card", "", 1);
        remind.idea_kind = Some(IdeaKind::Reminder);
        let mut skill = idea("s", "Packing list", "", 1);
        skill.idea_kind = Some(IdeaKind::Skill);
        let mut try_it = idea("t", "Try it", "", 1);
        try_it.idea_kind = Some(IdeaKind::Try);
        let got: Vec<&str> = [
            &auto,
            &remind,
            &skill,
            &try_it,
            &suggestion_card("x", "File notes", "", 1),
            &automate_offer_card("y", "Make it a loop", 1),
            &automation_done_card("z", "Brief", "done", 1),
            &digest_card("d", "Edition", "body", 1),
        ]
        .iter()
        .map(|c| pulse_type(c).label())
        .collect();
        assert_eq!(
            got,
            vec!["Automate", "Automate", "Learn", "Do", "Do", "Automate", "Watch", "Watch"]
        );
        // The row and the opened card read the same type: one source.
        for card in [&auto, &remind, &skill, &try_it] {
            let row = pulse_type(card).label();
            let opened = card.idea_type_label();
            assert_eq!(
                (row, opened),
                match opened {
                    "Automation" => ("Automate", "Automation"),
                    "Skill" => ("Learn", "Skill"),
                    _ => ("Do", "Suggestion"),
                }
            );
        }
    }

    #[test]
    fn ledger_lines_leave_memory_md_and_merge_in_front_once() {
        let memory = "# Memory\n\nLong-term notes.\n- [2026-10-04] pulse: dismissed \"Standup\" reason=dismiss\n- 2026-10-05: the spare key is under the planter\n  - [2026-10-06] pulse: liked \"Rust 1.92 ships\" reason=liked\nno newline at the end";
        let (kept, moved) = split_ledger(memory);
        assert_eq!(
            kept,
            "# Memory\n\nLong-term notes.\n- 2026-10-05: the spare key is under the planter\nno newline at the end"
        );
        assert_eq!(
            moved,
            vec![
                "- [2026-10-04] pulse: dismissed \"Standup\" reason=dismiss".to_string(),
                "- [2026-10-06] pulse: liked \"Rust 1.92 ships\" reason=liked".to_string(),
            ]
        );
        let ledger = "- [2026-10-06] pulse: liked \"Rust 1.92 ships\" reason=liked\n- [2026-10-07] pulse: used \"NFL scores\" reason=used\n";
        let merged = merge_ledger(ledger, &moved);
        assert_eq!(
            merged,
            "- [2026-10-04] pulse: dismissed \"Standup\" reason=dismiss\n- [2026-10-06] pulse: liked \"Rust 1.92 ships\" reason=liked\n- [2026-10-07] pulse: used \"NFL scores\" reason=used\n"
        );
        assert_eq!(
            merge_ledger(&merged, &moved),
            merged,
            "a second run adds nothing"
        );
        assert_eq!(
            split_ledger("# Memory\n"),
            ("# Memory\n".to_string(), Vec::new())
        );
    }

    #[test]
    fn ledger_lines_round_trip_through_memory_md() {
        let line = ledger_line(
            "2026-10-04",
            "summarize \"workboard\"",
            LedgerReason::NotThis,
        );
        assert_eq!(
            line,
            "- [2026-10-04] pulse: dismissed \"summarize workboard\" reason=not-this"
        );
        let md = format!("# Long-term notes\n- prefer nvim\n{line}\n- [2026-10-05] pulse: accepted \"Packing list\" reason=accepted\n");
        let ledger = parse_ledger(&md);
        assert_eq!(
            ledger,
            vec![
                LedgerEntry {
                    date: "2026-10-04".into(),
                    title: "summarize workboard".into(),
                    reason: LedgerReason::NotThis
                },
                LedgerEntry {
                    date: "2026-10-05".into(),
                    title: "Packing list".into(),
                    reason: LedgerReason::Accepted
                },
            ]
        );
    }

    #[test]
    fn ranking_rules_give_literal_scores_and_a_stable_order() {
        let now = 1_000_000_000_000;
        let mut bill = idea("bill", "Remind me to pay the power bill", "", 10);
        bill.pulse.due_at = Some(now + 3_600_000);
        let mut started = idea("started", "Tidy the photos folder", "", 20);
        started.modified = true;
        let offer = automate_offer_card("loop", "Make the morning brief a loop", 30);
        let plain = idea("plain", "Map a route through Witcher 3", "", 40);
        let watch = automation_done_card("auto-1", "Nightly backup", "ok", 50);
        let cards = vec![
            bill.clone(),
            started.clone(),
            offer.clone(),
            plain.clone(),
            watch.clone(),
        ];
        let skills = vec!["pay power bill".to_string()];
        let inputs = PulseInputs {
            ledger: &[],
            skills: &skills,
            open_work: &[],
            now_ms: now,
            reactions: None,
        };
        assert_eq!(pulse_score(&bill, &inputs), 150 + 1000 + 500);
        assert_eq!(pulse_score(&started, &inputs), 150 + 250);
        assert_eq!(pulse_score(&offer, &inputs), 140 + 120);
        assert_eq!(pulse_score(&plain, &inputs), 150);
        assert_eq!(pulse_score(&watch, &inputs), 110);
        let order: Vec<(String, i64)> = rank_pulse(&cards, &inputs)
            .into_iter()
            .map(|r| (r.id, r.score))
            .collect();
        let offer_id = offer.id.clone();
        let watch_id = watch.id.clone();
        assert_eq!(
            order,
            vec![
                ("bill".to_string(), 1650),
                ("started".to_string(), 400),
                (offer_id, 260),
                ("plain".to_string(), 150),
                (watch_id, 110),
            ]
        );
        assert_eq!(
            rank_pulse(&cards, &inputs),
            rank_pulse(&cards, &inputs),
            "same inputs, same order"
        );
        // Spike-6a: a proactive offer adds its help x 1000 and slots in by
        // score; the cards without candidate data keep their scores and order.
        let mut c = crate::proactive::Candidate::soft(
            crate::proactive::CandidateSource::Workboard,
            "sum up the new report on \"Taxes\"",
            "Taxes report",
            0.6,
            0.8,
            "A run left a report.",
        );
        c.reversible = crate::proactive::Reversibility::ByHand;
        let offer6 =
            crate::proactive::proactive_card(&c, crate::proactive::ProactiveRoute::ICan, 60);
        assert_eq!(pulse_score(&offer6, &inputs), 150 + 240);
        let mut with_offer = cards.clone();
        with_offer.push(offer6.clone());
        let order6: Vec<(String, i64)> = rank_pulse(&with_offer, &inputs)
            .into_iter()
            .map(|r| (r.id, r.score))
            .collect();
        assert_eq!(
            order6,
            vec![
                ("bill".to_string(), 1650),
                ("started".to_string(), 400),
                (offer6.id.clone(), 390),
                (offer.id.clone(), 260),
                ("plain".to_string(), 150),
                (watch.id.clone(), 110),
            ]
        );
        let mut reversed = with_offer.clone();
        reversed.reverse();
        assert_eq!(
            rank_pulse(&with_offer, &inputs),
            rank_pulse(&reversed, &inputs),
            "input order never matters"
        );
    }

    #[test]
    fn not_this_and_dismiss_suppress_and_accepted_lifts() {
        let now = 5_000;
        let plain = idea("plain", "Map a route through Witcher 3", "", 40);
        let other = idea("other", "Build an Ace Combat handbook", "", 30);
        let cards = vec![plain.clone(), other.clone()];
        let before: Vec<String> = rank_pulse(
            &cards,
            &PulseInputs {
                now_ms: now,
                ..Default::default()
            },
        )
        .into_iter()
        .map(|r| r.id)
        .collect();
        assert_eq!(before, vec!["plain", "other"], "same score: newer first");
        let ledger = parse_ledger(
            "- [2026-10-04] pulse: dismissed \"Map a route through Witcher 3\" reason=not-this\n",
        );
        let inputs = PulseInputs {
            ledger: &ledger,
            now_ms: now,
            ..Default::default()
        };
        assert_eq!(pulse_score(&plain, &inputs), 150 - 400);
        let after = rank_pulse(&cards, &inputs);
        assert_eq!(after[0].id, "other");
        assert_eq!(
            (after[1].id.as_str(), after[1].score, after[1].kind),
            ("plain", -250, PulseType::Quiet)
        );
        let dismissed = parse_ledger(
            "- [2026-10-04] pulse: dismissed \"Build an Ace Combat handbook\" reason=dismiss\n",
        );
        assert_eq!(
            pulse_score(
                &other,
                &PulseInputs {
                    ledger: &dismissed,
                    now_ms: now,
                    ..Default::default()
                }
            ),
            -50
        );
        let accepted = parse_ledger(
            "- [2026-10-04] pulse: accepted \"Ace Combat handbook\" reason=accepted\n",
        );
        assert_eq!(
            pulse_score(
                &other,
                &PulseInputs {
                    ledger: &accepted,
                    now_ms: now,
                    ..Default::default()
                }
            ),
            230
        );
    }

    #[test]
    fn ideas_lead_with_three_then_the_five_categories_in_order() {
        let now = 30 * 24 * 60 * 60 * 1000;
        let at = |id: &str, title: &str, cat: &str, ago: u64| {
            let mut c = idea(id, title, "", now - ago);
            c.pulse.category = Some(cat.into());
            c
        };
        let mut bill = at("bill", "I can sum up the Helldivers 2 patch", "games", 90);
        bill.modified = true;
        let cards = vec![
            bill,
            at("groc", "I can watch your wishlist", "deals", 80),
            at("subs", "I can check your GPU temps", "pc", 70),
            at("resume", "I can tidy your workboard", "automations", 60),
            at("gym", "I can check your load order", "mods", 50),
            at("trivia", "I can plan a trivia night", "relationships", 40),
            at("deals", "I can watch for deals", "shopping", 30),
            at("route", "I can map a route", "games", 20),
            automation_done_card("a", "Nightly backup", "ok", now - 10),
        ];
        let groups: Vec<(Option<&str>, Vec<String>)> = group_ideas(
            &cards,
            &PulseInputs {
                now_ms: now,
                ..Default::default()
            },
        )
        .into_iter()
        .map(|(cat, rows)| {
            (
                cat.map(PulseCategory::label),
                rows.into_iter().map(|r| r.id).collect(),
            )
        })
        .collect();
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            groups,
            vec![
                (None, ids(&["bill", "route", "deals"])),
                (Some("PC"), ids(&["subs"])),
                (Some("Mods & builds"), ids(&["gym"])),
                (Some("Deals"), ids(&["groc"])),
                (Some("Automations"), ids(&["resume"])),
                (Some("More for you"), ids(&["trivia"])),
            ],
            "the edited idea and the two newest lead, with no header; an older key maps over"
        );
    }

    #[test]
    fn ideas_retire_after_two_weeks_unless_edited() {
        let posted = 1_000_000;
        let idea = crate::pulse_ideas::MuseIdea {
            title: "Cheaper groceries".into(),
            description: "I can plan meals around the sales.".into(),
            emoji: "🛒".into(),
            category: PulseCategory::Deals,
            action: "plan meals from this week's sales".into(),
        };
        let mut old = crate::pulse_ideas::muse_idea_card(&idea, posted);
        assert!(!idea_retired(&old, posted + IDEA_TTL_MS - 1));
        assert!(idea_retired(&old, posted + IDEA_TTL_MS));
        assert_eq!(IDEA_TTL_MS, 1_209_600_000, "14 days");
        let mut post = automation_done_card("a", "Nightly backup", "ok", posted);
        post.expires_at = old.expires_at;
        assert!(
            !idea_retired(&post, posted + IDEA_TTL_MS),
            "only ideas retire"
        );
        let inputs = PulseInputs {
            now_ms: posted + IDEA_TTL_MS,
            ..Default::default()
        };
        assert!(group_ideas(std::slice::from_ref(&old), &inputs).is_empty());
        old.modified = true;
        assert!(!idea_retired(&old, posted + IDEA_TTL_MS));
        assert_eq!(group_ideas(&[old], &inputs).len(), 1);
    }

    #[test]
    fn run_snooze_and_feed_posts() {
        let mut c = idea("i", "Weekly price check", "", 1);
        c.prompt = Some("every weekday at 12, check the laptop stand price".into());
        assert_eq!(
            run_line(&c),
            "/bg every weekday at 12, check the laptop stand price"
        );
        c.draft = Some("check the stand price now".into());
        assert_eq!(
            run_line(&c),
            "/bg check the stand price now",
            "your edited action wins"
        );
        assert_eq!(
            run_line(&idea("user", "Tidy the photos", "", 1)),
            "/bg Tidy the photos"
        );
        // 14:30 local → tomorrow 09:00; 07:15 local → today 09:00.
        let now = 1_000_000_035_000;
        assert_eq!(
            snooze_until(now, 14, 30),
            1_000_000_020_000 - 870 * 60_000 + (540 + 1440) * 60_000
        );
        assert_eq!(snooze_until(now, 14, 30), 1_000_066_620_000);
        assert_eq!(
            snooze_until(now, 7, 15),
            1_000_000_020_000 - 435 * 60_000 + 540 * 60_000
        );
        let mut cards = vec![c];
        assert!(snooze_card(&mut cards, "i", 2_000));
        assert!(!pulse_visible(&cards[0], 1_999));
        assert!(pulse_visible(&cards[0], 2_000));
    }

    #[test]
    fn closing_a_card_on_the_main_deck_keeps_it_in_pulse() {
        let mut cards = vec![automation_done_card("job", "Backup", "ok", 5)];
        let id = cards[0].id.clone();
        assert!(hide_from_home(&mut cards, &id));
        assert!(cards[0].pulse.off_home);
        assert_eq!(cards[0].status, UpdateStatus::Unread);
        assert!(pulse_visible(&cards[0], 10), "Pulse still shows it");
        assert!(!hide_from_home(&mut cards, &id), "already off the deck");
        assert!(!hide_from_home(&mut cards, "missing"));
        let saved = serde_json::to_string(&cards[0]).unwrap();
        assert!(saved.contains("\"offHome\":true"), "{saved}");
        let back: UpdateCard = serde_json::from_str(&saved).unwrap();
        assert!(back.pulse.off_home);
        let mut fresh = automation_done_card("job2", "Sync", "ok", 6);
        let plain = serde_json::to_string(&fresh).unwrap();
        assert!(!plain.contains("offHome"), "{plain}");
        fresh.status = UpdateStatus::Dismissed;
        let mut gone = vec![fresh];
        let gone_id = gone[0].id.clone();
        assert!(
            !hide_from_home(&mut gone, &gone_id),
            "a dismissed card stays dismissed"
        );
    }

    #[test]
    fn idea_rows_speak_as_i_can_and_the_quiet_digest_reads_on_the_feed() {
        let mut c = idea("a", "Standup note", "", 1);
        c.prompt = Some("Draft my standup from yesterday's commits".into());
        assert_eq!(
            i_can_title(&c),
            "I can draft your standup from yesterday's commits"
        );
        let mut auto = idea("b", "Summarize the workboard", "", 1);
        auto.idea_kind = Some(IdeaKind::Automation);
        assert_eq!(
            i_can_title(&auto),
            "I can summarize the workboard automatically"
        );
        let mut tests = idea("t", "Morning test run", "", 1);
        tests.idea_kind = Some(IdeaKind::Automation);
        tests.prompt =
            Some("every weekday at 8, run cargo test in ~/GrokHub and summarize failures".into());
        assert_eq!(
            i_can_title(&tests),
            "I can run cargo test in ~/GrokHub every weekday at 8"
        );
        let mut board = idea("q", "Summarize the workboard", "", 1);
        board.idea_kind = Some(IdeaKind::Automation);
        board.prompt = Some("every weekday at 9 summarize the workboard".into());
        assert_eq!(
            i_can_title(&board),
            "I can summarize the workboard automatically"
        );
        let mut learn = idea("c", "Release notes", "", 1);
        learn.idea_kind = Some(IdeaKind::Skill);
        assert_eq!(i_can_title(&learn), "I can learn your release notes");
        let mut walks = idea("w", "Turn my evening walks into a weekly habit", "", 1);
        walks.idea_kind = Some(IdeaKind::Skill);
        assert_eq!(
            i_can_title(&walks),
            "I can learn how you turn your evening walks into a weekly habit"
        );
        assert_eq!(
            i_can_title(&idea("d", "I can stage the TXU bill", "", 1)),
            "I can stage the TXU bill"
        );
        // A model line over 70 characters falls back to the action.
        let mut long = idea(
            "l",
            "I can file the open notes from last night's review into the right project folders",
            "",
            1,
        );
        long.prompt = Some("file the open notes from last night's review".into());
        assert_eq!(
            i_can_title(&long),
            "I can file the open notes from last night's review"
        );
        // Nothing ever reads "I can help with …", and nothing runs past 70.
        let mut wordy = idea("v", "Notes", "", 1);
        wordy.prompt = Some(
            "go through every open note from the last two weeks of reviews and file each one"
                .into(),
        );
        assert_eq!(
            i_can_title(&wordy),
            "I can go through every open note from the last two weeks of reviews"
        );
        let mut long_one = idea("o", "Notes", "", 1);
        long_one.prompt = Some(
            "go through every open note from the last two weeks of reviews to file each one".into(),
        );
        let line = i_can_title(&long_one);
        assert_eq!(
            line,
            "I can go through every open note from the last two weeks of reviews…"
        );
        assert_eq!(line.chars().count(), 68);
        for card in [&c, &auto, &tests, &learn, &walks, &long, &wordy, &long_one] {
            let t = i_can_title(card);
            assert!(
                t.starts_with("I can ") && t.chars().count() <= I_CAN_MAX,
                "{t}"
            );
            assert!(!t.contains("help with") && !t.contains('"'), "{t}");
        }
        assert_eq!(snooze_label(8), "Snooze until 9 AM");
        assert_eq!(snooze_label(14), "Snooze until tomorrow 9 AM");
        let mut held = vec![idea("x", "One", "", 1), idea("y", "Two", "", 2)];
        for card in &mut held {
            card.held = true;
        }
        let (_, id) = release_quiet_batch(&mut held, 50);
        let digest = held
            .iter()
            .find(|c| Some(&c.id) == id.as_ref())
            .expect("digest");
        assert!(!is_idea_card(digest) && is_feed_card(digest));
        assert_eq!(pulse_type(digest), PulseType::Watch);
    }

    #[test]
    fn feed_instructions_prompt_rewrite_and_brief_migration() {
        assert_eq!(
            feed_instructions_or_default("  "),
            DEFAULT_FEED_INSTRUCTIONS
        );
        assert_eq!(
            instructions_from_brief("Rust and Linux gaming"),
            format!("{DEFAULT_FEED_INSTRUCTIONS}\n\nFocus: Rust and Linux gaming")
        );
        let prompt = feed_prompt("Rust news", "", "Only Rust.\nNo sports.");
        assert!(!prompt.contains("\n\n\n"), "{prompt}");
        let with = feed_prompt(
            "Rust news",
            "Their interests, strongest first: Rust.",
            "Only Rust.",
        );
        assert!(with.contains("NONE. Do not invent a source, a count, or a fact. Do not offer to send, pay, delete, or publish anything.\n\nTheir interests, strongest first: Rust.\n\nFeed instructions"), "{with}");
        assert!(prompt.ends_with("Feed instructions from the person (follow them; they apply to this and future posts):\nOnly Rust.\nNo sports."), "{prompt}");
        let taste = parse_ledger(
            "- [2026-10-04] pulse: liked \"Rust 1.95 ships\" reason=liked\n- [2026-10-04] pulse: dismissed \"NFL scores\" reason=not-this\n- [2026-10-04] pulse: dismissed \"Old card\" reason=dismiss\n",
        );
        let recent = recent_taste(&taste, 3);
        assert_eq!(recent.len(), 2, "a plain Dismiss is not taste");
        let ask = rewrite_prompt("Only Rust.", &recent);
        assert!(ask.contains("Current instructions:\nOnly Rust.\n\nWhat they did:\n- Liked: Rust 1.95 ships\n- Disliked: NFL scores\n"), "{ask}");
        assert_eq!(
            parse_rewrite("```\nOnly Rust. Show less of: sports scores.\n```").as_deref(),
            Some("Only Rust. Show less of: sports scores.")
        );
        assert_eq!(parse_rewrite("NONE"), None);
        assert_eq!(parse_rewrite("short"), None);
    }

    #[test]
    fn quiet_hours_release_batches_into_one_digest_card() {
        let mut cards: Vec<UpdateCard> = [
            "Backup ran",
            "Price check ran",
            "Brief ready",
            "Repo synced",
        ]
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut c = automation_done_card(&format!("auto-{i}"), t, "ok", 100 + i as u64);
            c.held = true;
            c
        })
        .collect();
        let (n, id) = release_quiet_batch(&mut cards, 9_000);
        assert_eq!((n, id.as_deref()), (4, Some("pulse-quiet-9000")));
        let digest = cards
            .iter()
            .find(|c| c.id == "pulse-quiet-9000")
            .expect("digest");
        assert_eq!(digest.title, "While you were in quiet hours");
        assert_eq!(
            digest.body.as_deref(),
            Some("4 updates: Backup ran · Price check ran · Brief ready · and 1 more")
        );
        assert_eq!(cards.iter().filter(|c| c.held).count(), 0);
        assert_eq!(cards.iter().filter(|c| c.pulse.quiet_batched).count(), 4);
        // Each update keeps its own line and opens its own card.
        let report = digest.pulse.report.clone().expect("report");
        assert_eq!(report.found, "4 updates came in during quiet hours");
        let items: Vec<(&str, &str, &str)> = report
            .items
            .iter()
            .map(|i| (i.card_id.as_str(), i.found.as_str(), i.summary.as_str()))
            .collect();
        assert_eq!(
            items,
            vec![
                ("done-auto-0", "Backup ran", "ok"),
                ("done-auto-1", "Price check ran", "ok"),
                ("done-auto-2", "Brief ready", "ok"),
                ("done-auto-3", "Repo synced", "ok"),
            ]
        );
        let mut one = vec![automation_done_card("solo", "Solo run", "ok", 1)];
        one[0].held = true;
        assert_eq!(release_quiet_batch(&mut one, 9_000), (1, None));
        assert_eq!(one.len(), 1);
        assert!(!one[0].pulse.quiet_batched);
    }

    #[test]
    fn a_dismissed_feed_post_does_not_bury_the_next_one_with_its_title() {
        let ledger =
            parse_ledger("- [2026-10-04] pulse: dismissed \"Nightly backup\" reason=dismiss\n");
        let inputs = PulseInputs {
            ledger: &ledger,
            now_ms: 9_000,
            ..Default::default()
        };
        let tomorrow = automation_done_card("backup", "Nightly backup", "ok", 8_000);
        assert_eq!(pulse_score(&tomorrow, &inputs), 110);
        let posts: Vec<String> = feed_posts(&[tomorrow], &inputs)
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(posts.len(), 1, "{posts:?}");
        // An idea with that title still drops below the line.
        let idea = idea("i", "Nightly backup", "", 10);
        assert_eq!(pulse_score(&idea, &inputs), 150 - 200);
    }

    fn source_post(id: &str, source: &str, at: u64) -> UpdateCard {
        let mut c = automation_done_card(id, id, "", at);
        c.id = id.into();
        c.source_id = format!("source:rss:https://example.test/{id}");
        c.pulse.source_name = Some(source.into());
        c
    }

    #[test]
    fn posts_retire_after_three_days_unless_kept() {
        const DAY: u64 = 24 * 60 * 60 * 1000;
        assert_eq!(POST_TTL_MS, 259_200_000, "3 days");
        let posted = 10 * DAY;
        let old = source_post("old", "r/buildapc", posted);
        assert!(!post_retired(&old, posted + POST_TTL_MS - 1));
        assert!(post_retired(&old, posted + POST_TTL_MS));
        let mut loved = old.clone();
        loved.reaction = Some(crate::update_feed::CardReaction::Up);
        assert!(
            !post_retired(&loved, posted + 30 * DAY),
            "a loved post keeps"
        );
        let mut saved = old.clone();
        saved.feed_kept = true;
        assert!(
            !post_retired(&saved, posted + 30 * DAY),
            "a saved post keeps"
        );
        let mut writer = old.clone();
        writer.source_id = "pulse:writer:1".into();
        assert!(
            post_retired(&writer, posted + POST_TTL_MS),
            "written briefings too"
        );
        let report = automation_done_card("backup", "Nightly backup", "ok", posted);
        assert!(
            !post_retired(&report, posted + 30 * DAY),
            "run reports keep their own rules"
        );
        let inputs = PulseInputs {
            now_ms: posted + POST_TTL_MS,
            ..Default::default()
        };
        let ids: Vec<String> = feed_posts(&[old, loved], &inputs)
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, vec!["old".to_string()], "only the loved copy is left");
    }

    #[test]
    fn plain_why_says_it_in_words() {
        let mut steam = source_post("p", "Helldivers 2 on Steam", 1);
        steam.pulse.because = Some("Helldivers 2".into());
        assert_eq!(plain_why(&steam).as_deref(), Some("You play Helldivers 2"));
        let mut topic = source_post("t", "Hacker News", 1);
        topic.pulse.because = Some("Rust".into());
        assert_eq!(
            plain_why(&topic).as_deref(),
            Some("Matches your interest in Rust")
        );
        let followed = source_post("f", "r/buildapc", 1);
        assert_eq!(
            plain_why(&followed).as_deref(),
            Some("From r/buildapc, a source you follow")
        );
        let mut nameless = source_post("n", "", 1);
        nameless.pulse.source_name = None;
        assert_eq!(
            plain_why(&nameless).as_deref(),
            Some("From a source you follow")
        );
        let report = automation_done_card("backup", "Nightly backup", "ok", 1);
        assert_eq!(plain_why(&report), None);
    }

    #[test]
    fn todays_set_caps_each_source_and_the_total_then_lists_the_rest() {
        const HOUR: u64 = 60 * 60 * 1000;
        let now = 100 * HOUR;
        let mut cards = vec![
            source_post("h1", "Helldivers 2 on Steam", now - HOUR),
            source_post("h2", "Helldivers 2 on Steam", now - 2 * HOUR),
            source_post("h3", "Helldivers 2 on Steam", now - 3 * HOUR),
            source_post("y", "Yesterday's news", now - 30 * HOUR),
        ];
        for i in 0..14 {
            cards.push(source_post(
                &format!("s{i:02}"),
                &format!("site {i}"),
                now - 4 * HOUR - i,
            ));
        }
        let inputs = PulseInputs {
            now_ms: now,
            ..Default::default()
        };
        let (today, older) = feed_day(&cards, &inputs);
        let today: Vec<&str> = today.iter().map(|c| c.id.as_str()).collect();
        let older: Vec<&str> = older.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(TODAY_MAX, 12);
        assert_eq!(
            today,
            vec!["h1", "h2", "s00", "s01", "s02", "s03", "s04", "s05", "s06", "s07", "s08", "s09"]
        );
        assert_eq!(older, vec!["h3", "s10", "s11", "s12", "s13", "y"]);
        assert_eq!(feed_posts(&cards, &inputs).len(), 18, "nothing is lost");
    }

    #[test]
    fn quiet_release_marks_only_held_cards_and_posts_after_a_dismissed_digest() {
        let held = |id: &str, title: &str, at: u64| {
            let mut c = automation_done_card(id, title, "ok", at);
            c.held = true;
            c
        };
        let mut cards = vec![
            automation_done_card("old", "Backup ran", "ok", 10),
            held("a", "Backup ran", 100),
            held("b", "Brief ready", 110),
        ];
        let (_, first) = release_quiet_batch(&mut cards, 1_000);
        let first = first.expect("digest");
        let mut batched: Vec<&str> = cards
            .iter()
            .filter(|c| c.pulse.quiet_batched)
            .map(|c| c.id.as_str())
            .collect();
        batched.sort();
        assert_eq!(
            batched,
            vec!["done-a", "done-b"],
            "an older post with the same title is not batched"
        );
        for c in cards.iter_mut().filter(|c| c.id == first) {
            c.status = UpdateStatus::Dismissed;
        }
        // The next night, inside a day of that dismiss.
        cards.push(held("c", "Repo synced", 2_000));
        cards.push(held("d", "Price check ran", 2_100));
        let (n, second) = release_quiet_batch(&mut cards, 3_000);
        assert_eq!((n, second.as_deref()), (2, Some("pulse-quiet-3000")));
        let digest = cards
            .iter()
            .find(|c| c.id == "pulse-quiet-3000")
            .expect("tonight's digest posts");
        assert_eq!(digest.status, UpdateStatus::Unread);
        assert_eq!(
            digest.body.as_deref(),
            Some("2 updates: Repo synced · Price check ran")
        );
    }

    #[test]
    fn og_image_reads_the_source_page_preview() {
        let html = r#"<html><head><meta charset="utf-8">
<meta name="twitter:image" content="https://cdn.example.com/tw.jpg">
<meta content="/img/lead.jpg?w=1200&amp;q=80" property="og:image" />
</head></html>"#;
        assert_eq!(
            og_image(html, "https://news.example.com/2026/10/story").as_deref(),
            Some("https://news.example.com/img/lead.jpg?w=1200&q=80")
        );
        assert_eq!(
            og_image(
                r#"<meta name="twitter:image" content="https://cdn.example.com/tw.jpg">"#,
                "https://a.example.com/"
            )
            .as_deref(),
            Some("https://cdn.example.com/tw.jpg")
        );
        assert_eq!(
            og_image(
                r#"<meta property="og:image" content="http://localhost/x.png">"#,
                "https://a.example.com/"
            ),
            None
        );
        assert_eq!(og_image("<p>no meta</p>", "https://a.example.com/"), None);
        assert_eq!(
            image_cache_name("https://cdn.example.com/a.JPEG?x=1"),
            format!(
                "{}.jpg",
                &image_cache_name("https://cdn.example.com/a.JPEG?x=1")[..16]
            )
        );
        assert!(image_cache_name("https://cdn.example.com/a").ends_with(".img"));
        assert_eq!(
            source_host("https://www.theverge.com/2026/x").as_deref(),
            Some("theverge.com")
        );
    }

    #[test]
    fn silence_line_and_ago_labels() {
        assert_eq!(
            silence_line(1_000_000, 1_120_000),
            "Last pulse 2m ago. Nothing to do."
        );
        assert_eq!(silence_line(0, 5), "No pulse yet. Nothing to do.");
        assert_eq!(ago_label(0, 3 * 3_600_000 + 5), "3h ago");
    }

    #[test]
    fn apply_heading_follows_pulse_type() {
        assert_eq!(PulseType::Do.apply_heading(), "What Apply will do");
        assert_eq!(
            PulseType::Automate.apply_heading(),
            "What Apply will schedule"
        );
        assert_eq!(PulseType::Learn.apply_heading(), "What I'll learn");
        assert_eq!(PulseType::Watch.apply_heading(), "What Apply will do");
        assert_eq!(
            PULSE_EMPTY,
            "Nothing on Pulse yet. I'll add ideas and posts when I spot something worth doing."
        );
        assert_eq!(IDEAS_LOADING, "Looking for ideas in your recent work…");
        assert_eq!(IDEAS_SIGN_IN, "Sign in to Grok to get ideas.");
        assert_eq!(PulseCategory::More.label(), "More for you");
    }

    #[test]
    fn a_like_lifts_the_same_topic() {
        let card = idea("idea-1", "Watch Helldivers 2 patch notes", "", 40);
        let liked = parse_ledger(&ledger_line(
            "2026-11-06",
            "Watch Helldivers 2 patch notes",
            LedgerReason::Liked,
        ));
        let inputs = PulseInputs {
            ledger: &liked,
            now_ms: 5_000,
            ..Default::default()
        };
        assert_eq!(pulse_score(&card, &inputs), 150 + SCORE_LIKED);
        assert_eq!(SCORE_LIKED, 60);
    }

    #[test]
    fn used_then_dismissed_does_not_score_negative() {
        let idea = idea("idea-1", "Tidy the workboard", "", 40);
        let mut used = idea.clone();
        used.status = UpdateStatus::Opened;
        assert_eq!(dismiss_kind(&used, 0), DismissKind::UsedDone);
        assert!(!dismiss_kind(&used, 0).counts_negative());
        assert_eq!(
            dismiss_kind(&idea, 0),
            DismissKind::NotInterested,
            "unused idea dismiss is not interested"
        );
        assert!(dismiss_kind(&idea, 0).counts_negative());

        let used_line = ledger_line("2026-10-28", "Tidy the workboard", LedgerReason::Used);
        assert_eq!(
            used_line,
            "- [2026-10-28] pulse: used \"Tidy the workboard\" reason=used"
        );
        let used_ledger = parse_ledger(&used_line);
        assert_eq!(used_ledger[0].reason, LedgerReason::Used);
        assert!(!LedgerReason::Used.is_negative_feedback());
        assert_eq!(
            pulse_score(
                &idea,
                &PulseInputs {
                    ledger: &used_ledger,
                    now_ms: 5_000,
                    ..Default::default()
                }
            ),
            150,
            "used+dismissed does not bury the topic"
        );

        let dismiss_ledger =
            parse_ledger("- [2026-10-28] pulse: dismissed \"Tidy the workboard\" reason=dismiss\n");
        assert_eq!(
            pulse_score(
                &idea,
                &PulseInputs {
                    ledger: &dismiss_ledger,
                    now_ms: 5_000,
                    ..Default::default()
                }
            ),
            150 - PENALTY_DISMISS,
            "unused+dismissed stays negative"
        );

        let not_this = parse_ledger(
            "- [2026-10-28] pulse: dismissed \"Tidy the workboard\" reason=not-this\n",
        );
        assert_eq!(
            pulse_score(
                &idea,
                &PulseInputs {
                    ledger: &not_this,
                    now_ms: 5_000,
                    ..Default::default()
                }
            ),
            150 - PENALTY_NOT_THIS,
            "explicit not-this stays negative"
        );
    }

    #[test]
    fn feed_liked_or_discussed_then_dismissed_is_not_negative() {
        let mut feed = digest_card("feed-1", "Rust 1.95 ships", "Notes.", 1_000);
        assert_eq!(dismiss_kind(&feed, 0), DismissKind::NotInterested);
        assert!(dismiss_kind(&feed, 0).counts_negative());

        feed.reaction = Some(CardReaction::Up);
        assert_eq!(dismiss_kind(&feed, 0), DismissKind::UsedDone);
        assert!(!dismiss_kind(&feed, 0).counts_negative());

        let mut discussed = digest_card("feed-2", "NFL scores", "Notes.", 2_000);
        discussed.discuss_thread = Some("thr-1".into());
        assert_eq!(dismiss_kind(&discussed, 0), DismissKind::UsedDone);

        let opened_only = {
            let mut c = digest_card("feed-3", "Nightly backup", "Notes.", 3_000);
            c.status = UpdateStatus::Opened;
            c
        };
        assert_eq!(
            dismiss_kind(&opened_only, 0),
            DismissKind::NotInterested,
            "Feed open without like/discuss still counts as not interested"
        );
    }

    #[test]
    fn the_gallery_takes_the_preview_and_two_large_images_and_skips_logos() {
        let html = r#"<head><meta property="og:image" content="/img/hero.jpg"></head>
<body><img src="/static/logo.png" width="600">
<img src="https://cdn.example.com/a/team.jpg" alt="The team" width="1200" height="800">
<img src="/icons/share.png">
<img src="/img/tiny.jpg" width="40" height="40">
<img src="/img/chart.webp" class="figure">
<img src="data:image/png;base64,AAAA">
<img src="/img/third.jpg" width="900"></body>"#;
        assert_eq!(
            gallery_images(html, "https://news.example.com/2026/story"),
            vec![
                "https://news.example.com/img/hero.jpg".to_string(),
                "https://cdn.example.com/a/team.jpg".to_string(),
                "https://news.example.com/img/chart.webp".to_string(),
            ]
        );
        // No preview: in-page images only, still at most three.
        let plain = r#"<img src="/1.jpg"><img src="/2.jpg"><img src="/3.jpg"><img src="/4.jpg">"#;
        assert_eq!(
            gallery_images(plain, "https://a.example.com/x").len(),
            GALLERY_MAX
        );
        assert!(gallery_images("<p>text</p>", "https://a.example.com/").is_empty());
    }

    #[test]
    fn only_real_mid_sized_image_files_are_cached() {
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpg.resize(IMAGE_MIN_BYTES, 0);
        assert!(image_bytes_ok(&jpg));
        assert!(!image_bytes_ok(&jpg[..IMAGE_MIN_BYTES - 1]), "tiny");
        let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        webp.resize(IMAGE_MIN_BYTES, 0);
        assert!(image_bytes_ok(&webp));
        let mut html = b"<!doctype html>".to_vec();
        html.resize(IMAGE_MIN_BYTES, b' ');
        assert!(!image_bytes_ok(&html));
        let mut huge = vec![0x89, b'P', b'N', b'G'];
        huge.resize(IMAGE_MAX_BYTES + 1, 0);
        assert!(!image_bytes_ok(&huge));
    }

    #[test]
    fn pulse_pings_only_when_time_sensitive_never_in_game_and_three_a_day() {
        let ask = |time_sensitive, game_running, quiet_hours, day| PingAsk {
            time_sensitive,
            game_running,
            quiet_hours,
            day,
        };
        let mut gate = PingGate::default();
        assert!(!gate.take(&ask(false, false, false, "2026-11-06")));
        assert!(!gate.take(&ask(true, true, false, "2026-11-06")));
        assert!(!gate.take(&ask(true, false, true, "2026-11-06")));
        assert_eq!(gate.sent_on("2026-11-06"), 0);
        for _ in 0..3 {
            assert!(gate.take(&ask(true, false, false, "2026-11-06")));
        }
        assert!(!gate.take(&ask(true, false, false, "2026-11-06")));
        assert_eq!(gate.sent_on("2026-11-06"), 3);
        assert!(gate.take(&ask(true, false, false, "2026-11-07")));
        assert_eq!(gate.sent_on("2026-11-07"), 1);
    }
}
