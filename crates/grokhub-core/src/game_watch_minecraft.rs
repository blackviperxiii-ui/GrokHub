//! Minecraft: Java Edition adapter for the game watcher. The game writes
//! `logs/latest.log` in its game folder as it runs, and starts a new one at
//! each launch. This follows that file in the vanilla launcher's folder and
//! in each Prism Launcher or MultiMC instance, with the same tail as the WoW
//! combat log, and keeps a session per folder: the player's deaths, the
//! advancements they made and the world they played. A session ends when
//! the game logs `Stopping!` as it closes, and then posts to Pulse. It only
//! reads the file the game writes; no mod, no process handle, no input.
//!
//! The player is the account the game names in `Setting user: …`. Lines
//! count only when the game's own server thread writes them, or when they
//! reach the client as a system chat line (`[CHAT]` with no `<name>`), so
//! what another player types can't pass for a death or an advancement.

use std::path::{Path, PathBuf};

use crate::game_watch_wow::CombatLogTail;
use crate::update_feed::{digest_card, UpdateCard};

pub const NAME: &str = "Minecraft: Java Edition";

/// `latest.log` in `dir`, when it is there.
pub fn latest_log(dir: &Path) -> Option<PathBuf> {
    let p = dir.join("latest.log");
    p.is_file().then_some(p)
}

/// The `logs` folders Minecraft may write to under `home`: the vanilla
/// launcher's, then each launcher instance's. Only folders that exist.
pub fn log_dirs(home: &Path) -> Vec<PathBuf> {
    let mut games = vec![
        home.join(".minecraft"),
        home.join("AppData/Roaming/.minecraft"),
        home.join("Library/Application Support/minecraft"),
    ];
    if let Some(appdata) = std::env::var_os("APPDATA") {
        games.push(PathBuf::from(appdata).join(".minecraft"));
    }
    for launcher in [
        ".local/share/PrismLauncher/instances",
        ".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/instances",
        ".local/share/multimc/instances",
        "AppData/Roaming/PrismLauncher/instances",
    ] {
        let Ok(rd) = std::fs::read_dir(home.join(launcher)) else {
            continue;
        };
        for entry in rd.flatten() {
            for sub in [".minecraft", "minecraft"] {
                games.push(entry.path().join(sub));
            }
        }
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in games.into_iter().map(|g| g.join("logs")) {
        if dir.is_dir() && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// One line the watcher reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McLine {
    /// `Setting user: Name`: the account playing.
    User(String),
    /// `Preparing level "World"`: a single-player world opened.
    World(String),
    /// A system message, without its timestamp and thread.
    System(String),
    /// The game is closing.
    Stopping,
}

/// Parse one `latest.log` line: `[21:14:03] [Server thread/INFO]: …`.
pub fn parse_line(line: &str) -> Option<McLine> {
    let rest = line.strip_prefix('[')?.split_once("] [")?.1;
    let (thread, msg) = rest.split_once("]: ")?;
    let msg = msg.trim_end();
    let thread = thread.split('/').next()?;
    if let Some(user) = msg.strip_prefix("Setting user: ") {
        return Some(McLine::User(user.to_owned()));
    }
    if msg == "Stopping!" {
        return Some(McLine::Stopping);
    }
    if thread == "Server thread" {
        if let Some(world) = msg
            .strip_prefix("Preparing level \"")
            .and_then(|w| w.strip_suffix('"'))
        {
            return Some(McLine::World(world.to_owned()));
        }
        return Some(McLine::System(msg.to_owned()));
    }
    // A chat line a player sent starts with `<name>`; system lines don't.
    let chat = msg.strip_prefix("[CHAT] ")?;
    (!chat.starts_with('<') && !chat.starts_with('[')).then(|| McLine::System(chat.to_owned()))
}

/// How Minecraft's death messages go on after the player's name.
const DEATH_STARTS: &[&str] = &[
    "was ",
    "fell ",
    "drowned",
    "died",
    "blew up",
    "burned",
    "went up in flames",
    "walked into",
    "hit the ground",
    "experienced kinetic energy",
    "froze to death",
    "starved to death",
    "suffocated",
    "tried to swim in lava",
    "withered away",
    "discovered the floor was lava",
    "left the confines of this world",
    "didn't want to live",
    "went off with a bang",
    "fell out of the world",
];

const ADVANCEMENT: &[&str] = &[
    "has made the advancement [",
    "has completed the challenge [",
    "has reached the goal [",
];

/// One play of the game, from launch to close.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McSession {
    pub player: Option<String>,
    pub world: Option<String>,
    pub deaths: u32,
    pub advancements: Vec<String>,
}

impl McSession {
    pub fn take(&mut self, line: McLine) {
        match line {
            McLine::User(name) => self.player = Some(name),
            McLine::World(world) => self.world = Some(world),
            McLine::Stopping => {}
            McLine::System(msg) => {
                let Some(me) = self.player.as_deref() else {
                    return;
                };
                let Some(rest) = msg.strip_prefix(me).and_then(|r| r.strip_prefix(' ')) else {
                    return;
                };
                if let Some(name) = ADVANCEMENT
                    .iter()
                    .find_map(|a| rest.strip_prefix(a))
                    .and_then(|r| r.strip_suffix(']'))
                {
                    if !self.advancements.iter().any(|a| a == name) {
                        self.advancements.push(name.to_owned());
                    }
                } else if DEATH_STARTS.iter().any(|d| rest.starts_with(d)) {
                    self.deaths += 1;
                }
            }
        }
    }

    /// Nothing worth a card.
    pub fn is_empty(&self) -> bool {
        self.deaths == 0 && self.advancements.is_empty()
    }

    /// `Testcrafter in New World. 2 deaths. Advancements: Stone Age, Acquire Hardware.`
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        let mut who = self.player.clone().unwrap_or_default();
        if let Some(world) = &self.world {
            if who.is_empty() {
                who = format!("World: {world}");
            } else {
                who.push_str(&format!(" in {world}"));
            }
        }
        if !who.is_empty() {
            parts.push(who);
        }
        parts.push(match self.deaths {
            0 => "No deaths".to_owned(),
            1 => "1 death".to_owned(),
            n => format!("{n} deaths"),
        });
        if !self.advancements.is_empty() {
            parts.push(format!("Advancements: {}", self.advancements.join(", ")));
        }
        parts.join(". ") + "."
    }
}

/// The session as a Pulse card, posted when the game closes.
pub fn session_card(s: &McSession, at_ms: u64) -> UpdateCard {
    let title = match s.advancements.len() {
        0 => format!("{NAME} session"),
        1 => format!("{NAME} session: 1 advancement"),
        n => format!("{NAME} session: {n} advancements"),
    };
    digest_card("source:game-watch:minecraft", &title, &s.summary(), at_ms)
}

/// One game folder's log and its session.
#[derive(Debug)]
struct Follow {
    tail: CombatLogTail,
    session: McSession,
}

/// Follows every Minecraft log folder at once.
#[derive(Debug)]
pub struct McWatcher {
    follows: Vec<Follow>,
}

impl McWatcher {
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self {
            follows: dirs
                .into_iter()
                .map(|dir| Follow {
                    tail: CombatLogTail::with_picker(dir, latest_log),
                    session: McSession::default(),
                })
                .collect(),
        }
    }

    /// Read new lines; the sessions that ended (the game logged
    /// `Stopping!`) come back, and their folders start fresh.
    pub fn poll(&mut self) -> Vec<McSession> {
        let mut ended = Vec::new();
        for f in &mut self.follows {
            // A log that can't be read now is tried again next poll.
            for line in f.tail.read_lines().unwrap_or_default() {
                match parse_line(&line) {
                    Some(McLine::Stopping) => ended.push(std::mem::take(&mut f.session)),
                    Some(l) => f.session.take(l),
                    None => {}
                }
            }
        }
        ended.retain(|s| !s.is_empty());
        ended
    }

    /// The session with the most in it so far, for Settings.
    pub fn current(&self) -> Option<&McSession> {
        self.follows
            .iter()
            .map(|f| &f.session)
            .filter(|s| !s.is_empty())
            .max_by_key(|s| s.deaths as usize + s.advancements.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Dir(PathBuf);

    impl Dir {
        fn new(label: &str) -> Self {
            let d =
                std::env::temp_dir().join(format!("grokhub-mclog-{label}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Self(d)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn server(msg: &str) -> String {
        format!("[21:14:03] [Server thread/INFO]: {msg}")
    }

    #[test]
    fn system_lines_parse_and_player_chat_does_not() {
        assert_eq!(
            parse_line("[21:14:00] [Render thread/INFO]: Setting user: Testcrafter"),
            Some(McLine::User("Testcrafter".into()))
        );
        assert_eq!(
            parse_line(&server("Preparing level \"New World\"")),
            Some(McLine::World("New World".into()))
        );
        assert_eq!(
            parse_line("[21:14:03] [Render thread/INFO]: [CHAT] Testcrafter was slain by Zombie"),
            Some(McLine::System("Testcrafter was slain by Zombie".into()))
        );
        assert_eq!(
            parse_line("[21:14:03] [Render thread/INFO]: [CHAT] <Testtroll> Testcrafter was slain by Zombie"),
            None
        );
        assert_eq!(
            parse_line("[21:20:00] [Render thread/INFO]: Stopping!"),
            Some(McLine::Stopping)
        );
        assert_eq!(parse_line("not a log line"), None);
    }

    #[test]
    fn only_the_players_own_deaths_and_advancements_count() {
        let mut s = McSession::default();
        for l in [
            "[21:14:00] [Render thread/INFO]: Setting user: Testcrafter".to_owned(),
            server("Preparing level \"New World\""),
            server("Testcrafter has made the advancement [Stone Age]"),
            server("Testcrafter has made the advancement [Stone Age]"),
            server("Testfriend fell from a high place"),
            server("Testcrafter fell from a high place"),
            server("Testcrafter joined the game"),
            server("Testcrafter has reached the goal [Acquire Hardware]"),
            server("Testcrafter was slain by Zombie"),
        ] {
            if let Some(line) = parse_line(&l) {
                s.take(line);
            }
        }
        assert_eq!(s.deaths, 2);
        assert_eq!(s.advancements, vec!["Stone Age", "Acquire Hardware"]);
        assert_eq!(
            s.summary(),
            "Testcrafter in New World. 2 deaths. Advancements: Stone Age, Acquire Hardware."
        );
        let card = session_card(&s, 5);
        assert_eq!(
            card.title,
            "Minecraft: Java Edition session: 2 advancements"
        );
        assert!(crate::update_feed::names_its_item(&card));
    }

    #[test]
    fn a_session_ends_when_the_game_stops_and_a_relaunch_starts_fresh() {
        let dir = Dir::new("tail");
        let log = dir.0.join("latest.log");
        std::fs::write(&log, server("Oldname was slain by Skeleton") + "\n").unwrap();
        let mut w = McWatcher::new(vec![dir.0.clone()]);
        assert!(w.poll().is_empty());
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        for l in [
            "[21:14:00] [Render thread/INFO]: Setting user: Testcrafter".to_owned(),
            server("Testcrafter drowned"),
        ] {
            writeln!(f, "{l}").unwrap();
        }
        assert!(w.poll().is_empty());
        assert_eq!(w.current().map(|s| s.deaths), Some(1));
        writeln!(f, "[21:20:00] [Render thread/INFO]: Stopping!").unwrap();
        let ended = w.poll();
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].player.as_deref(), Some("Testcrafter"));
        assert_eq!(w.current(), None);
    }

    #[test]
    fn log_folders_are_found_for_the_launcher_and_instances() {
        let home = Dir::new("home");
        std::fs::create_dir_all(home.0.join(".minecraft/logs")).unwrap();
        std::fs::create_dir_all(
            home.0
                .join(".local/share/PrismLauncher/instances/Modded/minecraft/logs"),
        )
        .unwrap();
        let dirs = log_dirs(&home.0);
        assert!(dirs.contains(&home.0.join(".minecraft/logs")));
        assert!(dirs.contains(
            &home
                .0
                .join(".local/share/PrismLauncher/instances/Modded/minecraft/logs")
        ));
    }
}
