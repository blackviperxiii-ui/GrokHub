//! PC health while gaming, as the cabin had it: an opt-in switch in Settings
//! → Pulse. While it is on and a game runs, the shell reads CPU and GPU
//! temperatures, use and clocks every 15 seconds. When the game ends it saves
//! the session to the same `pc-health.json` ledger the card's Options window
//! reads, and posts one Pulse summary. It only reads: it never changes fans,
//! clocks or drivers. Off, paused, or with Pulse turned off, it reads nothing.

use std::path::Path;
use std::sync::Mutex;

use grokhub_core::pc_health::{self as ph, GameSession, ObserveOut, PcHealthLedger, SensorSample};
use grokhub_core::pc_health_advice as advice;
use grokhub_core::{PcHealthSettings, UpdateCard};
use serde::Serialize;

/// The cabin's `app.json` key.
pub const KEY: &str = "pcHealth";
/// The same game posts at most one summary in this long.
const SAME_GAME_QUIET_MS: u64 = 30 * 60 * 1000;

/// One read-change-write of the switch at a time.
static SWITCH: Mutex<()> = Mutex::new(());

/// The switch as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub on: bool,
}

/// The saved settings, or the defaults (off) when there are none.
pub fn settings(config_dir: &Path) -> PcHealthSettings {
    std::fs::read_to_string(config_dir.join("app.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get(KEY).cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn view(config_dir: &Path) -> View {
    View {
        on: settings(config_dir).enabled,
    }
}

/// Turn it on or off. The rest of its settings (quiet, warm lines, pause)
/// stay as they were.
pub fn set_enabled(config_dir: &Path, on: bool) -> Result<View, String> {
    let _one = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let mut saved = settings(config_dir);
    saved.enabled = on;
    let value = serde_json::to_value(&saved).map_err(|e| e.to_string())?;
    grokhub_core::app_json::patch(config_dir, &[(KEY, value)])?;
    Ok(view(config_dir))
}

/// Whether to read the sensors now: switched on, Pulse allowed, not paused.
fn due(settings: &PcHealthSettings, pulse: bool, now_ms: u64) -> bool {
    settings.enabled && pulse && now_ms >= settings.paused_until_ms
}

/// One reading: whether a game runs, its sensors while it does, and the CPU
/// counters for the next reading's CPU use.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probe {
    pub game_up: bool,
    pub sample: Option<SensorSample>,
    pub cpu: Option<(u64, u64)>,
}

/// What the watcher keeps between readings.
#[derive(Debug, Default)]
pub struct Watch {
    session: Option<GameSession>,
    cpu_prev: Option<(u64, u64)>,
}

impl Watch {
    /// One tick. When it isn't [`due`], a session in progress is dropped
    /// without a card and `read` is never called. Otherwise `read` takes a
    /// reading, and when the game has ended the session is saved and its
    /// card, if it should post, comes back.
    pub fn tick(
        &mut self,
        config_dir: &Path,
        settings: &PcHealthSettings,
        pulse: bool,
        now_ms: u64,
        read: impl FnOnce(u64, Option<(u64, u64)>) -> Probe,
    ) -> Option<UpdateCard> {
        if !due(settings, pulse, now_ms) {
            *self = Self::default();
            return None;
        }
        let probe = read(now_ms, self.cpu_prev);
        self.cpu_prev = probe.cpu.or(self.cpu_prev);
        match ph::observe(true, probe.game_up, probe.sample, &mut self.session, now_ms) {
            ObserveOut::Idle | ObserveOut::Sampling => None,
            ObserveOut::Ended(summary) => session_ended(config_dir, &summary, settings, now_ms),
        }
    }
}

fn ledger_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(ph::LEDGER_FILE)
}

fn load_ledger(config_dir: &Path) -> PcHealthLedger {
    std::fs::read_to_string(ledger_path(config_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// A finished session goes into the ledger; its card comes back unless the
/// same game had one in the last half hour, the verdict is normal and quiet
/// is on, or the cards are paused.
fn session_ended(
    config_dir: &Path,
    summary: &ph::SessionSummary,
    settings: &PcHealthSettings,
    now_ms: u64,
) -> Option<UpdateCard> {
    let mut ledger = load_ledger(config_dir);
    let stat = advice::SessionStat::from_summary(summary, now_ms);
    let verdict = advice::verdict(&stat, &ledger.history, settings);
    let game = stat.game.clone();
    advice::push_history(&mut ledger.history, stat);
    let cooling = now_ms.saturating_sub(ledger.last_summary_ms) < SAME_GAME_QUIET_MS
        && ledger.last_game == game;
    let post = !cooling && advice::should_post(&verdict, settings, now_ms);
    if post {
        ledger.last_summary_ms = now_ms;
        ledger.last_game = game;
    }
    match serde_json::to_string_pretty(&ledger) {
        Ok(text) => {
            if let Err(why) =
                grokhub_core::pulse_writer::write_atomic(&ledger_path(config_dir), &text)
            {
                eprintln!("pc health ledger: {why}");
            }
        }
        Err(why) => eprintln!("pc health ledger: {why}"),
    }
    post.then(|| {
        let mut card = ph::summary_card(summary, now_ms);
        card.pulse.verdict = Some(verdict);
        card
    })
}

/// Reads while a game runs, every [`ph::SAMPLE_EVERY_MS`].
#[cfg(desktop)]
pub fn start(dir: std::path::PathBuf) {
    std::thread::spawn(move || {
        let mut watch = Watch::default();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(ph::SAMPLE_EVERY_MS));
            let card = watch.tick(
                &dir,
                &settings(&dir),
                crate::capabilities::current().pulse,
                grokhub_core::now_ms(),
                sensors::collect,
            );
            if let Some(card) = card {
                if let Err(why) = crate::pulse::add_post(&dir, card) {
                    eprintln!("pc health card: {why}");
                }
            }
        }
    });
}

#[cfg(mobile)]
pub fn start(_dir: std::path::PathBuf) {}

/// The readings: the process list, `nvidia-smi`, and on Linux the kernel's
/// hwmon, drm and `/proc` files. Nothing here writes anywhere.
#[cfg(desktop)]
mod sensors {
    use super::Probe;
    #[cfg(target_os = "linux")]
    use grokhub_core::pc_health::{
        cpu_util_between, parse_amdgpu_busy, parse_cpuinfo_mhz, parse_hwmon_temp_milli,
        parse_proc_stat_cpu, temps_from_hwmon,
    };
    #[cfg(any(target_os = "linux", windows))]
    use grokhub_core::pc_health::{game_running, nvidia_max_util, parse_nvidia_sensor_csv};
    #[cfg(any(target_os = "linux", windows))]
    use grokhub_core::SensorSample;

    /// A sensor command that hasn't answered by now is killed, and that
    /// reading counts as none.
    #[cfg(any(target_os = "linux", windows))]
    const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    pub fn collect(now: u64, cpu_prev: Option<(u64, u64)>) -> Probe {
        #[cfg(target_os = "linux")]
        {
            linux(now, cpu_prev)
        }
        #[cfg(windows)]
        {
            let _ = cpu_prev;
            windows(now)
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = (now, cpu_prev);
            Probe::default()
        }
    }

    #[cfg(any(target_os = "linux", windows))]
    fn probe_cmd(cmd: &str, args: &[&str]) -> Option<String> {
        let mut c = std::process::Command::new(cmd);
        c.args(args);
        grokhub_core::proc_util::output_within(&mut c, PROBE_TIMEOUT, 1024 * 1024)
    }

    /// The hottest NVIDIA card's temp, use and clock, and the busiest card's
    /// use for the GPU-busy game check.
    #[cfg(any(target_os = "linux", windows))]
    fn nvidia() -> Option<((f32, u8, u32), Option<u8>)> {
        let text = probe_cmd(
            "nvidia-smi",
            &[
                "--query-gpu=temperature.gpu,utilization.gpu,clocks.current.graphics",
                "--format=csv,noheader,nounits",
            ],
        )?;
        Some((parse_nvidia_sensor_csv(&text)?, nvidia_max_util(&text)))
    }

    #[cfg(target_os = "linux")]
    fn linux(now: u64, cpu_prev: Option<(u64, u64)>) -> Probe {
        let procs = grokhub_desktop::game_guard::processes().unwrap_or_default();
        let mut sample = SensorSample {
            at_ms: now,
            game_label: super::game_label(&procs),
            ..Default::default()
        };
        let mut busiest = None;
        if let Some(((t, u, c), max_u)) = nvidia() {
            sample.gpu_temp_c = Some(t);
            sample.gpu_util_pct = Some(u);
            sample.gpu_clock_mhz = Some(c);
            busiest = max_u;
        }
        if sample.gpu_util_pct.is_none() {
            sample.gpu_util_pct = amdgpu_busy();
        }
        let (cpu_t, gpu_t) = temps_from_hwmon(&hwmon_temps());
        sample.cpu_temp_c = cpu_t;
        sample.gpu_temp_c = sample.gpu_temp_c.or(gpu_t);
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

    /// Process names from a Toolhelp snapshot and Steam's running app, so no
    /// process is opened and no anti-cheat sees it; `nvidia-smi` when there.
    #[cfg(windows)]
    fn windows(now: u64) -> Probe {
        let mut sample = SensorSample {
            at_ms: now,
            game_label: grokhub_desktop::game_guard::windows_game_label(),
            ..Default::default()
        };
        let mut busiest = None;
        if let Some(((t, u, c), max_u)) = nvidia() {
            sample.gpu_temp_c = Some(t);
            sample.gpu_util_pct = Some(u);
            sample.gpu_clock_mhz = Some(c);
            busiest = max_u;
        }
        let game_up = grokhub_desktop::game_guard::windows_game() == Some(true)
            || game_running(&[], busiest.or(sample.gpu_util_pct));
        Probe {
            game_up,
            sample: game_up.then_some(sample),
            cpu: None,
        }
    }

    #[cfg(target_os = "linux")]
    fn hwmon_temps() -> Vec<(String, i64)> {
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
                if let Some(c) = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| parse_hwmon_temp_milli(t.trim()))
                {
                    out.push((name.clone(), (c * 1000.0) as i64));
                }
            }
        }
        out
    }

    #[cfg(target_os = "linux")]
    fn amdgpu_busy() -> Option<u8> {
        let entries = std::fs::read_dir("/sys/class/drm").ok()?;
        entries.flatten().find_map(|ent| {
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("card") || name.contains('-') {
                return None;
            }
            std::fs::read_to_string(ent.path().join("device/gpu_busy_percent"))
                .ok()
                .and_then(|t| parse_amdgpu_busy(&t))
        })
    }
}

/// The game's name for the card: an Unreal `-Win64-Shipping` process, else
/// "a game" for gamescope, Steam or Proton.
#[cfg(any(test, all(desktop, target_os = "linux")))]
fn game_label(procs: &[(String, String)]) -> Option<String> {
    for (comm, cmdline) in procs {
        let c = comm.to_ascii_lowercase();
        if let Some(name) = c.strip_suffix("-win64-shipping") {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-pc-health-{tag}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn on() -> PcHealthSettings {
        PcHealthSettings {
            enabled: true,
            ..PcHealthSettings::default()
        }
    }

    fn playing(gpu_c: f32) -> impl Fn(u64, Option<(u64, u64)>) -> Probe {
        move |at_ms, _| Probe {
            game_up: true,
            sample: Some(SensorSample {
                at_ms,
                gpu_temp_c: Some(gpu_c),
                cpu_temp_c: Some(60.0),
                gpu_util_pct: Some(80),
                cpu_util_pct: Some(40),
                game_label: Some("Hades".into()),
                ..SensorSample::default()
            }),
            cpu: Some((1_000 + at_ms, 500)),
        }
    }

    fn quit(_: u64, _: Option<(u64, u64)>) -> Probe {
        Probe::default()
    }

    /// Plays from `start` for 90 seconds at `gpu_c`, then quits. Returns
    /// the card the quit brought, if any.
    fn session(
        watch: &mut Watch,
        dir: &Path,
        settings: &PcHealthSettings,
        start: u64,
        gpu_c: f32,
    ) -> Option<UpdateCard> {
        for i in 0..7 {
            let card = watch.tick(dir, settings, true, start + i * 15_000, playing(gpu_c));
            assert_eq!(card, None, "no card while the game runs");
        }
        watch.tick(dir, settings, true, start + 120_000, quit)
    }

    #[test]
    fn the_switch_is_off_by_default_and_keeps_its_other_settings() {
        let dir = temp("switch");
        assert_eq!(view(&dir), View { on: false });
        std::fs::write(
            dir.join("app.json"),
            r#"{"theme":"dark","pcHealth":{"quietWhenNormal":true,"gpuWarnC":78}}"#,
        )
        .unwrap();
        assert_eq!(set_enabled(&dir, true).unwrap(), View { on: true });
        assert_eq!(
            settings(&dir),
            PcHealthSettings {
                enabled: true,
                quiet_when_normal: true,
                gpu_warn_c: Some(78),
                ..PcHealthSettings::default()
            }
        );
        assert_eq!(set_enabled(&dir, false).unwrap(), View { on: false });
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("app.json")).unwrap()).unwrap();
        assert_eq!(saved["theme"], "dark");
        assert_eq!(
            saved["pcHealth"],
            serde_json::json!({"enabled": false, "quietWhenNormal": true, "gpuWarnC": 78})
        );
    }

    #[test]
    fn off_paused_or_without_pulse_nothing_is_read_and_a_game_in_progress_is_dropped() {
        let dir = temp("idle");
        let reads = Cell::new(0);
        let counted = |at, prev| {
            reads.set(reads.get() + 1);
            playing(70.0)(at, prev)
        };
        let mut watch = Watch::default();
        assert_eq!(watch.tick(&dir, &on(), true, 0, counted), None);
        assert_eq!(watch.tick(&dir, &on(), true, 15_000, counted), None);
        assert_eq!(reads.get(), 2);
        assert!(watch.session.is_some(), "a game is being watched");
        let paused = PcHealthSettings {
            paused_until_ms: 1_000_000,
            ..on()
        };
        for (settings, pulse) in [
            (PcHealthSettings::default(), true),
            (paused, true),
            (on(), false),
        ] {
            assert_eq!(watch.tick(&dir, &settings, pulse, 30_000, counted), None);
            assert_eq!(reads.get(), 2, "{settings:?} pulse {pulse}: no reading");
            assert!(watch.session.is_none(), "the game in progress is dropped");
        }
        // Back on: the earlier game is gone, so quitting now posts nothing.
        assert_eq!(watch.tick(&dir, &on(), true, 200_000, quit), None);
        assert!(!dir.join(ph::LEDGER_FILE).exists(), "nothing was saved");
    }

    #[test]
    fn a_finished_game_saves_the_session_and_posts_one_card_its_options_can_read() {
        let dir = temp("posts");
        let _config = grokhub_agent::perm::ConfigGuard::set(&dir);
        let mut watch = Watch::default();
        let card = session(&mut watch, &dir, &on(), 1_000_000, 70.0).expect("a card");
        assert!(
            card.source_id.starts_with("pchealth:"),
            "{}",
            card.source_id
        );
        let verdict = card.pulse.verdict.as_ref().expect("a verdict");
        assert!(
            verdict.line.starts_with("Looks healthy"),
            "{}",
            verdict.line
        );
        let ledger = load_ledger(&dir);
        assert_eq!(ledger.history.len(), 1);
        assert_eq!(ledger.history[0].max_gpu_c, Some(70));
        assert_eq!(ledger.last_summary_ms, 1_120_000);
        crate::pulse::add_post(&dir, card.clone()).unwrap();
        let options = crate::pulse::fix(&dir, &card.id).unwrap();
        assert_eq!(options.title, card.title);
        assert!(!options.options.is_empty());
        assert!(
            options
                .options
                .iter()
                .all(|o| o.kind != advice::FixKind::FansFull),
            "the fan fix never shows here"
        );
    }

    #[test]
    fn quiet_and_the_same_game_half_hour_skip_the_card_but_keep_the_session() {
        let dir = temp("quiet");
        let mut watch = Watch::default();
        assert!(session(&mut watch, &dir, &on(), 1_000_000, 70.0).is_some());
        // The same game again ten minutes later: saved, no card.
        assert_eq!(session(&mut watch, &dir, &on(), 1_600_000, 86.0), None);
        assert_eq!(load_ledger(&dir).history.len(), 2);
        // An hour later with quiet on, a normal session posts nothing...
        let quiet = PcHealthSettings {
            quiet_when_normal: true,
            ..on()
        };
        assert_eq!(session(&mut watch, &dir, &quiet, 5_000_000, 70.0), None);
        assert_eq!(load_ledger(&dir).history.len(), 3);
        // ...and a hot one still does.
        assert!(session(&mut watch, &dir, &quiet, 9_000_000, 88.0).is_some());
        assert_eq!(load_ledger(&dir).history.len(), 4);
    }

    #[test]
    fn a_session_under_a_minute_is_not_saved() {
        let dir = temp("short");
        let mut watch = Watch::default();
        for i in 0..3 {
            watch.tick(&dir, &on(), true, i * 15_000, playing(70.0));
        }
        assert_eq!(watch.tick(&dir, &on(), true, 45_000, quit), None);
        assert!(!dir.join(ph::LEDGER_FILE).exists());
    }

    #[test]
    fn the_card_names_an_unreal_game_or_says_a_game() {
        let p = |comm: &str, cmd: &str| vec![(comm.to_string(), cmd.to_string())];
        assert_eq!(
            game_label(&p("Hades2-Win64-Shipping", "")).as_deref(),
            Some("hades2")
        );
        assert_eq!(
            game_label(&p("reaper", "SteamLaunch AppId=1145350")).as_deref(),
            Some("a game")
        );
        assert_eq!(game_label(&p("gamescope", "")).as_deref(), Some("a game"));
        assert_eq!(game_label(&p("bash", "/bin/bash")), None);
    }
}
