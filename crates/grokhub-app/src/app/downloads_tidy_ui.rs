//! Downloads tidy suggestions (`grokhub_core::downloads_tidy`), opt-in under
//! Settings → Behavior. Once a week, off the UI thread, it sorts what sits in
//! Downloads into old installers, big old archives and ISOs, archives
//! already extracted, and exact duplicates, and posts one Pulse card. The
//! review list lets the user pick; nothing moves until they approve the
//! exact list, and then it goes to the Trash or Recycle Bin, never deleted.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::*;
use grokhub_core::downloads_tidy::{self as dt, Finding};
use grokhub_core::{ChipKind, QuickChip};

/// Moves paths to the Trash; tests swap in a fake.
type Trasher = fn(&[PathBuf]) -> Result<(), String>;

#[derive(Default)]
pub(super) struct TidyState {
    rx: Option<mpsc::Receiver<Vec<Finding>>>,
    checked: bool,
    findings: Vec<Finding>,
    review: Option<Review>,
    note: String,
    trasher: Option<Trasher>,
}

/// The open review list: what is ticked, and whether the approval is showing.
#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Review {
    picked: BTreeSet<usize>,
    confirm: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TidyAct {
    Toggle(usize),
    All,
    None,
    /// Show the approval for the ticked files.
    AskTrash,
    /// The approval's "Move to Trash". Does nothing unless it is showing.
    Approve,
    Cancel,
    Close,
}

fn downloads() -> PathBuf {
    home().join("Downloads")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Where installed apps leave a name: Start Menu shortcuts on Windows,
/// desktop entries on Linux.
fn app_roots() -> Vec<PathBuf> {
    let start_menu = PathBuf::from("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    let mut roots: Vec<PathBuf> = ["ProgramData", "APPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|v| PathBuf::from(v).join(&start_menu))
        .collect();
    if !cfg!(windows) {
        let home = home();
        roots.extend([
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/var/lib/flatpak/exports/share/applications"),
            home.join(".local/share/applications"),
            home.join(".local/share/flatpak/exports/share/applications"),
        ]);
    }
    roots
}

fn scan(now: u64) -> Vec<Finding> {
    let (files, dirs) = dt::list_dir(&downloads());
    let installed = dt::installed_names(&app_roots());
    dt::classify(&files, &dirs, &installed, now, &mut |p| dt::sha256_file(p))
}

/// `GROKHUB_DECK_TIDY=approve` opens the review with every file ticked and
/// the approval showing, for the Windows deck shot.
fn deck_approve() -> bool {
    super::loadout_home::deck_switch_is("GROKHUB_DECK_TIDY", "approve")
}

impl Cabin {
    /// Each frame: the weekly check once per launch, then its result.
    pub(super) fn tick_downloads_tidy(&mut self) {
        if !self.cfg.downloads_tidy {
            return;
        }
        if !std::mem::replace(&mut self.focus.tidy.checked, true) {
            let now = now_ms();
            if !dt::load(&crate::config::config_dir()).due(now) {
                return;
            }
            let (tx, rx) = mpsc::channel();
            self.focus.tidy.rx = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(scan(now));
            });
        }
        let Some(rx) = self.focus.tidy.rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(found) => self.take_tidy_scan(found, now_ms()),
            Err(mpsc::TryRecvError::Empty) => self.focus.tidy.rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn take_tidy_scan(&mut self, found: Vec<Finding>, now: u64) {
        let dir = crate::config::config_dir();
        if let Err(err) = dt::save(&dir, &dt::Tidy { last_check_ms: now }) {
            self.status = err;
        }
        if let Some(card) = dt::card(&found, now) {
            if grokhub_core::post_update(&mut self.updates, card) {
                self.persist_updates();
            }
        }
        self.focus.tidy.findings = found;
        if deck_approve() && !self.focus.tidy.findings.is_empty() {
            self.tidy_act(TidyAct::All);
            self.tidy_act(TidyAct::AskTrash);
        }
        self.chip_paint_key.clear();
    }

    /// "Review Downloads cleanup" while there is something to review.
    pub(super) fn tidy_chips(&self) -> Vec<QuickChip> {
        let found = &self.focus.tidy.findings;
        if !self.cfg.downloads_tidy || found.is_empty() {
            return Vec::new();
        }
        let chip = QuickChip {
            id: dt::REVIEW_CHIP.into(),
            label: "Review Downloads cleanup".into(),
            value: dt::REVIEW_CHIP.into(),
            kind: ChipKind::Chat,
            score: 1.0,
            hint: format!(
                "{} files, {}",
                found.len(),
                dt::human_bytes(dt::total_bytes(found))
            ),
            primary: false,
        };
        if self.chip_dismissed.iter().any(|d| d == &chip.id) {
            return Vec::new();
        }
        vec![chip]
    }

    /// Opens the review instead of sending. True when `chip` was it.
    pub(super) fn take_tidy_chip(&mut self, chip: &QuickChip) -> bool {
        if chip.id != dt::REVIEW_CHIP {
            return false;
        }
        self.focus.tidy.review = Some(Review::default());
        true
    }

    pub(super) fn tidy_act(&mut self, act: TidyAct) {
        let count = self.focus.tidy.findings.len();
        match act {
            TidyAct::Approve => return self.trash_picked(),
            TidyAct::Close => {
                self.focus.tidy.review = None;
                return;
            }
            _ => {}
        }
        let review = self.focus.tidy.review.get_or_insert_with(Review::default);
        match act {
            TidyAct::Toggle(i) if i < count => {
                if !review.picked.remove(&i) {
                    review.picked.insert(i);
                }
                review.confirm = false;
            }
            TidyAct::All => {
                review.picked = (0..count).collect();
                review.confirm = false;
            }
            TidyAct::None => {
                review.picked.clear();
                review.confirm = false;
            }
            TidyAct::AskTrash => review.confirm = !review.picked.is_empty(),
            TidyAct::Cancel => review.confirm = false,
            TidyAct::Toggle(_) | TidyAct::Approve | TidyAct::Close => {}
        }
    }

    /// Only from the approval: the ticked files go to the Trash.
    fn trash_picked(&mut self) {
        let tidy = &mut self.focus.tidy;
        let Some(review) = tidy.review.as_ref().filter(|r| r.confirm) else {
            return;
        };
        let picked: Vec<usize> = review.picked.iter().copied().collect();
        let paths: Vec<PathBuf> = picked
            .iter()
            .filter_map(|i| tidy.findings.get(*i))
            .map(|f| f.file.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }
        let bytes: u64 = picked
            .iter()
            .filter_map(|i| tidy.findings.get(*i))
            .map(|f| f.file.bytes)
            .sum();
        let trash = tidy.trasher.unwrap_or(grokhub_desktop::trash_paths);
        match trash(&paths) {
            Ok(()) => {
                let mut i = 0;
                tidy.findings.retain(|_| {
                    let keep = !picked.contains(&i);
                    i += 1;
                    keep
                });
                tidy.review = None;
                tidy.note = format!(
                    "Moved {} {} ({}) to the Trash.",
                    paths.len(),
                    if paths.len() == 1 { "file" } else { "files" },
                    dt::human_bytes(bytes)
                );
                self.status = tidy.note.clone();
                self.chip_paint_key.clear();
            }
            Err(err) => {
                if let Some(r) = tidy.review.as_mut() {
                    r.confirm = false;
                }
                tidy.note = err;
            }
        }
    }

    /// The review window and its approval, while open.
    pub(super) fn paint_tidy_review(&mut self, ctx: &egui::Context) {
        if self.focus.tidy.review.is_none() {
            return;
        }
        let mut act: Option<TidyAct> = None;
        let mut open = true;
        egui::Window::new("Downloads cleanup")
            .collapsible(false)
            .open(&mut open)
            .default_width(560.0)
            .show(ctx, |ui| {
                act = self.ui_tidy_review(ui);
            });
        if !open {
            act = Some(TidyAct::Close);
        }
        if let Some(a) = act {
            self.tidy_act(a);
        }
    }

    fn ui_tidy_review(&self, ui: &mut egui::Ui) -> Option<TidyAct> {
        let tidy = &self.focus.tidy;
        let review = tidy.review.as_ref()?;
        let mut act = None;
        crate::cards::help_text(
            ui,
            "Pick what can go. Nothing moves until you approve the list, and then it goes to the Trash, not deleted.",
        );
        ui.horizontal(|ui| {
            if crate::cards::ghost_pill(ui, "Select all") {
                act = Some(TidyAct::All);
            }
            if crate::cards::ghost_pill(ui, "Clear") {
                act = Some(TidyAct::None);
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("tidy-review")
            .max_height(320.0)
            .show(ui, |ui| {
                for (i, f) in tidy.findings.iter().enumerate() {
                    let mut on = review.picked.contains(&i);
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut on, "").changed() {
                            act = Some(TidyAct::Toggle(i));
                        }
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{}  ·  {}",
                                    f.file.name,
                                    dt::human_bytes(f.file.bytes)
                                ))
                                .color(crate::theme::fg()),
                            );
                            ui.label(
                                RichText::new(format!("{}: {}", f.kind.label(), f.why))
                                    .size(crate::theme::FONT_TIP)
                                    .color(crate::theme::muted()),
                            );
                        });
                    });
                }
            });
        ui.add_space(8.0);
        let picked: Vec<&Finding> = review
            .picked
            .iter()
            .filter_map(|i| tidy.findings.get(*i))
            .collect();
        let bytes: u64 = picked.iter().map(|f| f.file.bytes).sum();
        if review.confirm {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "Move these {} to the Trash? ({})",
                        if picked.len() == 1 {
                            "file".to_string()
                        } else {
                            format!("{} files", picked.len())
                        },
                        dt::human_bytes(bytes)
                    ))
                    .strong()
                    .color(crate::theme::fg()),
                );
                for f in &picked {
                    ui.label(
                        RichText::new(f.file.path.to_string_lossy())
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::muted()),
                    );
                }
                crate::cards::help_text(ui, "You can restore them from the Trash.");
                ui.horizontal(|ui| {
                    if crate::cards::danger_pill(ui, "Move to Trash") {
                        act = Some(TidyAct::Approve);
                    }
                    if crate::cards::ghost_pill(ui, "Cancel") {
                        act = Some(TidyAct::Cancel);
                    }
                });
            });
        } else if !picked.is_empty()
            && crate::cards::ghost_pill(ui, &format!("Move {} to Trash…", dt::human_bytes(bytes)))
        {
            act = Some(TidyAct::AskTrash);
        }
        if !tidy.note.is_empty() {
            crate::cards::help_text(ui, &tidy.note);
        }
        act
    }

    pub(super) fn set_downloads_tidy(&mut self, on: bool) {
        self.cfg.downloads_tidy = on;
        self.focus.tidy = TidyState::default();
        self.persist_cfg();
    }

    /// Settings → Behavior: the toggle, and the review once a check found something.
    pub(super) fn ui_downloads_tidy_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.downloads_tidy;
        if crate::cards::settings_toggle(
            ui,
            "Suggest Downloads cleanups",
            "Once a week, look at the names, sizes and dates in Downloads (and hash same-size files to find exact copies) and suggest old installers, big old archives and duplicates. Nothing moves until you approve it, and then it goes to the Trash.",
            &mut on,
        ) {
            self.set_downloads_tidy(on);
            self.status = "Saved".into();
        }
        if !self.cfg.downloads_tidy {
            return;
        }
        ui.indent("downloads-tidy", |ui| {
            let found = &self.focus.tidy.findings;
            if found.is_empty() {
                crate::cards::help_text(ui, "Nothing to suggest right now.");
            } else {
                let line = format!(
                    "{} files to look at, {}.",
                    found.len(),
                    dt::human_bytes(dt::total_bytes(found))
                );
                crate::cards::help_text(ui, &line);
                if crate::cards::ghost_pill(ui, "Review") {
                    self.focus.tidy.review = Some(Review::default());
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TRASHED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    fn fake_trash(paths: &[PathBuf]) -> Result<(), String> {
        TRASHED.lock().unwrap().extend_from_slice(paths);
        Ok(())
    }

    fn finding(name: &str, bytes: u64) -> Finding {
        Finding {
            file: dt::FileInfo {
                path: PathBuf::from("/home/user/Downloads").join(name),
                name: name.into(),
                bytes,
                modified_ms: 0,
            },
            kind: dt::Kind::Duplicate,
            why: "Same file as a.bin".into(),
        }
    }

    fn cabin() -> Cabin {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.downloads_tidy = true;
        cabin.focus.tidy.trasher = Some(fake_trash);
        cabin.focus.tidy.findings = vec![
            finding("b.bin", 300 * 1024 * 1024),
            finding("c.bin", 200 * 1024 * 1024),
        ];
        cabin
    }

    #[test]
    fn nothing_moves_to_the_trash_without_the_approval() {
        TRASHED.lock().unwrap().clear();
        let mut cabin = cabin();
        assert!(cabin.take_tidy_chip(&cabin.tidy_chips()[0]));
        cabin.tidy_act(TidyAct::Toggle(0));
        cabin.tidy_act(TidyAct::Approve);
        assert!(TRASHED.lock().unwrap().is_empty(), "no approval showing");
        assert_eq!(cabin.focus.tidy.findings.len(), 2);

        cabin.tidy_act(TidyAct::AskTrash);
        cabin.tidy_act(TidyAct::Cancel);
        cabin.tidy_act(TidyAct::Approve);
        assert!(TRASHED.lock().unwrap().is_empty(), "cancelled");

        cabin.tidy_act(TidyAct::AskTrash);
        cabin.tidy_act(TidyAct::Toggle(1));
        cabin.tidy_act(TidyAct::Approve);
        assert!(
            TRASHED.lock().unwrap().is_empty(),
            "changing the list takes the approval down"
        );

        cabin.tidy_act(TidyAct::AskTrash);
        cabin.tidy_act(TidyAct::Approve);
        assert_eq!(
            *TRASHED.lock().unwrap(),
            vec![
                PathBuf::from("/home/user/Downloads/b.bin"),
                PathBuf::from("/home/user/Downloads/c.bin"),
            ]
        );
        assert!(cabin.focus.tidy.findings.is_empty());
        assert!(cabin.focus.tidy.review.is_none());
        assert_eq!(cabin.status, "Moved 2 files (500 MB) to the Trash.");
        assert!(cabin.tidy_chips().is_empty());
    }

    #[test]
    fn an_empty_pick_never_asks() {
        let mut cabin = cabin();
        cabin.tidy_act(TidyAct::AskTrash);
        assert_eq!(cabin.focus.tidy.review, Some(Review::default()));
        cabin.tidy_act(TidyAct::All);
        cabin.tidy_act(TidyAct::None);
        cabin.tidy_act(TidyAct::AskTrash);
        assert!(!cabin.focus.tidy.review.as_ref().unwrap().confirm);
    }

    #[test]
    fn off_means_no_scan_and_no_chip() {
        let mut cabin = cabin();
        cabin.cfg.downloads_tidy = false;
        cabin.tick_downloads_tidy();
        assert!(cabin.focus.tidy.rx.is_none());
        assert!(!cabin.focus.tidy.checked);
        assert!(cabin.tidy_chips().is_empty());
    }

    #[test]
    fn a_scan_posts_the_card_and_offers_the_review() {
        let _g = crate::config::hold_test_config();
        let mut cabin = cabin();
        let found = std::mem::take(&mut cabin.focus.tidy.findings);
        cabin.take_tidy_scan(found, 1_793_059_200_000);
        let card = cabin
            .updates
            .iter()
            .find(|c| c.source_id == dt::SOURCE_ID)
            .expect("card");
        assert_eq!(card.title, "500 MB of duplicates in Downloads");
        let chips = cabin.tidy_chips();
        assert_eq!(chips[0].label, "Review Downloads cleanup");
        assert_eq!(chips[0].hint, "2 files, 500 MB");
        assert_eq!(
            dt::load(&crate::config::config_dir()).last_check_ms,
            1_793_059_200_000
        );
    }
}
