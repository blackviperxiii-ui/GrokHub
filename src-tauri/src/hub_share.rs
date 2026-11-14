//! Devices → Start share: this computer's LAN hub, run in the app itself
//! with the same `grokhub-hub` server and `hub-state.json` the cabin uses, so
//! another computer pairs with either app the same way.
//!
//! Sharing is off at every launch and ends when the app quits. The pairing
//! code lives in memory only: it is never written to `hub-state.json`, never
//! logged and never part of an error text.
//!
//! A computer pairs inside the hub server, with no callback. So the file is
//! written when the paired list is seen to have changed: on the page's poll
//! (`view`) and when the app quits (`save`). Another app on this computer
//! writes the same file; what it saved meanwhile is kept.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use grokhub_core::{
    hub_pair_url, load_hub_state, now_ms, pair::pair_code_is_live, pair::start_hub_rotates_pair,
    save_hub_state, HubState, DEFAULT_PORT,
};
use serde::Serialize;

/// One computer paired with this hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerView {
    /// The paired computer's device id: what tells two with one name apart.
    pub id: String,
    pub name: String,
    /// Unix ms; 0 when it has not called since pairing.
    pub last_seen: u64,
}

/// What Devices shows about sharing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareView {
    pub on: bool,
    pub port: u16,
    /// The live pairing code while sharing; empty when none is live.
    pub code: String,
    /// The address the other computer opens, without the code.
    pub address: String,
    /// True when the code is gone because a computer paired with it; false
    /// when it was burned by wrong guesses, or while a code is live.
    pub paired_since_code: bool,
    pub peers: Vec<PeerView>,
}

/// A paired computer as the file knows it: its id and its token. A computer
/// that pairs again keeps its id and gets a new token.
type PeerKey = (String, String);

fn peer_keys(st: &HubState) -> Vec<PeerKey> {
    st.peers
        .iter()
        .map(|p| (p.id.clone(), p.token.clone()))
        .collect()
}

/// What another app saved to the file since this one read it stays: its
/// paired computers, and its snapshot, inbox and inhabit bundle where this
/// app holds none.
fn keep_theirs(mine: &mut HubState, theirs: HubState) {
    for peer in theirs.peers {
        if !mine.peers.iter().any(|p| p.id == peer.id) {
            mine.peers.push(peer);
        }
    }
    if mine.snapshot.is_none() {
        mine.snapshot = theirs.snapshot;
    }
    if mine.inbox.is_empty() {
        mine.inbox = theirs.inbox;
    }
    if mine.inhabit.is_none() {
        mine.inhabit = theirs.inhabit;
    }
}

pub struct HubShare {
    hub: Arc<Mutex<HubState>>,
    path: PathBuf,
    /// The paired computers as the file last had them from this app.
    saved: Mutex<Vec<PeerKey>>,
    /// The paired computers when the current pairing code was made. A code
    /// that is gone was used if the list changed since, and burned otherwise.
    at_code: Mutex<Vec<PeerKey>>,
    /// The port the listener is bound to, once it is. A listener can't be
    /// unbound while the app runs, so stopping share keeps it and closes the
    /// pairing instead.
    bound: Mutex<Option<u16>>,
    /// Starts the listener on a port (0 for any free one); the app uses the
    /// real hub, tests use loopback.
    bind: fn(Arc<Mutex<HubState>>, u16) -> Result<u16, String>,
    /// This computer's LAN address.
    lan: fn() -> Option<IpAddr>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl HubShare {
    /// The hub as the config folder has it: its paired computers stay, but
    /// sharing is off and no pairing code carries over.
    pub fn open(config_dir: &Path) -> Self {
        Self::with(config_dir, grokhub_hub::serve_lan, crate::remote::lan_ip)
    }

    fn with(
        config_dir: &Path,
        bind: fn(Arc<Mutex<HubState>>, u16) -> Result<u16, String>,
        lan: fn() -> Option<IpAddr>,
    ) -> Self {
        let path = config_dir.join("hub-state.json");
        let mut state = load_hub_state(&path).unwrap_or_else(HubState::empty);
        state.sharing = false;
        state.pair = None;
        let peers = peer_keys(&state);
        Self {
            hub: Arc::new(Mutex::new(state)),
            path,
            saved: Mutex::new(peers.clone()),
            at_code: Mutex::new(peers),
            bound: Mutex::new(None),
            bind,
            lan,
        }
    }

    /// Start share: listens on the LAN and mints a pairing code when none is live.
    pub fn start(&self) -> Result<ShareView, String> {
        let mut bound = lock(&self.bound);
        let port = match *bound {
            Some(port) => port,
            None => {
                let want = lock(&self.hub).port;
                let want = if want == 0 { DEFAULT_PORT } else { want };
                let port = (self.bind)(self.hub.clone(), want).map_err(|e| {
                    if grokhub_core::pair::lan_bind_in_use(&e) {
                        format!(
                            "Port {want} is already in use. Stop the other hub on this computer, then try again."
                        )
                    } else {
                        "Sharing could not start on this computer's network.".to_string()
                    }
                })?;
                *bound = Some(port);
                port
            }
        };
        drop(bound);
        {
            let mut st = lock(&self.hub);
            st.sharing = true;
            st.port = port;
            if start_hub_rotates_pair(st.pair.as_ref().map(|p| p.expires_at), now_ms()) {
                self.rotate(&mut st);
            }
        }
        self.persist();
        Ok(self.view())
    }

    /// Stop sharing: the pairing code goes, so no new computer can pair.
    pub fn stop(&self) -> ShareView {
        {
            let mut st = lock(&self.hub);
            st.sharing = false;
            st.pair = None;
        }
        self.persist();
        self.view()
    }

    /// A new pairing code, once the old one is used or expired.
    pub fn new_code(&self) -> ShareView {
        {
            let mut st = lock(&self.hub);
            if st.sharing {
                self.rotate(&mut st);
            }
        }
        self.view()
    }

    /// A new code, and the paired list it starts from.
    fn rotate(&self, st: &mut HubState) {
        st.rotate_pair();
        *lock(&self.at_code) = peer_keys(st);
    }

    /// What Devices shows. Saves a pairing made since the last look, so it
    /// writes a file then: call it off the main thread.
    pub fn view(&self) -> ShareView {
        let view = self.read();
        self.save();
        view
    }

    fn read(&self) -> ShareView {
        let port = lock(&self.bound).unwrap_or(DEFAULT_PORT);
        let mut st = lock(&self.hub);
        // An unused code that ran out is replaced, as the cabin's Devices
        // page does: a share nobody touched always has a code to pair with.
        let ran_out = st
            .pair
            .as_ref()
            .is_some_and(|p| !pair_code_is_live(p.expires_at, now_ms()));
        if st.sharing && ran_out {
            self.rotate(&mut st);
        }
        let code = st
            .pair
            .as_ref()
            .filter(|p| st.sharing && pair_code_is_live(p.expires_at, now_ms()))
            .map(|p| p.code.clone())
            .unwrap_or_default();
        let paired_since_code =
            st.sharing && code.is_empty() && peer_keys(&st) != *lock(&self.at_code);
        let lan = (self.lan)().map(|ip| ip.to_string());
        ShareView {
            on: st.sharing,
            port,
            code,
            address: if st.sharing {
                hub_pair_url(port, lan.as_deref())
            } else {
                String::new()
            },
            paired_since_code,
            peers: st
                .peers
                .iter()
                .map(|p| PeerView {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    last_seen: p.last_seen,
                })
                .collect(),
        }
    }

    /// Whether sharing is on, and the newest frame a paired computer sent,
    /// with the time it came. Memory only: writes no file.
    pub fn glance(&self) -> (bool, Option<(String, u64)>) {
        let st = lock(&self.hub);
        let frame = st.last_frame.as_ref().map(|f| (f.data_url.clone(), f.at));
        (st.sharing, frame)
    }

    /// Writes the file when a computer paired since the last write, and
    /// nothing otherwise. The page's poll and the app quitting both call it.
    pub fn save(&self) {
        let now = peer_keys(&lock(&self.hub));
        let changed = now != *lock(&self.saved);
        if changed {
            self.persist();
        }
    }

    /// Writes the hub's file without the pairing code or the sharing flag.
    fn persist(&self) {
        let mut disk = lock(&self.hub).clone();
        disk.pair = None;
        disk.sharing = false;
        let mine = peer_keys(&disk);
        if let Some(theirs) = load_hub_state(&self.path) {
            keep_theirs(&mut disk, theirs);
        }
        if save_hub_state(&self.path, &disk).is_ok() {
            *lock(&self.saved) = mine;
        }
    }
}

/// The chat's device glance, as the cabin's: "Sharing" while this
/// computer's hub is on, or the newest screenshot otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGlance {
    pub sharing: bool,
    /// The newest frame as a `data:image/...;base64,` URL, checked by
    /// [`grokhub_core::store_frame`]; `None` when there is none.
    pub frame: Option<String>,
}

/// None when sharing is off and no frame is held: the glance stays hidden.
/// Of `frames`, the newest one that is a whole image under the frame cap wins.
pub fn device_glance(
    sharing: bool,
    frames: impl IntoIterator<Item = (String, u64)>,
) -> Option<DeviceGlance> {
    let mut frames: Vec<(String, u64)> = frames.into_iter().collect();
    frames.sort_by_key(|f| std::cmp::Reverse(f.1));
    let frame = frames
        .into_iter()
        .find_map(|(url, at)| grokhub_core::store_frame(&url, at))
        .map(|f| f.data_url);
    if !sharing && frame.is_none() {
        return None;
    }
    Some(DeviceGlance { sharing, frame })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    fn loopback(state: Arc<Mutex<HubState>>, _port: u16) -> Result<u16, String> {
        grokhub_hub::serve_background(state, 0)
    }

    fn no_lan() -> Option<IpAddr> {
        None
    }

    fn lan() -> Option<IpAddr> {
        Some("192.168.1.20".parse().unwrap())
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hub-share-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";
    const JPG: &str = "data:image/jpeg;base64,/9j/4AA=";

    #[test]
    fn the_device_glance_hides_until_sharing_or_a_frame() {
        assert_eq!(device_glance(false, []), None);
        // Not an image, or past the cap: as if no frame came.
        let huge = format!(
            "data:image/png;base64,{}",
            "A".repeat(grokhub_core::FRAME_CAP)
        );
        assert_eq!(
            device_glance(
                false,
                [
                    ("https://example.com/a.png".to_string(), 9),
                    ("data:text/html;base64,PGI+".to_string(), 8),
                    (huge, 7),
                ]
            ),
            None
        );
        assert_eq!(
            device_glance(true, []),
            Some(DeviceGlance {
                sharing: true,
                frame: None
            })
        );
    }

    #[test]
    fn the_device_glance_shows_the_newest_whole_frame() {
        let got = device_glance(
            false,
            [
                (PNG.to_string(), 100),
                (JPG.to_string(), 300),
                ("data:image/png;base64,***".to_string(), 500),
            ],
        );
        assert_eq!(
            got,
            Some(DeviceGlance {
                sharing: false,
                frame: Some(JPG.to_string())
            })
        );
        let json = serde_json::to_value(got.unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({ "sharing": false, "frame": JPG }));
    }

    #[test]
    fn the_glance_reads_sharing_and_a_paired_computers_frame() {
        let dir = temp("glance");
        let share = HubShare::with(&dir, loopback, no_lan);
        assert_eq!(share.glance(), (false, None));
        lock(&share.hub).store_frame(PNG);
        let (on, frame) = share.glance();
        assert!(!on);
        assert_eq!(frame.map(|(url, _)| url), Some(PNG.to_string()));
        share.start().unwrap();
        assert!(share.glance().0);
        share.stop();
        assert!(!share.glance().0);
    }

    fn pair(port: u16, code: &str, name: &str) -> (u16, String) {
        let body = format!(r#"{{"code":"{code}","deviceId":"laptop-1","deviceName":"{name}"}}"#);
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            s,
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let status = out.split(' ').nth(1).unwrap().parse().unwrap();
        (status, out)
    }

    #[test]
    fn sharing_is_off_until_started_and_start_makes_a_code() {
        let dir = temp("start");
        let share = HubShare::with(&dir, loopback, lan);
        let off = share.view();
        assert!(!off.on);
        assert_eq!(off.code, "");
        assert_eq!(off.address, "");
        let on = share.start().unwrap();
        assert!(on.on);
        assert_eq!(on.code.len(), 7);
        assert_eq!(on.address, format!("http://192.168.1.20:{}", on.port));
        assert_ne!(on.port, 0);
    }

    #[test]
    fn a_pairing_round_trip_adds_the_peer_and_a_wrong_code_is_refused() {
        let dir = temp("pair");
        let share = HubShare::with(&dir, loopback, no_lan);
        let on = share.start().unwrap();
        assert_eq!(on.address, format!("http://127.0.0.1:{}", on.port));

        let (status, body) = pair(on.port, "AAAAAA", "Laptop");
        assert_eq!(status, 403, "{body}");
        assert!(body.contains("Pairing code does not match."));
        assert!(share.view().peers.is_empty());

        let (status, body) = pair(on.port, &on.code, "Laptop");
        assert_eq!(status, 200, "{body}");
        assert!(body.contains(r#""ok":true"#));
        let peers = share.view().peers;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "Laptop");
        assert_eq!(peers[0].id, "laptop-1");
    }

    #[test]
    fn a_computer_paired_while_sharing_is_still_paired_after_a_new_launch_without_stop() {
        let dir = temp("nostop");
        let share = HubShare::with(&dir, loopback, no_lan);
        let on = share.start().unwrap();
        assert_eq!(pair(on.port, &on.code, "Laptop").0, 200);
        // The page's next 15 second poll.
        assert_eq!(share.view().peers.len(), 1);
        drop(share);

        let next = HubShare::with(&dir, loopback, no_lan);
        let peers = next.view().peers;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "Laptop");
    }

    #[test]
    fn quitting_right_after_a_pairing_saves_it() {
        let dir = temp("quit");
        let share = HubShare::with(&dir, loopback, no_lan);
        let on = share.start().unwrap();
        assert_eq!(pair(on.port, &on.code, "Laptop").0, 200);
        share.save();
        drop(share);

        let raw = std::fs::read_to_string(dir.join("hub-state.json")).unwrap();
        assert!(
            !raw.contains(&on.code),
            "the pairing code was written to disk"
        );
        assert!(raw.contains(r#""sharing": false"#));
        let next = HubShare::with(&dir, loopback, no_lan);
        assert_eq!(next.view().peers.len(), 1);
    }

    #[test]
    fn quitting_without_a_new_pairing_writes_nothing() {
        let dir = temp("quiet");
        let share = HubShare::with(&dir, loopback, no_lan);
        assert!(share.view().peers.is_empty());
        share.save();
        assert!(!dir.join("hub-state.json").exists());
    }

    #[test]
    fn a_peer_another_app_saved_meanwhile_is_kept() {
        let dir = temp("meanwhile");
        let path = dir.join("hub-state.json");
        let share = HubShare::with(&dir, loopback, no_lan);
        // The cabin pairs a computer and syncs after this app launched.
        let mut theirs = HubState::empty();
        theirs.peers.push(grokhub_core::state::Peer {
            id: "desk-2".into(),
            name: "Desk".into(),
            token: "t-desk".into(),
            last_seen: 5,
        });
        theirs.snapshot = Some(Arc::new(serde_json::json!({ "chats": 3 })));
        save_hub_state(&path, &theirs).unwrap();

        let on = share.start().unwrap();
        let disk = load_hub_state(&path).unwrap();
        assert_eq!(disk.peers.len(), 1);
        assert_eq!(disk.peers[0].id, "desk-2");
        assert_eq!(disk.peers[0].token, "t-desk");
        assert_eq!(
            disk.snapshot.as_deref(),
            Some(&serde_json::json!({ "chats": 3 }))
        );

        assert_eq!(pair(on.port, &on.code, "Laptop").0, 200);
        share.view();
        let disk = load_hub_state(&path).unwrap();
        let ids: Vec<&str> = disk.peers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["laptop-1", "desk-2"]);
    }

    #[test]
    fn an_expired_code_is_replaced_while_sharing() {
        let dir = temp("expired");
        let share = HubShare::with(&dir, loopback, no_lan);
        let first = share.start().unwrap().code;
        lock(&share.hub).pair.as_mut().unwrap().expires_at = 1;
        let view = share.view();
        assert!(view.on);
        assert_eq!(view.code.len(), 7);
        assert_ne!(view.code, first);
        assert!(!view.paired_since_code);
        // Not while sharing is off.
        share.stop();
        assert_eq!(share.view().code, "");
    }

    #[test]
    fn a_used_code_says_paired_and_a_burned_one_does_not() {
        let dir = temp("gone");
        let share = HubShare::with(&dir, loopback, no_lan);
        let on = share.start().unwrap();
        assert!(!on.paired_since_code);
        for _ in 0..grokhub_core::pair::PAIR_MAX_TRIES {
            assert_eq!(pair(on.port, "AAAAAA", "Laptop").0, 403);
        }
        let burned = share.view();
        assert_eq!(burned.code, "");
        assert!(!burned.paired_since_code);

        let fresh = share.new_code();
        assert_eq!(fresh.code.len(), 7);
        assert_eq!(pair(on.port, &fresh.code, "Laptop").0, 200);
        let used = share.view();
        assert_eq!(used.code, "");
        assert!(used.paired_since_code);

        // The same computer pairing again is a pairing too.
        let again = share.new_code();
        assert!(!again.paired_since_code);
        assert_eq!(pair(on.port, &again.code, "Laptop").0, 200);
        assert!(share.view().paired_since_code);
        assert!(!share.stop().paired_since_code);
    }

    #[test]
    fn stopping_closes_pairing_and_starting_again_reuses_the_listener() {
        let dir = temp("stop");
        let share = HubShare::with(&dir, loopback, no_lan);
        let on = share.start().unwrap();
        let off = share.stop();
        assert!(!off.on);
        assert_eq!(off.code, "");
        let (status, body) = pair(on.port, &on.code, "Laptop");
        assert_eq!(status, 400, "{body}");
        assert!(body.contains("No active pairing code"));

        let again = share.start().unwrap();
        assert_eq!(again.port, on.port);
        assert_eq!(again.code.len(), 7);
        assert_ne!(again.code, on.code);
    }

    #[test]
    fn new_code_replaces_the_code_only_while_sharing() {
        let dir = temp("newcode");
        let share = HubShare::with(&dir, loopback, no_lan);
        assert_eq!(share.new_code().code, "");
        let first = share.start().unwrap().code;
        let second = share.new_code().code;
        assert_eq!(second.len(), 7);
        assert_ne!(first, second);
    }

    #[test]
    fn the_file_keeps_peers_but_never_the_code_or_the_sharing_flag() {
        let dir = temp("file");
        let share = HubShare::with(&dir, loopback, no_lan);
        let on = share.start().unwrap();
        let (status, _) = pair(on.port, &on.code, "Laptop");
        assert_eq!(status, 200);
        share.persist();
        let raw = std::fs::read_to_string(dir.join("hub-state.json")).unwrap();
        assert!(
            !raw.contains(&on.code),
            "the pairing code was written to disk"
        );
        assert!(raw.contains(r#""sharing": false"#));
        assert!(raw.contains("Laptop"));

        // A new launch: the peer is still paired, sharing is off, no code.
        let next = HubShare::with(&dir, loopback, no_lan);
        let view = next.view();
        assert!(!view.on);
        assert_eq!(view.code, "");
        assert_eq!(view.peers.len(), 1);
    }

    #[test]
    fn a_port_in_use_says_so_without_the_os_text() {
        fn busy(_: Arc<Mutex<HubState>>, _: u16) -> Result<u16, String> {
            Err("Address already in use (os error 98)".into())
        }
        let dir = temp("busy");
        let share = HubShare::with(&dir, busy, no_lan);
        let err = share.start().unwrap_err();
        assert_eq!(
            err,
            format!(
                "Port {DEFAULT_PORT} is already in use. Stop the other hub on this computer, then try again."
            )
        );
        assert!(!share.view().on);
    }
}
