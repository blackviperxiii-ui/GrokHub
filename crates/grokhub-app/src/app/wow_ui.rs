//! Battle.net games, World of Warcraft first (`grokhub_core::battlenet`).
//!
//! Battle.net games aren't in Steam, so the Library's Steam read never saw
//! them, and on Linux they live inside a Wine prefix. This reads the Agent's
//! `product.db` in every prefix it finds (and on Windows the system one),
//! lists the games in the Library, and while the game watcher is on and WoW
//! runs, follows the combat log WoW itself writes after `/combatlog` into
//! that flavor's `Logs` folder. Every read is a file on disk, off the UI
//! thread for the folder scan; nothing opens a handle to the game, reads its
//! memory or sends it input.

use std::path::PathBuf;

use super::*;
use grokhub_core::battlenet::{self as bnet, BnetGame, WowFlavor};
use grokhub_core::game_watch_wow::WowWatcher;

/// How often the prefixes are scanned again for new installs.
const SCAN_EVERY_MS: u64 = 5 * 60_000;
/// How often the process list is read for a running WoW.
const SAMPLE_EVERY_MS: u64 = 5_000;
/// How often the combat log is read while WoW runs.
const POLL_EVERY_MS: u64 = 1_000;
/// The name the game watcher's toasts and recap use.
pub(super) const WOW_NAME: &str = "World of Warcraft";

#[derive(Default)]
pub(super) struct WowState {
    games: Vec<BnetGame>,
    scanned: bool,
    scanned_ms: u64,
    scan_rx: Option<mpsc::Receiver<Vec<BnetGame>>>,
    sampled_ms: u64,
    sample_rx: Option<mpsc::Receiver<Option<WowFlavor>>>,
    /// The flavor running now and its game folder.
    running: Option<(WowFlavor, PathBuf)>,
    watcher: Option<WowWatcher>,
    polled_ms: u64,
    /// Tests set the scan and the process sample instead of reading the PC.
    scan_with: Option<fn() -> Vec<BnetGame>>,
    sample_with: Option<fn() -> Option<WowFlavor>>,
}

impl WowState {
    pub(super) fn running(&self) -> Option<WowFlavor> {
        self.running.as_ref().map(|(f, _)| *f)
    }
}

/// Every process's name and command line, the same no-handle reads the game
/// guard uses.
fn processes_now() -> Vec<(String, String)> {
    #[cfg(target_os = "linux")]
    {
        super::local_signals::processes().unwrap_or_default()
    }
    #[cfg(windows)]
    {
        grokhub_desktop::game_guard::process_names()
            .unwrap_or_default()
            .into_iter()
            .map(|(_, name)| (name, String::new()))
            .collect()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Vec::new()
    }
}

fn wow_running_now() -> Option<WowFlavor> {
    bnet::wow_running(&processes_now())
}

/// The flavors' labels for a game, in folder order (`Retail, Classic Era`).
pub(super) fn flavor_line(game: &BnetGame) -> String {
    let flavors = bnet::wow_flavors(&game.install);
    if !game.is_wow() || flavors.is_empty() {
        return String::new();
    }
    flavors
        .iter()
        .map(|f| f.label())
        .collect::<Vec<_>>()
        .join(", ")
}

/// One entry per install folder for the Library: WoW Retail and Classic
/// share a folder, and listing it twice reads as two games.
pub(super) fn library_rows(games: &[BnetGame]) -> Vec<&BnetGame> {
    let mut seen = std::collections::BTreeSet::new();
    games
        .iter()
        .filter(|g| seen.insert((g.is_wow(), g.install.clone())))
        .collect()
}

impl Cabin {
    /// Each frame: take finished scans and samples, start new ones when due,
    /// and read the combat log while WoW runs and the watcher is on.
    pub(super) fn tick_wow(&mut self) {
        let now = now_ms();
        self.take_wow_results();
        if !self.cfg.game_watch {
            let w = &mut self.focus.wow;
            w.watcher = None;
            w.running = None;
            return;
        }
        self.refresh_bnet(now);
        self.sample_wow(now);
        self.poll_wow_log(now);
    }

    fn take_wow_results(&mut self) {
        let w = &mut self.focus.wow;
        if let Some(rx) = w.scan_rx.take() {
            match rx.try_recv() {
                Ok(games) => {
                    w.games = games;
                    w.scanned = true;
                }
                Err(mpsc::TryRecvError::Empty) => w.scan_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => w.scanned = true,
            }
        }
        if let Some(rx) = w.sample_rx.take() {
            match rx.try_recv() {
                Ok(running) => self.note_wow_running(running),
                Err(mpsc::TryRecvError::Empty) => self.focus.wow.sample_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
    }

    /// Scan for Battle.net games off the UI thread, now and then.
    fn refresh_bnet(&mut self, now: u64) {
        let w = &mut self.focus.wow;
        if w.scan_rx.is_some()
            || (w.scanned_ms != 0 && now.saturating_sub(w.scanned_ms) < SCAN_EVERY_MS)
        {
            return;
        }
        w.scanned_ms = now.max(1);
        let scan = w.scan_with.unwrap_or(bnet::detect);
        let (tx, rx) = mpsc::channel();
        w.scan_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(scan());
        });
    }

    /// Read the process list off the UI thread every few seconds.
    fn sample_wow(&mut self, now: u64) {
        let w = &mut self.focus.wow;
        if w.sample_rx.is_some() || now.saturating_sub(w.sampled_ms) < SAMPLE_EVERY_MS {
            return;
        }
        w.sampled_ms = now;
        let sample = w.sample_with.unwrap_or(wow_running_now);
        let (tx, rx) = mpsc::channel();
        w.sample_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(sample());
        });
    }

    /// WoW started, switched flavor or closed: follow that flavor's log, or
    /// stop.
    fn note_wow_running(&mut self, running: Option<WowFlavor>) {
        let w = &mut self.focus.wow;
        let found = running.and_then(|flavor| {
            w.games
                .iter()
                .filter(|g| g.is_wow())
                .find_map(|g| bnet::running_flavor_dir(&g.install, flavor))
        });
        if found.as_ref().map(|(f, _)| f) == w.running.as_ref().map(|(f, _)| f) {
            return;
        }
        w.watcher = found
            .as_ref()
            .map(|(_, dir)| WowWatcher::new(dir.join("Logs")));
        w.running = found;
    }

    fn poll_wow_log(&mut self, now: u64) {
        let w = &mut self.focus.wow;
        if now.saturating_sub(w.polled_ms) < POLL_EVERY_MS {
            return;
        }
        w.polled_ms = now;
        let Some(watcher) = w.watcher.as_mut() else {
            return;
        };
        // A log that can't be read now (WoW holds it while rotating) is
        // tried again next poll.
        let events = watcher.poll(now).unwrap_or_default();
        for event in events {
            self.note_watch_event(WOW_NAME, event, now);
        }
    }

    /// The Battle.net games found so far; starts a scan the first time.
    pub(super) fn bnet_games(&mut self) -> Vec<BnetGame> {
        self.take_wow_results();
        if !self.focus.wow.scanned {
            self.refresh_bnet(now_ms());
        }
        self.focus.wow.games.clone()
    }

    /// The Library's Battle.net row: one card per install, with what's
    /// installed and whether it runs. Nothing when none was found.
    pub(super) fn ui_bnet_games(&mut self, ui: &mut egui::Ui) {
        let games = self.bnet_games();
        if self.focus.wow.scan_rx.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        let rows = library_rows(&games);
        if rows.is_empty() {
            return;
        }
        let running = self.focus.wow.running();
        ui.label(super::loadout_home::caption("Battle.net"));
        ui.add_space(6.0);
        for game in rows {
            super::loadout_home::card(ui, |ui| {
                ui.horizontal(|ui| {
                    let name = if game.is_wow() {
                        WOW_NAME.to_string()
                    } else {
                        game.name()
                    };
                    ui.label(
                        egui::RichText::new(name.to_uppercase())
                            .font(crate::theme::loadout_font(15.0))
                            .color(crate::theme::fg()),
                    );
                    if game.is_wow() && running.is_some() {
                        ui.label(
                            egui::RichText::new("RUNNING")
                                .size(crate::theme::TYPE_CAPTION)
                                .strong()
                                .color(crate::theme::amber()),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::cards::ghost_pill(ui, "Open folder") {
                            if let Err(err) =
                                crate::desktop::open_path(&game.install.to_string_lossy())
                            {
                                self.status = err;
                            }
                        }
                    });
                });
                let flavors = flavor_line(game);
                let mut line = if flavors.is_empty() {
                    String::new()
                } else {
                    format!("{flavors} · ")
                };
                line.push_str(if game.prefix.is_some() {
                    "Battle.net in a Wine prefix"
                } else {
                    "Battle.net"
                });
                ui.label(
                    egui::RichText::new(line)
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                );
                if game.is_wow() && !self.cfg.game_watch {
                    ui.label(
                        egui::RichText::new(
                            "Turn on the game watcher in Settings → Behavior to follow boss pulls from WoW's own combat log (type /combatlog in game).",
                        )
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                    );
                }
            });
            ui.add_space(8.0);
        }
        ui.add_space(8.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn cabin(label: &str) -> (Cabin, crate::config::TestConfigDir) {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let pin = crate::config::TestConfigDir::set(root);
        (Cabin::quiet_for_test(), pin)
    }

    fn tmp(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("loadout-wowui-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wow_at(root: &Path) -> BnetGame {
        BnetGame {
            product: "wow".into(),
            install: root.to_path_buf(),
            prefix: Some(PathBuf::from("/home/user/Games/battlenet")),
        }
    }

    #[test]
    fn retail_and_classic_in_one_folder_are_one_library_row() {
        let root = tmp("rows");
        std::fs::create_dir_all(root.join("_retail_/Interface")).unwrap();
        std::fs::create_dir_all(root.join("_classic_era_/WTF")).unwrap();
        let mut classic = wow_at(&root);
        classic.product = "wow_classic_era".into();
        let d4 = BnetGame {
            product: "fenris".into(),
            install: root.join("d4"),
            prefix: None,
        };
        let games = vec![wow_at(&root), classic, d4.clone()];
        let rows = library_rows(&games);
        assert_eq!(rows.len(), 2);
        assert_eq!(flavor_line(rows[0]), "Retail, Classic Era");
        assert_eq!(rows[1], &d4);
        assert_eq!(flavor_line(&d4), "");
    }

    #[test]
    fn a_running_wow_is_followed_through_its_flavor_logs_and_dropped_when_it_closes() {
        let root = tmp("follow");
        let logs = root.join("_retail_").join("Logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::create_dir_all(root.join("_retail_/Interface")).unwrap();
        let (mut cabin, _dir) = cabin("wow-follow");
        cabin.cfg.game_watch = true;
        cabin.focus.wow.games = vec![wow_at(&root)];
        cabin.focus.wow.scanned = true;
        cabin.focus.wow.scanned_ms = now_ms();
        cabin.note_wow_running(Some(WowFlavor::Retail));
        assert_eq!(
            cabin.focus.wow.running,
            Some((WowFlavor::Retail, root.join("_retail_")))
        );
        assert!(cabin.focus.wow.watcher.is_some());
        // The log that is there at start is read from its end; a pull after
        // that reaches the watcher.
        let log = logs.join("WoWCombatLog-103126_210000.txt");
        std::fs::write(&log, "10/31/2026 21:00:00.0000  COMBAT_LOG_VERSION,21\n").unwrap();
        cabin.poll_wow_log(10_000);
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        use std::io::Write;
        writeln!(
            f,
            "10/31/2026 21:01:00.0000  ENCOUNTER_START,2902,\"Ulgrax the Devourer\",16,20,2657"
        )
        .unwrap();
        drop(f);
        let events = cabin
            .focus
            .wow
            .watcher
            .as_mut()
            .unwrap()
            .poll(20_000)
            .unwrap();
        assert_eq!(events.len(), 1);
        cabin.note_wow_running(None);
        assert_eq!(cabin.focus.wow.running, None);
        assert!(cabin.focus.wow.watcher.is_none());
    }

    #[test]
    fn a_classic_exe_with_no_classic_install_is_not_followed() {
        let root = tmp("noclassic");
        std::fs::create_dir_all(root.join("_retail_/Interface")).unwrap();
        let (mut cabin, _dir) = cabin("wow-noclassic");
        cabin.focus.wow.games = vec![wow_at(&root)];
        cabin.note_wow_running(Some(WowFlavor::Classic));
        assert_eq!(cabin.focus.wow.running, None);
        assert!(cabin.focus.wow.watcher.is_none());
    }

    #[test]
    fn with_the_watcher_off_nothing_is_scanned_or_sampled() {
        let (mut cabin, _dir) = cabin("wow-off");
        cabin.cfg.game_watch = false;
        fn never() -> Vec<BnetGame> {
            panic!("scanned with the watcher off")
        }
        fn never_sample() -> Option<WowFlavor> {
            panic!("sampled with the watcher off")
        }
        cabin.focus.wow.scan_with = Some(never);
        cabin.focus.wow.sample_with = Some(never_sample);
        cabin.tick_wow();
        assert!(cabin.focus.wow.scan_rx.is_none());
        assert!(cabin.focus.wow.sample_rx.is_none());
    }
}
