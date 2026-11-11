//! The Code tab: the workspace's repo (branch, changed files and one file's
//! diff), the app's built-in skills with whatever overrides each one, a test
//! runner that asks before a command's first run, and recent Code activity.
//!
//! The logic is `grokhub_code`'s, the same the desktop shell's Code tab
//! calls, so both apps share the remembered commands, the activity log and
//! the secret names. Every git call and run happens off the UI thread: a job
//! sends its result down the tab's channel and wakes the page.
//!
//! Desktop cabin only. Nothing here is served to the LAN host, the web app or
//! the phone remote.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui::{self, RichText};
use grokhub_code::{activity, repo, runs, secrets};

use super::confirm::{paint_confirm_sheet, ConfirmAct, ConfirmSpec};
use super::Cabin;

pub(super) const CODE_NOT_REPO: &str = "This workspace isn't a git repo yet. Pick a project folder in Settings, or ask the agent to start one.";
pub(super) const CODE_CLEAN: &str = "No changes since the last commit.";
pub(super) const CODE_BUILTINS: &str = "Built-in skills";
pub(super) const CODE_TESTS: &str = "Tests";
pub(super) const CODE_CONFIRM: &str = "Run this test command?";
pub(super) const CODE_NO_COMMAND: &str = "Type a test command to run.";
pub(super) const CODE_FIRST_ASK: &str = "The first run of this command will ask before it starts.";
pub(super) const CODE_ACTIVITY: &str = "Recent activity";
pub(super) const CODE_PASSED: &str = "Tests passed.";
pub(super) const OUTPUT_CUT: &str = "\n… cut here; the output is too long to show in full.";
pub(super) const DIFF_CUT: &str = "\n… cut here; the file's diff is too long to show in full.";
/// Activity rows the tab lists.
pub(super) const ACTIVITY_ROWS: usize = 10;

/// The test confirm: the exact command shows under it.
pub(super) fn run_confirm_spec() -> ConfirmSpec {
    ConfirmSpec {
        title: CODE_CONFIRM,
        consequence: "It runs in this project folder. The same command runs without asking next time, until you forget it.",
        primary: "Run tests",
        danger: false,
    }
}

/// The label on a built-in row that a same-name skill currently shadows.
pub(super) fn override_label(over: &repo::OverriddenBy) -> String {
    format!("overridden by {} skill {}", over.scope, over.name)
}

/// The line under the workspace: branch, upstream, ahead and behind.
pub(super) fn branch_line(r: &repo::Repo) -> String {
    let mut line = match &r.branch {
        Some(b) => format!("On {b}"),
        None => "Detached HEAD".to_string(),
    };
    if let Some(up) = &r.upstream {
        line.push_str(&format!(" → {up}"));
    }
    if r.ahead > 0 || r.behind > 0 {
        line.push_str(&format!(" · {} ahead, {} behind", r.ahead, r.behind));
    }
    line
}

/// The note under a finished run.
pub(super) fn run_note_for(exit: i32) -> String {
    if exit == 0 {
        CODE_PASSED.to_string()
    } else {
        format!("Tests exited with {exit}.")
    }
}

/// One activity row: what happened, on which branch, and how it ended.
pub(super) fn activity_line(e: &activity::Entry) -> String {
    let kind = match e.kind {
        activity::Kind::Push => "Push",
        activity::Kind::Pr => "Pull request",
        activity::Kind::Branch => "New branch",
        activity::Kind::Blocked => "Blocked",
        activity::Kind::Deploy => "Deploy",
        activity::Kind::Secret => "Secret",
        activity::Kind::Edit => "Edit",
        activity::Kind::Terminal => "Terminal",
    };
    let mut parts = vec![kind.to_string()];
    if !e.branch.trim().is_empty() {
        parts.push(e.branch.trim().to_string());
    }
    let what = if e.summary.trim().is_empty() {
        e.command.trim()
    } else {
        e.summary.trim()
    };
    if !what.is_empty() {
        parts.push(what.to_string());
    }
    if let Some(code) = e.exit.filter(|c| *c != 0) {
        parts.push(format!("exit {code}"));
    }
    parts.join(" · ")
}

/// What a background job sends back to the page.
#[derive(Debug)]
pub(super) enum Msg {
    Loaded(Box<Loaded>),
    Diff {
        pick: u64,
        result: Result<repo::Diff, String>,
    },
    /// This command needs a yes before it runs.
    Ask {
        command: String,
    },
    /// Output so far, already redacted.
    Chunk(String),
    Ran(Result<runs::RunResult, String>),
    /// A fresh plan. `fill` puts its command in the box unless the user typed over it.
    Plan {
        plan: Result<runs::Plan, String>,
        fill: bool,
    },
}

/// One load's answers.
#[derive(Debug)]
pub(super) struct Loaded {
    pub(super) repo: repo::Repo,
    pub(super) builtins: Vec<repo::Builtin>,
    pub(super) plan: Result<runs::Plan, String>,
    pub(super) activity: Vec<activity::Entry>,
    /// The open file's diff read again, or `None` once it has no changes.
    pub(super) diff: Option<repo::Diff>,
}

/// What the Code tab holds between frames.
pub(super) struct CodeTab {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// The workspace last loaded; a different project folder loads again.
    loaded_for: Option<PathBuf>,
    pub(super) loading: bool,
    pub(super) repo: Option<repo::Repo>,
    pub(super) builtins: Vec<repo::Builtin>,
    pub(super) activity: Vec<activity::Entry>,
    pub(super) note: Option<String>,
    pub(super) diff: Option<repo::Diff>,
    /// The newest file picked; an answer for an older pick is dropped.
    picked: u64,
    pub(super) open_skill: Option<String>,
    pub(super) command: String,
    /// The command the last plan filled in. A box the user typed over keeps their text.
    planned: String,
    pub(super) needs_confirm: bool,
    pub(super) remembered: Option<String>,
    pub(super) confirming: bool,
    pub(super) running: bool,
    pub(super) log: String,
    pub(super) run_note: Option<String>,
    /// Push and PR, behind the shared push guard.
    pub(super) ship: super::code_ship_ui::ShipPanel,
    /// This project's deploy secrets, names only.
    pub(super) secrets: super::code_secrets_ui::SecretsPanel,
}

impl Default for CodeTab {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            loaded_for: None,
            loading: false,
            repo: None,
            builtins: Vec::new(),
            activity: Vec::new(),
            note: None,
            diff: None,
            picked: 0,
            open_skill: None,
            command: String::new(),
            planned: String::new(),
            needs_confirm: true,
            remembered: None,
            confirming: false,
            running: false,
            log: String::new(),
            run_note: None,
            ship: Default::default(),
            secrets: Default::default(),
        }
    }
}

impl CodeTab {
    pub(super) fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    /// Apply every result that has arrived. True when anything changed.
    pub(super) fn drain(&mut self) -> bool {
        let mut any = false;
        while let Ok(msg) = self.rx.try_recv() {
            self.apply(msg);
            any = true;
        }
        any
    }

    /// Whether a job is still out, so the page keeps polling.
    pub(super) fn busy(&self) -> bool {
        self.loading || self.running
    }

    /// The workspace changed (or nothing has loaded yet).
    pub(super) fn wants_load(&self, workspace: &Path) -> bool {
        !self.loading && self.loaded_for.as_deref() != Some(workspace)
    }

    /// Mark a load as started for this workspace.
    pub(super) fn begin_load(&mut self, workspace: &Path) {
        self.loading = true;
        self.note = None;
        if self.loaded_for.as_deref() != Some(workspace) {
            // Another project: its files, diff and plan replace this one's.
            self.diff = None;
            self.repo = None;
        }
        self.loaded_for = Some(workspace.to_path_buf());
    }

    /// The open diff's path, for a load to read again.
    pub(super) fn open_path(&self) -> Option<String> {
        self.diff.as_ref().map(|d| d.path.clone())
    }

    /// A file row was pressed: the pick number its answer must match.
    pub(super) fn begin_pick(&mut self) -> u64 {
        self.note = None;
        self.picked += 1;
        self.picked
    }

    /// The box changed: a different command needs its own yes.
    pub(super) fn edit_command(&mut self) {
        self.confirming = false;
        self.needs_confirm = true;
    }

    /// Start a run of the box's command, or `None` when there is nothing to
    /// run or one is already going. The job decides whether to ask first.
    pub(super) fn begin_run(&mut self) -> Option<String> {
        let command = self.command.trim().to_string();
        if command.is_empty() || self.running {
            return None;
        }
        self.confirming = false;
        self.running = true;
        self.log.clear();
        self.run_note = None;
        Some(command)
    }

    pub(super) fn cancel_confirm(&mut self) {
        self.confirming = false;
    }

    pub(super) fn apply(&mut self, msg: Msg) {
        match msg {
            Msg::Loaded(loaded) => {
                let Loaded {
                    repo,
                    builtins,
                    plan,
                    activity,
                    diff,
                } = *loaded;
                self.loading = false;
                self.repo = Some(repo);
                if self
                    .open_skill
                    .as_ref()
                    .is_some_and(|open| !builtins.iter().any(|b| &b.name == open))
                {
                    self.open_skill = None;
                }
                self.builtins = builtins;
                self.activity = activity;
                self.diff = diff;
                self.apply_plan(plan, true);
            }
            Msg::Diff { pick, result } => {
                if pick != self.picked {
                    return;
                }
                match result {
                    // Only a file that opened takes the pane; a failed pick
                    // leaves the diff on screen as it was.
                    Ok(d) => self.diff = Some(d),
                    Err(e) => self.note = Some(e),
                }
            }
            Msg::Ask { command } => {
                self.running = false;
                self.command = command;
                self.needs_confirm = true;
                self.confirming = true;
            }
            Msg::Chunk(text) => self.log.push_str(&text),
            Msg::Ran(Ok(done)) => {
                self.running = false;
                self.log = done.output;
                if done.cut {
                    self.log.push_str(OUTPUT_CUT);
                }
                self.run_note = Some(run_note_for(done.exit));
            }
            Msg::Ran(Err(e)) => {
                self.running = false;
                self.run_note = Some(e);
            }
            Msg::Plan { plan, fill } => self.apply_plan(plan, fill),
        }
    }

    fn apply_plan(&mut self, plan: Result<runs::Plan, String>, fill: bool) {
        match plan {
            Ok(p) => {
                self.needs_confirm = p.needs_confirm;
                self.remembered = p.remembered;
                if fill {
                    if self.command == self.planned {
                        self.command = p.command.clone();
                    }
                    self.planned = p.command;
                }
            }
            Err(e) => self.note = Some(e),
        }
    }
}

// ---- jobs: plain functions, run on a worker thread ----

/// Read the repo, the built-ins, the test plan and recent activity.
pub(super) fn load_job(
    config: &Path,
    workspace: &Path,
    home: Option<&Path>,
    open: Option<&str>,
    tx: &Sender<Msg>,
) {
    let r = repo::repo(workspace);
    let diff = open
        .filter(|p| r.files.iter().any(|f| f.path == *p))
        .and_then(|p| repo::diff(workspace, p).ok());
    let msg = Msg::Loaded(Box::new(Loaded {
        builtins: repo::builtins(workspace, home, config),
        plan: runs::plan(config, workspace, None, runs::Mode::Auto),
        activity: {
            // A held secret value never shows, whatever wrote the line.
            let held = secrets::held_for(config, workspace);
            activity::list(config, ACTIVITY_ROWS)
                .into_iter()
                .map(|e| super::code_ship_ui::clean_entry(e, &held))
                .collect()
        },
        diff,
        repo: r,
    }));
    let _ = tx.send(msg);
}

/// One changed file's diff. A path outside the workspace is refused.
pub(super) fn diff_job(workspace: &Path, rel: &str, pick: u64, tx: &Sender<Msg>) {
    let _ = tx.send(Msg::Diff {
        pick,
        result: repo::diff(workspace, rel),
    });
}

/// Run the test command. Unless `confirmed`, a command that still needs a
/// yes comes back as [`Msg::Ask`] and nothing runs. Every chunk is redacted
/// with the project's `held` secret values (and the usual token shapes)
/// before it is sent, and so is the final output.
pub(super) fn run_job(
    config: &Path,
    workspace: &Path,
    command: &str,
    confirmed: bool,
    held: Vec<String>,
    tx: &Sender<Msg>,
    wake: &dyn Fn(),
) {
    let plan = match runs::plan(config, workspace, Some(command), runs::Mode::Auto) {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(Msg::Ran(Err(e)));
            return;
        }
    };
    if plan.needs_confirm && !confirmed {
        let _ = tx.send(Msg::Ask {
            command: plan.command,
        });
        return;
    }
    let mut shield = secrets::Shield::new(held.clone());
    let result = runs::run(
        config,
        workspace,
        &plan.command,
        confirmed,
        runs::Mode::Auto,
        |chunk| {
            let safe = shield.push(chunk);
            if !safe.is_empty() {
                let _ = tx.send(Msg::Chunk(safe));
                wake();
            }
        },
    )
    .map(|mut done| {
        done.output = secrets::redact(&done.output, &held);
        done
    });
    let _ = tx.send(Msg::Ran(result));
    let _ = tx.send(Msg::Plan {
        plan: runs::plan(config, workspace, Some(&plan.command), runs::Mode::Auto),
        fill: false,
    });
}

/// Forget this project's remembered test command, so the next run asks.
pub(super) fn forget_job(config: &Path, workspace: &Path, tx: &Sender<Msg>) {
    let plan = runs::forget_test(config, workspace)
        .and_then(|_| runs::plan(config, workspace, None, runs::Mode::Auto));
    let _ = tx.send(Msg::Plan { plan, fill: false });
}

/// A diff as monospace lines: added green, removed red, the rest muted.
fn diff_layout(text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::monospace(crate::theme::TYPE_CAPTION);
    for line in text.split_inclusive('\n') {
        let color = if line.starts_with("+++") || line.starts_with("---") {
            crate::theme::fg()
        } else if line.starts_with('+') {
            crate::theme::code_string()
        } else if line.starts_with('-') {
            crate::theme::signal_red()
        } else if line.starts_with("@@") {
            crate::theme::code_keyword()
        } else {
            crate::theme::muted()
        };
        job.append(
            line,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            },
        );
    }
    job
}

fn well(ui: &mut egui::Ui, salt: &str, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(crate::theme::code_well())
        .corner_radius(crate::theme::CARD_RADIUS)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("code-well", salt))
                .max_height(height)
                .auto_shrink([false, true])
                .stick_to_bottom(salt == "log")
                .show(ui, add);
        });
}

fn mono(text: &str) -> RichText {
    RichText::new(text)
        .font(egui::FontId::monospace(crate::theme::TYPE_CAPTION))
        .color(crate::theme::fg())
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

impl Cabin {
    fn code_workspace(&self) -> PathBuf {
        self.grok_cwd()
    }

    /// Read the repo, skills, plan and activity again, off the UI thread.
    pub(super) fn code_kick_load(&mut self, ctx: &egui::Context) {
        let workspace = self.code_workspace();
        let open = self.code_tab.open_path();
        self.code_tab.begin_load(&workspace);
        let tx = self.code_tab.sender();
        let config = crate::config::config_dir();
        let home = grokhub_core::user_home();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            load_job(&config, &workspace, home.as_deref(), open.as_deref(), &tx);
            ctx.request_repaint();
        });
    }

    fn code_open_diff(&mut self, ctx: &egui::Context, rel: String) {
        let pick = self.code_tab.begin_pick();
        let workspace = self.code_workspace();
        let tx = self.code_tab.sender();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            diff_job(&workspace, &rel, pick, &tx);
            ctx.request_repaint();
        });
    }

    fn code_run(&mut self, ctx: &egui::Context, confirmed: bool) {
        let Some(command) = self.code_tab.begin_run() else {
            return;
        };
        let workspace = self.code_workspace();
        let tx = self.code_tab.sender();
        let config = crate::config::config_dir();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let held = secrets::held_for(&config, &workspace);
            let wake = || ctx.request_repaint();
            run_job(&config, &workspace, &command, confirmed, held, &tx, &wake);
            ctx.request_repaint();
        });
    }

    fn code_forget(&mut self, ctx: &egui::Context) {
        let workspace = self.code_workspace();
        let tx = self.code_tab.sender();
        let config = crate::config::config_dir();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            forget_job(&config, &workspace, &tx);
            ctx.request_repaint();
        });
    }

    pub(super) fn ui_code(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.code_tab.drain();
        if self.code_tab.wants_load(&self.code_workspace()) {
            self.code_kick_load(&ctx);
        }
        if self.code_tab.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        egui::CentralPanel::default()
            .frame(crate::theme::page_frame(&ctx))
            .show(ui, |ui| {
                let action = if self.code_tab.loading {
                    "Refreshing…"
                } else {
                    "Refresh"
                };
                if crate::cards::page_header(ui, "Code", action) && !self.code_tab.loading {
                    self.code_kick_load(&ctx);
                    let (config, ws) = (crate::config::config_dir(), self.code_workspace());
                    self.code_tab.ship.refresh(&ctx, &config, &ws);
                }
                super::pages::page_body(ui, "code", |ui| {
                    self.ui_code_repo(ui, &ctx);
                    ui.add_space(crate::theme::SPACING_24);
                    self.ui_code_tests(ui, &ctx);
                    ui.add_space(crate::theme::SPACING_24);
                    self.ui_code_ship(ui, &ctx);
                    ui.add_space(crate::theme::SPACING_24);
                    self.ui_code_editor(ui);
                    ui.add_space(crate::theme::SPACING_24);
                    self.ui_code_builtins(ui);
                    self.ui_code_activity(ui);
                });
            });
    }

    fn ui_code_repo(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tab = &self.code_tab;
        let title = tab
            .repo
            .as_ref()
            .map(|r| r.workspace.clone())
            .unwrap_or_else(|| "Workspace".into());
        crate::cards::section_label(ui, &title);
        if let Some(r) = tab.repo.as_ref().filter(|r| r.is_repo) {
            muted(ui, &branch_line(r));
        }
        if let Some(note) = &tab.note {
            muted(ui, note);
        }
        let Some(r) = tab.repo.clone() else {
            if tab.note.is_none() {
                muted(ui, "Loading the workspace…");
            }
            return;
        };
        if !r.is_repo {
            muted(ui, CODE_NOT_REPO);
            return;
        }
        if r.files.is_empty() {
            muted(ui, CODE_CLEAN);
            return;
        }
        ui.add_space(crate::theme::SPACING_8);
        let open = tab.diff.as_ref().map(|d| d.path.clone());
        let mut pick: Option<String> = None;
        ui.columns(2, |cols| {
            egui::ScrollArea::vertical()
                .id_salt("code-files")
                .max_height(360.0)
                .auto_shrink([false, true])
                .show(&mut cols[0], |ui| {
                    for f in &r.files {
                        let staged = if f.staged { " (staged)" } else { "" };
                        let label = format!("{}  {}{staged}", f.status, f.path);
                        let on = open.as_deref() == Some(f.path.as_str());
                        if ui.selectable_label(on, mono(&label)).clicked() {
                            pick = Some(f.path.clone());
                        }
                    }
                    if r.more {
                        muted(ui, "More files changed than this list shows.");
                    }
                });
            let ui = &mut cols[1];
            match &self.code_tab.diff {
                Some(d) => {
                    let mut text = if d.text.is_empty() {
                        "No text changes (binary or mode change).".to_string()
                    } else {
                        d.text.clone()
                    };
                    if d.cut {
                        text.push_str(DIFF_CUT);
                    }
                    well(ui, "diff", 340.0, |ui| {
                        ui.add(egui::Label::new(diff_layout(&text)).extend());
                    });
                }
                None => muted(ui, "Pick a file to see its changes."),
            }
        });
        if let Some(rel) = pick {
            self.code_open_diff(ctx, rel);
        }
    }

    fn ui_code_tests(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        crate::cards::section_label(ui, CODE_TESTS);
        let mut run = false;
        ui.horizontal(|ui| {
            let tab = &mut self.code_tab;
            let edit = ui.add_enabled(
                !tab.running,
                egui::TextEdit::singleline(&mut tab.command)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(crate::theme::hint("cargo test"))
                    .desired_width((ui.available_width() - 140.0).max(160.0)),
            );
            if edit.changed() {
                tab.edit_command();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                run = true;
            }
            let label = if tab.running {
                "Running…"
            } else {
                "Run tests"
            };
            if crate::cards::white_pill(ui, label) {
                run = true;
            }
        });
        if run {
            self.code_run(ctx, false);
        }
        let tab = &self.code_tab;
        if tab.command.trim().is_empty() {
            muted(ui, CODE_NO_COMMAND);
        }
        if tab.confirming {
            ui.add_space(crate::theme::SPACING_8);
            let command = tab.command.trim().to_string();
            match paint_confirm_sheet(ui, run_confirm_spec(), &command) {
                Some(ConfirmAct::Confirm) => self.code_run(ctx, true),
                Some(ConfirmAct::Cancel) => self.code_tab.cancel_confirm(),
                None => {}
            }
        }
        let tab = &self.code_tab;
        if let Some(note) = &tab.run_note {
            muted(ui, note);
        }
        if !tab.log.is_empty() || tab.running {
            let text = if tab.log.is_empty() {
                "Running…".to_string()
            } else {
                tab.log.clone()
            };
            ui.add_space(crate::theme::SPACING_8);
            well(ui, "log", 300.0, |ui| {
                ui.add(egui::Label::new(mono(&text)).extend());
            });
        }
        if tab.needs_confirm && !tab.confirming && !tab.running && !tab.command.trim().is_empty() {
            muted(ui, CODE_FIRST_ASK);
        }
        if let Some(remembered) = tab.remembered.clone() {
            let mut forget = false;
            ui.horizontal(|ui| {
                muted(ui, &format!("Runs without asking: {remembered}"));
                forget = crate::cards::ghost_pill(ui, "Forget");
            });
            if forget {
                self.code_forget(ctx);
            }
        }
    }

    fn ui_code_builtins(&mut self, ui: &mut egui::Ui) {
        if self.code_tab.builtins.is_empty() {
            return;
        }
        crate::cards::section_label(ui, CODE_BUILTINS);
        let mut toggle: Option<String> = None;
        for skill in &self.code_tab.builtins {
            let on = self.code_tab.open_skill.as_deref() == Some(skill.name.as_str());
            let resp = egui::Frame::NONE
                .fill(if on {
                    crate::theme::selected()
                } else {
                    crate::theme::elevated()
                })
                .corner_radius(crate::theme::CARD_RADIUS)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(&skill.name)
                            .size(crate::theme::TYPE_UI)
                            .color(crate::theme::fg()),
                    );
                    if !skill.description.is_empty() {
                        muted(ui, &skill.description);
                    }
                    if let Some(over) = &skill.overridden_by {
                        muted(ui, &override_label(over));
                    }
                })
                .response
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                toggle = Some(skill.name.clone());
            }
            if on {
                ui.add_space(crate::theme::SPACING_4);
                well(ui, "skill", 320.0, |ui| {
                    ui.add(egui::Label::new(mono(&skill.body)).extend());
                });
            }
            ui.add_space(crate::theme::SPACING_8);
        }
        if let Some(name) = toggle {
            let tab = &mut self.code_tab;
            tab.open_skill = if tab.open_skill.as_deref() == Some(name.as_str()) {
                None
            } else {
                Some(name)
            };
        }
    }

    /// Push and PR (a git repo only), then Secrets. A push, PR or new
    /// branch reads the repo and the activity again.
    fn ui_code_ship(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(is_repo) = self.code_tab.repo.as_ref().map(|r| r.is_repo) else {
            return;
        };
        let (config, ws) = (crate::config::config_dir(), self.code_workspace());
        if is_repo {
            self.code_tab.ship.ui(ui, &config, &ws);
            if self.code_tab.ship.changed && !self.code_tab.loading {
                self.code_tab.ship.changed = false;
                self.code_kick_load(ctx);
            }
            ui.add_space(crate::theme::SPACING_24);
        }
        self.code_tab.secrets.ui(ui, &config, &ws);
    }

    fn ui_code_activity(&mut self, ui: &mut egui::Ui) {
        if self.code_tab.activity.is_empty() {
            return;
        }
        ui.add_space(crate::theme::SPACING_16);
        crate::cards::section_label(ui, CODE_ACTIVITY);
        for e in &self.code_tab.activity {
            muted(ui, &activity_line(e));
        }
    }
}

#[cfg(test)]
#[path = "code_tab_tests.rs"]
mod tests;
