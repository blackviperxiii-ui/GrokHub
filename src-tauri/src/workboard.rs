//! Workboard (shell parity, KB-06): the cabin's work cards from the same
//! `workboard.json`. Every change goes through `grokhub_core::work_file`'s lock
//! and per-card merge, the same rule as the cabin's saves, so neither app
//! overwrites the other. A card's progress log is read only when its drawer
//! opens, and only its tail.
//!
//! Token budget (KB-04): all of this is local file work. Nothing here reaches
//! a prompt or the tool list; Resume only drafts a short first message that you
//! send yourself.

use std::path::{Path, PathBuf};

use grokhub_core::work_file::{board_stamp, read_board, read_work_log_tail, update_board};
use grokhub_core::work_item::{clip_line, remove_work_log};
use grokhub_core::{
    BoardCard, BoardStatus, KanbanColumn, StepStatus, WorkKind, WorkLogEvent, WorkOrigin,
};
use serde::{Deserialize, Serialize};

const FILE: &str = "workboard.json";
/// Lines the log drawer shows, newest last (the cabin's drawer shows as many).
pub const LOG_LINES: usize = 20;
/// Most of a progress log the drawer reads: its tail, never the whole file.
pub const LOG_READ_MAX: u64 = 32 * 1024;
/// Longest Resume draft. KB-04's `RESUME_BLOCK_MAX` (1,200 B) caps the real
/// resume block KB-03 sends; the draft stays inside the same cap.
pub const RESUME_DRAFT_MAX: usize = 1200;
const GONE: &str = "That card is gone. It may have been removed in the cabin.";

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

fn work_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("work")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub text: String,
    /// planned, doing, done, skipped or failed.
    pub status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOff {
    pub goal: String,
    pub next: String,
    pub blocker: String,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub notes: String,
    /// todo, doing, blocked, done, followUp or archived.
    pub column: &'static str,
    /// "Agent" or "You", as the cabin's badge reads.
    pub owner: &'static str,
    /// Automation, Background, Episode or Flow; a plain task has none.
    pub kind: Option<&'static str>,
    /// Where the work left off, the same line the cabin shows.
    pub left_off: Option<String>,
    pub steps: Vec<Step>,
    pub checkpoint: Option<LeftOff>,
    /// Steps done (or skipped) and in all; the column's place without steps.
    pub progress: [u32; 2],
    /// A paused or interrupted run: Resume and Hand back apply.
    pub can_resume: bool,
    /// The chat that works it.
    pub chat: Option<String>,
    pub updated_ms: u64,
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub id: &'static str,
    pub label: &'static str,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    /// Reports from scheduled runs, above the columns.
    pub follow_up: Vec<Card>,
    pub columns: Vec<Column>,
    /// Off the board, newest first.
    pub archived: Vec<Card>,
    /// The file's mtime, for the view's change check.
    pub stamp: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Act {
    /// To the end of a column: todo, doing, blocked or done.
    Move {
        column: String,
    },
    /// One place up or down in its column, through `rank` like the cabin.
    Rank {
        up: bool,
    },
    /// A paused run becomes yours: back to To do, and the sweep leaves it.
    HandBack,
    Archive,
    Restore,
    /// Gone for good, with its progress log.
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resume {
    /// The chat to open: the card's own, else the new one passed in.
    pub conversation_id: String,
    /// The first message, put in the chat box for you to send.
    pub prompt: String,
    pub board: Board,
}

fn column_id(column: KanbanColumn) -> &'static str {
    match column {
        KanbanColumn::Todo => "todo",
        KanbanColumn::Doing => "doing",
        KanbanColumn::Blocked => "blocked",
        KanbanColumn::Done => "done",
        KanbanColumn::FollowUp => "followUp",
    }
}

fn parse_column(id: &str) -> Option<KanbanColumn> {
    KanbanColumn::ALL.into_iter().find(|c| column_id(*c) == id)
}

fn step_status(s: StepStatus) -> &'static str {
    match s {
        StepStatus::Planned => "planned",
        StepStatus::Doing => "doing",
        StepStatus::Done => "done",
        StepStatus::Skipped => "skipped",
        StepStatus::Failed => "failed",
    }
}

fn kind_label(kind: WorkKind) -> Option<&'static str> {
    match kind {
        WorkKind::Task => None,
        WorkKind::Automation => Some("Automation"),
        WorkKind::Background => Some("Background"),
        WorkKind::Episode => Some("Episode"),
        WorkKind::Flow => Some("Flow"),
    }
}

/// The checkpoint's next step, else the step being done, else the first step
/// not started. Done and archived cards have none (the cabin's rule).
fn left_off(card: &BoardCard) -> Option<String> {
    if matches!(card.status, BoardStatus::Done | BoardStatus::Dismissed) {
        return None;
    }
    if let Some(next) = card
        .checkpoint
        .as_ref()
        .map(|c| c.next.trim())
        .filter(|n| !n.is_empty())
    {
        return Some(next.to_string());
    }
    let step = |want: StepStatus| card.steps.iter().find(|s| s.status == want);
    step(StepStatus::Doing)
        .or_else(|| step(StepStatus::Planned))
        .map(|s| s.text.trim().to_string())
        .filter(|t| !t.is_empty())
}

fn can_resume(card: &BoardCard) -> bool {
    card.status == BoardStatus::Blocked
        && (card.run || card.checkpoint.as_ref().is_some_and(|c| !c.is_empty()))
}

/// Steps done or skipped over all steps; without steps, the column's place.
fn progress(card: &BoardCard) -> [u32; 2] {
    if card.steps.is_empty() {
        return match card.status.column() {
            Some(KanbanColumn::Done) => [4, 4],
            Some(KanbanColumn::Blocked) => [2, 4],
            Some(KanbanColumn::Doing) => [2, 4],
            Some(KanbanColumn::Todo) | Some(KanbanColumn::FollowUp) | None => [0, 4],
        };
    }
    let total = card.steps.len() as u32;
    if card.status == BoardStatus::Done {
        return [total, total];
    }
    let filled = card
        .steps
        .iter()
        .filter(|s| matches!(s.status, StepStatus::Done | StepStatus::Skipped))
        .count() as u32;
    [filled, total]
}

fn card_view(c: &BoardCard) -> Card {
    Card {
        id: c.id.clone(),
        title: c.title.clone(),
        detail: c.detail.clone(),
        notes: c.notes.clone(),
        column: c.status.column().map(column_id).unwrap_or("archived"),
        owner: match c.origin {
            Some(WorkOrigin::Agent) => "Agent",
            Some(WorkOrigin::User) | None => "You",
        },
        kind: kind_label(c.kind),
        left_off: left_off(c),
        steps: c
            .steps
            .iter()
            .map(|s| Step {
                text: s.text.clone(),
                status: step_status(s.status),
            })
            .collect(),
        checkpoint: c
            .checkpoint
            .as_ref()
            .filter(|k| !k.is_empty())
            .map(|k| LeftOff {
                goal: k.goal.clone(),
                next: k.next.clone(),
                blocker: k.blocker.clone(),
                refs: k.refs.clone(),
            }),
        progress: progress(c),
        can_resume: can_resume(c),
        chat: c.thread_id.clone(),
        updated_ms: c.updated_ms.max(c.active_ms),
        fresh: c.fresh,
    }
}

/// A column's cards in board order: lowest rank first, file order on a tie.
fn in_column(cards: &[BoardCard], column: KanbanColumn) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..cards.len())
        .filter(|&i| cards[i].status.column() == Some(column))
        .collect();
    idx.sort_by_key(|&i| cards[i].rank);
    idx
}

fn board(cards: &[BoardCard], stamp: u64) -> Board {
    let views = |column| {
        in_column(cards, column)
            .into_iter()
            .map(|i| card_view(&cards[i]))
            .collect::<Vec<_>>()
    };
    let mut archived: Vec<&BoardCard> = cards
        .iter()
        .filter(|c| c.status == BoardStatus::Dismissed)
        .collect();
    archived.sort_by_key(|c| std::cmp::Reverse(c.archived_ms));
    Board {
        follow_up: views(KanbanColumn::FollowUp),
        columns: KanbanColumn::ALL
            .into_iter()
            .map(|c| Column {
                id: column_id(c),
                label: c.label(),
                cards: views(c),
            })
            .collect(),
        archived: archived.into_iter().map(card_view).collect(),
        stamp,
    }
}

pub fn load(config_dir: &Path) -> Result<Board, String> {
    let path = path(config_dir);
    let cards = read_board(&path)?;
    Ok(board(&cards, board_stamp(&path)))
}

/// The file's mtime, so the view reloads only after a save.
pub fn stamp(config_dir: &Path) -> u64 {
    board_stamp(&path(config_dir))
}

/// The newest [`LOG_LINES`] lines of a card's log, reading at most its last
/// [`LOG_READ_MAX`] bytes.
pub fn log_tail(config_dir: &Path, id: &str) -> Vec<WorkLogEvent> {
    read_work_log_tail(&work_dir(config_dir), id, LOG_READ_MAX, LOG_LINES)
}

fn find(cards: &[BoardCard], id: &str) -> Result<usize, String> {
    cards.iter().position(|c| c.id == id).ok_or(GONE.into())
}

fn apply(cards: &mut Vec<BoardCard>, id: &str, act: &Act, now: u64) -> Result<(), String> {
    let i = find(cards, id)?;
    let changed = match act {
        Act::Move { column } => {
            let column = parse_column(column).ok_or("That column isn't on the board.")?;
            if cards[i].status.column() == Some(column) {
                false
            } else {
                let rank = in_column(cards, column)
                    .last()
                    .map(|&j| cards[j].rank.saturating_add(1))
                    .unwrap_or(0);
                let c = &mut cards[i];
                c.status = column.status();
                c.rank = rank;
                true
            }
        }
        Act::Rank { up } => match cards[i].status.column() {
            Some(column) => {
                let order = in_column(cards, column);
                let at = order.iter().position(|&j| j == i).unwrap_or(0);
                let other = if *up {
                    at.checked_sub(1)
                } else {
                    Some(at + 1).filter(|&n| n < order.len())
                };
                match other {
                    Some(other) => {
                        // Number the column 1..n in its order, then swap the two.
                        for (n, &j) in order.iter().enumerate() {
                            cards[j].rank = n as i64 + 1;
                        }
                        let (a, b) = (order[at], order[other]);
                        let ra = cards[a].rank;
                        cards[a].rank = cards[b].rank;
                        cards[b].rank = ra;
                        true
                    }
                    None => false,
                }
            }
            None => false,
        },
        Act::HandBack => {
            let c = &mut cards[i];
            if can_resume(c) || c.origin == Some(WorkOrigin::Agent) {
                c.origin = Some(WorkOrigin::User);
                c.status = BoardStatus::Todo;
                true
            } else {
                false
            }
        }
        Act::Archive => grokhub_core::archive_work_card(cards, id, now),
        Act::Restore => grokhub_core::restore_work_card(cards, id, now),
        Act::Delete => grokhub_core::delete_work_card(cards, id),
    };
    if !changed {
        return Err("Nothing changed.".into());
    }
    // A move, a reorder or a hand back is yours, so the sweep leaves the card.
    if matches!(act, Act::Move { .. } | Act::Rank { .. } | Act::HandBack) {
        grokhub_core::mark_touched(cards, id, now);
    }
    Ok(())
}

pub fn act(config_dir: &Path, id: &str, act: Act) -> Result<Board, String> {
    let path = path(config_dir);
    let now = grokhub_core::now_ms();
    let ((), cards) = update_board(&path, |cards| apply(cards, id, &act, now))?;
    if matches!(act, Act::Delete) {
        remove_work_log(&work_dir(config_dir), id);
    }
    Ok(board(&cards, board_stamp(&path)))
}

/// The first message Resume puts in the chat box: the card and where it left
/// off, within [`RESUME_DRAFT_MAX`].
fn resume_draft(card: &BoardCard) -> String {
    let mut text = format!("Resume the work card \"{}\".", clip_line(&card.title, 120));
    if let Some(next) = left_off(card) {
        text.push_str(&format!("\nLeft off: {}", clip_line(&next, 200)));
    }
    if let Some(blocker) = card
        .checkpoint
        .as_ref()
        .map(|c| c.blocker.trim())
        .filter(|b| !b.is_empty())
    {
        text.push_str(&format!("\nBlocked on: {}", clip_line(blocker, 160)));
    }
    cap_bytes(&text, RESUME_DRAFT_MAX)
}

fn cap_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Resume a paused or interrupted run. Today this marks the card touched and
/// drafts the first message in its chat, the way the cabin's stub continues in
/// the card's chat. KB-03 replaces this body with the real resume (the capped
/// resume block sent in the turn input); the command and its reply stay.
pub fn resume(config_dir: &Path, id: &str, new_conversation: &str) -> Result<Resume, String> {
    let path = path(config_dir);
    let now = grokhub_core::now_ms();
    let ((conversation, prompt), cards) = update_board(&path, |cards| {
        let i = find(cards, id)?;
        if !can_resume(&cards[i]) {
            return Err("Only a paused or interrupted run can resume.".into());
        }
        let conversation = cards[i]
            .thread_id
            .clone()
            .unwrap_or_else(|| new_conversation.trim().to_string());
        let prompt = resume_draft(&cards[i]);
        grokhub_core::mark_touched(cards, id, now);
        Ok((conversation, prompt))
    })?;
    Ok(Resume {
        conversation_id: conversation,
        prompt,
        board: board(&cards, board_stamp(&path)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::work_item::append_work_log;
    use grokhub_core::{Checkpoint, WorkStep};

    fn dir(name: &str) -> PathBuf {
        let config = std::env::temp_dir().join(format!(
            "grokhub-shell-workboard-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&config);
        std::fs::create_dir_all(&config).expect("cfg");
        config
    }

    fn card(id: &str, status: BoardStatus) -> BoardCard {
        let mut c = BoardCard::new(id, "", "");
        c.id = id.into();
        c.title = format!("Card {id}");
        c.status = status;
        c
    }

    fn paused(id: &str) -> BoardCard {
        let mut c = card(id, BoardStatus::Blocked);
        c.run = true;
        c.origin = Some(WorkOrigin::Agent);
        c.thread_id = Some(format!("chat-{id}"));
        c.steps = vec![
            WorkStep {
                text: "Read the spec".into(),
                status: StepStatus::Done,
            },
            WorkStep {
                text: "Write the parser".into(),
                status: StepStatus::Doing,
            },
            WorkStep {
                text: "Test it".into(),
                status: StepStatus::Planned,
            },
        ];
        c.checkpoint = Some(Checkpoint {
            goal: "Parse .tsa files".into(),
            next: "Finish the lexer".into(),
            blocker: "Which grammar?".into(),
            ..Default::default()
        });
        c
    }

    /// The cabin's save of a whole board, as `serde_json::to_string_pretty`.
    fn cabin_writes(config: &Path, cards: &[BoardCard]) {
        std::fs::write(
            config.join(FILE),
            serde_json::to_string_pretty(cards).unwrap(),
        )
        .unwrap();
    }

    fn on_disk(config: &Path) -> Vec<BoardCard> {
        read_board(&config.join(FILE)).unwrap()
    }

    #[test]
    fn no_file_is_an_empty_board_with_four_columns() {
        let config = dir("empty");
        let b = load(&config).unwrap();
        let ids: Vec<_> = b.columns.iter().map(|c| c.id).collect();
        assert_eq!(ids, ["todo", "doing", "blocked", "done"]);
        assert!(b.columns.iter().all(|c| c.cards.is_empty()));
        assert!(b.archived.is_empty() && b.follow_up.is_empty());
        assert_eq!(b.stamp, 0);
        assert!(!config.join(FILE).exists(), "loading writes nothing");
    }

    #[test]
    fn a_card_made_by_the_cabin_shows_in_the_shell() {
        let config = dir("cabin-card");
        let mut follow = card("f", BoardStatus::FollowUp);
        follow.automation = Some("daily".into());
        // Saved before owners: loading fills the owner and the kind.
        follow.origin = None;
        let mut gone = card("z", BoardStatus::Dismissed);
        gone.archived_ms = 5;
        cabin_writes(
            &config,
            &[card("a", BoardStatus::Todo), paused("p"), follow, gone],
        );
        let b = load(&config).unwrap();
        assert_eq!(b.columns[0].cards[0].title, "Card a");
        assert_eq!(b.columns[0].cards[0].owner, "You");
        let p = &b.columns[2].cards[0];
        assert_eq!(p.owner, "Agent");
        assert!(p.can_resume);
        assert_eq!(p.left_off.as_deref(), Some("Finish the lexer"));
        assert_eq!(p.progress, [1, 3]);
        assert_eq!(p.steps[1].status, "doing");
        assert_eq!(p.checkpoint.as_ref().unwrap().blocker, "Which grammar?");
        assert_eq!(p.chat.as_deref(), Some("chat-p"));
        assert_eq!(b.follow_up[0].kind, Some("Automation"), "migrated on read");
        assert_eq!(b.archived[0].column, "archived");
        assert!(b.stamp > 0);
    }

    #[test]
    fn a_shell_move_survives_a_cabin_save() {
        let config = dir("merge");
        cabin_writes(
            &config,
            &[card("a", BoardStatus::Todo), card("b", BoardStatus::Todo)],
        );
        // The cabin loads the board, then the shell moves a card.
        let cabin_base = on_disk(&config);
        let b = act(
            &config,
            "a",
            Act::Move {
                column: "doing".into(),
            },
        )
        .unwrap();
        assert_eq!(b.columns[1].cards[0].id, "a");
        let moved = on_disk(&config);
        assert!(moved[0].touched, "a move is yours");
        assert!(moved[0].rev > cabin_base[0].rev);

        // The cabin edits b from its stale copy and saves through the merge.
        let base = grokhub_core::work_item::board_base(&cabin_base);
        let mut mine = cabin_base.clone();
        mine[1].notes = "check the cable".into();
        grokhub_core::work_item::stamp_changes(&mut mine, &base, 1);
        let merged = grokhub_core::work_item::merge_cards(&base, &mine, &moved);
        cabin_writes(&config, &merged);

        let b = load(&config).unwrap();
        assert_eq!(b.columns[1].cards[0].id, "a", "the shell's move kept");
        assert_eq!(
            b.columns[0].cards[0].notes, "check the cable",
            "the cabin's edit kept"
        );
    }

    #[test]
    fn rank_reorders_within_a_column_and_the_ends_refuse() {
        let config = dir("rank");
        cabin_writes(
            &config,
            &[
                card("a", BoardStatus::Todo),
                card("b", BoardStatus::Todo),
                card("c", BoardStatus::Todo),
            ],
        );
        let b = act(&config, "c", Act::Rank { up: true }).unwrap();
        let ids: Vec<_> = b.columns[0].cards.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["a", "c", "b"]);
        assert_eq!(
            act(&config, "a", Act::Rank { up: true }).unwrap_err(),
            "Nothing changed."
        );
        let b = act(&config, "a", Act::Rank { up: false }).unwrap();
        let ids: Vec<_> = b.columns[0].cards.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["c", "a", "b"]);
        // A moved card goes to the end of its new column.
        act(
            &config,
            "b",
            Act::Move {
                column: "done".into(),
            },
        )
        .unwrap();
        let b = act(
            &config,
            "c",
            Act::Move {
                column: "done".into(),
            },
        )
        .unwrap();
        let ids: Vec<_> = b.columns[3].cards.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["b", "c"]);
        assert_eq!(
            act(
                &config,
                "c",
                Act::Move {
                    column: "nope".into()
                }
            )
            .unwrap_err(),
            "That column isn't on the board."
        );
    }

    #[test]
    fn hand_back_archive_restore_and_delete() {
        let config = dir("life");
        cabin_writes(&config, &[paused("p"), card("t", BoardStatus::Todo)]);
        let b = act(&config, "p", Act::HandBack).unwrap();
        let p = &b.columns[0].cards.iter().find(|c| c.id == "p").unwrap();
        assert_eq!((p.owner, p.column), ("You", "todo"));
        assert_eq!(
            act(&config, "t", Act::HandBack).unwrap_err(),
            "Nothing changed."
        );

        let b = act(&config, "t", Act::Archive).unwrap();
        assert_eq!(b.archived[0].id, "t");
        let b = act(&config, "t", Act::Restore).unwrap();
        assert!(b.archived.is_empty());

        append_work_log(&work_dir(&config), "t", 1, "step", "did it").unwrap();
        let b = act(&config, "t", Act::Delete).unwrap();
        assert!(b
            .columns
            .iter()
            .all(|c| c.cards.iter().all(|x| x.id != "t")));
        assert!(log_tail(&config, "t").is_empty(), "its log is gone too");
        assert_eq!(act(&config, "t", Act::Archive).unwrap_err(), GONE);
    }

    #[test]
    fn resume_drafts_the_message_in_the_cards_chat() {
        let config = dir("resume");
        let mut no_chat = paused("q");
        no_chat.thread_id = None;
        cabin_writes(
            &config,
            &[paused("p"), no_chat, card("t", BoardStatus::Todo)],
        );
        let r = resume(&config, "p", "shell-new").unwrap();
        assert_eq!(r.conversation_id, "chat-p");
        assert_eq!(
            r.prompt,
            "Resume the work card \"Card p\".\nLeft off: Finish the lexer\nBlocked on: Which grammar?"
        );
        assert!(on_disk(&config)[0].touched);
        assert_eq!(
            resume(&config, "q", "shell-new").unwrap().conversation_id,
            "shell-new"
        );
        assert_eq!(
            resume(&config, "t", "x").unwrap_err(),
            "Only a paused or interrupted run can resume."
        );
        let mut huge = paused("h");
        huge.title = "t".repeat(5000);
        assert!(resume_draft(&huge).len() <= RESUME_DRAFT_MAX);
    }

    #[test]
    fn the_log_is_read_on_ask_and_only_its_tail() {
        let config = dir("log");
        for i in 0..30 {
            append_work_log(&work_dir(&config), "p", i, "step", &format!("line {i}")).unwrap();
        }
        let lines = log_tail(&config, "p");
        assert_eq!(lines.len(), LOG_LINES);
        assert_eq!(lines.last().unwrap().text, "line 29");
        assert!(log_tail(&config, "../escape").is_empty());
    }
}
