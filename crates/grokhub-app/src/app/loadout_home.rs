//! Loadout home dock (deck slide 2): the companion docked top-right as a
//! still picture, with Objectives, PC loadout and From Pulse beside the chat.
//! Every panel shows real state only: the open mission's checklist, this
//! machine's hardware profile and the Pulse home cards. Narrow windows drop
//! the dock so the chat keeps its column.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use grokhub_core::local_profile::{LocalProfile, Thermal};
use grokhub_core::mission::{MissionCheckItem, MissionCheckStatus, MissionView};
use grokhub_core::{ChipKind, QuickChip, UpdateCard, UpdateStatus};

use super::*;

mod chat_first;

/// The companion art slots, one per mood (slide 17). Swap a file to change
/// that picture. `companion.png` is the hanging-out pose.
const COMPANION_PNG: &[u8] = include_bytes!("../../../../brand/companion.png");
const COMPANION_HELLO_PNG: &[u8] = include_bytes!("../../../../brand/companion-hello.png");
const COMPANION_HELPING_PNG: &[u8] = include_bytes!("../../../../brand/companion-helping.png");
/// Below this page width the dock hides and the chat gets the whole page.
/// The default 1100 px window leaves about 840 px beside the rail, so the
/// dock shows there in its compact width.
const DOCK_MIN_PAGE_W: f32 = 780.0;
/// Companion art height: the compact dock keeps 196, wider docks grow to 260.
const ART_H: f32 = 196.0;
const ART_MAX_H: f32 = 260.0;
const MISSION_REFRESH: Duration = Duration::from_secs(2);
const PROFILE_REFRESH: Duration = Duration::from_secs(120);
const XP_REFRESH: Duration = Duration::from_secs(30);
const PULSE_LINES: usize = 2;
/// Listening waveform at rest, bar heights as a share of the row.
const WAVE: [f32; 9] = [0.35, 0.7, 1.0, 0.5, 0.85, 0.4, 0.95, 0.6, 0.3];
/// XP for each finished task in the outcome log and each checked step.
const XP_PER_TASK: u32 = 100;
const XP_PER_STEP: u32 = 25;
pub(super) const XP_PER_LEVEL: u32 = 500;

/// The companion's pose on slide 17: Hello waves you in, Helping holds the
/// checklist while a tool runs, Hanging out is just talking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mood {
    Hello,
    Helping,
    HangingOut,
}

impl Mood {
    fn png(self) -> &'static [u8] {
        match self {
            Mood::Hello => COMPANION_HELLO_PNG,
            Mood::Helping => COMPANION_HELPING_PNG,
            Mood::HangingOut => COMPANION_PNG,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Mood::Hello => "loadout-companion-hello",
            Mood::Helping => "loadout-companion-helping",
            Mood::HangingOut => "loadout-companion-art",
        }
    }
}

/// `GROKHUB_DECK_MOOD` (`hello`, `helping`, `hanging-out`) pins the pose for
/// the deck screenshots (`scripts/deck-shots.ps1`), where no turn can run.
pub(super) fn parse_mood(raw: &str) -> Option<Mood> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "hello" => Some(Mood::Hello),
        "helping" => Some(Mood::Helping),
        "hanging-out" => Some(Mood::HangingOut),
        _ => None,
    }
}

fn pinned_mood() -> Option<Mood> {
    static PIN: OnceLock<Option<Mood>> = OnceLock::new();
    *PIN.get_or_init(|| {
        std::env::var("GROKHUB_DECK_MOOD")
            .ok()
            .as_deref()
            .and_then(parse_mood)
    })
}

/// A tool running in this chat's turn wins; an empty chat says hello;
/// anything else is hanging out.
pub(super) fn mood_for(tool_running: bool, chat_empty: bool) -> Mood {
    if tool_running {
        Mood::Helping
    } else if chat_empty {
        Mood::Hello
    } else {
        Mood::HangingOut
    }
}

/// Art height for the companion card's inner width.
pub(super) fn art_height(inner_w: f32) -> f32 {
    (inner_w * 0.78).clamp(ART_H, ART_MAX_H)
}

/// Bar heights at `t` seconds. The bars move only while voice is on and
/// motion isn't reduced; otherwise they rest at [`WAVE`].
pub(super) fn wave_heights(t: f64, live: bool, still: bool) -> [f32; 9] {
    if !live || still {
        return WAVE;
    }
    let mut out = WAVE;
    for (i, h) in out.iter_mut().enumerate() {
        let swing = (t * 7.0 + i as f64 * 0.8).sin().abs() as f32;
        *h = (*h * (0.35 + 0.65 * swing)).max(0.15);
    }
    out
}

/// Level, total XP and progress into the level, from finished tasks and the
/// open mission's checked steps.
pub(super) fn level_xp(finished: usize, checked: usize) -> (u32, u32, f32) {
    let xp = finished as u32 * XP_PER_TASK + checked as u32 * XP_PER_STEP;
    let level = xp / XP_PER_LEVEL + 1;
    let frac = (xp % XP_PER_LEVEL) as f32 / XP_PER_LEVEL as f32;
    (level, xp, frac)
}

/// `2340` as `2,340`.
pub(super) fn grouped(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A 0-100 health score from what the probes read: heat, free memory and
/// free disk. `None` until anything has been read.
pub(super) fn pc_score(p: &LocalProfile) -> Option<u8> {
    if p.ram_mb == 0 && p.gpus.is_empty() && p.thermal == Thermal::Unknown {
        return None;
    }
    let mut score: i32 = 100;
    score -= match p.thermal {
        Thermal::Unknown | Thermal::Nominal => 0,
        Thermal::Fair => 10,
        Thermal::Serious => 30,
        Thermal::Critical => 50,
    };
    if let Some(free) = p.free_ram_mb.filter(|_| p.ram_mb > 0) {
        if free * 100 < p.ram_mb * 10 {
            score -= 15;
        } else if free * 100 < p.ram_mb * 25 {
            score -= 8;
        }
    }
    if p.free_disk_mb > 0 {
        if p.free_disk_mb < 10 * 1024 {
            score -= 25;
        } else if p.free_disk_mb < 50 * 1024 {
            score -= 10;
        }
    }
    Some(score.clamp(0, 100) as u8)
}

/// Dock width for the page width beside the rail, or `None` when it hides.
pub(super) fn dock_width(page_w: f32) -> Option<f32> {
    if page_w < DOCK_MIN_PAGE_W {
        None
    } else if page_w < 980.0 {
        Some(240.0)
    } else if page_w < 1300.0 {
        Some(280.0)
    } else if page_w < 1700.0 {
        Some(300.0)
    } else {
        Some(340.0)
    }
}

/// Verified and counted checklist items. Cancelled ones don't count.
pub(super) fn objective_progress(items: &[MissionCheckItem]) -> (usize, usize) {
    let counted = items
        .iter()
        .filter(|i| i.status != MissionCheckStatus::Cancelled);
    let total = counted.clone().count();
    let done = counted
        .filter(|i| i.status == MissionCheckStatus::Verified)
        .count();
    (done, total)
}

/// One PC loadout tile: a caption and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PcTile {
    pub label: &'static str,
    pub value: String,
}

fn gb(mb: u64) -> String {
    format!("{} GB", (mb + 512) / 1024)
}

/// Up to four tiles from what the probes actually read; blanks are skipped.
pub(super) fn pc_tiles(p: &LocalProfile) -> Vec<PcTile> {
    let mut tiles = pc_tile_list(p);
    tiles.truncate(4);
    tiles
}

/// Every tile the probes could fill, in the dock's order.
pub(super) fn pc_tile_list(p: &LocalProfile) -> Vec<PcTile> {
    let gpu = p
        .gpus
        .iter()
        .find(|g| !g.integrated)
        .or_else(|| p.gpus.first());
    let mut tiles = Vec::new();
    if let Some(g) = gpu.filter(|g| !g.name.trim().is_empty()) {
        let value = if g.vram_mb > 0 && !g.unified {
            format!("{} · {}", g.name.trim(), gb(g.vram_mb))
        } else {
            g.name.trim().to_string()
        };
        tiles.push(PcTile {
            label: "GPU",
            value,
        });
    }
    if let Some(d) = gpu.map(|g| g.driver.trim()).filter(|d| !d.is_empty()) {
        tiles.push(PcTile {
            label: "Driver",
            value: d.to_string(),
        });
    }
    let thermal = match p.thermal {
        Thermal::Unknown => None,
        Thermal::Nominal => Some("Cool"),
        Thermal::Fair => Some("Warm"),
        Thermal::Serious => Some("Hot"),
        Thermal::Critical => Some("Critical"),
    };
    if let Some(t) = thermal {
        tiles.push(PcTile {
            label: "Thermal",
            value: t.into(),
        });
    }
    if p.ram_mb > 0 {
        tiles.push(PcTile {
            label: "RAM",
            value: gb(p.ram_mb),
        });
    }
    if p.free_disk_mb > 0 {
        tiles.push(PcTile {
            label: "Disk free",
            value: gb(p.free_disk_mb),
        });
    }
    tiles
}

/// From Pulse: the newest home cards as (title, kind · age), plus the unread count.
pub(super) fn pulse_lines(
    cards: &[UpdateCard],
    pulse: &grokhub_core::FeedPulse,
    now: u64,
) -> (Vec<(String, String)>, usize) {
    let home = grokhub_core::home_event_cards(cards, pulse, now);
    let unread = home
        .iter()
        .filter(|c| c.status == UpdateStatus::Unread)
        .count();
    let lines = home
        .iter()
        .take(PULSE_LINES)
        .map(|c| {
            let age = grokhub_core::pulse::ago_label(c.created_at, now);
            (c.title.clone(), format!("{} · {age}", c.kind.label()))
        })
        .collect();
    (lines, unread)
}

#[derive(Default)]
struct ProfileSlot {
    profile: Option<LocalProfile>,
    read_at: Option<Instant>,
    reading: bool,
}

fn profile_slot() -> &'static Mutex<ProfileSlot> {
    static SLOT: OnceLock<Mutex<ProfileSlot>> = OnceLock::new();
    SLOT.get_or_init(Default::default)
}

/// The last hardware read; starts a background re-read when it's stale.
pub(super) fn pc_profile() -> Option<LocalProfile> {
    let mut slot = profile_slot().lock().unwrap_or_else(|e| e.into_inner());
    let stale = slot.read_at.is_none_or(|t| t.elapsed() >= PROFILE_REFRESH);
    if stale && !slot.reading {
        slot.reading = true;
        let _ = std::thread::Builder::new()
            .name("loadout-pc".into())
            .spawn(|| {
                let home = grokhub_core::paths::user_home()
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                let p = super::local_probe::read_profile(&home);
                let mut slot = profile_slot().lock().unwrap_or_else(|e| e.into_inner());
                slot.profile = Some(p);
                slot.read_at = Some(Instant::now());
                slot.reading = false;
            });
    }
    slot.profile.clone()
}

pub(super) fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    use crate::ui_kit::recipe::{look, Part, State};
    let l = look(Part::Card, State::Default);
    egui::Frame::NONE
        .fill(l.fill)
        .stroke(l.stroke)
        .corner_radius(l.radius)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        })
        .response
}

pub(super) fn caption(text: &str) -> egui::RichText {
    egui::RichText::new(text.to_uppercase())
        .size(crate::theme::TYPE_CAPTION)
        .extra_letter_spacing(1.5)
        .color(crate::theme::muted())
}

/// Amber diamond before a dock card title, as on the deck.
pub(super) fn diamond(ui: &mut egui::Ui, filled: bool) {
    let (mark, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    let c = mark.center();
    let points = vec![
        egui::pos2(c.x, c.y - 5.0),
        egui::pos2(c.x + 5.0, c.y),
        egui::pos2(c.x, c.y + 5.0),
        egui::pos2(c.x - 5.0, c.y),
    ];
    let (fill, stroke) = if filled {
        (crate::theme::amber(), egui::Stroke::NONE)
    } else {
        (
            egui::Color32::TRANSPARENT,
            egui::Stroke::new(1.0, crate::theme::amber()),
        )
    };
    ui.painter()
        .add(egui::Shape::convex_polygon(points, fill, stroke));
}

/// Card title on the left, after an amber diamond when `mark` is set, and a
/// count or score on the right in `right_color`.
pub(super) fn header(
    ui: &mut egui::Ui,
    mark: bool,
    title: &str,
    right: &str,
    right_color: egui::Color32,
) {
    ui.horizontal(|ui| {
        if mark {
            ui.spacing_mut().item_spacing.x = 6.0;
            diamond(ui, true);
        }
        ui.label(caption(title));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(right.to_uppercase())
                    .size(crate::theme::TYPE_CAPTION)
                    .extra_letter_spacing(1.5)
                    .strong()
                    .color(right_color),
            );
        });
    });
    ui.add_space(crate::theme::SPACING_8);
}

/// Amber corner brackets inside `rect`, the docked frame on slide 02.
fn paint_brackets(painter: &egui::Painter, rect: egui::Rect) {
    let r = rect.shrink(6.0);
    let arm = 18.0;
    let stroke = egui::Stroke::new(1.5, crate::theme::amber());
    for (corner, dx, dy) in [
        (r.left_top(), 1.0, 1.0),
        (r.right_top(), -1.0, 1.0),
        (r.left_bottom(), 1.0, -1.0),
        (r.right_bottom(), -1.0, -1.0),
    ] {
        painter.line_segment([corner, corner + egui::vec2(arm * dx, 0.0)], stroke);
        painter.line_segment([corner, corner + egui::vec2(0.0, arm * dy)], stroke);
    }
}

fn companion_texture(ctx: &egui::Context, mood: Mood) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(mood.key());
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(tex);
    }
    let rgba = image::load_from_memory(mood.png()).ok()?.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let tex = ctx.load_texture(
        mood.key(),
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// Circular face crop of the companion for Hangout bubbles and the chat header.
pub(super) fn companion_face(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("loadout-companion-face");
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(tex);
    }
    let rgba = image::load_from_memory(COMPANION_PNG).ok()?.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    // Head sits in the upper third of the full-body art.
    let side = (w.min(h) * 45 / 100).max(64);
    let x0 = w.saturating_sub(side) / 2;
    let y0 = (h * 6 / 100).min(h.saturating_sub(side));
    let out = side.max(1);
    let mut pixels = vec![0_u8; out * out * 4];
    let r = (out as f32 - 1.0) * 0.5;
    let cx = r;
    let cy = r;
    for y in 0..out {
        for x in 0..out {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let i = (y * out + x) * 4;
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let sx = x0 + x * side / out;
            let sy = y0 + y * side / out;
            let src = (sy.min(h - 1) * w + sx.min(w - 1)) * 4;
            pixels[i..i + 4].copy_from_slice(&rgba.as_raw()[src..src + 4]);
        }
    }
    let tex = ctx.load_texture(
        "loadout-companion-face",
        egui::ColorImage::from_rgba_unmultiplied([out, out], &pixels),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// Paint the circular companion face into `size`×`size`.
pub(super) fn paint_companion_face(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if let Some(tex) = companion_face(ui.ctx()) {
        ui.painter().image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        let mark = crate::theme::mark(ui.ctx());
        ui.painter().image(
            mark.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            crate::theme::muted(),
        );
    }
}

fn paint_wave(ui: &mut egui::Ui, live: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 14.0), egui::Sense::hover());
    let color = if live {
        crate::theme::amber()
    } else {
        crate::theme::muted()
    };
    let still = crate::motion::reduced_motion(ui);
    let bars = wave_heights(ui.input(|i| i.time), live, still);
    if live && !still {
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }
    let step = rect.width() / bars.len() as f32;
    for (i, h) in bars.iter().enumerate() {
        let x = rect.left() + step * (i as f32 + 0.5);
        let half = rect.height() * h / 2.0;
        ui.painter().line_segment(
            [
                egui::pos2(x, rect.center().y - half),
                egui::pos2(x, rect.center().y + half),
            ],
            egui::Stroke::new(2.0, color),
        );
    }
}

/// Cabin thread id for Windows deck shots (`scripts/deck-shots.ps1`).
pub(crate) const DECK_THREAD_ID: &str = "deck-hangout";

/// Windows deck screenshots (`scripts/deck-shots.ps1`) set `GROKHUB_DECK_SHOT=1`;
/// mood shots may also set `GROKHUB_DECK_MOOD`.
pub(super) fn deck_shot_requested() -> bool {
    deck_shot_env() || pinned_mood().is_some()
}

/// Windows web_search proof shots (`scripts/web-search-shots.ps1`).
pub(super) fn web_search_shot_requested() -> bool {
    web_search_shot_env()
}

/// Hide the signed-out auth banner for any proof-shot launch.
pub(super) fn proof_shot_requested() -> bool {
    deck_shot_requested() || web_search_shot_requested()
}

fn deck_shot_env() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("GROKHUB_DECK_SHOT")
            .ok()
            .is_some_and(|v| v.trim() == "1")
    })
}

fn web_search_shot_env() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("GROKHUB_WEB_SEARCH_SHOT")
            .ok()
            .is_some_and(|v| v.trim() == "1")
    })
}

/// Open Settings → Cabin defaults (Web search fields) for proof shots.
pub(super) fn web_search_settings_shot() -> bool {
    std::env::var("GROKHUB_WEB_SEARCH_SETTINGS")
        .ok()
        .is_some_and(|v| v.trim() == "1")
}

/// How many chat tabs a deck shot should plant (`GROKHUB_DECK_TABS`).
pub(super) fn deck_tab_count() -> Option<usize> {
    std::env::var("GROKHUB_DECK_TABS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| n >= 3)
}

/// When set, the tab strip paints the overflow menu open for HARD DONE shots.
pub(super) fn deck_overflow_open() -> bool {
    std::env::var("GROKHUB_DECK_OVERFLOW")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Home→Chat last-used proof: `home` parks on Home; `chat` restores Hangout.
pub(super) fn deck_last_chat_mode() -> Option<String> {
    std::env::var("GROKHUB_DECK_LAST_CHAT")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| v == "home" || v == "chat")
}

/// Slide 02 Hangout: quick replies above the composer (not Connect / Start here).
pub(super) fn hangout_quick_chips() -> Vec<QuickChip> {
    [
        "Plan my weekend",
        "Roast my build",
        "Find a duo",
        "Fix my stutter",
    ]
    .iter()
    .enumerate()
    .map(|(i, label)| QuickChip {
        id: format!("hangout-deck-{i}"),
        label: label.to_string(),
        value: label.to_string(),
        kind: ChipKind::Chat,
        score: 1.0,
        hint: String::new(),
        primary: false,
    })
    .collect()
}

/// Loadout Hangout with a transcript: slide 02 composer stack (chips above the pill, no mode row).
pub(super) fn hangout_composer_layout(messages_empty: bool) -> bool {
    crate::theme::is_loadout_paint() && !messages_empty
}

fn deck_transcript() -> Vec<(String, String)> {
    vec![
        (
            "user".into(),
            "yo, what are we running tonight?".into(),
        ),
        (
            "assistant".into(),
            "Diamond climb. You're 3 wins off — want a warm-up in the range, or I queue the duo finder?"
                .into(),
        ),
        (
            "user".into(),
            "queue the duo and fix my stutter first".into(),
        ),
        (
            "assistant".into(),
            "On it. I'll clear the shader cache, then find you a Diamond duo for tonight.".into(),
        ),
    ]
}

fn ensure_deck_session_file() {
    let path = crate::config::config_dir()
        .join("sessions")
        .join(format!("{DECK_THREAD_ID}.jsonl"));
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let now = now_ms();
    let steps = [
        "Diamond, 3 wins left",
        "Aim routine 12/20",
        "Warm up in the range",
        "Review last match",
        "Pick a duo for tonight",
    ];
    let todos: Vec<serde_json::Value> = steps
        .iter()
        .enumerate()
        .map(|(i, content)| {
            serde_json::json!({
                "id": (i + 1).to_string(),
                "content": content,
                "status": if i == 0 { "in_progress" } else { "pending" }
            })
        })
        .collect();
    let header = serde_json::json!({
        "id": DECK_THREAD_ID,
        "title": "Hangout",
        "created": now,
        "cwd": ".",
        "model": "grok",
        "engine": "native",
        "renamed": false
    });
    let call = serde_json::json!({
        "type": "function_call",
        "call_id": "c1",
        "name": "todo_write",
        "arguments": serde_json::json!({ "todos": todos }).to_string()
    });
    let body = format!(
        "{}\n{}\n",
        serde_json::to_string(&header).unwrap_or_default(),
        serde_json::to_string(&call).unwrap_or_default()
    );
    let _ = crate::config::atomic_write(&path, body.as_bytes());
}

impl Cabin {
    /// After `open_fresh_home`, put the Hangout demo transcript back for deck shots only.
    pub(super) fn apply_deck_shot_seed(&mut self) {
        if !deck_shot_requested() {
            return;
        }
        ensure_deck_session_file();
        let transcript = Arc::new(deck_transcript());
        let now = now_ms();
        if let Some(i) = self.threads.iter().position(|t| t.id == DECK_THREAD_ID) {
            self.thread_idx = i;
        } else {
            let mut t = ChatThread::new("Hangout", false);
            t.id = DECK_THREAD_ID.to_string();
            t.native = true;
            t.grok_session = Some(DECK_THREAD_ID.to_string());
            self.threads.push(t);
            self.thread_idx = self.threads.len() - 1;
        }
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.title = "Hangout".into();
            t.native = true;
            t.grok_session = Some(DECK_THREAD_ID.to_string());
            t.grok_show_pending = false;
            t.messages = transcript.clone();
            t.accessed_ms = now;
            t.last_active_ms = now;
        }
        // Extra titled chats so the titlebar tab strip has several tabs in HARD DONE shots.
        self.ensure_deck_extra_tab(
            "deck-weekend",
            "Weekend plan",
            true,
            now.saturating_sub(60_000),
        );
        self.ensure_deck_extra_tab(
            "deck-driver",
            "Driver update",
            false,
            now.saturating_sub(30_000),
        );
        // GROKHUB_DECK_TABS=N seeds N total chats (Hangout + extras) for overflow proof.
        let want = deck_tab_count().unwrap_or(3).max(3);
        let extras = [
            ("deck-duo", "Duo finder"),
            ("deck-stutter", "Fix stutter"),
            ("deck-fans", "Fan curve"),
            ("deck-shaders", "Shader cache"),
            ("deck-rank", "Rank climb"),
            ("deck-patch", "Season patch"),
            ("deck-aim", "Aim routine"),
            ("deck-warmup", "Warm up range"),
            ("deck-build", "Roast build"),
            ("deck-backlog", "Backlog sort"),
            ("deck-gpu", "GPU drivers"),
            ("deck-queue", "Queue night"),
            ("deck-voip", "Voip check"),
            ("deck-map", "Map pool"),
            ("deck-scrim", "Scrim notes"),
            ("deck-perk", "Perk loadout"),
        ];
        for (n, (id, title)) in extras.iter().enumerate() {
            // Hangout + Weekend + Driver already planted → total so far is 3.
            if 3 + n >= want {
                break;
            }
            self.ensure_deck_extra_tab(
                id,
                title,
                false,
                now.saturating_sub(90_000 + n as u64 * 1_000),
            );
        }
        self.messages = transcript;
        self.cfg.current_thread = DECK_THREAD_ID.to_string();
        self.cfg.last_chat_tab = DECK_THREAD_ID.to_string();
        self.visible_chips = hangout_quick_chips();
        // Viper is an adults-only preset; without this gate the titlebar falls
        // back to Nyx and HARD DONE shots look wrong.
        self.cfg.explicit_personalities = true;
        self.cfg.personality = "viper".into();
        self.persist_cfg();
        self.pin_chat_tail();
        self.seed_activity_dock_for_deck();
        // Windows proof: Home parks empty; Chat rail restores Hangout.
        match deck_last_chat_mode().as_deref() {
            Some("home") => self.open_fresh_home(),
            Some("chat") => {
                self.open_fresh_home();
                self.open_last_chat_tab();
                self.nav = Nav::Chat;
            }
            _ => {}
        }
        // Settings pin must open the modal itself — Ctrl+, / gear clicks miss
        // on some windows-release sizes (1600×900 Settings shots).
        if let Some(sec) = super::settings::pinned_settings_sec() {
            if self.nav != Nav::Settings {
                self.settings_back = self.nav;
            }
            self.settings_sec = sec;
            self.nav = Nav::Settings;
        }
    }

    /// After `open_fresh_home`, restore a citation transcript for web_search
    /// Windows proof shots. Seed file: `{config}/web-search-shot-seed.json`
    /// written by `scripts/web-search-shots.ps1` (title + messages). Launch
    /// clears a loaded transcript, so this must run after home open — same
    /// pattern as [`Self::apply_deck_shot_seed`].
    ///
    /// `GROKHUB_WEB_SEARCH_SETTINGS=1` opens Settings → Cabin defaults so the
    /// optional SearXNG / Brave fields are visible (no fragile click).
    pub(super) fn apply_web_search_shot_seed(&mut self) {
        if !web_search_shot_requested() {
            return;
        }
        if web_search_settings_shot() {
            if self.nav != Nav::Settings {
                self.settings_back = self.nav;
            }
            self.nav = Nav::Settings;
            self.settings_sec = settings::pinned_settings_sec().unwrap_or(SettingsSec::Defaults);
            return;
        }
        let path = crate::config::config_dir().join("web-search-shot-seed.json");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return;
        };
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("ws-proof")
            .to_string();
        let title = v
            .get("title")
            .and_then(|x| x.as_str())
            .unwrap_or("web search")
            .to_string();
        let mut pairs: Vec<(String, String)> = Vec::new();
        if let Some(arr) = v.get("messages").and_then(|m| m.as_array()) {
            for row in arr {
                let Some(pair) = row.as_array() else {
                    continue;
                };
                if pair.len() < 2 {
                    continue;
                }
                let role = pair[0].as_str().unwrap_or("").to_string();
                let text = pair[1].as_str().unwrap_or("").to_string();
                if role.is_empty() || text.is_empty() {
                    continue;
                }
                pairs.push((role, text));
            }
        }
        if pairs.is_empty() {
            return;
        }
        let transcript = Arc::new(pairs);
        let now = now_ms();
        if let Some(i) = self.threads.iter().position(|t| t.id == id) {
            self.thread_idx = i;
        } else {
            let mut t = ChatThread::new(&title, false);
            t.id = id.clone();
            t.native = true;
            t.grok_session = Some(id.clone());
            self.threads.push(t);
            self.thread_idx = self.threads.len() - 1;
        }
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.title = title;
            t.native = true;
            t.grok_session = Some(id.clone());
            t.grok_show_pending = false;
            t.messages = transcript.clone();
            t.accessed_ms = now;
            t.last_active_ms = now;
        }
        self.messages = transcript;
        self.cfg.current_thread = id;
        self.pin_chat_tail();
    }

    fn ensure_deck_extra_tab(&mut self, id: &str, title: &str, pinned: bool, at: u64) {
        if self.threads.iter().any(|t| t.id == id) {
            return;
        }
        let mut t = ChatThread::new(title, false);
        t.id = id.to_string();
        t.native = true;
        t.pinned = pinned;
        if pinned {
            t.pinned_ms = at;
        }
        t.accessed_ms = at;
        t.last_active_ms = at;
        t.messages = Arc::new(vec![("user".into(), format!("Seed for {title}"))]);
        self.threads.push(t);
    }

    /// Right-hand dock on the chat home. Call before `ui_chat` so the chat
    /// and its composer take the space that's left.
    pub(super) fn ui_loadout_dock(&mut self, ui: &mut egui::Ui) {
        if self.cfg.chat_objectives {
            self.ui_chat_sidebars(ui);
            return;
        }
        let Some(width) = dock_width(ui.available_width()) else {
            return;
        };
        egui::Panel::right("loadout-dock")
            .exact_size(width)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = crate::theme::SPACING_12;
                        self.dock_companion(ui);
                        self.dock_objectives(ui);
                        dock_pc(ui);
                        self.dock_pulse(ui);
                    });
            });
    }

    fn dock_companion(&mut self, ui: &mut egui::Ui) {
        let name = grokhub_core::brand::companion_name();
        let live = self.voice_is_on();
        let mood = self.companion_mood();
        let resp = card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                diamond(ui, true);
                ui.label(caption("Companion · docked"));
            });
            ui.add_space(crate::theme::SPACING_4);
            let w = ui.available_width();
            let (art, _) =
                ui.allocate_exact_size(egui::vec2(w, art_height(w)), egui::Sense::hover());
            ui.painter()
                .rect_filled(art, crate::theme::RADIUS_SM, crate::theme::void_bg());
            // A soft red glow behind the pedestal, as on the deck.
            let glow = art.center() + egui::vec2(0.0, art.height() * 0.12);
            for (r, a) in [
                (0.5, 0.06),
                (0.42, 0.08),
                (0.34, 0.1),
                (0.26, 0.12),
                (0.18, 0.14),
            ] {
                ui.painter().circle_filled(
                    glow,
                    art.height() * r,
                    crate::theme::red_dim().gamma_multiply(a),
                );
            }
            if let Some(tex) = companion_texture(ui.ctx(), mood) {
                let [tw, th] = tex.size().map(|v| v as f32);
                let scale = (art.height() / th).min(art.width() / tw);
                let fit = egui::Rect::from_center_size(art.center(), egui::vec2(tw, th) * scale);
                ui.painter().image(
                    tex.id(),
                    fit,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 4.0, crate::theme::amber());
                ui.label(
                    egui::RichText::new("Here · tap to talk")
                        .size(crate::theme::TYPE_BODY)
                        .strong()
                        .color(crate::theme::fg()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    paint_wave(ui, live);
                    if live {
                        ui.label(caption("Listening"));
                    }
                });
            });
        });
        paint_brackets(ui.painter(), resp.rect);
        let click = ui
            .interact(
                resp.rect,
                ui.id().with("loadout-companion"),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("Talk to {name}"));
        if click.clicked() {
            self.listen_voice();
        }
    }

    /// The dock's pose for what this chat is doing right now.
    pub(super) fn companion_mood(&self) -> Mood {
        if let Some(mood) = pinned_mood() {
            return mood;
        }
        let tool_running = self.thinking_here()
            && (self.perm_ask.is_some()
                || self
                    .live_blocks
                    .last()
                    .is_some_and(|b| b.kind == LiveKind::Tool));
        mood_for(tool_running, self.messages.is_empty())
    }

    pub(super) fn dock_mission(&self, ctx: &egui::Context) -> Option<MissionView> {
        type Cached = (Instant, String, Option<MissionView>);
        let id = egui::Id::new("loadout-mission");
        let trace = self.trace_id();
        if let Some((at, t, view)) = ctx.data(|d| d.get_temp::<Cached>(id)) {
            if t == trace && at.elapsed() < MISSION_REFRESH {
                return view;
            }
        }
        let view = self.mission_view_here();
        ctx.data_mut(|d| d.insert_temp::<Cached>(id, (Instant::now(), trace, view.clone())));
        view
    }

    /// Finished tasks in the outcome log, re-read every [`XP_REFRESH`].
    pub(super) fn dock_finished_tasks(&self, ctx: &egui::Context) -> usize {
        type Cached = (Instant, usize);
        let id = egui::Id::new("loadout-finished-tasks");
        if let Some((at, n)) = ctx.data(|d| d.get_temp::<Cached>(id)) {
            if at.elapsed() < XP_REFRESH {
                return n;
            }
        }
        let now = now_ms();
        let n = grokhub_core::outcome::read_outcomes(&crate::config::config_dir())
            .iter()
            .filter(|o| o.effective(now) == grokhub_core::outcome::OutcomeResult::Success)
            .count();
        ctx.data_mut(|d| d.insert_temp::<Cached>(id, (Instant::now(), n)));
        n
    }

    fn dock_objectives(&self, ui: &mut egui::Ui) {
        let mission = self.dock_mission(ui.ctx());
        let finished = self.dock_finished_tasks(ui.ctx());
        card(ui, |ui| {
            let items = mission.as_ref().map_or(&[][..], |m| &m.checklist[..]);
            let (done, total) = objective_progress(items);
            let right = if total > 0 {
                format!("{done} / {total}")
            } else {
                String::new()
            };
            header(ui, true, "Objectives", &right, crate::theme::amber());
            match mission.as_ref().filter(|_| total > 0) {
                None => {
                    ui.label(
                        egui::RichText::new(format!(
                            "Nothing on the list yet. Ask {} to plan something.",
                            grokhub_core::brand::companion_name()
                        ))
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::muted()),
                    );
                }
                Some(m) => {
                    if !m.title.trim().is_empty() {
                        ui.label(
                            egui::RichText::new(m.title.trim())
                                .size(crate::theme::TYPE_BODY)
                                .strong()
                                .color(crate::theme::fg()),
                        );
                    }
                    for item in m
                        .checklist
                        .iter()
                        .filter(|i| i.status != MissionCheckStatus::Cancelled)
                        .take(4)
                    {
                        let verified = item.status == MissionCheckStatus::Verified;
                        ui.horizontal(|ui| {
                            diamond(ui, verified);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&item.text)
                                        .size(crate::theme::TYPE_BODY)
                                        .color(if verified {
                                            crate::theme::muted()
                                        } else {
                                            crate::theme::fg()
                                        }),
                                )
                                .truncate(),
                            );
                        });
                    }
                }
            }
            let (level, xp, frac) = level_xp(finished, done);
            ui.add_space(crate::theme::SPACING_4);
            ui.horizontal(|ui| {
                ui.label(caption(&format!("Lvl {level}")));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} XP", grouped(xp)))
                            .size(crate::theme::TYPE_CAPTION)
                            .extra_letter_spacing(1.0)
                            .strong()
                            .color(crate::theme::amber()),
                    );
                });
            });
            let (bar, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 3.0, crate::theme::selected());
            let mut fill = bar;
            fill.set_width(bar.width() * frac);
            ui.painter().rect_filled(fill, 3.0, crate::theme::amber());
        });
    }

    fn dock_pulse(&mut self, ui: &mut egui::Ui) {
        let (lines, unread) = pulse_lines(&self.updates, &self.cfg.feed_pulse, now_ms());
        let resp = card(ui, |ui| {
            let right = if unread > 0 {
                format!("{unread} new")
            } else {
                String::new()
            };
            header(ui, false, "From Pulse", &right, crate::theme::amber());
            if lines.is_empty() {
                ui.label(
                    egui::RichText::new("All quiet. New finds land here.")
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::muted()),
                );
            }
            for (title, sub) in &lines {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(title)
                                .size(crate::theme::TYPE_BODY)
                                .strong()
                                .color(crate::theme::fg()),
                        )
                        .truncate(),
                    );
                    ui.label(
                        egui::RichText::new(sub)
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::muted()),
                    );
                });
            }
        });
        let open = ui
            .interact(
                resp.rect,
                ui.id().with("loadout-pulse"),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Open Pulse");
        if open.clicked() {
            self.set_nav_id("pulse");
        }
    }
}

fn dock_pc(ui: &mut egui::Ui) {
    let profile = pc_profile();
    card(ui, |ui| {
        let score = profile
            .as_ref()
            .and_then(pc_score)
            .map(|n| format!("{n}%"))
            .unwrap_or_default();
        header(ui, false, "PC loadout", &score, crate::theme::fg());
        let Some(p) = profile else {
            ui.label(
                egui::RichText::new("Reading this PC…")
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::muted()),
            );
            return;
        };
        pc_tile_grid(ui, &pc_tiles(&p));
    });
}

/// PC tiles two to a row; the GPU tile wears the amber rim.
pub(super) fn pc_tile_grid(ui: &mut egui::Ui, tiles: &[PcTile]) {
    use crate::ui_kit::recipe;
    let gap = crate::theme::SPACING_8;
    let w = (ui.available_width() - gap) / 2.0;
    for row in tiles.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (i, t) in row.iter().enumerate() {
                // The GPU tile is the selected one: amber rim and value.
                let picked = i == 0 && t.label == "GPU";
                let (stroke, ink) = if picked {
                    (
                        recipe::look(recipe::Part::Card, recipe::State::Accent).stroke,
                        crate::theme::amber(),
                    )
                } else {
                    (
                        egui::Stroke::new(1.0, crate::theme::border()),
                        crate::theme::fg(),
                    )
                };
                egui::Frame::NONE
                    .fill(crate::theme::void_bg())
                    .stroke(stroke)
                    .corner_radius(recipe::BUTTON_RADIUS)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        // Inner margin 8 and the rim on each side.
                        ui.set_width(w - 16.0 - stroke.width * 2.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(caption(t.label));
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&t.value)
                                        .size(crate::theme::TYPE_LABEL)
                                        .strong()
                                        .color(ink),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&t.value);
                        });
                    });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::local_profile::{CpuInfo, Gpu, GpuVendor, PowerInfo};

    fn item(id: &str, status: MissionCheckStatus) -> MissionCheckItem {
        MissionCheckItem {
            id: id.into(),
            text: format!("step {id}"),
            status,
        }
    }

    fn profile(gpus: Vec<Gpu>) -> LocalProfile {
        LocalProfile {
            os: "linux",
            arch: "x86_64",
            gpus,
            cpu: CpuInfo::default(),
            ram_mb: 32 * 1024,
            free_ram_mb: None,
            power: PowerInfo::default(),
            thermal: Thermal::Fair,
            free_disk_mb: 420 * 1024,
        }
    }

    fn gpu(name: &str, vram_mb: u64, integrated: bool, driver: &str) -> Gpu {
        Gpu {
            vendor: GpuVendor::Other,
            name: name.into(),
            vram_mb,
            free_vram_mb: None,
            integrated,
            unified: false,
            driver: driver.into(),
        }
    }

    #[test]
    fn dock_hides_on_narrow_pages_and_grows_on_wide_ones() {
        assert_eq!(dock_width(700.0), None);
        assert_eq!(dock_width(779.0), None);
        assert_eq!(dock_width(780.0), Some(240.0));
        // The default 1100 x 720 window leaves about 840 px beside the rail.
        assert_eq!(dock_width(840.0), Some(240.0));
        assert_eq!(dock_width(979.0), Some(240.0));
        assert_eq!(dock_width(980.0), Some(280.0));
        // A 1366 px laptop leaves about 1106 px beside the rail.
        assert_eq!(dock_width(1106.0), Some(280.0));
        assert_eq!(dock_width(1660.0), Some(300.0));
        assert_eq!(dock_width(2300.0), Some(340.0));
    }

    #[test]
    fn objectives_count_verified_steps_and_skip_cancelled_ones() {
        let items = vec![
            item("a", MissionCheckStatus::Verified),
            item("b", MissionCheckStatus::Attempted),
            item("c", MissionCheckStatus::Cancelled),
            item("d", MissionCheckStatus::Verified),
            item("e", MissionCheckStatus::Planned),
        ];
        assert_eq!(objective_progress(&items), (2, 4));
        assert_eq!(objective_progress(&[]), (0, 0));
    }

    #[test]
    fn pc_tiles_show_the_discrete_gpu_and_skip_what_was_not_read() {
        let p = profile(vec![
            gpu("Intel UHD 770", 0, true, ""),
            gpu("NVIDIA GeForce RTX 4070", 12 * 1024, false, "560.35.03"),
        ]);
        let tiles: Vec<(&str, String)> = pc_tiles(&p)
            .into_iter()
            .map(|t| (t.label, t.value))
            .collect();
        assert_eq!(
            tiles,
            vec![
                ("GPU", "NVIDIA GeForce RTX 4070 · 12 GB".to_string()),
                ("Driver", "560.35.03".to_string()),
                ("Thermal", "Warm".to_string()),
                ("RAM", "32 GB".to_string()),
            ]
        );

        let mut bare = profile(vec![]);
        bare.thermal = Thermal::Unknown;
        bare.ram_mb = 0;
        let tiles: Vec<(&str, String)> = pc_tiles(&bare)
            .into_iter()
            .map(|t| (t.label, t.value))
            .collect();
        assert_eq!(tiles, vec![("Disk free", "420 GB".to_string())]);
    }

    #[test]
    fn pulse_lines_take_the_newest_two_and_count_unread() {
        let now = 10 * 3_600_000;
        let card = |id: &str, at: u64, status| UpdateCard {
            status,
            ..grokhub_core::automation_done_card(id, &format!("Card {id}"), "", at)
        };
        let cards = vec![
            card("old", now - 3 * 3_600_000, UpdateStatus::Unread),
            card("mid", now - 2 * 3_600_000, UpdateStatus::Opened),
            card("new", now - 120_000, UpdateStatus::Unread),
            card("gone", now - 60_000, UpdateStatus::Dismissed),
        ];
        let (lines, unread) = pulse_lines(&cards, &grokhub_core::FeedPulse::default(), now);
        assert_eq!(
            lines,
            vec![
                ("Card new".to_string(), "Automation · 2m ago".to_string()),
                ("Card mid".to_string(), "Automation · 2h ago".to_string()),
            ]
        );
        assert_eq!(unread, 2);
    }

    #[test]
    fn chat_home_paints_the_dock_before_the_chat() {
        let src = include_str!("mod.rs");
        let chat = src
            .split("Nav::Chat => {")
            .nth(1)
            .and_then(|s| s.split("Nav::Devices").next())
            .expect("Nav::Chat arm");
        let dock = chat
            .find("ui_loadout_dock(ui)")
            .expect("dock on the chat home");
        let page = chat.find("ui_chat(ui)").expect("chat page");
        assert!(
            dock < page,
            "the dock must take its side before the chat: {chat}"
        );
    }

    #[test]
    fn companion_art_decodes() {
        let img = image::load_from_memory(COMPANION_PNG).expect("brand/companion.png decodes");
        assert!(img.width() > 100 && img.height() > 100);
    }

    #[test]
    fn companion_face_builds_a_circular_texture() {
        let ctx = egui::Context::default();
        let tex = companion_face(&ctx).expect("companion face");
        let [w, h] = tex.size();
        assert_eq!(w, h);
        assert!(w >= 64);
        let rgba = image::load_from_memory(COMPANION_PNG)
            .expect("decode")
            .to_rgba8();
        assert!(rgba.width() > 100);
    }

    #[test]
    fn every_mood_has_its_own_art_that_decodes() {
        let mut sizes = Vec::new();
        for mood in [Mood::Hello, Mood::Helping, Mood::HangingOut] {
            let img = image::load_from_memory(mood.png()).expect("mood art decodes");
            assert_eq!(img.height(), 440, "{mood:?} art is 440 px tall");
            sizes.push(img.width());
        }
        assert_eq!(sizes, vec![236, 393, 351]);
    }

    #[test]
    fn the_screenshot_pin_names_each_mood_and_nothing_else() {
        assert_eq!(parse_mood("hello"), Some(Mood::Hello));
        assert_eq!(parse_mood(" Helping "), Some(Mood::Helping));
        assert_eq!(parse_mood("hanging-out"), Some(Mood::HangingOut));
        assert_eq!(parse_mood(""), None);
        assert_eq!(parse_mood("angry"), None);
    }

    #[test]
    fn mood_follows_the_chat_state() {
        assert_eq!(mood_for(false, true), Mood::Hello);
        assert_eq!(mood_for(false, false), Mood::HangingOut);
        assert_eq!(mood_for(true, false), Mood::Helping);
        assert_eq!(mood_for(true, true), Mood::Helping);
    }

    #[test]
    fn the_dock_says_hello_then_helps_then_hangs_out() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
        cabin.thread_idx = 0;
        cabin.messages = Arc::new(Vec::new());
        assert_eq!(cabin.companion_mood(), Mood::Hello);

        cabin.messages = Arc::new(vec![("user".into(), "tidy my fans".into())]);
        cabin.running = true;
        cabin.chat_job_thread = Some(cabin.threads[0].id.clone());
        assert_eq!(cabin.companion_mood(), Mood::HangingOut);

        cabin.live_blocks.push(grokhub_core::LiveBlock {
            kind: LiveKind::Tool,
            body: String::new(),
            tool_id: "t1".into(),
            tool_title: "Read fan curve".into(),
            tool_status: "in_progress".into(),
            tool_detail: String::new(),
            fold_slot: 0,
        });
        assert_eq!(cabin.companion_mood(), Mood::Helping);

        cabin.running = false;
        cabin.chat_job_thread = None;
        assert_eq!(cabin.companion_mood(), Mood::HangingOut);
    }

    #[test]
    fn companion_art_grows_with_the_dock_between_196_and_260() {
        // Compact 240 px dock: 216 px inside the card.
        assert_eq!(art_height(216.0), 196.0);
        // 300 px dock: 276 px inside.
        assert_eq!(art_height(276.0), 215.28);
        assert_eq!(art_height(316.0), 246.48);
        assert_eq!(art_height(400.0), 260.0);
    }

    #[test]
    fn the_wave_moves_only_while_listening_and_motion_is_on() {
        assert_eq!(wave_heights(1.3, false, false), WAVE);
        assert_eq!(wave_heights(1.3, true, true), WAVE);
        let a = wave_heights(0.0, true, false);
        let b = wave_heights(0.2, true, false);
        assert_ne!(a, b);
        // t = 0: bar 0 swings sin(0) = 0, so 0.35 x 0.35 drops to the 0.15 floor.
        assert_eq!(a[0], 0.15);
        assert!(a.iter().chain(b.iter()).all(|h| (0.15..=1.0).contains(h)));
    }

    #[test]
    fn level_and_xp_come_from_finished_tasks_and_checked_steps() {
        assert_eq!(level_xp(0, 0), (1, 0, 0.0));
        // 23 finished tasks and 2 checked steps: 2,350 XP, level 5, 70% in.
        assert_eq!(level_xp(23, 2), (5, 2350, 0.7));
        assert_eq!(level_xp(5, 0), (2, 500, 0.0));
        assert_eq!(grouped(2350), "2,350");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000_000), "1,000,000");
    }

    #[test]
    fn deck_shot_seed_restores_hangout_after_fresh_home() {
        let _g = crate::config::hold_test_config();
        let _paint = crate::theme::hold_paint_test();
        crate::theme::set_paint_palette(crate::theme::PaintPalette::OpsDark);
        let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        let prev_mood = std::env::var("GROKHUB_DECK_MOOD").ok();
        let prev_last = std::env::var("GROKHUB_DECK_LAST_CHAT").ok();
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        std::env::remove_var("GROKHUB_DECK_MOOD");
        std::env::remove_var("GROKHUB_DECK_LAST_CHAT");
        let transcript = deck_transcript();
        let mut seeded = ChatThread::new("Hangout", false);
        seeded.id = DECK_THREAD_ID.to_string();
        seeded.messages = Arc::new(transcript.clone());
        let mut cabin = Cabin::quiet_cabin(vec![seeded], 0);
        cabin.messages = Arc::new(transcript);
        cabin.open_fresh_home();
        assert!(
            cabin.messages.is_empty(),
            "fresh home clears a transcript before the deck seed runs"
        );
        cabin.apply_deck_shot_seed();
        assert_eq!(cabin.messages.len(), 4);
        assert_eq!(cabin.threads[cabin.thread_idx].id, DECK_THREAD_ID);
        assert_eq!(cabin.cfg.last_chat_tab, DECK_THREAD_ID);
        let titles: Vec<&str> = cabin.threads.iter().map(|t| t.title.as_str()).collect();
        assert!(
            titles.contains(&"Hangout")
                && titles.contains(&"Weekend plan")
                && titles.contains(&"Driver update"),
            "deck seed must plant several tab titles for HARD DONE shots: {titles:?}"
        );
        assert!(
            cabin
                .threads
                .iter()
                .any(|t| t.title == "Weekend plan" && t.pinned),
            "Weekend plan stays pinned on the left of the strip"
        );
        assert_eq!(
            cabin
                .composer_chips()
                .iter()
                .map(|c| c.label.as_str())
                .collect::<Vec<_>>(),
            [
                "Plan my weekend",
                "Roast my build",
                "Find a duo",
                "Fix my stutter"
            ]
        );
        assert!(
            cabin.cfg.explicit_personalities,
            "Viper is adults-only; deck seed must turn the gate on"
        );
        assert_eq!(cabin.cfg.personality, "viper");
        assert_eq!(
            cabin.titlebar_personality_label().to_ascii_uppercase(),
            "VIPER"
        );
        match prev_shot {
            Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
            None => std::env::remove_var("GROKHUB_DECK_SHOT"),
        }
        match prev_mood {
            Some(v) => std::env::set_var("GROKHUB_DECK_MOOD", v),
            None => std::env::remove_var("GROKHUB_DECK_MOOD"),
        }
        match prev_last {
            Some(v) => std::env::set_var("GROKHUB_DECK_LAST_CHAT", v),
            None => std::env::remove_var("GROKHUB_DECK_LAST_CHAT"),
        }
    }

    #[test]
    fn deck_last_chat_home_then_chat_restores_hangout() {
        let _g = crate::config::hold_test_config();
        let _paint = crate::theme::hold_paint_test();
        crate::theme::set_paint_palette(crate::theme::PaintPalette::OpsDark);
        let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        let prev_last = std::env::var("GROKHUB_DECK_LAST_CHAT").ok();
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        std::env::set_var("GROKHUB_DECK_LAST_CHAT", "home");
        let mut cabin = Cabin::quiet_for_test();
        cabin.apply_deck_shot_seed();
        assert!(
            cabin.messages.is_empty(),
            "home mode parks on an empty draft"
        );
        assert_eq!(cabin.cfg.last_chat_tab, DECK_THREAD_ID);
        assert_ne!(cabin.threads[cabin.thread_idx].id, DECK_THREAD_ID);
        std::env::set_var("GROKHUB_DECK_LAST_CHAT", "chat");
        cabin.apply_deck_shot_seed();
        assert_eq!(cabin.threads[cabin.thread_idx].id, DECK_THREAD_ID);
        assert_eq!(cabin.messages.len(), 4);
        match prev_shot {
            Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
            None => std::env::remove_var("GROKHUB_DECK_SHOT"),
        }
        match prev_last {
            Some(v) => std::env::set_var("GROKHUB_DECK_LAST_CHAT", v),
            None => std::env::remove_var("GROKHUB_DECK_LAST_CHAT"),
        }
    }

    #[test]
    fn web_search_shot_seed_restores_citation_transcript() {
        let _g = crate::config::hold_test_config();
        let prev = std::env::var("GROKHUB_WEB_SEARCH_SHOT").ok();
        let prev_set = std::env::var("GROKHUB_WEB_SEARCH_SETTINGS").ok();
        std::env::set_var("GROKHUB_WEB_SEARCH_SHOT", "1");
        std::env::remove_var("GROKHUB_WEB_SEARCH_SETTINGS");
        let cite = "https://doc.rust-lang.org/stable/releases.html";
        let seed = serde_json::json!({
            "id": "ws-openrouter",
            "title": "OpenRouter · web search",
            "messages": [
                ["user", "Search the web and cite real links."],
                ["assistant", format!("Found it: {cite}")]
            ]
        });
        let path = crate::config::config_dir().join("web-search-shot-seed.json");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, seed.to_string()).expect("seed");
        let mut cabin = Cabin::quiet_cabin(vec![ChatThread::new("Chat", false)], 0);
        cabin.open_fresh_home();
        cabin.apply_web_search_shot_seed();
        assert_eq!(
            cabin.threads[cabin.thread_idx].title,
            "OpenRouter · web search"
        );
        assert_eq!(cabin.messages.len(), 2);
        assert!(
            cabin.messages[1].1.contains(cite),
            "assistant must cite the live URL: {}",
            cabin.messages[1].1
        );
        assert!(proof_shot_requested());
        match prev {
            Some(v) => std::env::set_var("GROKHUB_WEB_SEARCH_SHOT", v),
            None => std::env::remove_var("GROKHUB_WEB_SEARCH_SHOT"),
        }
        match prev_set {
            Some(v) => std::env::set_var("GROKHUB_WEB_SEARCH_SETTINGS", v),
            None => std::env::remove_var("GROKHUB_WEB_SEARCH_SETTINGS"),
        }
    }

    #[test]
    fn web_search_settings_shot_opens_cabin_defaults() {
        let _g = crate::config::hold_test_config();
        let prev = std::env::var("GROKHUB_WEB_SEARCH_SHOT").ok();
        let prev_set = std::env::var("GROKHUB_WEB_SEARCH_SETTINGS").ok();
        let prev_sec = std::env::var("GROKHUB_DECK_SETTINGS_SEC").ok();
        std::env::set_var("GROKHUB_WEB_SEARCH_SHOT", "1");
        std::env::set_var("GROKHUB_WEB_SEARCH_SETTINGS", "1");
        std::env::set_var("GROKHUB_DECK_SETTINGS_SEC", "defaults");
        let mut cabin = Cabin::quiet_cabin(vec![ChatThread::new("Chat", false)], 0);
        cabin.open_fresh_home();
        cabin.apply_web_search_shot_seed();
        assert!(matches!(cabin.nav, Nav::Settings));
        assert!(matches!(cabin.settings_sec, SettingsSec::Defaults));
        match prev {
            Some(v) => std::env::set_var("GROKHUB_WEB_SEARCH_SHOT", v),
            None => std::env::remove_var("GROKHUB_WEB_SEARCH_SHOT"),
        }
        match prev_set {
            Some(v) => std::env::set_var("GROKHUB_WEB_SEARCH_SETTINGS", v),
            None => std::env::remove_var("GROKHUB_WEB_SEARCH_SETTINGS"),
        }
        match prev_sec {
            Some(v) => std::env::set_var("GROKHUB_DECK_SETTINGS_SEC", v),
            None => std::env::remove_var("GROKHUB_DECK_SETTINGS_SEC"),
        }
    }

    #[test]
    fn pc_score_takes_off_for_heat_low_memory_and_a_full_disk() {
        let mut p = profile(vec![gpu("RTX 4070", 12 * 1024, false, "560")]);
        p.thermal = Thermal::Nominal;
        assert_eq!(pc_score(&p), Some(100));
        p.thermal = Thermal::Fair;
        assert_eq!(pc_score(&p), Some(90));
        p.free_ram_mb = Some(6 * 1024);
        assert_eq!(pc_score(&p), Some(82));
        p.free_ram_mb = Some(2 * 1024);
        p.free_disk_mb = 30 * 1024;
        assert_eq!(pc_score(&p), Some(65));
        p.thermal = Thermal::Critical;
        p.free_disk_mb = 4 * 1024;
        assert_eq!(pc_score(&p), Some(10));

        let mut bare = profile(vec![]);
        bare.thermal = Thermal::Unknown;
        bare.ram_mb = 0;
        assert_eq!(pc_score(&bare), None);
    }
}
