//! Code tab → Push and PR, in the cabin. Every push and PR goes through the
//! shared push guard in `grokhub_code::ship`, the same code the desktop
//! shell's Code tab runs, so both apps make the same call on the same repo.
//!
//! Fixed rules, not settings: never push `main` or the repo's default
//! branch, never force-push. The guard runs again when the command runs, so
//! a stale panel can't skip it. The cabin reads the Push and PR settings
//! from the shell's settings file and never writes them.
//!
//! Every git and gh call runs off the UI thread; results come back on a
//! channel. Output, notes and activity lines pass through the project's held
//! secret values before they are shown.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui::{self, RichText};
use grokhub_code::activity::{self, Entry, Kind};
use grokhub_code::secrets;
use grokhub_code::ship::{self, Action, CodeShip, Plan, ShipMode};

pub(super) const SHIP_TITLE: &str = "Push and PR";
pub(super) const SHIP_CONFIRM_PUSH: &str = "Push this branch?";
pub(super) const SHIP_CONFIRM_PR: &str = "Open this PR?";
pub(super) const SHIP_RULES: &str =
    "Never pushes main or the default branch, never force-pushes, never deletes a branch.";
pub(super) const SHIP_SETTINGS_NOTE: &str = "Change these in the desktop app's Code settings.";
pub(super) const SHIP_ACTIVITY: &str = "Activity";
pub(super) const SHIP_NO_ACTIVITY: &str = "No pushes or PRs from this tab yet.";
pub(super) const BRANCH_HINT: &str = "code/fix-typo";

/// The Push and PR settings, read fresh for every plan and run so a change
/// made in the desktop app counts at once. Read only here.
pub(super) fn settings(config_dir: &Path) -> CodeShip {
    ship::load_from_shell_json(config_dir)
}

/// The mode line over the buttons.
pub(super) fn mode_note(cfg: &CodeShip) -> String {
    match cfg.mode {
        ShipMode::AutoBeta => format!(
            "Auto-beta: a branch made here pushes into {} without asking. Everything else asks.",
            cfg.auto_base
        ),
        ShipMode::Ask => "Ask: every push and PR shows the exact command first.".into(),
    }
}

/// Protected branches as the settings line shows them: main always, then
/// the extra names, once each.
pub(super) fn protected_line(cfg: &CodeShip) -> String {
    let mut names = vec![ship::ALWAYS_PROTECTED.to_string()];
    for n in &cfg.extra_protected {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    format!(
        "Protected: {}, and the repo's default branch. PRs open into {}.",
        names.join(", "),
        cfg.auto_base
    )
}

pub(super) fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Push => "Push",
        Kind::Pr => "PR",
        Kind::Branch => "Branch",
        Kind::Blocked => "Blocked",
        Kind::Deploy => "Deploy",
        Kind::Secret => "Secret",
        Kind::Edit => "Edit",
        Kind::Terminal => "Terminal",
    }
}

/// Work for the worker thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ShipJob {
    /// Read both plans and the activity log; `then` is a press waiting on them.
    Refresh {
        then: Option<Action>,
    },
    /// Push or open the PR. `expected` is the command the confirm showed.
    Run {
        action: Action,
        confirmed: bool,
        expected: Option<String>,
    },
    NewBranch {
        name: String,
    },
}

/// What the worker sends back. Every string in it is already redacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ShipOut {
    Plans {
        push: Plan,
        pr: Plan,
        activity: Vec<Entry>,
        cfg: CodeShip,
        then: Option<Action>,
    },
    Shipped(Result<(String, String), String>),
    Branch(Result<String, String>),
}

fn clean_plan(mut p: Plan, held: &[String]) -> Plan {
    p.command = secrets::redact(&p.command, held);
    p.blocked = p.blocked.map(|b| secrets::redact(&b, held));
    p
}

fn clean_entry(mut e: Entry, held: &[String]) -> Entry {
    e.command = secrets::redact(&e.command, held);
    e.summary = secrets::redact(&e.summary, held);
    e.branch = secrets::redact(&e.branch, held);
    e
}

impl ShipJob {
    /// Runs on the worker thread. `held` reads the project's secret values
    /// (the keychain in the app), only to hide them in what comes back.
    pub(super) fn run(
        self,
        config_dir: &Path,
        ws: &Path,
        held: &dyn Fn() -> Vec<String>,
    ) -> ShipOut {
        let cfg = settings(config_dir);
        let held = held();
        match self {
            ShipJob::Refresh { then } => ShipOut::Plans {
                push: clean_plan(ship::plan(config_dir, ws, &cfg, Action::Push), &held),
                pr: clean_plan(ship::plan(config_dir, ws, &cfg, Action::Pr), &held),
                activity: activity::list(config_dir, activity::SHOW)
                    .into_iter()
                    .map(|e| clean_entry(e, &held))
                    .collect(),
                cfg,
                then,
            },
            ShipJob::Run {
                action,
                confirmed,
                expected,
            } => ShipOut::Shipped(
                ship::ship(config_dir, ws, &cfg, action, confirmed, expected.as_deref())
                    .map(|s| {
                        (
                            secrets::redact(&s.summary, &held),
                            secrets::redact(&s.output, &held),
                        )
                    })
                    .map_err(|e| secrets::redact(&e, &held)),
            ),
            ShipJob::NewBranch { name } => ShipOut::Branch(
                ship::new_branch(config_dir, ws, &cfg, &name)
                    .map_err(|e| secrets::redact(&e, &held)),
            ),
        }
    }
}

/// The Push and PR section between frames.
#[derive(Default)]
pub(super) struct ShipPanel {
    pub(super) push: Option<Plan>,
    pub(super) pr: Option<Plan>,
    pub(super) cfg: Option<CodeShip>,
    /// The plan whose command the confirm card shows.
    pub(super) confirming: Option<Plan>,
    pub(super) branch_name: String,
    pub(super) busy: bool,
    pub(super) note: Option<String>,
    pub(super) output: String,
    pub(super) activity: Vec<Entry>,
    /// The workspace the panel was read for; another one starts it over.
    ws: Option<PathBuf>,
    rx: Option<mpsc::Receiver<ShipOut>>,
}

impl ShipPanel {
    /// The tab just showed (or the project changed): read everything again,
    /// so a confirm left open for another branch's command goes too.
    pub(super) fn shown(&mut self) -> Option<ShipJob> {
        if self.busy {
            return None;
        }
        self.busy = true;
        Some(ShipJob::Refresh { then: None })
    }

    /// Push or Open PR pressed: read the plans fresh, then block, ask or run.
    pub(super) fn press(&mut self, action: Action) -> Option<ShipJob> {
        if self.busy {
            return None;
        }
        self.busy = true;
        self.note = None;
        self.output.clear();
        Some(ShipJob::Refresh { then: Some(action) })
    }

    /// The confirm card's Push or Open PR: runs only the command it showed.
    pub(super) fn confirm(&mut self) -> Option<ShipJob> {
        if self.busy {
            return None;
        }
        let plan = self.confirming.take()?;
        self.busy = true;
        Some(ShipJob::Run {
            action: plan.action,
            confirmed: true,
            expected: Some(plan.command),
        })
    }

    pub(super) fn cancel(&mut self) {
        self.confirming = None;
    }

    pub(super) fn make_branch(&mut self) -> Option<ShipJob> {
        let name = self.branch_name.trim().to_string();
        if name.is_empty() || self.busy {
            return None;
        }
        self.busy = true;
        self.note = None;
        Some(ShipJob::NewBranch { name })
    }

    /// One worker answer. Returns the next job, if the answer starts one.
    pub(super) fn apply(&mut self, out: ShipOut) -> Option<ShipJob> {
        match out {
            ShipOut::Plans {
                push,
                pr,
                activity,
                cfg,
                then,
            } => {
                // A card for a command that would no longer run goes away.
                if let Some(open) = &self.confirming {
                    let now = match open.action {
                        Action::Push => &push,
                        Action::Pr => &pr,
                    };
                    if now.command != open.command || now.blocked.is_some() {
                        self.confirming = None;
                    }
                }
                self.push = Some(push);
                self.pr = Some(pr);
                self.activity = activity;
                self.cfg = Some(cfg);
                let Some(action) = then else {
                    self.busy = false;
                    return None;
                };
                let plan = match action {
                    Action::Push => self.push.clone(),
                    Action::Pr => self.pr.clone(),
                }?;
                if let Some(why) = plan.blocked {
                    self.note = Some(why);
                    self.busy = false;
                    return None;
                }
                if plan.needs_confirm {
                    self.confirming = Some(plan);
                    self.busy = false;
                    return None;
                }
                // The guard said no confirm (Auto-beta); it checks again when it runs.
                Some(ShipJob::Run {
                    action,
                    confirmed: false,
                    expected: None,
                })
            }
            ShipOut::Shipped(res) => {
                match res {
                    Ok((summary, output)) => {
                        self.note = Some(summary);
                        self.output = output;
                    }
                    Err(e) => self.note = Some(e),
                }
                Some(ShipJob::Refresh { then: None })
            }
            ShipOut::Branch(res) => {
                match res {
                    Ok(made) => {
                        // An open card shows the command for the branch just left.
                        self.confirming = None;
                        self.branch_name.clear();
                        self.note = Some(format!("On {made} now."));
                    }
                    Err(e) => self.note = Some(e),
                }
                Some(ShipJob::Refresh { then: None })
            }
        }
    }

    fn spawn(&mut self, job: ShipJob, ctx: &egui::Context, config_dir: &Path, ws: &Path) {
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        let config = config_dir.to_path_buf();
        let ws = ws.to_path_buf();
        std::thread::spawn(move || {
            let out = job.run(&config, &ws, &|| secrets::held_for(&config, &ws));
            let _ = tx.send(out);
            ctx.request_repaint();
        });
    }

    /// Start a job if no other is in flight; one runs at a time.
    fn start(&mut self, job: Option<ShipJob>, ctx: &egui::Context, config_dir: &Path, ws: &Path) {
        if let Some(job) = job {
            if self.rx.is_none() {
                self.spawn(job, ctx, config_dir, ws);
            }
        }
    }

    /// Take a finished answer, and start what it leads to.
    fn poll(&mut self, ctx: &egui::Context, config_dir: &Path, ws: &Path) {
        let Some(rx) = self.rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(out) => {
                let next = self.apply(out);
                self.start(next, ctx, config_dir, ws);
            }
            Err(mpsc::TryRecvError::Empty) => self.rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.busy = false;
                self.note = Some("That stopped before it finished. Try again.".into());
            }
        }
    }

    /// The section: mode and rules, New branch, Push and Open PR, the confirm
    /// card, the result and the activity log.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, config_dir: &Path, ws: &Path) {
        let ctx = ui.ctx().clone();
        if self.ws.as_deref() != Some(ws) {
            *self = Self {
                ws: Some(ws.to_path_buf()),
                ..Self::default()
            };
            let job = self.shown();
            self.start(job, &ctx, config_dir, ws);
        }
        self.poll(&ctx, config_dir, ws);

        crate::cards::section_heading(ui, SHIP_TITLE);
        let cfg = self.cfg.clone().unwrap_or_default();
        muted(ui, &format!("{} {SHIP_RULES}", mode_note(&cfg)));
        muted(
            ui,
            &format!("{} {SHIP_SETTINGS_NOTE}", protected_line(&cfg)),
        );
        ui.add_space(8.0);

        let mut job = None;
        ui.horizontal(|ui| {
            ui.add_enabled(
                !self.busy,
                egui::TextEdit::singleline(&mut self.branch_name)
                    .hint_text(BRANCH_HINT)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(220.0),
            );
            if !self.branch_name.trim().is_empty()
                && !self.busy
                && crate::cards::ghost_pill(ui, "Make branch")
            {
                job = self.make_branch();
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let push_label = if self.busy {
                "Working…".to_string()
            } else {
                match self.push.as_ref().and_then(|p| p.branch.as_deref()) {
                    Some(b) => format!("Push {b}"),
                    None => "Push".into(),
                }
            };
            if crate::cards::white_pill(ui, &push_label) && self.push.is_some() {
                job = self.press(Action::Push);
            }
            match self.pr.as_ref().and_then(|p| p.pr_url.clone()) {
                Some(url) => muted(ui, &format!("PR open: {url}")),
                None => {
                    let base = self
                        .pr
                        .as_ref()
                        .map(|p| p.auto_base.clone())
                        .unwrap_or(cfg.auto_base.clone());
                    if crate::cards::ghost_pill(ui, &format!("Open PR into {base}"))
                        && self.pr.is_some()
                    {
                        job = self.press(Action::Pr);
                    }
                }
            }
        });
        if let Some(plan) = self.confirming.clone() {
            ui.add_space(8.0);
            confirm_card(ui, &plan, &mut |yes| {
                if yes {
                    job = self.confirm();
                } else {
                    self.cancel();
                }
            });
        }
        if let Some(note) = &self.note {
            ui.add_space(8.0);
            ui.label(
                RichText::new(note)
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::fg()),
            );
        }
        if !self.output.is_empty() {
            ui.add_space(8.0);
            mono_block(ui, &self.output);
        }
        self.start(job, &ctx, config_dir, ws);

        crate::cards::section_heading(ui, SHIP_ACTIVITY);
        if self.activity.is_empty() {
            muted(ui, SHIP_NO_ACTIVITY);
        }
        for e in &self.activity {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(kind_label(e.kind))
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::fg()),
                );
                ui.label(
                    RichText::new(&e.branch)
                        .monospace()
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::fg()),
                );
                muted(ui, &e.summary);
            });
            if !e.command.is_empty() {
                ui.label(
                    RichText::new(&e.command)
                        .monospace()
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                );
            }
            ui.add_space(4.0);
        }
    }
}

fn muted(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        )
        .wrap(),
    );
}

fn mono_block(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(crate::theme::elevated())
        .corner_radius(crate::theme::CARD_RADIUS)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(text).monospace().color(crate::theme::fg())).wrap(),
            );
        });
}

/// The confirm card: the question, the exact command, then the action and Cancel.
fn confirm_card(ui: &mut egui::Ui, plan: &Plan, on: &mut dyn FnMut(bool)) {
    let (question, primary) = match plan.action {
        Action::Push => (SHIP_CONFIRM_PUSH, "Push"),
        Action::Pr => (SHIP_CONFIRM_PR, "Open PR"),
    };
    egui::Frame::NONE
        .fill(crate::theme::elevated())
        .corner_radius(crate::theme::CARD_RADIUS)
        .stroke(egui::Stroke::new(2.0_f32, crate::theme::border_strong()))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.label(
                RichText::new(question)
                    .size(crate::theme::TYPE_UI)
                    .color(crate::theme::fg()),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(&plan.command)
                    .monospace()
                    .color(crate::theme::fg()),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if crate::cards::white_pill(ui, primary) {
                    on(true);
                }
                if crate::cards::ghost_pill(ui, "Cancel") {
                    on(false);
                }
            });
        });
}

#[cfg(test)]
#[path = "code_ship_ui_tests.rs"]
mod tests;
