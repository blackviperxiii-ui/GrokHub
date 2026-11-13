//! Tile words shared by every UI, so the desktop app, the shell, the web and
//! the phone say the same thing about the same facts. Pure: callers read the
//! facts, these only phrase them.

use super::view::{TileRow, TileText, Tone};
use crate::disk::DiskUse;
use crate::game_guard::GameGuard;
use crate::local_profile::Thermal;
use crate::{budget_level, compact_tokens, BudgetLevel};

/// The Small PC tile: the main GPU's °C when the OS reports it, else the
/// PC's thermal reading.
pub fn gpu_temp_text(temp_c: Option<i32>, thermal: Option<Thermal>) -> TileText {
    match (temp_c, thermal.unwrap_or(Thermal::Unknown)) {
        (Some(c), _) => {
            let (word, tone) = if c >= 85 {
                ("hot", Tone::Alert)
            } else if c >= 70 {
                ("warm", Tone::Warn)
            } else {
                ("cool", Tone::Normal)
            };
            TileText::new(format!("{c} °C"), format!("Main GPU · {word}"), tone)
        }
        (None, Thermal::Unknown) => TileText::new("—", "No temperature sensor found", Tone::Quiet),
        (None, th) => {
            let (word, tone) = match th {
                Thermal::Fair => ("Warm", Tone::Warn),
                Thermal::Serious => ("Hot", Tone::Alert),
                Thermal::Critical => ("Critical", Tone::Alert),
                _ => ("Cool", Tone::Normal),
            };
            TileText::new(word, "PC sensors · no GPU sensor", tone)
        }
    }
}

/// This week's model spend, for the usage tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeekSpend {
    pub usd: f64,
    /// Share of the weekly cap, when there is one.
    pub pct: Option<u8>,
}

/// Today's tokens against the daily cap, plus this week's spend and the last
/// day's prompt-cache share when known. A share under `cache_floor_pct` is
/// flagged.
pub fn usage_text(
    used: u64,
    cap: u64,
    week: Option<WeekSpend>,
    week_cap_usd: u32,
    cache_pct: Option<u8>,
    cache_floor_pct: u8,
) -> TileText {
    let caption = if cap == 0 {
        "Tokens today · no daily cap".to_string()
    } else {
        let pct = used.saturating_mul(100) / cap.max(1);
        format!("Tokens today · {pct}% of {}", compact_tokens(cap))
    };
    let tone = match budget_level(used, cap) {
        Some(BudgetLevel::Over) => Tone::Alert,
        Some(BudgetLevel::Near) => Tone::Warn,
        _ if used == 0 => Tone::Quiet,
        _ => Tone::Normal,
    };
    let spend = week.filter(|w| w.usd > 0.0 || w.pct.is_some()).map(|w| {
        if week_cap_usd > 0 {
            format!("${:.2} of ${week_cap_usd} this week", w.usd)
        } else {
            format!("${:.2} this week", w.usd)
        }
    });
    let cache = cache_pct.map(|pct| {
        if pct < cache_floor_pct {
            format!("cache {pct}% (low)")
        } else {
            format!("cache {pct}%")
        }
    });
    let detail = match (spend, cache) {
        (Some(s), Some(c)) => Some(format!("{s} · {c}")),
        (s, c) => s.or(c),
    };
    TileText {
        big: compact_tokens(used),
        caption,
        detail,
        tone,
    }
}

fn gb_bytes(bytes: u64) -> String {
    let gb = bytes as f64 / 1_000_000_000.0;
    if gb >= 1_000.0 {
        format!("{:.1} TB", gb / 1_000.0)
    } else {
        format!("{gb:.0} GB")
    }
}

fn free_tone(d: &DiskUse) -> Tone {
    let pct_free = (d.free * 100).checked_div(d.total).unwrap_or(100);
    if pct_free < 5 {
        Tone::Alert
    } else if pct_free < 15 {
        Tone::Warn
    } else {
        Tone::Normal
    }
}

/// One row per drive, fullest first: label and "120 GB free of 1.0 TB".
pub fn storage_rows(drives: &[DiskUse]) -> Vec<TileRow> {
    let mut ds: Vec<&DiskUse> = drives.iter().filter(|d| d.total > 0).collect();
    ds.sort_by_key(|d| (d.free * 1_000).checked_div(d.total).unwrap_or(1_000));
    ds.iter()
        .map(|d| {
            TileRow::new(
                d.label.clone(),
                format!("{} free of {}", gb_bytes(d.free), gb_bytes(d.total)),
                free_tone(d),
            )
        })
        .collect()
}

/// The Small tile: the fullest drive, or the profile's free space before
/// the first read.
pub fn storage_text(drives: &[DiskUse], fallback_free_mb: Option<u64>) -> TileText {
    let fullest = drives
        .iter()
        .filter(|d| d.total > 0)
        .min_by_key(|d| (d.free * 1_000).checked_div(d.total).unwrap_or(1_000));
    match (fullest, fallback_free_mb) {
        (Some(d), _) => {
            let used = 100 - (d.free * 100).checked_div(d.total).unwrap_or(100);
            let n = drives.iter().filter(|d| d.total > 0).count();
            TileText {
                big: format!("{} free", gb_bytes(d.free)),
                caption: format!("{} · {used}% used of {}", d.label, gb_bytes(d.total)),
                detail: (n > 1).then(|| format!("{n} drives · fullest shown")),
                tone: free_tone(d),
            }
        }
        (None, Some(mb)) if mb > 0 => TileText::new(
            format!("{} free", gb_bytes(mb * 1_048_576)),
            "Home drive",
            Tone::Normal,
        ),
        _ => TileText::new("—", "Reading drives…", Tone::Quiet),
    }
}

/// What the OS package check found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum OsUpdates {
    /// Not read yet.
    #[default]
    Unknown,
    /// System checks are off in Settings, so nothing ran.
    NotAllowed,
    /// No package manager this checks (Windows Update handles itself, or an
    /// unknown distro).
    NoChecker,
    Pending(usize),
}

/// What the Updates & Drivers tile knows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateFacts {
    pub os: OsUpdates,
    /// Installed Steam games with an update waiting, by name.
    pub steam: Vec<String>,
}

/// The Updates & Drivers tile in words: Loadout first, then games, the OS
/// and the GPU driver.
pub fn updates_text(loadout_update: bool, f: &UpdateFacts, gpu_driver: Option<&str>) -> TileText {
    let mut waiting = Vec::new();
    if loadout_update {
        waiting.push("Loadout".to_string());
    }
    match f.steam.len() {
        0 => {}
        1 => waiting.push(f.steam[0].clone()),
        n => waiting.push(format!("{n} Steam games")),
    }
    if let OsUpdates::Pending(n) = f.os {
        if n > 0 {
            waiting.push(format!(
                "{n} system package{}",
                if n == 1 { "" } else { "s" }
            ));
        }
    }
    let big = match waiting.len() {
        0 => "Up to date".to_string(),
        n => format!("{n} waiting"),
    };
    let caption = if waiting.is_empty() {
        match f.os {
            OsUpdates::NotAllowed => "Loadout and Steam checked · system checks off".into(),
            OsUpdates::Unknown | OsUpdates::NoChecker => "Loadout and Steam checked".into(),
            OsUpdates::Pending(_) => "Loadout, Steam and system checked".into(),
        }
    } else {
        waiting.join(" · ")
    };
    let detail = gpu_driver
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| format!("GPU driver {d}"));
    TileText {
        big,
        caption,
        detail,
        tone: if loadout_update || !f.steam.is_empty() {
            Tone::Warn
        } else {
            Tone::Normal
        },
    }
}

/// What the game guard sees, in words. Never reads game data itself.
pub fn game_mode_text(guard: GameGuard, game: Option<&str>, local_paused: bool) -> TileText {
    let playing = guard.gaming || guard.anticheat.is_some();
    let big = match (playing, game) {
        (true, Some(name)) => name.to_string(),
        (true, None) => "Game running".to_string(),
        (false, _) => "No game running".to_string(),
    };
    let caption = match (guard.anticheat, guard.idle_anticheat) {
        (Some(ac), _) => format!("{ac} active · Loadout stays hands-off"),
        (None, Some(ac)) => format!("{ac} installed · idle"),
        (None, None) if playing => "Desktop control paused while you play".to_string(),
        (None, None) => "Watching for games by process name only".to_string(),
    };
    TileText {
        big,
        caption,
        detail: local_paused.then(|| "Local AI paused to keep the GPU free".to_string()),
        tone: if playing { Tone::Warn } else { Tone::Quiet },
    }
}

/// How the internet looks to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetLink {
    Online,
    Spotty,
    Offline,
}

/// What the Connection tile knows besides the link.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SendFacts {
    /// Sends out of this PC in the last 24 h; `None` until the first read.
    pub sends_24h: Option<usize>,
    /// The destination most of those went to.
    pub top_dest: Option<String>,
    /// The local model is on, installed, and allowed to run.
    pub local_ready: bool,
}

/// Online state, local model, and how many sends left this PC in a day.
/// The log counts sends, not bytes, so the tile says sends.
pub fn connection_text(net: NetLink, local_on: bool, facts: &SendFacts) -> TileText {
    let net_word = match net {
        NetLink::Online => "Online",
        NetLink::Spotty => "Spotty",
        NetLink::Offline => "Offline",
    };
    let local = match (local_on, facts.local_ready) {
        (true, true) => "local AI ready",
        (true, false) => "local AI not ready",
        (false, _) => "local AI off",
    };
    let big = match facts.sends_24h {
        Some(1) => "1 send".to_string(),
        Some(n) => format!("{n} sends"),
        None => "—".to_string(),
    };
    let detail = facts
        .top_dest
        .as_ref()
        .filter(|_| facts.sends_24h.unwrap_or(0) > 0)
        .map(|d| format!("Mostly to {d} · last 24 h"));
    TileText {
        big,
        caption: format!("{net_word} · {local}"),
        detail,
        tone: match net {
            NetLink::Online => Tone::Normal,
            NetLink::Spotty => Tone::Warn,
            NetLink::Offline => Tone::Alert,
        },
    }
}

/// Sends in the last day and the destination most of them went to, from
/// the egress log's `(time, destination)` lines.
pub fn count_sends<'a>(
    lines: impl IntoIterator<Item = (u64, &'a str)>,
    now_ms: u64,
) -> (usize, Option<String>) {
    let since = now_ms.saturating_sub(86_400_000);
    let mut counts: Vec<(&str, usize)> = Vec::new();
    let mut total = 0;
    for (_, dest) in lines.into_iter().filter(|(ts, _)| *ts >= since) {
        total += 1;
        match counts.iter_mut().find(|(d, _)| *d == dest) {
            Some((_, n)) => *n += 1,
            None => counts.push((dest, 1)),
        }
    }
    let top = counts
        .iter()
        .max_by_key(|(_, n)| *n)
        .map(|(d, _)| (*d).to_string());
    (total, top)
}

/// What the frame counter last said, for the Frame Rate tile. The counter
/// reads the OS (display flips on Linux, present events on Windows), never
/// the game.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FpsRead {
    /// Opening the counter or waiting for the first second.
    Starting,
    Rate(f32),
    /// The counter needs the one-time setup.
    NeedsSetup,
    /// The OS or driver doesn't report frames.
    NoTrace,
    /// How the last one-tap setup went, while frames still aren't counted.
    Setup(FpsSetup),
}

/// What the Frame Rate tile says after its one-tap setup ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FpsSetup {
    /// Windows: the group change lands at the next sign-in.
    SignOut,
    /// The user closed the password or admin prompt.
    Dismissed,
    /// No prompt can show on this desktop; the steps go in a terminal.
    NoPrompt,
    /// The steps ran and frames still can't be counted.
    Failed,
}

/// The Frame Rate tile in words. `None` where frames can't be read on this
/// OS at all; `presenter` is the Windows app the rate came from.
pub fn frame_rate_text(read: Option<FpsRead>, windows: bool, presenter: Option<&str>) -> TileText {
    let brand = crate::brand::product_name();
    let Some(read) = read else {
        return TileText::new(
            "Not read",
            format!("{brand} never hooks into games"),
            Tone::Quiet,
        )
        .detail("Xbox Game Bar (Win+G) or Steam's in-game counter show FPS safely");
    };
    match read {
        FpsRead::Rate(r) if windows => TileText::new(
            format!("{} FPS", r.round() as u32),
            match presenter {
                Some(p) => format!("From {}", p.strip_suffix(".exe").unwrap_or(p)),
                None => "Nothing is drawing frames".into(),
            },
            if presenter.is_some() {
                Tone::Normal
            } else {
                Tone::Quiet
            },
        )
        .detail("Counted by Windows, never inside the game"),
        FpsRead::Rate(r) => TileText::new(
            format!("{} FPS", r.round() as u32),
            "Frames on screen",
            Tone::Normal,
        )
        .detail("Counted by the kernel, never inside the game"),
        FpsRead::Starting => TileText::new("...", "Counting frames on screen", Tone::Quiet),
        FpsRead::NeedsSetup if windows => {
            TileText::new("Not read", "Needs a one-time admin setup", Tone::Warn)
                .detail("Set up asks Windows for admin once")
        }
        FpsRead::NeedsSetup => TileText::new("Not read", "Needs a one-time setup", Tone::Warn)
            .detail("Set up asks for your password once"),
        FpsRead::Setup(FpsSetup::SignOut) => {
            TileText::new("Almost", "Sign out of Windows to finish", Tone::Warn)
                .detail("Windows adds the permission at your next sign-in")
        }
        FpsRead::Setup(FpsSetup::Dismissed) => {
            TileText::new("Not read", "The password prompt was closed", Tone::Quiet)
                .detail("Nothing changed. Set up again whenever you like")
        }
        FpsRead::Setup(FpsSetup::NoPrompt) => {
            TileText::new("Not read", "No password prompt on this desktop", Tone::Warn)
                .detail("Copy setup and paste it into a terminal")
        }
        FpsRead::Setup(FpsSetup::Failed) => {
            TileText::new("Not read", "The setup didn't finish", Tone::Warn)
                .detail("Copy setup to run each step by hand")
        }
        FpsRead::NoTrace if windows => TileText::new(
            "Not read",
            "Windows didn't start the frame trace",
            Tone::Quiet,
        )
        .detail("Xbox Game Bar (Win+G) or Steam's counter show FPS safely"),
        FpsRead::NoTrace => TileText::new(
            "Not read",
            "This driver doesn't report screen frames",
            Tone::Quiet,
        )
        .detail("Steam's in-game FPS counter shows it safely"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_sends_keeps_the_last_day_and_names_the_busiest_place() {
        let now = 10 * 86_400_000;
        let lines = [
            (now - 86_400_001, "old.example"),
            (now - 5_000, "api.example"),
            (now - 4_000, "cdn.example"),
            (now - 3_000, "api.example"),
        ];
        assert_eq!(count_sends(lines, now), (3, Some("api.example".into())));
        assert_eq!(count_sends([], now), (0, None));
    }
}
