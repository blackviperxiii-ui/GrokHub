//! Which of Snooze and Delete a card's row offers. Snooze suits cards tied
//! to a time (a reminder, a deadline, a schedule, something that expires);
//! Delete suits cards that are stale, repeat another card, or report a
//! one-off that is already done. When neither reading is clear the row shows
//! both, so the person always has the choice. No model call, no file.

use crate::update_feed::{UpdateCard, UpdateKind};

/// A card older than this counts as stale.
pub const STALE_MS: u64 = 3 * 24 * 3_600_000;

/// Words that tie a card to a time.
const TIME_WORDS: &[&str] = &[
    "today",
    "tonight",
    "tomorrow",
    "this week",
    "this weekend",
    "deadline",
    "due",
    "ends",
    "expires",
    "until",
    "remind",
    "reminder",
    "later",
    "starts",
    "launch",
    "release",
    "sale",
    "schedule",
    "scheduled",
    "appointment",
    "meeting",
    "o'clock",
    " am ",
    " pm ",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Offers {
    pub snooze: bool,
    pub delete: bool,
}

fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// True when the card is about a time: it expires, it was scheduled, or its
/// words point at a moment.
pub fn time_bound(card: &UpdateCard) -> bool {
    if card.expires_at.is_some() || card.kind == UpdateKind::ScheduleCreated {
        return true;
    }
    let text = format!(
        " {} {} ",
        norm(&card.title),
        norm(card.body.as_deref().unwrap_or(""))
    );
    TIME_WORDS.iter().any(|w| {
        let w = w.trim();
        text.split(|c: char| !c.is_alphanumeric() && c != '\'')
            .any(|tok| tok == w)
            || (w.contains(' ') && text.contains(w))
    })
}

/// True when the card has done its job: a finished run, a change already
/// made, or something done for the person.
pub fn one_off(card: &UpdateCard) -> bool {
    matches!(
        card.kind,
        UpdateKind::AutomationDone | UpdateKind::SelfChange | UpdateKind::DoneForYou
    )
}

/// True when another card on the page has the same title.
pub fn duplicate(card: &UpdateCard, all: &[UpdateCard]) -> bool {
    let me = norm(&card.title);
    !me.is_empty() && all.iter().any(|c| c.id != card.id && norm(&c.title) == me)
}

/// True when the card is older than [`STALE_MS`].
pub fn stale(card: &UpdateCard, now_ms: u64) -> bool {
    now_ms.saturating_sub(card.created_at) > STALE_MS
}

/// What the row offers for this card. Unsure (no clear reading) means both.
pub fn offers(card: &UpdateCard, all: &[UpdateCard], now_ms: u64) -> Offers {
    let delete = stale(card, now_ms) || duplicate(card, all) || one_off(card);
    // A stale card's moment has passed: Delete, not Snooze.
    let snooze = time_bound(card) && !stale(card, now_ms);
    if !snooze && !delete {
        return Offers {
            snooze: true,
            delete: true,
        };
    }
    Offers { snooze, delete }
}

#[cfg(test)]
#[path = "card_offers_tests.rs"]
mod tests;
