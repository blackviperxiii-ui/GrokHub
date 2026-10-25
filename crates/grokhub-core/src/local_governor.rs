//! Live resource governor for the on-device model. Every sample (2 s, 5 s on
//! battery) the OS readers fill [`Signals`]; [`target`] turns them into a
//! [`Level`] and [`Governor`] applies hysteresis: 5 samples in a row to step
//! down, 30 to step up, and no step up within 2 minutes of the last change.
//! Signals a reader can't see stay `None`; with none at all the governor never
//! moves, so the sidecar keeps today's load-on-demand and idle unload. The
//! parsers for each OS's raw output live here too, so they are tested from
//! fixtures. Everything in this module is pure.

use crate::local_profile::{PowerInfo, Thermal};

/// How hard the sidecar may run. Ordered from least to most load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Level {
    /// Stopped; nothing loads until the level rises again.
    Unload,
    /// Smaller memory budget and context.
    Throttle,
    #[default]
    Normal,
    /// The machine is idle on AC: a bigger budget.
    RampUp,
}

/// Samples in a row before stepping down (to less load).
pub const DOWN_SAMPLES: u32 = 5;
/// Samples in a row before stepping up (to more load).
pub const UP_SAMPLES: u32 = 30;
/// No step up within this long of the last change (each change reloads).
pub const RELOAD_COOLDOWN_MS: u64 = 120_000;
/// Idle this long on AC before RampUp.
pub const IDLE_RAMP_SECS: u64 = 300;
/// Another app keeping the GPU busier than this counts as a game.
pub const GPU_BUSY_PCT: u8 = 60;
/// Unload below this battery percent when not charging.
pub const LOW_BATTERY_PCT: u8 = 20;

/// Free RAM as the OS reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub avail_mb: u64,
    pub total_mb: u64,
}

/// One sample. `None` means the reader couldn't tell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Signals {
    /// A fullscreen app or game owns the screen (OS notification state, a
    /// foreground window covering the monitor, `_NET_WM_STATE_FULLSCREEN`,
    /// or a compositor's fullscreen flag).
    pub fullscreen: Option<bool>,
    /// A game is running: gamemode active, gamescope, a Steam game, Proton or
    /// Wine, or a frontmost app in the games category.
    pub game: Option<bool>,
    /// GPU busy by other processes, GrokHub's own share already subtracted.
    pub external_gpu_pct: Option<u8>,
    /// Seconds since the last keyboard or mouse input.
    pub idle_secs: Option<u64>,
    pub power: Option<PowerInfo>,
    pub thermal: Thermal,
    /// RAM, with GrokHub's sidecar added back to `avail_mb`.
    pub memory: Option<Memory>,
    /// The OS's own memory pressure flag (macOS warn or critical).
    pub os_pressure: Option<bool>,
    /// Executable or app name of the foreground window, lowercase.
    pub foreground: Option<String>,
}

impl Signals {
    /// No reader saw anything.
    pub fn is_empty(&self) -> bool {
        self.fullscreen.is_none()
            && self.game.is_none()
            && self.external_gpu_pct.is_none()
            && self.idle_secs.is_none()
            && self.power.is_none()
            && self.thermal == Thermal::Unknown
            && self.memory.is_none()
            && self.os_pressure.is_none()
    }
}

/// The user's app lists and battery choice (`{config}/local/governor.json`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Rules {
    /// Apps that never count as a game, even fullscreen (a video player).
    pub allow: Vec<String>,
    /// Apps that always count as a game.
    pub deny: Vec<String>,
    pub throttle_on_battery: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            allow: Vec::new(),
            deny: Vec::new(),
            throttle_on_battery: true,
        }
    }
}

impl Rules {
    /// Parse `governor.json`; a broken file keeps the defaults.
    pub fn from_json(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }

    fn lists(list: &[String], app: &str) -> bool {
        list.iter().any(|a| a.trim().eq_ignore_ascii_case(app))
    }
}

/// What the governor wants for one sample, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Want {
    pub level: Level,
    pub reason: &'static str,
}

fn want(level: Level, reason: &'static str) -> Option<Want> {
    Some(Want { level, reason })
}

/// Another app is using the machine hard: a game, a fullscreen app, or a busy GPU.
pub fn busy(s: &Signals, rules: &Rules) -> Option<&'static str> {
    if let Some(app) = s.foreground.as_deref() {
        if Rules::lists(&rules.allow, app) {
            return None;
        }
        if Rules::lists(&rules.deny, app) {
            return Some("an app on your deny list is in front");
        }
    }
    if s.game == Some(true) {
        return Some("a game is running");
    }
    if s.fullscreen == Some(true) {
        return Some("a fullscreen app is in front");
    }
    if s.external_gpu_pct.is_some_and(|p| p > GPU_BUSY_PCT) {
        return Some("another app is keeping the GPU busy");
    }
    None
}

fn on_battery(p: &PowerInfo) -> bool {
    p.battery && !p.on_ac
}

fn pressure(s: &Signals) -> bool {
    s.os_pressure == Some(true)
        || s.memory
            .is_some_and(|m| m.total_mb > 0 && m.avail_mb * 10 < m.total_mb)
}

/// The level these signals call for, or `None` when nothing was read.
pub fn target(s: &Signals, rules: &Rules) -> Option<Want> {
    if s.is_empty() {
        return None;
    }
    if s.thermal == Thermal::Critical {
        return want(Level::Unload, "the machine is too hot");
    }
    if let Some(p) = s.power.as_ref().filter(|p| on_battery(p)) {
        if p.percent.is_some_and(|pct| pct < LOW_BATTERY_PCT) {
            return want(Level::Unload, "the battery is low");
        }
    }
    if let Some(reason) = busy(s, rules) {
        return want(Level::Throttle, reason);
    }
    if pressure(s) {
        return want(Level::Throttle, "memory is running low");
    }
    if s.thermal == Thermal::Serious {
        return want(Level::Throttle, "the machine is running hot");
    }
    if let Some(p) = s.power.as_ref() {
        if p.power_saver {
            return want(Level::Throttle, "power saver is on");
        }
        if rules.throttle_on_battery && on_battery(p) {
            return want(Level::Throttle, "on battery");
        }
    }
    let idle = s.idle_secs.is_some_and(|i| i >= IDLE_RAMP_SECS);
    let on_ac = s.power.as_ref().is_none_or(|p| !on_battery(p));
    if idle && on_ac {
        return want(Level::RampUp, "idle on AC power");
    }
    want(Level::Normal, "normal use")
}

/// Sampling period: 2 s, or 5 s on battery.
pub fn interval_ms(power: Option<&PowerInfo>) -> u64 {
    if power.is_some_and(on_battery) {
        5_000
    } else {
        2_000
    }
}

/// Hysteresis over [`target`]'s samples.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Governor {
    pub level: Level,
    /// The level the current streak points at, and its length.
    streak: Option<(Level, u32)>,
    /// When the level last changed (each change reloads the sidecar).
    changed_ms: Option<u64>,
}

impl Governor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one sample's target; returns the new level when it changes.
    pub fn observe(&mut self, want: Option<Level>, now_ms: u64) -> Option<Level> {
        let Some(t) = want else {
            // An unread sample breaks a streak but never moves the level.
            self.streak = None;
            return None;
        };
        if t == self.level {
            self.streak = None;
            return None;
        }
        let down = t < self.level;
        let count = match self.streak {
            // Same direction: follow the latest target, keep the count.
            Some((prev, n)) if (prev < self.level) == down => n + 1,
            _ => 1,
        };
        self.streak = Some((t, count));
        let need = if down { DOWN_SAMPLES } else { UP_SAMPLES };
        if count < need {
            return None;
        }
        let cooled = self
            .changed_ms
            .is_none_or(|c| now_ms.saturating_sub(c) >= RELOAD_COOLDOWN_MS);
        if !down && !cooled {
            return None;
        }
        self.level = t;
        self.streak = None;
        self.changed_ms = Some(now_ms);
        Some(t)
    }
}

/// Failed starts in this window open the breaker.
pub const CRASH_WINDOW_MS: u64 = 600_000;
pub const CRASH_LIMIT: usize = 3;
/// How long an open breaker refuses starts.
pub const CRASH_HOLD_MS: u64 = 1_800_000;

/// Stops a sidecar that keeps dying from being restarted in a loop.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CrashBreaker {
    failures: Vec<u64>,
    open_until: Option<u64>,
}

impl CrashBreaker {
    pub fn allows(&self, now_ms: u64) -> bool {
        self.open_until.is_none_or(|u| now_ms >= u)
    }

    /// A start failed or a running child died.
    pub fn failed(&mut self, now_ms: u64) {
        self.failures
            .retain(|t| now_ms.saturating_sub(*t) < CRASH_WINDOW_MS);
        self.failures.push(now_ms);
        if self.failures.len() >= CRASH_LIMIT {
            self.open_until = Some(now_ms + CRASH_HOLD_MS);
            self.failures.clear();
        }
    }
}

/// GPU busy by others: the total minus GrokHub's own share.
pub fn external_gpu(total_pct: u8, own_pct: u8) -> u8 {
    total_pct.saturating_sub(own_pct)
}

/// Windows `SHQueryUserNotificationState`: busy (2), Direct3D fullscreen (3)
/// and presentation mode (4) mean something owns the screen.
pub fn quns_fullscreen(state: i32) -> bool {
    matches!(state, 2..=4)
}

/// A foreground window rect equal to its monitor rect (left, top, right, bottom).
pub fn covers_monitor(window: (i32, i32, i32, i32), monitor: (i32, i32, i32, i32)) -> bool {
    window.0 <= monitor.0 && window.1 <= monitor.1 && window.2 >= monitor.2 && window.3 >= monitor.3
}

/// Windows `GetSystemPowerStatus`: AC line (1 online), battery flag (128 none,
/// 255 unknown), percent (255 unknown) and the battery saver flag.
pub fn power_from_status(ac_line: u8, battery_flag: u8, percent: u8, saver: bool) -> PowerInfo {
    let battery = battery_flag != 128 && battery_flag != 255;
    PowerInfo {
        battery,
        on_ac: ac_line != 0,
        percent: (battery && percent <= 100).then_some(percent),
        power_saver: saver,
    }
}

/// PDH `\GPU Engine(*)\Utilization Percentage` rows as (instance, value):
/// each engine summed over processes, with `own_pid` left out, then the
/// busiest engine. Instance names look like
/// `pid_1234_luid_0x0_0xC8D3_phys_0_eng_0_engtype_3D`.
pub fn gpu_from_pdh(rows: &[(String, f64)], own_pid: Option<u32>) -> u8 {
    let mut by_engine: Vec<(String, f64)> = Vec::new();
    for (name, value) in rows {
        let (pid, engine) = match name.strip_prefix("pid_").and_then(|r| r.split_once('_')) {
            Some((p, rest)) => (p.parse::<u32>().ok(), rest.to_string()),
            None => (None, name.clone()),
        };
        if pid.is_some() && pid == own_pid {
            continue;
        }
        match by_engine.iter_mut().find(|(e, _)| *e == engine) {
            Some((_, v)) => *v += value,
            None => by_engine.push((engine, *value)),
        }
    }
    let busiest = by_engine.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max);
    busiest.clamp(0.0, 100.0).round() as u8
}

/// `nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader,nounits`:
/// the busiest GPU.
pub fn parse_nvidia_util(text: &str) -> Option<u8> {
    text.lines()
        .filter_map(|l| l.trim().parse::<u8>().ok())
        .max()
}

/// `nvidia-smi pmon -c 1 -s u`: the `sm` % of `pid` (`-` counts as 0).
pub fn parse_nvidia_pmon(text: &str, pid: u32) -> u8 {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            let p: u32 = cols.get(1)?.parse().ok()?;
            (p == pid).then(|| cols.get(3).and_then(|v| v.parse::<u8>().ok()).unwrap_or(0))
        })
        .max()
        .unwrap_or(0)
}

/// Processes whose name or command line marks a running game.
pub fn game_in_processes(procs: &[(String, String)]) -> bool {
    procs.iter().any(|(comm, cmdline)| {
        let c = comm.to_ascii_lowercase();
        matches!(
            c.as_str(),
            "gamescope" | "gamescope-wl" | "wine64-preloader" | "wine-preloader" | "wineserver"
        ) || cmdline.contains("SteamLaunch")
            || cmdline.contains("/proton ")
    })
}

/// `gamemoded -s`: "gamemode is active".
pub fn gamemode_active(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("is active") && !t.contains("inactive")
}

/// Hyprland `hyprctl activewindow -j`: (fullscreen, class).
pub fn parse_hyprctl_active(json: &str) -> (Option<bool>, Option<String>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return (None, None);
    };
    // 0.42+ sends a mode mask (1 maximized, 2 fullscreen); older builds send
    // a bool plus `fullscreenMode` (0 fullscreen, 1 maximized). Maximized
    // windows don't count.
    let full = match v.get("fullscreen") {
        Some(serde_json::Value::Bool(b)) => {
            Some(*b && v.get("fullscreenMode").and_then(|m| m.as_u64()) != Some(1))
        }
        Some(serde_json::Value::Number(n)) => n.as_u64().map(|n| n & 2 != 0),
        _ => None,
    };
    let class = v
        .get("class")
        .and_then(|c| c.as_str())
        .filter(|c| !c.is_empty())
        .map(str::to_ascii_lowercase);
    (full, class)
}

/// Sway `swaymsg -t get_tree`: (fullscreen, app id) of the focused node.
pub fn parse_sway_tree(json: &str) -> (Option<bool>, Option<String>) {
    fn focused(v: &serde_json::Value) -> Option<&serde_json::Value> {
        if v.get("focused").and_then(|f| f.as_bool()) == Some(true) {
            return Some(v);
        }
        ["nodes", "floating_nodes"]
            .iter()
            .filter_map(|k| v.get(*k).and_then(|n| n.as_array()))
            .flatten()
            .find_map(focused)
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return (None, None);
    };
    let Some(node) = focused(&v) else {
        return (None, None);
    };
    let full = node
        .get("fullscreen_mode")
        .and_then(|m| m.as_u64())
        .map(|m| m > 0);
    let app = node
        .get("app_id")
        .and_then(|a| a.as_str())
        .or_else(|| {
            node.get("window_properties")
                .and_then(|p| p.get("class"))
                .and_then(|c| c.as_str())
        })
        .map(str::to_ascii_lowercase);
    (full, app)
}

/// macOS `LSApplicationCategoryType` of a games app.
pub fn is_game_category(category: &str) -> bool {
    let c = category.trim().to_ascii_lowercase();
    c == "public.app-category.games" || c.ends_with("-games")
}

/// macOS `lsappinfo info -only bundlepath <front>`: the bundle path.
pub fn parse_lsappinfo_bundle(text: &str) -> Option<String> {
    let (_, rest) = text.split_once('=')?;
    let path = rest.trim().trim_matches('"');
    (!path.is_empty() && path != "[ NULL ]").then(|| path.to_string())
}

/// macOS `ioreg -c IOHIDSystem`: `"HIDIdleTime" = <ns>` as seconds.
pub fn parse_ioreg_idle_secs(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.contains("\"HIDIdleTime\""))?;
    let ns: u64 = line.rsplit('=').next()?.trim().parse().ok()?;
    Some(ns / 1_000_000_000)
}

/// macOS `sysctl -n kern.memorystatus_vm_pressure_level`: 1 normal, 2 warn, 4 critical.
pub fn parse_mac_pressure(text: &str) -> Option<bool> {
    text.trim().parse::<u8>().ok().map(|l| l >= 2)
}

/// macOS `pmset -g therm`: a CPU speed limit under 100, or a thermal warning.
pub fn parse_pmset_therm(text: &str) -> Thermal {
    let limit = text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "CPU_Speed_Limit")
            .then(|| v.trim().parse::<u32>().ok())
            .flatten()
    });
    let warning = text
        .lines()
        .any(|l| l.contains("thermal warning level") && !l.contains("No thermal warning"));
    match limit {
        Some(l) if l < 50 => Thermal::Critical,
        Some(l) if l < 100 => Thermal::Serious,
        Some(_) if warning => Thermal::Fair,
        Some(_) => Thermal::Nominal,
        None if warning => Thermal::Fair,
        None => Thermal::Unknown,
    }
}

/// macOS `pmset -g batt` and the `lowpowermode` line of `pmset -g`.
pub fn parse_pmset_batt(batt: &str, settings: &str) -> PowerInfo {
    let percent = batt.lines().find_map(|l| {
        let (head, _) = l.split_once('%')?;
        head.rsplit(|c: char| !c.is_ascii_digit())
            .next()?
            .parse::<u8>()
            .ok()
    });
    let saver = settings.lines().any(|l| {
        let mut w = l.split_whitespace();
        w.next() == Some("lowpowermode") && w.next() == Some("1")
    });
    PowerInfo {
        battery: batt.contains("InternalBattery"),
        on_ac: !batt.contains("'Battery Power'"),
        percent,
        power_saver: saver,
    }
}

/// `/proc/<pid>/status` `VmRSS` in MB.
pub fn parse_vmrss_mb(status: &str) -> Option<u64> {
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1_024)
}

#[cfg(test)]
#[path = "local_governor_tests.rs"]
mod tests;
