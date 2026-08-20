//! Grok catalog chrome — huge title, white pills, 3-column icon tiles.

use crate::icons::{self, TileIcon};
use eframe::egui::{self, Align2, Color32, ColorImage, FontId, RichText, Sense, Stroke, TextureHandle, TextureOptions};
use grokhub_core::{
    curate_wall, imagine_result_fit, wall_curate_seed, LearnedSuggestion, SkillMd, SuggestionKind,
    WallGif, WallSlot, IMAGE_FILE_CAP,
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
pub struct SuggestedSkill {
    pub icon: TileIcon,
    pub name: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    pub trigger: &'static str,
    pub instructions: &'static str,
    pub verify: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveConnector {
    pub icon: TileIcon,
    pub id: &'static str,
    pub title: &'static str,
    pub tools: &'static [(&'static str, &'static str)],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileHit {
    None,
    Add,
    Body,
}

pub const SUGGESTED_SKILLS: &[SuggestedSkill] = &[
    SuggestedSkill {
        icon: TileIcon::Sun,
        name: "morning-brief",
        title: "Morning brief",
        body: "Summarize the workboard, last host receipt, and pinned goal.",
        trigger: "morning brief",
        instructions: "Summarize the Workboard and last HOST_RESULT from the system context. List open cards and the next concrete step. If the board is empty, say so. No secrets.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        icon: TileIcon::Host,
        name: "host-snapshot",
        title: "Host snapshot",
        body: "Read-only uname / whoami / pwd via Grok Build bash.",
        trigger: "host snapshot",
        instructions: "Run uname -a && whoami && pwd with Grok Build tools. Summarize in four lines.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        icon: TileIcon::List,
        name: "workboard-triage",
        title: "Workboard triage",
        body: "Pull open tasks from this thread onto the workboard.",
        trigger: "triage the board",
        instructions: "Extract open tasks from this thread. For each task emit one line exactly: WORK_PIN: short title | one-line detail | priority=med. Do not emit WORK_UPDATE done without VERIFY_OK.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        icon: TileIcon::Image,
        name: "imagine-scene",
        title: "Imagine a scene",
        body: "Write a tight still-image prompt; the cabin generates it.",
        trigger: "imagine this",
        instructions: "Write one tight still-image prompt (grok-imagine-image-2.0, no faces). Emit one line exactly: IMAGINE_PROMPT: <prompt>. Stay in the bound project.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        icon: TileIcon::Github,
        name: "github-pulse",
        title: "GitHub pulse",
        body: "Who am I plus recent repos via CONNECTOR_CMD (needs a PAT).",
        trigger: "github pulse",
        instructions: "Emit these two lines exactly, each on its own line:\nCONNECTOR_CMD: github user\nCONNECTOR_CMD: github list_repos\nAfter both CONNECTOR_RESULT blocks, summarize in four lines. No secrets.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        icon: TileIcon::Check,
        name: "verify-last",
        title: "Verify last turn",
        body: "Run verify.sh and hold done until VERIFY_OK.",
        trigger: "verify this",
        instructions: "Run the skill verify.sh. After the output, report VERIFY_OK or the exact failure. Do not mark workboard cards done without VERIFY_OK.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        icon: TileIcon::Board,
        name: "board-status",
        title: "Board status",
        body: "List open workboard cards and the next concrete step.",
        trigger: "board status",
        instructions: "List every open Workboard card from the system context. One line each. Then the next concrete step. If empty, say so. Optional: WORK_PIN: follow-up | detail | priority=low",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        icon: TileIcon::Bolt,
        name: "host-health",
        title: "Host health",
        body: "Richer read-only snapshot: load, disk, and grokhub paths.",
        trigger: "host health",
        instructions: "Check uname, whoami, pwd, df -h /, and uptime with Grok Build tools. Four lines: machine, user, disk, load. No secrets.",
        verify: "echo VERIFY_OK",
    },
];

pub const LIVE_CONNECTORS: &[LiveConnector] = &[LiveConnector {
    icon: TileIcon::Github,
    id: "github",
    title: "GitHub",
    tools: &[
        ("Who am I", "user"),
        ("List repos", "list_repos"),
        ("List issues", "list_issues"),
        ("Search code", "search_code"),
        ("Search issues", "search_issues"),
    ],
}];

pub const SUGGESTED_AUTOS: &[SuggestedAuto] = &[
    SuggestedAuto {
        icon: TileIcon::Sun,
        title: "Morning brief",
        body: "Weekdays at 09:00 — workboard and last host receipt.",
        seed: "every weekday at 9, summarize the workboard",
    },
    SuggestedAuto {
        icon: TileIcon::Bolt,
        title: "Host heartbeat",
        body: "Every 60 min — read-only snapshot, then a short note.",
        seed: "heartbeat every 60 min, run a read-only host snapshot and summarize",
    },
    SuggestedAuto {
        icon: TileIcon::List,
        title: "Task extractor",
        body: "Weekdays at 18:00 — pull today's open tasks onto the board.",
        seed: "every weekday at 18, extract open tasks onto the workboard",
    },
    SuggestedAuto {
        icon: TileIcon::Host,
        title: "Dawn snapshot",
        body: "Weekdays at 08:00 — read-only host snapshot before the day.",
        seed: "every weekday at 8, run a read-only host snapshot and summarize",
    },
    SuggestedAuto {
        icon: TileIcon::Board,
        title: "Midday board",
        body: "Weekdays at 12:00 — summarize the workboard.",
        seed: "every weekday at 12, summarize the workboard",
    },
    SuggestedAuto {
        icon: TileIcon::Moon,
        title: "Nightly triage",
        body: "Every day at 21:00 — extract leftover tasks onto the board.",
        seed: "every day at 21, extract open tasks onto the workboard",
    },
    SuggestedAuto {
        icon: TileIcon::Host,
        title: "Replay last desktop run",
        body: "Ask Grok to repeat the last desktop task. Night does not chat first.",
        seed: "every day at 21, replay last",
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

/// grok.com/imagine Image-mode toolbar labels, measured 2026-08-15.
pub const IMAGINE_BAR_CHIPS: &[&str] = &[
    "Image",
    "Video",
    "Agent",
    "Speed",
    "Quality (v2.0)",
    "Auto",
];

/// grok.com/imagine Video-mode chips, measured 2026-08-15.
pub const IMAGINE_VIDEO_CHIPS: &[&str] = &["480p", "720p", "6s", "10s", "15s", "Video audio"];

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

pub fn imagine_quality_word(quality: bool) -> &'static str {
    if quality {
        "quality"
    } else {
        "speed"
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
/// inflate that past the pane so Stop paints off-screen).
pub fn composer_pill_w(screen_w: f32) -> f32 {
    (screen_w - crate::theme::SIDEBAR_W - 40.0)
        .min(crate::theme::QUERY_MAX_W)
        .max(360.0)
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

/// Inner width left for wrapping selector chips after send/mic are reserved.
pub fn imagine_chip_row_w(bar_w: f32) -> f32 {
    (bar_w - 24.0 - imagine_send_cluster_w()).max(crate::theme::IMAGINE_HIT * 4.0)
}

/// grok.com Imagine toolbox chips for the current kind. Aspect defaults to 2:3.
pub fn imagine_toolbar_labels(kind: ImagineKind, authed: bool) -> Vec<&'static str> {
    let mut out = vec!["Image", "Video", "Agent"];
    match kind {
        ImagineKind::Image | ImagineKind::Agent => {
            out.extend(["Speed", "Quality (v2.0)"]);
        }
        ImagineKind::Video => {
            out.extend(["480p", "720p", "6s", "10s", "15s", "Video audio"]);
        }
    }
    out.push("Auto");
    out.push("2:3");
    if !authed {
        out.push("Connect Grok");
    }
    out
}

/// Speed / Quality and aspect also map to Imagine API resolution and aspect_ratio.
pub fn imagine_still_prompt(prompt: &str, aspect: &str, quality: bool) -> String {
    let q = imagine_quality_word(quality);
    let p = prompt.trim();
    if p.is_empty() {
        return String::new();
    }
    let has_aspect = p.contains(aspect);
    let has_q = p.to_ascii_lowercase().contains(q);
    match (has_aspect, has_q) {
        (true, true) => p.to_string(),
        (true, false) => format!("{p}, {q} still"),
        (false, true) => format!("{p}, {aspect} still"),
        (false, false) => format!("{p}, {aspect} {q} still"),
    }
}

pub fn imagine_aspect_label(i: u8) -> &'static str {
    grokhub_core::imagine_aspect_label(i)
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

pub fn imagine_frame_key(scene: &ImagineScene, now_ms: u64) -> &'static str {
    imagine_frame_pair(scene, now_ms).0
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

pub fn is_cabin_catalog(name: &str) -> bool {
    let k = name.trim().to_ascii_lowercase();
    matches!(k.as_str(), "github" | "gh")
}

pub fn skill_from_learned(s: &LearnedSuggestion) -> SkillMd {
    let name = s
        .name
        .as_deref()
        .filter(|n| !n.is_empty())
        .unwrap_or("learned-skill");
    SkillMd {
        name: name.into(),
        description: s.body.clone(),
        slash: format!("/{name}"),
        trigger: s.trigger.clone().unwrap_or_default(),
        instructions: s.instructions.clone().unwrap_or_default(),
        pitfalls: "Do not write secrets into markdown.".into(),
        verify: "echo VERIFY_OK".into(),
        runs: 0,
    }
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
        let key = s.title.to_ascii_lowercase();
        if seen.iter().any(|n| n == &key) {
            continue;
        }
        seen.push(key);
        out.push((
            icons::icon_for_label(&s.title),
            s.title.clone(),
            s.body.clone(),
            s.seed.clone().unwrap_or_default(),
        ));
    }
    for s in SUGGESTED_AUTOS {
        let key = s.title.to_ascii_lowercase();
        if seen.iter().any(|n| n == &key) {
            continue;
        }
        seen.push(key);
        out.push((s.icon, s.title.into(), s.body.into(), s.seed.into()));
    }
    out
}

/// Learned skills first, then static seeds not already saved.
pub fn merge_suggested_skills(
    learned: &[LearnedSuggestion],
    saved_names: &[String],
) -> Vec<LearnedSuggestion> {
    let mut seen: Vec<String> = saved_names.iter().map(|s| s.to_ascii_lowercase()).collect();
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
        if seen.iter().any(|n| n == &key) {
            continue;
        }
        seen.push(key);
        out.push(s.clone());
    }
    for s in SUGGESTED_SKILLS {
        let key = s.name.to_ascii_lowercase();
        if seen.iter().any(|n| n == &key) {
            continue;
        }
        seen.push(key);
        out.push(LearnedSuggestion {
            kind: SuggestionKind::Skill,
            title: s.title.into(),
            body: s.body.into(),
            seed: None,
            name: Some(s.name.into()),
            trigger: Some(s.trigger.into()),
            instructions: Some(s.instructions.into()),
            provider: None,
            tool: None,
        });
    }
    out
}

/// Learned GitHub tiles first, then the live tools as day-one fallback.
pub fn merge_suggested_connectors(
    learned: &[LearnedSuggestion],
) -> Vec<(icons::TileIcon, String, String, String)> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for s in learned {
        if s.kind != SuggestionKind::Connector {
            continue;
        }
        let tool = s.tool.clone().unwrap_or_default();
        if tool.is_empty() || seen.iter().any(|t| t == &tool) {
            continue;
        }
        seen.push(tool.clone());
        out.push((
            icons::icon_for_label(&s.title),
            s.title.clone(),
            s.body.clone(),
            tool,
        ));
    }
    for c in LIVE_CONNECTORS {
        for (label, tool) in c.tools {
            if seen.iter().any(|t| t == *tool) {
                continue;
            }
            seen.push((*tool).into());
            out.push((
                c.icon,
                (*label).into(),
                format!("GitHub {tool} — runs the live connector or opens Settings for a PAT."),
                (*tool).into(),
            ));
        }
    }
    out
}

pub fn skill_from_suggested(s: &SuggestedSkill) -> SkillMd {
    SkillMd {
        name: s.name.into(),
        description: s.body.into(),
        slash: format!("/{}", s.name),
        trigger: s.trigger.into(),
        instructions: s.instructions.into(),
        pitfalls: "Do not write secrets into markdown.".into(),
        verify: s.verify.into(),
        runs: 0,
    }
}

pub fn skill_matches(name: &str, description: &str, q: &str) -> bool {
    let q = q.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    name.to_ascii_lowercase().contains(&q) || description.to_ascii_lowercase().contains(&q)
}

pub fn starter_skill(name: &str) -> SkillMd {
    SkillMd {
        name: name.into(),
        description: format!("Cabin skill {name}"),
        slash: format!("/{name}"),
        trigger: name.into(),
        instructions: format!("Follow skill {name}. Stay concrete."),
        pitfalls: "Do not write secrets into markdown.".into(),
        verify: "echo VERIFY_OK".into(),
        runs: 0,
    }
}

pub fn page_header(ui: &mut egui::Ui, title: &str, action: &str) -> bool {
    let mut clicked = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .font(crate::theme::title_font(36.0))
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
    crate::theme::pointing(ui.add(
        egui::Button::new(
            RichText::new(label)
                .size(crate::theme::FONT_CHROME)
                .strong()
                .color(crate::theme::bg()),
        )
        .fill(crate::theme::fg())
        .rounding(crate::theme::HIT)
        .min_size(egui::vec2(0.0, crate::theme::HIT)),
    ))
    .clicked()
}

pub fn ghost_pill(ui: &mut egui::Ui, label: &str) -> bool {
    crate::theme::pointing(ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(crate::theme::muted()))
            .fill(egui::Color32::TRANSPARENT)
            .rounding(14.0)
            .stroke(Stroke::new(1.0_f32, crate::theme::border())),
    ))
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

pub fn object_chip(ui: &mut egui::Ui, kind: &str, label: &str) {
    let tone = match kind {
        "wont" => ChipTone::Offline,
        "cursor" => ChipTone::Live,
        _ => ChipTone::Setup,
    };
    let color = chip_tone_color(tone);
    egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(12.0)
        .stroke(Stroke::new(1.0_f32, color))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("{kind} {label}"))
                    .size(12.0)
                    .color(color),
            );
        });
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
        ("ask", "Ask"),
    ]
}

pub fn permission_modes() -> &'static [(&'static str, &'static str)] {
    &[
        ("ask", "Ask"),
        ("auto", "Auto"),
        ("always-approve", "Always"),
    ]
}

pub fn mode_label(id: &str) -> &'static str {
    match id {
        "plan" => "Plan",
        "ask" => "Ask",
        "chat" | "code" | "normal" | "build" => "Chat",
        "max" | "deep" | "heavy" => "Max",
        "think" | "expert" => "Think",
        "balanced" | "balance" => "Balance",
        "fast" => "Fast",
        "always-approve" | "always" | "yolo" => "Always",
        "auto" => "Auto",
        _ => "Chat",
    }
}

pub fn perm_label(id: &str) -> &'static str {
    match id {
        "auto" => "Auto",
        "always-approve" | "always" | "yolo" => "Always",
        _ => "Ask",
    }
}

fn catalog_pill(
    ui: &mut egui::Ui,
    popup_id: &'static str,
    current: &str,
    items: &[(&'static str, &'static str)],
    label: &str,
) -> Option<String> {
    let mut next = None;
    let id = ui.make_persistent_id(popup_id);
    let resp = crate::theme::pointing(ui.add(
        egui::Button::new(
            RichText::new(label)
                .size(crate::theme::FONT_CHROME)
                .color(crate::theme::muted()),
        )
        .fill(Color32::TRANSPARENT)
        .rounding(14.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::border()))
        .min_size(egui::vec2(MODE_PILL_W, 28.0)),
    ));
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

/// Compact session picker (Chat / Plan / Ask).
pub fn mode_pill(ui: &mut egui::Ui, current: &str) -> Option<String> {
    catalog_pill(
        ui,
        "composer-session-pop",
        current,
        composer_modes(),
        mode_label(current),
    )
}

/// Permission Ask / Auto / Always-approve.
pub fn perm_pill(ui: &mut egui::Ui, current: &str) -> Option<String> {
    catalog_pill(
        ui,
        "composer-perm-pop",
        current,
        permission_modes(),
        perm_label(current),
    )
}

pub struct SessionRowOut {
    pub mode: Option<String>,
    pub perm: Option<String>,
}

/// Chat / Plan / Ask and Ask / Auto / Always as a row above the composer.
pub fn session_row(ui: &mut egui::Ui, mode: &str, perm: &str) -> SessionRowOut {
    let mut out = SessionRowOut {
        mode: None,
        perm: None,
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (id, label) in composer_modes() {
            let on = *id == mode;
            let hit = crate::theme::pointing(ui.add(
                egui::Button::new(
                    RichText::new(*label)
                        .size(crate::theme::FONT_CHROME)
                        .color(if on {
                            crate::theme::fg()
                        } else {
                            crate::theme::muted()
                        }),
                )
                .fill(if on {
                    crate::theme::nav_active()
                } else {
                    Color32::TRANSPARENT
                })
                .rounding(14.0)
                .min_size(egui::vec2(52.0, 28.0)),
            ));
            if hit.clicked() && !on {
                out.mode = Some((*id).to_string());
            }
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new("|")
                .size(crate::theme::FONT_META)
                .color(crate::theme::border()),
        );
        ui.add_space(6.0);
        for (id, label) in permission_modes() {
            let on = *id == perm;
            let hit = crate::theme::pointing(ui.add(
                egui::Button::new(
                    RichText::new(*label)
                        .size(crate::theme::FONT_CHROME)
                        .color(if on {
                            crate::theme::fg()
                        } else {
                            crate::theme::muted()
                        }),
                )
                .fill(if on {
                    crate::theme::nav_active()
                } else {
                    Color32::TRANSPARENT
                })
                .rounding(14.0)
                .min_size(egui::vec2(52.0, 28.0)),
            ));
            if hit.clicked() && !on {
                out.perm = Some((*id).to_string());
            }
        }
    });
    ui.add_space(8.0);
    out
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
    const MAX: usize = 22;
    let t = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= MAX {
        return t;
    }
    let mut out = String::new();
    for (i, ch) in t.chars().enumerate() {
        if i + 1 >= MAX {
            break;
        }
        out.push(ch);
    }
    format!("{}…", out.trim_end())
}

/// Max width of the chip cluster — the composer pill, not a forced full-width strip.
pub fn chip_row_width_lock(avail: f32) -> f32 {
    crate::theme::QUERY_MAX_W.min(avail.max(120.0))
}

pub fn quick_chip_fill(_primary: bool) -> Color32 {
    crate::theme::elevated()
}

pub fn quick_chip_stroke(_primary: bool) -> Color32 {
    crate::theme::border()
}

pub fn quick_chip_fg(_primary: bool) -> Color32 {
    crate::theme::fg()
}

pub fn quick_chip_inner_button_framed() -> bool {
    false
}

pub fn quick_chip_row(ui: &mut egui::Ui, chips: &[grokhub_core::QuickChip]) -> Option<ChipRowAct> {
    if chips.is_empty() {
        return None;
    }
    let mut act = None;
    let max_w = chip_row_width_lock(ui.available_width());
    ui.scope(|ui| {
        ui.set_max_width(max_w);
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::Center)
                .with_main_wrap(true)
                .with_main_align(egui::Align::Center),
            |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for (i, c) in chips.iter().take(grokhub_core::CHIP_VISIBLE_MAX).enumerate() {
                let chip_id = ui.id().with(("qchip", i));
                let hovered = ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(chip_id))
                    .unwrap_or(false);
                let fill = quick_chip_fill(c.primary);
                let stroke = quick_chip_stroke(c.primary);
                let color = quick_chip_fg(c.primary);
                let paint = chip_paint_label(&c.label);
                let tip = if paint != c.label {
                    if c.hint.is_empty() {
                        c.label.clone()
                    } else {
                        format!("{}\n{}", c.label, c.hint)
                    }
                } else if c.hint.is_empty() {
                    c.value.clone()
                } else {
                    c.hint.clone()
                };
                let ir = egui::Frame::none()
                    .fill(fill)
                    .rounding(18.0)
                    .stroke(Stroke::new(1.0_f32, stroke))
                    .inner_margin(egui::Margin::symmetric(12.0, 6.0))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
                        ui.horizontal(|ui| {
                            let hit = crate::theme::pointing(
                                ui.add(
                                    egui::Button::new(
                                        RichText::new(paint).size(13.0).color(color),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::NONE)
                                    .frame(quick_chip_inner_button_framed()),
                                ),
                            )
                            .on_hover_text(tip);
                            if hit.clicked() {
                                act = Some(ChipRowAct::Apply(i));
                            }
                            let x = crate::theme::pointing(
                                ui.add(
                                    egui::Button::new(
                                        RichText::new(if hovered { "×" } else { " " })
                                            .size(12.0)
                                            .color(if hovered {
                                                crate::theme::subtle()
                                            } else {
                                                Color32::TRANSPARENT
                                            }),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::NONE)
                                    .frame(quick_chip_inner_button_framed())
                                    .min_size(egui::vec2(16.0, 16.0)),
                                ),
                            );
                            if hovered {
                                let x = x.on_hover_text("Hide this suggestion");
                                if x.clicked() {
                                    act = Some(ChipRowAct::Dismiss(i));
                                }
                            }
                        });
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
    });
    ui.add_space(8.0);
    act
}

pub fn tab_pill(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    crate::theme::pointing(ui.add(
        egui::Button::new(RichText::new(label).size(13.0).strong().color(if active {
            crate::theme::bg()
        } else {
            crate::theme::muted()
        }))
        .fill(if active {
            crate::theme::fg()
        } else {
            Color32::TRANSPARENT
        })
        .rounding(18.0)
        .min_size(egui::vec2(0.0, 32.0))
        .stroke(Stroke::new(
            1.0_f32,
            if active {
                crate::theme::fg()
            } else {
                crate::theme::border()
            },
        )),
    ))
    .clicked()
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

pub fn settings_group(ui: &mut egui::Ui, title: &str, mut body: impl FnMut(&mut egui::Ui)) {
    section_label(ui, title);
    egui::Frame::none()
        .fill(crate::theme::surface())
        .rounding(16.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::symmetric(16.0, 6.0))
        .show(ui, |ui| {
            body(ui);
        });
    ui.add_space(18.0);
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
    let fill = if on {
        crate::theme::fg()
    } else {
        crate::theme::panel()
    };
    let (resp, rect, fill) = crate::theme::feel_response(ui, resp, fill);
    ui.painter().rect_filled(rect, 12.0, fill);
    if !on {
        ui.painter()
            .rect_stroke(rect, 12.0, Stroke::new(1.0_f32, crate::theme::border_strong()));
    }
    let knob_x = if on {
        rect.right() - 12.0
    } else {
        rect.left() + 12.0
    };
    let knob = if on {
        crate::theme::bg()
    } else {
        crate::theme::muted()
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 8.0, knob);
    resp.clicked()
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
    crate::theme::pointing(ui.add(
        egui::Button::new(
            RichText::new(label)
                .size(crate::theme::FONT_CHROME)
                .color(if active {
                    crate::theme::fg()
                } else {
                    crate::theme::muted()
                }),
        )
        .fill(if active {
            crate::theme::nav_active()
        } else {
            Color32::TRANSPARENT
        })
        .rounding(10.0)
        .min_size(egui::vec2(188.0, 36.0)),
    ))
    .clicked()
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
    let resp = egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(18.0)
        .stroke(Stroke::new(
            1.0_f32,
            if selected {
                crate::theme::fg()
            } else {
                crate::theme::border()
            },
        ))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_min_height(108.0);
            ui.horizontal(|ui| {
                icons::paint_icon(ui, icon, 40.0);
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(15.0).strong().color(crate::theme::fg()));
                    ui.add_space(4.0);
                    let clipped: String = body.chars().take(80).collect();
                    ui.label(RichText::new(clipped).size(12.0).color(crate::theme::muted()));
                });
                if let Some(label) = add {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        add_clicked = white_pill(ui, label);
                    });
                }
            });
        })
        .response
        .interact(Sense::click());
    let (resp, felt, wash) = crate::theme::feel_response(ui, resp, Color32::TRANSPARENT);
    if wash.a() > 0 {
        ui.painter().rect_filled(felt, 18.0, wash);
    }
    if add_clicked {
        hit = TileHit::Add;
    } else if resp.clicked() {
        hit = TileHit::Body;
    }
    if selected || resp.hovered() {
        ui.painter().rect_stroke(
            felt,
            18.0,
            Stroke::new(1.0_f32, crate::theme::border_strong()),
        );
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
            for c in 0..cols {
                let i = r * cols + c;
                if i < n {
                    each(&mut col_uis[c], i);
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

/// Generated still, letterboxed in the wall slot above the docked chat box.
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
    let (resp, _felt, wash) = crate::theme::feel_response(ui, resp, Color32::TRANSPARENT);
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
    if selected || resp.hovered() {
        ui.painter()
            .rect_stroke(rect, 0.0, Stroke::new(1.0_f32, crate::theme::fg()));
    }
    if wash.a() > 0 {
        ui.painter().rect_filled(rect, 0.0, wash);
    }
    resp.clicked()
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
    let (resp, _felt, wash) = crate::theme::feel_response(ui, resp, Color32::TRANSPARENT);
    let n = if gif.path_b.is_empty() { 1 } else { 2 };
    let tick = (now_ms / crate::theme::IMAGINE_FRAME_MS) as usize + gif.title.len();
    let path_a = if tick % n == 0 {
        gif.path_a.as_str()
    } else {
        gif.path_b.as_str()
    };
    let path_b = if tick % n == 0 {
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
    if selected || resp.hovered() {
        ui.painter()
            .rect_stroke(rect, 0.0, Stroke::new(1.0_f32, crate::theme::fg()));
    }
    if wash.a() > 0 {
        ui.painter().rect_filled(rect, 0.0, wash);
    }
    resp.clicked()
}

pub fn suggestion_card(ui: &mut egui::Ui, title: &str, body: &str) -> bool {
    grok_tile(ui, icons::icon_for_label(title), title, body, Some("Add"), false) == TileHit::Add
}

pub fn catalog_card(ui: &mut egui::Ui, title: &str, body: &str, selected: bool) -> bool {
    grok_tile(ui, icons::icon_for_label(title), title, body, None, selected) == TileHit::Body
}

pub fn empty_prompt_tile(ui: &mut egui::Ui, icon: TileIcon, title: &str, hint: &str) -> bool {
    let mut hit = false;
    let resp = egui::Frame::none()
        .fill(crate::theme::elevated())
        .rounding(18.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_min_height(112.0);
            ui.vertical_centered(|ui| {
                icons::paint_icon(ui, icon, 36.0);
                ui.add_space(8.0);
                ui.label(RichText::new(title).size(14.0).strong().color(crate::theme::fg()));
                ui.add_space(4.0);
                ui.label(RichText::new(hint).size(12.0).color(crate::theme::muted()));
            });
        })
        .response
        .interact(Sense::click());
    let (resp, felt, wash) = crate::theme::feel_response(ui, resp, Color32::TRANSPARENT);
    if wash.a() > 0 {
        ui.painter().rect_filled(felt, 18.0, wash);
    }
    if resp.clicked() {
        hit = true;
    }
    if resp.hovered() {
        ui.painter()
            .rect_stroke(felt, 18.0, Stroke::new(1.0_f32, crate::theme::border_strong()));
    }
    hit
}

pub fn chip_icon(label: &str) -> TileIcon {
    icons::icon_for_label(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::parse_nl_automation;

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
        assert!(long.chars().count() <= 22, "{long}");
        assert!(long.ends_with('…'), "{long}");
    }

    #[test]
    fn mode_pill_fits_the_composer_cluster() {
        assert_eq!(MODE_PILL_W, 84.0);
        assert_eq!(
            composer_go_cluster_w(),
            22.0 + 28.0 + 8.0 * 3.0 + 12.0
        );
        assert_eq!(mode_label("plan"), "Plan");
        assert_eq!(mode_label("code"), "Chat");
        assert_eq!(mode_label("chat"), "Chat");
        assert_eq!(mode_label("mystery"), "Chat");
        assert_eq!(perm_label("always-approve"), "Always");
        assert_eq!(composer_modes().len(), 3);
        assert_eq!(permission_modes().len(), 3);
        assert_eq!(clip_status("one\ntwo", 80), "one");
        assert_eq!(clip_status("abcdefghij", 6), "abcde…");
        assert_eq!(chip_tone_color(ChipTone::Offline), crate::theme::offline());
    }

    #[test]
    fn first_quick_chip_is_inline_not_selected() {
        assert_eq!(quick_chip_fill(true), crate::theme::elevated());
        assert_eq!(quick_chip_fill(true), quick_chip_fill(false));
        assert_eq!(quick_chip_stroke(true), quick_chip_stroke(false));
        assert_eq!(quick_chip_fg(true), crate::theme::fg());
        let max_w = chip_row_width_lock(640.0);
        assert_eq!(max_w, crate::theme::QUERY_MAX_W.min(640.0_f32.max(120.0)));
        assert!(max_w <= crate::theme::QUERY_MAX_W);
        assert_ne!(max_w, 0.0);
        assert!(!quick_chip_inner_button_framed());
        let src = include_str!("cards.rs");
        let start = src.find("pub fn quick_chip_row").expect("chip row");
        let slice = &src[start..start + 900];
        assert!(
            slice.contains("with_main_align(egui::Align::Center)"),
            "chips sit on the midline of the bar: {slice}"
        );
        assert!(
            !slice.contains("set_width(max_w)") && !slice.contains("set_min_width"),
            "chip cluster must shrink-wrap, not fill the pill: {slice}"
        );
    }

    #[test]
    fn suggested_autos_parse() {
        assert_eq!(SUGGESTED_AUTOS.len(), 7);
        for s in SUGGESTED_AUTOS {
            let a = parse_nl_automation(s.seed).expect(s.title);
            assert!(!a.instructions.is_empty());
            assert!(a.enabled);
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

        let learned_skill = LearnedSuggestion {
            kind: SuggestionKind::Skill,
            title: "Desk tidy".into(),
            body: "Straighten windows".into(),
            seed: None,
            name: Some("desk-tidy".into()),
            trigger: Some("messy desk".into()),
            instructions: Some("stack the windows".into()),
            provider: None,
            tool: None,
        };
        let skills = merge_suggested_skills(&[learned_skill], &[]);
        assert_eq!(skills[0].name.as_deref(), Some("desk-tidy"));
        assert!(skills.iter().any(|s| s.name.as_deref() == Some("morning-brief")));

        let learned_conn = LearnedSuggestion {
            kind: SuggestionKind::Connector,
            title: "My GitHub".into(),
            body: "Who am I".into(),
            seed: None,
            name: None,
            trigger: None,
            instructions: None,
            provider: Some("github".into()),
            tool: Some("user".into()),
        };
        let conns = merge_suggested_connectors(&[learned_conn]);
        assert_eq!(conns[0].1, "My GitHub");
        assert_eq!(conns[0].3, "user");
        assert!(!conns.iter().any(|t| t.1 == "Who am I" && t.3 == "user"));
        assert!(conns.iter().any(|t| t.3 == "list_repos"));
    }

    #[test]
    fn skill_search() {
        assert!(skill_matches("morning-brief", "cabin brief", "morn"));
        assert!(skill_matches("host-snapshot", "uname whoami", "whoami"));
        assert!(!skill_matches("morning-brief", "cabin brief", "pdf"));
        assert!(skill_matches("PDFs", "merge split", ""));
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
        assert_eq!(SUGGESTED_SKILLS.len(), 8);
        for s in SUGGESTED_SKILLS {
            let blob = format!("{} {} {}", s.name, s.body, s.instructions).to_ascii_lowercase();
            for w in forbidden {
                assert!(!blob.contains(w), "skill {} mentions {w}", s.name);
            }
            let real = blob.contains("host_cmd")
                || blob.contains("workboard")
                || blob.contains("imagine")
                || blob.contains("verify")
                || blob.contains("connector_cmd")
                || blob.contains("computer_cmd")
                || blob.contains("computer-use")
                || blob.contains("grok build")
                || blob.contains("uname");
            assert!(real, "skill {} is not a cabin verb", s.name);
            let md = skill_from_suggested(s);
            assert_eq!(md.name, s.name);
            assert!(!md.instructions.is_empty());
            let _ = s.icon;
        }
        assert_eq!(LIVE_CONNECTORS.len(), 1);
        assert_eq!(LIVE_CONNECTORS[0].id, "github");
        assert!(LIVE_CONNECTORS[0].tools.iter().any(|(l, t)| *l == "Who am I" && *t == "user"));
        assert!(LIVE_CONNECTORS[0].tools.iter().any(|(l, t)| *t == "list_repos"));
        assert!(LIVE_CONNECTORS[0].tools.iter().any(|(l, t)| *t == "list_issues"));
        assert!(LIVE_CONNECTORS[0].tools.iter().any(|(l, t)| *t == "search_code"));
        assert!(LIVE_CONNECTORS[0].tools.iter().any(|(l, t)| *t == "search_issues"));
        assert!(!LIVE_CONNECTORS[0].tools.iter().any(|(_, t)| *t == "create_pr_comment"));
        assert!(!is_cabin_catalog("outlook"));
        assert!(!is_cabin_catalog("gmail"));
        assert_eq!(IMAGINE_SCENES.len(), 9);
        assert_eq!(imagine_word(0), "the cabin");
        assert_eq!(imagine_word(2800), "the night");
        assert_eq!(imagine_aspect_label(0), "2:3");
        assert_eq!(imagine_aspect_label(1), "3:2");
        assert_eq!(imagine_aspect_label(2), "1:1");
        assert_eq!(imagine_aspect_label(3), "9:16");
        assert_eq!(imagine_aspect_label(4), "16:9");
        assert_eq!(grokhub_core::imagine_aspect_name(0), "Tall");
        assert_eq!(IMAGINE_BAR_CHIPS, ["Image", "Video", "Agent", "Speed", "Quality (v2.0)", "Auto"]);
        assert_eq!(IMAGINE_VIDEO_CHIPS, ["480p", "720p", "6s", "10s", "15s", "Video audio"]);
        assert_eq!(
            imagine_toolbar_labels(ImagineKind::Image, false),
            [
                "Image",
                "Video",
                "Agent",
                "Speed",
                "Quality (v2.0)",
                "Auto",
                "2:3",
                "Connect Grok",
            ]
        );
        assert_eq!(
            imagine_toolbar_labels(ImagineKind::Video, true),
            [
                "Image",
                "Video",
                "Agent",
                "480p",
                "720p",
                "6s",
                "10s",
                "15s",
                "Video audio",
                "Auto",
                "2:3",
            ]
        );
        assert!(
            imagine_chip_row_w(crate::theme::IMAGINE_BAR_W)
                > imagine_send_cluster_w() + crate::theme::IMAGINE_HIT * 3.0,
            "chip row must leave a clickable strip beside send/mic"
        );
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
            crate::theme::QUERY_MAX_W
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
        let bar_inner = crate::theme::IMAGINE_BAR_H - 20.0;
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
        assert_eq!(imagine_kind_label(ImagineKind::Image), "Image");
        assert_eq!(imagine_kind_label(ImagineKind::Video), "Video");
        assert_eq!(imagine_kind_label(ImagineKind::Agent), "Agent");
        assert_eq!(imagine_quality_label(false), "Speed");
        assert_eq!(imagine_quality_label(true), "Quality (v2.0)");
        let fallback = imagine_still_rgba(b"not-a-jpeg");
        assert_eq!((fallback.width(), fallback.height()), (1, 1));
        assert_eq!(imagine_quality_word(true), "quality");
        assert_eq!(imagine_quality_word(false), "speed");
        assert_eq!(
            imagine_still_prompt("a night cabin", "2:3", true),
            "a night cabin, 2:3 quality still"
        );
        assert_eq!(
            imagine_still_prompt("a night cabin, 2:3 still", "2:3", false),
            "a night cabin, 2:3 still, speed still"
        );
        assert_eq!(imagine_still_prompt("", "2:3", true), "");
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
                let img = image::load_from_memory(bytes).expect(*key);
                assert!(img.width() >= 256);
                assert!(img.height() >= 256);
            }
            let a = imagine_frame_key(s, 0);
            let b = imagine_frame_key(s, crate::theme::IMAGINE_FRAME_MS);
            assert_ne!(a, b, "imagine {} cover must change", s.title);
            let (_, _, fade0) = imagine_frame_pair(s, 0);
            let (_, _, fade1) = imagine_frame_pair(s, crate::theme::IMAGINE_FRAME_MS - 1);
            assert!(fade0 < 0.05);
            assert!(fade1 > 0.9);
        }
        let uv = cover_uv(768.0, 512.0, 345.0, 230.0);
        assert!(uv.width() > 0.4 && uv.height() > 0.9);
        assert!(is_cabin_catalog("github"));
        let pulse = SUGGESTED_SKILLS.iter().find(|s| s.name == "github-pulse").unwrap();
        let cmds = grokhub_core::extract_connector_cmds(pulse.instructions);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].tool, "user");
        assert_eq!(cmds[1].tool, "list_repos");
        let triage = SUGGESTED_SKILLS.iter().find(|s| s.name == "workboard-triage").unwrap();
        assert!(triage.instructions.contains("WORK_PIN:"));
        let img = SUGGESTED_SKILLS.iter().find(|s| s.name == "imagine-scene").unwrap();
        assert!(img.instructions.contains("IMAGINE_PROMPT:"));
        use grokhub_core::github_api_path;
        assert!(github_api_path("user", "").is_ok());
        assert!(github_api_path("list_repos", "").is_ok());
        assert!(github_api_path("list_issues", "repo:owner/name").is_ok());
        assert!(github_api_path("search_code", "query:foo").is_ok());
        assert!(github_api_path("search_issues", "query:foo").is_ok());
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
}
