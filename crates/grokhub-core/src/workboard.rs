use serde::{Deserialize, Serialize};

use crate::uid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardStatus {
    Proposed,
    Approved,
    Staged,
    InProgress,
    Done,
    Dismissed,
    Todo,
    Blocked,
}

impl BoardStatus {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "proposed" => Some(Self::Proposed),
            "approved" => Some(Self::Approved),
            "staged" => Some(Self::Staged),
            "in_progress" | "in-progress" | "progress" | "doing" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "dismissed" | "dismiss" | "archived" | "archive" => Some(Self::Dismissed),
            "todo" => Some(Self::Todo),
            "blocked" | "block" => Some(Self::Blocked),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Staged => "staged",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Dismissed => "dismissed",
            Self::Todo => "todo",
            Self::Blocked => "blocked",
        }
    }

    pub fn column(self) -> Option<KanbanColumn> {
        match self {
            Self::Todo | Self::Proposed | Self::Approved | Self::Staged => Some(KanbanColumn::Todo),
            Self::InProgress => Some(KanbanColumn::Doing),
            Self::Blocked => Some(KanbanColumn::Blocked),
            Self::Done => Some(KanbanColumn::Done),
            Self::Dismissed => None,
        }
    }
}

/// Four columns on the Workboards page. Archived (`dismissed`) stays off the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanbanColumn {
    Todo,
    Doing,
    Blocked,
    Done,
}

impl KanbanColumn {
    pub const ALL: [KanbanColumn; 4] = [
        KanbanColumn::Todo,
        KanbanColumn::Doing,
        KanbanColumn::Blocked,
        KanbanColumn::Done,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Todo => "Todo",
            Self::Doing => "Doing",
            Self::Blocked => "Blocked",
            Self::Done => "Done",
        }
    }

    pub fn status(self) -> BoardStatus {
        match self {
            Self::Todo => BoardStatus::Todo,
            Self::Doing => BoardStatus::InProgress,
            Self::Blocked => BoardStatus::Blocked,
            Self::Done => BoardStatus::Done,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCard {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub status: BoardStatus,
    #[serde(default)]
    pub priority: String,
    /// Cabin thread id. Opening it switches to that chat; the board stays on the rail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// The one run-start card for `thread_id`. Pins and linked cards share the
    /// thread so Open chat works, and are not this card.
    #[serde(default, skip_serializing_if = "is_false")]
    pub run: bool,
    /// How this hook put the card in Doing. Cleared on settle. Not persisted.
    #[serde(skip)]
    undo: Option<InflightUndo>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone)]
enum InflightUndo {
    Created,
    Reused { title: String, status: BoardStatus },
}

impl BoardCard {
    pub fn new(title: &str, detail: &str, priority: &str) -> Self {
        Self {
            id: uid("w"),
            title: title.trim().chars().take(120).collect(),
            detail: detail.trim().chars().take(2000).collect(),
            status: BoardStatus::Proposed,
            priority: priority.trim().chars().take(16).collect(),
            thread_id: None,
            run: false,
            undo: None,
        }
    }
}

/// First line of the user ask, else the thread label. Empty when both are blank.
pub fn inflight_card_title(ask: &str, thread_label: &str) -> String {
    fn one_line(s: &str) -> String {
        s.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .chars()
            .take(80)
            .collect()
    }
    let from_ask = one_line(ask);
    if !from_ask.is_empty() {
        return from_ask;
    }
    one_line(thread_label)
}

/// Run-start hook. One non-archived run card per thread, status `doing`.
/// Pins and linked user cards share `thread_id` and are left alone.
/// A finished run card for that thread is reused and retitled from the new ask.
pub fn upsert_inflight_card(cards: &mut Vec<BoardCard>, thread_id: &str, title: &str) -> bool {
    let thread_id = thread_id.trim();
    let title = title.trim();
    if thread_id.is_empty() || title.is_empty() {
        return false;
    }
    if let Some(c) = cards.iter_mut().rev().find(|c| {
        c.run && c.thread_id.as_deref() == Some(thread_id) && c.status != BoardStatus::Dismissed
    }) {
        if c.status == BoardStatus::InProgress {
            return false;
        }
        let next_title = if c.status == BoardStatus::Done {
            title.chars().take(120).collect()
        } else {
            c.title.clone()
        };
        c.undo = Some(InflightUndo::Reused {
            title: c.title.clone(),
            status: c.status,
        });
        c.title = next_title;
        c.status = BoardStatus::InProgress;
        return true;
    }
    let mut card = BoardCard::new(title, "", "");
    card.status = BoardStatus::InProgress;
    card.thread_id = Some(thread_id.to_string());
    card.run = true;
    card.undo = Some(InflightUndo::Created);
    cards.push(card);
    true
}

/// A new attempt does not own a Doing card left by an earlier stop.
pub fn release_inflight_card(cards: &mut [BoardCard], thread_id: &str) {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return;
    }
    if let Some(c) = cards.iter_mut().rev().find(|c| {
        c.run && c.thread_id.as_deref() == Some(thread_id) && c.status == BoardStatus::InProgress
    }) {
        c.undo = None;
    }
}

/// Run-complete hook. The in-flight run card for this thread moves to `done`.
pub fn settle_inflight_card(cards: &mut [BoardCard], thread_id: &str) -> bool {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return false;
    }
    let Some(c) = cards.iter_mut().rev().find(|c| {
        c.run && c.thread_id.as_deref() == Some(thread_id) && c.status == BoardStatus::InProgress
    }) else {
        return false;
    };
    c.undo = None;
    c.status = BoardStatus::Done;
    true
}

/// The turn never finished. A card this hook created is removed.
/// A reused card returns to the title and status it had before Doing.
pub fn abandon_inflight_card(cards: &mut Vec<BoardCard>, thread_id: &str) -> bool {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return false;
    }
    let Some(idx) = cards.iter().rposition(|c| {
        c.run && c.thread_id.as_deref() == Some(thread_id) && c.status == BoardStatus::InProgress
    }) else {
        return false;
    };
    match cards[idx].undo.take() {
        Some(InflightUndo::Created) => {
            cards.remove(idx);
            true
        }
        Some(InflightUndo::Reused { title, status }) => {
            cards[idx].title = title;
            cards[idx].status = status;
            true
        }
        None => false,
    }
}

/// `WORK_PIN:` adds a card. `WORK_UPDATE:` moves one. Pins inherit `thread_id` when set.
pub fn apply_assistant_work_marks(cards: &mut Vec<BoardCard>, text: &str, thread_id: &str) -> bool {
    let mut changed = false;
    let thread_id = thread_id.trim();
    for mut card in extract_work_pins(text) {
        let exists = cards.iter().any(|c| {
            c.title.eq_ignore_ascii_case(&card.title) && c.status != BoardStatus::Dismissed
        });
        if exists {
            continue;
        }
        if !thread_id.is_empty() {
            card.thread_id = Some(thread_id.to_string());
        }
        cards.push(card);
        changed = true;
    }
    for (key, status) in extract_work_updates(text) {
        if apply_work_update(cards, &key, status) {
            changed = true;
        }
    }
    changed
}

/// `WORK_PIN: title | detail | priority=high`
pub fn parse_work_pin(line: &str) -> Option<BoardCard> {
    let rest = line.trim().strip_prefix("WORK_PIN:")?;
    let mut parts = rest.split('|').map(|s| s.trim());
    let title = parts.next().filter(|s| !s.is_empty())?;
    let detail = parts.next().unwrap_or("").to_string();
    let mut priority = String::new();
    for p in parts {
        if let Some(v) = p.strip_prefix("priority=") {
            priority = v.to_string();
        }
    }
    Some(BoardCard::new(title, &detail, &priority))
}

/// `WORK_UPDATE: id-or-title | status=in_progress`
pub fn parse_work_update(line: &str) -> Option<(String, BoardStatus)> {
    let rest = line.trim().strip_prefix("WORK_UPDATE:")?;
    let mut parts = rest.split('|').map(|s| s.trim());
    let key = parts.next().filter(|s| !s.is_empty())?.to_string();
    let mut status = None;
    for p in parts {
        if let Some(v) = p.strip_prefix("status=") {
            status = BoardStatus::parse(v);
        }
    }
    Some((key, status?))
}

pub fn apply_work_update(cards: &mut [BoardCard], key: &str, status: BoardStatus) -> bool {
    if let Some(c) = cards
        .iter_mut()
        .find(|c| c.id == key || c.title.eq_ignore_ascii_case(key))
    {
        if c.status == status {
            return false;
        }
        c.status = status;
        return true;
    }
    false
}

pub fn extract_work_pins(text: &str) -> Vec<BoardCard> {
    text.lines().filter_map(parse_work_pin).collect()
}

pub fn extract_work_updates(text: &str) -> Vec<(String, BoardStatus)> {
    text.lines().filter_map(parse_work_update).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_and_update() {
        let card = parse_work_pin("WORK_PIN: Flash the pi | write image | priority=high").unwrap();
        assert_eq!(card.title, "Flash the pi");
        assert_eq!(card.priority, "high");
        assert_eq!(card.status, BoardStatus::Proposed);
        assert_eq!(card.status.column(), Some(KanbanColumn::Todo));
        let (key, st) = parse_work_update("WORK_UPDATE: Flash the pi | status=doing").unwrap();
        let mut cards = vec![card];
        assert!(apply_work_update(&mut cards, &key, st));
        assert_eq!(cards[0].status, BoardStatus::InProgress);
        assert_eq!(cards[0].status.column(), Some(KanbanColumn::Doing));
    }

    #[test]
    fn columns_cover_todo_doing_blocked_done() {
        assert_eq!(BoardStatus::Todo.column(), Some(KanbanColumn::Todo));
        assert_eq!(BoardStatus::Blocked.column(), Some(KanbanColumn::Blocked));
        assert_eq!(BoardStatus::Done.column(), Some(KanbanColumn::Done));
        assert_eq!(BoardStatus::Dismissed.column(), None);
        assert_eq!(KanbanColumn::Blocked.status(), BoardStatus::Blocked);
        assert_eq!(KanbanColumn::Blocked.label(), "Blocked");
    }

    #[test]
    fn inflight_upsert_and_settle_round_trip() {
        let mut cards = Vec::new();
        assert!(!upsert_inflight_card(&mut cards, "", "ask"));
        assert!(upsert_inflight_card(&mut cards, "thr-1", "Flash the pi"));
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].status, BoardStatus::InProgress);
        assert_eq!(cards[0].thread_id.as_deref(), Some("thr-1"));
        assert!(!upsert_inflight_card(&mut cards, "thr-1", "Flash the pi"));
        assert!(settle_inflight_card(&mut cards, "thr-1"));
        assert_eq!(cards[0].status, BoardStatus::Done);
        assert!(upsert_inflight_card(&mut cards, "thr-1", "Verify boot"));
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "Verify boot");
        assert_eq!(cards[0].status, BoardStatus::InProgress);
        cards[0].status = BoardStatus::Blocked;
        assert!(upsert_inflight_card(
            &mut cards,
            "thr-1",
            "ignored while blocked title"
        ));
        assert_eq!(cards[0].title, "Verify boot");
        assert_eq!(cards[0].status, BoardStatus::InProgress);
        cards[0].status = BoardStatus::Dismissed;
        assert!(upsert_inflight_card(&mut cards, "thr-1", "Fresh card"));
        assert_eq!(cards.len(), 2);
        assert_eq!(
            inflight_card_title("  \nsecond line", "Thread"),
            "second line"
        );
        assert_eq!(inflight_card_title("   ", "Cabin rail"), "Cabin rail");
    }

    #[test]
    fn assistant_pin_links_the_thread_and_survives_json() {
        let mut cards = Vec::new();
        let text = "WORK_PIN: Write the image | sd card | priority=high\nWORK_UPDATE: Write the image | status=blocked";
        assert!(apply_assistant_work_marks(&mut cards, text, "thr-9"));
        assert_eq!(cards[0].thread_id.as_deref(), Some("thr-9"));
        assert_eq!(cards[0].status, BoardStatus::Blocked);
        assert!(!apply_assistant_work_marks(&mut cards, text, "thr-9"));
        let raw = serde_json::to_string(&cards).unwrap();
        let back: Vec<BoardCard> = serde_json::from_str(&raw).unwrap();
        assert_eq!(back[0].title, "Write the image");
        assert_eq!(back[0].status, BoardStatus::Blocked);
        assert_eq!(back[0].thread_id.as_deref(), Some("thr-9"));
        let legacy = r#"[{"id":"w1","title":"Old","detail":"","status":"proposed","priority":""}]"#;
        let old: Vec<BoardCard> = serde_json::from_str(legacy).unwrap();
        assert!(old[0].thread_id.is_none());
        assert!(!old[0].run);
        assert_eq!(old[0].status.column(), Some(KanbanColumn::Todo));
    }

    #[test]
    fn inflight_does_not_steal_a_pin_or_linked_card() {
        let mut cards = Vec::new();
        assert!(upsert_inflight_card(&mut cards, "thr-1", "Flash the pi"));
        assert!(settle_inflight_card(&mut cards, "thr-1"));
        assert!(apply_assistant_work_marks(
            &mut cards,
            "WORK_PIN: Write the image | sd card | priority=high",
            "thr-1",
        ));
        assert!(upsert_inflight_card(&mut cards, "thr-1", "Verify boot"));
        assert_eq!(cards[0].title, "Verify boot");
        assert_eq!(cards[0].status, BoardStatus::InProgress);
        assert!(cards[0].run);
        assert_eq!(cards[1].title, "Write the image");
        assert_eq!(cards[1].status, BoardStatus::Proposed);
        assert!(!cards[1].run);
        assert!(settle_inflight_card(&mut cards, "thr-1"));
        assert_eq!(cards[0].status, BoardStatus::Done);
        assert_eq!(cards[1].status, BoardStatus::Proposed);

        let mut linked = BoardCard::new("Manual todo", "notes", "");
        linked.status = BoardStatus::Todo;
        linked.thread_id = Some("thr-2".into());
        let mut manual = vec![linked];
        assert!(upsert_inflight_card(&mut manual, "thr-2", "Clarify the pin"));
        assert_eq!(manual[0].status, BoardStatus::Todo);
        assert_eq!(manual[0].title, "Manual todo");
        assert_eq!(manual[1].status, BoardStatus::InProgress);
        assert!(manual[1].run);
        assert!(settle_inflight_card(&mut manual, "thr-2"));
        assert_eq!(manual[0].status, BoardStatus::Todo);
        assert_eq!(manual[1].status, BoardStatus::Done);
    }

    #[test]
    fn abandon_drops_a_new_card_and_restores_a_reused_one() {
        let mut cards = Vec::new();
        assert!(upsert_inflight_card(&mut cards, "thr-1", "Flash the pi"));
        assert!(abandon_inflight_card(&mut cards, "thr-1"));
        assert!(cards.is_empty());

        assert!(upsert_inflight_card(&mut cards, "thr-1", "Flash the pi"));
        assert!(settle_inflight_card(&mut cards, "thr-1"));
        assert!(upsert_inflight_card(&mut cards, "thr-1", "Verify boot"));
        assert!(abandon_inflight_card(&mut cards, "thr-1"));
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "Flash the pi");
        assert_eq!(cards[0].status, BoardStatus::Done);

        assert!(upsert_inflight_card(&mut cards, "thr-1", "Verify boot"));
        release_inflight_card(&mut cards, "thr-1");
        assert!(!abandon_inflight_card(&mut cards, "thr-1"));
        assert_eq!(cards[0].title, "Verify boot");
        assert_eq!(cards[0].status, BoardStatus::InProgress);
    }
}
