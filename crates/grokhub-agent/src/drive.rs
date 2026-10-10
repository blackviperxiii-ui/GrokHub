//! Completion drive: a run does not end while its goal checklist (the
//! session's `todo_write` list) has open items. When the model stops calling
//! tools, [`Drive::check`] names what is left and the loop goes on. An item
//! marked completed this run counts only once a real tool result exists.
//! A model that stops again and again without progress ends with the open
//! items named as blocked.

use crate::session_tools::TodoItem;

/// Stops in a row with no new tool result and the same open items before the
/// run ends with them named as blocked.
pub const STALL_LIMIT: u32 = 3;

/// Tools that change the checklist or the session, not the task.
const SESSION_TOOLS: &[&str] = &["todo_write", "ask_user_question", "report_findings", "enter_plan_mode", "exit_plan_mode"];

pub fn is_session_tool(name: &str) -> bool {
    SESSION_TOOLS.contains(&name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing open: the run may end.
    End,
    /// Send this note as a user message and keep going.
    Push(String),
    /// No progress after [`STALL_LIMIT`] pushes: end, saying this.
    Blocked(String),
}

pub struct Drive {
    /// Items (id and text) completed before this run, so a "continue"
    /// keeps earlier work but a replaced list reusing an id does not.
    done_at_start: Vec<(String, String)>,
    last_left: Vec<String>,
    evidence_at_push: usize,
    stalls: u32,
}

impl Drive {
    pub fn new(start: &[TodoItem]) -> Self {
        Self {
            done_at_start: start.iter().filter(|t| t.status == "completed").map(|t| (t.id.clone(), t.content.clone())).collect(),
            last_left: Vec::new(),
            evidence_at_push: 0,
            stalls: 0,
        }
    }

    /// `now` is the checklist as the model left it; `evidence` counts tool
    /// calls (not [`is_session_tool`]) that ran without failing this run.
    pub fn check(&mut self, now: &[TodoItem], evidence: usize) -> Verdict {
        let open: Vec<&TodoItem> = now.iter().filter(|t| matches!(t.status.as_str(), "pending" | "in_progress")).collect();
        let unproven: Vec<&TodoItem> = if evidence == 0 {
            now.iter()
                .filter(|t| t.status == "completed" && !self.done_at_start.iter().any(|(id, content)| *id == t.id && *content == t.content))
                .collect()
        } else {
            Vec::new()
        };
        if open.is_empty() && unproven.is_empty() {
            return Verdict::End;
        }
        let left: Vec<String> = open.iter().chain(unproven.iter()).map(|t| t.id.clone()).collect();
        if left == self.last_left && evidence == self.evidence_at_push {
            self.stalls += 1;
        } else {
            self.stalls = 0;
        }
        self.last_left = left;
        self.evidence_at_push = evidence;
        if self.stalls >= STALL_LIMIT {
            let names = list(open.iter().chain(unproven.iter()).copied());
            return Verdict::Blocked(format!(
                "Stopped with these still open: {names}. No approach worked after {STALL_LIMIT} re-plans."
            ));
        }
        let mut note = String::new();
        if self.stalls > 0 {
            let first = open.first().or(unproven.first()).map(|t| t.content.as_str()).unwrap_or("");
            note.push_str(&format!(
                "You stopped again without progress. Take a different approach for \"{first}\" (another tool, another route or a smaller sub-step), or mark it cancelled with the reason. "
            ));
        }
        if !open.is_empty() {
            note.push_str(&format!(
                "Not done yet: {}. Continue with the next open item. Mark an item completed only after a tool result shows it. If one is truly blocked (a hard approval was refused, hardware or access is missing, or every approach failed), mark it cancelled and say why.",
                list(open.iter().copied())
            ));
        }
        if !unproven.is_empty() {
            if !open.is_empty() {
                note.push(' ');
            }
            note.push_str(&format!(
                "Marked completed without any check this run: {}. Run a check that shows each one before you finish.",
                list(unproven.iter().copied())
            ));
        }
        Verdict::Push(note)
    }
}

fn list<'a>(items: impl Iterator<Item = &'a TodoItem>) -> String {
    items.map(|t| format!("\"{}\"", t.content)).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, status: &str) -> TodoItem {
        TodoItem { id: id.into(), content: format!("check {id}"), status: status.into() }
    }

    #[test]
    fn open_items_are_pushed_and_a_covered_list_ends() {
        let mut drive = Drive::new(&[]);
        let now = [item("a", "completed"), item("b", "pending"), item("c", "in_progress")];
        assert_eq!(
            drive.check(&now, 2),
            Verdict::Push("Not done yet: \"check b\", \"check c\". Continue with the next open item. Mark an item completed only after a tool result shows it. If one is truly blocked (a hard approval was refused, hardware or access is missing, or every approach failed), mark it cancelled and say why.".into())
        );
        let now = [item("a", "completed"), item("b", "completed"), item("c", "cancelled")];
        assert_eq!(drive.check(&now, 4), Verdict::End);
    }

    #[test]
    fn completed_without_a_tool_result_is_not_done() {
        let mut drive = Drive::new(&[item("old", "completed")]);
        let now = [item("old", "completed"), item("new", "completed")];
        assert_eq!(
            drive.check(&now, 0),
            Verdict::Push("Marked completed without any check this run: \"check new\". Run a check that shows each one before you finish.".into())
        );
        assert_eq!(drive.check(&now, 1), Verdict::End);
        // Work finished in an earlier run needs no new check.
        assert_eq!(Drive::new(&[item("old", "completed")]).check(&[item("old", "completed")], 0), Verdict::End);
        // A replaced list that reuses an id for new work needs a check.
        let reused = TodoItem { id: "old".into(), content: "firewall".into(), status: "completed".into() };
        assert_eq!(
            Drive::new(&[item("old", "completed")]).check(&[reused], 0),
            Verdict::Push("Marked completed without any check this run: \"firewall\". Run a check that shows each one before you finish.".into())
        );
    }

    #[test]
    fn stopping_without_progress_replans_then_ends_blocked() {
        let mut drive = Drive::new(&[]);
        let now = [item("ports", "pending")];
        assert!(matches!(drive.check(&now, 1), Verdict::Push(n) if n.starts_with("Not done yet")));
        for _ in 0..STALL_LIMIT - 1 {
            assert!(matches!(drive.check(&now, 1), Verdict::Push(n) if n.starts_with("You stopped again without progress. Take a different approach for \"check ports\"")));
        }
        assert_eq!(
            drive.check(&now, 1),
            Verdict::Blocked("Stopped with these still open: \"check ports\". No approach worked after 3 re-plans.".into())
        );
    }

    #[test]
    fn progress_resets_the_stall_count() {
        let mut drive = Drive::new(&[]);
        let now = [item("ports", "pending")];
        for evidence in 1..=(STALL_LIMIT as usize + 2) {
            assert!(matches!(drive.check(&now, evidence), Verdict::Push(n) if n.starts_with("Not done yet")), "{evidence}");
        }
    }

    #[test]
    fn only_task_tools_count_as_evidence() {
        assert!(is_session_tool("todo_write"));
        assert!(is_session_tool("exit_plan_mode"));
        assert!(!is_session_tool("shell"));
        assert!(!is_session_tool("read_file"));
    }
}
