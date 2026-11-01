//! Workboard (shell parity): the cabin's kanban of cards from the same
//! `workboard.json`, with the same `grokhub_core::workboard` types and rules,
//! so a card added or moved in either shows in both. Work puts the card's
//! first message in the main chat box and moves the card to Doing; nothing
//! is sent for you. The in-card chat (Say, Stop) stays in the cabin.
//!
//! Every change reloads the file first under one process lock, so a card the
//! cabin changed since the page loaded is kept. A file that does not parse is
//! an error and is never written over. The cabin keeps its board in memory
//! and writes it whole, so a cabin that is open at the same time can still
//! write over a change made here.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use grokhub_core::{BoardCard, BoardStatus, KanbanColumn};
use serde::{Deserialize, Serialize};

pub const FILE: &str = "workboard.json";
/// The cabin's own cap for its JSON stores.
const STORE_CAP: u64 = 32 * 1024 * 1024;
/// The cabin's caps on a card's title and detail.
const TITLE_MAX: usize = 120;
const DETAIL_MAX: usize = 2000;
pub const GONE: &str = "That card is gone. It may have been removed in the cabin.";

/// One read-change-write at a time in this process; the temp file is this
/// process's own.
static STORE: Mutex<()> = Mutex::new(());

fn store() -> MutexGuard<'static, ()> {
    STORE.lock().unwrap_or_else(|err| err.into_inner())
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
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
    /// Archived cards, off the board.
    pub archived: Vec<Card>,
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
    Move {
        to: Lane,
    },
    Archive,
    Restore,
    /// For good. The UI asks first.
    Delete,
}

/// What Work puts in the chat box, and the board after the card moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub prompt: String,
    pub page: Page,
}

fn card(c: &BoardCard) -> Card {
    Card {
        id: c.id.clone(),
        title: c.title.clone(),
        detail: c.detail.clone(),
        notes: c.notes.clone(),
        priority: c.priority.clone(),
        fresh: c.fresh,
        report: c.report.clone(),
    }
}

fn page(cards: &[BoardCard]) -> Page {
    let columns = Lane::ALL
        .iter()
        .map(|&lane| {
            let col = lane.column();
            let mut here: Vec<&BoardCard> = cards
                .iter()
                .filter(|c| c.status.column() == Some(col))
                .collect();
            if lane == Lane::FollowUp {
                here.sort_by_key(|c| std::cmp::Reverse(c.updated_ms));
            }
            Column {
                lane,
                label: col.label().to_string(),
                cards: here.into_iter().map(card).collect(),
            }
        })
        .collect();
    Page {
        columns,
        archived: cards
            .iter()
            .filter(|c| c.status == BoardStatus::Dismissed)
            .map(card)
            .collect(),
    }
}

fn load_unlocked(config_dir: &Path) -> Result<Vec<BoardCard>, String> {
    let path = path(config_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len > STORE_CAP {
        return Err(format!("{FILE} is too large to be a workboard."));
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(raw).map_err(|e| format!("{FILE}: {e}"))
}

/// Writes through a temp file and a rename, like the cabin. Callers hold
/// [`STORE`].
fn save(config_dir: &Path, cards: &[BoardCard]) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(cards).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.shell-{}.tmp", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

fn clip(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

pub fn load(config_dir: &Path) -> Result<Page, String> {
    let _held = store();
    Ok(page(&load_unlocked(config_dir)?))
}

/// Adds a card to Todo, as the cabin's New card does.
pub fn add(config_dir: &Path, title: &str, detail: &str) -> Result<Page, String> {
    if title.trim().is_empty() {
        return Err("Type a title first.".into());
    }
    let _held = store();
    let mut cards = load_unlocked(config_dir)?;
    let mut new = BoardCard::new(title, detail, "");
    new.status = BoardStatus::Todo;
    cards.push(new);
    save(config_dir, &cards)?;
    Ok(page(&cards))
}

pub fn act(config_dir: &Path, id: &str, act: Act) -> Result<Page, String> {
    if let Act::Edit { title, .. } = &act {
        if title.trim().is_empty() {
            return Err("A card needs a title.".into());
        }
    }
    let _held = store();
    let mut cards = load_unlocked(config_dir)?;
    let at = cards.iter().position(|c| c.id == id).ok_or(GONE)?;
    let changed = match act {
        Act::Edit { title, detail } => {
            let c = &mut cards[at];
            c.title = clip(&title, TITLE_MAX);
            c.detail = clip(&detail, DETAIL_MAX);
            true
        }
        Act::Notes { notes } => {
            cards[at].notes = grokhub_core::clean_card_notes(&notes);
            true
        }
        Act::Move { to } => {
            let c = &mut cards[at];
            // Dropping a card on the column it is in changes nothing.
            if c.status.column() == Some(to.column()) {
                false
            } else {
                c.status = to.column().status();
                true
            }
        }
        Act::Archive => {
            cards[at].status = BoardStatus::Dismissed;
            true
        }
        Act::Restore => {
            cards[at].status = BoardStatus::Todo;
            true
        }
        Act::Delete => {
            cards.remove(at);
            true
        }
    };
    if changed {
        save(config_dir, &cards)?;
    }
    Ok(page(&cards))
}

/// The card's first message for the main chat, with its notes. As the
/// cabin's Work on it does, the card moves to Doing unless it is done or an
/// automation's Follow up, and stops being new. The chat is the shell's own,
/// so the card is not linked to a cabin chat.
pub fn work(config_dir: &Path, id: &str) -> Result<Work, String> {
    let _held = store();
    let mut cards = load_unlocked(config_dir)?;
    let c = cards.iter_mut().find(|c| c.id == id).ok_or(GONE)?;
    let prompt = grokhub_core::card_work_prompt(c);
    let before = (c.status, c.fresh);
    if c.status != BoardStatus::Done && c.automation.is_none() {
        c.status = BoardStatus::InProgress;
    }
    c.fresh = false;
    if (c.status, c.fresh) != before {
        save(config_dir, &cards)?;
    }
    Ok(Work {
        prompt,
        page: page(&cards),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn titles(page: &Page, lane: Lane) -> Vec<String> {
        page.columns
            .iter()
            .find(|c| c.lane == lane)
            .expect("lane")
            .cards
            .iter()
            .map(|c| c.title.clone())
            .collect()
    }

    fn raw(dir: &Path) -> Vec<BoardCard> {
        serde_json::from_str(&std::fs::read_to_string(path(dir)).expect("read")).expect("parse")
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
        // Loading writes nothing.
        assert_eq!(std::fs::read_to_string(path(&dir)).expect("kept"), CABIN);
    }

    #[test]
    fn a_missing_file_is_an_empty_board_and_add_files_a_todo() {
        let dir = temp_dir("add");
        let empty = load(&dir).expect("empty");
        assert!(empty.columns.iter().all(|c| c.cards.is_empty()));
        assert!(!path(&dir).exists());

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
        assert!(!dir
            .join(format!("{FILE}.shell-{}.tmp", std::process::id()))
            .exists());
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
    fn a_broken_file_is_an_error_and_is_left_alone() {
        let dir = temp_dir("broken");
        std::fs::write(path(&dir), "[{not json").expect("seed");
        assert!(load(&dir)
            .expect_err("broken")
            .starts_with("workboard.json: "));
        assert!(add(&dir, "New", "").is_err());
        assert!(act(&dir, "w-1", Act::Archive).is_err());
        assert!(work(&dir, "w-1").is_err());
        assert_eq!(
            std::fs::read_to_string(path(&dir)).expect("kept"),
            "[{not json"
        );
    }

    #[test]
    fn delete_removes_that_card_only() {
        let dir = seed("delete");
        let page = act(&dir, "w-3", Act::Delete).expect("delete");
        assert_eq!(titles(&page, Lane::Blocked), Vec::<String>::new());
        assert_eq!(
            raw(&dir).iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["w-1", "w-2", "w-4", "w-5"]
        );
        assert_eq!(act(&dir, "w-3", Act::Delete), Err(GONE.to_string()));
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
    fn acts_arrive_as_the_ui_sends_them() {
        let parsed: Act = serde_json::from_str(r#"{"kind":"move","to":"followUp"}"#).expect("move");
        assert!(matches!(parsed, Act::Move { to: Lane::FollowUp }));
        let parsed: Act =
            serde_json::from_str(r#"{"kind":"edit","title":"A","detail":"B"}"#).expect("edit");
        assert!(matches!(parsed, Act::Edit { title, detail } if title == "A" && detail == "B"));
        let parsed: Act = serde_json::from_str(r#"{"kind":"delete"}"#).expect("delete");
        assert!(matches!(parsed, Act::Delete));
    }
}
