//! Home and Objectives in the shell (LO-17n). Home draws the cabin's tile
//! grid from the same `{config}/home_layout.json`, filling the tiles the
//! shell knows about and leaving the rest to the cabin. Objectives is the
//! open chat's checklist with level, XP and the weekly streak.
//!
//! The layout, the catalog and the mission and XP sums are the cabin's own
//! (`grokhub_core::home`, `mission`, `xp`, `outcome`). The shell only reads
//! the layout: a missing or broken file shows the default and is never
//! rewritten, so editing Home stays in the cabin. The one write here is
//! Accept as done (and its Undo) on Objectives, an `accepted` or `undo` line
//! in the cabin's outcome log, as the cabin's own self-review writes them.

use std::path::Path;

use grokhub_core::home::{
    lookup, place_slots, HomeLayout, ModuleSize, SlotId, HOME_LAYOUT_FILE, HOME_LAYOUT_VERSION,
};
use grokhub_core::mission::{MissionCheckStatus, MissionSeed, MissionView};
use grokhub_core::outcome::{self as oc, OutcomeResult, OutcomeSignals, TaskOutcome};
use grokhub_core::LocalClock;
use serde::Serialize;

use crate::capabilities::Capabilities;
use crate::settings::ShellSettings;
use crate::{automations, connectors, history, library, memory, nyx, personality, pulse};

/// A layout file past this is not a layout; Home shows the default.
const LAYOUT_CAP: u64 = 1024 * 1024;
/// Rows a list tile shows.
const TILE_ROWS: usize = 3;
const DAY_MS: u64 = 86_400_000;

/// A chat's `todo_write` rows: `(id, text, status)`.
pub type Todos = Vec<(String, String, String)>;
/// The chat a mission is read for: its id, title and to-dos.
pub type Chat<'a> = (&'a str, &'a str, Todos);

pub const NO_MISSION: &str = "There's no open chat with a checklist to accept.";
pub const STEPS_OPEN: &str = "Finish the open steps first, or ask the chat to.";

/// The layout as the cabin would show it. Missing, unreadable, oversized or
/// broken files give the default; an older (v1) file migrates in memory.
/// Nothing is written either way.
pub fn load_layout(config_dir: &Path) -> HomeLayout {
    let path = config_dir.join(HOME_LAYOUT_FILE);
    let fits = std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= LAYOUT_CAP);
    let raw = if fits {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return HomeLayout::default();
    };
    if !value.is_object() {
        return HomeLayout::default();
    }
    let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(1);
    if version < u64::from(HOME_LAYOUT_VERSION) {
        return HomeLayout::from_v1_value(value);
    }
    serde_json::from_value::<HomeLayout>(value)
        .map(HomeLayout::sanitize)
        .unwrap_or_default()
}

/// Who is looking: this computer's window, or a paired browser that may see
/// only the chats it started.
pub enum Viewer<'a> {
    Desktop,
    Web(&'a dyn Fn(&str) -> bool),
}

impl Viewer<'_> {
    fn sees(&self, conversation_id: &str) -> bool {
        match self {
            Viewer::Desktop => true,
            Viewer::Web(owns) => owns(conversation_id),
        }
    }

    fn web(&self) -> bool {
        matches!(self, Viewer::Web(_))
    }
}

/// Everything a Home or Objectives read needs.
pub struct Sources<'a> {
    pub config_dir: &'a Path,
    pub workspace: &'a Path,
    pub settings: &'a ShellSettings,
    pub caps: Capabilities,
    pub clock: LocalClock,
    pub viewer: Viewer<'a>,
    /// The chat the window has open; the last one opened when none.
    pub conversation_id: Option<String>,
}

impl Sources<'_> {
    /// The chat whose checklist Objectives shows. A browser gets only its own.
    fn mission_chat(&self) -> Option<String> {
        let asked = self
            .conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let id = match (&self.viewer, asked) {
            (_, Some(id)) => id.to_string(),
            (Viewer::Desktop, None) => self.settings.last_chat.trim().to_string(),
            (Viewer::Web(_), None) => return None,
        };
        (!id.is_empty() && self.viewer.sees(&id)).then_some(id)
    }
}

// ---- Objectives --------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: String,
    pub text: String,
    pub status: MissionCheckStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mission {
    pub conversation_id: String,
    pub title: String,
    /// `MissionStatus::as_str`: active, needs_approval, paused, blocked, done.
    pub status: String,
    pub steps: Vec<Step>,
    /// Verified steps and steps that count (cancelled ones don't).
    pub done: usize,
    pub total: usize,
    /// Every step is closed and the result is not accepted yet.
    pub can_accept: bool,
    /// The newest outcome for this chat is a success, so Undo has something to undo.
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Xp {
    pub level: u32,
    pub xp: u32,
    /// XP at which the next level starts.
    pub next_level_xp: u32,
    /// Progress into the level, 0 to 1.
    pub frac: f32,
    pub today: u32,
    /// The last seven days, today last: true when a task finished.
    pub streak: [bool; 7],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionPage {
    pub mission: Option<Mission>,
    pub xp: Xp,
}

/// The newest outcome line for a chat: the chat itself, or one of its turns.
fn chat_outcome<'a>(outcomes: &'a [TaskOutcome], id: &str) -> Option<&'a TaskOutcome> {
    let turn = format!("{id}:");
    outcomes
        .iter()
        .filter(|o| o.task == id || o.task.starts_with(&turn))
        .max_by_key(|o| (o.at, o.finished_at))
}

fn result_word(r: OutcomeResult) -> &'static str {
    match r {
        OutcomeResult::Success => "success",
        OutcomeResult::Failure => "failure",
        OutcomeResult::Pending => "pending",
    }
}

/// Days of the last week that finished a task, oldest first.
fn streak_days(outcomes: &[TaskOutcome], now: u64) -> [bool; 7] {
    let today = now / DAY_MS;
    let mut out = [false; 7];
    for o in outcomes
        .iter()
        .filter(|o| o.effective(now) == OutcomeResult::Success)
    {
        let day = o.finished_at / DAY_MS;
        if day <= today && today - day < 7 {
            out[6 - (today - day) as usize] = true;
        }
    }
    out
}

/// One chat's checklist (`(id, text, todo status)`) as the cabin projects it,
/// with level and XP from the outcome and card-signal logs.
pub fn mission_page(config_dir: &Path, chat: Option<Chat<'_>>, now: u64) -> MissionPage {
    let outcomes = oc::read_outcomes(config_dir);
    let mission = chat.and_then(|(id, title, todos)| {
        let last = chat_outcome(&outcomes, id);
        let reasons = last.map(|o| o.reasons.clone()).unwrap_or_default();
        let result = last.map(|o| result_word(o.result)).unwrap_or_default();
        let seed = MissionSeed {
            id: id.to_string(),
            title: title.to_string(),
            todos,
            todos_verified: result == "success"
                || reasons.iter().any(|r| r == oc::REASON_VERIFY_OK),
            outcome_result: result.to_string(),
            outcome_reasons: reasons,
            ..MissionSeed::default()
        };
        MissionView::project(&seed).map(|view| mission(id, &view, result == "success"))
    });
    let tally = grokhub_core::xp::tally(
        &outcomes,
        &grokhub_core::card_signals::read_signals(config_dir),
        now,
    );
    let done = mission.as_ref().map_or(0, |m| m.done);
    let (level, xp, frac) = grokhub_core::xp::level_xp(tally.finished, done, tally.bonus);
    MissionPage {
        mission,
        xp: Xp {
            level,
            xp,
            next_level_xp: level * grokhub_core::xp::XP_PER_LEVEL,
            frac,
            today: tally.today,
            streak: streak_days(&outcomes, now),
        },
    }
}

fn mission(id: &str, view: &MissionView, accepted: bool) -> Mission {
    let counted = || {
        view.checklist
            .iter()
            .filter(|c| c.status != MissionCheckStatus::Cancelled)
    };
    let open = view.checklist.iter().any(|c| {
        matches!(
            c.status,
            MissionCheckStatus::Planned | MissionCheckStatus::Attempted
        )
    });
    let total = counted().count();
    Mission {
        conversation_id: id.to_string(),
        title: view.title.clone(),
        status: view.status.as_str().to_string(),
        steps: view
            .checklist
            .iter()
            .map(|c| Step {
                id: c.id.clone(),
                text: c.text.clone(),
                status: c.status,
            })
            .collect(),
        done: counted()
            .filter(|c| c.status == MissionCheckStatus::Verified)
            .count(),
        total,
        can_accept: total > 0 && !open && !accepted,
        accepted,
    }
}

/// Accept as done (`done`) writes an `accepted` success for the chat; Undo
/// writes the `undo` line that supersedes it. Either is a no-op when the log
/// already says so.
pub fn set_mission_done(
    config_dir: &Path,
    chat: Chat<'_>,
    done: bool,
    now: u64,
) -> Result<(), String> {
    let (id, title, _) = chat;
    let page = mission_page(config_dir, Some(chat), now);
    let Some(mission) = page.mission else {
        return Err(NO_MISSION.into());
    };
    let outcomes = oc::read_outcomes(config_dir);
    let last = chat_outcome(&outcomes, id);
    if done {
        if mission.accepted {
            return Ok(());
        }
        if !mission.can_accept {
            return Err(STEPS_OPEN.into());
        }
        let sig = OutcomeSignals {
            accepted: true,
            ..OutcomeSignals::default()
        };
        let line = TaskOutcome::finished(id, &oc::topic_signature(title), &sig, now);
        return oc::append_outcome(config_dir, &line);
    }
    match last.filter(|o| o.result == OutcomeResult::Success) {
        Some(o) => oc::append_outcome(config_dir, &o.superseded(oc::REASON_UNDO, now)),
        None => Ok(()),
    }
}

// ---- Home --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatLine {
    pub id: String,
    pub title: String,
    pub updated: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationLine {
    pub id: String,
    pub name: String,
    /// "weekdays at 09:00 · next Tue 9:00 AM".
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostLine {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub ago: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaLine {
    pub id: String,
    pub title: String,
    /// What Let's do it puts in the chat box.
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameLine {
    pub app_id: u32,
    pub name: String,
}

/// What one tile shows. `Cabin` is a tile only the cabin paints; `Desktop`
/// one a paired browser can't read.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Tile {
    #[serde(rename_all = "camelCase")]
    Objectives {
        mission: Option<Mission>,
        xp: Xp,
    },
    #[serde(rename_all = "camelCase")]
    Streak {
        xp: Xp,
    },
    #[serde(rename_all = "camelCase")]
    RecentChats {
        chats: Vec<ChatLine>,
    },
    #[serde(rename_all = "camelCase")]
    Automations {
        active: usize,
        next: Vec<AutomationLine>,
    },
    #[serde(rename_all = "camelCase")]
    Pulse {
        post: Option<PostLine>,
        idea: Option<IdeaLine>,
    },
    /// The default voice and Nyx's picture for it (a repo path the UI bundles).
    #[serde(rename_all = "camelCase")]
    Personality {
        name: String,
        picture: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    UpNext {
        games: Vec<GameLine>,
    },
    Error {
        message: String,
    },
    Cabin,
    Desktop,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    /// "large", "medium-1" … "small-4".
    pub slot: &'static str,
    /// "large", "medium" or "small": the CSS grid's span.
    pub size: &'static str,
    pub module: String,
    pub name: String,
    pub purpose: String,
    pub tile: Tile,
}

/// The two counts no layout slot holds: learned memory and connectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Glance {
    pub memories: Option<usize>,
    pub proposed: usize,
    pub connectors: usize,
    pub connected: usize,
    pub failing: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub preset: String,
    pub xp_bar: bool,
    pub slots: Vec<Slot>,
    pub xp: Xp,
    pub glance: Glance,
}

fn slot_key(slot: SlotId) -> &'static str {
    match slot {
        SlotId::Large => "large",
        SlotId::Medium1 => "medium-1",
        SlotId::Medium2 => "medium-2",
        SlotId::Small1 => "small-1",
        SlotId::Small2 => "small-2",
        SlotId::Small3 => "small-3",
        SlotId::Small4 => "small-4",
    }
}

fn size_key(size: ModuleSize) -> &'static str {
    match size {
        ModuleSize::Large => "large",
        ModuleSize::Medium => "medium",
        ModuleSize::Small => "small",
    }
}

/// The layout's visible slots in the cabin's order (Large, Mediums, Smalls),
/// each with its module id. `place_slots` owns which slots are hidden; the
/// rect only picks its wide arrangement, and the shell keeps no pixels.
pub fn visible_slots(layout: &HomeLayout) -> Vec<(SlotId, String)> {
    place_slots((0.0, 0.0, 1600.0, 470.0), layout)
        .into_iter()
        .filter_map(|p| p.module_id.map(|id| (p.slot, id)))
        .collect()
}

/// The shell's half of the catalog: which modules it fills itself.
pub fn shell_fills(module: &str) -> bool {
    matches!(
        module,
        "objectives"
            | "streak"
            | "recent_chats"
            | "automations"
            | "pulse"
            | "personality"
            | "up_next"
    )
}

/// The open chat's title and to-dos, for its mission.
fn chat_todos(config_dir: &Path, id: &str) -> (String, Todos) {
    let title = history::load(config_dir, id, 1)
        .map(|t| t.title)
        .unwrap_or_default();
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let todos = grokhub_agent::todos_for(id)
        .into_iter()
        .map(|t| (t.id, t.content, t.status))
        .collect();
    (title, todos)
}

/// Objectives for whichever chat `src` points at.
pub fn mission_for(src: &Sources<'_>, now: u64) -> MissionPage {
    let chat = src.mission_chat();
    let found = chat
        .as_deref()
        .map(|id| (id, chat_todos(src.config_dir, id)));
    mission_page(
        src.config_dir,
        found
            .as_ref()
            .map(|(id, (title, todos))| (*id, title.as_str(), todos.clone())),
        now,
    )
}

/// Accept as done or Undo, for the chat `src` points at. Desktop only.
pub fn toggle_for(src: &Sources<'_>, done: bool, now: u64) -> Result<MissionPage, String> {
    if src.viewer.web() {
        return Err(crate::remote::refused());
    }
    let id = src.mission_chat().ok_or(NO_MISSION)?;
    let (title, todos) = chat_todos(src.config_dir, &id);
    set_mission_done(src.config_dir, (&id, &title, todos), done, now)?;
    Ok(mission_for(src, now))
}

fn recent_chats(src: &Sources<'_>) -> Tile {
    let chats = history::list(src.config_dir)
        .into_iter()
        .filter(|row| src.viewer.sees(&row.id))
        .take(TILE_ROWS)
        .map(|row| ChatLine {
            id: row.id,
            title: row.title,
            updated: row.updated,
        })
        .collect();
    Tile::RecentChats { chats }
}

fn automations_tile(src: &Sources<'_>) -> Tile {
    let list = match automations::load(src.config_dir) {
        Ok(list) => list,
        Err(message) => return Tile::Error { message },
    };
    let mut on: Vec<_> = list.iter().filter(|a| a.enabled).collect();
    on.sort_by_key(|a| (a.next_run.unwrap_or(u64::MAX), a.name.clone()));
    let rows = automations::rows(
        &on.iter()
            .take(TILE_ROWS)
            .map(|a| (*a).clone())
            .collect::<Vec<_>>(),
        src.clock,
    );
    Tile::Automations {
        active: on.len(),
        next: rows
            .into_iter()
            .map(|r| AutomationLine {
                id: r.id,
                name: r.name,
                summary: r.summary,
            })
            .collect(),
    }
}

fn pulse_tile(src: &Sources<'_>) -> Tile {
    if !src.caps.pulse {
        return Tile::Cabin;
    }
    match pulse::peek(src.config_dir, src.clock.now_ms) {
        Ok(page) => Tile::Pulse {
            post: page.feed.into_iter().next().map(|p| PostLine {
                id: p.id,
                title: p.title,
                summary: p.summary,
                ago: p.ago,
            }),
            idea: page
                .ideas
                .into_iter()
                .flat_map(|g| g.ideas)
                .next()
                .map(|i| IdeaLine {
                    id: i.id,
                    title: i.title,
                    action: i.action,
                }),
        },
        Err(message) => Tile::Error { message },
    }
}

fn personality_tile(src: &Sources<'_>) -> Tile {
    let (dir, s) = (src.config_dir, src.settings);
    let page = personality::load(dir, &s.personality, s.explicit_personalities);
    let name = page
        .presets
        .iter()
        .find(|p| p.id == page.current)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let picture = nyx::look(
        dir,
        &s.personality,
        s.explicit_personalities,
        grokhub_core::companion::Mood::HangingOut,
    )
    .and_then(|look| look.poses.into_iter().next());
    Tile::Personality { name, picture }
}

fn up_next_tile(src: &Sources<'_>) -> Tile {
    if src.viewer.web() {
        return Tile::Desktop;
    }
    if !src.caps.library {
        return Tile::Cabin;
    }
    let page = library::page(grokhub_core::steam::detect_root().as_deref());
    Tile::UpNext {
        games: page
            .games
            .into_iter()
            .take(TILE_ROWS)
            .map(|g| GameLine {
                app_id: g.app_id,
                name: g.name,
            })
            .collect(),
    }
}

fn glance(src: &Sources<'_>) -> Glance {
    let notes = memory::load(src.workspace).ok();
    let rows = connectors::list(src.workspace);
    let on: Vec<_> = rows.iter().filter(|r| r.enabled).collect();
    Glance {
        memories: notes
            .as_ref()
            .map(|n| n.rows.iter().filter(|r| !r.proposed).count()),
        proposed: notes
            .as_ref()
            .map_or(0, |n| n.rows.iter().filter(|r| r.proposed).count()),
        connectors: on.len(),
        connected: on.iter().filter(|r| r.status == "connected").count(),
        failing: on.iter().filter(|r| r.status == "error").count(),
    }
}

/// Home: the layout's visible slots, each filled or left to the cabin.
pub fn page(src: &Sources<'_>) -> Page {
    let layout = load_layout(src.config_dir);
    let now = src.clock.now_ms;
    let objectives = mission_for(src, now);
    let slots = visible_slots(&layout)
        .into_iter()
        .map(|(slot, module)| {
            let entry = lookup(&module);
            let tile = match module.as_str() {
                "objectives" => Tile::Objectives {
                    mission: objectives.mission.clone(),
                    xp: objectives.xp.clone(),
                },
                "streak" => Tile::Streak {
                    xp: objectives.xp.clone(),
                },
                "recent_chats" => recent_chats(src),
                "automations" => automations_tile(src),
                "pulse" => pulse_tile(src),
                "personality" => personality_tile(src),
                "up_next" => up_next_tile(src),
                _ => Tile::Cabin,
            };
            Slot {
                slot: slot_key(slot),
                size: size_key(slot.size()),
                name: entry.map(|e| e.name.to_string()).unwrap_or_default(),
                purpose: entry.map(|e| e.purpose.to_string()).unwrap_or_default(),
                module,
                tile,
            }
        })
        .collect();
    Page {
        preset: layout.preset.clone(),
        xp_bar: layout.xp_bar,
        slots,
        xp: objectives.xp,
        glance: glance(src),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::home::HomePreset;
    use std::fs;
    use std::path::PathBuf;

    const NOW: u64 = 40 * DAY_MS + 3_600_000;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "grokhub-shell-home-{tag}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn todo(id: &str, text: &str, status: &str) -> (String, String, String) {
        (id.into(), text.into(), status.into())
    }

    #[test]
    fn a_missing_layout_is_the_default_and_no_file_is_written() {
        let d = dir("missing");
        assert_eq!(load_layout(&d), HomeLayout::default());
        assert!(!d.join(HOME_LAYOUT_FILE).exists());
        let slots = visible_slots(&load_layout(&d));
        assert_eq!(
            slots,
            vec![
                (SlotId::Large, "objectives".to_string()),
                (SlotId::Medium1, "pc_loadout".to_string()),
                (SlotId::Small1, "up_next".to_string()),
            ]
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_broken_layout_is_the_default_and_the_file_is_left_alone() {
        let d = dir("broken");
        let path = d.join(HOME_LAYOUT_FILE);
        for bad in ["{ not json", "[1, 2]", "\"text\"", ""] {
            fs::write(&path, bad).unwrap();
            assert_eq!(load_layout(&d), HomeLayout::default(), "{bad:?}");
            assert_eq!(fs::read_to_string(&path).unwrap(), bad, "rewrote {bad:?}");
        }
        // A v2 file of the wrong shape (small is not four slots).
        let wrong = r#"{"version":2,"large":"pulse","medium":[],"small":["a"]}"#;
        fs::write(&path, wrong).unwrap();
        assert_eq!(load_layout(&d), HomeLayout::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), wrong);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_cabin_written_v1_layout_migrates_in_memory_only() {
        let d = dir("v1");
        let v1 = r#"{
            "version": 1,
            "preset": "streamer",
            "large": "pulse",
            "medium": ["recent_chats", "gone_module"],
            "small": ["automations", null, "streak", "up_next", "extra"],
            "xp_bar": false
        }"#;
        fs::write(d.join(HOME_LAYOUT_FILE), v1).unwrap();
        let layout = load_layout(&d);
        assert_eq!(layout.version, 2);
        assert_eq!(layout.preset, "streamer");
        assert_eq!(layout.large, "pulse");
        // An id the catalog dropped falls back to the default's slot (empty).
        assert_eq!(
            layout.medium,
            vec!["recent_chats".to_string(), String::new()]
        );
        assert_eq!(
            layout.small,
            [
                Some("automations".into()),
                None,
                Some("streak".into()),
                Some("up_next".into())
            ]
        );
        assert!(!layout.xp_bar);
        assert_eq!(fs::read_to_string(d.join(HOME_LAYOUT_FILE)).unwrap(), v1);
        let slots: Vec<&str> = visible_slots(&layout)
            .iter()
            .map(|(s, _)| slot_key(*s))
            .collect();
        assert_eq!(
            slots,
            ["large", "medium-1", "small-1", "small-3", "small-4"]
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_v2_layout_with_a_preset_loads_as_saved() {
        let d = dir("v2");
        let saved = HomeLayout::preset(HomePreset::Performance);
        fs::write(
            d.join(HOME_LAYOUT_FILE),
            serde_json::to_string(&saved).unwrap(),
        )
        .unwrap();
        assert_eq!(load_layout(&d), saved);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_shell_fills_its_tiles_and_leaves_the_rest_to_the_cabin() {
        for id in [
            "objectives",
            "recent_chats",
            "automations",
            "pulse",
            "up_next",
        ] {
            assert!(shell_fills(id), "{id}");
            assert!(lookup(id).is_some(), "{id} is not in the catalog");
        }
        for id in ["pc_loadout", "storage", "game_mode", "frame_rate", "clips"] {
            assert!(!shell_fills(id), "{id}");
        }
    }

    fn outcome(task: &str, result: OutcomeResult, finished_at: u64) -> TaskOutcome {
        TaskOutcome {
            task: task.into(),
            signature: "topic:fixture".into(),
            result,
            reasons: if result == OutcomeResult::Success {
                vec![oc::REASON_VERIFY_OK.into()]
            } else {
                Vec::new()
            },
            finished_at,
            at: finished_at,
            ..TaskOutcome::default()
        }
    }

    #[test]
    fn the_mission_from_a_fixture_log_has_the_right_progress_and_xp() {
        let d = dir("mission");
        // Two finished tasks from earlier days, one undone, and this chat's
        // turn verified today.
        for o in [
            outcome("shell-old:1", OutcomeResult::Success, NOW - 3 * DAY_MS),
            outcome("shell-old:2", OutcomeResult::Success, NOW - 3 * DAY_MS + 5),
            outcome("shell-bad:1", OutcomeResult::Failure, NOW - DAY_MS),
            outcome("shell-m:4", OutcomeResult::Success, NOW - 60_000),
        ] {
            oc::append_outcome(&d, &o).unwrap();
        }
        let todos = vec![
            todo("1", "Read the logs", "completed"),
            todo("2", "Patch the config", "completed"),
            todo("3", "Skip the old way", "cancelled"),
            todo("4", "Restart the service", "in_progress"),
        ];
        let page = mission_page(&d, Some(("shell-m", "Fix the server", todos)), NOW);
        let m = page.mission.expect("a mission");
        assert_eq!(m.title, "Fix the server");
        assert_eq!((m.done, m.total), (2, 3));
        assert_eq!(m.status, "active");
        assert!(!m.can_accept, "a step is still open");
        assert!(m.accepted);
        let steps: Vec<MissionCheckStatus> = m.steps.iter().map(|s| s.status).collect();
        assert_eq!(
            steps,
            [
                MissionCheckStatus::Verified,
                MissionCheckStatus::Verified,
                MissionCheckStatus::Cancelled,
                MissionCheckStatus::Attempted
            ]
        );
        // 3 finished tasks × 100 + 2 verified steps × 25 + 2 active days × 10.
        assert_eq!(page.xp.xp, 370);
        assert_eq!(page.xp.level, 1);
        assert_eq!(page.xp.next_level_xp, 500);
        assert!((page.xp.frac - 0.74).abs() < 1e-6, "{}", page.xp.frac);
        // Today: the one task (100) and the day (10).
        assert_eq!(page.xp.today, 110);
        assert_eq!(
            page.xp.streak,
            [false, false, false, true, false, false, true]
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn no_checklist_means_no_mission_but_xp_still_counts() {
        let d = dir("empty");
        oc::append_outcome(&d, &outcome("shell-x:1", OutcomeResult::Success, NOW)).unwrap();
        let page = mission_page(&d, Some(("shell-quiet", "", Vec::new())), NOW);
        assert_eq!(page.mission, None);
        assert_eq!(page.xp.xp, 110);
        assert_eq!(mission_page(&d, None, NOW).xp.xp, 110);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn accept_writes_an_accepted_line_and_undo_supersedes_it() {
        let d = dir("accept");
        let todos = || {
            vec![
                todo("1", "Write it", "completed"),
                todo("2", "Ship it", "completed"),
            ]
        };
        let before = mission_page(&d, Some(("shell-a", "Ship", todos())), NOW);
        let m = before.mission.expect("mission");
        assert_eq!((m.done, m.total), (0, 2), "completed but not verified yet");
        assert!(m.can_accept && !m.accepted);
        assert_eq!(before.xp.xp, 0);

        set_mission_done(&d, ("shell-a", "Ship", todos()), true, NOW).unwrap();
        let lines = oc::read_outcome_lines(&d);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].task, "shell-a");
        assert_eq!(lines[0].result, OutcomeResult::Success);
        assert_eq!(lines[0].reasons, vec![oc::REASON_ACCEPTED.to_string()]);
        assert_eq!(lines[0].signature, oc::topic_signature("Ship"));
        let after = mission_page(&d, Some(("shell-a", "Ship", todos())), NOW);
        let m = after.mission.expect("mission");
        assert_eq!((m.done, m.total), (2, 2));
        assert_eq!(m.status, "done");
        assert!(m.accepted && !m.can_accept);
        // One task, two steps, one active day.
        assert_eq!(after.xp.xp, 100 + 50 + 10);

        // Accepting twice writes nothing more.
        set_mission_done(&d, ("shell-a", "Ship", todos()), true, NOW + 1).unwrap();
        assert_eq!(oc::read_outcome_lines(&d).len(), 1);

        set_mission_done(&d, ("shell-a", "Ship", todos()), false, NOW + 2).unwrap();
        let lines = oc::read_outcome_lines(&d);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].result, OutcomeResult::Failure);
        assert_eq!(lines[1].reasons, vec![oc::REASON_UNDO.to_string()]);
        let undone = mission_page(&d, Some(("shell-a", "Ship", todos())), NOW + 2);
        let m = undone.mission.expect("mission");
        assert_eq!((m.done, m.total), (0, 2));
        assert!(m.can_accept && !m.accepted);
        assert_eq!(undone.xp.xp, 0);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn accept_refuses_open_steps_and_a_chat_with_no_checklist() {
        let d = dir("refuse");
        let open = vec![todo("1", "Plan", "pending")];
        assert_eq!(
            set_mission_done(&d, ("shell-o", "T", open), true, NOW),
            Err(STEPS_OPEN.to_string())
        );
        assert_eq!(
            set_mission_done(&d, ("shell-n", "T", Vec::new()), true, NOW),
            Err(NO_MISSION.to_string())
        );
        assert!(oc::read_outcome_lines(&d).is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_browser_sees_only_its_own_chat_and_the_desktop_falls_back_to_the_last() {
        let settings = ShellSettings {
            last_chat: "shell-last".into(),
            ..ShellSettings::default()
        };
        let owns = |id: &str| id == "shell-web";
        let src = |viewer, asked: Option<&str>| Sources {
            config_dir: Path::new("."),
            workspace: Path::new("."),
            settings: &settings,
            caps: crate::capabilities::current(),
            clock: LocalClock {
                now_ms: NOW,
                weekday: 1,
                hour: 9,
                minute: 0,
            },
            viewer,
            conversation_id: asked.map(str::to_string),
        };
        assert_eq!(
            src(Viewer::Desktop, None).mission_chat().as_deref(),
            Some("shell-last")
        );
        assert_eq!(
            src(Viewer::Desktop, Some("shell-open"))
                .mission_chat()
                .as_deref(),
            Some("shell-open")
        );
        assert_eq!(src(Viewer::Web(&owns), None).mission_chat(), None);
        assert_eq!(
            src(Viewer::Web(&owns), Some("shell-last")).mission_chat(),
            None
        );
        assert_eq!(
            src(Viewer::Web(&owns), Some("shell-web"))
                .mission_chat()
                .as_deref(),
            Some("shell-web")
        );
        assert_eq!(
            toggle_for(&src(Viewer::Web(&owns), Some("shell-web")), true, NOW),
            Err(crate::remote::refused())
        );
    }
}
