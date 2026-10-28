//! Companion corner-box photo sets: five DEFAULT stills per personality that
//! rotate while idle, plus ACTION stills (coding, fixing, reading, writing,
//! working, thinking, gaming) while a task is running. Stills live under
//! `assets/companion-photos/`; a later clip can replace any still with the
//! same slot name.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

include!(concat!(env!("OUT_DIR"), "/companion_photos.rs"));

/// Built-in DEFAULT / ACTION slots per set.
pub(super) const PHOTO_SLOTS: u8 = 5;
/// How long each still holds before advancing.
pub(super) const PHOTO_HOLD: Duration = Duration::from_millis(2800);

/// Task kinds that pick an ACTION photo set while tools run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum CompanionAction {
    Coding,
    Fixing,
    Reading,
    Writing,
    Working,
    Thinking,
    Gaming,
}

impl CompanionAction {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Coding => "coding",
            Self::Fixing => "fixing",
            Self::Reading => "reading",
            Self::Writing => "writing",
            Self::Working => "working",
            Self::Thinking => "thinking",
            Self::Gaming => "gaming",
        }
    }

    pub(super) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "coding" => Some(Self::Coding),
            "fixing" | "doctoring" | "doctor" => Some(Self::Fixing),
            "reading" | "research" => Some(Self::Reading),
            "writing" => Some(Self::Writing),
            "working" => Some(Self::Working),
            "thinking" => Some(Self::Thinking),
            "gaming" => Some(Self::Gaming),
            _ => None,
        }
    }

    /// Best-effort map from a running tool name to an ACTION set.
    pub(super) fn from_tool(name: &str) -> Self {
        let n = name.trim().to_ascii_lowercase();
        if n.contains("game") || n.contains("playbook") {
            return Self::Gaming;
        }
        if n.contains("search") || n.contains("web") || n.contains("fetch") || n.contains("read") {
            return Self::Reading;
        }
        if n.contains("write") || n.contains("edit") || n.contains("apply_patch") {
            return Self::Writing;
        }
        if n.contains("fix") || n.contains("doctor") || n.contains("repair") {
            return Self::Fixing;
        }
        if n.contains("shell") || n.contains("bash") || n.contains("code") || n.contains("cargo") {
            return Self::Coding;
        }
        if n.contains("think") || n.contains("plan") {
            return Self::Thinking;
        }
        Self::Working
    }
}

/// A still for the dock. Clip replacement later: look up the same
/// personality/action + slot under a `clips/` twin and paint a short loop
/// instead of `png`.
#[derive(Clone, Copy)]
pub(super) struct CompanionStill {
    pub png: &'static [u8],
}

/// PNG bytes for personality `id` DEFAULT slot `1..=5`. Missing art falls
/// back to Nyx, then to any lower slot that exists.
pub(super) fn default_still(personality_id: &str, slot: u8) -> Option<CompanionStill> {
    let slot = ((slot.saturating_sub(1)) % PHOTO_SLOTS) + 1;
    let id = if personality_id.trim().is_empty()
        || personality_id == grokhub_core::personality::LEGACY_DEFAULT_ID
    {
        grokhub_core::personality::DEFAULT_ID
    } else {
        personality_id.trim()
    };
    png_for_default(id, slot)
        .or_else(|| png_for_default(grokhub_core::personality::DEFAULT_ID, slot))
        .or_else(|| {
            (1..=PHOTO_SLOTS)
                .find_map(|s| png_for_default(grokhub_core::personality::DEFAULT_ID, s))
        })
        .map(|png| CompanionStill { png })
}

/// PNG bytes for an ACTION slot. Missing slots fall back to slot 1 of the
/// same action, then to `working`.
pub(super) fn action_still(action: CompanionAction, slot: u8) -> Option<CompanionStill> {
    let slot = ((slot.saturating_sub(1)) % PHOTO_SLOTS) + 1;
    png_for_action(action.id(), slot)
        .or_else(|| png_for_action(action.id(), 1))
        .or_else(|| png_for_action(CompanionAction::Working.id(), 1))
        .map(|png| CompanionStill { png })
}

/// `GROKHUB_DECK_ACTION` pins an ACTION set for deck screenshots.
pub(super) fn pinned_action() -> Option<CompanionAction> {
    static PIN: OnceLock<Option<CompanionAction>> = OnceLock::new();
    *PIN.get_or_init(|| {
        std::env::var("GROKHUB_DECK_ACTION")
            .ok()
            .as_deref()
            .and_then(CompanionAction::parse)
    })
}

/// Which DEFAULT / ACTION slot to show at `now`, advancing every [`PHOTO_HOLD`].
pub(super) fn rotating_slot(key: &str, now: Instant) -> u8 {
    type State = (String, Instant, u8);
    static STATE: OnceLock<std::sync::Mutex<State>> = OnceLock::new();
    let lock = STATE.get_or_init(|| std::sync::Mutex::new((String::new(), now, 1)));
    let Ok(mut g) = lock.lock() else {
        return 1;
    };
    if g.0 != key {
        g.0 = key.to_string();
        g.1 = now;
        g.2 = 1;
        return 1;
    }
    if now.duration_since(g.1) >= PHOTO_HOLD {
        g.2 = (g.2 % PHOTO_SLOTS) + 1;
        g.1 = now;
    }
    g.2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nyx_defaults_embed_five_slots() {
        for slot in 1..=PHOTO_SLOTS {
            let still = default_still("nyx", slot).expect("nyx default slot");
            assert!(
                still.png.len() > 100,
                "slot {slot} too small: {}",
                still.png.len()
            );
            image::load_from_memory(still.png).expect("decodes");
        }
    }

    #[test]
    fn other_personality_falls_back_to_nyx_defaults() {
        let still = default_still("pirate", 2).expect("fallback");
        let nyx = default_still("nyx", 2).expect("nyx");
        assert_eq!(still.png.as_ptr(), nyx.png.as_ptr());
    }

    #[test]
    fn legacy_default_id_uses_nyx() {
        let a = default_still("default", 1).expect("legacy");
        let b = default_still("nyx", 1).expect("nyx");
        assert_eq!(a.png.as_ptr(), b.png.as_ptr());
    }

    #[test]
    fn actions_have_at_least_slot_one() {
        for action in [
            CompanionAction::Coding,
            CompanionAction::Fixing,
            CompanionAction::Reading,
            CompanionAction::Writing,
            CompanionAction::Working,
            CompanionAction::Thinking,
            CompanionAction::Gaming,
        ] {
            let still = action_still(action, 3).unwrap_or_else(|| panic!("{}", action.id()));
            assert!(still.png.len() > 100);
            image::load_from_memory(still.png).expect("decodes");
        }
    }

    #[test]
    fn tool_name_maps_to_action() {
        assert_eq!(
            CompanionAction::from_tool("web_search"),
            CompanionAction::Reading
        );
        assert_eq!(
            CompanionAction::from_tool("apply_patch"),
            CompanionAction::Writing
        );
        assert_eq!(CompanionAction::from_tool("shell"), CompanionAction::Coding);
        assert_eq!(
            CompanionAction::from_tool("doctor"),
            CompanionAction::Fixing
        );
        assert_eq!(
            CompanionAction::from_tool("todo_write"),
            CompanionAction::Writing
        );
        assert_eq!(
            CompanionAction::from_tool("open_url"),
            CompanionAction::Working
        );
    }

    #[test]
    fn rotating_slot_advances_after_hold() {
        let t0 = Instant::now();
        assert_eq!(rotating_slot("test-rotate", t0), 1);
        assert_eq!(
            rotating_slot("test-rotate", t0 + Duration::from_millis(100)),
            1
        );
        assert_eq!(
            rotating_slot("test-rotate", t0 + PHOTO_HOLD + Duration::from_millis(1)),
            2
        );
    }
}
