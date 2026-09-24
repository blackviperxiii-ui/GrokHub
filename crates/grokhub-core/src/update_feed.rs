//! Home update feed. Cards live in `updates.json` until the user opens or dismisses one.
//!
//! An empty visible list paints nothing. There is no "No updates" placeholder.
//!
//! Live hook: `Cabin::poll_grok_loop` posts `automation_done` when a scheduled
//! `/loop` returns. A night recipe replay that counts as a finished run posts
//! the same kind from `fire_night`. `Cabin::commit_schedule` posts
//! `schedule_created` when a clock job or interval loop is saved.
//! `suggestion` and `automate_offer` are typed constructors for a later producer.
//! Interest learning and `interest_update` are out of scope.

use serde::{Deserialize, Serialize};

/// Newest cards painted in the home slot. Older undismissed cards stay on disk.
pub const FEED_PAINT_MAX: usize = 4;
const FEED_STORE_MAX: usize = 40;
const TITLE_CHARS: usize = 72;
const BODY_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateKind {
    AutomationDone,
    ScheduleCreated,
    Suggestion,
    AutomateOffer,
}

impl UpdateKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::AutomationDone => "Automation",
            Self::ScheduleCreated => "Scheduled",
            Self::Suggestion => "Suggestion",
            Self::AutomateOffer => "Automate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    Unread,
    Opened,
    Dismissed,
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
}

/// True when the home slot should paint. Dismissed cards do not count.
pub fn feed_visible(cards: &[UpdateCard]) -> bool {
    cards.iter().any(|c| c.status != UpdateStatus::Dismissed)
}

/// Undismissed cards, newest first.
pub fn visible_updates(cards: &[UpdateCard]) -> Vec<UpdateCard> {
    let mut out: Vec<UpdateCard> = cards
        .iter()
        .filter(|c| c.status != UpdateStatus::Dismissed)
        .cloned()
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    out
}

pub fn post_update(cards: &mut Vec<UpdateCard>, card: UpdateCard) {
    cards.retain(|c| c.id != card.id && c.status != UpdateStatus::Dismissed);
    cards.push(card);
    cards.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    if cards.len() > FEED_STORE_MAX {
        cards.truncate(FEED_STORE_MAX);
    }
}

/// Open keeps the card. A later dismiss is what drops it.
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
    UpdateCard {
        id: feed_card_id("done", source_id, created_at),
        kind: UpdateKind::AutomationDone,
        title,
        body,
        created_at,
        status: UpdateStatus::Unread,
        action: Some(UpdateAction::OpenAutomations),
    }
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
    UpdateCard {
        id: feed_card_id("sched", source_id, created_at),
        kind: UpdateKind::ScheduleCreated,
        title,
        body,
        created_at,
        status: UpdateStatus::Unread,
        action: Some(UpdateAction::OpenAutomations),
    }
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
    UpdateCard {
        id: feed_card_id("sugg", source_id, created_at),
        kind: UpdateKind::Suggestion,
        title,
        body,
        created_at,
        status: UpdateStatus::Unread,
        action: None,
    }
}

/// Typed offer. Accept marks it opened; a later finish posts `automation_done`.
pub fn automate_offer_card(source_id: &str, title: &str, created_at: u64) -> UpdateCard {
    let title = clip_line(title, TITLE_CHARS);
    let title = if title.is_empty() {
        "Want me to automate this and notify you here when done?".to_string()
    } else {
        title
    };
    UpdateCard {
        id: feed_card_id("offer", source_id, created_at),
        kind: UpdateKind::AutomateOffer,
        title,
        body: Some("Want me to automate this and notify you here when done?".into()),
        created_at,
        status: UpdateStatus::Unread,
        action: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, at: u64, status: UpdateStatus) -> UpdateCard {
        UpdateCard {
            id: id.into(),
            kind: UpdateKind::AutomationDone,
            title: id.into(),
            body: None,
            created_at: at,
            status,
            action: None,
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
}
