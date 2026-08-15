//! Grok catalog chrome — huge title, white pills, 3-column icon tiles.

use crate::icons::{self, TileIcon};
use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use grokhub_core::SkillMd;

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
                .color(crate::theme::FG),
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
    ui.add(
        egui::Button::new(RichText::new(label).size(13.0).strong().color(crate::theme::BG))
            .fill(crate::theme::FG)
            .rounding(18.0)
            .min_size(egui::vec2(0.0, 34.0)),
    )
    .clicked()
}

pub fn ghost_pill(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(crate::theme::MUTED))
            .fill(egui::Color32::TRANSPARENT)
            .rounding(14.0)
            .stroke(Stroke::new(1.0_f32, crate::theme::BORDER)),
    )
    .clicked()
}

pub fn tab_pill(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    ui.add(
        egui::Button::new(RichText::new(label).size(13.0).strong().color(if active {
            crate::theme::BG
        } else {
            crate::theme::MUTED
        }))
        .fill(if active {
            crate::theme::FG
        } else {
            Color32::TRANSPARENT
        })
        .rounding(18.0)
        .min_size(egui::vec2(0.0, 32.0))
        .stroke(Stroke::new(
            1.0_f32,
            if active {
                crate::theme::FG
            } else {
                crate::theme::BORDER
            },
        )),
    )
    .clicked()
}

pub fn section_label(ui: &mut egui::Ui, label: &str) {
    ui.label(RichText::new(label).size(13.0).strong().color(crate::theme::SUBTLE));
    ui.add_space(10.0);
}

pub fn search_field(ui: &mut egui::Ui, q: &mut String) {
    egui::Frame::none()
        .fill(crate::theme::ELEVATED)
        .rounding(18.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::BORDER))
        .inner_margin(egui::Margin::symmetric(10.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::paint_bar_icon(ui, icons::BarIcon::Search, 16.0, crate::theme::SUBTLE);
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
        .fill(crate::theme::ELEVATED)
        .rounding(18.0)
        .stroke(Stroke::new(
            1.0_f32,
            if selected {
                crate::theme::FG
            } else {
                crate::theme::BORDER
            },
        ))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_min_height(108.0);
            ui.horizontal(|ui| {
                icons::paint_icon(ui, icon, 40.0);
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(15.0).strong().color(crate::theme::FG));
                    ui.add_space(4.0);
                    let clipped: String = body.chars().take(80).collect();
                    ui.label(RichText::new(clipped).size(12.0).color(crate::theme::MUTED));
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
    if add_clicked {
        hit = TileHit::Add;
    } else if resp.clicked() {
        hit = TileHit::Body;
    }
    if resp.hovered() {
        ui.painter().rect_stroke(
            resp.rect,
            18.0,
            Stroke::new(1.0, crate::theme::BORDER_STRONG),
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
    let cols = if w >= 720.0 {
        3
    } else if w >= 480.0 {
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

pub fn suggestion_card(ui: &mut egui::Ui, title: &str, body: &str) -> bool {
    grok_tile(ui, icons::icon_for_label(title), title, body, Some("Add"), false) == TileHit::Add
}

pub fn catalog_card(ui: &mut egui::Ui, title: &str, body: &str, selected: bool) -> bool {
    grok_tile(ui, icons::icon_for_label(title), title, body, None, selected) == TileHit::Body
}

pub fn empty_prompt_chip(ui: &mut egui::Ui, icon: TileIcon, title: &str) -> bool {
    let mut hit = false;
    let resp = egui::Frame::none()
        .fill(crate::theme::ELEVATED)
        .rounding(20.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::BORDER))
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::paint_icon(ui, icon, 28.0);
                ui.add_space(8.0);
                ui.label(RichText::new(title).size(13.0).color(crate::theme::FG));
            });
        })
        .response
        .interact(Sense::click());
    if resp.clicked() {
        hit = true;
    }
    if resp.hovered() {
        ui.painter()
            .rect_stroke(resp.rect, 20.0, Stroke::new(1.0, crate::theme::BORDER_STRONG));
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
