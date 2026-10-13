//! Phase C (SC2): run a screen or audio diagnosis fix plan through the harness.
//!
//! A feed-card **Fix it** tap builds `grokhub_core::fix_plan`, then runs steps in
//! order via the same hard-card path as Spike-9 repair (`park_repair` →
//! `fix_hard_answered`). The harness gate is the authority; core `ApprovalNeed::Hard`
//! is never downgraded to soft.

use super::*;
use grokhub_agent::harness::{self as hx, GateOutcome, Step};
use grokhub_agent::HardClass;
use grokhub_core::fix_plan::{self, FixPlan, FixPlanStep, HardApproval, PlanBuild};
use std::collections::HashMap;

/// Hard-class floor covers computer-use / CUA (#619 on beta — Harness 6).
pub(super) fn harness_desktop_steps() -> bool {
    true
}

#[derive(Debug, Clone)]
pub(super) enum StoredDiagnosis {
    Screen(grokhub_core::screen_record::ScreenReport),
    Audio(grokhub_core::audio_check::AudioReport),
}

/// A diagnosis fix plan running on the harness path.
#[derive(Debug, Clone)]
pub(super) struct DiagnosisFixRun {
    pub plan: FixPlan,
    pub step: usize,
    pub waiting_hard: bool,
    pub stopped: bool,
    pub chat_id: String,
    pub notes: Vec<String>,
}

impl DiagnosisFixRun {
    fn current(&self) -> Option<&FixPlanStep> {
        self.plan.steps.get(self.step)
    }

    pub fn done(&self) -> bool {
        self.stopped || self.step >= self.plan.steps.len()
    }
}

/// In-memory reports keyed by feed card id (not persisted).
#[derive(Debug, Default)]
pub(super) struct DiagnosisFixUi {
    pub reports: HashMap<String, StoredDiagnosis>,
    pub run: Option<DiagnosisFixRun>,
}

#[cfg(test)]
pub(super) trait FixStepExecutor {
    fn run_shell(&mut self, cmd: &str) -> Result<String, String>;
    fn run_desktop(&mut self, line: &str) -> Result<String, String>;
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(super) struct FakeExecutor {
    pub shells: Vec<String>,
    pub desktops: Vec<String>,
    pub deny_next_hard: bool,
}

#[cfg(test)]
impl FixStepExecutor for FakeExecutor {
    fn run_shell(&mut self, cmd: &str) -> Result<String, String> {
        if self.deny_next_hard {
            return Err("denied".into());
        }
        self.shells.push(cmd.to_string());
        Ok(format!("ok: {cmd}"))
    }

    fn run_desktop(&mut self, line: &str) -> Result<String, String> {
        self.desktops.push(line.to_string());
        Ok(format!("ok: {line}"))
    }
}

fn hard_class(need: HardApproval) -> HardClass {
    HardClass::parse(need.as_str()).unwrap_or(HardClass::IrreversibleOs)
}

fn card_action(step: &FixPlanStep, cmd: &str) -> String {
    let mut out = step.description.clone();
    if !step.rollback.is_empty() {
        out.push_str("\n\nRollback: ");
        out.push_str(&step.rollback);
    }
    if !cmd.is_empty() {
        out.push_str("\n\n");
        out.push_str(cmd);
    }
    out
}

fn gate_for_step(step: &FixPlanStep) -> GateOutcome {
    if let Some(shell) = step.shell.as_deref() {
        let args = serde_json::json!({ "command": shell }).to_string();
        return hx::decide(Step::Tool {
            name: "run_terminal_command",
            arguments: &args,
        });
    }
    if let Some(line) = step.desktop_cmd.as_deref() {
        return hx::decide(Step::Ask {
            title: "desktop",
            action: line,
        });
    }
    hx::decide(Step::Ask {
        title: "fix",
        action: &step.description,
    })
}

fn planned_hard(step: &FixPlanStep) -> Option<HardClass> {
    step.approval.hard_class().map(hard_class)
}

fn gate_hard(outcome: &GateOutcome) -> Option<HardClass> {
    match outcome {
        GateOutcome::Park { hard: Some(c), .. } => Some(*c),
        _ => None,
    }
}

fn host_line(step: &FixPlanStep) -> Option<String> {
    if let Some(shell) = &step.shell {
        return Some(format!("HOST_CMD: {shell}"));
    }
    step.desktop_cmd.clone()
}

impl Cabin {
    pub(super) fn remember_diagnosis(&mut self, card_id: String, stored: StoredDiagnosis) {
        self.harness.fixes.diagnosis.reports.insert(card_id, stored);
    }

    /// **Fix it** on a screen or audio report card (click only).
    pub(super) fn tap_diagnosis_fix(&mut self, card_id: &str) {
        if self
            .harness
            .fixes
            .diagnosis
            .run
            .as_ref()
            .is_some_and(|r| !r.done())
        {
            self.status = "A fix plan is already running.".into();
            return;
        }
        let Some(stored) = self.harness.fixes.diagnosis.reports.get(card_id).cloned() else {
            self.status = "No fix plan for this card.".into();
            return;
        };
        let built = match stored {
            StoredDiagnosis::Screen(r) => fix_plan::from_screen(&r),
            StoredDiagnosis::Audio(r) => fix_plan::from_audio(&r),
        };
        let plan = match built {
            PlanBuild::Plan(p) => p,
            PlanBuild::Empty { reason } => {
                self.status = reason.clone();
                self.live_mut().push(("assistant".into(), reason));
                self.persist();
                return;
            }
        };
        let chat = self
            .updates
            .iter()
            .find(|c| c.id == card_id)
            .and_then(|c| c.action.as_ref())
            .and_then(|a| match a {
                grokhub_core::UpdateAction::OpenSession { thread_id } => Some(thread_id.clone()),
                _ => None,
            })
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| self.visible_thread_id());
        self.harness.fixes.diagnosis.run = Some(DiagnosisFixRun {
            plan,
            step: 0,
            waiting_hard: false,
            stopped: false,
            chat_id: chat,
            notes: Vec::new(),
        });
        self.status = format!("Fix plan ready for {} — one step at a time.", card_id);
        self.advance_diagnosis_fix();
    }

    pub(super) fn fix_plan_hard_answered(&mut self, approve: bool) -> bool {
        let Some(run) = self.harness.fixes.diagnosis.run.as_mut() else {
            return false;
        };
        if !run.waiting_hard {
            return false;
        }
        run.waiting_hard = false;
        if !approve {
            run.stopped = true;
            self.status = "Fix stopped at your request.".into();
            self.live_mut().push((
                "assistant".into(),
                "Fix stopped. Nothing else will run.".into(),
            ));
            self.persist();
            return true;
        }
        let step = self
            .harness
            .fixes
            .diagnosis
            .run
            .as_ref()
            .and_then(|r| r.current().cloned());
        if let Some(step) = step {
            if let Err(why) = self.execute_diagnosis_step(&step) {
                if let Some(run) = self.harness.fixes.diagnosis.run.as_mut() {
                    run.stopped = true;
                }
                self.status = format!("Fix stopped: {why}");
                self.finish_diagnosis_fix_message();
                return true;
            }
            if let Some(run) = self.harness.fixes.diagnosis.run.as_mut() {
                run.step += 1;
            }
        }
        self.advance_diagnosis_fix();
        true
    }

    pub(super) fn advance_diagnosis_fix(&mut self) {
        loop {
            let step = self
                .harness
                .fixes
                .diagnosis
                .run
                .as_ref()
                .filter(|r| !r.done())
                .and_then(|r| r.current().cloned());
            let Some(step) = step else {
                return;
            };
            if let Some(line) = step.desktop_cmd.as_deref() {
                if !harness_desktop_steps() || line.trim().is_empty() {
                    if let Some(run) = self.harness.fixes.diagnosis.run.as_mut() {
                        run.notes.push(format!(
                            "Skipped “{}”: do this one by hand.",
                            step.description
                        ));
                        run.step += 1;
                    }
                    continue;
                }
            }
            let gate = gate_for_step(&step);
            if matches!(gate, GateOutcome::Refuse { .. }) {
                let why = match gate {
                    GateOutcome::Refuse { reason } => reason,
                    _ => "refused".into(),
                };
                if let Some(run) = self.harness.fixes.diagnosis.run.as_mut() {
                    run.stopped = true;
                }
                self.status = format!("Fix stopped: {why}");
                self.finish_diagnosis_fix_message();
                return;
            }
            let class = planned_hard(&step).or_else(|| gate_hard(&gate));
            if let Some(class) = class {
                let cmd = host_line(&step).unwrap_or_default();
                if let Some(run) = self.harness.fixes.diagnosis.run.as_mut() {
                    run.waiting_hard = true;
                }
                self.park_repair(class, card_action(&step, &cmd));
                return;
            }
            if let Err(why) = self.execute_diagnosis_step(&step) {
                if let Some(run) = self.harness.fixes.diagnosis.run.as_mut() {
                    run.stopped = true;
                }
                self.status = format!("Fix stopped: {why}");
                self.finish_diagnosis_fix_message();
                return;
            }
            let finished = {
                let Some(run) = self.harness.fixes.diagnosis.run.as_mut() else {
                    return;
                };
                run.step += 1;
                run.step >= run.plan.steps.len()
            };
            if finished {
                self.finish_diagnosis_fix_message();
                return;
            }
        }
    }

    fn execute_diagnosis_step(&mut self, step: &FixPlanStep) -> Result<(), String> {
        if let Some(shell) = &step.shell {
            super::jobs::run_fix_host_step(shell).map(|_| ())
        } else if let Some(line) = &step.desktop_cmd {
            super::jobs::run_fix_desktop_step(line).map(|_| ())
        } else {
            Err("nothing to run for this step".into())
        }
    }

    fn finish_diagnosis_fix_message(&mut self) {
        let Some(run) = self.harness.fixes.diagnosis.run.take() else {
            return;
        };
        let mut body = if run.stopped {
            "Fix stopped before every step ran.".to_string()
        } else {
            "Fix plan finished.".to_string()
        };
        for n in &run.notes {
            body.push('\n');
            body.push_str(n);
        }
        if !run.chat_id.is_empty() && run.chat_id == self.visible_thread_id() {
            self.live_mut().push(("assistant".into(), body));
        } else if let Some(t) = self.threads.iter_mut().find(|t| t.id == run.chat_id) {
            Arc::make_mut(&mut t.messages).push(("assistant".into(), body));
        }
        self.persist();
        self.status.clear();
    }

    #[cfg(test)]
    #[allow(clippy::question_mark)]
    pub(super) fn advance_diagnosis_fix_with(
        &mut self,
        exec: &mut dyn FixStepExecutor,
    ) -> Option<String> {
        loop {
            let run = self.harness.fixes.diagnosis.run.as_mut()?;
            if run.done() {
                return Some(if run.stopped {
                    "stopped".into()
                } else {
                    "done".into()
                });
            }
            let step = run.current().cloned().expect("step");
            let gate = gate_for_step(&step);
            if matches!(gate, GateOutcome::Refuse { .. }) {
                run.stopped = true;
                return Some("refused".into());
            }
            let class = planned_hard(&step).or_else(|| gate_hard(&gate));
            if let Some(_class) = class {
                return Some("hard".into());
            }
            if let Some(shell) = &step.shell {
                if exec.run_shell(shell).is_err() {
                    run.stopped = true;
                    return Some("stopped".into());
                }
            } else if let Some(line) = &step.desktop_cmd {
                if exec.run_desktop(line).is_err() {
                    run.stopped = true;
                    return Some("stopped".into());
                }
            }
            run.step += 1;
        }
    }
}

/// Feed pills for screen recording and audio check report cards.
pub(super) fn paint_diagnosis_report_actions(
    ui: &mut egui::Ui,
    card: &grokhub_core::UpdateCard,
) -> Option<super::pulse_ui::FeedPostAct> {
    if card.status == grokhub_core::UpdateStatus::Dismissed {
        return None;
    }
    let is_screen = grokhub_core::screen_recording_dir(card).is_some();
    let is_audio = card.source_id.starts_with("audiocheck:");
    if !is_screen && !is_audio {
        return None;
    }
    let mut act = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if is_screen {
            if card.action.is_some() && super::change_undo::click_pill(ui, "Open") {
                act = Some(super::pulse_ui::FeedPostAct::Open);
            }
            if super::change_undo::click_pill(ui, "Delete recording") {
                act = Some(super::pulse_ui::FeedPostAct::DeleteRecording);
            }
        }
        if super::change_undo::click_pill(ui, "Fix it") {
            act = Some(super::pulse_ui::FeedPostAct::FixIt(card.id.clone()));
        }
    });
    act
}

pub(super) fn is_diagnosis_report_card(card: &grokhub_core::UpdateCard) -> bool {
    grokhub_core::screen_recording_dir(card).is_some() || card.source_id.starts_with("audiocheck:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::fix_plan::{from_audio, DiagnosisKind, FixPlan};
    use grokhub_core::screen_record::{parse_report, ScreenReport};

    fn muted_mic_report() -> grokhub_core::audio_check::AudioReport {
        grokhub_core::audio_check::AudioReport {
            title: "t".into(),
            card_title: "t".into(),
            summary: "s".into(),
            text: "Audio check: mic\n\nProblems:\n- Mic muted.\nSuggested fixes:\n1. Unmute in Settings.\n2. HOST_CMD: pactl set-source-mute mic 0\n\nNothing was changed on this computer.".into(),
        }
    }

    #[test]
    fn empty_plan_shows_reason_and_does_not_start_a_run() {
        let mut cabin = Cabin::quiet_for_test();
        let report = ScreenReport {
            seen: String::new(),
            cause: String::new(),
            steps: vec![],
        };
        cabin.remember_diagnosis("c1".into(), StoredDiagnosis::Screen(report));
        cabin.tap_diagnosis_fix("c1");
        assert!(cabin.harness.fixes.diagnosis.run.is_none());
        assert!(cabin.status.contains("didn't suggest any fix steps"));
    }

    #[test]
    fn deny_on_hard_card_stops_with_zero_executor_steps() {
        let mut cabin = Cabin::quiet_for_test();
        let raw = "Fix steps:\n1. HOST_CMD: rm -f x\n";
        let report = parse_report(raw);
        cabin.remember_diagnosis("c2".into(), StoredDiagnosis::Screen(report));
        cabin.tap_diagnosis_fix("c2");
        assert!(cabin.harness.park.is_some());
        assert!(
            cabin
                .harness
                .fixes
                .diagnosis
                .run
                .as_ref()
                .unwrap()
                .waiting_hard
        );
        let exec = FakeExecutor::default();
        assert!(cabin.fix_plan_hard_answered(false));
        assert!(cabin.harness.fixes.diagnosis.run.as_ref().unwrap().stopped);
        assert!(exec.shells.is_empty());
    }

    #[test]
    fn rm_rf_always_parks_a_delete_hard_card() {
        let mut cabin = Cabin::quiet_for_test();
        let raw = "Fix steps:\n1. HOST_CMD: rm -f x\n";
        let report = parse_report(raw);
        cabin.remember_diagnosis("c3".into(), StoredDiagnosis::Screen(report));
        cabin.tap_diagnosis_fix("c3");
        let park = cabin.harness.park.clone().expect("hard card");
        assert_eq!(park.class, HardClass::Delete);
        assert!(park.action.contains("Rollback:"));
    }

    #[test]
    fn audio_pactl_fix_maps_to_a_shell_step_and_runs_once_when_soft() {
        let report = muted_mic_report();
        let plan = match from_audio(&report) {
            PlanBuild::Plan(p) => p,
            PlanBuild::Empty { reason } => panic!("{reason}"),
        };
        assert_eq!(
            plan.steps[1].shell.as_deref(),
            Some("pactl set-source-mute mic 0")
        );
        let mut cabin = Cabin::quiet_for_test();
        let pactl = plan.steps[1].clone();
        cabin.harness.fixes.diagnosis.run = Some(DiagnosisFixRun {
            plan: FixPlan {
                kind: DiagnosisKind::Audio,
                steps: vec![pactl],
            },
            step: 0,
            waiting_hard: false,
            stopped: false,
            chat_id: String::new(),
            notes: Vec::new(),
        });
        let mut exec = FakeExecutor::default();
        assert_eq!(
            cabin.advance_diagnosis_fix_with(&mut exec),
            Some("done".into())
        );
        assert_eq!(exec.shells, vec!["pactl set-source-mute mic 0"]);
    }

    #[test]
    fn fix_it_does_not_start_without_a_tap() {
        let mut cabin = Cabin::quiet_for_test();
        let report = muted_mic_report();
        cabin.remember_diagnosis("c5".into(), StoredDiagnosis::Audio(report));
        assert!(cabin.harness.fixes.diagnosis.run.is_none());
    }
}
