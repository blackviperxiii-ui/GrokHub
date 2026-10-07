//! Spike-8b: `/diagnose` and the "something's wrong with my computer" intent.
//! The cabin runs the read-only probes (`grokhub_agent::repair`) off the UI
//! thread and posts the plain findings as a chat line. No new chrome (D2).
//! Without the `system_state` grant no probe runs and the answer is the ask.
//!
//! Spike-9: findings with a safe fix get one proposal card each on the
//! approval stack (What's wrong / What I'll do / Why / How to undo / Risk).
//! Fix it is a click, never Enter. The cabin never runs a step itself: soft
//! steps go to Grok Build one at a time under the pill, hard ones park the
//! hard card, and hard-floor steps are only guidance. Then the finding's probe
//! runs again, and the result card offers Undo fix (`UndoAsk::from_click`).

use super::*;
use grokhub_agent::harness as hx;
use grokhub_agent::repair;

/// A diagnose answer and the fixes it found.
pub(super) type DiagnoseDone = (String, Vec<repair::FixPlan>);

/// Fix proposals and the fix being applied.
#[derive(Debug, Default)]
pub(super) struct FixUi {
    /// One card per plan, worst finding first. Hidden while a fix runs.
    pub proposals: Vec<repair::FixPlan>,
    pub run: Option<repair::ApplyRun>,
    /// Grok Build picked up the step this run is waiting on.
    pub gb_started: bool,
    /// The re-check's report on its way.
    pub verify_rx: Option<mpsc::Receiver<repair::DiagnoseReport>>,
    /// The result card was closed (Done or Undo fix).
    pub closed: bool,
    /// The result went to the chat once.
    pub announced: bool,
}

impl FixUi {
    pub fn busy(&self) -> bool {
        self.verify_rx.is_some() || self.run.as_ref().is_some_and(|r| !r.is_done())
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
}

pub(super) const FIX_EYEBROW: &str = "Fix proposal";
pub(super) const FIX_PRIMARY: &str = "Fix it";
pub(super) const UNDO_PRIMARY: &str = "Undo fix";
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
        sections: plan.card_sections().iter().map(|(h, t)| (*h, t.to_string())).collect(),
        steps,
        primary: runnable.then_some(FIX_PRIMARY),
        secondary: if runnable { "Not now" } else { "Got it" },
        hard: plan.has_hard(),
    }
}

impl Cabin {
    /// A typed message that means "check my computer": the cabin answers it
    /// with diagnose instead of the model. True when it took the message.
    pub(super) fn try_diagnose_intent(&mut self, text: &str) -> bool {
        if self.running || self.scheduled_perm || self.harness.diagnose_rx.is_some() {
            return false;
        }
        let Some(probes) = repair::probes_for_intent(text, repair::Os::current()) else {
            return false;
        };
        self.touch();
        self.live_mut().push(("user".into(), text.to_string()));
        self.stamp_current_access();
        self.persist();
        self.run_diagnose(probes, false);
        true
    }

    /// Run `probes` (every one for this OS when empty). `slash` marks the
    /// answer as a slash result; an intent answer is a plain reply.
    pub(super) fn run_diagnose(&mut self, probes: Vec<repair::ProbeId>, slash: bool) {
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
            };
            let report = repair::diagnose(&ctx, &probes);
            let plans = repair::plans_for(&report.findings, os, &repair::on_path);
            let _ = tx.send((repair::report_text(&report), plans));
        });
    }

    pub(super) fn poll_diagnose(&mut self) {
        self.poll_fix();
        let Some(rx) = self.harness.diagnose_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((body, plans)) => {
                let body = if self.harness.diagnose_slash { mark_slash_result(&body) } else { body };
                self.live_mut().push(("assistant".into(), body));
                self.status.clear();
                self.stamp_current_access();
                self.persist();
                if !self.harness.fixes.busy() {
                    self.harness.fixes = FixUi { proposals: plans, ..FixUi::default() };
                }
            }
            Err(mpsc::TryRecvError::Empty) => self.harness.diagnose_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => self.status = "Couldn't finish checking your computer".into(),
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
            repair::Action::Gb { prompt, command, approved } => {
                if approved {
                    // Approved on the hard card: Grok's own Allow card asks once.
                    self.harness.oneshot = Some(super::harness_ui::OneShot {
                        action: Some(command),
                        restore: self.permission_mode,
                        started: false,
                    });
                    self.permission_mode = PermissionMode::Ask;
                }
                self.harness.fixes.gb_started = false;
                self.send_chat(prompt.clone());
                let queued = self.followup_queue.iter().any(|q| q == &prompt);
                if !self.running && !queued {
                    // Grok Build didn't take the step (not installed, not signed in).
                    let why = format!("{STEP_FAILED_NOTE} {}", self.status);
                    if let Some(run) = self.harness.fixes.run.as_mut() {
                        run.step_finished(&why);
                    }
                    self.advance_fix();
                    return;
                }
                self.status = "Fixing… if your computer asks for your password, type it yourself".into();
            }
            repair::Action::Park { class, command, .. } => {
                self.park_repair(class, command);
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
        let waiting = self.harness.fixes.run.as_ref().is_some_and(|r| r.awaiting_gb());
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
        let text = run.undo(hx::UndoAsk::from_click());
        self.harness.fixes.closed = true;
        self.live_mut().push(("assistant".into(), text));
        self.persist();
    }

    /// Proposal cards, or the running fix, or its result with Undo fix.
    pub(super) fn paint_fix_cards(&mut self, ui: &mut egui::Ui) {
        let running = self.running;
        if let Some(run) = self.harness.fixes.run.as_ref() {
            if !run.is_done() {
                let note = "One step at a time. If your computer asks for your password, type it yourself; I never do.";
                let card = FixCard { sections: vec![], steps: vec![], primary: None, secondary: "", hard: false };
                fix_card_ui(ui, ("fix-running", run.id.clone()), "Fixing", &run.plan.what, note, &card, running);
                return;
            }
            if !self.harness.fixes.closed && !run.undone {
                let eyebrow = if run.verified == Some(true) { "Fixed" } else { "Fix result" };
                let card = FixCard { sections: vec![], steps: vec![], primary: Some(UNDO_PRIMARY), secondary: "Done", hard: false };
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
    let stroke = if card.hard { crate::theme::fg() } else { crate::theme::border() };
    ui.add_space(8.0);
    let card_id = egui::Id::new(("fix-card", id.0, id.1));
    let enter_t = crate::motion::approval_enter_t(ui, card_id, true);
    let slot = ui.available_rect_before_wrap().translate(egui::vec2(0.0, crate::motion::approval_y(enter_t, false)));
    let mut hit = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| {
        ui.multiply_opacity(enter_t.clamp(0.0, 1.0));
        let inner = super::harness_ui::approval_card_inner(ui.available_width(), stroke_w);
        let muted = |t: &str, size: f32| egui::Label::new(RichText::new(t).size(size).color(crate::theme::muted())).wrap();
        let strong = |t: &str, size: f32| egui::Label::new(RichText::new(t).size(size).color(crate::theme::fg())).wrap();
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
                ui.add(egui::Label::new(RichText::new(title).size(14.0).strong().color(crate::theme::fg())).wrap());
                for (heading, text) in &card.sections {
                    ui.add_space(6.0);
                    ui.add(muted(heading, 12.0));
                    ui.add(strong(text, 13.0));
                }
                if !card.steps.is_empty() {
                    ui.add_space(6.0);
                    ui.add(muted("Steps", 12.0));
                    for (line, cmd) in &card.steps {
                        ui.add(strong(&format!("• {line}"), 13.0));
                        if let Some(cmd) = cmd {
                            ui.add(egui::Label::new(RichText::new(cmd).size(12.0).monospace().color(crate::theme::muted())).wrap());
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
                        } else if !card.secondary.is_empty() && crate::cards::ghost_pill(ui, card.secondary) {
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
            std::env::set_var("GROKHUB_GROK", root.join("missing-grok"));
            Self
        }
    }
    impl Drop for NoGrok {
        fn drop(&mut self) {
            std::env::remove_var("GROKHUB_GROK");
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
        hx::read_spans(root, "session").unwrap().into_iter().map(|s| (s.tool, s.decision)).collect()
    }

    fn wait_fix(cabin: &mut Cabin) {
        let start = std::time::Instant::now();
        while cabin.harness.fixes.verify_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(4) {
            cabin.poll_fix();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_hard_floor_step_is_never_an_apply_button() {
        let floor_only = plan(vec![DraftStep::new("mkfs.ext4 /dev/sdb1", "Erase the second disk.")]);
        let card = fix_card(&floor_only);
        assert_eq!(card.primary, None);
        assert_eq!(card.secondary, "Got it");
        assert_eq!(card.steps, vec![(format!("Erase the second disk. {}", repair::FLOOR_GUIDANCE), None)]);

        let mixed = plan(vec![
            DraftStep::new("dd if=/dev/zero of=/dev/sda", "Wipe the disk."),
            DraftStep::new("ipconfig /flushdns", "Clear the list."),
            DraftStep::new("pkexec apt-get clean", "Delete old installer copies."),
        ]);
        let card = fix_card(&mixed);
        assert_eq!(card.primary, Some(FIX_PRIMARY));
        assert!(card.hard, "a card with a hard step is the hard family");
        assert_eq!(card.steps[0].1, None, "the floor step shows no command");
        assert_eq!(card.steps[1], ("Clear the list.".to_string(), Some("ipconfig /flushdns".to_string())));
        assert_eq!(card.steps[2].0, "Delete old installer copies. I'll ask you before this step.");
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
        cabin.harness.fixes.proposals = vec![plan(vec![DraftStep::new("ipconfig /flushdns", "Clear the list.")])];
        cabin.apply_fix(0);
        assert!(cabin.harness.fixes.proposals.is_empty());
        assert_eq!(
            tools(&root),
            vec![
                (repair::RESTORE_TOOL.into(), "restore".into()),
                (repair::REPAIR_TOOL.into(), "allow".into()),
                (repair::REPAIR_TOOL.into(), "deny".into()),
            ]
        );
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
        let p = plan(vec![DraftStep::new("ipconfig /flushdns", "Clear the list.").touching(&[&conf])]);
        let ctx = repair::ApplyCtx {
            config_dir: &root,
            session_id: "session",
            os: repair::Os::current(),
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
        cabin.live_mut().push(("assistant".into(), "Ran it.\nSTEP_OK".into()));
        cabin.poll_fix();
        wait_fix(&mut cabin);
        // No System state grant: the re-check can't run, so it is not a fix.
        let last = cabin.messages.last().unwrap().1.clone();
        assert!(last.starts_with("That didn't fix it. I checked again: To check your computer"), "{last}");
        let spans = hx::read_spans(&root, "session").unwrap();
        assert_eq!(spans.iter().find(|s| s.tool == hx::VERIFY_TOOL).map(|s| s.result.as_str()), Some("fail"));
        assert!(hx::done_without_criteria(&spans).is_empty());
        cabin.undo_fix();
        assert_eq!(std::fs::read(&conf).unwrap(), before);
        assert_eq!(cabin.messages.last().unwrap().1, "Undone. I put back the one file this fix changed, exactly as it was.");
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
        cabin.harness.fixes.proposals = vec![plan(vec![DraftStep::new("pkexec apt-get clean", "Delete old installer copies.")])];
        cabin.apply_fix(0);
        let park = cabin.harness.park.clone().expect("hard step parks under Always and Full");
        assert_eq!(park.source, super::super::harness_ui::ParkSource::Repair);
        assert_eq!(park.class, hx::HardClass::Delete);
        assert_eq!(park.action, "pkexec apt-get clean");
        // Enter never approves a hard card; Esc denies.
        assert_eq!(hx::hard_card_key(true, false, false), None);
        cabin.halt_hard_parks();
        assert!(cabin.harness.park.is_none());
        assert!(cabin.harness.fixes.run.as_ref().unwrap().is_done());
        let decisions: Vec<(String, String)> = tools(&root);
        assert_eq!(
            decisions,
            vec![
                (repair::RESTORE_TOOL.into(), "restore".into()),
                (repair::REPAIR_TOOL.into(), "park".into()),
                (repair::APPLY_TOOL.into(), "deny".into()),
                (repair::REPAIR_TOOL.into(), "deny".into()),
            ]
        );
        let fix_lines: Vec<&String> = cabin.messages.iter().filter(|(_, b)| b.contains("Undo fix")).map(|(_, b)| b).collect();
        assert_eq!(
            fix_lines,
            vec!["I stopped the fix because you halted. Nothing more will run. Undo fix puts back what I changed."]
        );
        assert!(!cabin.messages.iter().any(|(r, _)| r == "user"), "nothing went to Grok Build");
        let _ = std::fs::remove_dir_all(root);
    }
}
