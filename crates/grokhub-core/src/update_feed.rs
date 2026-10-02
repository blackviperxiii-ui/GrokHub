//! Home update feed. Event cards live in `updates.json` until the user opens or dismisses one.
//!
//! An empty event list paints nothing. There is no "No updates" placeholder.
//!
//! Live hook: `Cabin::poll_grok_loop` posts `automation_done` when a scheduled
//! `/loop` returns. A night recipe replay that counts as a finished run posts
//! the same kind from `fire_night`. `Cabin::commit_schedule` posts
//! `schedule_created` when a clock job or interval loop is saved.
//! `suggestion` is the one situation offer from a paused run or a repeated action.
//! Idea cards and the editorial digest are separate writers into the same store.
//! They do not consume the event paint cap. Interest learning and `interest_update`
//! are out of scope.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Newest event cards painted in the home slot. Older undismissed event cards stay on disk.
pub const FEED_PAINT_MAX: usize = 3;
/// One day. Content dedupe and the muted-failure floor use this window.
pub const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// "Less like this" hides a topic or source for two weeks. Phase 1b scores it.
pub const LESS_MUTE_MS: u64 = 14 * DAY_MS;
/// Idea cards pinned on the home feed. The Ideas board keeps the rest, up to `IDEA_BOARD_MAX`.
pub const IDEA_DISCOVERY_MAX: usize = 3;
/// Ideas on the board you have not worked on. A newer card pushes out the oldest.
pub const IDEA_BOARD_MAX: usize = 15;
/// Ideas you changed (edited the action or talked about). They stay until you delete
/// or apply them, never pushed out, and do not count toward `IDEA_BOARD_MAX`.
pub const IDEA_MODIFIED_MAX: usize = 10;
/// Digest cards painted beside the event slot. They do not consume `FEED_PAINT_MAX`.
pub const DIGEST_PAINT_MAX: usize = 2;
const FEED_STORE_MAX: usize = 40;
const TITLE_CHARS: usize = 72;
const BODY_CHARS: usize = 160;
const DIGEST_BODY_CHARS: usize = 900;
/// A paused run has to sit this long before the situation card offers to resume it.
pub const PAUSE_OFFER_MS: u64 = 30 * 60 * 1000;
/// Workboard detail written by `abandon_inflight_card` when a run is parked.
const PAUSED_DETAIL: &str = "Paused. This is where to resume.";
const HONEST_EMPTY: &str = "I looked and did not find a source worth your time.";
const STEER_LINE: &str = "The brief steers the next edition.";
const IDEA_TITLE_MEMORY: usize = 64;
const TURNED_DOWN_MEMORY: usize = 64;

/// About two weeks. An explicit `expires_at` wins when it is set.
pub const IDEA_TTL_MS: u64 = 14 * 24 * 60 * 60 * 1000;

/// Background cadence. Not a settings panel. Housekeep is the only caller.
pub const DEFAULT_EXPIRY_SWEEP_MS: u64 = 60 * 60 * 1000;
pub const DEFAULT_QUIET_RELEASE_MS: u64 = 15_000;
pub const DEFAULT_DIGEST_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateKind {
    AutomationDone,
    ScheduleCreated,
    Suggestion,
    AutomateOffer,
    Idea,
    Digest,
}

impl UpdateKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::AutomationDone => "Automation",
            Self::ScheduleCreated => "Scheduled",
            Self::Suggestion => "Suggestion",
            Self::AutomateOffer => "Automate",
            Self::Idea => "Idea",
            Self::Digest => "Digest",
        }
    }

    pub fn event(self) -> bool {
        matches!(
            self,
            Self::AutomationDone | Self::ScheduleCreated | Self::Suggestion | Self::AutomateOffer
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    Unread,
    Opened,
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardReaction {
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpdateAction {
    OpenSession { thread_id: String },
    OpenWorkboard,
    OpenAutomations,
    DeepLink { href: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCard {
    pub id: String,
    pub kind: UpdateKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub created_at: u64,
    pub status: UpdateStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<UpdateAction>,
    /// Idea cards. Event cards ignore this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// Quiet hours recorded the card and have not released it yet.
    #[serde(default, skip_serializing_if = "is_false")]
    pub held: bool,
    /// URLs a research step actually returned. Never invented.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction: Option<CardReaction>,
    /// Discuss thread. User lines there are the only chat taste for this post.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discuss_thread: Option<String>,
    /// Idea Accept filed a workboard Todo.
    #[serde(default, skip_serializing_if = "is_false")]
    pub built: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Home feed is showing this idea. At most `IDEA_DISCOVERY_MAX` are pinned, once.
    #[serde(default, skip_serializing_if = "is_false")]
    pub feed_pin: bool,
    /// Taken off the home feed. The slot stays empty. The card remains on the Ideas board.
    #[serde(default, skip_serializing_if = "is_false")]
    pub feed_kept: bool,
    /// Generated ideas: the exact message that does it. Accept fills the draft with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Idea type: automation, reminder, skill, or try (shown as Suggestion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idea_kind: Option<crate::ideas::IdeaKind>,
    /// Full explanation shown when the card opens. `body` stays one short line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// Your edited action, kept until you apply it. `None` means the original `prompt`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<String>,
    /// You worked on this card. It stays until deleted and is never pushed out.
    #[serde(default, skip_serializing_if = "is_false")]
    pub modified: bool,
    /// Skill ideas from the nightly review: the skill Apply saves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<crate::review::LearnedSuggestion>,
    /// Stable id for an idea or situation offer. A dismissed one does not return.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_id: String,
    /// Runs this one card stands in for. Old feeds load as a single run.
    #[serde(default = "default_runs", skip_serializing_if = "runs_is_default")]
    pub runs: u32,
    /// First post in the group. `0` on cards saved before grouping.
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub first_at: u64,
    /// Folded away by `collapse_feed`. Out of Home, learning, and archives.
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
}

impl UpdateCard {
    /// Type shown first on an idea: Automation, Skill, or Suggestion.
    pub fn idea_type_label(&self) -> &'static str {
        if self.skill.is_some() {
            return "Skill";
        }
        self.idea_kind
            .map(crate::ideas::IdeaKind::type_label)
            .unwrap_or("Suggestion")
    }

    /// What Apply does: your draft if you changed it, else the original action
    /// (a Skill idea's steps).
    pub fn idea_action(&self) -> String {
        self.draft
            .as_deref()
            .or(self.prompt.as_deref())
            .or(self.skill.as_ref().and_then(|k| k.instructions.as_deref()))
            .map(str::trim)
            .unwrap_or("")
            .to_string()
    }
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn default_runs() -> u32 {
    1
}

fn runs_is_default(v: &u32) -> bool {
    *v == 1
}

fn u64_is_zero(v: &u64) -> bool {
    *v == 0
}

/// A topic or source hidden by "Less like this" until `until_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedMute {
    pub key: String,
    pub until_ms: u64,
}

/// One persisted config for the three feed passes. Default cadence stays background.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedPulse {
    #[serde(default = "default_on")]
    pub expiry_on: bool,
    #[serde(default = "default_expiry_ms")]
    pub expiry_ms: u64,
    #[serde(default = "default_on")]
    pub quiet_release_on: bool,
    #[serde(default = "default_quiet_release_ms")]
    pub quiet_release_ms: u64,
    #[serde(default = "default_on")]
    pub digest_on: bool,
    #[serde(default = "default_digest_ms")]
    pub digest_ms: u64,
    #[serde(default)]
    pub last_expiry_ms: u64,
    #[serde(default)]
    pub last_quiet_release_ms: u64,
    #[serde(default)]
    pub last_digest_ms: u64,
    /// The digest clock fired during quiet and has not posted yet.
    #[serde(default)]
    pub digest_held: bool,
    /// Titles already emitted. The generator does not recreate or delete them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub idea_titles: Vec<String>,
    /// Home-feed idea slots already used. Dismissing one does not free a slot.
    #[serde(default)]
    pub feed_slots_spent: u8,
    /// Last time the model was asked for ideas (`crate::ideas`).
    #[serde(default)]
    pub last_ideas_ms: u64,
    /// Idea and situation sources the user dismissed. They do not come back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dismissed_sources: Vec<String>,
    /// Idea titles the user deleted or took off the home feed, oldest first. A later
    /// idea on the same topic is not posted, and the prompts list these as turned down.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turned_down: Vec<String>,
    /// First time a paused job was seen, so the offer waits out `PAUSE_OFFER_MS`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub paused_seen: BTreeMap<String, u64>,
    /// Automation and loop ids hidden from Home until Undo. The workboard still updates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub muted_sources: Vec<String>,
    /// 14-day "Less like this" keys: group, `src:<id>`, `topic:<token>`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub less_mutes: Vec<FeedMute>,
    /// Last time a muted source was allowed to show a failure.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub muted_fail_at: BTreeMap<String, u64>,
}

fn default_on() -> bool {
    true
}

fn default_expiry_ms() -> u64 {
    DEFAULT_EXPIRY_SWEEP_MS
}

fn default_quiet_release_ms() -> u64 {
    DEFAULT_QUIET_RELEASE_MS
}

fn default_digest_ms() -> u64 {
    DEFAULT_DIGEST_MS
}

impl Default for FeedPulse {
    fn default() -> Self {
        Self {
            expiry_on: true,
            expiry_ms: DEFAULT_EXPIRY_SWEEP_MS,
            quiet_release_on: true,
            quiet_release_ms: DEFAULT_QUIET_RELEASE_MS,
            digest_on: true,
            digest_ms: DEFAULT_DIGEST_MS,
            last_expiry_ms: 0,
            last_quiet_release_ms: 0,
            last_digest_ms: 0,
            digest_held: false,
            idea_titles: Vec::new(),
            feed_slots_spent: 0,
            last_ideas_ms: 0,
            dismissed_sources: Vec::new(),
            turned_down: Vec::new(),
            paused_seen: BTreeMap::new(),
            muted_sources: Vec::new(),
            less_mutes: Vec::new(),
            muted_fail_at: BTreeMap::new(),
        }
    }
}

impl FeedPulse {
    pub fn is_background_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitedLink {
    pub url: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TasteNote {
    pub title: String,
    pub reaction: Option<CardReaction>,
    pub said: String,
}

#[derive(Debug, Clone, Copy)]
pub struct PulseNow {
    pub now_ms: u64,
    pub quiet: bool,
}

/// Memory, the brief, returned links, and explicit taste. No transcript dump.
#[derive(Debug, Clone, Copy)]
pub struct DigestMaterial<'a> {
    pub brief: &'a str,
    pub user_md: &'a str,
    pub memory_md: &'a str,
    pub soul_md: &'a str,
    pub links: &'a [CitedLink],
    pub taste: &'a [TasteNote],
    /// Written edition from the daily lookup. Absent means the lookup has not run.
    pub edition: Option<DigestEdition<'a>>,
}

/// One lookup result. Citations still come only from `DigestMaterial::links`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigestEdition<'a> {
    pub found: bool,
    pub refused: bool,
    pub title: &'a str,
    pub body: &'a str,
}

/// A workboard run parked with the pause sentence.
#[derive(Debug, Clone, Copy)]
pub struct PausedJob<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub detail: &'a str,
}

/// A chip action the person keeps taking.
#[derive(Debug, Clone, Copy)]
pub struct RepeatedAction<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub count: u32,
    pub dismissed: bool,
    pub automated: bool,
}

/// What `post_help` added this pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpTick {
    pub ideas: usize,
    pub posted: bool,
    pub ping: bool,
}

/// Parsed daily lookup. `found` is false when there is no real URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLookup {
    pub title: String,
    pub body: String,
    pub found: bool,
    pub refused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PulseTick {
    pub expired: usize,
    pub released: usize,
    pub digest_posted: bool,
    pub digest_held: bool,
    pub cards_changed: bool,
    pub pulse_changed: bool,
    /// The digest is due and no edition is attached. The caller looks it up.
    pub digest_needs_lookup: bool,
    /// An edition was attached, so the caller can drop the pending lookup text.
    pub digest_consumed: bool,
}

/// True when the home slot should paint. Held and dismissed cards do not count.
/// Ideas and digest cards can open the slot without entering the event cap.
pub fn feed_visible(cards: &[UpdateCard]) -> bool {
    home_feed_n(cards) > 0
}

/// Event cards only, newest first. Ideas and digest cards are not in this list,
/// so they cannot consume the paint cap of 3.
pub fn visible_updates(cards: &[UpdateCard]) -> Vec<UpdateCard> {
    visible_kind(cards, UpdateKind::event)
}

pub fn visible_ideas(cards: &[UpdateCard]) -> Vec<UpdateCard> {
    visible_kind(cards, |k| k == UpdateKind::Idea)
}

pub fn visible_digests(cards: &[UpdateCard]) -> Vec<UpdateCard> {
    visible_kind(cards, |k| k == UpdateKind::Digest)
}

pub fn archived_digests(cards: &[UpdateCard]) -> Vec<UpdateCard> {
    let mut out: Vec<UpdateCard> = cards
        .iter()
        .filter(|c| {
            c.kind == UpdateKind::Digest && c.status == UpdateStatus::Dismissed && !c.collapsed
        })
        .cloned()
        .collect();
    sort_feed(&mut out);
    out
}

pub fn home_feed_n(cards: &[UpdateCard]) -> usize {
    visible_updates(cards).len().min(FEED_PAINT_MAX)
        + feed_ideas(cards, 0).len()
        + visible_digests(cards).len().min(DIGEST_PAINT_MAX)
}

/// Ideas currently on the home feed, highest rank first. Dismissed slots are not refilled.
pub fn feed_ideas(cards: &[UpdateCard], now: u64) -> Vec<UpdateCard> {
    let mut out: Vec<UpdateCard> = cards
        .iter()
        .filter(|c| c.kind == UpdateKind::Idea && c.feed_pin && surfaced(c))
        .cloned()
        .collect();
    let _ = now;
    out.sort_by_key(|card| std::cmp::Reverse(card.created_at));
    out.truncate(IDEA_DISCOVERY_MAX);
    out
}

/// The user opened, discussed, built, or reacted. Untouched cards age down.
pub fn idea_touched(card: &UpdateCard) -> bool {
    card.modified
        || card.built
        || card.discuss_thread.is_some()
        || card.reaction.is_some()
        || card.status == UpdateStatus::Opened
        || card.feed_kept
}

/// Higher is more worth keeping. Untouched cards sink as they age.
/// The ideas engine sinks a card it was told not to suggest, and lifts one it was told to rank.
pub fn lesson_rank_delta(title: &str, lessons: &str) -> i64 {
    if setup_blocked_by_lessons(title, lessons) {
        return -800;
    }
    let mut bonus = 0i64;
    for line in lessons.lines() {
        let line_l = line.to_ascii_lowercase();
        if line_l.contains("rank ") && shares_word(title, line) {
            bonus += 200;
        }
    }
    bonus
}

fn shares_word(title: &str, line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    title.to_ascii_lowercase().split_whitespace().any(|word| {
        let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        word.len() >= 4 && line.contains(word)
    })
}

/// Ideas directives, and older skip lines, keep a matching setup off the board.
pub fn setup_blocked_by_lessons(title: &str, lessons: &str) -> bool {
    let lessons_l = lessons.to_ascii_lowercase();
    if lessons_l.is_empty() {
        return false;
    }
    let title_l = title.to_ascii_lowercase();
    let skip = |topic: &str| lessons_l.contains(&format!("skip {topic}"));
    if (skip("night") && (title_l.contains("night") || title_l.contains("wrap")))
        || (skip("imagine") && title_l.contains("imagine"))
        || (skip("the tour") && (title_l.contains("start here") || title_l.contains("what you can")))
    {
        return true;
    }
    lessons.lines().any(|line| {
        line.to_ascii_lowercase().contains("do not suggest") && shares_word(title, line)
    })
}

pub fn idea_rank(card: &UpdateCard, now: u64) -> i64 {
    let mut score: i64 = 100;
    if idea_touched(card) {
        score += 1_000;
    } else {
        let hours = now.saturating_sub(card.created_at) / 3_600_000;
        score -= hours as i64;
    }
    match card.reaction {
        Some(CardReaction::Up) => score += 250,
        Some(CardReaction::Down) => score -= 400,
        None => {}
    }
    score
}

/// Pin up to three ideas the first time the board has any. Later cards stay on Ideas only.
pub fn seal_feed_ideas(cards: &mut [UpdateCard], pulse: &mut FeedPulse, now: u64) {
    if pulse.feed_slots_spent > 0 {
        return;
    }
    let mut idxs: Vec<usize> = cards
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind == UpdateKind::Idea && surfaced(c) && !c.feed_kept)
        .map(|(i, _)| i)
        .collect();
    idxs.sort_by(|&a, &b| idea_rank(&cards[b], now).cmp(&idea_rank(&cards[a], now)));
    let n = idxs.len().min(IDEA_DISCOVERY_MAX);
    for i in idxs.into_iter().take(n) {
        cards[i].feed_pin = true;
    }
    if n > 0 {
        pulse.feed_slots_spent = n as u8;
    }
}

/// Take one idea off the home feed and leave it on the Ideas board. No replacement.
pub fn unpin_feed_idea(cards: &mut [UpdateCard], id: &str) -> bool {
    let Some(card) = cards
        .iter_mut()
        .find(|c| c.id == id && c.kind == UpdateKind::Idea && c.feed_pin)
    else {
        return false;
    };
    card.feed_pin = false;
    card.feed_kept = true;
    true
}

fn visible_kind(cards: &[UpdateCard], kind: impl Fn(UpdateKind) -> bool) -> Vec<UpdateCard> {
    let mut out: Vec<UpdateCard> = cards
        .iter()
        .filter(|c| kind(c.kind) && surfaced(c))
        .cloned()
        .collect();
    sort_feed(&mut out);
    out
}

fn surfaced(card: &UpdateCard) -> bool {
    card.status != UpdateStatus::Dismissed && !card.held
}

fn sort_feed(cards: &mut [UpdateCard]) {
    cards.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Lowercase, collapse whitespace, drop digits and clock times (`9:05`, `09:00 pm`).
fn normalize_feed_text(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let mut kept = String::with_capacity(lower.len());
    let mut rest = lower.as_str();
    while !rest.is_empty() {
        if let Some(n) = clock_prefix_len(rest) {
            rest = &rest[n..];
            kept.push(' ');
            continue;
        }
        let ch = rest.chars().next().unwrap_or(' ');
        let n = ch.len_utf8();
        if !ch.is_ascii_digit() {
            kept.push(ch);
        }
        rest = &rest[n..];
    }
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clock_prefix_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
        return None;
    }
    i += 1;
    if i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b':' {
        return None;
    }
    i += 1;
    if i + 1 >= bytes.len() || !bytes[i].is_ascii_digit() || !bytes[i + 1].is_ascii_digit() {
        return None;
    }
    i += 2;
    let mut j = i;
    if j < bytes.len() && bytes[j] == b' ' {
        j += 1;
    }
    if let Some(end) = meridiem_end(bytes, j) {
        return Some(end);
    }
    if i == bytes.len() || !bytes[i].is_ascii_alphanumeric() {
        return Some(i);
    }
    None
}

fn meridiem_end(bytes: &[u8], j: usize) -> Option<usize> {
    if j + 1 >= bytes.len() || (bytes[j] != b'a' && bytes[j] != b'p') {
        return None;
    }
    let end = if j + 3 < bytes.len() && bytes[j + 1] == b'.' && bytes[j + 2] == b'm' && bytes[j + 3] == b'.'
    {
        j + 4
    } else if bytes[j + 1] == b'm' {
        j + 2
    } else {
        return None;
    };
    if end == bytes.len() || !bytes[end].is_ascii_alphanumeric() {
        Some(end)
    } else {
        None
    }
}

/// Group key for an event card. Ideas and digests are not grouped.
pub fn group_key(card: &UpdateCard) -> Option<String> {
    let prefix = match card.kind {
        UpdateKind::AutomationDone => "run",
        UpdateKind::ScheduleCreated => "sched",
        UpdateKind::AutomateOffer => "offer",
        UpdateKind::Suggestion => "sugg",
        UpdateKind::Idea | UpdateKind::Digest => return None,
    };
    let source = card.source_id.trim();
    if source.is_empty() {
        let title = normalize_feed_text(&card.title);
        Some(format!("{prefix}:{:016x}", fnv1a64(title.as_bytes())))
    } else {
        Some(format!("{prefix}:{source}"))
    }
}

fn content_key(card: &UpdateCard) -> String {
    let body = card.body.as_deref().unwrap_or("");
    normalize_feed_text(&format!("{} {body}", card.title))
}

fn content_duplicate(cards: &[UpdateCard], card: &UpdateCard) -> bool {
    let key = content_key(card);
    if key.is_empty() {
        return false;
    }
    cards.iter().any(|prev| {
        prev.created_at.abs_diff(card.created_at) < DAY_MS && content_key(prev) == key
    })
}

/// True when the title is a failed automation, including after a group merge kept the old id.
pub fn card_is_failure(card: &UpdateCard) -> bool {
    card.kind == UpdateKind::AutomationDone
        && (card.title == "Automation failed" || card.title.ends_with(" failed"))
}

fn failure_name(title: &str) -> String {
    if title == "Automation failed" {
        return "Automation".into();
    }
    let name = title.strip_suffix(" failed").unwrap_or(title).trim();
    if name.is_empty() {
        "Automation".into()
    } else {
        name.to_string()
    }
}

/// Fixed why line for run, failure, and schedule cards. Other kinds keep their own `why`.
pub fn event_why_line(card: &UpdateCard) -> Option<String> {
    match card.kind {
        UpdateKind::AutomationDone if card_is_failure(card) => {
            Some(format!("“{}” failed and needs a look.", failure_name(&card.title)))
        }
        UpdateKind::AutomationDone => {
            let mut line = format!(
                "Your automation “{}” finished and left a report in Follow up.",
                card.title.trim()
            );
            if card.runs > 1 {
                line.push_str(&format!(" · {} runs since you last looked", card.runs));
            }
            Some(line)
        }
        UpdateKind::ScheduleCreated => Some("You saved this schedule.".into()),
        _ => None,
    }
}

fn apply_stored_why(card: &mut UpdateCard) {
    if let Some(line) = event_why_line(card) {
        card.why = Some(line);
    }
}

/// `×N runs · latest h:mm` for a card that has repeated. `hour` and `minute` are the local clock now.
pub fn runs_latest_line(
    runs: u32,
    created_at: u64,
    now_ms: u64,
    hour: u32,
    minute: u32,
) -> Option<String> {
    if runs <= 1 {
        return None;
    }
    let now_mins = i64::from(hour.min(23)) * 60 + i64::from(minute.min(59));
    let delta_min = now_ms.saturating_sub(created_at) / 60_000;
    let delta_min = i64::try_from(delta_min).unwrap_or(i64::MAX / 4);
    let mins = (now_mins - delta_min).rem_euclid(24 * 60);
    Some(format!(
        "×{runs} runs · latest {:02}:{:02}",
        mins / 60,
        mins % 60
    ))
}

/// Up to five topic tokens from a title. Same stemmer as idea topics.
pub fn topic_tokens(title: &str) -> Vec<String> {
    crate::ideas::topic_words(title).into_iter().take(5).collect()
}

/// On load, fold each event group down to its newest undismissed card.
/// Returns true when a card changed, so the caller can write `updates.json` once.
pub fn collapse_feed(cards: &mut [UpdateCard]) -> bool {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, card) in cards.iter().enumerate() {
        if card.collapsed {
            continue;
        }
        if let Some(key) = group_key(card) {
            groups.entry(key).or_default().push(i);
        }
    }
    let mut changed = false;
    for idxs in groups.into_values() {
        if idxs.len() < 2 {
            continue;
        }
        let undismissed: Vec<usize> = idxs
            .iter()
            .copied()
            .filter(|&i| cards[i].status != UpdateStatus::Dismissed)
            .collect();
        let pool: Vec<usize> = if undismissed.is_empty() {
            idxs.clone()
        } else {
            undismissed
        };
        let keeper = pool
            .iter()
            .copied()
            .max_by(|&a, &b| {
                cards[a]
                    .created_at
                    .cmp(&cards[b].created_at)
                    .then(cards[a].id.cmp(&cards[b].id))
            })
            .unwrap_or(idxs[0]);
        let runs = idxs.iter().fold(0u32, |acc, &i| {
            acc.saturating_add(cards[i].runs.max(1))
        });
        let first_at = idxs
            .iter()
            .map(|&i| {
                if cards[i].first_at == 0 {
                    cards[i].created_at
                } else {
                    cards[i].first_at
                }
            })
            .min()
            .unwrap_or(cards[keeper].created_at);
        let board_id = cards[keeper].board_id.clone().or_else(|| {
            idxs.iter()
                .copied()
                .filter(|&i| cards[i].board_id.is_some())
                .max_by(|&a, &b| {
                    cards[a]
                        .created_at
                        .cmp(&cards[b].created_at)
                        .then(cards[a].id.cmp(&cards[b].id))
                })
                .and_then(|i| cards[i].board_id.clone())
        });
        let discuss = cards[keeper].discuss_thread.clone().or_else(|| {
            idxs.iter()
                .copied()
                .find_map(|i| cards[i].discuss_thread.clone())
        });
        for &i in &idxs {
            if i == keeper {
                continue;
            }
            cards[i].status = UpdateStatus::Dismissed;
            cards[i].collapsed = true;
            cards[i].held = false;
        }
        cards[keeper].runs = runs;
        if cards[keeper].first_at == 0 || first_at < cards[keeper].first_at {
            cards[keeper].first_at = first_at;
        }
        if cards[keeper].board_id.is_none() {
            cards[keeper].board_id = board_id;
        }
        if cards[keeper].discuss_thread.is_none() {
            cards[keeper].discuss_thread = discuss;
        }
        apply_stored_why(&mut cards[keeper]);
        changed = true;
    }
    changed
}

/// Dismiss every undismissed card in the same group. They stay on disk so a repeat
/// within a day does not come back as a new row. `collapsed` stays false: this is
/// the user, not the migration fold.
pub fn dismiss_feed_group(cards: &mut [UpdateCard], id: &str) -> bool {
    let Some(key) = cards.iter().find(|c| c.id == id).and_then(group_key) else {
        return false;
    };
    let mut hit = false;
    for card in cards.iter_mut() {
        if card.collapsed || card.status == UpdateStatus::Dismissed {
            continue;
        }
        if group_key(card).as_deref() == Some(key.as_str()) {
            card.status = UpdateStatus::Dismissed;
            card.held = false;
            hit = true;
        }
    }
    hit
}

pub fn source_muted(pulse: &FeedPulse, source: &str) -> bool {
    let source = source.trim();
    !source.is_empty() && pulse.muted_sources.iter().any(|saved| saved == source)
}

pub fn mute_home_source(pulse: &mut FeedPulse, source: &str) {
    let source = source.trim();
    if source.is_empty() || source_muted(pulse, source) {
        return;
    }
    pulse.muted_sources.push(source.to_string());
}

pub fn unmute_home_source(pulse: &mut FeedPulse, source: &str) {
    let source = source.trim();
    pulse.muted_sources.retain(|saved| saved != source);
    pulse.muted_fail_at.remove(source);
}

fn mute_row_active(pulse: &FeedPulse, key: &str, now: u64) -> bool {
    pulse
        .less_mutes
        .iter()
        .any(|row| row.key == key && row.until_ms > now)
}

pub fn less_blocks(pulse: &FeedPulse, card: &UpdateCard, now: u64) -> bool {
    if let Some(key) = group_key(card) {
        if mute_row_active(pulse, &key, now) {
            return true;
        }
    }
    let source = card.source_id.trim();
    if !source.is_empty() && mute_row_active(pulse, &format!("src:{source}"), now) {
        return true;
    }
    topic_tokens(&card.title)
        .iter()
        .any(|token| mute_row_active(pulse, &format!("topic:{token}"), now))
}

pub fn mute_less_like(pulse: &mut FeedPulse, card: &UpdateCard, now: u64) {
    pulse.less_mutes.retain(|row| row.until_ms > now);
    let until = now.saturating_add(LESS_MUTE_MS);
    let mut keys = Vec::new();
    if let Some(key) = group_key(card) {
        keys.push(key);
    }
    let source = card.source_id.trim();
    if !source.is_empty() {
        keys.push(format!("src:{source}"));
    }
    for token in topic_tokens(&card.title) {
        keys.push(format!("topic:{token}"));
    }
    for key in keys {
        if let Some(row) = pulse.less_mutes.iter_mut().find(|row| row.key == key) {
            row.until_ms = row.until_ms.max(until);
        } else {
            pulse.less_mutes.push(FeedMute { key, until_ms: until });
        }
    }
}

pub fn muted_failure_allowed(pulse: &FeedPulse, source: &str, now: u64) -> bool {
    match pulse.muted_fail_at.get(source.trim()) {
        None => true,
        Some(&at) => now.saturating_sub(at) >= DAY_MS,
    }
}

pub fn note_muted_failure(pulse: &mut FeedPulse, source: &str, now: u64) {
    let source = source.trim();
    if source.is_empty() {
        return;
    }
    pulse.muted_fail_at.insert(source.to_string(), now);
}

/// What `post_feed_card` should do with a card before it hits the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeGate {
    pub skip: bool,
    /// The caller records `now` so the next muted failure waits a day.
    pub stamp_failure: bool,
}

pub fn home_post_gate(pulse: &FeedPulse, card: &UpdateCard, now: u64) -> HomeGate {
    let source = card.source_id.trim();
    let hard = !source.is_empty() && source_muted(pulse, source);
    let less = less_blocks(pulse, card, now);
    if !hard && !less {
        return HomeGate {
            skip: false,
            stamp_failure: false,
        };
    }
    if card_is_failure(card) && !source.is_empty() && muted_failure_allowed(pulse, source, now) {
        return HomeGate {
            skip: false,
            stamp_failure: true,
        };
    }
    HomeGate {
        skip: true,
        stamp_failure: false,
    }
}

pub fn post_update(cards: &mut Vec<UpdateCard>, mut card: UpdateCard) {
    if card.runs == 0 {
        card.runs = 1;
    }
    if card.first_at == 0 {
        card.first_at = card.created_at;
    }
    if card.kind.event() {
        if let Some(key) = group_key(&card) {
            if let Some(idx) = cards.iter().position(|prev| {
                !prev.collapsed
                    && prev.status != UpdateStatus::Dismissed
                    && group_key(prev).as_deref() == Some(key.as_str())
            }) {
                merge_group_card(cards, idx, card);
                trim_feed_store(cards);
                return;
            }
        }
        if content_duplicate(cards, &card) {
            return;
        }
    }
    apply_stored_why(&mut card);
    let id = card.id.clone();
    cards.retain(|c| c.id != id);
    cards.push(card);
    trim_feed_store(cards);
}

fn merge_group_card(cards: &mut [UpdateCard], idx: usize, mut card: UpdateCard) {
    let prev = &cards[idx];
    let runs = prev.runs.max(1).saturating_add(1);
    let first_at = if prev.first_at == 0 {
        prev.created_at.min(card.created_at)
    } else {
        prev.first_at
    };
    card.id = prev.id.clone();
    card.runs = runs;
    card.first_at = first_at;
    card.board_id = card.board_id.clone().or_else(|| prev.board_id.clone());
    card.discuss_thread = prev.discuss_thread.clone().or(card.discuss_thread.clone());
    card.reaction = card.reaction.or(prev.reaction);
    if card.citations.is_empty() {
        card.citations = prev.citations.clone();
    }
    card.status = UpdateStatus::Unread;
    card.collapsed = false;
    apply_stored_why(&mut card);
    cards[idx] = card;
}

fn trim_feed_store(cards: &mut Vec<UpdateCard>) {
    let mut ideas = Vec::new();
    let mut archive = Vec::new();
    let mut live = Vec::new();
    let mut folded = Vec::new();
    for card in cards.drain(..) {
        if card.kind == UpdateKind::Idea {
            ideas.push(card);
        } else if card.kind == UpdateKind::Digest && card.status == UpdateStatus::Dismissed && !card.collapsed
        {
            archive.push(card);
        } else if card.status == UpdateStatus::Dismissed || card.collapsed {
            folded.push(card);
        } else {
            live.push(card);
        }
    }
    sort_feed(&mut live);
    sort_feed(&mut archive);
    sort_feed(&mut ideas);
    sort_feed(&mut folded);
    if live.len() > FEED_STORE_MAX {
        live.truncate(FEED_STORE_MAX);
    }
    if archive.len() > FEED_STORE_MAX {
        archive.truncate(FEED_STORE_MAX);
    }
    if folded.len() > FEED_STORE_MAX {
        folded.truncate(FEED_STORE_MAX);
    }
    cards.extend(live);
    cards.extend(folded);
    cards.extend(archive);
    cards.extend(ideas);
    sort_feed(cards);
}

/// Open keeps the card. A later dismiss is what drops an event card.
pub fn mark_update_opened(cards: &mut [UpdateCard], id: &str) -> bool {
    let Some(card) = cards
        .iter_mut()
        .find(|c| c.id == id && c.status != UpdateStatus::Dismissed)
    else {
        return false;
    };
    card.status = UpdateStatus::Opened;
    true
}

pub fn dismiss_update(cards: &mut Vec<UpdateCard>, id: &str) -> bool {
    let before = cards.len();
    cards.retain(|c| c.id != id);
    cards.len() != before
}

/// User kill for an idea. The generator has no dismiss tool.
pub fn dismiss_idea(cards: &mut Vec<UpdateCard>, id: &str) -> bool {
    let Some(pos) = cards
        .iter()
        .position(|c| c.id == id && c.kind == UpdateKind::Idea)
    else {
        return false;
    };
    cards.remove(pos);
    true
}

/// Delete on a digest archives the row. It stays searchable and is not an event.
pub fn archive_digest(cards: &mut [UpdateCard], id: &str) -> bool {
    let Some(card) = cards.iter_mut().find(|c| {
        c.id == id && c.kind == UpdateKind::Digest && c.status != UpdateStatus::Dismissed
    }) else {
        return false;
    };
    card.status = UpdateStatus::Dismissed;
    card.held = false;
    true
}

pub fn expire_ideas(cards: &mut Vec<UpdateCard>, now: u64) -> usize {
    let before = cards.len();
    cards.retain(|c| !idea_expired(c, now));
    before - cards.len()
}

/// Ideas do not time out: a newer card pushes out the oldest unmodified one
/// (`IDEA_BOARD_MAX`), and a modified one stays until you delete it. Only a card
/// with an explicit `expires_at` (older stores) still expires.
pub fn idea_expired(card: &UpdateCard, now: u64) -> bool {
    if card.kind != UpdateKind::Idea || card.modified || card.idea_kind.is_some() || card.skill.is_some() {
        return false;
    }
    card.expires_at.is_some_and(|at| now >= at)
}

pub fn release_quiet_hold(cards: &mut [UpdateCard]) -> usize {
    let mut n = 0;
    for card in cards.iter_mut() {
        if card.held {
            card.held = false;
            n += 1;
        }
    }
    n
}

pub fn hold_if_quiet(card: &mut UpdateCard, quiet: bool) {
    if quiet
        && matches!(
            card.kind,
            UpdateKind::AutomationDone | UpdateKind::Idea | UpdateKind::Digest | UpdateKind::Suggestion
        )
    {
        card.held = true;
    }
}

/// Fresh empty home after minimize or close. A scratch tab or a transcript needs a new chat.
/// An already-empty non-scratch chat is the feed surface; do not stack another one.
pub fn resume_needs_fresh_chat(message_count: usize, scratch: bool) -> bool {
    message_count > 0 || scratch
}

pub fn card_matches(card: &UpdateCard, query: &str) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    card.title.to_ascii_lowercase().contains(&q)
        || card
            .body
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains(&q)
}

pub fn discuss_context(card: &UpdateCard) -> String {
    let mut out = format!("Post: {}", card.title);
    if let Some(body) = card.body.as_deref() {
        if !body.is_empty() {
            out.push('\n');
            out.push_str(body);
        }
    }
    if let Some(why) = card.why.as_deref() {
        if !why.is_empty() {
            out.push_str("\nWhy: ");
            out.push_str(why);
        }
    }
    out
}

/// Opening line for the idea talk. The note under it is the editable draft.
pub fn idea_open_line(card: &UpdateCard) -> String {
    if card.prompt.is_some() {
        return format!(
            "{}\n\nThe draft below does it. Send it as is, or change it first.",
            discuss_context(card)
        );
    }
    format!(
        "{}\n\nThis draft is what would help next time: why it came up, a quick chip, a skill, and an automation. Change the note if that is wrong. Do not copy the earlier chat back.",
        discuss_context(card)
    )
}

/// Editable note for the idea talk. The person can change it before it is sent.
pub fn idea_dialogue(card: &UpdateCard) -> String {
    if let Some(prompt) = card.prompt.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        return prompt.to_string();
    }
    let mut out = String::new();
    if let Some(why) = card.why.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("Why: ");
        out.push_str(why);
        out.push_str("\n\n");
    }
    if let Some(body) = card.body.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(body);
        out.push_str("\n\n");
    }
    out.push_str("Edit this, then send. Keep the chip, the skill, or the automation you want.");
    out
}

/// URLs that appeared in a research payload. Tokens that are not http(s) are ignored.
pub fn links_from_research(payload: &str) -> Vec<CitedLink> {
    let mut out = Vec::new();
    for token in payload.split_whitespace() {
        let Some(url) = http_url_in(token) else {
            continue;
        };
        if out.iter().any(|link: &CitedLink| link.url == url) {
            continue;
        }
        out.push(CitedLink {
            url,
            label: "Source".into(),
        });
    }
    out
}

/// An offer to send, pay, delete, or publish on its own. A story that mentions
/// a bank, the weather, or mail is not an offer.
pub fn digest_topic_refused(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const ACTIONS: &[&str] = &[
        "pay the invoice",
        "pay my ",
        "pay your ",
        "i paid",
        "i'll pay",
        "i will pay",
        "want me to pay",
        "send the email",
        "send an email",
        "send a message",
        "email them",
        "delete the ",
        "delete my ",
        "delete your ",
        "i deleted",
        "publish the post",
        "publish this",
        "i published",
        "i'll publish",
        "want me to publish",
        "want me to send",
        "want me to delete",
    ];
    ACTIONS.iter().any(|phrase| lower.contains(phrase))
}

fn due(last: u64, every: u64, now: u64) -> bool {
    if every == 0 {
        return true;
    }
    last == 0 || now.saturating_sub(last) >= every
}

/// Housekeep runner. Does not read transcripts and does not stamp a review day.
pub fn tick_feed_pulse(
    cards: &mut Vec<UpdateCard>,
    pulse: &mut FeedPulse,
    now: PulseNow,
    material: DigestMaterial<'_>,
) -> PulseTick {
    let mut tick = PulseTick {
        expired: 0,
        released: 0,
        digest_posted: false,
        digest_held: pulse.digest_held,
        cards_changed: false,
        pulse_changed: false,
        digest_needs_lookup: false,
        digest_consumed: false,
    };
    if pulse.expiry_on && due(pulse.last_expiry_ms, pulse.expiry_ms, now.now_ms) {
        let n = expire_ideas(cards, now.now_ms);
        pulse.last_expiry_ms = now.now_ms;
        tick.expired = n;
        if n > 0 {
            tick.cards_changed = true;
            tick.pulse_changed = true;
        }
    }
    if pulse.quiet_release_on
        && due(
            pulse.last_quiet_release_ms,
            pulse.quiet_release_ms,
            now.now_ms,
        )
    {
        pulse.last_quiet_release_ms = now.now_ms;
        if !now.quiet {
            let n = release_quiet_hold(cards);
            tick.released = n;
            if n > 0 {
                tick.cards_changed = true;
            }
        }
    }
    let digest_due = pulse.digest_on
        && (pulse.digest_held || due(pulse.last_digest_ms, pulse.digest_ms, now.now_ms));
    if digest_due && now.quiet {
        if !pulse.digest_held {
            pulse.digest_held = true;
            tick.pulse_changed = true;
        }
        tick.digest_held = true;
        return tick;
    }
    if digest_due {
        if digest_blocks_next(cards) {
            pulse.last_digest_ms = now.now_ms;
            pulse.digest_held = false;
            tick.digest_held = false;
            tick.pulse_changed = true;
            return tick;
        }
        if digest_topic_refused(material.brief) {
            stamp_digest(pulse, now.now_ms, &mut tick);
            return tick;
        }
        let Some(edition) = material.edition else {
            tick.digest_needs_lookup = true;
            return tick;
        };
        if let Some(card) = compose_digest(cards, now.now_ms, material, edition) {
            post_update(cards, card);
            tick.digest_posted = true;
            tick.cards_changed = true;
        }
        stamp_digest(pulse, now.now_ms, &mut tick);
        tick.digest_consumed = true;
    }
    tick
}

fn stamp_digest(pulse: &mut FeedPulse, now_ms: u64, tick: &mut PulseTick) {
    pulse.last_digest_ms = now_ms;
    pulse.digest_held = false;
    tick.digest_held = false;
    tick.pulse_changed = true;
}

/// Post ideas the model wrote (see `crate::ideas`). Skips a title already on the
/// board, already turned down, or already an automation, and an idea whose topic
/// a live card already covers. Only the first new idea of a batch pops up on the
/// home feed; the rest wait on the Ideas board. Returns how many posted.
pub fn post_generated_ideas(
    cards: &mut Vec<UpdateCard>,
    pulse: &mut FeedPulse,
    now: u64,
    seeds: &[crate::ideas::IdeaSeed],
    have_names: &[&str],
    lessons: &str,
) -> usize {
    let mut posted = 0usize;
    for seed in seeds {
        if board_covers_topic(cards, &crate::ideas::idea_topic_text(seed)) {
            continue;
        }
        // One id per idea: the source alone collides for ideas posted in one pass.
        let source = format!(
            "{}-{}",
            crate::ideas::IDEA_SOURCE_GENERATED,
            crate::cabin_engine::engine_slug(&seed.title)
        );
        let why = if seed.reason.trim().is_empty() {
            seed.kind.why_label()
        } else {
            seed.reason.trim()
        };
        if post_useful_idea(
            cards,
            pulse,
            now,
            &source,
            &seed.title,
            &seed.body,
            have_names,
            false,
            lessons,
            why,
        ) {
            if let Some(card) = cards
                .iter_mut()
                .find(|c| c.kind == UpdateKind::Idea && c.title.eq_ignore_ascii_case(seed.title.trim()))
            {
                card.prompt = Some(seed.prompt.trim().to_string());
                card.idea_kind = Some(seed.kind);
                card.details = Some(seed.details.trim().to_string()).filter(|d| !d.is_empty());
                // One new idea pops up on the home feed. Dismissing it there keeps it here.
                card.feed_pin = posted == 0;
            }
            posted += 1;
        }
    }
    posted
}

/// A live idea on the board (not applied, not deleted) is already about this.
pub fn board_covers_topic(cards: &[UpdateCard], text: &str) -> bool {
    cards.iter().any(|c| {
        c.kind == UpdateKind::Idea
            && c.status != UpdateStatus::Dismissed
            && crate::ideas::same_topic(
                &format!(
                    "{} {} {}",
                    c.title,
                    c.body.as_deref().unwrap_or(""),
                    c.prompt.as_deref().unwrap_or("")
                ),
                text,
            )
    })
}

/// A skill the nightly review suggested becomes a Skill idea. Apply saves it.
pub fn post_skill_idea(
    cards: &mut Vec<UpdateCard>,
    pulse: &mut FeedPulse,
    now: u64,
    item: &crate::review::LearnedSuggestion,
    have_skills: &[&str],
) -> bool {
    let name = item.name.as_deref().unwrap_or("").trim();
    if name.is_empty()
        || have_skills.iter().any(|s| s.eq_ignore_ascii_case(name))
        || cards.iter().any(|c| {
            c.skill
                .as_ref()
                .and_then(|k| k.name.as_deref())
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
    {
        return false;
    }
    let title = if item.title.trim().is_empty() {
        name.to_string()
    } else {
        item.title.trim().to_string()
    };
    let topic = format!(
        "{title} {} {}",
        item.body,
        item.trigger.as_deref().unwrap_or("")
    );
    if board_covers_topic(cards, &topic) {
        return false;
    }
    let source = format!("skill-{}", crate::cabin_engine::engine_slug(name));
    if !post_useful_idea(
        cards,
        pulse,
        now,
        &source,
        &title,
        &item.body,
        &[],
        false,
        "",
        crate::ideas::IdeaKind::Skill.why_label(),
    ) {
        return false;
    }
    if let Some(card) = cards
        .iter_mut()
        .find(|c| c.kind == UpdateKind::Idea && c.title.eq_ignore_ascii_case(&title))
    {
        card.idea_kind = Some(crate::ideas::IdeaKind::Skill);
        let mut details = item.body.trim().to_string();
        if let Some(trigger) = item.trigger.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            details.push_str(&format!("\n\nWhen it runs: {trigger}"));
        }
        if let Some(steps) = item.instructions.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            details.push_str(&format!("\n\nSteps:\n{steps}"));
        }
        card.details = Some(details);
        card.skill = Some(item.clone());
    }
    true
}

/// Generated ideas still live on the board (not accepted, not turned down).
pub fn live_generated_ideas(cards: &[UpdateCard]) -> usize {
    cards
        .iter()
        .filter(|c| {
            c.kind == UpdateKind::Idea && c.prompt.is_some() && c.skill.is_none() && !c.built && !c.modified
        })
        .count()
}

/// Cards the old template generators wrote ("Set up: …", "A chip for …") go,
/// unless you already built or discussed one. Returns how many were removed.
pub fn purge_template_ideas(cards: &mut Vec<UpdateCard>) -> usize {
    let before = cards.len();
    cards.retain(|c| {
        !(c.kind == UpdateKind::Idea
            && c.prompt.is_none()
            && !c.built
            && c.discuss_thread.is_none()
            && crate::ideas::is_template_idea_title(&c.title))
    });
    before - cards.len()
}

/// Untouched ideas that only answer a one-time job (one driver install, one fix)
/// go. Cards you opened, changed, filed, or talked about stay. Returns how many went.
pub fn purge_one_off_ideas(cards: &mut Vec<UpdateCard>, ground: &crate::ideas::IdeaGround) -> usize {
    let before = cards.len();
    cards.retain(|c| {
        if c.kind != UpdateKind::Idea || idea_touched(c) {
            return true;
        }
        let text = format!(
            "{} {} {}",
            c.title,
            c.body.as_deref().unwrap_or(""),
            c.prompt.as_deref().unwrap_or("")
        );
        !crate::ideas::idea_from_one_off(&text, ground)
    });
    before - cards.len()
}

fn post_useful_idea(
    cards: &mut Vec<UpdateCard>,
    pulse: &mut FeedPulse,
    now: u64,
    source: &str,
    title: &str,
    body: &str,
    have_names: &[&str],
    recover: bool,
    lessons: &str,
    why: &str,
) -> bool {
    if setup_blocked_by_lessons(title, lessons) {
        return false;
    }
    let title = title.trim();
    let body = body.trim();
    if title.is_empty() || body.is_empty() || digest_topic_refused(title) || digest_topic_refused(body)
    {
        return false;
    }
    if have_names
        .iter()
        .any(|name| name.trim().eq_ignore_ascii_case(title))
    {
        return false;
    }
    if !recover
        && (pulse
            .idea_titles
            .iter()
            .any(|t| t.eq_ignore_ascii_case(title))
            || source_gone(pulse, source)
            || turned_down_topic(pulse, title))
    {
        return false;
    }
    if cards
        .iter()
        .any(|c| c.kind == UpdateKind::Idea && c.title.eq_ignore_ascii_case(title))
    {
        return false;
    }
    if !make_room_for_idea(cards) {
        return false;
    }
    let mut card = idea_card(source, title, body, now);
    card.why = Some(if why.trim().is_empty() {
        "A way this cabin can work for you.".into()
    } else {
        why.trim().to_string()
    });
    if less_blocks(pulse, &card, now) {
        return false;
    }
    let kept = card.title.clone();
    post_update(cards, card);
    remember_idea_title(pulse, &kept);
    true
}

/// Room for one more unmodified idea: at `IDEA_BOARD_MAX` the oldest unmodified card
/// goes. Modified cards are outside the cap and never pushed out.
fn make_room_for_idea(cards: &mut Vec<UpdateCard>) -> bool {
    loop {
        let unmodified: Vec<usize> = cards
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == UpdateKind::Idea && !c.modified)
            .map(|(i, _)| i)
            .collect();
        if unmodified.len() < IDEA_BOARD_MAX {
            return true;
        }
        let Some(&oldest) = unmodified.iter().min_by_key(|&&i| cards[i].created_at) else {
            return true;
        };
        cards.remove(oldest);
    }
}

/// Ideas you have worked on. At most `IDEA_MODIFIED_MAX`.
pub fn modified_ideas(cards: &[UpdateCard]) -> usize {
    cards
        .iter()
        .filter(|c| c.kind == UpdateKind::Idea && c.modified)
        .count()
}

/// Mark an idea as worked on (you edited its action or talked about it). It leaves
/// the push-out queue and the home feed. Refused when `IDEA_MODIFIED_MAX` are open.
pub fn mark_idea_modified(cards: &mut [UpdateCard], id: &str) -> Result<(), String> {
    let open = modified_ideas(cards);
    let Some(card) = cards
        .iter_mut()
        .find(|c| c.id == id && c.kind == UpdateKind::Idea)
    else {
        return Err("That idea is gone".into());
    };
    if card.modified {
        return Ok(());
    }
    if open >= IDEA_MODIFIED_MAX {
        return Err(format!(
            "{IDEA_MODIFIED_MAX} ideas are already in progress. Apply or delete one first."
        ));
    }
    card.modified = true;
    card.feed_pin = false;
    card.status = UpdateStatus::Opened;
    Ok(())
}

/// Keep your edited action on the card until it is applied.
pub fn set_idea_draft(cards: &mut [UpdateCard], id: &str, draft: &str) -> Result<(), String> {
    mark_idea_modified(cards, id)?;
    if let Some(card) = cards.iter_mut().find(|c| c.id == id) {
        let d = draft.trim();
        card.draft = if d.is_empty() || Some(d) == card.prompt.as_deref().map(str::trim) {
            None
        } else {
            Some(d.chars().take(2000).collect())
        };
    }
    Ok(())
}

/// The line the agent ends an idea-card reply with to rewrite the card's action.
pub const CARD_ACTION_TAG: &str = "CARD_ACTION:";
/// What the chat shows once the card took the new action.
pub const CARD_ACTION_DONE: &str = "↳ Updated the card's action.";

/// First line of the chat inside an idea card.
pub fn idea_chat_open_line(card: &UpdateCard) -> String {
    let what = match card.idea_type_label() {
        "Skill" => "this skill",
        "Automation" => "this automation",
        _ => "this suggestion",
    };
    format!(
        "Want to change {what}? I can explain it, tighten the action, or fit it to how you work. Nothing runs until you press Apply."
    )
}

/// Hidden brief sent with every turn of an idea card's chat.
pub fn idea_card_brief(card: &UpdateCard) -> String {
    let mut out = format!(
        "You are helping the person refine one idea card in GrokHub before they apply it.\nType: {}\nTitle: {}",
        card.idea_type_label(),
        card.title
    );
    if let Some(body) = card.body.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\nSummary: ");
        out.push_str(body);
    }
    if let Some(details) = card.details.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\nDetails: ");
        out.push_str(details);
    }
    let action = card.idea_action();
    if !action.is_empty() {
        out.push_str("\nCurrent action (what Apply does): ");
        out.push_str(&action);
    }
    out.push_str(&format!(
        "\n\nAnswer briefly. Do not carry out the action yourself; the person applies it from the card. When you both settle a change to the action, end your reply with one line that starts with {CARD_ACTION_TAG} followed by the whole new action on that line."
    ));
    out
}

fn card_action_in(line: &str) -> Option<String> {
    let t = line.trim().trim_start_matches(['*', '`', '-', ' ']);
    let rest = t.strip_prefix(CARD_ACTION_TAG)?;
    let a = rest.trim().trim_matches(['*', '`', '"']).trim();
    (!a.is_empty()).then(|| a.chars().take(2000).collect())
}

/// Pull a `CARD_ACTION:` line out of an agent reply. Returns the reply with that
/// line swapped for a short note, and the new action. `None` when there is none.
/// Only the reply counts: a thought or a tool row that mentions the tag is left
/// alone, and the newest action line wins.
pub fn take_card_action(reply: &str) -> Option<(String, String)> {
    let says = crate::chat_view::strip_thinking(reply);
    let (line, action) = says
        .lines()
        .rev()
        .find_map(|l| card_action_in(l).map(|a| (l.trim().to_string(), a)))?;
    let lines: Vec<&str> = reply.lines().collect();
    let pos = lines.iter().rposition(|l| l.trim() == line)?;
    let kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| if i == pos { CARD_ACTION_DONE } else { *l })
        .collect();
    Some((kept.join("\n").trim_end().to_string(), action))
}

/// Newest ideas first, with the ones you are working on at the top.
pub fn ideas_board(cards: &[UpdateCard]) -> Vec<UpdateCard> {
    let mut out = visible_ideas(cards);
    out.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then(b.created_at.cmp(&a.created_at))
    });
    out
}

fn remember_idea_title(pulse: &mut FeedPulse, title: &str) {
    if pulse
        .idea_titles
        .iter()
        .any(|t| t.eq_ignore_ascii_case(title))
    {
        return;
    }
    // Once full this used to stop remembering, so every later idea could come back.
    if pulse.idea_titles.len() >= IDEA_TITLE_MEMORY {
        pulse.idea_titles.remove(0);
    }
    pulse.idea_titles.push(title.to_string());
}

/// The user deleted this idea or took it off the home feed. The oldest falls off.
pub fn remember_turned_down(pulse: &mut FeedPulse, title: &str) {
    let title = title.trim();
    if title.is_empty() {
        return;
    }
    pulse.turned_down.retain(|t| !t.eq_ignore_ascii_case(title));
    if pulse.turned_down.len() >= TURNED_DOWN_MEMORY {
        pulse.turned_down.remove(0);
    }
    pulse.turned_down.push(title.to_string());
}

/// A title the user turned down, or one on the same topic.
pub fn turned_down_topic(pulse: &FeedPulse, title: &str) -> bool {
    let title = title.trim();
    !title.is_empty()
        && pulse
            .turned_down
            .iter()
            .any(|t| t.eq_ignore_ascii_case(title) || crate::ideas::same_topic(t, title))
}

/// Turned-down titles for a prompt, newest first: what the user said no to, then ideas
/// that left the board some other way.
pub fn turned_down_titles(pulse: &FeedPulse, on_board: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in pulse
        .turned_down
        .iter()
        .rev()
        .chain(pulse.idea_titles.iter().rev())
    {
        if on_board.iter().any(|b| b.eq_ignore_ascii_case(t))
            || out.iter().any(|o| o.eq_ignore_ascii_case(t))
        {
            continue;
        }
        out.push(t.clone());
    }
    out
}

fn digest_blocks_next(cards: &[UpdateCard]) -> bool {
    let Some(card) = newest(cards, UpdateKind::Digest) else {
        return false;
    };
    if card.held {
        return true;
    }
    card.status == UpdateStatus::Unread && card.reaction.is_none() && card.discuss_thread.is_none()
}

fn newest(cards: &[UpdateCard], kind: UpdateKind) -> Option<&UpdateCard> {
    cards
        .iter()
        .filter(|c| c.kind == kind)
        .max_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)))
}

fn compose_digest(
    cards: &[UpdateCard],
    now: u64,
    material: DigestMaterial<'_>,
    edition: DigestEdition<'_>,
) -> Option<UpdateCard> {
    if edition.refused || digest_topic_refused(edition.body) || digest_topic_refused(edition.title)
    {
        return None;
    }
    let n = cards
        .iter()
        .filter(|c| c.kind == UpdateKind::Digest)
        .count()
        + 1;
    let allowed = real_links(material.links);
    if !edition.found || allowed.is_empty() {
        let body = format!("{HONEST_EMPTY} {STEER_LINE}");
        let mut card = digest_card("edition", &format!("Digest {n}"), &body, now);
        card.why = Some(STEER_LINE.into());
        return Some(card);
    }
    let title = {
        let given = clip_line(edition.title, TITLE_CHARS);
        if given.is_empty() {
            format!("Digest {n}")
        } else {
            given
        }
    };
    let taste: Vec<&TasteNote> = material
        .taste
        .iter()
        .filter(|note| note.reaction.is_some() || !note.said.trim().is_empty())
        .collect();
    let mut parts: Vec<String> = Vec::new();
    let written = edition.body.trim();
    if !written.is_empty() {
        parts.push(written.to_string());
    }
    if let Some(note) = taste.iter().find(|n| n.reaction == Some(CardReaction::Up)) {
        parts.push(format!("You marked up {}", note.title));
    } else if let Some(note) = taste
        .iter()
        .find(|n| n.reaction == Some(CardReaction::Down))
    {
        parts.push(format!("You marked down {}", note.title));
    }
    if let Some(note) = taste.iter().find(|n| !n.said.trim().is_empty()) {
        parts.push(format!("You said {}", clip_line(&note.said, 80)));
    }
    parts.push(STEER_LINE.to_string());
    let urls: Vec<String> = allowed.iter().map(|l| l.url.clone()).collect();
    let body = strip_foreign_urls(&parts.join(" "), &urls);
    if body.trim().is_empty() {
        return None;
    }
    let mut card = digest_card("edition", &title, &body, now);
    card.citations = urls;
    card.why = Some(STEER_LINE.into());
    Some(card)
}

fn real_links(links: &[CitedLink]) -> Vec<CitedLink> {
    let mut out = Vec::new();
    for link in links {
        let Some(url) = http_url_in(&link.url) else {
            continue;
        };
        if out.iter().any(|have: &CitedLink| have.url == url) {
            continue;
        }
        let label = clip_line(&link.label, 48);
        out.push(CitedLink {
            url,
            label: if label.is_empty() {
                "Source".into()
            } else {
                label
            },
        });
    }
    out
}

fn useful_profile_lines(raw: &str) -> Vec<String> {
    const SKIP: &[&str] = &[
        "Who this cabin is. Edit this.",
        "Who you are. Edit this.",
        "Long-term notes.",
    ];
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !SKIP.contains(line))
        .map(|line| clip_line(line, BODY_CHARS))
        .filter(|line| !line.is_empty())
        .take(4)
        .collect()
}

fn strip_foreign_urls(text: &str, allowed: &[String]) -> String {
    let mut kept = Vec::new();
    for token in text.split_whitespace() {
        if let Some(url) = http_url_in(token) {
            if allowed.iter().any(|ok| ok == &url) {
                kept.push(token.to_string());
            }
        } else {
            kept.push(token.to_string());
        }
    }
    kept.join(" ")
}

fn http_url_in(token: &str) -> Option<String> {
    let start = token.find("https://").or_else(|| token.find("http://"))?;
    let rest = &token[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '>' | '"' | '\'' | ','))
        .unwrap_or(rest.len());
    let url = &rest[..end];
    if url.len() <= "https://".len() {
        return None;
    }
    if url.starts_with("https://") || url.starts_with("http://") {
        Some(url.to_string())
    } else {
        None
    }
}

fn clip_line(raw: &str, max_chars: usize) -> String {
    let flat = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = flat.chars().take(max_chars).collect();
    if flat.chars().count() > max_chars {
        out.push('…');
    }
    out
}

fn feed_card_id(prefix: &str, source_id: &str, created_at: u64) -> String {
    let source = source_id.trim();
    if source.is_empty() {
        format!("{prefix}-{created_at}")
    } else {
        format!("{prefix}-{source}-{created_at}")
    }
}

fn blank_card(
    id: String,
    kind: UpdateKind,
    title: String,
    body: Option<String>,
    created_at: u64,
) -> UpdateCard {
    UpdateCard {
        id,
        kind,
        title,
        body,
        created_at,
        status: UpdateStatus::Unread,
        action: None,
        expires_at: None,
        held: false,
        citations: Vec::new(),
        reaction: None,
        discuss_thread: None,
        built: false,
        board_id: None,
        why: None,
        feed_pin: false,
        feed_kept: false,
        prompt: None,
        idea_kind: None,
        details: None,
        draft: None,
        modified: false,
        skill: None,
        source_id: String::new(),
        runs: 1,
        first_at: created_at,
        collapsed: false,
    }
}

/// Finished automation or scheduled loop. `summary` is the short result.
pub fn automation_done_card(
    source_id: &str,
    title: &str,
    summary: &str,
    created_at: u64,
) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let title = if title.is_empty() {
        "Automation finished".to_string()
    } else {
        title
    };
    let body = {
        let summary = clip_line(summary, BODY_CHARS);
        if summary.is_empty() {
            None
        } else {
            Some(summary)
        }
    };
    let mut card = blank_card(
        feed_card_id("done", source_id, created_at),
        UpdateKind::AutomationDone,
        title,
        body,
        created_at,
    );
    card.action = Some(UpdateAction::OpenAutomations);
    card.source_id = source_id.trim().to_string();
    apply_stored_why(&mut card);
    card
}

/// A scheduled job failed. Same kind as a finished one so older feeds still load;
/// the title says it failed and the body says why.
pub fn automation_failed_card(source_id: &str, name: &str, why: &str, created_at: u64) -> UpdateCard {
    let name = clip_line(name, TITLE_CHARS.saturating_sub(8).max(8));
    let title = if name.is_empty() {
        "Automation failed".to_string()
    } else {
        format!("{name} failed")
    };
    let why = clip_line(why, BODY_CHARS);
    let body = Some(if why.is_empty() {
        "Open Automations to run it again.".to_string()
    } else {
        why
    });
    let mut card = blank_card(
        feed_card_id("fail", source_id, created_at),
        UpdateKind::AutomationDone,
        title,
        body,
        created_at,
    );
    card.action = Some(UpdateAction::OpenAutomations);
    card.source_id = source_id.trim().to_string();
    apply_stored_why(&mut card);
    card
}

/// User saved a clock job or an interval loop.
pub fn schedule_created_card(
    source_id: &str,
    title: &str,
    when_label: &str,
    created_at: u64,
) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let title = if title.is_empty() {
        "Scheduled".to_string()
    } else {
        title
    };
    let when_label = clip_line(when_label, TITLE_CHARS);
    let body = if when_label.is_empty() {
        None
    } else {
        Some(format!("Scheduled · {when_label}"))
    };
    let mut card = blank_card(
        feed_card_id("sched", source_id, created_at),
        UpdateKind::ScheduleCreated,
        title,
        body,
        created_at,
    );
    card.action = Some(UpdateAction::OpenAutomations);
    card.source_id = source_id.trim().to_string();
    apply_stored_why(&mut card);
    card
}

/// Situation offer. `post_help` is the producer. Accept opens a discussion.
pub fn suggestion_card(source_id: &str, title: &str, body: &str, created_at: u64) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let title = if title.is_empty() {
        "Suggestion".to_string()
    } else {
        title
    };
    let body = {
        let body = clip_line(body, BODY_CHARS);
        if body.is_empty() {
            None
        } else {
            Some(body)
        }
    };
    let mut card = blank_card(
        feed_card_id("sugg", source_id, created_at),
        UpdateKind::Suggestion,
        title,
        body,
        created_at,
    );
    card.source_id = source_id.trim().to_string();
    card
}

/// Typed offer. Accept stays `commit_schedule`. It does not file a Todo.
pub fn automate_offer_card(source_id: &str, title: &str, created_at: u64) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let title = if title.is_empty() {
        "Want me to automate this and notify you here when done?".to_string()
    } else {
        title
    };
    let mut card = blank_card(
        feed_card_id("offer", source_id, created_at),
        UpdateKind::AutomateOffer,
        title,
        Some("Want me to automate this and notify you here when done?".into()),
        created_at,
    );
    card.source_id = source_id.trim().to_string();
    card
}

/// Idea card. Accept files a workboard Todo. The generator cannot dismiss it.
pub fn idea_card(source_id: &str, title: &str, body: &str, created_at: u64) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let title = if title.is_empty() {
        "Idea".to_string()
    } else {
        title
    };
    let body = {
        let body = clip_line(body, BODY_CHARS);
        if body.is_empty() {
            None
        } else {
            Some(body)
        }
    };
    let mut card = blank_card(
        feed_card_id("idea", source_id, created_at),
        UpdateKind::Idea,
        title,
        body,
        created_at,
    );
    card.action = Some(UpdateAction::OpenWorkboard);
    card.source_id = source_id.trim().to_string();
    card
}

pub fn digest_card(source_id: &str, title: &str, body: &str, created_at: u64) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let title = if title.is_empty() {
        "Digest".to_string()
    } else {
        title
    };
    let body = {
        let body = clip_line(body, DIGEST_BODY_CHARS);
        if body.is_empty() {
            None
        } else {
            Some(body)
        }
    };
    let mut card = blank_card(
        feed_card_id("digest", source_id, created_at),
        UpdateKind::Digest,
        title,
        body,
        created_at,
    );
    card.source_id = source_id.trim().to_string();
    card
}

/// One line the daily lookup should follow. An empty brief uses a profile line or the open project.
pub fn digest_steer(brief: &str, user_md: &str, memory_md: &str, project: &str) -> String {
    let brief = brief.trim();
    if !brief.is_empty() {
        return clip_line(brief, BODY_CHARS);
    }
    for raw in [user_md, memory_md] {
        if let Some(line) = useful_profile_lines(raw).into_iter().next() {
            return line;
        }
    }
    let name = project
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    if !name.is_empty() && name != "." {
        return format!("What is new around {name}");
    }
    "What they have been working on".into()
}

/// One completion. Two written items, real URLs only.
pub fn digest_lookup_prompt(steer: &str) -> String {
    let steer = clip_line(steer, BODY_CHARS);
    format!(
        "Write a short home-feed edition for this person. Steer: {steer}. \
Two items only. First a news note, then a story or longer read. \
Each item is two or three sentences on why it matters to them, then one real http URL on its own line. \
Use only URLs you actually found. If you cannot find a real URL, reply with exactly: NONE. \
Do not invent a source, a count, or a fact. Do not offer to send, pay, delete, or publish anything."
    )
}

pub fn parse_lookup(raw: &str) -> ParsedLookup {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("NONE") {
        return ParsedLookup {
            title: String::new(),
            body: String::new(),
            found: false,
            refused: false,
        };
    }
    if digest_topic_refused(raw) {
        return ParsedLookup {
            title: String::new(),
            body: raw.to_string(),
            found: false,
            refused: true,
        };
    }
    let links = links_from_research(raw);
    if links.is_empty() {
        return ParsedLookup {
            title: String::new(),
            body: String::new(),
            found: false,
            refused: false,
        };
    }
    let title = raw
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && http_url_in(line).is_none())
        .map(|line| clip_line(line, TITLE_CHARS))
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "For you".into());
    ParsedLookup {
        title,
        body: clip_line(raw, DIGEST_BODY_CHARS),
        found: true,
        refused: false,
    }
}

pub fn remember_dismissed_source(pulse: &mut FeedPulse, source_id: &str) {
    let id = source_id.trim();
    if id.is_empty() || source_gone(pulse, id) {
        return;
    }
    if pulse.dismissed_sources.len() >= 64 {
        pulse.dismissed_sources.remove(0);
    }
    pulse.dismissed_sources.push(id.to_string());
}

/// Idea cards from a paused run and a repeated action, plus at most one situation card.
/// Guide pace passes `offer_repeats` false so the repeated-action offer stays off.
/// These ideas stay off the home pin. The model still owns that one slot.
pub fn post_help(
    cards: &mut Vec<UpdateCard>,
    pulse: &mut FeedPulse,
    now: u64,
    quiet: bool,
    offer_repeats: bool,
    paused: &[PausedJob<'_>],
    repeats: &[RepeatedAction<'_>],
    lessons: &str,
) -> HelpTick {
    let mut ideas = 0usize;
    let live: Vec<String> = paused
        .iter()
        .filter(|job| job.detail == PAUSED_DETAIL && !job.id.trim().is_empty())
        .map(|job| job.id.to_string())
        .collect();
    for id in &live {
        pulse.paused_seen.entry(id.clone()).or_insert(now);
    }
    pulse
        .paused_seen
        .retain(|id, _| live.iter().any(|live_id| live_id == id));
    for job in paused {
        if job.detail != PAUSED_DETAIL {
            continue;
        }
        let title = clip_line(job.title, TITLE_CHARS);
        let source = format!("pause:{}", job.id.trim());
        if title.is_empty() || source_gone(pulse, &source) {
            continue;
        }
        if post_useful_idea(
            cards,
            pulse,
            now,
            &source,
            &title,
            "That job is still paused. I can pick it up when you say.",
            &[],
            false,
            lessons,
            "A job you left open.",
        ) {
            ideas += 1;
        }
    }
    for action in repeats {
        if action.count < 3 || action.dismissed || action.automated {
            continue;
        }
        let label = clip_line(action.label, TITLE_CHARS);
        let source = format!("repeat:{}", action.key.trim());
        if label.is_empty() || action.key.trim().is_empty() || source_gone(pulse, &source) {
            continue;
        }
        if post_useful_idea(
            cards,
            pulse,
            now,
            &source,
            &label,
            "You keep doing this. I can make it a reminder.",
            &[],
            false,
            lessons,
            "Something you keep doing.",
        ) {
            ideas += 1;
        }
    }
    let mut posted = false;
    if !has_unread_suggestion(cards) {
        for job in paused {
            if job.detail != PAUSED_DETAIL || job.id.trim().is_empty() {
                continue;
            }
            let seen = pulse.paused_seen.get(job.id).copied().unwrap_or(now);
            if now.saturating_sub(seen) < PAUSE_OFFER_MS {
                continue;
            }
            let source = format!("pause:{}", job.id.trim());
            if post_situation(
                cards,
                pulse,
                now,
                quiet,
                &source,
                "That job is still paused.",
                "Want me to pick it back up?",
            ) {
                posted = true;
                break;
            }
        }
    }
    if !posted && offer_repeats && !has_unread_suggestion(cards) {
        for action in repeats {
            if action.count < 3 || action.dismissed || action.automated || action.key.trim().is_empty()
            {
                continue;
            }
            let label = clip_line(action.label, 40);
            if label.is_empty() {
                continue;
            }
            let source = format!("repeat:{}", action.key.trim());
            let title = format!("You keep doing {label}.");
            if post_situation(
                cards,
                pulse,
                now,
                quiet,
                &source,
                &title,
                "Want me to make that a reminder?",
            ) {
                posted = true;
                break;
            }
        }
    }
    HelpTick {
        ideas,
        posted,
        ping: posted && !quiet,
    }
}

fn has_unread_suggestion(cards: &[UpdateCard]) -> bool {
    cards
        .iter()
        .any(|c| c.kind == UpdateKind::Suggestion && c.status == UpdateStatus::Unread)
}

fn post_situation(
    cards: &mut Vec<UpdateCard>,
    pulse: &FeedPulse,
    now: u64,
    quiet: bool,
    source: &str,
    title: &str,
    body: &str,
) -> bool {
    // Opening the card marks it read. Checking only unread offers posted the same one again.
    if source_gone(pulse, source)
        || has_unread_suggestion(cards)
        || cards
            .iter()
            .any(|c| c.kind == UpdateKind::Suggestion && c.source_id == source.trim())
    {
        return false;
    }
    let mut card = suggestion_card(source, title, body, now);
    hold_if_quiet(&mut card, quiet);
    if home_post_gate(pulse, &card, now).skip {
        return false;
    }
    post_update(cards, card);
    true
}

fn source_gone(pulse: &FeedPulse, source_id: &str) -> bool {
    let id = source_id.trim();
    !id.is_empty() && pulse.dismissed_sources.iter().any(|saved| saved == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, at: u64, status: UpdateStatus) -> UpdateCard {
        let mut card = blank_card(id.into(), UpdateKind::AutomationDone, id.into(), None, at);
        card.status = status;
        card
    }

    fn material<'a>(
        brief: &'a str,
        links: &'a [CitedLink],
        taste: &'a [TasteNote],
    ) -> DigestMaterial<'a> {
        DigestMaterial {
            brief,
            user_md: "",
            memory_md: "",
            soul_md: "",
            links,
            taste,
            edition: None,
        }
    }

    fn with_edition<'a>(
        material: DigestMaterial<'a>,
        edition: DigestEdition<'a>,
    ) -> DigestMaterial<'a> {
        DigestMaterial {
            edition: Some(edition),
            ..material
        }
    }

    #[test]
    fn visible_feed_is_newest_first_and_hides_when_empty() {
        let cards = vec![
            card("old", 10, UpdateStatus::Unread),
            card("new", 30, UpdateStatus::Opened),
            card("gone", 40, UpdateStatus::Dismissed),
        ];
        assert!(feed_visible(&cards));
        let visible = visible_updates(&cards);
        assert_eq!(
            visible.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["new", "old"]
        );
        assert!(!feed_visible(&[card("gone", 1, UpdateStatus::Dismissed)]));
        assert!(visible_updates(&[]).is_empty());
    }

    #[test]
    fn open_keeps_the_card_and_dismiss_drops_it() {
        let mut cards = vec![card("a", 1, UpdateStatus::Unread)];
        assert!(mark_update_opened(&mut cards, "a"));
        assert_eq!(cards[0].status, UpdateStatus::Opened);
        assert!(feed_visible(&cards));
        assert!(dismiss_update(&mut cards, "a"));
        assert!(cards.is_empty());
        assert!(!feed_visible(&cards));
    }

    #[test]
    fn kinds_round_trip_without_interest_update() {
        let done = automation_done_card("loop-1", "summarize the workboard", "three open", 9);
        let scheduled = schedule_created_card("auto-1", "Board", "weekdays at 09:00", 8);
        let suggestion = suggestion_card("s1", "File the notes", "from last night", 7);
        let offer = automate_offer_card("o1", "", 6);
        assert_eq!(done.kind, UpdateKind::AutomationDone);
        assert_eq!(done.body.as_deref(), Some("three open"));
        assert_eq!(scheduled.kind, UpdateKind::ScheduleCreated);
        assert!(scheduled
            .body
            .as_deref()
            .unwrap()
            .contains("weekdays at 09:00"));
        assert_eq!(suggestion.kind, UpdateKind::Suggestion);
        assert_eq!(offer.kind, UpdateKind::AutomateOffer);
        assert!(offer
            .body
            .as_deref()
            .unwrap()
            .contains("notify you here when done"));
        let blob = serde_json::to_string(&vec![done, scheduled, suggestion, offer]).unwrap();
        assert!(blob.contains("automation_done"));
        assert!(blob.contains("schedule_created"));
        assert!(blob.contains("suggestion"));
        assert!(blob.contains("automate_offer"));
        assert!(!blob.contains("interest_update"));
        let back: Vec<UpdateCard> = serde_json::from_str(&blob).unwrap();
        assert_eq!(back.len(), 4);
        assert_eq!(visible_updates(&back)[0].kind, UpdateKind::AutomationDone);
    }

    #[test]
    fn ideas_and_digest_do_not_consume_the_event_cap() {
        let mut cards = vec![idea_card("b", "more F1", "from the brief", 100)];
        cards.push(digest_card("d", "Sunday", "brief", 90));
        for n in 0..6 {
            cards.push(automation_done_card(&format!("e{n}"), "loop", "done", n));
        }
        let events = visible_updates(&cards);
        assert!(events.iter().all(|c| c.kind.event()));
        assert_eq!(events.len(), 6);
        assert_eq!(events.len().min(FEED_PAINT_MAX), FEED_PAINT_MAX);
        assert_eq!(visible_ideas(&cards).len(), 1);
        assert_eq!(visible_digests(&cards).len(), 1);
        assert!(home_feed_n(&cards) > FEED_PAINT_MAX);
    }

    #[test]
    fn store_cap_does_not_drop_ideas() {
        let mut cards = vec![idea_card("keep", "more F1", "brief", 1)];
        for n in 0..50 {
            post_update(
                &mut cards,
                automation_done_card(&format!("n{n}"), "loop", "done", 10 + n),
            );
        }
        assert!(cards
            .iter()
            .any(|c| c.kind == UpdateKind::Idea && c.title.contains("F1")));
        assert!(cards.iter().filter(|c| c.kind.event()).count() <= FEED_STORE_MAX);
    }

    #[test]
    fn expiry_drops_ideas_only() {
        let mut old_idea = idea_card("old", "more F1", "brief", 0);
        old_idea.expires_at = Some(50);
        let mut kept = idea_card("new", "later", "brief", 40);
        kept.expires_at = Some(10_000);
        let done = automation_done_card("loop", "finished", "ok", 0);
        let scheduled = schedule_created_card("auto", "morning", "09:00", 0);
        let mut cards = vec![old_idea, kept, done, scheduled];
        assert_eq!(expire_ideas(&mut cards, 50), 1);
        assert!(cards.iter().any(|c| c.kind == UpdateKind::Idea));
        assert!(cards.iter().any(|c| c.kind == UpdateKind::AutomationDone));
        assert!(cards.iter().any(|c| c.kind == UpdateKind::ScheduleCreated));
        assert!(!cards.iter().any(|c| c.title.contains("F1")));
    }

    #[test]
    fn user_dismiss_is_the_idea_kill() {
        let mut cards = vec![
            idea_card("a", "more F1", "brief", 1),
            automation_done_card("loop", "done", "ok", 2),
        ];
        assert!(!dismiss_idea(&mut cards, "missing"));
        let event_id = cards
            .iter()
            .find(|c| c.kind.event())
            .unwrap()
            .id
            .clone();
        assert!(!dismiss_idea(&mut cards, &event_id));
        let idea_id = cards
            .iter()
            .find(|c| c.kind == UpdateKind::Idea)
            .unwrap()
            .id
            .clone();
        assert!(dismiss_idea(&mut cards, &idea_id));
        assert!(cards.iter().all(|c| c.kind != UpdateKind::Idea));
    }

    #[test]
    fn quiet_hold_hides_then_release_shows() {
        let mut done = automation_done_card("loop", "done", "ok", 5);
        hold_if_quiet(&mut done, true);
        let mut cards = vec![done];
        assert!(!feed_visible(&cards));
        assert_eq!(release_quiet_hold(&mut cards), 1);
        assert!(feed_visible(&cards));
        assert_eq!(visible_updates(&cards).len(), 1);
    }

    #[test]
    fn quiet_pass_does_not_stamp_the_digest_clock() {
        let mut cards = Vec::new();
        let mut pulse = FeedPulse {
            digest_ms: 1,
            ..FeedPulse::default()
        };
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 100,
                quiet: true,
            },
            material("more F1", &[], &[]),
        );
        assert!(tick.digest_held);
        assert!(!tick.digest_posted);
        assert!(cards.is_empty());
        assert_eq!(pulse.last_digest_ms, 0);
        assert!(pulse.digest_held);
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 200,
                quiet: false,
            },
            material("more F1", &[], &[]),
        );
        assert!(tick.digest_needs_lookup);
        assert!(!tick.digest_posted);
        assert_eq!(pulse.last_digest_ms, 0);
        let edition = DigestEdition {
            found: true,
            refused: false,
            title: "More F1",
            body: "A note on the race. https://news.example/f1",
        };
        let links = [CitedLink {
            url: "https://news.example/f1".into(),
            label: "Source".into(),
        }];
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 300,
                quiet: false,
            },
            with_edition(material("more F1", &links, &[]), edition),
        );
        assert!(tick.digest_posted);
        assert!(tick.digest_consumed);
        assert_eq!(pulse.last_digest_ms, 300);
        assert!(cards.iter().any(|c| c.kind == UpdateKind::Digest));
        assert!(
            !cards.iter().any(|c| c.kind == UpdateKind::Idea),
            "the digest no longer posts a template idea beside it"
        );
        assert!(visible_updates(&cards).is_empty());
    }

    #[test]
    fn digest_cites_only_returned_urls_and_refuses_an_action() {
        let links = links_from_research("see https://news.example/f1 today");
        assert_eq!(links.len(), 1);
        let mut cards = Vec::new();
        let mut pulse = FeedPulse {
            last_expiry_ms: 1,
            expiry_ms: 10_000,
            last_quiet_release_ms: 1,
            quiet_release_ms: 10_000,
            ..FeedPulse::default()
        };
        let edition = DigestEdition {
            found: true,
            refused: false,
            title: "A bank and the weather",
            body: "A story about a bank and the weather. https://news.example/f1 and https://evil.example/phish",
        };
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 50,
                quiet: false,
            },
            with_edition(
                material("more F1 https://evil.example/phish", &links, &[]),
                edition,
            ),
        );
        assert!(tick.digest_posted);
        let digest = cards.iter().find(|c| c.kind == UpdateKind::Digest).unwrap();
        let blob = format!(
            "{} {}",
            digest.body.as_deref().unwrap_or(""),
            digest.citations.join(" ")
        );
        assert!(blob.contains("https://news.example/f1"));
        assert!(blob.contains("bank"));
        assert!(blob.contains("The brief steers the next edition."));
        assert!(!blob.contains("evil.example"));
        let mut refused = Vec::new();
        let mut pulse = FeedPulse::default();
        let tick = tick_feed_pulse(
            &mut refused,
            &mut pulse,
            PulseNow {
                now_ms: 50,
                quiet: false,
            },
            material("pay the invoice for me", &[], &[]),
        );
        assert!(!tick.digest_posted);
        assert!(!tick.digest_needs_lookup);
        assert!(refused.is_empty());
        assert_eq!(pulse.last_digest_ms, 50);
        assert!(!digest_topic_refused("A story about a bank and the weather"));
        assert!(digest_topic_refused("I paid your card"));
        assert!(!crate::review::cabin_real_text("Connect Gmail for this skill"));
    }

    fn seed(kind: crate::ideas::IdeaKind, title: &str) -> crate::ideas::IdeaSeed {
        crate::ideas::IdeaSeed {
            kind,
            title: title.into(),
            body: format!("{title}."),
            details: format!("{title}: the full story."),
            prompt: format!("every weekday at 9, {}", title.to_ascii_lowercase()),
            reason: String::new(),
        }
    }

    fn seeds() -> Vec<crate::ideas::IdeaSeed> {
        use crate::ideas::IdeaKind::*;
        vec![
            seed(Automation, "Morning test run"),
            seed(Skill, "AUR release checklist"),
            seed(Reminder, "Stream prep reminder"),
            seed(Try, "Triage open issues"),
        ]
    }

    #[test]
    fn generated_ideas_post_once_with_their_prompt_and_skip_what_you_run() {
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        let n = post_generated_ideas(&mut cards, &mut pulse, 40, &seeds(), &["Stream prep reminder"], "");
        assert_eq!(n, 3, "a title you already run is skipped");
        let test_run = cards.iter().find(|c| c.title == "Morning test run").expect("posted");
        assert_eq!(test_run.prompt.as_deref(), Some("every weekday at 9, morning test run"));
        assert_eq!(test_run.why.as_deref(), Some("An automation you can turn on"));
        assert_eq!(idea_dialogue(test_run), "every weekday at 9, morning test run", "Accept fills the draft");
        assert!(idea_open_line(test_run).contains("The draft below does it"));
        assert_eq!(live_generated_ideas(&cards), 3);
        assert_eq!(post_generated_ideas(&mut cards, &mut pulse, 50, &seeds(), &[], ""), 1, "only the new one");
        let gone = cards.iter().find(|c| c.title == "Triage open issues").unwrap().id.clone();
        dismiss_idea(&mut cards, &gone);
        assert_eq!(
            post_generated_ideas(&mut cards, &mut pulse, 60, &seeds(), &[], ""),
            0,
            "a turned-down title does not come back"
        );
    }

    #[test]
    fn new_ideas_pop_on_home_and_a_home_dismiss_keeps_them_on_the_board() {
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        let n = post_generated_ideas(&mut cards, &mut pulse, 1_000, &seeds(), &[], "");
        assert_eq!(n, 4);
        assert_eq!(
            cards.iter().filter(|c| c.feed_pin).count(),
            1,
            "one new idea per batch pops up on the home feed, not the whole batch"
        );
        assert_eq!(feed_ideas(&cards, 1_000).len(), 1);
        let gone = feed_ideas(&cards, 1_000)[0].id.clone();
        assert!(unpin_feed_idea(&mut cards, &gone));
        assert!(cards.iter().any(|c| c.id == gone && !c.feed_pin), "dismissed from home, still on the board");
        assert_eq!(ideas_board(&cards).len(), 4);
        let card = cards.iter().find(|c| c.title == "Morning test run").unwrap();
        assert_eq!(card.idea_type_label(), "Automation");
        assert!(card.details.as_deref().unwrap().contains("full story"));
        assert_eq!(card.idea_action(), "every weekday at 9, morning test run");
        let day = 3_600_000u64;
        let now = 10 * 24 * day;
        let old = idea_card("old", "Old untouched", "body", 0);
        let young = idea_card("young", "Young untouched", "body", now - day);
        assert!(idea_rank(&old, now) < idea_rank(&young, now));
    }

    fn many(n: usize, start: u64) -> Vec<crate::ideas::IdeaSeed> {
        (0..n)
            .map(|i| seed(crate::ideas::IdeaKind::Try, &format!("Errand{:02}", start as usize + i)))
            .collect()
    }

    #[test]
    fn board_keeps_fifteen_and_newer_cards_push_out_the_oldest() {
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        for (i, s) in many(IDEA_BOARD_MAX, 0).iter().enumerate() {
            post_generated_ideas(&mut cards, &mut pulse, 1_000 + i as u64, std::slice::from_ref(s), &[], "");
        }
        assert_eq!(ideas_board(&cards).len(), IDEA_BOARD_MAX);
        let oldest = cards.iter().min_by_key(|c| c.created_at).unwrap().id.clone();
        unpin_feed_idea(&mut cards, &oldest);
        post_generated_ideas(&mut cards, &mut pulse, 5_000, &many(1, 50), &[], "");
        assert_eq!(ideas_board(&cards).len(), IDEA_BOARD_MAX, "still fifteen");
        assert!(!cards.iter().any(|c| c.id == oldest), "the oldest was pushed out");
        assert!(ideas_board(&cards)[0].title == "Errand50", "newest first");
        assert!(expire_ideas(&mut cards, u64::MAX) == 0, "ideas do not time out");
    }

    #[test]
    fn modified_ideas_stay_outside_the_cap_up_to_ten() {
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        for (i, s) in many(IDEA_BOARD_MAX, 0).iter().enumerate() {
            post_generated_ideas(&mut cards, &mut pulse, 1_000 + i as u64, std::slice::from_ref(s), &[], "");
        }
        let oldest = cards.iter().min_by_key(|c| c.created_at).unwrap().id.clone();
        set_idea_draft(&mut cards, &oldest, "every day at 7, do it my way").unwrap();
        let card = cards.iter().find(|c| c.id == oldest).unwrap();
        assert!(card.modified && !card.feed_pin);
        assert_eq!(card.idea_action(), "every day at 7, do it my way");
        for i in 0..30u64 {
            post_generated_ideas(&mut cards, &mut pulse, 10_000 + i, &many(1, 100 + i), &[], "");
        }
        assert!(cards.iter().any(|c| c.id == oldest), "a modified card is never pushed out");
        assert_eq!(
            cards.iter().filter(|c| c.kind == UpdateKind::Idea && !c.modified).count(),
            IDEA_BOARD_MAX,
            "fifteen unmodified beside it"
        );
        assert_eq!(ideas_board(&cards)[0].id, oldest, "the one you work on sits at the top");
        let ids: Vec<String> = cards
            .iter()
            .filter(|c| !c.modified)
            .take(IDEA_MODIFIED_MAX)
            .map(|c| c.id.clone())
            .collect();
        for id in ids.iter().take(IDEA_MODIFIED_MAX - 1) {
            mark_idea_modified(&mut cards, id).unwrap();
        }
        assert_eq!(modified_ideas(&cards), IDEA_MODIFIED_MAX);
        let extra = cards.iter().find(|c| !c.modified).unwrap().id.clone();
        assert!(mark_idea_modified(&mut cards, &extra).unwrap_err().contains("Apply or delete"));
        assert!(mark_idea_modified(&mut cards, &oldest).is_ok(), "an open one stays editable");
        set_idea_draft(&mut cards, &oldest, "  ").unwrap();
        assert!(cards.iter().find(|c| c.id == oldest).unwrap().draft.is_none(), "blank goes back to the original");
    }

    #[test]
    fn review_skills_become_skill_ideas_once() {
        use crate::review::{LearnedSuggestion, SuggestionKind};
        let item = LearnedSuggestion {
            kind: SuggestionKind::Skill,
            title: "Release checklist".into(),
            body: "Bumps the version and tags the release in order.".into(),
            seed: None,
            name: Some("release-checklist".into()),
            trigger: Some("cutting a release".into()),
            instructions: Some("1. bump\n2. tag".into()),
            provider: None,
            tool: None,
        };
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        assert!(post_skill_idea(&mut cards, &mut pulse, 10, &item, &[]));
        assert!(!post_skill_idea(&mut cards, &mut pulse, 11, &item, &[]), "once");
        let c = &cards[0];
        assert_eq!(c.idea_type_label(), "Skill");
        assert!(c.details.as_deref().unwrap().contains("Steps:\n1. bump"));
        assert!(!c.feed_pin, "nightly skills wait on the Ideas board, not the home feed");
        let mut fresh = Vec::new();
        assert!(!post_skill_idea(&mut fresh, &mut pulse, 12, &item, &["release-checklist"]), "you already have it");
    }

    #[test]
    fn template_cards_are_purged_unless_you_used_them() {
        let mut cards = vec![
            idea_card("profile", "A chip for the next slice", "A chip does the next slice.", 1),
            idea_card("profile", "Set up: exit 1 · 9ms", "Turn this into a reminder.", 1),
            idea_card("remind-later", "Remind me later", "Say it once.", 1),
            idea_card("gen", "Morning test run", "Runs your tests before you sit down.", 1),
            digest_card("d", "Digest", "brief", 1),
        ];
        cards[2].built = true;
        assert_eq!(purge_template_ideas(&mut cards), 2);
        let titles: Vec<&str> = cards.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["Remind me later", "Morning test run", "Digest"]);
        assert_eq!(purge_template_ideas(&mut cards), 0);
    }

    #[test]
    fn older_updates_json_loads_without_a_prompt() {
        let raw = r#"[{"id":"i1","kind":"idea","title":"Old idea","createdAt":1,"status":"unread"}]"#;
        let cards: Vec<UpdateCard> = serde_json::from_str(raw).expect("old feed loads");
        assert!(cards[0].prompt.is_none());
        let back = serde_json::to_string(&cards[0]).unwrap();
        assert!(!back.contains("prompt"), "{back}");
    }

    #[test]
    fn generator_does_not_retract_an_existing_idea() {
        let mut cards = vec![idea_card("a", "more F1", "brief", 1)];
        let mut pulse = FeedPulse {
            idea_titles: vec!["more F1".into()],
            last_expiry_ms: 1,
            expiry_ms: 10_000,
            last_quiet_release_ms: 1,
            quiet_release_ms: 10_000,
            ..FeedPulse::default()
        };
        let _ = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 80,
                quiet: false,
            },
            material("more F1", &[], &[]),
        );
        assert_eq!(
            cards.iter().filter(|c| c.kind == UpdateKind::Idea).count(),
            1
        );
    }

    #[test]
    fn unread_digest_waits_and_reaction_is_taste() {
        let mut cards = vec![digest_card("d", "Digest 1", "brief", 10)];
        let mut pulse = FeedPulse {
            digest_ms: 1,
            ..FeedPulse::default()
        };
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 20,
                quiet: false,
            },
            material("more F1", &[], &[]),
        );
        assert!(!tick.digest_posted);
        assert_eq!(pulse.last_digest_ms, 20);
        cards[0].reaction = Some(CardReaction::Up);
        cards[0].status = UpdateStatus::Opened;
        pulse.digest_ms = 1;
        pulse.last_digest_ms = 20;
        let taste = [TasteNote {
            title: "Digest 1".into(),
            reaction: Some(CardReaction::Up),
            said: "more of that".into(),
        }];
        let links = [CitedLink {
            url: "https://news.example/f1".into(),
            label: "Source".into(),
        }];
        let edition = DigestEdition {
            found: true,
            refused: false,
            title: "More F1",
            body: "The next race. https://news.example/f1",
        };
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 30,
                quiet: false,
            },
            with_edition(material("more F1", &links, &taste), edition),
        );
        assert!(tick.digest_posted);
        let digest = cards
            .iter()
            .find(|c| c.kind == UpdateKind::Digest && c.created_at == 30)
            .unwrap();
        assert!(digest.body.as_deref().unwrap().contains("marked up"));
        assert!(digest.body.as_deref().unwrap().contains("You said"));
        assert!(digest.body.as_deref().unwrap().contains("more of that"));
    }

    #[test]
    fn passes_can_be_turned_off() {
        let mut cards = vec![idea_card("a", "more F1", "brief", 0)];
        cards[0].expires_at = Some(1);
        let mut held = automation_done_card("loop", "done", "ok", 1);
        held.held = true;
        cards.push(held);
        let mut pulse = FeedPulse {
            expiry_on: false,
            quiet_release_on: false,
            digest_on: false,
            ..FeedPulse::default()
        };
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 5_000,
                quiet: false,
            },
            material("more F1", &[], &[]),
        );
        assert_eq!(tick.expired, 0);
        assert_eq!(tick.released, 0);
        assert!(!tick.digest_posted);
        assert!(cards.iter().any(|c| c.kind == UpdateKind::Idea));
        assert!(cards.iter().any(|c| c.held));
    }

    #[test]
    fn resume_opens_a_fresh_chat_only_when_the_pane_is_not_already_empty() {
        assert!(!resume_needs_fresh_chat(0, false));
        assert!(resume_needs_fresh_chat(3, false));
        assert!(resume_needs_fresh_chat(0, true));
    }

    #[test]
    fn archive_keeps_a_digest_off_the_event_slot() {
        let mut cards = vec![digest_card("d", "Digest 1", "brief", 4)];
        let id = cards[0].id.clone();
        assert!(archive_digest(&mut cards, &id));
        assert!(visible_digests(&cards).is_empty());
        assert_eq!(archived_digests(&cards).len(), 1);
        assert!(visible_updates(&cards).is_empty());
        assert!(!feed_visible(&cards));
    }

    #[test]
    fn failed_automation_card_names_the_job_and_why() {
        let c = automation_failed_card("a1", "Board summary", "credit limit reached", 5);
        assert_eq!(c.kind, UpdateKind::AutomationDone);
        assert_eq!(c.title, "Board summary failed");
        assert_eq!(c.body.as_deref(), Some("credit limit reached"));
        assert_eq!(c.action, Some(UpdateAction::OpenAutomations));
        let blank = automation_failed_card("a2", "", "", 5);
        assert_eq!(blank.title, "Automation failed");
        assert!(blank.body.is_some());
        let done = automation_done_card("a1", "Board summary", "ok", 5);
        assert_ne!(c.id, done.id, "a failure must not replace the done card");
    }

    #[test]
    fn card_action_line_rewrites_the_action_once() {
        let reply = "Sure, weekdays only.\n\n**CARD_ACTION: every weekday at 8 summarize my inbox**";
        let (clean, action) = take_card_action(reply).expect("action");
        assert_eq!(action, "every weekday at 8 summarize my inbox");
        assert!(clean.contains("weekdays only"));
        assert!(clean.contains(CARD_ACTION_DONE));
        assert!(!clean.contains(CARD_ACTION_TAG));
        assert!(take_card_action(&clean).is_none(), "second pass finds nothing");
        assert!(take_card_action("CARD_ACTION:   ").is_none());
    }

    #[test]
    fn card_action_comes_from_the_reply_not_a_thought() {
        let turn = "TURN_THOUGHT:\nMaybe end with CARD_ACTION: every hour ping me\nTURN_TOOL: done\tRead\tnotes.md\nTURN_SAY:\nWeekdays it is.\nCARD_ACTION: every weekday at 8 sort my inbox\n";
        let (clean, action) = take_card_action(turn).expect("action");
        assert_eq!(action, "every weekday at 8 sort my inbox");
        assert!(clean.contains("Maybe end with CARD_ACTION: every hour ping me"), "the thought stays as it was");
        assert!(clean.contains(CARD_ACTION_DONE));
        let says = crate::turn_timeline::turn_says(&clean).expect("still a timeline");
        assert!(says.contains("Weekdays it is.") && says.contains(CARD_ACTION_DONE));
        let thought_only = "TURN_THOUGHT:\nI could say CARD_ACTION: every hour ping me\nTURN_SAY:\nWhat time suits you?\n";
        assert!(take_card_action(thought_only).is_none());
        let two = "First: CARD_ACTION: a\nThen\nCARD_ACTION: every day at 9 plan my day";
        assert_eq!(take_card_action(two).unwrap().1, "every day at 9 plan my day");
    }

    #[test]
    fn idea_card_brief_names_type_action_and_tag() {
        let mut c = idea_card("i1", "Inbox sweep", "Sort mail at 8", 1);
        c.idea_kind = Some(crate::ideas::IdeaKind::Automation);
        c.prompt = Some("every day at 8 sort my inbox".into());
        c.draft = Some("every weekday at 8 sort my inbox".into());
        let brief = idea_card_brief(&c);
        assert!(brief.contains("Type: Automation"));
        assert!(brief.contains("every weekday at 8 sort my inbox"));
        assert!(brief.contains(CARD_ACTION_TAG));
        assert!(idea_chat_open_line(&c).contains("this automation"));
    }

    #[test]
    fn one_suggestion_waits_for_an_old_pause_and_a_dismissed_source_stays_gone() {
        let paused = [PausedJob {
            id: "job-1",
            title: "Ship the harbor",
            detail: "Paused. This is where to resume.",
        }];
        let repeat = RepeatedAction {
            key: "open-cabin",
            label: "Open the cabin",
            count: 3,
            dismissed: false,
            automated: false,
        };
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        let first = post_help(&mut cards, &mut pulse, 1_000, false, false, &paused, &[repeat], "");
        assert!(first.ideas >= 1);
        assert!(!first.posted, "a fresh pause is not old enough to offer");
        let later = 1_000 + PAUSE_OFFER_MS;
        let second = post_help(&mut cards, &mut pulse, later, false, true, &paused, &[repeat], "");
        assert!(second.posted, "an old pause becomes the one suggestion");
        let suggestions: Vec<_> = cards
            .iter()
            .filter(|c| c.kind == UpdateKind::Suggestion)
            .map(|c| (c.title.clone(), c.body.clone(), c.source_id.clone()))
            .collect();
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].0.contains("still paused"));
        assert!(suggestions[0].1.as_deref().unwrap().contains("pick it back up"));
        let source = suggestions[0].2.clone();
        remember_dismissed_source(&mut pulse, &source);
        cards.retain(|c| c.kind != UpdateKind::Suggestion);
        let after = post_help(&mut cards, &mut pulse, later + PAUSE_OFFER_MS, false, false, &paused, &[], "");
        assert!(!after.posted);
        assert!(cards.iter().all(|c| c.kind != UpdateKind::Suggestion));
    }

    #[test]
    fn an_opened_situation_offer_is_not_posted_again() {
        let paused = [PausedJob {
            id: "job-1",
            title: "Ship the harbor",
            detail: "Paused. This is where to resume.",
        }];
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        post_help(&mut cards, &mut pulse, 1_000, false, false, &paused, &[], "");
        let later = 1_000 + PAUSE_OFFER_MS;
        assert!(post_help(&mut cards, &mut pulse, later, false, false, &paused, &[], "").posted);
        // Clicking the card opens its chat and marks it read.
        for c in cards.iter_mut().filter(|c| c.kind == UpdateKind::Suggestion) {
            c.status = UpdateStatus::Opened;
        }
        for step in 1..=3 {
            let tick = post_help(&mut cards, &mut pulse, later + step * 60_000, false, false, &paused, &[], "");
            assert!(!tick.posted, "the same offer came back on tick {step}");
        }
        assert_eq!(cards.iter().filter(|c| c.kind == UpdateKind::Suggestion).count(), 1);
    }

    #[test]
    fn a_turned_down_idea_stays_gone_even_reworded_or_from_the_review() {
        use crate::review::{LearnedSuggestion, SuggestionKind};
        let skill = |title: &str| LearnedSuggestion {
            kind: SuggestionKind::Skill,
            title: title.into(),
            body: "Reads recent sessions and lists what is left open.".into(),
            seed: None,
            name: Some("session-scanner".into()),
            trigger: Some("start of day".into()),
            instructions: Some("1. read\n2. list".into()),
            provider: None,
            tool: None,
        };
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        assert!(post_skill_idea(&mut cards, &mut pulse, 10, &skill("Session scanner"), &[]));
        let card = cards.iter().find(|c| c.kind == UpdateKind::Idea).cloned().unwrap();
        // Delete on the Ideas board.
        assert!(dismiss_idea(&mut cards, &card.id));
        remember_dismissed_source(&mut pulse, &card.source_id);
        remember_turned_down(&mut pulse, &card.title);
        assert!(!post_skill_idea(&mut cards, &mut pulse, 20, &skill("Session scanner"), &[]));
        assert!(
            !post_skill_idea(&mut cards, &mut pulse, 30, &skill("Scan my sessions"), &[]),
            "a reworded title on the same topic"
        );
        let mut other = skill("Release checklist");
        other.name = Some("release-checklist".into());
        assert!(post_skill_idea(&mut cards, &mut pulse, 40, &other, &[]), "other topics still post");
        use crate::ideas::IdeaKind::*;
        assert_eq!(
            post_generated_ideas(&mut cards, &mut pulse, 50, &[seed(Try, "Session scanner")], &[], ""),
            0
        );
        assert_eq!(
            turned_down_titles(&pulse, &["Release checklist".to_string()]),
            vec!["Session scanner".to_string()],
            "newest no first; titles still on the board are not turned down"
        );
    }

    #[test]
    fn title_memory_keeps_the_newest_once_full() {
        let mut pulse = FeedPulse::default();
        for i in 0..IDEA_TITLE_MEMORY + 5 {
            remember_idea_title(&mut pulse, &format!("Idea {i}"));
        }
        assert_eq!(pulse.idea_titles.len(), IDEA_TITLE_MEMORY);
        assert_eq!(
            pulse.idea_titles.last().map(String::as_str),
            Some(format!("Idea {}", IDEA_TITLE_MEMORY + 4).as_str()),
            "a full memory still records the newest title"
        );
        assert!(!pulse.idea_titles.iter().any(|t| t == "Idea 0"));
        for i in 0..TURNED_DOWN_MEMORY + 3 {
            remember_turned_down(&mut pulse, &format!("No {i}"));
        }
        remember_turned_down(&mut pulse, "no 10");
        assert_eq!(pulse.turned_down.len(), TURNED_DOWN_MEMORY);
        assert_eq!(pulse.turned_down.last().map(String::as_str), Some("no 10"));
        assert_eq!(pulse.turned_down.iter().filter(|t| t.eq_ignore_ascii_case("no 10")).count(), 1);
    }

    #[test]
    fn lookup_with_no_url_posts_an_honest_empty_card() {
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        let edition = DigestEdition {
            found: false,
            refused: false,
            title: "",
            body: "NONE",
        };
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow { now_ms: 10, quiet: false },
            with_edition(material("the harbor", &[], &[]), edition),
        );
        assert!(tick.digest_posted);
        assert!(tick.digest_consumed);
        let body = cards[0].body.as_deref().unwrap();
        assert!(body.contains("did not find a source"));
        assert!(cards[0].citations.is_empty());
    }

    #[test]
    fn five_runs_of_one_loop_stay_one_card_with_the_newest_body() {
        let mut cards = Vec::new();
        for n in 1..=5 {
            post_update(
                &mut cards,
                automation_done_card("loop-a", "Host snapshot", &format!("report word {n}"), 1_000 * n),
            );
        }
        let visible = visible_updates(&cards);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].runs, 5);
        assert_eq!(visible[0].body.as_deref(), Some("report word 5"));
        assert!(visible[0].id.starts_with("done-loop-a-"));
        assert!(visible[0]
            .why
            .as_deref()
            .unwrap()
            .contains("5 runs since you last looked"));
    }

    #[test]
    fn two_loops_stay_two_cards() {
        let mut cards = Vec::new();
        post_update(
            &mut cards,
            automation_done_card("loop-a", "Alpha snapshot", "alpha words", 1_000),
        );
        post_update(
            &mut cards,
            automation_done_card("loop-b", "Beta snapshot", "beta words", 2_000),
        );
        assert_eq!(visible_updates(&cards).len(), 2);
    }

    #[test]
    fn a_failure_replaces_the_done_card_in_the_same_group() {
        let mut cards = Vec::new();
        post_update(
            &mut cards,
            automation_done_card("loop-a", "Board summary", "all quiet", 10),
        );
        let kept = visible_updates(&cards)[0].id.clone();
        post_update(
            &mut cards,
            automation_failed_card("loop-a", "Board summary", "credit limit", 20),
        );
        let visible = visible_updates(&cards);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, kept);
        assert_eq!(visible[0].runs, 2);
        assert_eq!(visible[0].title, "Board summary failed");
        assert_eq!(visible[0].body.as_deref(), Some("credit limit"));
        assert!(visible[0]
            .why
            .as_deref()
            .unwrap()
            .contains("failed and needs a look"));
        assert_eq!(group_key(&visible[0]).as_deref(), Some("run:loop-a"));
    }

    #[test]
    fn collapse_feed_folds_four_done_cards_and_stays_put() {
        let mut cards = Vec::new();
        for ms in [10_u64, 20, 30, 40] {
            let mut card = automation_done_card("loop-a", "Host snapshot", "same report", ms);
            card.board_id = Some("board-1".into());
            cards.push(card);
        }
        assert_eq!(cards.len(), 4);
        assert!(collapse_feed(&mut cards));
        let visible = visible_updates(&cards);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].runs, 4);
        assert_eq!(visible[0].created_at, 40);
        assert_eq!(visible[0].board_id.as_deref(), Some("board-1"));
        assert_eq!(cards.iter().filter(|c| c.collapsed).count(), 3);
        assert!(archived_digests(&cards).is_empty());
        assert!(!collapse_feed(&mut cards));
        assert_eq!(visible_updates(&cards)[0].runs, 4);
    }

    #[test]
    fn dismiss_clears_the_group_and_the_same_text_stays_down_for_a_day() {
        let mut cards = Vec::new();
        for ms in [10_u64, 20] {
            cards.push(automation_done_card("loop-a", "Host snapshot", "alpha report", ms));
        }
        let id = cards.iter().max_by_key(|c| c.created_at).unwrap().id.clone();
        assert!(dismiss_feed_group(&mut cards, &id));
        assert!(visible_updates(&cards).is_empty());
        assert!(cards.iter().all(|c| !c.collapsed));
        post_update(
            &mut cards,
            automation_done_card("loop-a", "Host snapshot", "alpha report", 30),
        );
        assert!(visible_updates(&cards).is_empty(), "same text within a day is dropped");
        post_update(
            &mut cards,
            automation_done_card("loop-a", "Host snapshot", "beta findings", 40),
        );
        let visible = visible_updates(&cards);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].runs, 1);
        assert_eq!(visible[0].body.as_deref(), Some("beta findings"));
    }

    #[test]
    fn muted_failure_shows_once_per_day_and_a_success_stays_hidden() {
        let mut pulse = FeedPulse::default();
        mute_home_source(&mut pulse, "loop-a");
        let fail = automation_failed_card("loop-a", "Host snapshot", "disk full", 1_000);
        let first = home_post_gate(&pulse, &fail, 1_000);
        assert!(!first.skip && first.stamp_failure);
        note_muted_failure(&mut pulse, "loop-a", 1_000);
        let second = home_post_gate(&pulse, &fail, 1_000 + 60_000);
        assert!(second.skip && !second.stamp_failure);
        let later = home_post_gate(&pulse, &fail, 1_000 + DAY_MS);
        assert!(!later.skip && later.stamp_failure);
        let done = automation_done_card("loop-a", "Host snapshot", "all quiet", 2_000);
        assert!(home_post_gate(&pulse, &done, 2_000).skip);
        let mut board = Vec::new();
        let (id, is_new) = crate::file_follow_up(
            &mut board,
            "loop-a",
            "Host snapshot",
            "summarize the disk and report what changed",
            5,
        );
        let (again, is_new_again) = crate::file_follow_up(
            &mut board,
            "loop-a",
            "Host snapshot",
            "summarize the disk again with a fresh report",
            6,
        );
        assert!(is_new && !is_new_again);
        assert_eq!(id, again);
        assert_eq!(board.len(), 1);
        assert!(board[0].report.contains("fresh report"));
    }

    #[test]
    fn same_text_from_another_source_is_dropped_for_a_day() {
        let mut cards = Vec::new();
        post_update(
            &mut cards,
            automation_done_card("loop-a", "Same headline", "same story words", 1_000),
        );
        post_update(
            &mut cards,
            automation_done_card("loop-b", "Same headline", "same story words", 2_000),
        );
        assert_eq!(visible_updates(&cards).len(), 1);
        post_update(
            &mut cards,
            automation_done_card("loop-b", "Same headline", "same story words", 1_000 + DAY_MS),
        );
        assert_eq!(visible_updates(&cards).len(), 2);
    }

    #[test]
    fn quiet_hours_release_does_not_burst_grouped_runs() {
        let mut cards = Vec::new();
        for n in 1..=5 {
            let mut card = automation_done_card("loop-a", "Host snapshot", &format!("edition word {n}"), 1_000 * n);
            hold_if_quiet(&mut card, true);
            post_update(&mut cards, card);
        }
        assert!(!feed_visible(&cards));
        assert_eq!(cards.iter().filter(|c| c.held).count(), 1);
        assert_eq!(release_quiet_hold(&mut cards), 1);
        let visible = visible_updates(&cards);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].runs, 5);
        assert_eq!(visible[0].body.as_deref(), Some("edition word 5"));
    }

    #[test]
    fn older_event_json_round_trips_without_the_new_fields() {
        let raw = r#"[{"id":"d1","kind":"automation_done","title":"Old run","createdAt":5,"status":"unread","sourceId":"loop-a"}]"#;
        let cards: Vec<UpdateCard> = serde_json::from_str(raw).expect("old feed loads");
        assert_eq!(cards[0].runs, 1);
        assert_eq!(cards[0].first_at, 0);
        assert!(!cards[0].collapsed);
        assert!(cards[0].prompt.is_none());
        let back = serde_json::to_string(&cards[0]).unwrap();
        assert!(!back.contains("prompt"), "{back}");
        assert!(!back.contains("runs"), "{back}");
        assert!(!back.contains("collapsed"), "{back}");
        assert!(!back.contains("firstAt"), "{back}");
        let mut kept = cards[0].clone();
        kept.runs = 3;
        kept.first_at = 5;
        let again: UpdateCard = serde_json::from_str(&serde_json::to_string(&kept).unwrap()).unwrap();
        assert_eq!(again.runs, 3);
        assert_eq!(again.first_at, 5);
        assert!(!again.collapsed);
    }

    #[test]
    fn paint_cap_is_three() {
        assert_eq!(FEED_PAINT_MAX, 3);
    }

    #[test]
    fn empty_source_groups_by_normalized_title_and_clocks_do_not_split_it() {
        let morning = automation_done_card("", "Meet at 9:00", "notes", 1);
        let evening = automation_done_card("", "Meet at 10:30 pm", "notes", 2);
        assert_eq!(group_key(&morning), group_key(&evening));
        assert!(group_key(&idea_card("i", "Meet at 9:00", "notes", 1)).is_none());
        let mut pulse = FeedPulse::default();
        let card = automation_done_card("loop-a", "Host snapshot report", "done", 1);
        mute_less_like(&mut pulse, &card, 1_000);
        assert!(less_blocks(&pulse, &card, 1_000 + DAY_MS));
        assert!(!less_blocks(&pulse, &card, 1_000 + LESS_MUTE_MS));
        let schedule = schedule_created_card("auto-1", "Board", "weekdays at 09:00", 8);
        assert_eq!(schedule.why.as_deref(), Some("You saved this schedule."));
        assert_eq!(
            runs_latest_line(3, 0, 90 * 60_000, 15, 0).as_deref(),
            Some("×3 runs · latest 13:30")
        );
    }

    #[test]
    fn a_collapsed_digest_stays_out_of_the_archive() {
        let mut card = digest_card("d", "Sunday", "brief", 4);
        card.status = UpdateStatus::Dismissed;
        card.collapsed = true;
        assert!(archived_digests(&[card]).is_empty());
    }
}
