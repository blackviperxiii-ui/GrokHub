//! Connectivity for offline card PR B: `Offline | Online | Degraded`.
//!
//! The cabin installs [`grokhub_core::connectivity::Connectivity`] (L1) and mirrors
//! its debounced live state here via [`sync_from_core`] / [`on_core_state_changed`].
//! While [`NetState::Offline`] (or Degraded with no usable cloud route), the router
//! prefers the on-device model for chat and eligible background work.

use std::sync::atomic::{AtomicU8, Ordering};

use grokhub_core::connectivity::ConnectivityState;

/// How the agent router sees the network for model calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NetState {
    Online = 0,
    Degraded = 1,
    Offline = 2,
}

impl NetState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Degraded,
            2 => Self::Offline,
            _ => Self::Online,
        }
    }

    /// Map the cabin connectivity engine into router state (`Unknown` keeps online routing).
    pub fn from_core(core: ConnectivityState) -> Self {
        match core {
            ConnectivityState::Offline => Self::Offline,
            ConnectivityState::Online => Self::Online,
            ConnectivityState::Unknown => Self::Online,
        }
    }

    /// Route on-device when the network is down (or a test forces offline).
    pub fn prefer_local(self) -> bool {
        matches!(self, Self::Offline) || test_offline()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Degraded => "degraded",
            Self::Offline => "offline",
        }
    }
}

static STATE: AtomicU8 = AtomicU8::new(NetState::Online as u8);

/// Current connectivity (tests may force Offline via [`ForceOffline`]).
pub fn state() -> NetState {
    if test_offline() {
        return NetState::Offline;
    }
    NetState::from_u8(STATE.load(Ordering::SeqCst))
}

/// Cabin / probe writes the live state (tests and [`on_core_state_changed`]).
/// A change is logged to `models/local_health.jsonl` (`net`).
pub fn set_state(s: NetState) {
    if STATE.swap(s as u8, Ordering::SeqCst) != s as u8 {
        super::local_health::log("net", Some(s.as_str()));
    }
}

/// Mirror [`grokhub_core::connectivity::state`] into the router atom.
pub fn sync_from_core() {
    set_state(NetState::from_core(grokhub_core::connectivity::state()));
}

/// Called when the cabin connectivity engine publishes a new live state.
pub fn on_core_state_changed(core: ConnectivityState) {
    set_state(NetState::from_core(core));
}

/// Classes that must wait for the cloud (queue with a named note).
pub const CLOUD_ONLY_CLASSES: &[&str] = &[
    "media:image",
    "media:video",
    "web:search",
    "web:fetch",
    "prepare:hard",
];

/// Quiet status chip while offline on a named local model.
pub fn offline_chip(model_label: &str) -> String {
    let name = model_label.trim();
    if name.is_empty() {
        "Offline · on local model".into()
    } else {
        format!("Offline · on {name} (local)")
    }
}

/// Named note when a cloud-only step is parked until reconnect.
pub fn cloud_queue_note(class: &str) -> String {
    let what = match class.trim() {
        "media:image" => "image generation",
        "media:video" => "video generation",
        "web:search" => "web search",
        "web:fetch" => "web fetch",
        "prepare:hard" => "a hard prepare step",
        other => other,
    };
    format!("Waiting for the network: {what} needs the cloud.")
}

/// Classify a `ureq` error for [`grokhub_core::connectivity::note_global_transport_hint`].
pub fn classify_ureq_error(err: &ureq::Error) -> grokhub_core::connectivity::TransportHint {
    use grokhub_core::connectivity::{classify_http_status, TransportHint};
    match err {
        ureq::Error::Status(code, _) => classify_http_status(*code),
        ureq::Error::Transport(t) => match t.kind() {
            ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Io => {
                TransportHint::OfflineCandidate
            }
            _ => TransportHint::Inconclusive,
        },
    }
}

/// Feed ureq transport failures into the cabin connectivity engine.
pub fn note_ureq_transport(err: &ureq::Error) {
    let hint = classify_ureq_error(err);
    grokhub_core::connectivity::note_global_transport_hint(hint, grokhub_core::now_ms());
}

pub fn is_cloud_only(class: &str) -> bool {
    let class = super::policy::class_row(class)
        .map(|r| r.class)
        .unwrap_or(class.trim());
    CLOUD_ONLY_CLASSES.contains(&class)
}

#[cfg(not(test))]
fn test_offline() -> bool {
    false
}

#[cfg(test)]
thread_local! {
    static TEST_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn test_offline() -> bool {
    TEST_OFF.with(std::cell::Cell::get)
}

/// Tests only: force Offline for this thread until drop.
#[cfg(test)]
pub(crate) struct ForceOffline;

#[cfg(test)]
impl ForceOffline {
    pub(crate) fn on() -> Self {
        TEST_OFF.with(|c| c.set(true));
        Self
    }
}

#[cfg(test)]
impl Drop for ForceOffline {
    fn drop(&mut self) {
        TEST_OFF.with(|c| c.set(false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::connectivity::{Connectivity, TransportHint};

    #[test]
    fn chip_and_queue_notes_are_literal() {
        assert_eq!(
            offline_chip("Gemma 4 E2B"),
            "Offline · on Gemma 4 E2B (local)"
        );
        assert_eq!(offline_chip("  "), "Offline · on local model");
        assert_eq!(
            cloud_queue_note("media:image"),
            "Waiting for the network: image generation needs the cloud."
        );
        assert!(is_cloud_only("media:image"));
        assert!(!is_cloud_only("chat:default"));
        assert!(!is_cloud_only("background:summarize"));
    }

    #[test]
    fn force_offline_prefers_local_until_drop() {
        set_state(NetState::Online);
        assert!(!state().prefer_local());
        {
            let _off = ForceOffline::on();
            assert_eq!(state(), NetState::Offline);
            assert!(state().prefer_local());
        }
        assert_eq!(state(), NetState::Online);
    }

    #[test]
    fn set_state_survives_without_test_force() {
        set_state(NetState::Degraded);
        assert_eq!(state(), NetState::Degraded);
        set_state(NetState::Offline);
        assert!(state().prefer_local());
        set_state(NetState::Online);
        assert!(!state().prefer_local());
    }

    #[test]
    fn ureq_connect_errors_are_offline_candidates() {
        use grokhub_core::connectivity::TransportHint;
        let err = ureq::get("http://127.0.0.1:1/").call().unwrap_err();
        assert_eq!(classify_ureq_error(&err), TransportHint::OfflineCandidate);
    }

    #[test]
    fn core_unknown_maps_online_for_routing() {
        assert_eq!(
            NetState::from_core(ConnectivityState::Unknown),
            NetState::Online
        );
        assert!(!NetState::from_core(ConnectivityState::Unknown).prefer_local());
    }

    #[test]
    fn sync_from_core_follows_global_live_state() {
        set_state(NetState::Online);
        on_core_state_changed(ConnectivityState::Offline);
        assert!(state().prefer_local());
        sync_from_core();
        assert_eq!(
            state(),
            NetState::from_core(grokhub_core::connectivity::state())
        );
    }

    #[test]
    fn transport_401_does_not_flip_router_offline() {
        set_state(NetState::Online);
        assert_eq!(
            grokhub_core::connectivity::classify_http_status(401),
            TransportHint::CloudReachable
        );
        let mut engine = Connectivity::new(100);
        engine.note_os_online(true, 0);
        assert_eq!(
            engine.tick(500),
            Some(ConnectivityState::Online),
            "start online"
        );
        let connect_err = synthetic_connect_error();
        engine.note_transport_error(&connect_err, 600);
        assert_eq!(engine.tick(1_200), Some(ConnectivityState::Offline));
        on_core_state_changed(ConnectivityState::Offline);
        assert!(state().prefer_local());

        let status_err = synthetic_status_error(401);
        engine.note_transport_error(&status_err, 1_300);
        assert_eq!(
            engine.tick(1_900),
            Some(ConnectivityState::Online),
            "401 is cloud reachable after debounce"
        );
        on_core_state_changed(engine.live());
        assert!(!state().prefer_local(), "router stays online after 401");
    }

    fn synthetic_connect_error() -> reqwest::Error {
        reqwest::blocking::Client::builder()
            .build()
            .unwrap()
            .get("http://127.0.0.1:1/")
            .send()
            .unwrap_err()
    }

    fn synthetic_status_error(status: u16) -> reqwest::Error {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.read(&mut [0u8; 1024]);
                let body = "nope";
                let resp = format!(
                    "HTTP/1.1 {status} Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        reqwest::blocking::Client::builder()
            .build()
            .unwrap()
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .and_then(|r| r.error_for_status())
            .unwrap_err()
    }
}
