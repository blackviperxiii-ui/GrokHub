//! World of Warcraft adapter for the in-game watcher (design:
//! `docs/design/game-watcher/`). After `/combatlog`, retail WoW writes
//! `WoWCombatLog*.txt` under `_retail_/Logs`. The tail reads only the bytes
//! added since the last read and follows a new file or a truncated one; the
//! parser turns `ENCOUNTER_START` / `ENCOUNTER_END` into boss pulls and the
//! player's own `UNIT_DIED` into deaths. It only reads the file the game
//! writes for this: no game memory, no injection, no hooks.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::game_watch::{EventKind, Pull, WatchEvent};

/// Most bytes one read takes, so a burst (a raid pull logs thousands of
/// lines a second) is spread over reads instead of held at once.
pub const READ_CAP: u64 = 1 << 20;

/// Unit flags (`COMBATLOG_OBJECT_*`): the unit is a player, controlled by
/// the one who wrote the log.
const AFFILIATION_MINE: u32 = 0x1;
const TYPE_PLAYER: u32 = 0x400;

/// The combat log folder under a WoW install folder.
pub fn retail_logs_dir(install: &Path) -> PathBuf {
    install.join("_retail_").join("Logs")
}

/// Split the part after the timestamp into fields. Names are quoted and may
/// hold commas; quotes are dropped.
fn fields(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                out.push(s[start..i].trim_matches('"'));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].trim_end().trim_matches('"'));
    out
}

fn flags(s: &str) -> Option<u32> {
    u32::from_str_radix(s.strip_prefix("0x")?, 16).ok()
}

fn is_my_player(flag_field: &str) -> bool {
    flags(flag_field).is_some_and(|f| f & AFFILIATION_MINE != 0 && f & TYPE_PLAYER != 0)
}

/// Events whose fields start with the source and dest units
/// (`GUID, name, flags, raid flags` each).
fn has_units(event: &str) -> bool {
    [
        "SPELL_",
        "SWING_",
        "RANGE_",
        "DAMAGE_",
        "ENVIRONMENTAL_",
        "UNIT_",
        "PARTY_KILL",
    ]
    .iter()
    .any(|p| event.starts_with(p))
}

/// Reads combat log lines in order and keeps who the player is.
///
/// The log never names the player outright. Every unit carries flags,
/// though, and the player's own character is the one flagged both a player
/// and "mine" (`0x511`); pets are "mine" but not players, group members are
/// players but not "mine". The first such GUID is the player, so their
/// `UNIT_DIED` counts as a death and others' don't.
#[derive(Debug, Clone, Default)]
pub struct WowLog {
    player: Option<String>,
}

impl WowLog {
    pub fn player_guid(&self) -> Option<&str> {
        self.player.as_deref()
    }

    /// The event one log line means, if any.
    pub fn line(&mut self, line: &str) -> Option<EventKind> {
        // `10/30/2026 21:14:03.1234-4  EVENT,...`: two spaces end the stamp.
        let (_, body) = line.split_once("  ")?;
        let f = fields(body);
        let event = *f.first()?;
        if has_units(event) && self.player.is_none() {
            if let (Some(guid), Some(fl)) = (f.get(1), f.get(3)) {
                if guid.starts_with("Player-") && is_my_player(fl) {
                    self.player = Some((*guid).to_owned());
                }
            }
        }
        let num = |i: usize| f.get(i)?.parse::<u32>().ok();
        // Both lines open with: id, "name", difficulty, group size.
        let pull = || {
            Some(Pull {
                encounter_id: num(1)?,
                name: (*f.get(2)?).to_owned(),
                difficulty: num(3)?,
                group_size: num(4)?,
            })
        };
        match event {
            // ENCOUNTER_START,id,"name",difficulty,groupSize,instanceID
            "ENCOUNTER_START" => Some(EventKind::PullStart(pull()?)),
            // ENCOUNTER_END,id,"name",difficulty,groupSize,success[,fightTime]
            "ENCOUNTER_END" => Some(EventKind::PullEnd {
                pull: pull()?,
                kill: num(5)? == 1,
            }),
            // UNIT_DIED,src x4,destGUID,destName,destFlags,destRaidFlags[,unconscious]
            "UNIT_DIED" => {
                let guid = *f.get(5)?;
                let dest_flags = *f.get(7)?;
                // Unconscious on death (a feign or a cheat-death) isn't a death.
                if f.get(9) == Some(&"1") {
                    return None;
                }
                if self.player.is_none() && guid.starts_with("Player-") && is_my_player(dest_flags)
                {
                    self.player = Some(guid.to_owned());
                }
                (self.player.as_deref() == Some(guid)).then_some(EventKind::Death)
            }
            _ => None,
        }
    }
}

/// The newest `WoWCombatLog*.txt` in `dir`, by write time, then by name.
pub fn newest_combat_log(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            n.starts_with("wowcombatlog") && n.ends_with(".txt")
        })
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file()
                .then(|| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), e.path()))
        })
        .max()
        .map(|(_, p)| p)
}

/// Follows the newest combat log in a folder, reading only new bytes.
///
/// The file that is there when the tail starts is read from its end, so an
/// old session isn't replayed. A newer file (WoW starts one per session) is
/// read from its start. A file that got shorter was truncated and is read
/// from its start again. A line split across reads is held until its end
/// arrives.
#[derive(Debug)]
pub struct CombatLogTail {
    dir: PathBuf,
    /// Which file in `dir` to follow: the newest combat log for WoW.
    pick: fn(&Path) -> Option<PathBuf>,
    file: Option<PathBuf>,
    offset: u64,
    partial: Vec<u8>,
    started: bool,
}

impl CombatLogTail {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_picker(dir, newest_combat_log)
    }

    /// Follow the file `pick` chooses in `dir` instead, with the same rules
    /// (another game's own log).
    pub fn with_picker(dir: PathBuf, pick: fn(&Path) -> Option<PathBuf>) -> Self {
        Self {
            dir,
            pick,
            file: None,
            offset: 0,
            partial: Vec::new(),
            started: false,
        }
    }

    /// The complete lines added since the last read, at most [`READ_CAP`]
    /// bytes of them; call again for the rest.
    pub fn read_lines(&mut self) -> io::Result<Vec<String>> {
        let newest = (self.pick)(&self.dir);
        let first = !self.started;
        self.started = true;
        let Some(path) = newest else {
            return Ok(Vec::new());
        };
        let mut f = File::open(&path)?;
        let len = f.metadata()?.len();
        if self.file.as_ref() != Some(&path) {
            self.offset = if first { len } else { 0 };
            self.partial.clear();
            self.file = Some(path);
        } else if len < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return Ok(Vec::new());
        }
        f.seek(SeekFrom::Start(self.offset))?;
        let n = f
            .take((len - self.offset).min(READ_CAP))
            .read_to_end(&mut self.partial)?;
        self.offset += n as u64;
        let Some(end) = self.partial.iter().rposition(|b| *b == b'\n') else {
            // A "line" this long with no newline isn't a log line.
            if self.partial.len() as u64 > READ_CAP {
                self.partial.clear();
            }
            return Ok(Vec::new());
        };
        let rest = self.partial.split_off(end + 1);
        let done = std::mem::replace(&mut self.partial, rest);
        Ok(String::from_utf8_lossy(&done)
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect())
    }
}

/// The tail and the parser together.
#[derive(Debug)]
pub struct WowWatcher {
    tail: CombatLogTail,
    log: WowLog,
}

impl WowWatcher {
    pub fn new(logs_dir: PathBuf) -> Self {
        Self {
            tail: CombatLogTail::new(logs_dir),
            log: WowLog::default(),
        }
    }

    /// The events in the lines added since the last poll, stamped `now_ms`.
    pub fn poll(&mut self, now_ms: u64) -> io::Result<Vec<WatchEvent>> {
        Ok(self
            .tail
            .read_lines()?
            .iter()
            .filter_map(|l| self.log.line(l))
            .map(|kind| WatchEvent {
                at_ms: now_ms,
                kind,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Made-up characters and realm; the encounter lines follow retail's
    // field order.
    const VERSION: &str = "10/30/2026 21:14:00.0000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,11.2.5,PROJECT_ID,1";
    const PULL: &str =
        r#"10/30/2026 21:14:03.1234-4  ENCOUNTER_START,2902,"Gorvath the Unwashed",16,20,2657"#;
    const MY_CAST: &str = r#"10/30/2026 21:14:04.0000-4  SPELL_CAST_SUCCESS,Player-1111-0AAAAAAA,"Testmage-Fakerealm",0x511,0x0,Creature-0-1-2657-1-215657-000001,"Gorvath the Unwashed",0x10a48,0x0,116,"Frostbolt",0x10"#;
    const PET_CAST: &str = r#"10/30/2026 21:14:04.1000-4  SPELL_CAST_SUCCESS,Pet-0-1111-1-1-1-0BBBBBBB,"Fluffy",0x1111,0x0,0000000000000000,nil,0x80000000,0x80000000,17253,"Bite",0x1"#;
    const ALLY_DIED: &str = r#"10/30/2026 21:15:10.0000-4  UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Player-1111-0CCCCCCC,"Testpriest-Fakerealm",0x514,0x0,0"#;
    const I_DIED: &str = r#"10/30/2026 21:15:20.0000-4  UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Player-1111-0AAAAAAA,"Testmage-Fakerealm",0x511,0x0,0"#;
    const I_FEIGNED: &str = r#"10/30/2026 21:15:21.0000-4  UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Player-1111-0AAAAAAA,"Testmage-Fakerealm",0x511,0x0,1"#;
    const WIPE: &str =
        r#"10/30/2026 21:16:03.1234-4  ENCOUNTER_END,2902,"Gorvath the Unwashed",16,20,0,120000"#;
    const KILL: &str =
        r#"10/30/2026 21:30:00.0000-4  ENCOUNTER_END,2917,"Skrix, Eater of Socks",16,20,1,245321"#;

    fn gorvath(difficulty: u32, group_size: u32) -> Pull {
        Pull {
            encounter_id: 2902,
            name: "Gorvath the Unwashed".into(),
            difficulty,
            group_size,
        }
    }

    #[test]
    fn encounter_lines_become_pulls_with_kill_or_wipe() {
        let mut log = WowLog::default();
        assert_eq!(log.line(VERSION), None);
        assert_eq!(log.line(PULL), Some(EventKind::PullStart(gorvath(16, 20))));
        assert_eq!(
            log.line(WIPE),
            Some(EventKind::PullEnd {
                pull: gorvath(16, 20),
                kill: false,
            })
        );
        // A comma inside a quoted name stays in the name.
        assert_eq!(
            log.line(KILL),
            Some(EventKind::PullEnd {
                pull: Pull {
                    encounter_id: 2917,
                    name: "Skrix, Eater of Socks".into(),
                    difficulty: 16,
                    group_size: 20,
                },
                kill: true,
            })
        );
        // Older logs: no year in the stamp and no fight time at the end.
        let old = r#"10/30 21:16:03.123  ENCOUNTER_END,2902,"Gorvath the Unwashed",14,12,1"#;
        assert_eq!(
            log.line(old),
            Some(EventKind::PullEnd {
                pull: gorvath(14, 12),
                kill: true,
            })
        );
        assert_eq!(log.line("garbage"), None);
        assert_eq!(log.line("1/1 00:00:00.000  ENCOUNTER_START,x"), None);
    }

    #[test]
    fn only_the_players_own_deaths_count() {
        let mut log = WowLog::default();
        // A pet is "mine" but not a player; it doesn't name the player.
        assert_eq!(log.line(PET_CAST), None);
        assert_eq!(log.player_guid(), None);
        assert_eq!(log.line(MY_CAST), None);
        assert_eq!(log.player_guid(), Some("Player-1111-0AAAAAAA"));
        assert_eq!(log.line(ALLY_DIED), None);
        assert_eq!(log.line(I_DIED), Some(EventKind::Death));
        assert_eq!(log.line(I_FEIGNED), None);
    }

    #[test]
    fn a_death_before_any_cast_still_finds_the_player_from_its_flags() {
        let mut log = WowLog::default();
        assert_eq!(log.line(ALLY_DIED), None);
        assert_eq!(log.line(I_DIED), Some(EventKind::Death));
        assert_eq!(log.player_guid(), Some("Player-1111-0AAAAAAA"));
    }

    struct Dir(PathBuf);

    impl Dir {
        fn new(label: &str) -> Self {
            let d =
                std::env::temp_dir().join(format!("grokhub-wowlog-{label}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Self(d)
        }

        fn append(&self, name: &str, text: &str) {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.0.join(name))
                .unwrap();
            f.write_all(text.as_bytes()).unwrap();
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_tail_reads_only_new_whole_lines() {
        let d = Dir::new("tail");
        let log = "WoWCombatLog-103026_211400.txt";
        d.append(log, &format!("{VERSION}\n{PULL}\n"));
        d.append("notes.txt", "not a log\n");
        let mut t = CombatLogTail::new(d.0.clone());
        // What was there before the tail started isn't replayed.
        assert_eq!(t.read_lines().unwrap(), Vec::<String>::new());
        // A line split across writes waits for its end.
        let (head, tail) = WIPE.split_at(30);
        d.append(log, &format!("{MY_CAST}\r\n{head}"));
        assert_eq!(t.read_lines().unwrap(), vec![MY_CAST.to_string()]);
        assert_eq!(t.read_lines().unwrap(), Vec::<String>::new());
        d.append(log, &format!("{tail}\n"));
        assert_eq!(t.read_lines().unwrap(), vec![WIPE.to_string()]);
    }

    #[test]
    fn the_tail_follows_truncation_and_a_new_file() {
        let d = Dir::new("rotate");
        let old = "WoWCombatLog-103026_200000.txt";
        d.append(old, &format!("{VERSION}\n{PULL}\n{WIPE}\n"));
        let mut t = CombatLogTail::new(d.0.clone());
        assert_eq!(t.read_lines().unwrap(), Vec::<String>::new());
        // Truncated (logging restarted into the same file): read from the top.
        std::fs::write(d.0.join(old), format!("{PULL}\n")).unwrap();
        assert_eq!(t.read_lines().unwrap(), vec![PULL.to_string()]);
        // A new session's file is read from its start.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let new = "WoWCombatLog-103026_220000.txt";
        d.append(new, &format!("{VERSION}\n{KILL}\n"));
        let f = File::options().append(true).open(d.0.join(new)).unwrap();
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(newest_combat_log(&d.0), Some(d.0.join(new)));
        assert_eq!(
            t.read_lines().unwrap(),
            vec![VERSION.to_string(), KILL.to_string()]
        );
    }

    #[test]
    fn a_burst_is_read_in_capped_chunks() {
        let d = Dir::new("burst");
        let log = "WoWCombatLog-103026_211400.txt";
        d.append(log, "");
        let mut t = CombatLogTail::new(d.0.clone());
        assert_eq!(t.read_lines().unwrap(), Vec::<String>::new());
        // About 2.4 MB of lines: more than one read's cap.
        let line = format!("{MY_CAST}\n");
        let count = (2 * READ_CAP as usize) / line.len() + 500;
        d.append(log, &line.repeat(count));
        let first = t.read_lines().unwrap().len();
        // One read takes READ_CAP bytes, which hold this many whole lines.
        assert_eq!(first, READ_CAP as usize / line.len());
        let mut total = first;
        loop {
            let n = t.read_lines().unwrap().len();
            if n == 0 {
                break;
            }
            total += n;
        }
        assert_eq!(total, count);
    }

    #[test]
    fn the_watcher_turns_new_lines_into_events() {
        let d = Dir::new("watch");
        let log = "WoWCombatLog-103026_211400.txt";
        d.append(log, &format!("{VERSION}\n"));
        let mut w = WowWatcher::new(d.0.clone());
        assert_eq!(w.poll(1).unwrap(), vec![]);
        d.append(
            log,
            &format!("{PULL}\n{MY_CAST}\n{ALLY_DIED}\n{I_DIED}\n{WIPE}\n"),
        );
        let kinds: Vec<_> = w.poll(2).unwrap().into_iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                EventKind::PullStart(gorvath(16, 20)),
                EventKind::Death,
                EventKind::PullEnd {
                    pull: gorvath(16, 20),
                    kill: false,
                },
            ]
        );
    }

    #[test]
    fn the_logs_folder_sits_under_retail() {
        assert_eq!(
            retail_logs_dir(Path::new("wow")),
            Path::new("wow").join("_retail_").join("Logs")
        );
    }
}
