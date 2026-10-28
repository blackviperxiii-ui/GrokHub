//! Command Centre: opt-in PC health while gaming. Samples sensors lightly
//! when card 16.1 / local_governor game detection says a game is up, then
//! posts one Pulse summary. Never changes fans, clocks, or drivers.

use std::path::PathBuf;

use grokhub_core::pc_health::{
    self as ph, game_running, observe, parse_amdgpu_busy, parse_cpuinfo_mhz, parse_hwmon_temp_milli,
    parse_nvidia_sensor_csv, summary_card, temps_from_hwmon, GameSession, ObserveOut,
    PcHealthLedger, SensorSample, SAMPLE_EVERY_MS,
};

use super::*;
use crate::config;

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

    /// Heartbeat: when enabled, detect a game and sample; when the game ends,
    /// post one summary card. Zero sensor I/O when disabled.
    pub(super) fn tick_pc_health(&mut self) {
        if !self.cfg.pc_health.enabled {
            // Drop any leftover session without posting when the user turns it off mid-game.
            self.pc_health.session = None;
            return;
        }
        let now = now_ms();
        if now.saturating_sub(self.pc_health.last_sample_ms) < SAMPLE_EVERY_MS
            && self.pc_health.session.is_some()
        {
            // Still in a session but not due for a sample — leave state alone.
            return;
        }
        let (game_up, sample) = self.collect_pc_health_sample(now);
        self.pc_health.last_sample_ms = now;
        match observe(
            true,
            game_up,
            sample,
            &mut self.pc_health.session,
            now,
        ) {
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

    /// Process list + light sensors. Returns (game_running, optional sample).
    fn collect_pc_health_sample(&self, now: u64) -> (bool, Option<SensorSample>) {
        #[cfg(target_os = "linux")]
        {
            self.collect_linux(now)
        }
        #[cfg(windows)]
        {
            self.collect_windows(now)
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = now;
            (false, None)
        }
    }

    #[cfg(target_os = "linux")]
    fn collect_linux(&self, now: u64) -> (bool, Option<SensorSample>) {
        let procs = linux_processes().unwrap_or_default();
        let mut sample = SensorSample {
            at_ms: now,
            game_label: game_label_from_procs(&procs),
            ..Default::default()
        };
        if let Some(text) = super::setup_wizard::run(
            "nvidia-smi",
            &[
                "--query-gpu=temperature.gpu,utilization.gpu,clocks.current.graphics",
                "--format=csv,noheader,nounits",
            ],
        ) {
            if let Some((t, u, c)) = parse_nvidia_sensor_csv(&text) {
                sample.gpu_temp_c = Some(t);
                sample.gpu_util_pct = Some(u);
                sample.gpu_clock_mhz = Some(c);
            }
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
        sample.cpu_util_pct = linux_cpu_util_rough();
        let up = game_running(&procs, sample.gpu_util_pct);
        if up {
            (true, Some(sample))
        } else {
            (false, None)
        }
    }

    #[cfg(windows)]
    fn collect_windows(&self, now: u64) -> (bool, Option<SensorSample>) {
        // Prefer nvidia-smi when present (same CSV as Linux). Process list via
        // tasklist is enough for game hosts / shipping binaries.
        let procs = windows_processes();
        let mut sample = SensorSample {
            at_ms: now,
            game_label: game_label_from_procs(&procs),
            ..Default::default()
        };
        if let Some(text) = super::setup_wizard::run(
            "nvidia-smi",
            &[
                "--query-gpu=temperature.gpu,utilization.gpu,clocks.current.graphics",
                "--format=csv,noheader,nounits",
            ],
        ) {
            if let Some((t, u, c)) = parse_nvidia_sensor_csv(&text) {
                sample.gpu_temp_c = Some(t);
                sample.gpu_util_pct = Some(u);
                sample.gpu_clock_mhz = Some(c);
            }
        }
        let up = game_running(&procs, sample.gpu_util_pct);
        if up {
            (true, Some(sample))
        } else {
            (false, None)
        }
    }
}

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
fn linux_processes() -> Option<Vec<(String, String)>> {
    let entries = std::fs::read_dir("/proc").ok()?;
    Some(
        entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            })
            .filter_map(|e| {
                let comm = std::fs::read_to_string(e.path().join("comm"))
                    .ok()?
                    .trim()
                    .to_string();
                let cmdline = std::fs::read_to_string(e.path().join("cmdline"))
                    .unwrap_or_default()
                    .replace('\0', " ");
                Some((comm, cmdline))
            })
            .collect(),
    )
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

/// Rough whole-system CPU util from `/proc/stat` idle delta is heavy; use
/// loadavg × cores as a light stand-in (0–100 clamped).
#[cfg(target_os = "linux")]
fn linux_cpu_util_rough() -> Option<u8> {
    let load = std::fs::read_to_string("/proc/loadavg").ok()?;
    let one: f32 = load.split_whitespace().next()?.parse().ok()?;
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get() as f32)
        .unwrap_or(1.0)
        .max(1.0);
    Some(((one / cpus) * 100.0).clamp(0.0, 100.0) as u8)
}

#[cfg(windows)]
fn windows_processes() -> Vec<(String, String)> {
    let Some(text) = super::setup_wizard::run("tasklist", &["/FO", "CSV", "/NH"]) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let name = line.split(',').next()?.trim().trim_matches('"');
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), String::new()))
        })
        .take(400)
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
    fn settings_toggle_persists_enabled() {
        let mut cabin = Cabin::quiet_for_test();
        assert!(!cabin.cfg.pc_health.enabled);
        cabin.set_pc_health(true);
        assert!(cabin.cfg.pc_health.enabled);
        cabin.set_pc_health(false);
        assert!(!cabin.cfg.pc_health.enabled);
    }
}
