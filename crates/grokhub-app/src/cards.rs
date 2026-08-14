//! Grok catalog chrome — huge title, white pills, 3-column icon tiles.

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, Vec2};
use grokhub_core::SkillMd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuggestedAuto {
    pub glyph: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    pub seed: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuggestedSkill {
    pub glyph: &'static str,
    pub name: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    pub trigger: &'static str,
    pub instructions: &'static str,
    pub verify: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveConnector {
    pub glyph: &'static str,
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
        glyph: "☀",
        name: "morning-brief",
        title: "Morning brief",
        body: "Summarize the workboard, last host receipt, and pinned goal.",
        trigger: "morning brief",
        instructions: "Summarize the Workboard and last HOST_RESULT from the system context. List open cards and the next concrete step. If the board is empty, say so. No secrets.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        glyph: "⌘",
        name: "host-snapshot",
        title: "Host snapshot",
        body: "Read-only uname / whoami / pwd via HOST_CMD.",
        trigger: "host snapshot",
        instructions: "Emit HOST_CMD: uname -a && whoami && pwd. After HOST_RESULT, summarize in four lines.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        glyph: "☰",
        name: "workboard-triage",
        title: "Workboard triage",
        body: "Pull open tasks from this thread onto the workboard.",
        trigger: "triage the board",
        instructions: "Extract open tasks from this thread. For each task emit one line exactly: WORK_PIN: short title | one-line detail | priority=med. Do not emit WORK_UPDATE done without VERIFY_OK.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        glyph: "◈",
        name: "imagine-scene",
        title: "Imagine a scene",
        body: "Write a tight still-image prompt; the cabin generates it.",
        trigger: "imagine this",
        instructions: "Write one tight still-image prompt (grok-2-image, no faces). Emit one line exactly: IMAGINE_PROMPT: <prompt>. Stay in the bound project.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        glyph: "⌥",
        name: "github-pulse",
        title: "GitHub pulse",
        body: "Who am I plus recent repos via CONNECTOR_CMD (needs a PAT).",
        trigger: "github pulse",
        instructions: "Emit these two lines exactly, each on its own line:\nCONNECTOR_CMD: github user\nCONNECTOR_CMD: github list_repos\nAfter both CONNECTOR_RESULT blocks, summarize in four lines. No secrets.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        glyph: "✓",
        name: "verify-last",
        title: "Verify last turn",
        body: "Run verify.sh and hold done until VERIFY_OK.",
        trigger: "verify this",
        instructions: "Run the skill verify.sh. After the output, report VERIFY_OK or the exact failure. Do not mark workboard cards done without VERIFY_OK.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        glyph: "◎",
        name: "board-status",
        title: "Board status",
        body: "List open workboard cards and the next concrete step.",
        trigger: "board status",
        instructions: "List every open Workboard card from the system context. One line each. Then the next concrete step. If empty, say so. Optional: WORK_PIN: follow-up | detail | priority=low",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        glyph: "⌁",
        name: "host-health",
        title: "Host health",
        body: "Richer read-only snapshot: load, disk, and grokhub paths.",
        trigger: "host health",
        instructions: "Emit HOST_CMD: uname -a && whoami && pwd && df -h / && uptime. After HOST_RESULT, four lines: machine, user, disk, load. No secrets.",
        verify: "echo VERIFY_OK",
    },
];

pub const LIVE_CONNECTORS: &[LiveConnector] = &[LiveConnector {
    glyph: "G",
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
        glyph: "☀",
        title: "Morning brief",
        body: "Weekdays at 09:00 — workboard and last host receipt.",
        seed: "every weekday at 9, summarize the workboard",
    },
    SuggestedAuto {
        glyph: "⌁",
        title: "Host heartbeat",
        body: "Every 60 min — read-only snapshot, then a short note.",
        seed: "heartbeat every 60 min, run a read-only host snapshot and summarize",
    },
    SuggestedAuto {
        glyph: "☰",
        title: "Task extractor",
        body: "Weekdays at 18:00 — pull today's open tasks onto the board.",
        seed: "every weekday at 18, extract open tasks onto the workboard",
    },
    SuggestedAuto {
        glyph: "⌘",
        title: "Dawn snapshot",
        body: "Weekdays at 08:00 — read-only host snapshot before the day.",
        seed: "every weekday at 8, run a read-only host snapshot and summarize",
    },
    SuggestedAuto {
        glyph: "◎",
        title: "Midday board",
        body: "Weekdays at 12:00 — summarize the workboard.",
        seed: "every weekday at 12, summarize the workboard",
    },
    SuggestedAuto {
        glyph: "☾",
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
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(36.0).strong().color(crate::theme::FG));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            clicked = ui
                .add(
                    egui::Button::new(RichText::new(action).size(14.0).strong().color(crate::theme::BG))
                        .fill(crate::theme::FG)
                        .rounding(20.0)
                        .min_size(egui::vec2(0.0, 36.0)),
                )
                .clicked();
        });
    });
    ui.add_space(18.0);
    clicked
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
        .inner_margin(egui::Margin::symmetric(12.0, 6.0))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(q)
                    .hint_text("Search")
                    .desired_width(200.0)
                    .frame(false),
            );
        });
}

fn glyph_circle(ui: &mut egui::Ui, glyph: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 20.0, crate::theme::SURFACE);
    painter.circle_stroke(rect.center(), 20.0, Stroke::new(1.0, crate::theme::BORDER_STRONG));
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(16.0),
        crate::theme::FG,
    );
}

pub fn grok_tile(
    ui: &mut egui::Ui,
    glyph: &str,
    title: &str,
    body: &str,
    add: Option<&str>,
    selected: bool,
) -> TileHit {
    let mut hit = TileHit::None;
    let mut add_clicked = false;
    let resp = egui::Frame::none()
        .fill(crate::theme::ELEVATED)
        .rounding(16.0)
        .stroke(Stroke::new(
            1.0_f32,
            if selected {
                crate::theme::FG
            } else {
                crate::theme::BORDER
            },
        ))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_min_height(118.0);
            ui.horizontal(|ui| {
                glyph_circle(ui, glyph);
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(16.0).strong().color(crate::theme::FG));
                    ui.add_space(4.0);
                    ui.label(RichText::new(body).size(13.0).color(crate::theme::MUTED));
                });
                if let Some(label) = add {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        add_clicked = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(label).size(13.0).strong().color(crate::theme::BG),
                                )
                                .fill(crate::theme::FG)
                                .rounding(14.0)
                                .min_size(egui::vec2(52.0, 28.0)),
                            )
                            .clicked();
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
            16.0,
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
    let cols = if w >= 720.0 {
        3
    } else if w >= 480.0 {
        2
    } else {
        1
    };
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
    grok_tile(ui, chip_glyph(title), title, body, Some("Add"), false) == TileHit::Add
}

pub fn catalog_card(ui: &mut egui::Ui, title: &str, body: &str, selected: bool) -> bool {
    grok_tile(ui, chip_glyph(title), title, body, None, selected) == TileHit::Body
}

pub fn empty_prompt_tile(ui: &mut egui::Ui, glyph: &str, title: &str, hint: &str) -> bool {
    grok_tile(ui, glyph, title, hint, None, false) == TileHit::Body
}

pub fn chip_glyph(label: &str) -> &'static str {
    let l = label.to_ascii_lowercase();
    if l.contains("connect") {
        "●"
    } else if l.contains("host") || l.contains("machine") {
        "⌘"
    } else if l.contains("imagine") || l.contains("draw") {
        "◈"
    } else if l.contains("think") || l.contains("harder") {
        "✦"
    } else if l.contains("help") || l.contains("what can") {
        "?"
    } else if l.contains("fix") || l.contains("debug") {
        "✗"
    } else {
        "→"
    }
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
            assert!(!s.glyph.is_empty());
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
            assert!(!s.glyph.is_empty());
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
