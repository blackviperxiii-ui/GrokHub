//! Wind-down: the PC sleeps on its own when it sits idle. Opt-in. A card
//! posted while the PC is idle is never seen (nobody is there to read it),
//! so the card comes while the user is at the PC and offers to set it up
//! once: sleep after a chosen idle time, or open the OS's own sleep
//! settings. With an idle time saved, the cabin sleeps the PC itself after
//! that long idle, never while a game, a fullscreen app, another app's GPU
//! work, a download or an agent task is running (a signal the PC can't read
//! counts as busy).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::organs::LocalClock;

pub const FILE_NAME: &str = "wind-down.json";
pub const SOURCE_NAME: &str = "Wind-down";
/// The older card's link: the cabin still answers it with the confirm sheet.
pub const SLEEP_LINK: &str = "grokhub://wind-down/sleep";
/// The card's "sleep after" button; the minutes follow `?min=`.
pub const AUTO_LINK: &str = "grokhub://wind-down/auto";
/// The card's "Open sleep settings" button: the OS's own power page.
pub const OS_LINK: &str = "grokhub://wind-down/os-settings";
/// Past the wind-down hour by this many hours counts as late.
pub const LATE_AFTER_HOURS: u32 = 1;
/// Idle less than this means the user is at the PC and can see a card.
pub const PRESENT_IDLE_SECS: u64 = 2 * 60;
/// The idle times the user can pick, in minutes.
pub const IDLE_CHOICES: [u32; 5] = [15, 30, 60, 120, 240];
/// The card's suggested idle time.
pub const SUGGESTED_IDLE_MIN: u32 = 30;
/// Another app using this much GPU is treated as a render.
pub const RENDER_GPU_PCT: u8 = 40;
/// A night runs from noon to noon, so 1 AM belongs to the evening before.
const NIGHT_SHIFT_MIN: i64 = 12 * 60;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tracker {
    #[serde(default)]
    pub on: bool,
    /// The night (local days since 1970, noon to noon) last offered.
    #[serde(default)]
    pub last_night: Option<i64>,
    /// Sleep the PC after this many minutes idle. None: not set up yet.
    #[serde(default)]
    pub auto_sleep_min: Option<u32>,
    /// The set-up card has been posted once.
    #[serde(default)]
    pub setup_offered: bool,
    /// When the cabin last asked the OS to sleep (Unix ms), so a refused
    /// sleep isn't retried every minute.
    #[serde(default)]
    pub last_sleep_ms: Option<u64>,
}

/// Something running that a sleep would cut off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Busy {
    Game,
    Fullscreen,
    Render,
    Download,
    Task,
    /// This PC doesn't report one of the game, fullscreen or GPU signals, so
    /// the offer fails closed.
    Unknown,
}

impl Busy {
    pub fn label(self) -> &'static str {
        match self {
            Busy::Game => "a game is running",
            Busy::Fullscreen => "a fullscreen app is open",
            Busy::Render => "another app is busy on the GPU",
            Busy::Download => "a download is running",
            Busy::Task => "an agent task is running",
            Busy::Unknown => {
                "this PC can't tell whether a game, fullscreen app or GPU work is running"
            }
        }
    }
}

/// What the PC reported: idle time and the OS-level busy signals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Machine {
    pub idle_secs: Option<u64>,
    pub game: Option<bool>,
    pub fullscreen: Option<bool>,
    pub external_gpu_pct: Option<u8>,
}

impl Machine {
    /// The first OS-level reason not to offer, if any. A signal the PC
    /// can't read counts as busy: the offer fails closed.
    pub fn busy(&self) -> Option<Busy> {
        if self.game == Some(true) {
            Some(Busy::Game)
        } else if self.fullscreen == Some(true) {
            Some(Busy::Fullscreen)
        } else if self.external_gpu_pct.is_some_and(|p| p >= RENDER_GPU_PCT) {
            Some(Busy::Render)
        } else if self.game.is_none()
            || self.fullscreen.is_none()
            || self.external_gpu_pct.is_none()
        {
            Some(Busy::Unknown)
        } else {
            None
        }
    }
}

/// What the once-a-minute read should do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The user is here and nothing is set up: post the set-up card once.
    OfferSetup,
    /// Idle past the saved time with nothing running: sleep the PC.
    Sleep { idle_min: u64 },
}

/// The set-up card. Kept as `Offer` for callers that build it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    Setup,
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

pub fn load(config_dir: &Path) -> Tracker {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, tracker: &Tracker) -> Result<(), String> {
    let text = serde_json::to_string_pretty(tracker).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&path(config_dir), &text)
}

/// The local night `clock` falls in: local days since 1970, shifted by noon.
pub fn night_of(clock: &LocalClock) -> i64 {
    let offset = crate::pulse_writer::utc_offset_min(clock.hour, clock.minute, clock.now_ms);
    let local_min = (clock.now_ms / 60_000) as i64 + i64::from(offset);
    (local_min - NIGHT_SHIFT_MIN).div_euclid(24 * 60)
}

/// Hours since `wind_down` (0..24), counting past midnight.
fn hours_past(hour: u32, wind_down: u32) -> u32 {
    (hour.min(23) + 24 - wind_down.min(23)) % 24
}

/// Late: at least [`LATE_AFTER_HOURS`] past the wind-down hour, and still
/// within the first half of the night after it (not the next afternoon).
pub fn is_late(hour: u32, wind_down: Option<u32>) -> bool {
    wind_down.is_some_and(|w| (LATE_AFTER_HOURS..12).contains(&hours_past(hour, w)))
}

impl Tracker {
    /// What to do now. `idle_secs` is how long the PC has had no input;
    /// `busy` is any reason from the cabin or the PC not to sleep.
    pub fn decide(&self, now_ms: u64, idle_secs: Option<u64>, busy: Option<Busy>) -> Option<Step> {
        if !self.on {
            return None;
        }
        let idle = idle_secs?;
        match self.auto_sleep_min {
            None if !self.setup_offered && idle < PRESENT_IDLE_SECS => Some(Step::OfferSetup),
            None => None,
            Some(min) => {
                let after = u64::from(min.max(1)) * 60;
                let retry_ok = self
                    .last_sleep_ms
                    .is_none_or(|t| now_ms.saturating_sub(t) >= after * 1000);
                (busy.is_none() && idle >= after && retry_ok).then_some(Step::Sleep {
                    idle_min: idle / 60,
                })
            }
        }
    }

    /// The set-up card went up; it never posts again.
    pub fn offered(&mut self) {
        self.setup_offered = true;
    }

    /// The user picked an idle time: the automation is on.
    pub fn set_auto_sleep(&mut self, min: u32) {
        self.on = true;
        self.auto_sleep_min = Some(min.max(1));
        self.setup_offered = true;
    }

    /// A sleep was asked for now.
    pub fn slept(&mut self, now_ms: u64) {
        self.last_sleep_ms = Some(now_ms);
    }

    pub fn turn_off(&mut self) {
        self.on = false;
    }
}

/// The minutes in a "sleep after" link, when it is one.
pub fn auto_link_minutes(href: &str) -> Option<u32> {
    let rest = href.trim().strip_prefix(AUTO_LINK)?.strip_prefix("?min=")?;
    rest.parse::<u32>()
        .ok()
        .filter(|m| (1..=24 * 60).contains(m))
}

/// The "sleep after" link for `min` minutes.
pub fn auto_link(min: u32) -> String {
    format!("{AUTO_LINK}?min={min}")
}

/// "2 AM", "11 PM".
pub fn hour_label(hour: u32) -> String {
    match hour.min(23) {
        0 => "12 AM".into(),
        h @ 1..=11 => format!("{h} AM"),
        12 => "12 PM".into(),
        h => format!("{} PM", h - 12),
    }
}

/// "45 min", "1 hr 20 min".
pub fn idle_label(min: u64) -> String {
    match (min / 60, min % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

/// The card's tone follows the personality; the facts never change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice {
    Plain,
    Viper,
    Nyx,
    Pirate,
}

impl Voice {
    pub fn for_personality(id: &str) -> Self {
        match id.trim().to_ascii_lowercase().as_str() {
            "viper" => Voice::Viper,
            "nyx" => Voice::Nyx,
            "pirate" => Voice::Pirate,
            _ => Voice::Plain,
        }
    }
}

impl Offer {
    pub fn title(&self) -> String {
        "Let the PC sleep on its own when you step away?".into()
    }

    pub fn body(&self, voice: Voice) -> String {
        let nudge = match voice {
            Voice::Viper => "No reason to burn power while you're gone.",
            Voice::Nyx => "I can tuck it in for you when you wander off.",
            Voice::Pirate => "No sense leaving the ship lit while the crew's ashore.",
            Voice::Plain => "An idle PC can't show you a card, so set this up once.",
        };
        format!(
            "{nudge} Pick how long it sits idle before it sleeps, and it sleeps then, never during a game, a fullscreen app, a render, a download or an agent task. Or use your system's own sleep settings."
        )
    }

    pub fn card(&self, clock: &LocalClock, voice: Voice) -> crate::UpdateCard {
        let mut card = crate::update_feed::digest_card(
            "wind-down:setup",
            &self.title(),
            &self.body(voice),
            clock.now_ms,
        );
        card.pulse.source_name = Some(SOURCE_NAME.to_string());
        card.pulse.because = Some("You turned on wind-down".into());
        card.action = Some(crate::UpdateAction::DeepLink {
            href: auto_link(SUGGESTED_IDLE_MIN),
        });
        card
    }
}

/// The card's buttons: (label, link), the suggested idle time first.
pub fn card_buttons() -> Vec<(String, String)> {
    vec![
        (
            format!(
                "Sleep after {} idle",
                idle_label(u64::from(SUGGESTED_IDLE_MIN))
            ),
            auto_link(SUGGESTED_IDLE_MIN),
        ),
        ("Open sleep settings".into(), OS_LINK.into()),
    ]
}

/// A wind-down card: it gets the set-up buttons.
pub fn is_wind_down_card(card: &crate::UpdateCard) -> bool {
    card.source_id.starts_with("wind-down:")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 20_754 * 86_400_000 + 15 * 3_600_000;

    fn at(hour: u32) -> LocalClock {
        LocalClock {
            now_ms: 20_754 * 86_400_000 + u64::from(hour) * 3_600_000,
            weekday: 3,
            hour,
            minute: 0,
        }
    }

    fn on() -> Tracker {
        Tracker {
            on: true,
            ..Default::default()
        }
    }

    #[test]
    fn an_idle_pc_never_gets_a_card() {
        // Nobody is there to read it: no card at any idle length.
        for idle in [5 * 60, 45 * 60, 68 * 3600] {
            assert_eq!(on().decide(NOW, Some(idle), None), None, "{idle}");
        }
    }

    #[test]
    fn the_set_up_card_posts_once_while_the_user_is_here() {
        let mut t = on();
        assert_eq!(t.decide(NOW, Some(20), None), Some(Step::OfferSetup));
        t.offered();
        assert_eq!(t.decide(NOW, Some(20), None), None, "only once");
        assert_eq!(Tracker::default().decide(NOW, Some(20), None), None, "off");
        assert_eq!(on().decide(NOW, None, None), None, "no idle reading");
    }

    #[test]
    fn a_saved_idle_time_sleeps_the_pc_when_nothing_runs() {
        let mut t = on();
        t.set_auto_sleep(30);
        assert!(t.setup_offered);
        assert_eq!(t.decide(NOW, Some(29 * 60), None), None);
        assert_eq!(
            t.decide(NOW, Some(31 * 60), None),
            Some(Step::Sleep { idle_min: 31 })
        );
        for busy in [
            Busy::Game,
            Busy::Fullscreen,
            Busy::Render,
            Busy::Download,
            Busy::Task,
            Busy::Unknown,
        ] {
            assert_eq!(t.decide(NOW, Some(3 * 3600), Some(busy)), None, "{busy:?}");
        }
        // A refused sleep waits a full idle stretch before trying again.
        t.slept(NOW);
        assert_eq!(t.decide(NOW + 60_000, Some(40 * 60), None), None);
        assert!(t.decide(NOW + 31 * 60_000, Some(70 * 60), None).is_some());
        t.turn_off();
        assert_eq!(t.decide(NOW + 3_600_000, Some(3 * 3600), None), None);
    }

    #[test]
    fn busy_signals_fail_closed() {
        let gaming = Machine {
            idle_secs: Some(3600),
            game: Some(true),
            ..Default::default()
        };
        assert_eq!(gaming.busy(), Some(Busy::Game));
        let render = Machine {
            external_gpu_pct: Some(85),
            ..Default::default()
        };
        assert_eq!(render.busy(), Some(Busy::Render));
        let known = Machine {
            idle_secs: Some(3600),
            game: Some(false),
            fullscreen: Some(false),
            external_gpu_pct: Some(5),
        };
        assert_eq!(known.busy(), None);
        for m in [
            Machine {
                game: None,
                ..known
            },
            Machine {
                fullscreen: None,
                ..known
            },
            Machine {
                external_gpu_pct: None,
                ..known
            },
        ] {
            assert_eq!(m.busy(), Some(Busy::Unknown), "{m:?}");
        }
    }

    #[test]
    fn the_card_offers_an_idle_time_or_the_os_settings() {
        let card = Offer::Setup.card(&at(15), Voice::for_personality("Viper"));
        assert!(is_wind_down_card(&card));
        assert!(
            crate::update_feed::names_its_item(&card),
            "the feed takes it"
        );
        assert_eq!(
            card.title,
            "Let the PC sleep on its own when you step away?"
        );
        assert!(!card.title.contains("Idle"), "{}", card.title);
        let body = card.body.as_deref().unwrap_or_default();
        assert!(body.starts_with("No reason to burn power"), "{body}");
        let buttons = card_buttons();
        assert_eq!(buttons[0].0, "Sleep after 30 min idle");
        assert_eq!(auto_link_minutes(&buttons[0].1), Some(30));
        assert_eq!(buttons[1], ("Open sleep settings".into(), OS_LINK.into()));
        assert_eq!(auto_link_minutes(OS_LINK), None);
        assert_eq!(auto_link_minutes("grokhub://wind-down/auto?min=0"), None);
        assert_eq!(auto_link_minutes("grokhub://wind-down/auto?min=x"), None);
    }

    #[test]
    fn labels_and_nights() {
        assert!(is_late(0, Some(23)));
        assert!(!is_late(11, Some(23)));
        assert_eq!(hour_label(0), "12 AM");
        assert_eq!(hour_label(23), "11 PM");
        assert_eq!(idle_label(120), "2 hr");
        assert_eq!(idle_label(80), "1 hr 20 min");
        assert_eq!(night_of(&at(1)), night_of(&at(1)));
    }

    #[test]
    fn the_tracker_round_trips_on_disk_and_reads_old_files() {
        let dir = std::env::temp_dir().join(format!("grokhub-wind-down-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = on();
        t.set_auto_sleep(60);
        t.slept(NOW);
        save(&dir, &t).expect("save");
        assert_eq!(load(&dir), t);
        let old: Tracker = serde_json::from_str(r#"{"on":true,"lastNight":20754}"#).unwrap();
        assert!(old.on && old.auto_sleep_min.is_none() && !old.setup_offered);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
