//! Internet drop and speed log. Opt-in. About once a minute the cabin times
//! a TCP connect to two public DNS resolvers; when both fail twice in a row
//! the connection counts as down until one answers again. Outages, slow
//! minutes and on-demand speed tests go in a small local file the user can
//! export as CSV for their ISP. Only reachability and timings are kept:
//! no addresses of the user's own, no traffic, no ISP account data.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::update_feed::{digest_card, UpdateCard};

pub const FILE_NAME: &str = "net-log.json";
/// The public resolvers each probe tries, in order.
pub const PROBE_HOSTS: [&str; 2] = ["1.1.1.1:443", "9.9.9.9:443"];
pub const PROBE_EVERY_MS: u64 = 60_000;
/// No probe for this long means the app was closed or the PC asleep, not a
/// stalled probe thread; an outage that ends across such a gap is closed at
/// the last failed probe, not at the next launch.
pub const CLOSED_GAP_MS: u64 = 15 * PROBE_EVERY_MS;
/// Failed probes in a row before an outage starts.
pub const FAILS_TO_DROP: u32 = 2;
/// A connect slower than this counts as a slow minute.
pub const SLOW_MS: u32 = 300;
/// What the log keeps.
pub const KEEP_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// A speed test downloads this many bytes.
pub const SPEED_BYTES: u64 = 10_000_000;
pub const SPEED_URL: &str = "https://speed.cloudflare.com/__down?bytes=10000000";
const HOUR_MS: u64 = 60 * 60 * 1000;
const DAY_MS: u64 = 24 * HOUR_MS;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outage {
    pub start_ms: u64,
    /// `None` while still down.
    #[serde(default)]
    pub end_ms: Option<u64>,
    /// Minutes east of UTC when it started, for "around 2 AM".
    #[serde(default)]
    pub offset_min: i32,
}

impl Outage {
    pub fn minutes(&self, now_ms: u64) -> u64 {
        self.end_ms.unwrap_or(now_ms).saturating_sub(self.start_ms) / 60_000
    }

    pub fn local_hour(&self) -> u32 {
        let min = (self.start_ms / 60_000) as i64 + i64::from(self.offset_min);
        (min.rem_euclid(1440) / 60) as u32
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slow {
    pub at_ms: u64,
    pub latency_ms: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedTest {
    pub at_ms: u64,
    pub mbps: f64,
}

/// What a change of state means for the cabin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Dropped,
    /// Back up after this many minutes.
    Restored(u64),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetLog {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub outages: Vec<Outage>,
    #[serde(default)]
    pub slow: Vec<Slow>,
    #[serde(default)]
    pub speed: Vec<SpeedTest>,
    /// The last time an insight card posted.
    #[serde(default)]
    pub last_insight_ms: u64,
    /// When the last probe ran, so an outage that ended while the app was
    /// closed is closed at its last failed probe, not at the next launch.
    #[serde(default)]
    pub last_probe_ms: u64,
    #[serde(skip)]
    pub fails: u32,
}

impl NetLog {
    pub fn down(&self) -> bool {
        self.outages.last().is_some_and(|o| o.end_ms.is_none())
    }

    /// One probe: the connect time, or `None` when every host failed.
    pub fn note(
        &mut self,
        latency_ms: Option<u32>,
        now_ms: u64,
        offset_min: i32,
    ) -> Option<Change> {
        self.trim(now_ms);
        let last_probe_ms = std::mem::replace(&mut self.last_probe_ms, now_ms);
        match latency_ms {
            None => {
                self.fails += 1;
                if self.fails >= FAILS_TO_DROP && !self.down() {
                    // The first failed probe is when it went down.
                    let start = now_ms.saturating_sub(PROBE_EVERY_MS * u64::from(self.fails - 1));
                    self.outages.push(Outage {
                        start_ms: start,
                        end_ms: None,
                        offset_min,
                    });
                    return Some(Change::Dropped);
                }
                None
            }
            Some(ms) => {
                self.fails = 0;
                if ms > SLOW_MS {
                    self.slow.push(Slow {
                        at_ms: now_ms,
                        latency_ms: ms,
                    });
                }
                let open = self.outages.last_mut().filter(|o| o.end_ms.is_none())?;
                // No probes for [`CLOSED_GAP_MS`]: the app was closed, so the
                // outage is only known to have lasted one more interval.
                let end =
                    if last_probe_ms > 0 && now_ms.saturating_sub(last_probe_ms) > CLOSED_GAP_MS {
                        (last_probe_ms + PROBE_EVERY_MS).max(open.start_ms)
                    } else {
                        now_ms
                    };
                open.end_ms = Some(end);
                Some(Change::Restored(open.minutes(now_ms)))
            }
        }
    }

    pub fn note_speed(&mut self, bytes: u64, elapsed_ms: u64, now_ms: u64) -> Option<f64> {
        if elapsed_ms == 0 {
            return None;
        }
        let mbps = bytes as f64 * 8.0 / 1_000_000.0 / (elapsed_ms as f64 / 1000.0);
        let mbps = (mbps * 10.0).round() / 10.0;
        self.speed.push(SpeedTest {
            at_ms: now_ms,
            mbps,
        });
        Some(mbps)
    }

    fn trim(&mut self, now_ms: u64) {
        let cut = now_ms.saturating_sub(KEEP_MS);
        self.outages
            .retain(|o| o.end_ms.is_none() || o.start_ms >= cut);
        self.slow.retain(|s| s.at_ms >= cut);
        self.speed.retain(|s| s.at_ms >= cut);
    }

    /// Outages that started in the last `window_ms`.
    pub fn recent(&self, now_ms: u64, window_ms: u64) -> Vec<&Outage> {
        let since = now_ms.saturating_sub(window_ms);
        self.outages
            .iter()
            .filter(|o| o.start_ms >= since)
            .collect()
    }

    /// Drops per local hour over the log, for the chart.
    pub fn by_hour(&self) -> [u32; 24] {
        let mut hours = [0; 24];
        for o in &self.outages {
            hours[o.local_hour() as usize] += 1;
        }
        hours
    }

    /// The daily insight card, at most once a day, when the last day had drops.
    pub fn insight(&mut self, now_ms: u64) -> Option<UpdateCard> {
        if now_ms.saturating_sub(self.last_insight_ms) < DAY_MS {
            return None;
        }
        let card = insight_card(&self.recent(now_ms, DAY_MS), now_ms)?;
        self.last_insight_ms = now_ms;
        Some(card)
    }

    /// CSV for the ISP: one row per outage, then slow minutes and speed tests.
    pub fn csv(&self, now_ms: u64) -> String {
        let mut out = String::from("kind,start_utc,end_utc,minutes,latency_ms,mbps\n");
        for o in &self.outages {
            out.push_str(&format!(
                "outage,{},{},{},,\n",
                iso_utc(o.start_ms),
                o.end_ms.map(iso_utc).unwrap_or_default(),
                o.minutes(now_ms)
            ));
        }
        for s in &self.slow {
            out.push_str(&format!("slow,{},,,{},\n", iso_utc(s.at_ms), s.latency_ms));
        }
        for s in &self.speed {
            out.push_str(&format!("speed,{},,,,{}\n", iso_utc(s.at_ms), s.mbps));
        }
        out
    }
}

fn hour_text(hour: u32) -> String {
    match hour {
        0 => "midnight".into(),
        12 => "noon".into(),
        h if h < 12 => format!("{h} AM"),
        h => format!("{} PM", h - 12),
    }
}

/// "Your internet dropped 3 times last night around 2 AM".
pub fn insight_card(outages: &[&Outage], now_ms: u64) -> Option<UpdateCard> {
    if outages.is_empty() {
        return None;
    }
    let mut hours = [0u32; 24];
    for o in outages {
        hours[o.local_hour() as usize] += 1;
    }
    let peak = (0..24u32)
        .max_by_key(|h| (hours[*h as usize], 24 - h))
        .unwrap_or(0);
    let night = outages.iter().all(|o| o.local_hour() < 6);
    let when = if night { " last night" } else { "" };
    let times = if outages.len() == 1 {
        "once".to_string()
    } else {
        format!("{} times", outages.len())
    };
    let title = format!(
        "Your internet dropped {times}{when} around {}",
        hour_text(peak)
    );
    let longest = outages.iter().map(|o| o.minutes(now_ms)).max().unwrap_or(0);
    let total: u64 = outages.iter().map(|o| o.minutes(now_ms)).sum();
    let body = format!(
        "Down {total} min in all; the longest drop lasted {longest} min. Export the log as CSV from Settings → Behavior to send your ISP."
    );
    Some(digest_card("source:net-log", &title, &body, now_ms))
}

/// `2026-10-27T02:04:00Z`.
pub fn iso_utc(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub fn load(config_dir: &Path) -> NetLog {
    std::fs::read_to_string(config_dir.join(FILE_NAME))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, log: &NetLog) -> Result<(), String> {
    let text = serde_json::to_string_pretty(log).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&config_dir.join(FILE_NAME), &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-27 00:00 UTC.
    const DAY0: u64 = 1_793_059_200_000;
    const MIN: u64 = 60_000;

    #[test]
    fn iso_times_are_utc() {
        assert_eq!(iso_utc(DAY0), "2026-10-27T00:00:00Z");
        assert_eq!(
            iso_utc(DAY0 + 2 * HOUR_MS + 4 * MIN),
            "2026-10-27T02:04:00Z"
        );
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn an_outage_that_ended_while_the_app_was_closed_stops_at_its_last_failed_probe() {
        let mut log = NetLog::default();
        let t0 = DAY0 + 2 * HOUR_MS;
        assert_eq!(log.note(None, t0, 0), None);
        assert_eq!(log.note(None, t0 + MIN, 0), Some(Change::Dropped));
        assert_eq!(log.note(None, t0 + 2 * MIN, 0), None);
        assert_eq!(log.last_probe_ms, t0 + 2 * MIN);
        // A stalled probe thread is not a closed app: a short gap ends it now.
        let mut short = log.clone();
        assert_eq!(
            short.note(Some(20), t0 + 2 * MIN + CLOSED_GAP_MS, 0),
            Some(Change::Restored(17))
        );
        assert_eq!(short.outages[0].end_ms, Some(t0 + 2 * MIN + CLOSED_GAP_MS));
        // Closed for ten hours; the first probe after launch answers.
        let back = t0 + 10 * HOUR_MS;
        assert_eq!(log.note(Some(20), back, 0), Some(Change::Restored(3)));
        assert_eq!(log.outages[0].start_ms, t0);
        assert_eq!(log.outages[0].end_ms, Some(t0 + 3 * MIN));
        assert_eq!(log.outages[0].minutes(back), 3);
        assert_eq!(log.last_probe_ms, back);
        // The stamp is kept on disk; a file from before it loads as unknown.
        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("\"lastProbeMs\":"), "{json}");
        let old: NetLog = serde_json::from_str(r#"{"on":true,"outages":[]}"#).unwrap();
        assert_eq!(old.last_probe_ms, 0);
    }

    #[test]
    fn outages_come_from_two_failed_probes_in_a_row() {
        let mut log = NetLog::default();
        let mut at = DAY0 + 2 * HOUR_MS;
        assert_eq!(log.note(Some(20), at, 0), None);
        at += MIN;
        assert_eq!(log.note(None, at, 0), None, "one miss is not a drop");
        at += MIN;
        assert_eq!(log.note(Some(25), at, 0), None);
        at += MIN;
        assert_eq!(log.note(None, at, 0), None);
        at += MIN;
        assert_eq!(log.note(None, at, 0), Some(Change::Dropped));
        assert!(log.down());
        for _ in 0..10 {
            at += MIN;
            assert_eq!(log.note(None, at, 0), None, "still the same outage");
        }
        at += MIN;
        assert_eq!(log.note(Some(450), at, 0), Some(Change::Restored(12)));
        assert!(!log.down());
        assert_eq!(log.outages.len(), 1);
        assert_eq!(log.outages[0].start_ms, DAY0 + 2 * HOUR_MS + 3 * MIN);
        assert_eq!(
            log.slow,
            vec![Slow {
                at_ms: at,
                latency_ms: 450
            }]
        );
    }

    #[test]
    fn a_slow_minute_is_strictly_slower_than_the_threshold() {
        let mut log = NetLog::default();
        log.note(Some(SLOW_MS), DAY0, 0);
        assert!(log.slow.is_empty(), "exactly {SLOW_MS} ms is not slow");
        log.note(Some(SLOW_MS + 1), DAY0 + MIN, 0);
        assert_eq!(
            log.slow,
            vec![Slow {
                at_ms: DAY0 + MIN,
                latency_ms: SLOW_MS + 1
            }]
        );
    }

    #[test]
    fn the_insight_names_the_count_and_the_hour_once_a_day() {
        let mut log = NetLog::default();
        // Three drops around 2 AM local, an hour ahead of UTC.
        for (start_min, len) in [(60 + 5, 3), (60 + 20, 12), (60 + 40, 2)] {
            log.outages.push(Outage {
                start_ms: DAY0 + start_min * MIN,
                end_ms: Some(DAY0 + (start_min + len) * MIN),
                offset_min: 60,
            });
        }
        let now = DAY0 + 8 * HOUR_MS;
        let card = log.insight(now).unwrap();
        assert_eq!(
            card.title,
            "Your internet dropped 3 times last night around 2 AM"
        );
        assert_eq!(
            card.body.as_deref(),
            Some("Down 17 min in all; the longest drop lasted 12 min. Export the log as CSV from Settings → Behavior to send your ISP.")
        );
        assert_eq!(log.insight(now + HOUR_MS), None, "once a day");
        assert_eq!(log.by_hour()[2], 3);
        assert_eq!(NetLog::default().insight(now), None, "no drops, no card");
    }

    #[test]
    fn the_export_is_csv_with_utc_times() {
        let mut log = NetLog::default();
        log.outages.push(Outage {
            start_ms: DAY0 + 2 * HOUR_MS,
            end_ms: Some(DAY0 + 2 * HOUR_MS + 5 * MIN),
            offset_min: 0,
        });
        log.slow.push(Slow {
            at_ms: DAY0 + 3 * HOUR_MS,
            latency_ms: 512,
        });
        assert_eq!(
            log.note_speed(SPEED_BYTES, 2_000, DAY0 + 4 * HOUR_MS),
            Some(40.0)
        );
        assert_eq!(
            log.csv(DAY0 + 5 * HOUR_MS),
            "kind,start_utc,end_utc,minutes,latency_ms,mbps\n\
             outage,2026-10-27T02:00:00Z,2026-10-27T02:05:00Z,5,,\n\
             slow,2026-10-27T03:00:00Z,,,512,\n\
             speed,2026-10-27T04:00:00Z,,,,40\n"
        );
    }

    #[test]
    fn old_entries_age_out_but_an_open_outage_stays() {
        let mut log = NetLog::default();
        log.outages.push(Outage {
            start_ms: DAY0,
            end_ms: Some(DAY0 + MIN),
            offset_min: 0,
        });
        log.outages.push(Outage {
            start_ms: DAY0 + MIN * 2,
            end_ms: None,
            offset_min: 0,
        });
        log.note(None, DAY0 + KEEP_MS + DAY_MS, 0);
        assert_eq!(log.outages.len(), 1);
        assert!(log.down());
    }
}
