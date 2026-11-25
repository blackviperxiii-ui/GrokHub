//! One line per finished agent message in `{config}/agent-tasks.jsonl`: who
//! asked which agent what, and what came back. The desktop app's Team page
//! reads it. Text is clipped and secrets redacted, as in a routine's run log.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::a2a::TaskState;

pub const TASK_LOG: &str = "agent-tasks.jsonl";
/// Lines before the log rotates to `agent-tasks.jsonl.1`.
const CAP: usize = 1_000;
const MAX_ASKED: usize = 2_000;
const MAX_REPLY: usize = 4_000;

/// Who sent the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The person, from a chat or the app.
    Person,
    /// One of the person's agents, by id.
    Agent(String),
    /// A paired computer, by its device id.
    Peer(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub from: Origin,
    /// The agent that was asked.
    pub to: String,
    pub state: TaskState,
    pub asked: String,
    /// The agent's answer, or why there is none.
    pub reply: String,
    pub started_ms: u64,
    pub ended_ms: u64,
}

impl TaskRecord {
    /// A record with a fresh id, its text clipped and secrets redacted.
    pub fn new(
        from: Origin,
        to: &str,
        state: TaskState,
        asked: &str,
        reply: &str,
        started_ms: u64,
        ended_ms: u64,
    ) -> TaskRecord {
        TaskRecord {
            id: crate::uid("task"),
            from,
            to: to.to_string(),
            state,
            asked: clip(&crate::redact_secrets(asked), MAX_ASKED),
            reply: clip(&crate::redact_secrets(reply), MAX_REPLY),
            started_ms,
            ended_ms,
        }
    }
}

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", text[..at].trim_end()),
        None => text.to_string(),
    }
}

fn paths(config: &Path) -> [PathBuf; 2] {
    [config.join(format!("{TASK_LOG}.1")), config.join(TASK_LOG)]
}

/// Adds one line. At [`CAP`] lines the log moves to `.1`, replacing an older
/// one, and a new log starts.
pub fn append(config: &Path, record: &TaskRecord) -> Result<(), String> {
    let [rotated, path] = paths(config);
    fs::create_dir_all(config).map_err(|e| e.to_string())?;
    let lines = match fs::read_to_string(&path) {
        Ok(text) => text.lines().filter(|l| !l.trim().is_empty()).count(),
        Err(e) if e.kind() == ErrorKind::NotFound => 0,
        Err(e) => return Err(e.to_string()),
    };
    if lines >= CAP {
        let _ = fs::remove_file(&rotated);
        fs::rename(&path, &rotated).map_err(|e| e.to_string())?;
    }
    let mut line = serde_json::to_string(record).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

/// Up to `limit` records, newest first. Lines that don't read are skipped.
pub fn recent(config: &Path, limit: usize) -> Vec<TaskRecord> {
    let mut all: Vec<TaskRecord> = paths(config)
        .iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .flat_map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect::<Vec<TaskRecord>>()
        })
        .collect();
    all.reverse();
    all.truncate(limit);
    all
}

/// Removes the log and its rotated copy.
pub fn clear(config: &Path) -> Result<(), String> {
    for path in paths(config) {
        match fs::remove_file(&path) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grokhub-agent-tasks-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record(n: u64) -> TaskRecord {
        TaskRecord {
            id: format!("task-{n}"),
            from: Origin::Person,
            to: "writer".into(),
            state: TaskState::Completed,
            asked: format!("ask {n}"),
            reply: "done".into(),
            started_ms: n,
            ended_ms: n + 1,
        }
    }

    #[test]
    fn a_record_reads_back_newest_first_in_its_wire_shape() {
        let dir = scratch("wire");
        append(dir.as_path(), &record(1)).unwrap();
        let mut second = record(2);
        second.from = Origin::Agent("lead".into());
        second.state = TaskState::Rejected;
        append(dir.as_path(), &second).unwrap();
        let line = fs::read_to_string(dir.as_path().join(TASK_LOG)).unwrap();
        assert_eq!(
            line.lines().nth(1).unwrap(),
            r#"{"id":"task-2","from":{"agent":"lead"},"to":"writer","state":"TASK_STATE_REJECTED","asked":"ask 2","reply":"done","started_ms":2,"ended_ms":3}"#
        );
        assert!(
            line.starts_with(r#"{"id":"task-1","from":"person","#),
            "{line}"
        );
        let mut peer = record(3);
        peer.from = Origin::Peer("laptop".into());
        assert_eq!(
            serde_json::to_value(&peer.from).unwrap(),
            serde_json::json!({"peer": "laptop"})
        );
        let ids: Vec<String> = recent(dir.as_path(), 10)
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, ["task-2", "task-1"]);
        assert_eq!(recent(dir.as_path(), 1)[0].id, "task-2");
    }

    #[test]
    fn new_clips_and_redacts_and_mints_an_id() {
        let long = "a".repeat(MAX_ASKED + 10);
        let r = TaskRecord::new(
            Origin::Person,
            "writer",
            TaskState::Completed,
            &long,
            "use key sk-abcdefghijklmnopqrstuv now",
            5,
            9,
        );
        assert_eq!(r.asked.chars().count(), MAX_ASKED + 1);
        assert!(r.asked.ends_with('…'));
        assert!(
            !r.reply.contains("sk-abcdefghijklmnopqrstuv"),
            "{}",
            r.reply
        );
        assert!(r.reply.starts_with("use key "), "{}", r.reply);
        assert!(
            r.id.starts_with("task-") && r.id.len() == "task-".len() + 16,
            "{}",
            r.id
        );
        assert_ne!(
            r.id,
            TaskRecord::new(Origin::Person, "w", TaskState::Failed, "", "", 0, 0).id
        );
    }

    #[test]
    fn the_log_rotates_at_the_cap_and_still_reads_both_files() {
        let dir = scratch("rotate");
        for n in 0..CAP as u64 + 1 {
            append(dir.as_path(), &record(n)).unwrap();
        }
        let rotated = fs::read_to_string(dir.as_path().join(format!("{TASK_LOG}.1"))).unwrap();
        assert_eq!(rotated.lines().count(), CAP);
        let active = fs::read_to_string(dir.as_path().join(TASK_LOG)).unwrap();
        assert_eq!(active.lines().count(), 1);
        let newest = recent(dir.as_path(), 2);
        assert_eq!(
            newest.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            [format!("task-{CAP}"), format!("task-{}", CAP - 1)]
        );
        assert_eq!(recent(dir.as_path(), usize::MAX).len(), CAP + 1);
    }

    #[test]
    fn a_bad_line_is_skipped_and_clear_removes_both_files() {
        let dir = scratch("clear");
        assert_eq!(recent(dir.as_path(), 5), Vec::new());
        append(dir.as_path(), &record(1)).unwrap();
        let path = dir.as_path().join(TASK_LOG);
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str("not json\n");
        fs::write(&path, text).unwrap();
        fs::write(dir.as_path().join(format!("{TASK_LOG}.1")), "").unwrap();
        assert_eq!(recent(dir.as_path(), 5).len(), 1);
        clear(dir.as_path()).unwrap();
        assert!(!path.exists());
        assert!(!dir.as_path().join(format!("{TASK_LOG}.1")).exists());
        clear(dir.as_path()).unwrap();
    }
}
