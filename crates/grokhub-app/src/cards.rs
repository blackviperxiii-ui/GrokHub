//! Grok catalog chrome — huge title, white pills, 3-column icon tiles.

use crate::icons::{self, TileIcon};
use crate::ui_kit::{card_frame, CardVariant};
use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
use eframe::egui::{
    self, Align2, Color32, ColorImage, FontId, RichText, Sense, Stroke, TextureHandle,
    TextureOptions,
};
use grokhub_core::{
    chat_run_dot_alpha, curate_wall, imagine_media_click, imagine_result_fit, parse_loop_line,
    wall_curate_seed, ImagineMediaClick, LearnedSuggestion, SuggestionKind, WallGif, WallSlot,
    IMAGE_FILE_CAP,
};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
use std::sync::Arc;

pub use grokhub_core::ImagineKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuggestedAuto {
    pub icon: TileIcon,
    pub title: &'static str,
    pub body: &'static str,
    pub seed: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileHit {
    None,
    Add,
    Body,
}

pub const SUGGESTED_AUTOS: &[SuggestedAuto] = &[
    SuggestedAuto {
        icon: TileIcon::Sun,
        title: "Morning brief",
        body: "Every 1d — workboard and last host receipt.",
        seed: "/loop 1d summarize the workboard and last host receipt",
    },
    SuggestedAuto {
        icon: TileIcon::Bolt,
        title: "Host heartbeat",
        body: "Every 60m — read-only snapshot, then a short note.",
        seed: "/loop 60m run a read-only host snapshot and summarize",
    },
    SuggestedAuto {
        icon: TileIcon::List,
        title: "Task extractor",
        body: "Every 1d — pull today's open tasks onto the board.",
        seed: "/loop 1d extract open tasks onto the workboard",
    },
    SuggestedAuto {
        icon: TileIcon::Host,
        title: "Dawn snapshot",
        body: "Every 1d — read-only host snapshot.",
        seed: "/loop 1d run a read-only host snapshot and summarize",
    },
    SuggestedAuto {
        icon: TileIcon::Board,
        title: "Midday board",
        body: "Every 12h — summarize the workboard.",
        seed: "/loop 12h summarize the workboard",
    },
    SuggestedAuto {
        icon: TileIcon::Moon,
        title: "Nightly triage",
        body: "Every 1d — extract leftover tasks onto the board.",
        seed: "/loop 1d extract leftover tasks onto the workboard",
    },
    SuggestedAuto {
        icon: TileIcon::Host,
        title: "Replay last desktop run",
        body: "Every 1d — ask Grok to repeat the last desktop task.",
        seed: "/loop 1d replay the last desktop task",
    },
];

/// grok.com/imagine rotating h1 noun — cabin-real only.
pub const IMAGINE_WORDS: &[&str] = &["the cabin", "the night", "a scene", "the board"];

/// Still-image seeds. grok-imagine-image-2.0 only — no photo-edit tools we do not have.
/// `frames` cycle like grok.com/imagine cover GIFs — inspiration, not generated output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImagineScene {
    pub icon: TileIcon,
    pub title: &'static str,
    pub prompt: &'static str,
    pub tall: bool,
    pub frames: &'static [&'static str],
}

pub const IMAGINE_SCENES: &[ImagineScene] = &[
    ImagineScene {
        icon: TileIcon::Moon,
        title: "Night cabin",
        prompt: "still photograph of a dark timber cabin at night, one warm window, no people, no text",
        tall: false,
        frames: &["night_cabin", "night_cabin_b"],
    },
    ImagineScene {
        icon: TileIcon::Board,
        title: "Bound project",
        prompt: "still of a wooden workbench with a closed laptop and a bound notebook, dim cabin light, no people, no text",
        tall: true,
        frames: &["bound_project", "bound_project_b"],
    },
    ImagineScene {
        icon: TileIcon::Host,
        title: "Host desk",
        prompt: "still of a Linux workstation desk, dark room, monitor glow, no people, no faces, no text",
        tall: true,
        frames: &["host_desk", "host_desk_b"],
    },
    ImagineScene {
        icon: TileIcon::List,
        title: "Workboard still",
        prompt: "still of a wall of blank paper task cards in a dark cabin, warm lamp, no people, no readable text",
        tall: false,
        frames: &["workboard", "workboard_b"],
    },
    ImagineScene {
        icon: TileIcon::Sun,
        title: "Morning window",
        prompt: "still of a cabin window at dawn, frost on glass, empty room, no people, no text",
        tall: true,
        frames: &["morning_window", "morning_window_b"],
    },
    ImagineScene {
        icon: TileIcon::Image,
        title: "A scene",
        prompt: "tight still-image of an empty cabin room at night, one lamp, wood walls, no people, no text",
        tall: false,
        frames: &["a_scene", "a_scene_b"],
    },
    ImagineScene {
        icon: TileIcon::Moon,
        title: "Wood stove",
        prompt: "still of a wood stove in a dark timber cabin, embers, no people, no text",
        tall: true,
        frames: &["wood_stove", "wood_stove_b"],
    },
    ImagineScene {
        icon: TileIcon::Moon,
        title: "Pine ridge",
        prompt: "still of a pine ridge at night above a dark valley, no people, no text",
        tall: false,
        frames: &["pine_ridge", "pine_ridge_b"],
    },
    ImagineScene {
        icon: TileIcon::Sun,
        title: "Empty chair",
        prompt: "still of an empty wooden chair by a cabin window at night, one lamp, no people, no text",
        tall: true,
        frames: &["empty_chair", "empty_chair_b"],
    },
];

pub fn imagine_kind_label(kind: ImagineKind) -> &'static str {
    match kind {
        ImagineKind::Image => "Image",
        ImagineKind::Video => "Video",
        ImagineKind::Agent => "Agent",
    }
}

pub fn imagine_quality_label(quality: bool) -> &'static str {
    if quality {
        "Quality (v2.0)"
    } else {
        "Speed"
    }
}

/// Send + mic sit in a reserved right column so they never cover chips.
pub fn imagine_send_cluster_w() -> f32 {
    crate::theme::IMAGINE_HIT * 2.0 + 12.0
}

/// Mic + Send/Stop. Session and permission sit above the bar.
pub fn composer_go_cluster_w() -> f32 {
    22.0 + 28.0 + 8.0 * 3.0 + 12.0
}

/// Filled Send/Stop disc. Always reserve this, even when Idle is a 22px arrow.
pub fn composer_go_hit_w() -> f32 {
    28.0
}

/// Text + Fast + mic after Plus. Leaves the Stop disc and the two 8px gaps.
pub fn composer_mid_w(inner: f32) -> f32 {
    (inner - 22.0 - 8.0 - composer_go_hit_w() - 8.0).max(80.0)
}

/// Visible pill width from the window, not egui available (chips/wordmark
/// inflate that past the pane so Stop paints off-screen). Caps at the
/// official Grok conversation column so ultrawide stays a centered chat.
pub fn composer_pill_w(screen_w: f32) -> f32 {
    (screen_w - crate::theme::SIDEBAR_W - 40.0).clamp(360.0, crate::theme::CHAT_COL_W)
}

/// Prompt field is a fixed strip. A stretching `TextEdit` covers the chips
/// and steals their clicks (I-beam over 720p / Video audio).
pub fn imagine_prompt_h() -> f32 {
    32.0
}

/// Gap between the pinned prompt strip and the selector chip row.
pub fn imagine_prompt_chip_gap() -> f32 {
    8.0
}

/// Two wrapping chip rows (hit + row gap).
pub fn imagine_chip_stack_h() -> f32 {
    (crate::theme::IMAGINE_HIT + 8.0) * 2.0
}

/// Dark track + selected chip — grok.com Image|Video|Agent and Speed|Quality.
pub fn imagine_seg_track(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(crate::theme::bg())
        .corner_radius(crate::theme::IMAGINE_HIT)
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.set_height(crate::theme::IMAGINE_HIT);
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal_centered(add);
        });
}

pub fn imagine_seg_chip(
    ui: &mut egui::Ui,
    selected: bool,
    add: impl FnOnce(&mut egui::Ui),
) -> bool {
    let fill = if selected {
        crate::theme::panel()
    } else {
        Color32::TRANSPARENT
    };
    let resp = egui::Frame::NONE
        .fill(fill)
        .corner_radius(crate::theme::IMAGINE_HIT)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(
                crate::theme::IMAGINE_HIT - 8.0,
                crate::theme::IMAGINE_HIT - 8.0,
            ));
            ui.set_height(crate::theme::IMAGINE_HIT - 8.0);
            ui.horizontal_centered(add);
        })
        .response
        .interact(Sense::click());
    crate::motion::icon::note_card(ui, resp.rect);
    let (resp, felt, wash) = crate::theme::feel_button(ui, resp, Color32::TRANSPARENT);
    if wash.a() > 0 {
        ui.painter()
            .rect_filled(felt, crate::theme::IMAGINE_HIT, wash);
    }
    resp.clicked()
}

/// Current cover, next cover, and 0..1 crossfade into the next still.
pub fn imagine_frame_pair(scene: &ImagineScene, now_ms: u64) -> (&'static str, &'static str, f32) {
    let n = scene.frames.len().max(1);
    let tick = (now_ms / crate::theme::IMAGINE_FRAME_MS) as usize + scene.title.len();
    let a = scene.frames[tick % n];
    let b = scene.frames[(tick + 1) % n];
    if n == 1 {
        return (a, a, 0.0);
    }
    let t =
        (now_ms % crate::theme::IMAGINE_FRAME_MS) as f32 / crate::theme::IMAGINE_FRAME_MS as f32;
    let fade = ((t - 0.72) / 0.28).clamp(0.0, 1.0);
    (a, b, fade)
}

pub fn imagine_word(now_ms: u64) -> &'static str {
    IMAGINE_WORDS[((now_ms / 2800) as usize) % IMAGINE_WORDS.len()]
}

fn auto_seen_keys(title: &str, seed: &str) -> Vec<String> {
    let mut keys = vec![title.trim().to_ascii_lowercase()];
    let seed = seed.trim();
    if !seed.is_empty() {
        keys.push(seed.to_ascii_lowercase());
        if let Some((_, prompt)) = parse_loop_line(seed) {
            keys.push(prompt.to_ascii_lowercase());
        }
    }
    keys.retain(|k| !k.is_empty());
    keys
}

fn autos_already_active(seen: &[String], keys: &[String]) -> bool {
    keys.iter().any(|k| seen.iter().any(|s| s == k))
}

/// Learned automations first, then static seeds not already active.
pub fn merge_suggested_autos(
    learned: &[LearnedSuggestion],
    active_names: &[String],
) -> Vec<(icons::TileIcon, String, String, String)> {
    let mut seen: Vec<String> = active_names
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let mut out = Vec::new();
    for s in learned {
        if s.kind != SuggestionKind::Auto {
            continue;
        }
        let seed = s.seed.clone().unwrap_or_default();
        let keys = auto_seen_keys(&s.title, &seed);
        if autos_already_active(&seen, &keys) {
            continue;
        }
        seen.extend(keys);
        out.push((
            icons::icon_for_label(&s.title),
            s.title.clone(),
            s.body.clone(),
            seed,
        ));
    }
    for s in SUGGESTED_AUTOS {
        let keys = auto_seen_keys(s.title, s.seed);
        if autos_already_active(&seen, &keys) {
            continue;
        }
        seen.extend(keys);
        out.push((s.icon, s.title.into(), s.body.into(), s.seed.into()));
    }
    out
}

/// Built-in read-only GitHub tiles. Who am I / List repos — no writes.
pub const GITHUB_TILES: &[(&str, &str, &str)] = &[
    (
        "Who am I",
        "Authenticated login and public repo count.",
        "user",
    ),
    (
        "List repos",
        "Twenty most recently updated repositories.",
        "list_repos",
    ),
];

pub fn page_header(ui: &mut egui::Ui, title: &str, action: &str) -> bool {
    let mut clicked = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .font(crate::theme::title_font(28.0))
                .color(crate::theme::fg()),
        );
        if !action.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = white_pill(ui, action);
            });
        }
    });
    ui.add_space(14.0);
    clicked
}

pub fn white_pill(ui: &mut egui::Ui, label: &str) -> bool {
    felt_pill(ui, label, PillStyle::Solid)
}

pub fn ghost_pill(ui: &mut egui::Ui, label: &str) -> bool {
    felt_pill(ui, label, PillStyle::Ghost)
}

pub fn danger_pill(ui: &mut egui::Ui, label: &str) -> bool {
    felt_pill(ui, label, PillStyle::Danger)
}

/// "···" with no frame until hovered, like the rest of the quiet chrome.
/// The dots brighten while their row is hovered or focused, and the frame
/// stays while the menu is open. The UI font has no "⋯" glyph.
pub fn dots_menu(ui: &mut egui::Ui, bright: bool, add: impl FnOnce(&mut egui::Ui)) {
    ui.scope(|ui| {
        let w = &mut ui.style_mut().visuals.widgets;
        w.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
        w.inactive.bg_stroke = egui::Stroke::NONE;
        let color = if bright {
            crate::theme::fg()
        } else {
            crate::theme::muted()
        };
        ui.menu_button(RichText::new("···").size(16.0).strong().color(color), add);
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillStyle {
    Solid,
    Ghost,
    Danger,
}

pub fn felt_pill(ui: &mut egui::Ui, label: &str, style: PillStyle) -> bool {
    felt_pill_at(ui, None, label, style).clicked()
}

/// A pill with an optional fixed id. The caller picks `clicked()` (keys
/// too) or `clicked_by(Primary)` (a pointer click only, for hard actions).
pub fn felt_pill_at(
    ui: &mut egui::Ui,
    id: Option<egui::Id>,
    label: &str,
    style: PillStyle,
) -> egui::Response {
    // Loadout themes paint the slide 07 button: red outline, amber primary.
    if crate::theme::is_loadout_paint() && style != PillStyle::Danger {
        let solid = style == PillStyle::Solid;
        return crate::ui_kit::recipe::button(ui, id, label, solid, egui::vec2(0.0, 28.0), solid);
    }
    let (base_fill, text_color, rounding, min_size, stroke, strong) = match style {
        PillStyle::Solid => (
            crate::theme::fg(),
            crate::theme::bg(),
            8.0,
            egui::vec2(0.0, 28.0),
            None,
            true,
        ),
        PillStyle::Ghost => (
            Color32::TRANSPARENT,
            crate::theme::muted(),
            8.0,
            egui::vec2(0.0, 28.0),
            Some(Stroke::new(1.0_f32, crate::theme::border())),
            false,
        ),
        PillStyle::Danger => (
            crate::theme::offline(),
            crate::theme::bg(),
            8.0,
            egui::vec2(0.0, 28.0),
            None,
            true,
        ),
    };
    let resp = crate::theme::felt_label_button_with(
        ui, id, label, base_fill, text_color, rounding, min_size, stroke, strong,
    );
    let enabled = resp.enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    resp
}

/// Horizontal inset so a long session label is not jammed on the pill edge.
pub const SEG_INSET_X: f32 = 8.0;
/// Vertical inset for session / permission pills.
pub const SEG_INSET_Y: f32 = 6.0;

/// Session Chat / Plan / btw — quieter than the permission row (no stroke).
pub fn felt_segment(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    felt_segment_styled(ui, label, selected, None, false, crate::theme::FONT_BODY)
}

/// Idle Always matches Ask/Auto. Selected Always settles in 140ms: fill α 0→1, scale 0.98→1.0, 2px white ring — then stops.
pub fn permission_risk_stroke_w(id: &str, selected: bool) -> f32 {
    if selected && id == "always-approve" {
        2.0
    } else {
        1.0
    }
}

pub fn permission_risk_stroke_color(id: &str, selected: bool) -> Color32 {
    if selected && id == "always-approve" {
        crate::theme::always_amber()
    } else if selected {
        crate::theme::border_strong()
    } else {
        crate::theme::border()
    }
}

pub fn permission_risk_strong(id: &str, selected: bool) -> bool {
    selected && id == "always-approve"
}

pub fn permission_risk_fill(selected: bool) -> Color32 {
    if selected {
        crate::theme::nav_active()
    } else {
        Color32::TRANSPARENT
    }
}

pub fn felt_perm_segment(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    id: &str,
) -> egui::Response {
    let stroke = Stroke::new(
        permission_risk_stroke_w(id, selected),
        permission_risk_stroke_color(id, selected),
    );
    felt_segment_styled(
        ui,
        label,
        selected,
        Some(stroke),
        permission_risk_strong(id, selected),
        crate::theme::FONT_CHROME,
    )
}

fn felt_segment_styled(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    stroke: Option<Stroke>,
    strong: bool,
    font_size: f32,
) -> egui::Response {
    let font = if strong {
        crate::theme::title_font(font_size)
    } else {
        FontId::proportional(font_size)
    };
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER));
    let size = egui::vec2(
        (galley.size().x + SEG_INSET_X * 2.0).max(52.0),
        (galley.size().y + SEG_INSET_Y * 2.0).max(28.0),
    );
    let (_id_rect, resp) = ui.allocate_exact_size(size, Sense::click());
    // Always settles in 140ms (fill α + scale); other segments keep SELECT_SECS.
    let settle = if strong {
        grokhub_core::ALWAYS_SETTLE_SECS
    } else {
        grokhub_core::SELECT_SECS
    };
    let on_t = crate::theme::animate_selection_secs(ui, resp.id.with("seg"), selected, settle);
    let base_fill =
        crate::theme::blend_color(Color32::TRANSPARENT, permission_risk_fill(true), on_t);
    let text_color = crate::theme::blend_color(crate::theme::muted(), crate::theme::fg(), on_t);
    let (resp, rect, _) = crate::theme::feel_response(ui, resp, Color32::TRANSPARENT);
    // Always: scale 0.98 → 1.0 as it settles (no perpetual pulse).
    let scale = if strong { 0.98 + 0.02 * on_t } else { 1.0 };
    let (sx, sy, sw, sh) =
        grokhub_core::felt_rect(rect.min.x, rect.min.y, rect.width(), rect.height(), scale);
    let draw = egui::Rect::from_min_size(egui::pos2(sx, sy), egui::vec2(sw, sh));
    if base_fill.a() > 0 {
        ui.painter().rect_filled(draw, 8.0, base_fill);
    }
    if let Some(stroke) = stroke {
        let idle = Stroke::new(1.0_f32, crate::theme::border());
        let w = idle.width + (stroke.width - idle.width) * on_t;
        let c = crate::theme::blend_color(idle.color, stroke.color, on_t);
        ui.painter()
            .rect_stroke(draw, 8.0, Stroke::new(w, c), egui::StrokeKind::Middle);
    } else {
        ui.painter().rect_stroke(
            draw,
            8.0,
            Stroke::new(1.0_f32, crate::theme::border()),
            egui::StrokeKind::Middle,
        );
    }
    ui.painter().galley(
        egui::pos2(
            draw.center().x - galley.size().x * 0.5,
            draw.center().y - galley.size().y * 0.5,
        ),
        galley,
        text_color,
    );
    let enabled = resp.enabled();
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, label)
    });
    resp
}

/// Catalog / settings tab with animated active fill.
pub fn felt_tab(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let font = FontId::proportional(13.0);
    // PLACEHOLDER lets the painter pick the colour below. Baking one in here painted the
    // selected tab's label in fg() on top of an fg() pill — a white label on white.
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER));
    let pad = ui.style().spacing.button_padding;
    let size = egui::vec2((galley.size().x + pad.x * 2.0).max(32.0), 32.0);
    let (_rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let on_t = crate::theme::animate_selection(ui, resp.id.with("tab"), active);
    if crate::theme::is_loadout_paint() {
        use crate::ui_kit::recipe::{look, paint, state, Part};
        let (resp, rect, _) = crate::theme::feel_button(ui, resp, Color32::TRANSPARENT);
        let look = look(Part::Tab, state(resp.hovered(), active, false));
        paint(ui.painter(), rect, &look);
        ui.painter().galley(
            egui::pos2(rect.min.x + pad.x, rect.center().y - galley.size().y * 0.5),
            galley,
            look.ink,
        );
        let enabled = resp.enabled();
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, label)
        });
        return resp.clicked();
    }
    let base_fill = crate::theme::blend_color(Color32::TRANSPARENT, crate::theme::fg(), on_t);
    let text_color = crate::theme::blend_color(crate::theme::muted(), crate::theme::bg(), on_t);
    let stroke_color = crate::theme::blend_color(crate::theme::border(), crate::theme::fg(), on_t);
    let (resp, rect, fill) = crate::theme::feel_button(ui, resp, base_fill);
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0_f32, stroke_color),
        egui::StrokeKind::Middle,
    );
    ui.painter().galley(
        egui::pos2(rect.min.x + pad.x, rect.center().y - galley.size().y * 0.5),
        galley,
        text_color,
    );
    let enabled = resp.enabled();
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, label)
    });
    resp.clicked()
}

pub fn felt_menu_row(ui: &mut egui::Ui, label: &str) -> bool {
    crate::theme::felt_label_button(
        ui,
        label,
        Color32::TRANSPARENT,
        crate::theme::fg(),
        8.0,
        egui::vec2(204.0, 36.0),
        None,
        false,
    )
    .clicked()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipTone {
    Live,
    Setup,
    Offline,
    Mute,
}

pub fn chip_tone_color(tone: ChipTone) -> Color32 {
    match tone {
        ChipTone::Live => crate::theme::live(),
        ChipTone::Setup => crate::theme::setup(),
        ChipTone::Offline => crate::theme::offline(),
        ChipTone::Mute => crate::theme::muted(),
    }
}

pub fn status_chip(ui: &mut egui::Ui, label: &str, tone: ChipTone) {
    let color = chip_tone_color(tone);
    card_frame(ui, CardVariant::Outlined, true, |ui| {
        ui.label(
            RichText::new(label)
                .size(crate::theme::TYPE_CAPTION)
                .color(color),
        );
    });
}

/// Glanceable in-progress chrome: pulsing live dot + phase label.
/// Hover shows the current action. Halt stays on the composer disc.
/// Not a blinking caret.
pub fn paint_run_pulse(ui: &mut egui::Ui, label: &str, hint: &str) {
    if label.is_empty() {
        return;
    }
    let t = ui.ctx().input(|i| i.time) as f32;
    let pulse = chat_run_dot_alpha(t);
    let fill = crate::theme::live();
    let color =
        Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), (pulse * 255.0) as u8);
    ui.add_space(6.0);
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, color);
        ui.label(
            RichText::new(label)
                .size(crate::theme::FONT_META)
                .color(crate::theme::muted()),
        );
    });
    if !hint.is_empty() {
        row.response.on_hover_text(hint);
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
}

pub fn titlebar_update_chip(ui: &mut egui::Ui, label: &str) -> bool {
    // Fixed height: a Frame in the centred titlebar row stretched to the full 40px bar.
    let galley = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            label.to_owned(),
            FontId::proportional(12.0),
            Color32::PLACEHOLDER,
        )
    });
    let size = egui::vec2(galley.size().x + 16.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.hovered() {
        crate::theme::surface_hover()
    } else {
        crate::theme::elevated()
    };
    ui.painter().rect_filled(rect, 10.0, fill);
    ui.painter().rect_stroke(
        rect,
        10.0,
        Stroke::new(1.0_f32, crate::theme::border()),
        egui::StrokeKind::Middle,
    );
    ui.painter().galley(
        egui::pos2(rect.min.x + 8.0, rect.center().y - galley.size().y * 0.5),
        galley,
        crate::theme::live(),
    );
    crate::theme::pointing(resp).clicked()
}

/// Returns the image rect so a marker can sit on the frame.
pub fn framed_preview(
    ui: &mut egui::Ui,
    tex: &TextureHandle,
    size: [usize; 2],
    max_w: f32,
) -> egui::Rect {
    let scale = max_w / size[0].max(1) as f32;
    let h = size[1] as f32 * scale;
    let mut image_rect = egui::Rect::NOTHING;
    card_frame(ui, CardVariant::Outlined, true, |ui| {
        image_rect = ui
            .add(egui::Image::new((tex.id(), egui::vec2(max_w, h))).corner_radius(10.0))
            .rect;
    });
    image_rect
}

pub fn composer_modes() -> &'static [(&'static str, &'static str)] {
    &[
        ("chat", "Chat"),
        ("plan", "Plan"),
        ("ask", grokhub_core::BTW_LABEL),
    ]
}

pub fn permission_modes() -> &'static [(&'static str, &'static str)] {
    &[
        ("ask", "Ask"),
        ("auto", "Auto"),
        ("always-approve", "Always"),
    ]
}

/// Hover copy for the Chat / Plan / btw session pills. Unknown ids stay silent.
pub fn composer_session_tip(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "chat" => Some((
            "Chat",
            "Normal chat. Grok can edit files and use tools in the bound project. Everyday work.",
        )),
        "plan" => Some((
            "Plan",
            "Grok writes a plan before changing things. Use this for bigger or riskier work. /plan is the same.",
        )),
        "ask" => Some((grokhub_core::BTW_TIP_TITLE, grokhub_core::BTW_TIP_BODY)),
        _ => None,
    }
}

/// Hover copy for the Ask / Auto / Always permission pills. Unknown ids stay silent.
pub fn composer_perm_tip(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "ask" => Some((
            "Ask",
            "You approve each tool. Allow / Deny shows in chat. If ACP is down the turn is denied.",
        )),
        "auto" => Some((
            "Auto",
            "Safe tools run on their own. Use this when you trust the turn. /auto is the same.",
        )),
        "always-approve" => Some((
            "Always",
            "Skip every tool prompt except money, sending, deleting, credentials and irreversible changes. Stays until Ask or Auto. /always-approve.",
        )),
        _ => None,
    }
}

fn show_composer_tip(ui: &mut egui::Ui, title: &str, body: &str) {
    ui.set_max_width(240.0);
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.visuals_mut().window_fill = crate::theme::hover();
    ui.visuals_mut().widgets.noninteractive.bg_fill = crate::theme::hover();
    ui.label(
        RichText::new(title)
            .size(crate::theme::FONT_BODY)
            .strong()
            .color(crate::theme::fg()),
    );
    ui.label(
        RichText::new(body)
            .size(crate::theme::FONT_TIP)
            .color(crate::theme::muted()),
    );
}

fn with_composer_tip(resp: egui::Response, title: &str, body: &str) -> egui::Response {
    resp.on_hover_ui_at_pointer(|ui| show_composer_tip(ui, title, body))
}

/// Title of the Auto chip's hover card.
pub const AUTO_CHIP_TIP: &str = "Auto effort";

/// "Auto · Medium": the word Auto, then the current rung in small text.
pub fn auto_chip_job(rung: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let fmt = |size: f32, color: Color32| TextFormat {
        font_id: FontId::proportional(size),
        color,
        valign: egui::Align::Center,
        ..Default::default()
    };
    job.append(
        "Auto",
        0.0,
        fmt(crate::theme::FONT_CHROME, crate::theme::fg()),
    );
    job.append(
        &format!(" · {rung}"),
        0.0,
        fmt(crate::theme::FONT_META, crate::theme::muted()),
    );
    job
}

/// The read-only Auto chip where the effort pill was. Hover shows the last
/// reason; a click asks for `/why`. No dropdown, no pulse. True when clicked.
pub fn auto_chip(ui: &mut egui::Ui, rung: &str, reason: &str) -> bool {
    let galley = ui.fonts_mut(|f| f.layout_job(auto_chip_job(rung)));
    let size = egui::vec2(galley.size().x + 24.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let stroke = if resp.hovered() {
        crate::theme::fg()
    } else {
        crate::theme::border()
    };
    ui.painter().rect_stroke(
        rect,
        14.0,
        Stroke::new(1.0_f32, stroke),
        egui::StrokeKind::Inside,
    );
    let at = egui::pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, crate::theme::fg());
    let name = format!("Auto effort, {rung}");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    let resp = with_composer_tip(
        resp.on_hover_cursor(egui::CursorIcon::PointingHand),
        AUTO_CHIP_TIP,
        reason,
    );
    resp.clicked()
}

pub struct SessionRowOut {
    pub mode: Option<String>,
    pub perm: Option<String>,
    /// The Auto chip was clicked: show `/why`.
    pub why: bool,
}

/// Chat / Plan / btw, Ask / Auto / Always, and the read-only Auto effort chip above the composer.
/// `auto` is the chip's rung label and its hover reason.
pub fn session_row(ui: &mut egui::Ui, mode: &str, perm: &str, auto: (&str, &str)) -> SessionRowOut {
    let mut out = SessionRowOut {
        mode: None,
        perm: None,
        why: false,
    };
    // Wrapped so a narrow pane drops the Auto chip to a second line instead of clipping it.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        crate::theme::glide_paint(ui, "session-mode");
        let mut mode_hover = None;
        let mut mode_on = None;
        for (id, label) in composer_modes() {
            let on = *id == mode;
            let mut resp = felt_segment(ui, label, on);
            if resp.hovered() {
                mode_hover = Some(resp.rect);
            }
            if on {
                mode_on = Some(resp.rect);
            }
            if let Some((title, body)) = composer_session_tip(id) {
                resp = with_composer_tip(resp, title, body);
            }
            if resp.clicked() && !on {
                out.mode = Some((*id).to_string());
            }
        }
        crate::theme::glide_candidate(
            ui,
            "session-mode",
            mode_hover.or(mode_on).unwrap_or(egui::Rect::NOTHING),
            mode_hover.is_some(),
            mode_on.is_some(),
        );
        crate::theme::glide_aim(ui, "session-mode");
        session_row_divider(ui);
        crate::theme::glide_paint(ui, "session-perm");
        let mut perm_hover = None;
        let mut perm_on = None;
        for (id, label) in permission_modes() {
            let on = *id == perm;
            let mut resp = felt_perm_segment(ui, label, on, id);
            if resp.hovered() {
                perm_hover = Some(resp.rect);
            }
            if on {
                perm_on = Some(resp.rect);
            }
            if let Some((title, body)) = composer_perm_tip(id) {
                resp = with_composer_tip(resp, title, body);
            }
            if resp.clicked() && !on {
                out.perm = Some((*id).to_string());
            }
        }
        crate::theme::glide_candidate(
            ui,
            "session-perm",
            perm_hover.or(perm_on).unwrap_or(egui::Rect::NOTHING),
            perm_hover.is_some(),
            perm_on.is_some(),
        );
        crate::theme::glide_aim(ui, "session-perm");
        // The chip drops to its own line as a unit; no divider left dangling at a line end.
        let chip_w = ui.fonts_mut(|f| f.layout_job(auto_chip_job(auto.0)).size().x) + 24.0;
        let gap = ui.spacing().item_spacing.x;
        // Inside horizontal_wrapped, available_width() is the whole row; use the cursor.
        let left_on_line = ui.max_rect().right() - ui.cursor().min.x;
        if left_on_line < SESSION_DIVIDER_W + chip_w + gap * 2.0 + 1.0 {
            ui.end_row();
        } else {
            session_row_divider(ui);
        }
        out.why = auto_chip(ui, auto.0, auto.1);
    });
    ui.add_space(8.0);
    out
}

/// Hairline between the session, permission, and Auto chip groups.
const SESSION_DIVIDER_W: f32 = 9.0;

fn session_row_divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(SESSION_DIVIDER_W, 16.0), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0_f32, crate::theme::border_strong()),
    );
}

/// Live Voice strip: state label + Stop. Caller hides this when idle.
pub fn voice_mode_row(ui: &mut egui::Ui, label: &str) -> bool {
    let mut stop = false;
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
        ui.painter()
            .circle_filled(dot.center(), 3.5, crate::theme::live());
        ui.add_space(4.0);
        ui.label(
            RichText::new(label)
                .size(crate::theme::FONT_META)
                .color(crate::theme::fg()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ghost_pill(ui, "Stop") {
                stop = true;
            }
        });
    });
    ui.add_space(8.0);
    stop
}

#[allow(unused_imports)]
pub use crate::app::chips::row::{
    chip_cluster_lead, chip_cluster_w, chip_dismiss_rect, chip_hit_act, chip_label_budget,
    chip_paint_label, chip_pill_width, chip_row_visible_w, chip_row_width_lock, chip_why_tip,
    fluid_chip_count, layout_chip_label, paint_empty_chip_state, quick_chip_fg, quick_chip_fill,
    quick_chip_row, quick_chip_stroke, quick_chip_stroke_w, quick_chip_strong, ChipRowAct,
    CHIP_DISMISS_GAP, CHIP_DISMISS_SLOT, CHIP_EMPTY_LABEL, CHIP_GAP, CHIP_LABEL_MAX_W, CHIP_PAD_X,
    CHIP_ROW_H,
};

#[cfg(test)]
pub use crate::app::chips::row::CHIP_PAD_Y;

pub fn tab_pill(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    felt_tab(ui, label, active)
}

/// Extra room above a section heading that opens a group of settings rows
/// (SY-10), so the heading reads as a break and not as one more row.
pub const SECTION_HEAD_GAP: f32 = 12.0;

/// A heading that starts a new group of rows: [`SECTION_HEAD_GAP`] above, then
/// the same [`section_label`] (the size stays).
pub fn section_heading(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add_space(SECTION_HEAD_GAP);
    section_label(ui, label)
}

/// Section heading. Brighter and heavier than the muted help line under it.
pub fn section_label(ui: &mut egui::Ui, label: &str) -> bool {
    let hit = ui
        .add(
            egui::Label::new(
                RichText::new(label)
                    .font(crate::theme::title_font(14.0))
                    .color(crate::theme::fg()),
            )
            .sense(egui::Sense::click()),
        )
        .clicked();
    ui.add_space(6.0);
    hit
}

/// Muted help text where `backtick` spans paint as monospace code, not literal backticks.
pub fn inline_code_job(text: &str, size: f32, color: Color32, wrap_width: f32) -> LayoutJob {
    let mut job = LayoutJob {
        wrap: TextWrapping {
            max_width: wrap_width,
            ..Default::default()
        },
        ..Default::default()
    };
    let plain = TextFormat {
        font_id: FontId::proportional(size),
        color,
        ..Default::default()
    };
    let code = TextFormat {
        font_id: FontId::monospace(size),
        color: crate::theme::fg(),
        background: crate::theme::surface(),
        ..Default::default()
    };
    for (i, part) in text.split('`').enumerate() {
        if part.is_empty() {
            continue;
        }
        // Odd segments sat between a pair of backticks. An unpaired tail stays plain.
        let closed = i % 2 == 1 && text.split('`').count() > i + 1;
        job.append(part, 0.0, if closed { code.clone() } else { plain.clone() });
    }
    job
}

/// One muted help line under a section label. Backtick spans render as code.
pub fn help_text(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let job = inline_code_job(
        text,
        crate::theme::FONT_TIP,
        crate::theme::muted(),
        ui.available_width(),
    );
    ui.label(job)
}

pub fn settings_toggle(ui: &mut egui::Ui, title: &str, hint: &str, on: &mut bool) -> bool {
    let mut hit = false;
    ui.horizontal(|ui| {
        let text_w = settings_toggle_text_width(ui.available_width());
        ui.vertical(|ui| {
            // A long hint wraps before the switch instead of running under it.
            ui.set_max_width(text_w);
            ui.add_space(6.0);
            ui.label(RichText::new(title).size(15.0).color(crate::theme::fg()));
            if !hint.is_empty() {
                ui.label(RichText::new(hint).size(12.0).color(crate::theme::muted()));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if settings_switch(ui, *on) {
                *on = !*on;
                hit = true;
            }
        });
    });
    ui.add_space(10.0);
    hit
}

/// Settings switch track width and the gap kept between a toggle's text and its switch.
const SETTINGS_SWITCH_W: f32 = 40.0;
const SETTINGS_TOGGLE_GAP: f32 = 16.0;

/// Text column width for a Settings toggle row: the row minus the switch and a gap.
pub fn settings_toggle_text_width(row: f32) -> f32 {
    (row - SETTINGS_SWITCH_W - SETTINGS_TOGGLE_GAP).max(0.0)
}

pub fn settings_switch(ui: &mut egui::Ui, on: bool) -> bool {
    finish_switch(
        paint_switch(ui, on, egui::vec2(SETTINGS_SWITCH_W, 24.0)),
        on,
    )
}

/// Compact on/off for an Automations job row. About 28×16, tooltip "Enabled".
pub fn enabled_switch(ui: &mut egui::Ui, on: bool) -> bool {
    let resp = paint_switch(ui, on, egui::vec2(28.0, 16.0)).on_hover_text("Enabled");
    finish_switch(resp, on)
}

/// Same track and knob as the Settings switch, scaled from its 24px height.
fn paint_switch(ui: &mut egui::Ui, on: bool, size: egui::Vec2) -> egui::Response {
    let (_rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let on_t = crate::theme::animate_selection(ui, resp.id.with("sw-on"), on);
    let base_fill = crate::theme::blend_color(crate::theme::panel(), crate::theme::fg(), on_t);
    let (resp, rect, fill) = crate::theme::feel_button(ui, resp, base_fill);
    let scale = size.y / 24.0;
    ui.painter().rect_filled(rect, 8.0 * scale, fill);
    if on_t < 0.98 {
        ui.painter().rect_stroke(
            rect,
            rect.height() * 0.5,
            Stroke::new(1.0_f32, crate::theme::border_strong()),
            egui::StrokeKind::Middle,
        );
    }
    let inset = 12.0 * scale;
    let knob_x = grokhub_core::lerp_f32(rect.left() + inset, rect.right() - inset, on_t);
    let knob = crate::theme::blend_color(crate::theme::muted(), crate::theme::bg(), on_t);
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 8.0 * scale, knob);
    resp
}

fn finish_switch(resp: egui::Response, on: bool) -> bool {
    let enabled = resp.enabled();
    let name = switch_access_name(on);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, name));
    resp.clicked()
}

pub fn switch_access_name(on: bool) -> &'static str {
    if on {
        "On"
    } else {
        "Off"
    }
}

pub fn settings_dropdown(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    selected: &str,
    choices: &[String],
) -> Option<usize> {
    let mut picked = None;
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(15.0).color(crate::theme::fg()));
    if !hint.is_empty() {
        ui.label(RichText::new(hint).size(12.0).color(crate::theme::muted()));
    }
    ui.add_space(6.0);
    card_frame(ui, CardVariant::Outlined, true, |ui| {
        egui::ComboBox::from_id_salt(title)
            .selected_text(RichText::new(selected).size(15.0).color(crate::theme::fg()))
            .width(ui.available_width().max(160.0))
            .show_ui(ui, |ui| {
                for (i, label) in choices.iter().enumerate() {
                    if ui
                        .selectable_label(label == selected, label.as_str())
                        .clicked()
                    {
                        picked = Some(i);
                    }
                }
            });
    });
    ui.add_space(10.0);
    picked
}

pub fn settings_field(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    value: &mut String,
    masked: bool,
) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(15.0).color(crate::theme::fg()));
    if !hint.is_empty() {
        ui.label(RichText::new(hint).size(12.0).color(crate::theme::muted()));
    }
    ui.add_space(6.0);
    card_frame(ui, CardVariant::Outlined, true, |ui| {
        let mut edit = egui::TextEdit::singleline(value)
            .desired_width(f32::INFINITY)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2)));
        if masked {
            edit = edit.password(true);
        }
        ui.add(edit);
    });
    ui.add_space(10.0);
}

pub fn get_started_panel(
    ui: &mut egui::Ui,
    pending: Option<&str>,
    error: Option<&str>,
    connect_enabled: bool,
) -> bool {
    let mut connect = false;
    ui.vertical_centered(|ui| {
        ui.add_space(24.0);
        ui.label(
            RichText::new("Get Started")
                .font(crate::theme::title_font(crate::theme::GREET_HERO))
                .color(crate::theme::fg()),
        );
        ui.add_space(12.0);
        ui.label(
            RichText::new(format!("Connect your Super Grok account. {} signs in on its own; the Grok Build CLI is not needed.", grokhub_core::brand::product_name()))
                .size(15.0)
                .color(crate::theme::muted()),
        );
        ui.add_space(20.0);
        if let Some(p) = pending {
            settings_note(ui, p);
        }
        if let Some(e) = error.filter(|s| !s.trim().is_empty()) {
            settings_note(ui, e);
        }
        if connect_enabled {
            connect = white_pill(ui, "Connect Super Grok");
        }
    });
    connect
}

pub fn settings_action(ui: &mut egui::Ui, title: &str, hint: &str, action: &str) -> bool {
    settings_action_styled(ui, title, hint, action, PillStyle::Solid)
}

/// The pill that grants access: an outline, not a fill (SB-08), so the off
/// rows read as the calm default and Allow is not the brightest column on
/// the page. Count only `clicked_by(Primary)` on it: Enter or Space on a
/// focused Allow never grants (rule 4: only the user's click writes a grant).
/// [`settings_grant_row`] does.
fn grant_pill(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let resp = crate::theme::felt_label_button(
        ui,
        label,
        Color32::TRANSPARENT,
        crate::theme::fg(),
        8.0,
        egui::vec2(0.0, 28.0),
        Some(Stroke::new(1.0_f32, crate::theme::border_strong())),
        true,
    );
    let enabled = resp.enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    resp
}

/// SB-07: a row hint that leads with "On" or "Off" paints that word in the
/// foreground colour (the rest stays muted), so a column of rows scans at a
/// glance. Any other hint is all muted. The galley text is the hint as given.
pub fn state_hint_job(hint: &str) -> LayoutJob {
    let lead = ["On", "Off"]
        .into_iter()
        .find(|w| {
            hint.strip_prefix(w)
                .is_some_and(|rest| rest.starts_with([' ', '.']))
        })
        .unwrap_or("");
    let font = FontId::proportional(12.0);
    let mut job = LayoutJob::default();
    if !lead.is_empty() {
        job.append(
            lead,
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: crate::theme::fg(),
                ..Default::default()
            },
        );
    }
    job.append(
        &hint[lead.len()..],
        0.0,
        TextFormat {
            font_id: font,
            color: crate::theme::muted(),
            ..Default::default()
        },
    );
    job
}

/// What a grant row's pill does: Allow (outline, pointer-only) or Revoke
/// (ghost, it only takes access away).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantPill {
    Allow,
    Revoke,
}

/// One Settings grant row: title, a state-led hint ([`state_hint_job`], with
/// `hint_hover` on hover, e.g. a whole folder path), then the pill on the right
/// and `extra` left of it (a folder field, a browser picker), laid out right to
/// left. `extra_line` puts the pill and `extra` on their own line under the
/// hint, for inputs too wide to share the title's line. `locked` = why
/// nothing here can act right now (SB-01): the pill and `extra` take egui's
/// disabled look and show `locked` on hover.
pub fn settings_grant_row(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    hint_hover: Option<&str>,
    pill: GrantPill,
    locked: Option<&str>,
    extra_line: bool,
    extra: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut hit = false;
    let labels = |ui: &mut egui::Ui| {
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(title).size(15.0).color(crate::theme::fg()));
            if !hint.is_empty() {
                let r = ui.label(state_hint_job(hint));
                if let Some(full) = hint_hover {
                    r.on_hover_text(full);
                }
            }
        });
    };
    let pills = |ui: &mut egui::Ui, hit: &mut bool| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(locked.is_none(), |ui| {
                let resp = match pill {
                    GrantPill::Allow => grant_pill(ui, "Allow"),
                    GrantPill::Revoke => ghost_pill_response(ui, "Revoke"),
                };
                *hit = match pill {
                    GrantPill::Allow => resp.clicked_by(egui::PointerButton::Primary),
                    GrantPill::Revoke => resp.clicked(),
                } && locked.is_none();
                if let Some(why) = locked {
                    resp.on_disabled_hover_text(why);
                }
                extra(ui);
            });
        });
    };
    if extra_line {
        labels(ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| pills(ui, &mut hit));
    } else {
        ui.horizontal(|ui| {
            labels(ui);
            pills(ui, &mut hit);
        });
    }
    ui.add_space(10.0);
    hit
}

/// The ghost pill's response, for gated rows.
fn ghost_pill_response(ui: &mut egui::Ui, label: &str) -> egui::Response {
    crate::theme::felt_label_button(
        ui,
        label,
        Color32::TRANSPARENT,
        crate::theme::muted(),
        8.0,
        egui::vec2(0.0, 28.0),
        Some(Stroke::new(1.0_f32, crate::theme::border())),
        false,
    )
}

fn settings_action_styled(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    action: &str,
    style: PillStyle,
) -> bool {
    let mut hit = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(title).size(15.0).color(crate::theme::fg()));
            if !hint.is_empty() {
                ui.label(RichText::new(hint).size(12.0).color(crate::theme::muted()));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            hit = felt_pill(ui, action, style);
        });
    });
    ui.add_space(10.0);
    hit
}

pub fn settings_progress(ui: &mut egui::Ui, pct: u8, fill: Color32) {
    ui.horizontal(|ui| {
        ui.add(
            egui::ProgressBar::new((pct as f32 / 100.0).clamp(0.0, 1.0))
                .desired_width(240.0)
                .desired_height(10.0)
                .fill(fill),
        );
        ui.label(
            RichText::new(format!("{pct}%"))
                .size(13.0)
                .color(crate::theme::fg()),
        );
    });
    ui.add_space(8.0);
}

pub fn settings_nav(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let (_rect, resp) = ui.allocate_exact_size(egui::vec2(188.0, 36.0), Sense::click());
    if crate::theme::is_loadout_paint() {
        use crate::ui_kit::recipe::{look, paint, state, Part, State};
        let (resp, rect, _) = crate::theme::feel_button(ui, resp, Color32::TRANSPARENT);
        let st = state(resp.hovered(), active, false);
        let mut look = look(Part::Nav, st);
        // A text list: the resting tile rim is for icon tiles only.
        if st == State::Default {
            look.stroke = Stroke::NONE;
        }
        paint(ui.painter(), rect, &look);
        ui.painter().text(
            rect.left_center() + egui::vec2(12.0, 0.0),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(crate::theme::FONT_CHROME),
            look.ink,
        );
        return resp.clicked();
    }
    let on_t = crate::theme::animate_selection(ui, resp.id.with("nav"), active);
    let base_fill =
        crate::theme::blend_color(Color32::TRANSPARENT, crate::theme::nav_active(), on_t);
    let text_color = crate::theme::blend_color(crate::theme::muted(), crate::theme::fg(), on_t);
    let (resp, rect, fill) = crate::theme::feel_button(ui, resp, base_fill);
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().text(
        rect.left_center() + egui::vec2(12.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(crate::theme::FONT_CHROME),
        text_color,
    );
    resp.clicked()
}

pub fn settings_note(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).color(crate::theme::muted()));
    ui.add_space(8.0);
}

pub fn appearance_card(ui: &mut egui::Ui, label: &str, selected: bool, preview: Color32) -> bool {
    let (_rect, resp) = ui.allocate_exact_size(egui::vec2(108.0, 96.0), Sense::click_and_drag());
    let (resp, rect) = if crate::theme::is_loadout_paint() {
        use crate::ui_kit::recipe::{look, paint, state, Part};
        let (resp, rect, _) = crate::theme::feel_button(ui, resp, Color32::TRANSPARENT);
        paint(
            ui.painter(),
            rect,
            &look(Part::Card, state(resp.hovered(), selected, false)),
        );
        (resp, rect)
    } else {
        let fill = if selected {
            crate::theme::nav_active()
        } else {
            crate::theme::surface()
        };
        let stroke = if selected {
            crate::theme::fg()
        } else {
            crate::theme::border()
        };
        let (resp, rect, fill) = crate::theme::feel_button(ui, resp, fill);
        ui.painter().rect_filled(rect, 12.0, fill);
        ui.painter().rect_stroke(
            rect,
            12.0,
            Stroke::new(1.0_f32, stroke),
            egui::StrokeKind::Middle,
        );
        (resp, rect)
    };
    let preview_rect =
        egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 10.0), egui::vec2(88.0, 56.0));
    ui.painter().rect_filled(preview_rect, 6.0, preview);
    ui.painter().rect_stroke(
        preview_rect,
        6.0,
        Stroke::new(1.0_f32, crate::theme::fg()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 14.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(13.0),
        crate::theme::fg(),
    );
    resp.clicked() || (resp.contains_pointer() && resp.ctx.input(|i| i.pointer.primary_released()))
}

pub fn search_field(ui: &mut egui::Ui, q: &mut String) {
    search_bar(ui, q, "Search", 180.0);
}

pub fn search_bar(ui: &mut egui::Ui, q: &mut String, hint: &str, width: f32) {
    card_frame(ui, CardVariant::Outlined, true, |ui| {
        ui.horizontal(|ui| {
            icons::paint_bar_icon(ui, icons::BarIcon::Search, 16.0, crate::theme::subtle());
            ui.add(
                egui::TextEdit::singleline(q)
                    .hint_text(crate::theme::hint(hint.to_owned()))
                    .desired_width(width)
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2))),
            );
        });
    });
}

fn paint_slot_card(
    ui: &mut egui::Ui,
    mut prepared: egui::containers::frame::Prepared,
    selected: bool,
    rounding: f32,
) -> egui::Response {
    let resp = prepared.allocate_space(ui).interact(Sense::click());
    let resting = prepared.frame.fill;
    let (resp, slot, veil) = crate::theme::feel_response_in_slot(ui, resp, Color32::TRANSPARENT);
    prepared.frame.fill = crate::theme::veil_over(resting, veil);
    prepared.frame.stroke = Stroke::NONE;
    prepared.paint(ui);
    let (stroke, stroke_w) = if selected {
        (crate::theme::fg(), 1.5_f32)
    } else if resp.hovered() {
        (crate::theme::border_strong(), 1.0_f32)
    } else {
        (crate::theme::border(), 1.0_f32)
    };
    ui.painter().rect_stroke(
        slot.shrink(0.5),
        rounding,
        Stroke::new(stroke_w, stroke),
        egui::StrokeKind::Middle,
    );
    crate::motion::icon::note_card(ui, resp.rect);
    resp
}

/// Description lines reserved on every skill / connector tile.
pub const TILE_BODY_MAX_LINES: usize = 3;

/// One `tile_row` grid row. The previous frame's natural card heights
/// stretch shorter neighbors so the row is even and the action can pin.
#[derive(Clone, Debug, Default)]
struct TileRowPass {
    active: bool,
    cursor: usize,
    fill: f32,
    prev: Vec<f32>,
    next: Vec<f32>,
}

fn tile_pass_id() -> egui::Id {
    egui::Id::new("grokhub-tile-row-pass")
}

fn tile_body_font() -> FontId {
    FontId::proportional(crate::theme::FONT_BODY)
}

fn tile_body_width(wrap_width: f32) -> f32 {
    if wrap_width.is_finite() {
        wrap_width.max(8.0)
    } else {
        4_000.0
    }
}

fn tile_body_galley(
    fonts: &mut egui::epaint::text::FontsView<'_>,
    text: &str,
    width: f32,
) -> std::sync::Arc<egui::epaint::text::Galley> {
    fonts.layout(
        text.to_owned(),
        tile_body_font(),
        Color32::PLACEHOLDER,
        width,
    )
}

fn tile_body_fits(fonts: &mut egui::epaint::text::FontsView<'_>, text: &str, width: f32) -> bool {
    let galley = tile_body_galley(fonts, text, width);
    galley.rows.len() <= TILE_BODY_MAX_LINES && galley.rect.width() <= width + 1.0
}

/// Body copy for a skill/connector tile. At most three wrapped lines.
/// Overflow ends on a word boundary with an ellipsis. A single token wider
/// than the card is the only case that ellipsizes inside the token.
pub fn tile_body_text(
    fonts: &mut egui::epaint::text::FontsView<'_>,
    body: &str,
    wrap_width: f32,
) -> String {
    let t = body.trim();
    if t.is_empty() {
        return String::new();
    }
    let width = tile_body_width(wrap_width);
    if tile_body_fits(fonts, t, width) {
        return t.to_string();
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }
    let mut lo = 0usize;
    let mut hi = words.len();
    let mut best = 0usize;
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        let mut candidate = words[..mid].join(" ");
        candidate.push('…');
        if tile_body_fits(fonts, &candidate, width) {
            best = mid;
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if best > 0 {
        let mut out = words[..best].join(" ");
        out.push('…');
        return out;
    }
    ellipsize_tile_token(fonts, words[0], width)
}

fn ellipsize_tile_token(
    fonts: &mut egui::epaint::text::FontsView<'_>,
    token: &str,
    width: f32,
) -> String {
    let chars: Vec<char> = token.chars().collect();
    let mut lo = 0usize;
    let mut hi = chars.len();
    let mut best = 0usize;
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        let mut candidate: String = chars[..mid].iter().collect();
        candidate.push('…');
        if tile_body_fits(fonts, &candidate, width) {
            best = mid;
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let mut out: String = chars[..best].iter().collect();
    out.push('…');
    out
}

/// Gap above the primary action. `extra` is the slack that pins the action
/// to the bottom once this card stretches up to the tallest neighbor.
fn tile_action_gap(extra: f32) -> f32 {
    8.0 + extra.max(0.0)
}

fn next_tile_row_id(ui: &egui::Ui) -> egui::Id {
    let frame = ui.ctx().cumulative_pass_nr();
    let seq_id = ui.id().with("grokhub-tile-row-seq");
    let (seen, seq) = ui
        .ctx()
        .data(|d| d.get_temp::<(u64, u32)>(seq_id))
        .unwrap_or((u64::MAX, 0));
    let seq = if seen == frame { seq } else { 0 };
    ui.ctx()
        .data_mut(|d| d.insert_temp(seq_id, (frame, seq + 1)));
    ui.id().with(("grokhub-tile-row", seq))
}

fn begin_tile_slot(ctx: &egui::Context) -> Option<(f32, f32)> {
    let mut pass = ctx
        .data(|d| d.get_temp::<TileRowPass>(tile_pass_id()))
        .unwrap_or_default();
    if !pass.active {
        return None;
    }
    let prev = pass.prev.get(pass.cursor).copied().unwrap_or(0.0);
    let fill = pass.fill;
    pass.cursor += 1;
    ctx.data_mut(|d| d.insert_temp(tile_pass_id(), pass));
    Some((prev, fill))
}

fn note_tile_natural(ctx: &egui::Context, natural: f32) {
    let mut pass = ctx
        .data(|d| d.get_temp::<TileRowPass>(tile_pass_id()))
        .unwrap_or_default();
    if !pass.active {
        return;
    }
    pass.next.push(natural);
    ctx.data_mut(|d| d.insert_temp(tile_pass_id(), pass));
}

fn tile_row_slack(prev: f32, fill: f32) -> f32 {
    if prev > 0.0 && fill > prev + 0.5 {
        fill - prev
    } else {
        0.0
    }
}

pub fn grok_tile(
    ui: &mut egui::Ui,
    icon: TileIcon,
    title: &str,
    body: &str,
    add: Option<&str>,
    selected: bool,
) -> TileHit {
    let mut hit = TileHit::None;
    let mut add_clicked = false;
    let mut add_rect = None;
    let mut body_h = 0.0_f32;
    let slot = begin_tile_slot(ui.ctx());
    let extra = slot
        .map(|(prev, fill)| tile_row_slack(prev, fill))
        .unwrap_or(0.0);
    let mut prepared = egui::Frame {
        fill: crate::theme::elevated(),
        stroke: Stroke::NONE,
        shadow: crate::theme::kit_shadow(crate::theme::ShadowLevel::Lift),
        inner_margin: egui::Margin::same(14),
        outer_margin: egui::Margin::ZERO,
        corner_radius: crate::theme::CARD_RADIUS.into(),
    }
    .begin(ui);
    {
        let ui = &mut prepared.content_ui;
        ui.horizontal(|ui| {
            icons::paint_icon(ui, icon, crate::theme::TILE_ICON);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                let text_w = ui.available_width();
                if text_w.is_finite() {
                    ui.set_max_width(text_w);
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .font(crate::theme::card_title_font())
                            .color(crate::theme::fg()),
                    )
                    .wrap(),
                );
                ui.add_space(3.0);
                let body_w = ui.available_width().max(8.0);
                let galley = ui.fonts_mut(|fonts| {
                    let painted = tile_body_text(fonts, body, body_w);
                    tile_body_galley(fonts, &painted, body_w)
                });
                let line_h = ui
                    .fonts_mut(|fonts| fonts.row_height(&tile_body_font()))
                    .max(1.0);
                body_h = line_h * TILE_BODY_MAX_LINES as f32;
                let (body_rect, _) =
                    ui.allocate_exact_size(egui::vec2(body_w, body_h), Sense::hover());
                ui.painter()
                    .galley(body_rect.min, galley, crate::theme::muted());
                if let Some(label) = add {
                    ui.add_space(tile_action_gap(extra));
                    let r = crate::theme::felt_label_button(
                        ui,
                        label,
                        crate::theme::fg(),
                        crate::theme::bg(),
                        crate::theme::HIT,
                        egui::vec2(0.0, crate::theme::HIT),
                        None,
                        true,
                    );
                    add_clicked = r.clicked();
                    add_rect = Some(r.rect);
                } else if extra > 0.0 {
                    ui.add_space(extra);
                }
            });
        });
    }
    let resp = paint_slot_card(ui, prepared, selected, crate::theme::CARD_RADIUS);
    if slot.is_some() {
        note_tile_natural(ui.ctx(), (resp.rect.height() - extra).max(1.0));
    }
    #[cfg(test)]
    push_tile_metric(TileMetric {
        height: resp.rect.height(),
        bottom: resp.rect.bottom(),
        button_bottom: add_rect.map(|r| r.bottom()),
        body_h,
    });
    let click_on_add = add_rect
        .zip(ui.input(|i| i.pointer.interact_pos()))
        .is_some_and(|(r, p)| r.expand(6.0).contains(p));
    if add_clicked || (resp.clicked() && click_on_add) {
        hit = TileHit::Add;
    } else if resp.clicked() {
        hit = TileHit::Body;
    }
    hit
}

pub fn tile_row(ui: &mut egui::Ui, n: usize, mut each: impl FnMut(&mut egui::Ui, usize)) {
    if n == 0 {
        return;
    }
    let w = ui.available_width();
    let spacing = ui.spacing().item_spacing.x;
    if !w.is_finite() || w < 16.0 {
        for i in 0..n {
            each(ui, i);
            ui.add_space(12.0);
        }
        return;
    }
    let cols = if w >= 1100.0 {
        3
    } else if w >= 520.0 {
        2
    } else {
        1
    };
    let col_w = (w - spacing * (cols as f32 - 1.0)) / cols as f32;
    if col_w < 8.0 {
        for i in 0..n {
            each(ui, i);
            ui.add_space(12.0);
        }
        return;
    }
    let rows = n.div_ceil(cols);
    for r in 0..rows {
        let row_id = next_tile_row_id(ui);
        let prev: Vec<f32> = ui.ctx().data(|d| d.get_temp(row_id)).unwrap_or_default();
        let fill = prev.iter().copied().fold(0.0_f32, f32::max);
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                tile_pass_id(),
                TileRowPass {
                    active: true,
                    cursor: 0,
                    fill,
                    prev: prev.clone(),
                    next: Vec::new(),
                },
            );
        });
        ui.columns(cols, |col_uis| {
            for (c, col_ui) in col_uis.iter_mut().enumerate() {
                let i = r * cols + c;
                if i < n {
                    each(col_ui, i);
                }
            }
        });
        let pass = ui
            .ctx()
            .data(|d| d.get_temp::<TileRowPass>(tile_pass_id()))
            .unwrap_or_default();
        ui.ctx().data_mut(|d| {
            d.insert_temp(tile_pass_id(), TileRowPass::default());
            d.insert_temp(row_id, pass.next.clone());
        });
        let changed = pass.next.len() != prev.len()
            || pass
                .next
                .iter()
                .zip(prev.iter())
                .any(|(a, b)| (a - b).abs() > 1.0);
        if changed {
            ui.ctx().request_repaint();
        }
        ui.add_space(14.0);
    }
}

fn still_jpeg(key: &str) -> &'static [u8] {
    match key {
        "night_cabin" => include_bytes!("../assets/imagine/night_cabin.jpg"),
        "night_cabin_b" => include_bytes!("../assets/imagine/night_cabin_b.jpg"),
        "bound_project" => include_bytes!("../assets/imagine/bound_project.jpg"),
        "bound_project_b" => include_bytes!("../assets/imagine/bound_project_b.jpg"),
        "host_desk" => include_bytes!("../assets/imagine/host_desk.jpg"),
        "host_desk_b" => include_bytes!("../assets/imagine/host_desk_b.jpg"),
        "workboard" => include_bytes!("../assets/imagine/workboard.jpg"),
        "workboard_b" => include_bytes!("../assets/imagine/workboard_b.jpg"),
        "morning_window" => include_bytes!("../assets/imagine/morning_window.jpg"),
        "morning_window_b" => include_bytes!("../assets/imagine/morning_window_b.jpg"),
        "a_scene" => include_bytes!("../assets/imagine/a_scene.jpg"),
        "a_scene_b" => include_bytes!("../assets/imagine/a_scene_b.jpg"),
        "wood_stove" => include_bytes!("../assets/imagine/wood_stove.jpg"),
        "wood_stove_b" => include_bytes!("../assets/imagine/wood_stove_b.jpg"),
        "pine_ridge" => include_bytes!("../assets/imagine/pine_ridge.jpg"),
        "pine_ridge_b" => include_bytes!("../assets/imagine/pine_ridge_b.jpg"),
        "empty_chair" => include_bytes!("../assets/imagine/empty_chair.jpg"),
        "empty_chair_b" => include_bytes!("../assets/imagine/empty_chair_b.jpg"),
        other => {
            let _ = other;
            include_bytes!("../assets/imagine/a_scene.jpg")
        }
    }
}

fn imagine_still_rgba(bytes: &[u8]) -> image::RgbaImage {
    image::load_from_memory(bytes)
        .map(|img| img.to_rgba8())
        .unwrap_or_else(|_| {
            image::RgbaImage::from_pixel(1, 1, image::Rgba([0x14, 0x14, 0x14, 0xff]))
        })
}

fn imagine_still_tex(ctx: &egui::Context, key: &str) -> (TextureHandle, [usize; 2]) {
    let id = egui::Id::new(("imagine-still", key));
    if let Some(hit) = ctx.data(|d| d.get_temp::<(TextureHandle, [usize; 2])>(id)) {
        return hit;
    }
    if let Some(rgba) = take_still_rgba(key) {
        let size = [rgba.width() as usize, rgba.height() as usize];
        let tex = ctx.load_texture(
            format!("imagine-still-{key}"),
            ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
            TextureOptions::LINEAR,
        );
        let hit = (tex, size);
        ctx.data_mut(|d| d.insert_temp(id, hit.clone()));
        return hit;
    }
    kick_still_tex(ctx.clone(), key.to_string());
    imagine_disk_pending_tex(ctx)
}

fn take_still_rgba(key: &str) -> Option<image::RgbaImage> {
    let mut g = still_tex_gate().lock().ok()?;
    g.ready.remove(key)
}

fn kick_still_tex(ctx: egui::Context, key: String) {
    {
        let Ok(mut g) = still_tex_gate().lock() else {
            return;
        };
        if g.ready.contains_key(&key) || !g.inflight.insert(key.clone()) {
            return;
        }
    }
    std::thread::spawn(move || {
        let rgba = imagine_still_rgba(still_jpeg(&key));
        if let Ok(mut g) = still_tex_gate().lock() {
            g.inflight.remove(&key);
            g.ready.insert(key, rgba);
        }
        ctx.request_repaint();
    });
}

struct StillTexGate {
    inflight: HashSet<String>,
    ready: HashMap<String, image::RgbaImage>,
}

fn still_tex_gate() -> &'static Mutex<StillTexGate> {
    static G: OnceLock<Mutex<StillTexGate>> = OnceLock::new();
    G.get_or_init(|| {
        Mutex::new(StillTexGate {
            inflight: HashSet::new(),
            ready: HashMap::new(),
        })
    })
}

fn cover_uv(iw: f32, ih: f32, dw: f32, dh: f32) -> egui::Rect {
    let ia = iw / ih.max(1.0);
    let da = dw / dh.max(1.0);
    if ia > da {
        let used = da / ia;
        let pad = (1.0 - used) * 0.5;
        egui::Rect::from_min_max(egui::pos2(pad, 0.0), egui::pos2(1.0 - pad, 1.0))
    } else {
        let used = ia / da;
        let pad = (1.0 - used) * 0.5;
        egui::Rect::from_min_max(egui::pos2(0.0, pad), egui::pos2(1.0, 1.0 - pad))
    }
}

fn tile_h(tall: bool, scale: f32) -> f32 {
    let base = if tall {
        crate::theme::IMAGINE_TILE_TALL
    } else {
        crate::theme::IMAGINE_TILE_SHORT
    };
    base * scale
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImagineStageHit {
    pub expand: bool,
    pub save: bool,
    pub open: bool,
    pub play: bool,
    pub retry: bool,
}

/// Generating or finished still/video above the docked Imagine chat box.
pub fn imagine_stage(
    ui: &mut egui::Ui,
    path: &str,
    working: bool,
    video: bool,
    error: &str,
) -> ImagineStageHit {
    let mut hit = ImagineStageHit::default();
    let wall = ui.max_rect();
    if wall.width() < 8.0 || wall.height() < 8.0 {
        return hit;
    }
    ui.allocate_rect(wall, Sense::hover());
    let r = wall.shrink(1.0);
    let stage_radius = crate::theme::RADIUS_LG;
    ui.painter()
        .rect_filled(r, stage_radius, crate::theme::elevated());
    ui.painter().rect_stroke(
        r,
        stage_radius,
        Stroke::new(1.0_f32, crate::theme::border()),
        egui::StrokeKind::Middle,
    );
    if working {
        let label = if video {
            "Imagining video…"
        } else {
            "Imagining…"
        };
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(crate::theme::FONT_CHROME),
            crate::theme::muted(),
        );
        return hit;
    }
    let fail = error.trim();
    if !fail.is_empty() {
        let bar_h = 40.0;
        let message = egui::Rect::from_min_max(
            r.min,
            egui::pos2(r.right(), (r.bottom() - bar_h).max(r.top() + 8.0)),
        );
        ui.painter().text(
            message.center(),
            Align2::CENTER_CENTER,
            format!("Imagine failed — {fail}"),
            FontId::proportional(crate::theme::FONT_CHROME),
            crate::theme::fg(),
        );
        let bar = egui::Rect::from_min_max(egui::pos2(r.left() + 10.0, message.bottom()), r.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
            ui.horizontal(|ui| {
                if ghost_pill(ui, "Retry") {
                    hit.retry = true;
                }
            });
        });
        return hit;
    }
    if path.is_empty() {
        return hit;
    }
    let bar_h = 40.0;
    let media = egui::Rect::from_min_max(
        r.min,
        egui::pos2(r.right(), (r.bottom() - bar_h).max(r.top() + 8.0)),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(media), |ui| {
        ui.set_clip_rect(media);
        imagine_result_hero(ui, path);
        let resp = ui.interact(media, egui::Id::new("imagine-stage-media"), Sense::click());
        let click = imagine_media_click(path);
        if resp.clicked() {
            match click {
                ImagineMediaClick::Play => hit.play = true,
                ImagineMediaClick::Expand => hit.expand = true,
            }
        }
        resp.on_hover_text(match click {
            ImagineMediaClick::Play => "Play",
            ImagineMediaClick::Expand => "Expand",
        });
    });
    let bar = egui::Rect::from_min_max(egui::pos2(r.left() + 10.0, media.bottom()), r.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
        ui.horizontal(|ui| {
            match imagine_media_click(path) {
                ImagineMediaClick::Play => {
                    if ghost_pill(ui, "Play") {
                        hit.play = true;
                    }
                }
                ImagineMediaClick::Expand => {
                    if ghost_pill(ui, "Expand") {
                        hit.expand = true;
                    }
                }
            }
            if ghost_pill(ui, "Save") {
                hit.save = true;
            }
            if ghost_pill(ui, "Open") {
                hit.open = true;
            }
        });
    });
    hit
}

/// One `IMAGINE:` receipt inside a native image or video tool card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeMediaCard {
    pub path: String,
    pub video: bool,
}

fn native_media_tool(title: &str) -> bool {
    matches!(
        title,
        "image_generate" | "image_edit" | "video_generate" | "video_edit" | "video_extend"
    )
}

/// Receipts from a native media tool. Other titles stay text, even if they mention `IMAGINE:`.
pub fn media_tool_cards(title: &str, detail: &str) -> Option<Vec<NativeMediaCard>> {
    if !native_media_tool(title) {
        return None;
    }
    let mut out = Vec::new();
    for line in detail.lines() {
        let Some(rest) = line.trim().strip_prefix("IMAGINE:") else {
            continue;
        };
        let path = rest.trim();
        if path.is_empty() {
            continue;
        }
        out.push(NativeMediaCard {
            video: grokhub_core::imagine_is_video_path(path),
            path: path.to_string(),
        });
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Letterboxed Imagine still or video inside a tool card. Returns false when the card is not media.
pub fn paint_native_media_card(ui: &mut egui::Ui, title: &str, detail: &str) -> bool {
    let Some(cards) = media_tool_cards(title, detail) else {
        return false;
    };
    for card in cards {
        let width = ui.available_width().clamp(8.0, 360.0);
        ui.allocate_ui(egui::vec2(width, 200.0), |ui| {
            imagine_result_hero(ui, &card.path);
        });
        ui.add_space(4.0);
    }
    true
}

/// Generated still, letterboxed in the Imagine stage under the chat box.
pub fn imagine_result_hero(ui: &mut egui::Ui, path: &str) {
    let wall = ui.max_rect();
    if wall.width() < 8.0 || wall.height() < 8.0 || path.is_empty() {
        return;
    }
    ui.allocate_rect(wall, Sense::hover());
    ui.painter().rect_filled(wall, 0.0, crate::theme::bg());
    if grokhub_core::imagine_is_video_path(path) {
        imagine_video_hero(ui, wall, path);
        return;
    }
    let (tex, size) = imagine_disk_tex(ui.ctx(), path);
    let (x, y, w, h) = imagine_result_fit(
        wall.left(),
        wall.top(),
        wall.width(),
        wall.height(),
        size[0] as f32,
        size[1] as f32,
    );
    if w <= 1.0 || h <= 1.0 {
        return;
    }
    let dest = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
    ui.painter().image(
        tex.id(),
        dest,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn imagine_video_hero(ui: &mut egui::Ui, wall: egui::Rect, path: &str) {
    if let Some(poster) = crate::desktop::video_poster_ready(path) {
        let (tex, size) = imagine_disk_tex(ui.ctx(), &poster);
        let (x, y, w, h) = imagine_result_fit(
            wall.left(),
            wall.top(),
            wall.width(),
            wall.height(),
            size[0] as f32,
            size[1] as f32,
        );
        if w > 1.0 && h > 1.0 {
            let dest = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
            ui.painter().image(
                tex.id(),
                dest,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    } else {
        kick_video_poster(ui.ctx().clone(), path.to_string());
    }
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    let c = wall.center();
    ui.painter()
        .circle_filled(egui::pos2(c.x, c.y - 18.0), 28.0, crate::theme::panel());
    ui.painter().text(
        egui::pos2(c.x, c.y - 18.0),
        Align2::CENTER_CENTER,
        "▶",
        FontId::proportional(22.0),
        crate::theme::fg(),
    );
    ui.painter().text(
        egui::pos2(c.x, c.y + 24.0),
        Align2::CENTER_TOP,
        format!("Video ready · {name}"),
        FontId::proportional(crate::theme::FONT_CHROME),
        crate::theme::fg(),
    );
}

fn kick_video_poster(ctx: egui::Context, path: String) {
    {
        let Ok(mut g) = video_poster_gate().lock() else {
            return;
        };
        if !g.insert(path.clone()) {
            return;
        }
    }
    // Keep the gate after a miss so we do not spawn ffmpeg every frame.
    std::thread::spawn(move || {
        let _ = crate::desktop::ensure_video_poster(&path);
        ctx.request_repaint();
    });
}

fn video_poster_gate() -> &'static Mutex<HashSet<String>> {
    static G: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(HashSet::new()))
}

/// grok.com/imagine masonry: full-bleed stills, 1px gutters, caption over the photo.
/// Generated covers sit in a random seat among the stock stills.
pub fn imagine_masonry(
    ui: &mut egui::Ui,
    selected: &str,
    now_ms: u64,
    gifs: &[WallGif],
    mut on_pick: impl FnMut(String),
) {
    let w = ui.available_width();
    if !w.is_finite() || w < 16.0 {
        return;
    }
    let cols = if w >= 900.0 {
        3
    } else if w >= 420.0 {
        2
    } else {
        1
    };
    let gap = 1.0;
    let col_w = ((w - gap * (cols as f32 - 1.0)) / cols as f32).max(8.0);
    let scale = (col_w / 345.0).clamp(0.62, 1.25);
    let slots = curate_wall(IMAGINE_SCENES.len(), gifs.len(), wall_curate_seed(gifs));
    let heights: Vec<f32> = slots
        .iter()
        .map(|slot| match slot {
            WallSlot::Stock(i) => tile_h(
                IMAGINE_SCENES.get(*i).map(|s| s.tall).unwrap_or(false),
                scale,
            ),
            WallSlot::Gif(i) => tile_h(gifs.get(*i).map(|g| g.tall).unwrap_or(false), scale),
        })
        .collect();
    let mut col_h = vec![0.0_f32; cols];
    for (i, h) in heights.iter().enumerate() {
        let c = i % cols;
        if col_h[c] > 0.0 {
            col_h[c] += gap;
        }
        col_h[c] += *h;
    }
    let total_h = col_h.into_iter().fold(0.0_f32, f32::max);
    let (full, _) = ui.allocate_exact_size(egui::vec2(w, total_h), Sense::hover());
    let mut ys: Vec<f32> = (0..cols).map(|_| full.top()).collect();
    for (i, slot) in slots.iter().enumerate() {
        let c = i % cols;
        let h = heights[i];
        let rect = egui::Rect::from_min_size(
            egui::pos2(full.left() + c as f32 * (col_w + gap), ys[c]),
            egui::vec2(col_w, h),
        );
        match slot {
            WallSlot::Stock(si) => {
                if let Some(scene) = IMAGINE_SCENES.get(*si) {
                    if imagine_photo_tile(ui, scene, selected == scene.prompt, rect, i, now_ms) {
                        on_pick(scene.prompt.to_string());
                    }
                }
            }
            WallSlot::Gif(gi) => {
                if let Some(gif) = gifs.get(*gi) {
                    if imagine_disk_tile(ui, gif, selected == gif.prompt, rect, i, now_ms) {
                        on_pick(gif.prompt.clone());
                    }
                }
            }
        }
        ys[c] += h + gap;
    }
}

fn imagine_photo_tile(
    ui: &mut egui::Ui,
    scene: &ImagineScene,
    selected: bool,
    rect: egui::Rect,
    idx: usize,
    now_ms: u64,
) -> bool {
    let resp = ui.interact(rect, egui::Id::new(("imagine-tile", idx)), Sense::click());
    let (key_a, key_b, fade) = imagine_frame_pair(scene, now_ms);
    let (tex, size) = imagine_still_tex(ui.ctx(), key_a);
    let uv = cover_uv(size[0] as f32, size[1] as f32, rect.width(), rect.height());
    ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
    if fade > 0.02 && key_b != key_a {
        let (tex_b, size_b) = imagine_still_tex(ui.ctx(), key_b);
        let uv_b = cover_uv(
            size_b[0] as f32,
            size_b[1] as f32,
            rect.width(),
            rect.height(),
        );
        let alpha = (fade * 255.0).round().clamp(0.0, 255.0) as u8;
        ui.painter()
            .image(tex_b.id(), rect, uv_b, Color32::from_white_alpha(alpha));
    }
    let fade = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 42.0), rect.max);
    ui.painter()
        .rect_filled(fade, 0.0, Color32::from_black_alpha(140));
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.bottom() - 12.0),
        egui::Align2::LEFT_BOTTOM,
        scene.title,
        egui::FontId::proportional(crate::theme::FONT_CHROME),
        Color32::WHITE,
    );
    wall_slot_feel(ui, resp, selected).clicked()
}

fn imagine_disk_tex(ctx: &egui::Context, path: &str) -> (TextureHandle, [usize; 2]) {
    let id = egui::Id::new(("imagine-disk", path));
    if let Some(hit) = ctx.data(|d| d.get_temp::<(TextureHandle, [usize; 2])>(id)) {
        return hit;
    }
    if let Some(rgba) = take_disk_rgba(path) {
        let size = [rgba.width() as usize, rgba.height() as usize];
        let tex = ctx.load_texture(
            format!("imagine-disk-{path}"),
            ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
            TextureOptions::LINEAR,
        );
        let hit = (tex, size);
        ctx.data_mut(|d| d.insert_temp(id, hit.clone()));
        return hit;
    }
    kick_disk_tex(ctx.clone(), path.to_string());
    imagine_disk_pending_tex(ctx)
}

fn imagine_disk_pending_tex(ctx: &egui::Context) -> (TextureHandle, [usize; 2]) {
    let id = egui::Id::new("imagine-disk-pending");
    if let Some(hit) = ctx.data(|d| d.get_temp::<(TextureHandle, [usize; 2])>(id)) {
        return hit;
    }
    let rgba = image::RgbaImage::new(8, 8);
    let size = [8usize, 8];
    let tex = ctx.load_texture(
        "imagine-disk-pending",
        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        TextureOptions::LINEAR,
    );
    let hit = (tex, size);
    ctx.data_mut(|d| d.insert_temp(id, hit.clone()));
    hit
}

struct DiskTexGate {
    inflight: HashSet<String>,
    ready: HashMap<String, image::RgbaImage>,
}

fn disk_tex_gate() -> &'static Mutex<DiskTexGate> {
    static G: OnceLock<Mutex<DiskTexGate>> = OnceLock::new();
    G.get_or_init(|| {
        Mutex::new(DiskTexGate {
            inflight: HashSet::new(),
            ready: HashMap::new(),
        })
    })
}

fn take_disk_rgba(path: &str) -> Option<image::RgbaImage> {
    let mut g = disk_tex_gate().lock().ok()?;
    g.ready.remove(path)
}

fn kick_disk_tex(ctx: egui::Context, path: String) {
    {
        let Ok(mut g) = disk_tex_gate().lock() else {
            return;
        };
        if g.ready.contains_key(&path) || !g.inflight.insert(path.clone()) {
            return;
        }
    }
    std::thread::spawn(move || {
        let len = std::fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(u64::MAX);
        let img = if len > IMAGE_FILE_CAP {
            None
        } else {
            std::fs::read(&path).ok().and_then(|b| {
                if !crate::desktop::image_pixels_ok_for_bytes(&b) {
                    return None;
                }
                image::load_from_memory(&b).ok()
            })
        }
        .unwrap_or_else(|| image::DynamicImage::new_rgb8(8, 8));
        let rgba = img.to_rgba8();
        if let Ok(mut g) = disk_tex_gate().lock() {
            g.inflight.remove(&path);
            g.ready.insert(path, rgba);
        }
        ctx.request_repaint();
    });
}

fn imagine_disk_tile(
    ui: &mut egui::Ui,
    gif: &WallGif,
    selected: bool,
    rect: egui::Rect,
    idx: usize,
    now_ms: u64,
) -> bool {
    let resp = ui.interact(
        rect,
        egui::Id::new(("imagine-wall", idx, gif.id.as_str())),
        Sense::click(),
    );
    if grokhub_core::imagine_is_video_path(&gif.path_a) {
        ui.painter()
            .rect_filled(rect, 0.0, crate::theme::elevated());
        ui.painter()
            .circle_filled(rect.center(), 22.0, crate::theme::panel());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "▶",
            FontId::proportional(18.0),
            crate::theme::fg(),
        );
        ui.painter().rect_filled(
            egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 42.0), rect.max),
            0.0,
            Color32::from_black_alpha(140),
        );
        ui.painter().text(
            egui::pos2(rect.left() + 12.0, rect.bottom() - 12.0),
            egui::Align2::LEFT_BOTTOM,
            &gif.title,
            egui::FontId::proportional(crate::theme::FONT_CHROME),
            Color32::WHITE,
        );
        return wall_slot_feel(ui, resp, selected).clicked();
    }
    let n = if gif.path_b.is_empty() { 1 } else { 2 };
    let tick = (now_ms / crate::theme::IMAGINE_FRAME_MS) as usize + gif.title.len();
    let path_a = if tick.is_multiple_of(n) {
        gif.path_a.as_str()
    } else {
        gif.path_b.as_str()
    };
    let path_b = if tick.is_multiple_of(n) {
        gif.path_b.as_str()
    } else {
        gif.path_a.as_str()
    };
    let t =
        (now_ms % crate::theme::IMAGINE_FRAME_MS) as f32 / crate::theme::IMAGINE_FRAME_MS as f32;
    let fade = if n == 1 {
        0.0
    } else {
        ((t - 0.72) / 0.28).clamp(0.0, 1.0)
    };
    let (tex, size) = imagine_disk_tex(ui.ctx(), path_a);
    let uv = cover_uv(size[0] as f32, size[1] as f32, rect.width(), rect.height());
    ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
    if fade > 0.02 && path_b != path_a && !path_b.is_empty() {
        let (tex_b, size_b) = imagine_disk_tex(ui.ctx(), path_b);
        let uv_b = cover_uv(
            size_b[0] as f32,
            size_b[1] as f32,
            rect.width(),
            rect.height(),
        );
        let alpha = (fade * 255.0).round().clamp(0.0, 255.0) as u8;
        ui.painter()
            .image(tex_b.id(), rect, uv_b, Color32::from_white_alpha(alpha));
    }
    let fade_bar =
        egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 42.0), rect.max);
    ui.painter()
        .rect_filled(fade_bar, 0.0, Color32::from_black_alpha(140));
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.bottom() - 12.0),
        egui::Align2::LEFT_BOTTOM,
        &gif.title,
        egui::FontId::proportional(crate::theme::FONT_CHROME),
        Color32::WHITE,
    );
    wall_slot_feel(ui, resp, selected).clicked()
}

fn wall_slot_feel(ui: &egui::Ui, resp: egui::Response, selected: bool) -> egui::Response {
    let (resp, slot, _veil) = crate::theme::feel_response_in_slot(ui, resp, Color32::TRANSPARENT);
    if selected || resp.hovered() {
        ui.painter().rect_stroke(
            slot.shrink(0.5),
            0.0,
            Stroke::new(1.0_f32, crate::theme::fg()),
            egui::StrokeKind::Middle,
        );
    }
    resp
}

pub fn empty_prompt_tile(ui: &mut egui::Ui, icon: TileIcon, title: &str, hint: &str) -> bool {
    let mut prepared = egui::Frame {
        fill: crate::theme::elevated(),
        stroke: Stroke::NONE,
        shadow: crate::theme::kit_shadow(crate::theme::ShadowLevel::Lift),
        inner_margin: egui::Margin::same(14),
        outer_margin: egui::Margin::ZERO,
        corner_radius: crate::theme::CARD_RADIUS.into(),
    }
    .begin(ui);
    {
        let ui = &mut prepared.content_ui;
        ui.set_min_height(100.0);
        ui.vertical_centered(|ui| {
            icons::paint_icon(ui, icon, crate::theme::TILE_ICON);
            ui.add_space(8.0);
            ui.label(
                RichText::new(title)
                    .font(crate::theme::card_title_font())
                    .color(crate::theme::fg()),
            );
            ui.add_space(4.0);
            help_text(ui, hint);
        });
    }
    paint_slot_card(ui, prepared, false, crate::theme::CARD_RADIUS).clicked()
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
struct TileMetric {
    height: f32,
    bottom: f32,
    button_bottom: Option<f32>,
    body_h: f32,
}

#[cfg(test)]
thread_local! {
    static TILE_METRICS: std::cell::RefCell<Vec<TileMetric>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn push_tile_metric(metric: TileMetric) {
    TILE_METRICS.with(|v| v.borrow_mut().push(metric));
}

#[cfg(test)]
fn take_tile_metrics() -> Vec<TileMetric> {
    TILE_METRICS.with(|v| std::mem::take(&mut *v.borrow_mut()))
}

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
