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
//! secret values before they are shown. The activity log itself shows in
//! the Code tab's Activity section.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui::{self, RichText};
use grokhub_code::activity::Entry;
use grokhub_code::secrets;
use grokhub_code::ship::{self, Action, CodeShip, Plan, ShipMode};

use super::confirm::{paint_confirm_sheet, ConfirmAct, ConfirmSpec};

pub(super) const SHIP_TITLE: &str = "Push and PR";
pub(super) const SHIP_CONFIRM_PUSH: &str = "Push this branch?";
pub(super) const SHIP_CONFIRM_PR: &str = "Open this PR?";
pub(super) const SHIP_RULES: &str =
    "Never pushes main or the default branch, never force-pushes, never deletes a branch.";
pub(super) const SHIP_SETTINGS_NOTE: &str = "Change these in the desktop app's Code settings.";
pub(super) const BRANCH_HINT: &str = "code/fix-typo";

/// The push and PR confirms: the exact command shows under each.
pub(super) fn confirm_spec(action: Action) -> ConfirmSpec {
    match action {
        Action::Push => ConfirmSpec {
            title: SHIP_CONFIRM_PUSH,
            consequence: "It sends this branch to origin. Never main or the default branch, never a force-push.",
            primary: "Push",
            danger: false,
        },
        Action::Pr => ConfirmSpec {
            title: SHIP_CONFIRM_PR,
            consequence: "It opens a pull request from this branch on GitHub, titled from its commits.",
            primary: "Open PR",
            danger: false,
        },
    }
}

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

/// Work for the worker thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ShipJob {
    /// Read both plans; `then` is a press waiting on them.
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

/// The push and PR commands as the guard builds them, before the project's
/// secret values are hidden. A confirm sends its one back to the guard, which
/// compares it with the command it is about to run; the redacted text on the
/// card would never match. Never drawn and never printed.
#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Real {
    push: String,
    pr: String,
}

impl Real {
    /// The command for `action`, or `shown` when none was read.
    fn of(&self, action: Action, shown: &str) -> String {
        let real = match action {
            Action::Push => &self.push,
            Action::Pr => &self.pr,
        };
        if real.is_empty() {
            shown.to_string()
        } else {
            real.clone()
        }
    }
}

impl std::fmt::Debug for Real {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Real(..)")
    }
}

/// What the worker sends back. Every string in it is already redacted,
/// except `real`, which is never shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ShipOut {
    Plans {
        push: Box<Plan>,
        pr: Box<Plan>,
        cfg: CodeShip,
        then: Option<Action>,
        real: Real,
    },
    Shipped(Result<(String, String), String>),
    Branch(Result<String, String>),
}

fn clean_plan(mut p: Plan, held: &[String]) -> Plan {
    p.command = secrets::redact(&p.command, held);
    p.blocked = p.blocked.map(|b| secrets::redact(&b, held));
    p
}

/// An activity row with the project's held secret values hidden.
pub(super) fn clean_entry(mut e: Entry, held: &[String]) -> Entry {
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
            ShipJob::Refresh { then } => {
                let push = ship::plan(config_dir, ws, &cfg, Action::Push);
                let pr = ship::plan(config_dir, ws, &cfg, Action::Pr);
                let real = Real {
                    push: push.command.clone(),
                    pr: pr.command.clone(),
                };
                ShipOut::Plans {
                    push: Box::new(clean_plan(push, &held)),
                    pr: Box::new(clean_plan(pr, &held)),
                    cfg,
                    then,
                    real,
                }
            }
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
    /// That card's command before redaction: what its confirm sends.
    confirm_real: String,
    /// The newest plans' commands before redaction.
    real: Real,
    /// The branch the pressed button named; `None` when it named none.
    pressed_branch: Option<String>,
    /// The repo was read with another branch checked out than the Push
    /// button names: read the plans again.
    reread: bool,
    pub(super) branch_name: String,
    pub(super) busy: bool,
    pub(super) note: Option<String>,
    pub(super) output: String,
    /// A push, PR or new branch finished: the tab reads the repo and activity again.
    pub(super) changed: bool,
    /// The workspace the panel was read for; another one starts it over.
    ws: Option<PathBuf>,
    rx: Option<mpsc::Receiver<ShipOut>>,
}

impl ShipPanel {
    /// The project changed, the page's Refresh was pressed or the repo was
    /// read on another branch: read everything again, so a confirm left open
    /// for another branch's command goes too.
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
        let shown = match action {
            Action::Push => &self.push,
            Action::Pr => &self.pr,
        };
        self.pressed_branch = shown.as_ref().and_then(|p| p.branch.clone());
        Some(ShipJob::Refresh { then: Some(action) })
    }

    /// The repo was just read with `branch` checked out. When the Push button
    /// names another one (it was switched in the terminal or by the agent),
    /// the plans are read again on the next frame.
    pub(super) fn saw_branch(&mut self, branch: Option<&str>) {
        if let Some(push) = &self.push {
            if push.branch.as_deref() != branch {
                self.reread = true;
            }
        }
    }

    /// The confirm card's Push or Open PR: runs only the command it showed.
    pub(super) fn confirm(&mut self) -> Option<ShipJob> {
        if self.busy {
            return None;
        }
        let plan = self.confirming.take()?;
        self.busy = true;
        let real = std::mem::take(&mut self.confirm_real);
        Some(ShipJob::Run {
            action: plan.action,
            confirmed: true,
            // The command before redaction when one was read: the guard
            // compares this with what it is about to run.
            expected: Some(if real.is_empty() { plan.command } else { real }),
        })
    }

    pub(super) fn cancel(&mut self) {
        self.confirming = None;
        self.confirm_real.clear();
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
                cfg,
                then,
                real,
            } => {
                // A card for a command that would no longer run goes away,
                // also when only the part hidden as [redacted] changed.
                if let Some(open) = &self.confirming {
                    let now = match open.action {
                        Action::Push => &push,
                        Action::Pr => &pr,
                    };
                    let same = now.command == open.command
                        && real.of(open.action, &now.command) == self.confirm_real;
                    if !same || now.blocked.is_some() {
                        self.confirming = None;
                        self.confirm_real.clear();
                    }
                }
                self.push = Some(*push);
                self.pr = Some(*pr);
                self.cfg = Some(cfg);
                self.real = real;
                let Some(action) = then else {
                    self.busy = false;
                    return None;
                };
                let pressed = self.pressed_branch.take();
                let plan = match action {
                    Action::Push => self.push.clone(),
                    Action::Pr => self.pr.clone(),
                }?;
                if let Some(why) = plan.blocked {
                    self.note = Some(why);
                    self.busy = false;
                    return None;
                }
                // The button named one branch and another is checked out
                // now: nothing runs or asks until the button says so too.
                if let (Some(was), Some(now)) = (&pressed, &plan.branch) {
                    if was != now {
                        self.note = Some(format!(
                            "You are on {now} now, not {was}. Check the button and press again."
                        ));
                        self.busy = false;
                        return None;
                    }
                }
                if plan.needs_confirm {
                    self.confirm_real = self.real.of(action, &plan.command);
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
                self.changed = true;
                Some(ShipJob::Refresh { then: None })
            }
            ShipOut::Branch(res) => {
                match res {
                    Ok(made) => {
                        // An open card shows the command for the branch just left.
                        self.confirming = None;
                        self.confirm_real.clear();
                        self.branch_name.clear();
                        self.note = Some(format!("On {made} now."));
                    }
                    Err(e) => self.note = Some(e),
                }
                self.changed = true;
                Some(ShipJob::Refresh { then: None })
            }
        }
    }

    /// The page's Refresh: read the plans again, unless something is running.
    pub(super) fn refresh(&mut self, ctx: &egui::Context, config_dir: &Path, ws: &Path) {
        let job = self.shown();
        self.start(job, ctx, config_dir, ws);
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
    /// card and the result.
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
        if self.reread && !self.busy {
            self.reread = false;
            let job = self.shown();
            self.start(job, &ctx, config_dir, ws);
        }

        crate::cards::section_label(ui, SHIP_TITLE);
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
            match paint_confirm_sheet(ui, confirm_spec(plan.action), &plan.command) {
                Some(ConfirmAct::Confirm) => job = self.confirm(),
                Some(ConfirmAct::Cancel) => self.cancel(),
                None => {}
            }
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
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(text).monospace().color(crate::theme::fg())).wrap(),
            );
        });
}

#[cfg(test)]
#[path = "code_ship_ui_tests.rs"]
mod tests;
