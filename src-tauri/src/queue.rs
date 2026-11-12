//! The Agents page, the cabin's Queue: the open chat's to-dos and its
//! background commands, subagents and monitors, each with a Stop, and the
//! queued jobs, each with a Run.

use std::path::Path;
use std::sync::Mutex;

use grokhub_agent::{
    By, Counts, Ended, Held, Resume, TaskProgress, TaskRow, TaskState, Work, WorkState,
};
use serde::Serialize;

/// A background task's status on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Failed,
    Done,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: Status,
    /// Still going, so Stop has something to stop.
    pub live: bool,
    /// A helper's progress, when this app watched it run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,
}

/// How far a helper has got: its tool steps, the last tool's name, the
/// tokens it used and the seconds it has run (or ran, once it ended).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub steps: u32,
    pub last_step: String,
    pub tokens: u64,
    pub seconds: u64,
}

/// A helper's progress as the page shows it, with its clock read at `now_ms`.
fn progress_of(progress: &TaskProgress, now_ms: u64) -> Progress {
    let end = progress.ended_ms.unwrap_or(now_ms);
    Progress {
        steps: progress.steps,
        last_step: progress.last_step.clone(),
        tokens: progress.tokens,
        seconds: end.saturating_sub(progress.started_ms) / 1000,
    }
}

/// Tasks → Details: a helper's steps, by tool name and oldest first, and its
/// result as plain text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub steps: Vec<String>,
    pub result: String,
}

/// One `todo_write` row: "pending", "in_progress", "completed" or "cancelled".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: String,
    pub content: String,
    pub status: String,
    /// The helper the task was handed to, by its task id (`s3`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub state: TodoState,
}

/// Where a to-do stands on the Tasks panel, from its status and its helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TodoState {
    Planned,
    Running,
    /// Running, and the session waits on the person.
    NeedsYou,
    /// Its helper finished; the lead has not closed the task yet.
    Review,
    /// Its helper's work waits in a worktree: ready, in conflict or held.
    NotMerged,
    Done,
    Failed,
}

/// Whether a helper's title names the to-do: it is the to-do's id, or starts
/// with it and then `:` or a space.
fn names(title: &str, todo_id: &str) -> bool {
    !todo_id.is_empty()
        && title
            .strip_prefix(todo_id)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', ' ']))
}

/// The helper a to-do was handed to: the last helper row (`s…`) whose title
/// names it. The last one wins, so a retry replaces a stopped helper.
fn owner_of<'a>(todo_id: &str, rows: &'a [TaskRow]) -> Option<&'a TaskRow> {
    rows.iter()
        .rev()
        .find(|row| row.id.starts_with('s') && names(&row.title, todo_id))
}

/// The worktree work done for a to-do: the newest record whose helper's
/// description names it. It needs no helper row, so it holds after a
/// relaunch.
fn work_of<'a>(todo_id: &str, work: &'a [Work]) -> Option<&'a Work> {
    work.iter()
        .filter(|record| names(&record.description, todo_id))
        .max_by_key(|record| record.created_ms)
}

/// A helper's work as a row states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkStatus {
    Running,
    Ready,
    Conflict,
    Held,
    Merged,
    Empty,
    Discarded,
}

/// One helper's work in a worktree, as the Tasks panel lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperWork {
    pub key: String,
    /// The helper's task id (`s2`) in the hub that ran it.
    pub task: String,
    pub title: String,
    pub state: WorkStatus,
    /// The helper did not finish. Shown whatever the state is.
    pub partial: bool,
    /// How the helper ended, in words.
    pub ended: String,
    pub files: Vec<String>,
    pub counts: Counts,
    /// Ignored paths the helper wrote. They are never merged.
    pub ignored: Vec<String>,
    /// The files of a conflict.
    pub conflicts: Vec<String>,
    /// Why the work is held: `partial`, `gitStateChanged`, `changedAfterSeal`,
    /// `ignoredOnly`, `nestedRepo`, `sealFailed`, `interrupted`, `orphan`,
    /// `projectChanged`, `blocked`, `mergeFailed`, `rolledBack` (a merge that
    /// could not be written and was put back) or `applyFailed` (one that
    /// stopped part way).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<&'static str>,
    /// The same reason in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why_text: Option<String>,
    /// Files a finished or put-back merge left as the person changed them.
    pub skipped: Vec<String>,
    /// The worktree folder.
    pub path: String,
    /// The worktree is still on disk.
    pub tree_kept: bool,
    /// A merge stopped part way: it is finished or put back, nothing else.
    pub landing: bool,
    /// A helper row of this launch is this work's helper.
    pub live: bool,
    /// The title of the work this helper redid, or that work's key once its
    /// record is gone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redo_of: Option<String>,
    /// This work still waits, and a newer helper that finished redid it: the
    /// redo's work is sealed whole, or merged.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub redone: bool,
}

fn why_kind(why: &Held) -> &'static str {
    match why {
        Held::Partial => "partial",
        Held::GitStateChanged => "gitStateChanged",
        Held::ChangedAfterSeal => "changedAfterSeal",
        Held::IgnoredOnly => "ignoredOnly",
        Held::NestedRepo(_) => "nestedRepo",
        Held::SealFailed(_) => "sealFailed",
        Held::Interrupted => "interrupted",
        Held::Orphan => "orphan",
        Held::ProjectChanged => "projectChanged",
        Held::Blocked(_) => "blocked",
        Held::ApplyFailed {
            rolled_back: true, ..
        } => "rolledBack",
        Held::ApplyFailed { .. } => "applyFailed",
        Held::MergeFailed(_) => "mergeFailed",
    }
}

/// A record as a row. `live` is whether a helper row of this launch is its
/// helper, and `tree_kept` whether its worktree is on disk. A tree that is
/// still being prepared has no row.
fn helper_work(work: &Work, live: bool, tree_kept: bool) -> Option<HelperWork> {
    let (state, conflicts, why) = match &work.state {
        WorkState::Opening => return None,
        WorkState::Running => (WorkStatus::Running, Vec::new(), None),
        WorkState::Empty => (WorkStatus::Empty, Vec::new(), None),
        WorkState::Ready => (WorkStatus::Ready, Vec::new(), None),
        WorkState::Conflict { files } => (WorkStatus::Conflict, files.clone(), None),
        WorkState::Held { why } => (WorkStatus::Held, Vec::new(), Some(why)),
        WorkState::Merged => (WorkStatus::Merged, Vec::new(), None),
        WorkState::Discarded => (WorkStatus::Discarded, Vec::new(), None),
    };
    Some(HelperWork {
        key: work.key.clone(),
        task: work.task.clone(),
        title: work.title.clone(),
        state,
        partial: state != WorkStatus::Running && work.ended != Some(Ended::Done),
        ended: grokhub_agent::how_ended(work.ended.as_ref()),
        files: work.files.clone(),
        counts: work.counts,
        ignored: work.ignored.clone(),
        conflicts,
        why: why.map(why_kind),
        why_text: why.map(grokhub_agent::why_held),
        skipped: work.skipped.clone(),
        path: work.tree.display().to_string(),
        tree_kept,
        landing: work.landing.is_some(),
        live,
        redo_of: None,
        redone: false,
    })
}

/// Whether the person still has this work to merge or discard.
fn waits(state: &WorkState) -> bool {
    matches!(
        state,
        WorkState::Ready | WorkState::Conflict { .. } | WorkState::Held { .. }
    )
}

/// Whether a redo did the work again: it is merged, or its helper finished
/// and what it made is sealed and waits. A redo that still runs, changed
/// nothing, was stopped part way, was never sealed or was discarded redid
/// nothing, and the person must not discard the old work on its word.
fn redid(redo: &Work) -> bool {
    redo.state == WorkState::Merged
        || (waits(&redo.state) && redo.tip.is_some() && redo.ended == Some(Ended::Done))
}

/// Whether a record has a row. Work that waits for the person always has
/// one, from any launch. Work that is over has one only for a helper row of
/// this launch, or while its worktree is kept.
fn listed(row: &HelperWork) -> bool {
    match row.state {
        WorkStatus::Running | WorkStatus::Ready | WorkStatus::Conflict | WorkStatus::Held => true,
        WorkStatus::Merged => row.live || row.tree_kept,
        WorkStatus::Empty | WorkStatus::Discarded => row.live,
    }
}

/// The rows of a chat's records. `nonce` is the live hub's id and `rows` its
/// tasks; a chat with no live hub has neither.
fn work_rows(records: &[Work], nonce: Option<&str>, rows: &[TaskRow]) -> Vec<HelperWork> {
    records
        .iter()
        .filter_map(|work| {
            let live =
                nonce == Some(work.hub.as_str()) && rows.iter().any(|row| row.id == work.task);
            let tree_kept = std::fs::symlink_metadata(&work.tree).is_ok();
            let mut row = helper_work(work, live, tree_kept)?;
            row.redo_of = work.redo_of.as_ref().map(|old| {
                let found = records.iter().find(|other| &other.key == old);
                found.map_or_else(|| old.clone(), |other| other.title.clone())
            });
            row.redone = waits(&work.state)
                && records.iter().any(|other| {
                    other.redo_of.as_deref() == Some(work.key.as_str()) && redid(other)
                });
            Some(row)
        })
        .filter(listed)
        .collect()
}

/// A to-do's state. `working` is whether a turn of the session runs now, and
/// `needs_you` whether the session waits on the person. A helper that already
/// ended does not speak for a task that has not started: its id may be an
/// earlier plan's.
///
/// `work` is the state of the newest worktree work done for it. Work that
/// waits for the person comes first: a task whose files are not in the
/// project is not done, whatever its status says.
fn todo_state(
    status: &str,
    owner: Option<&TaskRow>,
    work: Option<&WorkState>,
    working: bool,
    needs_you: bool,
) -> TodoState {
    if work.is_some_and(waits) {
        return TodoState::NotMerged;
    }
    let running = if needs_you {
        TodoState::NeedsYou
    } else {
        TodoState::Running
    };
    match (status, owner.map(|row| &row.state)) {
        ("completed", _) => TodoState::Done,
        ("cancelled", _) => TodoState::Failed,
        (_, Some(TaskState::Running)) => running,
        ("in_progress", Some(TaskState::Exited(0))) => TodoState::Review,
        ("in_progress", Some(TaskState::Exited(_) | TaskState::Killed)) => TodoState::Failed,
        ("in_progress", None) if working => running,
        _ => TodoState::Planned,
    }
}

/// A job waiting on the Queue: the cabin's `AgentJob`. Run sends `prompt`
/// to Chat as a normal turn, in `thread_id` when it has one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub title: String,
    pub status: String,
    pub prompt: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Queue {
    pub todos: Vec<Todo>,
    pub tasks: Vec<Task>,
    /// The chat's helper work in worktrees. It is read from disk, so it is
    /// here after a relaunch, when there are no tasks.
    pub work: Vec<HelperWork>,
    /// Why the helper work list could not be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_error: Option<String>,
    pub jobs: Vec<Job>,
}

/// The queued jobs, kept in memory for this run of the app as the cabin
/// keeps its own. Like the cabin's, nothing adds to it yet.
static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());

fn jobs() -> std::sync::MutexGuard<'static, Vec<Job>> {
    JOBS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Put a job on the Queue.
pub fn queue_job(job: Job) {
    jobs().push(job);
}

/// The jobs waiting on the Queue, oldest first.
pub fn queued_jobs() -> Vec<Job> {
    jobs().clone()
}

/// The cabin Queue's label rule: a finished task is done, a live task whose
/// title starts with "failed" is failed, and every other live task is running.
pub fn label(title: &str, done: bool) -> Status {
    if done {
        Status::Done
    } else if title.to_ascii_lowercase().starts_with("failed") {
        Status::Failed
    } else {
        Status::Running
    }
}

/// [`label`], told apart further by how a finished task ended: a non-zero
/// exit failed, and a kill or Halt stopped it.
pub fn status(row: &TaskRow) -> Status {
    match row.state {
        TaskState::Running => label(&row.title, false),
        TaskState::Exited(0) => label(&row.title, true),
        TaskState::Exited(_) => Status::Failed,
        TaskState::Killed => Status::Stopped,
    }
}

/// The conversation's to-dos (replayed from its session file the first time)
/// and background tasks. A chat that never started a task has none, and
/// looking does not open a task hub for it.
pub fn queue(config_dir: &Path, conversation_id: &str) -> Queue {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let hub = grokhub_agent::session_is_live(conversation_id)
        .then(|| grokhub_agent::hub_for(conversation_id));
    let rows = hub.as_ref().map(|hub| hub.rows()).unwrap_or_default();
    let (records, work_error) = match grokhub_agent::work_for(conversation_id) {
        Ok(records) => (records, None),
        Err(err) => (Vec::new(), Some(err)),
    };
    let work = work_rows(&records, hub.as_ref().map(|hub| hub.nonce()), &rows);
    let now_ms = grokhub_core::now_ms();
    let needs_you = crate::helper_asks::needs_you(conversation_id);
    let working = crate::helper_asks::in_turn(conversation_id);
    let todos = grokhub_agent::todos_for(conversation_id)
        .into_iter()
        .map(|todo| {
            // A task that has not started is not an ended helper's.
            let owner = owner_of(&todo.id, &rows)
                .filter(|row| todo.status != "pending" || row.state == TaskState::Running);
            // Nor is an earlier plan's work.
            let waiting = work_of(&todo.id, &records)
                .filter(|_| todo.status != "pending")
                .map(|record| &record.state);
            Todo {
                state: todo_state(&todo.status, owner, waiting, working, needs_you),
                owner: owner.map(|row| row.id.clone()),
                id: todo.id,
                content: todo.content,
                status: todo.status,
            }
        })
        .collect();
    let tasks = rows
        .iter()
        .map(|row| Task {
            id: row.id.clone(),
            title: row.title.clone(),
            status: status(row),
            live: row.state == TaskState::Running,
            progress: hub
                .as_ref()
                .and_then(|hub| hub.progress(&row.id))
                .map(|progress| progress_of(&progress, now_ms)),
        })
        .collect();
    Queue {
        todos,
        tasks,
        work,
        work_error,
        jobs: queued_jobs(),
    }
}

/// One task's steps and result. Reading them takes nothing from the session
/// that started the task: its result still waits there.
pub fn detail(conversation_id: &str, task_id: &str) -> Result<TaskDetail, String> {
    let id = task_id.trim();
    if !grokhub_agent::session_is_live(conversation_id) {
        return Err(format!("task {id} not found"));
    }
    let peek = grokhub_agent::hub_for(conversation_id).peek(id)?;
    Ok(TaskDetail {
        steps: peek.steps,
        result: peek.result,
    })
}

/// Stops one task, or with no `task_id` halts every task, monitor and
/// subagent of the conversation. A later message in the chat can start new
/// ones; the chat's own turn keeps going (its Stop is in the chat).
pub fn stop(conversation_id: &str, task_id: Option<&str>) -> Result<(), String> {
    match task_id.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => {
            if !grokhub_agent::session_is_live(conversation_id) {
                return Err(format!("task {id} not found"));
            }
            grokhub_agent::hub_for(conversation_id).kill(id).map(|_| ())
        }
        None => {
            grokhub_agent::halt_session(conversation_id);
            Ok(())
        }
    }
}

/// Tasks → Merge: puts a helper's sealed work into the project folder, as
/// uncommitted changes. A merge that ends in a conflict or held is not an
/// error: the row says how it ended. An error is a refusal that changed
/// nothing.
pub fn merge_work(config_dir: &Path, conversation_id: &str, key: &str) -> Result<(), String> {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    grokhub_agent::merge_work(conversation_id, key.trim(), By::Person).map(|_| ())
}

/// Tasks → Finish merge (`finish`) or Put files back, for a merge that
/// stopped part way.
pub fn resume_work(
    config_dir: &Path,
    conversation_id: &str,
    key: &str,
    finish: bool,
) -> Result<(), String> {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let how = if finish {
        Resume::Finish
    } else {
        Resume::PutBack
    };
    grokhub_agent::resume_work(conversation_id, key.trim(), how).map(|_| ())
}

/// Tasks → Discard, or Delete worktree for merged work whose worktree was
/// kept.
pub fn discard_work(config_dir: &Path, conversation_id: &str, key: &str) -> Result<(), String> {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    grokhub_agent::discard_work(conversation_id, key.trim()).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-queue-{tag}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn task(id: &str, title: &str, status: Status, live: bool) -> Task {
        Task {
            id: id.into(),
            title: title.into(),
            status,
            live,
            progress: None,
        }
    }

    #[test]
    fn the_label_rule_matches_the_cabin_queue() {
        assert_eq!(label("cargo build", false), Status::Running);
        assert_eq!(label("Failed to fetch the page", false), Status::Failed);
        assert_eq!(label("failed: lint", true), Status::Done);
        let row = |state| TaskRow {
            id: "t1".into(),
            title: "cargo test".into(),
            state,
        };
        assert_eq!(status(&row(TaskState::Exited(0))), Status::Done);
        assert_eq!(status(&row(TaskState::Exited(101))), Status::Failed);
        assert_eq!(status(&row(TaskState::Killed)), Status::Stopped);
    }

    #[test]
    fn a_session_with_a_todo_call_lists_it() {
        let cfg = scratch("todo");
        let id = format!("queue-todo-{}", grokhub_core::now_ms());
        let todos = serde_json::json!({
            "todos": [
                {"id": "a", "content": "Read the spec", "status": "completed"},
                {"id": "b", "content": "Write the page", "status": "in_progress"}
            ]
        })
        .to_string();
        {
            let _config = grokhub_agent::perm::ConfigGuard::set(&cfg);
            grokhub_agent::record_turn(
                &id,
                "/home/user/my-app",
                "grok-4",
                &[grokhub_agent::InputItem::FunctionCall {
                    call_id: "call-1".into(),
                    name: "todo_write".into(),
                    arguments: todos,
                }],
                &grokhub_agent::Usage::default(),
                "API credits",
                "plan it",
            )
            .unwrap();
        }
        let queue = queue(&cfg, &id);
        assert_eq!(
            queue.todos,
            vec![
                Todo {
                    id: "a".into(),
                    content: "Read the spec".into(),
                    status: "completed".into(),
                    owner: None,
                    state: TodoState::Done,
                },
                // The session is not running, so nothing works on it now.
                Todo {
                    id: "b".into(),
                    content: "Write the page".into(),
                    status: "in_progress".into(),
                    owner: None,
                    state: TodoState::Planned,
                },
            ]
        );
        assert_eq!(queue.tasks, Vec::<Task>::new());
        assert_eq!(
            serde_json::to_value(&queue.todos[1]).unwrap(),
            serde_json::json!({
                "id": "b",
                "content": "Write the page",
                "status": "in_progress",
                "state": "planned"
            })
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    fn row(id: &str, title: &str, state: TaskState) -> TaskRow {
        TaskRow {
            id: id.into(),
            title: title.into(),
            state,
        }
    }

    #[test]
    fn a_task_reads_its_state_from_its_status_and_its_helper() {
        let running = row("s1", "task-1: Add the parser", TaskState::Running);
        let exited0 = row("s1", "task-1: Add the parser", TaskState::Exited(0));
        let exited3 = row("s1", "task-1: Add the parser", TaskState::Exited(3));
        let killed = row("s1", "task-1: Add the parser", TaskState::Killed);
        use TodoState::{Done, Failed, NeedsYou, Planned, Review, Running};
        let table = [
            ("completed", None, true, false, Done),
            ("completed", Some(&running), true, true, Done),
            ("cancelled", Some(&running), true, false, Failed),
            ("in_progress", Some(&running), true, false, Running),
            ("pending", Some(&running), true, false, Running),
            ("in_progress", Some(&running), true, true, NeedsYou),
            ("in_progress", Some(&exited0), true, false, Review),
            ("in_progress", Some(&exited0), true, true, Review),
            ("in_progress", Some(&exited3), true, false, Failed),
            ("in_progress", Some(&killed), true, false, Failed),
            ("pending", Some(&killed), true, false, Planned),
            ("pending", Some(&exited0), true, false, Planned),
            ("pending", Some(&exited3), false, false, Planned),
            ("in_progress", None, true, false, Running),
            ("in_progress", None, true, true, NeedsYou),
            ("in_progress", None, false, false, Planned),
            ("pending", None, true, false, Planned),
            ("pending", None, true, true, Planned),
        ];
        for (status, owner, live, needs_you, want) in table {
            assert_eq!(
                todo_state(status, owner, None, live, needs_you),
                want,
                "{status} {owner:?} live={live} needs_you={needs_you}"
            );
        }
    }

    /// A record as `work.json` holds it. Its worktree is `tree`.
    fn record(key: &str, description: &str, state: WorkState, created_ms: u64) -> Work {
        Work {
            key: key.into(),
            hub: "hub-a".into(),
            task: "s1".into(),
            title: description.into(),
            description: description.into(),
            project: "/home/user/my-app".into(),
            common: "/home/user/my-app/.git".into(),
            tree: format!("/home/user/.config/helper-work/chat/{key}").into(),
            admin: format!("/home/user/my-app/.git/worktrees/{key}").into(),
            pointer: String::new(),
            head: String::new(),
            base: Some("b1".into()),
            tip: Some("t1".into()),
            files: vec!["src/a.rs".into(), "src/b.rs".into(), "old.rs".into()],
            counts: Counts {
                added: 1,
                changed: 1,
                deleted: 1,
            },
            ignored: Vec::new(),
            ended: Some(Ended::Done),
            redo_of: None,
            landing: None,
            skipped: Vec::new(),
            state,
            created_ms,
            updated_ms: created_ms,
            merged_ms: None,
        }
    }

    fn held(why: Held) -> WorkState {
        WorkState::Held { why }
    }

    /// Writes a chat's records where the engine reads them.
    fn save_records(cfg: &Path, session: &str, records: &[Work]) {
        let dir = cfg.join("helper-work").join(session);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("work.json"),
            serde_json::json!({ "work": records }).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn helper_work_serialises_to_this_json() {
        let ready = record("k1-aaaa", "task-1: Add the parser", WorkState::Ready, 5);
        assert_eq!(
            serde_json::to_value(helper_work(&ready, true, true).unwrap()).unwrap(),
            serde_json::json!({
                "key": "k1-aaaa",
                "task": "s1",
                "title": "task-1: Add the parser",
                "state": "ready",
                "partial": false,
                "ended": "it finished",
                "files": ["src/a.rs", "src/b.rs", "old.rs"],
                "counts": {"added": 1, "changed": 1, "deleted": 1},
                "ignored": [],
                "conflicts": [],
                "skipped": [],
                "path": "/home/user/.config/helper-work/chat/k1-aaaa",
                "treeKept": true,
                "landing": false,
                "live": true
            })
        );

        let mut stopped = record("k2-bbbb", "task-2", held(Held::Partial), 6);
        stopped.ended = Some(Ended::Stopped);
        stopped.ignored = vec!["target/".into()];
        stopped.skipped = vec!["notes.md".into()];
        stopped.landing = Some(grokhub_agent::Landing {
            before: "c1".into(),
            merged: "c2".into(),
            paths: vec!["src/a.rs".into()],
            made: Vec::new(),
            from: Box::new(held(Held::Partial)),
            started_ms: 7,
        });
        assert_eq!(
            serde_json::to_value(helper_work(&stopped, false, false).unwrap()).unwrap(),
            serde_json::json!({
                "key": "k2-bbbb",
                "task": "s1",
                "title": "task-2",
                "state": "held",
                "partial": true,
                "ended": "it was stopped",
                "files": ["src/a.rs", "src/b.rs", "old.rs"],
                "counts": {"added": 1, "changed": 1, "deleted": 1},
                "ignored": ["target/"],
                "conflicts": [],
                "why": "partial",
                "whyText": "the helper did not finish",
                "skipped": ["notes.md"],
                "path": "/home/user/.config/helper-work/chat/k2-bbbb",
                "treeKept": false,
                "landing": true,
                "live": false
            })
        );

        let conflict = WorkState::Conflict {
            files: vec!["src/a.rs".into()],
        };
        let row = helper_work(&record("k3-cccc", "x", conflict, 8), false, true).unwrap();
        assert_eq!(row.state, WorkStatus::Conflict);
        assert_eq!(row.conflicts, vec!["src/a.rs".to_string()]);
        assert_eq!(row.why, None);
    }

    #[test]
    fn every_state_and_reason_has_its_word() {
        let state = |state: WorkState| {
            helper_work(&record("k", "x", state, 1), false, true).map(|row| row.state)
        };
        assert_eq!(state(WorkState::Opening), None);
        assert_eq!(state(WorkState::Running), Some(WorkStatus::Running));
        assert_eq!(state(WorkState::Empty), Some(WorkStatus::Empty));
        assert_eq!(state(WorkState::Merged), Some(WorkStatus::Merged));
        assert_eq!(state(WorkState::Discarded), Some(WorkStatus::Discarded));
        let states = [
            WorkStatus::Running,
            WorkStatus::Ready,
            WorkStatus::Conflict,
            WorkStatus::Held,
            WorkStatus::Merged,
            WorkStatus::Empty,
            WorkStatus::Discarded,
        ];
        assert_eq!(
            serde_json::to_string(&states).unwrap(),
            r#"["running","ready","conflict","held","merged","empty","discarded"]"#
        );

        let why = |why: Held| {
            let row = helper_work(&record("k", "x", held(why), 1), false, true).unwrap();
            (row.why.unwrap(), row.why_text.unwrap())
        };
        let apply = |rolled_back| Held::ApplyFailed {
            rolled_back,
            files: vec!["a.rs".into()],
        };
        let table = [
            (Held::Partial, "partial", "the helper did not finish"),
            (
                Held::GitStateChanged,
                "gitStateChanged",
                "the helper changed the worktree's git state",
            ),
            (
                Held::ChangedAfterSeal,
                "changedAfterSeal",
                "the worktree changed after the helper ended",
            ),
            (
                Held::IgnoredOnly,
                "ignoredOnly",
                "it wrote only ignored files",
            ),
            (
                Held::NestedRepo("vendor/lib".into()),
                "nestedRepo",
                "there is another repository at vendor/lib",
            ),
            (
                Held::SealFailed("disk full".into()),
                "sealFailed",
                "the work could not be sealed: disk full",
            ),
            (
                Held::Interrupted,
                "interrupted",
                "the app closed while the helper ran",
            ),
            (Held::Orphan, "orphan", "a folder with no record"),
            (
                Held::ProjectChanged,
                "projectChanged",
                "the project changed during the merge",
            ),
            (
                Held::Blocked(vec!["a.rs".into(), "b".into()]),
                "blocked",
                "something is in the way: a.rs, b",
            ),
            (
                apply(true),
                "rolledBack",
                "the merge could not be written; the files were put back",
            ),
            (apply(false), "applyFailed", "the merge stopped part way"),
            (
                Held::MergeFailed("bad object".into()),
                "mergeFailed",
                "git could not merge it: bad object",
            ),
        ];
        for (held, kind, text) in table {
            assert_eq!(why(held), (kind, text.to_string()));
        }
    }

    #[test]
    fn a_row_says_partial_and_how_the_helper_ended() {
        let ended = |ended: Option<Ended>, state: WorkState| {
            let mut work = record("k", "x", state, 1);
            work.ended = ended;
            let row = helper_work(&work, false, true).unwrap();
            (row.partial, row.ended)
        };
        let partial = || held(Held::Partial);
        assert_eq!(
            ended(Some(Ended::Done), WorkState::Ready),
            (false, "it finished".to_string())
        );
        assert_eq!(
            ended(Some(Ended::StepLimit), partial()),
            (true, "it ran out of steps or time".to_string())
        );
        assert_eq!(
            ended(Some(Ended::Halted), partial()),
            (true, "it was halted".to_string())
        );
        assert_eq!(
            ended(Some(Ended::Failed("no key".into())), partial()),
            (true, "it failed: no key".to_string())
        );
        assert_eq!(
            ended(None, held(Held::Interrupted)),
            (true, "the app closed while it ran".to_string())
        );
        // Partial stays on the row whatever became of the work.
        assert_eq!(
            ended(Some(Ended::Stopped), WorkState::Merged),
            (true, "it was stopped".to_string())
        );
        // A helper that still runs has not ended at all.
        assert!(!ended(None, WorkState::Running).0);
    }

    #[test]
    fn work_that_waits_for_the_person_is_not_merged() {
        let exited0 = row("s1", "task-1: Add the parser", TaskState::Exited(0));
        let running = row("s1", "task-1: Add the parser", TaskState::Running);
        let conflict = WorkState::Conflict {
            files: vec!["a.rs".into()],
        };
        let partial = held(Held::Partial);
        use TodoState::{Done, NotMerged, Review, Running};
        let table = [
            (
                "in_progress",
                Some(&exited0),
                Some(&WorkState::Ready),
                NotMerged,
            ),
            ("in_progress", Some(&exited0), Some(&conflict), NotMerged),
            ("in_progress", Some(&exited0), Some(&partial), NotMerged),
            // The lead closed the task, but its files are not in the project.
            (
                "completed",
                Some(&exited0),
                Some(&WorkState::Ready),
                NotMerged,
            ),
            ("completed", None, Some(&conflict), NotMerged),
            ("completed", None, Some(&partial), NotMerged),
            ("cancelled", None, Some(&WorkState::Ready), NotMerged),
            // After a relaunch there are no helper rows.
            ("in_progress", None, Some(&WorkState::Ready), NotMerged),
            // Work that is over, or still being done, says nothing.
            ("completed", None, Some(&WorkState::Merged), Done),
            ("completed", None, Some(&WorkState::Discarded), Done),
            ("completed", None, Some(&WorkState::Empty), Done),
            (
                "in_progress",
                Some(&exited0),
                Some(&WorkState::Merged),
                Review,
            ),
            (
                "in_progress",
                Some(&running),
                Some(&WorkState::Running),
                Running,
            ),
            (
                "in_progress",
                Some(&running),
                Some(&WorkState::Opening),
                Running,
            ),
        ];
        for (status, owner, work, want) in table {
            assert_eq!(
                todo_state(status, owner, work, true, false),
                want,
                "{status} {owner:?} {work:?}"
            );
        }
    }

    #[test]
    fn the_newest_work_whose_helper_names_the_task_is_its_work() {
        let records = [
            record("k1", "task-1: Add the parser", WorkState::Ready, 30),
            record("k2", "task-1: Add the parser", WorkState::Merged, 40),
            record("k3", "task-10: Other", WorkState::Ready, 50),
            record("k4", "task-3", WorkState::Ready, 10),
            record("k5", "", WorkState::Ready, 60),
        ];
        let key = |id: &str| work_of(id, &records).map(|work| work.key.as_str());
        assert_eq!(key("task-1"), Some("k2"));
        assert_eq!(key("task-10"), Some("k3"));
        assert_eq!(key("task-3"), Some("k4"));
        assert_eq!(key("task-2"), None);
        assert_eq!(key(""), None);
    }

    #[test]
    fn only_the_record_of_the_live_hub_is_live() {
        let rows = [row("s1", "task-1: Add the parser", TaskState::Exited(0))];
        let mut other = record("k1", "task-1: Add the parser", WorkState::Ready, 1);
        other.hub = "hub-b".into();
        let mine = record("k2", "task-1: Add the parser", WorkState::Ready, 2);
        let mut no_row = record("k3", "task-2", WorkState::Ready, 3);
        no_row.task = "s2".into();
        let records = [other, mine, no_row];
        let live = |nonce: Option<&str>, rows: &[TaskRow]| -> Vec<(String, bool)> {
            work_rows(&records, nonce, rows)
                .into_iter()
                .map(|work| (work.key, work.live))
                .collect()
        };
        assert_eq!(
            live(Some("hub-a"), &rows),
            vec![
                ("k1".to_string(), false),
                ("k2".to_string(), true),
                ("k3".to_string(), false),
            ]
        );
        // With no live hub, as after a relaunch, nothing is live.
        assert_eq!(
            live(None, &[]),
            vec![
                ("k1".to_string(), false),
                ("k2".to_string(), false),
                ("k3".to_string(), false),
            ]
        );
    }

    #[test]
    fn work_that_is_over_is_listed_only_for_this_launch_or_a_kept_worktree() {
        let cfg = scratch("listed");
        let kept = cfg.join("kept");
        std::fs::create_dir_all(&kept).unwrap();
        let rows = [row("s1", "x", TaskState::Exited(0))];
        let at = |key: &str, state: WorkState, tree: &Path| {
            let mut work = record(key, "x", state, 1);
            work.tree = tree.to_path_buf();
            work
        };
        let gone = cfg.join("gone");
        let records = [
            at("ready", WorkState::Ready, &gone),
            at("held", held(Held::SealFailed("gone".into())), &gone),
            at("merged", WorkState::Merged, &gone),
            at("merged-kept", WorkState::Merged, &kept),
            at("empty", WorkState::Empty, &gone),
            at("discarded", WorkState::Discarded, &gone),
            at("opening", WorkState::Opening, &kept),
            at("running", WorkState::Running, &kept),
        ];
        let keys = |nonce: Option<&str>| -> Vec<(String, bool)> {
            work_rows(&records, nonce, &rows)
                .into_iter()
                .map(|work| (work.key, work.tree_kept))
                .collect()
        };
        assert_eq!(
            keys(None),
            vec![
                ("ready".to_string(), false),
                ("held".to_string(), false),
                ("merged-kept".to_string(), true),
                ("running".to_string(), true),
            ]
        );
        assert_eq!(
            keys(Some("hub-a")),
            vec![
                ("ready".to_string(), false),
                ("held".to_string(), false),
                ("merged".to_string(), false),
                ("merged-kept".to_string(), true),
                ("empty".to_string(), false),
                ("discarded".to_string(), false),
                ("running".to_string(), true),
            ]
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_redo_names_the_work_it_redid_and_that_work_says_a_newer_helper_redid_it() {
        let conflict = || WorkState::Conflict {
            files: vec!["src/a.rs".into()],
        };
        let redo = |key: &str, of: &str, state: WorkState| {
            let mut work = record(key, "Fix the parser again", state, 2);
            work.redo_of = Some(of.into());
            work
        };
        let marks = |records: &[Work]| -> Vec<(String, Option<String>, bool)> {
            work_rows(
                records,
                Some("hub-a"),
                &[row("s1", "x", TaskState::Exited(0))],
            )
            .into_iter()
            .map(|work| (work.key, work.redo_of, work.redone))
            .collect()
        };
        let old = record("k1", "Fix the parser", conflict(), 1);
        let again = Some("Fix the parser".to_string());

        // A redo that finished and is sealed marks the work it redid, merged
        // or not, and so does a stopped one the person merged anyway.
        let stopped_and_merged = {
            let mut work = redo("k2", "k1", WorkState::Merged);
            work.ended = Some(Ended::Stopped);
            work
        };
        for newer in [
            redo("k2", "k1", WorkState::Ready),
            redo("k2", "k1", conflict()),
            redo("k2", "k1", held(Held::ChangedAfterSeal)),
            redo("k2", "k1", WorkState::Merged),
            stopped_and_merged,
        ] {
            assert_eq!(
                marks(&[old.clone(), newer]),
                [
                    ("k1".to_string(), None, true),
                    ("k2".to_string(), again.clone(), false),
                ]
            );
        }
        // One that still runs, changed nothing or was discarded redid
        // nothing. Nor did one that was stopped part way, or one the app
        // closed under or whose seal failed: nothing of those is sealed.
        let stopped = {
            let mut work = redo("k2", "k1", held(Held::Partial));
            work.ended = Some(Ended::Stopped);
            work
        };
        let unsealed = |why: Held| {
            let mut work = redo("k2", "k1", held(why));
            work.tip = None;
            work.ended = None;
            work
        };
        for newer in [
            redo("k2", "k1", WorkState::Running),
            redo("k2", "k1", WorkState::Empty),
            redo("k2", "k1", WorkState::Discarded),
            stopped,
            unsealed(Held::Interrupted),
            unsealed(Held::SealFailed("disk full".into())),
        ] {
            assert_eq!(
                marks(&[old.clone(), newer]),
                [
                    ("k1".to_string(), None, false),
                    ("k2".to_string(), again.clone(), false),
                ]
            );
        }
        // Old work that is over carries no mark, and a redo whose old record
        // is gone names it by its key.
        let mut merged = old.clone();
        merged.state = WorkState::Merged;
        assert_eq!(
            marks(&[merged, redo("k2", "k1", WorkState::Ready)]),
            [
                ("k1".to_string(), None, false),
                ("k2".to_string(), again, false),
            ]
        );
        assert_eq!(
            marks(&[redo("k2", "k1", WorkState::Ready)]),
            [("k2".to_string(), Some("k1".to_string()), false)]
        );

        let mut row = helper_work(&old, false, true).unwrap();
        row.redo_of = Some("An older fix".into());
        row.redone = true;
        let json = serde_json::to_value(row).unwrap();
        assert_eq!(json["redoOf"], "An older fix");
        assert_eq!(json["redone"], true);
    }

    /// A session file whose one turn wrote these to-dos.
    fn save_todos(cfg: &Path, id: &str, todos: serde_json::Value) {
        let _config = grokhub_agent::perm::ConfigGuard::set(cfg);
        grokhub_agent::record_turn(
            id,
            "/home/user/my-app",
            "grok-4",
            &[grokhub_agent::InputItem::FunctionCall {
                call_id: "call-1".into(),
                name: "todo_write".into(),
                arguments: serde_json::json!({ "todos": todos }).to_string(),
            }],
            &grokhub_agent::Usage::default(),
            "API credits",
            "go",
        )
        .unwrap();
    }

    #[test]
    fn a_relaunched_chat_lists_its_work_and_marks_its_tasks_not_merged() {
        let cfg = scratch("work");
        let id = format!("queue-work-{}", grokhub_core::now_ms());
        save_todos(
            &cfg,
            &id,
            serde_json::json!([
                {"id": "task-1", "content": "Add the parser", "status": "completed"},
                {"id": "task-2", "content": "Add the page", "status": "completed"},
                {"id": "task-3", "content": "Write the docs", "status": "pending"},
                {"id": "task-4", "content": "Fix the test", "status": "in_progress"}
            ]),
        );
        let mut stopped = record("k4-dddd", "task-4 Fix the test", held(Held::Partial), 4);
        stopped.ended = Some(Ended::Stopped);
        save_records(
            &cfg,
            &id,
            &[
                record("k1-aaaa", "task-1: Add the parser", WorkState::Ready, 1),
                record("k2-bbbb", "task-2: Add the page", WorkState::Merged, 2),
                // An earlier plan's work does not speak for a task not started.
                record("k3-cccc", "task-3: Old docs", WorkState::Ready, 3),
                stopped,
            ],
        );
        let queue = queue(&cfg, &id);
        assert!(!grokhub_agent::session_is_live(&id), "looking opens no hub");
        assert_eq!(queue.tasks, Vec::<Task>::new());
        assert_eq!(queue.work_error, None);
        let states: Vec<(&str, TodoState)> = queue
            .todos
            .iter()
            .map(|todo| (todo.id.as_str(), todo.state))
            .collect();
        assert_eq!(
            states,
            vec![
                ("task-1", TodoState::NotMerged),
                ("task-2", TodoState::Done),
                ("task-3", TodoState::Planned),
                ("task-4", TodoState::NotMerged),
            ]
        );
        assert_eq!(
            serde_json::to_value(&queue.todos[0]).unwrap(),
            serde_json::json!({
                "id": "task-1",
                "content": "Add the parser",
                "status": "completed",
                "state": "notMerged"
            })
        );
        let rows: Vec<(&str, WorkStatus, bool, bool)> = queue
            .work
            .iter()
            .map(|work| (work.key.as_str(), work.state, work.partial, work.live))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("k1-aaaa", WorkStatus::Ready, false, false),
                ("k3-cccc", WorkStatus::Ready, false, false),
                ("k4-dddd", WorkStatus::Held, true, false),
            ]
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn an_unreadable_work_list_is_an_error_line_and_no_rows() {
        let cfg = scratch("work-error");
        let id = format!("queue-work-error-{}", grokhub_core::now_ms());
        save_todos(
            &cfg,
            &id,
            serde_json::json!([{"id": "task-1", "content": "Add the parser", "status": "completed"}]),
        );
        let dir = cfg.join("helper-work").join(&id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("work.json"), r#"{"work": [{"key": "#).unwrap();
        let queue = queue(&cfg, &id);
        assert_eq!(queue.work, Vec::<HelperWork>::new());
        assert_eq!(
            queue.work_error,
            Some(format!(
                "{}: EOF while parsing a value at line 1 column 18",
                dir.join("work.json").display()
            ))
        );
        // The to-dos still show, as their status says.
        assert_eq!(queue.todos[0].state, TodoState::Done);
        let value = serde_json::to_value(&queue).unwrap();
        assert_eq!(value["work"], serde_json::json!([]));
        assert_eq!(value["workError"], serde_json::json!(queue.work_error));
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_queue_with_no_work_has_an_empty_list_and_no_error_key() {
        let cfg = scratch("no-work");
        let id = format!("queue-no-work-{}", grokhub_core::now_ms());
        assert_eq!(
            serde_json::to_string(&queue(&cfg, &id)).unwrap(),
            r#"{"todos":[],"tasks":[],"work":[],"jobs":[]}"#
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_click_on_work_this_chat_does_not_have_is_refused() {
        let cfg = scratch("refused");
        let id = format!("queue-refused-{}", grokhub_core::now_ms());
        assert_eq!(
            merge_work(&cfg, &id, " zzzz-0000 "),
            Err("there is no helper work zzzz-0000 in this chat".into())
        );
        assert_eq!(
            resume_work(&cfg, &id, "zzzz-0000", true),
            Err("there is no helper work zzzz-0000 in this chat".into())
        );
        assert_eq!(
            resume_work(&cfg, &id, "zzzz-0000", false),
            Err("there is no helper work zzzz-0000 in this chat".into())
        );
        assert_eq!(
            discard_work(&cfg, &id, "zzzz-0000"),
            Err("there is no helper work zzzz-0000 in this chat".into())
        );
        assert_eq!(
            merge_work(&cfg, &id, "../x"),
            Err("not a helper work key: ../x".into())
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_helper_owns_the_task_its_title_names() {
        let rows = [
            row("s2", "task-1: Add the parser", TaskState::Killed),
            row("t3", "task-1: a command", TaskState::Running),
            row("s4", "task-10: Other", TaskState::Running),
            row("s5", "task-1: Add the parser", TaskState::Running),
            row("s6", "task-3", TaskState::Exited(0)),
            row("s7", "task-4 Fix the test", TaskState::Running),
        ];
        let owner = |id: &str| owner_of(id, &rows).map(|row| row.id.as_str());
        assert_eq!(owner("task-1"), Some("s5"));
        assert_eq!(owner("task-10"), Some("s4"));
        assert_eq!(owner("task-2"), None);
        assert_eq!(owner("task-3"), Some("s6"));
        assert_eq!(owner("task-4"), Some("s7"));
        assert_eq!(owner(""), None);
    }

    #[test]
    fn a_live_session_shows_each_tasks_helper_and_state() {
        let cfg = scratch("lead-tasks");
        let id = format!("queue-lead-{}", grokhub_core::now_ms());
        let todos = serde_json::json!({
            "todos": [
                {"id": "task-1", "content": "Add the parser", "status": "in_progress"},
                {"id": "task-2", "content": "Add the page", "status": "in_progress"},
                {"id": "task-3", "content": "Write the docs", "status": "pending"}
            ]
        })
        .to_string();
        {
            let _config = grokhub_agent::perm::ConfigGuard::set(&cfg);
            grokhub_agent::record_turn(
                &id,
                "/home/user/my-app",
                "grok-4",
                &[grokhub_agent::InputItem::FunctionCall {
                    call_id: "call-1".into(),
                    name: "todo_write".into(),
                    arguments: todos,
                }],
                &grokhub_agent::Usage::default(),
                "API credits",
                "go",
            )
            .unwrap();
        }
        let hub = grokhub_agent::hub_for(&id);
        hub.register_agent(None, "task-1: Add the parser", Arc::new(|| {}))
            .unwrap();
        hub.register_agent(None, "task-2: Add the page", Arc::new(|| {}))
            .unwrap();
        hub.finish_agent("s2", "done");
        let states = |queue: Queue| -> Vec<(String, Option<String>, TodoState)> {
            queue
                .todos
                .into_iter()
                .map(|todo| (todo.id, todo.owner, todo.state))
                .collect()
        };
        assert_eq!(
            states(queue(&cfg, &id)),
            vec![
                ("task-1".into(), Some("s1".into()), TodoState::Running),
                ("task-2".into(), Some("s2".into()), TodoState::Review),
                ("task-3".into(), None, TodoState::Planned),
            ]
        );
        // The session waits on the person: its running task says so.
        crate::helper_asks::turn_ask(
            &id,
            &crate::agent::UiEvent::Permission {
                id: "p1".into(),
                title: "write".into(),
                action: "parser.rs".into(),
                reason: String::new(),
            },
        );
        let queue = queue(&cfg, &id);
        assert_eq!(
            serde_json::to_value(&queue.todos[0]).unwrap(),
            serde_json::json!({
                "id": "task-1",
                "content": "Add the parser",
                "status": "in_progress",
                "owner": "s1",
                "state": "needsYou"
            })
        );
        assert_eq!(queue.todos[1].state, TodoState::Review);
        crate::helper_asks::turn_ended(&id);
        grokhub_agent::forget_session(&id);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn tasks_show_running_or_failed_and_stop_halts_them() {
        let cfg = scratch("tasks");
        let id = format!("queue-tasks-{}", grokhub_core::now_ms());
        assert_eq!(queue(&cfg, &id).tasks, Vec::<Task>::new());
        assert!(!grokhub_agent::session_is_live(&id), "looking opens no hub");

        let hub = grokhub_agent::hub_for(&id);
        hub.register_agent(None, "Index the repo", Arc::new(|| {}))
            .unwrap();
        hub.register_agent(None, "failed to fetch the docs", Arc::new(|| {}))
            .unwrap();
        hub.register_agent(None, "Sum the logs", Arc::new(|| {}))
            .unwrap();
        hub.finish_agent("s3", "42");
        assert_eq!(
            queue(&cfg, &id).tasks,
            vec![
                task("s1", "Index the repo", Status::Running, true),
                task("s2", "failed to fetch the docs", Status::Failed, true),
                task("s3", "Sum the logs", Status::Done, false),
            ]
        );

        stop(&id, Some("s1")).unwrap();
        assert_eq!(
            queue(&cfg, &id).tasks[0],
            task("s1", "Index the repo", Status::Stopped, false)
        );
        assert_eq!(
            stop(&id, Some("s9")),
            Err("No task with id s9 was started in this chat. The ids started in this chat are: s1, s2, s3.".into())
        );

        stop(&id, None).unwrap();
        assert!(hub.is_halted());
        assert_eq!(
            queue(&cfg, &id).tasks,
            vec![
                task("s1", "Index the repo", Status::Stopped, false),
                task("s2", "failed to fetch the docs", Status::Stopped, false),
                task("s3", "Sum the logs", Status::Done, false),
            ]
        );
        grokhub_agent::forget_session(&id);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_helpers_clock_runs_until_it_ends() {
        let running = TaskProgress {
            started_ms: 1_000,
            ended_ms: None,
            steps: 2,
            last_step: "shell".into(),
            tokens: 750,
        };
        assert_eq!(
            progress_of(&running, 43_500),
            Progress {
                steps: 2,
                last_step: "shell".into(),
                tokens: 750,
                seconds: 42,
            }
        );
        let ended = TaskProgress {
            ended_ms: Some(11_000),
            ..running.clone()
        };
        assert_eq!(progress_of(&ended, 43_500).seconds, 10);
        // A clock that reads before the start gives 0, not a wrapped number.
        assert_eq!(progress_of(&running, 500).seconds, 0);
    }

    #[test]
    fn a_task_carries_progress_only_when_it_has_some() {
        let mut plain = task("t1", "x", Status::Running, true);
        assert_eq!(
            serde_json::to_string(&plain).unwrap(),
            r#"{"id":"t1","title":"x","status":"running","live":true}"#
        );
        plain.progress = Some(Progress {
            steps: 2,
            last_step: "shell".into(),
            tokens: 750,
            seconds: 42,
        });
        assert_eq!(
            serde_json::to_string(&plain).unwrap(),
            r#"{"id":"t1","title":"x","status":"running","live":true,"progress":{"steps":2,"lastStep":"shell","tokens":750,"seconds":42}}"#
        );
    }

    #[test]
    fn details_for_a_chat_with_no_tasks_names_the_task() {
        assert_eq!(
            detail("no-such-chat", "s1"),
            Err("task s1 not found".into())
        );
    }

    #[test]
    fn details_show_a_helpers_result_and_leave_it_for_the_session() {
        let id = format!("queue-detail-{}", grokhub_core::now_ms());
        let hub = grokhub_agent::hub_for(&id);
        hub.register_agent(None, "task-1: Add the parser", Arc::new(|| {}))
            .unwrap();
        assert_eq!(
            detail(&id, "s1"),
            Ok(TaskDetail {
                steps: Vec::new(),
                result: String::new(),
            })
        );
        hub.finish_agent("s1", "the answer");
        let want = TaskDetail {
            steps: Vec::new(),
            result: "the answer".into(),
        };
        assert_eq!(detail(&id, " s1 "), Ok(want.clone()));
        assert_eq!(detail(&id, "s1"), Ok(want));
        assert_eq!(
            serde_json::to_string(&detail(&id, "s1").unwrap()).unwrap(),
            r#"{"steps":[],"result":"the answer"}"#
        );
        // The session that started the helper still has its result to read.
        assert_eq!(hub.drain_notices().len(), 1);
        assert_eq!(
            detail(&id, "s9"),
            Err("No task with id s9 was started in this chat. The ids started in this chat are: s1.".into())
        );
        grokhub_agent::forget_session(&id);
    }

    #[test]
    fn a_stop_in_a_chat_with_no_tasks_names_the_task() {
        let id = format!("queue-none-{}", grokhub_core::now_ms());
        assert_eq!(stop(&id, Some("t1")), Err("task t1 not found".into()));
    }

    #[test]
    fn a_queued_job_shows_with_its_prompt_and_thread() {
        let cfg = scratch("jobs");
        let id = format!("queue-jobs-{}", grokhub_core::now_ms());
        let job = Job {
            title: "Summarize the logs".into(),
            status: "queued".into(),
            prompt: "Summarize today's build logs".into(),
            thread_id: "shell-t7".into(),
        };
        queue_job(job.clone());
        let page = queue(&cfg, &id);
        assert!(page.jobs.contains(&job), "{:?}", page.jobs);
        let wire = serde_json::to_value(&page).unwrap();
        assert!(wire["jobs"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!({
                "title": "Summarize the logs",
                "status": "queued",
                "prompt": "Summarize today's build logs",
                "threadId": "shell-t7"
            })));
        let _ = std::fs::remove_dir_all(&cfg);
    }
}
