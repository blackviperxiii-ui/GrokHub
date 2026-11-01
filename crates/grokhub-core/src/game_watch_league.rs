//! League of Legends adapter for the in-game watcher (design:
//! `docs/design/game-watcher/`). During a match the game client serves
//! Riot's documented Live Client Data API on `https://127.0.0.1:2999`. The
//! poller reads `allgamedata` about once a second while League's game
//! process runs, and diffs its event list by `EventID` into
//! [`WatchEvent`]s. It only reads that local API: no game memory, no
//! injection, no hooks.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use crate::game_guard::file_name_lower;
use crate::game_watch::{EventKind, Objective, WatchEvent};

/// The only origin the poller talks to.
pub const LIVE_CLIENT_ORIGIN: &str = "https://127.0.0.1:2999";
/// Everything in one read: the player, the roster, the events, the clock.
pub const ALL_GAME_DATA: &str = "/liveclientdata/allgamedata";

/// Poll interval while a match is up.
pub const POLL_MS: u64 = 1_000;
/// Wait between process checks while League's game isn't running. No
/// request is made then.
pub const IDLE_MS: u64 = 5_000;
/// Longest wait while the game runs but the port is closed (loading screen).
pub const MAX_BACKOFF_MS: u64 = 16_000;

/// One entry of the API's `Events` list, with the fields the watcher reads.
#[derive(Debug, Clone, PartialEq)]
pub struct LeagueEvent {
    pub id: u64,
    pub name: String,
    /// Seconds since the match began.
    pub time_s: f64,
    pub killer: Option<String>,
    pub victim: Option<String>,
    /// `TurretKilled`: the turret's name, which carries its side (`_T1_`
    /// blue / ORDER, `_T2_` red / CHAOS).
    pub turret: Option<String>,
    /// `GameEnd`: `Win` or `Lose`, from the active player's side.
    pub result: Option<String>,
}

/// The parts of `allgamedata` the watcher reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LeagueSnapshot {
    /// Seconds since the match began.
    pub game_time_s: f64,
    /// Every name the active player goes by in events (summoner name,
    /// Riot ID, Riot ID game name). Empty while spectating.
    pub me: Vec<String>,
    /// Player name to team (`ORDER` or `CHAOS`), under each of their names.
    pub teams: HashMap<String, String>,
    pub events: Vec<LeagueEvent>,
}

impl LeagueSnapshot {
    /// The active player's team, when the roster lists them.
    pub fn my_team(&self) -> Option<&str> {
        self.me
            .iter()
            .find_map(|n| self.teams.get(n))
            .map(String::as_str)
    }

    fn is_me(&self, name: Option<&str>) -> bool {
        name.is_some_and(|n| self.me.iter().any(|m| m == n))
    }
}

fn names(v: &Value) -> Vec<String> {
    ["summonerName", "riotId", "riotIdGameName"]
        .iter()
        .filter_map(|k| v.get(k)?.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k)?.as_str().map(str::to_owned)
}

fn parse_events(list: &Value) -> Vec<LeagueEvent> {
    let Some(list) = list.get("Events").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|e| {
            Some(LeagueEvent {
                id: e.get("EventID")?.as_u64()?,
                name: text(e, "EventName")?,
                time_s: e.get("EventTime").and_then(Value::as_f64).unwrap_or(0.0),
                killer: text(e, "KillerName"),
                victim: text(e, "VictimName"),
                turret: text(e, "TurretKilled"),
                result: text(e, "Result"),
            })
        })
        .collect()
}

/// Parse an `eventdata` body (`{"Events":[...]}`). `None` when it isn't
/// JSON.
pub fn parse_event_data(body: &str) -> Option<Vec<LeagueEvent>> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(parse_events(&v))
}

/// Parse an `allgamedata` body. `None` when it isn't JSON or has no game
/// clock (the API answers with an error object between matches).
pub fn parse_all_game_data(body: &str) -> Option<LeagueSnapshot> {
    let v: Value = serde_json::from_str(body).ok()?;
    let game_time_s = v.get("gameData")?.get("gameTime")?.as_f64()?;
    let mut teams = HashMap::new();
    for p in v
        .get("allPlayers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(team) = text(p, "team") {
            for n in names(p) {
                teams.insert(n, team.clone());
            }
        }
    }
    Some(LeagueSnapshot {
        game_time_s,
        // Spectating, `activePlayer` is an error object with no names.
        me: v.get("activePlayer").map(names).unwrap_or_default(),
        teams,
        events: v.get("events").map(parse_events).unwrap_or_default(),
    })
}

/// The side whose turret this is, from its name (`Turret_T1_...` is
/// ORDER's, `Turret_T2_...` CHAOS's).
fn turret_owner(name: &str) -> Option<&'static str> {
    if name.contains("_T1_") {
        Some("ORDER")
    } else if name.contains("_T2_") {
        Some("CHAOS")
    } else {
        None
    }
}

/// What one API event means for the player. Kills, deaths and the result
/// need the active player; objectives fall back to `ally: None` without one.
pub fn event_kind(snap: &LeagueSnapshot, e: &LeagueEvent) -> Option<EventKind> {
    let mine = snap.my_team();
    let killer_side = |e: &LeagueEvent| -> Option<bool> {
        let team = snap.teams.get(e.killer.as_deref()?)?;
        Some(team == mine?)
    };
    match e.name.as_str() {
        "GameStart" => Some(EventKind::MatchStart),
        "ChampionKill" if snap.is_me(e.killer.as_deref()) => Some(EventKind::Kill),
        "ChampionKill" if snap.is_me(e.victim.as_deref()) => Some(EventKind::Death),
        "DragonKill" => Some(EventKind::Objective {
            what: Objective::Dragon,
            ally: killer_side(e),
        }),
        "BaronKill" => Some(EventKind::Objective {
            what: Objective::Baron,
            ally: killer_side(e),
        }),
        // Minions often take turrets, so the side comes from the turret.
        "TurretKilled" => {
            let owner = e.turret.as_deref().and_then(turret_owner);
            let ally = match (owner, mine) {
                (Some(owner), Some(mine)) => Some(owner != mine),
                _ => killer_side(e),
            };
            Some(EventKind::Objective {
                what: Objective::Turret,
                ally,
            })
        }
        // A League match is one game, so the score is 1-0 or 0-1.
        "GameEnd" if !snap.me.is_empty() => match e.result.as_deref() {
            Some("Win") => Some(EventKind::MatchEnd { won: 1, lost: 0 }),
            Some("Lose") => Some(EventKind::MatchEnd { won: 0, lost: 1 }),
            _ => None,
        },
        _ => None,
    }
}

/// Tracks the last `EventID` seen, so each API event becomes at most one
/// [`WatchEvent`]. Unlike GSI snapshots, the API keeps the whole event list
/// with each event's game time, so a watcher that starts mid match picks up
/// the events before it too, at the time they happened.
#[derive(Debug, Clone, Default)]
pub struct LeagueDiff {
    last_id: Option<u64>,
}

impl LeagueDiff {
    /// The new events in `snap`, stamped `now_ms` minus how long ago in game
    /// time they happened.
    pub fn events(&mut self, snap: &LeagueSnapshot, now_ms: u64) -> Vec<WatchEvent> {
        let top = snap.events.iter().map(|e| e.id).max();
        // A list that shrank is a new match: IDs start over.
        if let Some(last) = self.last_id {
            if top.is_none_or(|top| top < last) {
                self.last_id = None;
            }
        }
        let mut new: Vec<&LeagueEvent> = snap
            .events
            .iter()
            .filter(|e| self.last_id.is_none_or(|last| e.id > last))
            .collect();
        new.sort_by_key(|e| e.id);
        if let Some(top) = top {
            self.last_id = Some(top);
        }
        new.into_iter()
            .filter_map(|e| {
                let ago_s = (snap.game_time_s - e.time_s).max(0.0);
                Some(WatchEvent {
                    at_ms: now_ms.saturating_sub((ago_s * 1000.0).round() as u64),
                    kind: event_kind(snap, e)?,
                })
            })
            .collect()
    }
}

/// League's game process (not the lobby client) in a process name list:
/// `League of Legends.exe` on Windows, or its 15-character `comm` under
/// Wine on Linux.
pub fn league_game_running<N: AsRef<str>>(names: &[N]) -> bool {
    names.iter().any(|n| {
        let n = file_name_lower(n.as_ref());
        n == "league of legends.exe" || n == "league of legen"
    })
}

/// The one read the poller needs, behind a trait so tests need no network.
pub trait LiveClient {
    /// The `allgamedata` body, or `None` when the port is closed or the
    /// match isn't up yet.
    fn all_game_data(&mut self) -> Option<String>;
}

/// The real client. Riot's certificate on port 2999 is self-signed, so this
/// agent skips certificate checks. It is private to that one origin: it
/// only ever requests [`LIVE_CLIENT_ORIGIN`] paths, dials loopback addresses
/// and `https` only (`loopback_self_signed`), follows no redirects and uses
/// no proxy, so the relaxed check can't reach any other host. No other
/// agent in the app is affected.
pub struct RiotLoopback {
    http: crate::http::Agent,
}

impl RiotLoopback {
    pub fn new() -> Result<Self, crate::http::Error> {
        let http = crate::http::agent(crate::http::Opts {
            connect: Duration::from_millis(500),
            overall: Some(Duration::from_secs(2)),
            redirects: 0,
            loopback_self_signed: true,
            ..crate::http::Opts::default()
        });
        Ok(Self { http })
    }
}

impl LiveClient for RiotLoopback {
    fn all_game_data(&mut self) -> Option<String> {
        let url = format!("{LIVE_CLIENT_ORIGIN}{ALL_GAME_DATA}");
        let resp = self.http.get(&url).call().ok()?;
        if !(200..300).contains(&resp.status()) {
            return None;
        }
        resp.into_string().ok()
    }
}

/// What one poll found and when to poll next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    pub events: Vec<WatchEvent>,
    pub next_in_ms: u64,
}

/// Drives [`LiveClient`] reads: none while League's game isn't running,
/// every [`POLL_MS`] during a match, and doubling up to [`MAX_BACKOFF_MS`]
/// while the port is closed.
#[derive(Debug, Default)]
pub struct LeaguePoller {
    diff: LeagueDiff,
    misses: u32,
}

impl LeaguePoller {
    pub fn tick(&mut self, game_running: bool, client: &mut impl LiveClient, now_ms: u64) -> Tick {
        if !game_running {
            *self = Self::default();
            return Tick {
                events: Vec::new(),
                next_in_ms: IDLE_MS,
            };
        }
        match client
            .all_game_data()
            .as_deref()
            .and_then(parse_all_game_data)
        {
            Some(snap) => {
                self.misses = 0;
                Tick {
                    events: self.diff.events(&snap, now_ms),
                    next_in_ms: POLL_MS,
                }
            }
            None => {
                self.misses = self.misses.saturating_add(1);
                Tick {
                    events: Vec::new(),
                    next_in_ms: (POLL_MS << self.misses.min(4)).min(MAX_BACKOFF_MS),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trimmed `allgamedata` body: the real one carries abilities, runes,
    // items and stats too, which the parser skips.
    fn all_game_data(game_time: f64, events: &str) -> String {
        format!(
            r#"{{
  "activePlayer": {{"summonerName": "Tester", "riotId": "Tester#NA1", "riotIdGameName": "Tester", "level": 9}},
  "allPlayers": [
    {{"championName": "Ahri", "summonerName": "Tester", "riotId": "Tester#NA1", "riotIdGameName": "Tester", "team": "ORDER"}},
    {{"championName": "Garen", "summonerName": "Buddy", "riotId": "Buddy#NA1", "riotIdGameName": "Buddy", "team": "ORDER"}},
    {{"championName": "Lux", "summonerName": "Rival", "riotId": "Rival#EUW", "riotIdGameName": "Rival", "team": "CHAOS"}}
  ],
  "events": {{"Events": [{events}]}},
  "gameData": {{"gameMode": "CLASSIC", "gameTime": {game_time}, "mapName": "Map11"}}
}}"#
        )
    }

    const START: &str = r#"{"EventID":0,"EventName":"GameStart","EventTime":0.05}"#;
    const MY_KILL: &str = r#"{"EventID":1,"EventName":"ChampionKill","EventTime":200.5,"KillerName":"Tester","VictimName":"Rival","Assisters":["Buddy"]}"#;
    const MY_DEATH: &str = r#"{"EventID":2,"EventName":"ChampionKill","EventTime":300.0,"KillerName":"Rival","VictimName":"Tester","Assisters":[]}"#;
    const OTHER_KILL: &str = r#"{"EventID":3,"EventName":"ChampionKill","EventTime":310.0,"KillerName":"Buddy","VictimName":"Rival","Assisters":[]}"#;
    const DRAGON: &str = r#"{"EventID":4,"EventName":"DragonKill","EventTime":400.0,"DragonType":"Fire","Stolen":"False","KillerName":"Rival","Assisters":[]}"#;
    const BARON: &str = r#"{"EventID":5,"EventName":"BaronKill","EventTime":1300.0,"Stolen":"False","KillerName":"Buddy","Assisters":["Tester"]}"#;
    const TURRET: &str = r#"{"EventID":6,"EventName":"TurretKilled","EventTime":800.0,"TurretKilled":"Turret_T2_L_03_A","KillerName":"Minion_T100L1S12N0003","Assisters":[]}"#;
    const LOST_TURRET: &str = r#"{"EventID":7,"EventName":"TurretKilled","EventTime":900.0,"TurretKilled":"Turret_T1_C_05_A","KillerName":"Rival","Assisters":[]}"#;
    const END: &str = r#"{"EventID":8,"EventName":"GameEnd","EventTime":1800.0,"Result":"Win"}"#;

    #[test]
    fn allgamedata_parses_the_player_roster_clock_and_events() {
        let s = parse_all_game_data(&all_game_data(205.0, &[START, MY_KILL].join(","))).unwrap();
        assert_eq!(s.game_time_s, 205.0);
        assert_eq!(s.me, vec!["Tester", "Tester#NA1", "Tester"]);
        assert_eq!(s.my_team(), Some("ORDER"));
        assert_eq!(s.teams.get("Rival#EUW").map(String::as_str), Some("CHAOS"));
        assert_eq!(s.events.len(), 2);
        assert_eq!(
            s.events[1],
            LeagueEvent {
                id: 1,
                name: "ChampionKill".into(),
                time_s: 200.5,
                killer: Some("Tester".into()),
                victim: Some("Rival".into()),
                turret: None,
                result: None,
            }
        );
        assert_eq!(parse_all_game_data("not json"), None);
        // Between matches the API answers with an error object.
        assert_eq!(
            parse_all_game_data(r#"{"errorCode":"RESOURCE_NOT_FOUND","httpStatus":404}"#),
            None
        );
    }

    #[test]
    fn eventdata_parses_the_event_list() {
        let body = format!(r#"{{"Events":[{START},{END}]}}"#);
        let ev = parse_event_data(&body).unwrap();
        assert_eq!(
            ev.iter()
                .map(|e| (e.id, e.name.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "GameStart"), (8, "GameEnd")]
        );
        assert_eq!(ev[1].result.as_deref(), Some("Win"));
        assert_eq!(parse_event_data("{}"), Some(vec![]));
        assert_eq!(parse_event_data("nope"), None);
    }

    #[test]
    fn api_events_map_onto_watch_kinds() {
        let all = [
            START,
            MY_KILL,
            MY_DEATH,
            OTHER_KILL,
            DRAGON,
            BARON,
            TURRET,
            LOST_TURRET,
            END,
        ];
        let s = parse_all_game_data(&all_game_data(1800.0, &all.join(","))).unwrap();
        let kinds: Vec<_> = s.events.iter().map(|e| event_kind(&s, e)).collect();
        assert_eq!(
            kinds,
            vec![
                Some(EventKind::MatchStart),
                Some(EventKind::Kill),
                Some(EventKind::Death),
                // A teammate's kill is not the player's.
                None,
                Some(EventKind::Objective {
                    what: Objective::Dragon,
                    ally: Some(false)
                }),
                Some(EventKind::Objective {
                    what: Objective::Baron,
                    ally: Some(true)
                }),
                // A minion took CHAOS's turret: ORDER, the player's side, gained it.
                Some(EventKind::Objective {
                    what: Objective::Turret,
                    ally: Some(true)
                }),
                Some(EventKind::Objective {
                    what: Objective::Turret,
                    ally: Some(false)
                }),
                Some(EventKind::MatchEnd { won: 1, lost: 0 }),
            ]
        );
        let lose = LeagueEvent {
            result: Some("Lose".into()),
            ..s.events[8].clone()
        };
        assert_eq!(
            event_kind(&s, &lose),
            Some(EventKind::MatchEnd { won: 0, lost: 1 })
        );
    }

    #[test]
    fn spectating_records_objectives_without_a_side_and_no_result() {
        let body = all_game_data(1800.0, &[MY_KILL, DRAGON, TURRET, END].join(","))
            .replace(
                r#""activePlayer": {"summonerName": "Tester", "riotId": "Tester#NA1", "riotIdGameName": "Tester", "level": 9}"#,
                r#""activePlayer": {"error": "Spectator mode doesn't currently support this feature"}"#,
            );
        let s = parse_all_game_data(&body).unwrap();
        assert!(s.me.is_empty());
        let kinds: Vec<_> = s.events.iter().map(|e| event_kind(&s, e)).collect();
        assert_eq!(
            kinds,
            vec![
                None,
                Some(EventKind::Objective {
                    what: Objective::Dragon,
                    ally: None
                }),
                Some(EventKind::Objective {
                    what: Objective::Turret,
                    ally: None
                }),
                None,
            ]
        );
    }

    #[test]
    fn the_diff_emits_each_event_id_once_at_its_game_time() {
        let mut d = LeagueDiff::default();
        // Started mid match: earlier events still come through, dated back.
        let s = parse_all_game_data(&all_game_data(205.0, &[START, MY_KILL].join(","))).unwrap();
        assert_eq!(
            d.events(&s, 1_000_000),
            vec![
                WatchEvent {
                    at_ms: 1_000_000 - 204_950,
                    kind: EventKind::MatchStart
                },
                WatchEvent {
                    at_ms: 1_000_000 - 4_500,
                    kind: EventKind::Kill
                },
            ]
        );
        // The same list again adds nothing.
        assert_eq!(d.events(&s, 1_001_000), vec![]);
        // Only the new IDs come through, even out of order in the list.
        let s = parse_all_game_data(&all_game_data(
            310.0,
            &[OTHER_KILL, START, MY_DEATH, MY_KILL].join(","),
        ))
        .unwrap();
        assert_eq!(
            d.events(&s, 1_100_000),
            vec![WatchEvent {
                at_ms: 1_090_000,
                kind: EventKind::Death
            }]
        );
        // A new match restarts the IDs: its events are new again.
        let s = parse_all_game_data(&all_game_data(1.0, START)).unwrap();
        assert_eq!(
            d.events(&s, 2_000_000),
            vec![WatchEvent {
                at_ms: 1_999_050,
                kind: EventKind::MatchStart
            }]
        );
    }

    #[test]
    fn only_leagues_game_process_counts() {
        assert!(league_game_running(&[
            "explorer.exe",
            r"C:\Riot Games\League of Legends\Game\League of Legends.exe"
        ]));
        assert!(league_game_running(&["League of Legen"]));
        assert!(!league_game_running(&[
            "LeagueClient.exe",
            "LeagueClientUx.exe",
            "RiotClientServices.exe"
        ]));
    }

    /// Plays back canned responses; `None` is a closed port.
    struct Canned(Vec<Option<String>>, u32);

    impl LiveClient for Canned {
        fn all_game_data(&mut self) -> Option<String> {
            self.1 += 1;
            if self.0.is_empty() {
                None
            } else {
                self.0.remove(0)
            }
        }
    }

    #[test]
    fn the_poller_idles_without_the_game_and_backs_off_while_the_port_is_closed() {
        let mut p = LeaguePoller::default();
        let mut c = Canned(
            vec![
                None,
                None,
                None,
                None,
                None,
                None,
                Some(all_game_data(205.0, &[START, MY_KILL].join(","))),
                Some(all_game_data(206.0, &[START, MY_KILL].join(","))),
                None,
            ],
            0,
        );
        // No game process: no request at all.
        assert_eq!(p.tick(false, &mut c, 0).next_in_ms, IDLE_MS);
        assert_eq!(c.1, 0);
        // Loading screen: the port is closed and the wait doubles to the cap.
        let waits: Vec<u64> = (0..6).map(|_| p.tick(true, &mut c, 0).next_in_ms).collect();
        assert_eq!(waits, vec![2_000, 4_000, 8_000, 16_000, 16_000, 16_000]);
        // The match is up: one-second polls, events once.
        let t = p.tick(true, &mut c, 500_000);
        assert_eq!(t.next_in_ms, POLL_MS);
        assert_eq!(t.events.len(), 2);
        assert_eq!(p.tick(true, &mut c, 501_000).events, vec![]);
        // A miss after a good read starts the backoff over.
        assert_eq!(p.tick(true, &mut c, 502_000).next_in_ms, 2_000);
        assert_eq!(c.1, 9);
    }

    #[test]
    fn a_closed_game_resets_the_diff_for_the_next_match() {
        let mut p = LeaguePoller::default();
        let body = all_game_data(205.0, &[START, MY_KILL].join(","));
        let mut c = Canned(vec![Some(body.clone()), Some(body)], 0);
        assert_eq!(p.tick(true, &mut c, 1_000_000).events.len(), 2);
        p.tick(false, &mut c, 1_001_000);
        // Same IDs after the game closed and reopened: a new match.
        assert_eq!(p.tick(true, &mut c, 2_000_000).events.len(), 2);
    }
}
