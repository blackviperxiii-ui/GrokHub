//! Wind-down (`grokhub_core::wind_down`), opt-in under Settings → Behavior.
//! Once a minute, off the UI thread, read the PC's idle time and whether a
//! game, a fullscreen app or another app's GPU work is on; the cabin adds
//! its own downloads and agent tasks. While the user is at the PC, one card
//! offers to set up sleep-after-idle or open the OS sleep settings. Once an
//! idle time is saved, the PC sleeps itself after that long idle.

use grokhub_core::wind_down::{self as wd, Busy, Machine, Step, Tracker, Voice};
use grokhub_core::LocalClock;

use super::*;

/// Under the switch when the PC can't report what's running.
const WIND_UNKNOWN_NOTE: &str = "This PC doesn't say whether a game, a fullscreen app or GPU work is running, so it won't sleep on its own.";

/// How often the PC is read while the reminder is on.
const READ_EVERY: Duration = Duration::from_secs(60);

/// Deck shots (`scripts/deck-shots.ps1`) set this to read a fixed machine
/// with the user present instead of the runner's own; `confirm` also opens
/// the older sleep sheet.
const DECK_ENV: &str = "GROKHUB_DECK_WIND_DOWN";

#[derive(Default)]
pub(super) struct WindDownState {
    tracker: Option<Tracker>,
    rx: Option<mpsc::Receiver<Machine>>,
    last_read: Option<Instant>,
    /// The last read couldn't see a game, fullscreen or GPU signal.
    unknown: bool,
}

fn deck_check() -> bool {
    super::loadout_home::deck_switch(DECK_ENV)
        .is_some_and(|v| matches!(v.as_str(), "1" | "confirm"))
}

fn deck_confirm() -> bool {
    super::loadout_home::deck_switch_is(DECK_ENV, "confirm")
}

/// The user is here (30 s idle), nothing running.
fn deck_machine() -> Machine {
    Machine {
        idle_secs: Some(30),
        game: Some(false),
        fullscreen: Some(false),
        external_gpu_pct: Some(3),
    }
}

fn deck_clock() -> LocalClock {
    // Local time equals UTC here; only the hour and the night matter.
    let midnight = now_ms() / 86_400_000 * 86_400_000;
    LocalClock {
        now_ms: midnight + 2 * 3_600_000,
        weekday: 3,
        hour: 2,
        minute: 0,
    }
}

/// Ask the OS to sleep. Windows sleeps rather than hibernates unless the
/// user's own power settings say otherwise.
fn sleep_pc() -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Power::SetSuspendState;
        // SAFETY: plain flags, no pointers.
        if !unsafe { SetSuspendState(false, false, false) } {
            return Err("Windows didn't sleep. Another app may be keeping it awake.".into());
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        run_sleep("pmset", &["sleepnow"])
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        run_sleep("systemctl", &["suspend"])
    }
}

#[cfg(unix)]
fn run_sleep(cmd: &str, args: &[&str]) -> Result<(), String> {
    match std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::null())
        .status()
    {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(format!("{cmd} refused to sleep the PC.")),
        Err(e) => Err(format!("Couldn't run {cmd}: {e}")),
    }
}

/// Open the OS's own sleep settings page.
fn open_os_sleep_settings() -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::win_native::open_path("ms-settings:powersleep")
    }
    #[cfg(target_os = "macos")]
    {
        spawn_first(&[(
            "open",
            &["x-apple.systempreferences:com.apple.preference.energysaver"],
        )])
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        spawn_first(&[
            ("gnome-control-center", &["power"]),
            ("systemsettings", &["kcm_powerdevilprofilesconfig"]),
            ("xfce4-power-manager-settings", &[]),
        ])
    }
}

/// Start the first of these settings apps that exists.
#[cfg(unix)]
fn spawn_first(apps: &[(&str, &[&str])]) -> Result<(), String> {
    for (cmd, args) in apps {
        let started = std::process::Command::new(cmd)
            .args(*args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if started.is_ok() {
            return Ok(());
        }
    }
    Err("Couldn't open the sleep settings. Look for Power or Sleep in your system settings.".into())
}

impl Cabin {
    fn wind_now(&mut self) -> &mut Tracker {
        self.focus
            .wind
            .tracker
            .get_or_insert_with(|| wd::load(&crate::config::config_dir()))
    }

    /// False (with the error in the status line) when the write failed.
    fn save_wind(&mut self) -> bool {
        let tracker = self.wind_now().clone();
        match wd::save(&crate::config::config_dir(), &tracker) {
            Ok(()) => true,
            Err(err) => {
                self.status = err;
                false
            }
        }
    }

    /// The hour the user usually stops: their quiet hours, else the learned window.
    pub(super) fn wind_down_hour(&mut self) -> Option<u32> {
        let (start, end) = (self.cfg.quiet_start.trim(), self.cfg.quiet_end.trim());
        if !start.is_empty() && start != end {
            if let Some(min) = grokhub_core::organs::hm_min(start) {
                return Some(min / 60);
            }
        }
        self.rhythm_now().suggested_quiet().map(|(start, _)| start)
    }

    /// Cabin work a sleep would cut off.
    fn wind_down_busy(&self) -> Option<Busy> {
        if self.running || self.bg.runs.iter().any(|r| r.live()) {
            Some(Busy::Task)
        } else if self.setup.dl.is_some() || self.setup.runtime_dl.is_some() {
            Some(Busy::Download)
        } else {
            None
        }
    }

    /// Every frame: finish a read, or start the next one when it's due.
    pub(super) fn tick_wind_down(&mut self) {
        if let Some(rx) = self.focus.wind.rx.take() {
            match rx.try_recv() {
                Ok(machine) => {
                    let clock = if deck_check() {
                        deck_clock()
                    } else {
                        Self::local_clock()
                    };
                    self.finish_wind_down(machine, &clock);
                }
                Err(mpsc::TryRecvError::Empty) => self.focus.wind.rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
            return;
        }
        if !self.wind_now().on
            || self
                .focus
                .wind
                .last_read
                .is_some_and(|t| t.elapsed() < READ_EVERY)
        {
            return;
        }
        self.focus.wind.last_read = Some(Instant::now());
        let (tx, rx) = mpsc::channel();
        self.focus.wind.rx = Some(rx);
        std::thread::spawn(move || {
            let machine = if deck_check() {
                deck_machine()
            } else {
                super::local_signals::read_machine()
            };
            let _ = tx.send(machine);
        });
    }

    fn finish_wind_down(&mut self, machine: Machine, clock: &LocalClock) {
        self.focus.wind.unknown = machine.busy() == Some(Busy::Unknown);
        let busy = self.wind_down_busy().or(machine.busy());
        match self
            .wind_now()
            .decide(clock.now_ms, machine.idle_secs, busy)
        {
            Some(Step::OfferSetup) => {
                let voice = Voice::for_personality(&self.personality_id());
                if grokhub_core::post_update(&mut self.updates, wd::Offer::Setup.card(clock, voice))
                {
                    self.persist_updates();
                }
                self.wind_now().offered();
                self.save_wind();
                if deck_confirm() {
                    self.confirm = Some(super::confirm::ConfirmKind::SleepPc);
                }
            }
            Some(Step::Sleep { .. }) => {
                self.wind_now().slept(clock.now_ms);
                self.save_wind();
                // Tests and deck shots never sleep the machine they run on.
                if !deck_check() && !cfg!(test) {
                    if let Err(err) = sleep_pc() {
                        self.status = err;
                    }
                }
            }
            None => {}
        }
    }

    /// The card's links: sleep-after-idle, the OS settings, or the older
    /// card's confirm sheet. False for any other link.
    pub(super) fn open_wind_down_link(&mut self, href: &str) -> bool {
        let href = href.trim();
        if let Some(min) = wd::auto_link_minutes(href) {
            self.set_auto_sleep(min);
            return true;
        }
        if href == wd::OS_LINK {
            self.status = match open_os_sleep_settings() {
                Ok(()) => "Opened your sleep settings".into(),
                Err(err) => err,
            };
            return true;
        }
        if href != wd::SLEEP_LINK {
            return false;
        }
        self.confirm = Some(super::confirm::ConfirmKind::SleepPc);
        true
    }

    /// Save an idle time; the PC sleeps after that long idle from now on.
    pub(super) fn set_auto_sleep(&mut self, min: u32) -> bool {
        let before = self.wind_now().clone();
        self.wind_now().set_auto_sleep(min);
        if !self.save_wind() {
            *self.wind_now() = before;
            return false;
        }
        self.focus.wind.last_read = None;
        self.status = format!(
            "The PC sleeps after {} idle. Change it in Settings › Behavior.",
            wd::idle_label(u64::from(min))
        );
        true
    }

    /// Only from the confirm sheet.
    pub(super) fn sleep_pc_confirmed(&mut self) {
        // Work may have started while the sheet was open.
        if self.wind_down_busy().is_some() {
            self.status = "Not sleeping: something is still running".into();
            return;
        }
        self.status = match sleep_pc() {
            Ok(()) => "Sleeping the PC".into(),
            Err(err) => err,
        };
    }

    pub(super) fn wind_down_on(&mut self) -> bool {
        self.wind_now().on
    }

    /// False when the switch couldn't be saved; the status says why and the
    /// switch stays as it was, so nothing acts on a change that isn't on disk.
    pub(super) fn set_wind_on(&mut self, on: bool) -> bool {
        let before = self.wind_now().clone();
        if on {
            self.wind_now().on = true;
        } else {
            self.wind_now().turn_off();
        }
        if !self.save_wind() {
            *self.wind_now() = before;
            return false;
        }
        // Only a saved switch drops the in-flight read or the last one.
        if on {
            self.focus.wind.last_read = None;
        } else {
            self.focus.wind.rx = None;
        }
        true
    }

    /// Settings → Behavior: the wind-down toggle and its idle time.
    pub(super) fn ui_wind_down_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.wind_now().on;
        if crate::cards::settings_toggle(
            ui,
            "Sleep when idle",
            "Sleep the PC after it sits idle the time you pick. Never during a game, a fullscreen app, a render, a download or an agent task.",
            &mut on,
        ) {
            self.set_wind_on(on);
        }
        if !on {
            return;
        }
        let current = self.wind_now().auto_sleep_min;
        let mut pick = None;
        ui.horizontal_wrapped(|ui| {
            ui.label("Sleep after");
            for min in wd::IDLE_CHOICES {
                let label = wd::idle_label(u64::from(min));
                if ui.selectable_label(current == Some(min), label).clicked() {
                    pick = Some(min);
                }
            }
        });
        if let Some(min) = pick {
            self.set_auto_sleep(min);
        }
        if ui.button("Open sleep settings").clicked() {
            self.open_wind_down_link(wd::OS_LINK);
        }
        if self.focus.wind.unknown {
            crate::cards::settings_note(ui, WIND_UNKNOWN_NOTE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deck_switch_needs_a_deck_shot() {
        let _lock = crate::config::hold_test_config();
        let before = std::env::var(DECK_ENV).ok();
        let before_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        unsafe { std::env::set_var(DECK_ENV, "confirm") };
        unsafe { std::env::remove_var("GROKHUB_DECK_SHOT") };
        assert!(!deck_check(), "a stray switch never fakes an idle PC");
        assert!(!deck_confirm(), "nor opens the sleep confirm");
        unsafe { std::env::set_var("GROKHUB_DECK_SHOT", "1") };
        assert!(deck_check());
        assert!(deck_confirm());
        match before {
            Some(v) => unsafe { std::env::set_var(DECK_ENV, v) },
            None => unsafe { std::env::remove_var(DECK_ENV) },
        }
        match before_shot {
            Some(v) => unsafe { std::env::set_var("GROKHUB_DECK_SHOT", v) },
            None => unsafe { std::env::remove_var("GROKHUB_DECK_SHOT") },
        }
    }

    fn late() -> LocalClock {
        LocalClock {
            now_ms: 20_754 * 86_400_000 + 2 * 3_600_000,
            weekday: 3,
            hour: 2,
            minute: 0,
        }
    }

    fn armed(cabin: &mut Cabin) {
        cabin.cfg.quiet_start = "23:00".into();
        cabin.cfg.quiet_end = "07:00".into();
        cabin.focus.wind.tracker = Some(Tracker {
            on: true,
            ..Default::default()
        });
    }

    fn away() -> Machine {
        Machine {
            idle_secs: Some(68 * 3600),
            ..deck_machine()
        }
    }

    #[test]
    fn an_idle_pc_gets_no_card_and_a_present_user_gets_the_set_up_once() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        armed(&mut cabin);
        assert_eq!(cabin.wind_down_hour(), Some(23));
        let before = cabin.updates.len();
        cabin.finish_wind_down(away(), &late());
        assert_eq!(cabin.updates.len(), before, "nobody is there to see it");
        cabin.finish_wind_down(deck_machine(), &late());
        assert_eq!(cabin.updates.len(), before + 1);
        assert!(cabin
            .updates
            .iter()
            .any(|c| c.title == "Let the PC sleep on its own when you step away?"));
        assert!(!cabin
            .updates
            .iter()
            .any(|c| c.title.contains("Sleep the PC?")));
        cabin.finish_wind_down(deck_machine(), &late());
        assert_eq!(cabin.updates.len(), before + 1, "only once");
        assert!(wd::load(&crate::config::config_dir()).setup_offered);
    }

    #[test]
    fn a_saved_idle_time_sleeps_only_when_nothing_runs() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        armed(&mut cabin);
        assert!(cabin.open_wind_down_link(&wd::auto_link(30)));
        assert_eq!(cabin.wind_now().auto_sleep_min, Some(30));
        assert_eq!(
            wd::load(&crate::config::config_dir()).auto_sleep_min,
            Some(30)
        );
        assert!(cabin.status.contains("30 min"), "{}", cabin.status);
        let before = cabin.updates.len();
        cabin.running = true;
        cabin.finish_wind_down(away(), &late());
        assert_eq!(cabin.wind_down_busy(), Some(Busy::Task));
        assert_eq!(
            cabin.wind_now().last_sleep_ms,
            None,
            "an agent task holds it"
        );
        cabin.running = false;
        let gaming = Machine {
            game: Some(true),
            ..away()
        };
        cabin.finish_wind_down(gaming, &late());
        assert_eq!(cabin.wind_now().last_sleep_ms, None, "a game holds it");
        let blind = Machine {
            game: None,
            ..away()
        };
        cabin.finish_wind_down(blind, &late());
        assert!(cabin.focus.wind.unknown);
        assert_eq!(cabin.wind_now().last_sleep_ms, None, "unknown fails closed");
        cabin.finish_wind_down(away(), &late());
        assert_eq!(cabin.wind_now().last_sleep_ms, Some(late().now_ms));
        assert_eq!(cabin.updates.len(), before, "sleeping posts no card");
    }

    #[test]
    fn the_links_set_up_open_settings_or_confirm_and_nothing_else() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        assert!(!cabin.open_wind_down_link("https://example.com"));
        assert!(!cabin.open_wind_down_link("grokhub://wind-down/auto?min=0"));
        assert_eq!(cabin.confirm, None);
        assert!(cabin.open_wind_down_link("grokhub://wind-down/sleep"));
        assert_eq!(
            cabin.confirm,
            Some(super::super::confirm::ConfirmKind::SleepPc)
        );
    }

    #[test]
    fn off_reads_nothing() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.wind.tracker = Some(Tracker::default());
        cabin.tick_wind_down();
        assert!(cabin.focus.wind.rx.is_none());
        assert!(cabin.focus.wind.last_read.is_none());
    }

    #[test]
    fn a_switch_that_cannot_be_saved_keeps_the_running_read() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        assert!(cabin.set_wind_on(true));
        let (_tx, rx) = mpsc::channel();
        cabin.focus.wind.rx = Some(rx);
        let read_at = Instant::now();
        cabin.focus.wind.last_read = Some(read_at);
        let file = crate::config::config_dir().join(grokhub_core::wind_down::FILE_NAME);
        let _ = std::fs::remove_file(&file);
        std::fs::create_dir_all(&file).unwrap();

        assert!(!cabin.set_wind_on(false));
        assert!(cabin.wind_down_on(), "the switch stays on");
        assert!(cabin.focus.wind.rx.is_some(), "the in-flight read is kept");
        assert!(!cabin.set_wind_on(true));
        assert_eq!(cabin.focus.wind.last_read, Some(read_at));

        std::fs::remove_dir_all(&file).unwrap();
        assert!(cabin.set_wind_on(false));
        assert!(cabin.focus.wind.rx.is_none(), "a saved off drops the read");
    }
}
