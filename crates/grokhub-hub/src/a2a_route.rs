//! `/a2a/v1`: agent cards and A2A JSON-RPC (`SendMessage`, `GetTask`,
//! `ListTasks`, `CancelTask`) for paired computers, for the agents shared
//! with them (`share` on, and the computer in `share_with`). The hub checks
//! the pairing token and that sharing is on before a request gets here.
//!
//! The app that runs the hub answers each message through [`PeerTurns`]. A
//! hub without one (the standalone `grokhub-hub`) has no `/a2a` routes.
//! Tasks live in memory, one book per paired computer (by id and token) and
//! agent, so a computer only ever sees its own tasks.
//!
//! Who may reach an agent is checked again where it matters: when a turn is
//! about to start (after the body is read) and before its answer goes out.
//! [`A2aDoor::recheck`] cancels the turns of whoever may no longer reach the
//! agent. Locks: the hub state's is never held while the door's is taken.

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use grokhub_agent::CancelToken;
use grokhub_core::a2a::{self, A2aError, Call, Outcome, TaskBook};
use grokhub_core::agent_team::{Sender, TeamFile};
use grokhub_core::state::Peer;
use grokhub_core::HubState;
use serde_json::{json, Value};
use tiny_http::{Method, Request};

use crate::server::{from_web_page, not_found, send_json};

pub const A2A_PATH: &str = "/a2a/v1";
/// Tasks one book keeps; ended ones give way to new ones.
const BOOK_CAP: usize = 16;
/// Tasks working at once for one paired computer, and for all of them.
const PER_PEER: usize = 2;
const ALL_PEERS: usize = 4;
/// Messages one paired computer may send an hour.
const PER_HOUR: usize = 20;
const HOUR: Duration = Duration::from_secs(60 * 60);
/// Largest JSON-RPC request body: a message is at most 64 KiB of parts.
const MAX_BODY: usize = 96 * 1024;
/// What a computer hears when it may no longer reach the agent as its
/// message is about to start.
const NOT_SHARED: &str = "this computer stopped sharing that agent with yours";
/// What a task ends with when its turn panics or can't start.
const TURN_FAILED: &str = "the agent couldn't answer";

/// Runs one agent turn for a paired computer's message, and blocks until it
/// ends. The app implements it with `grokhub_agent::answer_peer`.
pub trait PeerTurns: Send + Sync {
    fn run(&self, ask: PeerAsk) -> Outcome;

    /// The longest a turn runs. A blocking `SendMessage` waits this long for
    /// the end, as A2A asks, and then answers with an error, never with a
    /// task still working.
    fn time_limit(&self) -> Duration {
        Duration::from_secs(20 * 60)
    }
}

/// One message to answer.
pub struct PeerAsk {
    /// The paired computer's device id.
    pub peer: String,
    pub agent: String,
    pub text: String,
    /// Canceled by `CancelTask`, or by [`A2aDoor::recheck`] once the
    /// computer may no longer reach the agent.
    pub cancel: CancelToken,
}

/// A paired computer as the hub knows it now: a computer that pairs again
/// gets a new token, and with it a fresh set of books.
type PeerKey = (String, String);

/// The route's state: the team file's folder, the turns, and the tasks.
pub struct A2aDoor {
    config: PathBuf,
    turns: Arc<dyn PeerTurns>,
    /// This computer's LAN address, for the card's URL when the request
    /// doesn't name a usable host.
    lan: fn() -> Option<IpAddr>,
    /// The app's version, for the card.
    version: String,
    desk: Mutex<Desk>,
    /// Signalled whenever a task's turn ends.
    ended: Condvar,
}

#[derive(Default)]
struct Desk {
    /// One book per computer and agent.
    books: HashMap<(PeerKey, String), TaskBook>,
    /// The cancel token of each task whose turn is running.
    running: HashMap<(PeerKey, String, String), CancelToken>,
    /// When each computer's messages of the last hour started, on a clock
    /// that never steps back.
    sent: HashMap<PeerKey, VecDeque<Instant>>,
}

impl A2aDoor {
    pub fn new(
        config: PathBuf,
        turns: Arc<dyn PeerTurns>,
        lan: fn() -> Option<IpAddr>,
        version: &str,
    ) -> A2aDoor {
        A2aDoor {
            config,
            turns,
            lan,
            version: version.to_string(),
            desk: Mutex::new(Desk::default()),
            ended: Condvar::new(),
        }
    }

    /// Cancels the turns, and forgets the tasks, of every computer that may
    /// no longer reach the agent: sharing is off, the computer isn't paired
    /// with that token any more, or the agent isn't shared with it. The app
    /// calls it after it stops sharing, unpairs a computer or saves the
    /// team; the hub after a pairing and on every `/a2a` request. Never call
    /// it while holding the hub state's lock.
    pub fn recheck(&self, state: &Mutex<HubState>) {
        let _ = self.sweep(state);
    }

    /// [`A2aDoor::recheck`], giving back the team file it read.
    fn sweep(&self, state: &Mutex<HubState>) -> TeamFile {
        let (sharing, live) = {
            let st = lock(state);
            let live: Vec<PeerKey> = st
                .peers
                .iter()
                .map(|p| (p.id.clone(), p.token.clone()))
                .collect();
            (st.sharing, live)
        };
        let team = TeamFile::load(&self.config).unwrap_or_default();
        let reaches = |key: &PeerKey, agent: &str| {
            sharing
                && live.contains(key)
                && team.may_message(Sender::Peer(&key.0), agent, &[]).is_ok()
        };
        let mut desk = self.desk();
        for ((key, agent, _), token) in &desk.running {
            if !reaches(key, agent) {
                token.cancel();
            }
        }
        desk.books.retain(|(key, agent), _| reaches(key, agent));
        desk.sent.retain(|key, _| live.contains(key));
        drop(desk);
        team
    }

    /// Whether `key` may still reach `agent`: sharing on, the computer
    /// paired with this token, and the agent shared with it.
    fn admits(&self, state: &Mutex<HubState>, key: &PeerKey, agent: &str) -> bool {
        let (sharing, paired) = {
            let st = lock(state);
            let paired = st.peers.iter().any(|p| p.id == key.0 && p.token == key.1);
            (st.sharing, paired)
        };
        sharing
            && paired
            && TeamFile::load(&self.config)
                .is_ok_and(|team| team.may_message(Sender::Peer(&key.0), agent, &[]).is_ok())
    }

    fn desk(&self) -> MutexGuard<'_, Desk> {
        self.desk.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn lock(state: &Mutex<HubState>) -> MutexGuard<'_, HubState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where this computer's agents are, as the paired computer reached them:
/// its `Host` header when that is a plain host and port, else the LAN
/// address and the hub's port, else loopback.
fn base_url(req: &Request, port: u16, lan: fn() -> Option<IpAddr>) -> String {
    let host = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Host"))
        .map(|h| h.value.as_str().trim());
    match host.and_then(authority) {
        Some((host, Some(_))) => format!("http://{host}"),
        Some((host, None)) => format!("http://{host}:{port}"),
        None => {
            let ip = lan().unwrap_or(IpAddr::from([127, 0, 0, 1]));
            format!("http://{}", SocketAddr::new(ip, port))
        }
    }
}

/// `host[:port]` and its port, when the host is a name, an IPv4 address or
/// a bracketed IPv6 address and the port is digits, and nothing else.
fn authority(raw: &str) -> Option<(&str, Option<u16>)> {
    let (host_ok, port) = match raw.strip_prefix('[') {
        Some(rest) => {
            let (inside, after) = rest.split_once(']')?;
            let ok = !inside.is_empty()
                && inside
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
            let port = match after {
                "" => None,
                _ => Some(after.strip_prefix(':')?),
            };
            (ok, port)
        }
        None => {
            let (host, port) = match raw.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (raw, None),
            };
            let ok = !host.is_empty()
                && host.len() <= 253
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
            (ok, port)
        }
    };
    if !host_ok {
        return None;
    }
    let port = match port {
        None => None,
        Some(p) if (1..=5).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()) => {
            Some(p.parse::<u16>().ok().filter(|p| *p > 0)?)
        }
        Some(_) => return None,
    };
    Some((raw, port))
}

/// Serves one `/a2a/v1` request from `peer`, whose token and sharing the hub
/// has checked; `port` is the hub's. The hub state's lock is not held.
pub(crate) fn handle(
    door: &Arc<A2aDoor>,
    mut req: Request,
    path: &str,
    peer: &Peer,
    state: &Arc<Mutex<HubState>>,
    port: u16,
) -> Result<(), ()> {
    // Paired computers call from the app; a web page could ride a browser
    // that can reach the LAN.
    if from_web_page(&req) {
        return send_json(
            req,
            403,
            json!({ "ok": false, "error": "Agents answer the app on a paired computer, not a web page." }),
        );
    }
    let key: PeerKey = (peer.id.clone(), peer.token.clone());
    let team = door.sweep(state);
    let shared = |id: &str| team.may_message(Sender::Peer(&peer.id), id, &[]).is_ok();
    let base = base_url(&req, port, door.lan);
    let card_url = |agent: &str| format!("{base}{A2A_PATH}/{agent}");
    let method = req.method().clone();
    if path == A2A_PATH {
        if method != Method::Get {
            return not_found(req);
        }
        let agents: Vec<Value> = team
            .agents
            .iter()
            .filter(|p| shared(&p.id))
            .map(|p| json!({ "id": p.id, "name": p.name, "url": card_url(&p.id) }))
            .collect();
        return send_json(req, 200, json!({ "ok": true, "agents": agents }));
    }
    let agent = path
        .strip_prefix(A2A_PATH)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or_default();
    // An agent that isn't shared with this computer looks like no route at all.
    let Some(profile) = team.agent(agent).filter(|p| shared(&p.id)) else {
        return not_found(req);
    };
    match method {
        Method::Get => {
            let card = a2a::card(profile, &card_url(agent), &door.version);
            send_json(req, 200, card)
        }
        Method::Post => {
            let version = req
                .headers()
                .iter()
                .find(|h| h.field.equiv(a2a::VERSION_HEADER))
                .map(|h| h.value.as_str().to_string());
            if let Err(e) = a2a::check_version(version.as_deref()) {
                return send_json(req, 200, a2a::refuse(Value::Null, &e));
            }
            let mut body = Vec::new();
            if req
                .as_reader()
                .take(MAX_BODY as u64 + 1)
                .read_to_end(&mut body)
                .is_err()
                || body.len() > MAX_BODY
            {
                let e = A2aError::InvalidRequest(format!(
                    "a request is at most {} KiB",
                    MAX_BODY / 1024
                ));
                return send_json(req, 200, a2a::refuse(Value::Null, &e));
            }
            let reply = match a2a::parse_call(&body) {
                Ok((id, call)) => match door.call(state, &key, agent, call) {
                    Ok(result) => a2a::answer(id, result),
                    Err(e) => a2a::refuse(id, &e),
                },
                Err((id, e)) => a2a::refuse(id, &e),
            };
            send_json(req, 200, reply)
        }
        _ => not_found(req),
    }
}

impl A2aDoor {
    fn call(
        self: &Arc<Self>,
        state: &Mutex<HubState>,
        key: &PeerKey,
        agent: &str,
        call: Call,
    ) -> Result<Value, A2aError> {
        let book_key = (key.clone(), agent.to_string());
        let task_json = |task: a2a::Task| serde_json::to_value(task).unwrap_or(Value::Null);
        match call {
            Call::SendMessage { message, config } => self.send(state, key, agent, message, config),
            Call::GetTask { id, history_length } => {
                let desk = self.desk();
                let book = desk
                    .books
                    .get(&book_key)
                    .ok_or_else(|| A2aError::TaskNotFound(id.clone()))?;
                book.get(&id, history_length).map(task_json)
            }
            Call::ListTasks(query) => {
                let desk = self.desk();
                let page = match desk.books.get(&book_key) {
                    Some(book) => book.list(&query)?,
                    None => TaskBook::new(BOOK_CAP).list(&query)?,
                };
                Ok(serde_json::to_value(page).unwrap_or(Value::Null))
            }
            Call::CancelTask { id } => {
                let mut desk = self.desk();
                let book = desk
                    .books
                    .get_mut(&book_key)
                    .ok_or_else(|| A2aError::TaskNotFound(id.clone()))?;
                let task = book.cancel(&id, grokhub_core::now_ms())?;
                let run = (key.clone(), agent.to_string(), id);
                if let Some(token) = desk.running.get(&run) {
                    token.cancel();
                }
                Ok(task_json(task))
            }
        }
    }

    fn send(
        self: &Arc<Self>,
        state: &Mutex<HubState>,
        key: &PeerKey,
        agent: &str,
        message: a2a::Message,
        config: a2a::SendConfig,
    ) -> Result<Value, A2aError> {
        let text = message.plain_text();
        let max = grokhub_agent::AGENT_MESSAGE_CHARS;
        if text.chars().count() > max {
            return Err(A2aError::InvalidParams(format!(
                "a message to an agent is at most {max} characters"
            )));
        }
        let now = grokhub_core::now_ms();
        let tick = Instant::now();
        let mut desk = self.desk();
        let busy = |desk: &Desk| desk.running.keys().filter(|(k, _, _)| k == key).count();
        if busy(&desk) >= PER_PEER {
            return Err(A2aError::UnsupportedOperation(format!(
                "{PER_PEER} messages from this computer are already being answered; wait for one to end"
            )));
        }
        if desk.running.len() >= ALL_PEERS {
            return Err(A2aError::UnsupportedOperation(
                "this computer is answering as many messages as it can; try again soon".into(),
            ));
        }
        let sent = desk.sent.entry(key.clone()).or_default();
        while sent
            .front()
            .is_some_and(|at| tick.duration_since(*at) >= HOUR)
        {
            sent.pop_front();
        }
        if sent.len() >= PER_HOUR {
            return Err(A2aError::UnsupportedOperation(format!(
                "this computer takes {PER_HOUR} messages an hour from each paired computer"
            )));
        }
        let book_key = (key.clone(), agent.to_string());
        let book = desk
            .books
            .entry(book_key.clone())
            .or_insert_with(|| TaskBook::new(BOOK_CAP));
        let id = book.open(message, now)?;
        book.start(&id, now)?;
        desk.sent.entry(key.clone()).or_default().push_back(tick);
        let cancel = CancelToken::new();
        let run = (key.clone(), agent.to_string(), id.clone());
        desk.running.insert(run.clone(), cancel.clone());
        drop(desk);
        let mut slot = Slot {
            door: Arc::clone(self),
            book_key: book_key.clone(),
            run: run.clone(),
            ended: false,
        };
        // Sharing, pairing or the share list may have changed while the body
        // was read. A change after this check finds the turn in `running`.
        if !self.admits(state, key, agent) {
            slot.end(Outcome::Rejected(NOT_SHARED.into()));
            return Err(A2aError::UnsupportedOperation(NOT_SHARED.into()));
        }
        let ask = PeerAsk {
            peer: key.0.clone(),
            agent: agent.to_string(),
            text,
            cancel,
        };
        let turns = Arc::clone(&self.turns);
        let started = std::thread::Builder::new()
            .name("a2a-turn".into())
            .spawn(move || {
                let outcome = turns.run(ask);
                slot.end(outcome);
            });
        if started.is_err() {
            // The slot went down with the closure and failed the task.
            return Err(A2aError::Internal(TURN_FAILED.into()));
        }

        let deadline = Instant::now() + self.turns.time_limit();
        let mut desk = self.desk();
        let waiting = |desk: &Desk| {
            desk.books
                .get(&book_key)
                .and_then(|book| book.get(&id, None).ok())
                .is_some_and(|task| {
                    !task.status.state.is_terminal() && !task.status.state.is_interrupted()
                })
        };
        if !config.return_immediately {
            while waiting(&desk) {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(A2aError::Internal(format!(
                        "the agent is still working on {id}; get it with GetTask"
                    )));
                }
                desk = self
                    .ended
                    .wait_timeout(desk, left)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        }
        drop(desk);
        // Nothing goes back to a computer that may no longer reach the agent.
        if !self.admits(state, key, agent) {
            if let Some(token) = self.desk().running.get(&run) {
                token.cancel();
            }
            return Err(A2aError::TaskNotFound(id));
        }
        let desk = self.desk();
        let task = desk
            .books
            .get(&book_key)
            .ok_or_else(|| A2aError::TaskNotFound(id.clone()))?
            .get(&id, config.history_length)?;
        Ok(json!({ "task": task }))
    }
}

/// A running turn's place on the desk. Ending it finishes the task and
/// frees the place; dropping it unended, when the turn's thread panics or
/// never starts, fails the task.
struct Slot {
    door: Arc<A2aDoor>,
    book_key: (PeerKey, String),
    run: (PeerKey, String, String),
    ended: bool,
}

impl Slot {
    fn end(&mut self, outcome: Outcome) {
        if std::mem::replace(&mut self.ended, true) {
            return;
        }
        let mut desk = self.door.desk();
        if let Some(book) = desk.books.get_mut(&self.book_key) {
            let _ = book.finish(&self.run.2, outcome, grokhub_core::now_ms());
        }
        desk.running.remove(&self.run);
        drop(desk);
        self.door.ended.notify_all();
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.end(Outcome::Failed(TURN_FAILED.into()));
    }
}

#[cfg(test)]
#[path = "a2a_route_tests.rs"]
mod tests;
