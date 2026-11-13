//! Home and Objectives in the shell (LO-17n). Home draws the cabin's tile
//! grid from the same `{config}/home_layout.json` and fills every tile:
//! the ones below have their own look, the rest come from `home_tiles` in
//! the shared tile model. Objectives is the
//! open chat's checklist with level, XP and weeks in a row.
//!
//! The layout, the catalog and the mission and XP sums are the cabin's own
//! (`grokhub_core::home`, `mission`, `xp`, `outcome`). A missing or broken
//! layout shows the default and is left alone until Edit Home saves one
//! ([`save_pick`]), the same file the cabin writes. The other write here is
//! Accept as done (and its Undo) on Objectives, an `accepted` or `undo` line
//! in the cabin's outcome log, as the cabin's own self-review writes them.

use std::path::Path;

use grokhub_core::home::{
    lookup, place_slots, HomeLayout, HomePick, ModuleSize, SlotId, TileView, HOME_LAYOUT_FILE,
    HOME_LAYOUT_VERSION, HOME_PRESETS, REGISTRY,
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

/// One module Edit Home can place, with the sizes it draws at.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModule {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub sizes: Vec<&'static str>,
    pub purpose: &'static str,
}

/// A named layout Edit Home offers as a starting point.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub pick: HomePick,
}

/// What Edit Home needs: today's slots, every module and the presets.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub current: HomePick,
    pub modules: Vec<CatalogModule>,
    pub presets: Vec<CatalogPreset>,
}

pub fn catalog(config_dir: &Path) -> Catalog {
    Catalog {
        current: load_layout(config_dir).pick(),
        modules: REGISTRY
            .iter()
            .map(|m| CatalogModule {
                id: m.id,
                name: m.name,
                category: m.category.label(),
                sizes: m.sizes.iter().map(|s| size_key(*s)).collect(),
                purpose: m.purpose,
            })
            .collect(),
        presets: HOME_PRESETS
            .iter()
            .map(|kind| CatalogPreset {
                id: kind.id(),
                label: kind.label(),
                pick: HomeLayout::preset(*kind).pick(),
            })
            .collect(),
    }
}

/// Edit Home → Save: the picked slots into `home_layout.json`, keeping the
/// XP bar and each module's settings. Written whole through a temp file, so
/// the cabin never reads half a layout.
pub fn save_pick(config_dir: &Path, pick: &HomePick) -> Result<(), String> {
    let layout = load_layout(config_dir).with_pick(pick);
    let text = serde_json::to_string_pretty(&layout).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&config_dir.join(HOME_LAYOUT_FILE), &text)
        .map_err(|_| "Couldn't save Home. Try again.".to_string())
}

/// Who is looking: this computer's window, or a paired browser that may see
/// only the chats it started.
pub enum Viewer<'a> {
    Desktop,
    Web(&'a dyn Fn(&str) -> bool),
}

impl Viewer<'_> {
    pub(crate) fn sees(&self, conversation_id: &str) -> bool {
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

/// Level, title, XP and this week, as the cabin shows them.
pub type Xp = grokhub_core::xp::XpSummary;

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

/// One chat's checklist (`(id, text, todo status)`) as the cabin projects it,
/// with level and XP from the XP ledger, which banks the chat's checked steps.
/// `offset_min` is the local clock's offset from UTC, for days and weeks.
pub fn mission_page(
    config_dir: &Path,
    chat: Option<Chat<'_>>,
    now: u64,
    offset_min: i32,
) -> MissionPage {
    let outcomes = oc::read_outcomes(config_dir);
    let projected = chat.and_then(|(id, title, todos)| {
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
        MissionView::project(&seed).map(|view| (id, view, result == "success"))
    });
    let open = projected
        .as_ref()
        .map(|(id, view, _)| grokhub_core::xp::OpenChat {
            id,
            items: &view.checklist,
        });
    let xp = grokhub_core::xp::refresh(config_dir, open, now, offset_min);
    MissionPage {
        mission: projected.map(|(id, view, accepted)| mission(id, &view, accepted)),
        xp,
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
    offset_min: i32,
) -> Result<(), String> {
    let (id, title, _) = chat;
    let page = mission_page(config_dir, Some(chat), now, offset_min);
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

/// What one tile shows. `View` is any module in the shared tile model.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Tile {
    #[serde(rename_all = "camelCase")]
    Objectives {
        mission: Option<Mission>,
        xp: Xp,
        /// A follow-up on a goal left a few days, with "Plan it" under it.
        #[serde(skip_serializing_if = "Option::is_none")]
        follow_up: Option<grokhub_core::objectives::FollowUp>,
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
    View {
        view: TileView,
    },
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
    /// How often this tile's data changes, so the screen asks again on time.
    pub refresh_secs: Option<u64>,
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

/// A shared-model tile, stamped with when it was read.
fn module_view(module: &str, src: &Sources<'_>, now: u64) -> TileView {
    let web = matches!(src.viewer, Viewer::Web(_));
    if let Some(v) = crate::home_tiles::cached(module, web, now) {
        return v;
    }
    let mut view = crate::home_tiles::view(module, src)
        .unwrap_or_else(|| TileView::unsupported("This tile isn't in this version"));
    view.updated_ms.get_or_insert(now);
    crate::home_tiles::keep(module, web, &view);
    view
}

/// The modules with a look of their own here; every other one is a
/// shared-model tile.
pub fn own_look(module: &str) -> bool {
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

/// At the end of a chat turn: banks the chat's checked steps, and once
/// every step is closed (what Accept as done asks for), the finished task counts for
/// XP as a pending line on the chat, as the cabin's turn-end review writes
/// one. Accept as done later writes on the same chat, so nothing pays twice;
/// a chat with any outcome line already (accepted, undone) is left alone.
/// True when a line was written.
pub fn turn_done(config_dir: &Path, chat: Chat<'_>, now: u64, offset_min: i32) -> bool {
    let (id, title, _) = chat;
    let title = title.to_string();
    let page = mission_page(config_dir, Some(chat), now, offset_min);
    let Some(m) = page.mission else { return false };
    if !m.can_accept {
        return false;
    }
    if chat_outcome(&oc::read_outcomes(config_dir), id).is_some() {
        return false;
    }
    let sig = oc::topic_signature(&title);
    let line = TaskOutcome::finished(id, &sig, &OutcomeSignals::default(), now);
    oc::append_outcome(config_dir, &line).is_ok()
}

/// [`turn_done`] for a chat by id, with its saved title and to-dos.
pub fn turn_done_for(config_dir: &Path, id: &str, now: u64, offset_min: i32) -> bool {
    let (title, todos) = chat_todos(config_dir, id);
    turn_done(config_dir, (id, &title, todos), now, offset_min)
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
        src.clock.utc_offset_min(),
    )
}

/// Accept as done or Undo, for the chat `src` points at. Desktop only.
pub fn toggle_for(src: &Sources<'_>, done: bool, now: u64) -> Result<MissionPage, String> {
    if src.viewer.web() {
        return Err(crate::remote::refused());
    }
    let id = src.mission_chat().ok_or(NO_MISSION)?;
    let (title, todos) = chat_todos(src.config_dir, &id);
    set_mission_done(
        src.config_dir,
        (&id, &title, todos),
        done,
        now,
        src.clock.utc_offset_min(),
    )?;
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
        return Tile::View {
            view: TileView::unsupported("Pulse isn't on this device"),
        };
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

/// The name of the personality every chat uses by default.
pub(crate) fn personality_name(src: &Sources<'_>) -> String {
    let (dir, s) = (src.config_dir, src.settings);
    let page = personality::load(dir, s);
    page.presets
        .iter()
        .find(|p| p.id == page.current)
        .map(|p| p.name.clone())
        .unwrap_or_default()
}

fn personality_tile(src: &Sources<'_>) -> Tile {
    let (dir, s) = (src.config_dir, src.settings);
    let name = personality_name(src);
    let picture = nyx::look(
        dir,
        &s.personality,
        s.adults_on(),
        grokhub_core::companion::Mood::HangingOut,
    )
    .and_then(|look| look.poses.into_iter().next());
    Tile::Personality { name, picture }
}

fn up_next_tile(src: &Sources<'_>) -> Tile {
    if src.viewer.web() || !src.caps.library {
        return Tile::View {
            view: TileView::unsupported(&crate::home_tiles::on_pc()),
        };
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

/// Home: the layout's visible slots, every one filled.
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
                    follow_up: crate::goals::follow_up(src.config_dir, now),
                },
                "streak" => Tile::Streak {
                    xp: objectives.xp.clone(),
                },
                "recent_chats" => recent_chats(src),
                "automations" => automations_tile(src),
                "pulse" => pulse_tile(src),
                "personality" => personality_tile(src),
                "up_next" => up_next_tile(src),
                other => Tile::View {
                    view: module_view(other, src, now),
                },
            };
            Slot {
                slot: slot_key(slot),
                size: size_key(slot.size()),
                name: entry.map(|e| e.name.to_string()).unwrap_or_default(),
                purpose: entry.map(|e| e.purpose.to_string()).unwrap_or_default(),
                refresh_secs: grokhub_core::home::refresh_secs(&module),
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

    const DAY_MS: u64 = 86_400_000;
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
    fn every_catalog_module_gets_a_real_tile() {
        use grokhub_core::home::{TileState, REGISTRY};
        for id in [
            "objectives",
            "recent_chats",
            "automations",
            "pulse",
            "up_next",
        ] {
            assert!(own_look(id), "{id}");
            assert!(lookup(id).is_some(), "{id} is not in the catalog");
        }
        let d = dir("every-tile");
        let settings = ShellSettings::default();
        let src = Sources {
            config_dir: &d,
            workspace: &d,
            settings: &settings,
            caps: crate::capabilities::current(),
            clock: LocalClock {
                now_ms: NOW,
                weekday: 1,
                hour: 9,
                minute: 0,
            },
            viewer: Viewer::Desktop,
            conversation_id: None,
        };
        for m in REGISTRY.iter().filter(|m| !own_look(m.id)) {
            let view = crate::home_tiles::view(m.id, &src)
                .unwrap_or_else(|| panic!("{} has no tile", m.id));
            assert!(!view.text.caption.is_empty(), "{} says nothing", m.id);
            if matches!(
                view.state,
                TileState::Empty | TileState::Off | TileState::Error
            ) {
                assert_eq!(view.buttons.len(), 1, "{} needs one way forward", m.id);
            }
            let tile = Tile::View {
                view: module_view(m.id, &src, NOW),
            };
            let json = serde_json::to_value(&tile).unwrap();
            assert_eq!(json["kind"], "view", "{}", m.id);
            assert!(json["view"]["updatedMs"].is_u64(), "{}", m.id);
        }
        let _ = fs::remove_dir_all(&d);
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
        let page = mission_page(&d, Some(("shell-m", "Fix the server", todos)), NOW, 0);
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
        // 3 finished tasks × 100 + 2 banked steps × 25 + 2 active days × 10.
        assert_eq!(page.xp.xp, 370);
        // Level 1 takes 200, level 2 takes 250: 170 into level 2.
        assert_eq!((page.xp.level, page.xp.title), (2, "Rookie"));
        assert_eq!(page.xp.next_level_xp, 450);
        assert!((page.xp.frac - 0.68).abs() < 1e-6, "{}", page.xp.frac);
        // Today: the one task (100), its two steps (50) and the day (10).
        assert_eq!(page.xp.today, 160);
        // Day 40 is a Tuesday; the finished tasks three days back were last Saturday.
        assert_eq!(
            page.xp.week.days,
            [false, true, false, false, false, false, false]
        );
        assert_eq!(page.xp.week.today, 1);
        assert_eq!((page.xp.week.active, page.xp.week.target), (1, 2));
        assert_eq!(page.xp.week.in_a_row, 2);
        // The steps stay banked when the chat's checklist is gone.
        assert_eq!(mission_page(&d, None, NOW, 0).xp.xp, 370);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn no_checklist_means_no_mission_but_xp_still_counts() {
        let d = dir("empty");
        oc::append_outcome(&d, &outcome("shell-x:1", OutcomeResult::Success, NOW)).unwrap();
        let page = mission_page(&d, Some(("shell-quiet", "", Vec::new())), NOW, 0);
        assert_eq!(page.mission, None);
        assert_eq!(page.xp.xp, 110);
        assert_eq!(mission_page(&d, None, NOW, 0).xp.xp, 110);
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
        let before = mission_page(&d, Some(("shell-a", "Ship", todos())), NOW, 0);
        let m = before.mission.expect("mission");
        assert_eq!((m.done, m.total), (0, 2), "completed but not verified yet");
        assert!(m.can_accept && !m.accepted);
        assert_eq!(before.xp.xp, 0);

        set_mission_done(&d, ("shell-a", "Ship", todos()), true, NOW, 0).unwrap();
        let lines = oc::read_outcome_lines(&d);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].task, "shell-a");
        assert_eq!(lines[0].result, OutcomeResult::Success);
        assert_eq!(lines[0].reasons, vec![oc::REASON_ACCEPTED.to_string()]);
        assert_eq!(lines[0].signature, oc::topic_signature("Ship"));
        let after = mission_page(&d, Some(("shell-a", "Ship", todos())), NOW, 0);
        let m = after.mission.expect("mission");
        assert_eq!((m.done, m.total), (2, 2));
        assert_eq!(m.status, "done");
        assert!(m.accepted && !m.can_accept);
        // One task, two steps, one active day.
        assert_eq!(after.xp.xp, 100 + 50 + 10);

        // Accepting twice writes nothing more.
        set_mission_done(&d, ("shell-a", "Ship", todos()), true, NOW + 1, 0).unwrap();
        assert_eq!(oc::read_outcome_lines(&d).len(), 1);

        set_mission_done(&d, ("shell-a", "Ship", todos()), false, NOW + 2, 0).unwrap();
        let lines = oc::read_outcome_lines(&d);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].result, OutcomeResult::Failure);
        assert_eq!(lines[1].reasons, vec![oc::REASON_UNDO.to_string()]);
        let undone = mission_page(&d, Some(("shell-a", "Ship", todos())), NOW + 2, 0);
        let m = undone.mission.expect("mission");
        assert_eq!((m.done, m.total), (0, 2));
        assert!(m.can_accept && !m.accepted);
        assert_eq!(undone.xp.xp, 0);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_turn_that_closes_the_checklist_counts_the_task_once() {
        let d = dir("turn");
        let open = || {
            vec![
                todo("1", "Write it", "completed"),
                todo("2", "Ship it", "in_progress"),
            ]
        };
        let closed = || {
            vec![
                todo("1", "Write it", "completed"),
                todo("2", "Ship it", "completed"),
            ]
        };
        assert!(!turn_done(&d, ("shell-t", "Ship", open()), NOW, 0));
        assert!(!turn_done(&d, ("shell-n", "Ship", Vec::new()), NOW, 0));
        assert!(oc::read_outcome_lines(&d).is_empty());

        assert!(turn_done(&d, ("shell-t", "Ship", closed()), NOW, 0));
        let lines = oc::read_outcome_lines(&d);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].task, "shell-t");
        assert_eq!(lines[0].result, OutcomeResult::Pending);
        assert_eq!(lines[0].signature, oc::topic_signature("Ship"));
        let page = mission_page(&d, Some(("shell-t", "Ship", closed())), NOW, 0);
        // One task and one active day; the steps wait for Accept as done.
        assert_eq!(page.xp.xp, 100 + 10);
        assert!(page.mission.expect("mission").can_accept);

        // The next turn writes nothing more, and Accept as done pays no second task.
        assert!(!turn_done(&d, ("shell-t", "Ship", closed()), NOW + 1, 0));
        set_mission_done(&d, ("shell-t", "Ship", closed()), true, NOW + 2, 0).unwrap();
        let page = mission_page(&d, Some(("shell-t", "Ship", closed())), NOW + 2, 0);
        assert_eq!(page.xp.xp, 100 + 50 + 10);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn accept_refuses_open_steps_and_a_chat_with_no_checklist() {
        let d = dir("refuse");
        let open = vec![todo("1", "Plan", "pending")];
        assert_eq!(
            set_mission_done(&d, ("shell-o", "T", open), true, NOW, 0),
            Err(STEPS_OPEN.to_string())
        );
        assert_eq!(
            set_mission_done(&d, ("shell-n", "T", Vec::new()), true, NOW, 0),
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

    #[test]
    fn edit_home_saves_the_picked_slots_and_keeps_the_rest() {
        let d = dir("edit");
        let cat = catalog(&d);
        assert_eq!(cat.current, HomeLayout::default().pick());
        assert_eq!(cat.presets.len(), 3);
        assert!(cat.modules.iter().all(|m| !m.sizes.is_empty()));
        assert!(!d.join(HOME_LAYOUT_FILE).exists(), "reading writes nothing");

        let base = HomeLayout {
            xp_bar: false,
            ..HomeLayout::default()
        };
        fs::write(
            d.join(HOME_LAYOUT_FILE),
            serde_json::to_string(&base).unwrap(),
        )
        .unwrap();
        let streamer = cat.presets.iter().find(|p| p.id == "streamer").unwrap();
        save_pick(&d, &streamer.pick).unwrap();
        let saved = load_layout(&d);
        assert_eq!(saved.large, "clips");
        assert_eq!(saved.preset, "streamer");
        assert!(!saved.xp_bar);
        let slots: Vec<String> = visible_slots(&saved).into_iter().map(|(_, m)| m).collect();
        assert!(slots.contains(&"squad".to_string()), "{slots:?}");
        let _ = fs::remove_dir_all(&d);
    }
}
