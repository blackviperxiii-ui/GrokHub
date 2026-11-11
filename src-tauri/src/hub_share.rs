//! Devices → Start share: this computer's LAN hub, run in the app itself
//! with the same `grokhub-hub` server and `hub-state.json` the cabin uses, so
//! another computer pairs with either app the same way.
//!
//! Sharing is off at every launch and ends when the app quits. The pairing
//! code lives in memory only: it is never written to `hub-state.json`, never
//! logged and never part of an error text.

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
    pub peers: Vec<PeerView>,
}

pub struct HubShare {
    hub: Arc<Mutex<HubState>>,
    path: PathBuf,
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
        Self {
            hub: Arc::new(Mutex::new(state)),
            path,
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
                st.rotate_pair();
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
                st.rotate_pair();
            }
        }
        self.view()
    }

    pub fn view(&self) -> ShareView {
        let port = lock(&self.bound).unwrap_or(DEFAULT_PORT);
        let st = lock(&self.hub);
        let code = st
            .pair
            .as_ref()
            .filter(|p| st.sharing && pair_code_is_live(p.expires_at, now_ms()))
            .map(|p| p.code.clone())
            .unwrap_or_default();
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
            peers: st
                .peers
                .iter()
                .map(|p| PeerView {
                    name: p.name.clone(),
                    last_seen: p.last_seen,
                })
                .collect(),
        }
    }

    /// Writes the hub's file without the pairing code or the sharing flag.
    fn persist(&self) {
        let mut disk = lock(&self.hub).clone();
        disk.pair = None;
        disk.sharing = false;
        let _ = save_hub_state(&self.path, &disk);
    }
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
