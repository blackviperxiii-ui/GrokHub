//! Chat tab strip under the title bar (Hangout / Chat).

use super::sidebar::SEARCH_KEY_HINT;
use super::*;

/// Strip height under [`crate::theme::TITLEBAR_H`].
pub(super) const CHAT_TABS_H: f32 = 36.0;

impl Cabin {
    /// Loadout tab strip: black base, red outlines, gray selection, amber accent.
    pub(super) fn ui_chat_tabs(&mut self, ui: &mut egui::Ui) {
        let mut act: Option<TabAct> = None;
        let mut new_tab = false;
        let mut open_search = false;
        let mut restore_archived: Option<usize> = None;
        let mut restore_deleted: Option<usize> = None;

        let archived = self
            .threads
            .iter()
            .enumerate()
            .filter(|(_, t)| threads::is_archived(t) && !threads::is_deleted(t))
            .map(|(i, _)| (i, self.thread_rail_title(i)))
            .collect::<Vec<_>>();
        let deleted = self
            .threads
            .iter()
            .enumerate()
            .filter(|(_, t)| threads::is_deleted(t))
            .map(|(i, _)| (i, self.thread_rail_title(i)))
            .collect::<Vec<_>>();
        let listed = self.chat_tab_indices();
        let order = self.chat_tab_order(&listed);

        egui::Panel::top("chat-tabs")
            .exact_size(CHAT_TABS_H)
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .stroke(egui::Stroke::new(1.0, crate::theme::signal_red()))
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let strip_w = (ui.available_width() - 36.0).max(48.0);
                    egui::ScrollArea::horizontal()
                        .id_salt("chat-tab-scroll")
                        .auto_shrink([false, true])
                        .max_width(strip_w)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for &pos in &order {
                                    let i = listed[pos];
                                    if let Some(next) = self.paint_chat_tab(ui, i) {
                                        act = Some(next);
                                    }
                                }
                                let plus = crate::theme::felt_icon_hit(
                                    ui,
                                    "+",
                                    24.0,
                                    crate::theme::amber(),
                                    16.0,
                                )
                                .on_hover_text("New chat");
                                if plus.clicked() {
                                    new_tab = true;
                                }
                            });
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let tools = RichText::new("⋯").size(16.0).color(crate::theme::muted());
                        ui.menu_button(tools, |ui| {
                            if ui.button(format!("Search ({SEARCH_KEY_HINT})")).clicked() {
                                open_search = true;
                                ui.close();
                            }
                            ui.separator();
                            ui.menu_button("Archived", |ui| {
                                if archived.is_empty() {
                                    ui.label(
                                        RichText::new("No archived chats")
                                            .color(crate::theme::muted()),
                                    );
                                } else {
                                    for (i, title) in &archived {
                                        if ui.button(format!("Restore {title}")).clicked() {
                                            restore_archived = Some(*i);
                                            ui.close();
                                        }
                                    }
                                }
                            });
                            ui.menu_button("Recently deleted", |ui| {
                                if deleted.is_empty() {
                                    ui.label(
                                        RichText::new("Trash is empty")
                                            .color(crate::theme::muted()),
                                    );
                                } else {
                                    for (i, title) in &deleted {
                                        if ui.button(format!("Restore {title}")).clicked() {
                                            restore_deleted = Some(*i);
                                            ui.close();
                                        }
                                    }
                                }
                            });
                        })
                        .response
                        .on_hover_text("Chat tools");
                    });
                });
            });

        if new_tab {
            self.new_thread(false);
            self.nav = Nav::Chat;
            self.composer_want_focus = true;
        } else if let Some(act) = act {
            self.apply_tab_act(act);
        }
        if open_search {
            self.open_palette();
        }
        if let Some(i) = restore_archived {
            self.unarchive_thread_at(i);
        }
        if let Some(i) = restore_deleted {
            self.restore_deleted_thread_at(i);
        }
    }

    fn chat_tab_indices(&self) -> Vec<usize> {
        let live_empty = self.messages.is_empty();
        threads::chat_section_indices(&self.threads, Some(self.thread_idx), live_empty)
            .into_iter()
            .filter(|&i| !threads::is_background_history_title(&self.thread_rail_title(i)))
            .collect()
    }

    fn chat_tab_order(&self, listed: &[usize]) -> Vec<usize> {
        let keys: Vec<threads::SessionSortKey> = listed
            .iter()
            .map(|&i| {
                let t = &self.threads[i];
                threads::SessionSortKey {
                    pinned: t.pinned,
                    pinned_ms: t.pinned_ms,
                    accessed_ms: t.accessed_ms,
                    list_rank: 0,
                }
            })
            .collect();
        threads::session_list_order(&keys)
    }

    fn paint_chat_tab(&mut self, ui: &mut egui::Ui, i: usize) -> Option<TabAct> {
        if self.rename_idx == Some(i) {
            let mut act = None;
            ui.allocate_ui_with_layout(
                egui::vec2(140.0, 26.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if let Some(next) = self.chat_rename_act(ui, i) {
                        act = Some(next);
                    }
                },
            );
            return act;
        }
        let title = self.thread_rail_title(i);
        let pinned = self.threads.get(i).is_some_and(|t| t.pinned);
        let selected = i == self.thread_idx;
        let fill = if selected {
            crate::theme::steel()
        } else {
            crate::theme::bg()
        };
        let stroke = if selected {
            egui::Stroke::new(1.0, crate::theme::amber())
        } else {
            egui::Stroke::new(1.0, crate::theme::signal_red())
        };
        let resp = ui
            .horizontal(|ui| {
                if pinned {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    crate::icons::paint_rail_icon_at(
                        ui.painter(),
                        icon_rect,
                        crate::icons::RailIcon::Pin,
                        crate::theme::amber(),
                    );
                }
                ui.add(
                    egui::Button::new(
                        RichText::new(&title)
                            .size(crate::theme::FONT_CHROME)
                            .color(crate::theme::fg()),
                    )
                    .fill(fill)
                    .stroke(stroke)
                    .min_size(egui::vec2(0.0, 24.0)),
                )
            })
            .inner
            .on_hover_text(&title);
        let mut act = None;
        if resp.clicked() {
            act = Some(TabAct::Switch(i));
        }
        if resp.double_clicked() {
            act = Some(TabAct::StartRename(i));
        }
        let archived = self.threads.get(i).is_some_and(threads::is_archived);
        let pin_label = if pinned { "Unpin" } else { "Pin" };
        let arch_label = if archived { "Unarchive" } else { "Archive" };
        let fill_menu = |ui: &mut egui::Ui, act: &mut Option<TabAct>| {
            if ui.button(pin_label).clicked() {
                *act = Some(TabAct::Pin(i));
                ui.close();
            }
            if ui.button(arch_label).clicked() {
                *act = Some(TabAct::Archive(i));
                ui.close();
            }
            if ui.button("Rename").clicked() {
                *act = Some(TabAct::StartRename(i));
                ui.close();
            }
            if ui.button("Delete").clicked() {
                *act = Some(TabAct::Delete(i));
                ui.close();
            }
        };
        resp.context_menu(|ui| fill_menu(ui, &mut act));
        ui.menu_button(
            RichText::new("⋯").size(14.0).color(crate::theme::muted()),
            |ui| fill_menu(ui, &mut act),
        )
        .response
        .on_hover_text("Tab menu");
        act
    }
}

#[cfg(test)]
mod chat_tabs_source_tests {
    #[test]
    fn tab_strip_offers_pin_archive_delete_and_far_right_tools() {
        let src = include_str!("chat_tabs.rs");
        assert!(
            src.contains("ui_chat_tabs")
                && src.contains("\"Pin\"")
                && src.contains("\"Archive\"")
                && src.contains("\"Delete\""),
            "each tab menu must offer Pin, Archive, Delete"
        );
        assert!(
            src.contains("Archived")
                && src.contains("Recently deleted")
                && src.contains("Search")
                && src.contains("open_palette"),
            "far-right menu must offer Search, Archived, Recently deleted"
        );
        assert!(
            src.contains("session_list_order") && src.contains("chat_section_indices"),
            "tabs reuse pinned session order and live chat indices"
        );
        assert!(
            src.contains("CHAT_TABS_H") && src.contains("signal_red") && src.contains("amber"),
            "Loadout strip uses red outlines and amber accent"
        );
    }
}
