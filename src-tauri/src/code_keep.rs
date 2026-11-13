//! Where a Code tab session left off, so a restart can resume it.
//!
//! The file sits next to the session's plan (`<config>/sessions/<id>/checkpoint.json`).
//! The goal is the plan's first line. Next is the first open to-do, else the
//! first open checkbox, else the first numbered step. A turn that errors
//! records the error as the blocker; a clean turn clears it.

use std::path::Path;

use grokhub_core::work_item::{
    clip_line, CHECKPOINT_BLOCKER_MAX, CHECKPOINT_GOAL_MAX, CHECKPOINT_NEXT_MAX,
};
use serde::{Deserialize, Serialize};

use crate::code_sessions::{self, CHECKPOINT_FILE};
use crate::lead;

/// Shown when a turn ends with the agent's error stop and no message of its own.
pub const TURN_ERROR: &str = "The last turn ended in an error.";

/// Goal, next step and blocker, clipped the same way a work card's checkpoint is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOff {
    pub goal: String,
    pub next: String,
    pub blocker: String,
}

impl LeftOff {
    fn is_empty(&self) -> bool {
        self.goal.is_empty() && self.next.is_empty() && self.blocker.is_empty()
    }
}

/// The prompt Resume sends. The three lines are fixed so a test can match them.
pub fn resume_prompt(left: &LeftOff) -> String {
    let mut lines = vec!["Continue this Code tab session where it left off.".to_string()];
    if !left.goal.is_empty() {
        lines.push(format!("Goal: {}", left.goal));
    }
    if !left.next.is_empty() {
        lines.push(format!("Next: {}", left.next));
    }
    if !left.blocker.is_empty() {
        lines.push(format!("Blocker: {}", left.blocker));
    }
    lines.join("\n")
}

fn closed(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "completed" | "complete" | "done" | "cancelled" | "canceled" | "skipped"
    )
}

fn numbered(line: &str) -> Option<&str> {
    let rest = line.trim();
    let bytes = rest.as_bytes();
    if bytes.first().is_none_or(|b| !b.is_ascii_digit()) {
        return None;
    }
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if bytes.get(i) != Some(&b'.') || bytes.get(i + 1) != Some(&b' ') {
        return None;
    }
    let text = rest[i + 2..].trim();
    (!text.is_empty()).then_some(text)
}

fn plan_next(plan: &str, goal: &str) -> String {
    for line in plan.lines() {
        if let Some(rest) = line.trim().strip_prefix("- [ ] ") {
            let text = clip_line(rest, CHECKPOINT_NEXT_MAX);
            if !text.is_empty() {
                return text;
            }
        }
    }
    for line in plan.lines() {
        if let Some(text) = numbered(line) {
            let text = clip_line(text, CHECKPOINT_NEXT_MAX);
            if !text.is_empty() && text != goal {
                return text;
            }
        }
    }
    String::new()
}

/// `todos` is `(status, content)` in list order. Open rows come before the plan.
pub fn left_off(plan: &str, todos: &[(&str, &str)], blocker: &str) -> Option<LeftOff> {
    let goal = plan
        .lines()
        .find_map(|line| {
            let text = clip_line(line, CHECKPOINT_GOAL_MAX);
            (!text.is_empty()).then_some(text)
        })
        .unwrap_or_default();
    let next = todos
        .iter()
        .find(|(status, content)| !closed(status) && !content.trim().is_empty())
        .map(|(_, content)| clip_line(content, CHECKPOINT_NEXT_MAX))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| plan_next(plan, &goal));
    let left = LeftOff {
        goal,
        next,
        blocker: clip_line(
            &grokhub_core::redact::redact_secrets(blocker),
            CHECKPOINT_BLOCKER_MAX,
        ),
    };
    (!left.is_empty()).then_some(left)
}

/// Reads the saved checkpoint. A missing or unreadable file is `None`.
pub fn read(config_dir: &Path, conversation_id: &str) -> Option<LeftOff> {
    let dir = code_sessions::session_dir(config_dir, conversation_id)?;
    let text = std::fs::read_to_string(dir.join(CHECKPOINT_FILE)).ok()?;
    let left: LeftOff = serde_json::from_str(&text).ok()?;
    (!left.is_empty()).then_some(left)
}

/// Writes the checkpoint from the saved plan and the session's to-dos.
/// `blocker` is `Some` when the turn failed; `None` clears a previous blocker.
pub fn record(config_dir: &Path, conversation_id: &str, blocker: Option<&str>) {
    let plan = lead::read_plan(config_dir, conversation_id).unwrap_or_default();
    let todos = grokhub_agent::todos_for(conversation_id);
    let pairs: Vec<(&str, &str)> = todos
        .iter()
        .map(|todo| (todo.status.as_str(), todo.content.as_str()))
        .collect();
    let Some(left) = left_off(&plan, &pairs, blocker.unwrap_or("")) else {
        return;
    };
    let Some(dir) = code_sessions::session_dir(config_dir, conversation_id) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(text) = serde_json::to_string(&left) {
        let _ = std::fs::write(dir.join(CHECKPOINT_FILE), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-code-keep-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn an_open_checkbox_is_the_next_step_and_a_done_one_is_not() {
        let plan = "Ship the note\n- [x] Read the file\n- [ ] Write the note";
        let left = left_off(plan, &[], "").expect("checkpoint");
        assert_eq!(left.goal, "Ship the note");
        assert_eq!(left.next, "Write the note");
        assert_eq!(left.blocker, "");
        assert_eq!(
            resume_prompt(&left),
            "Continue this Code tab session where it left off.\nGoal: Ship the note\nNext: Write the note"
        );
    }

    #[test]
    fn an_open_todo_wins_over_the_plan_line() {
        let plan = "Ship the note\n- [ ] Write the note";
        let left = left_off(
            plan,
            &[
                ("completed", "Read the file"),
                ("in_progress", "Write the parser"),
            ],
            "",
        )
        .expect("checkpoint");
        assert_eq!(left.goal, "Ship the note");
        assert_eq!(left.next, "Write the parser");
    }

    #[test]
    fn a_numbered_step_is_next_when_the_plan_has_no_checkbox() {
        let left = left_off(
            "Ship the note\n1. Read the file\n2. Write the note",
            &[],
            "",
        )
        .expect("checkpoint");
        assert_eq!(left.next, "Read the file");
    }

    #[test]
    fn a_blocker_is_kept_and_a_token_in_it_is_redacted() {
        let left = left_off(
            "Ship the note",
            &[],
            "push failed ghp_abcdefghijklmnopqrstuvwxyz012345",
        )
        .expect("checkpoint");
        assert_eq!(left.goal, "Ship the note");
        assert!(!left
            .blocker
            .contains("ghp_abcdefghijklmnopqrstuvwxyz012345"));
        assert!(left.blocker.contains("[redacted]"));
        assert!(resume_prompt(&left).contains("Blocker: "));
    }

    #[test]
    fn an_empty_plan_and_no_todos_has_no_checkpoint() {
        assert_eq!(left_off("  \n", &[], ""), None);
    }

    #[test]
    fn a_saved_checkpoint_reads_back_and_a_bad_id_writes_nothing() {
        let cfg = temp_dir("save");
        lead::save_plan(&cfg, "code-keep-s1", "Ship the note\n- [ ] Write the note").expect("plan");
        record(&cfg, "code-keep-s1", None);
        let left = read(&cfg, "code-keep-s1").expect("read");
        assert_eq!(left.goal, "Ship the note");
        assert_eq!(left.next, "Write the note");
        assert_eq!(left.blocker, "");

        record(&cfg, "code-keep-s1", Some("disk full"));
        assert_eq!(
            read(&cfg, "code-keep-s1").expect("again").blocker,
            "disk full"
        );
        record(&cfg, "code-keep-s1", None);
        assert_eq!(read(&cfg, "code-keep-s1").expect("cleared").blocker, "");

        record(&cfg, "../s1", Some("nope"));
        assert!(read(&cfg, "../s1").is_none());
        assert!(!cfg.join("s1").exists());
        assert!(!cfg.join("checkpoint.json").exists());
        let _ = std::fs::remove_dir_all(&cfg);
    }
}
