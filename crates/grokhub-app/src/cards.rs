//! Grok catalog chrome — huge title, white pills, 3-column icon tiles.

use crate::icons::{self, TileIcon};
use eframe::egui::{self, Align2, Color32, ColorImage, FontId, RichText, Sense, Stroke, TextureHandle, TextureOptions};
use grokhub_core::{curate_wall, wall_curate_seed, SkillMd, WallGif, WallSlot};

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
        body: "Read-only uname / whoami / pwd via HOST_CMD.",
        trigger: "host snapshot",
        instructions: "Emit HOST_CMD: uname -a && whoami && pwd. After HOST_RESULT, summarize in four lines.",
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
        instructions: "Write one tight still-image prompt (grok-2-image, no faces). Emit one line exactly: IMAGINE_PROMPT: <prompt>. Stay in the bound project.",
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
        instructions: "Emit HOST_CMD: uname -a && whoami && pwd && df -h / && uptime. After HOST_RESULT, four lines: machine, user, disk, load. No secrets.",
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
];

/// grok.com/imagine rotating h1 noun — cabin-real only.
pub const IMAGINE_WORDS: &[&str] = &["the cabin", "the night", "a scene", "the board"];

/// Still-image seeds. grok-2-image only — no video, no photo-edit tools we do not have.
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

/// Speed / Quality and aspect are still-prompt words. grok-2-image has no other lever.
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
        .rounding(8.0)
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
    let (_rect, resp) = ui.allocate_exact_size(egui::vec2(108.0, 96.0), Sense::click());
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
}

pub fn search_field(ui: &mut egui::Ui, q: &mut String) {
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
                        .hint_text("Search")
                        .desired_width(180.0)
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
    let rgba = imagine_still_rgba(still_jpeg(key));
    let size = [rgba.width() as usize, rgba.height() as usize];
    let tex = ctx.load_texture(
        format!("imagine-still-{key}"),
        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        TextureOptions::LINEAR,
    );
    let hit = (tex, size);
    ctx.data_mut(|d| d.insert_temp(id, hit.clone()));
    hit
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
    let img = std::fs::read(path)
        .ok()
        .and_then(|b| image::load_from_memory(&b).ok())
        .unwrap_or_else(|| image::DynamicImage::new_rgb8(8, 8));
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let tex = ctx.load_texture(
        format!("imagine-disk-{path}"),
        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        TextureOptions::LINEAR,
    );
    let hit = (tex, size);
    ctx.data_mut(|d| d.insert_temp(id, hit.clone()));
    hit
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

#[allow(dead_code)]
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
    fn suggested_autos_parse() {
        assert_eq!(SUGGESTED_AUTOS.len(), 6);
        for s in SUGGESTED_AUTOS {
            let a = parse_nl_automation(s.seed).expect(s.title);
            assert!(!a.instructions.is_empty());
            assert!(a.enabled);
            let _ = s.icon;
        }
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
                || blob.contains("connector_cmd");
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
}
