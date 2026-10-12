//! Connectivity for offline card PR B: `Offline | Online | Degraded`.
//!
//! The cabin (or tests) set the state from a cheap probe / OS signal. While
//! [`NetState::Offline`] (or Degraded with no usable cloud route), the router
//! prefers the on-device model for chat and eligible background work.

use std::sync::atomic::{AtomicU8, Ordering};

/// How the cabin sees the network for model calls.
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

/// Cabin / probe writes the live state.
pub fn set_state(s: NetState) {
    STATE.store(s as u8, Ordering::SeqCst);
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

    #[test]
    fn chip_and_queue_notes_are_literal() {
        assert_eq!(
            offline_chip("Qwen 2.5 3B"),
            "Offline · on Qwen 2.5 3B (local)"
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
}
