//! Ideas board. Each idea is a short card: its type, a title, and one line.
//! Hover (or a click) opens it in place with the details, the action Apply runs,
//! and a chat with the agent about this card. Edits and the chat stay on the card
//! when you move away, until it is applied or deleted.

use super::*;
use grokhub_core::{
    dismiss_idea, idea_chat_open_line, ideas_board, mark_idea_modified, modified_ideas,
    set_idea_draft, take_card_action, UpdateCard, UpdateKind, UpdateStatus, CARD_ACTION_TAG,
    IDEA_BOARD_MAX, IDEA_MODIFIED_MAX,
};

const IDEA_CARD_H: f32 = 76.0;
const IDEA_GAP: f32 = 8.0;
/// Rest on a card this long before it opens, so sweeping past does not flicker.
const HOVER_OPEN_SECS: f64 = 0.25;
/// Leave an open card this long before it folds back.
const LEAVE_CLOSE_SECS: f64 = 0.6;

/// What the Ideas board remembers between frames. Drafts and chats live on the
/// card itself; this holds the open card and what you are typing to the agent.
#[derive(Clone, Debug, Default)]
pub(super) struct IdeaBoardView {
    pub open: Option<String>,
    /// Opened by a click or from the home feed: stays open until the pointer
    /// has been on it and left, or rests on another card.
    pub pinned: bool,
    pub hover: Option<(String, f64)>,
    pub left_at: Option<f64>,
    pub composers: std::collections::HashMap<String, String>,
    /// A line under one card: why Apply or Send did not go through.
    pub note: Option<(String, String)>,
}

enum IdeaAct {
    Open(String),
    Apply(String),
    Delete(String),
    Board(String),
    Draft { id: String, text: String, save: bool },
    Send(String),
}

fn action_edit_id(id: &str) -> egui::Id {
    egui::Id::new(("idea-action", id))
}

fn chat_edit_id(id: &str) -> egui::Id {
    egui::Id::new(("idea-chat", id))
}

fn type_color(label: &str) -> egui::Color32 {
    match label {
        "Skill" => crate::theme::link(),
        "Automation" => crate::theme::live(),
        _ => crate::theme::muted(),
    }
}

fn type_line(ui: &mut egui::Ui, card: &UpdateCard) {
    ui.horizontal(|ui| {
        let label = card.idea_type_label();
        ui.label(
            RichText::new(label.to_ascii_uppercase())
                .size(crate::theme::FONT_TIP)
                .strong()
                .color(type_color(label)),
        );
        if card.modified {
            ui.label(
                RichText::new("· In progress")
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::subtle()),
            );
        } else if card.built {
            ui.label(
                RichText::new("· On the board")
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::subtle()),
            );
        }
    });
}

/// Type, title, one line. Nothing else until you hover.
fn paint_idea_compact(ui: &mut egui::Ui, card: &UpdateCard, act: &mut Option<IdeaAct>) -> egui::Rect {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, IDEA_CARD_H), egui::Sense::click());
    let stroke = if resp.hovered() {
        crate::theme::border_strong()
    } else {
        crate::theme::border()
    };
    ui.painter().rect(
        rect,
        crate::theme::CARD_RADIUS,
        crate::theme::elevated(),
        egui::Stroke::new(1.0_f32, stroke),
        egui::StrokeKind::Middle,
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(14.0, 10.0))), |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        ui.spacing_mut().item_spacing.y = 3.0;
        type_line(ui, card);
        ui.label(
            RichText::new(&card.title)
                .size(crate::theme::FONT_UI)
                .strong()
                .color(crate::theme::fg()),
        );
        if let Some(body) = card.body.as_deref().filter(|b| !b.trim().is_empty()) {
            ui.label(
                RichText::new(body)
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::muted()),
            );
        }
    });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        *act = Some(IdeaAct::Open(card.id.clone()));
    }
    rect
}

impl Cabin {
    pub(super) fn ui_ideas(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
                // Same header as Automations, Skills, and Workboards.
                let busy = self.ideas_rx.is_some();
                let label = if busy { "Thinking up ideas…" } else { "Suggest ideas" };
                if crate::cards::page_header(ui, "Ideas", label) && !busy {
                    if self.llm_ready() {
                        self.maybe_suggest_ideas(true);
                        self.status = "Thinking up ideas from your recent work…".into();
                    } else {
                        self.status = "Connect Grok in Settings to get ideas".into();
                    }
                }
                self.ensure_useful_ideas();
                let ideas = ideas_board(&self.updates);
                let working = modified_ideas(&self.updates);
                ui.label(
                    RichText::new(format!(
                        "Hover a card to open it. New ideas push the oldest out after {IDEA_BOARD_MAX}; ones you change stay until you apply or delete them ({working} of {IDEA_MODIFIED_MAX})."
                    ))
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::muted()),
                );
                ui.add_space(12.0);
                let mut act = None;
                let mut rects = Vec::new();
                let out = egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if ideas.is_empty() {
                            ui.label(
                                RichText::new("No ideas yet. They come from your recent chats, board, and skills.")
                                    .size(crate::theme::FONT_BODY)
                                    .color(crate::theme::muted()),
                            );
                        }
                        for card in &ideas {
                            let open = self.idea_board.open.as_deref() == Some(card.id.as_str());
                            let rect = if open {
                                self.paint_idea_open(ui, card, &mut act)
                            } else {
                                paint_idea_compact(ui, card, &mut act)
                            };
                            rects.push((card.id.clone(), rect));
                            ui.add_space(IDEA_GAP);
                        }
                    });
                let view = out.inner_rect;
                self.track_idea_hover(&ctx, view, &rects);
                self.apply_idea_act(act);
            });
    }

    /// The card opened in place: details, the action, and the chat.
    fn paint_idea_open(&mut self, ui: &mut egui::Ui, card: &UpdateCard, act: &mut Option<IdeaAct>) -> egui::Rect {
        let id = card.id.clone();
        let thread = card
            .discuss_thread
            .as_deref()
            .and_then(|tid| self.threads.iter().find(|t| t.id == tid))
            .map(|t| (t.id.clone(), t.messages.clone()));
        let thinking = self.running
            && thread.is_some()
            && self.chat_job_thread.as_deref() == thread.as_ref().map(|t| t.0.as_str());
        let stream = if thinking {
            self.stream_buf.clone()
        } else {
            String::new()
        };
        let note = self
            .idea_board
            .note
            .as_ref()
            .filter(|(n, _)| n == &id)
            .map(|(_, t)| t.clone());
        let mut composer = self.idea_board.composers.get(&id).cloned().unwrap_or_default();
        let resp = egui::Frame::NONE
            .fill(crate::theme::elevated())
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border_strong()))
            .corner_radius(crate::theme::CARD_RADIUS)
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    type_line(ui, card);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::cards::ghost_pill(ui, "Delete") {
                            *act = Some(IdeaAct::Delete(id.clone()));
                        }
                        if !card.built && crate::cards::ghost_pill(ui, "To workboard") {
                            *act = Some(IdeaAct::Board(id.clone()));
                        }
                        if crate::cards::white_pill(ui, "Apply") {
                            *act = Some(IdeaAct::Apply(id.clone()));
                        }
                    });
                });
                ui.label(
                    RichText::new(&card.title)
                        .size(crate::theme::FONT_SECTION)
                        .strong()
                        .color(crate::theme::fg()),
                );
                if let Some(body) = card.body.as_deref().filter(|b| !b.trim().is_empty()) {
                    ui.label(
                        RichText::new(body)
                            .size(crate::theme::FONT_BODY)
                            .color(crate::theme::muted()),
                    );
                }
                if let Some(details) = card
                    .details
                    .as_deref()
                    .filter(|d| !d.trim().is_empty() && Some(d.trim()) != card.body.as_deref().map(str::trim))
                {
                    ui.add_space(8.0);
                    crate::cards::section_label(ui, "Details");
                    ui.label(
                        RichText::new(details)
                            .size(crate::theme::FONT_BODY)
                            .color(crate::theme::fg()),
                    );
                }
                ui.add_space(8.0);
                crate::cards::section_label(
                    ui,
                    match card.idea_type_label() {
                        "Skill" => "Steps Apply saves",
                        "Automation" => "What Apply schedules",
                        _ => "What Apply sends",
                    },
                );
                let mut action = card.idea_action();
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut action)
                        .id(action_edit_id(&id))
                        .desired_rows(3)
                        .hint_text(crate::theme::hint("Write what this should do"))
                        .desired_width(f32::INFINITY),
                );
                if edit.changed() {
                    *act = Some(IdeaAct::Draft {
                        id: id.clone(),
                        text: action.clone(),
                        save: false,
                    });
                } else if edit.lost_focus() && card.modified {
                    *act = Some(IdeaAct::Draft {
                        id: id.clone(),
                        text: action.clone(),
                        save: true,
                    });
                }
                ui.add_space(8.0);
                crate::cards::section_label(ui, "Talk it through");
                egui::ScrollArea::vertical()
                    .id_salt(("idea-chat-scroll", id.as_str()))
                    .stick_to_bottom(true)
                    .max_height(220.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let opening = [("assistant".to_string(), idea_chat_open_line(card))];
                        let messages: Vec<(String, String)> = match &thread {
                            Some((_, m)) if !m.is_empty() => m.iter().cloned().collect(),
                            _ => opening.to_vec(),
                        };
                        for (role, text) in &messages {
                            let mine = role == "user";
                            // A stored turn can carry thoughts and tool rows; the card shows replies.
                            let text = if mine {
                                text.clone()
                            } else {
                                grokhub_core::assistant_prose(text)
                            };
                            if text.trim().is_empty() {
                                continue;
                            }
                            ui.label(
                                RichText::new(if mine { "You" } else { "Agent" })
                                    .size(crate::theme::FONT_TIP)
                                    .color(crate::theme::subtle()),
                            );
                            ui.label(
                                RichText::new(&text)
                                    .size(crate::theme::FONT_BODY)
                                    .color(if mine {
                                        crate::theme::fg()
                                    } else {
                                        crate::theme::muted()
                                    }),
                            );
                            ui.add_space(6.0);
                        }
                        if thinking {
                            let live = if stream.trim().is_empty() {
                                "Thinking…".to_string()
                            } else {
                                stream.clone()
                            };
                            ui.label(
                                RichText::new(live)
                                    .size(crate::theme::FONT_BODY)
                                    .color(crate::theme::muted()),
                            );
                        }
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let send_w = 64.0;
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut composer)
                            .id(chat_edit_id(&id))
                            .hint_text(crate::theme::hint("Ask the agent to change this…"))
                            .desired_width((ui.available_width() - send_w).max(120.0)),
                    );
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (enter || crate::cards::ghost_pill(ui, "Send")) && !composer.trim().is_empty() {
                        *act = Some(IdeaAct::Send(id.clone()));
                        // Stay in the box for the next line; the card stays open while you type.
                        ui.memory_mut(|m| m.request_focus(chat_edit_id(&id)));
                    }
                });
                if let Some(note) = &note {
                    ui.label(
                        RichText::new(note)
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::setup()),
                    );
                }
            })
            .response;
        if thinking {
            ui.ctx().request_repaint();
        }
        if composer.is_empty() {
            self.idea_board.composers.remove(&id);
        } else {
            self.idea_board.composers.insert(id, composer);
        }
        resp.rect
    }

    /// Hover opens a card after a short rest; leaving folds it back. A card you
    /// are typing in stays open.
    fn track_idea_hover(&mut self, ctx: &egui::Context, view: egui::Rect, rects: &[(String, egui::Rect)]) {
        let now = ctx.input(|i| i.time);
        let hovered = ctx
            .pointer_hover_pos()
            .filter(|p| view.contains(*p))
            .and_then(|p| rects.iter().find(|(_, r)| r.contains(p)))
            .map(|(id, _)| id.clone());
        let typing = self.idea_board.open.as_deref().is_some_and(|o| {
            let focused = ctx.memory(|m| m.focused());
            focused == Some(action_edit_id(o)) || focused == Some(chat_edit_id(o))
        });
        let v = &mut self.idea_board;
        match hovered {
            Some(h) if v.open.as_deref() == Some(h.as_str()) => {
                v.hover = None;
                v.left_at = None;
                v.pinned = false;
            }
            Some(_) if typing => v.hover = None,
            Some(h) => match v.hover.clone() {
                Some((id, since)) if id == h => {
                    if now - since >= HOVER_OPEN_SECS {
                        v.open = Some(h);
                        v.pinned = false;
                        v.hover = None;
                        v.left_at = None;
                    } else {
                        ctx.request_repaint_after(std::time::Duration::from_millis(60));
                    }
                }
                _ => {
                    v.hover = Some((h, now));
                    ctx.request_repaint_after(std::time::Duration::from_millis(60));
                }
            },
            None => {
                v.hover = None;
                if v.open.is_some() && !v.pinned && !typing {
                    let since = *v.left_at.get_or_insert(now);
                    if now - since >= LEAVE_CLOSE_SECS {
                        v.open = None;
                        v.left_at = None;
                    } else {
                        ctx.request_repaint_after(std::time::Duration::from_millis(60));
                    }
                } else {
                    v.left_at = None;
                }
            }
        }
    }

    fn apply_idea_act(&mut self, act: Option<IdeaAct>) {
        match act {
            Some(IdeaAct::Open(id)) => self.open_idea_on_board(&id),
            Some(IdeaAct::Apply(id)) => self.apply_idea(&id),
            Some(IdeaAct::Delete(id)) => self.delete_idea(&id),
            Some(IdeaAct::Board(id)) => self.build_idea(&id),
            Some(IdeaAct::Draft { id, text, save }) => {
                match set_idea_draft(&mut self.updates, &id, &text) {
                    Ok(()) => {
                        if save {
                            self.persist_updates();
                        }
                    }
                    Err(e) => self.idea_board.note = Some((id, e)),
                }
            }
            Some(IdeaAct::Send(id)) => {
                let text = self.idea_board.composers.get(&id).cloned().unwrap_or_default();
                self.send_idea_card_chat(&id, text);
            }
            None => {}
        }
    }

    /// Open an idea on the Ideas board, from a click there or from the home feed.
    pub(super) fn open_idea_on_board(&mut self, id: &str) {
        let Some(card) = self
            .updates
            .iter()
            .find(|c| c.id == id && c.kind == UpdateKind::Idea)
            .cloned()
        else {
            return;
        };
        let thread_id = match card.discuss_thread.clone() {
            Some(tid) if self.threads.iter().any(|t| t.id == tid) => tid,
            _ => self.make_idea_thread(&card),
        };
        let first = card.status == UpdateStatus::Unread;
        if let Some(saved) = self.updates.iter_mut().find(|c| c.id == id) {
            saved.discuss_thread = Some(thread_id);
            saved.status = UpdateStatus::Opened;
        }
        self.nav = Nav::Ideas;
        self.idea_board.open = Some(id.to_string());
        self.idea_board.pinned = true;
        self.idea_board.hover = None;
        self.idea_board.left_at = None;
        self.persist_updates();
        self.persist();
        if first {
            let key = format!("opened:{}", grokhub_core::engine_slug(&card.title));
            self.engine_note("ideas", &key, &card.title);
        }
    }

    /// A hidden chat for this card. It is not a History chat.
    pub(super) fn make_idea_thread(&mut self, card: &UpdateCard) -> String {
        let mut thread = crate::threads::ChatThread::new(&format!("Discuss · {}", card.title), false);
        thread.background = true;
        thread.title_locked = true;
        thread
            .messages_mut()
            .push(("assistant".into(), idea_chat_open_line(card)));
        let id = thread.id.clone();
        let alone = self.threads.is_empty();
        self.threads.push(thread);
        if alone {
            let mut draft = crate::threads::ChatThread::new("Chat", false);
            draft.accessed_ms = now_ms();
            self.threads.insert(0, draft);
            self.thread_idx = 0;
            self.messages = std::sync::Arc::new(Vec::new());
        }
        id
    }

    /// Talk to the agent inside the card. The card now counts as one you are
    /// working on, so newer ideas never push it out.
    pub(super) fn send_idea_card_chat(&mut self, id: &str, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(card) = self
            .updates
            .iter()
            .find(|c| c.id == id && c.kind == UpdateKind::Idea)
            .cloned()
        else {
            return;
        };
        if self.running {
            self.idea_board.note = Some((id.to_string(), "Finish the open chat first".into()));
            return;
        }
        if !self.can_agent() {
            self.idea_board.note = Some((
                id.to_string(),
                "Install Grok Build (x.ai/cli) or Connect Grok in Settings".into(),
            ));
            return;
        }
        if let Err(e) = mark_idea_modified(&mut self.updates, id) {
            self.idea_board.note = Some((id.to_string(), e));
            return;
        }
        let thread_id = match card.discuss_thread.clone() {
            Some(tid) if self.threads.iter().any(|t| t.id == tid) => tid,
            _ => self.make_idea_thread(&card),
        };
        if let Some(saved) = self.updates.iter_mut().find(|c| c.id == id) {
            saved.discuss_thread = Some(thread_id.clone());
        }
        self.idea_board.note = None;
        self.idea_board.composers.remove(id);
        // Notes from the last Chat send must not ride along into this card's chat.
        self.card_notes_follow = None;
        self.chat_job_thread = Some(thread_id);
        self.push_bound_msg("user", text);
        self.persist_updates();
        self.persist();
        self.kick_model(false);
    }

    /// The hidden brief for a turn in an idea card's chat.
    pub(super) fn idea_talk_brief(&self) -> Option<String> {
        if !self.job_is_idea_talk() {
            return None;
        }
        let job = self.chat_job_thread.as_deref()?;
        self.updates
            .iter()
            .find(|c| c.kind == UpdateKind::Idea && c.discuss_thread.as_deref() == Some(job))
            .map(grokhub_core::idea_card_brief)
    }

    /// When the agent ends a card reply with `CARD_ACTION:`, the card takes the
    /// new action and the chat shows a short note instead of the raw line.
    pub(super) fn sync_idea_card_actions(&mut self) {
        let busy = if self.running {
            self.chat_job_thread.clone()
        } else {
            None
        };
        let mut changed = false;
        for i in 0..self.updates.len() {
            let (card_id, tid) = {
                let c = &self.updates[i];
                let Some(tid) = c.discuss_thread.clone() else {
                    continue;
                };
                if c.kind != UpdateKind::Idea || busy.as_deref() == Some(tid.as_str()) {
                    continue;
                }
                (c.id.clone(), tid)
            };
            let Some(thread) = self.threads.iter_mut().find(|t| t.id == tid) else {
                continue;
            };
            let Some((role, last)) = thread.messages.last() else {
                continue;
            };
            if role != "assistant" || !last.contains(CARD_ACTION_TAG) {
                continue;
            }
            let Some((clean, action)) = take_card_action(last) else {
                continue;
            };
            if let Some(m) = thread.messages_mut().last_mut() {
                m.1 = clean;
            }
            changed = true;
            if let Err(e) = set_idea_draft(&mut self.updates, &card_id, &action) {
                self.idea_board.note = Some((card_id, e));
            }
        }
        if changed {
            self.persist_updates();
            self.persist();
        }
    }

    /// Apply by type: a Skill is saved, an Automation is scheduled, a Suggestion
    /// is sent in a new chat. The card leaves the board once it went through.
    pub(super) fn apply_idea(&mut self, id: &str) {
        let Some(card) = self
            .updates
            .iter()
            .find(|c| c.id == id && c.kind == UpdateKind::Idea)
            .cloned()
        else {
            return;
        };
        let action = card.idea_action();
        let done = match (card.idea_type_label(), card.skill.clone()) {
            ("Skill", Some(mut skill)) => {
                if !action.is_empty() {
                    skill.instructions = Some(action.clone());
                }
                self.add_suggested_skill(&skill);
                if self.status.starts_with("Wrote skill") {
                    Ok(self.status.clone())
                } else {
                    Err(self.status.clone())
                }
            }
            ("Automation", _) => match self.save_schedule(&action) {
                Some(msg) if msg.starts_with("Maximum") => Err(msg),
                Some(msg) => Ok(msg),
                None => self.run_idea_in_chat(&format!(
                    "Set this up as a scheduled automation for me: {action}"
                )),
            },
            _ => self.run_idea_in_chat(&action),
        };
        match done {
            Ok(msg) => {
                if dismiss_idea(&mut self.updates, id) {
                    self.persist_updates();
                }
                let key = format!("applied:{}", grokhub_core::engine_slug(&card.title));
                self.engine_note("ideas", &key, &card.title);
                self.forget_idea_view(id);
                self.drop_idea_thread(card.discuss_thread.as_deref());
                self.status = msg;
            }
            Err(msg) => self.idea_board.note = Some((id.to_string(), msg)),
        }
    }

    fn run_idea_in_chat(&mut self, text: &str) -> Result<String, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Write what Apply should do first".into());
        }
        if self.running {
            return Err("Finish the open chat first".into());
        }
        if !self.can_agent() {
            return Err("Install Grok Build (x.ai/cli) or Connect Grok in Settings".into());
        }
        self.new_thread(false);
        self.nav = Nav::Chat;
        self.send_from_composer(text.to_string());
        Ok("Sent to chat".into())
    }

    /// Delete on the Ideas board. The model hears it was turned down.
    pub(super) fn delete_idea(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        if dismiss_idea(&mut self.updates, id) {
            self.persist_updates();
            // Without these a reworded copy, or the same skill from the nightly review, came back.
            grokhub_core::remember_dismissed_source(&mut self.cfg.feed_pulse, &card.source_id);
            grokhub_core::remember_turned_down(&mut self.cfg.feed_pulse, &card.title);
            self.persist_cfg();
            if !card.title.is_empty() {
                let key = format!("rejected:{}", grokhub_core::engine_slug(&card.title));
                self.engine_note("ideas", &key, &card.title);
            }
            self.drop_idea_thread(card.discuss_thread.as_deref());
            self.status = "Idea deleted".into();
        }
        self.forget_idea_view(id);
    }

    /// A card's hidden chat goes with the card, along with its Grok Build session.
    fn drop_idea_thread(&mut self, thread_id: Option<&str>) {
        let Some(idx) = thread_id.and_then(|tid| {
            self.threads
                .iter()
                .position(|t| t.id == tid && t.background)
        }) else {
            return;
        };
        self.delete_thread_at(idx);
    }

    fn forget_idea_view(&mut self, id: &str) {
        let v = &mut self.idea_board;
        v.composers.remove(id);
        if v.open.as_deref() == Some(id) {
            v.open = None;
            v.pinned = false;
        }
        if v.note.as_ref().is_some_and(|(n, _)| n == id) {
            v.note = None;
        }
    }
}
