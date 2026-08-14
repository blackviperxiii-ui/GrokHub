//! Grok-style page chrome — large titles, white pills, suggestion cards.

use eframe::egui::{self, Color32, RichText, Stroke};
use grokhub_core::SkillMd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuggestedAuto {
    pub title: &'static str,
    pub body: &'static str,
    pub seed: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuggestedSkill {
    pub name: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    pub trigger: &'static str,
    pub instructions: &'static str,
    pub verify: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveConnector {
    pub id: &'static str,
    pub title: &'static str,
    pub tools: &'static [(&'static str, &'static str)],
}

pub const SUGGESTED_SKILLS: &[SuggestedSkill] = &[
    SuggestedSkill {
        name: "morning-brief",
        title: "Morning brief",
        body: "Summarize the workboard, last host receipt, and pinned goal.",
        trigger: "morning brief",
        instructions: "Summarize the Workboard and last HOST_RESULT from the system context. List open cards and the next concrete step. If the board is empty, say so. No secrets.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        name: "host-snapshot",
        title: "Host snapshot",
        body: "Read-only uname / whoami / pwd via HOST_CMD.",
        trigger: "host snapshot",
        instructions: "Emit HOST_CMD: uname -a && whoami && pwd. After HOST_RESULT, summarize in four lines.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        name: "workboard-triage",
        title: "Workboard triage",
        body: "Pull open tasks from this thread onto the workboard.",
        trigger: "triage the board",
        instructions: "Extract open tasks from this thread. For each task emit one line exactly: WORK_PIN: short title | one-line detail | priority=med. Do not emit WORK_UPDATE done without VERIFY_OK.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        name: "imagine-scene",
        title: "Imagine a scene",
        body: "Write a tight image prompt; cabin generates the still.",
        trigger: "imagine this",
        instructions: "Write one tight still-image prompt (grok-2-image, no faces). Emit one line exactly: IMAGINE_PROMPT: <prompt>. Stay in the bound project.",
        verify: "echo VERIFY_OK",
    },
    SuggestedSkill {
        name: "github-pulse",
        title: "GitHub pulse",
        body: "Who am I plus recent repos via CONNECTOR_CMD (needs a PAT).",
        trigger: "github pulse",
        instructions: "Emit these two lines exactly, each on its own line:\nCONNECTOR_CMD: github user\nCONNECTOR_CMD: github list_repos\nAfter both CONNECTOR_RESULT blocks, summarize in four lines. No secrets.",
        verify: "echo VERIFY_OK",
    },
];

pub const LIVE_CONNECTORS: &[LiveConnector] = &[LiveConnector {
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

pub const SUGGESTED_AUTOS: &[SuggestedAuto] = &[
    SuggestedAuto {
        title: "Morning brief",
        body: "Weekday 09:00 — summarize the workboard and last host receipt.",
        seed: "every weekday at 9, summarize the workboard",
    },
    SuggestedAuto {
        title: "Host heartbeat",
        body: "Every 60 min — read-only snapshot, then a short cabin note.",
        seed: "heartbeat every 60 min, run a read-only host snapshot and summarize",
    },
    SuggestedAuto {
        title: "Task extractor",
        body: "Weekday 18:00 — pull open tasks from today onto the workboard.",
        seed: "every weekday at 18, extract open tasks onto the workboard",
    },
];

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
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(28.0).strong().color(crate::theme::FG));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            clicked = ui
                .add(
                    egui::Button::new(RichText::new(action).strong().color(crate::theme::BG))
                        .fill(crate::theme::FG)
                        .rounding(16.0),
                )
                .clicked();
        });
    });
    clicked
}

pub fn tab_pill(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    ui.add(
        egui::Button::new(RichText::new(label).size(13.0).color(if active {
            crate::theme::FG
        } else {
            crate::theme::MUTED
        }))
        .fill(if active {
            crate::theme::ELEVATED
        } else {
            Color32::TRANSPARENT
        })
        .rounding(16.0)
        .stroke(Stroke::new(
            1.0_f32,
            if active {
                crate::theme::BORDER_STRONG
            } else {
                crate::theme::BORDER
            },
        )),
    )
    .clicked()
}

pub fn suggestion_card(ui: &mut egui::Ui, title: &str, body: &str) -> bool {
    let mut add = false;
    egui::Frame::none()
        .fill(crate::theme::ELEVATED)
        .rounding(12.0)
        .stroke(Stroke::new(1.0_f32, crate::theme::BORDER))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_width(248.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).strong().color(crate::theme::FG));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    add = ui
                        .add(
                            egui::Button::new(RichText::new("Add").small().color(crate::theme::FG))
                                .fill(crate::theme::SURFACE)
                                .rounding(12.0),
                        )
                        .clicked();
                });
            });
            ui.label(RichText::new(body).small().color(crate::theme::MUTED));
        });
    add
}

pub fn catalog_card(ui: &mut egui::Ui, title: &str, body: &str, selected: bool) -> bool {
    let stroke = if selected {
        crate::theme::BORDER_STRONG
    } else {
        crate::theme::BORDER
    };
    egui::Frame::none()
        .fill(crate::theme::ELEVATED)
        .rounding(12.0)
        .stroke(Stroke::new(1.0_f32, stroke))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_min_width(220.0);
            ui.label(RichText::new(title).strong().size(15.0).color(crate::theme::FG));
            ui.label(RichText::new(body).small().color(crate::theme::MUTED));
        })
        .response
        .interact(egui::Sense::click())
        .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::parse_nl_automation;

    #[test]
    fn suggested_autos_parse() {
        assert_eq!(SUGGESTED_AUTOS.len(), 3);
        for s in SUGGESTED_AUTOS {
            let a = parse_nl_automation(s.seed).expect(s.title);
            assert!(!a.instructions.is_empty());
            assert!(a.enabled);
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
        assert!(!SUGGESTED_SKILLS.is_empty());
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
