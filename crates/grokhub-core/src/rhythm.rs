//! Daily rhythm: when the user is usually at the computer, learned only from
//! the cabin's own activity (a minute counts when the user typed in the last
//! two minutes) and, opt-in, the last-played times of the user's games. No
//! keystroke, window or screen tracking: only a count per weekday and hour.
//!
//! The rhythm drives three things: low-priority nudges wait for the next
//! usually-active hour, a suggested quiet window for the Settings page, and a
//! one-time offer to put the Pulse brief at the hour the user usually sits
//! down. Quiet hours the user set always win over anything learned here.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::organs::LocalClock;

pub const FILE_NAME: &str = "rhythm.json";
/// Weekdays (0 = Sunday) by hours.
pub const SLOTS: usize = 7 * 24;
/// Below this many active minutes the rhythm says nothing yet.
pub const ENOUGH_MINUTES: u32 = 10 * 60;
/// A gap this long between active minutes starts a new sitting.
pub const SITTING_GAP_MS: u64 = 30 * 60 * 1000;
/// An hour counts as usually active at this share of the busiest hour.
pub const ACTIVE_SHARE: u32 = 4;
/// An hour counts as usually away under this share of the busiest hour.
pub const AWAY_SHARE: u32 = 20;
/// The shortest run of away hours worth suggesting as quiet hours.
pub const QUIET_MIN_HOURS: usize = 4;
/// Sittings needed before the brief offer, and at the offered hour.
pub const OFFER_SITTINGS: u32 = 5;
pub const OFFER_AT_HOUR: u32 = 3;
/// A game's last play counts as this many active minutes in its hour.
pub const PLAY_MINUTES: u32 = 30;
/// Never post two briefs closer than this.
pub const BRIEF_GAP_MS: u64 = 12 * 60 * 60 * 1000;
const MINUTE_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefOffer {
    #[default]
    NotAsked,
    Accepted,
    Declined,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rhythm {
    /// Learn from the cabin's own activity. On by default; Settings turns it off.
    #[serde(default = "yes")]
    pub on: bool,
    /// Also count the last-played times of games in the user's libraries.
    #[serde(default)]
    pub games_on: bool,
    /// Active minutes per weekday and hour, `weekday * 24 + hour`.
    #[serde(default = "empty_slots")]
    pub minutes: Vec<u32>,
    /// Sittings started in each local hour.
    #[serde(default = "empty_hours")]
    pub sittings: Vec<u32>,
    #[serde(default)]
    pub last_active_ms: u64,
    /// Last-played times already counted, by game id.
    #[serde(default)]
    pub games_seen: BTreeMap<String, u64>,
    #[serde(default)]
    pub offer: BriefOffer,
    /// The hour the Pulse brief posts at, when the user picked one.
    #[serde(default)]
    pub brief_hour: Option<u32>,
}

fn yes() -> bool {
    true
}

fn empty_slots() -> Vec<u32> {
    vec![0; SLOTS]
}

fn empty_hours() -> Vec<u32> {
    vec![0; 24]
}

impl Default for Rhythm {
    fn default() -> Self {
        Self {
            on: true,
            games_on: false,
            minutes: empty_slots(),
            sittings: empty_hours(),
            last_active_ms: 0,
            games_seen: BTreeMap::new(),
            offer: BriefOffer::NotAsked,
            brief_hour: None,
        }
    }
}

fn slot(weekday: u8, hour: u32) -> usize {
    usize::from(weekday.min(6)) * 24 + hour.min(23) as usize
}

/// Local weekday and hour of `at_ms`, given the offset of local time from UTC.
pub fn local_slot(at_ms: u64, offset_min: i32) -> (u8, u32) {
    let local_min = (at_ms / MINUTE_MS) as i64 + i64::from(offset_min);
    let day = local_min.div_euclid(1440);
    let hour = (local_min.rem_euclid(1440) / 60) as u32;
    // 1970-01-01 was a Thursday (4 with Sunday = 0).
    let weekday = (day + 4).rem_euclid(7) as u8;
    (weekday, hour)
}

/// "7 PM" style label for an hour.
pub fn hour_label(hour: u32) -> String {
    match hour % 24 {
        0 => "12 AM".into(),
        12 => "12 PM".into(),
        h if h < 12 => format!("{h} AM"),
        h => format!("{} PM", h - 12),
    }
}

impl Rhythm {
    fn fix(&mut self) {
        self.minutes.resize(SLOTS, 0);
        self.sittings.resize(24, 0);
    }

    /// One active minute at `clock`. Notes at most one per minute; a gap
    /// over [`SITTING_GAP_MS`] starts a sitting. Returns true when it counted.
    pub fn note_active(&mut self, clock: &LocalClock) -> bool {
        if !self.on {
            return false;
        }
        self.fix();
        let since = clock.now_ms.saturating_sub(self.last_active_ms);
        if self.last_active_ms != 0 && since < MINUTE_MS {
            return false;
        }
        if self.last_active_ms == 0 || since >= SITTING_GAP_MS {
            self.sittings[clock.hour.min(23) as usize] += 1;
        }
        self.minutes[slot(clock.weekday, clock.hour)] += 1;
        self.last_active_ms = clock.now_ms;
        true
    }

    /// A game's last-played time, counted once per new value. Opt-in.
    pub fn note_played(&mut self, game: &str, played_ms: u64, offset_min: i32) -> bool {
        if !self.on || !self.games_on || played_ms == 0 {
            return false;
        }
        if self.games_seen.get(game) == Some(&played_ms) {
            return false;
        }
        self.fix();
        self.games_seen.insert(game.to_string(), played_ms);
        let (weekday, hour) = local_slot(played_ms, offset_min);
        self.minutes[slot(weekday, hour)] += PLAY_MINUTES;
        true
    }

    pub fn total_minutes(&self) -> u32 {
        self.minutes.iter().sum()
    }

    pub fn enough(&self) -> bool {
        self.on && self.total_minutes() >= ENOUGH_MINUTES
    }

    /// Active minutes per hour of the day, all weekdays together.
    pub fn by_hour(&self) -> [u32; 24] {
        let mut out = [0; 24];
        for (i, m) in self.minutes.iter().enumerate().take(SLOTS) {
            out[i % 24] += m;
        }
        out
    }

    /// Minutes in one weekday and hour, for the heatmap.
    pub fn at(&self, weekday: u8, hour: u32) -> u32 {
        self.minutes.get(slot(weekday, hour)).copied().unwrap_or(0)
    }

    pub fn busiest_slot(&self) -> u32 {
        self.minutes.iter().copied().max().unwrap_or(0)
    }

    pub fn usually_active(&self, hour: u32) -> bool {
        if !self.enough() {
            return false;
        }
        let hours = self.by_hour();
        let peak = hours.iter().copied().max().unwrap_or(0);
        hours[hour.min(23) as usize] * ACTIVE_SHARE >= peak
    }

    pub fn usually_away(&self, hour: u32) -> bool {
        if !self.enough() {
            return false;
        }
        let hours = self.by_hour();
        let peak = hours.iter().copied().max().unwrap_or(0);
        hours[hour.min(23) as usize] * AWAY_SHARE < peak
    }

    /// The first usually-active hour from `hour` on (wrapping), if any.
    pub fn next_active_hour(&self, hour: u32) -> Option<u32> {
        (0..24)
            .map(|i| (hour + i) % 24)
            .find(|h| self.usually_active(*h))
    }

    /// The longest run of usually-away hours, at least [`QUIET_MIN_HOURS`]
    /// long, as `(start, end)` hours. The run may wrap past midnight.
    pub fn suggested_quiet(&self) -> Option<(u32, u32)> {
        if !self.enough() {
            return None;
        }
        let away: Vec<bool> = (0..24).map(|h| self.usually_away(h)).collect();
        if away.iter().all(|a| *a) {
            return None;
        }
        // Walk from just after an active hour so a run past midnight is whole.
        let first_active = away.iter().position(|a| !a)?;
        let mut runs: Vec<(u32, usize)> = Vec::new();
        let mut run: Option<(u32, usize)> = None;
        for i in 1..=24 {
            let h = (first_active + i) % 24;
            match (&mut run, away[h]) {
                (Some((_, len)), true) => *len += 1,
                (None, true) => run = Some((h as u32, 1)),
                (Some(_), false) => runs.extend(run.take()),
                (None, false) => {}
            }
        }
        runs.extend(run);
        // Longest first; on a tie, the run over the small hours.
        let night = |(start, len): (u32, usize)| (0..len).any(|i| (start as usize + i) % 24 == 3);
        runs.into_iter()
            .filter(|(_, len)| *len >= QUIET_MIN_HOURS)
            .max_by_key(|r| (r.1, night(*r)))
            .map(|(start, len)| (start, (start + len as u32) % 24))
    }

    /// The hour the user most often sits down, once there is enough of it.
    pub fn usual_start_hour(&self) -> Option<u32> {
        let total: u32 = self.sittings.iter().sum();
        if !self.on || total < OFFER_SITTINGS {
            return None;
        }
        let (hour, count) = self
            .sittings
            .iter()
            .enumerate()
            .max_by_key(|(h, c)| (**c, std::cmp::Reverse(*h)))?;
        (*count >= OFFER_AT_HOUR).then_some(hour as u32)
    }

    /// The hour to offer for the Pulse brief, while the offer is still open.
    pub fn brief_offer(&self) -> Option<u32> {
        if self.offer != BriefOffer::NotAsked || self.brief_hour.is_some() {
            return None;
        }
        self.usual_start_hour()
    }

    pub fn accept_brief(&mut self, hour: u32) {
        self.offer = BriefOffer::Accepted;
        self.brief_hour = Some(hour.min(23));
    }

    pub fn decline_brief(&mut self) {
        self.offer = BriefOffer::Declined;
    }

    /// Whether the brief may post now. `None`: no brief hour, the digest's
    /// own interval decides. `Some(true)`: it is the brief hour and the last
    /// brief is at least [`BRIEF_GAP_MS`] old. `Some(false)`: wait.
    pub fn brief_gate(&self, local_hour: u32, last_brief_ms: u64, now_ms: u64) -> Option<bool> {
        let hour = self.brief_hour?;
        let gap_ok = last_brief_ms == 0 || now_ms.saturating_sub(last_brief_ms) >= BRIEF_GAP_MS;
        Some(local_hour == hour && gap_ok)
    }

    /// Forget every count; settings (on, games, brief hour) stay.
    pub fn reset(&mut self) {
        self.minutes = empty_slots();
        self.sittings = empty_hours();
        self.last_active_ms = 0;
        self.games_seen.clear();
        if self.offer == BriefOffer::Declined {
            self.offer = BriefOffer::NotAsked;
        }
    }
}

pub fn load(path: &Path) -> Rhythm {
    let mut r: Rhythm = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    r.fix();
    r
}

pub fn save(path: &Path, state: &Rhythm) -> Result<(), String> {
    let text = serde_json::to_string(state).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2027-01-15 00:00 UTC, a Friday.
    const FRI: u64 = 1_800_000_000_000 - (1_800_000_000_000 % 86_400_000);

    fn clock(at_ms: u64) -> LocalClock {
        let (weekday, hour) = local_slot(at_ms, 0);
        LocalClock {
            now_ms: at_ms,
            weekday,
            hour,
            minute: ((at_ms / MINUTE_MS) % 60) as u32,
        }
    }

    /// `days` days of evenings 18:00-23:00 and mornings 08:00-09:00, every minute.
    fn evenings(days: u64) -> Rhythm {
        let mut r = Rhythm::default();
        for d in 0..days {
            let day = FRI + d * 86_400_000;
            for (from, to) in [(8, 9), (18, 23)] {
                for m in (from * 60)..(to * 60) {
                    r.note_active(&clock(day + m * MINUTE_MS));
                }
            }
        }
        r
    }

    #[test]
    fn local_slot_knows_weekdays_and_offsets() {
        assert_eq!(local_slot(FRI, 0), (5, 0));
        assert_eq!(local_slot(FRI + 19 * 3_600_000, 0), (5, 19));
        // UTC-5 at 02:00 UTC is Thursday 21:00.
        assert_eq!(local_slot(FRI + 2 * 3_600_000, -300), (4, 21));
        assert_eq!(hour_label(19), "7 PM");
        assert_eq!(hour_label(0), "12 AM");
        assert_eq!(hour_label(12), "12 PM");
        assert_eq!(hour_label(8), "8 AM");
    }

    #[test]
    fn minutes_count_once_and_gaps_start_sittings() {
        let mut r = Rhythm::default();
        let t = FRI + 19 * 3_600_000;
        assert!(r.note_active(&clock(t)));
        assert!(!r.note_active(&clock(t + 30_000)), "same minute");
        assert!(r.note_active(&clock(t + MINUTE_MS)));
        assert!(r.note_active(&clock(t + MINUTE_MS + SITTING_GAP_MS)));
        assert_eq!(r.at(5, 19), 3);
        assert_eq!(r.sittings[19], 2);
        r.on = false;
        assert!(!r.note_active(&clock(t + 3 * SITTING_GAP_MS)));
        assert_eq!(r.at(5, 19), 3);
    }

    #[test]
    fn a_week_of_evenings_makes_a_heatmap_and_a_quiet_window() {
        let r = Rhythm::default();
        assert!(!r.enough());
        assert_eq!(r.suggested_quiet(), None);
        assert!(
            !r.usually_away(3),
            "nothing learned yet means nothing is away"
        );

        let r = evenings(7);
        assert_eq!(r.total_minutes(), 7 * 6 * 60);
        assert_eq!(r.at(5, 19), 60);
        assert_eq!(r.at(0, 8), 60);
        assert_eq!(r.at(3, 14), 0);
        assert_eq!(r.busiest_slot(), 60);
        assert!(r.usually_active(19));
        assert!(r.usually_active(8));
        assert!(!r.usually_active(14));
        assert!(r.usually_away(3));
        assert_eq!(r.next_active_hour(10), Some(18));
        assert_eq!(r.next_active_hour(23), Some(8));
        // Away 23:00-08:00 and 09:00-18:00 are both 9 h; the night run wins.
        assert_eq!(r.suggested_quiet(), Some((23, 8)));
        // Seven sittings in the morning, seven in the evening: the earlier hour wins the tie.
        assert_eq!(r.usual_start_hour(), Some(8));
    }

    #[test]
    fn last_played_counts_once_and_only_when_opted_in() {
        let mut r = Rhythm::default();
        let played = FRI + 21 * 3_600_000;
        assert!(!r.note_played("570", played, 0), "off by default");
        r.games_on = true;
        assert!(r.note_played("570", played, 0));
        assert!(!r.note_played("570", played, 0), "same value again");
        assert_eq!(r.at(5, 21), PLAY_MINUTES);
        assert!(r.note_played("570", played + 86_400_000, 0));
        assert_eq!(r.at(6, 21), PLAY_MINUTES);
    }

    #[test]
    fn the_brief_offer_is_made_once_and_gates_the_brief_to_its_hour() {
        let mut r = Rhythm::default();
        let t = FRI + 19 * 3_600_000;
        for d in 0..5 {
            r.note_active(&clock(t + d * 86_400_000));
        }
        assert_eq!(r.brief_offer(), Some(19));
        assert_eq!(r.brief_gate(19, 0, t), None, "no brief hour yet");
        r.accept_brief(19);
        assert_eq!(r.brief_offer(), None);
        assert_eq!(r.brief_gate(19, 0, t), Some(true));
        assert_eq!(r.brief_gate(18, 0, t), Some(false));
        assert_eq!(
            r.brief_gate(19, t - 3_600_000, t),
            Some(false),
            "posted an hour ago"
        );
        assert_eq!(r.brief_gate(19, t - BRIEF_GAP_MS, t), Some(true));

        let mut no = Rhythm::default();
        for d in 0..5 {
            no.note_active(&clock(t + d * 86_400_000));
        }
        no.decline_brief();
        assert_eq!(no.brief_offer(), None);
        assert_eq!(no.brief_hour, None);
    }

    #[test]
    fn reset_forgets_counts_and_keeps_choices() {
        let mut r = evenings(7);
        r.games_on = true;
        r.accept_brief(19);
        r.reset();
        assert_eq!(r.total_minutes(), 0);
        assert_eq!(r.sittings.iter().sum::<u32>(), 0);
        assert!(r.games_on);
        assert_eq!(r.brief_hour, Some(19));
    }

    #[test]
    fn saves_and_loads() {
        let dir = std::env::temp_dir().join(format!("rhythm-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        let r = evenings(1);
        save(&path, &r).expect("save");
        assert_eq!(load(&path), r);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&path), Rhythm::default());
    }
}
