//! Code tab → Deploy: the project's deploy switch and command, and one Deploy
//! button with a confirm that shows the exact command.
//!
//! Everything goes through `grokhub_code::deploy`, the same code the desktop
//! app's Code tab uses, so the two share one settings file, one remembered
//! command and one activity log. Off until it's turned on. Ask confirms every
//! deploy; Auto-beta runs a remembered command without asking and asks again
//! when the command changes. The project's secrets are set in the child's
//! environment and hidden in the output before it reaches this panel.
//!
//! Every call (a plan reads the repo's remote, a run can take minutes) runs on
//! a worker thread and comes back through a channel, so the window never waits.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui::{self, RichText};
use grokhub_code::deploy::{self, Plan, Settings};
use grokhub_code::runs::RunResult;
use grokhub_code::secrets::Vault;
use grokhub_code::ship::{CodeShip, ShipMode};

pub(super) const DEPLOY_TITLE: &str = "Deploy";
pub(super) const DEPLOY_CONFIRM: &str = "Run this deploy command?";
pub(super) const DEPLOY_RULES: &str = "Your project's secrets are set as environment variables for the run and hidden in the output and the activity list.";
pub(super) const MODE_ASK: &str = "Ask: every deploy shows the exact command first.";
pub(super) const MODE_AUTO: &str =
    "Auto-beta: a remembered deploy runs without asking, and asks again if its command changes.";
pub(super) const SETTINGS_RULES: &str = "Off until you turn it on. Ask mode confirms every deploy and shows the exact command. In Auto-beta, a deploy you confirmed once runs without asking until its command changes.";
pub(super) const NOTHING_REMEMBERED: &str =
    "Nothing remembered yet. The next deploy will ask you to confirm the command.";
pub(super) const CUT_NOTE: &str = "\n… cut here; the output is too long to show in full.";
pub(super) const RUNNING: &str = "Running…";

/// What a worker sends back.
pub(super) enum Msg {
    Plan(Result<Plan, String>),
    Saved(Result<Settings, String>),
    /// One piece of output, already redacted.
    Chunk(String),
    Done(Result<RunResult, String>),
}

/// What pressing Deploy leads to, from the plan just read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Start {
    /// Off, or no command: say why and run nothing.
    Blocked(String),
    /// Show this exact command and wait for a yes.
    Confirm(String),
    /// Remembered in Auto-beta: run it without asking.
    Run(String),
}

pub(super) fn start_step(plan: &Plan) -> Start {
    if let Some(why) = &plan.blocked {
        return Start::Blocked(why.clone());
    }
    if plan.needs_confirm {
        Start::Confirm(plan.command.clone())
    } else {
        Start::Run(plan.command.clone())
    }
}

/// The line under the Deploy title.
pub(super) fn mode_line(plan: Option<&Plan>) -> String {
    match plan {
        Some(p) if !p.deploy_enabled => deploy::DISABLED.to_string(),
        Some(p) if p.mode == ShipMode::AutoBeta.id() => format!("{MODE_AUTO} {DEPLOY_RULES}"),
        _ => format!("{MODE_ASK} {DEPLOY_RULES}"),
    }
}

/// The note a finished run leaves.
pub(super) fn done_note(done: &RunResult) -> String {
    if done.exit == 0 {
        "Deploy finished.".into()
    } else {
        format!("Deploy exited with {}.", done.exit)
    }
}

/// Run the deploy on its own thread. `vault` holds the project's secret values
/// (the OS keychain in the app). Output chunks arrive already redacted.
#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_run_with(
    config_dir: PathBuf,
    repo_key: String,
    workspace: PathBuf,
    ship: CodeShip,
    command: String,
    confirmed: bool,
    vault: Box<dyn Vault + Send>,
    place: deploy::Where,
    repaint: Option<egui::Context>,
) -> mpsc::Receiver<Msg> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let chunk_tx = tx.clone();
        let chunk_ctx = repaint.clone();
        let done = deploy::run_with(
            &config_dir,
            &repo_key,
            &workspace,
            &ship,
            &command,
            confirmed,
            vault.as_ref(),
            &place,
            move |chunk| {
                let _ = chunk_tx.send(Msg::Chunk(chunk.to_string()));
                if let Some(ctx) = &chunk_ctx {
                    ctx.request_repaint();
                }
            },
        );
        let _ = tx.send(Msg::Done(done));
        if let Some(ctx) = &repaint {
            ctx.request_repaint();
        }
    });
    rx
}

/// Any other deploy call on its own thread: plan, save or forget.
fn spawn(
    repaint: Option<egui::Context>,
    job: impl FnOnce() -> Msg + Send + 'static,
) -> mpsc::Receiver<Msg> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(job());
        if let Some(ctx) = &repaint {
            ctx.request_repaint();
        }
    });
    rx
}

/// Where this project pushes, for the activity line.
fn place_of(workspace: &Path) -> deploy::Where {
    deploy::Where {
        host: grokhub_code::ship::push_host(workspace)
            .map(|h| h.id().to_string())
            .unwrap_or_else(|| "other".into()),
        branch: grokhub_code::ship::current_branch(workspace).unwrap_or_default(),
    }
}

fn key_of(workspace: &Path) -> String {
    grokhub_code::runs::repo_key(
        workspace,
        grokhub_code::repo::origin_url(workspace).as_deref(),
    )
}

/// What the Deploy section holds between frames.
#[derive(Default)]
pub(super) struct DeployPanel {
    pub(super) plan: Option<Plan>,
    /// The switch and command as typed, before Save.
    pub(super) draft_on: bool,
    pub(super) draft_command: String,
    /// The command a confirm is showing.
    pub(super) confirming: Option<String>,
    pub(super) busy: bool,
    /// A worker is reading or saving settings.
    pub(super) loading: bool,
    pub(super) note: Option<String>,
    pub(super) settings_note: Option<String>,
    pub(super) log: String,
    rx: Option<mpsc::Receiver<Msg>>,
    /// The workspace the plan was read for; another one reads it again.
    loaded_for: Option<PathBuf>,
    /// A run or save is out: read the plan again when it ends, for the
    /// remembered line and the saved command.
    refresh_after: bool,
    /// A deploy ended: the tab reads its activity list again.
    pub(super) changed: bool,
}

impl DeployPanel {
    fn watch(&mut self, rx: mpsc::Receiver<Msg>) {
        self.rx = Some(rx);
    }

    /// Read the plan again (on a worker). The Code settings block is read
    /// there too, so a mode changed in the desktop app shows here.
    pub(super) fn refresh(
        &mut self,
        config_dir: &Path,
        workspace: &Path,
        repaint: Option<egui::Context>,
    ) {
        if self.busy || self.loading {
            return;
        }
        self.loading = true;
        self.loaded_for = Some(workspace.to_path_buf());
        let (config, dir) = (config_dir.to_path_buf(), workspace.to_path_buf());
        let rx = spawn(repaint, move || {
            let ship = grokhub_code::ship::load_from_shell_json(&config);
            Msg::Plan(deploy::plan(&config, &dir, &ship))
        });
        self.watch(rx);
    }

    /// The page's Refresh: read the plan again on the next frame.
    pub(super) fn reload(&mut self) {
        self.loaded_for = None;
    }

    /// Take a plan read on a worker. The draft follows it.
    pub(super) fn take_plan(&mut self, plan: Plan) {
        self.draft_on = plan.deploy_enabled;
        self.draft_command = plan.command.clone();
        self.plan = Some(plan);
    }

    /// Deploy was pressed with this plan. Returns the command to run now, if any.
    pub(super) fn press(&mut self, plan: &Plan) -> Option<String> {
        self.note = None;
        match start_step(plan) {
            Start::Blocked(why) => {
                self.note = Some(why);
                None
            }
            Start::Confirm(cmd) => {
                self.confirming = Some(cmd);
                None
            }
            Start::Run(cmd) => Some(cmd),
        }
    }

    /// Cancel on the confirm: nothing runs.
    pub(super) fn cancel(&mut self) {
        self.confirming = None;
    }

    /// A run starts: the log clears and the button waits.
    pub(super) fn begin(&mut self) {
        self.confirming = None;
        self.busy = true;
        self.log.clear();
        self.note = None;
    }

    /// Apply one message from a worker.
    pub(super) fn apply(&mut self, msg: Msg) {
        match msg {
            Msg::Plan(Ok(plan)) => {
                self.loading = false;
                self.take_plan(plan);
            }
            Msg::Plan(Err(err)) => {
                self.loading = false;
                self.note = Some(err);
            }
            Msg::Saved(Ok(_)) => {
                self.loading = false;
                self.settings_note = Some("Saved.".into());
            }
            Msg::Saved(Err(err)) => {
                self.loading = false;
                self.settings_note = Some(err);
            }
            Msg::Chunk(text) => self.log.push_str(&text),
            Msg::Done(result) => {
                self.busy = false;
                self.changed = true;
                match result {
                    Ok(done) => {
                        self.note = Some(done_note(&done));
                        self.log = done.output.clone();
                        if done.cut {
                            self.log.push_str(CUT_NOTE);
                        }
                    }
                    Err(err) => self.note = Some(err),
                }
            }
        }
    }

    /// Drain the worker. True when the worker finished (its sender dropped).
    pub(super) fn pump(&mut self) -> bool {
        let Some(rx) = self.rx.take() else {
            return false;
        };
        loop {
            match rx.try_recv() {
                Ok(msg) => self.apply(msg),
                Err(mpsc::TryRecvError::Empty) => {
                    self.rx = Some(rx);
                    return false;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.busy || self.loading {
                        // The worker went without a last word.
                        self.busy = false;
                        self.loading = false;
                        self.note = Some("The deploy stopped without a result.".into());
                    }
                    return true;
                }
            }
        }
    }

    fn start_run(
        &mut self,
        config_dir: &Path,
        workspace: &Path,
        command: String,
        confirmed: bool,
        repaint: Option<egui::Context>,
    ) {
        self.begin();
        self.refresh_after = true;
        let (config, dir) = (config_dir.to_path_buf(), workspace.to_path_buf());
        let (tx, rx) = mpsc::channel();
        // The repo's key, remote and branch are read off the UI thread too.
        std::thread::spawn(move || {
            let ship = grokhub_code::ship::load_from_shell_json(&config);
            let inner = spawn_run_with(
                config,
                key_of(&dir),
                dir.clone(),
                ship,
                command,
                confirmed,
                Box::new(grokhub_code::secrets::Keychain),
                place_of(&dir),
                repaint,
            );
            for msg in inner {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });
        self.watch(rx);
    }

    /// The Deploy section.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, config_dir: &Path, workspace: &Path) {
        let ctx = ui.ctx().clone();
        if self.pump() && std::mem::take(&mut self.refresh_after) {
            self.refresh(config_dir, workspace, Some(ctx.clone()));
        }
        if self.loaded_for.as_deref() != Some(workspace) && !self.busy {
            self.refresh(config_dir, workspace, Some(ctx.clone()));
        }

        section_title(ui, DEPLOY_TITLE);
        muted(ui, &mode_line(self.plan.as_ref()));
        ui.add_space(crate::theme::SPACING_8);

        let off = self.plan.as_ref().is_none_or(|p| !p.deploy_enabled);
        let label = if self.busy { "Deploying…" } else { "Deploy" };
        let pressed = ui
            .add_enabled_ui(!self.busy && !self.loading && !off, |ui| {
                crate::cards::white_pill(ui, label)
            })
            .inner;
        if pressed {
            if let Some(plan) = self.plan.clone() {
                if let Some(cmd) = self.press(&plan) {
                    self.start_run(config_dir, workspace, cmd, false, Some(ctx.clone()));
                }
            }
        }

        if let Some(cmd) = self.confirming.clone() {
            ui.add_space(crate::theme::SPACING_8);
            let spec = super::confirm::ConfirmSpec {
                title: DEPLOY_CONFIRM,
                consequence: DEPLOY_RULES,
                primary: "Deploy",
                danger: false,
            };
            match super::confirm::paint_confirm_sheet(ui, spec, &cmd) {
                Some(super::confirm::ConfirmAct::Confirm) => {
                    self.start_run(config_dir, workspace, cmd, true, Some(ctx.clone()));
                }
                Some(super::confirm::ConfirmAct::Cancel) => self.cancel(),
                None => {}
            }
        }

        if let Some(note) = &self.note {
            ui.add_space(crate::theme::SPACING_8);
            muted(ui, note);
        }
        if self.busy || !self.log.is_empty() {
            ui.add_space(crate::theme::SPACING_8);
            let shown = if self.log.is_empty() {
                RUNNING
            } else {
                self.log.as_str()
            };
            log_pane(ui, shown);
        }

        ui.add_space(crate::theme::SPACING_16);
        self.settings_ui(ui, config_dir, workspace, &ctx);
    }

    fn settings_ui(
        &mut self,
        ui: &mut egui::Ui,
        config_dir: &Path,
        workspace: &Path,
        ctx: &egui::Context,
    ) {
        section_title(ui, "Deploy settings");
        muted(ui, SETTINGS_RULES);
        ui.add_space(crate::theme::SPACING_8);
        let Some(plan) = self.plan.clone() else {
            muted(ui, "Loading…");
            return;
        };
        let idle = !self.busy && !self.loading;
        let mut save: Option<Settings> = None;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(idle, |ui| {
                if crate::cards::enabled_switch(ui, self.draft_on) {
                    save = Some(Settings {
                        deploy_enabled: !self.draft_on,
                        command: self.draft_command.clone(),
                    });
                }
            });
            ui.label(
                RichText::new("Let the Code tab deploy this project")
                    .size(crate::theme::TYPE_UI)
                    .color(crate::theme::fg()),
            );
        });
        ui.add_space(crate::theme::SPACING_4);
        muted(ui, "Deploy command");
        ui.add_enabled(
            idle,
            egui::TextEdit::singleline(&mut self.draft_command)
                .font(egui::TextStyle::Monospace)
                .hint_text("npm run deploy")
                .desired_width(f32::INFINITY),
        );
        let changed = self.draft_command.trim() != plan.command;
        ui.add_space(crate::theme::SPACING_4);
        ui.horizontal(|ui| {
            let can_save = idle && changed;
            if ui
                .add_enabled_ui(can_save, |ui| {
                    crate::cards::ghost_pill(ui, "Save deploy command")
                })
                .inner
            {
                save = Some(Settings {
                    deploy_enabled: self.draft_on,
                    command: self.draft_command.clone(),
                });
            }
        });
        if let Some(next) = save {
            self.refresh_after = true;
            self.loading = true;
            self.settings_note = None;
            let (config, dir) = (config_dir.to_path_buf(), workspace.to_path_buf());
            let rx = spawn(Some(ctx.clone()), move || {
                Msg::Saved(deploy::save_settings_of(&config, &dir, next))
            });
            self.watch(rx);
        }

        ui.add_space(crate::theme::SPACING_12);
        muted(ui, "Remember this project's deploy command");
        match &plan.remembered {
            Some(cmd) => {
                code_line(ui, cmd);
                if ui
                    .add_enabled_ui(idle, |ui| crate::cards::ghost_pill(ui, "Forget"))
                    .inner
                {
                    self.loading = true;
                    let (config, dir) = (config_dir.to_path_buf(), workspace.to_path_buf());
                    let rx = spawn(Some(ctx.clone()), move || {
                        let ship = grokhub_code::ship::load_from_shell_json(&config);
                        Msg::Plan(deploy::forget(&config, &dir, &ship))
                    });
                    self.watch(rx);
                }
            }
            None => muted(ui, NOTHING_REMEMBERED),
        }
        if let Some(note) = &self.settings_note {
            muted(ui, note);
        }
    }
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(crate::theme::TYPE_TITLE)
            .color(crate::theme::fg()),
    );
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

fn code_line(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .monospace()
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::fg()),
        )
        .wrap(),
    );
}

fn log_pane(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(crate::theme::elevated())
        .corner_radius(crate::theme::CARD_RADIUS)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("code-deploy-log")
                .max_height(260.0)
                .stick_to_bottom(true)
                .show(ui, |ui| code_line(ui, text));
        });
}

#[cfg(test)]
#[path = "code_deploy_ui_tests.rs"]
mod tests;
