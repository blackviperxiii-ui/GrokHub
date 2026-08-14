//! Grok-style page chrome — large titles, white pills, suggestion cards.

use eframe::egui::{self, Color32, RichText, Stroke};
use grokhub_core::{parse_nl_automation, SkillMd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuggestedAuto {
    pub title: &'static str,
    pub body: &'static str,
    pub seed: &'static str,
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
            ui.set_min_width(200.0);
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
}
