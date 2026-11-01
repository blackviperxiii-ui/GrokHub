//! Workboard (shell parity): the cabin's kanban of cards from the same
//! `workboard.json`, with the same `grokhub_core::workboard` types and rules,
//! so a card added or moved in either shows in both. Work puts the card's
//! first message in the main chat box and moves the card to Doing; nothing
//! is sent for you. The in-card chat (Say, Stop) stays in the cabin.
//!
//! Every change goes through `grokhub_core::work_file`: the one board lock
//! that the cabin, the shell and the agent's `work_*` tools all take, a
//! re-read under it, and the per-card merge (the higher `rev` wins), so no
//! writer loses another's change. A file that does not parse is an error and
//! is never written over. A change that changes nothing writes nothing.
//!
//! The page shows each card's steps, where it left off and who owns it. A
//! card's progress log is read only when the card is opened, and only its
//! tail. Resume only drafts a short first message that you send yourself.
//!
//! Token budget (KB-04): all of this is local file work. Nothing here reaches
//! a prompt or the tool list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use grokhub_core::scheduler_lease::{lease_fresh, read_lease, LeaseOwner};
use grokhub_core::work_file::{
    board_stamp, delete_archived_board, read_board, read_tidy, read_work_log_tail,
    set_auto_delete_days, sweep_board, update_board, TidyRun,
};
use grokhub_core::work_item::{
    clip_line, remove_work_log, DAY_MS, IDLE_ARCHIVE_MS, SWEEP_EVERY_MS,
};
use grokhub_core::work_triage::{self as wt, end_rank, in_column};
use grokhub_core::{
    BoardCard, BoardStatus, KanbanColumn, StepStatus, WorkKind, WorkLogEvent, WorkOrigin,
};
use serde::{Deserialize, Serialize};

pub const FILE: &str = "workboard.json";
/// The cabin's caps on a card's title and detail.
const TITLE_MAX: usize = 120;
const DETAIL_MAX: usize = 2000;
/// Lines the log shows, newest last (the cabin's drawer shows as many).
pub const LOG_LINES: usize = 20;
/// Most of a progress log the page reads: its tail, never the whole file.
pub const LOG_READ_MAX: u64 = 32 * 1024;
/// Longest Resume draft. KB-04's `RESUME_BLOCK_MAX` (1,200 B) caps the real
/// resume block KB-03 sends; the draft stays inside the same cap.
pub const RESUME_DRAFT_MAX: usize = 1200;
pub const GONE: &str = "That card is gone. It may have been removed in the cabin.";

/// The page's line while cards wait in the queue and the cabin is closed.
pub const QUEUE_WAITS_FOR_CABIN: &str =
    "Queued cards start in the cabin once it is open and nothing else is running.";

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

fn work_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("work")
}

/// A column on the page: the Follow up row, then the four kanban columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lane {
    FollowUp,
    Todo,
    Doing,
    Blocked,
    Done,
}

impl Lane {
    /// In the order the page shows them.
    pub const ALL: [Lane; 5] = [
        Lane::FollowUp,
        Lane::Todo,
        Lane::Doing,
        Lane::Blocked,
        Lane::Done,
    ];

    fn column(self) -> KanbanColumn {
        match self {
            Self::FollowUp => KanbanColumn::FollowUp,
            Self::Todo => KanbanColumn::Todo,
            Self::Doing => KanbanColumn::Doing,
            Self::Blocked => KanbanColumn::Blocked,
            Self::Done => KanbanColumn::Done,
        }
    }
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
    /// Your notes for the agent; Work hands them over.
    pub notes: String,
    pub priority: String,
    /// A new report you have not opened yet.
    pub fresh: bool,
    /// A Follow up card's latest report from its automation.
    pub report: String,
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
    /// The cabin chat that works it.
    pub chat: Option<String>,
    pub updated_ms: u64,
    /// When it was archived; 0 for a card on the board.
    pub archived_ms: u64,
    /// "Queued", "Queued · 2nd": waiting for the agent. None when not queued.
    pub queued: Option<String>,
    /// The playbook, recipe or automation started on it: "Playbook · Linux repair".
    pub flow: Option<String>,
    /// You edited the steps, so the agent works from your version.
    pub user_steps: bool,
    /// The steps as the editor shows them, one a line with its mark.
    pub steps_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub lane: Lane,
    pub label: String,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// Follow up first (newest run first), then Todo, Doing, Blocked, Done.
    pub columns: Vec<Column>,
    /// Archived cards, off the board, newest first.
    pub archived: Vec<Card>,
    /// The file's mtime, for the view's change check.
    pub stamp: u64,
    /// The shell's last sweep archived these cards and you have not undone or
    /// dismissed it. Only cards still archived count.
    pub swept: Option<Swept>,
    /// Cards wait in the queue and the cabin, which starts them, is closed.
    pub queue_note: Option<String>,
}

/// One sweep's notice: "Archived 2 finished cards", with Undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Swept {
    pub text: String,
    pub ids: Vec<String>,
    pub at: u64,
}

/// Settings → Workboard tidy, as both apps show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tidy {
    /// Days with no activity before the sweep archives a card. Fixed.
    pub auto_archive_days: u32,
    /// 0 is off (the default); else 30 or 90.
    pub auto_delete_days: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Act {
    Edit {
        title: String,
        detail: String,
    },
    Notes {
        notes: String,
    },
    /// To the end of a column.
    Move {
        to: Lane,
    },
    /// One place up or down in its column, through `rank` like the cabin.
    Rank {
        up: bool,
    },
    /// A paused run becomes yours: back to Todo, and the sweep leaves it.
    HandBack,
    Archive,
    Restore,
    /// For good, with its progress log. The UI asks first.
    Delete,
    /// Agent: do this → Queue. The cabin starts it once nothing else runs.
    Queue,
    Unqueue,
    /// "high", "normal" or "low".
    Priority {
        priority: String,
    },
    /// To Blocked, with the reason written into the checkpoint.
    Block {
        reason: String,
    },
    Unblock,
    /// Your edit of the steps, one a line.
    Steps {
        text: String,
    },
    /// Into `to`, just above `before` (a card there), or at its end.
    Place {
        to: Lane,
        before: Option<String>,
    },
}

/// What Work puts in the chat box, and the board after the card moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub prompt: String,
    pub page: Page,
}

/// What Resume puts in the chat box, and the chat it goes to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resume {
    /// The chat to open: the card's own, else the new one passed in.
    pub conversation_id: String,
    /// The first message, put in the chat box for you to send.
    pub prompt: String,
    pub page: Page,
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

fn card(c: &BoardCard, all: &[BoardCard]) -> Card {
    Card {
        id: c.id.clone(),
        title: c.title.clone(),
        detail: c.detail.clone(),
        notes: c.notes.clone(),
        priority: c.priority.clone(),
        fresh: c.fresh,
        report: c.report.clone(),
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
        archived_ms: c.archived_ms,
        queued: wt::queue_place(all, &c.id).map(wt::queue_label),
        flow: c.flow.as_ref().map(|f| f.line()),
        user_steps: c.user_steps,
        steps_text: wt::steps_text(c),
    }
}

/// The shell's sweep notices, one per config folder, until Undo or Dismiss.
fn swept_notes() -> &'static Mutex<HashMap<PathBuf, Swept>> {
    static NOTES: std::sync::OnceLock<Mutex<HashMap<PathBuf, Swept>>> = std::sync::OnceLock::new();
    NOTES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The open notice for `config_dir`, trimmed to cards still archived; none
/// once all of them are back on the board or gone.
fn swept_for(config_dir: &Path, cards: &[BoardCard]) -> Option<Swept> {
    let notes = swept_notes().lock().unwrap_or_else(|e| e.into_inner());
    let note = notes.get(config_dir)?;
    let ids: Vec<String> = note
        .ids
        .iter()
        .filter(|id| {
            cards
                .iter()
                .any(|c| &c.id == *id && c.status == BoardStatus::Dismissed)
        })
        .cloned()
        .collect();
    (!ids.is_empty()).then(|| Swept {
        ids,
        ..note.clone()
    })
}

/// The notice line for a sweep, as the cabin's bell words it.
pub fn sweep_text(report: &grokhub_core::SweepReport) -> Option<String> {
    let done = report.archived_done.len();
    let idle = report.archived_idle.len();
    let plural = |n: usize| if n == 1 { "card" } else { "cards" };
    match (done, idle) {
        (0, 0) => None,
        (d, 0) => Some(format!("Archived {d} finished {}", plural(d))),
        (0, i) => Some(format!("Archived {i} {} idle for 30 days", plural(i))),
        (d, i) => Some(format!(
            "Archived {} {}: {d} finished, {i} idle for 30 days",
            d + i,
            plural(d + i)
        )),
    }
}

/// True while the cabin is open: its scheduler beat is fresh. The cabin runs
/// the sweep then, so the shell leaves it.
fn cabin_open(config_dir: &Path, now: u64) -> bool {
    read_lease(config_dir).is_some_and(|l| l.owner == LeaseOwner::Cabin && lease_fresh(&l, now))
}

/// One look of the shell's sweep timer. While the cabin is open it does
/// nothing. Otherwise it runs the shared tidy: at once when `first` (the
/// shell just started), else only when no app swept in six hours. What it
/// archived becomes the page's notice with Undo. Local only; no model.
pub fn sweep_tick(config_dir: &Path, now: u64, first: bool) -> Result<Option<TidyRun>, String> {
    if cabin_open(config_dir, now) {
        return Ok(None);
    }
    let every = if first { 0 } else { SWEEP_EVERY_MS };
    // The shell runs no turns that hold a card open for 30 days; a live run
    // updates its card, so nothing live is idle long enough to be archived.
    let run = sweep_board(&path(config_dir), &work_dir(config_dir), now, &[], every)?;
    if let Some(r) = run.as_ref().map(|r| &r.report) {
        if let Some(text) = sweep_text(r) {
            let ids = r
                .archived_done
                .iter()
                .chain(&r.archived_idle)
                .cloned()
                .collect();
            swept_notes()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(config_dir.to_path_buf(), Swept { text, ids, at: now });
        }
    }
    Ok(run)
}

/// How often the shell's sweep timer looks. The sweep itself runs only when
/// six hours passed since either app swept.
const SWEEP_LOOK: Duration = Duration::from_secs(10 * 60);

/// The shell's sweep timer: once at start, then a look every ten minutes.
pub fn start_sweeper(config_dir: PathBuf) {
    std::thread::spawn(move || {
        let mut first = true;
        loop {
            if sweep_tick(&config_dir, grokhub_core::now_ms(), first).is_ok() {
                first = false;
            }
            std::thread::sleep(SWEEP_LOOK);
        }
    });
}

/// Undo on the sweep notice: the cards it archived go back to Todo.
pub fn undo_sweep(config_dir: &Path) -> Result<Page, String> {
    let note = swept_notes()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(config_dir);
    let now = grokhub_core::now_ms();
    let ((), page) = change(config_dir, |cards| {
        for id in note.iter().flat_map(|n| &n.ids) {
            grokhub_core::restore_work_card(cards, id, now);
        }
        Ok(())
    })?;
    Ok(page)
}

/// Dismiss on the sweep notice: the cards stay archived.
pub fn dismiss_sweep(config_dir: &Path) -> Result<Page, String> {
    swept_notes()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(config_dir);
    load(config_dir)
}

/// The bulk delete: every archived card archived at least `days` ago (0: all
/// archived cards), with their logs. Cards on the board are never touched.
/// The UI asks first with the count.
pub fn delete_archived(config_dir: &Path, days: u32) -> Result<Page, String> {
    if days > 3650 {
        return Err("Pick all, 30 days or 90 days.".into());
    }
    let path = path(config_dir);
    let (_, cards) = delete_archived_board(
        &path,
        &work_dir(config_dir),
        u64::from(days) * DAY_MS,
        grokhub_core::now_ms(),
    )?;
    Ok(page(config_dir, &cards, board_stamp(&path)))
}

pub fn tidy(config_dir: &Path) -> Tidy {
    Tidy {
        auto_archive_days: (IDLE_ARCHIVE_MS / DAY_MS) as u32,
        auto_delete_days: read_tidy(&path(config_dir)).auto_delete_days,
    }
}

/// Settings: auto-delete archived cards after 30 or 90 days, or Off (0).
pub fn set_auto_delete(config_dir: &Path, days: u32) -> Result<Tidy, String> {
    set_auto_delete_days(&path(config_dir), days)?;
    Ok(tidy(config_dir))
}

fn page(config_dir: &Path, cards: &[BoardCard], stamp: u64) -> Page {
    let columns = Lane::ALL
        .iter()
        .map(|&lane| {
            let col = lane.column();
            let mut here = in_column(cards, col);
            if lane == Lane::FollowUp {
                here.sort_by_key(|&i| std::cmp::Reverse(cards[i].updated_ms));
            }
            Column {
                lane,
                label: col.label().to_string(),
                cards: here.into_iter().map(|i| card(&cards[i], cards)).collect(),
            }
        })
        .collect();
    let mut archived: Vec<&BoardCard> = cards
        .iter()
        .filter(|c| c.status == BoardStatus::Dismissed)
        .collect();
    archived.sort_by_key(|c| std::cmp::Reverse(c.archived_ms));
    Page {
        columns,
        archived: archived.into_iter().map(|c| card(c, cards)).collect(),
        stamp,
        swept: swept_for(config_dir, cards),
        queue_note: (cards.iter().any(wt::is_queued)
            && !cabin_open(config_dir, grokhub_core::now_ms()))
        .then(|| QUEUE_WAITS_FOR_CABIN.to_string()),
    }
}

fn clip(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

pub fn load(config_dir: &Path) -> Result<Page, String> {
    let path = path(config_dir);
    let cards = read_board(&path)?;
    Ok(page(config_dir, &cards, board_stamp(&path)))
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

/// One read-change-write under the board lock; the page as it is on disk after.
fn change<R>(
    config_dir: &Path,
    edit: impl FnOnce(&mut Vec<BoardCard>) -> Result<R, String>,
) -> Result<(R, Page), String> {
    let path = path(config_dir);
    let (out, cards) = update_board(&path, edit)?;
    Ok((out, page(config_dir, &cards, board_stamp(&path))))
}

/// Adds a card to the end of Todo, as the cabin's New card does.
pub fn add(config_dir: &Path, title: &str, detail: &str) -> Result<Page, String> {
    if title.trim().is_empty() {
        return Err("Type a title first.".into());
    }
    let ((), page) = change(config_dir, |cards| {
        let mut new = BoardCard::new(&clip(title, TITLE_MAX), &clip(detail, DETAIL_MAX), "");
        new.status = BoardStatus::Todo;
        new.rank = end_rank(cards, KanbanColumn::Todo);
        cards.push(new);
        Ok(())
    })?;
    Ok(page)
}

fn find(cards: &[BoardCard], id: &str) -> Result<usize, String> {
    cards.iter().position(|c| c.id == id).ok_or(GONE.into())
}

/// Applies `act`; true when it changed the card. A no-op (a drop on the
/// card's own column, a rank past the end) changes nothing and writes nothing.
fn apply(cards: &mut Vec<BoardCard>, id: &str, act: &Act, now: u64) -> Result<bool, String> {
    let i = find(cards, id)?;
    let changed = match act {
        Act::Edit { title, detail } => {
            let (title, detail) = (clip(title, TITLE_MAX), clip(detail, DETAIL_MAX));
            let c = &mut cards[i];
            let changed = c.title != title || c.detail != detail;
            c.title = title;
            c.detail = detail;
            changed
        }
        Act::Notes { notes } => {
            let notes = grokhub_core::clean_card_notes(notes);
            let changed = cards[i].notes != notes;
            cards[i].notes = notes;
            changed
        }
        Act::Move { to } => {
            let column = to.column();
            cards[i].status.column() != Some(column) && wt::place_card(cards, id, column, None, now)
        }
        Act::Place { to, before } => wt::place_card(cards, id, to.column(), before.as_deref(), now),
        Act::Queue => wt::queue_card(cards, id, now)?,
        Act::Unqueue => wt::unqueue_card(cards, id),
        Act::Priority { priority } => wt::set_priority(cards, id, priority)?,
        Act::Block { reason } => wt::block_card(cards, id, reason, now)?,
        Act::Unblock => wt::unblock_card(cards, id, now),
        Act::Steps { text } => wt::edit_steps(&mut cards[i], text, now),
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
    // A change you made is yours, so the sweep leaves the card.
    let yours = matches!(
        act,
        Act::Edit { .. }
            | Act::Notes { .. }
            | Act::Move { .. }
            | Act::Rank { .. }
            | Act::HandBack
            | Act::Place { .. }
            | Act::Priority { .. }
            | Act::Block { .. }
            | Act::Unblock
            | Act::Steps { .. }
    );
    if changed && yours {
        grokhub_core::mark_touched(cards, id, now);
    }
    Ok(changed)
}

pub fn act(config_dir: &Path, id: &str, act: Act) -> Result<Page, String> {
    if let Act::Edit { title, .. } = &act {
        if title.trim().is_empty() {
            return Err("A card needs a title.".into());
        }
    }
    let now = grokhub_core::now_ms();
    let (changed, page) = change(config_dir, |cards| apply(cards, id, &act, now))?;
    if changed && matches!(act, Act::Delete) {
        remove_work_log(&work_dir(config_dir), id);
    }
    Ok(page)
}

/// The card's first message for the main chat, with its notes. As the
/// cabin's Work on it does, the card moves to the end of Doing unless it is
/// done or an automation's Follow up, and stops being new. The chat is the
/// shell's own, so the card is not linked to a cabin chat.
pub fn work(config_dir: &Path, id: &str) -> Result<Work, String> {
    let now = grokhub_core::now_ms();
    let (prompt, page) = change(config_dir, |cards| {
        let i = find(cards, id)?;
        // Agent: do this → Now. A card in Done or Blocked still opens its chat.
        if wt::assign_now(cards, id).is_ok() {
            grokhub_core::mark_touched(cards, id, now);
        }
        let prompt = grokhub_core::card_work_prompt(&cards[i]);
        let to_doing = cards[i].status != BoardStatus::Done
            && cards[i].automation.is_none()
            && cards[i].status.column() != Some(KanbanColumn::Doing);
        let rank = end_rank(cards, KanbanColumn::Doing);
        let c = &mut cards[i];
        if to_doing {
            c.status = BoardStatus::InProgress;
            c.rank = rank;
        }
        c.fresh = false;
        Ok(prompt)
    })?;
    Ok(Work { prompt, page })
}

/// Start a playbook on the card: the card links it, then it starts as Work
/// (Now) does. The agent picks its playbook from the work itself, so the
/// first message is the card's own and nothing more.
pub fn start_playbook(
    config_dir: &Path,
    id: &str,
    playbook: &str,
    name: &str,
) -> Result<Work, String> {
    if playbook.trim().is_empty() {
        return Err("Pick a playbook.".into());
    }
    let flow = wt::CardFlow::new(wt::FlowKind::Playbook, playbook, name);
    change(config_dir, |cards| {
        find(cards, id)?;
        wt::assign_now(cards, id)?;
        wt::link_flow(cards, id, flow);
        Ok(())
    })?;
    work(config_dir, id)
}

/// Make it recurring: a new automation from the card's title on one of
/// [`wt::RECUR_CHOICES`], which then owns the card. Its runs report here.
pub fn make_recurring(
    config_dir: &Path,
    id: &str,
    choice: usize,
    clock: grokhub_core::LocalClock,
) -> Result<Page, String> {
    let cards = read_board(&path(config_dir))?;
    let c = cards.iter().find(|c| c.id == id).ok_or(GONE)?;
    if c.automation.is_some() {
        return Err("An automation already owns this card.".into());
    }
    let seed = wt::recurring_seed(choice, c).ok_or("Pick a schedule.")?;
    let list = crate::automations::add(config_dir, &seed, clock)?;
    let added = list.last().ok_or("The automation was not saved.")?;
    let (auto_id, name) = (added.id.clone(), added.name.clone());
    let owned = change(config_dir, |cards| {
        find(cards, id)?;
        wt::own_by_automation(cards, id, &auto_id, &name);
        Ok(())
    });
    if owned.is_err() {
        // The card went meanwhile: no automation left behind for it.
        let _ = crate::automations::remove(config_dir, &auto_id);
    }
    Ok(owned?.1)
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
    let now = grokhub_core::now_ms();
    let ((conversation_id, prompt), page) = change(config_dir, |cards| {
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
        conversation_id,
        prompt,
        page,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::work_item::append_work_log;
    use grokhub_core::{Checkpoint, WorkStep};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-board-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// As the cabin writes it: a run card linked to its chat, a proposed card,
    /// a blocked one, a Follow up from an automation and an archived one.
    const CABIN: &str = r#"[
  {"id":"w-1","title":"Fix the build","detail":"CI is red","status":"in_progress","priority":"high","threadId":"t-1","run":true,"notes":"Start with the lint job","notesSent":42},
  {"id":"w-2","title":"Write release notes","detail":"","status":"proposed","priority":""},
  {"id":"w-3","title":"Wait on review","detail":"","status":"blocked","priority":""},
  {"id":"w-4","title":"Inbox digest","detail":"Two new mails","status":"follow_up","priority":"","automation":"auto-1","report":"Two new mails need a reply.","fresh":true,"updatedMs":1800000000000},
  {"id":"w-5","title":"Old idea","detail":"","status":"dismissed","priority":""}
]"#;

    fn seed(name: &str) -> PathBuf {
        let dir = temp_dir(name);
        std::fs::write(path(&dir), CABIN).expect("seed");
        dir
    }

    fn lane(page: &Page, lane: Lane) -> &[Card] {
        &page
            .columns
            .iter()
            .find(|c| c.lane == lane)
            .expect("lane")
            .cards
    }

    fn titles(page: &Page, l: Lane) -> Vec<String> {
        lane(page, l).iter().map(|c| c.title.clone()).collect()
    }

    fn ids(page: &Page, l: Lane) -> Vec<String> {
        lane(page, l).iter().map(|c| c.id.clone()).collect()
    }

    fn raw(dir: &Path) -> Vec<BoardCard> {
        serde_json::from_str(&std::fs::read_to_string(path(dir)).expect("read")).expect("parse")
    }

    fn board_card(id: &str, status: BoardStatus) -> BoardCard {
        let mut c = BoardCard::new(id, "", "");
        c.id = id.into();
        c.title = format!("Card {id}");
        c.status = status;
        c
    }

    fn paused(id: &str) -> BoardCard {
        let mut c = board_card(id, BoardStatus::Blocked);
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
        std::fs::write(path(config), serde_json::to_string_pretty(cards).unwrap()).unwrap();
    }

    fn on_disk(config: &Path) -> Vec<BoardCard> {
        read_board(&path(config)).unwrap()
    }

    #[test]
    fn a_board_the_cabin_wrote_loads_into_its_columns() {
        let dir = seed("cabin");
        let page = load(&dir).expect("load");
        assert_eq!(
            page.columns
                .iter()
                .map(|c| (c.lane, c.label.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (Lane::FollowUp, "Follow up"),
                (Lane::Todo, "Todo"),
                (Lane::Doing, "Doing"),
                (Lane::Blocked, "Blocked"),
                (Lane::Done, "Done"),
            ]
        );
        assert_eq!(titles(&page, Lane::FollowUp), vec!["Inbox digest"]);
        assert_eq!(titles(&page, Lane::Todo), vec!["Write release notes"]);
        assert_eq!(titles(&page, Lane::Doing), vec!["Fix the build"]);
        assert_eq!(titles(&page, Lane::Blocked), vec!["Wait on review"]);
        assert_eq!(titles(&page, Lane::Done), Vec::<String>::new());
        assert_eq!(
            page.archived
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["w-5"]
        );
        let digest = &page.columns[0].cards[0];
        assert_eq!(
            (digest.report.as_str(), digest.fresh),
            ("Two new mails need a reply.", true)
        );
        assert_eq!(page.columns[2].cards[0].notes, "Start with the lint job");
        assert!(page.stamp > 0);
        // Loading writes nothing.
        assert_eq!(std::fs::read_to_string(path(&dir)).expect("kept"), CABIN);
    }

    #[test]
    fn a_missing_file_is_an_empty_board_and_add_files_a_todo() {
        let dir = temp_dir("add");
        let empty = load(&dir).expect("empty");
        assert!(empty.columns.iter().all(|c| c.cards.is_empty()));
        assert!(empty.archived.is_empty());
        assert_eq!(empty.stamp, 0);
        assert!(!path(&dir).exists(), "loading writes nothing");

        assert_eq!(
            add(&dir, "   ", "x"),
            Err("Type a title first.".to_string())
        );
        assert!(!path(&dir).exists());

        let page = add(&dir, "  Ship the board  ", " in the shell ").expect("add");
        assert_eq!(titles(&page, Lane::Todo), vec!["Ship the board"]);
        let saved = raw(&dir);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].status, BoardStatus::Todo);
        assert_eq!(saved[0].detail, "in the shell");
        assert!(saved[0].id.starts_with('w'), "{}", saved[0].id);
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp") || n.ends_with(".lock"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        let page = add(&dir, "Second", "").expect("add");
        assert_eq!(
            titles(&page, Lane::Todo),
            vec!["Ship the board", "Second"],
            "a new card goes to the end of Todo"
        );
    }

    #[test]
    fn a_card_made_by_the_cabin_shows_its_run_in_the_shell() {
        let config = temp_dir("cabin-card");
        let mut follow = board_card("f", BoardStatus::FollowUp);
        follow.automation = Some("daily".into());
        // Saved before owners: loading fills the owner and the kind.
        follow.origin = None;
        let mut gone = board_card("z", BoardStatus::Dismissed);
        gone.archived_ms = 5;
        cabin_writes(
            &config,
            &[
                board_card("a", BoardStatus::Todo),
                paused("p"),
                follow,
                gone,
            ],
        );
        let b = load(&config).unwrap();
        assert_eq!(lane(&b, Lane::Todo)[0].title, "Card a");
        assert_eq!(lane(&b, Lane::Todo)[0].owner, "You");
        let p = &lane(&b, Lane::Blocked)[0];
        assert_eq!(p.owner, "Agent");
        assert!(p.can_resume);
        assert_eq!(p.left_off.as_deref(), Some("Finish the lexer"));
        assert_eq!(p.progress, [1, 3]);
        assert_eq!(p.steps[1].status, "doing");
        assert_eq!(p.checkpoint.as_ref().unwrap().blocker, "Which grammar?");
        assert_eq!(p.chat.as_deref(), Some("chat-p"));
        assert_eq!(
            lane(&b, Lane::FollowUp)[0].kind,
            Some("Automation"),
            "migrated on read"
        );
        assert_eq!(b.archived[0].id, "z");
        assert!(b.stamp > 0);
    }

    #[test]
    fn move_changes_one_card_and_keeps_the_others() {
        let dir = seed("move");
        let page = act(&dir, "w-2", Act::Move { to: Lane::Doing }).expect("move");
        assert_eq!(
            titles(&page, Lane::Doing),
            vec!["Fix the build", "Write release notes"]
        );
        assert_eq!(titles(&page, Lane::Todo), Vec::<String>::new());
        let saved = raw(&dir);
        assert_eq!(
            saved
                .iter()
                .map(|c| (c.id.as_str(), c.status))
                .collect::<Vec<_>>(),
            vec![
                ("w-1", BoardStatus::InProgress),
                ("w-2", BoardStatus::InProgress),
                ("w-3", BoardStatus::Blocked),
                ("w-4", BoardStatus::FollowUp),
                ("w-5", BoardStatus::Dismissed),
            ]
        );
        // The cabin's own fields ride along untouched.
        assert_eq!(saved[0].thread_id.as_deref(), Some("t-1"));
        assert!(saved[0].run);
        assert_eq!(saved[0].notes_sent, 42);
        assert_eq!(saved[3].automation.as_deref(), Some("auto-1"));
        assert_eq!(saved[3].updated_ms, 1_800_000_000_000);
        // The move is yours, and wins the cabin's merge.
        assert!(saved[1].touched);
        assert!(saved[1].rev > saved[2].rev);
    }

    #[test]
    fn a_drop_on_its_own_column_writes_nothing() {
        let dir = seed("same");
        // Proposed already sits in Todo.
        let page = act(&dir, "w-2", Act::Move { to: Lane::Todo }).expect("noop");
        assert_eq!(titles(&page, Lane::Todo), vec!["Write release notes"]);
        assert_eq!(std::fs::read_to_string(path(&dir)).expect("kept"), CABIN);
    }

    #[test]
    fn an_edit_after_the_cabin_changed_the_file_keeps_the_cabin_change() {
        let dir = seed("reload");
        let first = load(&dir).expect("load");
        assert_eq!(titles(&first, Lane::Blocked), vec!["Wait on review"]);
        // The cabin adds a card and finishes the blocked one while the page is open.
        let mut cabin = raw(&dir);
        cabin[2].status = BoardStatus::Done;
        let mut added = BoardCard::new("Added in the cabin", "", "");
        added.status = BoardStatus::Todo;
        cabin.push(added);
        std::fs::write(path(&dir), serde_json::to_string(&cabin).expect("json")).expect("cabin");

        let page = act(
            &dir,
            "w-2",
            Act::Notes {
                notes: "  Keep it short  ".into(),
            },
        )
        .expect("notes");
        assert_eq!(
            titles(&page, Lane::Todo),
            vec!["Write release notes", "Added in the cabin"]
        );
        assert_eq!(titles(&page, Lane::Done), vec!["Wait on review"]);
        let saved = raw(&dir);
        assert_eq!(saved.len(), 6);
        assert_eq!(saved[1].notes, "Keep it short");
        assert_eq!(saved[2].status, BoardStatus::Done);
    }

    #[test]
    fn a_shell_move_survives_a_cabin_save() {
        let config = temp_dir("merge");
        cabin_writes(
            &config,
            &[
                board_card("a", BoardStatus::Todo),
                board_card("b", BoardStatus::Todo),
            ],
        );
        // The cabin loads the board, then the shell moves a card.
        let cabin_base = on_disk(&config);
        let b = act(&config, "a", Act::Move { to: Lane::Doing }).unwrap();
        assert_eq!(ids(&b, Lane::Doing), ["a"]);
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
        assert_eq!(ids(&b, Lane::Doing), ["a"], "the shell's move kept");
        assert_eq!(
            lane(&b, Lane::Todo)[0].notes,
            "check the cable",
            "the cabin's edit kept"
        );
    }

    #[test]
    fn a_broken_file_is_an_error_and_is_left_alone() {
        let dir = temp_dir("broken");
        std::fs::write(path(&dir), "[{not json").expect("seed");
        assert!(load(&dir)
            .expect_err("broken")
            .starts_with("workboard.json: "));
        assert!(add(&dir, "New", "").is_err());
        assert!(act(&dir, "w-1", Act::Archive).is_err());
        assert!(work(&dir, "w-1").is_err());
        assert!(resume(&dir, "w-1", "x").is_err());
        assert_eq!(
            std::fs::read_to_string(path(&dir)).expect("kept"),
            "[{not json"
        );
    }

    #[test]
    fn delete_removes_that_card_only_with_its_log() {
        let dir = seed("delete");
        append_work_log(&work_dir(&dir), "w-3", 1, "step", "did it").unwrap();
        let page = act(&dir, "w-3", Act::Delete).expect("delete");
        assert_eq!(titles(&page, Lane::Blocked), Vec::<String>::new());
        assert_eq!(
            raw(&dir).iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["w-1", "w-2", "w-4", "w-5"]
        );
        assert!(log_tail(&dir, "w-3").is_empty(), "its log is gone too");
        assert_eq!(act(&dir, "w-3", Act::Delete), Err(GONE.to_string()));
    }

    #[test]
    fn the_shell_sweep_archives_without_a_model_and_undo_restores() {
        let dir = temp_dir("sweep");
        let now = grokhub_core::now_ms();
        let mut agent = board_card("agent", BoardStatus::Done);
        agent.origin = Some(WorkOrigin::Agent);
        agent.created_ms = now - 2 * DAY_MS;
        let mut mine = board_card("mine", BoardStatus::Done);
        mine.origin = Some(WorkOrigin::User);
        mine.created_ms = now - 2 * DAY_MS;
        let mut touched = board_card("touched", BoardStatus::Done);
        touched.origin = Some(WorkOrigin::Agent);
        touched.created_ms = now - 2 * DAY_MS;
        touched.touched = true;
        cabin_writes(&dir, &[agent, mine, touched]);

        let run = sweep_tick(&dir, now, true)
            .expect("tick")
            .expect("at start it runs");
        assert_eq!(run.report.archived_done, vec!["agent".to_string()]);
        let page = load(&dir).expect("load");
        let archived: Vec<&str> = page.archived.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            archived,
            vec!["agent"],
            "your cards and touched ones stay on the board"
        );
        assert_eq!(page.archived[0].archived_ms, now);
        let swept = page.swept.expect("one notice for the sweep");
        assert_eq!(swept.text, "Archived 1 finished card");
        assert_eq!(swept.ids, vec!["agent".to_string()]);

        // Ten minutes on, the timer looks again: not due for six hours.
        assert_eq!(sweep_tick(&dir, now + 600_000, false).expect("tick"), None);

        let page = undo_sweep(&dir).expect("undo");
        assert_eq!(page.swept, None);
        assert!(page.archived.is_empty());
        assert_eq!(
            ids(&page, Lane::Todo),
            vec!["agent"],
            "Undo puts it back in Todo"
        );
        assert_eq!(
            raw(&dir).iter().find(|c| c.id == "agent").unwrap().status,
            BoardStatus::Todo
        );
    }

    #[test]
    fn the_shell_leaves_the_sweep_to_an_open_cabin() {
        use grokhub_core::scheduler_lease::{write_lease, SchedulerLease};
        let dir = temp_dir("sweep-cabin");
        let now = grokhub_core::now_ms();
        let mut agent = board_card("agent", BoardStatus::Done);
        agent.origin = Some(WorkOrigin::Agent);
        agent.created_ms = now - 2 * DAY_MS;
        cabin_writes(&dir, &[agent]);
        let beat = |beat_ms| SchedulerLease {
            owner: LeaseOwner::Cabin,
            pid: 1,
            beat_ms,
        };
        write_lease(&dir, &beat(now)).unwrap();
        assert_eq!(sweep_tick(&dir, now, true).expect("tick"), None);
        assert_eq!(
            raw(&dir)[0].status,
            BoardStatus::Done,
            "the cabin's to sweep"
        );
        assert_eq!(load(&dir).unwrap().swept, None);

        // The cabin closed: its beat goes stale and the shell sweeps.
        write_lease(&dir, &beat(now - 10 * 60_000)).unwrap();
        assert!(sweep_tick(&dir, now, true).expect("tick").is_some());
        assert_eq!(raw(&dir)[0].status, BoardStatus::Dismissed);
        let page = dismiss_sweep(&dir).expect("dismiss");
        assert_eq!(page.swept, None);
        assert_eq!(page.archived.len(), 1, "Dismiss leaves the card archived");
    }

    #[test]
    fn bulk_delete_takes_archived_cards_and_logs_and_never_the_board() {
        let dir = temp_dir("bulk");
        let now = grokhub_core::now_ms();
        let archived = |id: &str, ago: u64| {
            let mut c = board_card(id, BoardStatus::Dismissed);
            c.archived_ms = now - ago;
            c
        };
        cabin_writes(
            &dir,
            &[
                archived("old", 95 * DAY_MS),
                archived("mid", 45 * DAY_MS),
                archived("new", DAY_MS),
                board_card("todo", BoardStatus::Todo),
                board_card("done", BoardStatus::Done),
            ],
        );
        for id in ["old", "mid", "new", "todo"] {
            append_work_log(&work_dir(&dir), id, 1, "step", "x").unwrap();
        }
        let page = delete_archived(&dir, 90).expect("older than 90");
        assert_eq!(page.archived.len(), 2);
        let page = delete_archived(&dir, 30).expect("older than 30");
        let left: Vec<&str> = page.archived.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(left, vec!["new"]);
        let page = delete_archived(&dir, 0).expect("all archived");
        assert!(page.archived.is_empty());
        assert_eq!(
            raw(&dir).iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["todo", "done"],
            "the board is never touched"
        );
        assert!(log_tail(&dir, "old").is_empty() && log_tail(&dir, "new").is_empty());
        assert_eq!(log_tail(&dir, "todo").len(), 1);
        assert!(delete_archived(&dir, 99_999).is_err());
    }

    #[test]
    fn the_tidy_setting_defaults_to_off_and_takes_30_or_90() {
        let dir = temp_dir("tidy");
        assert_eq!(
            tidy(&dir),
            Tidy {
                auto_archive_days: 30,
                auto_delete_days: 0
            }
        );
        assert_eq!(set_auto_delete(&dir, 90).expect("90").auto_delete_days, 90);
        assert_eq!(tidy(&dir).auto_delete_days, 90);
        assert!(set_auto_delete(&dir, 12).is_err());
        assert_eq!(set_auto_delete(&dir, 0).expect("off").auto_delete_days, 0);
    }

    #[test]
    fn archive_restore_and_edit() {
        let dir = seed("archive");
        let page = act(&dir, "w-3", Act::Archive).expect("archive");
        assert_eq!(
            page.archived
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["w-3", "w-5"]
        );
        let page = act(&dir, "w-5", Act::Restore).expect("restore");
        assert_eq!(
            titles(&page, Lane::Todo),
            vec!["Write release notes", "Old idea"]
        );
        assert_eq!(
            act(
                &dir,
                "w-5",
                Act::Edit {
                    title: " ".into(),
                    detail: "x".into()
                }
            ),
            Err("A card needs a title.".to_string())
        );
        let page = act(
            &dir,
            "w-5",
            Act::Edit {
                title: " New idea ".into(),
                detail: " Try it ".into(),
            },
        )
        .expect("edit");
        assert_eq!(
            titles(&page, Lane::Todo),
            vec!["Write release notes", "New idea"]
        );
        assert_eq!(raw(&dir)[4].detail, "Try it");
    }

    #[test]
    fn rank_reorders_within_a_column_and_the_ends_change_nothing() {
        let config = temp_dir("rank");
        cabin_writes(
            &config,
            &[
                board_card("a", BoardStatus::Todo),
                board_card("b", BoardStatus::Todo),
                board_card("c", BoardStatus::Todo),
            ],
        );
        let b = act(&config, "c", Act::Rank { up: true }).unwrap();
        assert_eq!(ids(&b, Lane::Todo), ["a", "c", "b"]);
        let before = std::fs::read_to_string(path(&config)).unwrap();
        let b = act(&config, "a", Act::Rank { up: true }).unwrap();
        assert_eq!(ids(&b, Lane::Todo), ["a", "c", "b"]);
        assert_eq!(
            std::fs::read_to_string(path(&config)).unwrap(),
            before,
            "the top card going up writes nothing"
        );
        let b = act(&config, "a", Act::Rank { up: false }).unwrap();
        assert_eq!(ids(&b, Lane::Todo), ["c", "a", "b"]);
        // A moved card goes to the end of its new column.
        act(&config, "b", Act::Move { to: Lane::Done }).unwrap();
        let b = act(&config, "c", Act::Move { to: Lane::Done }).unwrap();
        assert_eq!(ids(&b, Lane::Done), ["b", "c"]);
    }

    #[test]
    fn hand_back_makes_a_paused_run_yours() {
        let config = temp_dir("hand-back");
        cabin_writes(&config, &[paused("p"), board_card("t", BoardStatus::Todo)]);
        let b = act(&config, "p", Act::HandBack).unwrap();
        let p = lane(&b, Lane::Todo).iter().find(|c| c.id == "p").unwrap();
        assert_eq!(p.owner, "You");
        assert!(on_disk(&config)[0].touched);
        // Your own card has nothing to hand back.
        let before = std::fs::read_to_string(path(&config)).unwrap();
        act(&config, "t", Act::HandBack).unwrap();
        assert_eq!(std::fs::read_to_string(path(&config)).unwrap(), before);
    }

    #[test]
    fn work_returns_the_prompt_and_moves_the_card_to_doing() {
        let dir = seed("work");
        act(
            &dir,
            "w-2",
            Act::Notes {
                notes: "Use the changelog fragments".into(),
            },
        )
        .expect("notes");
        let started = work(&dir, "w-2").expect("work");
        assert_eq!(
            started.prompt,
            "Work on this workboard card: Write release notes\n\nMy notes for this card (follow them):\nUse the changelog fragments\n\nWhen it is done, say so in one line."
        );
        assert_eq!(
            titles(&started.page, Lane::Doing),
            vec!["Fix the build", "Write release notes"]
        );
        let saved = raw(&dir);
        assert_eq!(saved[1].status, BoardStatus::InProgress);
        // The shell's chat is not a cabin chat, so the card stays unlinked.
        assert_eq!(saved[1].thread_id, None);
        assert_eq!(saved[1].notes_sent, 0);

        // A Follow up card is opened (no longer new) and stays in Follow up.
        let digest = work(&dir, "w-4").expect("digest");
        assert_eq!(
            digest.prompt,
            "Work on this workboard card: Inbox digest\n\nWhen it is done, say so in one line."
        );
        assert_eq!(titles(&digest.page, Lane::FollowUp), vec!["Inbox digest"]);
        assert!(!raw(&dir)[3].fresh);
        assert_eq!(raw(&dir)[3].status, BoardStatus::FollowUp);

        assert_eq!(work(&dir, "w-9"), Err(GONE.to_string()));
    }

    #[test]
    fn work_on_a_done_card_leaves_it_done() {
        let dir = seed("done");
        act(&dir, "w-3", Act::Move { to: Lane::Done }).expect("done");
        let before = std::fs::read_to_string(path(&dir)).expect("before");
        let started = work(&dir, "w-3").expect("work");
        assert_eq!(titles(&started.page, Lane::Done), vec!["Wait on review"]);
        assert_eq!(std::fs::read_to_string(path(&dir)).expect("after"), before);
    }

    #[test]
    fn resume_drafts_the_message_in_the_cards_chat() {
        let config = temp_dir("resume");
        let mut no_chat = paused("q");
        no_chat.thread_id = None;
        cabin_writes(
            &config,
            &[paused("p"), no_chat, board_card("t", BoardStatus::Todo)],
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
        let config = temp_dir("log");
        for i in 0..30 {
            append_work_log(&work_dir(&config), "p", i, "step", &format!("line {i}")).unwrap();
        }
        let lines = log_tail(&config, "p");
        assert_eq!(lines.len(), LOG_LINES);
        assert_eq!(lines.last().unwrap().text, "line 29");
        assert!(log_tail(&config, "../escape").is_empty());
    }

    #[test]
    fn acts_arrive_as_the_ui_sends_them() {
        let parsed: Act = serde_json::from_str(r#"{"kind":"move","to":"followUp"}"#).expect("move");
        assert!(matches!(parsed, Act::Move { to: Lane::FollowUp }));
        let parsed: Act =
            serde_json::from_str(r#"{"kind":"edit","title":"A","detail":"B"}"#).expect("edit");
        assert!(matches!(parsed, Act::Edit { title, detail } if title == "A" && detail == "B"));
        let parsed: Act = serde_json::from_str(r#"{"kind":"delete"}"#).expect("delete");
        assert!(matches!(parsed, Act::Delete));
        let parsed: Act = serde_json::from_str(r#"{"kind":"rank","up":true}"#).expect("rank");
        assert!(matches!(parsed, Act::Rank { up: true }));
        let parsed: Act = serde_json::from_str(r#"{"kind":"handBack"}"#).expect("hand back");
        assert!(matches!(parsed, Act::HandBack));
        let parsed: Act =
            serde_json::from_str(r#"{"kind":"place","to":"todo","before":"w-1"}"#).expect("place");
        assert!(matches!(parsed, Act::Place { to: Lane::Todo, before: Some(b) } if b == "w-1"));
        let parsed: Act = serde_json::from_str(r#"{"kind":"place","to":"done","before":null}"#)
            .expect("place at the end");
        assert!(matches!(
            parsed,
            Act::Place {
                to: Lane::Done,
                before: None
            }
        ));
        let parsed: Act = serde_json::from_str(r#"{"kind":"queue"}"#).expect("queue");
        assert!(matches!(parsed, Act::Queue));
        let parsed: Act =
            serde_json::from_str(r#"{"kind":"block","reason":"Review"}"#).expect("block");
        assert!(matches!(parsed, Act::Block { reason } if reason == "Review"));
        let parsed: Act =
            serde_json::from_str(r#"{"kind":"priority","priority":"high"}"#).expect("priority");
        assert!(matches!(parsed, Act::Priority { priority } if priority == "high"));
        let parsed: Act = serde_json::from_str(r#"{"kind":"steps","text":"A\nB"}"#).expect("steps");
        assert!(matches!(parsed, Act::Steps { text } if text == "A\nB"));
    }

    #[test]
    fn a_reorder_and_a_block_land_where_the_cabin_reads_them() {
        let config = temp_dir("triage");
        cabin_writes(
            &config,
            &[
                board_card("a", BoardStatus::Todo),
                board_card("b", BoardStatus::Todo),
                board_card("c", BoardStatus::Todo),
            ],
        );
        // Drop c on a: c lands above a, in the ranks the cabin sorts by.
        let to_a = Act::Place {
            to: Lane::Todo,
            before: Some("a".into()),
        };
        let page = act(&config, "c", to_a).unwrap();
        assert_eq!(ids(&page, Lane::Todo), ["c", "a", "b"]);
        let disk = on_disk(&config);
        let cabin_order: Vec<&str> = in_column(&disk, KanbanColumn::Todo)
            .iter()
            .map(|&i| disk[i].id.as_str())
            .collect();
        assert_eq!(cabin_order, ["c", "a", "b"]);
        assert!(disk[2].touched, "a reorder is yours");

        // An order the cabin saved shows the same here.
        let mut cabin = on_disk(&config);
        assert!(wt::place_card(
            &mut cabin,
            "b",
            KanbanColumn::Todo,
            Some("c"),
            1
        ));
        cabin_writes(&config, &cabin);
        assert_eq!(ids(&load(&config).unwrap(), Lane::Todo), ["b", "c", "a"]);

        // Block with a reason: it lands in the checkpoint a resume reads.
        assert!(act(
            &config,
            "a",
            Act::Block {
                reason: "  ".into()
            }
        )
        .is_err());
        let block = Act::Block {
            reason: "Waiting on review".into(),
        };
        let page = act(&config, "a", block).unwrap();
        let a = &lane(&page, Lane::Blocked)[0];
        assert_eq!(a.checkpoint.as_ref().unwrap().blocker, "Waiting on review");
        let saved = on_disk(&config);
        let a = saved.iter().find(|c| c.id == "a").unwrap();
        assert_eq!(a.checkpoint.as_ref().unwrap().blocker, "Waiting on review");
        let page = act(&config, "a", Act::Unblock).unwrap();
        assert_eq!(
            ids(&page, Lane::Todo),
            ["b", "c", "a"],
            "back at the end of Todo"
        );
        assert_eq!(
            lane(&page, Lane::Todo)[2]
                .checkpoint
                .as_ref()
                .unwrap()
                .blocker,
            ""
        );

        let high = Act::Priority {
            priority: "high".into(),
        };
        let page = act(&config, "b", high).unwrap();
        assert_eq!(lane(&page, Lane::Todo)[0].priority, "high");

        let steps = Act::Steps {
            text: "[x] Read the spec\nWrite the parser".into(),
        };
        let page = act(&config, "b", steps).unwrap();
        let b = &lane(&page, Lane::Todo)[0];
        assert!(b.user_steps);
        assert_eq!(b.steps_text, "[x] Read the spec\n[ ] Write the parser");
        assert_eq!(b.checkpoint.as_ref().unwrap().next, "Write the parser");
    }

    #[test]
    fn a_card_queued_here_waits_for_the_cabin() {
        use grokhub_core::scheduler_lease::{write_lease, SchedulerLease};
        let config = temp_dir("queue");
        let mut agent = board_card("a", BoardStatus::Todo);
        agent.origin = Some(WorkOrigin::Agent);
        cabin_writes(&config, &[agent, board_card("b", BoardStatus::Done)]);
        let page = act(&config, "a", Act::Queue).unwrap();
        let a = &lane(&page, Lane::Todo)[0];
        assert_eq!(a.queued.as_deref(), Some("Queued"));
        assert_eq!(a.owner, "You", "a card you hand over is yours");
        assert_eq!(page.queue_note.as_deref(), Some(QUEUE_WAITS_FOR_CABIN));
        assert!(
            act(&config, "b", Act::Queue).is_err(),
            "a done card is not queued"
        );

        // The cabin is open: it starts queued cards, so no note.
        let now = grokhub_core::now_ms();
        let lease = SchedulerLease {
            owner: LeaseOwner::Cabin,
            pid: 1,
            beat_ms: now,
        };
        write_lease(&config, &lease).unwrap();
        assert_eq!(load(&config).unwrap().queue_note, None);

        let page = act(&config, "a", Act::Unqueue).unwrap();
        assert_eq!(lane(&page, Lane::Todo)[0].queued, None);
    }

    #[test]
    fn make_it_recurring_files_an_automation_that_owns_the_card() {
        let config = temp_dir("recur");
        cabin_writes(&config, &[board_card("a", BoardStatus::Todo)]);
        let clock = crate::automations::Clock {
            weekday: 1,
            hour: 8,
            minute: 0,
        }
        .at(grokhub_core::now_ms());
        assert!(make_recurring(&config, "a", 9, clock).is_err());
        let page = make_recurring(&config, "a", 0, clock).unwrap();
        let list = crate::automations::load(&config).unwrap();
        assert_eq!(list.len(), 1);
        let card = &lane(&page, Lane::Todo)[0];
        assert_eq!(card.kind, Some("Automation"));
        assert!(card.flow.as_deref().unwrap().starts_with("Automation · "));
        let saved = on_disk(&config);
        assert_eq!(saved[0].automation.as_deref(), Some(list[0].id.as_str()));
        assert!(
            make_recurring(&config, "a", 1, clock).is_err(),
            "one automation owns a card"
        );
        assert_eq!(crate::automations::load(&config).unwrap().len(), 1);
    }
}
