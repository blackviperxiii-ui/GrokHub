//! Internet drop and speed log (`grokhub_core::net_log`), opt-in under
//! Settings → Behavior. About once a minute, off the UI thread, a timed TCP
//! connect to two public resolvers; drops and slow minutes go in the log,
//! the cabin says when it is back online and why it was offline, and a daily
//! Feed card names the pattern. Off, nothing is sent at all.

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use super::*;
use grokhub_core::net_log::{self as nl, Change, NetLog};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct NetState {
    log: Option<NetLog>,
    probed_ms: u64,
    probe_rx: Option<mpsc::Receiver<Option<u32>>>,
    speed_rx: Option<mpsc::Receiver<Result<(u64, u64), String>>>,
    note: String,
    /// The last few connect times, newest last, for Home's ping and jitter.
    /// In memory only.
    recent: std::collections::VecDeque<u32>,
}

/// Samples kept for jitter.
const RECENT_MAX: usize = 20;

/// What Home's Speeds tile shows, from the opt-in internet log.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::app) struct NetTileFacts {
    pub on: bool,
    pub down: bool,
    pub ping_ms: Option<u32>,
    pub jitter_ms: Option<u32>,
    /// Last download test: Mbps and when.
    pub last_speed: Option<(f64, u64)>,
    pub testing: bool,
    pub note: String,
}

/// Mean change between back-to-back connect times.
pub(in crate::app) fn jitter_ms(samples: &[u32]) -> Option<u32> {
    if samples.len() < 2 {
        return None;
    }
    let total: u64 = samples
        .windows(2)
        .map(|w| u64::from(w[0].abs_diff(w[1])))
        .sum();
    Some((total / (samples.len() as u64 - 1)) as u32)
}

/// The connect time to the first resolver that answers, or `None`.
fn probe() -> Option<u32> {
    grokhub_core::offline::refuse().ok()?;
    nl::PROBE_HOSTS.iter().find_map(|host| {
        let addr: SocketAddr = host.parse().ok()?;
        let start = Instant::now();
        TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok()?;
        Some(u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX))
    })
}

/// Bytes read and milliseconds taken for one speed-test download.
fn speed_test() -> Result<(u64, u64), String> {
    grokhub_core::offline::refuse()?;
    let client = grokhub_core::connectivity::default_http_client().map_err(|e| e.to_string())?;
    let start = Instant::now();
    let mut resp = client
        .get(nl::SPEED_URL)
        .timeout(Duration::from_secs(60))
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("the test server answered {}", resp.status()));
    }
    let bytes = std::io::copy(&mut resp, &mut std::io::sink()).map_err(|e| e.to_string())?;
    let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok((bytes, ms))
}

fn log_dir() -> std::path::PathBuf {
    crate::config::config_dir()
}

/// Where the CSV export goes: the user's Downloads folder, under USERPROFILE
/// on Windows (a HOME set there can point somewhere else) and HOME elsewhere.
fn downloads() -> std::path::PathBuf {
    downloads_in(
        cfg!(windows),
        std::env::var_os("HOME"),
        std::env::var_os("USERPROFILE"),
    )
}

/// Kept as `OsString` so a home path that is not UTF-8 still works.
fn downloads_in(
    windows: bool,
    home: Option<std::ffi::OsString>,
    userprofile: Option<std::ffi::OsString>,
) -> std::path::PathBuf {
    let set = |v: Option<std::ffi::OsString>| v.filter(|s| !s.is_empty());
    let (first, second) = if windows {
        (userprofile, home)
    } else {
        (home, userprofile)
    };
    set(first)
        .or_else(|| set(second))
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join("Downloads")
}

impl Cabin {
    fn net_now(&mut self) -> &mut NetLog {
        self.focus
            .net
            .log
            .get_or_insert_with(|| nl::load(&log_dir()))
    }

    fn save_net(&mut self) {
        let log = self.net_now().clone();
        if let Err(err) = nl::save(&log_dir(), &log) {
            self.focus.net.note = err;
        }
    }

    /// Each frame: take a finished probe or speed test, start the next probe.
    pub(super) fn tick_net_log(&mut self) {
        if !self.cfg.net_log {
            return;
        }
        if let Some(rx) = self.focus.net.speed_rx.take() {
            match rx.try_recv() {
                Ok(result) => self.finish_speed_test(result, now_ms()),
                Err(mpsc::TryRecvError::Empty) => self.focus.net.speed_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if let Some(rx) = self.focus.net.probe_rx.take() {
            match rx.try_recv() {
                Ok(latency) => {
                    let clock = Self::local_clock();
                    let offset = grokhub_core::pulse_writer::utc_offset_min(
                        clock.hour,
                        clock.minute,
                        clock.now_ms,
                    );
                    self.note_probe(latency, now_ms(), offset);
                }
                Err(mpsc::TryRecvError::Empty) => self.focus.net.probe_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
            return;
        }
        let now = now_ms();
        if now.saturating_sub(self.focus.net.probed_ms) < nl::PROBE_EVERY_MS {
            return;
        }
        self.focus.net.probed_ms = now;
        let (tx, rx) = mpsc::channel();
        self.focus.net.probe_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(probe());
        });
    }

    /// One probe's result: log it, say when the connection comes back, and
    /// post the daily card when the last day had drops.
    pub(super) fn note_probe(&mut self, latency: Option<u32>, now: u64, offset_min: i32) {
        if let Some(ms) = latency {
            let recent = &mut self.focus.net.recent;
            recent.push_back(ms);
            while recent.len() > RECENT_MAX {
                recent.pop_front();
            }
        }
        let slow_before = self.net_now().slow.len();
        let change = self.net_now().note(latency, now, offset_min);
        match change {
            Some(Change::Dropped) => {
                let line = format!(
                    "Offline since {}: neither public resolver answered.",
                    grokhub_core::pulse_writer::local_hm(
                        self.net_now().outages.last().map_or(now, |o| o.start_ms),
                        offset_min
                    )
                );
                // `status_toasts` raises this on every page, not just in Settings.
                self.status = line.clone();
                self.focus.net.note = line;
            }
            Some(Change::Restored(minutes)) => {
                let since = self.net_now().outages.last().map_or(now, |o| o.start_ms);
                let line = format!(
                    "Back online. The internet dropped at {} for {minutes} min; it's in the drop log.",
                    grokhub_core::pulse_writer::local_hm(since, offset_min)
                );
                self.status = line.clone();
                self.focus.net.note = line;
            }
            None => {}
        }
        let insight = self.net_now().insight(now);
        if change.is_some() || insight.is_some() || self.net_now().slow.len() != slow_before {
            self.save_net();
        }
        if let Some(card) = insight {
            if grokhub_core::post_update(&mut self.updates, card) {
                self.persist_updates();
            }
        }
    }

    fn finish_speed_test(&mut self, result: Result<(u64, u64), String>, now: u64) {
        self.focus.net.note = match result {
            Ok((bytes, ms)) => match self.net_now().note_speed(bytes, ms, now) {
                Some(mbps) => {
                    self.save_net();
                    format!("Download speed: {mbps} Mbps.")
                }
                None => "The speed test finished too fast to measure.".into(),
            },
            Err(why) => format!("Speed test failed: {why}."),
        };
    }

    /// One 10 MB download from the test server, off the UI thread. Only on a tap.
    pub(super) fn start_speed_test(&mut self) {
        if !self.cfg.net_log || self.focus.net.speed_rx.is_some() {
            return;
        }
        self.focus.net.note = "Testing download speed…".into();
        let (tx, rx) = mpsc::channel();
        self.focus.net.speed_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(speed_test());
        });
    }

    /// Home's Speeds tile: nothing new is measured here, it reads the log.
    pub(super) fn net_tile_facts(&mut self) -> NetTileFacts {
        if !self.cfg.net_log {
            return NetTileFacts {
                on: false,
                down: false,
                ping_ms: None,
                jitter_ms: None,
                last_speed: None,
                testing: false,
                note: String::new(),
            };
        }
        let samples: Vec<u32> = self.focus.net.recent.iter().copied().collect();
        let testing = self.focus.net.speed_rx.is_some();
        let note = self.focus.net.note.clone();
        let log = self.net_now();
        NetTileFacts {
            on: true,
            down: log.down(),
            ping_ms: samples.last().copied(),
            jitter_ms: jitter_ms(&samples),
            last_speed: log.speed.last().map(|s| (s.mbps, s.at_ms)),
            testing,
            note,
        }
    }

    fn export_net_csv(&mut self, now: u64) {
        let name = format!("internet-log-{}.csv", &nl::iso_utc(now)[..10]);
        let path = downloads().join(&name);
        let csv = self.net_now().csv(now);
        self.focus.net.note = match grokhub_core::pulse_writer::write_atomic(&path, &csv) {
            Ok(()) => format!("Saved {name} in Downloads."),
            Err(err) => err,
        };
    }

    /// Settings → Behavior: the toggle, the drops-by-hour chart, a speed
    /// test and the CSV export.
    pub(super) fn ui_net_log_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.net_log;
        if crate::cards::settings_toggle(
            ui,
            "Log internet drops",
            "About once a minute, check that the internet answers (a quick connection to two public DNS servers) and log drops and slow minutes on this computer. Export the log for your ISP any time.",
            &mut on,
        ) {
            self.cfg.net_log = on;
            self.focus.net = NetState::default();
            self.persist_cfg();
            self.status = "Saved".into();
        }
        if !self.cfg.net_log {
            return;
        }
        ui.indent("net-log", |ui| {
            let hours = self.net_now().by_hour();
            let drops: u32 = hours.iter().sum();
            if drops == 0 {
                crate::cards::help_text(ui, "No drops logged yet.");
            } else {
                crate::cards::help_text(
                    ui,
                    &format!("{drops} drops in the last 30 days, by hour:"),
                );
                paint_hours(ui, &hours);
            }
            ui.horizontal(|ui| {
                let testing = self.focus.net.speed_rx.is_some();
                if !testing && crate::cards::ghost_pill(ui, "Test speed now") {
                    self.start_speed_test();
                }
                if crate::cards::ghost_pill(ui, "Export CSV") {
                    self.export_net_csv(now_ms());
                }
            });
            if !self.focus.net.note.is_empty() {
                crate::cards::help_text(ui, &self.focus.net.note);
            }
        });
    }
}

/// 24 bars, midnight to 11 PM.
fn paint_hours(ui: &mut egui::Ui, hours: &[u32; 24]) {
    let max = hours.iter().copied().max().unwrap_or(1).max(1) as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0 * 10.0, 40.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let color = ui.visuals().strong_text_color();
    for (h, n) in hours.iter().enumerate() {
        let height = (*n as f32 / max) * (rect.height() - 2.0);
        let x = rect.left() + h as f32 * 10.0;
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x + 1.0, rect.bottom() - height.max(1.0)),
                egui::pos2(x + 8.0, rect.bottom()),
            ),
            1.0,
            if *n == 0 {
                color.gamma_multiply(0.2)
            } else {
                color
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY0: u64 = 1_793_059_200_000;
    const MIN: u64 = 60_000;

    #[test]
    fn the_csv_goes_to_downloads_under_userprofile_on_windows() {
        let os = |s: &str| Some(std::ffi::OsString::from(s));
        assert_eq!(
            downloads_in(true, os("C:/Users/user/AppData/Local"), os("C:/Users/user")),
            std::path::PathBuf::from("C:/Users/user").join("Downloads")
        );
        assert_eq!(
            downloads_in(false, os("/home/user"), os("/elsewhere")),
            std::path::PathBuf::from("/home/user").join("Downloads")
        );
        assert_eq!(
            downloads_in(true, os("C:/Users/user"), os("")),
            std::path::PathBuf::from("C:/Users/user").join("Downloads"),
            "an empty USERPROFILE falls back to HOME"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_home_that_is_not_utf8_still_gets_its_downloads() {
        use std::os::unix::ffi::OsStringExt;
        let home = std::ffi::OsString::from_vec(b"/home/us\xffer".to_vec());
        assert_eq!(
            downloads_in(false, Some(home.clone()), None),
            std::path::PathBuf::from(home).join("Downloads")
        );
    }

    #[test]
    fn off_sends_nothing() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.net_log = false;
        cabin.tick_net_log();
        assert!(
            cabin.focus.net.probe_rx.is_none(),
            "no probe thread when off"
        );
        assert_eq!(cabin.focus.net.probed_ms, 0);
    }

    #[test]
    fn a_drop_says_why_then_back_online_and_the_daily_card_posts() {
        let root = crate::config::test_config_root("net-log");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.net_log = true;
        cabin.net_now().last_insight_ms = DAY0 + 2 * 60 * MIN;
        let mut at = DAY0 + 2 * 60 * MIN;
        cabin.note_probe(Some(20), at, 0);
        at += MIN;
        cabin.note_probe(None, at, 0);
        at += MIN;
        cabin.note_probe(None, at, 0);
        assert_eq!(
            cabin.focus.net.note,
            "Offline since 02:01: neither public resolver answered."
        );
        assert_eq!(
            cabin.status, "Offline since 02:01: neither public resolver answered.",
            "a drop shows on Pulse and Chat too"
        );
        at += 4 * MIN;
        cabin.note_probe(Some(30), at, 0);
        assert_eq!(
            cabin.status,
            "Back online. The internet dropped at 02:01 for 5 min; it's in the drop log."
        );
        assert_eq!(nl::load(&log_dir()).outages.len(), 1, "saved");
        assert!(cabin
            .updates
            .iter()
            .all(|c| c.source_id != "source:net-log"));
        cabin.note_probe(Some(30), DAY0 + 26 * 60 * MIN, 0);
        let card = cabin
            .updates
            .iter()
            .find(|c| c.source_id == "source:net-log")
            .expect("daily card");
        assert_eq!(
            card.title,
            "Your internet dropped once last night around 2 AM"
        );
    }
}
