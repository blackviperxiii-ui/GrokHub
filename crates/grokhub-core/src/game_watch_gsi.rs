//! Valve Game State Integration (GSI) listener for the in-game watcher
//! (design: `docs/design/game-watcher/`). Counter-Strike 2 and Dota 2 POST
//! JSON snapshots to a local URL named in a `gamestate_integration_*.cfg`
//! file in the game's own `cfg/` folder. That is Valve's documented feed for
//! this: nothing reads game memory, injects into or hooks the game, so there
//! is nothing for an anti-cheat to see.
//!
//! This module writes and removes Loadout's cfg file, keeps the auth token
//! the cfg names, and runs the listener: `127.0.0.1` only, one fixed port,
//! posts without the token or with another `Host` dropped, bodies capped.
//! Each accepted post is diffed against the last one from the same game into
//! `WatchEvent`s (`game_watch::cs2_events`, `game_watch_dota::dota_events`).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::Value;

use crate::game_watch::{self, Cs2State, WatchEvent};
use crate::game_watch_dota::{self, DotaState};

/// The one port the listener binds, on `127.0.0.1` only. Fixed because the
/// cfg file names it and the game reads that file once at launch. 29174 is
/// below the Linux (32768-60999) and Windows (49152-65535) ephemeral ranges,
/// so an outgoing connection never holds it, clear of Steam's own 27000-27100
/// range, and away from the 3000/4000/8080 ports other GSI tools and dev
/// servers default to.
pub const GSI_PORT: u16 = 29174;
/// The cfg file name, the same in every supported game.
pub const CFG_FILE: &str = "gamestate_integration_loadout.cfg";
/// The first line of every cfg Loadout writes. Removal checks for it, so a
/// file of the same name that Loadout didn't write is left alone.
pub const CFG_MARKER: &str =
    "// Written by Loadout's game watcher. Turning the watcher off in Loadout's Settings removes this file.";
/// The token file, under the config dir.
pub const TOKEN_FILE: &str = "game-watch/gsi-token";
/// The biggest POST body read. The cfg asks only for the few blocks the
/// watcher uses, which come to a few KB.
pub const MAX_BODY: usize = 64 * 1024;
/// The biggest request head read.
const MAX_HEAD: usize = 8 * 1024;
/// A client that stalls mid-request is dropped after this.
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// A game that sends GSI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GsiGame {
    Cs2,
    Dota2,
}

impl GsiGame {
    pub const ALL: [GsiGame; 2] = [GsiGame::Cs2, GsiGame::Dota2];

    pub fn app_id(self) -> u32 {
        match self {
            GsiGame::Cs2 => 730,
            GsiGame::Dota2 => 570,
        }
    }

    /// The game for a Steam app id, from `provider.appid` or what
    /// `game_companion` sees running.
    pub fn from_app_id(id: u32) -> Option<GsiGame> {
        GsiGame::ALL.into_iter().find(|g| g.app_id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            GsiGame::Cs2 => "Counter-Strike 2",
            GsiGame::Dota2 => "Dota 2",
        }
    }

    /// The install folder under `steamapps/common`.
    fn install_dir(self) -> &'static str {
        match self {
            GsiGame::Cs2 => "Counter-Strike Global Offensive",
            GsiGame::Dota2 => "dota 2 beta",
        }
    }

    /// The game's own `cfg` folder under its install folder.
    pub fn cfg_root(self, game_dir: &Path) -> PathBuf {
        let mod_dir = match self {
            GsiGame::Cs2 => "csgo",
            GsiGame::Dota2 => "dota",
        };
        game_dir.join("game").join(mod_dir).join("cfg")
    }

    /// Where the game looks for GSI cfg files: CS2 reads `cfg/`, Dota 2
    /// reads `cfg/gamestate_integration/`.
    pub fn cfg_path(self, game_dir: &Path) -> PathBuf {
        let root = self.cfg_root(game_dir);
        match self {
            GsiGame::Cs2 => root.join(CFG_FILE),
            GsiGame::Dota2 => root.join("gamestate_integration").join(CFG_FILE),
        }
    }

    /// The blocks the game is asked to send: only what the adapter reads.
    fn data_keys(self) -> &'static [&'static str] {
        match self {
            GsiGame::Cs2 => &[
                "provider",
                "map",
                "round",
                "player_id",
                "player_match_stats",
            ],
            GsiGame::Dota2 => &["provider", "map", "player", "hero"],
        }
    }

    /// The cfg text. `buffer` and `throttle` keep posts to about two a
    /// second at most, which is what keeps the listener far under 1% of a
    /// core during a match.
    pub fn cfg_text(self, token: &GsiToken, port: u16) -> String {
        let data: String = self
            .data_keys()
            .iter()
            .map(|k| format!("\t\t\"{k}\"\t\"1\"\n"))
            .collect();
        format!(
            "{CFG_MARKER}\n\"Loadout\"\n{{\n\t\"uri\"\t\"http://127.0.0.1:{port}/\"\n\t\"timeout\"\t\"5.0\"\n\t\"buffer\"\t\"0.1\"\n\t\"throttle\"\t\"0.5\"\n\t\"heartbeat\"\t\"30.0\"\n\t\"auth\"\n\t{{\n\t\t\"token\"\t\"{}\"\n\t}}\n\t\"data\"\n\t{{\n{data}\t}}\n}}\n",
            token.0
        )
    }
}

/// The per-install auth token the cfg names and the listener checks. Its
/// `Debug` never shows the value, so it can't end up in a log.
#[derive(Clone, PartialEq, Eq)]
pub struct GsiToken(String);

impl std::fmt::Debug for GsiToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GsiToken(..)")
    }
}

impl GsiToken {
    /// A fresh random token.
    pub fn new() -> GsiToken {
        GsiToken(crate::new_token())
    }

    /// The token saved under `config_dir`, made and saved on first use.
    pub fn load_or_create(config_dir: &Path) -> Result<GsiToken, String> {
        let path = config_dir.join(TOKEN_FILE);
        if let Ok(text) = std::fs::read_to_string(&path) {
            let text = text.trim();
            if text.len() == 48 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Ok(GsiToken(text.to_owned()));
            }
        }
        let token = GsiToken::new();
        crate::pulse_writer::write_atomic(&path, &token.0)
            .map_err(|e| format!("Couldn't save the game watcher's token: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        #[cfg(windows)]
        crate::win_acl::restrict_user_only(&path);
        Ok(token)
    }

    /// Compare in constant time, so response timing can't leak the token.
    fn matches(&self, other: &str) -> bool {
        let (a, b) = (self.0.as_bytes(), other.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

impl Default for GsiToken {
    fn default() -> Self {
        GsiToken::new()
    }
}

/// Each supported game installed in any Steam library under `steam_root`
/// (`libraryfolders.vdf`), with its install folder. A game counts only when
/// its own `cfg` folder is there.
pub fn installed_games(steam_root: &Path) -> Vec<(GsiGame, PathBuf)> {
    let libraries = crate::steam::library_folders(steam_root);
    let mut out = Vec::new();
    for game in GsiGame::ALL {
        let found = libraries
            .iter()
            .map(|lib| {
                lib.join("steamapps")
                    .join("common")
                    .join(game.install_dir())
            })
            .find(|dir| game.cfg_root(dir).is_dir());
        if let Some(dir) = found {
            out.push((game, dir));
        }
    }
    out
}

/// Write Loadout's cfg into one game's folder, through a temp file and a
/// rename so the game never reads half a file. Returns the path written.
pub fn write_cfg(
    game: GsiGame,
    game_dir: &Path,
    token: &GsiToken,
    port: u16,
) -> Result<PathBuf, String> {
    if !game.cfg_root(game_dir).is_dir() {
        return Err(format!("{}'s cfg folder isn't there.", game.name()));
    }
    let path = game.cfg_path(game_dir);
    crate::pulse_writer::write_atomic(&path, &game.cfg_text(token, port))
        .map_err(|e| format!("Couldn't write the {} game watcher file: {e}", game.name()))?;
    Ok(path)
}

/// Remove Loadout's cfg from one game's folder. Only a file that starts with
/// `CFG_MARKER` goes; nothing else in the folder is touched. True when a
/// file was removed.
pub fn remove_cfg(game: GsiGame, game_dir: &Path) -> Result<bool, String> {
    let path = game.cfg_path(game_dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(false);
    };
    if !text.starts_with(CFG_MARKER) {
        return Ok(false);
    }
    std::fs::remove_file(&path)
        .map(|()| true)
        .map_err(|e| format!("Couldn't remove the {} game watcher file: {e}", game.name()))
}

/// Write the cfg into every supported game found under `steam_root`.
pub fn install_cfgs(
    steam_root: &Path,
    token: &GsiToken,
    port: u16,
) -> Vec<(GsiGame, Result<PathBuf, String>)> {
    installed_games(steam_root)
        .into_iter()
        .map(|(game, dir)| (game, write_cfg(game, &dir, token, port)))
        .collect()
}

/// Remove Loadout's cfg from every supported game found under `steam_root`.
pub fn remove_cfgs(steam_root: &Path) -> Vec<(GsiGame, Result<bool, String>)> {
    installed_games(steam_root)
        .into_iter()
        .map(|(game, dir)| (game, remove_cfg(game, &dir)))
        .collect()
}

/// The last snapshot per game, so each post becomes the events since the
/// one before it.
#[derive(Debug, Default)]
pub struct GsiFeed {
    cs2: Option<Cs2State>,
    dota: Option<DotaState>,
}

impl GsiFeed {
    /// The events in one accepted post, stamped `at_ms`.
    pub fn take(&mut self, game: GsiGame, body: &str, at_ms: u64) -> Vec<WatchEvent> {
        let kinds = match game {
            GsiGame::Cs2 => {
                let Some(next) = game_watch::parse_cs2(body) else {
                    return Vec::new();
                };
                let kinds = game_watch::cs2_events(self.cs2.as_ref(), &next);
                self.cs2 = Some(next);
                kinds
            }
            GsiGame::Dota2 => {
                let Some(next) = game_watch_dota::parse_dota(body) else {
                    return Vec::new();
                };
                let kinds = game_watch_dota::dota_events(self.dota.as_ref(), &next);
                self.dota = Some(next);
                kinds
            }
        };
        kinds
            .into_iter()
            .map(|kind| WatchEvent { at_ms, kind })
            .collect()
    }
}

/// One event and the game it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GsiEvent {
    pub game: GsiGame,
    pub event: WatchEvent,
}

/// Why a request was dropped, and the status sent back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    BadRequest,
    /// Wrong `Host` or wrong or missing auth token.
    Forbidden,
    NotPost,
    TooLarge,
}

impl Reject {
    fn status(self) -> &'static str {
        match self {
            Reject::BadRequest => "400 Bad Request",
            Reject::Forbidden => "403 Forbidden",
            Reject::NotPost => "405 Method Not Allowed",
            Reject::TooLarge => "413 Payload Too Large",
        }
    }
}

/// One HTTP request, as far as the listener reads it.
#[derive(Debug)]
struct Request {
    method: String,
    host: Option<String>,
    body: Vec<u8>,
}

fn read_request(r: &mut impl Read) -> Result<Request, Reject> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(Reject::TooLarge);
        }
        let n = r.read(&mut chunk).map_err(|_| Reject::BadRequest)?;
        if n == 0 {
            return Err(Reject::BadRequest);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| Reject::BadRequest)?;
    let mut lines = head.split("\r\n");
    let method = lines
        .next()
        .and_then(|l| l.split(' ').next())
        .unwrap_or_default()
        .to_owned();
    let (mut host, mut len) = (None, None);
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(Reject::BadRequest);
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "host" => host = Some(value.to_owned()),
            "content-length" => len = Some(value.parse::<usize>().map_err(|_| Reject::BadRequest)?),
            // GSI sends a plain length; a chunked body could dodge the cap.
            "transfer-encoding" => return Err(Reject::BadRequest),
            _ => {}
        }
    }
    let len = len.unwrap_or(0);
    if len > MAX_BODY {
        return Err(Reject::TooLarge);
    }
    let mut body = buf[head_end + 4..].to_vec();
    body.truncate(len);
    let missing = (len - body.len()) as u64;
    r.take(missing)
        .read_to_end(&mut body)
        .map_err(|_| Reject::BadRequest)?;
    if body.len() != len {
        return Err(Reject::BadRequest);
    }
    Ok(Request { method, host, body })
}

/// Check a request and decode its body: a POST, to this listener's own
/// loopback host (so a web page can't reach it through DNS rebinding), with
/// the right token, from a supported game.
fn accept(req: &Request, token: &GsiToken, port: u16) -> Result<GsiGame, Reject> {
    if req.method != "POST" {
        return Err(Reject::NotPost);
    }
    let host_ok = req.host.as_deref().is_some_and(|h| {
        let h = h.to_ascii_lowercase();
        h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}")
    });
    if !host_ok {
        return Err(Reject::Forbidden);
    }
    let body: Value = serde_json::from_slice(&req.body).map_err(|_| Reject::BadRequest)?;
    let sent = body
        .get("auth")
        .and_then(|a| a.get("token"))
        .and_then(Value::as_str);
    if !sent.is_some_and(|t| token.matches(t)) {
        return Err(Reject::Forbidden);
    }
    body.get("provider")
        .and_then(|p| p.get("appid"))
        .and_then(Value::as_u64)
        .and_then(|id| u32::try_from(id).ok())
        .and_then(GsiGame::from_app_id)
        .ok_or(Reject::BadRequest)
}

fn respond(stream: &mut TcpStream, status: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
}

/// Why the listener couldn't start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenError {
    /// Another app holds the port, or Windows reserved it.
    PortTaken(u16),
    Other(String),
}

impl std::fmt::Display for ListenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListenError::PortTaken(port) => write!(
                f,
                "Another app is already using port {port} on this computer, so the game watcher can't hear the game. Close that app, then turn the game watcher off and on again."
            ),
            ListenError::Other(e) => write!(f, "The game watcher couldn't start: {e}"),
        }
    }
}

/// The running listener. It sits blocked in `accept` between posts, so it
/// uses no CPU while no game sends anything. Stops on `stop` or drop.
#[derive(Debug)]
pub struct GsiServer {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl GsiServer {
    /// Bind `127.0.0.1:port` and serve on a background thread. Events go to
    /// `sink` without ever blocking the listener: when the cabin falls
    /// behind, new events are dropped rather than queued without end.
    pub fn start(
        port: u16,
        token: GsiToken,
        sink: SyncSender<GsiEvent>,
    ) -> Result<GsiServer, ListenError> {
        let listener =
            TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).map_err(|e| {
                match e.kind() {
                    std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied => {
                        ListenError::PortTaken(port)
                    }
                    _ => ListenError::Other(e.to_string()),
                }
            })?;
        let port = listener
            .local_addr()
            .map_err(|e| ListenError::Other(e.to_string()))?
            .port();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::Builder::new()
            .name("gsi-listener".into())
            .spawn(move || serve(listener, port, &token, &stopping, &sink))
            .map_err(|e| ListenError::Other(e.to_string()))?;
        Ok(GsiServer {
            port,
            stop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stop listening and wait for the thread to end.
    pub fn stop(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocked `accept` so it sees the flag.
        let _ =
            TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], self.port)), IO_TIMEOUT);
        let _ = thread.join();
    }
}

impl Drop for GsiServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve(
    listener: TcpListener,
    port: u16,
    token: &GsiToken,
    stop: &AtomicBool,
    sink: &SyncSender<GsiEvent>,
) {
    let mut feed = GsiFeed::default();
    for conn in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(mut stream) = conn else {
            // Out of file handles or similar: don't spin.
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        let accepted = read_request(&mut stream)
            .and_then(|req| accept(&req, token, port).map(|game| (game, req.body)));
        match accepted {
            Ok((game, body)) => {
                // `accept` decoded it as JSON, so it is UTF-8. The events go
                // out before the reply, so a reply means they are queued.
                let body = String::from_utf8_lossy(&body);
                for event in feed.take(game, &body, crate::now_ms()) {
                    let _ = sink.try_send(GsiEvent { game, event });
                }
                respond(&mut stream, "200 OK");
            }
            Err(reject) => respond(&mut stream, reject.status()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_watch::EventKind;
    use std::sync::mpsc;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";

    fn token() -> GsiToken {
        GsiToken(TOKEN.into())
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-gsi-{name}-{}-{}",
            std::process::id(),
            crate::uid("t")
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A CS2 GSI body as the game sends it with Loadout's cfg.
    fn cs2(token: &str, round: u32, phase: &str, win: &str, kills: u32, deaths: u32) -> String {
        format!(
            r#"{{"provider":{{"name":"Counter-Strike: Global Offensive","appid":730,"version":14000,"steamid":"76561190000000000","timestamp":1700000000}},
               "map":{{"mode":"competitive","name":"de_mirage","phase":"live","round":{round},"team_ct":{{"score":3}},"team_t":{{"score":5}}}},
               "round":{{"phase":"{phase}","win_team":"{win}"}},
               "player":{{"steamid":"76561190000000000","name":"player","team":"CT","match_stats":{{"kills":{kills},"assists":0,"deaths":{deaths},"mvps":0,"score":0}}}},
               "auth":{{"token":"{token}"}}}}"#
        )
    }

    fn dota(token: &str, state: &str, win: &str, kills: u32) -> String {
        format!(
            r#"{{"provider":{{"name":"Dota 2","appid":570,"version":47,"timestamp":1700000000}},
               "map":{{"name":"start","game_state":"{state}","win_team":"{win}"}},
               "player":{{"team_name":"dire","kills":{kills},"deaths":0}},
               "hero":{{"name":"npc_dota_hero_lina"}},
               "auth":{{"token":"{token}"}}}}"#
        )
    }

    fn request(host: &str, body: &str) -> Vec<u8> {
        format!(
            "POST / HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn check(raw: &[u8], port: u16) -> Result<GsiGame, Reject> {
        read_request(&mut &raw[..]).and_then(|r| accept(&r, &token(), port))
    }

    /// POST over real loopback and return the status line.
    fn post(port: u16, raw: &[u8]) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(raw).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out.lines().next().unwrap_or_default().to_owned()
    }

    #[test]
    fn a_post_without_the_right_token_is_dropped() {
        let host = "127.0.0.1:29174";
        assert_eq!(
            check(&request(host, &cs2(TOKEN, 1, "live", "", 0, 0)), 29174),
            Ok(GsiGame::Cs2)
        );
        let wrong = TOKEN.replace('0', "1");
        assert_eq!(
            check(&request(host, &cs2(&wrong, 1, "live", "", 0, 0)), 29174),
            Err(Reject::Forbidden)
        );
        assert_eq!(
            check(&request(host, &cs2("", 1, "live", "", 0, 0)), 29174),
            Err(Reject::Forbidden)
        );
        let no_auth = r#"{"provider":{"appid":730},"map":{"phase":"live"}}"#;
        assert_eq!(
            check(&request(host, no_auth), 29174),
            Err(Reject::Forbidden)
        );
        assert!(!token().matches(&TOKEN[..47]));
    }

    #[test]
    fn a_post_to_another_host_is_dropped() {
        let body = cs2(TOKEN, 1, "live", "", 0, 0);
        assert_eq!(
            check(&request("localhost:29174", &body), 29174),
            Ok(GsiGame::Cs2)
        );
        for host in [
            "evil.example:29174",
            "127.0.0.1:8080",
            "127.0.0.1",
            "192.168.1.5:29174",
        ] {
            assert_eq!(
                check(&request(host, &body), 29174),
                Err(Reject::Forbidden),
                "{host}"
            );
        }
        let no_host = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(check(no_host.as_bytes(), 29174), Err(Reject::Forbidden));
    }

    #[test]
    fn oversized_chunked_and_non_post_requests_are_dropped() {
        let big = format!(
            "POST / HTTP/1.1\r\nHost: 127.0.0.1:29174\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(check(big.as_bytes(), 29174), Err(Reject::TooLarge));
        let chunked = "POST / HTTP/1.1\r\nHost: 127.0.0.1:29174\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n";
        assert_eq!(check(chunked.as_bytes(), 29174), Err(Reject::BadRequest));
        let get = "GET / HTTP/1.1\r\nHost: 127.0.0.1:29174\r\n\r\n";
        assert_eq!(check(get.as_bytes(), 29174), Err(Reject::NotPost));
        let head = format!("POST / HTTP/1.1\r\nX: {}\r\n", "a".repeat(MAX_HEAD + 10));
        assert_eq!(check(head.as_bytes(), 29174), Err(Reject::TooLarge));
        // A body cut short of its length is a bad request, not a hang.
        let short = "POST / HTTP/1.1\r\nHost: 127.0.0.1:29174\r\nContent-Length: 50\r\n\r\n{}";
        assert_eq!(check(short.as_bytes(), 29174), Err(Reject::BadRequest));
        // An app id the watcher doesn't read.
        let other = format!(r#"{{"provider":{{"appid":440}},"auth":{{"token":"{TOKEN}"}}}}"#);
        assert_eq!(
            check(&request("127.0.0.1:29174", &other), 29174),
            Err(Reject::BadRequest)
        );
    }

    #[test]
    fn the_cfg_is_valid_keyvalues_with_the_uri_and_token() {
        let text = GsiGame::Cs2.cfg_text(&token(), GSI_PORT);
        assert!(text.starts_with(CFG_MARKER));
        let vdf = crate::steam::parse_vdf(&text);
        let at = |p: &[&str]| vdf.at(p).and_then(|v| v.str()).map(str::to_owned);
        assert_eq!(
            at(&["Loadout", "uri"]).as_deref(),
            Some("http://127.0.0.1:29174/")
        );
        assert_eq!(at(&["Loadout", "auth", "token"]).as_deref(), Some(TOKEN));
        assert_eq!(at(&["Loadout", "throttle"]).as_deref(), Some("0.5"));
        let keys = |g: GsiGame| -> Vec<String> {
            let vdf = crate::steam::parse_vdf(&g.cfg_text(&token(), GSI_PORT));
            vdf.at(&["Loadout", "data"])
                .unwrap()
                .entries()
                .iter()
                .map(|(k, _)| k.clone())
                .collect()
        };
        assert_eq!(
            keys(GsiGame::Cs2),
            vec![
                "provider",
                "map",
                "round",
                "player_id",
                "player_match_stats"
            ]
        );
        assert_eq!(
            keys(GsiGame::Dota2),
            vec!["provider", "map", "player", "hero"]
        );
        assert_eq!(format!("{:?}", token()), "GsiToken(..)");
    }

    /// A Steam root with one extra library (in `libraryfolders.vdf`) that
    /// holds both games.
    fn steam_with_games(name: &str) -> (PathBuf, PathBuf) {
        let root = temp(name);
        let lib = root.join("library2");
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(
            root.join("steamapps").join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n}}\n",
                root.display().to_string().replace('\\', "\\\\"),
                lib.display().to_string().replace('\\', "\\\\"),
            ),
        )
        .unwrap();
        let common = lib.join("steamapps").join("common");
        std::fs::create_dir_all(
            common
                .join("Counter-Strike Global Offensive")
                .join("game/csgo/cfg"),
        )
        .unwrap();
        std::fs::create_dir_all(common.join("dota 2 beta").join("game/dota/cfg")).unwrap();
        (root, common)
    }

    #[test]
    fn the_cfg_is_written_into_each_game_and_removed_on_off() {
        let (root, common) = steam_with_games("cfg");
        let cs2_cfg = common.join("Counter-Strike Global Offensive/game/csgo/cfg");
        let dota_cfg = common.join("dota 2 beta/game/dota/cfg/gamestate_integration");
        // The user's own cfg files sit next to ours and must survive.
        std::fs::write(cs2_cfg.join("autoexec.cfg"), "bind x noclip").unwrap();
        std::fs::write(
            cs2_cfg.join("gamestate_integration_other.cfg"),
            "\"Other\" {}",
        )
        .unwrap();

        let written = install_cfgs(&root, &token(), GSI_PORT);
        assert_eq!(
            written,
            vec![
                (GsiGame::Cs2, Ok(cs2_cfg.join(CFG_FILE))),
                (GsiGame::Dota2, Ok(dota_cfg.join(CFG_FILE))),
            ]
        );
        assert_eq!(
            std::fs::read_to_string(cs2_cfg.join(CFG_FILE)).unwrap(),
            GsiGame::Cs2.cfg_text(&token(), GSI_PORT)
        );
        assert_eq!(
            std::fs::read_to_string(dota_cfg.join(CFG_FILE)).unwrap(),
            GsiGame::Dota2.cfg_text(&token(), GSI_PORT)
        );
        // No temp file is left behind.
        let mut names: Vec<String> = std::fs::read_dir(&cs2_cfg)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "autoexec.cfg",
                "gamestate_integration_loadout.cfg",
                "gamestate_integration_other.cfg"
            ]
        );

        assert_eq!(
            remove_cfgs(&root),
            vec![(GsiGame::Cs2, Ok(true)), (GsiGame::Dota2, Ok(true))]
        );
        assert!(!cs2_cfg.join(CFG_FILE).exists() && !dota_cfg.join(CFG_FILE).exists());
        assert_eq!(
            std::fs::read_to_string(cs2_cfg.join("autoexec.cfg")).unwrap(),
            "bind x noclip"
        );
        assert!(cs2_cfg.join("gamestate_integration_other.cfg").exists());
        // Off again finds nothing to remove.
        assert_eq!(
            remove_cfgs(&root),
            vec![(GsiGame::Cs2, Ok(false)), (GsiGame::Dota2, Ok(false))]
        );
        // A file of our name that Loadout didn't write stays.
        std::fs::write(cs2_cfg.join(CFG_FILE), "\"Mine\" {}").unwrap();
        assert_eq!(
            remove_cfg(
                GsiGame::Cs2,
                &common.join("Counter-Strike Global Offensive")
            ),
            Ok(false)
        );
        assert_eq!(
            std::fs::read_to_string(cs2_cfg.join(CFG_FILE)).unwrap(),
            "\"Mine\" {}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_game_without_its_cfg_folder_is_skipped() {
        let root = temp("none");
        std::fs::create_dir_all(root.join("steamapps/common/Counter-Strike Global Offensive/game"))
            .unwrap();
        assert_eq!(installed_games(&root), vec![]);
        assert_eq!(install_cfgs(&root, &token(), GSI_PORT), vec![]);
        assert_eq!(
            write_cfg(
                GsiGame::Cs2,
                &root.join("steamapps/common/Counter-Strike Global Offensive"),
                &token(),
                GSI_PORT
            ),
            Err("Counter-Strike 2's cfg folder isn't there.".to_string())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_token_is_made_once_and_kept() {
        let dir = temp("token");
        let a = GsiToken::load_or_create(&dir).unwrap();
        let saved = std::fs::read_to_string(dir.join(TOKEN_FILE)).unwrap();
        assert_eq!(saved.len(), 48);
        assert_eq!(a, GsiToken(saved));
        assert_eq!(GsiToken::load_or_create(&dir).unwrap(), a);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(TOKEN_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // A damaged file gets a new token.
        std::fs::write(dir.join(TOKEN_FILE), "short").unwrap();
        let b = GsiToken::load_or_create(&dir).unwrap();
        assert_ne!(b, a);
        assert_ne!(GsiToken::new(), GsiToken::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_taken_port_is_a_clear_error() {
        let held = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = held.local_addr().unwrap().port();
        let (tx, _rx) = mpsc::sync_channel(8);
        let err = GsiServer::start(port, token(), tx).unwrap_err();
        assert_eq!(err, ListenError::PortTaken(port));
        assert_eq!(
            err.to_string(),
            format!("Another app is already using port {port} on this computer, so the game watcher can't hear the game. Close that app, then turn the game watcher off and on again.")
        );
    }

    #[test]
    fn real_loopback_posts_become_cs2_and_dota_events() {
        let (tx, rx) = mpsc::sync_channel(64);
        let mut server = GsiServer::start(0, token(), tx).unwrap();
        let port = server.port();
        let host = format!("127.0.0.1:{port}");
        let ok = "HTTP/1.1 200 OK";
        // CS2: baseline, round live, two kills and a death, round lost.
        for body in [
            cs2(TOKEN, 7, "freezetime", "", 4, 2),
            cs2(TOKEN, 7, "live", "", 4, 2),
            cs2(TOKEN, 7, "live", "", 6, 3),
            cs2(TOKEN, 7, "over", "T", 6, 3),
        ] {
            assert_eq!(post(port, &request(&host, &body)), ok);
        }
        // Dropped posts never reach the feed.
        let wrong = TOKEN.replace('a', "b");
        assert_eq!(
            post(port, &request(&host, &cs2(&wrong, 8, "live", "", 9, 9))),
            "HTTP/1.1 403 Forbidden"
        );
        assert_eq!(
            post(
                port,
                &request("evil.example", &cs2(TOKEN, 8, "live", "", 9, 9))
            ),
            "HTTP/1.1 403 Forbidden"
        );
        // Dota on the same listener: baseline, a kill, the match won.
        for body in [
            dota(TOKEN, "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS", "none", 0),
            dota(TOKEN, "DOTA_GAMERULES_STATE_GAME_IN_PROGRESS", "none", 1),
            dota(TOKEN, "DOTA_GAMERULES_STATE_POST_GAME", "dire", 1),
        ] {
            assert_eq!(post(port, &request(&host, &body)), ok);
        }
        server.stop();
        let got: Vec<(GsiGame, EventKind)> =
            rx.try_iter().map(|e| (e.game, e.event.kind)).collect();
        assert_eq!(
            got,
            vec![
                (GsiGame::Cs2, EventKind::RoundStart { round: 7 }),
                (GsiGame::Cs2, EventKind::Kill),
                (GsiGame::Cs2, EventKind::Kill),
                (GsiGame::Cs2, EventKind::Death),
                (
                    GsiGame::Cs2,
                    EventKind::RoundEnd {
                        round: 7,
                        won: false
                    }
                ),
                (GsiGame::Dota2, EventKind::Kill),
                (GsiGame::Dota2, EventKind::MatchEnd { won: 1, lost: 0 }),
            ]
        );
        // Stopped means nothing listens there any more.
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    }

    #[test]
    fn only_supported_app_ids_are_games() {
        assert_eq!(GsiGame::from_app_id(730), Some(GsiGame::Cs2));
        assert_eq!(GsiGame::from_app_id(570), Some(GsiGame::Dota2));
        assert_eq!(GsiGame::from_app_id(440), None);
    }
}
