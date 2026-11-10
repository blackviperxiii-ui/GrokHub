//! Spike-8b: `/diagnose` and the "something's wrong with my computer" intent.
//! The cabin runs the read-only probes (`grokhub_agent::repair`) off the UI
//! thread and posts the plain findings as a chat line. No new chrome (D2).
//! Without the `system_state` grant no probe runs: the answer is the ask, and
//! Spike-8a's inline ask card (`ScopeAsks`) is queued in the Work tree.
//!
//! Spike-9: findings with a safe fix get one proposal card each on the
//! approval stack (What's wrong / What I'll do / Why / How to undo / Risk).
//! Fix it is a click, never Enter. The cabin never runs a step itself: soft
//! steps go to the engine one at a time under the pill, hard ones park the
//! hard card, and hard-floor steps are only guidance. Then the finding's probe
//! runs again, and the result card offers Undo fix (`UndoAsk::from_click`).

use super::*;
use grokhub_agent::harness as hx;
use grokhub_agent::repair;

/// A diagnose answer, whether it needs the System state ask, and the fixes it found.
pub(super) type DiagnoseDone = (String, bool, Vec<repair::FixPlan>);
/// A card's quiet look: the card, what was checked, and what it found.
pub(super) type CardLookDone = (String, String, repair::DiagnoseReport);
/// The receipt line when the look couldn't read system state yet.
pub(super) const LOOK_NEEDS_SCOPE: &str = "I need your OK to read system state first. Allow it on the card in your chat, then tap this again.";

/// After undoing the fan undo: the fans keep the settings they had.
const FANS_BACK_NOTE: &str =
    "Your fans keep the settings I saved. Run the fan fix on the card to set full speed again.";

/// Fix proposals and the fix being applied.
#[derive(Debug, Default)]
pub(super) struct FixUi {
    /// One card per plan, worst finding first. Hidden while a fix runs.
    pub proposals: Vec<repair::FixPlan>,
    /// Screen / audio diagnosis fix plans (Phase C).
    pub diagnosis: super::fix_it::DiagnosisFixUi,
    pub run: Option<repair::ApplyRun>,
    /// Grok Build picked up the step this run is waiting on.
    pub gb_started: bool,
    /// The re-check's report on its way.
    pub verify_rx: Option<mpsc::Receiver<repair::DiagnoseReport>>,
    /// The result card was closed (Done or Undo fix).
    pub closed: bool,
    /// The result went to the chat once.
    pub announced: bool,
    /// The fan fix's saved settings, for its re-check and its Undo.
    pub fans: Option<FanFix>,
}

/// What a fan fix saved before it ran, and what its re-check looks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FanFix {
    pub saved: Vec<repair::SavedFan>,
    pub goal: repair::FanGoal,
}

impl FixUi {
    pub fn busy(&self) -> bool {
        self.verify_rx.is_some()
            || self.run.as_ref().is_some_and(|r| !r.is_done())
            || self.diagnosis.run.as_ref().is_some_and(|r| !r.done())
    }
}

/// What one proposal card shows. Built apart from painting so tests can read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FixCard {
    pub sections: Vec<(&'static str, String)>,
    /// Each step's plain line, with its command when the app may run it.
    pub steps: Vec<(String, Option<String>)>,
    /// `Fix it` when anything may run; none when every step is hard floor.
    pub primary: Option<&'static str>,
    pub secondary: &'static str,
    pub hard: bool,
    /// Steps are a live checklist: each row carries its own mark, no bullet.
    pub checklist: bool,
}

pub(super) const FIX_EYEBROW: &str = "Fix proposal";
pub(super) const FIX_PRIMARY: &str = "Fix it";
pub(super) const UNDO_PRIMARY: &str = "Undo fix";
/// The running fix's button: stops the step in flight, nothing after it runs.
pub(super) const STOP_FIX: &str = "Stop fix";

/// The live checklist's rows, each with the app's step mark in front.
pub(super) fn checklist_rows(lines: &[repair::ChecklistLine]) -> Vec<(String, Option<String>)> {
    use repair::StepMark;
    lines
        .iter()
        .map(|l| {
            let row = match l.mark {
                StepMark::Done => format!("✓ {}", l.plain),
                StepMark::Running => format!("→ {}", l.plain),
                StepMark::NeedsOk => format!("→ Waiting for your OK: {}", l.plain),
                StepMark::Pending => format!("· {}", l.plain),
                StepMark::Skipped => format!("– {} (didn't run)", l.plain.trim_end_matches('.')),
                StepMark::Failed => format!("× {} (didn't work)", l.plain.trim_end_matches('.')),
            };
            (row, None)
        })
        .collect()
}
/// The reply recorded when Grok Build couldn't take a step at all.
const STEP_FAILED_NOTE: &str = "STEP_FAILED Grok Build didn't take the step:";

pub(super) fn fix_card(plan: &repair::FixPlan) -> FixCard {
    let steps = plan
        .steps
        .iter()
        .map(|s| {
            let cmd = (s.class != repair::StepClass::HardFloor).then(|| s.command.clone());
            (s.card_line(), cmd)
        })
        .collect();
    let runnable = plan.runnable() > 0;
    FixCard {
        sections: plan
            .card_sections()
            .iter()
            .map(|(h, t)| (*h, t.to_string()))
            .collect(),
        steps,
        primary: runnable.then_some(FIX_PRIMARY),
        secondary: if runnable { "Not now" } else { "Got it" },
        hard: plan.has_hard(),
        checklist: false,
    }
}

impl Cabin {
    /// A typed message that means "check my computer": the cabin answers it
    /// with diagnose instead of the model. True when it took the message.
    pub(super) fn try_diagnose_intent(&mut self, text: &str) -> bool {
        if self.running || self.scheduled_perm || self.harness.diagnose_rx.is_some() {
            return false;
        }
        let os = repair::Os::current();
        // In a chat opened from a crash card, "check into it" reads that
        // program's crash history, by the name on the card.
        let subject = self.card_program_here();
        let probes = subject
            .as_ref()
            .and_then(|_| repair::probes_for_subject(text, os))
            .or_else(|| repair::probes_for_intent(text, os));
        let Some(probes) = probes else {
            return false;
        };
        self.touch();
        self.live_mut().push(("user".into(), text.to_string()));
        self.stamp_current_access();
        self.persist();
        self.run_diagnose_about(probes, false, subject);
        true
    }

    /// The program the open chat's card is about, when it was opened from one.
    pub(super) fn card_program_here(&self) -> Option<repair::ProgramName> {
        let here = self.visible_thread_id();
        self.updates
            .iter()
            .filter(|c| c.discuss_thread.as_deref() == Some(here.as_str()))
            .filter_map(|c| c.pulse.report.as_ref()?.subject.as_ref())
            .find(|s| s.kind == grokhub_core::card_report::SubjectKind::Program)
            .and_then(|s| repair::ProgramName::parse(&s.name))
    }

    /// Run `probes` (every one for this OS when empty). `slash` marks the
    /// answer as a slash result; an intent answer is a plain reply.
    pub(super) fn run_diagnose(&mut self, probes: Vec<repair::ProbeId>, slash: bool) {
        self.run_diagnose_about(probes, slash, None);
    }

    /// [`Self::run_diagnose`], with the program a card is about.
    fn run_diagnose_about(
        &mut self,
        probes: Vec<repair::ProbeId>,
        slash: bool,
        subject: Option<repair::ProgramName>,
    ) {
        if self.harness.diagnose_rx.is_some() {
            self.status = "Already checking your computer".into();
            return;
        }
        let dir = crate::config::config_dir();
        let session = self.fix_session();
        let access = self.access_mode();
        let (tx, rx) = mpsc::channel();
        self.harness.diagnose_rx = Some(rx);
        self.harness.diagnose_slash = slash;
        self.harness.diagnose_chat = self.visible_thread_id();
        self.status = "Checking your computer (read only)…".into();
        std::thread::spawn(move || {
            let ledger = hx::ConsentLedger::load(&dir);
            let os = repair::Os::current();
            let ctx = repair::DiagnoseCtx {
                config_dir: &dir,
                session_id: &session,
                ledger: &ledger,
                access,
                os,
                has_bin: &repair::on_path,
                runner: &repair::run_spec,
                subject: subject.as_ref(),
            };
            let report = repair::diagnose(&ctx, &probes);
            let plans = repair::plans_for(&report.findings, os, &repair::on_path);
            let _ = tx.send((repair::report_text(&report), report.ask.is_some(), plans));
        });
    }

    /// A card's read-only look, run quietly: the card's program's crash
    /// history and the background services. Its receipt lands on the card;
    /// nothing goes to a chat. False when there is no program to look at.
    pub(super) fn start_card_look(&mut self, card_id: &str) -> bool {
        let Some(name) = self
            .updates
            .iter()
            .find(|c| c.id == card_id)
            .and_then(|c| c.pulse.report.as_ref()?.subject.as_ref())
            .filter(|s| s.kind == grokhub_core::card_report::SubjectKind::Program)
            .and_then(|s| repair::ProgramName::parse(&s.name))
        else {
            return false;
        };
        if self.harness.card_look_rx.is_some() {
            self.status = "Already checking a card".into();
            return true;
        }
        let os = repair::Os::current();
        let mut probes = repair::ProbeId::for_subject(os).to_vec();
        probes.push(if os == repair::Os::Windows {
            repair::ProbeId::ServicesStoppedAuto
        } else {
            repair::ProbeId::FailedServices
        });
        let what = format!("Checked {}'s crash history", name.as_str());
        let dir = crate::config::config_dir();
        let session = format!("card-{card_id}");
        let access = self.access_mode();
        let (tx, rx) = mpsc::channel();
        self.harness.card_look_rx = Some(rx);
        self.status = format!("Checking {}'s crash history (read only)…", name.as_str());
        let id = card_id.to_string();
        std::thread::spawn(move || {
            let ledger = hx::ConsentLedger::load(&dir);
            let ctx = repair::DiagnoseCtx {
                config_dir: &dir,
                session_id: &session,
                ledger: &ledger,
                access,
                os,
                has_bin: &repair::on_path,
                runner: &repair::run_spec,
                subject: Some(&name),
            };
            let _ = tx.send((id, what, repair::diagnose(&ctx, &probes)));
        });
        true
    }

    /// The look's receipt goes on its card, which opens to show it.
    pub(super) fn poll_card_look(&mut self) {
        let Some(rx) = self.harness.card_look_rx.take() else {
            return;
        };
        let (id, what, report) = match rx.try_recv() {
            Ok(done) => done,
            Err(mpsc::TryRecvError::Empty) => {
                self.harness.card_look_rx = Some(rx);
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = "Couldn't finish checking".into();
                return;
            }
        };
        let result = if report.ask.is_some() {
            let ledger = hx::ConsentLedger::load_now(&crate::config::config_dir());
            self.harness.indexer.asks.ask(
                hx::Scope::SystemState,
                repair::SCOPE_ASK_WHY,
                &ledger,
                now_ms(),
            );
            vec![LOOK_NEEDS_SCOPE.to_string()]
        } else {
            report.findings.iter().map(|f| f.plain.clone()).collect()
        };
        let Some(card) = self.updates.iter_mut().find(|c| c.id == id) else {
            self.status.clear();
            return;
        };
        grokhub_core::card_report::add_receipt(
            card,
            grokhub_core::card_report::CardReceipt {
                at_ms: now_ms(),
                what: what.clone(),
                result,
            },
        );
        self.pulse_view.report_open.insert(id);
        self.status = format!("{what}. It's on the card.");
        self.persist_updates();
    }

    pub(super) fn poll_diagnose(&mut self) {
        self.poll_card_look();
        self.poll_fix();
        let Some(rx) = self.harness.diagnose_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((body, ask, plans)) => {
                if ask {
                    let ledger = hx::ConsentLedger::load_now(&crate::config::config_dir());
                    self.harness.indexer.asks.ask(
                        hx::Scope::SystemState,
                        repair::SCOPE_ASK_WHY,
                        &ledger,
                        now_ms(),
                    );
                }
                let body = if self.harness.diagnose_slash {
                    mark_slash_result(&body)
                } else {
                    body
                };
                self.status.clear();
                self.post_diagnose(body);
                if !self.harness.fixes.busy() {
                    self.harness.fixes = FixUi {
                        proposals: plans,
                        ..FixUi::default()
                    };
                }
            }
            Err(mpsc::TryRecvError::Empty) => self.harness.diagnose_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = "Couldn't finish checking your computer".into()
            }
        }
    }

    fn fix_session(&self) -> String {
        self.threads
            .get(self.thread_idx)
            .map(|t| t.id.clone())
            .unwrap_or_else(|| "session".into())
    }

    /// Fix it on proposal `idx`: take the restore point, then the first step.
    /// Only a click calls this, so the user is in the seat.
    pub(super) fn apply_fix(&mut self, idx: usize) {
        if self.running || self.harness.fixes.busy() {
            self.status = "Wait for the current reply to finish, then press Fix it".into();
            return;
        }
        if idx >= self.harness.fixes.proposals.len() {
            return;
        }
        let plan = self.harness.fixes.proposals.remove(idx);
        let dir = crate::config::config_dir();
        let session = self.fix_session();
        let ctx = repair::ApplyCtx {
            config_dir: &dir,
            session_id: &session,
            os: repair::Os::current(),
            attended: true,
            access: self.access_mode(),
            has_bin: &repair::on_path,
        };
        match repair::ApplyRun::begin(&ctx, plan) {
            Ok(run) => {
                self.harness.fixes.run = Some(run);
                self.harness.fixes.closed = false;
                self.harness.fixes.announced = false;
                self.advance_fix();
            }
            Err(why) => {
                self.live_mut().push(("assistant".into(), why));
                self.persist();
            }
        }
    }

    /// Ask the run what's next and do it.
    fn advance_fix(&mut self) {
        let Some(run) = self.harness.fixes.run.as_mut() else {
            return;
        };
        match run.next_action() {
            repair::Action::Gb {
                prompt,
                command,
                approved,
            } => {
                if approved {
                    // Approved on the hard card: Grok's own Allow card asks once.
                    self.harness.oneshot = Some(super::harness_ui::OneShot {
                        action: Some(command),
                        step: None,
                        restore: self.permission_mode,
                        started: false,
                    });
                    self.permission_mode = PermissionMode::Ask;
                }
                self.harness.fixes.gb_started = false;
                self.send_chat(prompt.clone());
                let queued = self.followup_queue.contains_text(&prompt);
                if !self.running && !queued {
                    // Grok Build didn't take the step (not installed, not signed in).
                    let why = format!("{STEP_FAILED_NOTE} {}", self.status);
                    if let Some(run) = self.harness.fixes.run.as_mut() {
                        run.step_finished(&why);
                    }
                    self.advance_fix();
                    return;
                }
                self.status =
                    "Fixing… if your computer asks for your password, type it yourself".into();
            }
            repair::Action::Park { class, command, .. } => {
                self.park_repair(class, command);
            }
            repair::Action::Verify(repair::ProbeId::Fans) => {
                // The fan files read back as they are; no probe, no admin.
                let report = match self.harness.fixes.fans.as_ref() {
                    Some(f) => repair::fans_check(&f.saved, f.goal),
                    None => repair::DiagnoseReport::default(),
                };
                run.record_verify(&report);
                self.advance_fix();
            }
            repair::Action::Verify(probe) => {
                let dir = crate::config::config_dir();
                let session = self.fix_session();
                let access = self.access_mode();
                let (tx, rx) = mpsc::channel();
                self.harness.fixes.verify_rx = Some(rx);
                self.status = "Checking whether the fix worked…".into();
                std::thread::spawn(move || {
                    let ledger = hx::ConsentLedger::load(&dir);
                    let ctx = repair::DiagnoseCtx {
                        config_dir: &dir,
                        session_id: &session,
                        ledger: &ledger,
                        access,
                        os: repair::Os::current(),
                        has_bin: &repair::on_path,
                        runner: &repair::run_spec,
                        subject: None,
                    };
                    let _ = tx.send(repair::diagnose(&ctx, &[probe]));
                });
            }
            repair::Action::Done if self.harness.fixes.announced => {}
            repair::Action::Done => {
                let text = run.result_text();
                self.harness.fixes.announced = true;
                self.status.clear();
                self.live_mut().push(("assistant".into(), text));
                self.persist();
            }
        }
    }

    /// Step progress: Grok Build's reply to the step, and the re-check.
    pub(super) fn poll_fix(&mut self) {
        let waiting = self
            .harness
            .fixes
            .run
            .as_ref()
            .is_some_and(|r| r.awaiting_gb());
        if waiting {
            if self.running {
                self.harness.fixes.gb_started = true;
            } else if self.harness.fixes.gb_started {
                let reply = self
                    .messages
                    .iter()
                    .rev()
                    .find(|(r, _)| r == "assistant")
                    .map(|(_, b)| b.clone())
                    .unwrap_or_default();
                if let Some(run) = self.harness.fixes.run.as_mut() {
                    run.step_finished(&reply);
                }
                self.advance_fix();
            }
        }
        let Some(rx) = self.harness.fixes.verify_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(report) => {
                if let Some(run) = self.harness.fixes.run.as_mut() {
                    run.record_verify(&report);
                }
                self.advance_fix();
            }
            Err(mpsc::TryRecvError::Empty) => self.harness.fixes.verify_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {
                if let Some(run) = self.harness.fixes.run.as_mut() {
                    run.record_verify(&repair::DiagnoseReport::default());
                }
                self.advance_fix();
            }
        }
    }

    /// The hard card for a fix step was answered (click, Esc, TTL or Halt).
    pub(super) fn fix_hard_answered(&mut self, approve: bool) {
        if self.fix_plan_hard_answered(approve) {
            return;
        }
        if let Some(run) = self.harness.fixes.run.as_mut() {
            run.answer_hard(approve);
        }
        self.advance_fix();
    }

    /// Halt: the fix stops with a span. Its hard card was denied already.
    pub(super) fn halt_fix(&mut self) {
        let Some(run) = self.harness.fixes.run.as_mut() else {
            return;
        };
        if run.is_done() {
            return;
        }
        run.halt("halted — fail-closed Deny");
        self.harness.fixes.verify_rx = None;
        self.advance_fix();
    }

    /// Undo fix, from the result card's click only.
    fn undo_fix(&mut self) {
        let Some(run) = self.harness.fixes.run.as_mut() else {
            return;
        };
        if run.plan.finding.probe == repair::ProbeId::Fans {
            if let Some(fans) = self.harness.fixes.fans.clone() {
                let finding = run.plan.finding.clone();
                self.undo_fans(fans, finding);
                return;
            }
        }
        let text = run.undo(hx::UndoAsk::from_click());
        self.harness.fixes.closed = true;
        self.live_mut().push(("assistant".into(), text));
        self.persist();
    }

    /// Start a fan fix (or its undo) with the settings it saved.
    pub(super) fn start_fan_fix(
        &mut self,
        plan: repair::FixPlan,
        saved: Vec<repair::SavedFan>,
        goal: repair::FanGoal,
    ) {
        self.harness.fixes.fans = Some(FanFix { saved, goal });
        self.harness.fixes.proposals.push(plan);
        let last = self.harness.fixes.proposals.len() - 1;
        self.apply_fix(last);
    }

    /// Undo for a fan fix: writing the fans back needs admin, so it runs as
    /// its own step the user types the password for, then reads them back.
    fn undo_fans(&mut self, fans: FanFix, finding: repair::Finding) {
        if fans.goal == repair::FanGoal::Saved {
            if let Some(run) = self.harness.fixes.run.as_mut() {
                run.undone = true;
            }
            self.harness.fixes.closed = true;
            self.live_mut()
                .push(("assistant".into(), FANS_BACK_NOTE.into()));
            self.persist();
            return;
        }
        let game = self.game_running().is_some();
        match repair::restore_fans_plan(finding, &fans.saved, game) {
            Ok(plan) => {
                if let Some(run) = self.harness.fixes.run.as_mut() {
                    run.undone = true;
                }
                self.harness.fixes.closed = true;
                self.start_fan_fix(plan, fans.saved, repair::FanGoal::Saved);
            }
            // The fans stay at full speed and Undo fix stays on the card.
            Err(block) => self.status = block.plain().into(),
        }
    }

    /// Proposal cards, or the running fix, or its result with Undo fix.
    pub(super) fn paint_fix_cards(&mut self, ui: &mut egui::Ui) {
        let running = self.running;
        if let Some(run) = self.harness.fixes.run.as_ref() {
            if !run.is_done() {
                let note = "One step at a time. If your computer asks for your password, type it yourself; I never do.";
                let card = FixCard {
                    sections: vec![],
                    steps: checklist_rows(&run.checklist()),
                    primary: None,
                    secondary: STOP_FIX,
                    hard: false,
                    checklist: true,
                };
                let answer = fix_card_ui(
                    ui,
                    ("fix-running", run.id.clone()),
                    "Fixing",
                    &run.plan.what,
                    note,
                    &card,
                    running,
                );
                if answer == Some(false) {
                    // The chat's own Stop: it ends the step in flight and denies its card.
                    self.halt_in_flight_for("you stopped it");
                }
                return;
            }
            if !self.harness.fixes.closed && !run.undone {
                let eyebrow = if run.verified == Some(true) {
                    "Fixed"
                } else {
                    "Fix result"
                };
                let card = FixCard {
                    sections: vec![],
                    steps: checklist_rows(&run.checklist()),
                    primary: Some(UNDO_PRIMARY),
                    secondary: "Done",
                    hard: false,
                    checklist: true,
                };
                let (id, text) = (run.id.clone(), run.result_text());
                match fix_card_ui(ui, ("fix-result", id), eyebrow, &text, "", &card, running) {
                    Some(true) => self.undo_fix(),
                    Some(false) => self.harness.fixes.closed = true,
                    None => {}
                }
            }
            return;
        }
        let mut hit = None;
        for (i, plan) in self.harness.fixes.proposals.iter().enumerate() {
            let card = fix_card(plan);
            let id = ("fix-proposal", format!("{}:{i}", plan.finding.probe.key()));
            if let Some(answer) = fix_card_ui(ui, id, FIX_EYEBROW, &plan.what, "", &card, running) {
                hit = Some((i, answer));
            }
        }
        match hit {
            Some((i, true)) => self.apply_fix(i),
            Some((i, false)) => {
                self.harness.fixes.proposals.remove(i);
            }
            None => {}
        }
    }
    /// The answer goes to the chat that asked, even after a switch; a chat
    /// deleted meanwhile gets nothing.
    fn post_diagnose(&mut self, body: String) {
        let chat = std::mem::take(&mut self.harness.diagnose_chat);
        if chat.is_empty() || chat == self.visible_thread_id() {
            self.live_mut().push(("assistant".into(), body));
            self.stamp_current_access();
        } else if let Some(t) = self.threads.iter_mut().find(|t| t.id == chat) {
            Arc::make_mut(&mut t.messages).push(("assistant".into(), body));
        } else {
            return;
        }
        self.persist();
    }
}

/// The approval-card family: white accents on the dark cabin, the approval
/// enter motion (snaps under reduced motion), no pulse at rest. Click only:
/// Enter never answers a fix card. Some(true) primary, Some(false) secondary.
fn fix_card_ui(
    ui: &mut egui::Ui,
    id: (&str, String),
    eyebrow: &str,
    title: &str,
    note: &str,
    card: &FixCard,
    running: bool,
) -> Option<bool> {
    let stroke_w = if card.hard { 2.0 } else { 1.0 };
    let stroke = if card.hard {
        crate::theme::fg()
    } else {
        crate::theme::border()
    };
    ui.add_space(8.0);
    let card_id = egui::Id::new(("fix-card", id.0, id.1));
    let enter_t = crate::motion::approval_enter_t(ui, card_id, true);
    let slot = ui
        .available_rect_before_wrap()
        .translate(egui::vec2(0.0, crate::motion::approval_y(enter_t, false)));
    let mut hit = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| {
        ui.multiply_opacity(enter_t.clamp(0.0, 1.0));
        let inner = super::harness_ui::approval_card_inner(ui.available_width(), stroke_w);
        let muted = |t: &str, size: f32| {
            egui::Label::new(RichText::new(t).size(size).color(crate::theme::muted())).wrap()
        };
        let strong = |t: &str, size: f32| {
            egui::Label::new(RichText::new(t).size(size).color(crate::theme::fg())).wrap()
        };
        let framed = egui::Frame::NONE
            .fill(egui::Color32::TRANSPARENT)
            .corner_radius(crate::theme::CHROME_RADIUS)
            .stroke(egui::Stroke::new(stroke_w, stroke))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(inner);
                ui.set_max_width(inner);
                ui.add(muted(eyebrow, 12.0));
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(crate::theme::TYPE_BODY)
                            .strong()
                            .color(crate::theme::fg()),
                    )
                    .wrap(),
                );
                for (heading, text) in &card.sections {
                    ui.add_space(8.0);
                    ui.add(muted(heading, 12.0));
                    ui.add(strong(text, 13.0));
                }
                if !card.steps.is_empty() {
                    ui.add_space(8.0);
                    ui.add(muted("Steps", 12.0));
                    for (line, cmd) in &card.steps {
                        if card.checklist {
                            ui.add(strong(line, 13.0));
                        } else {
                            ui.add(strong(&format!("• {line}"), 13.0));
                        }
                        if let Some(cmd) = cmd {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(cmd)
                                        .size(crate::theme::TYPE_CAPTION)
                                        .monospace()
                                        .color(crate::theme::muted()),
                                )
                                .wrap(),
                            );
                        }
                    }
                }
                if !note.is_empty() {
                    ui.add_space(4.0);
                    ui.add(muted(note, 12.0));
                }
                if card.primary.is_some() || !card.secondary.is_empty() {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let primary = card.primary.is_some_and(|label| {
                            if card.hard {
                                crate::cards::danger_pill(ui, label)
                            } else {
                                crate::cards::white_pill(ui, label)
                            }
                        });
                        if primary {
                            hit = Some(true);
                        } else if !card.secondary.is_empty()
                            && crate::cards::ghost_pill(ui, card.secondary)
                        {
                            hit = Some(false);
                        }
                    });
                }
            });
        let time = ui.ctx().input(|i| i.time) as f32;
        crate::motion::paint_thinking_rim(ui.painter(), framed.response.rect, running, time);
    });
    hit
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::repair::{DraftStep, Finding, FixPlan, ProbeId, Severity, StepClass};

    fn pinned(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::create_dir_all(&root);
        (crate::config::TestConfigDir::set(root.clone()), root)
    }

    /// No Grok Build on PATH, so a send stays local.
    struct NoGrok;
    impl NoGrok {
        fn set(root: &std::path::Path) -> Self {
            unsafe { std::env::set_var("GROKHUB_GROK", root.join("missing-grok")) };
            Self
        }
    }
    impl Drop for NoGrok {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("GROKHUB_GROK") };
        }
    }

    fn dns() -> Finding {
        Finding {
            severity: Severity::Warning,
            plain: "Your computer can't look up website names (DNS), so websites won't load even when you're connected.".into(),
            detail: String::new(),
            probe: ProbeId::Dns,
        }
    }

    fn plan(steps: Vec<DraftStep>) -> FixPlan {
        FixPlan::new(
            dns(),
            "I'll clear the list of website addresses your computer remembers.",
            "A stale list stops websites loading.",
            "Undo fix puts the settings file back.",
            "Low.",
            steps,
        )
        .unwrap()
    }

    fn tools(root: &std::path::Path) -> Vec<(String, String)> {
        hx::read_spans(root, "session")
            .unwrap()
            .into_iter()
            .map(|s| (s.tool, s.decision))
            .collect()
    }

    /// A snapshot step comes first wherever this computer has one (System
    /// Restore on Windows, a btrfs or snapper binary on a Linux runner). With
    /// no Grok Build it can't run, so the fix goes on with the file backup alone.
    fn snapshot_spans() -> Vec<(String, String)> {
        if !repair::detect_backends(repair::Os::current(), &repair::on_path).is_empty() {
            vec![
                (repair::REPAIR_TOOL.into(), "allow".into()),
                (repair::RESTORE_TOOL.into(), "deny".into()),
            ]
        } else {
            vec![]
        }
    }

    fn wait_fix(cabin: &mut Cabin) {
        let start = std::time::Instant::now();
        while cabin.harness.fixes.verify_rx.is_some()
            && start.elapsed() < std::time::Duration::from_secs(4)
        {
            cabin.poll_fix();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_hard_floor_step_is_never_an_apply_button() {
        let floor_only = plan(vec![DraftStep::new(
            "mkfs.ext4 /dev/sdb1",
            "Erase the second disk.",
        )]);
        let card = fix_card(&floor_only);
        assert_eq!(card.primary, None);
        assert_eq!(card.secondary, "Got it");
        assert_eq!(
            card.steps,
            vec![(
                format!("Erase the second disk. {}", repair::FLOOR_GUIDANCE),
                None
            )]
        );

        let mixed = plan(vec![
            DraftStep::new("dd if=/dev/zero of=/dev/sda", "Wipe the disk."),
            DraftStep::new("ipconfig /flushdns", "Clear the list."),
            DraftStep::new("pkexec apt-get clean", "Delete old installer copies."),
        ]);
        let card = fix_card(&mixed);
        assert_eq!(card.primary, Some(FIX_PRIMARY));
        assert!(card.hard, "a card with a hard step is the hard family");
        assert_eq!(card.steps[0].1, None, "the floor step shows no command");
        assert_eq!(
            card.steps[1],
            (
                "Clear the list.".to_string(),
                Some("ipconfig /flushdns".to_string())
            )
        );
        assert_eq!(
            card.steps[2].0,
            "Delete old installer copies. I'll ask you before this step."
        );
        assert_eq!(
            card.sections.iter().map(|(h, _)| *h).collect::<Vec<_>>(),
            vec!["What's wrong", "What I'll do", "Why", "How to undo", "Risk"]
        );
        assert!(card.sections.iter().all(|(_, t)| !t.trim().is_empty()));
        assert_eq!(mixed.steps[0].class, StepClass::HardFloor);
    }

    #[test]
    fn fix_it_with_no_grok_build_stops_at_the_first_step_and_says_so() {
        let (_pin, root) = pinned("fix-cabin-nogb");
        let _grok = NoGrok::set(&root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.harness.fixes.proposals = vec![plan(vec![DraftStep::new(
            "ipconfig /flushdns",
            "Clear the list.",
        )])];
        cabin.apply_fix(0);
        assert!(cabin.harness.fixes.proposals.is_empty());
        let mut want = vec![(repair::RESTORE_TOOL.to_string(), "restore".to_string())];
        want.extend(snapshot_spans());
        want.extend([
            (repair::REPAIR_TOOL.into(), "allow".into()),
            (repair::REPAIR_TOOL.into(), "deny".into()),
        ]);
        assert_eq!(tools(&root), want);
        assert_eq!(
            cabin.messages.last().unwrap().1,
            "A step didn't work (Clear the list), so I stopped there. Undo fix puts back what I changed."
        );
        assert!(!cabin.harness.fixes.busy());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_fix_step_rechecks_after_grok_builds_reply_and_undo_puts_the_file_back() {
        let (_pin, root) = pinned("fix-cabin");
        let mut cabin = Cabin::quiet_for_test();
        let conf = root.join("resolved.conf");
        let before: &[u8] = b"[Resolve]\r\nDNS=9.9.9.9\r\n";
        std::fs::write(&conf, before).unwrap();
        let p = plan(vec![DraftStep::new(
            "ipconfig /flushdns",
            "Clear the list.",
        )
        .touching(&[&conf])]);
        let ctx = repair::ApplyCtx {
            config_dir: &root,
            session_id: "session",
            os: repair::Os::Linux,
            attended: true,
            access: cabin.access_mode(),
            has_bin: &|_: &str| false,
        };
        let mut run = repair::ApplyRun::begin(&ctx, p).unwrap();
        assert!(matches!(run.next_action(), repair::Action::Gb { .. }));
        cabin.harness.fixes.run = Some(run);
        // What the fix did, then Grok Build's reply to the step.
        std::fs::write(&conf, b"changed").unwrap();
        cabin.running = true;
        cabin.poll_fix();
        assert!(cabin.harness.fixes.gb_started);
        cabin.running = false;
        cabin
            .live_mut()
            .push(("assistant".into(), "Ran it.\nSTEP_OK".into()));
        cabin.poll_fix();
        wait_fix(&mut cabin);
        // No System state grant: the re-check can't run, so it is not a fix.
        let last = cabin.messages.last().unwrap().1.clone();
        assert!(
            last.starts_with("That didn't fix it. I checked again: To check your computer"),
            "{last}"
        );
        let spans = hx::read_spans(&root, "session").unwrap();
        assert_eq!(
            spans
                .iter()
                .find(|s| s.tool == hx::VERIFY_TOOL)
                .map(|s| s.result.as_str()),
            Some("fail")
        );
        assert!(hx::done_without_criteria(&spans).is_empty());
        cabin.undo_fix();
        assert_eq!(std::fs::read(&conf).unwrap(), before);
        assert_eq!(
            cabin.messages.last().unwrap().1,
            "Undone. I put back the one file this fix changed, exactly as it was."
        );
        assert!(cabin.harness.fixes.closed);
        let _ = std::fs::remove_dir_all(root);
    }

    // Fan fixes write Linux sysfs paths, so the fake hwmon tree needs Linux paths.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_fan_fix_rechecks_the_fans_and_undo_runs_its_own_step() {
        let (_pin, root) = pinned("fix-cabin-fans");
        let _grok = NoGrok::set(&root);
        let mut cabin = Cabin::quiet_for_test();
        let chip = root.join("hwmon").join("hwmon0");
        std::fs::create_dir_all(&chip).unwrap();
        std::fs::write(chip.join("pwm1"), "90\n").unwrap();
        std::fs::write(chip.join("pwm1_enable"), "5\n").unwrap();
        let channels = repair::pwm_channels(&root.join("hwmon"));
        let saved = repair::save_fans(&channels);
        let backend = repair::fan_backend(repair::Os::Linux, &|_: &str| false, channels);
        let hot = Finding {
            severity: Severity::Warning,
            plain: "Your CPU reached 96°C while gaming.".into(),
            detail: String::new(),
            probe: ProbeId::Fans,
        };
        let p = repair::full_speed_plan(hot, &backend, &saved, false).unwrap();
        let ctx = repair::ApplyCtx {
            config_dir: &root,
            session_id: "session",
            os: repair::Os::Linux,
            attended: true,
            access: cabin.access_mode(),
            has_bin: &|_: &str| false,
        };
        let run = repair::ApplyRun::begin(&ctx, p).unwrap();
        cabin.harness.fixes.run = Some(run);
        cabin.harness.fixes.fans = Some(FanFix {
            saved: saved.clone(),
            goal: repair::FanGoal::Full,
        });
        assert!(matches!(
            cabin.harness.fixes.run.as_mut().unwrap().next_action(),
            repair::Action::Gb { .. }
        ));
        // The step ran: both files read full speed, then Grok Build replied.
        std::fs::write(chip.join("pwm1_enable"), "1\n").unwrap();
        std::fs::write(chip.join("pwm1"), "255\n").unwrap();
        cabin.running = true;
        cabin.poll_fix();
        cabin.running = false;
        cabin
            .live_mut()
            .push(("assistant".into(), "Ran it.\nSTEP_OK".into()));
        cabin.poll_fix();
        assert_eq!(
            cabin.messages.last().unwrap().1,
            "Fixed. I checked again: Every fan reads full speed now. If anything seems off, press Undo fix."
        );
        // Undo is its own elevated step, never a file copy over sysfs.
        cabin.undo_fix();
        let c = chip.display();
        let undo = cabin.harness.fixes.run.as_ref().unwrap();
        assert_eq!(
            undo.plan.steps[0].command,
            format!("pkexec sh -c 'echo 5 > {c}/pwm1_enable'")
        );
        assert_eq!(
            cabin.harness.fixes.fans.as_ref().map(|f| f.goal),
            Some(repair::FanGoal::Saved)
        );
        // Undo on the undo leaves the fans where the saved settings put them.
        cabin.undo_fix();
        assert_eq!(cabin.messages.last().unwrap().1, FANS_BACK_NOTE);
        assert!(cabin.harness.fixes.closed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_hard_fix_step_parks_the_hard_card_and_halt_denies_it() {
        let (_pin, root) = pinned("fix-cabin-hard");
        let _grok = NoGrok::set(&root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.permission_mode = PermissionMode::AlwaysApprove;
        cabin.harness.access_full = true;
        cabin.harness.fixes.proposals = vec![plan(vec![DraftStep::new(
            "pkexec apt-get clean",
            "Delete old installer copies.",
        )])];
        cabin.apply_fix(0);
        let park = cabin
            .harness
            .park
            .clone()
            .expect("hard step parks under Always and Full");
        assert_eq!(park.source, super::super::harness_ui::ParkSource::Repair);
        assert_eq!(park.class, hx::HardClass::Delete);
        assert_eq!(park.action, "pkexec apt-get clean");
        // Enter never approves a hard card; Esc denies.
        assert_eq!(hx::hard_card_key(true, false, false), None);
        cabin.halt_hard_parks();
        assert!(cabin.harness.park.is_none());
        assert!(cabin.harness.fixes.run.as_ref().unwrap().is_done());
        let mut want = vec![(repair::RESTORE_TOOL.to_string(), "restore".to_string())];
        want.extend(snapshot_spans());
        want.extend([
            (repair::REPAIR_TOOL.into(), "park".into()),
            (repair::APPLY_TOOL.into(), "deny".into()),
            (repair::REPAIR_TOOL.into(), "deny".into()),
        ]);
        assert_eq!(tools(&root), want);
        let fix_lines: Vec<&String> = cabin
            .messages
            .iter()
            .filter(|(_, b)| b.contains("Undo fix"))
            .map(|(_, b)| b)
            .collect();
        assert_eq!(
            fix_lines,
            vec!["I stopped the fix because you halted. Nothing more will run. Undo fix puts back what I changed."]
        );
        assert!(
            !cabin.messages.iter().any(|(r, _)| r == "user"),
            "nothing went to Grok Build"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_running_fix_shows_each_step_with_its_mark() {
        use repair::{ChecklistLine, StepMark};
        let line = |plain: &str, mark: StepMark| ChecklistLine {
            plain: plain.into(),
            mark,
        };
        let rows: Vec<String> = checklist_rows(&[
            line("Save a system snapshot first.", StepMark::Done),
            line("Restart the lookup service.", StepMark::Running),
            line("Delete old installers.", StepMark::NeedsOk),
            line("Clear the list.", StepMark::Pending),
            line("Reset the adapter.", StepMark::Failed),
            line("Check it worked.", StepMark::Skipped),
        ])
        .into_iter()
        .map(|(row, cmd)| {
            assert_eq!(cmd, None);
            row
        })
        .collect();
        assert_eq!(
            rows,
            vec![
                "✓ Save a system snapshot first.",
                "→ Restart the lookup service.",
                "→ Waiting for your OK: Delete old installers.",
                "· Clear the list.",
                "× Reset the adapter (didn't work)",
                "– Check it worked (didn't run)",
            ]
        );
        // Every mark is in the bundled fonts.
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let _ = crate::theme::test_pass(&ctx, Default::default(), |_| {});
        let font = egui::FontId::proportional(crate::theme::TYPE_BODY);
        for c in "✓→·×–".chars() {
            assert!(ctx.fonts_mut(|f| f.has_glyph(&font, c)), "{c}");
        }
    }
}
