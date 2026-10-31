//! Modular Mission Control Home page (deck slide 9 / Order -38).

use grokhub_core::home::{place_slots, GAP};

use super::super::loadout_home::{
    caption, grouped, level_xp, objective_progress, xp_hover, XP_PER_LEVEL,
};
use super::super::mission_page::{day_caption, greeting};
use super::super::*;
use super::edit::HomeEditDraft;
use super::paint::{paint_module, ModuleAction};

const PAGE_PAD: f32 = 24.0;
/// Chips + composer height before the first frame measures it.
const COMPOSE_H: f32 = 140.0;
const DRAFT_ID: &str = "home-mission-draft";
const COMPOSE_H_ID: &str = "home-mission-compose-h";

/// Default Home chips (Mission Control preset).
pub(in crate::app) const HOME_CHIPS: [&str; 4] = [
    "Fix my stutter",
    "Find a duo tonight",
    "Free up storage",
    "Plan my week",
];

impl Cabin {
    pub(in crate::app) fn ui_home_mission(&mut self, ui: &mut egui::Ui) {
        let mut actions = Vec::new();
        egui::Frame::NONE
            .fill(crate::theme::bg())
            .inner_margin(egui::Margin::same(PAGE_PAD as i8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let mission = self.dock_mission(ui.ctx());
                let done = mission
                    .as_ref()
                    .map_or(0, |m| objective_progress(&m.checklist).0);
                self.home_header(ui, done);
                ui.add_space(GAP);
                let w = ui.available_width();
                let compose_id = egui::Id::new(COMPOSE_H_ID);
                let compose_h = ui
                    .ctx()
                    .data(|d| d.get_temp::<f32>(compose_id))
                    .unwrap_or(COMPOSE_H);
                let body_h = (ui.available_height() - compose_h).max(240.0);
                let origin = ui.cursor().min;
                let placed = place_slots((0.0, 0.0, w, body_h), &self.home_layout);
                let edit_btn = egui::Rect::from_min_size(
                    egui::pos2(origin.x + w - 28.0, origin.y - 4.0),
                    egui::vec2(22.0, 22.0),
                );
                let edit_resp = ui.allocate_rect(edit_btn, egui::Sense::click());
                {
                    let stroke = egui::Stroke::new(1.0, crate::theme::signal_red());
                    ui.painter()
                        .rect_stroke(edit_btn, 4.0, stroke, egui::StrokeKind::Outside);
                    let pose = crate::motion::icon::pose(
                        ui,
                        edit_resp.id,
                        crate::motion::icon::Cue::of(&edit_resp, false),
                        crate::motion::icon::Kick::Nudge(egui::vec2(1.0, -1.0)),
                    );
                    // 12 pt, as before: the glyph paints at 0.78 of its rect.
                    crate::motion::icon::paint_glyph(
                        ui.painter(),
                        egui::Rect::from_center_size(edit_btn.center(), egui::vec2(15.4, 15.4)),
                        crate::icons::ph::regular::PENCIL_SIMPLE,
                        crate::theme::amber(),
                        pose,
                    );
                }
                if edit_resp.on_hover_text("Edit Home").clicked() {
                    self.home_edit = Some(HomeEditDraft::from_layout(&self.home_layout));
                }

                ui.allocate_ui_with_layout(
                    egui::vec2(w, body_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("home-mission-body")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                // Reserve exactly the grid's height up front: the slots
                                // paint into child uis that don't move the cursor, so the
                                // body only scrolls when the modules really overflow.
                                let bottom = placed.iter().map(|p| p.y + p.h).fold(0.0, f32::max);
                                let (area, _) = ui.allocate_exact_size(
                                    egui::vec2(w, bottom),
                                    egui::Sense::hover(),
                                );
                                let base = area.min;
                                for slot in &placed {
                                    let rect = egui::Rect::from_min_size(
                                        egui::pos2(base.x + slot.x, base.y + slot.y),
                                        egui::vec2(slot.w, slot.h),
                                    );
                                    let Some(id) = slot.module_id.as_deref() else {
                                        continue;
                                    };
                                    let mut child =
                                        ui.new_child(egui::UiBuilder::new().max_rect(rect));
                                    let ui = &mut child;
                                    ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                                    ui.set_min_size(rect.size());
                                    actions.extend(paint_module(self, ui, id, slot.size));
                                }
                            });
                    },
                );
                let top = ui.cursor().min.y;
                self.home_compose(ui, &mut actions);
                // Measure the real chips + composer so the grid above never
                // pushes the composer border past the window bottom.
                let used = ui.cursor().min.y - top;
                ui.ctx()
                    .data_mut(|d| d.insert_temp(compose_id, used.ceil()));
            });

        for act in actions {
            match act {
                ModuleAction::OpenChat(text) => self.send_home_to_chat(text),
                ModuleAction::Nav(id) => self.set_nav_id(id),
                ModuleAction::Settings(sec) => {
                    self.set_nav_id("settings");
                    self.settings_sec = sec;
                }
            }
        }
    }

    fn home_header(&mut self, ui: &mut egui::Ui, done: usize) {
        let name = greeting_name(
            &self.greeting_user_md,
            self.secrets
                .oauth
                .as_ref()
                .and_then(|t| t.name.as_deref())
                .unwrap_or(""),
        );
        let name = if self.cfg.display_name.trim().is_empty() {
            name
        } else {
            self.cfg.display_name.trim().to_string()
        };
        let show_xp = self.home_layout.xp_bar;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label(caption(&day_caption(&Self::local_day())));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(greeting(&name))
                        .font(crate::theme::title_font(28.0))
                        .color(crate::theme::fg()),
                );
            });
            if show_xp {
                let xp_tally = self.dock_xp(ui.ctx());
                let (level, xp, frac) = level_xp(xp_tally, done);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(280.0_f32.min(ui.available_width()), 48.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                ui.label(caption(&format!("Lvl {level}")));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "{} / {} XP",
                                                grouped(xp),
                                                grouped(level * XP_PER_LEVEL)
                                            ))
                                            .size(crate::theme::TYPE_CAPTION)
                                            .extra_letter_spacing(1.0)
                                            .strong()
                                            .color(crate::theme::amber()),
                                        );
                                    },
                                );
                            });
                            let (bar, bar_resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 6.0),
                                egui::Sense::hover(),
                            );
                            bar_resp.on_hover_text(xp_hover(xp_tally));
                            ui.painter().rect_filled(bar, 3.0, crate::theme::selected());
                            let mut fill = bar;
                            fill.set_width(bar.width() * frac);
                            ui.painter().rect_filled(fill, 3.0, crate::theme::amber());
                        },
                    );
                });
            }
        });
    }

    fn home_compose(&mut self, ui: &mut egui::Ui, actions: &mut Vec<ModuleAction>) {
        let id = egui::Id::new(DRAFT_ID);
        let mut draft: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        ui.add_space(GAP);
        ui.horizontal_wrapped(|ui| {
            for s in HOME_CHIPS {
                if crate::ui_kit::chip(ui, s, true, true).clicked() {
                    actions.push(ModuleAction::OpenChat(s.to_string()));
                }
            }
        });
        ui.add_space(10.0);
        let mut send = false;
        egui::Frame::NONE
            .fill(crate::theme::surface())
            .stroke(egui::Stroke::new(1.0, crate::theme::signal_red()))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(14, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let field_w = ui.available_width() - 48.0;
                    let edit = ui.add_sized(
                        egui::vec2(field_w, 32.0),
                        egui::TextEdit::singleline(&mut draft)
                            .frame(egui::Frame::NONE)
                            .hint_text("Assign an objective or ask anything…")
                            .vertical_align(egui::Align::Center)
                            .font(egui::FontId::proportional(crate::theme::TYPE_LABEL)),
                    );
                    if edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        send = true;
                    }
                    if crate::ui_kit::recipe::button(
                        ui,
                        None,
                        "→",
                        true,
                        egui::vec2(36.0, 36.0),
                        true,
                    )
                    .on_hover_text("Send")
                    .clicked()
                    {
                        send = true;
                    }
                });
            });
        let text = draft.trim().to_string();
        if send && !text.is_empty() {
            draft.clear();
            actions.push(ModuleAction::OpenChat(text));
        }
        ui.ctx().data_mut(|d| d.insert_temp(id, draft));
    }

    /// Switch to Chat, open a new tab, and send `text` so the reply streams there.
    pub(in crate::app) fn send_home_to_chat(&mut self, text: String) {
        self.new_thread(false);
        self.nav = Nav::Chat;
        self.send_from_composer(text);
    }

    pub(in crate::app) fn open_mission_home(&mut self) {
        self.nav = Nav::Home;
    }
}
