//! Rocket League adapter for the game watcher, through Psyonix's official
//! Stats API: while the game runs and the user has turned the API on in the
//! game's own `DefaultStatsAPI.ini` (`PacketSendRate` above 0), the game
//! sends JSON messages on a local TCP port (49123 by default). Loadout opens
//! that loopback socket, reads, and never sends anything on it. Nothing
//! reads game memory, injects, hooks or writes into the game folder (the
//! game runs Easy Anti-Cheat), and Loadout never edits the ini itself.
//!
//! Each message is `{"Event": "...", "Data": {...}}`; some builds send
//! `Data` as a JSON string, which is read the same way. Unknown events are
//! skipped. Only the player the game is showing (`Game.Target`, the local
//! player while playing) counts as "you".

use std::io::{self, Read};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::Value;

use crate::update_feed::{digest_card, UpdateCard};

/// Rocket League on Steam.
pub const APP_ID: u32 = 252950;
pub const NAME: &str = "Rocket League";
/// The Stats API's default TCP port.
pub const PORT: u16 = 49123;
/// The line the user sets in the game's Stats API ini to turn it on.
pub const INI_LINE: &str = "PacketSendRate=30";
/// Biggest buffered message; anything longer isn't a Stats API message.
const MAX_BUFFER: usize = 1 << 20;
/// Most bytes read in one poll.
const READ_CAP: usize = 256 * 1024;

/// One Stats API message the watcher uses.
#[derive(Debug, Clone, PartialEq)]
pub enum RlMessage {
    MatchStart,
    /// The game's state: the player shown, scores and the shown player's
    /// stat line.
    State(RlState),
    Goal {
        scorer: Option<String>,
        assister: Option<String>,
    },
    /// `WinnerTeamNum`.
    MatchEnded(Option<u32>),
    MatchDestroyed,
}

/// From `UpdateState`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RlState {
    pub me: Option<String>,
    pub my_team: Option<u32>,
    pub arena: Option<String>,
    /// Goals per team number.
    pub team_scores: Vec<(u32, u32)>,
    pub line: Option<StatLine>,
}

/// The shown player's totals for the match.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatLine {
    pub score: u32,
    pub goals: u32,
    pub assists: u32,
    pub saves: u32,
    pub shots: u32,
    pub demos: u32,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn n(v: &Value, k: &str) -> u32 {
    v.get(k)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

/// Parse one message.
pub fn parse_message(v: &Value) -> Option<RlMessage> {
    let event = v.get("Event")?.as_str()?;
    let data = match v.get("Data") {
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or(Value::Null),
        Some(d) => d.clone(),
        None => Value::Null,
    };
    match event {
        "MatchCreated" | "MatchInitialized" => Some(RlMessage::MatchStart),
        "UpdateState" => Some(RlMessage::State(parse_state(&data))),
        "GoalScored" => Some(RlMessage::Goal {
            scorer: data.get("Scorer").and_then(|p| s(p, "Name")),
            assister: data.get("Assister").and_then(|p| s(p, "Name")),
        }),
        "MatchEnded" => Some(RlMessage::MatchEnded(
            data.get("WinnerTeamNum")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok()),
        )),
        "MatchDestroyed" => Some(RlMessage::MatchDestroyed),
        _ => None,
    }
}

fn parse_state(data: &Value) -> RlState {
    let game = data.get("Game").unwrap_or(&Value::Null);
    let has_target = game
        .get("bHasTarget")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let target = game.get("Target").filter(|_| has_target);
    let me = target.and_then(|t| s(t, "Name"));
    let mut my_team = target
        .and_then(|t| t.get("TeamNum"))
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());
    let team_scores = game
        .get("Teams")
        .and_then(Value::as_array)
        .map(|teams| {
            teams
                .iter()
                .map(|t| (n(t, "TeamNum"), n(t, "Score")))
                .collect()
        })
        .unwrap_or_default();
    let mut line = None;
    if let Some(me) = &me {
        let players = data.get("Players").and_then(Value::as_array);
        if let Some(p) = players
            .into_iter()
            .flatten()
            .find(|p| s(p, "Name").as_deref() == Some(me))
        {
            if my_team.is_none() {
                my_team = Some(n(p, "TeamNum"));
            }
            line = Some(StatLine {
                score: n(p, "Score"),
                goals: n(p, "Goals"),
                assists: n(p, "Assists"),
                saves: n(p, "Saves"),
                shots: n(p, "Shots"),
                demos: n(p, "Demos"),
            });
        }
    }
    RlState {
        me,
        my_team,
        arena: s(game, "Arena"),
        team_scores,
        line,
    }
}

/// Splits the byte stream into whole JSON messages. Messages can arrive
/// several to a read or split across reads.
#[derive(Debug, Default)]
pub struct MessageStream {
    buf: Vec<u8>,
}

impl MessageStream {
    /// Add bytes; get every whole message they finish.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Value> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        let mut used = 0;
        let mut iter = serde_json::Deserializer::from_slice(&self.buf).into_iter::<Value>();
        loop {
            match iter.next() {
                Some(Ok(v)) => {
                    used = iter.byte_offset();
                    out.push(v);
                }
                Some(Err(e)) if e.is_eof() => break,
                Some(Err(_)) => {
                    // Not JSON: drop what's buffered and start clean.
                    used = self.buf.len();
                    break;
                }
                None => {
                    used = self.buf.len();
                    break;
                }
            }
        }
        self.buf.drain(..used);
        if self.buf.len() > MAX_BUFFER {
            self.buf.clear();
        }
        out
    }
}

/// One match, from the messages since it began.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RlMatch {
    pub me: Option<String>,
    pub my_team: Option<u32>,
    pub arena: Option<String>,
    pub team_scores: Vec<(u32, u32)>,
    pub line: StatLine,
    /// `Some` once the match ended: whether the shown player's team won.
    pub won: Option<bool>,
}

impl RlMatch {
    fn score(&self, mine: bool) -> u32 {
        let Some(team) = self.my_team else {
            return 0;
        };
        self.team_scores
            .iter()
            .filter(|(t, _)| (*t == team) == mine)
            .map(|(_, s)| *s)
            .sum()
    }

    /// Nothing worth a card: no state ever came in.
    pub fn is_empty(&self) -> bool {
        self.me.is_none() && self.team_scores.is_empty()
    }

    /// `Won 3-1 in DFH Stadium. 2 goals, 1 assist, 3 saves, 5 shots, 1 demo. Score 640.`
    pub fn summary(&self) -> String {
        let (us, them) = (self.score(true), self.score(false));
        let mut first = match self.won {
            Some(true) => format!("Won {us}-{them}"),
            Some(false) => format!("Lost {us}-{them}"),
            None => format!("{us}-{them} so far"),
        };
        if let Some(arena) = &self.arena {
            first.push_str(&format!(" in {}", arena_name(arena)));
        }
        let l = self.line;
        let count =
            |n: u32, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let stats = [
            count(l.goals, "goal", "goals"),
            count(l.assists, "assist", "assists"),
            count(l.saves, "save", "saves"),
            count(l.shots, "shot", "shots"),
            count(l.demos, "demo", "demos"),
        ]
        .join(", ");
        format!("{first}. {stats}. Score {}.", l.score)
    }
}

/// `Stadium_P` → `Stadium`: the map's internal name without its suffix.
fn arena_name(arena: &str) -> String {
    let base = arena.split('_').next().unwrap_or(arena);
    if base.is_empty() {
        arena.to_owned()
    } else {
        base.to_owned()
    }
}

/// The match as a Pulse card. Player names stay out of it.
pub fn match_card(m: &RlMatch, at_ms: u64) -> UpdateCard {
    let (us, them) = (m.score(true), m.score(false));
    let title = match m.won {
        Some(true) => format!("{NAME} match: won {us}-{them}"),
        Some(false) => format!("{NAME} match: lost {us}-{them}"),
        None => format!("{NAME} match"),
    };
    digest_card(
        "source:game-watch:rocket-league",
        &title,
        &m.summary(),
        at_ms,
    )
}

/// Messages into matches and watch events.
#[derive(Debug, Default)]
pub struct RlTracker {
    current: RlMatch,
    in_match: bool,
}

impl RlTracker {
    pub fn current(&self) -> Option<&RlMatch> {
        (self.in_match && !self.current.is_empty()).then_some(&self.current)
    }

    /// Take one message; a match that ended comes back to post. Goals and
    /// results stay in the match's own card rather than the round-based
    /// match recap the other watcher games share.
    pub fn take(&mut self, msg: RlMessage) -> Option<RlMatch> {
        match msg {
            RlMessage::MatchStart => {
                if !self.in_match {
                    self.current = RlMatch::default();
                    self.in_match = true;
                }
            }
            RlMessage::State(st) => {
                if !self.in_match {
                    self.current = RlMatch::default();
                    self.in_match = true;
                }
                let m = &mut self.current;
                if st.me.is_some() {
                    m.me = st.me;
                    m.my_team = st.my_team.or(m.my_team);
                }
                if st.arena.is_some() {
                    m.arena = st.arena;
                }
                if !st.team_scores.is_empty() {
                    m.team_scores = st.team_scores;
                }
                if let Some(line) = st.line {
                    m.line = line;
                }
            }
            // The totals in the next state carry the goal.
            RlMessage::Goal { .. } => {}
            RlMessage::MatchEnded(winner) => {
                if self.in_match {
                    self.in_match = false;
                    let m = &mut self.current;
                    m.won = winner.zip(m.my_team).map(|(w, t)| w == t);
                    return (!m.is_empty()).then(|| m.clone());
                }
            }
            RlMessage::MatchDestroyed => self.in_match = false,
        }
        None
    }
}

/// The loopback reader: connects to the Stats API port while the game
/// runs, reads without blocking, and sends nothing.
#[derive(Debug, Default)]
pub struct RlClient {
    sock: Option<TcpStream>,
    stream: MessageStream,
}

impl RlClient {
    /// Whether the socket is open.
    pub fn connected(&self) -> bool {
        self.sock.is_some()
    }

    /// Try the port on 127.0.0.1 once. `false` while the game has the API
    /// off or isn't up yet.
    pub fn connect(&mut self, port: u16) -> bool {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let Ok(sock) = TcpStream::connect_timeout(&addr, Duration::from_millis(200)) else {
            return false;
        };
        if sock.set_nonblocking(true).is_err() {
            return false;
        }
        self.sock = Some(sock);
        self.stream = MessageStream::default();
        true
    }

    pub fn close(&mut self) {
        self.sock = None;
        self.stream = MessageStream::default();
    }

    /// Whole messages that arrived since the last read. A closed or broken
    /// socket closes the client.
    pub fn read(&mut self) -> Vec<Value> {
        let Some(sock) = self.sock.as_mut() else {
            return Vec::new();
        };
        let mut got = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        while got.len() < READ_CAP {
            match sock.read(&mut chunk) {
                Ok(0) => {
                    self.sock = None;
                    break;
                }
                Ok(n) => got.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.sock = None;
                    break;
                }
            }
        }
        self.stream.push(&got)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Made-up players; the shapes follow Psyonix's Stats API page.
    fn state(my_goals: u32, blue: u32, orange: u32) -> String {
        format!(
            r#"{{"Event":"UpdateState","Data":{{"MatchGuid":"G1","Players":[{{"Name":"Testdriver","PrimaryId":"Steam|1|0","Shortcut":1,"TeamNum":0,"Score":{},"Goals":{my_goals},"Shots":5,"Assists":1,"Saves":3,"Touches":20,"Demos":1}},{{"Name":"Mate","TeamNum":0,"Score":100,"Goals":1}},{{"Name":"Rival","TeamNum":1,"Score":300,"Goals":{orange}}}],"Game":{{"Teams":[{{"Name":"Blue","TeamNum":0,"Score":{blue}}},{{"Name":"Orange","TeamNum":1,"Score":{orange}}}],"TimeSeconds":120,"bOvertime":false,"Arena":"Stadium_P","bHasTarget":true,"Target":{{"Name":"Testdriver","Shortcut":1,"TeamNum":0}}}}}}}}"#,
            my_goals * 200 + 240
        )
    }
    const CREATED: &str = r#"{"Event":"MatchCreated","Data":{"MatchGuid":"G1"}}"#;
    const MY_GOAL: &str = r#"{"Event":"GoalScored","Data":{"MatchGuid":"G1","GoalSpeed":90.5,"Scorer":{"Name":"Testdriver","Shortcut":1,"TeamNum":0},"Assister":{"Name":"Mate","Shortcut":2,"TeamNum":0}}}"#;
    const MATE_GOAL: &str = r#"{"Event":"GoalScored","Data":{"MatchGuid":"G1","Scorer":{"Name":"Mate","Shortcut":2,"TeamNum":0}}}"#;
    const ENDED: &str = r#"{"Event":"MatchEnded","Data":{"MatchGuid":"G1","WinnerTeamNum":0}}"#;

    fn msgs(texts: &[&str]) -> Vec<RlMessage> {
        texts
            .iter()
            .map(|t| parse_message(&serde_json::from_str(t).unwrap()).unwrap())
            .collect()
    }

    #[test]
    fn messages_parse_including_data_sent_as_a_string() {
        let m = msgs(&[CREATED, MY_GOAL, ENDED]);
        assert_eq!(m[0], RlMessage::MatchStart);
        assert_eq!(
            m[1],
            RlMessage::Goal {
                scorer: Some("Testdriver".into()),
                assister: Some("Mate".into())
            }
        );
        assert_eq!(m[2], RlMessage::MatchEnded(Some(0)));
        let as_string = serde_json::json!({
            "Event": "MatchEnded",
            "Data": "{\"MatchGuid\":\"G1\",\"WinnerTeamNum\":1}"
        });
        assert_eq!(
            parse_message(&as_string),
            Some(RlMessage::MatchEnded(Some(1)))
        );
        let st = state(2, 3, 1);
        let RlMessage::State(s) = &msgs(&[st.as_str()])[0] else {
            panic!("not a state");
        };
        assert_eq!(s.me.as_deref(), Some("Testdriver"));
        assert_eq!(s.my_team, Some(0));
        assert_eq!(s.line.unwrap().goals, 2);
        let unknown: Value = serde_json::from_str(r#"{"Event":"BallHit","Data":{}}"#).unwrap();
        assert_eq!(parse_message(&unknown), None);
    }

    #[test]
    fn the_stream_splits_joined_and_partial_messages_and_drops_junk() {
        let mut s = MessageStream::default();
        let joined = format!("{CREATED}{MY_GOAL}\n{}", &ENDED[..10]);
        let got = s.push(joined.as_bytes());
        assert_eq!(got.len(), 2);
        let got = s.push(&ENDED.as_bytes()[10..]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["Event"], "MatchEnded");
        assert!(s.push(b"not json at all").is_empty());
        assert_eq!(s.push(CREATED.as_bytes()).len(), 1);
    }

    #[test]
    fn a_match_counts_only_the_shown_players_goals_and_posts_at_the_end() {
        let mut t = RlTracker::default();
        let s1 = state(0, 0, 0);
        let s2 = state(2, 3, 1);
        for m in msgs(&[CREATED, s1.as_str(), MY_GOAL, MATE_GOAL, s2.as_str()]) {
            assert_eq!(t.take(m), None);
        }
        assert_eq!(
            t.current().unwrap().summary(),
            "3-1 so far in Stadium. 2 goals, 1 assist, 3 saves, 5 shots, 1 demo. Score 640."
        );
        let m = t.take(msgs(&[ENDED]).remove(0)).unwrap();
        assert_eq!(m.won, Some(true));
        let card = match_card(&m, 5);
        assert_eq!(card.title, "Rocket League match: won 3-1");
        let body = card.body.as_deref().unwrap_or("");
        assert!(body.starts_with("Won 3-1 in Stadium."));
        assert!(!body.contains("Testdriver") && !body.contains("Rival"));
        assert_eq!(t.current(), None);
    }

    #[test]
    fn a_left_match_posts_nothing() {
        let mut t = RlTracker::default();
        let s1 = state(1, 1, 0);
        for m in msgs(&[CREATED, s1.as_str()]) {
            t.take(m);
        }
        assert_eq!(t.take(RlMessage::MatchDestroyed), None);
        assert_eq!(t.take(msgs(&[ENDED]).remove(0)), None);
    }

    #[test]
    fn the_client_reads_from_the_loopback_port_and_sends_nothing() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut c = RlClient::default();
        assert!(c.connect(port));
        let (mut game, _) = listener.accept().unwrap();
        game.write_all(format!("{CREATED}{}", &MY_GOAL[..20]).as_bytes())
            .unwrap();
        let mut got = Vec::new();
        for _ in 0..200 {
            got.extend(c.read());
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got.len(), 1);
        game.write_all(&MY_GOAL.as_bytes()[20..]).unwrap();
        drop(game);
        let mut more = Vec::new();
        for _ in 0..200 {
            more.extend(c.read());
            if !c.connected() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(more.len(), 1);
        assert!(!c.connected());
    }

    #[test]
    fn a_closed_port_does_not_connect() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(!RlClient::default().connect(port));
    }
}
