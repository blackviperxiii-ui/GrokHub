//! Chat-first layout (deck slide 10, layout B): with Settings → Appearance →
//! "Objectives beside chat" on, a chat page shows the open mission's steps
//! and recent chats on the left and a PC snapshot on the right, instead of
//! the companion dock. Everything is real state: the checklist, the thread
//! list and this machine's hardware read.

use crate::threads::ChatThread;

use super::*;

/// Both sidebars from this page width; the left one alone from
/// [`LEFT_ONLY_MIN_W`]; none below that.
const BOTH_MIN_W: f32 = 1100.0;
const LEFT_ONLY_MIN_W: f32 = 820.0;
const LEFT_W: f32 = 260.0;
const RIGHT_W: f32 = 240.0;
const RECENT_CHATS: usize = 5;
const SNAPSHOT_TILES: usize = 3;
pub(in crate::app) const LAYOUT_ROW: &str = "Objectives beside chat";
const LAYOUT_HINT: &str =
    "Chats show the open objective's steps and recent chats on the left and a PC snapshot on the right, in place of the companion dock.";

/// Which sidebars fit a page this wide: (left, right).
pub(in crate::app) fn sidebars_for(page_w: f32) -> (bool, bool) {
    (page_w >= LEFT_ONLY_MIN_W, page_w >= BOTH_MIN_W)
}

/// Recent chats, newest first: not the open one, not archived, not empty.
pub(in crate::app) fn recent_chats(threads: &[ChatThread], open: usize) -> Vec<(usize, String)> {
    let mut rows: Vec<(usize, &ChatThread)> = threads
        .iter()
        .enumerate()
        .filter(|(i, t)| *i != open && t.archived_at.is_none() && !t.messages.is_empty())
        .collect();
    rows.sort_by_key(|(_, t)| std::cmp::Reverse(t.accessed_ms));
    rows.into_iter()
        .take(RECENT_CHATS)
        .map(|(i, t)| {
            let title = t.title.trim();
            let title = if title.is_empty() {
                "Untitled chat"
            } else {
                title
            };
            (i, title.to_string())
        })
        .collect()
}

impl Cabin {
    pub(in crate::app) fn ui_chat_layout_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.chat_objectives;
        if crate::cards::settings_toggle(ui, LAYOUT_ROW, LAYOUT_HINT, &mut on) {
            self.cfg.chat_objectives = on;
            self.persist_cfg();
        }
    }

    /// Call before `ui_chat`, like the dock, so the chat takes what's left.
    pub(super) fn ui_chat_sidebars(&mut self, ui: &mut egui::Ui) {
        let (left, right) = sidebars_for(ui.available_width());
        let frame = egui::Frame::NONE
            .fill(crate::theme::bg())
            .inner_margin(egui::Margin::symmetric(16, 20));
        if right {
            egui::Panel::right("chat-first-snapshot")
                .exact_size(RIGHT_W)
                .resizable(false)
                .frame(frame)
                .show(ui, chat_snapshot);
        }
        if left {
            egui::Panel::left("chat-first-objectives")
                .exact_size(LEFT_W)
                .resizable(false)
                .frame(frame)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.chat_objectives_column(ui));
                });
        }
    }

    fn chat_objectives_column(&mut self, ui: &mut egui::Ui) {
        ui.label(caption("Objectives"));
        ui.add_space(10.0);
        let mission = self.dock_mission(ui.ctx());
        let steps: Vec<_> = mission
            .as_ref()
            .map(|m| {
                m.checklist
                    .iter()
                    .filter(|i| i.status != MissionCheckStatus::Cancelled)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if steps.is_empty() {
            ui.label(
                egui::RichText::new(format!(
                    "Nothing on the list yet. Ask {} to plan something.",
                    grokhub_core::brand::companion_name()
                ))
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::muted()),
            );
        }
        let current = steps
            .iter()
            .position(|i| i.status != MissionCheckStatus::Verified);
        for (n, item) in steps.iter().enumerate() {
            let done = item.status == MissionCheckStatus::Verified;
            let fill = if Some(n) == current {
                crate::theme::selected()
            } else {
                egui::Color32::TRANSPARENT
            };
            egui::Frame::NONE
                .fill(fill)
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(10, 9))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        diamond(ui, !done);
                        let text = egui::RichText::new(&item.text).size(crate::theme::TYPE_LABEL);
                        let text = if done {
                            text.strikethrough().color(crate::theme::muted())
                        } else {
                            text.color(crate::theme::fg())
                        };
                        ui.add(egui::Label::new(text).truncate());
                    });
                });
        }
        ui.add_space(20.0);
        ui.label(caption("Recent"));
        ui.add_space(6.0);
        let recent = recent_chats(&self.threads, self.thread_idx);
        if recent.is_empty() {
            ui.label(
                egui::RichText::new("Other chats show up here.")
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::muted()),
            );
        }
        let mut open = None;
        for (idx, title) in recent {
            let resp = ui.add(
                egui::Label::new(
                    egui::RichText::new(title)
                        .size(crate::theme::TYPE_LABEL)
                        .color(crate::theme::muted()),
                )
                .truncate()
                .sense(egui::Sense::click()),
            );
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                open = Some(idx);
            }
            ui.add_space(6.0);
        }
        if let Some(idx) = open {
            self.switch_thread(idx);
        }
    }
}

/// SNAPSHOT: the first few PC tiles, one per row; the GPU one in amber.
fn chat_snapshot(ui: &mut egui::Ui) {
    use crate::ui_kit::recipe;
    ui.label(caption("Snapshot"));
    ui.add_space(10.0);
    let Some(p) = pc_profile() else {
        ui.label(
            egui::RichText::new("Reading this PC…")
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::muted()),
        );
        return;
    };
    let mut tiles = pc_tile_list(&p);
    tiles.truncate(SNAPSHOT_TILES);
    for (i, t) in tiles.iter().enumerate() {
        let picked = i == 0 && t.label == "GPU";
        let (stroke, ink) = if picked {
            (
                recipe::look(recipe::Part::Card, recipe::State::Accent).stroke,
                crate::theme::amber(),
            )
        } else {
            (
                egui::Stroke::new(1.0, crate::theme::border()),
                crate::theme::fg(),
            )
        };
        egui::Frame::NONE
            .fill(crate::theme::surface())
            .stroke(stroke)
            .corner_radius(recipe::BUTTON_RADIUS)
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(caption(t.label));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&t.value)
                            .font(crate::theme::title_font(crate::theme::TYPE_TITLE))
                            .color(ink),
                    )
                    .truncate(),
                )
                .on_hover_text(&t.value);
            });
        ui.add_space(10.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn thread(title: &str, accessed_ms: u64, messages: usize, archived: bool) -> ChatThread {
        let mut t = ChatThread::new(title, false);
        t.title = title.into();
        t.accessed_ms = accessed_ms;
        t.messages = Arc::new(vec![("user".into(), "hi".into()); messages]);
        t.archived_at = archived.then_some(1);
        t
    }

    #[test]
    fn sidebars_fold_away_on_narrow_pages() {
        assert_eq!(sidebars_for(1500.0), (true, true));
        assert_eq!(sidebars_for(1100.0), (true, true));
        assert_eq!(sidebars_for(1000.0), (true, false));
        assert_eq!(sidebars_for(700.0), (false, false));
    }

    #[test]
    fn recent_is_newest_first_without_the_open_archived_or_empty_chats() {
        let threads = vec![
            thread("Open one", 900, 2, false),
            thread("Fan curve tweak", 500, 2, false),
            thread("Backlog sort", 700, 1, false),
            thread("Old archive", 800, 3, true),
            thread("Blank draft", 999, 0, false),
            thread("  ", 100, 1, false),
        ];
        assert_eq!(
            recent_chats(&threads, 0),
            vec![
                (2, "Backlog sort".to_string()),
                (1, "Fan curve tweak".to_string()),
                (5, "Untitled chat".to_string()),
            ]
        );
    }

    #[test]
    fn the_layout_is_off_until_settings_turns_it_on() {
        assert!(!crate::config::AppConfig::default().chat_objectives);
        let on: crate::config::AppConfig =
            serde_json::from_str(r#"{"chatObjectives": true}"#).expect("config");
        assert!(on.chat_objectives);
    }
}
