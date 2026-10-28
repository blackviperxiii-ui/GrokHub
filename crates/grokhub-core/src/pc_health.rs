//! Command Centre card: opt-in PC health while a game is running. Pure
//! parsers, session math, and the post-session Pulse summary. The cabin
//! samples sensors lightly and posts the card; this module never touches
//! fans, clocks, or drivers. Game-running detection reuses card 16.1 /
//! [`crate::local_governor`] (`game_in_processes`, GPU-busy fallback).

use serde::{Deserialize, Serialize};

use crate::pulse::PulseMeta;
use crate::update_feed::{suggestion_card, UpdateCard};

/// How often the cabin should sample while a game is up (low rate).
pub const SAMPLE_EVERY_MS: u64 = 15_000;
/// Ignore sessions shorter than this (seconds of wall time with samples).
pub const MIN_SESSION_MS: u64 = 60_000;
/// Sustained GPU util above this with no process-name hit counts as a game
/// (same idea as local_governor `GPU_BUSY_PCT`).
pub const GPU_BUSY_GAME_PCT: u8 = 60;
/// GPU millidegrees / °C that flag thermal pressure.
pub const GPU_HOT_C: f32 = 83.0;
pub const GPU_CRITICAL_C: f32 = 90.0;
/// CPU millidegrees / °C that flag thermal pressure.
pub const CPU_HOT_C: f32 = 90.0;
pub const CPU_CRITICAL_C: f32 = 97.0;
/// Clock drop from session peak that, with a hot temp, counts as throttle.
pub const CLOCK_DROP_PCT: f32 = 12.0;

pub const SOURCE_PREFIX: &str = "pchealth:";
pub const LEDGER_FILE: &str = "pc-health.json";

/// Settings → Behavior / Pulse. Off by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PcHealthSettings {
    #[serde(default)]
    pub enabled: bool,
}

impl PcHealthSettings {
    pub fn is_default(&self) -> bool {
        !self.enabled
    }
}

/// One light sample: temps, clocks, util. Missing sensors stay `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SensorSample {
    pub at_ms: u64,
    pub cpu_temp_c: Option<f32>,
    pub gpu_temp_c: Option<f32>,
    pub cpu_util_pct: Option<u8>,
    pub gpu_util_pct: Option<u8>,
    pub cpu_clock_mhz: Option<u32>,
    pub gpu_clock_mhz: Option<u32>,
    /// Optional game / process label for the summary title.
    pub game_label: Option<String>,
}

/// A throttling (or near-throttle) moment on the trace.
#[derive(Debug, Clone, PartialEq)]
pub struct ThrottleEvent {
    pub at_ms: u64,
    pub kind: ThrottleKind,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleKind {
    GpuThermal,
    CpuThermal,
    GpuClockDrop,
    CpuClockDrop,
}

impl ThrottleKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::GpuThermal => "GPU thermal",
            Self::CpuThermal => "CPU thermal",
            Self::GpuClockDrop => "GPU clock drop",
            Self::CpuClockDrop => "CPU clock drop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundGuess {
    Gpu,
    Cpu,
    Balanced,
    Unknown,
}

impl BoundGuess {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gpu => "GPU-bound",
            Self::Cpu => "CPU-bound",
            Self::Balanced => "balanced",
            Self::Unknown => "unclear",
        }
    }
}

/// Running session while a game is detected.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameSession {
    pub started_ms: u64,
    pub samples: Vec<SensorSample>,
    pub game_label: Option<String>,
    pub peak_gpu_clock: Option<u32>,
    pub peak_cpu_clock: Option<u32>,
}

/// Post-session numbers for the summary card.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    pub game_label: Option<String>,
    pub duration_ms: u64,
    pub sample_count: usize,
    pub max_cpu_temp_c: Option<f32>,
    pub max_gpu_temp_c: Option<f32>,
    pub avg_cpu_util: Option<f32>,
    pub avg_gpu_util: Option<f32>,
    pub throttles: Vec<ThrottleEvent>,
    pub bound: BoundGuess,
    pub tips: Vec<String>,
}

/// Disk ledger so we do not spam summaries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PcHealthLedger {
    #[serde(default)]
    pub last_summary_ms: u64,
    #[serde(default)]
    pub last_game: String,
}

/// Outcome of one observe tick.
#[derive(Debug, Clone, PartialEq)]
pub enum ObserveOut {
    /// Disabled or nothing to do (no I/O expected from the cabin).
    Idle,
    /// Still in a game; keep the session (cabin may sample).
    Sampling,
    /// Game ended; post this summary once.
    Ended(SessionSummary),
}

/// `nvidia-smi --query-gpu=temperature.gpu,utilization.gpu,clocks.current.graphics
/// --format=csv,noheader,nounits` → (temp_c, util_pct, clock_mhz) for the
/// hottest / busiest card.
pub fn parse_nvidia_sensor_csv(text: &str) -> Option<(f32, u8, u32)> {
    let mut best: Option<(f32, u8, u32)> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split(',').map(|c| c.trim());
        let temp: f32 = cols.next()?.parse().ok()?;
        let util: u8 = cols.next()?.parse::<f32>().ok()?.clamp(0.0, 100.0) as u8;
        let clock: u32 = cols.next()?.parse::<f32>().ok()?.max(0.0) as u32;
        best = Some(match best {
            None => (temp, util, clock),
            Some((t, u, _)) if temp > t || util > u => (temp, util, clock),
            Some(prev) => prev,
        });
    }
    best
}

/// AMDGPU sysfs `gpu_busy_percent` file contents.
pub fn parse_amdgpu_busy(text: &str) -> Option<u8> {
    text.trim()
        .parse::<f32>()
        .ok()
        .map(|v| v.clamp(0.0, 100.0) as u8)
}

/// AMDGPU / hwmon `temp*_input` millidegrees → °C.
pub fn parse_hwmon_temp_milli(text: &str) -> Option<f32> {
    let milli: i64 = text.trim().parse().ok()?;
    if !(1..150_000).contains(&milli) {
        return None;
    }
    Some(milli as f32 / 1000.0)
}

/// `hwmon` name + millidegree rows → CPU and GPU temps when the name hints
/// at the sensor (k10temp/coretemp/zenpower → CPU; amdgpu/nvidia → GPU).
pub fn temps_from_hwmon(rows: &[(String, i64)]) -> (Option<f32>, Option<f32>) {
    let mut cpu: Option<f32> = None;
    let mut gpu: Option<f32> = None;
    for (name, milli) in rows {
        let Some(c) = parse_hwmon_temp_milli(&milli.to_string()) else {
            continue;
        };
        let n = name.to_ascii_lowercase();
        let is_gpu = n.contains("amdgpu") || n.contains("nouveau") || n.contains("nvidia");
        let is_cpu = n.contains("k10temp")
            || n.contains("coretemp")
            || n.contains("zenpower")
            || n.contains("cpu")
            || n.contains("acpitz")
            || n.contains("x86_pkg");
        if is_gpu {
            gpu = Some(gpu.map_or(c, |g| g.max(c)));
        } else if is_cpu {
            cpu = Some(cpu.map_or(c, |t| t.max(c)));
        }
    }
    (cpu, gpu)
}

/// `/proc/cpuinfo` `cpu MHz` lines → average MHz.
pub fn parse_cpuinfo_mhz(text: &str) -> Option<u32> {
    let mut sum = 0.0_f64;
    let mut n = 0u32;
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line
            .strip_prefix("cpu MHz")
            .or_else(|| line.strip_prefix("cpu MHz\t"))
        else {
            continue;
        };
        let val = rest.trim().trim_start_matches(':').trim();
        if let Ok(mhz) = val.parse::<f64>() {
            sum += mhz;
            n += 1;
        }
    }
    (n > 0).then(|| (sum / f64::from(n)).round() as u32)
}

/// WMI-style `Name,Temperature,LoadPercentage` CSV (one GPU or CPU row).
pub fn parse_wmi_temp_util(text: &str) -> Option<(f32, u8)> {
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        if cols.len() < 3 {
            continue;
        }
        let temp: f32 = cols[1].parse().ok()?;
        let util: u8 = cols[2].parse::<f32>().ok()?.clamp(0.0, 100.0) as u8;
        if (1.0..150.0).contains(&temp) {
            return Some((temp, util));
        }
    }
    None
}

/// Game running: process hit (card 16.1 / local_governor) or sustained GPU busy.
pub fn game_running(procs: &[(String, String)], gpu_util: Option<u8>) -> bool {
    if crate::local_governor::game_in_processes(procs) {
        return true;
    }
    gpu_util.is_some_and(|u| u >= GPU_BUSY_GAME_PCT)
}

/// Start or extend a session; call only when enabled.
pub fn note_sample(session: &mut Option<GameSession>, sample: SensorSample, now_ms: u64) {
    let label = sample.game_label.clone();
    match session {
        None => {
            let mut s = GameSession {
                started_ms: now_ms,
                game_label: label,
                peak_gpu_clock: sample.gpu_clock_mhz,
                peak_cpu_clock: sample.cpu_clock_mhz,
                samples: vec![sample],
            };
            s.samples.reserve(32);
            *session = Some(s);
        }
        Some(s) => {
            if s.game_label.is_none() {
                s.game_label = label;
            }
            if let Some(c) = sample.gpu_clock_mhz {
                s.peak_gpu_clock = Some(s.peak_gpu_clock.map_or(c, |p| p.max(c)));
            }
            if let Some(c) = sample.cpu_clock_mhz {
                s.peak_cpu_clock = Some(s.peak_cpu_clock.map_or(c, |p| p.max(c)));
            }
            s.samples.push(sample);
        }
    }
}

/// Scan one sample against session peaks for throttle events.
pub fn throttle_on_sample(session: &GameSession, sample: &SensorSample) -> Vec<ThrottleEvent> {
    let mut out = Vec::new();
    if let Some(t) = sample.gpu_temp_c {
        if t >= GPU_CRITICAL_C {
            out.push(ThrottleEvent {
                at_ms: sample.at_ms,
                kind: ThrottleKind::GpuThermal,
                detail: format!("GPU {t:.0}°C"),
            });
        } else if t >= GPU_HOT_C {
            if let (Some(cur), Some(peak)) = (sample.gpu_clock_mhz, session.peak_gpu_clock) {
                if peak > 0
                    && (peak.saturating_sub(cur) as f32) / peak as f32 * 100.0 >= CLOCK_DROP_PCT
                {
                    out.push(ThrottleEvent {
                        at_ms: sample.at_ms,
                        kind: ThrottleKind::GpuClockDrop,
                        detail: format!("GPU {t:.0}°C, clock {cur}→peak {peak} MHz"),
                    });
                }
            }
        }
    }
    if let Some(t) = sample.cpu_temp_c {
        if t >= CPU_CRITICAL_C {
            out.push(ThrottleEvent {
                at_ms: sample.at_ms,
                kind: ThrottleKind::CpuThermal,
                detail: format!("CPU {t:.0}°C"),
            });
        } else if t >= CPU_HOT_C {
            if let (Some(cur), Some(peak)) = (sample.cpu_clock_mhz, session.peak_cpu_clock) {
                if peak > 0
                    && (peak.saturating_sub(cur) as f32) / peak as f32 * 100.0 >= CLOCK_DROP_PCT
                {
                    out.push(ThrottleEvent {
                        at_ms: sample.at_ms,
                        kind: ThrottleKind::CpuClockDrop,
                        detail: format!("CPU {t:.0}°C, clock {cur}→peak {peak} MHz"),
                    });
                }
            }
        }
    }
    out
}

fn avg_util(samples: &[SensorSample], gpu: bool) -> Option<f32> {
    let mut sum = 0.0_f32;
    let mut n = 0u32;
    for s in samples {
        let v = if gpu { s.gpu_util_pct } else { s.cpu_util_pct };
        if let Some(u) = v {
            sum += f32::from(u);
            n += 1;
        }
    }
    (n > 0).then(|| sum / n as f32)
}

fn max_temp(samples: &[SensorSample], gpu: bool) -> Option<f32> {
    samples
        .iter()
        .filter_map(|s| if gpu { s.gpu_temp_c } else { s.cpu_temp_c })
        .fold(None, |acc, t| Some(acc.map_or(t, |a: f32| a.max(t))))
}

/// GPU vs CPU bound from average util (needs both).
pub fn guess_bound(avg_cpu: Option<f32>, avg_gpu: Option<f32>) -> BoundGuess {
    match (avg_cpu, avg_gpu) {
        (Some(c), Some(g)) if g >= 85.0 && c + 15.0 < g => BoundGuess::Gpu,
        (Some(c), Some(g)) if c >= 85.0 && g + 15.0 < c => BoundGuess::Cpu,
        (Some(c), Some(g)) if c >= 50.0 || g >= 50.0 => BoundGuess::Balanced,
        (Some(_), Some(_)) => BoundGuess::Balanced,
        _ => BoundGuess::Unknown,
    }
}

/// Advice only — never applies changes.
pub fn tips_for(summary: &SessionSummary) -> Vec<String> {
    let mut tips = Vec::new();
    let hot_gpu = summary.max_gpu_temp_c.is_some_and(|t| t >= GPU_HOT_C);
    let hot_cpu = summary.max_cpu_temp_c.is_some_and(|t| t >= CPU_HOT_C);
    let throttled = !summary.throttles.is_empty();

    if throttled || hot_gpu || hot_cpu {
        tips.push(
            "Check the fan curve in your OEM/vendor tool — GrokHub never changes fans or clocks."
                .into(),
        );
        tips.push("Blow dust out of heatsinks and make sure intake vents are clear.".into());
    }
    if hot_gpu
        || summary.throttles.iter().any(|e| {
            matches!(
                e.kind,
                ThrottleKind::GpuThermal | ThrottleKind::GpuClockDrop
            )
        })
    {
        tips.push(
            "Update the GPU driver from the vendor, then re-test with the same in-game preset."
                .into(),
        );
        tips.push(
            "In-game: drop shadows/ray tracing one step, or cap FPS near your display refresh."
                .into(),
        );
    }
    if hot_cpu
        || summary.throttles.iter().any(|e| {
            matches!(
                e.kind,
                ThrottleKind::CpuThermal | ThrottleKind::CpuClockDrop
            )
        })
    {
        tips.push(
            "Close heavy background apps and lower CPU-heavy settings (crowd density, sim distance)."
                .into(),
        );
    }
    match summary.bound {
        BoundGuess::Gpu => tips.push(
            "Looks GPU-bound: try DLSS/FSR/XeSS or a lower render scale before cutting CPU settings."
                .into(),
        ),
        BoundGuess::Cpu => tips.push(
            "Looks CPU-bound: lower CPU-side settings first; a higher GPU preset may not help FPS."
                .into(),
        ),
        BoundGuess::Balanced | BoundGuess::Unknown => {}
    }
    if tips.is_empty() {
        tips.push(
            "Temps looked fine this session. For more FPS, try the game-tuning playbook after you finish."
                .into(),
        );
    }
    tips.truncate(5);
    tips
}

/// Build a summary from a finished session (pure).
pub fn summarize(session: &GameSession, ended_ms: u64) -> SessionSummary {
    let mut throttles = Vec::new();
    for s in &session.samples {
        throttles.extend(throttle_on_sample(session, s));
    }
    // Collapse near-duplicate throttle kinds within 30s.
    throttles.sort_by_key(|e| (e.kind as u8, e.at_ms));
    let mut deduped = Vec::new();
    for e in throttles {
        let same = deduped.last().is_some_and(|p: &ThrottleEvent| {
            p.kind == e.kind && e.at_ms.saturating_sub(p.at_ms) < 30_000
        });
        if !same {
            deduped.push(e);
        }
    }
    let avg_cpu = avg_util(&session.samples, false);
    let avg_gpu = avg_util(&session.samples, true);
    let bound = guess_bound(avg_cpu, avg_gpu);
    let mut summary = SessionSummary {
        game_label: session.game_label.clone(),
        duration_ms: ended_ms.saturating_sub(session.started_ms),
        sample_count: session.samples.len(),
        max_cpu_temp_c: max_temp(&session.samples, false),
        max_gpu_temp_c: max_temp(&session.samples, true),
        avg_cpu_util: avg_cpu,
        avg_gpu_util: avg_gpu,
        throttles: deduped,
        bound,
        tips: Vec::new(),
    };
    summary.tips = tips_for(&summary);
    summary
}

fn fmt_temp(t: Option<f32>) -> String {
    match t {
        Some(v) => format!("{v:.0}°C"),
        None => "n/a".into(),
    }
}

fn fmt_util(u: Option<f32>) -> String {
    match u {
        Some(v) => format!("{v:.0}%"),
        None => "n/a".into(),
    }
}

/// Title + short body (feed cards clip body to ~160 chars). Tips go in `why`.
pub fn summary_text(summary: &SessionSummary) -> (String, String) {
    let game = summary
        .game_label
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("your game");
    let mins = (summary.duration_ms / 60_000).max(1);
    let title = if summary.throttles.is_empty() {
        format!("PC health after {game}")
    } else {
        format!("Thermal pressure during {game}")
    };
    let throttle_bit = if summary.throttles.is_empty() {
        "no throttle flags".to_string()
    } else {
        format!("{} throttle flag(s)", summary.throttles.len())
    };
    // Keep under UpdateCard body clip (~160). Full tips live in `why` + prompt.
    let body = format!(
        "{mins} min · CPU {} / GPU {} · util {}/{} · {} · {}. Report only — never changes fans, clocks, or drivers.",
        fmt_temp(summary.max_cpu_temp_c),
        fmt_temp(summary.max_gpu_temp_c),
        fmt_util(summary.avg_cpu_util),
        fmt_util(summary.avg_gpu_util),
        summary.bound.label(),
        throttle_bit,
    );
    (title, body)
}

pub fn is_pc_health_card(card: &UpdateCard) -> bool {
    card.source_id.starts_with(SOURCE_PREFIX)
}

/// Pulse suggestion after a gaming session.
pub fn summary_card(summary: &SessionSummary, now_ms: u64) -> UpdateCard {
    let (title, body) = summary_text(summary);
    let game_key = summary
        .game_label
        .as_deref()
        .unwrap_or("game")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(24)
        .collect::<String>()
        .to_ascii_lowercase();
    let source = format!("{SOURCE_PREFIX}{game_key}:{now_ms}");
    let mut card = suggestion_card(&source, &title, &body, now_ms);
    card.prompt = Some(format!(
        "I just finished a gaming session ({}). Max CPU {} / GPU {}. Bound {}. Suggest concrete FPS and cooling fixes for this PC without changing fans or clocks yourself.",
        summary.game_label.as_deref().unwrap_or("a game"),
        fmt_temp(summary.max_cpu_temp_c),
        fmt_temp(summary.max_gpu_temp_c),
        summary.bound.label(),
    ));
    let mut why = format!("PC health · {} samples", summary.sample_count);
    for e in summary.throttles.iter().take(4) {
        why.push('\n');
        why.push_str(&e.detail);
    }
    if !summary.tips.is_empty() {
        why.push_str("\nTips:");
        for tip in summary.tips.iter().take(4) {
            why.push('\n');
            why.push_str("- ");
            why.push_str(tip);
        }
    }
    why.push_str("\nGrokHub never changes fans, clocks, or drivers.");
    card.why = Some(why);
    card.pulse = PulseMeta {
        category: Some("gaming".into()),
        because: Some("PC health while gaming".into()),
        ..Default::default()
    };
    card
}

/// One cabin tick: when disabled, always Idle and never mutates `session`.
pub fn observe(
    enabled: bool,
    game_up: bool,
    sample: Option<SensorSample>,
    session: &mut Option<GameSession>,
    now_ms: u64,
) -> ObserveOut {
    if !enabled {
        return ObserveOut::Idle;
    }
    if game_up {
        if let Some(s) = sample {
            note_sample(session, s, now_ms);
        } else if session.is_none() {
            // Game seen but no sensors yet — open an empty session so end still fires.
            *session = Some(GameSession {
                started_ms: now_ms,
                ..Default::default()
            });
        }
        return ObserveOut::Sampling;
    }
    // Game not running: finish any open session.
    if let Some(done) = session.take() {
        if done.samples.is_empty() || now_ms.saturating_sub(done.started_ms) < MIN_SESSION_MS {
            return ObserveOut::Idle;
        }
        return ObserveOut::Ended(summarize(&done, now_ms));
    }
    ObserveOut::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(
        at: u64,
        cpu_t: f32,
        gpu_t: f32,
        cpu_u: u8,
        gpu_u: u8,
        cpu_c: u32,
        gpu_c: u32,
    ) -> SensorSample {
        SensorSample {
            at_ms: at,
            cpu_temp_c: Some(cpu_t),
            gpu_temp_c: Some(gpu_t),
            cpu_util_pct: Some(cpu_u),
            gpu_util_pct: Some(gpu_u),
            cpu_clock_mhz: Some(cpu_c),
            gpu_clock_mhz: Some(gpu_c),
            game_label: Some("Hades".into()),
        }
    }

    #[test]
    fn parse_nvidia_sensor_csv_picks_hottest() {
        let text = "72, 40, 1500\n88, 95, 1800\n";
        assert_eq!(parse_nvidia_sensor_csv(text), Some((88.0, 95, 1800)));
    }

    #[test]
    fn parse_hwmon_and_amdgpu_busy() {
        assert_eq!(parse_hwmon_temp_milli("85000"), Some(85.0));
        assert_eq!(parse_hwmon_temp_milli("0"), None);
        assert_eq!(parse_amdgpu_busy("91\n"), Some(91));
        let (cpu, gpu) = temps_from_hwmon(&[("k10temp".into(), 78_000), ("amdgpu".into(), 86_000)]);
        assert_eq!(cpu, Some(78.0));
        assert_eq!(gpu, Some(86.0));
    }

    #[test]
    fn parse_cpuinfo_mhz_averages() {
        let text = "processor\t: 0\ncpu MHz\t\t: 3600.000\nprocessor\t: 1\ncpu MHz\t\t: 3400.000\n";
        assert_eq!(parse_cpuinfo_mhz(text), Some(3500));
    }

    #[test]
    fn game_running_uses_local_governor_and_gpu_fallback() {
        let procs = vec![("gamescope".into(), String::new())];
        assert!(game_running(&procs, None));
        let idle = vec![("firefox".into(), String::new())];
        assert!(!game_running(&idle, Some(10)));
        assert!(game_running(&idle, Some(70)));
    }

    #[test]
    fn disabled_observe_is_idle_and_leaves_session_untouched() {
        let mut session = Some(GameSession {
            started_ms: 1,
            samples: vec![sample(1, 60.0, 60.0, 20, 20, 3000, 1500)],
            ..Default::default()
        });
        let before = session.clone();
        let out = observe(
            false,
            true,
            Some(sample(2, 99.0, 99.0, 99, 99, 1000, 1000)),
            &mut session,
            2,
        );
        assert_eq!(out, ObserveOut::Idle);
        assert_eq!(session, before);
    }

    #[test]
    fn throttle_on_hot_gpu_with_clock_drop() {
        let mut opt = None;
        note_sample(&mut opt, sample(0, 60.0, 70.0, 30, 80, 4500, 2100), 0);
        let session = opt.expect("session");
        let s = sample(10_000, 70.0, 86.0, 40, 98, 4400, 1600);
        let events = throttle_on_sample(&session, &s);
        assert!(
            events.iter().any(|e| e.kind == ThrottleKind::GpuClockDrop),
            "{events:?}"
        );
    }

    #[test]
    fn summarize_flags_critical_temps_and_gpu_bound() {
        let mut session = None;
        note_sample(&mut session, sample(0, 70.0, 75.0, 40, 95, 4200, 2000), 0);
        note_sample(
            &mut session,
            sample(30_000, 72.0, 92.0, 42, 97, 4100, 1990),
            30_000,
        );
        note_sample(
            &mut session,
            sample(90_000, 71.0, 91.0, 41, 96, 4150, 1980),
            90_000,
        );
        let summary = summarize(session.as_ref().unwrap(), 90_000);
        assert!(summary.max_gpu_temp_c.unwrap() >= 91.0);
        assert_eq!(summary.bound, BoundGuess::Gpu);
        assert!(summary
            .throttles
            .iter()
            .any(|e| e.kind == ThrottleKind::GpuThermal));
        assert!(!summary.tips.is_empty());
        let card = summary_card(&summary, 90_000);
        assert!(is_pc_health_card(&card));
        assert!(card.body.as_deref().unwrap().contains("never changes"));
        assert!(card.why.as_deref().unwrap().contains("Tips:"));
        assert!(card.why.as_deref().unwrap().contains("never changes fans"));
    }

    #[test]
    fn observe_ends_session_when_game_stops() {
        let mut session = None;
        assert_eq!(
            observe(
                true,
                true,
                Some(sample(0, 65.0, 70.0, 50, 80, 4000, 1800)),
                &mut session,
                0
            ),
            ObserveOut::Sampling
        );
        // Keep sampling past MIN_SESSION
        let _ = observe(
            true,
            true,
            Some(sample(70_000, 66.0, 71.0, 51, 81, 4000, 1800)),
            &mut session,
            70_000,
        );
        match observe(true, false, None, &mut session, 80_000) {
            ObserveOut::Ended(s) => {
                assert!(s.sample_count >= 2);
                assert_eq!(s.game_label.as_deref(), Some("Hades"));
            }
            other => panic!("expected Ended, got {other:?}"),
        }
        assert!(session.is_none());
    }

    #[test]
    fn short_session_is_dropped() {
        let mut session = None;
        let _ = observe(
            true,
            true,
            Some(sample(0, 65.0, 70.0, 50, 80, 4000, 1800)),
            &mut session,
            0,
        );
        assert_eq!(
            observe(true, false, None, &mut session, 10_000),
            ObserveOut::Idle
        );
    }

    #[test]
    fn guess_bound_cpu_vs_gpu() {
        assert_eq!(guess_bound(Some(92.0), Some(40.0)), BoundGuess::Cpu);
        assert_eq!(guess_bound(Some(40.0), Some(93.0)), BoundGuess::Gpu);
        assert_eq!(guess_bound(Some(70.0), Some(72.0)), BoundGuess::Balanced);
        assert_eq!(guess_bound(None, Some(90.0)), BoundGuess::Unknown);
    }

    #[test]
    fn parse_wmi_temp_util_reads_row() {
        let text = "Name,Temperature,LoadPercentage\nGPU,81,88\n";
        assert_eq!(parse_wmi_temp_util(text), Some((81.0, 88)));
    }
}
