//! "Pick up where you left off" (`grokhub_core::code_resume`), opt-in under
//! Settings → Behavior. Once per launch, off the UI thread, it reads git
//! metadata for the repos in the folders the user picked. A repo with
//! unpushed commits or changed files puts one-taps first in the composer
//! while the chat is empty; stale merged branches get a tidy offer that goes
//! through the agent, so deleting still asks.

use std::path::PathBuf;

use super::*;
use grokhub_core::code_resume::{self as cr, RepoStatus};
use grokhub_core::QuickChip;

/// Editors tried, in order, for "Open in editor".
const EDITORS: &[&str] = &["cursor", "code", "codium", "zed", "subl"];

#[derive(Default)]
pub(super) struct ResumeState {
    rx: Option<mpsc::Receiver<(Vec<RepoStatus>, Option<String>)>>,
    scanned: bool,
    repos: Vec<RepoStatus>,
    editor: Option<String>,
}

impl ResumeState {
    /// Cached repos from the launch scan (empty until the scan finishes).
    pub(super) fn repos(&self) -> &[RepoStatus] {
        &self.repos
    }

    /// Seed repos for tests (no git scan).
    #[cfg(test)]
    pub(super) fn seed_repos_for_test(&mut self, repos: Vec<RepoStatus>) {
        self.repos = repos;
        self.scanned = true;
    }
}

pub(super) fn scan(folders: &[String], now_s: u64) -> Vec<RepoStatus> {
    let mut out = Vec::new();
    for folder in folders {
        for repo in cr::repo_dirs(&PathBuf::from(folder)) {
            if let Some(status) = cr::read_repo(&repo, now_s, &mut |args| {
                super::setup_wizard::run("git", args)
            }) {
                out.push(status);
            }
        }
    }
    out
}

impl Cabin {
    /// Each frame: start the launch scan once, then take its result.
    pub(super) fn tick_code_resume(&mut self) {
        if !self.cfg.resume_code || self.cfg.code_folders.is_empty() {
            return;
        }
        if !std::mem::replace(&mut self.focus.resume.scanned, true) {
            let folders = self.cfg.code_folders.clone();
            let (tx, rx) = mpsc::channel();
            self.focus.resume.rx = Some(rx);
            std::thread::spawn(move || {
                let editor = EDITORS
                    .iter()
                    .find(|e| grokhub_core::runtime_download::find_on_path(e).is_some())
                    .map(|e| e.to_string());
                let _ = tx.send((scan(&folders, now_ms() / 1000), editor));
            });
        }
        let Some(rx) = self.focus.resume.rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((repos, editor)) => self.take_code_scan(repos, editor),
            Err(mpsc::TryRecvError::Empty) => self.focus.resume.rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    fn take_code_scan(&mut self, repos: Vec<RepoStatus>, editor: Option<String>) {
        self.focus.resume.repos = repos;
        self.focus.resume.editor = editor;
        self.chip_paint_key.clear();
    }

    /// The resume one-taps and the tidy offer, while the chat is empty.
    pub(super) fn resume_chips(&self) -> Vec<QuickChip> {
        if !self.cfg.resume_code || !self.messages.is_empty() {
            return Vec::new();
        }
        let mut chips = cr::pick(&self.focus.resume.repos)
            .map(|repo| repo.chips(self.focus.resume.editor.as_deref()))
            .unwrap_or_default();
        chips.extend(
            self.focus
                .resume
                .repos
                .iter()
                .find_map(RepoStatus::tidy_chip),
        );
        chips.retain(|c| {
            !self
                .chip_dismissed
                .iter()
                .any(|d| d == &c.id || d == &c.value)
        });
        chips
    }

    fn set_resume_code(&mut self, on: bool) {
        self.cfg.resume_code = on;
        self.focus.resume = ResumeState::default();
        self.chip_paint_key.clear();
        self.persist_cfg();
        self.status = "Saved".into();
    }

    fn set_code_folders(&mut self, folders: Vec<String>) {
        self.cfg.code_folders = folders;
        self.focus.resume = ResumeState::default();
        self.chip_paint_key.clear();
        self.persist_cfg();
    }

    pub(super) fn ui_code_resume_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.resume_code;
        if crate::cards::settings_toggle(
            ui,
            "Pick up where I left off in code",
            "When you open the app, offer to continue a repo with unpushed commits or changed files. Reads git metadata only (branch, counts, dates) in the folders you pick, never diffs or file contents.",
            &mut on,
        ) {
            self.set_resume_code(on);
        }
        if !self.cfg.resume_code {
            return;
        }
        let mut remove = None;
        ui.indent("code-folders", |ui| {
            for (i, folder) in self.cfg.code_folders.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(folder).size(crate::theme::FONT_TIP));
                    if crate::cards::ghost_pill(ui, "Remove") {
                        remove = Some(i);
                    }
                });
            }
            if self.cfg.code_folders.is_empty() {
                crate::cards::help_text(ui, "Add a repo, or a folder that holds repos.");
            }
            if crate::cards::ghost_pill(ui, "Add code folder") {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Code folder")
                    .pick_folder()
                {
                    let mut folders = self.cfg.code_folders.clone();
                    let dir = dir.to_string_lossy().to_string();
                    if !folders.contains(&dir) {
                        folders.push(dir);
                        self.set_code_folders(folders);
                    }
                }
            }
        });
        if let Some(i) = remove {
            let mut folders = self.cfg.code_folders.clone();
            folders.remove(i);
            self.set_code_folders(folders);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str, ahead: u32, stale: &[&str]) -> RepoStatus {
        RepoStatus {
            path: PathBuf::from(format!("/home/user/code/{name}")),
            name: name.into(),
            branch: "feat-x".into(),
            ahead,
            last_commit_s: 5,
            stale_merged: stale.iter().map(|s| s.to_string()).collect(),
            base: "main".into(),
            ..Default::default()
        }
    }

    #[test]
    fn off_or_a_busy_chat_shows_nothing() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.resume.repos = vec![repo("my-app", 2, &[])];
        cabin.cfg.resume_code = false;
        assert!(cabin.resume_chips().is_empty());
        cabin.cfg.resume_code = true;
        assert_eq!(cabin.resume_chips().len(), 2);
        cabin.messages = Arc::new(vec![("user".into(), "hi".into())]);
        assert!(cabin.resume_chips().is_empty());
    }

    #[test]
    fn the_scan_result_puts_continue_first_and_the_editor_on_open() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.resume_code = true;
        cabin.take_code_scan(
            vec![repo("my-app", 3, &[]), repo("tools", 0, &["old-fix"])],
            Some("code".into()),
        );
        let chips = cabin.resume_chips();
        let labels: Vec<&str> = chips.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Continue feat-x",
                "my-app status",
                "Open my-app in editor",
                "Tidy 1 merged branch in tools"
            ]
        );
        assert_eq!(
            chips[0].hint,
            "my-app is on branch feat-x with 3 unpushed commits. Want to continue?"
        );
        assert_eq!(cabin.composer_chips()[0].id, "resume:continue:my-app");
        cabin.dismiss_chip(chips[0].clone());
        assert_eq!(cabin.resume_chips().len(), 3, "dismissed stays gone");
    }
}
