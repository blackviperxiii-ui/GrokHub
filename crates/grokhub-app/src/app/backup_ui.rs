//! Backup and unpushed-work reminders (`grokhub_core::backup_nudge`),
//! opt-in under Settings → Behavior. Once per launch, at most once a day and
//! never while snoozed, a background check reads git metadata for the code
//! folders, modified times under the important folders and the backup
//! tools' snapshot times. One Pulse post and its one-taps follow; pushing
//! goes through the agent, which asks first.

use std::path::PathBuf;

use super::*;
use grokhub_core::backup_nudge::{self as bn, Reminder};
use grokhub_core::QuickChip;

#[derive(Default)]
pub(super) struct BackupState {
    rx: Option<mpsc::Receiver<Option<Reminder>>>,
    checked: bool,
    reminder: Option<Reminder>,
}

fn check(code_folders: &[String], important: &[String], now: u64) -> Option<Reminder> {
    let repos = super::code_resume_ui::scan(code_folders, now / 1000);
    let folders: Vec<(PathBuf, u64)> = important
        .iter()
        .map(|f| {
            let path = PathBuf::from(f);
            let changed = bn::newest_change_ms(&path);
            (path, changed)
        })
        .collect();
    let root = if cfg!(windows) {
        PathBuf::from("C:\\")
    } else {
        PathBuf::from("/")
    };
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let tools = bn::detect_tools(&root, local.as_deref());
    bn::reminder(&repos, &folders, &tools, now)
}

impl Cabin {
    /// Each frame: the launch check once, then its result.
    pub(super) fn tick_backup(&mut self) {
        if !self.cfg.backup_nudge {
            return;
        }
        if !std::mem::replace(&mut self.focus.backup.checked, true) {
            let now = now_ms();
            if !bn::load(&crate::config::config_dir()).may_show(now) {
                return;
            }
            let code = self.cfg.code_folders.clone();
            let important = self.cfg.important_folders.clone();
            let (tx, rx) = mpsc::channel();
            self.focus.backup.rx = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(check(&code, &important, now));
            });
        }
        let Some(rx) = self.focus.backup.rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Some(reminder)) => self.show_backup_reminder(reminder, now_ms()),
            Ok(None) | Err(mpsc::TryRecvError::Disconnected) => {}
            Err(mpsc::TryRecvError::Empty) => self.focus.backup.rx = Some(rx),
        }
    }

    fn show_backup_reminder(&mut self, reminder: Reminder, now: u64) {
        if grokhub_core::post_update(&mut self.updates, reminder.card(now)) {
            self.persist_updates();
        }
        let dir = crate::config::config_dir();
        let mut state = bn::load(&dir);
        state.last_shown_ms = now;
        if let Err(err) = bn::save(&dir, &state) {
            self.status = err;
        }
        self.focus.backup.reminder = Some(reminder);
        self.chip_paint_key.clear();
    }

    pub(super) fn backup_chips(&self) -> Vec<QuickChip> {
        let Some(reminder) = self
            .focus
            .backup
            .reminder
            .as_ref()
            .filter(|_| self.cfg.backup_nudge)
        else {
            return Vec::new();
        };
        reminder
            .chips()
            .into_iter()
            .filter(|c| !self.chip_dismissed.iter().any(|d| d == &c.id))
            .collect()
    }

    /// Snooze is handled here, not sent. True when `chip` was it.
    pub(super) fn take_backup_chip(&mut self, chip: &QuickChip) -> bool {
        if chip.id != bn::SNOOZE_ID {
            return false;
        }
        let dir = crate::config::config_dir();
        let mut state = bn::load(&dir);
        state.snooze(now_ms());
        if let Err(err) = bn::save(&dir, &state) {
            self.status = err;
        }
        self.focus.backup.reminder = None;
        self.chip_paint_key.clear();
        self.status = format!("Backup reminders snoozed for {} days.", bn::SNOOZE_DAYS);
        true
    }

    /// The reminder's one-taps as pills under its own Pulse post, so they sit
    /// with the card and not only in the chat's chip row.
    pub(super) fn paint_backup_one_taps(
        &self,
        ui: &mut egui::Ui,
        card: &grokhub_core::UpdateCard,
    ) -> Option<String> {
        if !card.source_id.starts_with(bn::CARD_SOURCE) {
            return None;
        }
        let chips = self.backup_chips();
        let mut tapped = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
            for chip in &chips {
                if super::change_undo::click_pill(ui, &chip.label) {
                    tapped = Some(chip.id.clone());
                }
            }
        });
        tapped
    }

    /// A one-tap from the post. Snooze stays on Pulse; push and open go to
    /// Chat, where the agent asks before pushing or the tool's line runs.
    pub(super) fn tap_backup_chip(&mut self, id: &str) {
        let Some(chip) = self.backup_chips().into_iter().find(|c| c.id == id) else {
            return;
        };
        if chip.id != bn::SNOOZE_ID {
            self.nav = Nav::Chat;
        }
        self.apply_chip(chip);
    }

    pub(super) fn ui_backup_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.backup_nudge;
        if crate::cards::settings_toggle(
            ui,
            "Remind me about work that isn't backed up",
            "At most once a day: repos in your code folders with commits unpushed for 3 days, important folders changed since the last backup, and no system snapshot for 2 weeks. Uses dates and counts only. It never copies or uploads files.",
            &mut on,
        ) {
            self.cfg.backup_nudge = on;
            self.focus.backup = BackupState::default();
            self.chip_paint_key.clear();
            self.persist_cfg();
            self.status = "Saved".into();
        }
        if !self.cfg.backup_nudge {
            return;
        }
        let mut remove = None;
        ui.indent("important-folders", |ui| {
            for (i, folder) in self.cfg.important_folders.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(folder).size(crate::theme::FONT_TIP));
                    if crate::cards::ghost_pill(ui, "Remove") {
                        remove = Some(i);
                    }
                });
            }
            if crate::cards::ghost_pill(ui, "Add important folder") {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Important folder")
                    .pick_folder()
                {
                    let dir = dir.to_string_lossy().to_string();
                    if !self.cfg.important_folders.contains(&dir) {
                        self.cfg.important_folders.push(dir);
                        self.persist_cfg();
                    }
                }
            }
        });
        if let Some(i) = remove {
            self.cfg.important_folders.remove(i);
            self.persist_cfg();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reminder() -> Reminder {
        Reminder {
            lines: vec!["my-app has 2 commits on feat-x not pushed for 4 days.".into()],
            push: Some((PathBuf::from("/home/user/code/my-app"), "feat-x".into())),
            tool: None,
        }
    }

    #[test]
    fn a_reminder_posts_once_and_snooze_hides_it() {
        let root = crate::config::test_config_root("backup-nudge");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.backup_nudge = true;
        cabin.show_backup_reminder(reminder(), 5_000);
        assert!(cabin
            .updates
            .iter()
            .any(|c| c.title == "Some work isn't backed up"));
        assert_eq!(bn::load(&root).last_shown_ms, 5_000);
        let chips = cabin.backup_chips();
        let labels: Vec<&str> = chips.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["Push my-app", "Snooze 3 days"]);
        assert!(
            !cabin.take_backup_chip(&chips[0]),
            "push is sent as a chat ask"
        );
        assert!(cabin.take_backup_chip(&chips[1]));
        assert!(cabin.backup_chips().is_empty());
        assert!(bn::load(&root).snoozed_until_ms > now_ms());
        // Snoozed: the next launch check doesn't run.
        let mut next = Cabin::quiet_for_test();
        next.cfg.backup_nudge = true;
        next.tick_backup();
        assert!(next.focus.backup.rx.is_none());
    }

    #[test]
    fn the_posts_one_taps_snooze_in_place_and_push_from_chat() {
        let root = crate::config::test_config_root("backup-nudge-post");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.backup_nudge = true;
        cabin.nav = Nav::Pulse;
        cabin.show_backup_reminder(reminder(), 5_000);
        let card = cabin
            .updates
            .iter()
            .find(|c| c.title == "Some work isn't backed up")
            .cloned()
            .expect("posted");
        assert_eq!(card.source_id, "backup-nudge:0");
        cabin.tap_backup_chip("backup:push:my-app");
        assert_eq!(cabin.nav, Nav::Chat);
        cabin.nav = Nav::Pulse;
        cabin.tap_backup_chip(bn::SNOOZE_ID);
        assert_eq!(cabin.nav, Nav::Pulse, "snooze stays on Pulse");
        assert!(cabin.backup_chips().is_empty());
        assert!(bn::load(&root).snoozed_until_ms > now_ms());
    }

    #[test]
    fn off_checks_nothing() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.backup_nudge = false;
        cabin.tick_backup();
        assert!(!cabin.focus.backup.checked);
    }
}
