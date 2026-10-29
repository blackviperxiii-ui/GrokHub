//! Command Centre: opt-in PC health while gaming. Samples sensors lightly
//! when card 16.1 / local_governor game detection says a game is up, then
//! posts one Pulse summary. Never changes fans, clocks, or drivers.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

#[cfg(windows)]
use grokhub_core::pc_health::game_label_from_exe_paths;
use grokhub_core::pc_health::{
    self as ph, observe, summary_card, GameSession, ObserveOut, PcHealthLedger, SensorSample,
    SAMPLE_EVERY_MS,
};
#[cfg(target_os = "linux")]
use grokhub_core::pc_health::{
    cpu_util_between, parse_amdgpu_busy, parse_cpuinfo_mhz, parse_hwmon_temp_milli,
    parse_proc_stat_cpu, temps_from_hwmon,
};
#[cfg(any(target_os = "linux", windows))]
use grokhub_core::pc_health::{game_running, nvidia_max_util, parse_nvidia_sensor_csv};

use super::*;
use crate::config;

/// A sensor or process-list command that hasn't answered by now is killed,
/// and that sample counts as no reading.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

fn ledger_path() -> PathBuf {
    config::config_dir().join(ph::LEDGER_FILE)
}

fn load_ledger() -> PcHealthLedger {
    std::fs::read_to_string(ledger_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_ledger(ledger: &PcHealthLedger) {
    if let Ok(text) = serde_json::to_string_pretty(ledger) {
        let path = ledger_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

#[derive(Default)]
pub(super) struct PcHealthState {
    pub session: Option<GameSession>,
    last_sample_ms: u64,
    /// The sample being read off the UI thread, if one is in flight.
    probe: Option<mpsc::Receiver<Probe>>,
    /// The last `/proc/stat` CPU reading, for the next sample's CPU use.
    cpu_prev: Option<(u64, u64)>,
}

/// One off-thread reading: whether a game runs, its sensors, and the CPU
/// counters to diff next time.
struct Probe {
    game_up: bool,
    sample: Option<SensorSample>,
    cpu: Option<(u64, u64)>,
}

impl Cabin {
    pub(super) fn set_pc_health(&mut self, on: bool) {
        if self.cfg.pc_health.enabled == on {
            return;
        }
        self.cfg.pc_health.enabled = on;
        if !on {
            self.pc_health = PcHealthState::default();
        }
        self.persist_cfg();
        self.status = "Saved".into();
    }

    pub(super) fn ui_pc_health_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.pc_health.enabled;
        if crate::cards::settings_toggle(
            ui,
            "PC health while gaming",
            "While a game is running, sample CPU/GPU temps and util lightly. After you quit, Pulse shows max temps, throttle flags, a GPU vs CPU bound guess, and tips. Never changes fans, clocks, or drivers. Off by default.",
            &mut on,
        ) {
            self.set_pc_health(on);
        }
    }

    /// Heartbeat: when enabled, start a reading off the UI thread when one is
    /// due and fold in the last one that finished; when the game ends, post
    /// one summary card. Zero sensor I/O when disabled.
    pub(super) fn tick_pc_health(&mut self) {
        if !self.cfg.pc_health.enabled {
            // Drop any leftover session without posting when the user turns it off mid-game.
            self.pc_health.session = None;
            self.pc_health.probe = None;
            return;
        }
        if let Some(rx) = &self.pc_health.probe {
            match rx.try_recv() {
                Ok(probe) => {
                    self.pc_health.probe = None;
                    self.pc_health.cpu_prev = probe.cpu.or(self.pc_health.cpu_prev);
                    self.observe_pc_health(probe.game_up, probe.sample, now_ms());
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.pc_health.probe = None,
            }
            return;
        }
        let now = now_ms();
        if now.saturating_sub(self.pc_health.last_sample_ms) < SAMPLE_EVERY_MS
            && self.pc_health.session.is_some()
        {
            // Still in a session but not due for a sample — leave state alone.
            return;
        }
        self.pc_health.last_sample_ms = now;
        let cpu_prev = self.pc_health.cpu_prev;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(collect_sample(now, cpu_prev));
        });
        self.pc_health.probe = Some(rx);
    }

    fn observe_pc_health(&mut self, game_up: bool, sample: Option<SensorSample>, now: u64) {
        match observe(true, game_up, sample, &mut self.pc_health.session, now) {
            ObserveOut::Idle | ObserveOut::Sampling => {}
            ObserveOut::Ended(summary) => {
                let mut ledger = load_ledger();
                // At most one summary per 30 minutes for the same game label.
                let key = summary
                    .game_label
                    .as_deref()
                    .unwrap_or("game")
                    .to_ascii_lowercase();
                let cool = now.saturating_sub(ledger.last_summary_ms) < 30 * 60 * 1000
                    && ledger.last_game == key;
                if !cool {
                    ledger.last_summary_ms = now;
                    ledger.last_game = key;
                    save_ledger(&ledger);
                    self.post_feed_card(summary_card(&summary, now));
                }
            }
        }
    }
}

/// Run a sensor or process-list command with `PROBE_TIMEOUT`.
#[cfg(any(target_os = "linux", windows))]
fn probe_cmd(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new(cmd);
    c.args(args);
    grokhub_core::proc_util::output_within(&mut c, PROBE_TIMEOUT, 1024 * 1024)
}

/// `nvidia-smi`: temp, util and clock for the hottest card, and the busiest
/// card's util for the GPU-busy game check.
#[cfg(any(target_os = "linux", windows))]
fn nvidia_sensors() -> Option<((f32, u8, u32), Option<u8>)> {
    let text = probe_cmd(
        "nvidia-smi",
        &[
            "--query-gpu=temperature.gpu,utilization.gpu,clocks.current.graphics",
            "--format=csv,noheader,nounits",
        ],
    )?;
    Some((parse_nvidia_sensor_csv(&text)?, nvidia_max_util(&text)))
}

/// Process list + light sensors, read on a worker thread.
fn collect_sample(now: u64, cpu_prev: Option<(u64, u64)>) -> Probe {
    #[cfg(target_os = "linux")]
    {
        collect_linux(now, cpu_prev)
    }
    #[cfg(windows)]
    {
        let _ = cpu_prev;
        collect_windows(now)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (now, cpu_prev);
        Probe {
            game_up: false,
            sample: None,
            cpu: None,
        }
    }
}

#[cfg(target_os = "linux")]
fn collect_linux(now: u64, cpu_prev: Option<(u64, u64)>) -> Probe {
    let procs = super::local_signals::processes().unwrap_or_default();
    let mut sample = SensorSample {
        at_ms: now,
        game_label: game_label_from_procs(&procs),
        ..Default::default()
    };
    let mut busiest = None;
    if let Some(((t, u, c), max_u)) = nvidia_sensors() {
        sample.gpu_temp_c = Some(t);
        sample.gpu_util_pct = Some(u);
        sample.gpu_clock_mhz = Some(c);
        busiest = max_u;
    }
    if sample.gpu_util_pct.is_none() {
        if let Some(busy) = read_first_amdgpu_busy() {
            sample.gpu_util_pct = Some(busy);
        }
    }
    let hwmon = read_hwmon_temps();
    let (cpu_t, gpu_t) = temps_from_hwmon(&hwmon);
    if sample.cpu_temp_c.is_none() {
        sample.cpu_temp_c = cpu_t;
    }
    if sample.gpu_temp_c.is_none() {
        sample.gpu_temp_c = gpu_t;
    }
    if let Ok(text) = std::fs::read_to_string("/proc/cpuinfo") {
        sample.cpu_clock_mhz = parse_cpuinfo_mhz(&text);
    }
    let cpu = std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|t| parse_proc_stat_cpu(&t));
    sample.cpu_util_pct = cpu_prev.zip(cpu).and_then(|(a, b)| cpu_util_between(a, b));
    let game_up = game_running(&procs, busiest.or(sample.gpu_util_pct));
    Probe {
        game_up,
        sample: game_up.then_some(sample),
        cpu,
    }
}

#[cfg(windows)]
fn collect_windows(now: u64) -> Probe {
    // Full exe paths, like the wind-down check: a game is an exe under a
    // store's library (Steam, Epic, GOG, Xbox). nvidia-smi when present.
    let paths = windows_process_paths();
    let mut sample = SensorSample {
        at_ms: now,
        game_label: game_label_from_exe_paths(&paths),
        ..Default::default()
    };
    let mut busiest = None;
    if let Some(((t, u, c), max_u)) = nvidia_sensors() {
        sample.gpu_temp_c = Some(t);
        sample.gpu_util_pct = Some(u);
        sample.gpu_clock_mhz = Some(c);
        busiest = max_u;
    }
    let game_up = grokhub_core::local_governor::game_in_process_list(&paths) == Some(true)
        || game_running(&[], busiest.or(sample.gpu_util_pct));
    Probe {
        game_up,
        sample: game_up.then_some(sample),
        cpu: None,
    }
}

#[cfg(target_os = "linux")]
fn game_label_from_procs(procs: &[(String, String)]) -> Option<String> {
    for (comm, cmdline) in procs {
        let c = comm.to_ascii_lowercase();
        if c.ends_with("-win64-shipping") {
            let name = c.trim_end_matches("-win64-shipping");
            if name.len() >= 3 {
                return Some(name.to_string());
            }
        }
        if c == "gamescope" || cmdline.contains("SteamLaunch") || cmdline.contains("/proton ") {
            return Some("a game".into());
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn read_hwmon_temps() -> Vec<(String, i64)> {
    let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ent in entries.flatten().take(24) {
        let name = std::fs::read_to_string(ent.path().join("name"))
            .unwrap_or_default()
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        for i in 1..=5 {
            let path = ent.path().join(format!("temp{i}_input"));
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Some(c) = parse_hwmon_temp_milli(text.trim()) {
                    out.push((name.clone(), (c * 1000.0) as i64));
                }
            }
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn read_first_amdgpu_busy() -> Option<u8> {
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    for ent in entries.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let path = ent.path().join("device/gpu_busy_percent");
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Some(u) = parse_amdgpu_busy(&text) {
                return Some(u);
            }
        }
    }
    None
}

/// This session's processes, one line each: the full exe path, or `?name`
/// when the path can't be read (see `game_in_process_list`).
#[cfg(windows)]
fn windows_process_paths() -> Vec<String> {
    let Some(text) = probe_cmd(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$s = (Get-Process -Id $PID).SessionId; Get-Process | Where-Object SessionId -eq $s | ForEach-Object { if ($_.Path) { $_.Path } else { '?' + $_.ProcessName } }",
        ],
    ) else {
        return Vec::new();
    };
    text.lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .take(1000)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_tick_does_no_session_work() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.pc_health.enabled = false;
        cabin.pc_health.session = Some(GameSession {
            started_ms: 1,
            ..Default::default()
        });
        cabin.tick_pc_health();
        assert!(cabin.pc_health.session.is_none());
    }

    #[test]
    fn a_finished_reading_is_folded_in_on_the_next_tick() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.pc_health.enabled = true;
        let (tx, rx) = mpsc::channel();
        cabin.pc_health.probe = Some(rx);
        // Still reading: the tick leaves the session alone.
        cabin.tick_pc_health();
        assert!(cabin.pc_health.session.is_none());
        assert!(cabin.pc_health.probe.is_some());
        let sample = SensorSample {
            at_ms: 5,
            gpu_temp_c: Some(71.0),
            cpu_util_pct: Some(40),
            game_label: Some("hades".into()),
            ..Default::default()
        };
        tx.send(Probe {
            game_up: true,
            sample: Some(sample.clone()),
            cpu: Some((1_800, 1_200)),
        })
        .unwrap();
        cabin.pc_health.last_sample_ms = now_ms();
        cabin.tick_pc_health();
        assert!(cabin.pc_health.probe.is_none());
        assert_eq!(cabin.pc_health.cpu_prev, Some((1_800, 1_200)));
        let session = cabin.pc_health.session.as_ref().expect("session");
        assert_eq!(session.samples, vec![sample]);
        assert_eq!(session.game_label.as_deref(), Some("hades"));
        // Not due yet, so no new reading starts.
        cabin.tick_pc_health();
        assert!(cabin.pc_health.probe.is_none());
    }

    #[test]
    fn the_heartbeat_never_runs_a_probe_itself() {
        let src = include_str!("pc_health_ui.rs");
        let tick = src
            .split("pub(super) fn tick_pc_health(")
            .nth(1)
            .and_then(|s| s.split("\n    fn observe_pc_health(").next())
            .expect("tick_pc_health");
        let (before, after) = tick
            .split_once("std::thread::spawn(")
            .expect("worker thread");
        assert!(!before.contains("collect_sample("), "{before}");
        assert!(after.contains("collect_sample(now, cpu_prev)"), "{after}");
        assert!(
            !tick.contains(concat!(".out", "put()"))
                && !tick.contains(concat!("setup_wizard", "::run"))
        );
    }

    #[test]
    fn settings_toggle_persists_enabled() {
        let mut cabin = Cabin::quiet_for_test();
        assert!(!cabin.cfg.pc_health.enabled);
        cabin.set_pc_health(true);
        assert!(cabin.cfg.pc_health.enabled);
        cabin.set_pc_health(false);
        assert!(!cabin.cfg.pc_health.enabled);
    }
}
