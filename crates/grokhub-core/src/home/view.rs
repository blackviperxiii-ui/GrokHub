//! What one Home tile shows, in one shape the desktop app and the shell both
//! draw. A UI never decides what a tile says; it only lays out a [`TileView`].
//!
//! Every tile has an explicit [`TileState`], so nothing on Home looks dead:
//! a tile with no data says why in one sentence and offers one button.
//! Freshness is the UI's job: each module has a [`refresh_secs`] cadence, and
//! [`is_stale`] says when the shown value is old enough to say "Updated N ago".

use serde::Serialize;

use super::layout::ModuleSize;

/// How loud a value reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    #[default]
    Normal,
    Warn,
    Alert,
    Quiet,
}

/// Big number, its caption, and an optional second line for one tile.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileText {
    pub big: String,
    pub caption: String,
    pub detail: Option<String>,
    pub tone: Tone,
}

impl TileText {
    pub fn new(big: impl Into<String>, caption: impl Into<String>, tone: Tone) -> Self {
        Self {
            big: big.into(),
            caption: caption.into(),
            detail: None,
            tone,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// One line of a list tile: a label on the left, a value on the right.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileRow {
    pub label: String,
    pub value: String,
    pub tone: Tone,
    /// What tapping the row does, if anything.
    pub tap: Option<TileAction>,
}

impl TileRow {
    pub fn new(label: impl Into<String>, value: impl Into<String>, tone: Tone) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            tone,
            tap: None,
        }
    }

    pub fn on_tap(mut self, action: TileAction) -> Self {
        self.tap = Some(action);
        self
    }
}

/// Where a tile stands. Loading and stale are the UI's: loading before the
/// first answer, stale from [`is_stale`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TileState {
    /// Real data to show.
    #[default]
    Ready,
    /// Nothing yet: one sentence and one button that fills it.
    Empty,
    /// The feature behind it is switched off; the button turns it on.
    Off,
    /// The read failed; the button retries.
    Error,
    /// It needs the internet and there is none.
    Offline,
    /// This device can't read it (a phone can't see the PC's GPU).
    Unsupported,
}

/// What a tile, row or button does. Each UI maps these onto its own routes;
/// none of them runs anything by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TileAction {
    /// Start a new chat with this ask (one of the fixed Home asks).
    Ask { ask: String },
    /// Open an existing chat.
    OpenChat { id: String },
    /// Go to a page: `pulse`, `library`, `board`, `playbooks`...
    Nav { page: String },
    /// Open a Settings section by its id.
    Settings { section: String },
    /// Run a playbook by id, through the same gates as anywhere else.
    RunPlaybook { id: String },
    /// Open an approval card to approve or review it.
    Review { id: String },
    /// Switch a Home-level feature on or off.
    Toggle { feature: String, on: bool },
    /// Switch on an off-by-default feature.
    TurnOn { feature: String },
    /// Open a web page in the browser.
    OpenUrl { url: String },
    /// Launch a Steam game through Steam.
    Play { app_id: u32 },
    /// Open the clip at this place in the Clips tile's list. The app finds
    /// the file again itself, so no path ever travels with the action.
    OpenClip { index: usize },
    /// Open Steam's own friends list, where Steam shows who is online.
    SteamFriends,
    /// Open Steam's Settings, where Game Recording saves clips.
    SteamSettings,
    /// Run a module's one-time setup (or its undo) behind the OS's own
    /// password or admin prompt.
    Setup { module: String, undo: bool },
    /// Copy a module's setup steps, to paste into a terminal by hand.
    CopySetup { module: String },
    /// Ask the tile for another pick.
    Reroll,
    /// Read the tile again.
    Retry,
}

impl TileAction {
    pub fn ask(ask: &str) -> Self {
        Self::Ask { ask: ask.into() }
    }

    pub fn nav(page: &str) -> Self {
        Self::Nav { page: page.into() }
    }

    pub fn settings(section: &str) -> Self {
        Self::Settings {
            section: section.into(),
        }
    }

    pub fn setup(module: &str, undo: bool) -> Self {
        Self::Setup {
            module: module.into(),
            undo,
        }
    }

    pub fn copy_setup(module: &str) -> Self {
        Self::CopySetup {
            module: module.into(),
        }
    }

    pub fn turn_on(feature: &str) -> Self {
        Self::TurnOn {
            feature: feature.into(),
        }
    }
}

/// A labelled button under the tile body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileButton {
    pub label: String,
    pub action: TileAction,
    /// The one button the tile wants pressed.
    pub primary: bool,
}

/// Everything one tile shows at any size. Small draws `text` alone; Medium
/// and Large add rows, progress and buttons up to [`fit`]'s limits.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileView {
    pub state: TileState,
    /// Short tag next to the title (`ON`, `3 NEW`).
    pub tag: Option<String>,
    pub text: TileText,
    pub rows: Vec<TileRow>,
    /// 0 to 100, for a ring or bar.
    pub progress: Option<u8>,
    pub buttons: Vec<TileButton>,
    /// What tapping the whole tile does (Small tiles have no buttons).
    pub tap: Option<TileAction>,
    /// When the data was read, in ms since the epoch.
    pub updated_ms: Option<u64>,
}

impl TileView {
    pub fn ready(text: TileText) -> Self {
        Self {
            text,
            ..Self::default()
        }
    }

    /// No data yet: one sentence, one button.
    pub fn empty(sentence: &str, label: &str, action: TileAction) -> Self {
        Self::needs(TileState::Empty, "—", sentence, label, action)
    }

    /// The feature is off: one sentence and a Turn on button.
    pub fn off(feature: &str, sentence: &str) -> Self {
        Self::needs(
            TileState::Off,
            "Off",
            sentence,
            "Turn on",
            TileAction::turn_on(feature),
        )
    }

    /// The read failed: what went wrong and a Retry.
    pub fn error(sentence: &str) -> Self {
        Self::needs(TileState::Error, "—", sentence, "Retry", TileAction::Retry)
    }

    /// This device can't read it; say where it can be seen.
    pub fn unsupported(sentence: &str) -> Self {
        Self {
            state: TileState::Unsupported,
            text: TileText::new("—", sentence, Tone::Quiet),
            ..Self::default()
        }
    }

    fn needs(state: TileState, big: &str, sentence: &str, label: &str, action: TileAction) -> Self {
        Self {
            state,
            text: TileText::new(big, sentence, Tone::Quiet),
            tap: Some(action.clone()),
            buttons: vec![TileButton {
                label: label.into(),
                action,
                primary: true,
            }],
            ..Self::default()
        }
    }

    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    pub fn rows(mut self, rows: Vec<TileRow>) -> Self {
        self.rows = rows;
        self
    }

    pub fn progress(mut self, pct: u8) -> Self {
        self.progress = Some(pct.min(100));
        self
    }

    pub fn button(mut self, label: &str, action: TileAction) -> Self {
        self.buttons.push(TileButton {
            label: label.into(),
            action,
            primary: false,
        });
        self
    }

    pub fn primary(mut self, label: &str, action: TileAction) -> Self {
        self.buttons.push(TileButton {
            label: label.into(),
            action,
            primary: true,
        });
        self
    }

    pub fn on_tap(mut self, action: TileAction) -> Self {
        self.tap = Some(action);
        self
    }

    pub fn at(mut self, updated_ms: u64) -> Self {
        self.updated_ms = Some(updated_ms);
        self
    }
}

/// How much of a [`TileView`] a size has room for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    pub rows: usize,
    pub buttons: usize,
}

/// Small shows the text alone; Medium about four rows and two buttons;
/// Large about eight rows and three buttons.
pub fn fit(size: ModuleSize) -> Fit {
    match size {
        ModuleSize::Small => Fit {
            rows: 0,
            buttons: 0,
        },
        ModuleSize::Medium => Fit {
            rows: 4,
            buttons: 2,
        },
        ModuleSize::Large => Fit {
            rows: 8,
            buttons: 3,
        },
    }
}

/// How often a module's data is worth reading again while Home shows, in
/// seconds: sensors every couple of seconds, system state each minute, feeds
/// and the library every 15 to 60 minutes, outside services every few hours.
/// `None` for an id that isn't in the catalog.
pub fn refresh_secs(module: &str) -> Option<u64> {
    let secs = match module {
        // Sensors.
        "frame_rate" => 2,
        "pc_loadout" | "session_timer" => 5,
        // Live state.
        "game_mode" | "stream_check" => 10,
        "objectives" | "decisions" | "recent_chats" | "now_playing" => 30,
        "speeds" | "connection" | "storage" | "usage" | "local_ai" | "workboard"
        | "automations" | "audio_check" | "squad" => 60,
        // Local files and feeds.
        "streak" | "up_next" | "systems_brief" | "journal" | "personality" | "clips"
        | "tonight" => 300,
        "updates" => 600,
        "game_library" | "what_to_play" | "playbooks" | "coach_tip" | "pulse" => 900,
        // Outside services.
        "achievements" | "patch_notes" | "daily_drop" | "hype_line" => 3_600,
        "deals" => 21_600,
        _ => return None,
    };
    Some(secs)
}

/// A value is stale once it is more than twice its module's cadence old;
/// only then does a tile say "Updated N ago".
pub fn is_stale(updated_ms: u64, now_ms: u64, refresh_secs: u64) -> bool {
    now_ms.saturating_sub(updated_ms) > refresh_secs.saturating_mul(2_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::REGISTRY;

    #[test]
    fn every_catalog_module_has_a_refresh_cadence() {
        for m in REGISTRY {
            assert!(refresh_secs(m.id).is_some(), "{} has no cadence", m.id);
        }
        assert_eq!(refresh_secs("not_a_module"), None);
    }

    #[test]
    fn sensors_refresh_faster_than_feeds() {
        let s = |id| refresh_secs(id).unwrap();
        assert!(s("frame_rate") < s("connection"));
        assert!(s("connection") < s("patch_notes"));
        assert!(s("patch_notes") < s("deals"));
    }

    #[test]
    fn a_value_goes_stale_after_twice_its_cadence() {
        let at = 1_000_000;
        assert!(!is_stale(at, at + 120_000, 60));
        assert!(is_stale(at, at + 120_001, 60));
        assert!(!is_stale(at, at - 5, 60), "a clock step back is not stale");
    }

    #[test]
    fn empty_off_and_error_tiles_say_why_and_offer_one_button() {
        for t in [
            TileView::empty("No games yet", "Open Library", TileAction::nav("library")),
            TileView::off("internet_log", "Turn on the internet log to see ping"),
            TileView::error("Couldn't read drives"),
        ] {
            assert!(!t.text.caption.is_empty());
            assert_eq!(t.buttons.len(), 1);
            assert!(t.buttons[0].primary);
            assert_eq!(t.tap.as_ref(), Some(&t.buttons[0].action));
        }
        let off = TileView::off("internet_log", "x");
        assert_eq!(off.state, TileState::Off);
        assert_eq!(off.buttons[0].label, "Turn on");
        assert!(TileView::unsupported("Open Loadout on the PC")
            .buttons
            .is_empty());
    }

    #[test]
    fn sizes_fit_more_as_they_grow() {
        let (s, m, l) = (
            fit(ModuleSize::Small),
            fit(ModuleSize::Medium),
            fit(ModuleSize::Large),
        );
        assert_eq!((s.rows, s.buttons), (0, 0));
        assert!(m.rows < l.rows && m.buttons < l.buttons);
    }

    #[test]
    fn a_view_serializes_for_the_shell() {
        let v = TileView::ready(TileText::new("3 sends", "Online", Tone::Warn))
            .tag("ON")
            .progress(140)
            .primary("Retry", TileAction::Retry)
            .at(5);
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["state"], "ready");
        assert_eq!(j["text"]["tone"], "warn");
        assert_eq!(j["progress"], 100);
        assert_eq!(j["buttons"][0]["action"]["kind"], "retry");
        assert_eq!(j["updatedMs"], 5);
        let play = serde_json::to_value(TileAction::Play { app_id: 570 }).unwrap();
        assert_eq!(play["kind"], "play");
        assert_eq!(play["appId"], 570);
    }
}
