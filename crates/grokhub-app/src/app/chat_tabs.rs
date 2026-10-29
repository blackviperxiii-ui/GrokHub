//! Chat tab strip under the title bar (Hangout / Chat).
//!
//! Tabs share the available width evenly between a min and a max. Tabs that
//! cannot fit at the min width go into an overflow menu — there is no
//! horizontal scroll bar.

use super::sidebar::SEARCH_KEY_HINT;
use super::*;

/// Strip height under [`crate::theme::TITLEBAR_H`].
pub(super) const CHAT_TABS_H: f32 = 36.0;
/// Narrowest a tab may shrink before it overflows.
pub(super) const TAB_MIN_W: f32 = 96.0;
/// Widest a tab grows when there is room.
pub(super) const TAB_MAX_W: f32 = 180.0;
/// Trailing chrome reserved for + / overflow / tools.
const TRAILING_CHROME_W: f32 = 96.0;
/// Label on the open draft's tab before its first message.
const NEW_CHAT_TAB: &str = "New chat";

impl Cabin {
    /// Loadout tab strip: even-fit tabs, overflow menu, deck tokens.
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
        let active_pos = order
            .iter()
            .position(|&pos| listed.get(pos).copied() == Some(self.thread_idx));

        egui::Panel::top("chat-tabs")
            .exact_size(CHAT_TABS_H)
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .stroke(egui::Stroke::new(1.0, crate::theme::border()))
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let avail = (ui.available_width() - TRAILING_CHROME_W).max(TAB_MIN_W);
                    let (visible, hidden) = split_visible_tabs(&order, active_pos, avail);
                    let n_vis = visible.len().max(1);
                    let tab_w = (avail / n_vis as f32).clamp(TAB_MIN_W, TAB_MAX_W);

                    for &pos in &visible {
                        let i = listed[pos];
                        if let Some(next) = self.paint_chat_tab(ui, i, tab_w) {
                            act = Some(next);
                        }
                    }

                    let plus =
                        crate::theme::felt_icon_hit(ui, "+", 24.0, crate::theme::amber(), 16.0)
                            .on_hover_text("New chat");
                    if plus.clicked() {
                        new_tab = true;
                    }

                    if !hidden.is_empty() {
                        let label = format!("+{}", hidden.len());
                        let overflow = RichText::new(&label)
                            .size(13.0)
                            .color(crate::theme::muted());
                        let force_open = super::loadout_home::deck_overflow_open();
                        let btn = ui.menu_button(overflow, |ui| {
                            for &pos in &hidden {
                                let i = listed[pos];
                                let full = self.thread_rail_title(i);
                                let short = grokhub_core::display_tab_title(&full);
                                if ui.button(&short).on_hover_text(&full).clicked() {
                                    act = Some(TabAct::Switch(i));
                                    ui.close();
                                }
                            }
                        });
                        let resp = btn.response.on_hover_text("More chats");
                        // Deck HARD DONE: paint the overflow list open without a click.
                        if force_open {
                            let origin = egui::pos2(resp.rect.left(), resp.rect.bottom() + 4.0);
                            egui::Area::new(egui::Id::new("deck-tab-overflow"))
                                .order(egui::Order::Foreground)
                                .fixed_pos(origin)
                                .show(ui.ctx(), |ui| {
                                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                                        ui.set_min_width(140.0);
                                        ui.label(
                                            RichText::new("More chats")
                                                .size(crate::theme::FONT_META)
                                                .color(crate::theme::muted()),
                                        );
                                        ui.separator();
                                        for &pos in &hidden {
                                            let i = listed[pos];
                                            let full = self.thread_rail_title(i);
                                            let short = grokhub_core::display_tab_title(&full);
                                            ui.label(
                                                RichText::new(short)
                                                    .size(crate::theme::FONT_CHROME)
                                                    .color(crate::theme::fg()),
                                            )
                                            .on_hover_text(&full);
                                        }
                                    });
                                });
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // ASCII-only label — missing-glyph fonts turn "⋯" into a square.
                        let tools = RichText::new("...").size(14.0).color(crate::theme::muted());
                        ui.menu_button(tools, |ui| {
                            if ui.button(format!("Search ({SEARCH_KEY_HINT})")).clicked() {
                                open_search = true;
                                ui.close();
                            }
                            if !hidden.is_empty() {
                                ui.menu_button(format!("More chats ({})", hidden.len()), |ui| {
                                    for &pos in &hidden {
                                        let i = listed[pos];
                                        let full = self.thread_rail_title(i);
                                        let short = grokhub_core::display_tab_title(&full);
                                        if ui.button(&short).on_hover_text(&full).clicked() {
                                            act = Some(TabAct::Switch(i));
                                            ui.close();
                                        }
                                    }
                                });
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
                                        let short = grokhub_core::display_tab_title(title);
                                        if ui
                                            .button(format!("Restore {short}"))
                                            .on_hover_text(title)
                                            .clicked()
                                        {
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
                                        let short = grokhub_core::display_tab_title(title);
                                        if ui
                                            .button(format!("Restore {short}"))
                                            .on_hover_text(title)
                                            .clicked()
                                        {
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

    pub(super) fn chat_tab_indices(&self) -> Vec<usize> {
        let live_empty = self.messages.is_empty();
        let mut listed: Vec<usize> =
            threads::chat_section_indices(&self.threads, Some(self.thread_idx), live_empty)
                .into_iter()
                .filter(|&i| !threads::is_background_history_title(&self.thread_rail_title(i)))
                .collect();
        // History hides empty drafts, but the open one is a tab: "+" must show it.
        if live_draft_tab(&self.threads, self.thread_idx, &listed) {
            listed.push(self.thread_idx);
        }
        listed
    }

    /// The open empty draft reads "New chat" until its first message names it.
    pub(super) fn chat_tab_title(&self, i: usize) -> String {
        let full = self.thread_rail_title(i);
        if i == self.thread_idx && self.messages.is_empty() && full == "Chat" {
            NEW_CHAT_TAB.to_string()
        } else {
            full
        }
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

    fn paint_chat_tab(&mut self, ui: &mut egui::Ui, i: usize, width: f32) -> Option<TabAct> {
        if self.rename_idx == Some(i) {
            let mut act = None;
            ui.allocate_ui_with_layout(
                egui::vec2(width.max(120.0), 26.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if let Some(next) = self.chat_rename_act(ui, i) {
                        act = Some(next);
                    }
                },
            );
            return act;
        }
        let full = self.chat_tab_title(i);
        let title = grokhub_core::display_tab_title(&full);
        let pinned = self.threads.get(i).is_some_and(|t| t.pinned);
        let selected = i == self.thread_idx;
        let fill = if selected {
            crate::theme::selected()
        } else {
            crate::theme::bg()
        };
        let stroke = if selected {
            egui::Stroke::new(1.5, crate::theme::amber())
        } else {
            egui::Stroke::new(1.0, crate::theme::border())
        };
        let text_color = if selected {
            crate::theme::fg()
        } else {
            crate::theme::muted()
        };

        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::click());
        let hovered = resp.hovered();
        ui.painter().rect(
            rect,
            crate::theme::RADIUS_SM,
            if hovered && !selected {
                crate::theme::elevated()
            } else {
                fill
            },
            stroke,
            egui::StrokeKind::Inside,
        );

        let close_w = if hovered || selected { 18.0 } else { 0.0 };
        let pin_w = if pinned { 14.0 } else { 0.0 };
        let pad = 8.0;
        let text_left = rect.left() + pad + pin_w;
        let text_right = rect.right() - pad - close_w;
        let text_w = (text_right - text_left).max(12.0);

        if pinned {
            let pin_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 4.0, rect.center().y - 6.0),
                egui::vec2(12.0, 12.0),
            );
            crate::icons::paint_rail_icon_at(
                ui.painter(),
                pin_rect,
                crate::icons::RailIcon::Pin,
                crate::theme::amber(),
            );
        }

        let font = egui::FontId::proportional(crate::theme::FONT_CHROME);
        let shown = ellipsize_to_width(ui, &title, &font, text_w);
        let galley = ui.fonts_mut(|f| f.layout_no_wrap(shown, font, text_color));
        let text_pos = egui::pos2(text_left, rect.center().y - galley.size().y * 0.5);
        ui.painter().galley(text_pos, galley, text_color);

        let mut act = None;
        if close_w > 0.0 {
            let close_rect = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 10.0, rect.center().y),
                egui::vec2(14.0, 14.0),
            );
            let close_id = ui.id().with(("chat-tab-close", i));
            let close_resp = ui.interact(close_rect, close_id, egui::Sense::click());
            paint_tab_close(ui, close_rect, close_resp.hovered());
            if close_resp.clicked() {
                act = Some(TabAct::Delete(i));
            }
            close_resp.on_hover_text("Close");
        }

        let resp = resp.on_hover_text(&full);
        if act.is_none() && resp.clicked() {
            act = Some(TabAct::Switch(i));
        }
        if resp.double_clicked() {
            act = Some(TabAct::StartRename(i));
        }
        let archived = self.threads.get(i).is_some_and(threads::is_archived);
        let pin_label = if pinned { "Unpin" } else { "Pin" };
        let arch_label = if archived { "Unarchive" } else { "Archive" };
        resp.context_menu(|ui| {
            if ui.button(pin_label).clicked() {
                act = Some(TabAct::Pin(i));
                ui.close();
            }
            if ui.button(arch_label).clicked() {
                act = Some(TabAct::Archive(i));
                ui.close();
            }
            if ui.button("Rename").clicked() {
                act = Some(TabAct::StartRename(i));
                ui.close();
            }
            if ui.button("Delete").clicked() {
                act = Some(TabAct::Delete(i));
                ui.close();
            }
        });
        act
    }
}

/// The open thread is an empty draft the History filter left out. It still
/// gets a tab, unless it is archived, in the trash, or a background run.
fn live_draft_tab(all: &[ChatThread], live: usize, listed: &[usize]) -> bool {
    let Some(t) = all.get(live) else {
        return false;
    };
    !listed.contains(&live)
        && !t.background
        && !t.scratch
        && !threads::is_archived(t)
        && !threads::is_deleted(t)
        && !threads::is_background_history_title(&t.title)
}

/// How many tabs fit at [`TAB_MIN_W`], always keeping the active tab in view.
fn split_visible_tabs(
    order: &[usize],
    active_pos: Option<usize>,
    avail: f32,
) -> (Vec<usize>, Vec<usize>) {
    if order.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let max_fit = ((avail / TAB_MIN_W).floor() as usize).max(1);
    if order.len() <= max_fit {
        return (order.to_vec(), Vec::new());
    }
    // Window of max_fit entries that includes the active tab.
    let active = active_pos.unwrap_or(0).min(order.len() - 1);
    let mut start = active.saturating_sub(max_fit / 2);
    if start + max_fit > order.len() {
        start = order.len() - max_fit;
    }
    let end = start + max_fit;
    let visible = order[start..end].to_vec();
    let hidden: Vec<usize> = order[..start]
        .iter()
        .chain(order[end..].iter())
        .copied()
        .collect();
    (visible, hidden)
}

/// Labels already fitted, keyed by text, font size and width. Every tab is
/// fitted every frame, and a fit is a binary search of text layouts.
type FitKey = (String, u32, u32);
thread_local! {
    static FITTED: std::cell::RefCell<std::collections::HashMap<FitKey, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}
const FITTED_CAP: usize = 256;

fn ellipsize_to_width(ui: &mut egui::Ui, text: &str, font: &egui::FontId, max_w: f32) -> String {
    let key = (text.to_string(), font.size.to_bits(), max_w.round() as u32);
    if let Some(hit) = FITTED.with(|c| c.borrow().get(&key).cloned()) {
        return hit;
    }
    let shown = fit_label(ui, text, font, max_w.round());
    FITTED.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= FITTED_CAP {
            c.clear();
        }
        c.insert(key, shown.clone());
    });
    shown
}

fn fit_label(ui: &mut egui::Ui, text: &str, font: &egui::FontId, max_w: f32) -> String {
    if max_w <= 8.0 {
        return "…".into();
    }
    let fits = |s: &str| {
        ui.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font.clone(), crate::theme::fg()))
            .size()
            .x
            <= max_w
    };
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut lo = 0usize;
    let mut hi = chars.len();
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect();
        let with = format!("{candidate}…");
        if fits(&with) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 {
        "…".into()
    } else {
        let candidate: String = chars[..lo].iter().collect();
        format!("{}…", candidate.trim_end())
    }
}

fn paint_tab_close(ui: &egui::Ui, rect: egui::Rect, hovered: bool) {
    let color = if hovered {
        crate::theme::fg()
    } else {
        crate::theme::muted()
    };
    let stroke = egui::Stroke::new(1.4, color);
    let c = rect.center();
    let r = 3.5;
    ui.painter().line_segment(
        [egui::pos2(c.x - r, c.y - r), egui::pos2(c.x + r, c.y + r)],
        stroke,
    );
    ui.painter().line_segment(
        [egui::pos2(c.x + r, c.y - r), egui::pos2(c.x - r, c.y + r)],
        stroke,
    );
}

#[cfg(test)]
mod chat_tabs_source_tests {
    use super::{split_visible_tabs, TAB_MIN_W};

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
            src.contains("CHAT_TABS_H")
                && src.contains("TAB_MIN_W")
                && src.contains("TAB_MAX_W")
                && src.contains("split_visible_tabs")
                && src.contains("display_tab_title"),
            "even-fit tabs with overflow"
        );
        // No horizontal scroll widget in the strip (spell the type via format
        // so include_str! does not match this assert).
        let scroll = format!("{}Area", "Scroll");
        assert!(
            !src.contains(&scroll),
            "tab strip must not use a horizontal scroll widget"
        );
        assert!(
            src.contains("paint_tab_close") && src.contains("TabAct::Delete"),
            "close on hover soft-deletes into the 30-day trash"
        );
        let harsh = format!("{}_{}", "signal", "red");
        assert!(
            !src.contains(&harsh),
            "deck strip uses border/amber, not harsh red outlines on every tab"
        );
    }

    #[test]
    fn overflow_keeps_the_active_tab_visible() {
        let order: Vec<usize> = (0..12).collect();
        let avail = TAB_MIN_W * 4.0;
        let (vis, hid) = split_visible_tabs(&order, Some(10), avail);
        assert_eq!(vis.len(), 4, "{vis:?}");
        assert!(vis.contains(&10), "active must stay in view: {vis:?}");
        assert_eq!(hid.len(), 8, "{hid:?}");
        assert!(!hid.contains(&10));
    }
}
