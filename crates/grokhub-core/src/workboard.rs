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

/// One tap on an idea. Files `BoardStatus::Todo` whose title is the task.
/// The feed card's canned line and the raw message are not stored.
pub fn file_idea_todo(cards: &mut Vec<BoardCard>, title: &str, detail: &str) -> String {
    let title = idea_todo_title(title, detail);
    if let Some(existing) = cards
        .iter()
        .find(|c| c.title.eq_ignore_ascii_case(&title) && c.status != BoardStatus::Dismissed)
    {
        return existing.id.clone();
    }
    let mut card = BoardCard::new(&title, "", "");
    card.status = BoardStatus::Todo;
    let id = card.id.clone();
    cards.push(card);
    id
}

/// Run-card label. A blank ask can still name the live turn from the thread.
/// An ordinary prompt, including "summarize the workboard", files nothing.
pub fn inflight_card_title(ask: &str, thread_label: &str) -> String {
    if ask.trim().is_empty() {
        return first_line(thread_label).chars().take(TASK_CHARS).collect();
    }
    todo_task_line(ask).unwrap_or_default()
}

const TASK_CHARS: usize = 80;

/// One line of work the user can act on. `None` for chat, questions, and background.
pub fn todo_task_line(text: &str) -> Option<String> {
    let line = first_line(text);
    if line.is_empty() || is_canned_line(&line) || is_background_line(&line) {
        return None;
    }
    let acted = without_polite(&line);
    if acted.is_empty() || is_canned_line(acted) || is_background_line(acted) {
        return None;
    }
    if !is_work_verb(&first_word(acted)) {
        return None;
    }
    let task = task_title(acted);
    if task.is_empty() {
        None
    } else {
        Some(task)
    }
}

/// Idea Accept title. The canned feed line and the raw message stay off the card.
pub fn idea_todo_title(title: &str, body: &str) -> String {
    for raw in [title, body] {
        let line = first_line(raw);
        if line.is_empty() || is_canned_line(&line) {
            continue;
        }
        if let Some(task) = todo_task_line(&line) {
            return task;
        }
    }
    for raw in [title, body] {
        if let Some(task) = steer_cover(&first_line(raw)) {
            return task;
        }
    }
    "Take the next step on this idea".to_string()
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn task_title(line: &str) -> String {
    let clipped: String = line.chars().take(TASK_CHARS).collect();
    let clipped = clipped.trim();
    let mut chars = clipped.chars();
    match chars.next() {
        Some(c) => {
            let mut titled = c.to_uppercase().collect::<String>();
            titled.push_str(chars.as_str());
            titled
        }
        None => String::new(),
    }
}

fn first_word(line: &str) -> String {
    line.split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_ascii_lowercase()
}

fn without_polite(line: &str) -> &str {
    let lower = line.to_ascii_lowercase();
    const PREFIXES: &[&str] = &[
        "please ",
        "can you ",
        "could you ",
        "would you ",
        "will you ",
        "i need you to ",
        "i want you to ",
        "let's ",
        "lets ",
    ];
    for prefix in PREFIXES {
        if lower.starts_with(prefix) {
            return line[prefix.len()..].trim();
        }
    }
    line
}

fn is_canned_line(line: &str) -> bool {
    let lower = line
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
    const EXACT: &[&str] = &[
        "idea",
        "worth doing",
        "from your brief",
        "from the brief",
        "from your profile",
        "want me to automate this and notify you here when done",
        "nothing queued",
        "no updates",
        "digest",
    ];
    EXACT.contains(&lower.as_str()) || lower.starts_with("digest ")
}

fn is_background_line(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    if lower.ends_with('?') || lower.starts_with('/') {
        return true;
    }
    const EXACT: &[&str] = &[
        "hi",
        "hello",
        "hey",
        "thanks",
        "thank you",
        "ok",
        "okay",
        "yes",
        "no",
        "lol",
        "yo",
        "status",
    ];
    if EXACT.contains(&lower.as_str()) {
        return true;
    }
    const STARTS: &[&str] = &[
        "summarize",
        "summarise",
        "summary",
        "recap",
        "what ",
        "what's ",
        "whats ",
        "how ",
        "why ",
        "when ",
        "who ",
        "where ",
        "tell me",
        "explain ",
        "describe ",
        "show me",
        "list ",
        "give me",
        "status of",
    ];
    if STARTS.iter().any(|prefix| lower.starts_with(prefix)) {
        return true;
    }
    const PHRASES: &[&str] = &[
        "summarize the workboard",
        "summarise the workboard",
        "summary of the workboard",
        "recap the workboard",
        "status of the workboard",
        "status of the board",
    ];
    PHRASES.iter().any(|phrase| lower.contains(phrase))
}

fn is_work_verb(word: &str) -> bool {
    const VERBS: &[&str] = &[
        "add",
        "apply",
        "archive",
        "build",
        "check",
        "clarify",
        "clean",
        "close",
        "configure",
        "cover",
        "create",
        "cut",
        "debug",
        "deploy",
        "draft",
        "edit",
        "enable",
        "file",
        "finish",
        "fix",
        "flash",
        "follow",
        "implement",
        "install",
        "merge",
        "move",
        "open",
        "pair",
        "pin",
        "publish",
        "push",
        "record",
        "release",
        "remove",
        "repair",
        "replace",
        "retry",
        "run",
        "schedule",
        "set",
        "ship",
        "start",
        "stop",
        "take",
        "test",
        "update",
        "verify",
        "wire",
        "write",
    ];
    VERBS.contains(&word)
}

/// "less crypto, more F1" is a steer, not the task. The work is to cover F1.
fn steer_cover(line: &str) -> Option<String> {
    if line.is_empty() || is_canned_line(line) || is_background_line(line) {
        return None;
    }
    let lower = line.to_ascii_lowercase();
    let idx = lower.find("more ")?;
    let before = lower[..idx].trim().trim_end_matches(',');
    let steered = before.is_empty() || before == "less" || before.starts_with("less ");
    if !steered {
        return None;
    }
    let more = line[idx + "more ".len()..]
        .trim()
        .trim_matches(|c: char| matches!(c, '.' | ','));
    if more.is_empty() || is_canned_line(more) || is_background_line(more) {
        return None;
    }
    let task = task_title(&format!("Cover {more}"));
    if task.is_empty() {
        None
    } else {
        Some(task)
    }
}

/// Run-start hook. One Doing or Done run card per thread.
/// Pins and linked user cards share `thread_id` and are left alone.
/// Proposed, Todo, and Blocked cards are never moved into Doing.
/// A finished run card for that thread is reused and retitled from the new ask.
pub fn upsert_inflight_card(cards: &mut Vec<BoardCard>, thread_id: &str, title: &str) -> bool {
    let thread_id = thread_id.trim();
    let title = title.trim();
    if thread_id.is_empty() || title.is_empty() {
        return false;
    }
    if let Some(c) = cards.iter_mut().rev().find(|c| {
        c.run
            && c.thread_id.as_deref() == Some(thread_id)
            && matches!(c.status, BoardStatus::InProgress | BoardStatus::Done)
    }) {
        if c.status == BoardStatus::InProgress {
            return false;
        }
        let next_title: String = title.chars().take(120).collect();
        c.undo = Some(InflightUndo::Reused {
            title: c.title.clone(),
            status: BoardStatus::Done,
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
        let mut board = Vec::new();
        let id = file_idea_todo(&mut board, "More F1", "from the brief");
        assert_eq!(board[0].status, BoardStatus::Todo);
        assert_eq!(board[0].title, "Cover F1");
        assert!(board[0].detail.is_empty());
        assert_eq!(file_idea_todo(&mut board, "more f1", "again"), id);
        assert_eq!(board.len(), 1);
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
            "Retry after block"
        ));
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].title, "Verify boot");
        assert_eq!(cards[0].status, BoardStatus::Blocked);
        assert_eq!(cards[1].title, "Retry after block");
        assert_eq!(cards[1].status, BoardStatus::InProgress);
        cards[1].status = BoardStatus::Dismissed;
        assert!(upsert_inflight_card(&mut cards, "thr-1", "Fresh card"));
        assert_eq!(cards.len(), 3);
        assert_eq!(cards[2].title, "Fresh card");
        assert_eq!(inflight_card_title("   ", "Cabin rail"), "Cabin rail");
        assert_eq!(
            inflight_card_title("  \nFlash the pi", "Thread"),
            "Flash the pi"
        );
    }

    #[test]
    fn chat_prompt_is_not_a_todo() {
        assert_eq!(todo_task_line("summarize the workboard"), None);
        assert_eq!(
            todo_task_line("  \nsecond line"),
            None,
            "a chat line with no work verb is not a task"
        );
        assert_eq!(inflight_card_title("summarize the workboard", "Thread"), "");
        assert_eq!(
            inflight_card_title("Can you summarize the workboard", "Thread"),
            ""
        );
        assert_eq!(
            inflight_card_title("please flash the pi", "Thread"),
            "Flash the pi"
        );
        assert_eq!(
            todo_task_line("Flash the pi\nextra chat"),
            Some("Flash the pi".into())
        );
        assert_eq!(todo_task_line("Verify boot"), Some("Verify boot".into()));
        assert_eq!(
            todo_task_line("Write the image"),
            Some("Write the image".into())
        );
        assert_eq!(
            idea_todo_title("less crypto, more F1", "less crypto, more F1"),
            "Cover F1"
        );
        assert_eq!(
            idea_todo_title("From your brief.", "summarize the workboard"),
            "Take the next step on this idea"
        );
        assert_eq!(idea_todo_title("Idea", "Flash the pi"), "Flash the pi");
        let mut board = Vec::new();
        file_idea_todo(
            &mut board,
            "Want me to automate this and notify you here when done?",
            "summarize the workboard",
        );
        assert_eq!(board[0].title, "Take the next step on this idea");
        assert!(board[0].detail.is_empty());
        assert!(!board[0].title.contains("summarize"));
        assert_eq!(board[0].status, BoardStatus::Todo);
        assert!(!board[0].run);
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
        assert!(upsert_inflight_card(
            &mut manual,
            "thr-2",
            "Clarify the pin"
        ));
        assert_eq!(manual[0].status, BoardStatus::Todo);
        assert_eq!(manual[0].title, "Manual todo");
        assert_eq!(manual[1].status, BoardStatus::InProgress);
        assert!(manual[1].run);
        assert!(settle_inflight_card(&mut manual, "thr-2"));
        assert_eq!(manual[0].status, BoardStatus::Todo);
        assert_eq!(manual[1].status, BoardStatus::Done);

        let mut parked = BoardCard::new("Stuck flash", "", "");
        parked.status = BoardStatus::Blocked;
        parked.thread_id = Some("thr-3".into());
        parked.run = true;
        let mut blocked = vec![parked];
        assert!(upsert_inflight_card(
            &mut blocked,
            "thr-3",
            "Retry the flash"
        ));
        assert_eq!(blocked[0].title, "Stuck flash");
        assert_eq!(blocked[0].status, BoardStatus::Blocked);
        assert_eq!(blocked[1].title, "Retry the flash");
        assert_eq!(blocked[1].status, BoardStatus::InProgress);
        assert!(blocked[1].run);
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
