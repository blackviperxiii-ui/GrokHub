//! Resumable work on a workboard card: steps, a capped checkpoint, who owns it,
//! archive and delete, the 30-day sweep, the per-card merge that lets several
//! writers share `workboard.json`, and the short progress log lines.
//!
//! Everything here is plain data work. No model reads or writes any of it, and
//! nothing here goes into a prompt.

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::workboard::{BoardCard, BoardStatus};

/// Who owns a card. Agent cards come from runs, automations and pins; the sweep
/// may archive them once they are done and you never touched them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkOrigin {
    Agent,
    #[default]
    User,
}

/// What kind of work a card tracks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    #[default]
    Task,
    Automation,
    Background,
    Episode,
    Flow,
}

impl WorkKind {
    pub fn is_task(&self) -> bool {
        *self == Self::Task
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    #[default]
    Planned,
    Doing,
    Done,
    Skipped,
    Failed,
}

/// One step of a card's plan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkStep {
    pub text: String,
    #[serde(default)]
    pub status: StepStatus,
}

/// Most steps kept on a card.
pub const STEPS_MAX: usize = 12;
/// Longest step text, in bytes.
pub const STEP_TEXT_MAX: usize = 80;
pub const CHECKPOINT_GOAL_MAX: usize = 160;
pub const CHECKPOINT_NEXT_MAX: usize = 200;
pub const CHECKPOINT_BLOCKER_MAX: usize = 160;
pub const CHECKPOINT_REFS_MAX: usize = 4;
pub const CHECKPOINT_REF_MAX: usize = 80;
/// Session id and model name on a checkpoint.
pub const CHECKPOINT_TAG_MAX: usize = 64;

/// Where the work left off, small enough to hand back on resume.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goal: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub next: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub blocker: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub at_ms: u64,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

impl Checkpoint {
    /// Every field on one line and within its byte cap. Empty refs are dropped.
    pub fn clean(self) -> Self {
        Self {
            goal: clip_line(&self.goal, CHECKPOINT_GOAL_MAX),
            next: clip_line(&self.next, CHECKPOINT_NEXT_MAX),
            blocker: clip_line(&self.blocker, CHECKPOINT_BLOCKER_MAX),
            refs: self
                .refs
                .iter()
                .map(|r| clip_line(r, CHECKPOINT_REF_MAX))
                .filter(|r| !r.is_empty())
                .take(CHECKPOINT_REFS_MAX)
                .collect(),
            session: clip_line(&self.session, CHECKPOINT_TAG_MAX),
            model: clip_line(&self.model, CHECKPOINT_TAG_MAX),
            attempt: self.attempt,
            at_ms: self.at_ms,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.goal.is_empty()
            && self.next.is_empty()
            && self.blocker.is_empty()
            && self.refs.is_empty()
    }
}

/// Steps on one line each, within caps, at most [`STEPS_MAX`]. Blank steps go.
pub fn clean_steps(steps: Vec<WorkStep>) -> Vec<WorkStep> {
    steps
        .into_iter()
        .map(|s| WorkStep {
            text: clip_line(&s.text, STEP_TEXT_MAX),
            status: s.status,
        })
        .filter(|s| !s.text.is_empty())
        .take(STEPS_MAX)
        .collect()
}

/// Whitespace collapsed to single spaces, then cut to at most `max` bytes on a
/// character boundary.
pub fn clip_line(text: &str, max: usize) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.len() <= max {
        return joined;
    }
    let mut end = max;
    while !joined.is_char_boundary(end) {
        end -= 1;
    }
    joined[..end].trim_end().to_string()
}

/// A card's steps and checkpoint within their caps.
pub fn clean_card(card: &mut BoardCard) {
    card.steps = clean_steps(std::mem::take(&mut card.steps));
    card.checkpoint = card
        .checkpoint
        .take()
        .map(Checkpoint::clean)
        .filter(|c| !c.is_empty());
}

/// Fill what cards from before owners lack. Safe to run on every load.
/// Run and automation cards are the agent's; the rest are yours. A card with no
/// creation time counts from `now`, so an upgrade never makes old cards look idle.
/// Old archived cards take `updated_ms` as when they were archived.
pub fn migrate_cards(cards: &mut [BoardCard], now: u64) -> bool {
    let mut changed = false;
    for c in cards.iter_mut() {
        if c.origin.is_none() {
            let agent = c.run || c.automation.is_some();
            c.origin = Some(if agent {
                WorkOrigin::Agent
            } else {
                WorkOrigin::User
            });
            if c.automation.is_some() && c.kind.is_task() {
                c.kind = WorkKind::Automation;
            }
            changed = true;
        }
        if c.created_ms == 0 {
            c.created_ms = now;
            changed = true;
        }
        if c.status == BoardStatus::Dismissed && c.archived_ms == 0 {
            c.archived_ms = if c.updated_ms > 0 { c.updated_ms } else { now };
            changed = true;
        }
        let before = (c.steps.clone(), c.checkpoint.clone());
        clean_card(c);
        if (c.steps.clone(), c.checkpoint.clone()) != before {
            changed = true;
        }
    }
    changed
}

/// The newest of creation, the last run update, the last change and the checkpoint.
pub fn last_activity_ms(card: &BoardCard) -> u64 {
    let at = card.checkpoint.as_ref().map(|c| c.at_ms).unwrap_or(0);
    card.created_ms
        .max(card.updated_ms)
        .max(card.active_ms)
        .max(at)
}

fn bump(card: &mut BoardCard, now: u64) {
    card.rev = card.rev.saturating_add(1);
    card.active_ms = card.active_ms.max(now);
}

/// You looked at or changed the card, so the sweep leaves it to you.
pub fn mark_touched(cards: &mut [BoardCard], id: &str, now: u64) -> bool {
    let Some(c) = cards.iter_mut().find(|c| c.id == id) else {
        return false;
    };
    c.touched = true;
    bump(c, now);
    true
}

/// Off the board into Archived.
pub fn archive(cards: &mut [BoardCard], id: &str, now: u64) -> bool {
    let Some(c) = cards.iter_mut().find(|c| c.id == id) else {
        return false;
    };
    archive_card(c, now)
}

pub(crate) fn archive_card(c: &mut BoardCard, now: u64) -> bool {
    if c.status == BoardStatus::Dismissed {
        return false;
    }
    c.status = BoardStatus::Dismissed;
    c.archived_ms = now;
    bump(c, now);
    true
}

/// Back on the board in Todo, as Restore always did. Counts as yours.
pub fn restore(cards: &mut [BoardCard], id: &str, now: u64) -> bool {
    let Some(c) = cards.iter_mut().find(|c| c.id == id) else {
        return false;
    };
    if c.status != BoardStatus::Dismissed {
        return false;
    }
    c.status = BoardStatus::Todo;
    c.archived_ms = 0;
    c.touched = true;
    bump(c, now);
    true
}

/// Remove the card. The caller removes its progress log too.
pub fn delete(cards: &mut Vec<BoardCard>, id: &str) -> bool {
    let before = cards.len();
    cards.retain(|c| c.id != id);
    cards.len() != before
}

/// Delete archived cards archived at least `older_than_ms` ago. Returns their ids
/// so the caller can remove their logs. Only ever run on your say-so.
pub fn delete_archived(cards: &mut Vec<BoardCard>, older_than_ms: u64, now: u64) -> Vec<String> {
    let mut gone = Vec::new();
    cards.retain(|c| {
        let old = c.status == BoardStatus::Dismissed
            && c.archived_ms > 0
            && now.saturating_sub(c.archived_ms) >= older_than_ms;
        if old {
            gone.push(c.id.clone());
        }
        !old
    });
    gone
}

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// A card with no activity this long and no live run is archived.
pub const IDLE_ARCHIVE_MS: u64 = 30 * DAY_MS;
/// An agent's finished card stays in Done this long so you can see the result.
pub const AGENT_DONE_GRACE_MS: u64 = DAY_MS;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Agent cards that finished and that you never touched.
    pub archived_done: Vec<String>,
    /// Cards idle for [`IDLE_ARCHIVE_MS`].
    pub archived_idle: Vec<String>,
}

impl SweepReport {
    pub fn is_empty(&self) -> bool {
        self.archived_done.is_empty() && self.archived_idle.is_empty()
    }
}

/// The local tidy. Archives an agent's untouched Done card a day after it
/// finished, and any card idle for 30 days. A card whose id or thread id is in
/// `live_ids` is never archived. Nothing is ever deleted.
pub fn sweep(cards: &mut [BoardCard], now: u64, live_ids: &[String]) -> SweepReport {
    let mut report = SweepReport::default();
    for c in cards.iter_mut() {
        if c.status == BoardStatus::Dismissed {
            continue;
        }
        let live = live_ids
            .iter()
            .any(|l| *l == c.id || c.thread_id.as_deref() == Some(l.as_str()));
        if live {
            continue;
        }
        let active = last_activity_ms(c);
        if active == 0 {
            continue;
        }
        let idle = now.saturating_sub(active);
        let agent_done =
            c.status == BoardStatus::Done && c.origin == Some(WorkOrigin::Agent) && !c.touched;
        if agent_done && idle >= AGENT_DONE_GRACE_MS {
            let id = c.id.clone();
            if archive_card(c, now) {
                report.archived_done.push(id);
            }
        } else if idle >= IDLE_ARCHIVE_MS {
            let id = c.id.clone();
            if archive_card(c, now) {
                report.archived_idle.push(id);
            }
        }
    }
    report
}

/// What a writer last saw of each card: its rev and a fingerprint of the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaseEntry {
    pub rev: u64,
    pub fp: u64,
}

pub type BoardBase = HashMap<String, BaseEntry>;

/// FNV-1a over the card's JSON with `rev` and `active_ms` left out.
pub fn card_fingerprint(card: &BoardCard) -> u64 {
    let mut c = card.clone();
    c.rev = 0;
    c.active_ms = 0;
    let bytes = serde_json::to_vec(&c).unwrap_or_default();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub fn board_base(cards: &[BoardCard]) -> BoardBase {
    cards
        .iter()
        .map(|c| {
            (
                c.id.clone(),
                BaseEntry {
                    rev: c.rev,
                    fp: card_fingerprint(c),
                },
            )
        })
        .collect()
}

/// Bump `rev` on each card that changed since `base` and was not bumped yet,
/// so an edit made by plain field writes still wins the merge. Returns how many.
pub fn stamp_changes(cards: &mut [BoardCard], base: &BoardBase, now: u64) -> usize {
    let mut n = 0;
    for c in cards.iter_mut() {
        let Some(b) = base.get(&c.id) else {
            continue;
        };
        if c.rev <= b.rev && card_fingerprint(c) != b.fp {
            c.rev = b.rev + 1;
            c.active_ms = c.active_ms.max(now);
            n += 1;
        }
    }
    n
}

/// Merge this writer's cards with what is on disk now, card by card.
/// The higher `rev` wins; a tie keeps this writer's copy. A card in `base` that
/// this writer no longer has was deleted here. A card in `base` that is gone
/// from disk and unchanged here was deleted by another writer. Cards only on
/// disk were added elsewhere and are kept after this writer's cards.
pub fn merge_cards(base: &BoardBase, mine: &[BoardCard], disk: &[BoardCard]) -> Vec<BoardCard> {
    let on_disk: HashMap<&str, &BoardCard> = disk.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut out = Vec::with_capacity(mine.len().max(disk.len()));
    for m in mine {
        match on_disk.get(m.id.as_str()) {
            Some(d) if d.rev > m.rev => out.push((*d).clone()),
            Some(_) => out.push(m.clone()),
            None => {
                let deleted_elsewhere = base.get(&m.id).is_some_and(|b| m.rev <= b.rev);
                if !deleted_elsewhere {
                    out.push(m.clone());
                }
            }
        }
    }
    for d in disk {
        let in_mine = mine.iter().any(|m| m.id == d.id);
        if !in_mine && !base.contains_key(&d.id) {
            out.push(d.clone());
        }
    }
    out
}

/// Longest progress log line, in bytes, JSON included.
pub const WORK_LOG_LINE_MAX: usize = 160;
const WORK_LOG_KIND_MAX: usize = 16;
/// A progress log past this size drops its oldest half.
pub const WORK_LOG_FILE_MAX: usize = 32 * 1024;

/// One line of a card's progress log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkLogEvent {
    pub at: u64,
    pub kind: String,
    pub text: String,
}

/// One JSON line (no newline) within [`WORK_LOG_LINE_MAX`] bytes. The text is
/// shortened to fit.
pub fn work_log_line(at: u64, kind: &str, text: &str) -> String {
    let mut ev = WorkLogEvent {
        at,
        kind: clip_line(kind, WORK_LOG_KIND_MAX),
        text: clip_line(text, WORK_LOG_LINE_MAX),
    };
    loop {
        let line = serde_json::to_string(&ev).unwrap_or_default();
        if line.len() <= WORK_LOG_LINE_MAX || ev.text.is_empty() {
            return line;
        }
        let over = line.len() - WORK_LOG_LINE_MAX;
        let keep = ev.text.len().saturating_sub(over);
        ev.text = clip_line(&ev.text, keep);
    }
}

/// The newest lines that fit in half of [`WORK_LOG_FILE_MAX`], once the log is
/// over the cap. `None` while it is within the cap.
pub fn trim_work_log(body: &str) -> Option<String> {
    if body.len() <= WORK_LOG_FILE_MAX {
        return None;
    }
    let budget = WORK_LOG_FILE_MAX / 2;
    let mut kept: Vec<&str> = Vec::new();
    let mut size = 0;
    for line in body.lines().rev().filter(|l| !l.trim().is_empty()) {
        if size + line.len() + 1 > budget {
            break;
        }
        size += line.len() + 1;
        kept.push(line);
    }
    kept.reverse();
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Some(out)
}

/// Events in a log body. Lines that do not parse are skipped.
pub fn parse_work_log(body: &str) -> Vec<WorkLogEvent> {
    body.lines()
        .filter_map(|l| serde_json::from_str(l.trim()).ok())
        .collect()
}

/// `<id>.jsonl`, or `None` when the id could step outside the log folder.
pub fn work_log_file_name(id: &str) -> Option<String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| format!("{id}.jsonl"))
}

fn work_log_path(dir: &Path, id: &str) -> Option<PathBuf> {
    work_log_file_name(id).map(|n| dir.join(n))
}

pub(crate) fn open_private(path: &Path, append: bool) -> std::io::Result<fs::File> {
    let mut o = fs::OpenOptions::new();
    o.create(true);
    if append {
        o.append(true);
    } else {
        o.write(true).truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
}

/// Add one line to the card's log in `dir` (`{config_dir}/work`). Past
/// [`WORK_LOG_FILE_MAX`] the oldest half is dropped (temp file, then rename).
pub fn append_work_log(
    dir: &Path,
    id: &str,
    at: u64,
    kind: &str,
    text: &str,
) -> Result<(), String> {
    let path = work_log_path(dir, id).ok_or_else(|| "bad card id".to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut line = work_log_line(at, kind, text);
    line.push('\n');
    let mut f = open_private(&path, true).map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    drop(f);
    if len <= WORK_LOG_FILE_MAX as u64 {
        return Ok(());
    }
    let body = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let Some(kept) = trim_work_log(&body) else {
        return Ok(());
    };
    let tmp = dir.join(format!(".{id}.{}.tmp", std::process::id()));
    let mut t = open_private(&tmp, false).map_err(|e| e.to_string())?;
    t.write_all(kept.as_bytes()).map_err(|e| e.to_string())?;
    t.sync_all().map_err(|e| e.to_string())?;
    drop(t);
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

/// The card's log, oldest first. Empty when there is none.
pub fn read_work_log(dir: &Path, id: &str) -> Vec<WorkLogEvent> {
    work_log_path(dir, id)
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|b| parse_work_log(&b))
        .unwrap_or_default()
}

/// Remove the card's log, if it has one.
pub fn remove_work_log(dir: &Path, id: &str) {
    if let Some(p) = work_log_path(dir, id) {
        let _ = fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = DAY_MS;

    fn card(id: &str, status: BoardStatus, origin: WorkOrigin, active: u64) -> BoardCard {
        let mut c = BoardCard::new(id, "", "");
        c.id = id.into();
        c.status = status;
        c.origin = Some(origin);
        c.created_ms = active;
        c
    }

    #[test]
    fn old_workboard_json_loads_and_migrates() {
        let old = r#"[
          {"id":"w-1","title":"Flash the pi","detail":"","status":"done","run":true,"threadId":"t1","updatedMs":500},
          {"id":"w-2","title":"Buy cable","detail":"","status":"todo","priority":"high"},
          {"id":"w-3","title":"Old idea","detail":"","status":"dismissed","updatedMs":700},
          {"id":"w-4","title":"Nightly","detail":"","status":"follow_up","automation":"a1","report":"r"}
        ]"#;
        let mut cards: Vec<BoardCard> = serde_json::from_str(old).unwrap();
        assert_eq!(cards[0].origin, None);
        assert_eq!(cards[0].rev, 0);
        assert!(migrate_cards(&mut cards, 9_000));
        assert_eq!(cards[0].origin, Some(WorkOrigin::Agent));
        assert_eq!(cards[0].kind, WorkKind::Task);
        assert_eq!(cards[1].origin, Some(WorkOrigin::User));
        assert_eq!(cards[1].priority, "high");
        assert_eq!(cards[2].archived_ms, 700);
        assert_eq!(cards[2].status, BoardStatus::Dismissed);
        assert_eq!(cards[3].origin, Some(WorkOrigin::Agent));
        assert_eq!(cards[3].kind, WorkKind::Automation);
        assert_eq!(cards[1].created_ms, 9_000);
        assert!(!migrate_cards(&mut cards, 10_000), "migration runs once");
        assert_eq!(cards[1].created_ms, 9_000);

        let json = serde_json::to_string(&cards).unwrap();
        let back: Vec<BoardCard> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 4);
        assert_eq!(back[0].origin, Some(WorkOrigin::Agent));
        assert_eq!(back[0].thread_id.as_deref(), Some("t1"));
        assert_eq!(back[2].archived_ms, 700);
        assert_eq!(back[3].kind, WorkKind::Automation);
        assert!(
            !json.contains("\"steps\"") && !json.contains("\"checkpoint\""),
            "empty work fields stay out of the file: {json}"
        );
    }

    #[test]
    fn caps_hold_on_oversized_input() {
        let big = "x".repeat(1000);
        let cp = Checkpoint {
            goal: format!("  {big}\n\n"),
            next: big.clone(),
            blocker: big.clone(),
            refs: vec![big.clone(); 9],
            session: big.clone(),
            model: big.clone(),
            attempt: 3,
            at_ms: 42,
        }
        .clean();
        assert_eq!(cp.goal.len(), 160);
        assert_eq!(cp.next.len(), 200);
        assert_eq!(cp.blocker.len(), 160);
        assert_eq!(cp.refs.len(), 4);
        assert!(cp.refs.iter().all(|r| r.len() == 80));
        assert_eq!(cp.session.len(), 64);
        assert_eq!(cp.model.len(), 64);
        assert_eq!(cp.attempt, 3);
        // Multi-byte text is cut on a character boundary.
        let snow = "é".repeat(200);
        assert_eq!(clip_line(&snow, 161).len(), 160);
        assert_eq!(clip_line("a\n  b\tc", 80), "a b c");

        let steps: Vec<WorkStep> = (0..30)
            .map(|i| WorkStep {
                text: if i == 0 { "   ".into() } else { big.clone() },
                status: StepStatus::Doing,
            })
            .collect();
        let steps = clean_steps(steps);
        assert_eq!(steps.len(), 12);
        assert!(steps.iter().all(|s| s.text.len() == 80));

        let mut c = card("w-c", BoardStatus::Todo, WorkOrigin::User, 1);
        c.checkpoint = Some(Checkpoint {
            goal: "   ".into(),
            ..Default::default()
        });
        clean_card(&mut c);
        assert_eq!(c.checkpoint, None, "a blank checkpoint is dropped");
    }

    #[test]
    fn sweep_archives_finished_agent_work_and_idle_cards_never_deletes() {
        let now = 100 * DAY;
        let mut cards = vec![
            card(
                "agent-done",
                BoardStatus::Done,
                WorkOrigin::Agent,
                now - 2 * DAY,
            ),
            card(
                "agent-done-new",
                BoardStatus::Done,
                WorkOrigin::Agent,
                now - 1000,
            ),
            card(
                "user-done",
                BoardStatus::Done,
                WorkOrigin::User,
                now - 2 * DAY,
            ),
            card(
                "touched",
                BoardStatus::Done,
                WorkOrigin::Agent,
                now - 2 * DAY,
            ),
            card("idle", BoardStatus::Todo, WorkOrigin::User, now - 31 * DAY),
            card(
                "recent",
                BoardStatus::Todo,
                WorkOrigin::User,
                now - 29 * DAY,
            ),
            card(
                "live",
                BoardStatus::InProgress,
                WorkOrigin::Agent,
                now - 40 * DAY,
            ),
            card(
                "live-thread",
                BoardStatus::Done,
                WorkOrigin::Agent,
                now - 40 * DAY,
            ),
            card("unknown-age", BoardStatus::Todo, WorkOrigin::User, 0),
        ];
        cards[3].touched = true;
        cards[7].thread_id = Some("thr-live".into());
        let live = vec!["live".to_string(), "thr-live".to_string()];
        let report = sweep(&mut cards, now, &live);
        assert_eq!(report.archived_done, vec!["agent-done".to_string()]);
        assert_eq!(report.archived_idle, vec!["idle".to_string()]);
        assert_eq!(cards.len(), 9, "the sweep never deletes");
        let status = |id: &str| cards.iter().find(|c| c.id == id).unwrap().status;
        assert_eq!(status("agent-done"), BoardStatus::Dismissed);
        assert_eq!(status("idle"), BoardStatus::Dismissed);
        for id in [
            "agent-done-new",
            "user-done",
            "touched",
            "recent",
            "live",
            "live-thread",
            "unknown-age",
        ] {
            assert_ne!(status(id), BoardStatus::Dismissed, "{id} must wait");
        }
        let archived = cards.iter().find(|c| c.id == "idle").unwrap();
        assert_eq!(archived.archived_ms, now);
        assert_eq!(archived.rev, 1);
        assert!(
            sweep(&mut cards, now, &live).is_empty(),
            "a second sweep is a no-op"
        );
    }

    /// KB-04: the sweep, the 30-day archive and the log trim are local jobs.
    /// Their signatures take cards, times and ids, never a model client; this
    /// crate cannot reach one (it does not depend on the agent crate), and no
    /// board job here names a model or a request.
    #[test]
    fn board_sweeps_are_local_jobs_with_no_model() {
        let _: fn(&mut [BoardCard], u64, &[String]) -> SweepReport = sweep;
        let _: fn(&mut Vec<BoardCard>, u64, u64) -> Vec<String> = delete_archived;
        let _: fn(&mut [BoardCard], &str, u64) -> bool = archive;
        let _: fn(&mut BoardCard, u64) -> bool = archive_card;
        let _: fn(&str) -> Option<String> = trim_work_log;
        let _: fn(&mut [BoardCard], u64) -> bool = migrate_cards;
        let manifest = include_str!("../Cargo.toml");
        assert!(
            !manifest
                .lines()
                .any(|l| l.trim_start().starts_with("grokhub-agent")),
            "grokhub-core must not depend on the agent crate"
        );
        let src = include_str!("work_item.rs").replace("\r\n", "\n");
        let jobs = src.split("\n#[cfg(test)]").next().unwrap_or_default();
        for word in [
            concat!("Model", "Client"),
            concat!("Responses", "Request"),
            concat!("grokhub", "_agent"),
            concat!("reqwest", "::"),
            concat!("ureq", "::"),
        ] {
            assert!(!jobs.contains(word), "{word} in the board jobs");
        }
        assert_eq!(IDLE_ARCHIVE_MS, 30 * DAY_MS);
    }

    #[test]
    fn archive_restore_delete_round_trip() {
        let mut cards = vec![
            card("a", BoardStatus::Done, WorkOrigin::Agent, 10),
            card("b", BoardStatus::Todo, WorkOrigin::User, 10),
        ];
        assert!(archive(&mut cards, "a", 50));
        assert!(!archive(&mut cards, "a", 60), "already archived");
        assert_eq!(cards[0].status, BoardStatus::Dismissed);
        assert_eq!(cards[0].archived_ms, 50);
        assert!(restore(&mut cards, "a", 70));
        assert_eq!(cards[0].status, BoardStatus::Todo);
        assert_eq!(cards[0].archived_ms, 0);
        assert!(cards[0].touched, "restoring hands the card to you");
        assert_eq!(cards[0].rev, 2);
        assert!(mark_touched(&mut cards, "b", 80));
        assert!(cards[1].touched);
        assert_eq!(cards[1].active_ms, 80);
        assert!(!mark_touched(&mut cards, "missing", 80));

        assert!(archive(&mut cards, "b", 100));
        assert!(archive(&mut cards, "a", 900));
        assert_eq!(
            delete_archived(&mut cards, 500, 1000),
            vec!["b".to_string()]
        );
        assert_eq!(cards.len(), 1);
        assert!(delete(&mut cards, "a"));
        assert!(!delete(&mut cards, "a"));
        assert!(cards.is_empty());
    }

    #[test]
    fn merge_keeps_both_writers_edits() {
        let mut start = vec![
            card("one", BoardStatus::Todo, WorkOrigin::User, 1),
            card("two", BoardStatus::Todo, WorkOrigin::User, 1),
            card("gone", BoardStatus::Todo, WorkOrigin::User, 1),
        ];
        migrate_cards(&mut start, 1);
        let base = board_base(&start);

        // Writer A moves "one" and deletes "gone" by plain field writes.
        let mut a = start.clone();
        a[0].status = BoardStatus::InProgress;
        a.retain(|c| c.id != "gone");
        assert_eq!(stamp_changes(&mut a, &base, 5), 1);
        assert_eq!(a[0].rev, 1);
        assert_eq!(stamp_changes(&mut a, &base, 6), 0, "one bump per change");

        // Writer B edits "two" and adds "three", from the same start.
        let mut b = start.clone();
        b[1].notes = "check the cable".into();
        b.push(card("three", BoardStatus::Todo, WorkOrigin::Agent, 2));
        stamp_changes(&mut b, &base, 5);

        let disk_after_a = merge_cards(&base, &a, &start);
        let ids: Vec<&str> = disk_after_a.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["one", "two"]);

        let disk_after_b = merge_cards(&base, &b, &disk_after_a);
        let ids: Vec<&str> = disk_after_b.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["one", "two", "three"],
            "B does not resurrect gone"
        );
        assert_eq!(
            disk_after_b[0].status,
            BoardStatus::InProgress,
            "A's move kept"
        );
        assert_eq!(disk_after_b[1].notes, "check the cable", "B's note kept");
    }

    #[test]
    fn log_lines_are_capped_and_the_log_trims_its_oldest_half() {
        let line = work_log_line(
            1_730_000_000_000,
            "step-done-and-more-kind",
            &"é\"".repeat(300),
        );
        assert!(line.len() <= WORK_LOG_LINE_MAX, "{} bytes", line.len());
        let ev: WorkLogEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(ev.kind.len(), 16);
        assert!(!ev.text.is_empty());
        assert_eq!(
            work_log_line(5, "note", "short"),
            r#"{"at":5,"kind":"note","text":"short"}"#
        );

        let mut body = String::new();
        let mut n = 0;
        while body.len() <= WORK_LOG_FILE_MAX {
            body.push_str(&work_log_line(n, "note", &format!("event {n}")));
            body.push('\n');
            n += 1;
        }
        assert_eq!(trim_work_log("small\n"), None);
        let trimmed = trim_work_log(&body).unwrap();
        assert!(trimmed.len() <= WORK_LOG_FILE_MAX / 2);
        let events = parse_work_log(&trimmed);
        assert_eq!(events.last().unwrap().at, n - 1, "the newest event stays");
        assert!(events[0].at > 0, "the oldest events go");
        assert_eq!(work_log_file_name("w-0a1b"), Some("w-0a1b.jsonl".into()));
        assert_eq!(work_log_file_name("../x"), None);
        assert_eq!(work_log_file_name(""), None);
    }

    #[test]
    fn progress_log_appends_trims_and_is_removed() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-work-log-{}-{}",
            std::process::id(),
            crate::uid("t")
        ));
        let _ = fs::remove_dir_all(&dir);
        append_work_log(&dir, "w-1", 10, "step", "Wrote the image").unwrap();
        append_work_log(&dir, "w-1", 11, "note", "Booted").unwrap();
        let events = read_work_log(&dir, "w-1");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].text, "Wrote the image");
        assert_eq!(events[0].kind, "step");
        assert_eq!(events[1].at, 11);
        assert!(append_work_log(&dir, "../w", 1, "x", "y").is_err());

        for i in 0..400 {
            append_work_log(&dir, "w-1", 100 + i, "note", &"z".repeat(120)).unwrap();
        }
        let size = fs::metadata(dir.join("w-1.jsonl")).unwrap().len() as usize;
        assert!(size <= WORK_LOG_FILE_MAX, "{size}");
        let events = read_work_log(&dir, "w-1");
        assert_eq!(events.last().unwrap().at, 499, "newest kept");
        assert!(events[0].at > 11, "oldest dropped");
        assert!(events.len() < 402);

        remove_work_log(&dir, "w-1");
        assert!(read_work_log(&dir, "w-1").is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
