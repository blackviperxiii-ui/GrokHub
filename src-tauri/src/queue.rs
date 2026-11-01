//! The Agents page, the cabin's Queue: the open chat's to-dos and its
//! background commands, subagents and monitors, each with a Stop.

use std::path::Path;

use grokhub_agent::{TaskRow, TaskState};
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
}

/// One `todo_write` row: "pending", "in_progress", "completed" or "cancelled".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: String,
    pub content: String,
    pub status: String,
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
    let todos = grokhub_agent::todos_for(conversation_id)
        .into_iter()
        .map(|todo| Todo {
            id: todo.id,
            content: todo.content,
            status: todo.status,
        })
        .collect();
    let tasks = if grokhub_agent::session_is_live(conversation_id) {
        grokhub_agent::hub_for(conversation_id)
            .rows()
            .iter()
            .map(|row| Task {
                id: row.id.clone(),
                title: row.title.clone(),
                status: status(row),
                live: row.state == TaskState::Running,
            })
            .collect()
    } else {
        Vec::new()
    };
    Queue { todos, tasks }
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
                },
                Todo {
                    id: "b".into(),
                    content: "Write the page".into(),
                    status: "in_progress".into(),
                },
            ]
        );
        assert_eq!(queue.tasks, Vec::<Task>::new());
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
            Err("task s9 not found. Known ids: s1, s2, s3".into())
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
    fn a_stop_in_a_chat_with_no_tasks_names_the_task() {
        let id = format!("queue-none-{}", grokhub_core::now_ms());
        assert_eq!(stop(&id, Some("t1")), Err("task t1 not found".into()));
    }
}
