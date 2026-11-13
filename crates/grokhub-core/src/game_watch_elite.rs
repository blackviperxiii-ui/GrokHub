//! Elite Dangerous adapter for the game watcher. The game writes a Player
//! Journal as it runs: one JSON object per line in `Journal.*.log` under the
//! user's Saved Games folder (inside the Proton prefix on Linux), with a new
//! file each session. This follows the newest journal with the same tail as
//! the WoW combat log (only new bytes, from the end of the file that is there
//! at the start) and keeps a session summary that posts to Pulse when the
//! game closes. It only reads the file Frontier documents for third-party
//! tools; nothing reads game memory, hooks or sends the game input, and
//! nothing is uploaded (no EDDN, no Frontier API).
//!
//! Only the events below count. Anything else in the journal, including
//! events a later game version adds, is skipped.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::game_watch::{EventKind, WatchEvent};
use crate::game_watch_wow::CombatLogTail;
use crate::update_feed::{digest_card, UpdateCard};

/// Elite Dangerous on Steam.
pub const APP_ID: u32 = 359320;
pub const NAME: &str = "Elite Dangerous";

/// The journal folder under a Windows user's home folder.
const SAVED_GAMES: [&str; 3] = ["Saved Games", "Frontier Developments", "Elite Dangerous"];

/// Where the journal can be: the Windows user's Saved Games folder, then
/// the Proton prefix in each Steam library under `steam_root`.
pub fn journal_dirs(steam_root: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let saved = |base: PathBuf| SAVED_GAMES.iter().fold(base, |p, part| p.join(part));
    let mut dirs = Vec::new();
    if cfg!(windows) {
        if let Some(home) = home {
            dirs.push(saved(home.to_path_buf()));
        }
    }
    if let Some(root) = steam_root {
        for lib in crate::steam::library_folders(root) {
            let user = lib
                .join("steamapps/compatdata")
                .join(APP_ID.to_string())
                .join("pfx/drive_c/users/steamuser");
            dirs.push(saved(user));
        }
    }
    dirs
}

/// The first journal folder that exists.
pub fn find_journal_dir(steam_root: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    journal_dirs(steam_root, home)
        .into_iter()
        .find(|d| d.is_dir())
}

/// The journal folder on this computer: this user's home folder, and the
/// Steam libraries under `steam_root`.
pub fn find_journal(steam_root: Option<&Path>) -> Option<PathBuf> {
    find_journal_dir(steam_root, dirs::home_dir().as_deref())
}

/// The newest `Journal.*.log` in `dir`, by write time, then by name (the
/// names carry the session's start time).
pub fn newest_journal(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            n.starts_with("journal.") && n.ends_with(".log")
        })
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let name = e.file_name();
            m.is_file().then(|| {
                (
                    m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    name,
                    e.path(),
                )
            })
        })
        .max()
        .map(|(_, _, p)| p)
}

/// One journal event the watcher reads.
#[derive(Debug, Clone, PartialEq)]
pub enum EliteLine {
    /// `Commander` or `LoadGame`: who is playing, and in which ship.
    Commander {
        name: String,
        ship: Option<String>,
    },
    /// `Location` or `CarrierJump`: where the player is, with no jump of
    /// their own.
    At(String),
    /// `FSDJump`: a hyperspace jump, and how far it went.
    Jump {
        system: String,
        ly: f64,
    },
    Docked(String),
    /// `Bounty`: credits earned.
    Bounty(u64),
    /// `MissionCompleted`: credits earned.
    Mission(u64),
    /// `Promotion`: a rank went up.
    Promotion,
    Died,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn credits(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Parse one journal line:
/// `{ "timestamp":"2026-10-31T21:14:03Z", "event":"FSDJump", "StarSystem":"Lave", "JumpDist":8.2 }`
pub fn parse_line(line: &str) -> Option<EliteLine> {
    let v: Value = serde_json::from_str(line).ok()?;
    match v.get("event")?.as_str()? {
        "Commander" => Some(EliteLine::Commander {
            name: text(&v, "Name")?,
            ship: None,
        }),
        "LoadGame" => Some(EliteLine::Commander {
            name: text(&v, "Commander")?,
            ship: text(&v, "Ship_Localised").or_else(|| text(&v, "Ship")),
        }),
        "Location" | "CarrierJump" => text(&v, "StarSystem").map(EliteLine::At),
        "FSDJump" => Some(EliteLine::Jump {
            system: text(&v, "StarSystem")?,
            ly: v
                .get("JumpDist")
                .and_then(Value::as_f64)
                .filter(|d| d.is_finite() && *d >= 0.0)
                .unwrap_or(0.0),
        }),
        "Docked" => text(&v, "StationName").map(EliteLine::Docked),
        // Ship combat gives `TotalReward`; on foot it is `Reward`.
        "Bounty" => Some(EliteLine::Bounty(
            credits(&v, "TotalReward").max(credits(&v, "Reward")),
        )),
        "MissionCompleted" => Some(EliteLine::Mission(credits(&v, "Reward"))),
        "Promotion" => Some(EliteLine::Promotion),
        "Died" => Some(EliteLine::Died),
        _ => None,
    }
}

/// What one play session did, from the journal lines seen since the game
/// started.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EliteSession {
    pub commander: Option<String>,
    pub ship: Option<String>,
    pub system: Option<String>,
    pub station: Option<String>,
    pub jumps: u32,
    pub ly: f64,
    pub dockings: u32,
    pub bounties: u64,
    pub missions: u32,
    pub mission_credits: u64,
    pub promotions: u32,
    pub deaths: u32,
}

impl EliteSession {
    /// Take one line; a death is also a watch event.
    pub fn take(&mut self, line: EliteLine) -> Option<EventKind> {
        match line {
            EliteLine::Commander { name, ship } => {
                self.commander = Some(name);
                if ship.is_some() {
                    self.ship = ship;
                }
            }
            EliteLine::At(system) => self.system = Some(system),
            EliteLine::Jump { system, ly } => {
                self.jumps += 1;
                self.ly += ly;
                self.system = Some(system);
                self.station = None;
            }
            EliteLine::Docked(station) => {
                self.dockings += 1;
                self.station = Some(station);
            }
            EliteLine::Bounty(cr) => self.bounties = self.bounties.saturating_add(cr),
            EliteLine::Mission(cr) => {
                self.missions += 1;
                self.mission_credits = self.mission_credits.saturating_add(cr);
            }
            EliteLine::Promotion => self.promotions += 1,
            EliteLine::Died => {
                self.deaths += 1;
                return Some(EventKind::Death);
            }
        }
        None
    }

    /// Nothing worth a card: no jump, docking, earning, promotion or death.
    pub fn is_empty(&self) -> bool {
        self.jumps == 0
            && self.dockings == 0
            && self.bounties == 0
            && self.missions == 0
            && self.promotions == 0
            && self.deaths == 0
    }

    /// `Krait Mk II. 14 jumps, 182.4 ly, last in Lave. Docked 3 times. 2 missions for 1.2M credits. 340K in bounties. 1 promotion. No deaths.`
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ship) = &self.ship {
            parts.push(ship.clone());
        }
        if self.jumps > 0 {
            let jumps = if self.jumps == 1 { "jump" } else { "jumps" };
            let mut line = format!("{} {jumps}, {:.1} ly", self.jumps, self.ly);
            if let Some(system) = &self.system {
                line.push_str(&format!(", last in {system}"));
            }
            parts.push(line);
        }
        match self.dockings {
            0 => {}
            1 => parts.push("Docked once".into()),
            n => parts.push(format!("Docked {n} times")),
        }
        if self.missions > 0 {
            let missions = if self.missions == 1 {
                "mission"
            } else {
                "missions"
            };
            parts.push(format!(
                "{} {missions} for {} credits",
                self.missions,
                short_credits(self.mission_credits)
            ));
        }
        if self.bounties > 0 {
            parts.push(format!("{} in bounties", short_credits(self.bounties)));
        }
        match self.promotions {
            0 => {}
            1 => parts.push("1 promotion".into()),
            n => parts.push(format!("{n} promotions")),
        }
        parts.push(match self.deaths {
            0 => "No deaths".into(),
            1 => "1 death".into(),
            n => format!("{n} deaths"),
        });
        parts.join(". ") + "."
    }

    /// The commander and the session so far, for the game watcher's row in
    /// Settings.
    pub fn state_line(&self) -> String {
        match &self.commander {
            Some(cmdr) => format!("CMDR {cmdr}: {}", self.summary()),
            None => self.summary(),
        }
    }
}

/// `950`, `340K`, `1.2M`, `3.4B`.
pub fn short_credits(cr: u64) -> String {
    match cr {
        0..=999 => cr.to_string(),
        1_000..=999_999 => format!("{}K", cr / 1_000),
        1_000_000..=999_999_999 => format!("{:.1}M", cr as f64 / 1e6),
        _ => format!("{:.1}B", cr as f64 / 1e9),
    }
}

/// The session as a Pulse card, posted when the game closes. The commander
/// name stays out of it.
pub fn session_card(s: &EliteSession, at_ms: u64) -> UpdateCard {
    let title = match s.jumps {
        0 => format!("{NAME} session"),
        1 => format!("{NAME} session: 1 jump"),
        n => format!("{NAME} session: {n} jumps"),
    };
    digest_card(
        "source:game-watch:elite-dangerous",
        &title,
        &s.summary(),
        at_ms,
    )
}

/// The tail and the session together.
#[derive(Debug)]
pub struct EliteWatcher {
    tail: CombatLogTail,
    session: EliteSession,
}

impl EliteWatcher {
    pub fn new(journal_dir: PathBuf) -> Self {
        Self {
            tail: CombatLogTail::with_picker(journal_dir, newest_journal),
            session: EliteSession::default(),
        }
    }

    pub fn session(&self) -> &EliteSession {
        &self.session
    }

    /// Read the lines added since the last poll into the session; deaths
    /// come back stamped `now_ms`.
    pub fn poll(&mut self, now_ms: u64) -> io::Result<Vec<WatchEvent>> {
        let mut out = Vec::new();
        for line in self.tail.read_lines()? {
            if let Some(kind) = parse_line(&line).and_then(|l| self.session.take(l)) {
                out.push(WatchEvent {
                    at_ms: now_ms,
                    kind,
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Dir(PathBuf);

    impl Dir {
        fn new(label: &str) -> Self {
            let d = std::env::temp_dir()
                .join(format!("grokhub-elitelog-{label}-{}", std::process::id()));
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

    // A made-up commander; the fields follow Frontier's journal manual.
    const LOAD: &str = r#"{ "timestamp":"2026-10-31T21:14:03Z", "event":"LoadGame", "FID":"F0000001", "Commander":"Testpilot", "Ship":"krait_mkii", "Ship_Localised":"Krait Mk II", "GameMode":"Solo", "Credits":1000000 }"#;
    const JUMP: &str = r#"{ "timestamp":"2026-10-31T21:20:00Z", "event":"FSDJump", "StarSystem":"Lave", "JumpDist":8.25, "FuelUsed":1.2 }"#;
    const JUMP2: &str = r#"{ "timestamp":"2026-10-31T21:22:00Z", "event":"FSDJump", "StarSystem":"Leesti", "JumpDist":4.15 }"#;
    const DOCK: &str = r#"{ "timestamp":"2026-10-31T21:25:00Z", "event":"Docked", "StationName":"George Lucas", "StarSystem":"Leesti" }"#;
    const BOUNTY: &str = r#"{ "timestamp":"2026-10-31T21:30:00Z", "event":"Bounty", "TotalReward":340500, "VictimFaction":"Pirates" }"#;
    const MISSION: &str = r#"{ "timestamp":"2026-10-31T21:31:00Z", "event":"MissionCompleted", "Name":"Mission_Courier", "Reward":1250000 }"#;
    const DIED: &str =
        r#"{ "timestamp":"2026-10-31T21:40:00Z", "event":"Died", "KillerName":"Pirate" }"#;

    #[test]
    fn known_events_parse_and_others_do_not() {
        assert_eq!(
            parse_line(LOAD),
            Some(EliteLine::Commander {
                name: "Testpilot".into(),
                ship: Some("Krait Mk II".into())
            })
        );
        assert_eq!(
            parse_line(JUMP),
            Some(EliteLine::Jump {
                system: "Lave".into(),
                ly: 8.25
            })
        );
        assert_eq!(
            parse_line(DOCK),
            Some(EliteLine::Docked("George Lucas".into()))
        );
        assert_eq!(parse_line(BOUNTY), Some(EliteLine::Bounty(340500)));
        assert_eq!(
            parse_line(r#"{"event":"Bounty","Reward":5000}"#),
            Some(EliteLine::Bounty(5000))
        );
        assert_eq!(parse_line(DIED), Some(EliteLine::Died));
        assert_eq!(
            parse_line(r#"{"event":"Location","StarSystem":"Sol","Docked":false}"#),
            Some(EliteLine::At("Sol".into()))
        );
        // Unknown and future events, broken lines and plain text are skipped.
        assert_eq!(parse_line(r#"{"event":"Scan","BodyName":"Lave 1"}"#), None);
        assert_eq!(parse_line(r#"{"event":"SomethingNew2030"}"#), None);
        assert_eq!(parse_line(r#"{"event":"FSDJump","StarSys"#), None);
        assert_eq!(parse_line("Died"), None);
        assert_eq!(parse_line(r#"{"event":"FSDJump"}"#), None);
    }

    #[test]
    fn a_session_adds_up_and_a_death_is_an_event() {
        let mut s = EliteSession::default();
        assert!(s.is_empty());
        for line in [LOAD, JUMP, JUMP2, DOCK, BOUNTY, MISSION] {
            assert_eq!(s.take(parse_line(line).unwrap()), None);
        }
        assert_eq!(s.take(EliteLine::Died), Some(EventKind::Death));
        assert!(!s.is_empty());
        assert_eq!(
            s.summary(),
            "Krait Mk II. 2 jumps, 12.4 ly, last in Leesti. Docked once. 1 mission for 1.2M credits. 340K in bounties. 1 death."
        );
        assert!(s.state_line().starts_with("CMDR Testpilot: Krait Mk II."));
        let card = session_card(&s, 7);
        assert_eq!(card.title, "Elite Dangerous session: 2 jumps");
        assert!(!card.body.as_deref().unwrap_or("").contains("Testpilot"));
    }

    #[test]
    fn loading_in_alone_is_not_a_session() {
        let mut s = EliteSession::default();
        s.take(parse_line(LOAD).unwrap());
        s.take(EliteLine::At("Lave".into()));
        assert!(s.is_empty());
    }

    #[test]
    fn credits_read_short() {
        assert_eq!(short_credits(950), "950");
        assert_eq!(short_credits(340_500), "340K");
        assert_eq!(short_credits(1_250_000), "1.2M");
        assert_eq!(short_credits(3_400_000_000), "3.4B");
    }

    #[test]
    fn the_newest_journal_is_picked() {
        let d = Dir::new("pick");
        assert_eq!(newest_journal(&d.0), None);
        std::fs::write(d.0.join("Status.json"), "{}").unwrap();
        std::fs::write(d.0.join("Journal.2026-10-30T200000.01.log"), "").unwrap();
        std::fs::write(d.0.join("Journal.2026-10-31T211403.01.log"), "").unwrap();
        let newest = newest_journal(&d.0).unwrap();
        assert_eq!(
            newest.file_name().unwrap(),
            "Journal.2026-10-31T211403.01.log"
        );
    }

    #[test]
    fn the_proton_journal_folder_is_found_in_the_steam_library() {
        let d = Dir::new("proton");
        let steam = d.0.join("steam");
        let journal = steam.join(
            "steamapps/compatdata/359320/pfx/drive_c/users/steamuser/Saved Games/Frontier Developments/Elite Dangerous",
        );
        assert_eq!(find_journal_dir(Some(&steam), None), None);
        std::fs::create_dir_all(&journal).unwrap();
        assert_eq!(find_journal_dir(Some(&steam), None), Some(journal));
    }

    #[test]
    fn the_watcher_reads_only_new_lines_and_holds_a_half_written_one() {
        let d = Dir::new("watch");
        let log = d.0.join("Journal.2026-10-31T211403.01.log");
        // What is there when the watcher starts is an old session.
        std::fs::write(&log, format!("{DIED}\n")).unwrap();
        let mut w = EliteWatcher::new(d.0.clone());
        assert!(w.poll(1).unwrap().is_empty());
        assert!(w.session().is_empty());

        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        write!(f, "{LOAD}\n{JUMP}\n{}", &DIED[..20]).unwrap();
        assert!(w.poll(2).unwrap().is_empty());
        assert_eq!(w.session().jumps, 1);
        assert_eq!(w.session().deaths, 0);

        writeln!(f, "{}", &DIED[20..]).unwrap();
        let events = w.poll(3).unwrap();
        assert_eq!(
            events,
            vec![WatchEvent {
                at_ms: 3,
                kind: EventKind::Death
            }]
        );
        assert_eq!(w.session().deaths, 1);
    }

    #[test]
    fn a_new_session_file_is_read_from_its_start() {
        let d = Dir::new("next");
        std::fs::write(
            d.0.join("Journal.2026-10-30T200000.01.log"),
            format!("{JUMP}\n"),
        )
        .unwrap();
        let mut w = EliteWatcher::new(d.0.clone());
        w.poll(1).unwrap();
        assert_eq!(w.session().jumps, 0);
        let next = d.0.join("Journal.2026-10-31T211403.01.log");
        std::fs::write(&next, format!("{LOAD}\n{JUMP2}\n")).unwrap();
        // Make sure the new file is the newest even on a coarse clock.
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&next)
            .unwrap()
            .set_modified(later)
            .unwrap();
        w.poll(2).unwrap();
        assert_eq!(w.session().jumps, 1);
        assert_eq!(w.session().system.as_deref(), Some("Leesti"));
    }
}
