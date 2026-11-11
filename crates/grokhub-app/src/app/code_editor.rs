//! The Code tab's editor: a folder list of the project and one open file,
//! edited in a monospace box with syntax colors and line numbers.
//!
//! Every read, list and save goes through `grokhub_code::files`, so the
//! editor only reaches files inside the project folder: `..`, a symlink that
//! leads out, and anything under `.git` are refused there. A save is logged
//! by path and size only, never the text. The open file and its unsaved
//! edits live on the cabin, so they are still there after a tab switch.

use eframe::egui::{self, RichText};
use grokhub_code::files::{self, FileText, Listing};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

pub(super) const EDITOR_TITLE: &str = "Editor";
pub(super) const EDITOR_EMPTY: &str = "Pick a file on the left to open it.";
pub(super) const EDITOR_DISCARD: &str = "Discard unsaved changes?";
pub(super) const EDITOR_UNSAVED: &str = "· unsaved";
pub(super) const EDITOR_SAVING: &str = "Saving…";
pub(super) const EDITOR_MORE: &str = "This folder has more entries than this list shows.";
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
}

/// A save running off the UI thread: the text it wrote and how it went.
struct Saving {
    path: String,
    text: String,
    rx: mpsc::Receiver<Result<FileText, String>>,
}

/// What the editor holds between frames.
#[derive(Default)]
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

    /// List one folder of the project.
    pub(super) fn browse(&mut self, folder: &str) {
        let Some(ws) = self.workspace.clone() else {
            return;
        };
        match files::list(&ws, folder) {
            Ok(listing) => self.listing = Some(listing),
            Err(e) => self.note = Some(e),
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
        self.open_file(path)
    }

    /// "Discard and open": drop the unsaved edits and open the waiting file.
    pub(super) fn discard_and_open(&mut self) -> Pick {
        match self.pending.take() {
            Some(path) => self.open_file(&path),
            None => Pick::Refused,
        }
    }

    /// "Keep editing": put the discard question away.
    pub(super) fn keep_editing(&mut self) {
        self.pending = None;
    }

    fn open_file(&mut self, path: &str) -> Pick {
        self.pending = None;
        let Some(ws) = self.workspace.clone() else {
            return Pick::Refused;
        };
        match files::read(&ws, path) {
            Ok(got) => {
                self.note = None;
                self.open = Some(OpenFile {
                    workspace: ws,
                    path: got.path,
                    saved: got.text.clone(),
                    text: got.text,
                });
                Pick::Opened
            }
            Err(e) => {
                self.note = Some(e);
                Pick::Refused
            }
        }
    }

    pub(super) fn is_saving(&self) -> bool {
        self.saving.is_some()
    }

    /// Start saving the open file off the UI thread (the save also looks up
    /// the repo for the activity log). `keep` is how many activity lines
    /// stay; `None` reads the desktop app's Code setting, like every other
    /// Code tab log line. False when there is nothing to save.
    pub(super) fn save(&mut self, config_dir: &Path, keep: Option<usize>) -> bool {
        if self.saving.is_some() {
            return false;
        }
        let Some(open) = self.open.as_ref().filter(|f| f.dirty()) else {
            return false;
        };
        let (tx, rx) = mpsc::channel();
        let (config, ws, path, text) = (
            config_dir.to_path_buf(),
            open.workspace.clone(),
            open.path.clone(),
            open.text.clone(),
        );
        self.saving = Some(Saving {
            path: path.clone(),
            text: text.clone(),
            rx,
        });
        self.note = None;
        std::thread::spawn(move || {
            let keep = keep
                .unwrap_or_else(|| grokhub_code::ship::load_from_shell_json(&config).activity_keep);
            let _ = tx.send(files::write_logged(&config, &ws, &path, &text, keep));
        });
        true
    }

    /// Take a finished save, if there is one. True when one finished.
    pub(super) fn poll_save(&mut self) -> bool {
        let Some(saving) = &self.saving else {
            return false;
        };
        let got = match saving.rx.try_recv() {
            Ok(got) => got,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The save stopped before it finished.".into())
            }
        };
        let Some(saving) = self.saving.take() else {
            return false;
        };
        match got {
            Ok(done) => {
                if let Some(open) = self.open.as_mut().filter(|f| f.path == saving.path) {
                    // Typing during the save keeps those newer edits unsaved.
                    open.saved = saving.text;
                }
                self.note = Some(format!("Saved {}.", done.path));
            }
            Err(e) => self.note = Some(e),
        }
        true
    }

    /// The editor, drawn into the Code page.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, workspace: &Path, config_dir: &Path) {
        self.set_workspace(workspace);
        if self.poll_save() {
            ui.ctx().request_repaint();
        }
        if self.saving.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        crate::cards::section_label(ui, EDITOR_TITLE);
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
        if let Some(note) = &self.note {
            crate::cards::help_text(ui, note);
        }
        if let Some(folder) = browse_to {
            self.browse(&folder);
        }
        if let Some(path) = picked {
            self.pick(&path);
        }
    }

    fn ui_tree(
        &self,
        ui: &mut egui::Ui,
        browse_to: &mut Option<String>,
        picked: &mut Option<String>,
    ) {
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
        let saving = self.saving.is_some();
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
        ui.add_space(6.0);
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
                ui.add_space(6.0);
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

#[cfg(test)]
#[path = "code_editor_tests.rs"]
mod tests;
