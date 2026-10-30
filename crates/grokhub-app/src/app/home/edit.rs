//! Edit Home modules dialog (SPEC: draft layout, Save commits).

use grokhub_core::home::{
    lookup, modules_fitting, modules_for_category, Category, HomeLayout, HomePreset, ModuleSize,
    SlotId, REGISTRY,
};

use super::super::*;
use super::save_home_layout;

/// In-progress Edit Home draft. `None` on Cabin means the dialog is closed.
#[derive(Debug, Clone)]
pub(in crate::app) struct HomeEditDraft {
    pub layout: HomeLayout,
    pub selected: SlotId,
    pub search: String,
    pub category: Option<Category>,
    pub fits_only: bool,
    pub confirm_reset: bool,
}

impl HomeEditDraft {
    pub fn from_layout(layout: &HomeLayout) -> Self {
        Self {
            layout: layout.clone(),
            selected: SlotId::Medium1,
            search: String::new(),
            category: None,
            fits_only: true,
            confirm_reset: false,
        }
    }

    fn selected_size(&self) -> ModuleSize {
        self.selected.size()
    }

    fn slot_module(&self, slot: SlotId) -> Option<&str> {
        match slot {
            SlotId::Large => Some(self.layout.large.as_str()),
            SlotId::Medium1 => self.layout.medium.first().map(|s| s.as_str()),
            SlotId::Medium2 => self.layout.medium.get(1).map(|s| s.as_str()),
            SlotId::Small1 => self.layout.small[0].as_deref(),
            SlotId::Small2 => self.layout.small[1].as_deref(),
            SlotId::Small3 => self.layout.small[2].as_deref(),
            SlotId::Small4 => self.layout.small[3].as_deref(),
        }
        .filter(|s| !s.is_empty())
    }

    fn assign(&mut self, id: &str) {
        // Clear any other slot already holding this module.
        if self.layout.large == id {
            self.layout.large.clear();
        }
        for m in &mut self.layout.medium {
            if m == id {
                m.clear();
            }
        }
        for s in &mut self.layout.small {
            if s.as_deref() == Some(id) {
                *s = None;
            }
        }
        match self.selected {
            SlotId::Large => self.layout.large = id.into(),
            SlotId::Medium1 => {
                if self.layout.medium.is_empty() {
                    self.layout.medium.push(id.into());
                } else {
                    self.layout.medium[0] = id.into();
                }
            }
            SlotId::Medium2 => {
                while self.layout.medium.len() < 2 {
                    self.layout.medium.push(String::new());
                }
                self.layout.medium[1] = id.into();
            }
            SlotId::Small1 => self.layout.small[0] = Some(id.into()),
            SlotId::Small2 => self.layout.small[1] = Some(id.into()),
            SlotId::Small3 => self.layout.small[2] = Some(id.into()),
            SlotId::Small4 => self.layout.small[3] = Some(id.into()),
        }
        self.layout.preset = "custom".into();
    }

    fn hide_selected(&mut self) {
        match self.selected {
            SlotId::Large => self.layout.large.clear(),
            SlotId::Medium1 => {
                if !self.layout.medium.is_empty() {
                    self.layout.medium[0].clear();
                }
            }
            SlotId::Medium2 => {
                if self.layout.medium.len() > 1 {
                    self.layout.medium[1].clear();
                }
            }
            SlotId::Small1 => self.layout.small[0] = None,
            SlotId::Small2 => self.layout.small[1] = None,
            SlotId::Small3 => self.layout.small[2] = None,
            SlotId::Small4 => self.layout.small[3] = None,
        }
        self.layout.preset = "custom".into();
    }

    fn move_selected(&mut self, up: bool) {
        let swap = |a: &mut String, b: &mut String| {
            let t = a.clone();
            *a = b.clone();
            *b = t;
        };
        match self.selected {
            SlotId::Medium1 if !up && self.layout.medium.len() > 1 => {
                let (a, b) = self.layout.medium.split_at_mut(1);
                swap(&mut a[0], &mut b[0]);
                self.selected = SlotId::Medium2;
            }
            SlotId::Medium2 if up && !self.layout.medium.is_empty() => {
                let (a, b) = self.layout.medium.split_at_mut(1);
                swap(&mut a[0], &mut b[0]);
                self.selected = SlotId::Medium1;
            }
            SlotId::Small1 if !up => {
                self.layout.small.swap(0, 1);
                self.selected = SlotId::Small2;
            }
            SlotId::Small2 if up => {
                self.layout.small.swap(0, 1);
                self.selected = SlotId::Small1;
            }
            SlotId::Small2 if !up => {
                self.layout.small.swap(1, 2);
                self.selected = SlotId::Small3;
            }
            SlotId::Small3 if up => {
                self.layout.small.swap(1, 2);
                self.selected = SlotId::Small2;
            }
            SlotId::Small3 if !up => {
                self.layout.small.swap(2, 3);
                self.selected = SlotId::Small4;
            }
            SlotId::Small4 if up => {
                self.layout.small.swap(2, 3);
                self.selected = SlotId::Small3;
            }
            _ => {}
        }
        self.layout.preset = "custom".into();
    }
}

/// Catalog note for a module whose data isn't wired up yet.
const NOT_BUILT: &str = "Not built yet: shows a placeholder card for now";

/// Catalog row text in plain words: what's in the slot, or which sizes fit.
fn catalog_title(name: &str, sizes: &[ModuleSize], in_slot: bool) -> String {
    if in_slot {
        return format!("{name} · in this slot");
    }
    let sizes: Vec<_> = sizes.iter().map(|s| s.label()).collect();
    format!("{name} · fits {}", sizes.join(", "))
}

impl Cabin {
    pub(in crate::app) fn ui_home_edit(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.home_edit.take() else {
            return;
        };
        let mut close = false;
        let mut save = false;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if draft.confirm_reset {
                draft.confirm_reset = false;
            } else {
                close = true;
            }
        }
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("home-edit-overlay"))
            .fixed_pos(screen.min)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                ui.painter()
                    .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(180));
                let modal = egui::Rect::from_center_size(
                    screen.center(),
                    egui::vec2(
                        (screen.width() - 48.0).min(920.0),
                        (screen.height() - 48.0).min(620.0),
                    ),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(modal), |ui| {
                    egui::Frame::NONE
                        .fill(crate::theme::bg())
                        .corner_radius(crate::theme::SHEET_RADIUS)
                        .stroke(egui::Stroke::new(1.0, crate::theme::border()))
                        .inner_margin(egui::Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_min_size(modal.size() - egui::vec2(32.0, 32.0));
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("SETTINGS · HOME")
                                        .size(crate::theme::TYPE_CAPTION)
                                        .color(crate::theme::muted()),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if crate::icons::glyph_button(
                                            ui,
                                            crate::icons::ph::regular::X,
                                            24.0,
                                            crate::theme::muted(),
                                            crate::motion::icon::Kick::Turn(
                                                std::f32::consts::FRAC_PI_2,
                                            ),
                                        )
                                        .on_hover_text("Close")
                                        .clicked()
                                        {
                                            close = true;
                                        }
                                    },
                                );
                            });
                            ui.label(
                                RichText::new("Edit Home modules")
                                    .font(crate::theme::title_font(22.0))
                                    .color(crate::theme::fg()),
                            );
                            ui.add_space(12.0);
                            let body_h = ui.available_height() - 52.0;
                            ui.horizontal(|ui| {
                                ui.set_min_height(body_h);
                                // Left: slot map + presets
                                ui.allocate_ui_with_layout(
                                    egui::vec2(280.0, body_h),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.label(
                                            RichText::new("YOUR LAYOUT")
                                                .size(crate::theme::TYPE_CAPTION)
                                                .color(crate::theme::muted()),
                                        );
                                        ui.add_space(6.0);
                                        for slot in [
                                            SlotId::Large,
                                            SlotId::Medium1,
                                            SlotId::Medium2,
                                            SlotId::Small1,
                                            SlotId::Small2,
                                            SlotId::Small3,
                                            SlotId::Small4,
                                        ] {
                                            let selected = draft.selected == slot;
                                            let name = draft
                                                .slot_module(slot)
                                                .and_then(lookup)
                                                .map(|m| m.name)
                                                .unwrap_or("+ Add module");
                                            let label = format!("{} · {name}", slot.label());
                                            let resp = ui.selectable_label(selected, label);
                                            if resp.clicked() {
                                                draft.selected = slot;
                                            }
                                            if selected {
                                                ui.horizontal(|ui| {
                                                    if ui.small_button("↑").clicked() {
                                                        draft.move_selected(true);
                                                    }
                                                    if ui.small_button("↓").clicked() {
                                                        draft.move_selected(false);
                                                    }
                                                    if ui.small_button("Hide").clicked() {
                                                        draft.hide_selected();
                                                    }
                                                });
                                            }
                                        }
                                        ui.add_space(10.0);
                                        ui.label(
                                            RichText::new("PRESETS")
                                                .size(crate::theme::TYPE_CAPTION)
                                                .color(crate::theme::muted()),
                                        );
                                        for preset in [
                                            HomePreset::MissionControl,
                                            HomePreset::Streamer,
                                            HomePreset::Performance,
                                        ] {
                                            if ui.button(preset.label()).clicked() {
                                                draft.layout = HomeLayout::preset(preset);
                                            }
                                        }
                                    },
                                );
                                ui.separator();
                                // Right: catalog
                                ui.allocate_ui_with_layout(
                                    egui::vec2(ui.available_width(), body_h),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        let size = draft.selected_size();
                                        ui.label(
                                            RichText::new(format!(
                                                "CATALOG · picking for {}",
                                                draft.selected.label()
                                            ))
                                            .size(crate::theme::TYPE_CAPTION)
                                            .color(crate::theme::muted()),
                                        );
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            ui.add(
                                                egui::TextEdit::singleline(&mut draft.search)
                                                    .hint_text("Search modules…")
                                                    .desired_width(180.0),
                                            );
                                            ui.checkbox(
                                                &mut draft.fits_only,
                                                format!("Fits {}", size.label()),
                                            );
                                        });
                                        ui.horizontal_wrapped(|ui| {
                                            if ui
                                                .selectable_label(draft.category.is_none(), "All")
                                                .clicked()
                                            {
                                                draft.category = None;
                                            }
                                            for cat in Category::ALL {
                                                if ui
                                                    .selectable_label(
                                                        draft.category == Some(cat),
                                                        cat.label(),
                                                    )
                                                    .clicked()
                                                {
                                                    draft.category = Some(cat);
                                                }
                                            }
                                        });
                                        ui.add_space(6.0);
                                        egui::ScrollArea::vertical()
                                            .id_salt("home-edit-catalog")
                                            .show(ui, |ui| {
                                                let q = draft.search.to_ascii_lowercase();
                                                let size = draft.selected_size();
                                                let list: Vec<_> = REGISTRY
                                                    .iter()
                                                    .filter(|m| {
                                                        draft
                                                            .category
                                                            .map(|c| m.category == c)
                                                            .unwrap_or(true)
                                                    })
                                                    .filter(|m| {
                                                        !draft.fits_only || m.supports(size)
                                                    })
                                                    .filter(|m| {
                                                        q.is_empty()
                                                            || m.name
                                                                .to_ascii_lowercase()
                                                                .contains(&q)
                                                            || m.id.contains(&q)
                                                    })
                                                    .collect();
                                                for m in list {
                                                    let in_slot = draft.slot_module(draft.selected)
                                                        == Some(m.id);
                                                    let title =
                                                        catalog_title(m.name, m.sizes, in_slot);
                                                    let resp = ui.add_sized(
                                                        egui::vec2(ui.available_width(), 36.0),
                                                        egui::Button::new(title).selected(in_slot),
                                                    );
                                                    if resp.clicked() && m.supports(size) {
                                                        draft.assign(m.id);
                                                    }
                                                    if !m.supports(size) {
                                                        ui.label(
                                                            RichText::new("Does not fit this slot")
                                                                .size(crate::theme::TYPE_CAPTION)
                                                                .color(crate::theme::muted()),
                                                        );
                                                    } else if m.data
                                                        == grokhub_core::home::DataStatus::New
                                                    {
                                                        ui.label(
                                                            RichText::new(NOT_BUILT)
                                                                .size(crate::theme::TYPE_CAPTION)
                                                                .color(crate::theme::muted()),
                                                        );
                                                    }
                                                }
                                                let _ = modules_fitting(size);
                                                let _ = modules_for_category(Category::Gaming);
                                            });
                                    },
                                );
                            });
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if draft.confirm_reset {
                                    ui.label("Reset to Mission Control?");
                                    if ui.button("Confirm reset").clicked() {
                                        draft.layout =
                                            HomeLayout::preset(HomePreset::MissionControl);
                                        draft.confirm_reset = false;
                                    }
                                    if ui.button("Keep editing").clicked() {
                                        draft.confirm_reset = false;
                                    }
                                } else if ui.button("Reset to default").clicked() {
                                    draft.confirm_reset = true;
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if crate::ui_kit::recipe::button(
                                            ui,
                                            None,
                                            "SAVE",
                                            true,
                                            egui::vec2(96.0, 36.0),
                                            true,
                                        )
                                        .clicked()
                                        {
                                            save = true;
                                        }
                                        if ui.button("Cancel").clicked() {
                                            close = true;
                                        }
                                    },
                                );
                            });
                        });
                });
            });

        if save {
            let layout = draft.layout.sanitize();
            let _ = save_home_layout(&crate::config::config_dir(), &layout);
            self.home_layout = layout;
            self.home_edit = None;
        } else if close {
            self.home_edit = None;
        } else {
            self.home_edit = Some(draft);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_rows_use_plain_words() {
        let all = [ModuleSize::Large, ModuleSize::Medium, ModuleSize::Small];
        assert_eq!(
            catalog_title("Storage", &all, false),
            "Storage · fits Large, Medium, Small"
        );
        assert_eq!(
            catalog_title("Up next", &[ModuleSize::Small], false),
            "Up next · fits Small"
        );
        assert_eq!(
            catalog_title("Storage", &all, true),
            "Storage · in this slot"
        );
        assert_eq!(NOT_BUILT, "Not built yet: shows a placeholder card for now");
    }
}
