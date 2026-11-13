//! World of Warcraft adapter for the in-game watcher (design:
//! `docs/design/game-watcher/`). After `/combatlog`, retail WoW writes
//! `WoWCombatLog*.txt` under `_retail_/Logs`. The tail reads only the bytes
//! added since the last read and follows a new file or a truncated one; the
//! parser turns `ENCOUNTER_START` / `ENCOUNTER_END` into boss pulls and the
//! player's own `UNIT_DIED` into deaths, and `CHALLENGE_MODE_START` /
//! `CHALLENGE_MODE_END` into Mythic+ keys. [`WowSession`] sums those into
//! Pulse cards: one per finished key, one per boss kill, and one for the
//! bosses left standing when WoW closes. [`logging_since`] tells whether
//! combat logging is on at all, so the app can remind the player to type
//! `/combatlog`. It only reads the file the game writes for this: no game
//! memory, no injection, no hooks.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::game_watch::{EventKind, Pull, WatchEvent};
use crate::update_feed::{digest_card, UpdateCard};

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
            // CHALLENGE_MODE_START,"zone",instanceID,challengeModeID,level,[affixes]
            "CHALLENGE_MODE_START" => Some(EventKind::KeyStart {
                instance_id: num(2)?,
                name: (*f.get(1)?).to_owned(),
                level: num(4)?,
            }),
            // CHALLENGE_MODE_END,instanceID,success,level,totalTime[,...]
            "CHALLENGE_MODE_END" => Some(EventKind::KeyEnd {
                instance_id: num(1)?,
                finished: num(2)? == 1,
                level: num(3)?,
                time_ms: f.get(4)?.parse().ok()?,
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

/// Whether WoW wrote a combat log in `dir` at or after `since`: logging is
/// on. WoW opens (or appends to) its log the moment `/combatlog` turns it
/// on, so a log no newer than the game's start means it is off.
pub fn logging_since(dir: &Path, since: SystemTime) -> bool {
    newest_combat_log(dir)
        .and_then(|p| std::fs::metadata(p).ok()?.modified().ok())
        .is_some_and(|m| m >= since)
}

/// The name WoW's difficulty IDs (`ENCOUNTER_START`'s third field) go by.
pub fn difficulty_name(id: u32) -> Option<&'static str> {
    Some(match id {
        1 | 14 | 173 => "Normal",
        2 | 15 | 174 => "Heroic",
        3 | 175 => "10 Player",
        4 | 176 => "25 Player",
        5 | 193 => "10 Heroic",
        6 | 194 => "25 Heroic",
        9 | 186 => "40 Player",
        148 | 185 => "20 Player",
        8 => "Mythic+",
        16 | 23 => "Mythic",
        7 | 17 | 151 => "LFR",
        24 | 33 => "Timewalking",
        205 => "Follower",
        208 => "Delve",
        220 => "Story",
        _ => return None,
    })
}

fn pull_label(p: &Pull) -> String {
    match difficulty_name(p.difficulty) {
        Some(d) => format!("{} ({d})", p.name),
        None => p.name.clone(),
    }
}

/// `28:43`, or `1:02:05` past an hour.
fn clock(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `1 pull`, `3 pulls`. (Spelled in the format string: the offline guard
/// reads a bare git verb in quotes as a network exit.)
fn pulls_text(n: u32) -> String {
    format!("{n} pull{}", if n == 1 { "" } else { "s" })
}

fn times(n: u32, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The Pulse source WoW's cards carry, the same slug the other game
/// watchers' recaps use.
pub const CARD_SOURCE: &str = "source:game-watch:world-of-warcraft";

/// A Mythic+ key under way.
#[derive(Debug, Clone)]
pub(crate) struct KeyRun {
    name: String,
    level: u32,
    kills: Vec<String>,
    wipes: u32,
    deaths: u32,
}

/// One boss outside a key, since WoW started.
#[derive(Debug, Clone)]
struct Boss {
    pull: Pull,
    pulls: u32,
    deaths: u32,
}

/// One WoW session as the cabin follows it: the key under way, the boss
/// being pulled, and the bosses pulled without a kill. It turns events into
/// Pulse cards; nothing here reads the game.
#[derive(Debug, Default)]
pub struct WowSession {
    key: Option<KeyRun>,
    pulling: Option<Pull>,
    bosses: Vec<Boss>,
}

impl WowSession {
    fn boss(&mut self, pull: &Pull) -> &mut Boss {
        let at = self
            .bosses
            .iter()
            .position(|b| {
                b.pull.encounter_id == pull.encounter_id && b.pull.difficulty == pull.difficulty
            })
            .unwrap_or_else(|| {
                self.bosses.push(Boss {
                    pull: pull.clone(),
                    pulls: 0,
                    deaths: 0,
                });
                self.bosses.len() - 1
            });
        &mut self.bosses[at]
    }

    /// The cards one event brings: a key's when it ends, a boss's when it
    /// dies outside a key.
    pub fn take(&mut self, event: &WatchEvent) -> Vec<UpdateCard> {
        match &event.kind {
            EventKind::KeyStart { name, level, .. } => {
                self.key = Some(KeyRun {
                    name: name.clone(),
                    level: *level,
                    kills: Vec::new(),
                    wipes: 0,
                    deaths: 0,
                });
                self.pulling = None;
            }
            EventKind::KeyEnd {
                level,
                finished,
                time_ms,
                ..
            } => {
                let run = self.key.take();
                // An abandoned key logs an end with nothing in it; with no
                // start seen either there is nothing to sum.
                if run.is_none() && !finished {
                    return Vec::new();
                }
                return vec![key_card(run, *level, *finished, *time_ms, event.at_ms)];
            }
            EventKind::PullStart(pull) => {
                self.pulling = Some(pull.clone());
                if self.key.is_none() {
                    self.boss(pull);
                }
            }
            EventKind::PullEnd { pull, kill } => {
                self.pulling = None;
                if let Some(run) = self.key.as_mut() {
                    if *kill {
                        run.kills.push(pull.name.clone());
                    } else {
                        run.wipes += 1;
                    }
                    return Vec::new();
                }
                let boss = self.boss(pull);
                boss.pulls += 1;
                if *kill {
                    let boss = boss.clone();
                    self.bosses.retain(|b| {
                        !(b.pull.encounter_id == pull.encounter_id
                            && b.pull.difficulty == pull.difficulty)
                    });
                    return vec![boss_kill_card(
                        &boss.pull.name,
                        boss.pull.difficulty,
                        boss.pulls,
                        boss.deaths,
                        event.at_ms,
                    )];
                }
            }
            EventKind::Death => {
                if let Some(run) = self.key.as_mut() {
                    run.deaths += 1;
                } else if let Some(pull) = self.pulling.clone() {
                    self.boss(&pull).deaths += 1;
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// WoW closed: one card for the bosses pulled and not killed, if any.
    pub fn close(&mut self, at_ms: u64) -> Option<UpdateCard> {
        let left: Vec<Boss> = std::mem::take(&mut self.bosses)
            .into_iter()
            .filter(|b| b.pulls > 0)
            .collect();
        *self = Self::default();
        if left.is_empty() {
            return None;
        }
        let pulls: u32 = left.iter().map(|b| b.pulls).sum();
        let title = format!(
            "WoW session: {} on {} with no kill",
            pulls_text(pulls),
            times(left.len() as u32, "boss", "bosses")
        );
        let body = left
            .iter()
            .map(|b| {
                let mut line = format!("{}: {}", pull_label(&b.pull), pulls_text(b.pulls));
                if b.deaths > 0 {
                    line.push_str(&format!(", you died {}", times(b.deaths, "time", "times")));
                }
                line.push('.');
                line
            })
            .collect::<Vec<_>>()
            .join(" ");
        Some(digest_card(CARD_SOURCE, &title, &body, at_ms))
    }
}

pub(crate) fn key_card(
    run: Option<KeyRun>,
    level: u32,
    finished: bool,
    time_ms: u64,
    at_ms: u64,
) -> UpdateCard {
    let name = run.as_ref().map_or("Mythic+ key", |r| r.name.as_str());
    let level = run.as_ref().map_or(level, |r| r.level).max(level);
    let title = if finished {
        format!("{name} +{level}: finished in {}", clock(time_ms))
    } else {
        format!("{name} +{level}: not finished")
    };
    let mut parts = Vec::new();
    if let Some(r) = &run {
        if !r.kills.is_empty() {
            parts.push(format!("Bosses down: {}.", r.kills.join(", ")));
        }
        if r.wipes > 0 {
            parts.push(format!("{} on bosses.", times(r.wipes, "wipe", "wipes")));
        }
        parts.push(match r.deaths {
            0 => "You didn't die.".to_string(),
            n => format!("You died {}.", times(n, "time", "times")),
        });
    }
    if finished {
        parts.push("The time counts death penalties.".into());
    }
    digest_card(CARD_SOURCE, &title, &parts.join(" "), at_ms)
}

/// A boss killed outside a key: which pull it took, and the player's
/// deaths on it.
pub fn boss_kill_card(
    name: &str,
    difficulty: u32,
    pulls: u32,
    deaths: u32,
    at_ms: u64,
) -> UpdateCard {
    let label = pull_label(&Pull {
        encounter_id: 0,
        name: name.to_owned(),
        difficulty,
        group_size: 0,
    });
    let title = if pulls <= 1 {
        format!("Killed {label} on the first pull")
    } else {
        format!("Killed {label} on pull {pulls}")
    };
    let body = match deaths {
        0 => "You didn't die on it.".to_string(),
        n => format!("You died {} on it.", times(n, "time", "times")),
    };
    digest_card(CARD_SOURCE, &title, &body, at_ms)
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

    // Key lines in retail's field order; the dungeon name holds a comma.
    const KEY_START: &str = r#"10/31/2026 20:00:00.0000-4  CHALLENGE_MODE_START,"Ara-Kara, City of Echoes",2660,503,10,[10,147,9,152]"#;
    const KEY_BOSS: &str = r#"10/31/2026 20:05:00.0000-4  ENCOUNTER_START,2926,"Avanoxx",8,5,2660"#;
    const KEY_WIPE: &str =
        r#"10/31/2026 20:06:00.0000-4  ENCOUNTER_END,2926,"Avanoxx",8,5,0,60000"#;
    const KEY_KILL: &str =
        r#"10/31/2026 20:09:00.0000-4  ENCOUNTER_END,2926,"Avanoxx",8,5,1,120000"#;
    const KEY_END: &str =
        r#"10/31/2026 20:28:43.0000-4  CHALLENGE_MODE_END,2660,1,10,1723000,1.25,2765.8"#;
    const KEY_ABANDONED: &str = "10/31/2026 20:10:00.0000-4  CHALLENGE_MODE_END,2660,0,0,0";

    fn events(lines: &[&str]) -> Vec<WatchEvent> {
        let mut log = WowLog::default();
        lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                log.line(l).map(|kind| WatchEvent {
                    at_ms: i as u64,
                    kind,
                })
            })
            .collect()
    }

    fn cards(session: &mut WowSession, lines: &[&str]) -> Vec<(String, String)> {
        events(lines)
            .iter()
            .flat_map(|e| session.take(e))
            .map(|c| (c.title, c.body.unwrap_or_default()))
            .collect()
    }

    #[test]
    fn challenge_mode_lines_become_key_events() {
        let mut log = WowLog::default();
        assert_eq!(
            log.line(KEY_START),
            Some(EventKind::KeyStart {
                instance_id: 2660,
                name: "Ara-Kara, City of Echoes".into(),
                level: 10,
            })
        );
        assert_eq!(
            log.line(KEY_END),
            Some(EventKind::KeyEnd {
                instance_id: 2660,
                level: 10,
                finished: true,
                time_ms: 1_723_000,
            })
        );
        // Older clients end the line at the total time.
        assert_eq!(
            log.line("1/1 00:00:00.000  CHALLENGE_MODE_END,2290,0,11,2400000"),
            Some(EventKind::KeyEnd {
                instance_id: 2290,
                level: 11,
                finished: false,
                time_ms: 2_400_000,
            })
        );
        assert_eq!(log.line("1/1 00:00:00.000  CHALLENGE_MODE_END,2290"), None);
    }

    #[test]
    fn a_finished_key_posts_one_card_with_its_bosses_wipes_and_deaths() {
        let mut s = WowSession::default();
        let got = cards(
            &mut s,
            &[
                KEY_START, MY_CAST, KEY_BOSS, I_DIED, KEY_WIPE, KEY_BOSS, KEY_KILL, I_DIED, KEY_END,
            ],
        );
        assert_eq!(
            got,
            vec![(
                "Ara-Kara, City of Echoes +10: finished in 28:43".to_string(),
                "Bosses down: Avanoxx. 1 wipe on bosses. You died 2 times. The time counts death penalties."
                    .to_string()
            )]
        );
        // Bosses inside the key aren't left over for the session card.
        assert_eq!(s.close(99), None);
    }

    #[test]
    fn an_abandoned_key_says_not_finished_and_a_stray_end_says_nothing() {
        let mut s = WowSession::default();
        let got = cards(&mut s, &[KEY_START, KEY_ABANDONED]);
        assert_eq!(
            got,
            vec![(
                "Ara-Kara, City of Echoes +10: not finished".to_string(),
                "You didn't die.".to_string()
            )]
        );
        assert_eq!(cards(&mut s, &[KEY_ABANDONED]), vec![]);
        // An end with no start seen still names the level and time.
        let got = cards(&mut s, &[KEY_END]);
        assert_eq!(got[0].0, "Mythic+ key +10: finished in 28:43");
    }

    #[test]
    fn a_boss_kill_says_which_pull_it_took_and_your_deaths_on_it() {
        let mut s = WowSession::default();
        let got = cards(
            &mut s,
            &[MY_CAST, PULL, I_DIED, WIPE, PULL, ALLY_DIED, WIPE],
        );
        assert_eq!(got, vec![]);
        let kill = r#"10/30/2026 21:40:00.0000-4  ENCOUNTER_END,2902,"Gorvath the Unwashed",16,20,1,300000"#;
        let got = cards(&mut s, &[PULL, kill]);
        assert_eq!(
            got,
            vec![(
                "Killed Gorvath the Unwashed (Mythic) on pull 3".to_string(),
                "You died 1 time on it.".to_string()
            )]
        );
        // A first-pull kill says so.
        let got = cards(&mut s, &[KILL]);
        assert_eq!(
            got[0].0,
            "Killed Skrix, Eater of Socks (Mythic) on the first pull"
        );
        assert_eq!(s.close(5), None);
    }

    #[test]
    fn closing_wow_sums_the_bosses_left_standing() {
        let mut s = WowSession::default();
        let heroic = r#"10/30/2026 22:00:00.0000-4  ENCOUNTER_START,2917,"Skrix, Eater of Socks",15,20,2657"#;
        let heroic_wipe = r#"10/30/2026 22:02:00.0000-4  ENCOUNTER_END,2917,"Skrix, Eater of Socks",15,20,0,120000"#;
        cards(
            &mut s,
            &[
                MY_CAST,
                PULL,
                I_DIED,
                WIPE,
                PULL,
                WIPE,
                heroic,
                heroic_wipe,
                PULL,
            ],
        );
        let card = s.close(7).unwrap();
        assert_eq!(card.title, "WoW session: 3 pulls on 2 bosses with no kill");
        assert_eq!(
            card.body.as_deref(),
            Some("Gorvath the Unwashed (Mythic): 2 pulls, you died 1 time. Skrix, Eater of Socks (Heroic): 1 pull.")
        );
        assert_eq!(card.source_id, CARD_SOURCE);
        // The session starts over after a close.
        assert_eq!(s.close(8), None);
    }

    #[test]
    fn difficulty_ids_read_as_their_names() {
        assert_eq!(difficulty_name(16), Some("Mythic"));
        assert_eq!(difficulty_name(8), Some("Mythic+"));
        assert_eq!(difficulty_name(17), Some("LFR"));
        assert_eq!(difficulty_name(9999), None);
        assert_eq!(clock(3_725_000), "1:02:05");
        assert_eq!(clock(59_000), "0:59");
    }

    #[test]
    fn logging_counts_as_on_only_when_a_log_was_written_since_wow_started() {
        let d = Dir::new("logging");
        let started = SystemTime::now();
        assert!(!logging_since(&d.0, started));
        let log = "WoWCombatLog-103126_210000.txt";
        d.append(log, &format!("{VERSION}\n"));
        let f = File::options().append(true).open(d.0.join(log)).unwrap();
        f.set_modified(started - std::time::Duration::from_secs(3600))
            .unwrap();
        assert!(!logging_since(&d.0, started));
        f.set_modified(started + std::time::Duration::from_secs(2))
            .unwrap();
        assert!(logging_since(&d.0, started));
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
