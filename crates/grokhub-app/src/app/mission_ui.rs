//! Mission-based OS v1: project one foreground mission into the Work tree.
//!
//! Adapter only — Brief, todos, episode, parks and outcomes stay the source
//! of truth. No second run loop and no new Nav page.

use super::*;
use grokhub_core::{
    mission_worthy_intent, MissionCheckStatus, MissionSeed, MissionStatus, MissionView,
};

impl Cabin {
    /// Build the foreground mission for the visible chat, if any.
    pub(super) fn mission_view_here(&self) -> Option<MissionView> {
        MissionView::project(&self.mission_seed_here())
    }

    /// Gather existing run signals for [`MissionView::project`].
    pub(super) fn mission_seed_here(&self) -> MissionSeed {
        let trace = self.trace_id();
        let turn = self.turn_no();
        let dir = crate::config::config_dir();
        let brief = grokhub_agent::brain::brief_for_turn(&dir, &trace, turn)
            .or_else(|| {
                grokhub_agent::brain::read_briefs(&dir)
                    .into_iter()
                    .rev()
                    .find(|l| l.chat_id == trace)
            });
        let episode = self.episode_here();
        let todos = grokhub_agent::todos_for(&trace);
        let hard_for_chat = |p: &super::harness_ui::HardParkUi| p.chat_id == trace;
        let approval = self
            .harness
            .park
            .as_ref()
            .filter(|p| hard_for_chat(p))
            .or_else(|| self.harness.queue.iter().find(|p| hard_for_chat(p)))
            .map(|p| {
                if p.action.is_empty() {
                    p.tool.clone()
                } else {
                    format!("{} · {}", p.tool, p.action)
                }
            });
        let soft = self
            .harness
            .soft_parks
            .iter()
            .any(|p| p.chat_id == trace);
        let (intent, care, done_goal, playbook_title) = match &brief {
            Some(b) => (
                b.brief.intent.as_str().to_string(),
                b.brief.care.as_str().to_string(),
                b.brief.done.join(", "),
                b.playbook.clone(),
            ),
            None => (String::new(), String::new(), String::new(), String::new()),
        };
        let thread_title = self
            .threads
            .iter()
            .find(|t| t.id == trace)
            .map(|t| t.goal.label.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();
        let title = if !thread_title.is_empty() {
            thread_title
        } else if !playbook_title.is_empty() {
            playbook_title
        } else if mission_worthy_intent(&intent) {
            format!("{intent} mission")
        } else if episode.is_some() {
            "Desktop mission".into()
        } else {
            String::new()
        };
        let goal = if !done_goal.is_empty() {
            done_goal
        } else {
            title.clone()
        };
        let id = episode
            .map(|e| e.id.clone())
            .unwrap_or_else(|| {
                if trace.is_empty() {
                    String::new()
                } else {
                    format!("{trace}:{turn}")
                }
            });
        let activity = match episode {
            Some(e) => e.header(now_ms()),
            None if !todos.is_empty() => {
                let open = todos
                    .iter()
                    .filter(|t| t.status == "pending" || t.status == "in_progress")
                    .count();
                format!("{} checklist items · {open} open", todos.len())
            }
            None => String::new(),
        };
        let resume_ok = episode.map(|e| e.resume).unwrap_or(false);
        let outcome = grokhub_core::outcome::read_outcomes(&dir)
            .into_iter()
            .rev()
            .find(|o| {
                (!o.episode.is_empty() && episode.is_some_and(|e| e.id == o.episode))
                    || o.task == format!("{trace}:{turn}")
                    || o.task == trace
            });
        let (outcome_result, outcome_reasons, outcome_span_ids) = match outcome {
            Some(o) => (
                match o.result {
                    grokhub_core::outcome::OutcomeResult::Success => "success".into(),
                    grokhub_core::outcome::OutcomeResult::Failure => "failure".into(),
                    grokhub_core::outcome::OutcomeResult::Pending => "pending".into(),
                },
                o.reasons,
                o.span_ids,
            ),
            None => (String::new(), Vec::new(), Vec::new()),
        };
        let goal_outcome = String::new();
        let todos_verified = outcome_reasons.iter().any(|r| r == "verify_ok")
            || matches!(
                outcome_result.as_str(),
                "success"
            );
        MissionSeed {
            id,
            title,
            goal,
            intent,
            care,
            todos: todos
                .into_iter()
                .map(|t| (t.id, t.content, t.status))
                .collect(),
            todos_verified,
            current_step: String::new(),
            approval,
            activity,
            running: self.running,
            resume_ok,
            soft_paused: soft,
            goal_outcome,
            outcome_result,
            outcome_reasons,
            outcome_span_ids,
            uncertainty: String::new(),
            next_action: String::new(),
        }
    }

    /// Work-tree mission card: title, status, checklist, approval, outcome.
    pub(super) fn paint_mission_header(&self, ui: &mut egui::Ui) {
        let Some(m) = self.mission_view_here() else {
            return;
        };
        ui.label(
            RichText::new(m.header_line())
                .size(crate::theme::FONT_META)
                .color(crate::theme::fg()),
        );
        if !m.goal.is_empty() && m.goal != m.title {
            ui.label(
                RichText::new(format!("Goal: {}", m.goal))
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        }
        if !m.current_step.is_empty() && m.status == MissionStatus::Active {
            ui.label(
                RichText::new(format!("Now: {}", m.current_step))
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        }
        for item in &m.checklist {
            let mark = match item.status {
                MissionCheckStatus::Planned => "·",
                MissionCheckStatus::Attempted => "›",
                MissionCheckStatus::Verified => "✓",
                MissionCheckStatus::Unverified => "?",
                MissionCheckStatus::Cancelled => "×",
            };
            ui.label(
                RichText::new(format!("{mark} {} ({})", item.text, item.status.as_str()))
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        }
        if let Some(a) = &m.approval {
            ui.label(
                RichText::new(format!("Needs approval: {a}"))
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::fg()),
            );
        }
        if !m.activity.is_empty() {
            ui.label(
                RichText::new(&m.activity)
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        }
        if let Some(o) = &m.outcome {
            let mut line = format!("Outcome: {}", o.result);
            if !o.completed.is_empty() {
                line.push_str(&format!(" · {}", o.completed));
            }
            if !o.uncertainty.is_empty() {
                line.push_str(&format!(" · {}", o.uncertainty));
            }
            if !o.next_action.is_empty() {
                line.push_str(&format!(" · next: {}", o.next_action));
            }
            ui.label(
                RichText::new(line)
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        }
        if m.resume_ok {
            ui.label(
                RichText::new("Paused · Continue resumes without redoing finished steps")
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        }
        ui.add_space(4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cabin_has_no_mission() {
        let cabin = Cabin::quiet_for_test();
        assert!(cabin.mission_view_here().is_none());
    }

    #[test]
    fn repair_brief_promotes_a_mission() {
        let root = crate::config::test_config_root("mission-brief");
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let cabin = Cabin::quiet_for_test();
        let trace = cabin.trace_id();
        let turn = cabin.turn_no();
        let line = grokhub_agent::brain::BriefLine {
            at: 1,
            session: trace.clone(),
            chat_id: trace.clone(),
            turn,
            brief: grokhub_agent::brain::Brief {
                intent: grokhub_agent::brain::Intent::Repair,
                care: grokhub_agent::brain::Care::Critical,
                mood: "calm".into(),
                done: vec!["services".into(), "packages".into()],
                domain: "system".into(),
                by: "rules".into(),
            },
            policy: None,
            playbook: "Linux repair".into(),
            playbook_id: "linux_repair".into(),
        };
        grokhub_agent::brain::record_brief(&root, &line).unwrap();
        let m = cabin.mission_view_here().expect("promoted");
        assert_eq!(m.intent, "repair");
        assert_eq!(m.care, "critical");
        assert_eq!(m.title, "Linux repair");
        assert_eq!(m.status, MissionStatus::Active);
    }

    #[test]
    fn desktop_episode_is_an_entry_point() {
        let root = crate::config::test_config_root("mission-ep");
        let _pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_control = true;
        cabin.harness_user_sent();
        let m = cabin.mission_view_here().expect("episode mission");
        assert!(m.id.starts_with("ep-"), "{}", m.id);
        assert_eq!(m.title, "Desktop mission");
    }

    #[test]
    fn park_marks_needs_approval() {
        let root = crate::config::test_config_root("mission-park");
        let _pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_control = true;
        cabin.harness_user_sent();
        let chat = cabin.trace_id();
        cabin.harness.park = Some(super::harness_ui::HardParkUi {
            source: super::harness_ui::ParkSource::Desk("park-1".into()),
            class: grokhub_agent::HardClass::Delete,
            path: "A",
            tool: "shell".into(),
            action: "rm -rf /tmp/x".into(),
            parked_at: std::time::Instant::now(),
            chat_id: chat,
        });
        let m = cabin.mission_view_here().expect("mission");
        assert_eq!(m.status, MissionStatus::NeedsApproval);
        assert!(m.approval.as_deref().unwrap_or("").contains("shell"));
    }
}
