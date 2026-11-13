//! The router's money surfaces on the shell, as the cabin had them: the
//! budget cards over the chat box (80% of the week once a week, the ask at
//! 100%, the upgrade nudge) and Settings → Account's SuperGrok line (how
//! much room the pool has, or that xAI's plan gate turned the sign-in away).
//!
//! The shell has no path to credits, top-ups or plans: the nudge's details
//! name grok.com and open nothing.

use std::path::Path;
use std::sync::Mutex;

use grokhub_agent::plan_room;
use grokhub_agent::route::budget::{self, Budget, BudgetNotes, NudgeFacts, NudgeGate};
use grokhub_core::model_registry::store::load_registry;
use serde::Serialize;

/// Settings → Account when xAI's plan gate refused the sign-in.
pub const ACCOUNT_PLAN_HINT: &str = "Signed in, but xAI says your Grok plan can't take chats from apps right now: it doesn't include app use, or its pool is used up. Check your plan on x.ai, or add an API key.";

const BUDGET_EYEBROW: &str = "This week's usage";
const NUDGE_EYEBROW: &str = "Your plan";

/// One card over the chat box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Card {
    /// 80% of the week used (once a week).
    Eighty,
    /// 100%: user-facing work waits for this answer.
    Ask,
    /// The upgrade nudge, with its words.
    Nudge(String),
}

/// A card as the page shows it. Click only: nothing answers it on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    /// `eighty`, `ask` or `nudge`.
    pub key: &'static str,
    pub eyebrow: &'static str,
    pub title: String,
    pub note: &'static str,
    pub primary: &'static str,
    pub secondary: &'static str,
}

/// What a click did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answered {
    /// One line for the page.
    pub status: String,
    /// Open Settings → AI models (Change cap).
    pub open_ai_models: bool,
    /// The nudge's details, shown in place.
    pub details: Option<String>,
}

#[derive(Debug, Default)]
struct Cards {
    card: Option<Card>,
    facts: NudgeFacts,
}

static CARDS: Mutex<Cards> = Mutex::new(Cards {
    card: None,
    facts: NudgeFacts {
        would_upgrade: 0,
        fails: 0,
        last_of: 0,
        last_failed: 0,
    },
});

fn cards() -> std::sync::MutexGuard<'static, Cards> {
    CARDS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn view(card: &Card) -> CardView {
    match card {
        Card::Eighty => CardView {
            key: "eighty",
            eyebrow: BUDGET_EYEBROW,
            title: budget::EIGHTY_CARD.into(),
            note: "Settings → AI models has the cap.",
            primary: "OK",
            secondary: "Change cap",
        },
        Card::Ask => CardView {
            key: "ask",
            eyebrow: BUDGET_EYEBROW,
            title: budget::BUDGET_ASK.into(),
            note: "Background work stays paused until next week.",
            primary: "Keep going",
            secondary: "Wait",
        },
        Card::Nudge(text) => CardView {
            key: "nudge",
            eyebrow: NUDGE_EYEBROW,
            title: text.clone(),
            note: "Nothing opens until you click.",
            primary: "Details",
            secondary: "Not now",
        },
    }
}

/// The card up now. A chat that paused at 100% raises the ask first.
pub fn current() -> Option<CardView> {
    let mut c = cards();
    if grokhub_agent::route::live::take_budget_ask() {
        c.card = Some(Card::Ask);
    }
    c.card.as_ref().map(view)
}

/// A tally landed: the 80% card once a week, else the nudge when its rule
/// allows. One card at a time; no OS notice.
pub fn landed(
    config_dir: &Path,
    b: &Budget,
    facts: NudgeFacts,
    gate: NudgeGate,
    tier: Option<&str>,
    now: u64,
) {
    let mut c = cards();
    c.facts = facts.clone();
    if c.card.is_some() {
        return;
    }
    let mut notes = BudgetNotes::load(config_dir);
    if notes.eighty_due(b) {
        notes.eighty_week = b.week;
        notes.save(config_dir);
        c.card = Some(Card::Eighty);
        return;
    }
    if budget::nudge_due(&facts, &notes, gate, now) {
        notes.nudge_shown_ms = now;
        notes.save(config_dir);
        c.card = Some(Card::Nudge(budget::nudge_text(&facts, tier)));
    }
}

/// A click on the card. `primary` is OK / Keep going / Details.
pub fn answer(config_dir: &Path, key: &str, primary: bool, now: u64) -> Result<Answered, String> {
    let mut c = cards();
    let card = match c.card.take() {
        Some(card) if view(&card).key == key => card,
        other => {
            c.card = other;
            return Err("That card is gone.".into());
        }
    };
    let said = |status: &str| Answered {
        status: status.to_string(),
        open_ai_models: false,
        details: None,
    };
    Ok(match (card, primary) {
        (Card::Eighty, true) => said("OK. Chats stay at normal quality."),
        (Card::Eighty, false) => Answered {
            open_ai_models: true,
            ..said("")
        },
        (Card::Ask, true) => {
            let mut notes = BudgetNotes::load(config_dir);
            notes.go_on_week = budget::week_of(now);
            notes.save(config_dir);
            said("Chats go on this week. Background work waits.")
        }
        (Card::Ask, false) => said("Chats wait until next week or a higher cap."),
        (Card::Nudge(_), true) => Answered {
            details: Some(budget::nudge_details(&c.facts)),
            ..said("")
        },
        (Card::Nudge(_), false) => {
            let mut notes = BudgetNotes::load(config_dir);
            notes.nudge_muted_until_ms = now + budget::NUDGE_EVERY_MS;
            notes.save(config_dir);
            said(&format!(
                "Not now. {} won't bring this up for 30 days.",
                grokhub_core::brand::product_name()
            ))
        }
    })
}

/// One tally: this week's spend against the cap, and the nudge's facts.
pub fn tally(config_dir: &Path, gate: NudgeGate, now: u64) {
    let reg = load_registry(config_dir);
    let cap = grokhub_agent::route::host::read(config_dir)
        .spend
        .weekly_cap_usd;
    let week = budget::load_week(config_dir, &reg, now);
    let b = Budget::of(&week, reg.entitlement.credential, cap);
    let records = budget::read_records(config_dir);
    let facts = budget::nudge_facts(&records, now);
    landed(
        config_dir,
        &b,
        facts,
        gate,
        reg.entitlement.tier.as_deref(),
        now,
    );
}

/// Settings → Account's line under a live sign-in: the plan gate's refusal
/// within the last hour, else the pool's room as xAI last reported it.
pub fn plan_note(
    refused_ms: Option<u64>,
    room: Option<&plan_room::PlanRoom>,
    now_ms: u64,
) -> Option<String> {
    if refused_ms.is_some_and(|t| now_ms.saturating_sub(t) <= plan_room::FRESH_MS) {
        return Some(ACCOUNT_PLAN_HINT.into());
    }
    room.and_then(|r| plan_room::room_line(r, now_ms))
}

/// [`plan_note`] from what this run's signed-in replies said.
pub fn plan_note_now() -> Option<String> {
    plan_note(
        plan_room::refused_since(),
        plan_room::latest().as_ref(),
        grokhub_core::now_ms(),
    )
}

#[cfg(test)]
#[path = "spend_cards_tests.rs"]
mod tests;
