//! Late-night wind-down (`grokhub_core::wind_down`), opt-in under Settings →
//! Behavior. Once a minute, off the UI thread, read the PC's idle time and
//! whether a game, a fullscreen app or another app's GPU work is on; the
//! cabin adds its own downloads and agent tasks. One soft Pulse card a night
//! offers to sleep the PC, and its button only opens the confirm sheet.

use grokhub_core::wind_down::{self as wd, Busy, Machine, Tracker, Voice};
use grokhub_core::LocalClock;

use super::*;

/// How often the PC is read while the reminder is on.
const READ_EVERY: Duration = Duration::from_secs(60);

/// Deck shots (`scripts/deck-shots.ps1`) set this to read a fixed late,
/// idle machine instead of the runner's own; `confirm` also opens the sheet.
const DECK_ENV: &str = "GROKHUB_DECK_WIND_DOWN";

#[derive(Default)]
pub(super) struct WindDownState {
    tracker: Option<Tracker>,
    rx: Option<mpsc::Receiver<Machine>>,
    last_read: Option<Instant>,
}

fn deck_check() -> bool {
    std::env::var(DECK_ENV).is_ok_and(|v| matches!(v.trim(), "1" | "confirm"))
}

fn deck_confirm() -> bool {
    std::env::var(DECK_ENV).is_ok_and(|v| v.trim() == "confirm")
}

/// 2 AM, 45 min idle, nothing running.
fn deck_machine() -> Machine {
    Machine {
        idle_secs: Some(45 * 60),
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
        if unsafe { SetSuspendState(0, 0, 0) } == 0 {
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

impl Cabin {
    fn wind_now(&mut self) -> &mut Tracker {
        self.focus
            .wind
            .tracker
            .get_or_insert_with(|| wd::load(&crate::config::config_dir()))
    }

    fn save_wind(&mut self) {
        let tracker = self.wind_now().clone();
        if let Err(err) = wd::save(&crate::config::config_dir(), &tracker) {
            self.status = err;
        }
    }

    /// The hour the user usually stops: their quiet hours, else the learned window.
    fn wind_down_hour(&mut self) -> Option<u32> {
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
        if self.running {
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
        // The deck runner has no quiet hours or rhythm: wind down at 11 PM.
        let wind_down = if deck_check() {
            Some(23)
        } else {
            self.wind_down_hour()
        };
        let busy = self.wind_down_busy().or(machine.busy());
        let Some(offer) = self
            .wind_now()
            .decide(clock, wind_down, machine.idle_secs, busy)
        else {
            return;
        };
        let voice = Voice::for_personality(&self.personality_id());
        if grokhub_core::post_update(&mut self.updates, offer.card(clock, voice)) {
            self.persist_updates();
        }
        self.wind_now().offered(clock);
        self.save_wind();
        if deck_confirm() {
            self.confirm = Some(super::confirm::ConfirmKind::SleepPc);
        }
    }

    /// The card's Sleep the PC link: the confirm sheet. False for any other link.
    pub(super) fn open_wind_down_link(&mut self, href: &str) -> bool {
        if href.trim() != wd::SLEEP_LINK {
            return false;
        }
        self.confirm = Some(super::confirm::ConfirmKind::SleepPc);
        true
    }

    /// Only from the confirm sheet.
    pub(super) fn sleep_pc_confirmed(&mut self) {
        self.status = match sleep_pc() {
            Ok(()) => "Sleeping the PC".into(),
            Err(err) => err,
        };
    }

    fn set_wind_on(&mut self, on: bool) {
        if on {
            self.wind_now().on = true;
            self.focus.wind.last_read = None;
        } else {
            self.wind_now().turn_off();
            self.focus.wind.rx = None;
        }
        self.save_wind();
    }

    /// Settings → Behavior: the wind-down toggle.
    pub(super) fn ui_wind_down_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.wind_now().on;
        if crate::cards::settings_toggle(
            ui,
            "Late-night wind-down",
            "Well past your usual wind-down, or after an hour idle with nothing running, offer once a night to sleep the PC. Never during a game, a fullscreen app, a render, a download or an agent task, and only after you confirm.",
            &mut on,
        ) {
            self.set_wind_on(on);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn late_and_idle_posts_one_card_a_night() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        armed(&mut cabin);
        assert_eq!(cabin.wind_down_hour(), Some(23));
        let before = cabin.updates.len();
        cabin.finish_wind_down(deck_machine(), &late());
        assert_eq!(cabin.updates.len(), before + 1);
        assert!(cabin
            .updates
            .iter()
            .any(|c| c.title == "2 AM and you've been idle 45 min. Sleep the PC?"));
        cabin.finish_wind_down(deck_machine(), &late());
        assert_eq!(cabin.updates.len(), before + 1, "once a night");
        assert_eq!(
            wd::load(&crate::config::config_dir()).last_night,
            Some(wd::night_of(&late()))
        );
    }

    #[test]
    fn an_agent_task_or_a_download_holds_the_card_back() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        armed(&mut cabin);
        let before = cabin.updates.len();
        cabin.running = true;
        cabin.finish_wind_down(deck_machine(), &late());
        assert_eq!(cabin.updates.len(), before);
        assert_eq!(cabin.wind_down_busy(), Some(Busy::Task));
        cabin.running = false;
        let gaming = Machine {
            game: Some(true),
            ..deck_machine()
        };
        cabin.finish_wind_down(gaming, &late());
        assert_eq!(cabin.updates.len(), before);
        assert_eq!(cabin.wind_now().last_night, None, "nothing offered yet");
    }

    #[test]
    fn the_link_opens_the_confirm_sheet_and_nothing_else() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        assert!(!cabin.open_wind_down_link("https://example.com"));
        assert_eq!(cabin.confirm, None);
        assert!(cabin.open_wind_down_link("grokhub://wind-down/sleep"));
        assert_eq!(cabin.confirm, Some(super::super::confirm::ConfirmKind::SleepPc));
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
}
