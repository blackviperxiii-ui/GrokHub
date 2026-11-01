//! Battle.net games, World of Warcraft first (`grokhub_core::battlenet`).
//!
//! Battle.net games aren't in Steam, so the Library's Steam read never saw
//! them, and on Linux they live inside a Wine prefix. This reads the Agent's
//! `product.db` in every prefix it finds (and on Windows the system one),
//! lists the games in the Library, and while the game watcher is on and WoW
//! runs, follows the combat log WoW itself writes after `/combatlog` into
//! that flavor's `Logs` folder: finished Mythic+ keys, boss kills and the
//! bosses left standing post to Pulse (`game_watch_wow::WowSession`), and
//! when no log has been written a minute after WoW starts, a quiet note
//! says to type `/combatlog`. It also installs Loadout's own WoW addon
//! (`grokhub_core::wow_addon`) into a flavor when asked, and shows what that
//! addon saved (`grokhub_core::wow_saved`). Every read is a file on disk,
//! off the UI thread for the folder scan; nothing opens a handle to the
//! game, reads its memory or sends it input.

use std::path::PathBuf;

use super::*;
use grokhub_core::battlenet::{self as bnet, BnetGame, WowFlavor};
use grokhub_core::game_watch_wow::{self as wowlog, WowSession, WowWatcher};
use grokhub_core::wow_addon::{self, AddonState};
use grokhub_core::wow_saved::{self, WowSaved};

/// How often the prefixes are scanned again for new installs.
const SCAN_EVERY_MS: u64 = 5 * 60_000;
/// How often the process list is read for a running WoW.
const SAMPLE_EVERY_MS: u64 = 5_000;
/// How often the combat log is read while WoW runs.
const POLL_EVERY_MS: u64 = 1_000;
/// How long WoW runs before a missing combat log counts as logging off.
const LOG_GRACE: std::time::Duration = std::time::Duration::from_secs(60);
const LOG_NUDGE: &str = "WoW isn't writing its combat log, so Loadout can't follow your keys and pulls. Type /combatlog in game to turn it on; it stays on until you log out.";
/// The name the game watcher's toasts and recap use.
pub(super) const WOW_NAME: &str = "World of Warcraft";

/// One installed WoW flavor: its addon and what the addon saved.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct WowFlavorInfo {
    install: PathBuf,
    flavor: WowFlavor,
    dir: PathBuf,
    addon: AddonState,
    saved: WowSaved,
}

/// What a scan found: the games, and each WoW flavor's addon and data.
type Scan = (Vec<BnetGame>, Vec<WowFlavorInfo>);

#[derive(Default)]
pub(super) struct WowState {
    games: Vec<BnetGame>,
    flavors: Vec<WowFlavorInfo>,
    scanned: bool,
    scanned_ms: u64,
    scan_rx: Option<mpsc::Receiver<Scan>>,
    sampled_ms: u64,
    sample_rx: Option<mpsc::Receiver<Option<WowFlavor>>>,
    /// The flavor running now and its game folder.
    running: Option<(WowFlavor, PathBuf)>,
    watcher: Option<WowWatcher>,
    polled_ms: u64,
    /// The running WoW's keys and pulls, summed into Pulse cards.
    session: WowSession,
    /// When WoW was seen starting, for the combat log check.
    started: Option<std::time::SystemTime>,
    /// Whether combat logging was seen on this session (`None`: not checked
    /// yet), and whether the reminder showed.
    logging: Option<bool>,
    nudged: bool,
    log_checked_ms: u64,
    /// Tests set the scan and the process sample instead of reading the PC.
    scan_with: Option<fn() -> Vec<BnetGame>>,
    sample_with: Option<fn() -> Option<WowFlavor>>,
}

impl WowState {
    /// A folder scan is on its thread.
    pub(super) fn scanning(&self) -> bool {
        self.scan_rx.is_some()
    }

    pub(super) fn running(&self) -> Option<WowFlavor> {
        self.running.as_ref().map(|(f, _)| *f)
    }

    /// Whether the running WoW is writing its combat log, once that has
    /// been checked.
    pub(super) fn logging(&self) -> Option<bool> {
        self.running.as_ref().and(self.logging)
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

/// Each installed WoW flavor's addon state and saved data. A copy of the
/// addon this app installed is brought up to the built-in version here;
/// flavors without it, or with someone else's `Loadout` folder, are left
/// as they are.
pub(super) fn flavor_infos(games: &[BnetGame]) -> Vec<WowFlavorInfo> {
    let mut out = Vec::new();
    for game in library_rows(games).into_iter().filter(|g| g.is_wow()) {
        for flavor in bnet::wow_flavors(&game.install) {
            let dir = flavor.dir(&game.install);
            let _ = wow_addon::sync(&game.install, &dir);
            out.push(WowFlavorInfo {
                install: game.install.clone(),
                flavor,
                addon: wow_addon::state(&dir),
                saved: wow_saved::read_flavor(&dir),
                dir,
            });
        }
    }
    out
}

/// The addon's state in a few words.
fn addon_line(addon: &AddonState) -> String {
    match addon {
        AddonState::NotInstalled => "Loadout addon not installed".into(),
        AddonState::Current { version } => format!("Loadout addon {version}"),
        AddonState::Outdated { version } => format!("Loadout addon {version}, update ready"),
        AddonState::Foreign => "Another addon named Loadout is installed".into(),
    }
}

/// The running WoW's combat log state in a line, once it has been checked.
fn logging_line(logging: Option<bool>) -> Option<&'static str> {
    match logging? {
        true => Some("Combat log on: finished keys and boss pulls post to Pulse."),
        false => {
            Some("Combat log off: type /combatlog in game so Loadout can follow keys and pulls.")
        }
    }
}

/// The newest few kills, level-ups and achievements, newest first.
fn recent_lines(saved: &WowSaved, max: usize) -> Vec<String> {
    saved
        .events
        .iter()
        .rev()
        .filter_map(|e| {
            let who = e.character.split('-').next().unwrap_or("");
            match e.kind.as_str() {
                "kill" => Some(format!("Killed {} ({who})", e.name)),
                "level" => e.level.map(|l| format!("{who} reached level {l}")),
                "achievement" if !e.name.is_empty() => Some(format!("{who} earned {}", e.name)),
                _ => None,
            }
        })
        .take(max)
        .collect()
}

/// The tile's name: WoW by its own name for any flavor.
pub(super) fn bnet_title(game: &BnetGame) -> String {
    if game.is_wow() {
        WOW_NAME.to_string()
    } else {
        game.name()
    }
}

/// What picks a Battle.net game's painted cover; the Library tile uses the
/// same, so the detail header matches it.
pub(super) fn cover_seed(game: &BnetGame) -> u32 {
    game.product
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
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
            w.session = WowSession::default();
            return;
        }
        self.refresh_bnet(now);
        self.sample_wow(now);
        self.poll_wow_log(now);
        self.check_wow_logging(std::time::SystemTime::now(), now);
    }

    fn take_wow_results(&mut self) {
        let w = &mut self.focus.wow;
        if let Some(rx) = w.scan_rx.take() {
            match rx.try_recv() {
                Ok((games, flavors)) => {
                    w.games = games;
                    w.flavors = flavors;
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
            let games = scan();
            let flavors = flavor_infos(&games);
            let _ = tx.send((games, flavors));
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
        // WoW writes saved variables as it closes: read them again.
        if found.is_none() && w.running.is_some() {
            w.scanned_ms = 0;
        }
        // The last lines the old log holds count before its session closes.
        let now = now_ms();
        let mut cards = Vec::new();
        if let Some(watcher) = w.watcher.as_mut() {
            for event in watcher.poll(now).unwrap_or_default() {
                cards.extend(w.session.take(&event));
            }
        }
        cards.extend(w.session.close(now));
        w.watcher = found
            .as_ref()
            .map(|(_, dir)| WowWatcher::new(dir.join("Logs")));
        w.started = found.as_ref().map(|_| std::time::SystemTime::now());
        w.logging = None;
        w.nudged = false;
        w.running = found;
        self.post_wow_cards(cards);
    }

    /// A minute after WoW starts, check that it writes its combat log, and
    /// say once how to turn it on when it doesn't. A log that shows up
    /// later (the player typed /combatlog) turns the check back to on.
    fn check_wow_logging(&mut self, now: std::time::SystemTime, now_ms: u64) {
        let w = &mut self.focus.wow;
        let (Some(started), Some((_, dir))) = (w.started, w.running.as_ref()) else {
            return;
        };
        if w.logging == Some(true)
            || now.duration_since(started).unwrap_or_default() < LOG_GRACE
            || now_ms.saturating_sub(w.log_checked_ms) < SAMPLE_EVERY_MS
        {
            return;
        }
        w.log_checked_ms = now_ms;
        let on = wowlog::logging_since(&dir.join("Logs"), started);
        w.logging = Some(on);
        if !on && !w.nudged {
            w.nudged = true;
            self.toasts
                .info(LOG_NUDGE)
                .duration(Some(std::time::Duration::from_secs(20)))
                .closable(true);
        }
    }

    /// Post WoW's key, kill and session cards to Pulse.
    fn post_wow_cards(&mut self, cards: Vec<grokhub_core::UpdateCard>) {
        let mut posted = false;
        for card in cards {
            posted |= grokhub_core::post_update(&mut self.updates, card);
        }
        if posted {
            self.persist_updates();
        }
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
        if events.is_empty() {
            return;
        }
        // A line in the log means logging is on, whatever the file times say.
        w.logging = Some(true);
        let cards: Vec<_> = events.iter().flat_map(|e| w.session.take(e)).collect();
        self.post_wow_cards(cards);
    }

    /// The Battle.net games found so far; starts a scan the first time.
    pub(super) fn bnet_games(&mut self) -> Vec<BnetGame> {
        self.take_wow_results();
        self.refresh_bnet(now_ms());
        self.focus.wow.games.clone()
    }

    /// The picked Battle.net game's detail column in Library: its name,
    /// where it lives, Refresh and Open folder, and for WoW each flavor's
    /// addon and what it saved.
    pub(super) fn bnet_detail(&mut self, ui: &mut egui::Ui, game: &BnetGame) {
        ui.spacing_mut().item_spacing.y = 10.0;
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, w * 0.6), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        super::library_ui::paint_cover(&painter, rect, cover_seed(game));
        painter.text(
            egui::pos2(rect.left() + 12.0, rect.bottom() - 12.0),
            egui::Align2::LEFT_BOTTOM,
            bnet_title(game).to_uppercase(),
            crate::theme::loadout_font(15.0),
            crate::theme::fg(),
        );
        if game.is_wow() && self.focus.wow.running().is_some() {
            ui.label(
                egui::RichText::new("RUNNING")
                    .size(crate::theme::TYPE_CAPTION)
                    .strong()
                    .color(crate::theme::amber()),
            );
        }
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
        ui.horizontal(|ui| {
            if crate::cards::ghost_pill(ui, "Open folder") {
                if let Err(err) = crate::desktop::open_path(&game.install.to_string_lossy()) {
                    self.status = err;
                }
            }
            if game.is_wow() && crate::cards::ghost_pill(ui, "Refresh") {
                self.focus.wow.scanned_ms = 0;
                self.refresh_bnet(now_ms());
            }
        });
        if game.is_wow() {
            if let Some(line) = logging_line(self.focus.wow.logging()) {
                ui.label(
                    egui::RichText::new(line)
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                );
            }
            self.ui_wow_flavors(ui, &game.install);
            if !self.cfg.game_watch {
                ui.label(
                    egui::RichText::new(
                        "Turn on the game watcher in Settings → Behavior to follow boss pulls from WoW's own combat log (type /combatlog in game).",
                    )
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
                );
            }
            self.ui_bnet_profile(ui);
        }
    }

    /// Per flavor: the addon with its Install, Update or Remove button, and
    /// the characters and recent kills it saved.
    fn ui_wow_flavors(&mut self, ui: &mut egui::Ui, install: &std::path::Path) {
        let flavors: Vec<WowFlavorInfo> = self
            .focus
            .wow
            .flavors
            .iter()
            .filter(|f| f.install == install)
            .cloned()
            .collect();
        let missing = flavors
            .iter()
            .filter(|f| {
                matches!(
                    f.addon,
                    AddonState::NotInstalled | AddonState::Outdated { .. }
                )
            })
            .count();
        if missing > 1 {
            ui.add_space(6.0);
            if crate::cards::white_pill(ui, &format!("Install addon in all {missing} versions")) {
                self.install_addon_everywhere(install);
            }
        }
        for info in flavors {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        info.flavor.label(),
                        addon_line(&info.addon)
                    ))
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::fg()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let action = match info.addon {
                        AddonState::NotInstalled => crate::cards::white_pill(ui, "Install addon")
                            .then_some(AddonAction::Install),
                        AddonState::Outdated { .. } => crate::cards::white_pill(ui, "Update addon")
                            .then_some(AddonAction::Install),
                        AddonState::Current { .. } => crate::cards::ghost_pill(ui, "Remove addon")
                            .then_some(AddonAction::Remove),
                        AddonState::Foreign => None,
                    };
                    if let Some(action) = action {
                        self.run_addon_action(&info, action);
                    }
                });
            });
            let muted = |ui: &mut egui::Ui, text: String| {
                ui.label(
                    egui::RichText::new(text)
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                );
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            for c in info.saved.characters.iter().take(3) {
                let details = wow_saved::detail_lines(c, now);
                if details.is_empty() && c.gear.is_empty() {
                    muted(ui, wow_saved::character_line(c));
                    continue;
                }
                egui::CollapsingHeader::new(
                    egui::RichText::new(wow_saved::character_line(c))
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::fg()),
                )
                .id_salt(("wow-char", &info.dir, &c.name, &c.realm))
                .show(ui, |ui| {
                    for line in details {
                        muted(ui, line);
                    }
                    if !c.gear.is_empty() {
                        egui::CollapsingHeader::new(
                            egui::RichText::new("Gear").size(crate::theme::TYPE_CAPTION),
                        )
                        .id_salt(("wow-gear", &info.dir, &c.name, &c.realm))
                        .show(ui, |ui| {
                            for item in &c.gear {
                                let level =
                                    item.level.map(|l| format!(" · {l}")).unwrap_or_default();
                                muted(ui, format!("{}: {}{level}", item.slot, item.name));
                            }
                        });
                    }
                });
            }
            for line in recent_lines(&info.saved, 3) {
                muted(ui, line);
            }
            if info.addon.installed() && info.saved.characters.is_empty() {
                muted(
                    ui,
                    "Log in, then /reload or log out: WoW saves addon data then, and it shows here."
                        .into(),
                );
            }
        }
    }

    /// Install or update the addon in every flavor of one install, on the
    /// user's click, and show what happened.
    fn install_addon_everywhere(&mut self, install: &std::path::Path) {
        let results = wow_addon::install_all(install);
        let done: Vec<&str> = results
            .iter()
            .filter(|(_, r)| r.is_ok())
            .map(|(f, _)| f.label())
            .collect();
        let failed: Vec<String> = results
            .iter()
            .filter_map(|(_, r)| r.as_ref().err().cloned())
            .collect();
        self.status = if failed.is_empty() {
            format!(
                "Installed the Loadout addon for WoW {}. Quit and restart WoW to load it: the game only finds new addons when it starts.",
                done.join(", ")
            )
        } else {
            failed.join(" ")
        };
        for f in self
            .focus
            .wow
            .flavors
            .iter_mut()
            .filter(|f| f.install == install)
        {
            f.addon = wow_addon::state(&f.dir);
        }
    }

    /// Install, update or remove the addon in one flavor, on the user's
    /// click, and show the new state.
    fn run_addon_action(&mut self, info: &WowFlavorInfo, action: AddonAction) {
        let result = match action {
            AddonAction::Install => wow_addon::install(&info.install, &info.dir).map(|()| {
                // Only Retail and its test clients have the minimap's addon menu.
                let menu = if matches!(
                    info.flavor,
                    WowFlavor::Retail | WowFlavor::Ptr | WowFlavor::Xptr | WowFlavor::Beta
                ) {
                    " and sits in the minimap's addon menu"
                } else {
                    "; type /loadout for a summary"
                };
                format!(
                    "Installed the Loadout addon for WoW {}. Quit and restart WoW to load it: the game only finds new addons when it starts. Then it says hello in chat{menu}.",
                    info.flavor.label()
                )
            }),
            AddonAction::Remove => wow_addon::remove(&info.dir).map(|()| {
                format!(
                    "Removed the Loadout addon from WoW {}. What it saved stays in WoW's WTF folder.",
                    info.flavor.label()
                )
            }),
        };
        self.status = result.unwrap_or_else(|e| e);
        let state = wow_addon::state(&info.dir);
        if let Some(f) = self
            .focus
            .wow
            .flavors
            .iter_mut()
            .find(|f| f.dir == info.dir)
        {
            f.addon = state;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddonAction {
    Install,
    Remove,
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
    fn keys_and_pulls_from_the_log_post_to_pulse_and_closing_sums_the_rest() {
        let root = tmp("keys");
        let logs = root.join("_retail_").join("Logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::create_dir_all(root.join("_retail_/Interface")).unwrap();
        let (mut cabin, _dir) = cabin("wow-keys");
        cabin.cfg.game_watch = true;
        cabin.focus.wow.games = vec![wow_at(&root)];
        cabin.note_wow_running(Some(WowFlavor::Retail));
        let log = logs.join("WoWCombatLog-103126_200000.txt");
        std::fs::write(&log, "10/31/2026 20:00:00.0000  COMBAT_LOG_VERSION,22\n").unwrap();
        cabin.poll_wow_log(10_000);
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        use std::io::Write;
        for line in [
            r#"10/31/2026 20:01:00.0000  CHALLENGE_MODE_START,"Ara-Kara, City of Echoes",2660,503,10,[10,147]"#,
            r#"10/31/2026 20:05:00.0000  ENCOUNTER_START,2926,"Avanoxx",8,5,2660"#,
            r#"10/31/2026 20:07:00.0000  ENCOUNTER_END,2926,"Avanoxx",8,5,1,120000"#,
            "10/31/2026 20:29:00.0000  CHALLENGE_MODE_END,2660,1,10,1680000",
            r#"10/31/2026 21:00:00.0000  ENCOUNTER_START,2902,"Ulgrax the Devourer",16,20,2657"#,
            r#"10/31/2026 21:03:00.0000  ENCOUNTER_END,2902,"Ulgrax the Devourer",16,20,0,180000"#,
        ] {
            writeln!(f, "{line}").unwrap();
        }
        drop(f);
        cabin.poll_wow_log(20_000);
        assert_eq!(cabin.focus.wow.logging(), Some(true));
        let titles = |cabin: &Cabin| -> Vec<String> {
            cabin
                .updates
                .iter()
                .filter(|c| c.source_id == wowlog::CARD_SOURCE)
                .map(|c| c.title.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        assert_eq!(
            titles(&cabin),
            vec!["Ara-Kara, City of Echoes +10: finished in 28:00"]
        );
        // A wipe written just before WoW closes still counts.
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(
            f,
            r#"10/31/2026 21:10:00.0000  ENCOUNTER_END,2902,"Ulgrax the Devourer",16,20,0,90000"#
        )
        .unwrap();
        drop(f);
        cabin.note_wow_running(None);
        assert_eq!(
            titles(&cabin),
            vec![
                "Ara-Kara, City of Echoes +10: finished in 28:00",
                "WoW session: 2 pulls on 1 boss with no kill",
            ]
        );
    }

    #[test]
    fn a_wow_with_no_combat_log_gets_one_reminder_until_a_log_shows_up() {
        let root = tmp("nudge");
        let logs = root.join("_retail_").join("Logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::create_dir_all(root.join("_retail_/Interface")).unwrap();
        let (mut cabin, _dir) = cabin("wow-nudge");
        cabin.focus.wow.games = vec![wow_at(&root)];
        cabin.note_wow_running(Some(WowFlavor::Retail));
        let started = cabin.focus.wow.started.unwrap();
        // An old session's log doesn't count.
        let old = logs.join("WoWCombatLog-103026_200000.txt");
        std::fs::write(&old, "old\n").unwrap();
        std::fs::File::options()
            .append(true)
            .open(&old)
            .unwrap()
            .set_modified(started - std::time::Duration::from_secs(3600))
            .unwrap();
        // Inside the first minute nothing is checked.
        cabin.check_wow_logging(started + std::time::Duration::from_secs(10), 10_000);
        assert_eq!(cabin.focus.wow.logging(), None);
        assert_eq!(logging_line(None), None);
        cabin.check_wow_logging(started + std::time::Duration::from_secs(70), 70_000);
        assert_eq!(cabin.focus.wow.logging(), Some(false));
        assert!(cabin.focus.wow.nudged);
        assert!(logging_line(Some(false)).unwrap().contains("/combatlog"));
        // The player typed /combatlog: a fresh log turns it on.
        let new = logs.join("WoWCombatLog-103126_200100.txt");
        std::fs::write(&new, "new\n").unwrap();
        std::fs::File::options()
            .append(true)
            .open(&new)
            .unwrap()
            .set_modified(started + std::time::Duration::from_secs(80))
            .unwrap();
        cabin.check_wow_logging(started + std::time::Duration::from_secs(90), 90_000);
        assert_eq!(cabin.focus.wow.logging(), Some(true));
        // Closing WoW forgets it.
        cabin.note_wow_running(None);
        assert_eq!(cabin.focus.wow.logging(), None);
        assert!(!cabin.focus.wow.nudged);
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
    fn the_install_button_adds_the_addon_and_remove_takes_it_back_out() {
        let root = tmp("addon");
        std::fs::create_dir_all(root.join("_retail_/Interface")).unwrap();
        let (mut cabin, _dir) = cabin("wow-addon");
        let games = vec![wow_at(&root)];
        cabin.focus.wow.flavors = flavor_infos(&games);
        cabin.focus.wow.games = games;
        let info = cabin.focus.wow.flavors[0].clone();
        assert_eq!(info.addon, AddonState::NotInstalled);
        cabin.run_addon_action(&info, AddonAction::Install);
        assert!(cabin
            .status
            .starts_with("Installed the Loadout addon for WoW Retail"));
        assert!(root
            .join("_retail_/Interface/AddOns/Loadout/Loadout.toc")
            .is_file());
        assert!(cabin.focus.wow.flavors[0].addon.installed());
        let info = cabin.focus.wow.flavors[0].clone();
        cabin.run_addon_action(&info, AddonAction::Remove);
        assert_eq!(cabin.focus.wow.flavors[0].addon, AddonState::NotInstalled);
        assert!(!root.join("_retail_/Interface/AddOns/Loadout").exists());
    }

    #[test]
    fn one_click_installs_the_addon_in_retail_and_every_classic_flavor() {
        let root = tmp("everywhere");
        for flavor in ["_retail_", "_classic_", "_anniversary_", "_classic_era_"] {
            std::fs::create_dir_all(root.join(flavor).join("Interface")).unwrap();
        }
        let (mut cabin, _dir) = cabin("wow-everywhere");
        let games = vec![wow_at(&root)];
        cabin.focus.wow.flavors = flavor_infos(&games);
        cabin.focus.wow.games = games;
        assert_eq!(cabin.focus.wow.flavors.len(), 4);
        cabin.install_addon_everywhere(&root);
        assert!(
            cabin
                .status
                .starts_with("Installed the Loadout addon for WoW Retail, "),
            "{}",
            cabin.status
        );
        for info in &cabin.focus.wow.flavors {
            assert!(info.addon.installed(), "{:?}", info.flavor);
            assert!(info
                .dir
                .join("Interface/AddOns/Loadout/Loadout.toc")
                .is_file());
        }
    }

    #[test]
    fn a_scan_updates_an_old_copy_and_reads_what_the_addon_saved() {
        let root = tmp("saved");
        let retail = root.join("_retail_");
        std::fs::create_dir_all(retail.join("Interface")).unwrap();
        wow_addon::install(&root, &retail).unwrap();
        let toc = retail.join("Interface/AddOns/Loadout/Loadout.toc");
        let old = std::fs::read_to_string(&toc).unwrap().replace(
            &format!("## Version: {}", wow_addon::bundled_version()),
            "## Version: 0.1.0",
        );
        std::fs::write(&toc, old).unwrap();
        let sv = retail.join("WTF/Account/ACCOUNT1/SavedVariables");
        std::fs::create_dir_all(&sv).unwrap();
        std::fs::write(
            sv.join("Loadout.lua"),
            "LoadoutDB = { [\"characters\"] = { [\"Testchar-Area 52\"] = { [\"name\"] = \"Testchar\", [\"level\"] = 80, [\"className\"] = \"Paladin\", [\"lastSeen\"] = 5 } }, [\"events\"] = { { [\"kind\"] = \"kill\", [\"name\"] = \"Ulgrax\", [\"character\"] = \"Testchar-Area 52\", [\"at\"] = 4 }, { [\"kind\"] = \"wipe\", [\"name\"] = \"Bloodbound Horror\", [\"at\"] = 5 } } }\n",
        )
        .unwrap();
        let infos = flavor_infos(&[wow_at(&root)]);
        assert_eq!(infos.len(), 1);
        assert_eq!(
            infos[0].addon,
            AddonState::Current {
                version: wow_addon::bundled_version().to_string()
            }
        );
        assert_eq!(infos[0].saved.characters[0].name, "Testchar");
        assert_eq!(
            recent_lines(&infos[0].saved, 3),
            vec!["Killed Ulgrax (Testchar)"]
        );
        assert_eq!(
            addon_line(&infos[0].addon),
            format!("Loadout addon {}", wow_addon::bundled_version())
        );
    }

    #[test]
    fn someone_elses_loadout_folder_gets_no_button_and_is_not_touched() {
        let root = tmp("foreign");
        let dir = root.join("_retail_/Interface/AddOns/Loadout");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Loadout.toc"), "## Title: Something else\n").unwrap();
        let infos = flavor_infos(&[wow_at(&root)]);
        assert_eq!(infos[0].addon, AddonState::Foreign);
        assert_eq!(
            addon_line(&infos[0].addon),
            "Another addon named Loadout is installed"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("Loadout.toc")).unwrap(),
            "## Title: Something else\n"
        );
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
