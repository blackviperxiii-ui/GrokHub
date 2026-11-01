//! Rocket League (`grokhub_core::game_watch_rl`).
//!
//! While the game watcher is on and Rocket League runs from Steam, this
//! connects to the game's own Stats API on 127.0.0.1 (if the user turned it
//! on in the game's ini), follows the match, and posts it to Pulse when it
//! ends. It only reads from that loopback socket; it never sends on it,
//! writes nothing into the game folder and never edits the ini.

use super::*;
use grokhub_core::game_watch_rl::{self as rl, RlClient, RlMatch, RlTracker};

/// How long to wait before trying the port again while it's closed.
const RETRY_MS: u64 = 5_000;
/// How often the socket is read while connected.
const READ_EVERY_MS: u64 = 250;

#[derive(Default)]
pub(super) struct RlUi {
    running: bool,
    client: RlClient,
    tracker: RlTracker,
    next_try_ms: u64,
    read_ms: u64,
    /// The last match that ended, for Settings.
    last: Option<RlMatch>,
    /// Tests use a free port.
    port: Option<u16>,
}

impl Cabin {
    /// Each frame: connect while the game runs, read, post finished matches.
    pub(super) fn tick_rl(&mut self) {
        let now = now_ms();
        let running = self.cfg.game_watch && self.game_running() == Some(rl::APP_ID);
        let r = &mut self.focus.rl;
        if !running {
            if r.running {
                r.client.close();
                r.tracker = RlTracker::default();
                r.running = false;
            }
            return;
        }
        r.running = true;
        if !r.client.connected() {
            if now < r.next_try_ms {
                return;
            }
            if !r.client.connect(r.port.unwrap_or(rl::PORT)) {
                r.next_try_ms = now + RETRY_MS;
                return;
            }
        }
        if now.saturating_sub(r.read_ms) < READ_EVERY_MS {
            return;
        }
        r.read_ms = now;
        let mut done = Vec::new();
        for msg in r.client.read() {
            if let Some(m) = rl::parse_message(&msg).and_then(|m| r.tracker.take(m)) {
                done.push(m);
            }
        }
        for m in done {
            let card = rl::match_card(&m, now);
            self.focus.rl.last = Some(m);
            if grokhub_core::post_update(&mut self.updates, card) {
                self.persist_updates();
            }
        }
    }

    /// For the game watcher's row in Settings: the match so far, the last
    /// match, or how to turn the Stats API on.
    pub(super) fn rl_line(&self) -> Option<(String, bool)> {
        let r = &self.focus.rl;
        if let Some(m) = r.tracker.current() {
            return Some((format!("{}: {}", rl::NAME, m.summary()), false));
        }
        if r.running && !r.client.connected() {
            return Some((
                format!(
                    "Rocket League is running, but its Stats API is off. To turn it on, set {} under [TAGame.MatchStatsExporter_TA] in TAGame/Config/DefaultStatsAPI.ini in the game's folder, then restart the game. Loadout doesn't edit the file.",
                    rl::INI_LINE
                ),
                true,
            ));
        }
        r.last
            .as_ref()
            .map(|m| (format!("{} last match: {}", rl::NAME, m.summary()), false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_match_from_the_stats_api_posts_when_it_ends() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_watch = true;
        cabin.focus.rl.port = Some(listener.local_addr().unwrap().port());
        cabin.note_running(None, 1);
        cabin.note_running(Some(rl::APP_ID), 2);
        cabin.tick_rl();
        assert!(cabin.focus.rl.client.connected());
        let (mut game, _) = listener.accept().unwrap();
        game.write_all(br#"{"Event":"MatchCreated","Data":{}}{"Event":"UpdateState","Data":{"Players":[{"Name":"Testdriver","TeamNum":1,"Score":300,"Goals":1}],"Game":{"Teams":[{"TeamNum":0,"Score":0},{"TeamNum":1,"Score":2}],"Arena":"Park_P","bHasTarget":true,"Target":{"Name":"Testdriver","TeamNum":1}}}}{"Event":"MatchEnded","Data":{"WinnerTeamNum":1}}"#).unwrap();
        let before = cabin.updates.len();
        for _ in 0..200 {
            cabin.focus.rl.read_ms = 0;
            cabin.tick_rl();
            if cabin.updates.len() > before {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(cabin.updates.len(), before + 1);
        assert_eq!(
            cabin.updates.last().unwrap().title,
            "Rocket League match: won 2-0"
        );
        let (line, how_to) = cabin.rl_line().unwrap();
        assert!(line.starts_with("Rocket League last match: Won 2-0 in Park."));
        assert!(!how_to);

        cabin.note_running(None, 3);
        cabin.tick_rl();
        assert!(!cabin.focus.rl.client.connected());
    }

    #[test]
    fn a_closed_port_shows_how_to_turn_the_api_on() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_watch = true;
        cabin.focus.rl.port = Some(port);
        cabin.note_running(None, 1);
        cabin.note_running(Some(rl::APP_ID), 2);
        cabin.tick_rl();
        let (line, how_to) = cabin.rl_line().unwrap();
        assert!(how_to);
        assert!(line.contains("PacketSendRate=30"));
    }
}
