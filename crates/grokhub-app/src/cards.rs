//! Grok catalog chrome — huge title, white pills, 3-column icon tiles.

use crate::icons::{self, TileIcon};
use eframe::egui::{self, Align2, Color32, ColorImage, FontId, RichText, Sense, Stroke, TextureHandle, TextureOptions};
use grokhub_core::{
    chat_run_dot_alpha, curate_wall, imagine_media_click, imagine_result_fit, parse_loop_line,
    wall_curate_seed, ImagineMediaClick, LearnedSuggestion, SuggestionKind, WallGif, WallSlot,
    IMAGE_FILE_CAP,
};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

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

/// Mode pill width inside the composer (replaces ComboBox `.width(84)`).
pub const MODE_PILL_W: f32 = 84.0;

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
    egui::Frame::none()
        .fill(crate::theme::bg())
        .rounding(crate::theme::IMAGINE_HIT)
        .inner_margin(egui::Margin::same(2.0))
        .show(ui, |ui| {
            ui.set_height(crate::theme::IMAGINE_HIT);
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal_centered(add);
        });
}

pub fn imagine_seg_chip(ui: &mut egui::Ui, selected: bool, add: impl FnOnce(&mut egui::Ui)) -> bool {
    let fill = if selected {
        crate::theme::panel()
    } else {
        Color32::TRANSPARENT
    };
    let resp = egui::Frame::none()
        .fill(fill)
        .rounding(crate::theme::IMAGINE_HIT)
        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
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
    let (resp, felt, wash) = crate::theme::feel_response(ui, resp, Color32::TRANSPARENT);
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
    let t = (now_ms % crate::theme::IMAGINE_FRAME_MS) as f32 / crate::theme::IMAGINE_FRAME_MS as f32;
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
    let mut seen: Vec<String> = active_names.iter().map(|s| s.to_ascii_lowercase()).collect();
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

/// Nightly Suggested skills, minus names already saved as Cabin skills.
pub fn merge_suggested_skills(
    learned: &[LearnedSuggestion],
    existing_names: &[String],
) -> Vec<(icons::TileIcon, String, String, LearnedSuggestion)> {
    let seen: Vec<String> = existing_names.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut out = Vec::new();
    for s in learned {
        if s.kind != SuggestionKind::Skill {
            continue;
        }
        let key = s
            .name
            .as_deref()
            .unwrap_or(&s.title)
            .to_ascii_lowercase();
        if key.is_empty() || seen.iter().any(|n| n == &key) {
            continue;
        }
        out.push((
            icons::icon_for_label(&s.title),
            s.title.clone(),
            s.body.clone(),
            s.clone(),
        ));
    }
    out
}

/// Built-in read-only GitHub tiles. Who am I / List repos — no writes.
pub const GITHUB_TILES: &[(&str, &str, &str)] = &[
    ("Who am I", "Authenticated login and public repo count.", "user"),
    ("List repos", "Twenty most recently updated repositories.", "list_repos"),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillStyle {
    Solid,
    Ghost,
    Danger,
}

pub fn felt_pill(ui: &mut egui::Ui, label: &str, style: PillStyle) -> bool {
    let (base_fill, text_color, rounding, min_size, stroke, strong) = match style {
        PillStyle::Solid => (
            crate::theme::fg(),
            crate::theme::bg(),
            crate::theme::HIT,
            egui::vec2(0.0, crate::theme::HIT),
            None,
            true,
        ),
        PillStyle::Ghost => (
            Color32::TRANSPARENT,
            crate::theme::muted(),
            14.0,
            egui::vec2(0.0, 0.0),
            Some(Stroke::new(1.0_f32, crate::theme::border())),
            false,
        ),
        PillStyle::Danger => (
            crate::theme::offline(),
            crate::theme::bg(),
            crate::theme::HIT,
            egui::vec2(0.0, crate::theme::HIT),
            None,
            true,
        ),
    };
    crate::theme::felt_label_button(
        ui,
        label,
        base_fill,
        text_color,
        rounding,
        min_size,
        stroke,
        strong,
    )
    .clicked()
}

/// Horizontal inset so a long session label (Questions) is not jammed on the pill edge.
pub const SEG_INSET_X: f32 = 8.0;
/// Vertical inset for session / permission pills.
pub const SEG_INSET_Y: f32 = 6.0;

/// Session Chat / Plan / Questions — quieter than the permission row (no stroke).
pub fn felt_segment(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    felt_segment_styled(ui, label, selected, None, false, crate::theme::FONT_BODY)
}

/// Idle Always matches Ask/Auto. Selected Always = 2px amber stroke, same fill.
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
    let galley = ui.fonts(|f| f.layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER));
    // Questions is longer than Chat / Plan / Ask. Size to the label + 8px inset.
    let size = egui::vec2(
        (galley.size().x + SEG_INSET_X * 2.0).max(52.0),
        (galley.size().y + SEG_INSET_Y * 2.0).max(28.0),
    );
    let (_rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let on_t = crate::theme::animate_selection(ui, resp.id.with("seg"), selected);
    let base_fill =
        crate::theme::blend_color(Color32::TRANSPARENT, permission_risk_fill(true), on_t);
    let text_color = crate::theme::blend_color(crate::theme::muted(), crate::theme::fg(), on_t);
    let (resp, rect, fill) = crate::theme::feel_response(ui, resp, base_fill);
    ui.painter().rect_filled(rect, 14.0, fill);
    if let Some(stroke) = stroke {
        ui.painter().rect_stroke(rect, 14.0, stroke);
    }
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        text_color,
    );
    resp
}

/// Catalog / settings tab with animated active fill.
pub fn felt_tab(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let font = FontId::proportional(13.0);
    // PLACEHOLDER lets the painter pick the colour below. Baking one in here painted the
    // selected tab's label in fg() on top of an fg() pill — a white label on white.
    let galley =
        ui.fonts(|f| f.layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER));
    let pad = ui.style().spacing.button_padding;
    let size = egui::vec2((galley.size().x + pad.x * 2.0).max(32.0), 32.0);
    let (_rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let on_t = crate::theme::animate_selection(ui, resp.id.with("tab"), active);
    let base_fill = crate::theme::blend_color(Color32::TRANSPARENT, crate::theme::fg(), on_t);
    let text_color = crate::theme::blend_color(crate::theme::muted(), crate::theme::bg(), on_t);
    let stroke_color = crate::theme::blend_color(crate::theme::border(), crate::theme::fg(), on_t);
    let (resp, rect, fill) = crate::theme::feel_response(ui, resp, base_fill);
    ui.painter().rect_filled(rect, 18.0, fill);
    ui.painter()
        .rect_stroke(rect, 18.0, Stroke::new(1.0_f32, stroke_color));
    ui.painter()
        .galley(rect.min + pad, galley, text_color);
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
    egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(12.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(12.0).color(color));
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
    let color = Color32::from_rgba_unmultiplied(
        fill.r(),
        fill.g(),
        fill.b(),
        (pulse * 255.0) as u8,
    );
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
    ui.ctx().request_repaint();
}

pub fn titlebar_update_chip(ui: &mut egui::Ui, label: &str) -> bool {
    egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(10.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(label).size(12.0).color(crate::theme::LIVE))
                    .sense(Sense::click()),
            )
            .clicked()
        })
        .inner
}

pub fn framed_preview(ui: &mut egui::Ui, tex: &TextureHandle, size: [usize; 2], max_w: f32) {
    let scale = max_w / size[0].max(1) as f32;
    let h = size[1] as f32 * scale;
    egui::Frame::none()
        .rounding(12.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(2.0))
        .show(ui, |ui| {
            ui.add(
                egui::Image::new((tex.id(), egui::vec2(max_w, h))).rounding(10.0),
            );
        });
}

pub fn composer_modes() -> &'static [(&'static str, &'static str)] {
    &[
        ("chat", "Chat"),
        ("plan", "Plan"),
        ("ask", "Questions"),
    ]
}

pub fn permission_modes() -> &'static [(&'static str, &'static str)] {
    &[
        ("ask", "Ask"),
        ("auto", "Auto"),
        ("always-approve", "Always"),
    ]
}

pub fn effort_modes() -> &'static [(&'static str, &'static str)] {
    grokhub_core::REASONING_EFFORTS
}

pub fn effort_label(id: &str) -> &'static str {
    grokhub_core::effort_label(id)
}

/// Hover copy for the Chat / Plan / Questions session pills. Unknown ids stay silent.
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
        "ask" => Some((
            "Questions",
            "Look-only session. Grok explains without editing. Switch to Chat to do the work.",
        )),
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
            "Skip every tool prompt this launch. Resets to Ask next time. /always-approve.",
        )),
        _ => None,
    }
}

/// Hover copy for the effort dropdown above the composer.
pub fn composer_effort_tip() -> (&'static str, &'static str) {
    (
        "Effort",
        "How hard Grok thinks. Higher is slower and deeper. None through Extra High; /effort sets the same.",
    )
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

fn catalog_pill(
    ui: &mut egui::Ui,
    popup_id: &'static str,
    current: &str,
    items: &[(&'static str, &'static str)],
    label: &str,
    tip: Option<(&'static str, &'static str)>,
) -> Option<String> {
    let mut next = None;
    let id = ui.make_persistent_id(popup_id);
    let mut resp = crate::theme::felt_label_button(
        ui,
        label,
        Color32::TRANSPARENT,
        crate::theme::muted(),
        14.0,
        egui::vec2(MODE_PILL_W, 28.0),
        Some(Stroke::new(1.0_f32, crate::theme::border())),
        false,
    );
    if let Some((title, body)) = tip {
        resp = with_composer_tip(resp, title, body);
    }
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    egui::popup::popup_above_or_below_widget(
        ui,
        id,
        &resp,
        egui::AboveOrBelow::Below,
        egui::popup::PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.set_min_width(MODE_PILL_W);
            for (item_id, item_label) in items {
                let on = *item_id == current;
                if ui.selectable_label(on, *item_label).clicked() {
                    if !on {
                        next = Some((*item_id).to_string());
                    }
                    ui.memory_mut(|m| m.close_popup());
                }
            }
        },
    );
    next
}

/// Grok Build reasoning effort (low / medium / high / xhigh).
pub fn effort_pill(ui: &mut egui::Ui, current: &str) -> Option<String> {
    let id = grokhub_core::parse_reasoning_effort(current).unwrap_or("high");
    catalog_pill(
        ui,
        "composer-effort-pop",
        id,
        effort_modes(),
        effort_label(id),
        Some(composer_effort_tip()),
    )
}

pub struct SessionRowOut {
    pub mode: Option<String>,
    pub perm: Option<String>,
    pub effort: Option<String>,
}

/// Chat / Plan / Questions, Ask / Auto / Always, and reasoning effort above the composer.
pub fn session_row(ui: &mut egui::Ui, mode: &str, perm: &str, effort: &str) -> SessionRowOut {
    let mut out = SessionRowOut {
        mode: None,
        perm: None,
        effort: None,
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (id, label) in composer_modes() {
            let on = *id == mode;
            let mut resp = felt_segment(ui, label, on);
            if let Some((title, body)) = composer_session_tip(id) {
                resp = with_composer_tip(resp, title, body);
            }
            if resp.clicked() && !on {
                out.mode = Some((*id).to_string());
            }
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new("|")
                .size(crate::theme::FONT_TIP)
                .color(crate::theme::subtle()),
        );
        ui.add_space(4.0);
        for (id, label) in permission_modes() {
            let on = *id == perm;
            let mut resp = felt_perm_segment(ui, label, on, id);
            if let Some((title, body)) = composer_perm_tip(id) {
                resp = with_composer_tip(resp, title, body);
            }
            if resp.clicked() && !on {
                out.perm = Some((*id).to_string());
            }
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new("|")
                .size(crate::theme::FONT_TIP)
                .color(crate::theme::subtle()),
        );
        ui.add_space(4.0);
        if let Some(next) = effort_pill(ui, effort) {
            out.effort = Some(next);
        }
    });
    ui.add_space(8.0);
    out
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

pub fn clip_status(text: &str, max_chars: usize) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    if first.chars().count() <= max_chars {
        return first.to_string();
    }
    let take = max_chars.saturating_sub(1);
    format!("{}…", first.chars().take(take).collect::<String>())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipRowAct {
    Apply(usize),
    Dismiss(usize),
}

pub(crate) fn chip_paint_label(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Wrap width for a suggestion chip. Two lines, not a one-line ellipsis.
pub const CHIP_LABEL_WRAP: f32 = 180.0;
/// Pad inside the fill with spaces — Frame inner_margin clips the first glyphs.
pub const CHIP_PAD_X: f32 = 14.0;
pub const CHIP_PAD_Y: f32 = 8.0;

/// Max width of the chip cluster — follows the composer column.
pub fn chip_row_width_lock(avail: f32) -> f32 {
    avail.max(120.0)
}

/// Two-line chip row so leftover empty-home height cannot vertically center the chips.
pub const CHIP_ROW_H: f32 = 68.0;

/// Empty-home placeholder when ranking yields none. Not a ranked action chip.
pub const CHIP_EMPTY_LABEL: &str = "Nothing queued";

pub fn quick_chip_fill(primary: bool) -> Color32 {
    if primary {
        crate::theme::surface_hover()
    } else {
        crate::theme::elevated()
    }
}

pub fn quick_chip_stroke(primary: bool) -> Color32 {
    if primary {
        crate::theme::border_strong()
    } else {
        crate::theme::border()
    }
}

pub fn quick_chip_stroke_w(primary: bool) -> f32 {
    if primary {
        2.0
    } else {
        1.0
    }
}

pub fn quick_chip_fg(primary: bool) -> Color32 {
    if primary {
        crate::theme::fg()
    } else {
        crate::theme::muted()
    }
}

pub fn quick_chip_strong(primary: bool) -> bool {
    primary
}

/// Hover-only why-this copy. No second chip row.
pub fn chip_why_tip(hint: &str, label: &str) -> String {
    let why = hint.trim();
    if !why.is_empty() {
        format!("Why this?\n{why}")
    } else {
        let label = label.trim();
        if label.is_empty() {
            "Why this?".into()
        } else {
            format!("Why this?\n{label}")
        }
    }
}

pub fn paint_empty_chip_state(ui: &mut egui::Ui) {
    let max_w = chip_row_width_lock(ui.available_width());
    ui.allocate_ui_with_layout(
        egui::vec2(max_w, CHIP_ROW_H),
        egui::Layout::left_to_right(egui::Align::Center)
            .with_main_wrap(false)
            .with_main_align(egui::Align::Center),
        |ui| {
            let fill = quick_chip_fill(false);
            let stroke = Stroke::new(quick_chip_stroke_w(false), quick_chip_stroke(false));
            let color = quick_chip_fg(false);
            egui::Frame::none()
                .fill(fill)
                .rounding(18.0)
                .stroke(stroke)
                .inner_margin(egui::Margin::ZERO)
                .show(ui, |ui| {
                    ui.add_space(CHIP_PAD_Y);
                    ui.horizontal(|ui| {
                        ui.add_space(CHIP_PAD_X);
                        ui.add(
                            egui::Label::new(
                                RichText::new(CHIP_EMPTY_LABEL)
                                    .size(13.0)
                                    .color(color),
                            )
                            .wrap()
                            .sense(Sense::hover()),
                        );
                        ui.add_space(CHIP_PAD_X);
                    });
                    ui.add_space(CHIP_PAD_Y);
                });
        },
    );
}

pub fn quick_chip_row(ui: &mut egui::Ui, chips: &[grokhub_core::QuickChip]) -> Option<ChipRowAct> {
    if chips.is_empty() {
        paint_empty_chip_state(ui);
        return None;
    }
    let mut act = None;
    let max_w = chip_row_width_lock(ui.available_width());
    ui.allocate_ui_with_layout(
        egui::vec2(max_w, CHIP_ROW_H),
        egui::Layout::left_to_right(egui::Align::Center)
            .with_main_wrap(false)
            .with_main_align(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for (i, c) in chips.iter().take(grokhub_core::CHIP_VISIBLE_MAX).enumerate() {
                let chip_id = ui.id().with(("qchip", i));
                let hovered = ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(chip_id))
                    .unwrap_or(false);
                let dismiss_t = ui.ctx().animate_bool_with_time(
                    chip_id.with("dismiss"),
                    hovered,
                    grokhub_core::SELECT_SECS,
                );
                let fill = quick_chip_fill(c.primary);
                let stroke = quick_chip_stroke(c.primary);
                let color = quick_chip_fg(c.primary);
                let paint = chip_paint_label(&c.label);
                let why = if c.hint.is_empty() && paint != c.label {
                    c.label.as_str()
                } else {
                    c.hint.as_str()
                };
                let tip = chip_why_tip(why, &c.label);
                let font = if quick_chip_strong(c.primary) {
                    crate::theme::title_font(13.0)
                } else {
                    FontId::proportional(13.0)
                };
                let ir = egui::Frame::none()
                    .fill(fill)
                    .rounding(18.0)
                    .stroke(Stroke::new(quick_chip_stroke_w(c.primary), stroke))
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
                        ui.add_space(CHIP_PAD_Y);
                        ui.horizontal(|ui| {
                            ui.add_space(CHIP_PAD_X);
                            let label_galley = ui.fonts(|f| {
                                f.layout(paint.clone(), font.clone(), color, CHIP_LABEL_WRAP)
                            });
                            let label_w = label_galley.size().x.clamp(8.0, CHIP_LABEL_WRAP);
                            let label_h = label_galley.size().y.max(20.0);
                            let (_rect, hit_resp) =
                                ui.allocate_exact_size(egui::vec2(label_w, label_h), Sense::click());
                            let (hit_resp, felt, wash) =
                                crate::theme::feel_response(ui, hit_resp, Color32::TRANSPARENT);
                            if wash.a() > 0 {
                                ui.painter().rect_filled(felt, 10.0, wash);
                            }
                            ui.painter().galley(felt.min, label_galley, color);
                            let hit = hit_resp.on_hover_text(tip);
                            if hit.clicked() {
                                act = Some(ChipRowAct::Apply(i));
                            }
                            if dismiss_t > 0.01 {
                                let (_xr, x_resp) =
                                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
                                let (x_resp, x_felt, x_wash) =
                                    crate::theme::feel_response(ui, x_resp, Color32::TRANSPARENT);
                                if x_wash.a() > 0 {
                                    ui.painter().rect_filled(x_felt, 6.0, x_wash);
                                }
                                let x_color = crate::theme::blend_color(
                                    Color32::TRANSPARENT,
                                    crate::theme::subtle(),
                                    dismiss_t,
                                );
                                ui.painter().text(
                                    x_felt.center(),
                                    Align2::CENTER_CENTER,
                                    "×",
                                    FontId::proportional(12.0),
                                    x_color,
                                );
                                let x = x_resp.on_hover_text("Hide this suggestion");
                                if x.clicked() {
                                    act = Some(ChipRowAct::Dismiss(i));
                                }
                            }
                            ui.add_space(CHIP_PAD_X);
                        });
                        ui.add_space(CHIP_PAD_Y);
                    });
                let hit = ir.response.interact(egui::Sense::click());
                if hit.clicked() && act.is_none() {
                    act = Some(ChipRowAct::Apply(i));
                }
                ui.ctx()
                    .data_mut(|d| d.insert_temp(chip_id, hit.hovered()));
            }
        },
    );
    act
}

pub fn tab_pill(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    felt_tab(ui, label, active)
}

pub fn section_label(ui: &mut egui::Ui, label: &str) -> bool {
    let hit = ui
        .add(
            egui::Label::new(RichText::new(label).size(13.0).strong().color(crate::theme::subtle()))
                .sense(egui::Sense::click()),
        )
        .clicked();
    ui.add_space(10.0);
    hit
}

pub fn settings_toggle(ui: &mut egui::Ui, title: &str, hint: &str, on: &mut bool) -> bool {
    let mut hit = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
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

pub fn settings_switch(ui: &mut egui::Ui, on: bool) -> bool {
    let (_rect, resp) = ui.allocate_exact_size(egui::vec2(40.0, 24.0), Sense::click());
    let on_t = crate::theme::animate_selection(ui, resp.id.with("sw-on"), on);
    let base_fill = crate::theme::blend_color(crate::theme::panel(), crate::theme::fg(), on_t);
    let (resp, rect, fill) = crate::theme::feel_response(ui, resp, base_fill);
    ui.painter().rect_filled(rect, 12.0, fill);
    if on_t < 0.98 {
        ui.painter()
            .rect_stroke(rect, 12.0, Stroke::new(1.0_f32, crate::theme::border_strong()));
    }
    let knob_x = grokhub_core::lerp_f32(rect.left() + 12.0, rect.right() - 12.0, on_t);
    let knob = crate::theme::blend_color(crate::theme::muted(), crate::theme::bg(), on_t);
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 8.0, knob);
    resp.clicked()
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
    egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(10.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .show(ui, |ui| {
            egui::ComboBox::from_id_salt(title)
                .selected_text(
                    RichText::new(selected)
                        .size(15.0)
                        .color(crate::theme::fg()),
                )
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
    password: bool,
) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(15.0).color(crate::theme::fg()));
    if !hint.is_empty() {
        ui.label(RichText::new(hint).size(12.0).color(crate::theme::muted()));
    }
    ui.add_space(6.0);
    egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(10.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .show(ui, |ui| {
            let mut edit = egui::TextEdit::singleline(value)
                .desired_width(f32::INFINITY)
                .frame(false);
            if password {
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
            RichText::new("Connect your Super Grok account. This signs in GrokHub and the Grok Build CLI together.")
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
            hit = white_pill(ui, action);
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
    let on_t = crate::theme::animate_selection(ui, resp.id.with("nav"), active);
    let base_fill =
        crate::theme::blend_color(Color32::TRANSPARENT, crate::theme::nav_active(), on_t);
    let text_color = crate::theme::blend_color(crate::theme::muted(), crate::theme::fg(), on_t);
    let (resp, rect, fill) = crate::theme::feel_response(ui, resp, base_fill);
    ui.painter().rect_filled(rect, 10.0, fill);
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
    let (_rect, resp) =
        ui.allocate_exact_size(egui::vec2(108.0, 96.0), Sense::click_and_drag());
    let (resp, rect, fill) = crate::theme::feel_response(ui, resp, fill);
    ui.painter().rect_filled(rect, 12.0, fill);
    ui.painter()
        .rect_stroke(rect, 12.0, Stroke::new(1.0_f32, stroke));
    let preview_rect = egui::Rect::from_min_size(
        rect.min + egui::vec2(10.0, 10.0),
        egui::vec2(88.0, 56.0),
    );
    ui.painter().rect_filled(preview_rect, 6.0, preview);
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 14.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(13.0),
        crate::theme::fg(),
    );
    resp.clicked()
        || (resp.contains_pointer() && resp.ctx.input(|i| i.pointer.primary_released()))
}

pub fn search_field(ui: &mut egui::Ui, q: &mut String) {
    search_bar(ui, q, "Search", 180.0);
}

pub fn search_bar(ui: &mut egui::Ui, q: &mut String, hint: &str, width: f32) {
    egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(18.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::symmetric(10.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::paint_bar_icon(ui, icons::BarIcon::Search, 16.0, crate::theme::subtle());
                ui.add(
                    egui::TextEdit::singleline(q)
                        .hint_text(hint.to_owned())
                        .desired_width(width)
                        .frame(false),
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
    ui.painter()
        .rect_stroke(slot.shrink(0.5), rounding, Stroke::new(stroke_w, stroke));
    resp
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
    let mut prepared = egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(crate::theme::CARD_RADIUS)
        .inner_margin(egui::Margin::same(14.0))
        .begin(ui);
    {
        let ui = &mut prepared.content_ui;
        ui.set_min_height(96.0);
        ui.horizontal(|ui| {
            icons::paint_icon(ui, icon, crate::theme::TILE_ICON);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title)
                        .font(crate::theme::card_title_font())
                        .color(crate::theme::fg()),
                );
                ui.add_space(3.0);
                let clipped: String = body.chars().take(80).collect();
                ui.label(
                    RichText::new(clipped)
                        .size(crate::theme::FONT_BODY)
                        .color(crate::theme::muted()),
                );
            });
            if let Some(label) = add {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
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
                });
            }
        });
    }
    let resp = paint_slot_card(ui, prepared, selected, crate::theme::CARD_RADIUS);
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
        ui.columns(cols, |col_uis| {
            for (c, col_ui) in col_uis.iter_mut().enumerate() {
                let i = r * cols + c;
                if i < n {
                    each(col_ui, i);
                }
            }
        });
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
        .unwrap_or_else(|_| image::RgbaImage::from_pixel(1, 1, image::Rgba([0x14, 0x14, 0x14, 0xff])))
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
    ui.painter()
        .rect_filled(r, 14.0, crate::theme::elevated());
    ui.painter()
        .rect_stroke(r, 14.0, Stroke::new(1.0_f32, crate::theme::border()));
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
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            format!("Imagine failed — {fail}"),
            FontId::proportional(crate::theme::FONT_CHROME),
            crate::theme::fg(),
        );
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
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(media), |ui| {
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
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar), |ui| {
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
            WallSlot::Stock(i) => tile_h(IMAGINE_SCENES.get(*i).map(|s| s.tall).unwrap_or(false), scale),
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
    let uv = cover_uv(
        size[0] as f32,
        size[1] as f32,
        rect.width(),
        rect.height(),
    );
    ui.painter()
        .image(tex.id(), rect, uv, Color32::WHITE);
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
    let fade = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - 42.0),
        rect.max,
    );
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
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(u64::MAX);
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
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - 42.0),
                rect.max,
            ),
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
    let t = (now_ms % crate::theme::IMAGINE_FRAME_MS) as f32 / crate::theme::IMAGINE_FRAME_MS as f32;
    let fade = if n == 1 {
        0.0
    } else {
        ((t - 0.72) / 0.28).clamp(0.0, 1.0)
    };
    let (tex, size) = imagine_disk_tex(ui.ctx(), path_a);
    let uv = cover_uv(
        size[0] as f32,
        size[1] as f32,
        rect.width(),
        rect.height(),
    );
    ui.painter()
        .image(tex.id(), rect, uv, Color32::WHITE);
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
    let fade_bar = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - 42.0),
        rect.max,
    );
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
        );
    }
    resp
}

pub fn empty_prompt_tile(ui: &mut egui::Ui, icon: TileIcon, title: &str, hint: &str) -> bool {
    let mut prepared = egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(crate::theme::CARD_RADIUS)
        .inner_margin(egui::Margin::same(14.0))
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
            ui.label(
                RichText::new(hint)
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::muted()),
            );
        });
    }
    paint_slot_card(ui, prepared, false, crate::theme::CARD_RADIUS).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::parse_loop_line;

    #[test]
    fn card_and_wall_hover_stay_in_slot() {
        let src = include_str!("cards.rs");
        let card = src
            .split("fn paint_slot_card(")
            .nth(1)
            .and_then(|s| s.split("pub fn grok_tile(").next())
            .expect("paint_slot_card");
        assert!(
            card.contains("feel_response_in_slot") && card.contains("veil_over"),
            "skill and automation cards must tint inside the slot: {card}"
        );
        assert!(!card.contains("rect_filled"), "hover plate must not cover the title: {card}");
        let tile = src
            .split("pub fn grok_tile(")
            .nth(1)
            .and_then(|s| s.split("pub fn tile_row(").next())
            .expect("grok_tile");
        assert!(
            tile.contains("paint_slot_card") && tile.contains("card_title_font"),
            "{tile}"
        );
        let empty = src
            .split("pub fn empty_prompt_tile(")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("empty_prompt_tile");
        assert!(empty.contains("paint_slot_card") && !empty.contains("rect_filled"), "{empty}");
        let wall = src
            .split("fn wall_slot_feel(")
            .nth(1)
            .and_then(|s| s.split("pub fn empty_prompt_tile(").next())
            .expect("wall_slot_feel");
        assert!(
            wall.contains("feel_response_in_slot") && !wall.contains("rect_filled"),
            "{wall}"
        );
        let photo = src
            .split("fn imagine_photo_tile(")
            .nth(1)
            .and_then(|s| s.split("fn imagine_disk_tex(").next())
            .expect("imagine_photo_tile");
        assert!(photo.contains("wall_slot_feel") && !photo.contains("wash"), "{photo}");
        let disk = src
            .split("fn imagine_disk_tile(")
            .nth(1)
            .and_then(|s| s.split("fn wall_slot_feel(").next())
            .expect("imagine_disk_tile");
        assert!(disk.contains("wall_slot_feel") && !disk.contains("wash"), "{disk}");
    }

    #[test]
    fn the_selected_tab_label_is_readable_on_its_pill() {
        let src = include_str!("cards.rs");
        let tab = src
            .split("pub fn felt_tab(")
            .nth(1)
            .and_then(|s| s.split("pub fn felt_menu_row(").next())
            .expect("felt_tab");
        let layout = tab.find("layout_no_wrap").expect("tab galley");
        let paint = tab.find(".galley(").expect("tab paint");
        assert!(
            tab[layout..paint].contains("Color32::PLACEHOLDER"),
            "a galley laid out in fg() ignores the paint colour, so the selected tab \
             paints a white label on a white pill: {tab}"
        );
        assert!(
            tab.contains("blend_color(crate::theme::muted(), crate::theme::bg()"),
            "the selected tab reads in the background colour: {tab}"
        );
    }

    #[test]
    fn chip_row_act_is_apply_or_dismiss() {
        assert_ne!(ChipRowAct::Apply(0), ChipRowAct::Dismiss(0));
        match ChipRowAct::Apply(4) {
            ChipRowAct::Apply(i) => assert_eq!(i, 4),
            ChipRowAct::Dismiss(_) => panic!("apply is not dismiss"),
        }
        assert_eq!(chip_paint_label("Continue Night cabin"), "Continue Night cabin");
        let long = chip_paint_label(
            "Continue the work from the chat \"Night cabin\". Last ask: paint the wall.",
        );
        assert!(
            long.contains("Night cabin") && long.contains("paint the wall"),
            "{long}"
        );
        assert!(!long.contains('…'), "{long}");
    }

    #[test]
    fn composer_hover_copy_covers_the_real_pills() {
        let modes: Vec<_> = composer_modes().iter().map(|(id, _)| *id).collect();
        assert_eq!(modes, ["chat", "plan", "ask"]);
        for id in modes {
            let (title, body) = composer_session_tip(id).expect(id);
            assert!(!title.is_empty(), "{id}");
            assert!(body.len() < 160, "{id} tip too long: {body}");
        }
        let (chat_title, chat) = composer_session_tip("chat").unwrap();
        assert_eq!(chat_title, "Chat");
        assert!(chat.contains("Normal chat"), "{chat}");
        let (plan_title, plan) = composer_session_tip("plan").unwrap();
        assert_eq!(plan_title, "Plan");
        assert!(plan.contains("plan") && plan.contains("/plan"), "{plan}");
        let (look_title, look) = composer_session_tip("ask").unwrap();
        assert_eq!(look_title, "Questions");
        assert_eq!(composer_modes()[2], ("ask", "Questions"));
        assert_ne!(composer_modes()[2].1, permission_modes()[0].1);
        assert!(SEG_INSET_X >= 8.0);
        assert!(SEG_INSET_Y >= 6.0);
        let styled = include_str!("cards.rs")
            .split("fn felt_segment_styled(")
            .nth(1)
            .and_then(|s| s.split("pub fn felt_tab(").next())
            .expect("felt_segment_styled");
        assert!(
            styled.contains("SEG_INSET_X")
                && styled.contains("layout_no_wrap")
                && styled.contains("galley"),
            "Questions must size to the label plus inset, not a 52px jam: {styled}"
        );
        assert!(look.contains("without editing"), "{look}");
        assert!(!look.contains("Ask"), "{look}");
        assert!(!look_title.contains("Read"), "{look_title}");
        assert!(composer_session_tip("always-approve").is_none());
        assert!(composer_session_tip("auto").is_none());

        let perms: Vec<_> = permission_modes().iter().map(|(id, _)| *id).collect();
        assert_eq!(perms, ["ask", "auto", "always-approve"]);
        for id in perms {
            let (title, body) = composer_perm_tip(id).expect(id);
            assert!(!title.is_empty(), "{id}");
            assert!(body.len() < 160, "{id} tip too long: {body}");
        }
        let (ask_p, ask_body) = composer_perm_tip("ask").unwrap();
        assert_eq!(ask_p, "Ask");
        assert!(
            ask_body.contains("Allow / Deny") && ask_body.contains("denied"),
            "{ask_body}"
        );
        let (auto_t, auto) = composer_perm_tip("auto").unwrap();
        assert_eq!(auto_t, "Auto");
        assert!(auto.contains("Safe tools") && auto.contains("/auto"), "{auto}");
        let (always_t, always) = composer_perm_tip("always-approve").unwrap();
        assert_eq!(always_t, "Always");
        assert!(
            always.contains("Skip every tool") && always.contains("/always-approve"),
            "{always}"
        );
        assert!(composer_perm_tip("chat").is_none());
        assert!(composer_perm_tip("plan").is_none());

        let (effort_t, effort) = composer_effort_tip();
        assert_eq!(effort_t, "Effort");
        assert!(
            effort.contains("None through Extra High") && effort.contains("/effort") && !effort.contains("Max"),
            "{effort}"
        );
        let effort_src = include_str!("cards.rs")
            .split("pub fn effort_pill(")
            .nth(1)
            .and_then(|s| s.split("pub struct SessionRowOut").next())
            .expect("effort_pill");
        assert!(
            effort_src.contains("composer_effort_tip"),
            "effort dropdown must show hover help: {effort_src}"
        );
    }

    #[test]
    fn mode_pill_fits_the_composer_cluster() {
        assert_eq!(MODE_PILL_W, 84.0);
        assert_eq!(
            composer_go_cluster_w(),
            22.0 + 28.0 + 8.0 * 3.0 + 12.0
        );
        assert_eq!(composer_modes().len(), 3);
        assert_eq!(permission_modes().len(), 3);
        assert_eq!(effort_modes().len(), 6);
        assert!(effort_modes().iter().all(|(id, label)| *id != "max" && *label != "Max"));
        assert_eq!(effort_label("high"), "High");
        let session = include_str!("cards.rs")
            .split("pub fn session_row(")
            .nth(1)
            .and_then(|s| s.split("pub fn voice_mode_row(").next())
            .expect("session_row");
        assert!(
            session.contains("felt_segment")
                && session.contains("felt_perm_segment")
                && session.contains("out.effort = Some(next)"),
            "composer session row must include effort dropdown: {session}"
        );
        assert_eq!(
            permission_risk_stroke_w("always-approve", false),
            permission_risk_stroke_w("ask", false)
        );
        assert_eq!(
            permission_risk_stroke_w("always-approve", false),
            permission_risk_stroke_w("auto", false)
        );
        assert_eq!(
            permission_risk_stroke_color("always-approve", false),
            permission_risk_stroke_color("ask", false)
        );
        assert_eq!(
            permission_risk_stroke_color("always-approve", false),
            crate::theme::border()
        );
        let _paint = crate::theme::hold_paint_test();
        crate::theme::set_paint_dark(true);
        assert_eq!(
            permission_risk_stroke_color("always-approve", true),
            crate::theme::ALWAYS_AMBER_DARK
        );
        crate::theme::set_paint_dark(false);
        assert_eq!(
            permission_risk_stroke_color("always-approve", true),
            crate::theme::ALWAYS_AMBER_LIGHT
        );
        crate::theme::set_paint_dark(true);
        assert_ne!(
            permission_risk_stroke_color("always-approve", true),
            crate::theme::setup()
        );
        assert_ne!(
            permission_risk_stroke_color("always-approve", false),
            crate::theme::always_amber()
        );
        assert_eq!(permission_risk_stroke_w("always-approve", true), 2.0);
        assert_eq!(permission_risk_stroke_w("auto", true), 1.0);
        assert_eq!(permission_risk_stroke_w("ask", true), 1.0);
        assert_eq!(permission_risk_fill(true), crate::theme::nav_active());
        assert_eq!(permission_risk_fill(false), Color32::TRANSPARENT);
        assert_ne!(permission_risk_fill(true), crate::theme::always_amber());
        assert!(permission_risk_strong("always-approve", true));
        assert!(!permission_risk_strong("always-approve", false));
        assert!(!permission_risk_strong("ask", true));
        assert!(!permission_risk_strong("auto", true));
        assert!(
            session.contains("composer_session_tip")
                && session.contains("composer_perm_tip")
                && session.contains("with_composer_tip"),
            "composer pills must show hover help: {session}"
        );
        let seg = include_str!("cards.rs")
            .split("pub fn felt_segment(")
            .nth(1)
            .and_then(|s| s.split("pub fn permission_risk_stroke_w(").next())
            .expect("felt_segment");
        assert!(
            seg.contains("FONT_BODY") && seg.contains("None"),
            "session pills stay quieter than permission: {seg}"
        );
        let perm_seg = include_str!("cards.rs")
            .split("pub fn felt_perm_segment(")
            .nth(1)
            .and_then(|s| s.split("fn felt_segment_styled(").next())
            .expect("felt_perm_segment");
        assert!(
            perm_seg.contains("FONT_CHROME") && perm_seg.contains("permission_risk_stroke_w"),
            "permission row keeps a designed stroke: {perm_seg}"
        );
        let voice = include_str!("cards.rs")
            .split("pub fn voice_mode_row(")
            .nth(1)
            .and_then(|s| s.split("pub fn clip_status(").next())
            .expect("voice_mode_row");
        assert!(
            voice.contains("theme::live()") && voice.contains("ghost_pill(ui, \"Stop\")"),
            "voice mode must show a live indicator and Stop: {voice}"
        );
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(!voice_mode_row(ui, "Listening"));
            });
        });
        let switch = include_str!("cards.rs")
            .split("pub fn settings_switch(")
            .nth(1)
            .and_then(|s| s.split("pub fn settings_dropdown(").next())
            .expect("settings_switch");
        assert!(
            switch.contains("animate_selection") && switch.contains("lerp_f32"),
            "settings switch must slide the knob: {switch}"
        );
        let dropdown = include_str!("cards.rs")
            .split("pub fn settings_dropdown(")
            .nth(1)
            .and_then(|s| s.split("pub fn settings_field(").next())
            .expect("settings_dropdown");
        assert!(
            dropdown.contains("ComboBox") && dropdown.contains("selectable_label"),
            "settings dropdown must be one ComboBox: {dropdown}"
        );
        let pills = include_str!("cards.rs");
        assert!(
            pills.contains("pub fn felt_pill(")
                && pills.contains("white_pill(ui, label, PillStyle::Solid)"),
            "white_pill must delegate to felt_pill"
        );
        assert_eq!(clip_status("one\ntwo", 80), "one");
        assert_eq!(clip_status("abcdefghij", 6), "abcde…");
        assert_eq!(chip_tone_color(ChipTone::Offline), crate::theme::offline());
    }

    #[test]
    fn first_quick_chip_is_inline_not_selected() {
        assert_ne!(quick_chip_fill(true), quick_chip_fill(false));
        assert_ne!(quick_chip_stroke(true), quick_chip_stroke(false));
        assert!(quick_chip_stroke_w(true) > quick_chip_stroke_w(false));
        assert!(quick_chip_strong(true));
        assert!(!quick_chip_strong(false));
        assert_eq!(quick_chip_fg(true), crate::theme::fg());
        assert_eq!(quick_chip_fg(false), crate::theme::muted());
        assert!(chip_why_tip("Last slash", "/plan").starts_with("Why this?"));
        assert_eq!(chip_why_tip("", "Continue"), "Why this?\nContinue");
        let max_w = chip_row_width_lock(640.0);
        assert_eq!(max_w, 640.0);
        assert_ne!(max_w, 0.0);
        let src = include_str!("cards.rs");
        let start = src.find("pub fn quick_chip_row").expect("chip row");
        let slice = &src[start..start + 2200];
        assert!(
            slice.contains("with_main_align(egui::Align::Center)"),
            "chips sit on the midline of the bar: {slice}"
        );
        assert!(
            !slice.contains("set_width(max_w)") && !slice.contains("set_min_width"),
            "chip cluster must shrink-wrap, not fill the pill: {slice}"
        );
        assert!(
            slice.contains("CHIP_ROW_H") && slice.contains("allocate_ui_with_layout"),
            "chip row must use a tight height or leftover empty-home space vertically centers it: {slice}"
        );
        assert!(
            !slice.contains('…') && slice.contains("CHIP_PAD_X") && slice.contains("add_space"),
            "chip paint pads inside the fill and must not ellipsize: {slice}"
        );
        assert!(
            !slice.contains("ui.with_layout("),
            "with_layout eats remaining height and drops chips to the bottom: {slice}"
        );
        assert_eq!(CHIP_EMPTY_LABEL, "Nothing queued");
        assert!(
            slice.contains("paint_empty_chip_state(ui)"),
            "empty ranking must paint a muted placeholder, not a blank row: {slice}"
        );
        let empty = include_str!("cards.rs")
            .split("pub fn paint_empty_chip_state(")
            .nth(1)
            .and_then(|s| s.split("pub fn quick_chip_row(").next())
            .expect("paint_empty_chip_state");
        assert!(
            empty.contains("CHIP_EMPTY_LABEL")
                && empty.contains("quick_chip_fill(false)")
                && empty.contains("quick_chip_stroke_w(false)")
                && empty.contains("CHIP_ROW_H")
                && empty.contains("Sense::hover()"),
            "empty state is one muted chip-shaped label, not a ranked action: {empty}"
        );
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(quick_chip_row(ui, &[]).is_none());
            });
        });
    }

    #[test]
    fn suggested_autos_parse() {
        assert_eq!(SUGGESTED_AUTOS.len(), 7);
        for s in SUGGESTED_AUTOS {
            let (iv, prompt) = parse_loop_line(s.seed).expect(s.title);
            assert!(!iv.is_empty());
            assert!(!prompt.is_empty());
            assert!(s.seed.contains("/loop"));
            let _ = s.icon;
        }
    }

    #[test]
    fn learned_tiles_lead_static_fallback() {
        let learned_auto = LearnedSuggestion {
            kind: SuggestionKind::Auto,
            title: "Night wrap".into(),
            body: "Close the day".into(),
            seed: Some("every day at 21, say good night".into()),
            name: None,
            trigger: None,
            instructions: None,
            provider: None,
            tool: None,
        };
        let autos = merge_suggested_autos(&[learned_auto], &[]);
        assert_eq!(autos[0].1, "Night wrap");
        assert!(autos.iter().any(|t| t.1 == "Morning brief"));
        let hidden = merge_suggested_autos(&[], &["Morning brief".into()]);
        assert!(!hidden.iter().any(|t| t.1 == "Morning brief"));
        let by_prompt = merge_suggested_autos(
            &[],
            &["summarize the workboard and last host receipt".into()],
        );
        assert!(
            !by_prompt.iter().any(|t| t.1 == "Morning brief"),
            "adding a /loop seed must hide the matching Suggested tile: {by_prompt:?}"
        );
        let learned_skill = LearnedSuggestion {
            kind: SuggestionKind::Skill,
            title: "Desk tidy".into(),
            body: "Straighten windows".into(),
            seed: None,
            name: Some("desk-tidy".into()),
            trigger: Some("when the desk is messy".into()),
            instructions: Some("stack the windows".into()),
            provider: None,
            tool: None,
        };
        let skills = merge_suggested_skills(std::slice::from_ref(&learned_skill), &[]);
        assert_eq!(skills[0].1, "Desk tidy");
        let hidden_skill = merge_suggested_skills(&[learned_skill], &["desk-tidy".into()]);
        assert!(hidden_skill.is_empty());
        assert_eq!(GITHUB_TILES.len(), 2);
        assert_eq!(GITHUB_TILES[0].2, "user");
        assert_eq!(GITHUB_TILES[1].2, "list_repos");
    }

    #[test]
    fn catalog_is_cabin_real() {
        let forbidden = [
            "outlook", "gmail", "stock", "ticker", "docx", "xlsx", "pptx",
            "powerpoint", "spreadsheet", "word document", "pdf", "video",
        ];
        for s in SUGGESTED_AUTOS {
            let blob = format!("{} {} {}", s.title, s.body, s.seed).to_ascii_lowercase();
            for w in forbidden {
                assert!(!blob.contains(w), "auto {} mentions {w}", s.title);
            }
        }
        assert_eq!(IMAGINE_SCENES.len(), 9);
        assert_eq!(imagine_word(0), "the cabin");
        assert_eq!(imagine_word(2800), "the night");
        assert_eq!(grokhub_core::imagine_aspect_label(0), "2:3");
        assert_eq!(grokhub_core::imagine_aspect_label(4), "16:9");
        assert_eq!(grokhub_core::imagine_aspect_name(0), "Tall");
        assert_eq!(
            imagine_send_cluster_w(),
            crate::theme::IMAGINE_HIT * 2.0 + 12.0
        );
        assert!(
            composer_go_cluster_w() >= 22.0 + 28.0,
            "mic + Stop disc stay inside the bar after session pills moved above"
        );
        assert_eq!(
            composer_pill_w(900.0),
            600.0,
            "900-wide cabin minus rail and central margins"
        );
        assert_eq!(
            composer_pill_w(1400.0),
            crate::theme::CHAT_COL_W,
            "wide cabins lock to the Grok conversation column"
        );
        assert_eq!(
            composer_pill_w(3440.0),
            crate::theme::CHAT_COL_W,
            "ultrawide composer stays a centered Grok column, got {}",
            composer_pill_w(3440.0)
        );
        assert_eq!(
            composer_pill_w(3440.0),
            composer_pill_w(1920.0),
            "past the column cap the pill does not keep growing"
        );
        assert!(
            composer_pill_w(900.0) > composer_go_cluster_w() + 80.0,
            "Stop cluster must fit inside a 900-wide cabin pill"
        );
        let inner = composer_pill_w(900.0) - 16.0;
        assert_eq!(
            22.0 + 8.0 + composer_mid_w(inner) + 8.0 + composer_go_hit_w(),
            inner,
            "Plus + mid + Stop must fill the frame inner, not overflow it"
        );
        // grok.com/imagine `.query-bar` measured height (94) minus its padding.
        let bar_inner = 94.0 - 20.0;
        assert_eq!(imagine_prompt_h(), 32.0);
        assert_eq!(imagine_prompt_chip_gap(), 8.0);
        assert_eq!(
            imagine_chip_stack_h(),
            (crate::theme::IMAGINE_HIT + 8.0) * 2.0
        );
        assert!(
            imagine_prompt_h() < bar_inner,
            "prompt must be pinned, not stretched to bar min-height {bar_inner}"
        );
        let chip_top = imagine_prompt_h() + imagine_prompt_chip_gap();
        assert!(
            bar_inner > chip_top,
            "a stretching prompt of {bar_inner}px would cover chips starting at {chip_top}"
        );
        let stage = include_str!("cards.rs");
        let stage = stage
            .split("pub fn imagine_stage(")
            .nth(1)
            .and_then(|s| s.split("pub fn imagine_result_hero(").next())
            .expect("imagine_stage");
        assert!(
            stage.contains("Imagining…")
                && stage.contains("Imagining video…")
                && stage.contains("Imagine failed")
                && stage.contains("Expand")
                && stage.contains("Play")
                && stage.contains("Save")
                && stage.contains("Open")
                && stage.contains("imagine_media_click")
                && stage.contains("ImagineMediaClick::Play"),
            "generating box must be interactive: {stage}"
        );
        assert!(
            stage.contains("ImagineMediaClick::Play => hit.play = true")
                && stage.contains("ghost_pill(ui, \"Play\")"),
            "a video click must play, not only expand: {stage}"
        );
        let hero = include_str!("cards.rs");
        let hero = hero
            .split("fn imagine_video_hero(")
            .nth(1)
            .and_then(|s| s.split("pub fn imagine_masonry(").next())
            .expect("imagine_video_hero");
        assert!(
            hero.contains("video_poster_ready")
                && hero.contains("kick_video_poster")
                && hero.contains("thread::spawn")
                && !hero.contains("g.remove(&path)"),
            "video poster extract must leave the UI thread and attempt a clip once: {hero}"
        );
        assert!(
            stage.contains("!fail.is_empty()")
                && !stage.contains("!fail.is_empty() && path.is_empty()"),
            "on-stage error must win over leftover imagine_last: {stage}"
        );
        assert_eq!(imagine_kind_label(ImagineKind::Image), "Image");
        assert_eq!(imagine_kind_label(ImagineKind::Video), "Video");
        assert_eq!(imagine_kind_label(ImagineKind::Agent), "Agent");
        assert_eq!(imagine_quality_label(false), "Speed");
        assert_eq!(imagine_quality_label(true), "Quality (v2.0)");
        let fallback = imagine_still_rgba(b"not-a-jpeg");
        assert_eq!((fallback.width(), fallback.height()), (1, 1));
        for s in IMAGINE_SCENES {
            let blob = format!("{} {}", s.title, s.prompt).to_ascii_lowercase();
            for w in forbidden {
                assert!(!blob.contains(w), "imagine {} mentions {w}", s.title);
            }
            assert!(
                blob.contains("still") || blob.contains("cabin") || blob.contains("desk"),
                "imagine {} is not a still",
                s.title
            );
            assert!(!blob.contains("video"));
            assert!(!blob.contains("photo edit"));
            let _ = s.icon;
            let _ = s.tall;
            assert!(
                s.frames.len() >= 2,
                "imagine {} needs two frames to live like a cover GIF",
                s.title
            );
            for key in s.frames {
                let bytes = still_jpeg(key);
                assert!(bytes.len() > 1000, "imagine still {key} is empty");
                let img = image::load_from_memory(bytes).expect(key);
                assert!(img.width() >= 256);
                assert!(img.height() >= 256);
            }
            let (a, _, _) = imagine_frame_pair(s, 0);
            let (b, _, _) = imagine_frame_pair(s, crate::theme::IMAGINE_FRAME_MS);
            assert_ne!(a, b, "imagine {} cover must change", s.title);
            let (_, _, fade0) = imagine_frame_pair(s, 0);
            let (_, _, fade1) = imagine_frame_pair(s, crate::theme::IMAGINE_FRAME_MS - 1);
            assert!(fade0 < 0.05);
            assert!(fade1 > 0.9);
        }
        let uv = cover_uv(768.0, 512.0, 345.0, 230.0);
        assert!(uv.width() > 0.4 && uv.height() > 0.9);
    }

    #[test]
    fn composer_pill_tracks_the_monitor() {
        assert_eq!(composer_pill_w(900.0), 600.0);
        assert_eq!(composer_pill_w(1400.0), crate::theme::CHAT_COL_W);
        assert_eq!(
            composer_pill_w(3440.0),
            crate::theme::CHAT_COL_W,
            "ultrawide composer stays a centered Grok column, got {}",
            composer_pill_w(3440.0)
        );
        assert_eq!(composer_pill_w(3440.0), composer_pill_w(1920.0));
        assert!(composer_pill_w(1400.0) > composer_pill_w(900.0));
    }

    #[test]
    fn imagine_still_tex_decodes_off_the_ui_thread() {
        let src = include_str!("cards.rs");
        let tex = src
            .split("fn imagine_still_tex(")
            .nth(1)
            .and_then(|s| s.split("fn cover_uv(").next())
            .expect("imagine_still_tex");
        let spawn = tex.find("thread::spawn").expect("decode must leave the UI thread");
        let decode = tex.find("imagine_still_rgba").expect("bundled JPEG decode");
        assert!(
            spawn < decode && tex.contains("inflight"),
            "stock Imagine stills must not JPEG-decode on the first paint: {tex}"
        );
    }

    #[test]
    fn imagine_disk_tex_rejects_a_huge_file() {
        let src = include_str!("cards.rs");
        let tex = src
            .split("fn imagine_disk_tex(")
            .nth(1)
            .and_then(|s| s.split("fn imagine_disk_tile(").next())
            .expect("imagine_disk_tex");
        let meta = tex.find("metadata").expect("size check before decode");
        let read = tex.find("std::fs::read").expect("read image");
        let spawn = tex.find("thread::spawn").expect("decode must leave the UI thread");
        assert!(
            spawn < read && meta < read && tex.contains("IMAGE_FILE_CAP"),
            "a huge wall still must not decode on the UI thread: {tex}"
        );
        assert!(
            tex.contains("image_pixels_ok") || tex.contains("IMAGE_PIXEL_CAP"),
            "a tiny wall still with huge pixels must not decode on the UI thread: {tex}"
        );
    }

    #[test]
    fn run_pulse_is_a_labeled_live_dot() {
        let src = include_str!("cards.rs");
        let pulse = src
            .split("pub fn paint_run_pulse(")
            .nth(1)
            .and_then(|s| s.split("pub fn titlebar_update_chip(").next())
            .expect("paint_run_pulse");
        assert!(
            pulse.contains("theme::live()")
                && pulse.contains("circle_filled")
                && pulse.contains("chat_run_dot_alpha")
                && pulse.contains("on_hover_text")
                && !pulse.contains("ghost_pill")
                && !pulse.contains("Stop"),
            "a chat turn shows a pulsing live dot and label, and the row has no Stop: {pulse}"
        );
        assert!(
            !pulse.contains("vec2(2.0, 16.0)") && !pulse.contains("rect_filled"),
            "the running cue is not a blinking caret: {pulse}"
        );
    }

    #[test]
    fn run_pulse_paints_a_labeled_row() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let _paint = crate::theme::hold_paint_test();
                crate::theme::set_paint_dark(true);
                paint_run_pulse(ui, "Running", "run_terminal_cmd");
                paint_run_pulse(ui, "", "hidden");
            });
        });
    }

    #[test]
    fn get_started_copy_and_connect() {
        let src = include_str!("cards.rs");
        assert!(
            src.contains("Get Started")
                && src.contains("Connect your Super Grok account")
                && src.contains("Connect Super Grok"),
            "{src}"
        );
        let app = concat!(
            include_str!("app/mod.rs"),
            include_str!("app/pages.rs"),
            include_str!("app/oauth.rs"),
            include_str!("app/settings.rs"),
        );
        assert!(
            app.contains("ui_get_started")
                && app.contains("should_show_get_started")
                && app.contains("start_oauth")
                && app.contains("oauth_err"),
            "Get Started must use cabin device-code OAuth and surface errors: {app}"
        );
        assert!(
            app.contains("Also signs in the Grok Build CLI if it is not already connected"),
            "settings Connect must say it also signs the CLI in: {app}"
        );
    }
}
