//! The Code tab's editor: a folder list of the project and one open file,
//! edited in a monospace box with syntax colors and line numbers.
//!
//! Every read, list and save goes through `grokhub_code::files`, so the
//! editor only reaches files inside the project folder: `..`, a symlink that
//! leads out, and anything under `.git` are refused there. A save is logged
//! by path and size only, never the text. The open file and its unsaved
//! edits live on the cabin, so they are still there after a tab switch.
//!
//! In the window a list, a read and a save each run on a worker thread and
//! come back through a channel, so a slow or stalled folder never holds up a
//! frame. Only the newest list and the newest read count; an older answer, or
//! one for another project folder, is dropped.

use eframe::egui::{self, RichText};
use grokhub_code::files::{self, FileText, Listing};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

pub(super) const EDITOR_TITLE: &str = "Editor";
pub(super) const EDITOR_EMPTY: &str = "Pick a file on the left to open it.";
pub(super) const EDITOR_DISCARD: &str = "Discard unsaved changes?";
pub(super) const EDITOR_UNSAVED: &str = "· unsaved";
pub(super) const EDITOR_SAVING: &str = "Saving…";
pub(super) const EDITOR_MORE: &str = "This folder has more entries than this list shows.";
pub(super) const EDITOR_LOADING: &str = "Loading…";
/// Width of the file list beside the editor.
const TREE_W: f32 = 220.0;
/// Shortest and tallest the editor gets, in points. The Code page scrolls,
/// so the editor keeps its own height and scrolls inside it.
const MIN_EDIT_H: f32 = 320.0;
const MAX_EDIT_H: f32 = 560.0;

/// One file open in the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OpenFile {
    /// The project folder the file was opened from. Saves go back there even
    /// when the cabin's project changes while edits are unsaved.
    pub(super) workspace: PathBuf,
    /// That folder with links resolved, as the read returned it. A save
    /// sends it back and is refused when the folder is a different one now.
    pub(super) root: String,
    /// The path from the project root, with `/` between parts.
    pub(super) path: String,
    /// The text in the editor box.
    pub(super) text: String,
    /// The text as last read or saved.
    pub(super) saved: String,
}

impl OpenFile {
    pub(super) fn dirty(&self) -> bool {
        self.text != self.saved
    }
}

/// What a press on a file row did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Pick {
    /// The file is open now.
    Opened,
    /// It is already the open file.
    Same,
    /// The open file has unsaved edits, so the discard question is up.
    Asked,
    /// It couldn't be opened; the note says why.
    Refused,
    /// The read is running off the UI thread; its answer opens the file.
    Loading,
}

/// What a list or a read sends back. `ask` is the number it was started
/// with and `workspace` the project folder it ran in.
pub(super) enum Answer {
    Listed {
        ask: u64,
        workspace: PathBuf,
        result: Result<Listing, String>,
    },
    Read {
        ask: u64,
        workspace: PathBuf,
        /// "Discard and open" asked for it: it replaces unsaved edits.
        discard: bool,
        /// The path as picked, for the discard question.
        path: String,
        result: Result<FileText, String>,
    },
}

/// Wakes the window when an answer is ready.
type Wake = Arc<dyn Fn() + Send + Sync>;

/// A save running off the UI thread: the text it wrote and how it went.
struct Saving {
    workspace: PathBuf,
    path: String,
    text: String,
    rx: mpsc::Receiver<Result<FileText, String>>,
}

/// What the editor holds between frames.
pub(super) struct CodeEditor {
    /// The project folder the list shows. A different one lists again.
    workspace: Option<PathBuf>,
    pub(super) listing: Option<Listing>,
    pub(super) open: Option<OpenFile>,
    /// The file waiting behind the discard question.
    pub(super) pending: Option<String>,
    /// The last error or "Saved …" line.
    pub(super) note: Option<String>,
    saving: Option<Saving>,
    tx: mpsc::Sender<Answer>,
    rx: mpsc::Receiver<Answer>,
    /// Set once the editor is drawn in a window: lists and reads then run
    /// on a worker and this wakes the window when one lands. Without it
    /// there is no frame to wait in, and a call runs where it is made.
    wake: Option<Wake>,
    /// The newest list and the newest read asked for.
    list_ask: u64,
    read_ask: u64,
    /// A list is out: the tree says so.
    pub(super) listing_out: bool,
}

impl Default for CodeEditor {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            workspace: None,
            listing: None,
            open: None,
            pending: None,
            note: None,
            saving: None,
            tx,
            rx,
            wake: None,
            list_ask: 0,
            read_ask: 0,
            listing_out: false,
        }
    }
}

/// The highlighter language for a file, by its extension. egui_extras' own
/// highlighter knows Rust, Python, C/C++ and TOML; the C-like web languages
/// use the C++ rules, and `#`-comment config files the TOML rules. Anything
/// else (Markdown, plain text) is shown without colors.
pub(super) fn language_for(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        _ => return "",
    };
    match ext.as_str() {
        "rs" => "rs",
        "py" | "pyi" => "py",
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx" | "json" | "java" | "kt"
        | "go" | "cs" | "swift" => "cpp",
        "toml" | "yaml" | "yml" | "sh" | "bash" | "ini" | "cfg" | "conf" => "toml",
        _ => "",
    }
}

/// The folder above `path` (`""` is the project root).
pub(super) fn parent_of(path: &str) -> &str {
    path.rfind('/').map_or("", |cut| &path[..cut])
}

/// The line-number gutter for `text`: one number per line, the last line
/// included even when it is empty.
pub(super) fn gutter(text: &str) -> String {
    let lines = text.split('\n').count().max(1);
    let width = lines.to_string().len();
    (1..=lines)
        .map(|n| format!("{n:>width$}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The button that answers the discard question.
pub(super) fn discard_label(pending: &str) -> String {
    format!("Discard and open {pending}")
}

impl CodeEditor {
    /// Point the editor at the cabin's project folder. A new folder lists
    /// again from its root. An open file with no unsaved edits closes; one
    /// with edits stays, bound to the folder it came from.
    pub(super) fn set_workspace(&mut self, workspace: &Path) {
        if self.workspace.as_deref() == Some(workspace) {
            return;
        }
        self.workspace = Some(workspace.to_path_buf());
        self.listing = None;
        self.pending = None;
        if self.open.as_ref().is_some_and(|f| !f.dirty()) {
            self.open = None;
        }
        self.browse("");
    }

    /// Run lists and reads on a worker from now on; `wake` is called when
    /// an answer is ready for [`Self::poll`].
    pub(super) fn wake_with(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.wake = Some(Arc::new(wake));
    }

    /// List one folder of the project. With a window the listing arrives
    /// through [`Self::poll`].
    pub(super) fn browse(&mut self, folder: &str) {
        let Some(ws) = self.workspace.clone() else {
            return;
        };
        self.list_ask += 1;
        let ask = self.list_ask;
        let folder = folder.to_string();
        let job = move || {
            let result = files::list(&ws, &folder);
            Answer::Listed {
                ask,
                workspace: ws,
                result,
            }
        };
        self.listing_out = true;
        self.run(job);
    }

    /// Run a list or a read: on a worker when there is a window to wake,
    /// here otherwise. The answer's own pick, when it ran here.
    fn run(&mut self, job: impl FnOnce() -> Answer + Send + 'static) -> Option<Pick> {
        match self.wake.clone() {
            Some(wake) => {
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(job());
                    wake();
                });
                None
            }
            None => self.apply(job()),
        }
    }

    /// Take the answers that have arrived. True when any did.
    pub(super) fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok(answer) = self.rx.try_recv() {
            self.apply(answer);
            any = true;
        }
        any
    }

    /// One answer. `None` when it was dropped: an older ask, or one for a
    /// project folder the editor has left.
    pub(super) fn apply(&mut self, answer: Answer) -> Option<Pick> {
        match answer {
            Answer::Listed {
                ask,
                workspace,
                result,
            } => {
                if ask != self.list_ask || self.workspace.as_deref() != Some(workspace.as_path()) {
                    return None;
                }
                self.listing_out = false;
                match result {
                    Ok(listing) => self.listing = Some(listing),
                    Err(e) => self.note = Some(e),
                }
                None
            }
            Answer::Read {
                ask,
                workspace,
                discard,
                path,
                result,
            } => {
                if ask != self.read_ask || self.workspace.as_deref() != Some(workspace.as_path()) {
                    return None;
                }
                // Edits typed while the read was out are not thrown away:
                // the file waits behind the discard question instead.
                if !discard && self.open.as_ref().is_some_and(OpenFile::dirty) {
                    self.pending = Some(path);
                    return Some(Pick::Asked);
                }
                match result {
                    Ok(got) => {
                        self.note = None;
                        self.open = Some(OpenFile {
                            workspace,
                            root: got.root,
                            path: got.path,
                            saved: got.text.clone(),
                            text: got.text,
                        });
                        Some(Pick::Opened)
                    }
                    Err(e) => {
                        self.note = Some(e);
                        Some(Pick::Refused)
                    }
                }
            }
        }
    }

    /// A press on a file row: open it, or ask first when edits are unsaved.
    pub(super) fn pick(&mut self, path: &str) -> Pick {
        if let Some(open) = &self.open {
            if open.path == path && self.workspace.as_deref() == Some(open.workspace.as_path()) {
                return Pick::Same;
            }
            if open.dirty() {
                self.pending = Some(path.to_string());
                return Pick::Asked;
            }
        }
        self.open_file(path, false)
    }

    /// "Discard and open": drop the unsaved edits and open the waiting file.
    pub(super) fn discard_and_open(&mut self) -> Pick {
        match self.pending.take() {
            Some(path) => self.open_file(&path, true),
            None => Pick::Refused,
        }
    }

    /// "Keep editing": put the discard question away.
    pub(super) fn keep_editing(&mut self) {
        self.pending = None;
    }

    fn open_file(&mut self, path: &str, discard: bool) -> Pick {
        self.pending = None;
        let Some(ws) = self.workspace.clone() else {
            return Pick::Refused;
        };
        self.read_ask += 1;
        let ask = self.read_ask;
        let path = path.to_string();
        let job = move || {
            let result = files::read(&ws, &path);
            Answer::Read {
                ask,
                workspace: ws,
                discard,
                path,
                result,
            }
        };
        // With a window the answer comes through `poll`.
        self.run(job).unwrap_or(Pick::Loading)
    }

    pub(super) fn is_saving(&self) -> bool {
        self.saving.is_some()
    }

    /// Start saving the open file off the UI thread (the save also looks up
    /// the repo for the activity log). `keep` is how many activity lines
    /// stay; `None` reads the desktop app's Code setting, like every other
    /// Code tab log line. False when there is nothing to save.
    pub(super) fn save(&mut self, config_dir: &Path, keep: Option<usize>) -> bool {
        if self.is_saving() {
            return false;
        }
        let Some(open) = self.open.as_ref().filter(|f| f.dirty()) else {
            return false;
        };
        let (tx, rx) = mpsc::channel();
        let (config, ws, path, text, root) = (
            config_dir.to_path_buf(),
            open.workspace.clone(),
            open.path.clone(),
            open.text.clone(),
            open.root.clone(),
        );
        self.saving = Some(Saving {
            workspace: ws.clone(),
            path: path.clone(),
            text: text.clone(),
            rx,
        });
        self.note = None;
        std::thread::spawn(move || {
            let keep = keep
                .unwrap_or_else(|| grokhub_code::ship::load_from_shell_json(&config).activity_keep);
            let _ = tx.send(files::write_logged(&config, &ws, &path, &text, &root, keep));
        });
        true
    }

    /// Take a finished save, if there is one: `Some(true)` when it saved,
    /// `Some(false)` when it was refused, `None` while nothing finished.
    pub(super) fn poll_save(&mut self) -> Option<bool> {
        let saving = self.saving.as_ref()?;
        let got = match saving.rx.try_recv() {
            Ok(got) => got,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The save stopped before it finished.".into())
            }
        };
        let saving = self.saving.take()?;
        match got {
            Ok(done) => {
                if let Some(open) = self
                    .open
                    .as_mut()
                    .filter(|f| f.path == saving.path && f.workspace == saving.workspace)
                {
                    // Typing during the save keeps those newer edits unsaved.
                    open.saved = saving.text;
                }
                self.note = Some(format!("Saved {}.", done.path));
                Some(true)
            }
            Err(e) => {
                self.note = Some(e);
                Some(false)
            }
        }
    }

    /// The editor, drawn into the Code page. True on the frame a save lands,
    /// so the page can read the repo's changes again.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, workspace: &Path, config_dir: &Path) -> bool {
        if self.wake.is_none() {
            let ctx = ui.ctx().clone();
            self.wake_with(move || ctx.request_repaint());
        }
        self.set_workspace(workspace);
        self.poll();
        let saved = self.poll_save() == Some(true);
        if self.is_saving() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        crate::cards::section_label(ui, EDITOR_TITLE);
        // The last save or refusal sits above the editor, where it's seen.
        if let Some(note) = &self.note {
            crate::cards::help_text(ui, note);
            ui.add_space(crate::theme::SPACING_8);
        }
        let mut browse_to: Option<String> = None;
        let mut picked: Option<String> = None;
        let height = ui.available_height().clamp(MIN_EDIT_H, MAX_EDIT_H);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(TREE_W, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(TREE_W);
                    egui::ScrollArea::vertical()
                        .id_salt("code_editor_tree")
                        .auto_shrink([false, false])
                        .max_height(height)
                        .show(ui, |ui| self.ui_tree(ui, &mut browse_to, &mut picked));
                },
            );
            ui.separator();
            ui.vertical(|ui| self.ui_pane(ui, config_dir, height));
        });
        if let Some(folder) = browse_to {
            self.browse(&folder);
        }
        if let Some(path) = picked {
            self.pick(&path);
        }
        saved
    }

    fn ui_tree(
        &self,
        ui: &mut egui::Ui,
        browse_to: &mut Option<String>,
        picked: &mut Option<String>,
    ) {
        if self.listing_out {
            crate::cards::help_text(ui, EDITOR_LOADING);
        }
        let Some(listing) = &self.listing else {
            return;
        };
        let row = |ui: &mut egui::Ui, text: String, on: bool| {
            ui.add(
                egui::Button::selectable(
                    on,
                    RichText::new(text)
                        .monospace()
                        .size(crate::theme::FONT_META)
                        .color(crate::theme::fg()),
                )
                .frame_when_inactive(false),
            )
            .clicked()
        };
        if !listing.path.is_empty() && row(ui, format!("../  (up from {})", listing.path), false) {
            *browse_to = Some(parent_of(&listing.path).to_string());
        }
        let open_path = self.open.as_ref().map(|f| f.path.as_str());
        for e in &listing.entries {
            let label = if e.dir {
                format!("{}/", e.name)
            } else {
                e.name.clone()
            };
            let on = !e.dir && open_path == Some(e.path.as_str());
            if row(ui, label, on) {
                if e.dir {
                    *browse_to = Some(e.path.clone());
                } else {
                    *picked = Some(e.path.clone());
                }
            }
        }
        if listing.more {
            crate::cards::help_text(ui, EDITOR_MORE);
        }
    }

    fn ui_pane(&mut self, ui: &mut egui::Ui, config_dir: &Path, height: f32) {
        let saving = self.is_saving();
        let Some(open) = self.open.as_ref() else {
            crate::cards::help_text(ui, EDITOR_EMPTY);
            return;
        };
        let dirty = open.dirty();
        let path = open.path.clone();
        let mut save = false;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&path)
                    .monospace()
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::fg()),
            );
            if dirty {
                ui.label(
                    RichText::new(EDITOR_UNSAVED)
                        .size(crate::theme::FONT_META)
                        .color(crate::theme::muted()),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Clear of the page's scroll bar.
                ui.add_space(crate::theme::SPACING_16);
                let label = if saving { EDITOR_SAVING } else { "Save" };
                ui.add_enabled_ui(dirty && !saving, |ui| {
                    if crate::cards::white_pill(ui, label) {
                        save = true;
                    }
                });
            });
        });
        if let Some(pending) = self.pending.clone() {
            self.ui_discard(ui, &path, &pending);
        }
        ui.add_space(crate::theme::SPACING_8);
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let language = language_for(&open.path);
        let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), ui.style());
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            let job = egui_extras::syntax_highlighting::highlight(
                ui.ctx(),
                ui.style(),
                &theme,
                buf.as_str(),
                language,
            );
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let numbers = gutter(&open.text);
        let edit_h = (height - 48.0).max(MIN_EDIT_H);
        let resp = egui::ScrollArea::both()
            .id_salt(("code_editor_text", open.path.as_str()))
            .auto_shrink([false, false])
            .max_height(edit_h)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.label(
                        RichText::new(numbers)
                            .monospace()
                            .color(crate::theme::muted()),
                    );
                    ui.add(
                        egui::TextEdit::multiline(&mut open.text)
                            .id_salt(("code_editor_box", open.path.as_str()))
                            .code_editor()
                            .lock_focus(true)
                            .desired_width(f32::INFINITY)
                            .desired_rows(24)
                            .layouter(&mut layouter),
                    )
                })
                .inner
            })
            .inner;
        // Ctrl+S (Cmd+S on a Mac) saves while the editor box has the keys.
        if resp.has_focus()
            && ui.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::S,
                ))
            })
        {
            save = true;
        }
        if save {
            self.save(config_dir, None);
        }
    }

    fn ui_discard(&mut self, ui: &mut egui::Ui, path: &str, pending: &str) {
        let mut act: Option<bool> = None;
        egui::Frame::NONE
            .fill(crate::theme::elevated())
            .corner_radius(10.0)
            .stroke(egui::Stroke::new(2.0_f32, crate::theme::offline()))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{EDITOR_DISCARD} {path} has edits that aren't saved."
                    ))
                    .color(crate::theme::fg()),
                );
                ui.add_space(crate::theme::SPACING_8);
                ui.horizontal(|ui| {
                    if crate::cards::danger_pill(ui, &discard_label(pending)) {
                        act = Some(true);
                    }
                    if crate::cards::ghost_pill(ui, "Keep editing") {
                        act = Some(false);
                    }
                });
            });
        match act {
            Some(true) => {
                self.discard_and_open();
            }
            Some(false) => self.keep_editing(),
            None => {}
        }
    }
}

impl super::Cabin {
    /// The Code page's Editor section, on the same project folder as the
    /// rest of the page. A save reads the repo again, so the changed-files
    /// list and the open diff show it.
    pub(super) fn ui_code_editor(&mut self, ui: &mut egui::Ui) {
        let workspace = self.grok_cwd();
        let config = crate::config::config_dir();
        if self.code_tab.editor.ui(ui, &workspace, &config) {
            self.code_kick_load(ui.ctx());
        }
    }
}

#[cfg(test)]
#[path = "code_editor_tests.rs"]
mod tests;
