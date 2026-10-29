//! Completion drive: a run does not end while its goal checklist (the
//! session's `todo_write` list) has open items. When the model stops calling
//! tools, [`Drive::check`] names what is left and the loop goes on. An item
//! marked completed this run counts only once a real tool result exists.
//! A model that stops again and again without progress ends with the open
//! items named as blocked. A checklist left open by an earlier run drives
//! nothing until this run changes it, so it can't take over an unrelated ask.
//! An item seen open or cancelled at one stop counts as completed at a later
//! stop only when a tool result ran in between, so a run can't flip a blocked
//! item to done on its say-so. Swapping in new items without finishing any
//! counts as a re-plan, the same as stopping with nothing new.

use crate::checkpoint::Gaps;
use crate::session_tools::TodoItem;

/// Re-plans in a row (a stop with no new tool result and the same open items,
/// or new items added with none finished) before the run ends with the open
/// items named as blocked.
pub const STALL_LIMIT: u32 = 3;

/// Tools that change the checklist or the session, not the task.
const SESSION_TOOLS: &[&str] = &[
    "todo_write",
    "ask_user_question",
    "report_findings",
    "enter_plan_mode",
    "exit_plan_mode",
];

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
    /// The checklist as the run found it.
    start: Vec<TodoItem>,
    /// Items (id and text) completed before this run, so a "continue"
    /// keeps earlier work but a replaced list reusing an id does not.
    done_at_start: Vec<(String, String)>,
    last_left: Vec<String>,
    evidence_at_push: usize,
    stalls: u32,
    /// Ids seen pending, in progress or cancelled at an earlier stop, with the
    /// evidence count then. Completing one needs a tool result after that.
    unresolved_at: Vec<(String, usize)>,
    /// Ids completed (with proof) or cancelled at the last stop.
    resolved: Vec<String>,
}

impl Drive {
    pub fn new(start: &[TodoItem]) -> Self {
        Self {
            start: start.to_vec(),
            done_at_start: start
                .iter()
                .filter(|t| t.status == "completed")
                .map(|t| (t.id.clone(), t.content.clone()))
                .collect(),
            last_left: Vec::new(),
            evidence_at_push: 0,
            stalls: 0,
            unresolved_at: Vec::new(),
            resolved: Vec::new(),
        }
    }

    /// `now` is the checklist as the model left it; `evidence` counts tool
    /// calls (not [`is_session_tool`]) that ran without failing this run.
    #[cfg(test)]
    pub fn check(&mut self, now: &[TodoItem], evidence: usize) -> Verdict {
        self.check_with(now, evidence, &Gaps::default())
    }

    /// Pending or in-progress ids this run changed. An untouched list is empty,
    /// the same rule [`Drive::check_with`] uses so an earlier checklist cannot
    /// take over the next ask.
    pub fn open_ids(&self, now: &[TodoItem]) -> Vec<String> {
        if now == self.start.as_slice() {
            return Vec::new();
        }
        now.iter()
            .filter(|item| matches!(item.status.as_str(), "pending" | "in_progress"))
            .map(|item| item.id.clone())
            .collect()
    }

    /// [`Drive::check`] plus what the run still owes beyond its checklist
    /// ([`crate::checkpoint`]): an unchecked change, the Brief's done items, a backup.
    pub fn check_with(&mut self, now: &[TodoItem], evidence: usize, gaps: &Gaps) -> Verdict {
        let untouched = now == self.start.as_slice();
        let open: Vec<&TodoItem> = if untouched {
            Vec::new()
        } else {
            now.iter()
                .filter(|t| matches!(t.status.as_str(), "pending" | "in_progress"))
                .collect()
        };
        let unproven: Vec<&TodoItem> = if untouched {
            Vec::new()
        } else {
            now.iter()
                .filter(|t| {
                    t.status == "completed"
                        && !self
                            .done_at_start
                            .iter()
                            .any(|(id, content)| *id == t.id && *content == t.content)
                        && (evidence == 0
                            || self
                                .unresolved_at
                                .iter()
                                .any(|(id, seen)| *id == t.id && evidence <= *seen))
                })
                .collect()
        };
        if !untouched {
            // A completion pushed back as unproven also needs a result after this stop.
            for t in now
                .iter()
                .filter(|t| matches!(t.status.as_str(), "pending" | "in_progress" | "cancelled"))
                .chain(unproven.iter().copied())
            {
                match self.unresolved_at.iter_mut().find(|(id, _)| *id == t.id) {
                    Some(row) => row.1 = evidence,
                    None => self.unresolved_at.push((t.id.clone(), evidence)),
                }
            }
        }
        let resolved: Vec<String> = now
            .iter()
            .filter(|t| matches!(t.status.as_str(), "completed" | "cancelled"))
            .filter(|t| !unproven.iter().any(|u| u.id == t.id))
            .map(|t| t.id.clone())
            .collect();
        let finished_any = resolved.iter().any(|id| !self.resolved.contains(id));
        self.resolved = resolved;
        if open.is_empty() && unproven.is_empty() && gaps.is_empty() {
            return Verdict::End;
        }
        let mut left: Vec<String> = open
            .iter()
            .chain(unproven.iter())
            .map(|t| t.id.clone())
            .collect();
        left.extend(gaps.keys());
        let stalled = left == self.last_left && evidence == self.evidence_at_push;
        // New items with none finished since the last stop: the plan churned.
        let churned = !self.last_left.is_empty()
            && !finished_any
            && left.iter().any(|id| !self.last_left.contains(id));
        if stalled || churned {
            self.stalls += 1;
        } else {
            self.stalls = 0;
        }
        self.last_left = left;
        self.evidence_at_push = evidence;
        if self.stalls >= STALL_LIMIT {
            let mut names: Vec<String> = open
                .iter()
                .chain(unproven.iter())
                .map(|t| format!("\"{}\"", t.content))
                .collect();
            names.extend(gaps.names());
            return Verdict::Blocked(format!(
                "Not done: {} {} still open. I stopped after {STALL_LIMIT} re-plans without progress, so none of it is marked done. \
If this needs something GrokHub can't do yet, I can note it as a feature request instead.",
                names.join(", "),
                if names.len() == 1 { "is" } else { "are" }
            ));
        }
        let mut note = String::new();
        if self.stalls > 0 {
            let first = open
                .first()
                .or(unproven.first())
                .map(|t| t.content.clone())
                .or_else(|| gaps.names().into_iter().next())
                .unwrap_or_default();
            let first = first.trim_matches('"');
            note.push_str(&format!(
                "You stopped again without progress. Take a different approach for \"{first}\" (another tool, another route or a smaller sub-step), or mark it cancelled with the reason. \
If GrokHub has no way to do it, mark it cancelled, say so once in plain words and offer a next step. Don't add checklist items for tools the ask doesn't need. "
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
        if !gaps.is_empty() {
            if !open.is_empty() || !unproven.is_empty() {
                note.push(' ');
            }
            note.push_str(&gaps.note());
        }
        Verdict::Push(note)
    }
}

/// The line a change request's reply gets when it claims the work is done
/// but no tool ran this run: nothing backs the claim, so the turn says so.
pub fn unbacked_done(reply: &str, evidence: usize) -> Option<&'static str> {
    if evidence > 0 {
        return None;
    }
    let text = reply.to_lowercase();
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .collect();
    let claims = words
        .iter()
        .any(|w| ["done", "updated", "fixed", "completed", "finished"].contains(w))
        || [
            "all set",
            "error gone",
            "error is gone",
            "works again",
            "it works now",
        ]
        .iter()
        .any(|p| text.contains(p));
    claims.then_some("That isn't done yet: nothing ran this turn to make or check the change.")
}

fn list<'a>(items: impl Iterator<Item = &'a TodoItem>) -> String {
    items
        .map(|t| format!("\"{}\"", t.content))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, status: &str) -> TodoItem {
        TodoItem {
            id: id.into(),
            content: format!("check {id}"),
            status: status.into(),
        }
    }

    #[test]
    fn open_items_are_pushed_and_a_covered_list_ends() {
        let mut drive = Drive::new(&[]);
        let now = [
            item("a", "completed"),
            item("b", "pending"),
            item("c", "in_progress"),
        ];
        assert_eq!(
            drive.check(&now, 2),
            Verdict::Push("Not done yet: \"check b\", \"check c\". Continue with the next open item. Mark an item completed only after a tool result shows it. If one is truly blocked (a hard approval was refused, hardware or access is missing, or every approach failed), mark it cancelled and say why.".into())
        );
        let now = [
            item("a", "completed"),
            item("b", "completed"),
            item("c", "cancelled"),
        ];
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
        assert_eq!(
            Drive::new(&[item("old", "completed")]).check(&[item("old", "completed")], 0),
            Verdict::End
        );
        // A replaced list that reuses an id for new work needs a check.
        let reused = TodoItem {
            id: "old".into(),
            content: "firewall".into(),
            status: "completed".into(),
        };
        assert_eq!(
            Drive::new(&[item("old", "completed")]).check(&[reused], 0),
            Verdict::Push("Marked completed without any check this run: \"firewall\". Run a check that shows each one before you finish.".into())
        );
    }

    #[test]
    fn a_checklist_left_open_by_an_earlier_run_does_not_drive_a_new_ask() {
        let earlier = [item("a", "completed"), item("b", "pending")];
        // The new run never touches the list: it ends, with no evidence needed.
        assert_eq!(Drive::new(&earlier).check(&earlier, 0), Verdict::End);
        // Once the run works the list, its open items drive it again.
        let worked = [item("a", "completed"), item("b", "in_progress")];
        assert_eq!(
            Drive::new(&earlier).check(&worked, 1),
            Verdict::Push("Not done yet: \"check b\". Continue with the next open item. Mark an item completed only after a tool result shows it. If one is truly blocked (a hard approval was refused, hardware or access is missing, or every approach failed), mark it cancelled and say why.".into())
        );
    }

    #[test]
    fn stopping_without_progress_replans_then_ends_blocked() {
        let mut drive = Drive::new(&[]);
        let now = [item("ports", "pending")];
        assert!(matches!(drive.check(&now, 1), Verdict::Push(n) if n.starts_with("Not done yet")));
        for _ in 0..STALL_LIMIT - 1 {
            assert!(
                matches!(drive.check(&now, 1), Verdict::Push(n) if n.starts_with("You stopped again without progress. Take a different approach for \"check ports\""))
            );
        }
        assert_eq!(
            drive.check(&now, 1),
            Verdict::Blocked("Not done: \"check ports\" is still open. I stopped after 3 re-plans without progress, so none of it is marked done. If this needs something GrokHub can't do yet, I can note it as a feature request instead.".into())
        );
    }

    #[test]
    fn an_item_seen_open_needs_a_new_tool_result_to_count_as_completed() {
        let mut drive = Drive::new(&[]);
        assert!(matches!(
            drive.check(&[item("pulse", "pending")], 2),
            Verdict::Push(_)
        ));
        // Earlier tool work doesn't back a completion made with no new result.
        assert!(matches!(
            drive.check(&[item("pulse", "completed")], 2),
            Verdict::Push(n) if n.ends_with("Marked completed without any check this run: \"check pulse\". Run a check that shows each one before you finish.")
        ));
        assert_eq!(drive.check(&[item("pulse", "completed")], 3), Verdict::End);
    }

    #[test]
    fn a_cancelled_item_cannot_flip_to_completed_without_a_new_tool_result() {
        let mut drive = Drive::new(&[]);
        let blocked = [item("a", "completed"), item("pulse", "cancelled")];
        assert!(matches!(
            drive.check(&[item("a", "pending"), item("pulse", "pending")], 1),
            Verdict::Push(_)
        ));
        assert_eq!(drive.check(&blocked, 2), Verdict::End);
        // Said blocked, then "error gone" with nothing new run: pushed back.
        let flipped = [item("a", "completed"), item("pulse", "completed")];
        assert_eq!(
            drive.check(&flipped, 2),
            Verdict::Push("Marked completed without any check this run: \"check pulse\". Run a check that shows each one before you finish.".into())
        );
    }

    #[test]
    fn adding_new_items_without_finishing_any_counts_as_a_re_plan() {
        let mut drive = Drive::new(&[]);
        let mut verdicts = Vec::new();
        // Each stop swaps in a new probe and runs one tool: busy, but nothing finishes.
        for (n, probe) in [
            "skill_list",
            "scheduler_list",
            "automation_list",
            "diagnose",
        ]
        .iter()
        .enumerate()
        {
            let now = [item("pulse", "pending"), item(probe, "pending")];
            verdicts.push(drive.check(&now, n + 1));
        }
        assert!(matches!(&verdicts[0], Verdict::Push(n) if n.starts_with("Not done yet")));
        assert!(
            matches!(&verdicts[1], Verdict::Push(n) if n.contains("Don't add checklist items for tools the ask doesn't need."))
        );
        assert!(matches!(&verdicts[2], Verdict::Push(n) if n.starts_with("You stopped again")));
        assert_eq!(
            verdicts[3],
            Verdict::Blocked("Not done: \"check pulse\", \"check diagnose\" are still open. I stopped after 3 re-plans without progress, so none of it is marked done. If this needs something GrokHub can't do yet, I can note it as a feature request instead.".into())
        );
        // Finishing one item while adding another is progress, not churn.
        let mut drive = Drive::new(&[]);
        assert!(matches!(
            drive.check(&[item("a", "pending")], 1),
            Verdict::Push(_)
        ));
        assert!(matches!(
            drive.check(&[item("a", "completed"), item("b", "pending")], 2),
            Verdict::Push(n) if n.starts_with("Not done yet")
        ));
    }

    #[test]
    fn a_done_claim_with_no_tool_run_gets_an_honest_line() {
        let line = "That isn't done yet: nothing ran this turn to make or check the change.";
        assert_eq!(
            unbacked_done("Pulse feed updated. <b>Done.</b> ✨", 0),
            Some(line)
        );
        assert_eq!(unbacked_done("Error gone, all good.", 0), Some(line));
        assert_eq!(unbacked_done("Pulse feed updated. Done.", 1), None);
        assert_eq!(
            unbacked_done(
                "Pulse can't read your Steam library yet. Want me to note it as a feature?",
                0
            ),
            None
        );
        // "undone" is not "done".
        assert_eq!(unbacked_done("That change is undone.", 0), None);
    }

    #[test]
    fn progress_resets_the_stall_count() {
        let mut drive = Drive::new(&[]);
        let now = [item("ports", "pending")];
        for evidence in 1..=(STALL_LIMIT as usize + 2) {
            assert!(
                matches!(drive.check(&now, evidence), Verdict::Push(n) if n.starts_with("Not done yet")),
                "{evidence}"
            );
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
