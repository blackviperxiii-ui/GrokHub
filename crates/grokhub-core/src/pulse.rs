//! Pulse: one page for what the cabin would do next (Ideas) and what it noticed
//! (Feed). Rules pick and order the cards; a model only ever writes sentences.
//! Same inputs give the same order. Feedback lands in MEMORY.md as one
//! structured ledger line per choice, and the ledger feeds the next ranking and
//! the feed instructions rewrite.

use serde::{Deserialize, Serialize};

use crate::ideas::same_topic;
use crate::update_feed::{
    post_update, public_http_url, FeedPulse, UpdateCard, UpdateKind, UpdateStatus,
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
    /// Released from quiet hours inside one digest card: no ping of its own.
    #[serde(default, skip_serializing_if = "is_false")]
    pub quiet_batched: bool,
    /// Closed on the main window's deck. Pulse still shows it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub off_home: bool,
    /// A Spike-6a proactive offer: its help score, route and topic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proactive: Option<crate::proactive::ProactiveMeta>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn is_false(v: &bool) -> bool {
    !*v
}

pub const PULSE_TITLE: &str = "Pulse";
pub const PULSE_SUBTITLE: &str = "What I'd do next, from how you use the cabin.";
/// Ideas with nothing to show and nothing running.
pub const IDEAS_EMPTY: &str = "No ideas yet. I'll add one when I spot something worth doing.";
/// Feed with nothing to show.
pub const FEED_EMPTY: &str = "No posts yet. Tell me what to watch in Feed instructions.";
/// Lead before the Feed instructions link on the empty Feed.
pub const FEED_EMPTY_LEAD: &str = "No posts yet. Tell me what to watch in ";
/// Clickable end of the empty Feed line (opens the instructions sheet).
pub const FEED_EMPTY_LINK: &str = "Feed instructions";
/// Ideas while a suggestion call is running: shown with placeholder rows.
pub const IDEAS_LOADING: &str = "Looking for ideas in your recent work…";
/// Suggest ideas pressed with no Grok sign-in or key.
pub const IDEAS_SIGN_IN: &str = "Sign in to Grok to get ideas.";
/// The longest bold line on an Ideas row.
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

// ---------------------------------------------------------------- categories

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PulseCategory {
    Financial,
    Productivity,
    Relationships,
    Health,
    Shopping,
    More,
}

impl PulseCategory {
    /// Header order on the Ideas view.
    pub const ORDER: [PulseCategory; 6] = [
        Self::Financial,
        Self::Productivity,
        Self::Relationships,
        Self::Health,
        Self::Shopping,
        Self::More,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Financial => "Financial Management",
            Self::Productivity => "Productivity",
            Self::Relationships => "Relationships",
            Self::Health => "Health & Fitness",
            Self::Shopping => "Shopping",
            Self::More => "More ideas",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Financial => "financial",
            Self::Productivity => "productivity",
            Self::Relationships => "relationships",
            Self::Health => "health",
            Self::Shopping => "shopping",
            Self::More => "more",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ORDER.into_iter().find(|c| c.key() == key.trim())
    }
}

const FINANCIAL: &[&str] = &[
    "bill",
    "bills",
    "budget",
    "invoice",
    "invoices",
    "payment",
    "pay ",
    "paycheck",
    "payday",
    "subscription",
    "subscriptions",
    "renewal",
    "renews",
    "insurance",
    "tax",
    "taxes",
    "bank",
    "rent",
    "mortgage",
    "loan",
    "refund",
    "expense",
    "expenses",
    "spending",
    "savings",
    "price drop",
];
const HEALTH: &[&str] = &[
    "workout",
    "exercise",
    "gym",
    "run ",
    "running",
    "steps",
    "sleep",
    "doctor",
    "dentist",
    "meal",
    "meals",
    "calorie",
    "calories",
    "habit",
    "health",
    "fitness",
    "stretch",
    "water",
    "medication",
    "dinner plan",
    "grocery plan",
];
const RELATIONSHIPS: &[&str] = &[
    "family",
    "friend",
    "friends",
    "birthday",
    "anniversary",
    "mom",
    "dad",
    "partner",
    "wife",
    "husband",
    "kids",
    "invite",
    "catch up",
    "reunion",
    "pet ",
    "dog",
    "cat ",
];
const SHOPPING: &[&str] = &[
    "buy",
    "shopping",
    "deal",
    "deals",
    "price",
    "prices",
    "compare",
    "order",
    "marketplace",
    "best-reviewed",
    "cart",
    "gift",
    "laptop stand",
];
const PRODUCTIVITY: &[&str] = &[
    "resume",
    "job",
    "jobs",
    "interview",
    "calendar",
    "meeting",
    "email",
    "inbox",
    "workboard",
    "report",
    "review",
    "notes",
    "plan",
    "schedule",
    "build",
    "release",
    "repo",
    "pull request",
    "deploy",
    "summarize",
    "summary",
    "brief",
    "draft",
    "folder",
    "files",
    "backup",
];

/// Category from the card's words. Fixed keyword table, first match in this
/// order: Financial, Health, Relationships, Shopping, Productivity, else More.
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
    if hit(FINANCIAL) {
        PulseCategory::Financial
    } else if hit(HEALTH) {
        PulseCategory::Health
    } else if hit(RELATIONSHIPS) {
        PulseCategory::Relationships
    } else if hit(SHOPPING) {
        PulseCategory::Shopping
    } else if hit(PRODUCTIVITY) {
        PulseCategory::Productivity
    } else {
        PulseCategory::More
    }
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

// ---------------------------------------------------------------- card types

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
pub fn is_idea_card(card: &UpdateCard) -> bool {
    matches!(
        card.kind,
        UpdateKind::Idea | UpdateKind::Suggestion | UpdateKind::AutomateOffer
    ) && card.source_id != QUIET_DIGEST_SOURCE
}

/// The bold line on an Ideas row, always in the cabin's own voice and at most
/// `I_CAN_MAX` characters. The model writes it as the idea's title ("I can …");
/// a card without one gets a plain line from its action: Do "I can {action}",
/// Automate "I can {action} {cadence}", Learn "I can learn how you {action}".
pub fn i_can_title(card: &UpdateCard) -> String {
    let title = card.title.trim();
    let lower = title.to_ascii_lowercase();
    if lower.starts_with("i can ") && title.chars().count() <= I_CAN_MAX {
        return title.to_string();
    }
    // A proactive ask card asks first ("Should I …?") in the same voice.
    let asks = card
        .pulse
        .proactive
        .as_ref()
        .is_some_and(|p| p.route == crate::proactive::ProactiveRoute::Ask);
    if asks && title.chars().count() <= I_CAN_MAX {
        return title.to_string();
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

// ---------------------------------------------------------------- ledger

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerReason {
    NotThis,
    Dismiss,
    Accepted,
    Wrong,
    Liked,
}

impl LedgerReason {
    pub fn key(self) -> &'static str {
        match self {
            Self::NotThis => "not-this",
            Self::Dismiss => "dismiss",
            Self::Accepted => "accepted",
            Self::Wrong => "wrong",
            Self::Liked => "liked",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Self::NotThis | Self::Dismiss => "dismissed",
            Self::Accepted => "accepted",
            Self::Wrong => "rejected",
            Self::Liked => "liked",
        }
    }

    fn parse(key: &str) -> Option<Self> {
        [
            Self::NotThis,
            Self::Dismiss,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub date: String,
    pub title: String,
    pub reason: LedgerReason,
}

/// One MEMORY.md line: `- [2026-10-04] pulse: dismissed "summarize workboard" reason=not-this`.
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

/// Every pulse ledger line in MEMORY.md, oldest first. Other lines are ignored.
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

// ---------------------------------------------------------------- ranking

/// Rule weights, in the brief's order. Deterministic: no clock noise, no model.
pub const SCORE_SKILL_ASK: i64 = 1000;
pub const SCORE_DUE: i64 = 500;
pub const SCORE_UNFINISHED: i64 = 250;
pub const SCORE_REPEAT: i64 = 120;
pub const SCORE_LEARN_BOOST: i64 = 80;
pub const PENALTY_NOT_THIS: i64 = 400;
pub const PENALTY_DISMISS: i64 = 200;
/// A card below this is Quiet: nothing is shown for it.
pub const PULSE_THRESHOLD: i64 = 100;
/// The unlabeled top group on the Ideas view needs at least one real rule hit.
pub const TOP_GROUP_MIN: i64 = 250;
pub const TOP_GROUP_MAX: usize = 4;
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

/// The card's score: base by type, plus each rule that fires.
pub fn pulse_score(card: &UpdateCard, inputs: &PulseInputs) -> i64 {
    let text = card_text(card);
    let mut score = base_score(pulse_type(card));
    // 1. An explicit remind-me / need-to that a saved skill covers.
    if asks_explicitly(&text) && inputs.skills.iter().any(|s| same_topic(s, &text)) {
        score += SCORE_SKILL_ASK;
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
        || inputs.open_work.iter().any(|w| same_topic(w, &card.title))
    {
        score += SCORE_UNFINISHED;
    }
    // 4. Something you did three times.
    if card.runs >= 3 || card.kind == UpdateKind::AutomateOffer {
        score += SCORE_REPEAT;
    }
    // 5 and 6. The ledger: accepted Learn cards lift a topic, Not this and
    // Dismiss push it down. Dismiss only buries ideas: a dismissed Feed post is
    // gone, but tomorrow's run report or quiet digest with that title still posts.
    let idea = is_idea_card(card);
    for entry in inputs.ledger {
        let hit = entry.title.eq_ignore_ascii_case(card.title.trim())
            || same_topic(&entry.title, &card.title);
        if !hit {
            continue;
        }
        score += match entry.reason {
            LedgerReason::Accepted => SCORE_LEARN_BOOST,
            LedgerReason::NotThis | LedgerReason::Wrong => -PENALTY_NOT_THIS,
            LedgerReason::Dismiss if idea => -PENALTY_DISMISS,
            LedgerReason::Dismiss => 0,
            LedgerReason::Liked => 0,
        };
    }
    // 7. A proactive offer's help (value × confidence × reversibility, × 1000).
    // Cards without candidate data add nothing, so their order is unchanged.
    if let Some(p) = &card.pulse.proactive {
        score += i64::from(p.help);
    }
    score
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
    let mut out: Vec<(Ranked, u64)> = cards
        .iter()
        .filter(|c| pulse_visible(c, inputs.now_ms))
        .map(|c| {
            let score = pulse_score(c, inputs);
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

/// The Ideas view: an unlabeled top group (`None`) of the strongest cards, then
/// one group per category in `PulseCategory::ORDER`. Quiet cards and empty
/// groups are left out.
pub fn group_ideas(
    cards: &[UpdateCard],
    inputs: &PulseInputs,
) -> Vec<(Option<PulseCategory>, Vec<Ranked>)> {
    let ranked: Vec<Ranked> = rank_pulse(cards, inputs)
        .into_iter()
        .filter(|r| r.kind != PulseType::Quiet)
        .filter(|r| cards.iter().any(|c| c.id == r.id && is_idea_card(c)))
        .collect();
    let top: Vec<Ranked> = ranked
        .iter()
        .filter(|r| r.score >= TOP_GROUP_MIN)
        .take(TOP_GROUP_MAX)
        .cloned()
        .collect();
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

/// The Feed view: posts newest first. Quiet ones are left out.
pub fn feed_posts(cards: &[UpdateCard], inputs: &PulseInputs) -> Vec<UpdateCard> {
    let mut posts: Vec<UpdateCard> = cards
        .iter()
        .filter(|c| is_feed_card(c) && pulse_visible(c, inputs.now_ms))
        .filter(|c| pulse_score(c, inputs) >= PULSE_THRESHOLD)
        .cloned()
        .collect();
    posts.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    posts
}

// ---------------------------------------------------------------- buttons

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

// ---------------------------------------------------------------- feed instructions

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

/// The digest lookup with the person's instructions. They decide what belongs.
pub fn feed_prompt(steer: &str, instructions: &str) -> String {
    let base = crate::update_feed::digest_lookup_prompt(steer);
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

// ---------------------------------------------------------------- quiet hours

pub const QUIET_DIGEST_TITLE: &str = "While you were in quiet hours";
pub const QUIET_DIGEST_SOURCE: &str = "pulse:quiet";

/// Quiet hours ended: release every held card. Two or more become one digest
/// card (and ping nothing on their own); a single card is released as is.
/// Returns how many were released and the digest id, if one was posted.
pub fn release_quiet_batch(cards: &mut Vec<UpdateCard>, now_ms: u64) -> (usize, Option<String>) {
    let mut titles = Vec::new();
    let mut ids = Vec::new();
    for card in cards.iter_mut().filter(|c| c.held) {
        card.held = false;
        titles.push(card.title.clone());
        ids.push(card.id.clone());
    }
    let n = titles.len();
    if n < 2 {
        return (n, None);
    }
    for card in cards.iter_mut().filter(|c| ids.contains(&c.id)) {
        card.pulse.quiet_batched = true;
    }
    // Yesterday's dismissed digest must not swallow today's.
    cards.retain(|c| {
        !(c.source_id == QUIET_DIGEST_SOURCE && c.status == UpdateStatus::Dismissed)
    });
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
    post_update(cards, digest);
    (n, Some(id))
}

// ---------------------------------------------------------------- real images

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

// ---------------------------------------------------------------- migration

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
        assert_eq!(
            categorize("I can stage your $287.63 power bill before October 5"),
            PulseCategory::Financial
        );
        assert_eq!(
            categorize("I can find and cancel subscriptions you forgot"),
            PulseCategory::Financial
        );
        assert_eq!(
            categorize("Name a habit you want to break; I'll nudge you"),
            PulseCategory::Health
        );
        assert_eq!(
            categorize("Gather the family for a trivia night"),
            PulseCategory::Relationships
        );
        assert_eq!(
            categorize("Tell me the item and I'll compare prices"),
            PulseCategory::Shopping
        );
        assert_eq!(
            categorize("I can tailor your resume for entry-level jobs"),
            PulseCategory::Productivity
        );
        assert_eq!(
            categorize("I can map a spoiler-free path through Witcher 3"),
            PulseCategory::More
        );
        let labels: Vec<&str> = PulseCategory::ORDER.iter().map(|c| c.label()).collect();
        assert_eq!(
            labels,
            vec![
                "Financial Management",
                "Productivity",
                "Relationships",
                "Health & Fitness",
                "Shopping",
                "More ideas"
            ]
        );
        let mut stored = idea("i1", "Witcher 3 route", "", 1);
        stored.pulse.category = Some("shopping".into());
        assert_eq!(
            card_category(&stored),
            PulseCategory::Shopping,
            "a stored category wins"
        );
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
        let offer6 = crate::proactive::proactive_card(&c, crate::proactive::ProactiveRoute::ICan, 60);
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
        assert_eq!(rank_pulse(&with_offer, &inputs), rank_pulse(&reversed, &inputs), "input order never matters");
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
    fn ideas_group_under_a_top_group_then_categories_in_order() {
        let now = 9_000;
        let mut bill = idea("bill", "I can stage your power bill", "", 1);
        bill.modified = true;
        let subs = idea("subs", "I can cancel subscriptions you forgot", "", 2);
        let resume = idea("resume", "I can tailor your resume", "", 3);
        let gym = idea("gym", "I can plan a workout around your gear", "", 4);
        let witcher = idea("witcher", "I can map a path through Witcher 3", "", 5);
        let post = automation_done_card("a", "Nightly backup", "ok", 6);
        let cards = vec![bill, subs, resume, gym, witcher, post];
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
        assert_eq!(
            groups,
            vec![
                (None, vec!["bill".to_string()]),
                (Some("Financial Management"), vec!["subs".to_string()]),
                (Some("Productivity"), vec!["resume".to_string()]),
                (Some("Health & Fitness"), vec!["gym".to_string()]),
                (Some("More ideas"), vec!["witcher".to_string()]),
            ]
        );
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
            run_line(&idea("j", "Tidy the photos", "", 1)),
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
        assert!(!hide_from_home(&mut gone, &gone_id), "a dismissed card stays dismissed");
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
        let prompt = feed_prompt("Rust news", "Only Rust.\nNo sports.");
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
        let mut one = vec![automation_done_card("solo", "Solo run", "ok", 1)];
        one[0].held = true;
        assert_eq!(release_quiet_batch(&mut one, 9_000), (1, None));
        assert_eq!(one.len(), 1);
        assert!(!one[0].pulse.quiet_batched);
    }

    #[test]
    fn a_dismissed_feed_post_does_not_bury_the_next_one_with_its_title() {
        let ledger = parse_ledger(
            "- [2026-10-04] pulse: dismissed \"Nightly backup\" reason=dismiss\n",
        );
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
        assert_eq!(digest.body.as_deref(), Some("2 updates: Repo synced · Price check ran"));
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
        assert_eq!(PulseType::Automate.apply_heading(), "What Apply will schedule");
        assert_eq!(PulseType::Learn.apply_heading(), "What I'll learn");
        assert_eq!(PulseType::Watch.apply_heading(), "What Apply will do");
        assert_eq!(
            format!("{}{}.", FEED_EMPTY_LEAD, FEED_EMPTY_LINK),
            FEED_EMPTY
        );
        assert_eq!(IDEAS_EMPTY, "No ideas yet. I'll add one when I spot something worth doing.");
        assert_eq!(IDEAS_LOADING, "Looking for ideas in your recent work…");
        assert_eq!(IDEAS_SIGN_IN, "Sign in to Grok to get ideas.");
    }
}
