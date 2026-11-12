//! The Agents page, the cabin's Queue: the open chat's to-dos and its
//! background commands, subagents and monitors, each with a Stop.

use std::path::Path;

use grokhub_agent::{TaskProgress, TaskRow, TaskState};
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
    Done,
    Failed,
}

/// The helper a to-do was handed to: the last helper row (`s…`) whose title
/// is the to-do's id, or starts with it and then `:` or a space. The last
/// one wins, so a retry replaces a stopped helper.
fn owner_of<'a>(todo_id: &str, rows: &'a [TaskRow]) -> Option<&'a TaskRow> {
    if todo_id.is_empty() {
        return None;
    }
    rows.iter().rev().find(|row| {
        row.id.starts_with('s')
            && row
                .title
                .strip_prefix(todo_id)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', ' ']))
    })
}

/// A to-do's state. `working` is whether a turn of the session runs now, and
/// `needs_you` whether the session waits on the person. A helper that already
/// ended does not speak for a task that has not started: its id may be an
/// earlier plan's.
fn todo_state(status: &str, owner: Option<&TaskRow>, working: bool, needs_you: bool) -> TodoState {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Queue {
    pub todos: Vec<Todo>,
    pub tasks: Vec<Task>,
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
    let now_ms = grokhub_core::now_ms();
    let needs_you = crate::helper_asks::needs_you(conversation_id);
    let working = crate::helper_asks::in_turn(conversation_id);
    let todos = grokhub_agent::todos_for(conversation_id)
        .into_iter()
        .map(|todo| {
            // A task that has not started is not an ended helper's.
            let owner = owner_of(&todo.id, &rows)
                .filter(|row| todo.status != "pending" || row.state == TaskState::Running);
            Todo {
                state: todo_state(&todo.status, owner, working, needs_you),
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
    Queue { todos, tasks }
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
                todo_state(status, owner, live, needs_you),
                want,
                "{status} {owner:?} live={live} needs_you={needs_you}"
            );
        }
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
}
