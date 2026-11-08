//! Settings → Privacy & data → Storage: where your files go (Open, Change,
//! Reset to default), one Open per folder, where local models sit, and
//! Bring older files over from the folders older versions used.

use super::*;
use grokhub_core::user_files::{self, BringOver, UserFolder};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// How long a count of waiting files is trusted before it is taken again.
const WAITING_TTL: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Default)]
pub(super) struct StorageState {
    /// The files folder before a Change or Reset this session, so its files
    /// can be brought along.
    pub(super) previous_root: Option<PathBuf>,
    /// A bring-over running off the UI thread; its summary arrives here.
    pub(super) bring_rx: Option<mpsc::Receiver<String>>,
    /// The last thing the block has to say.
    pub(super) note: String,
    /// Items waiting in older folders, and when that was counted.
    waiting: Option<(usize, std::time::Instant)>,
}

/// Folders older versions saved into, each with the folder its files belong
/// in now. A Change this session adds the old files folder's own folders.
pub(super) fn older_spots(
    home: Option<&Path>,
    previous_root: Option<&Path>,
) -> Vec<(PathBuf, UserFolder)> {
    let mut spots = Vec::new();
    if let Some(home) = home {
        spots.push((
            grokhub_core::screen_record::old_recordings_root(home),
            UserFolder::Recordings,
        ));
        spots.push((
            home.join("GrokHub-Work").join("imagine"),
            UserFolder::Images,
        ));
    }
    if let Some(old) = previous_root {
        spots.extend(UserFolder::ALL.map(|kind| (old.join(kind.name()), kind)));
    }
    spots
}

/// Bring every older spot into its folder under `config`'s files folder.
pub(super) fn bring_all_over(config: &Path, spots: &[(PathBuf, UserFolder)]) -> BringOver {
    let mut all = BringOver::default();
    for (from, kind) in spots {
        if !from.is_dir() {
            continue;
        }
        let to = match crate::config::user_folder_in(config, *kind) {
            Ok(dir) => dir,
            Err(e) => {
                all.failed.push(format!(
                    "{}: {}",
                    kind.name(),
                    user_files::plain_io_error(&e)
                ));
                continue;
            }
        };
        let done = user_files::bring_over(from, &to);
        all.moved += done.moved;
        all.copied += done.copied;
        all.already += done.already;
        all.failed.extend(done.failed);
    }
    all
}

impl Cabin {
    fn storage_spots(&self) -> Vec<(PathBuf, UserFolder)> {
        older_spots(
            grokhub_core::paths::user_home().as_deref(),
            self.storage.previous_root.as_deref(),
        )
    }

    /// Items still waiting in older folders (counted at most every 10 s).
    fn storage_waiting(&mut self) -> usize {
        if let Some((n, at)) = self.storage.waiting {
            if at.elapsed() < WAITING_TTL {
                return n;
            }
        }
        let config = crate::config::config_dir();
        let n = self
            .storage_spots()
            .iter()
            .map(|(from, kind)| {
                let to = crate::config::user_folder_path_in(&config, *kind);
                user_files::waiting_to_bring_over(from, &to)
            })
            .sum();
        self.storage.waiting = Some((n, std::time::Instant::now()));
        n
    }

    /// Make `dir` the files folder. Files already saved stay put; the old
    /// folder's files can follow with Bring them over.
    pub(super) fn change_files_root(&mut self, dir: &Path) {
        let config = crate::config::config_dir();
        let before = user_files::files_root(&config);
        self.storage.note = match user_files::set_files_root(&config, dir) {
            Ok(()) => {
                if before != dir {
                    self.storage.previous_root = Some(before);
                }
                format!(
                    "New files go to {}. Files already saved stay where they are.",
                    dir.display()
                )
            }
            Err(problem) => format!("Can't use that folder: {problem}."),
        };
        self.storage.waiting = None;
    }

    pub(super) fn reset_files_root(&mut self) {
        let config = crate::config::config_dir();
        let before = user_files::files_root(&config);
        self.storage.note = match user_files::reset_files_root(&config) {
            Ok(()) => {
                let now = user_files::files_root(&config);
                if before != now {
                    self.storage.previous_root = Some(before);
                }
                format!("New files go to {} again.", now.display())
            }
            Err(e) => format!("Couldn't reset it: {}.", user_files::plain_io_error(&e)),
        };
        self.storage.waiting = None;
    }

    /// Move (same drive) or copy (another drive, originals kept) every older
    /// folder's files into place, off the UI thread.
    pub(super) fn start_bring_over(&mut self) {
        if self.storage.bring_rx.is_some() {
            return;
        }
        let config = crate::config::config_dir();
        let spots = self.storage_spots();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(bring_all_over(&config, &spots).summary());
        });
        self.storage.bring_rx = Some(rx);
        self.storage.note = "Bringing your older files over…".into();
    }

    pub(super) fn poll_bring_over(&mut self) {
        let Some(rx) = self.storage.bring_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(summary) => {
                self.storage.bring_rx = None;
                self.storage.note = summary;
                self.storage.waiting = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.storage.bring_rx = None;
                self.storage.note = "Bringing files over stopped before it finished.".into();
            }
        }
    }

    fn open_storage_dir(&mut self, dir: &Path) {
        let _ = std::fs::create_dir_all(dir);
        if let Err(e) = crate::desktop::open_path(&dir.display().to_string()) {
            self.status = format!("Couldn't open {}: {e}", dir.display());
        }
    }

    /// The Storage block.
    pub(super) fn ui_storage_block(&mut self, ui: &mut egui::Ui) {
        self.poll_bring_over();
        if self.storage.bring_rx.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        let config = crate::config::config_dir();
        let root = user_files::files_root(&config);
        crate::cards::section_label(ui, "Storage");
        crate::cards::help_text(
            ui,
            &format!(
                "Your files: {}. Exports, recordings, images and reports go here.",
                root.display()
            ),
        );
        ui.horizontal_wrapped(|ui| {
            if crate::cards::ghost_pill(ui, "Open") {
                self.open_storage_dir(&root);
            }
            if crate::cards::ghost_pill(ui, "Change…") {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Where should your files go?")
                    .set_directory(&root)
                    .pick_folder()
                {
                    self.change_files_root(&dir);
                }
            }
            if user_files::files_root_is_custom(&config)
                && root != user_files::default_files_root(&config)
                && crate::cards::ghost_pill(ui, "Reset to default")
            {
                self.reset_files_root();
            }
        });
        ui.add_space(4.0);
        for kind in UserFolder::ALL {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{}: {}", kind.name(), kind.what()))
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::muted()),
                );
                if crate::cards::ghost_pill(ui, "Open") {
                    match crate::config::user_folder_in(&config, kind) {
                        Ok(dir) => self.open_storage_dir(&dir),
                        Err(e) => {
                            self.status = format!(
                                "Couldn't open {}: {}",
                                kind.name(),
                                user_files::plain_io_error(&e)
                            )
                        }
                    }
                }
            });
        }
        let models = super::setup_wizard::models_dir();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Local models: {}", models.display()))
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::muted()),
            );
            if crate::cards::ghost_pill(ui, "Open") {
                self.open_storage_dir(&models);
            }
        });
        let waiting = if self.storage.bring_rx.is_some() {
            0
        } else {
            self.storage_waiting()
        };
        if waiting > 0 {
            ui.add_space(4.0);
            let items = if waiting == 1 {
                "1 older item is".to_string()
            } else {
                format!("{waiting} older items are")
            };
            crate::cards::help_text(
                ui,
                &format!(
                    "{items} still in the folders earlier versions used. Bringing them over moves them on the same drive, or copies them and keeps the originals on another drive."
                ),
            );
            if crate::cards::ghost_pill(ui, "Bring them over") {
                self.start_bring_over();
            }
        }
        if !self.storage.note.is_empty() {
            crate::cards::help_text(ui, &self.storage.note);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("storage-ui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn older_spots_are_the_old_recordings_and_imagine_folders_plus_a_changed_root() {
        let home = Path::new("/home/user");
        let spots = older_spots(Some(home), None);
        assert_eq!(
            spots,
            vec![
                (
                    PathBuf::from("/home/user/GrokHub/recordings"),
                    UserFolder::Recordings
                ),
                (
                    PathBuf::from("/home/user/GrokHub-Work/imagine"),
                    UserFolder::Images
                ),
            ]
        );
        let old = Path::new("/media/old/Loadout");
        let spots = older_spots(None, Some(old));
        assert_eq!(spots.len(), UserFolder::ALL.len());
        assert!(spots.contains(&(old.join("Chats"), UserFolder::Chats)));
    }

    #[test]
    fn bring_all_over_fills_each_folder_and_keeps_nothing_behind_on_one_drive() {
        let root = temp("bring");
        let config = root.join("cfg");
        std::fs::create_dir_all(&config).unwrap();
        let files = root.join("files");
        user_files::set_files_root(&config, &files).unwrap();
        let home = root.join("home");
        let rec = grokhub_core::screen_record::old_recordings_root(&home).join("2026-10-01-1");
        std::fs::create_dir_all(&rec).unwrap();
        std::fs::write(rec.join("frame-001.png"), b"png").unwrap();
        let imagine = home.join("GrokHub-Work/imagine");
        std::fs::create_dir_all(&imagine).unwrap();
        std::fs::write(imagine.join("a-cat.png"), b"cat").unwrap();
        let spots = older_spots(Some(&home), None);
        let done = bring_all_over(&config, &spots);
        assert_eq!(done.summary(), "Moved 2 items.");
        assert!(files
            .join("Recordings/2026-10-01-1/frame-001.png")
            .is_file());
        assert!(files.join("Images/a-cat.png").is_file());
        assert!(!rec.exists() && !imagine.join("a-cat.png").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn change_then_reset_remember_the_old_folder_and_a_bad_pick_says_why() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("storage-change");
        let _ = std::fs::remove_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.join("cfg"));
        std::fs::create_dir_all(root.join("cfg")).unwrap();
        let mut cabin = Cabin::quiet_for_test();
        let first = root.join("first");
        cabin.change_files_root(&first);
        let second = root.join("second");
        cabin.change_files_root(&second);
        assert_eq!(
            cabin.storage.previous_root.as_deref(),
            Some(first.as_path())
        );
        assert!(
            cabin.storage.note.starts_with("New files go to "),
            "{}",
            cabin.storage.note
        );
        assert_eq!(user_files::files_root(&crate::config::config_dir()), second);
        cabin.change_files_root(Path::new("relative/folder"));
        assert_eq!(
            cabin.storage.note,
            "Can't use that folder: pick a full folder path."
        );
        assert_eq!(
            user_files::files_root(&crate::config::config_dir()),
            second,
            "a refused pick changes nothing"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
