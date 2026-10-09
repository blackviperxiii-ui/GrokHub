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
/// "Less like this" (card menu before 2.10.86) keeps that group off Home for two weeks.
pub const LESS_MUTE_MS: u64 = 14 * DISMISS_HIDE_MS;
/// Shown on Automations when this source's runs are hidden from Home.
pub const HOME_HIDDEN_NOTE: &str = "Hidden from Home · Undo";
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
/// A dismissed event stays down this long. A failure still posts inside the window.
/// A muted automation's failure uses the same window as its once-a-day floor.
const DISMISS_HIDE_MS: u64 = 24 * 60 * 60 * 1000;
const FLOOR_WINDOW_MS: u64 = DISMISS_HIDE_MS;
const TITLE_CHARS: usize = 72;
const BODY_CHARS: usize = 160;
const DIGEST_BODY_CHARS: usize = 900;
/// The written edition's share of a digest body, leaving room for the taste
/// and steer lines `compose_digest` adds after it.
const DIGEST_EDITION_CHARS: usize = 680;
/// What the feed paints under a title. A stored body can be a long dump.
/// Two short sentences, cut on a sentence or a word, never mid-word.
pub const TAKEAWAY_MAX: usize = 220;
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
    /// GrokHub added, changed, or removed a skill, connection, or automation
    /// on its own (Spike-5b). Undo lives on its Work-tree row.
    SelfChange,
    /// GrokHub did a small, undoable thing on its own under the autonomy
    /// ceiling (Spike-6b). Undo and "Don't do this again" sit on the card.
    DoneForYou,
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
            Self::SelfChange => "Changed",
            Self::DoneForYou => "Done for you",
        }
    }

    pub fn event(self) -> bool {
        matches!(
            self,
            Self::AutomationDone
                | Self::ScheduleCreated
                | Self::Suggestion
                | Self::AutomateOffer
                | Self::SelfChange
                | Self::DoneForYou
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
    /// Runs this card stands for. Repeats of one automation share a card and add one.
    #[serde(default = "default_runs", skip_serializing_if = "runs_is_one")]
    pub runs: u32,
    /// When the user dismissed this event. A repeat stays hidden for a day.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dismissed_at: u64,
    /// Done-for-you cards (Spike-6b): what Undo and "Don't do this again" act on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_for_you: Option<DoneForYou>,
    /// Pulse page fields: category, snooze, due time, source images.
    #[serde(default, flatten)]
    pub pulse: crate::pulse::PulseMeta,
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

fn runs_is_one(runs: &u32) -> bool {
    *runs == 1
}

fn is_zero(n: &u64) -> bool {
    *n == 0
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
    /// Automation sources whose runs stay off Home until Undo. Failures can still use the floor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub muted_sources: Vec<String>,
    /// Group key → unix ms when a "Less like this" mute ends. Set only by builds before 2.10.86.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub less_until: BTreeMap<String, u64>,
    /// Source → `created_at` of the failure Home last admitted through the safety floor.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub floor_shown: BTreeMap<String, u64>,
    /// The Home deck and the Ideas board moved into Pulse (`crate::pulse::migrate_to_pulse`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub pulse_v1: bool,
    /// Like and dislike ledger lines since the feed instructions were last rewritten.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub taste_since_rewrite: u32,
}

fn is_zero_u32(n: &u32) -> bool {
    *n == 0
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
            less_until: BTreeMap::new(),
            floor_shown: BTreeMap::new(),
            pulse_v1: false,
            taste_since_rewrite: 0,
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
    /// Every URL in the whole reply, taken before the body is clipped.
    pub links: Vec<CitedLink>,
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
        .filter(|c| c.kind == UpdateKind::Digest && c.status == UpdateStatus::Dismissed)
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

pub fn post_update(cards: &mut Vec<UpdateCard>, card: UpdateCard) {
    let now = card.created_at;
    if let Some(key) = feed_group_key(&card) {
        if let Some(existing) = take_live_group(cards, &key) {
            cards.retain(|c| !is_group_tombstone(c, &key));
            cards.push(merge_event(existing, card));
            trim_feed_store(cards, now);
            return;
        }
        let fresh_tombstone = cards.iter().any(|c| is_group_tombstone(c, &key) && tombstone_fresh(c, now));
        if fresh_tombstone && !card_is_failure(&card) {
            trim_feed_store(cards, now);
            return;
        }
        cards.retain(|c| !is_group_tombstone(c, &key));
    }
    let id = card.id.clone();
    cards.retain(|c| c.id != id);
    cards.push(card);
    trim_feed_store(cards, now);
}

fn is_group_tombstone(card: &UpdateCard, key: &str) -> bool {
    card.status == UpdateStatus::Dismissed && feed_group_key(card).as_deref() == Some(key)
}

/// Pull every live card in `key` out of the store. The one returned is the newest,
/// with `runs` already the sum of the group, and its discuss thread or reaction
/// kept when an older copy was the one the user touched.
fn take_live_group(cards: &mut Vec<UpdateCard>, key: &str) -> Option<UpdateCard> {
    let live: Vec<usize> = cards
        .iter()
        .enumerate()
        .filter_map(|(i, card)| {
            (feed_group_key(card).as_deref() == Some(key) && card.status != UpdateStatus::Dismissed)
                .then_some(i)
        })
        .collect();
    let keep = newest_index(cards, &live)?;
    let mut base = cards[keep].clone();
    if base.discuss_thread.is_none() {
        base.discuss_thread = live
            .iter()
            .filter(|&&i| i != keep)
            .find_map(|&i| cards[i].discuss_thread.clone());
    }
    if base.reaction.is_none() {
        base.reaction = live.iter().filter(|&&i| i != keep).find_map(|&i| cards[i].reaction);
    }
    base.runs = live
        .iter()
        .fold(0u32, |acc, &i| acc.saturating_add(card_runs(&cards[i])));
    let mut drop_idx = live;
    drop_idx.sort_unstable();
    for i in drop_idx.into_iter().rev() {
        cards.remove(i);
    }
    Some(base)
}

/// The new run replaces the text. `runs` keeps counting until the card was opened.
/// An Opened card starts again at the incoming run: runs since you last looked.
fn merge_event(mut existing: UpdateCard, new: UpdateCard) -> UpdateCard {
    let opened = existing.status == UpdateStatus::Opened;
    existing.runs = if opened {
        card_runs(&new)
    } else {
        existing.runs.saturating_add(card_runs(&new))
    };
    existing.held = existing.held && new.held;
    existing.id = new.id;
    existing.kind = new.kind;
    existing.title = new.title;
    existing.body = new.body;
    existing.created_at = new.created_at;
    existing.status = UpdateStatus::Unread;
    existing.action = new.action;
    // A fresh run card does not carry the Follow up link. Keep the one already filed.
    if new.board_id.is_some() {
        existing.board_id = new.board_id;
    }
    existing.why = new.why;
    existing.dismissed_at = 0;
    existing.source_id = new.source_id;
    refresh_event_why(&mut existing);
    existing
}

fn card_runs(card: &UpdateCard) -> u32 {
    card.runs.max(1)
}

fn newest_index(cards: &[UpdateCard], idxs: &[usize]) -> Option<usize> {
    idxs.iter().copied().max_by(|&a, &b| {
        cards[a]
            .created_at
            .cmp(&cards[b].created_at)
            .then_with(|| cards[a].id.cmp(&cards[b].id))
    })
}

fn tombstone_fresh(card: &UpdateCard, now: u64) -> bool {
    card.dismissed_at > 0 && now.saturating_sub(card.dismissed_at) < DISMISS_HIDE_MS
}

/// A failed run. Dismissing the last success must not hide a failure.
fn card_is_failure(card: &UpdateCard) -> bool {
    if card.kind != UpdateKind::AutomationDone {
        return false;
    }
    if card.id.starts_with("fail-") || card.id.starts_with("crash-") {
        return true;
    }
    let title = card.title.to_ascii_lowercase();
    let body = card.body.as_deref().unwrap_or("").to_ascii_lowercase();
    title_or_body_says_failed(&title) || title_or_body_says_failed(&body)
}

fn title_or_body_says_failed(text: &str) -> bool {
    text == "automation failed" || text.starts_with("failed") || text.contains(" failed")
}

/// A failed automation run, from the card id or title. Not inferred from the body.
pub fn card_needs_you(card: &UpdateCard) -> bool {
    failed_run(card)
}

/// A failed automation run, from the card id or title. Not inferred from the body.
fn failed_run(card: &UpdateCard) -> bool {
    card.kind == UpdateKind::AutomationDone
        && (card.id.starts_with("fail-")
            || card.title == "Automation failed"
            || card.title.ends_with(" failed"))
}

fn automation_display_name(card: &UpdateCard) -> String {
    let title = card.title.trim();
    if failed_run(card) {
        if let Some(name) = title.strip_suffix(" failed") {
            let name = name.trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
        return "Automation".to_string();
    }
    if title.is_empty() {
        "Automation".to_string()
    } else {
        title.to_string()
    }
}

/// Fixed why line for a run, a failure, or a saved schedule. Other kinds keep `why`.
pub fn refresh_event_why(card: &mut UpdateCard) {
    let name = automation_display_name(card);
    match card.kind {
        UpdateKind::AutomationDone if failed_run(card) => {
            card.why = Some(format!("“{name}” failed and needs a look."));
        }
        UpdateKind::AutomationDone => {
            let mut line = if card.board_id.as_ref().is_some_and(|id| !id.trim().is_empty()) {
                format!("Your automation “{name}” finished and left a report in Follow up.")
            } else {
                format!("Your automation “{name}” finished.")
            };
            let runs = card.runs.max(1);
            if runs > 1 {
                line.push_str(&format!(" · {runs} runs since you last looked"));
            }
            card.why = Some(line);
        }
        UpdateKind::ScheduleCreated => {
            card.why = Some("You saved this schedule.".to_string());
        }
        _ => {}
    }
}

/// `×N runs · latest h:mm` when the card stands for more than one run.
/// `hour` and `minute` are the local clock now. `created_at` and `now_ms` are unix ms.
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

pub fn source_hidden(pulse: &FeedPulse, source: &str) -> bool {
    let source = source.trim();
    !source.is_empty() && pulse.muted_sources.iter().any(|saved| saved == source)
}

pub fn automation_home_note(pulse: &FeedPulse, source_id: &str) -> Option<&'static str> {
    if source_hidden(pulse, source_id) {
        Some(HOME_HIDDEN_NOTE)
    } else {
        None
    }
}

pub fn hide_home_source(pulse: &mut FeedPulse, source: &str) {
    let source = source.trim();
    if source.is_empty() || source_hidden(pulse, source) {
        return;
    }
    pulse.muted_sources.push(source.to_string());
}

pub fn unhide_home_source(pulse: &mut FeedPulse, source: &str) {
    let source = source.trim();
    pulse.muted_sources.retain(|saved| saved != source);
}

fn less_active(pulse: &FeedPulse, key: &str, now: u64) -> bool {
    pulse.less_until.get(key).is_some_and(|&until| until > now)
}

/// Mute this card's group until two weeks from `now`.
pub fn mute_less_like(pulse: &mut FeedPulse, card: &UpdateCard, now: u64) {
    let Some(key) = feed_group_key(card) else {
        return;
    };
    pulse
        .less_until
        .insert(key, now.saturating_add(LESS_MUTE_MS));
}

/// Drop an active Less mute for this card's group and source.
pub fn clear_less_mute(pulse: &mut FeedPulse, card: &UpdateCard) {
    if let Some(key) = feed_group_key(card) {
        pulse.less_until.remove(&key);
    }
    let source = card.source_id.trim();
    if source.is_empty() {
        return;
    }
    let suffix = format!(":{source}");
    pulse
        .less_until
        .retain(|key, _| key != source && !key.ends_with(&suffix));
}

fn home_suppressed(card: &UpdateCard, pulse: &FeedPulse, now: u64) -> bool {
    if source_hidden(pulse, &card.source_id) {
        return true;
    }
    if let Some(key) = feed_group_key(card) {
        if less_active(pulse, &key, now) {
            return true;
        }
    }
    false
}

fn floor_open(card: &UpdateCard, pulse: &FeedPulse, now: u64) -> bool {
    if !failed_run(card) {
        return false;
    }
    let source = card.source_id.trim();
    if source.is_empty() {
        return false;
    }
    match pulse.floor_shown.get(source) {
        None => true,
        Some(&at) if at == card.created_at => true,
        Some(&at) if now.saturating_sub(at) >= FLOOR_WINDOW_MS => true,
        Some(_) => false,
    }
}

/// Home deck filter. Hidden and Less-muted cards stay in the store and off Home.
/// A muted automation's failure still shows, at most once per 24 h per source.
pub fn surfaces_on_home(card: &UpdateCard, pulse: &FeedPulse, now: u64) -> bool {
    if !home_suppressed(card, pulse, now) {
        return true;
    }
    floor_open(card, pulse, now)
}

/// Event cards for the home deck, newest first, at most `FEED_PAINT_MAX`.
pub fn home_event_cards(cards: &[UpdateCard], pulse: &FeedPulse, now: u64) -> Vec<UpdateCard> {
    visible_updates(cards)
        .into_iter()
        .filter(|card| surfaces_on_home(card, pulse, now))
        .take(FEED_PAINT_MAX)
        .collect()
}

/// Remember a failure that Home is showing only because of the safety floor.
/// Returns true when `pulse` changed.
pub fn record_home_floors(pulse: &mut FeedPulse, shown: &[UpdateCard], now: u64) -> bool {
    let mut changed = false;
    for card in shown {
        if !failed_run(card) || !home_suppressed(card, pulse, now) {
            continue;
        }
        let source = card.source_id.trim();
        if source.is_empty() {
            continue;
        }
        if pulse.floor_shown.get(source) != Some(&card.created_at) {
            pulse
                .floor_shown
                .insert(source.to_string(), card.created_at);
            changed = true;
        }
    }
    changed
}

fn trim_feed_store(cards: &mut Vec<UpdateCard>, now: u64) {
    let mut ideas = Vec::new();
    let mut archive = Vec::new();
    let mut tombs = Vec::new();
    let mut live = Vec::new();
    for card in cards.drain(..) {
        if card.kind == UpdateKind::Idea {
            ideas.push(card);
        } else if card.kind == UpdateKind::Digest && card.status == UpdateStatus::Dismissed {
            archive.push(card);
        } else if card.kind.event() && card.status == UpdateStatus::Dismissed {
            if tombstone_fresh(&card, now) {
                tombs.push(card);
            }
        } else {
            live.push(card);
        }
    }
    sort_feed(&mut live);
    sort_feed(&mut archive);
    sort_feed(&mut ideas);
    tombs.sort_by(|a, b| {
        b.dismissed_at
            .cmp(&a.dismissed_at)
            .then(b.created_at.cmp(&a.created_at))
            .then(b.id.cmp(&a.id))
    });
    if live.len() > FEED_STORE_MAX {
        live.truncate(FEED_STORE_MAX);
    }
    if archive.len() > FEED_STORE_MAX {
        archive.truncate(FEED_STORE_MAX);
    }
    if tombs.len() > FEED_STORE_MAX {
        tombs.truncate(FEED_STORE_MAX);
    }
    cards.extend(live);
    cards.extend(tombs);
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
    let now = cards
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.created_at)
        .unwrap_or(0);
    dismiss_update_at(cards, id, now)
}

/// Dismiss one card. An event with a group key leaves a single tombstone stamped
/// `now`, so a repeat of that run stays hidden for a day. Ideas and digests
/// still leave the store (a digest archive uses `archive_digest`, not this).
pub fn dismiss_update_at(cards: &mut Vec<UpdateCard>, id: &str, now: u64) -> bool {
    let Some(pos) = cards.iter().position(|c| c.id == id) else {
        return false;
    };
    if let Some(key) = feed_group_key(&cards[pos]) {
        let mut tomb = cards[pos].clone();
        tomb.status = UpdateStatus::Dismissed;
        tomb.dismissed_at = now;
        tomb.held = false;
        cards.retain(|c| feed_group_key(c).as_deref() != Some(key.as_str()));
        cards.push(tomb);
        return true;
    }
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

/// What the person said on the card, in full. Idea drafts keep this.
fn posted_text(card: &UpdateCard) -> String {
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

/// Seed for Discuss on a digest or suggestion. Main chat gets the title, the
/// short takeaway, the source URL, and why it matters.
pub fn discuss_context(card: &UpdateCard) -> String {
    let takeaway = short_takeaway(card);
    let mut out = format!("Feed post: {}", card.title.trim());
    if !takeaway.is_empty() {
        out.push('\n');
        out.push_str(&takeaway);
    }
    if let Some(url) = card
        .citations
        .first()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
    {
        out.push_str("\nSource: ");
        out.push_str(url);
    }
    if let Some(why) = card
        .why
        .as_deref()
        .map(str::trim)
        .filter(|why| !why.is_empty() && !is_steer(why))
    {
        if !out.to_ascii_lowercase().contains(&why.to_ascii_lowercase()) {
            out.push_str("\nWhy it matters: ");
            out.push_str(why);
        }
    }
    out.push_str("\n\nYou opened this from your feed and want to act on it.");
    out
}

/// Opening line for the idea talk. The note under it is the editable draft.
pub fn idea_open_line(card: &UpdateCard) -> String {
    if card.prompt.is_some() {
        return format!(
            "{}\n\nThe draft below does it. Send it as is, or change it first.",
            posted_text(card)
        );
    }
    format!(
        "{}\n\nThis draft is what would help next time: why it came up, a quick chip, a skill, and an automation. Change the note if that is wrong. Do not copy the earlier chat back.",
        posted_text(card)
    )
}

/// Short context under a feed title: what is going on, then why it matters
/// when that line is short enough to fit. URLs and a repeated title stay out.
/// A digest's opening "I'll look up …" line is the model talking, not the news,
/// so it is skipped. A long stored body still comes back inside `TAKEAWAY_MAX`.
pub fn short_takeaway(card: &UpdateCard) -> String {
    takeaway_text(
        card.title.trim(),
        card.body.as_deref().unwrap_or(""),
        card.why.as_deref().unwrap_or(""),
        card.kind == UpdateKind::Digest,
    )
}

fn takeaway_text(title: &str, body: &str, why: &str, digest: bool) -> String {
    let prose = strip_title_prefix(&strip_http_urls(body), title);
    let mut sentences: Vec<String> = split_sentences(&prose)
        .into_iter()
        .filter(|sentence| !is_steer(sentence) && !same_line(sentence, title))
        .collect();
    if digest {
        sentences = skip_lead_ins(sentences);
    }
    let why = why.trim();
    if !why.is_empty() && !is_steer(why) && !same_line(why, title) && why.chars().count() <= 140 {
        let already = sentences
            .iter()
            .any(|sentence| sentence.to_ascii_lowercase().contains(&why.to_ascii_lowercase()));
        if !already {
            // What is going on, then why it matters.
            let at = sentences.len().min(1);
            sentences.insert(at, as_sentence(why));
        }
    }
    fit_sentences(&sentences, TAKEAWAY_MAX)
}

/// The model announcing itself ("I'll look up …", "Here are …").
fn is_lead_in(sentence: &str) -> bool {
    const LEADS: &[&str] = &[
        "i'll ",
        "i will ",
        "i'm going to ",
        "let me ",
        "here's ",
        "here is ",
        "here are ",
        "i looked up ",
        "i searched ",
        "below are ",
        "below is ",
    ];
    let lower = sentence.trim_start().to_ascii_lowercase().replace('\u{2019}', "'");
    LEADS.iter().any(|lead| lower.starts_with(lead))
}

/// Drop opening lead-in sentences. Kept when nothing else is left.
fn skip_lead_ins(sentences: Vec<String>) -> Vec<String> {
    let n = sentences.iter().take_while(|s| is_lead_in(s)).count();
    if n == 0 || n == sentences.len() {
        return sentences;
    }
    sentences.into_iter().skip(n).collect()
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
            // Two or more held cards fold into one Pulse digest card.
            let (n, _) = crate::pulse::release_quiet_batch(cards, now.now_ms);
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
        // The body already ends with the steer line; a `why` too painted it twice.
        return Some(digest_card("edition", &format!("Digest {n}"), &body, now));
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
    // The painted line is short. Keep the written edition when it was longer.
    if written.chars().count() > TAKEAWAY_MAX {
        card.details = Some(written.to_string());
    }
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

fn is_steer(text: &str) -> bool {
    text.to_ascii_lowercase()
        .contains("the brief steers the next edition")
}

fn same_line(a: &str, b: &str) -> bool {
    fn norm(text: &str) -> String {
        text.trim()
            .trim_end_matches(['.', '!', '?', ' '])
            .to_ascii_lowercase()
    }
    let a = norm(a);
    !a.is_empty() && a == norm(b)
}

fn as_sentence(text: &str) -> String {
    let text = text.trim();
    if text.ends_with(['.', '!', '?']) {
        text.to_string()
    } else {
        format!("{text}.")
    }
}

fn strip_http_urls(text: &str) -> String {
    let mut kept = Vec::new();
    for token in text.split_whitespace() {
        if http_url_in(token).is_some() {
            let cleaned = strip_url_token(token);
            if !cleaned.is_empty() {
                kept.push(cleaned);
            }
        } else {
            kept.push(token.to_string());
        }
    }
    kept.join(" ")
}

fn strip_url_token(token: &str) -> String {
    let Some(url) = http_url_in(token) else {
        return token.to_string();
    };
    let rest = token.replace(&url, "");
    let rest = rest.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
    if rest.chars().any(|c| c.is_alphanumeric()) {
        rest.to_string()
    } else {
        String::new()
    }
}

fn strip_title_prefix(text: &str, title: &str) -> String {
    let title = title.trim();
    let text = text.trim();
    if title.is_empty() {
        return text.to_string();
    }
    let n = title.chars().count();
    let head: String = text.chars().take(n).collect();
    if !head.eq_ignore_ascii_case(title) {
        return text.to_string();
    }
    let rest: String = text.chars().skip(n).collect();
    rest.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, ':' | '-' | '—' | '|' | ',' | '.' | ';')
    })
    .trim()
    .to_string()
}

/// End a sentence on `.` `!` `?` when the next letter is uppercase, or at the end.
/// `1.0.50` stays one token. A missing space (`tips.Grok`) still splits.
fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '.' || c == '!' || c == '?' {
            let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
            let next_digit = chars.get(i + 1).is_some_and(|n| n.is_ascii_digit());
            if c == '.' && prev_digit && next_digit {
                i += 1;
                continue;
            }
            let end = i + 1;
            let mut j = end;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let boundary =
                j >= chars.len() || c == '!' || c == '?' || chars[j].is_uppercase();
            if boundary {
                let sentence: String = chars[start..end].iter().collect();
                let sentence = sentence.trim().to_string();
                if !sentence.is_empty() {
                    out.push(sentence);
                }
                start = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if start < chars.len() {
        let rest: String = chars[start..].iter().collect();
        let rest = rest.trim().to_string();
        if !rest.is_empty() {
            out.push(rest);
        }
    }
    out
}

fn fit_sentences(sentences: &[String], max: usize) -> String {
    let mut out = String::new();
    for sentence in sentences.iter().take(2) {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }
        let next = if out.is_empty() {
            sentence.to_string()
        } else {
            format!("{out} {sentence}")
        };
        if next.chars().count() <= max {
            out = next;
            continue;
        }
        if out.is_empty() {
            return clip_words(sentence, max.saturating_sub(1));
        }
        break;
    }
    out
}

/// Prose stored from a lookup: no URLs, at most two sentences, and never
/// longer than the old edition cap.
fn edition_prose(raw: &str) -> String {
    let prose = strip_http_urls(raw);
    let max = TAKEAWAY_MAX.min(DIGEST_EDITION_CHARS);
    let fitted = fit_sentences(&skip_lead_ins(split_sentences(&prose)), max);
    if fitted.is_empty() {
        clip_words(&prose, max.saturating_sub(1))
    } else {
        fitted
    }
}

/// Like `clip_line`, but cuts between words so a URL is never cut in half.
/// A single word longer than `max_chars` still falls back to a character cut.
fn clip_words(raw: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    let mut cut = false;
    for word in raw.split_whitespace() {
        let n = word.chars().count() + usize::from(!out.is_empty());
        if used + n > max_chars {
            cut = true;
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
        used += n;
    }
    if out.is_empty() && cut {
        return clip_line(raw, max_chars);
    }
    if cut {
        out.push('…');
    }
    out
}

/// An http(s) URL whose host is a public name or address, not localhost, a
/// private range, or link-local. Used before fetching a URL a model gave.
pub fn public_http_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(v6) = hostport.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        hostport.split(':').next().unwrap_or("")
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || !host.contains(['.', ':'])
    {
        return false;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            !(ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified())
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            let seg = ip.segments()[0];
            !(ip.is_loopback()
                || ip.is_unspecified()
                || (seg & 0xfe00) == 0xfc00
                || (seg & 0xffc0) == 0xfe80)
        }
        Err(_) => true,
    }
}

/// Drop the URLs a check found dead, so only links that answered can be cited.
pub fn drop_dead_links(raw: &str, dead: &[String]) -> String {
    if dead.is_empty() {
        return raw.to_string();
    }
    raw.lines()
        .map(|line| {
            line.split_whitespace()
                .filter(|token| http_url_in(token).is_none_or(|url| !dead.contains(&url)))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn clip_line(raw: &str, max_chars: usize) -> String {
    let flat = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = flat.chars().take(max_chars).collect();
    if flat.chars().count() > max_chars {
        out.push('…');
    }
    out
}

fn feed_card_id(prefix: &str, source_id: &str, title: &str, created_at: u64, stable: bool) -> String {
    let source = source_id.trim();
    if stable {
        if source.is_empty() {
            format!("{prefix}-{}", title_hash(title))
        } else {
            format!("{prefix}-{source}")
        }
    } else if source.is_empty() {
        format!("{prefix}-{created_at}")
    } else {
        format!("{prefix}-{source}-{created_at}")
    }
}

/// Group key for one event source. Done and failed runs of the same automation
/// share `run:<source>`. Ideas and digests are not grouped here.
pub fn feed_group_key(card: &UpdateCard) -> Option<String> {
    let source = card.source_id.trim();
    if !source.is_empty() {
        let prefix = match card.kind {
            UpdateKind::AutomationDone => "run",
            UpdateKind::ScheduleCreated => "sched",
            UpdateKind::AutomateOffer => "offer",
            UpdateKind::Suggestion => "sugg",
            UpdateKind::SelfChange => "chg",
            UpdateKind::DoneForYou => "dfy",
            UpdateKind::Idea | UpdateKind::Digest => return None,
        };
        return Some(format!("{prefix}:{source}"));
    }
    let kind = match card.kind {
        UpdateKind::AutomationDone => "automation_done",
        UpdateKind::ScheduleCreated => "schedule_created",
        UpdateKind::AutomateOffer => "automate_offer",
        UpdateKind::Suggestion => "suggestion",
        UpdateKind::SelfChange => "self_change",
        UpdateKind::DoneForYou => "done_for_you",
        UpdateKind::Idea | UpdateKind::Digest => return None,
    };
    Some(format!("{kind}:{}", title_hash(&card.title)))
}

fn stable_event_id(card: &UpdateCard) -> String {
    let prefix = match card.kind {
        UpdateKind::AutomationDone => {
            if card.id.starts_with("fail-") {
                "fail"
            } else {
                "done"
            }
        }
        UpdateKind::ScheduleCreated => "sched",
        UpdateKind::Suggestion => "sugg",
        UpdateKind::AutomateOffer => "offer",
        UpdateKind::SelfChange => "chg",
        UpdateKind::DoneForYou => "dfy",
        UpdateKind::Idea | UpdateKind::Digest => return card.id.clone(),
    };
    feed_card_id(prefix, &card.source_id, &card.title, card.created_at, true)
}

/// Fold saved duplicate runs into one card per group. The newest live card stays,
/// `runs` is the sum of the group's runs, and its id is the stable one. A second
/// call changes nothing.
pub fn collapse_feed(cards: &mut Vec<UpdateCard>) -> bool {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, card) in cards.iter().enumerate() {
        if let Some(key) = feed_group_key(card) {
            groups.entry(key).or_default().push(i);
        }
    }
    let mut changed = false;
    let mut remove = Vec::new();
    let mut edits: Vec<(usize, UpdateCard)> = Vec::new();
    for idxs in groups.values() {
        if idxs.len() < 2 {
            continue;
        }
        let live: Vec<usize> = idxs
            .iter()
            .copied()
            .filter(|&i| cards[i].status != UpdateStatus::Dismissed)
            .collect();
        let pool: &[usize] = if live.is_empty() { idxs.as_slice() } else { &live };
        let Some(keep) = newest_index(cards, pool) else {
            continue;
        };
        let drop_set: Vec<usize> = idxs.iter().copied().filter(|&i| i != keep).collect();
        if drop_set.is_empty() {
            continue;
        }
        if !live.is_empty() {
            let runs = live
                .iter()
                .fold(0u32, |acc, &i| acc.saturating_add(card_runs(&cards[i])));
            let mut kept = cards[keep].clone();
            if kept.discuss_thread.is_none() {
                kept.discuss_thread = live
                    .iter()
                    .filter(|&&i| i != keep)
                    .find_map(|&i| cards[i].discuss_thread.clone());
            }
            if kept.reaction.is_none() {
                kept.reaction = live
                    .iter()
                    .filter(|&&i| i != keep)
                    .find_map(|&i| cards[i].reaction);
            }
            kept.runs = runs;
            kept.id = stable_event_id(&kept);
            edits.push((keep, kept));
        }
        remove.extend(drop_set);
        changed = true;
    }
    if !changed {
        return false;
    }
    for (i, card) in edits {
        cards[i] = card;
    }
    remove.sort_unstable();
    remove.dedup();
    for i in remove.into_iter().rev() {
        cards.remove(i);
    }
    true
}

/// Lowercase, collapse whitespace, and drop clock times and ASCII digits so
/// "Board at 9:05" and "Board at 10:30" share a key when the source id is empty.
fn normalize_title(title: &str) -> String {
    let chars: Vec<char> = title.to_ascii_lowercase().chars().collect();
    let mut stripped = String::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some(n) = clock_len(&chars[i..]) {
            i += n;
            continue;
        }
        stripped.push(chars[i]);
        i += 1;
    }
    let mut collapsed = String::new();
    let mut pending_space = false;
    for c in stripped.chars() {
        if c.is_ascii_digit() {
            continue;
        }
        if c.is_whitespace() {
            pending_space = !collapsed.is_empty();
            continue;
        }
        if pending_space {
            collapsed.push(' ');
            pending_space = false;
        }
        collapsed.push(c);
    }
    collapsed
}

fn clock_len(chars: &[char]) -> Option<usize> {
    let digits = |slice: &[char], max: usize| {
        slice.iter().take(max).take_while(|c| c.is_ascii_digit()).count()
    };
    let mut hours = digits(chars, 2);
    if hours == 0 {
        return None;
    }
    if hours == 2 && chars.get(2) != Some(&':') {
        hours = 1;
    }
    if chars.get(hours) != Some(&':') {
        return None;
    }
    let mut i = hours + 1;
    if digits(&chars[i..], 2) != 2 {
        return None;
    }
    i += 2;
    if chars.get(i) == Some(&':') && digits(&chars[i + 1..], 2) == 2 {
        i += 3;
    }
    Some(i)
}

/// FNV-1a 64-bit. `std::hash::DefaultHasher` is not stable across releases.
fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn title_hash(title: &str) -> String {
    format!("{:016x}", fnv1a64(normalize_title(title).as_bytes()))
}

pub(crate) fn blank_card(
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
        dismissed_at: 0,
        done_for_you: None,
        pulse: Default::default(),
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
        feed_card_id("done", source_id, &title, created_at, true),
        UpdateKind::AutomationDone,
        title,
        body,
        created_at,
    );
    card.action = Some(UpdateAction::OpenAutomations);
    card.source_id = source_id.trim().to_string();
    refresh_event_why(&mut card);
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
        feed_card_id("fail", source_id, &title, created_at, true),
        UpdateKind::AutomationDone,
        title,
        body,
        created_at,
    );
    card.action = Some(UpdateAction::OpenAutomations);
    card.source_id = source_id.trim().to_string();
    refresh_event_why(&mut card);
    card
}

/// Source prefix of a crash card: a run killed from outside (exit 143).
pub const CRASH_SOURCE_PREFIX: &str = "crash:";

/// A run was killed from outside (SIGTERM, exit 143) and no retry finished it.
/// The title names the job ("Crashed: Index ~/Projects (exit 143, killed)").
/// Open goes to its chat; Retry sends `retry` there. Same kind as a finished
/// automation so older feeds still load. One card per `source_id`: a second
/// crash of the same job bumps it instead of adding another.
pub fn crash_card(source_id: &str, job: &str, thread_id: &str, retry: &str, created_at: u64) -> UpdateCard {
    const TAIL: &str = " (exit 143, killed)";
    let job = clip_line(job, TITLE_CHARS - "Crashed: ".len() - TAIL.len());
    let job = if job.is_empty() { "Reply".to_string() } else { job };
    let title = format!("Crashed: {job}{TAIL}");
    let source = format!("{CRASH_SOURCE_PREFIX}{}", source_id.trim());
    let mut card = blank_card(
        feed_card_id("crash", &source, &title, created_at, true),
        UpdateKind::AutomationDone,
        title,
        Some("Something outside GrokHub stopped it before it finished. Retry runs it again.".into()),
        created_at,
    );
    if !thread_id.trim().is_empty() {
        card.action = Some(UpdateAction::OpenSession {
            thread_id: thread_id.trim().to_string(),
        });
    }
    card.prompt = Some(retry.trim().to_string()).filter(|r| !r.is_empty());
    card.source_id = source;
    refresh_event_why(&mut card);
    card
}

pub fn is_crash_card(card: &UpdateCard) -> bool {
    card.source_id.starts_with(CRASH_SOURCE_PREFIX)
}

pub const SCREEN_RECORDING_SOURCE_PREFIX: &str = "screenrec:";

/// A screen recording and its diagnosis. The title names the recording
/// ("Screen recording 0:42: video stutters on 4K YouTube"); the body is the
/// likely cause, or why it was not diagnosed. Open goes to the full report in
/// its chat; Delete recording removes the folder after a confirm.
pub fn screen_recording_card(dir: &str, title: &str, summary: &str, thread_id: &str, created_at: u64) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let summary = clip_line(summary, BODY_CHARS);
    let source = format!("{SCREEN_RECORDING_SOURCE_PREFIX}{}", dir.trim());
    let mut card = blank_card(
        feed_card_id("screenrec", &source, &title, created_at, true),
        UpdateKind::AutomationDone,
        title,
        (!summary.is_empty()).then_some(summary),
        created_at,
    );
    if !thread_id.trim().is_empty() {
        card.action = Some(UpdateAction::OpenSession {
            thread_id: thread_id.trim().to_string(),
        });
    }
    card.source_id = source;
    refresh_event_why(&mut card);
    card
}

/// The recording folder a screen recording card stands for.
pub fn screen_recording_dir(card: &UpdateCard) -> Option<&str> {
    card.source_id
        .strip_prefix(SCREEN_RECORDING_SOURCE_PREFIX)
        .filter(|d| !d.is_empty())
}

/// An audio check. The title names the input and the problem ("Audio check:
/// Yeti input clipping at -0.2 dBFS"); the body is the first fix and the
/// default output. A new check of the same input replaces the card. Open
/// goes to the full report in its chat.
pub fn audio_check_card(input: &str, title: &str, summary: &str, thread_id: &str, created_at: u64) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let summary = clip_line(summary, BODY_CHARS);
    let input = input.trim();
    let source = format!("audiocheck:{}", if input.is_empty() { "default" } else { input });
    let mut card = blank_card(
        feed_card_id("audiocheck", &source, &title, created_at, true),
        UpdateKind::AutomationDone,
        title,
        (!summary.is_empty()).then_some(summary),
        created_at,
    );
    if !thread_id.trim().is_empty() {
        card.action = Some(UpdateAction::OpenSession {
            thread_id: thread_id.trim().to_string(),
        });
    }
    card.source_id = source;
    refresh_event_why(&mut card);
    card
}

/// User saved a clock job or an interval loop.
/// Home update for a change GrokHub made on its own (Spike-5b): "GrokHub
/// added connection notes". `kind` is `skill`, `connection`, or
/// `automation`; `verb` is `added`, `changed`, or `removed`. Undo is on the
/// Work-tree row and in `/<kind>s changes`, never on the card.
pub fn self_change_card(kind: &str, name: &str, verb: &str, reason: &str, created_at: u64) -> UpdateCard {
    let name = clip_line(name, TITLE_CHARS);
    let title = clip_line(&format!("GrokHub {verb} {kind} {name}"), TITLE_CHARS);
    let why = clip_line(reason, TITLE_CHARS);
    let undo = format!("Undo it from its Work-tree row or /{kind}s changes.");
    let body = if why.is_empty() { undo } else { format!("{why} · {undo}") };
    let source = format!("{kind}:{name}");
    let mut card = blank_card(
        feed_card_id("chg", &source, &title, created_at, false),
        UpdateKind::SelfChange,
        title,
        Some(body),
        created_at,
    );
    if kind == "automation" {
        card.action = Some(UpdateAction::OpenAutomations);
    }
    card.source_id = source;
    card
}

/// Home update from the router (R2a): a model fell back, was retired, or
/// joined your plan. Tell-only; `source_id` keys it so one incident posts once.
pub fn router_update_card(source_id: &str, title: &str, body: &str, created_at: u64) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let body = clip_line(body, BODY_CHARS);
    let mut card = blank_card(
        feed_card_id("route", source_id, &title, created_at, false),
        UpdateKind::SelfChange,
        title,
        (!body.is_empty()).then_some(body),
        created_at,
    );
    card.source_id = source_id.trim().to_string();
    card
}

/// What a Done-for-you card points at: the ledger line it can undo and
/// the MindCheck class "Don't do this again" closes. Ids and keys only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoneForYou {
    /// Ledger kind (`connection`, `automation`, `skill`).
    pub kind: String,
    /// Ledger id of the target.
    pub target: String,
    /// The ledger line the act wrote (its `undo_ref`).
    pub undo_ref: u64,
    /// MindCheck key (`proactive:connection_disable`).
    pub mind_key: String,
    /// Undo or "Don't do this again" was clicked; the pills go away.
    #[serde(default, skip_serializing_if = "is_false")]
    pub answered: bool,
}

/// The Done-for-you card for one auto-act: what was done, why, and the
/// ledger line Undo reverts.
pub fn done_for_you_card(summary: &str, why: &str, done: DoneForYou, created_at: u64) -> UpdateCard {
    let title = clip_line(summary, TITLE_CHARS);
    let why = clip_line(why, TITLE_CHARS);
    let source = format!("{}:{}:{}", done.kind, done.target, done.undo_ref);
    let body = if why.is_empty() { "Done for you. Undo puts it back.".to_string() } else { why };
    let mut card = blank_card(
        feed_card_id("dfy", &source, &title, created_at, false),
        UpdateKind::DoneForYou,
        title,
        Some(body),
        created_at,
    );
    card.source_id = source;
    card.done_for_you = Some(done);
    card
}

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
        feed_card_id("sched", source_id, &title, created_at, true),
        UpdateKind::ScheduleCreated,
        title,
        body,
        created_at,
    );
    card.action = Some(UpdateAction::OpenAutomations);
    card.source_id = source_id.trim().to_string();
    refresh_event_why(&mut card);
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
        feed_card_id("sugg", source_id, &title, created_at, true),
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
        feed_card_id("offer", source_id, &title, created_at, true),
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
        feed_card_id("idea", source_id, &title, created_at, false),
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
        let body = clip_words(body, DIGEST_BODY_CHARS);
        if body.is_empty() {
            None
        } else {
            Some(body)
        }
    };
    let mut card = blank_card(
        feed_card_id("digest", source_id, &title, created_at, false),
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
            links: Vec::new(),
        };
    }
    if digest_topic_refused(raw) {
        return ParsedLookup {
            title: String::new(),
            body: raw.to_string(),
            found: false,
            refused: true,
            links: Vec::new(),
        };
    }
    let links = links_from_research(raw);
    if links.is_empty() {
        return ParsedLookup {
            title: String::new(),
            body: String::new(),
            found: false,
            refused: false,
            links: Vec::new(),
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
        body: edition_prose(raw),
        found: true,
        refused: false,
        links,
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
            // Name the job: "That job is still paused" left you guessing which one.
            let name = clip_line(job.title, TITLE_CHARS);
            if name.is_empty() {
                continue;
            }
            let source = format!("pause:{}", job.id.trim());
            let title = format!("Paused: {name}");
            let body = format!("Paused {}. Want me to pick it back up?", paused_ago(now.saturating_sub(seen)));
            if post_situation(cards, pulse, now, quiet, &source, &title, &body) {
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

/// How long a job has sat paused, in plain words ("2 hours ago").
fn paused_ago(ms: u64) -> String {
    let mins = ms / 60_000;
    let (n, unit) = match mins {
        0 => return "just now".into(),
        1..=59 => (mins, "minute"),
        60..=1_439 => (mins / 60, "hour"),
        _ => (mins / 1_440, "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// The id of the workboard card a paused-job suggestion is about.
pub fn paused_job_of(card: &UpdateCard) -> Option<&str> {
    if card.kind != UpdateKind::Suggestion {
        return None;
    }
    card.source_id.strip_prefix("pause:").map(str::trim).filter(|id| !id.is_empty())
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
    let source_key = source.trim();
    if source_gone(pulse, source)
        || has_unread_suggestion(cards)
        || cards.iter().any(|c| {
            c.kind == UpdateKind::Suggestion
                && c.status != UpdateStatus::Dismissed
                && c.source_id == source_key
        })
    {
        return false;
    }
    let mut card = suggestion_card(source, title, body, now);
    hold_if_quiet(&mut card, quiet);
    post_update(cards, card);
    cards.iter().any(|c| {
        c.kind == UpdateKind::Suggestion
            && c.status != UpdateStatus::Dismissed
            && c.source_id == source_key
    })
}

fn source_gone(pulse: &FeedPulse, source_id: &str) -> bool {
    let id = source_id.trim();
    !id.is_empty() && pulse.dismissed_sources.iter().any(|saved| saved == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_links_come_from_the_whole_reply_and_never_split() {
        let filler = "word ".repeat(200);
        let raw = format!("News note\n{filler}\nhttps://example.com/a-real-story");
        let parsed = parse_lookup(&raw);
        assert!(parsed.found);
        assert_eq!(parsed.links.len(), 1, "a link past the clip still counts");
        assert_eq!(parsed.links[0].url, "https://example.com/a-real-story");
        assert!(!parsed.body.contains("https://example.com/a-real"), "{}", parsed.body);
        assert!(parsed.body.chars().count() <= DIGEST_EDITION_CHARS + 1);
        assert!(
            parsed.body.chars().count() <= TAKEAWAY_MAX + 1,
            "a new edition stores a short body, got {} chars",
            parsed.body.chars().count()
        );
        let cut = clip_words("see https://example.com/long-path here", 20);
        assert_eq!(cut, "see…");
    }

    #[test]
    fn short_takeaway_stays_within_two_sentences_and_drops_urls() {
        let long = "I'll look up two real pieces that fit your Linux work. \
Grok Build alpha 1.0.50 changes how a cancel hits a live turn. \
Streams are retried instead of partial output and then the dump continues. \
https://xstack.grok.me/post ZEPHYRTAIL"
            .to_string();
        let mut card = digest_card("xstack", "For you", &long, 1);
        card.citations = vec!["https://xstack.grok.me/post".into()];
        card.why = Some("It changes the cancel button you use.".into());
        let take = short_takeaway(&card);
        assert!(
            take.chars().count() <= TAKEAWAY_MAX,
            "takeaway over budget: {} {take}",
            take.chars().count()
        );
        // The model's own lead-in is not the news.
        assert!(!take.contains("two real pieces"), "{take}");
        assert_eq!(
            take,
            "Grok Build alpha 1.0.50 changes how a cancel hits a live turn. It changes the cancel button you use."
        );
        assert!(!take.contains("https://"));
        assert!(!take.contains("ZEPHYRTAIL"));
        assert!(!take.contains("Streams are retried"));

        // Jeremy's X Stack card: a long first news sentence still keeps the why.
        let xstack = "I'll look up what changed in the tools you use every day. Grok Build alpha 1.0.50 changes how a cancel hits a live turn: tool calls that were already running now finish and report instead of being cut off, and the session keeps its place. It also retries dropped streams instead of showing partial output, and sessions no longer drop when the conversation passes 4,000 lines.";
        let mut x = digest_card("xstack", "For you", xstack, 1);
        x.why = Some("You run Grok Build agents overnight.".into());
        let xt = short_takeaway(&x);
        assert!(xt.starts_with("Grok Build alpha 1.0.50 changes how a cancel hits a live turn"), "{xt}");
        assert!(xt.ends_with("You run Grok Build agents overnight."), "{xt}");
        assert!(xt.chars().count() <= TAKEAWAY_MAX, "{} {xt}", xt.chars().count());

        // Only a lead-in: keep it rather than show nothing.
        let alone = digest_card("d", "Digest 3", "I looked and did not find a source worth your time.", 1);
        assert_eq!(short_takeaway(&alone), "I looked and did not find a source worth your time.");
        // A suggestion in the cabin's voice keeps its "I'll" line.
        let sugg = suggestion_card("s", "Standup", "I'll draft it at 8:45 so you only edit.", 1);
        assert_eq!(short_takeaway(&sugg), "I'll draft it at 8:45 so you only edit.");

        let mut short = digest_card(
            "rust",
            "Rust 1.92 ships",
            "Faster incremental builds and a new lint.",
            2,
        );
        short.why = Some("Your morning build is the one that waits.".into());
        let kept = short_takeaway(&short);
        assert!(kept.contains("Faster incremental builds and a new lint."));
        assert!(kept.contains("Your morning build is the one that waits."));
        assert!(kept.chars().count() <= TAKEAWAY_MAX);

        let mut echoed = digest_card(
            "echo",
            "For you",
            "For you. No beginner tips.Grok Build changes how cancel works. ZEPHYRTAIL stays out of this third sentence.",
            3,
        );
        echoed.citations = vec!["https://evil.example/secret".into()];
        let echo = short_takeaway(&echoed);
        assert!(echo.starts_with("No beginner tips."));
        assert!(echo.contains("Grok Build changes how cancel works."));
        assert!(!echo.contains("ZEPHYRTAIL"));
        assert!(!echo.contains("https://"));

        let wall = format!("{} ZEPHYRTAIL", "alpha ".repeat(80));
        let wall_card = digest_card("wall", "Notes", &wall, 4);
        let clipped = short_takeaway(&wall_card);
        assert!(clipped.ends_with('…'), "{clipped}");
        assert!(!clipped.contains("ZEPHYRTAIL"));
        assert!(clipped.chars().count() <= TAKEAWAY_MAX, "{clipped}");
        let head = clipped.trim_end_matches('…').trim();
        assert!(
            head.split_whitespace().all(|word| word == "alpha"),
            "cut a word in half: {clipped}"
        );

        let steered = digest_card(
            "steer",
            "A bank",
            "A story about a bank and the weather. The brief steers the next edition.",
            5,
        );
        let bank = short_takeaway(&steered);
        assert!(bank.contains("bank"));
        assert!(!bank.contains("brief steers"));
    }

    #[test]
    fn discuss_context_seeds_title_takeaway_url_and_why() {
        let mut card = digest_card(
            "xstack",
            "For you",
            "I'll look up two real pieces that fit your Linux work. Grok Build changes how a cancel hits a live turn. ZEPHYRTAIL is the rest of the dump https://evil.example/nope",
            10,
        );
        card.citations = vec!["https://xstack.grok.me/post".into()];
        card.why = Some("It changes the cancel button you use.".into());
        let seed = discuss_context(&card);
        assert!(seed.contains("For you"), "{seed}");
        assert!(!seed.contains("two real pieces"), "{seed}");
        assert!(seed.contains("cancel hits a live turn"), "{seed}");
        assert!(seed.contains("https://xstack.grok.me/post"), "{seed}");
        assert!(seed.contains("It changes the cancel button you use."), "{seed}");
        assert!(!seed.contains("ZEPHYRTAIL"), "{seed}");
        assert!(!seed.contains("evil.example"), "{seed}");
        assert!(
            seed.contains("You opened this from your feed and want to act on it."),
            "{seed}"
        );
        assert!(!seed.contains("The person opened"), "{seed}");
    }

    #[test]
    fn parse_lookup_stores_a_short_body_without_the_url() {
        let raw = format!(
            "Headline\n{} https://example.com/long-story ZEPHYRTAIL",
            "word ".repeat(80)
        );
        let parsed = parse_lookup(&raw);
        assert!(parsed.found, "{parsed:?}");
        assert_eq!(parsed.title, "Headline");
        assert_eq!(parsed.links[0].url, "https://example.com/long-story");
        assert!(!parsed.body.contains("https://"), "{}", parsed.body);
        assert!(!parsed.body.contains("ZEPHYRTAIL"), "{}", parsed.body);
        assert!(parsed.body.chars().count() <= TAKEAWAY_MAX);
        let head = parsed.body.trim_end_matches('…').trim();
        assert!(
            head.split_whitespace().all(|word| word == "Headline" || word == "word"),
            "stored body cut a word: {}",
            parsed.body
        );
    }

    #[test]
    fn digest_dead_links_are_dropped_and_private_hosts_never_fetched() {
        let raw = "One https://made.up/x\nTwo https://real.example/y";
        let kept = drop_dead_links(raw, &["https://made.up/x".to_string()]);
        assert_eq!(kept, "One\nTwo https://real.example/y");
        assert!(!parse_lookup(&drop_dead_links(raw, &[
            "https://made.up/x".to_string(),
            "https://real.example/y".to_string(),
        ]))
        .found);
        assert!(public_http_url("https://news.example.com/a"));
        for bad in [
            "http://localhost:8080/",
            "http://127.0.0.1/",
            "http://192.168.1.4/admin",
            "http://10.0.0.1",
            "http://169.254.169.254/latest",
            "http://[::1]:3000/",
            "http://intranet/",
            "ftp://example.com/",
            "http://user@127.0.0.1/",
        ] {
            assert!(!public_http_url(bad), "{bad}");
        }
    }

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
        assert_eq!(cards.len(), 1, "an event dismiss leaves one tombstone");
        assert_eq!(cards[0].status, UpdateStatus::Dismissed);
        assert!(cards[0].dismissed_at > 0);
        assert!(visible_updates(&cards).is_empty());
        assert!(!feed_visible(&cards));
        assert_eq!(home_feed_n(&cards), 0);
    }

    #[test]
    fn repeat_runs_of_one_automation_keep_one_card() {
        let mut cards = Vec::new();
        post_update(&mut cards, automation_done_card("loop-1", "Board", "summary 0", 100));
        cards[0].reaction = Some(CardReaction::Up);
        cards[0].discuss_thread = Some("thr-board".into());
        for (i, at) in [200u64, 300, 400, 500].into_iter().enumerate() {
            post_update(
                &mut cards,
                automation_done_card("loop-1", "Board", &format!("summary {}", i + 1), at),
            );
        }
        let visible = visible_updates(&cards);
        assert_eq!(visible.len(), 1);
        assert_eq!(cards.iter().filter(|c| c.kind.event()).count(), 1);
        assert_eq!(visible[0].runs, 5);
        assert_eq!(visible[0].body.as_deref(), Some("summary 4"));
        assert_eq!(visible[0].created_at, 500);
        assert_eq!(visible[0].id, "done-loop-1");
        assert_eq!(visible[0].reaction, Some(CardReaction::Up));
        assert_eq!(visible[0].discuss_thread.as_deref(), Some("thr-board"));
        assert_eq!(feed_group_key(&visible[0]).as_deref(), Some("run:loop-1"));
    }

    #[test]
    fn two_automations_keep_two_cards() {
        let mut cards = Vec::new();
        for at in [1u64, 2, 3] {
            post_update(&mut cards, automation_done_card("alpha", "Alpha", &format!("a{at}"), at));
            post_update(
                &mut cards,
                automation_done_card("beta", "Beta", &format!("b{at}"), at + 10),
            );
        }
        let visible = visible_updates(&cards);
        assert_eq!(visible.len(), 2);
        let alpha = visible.iter().find(|c| c.source_id == "alpha").unwrap();
        let beta = visible.iter().find(|c| c.source_id == "beta").unwrap();
        assert_eq!(alpha.runs, 3);
        assert_eq!(alpha.body.as_deref(), Some("a3"));
        assert_eq!(beta.runs, 3);
        assert_eq!(beta.body.as_deref(), Some("b3"));
        assert_ne!(feed_group_key(alpha), feed_group_key(beta));
    }

    #[test]
    fn failed_run_replaces_done_card_in_same_group() {
        let mut cards = Vec::new();
        post_update(&mut cards, automation_done_card("loop-1", "Board", "ok", 10));
        post_update(
            &mut cards,
            automation_failed_card("loop-1", "Board", "credit limit", 20),
        );
        let visible = visible_updates(&cards);
        assert_eq!(visible.len(), 1);
        assert_eq!(cards.iter().filter(|c| c.kind.event()).count(), 1);
        assert_eq!(visible[0].runs, 2);
        assert_eq!(visible[0].created_at, 20);
        assert_eq!(visible[0].title, "Board failed");
        assert_eq!(visible[0].body.as_deref(), Some("credit limit"));
        assert!(visible[0].id.starts_with("fail-"));
        assert_eq!(feed_group_key(&visible[0]).as_deref(), Some("run:loop-1"));
    }

    #[test]
    fn collapse_feed_folds_saved_duplicate_runs() {
        let mut cards = Vec::new();
        for (i, at) in [10u64, 20, 30, 40].into_iter().enumerate() {
            let mut card = automation_done_card("loop-9", "Board", &format!("sum {i}"), at);
            card.id = format!("done-loop-9-{at}");
            cards.push(card);
        }
        assert!(collapse_feed(&mut cards));
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].runs, 4);
        assert_eq!(cards[0].created_at, 40);
        assert_eq!(cards[0].body.as_deref(), Some("sum 3"));
        assert_eq!(cards[0].id, "done-loop-9");
        assert!(!collapse_feed(&mut cards), "a second pass is a no-op");
        let raw = r#"{"id":"done-x-1","kind":"automation_done","title":"Board","createdAt":1,"status":"unread"}"#;
        let old: UpdateCard = serde_json::from_str(raw).unwrap();
        assert_eq!(old.runs, 1);
        assert_eq!(old.dismissed_at, 0);
        let fresh = automation_done_card("z", "T", "b", 1);
        let encoded = serde_json::to_string(&fresh).unwrap();
        assert!(!encoded.contains("runs"));
        assert!(!encoded.contains("dismissedAt"));
        let folded = serde_json::to_string(&cards[0]).unwrap();
        assert!(folded.contains("\"runs\":4"));
    }

    #[test]
    fn dismissed_run_stays_gone_for_a_day() {
        let day = 24 * 60 * 60 * 1000;
        let t0 = 5_000_000u64;
        let mut cards = Vec::new();
        post_update(&mut cards, automation_done_card("loop-1", "Board", "first", t0));
        let mut older = automation_done_card("loop-1", "Board", "older", t0 - 10);
        older.id = format!("done-loop-1-{}", t0 - 10);
        cards.push(older);
        let id = cards.iter().find(|c| c.created_at == t0).unwrap().id.clone();
        assert!(dismiss_update_at(&mut cards, &id, t0));
        assert!(visible_updates(&cards).is_empty());
        assert_eq!(home_feed_n(&cards), 0);
        assert_eq!(
            cards.iter().filter(|c| feed_group_key(c).as_deref() == Some("run:loop-1")).count(),
            1,
            "the group keeps one tombstone"
        );
        assert!(cards.iter().all(|c| c.status == UpdateStatus::Dismissed));

        post_update(&mut cards, automation_done_card("loop-1", "Board", "again", t0 + 60_000));
        assert!(visible_updates(&cards).is_empty(), "a repeat within a day stays hidden");

        post_update(
            &mut cards,
            automation_failed_card("loop-1", "Board", "disk full", t0 + 120_000),
        );
        let visible = visible_updates(&cards);
        assert_eq!(visible.len(), 1, "a failure still posts");
        assert_eq!(visible[0].body.as_deref(), Some("disk full"));
        assert!(visible[0].id.starts_with("fail-") || visible[0].title.ends_with("failed"));
        assert_eq!(cards.iter().filter(|c| c.source_id == "loop-1").count(), 1);

        let mut later = Vec::new();
        post_update(&mut later, automation_done_card("loop-2", "Nightly", "once", t0));
        let id2 = later[0].id.clone();
        assert!(dismiss_update_at(&mut later, &id2, t0));
        post_update(&mut later, automation_done_card("loop-2", "Nightly", "next day", t0 + day));
        let back = visible_updates(&later);
        assert_eq!(back.len(), 1, "a run after a day comes back");
        assert_eq!(back[0].body.as_deref(), Some("next day"));
        assert!(later.iter().all(|c| c.status != UpdateStatus::Dismissed));
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
    fn quiet_hours_end_folds_held_cards_into_one_pulse_digest() {
        let mut cards = Vec::new();
        for (i, title) in [
            "Backup ran",
            "Inbox sorted",
            "Standup drafted",
            "Prices checked",
        ]
        .iter()
        .enumerate()
        {
            let mut done =
                automation_done_card(&format!("loop-{i}"), title, "ok", 10 + i as u64);
            hold_if_quiet(&mut done, true);
            cards.push(done);
        }
        let mut pulse = FeedPulse {
            digest_on: false,
            expiry_on: false,
            ..FeedPulse::default()
        };
        // Still quiet: nothing is released and nothing is posted.
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 1_000,
                quiet: true,
            },
            material("", &[], &[]),
        );
        assert_eq!(tick.released, 0);
        assert_eq!(cards.len(), 4);
        assert!(!feed_visible(&cards));
        // The window ends: four cards come back under one digest card.
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 1_000 + DEFAULT_QUIET_RELEASE_MS,
                quiet: false,
            },
            material("", &[], &[]),
        );
        assert_eq!(tick.released, 4);
        assert!(tick.cards_changed);
        let digest = cards
            .iter()
            .find(|c| c.id == "pulse-quiet-16000")
            .expect("digest card");
        assert_eq!(digest.title, "While you were in quiet hours");
        assert_eq!(
            digest.body.as_deref(),
            Some("4 updates: Backup ran · Inbox sorted · Standup drafted · and 1 more")
        );
        assert_eq!(digest.source_id, "pulse:quiet");
        assert_eq!(cards.iter().filter(|c| c.pulse.quiet_batched).count(), 4);
        assert!(cards.iter().all(|c| !c.held));
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
        assert_eq!(digest.why, None, "the steer line must not paint twice");
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
    fn a_screen_recording_card_names_the_recording_and_keeps_its_folder() {
        let c = screen_recording_card(
            "/home/ada/GrokHub/recordings/r1",
            "Screen recording 0:42: video stutters on 4K YouTube",
            "Likely cause: hardware video decoding is off.",
            "t9",
            7,
        );
        assert_eq!(c.title, "Screen recording 0:42: video stutters on 4K YouTube");
        assert_eq!(c.body.as_deref(), Some("Likely cause: hardware video decoding is off."));
        assert_eq!(c.action, Some(UpdateAction::OpenSession { thread_id: "t9".into() }));
        assert_eq!(c.source_id, "screenrec:/home/ada/GrokHub/recordings/r1");
        assert_eq!(screen_recording_dir(&c), Some("/home/ada/GrokHub/recordings/r1"));
        assert!(!is_crash_card(&c));
        assert_eq!(screen_recording_dir(&crash_card("t1", "Index", "t1", "/retry", 5)), None);
    }

    #[test]
    fn an_audio_check_card_names_the_input_and_replaces_the_last_check_of_it() {
        let c = audio_check_card(
            "alsa_input.usb-Yeti",
            "Audio check: Yeti input clipping at -0.2 dBFS",
            "Lower Yeti's input volume. Default output: HDMI.",
            "t4",
            7,
        );
        assert_eq!(c.title, "Audio check: Yeti input clipping at -0.2 dBFS");
        assert_eq!(c.body.as_deref(), Some("Lower Yeti's input volume. Default output: HDMI."));
        assert_eq!(c.action, Some(UpdateAction::OpenSession { thread_id: "t4".into() }));
        assert_eq!(c.source_id, "audiocheck:alsa_input.usb-Yeti");
        assert_eq!(c.id, "audiocheck-audiocheck:alsa_input.usb-Yeti");
        assert_eq!(audio_check_card("alsa_input.usb-Yeti", "Audio check: later", "", "", 9).id, c.id);
        assert_eq!(audio_check_card("", "Audio check: x", "", "", 9).source_id, "audiocheck:default");
        assert_eq!(audio_check_card("", "Audio check: x", "", "", 9).action, None);
    }

    #[test]
    fn a_crash_card_names_the_job_opens_its_chat_and_survives_a_dismiss() {
        let c = crash_card("t1", "Index ~/Projects", "t1", "/retry", 5);
        assert_eq!(c.kind, UpdateKind::AutomationDone);
        assert_eq!(c.title, "Crashed: Index ~/Projects (exit 143, killed)");
        assert_eq!(
            c.action,
            Some(UpdateAction::OpenSession { thread_id: "t1".into() })
        );
        assert_eq!(c.prompt.as_deref(), Some("/retry"));
        assert_eq!(c.source_id, "crash:t1");
        assert!(is_crash_card(&c));
        assert!(!is_crash_card(&automation_failed_card("t1", "Index", "x", 5)));
        assert_eq!(crash_card("t2", "  ", "", "", 5).title, "Crashed: Reply (exit 143, killed)");
        let long = crash_card("t3", &"word ".repeat(30), "t3", "/retry", 5);
        assert!(long.title.starts_with("Crashed: word word"), "{}", long.title);
        assert!(long.title.ends_with("… (exit 143, killed)"), "{}", long.title);

        // A second crash of the same chat bumps one card.
        let mut cards = Vec::new();
        post_update(&mut cards, c.clone());
        post_update(&mut cards, crash_card("t1", "Index ~/Projects", "t1", "/retry", 9));
        assert_eq!(cards.len(), 1, "{cards:?}");
        // After a dismiss, a new crash still shows: it is a failure.
        let first = cards[0].id.clone();
        dismiss_update(&mut cards, &first);
        post_update(&mut cards, crash_card("t1", "Index ~/Projects", "t1", "/retry", 20));
        assert!(
            cards.iter().any(|x| x.status != UpdateStatus::Dismissed && is_crash_card(x)),
            "{cards:?}"
        );
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
        assert_eq!(suggestions[0].0, "Paused: Ship the harbor");
        assert_eq!(suggestions[0].1.as_deref(), Some("Paused 30 minutes ago. Want me to pick it back up?"));
        assert_eq!(suggestions[0].2, "pause:job-1");
        let source = suggestions[0].2.clone();
        remember_dismissed_source(&mut pulse, &source);
        cards.retain(|c| c.kind != UpdateKind::Suggestion);
        let after = post_help(&mut cards, &mut pulse, later + PAUSE_OFFER_MS, false, false, &paused, &[], "");
        assert!(!after.posted);
        assert!(cards.iter().all(|c| c.kind != UpdateKind::Suggestion));
    }

    #[test]
    fn a_paused_job_with_no_title_gets_no_suggestion() {
        let paused = [PausedJob { id: "job-2", title: "   ", detail: "Paused. This is where to resume." }];
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        post_help(&mut cards, &mut pulse, 1_000, false, false, &paused, &[], "");
        let tick = post_help(&mut cards, &mut pulse, 1_000 + 3 * PAUSE_OFFER_MS, false, false, &paused, &[], "");
        assert!(!tick.posted);
        assert!(cards.is_empty(), "{cards:?}");
    }

    #[test]
    fn the_paused_suggestion_points_at_its_job_and_says_how_long() {
        let paused = [PausedJob { id: " job-9 ", title: "Fix the tray icon", detail: "Paused. This is where to resume." }];
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        post_help(&mut cards, &mut pulse, 0, false, false, &paused, &[], "");
        assert!(post_help(&mut cards, &mut pulse, 2 * 3_600_000 + 5 * 60_000, false, false, &paused, &[], "").posted);
        let card = cards.iter().find(|c| c.kind == UpdateKind::Suggestion).expect("suggestion");
        assert_eq!(card.title, "Paused: Fix the tray icon");
        assert_eq!(card.body.as_deref(), Some("Paused 2 hours ago. Want me to pick it back up?"));
        assert_eq!(paused_job_of(card), Some("job-9"));
        let idea = cards.iter().find(|c| c.kind == UpdateKind::Idea).expect("idea");
        assert_eq!(paused_job_of(idea), None, "only the suggestion opens the job");
        assert_eq!(
            [paused_ago(0), paused_ago(60_000), paused_ago(45 * 60_000), paused_ago(3_600_000), paused_ago(3 * 86_400_000)],
            ["just now", "1 minute ago", "45 minutes ago", "1 hour ago", "3 days ago"]
        );
    }

    /// No situation card may point at "that job" or "it" without naming the thing.
    #[test]
    fn situation_cards_name_what_they_are_about() {
        let paused = [PausedJob { id: "job-1", title: "Ship the harbor", detail: "Paused. This is where to resume." }];
        let repeat = RepeatedAction { key: "open-cabin", label: "Open the cabin", count: 3, dismissed: false, automated: false };
        for (paused, repeats, name) in [(&paused[..], &[][..], "Ship the harbor"), (&[][..], &[repeat][..], "Open the cabin")] {
            let mut cards = Vec::new();
            let mut pulse = FeedPulse::default();
            post_help(&mut cards, &mut pulse, 0, false, true, paused, repeats, "");
            post_help(&mut cards, &mut pulse, PAUSE_OFFER_MS, false, true, paused, repeats, "");
            let card = cards.iter().find(|c| c.kind == UpdateKind::Suggestion).expect("suggestion");
            assert!(card.title.contains(name), "{card:?}");
            let text = format!("{} {}", card.title, card.body.as_deref().unwrap_or("")).to_ascii_lowercase();
            for vague in ["that job", "this task", "this job", "that task"] {
                assert!(!text.contains(vague), "{vague:?} in {text:?}");
            }
        }
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
        assert_eq!(
            body,
            "I looked and did not find a source worth your time. The brief steers the next edition."
        );
        assert_eq!(cards[0].why, None, "the steer line is in the body; once is enough");
        assert!(cards[0].citations.is_empty());
    }

    #[test]
    fn opened_card_restarts_its_run_count() {
        // Unread repeats still add. `repeat_runs_of_one_automation_keep_one_card`
        // covers that path (five posts, never opened, runs = 5) and is unchanged.
        let mut cards = Vec::new();
        for at in [10u64, 20, 30] {
            post_update(&mut cards, automation_done_card("loop-1", "Board", "ok", at));
        }
        assert_eq!(visible_updates(&cards)[0].runs, 3);
        assert!(mark_update_opened(&mut cards, "done-loop-1"));
        post_update(&mut cards, automation_done_card("loop-1", "Board", "again", 40));
        let card = &visible_updates(&cards)[0];
        assert_eq!(card.runs, 1, "an opened card starts again at the new run");
        assert_eq!(card.status, UpdateStatus::Unread);
        assert_eq!(card.body.as_deref(), Some("again"));
        let why = card.why.as_deref().unwrap();
        assert!(!why.contains("runs since you last looked"));
        post_update(&mut cards, automation_done_card("loop-1", "Board", "third", 50));
        let card = &visible_updates(&cards)[0];
        assert_eq!(card.runs, 2);
        assert!(card.why.as_deref().unwrap().contains("· 2 runs since you last looked"));
        assert_eq!(
            runs_latest_line(3, 0, 90 * 60_000, 15, 0).as_deref(),
            Some("×3 runs · latest 13:30")
        );
        assert!(runs_latest_line(1, 0, 0, 15, 0).is_none());
    }

    #[test]
    fn hidden_automation_stays_off_home_but_still_updates() {
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        post_update(&mut cards, automation_done_card("hid", "Nightly", "one", 10));
        hide_home_source(&mut pulse, "hid");
        hide_home_source(&mut pulse, "hid");
        assert_eq!(pulse.muted_sources, vec!["hid".to_string()]);
        assert!(home_event_cards(&cards, &pulse, 10).is_empty());
        post_update(&mut cards, automation_done_card("hid", "Nightly", "two", 20));
        let live: Vec<&UpdateCard> = cards
            .iter()
            .filter(|c| c.source_id == "hid" && c.status != UpdateStatus::Dismissed)
            .collect();
        assert_eq!(live.len(), 1, "the hidden run still merges in place");
        assert_eq!(live[0].runs, 2);
        assert_eq!(live[0].body.as_deref(), Some("two"));
        assert!(home_event_cards(&cards, &pulse, 20).is_empty());
        post_update(&mut cards, automation_done_card("other", "Other", "stay", 30));
        let home = home_event_cards(&cards, &pulse, 30);
        assert_eq!(home.len(), 1);
        assert_eq!(home[0].source_id, "other");
        unhide_home_source(&mut pulse, "hid");
        assert!(!source_hidden(&pulse, "hid"));
        assert_eq!(home_event_cards(&cards, &pulse, 30).len(), 2);
    }

    #[test]
    fn hidden_automation_failure_shows_once_a_day() {
        let mut cards = Vec::new();
        let mut pulse = FeedPulse::default();
        hide_home_source(&mut pulse, "hid");
        post_update(
            &mut cards,
            automation_failed_card("hid", "Nightly", "disk full", 1_000),
        );
        let shown = home_event_cards(&cards, &pulse, 1_000);
        assert_eq!(shown.len(), 1, "the first failure still surfaces");
        assert!(record_home_floors(&mut pulse, &shown, 1_000));
        assert!(!record_home_floors(&mut pulse, &shown, 1_000), "the same failure stays put");
        assert_eq!(pulse.floor_shown.get("hid"), Some(&1_000));
        assert_eq!(home_event_cards(&cards, &pulse, 1_500).len(), 1);
        post_update(
            &mut cards,
            automation_failed_card("hid", "Nightly", "disk again", 2_000),
        );
        assert_eq!(
            cards.iter().filter(|c| c.status != UpdateStatus::Dismissed).count(),
            1
        );
        assert_eq!(cards.iter().find(|c| c.source_id == "hid").unwrap().runs, 2);
        assert!(
            home_event_cards(&cards, &pulse, 2_000).is_empty(),
            "a newer failure inside the day stays off Home"
        );
        let day = 24 * 60 * 60 * 1000;
        assert!(home_event_cards(&cards, &pulse, 1_000 + day - 1).is_empty());
        assert_eq!(home_event_cards(&cards, &pulse, 1_000 + day).len(), 1);
    }

    #[test]
    fn less_like_this_mutes_the_group_for_two_weeks() {
        let bare: FeedPulse = serde_json::from_str("{}").unwrap();
        assert!(bare.muted_sources.is_empty());
        assert!(bare.less_until.is_empty());
        assert!(bare.floor_shown.is_empty());
        let flagged: FeedPulse = serde_json::from_str(r#"{"expiryOn":true}"#).unwrap();
        assert!(flagged.less_until.is_empty());

        let card = automation_done_card("loop-9", "Board", "ok", 10);
        let other = automation_done_card("loop-8", "Other", "ok", 11);
        let mut pulse = FeedPulse::default();
        let now = 5_000u64;
        mute_less_like(&mut pulse, &card, now);
        assert_eq!(
            pulse.less_until.get("run:loop-9").copied(),
            Some(now + LESS_MUTE_MS)
        );
        assert!(!surfaces_on_home(&card, &pulse, now));
        assert!(surfaces_on_home(&other, &pulse, now));
        assert!(!surfaces_on_home(&card, &pulse, now + LESS_MUTE_MS - 1));
        assert!(surfaces_on_home(&card, &pulse, now + LESS_MUTE_MS));

        let fail = automation_failed_card("loop-9", "Board", "disk", now + 10);
        assert!(surfaces_on_home(&fail, &pulse, now + 10));
        assert!(record_home_floors(&mut pulse, std::slice::from_ref(&fail), now + 10));
        let again = automation_failed_card("loop-9", "Board", "disk again", now + 20);
        assert!(!surfaces_on_home(&again, &pulse, now + 20));

        clear_less_mute(&mut pulse, &card);
        assert!(surfaces_on_home(&card, &pulse, now));
        assert!(pulse.less_until.is_empty());
    }

    #[test]
    fn why_line_per_kind() {
        let done = automation_done_card("a", "Board", "ok", 1);
        assert_eq!(
            done.why.as_deref(),
            Some("Your automation “Board” finished.")
        );
        let fail = automation_failed_card("a", "Board", "disk full", 2);
        assert_eq!(fail.why.as_deref(), Some("“Board” failed and needs a look."));
        assert!(!fail.why.as_deref().unwrap().contains("runs since"));
        let sched = schedule_created_card("a", "Board", "weekdays at 9", 3);
        assert_eq!(sched.why.as_deref(), Some("You saved this schedule."));
        let mut sugg = suggestion_card("s", "File the notes", "from last night", 4);
        sugg.why = Some("kept".into());
        refresh_event_why(&mut sugg);
        assert_eq!(sugg.why.as_deref(), Some("kept"));

        let mut cards = vec![done];
        cards[0].board_id = Some("follow-1".into());
        post_update(&mut cards, automation_done_card("a", "Board", "next", 5));
        let card = &visible_updates(&cards)[0];
        assert_eq!(card.board_id.as_deref(), Some("follow-1"));
        assert_eq!(card.runs, 2);
        assert_eq!(
            card.why.as_deref(),
            Some(
                "Your automation “Board” finished and left a report in Follow up. · 2 runs since you last looked"
            )
        );
    }

    #[test]
    fn home_paints_at_most_three_event_cards() {
        assert_eq!(FEED_PAINT_MAX, 3);
        let mut cards = Vec::new();
        for n in 0..5u64 {
            post_update(
                &mut cards,
                automation_done_card(&format!("e{n}"), "Loop", "done", n),
            );
        }
        cards.push(idea_card("idea", "A thought", "not an event", 9));
        let pulse = FeedPulse::default();
        let home = home_event_cards(&cards, &pulse, 10);
        assert_eq!(home.len(), 3);
        assert!(home.iter().all(|c| c.kind.event()));
        assert_eq!(
            home.iter().map(|c| c.source_id.as_str()).collect::<Vec<_>>(),
            vec!["e4", "e3", "e2"]
        );
        assert_eq!(visible_updates(&cards).len(), 5);
    }

    fn prefs_weight(pos: f64, neg: f64, n: u32, at: u64) -> crate::card_prefs::Weight {
        crate::card_prefs::Weight { pos, neg, n, at }
    }

    #[test]
    fn liked_group_rises_and_dismissed_group_folds() {
        use crate::card_prefs::{apply_card_event, rank_home_events, CardPrefs};
        use crate::card_signals::CardEvent;
        let loved = automation_done_card("loved", "Alpha snapshot report", "one", 100);
        let neutral = automation_done_card("neutral", "Beta widget module", "two", 300);
        let hated = automation_done_card("hated", "Gamma ledger nightly", "three", 200);
        let mut prefs = CardPrefs::default();
        for _ in 0..3 {
            apply_card_event(&mut prefs, &loved, CardEvent::More, None, 1_000);
        }
        for _ in 0..4 {
            apply_card_event(&mut prefs, &hated, CardEvent::Less, None, 1_000);
        }
        let cards = vec![loved, neutral, hated];
        let rank = rank_home_events(&cards, &FeedPulse::default(), &prefs, 1_000);
        assert_eq!(rank.deck.first().map(|card| card.source_id.as_str()), Some("loved"));
        assert!(rank.deck.iter().any(|card| card.source_id == "neutral"));
        assert!(rank.deck.iter().all(|card| card.source_id != "hated"));
        assert!(rank.folded.iter().any(|card| card.source_id == "hated"));
        let again = rank_home_events(&cards, &FeedPulse::default(), &prefs, 1_000);
        assert_eq!(
            rank.deck.iter().map(|card| &card.id).collect::<Vec<_>>(),
            again.deck.iter().map(|card| &card.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn failures_and_pinned_cards_are_never_folded() {
        use crate::card_prefs::{apply_card_event, rank_home_events, CardPrefs};
        use crate::card_signals::CardEvent;
        let fail = automation_failed_card("bad", "Nightly", "disk full", 500);
        let mut pinned = automation_done_card("pin", "Pinned ledger report", "x", 400);
        pinned.feed_pin = true;
        let mut worked = automation_done_card("work", "Worked ledger report", "y", 350);
        worked.modified = true;
        let plain = automation_done_card("plain", "Plain ledger report", "z", 300);
        let mut prefs = CardPrefs::default();
        for card in [&fail, &pinned, &worked, &plain] {
            for _ in 0..10 {
                apply_card_event(&mut prefs, card, CardEvent::Less, None, 500);
            }
        }
        let cards = vec![fail.clone(), pinned.clone(), worked.clone(), plain.clone()];
        let rank = rank_home_events(&cards, &FeedPulse::default(), &prefs, 500);
        assert!(rank.deck.iter().any(|card| card.id == fail.id));
        assert!(rank.deck.iter().any(|card| card.id == pinned.id));
        assert!(rank.deck.iter().any(|card| card.id == worked.id));
        assert!(rank.deck.iter().all(|card| card.id != plain.id));
        assert!(rank.folded.iter().any(|card| card.id == plain.id));
        assert!(rank.folded.iter().all(|card| card.id != fail.id && card.id != pinned.id && card.id != worked.id));
        assert!(rank.deck.len() <= FEED_PAINT_MAX);
    }

    #[test]
    fn ranking_respects_hide_mute_and_the_three_card_limit() {
        use crate::card_prefs::{apply_card_event, rank_home_events, CardPrefs};
        use crate::card_signals::CardEvent;
        let mut prefs = CardPrefs::default();
        let mut cards = Vec::new();
        for i in 0..4 {
            let card = automation_done_card(&format!("s{i}"), "Alpha snapshot report", "ok", 1_000 + i * 100);
            apply_card_event(&mut prefs, &card, CardEvent::More, None, 1_000);
            cards.push(card);
        }
        let hidden = automation_done_card("hid", "Alpha snapshot report", "ok", 2_000);
        let muted = automation_done_card("mute", "Alpha snapshot report", "ok", 1_800);
        apply_card_event(&mut prefs, &muted, CardEvent::More, None, 1_000);
        let fail = automation_failed_card("hid", "Nightly", "disk full", 1_500);
        cards.push(hidden);
        cards.push(muted.clone());
        cards.push(fail);
        let mut pulse = FeedPulse::default();
        hide_home_source(&mut pulse, "hid");
        mute_less_like(&mut pulse, &muted, 1_000);
        let rank = rank_home_events(&cards, &pulse, &prefs, 1_500);
        assert_eq!(rank.deck.len(), FEED_PAINT_MAX);
        assert!(rank.deck.iter().any(|card| card.id.starts_with("fail-")));
        assert!(rank.deck.iter().all(|card| card.source_id != "mute"));
        assert!(rank.deck.iter().all(|card| card.source_id != "hid" || card.id.starts_with("fail-")));
        assert!(rank.folded.iter().all(|card| card.source_id != "hid" && card.source_id != "mute"));
        let parked = cards.iter().filter(|card| {
            card.source_id.starts_with('s') && rank.deck.iter().all(|shown| shown.id != card.id)
        });
        assert!(parked.clone().count() >= 1);
        assert!(parked.into_iter().all(|card| rank.folded.iter().all(|folded| folded.id != card.id)));
    }

    #[test]
    fn novelty_bonus_goes_to_one_card() {
        use crate::card_prefs::{card_score, rank_home_events, CardPrefs, NOVELTY_BONUS};
        let a = automation_done_card("a", "Alpha snapshot report", "x", 1_000);
        let b = automation_done_card("b", "Beta widget report", "y", 1_000);
        let c = automation_done_card("c", "Gamma ledger report", "z", 1_000);
        let prefs = CardPrefs::default();
        let cards = vec![c.clone(), a.clone(), b.clone()];
        let pulse = FeedPulse::default();
        let rank = rank_home_events(&cards, &pulse, &prefs, 1_000);
        let again = rank_home_events(&cards, &pulse, &prefs, 1_000);
        assert_eq!(rank.novelty_id.as_deref(), Some(a.id.as_str()));
        assert_eq!(rank.novelty_id, again.novelty_id);
        assert_ne!(rank.novelty_id.as_deref(), Some(b.id.as_str()));
        let plain = card_score(&a, &prefs, 1_000, false);
        let boosted = card_score(&a, &prefs, 1_000, true);
        assert!((boosted - plain - NOVELTY_BONUS).abs() < 1e-9);
        assert!((card_score(&b, &prefs, 1_000, false) - plain).abs() < 1e-9);
    }

    #[test]
    fn explain_hint_picks_the_top_term() {
        use crate::card_prefs::{
            explain_hint, hint_open_topic, CardPrefs, FOLD_NOTE, HINT_NEEDS, HINT_NEW, HINT_OPEN_GROUP,
        };
        use crate::card_signals::signal_group;
        assert_eq!(FOLD_NOTE, "Showing fewer of these; you've been dismissing them");
        let now = 1_000u64;
        let card = automation_done_card("src", "Snapshot ledger report", "body text", now);
        let group = signal_group(&card);
        let mut prefs = CardPrefs::default();
        prefs.groups.insert(group.clone(), prefs_weight(8.0, 0.0, 5, now));
        prefs.topics.insert("snapshot".into(), prefs_weight(1.0, 0.0, 2, now));
        assert_eq!(
            explain_hint(&card, &prefs, now, true).as_deref(),
            Some(HINT_OPEN_GROUP)
        );

        prefs.groups.insert(group.clone(), prefs_weight(0.0, 0.0, 5, now));
        prefs.topics.insert("ledger".into(), prefs_weight(8.0, 0.0, 5, now));
        assert_eq!(
            explain_hint(&card, &prefs, now, true).as_deref(),
            Some(hint_open_topic("ledger").as_str())
        );

        let fail = automation_failed_card("src", "Nightly", "disk full", now);
        prefs.groups.insert(signal_group(&fail), prefs_weight(8.0, 0.0, 5, now));
        assert_eq!(explain_hint(&fail, &prefs, now, true).as_deref(), Some(HINT_NEEDS));

        let fresh = automation_done_card("new", "Widget digest report", "x", now);
        let empty = CardPrefs::default();
        assert_eq!(explain_hint(&fresh, &empty, now, true).as_deref(), Some(HINT_NEW));
        assert!(explain_hint(&fresh, &empty, now, false).is_none());

        let mut disliked = CardPrefs::default();
        disliked.groups.insert(signal_group(&fresh), prefs_weight(0.0, 8.0, 5, now));
        assert!(
            explain_hint(&fresh, &disliked, now, true).is_none(),
            "a stronger negative group blocks New for you"
        );
    }
}
