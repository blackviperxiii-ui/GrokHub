//! KB-02: agent work logs itself on the board. A run (a task turn, an
//! automation, a background run, a desktop episode) opens or reuses one card
//! and checkpoints it at each step: the plan from `todo_write`, one short log
//! line per tool, the files it changed, and where it stopped. Chats file no
//! card; the run hooks only follow a card that a work ask filed.
//!
//! Everything here is a plain local call. No model reads or writes any of it,
//! and nothing here goes into a prompt (KB-04).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::work_item::{
    archive_card, board_base, clean_card, clean_steps, clip_line, migrate_cards, open_private,
    stamp_changes, Checkpoint, StepStatus, WorkKind, WorkOrigin, WorkStep, CHECKPOINT_BLOCKER_MAX,
    CHECKPOINT_NEXT_MAX, CHECKPOINT_REFS_MAX, CHECKPOINT_REF_MAX,
};
use crate::workboard::{follow_up_title, BoardCard, BoardStatus};

/// A `todo_write` status as a step status.
pub fn step_status(status: &str) -> StepStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "completed" | "complete" | "done" => StepStatus::Done,
        "in_progress" | "doing" | "active" => StepStatus::Doing,
        "cancelled" | "canceled" | "skipped" => StepStatus::Skipped,
        "failed" => StepStatus::Failed,
        _ => StepStatus::Planned,
    }
}

/// The first step not done or skipped, as the checkpoint's `next`.
pub fn next_step(steps: &[WorkStep]) -> String {
    steps
        .iter()
        .find(|s| !matches!(s.status, StepStatus::Done | StepStatus::Skipped))
        .map(|s| clip_line(&s.text, CHECKPOINT_NEXT_MAX))
        .unwrap_or_default()
}

/// The card's checkpoint, made when it has none. The goal starts as the title,
/// so a checkpoint is never empty and survives `clean_card`.
fn checkpoint(card: &mut BoardCard) -> &mut Checkpoint {
    let title = card.title.clone();
    let cp = card.checkpoint.get_or_insert_with(Checkpoint::default);
    if cp.goal.trim().is_empty() {
        cp.goal = title;
    }
    cp
}

/// A run started on `card`: its kind, session and model, one more attempt, and
/// no blocker. A task run keeps a kind the card already has (an automation's
/// follow-up chat stays an automation card).
pub fn begin_run(card: &mut BoardCard, kind: WorkKind, session: &str, model: &str, now: u64) {
    if kind != WorkKind::Task || card.kind == WorkKind::Task {
        card.kind = kind;
    }
    let cp = checkpoint(card);
    if !session.trim().is_empty() {
        cp.session = session.trim().to_string();
    }
    if !model.trim().is_empty() {
        cp.model = model.trim().to_string();
    }
    cp.attempt = cp.attempt.saturating_add(1);
    cp.blocker.clear();
    cp.at_ms = now;
    clean_card(card);
}

/// Mirror the run's `todo_write` list (text, status) into the card's steps, and
/// point `next` at the first open one. True when the card changed.
pub fn mirror_todos(card: &mut BoardCard, todos: &[(String, String)], now: u64) -> bool {
    let steps = clean_steps(
        todos
            .iter()
            .map(|(text, status)| WorkStep {
                text: text.clone(),
                status: step_status(status),
            })
            .collect(),
    );
    let next = next_step(&steps);
    let same_next = card.checkpoint.as_ref().is_some_and(|c| c.next == next);
    if steps == card.steps && same_next {
        return false;
    }
    card.steps = steps;
    let cp = checkpoint(card);
    cp.next = next;
    cp.at_ms = now;
    clean_card(card);
    true
}

/// Files the run changed, newest first, ahead of the refs already there.
/// At most [`CHECKPOINT_REFS_MAX`]. True when the card changed.
pub fn note_refs(card: &mut BoardCard, files: &[String], now: u64) -> bool {
    let fresh: Vec<String> = files
        .iter()
        .map(|f| clip_tail(f, CHECKPOINT_REF_MAX))
        .filter(|f| !f.is_empty())
        .collect();
    if fresh.is_empty() {
        return false;
    }
    let cp = checkpoint(card);
    let mut refs: Vec<String> = Vec::new();
    for r in fresh.iter().rev().chain(cp.refs.iter()) {
        if !refs.contains(r) {
            refs.push(r.clone());
        }
    }
    refs.truncate(CHECKPOINT_REFS_MAX);
    if refs == cp.refs {
        return false;
    }
    cp.refs = refs;
    cp.at_ms = now;
    true
}

/// A path cut to its last `max` bytes, so a long one keeps its file name.
fn clip_tail(path: &str, max: usize) -> String {
    let p = clip_line(path, usize::MAX);
    if p.len() <= max {
        return p;
    }
    let mut start = p.len() - (max - 3);
    while !p.is_char_boundary(start) {
        start += 1;
    }
    format!("...{}", &p[start..])
}

/// One progress log line for a finished tool call: its name and ok or fail.
pub fn tool_log_text(name: &str, ok: bool) -> String {
    let name = clip_line(name, 100);
    let name = if name.is_empty() { "tool".into() } else { name };
    format!("{name} {}", if ok { "ok" } else { "fail" })
}

/// Log kinds the run hooks write.
pub const LOG_START: &str = "start";
pub const LOG_TOOL: &str = "tool";
pub const LOG_STEP: &str = "step";
pub const LOG_END: &str = "end";
pub const LOG_NOTE: &str = "note";

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    Done,
    /// You stopped it. The card waits in Blocked, paused where it was.
    Stopped,
    Failed(String),
}

/// The blocker a stopped run leaves.
pub const STOPPED_BLOCKER: &str = "Stopped before it finished.";

impl From<&crate::bg_task::BgEnd> for RunEnd {
    fn from(end: &crate::bg_task::BgEnd) -> Self {
        match end {
            crate::bg_task::BgEnd::Done => Self::Done,
            crate::bg_task::BgEnd::Stopped => Self::Stopped,
            crate::bg_task::BgEnd::Failed(why) => Self::Failed(why.clone()),
        }
    }
}

impl RunEnd {
    /// The progress log line for this end.
    pub fn log_text(&self) -> String {
        match self {
            Self::Done => "done".into(),
            Self::Stopped => "stopped".into(),
            Self::Failed(why) => format!("failed: {why}"),
        }
    }
}

/// The final checkpoint. Done clears the blocker; Stopped and Failed say why,
/// and `next` keeps the step to resume from.
pub fn end_run(card: &mut BoardCard, end: &RunEnd, now: u64) {
    let next = next_step(&card.steps);
    let has_steps = !card.steps.is_empty();
    let cp = checkpoint(card);
    cp.at_ms = now;
    if has_steps {
        cp.next = next;
    }
    match end {
        RunEnd::Done => cp.blocker.clear(),
        RunEnd::Stopped => cp.blocker = STOPPED_BLOCKER.into(),
        RunEnd::Failed(why) => {
            cp.blocker = clip_line(&format!("Failed: {why}"), CHECKPOINT_BLOCKER_MAX)
        }
    }
    clean_card(card);
}

/// The card a chat thread's run hooks filed (newest first), unless archived.
pub fn run_card_index(cards: &[BoardCard], thread_id: &str) -> Option<usize> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return None;
    }
    cards.iter().rposition(|c| {
        c.run && c.thread_id.as_deref() == Some(thread_id) && c.status != BoardStatus::Dismissed
    })
}

fn ours(c: &BoardCard) -> bool {
    c.origin == Some(WorkOrigin::Agent) && !c.touched
}

/// One card per automation, reused on every run and moved to Doing. An open
/// card wins; else the agent's own Done or archived card comes back. A card you
/// finished or archived stays put and the run files a new one. Returns its id.
pub fn begin_automation_card(
    cards: &mut Vec<BoardCard>,
    automation_id: &str,
    name: &str,
    session: &str,
    model: &str,
    now: u64,
) -> String {
    let mine = |c: &BoardCard| c.automation.as_deref() == Some(automation_id);
    let pick = cards
        .iter()
        .rposition(|c| mine(c) && !matches!(c.status, BoardStatus::Done | BoardStatus::Dismissed))
        .or_else(|| cards.iter().rposition(|c| mine(c) && ours(c)));
    let idx = match pick {
        Some(i) => i,
        None => {
            let mut card = BoardCard::new(&follow_up_title(name), "", "").by_agent();
            card.automation = Some(automation_id.to_string());
            cards.push(card);
            cards.len() - 1
        }
    };
    let c = &mut cards[idx];
    c.archived_ms = 0;
    c.status = BoardStatus::InProgress;
    c.updated_ms = now;
    begin_run(c, WorkKind::Automation, session, model, now);
    c.id.clone()
}

/// The automation's run ended. With a report to read (`follow_up`), Done leaves
/// the card for `file_follow_up`, which moves it to Follow up. A Done run with
/// nothing new goes back to Follow up when an unread report waits, else to Done,
/// and the agent's own untouched card is archived right away. Stopped and Failed
/// wait in Blocked. Returns the card id.
pub fn finish_automation_card(
    cards: &mut [BoardCard],
    automation_id: &str,
    end: &RunEnd,
    follow_up: bool,
    now: u64,
) -> Option<String> {
    let c = cards.iter_mut().rev().find(|c| {
        c.automation.as_deref() == Some(automation_id) && c.status == BoardStatus::InProgress
    })?;
    end_run(c, end, now);
    c.updated_ms = now;
    match end {
        RunEnd::Done if follow_up => {}
        RunEnd::Done if c.fresh && !c.report.trim().is_empty() => {
            c.status = BoardStatus::FollowUp;
        }
        RunEnd::Done => {
            c.status = BoardStatus::Done;
            if ours(c) {
                archive_card(c, now);
            }
        }
        RunEnd::Stopped | RunEnd::Failed(_) => c.status = BoardStatus::Blocked,
    }
    Some(c.id.clone())
}

/// A new card in Doing for one background run or desktop session. Returns its id.
pub fn open_run_card(
    cards: &mut Vec<BoardCard>,
    kind: WorkKind,
    title: &str,
    session: &str,
    model: &str,
    now: u64,
) -> String {
    let mut card = BoardCard::new(title, "", "").by_agent();
    card.status = BoardStatus::InProgress;
    card.updated_ms = now;
    begin_run(&mut card, kind, session, model, now);
    let id = card.id.clone();
    cards.push(card);
    id
}

/// Settle the card `id`: Done, or Blocked for a stop or a failure, with its
/// final checkpoint. True when the card is there.
pub fn finish_run_card(cards: &mut [BoardCard], id: &str, end: &RunEnd, now: u64) -> bool {
    let Some(c) = cards.iter_mut().find(|c| c.id == id) else {
        return false;
    };
    end_run(c, end, now);
    c.updated_ms = now;
    if c.status != BoardStatus::Dismissed {
        c.status = match end {
            RunEnd::Done => BoardStatus::Done,
            RunEnd::Stopped | RunEnd::Failed(_) => BoardStatus::Blocked,
        };
    }
    true
}

/// `workboard.json` in the config folder.
pub fn board_path(config_dir: &Path) -> PathBuf {
    config_dir.join("workboard.json")
}

/// `{config_dir}/work`, one `<card id>.jsonl` progress log per card.
pub fn work_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("work")
}

/// A lock older than this was left by a writer that died; it is broken.
const LOCK_STALE: Duration = Duration::from_secs(5);
/// Give up on the lock after this long.
const LOCK_WAIT: Duration = Duration::from_secs(7);
/// Bigger than this is not a board file.
const BOARD_READ_CAP: u64 = 32 * 1024 * 1024;

/// Held while a writer re-reads, changes and writes the board. Removed on drop.
pub struct BoardLock(PathBuf);

impl Drop for BoardLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    path.with_file_name(name)
}

/// Take `<file>.lock` with an exclusive create. A lock older than five seconds
/// is stale and broken. Every writer of the board (the cabin, the shell, the
/// agent's `work_*` tools) takes this same lock.
pub fn lock_board(path: &Path) -> Result<BoardLock, String> {
    let lock = lock_path(path);
    if let Some(dir) = lock.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let start = Instant::now();
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(mut f) => {
                let _ = write!(f, "{}", std::process::id());
                return Ok(BoardLock(lock));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = fs::metadata(&lock)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age >= LOCK_STALE);
                if stale {
                    let _ = fs::remove_file(&lock);
                    continue;
                }
                if start.elapsed() >= LOCK_WAIT {
                    return Err("workboard.json is locked by another writer".into());
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The cards on disk. A missing or empty file is an empty board; one too big or
/// not a board is an error, so a writer never replaces cards it failed to read.
pub fn read_board_file(path: &Path) -> Result<Vec<BoardCard>, String> {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    if meta.len() > BOARD_READ_CAP {
        return Err("workboard.json is too large to be a board".into());
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut cards: Vec<BoardCard> =
        serde_json::from_str(raw).map_err(|e| format!("workboard.json: {e}"))?;
    migrate_cards(&mut cards, crate::now_ms());
    Ok(cards)
}

fn write_board_file(path: &Path, cards: &[BoardCard]) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| "the board needs a folder".to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let body = serde_json::to_string_pretty(cards).map_err(|e| e.to_string())?;
    static NEXT_TMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT_TMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".workboard.json.{}-{n}.tmp", std::process::id()));
    let mut f = open_private(&tmp, false).map_err(|e| e.to_string())?;
    f.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    drop(f);
    let mut renamed = fs::rename(&tmp, path);
    for _ in 0..5 {
        if renamed.is_ok() || !cfg!(windows) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        renamed = fs::rename(&tmp, path);
    }
    renamed.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

/// One change to the board file by a writer that holds no copy of its own (the
/// shell, the agent's `work_*` tools): take the lock, read, `edit`, bump `rev`
/// on each card that changed so it wins the cabin's merge, and write when
/// anything changed. Returns what `edit` returned.
pub fn edit_board_file<R>(
    path: &Path,
    now: u64,
    edit: impl FnOnce(&mut Vec<BoardCard>) -> R,
) -> Result<R, String> {
    let _lock = lock_board(path)?;
    let mut cards = read_board_file(path)?;
    let before = serde_json::to_string(&cards).map_err(|e| e.to_string())?;
    let base = board_base(&cards);
    let out = edit(&mut cards);
    stamp_changes(&mut cards, &base, now);
    let after = serde_json::to_string(&cards).map_err(|e| e.to_string())?;
    if after != before {
        write_board_file(path, &cards)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todos(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter()
            .map(|(t, s)| (t.to_string(), s.to_string()))
            .collect()
    }

    fn run_card(title: &str) -> BoardCard {
        let mut c = BoardCard::new(title, "", "").by_agent();
        c.status = BoardStatus::InProgress;
        c.run = true;
        c.thread_id = Some("thr-1".into());
        c
    }

    #[test]
    fn a_scripted_todo_write_run_gives_matching_steps_and_checkpoint() {
        let mut c = run_card("Flash the pi");
        begin_run(&mut c, WorkKind::Task, "native-s1", "grok-4.7", 100);
        let cp = c.checkpoint.clone().unwrap();
        assert_eq!(cp.goal, "Flash the pi");
        assert_eq!(cp.session, "native-s1");
        assert_eq!(cp.model, "grok-4.7");
        assert_eq!(cp.attempt, 1);
        assert_eq!(cp.at_ms, 100);

        let plan = todos(&[
            ("Download the image", "completed"),
            ("Write the SD card", "in_progress"),
            ("Verify boot", "pending"),
        ]);
        assert!(mirror_todos(&mut c, &plan, 200));
        assert!(!mirror_todos(&mut c, &plan, 300), "same plan, no change");
        let statuses: Vec<StepStatus> = c.steps.iter().map(|s| s.status).collect();
        assert_eq!(
            statuses,
            [StepStatus::Done, StepStatus::Doing, StepStatus::Planned]
        );
        assert_eq!(c.steps[2].text, "Verify boot");
        let cp = c.checkpoint.clone().unwrap();
        assert_eq!(cp.next, "Write the SD card");
        assert_eq!(cp.at_ms, 200);

        assert!(note_refs(
            &mut c,
            &["src/a.rs".into(), "src/b.rs".into()],
            250
        ));
        assert!(note_refs(
            &mut c,
            &["src/c.rs".into(), "src/d.rs".into(), "src/a.rs".into()],
            260
        ));
        assert_eq!(
            c.checkpoint.as_ref().unwrap().refs,
            ["src/a.rs", "src/d.rs", "src/c.rs", "src/b.rs"]
        );
        let deep = format!("/home/me/{}/main.rs", "nested/".repeat(20));
        assert!(note_refs(&mut c, &[deep], 270));
        let first = &c.checkpoint.as_ref().unwrap().refs[0];
        assert_eq!(first.len(), CHECKPOINT_REF_MAX);
        assert!(
            first.starts_with("...") && first.ends_with("/main.rs"),
            "{first}"
        );

        let done = todos(&[
            ("Download the image", "completed"),
            ("Write the SD card", "completed"),
            ("Verify boot", "cancelled"),
        ]);
        assert!(mirror_todos(&mut c, &done, 400));
        end_run(&mut c, &RunEnd::Done, 500);
        let cp = c.checkpoint.clone().unwrap();
        assert_eq!(cp.next, "");
        assert_eq!(cp.blocker, "");
        assert_eq!(cp.at_ms, 500);
    }

    #[test]
    fn caps_hold_on_oversized_todos_and_log_lines() {
        let mut c = run_card("Big plan");
        let long = "x".repeat(500);
        let plan: Vec<(String, String)> = (0..30)
            .map(|i| (format!("{i} {long}"), "pending".to_string()))
            .collect();
        assert!(mirror_todos(&mut c, &plan, 1));
        assert_eq!(c.steps.len(), crate::work_item::STEPS_MAX);
        assert!(c
            .steps
            .iter()
            .all(|s| s.text.len() <= crate::work_item::STEP_TEXT_MAX));
        assert!(c.checkpoint.as_ref().unwrap().next.len() <= CHECKPOINT_NEXT_MAX);
        let line = crate::work_item::work_log_line(1, LOG_TOOL, &tool_log_text(&long, false));
        assert!(line.len() <= crate::work_item::WORK_LOG_LINE_MAX, "{line}");
        assert_eq!(tool_log_text("web_fetch", true), "web_fetch ok");
        assert_eq!(tool_log_text(" ", false), "tool fail");
    }

    #[test]
    fn a_stop_keeps_the_card_paused_with_its_checkpoint() {
        let mut c = run_card("Flash the pi");
        begin_run(&mut c, WorkKind::Task, "s", "m", 1);
        mirror_todos(
            &mut c,
            &todos(&[("Write the SD card", "in_progress"), ("Verify", "pending")]),
            2,
        );
        end_run(&mut c, &RunEnd::Stopped, 3);
        let cp = c.checkpoint.clone().unwrap();
        assert_eq!(cp.blocker, STOPPED_BLOCKER);
        assert_eq!(cp.next, "Write the SD card");
        // The next attempt clears the blocker and counts.
        begin_run(&mut c, WorkKind::Task, "", "", 4);
        let cp = c.checkpoint.clone().unwrap();
        assert_eq!(cp.blocker, "");
        assert_eq!(cp.attempt, 2);
        assert_eq!(cp.session, "s", "a blank session keeps the last one");
        end_run(&mut c, &RunEnd::Failed("API key rejected".into()), 5);
        assert_eq!(
            c.checkpoint.as_ref().unwrap().blocker,
            "Failed: API key rejected"
        );
    }

    #[test]
    fn an_automation_reuses_its_card_across_runs() {
        let mut cards = Vec::new();
        let first = begin_automation_card(&mut cards, "auto-1", "Nightly backup", "s1", "m", 10);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].status, BoardStatus::InProgress);
        assert_eq!(cards[0].kind, WorkKind::Automation);
        assert_eq!(cards[0].origin, Some(WorkOrigin::Agent));
        assert_eq!(cards[0].title, "Nightly backup");

        // A run with a report: Done leaves it for file_follow_up.
        let id = finish_automation_card(&mut cards, "auto-1", &RunEnd::Done, true, 20);
        assert_eq!(id.as_deref(), Some(first.as_str()));
        let (fid, new) = crate::workboard::file_follow_up(
            &mut cards,
            "auto-1",
            "Nightly backup",
            "All good",
            21,
        );
        assert_eq!((fid.as_str(), new), (first.as_str(), false));
        assert_eq!(cards[0].status, BoardStatus::FollowUp);

        // The next run takes the same card, and with nothing new it goes back
        // to Follow up because the unread report still waits.
        let again = begin_automation_card(&mut cards, "auto-1", "Nightly backup", "s2", "m", 30);
        assert_eq!(again, first);
        assert_eq!(cards[0].checkpoint.as_ref().unwrap().attempt, 2);
        finish_automation_card(&mut cards, "auto-1", &RunEnd::Done, false, 40);
        assert_eq!(cards[0].status, BoardStatus::FollowUp);

        // Read: a quiet run moves the untouched card to Done and archives it.
        cards[0].fresh = false;
        begin_automation_card(&mut cards, "auto-1", "Nightly backup", "s3", "m", 50);
        finish_automation_card(&mut cards, "auto-1", &RunEnd::Done, false, 60);
        assert_eq!(cards[0].status, BoardStatus::Dismissed);
        assert_eq!(cards[0].archived_ms, 60);

        // The archived agent card comes back on the next run: still one card.
        let back = begin_automation_card(&mut cards, "auto-1", "Nightly backup", "s4", "m", 70);
        assert_eq!(back, first);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].archived_ms, 0);
        finish_automation_card(&mut cards, "auto-1", &RunEnd::Stopped, false, 80);
        assert_eq!(cards[0].status, BoardStatus::Blocked);

        // A card you finished stays yours: the run files a new one.
        cards[0].status = BoardStatus::Done;
        cards[0].touched = true;
        let other = begin_automation_card(&mut cards, "auto-1", "Nightly backup", "s5", "m", 90);
        assert_ne!(other, first);
        assert_eq!(cards.len(), 2);
    }

    #[test]
    fn background_and_episode_runs_get_their_own_card() {
        let mut cards = Vec::new();
        let bg = open_run_card(
            &mut cards,
            WorkKind::Background,
            "Index the logs",
            "native-b",
            "m",
            5,
        );
        assert_eq!(cards[0].kind, WorkKind::Background);
        assert_eq!(cards[0].status, BoardStatus::InProgress);
        assert!(finish_run_card(&mut cards, &bg, &RunEnd::Done, 6));
        assert_eq!(cards[0].status, BoardStatus::Done);
        assert!(!finish_run_card(&mut cards, "nope", &RunEnd::Done, 6));

        let mut ep = run_card("Open the settings");
        begin_run(&mut ep, WorkKind::Episode, "", "", 7);
        assert_eq!(ep.kind, WorkKind::Episode);
        // A later task turn on the same card keeps its kind.
        begin_run(&mut ep, WorkKind::Task, "", "", 8);
        assert_eq!(ep.kind, WorkKind::Episode);
    }

    #[test]
    fn edit_board_file_bumps_rev_and_skips_a_no_op_write() {
        let dir = std::env::temp_dir().join(format!("kb02-edit-{}", crate::uid("t")));
        let path = board_path(&dir);
        let id = edit_board_file(&path, 10, |cards| {
            open_run_card(cards, WorkKind::Task, "Plan the trip", "", "", 10)
        })
        .unwrap();
        let cards = read_board_file(&path).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].rev, 0, "a new card starts at rev 0");
        let mtime = fs::metadata(&path).unwrap().modified().unwrap();

        edit_board_file(&path, 20, |cards| {
            mirror_todos(&mut cards[0], &todos(&[("Book", "pending")]), 20)
        })
        .unwrap();
        let cards = read_board_file(&path).unwrap();
        assert_eq!(cards[0].id, id);
        assert_eq!(cards[0].rev, 1, "the edit bumps rev so it wins the merge");
        assert_eq!(cards[0].steps[0].text, "Book");

        std::thread::sleep(Duration::from_millis(20));
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        assert!(before >= mtime);
        edit_board_file(&path, 30, |_| ()).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            before,
            "nothing changed, nothing written"
        );
        assert!(!lock_path(&path).exists(), "the lock is released");

        fs::write(&path, "not json").unwrap();
        assert!(edit_board_file(&path, 40, |_| ()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "not json", "left alone");
        let _ = fs::remove_dir_all(&dir);
    }
}
