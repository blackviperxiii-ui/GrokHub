//! Files the app saves for you (chat exports, reports): where they land, and
//! the toast, bell row and card that say so.
//!
//! An export goes to the Chats folder in your files (`Documents/Loadout/Chats`
//! by default), then the Desktop, then the app's own folder when the one
//! before can't be written. When it is on disk a toast says "Saved <file> to
//! Chats", the bell keeps a Show in folder row, and a card above the composer
//! stays until closed: the file name opens it, then Open, Show in folder and
//! Copy path. A failed save says why in plain words and offers Try again.

use super::*;
use grokhub_core::user_files::{self, UserFolder};
use std::path::{Path, PathBuf};

/// What to redo when Try again is pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ExportRetry {
    /// This chat (by id) in this format.
    Thread(String, grokhub_core::ChatExport),
    /// A History session (by agent session id).
    Native(String),
}

/// The last save the card reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SavedFile {
    /// The file on disk; `None` when the save failed.
    pub(super) path: Option<PathBuf>,
    /// What the card and toast say ("Saved chat.md to Chats").
    pub(super) line: String,
    /// Plain reason when it failed.
    pub(super) error: Option<String>,
    pub(super) retry: Option<ExportRetry>,
}

/// One folder an export may land in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SaveDir {
    /// A folder in your files, created when missing.
    User(UserFolder),
    /// A folder that must already be there (the Desktop).
    Existing(PathBuf),
    /// The app's own folder, created when missing.
    AppData,
}

/// Write `body` as `name` into the first of `dirs` that takes it. Never
/// overwrites (`-2`, `-3`, … when the name is taken). On success returns the
/// path; on failure the plain reason from the first folder (the one the user
/// expects), since later ones are only fallbacks.
pub(super) fn save_into(
    config: &Path,
    dirs: &[SaveDir],
    name: &str,
    body: &[u8],
) -> Result<PathBuf, String> {
    let mut first_err: Option<String> = None;
    for dir in dirs {
        let found = match dir {
            SaveDir::User(kind) => crate::config::user_folder_in(config, *kind),
            SaveDir::Existing(p) if p.is_dir() => Ok(p.clone()),
            SaveDir::Existing(_) => continue,
            SaveDir::AppData => std::fs::create_dir_all(config).map(|_| config.to_path_buf()),
        };
        match found.and_then(|d| user_files::write_new_file(&d, name, body)) {
            Ok(path) => return Ok(path),
            Err(e) => {
                first_err.get_or_insert_with(|| user_files::plain_io_error(&e));
            }
        }
    }
    Err(first_err.unwrap_or_else(|| "there was no folder to save it in".into()))
}

/// The folder's name as the user knows it: "Chats", "Desktop", or the last
/// part of the path.
pub(super) fn folder_label(path: &Path) -> String {
    path.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "its folder".into())
}

impl Cabin {
    /// Where a chat export may land, in order.
    pub(super) fn chat_export_dirs() -> Vec<SaveDir> {
        let mut dirs = vec![SaveDir::User(UserFolder::Chats)];
        if let Some(desktop) = super::slash::export_desktop() {
            dirs.push(SaveDir::Existing(desktop));
        }
        dirs.push(SaveDir::AppData);
        dirs
    }

    /// Format and write a chat export off the UI thread. `body` builds the
    /// file; its result lands on the card.
    pub(super) fn spawn_export(
        &mut self,
        name: String,
        retry: ExportRetry,
        body: impl FnOnce() -> Result<String, String> + Send + 'static,
    ) {
        let config = crate::config::config_dir();
        let dirs = Self::chat_export_dirs();
        self.status = "Saving the chat…".into();
        self.spawn_export_write(retry, move || {
            let body = body()?;
            save_into(&config, &dirs, &name, body.as_bytes()).map(|p| p.display().to_string())
        });
    }

    /// An export write finished: say where it went (or why not) on every
    /// surface: toast, bell row with Show in folder, and the card.
    pub(super) fn finish_export(&mut self, result: Result<String, String>, retry: ExportRetry) {
        match result {
            Ok(shown) => {
                let path = PathBuf::from(shown);
                let line = self.announce_saved(&path);
                self.saved_file = Some(SavedFile {
                    path: Some(path),
                    line,
                    error: None,
                    retry: Some(retry),
                });
            }
            Err(reason) => {
                let line = format!("Couldn't save the chat: {reason}. Nothing was saved.");
                self.status = line.clone();
                self.saved_file = Some(SavedFile {
                    path: None,
                    line,
                    error: Some(reason),
                    retry: Some(retry),
                });
            }
        }
    }

    /// "Saved <file> to <folder>" as a toast, a bell row with Show in folder,
    /// and the status line. Returns the line.
    pub(super) fn announce_saved(&mut self, path: &Path) -> String {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let line = format!("Saved {name} to {}", folder_label(path));
        // Bell first with its action, so the status mirror sees the same
        // line already logged and doesn't add a bare copy.
        self.notify(
            super::notices::NoticeKind::Success,
            line.clone(),
            Some(self.notice_here()),
            Some(super::notices::NoticeAction::ShowInFolder(
                path.to_path_buf(),
            )),
        );
        self.status = line.clone();
        line
    }

    /// Redo the export the card names.
    pub(super) fn retry_export(&mut self, retry: ExportRetry) {
        match retry {
            ExportRetry::Thread(id, format) => match self.threads.iter().position(|t| t.id == id) {
                Some(i) => self.export_thread_at(i, format),
                None => self.status = "That chat is gone, so there's nothing to save".into(),
            },
            ExportRetry::Native(id) => self.export_native_history(&id),
        }
    }

    /// The card above the composer for the last save. Stays until closed.
    pub(super) fn ui_saved_file_card(&mut self, ui: &mut egui::Ui) {
        let Some(saved) = self.saved_file.clone() else {
            return;
        };
        ui.add_space(8.0);
        let mut close = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            match &saved.path {
                Some(path) => {
                    let link = ui
                        .link(saved.line.as_str())
                        .on_hover_text(path.display().to_string());
                    if link.clicked() || crate::cards::ghost_pill(ui, "Open") {
                        if let Err(e) = crate::desktop::open_path(&path.display().to_string()) {
                            self.status = format!("Couldn't open it: {e}");
                        }
                    }
                    if crate::cards::ghost_pill(ui, crate::desktop::reveal_label()) {
                        self.show_in_folder(path);
                    }
                    if crate::cards::ghost_pill(ui, "Copy path") {
                        ui.ctx().copy_text(path.display().to_string());
                        self.status = "Copied the file's path".into();
                    }
                }
                None => {
                    ui.colored_label(crate::theme::signal_red(), saved.line.as_str());
                    if let Some(retry) = saved.retry.clone() {
                        if crate::cards::ghost_pill(ui, "Try again") {
                            self.retry_export(retry);
                        }
                    }
                }
            }
            close = crate::cards::ghost_pill(ui, "Close");
        });
        if close {
            self.saved_file = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("saved-file-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn export_lands_in_chats_and_never_overwrites() {
        let root = temp("chats");
        let config = root.join("cfg");
        let dirs = [SaveDir::User(UserFolder::Chats), SaveDir::AppData];
        let a = save_into(&config, &dirs, "chat.md", b"one").unwrap();
        let b = save_into(&config, &dirs, "chat.md", b"two").unwrap();
        assert_eq!(a, config.join("test-files/Chats/chat.md"));
        assert_eq!(b, config.join("test-files/Chats/chat-2.md"));
        assert_eq!(folder_label(&a), "Chats");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_broken_chats_folder_falls_back_and_a_missing_desktop_is_skipped() {
        let root = temp("fallback");
        let config = root.join("cfg");
        std::fs::create_dir_all(&config).unwrap();
        // The files root is a plain file, so Chats can't be made under it.
        let blocker = root.join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        std::fs::write(
            config.join(user_files::FOLDERS_FILE),
            serde_json::json!({ "files_root": blocker }).to_string(),
        )
        .unwrap();
        let dirs = [
            SaveDir::User(UserFolder::Chats),
            SaveDir::Existing(root.join("no-desktop")),
            SaveDir::AppData,
        ];
        let got = save_into(&config, &dirs, "chat.md", b"body").unwrap();
        assert_eq!(got, config.join("chat.md"));
        assert_eq!(std::fs::read_to_string(&got).unwrap(), "body");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn when_every_folder_fails_the_first_reason_is_kept() {
        let root = temp("allfail");
        let blocker = root.join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        // The config folder itself is under a plain file: nothing can be made.
        let config = blocker.join("cfg");
        let dirs = [SaveDir::Existing(root.join("gone")), SaveDir::AppData];
        let err = save_into(&config, &dirs, "chat.md", b"body").unwrap_err();
        assert!(!err.is_empty());
        assert!(!config.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
