//! Late-night wind-down and idle power saver. Opt-in. When it is well past
//! the user's wind-down hour (their quiet hours, else the quiet window the
//! daily rhythm learned) and the PC has sat idle a while, or the PC has sat
//! idle a long time at any hour, one soft card offers to sleep the PC. Never
//! while a game, a fullscreen app, another app's GPU work, a download or an
//! agent task is running, and at most once a night. Sleeping always goes
//! through the cabin's confirm sheet; nothing here sleeps the PC.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::organs::LocalClock;

pub const FILE_NAME: &str = "wind-down.json";
pub const SOURCE_NAME: &str = "Wind-down";
/// The card's link; the cabin answers it with the confirm sheet.
pub const SLEEP_LINK: &str = "grokhub://wind-down/sleep";
/// Past the wind-down hour by this many hours counts as late.
pub const LATE_AFTER_HOURS: u32 = 1;
/// Late at night, idle this long is enough.
pub const LATE_IDLE_SECS: u64 = 20 * 60;
/// At any hour, idle this long is enough.
pub const LONG_IDLE_SECS: u64 = 60 * 60;
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
}

/// Something running that a sleep would cut off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Busy {
    Game,
    Fullscreen,
    Render,
    Download,
    Task,
}

impl Busy {
    pub fn label(self) -> &'static str {
        match self {
            Busy::Game => "a game is running",
            Busy::Fullscreen => "a fullscreen app is open",
            Busy::Render => "another app is busy on the GPU",
            Busy::Download => "a download is running",
            Busy::Task => "an agent task is running",
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
    /// The first OS-level reason not to offer, if any.
    pub fn busy(&self) -> Option<Busy> {
        if self.game == Some(true) {
            Some(Busy::Game)
        } else if self.fullscreen == Some(true) {
            Some(Busy::Fullscreen)
        } else if self.external_gpu_pct.is_some_and(|p| p >= RENDER_GPU_PCT) {
            Some(Busy::Render)
        } else {
            None
        }
    }
}

/// Why the card posts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    /// Past the wind-down hour and idle a while.
    Late { idle_min: u64 },
    /// Idle a long time at any hour.
    Idle { idle_min: u64 },
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
    /// Whether to post now, and why. `wind_down` is the hour the user
    /// usually stops (None when nothing is set or learned yet); `busy` is any
    /// reason from the cabin or the PC not to offer.
    pub fn decide(
        &self,
        clock: &LocalClock,
        wind_down: Option<u32>,
        idle_secs: Option<u64>,
        busy: Option<Busy>,
    ) -> Option<Offer> {
        if !self.on || busy.is_some() || self.last_night == Some(night_of(clock)) {
            return None;
        }
        let idle = idle_secs?;
        let idle_min = idle / 60;
        if is_late(clock.hour, wind_down) && idle >= LATE_IDLE_SECS {
            Some(Offer::Late { idle_min })
        } else if idle >= LONG_IDLE_SECS {
            Some(Offer::Idle { idle_min })
        } else {
            None
        }
    }

    /// Remember tonight's offer so the card posts once a night.
    pub fn offered(&mut self, clock: &LocalClock) {
        self.last_night = Some(night_of(clock));
    }

    pub fn turn_off(&mut self) {
        self.on = false;
    }
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
    pub fn title(&self, hour: u32) -> String {
        match self {
            Offer::Late { idle_min } => format!(
                "{} and you've been idle {}. Sleep the PC?",
                hour_label(hour),
                idle_label(*idle_min)
            ),
            Offer::Idle { idle_min } => format!(
                "Idle {} with nothing running. Sleep the PC?",
                idle_label(*idle_min)
            ),
        }
    }

    pub fn body(&self, voice: Voice) -> String {
        let nudge = match (self, voice) {
            (Offer::Late { .. }, Voice::Viper) => "Long day. Let the rig rest too.",
            (Offer::Late { .. }, Voice::Nyx) => "I'll keep your spot warm. Get some sleep.",
            (Offer::Late { .. }, Voice::Pirate) => "The night watch is over, captain.",
            (Offer::Late { .. }, Voice::Plain) => "It's well past your usual wind-down.",
            (Offer::Idle { .. }, Voice::Viper) => "Nothing's running. No reason to burn power.",
            (Offer::Idle { .. }, Voice::Nyx) => "Looks like you stepped away. I can tuck it in.",
            (Offer::Idle { .. }, Voice::Pirate) => "The ship's adrift and the crew's ashore.",
            (Offer::Idle { .. }, Voice::Plain) => "Nothing is running.",
        };
        format!(
            "{nudge} Save anything open first: sleep keeps your windows, but a power cut won't. Nothing happens until you confirm."
        )
    }

    pub fn card(&self, clock: &LocalClock, voice: Voice) -> crate::UpdateCard {
        let source_id = format!("wind-down:{}", night_of(clock));
        let mut card = crate::update_feed::digest_card(
            &source_id,
            &self.title(clock.hour),
            &self.body(voice),
            clock.now_ms,
        );
        card.pulse.source_name = Some(SOURCE_NAME.to_string());
        card.pulse.because = Some("You turned on the wind-down reminder".into());
        card.action = Some(crate::UpdateAction::DeepLink {
            href: SLEEP_LINK.to_string(),
        });
        card
    }
}

/// A wind-down card: it gets the Sleep the PC button.
pub fn is_wind_down_card(card: &crate::UpdateCard) -> bool {
    card.source_id.starts_with("wind-down:")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wednesday 2026-10-28 at `hour:minute` UTC, with the local clock equal to UTC.
    fn at(hour: u32, minute: u32) -> LocalClock {
        let day_ms = 20_754 * 86_400_000u64;
        LocalClock {
            now_ms: day_ms + u64::from(hour) * 3_600_000 + u64::from(minute) * 60_000,
            weekday: 3,
            hour,
            minute,
        }
    }

    fn on() -> Tracker {
        Tracker {
            on: true,
            last_night: None,
        }
    }

    #[test]
    fn late_and_idle_posts_with_the_hour_and_the_idle_time() {
        let offer = on().decide(&at(2, 0), Some(23), Some(45 * 60), None);
        assert_eq!(offer, Some(Offer::Late { idle_min: 45 }));
        assert_eq!(
            offer.unwrap().title(2),
            "2 AM and you've been idle 45 min. Sleep the PC?"
        );
    }

    #[test]
    fn before_the_wind_down_hour_a_short_idle_waits() {
        // 10 PM, wind-down at 11 PM: not late, and 45 min is not a long idle.
        assert_eq!(on().decide(&at(22, 0), Some(23), Some(45 * 60), None), None);
        // The first hour after wind-down is not "well past" yet.
        assert_eq!(
            on().decide(&at(23, 30), Some(23), Some(45 * 60), None),
            None
        );
        // Late but only 10 min idle: the user is still here.
        assert_eq!(on().decide(&at(1, 0), Some(23), Some(10 * 60), None), None);
    }

    #[test]
    fn a_long_idle_posts_at_any_hour_and_without_a_learned_rhythm() {
        assert_eq!(
            on().decide(&at(15, 0), None, Some(80 * 60), None),
            Some(Offer::Idle { idle_min: 80 })
        );
        assert_eq!(
            Offer::Idle { idle_min: 80 }.title(15),
            "Idle 1 hr 20 min with nothing running. Sleep the PC?"
        );
        assert_eq!(on().decide(&at(15, 0), None, Some(59 * 60), None), None);
    }

    #[test]
    fn anything_running_holds_the_card_back() {
        for busy in [
            Busy::Game,
            Busy::Fullscreen,
            Busy::Render,
            Busy::Download,
            Busy::Task,
        ] {
            assert_eq!(
                on().decide(&at(2, 0), Some(23), Some(3 * 3600), Some(busy)),
                None,
                "{busy:?}"
            );
        }
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
        let light = Machine {
            external_gpu_pct: Some(12),
            game: Some(false),
            fullscreen: Some(false),
            ..Default::default()
        };
        assert_eq!(light.busy(), None);
    }

    #[test]
    fn once_a_night_from_noon_to_noon() {
        let mut t = on();
        let first = at(1, 0);
        assert!(t.decide(&first, Some(23), Some(3600), None).is_some());
        t.offered(&first);
        // Later the same night: no second card.
        assert_eq!(t.decide(&at(4, 0), Some(23), Some(2 * 3600), None), None);
        // 1 AM and 11 PM the evening before are the same night.
        let evening_before = LocalClock {
            now_ms: first.now_ms - 2 * 3_600_000,
            weekday: 2,
            hour: 23,
            minute: 0,
        };
        assert_eq!(night_of(&evening_before), night_of(&first));
        // The next night it can post again.
        let next = LocalClock {
            now_ms: first.now_ms + 24 * 3_600_000,
            weekday: 4,
            hour: 1,
            minute: 0,
        };
        assert_eq!(night_of(&next), night_of(&first) + 1);
        assert!(t.decide(&next, Some(23), Some(3600), None).is_some());
    }

    #[test]
    fn off_never_posts_and_turning_off_keeps_it_off() {
        let mut t = on();
        t.turn_off();
        assert_eq!(t.decide(&at(2, 0), Some(23), Some(5 * 3600), None), None);
        assert_eq!(
            Tracker::default().decide(&at(2, 0), Some(23), Some(5 * 3600), None),
            None
        );
    }

    #[test]
    fn the_card_links_the_confirm_sheet_and_speaks_in_the_personality() {
        let card = Offer::Late { idle_min: 45 }.card(&at(2, 0), Voice::for_personality("Viper"));
        assert!(is_wind_down_card(&card));
        assert!(
            crate::update_feed::names_its_item(&card),
            "the feed takes it"
        );
        let idle = Offer::Idle { idle_min: 70 }.card(&at(15, 0), Voice::Plain);
        assert!(crate::update_feed::names_its_item(&idle));
        assert_eq!(
            card.title,
            "2 AM and you've been idle 45 min. Sleep the PC?"
        );
        assert_eq!(
            card.action,
            Some(crate::UpdateAction::DeepLink {
                href: "grokhub://wind-down/sleep".into()
            })
        );
        let body = card.body.as_deref().unwrap_or_default();
        assert!(
            body.starts_with("Long day. Let the rig rest too."),
            "{body}"
        );
        assert!(
            body.ends_with("Nothing happens until you confirm."),
            "{body}"
        );
        assert_eq!(
            Offer::Idle { idle_min: 60 }.body(Voice::for_personality("default")),
            "Nothing is running. Save anything open first: sleep keeps your windows, but a power cut won't. Nothing happens until you confirm."
        );
    }

    #[test]
    fn late_wraps_past_midnight_and_stops_by_the_next_morning() {
        assert!(is_late(0, Some(23)));
        assert!(is_late(9, Some(23)));
        assert!(!is_late(11, Some(23)), "12 hours on is the next day");
        assert!(!is_late(3, None));
        assert_eq!(hour_label(0), "12 AM");
        assert_eq!(hour_label(23), "11 PM");
        assert_eq!(idle_label(120), "2 hr");
    }

    #[test]
    fn the_tracker_round_trips_on_disk() {
        let dir = std::env::temp_dir().join(format!("grokhub-wind-down-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = on();
        t.offered(&at(2, 0));
        save(&dir, &t).expect("save");
        assert_eq!(load(&dir), t);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
