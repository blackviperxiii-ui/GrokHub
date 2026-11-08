//! The settings auto setup may tune, in one list both apps share.
//!
//! Each app maps a [`Key`] onto its own config through [`Target`]. The list
//! never holds 18+ or adult personalities, joke heat, desktop control, screen
//! recording, spend caps or the local model's launch tuning: those stay the
//! user's alone.

use serde::{Deserialize, Serialize};

/// A setting auto setup knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Key {
    /// Text and controls size, 100 to 150 %.
    TextSize,
    /// Fewer animations.
    ReduceMotion,
    /// `system`, `dark` or `light`.
    Theme,
    /// The image wall and composer glow behind the chat.
    Effects,
    /// Keep running in the tray when the window closes.
    CloseToTray,
    /// The AI model that runs on this PC.
    LocalModel,
    /// The local model tidies a prompt before it goes out.
    PromptCleanup,
    /// No pings between these hours.
    QuietHours,
    /// When background tidying runs.
    DreamHour,
    /// `all`, `important` or `quiet`.
    Notifications,
    /// The game companion overlay and its pings.
    GameCompanion,
    /// Steam patch and graphics driver nudges.
    GameNudges,
    /// Pick up the code folder you were last in.
    ResumeCode,
    /// The objectives column beside the chat.
    ChatObjectives,
    /// Side panels docked beside the chat.
    DockPanels,
    /// Thinking shows collapsed.
    CollapseThoughts,
    /// Suggestion chips under a reply.
    Suggestions,
    /// How the assistant talks: `friendly`, `direct` or `playful`.
    Personality,
    /// `ask` (ask before acting) or `risky` (ask only for risky steps).
    Approvals,
}

/// A setting's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "camelCase")]
pub enum Value {
    Bool(bool),
    /// A percentage (text size).
    Pct(u16),
    /// An hour of the day, 0 to 23.
    Hour(u8),
    /// From an hour to an hour.
    Hours(u8, u8),
    /// One of a few named choices.
    Pick(String),
}

impl Value {
    pub fn pick(s: &str) -> Value {
        Value::Pick(s.to_string())
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// Text sizes on offer, smallest first.
pub const TEXT_STEPS: [u16; 4] = [100, 115, 130, 150];

/// The step at or below `pct` (an OS at 140 % gets 130 %).
pub fn text_step_for(pct: u16) -> u16 {
    TEXT_STEPS
        .iter()
        .rev()
        .copied()
        .find(|s| *s <= pct)
        .unwrap_or(TEXT_STEPS[0])
}

/// How a setting is described and how far auto setup may go with it.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub key: Key,
    /// What the Auto setup page calls it.
    pub label: &'static str,
    /// The user sees the change at once (counts toward the change budget).
    pub visible: bool,
    /// Auto setup may change it without asking when it is sure. Otherwise a
    /// rule can only suggest it, and only an answer sets it.
    pub auto: bool,
    /// The value a tie between two rules settles on: the one that costs less
    /// if it turns out wrong (fewer extras, less battery, more privacy).
    pub safer: Option<fn() -> Value>,
    /// Words that name it in a chat ask ("make the text bigger").
    pub words: &'static [&'static str],
}

fn off() -> Value {
    Value::Bool(false)
}
fn on() -> Value {
    Value::Bool(true)
}
fn important() -> Value {
    Value::pick("important")
}

const SPECS: &[Spec] = &[
    Spec {
        key: Key::TextSize,
        label: "Text size",
        visible: true,
        auto: true,
        safer: None,
        words: &[
            "text",
            "text size",
            "font",
            "font size",
            "fonts",
            "letters",
            "words",
            "everything",
            "things",
        ],
    },
    Spec {
        key: Key::ReduceMotion,
        label: "Reduce motion",
        visible: true,
        auto: true,
        safer: Some(on),
        words: &["animations", "animation", "motion", "moving stuff"],
    },
    Spec {
        key: Key::Theme,
        label: "Theme",
        visible: true,
        auto: true,
        safer: None,
        words: &[
            "dark mode",
            "dark theme",
            "light mode",
            "light theme",
            "theme",
        ],
    },
    Spec {
        key: Key::Effects,
        label: "Background effects",
        visible: true,
        auto: true,
        safer: Some(off),
        words: &["effects", "background effects", "glow", "image wall"],
    },
    Spec {
        key: Key::CloseToTray,
        label: "Keep running when closed",
        visible: false,
        auto: false,
        safer: None,
        words: &["tray", "close to tray", "run in the background"],
    },
    Spec {
        key: Key::LocalModel,
        label: "AI on this PC",
        visible: false,
        auto: false,
        safer: None,
        words: &["local model", "local ai", "offline ai"],
    },
    Spec {
        key: Key::PromptCleanup,
        label: "Prompt cleanup",
        visible: false,
        auto: false,
        safer: None,
        words: &["prompt cleanup"],
    },
    Spec {
        key: Key::QuietHours,
        label: "Quiet hours",
        visible: false,
        auto: false,
        safer: None,
        words: &["quiet hours"],
    },
    Spec {
        key: Key::DreamHour,
        label: "Tidy-up time",
        visible: false,
        auto: false,
        safer: None,
        words: &["dream time", "tidy up time"],
    },
    Spec {
        key: Key::Notifications,
        label: "Notifications",
        visible: false,
        auto: false,
        safer: Some(important),
        words: &[
            "notifications",
            "notification",
            "pings",
            "alerts",
            "reminders",
        ],
    },
    Spec {
        key: Key::GameCompanion,
        label: "Game companion",
        visible: false,
        auto: false,
        safer: Some(off),
        words: &["game companion"],
    },
    Spec {
        key: Key::GameNudges,
        label: "Game and driver updates",
        visible: false,
        auto: false,
        safer: Some(off),
        words: &["driver nudges", "patch nudges"],
    },
    Spec {
        key: Key::ResumeCode,
        label: "Resume coding",
        visible: false,
        auto: false,
        safer: Some(off),
        words: &["resume code"],
    },
    Spec {
        key: Key::ChatObjectives,
        label: "Objectives beside the chat",
        visible: true,
        auto: false,
        safer: Some(off),
        words: &["objectives"],
    },
    Spec {
        key: Key::DockPanels,
        label: "Docked panels",
        visible: true,
        auto: false,
        safer: Some(off),
        words: &["docked panels", "dock panels"],
    },
    Spec {
        key: Key::CollapseThoughts,
        label: "Collapse thinking",
        visible: true,
        auto: true,
        safer: None,
        words: &["thoughts", "thinking"],
    },
    Spec {
        key: Key::Suggestions,
        label: "Suggestion chips",
        visible: true,
        auto: false,
        safer: None,
        words: &["suggestions", "suggestion chips", "chips"],
    },
    Spec {
        key: Key::Personality,
        label: "How I talk",
        visible: true,
        auto: false,
        safer: None,
        words: &[],
    },
    Spec {
        key: Key::Approvals,
        label: "When I ask first",
        visible: false,
        auto: false,
        safer: None,
        words: &[],
    },
];

pub fn all() -> &'static [Spec] {
    SPECS
}

impl Key {
    pub fn spec(self) -> &'static Spec {
        SPECS.iter().find(|s| s.key == self).unwrap_or(&SPECS[0])
    }

    pub fn label(self) -> &'static str {
        self.spec().label
    }
}

/// A value in plain words: `130%`, `On`, `10 PM to 7 AM`, `Dark`.
pub fn describe(key: Key, v: &Value, clock_12h: bool) -> String {
    match v {
        Value::Bool(true) => "On".into(),
        Value::Bool(false) => "Off".into(),
        Value::Pct(p) => format!("{p}%"),
        Value::Hour(h) => hour_text(*h, clock_12h),
        Value::Hours(a, b) => format!(
            "{} to {}",
            hour_text(*a, clock_12h),
            hour_text(*b, clock_12h)
        ),
        Value::Pick(p) => match (key, p.as_str()) {
            (Key::Approvals, "ask") => "Ask first".into(),
            (Key::Approvals, "risky") => "Only for risky steps".into(),
            (Key::Notifications, "important") => "Only important".into(),
            (Key::Personality, "direct") => "Straight to the point".into(),
            (_, other) => {
                let mut c = other.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            }
        },
    }
}

/// A changed setting on one line: `Text size: 130%`.
pub fn change_line(key: Key, to: &Value, clock_12h: bool) -> String {
    format!("{}: {}", key.label(), describe(key, to, clock_12h))
}

/// An offer as a yes-or-no question: `Turn on game companion?`,
/// `Set notifications to only important?`.
pub fn offer_line(key: Key, to: &Value, clock_12h: bool) -> String {
    let label = lower_first(key.label());
    match to {
        Value::Bool(true) => format!("Turn on {label}?"),
        Value::Bool(false) => format!("Turn off {label}?"),
        _ => format!(
            "Set {label} to {}?",
            lower_first(&describe(key, to, clock_12h))
        ),
    }
}

/// `Game companion` → `game companion`; `10 PM to 7 AM` stays as it is.
pub fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) if !s.contains(" AM") && !s.contains(" PM") => {
            f.to_lowercase().collect::<String>() + c.as_str()
        }
        _ => s.to_string(),
    }
}

/// `22` → `10 PM` or `22:00`.
pub fn hour_text(h: u8, clock_12h: bool) -> String {
    let h = h % 24;
    if !clock_12h {
        return format!("{h:02}:00");
    }
    match h {
        0 => "12 AM".into(),
        12 => "12 PM".into(),
        1..=11 => format!("{h} AM"),
        _ => format!("{} PM", h - 12),
    }
}

/// How an app reads and writes its own config for auto setup.
pub trait Target {
    /// The current value, or `None` when this app has no such setting.
    fn get(&self, key: Key) -> Option<Value>;
    /// Write it; `false` when this app can't hold that value.
    fn set(&mut self, key: Key, value: &Value) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_one_spec() {
        for s in all() {
            assert_eq!(
                all().iter().filter(|o| o.key == s.key).count(),
                1,
                "{:?}",
                s.key
            );
            assert_eq!(s.key.spec().key, s.key);
        }
        assert_eq!(all().len(), 19);
    }

    #[test]
    fn text_steps_round_down() {
        assert_eq!(text_step_for(100), 100);
        assert_eq!(text_step_for(125), 115);
        assert_eq!(text_step_for(140), 130);
        assert_eq!(text_step_for(225), 150);
        assert_eq!(text_step_for(90), 100);
    }

    #[test]
    fn values_read_as_plain_words() {
        assert_eq!(describe(Key::TextSize, &Value::Pct(130), true), "130%");
        assert_eq!(
            describe(Key::QuietHours, &Value::Hours(22, 7), true),
            "10 PM to 7 AM"
        );
        assert_eq!(
            describe(Key::QuietHours, &Value::Hours(23, 6), false),
            "23:00 to 06:00"
        );
        assert_eq!(describe(Key::Theme, &Value::pick("dark"), true), "Dark");
        assert_eq!(
            describe(Key::Approvals, &Value::pick("risky"), true),
            "Only for risky steps"
        );
        assert_eq!(hour_text(0, true), "12 AM");
        assert_eq!(hour_text(12, true), "12 PM");
    }

    #[test]
    fn changes_and_offers_read_as_one_line() {
        assert_eq!(
            change_line(Key::TextSize, &Value::Pct(150), true),
            "Text size: 150%"
        );
        assert_eq!(
            offer_line(Key::GameCompanion, &Value::Bool(true), true),
            "Turn on game companion?"
        );
        assert_eq!(
            offer_line(Key::Notifications, &Value::pick("important"), true),
            "Set notifications to only important?"
        );
        assert_eq!(
            offer_line(Key::QuietHours, &Value::Hours(23, 8), true),
            "Set quiet hours to 11 PM to 8 AM?"
        );
    }

    #[test]
    fn values_round_trip_through_json() {
        for v in [
            Value::Bool(true),
            Value::Pct(115),
            Value::Hour(21),
            Value::Hours(22, 7),
            Value::pick("dark"),
        ] {
            let s = serde_json::to_string(&v).expect("json");
            assert_eq!(serde_json::from_str::<Value>(&s).expect("back"), v);
        }
        assert_eq!(
            serde_json::to_string(&Key::TextSize).expect("key"),
            "\"textSize\""
        );
    }
}
