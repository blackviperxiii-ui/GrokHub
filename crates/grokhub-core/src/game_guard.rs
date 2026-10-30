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
//! - An always-on anti-cheat (Riot Vanguard: a boot-start driver and a
//!   service that run from install on) counts as running only while one of
//!   the games it protects runs. Installed with no such game open, it pauses
//!   nothing; Settings only notes that it is there and always running.
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
    /// Set when the anti-cheat runs all the time once installed, so its own
    /// names only say it is installed. `None`: running means a game is on.
    pub always_on: Option<AlwaysOn>,
}

/// An anti-cheat that stays running from install on, with or without a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlwaysOn {
    /// Exe file names of the games it protects (lowercase). One of these
    /// running is what makes the anti-cheat count as running.
    pub games: &'static [&'static str],
    /// The Settings line while it is installed and no protected game runs.
    pub idle_note: &'static str,
}

/// The note Settings shows while Riot Vanguard is installed and idle.
pub const VANGUARD_IDLE_NOTE: &str = "Riot Vanguard is installed. It runs all the time, even with no Riot game open; desktop control pauses only while a Riot game runs.";

/// Known anti-cheats, matched case-insensitively.
pub const ANTI_CHEATS: &[AntiCheat] = &[
    AntiCheat {
        name: "Easy Anti-Cheat",
        exes: &["easyanticheat.exe", "easyanticheat_eos.exe"],
        services: &["easyanticheat", "easyanticheat_eos"],
        always_on: None,
    },
    AntiCheat {
        name: "BattlEye",
        exes: &["beservice.exe", "beservice_x64.exe"],
        services: &["beservice", "bedaisy"],
        always_on: None,
    },
    AntiCheat {
        name: "Vanguard",
        exes: &["vgc.exe", "vgtray.exe"],
        services: &["vgc", "vgk"],
        // vgk is a boot-start kernel driver and vgc a service: both run from
        // install on. The protected processes are the games themselves, not
        // the Riot Client or League's lobby client (RiotClientServices.exe,
        // RiotClientUx.exe, Riot Client.exe, LeagueClient.exe,
        // LeagueClientUx.exe), which run without Vanguard's game protection.
        always_on: Some(AlwaysOn {
            games: &[
                // VALORANT: the bootstrap and the Unreal game it starts.
                "valorant.exe",
                "valorant-win64-shipping.exe",
                // League of Legends and Teamfight Tactics (one game client).
                "league of legends.exe",
                // 2XKO (Unreal project "Lion"): the bootstrap and the game.
                "2xko.exe",
                "lion-win64-shipping.exe",
            ],
            idle_note: VANGUARD_IDLE_NOTE,
        }),
    },
    AntiCheat {
        name: "FACEIT",
        exes: &["faceit.exe", "faceitclient.exe"],
        services: &["faceit"],
        always_on: None,
    },
    AntiCheat {
        name: "ESEA",
        exes: &[],
        services: &["eseadriver2"],
        always_on: None,
    },
    AntiCheat {
        name: "PunkBuster",
        exes: &["pnkbstra.exe", "pnkbstrb.exe"],
        services: &["pnkbstra", "pnkbstrb"],
        always_on: None,
    },
    AntiCheat {
        name: "nProtect GameGuard",
        exes: &["gamemon.exe", "gamemon64.exe"],
        services: &[],
        always_on: None,
    },
    AntiCheat {
        name: "XIGNCODE3",
        exes: &["xxd-0.xem"],
        services: &["xhunter1"],
        always_on: None,
    },
    AntiCheat {
        name: "EA Javelin",
        exes: &["eaanticheat.gameservice.exe"],
        services: &["eaanticheatservice"],
        always_on: None,
    },
];

/// What the anti-cheat names in a process and service list mean.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AntiCheatScan {
    /// The anti-cheat that counts as running: one whose exe or service is
    /// listed, or, for an always-on one, one of its protected games.
    pub running: Option<&'static str>,
    /// An always-on anti-cheat that is installed and running but protects
    /// no game now. It pauses nothing.
    pub idle: Option<&'static str>,
}

/// Read the anti-cheats from these lists. `exes` are file names (a path
/// keeps only its last part); `services` are running service or driver
/// names. Any case. The first match in [`ANTI_CHEATS`] wins each field.
pub fn scan_anticheats<E: AsRef<str>, S: AsRef<str>>(exes: &[E], services: &[S]) -> AntiCheatScan {
    let exes: Vec<String> = exes.iter().map(|e| file_name_lower(e.as_ref())).collect();
    let services: Vec<String> = services
        .iter()
        .map(|s| s.as_ref().trim().to_ascii_lowercase())
        .collect();
    let has_exe = |names: &[&str]| names.iter().any(|n| exes.iter().any(|e| e == n));
    let own_names = |ac: &AntiCheat| {
        has_exe(ac.exes) || ac.services.iter().any(|n| services.iter().any(|s| s == n))
    };
    let running = ANTI_CHEATS
        .iter()
        .find(|ac| match ac.always_on {
            None => own_names(ac),
            Some(on) => has_exe(on.games),
        })
        .map(|ac| ac.name);
    let idle = ANTI_CHEATS
        .iter()
        .find(|ac| ac.always_on.is_some_and(|on| !has_exe(on.games)) && own_names(ac))
        .map(|ac| ac.name);
    AntiCheatScan { running, idle }
}

/// The anti-cheat that counts as running in these lists (see
/// [`scan_anticheats`]). An always-on one only counts while its game runs.
pub fn detect_anticheat<E: AsRef<str>, S: AsRef<str>>(
    exes: &[E],
    services: &[S],
) -> Option<&'static str> {
    scan_anticheats(exes, services).running
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
    /// An always-on anti-cheat installed with none of its games running.
    pub idle_anticheat: Option<&'static str>,
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
    /// An always-on anti-cheat that is installed but protects no running
    /// game. It pauses nothing; Settings notes it ([`GameGuard::idle_note`]).
    pub idle_anticheat: Option<&'static str>,
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
        idle_anticheat: s.idle_anticheat,
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

    /// The note for an always-on anti-cheat that is installed but idle, or
    /// `None`. It pauses nothing; it only tells the user it is always on.
    pub fn idle_note(&self) -> Option<&'static str> {
        let name = self.idle_anticheat?;
        ANTI_CHEATS
            .iter()
            .find(|ac| ac.name == name)
            .and_then(|ac| ac.always_on)
            .map(|on| on.idle_note)
    }

    /// The guard's lines for the Settings note under the desktop switch:
    /// what is paused, then any idle always-on anti-cheat. `None` when both
    /// are empty.
    pub fn panel_note(&self) -> Option<String> {
        let lines: Vec<String> = self
            .status_line()
            .into_iter()
            .chain(self.idle_note().map(str::to_string))
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
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
