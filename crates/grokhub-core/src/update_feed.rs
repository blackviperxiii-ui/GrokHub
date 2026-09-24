//! Home update feed. Event cards live in `updates.json` until the user opens or dismisses one.
//!
//! An empty event list paints nothing. There is no "No updates" placeholder.
//!
//! Live hook: `Cabin::poll_grok_loop` posts `automation_done` when a scheduled
//! `/loop` returns. A night recipe replay that counts as a finished run posts
//! the same kind from `fire_night`. `Cabin::commit_schedule` posts
//! `schedule_created` when a clock job or interval loop is saved.
//! `suggestion` and `automate_offer` stay typed constructors with no review producer.
//! Idea cards and the editorial digest are separate writers into the same store.
//! They do not consume the event paint cap. Interest learning and `interest_update`
//! are out of scope.

use serde::{Deserialize, Serialize};

use crate::review::cabin_real_text;

/// Newest event cards painted in the home slot. Older undismissed event cards stay on disk.
pub const FEED_PAINT_MAX: usize = 4;
/// Idea cards shown on the home discovery row. The Ideas surface shows the full set.
pub const IDEA_DISCOVERY_MAX: usize = 2;
/// Digest cards painted beside the event slot. They do not consume `FEED_PAINT_MAX`.
pub const DIGEST_PAINT_MAX: usize = 2;
const FEED_STORE_MAX: usize = 40;
const TITLE_CHARS: usize = 72;
const BODY_CHARS: usize = 160;
const IDEA_TITLE_MEMORY: usize = 64;

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
}

fn is_false(v: &bool) -> bool {
    !*v
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PulseTick {
    pub expired: usize,
    pub released: usize,
    pub digest_posted: bool,
    pub idea_posted: bool,
    pub digest_held: bool,
    pub cards_changed: bool,
    pub pulse_changed: bool,
}

/// True when the home slot should paint. Held and dismissed cards do not count.
/// Ideas and digest cards can open the slot without entering the event cap.
pub fn feed_visible(cards: &[UpdateCard]) -> bool {
    home_feed_n(cards) > 0
}

/// Event cards only, newest first. Ideas and digest cards are not in this list,
/// so they cannot consume the paint cap of 4.
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
        + visible_ideas(cards).len().min(IDEA_DISCOVERY_MAX)
        + visible_digests(cards).len().min(DIGEST_PAINT_MAX)
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
    let id = card.id.clone();
    cards.retain(|c| {
        if c.id == id {
            return false;
        }
        if c.kind == UpdateKind::Idea || c.kind == UpdateKind::Digest {
            return true;
        }
        c.status != UpdateStatus::Dismissed
    });
    cards.push(card);
    trim_feed_store(cards);
}

fn trim_feed_store(cards: &mut Vec<UpdateCard>) {
    let mut ideas = Vec::new();
    let mut archive = Vec::new();
    let mut live = Vec::new();
    for card in cards.drain(..) {
        if card.kind == UpdateKind::Idea {
            ideas.push(card);
        } else if card.kind == UpdateKind::Digest && card.status == UpdateStatus::Dismissed {
            archive.push(card);
        } else {
            live.push(card);
        }
    }
    sort_feed(&mut live);
    sort_feed(&mut archive);
    sort_feed(&mut ideas);
    if live.len() > FEED_STORE_MAX {
        live.truncate(FEED_STORE_MAX);
    }
    if archive.len() > FEED_STORE_MAX {
        archive.truncate(FEED_STORE_MAX);
    }
    cards.extend(live);
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

pub fn idea_expired(card: &UpdateCard, now: u64) -> bool {
    if card.kind != UpdateKind::Idea {
        return false;
    }
    now >= card
        .expires_at
        .unwrap_or_else(|| card.created_at.saturating_add(IDEA_TTL_MS))
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
            UpdateKind::AutomationDone | UpdateKind::Idea | UpdateKind::Digest
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

/// Weather, mail, calendar, and bank stay refused, plus the review rail.
pub fn digest_topic_refused(text: &str) -> bool {
    if !cabin_real_text(text) {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    const EXTRA: &[&str] = &[
        "weather", "forecast", "calendar", "inbox", "mailbox", "gmail", "outlook", "bank",
    ];
    EXTRA.iter().any(|word| lower.contains(word))
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
        idea_posted: false,
        digest_held: pulse.digest_held,
        cards_changed: false,
        pulse_changed: false,
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
        if let Some(card) = compose_digest(cards, now.now_ms, material) {
            post_update(cards, card);
            tick.digest_posted = true;
            tick.cards_changed = true;
            if let Some(idea) = compose_idea(cards, pulse, now.now_ms, material.brief) {
                let title = idea.title.clone();
                post_update(cards, idea);
                remember_idea_title(pulse, &title);
                tick.idea_posted = true;
            }
            pulse.last_digest_ms = now.now_ms;
            pulse.digest_held = false;
            tick.digest_held = false;
            tick.pulse_changed = true;
        } else if pulse.digest_held {
            pulse.digest_held = false;
            tick.digest_held = false;
            tick.pulse_changed = true;
        }
    }
    tick
}

fn remember_idea_title(pulse: &mut FeedPulse, title: &str) {
    if pulse
        .idea_titles
        .iter()
        .any(|t| t.eq_ignore_ascii_case(title))
    {
        return;
    }
    if pulse.idea_titles.len() >= IDEA_TITLE_MEMORY {
        return;
    }
    pulse.idea_titles.push(title.to_string());
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
) -> Option<UpdateCard> {
    let brief = material.brief.trim();
    if digest_topic_refused(brief) {
        return None;
    }
    let user = profile_line(material.user_md);
    let memory = profile_line(material.memory_md);
    let soul = profile_line(material.soul_md);
    let taste: Vec<&TasteNote> = material
        .taste
        .iter()
        .filter(|note| note.reaction.is_some() || !note.said.trim().is_empty())
        .collect();
    let links = real_links(material.links);
    if brief.is_empty()
        && user.is_empty()
        && memory.is_empty()
        && soul.is_empty()
        && taste.is_empty()
        && links.is_empty()
    {
        return None;
    }
    let edition = cards
        .iter()
        .filter(|c| c.kind == UpdateKind::Digest)
        .count()
        + 1;
    let title = if brief.is_empty() {
        format!("Digest {edition}")
    } else {
        format!("Digest {edition} · {}", clip_line(brief, 48))
    };
    let mut parts: Vec<String> = Vec::new();
    if !brief.is_empty() {
        parts.push(format!("Brief: {brief}"));
    }
    if !user.is_empty() {
        parts.push(user);
    } else if !memory.is_empty() {
        parts.push(memory);
    } else if !soul.is_empty() {
        parts.push(soul);
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
    let allowed: Vec<String> = links.iter().map(|l| l.url.clone()).collect();
    for link in &links {
        parts.push(format!("{} {}", link.label, link.url));
    }
    if let Some(prev) = newest(cards, UpdateKind::Digest) {
        parts.push(format!("Not repeating {}", prev.title));
    }
    let body = strip_foreign_urls(&parts.join(". "), &allowed);
    if body.trim().is_empty() || digest_topic_refused(&body) {
        return None;
    }
    let mut card = digest_card("edition", &title, &body, now);
    card.citations = allowed;
    card.why = Some(if brief.is_empty() {
        "From your profile.".into()
    } else {
        "From your brief.".into()
    });
    Some(card)
}

fn compose_idea(
    cards: &[UpdateCard],
    pulse: &FeedPulse,
    now: u64,
    brief: &str,
) -> Option<UpdateCard> {
    let brief = brief.trim();
    if brief.is_empty() || digest_topic_refused(brief) {
        return None;
    }
    let title = clip_line(brief, TITLE_CHARS);
    if title.is_empty() {
        return None;
    }
    if pulse
        .idea_titles
        .iter()
        .any(|t| t.eq_ignore_ascii_case(&title))
    {
        return None;
    }
    if cards
        .iter()
        .any(|c| c.kind == UpdateKind::Idea && c.title.eq_ignore_ascii_case(&title))
    {
        return None;
    }
    if pulse.idea_titles.len() >= IDEA_TITLE_MEMORY {
        return None;
    }
    let mut card = idea_card("brief", &title, brief, now);
    card.why = Some("From your brief.".into());
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

fn profile_line(raw: &str) -> String {
    const SKIP: &[&str] = &[
        "Who this cabin is. Edit this.",
        "Who you are. Edit this.",
        "Long-term notes.",
    ];
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && !SKIP.contains(line))
        .map(|line| clip_line(line, BODY_CHARS))
        .unwrap_or_default()
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
    card
}

/// Typed suggestion card. No ranking producer in this pass.
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
    blank_card(
        feed_card_id("sugg", source_id, created_at),
        UpdateKind::Suggestion,
        title,
        body,
        created_at,
    )
}

/// Typed offer. Accept stays `commit_schedule`. It does not file a Todo.
pub fn automate_offer_card(source_id: &str, title: &str, created_at: u64) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let title = if title.is_empty() {
        "Want me to automate this and notify you here when done?".to_string()
    } else {
        title
    };
    blank_card(
        feed_card_id("offer", source_id, created_at),
        UpdateKind::AutomateOffer,
        title,
        Some("Want me to automate this and notify you here when done?".into()),
        created_at,
    )
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
    card.expires_at = Some(created_at.saturating_add(IDEA_TTL_MS));
    card.action = Some(UpdateAction::OpenWorkboard);
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
        let body = clip_line(body, BODY_CHARS);
        if body.is_empty() {
            None
        } else {
            Some(body)
        }
    };
    blank_card(
        feed_card_id("digest", source_id, created_at),
        UpdateKind::Digest,
        title,
        body,
        created_at,
    )
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
        assert!(tick.digest_posted);
        assert!(tick.idea_posted);
        assert_eq!(pulse.last_digest_ms, 200);
        assert!(!pulse.digest_held);
        assert!(cards.iter().any(|c| c.kind == UpdateKind::Digest));
        assert!(cards.iter().any(|c| c.kind == UpdateKind::Idea));
        assert!(visible_updates(&cards).is_empty());
    }

    #[test]
    fn digest_cites_only_returned_urls_and_refuses_mail() {
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
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 50,
                quiet: false,
            },
            material("more F1 https://evil.example/phish", &links, &[]),
        );
        assert!(tick.digest_posted);
        let digest = cards.iter().find(|c| c.kind == UpdateKind::Digest).unwrap();
        let blob = format!(
            "{} {}",
            digest.body.as_deref().unwrap_or(""),
            digest.citations.join(" ")
        );
        assert!(blob.contains("https://news.example/f1"));
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
            material("check gmail and the weather", &[], &[]),
        );
        assert!(!tick.digest_posted);
        assert!(refused.is_empty());
        assert_eq!(pulse.last_digest_ms, 0);
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
        let tick = tick_feed_pulse(
            &mut cards,
            &mut pulse,
            PulseNow {
                now_ms: 30,
                quiet: false,
            },
            material("more F1", &[], &taste),
        );
        assert!(tick.digest_posted);
        let digest = cards
            .iter()
            .find(|c| c.kind == UpdateKind::Digest && c.created_at == 30)
            .unwrap();
        assert!(digest.body.as_deref().unwrap().contains("marked up"));
        assert!(digest.body.as_deref().unwrap().contains("You said"));
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
}
