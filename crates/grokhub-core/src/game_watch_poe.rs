//! Path of Exile adapter for the game watcher. Path of Exile and Path of
//! Exile 2 both write `logs/Client.txt` in the game folder as they run: the
//! areas the player enters, level-ups, deaths, and who joins the area. This
//! follows that file with the same tail as the WoW combat log (only new
//! bytes, from the end of the file that is there at the start) and keeps a
//! session summary that posts to Pulse when the game closes. It only reads
//! the file the game writes; nothing reads game memory, hooks or sends the
//! game input.
//!
//! Only system lines count. In `Client.txt` those follow the log tag with
//! `] : `, while chat lines carry a name there (`] Name: text`), so nothing
//! typed in chat can pass for a death or a level-up.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::game_watch::{EventKind, WatchEvent};
use crate::game_watch_wow::CombatLogTail;
use crate::update_feed::{digest_card, UpdateCard};

/// The two games, both on Steam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoeGame {
    Poe1,
    Poe2,
}

impl PoeGame {
    pub const ALL: [PoeGame; 2] = [PoeGame::Poe1, PoeGame::Poe2];

    pub fn app_id(self) -> u32 {
        match self {
            PoeGame::Poe1 => 238960,
            PoeGame::Poe2 => 2694490,
        }
    }

    pub fn from_app_id(id: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.app_id() == id)
    }

    /// The game's name, which is also its folder under `steamapps/common`.
    pub fn name(self) -> &'static str {
        match self {
            PoeGame::Poe1 => "Path of Exile",
            PoeGame::Poe2 => "Path of Exile 2",
        }
    }
}

/// The game's log folder under its install folder.
pub fn logs_dir(install: &Path) -> PathBuf {
    install.join("logs")
}

/// The game's install folder in any Steam library under `steam_root`.
pub fn find_install(steam_root: &Path, game: PoeGame) -> Option<PathBuf> {
    crate::steam::library_folders(steam_root)
        .into_iter()
        .map(|lib| lib.join("steamapps").join("common").join(game.name()))
        .find(|dir| dir.is_dir())
}

/// `Client.txt`, or `LatestClient.txt` where a build writes that, whichever
/// was written last.
pub fn newest_client_log(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            n == "client.txt" || n == "latestclient.txt"
        })
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file()
                .then(|| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), e.path()))
        })
        .max()
        .map(|(_, p)| p)
}

/// One system line the watcher reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoeLine {
    Entered(String),
    Level {
        name: String,
        class: String,
        level: u32,
    },
    Slain(String),
    /// Someone else came into the player's area (a party member).
    Joined(String),
    /// The game built a new area instance: its level, its internal id
    /// (maps start with `Map`) and its seed. The `Entered` line follows.
    Generated {
        level: u32,
        id: String,
        seed: u64,
    },
}

/// Seconds of a line's `2026/10/31 21:14:03` stamp, counted from a fixed
/// day: only differences between lines are used, so the clock's zone
/// doesn't matter.
pub fn line_secs(line: &str) -> Option<i64> {
    let stamp = line.get(..19)?;
    let (date, time) = stamp.split_once(' ')?;
    let mut d = date.split('/').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|p| p.parse::<i64>().ok());
    let (h, min, sec) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || min > 59 || sec > 60 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe;
    Some(days * 86_400 + h * 3_600 + min * 60 + sec)
}

/// A map instance, not a town, hideout or campaign zone.
fn is_map(id: &str) -> bool {
    id.starts_with("Map") && !id.contains("Hideout")
}

/// A character name: one word, no spaces.
fn name(s: &str) -> Option<String> {
    (!s.is_empty() && !s.contains(char::is_whitespace)).then(|| s.to_owned())
}

/// Parse one `Client.txt` line:
/// `2026/10/31 21:14:03 123456 cff94598 [INFO Client 1234] : You have entered Lioneye's Watch.`
pub fn parse_line(line: &str) -> Option<PoeLine> {
    let tag = line.find(" [")?;
    let (_, rest) = line[tag + 2..].split_once("] ")?;
    let rest = rest.trim_end();
    // Both games name the scene they load: `[SCENE] Set Source [Clearfell]`.
    if let Some(area) = rest.strip_prefix("[SCENE] Set Source [") {
        let area = area.strip_suffix(']')?;
        let skip = area.is_empty() || area.starts_with('(');
        return (!skip).then(|| PoeLine::Entered(area.to_owned()));
    }
    // `Generating level 83 area "MapWorldsCemetery" with seed 1234`, a
    // debug line with no name in front, so chat can't make one.
    if let Some(r#gen) = rest.strip_prefix("Generating level ") {
        let (level, r#gen) = r#gen.split_once(" area \"")?;
        let (id, seed) = r#gen.split_once("\" with seed ")?;
        return Some(PoeLine::Generated {
            level: level.parse().ok()?,
            id: id.to_owned(),
            seed: seed.trim().parse().ok()?,
        });
    }
    let msg = rest.strip_prefix(": ")?;
    if let Some(area) = msg.strip_prefix("You have entered ") {
        let area = area.strip_suffix('.').unwrap_or(area);
        return (!area.is_empty()).then(|| PoeLine::Entered(area.to_owned()));
    }
    if let Some(who) = msg.strip_suffix(" has been slain.") {
        return name(who).map(PoeLine::Slain);
    }
    if let Some(who) = msg.strip_suffix(" has joined the area.") {
        return name(who).map(PoeLine::Joined);
    }
    // `Testwitch (Witch) is now level 12`
    let (who, rest) = msg.split_once(" (")?;
    let (class, level) = rest.split_once(") is now level ")?;
    Some(PoeLine::Level {
        name: name(who)?,
        class: class.to_owned(),
        level: level.trim_end_matches('.').parse().ok()?,
    })
}

/// What one play session did, from the lines seen since the game started.
///
/// The log never says which character is the player's. A party member is
/// someone who joined the player's area, so the first one to level up or
/// die who never joined is taken as the player; party members' lines count
/// for nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoeSession {
    pub character: Option<String>,
    pub class: Option<String>,
    /// The level the session started at, known from the first level-up.
    pub start_level: Option<u32>,
    pub level: Option<u32>,
    pub deaths: u32,
    pub area: Option<String>,
    /// Times the player went into another area.
    pub areas: u32,
    /// New map instances the player went into.
    pub maps: u32,
    /// The level of the area the player is in, and the highest this session.
    pub area_level: Option<u32>,
    pub highest_area_level: Option<u32>,
    /// Path of Exile 1's experience penalty for a level gap applies.
    xp_rule: bool,
    /// Areas entered where the level gap cut experience by a tenth or
    /// more, and the lowest share of experience kept in one, in percent.
    pub xp_cut_areas: u32,
    pub worst_xp_pct: Option<u32>,
    /// Seconds spent in maps before the current area.
    map_secs: i64,
    party: Vec<String>,
    /// The instance the game built last, not yet entered.
    generated: Option<(u32, String, u64)>,
    /// The map instances seen, so a portal back in isn't a new map, and
    /// their area names, so going back in still counts as time in a map.
    map_seeds: Vec<u64>,
    map_areas: Vec<String>,
    in_map: bool,
    /// When the current area was entered, and the newest line's time.
    entered_at: Option<i64>,
    last_at: Option<i64>,
}

/// Share of experience Path of Exile 1 gives a player of `player` level
/// in an area of `area` level, from the wiki's experience page: no cut
/// within a safe zone of 3 plus one per 16 levels, then
/// ((L + 5) / (L + 5 + gap^2.5))^1.5, at least 1%. Levels 95 and up carry
/// further penalties the log can't size, so they get no number.
pub fn xp_share(player: u32, area: u32) -> Option<f64> {
    if player == 0 || player >= 95 {
        return None;
    }
    let safe = 3 + player / 16;
    let gap = player.abs_diff(area).saturating_sub(safe);
    if gap == 0 {
        return Some(1.0);
    }
    let l = f64::from(player) + 5.0;
    Some((l / (l + f64::from(gap).powf(2.5))).powf(1.5).max(0.01))
}

impl PoeSession {
    /// A session for one game; only Path of Exile 1 gets experience hints.
    pub fn new(game: PoeGame) -> Self {
        Self {
            xp_rule: game == PoeGame::Poe1,
            ..Self::default()
        }
    }

    fn note_xp(&mut self, area_level: u32) {
        if !self.xp_rule {
            return;
        }
        let Some(share) = self.level.and_then(|l| xp_share(l, area_level)) else {
            return;
        };
        if share <= 0.9 {
            self.xp_cut_areas += 1;
            let pct = (share * 100.0).round() as u32;
            self.worst_xp_pct = Some(self.worst_xp_pct.map_or(pct, |w| w.min(pct)));
        }
    }

    fn is_player(&mut self, who: &str) -> bool {
        match &self.character {
            Some(me) => me == who,
            None if self.party.iter().any(|p| p == who) => false,
            None => {
                self.character = Some(who.to_owned());
                true
            }
        }
    }

    /// The player went into an instance the game just built.
    fn entered_new(&mut self, area: &str, level: u32, id: &str, seed: u64) {
        self.area_level = Some(level);
        self.highest_area_level = Some(self.highest_area_level.map_or(level, |h| h.max(level)));
        self.note_xp(level);
        self.in_map = is_map(id);
        if self.in_map && !self.map_seeds.contains(&seed) {
            self.map_seeds.push(seed);
            self.maps += 1;
            self.map_areas.retain(|a| a != area);
            self.map_areas.push(area.to_owned());
            // Only the last few maps can still have a portal open.
            if self.map_areas.len() > 6 {
                self.map_areas.remove(0);
            }
        }
    }

    /// Take one line with no time on it.
    pub fn take(&mut self, line: PoeLine) -> Option<EventKind> {
        self.take_at(line, None)
    }

    /// Seconds spent in maps, the current one included up to the newest line.
    pub fn map_secs(&self) -> i64 {
        let open = match (self.in_map, self.entered_at, self.last_at) {
            (true, Some(a), Some(b)) => (b - a).max(0),
            _ => 0,
        };
        self.map_secs + open
    }

    fn close_area(&mut self) {
        self.map_secs = self.map_secs();
        self.in_map = false;
        self.entered_at = self.last_at;
    }

    /// Take one line stamped `at` (from `line_secs`); a death of the player
    /// is also a watch event.
    pub fn take_at(&mut self, line: PoeLine, at: Option<i64>) -> Option<EventKind> {
        if let Some(t) = at {
            // A clock that steps back counts as no time passing.
            self.last_at = Some(self.last_at.map_or(t, |l| l.max(t)));
        }
        match line {
            PoeLine::Generated { level, id, seed } => {
                self.generated = Some((level, id, seed));
                None
            }
            PoeLine::Entered(area) => {
                if self.area.as_deref() != Some(area.as_str()) {
                    self.close_area();
                    self.areas += 1;
                    self.area_level = None;
                    match self.generated.take() {
                        Some((level, id, seed)) => self.entered_new(&area, level, &id, seed),
                        None => self.in_map = self.map_areas.contains(&area),
                    }
                    self.area = Some(area);
                }
                None
            }
            PoeLine::Joined(who) => {
                if self.character.as_deref() != Some(who.as_str()) && !self.party.contains(&who) {
                    self.party.push(who);
                }
                None
            }
            PoeLine::Level { name, class, level } => {
                if self.is_player(&name) {
                    self.start_level.get_or_insert(level.saturating_sub(1));
                    self.level = Some(level);
                    self.class = Some(class);
                }
                None
            }
            PoeLine::Slain(who) => self.is_player(&who).then(|| {
                self.deaths += 1;
                EventKind::Death
            }),
        }
    }

    /// Nothing worth a card: no level, death or area change.
    pub fn is_empty(&self) -> bool {
        self.level.is_none() && self.deaths == 0 && self.areas == 0
    }

    /// `Testwitch (Witch), level 12 to 15. 2 deaths. 9 areas, last in Lioneye's Watch.`
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        let mut who = self.character.clone().unwrap_or_default();
        if let Some(class) = &self.class {
            who.push_str(&format!(" ({class})"));
        }
        match (self.start_level, self.level) {
            (Some(a), Some(b)) if b > a => who.push_str(&format!(", level {a} to {b}")),
            (_, Some(b)) => who.push_str(&format!(", level {b}")),
            _ => {}
        }
        if !who.is_empty() {
            parts.push(who);
        }
        parts.push(match self.deaths {
            0 => "No deaths".to_owned(),
            1 => "1 death".to_owned(),
            n => format!("{n} deaths"),
        });
        if self.maps > 0 {
            let maps = if self.maps == 1 { "map" } else { "maps" };
            let mut line = format!("{} {maps}", self.maps);
            let mins = self.map_secs() / 60;
            if mins > 0 {
                line.push_str(&format!(" in {}", minutes_label(mins)));
            }
            parts.push(line);
        }
        if self.areas > 0 {
            let areas = if self.areas == 1 { "area" } else { "areas" };
            let mut line = format!("{} {areas}", self.areas);
            if let Some(area) = &self.area {
                line.push_str(&format!(", last in {area}"));
                if let Some(level) = self.area_level {
                    line.push_str(&format!(" (area level {level})"));
                }
            }
            parts.push(line);
        }
        if let Some(h) = self.highest_area_level {
            if self.area_level != Some(h) {
                parts.push(format!("Highest area level {h}"));
            }
        }
        if let Some(worst) = self.worst_xp_pct {
            let n = self.xp_cut_areas;
            let areas = if n == 1 { "area" } else { "areas" };
            parts.push(format!(
                "The level gap cut your XP in {n} {areas}, down to {worst}% at worst"
            ));
        }
        parts.join(". ") + "."
    }
}

/// `42 min` or `1 h 5 min`.
fn minutes_label(mins: i64) -> String {
    if mins >= 60 {
        format!("{} h {} min", mins / 60, mins % 60)
    } else {
        format!("{mins} min")
    }
}

/// The session as a Pulse card, posted when the game closes.
pub fn session_card(game: &str, s: &PoeSession, at_ms: u64) -> UpdateCard {
    let title = match (s.start_level, s.level) {
        (Some(a), Some(b)) if b > a => format!("{game} session: level {a} to {b}"),
        _ => format!("{game} session"),
    };
    let slug: String = game
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    digest_card(
        &format!("source:game-watch:{slug}"),
        &title,
        &s.summary(),
        at_ms,
    )
}

/// The tail and the session together.
#[derive(Debug)]
pub struct PoeWatcher {
    tail: CombatLogTail,
    session: PoeSession,
}

impl PoeWatcher {
    pub fn new(logs_dir: PathBuf, game: PoeGame) -> Self {
        Self {
            tail: CombatLogTail::with_picker(logs_dir, newest_client_log),
            session: PoeSession::new(game),
        }
    }

    pub fn session(&self) -> &PoeSession {
        &self.session
    }

    /// Read the lines added since the last poll into the session; the
    /// player's deaths come back stamped `now_ms`.
    pub fn poll(&mut self, now_ms: u64) -> io::Result<Vec<WatchEvent>> {
        let mut out = Vec::new();
        for line in self.tail.read_lines()? {
            let at = line_secs(&line);
            if let Some(kind) = parse_line(&line).and_then(|l| self.session.take_at(l, at)) {
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
            let d =
                std::env::temp_dir().join(format!("grokhub-poelog-{label}-{}", std::process::id()));
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

    // Made-up characters; the line shape follows the game's own log.
    fn sys(msg: &str) -> String {
        format!("2026/10/31 21:14:03 123456789 cff94598 [INFO Client 1234] : {msg}")
    }

    #[test]
    fn system_lines_parse_and_chat_lines_do_not() {
        assert_eq!(
            parse_line(&sys("You have entered Lioneye's Watch.")),
            Some(PoeLine::Entered("Lioneye's Watch".into()))
        );
        assert_eq!(
            parse_line(
                "2026/10/31 21:14:03 1 cff94598 [INFO Client 1234] [SCENE] Set Source [Clearfell Encampment]"
            ),
            Some(PoeLine::Entered("Clearfell Encampment".into()))
        );
        assert_eq!(
            parse_line(
                "2026/10/31 21:14:03 1 cff94598 [INFO Client 1234] [SCENE] Set Source [(null)]"
            ),
            None
        );
        assert_eq!(
            parse_line(&sys("Testwitch (Witch) is now level 12")),
            Some(PoeLine::Level {
                name: "Testwitch".into(),
                class: "Witch".into(),
                level: 12,
            })
        );
        assert_eq!(
            parse_line(&sys("Testwitch has been slain.")),
            Some(PoeLine::Slain("Testwitch".into()))
        );
        assert_eq!(
            parse_line(&sys("Testfriend has joined the area.")),
            Some(PoeLine::Joined("Testfriend".into()))
        );
        // Chat: a name follows the tag, not `: `.
        assert_eq!(
            parse_line(
                "2026/10/31 21:14:03 1 cff94598 [INFO Client 1234] Testtroll: Testwitch has been slain."
            ),
            None
        );
        assert_eq!(
            parse_line(
                "2026/10/31 21:14:03 1 cff94598 [INFO Client 1234] @From Testtroll: Testwitch (Witch) is now level 99"
            ),
            None
        );
        assert_eq!(parse_line(&sys("AFK mode is now ON.")), None);
        assert_eq!(parse_line("not a log line"), None);
    }

    fn take_all(lines: &[String]) -> (PoeSession, u32) {
        let mut s = PoeSession::default();
        let mut deaths = 0;
        for l in lines {
            if parse_line(l).and_then(|l| s.take(l)) == Some(EventKind::Death) {
                deaths += 1;
            }
        }
        (s, deaths)
    }

    #[test]
    fn party_members_do_not_count_as_the_player() {
        let (s, deaths) = take_all(&[
            sys("You have entered The Coast."),
            sys("Testfriend has joined the area."),
            sys("Testfriend (Ranger) is now level 30"),
            sys("Testfriend has been slain."),
            sys("Testwitch has been slain."),
            sys("Testwitch (Witch) is now level 13"),
            sys("You have entered Lioneye's Watch."),
            sys("Testwitch (Witch) is now level 14"),
        ]);
        assert_eq!(deaths, 1);
        assert_eq!(s.character.as_deref(), Some("Testwitch"));
        assert_eq!((s.start_level, s.level), (Some(12), Some(14)));
        assert_eq!(s.areas, 2);
        assert_eq!(
            s.summary(),
            "Testwitch (Witch), level 12 to 14. 1 death. 2 areas, last in Lioneye's Watch."
        );
        let card = session_card("Path of Exile", &s, 5);
        assert_eq!(card.title, "Path of Exile session: level 12 to 14");
        assert!(crate::update_feed::names_its_item(&card));
    }

    #[test]
    fn a_repeated_area_line_is_one_area_and_a_quiet_session_is_empty() {
        let (s, _) = take_all(&[
            sys("You have entered Hideout."),
            "2026/10/31 21:14:03 1 cff94598 [INFO Client 1234] [SCENE] Set Source [Hideout]".into(),
        ]);
        assert_eq!(s.areas, 1);
        assert_eq!(s.summary(), "No deaths. 1 area, last in Hideout.");
        assert!(PoeSession::default().is_empty());
        assert!(!s.is_empty());
    }

    #[test]
    fn the_watcher_reads_only_lines_written_after_it_starts() {
        let dir = Dir::new("tail");
        let log = dir.0.join("Client.txt");
        let mut f = std::fs::File::create(&log).unwrap();
        writeln!(f, "{}", sys("Oldchar has been slain.")).unwrap();
        let mut w = PoeWatcher::new(dir.0.clone(), PoeGame::Poe1);
        assert!(w.poll(1).unwrap().is_empty());
        writeln!(f, "{}", sys("You have entered The Twilight Strand.")).unwrap();
        writeln!(f, "{}", sys("Testwitch has been slain.")).unwrap();
        let events = w.poll(2).unwrap();
        assert_eq!(
            events,
            vec![WatchEvent {
                at_ms: 2,
                kind: EventKind::Death,
            }]
        );
        assert_eq!(w.session().deaths, 1);
        assert_eq!(w.session().area.as_deref(), Some("The Twilight Strand"));
    }

    #[test]
    fn installs_are_found_in_steam_libraries_and_app_ids_map_back() {
        let root = Dir::new("steam");
        let common = root.0.join("steamapps").join("common");
        std::fs::create_dir_all(common.join("Path of Exile 2").join("logs")).unwrap();
        assert_eq!(find_install(&root.0, PoeGame::Poe1), None);
        assert_eq!(
            find_install(&root.0, PoeGame::Poe2),
            Some(common.join("Path of Exile 2"))
        );
        assert_eq!(PoeGame::from_app_id(238960), Some(PoeGame::Poe1));
        assert_eq!(PoeGame::from_app_id(2694490), Some(PoeGame::Poe2));
        assert_eq!(PoeGame::from_app_id(730), None);
    }

    fn at(time: &str, rest: &str) -> String {
        format!("2026/10/31 {time} 123456789 cff94598 [{rest}")
    }

    #[test]
    fn a_level_gap_that_cuts_xp_is_counted_in_path_of_exile_1_only() {
        // Inside the safe zone (3 + 40/16 = 5 levels) nothing is lost.
        assert_eq!(xp_share(40, 44), Some(1.0));
        assert_eq!(xp_share(40, 35), Some(1.0));
        // 7 over: 2 past the zone, ((45) / (45 + 2^2.5))^1.5.
        let share = xp_share(40, 47).unwrap();
        assert!((share - 0.837).abs() < 0.001, "{share}");
        // Far past it, never under 1%.
        assert_eq!(xp_share(2, 80), Some(0.01));
        // 95 and up has penalties the log can't size.
        assert_eq!(xp_share(95, 83), None);

        let lines = [
            at(
                "20:00:00",
                "INFO Client 1] : Testwitch (Witch) is now level 40",
            ),
            at(
                "20:00:10",
                "DEBUG Client 1] Generating level 44 area \"1_3_1\" with seed 1",
            ),
            at(
                "20:00:11",
                "INFO Client 1] : You have entered The Fellshrine Ruins.",
            ),
            at(
                "20:10:00",
                "DEBUG Client 1] Generating level 47 area \"1_3_2\" with seed 2",
            ),
            at("20:10:01", "INFO Client 1] : You have entered The Crypt."),
            at(
                "20:20:00",
                "DEBUG Client 1] Generating level 52 area \"1_3_3\" with seed 3",
            ),
            at(
                "20:20:01",
                "INFO Client 1] : You have entered The Catacombs.",
            ),
        ];
        let run = |game| {
            let mut s = PoeSession::new(game);
            for l in &lines {
                s.take_at(parse_line(l).unwrap(), line_secs(l));
            }
            s
        };
        let poe1 = run(PoeGame::Poe1);
        assert_eq!((poe1.xp_cut_areas, poe1.worst_xp_pct), (2, Some(13)));
        assert!(
            poe1.summary()
                .ends_with("The level gap cut your XP in 2 areas, down to 13% at worst."),
            "{}",
            poe1.summary()
        );
        // Path of Exile 2 works its experience out another way: no hint.
        let poe2 = run(PoeGame::Poe2);
        assert_eq!((poe2.xp_cut_areas, poe2.worst_xp_pct), (0, None));
        assert!(!poe2.summary().contains("XP"));
    }

    #[test]
    fn maps_area_levels_and_time_in_maps_add_up() {
        assert_eq!(
            parse_line(&at(
                "21:00:00",
                "DEBUG Client 1234] Generating level 83 area \"MapWorldsCemetery\" with seed 42"
            )),
            Some(PoeLine::Generated {
                level: 83,
                id: "MapWorldsCemetery".into(),
                seed: 42,
            })
        );
        // Chat can't pass for one.
        assert_eq!(
            parse_line(&at(
                "21:00:00",
                "INFO Client 1234] Troll: Generating level 99 area \"MapX\" with seed 1"
            )),
            None
        );
        let lines = [
            at(
                "21:00:00",
                "DEBUG Client 1] Generating level 68 area \"HideoutFelled\" with seed 1",
            ),
            at(
                "21:00:01",
                "INFO Client 1] : You have entered Felled Hideout.",
            ),
            at(
                "21:02:00",
                "DEBUG Client 1] Generating level 81 area \"MapWorldsCemetery\" with seed 42",
            ),
            at("21:02:05", "INFO Client 1] : You have entered Cemetery."),
            at(
                "21:10:05",
                "INFO Client 1] : You have entered Felled Hideout.",
            ),
            // Back through the same portal: no new instance, same map.
            at("21:11:05", "INFO Client 1] : You have entered Cemetery."),
            at("21:15:05", "INFO Client 1] : Testwitch has been slain."),
            at(
                "21:16:00",
                "DEBUG Client 1] Generating level 83 area \"MapWorldsStrand\" with seed 7",
            ),
            at("21:16:05", "INFO Client 1] : You have entered Strand."),
            at(
                "21:46:05",
                "INFO Client 1] : Testwitch (Witch) is now level 95",
            ),
        ];
        let mut s = PoeSession::default();
        for l in &lines {
            s.take_at(parse_line(l).unwrap(), line_secs(l));
        }
        assert_eq!(s.maps, 2);
        assert_eq!((s.area_level, s.highest_area_level), (Some(83), Some(83)));
        // 8 min + 5 min in Cemetery, then 30 min so far in Strand.
        assert_eq!(s.map_secs(), (8 + 5 + 30) * 60);
        assert_eq!(
            s.summary(),
            "Testwitch (Witch), level 94 to 95. 1 death. 2 maps in 43 min. 5 areas, last in Strand (area level 83)."
        );
        assert_eq!(
            line_secs("2026/11/01 00:00:00 x").unwrap()
                - line_secs("2026/10/31 23:59:30 x").unwrap(),
            30
        );
        assert_eq!(line_secs("garbage"), None);
        assert_eq!(minutes_label(65), "1 h 5 min");
    }
}
