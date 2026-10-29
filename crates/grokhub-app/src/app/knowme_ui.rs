//! Settings → Pulse → Know me: the opt-in scan of harmless local signals,
//! one toggle per signal, and "What I know about you" with Delete on each tag
//! and Wipe all. Tags that are interests join Pulse's profile as
//! "From your PC"; deleting a tag takes its interest out again. The scan runs
//! on a worker thread, weekly or on Scan now, and only tags and counts are kept
//! in `{config}/know-me.json`.

use super::*;
use grokhub_core::interests::InterestSource;
use grokhub_core::know_me::{self, KnowMe, Scan, ScanInput, Signal};

#[derive(Default)]
pub(super) struct KnowMeState {
    /// Read from disk the first time anything needs it.
    state: Option<KnowMe>,
    rx: Option<mpsc::Receiver<Scan>>,
}

fn know_me_path() -> std::path::PathBuf {
    crate::config::config_dir().join(know_me::FILE_NAME)
}

/// The scan's inputs from this computer. Runs on the worker thread.
fn scan_input() -> ScanInput {
    let home = grokhub_core::paths::user_home().unwrap_or_else(|| std::path::PathBuf::from("."));
    let path_dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let mut app_dirs = vec![
        home.join(".local/share/applications"),
        std::path::PathBuf::from("/usr/share/applications"),
        std::path::PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    for var in ["APPDATA", "ProgramData"] {
        if let Some(dir) = std::env::var_os(var) {
            app_dirs
                .push(std::path::PathBuf::from(dir).join("Microsoft/Windows/Start Menu/Programs"));
        }
    }
    let games = grokhub_core::steam::detect_root()
        .map(|root| grokhub_core::steam::read_library(&root))
        .unwrap_or_default();
    let profile = super::local_probe::read_profile(&home);
    let gpu = profile
        .gpus
        .iter()
        .find(|g| !g.integrated)
        .or_else(|| profile.gpus.first())
        .map(|g| g.name.trim().to_string())
        .unwrap_or_default();
    let locale = ["LC_ALL", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
        .unwrap_or_default();
    ScanInput {
        home,
        path_dirs,
        app_dirs,
        games,
        hardware: (profile.ram_mb > 0).then_some((gpu, profile.ram_mb)),
        locale,
    }
}

impl Cabin {
    fn know_me(&mut self) -> &mut KnowMe {
        self.sources
            .know_me
            .state
            .get_or_insert_with(|| know_me::load(&know_me_path()))
    }

    fn save_know_me(&mut self) {
        let state = self.know_me().clone();
        if let Err(err) = know_me::save(&know_me_path(), &state) {
            self.status = err;
        }
    }

    pub(super) fn know_me_on(&mut self) -> bool {
        self.know_me().on
    }

    pub(super) fn set_know_me_on(&mut self, on: bool) {
        self.know_me().on = on;
        self.save_know_me();
        self.kick_know_me(true);
    }

    /// Turns Know me off and deletes every tag it learned, with the topics
    /// those tags added.
    pub(super) fn wipe_know_me(&mut self) {
        let gone = self.know_me().wipe();
        self.save_know_me();
        let mut dropped = false;
        for tag in gone {
            dropped |= self.drop_know_me_topic(tag.topic.as_deref());
        }
        if dropped {
            self.persist_interests();
        }
        self.status = "Know me is off and everything it learned is gone.".into();
    }

    /// Starts a scan on a worker thread. `force` ignores the weekly wait.
    pub(super) fn kick_know_me(&mut self, force: bool) {
        if self.sources.know_me.rx.is_some() {
            return;
        }
        let state = self.know_me().clone();
        if !state.on || !(force || state.due(now_ms())) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.sources.know_me.rx = Some(rx);
        let _ = std::thread::Builder::new()
            .name("know-me".into())
            .spawn(move || {
                let _ = tx.send(know_me::scan(&state, &scan_input()));
            });
    }

    /// Takes a finished scan: saves the tags and adds the interest ones to Pulse.
    pub(super) fn poll_know_me(&mut self) {
        let Some(rx) = self.sources.know_me.rx.take() else {
            return;
        };
        let scan = match rx.try_recv() {
            Ok(scan) => scan,
            Err(mpsc::TryRecvError::Empty) => {
                self.sources.know_me.rx = Some(rx);
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => return,
        };
        let now = now_ms();
        self.know_me().apply(scan, now);
        self.save_know_me();
        let topics: Vec<String> = self
            .know_me()
            .tags
            .iter()
            .filter_map(|t| t.topic.clone())
            .collect();
        let mut added = false;
        for topic in topics {
            added |= self
                .interests_now()
                .add(&topic, InterestSource::KnowMe, now);
        }
        if added {
            self.persist_interests();
        }
    }

    /// Takes a deleted tag's interest out of Pulse, unless the user added it.
    fn drop_know_me_topic(&mut self, topic: Option<&str>) -> bool {
        let Some(topic) = topic else {
            return false;
        };
        let from_pc = self
            .interests_now()
            .find(topic)
            .is_some_and(|t| t.source == InterestSource::KnowMe);
        from_pc && self.interests_now().remove(topic)
    }

    pub(super) fn ui_know_me(&mut self, ui: &mut egui::Ui) {
        self.poll_know_me();
        ui.label(
            RichText::new("Know me")
                .size(crate::theme::FONT_BODY)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.add_space(2.0);
        crate::cards::help_text(
            ui,
            "Learn your games, hobbies and setup from names and counts on this computer, never file contents, browser data, mail, messages or keys. Only the tags below are kept.",
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            crate::cards::settings_note(
                ui,
                "What chats teach goes to USER.md (you), SOUL.md (how the cabin behaves) and MEMORY.md (what you asked it to remember).",
            );
            if crate::cards::ghost_pill(ui, "See and edit") {
                self.nav = Nav::Memory;
                self.open_memory_file("USER.md");
            }
        });
        ui.add_space(6.0);
        let mut on = self.know_me().on;
        if ui
            .checkbox(
                &mut on,
                RichText::new("Know me from this computer").size(crate::theme::FONT_BODY),
            )
            .changed()
        {
            self.set_know_me_on(on);
        }
        if !on {
            ui.add_space(12.0);
            return;
        }
        let mut changed = false;
        ui.indent("know-me-signals", |ui| {
            for signal in Signal::ALL {
                let mut want = self.know_me().enabled(signal);
                let label = RichText::new(signal.label()).size(crate::theme::FONT_BODY);
                if ui
                    .checkbox(&mut want, label)
                    .on_hover_text(signal.reads())
                    .changed()
                {
                    self.know_me().set_signal(signal, want);
                    changed = true;
                }
            }
        });
        if changed {
            self.save_know_me();
        }
        ui.add_space(6.0);
        let scanning = self.sources.know_me.rx.is_some();
        if scanning {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(300));
            ui.label(
                RichText::new("Scanning…")
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::muted()),
            );
        } else if crate::cards::ghost_pill(ui, "Scan now") {
            self.kick_know_me(true);
        }
        if let Some(log) = self.know_me().log.last().cloned() {
            let parts: Vec<String> = log
                .counts
                .iter()
                .map(|(s, n)| format!("{} {n}", s.label().to_lowercase()))
                .collect();
            ui.label(
                RichText::new(format!(
                    "Last scan {}: looked at {}.",
                    super::pulse_sources_ui::ago_label(log.at_ms, now_ms()),
                    parts.join(", ")
                ))
                .size(crate::theme::FONT_TIP)
                .color(crate::theme::muted()),
            );
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new("What I know about you")
                .size(crate::theme::FONT_BODY)
                .strong()
                .color(crate::theme::fg()),
        );
        let tags = self.know_me().tags.clone();
        if tags.is_empty() {
            ui.label(
                RichText::new("Nothing yet. Scan now fills this in.")
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::muted()),
            );
        }
        let mut delete: Option<String> = None;
        for tag in &tags {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&tag.tag)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::fg()),
                );
                ui.label(
                    RichText::new(format!("from {}", tag.signal.label().to_lowercase()))
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::muted()),
                );
                if crate::cards::ghost_pill(ui, "Delete") {
                    delete = Some(tag.tag.clone());
                }
            });
        }
        if let Some(tag) = delete {
            if let Some(gone) = self.know_me().delete(&tag) {
                self.save_know_me();
                if self.drop_know_me_topic(gone.topic.as_deref()) {
                    self.persist_interests();
                }
            }
        }
        if !tags.is_empty() && crate::cards::ghost_pill(ui, "Wipe all") {
            self.wipe_know_me();
        }
        ui.add_space(16.0);
    }
}
