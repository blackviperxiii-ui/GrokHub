//! What a PC health summary means, and what to do about it. Pure rules, no
//! model call: a one-line verdict against this PC's own recent sessions, and
//! the options a card's Fix it sheet offers. Report and guidance only — this
//! module never changes fans, clocks, or drivers, and nothing here touches a
//! game.

use serde::{Deserialize, Serialize};

use crate::pc_health::{
    BoundGuess, PcHealthSettings, SessionSummary, CPU_CRITICAL_C, CPU_HOT_C, GPU_CRITICAL_C,
    GPU_HOT_C, SOURCE_PREFIX,
};
use crate::pulse::{PulseVerdict, VerdictTone};

/// Sessions kept in `pc-health.json` for the baseline and the compare view.
pub const HISTORY_CAP: usize = 30;
/// Prior sessions needed before "your usual" means anything.
pub const BASELINE_MIN: usize = 3;
/// A peak this many °C over the usual one reads as warmer than usual.
pub const DRIFT_C: u8 = 6;
/// GPU peak and average busy that read as hot for a light load.
pub const LIGHT_LOAD_HOT_C: u8 = 75;
pub const LIGHT_LOAD_PCT: u8 = 35;
/// How long Pause these cards lasts.
pub const PAUSE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// One finished session as the ledger keeps it: whole °C and %.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStat {
    /// When the session ended (the card's source id carries the same ms).
    pub at_ms: u64,
    /// Lowercase game key ("" when unknown).
    #[serde(default)]
    pub game: String,
    /// The game's name as detected, for the compare table.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default)]
    pub mins: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cpu_c: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_gpu_c: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_cpu_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_gpu_pct: Option<u8>,
    #[serde(default)]
    pub throttles: u32,
    /// "gpu", "cpu" or "" — the bound guess.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bound: String,
}

fn whole(v: Option<f32>) -> Option<u8> {
    v.map(|x| x.round().clamp(0.0, 255.0) as u8)
}

/// The ledger key for a game label.
pub fn game_key(label: Option<&str>) -> String {
    label.unwrap_or("game").to_ascii_lowercase()
}

impl SessionStat {
    pub fn from_summary(summary: &SessionSummary, at_ms: u64) -> Self {
        Self {
            at_ms,
            game: game_key(summary.game_label.as_deref()),
            label: summary.game_label.clone().unwrap_or_default(),
            mins: (summary.duration_ms / 60_000).max(1) as u32,
            max_cpu_c: whole(summary.max_cpu_temp_c),
            max_gpu_c: whole(summary.max_gpu_temp_c),
            avg_cpu_pct: whole(summary.avg_cpu_util),
            avg_gpu_pct: whole(summary.avg_gpu_util),
            throttles: summary.throttles.len() as u32,
            bound: match summary.bound {
                BoundGuess::Gpu => "gpu".into(),
                BoundGuess::Cpu => "cpu".into(),
                BoundGuess::Balanced | BoundGuess::Unknown => String::new(),
            },
        }
    }

    fn game_name(&self) -> String {
        if !self.label.trim().is_empty() {
            self.label.trim().to_string()
        } else if !self.game.is_empty() && self.game != "game" {
            self.game.clone()
        } else {
            "your game".into()
        }
    }
}

/// Keep the newest `HISTORY_CAP` sessions, oldest first.
pub fn push_history(history: &mut Vec<SessionStat>, stat: SessionStat) {
    history.push(stat);
    let over = history.len().saturating_sub(HISTORY_CAP);
    if over > 0 {
        history.drain(..over);
    }
}

/// This PC's usual peaks, from earlier sessions. `same_game` says the numbers
/// come from that game alone (it had enough sessions of its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Baseline {
    pub gpu_c: Option<u8>,
    pub cpu_c: Option<u8>,
    pub sessions: usize,
    pub same_game: bool,
}

fn median(mut v: Vec<u8>) -> Option<u8> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[v.len() / 2])
}

/// The usual peaks before `stat`: the same game when it has `BASELINE_MIN`
/// earlier sessions, else every game. `None` until there are enough.
pub fn baseline(stat: &SessionStat, history: &[SessionStat]) -> Option<Baseline> {
    let earlier: Vec<&SessionStat> = history.iter().filter(|s| s.at_ms < stat.at_ms).collect();
    let same: Vec<&SessionStat> = earlier
        .iter()
        .copied()
        .filter(|s| s.game == stat.game)
        .collect();
    let (pool, same_game) = if same.len() >= BASELINE_MIN {
        (same, true)
    } else if earlier.len() >= BASELINE_MIN {
        (earlier, false)
    } else {
        return None;
    };
    Some(Baseline {
        gpu_c: median(pool.iter().filter_map(|s| s.max_gpu_c).collect()),
        cpu_c: median(pool.iter().filter_map(|s| s.max_cpu_c).collect()),
        sessions: pool.len(),
        same_game,
    })
}

/// The temperature lines this user warns at (their own, else the defaults).
pub fn warn_lines(settings: &PcHealthSettings) -> (u8, u8) {
    (
        settings.gpu_warn_c.unwrap_or(GPU_HOT_C as u8),
        settings.cpu_warn_c.unwrap_or(CPU_HOT_C as u8),
    )
}

/// One line that says what the numbers mean. Rules, in order: throttling or a
/// danger-line peak is worth a look; a peak over the user's warning line, a
/// peak well over their usual one, or a hot GPU on a light load is worth
/// watching; anything else is normal for this PC.
pub fn verdict(
    stat: &SessionStat,
    history: &[SessionStat],
    settings: &PcHealthSettings,
) -> PulseVerdict {
    let (gpu_warn, cpu_warn) = warn_lines(settings);
    let act = |line: String| PulseVerdict {
        tone: VerdictTone::Act,
        line,
    };
    let watch = |line: String| PulseVerdict {
        tone: VerdictTone::Watch,
        line,
    };
    let fine = |line: String| PulseVerdict {
        tone: VerdictTone::Fine,
        line,
    };
    let hottest = match (stat.max_gpu_c, stat.max_cpu_c) {
        (Some(g), Some(c)) if c as f32 - CPU_HOT_C > g as f32 - GPU_HOT_C => ("CPU", c),
        (Some(g), _) => ("GPU", g),
        (None, Some(c)) => ("CPU", c),
        (None, None) => ("GPU", 0),
    };
    if stat.throttles > 0 {
        let times = if stat.throttles == 1 {
            "once".to_string()
        } else {
            format!("{} times", stat.throttles)
        };
        return act(format!(
            "Worth a look: it throttled {times}, and the {} hit {}°C.",
            hottest.0, hottest.1
        ));
    }
    if let Some(g) = stat.max_gpu_c.filter(|g| *g as f32 >= GPU_CRITICAL_C) {
        return act(format!(
            "Worth a look: the GPU hit {g}°C, past the {}°C danger line.",
            GPU_CRITICAL_C as u8
        ));
    }
    if let Some(c) = stat.max_cpu_c.filter(|c| *c as f32 >= CPU_CRITICAL_C) {
        return act(format!(
            "Worth a look: the CPU hit {c}°C, past the {}°C danger line.",
            CPU_CRITICAL_C as u8
        ));
    }
    if let Some(g) = stat.max_gpu_c.filter(|g| *g >= gpu_warn) {
        return watch(format!(
            "Running warm: the GPU peaked at {g}°C, over your {gpu_warn}°C line."
        ));
    }
    if let Some(c) = stat.max_cpu_c.filter(|c| *c >= cpu_warn) {
        return watch(format!(
            "Running warm: the CPU peaked at {c}°C, over your {cpu_warn}°C line."
        ));
    }
    let base = baseline(stat, history);
    let usual = |b: &Baseline| {
        if b.same_game {
            format!("your usual in {}", stat.game_name())
        } else {
            "your usual".to_string()
        }
    };
    if let Some(b) = base.as_ref() {
        for (part, now, was) in [
            ("GPU", stat.max_gpu_c, b.gpu_c),
            ("CPU", stat.max_cpu_c, b.cpu_c),
        ] {
            if let (Some(now), Some(was)) = (now, was) {
                if now >= was.saturating_add(DRIFT_C) {
                    return watch(format!(
                        "Warmer than usual: the {part} ran {}°C hotter than {} ({was}°C).",
                        now - was,
                        usual(b)
                    ));
                }
            }
        }
    }
    if let (Some(g), Some(busy)) = (stat.max_gpu_c, stat.avg_gpu_pct) {
        if g >= LIGHT_LOAD_HOT_C && busy < LIGHT_LOAD_PCT {
            return watch(format!(
                "Warm for a light load: the GPU hit {g}°C while only {busy}% busy on average. Airflow or a background app could be why."
            ));
        }
    }
    match base {
        Some(b) => {
            if let (Some(now), Some(was)) = (stat.max_gpu_c, b.gpu_c) {
                if was >= now.saturating_add(DRIFT_C) {
                    return fine(format!(
                        "Cooler than usual: the GPU ran {}°C under {} ({was}°C).",
                        was - now,
                        usual(&b)
                    ));
                }
            }
            let peak = match b.gpu_c {
                Some(g) => format!(" (GPU {g}°C)"),
                None => String::new(),
            };
            fine(format!(
                "Normal for this PC: peaks matched {}{peak}, with no throttling.",
                usual(&b)
            ))
        }
        None => {
            fine("Looks healthy: no throttling, and temps stayed under your warning lines.".into())
        }
    }
}

/// What a Fix it sheet row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FixKind {
    /// Show this session next to the last few.
    Compare,
    /// Steps to cap FPS or turn on vsync for the game.
    FpsCap,
    /// Steps to turn on DLSS / FSR / XeSS.
    Upscale,
    /// Steps to clear dust and check airflow.
    Airflow,
    /// Steps to check the fan curve in the vendor tool.
    FanCurve,
    /// Steps to think about fresh thermal paste.
    Repaste,
    /// Settings: post only when something's off (or after every session).
    QuietWhenNormal,
    /// Settings: the °C lines the verdict warns at.
    WarnLines,
    /// Settings: no PC health cards for a week.
    Pause,
    /// Open a chat about it with the card's prompt.
    Ask,
}

impl FixKind {
    /// A row that changes a PC health setting rather than showing guidance.
    pub fn is_setting(self) -> bool {
        matches!(self, Self::QuietWhenNormal | Self::WarnLines | Self::Pause)
    }
}

/// One row of the Fix it sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixOption {
    pub kind: FixKind,
    pub title: String,
    pub why: String,
    /// Guidance the user follows. Empty for rows that act in the app.
    pub steps: Vec<String>,
}

fn opt(kind: FixKind, title: impl Into<String>, why: impl Into<String>) -> FixOption {
    FixOption {
        kind,
        title: title.into(),
        why: why.into(),
        steps: Vec::new(),
    }
}

fn hot_in_recent(history: &[SessionStat], settings: &PcHealthSettings) -> usize {
    let (gpu_warn, cpu_warn) = warn_lines(settings);
    history
        .iter()
        .rev()
        .take(5)
        .filter(|s| {
            s.throttles > 0
                || s.max_gpu_c.is_some_and(|g| g >= gpu_warn)
                || s.max_cpu_c.is_some_and(|c| c >= cpu_warn)
        })
        .count()
}

/// The sheet's rows for one session, most useful first. Fixes come from what
/// the session showed; the settings rows and Ask are always there.
pub fn fix_options(
    stat: &SessionStat,
    history: &[SessionStat],
    settings: &PcHealthSettings,
) -> Vec<FixOption> {
    let v = verdict(stat, history, settings);
    let (gpu_warn, cpu_warn) = warn_lines(settings);
    let game = stat.game_name();
    let gpu_warm = stat.max_gpu_c.is_some_and(|g| g >= LIGHT_LOAD_HOT_C);
    let warm = v.tone != VerdictTone::Fine;
    let mut out = Vec::new();
    if history.iter().any(|s| s.at_ms != stat.at_ms) {
        out.push(opt(
            FixKind::Compare,
            "Compare with your last sessions",
            "See this run next to your recent ones: time, peak temps and how busy each part was.",
        ));
    }
    if warm || gpu_warm {
        let mut fps = opt(
            FixKind::FpsCap,
            format!("Cap the frame rate in {game}"),
            "Frames past your monitor's refresh rate are heat you never see. A cap is the quickest way to cool a GPU down.",
        );
        fps.steps = vec![
            format!("In {game}'s video settings, set a frame limit at or just under your monitor's refresh rate, or turn on vsync."),
            "No in-game limit? NVIDIA: Control Panel > Manage 3D settings > Program Settings > pick the game > Max Frame Rate.".into(),
            "AMD: Adrenalin > Gaming > pick the game > Radeon Chill or Frame Rate Target Control.".into(),
            "Play a while, then compare the next card with this one.".into(),
        ];
        out.push(fps);
    }
    if stat.bound == "gpu" {
        let mut up = opt(
            FixKind::Upscale,
            "Turn on DLSS, FSR or XeSS",
            "This session looked GPU-bound. Upscaling renders fewer pixels, so you get more FPS and less heat.",
        );
        up.steps = vec![
            format!("In {game}'s graphics settings, find Upscaling (DLSS, FSR or XeSS)."),
            "Start at Quality mode and only drop to Balanced if you need more FPS.".into(),
        ];
        out.push(up);
    }
    if warm {
        let mut air = opt(
            FixKind::Airflow,
            "Clear the dust and check airflow",
            "Dust in the heatsinks and blocked vents are the most common reason temps creep up.",
        );
        air.steps = vec![
            "Shut down and unplug the PC first.".into(),
            "Blow dust out of the GPU, CPU cooler, radiator and case filters with compressed air. Hold the fans still while you do.".into(),
            "Make sure the intake and exhaust vents aren't blocked by a wall, desk or carpet.".into(),
            "On a laptop, play on a hard, flat surface or a stand, not on a bed.".into(),
        ];
        out.push(air);
        let mut fans = opt(
            FixKind::FanCurve,
            "Check the fan curve",
            crate::brand::filled(
                "A quiet fan profile can let parts run hot. {product} only reads sensors and never changes fans itself.",
            ),
        );
        fans.steps = vec![
            "Open your vendor tool: MSI Afterburner, your motherboard's BIOS or app, Alienware Command Center, or Armoury Crate.".into(),
            "Pick a Balanced or Performance profile, or make the curve ramp up earlier once it passes 70°C.".into(),
            "Test it in the same game and compare the next card.".into(),
        ];
        out.push(fans);
        if stat.throttles > 0 || hot_in_recent(history, settings) >= 3 {
            let mut paste = opt(
                FixKind::Repaste,
                "Think about fresh thermal paste",
                "It's been running hot over several sessions. Old paste on a GPU or CPU that's a few years old can add 10°C or more.",
            );
            paste.steps = vec![
                "Try the dust and fan steps first. They're easier and often enough.".into(),
                "If temps stay high and the PC is two or more years old, a repaste can help. A shop can do it if you'd rather not open the card.".into(),
                "Check your warranty first, because opening a GPU can void it.".into(),
            ];
            out.push(paste);
        }
    }
    if settings.quiet_when_normal {
        out.push(opt(
            FixKind::QuietWhenNormal,
            "Post after every session",
            "Right now I only post when something's off.",
        ));
    } else {
        out.push(opt(
            FixKind::QuietWhenNormal,
            "Only post when something's off",
            "Skip the card after normal sessions. You'll still hear about throttling and hot runs.",
        ));
    }
    out.push(opt(
        FixKind::WarnLines,
        "Change when I warn you",
        format!("I call it warm at GPU {gpu_warn}°C and CPU {cpu_warn}°C right now."),
    ));
    out.push(opt(
        FixKind::Pause,
        "Pause these cards for a week",
        "Sensors stay off while paused. Turn it back on any time in Settings.",
    ));
    out.push(opt(
        FixKind::Ask,
        crate::brand::filled("Ask {product} about it"),
        "Open a chat with this session's numbers to dig in.",
    ));
    out
}

/// The sheet's rows for a card. A card whose session fell out of the ledger
/// still gets the settings rows and Ask.
pub fn sheet_options(
    stat: Option<&SessionStat>,
    history: &[SessionStat],
    settings: &PcHealthSettings,
) -> Vec<FixOption> {
    let opts = fix_options(&stat.cloned().unwrap_or_default(), history, settings);
    if stat.is_some() {
        opts
    } else {
        opts.into_iter()
            .filter(|o| o.kind.is_setting() || o.kind == FixKind::Ask)
            .collect()
    }
}

/// The main button's word on a PC health card: Fix it when the verdict
/// found something, Options when it's normal. `None` on any other card.
pub fn fix_label(card: &crate::UpdateCard) -> Option<&'static str> {
    if !crate::pc_health::is_pc_health_card(card) {
        return None;
    }
    Some(match card.pulse.verdict.as_ref().map(|v| v.tone) {
        Some(VerdictTone::Watch | VerdictTone::Act) => "Fix it",
        _ => "Options",
    })
}

/// One line of the compare table, ready to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareRow {
    pub session: String,
    pub time: String,
    pub gpu_peak: String,
    pub cpu_peak: String,
    pub gpu_busy: String,
}

/// The compare table: this session first ("This one"), then the five most
/// recent others, newest first.
pub fn compare_rows(stat: Option<&SessionStat>, history: &[SessionStat]) -> Vec<CompareRow> {
    let deg = |v: Option<u8>| v.map(|c| format!("{c}°C")).unwrap_or_else(|| "–".into());
    let pct = |v: Option<u8>| v.map(|c| format!("{c}%")).unwrap_or_else(|| "–".into());
    let row = |name: String, s: &SessionStat| CompareRow {
        session: name,
        time: format!("{} min", s.mins),
        gpu_peak: deg(s.max_gpu_c),
        cpu_peak: deg(s.max_cpu_c),
        gpu_busy: pct(s.avg_gpu_pct),
    };
    let mut out: Vec<CompareRow> = stat
        .map(|s| row("This one".into(), s))
        .into_iter()
        .collect();
    out.extend(
        history
            .iter()
            .rev()
            .filter(|s| stat.is_none_or(|t| t.at_ms != s.at_ms))
            .take(5)
            .map(|s| {
                let name = if s.label.is_empty() {
                    s.game.clone()
                } else {
                    s.label.clone()
                };
                row(name, s)
            }),
    );
    out
}

/// The session behind a PC health card, from its source id
/// (`pchealth:<game>:<ms>`).
pub fn card_session<'a>(source_id: &str, history: &'a [SessionStat]) -> Option<&'a SessionStat> {
    let rest = source_id.strip_prefix(SOURCE_PREFIX)?;
    let ms: u64 = rest.rsplit(':').next()?.parse().ok()?;
    history.iter().find(|s| s.at_ms == ms)
}

/// Whether a finished session should post a card at all.
pub fn should_post(v: &PulseVerdict, settings: &PcHealthSettings, now_ms: u64) -> bool {
    if now_ms < settings.paused_until_ms {
        return false;
    }
    !(settings.quiet_when_normal && v.tone == VerdictTone::Fine)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(at: u64, game: &str, gpu: u8, cpu: u8) -> SessionStat {
        SessionStat {
            at_ms: at,
            game: game.into(),
            label: game.into(),
            mins: 30,
            max_gpu_c: Some(gpu),
            max_cpu_c: Some(cpu),
            avg_gpu_pct: Some(80),
            avg_cpu_pct: Some(40),
            ..Default::default()
        }
    }

    #[test]
    fn throttling_is_worth_a_look_and_names_the_hottest_part() {
        let mut s = stat(10, "hades", 84, 70);
        s.throttles = 2;
        let v = verdict(&s, &[], &PcHealthSettings::default());
        assert_eq!(v.tone, VerdictTone::Act);
        assert_eq!(
            v.line,
            "Worth a look: it throttled 2 times, and the GPU hit 84°C."
        );
    }

    #[test]
    fn a_peak_over_the_users_own_line_is_worth_watching() {
        let s = stat(10, "hades", 80, 60);
        let mut settings = PcHealthSettings::default();
        assert_eq!(verdict(&s, &[], &settings).tone, VerdictTone::Fine);
        settings.gpu_warn_c = Some(78);
        let v = verdict(&s, &[], &settings);
        assert_eq!(v.tone, VerdictTone::Watch);
        assert!(v.line.contains("over your 78°C line"), "{}", v.line);
    }

    #[test]
    fn warmer_than_the_same_games_usual_says_by_how_much() {
        let history = vec![
            stat(1, "hades", 70, 60),
            stat(2, "hades", 72, 61),
            stat(3, "hades", 71, 59),
            stat(4, "other", 50, 50),
        ];
        let s = stat(10, "hades", 79, 62);
        let v = verdict(&s, &history, &PcHealthSettings::default());
        assert_eq!(v.tone, VerdictTone::Watch);
        assert_eq!(
            v.line,
            "Warmer than usual: the GPU ran 8°C hotter than your usual in hades (71°C)."
        );
    }

    #[test]
    fn the_baseline_needs_three_earlier_sessions_and_ignores_later_ones() {
        let s = stat(10, "hades", 79, 62);
        assert!(baseline(&s, &[stat(1, "a", 60, 60), stat(2, "b", 60, 60)]).is_none());
        let later = [
            stat(11, "a", 60, 60),
            stat(12, "a", 60, 60),
            stat(13, "a", 60, 60),
        ];
        assert!(baseline(&s, &later).is_none());
        let mixed = [
            stat(1, "a", 60, 50),
            stat(2, "b", 64, 52),
            stat(3, "c", 70, 54),
        ];
        let b = baseline(&s, &mixed).unwrap();
        assert_eq!((b.gpu_c, b.cpu_c, b.same_game), (Some(64), Some(52), false));
    }

    #[test]
    fn a_hot_gpu_on_a_light_load_is_flagged() {
        // The screenshot's card: GPU 81°C at 15% busy, CPU 69°C, no throttling.
        let mut s = stat(10, "game", 81, 69);
        s.avg_gpu_pct = Some(15);
        s.avg_cpu_pct = Some(4);
        let v = verdict(&s, &[], &PcHealthSettings::default());
        assert_eq!(v.tone, VerdictTone::Watch);
        assert!(v
            .line
            .starts_with("Warm for a light load: the GPU hit 81°C while only 15% busy"));
        let kinds: Vec<FixKind> = fix_options(&s, &[], &PcHealthSettings::default())
            .iter()
            .map(|o| o.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                FixKind::FpsCap,
                FixKind::Airflow,
                FixKind::FanCurve,
                FixKind::QuietWhenNormal,
                FixKind::WarnLines,
                FixKind::Pause,
                FixKind::Ask,
            ]
        );
    }

    #[test]
    fn a_normal_session_offers_compare_and_settings_but_no_hardware_steps() {
        let history = vec![
            stat(1, "a", 70, 60),
            stat(2, "a", 71, 60),
            stat(3, "a", 70, 61),
        ];
        let s = stat(10, "a", 71, 60);
        let v = verdict(&s, &history, &PcHealthSettings::default());
        assert_eq!(v.tone, VerdictTone::Fine);
        assert_eq!(
            v.line,
            "Normal for this PC: peaks matched your usual in a (GPU 70°C), with no throttling."
        );
        let opts = fix_options(&s, &history, &PcHealthSettings::default());
        assert_eq!(opts[0].kind, FixKind::Compare);
        assert!(opts.iter().all(|o| !matches!(
            o.kind,
            FixKind::Airflow | FixKind::FanCurve | FixKind::Repaste
        )));
    }

    #[test]
    fn repaste_only_after_repeated_hot_runs() {
        let settings = PcHealthSettings::default();
        let hot = |at| stat(at, "a", 86, 60);
        let s = hot(10);
        let few = vec![hot(1), stat(2, "a", 60, 60)];
        assert!(!fix_options(&s, &few, &settings)
            .iter()
            .any(|o| o.kind == FixKind::Repaste));
        let many = vec![hot(1), hot(2), hot(3)];
        assert!(fix_options(&s, &many, &settings)
            .iter()
            .any(|o| o.kind == FixKind::Repaste));
    }

    #[test]
    fn steps_never_say_the_app_changes_hardware() {
        let mut s = stat(10, "a", 95, 98);
        s.throttles = 3;
        s.bound = "gpu".into();
        for o in fix_options(&s, &[stat(1, "a", 90, 90)], &PcHealthSettings::default()) {
            for line in o.steps.iter().chain([&o.why]) {
                let l = line.to_ascii_lowercase();
                assert!(
                    !l.contains("i'll set") && !l.contains("i changed"),
                    "{line}"
                );
            }
        }
    }

    #[test]
    fn quiet_and_pause_decide_whether_a_card_posts() {
        let fine = PulseVerdict {
            tone: VerdictTone::Fine,
            line: String::new(),
        };
        let warm = PulseVerdict {
            tone: VerdictTone::Watch,
            line: String::new(),
        };
        let mut settings = PcHealthSettings::default();
        assert!(should_post(&fine, &settings, 5));
        settings.quiet_when_normal = true;
        assert!(!should_post(&fine, &settings, 5));
        assert!(should_post(&warm, &settings, 5));
        settings.paused_until_ms = 10;
        assert!(!should_post(&warm, &settings, 5));
        assert!(should_post(&warm, &settings, 10));
    }

    #[test]
    fn a_card_finds_its_session_by_the_source_ms() {
        let history = vec![stat(111, "a", 1, 1), stat(222, "b", 2, 2)];
        assert_eq!(card_session("pchealth:b:222", &history).unwrap().game, "b");
        assert!(card_session("pchealth:b:333", &history).is_none());
        assert!(card_session("other:222", &history).is_none());
    }

    #[test]
    fn history_keeps_the_newest_thirty() {
        let mut h = Vec::new();
        for i in 0..35 {
            push_history(&mut h, stat(i, "a", 1, 1));
        }
        assert_eq!(h.len(), HISTORY_CAP);
        assert_eq!(h[0].at_ms, 5);
    }
}
