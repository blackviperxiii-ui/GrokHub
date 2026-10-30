//! Game guard: the app never gets a PC flagged by a game's anti-cheat.
//!
//! The readers (`grokhub-desktop::game_guard`) look only at things that open
//! no handle to another process: the Toolhelp process-name snapshot and the
//! Service Control Manager on Windows, `/proc/*/comm` on Linux, Steam's own
//! `RunningAppID`, and the launcher manifests on disk. This module holds the
//! pure parts: which names mean which anti-cheat, whether those signals mean
//! the user is gaming, and what a paused action tells the agent and the user.
//!
//! Rules:
//! - An anti-cheat running counts as gaming, and also pauses window checks
//!   and user-started screen recording.
//! - Gaming (an anti-cheat, a Steam game, a known game process, an app on the
//!   governor's deny list, or an exclusive fullscreen Direct3D app) pauses
//!   all desktop control: input, window move/resize/focus, opening apps, and
//!   the screen capture desktop control and the agent's eyes use.
//!
//! Call of Duty's Ricochet has no stable, public process or service name, so
//! it is not listed: the game itself is caught as a Steam game or a game
//! process instead.

/// One anti-cheat and the names it runs under. Names are lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AntiCheat {
    pub name: &'static str,
    /// Executable file names.
    pub exes: &'static [&'static str],
    /// Service and kernel driver names (Service Control Manager).
    pub services: &'static [&'static str],
}

/// Known anti-cheats, matched case-insensitively.
pub const ANTI_CHEATS: &[AntiCheat] = &[
    AntiCheat {
        name: "Easy Anti-Cheat",
        exes: &["easyanticheat.exe", "easyanticheat_eos.exe"],
        services: &["easyanticheat", "easyanticheat_eos"],
    },
    AntiCheat {
        name: "BattlEye",
        exes: &["beservice.exe", "beservice_x64.exe"],
        services: &["beservice", "bedaisy"],
    },
    AntiCheat {
        name: "Vanguard",
        exes: &["vgc.exe", "vgtray.exe"],
        services: &["vgc", "vgk"],
    },
    AntiCheat {
        name: "FACEIT",
        exes: &["faceit.exe", "faceitclient.exe"],
        services: &["faceit"],
    },
    AntiCheat {
        name: "ESEA",
        exes: &[],
        services: &["eseadriver2"],
    },
    AntiCheat {
        name: "PunkBuster",
        exes: &["pnkbstra.exe", "pnkbstrb.exe"],
        services: &["pnkbstra", "pnkbstrb"],
    },
    AntiCheat {
        name: "nProtect GameGuard",
        exes: &["gamemon.exe", "gamemon64.exe"],
        services: &[],
    },
    AntiCheat {
        name: "XIGNCODE3",
        exes: &["xxd-0.xem"],
        services: &["xhunter1"],
    },
    AntiCheat {
        name: "EA Javelin",
        exes: &["eaanticheat.gameservice.exe"],
        services: &["eaanticheatservice"],
    },
];

/// The first anti-cheat whose executable or service is in these lists.
/// `exes` are file names (a path keeps only its last part); `services` are
/// running service or driver names. Any case.
pub fn detect_anticheat<E: AsRef<str>, S: AsRef<str>>(
    exes: &[E],
    services: &[S],
) -> Option<&'static str> {
    let exes: Vec<String> = exes.iter().map(|e| file_name_lower(e.as_ref())).collect();
    let services: Vec<String> = services
        .iter()
        .map(|s| s.as_ref().trim().to_ascii_lowercase())
        .collect();
    ANTI_CHEATS
        .iter()
        .find(|ac| {
            ac.exes.iter().any(|n| exes.iter().any(|e| e == n))
                || ac.services.iter().any(|n| services.iter().any(|s| s == n))
        })
        .map(|ac| ac.name)
}

/// The last part of a Windows or Unix path, trimmed and lowercased.
pub fn file_name_lower(path: &str) -> String {
    path.trim()
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Windows: the running game's exe file name (lowercase), from process file
/// names only. A game is an Unreal `-Shipping` build or an exe a launcher's
/// manifest names (`known`, see [`epic_manifest_exe`]). Without full paths,
/// a game outside these and outside Steam (`RunningAppID`) isn't seen; the
/// governor's deny list covers those.
pub fn windows_game_exe<N: AsRef<str>>(names: &[N], known: &[String]) -> Option<String> {
    names.iter().find_map(|n| {
        let name = file_name_lower(n.as_ref());
        let stem = name.strip_suffix(".exe")?;
        let shipping = ["-win64-shipping", "-win32-shipping", "-wingdk-shipping"]
            .iter()
            .any(|s| stem.len() > s.len() && stem.ends_with(s));
        let listed = known.iter().any(|k| k.eq_ignore_ascii_case(&name));
        (shipping || listed).then_some(name)
    })
}

/// A game label from [`windows_game_exe`]: the exe name without `.exe`.
pub fn game_label(exe: &str) -> String {
    let name = file_name_lower(exe);
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

/// The game exe an Epic Games launcher manifest (`*.item`) starts, lowercase.
/// `None` for engines and tools (no `games` category) or a broken file.
pub fn epic_manifest_exe(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if let Some(cats) = v.get("AppCategories").and_then(|c| c.as_array()) {
        let game = cats
            .iter()
            .filter_map(|c| c.as_str())
            .any(|c| c.eq_ignore_ascii_case("games"));
        if !game {
            return None;
        }
    }
    let exe = file_name_lower(v.get("LaunchExecutable")?.as_str()?);
    exe.ends_with(".exe").then_some(exe)
}

/// What the readers saw. Unknown readers leave their field at the default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardSignals {
    pub anticheat: Option<&'static str>,
    /// Steam's `RunningAppID` (Windows) or the `SteamLaunch AppId=` wrapper (Linux).
    pub steam_app: Option<u32>,
    /// A known game process (Unreal shipping build, launcher manifest exe,
    /// gamescope, Proton, Wine).
    pub game_process: bool,
    /// A running app on the governor's deny list ("always counts as a game").
    pub listed_game: bool,
    /// Windows says an exclusive fullscreen Direct3D app owns the screen.
    pub exclusive_fullscreen: bool,
}

/// The one state the app consults before anything an anti-cheat could see.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GameGuard {
    /// The anti-cheat running now.
    pub anticheat: Option<&'static str>,
    /// The user is gaming. Always true while an anti-cheat runs.
    pub gaming: bool,
}

/// Turn the readers' signals into the guard.
pub fn decide(s: &GuardSignals) -> GameGuard {
    GameGuard {
        anticheat: s.anticheat,
        gaming: s.anticheat.is_some()
            || s.steam_app.is_some_and(|id| id != 0)
            || s.game_process
            || s.listed_game
            || s.exclusive_fullscreen,
    }
}

/// What the guard is asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guarded {
    /// Input, window move/resize/focus, opening apps (desktop tools and COMPUTER_CMD).
    DesktopControl,
    /// Screenshots for desktop control and the agent's eyes.
    ScreenCapture,
    /// "Record my screen", which the user starts. Only an anti-cheat pauses it.
    ScreenRecording,
    /// Reading other apps' window titles. Only an anti-cheat pauses it.
    WindowCheck,
}

impl GameGuard {
    /// Nothing running that the guard cares about.
    pub fn is_clear(&self) -> bool {
        self.anticheat.is_none() && !self.gaming
    }

    /// `Some(message)` when this action must not run now. The message goes
    /// back to the agent or the user as the action's error.
    pub fn blocks(&self, what: Guarded) -> Option<String> {
        let subject = match what {
            Guarded::DesktopControl => "Desktop control",
            Guarded::ScreenCapture => "Screen capture",
            Guarded::ScreenRecording => "Screen recording",
            Guarded::WindowCheck => "Reading other windows",
        };
        if let Some(ac) = self.anticheat {
            return Some(format!(
                "{subject} is blocked while {ac} is running so it doesn't flag this PC."
            ));
        }
        let gaming_pauses = matches!(what, Guarded::DesktopControl | Guarded::ScreenCapture);
        (self.gaming && gaming_pauses).then(|| {
            format!(
                "{subject} is paused while you're gaming so the game's anti-cheat doesn't flag this PC."
            )
        })
    }

    /// One status line for Settings, or `None` when nothing is paused.
    pub fn status_line(&self) -> Option<String> {
        if let Some(ac) = self.anticheat {
            return Some(format!(
                "Anti-cheat running ({ac}): desktop control, screen capture and window checks paused"
            ));
        }
        self.gaming
            .then(|| "Game mode: desktop control paused".to_string())
    }
}

/// Desktop tools the guard pauses while gaming. File tools (`delete_files`,
/// `watch_*`) and `list_monitors` touch no other app and stay on.
pub fn desktop_tool_guarded(name: &str) -> bool {
    matches!(
        name,
        "screenshot"
            | "click"
            | "move"
            | "drag"
            | "scroll"
            | "type"
            | "key"
            | "open_app"
            | "focus_window"
            | "get_window_geometry"
            | "set_window_geometry"
    )
}

#[cfg(test)]
#[path = "game_guard_tests.rs"]
mod tests;
