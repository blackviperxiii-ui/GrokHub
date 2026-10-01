//! Workboard cards. Each card is short: a title, one line, and what it has
//! (a chat, notes, a new report). Hover (or a click) opens it in place with the
//! full text, its notes, and a real chat with the agent: the same bubbles,
//! thoughts, and tool rows as the Chat page, streaming while it works. Starting
//! a drag folds an open card back so it moves as one small card.
//!
//! Follow up cards come from scheduled runs that left something to read or act
//! on (`file_automation_follow_up`). Each automation keeps one open card and one
//! chat; a new run adds its report to that chat.

use super::*;
use super::pages::{BoardAct, BoardDrag};
use grokhub_core::{
    encode_turn, visible_chat, visible_chat_refs, views_up_to_last_user, ChatView, ThoughtFold,
    UpdateAction, UpdateKind,
};

/// Rest on a card this long before it opens, so sweeping past does not flicker.
const HOVER_OPEN_SECS: f64 = 0.3;
/// Leave an open card this long before it folds back.
const LEAVE_CLOSE_SECS: f64 = 0.6;
/// Chat height inside an open card: a column card, and a full-width Follow up card.
pub(super) const CARD_CHAT_H: f32 = 260.0;
pub(super) const FOLLOW_CHAT_H: f32 = 340.0;

/// What the Workboards page remembers between frames.
#[derive(Clone, Debug, Default)]
pub(super) struct BoardView {
    pub open: Option<String>,
    /// Opened by a click: stays open until the pointer has been on it and left.
    pub pinned: bool,
    pub hover: Option<(String, f64)>,
    pub left_at: Option<f64>,
    /// What you are typing to the agent, per card.
    pub composers: HashMap<String, String>,
    /// A line under one card: why Send or Work on it did not go through.
    pub note: Option<(String, String)>,
}

pub(super) fn board_chat_edit_id(id: &str) -> egui::Id {
    egui::Id::new(("board-chat-edit", id))
}

fn board_notes_edit_id(id: &str) -> egui::Id {
    egui::Id::new(("board-notes-edit", id))
}

/// "3m ago", "2h ago", "4d ago" for a Follow up card's last run.
pub(super) fn ago_label(then_ms: u64, now_ms: u64) -> String {
    if then_ms == 0 {
        return String::new();
    }
    let s = now_ms.saturating_sub(then_ms) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3_599 => format!("{}m ago", s / 60),
        3_600..=86_399 => format!("{}h ago", s / 3_600),
        _ => format!("{}d ago", s / 86_400),
    }
}

/// The short line under a card's title: what it has, not buttons.
pub(super) fn card_meta_line(card: &BoardCard, now_ms: u64) -> String {
    let mut parts: Vec<String> = Vec::new();
    if card.automation.is_some() {
        let ago = ago_label(card.updated_ms, now_ms);
        parts.push(if ago.is_empty() {
            "From an automation".into()
        } else {
            format!("Ran {ago}")
        });
    }
    if card.thread_id.is_some() {
        parts.push("Chat".into());
    }
    if !card.notes.trim().is_empty() {
        parts.push("Notes".into());
    }
    parts.join(" · ")
}

/// Hover opens a card after a short rest; leaving folds it back. A card you are
/// typing in stays open. While a card is being dragged nothing opens, and the
/// open card is already folded.
pub(super) fn track_board_hover(
    v: &mut BoardView,
    now: f64,
    hovered: Option<String>,
    typing: bool,
    dragging: bool,
) -> bool {
    if dragging {
        v.open = None;
        v.hover = None;
        v.left_at = None;
        v.pinned = false;
        return false;
    }
    match hovered {
        Some(h) if v.open.as_deref() == Some(h.as_str()) => {
            v.hover = None;
            v.left_at = None;
            v.pinned = false;
            false
        }
        Some(_) if typing => {
            v.hover = None;
            false
        }
        Some(h) => match v.hover.clone() {
            Some((id, since)) if id == h => {
                if now - since >= HOVER_OPEN_SECS {
                    v.open = Some(h);
                    v.pinned = false;
                    v.hover = None;
                    v.left_at = None;
                    false
                } else {
                    true
                }
            }
            _ => {
                v.hover = Some((h, now));
                true
            }
        },
        None => {
            v.hover = None;
            if v.open.is_some() && !v.pinned && !typing {
                let since = *v.left_at.get_or_insert(now);
                if now - since >= LEAVE_CLOSE_SECS {
                    v.open = None;
                    v.left_at = None;
                    false
                } else {
                    true
                }
            } else {
                v.left_at = None;
                false
            }
        }
    }
}

/// Six dots: the drag handle.
pub(super) fn paint_grip(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
    let ink = crate::theme::subtle();
    for row in 0..3 {
        for col in 0..2 {
            let c = egui::pos2(
                rect.min.x + 2.5 + col as f32 * 4.5,
                rect.min.y + 3.0 + row as f32 * 4.0,
            );
            ui.painter().circle_filled(c, 1.1, ink);
        }
    }
}

fn fresh_badge(ui: &mut egui::Ui) {
    ui.label(
        RichText::new("NEW")
            .size(10.0)
            .strong()
            .color(crate::theme::live()),
    );
}

impl Cabin {
    /// One card in a column or in the Follow up row: compact, or open in place.
    /// Returns its rect for the hover tracker.
    pub(super) fn paint_board_card(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        wide: bool,
        act: &mut Option<BoardAct>,
    ) -> egui::Rect {
        let Some(card) = self.board.iter().find(|c| c.id == id).cloned() else {
            return egui::Rect::NOTHING;
        };
        let dragging_any = egui::DragAndDrop::has_payload_of_type::<BoardDrag>(ui.ctx());
        let open = !dragging_any && self.board_view.open.as_deref() == Some(id);
        if open && card.fresh {
            // Opened is read: the NEW badge goes.
            if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
                c.fresh = false;
            }
            self.flush_board();
        }
        if open {
            self.paint_board_card_open(ui, &card, wide, act)
        } else {
            self.paint_board_card_compact(ui, &card, act)
        }
    }

    /// Title, one line, and what the card has. The whole card drags; a click opens it.
    fn paint_board_card_compact(
        &mut self,
        ui: &mut egui::Ui,
        card: &BoardCard,
        act: &mut Option<BoardAct>,
    ) -> egui::Rect {
        let id = card.id.clone();
        let dragging = egui::DragAndDrop::payload::<BoardDrag>(ui.ctx()).is_some_and(|d| d.0 == id);
        let fill = if dragging {
            crate::theme::elevated().gamma_multiply(0.5)
        } else {
            crate::theme::elevated()
        };
        let meta = card_meta_line(card, now_ms());
        let frame = egui::Frame::none()
            .fill(fill)
            .rounding(crate::theme::CARD_RADIUS)
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.horizontal(|ui| {
                    paint_grip(ui);
                    if card.fresh {
                        fresh_badge(ui);
                    }
                    ui.add(
                        egui::Label::new(
                            RichText::new(&card.title)
                                .size(crate::theme::FONT_BODY)
                                .strong()
                                .color(crate::theme::fg()),
                        )
                        .truncate()
                        .selectable(false),
                    );
                });
                if !card.detail.trim().is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(card.detail.trim())
                                .size(crate::theme::FONT_TIP)
                                .color(crate::theme::muted()),
                        )
                        .truncate()
                        .selectable(false),
                    );
                }
                if !meta.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&meta)
                                .size(11.0)
                                .color(crate::theme::subtle()),
                        )
                        .truncate()
                        .selectable(false),
                    );
                }
            });
        let rect = frame.response.rect;
        let resp = ui
            .interact(rect, egui::Id::new(("board-card", id.as_str())), egui::Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Grab);
        if resp.hovered() {
            ui.painter().rect_stroke(
                rect,
                crate::theme::CARD_RADIUS,
                egui::Stroke::new(1.0_f32, crate::theme::border_strong()),
            );
        }
        if resp.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), BoardDrag(id.clone()));
        } else if resp.clicked() {
            *act = Some(BoardAct::Expand(id.clone()));
        }
        if dragging {
            super::pages::paint_drag_ghost(ui.ctx(), &card.title);
        }
        rect
    }

    /// The card opened in place: full text, notes, and the chat with the agent.
    /// Everything else (move, edit, link, archive) sits in the ··· menu.
    fn paint_board_card_open(
        &mut self,
        ui: &mut egui::Ui,
        card: &BoardCard,
        wide: bool,
        act: &mut Option<BoardAct>,
    ) -> egui::Rect {
        let id = card.id.clone();
        let editing_notes = self
            .board_notes_edit
            .as_ref()
            .is_some_and(|(eid, _)| eid == &id);
        let thread = card
            .thread_id
            .clone()
            .filter(|tid| self.threads.iter().any(|t| &t.id == tid));
        let busy_here = self.running
            && thread.is_some()
            && self.chat_job_thread.as_deref() == thread.as_deref();
        let note = self
            .board_view
            .note
            .as_ref()
            .filter(|(n, _)| n == &id)
            .map(|(_, t)| t.clone());
        let resp = egui::Frame::none()
            .fill(crate::theme::elevated())
            .rounding(crate::theme::CARD_RADIUS)
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border_strong()))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // Only the title row drags here; the rest of the card has controls.
                let mut menu_rect = egui::Rect::NOTHING;
                let head = ui.horizontal(|ui| {
                    paint_grip(ui);
                    if card.fresh {
                        fresh_badge(ui);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Middle dots: the UI font has no "⋯" glyph.
                        menu_rect = ui
                            .menu_button(RichText::new("···").size(16.0).strong(), |ui| {
                                self.board_card_menu(ui, card, thread.is_some(), act);
                            })
                            .response
                            .rect;
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&card.title)
                                        .size(crate::theme::FONT_UI)
                                        .strong()
                                        .color(crate::theme::fg()),
                                )
                                .wrap()
                                .selectable(false),
                            );
                        });
                    });
                });
                // The drag area stops short of the menu button, or it would take its clicks.
                let mut grip_rect = head.response.rect;
                if menu_rect.is_positive() {
                    grip_rect.max.x = (menu_rect.min.x - 6.0).max(grip_rect.min.x);
                }
                let grip = ui
                    .interact(
                        grip_rect,
                        egui::Id::new(("board-grip", id.as_str())),
                        egui::Sense::drag(),
                    )
                    .on_hover_cursor(egui::CursorIcon::Grab);
                if grip.drag_started() {
                    // The card folds back and moves as one small card.
                    egui::DragAndDrop::set_payload(ui.ctx(), BoardDrag(id.clone()));
                    self.board_view.open = None;
                }
                if !card.detail.trim().is_empty() && card.automation.is_none() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(card.detail.trim())
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::muted()),
                    );
                }
                if editing_notes {
                    ui.add_space(6.0);
                    if let Some((_, buf)) = self.board_notes_edit.as_mut() {
                        ui.add(
                            egui::TextEdit::multiline(buf)
                                .id(board_notes_edit_id(&id))
                                .hint_text(crate::theme::hint(
                                    "Notes for the agent: what to do, what to avoid, links, files",
                                ))
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                    }
                    ui.horizontal(|ui| {
                        if crate::cards::white_pill(ui, "Save notes") {
                            let buf = self
                                .board_notes_edit
                                .as_ref()
                                .map(|(_, b)| b.clone())
                                .unwrap_or_default();
                            *act = Some(BoardAct::SaveNotes { id: id.clone(), notes: buf });
                        }
                        if crate::cards::ghost_pill(ui, "Cancel") {
                            *act = Some(BoardAct::CancelNotes);
                        }
                    });
                } else if !card.notes.trim().is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Notes")
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::subtle()),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(card.notes.trim())
                                .size(crate::theme::FONT_TIP)
                                .color(crate::theme::fg()),
                        )
                        .wrap(),
                    );
                }
                ui.add_space(8.0);
                let chat_h = if wide { FOLLOW_CHAT_H } else { CARD_CHAT_H };
                match thread.as_deref() {
                    Some(tid) => self.paint_card_chat(ui, &id, tid, chat_h),
                    None if card.automation.is_some() && !card.report.trim().is_empty() => {
                        // A report whose chat is gone still reads like the chat did.
                        let view = ChatView {
                            kind: ChatKind::Assistant,
                            title: String::new(),
                            body: card.report.clone(),
                        };
                        paint_chat_block(ui, &view, false, false, ThoughtFold::Expanded);
                    }
                    None => {}
                }
                self.paint_card_composer(ui, card, thread.is_some(), busy_here, act);
                if let Some(note) = &note {
                    ui.label(
                        RichText::new(note)
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::setup()),
                    );
                }
            })
            .response;
        if busy_here {
            ui.ctx().request_repaint();
        }
        resp.rect
    }

    /// Everything that is not talking to the agent.
    fn board_card_menu(
        &mut self,
        ui: &mut egui::Ui,
        card: &BoardCard,
        linked: bool,
        act: &mut Option<BoardAct>,
    ) {
        let id = card.id.clone();
        let mut chosen: Option<BoardAct> = None;
        if linked {
            if ui.button("Open chat").clicked() {
                chosen = Some(BoardAct::Open(id.clone()));
            }
        } else if card.status != BoardStatus::Done && ui.button("Work on it").clicked() {
            chosen = Some(BoardAct::Work(id.clone()));
        }
        let notes_label = if card.notes.trim().is_empty() { "Add notes" } else { "Edit notes" };
        if ui.button(notes_label).clicked() {
            chosen = Some(BoardAct::EditNotes(id.clone()));
        }
        if ui.button("Edit").clicked() {
            chosen = Some(BoardAct::Edit(id.clone()));
        }
        ui.menu_button("Move to", |ui| {
            let dests = std::iter::once(KanbanColumn::FollowUp).chain(KanbanColumn::ALL);
            for dest in dests {
                if card.status.column() == Some(dest) {
                    continue;
                }
                if ui.button(dest.label()).clicked() {
                    chosen = Some(BoardAct::Move {
                        id: id.clone(),
                        status: dest.status(),
                    });
                }
            }
        });
        if linked {
            if ui.button("Unlink chat").clicked() {
                chosen = Some(BoardAct::Unlink(id.clone()));
            }
        } else if ui.button("Link current chat").clicked() {
            chosen = Some(BoardAct::Link(id.clone()));
        }
        ui.separator();
        if ui.button("Archive").clicked() {
            chosen = Some(BoardAct::Archive(id.clone()));
        }
        if chosen.is_some() {
            *act = chosen;
            ui.close_menu();
        }
    }

    /// The card's chat, drawn like the Chat page: replies as markdown bubbles,
    /// thoughts as short rows you can open, tool runs folded. A turn that is
    /// still running streams in from `turn_log`.
    pub(super) fn paint_card_chat(&mut self, ui: &mut egui::Ui, card_id: &str, thread_id: &str, max_h: f32) {
        // The open chat's live copy is `self.messages`; its thread row can lag.
        let messages = if thread_id == self.visible_thread_id() {
            self.messages.clone()
        } else {
            let Some(m) = self
                .threads
                .iter()
                .find(|t| t.id == thread_id)
                .map(|t| t.messages.clone())
            else {
                return;
            };
            m
        };
        let live = self.running && self.chat_job_thread.as_deref() == Some(thread_id);
        let mut views = visible_chat(&messages);
        if live && !self.turn_log.is_empty() {
            views = views_up_to_last_user(&views).to_vec();
            let turn = encode_turn(&self.turn_log);
            views.extend(visible_chat_refs([("assistant", turn.as_str())]));
        }
        let mut act = ChatBlockAct::None;
        egui::ScrollArea::vertical()
            .id_salt(("board-chat", thread_id))
            .stick_to_bottom(true)
            .auto_shrink([false, true])
            .max_height(max_h)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let last_reply = views.iter().rposition(|v| v.kind == ChatKind::Assistant);
                for (i, view) in views.iter().enumerate() {
                    let fold_id = thought_fold_id(thread_id, "card", i as u64);
                    let fold = if view.kind == ChatKind::Thought {
                        resolve_thought_fold(ui.ctx(), fold_id, true)
                    } else {
                        ThoughtFold::Expanded
                    };
                    let painted = ui
                        .push_id(("board-chat-row", thread_id, i), |ui| {
                            paint_chat_block_with(ui, view, false, false, fold, last_reply == Some(i))
                        })
                        .inner;
                    if view.kind == ChatKind::Thought && painted.thought_fold != fold {
                        write_thought_fold(ui.ctx(), fold_id, painted.thought_fold);
                    }
                    if !matches!(painted.act, ChatBlockAct::None) {
                        act = painted.act;
                    }
                    if painted.drawn {
                        ui.add_space(grokhub_core::CHAT_BLOCK_GAP);
                    }
                }
                if live {
                    paint_running(ui, "Working…", "");
                }
            });
        if let Some(code) = crate::markdown::take_code_copy(ui.ctx()) {
            act = ChatBlockAct::Copy(code);
        }
        match act {
            ChatBlockAct::Copy(body) => {
                ui.ctx().copy_text(body);
                self.status = "Copied".into();
            }
            ChatBlockAct::Reply(body) | ChatBlockAct::Edit(body) => {
                let draft = self.board_view.composers.entry(card_id.to_string()).or_default();
                *draft = append_composer(draft, &quote_for_reply(&body));
                if !draft.ends_with('\n') {
                    draft.push('\n');
                }
                ui.memory_mut(|m| m.request_focus(board_chat_edit_id(card_id)));
            }
            ChatBlockAct::None => {}
        }
    }

    /// Talk to the agent from the card. Enter sends, Shift+Enter is a new line.
    /// A card with no chat yet starts one with what you typed (or the card itself).
    fn paint_card_composer(
        &mut self,
        ui: &mut egui::Ui,
        card: &BoardCard,
        linked: bool,
        busy_here: bool,
        act: &mut Option<BoardAct>,
    ) {
        let id = card.id.clone();
        let edit_id = board_chat_edit_id(&id);
        let mut text = self.board_view.composers.get(&id).cloned().unwrap_or_default();
        let focused = ui.memory(|m| m.has_focus(edit_id));
        let enter = focused
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let hint = if linked {
            "Reply to the agent…"
        } else if card.status == BoardStatus::Done {
            "Ask the agent about this card…"
        } else {
            "Tell the agent how to work on this…"
        };
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::multiline(&mut text)
                .id(edit_id)
                .hint_text(crate::theme::hint(hint))
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            if busy_here {
                if crate::cards::ghost_pill(ui, "Stop") {
                    *act = Some(BoardAct::Stop);
                }
            } else {
                let label = if linked { "Send" } else { "Work on it" };
                let clicked = crate::cards::white_pill(ui, label);
                if (clicked || enter) && (!text.trim().is_empty() || !linked) {
                    *act = Some(BoardAct::Say {
                        id: id.clone(),
                        text: text.trim().to_string(),
                    });
                    ui.memory_mut(|m| m.request_focus(edit_id));
                }
            }
            if linked && crate::cards::ghost_pill(ui, "Open chat") {
                *act = Some(BoardAct::Open(id.clone()));
            }
        });
        if text.is_empty() {
            self.board_view.composers.remove(&id);
        } else {
            self.board_view.composers.insert(id, text);
        }
    }

    /// Send a line from a card. A card with no chat gets one first, filed as
    /// that chat's run card (so it moves to Done when the run ends), and you stay
    /// on the board to watch it work.
    pub(super) fn say_on_card(&mut self, id: &str, text: &str) -> bool {
        let Some(card) = self.board.iter().find(|c| c.id == id).cloned() else {
            return false;
        };
        if self.running {
            self.board_view.note = Some((id.to_string(), "Finish the open chat first".into()));
            return false;
        }
        if !self.can_agent() {
            self.board_view.note = Some((
                id.to_string(),
                "Install Grok Build (x.ai/cli) or Connect Grok in Settings".into(),
            ));
            return false;
        }
        let thread_id = match card
            .thread_id
            .clone()
            .filter(|tid| self.threads.iter().any(|t| &t.id == tid))
        {
            Some(tid) => tid,
            None => {
                let tid = self.make_card_thread(&card.title);
                if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
                    c.thread_id = Some(tid.clone());
                    if card.automation.is_none() {
                        c.run = true;
                        if c.status != BoardStatus::Done {
                            c.status = BoardStatus::InProgress;
                        }
                    }
                }
                tid
            }
        };
        let message = if text.trim().is_empty() {
            grokhub_core::card_work_prompt(&card)
        } else if card.thread_id.is_none() && card.automation.is_none() {
            format!(
                "{}\n\n{}",
                grokhub_core::card_work_prompt(&card),
                text.trim()
            )
        } else {
            text.trim().to_string()
        };
        // Notes and a Follow up card's newest report go with this message, once each.
        self.card_notes_follow = grokhub_core::take_card_notes_block(&mut self.board, &thread_id);
        if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
            c.notes_sent = grokhub_core::card_notes_hash(&c.notes);
            c.fresh = false;
        }
        self.board_view.note = None;
        self.board_view.composers.remove(id);
        self.chat_job_thread = Some(thread_id);
        self.push_bound_msg("user", message);
        self.flush_board();
        self.kick_model(false);
        true
    }

    /// A chat for a card, listed in History like any other chat.
    pub(super) fn make_card_thread(&mut self, title: &str) -> String {
        let mut thread = crate::threads::ChatThread::new(title.trim(), false);
        thread.title_locked = true;
        thread.accessed_ms = now_ms();
        let id = thread.id.clone();
        let alone = self.threads.is_empty();
        self.threads.push(thread);
        if alone {
            let mut draft = crate::threads::ChatThread::new("Chat", false);
            draft.accessed_ms = now_ms();
            self.threads.insert(0, draft);
            self.thread_idx = 0;
            self.messages = Arc::new(Vec::new());
        }
        id
    }

    /// A scheduled run finished. When it left something to read or act on (a
    /// report you asked for, a question, a problem), it lands in Follow up with a
    /// chat that starts with the report. Later runs of the same automation add
    /// to that card and that chat instead of filing another.
    pub(super) fn file_automation_follow_up(
        &mut self,
        automation_id: &str,
        name: &str,
        instructions: &str,
        reply: &str,
    ) -> bool {
        if automation_id.trim().is_empty()
            || !grokhub_core::automation_needs_follow_up(instructions, reply)
        {
            return false;
        }
        let title = if name.trim().is_empty() { instructions } else { name };
        let (card_id, _) =
            grokhub_core::file_follow_up(&mut self.board, automation_id, title, reply, now_ms());
        let Some(card) = self.board.iter().find(|c| c.id == card_id).cloned() else {
            return false;
        };
        let report = card.report.clone();
        let thread_id = match card
            .thread_id
            .clone()
            .filter(|tid| self.threads.iter().any(|t| &t.id == tid))
        {
            Some(tid) => tid,
            None => {
                let tid = self.make_card_thread(&format!("Follow up · {}", card.title));
                if let Some(c) = self.board.iter_mut().find(|c| c.id == card_id) {
                    c.thread_id = Some(tid.clone());
                }
                tid
            }
        };
        let stamp = format!("Scheduled run · {}", Self::local_clock().hm());
        let entry = ("assistant".to_string(), format!("**{stamp}**\n\n{report}"));
        if thread_id == self.visible_thread_id() {
            self.live_mut().push(entry);
        } else if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
            t.messages_mut().push(entry);
            t.accessed_ms = now_ms();
        }
        // The run's card on the home feed now leads to this card.
        let prefix = format!("done-{}-", automation_id.trim());
        if let Some(feed) = self
            .updates
            .iter_mut()
            .filter(|c| c.kind == UpdateKind::AutomationDone && c.id.starts_with(&prefix))
            .max_by_key(|c| c.created_at)
        {
            feed.action = Some(UpdateAction::OpenWorkboard);
            feed.board_id = Some(card_id.clone());
            feed.body = Some("In Follow up on your workboard. Open it to read and reply.".into());
            self.persist_updates();
        }
        self.flush_board();
        self.persist();
        true
    }
}
