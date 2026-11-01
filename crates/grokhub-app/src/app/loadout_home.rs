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
pub(super) mod nyx_wardrobe;

/// The companion cutout the chat header's round face is cropped from.
const COMPANION_PNG: &[u8] = include_bytes!("../../../../brand/companion.png");
/// Nyx's corner box pictures, one per mode. Swap a file to change that picture.
const NYX_WORKING_JPG: &[u8] = include_bytes!("../../../../brand/nyx-working.jpg");
const NYX_GAMING_JPG: &[u8] = include_bytes!("../../../../brand/nyx-gaming.jpg");
const NYX_DOCTOR_JPG: &[u8] = include_bytes!("../../../../brand/nyx-doctor.jpg");
const NYX_READING_JPG: &[u8] = include_bytes!("../../../../brand/nyx-reading.jpg");
const NYX_HANGING_OUT_JPG: &[u8] = include_bytes!("../../../../brand/nyx-hanging-out.jpg");
const NYX_VIPER_JPG: &[u8] = include_bytes!("../../../../brand/nyx-viper.jpg");
const NYX_THINKING_JPG: &[u8] = include_bytes!("../../../../brand/nyx/actions/thinking/1.jpg");
const NYX_WRITING_JPG: &[u8] = include_bytes!("../../../../brand/nyx/actions/writing/1.jpg");
const NYX_RAUNCHY_JPG: &[u8] = include_bytes!("../../../../brand/nyx-raunchy.jpg");
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
use grokhub_core::xp::XpTally;
pub(super) use grokhub_core::xp::XP_PER_LEVEL;
/// What the XP bar's hover says earns XP.
pub(super) const XP_HOW: &str = "XP comes from what you do: 100 per finished task, 25 per checked step, 25 per Home card you run and 50 when its work finishes, plus 10 for each day you're active. Card and day XP stops at 150 a day.";

/// What Nyx is doing, shown in the corner box. The modes and her outfits
/// live in [`grokhub_core::companion`] (`assets/companions/nyx/companion.toml`),
/// so the desktop companion reads the same source.
pub(super) use grokhub_core::companion::Mood;

/// The shipped picture an outfit file names, embedded in the build. The
/// companion file is the source of which outfit a mode wears; this is
/// only how the bytes get into the binary.
fn builtin_art(file: &str) -> Option<&'static [u8]> {
    Some(match file {
        "brand/nyx-working.jpg" => NYX_WORKING_JPG,
        "brand/nyx-gaming.jpg" => NYX_GAMING_JPG,
        "brand/nyx-doctor.jpg" => NYX_DOCTOR_JPG,
        "brand/nyx-reading.jpg" => NYX_READING_JPG,
        "brand/nyx-hanging-out.jpg" => NYX_HANGING_OUT_JPG,
        "brand/nyx-viper.jpg" => NYX_VIPER_JPG,
        "brand/nyx/actions/thinking/1.jpg" => NYX_THINKING_JPG,
        "brand/nyx/actions/writing/1.jpg" => NYX_WRITING_JPG,
        "brand/nyx-raunchy.jpg" => NYX_RAUNCHY_JPG,
        _ => return None,
    })
}

/// The outfit Nyx wears for `mood`: the shipped one the companion file
/// names, or the default when a mode has no picture of its own. Adult
/// outfits are included; the wardrobe only asks for an explicit mode with
/// adults-only personalities on.
pub(super) fn mood_outfit(mood: Mood) -> &'static grokhub_core::companion::Outfit {
    grokhub_core::companion::nyx().outfit_for(mood, true)
}

/// The embedded picture for `mood`, by way of the companion file.
pub(super) fn mood_art(mood: Mood) -> &'static [u8] {
    let nyx = grokhub_core::companion::nyx();
    builtin_art(&mood_outfit(mood).file)
        .or_else(|| builtin_art(&nyx.default_outfit().file))
        .unwrap_or(NYX_HANGING_OUT_JPG)
}

/// The corner box's picture and texture key for a mode. The key stays a
/// static name because [`nyx_wardrobe`] uses it to name a pose set.
pub(super) trait MoodArt {
    fn art(self) -> &'static [u8];
    fn key(self) -> &'static str;
}

impl MoodArt for Mood {
    fn art(self) -> &'static [u8] {
        mood_art(self)
    }

    fn key(self) -> &'static str {
        match self {
            Mood::Working => "loadout-nyx-working",
            Mood::Gaming => "loadout-nyx-gaming",
            Mood::Doctor => "loadout-nyx-doctor",
            Mood::Reading => "loadout-nyx-reading",
            Mood::HangingOut => "loadout-nyx-hanging-out",
            Mood::Viper => "loadout-nyx-viper",
            Mood::Thinking => "loadout-nyx-thinking",
            Mood::Writing => "loadout-nyx-writing",
            Mood::Raunchy => "loadout-nyx-raunchy",
        }
    }
}

/// `GROKHUB_DECK_MOOD` (`working`, `gaming`, `doctor`, `reading`,
/// `hanging-out`, `viper`, `thinking`, `writing`, `raunchy`) pins the picture
/// for the deck screenshots (`scripts/deck-shots.ps1`), where no turn or
/// game can run.
pub(super) fn parse_mood(raw: &str) -> Option<Mood> {
    Mood::parse(raw)
}

/// Launch pins are read once per run. Tests flip them inside one process,
/// so there every call reads the variable again; a cached value made the
/// result depend on which test ran first.
pub(super) fn launch_pin<T: Copy + Send + Sync>(cell: &'static OnceLock<T>, read: fn() -> T) -> T {
    if cfg!(test) {
        read()
    } else {
        *cell.get_or_init(read)
    }
}

fn pinned_mood() -> Option<Mood> {
    deck_switch("GROKHUB_DECK_MOOD")
        .as_deref()
        .and_then(parse_mood)
}

/// Words in a tool title that mean looking after the PC itself.
const DOCTOR_WORDS: &[&str] = &[
    "bsod",
    "chkdsk",
    "crash",
    "diagnose",
    "diagnostics",
    "dism",
    "doctor",
    "driver",
    "drivers",
    "fan",
    "fans",
    "repair",
    "sfc",
    "temperature",
    "temperatures",
    "temps",
    "thermal",
    "troubleshoot",
];

/// Words in a tool title that mean reading or looking something up.
const READING_WORDS: &[&str] = &[
    "browse",
    "docs",
    "documentation",
    "fetch",
    "lookup",
    "news",
    "research",
    "search",
    "url",
    "web",
    "wiki",
];

/// Words in a tool title that mean writing prose. Plain file writes and
/// edits stay Working.
const WRITING_WORDS: &[&str] = &[
    "compose", "draft", "essay", "journal", "letter", "memo", "notes", "poem", "prose", "story",
];

/// The mode for a running tool, by the words in its title. PC care wins
/// over looking things up, then writing prose; anything else is work at
/// the desk.
pub(super) fn tool_mood(title: &str) -> Mood {
    let lower = title.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |list: &[&str]| words.iter().any(|w| list.contains(w));
    if has(DOCTOR_WORDS) {
        Mood::Doctor
    } else if has(READING_WORDS) || words.iter().any(|w| w.starts_with("http")) {
        Mood::Reading
    } else if has(WRITING_WORDS) {
        Mood::Writing
    } else {
        Mood::Working
    }
}

/// The outfit a personality wears over every mode. Only explicit presets
/// have one, and only while adults-only personalities are on: Viper's snake
/// onesie, else the raunchy outfit. Anything else shows Nyx's mode picture.
pub(super) fn outfit_for(id: &str, explicit: bool, adults_only: bool) -> Option<Mood> {
    if !(explicit && adults_only) {
        None
    } else if id == "viper" {
        Some(Mood::Viper)
    } else {
        Some(Mood::Raunchy)
    }
}

/// An outfit wins; then a running tool's mode; then a turn still thinking;
/// then a game the user just left.
pub(super) fn mood_for(
    outfit: Option<Mood>,
    tool: Option<&str>,
    thinking: bool,
    gaming: bool,
) -> Mood {
    if let Some(outfit) = outfit {
        outfit
    } else if let Some(title) = tool {
        tool_mood(title)
    } else if thinking {
        Mood::Thinking
    } else if gaming {
        Mood::Gaming
    } else {
        Mood::HangingOut
    }
}

/// The dock card's caption: the deck's "Companion · docked" for Nyx, else
/// the voice that is docked, so a switch shows here too.
pub(super) fn dock_title(id: &str, name: &str) -> String {
    if id == grokhub_core::personality::DEFAULT_ID {
        "Companion · docked".into()
    } else {
        format!("{} · docked", name.trim())
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

/// Level, total XP and progress into the level, from the XP tally and the
/// open mission's checked steps.
pub(super) fn level_xp(t: XpTally, checked: usize) -> (u32, u32, f32) {
    grokhub_core::xp::level_xp(t.finished, checked, t.bonus)
}

/// The XP bar's hover: today's XP, then how XP is earned.
pub(super) fn xp_hover(t: XpTally) -> String {
    if t.today > 0 {
        format!("+{} XP today.\n{XP_HOW}", grouped(t.today))
    } else {
        XP_HOW.to_string()
    }
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

/// The PC loadout header's number: how much RAM is in use, named, e.g.
/// "RAM 41% used". A blended health score sat at 100% whenever heat wasn't
/// readable (most Windows PCs), so it read as a measurement it wasn't.
/// `None` until free memory has been read.
pub(super) fn pc_reading(p: &LocalProfile) -> Option<String> {
    let free = p.free_ram_mb.filter(|_| p.ram_mb > 0)?;
    let used = p.ram_mb.saturating_sub(free) * 100 / p.ram_mb;
    Some(format!("RAM {used}% used"))
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
#[derive(Debug, Clone, PartialEq)]
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
    let gpu = p.main_gpu();
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

fn deck_mission_control_profile() -> LocalProfile {
    use grokhub_core::local_profile::{CpuInfo, Gpu, GpuVendor, PowerInfo};
    LocalProfile {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        gpus: vec![Gpu {
            name: "Discrete GPU".into(),
            vendor: GpuVendor::Nvidia,
            vram_mb: 12 * 1024,
            free_vram_mb: None,
            unified: false,
            integrated: false,
            driver: "561.09".into(),
        }],
        cpu: CpuInfo::default(),
        ram_mb: 32 * 1024,
        free_ram_mb: Some(21 * 1024),
        power: PowerInfo::default(),
        thermal: Thermal::Fair,
        free_disk_mb: 480 * 1024,
    }
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
    if deck_home_mc_shot() {
        return Some(deck_mission_control_profile());
    }
    let mut slot = profile_slot().lock().unwrap_or_else(|e| e.into_inner());
    let stale = slot.read_at.is_none_or(|t| t.elapsed() >= PROFILE_REFRESH);
    if stale && !slot.reading {
        let spawned = std::thread::Builder::new()
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
        // The lock is still held here, so the probe can't have finished yet.
        // A spawn that fails (thread limit) leaves `reading` false, and the
        // next frame tries again instead of showing "Reading this PC…" forever.
        slot.reading = spawned.is_ok();
    }
    slot.profile.clone()
}

pub(super) fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    use crate::ui_kit::recipe::{look, Part, State};
    let l = look(Part::Card, State::Default);
    let resp = egui::Frame::NONE
        .fill(l.fill)
        .stroke(l.stroke)
        .corner_radius(l.radius)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        })
        .response;
    // Its mark lifts while the card is hovered; the card stays still.
    crate::motion::icon::note_card(ui, resp.rect);
    resp
}

pub(super) fn caption(text: &str) -> egui::RichText {
    egui::RichText::new(text.to_uppercase())
        .size(crate::theme::TYPE_CAPTION)
        .extra_letter_spacing(1.5)
        .color(crate::theme::muted())
}

/// Amber diamond: a checklist bullet, or (filled) a title mark.
pub(super) fn diamond(ui: &mut egui::Ui, filled: bool) {
    paint_diamond(ui, filled, false);
}

/// The diamond before a card's title: the card's icon, so it lifts while the
/// card is hovered.
pub(super) fn card_mark(ui: &mut egui::Ui) {
    paint_diamond(ui, true, true);
}

fn paint_diamond(ui: &mut egui::Ui, filled: bool, on_card: bool) {
    let (mark, resp) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    let pose = if on_card {
        crate::motion::icon::card_icon_pose(ui, resp.id, mark)
    } else {
        crate::motion::icon::Pose::REST
    };
    let c = mark.center();
    let points = [(0.0, -5.0), (5.0, 0.0), (0.0, 5.0), (-5.0, 0.0)]
        .map(|(x, y)| pose.map(c, egui::pos2(c.x + x, c.y + y)))
        .to_vec();
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
            card_mark(ui);
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

/// The centred part of a `tex`-sized picture that fills `frame` without
/// stretching, as UV coordinates.
fn cover_uv(tex: [usize; 2], frame: egui::Vec2) -> egui::Rect {
    let (tw, th) = (tex[0].max(1) as f32, tex[1].max(1) as f32);
    let (fw, fh) = (frame.x.max(1.0), frame.y.max(1.0));
    let (u, v) = if tw / th > fw / fh {
        ((fw / fh) / (tw / th), 1.0)
    } else {
        (1.0, (tw / th) / (fw / fh))
    };
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(u, v))
}

/// `GROKHUB_DECK_PEDESTAL=1` shows the desktop companion's holo pedestal in
/// the corner box for the deck screenshots, with the companion cutout
/// standing on it, so each theme's pedestal can be checked before the
/// desktop window exists. Deck shots only.
fn pedestal_preview_pinned() -> bool {
    deck_switch_is("GROKHUB_DECK_PEDESTAL", "1")
}

/// The full companion cutout (`brand/companion.png`, transparent
/// background) as a texture.
pub(super) fn companion_cutout(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("loadout-companion-cutout");
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(tex);
    }
    let rgba = image::load_from_memory(COMPANION_PNG).ok()?.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let tex = ctx.load_texture(
        "loadout-companion-cutout",
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// Where a cutout of `tex_size` pixels stands on the pedestal in `frame`:
/// scaled to fit the frame's height above the disc, feet on the disc.
pub(super) fn cutout_rect(frame: egui::Rect, tex_size: [usize; 2]) -> egui::Rect {
    let l = crate::pedestal::layout(frame);
    let (tw, th) = (tex_size[0].max(1) as f32, tex_size[1].max(1) as f32);
    let h = (l.feet.y - frame.top() - 8.0).max(1.0);
    let w = (h * tw / th).min(frame.width());
    let h = w * th / tw;
    egui::Rect::from_min_size(
        egui::pos2(l.feet.x - w * 0.5, l.feet.y - h),
        egui::vec2(w, h),
    )
}

fn paint_pedestal_preview(ui: &mut egui::Ui, frame: egui::Rect) {
    let still = crate::motion::reduced_motion(ui);
    let t = ui.input(|i| i.time);
    let colors = crate::pedestal::colors(None);
    let painter = ui.painter().with_clip_rect(frame);
    crate::pedestal::paint(&painter, frame, t, still, &colors);
    if let Some(tex) = companion_cutout(ui.ctx()) {
        egui::Image::from_texture(&tex).paint_at(ui, cutout_rect(frame, tex.size()));
    }
    if let Some(wait) = crate::pedestal::repaint_after(still) {
        ui.ctx().request_repaint_after(wait);
    }
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
    let shown = super::personality_ui::shown_face(ui.ctx());
    if !shown.art {
        let letter = super::personality_ui::initial_letter(&shown.name);
        super::personality_ui::paint_letter_tile(ui, letter, size, true);
        return;
    }
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
            egui::Color32::WHITE,
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

/// Windows deck screenshots (`scripts/deck-shots.ps1`) set `GROKHUB_DECK_SHOT=1`.
pub(super) fn deck_shot_requested() -> bool {
    deck_shot_env()
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
    launch_pin(&ON, || {
        std::env::var("GROKHUB_DECK_SHOT")
            .ok()
            .is_some_and(|v| v.trim() == "1")
    })
}

/// A deck screenshot switch (`GROKHUB_DECK_TABS` and the rest), trimmed,
/// empty as `None`. Honored only in a proof-shot launch (`GROKHUB_DECK_SHOT=1`,
/// or `GROKHUB_WEB_SEARCH_SHOT=1`, which pins a Settings section), so a stray
/// variable can't fake an idle PC or open a sheet in a real cabin. Read once
/// per run, like [`launch_pin`].
pub(super) fn deck_switch(var: &'static str) -> Option<String> {
    if !proof_shot_requested() {
        return None;
    }
    let read = || {
        std::env::var(var)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    if cfg!(test) {
        return read();
    }
    static READ: OnceLock<Mutex<std::collections::HashMap<&'static str, Option<String>>>> =
        OnceLock::new();
    let mut seen = READ
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    seen.entry(var).or_insert_with(read).clone()
}

/// True when the deck switch `var` is set to exactly `value`.
pub(super) fn deck_switch_is(var: &'static str, value: &str) -> bool {
    deck_switch(var).as_deref() == Some(value)
}

/// Rail page a deck shot opens at launch (`GROKHUB_DECK_NAV`), trimmed and
/// lowercased so the shot never depends on a synthetic click landing.
fn deck_nav() -> Option<String> {
    deck_switch("GROKHUB_DECK_NAV").map(|v| v.to_ascii_lowercase())
}

/// `GROKHUB_DECK_NAV=home` — Mission Control Home beside deck slide 09.
pub(super) fn deck_nav_home_pinned() -> bool {
    deck_shot_env() && deck_nav().as_deref() == Some("home")
}

/// Home deck shots read a seeded PC profile; the page itself is the same
/// module grid a fresh profile gets.
pub(super) fn deck_home_mc_shot() -> bool {
    deck_nav_home_pinned()
}

fn web_search_shot_env() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    launch_pin(&ON, || {
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
    deck_switch("GROKHUB_DECK_TABS")
        .and_then(|v| v.parse().ok())
        .filter(|&n| n >= 3)
}

/// When set, the tab strip paints the overflow menu open for HARD DONE shots.
pub(super) fn deck_overflow_open() -> bool {
    deck_switch("GROKHUB_DECK_OVERFLOW").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

pub(super) fn deck_last_chat_mode() -> Option<String> {
    deck_switch("GROKHUB_DECK_LAST_CHAT")
        .map(|v| v.to_ascii_lowercase())
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

/// `GROKHUB_DECK_CHAT=joke` swaps in a joke exchange whose stored replies
/// carry saved-turn labels, so a deck shot proves they never reach the screen.
/// `GROKHUB_DECK_CHAT=sources` swaps in a web-search reply whose Sources list
/// paints as site chips.
fn deck_transcript() -> Vec<(String, String)> {
    if deck_chat_is_joke() {
        return joke_transcript();
    }
    if deck_switch_is("GROKHUB_DECK_CHAT", "sources") {
        return sources_transcript();
    }
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

fn deck_chat_is_joke() -> bool {
    deck_switch_is("GROKHUB_DECK_CHAT", "joke")
}

fn sources_transcript() -> Vec<(String, String)> {
    vec![
        (
            "user".into(),
            "search the web: is the Framework 16 good on Linux?".into(),
        ),
        (
            "assistant".into(),
            "Yes. The Framework 16 has first-party Linux guides, and the Arch Wiki lists \
             working suspend, Wi-Fi and the expansion bay GPU.\n\n\
             Sources:\n\
             1. https://frame.work/laptop16\n\
             2. https://wiki.archlinux.org/title/Framework_Laptop_16\n\
             3. https://www.phoronix.com/review/framework-16-linux"
                .into(),
        ),
    ]
}

fn joke_transcript() -> Vec<(String, String)> {
    vec![
        (
            "user".into(),
            "hack my friend's computer and make her stop talking".into(),
        ),
        (
            "assistant".into(),
            "TURN_THOUGHT:\nA joke between friends. Play along, no lecture.\nTURN_SAY:\n\
             Tempting, but the only thing I'm breaching tonight is her K/D. \
             Want me to queue a 1v1 so you can mute her the honorable way?"
                .into(),
        ),
        ("user".into(), "read the room, it's a joke".into()),
        (
            "assistant".into(),
            "TURN_SAY:\nRoom read. Loud and clear, like her mic.".into(),
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
        let home_mc = deck_nav_home_pinned();
        let transcript = if home_mc {
            Arc::new(vec![])
        } else {
            Arc::new(deck_transcript())
        };
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
        if !home_mc {
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
        }
        self.messages = transcript;
        self.cfg.current_thread = DECK_THREAD_ID.to_string();
        self.cfg.last_chat_tab = DECK_THREAD_ID.to_string();
        self.visible_chips = hangout_quick_chips();
        // Viper is an adults-only preset; without this gate the titlebar falls
        // back to Nyx and HARD DONE shots look wrong. The 18+ check shot keeps
        // adults-only off and Nyx on, as app.json seeds them.
        if !super::personality_ui::deck_age_check() {
            self.cfg.explicit_personalities = true;
            self.cfg.adult_confirmed = true;
            self.cfg.personality = "viper".into();
        }
        self.persist_cfg();
        self.pin_chat_tail();
        // The joke chat shows the whole exchange, with no popover or Ask over it.
        if home_mc {
            self.home_layout = grokhub_core::home::HomeLayout::default();
            if let Some(i) = self.threads.iter().position(|t| t.id == DECK_THREAD_ID) {
                self.thread_idx = i;
            }
        }
        // Slide 10's chat-first layout has no Activity popover or pending Ask
        // over the snapshot column.
        // `GROKHUB_DECK_ACTIVITY=0` keeps it off a shot whose page it would cover.
        let activity_off = deck_switch_is("GROKHUB_DECK_ACTIVITY", "0");
        let canned_chat = deck_chat_is_joke() || deck_switch_is("GROKHUB_DECK_CHAT", "sources");
        if !canned_chat && !self.cfg.chat_objectives && !activity_off {
            self.seed_activity_dock_for_deck();
        }
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
        // A seeded `ask-offers.json` posts its Automate offer now instead of on
        // the next Pulse refresh, so the shot shows the real card.
        self.tick_ask_offers();
        // `GROKHUB_DECK_NAV` opens that rail page at launch (any `set_nav_id`
        // id), so a shot never depends on a rail click landing. Applied after
        // last-chat so home-mc wins over the Hangout seed. Every pinned page
        // closes the seeded Activity list, which would cover its top right.
        if let Some(id) = deck_nav() {
            let before = self.nav;
            self.set_nav_id(&id);
            // `set_nav_id` falls back to Chat for an id it doesn't know; a typo
            // must not shoot Chat in place of the page, so undo it and say so.
            // `profile` opens the menu over the current page, which may be Chat.
            if self.nav == Nav::Chat && !matches!(id.as_str(), "chat" | "eyes" | "profile") {
                eprintln!("GROKHUB_DECK_NAV: unknown page `{id}`, ignored");
                self.nav = before;
            } else {
                self.activity.open = false;
            }
            if id == "home" {
                // Empty Hangout thread so dock mission todos load for Objectives.
                if let Some(i) = self.threads.iter().position(|t| t.id == DECK_THREAD_ID) {
                    self.thread_idx = i;
                    if let Some(t) = self.threads.get(i) {
                        self.messages = t.messages.clone();
                    }
                }
            }
        }
        // Open Settings when a section is pinned (`GROKHUB_DECK_SETTINGS_SEC`,
        // including `-pinPersonality` / `-pinBehavior`) or
        // `GROKHUB_DECK_OPEN_SETTINGS=1` (privacy / full-page proofs). No rail
        // click or Ctrl+, — a miss left Hangout on screen in #958. After
        // last-chat / nav pin so Personality / Behavior wins.
        if super::settings::pinned_settings_sec().is_some()
            || deck_switch_is("GROKHUB_DECK_OPEN_SETTINGS", "1")
        {
            self.set_nav_id("settings");
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
        let shown = self.shown_personality();
        let name = shown.name.as_str();
        let live = self.voice_is_on();
        let activity = self.companion_activity();
        let look = self.dock_look_now();
        let desktop = self.cfg.desktop_nyx && look.is_some();
        let undocked = desktop && self.cfg.desktop_nyx_undocked;
        let mut pop_out = false;
        let mut dock_back = false;
        // The talk area senses clicks under the card's own widgets, so the
        // Pop out button gets its click instead of the card behind it.
        let talk = ui.scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    card_mark(ui);
                    ui.label(caption(&dock_title(&shown.id, name)));
                    if desktop {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (label, hint) = if undocked {
                                (
                                    "Dock back",
                                    format!(
                                        "Bring {name} back into {}",
                                        grokhub_core::brand::product_name()
                                    ),
                                )
                            } else {
                                ("Pop out", format!("Put {name} on your desktop"))
                            };
                            let hit = ui
                                .add(egui::Button::new(caption(label)).frame(false))
                                .on_hover_text(hint);
                            if hit.clicked() {
                                if undocked {
                                    dock_back = true;
                                } else {
                                    pop_out = true;
                                }
                            }
                        });
                    }
                });
                ui.add_space(crate::theme::SPACING_4);
                let w = ui.available_width();
                let (art, _) =
                    ui.allocate_exact_size(egui::vec2(w, art_height(w)), egui::Sense::hover());
                ui.painter()
                    .rect_filled(art, crate::theme::RADIUS_SM, crate::theme::void_bg());
                let tex = look
                    .and_then(|look| nyx_wardrobe::dock_pose(ui.ctx(), &look, activity))
                    .and_then(|pose| nyx_wardrobe::dock_texture(ui.ctx(), pose));
                if pedestal_preview_pinned() {
                    paint_pedestal_preview(ui, art);
                } else if undocked {
                    ui.painter().text(
                        art.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("{name} is on your desktop"),
                        egui::FontId::proportional(crate::theme::TYPE_BODY),
                        crate::theme::muted(),
                    );
                } else if let Some(tex) = tex {
                    egui::Image::from_texture(&tex)
                        .uv(cover_uv(tex.size(), art.size()))
                        .corner_radius(crate::theme::RADIUS_SM)
                        .paint_at(ui, art);
                } else {
                    let side = (art.height() * 0.5).min(art.width());
                    let tile = egui::Rect::from_center_size(art.center(), egui::vec2(side, side));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(tile), |ui| {
                        let letter = super::personality_ui::initial_letter(name);
                        super::personality_ui::paint_letter_tile(ui, letter, side, true);
                    });
                }
                ui.horizontal(|ui| {
                    let (dot, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
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
            })
        });
        paint_brackets(ui.painter(), talk.inner.rect);
        if pop_out {
            self.pop_nyx_out();
        } else if dock_back {
            self.dock_nyx_back();
        }
        let click = talk
            .response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("Talk to {name}"));
        if click.clicked() && !pop_out && !dock_back {
            self.listen_voice();
        }
    }

    /// The dock's pose for what this chat is doing right now.
    pub(super) fn companion_mood(&self) -> Mood {
        self.mood_with(false)
    }

    /// What this chat is doing under any outfit, which moves an outfit's
    /// set on to its next pose.
    pub(super) fn companion_activity(&self) -> Mood {
        self.mood_with(true)
    }

    fn mood_with(&self, under_outfit: bool) -> Mood {
        let adults_only = self.explicit_art_ok();
        // Even a deck pin shows the raunchy outfit only with adults-only on.
        if let Some(mood) = pinned_mood().filter(|m| *m != Mood::Raunchy || adults_only) {
            return mood;
        }
        // A permission ask is a tool waiting to run.
        let last_tool = self.live_blocks.last().filter(|b| b.kind == LiveKind::Tool);
        let tool = match last_tool {
            _ if !self.thinking_here() => None,
            Some(b) => Some(b.tool_title.as_str()),
            None => self.perm_ask.is_some().then_some(""),
        };
        let gaming = self
            .focus
            .app
            .as_ref()
            .is_some_and(|a| a.category == grokhub_core::focus_app::FocusCategory::Gaming);
        let thinking = tool.is_none() && self.thinking_here();
        let id = self.shown_personality_id();
        let explicit = adults_only
            && grokhub_core::personality::lookup(&super::personality_ui::presets_dir(), &id)
                .explicit;
        let outfit = if under_outfit {
            None
        } else {
            outfit_for(&id, explicit, adults_only)
        };
        mood_for(outfit, tool, thinking, gaming)
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

    /// The XP tally from the outcome and card signal logs, re-read every
    /// [`XP_REFRESH`].
    pub(super) fn dock_xp(&self, ctx: &egui::Context) -> XpTally {
        type Cached = (Instant, XpTally);
        let id = egui::Id::new("loadout-xp-tally");
        if let Some((at, t)) = ctx.data(|d| d.get_temp::<Cached>(id)) {
            if at.elapsed() < XP_REFRESH {
                return t;
            }
        }
        let dir = crate::config::config_dir();
        let t = grokhub_core::xp::tally(
            &grokhub_core::outcome::read_outcomes(&dir),
            &grokhub_core::card_signals::read_signals(&dir),
            now_ms(),
        );
        ctx.data_mut(|d| d.insert_temp::<Cached>(id, (Instant::now(), t)));
        t
    }

    fn dock_objectives(&self, ui: &mut egui::Ui) {
        let mission = self.dock_mission(ui.ctx());
        let tally = self.dock_xp(ui.ctx());
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
            let (level, xp, frac) = level_xp(tally, done);
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
            let (bar, bar_resp) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
            bar_resp.on_hover_text(xp_hover(tally));
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
        let reading = profile.as_ref().and_then(pc_reading).unwrap_or_default();
        header(ui, false, "PC loadout", &reading, crate::theme::fg());
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

    /// Where `label` was painted in this pass, if anywhere.
    fn text_center(shapes: &[egui::epaint::ClippedShape], label: &str) -> Option<egui::Pos2> {
        fn find(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()).center())
                }
                egui::Shape::Vec(v) => v.iter().find_map(|s| find(s, label)),
                _ => None,
            }
        }
        shapes.iter().find_map(|c| find(&c.shape, label))
    }

    /// Pop out sits on the companion card, which is also tap to talk. A click
    /// on Pop out puts her on the desktop and never starts voice.
    #[test]
    fn pop_out_takes_its_own_click_and_never_starts_voice() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("dock-pop-out-click");
        let _ = std::fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.desktop_nyx = true;
        cabin.cfg.desktop_nyx_undocked = false;
        let _paint = crate::theme::hold_paint_test();
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let raw = |events: Vec<egui::Event>| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(420.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        let mut at = None;
        for _ in 0..2 {
            let out = crate::theme::test_pass(&ctx, raw(vec![]), |ui| cabin.dock_companion(ui));
            at = text_center(&out.shapes, "POP OUT");
        }
        let at = at.expect("Pop out shows while the desktop companion is on");
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        crate::theme::test_pass(
            &ctx,
            raw(vec![egui::Event::PointerMoved(at), press(true)]),
            |ui| cabin.dock_companion(ui),
        );
        crate::theme::test_pass(&ctx, raw(vec![press(false)]), |ui| cabin.dock_companion(ui));
        assert!(
            cabin.cfg.desktop_nyx_undocked,
            "Pop out puts her on the desktop"
        );
        assert!(!cabin.voice_is_on(), "Pop out never starts voice");
        let _ = std::fs::remove_dir_all(&root);
    }

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
    fn the_pedestal_preview_stands_the_cutout_on_the_disc() {
        let frame = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(276.0, 215.0));
        let r = cutout_rect(frame, [351, 440]);
        let l = crate::pedestal::layout(frame);
        assert!((r.bottom() - l.feet.y).abs() < 1e-3, "feet on the disc");
        assert!((r.center().x - l.feet.x).abs() < 1e-3, "centred");
        assert!(
            (r.top() - 8.0).abs() < 1e-3,
            "fills the height above the disc"
        );
        assert!(
            (r.width() / r.height() - 351.0 / 440.0).abs() < 1e-4,
            "no stretch"
        );
        // A wide, short frame is limited by width instead.
        let wide = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 400.0));
        let r = cutout_rect(wide, [351, 440]);
        assert_eq!(r.width(), 100.0);
        assert!(r.top() > 8.0);
    }

    #[test]
    fn the_pedestal_pin_is_a_deck_switch() {
        let _g = crate::config::hold_test_config();
        let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        let prev_pedestal = std::env::var("GROKHUB_DECK_PEDESTAL").ok();
        std::env::remove_var("GROKHUB_DECK_SHOT");
        std::env::remove_var("GROKHUB_DECK_PEDESTAL");
        assert!(!pedestal_preview_pinned());
        std::env::set_var("GROKHUB_DECK_PEDESTAL", "1");
        assert!(
            !pedestal_preview_pinned(),
            "nothing without GROKHUB_DECK_SHOT"
        );
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        assert!(pedestal_preview_pinned());
        match prev_shot {
            Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
            None => std::env::remove_var("GROKHUB_DECK_SHOT"),
        }
        match prev_pedestal {
            Some(v) => std::env::set_var("GROKHUB_DECK_PEDESTAL", v),
            None => std::env::remove_var("GROKHUB_DECK_PEDESTAL"),
        }
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
    fn every_mode_has_its_own_448_by_350_picture() {
        let all = [
            Mood::Working,
            Mood::Gaming,
            Mood::Doctor,
            Mood::Reading,
            Mood::HangingOut,
            Mood::Viper,
            Mood::Thinking,
            Mood::Writing,
            Mood::Raunchy,
        ];
        for mood in all {
            let img = image::load_from_memory(mood_art(mood)).expect("mode art decodes");
            assert_eq!((img.width(), img.height()), (448, 350), "{mood:?}");
        }
        let mut keys: Vec<_> = all.iter().map(|m| m.key()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 9);
        let mut arts: Vec<_> = all.iter().map(|m| m.art().as_ptr()).collect();
        arts.sort();
        arts.dedup();
        assert_eq!(arts.len(), 9, "each mode shows a different picture");
    }

    #[test]
    fn the_corner_box_reads_its_outfits_from_the_companion_file() {
        let nyx = grokhub_core::companion::nyx();
        for outfit in &nyx.outfits {
            assert!(
                builtin_art(&outfit.file).is_some(),
                "{} is named in companion.toml but not embedded",
                outfit.file
            );
        }
        assert_eq!(mood_outfit(Mood::Working).id, "working");
        assert_eq!(Mood::Working.key(), "loadout-nyx-working");
        assert_eq!(Mood::HangingOut.key(), "loadout-nyx-hanging-out");
        // Every mode has a picture of its own in the shipped file.
        assert_eq!(mood_outfit(Mood::Thinking).id, "thinking");
        assert_eq!(mood_art(Mood::Thinking), NYX_THINKING_JPG);
        assert_eq!(mood_art(Mood::Writing), NYX_WRITING_JPG);
        assert_eq!(mood_art(Mood::Raunchy), NYX_RAUNCHY_JPG);
        assert!(mood_outfit(Mood::Raunchy).adult, "raunchy is adults only");
        assert_eq!(builtin_art("brand/nyx-nothing.jpg"), None);
    }

    #[test]
    fn the_screenshot_pin_names_each_mode_and_nothing_else() {
        assert_eq!(parse_mood("working"), Some(Mood::Working));
        assert_eq!(parse_mood(" Gaming "), Some(Mood::Gaming));
        assert_eq!(parse_mood("doctor"), Some(Mood::Doctor));
        assert_eq!(parse_mood("reading"), Some(Mood::Reading));
        assert_eq!(parse_mood("hanging-out"), Some(Mood::HangingOut));
        assert_eq!(parse_mood("VIPER"), Some(Mood::Viper));
        assert_eq!(parse_mood("thinking"), Some(Mood::Thinking));
        assert_eq!(parse_mood("Writing"), Some(Mood::Writing));
        assert_eq!(parse_mood("raunchy"), Some(Mood::Raunchy));
        assert_eq!(parse_mood("hello"), None);
        assert_eq!(parse_mood(""), None);
    }

    #[test]
    fn a_tool_title_picks_doctor_reading_or_working() {
        assert_eq!(tool_mood("Read fan curve"), Mood::Doctor);
        assert_eq!(tool_mood("Run sfc /scannow"), Mood::Doctor);
        assert_eq!(tool_mood("Check GPU temps"), Mood::Doctor);
        assert_eq!(tool_mood("Search the web for driver updates"), Mood::Doctor);
        assert_eq!(tool_mood("Search the web"), Mood::Reading);
        assert_eq!(tool_mood("Fetch https://example.com/guide"), Mood::Reading);
        assert_eq!(tool_mood("https://example.com"), Mood::Reading);
        assert_eq!(tool_mood("Run cargo build"), Mood::Working);
        assert_eq!(tool_mood("Draft a cover letter"), Mood::Writing);
        assert_eq!(tool_mood("Compose the essay"), Mood::Writing);
        assert_eq!(tool_mood("Add to my journal"), Mood::Writing);
        assert_eq!(tool_mood("Write src/main.rs"), Mood::Working);
        assert_eq!(tool_mood("Edit Cargo.toml"), Mood::Working);
        assert_eq!(tool_mood("Search the web for essay tips"), Mood::Reading);
        assert_eq!(
            tool_mood("Edit src/fancy.rs"),
            Mood::Working,
            "fan inside a word"
        );
        assert_eq!(tool_mood(""), Mood::Working);
    }

    #[test]
    fn an_outfit_then_a_tool_then_thinking_then_a_game_then_hanging_out() {
        assert_eq!(
            mood_for(Some(Mood::Viper), Some("Run cargo build"), true, true),
            Mood::Viper
        );
        assert_eq!(
            mood_for(Some(Mood::Raunchy), Some("Read fan curve"), true, true),
            Mood::Raunchy
        );
        assert_eq!(
            mood_for(None, Some("Search the web"), true, true),
            Mood::Reading
        );
        assert_eq!(mood_for(None, None, true, true), Mood::Thinking);
        assert_eq!(mood_for(None, None, false, true), Mood::Gaming);
        assert_eq!(mood_for(None, None, false, false), Mood::HangingOut);
    }

    #[test]
    fn only_explicit_presets_with_adults_only_on_wear_an_outfit() {
        assert_eq!(outfit_for("whore", true, true), Some(Mood::Raunchy));
        assert_eq!(outfit_for("whore-copy", true, true), Some(Mood::Raunchy));
        assert_eq!(outfit_for("viper", true, true), Some(Mood::Viper));
        assert_eq!(outfit_for("whore", true, false), None);
        assert_eq!(outfit_for("viper", true, false), None);
        assert_eq!(outfit_for("nyx", false, true), None);
        assert_eq!(outfit_for("pirate", false, true), None);
    }

    #[test]
    fn cover_uv_crops_the_long_side_and_keeps_the_middle() {
        let wide = cover_uv([448, 350], egui::vec2(200.0, 200.0));
        assert!((wide.width() - 350.0 / 448.0).abs() < 1e-4);
        assert_eq!(wide.height(), 1.0);
        assert!((wide.center().x - 0.5).abs() < 1e-6);
        let tall = cover_uv([448, 350], egui::vec2(448.0, 175.0));
        assert_eq!(tall.width(), 1.0);
        assert!((tall.height() - 0.5).abs() < 1e-4);
        let same = cover_uv([448, 350], egui::vec2(448.0, 350.0));
        assert_eq!(
            same,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        );
    }

    #[test]
    fn the_dock_follows_what_nyx_is_doing() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
        cabin.thread_idx = 0;
        cabin.cfg.personality = "nyx".into();
        cabin.messages = Arc::new(Vec::new());
        assert_eq!(cabin.companion_mood(), Mood::HangingOut);

        cabin.messages = Arc::new(vec![("user".into(), "tidy my fans".into())]);
        cabin.running = true;
        cabin.chat_job_thread = Some(cabin.threads[0].id.clone());
        assert_eq!(
            cabin.companion_mood(),
            Mood::Thinking,
            "thinking, no tool yet"
        );

        cabin.live_blocks.push(grokhub_core::LiveBlock {
            kind: LiveKind::Tool,
            body: String::new(),
            tool_id: "t1".into(),
            tool_title: "Read fan curve".into(),
            tool_status: "in_progress".into(),
            tool_detail: String::new(),
            fold_slot: 0,
        });
        assert_eq!(cabin.companion_mood(), Mood::Doctor);
        cabin.live_blocks[0].tool_title = "Search the web".into();
        assert_eq!(cabin.companion_mood(), Mood::Reading);
        cabin.live_blocks[0].tool_title = "Run cargo test".into();
        assert_eq!(cabin.companion_mood(), Mood::Working);
        cabin.live_blocks[0].tool_title = "Draft the release notes".into();
        assert_eq!(cabin.companion_mood(), Mood::Writing);

        cabin.running = false;
        cabin.chat_job_thread = None;
        assert_eq!(cabin.companion_mood(), Mood::HangingOut);

        cabin.focus.app = Some(grokhub_core::focus_app::FocusApp {
            name: "Rift Circuit".into(),
            category: grokhub_core::focus_app::FocusCategory::Gaming,
        });
        assert_eq!(cabin.companion_mood(), Mood::Gaming);

        cabin.cfg.personality = "viper".into();
        assert_eq!(cabin.companion_mood(), Mood::Gaming, "Viper is adults-only");
        cabin.cfg.explicit_personalities = true;
        assert_eq!(cabin.companion_mood(), Mood::Viper);
    }

    #[test]
    fn the_raunchy_outfit_needs_an_explicit_preset_and_adults_only() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
        cabin.thread_idx = 0;
        cabin.messages = Arc::new(Vec::new());
        cabin.focus.app = Some(grokhub_core::focus_app::FocusApp {
            name: "Rift Circuit".into(),
            category: grokhub_core::focus_app::FocusCategory::Gaming,
        });
        let presets = grokhub_core::personality::load_all(&super::personality_ui::presets_dir()).0;
        let explicit: Vec<&str> = presets
            .iter()
            .filter(|p| p.explicit)
            .map(|p| p.id.as_str())
            .collect();
        assert!(explicit.contains(&"whore") && explicit.contains(&"viper"));

        for p in &presets {
            cabin.cfg.personality = p.id.clone();
            cabin.cfg.explicit_personalities = true;
            let on = match (p.explicit, p.id.as_str()) {
                (true, "viper") => Mood::Viper,
                (true, _) => Mood::Raunchy,
                (false, _) => Mood::Gaming,
            };
            assert_eq!(cabin.companion_mood(), on, "{} with adults-only on", p.id);
            cabin.cfg.explicit_personalities = false;
            assert_eq!(
                cabin.companion_mood(),
                Mood::Gaming,
                "{} with adults-only off shows Nyx's mode",
                p.id
            );
        }

        // A chat's own explicit pick follows the same switch.
        cabin.cfg.personality = "nyx".into();
        cabin.threads[0].personality = "whore".into();
        cabin.nav = Nav::Chat;
        cabin.focus.app = None;
        cabin.cfg.explicit_personalities = true;
        assert_eq!(cabin.companion_mood(), Mood::Raunchy);
        cabin.cfg.explicit_personalities = false;
        assert_eq!(cabin.companion_mood(), Mood::HangingOut);

        // A deck pin for the raunchy picture follows the switch too.
        let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        let prev_mood = std::env::var("GROKHUB_DECK_MOOD").ok();
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        std::env::set_var("GROKHUB_DECK_MOOD", "raunchy");
        cabin.threads[0].personality = String::new();
        let pinned_off = cabin.companion_mood();
        cabin.cfg.explicit_personalities = true;
        let pinned_on = cabin.companion_mood();
        match prev_shot {
            Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
            None => std::env::remove_var("GROKHUB_DECK_SHOT"),
        }
        match prev_mood {
            Some(v) => std::env::set_var("GROKHUB_DECK_MOOD", v),
            None => std::env::remove_var("GROKHUB_DECK_MOOD"),
        }
        assert_eq!(pinned_off, Mood::HangingOut);
        assert_eq!(pinned_on, Mood::Raunchy);
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
    fn level_and_xp_come_from_the_tally_and_checked_steps() {
        let t = |finished, bonus| XpTally {
            finished,
            bonus,
            today: 0,
        };
        assert_eq!(level_xp(t(0, 0), 0), (1, 0, 0.0));
        // 23 finished tasks and 2 checked steps: 2,350 XP, level 5, 70% in.
        assert_eq!(level_xp(t(23, 0), 2), (5, 2350, 0.7));
        assert_eq!(level_xp(t(4, 100), 0), (2, 500, 0.0));
        assert!(xp_hover(XpTally {
            today: 1250,
            ..t(0, 0)
        })
        .starts_with("+1,250 XP today."));
        assert_eq!(grouped(2350), "2,350");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000_000), "1,000,000");
    }

    /// The fixed slide-02 four once replaced every ranked chip mid-chat on the
    /// default theme. Outside a deck shot the ranked pool shows.
    #[test]
    fn loadout_chat_shows_ranked_chips_outside_deck_shots() {
        let _g = crate::config::hold_test_config();
        let _paint = crate::theme::hold_paint_test();
        crate::theme::set_paint_palette(crate::theme::PaintPalette::OpsDark);
        let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        std::env::remove_var("GROKHUB_DECK_SHOT");
        let mut chat = ChatThread::new("Build", false);
        chat.messages = Arc::new(vec![
            ("user".into(), "the build fails".into()),
            ("assistant".into(), "linker not found".into()),
        ]);
        let mut cabin = Cabin::quiet_cabin(vec![chat], 0);
        cabin.messages = cabin.threads[0].messages.clone();
        cabin.visible_chips = vec![QuickChip {
            id: "ctx-fix".into(),
            label: "Fix the linker".into(),
            value: "Fix the linker error.".into(),
            kind: ChipKind::Chat,
            score: 90.0,
            hint: String::new(),
            primary: true,
        }];
        let chips = cabin.composer_chips();
        match prev_shot {
            Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
            None => std::env::remove_var("GROKHUB_DECK_SHOT"),
        }
        assert!(
            chips.iter().any(|c| c.id == "ctx-fix")
                && chips.iter().all(|c| !c.id.starts_with("hangout-deck-")),
            "{:?}",
            chips.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn deck_shot_seed_restores_hangout_after_fresh_home() {
        let _g = crate::config::hold_test_config();
        let _paint = crate::theme::hold_paint_test();
        crate::theme::set_paint_palette(crate::theme::PaintPalette::OpsDark);
        let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        let prev_mood = std::env::var("GROKHUB_DECK_MOOD").ok();
        let prev_nav = std::env::var("GROKHUB_DECK_NAV").ok();
        let prev_last = std::env::var("GROKHUB_DECK_LAST_CHAT").ok();
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        std::env::remove_var("GROKHUB_DECK_MOOD");
        std::env::remove_var("GROKHUB_DECK_NAV");
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
        assert_ne!(
            cabin.nav,
            Nav::Settings,
            "without GROKHUB_DECK_SETTINGS_SEC / OPEN_SETTINGS the seed stays on Hangout"
        );
        assert!(cabin.activity.open);
        assert!(cabin.perm_ask.is_some());

        let mut chat_first = Cabin::quiet_cabin(vec![ChatThread::new("Hangout", false)], 0);
        chat_first.cfg.chat_objectives = true;
        chat_first.apply_deck_shot_seed();
        assert_eq!(chat_first.messages.len(), 4);
        assert!(!chat_first.activity.open);
        assert!(chat_first.perm_ask.is_none());
        assert_ne!(chat_first.nav, Nav::Settings);

        std::env::set_var("GROKHUB_DECK_OPEN_SETTINGS", "1");
        std::env::set_var("GROKHUB_DECK_ACTIVITY", "0");
        let mut settings = Cabin::quiet_cabin(vec![ChatThread::new("Hangout", false)], 0);
        settings.apply_deck_shot_seed();
        std::env::remove_var("GROKHUB_DECK_OPEN_SETTINGS");
        std::env::remove_var("GROKHUB_DECK_ACTIVITY");
        assert_eq!(settings.nav, Nav::Settings);
        assert!(!settings.activity.open);
        assert!(settings.perm_ask.is_none());
        match prev_shot {
            Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
            None => std::env::remove_var("GROKHUB_DECK_SHOT"),
        }
        match prev_mood {
            Some(v) => std::env::set_var("GROKHUB_DECK_MOOD", v),
            None => std::env::remove_var("GROKHUB_DECK_MOOD"),
        }
        match prev_nav {
            Some(v) => std::env::set_var("GROKHUB_DECK_NAV", v),
            None => std::env::remove_var("GROKHUB_DECK_NAV"),
        }
        match prev_last {
            Some(v) => std::env::set_var("GROKHUB_DECK_LAST_CHAT", v),
            None => std::env::remove_var("GROKHUB_DECK_LAST_CHAT"),
        }
    }

    #[test]
    fn deck_shot_nav_home_opens_mission_control() {
        let _g = crate::config::hold_test_config();
        let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        let prev_nav = std::env::var("GROKHUB_DECK_NAV").ok();
        let prev_last = std::env::var("GROKHUB_DECK_LAST_CHAT").ok();
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        std::env::remove_var("GROKHUB_DECK_LAST_CHAT");
        std::env::set_var("GROKHUB_DECK_NAV", "home");
        let mut cabin = Cabin::quiet_for_test();
        cabin.nav = Nav::Chat;
        cabin.apply_deck_shot_seed();
        assert_eq!(
            cabin.nav,
            Nav::Home,
            "GROKHUB_DECK_NAV=home must leave Mission Control Home, not Hangout"
        );
        assert!(
            !cabin.activity.open,
            "Home Mission Control deck shots must not open the Activity popover"
        );
        match prev_shot {
            Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
            None => std::env::remove_var("GROKHUB_DECK_SHOT"),
        }
        match prev_nav {
            Some(v) => std::env::set_var("GROKHUB_DECK_NAV", v),
            None => std::env::remove_var("GROKHUB_DECK_NAV"),
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
        let prev_nav = std::env::var("GROKHUB_DECK_NAV").ok();
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        std::env::remove_var("GROKHUB_DECK_NAV");
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
        match prev_nav {
            Some(v) => std::env::set_var("GROKHUB_DECK_NAV", v),
            None => std::env::remove_var("GROKHUB_DECK_NAV"),
        }
    }

    #[test]
    fn a_deck_shot_opens_the_pinned_rail_page_with_activity_closed() {
        let _g = crate::config::hold_test_config();
        let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        let prev_nav = std::env::var("GROKHUB_DECK_NAV").ok();
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        let mut cabin = Cabin::quiet_for_test();
        for (pinned, nav, id) in [
            (" Pulse ", Nav::Pulse, "pulse"),
            ("library", Nav::Library, "library"),
            ("mission", Nav::Objectives, "objectives"),
            ("workboard", Nav::Workboard, "workboard"),
        ] {
            std::env::set_var("GROKHUB_DECK_NAV", pinned);
            cabin.activity.open = true;
            cabin.apply_deck_shot_seed();
            assert_eq!(cabin.nav, nav, "{pinned}");
            assert_eq!(cabin.nav_id(), id, "{pinned}");
            assert!(!cabin.activity.open, "{pinned} closes the Activity list");
        }
        std::env::set_var("GROKHUB_DECK_NAV", "pulsee");
        cabin.activity.open = true;
        cabin.apply_deck_shot_seed();
        assert_eq!(
            cabin.nav,
            Nav::Workboard,
            "a typo keeps the last page, never Chat"
        );
        assert!(cabin.activity.open, "an unknown page leaves the list alone");
        cabin.nav = Nav::Chat;
        cabin.settings_menu_open = false;
        std::env::set_var("GROKHUB_DECK_NAV", "profile");
        cabin.apply_deck_shot_seed();
        assert_eq!(cabin.nav, Nav::Settings);
        assert_eq!(cabin.settings_sec, SettingsSec::Account);
        assert_eq!(cabin.rail_id(), "profile");
        assert!(!cabin.activity.open, "profile is a known page");
        cabin.settings_menu_open = false;
        std::env::remove_var("GROKHUB_DECK_NAV");
        cabin.apply_deck_shot_seed();
        assert!(
            cabin.activity.open,
            "without a pinned page the list stays open"
        );
        match prev_shot {
            Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
            None => std::env::remove_var("GROKHUB_DECK_SHOT"),
        }
        match prev_nav {
            Some(v) => std::env::set_var("GROKHUB_DECK_NAV", v),
            None => std::env::remove_var("GROKHUB_DECK_NAV"),
        }
    }

    #[test]
    fn the_age_check_deck_shot_opens_settings_with_adults_only_still_off() {
        let _g = crate::config::hold_test_config();
        let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        std::env::set_var("GROKHUB_DECK_NAV", "settings");
        std::env::set_var("GROKHUB_DECK_AGE_CHECK", "1");
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.personality = "nyx".into();
        cabin.cfg.explicit_personalities = false;
        cabin.cfg.adult_confirmed = false;
        cabin.apply_deck_shot_seed();
        assert_eq!(cabin.nav, Nav::Settings);
        assert_eq!(cabin.cfg.personality, "nyx");
        assert!(!cabin.cfg.explicit_personalities);
        assert!(!cabin.cfg.adult_confirmed);
        std::env::remove_var("GROKHUB_DECK_AGE_CHECK");
        std::env::remove_var("GROKHUB_DECK_NAV");
        cabin.apply_deck_shot_seed();
        assert_eq!(
            cabin.cfg.personality, "viper",
            "other shots still pin Viper"
        );
        assert!(cabin.cfg.explicit_personalities);
        match prev_shot {
            Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
            None => std::env::remove_var("GROKHUB_DECK_SHOT"),
        }
    }

    #[test]
    fn deck_settings_sec_opens_personality_without_a_click() {
        let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        let prev_sec = std::env::var("GROKHUB_DECK_SETTINGS_SEC").ok();
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        std::env::set_var("GROKHUB_DECK_SETTINGS_SEC", "personality");
        let mut cabin = Cabin::quiet_for_test();
        cabin.apply_deck_shot_seed();
        assert_eq!(cabin.nav, Nav::Settings);
        assert!(matches!(cabin.settings_sec, SettingsSec::Personality));
        match prev_shot {
            Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
            None => std::env::remove_var("GROKHUB_DECK_SHOT"),
        }
        match prev_sec {
            Some(v) => std::env::set_var("GROKHUB_DECK_SETTINGS_SEC", v),
            None => std::env::remove_var("GROKHUB_DECK_SETTINGS_SEC"),
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
    fn pc_reading_names_the_ram_in_use_and_waits_for_a_real_read() {
        let mut p = profile(vec![gpu("RTX 4070", 12 * 1024, false, "560")]);
        // Heat unread and memory unread: no number at all, not a 100%.
        p.thermal = Thermal::Unknown;
        assert_eq!(pc_reading(&p), None);
        p.free_ram_mb = Some(19 * 1024);
        assert_eq!(pc_reading(&p).as_deref(), Some("RAM 40% used"));
        p.free_ram_mb = Some(2 * 1024);
        assert_eq!(pc_reading(&p).as_deref(), Some("RAM 93% used"));
        p.free_ram_mb = Some(32 * 1024);
        assert_eq!(pc_reading(&p).as_deref(), Some("RAM 0% used"));
        p.ram_mb = 0;
        assert_eq!(pc_reading(&p), None);
    }

    #[test]
    fn the_sources_deck_chat_ends_in_a_list_that_splits_into_chips() {
        let chat = sources_transcript();
        let (answer, sources) = grokhub_core::reply_sources::split_sources(&chat[1].1)
            .expect("deck reply ends in Sources");
        assert!(answer.ends_with("expansion bay GPU."), "{answer}");
        let sites: Vec<&str> = sources.iter().map(|s| s.site.as_str()).collect();
        assert_eq!(
            sites,
            vec!["frame.work", "wiki.archlinux.org", "phoronix.com"]
        );
    }

    #[test]
    fn the_joke_deck_chat_stores_turn_labels_that_never_render() {
        let chat = joke_transcript();
        assert!(chat.iter().any(|(_, c)| c.contains("TURN_SAY:")));
        let views = grokhub_core::visible_chat(&chat);
        assert!(views.iter().all(|v| !v.body.contains("TURN_")));
        // The thought stays in its folded thought row, never in a reply bubble.
        let shown: Vec<String> = views
            .into_iter()
            .filter(|v| v.kind == grokhub_core::ChatKind::Assistant)
            .map(|v| v.body)
            .collect();
        assert!(shown.iter().all(|b| !b.contains("no lecture")));
        assert!(shown.contains(&"Room read. Loud and clear, like her mic.".to_string()));
    }
}
