//! Chat pane, composer, and bubbles.

use super::*;
use grokhub_core::{chat_find_label, chat_find_rows, chat_find_step};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChatJump {
    None,
    Latest,
    LastYou,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ComposerStackSlot {
    AuthBanner,
    ContextBar,
    SessionTools,
    /// Steer / Queue for typed text during a run, queued messages, background runs.
    LiveWork,
    SlashPalette,
    Chips,
    Attach,
    Voice,
    Pill,
}

/// Ctrl+F inside the open chat. Hits are transcript rows, newest picked first.
#[derive(Default)]
pub(super) struct ChatFind {
    pub open: bool,
    pub query: String,
    pub pick: usize,
    pub want_focus: bool,
    /// Scroll the picked row into view on the next paint.
    pub jump: bool,
    /// The find box had focus last frame. Enter / Esc belong to it, not a permission card.
    pub focused: bool,
    key: (String, String, usize, usize),
    hits: Vec<usize>,
}

impl ChatFind {
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open = true;
            self.want_focus = true;
            self.jump = !self.hits.is_empty();
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.focused = false;
        self.jump = false;
    }

    /// Recount only when the query or the transcript changed.
    pub fn refresh(&mut self, thread_id: &str, views: &[ChatView]) {
        let last = views.last().map(|v| v.body.len()).unwrap_or(0);
        let key = (self.query.clone(), thread_id.to_string(), views.len(), last);
        if key == self.key {
            return;
        }
        let query_changed = key.0 != self.key.0 || key.1 != self.key.1;
        self.key = key;
        self.hits = chat_find_rows(views.iter().map(|v| v.body.as_str()), &self.query);
        if query_changed {
            // Start at the newest hit: the pane is usually parked at the bottom.
            self.pick = self.hits.len().saturating_sub(1);
            self.jump = !self.hits.is_empty();
        } else {
            self.pick = self.pick.min(self.hits.len().saturating_sub(1));
        }
    }

    pub fn step(&mut self, forward: bool) {
        self.pick = chat_find_step(self.pick, self.hits.len(), forward);
        self.jump = !self.hits.is_empty();
    }

    pub fn hits(&self) -> &[usize] {
        if self.open {
            &self.hits
        } else {
            &[]
        }
    }

    pub fn current_row(&self) -> Option<usize> {
        self.hits().get(self.pick).copied()
    }

    pub fn label(&self) -> String {
        chat_find_label(self.pick, self.hits.len(), &self.query)
    }
}

/// Mark a find hit. The picked row gets a ring; the rest a quiet bar.
fn paint_find_mark(ui: &egui::Ui, row: egui::Rect, current: bool) {
    if row.height() <= 0.0 {
        return;
    }
    let bar = egui::Rect::from_min_max(
        egui::pos2(row.min.x - 6.0, row.min.y + 2.0),
        egui::pos2(row.min.x - 3.0, row.max.y - 2.0),
    );
    let ink = if current {
        crate::theme::link()
    } else {
        crate::theme::border_strong()
    };
    ui.painter().rect_filled(bar, 1.5, ink);
    if current {
        ui.painter().rect_stroke(
            row.expand2(egui::vec2(2.0, 2.0)),
            8.0,
            egui::Stroke::new(1.2_f32, crate::theme::link().gamma_multiply(0.7)),
            egui::StrokeKind::Middle,
        );
    }
}

/// Compact, Copy session, Export. Same gates the composer pills used.
pub(super) fn session_menu_enabled(has_messages: bool, usage_empty: bool) -> (bool, bool, bool) {
    let compact = has_messages || !usage_empty;
    (compact, has_messages, has_messages)
}

fn session_menu_row(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let color = if enabled {
        crate::theme::fg()
    } else {
        crate::theme::subtle()
    };
    let resp = crate::theme::felt_label_button(
        ui,
        label,
        egui::Color32::TRANSPARENT,
        color,
        8.0,
        egui::vec2(176.0, 32.0),
        None,
        false,
    );
    resp.clicked() && enabled
}

pub(super) fn composer_stack_order() -> &'static [ComposerStackSlot] {
    &[
        ComposerStackSlot::AuthBanner,
        ComposerStackSlot::ContextBar,
        ComposerStackSlot::SessionTools,
        ComposerStackSlot::LiveWork,
        ComposerStackSlot::SlashPalette,
        ComposerStackSlot::Attach,
        ComposerStackSlot::Voice,
        ComposerStackSlot::Pill,
        ComposerStackSlot::Chips,
    ]
}

pub(super) fn empty_home_side_gap(avail_w: f32, pane_w: f32) -> f32 {
    ((avail_w - pane_w) * 0.5).max(0.0)
}

/// Chat-pill top so the box stays on the pane midline.
pub(super) fn empty_home_composer_top(avail_h: f32, pill_h: f32) -> f32 {
    ((avail_h - pill_h) * 0.5).max(16.0)
}

/// Greeting top: middle of the title-bar-to-composer gap, using wrapped height.
pub(super) fn empty_home_greet_top(gap_h: f32, greet_h: f32, gap_below: f32) -> f32 {
    let usable = (gap_h - gap_below).max(0.0);
    if greet_h >= usable {
        0.0
    } else {
        (usable - greet_h) * 0.5
    }
}

pub(super) fn greeting_galley_h(ui: &egui::Ui, text: &str, wrap_w: f32) -> f32 {
    let font = crate::theme::title_font(crate::theme::GREET_HERO);
    let galley = ui.fonts_mut(|f| {
        f.layout(
            text.to_string(),
            font,
            crate::theme::muted(),
            wrap_w.max(1.0),
        )
    });
    galley.size().y.max(crate::theme::GREET_HERO)
}

/// Empty-composer placeholder. Sits on the pill fill: quiet, but still legible (~3:1).
pub(super) fn composer_hint_ink() -> egui::Color32 {
    crate::theme::blend_color(crate::theme::subtle(), crate::theme::muted(), 0.5)
}

pub(super) fn consume_enter_keys(ui: &mut egui::Ui) {
    ui.input_mut(|i| {
        i.events.retain(|ev| match ev {
            egui::Event::Key {
                key: egui::Key::Enter,
                ..
            } => false,
            egui::Event::Text(t) if t == "\n" || t == "\r" || t == "\r\n" => false,
            _ => true,
        });
    });
}

/// A first, unmodified press of `key`. A held key's repeats and a chord such as
/// Shift+Esc never answer a permission card.
pub(super) fn bare_press(ui: &egui::Ui, key: egui::Key) -> bool {
    ui.input(|i| {
        i.events.iter().any(|ev| {
            matches!(ev, egui::Event::Key {
                key: k,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } if *k == key && modifiers.is_none())
        })
    })
}

/// Drop this frame's presses of `key`: a field that used it (Esc cancels a rename,
/// Enter commits one) must not also answer a permission card painted later.
pub(super) fn drop_key(ui: &egui::Ui, key: egui::Key) {
    ui.input_mut(|i| {
        i.events
            .retain(|ev| !matches!(ev, egui::Event::Key { key: k, .. } if *k == key))
    });
}

/// A menu, popup or sheet is over the chat (this frame or the last), so Esc and
/// Enter are its keys. The passive jump-to-latest pill does not count.
pub(super) fn overlay_over_chat(ctx: &egui::Context) -> bool {
    let jump = egui::Id::new("chat-jump");
    egui::Popup::is_any_open(ctx)
        || ctx.memory(|m| {
            m.areas()
                .visible_layer_ids()
                .iter()
                .any(|l| l.order == egui::Order::Foreground && l.id != jump)
        })
}

/// Shift+Enter breaks the line like Ctrl+Enter: its Enter presses take the
/// Command bit, so the composer's `return_key` inserts the newline at the cursor.
pub(super) fn shift_enter_as_newline(events: &mut [egui::Event]) {
    for ev in events {
        if let egui::Event::Key {
            key: egui::Key::Enter,
            modifiers,
            ..
        } = ev
        {
            if modifiers.shift && !modifiers.alt && !modifiers.ctrl && !modifiers.command {
                modifiers.command = true;
            }
        }
    }
}

/// Enter sends. Control+Enter and Shift+Enter are left for TextEdit (`return_key`)
/// to insert a newline.
pub(super) fn take_focused_composer(
    ui: &mut egui::Ui,
    composer: &mut String,
    focused: bool,
) -> Option<String> {
    if !focused {
        return None;
    }
    ui.input_mut(|i| shift_enter_as_newline(&mut i.events));
    let (enter, control) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.modifiers.ctrl || i.modifiers.command || (i.modifiers.shift && !i.modifiers.alt),
        )
    });
    match composer_enter(enter, control) {
        Some(ComposerEnter::Send) => {
            consume_enter_keys(ui);
            if composer.ends_with('\n') {
                composer.pop();
            }
            Some(std::mem::take(composer))
        }
        Some(ComposerEnter::Newline) => None,
        None => None,
    }
}

/// Edit puts your message in the composer. A draft already there stays on top.
pub(super) fn edit_into_composer(draft: &str, body: &str) -> String {
    if draft.trim().is_empty() {
        body.to_string()
    } else {
        format!("{}\n\n{body}", draft.trim_end())
    }
}

pub(super) fn slash_pick_step(pick: usize, len: usize, dir: i8) -> usize {
    if len == 0 {
        return 0;
    }
    let clamped = pick.min(len - 1);
    match dir {
        1 => (clamped + 1).min(len - 1),
        -1 => clamped.saturating_sub(1),
        _ => clamped,
    }
}

/// Tab / click accept. `Some` means run the command this frame.
pub(super) fn slash_pick_take(
    composer: &mut String,
    insert: &str,
    run_on_pick: bool,
) -> Option<String> {
    *composer = insert.to_string();
    if run_on_pick {
        Some(std::mem::take(composer))
    } else {
        None
    }
}

pub(super) fn slash_pick_retain(pick: usize, list_changed: bool, len: usize) -> usize {
    if list_changed || len == 0 {
        0
    } else {
        slash_pick_step(pick, len, 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ChatBlockAct {
    None,
    Copy(String),
    Reply(String),
    /// Your own message back into the composer to fix and send again.
    Edit(String),
}

pub(super) struct ChatBlockPaint {
    pub(super) act: ChatBlockAct,
    pub(super) drawn: bool,
    pub(super) thought_fold: ThoughtFold,
}

/// Failed turns land as an assistant block that starts with `Error:` (chat_job / mod.rs).
pub(super) fn is_error_reply(body: &str) -> bool {
    body.trim_start().starts_with("Error:")
}

pub(super) fn paint_speech_bubble(
    ui: &mut egui::Ui,
    body: &str,
    user: bool,
    markdown: bool,
) -> egui::Response {
    let body = crate::markdown::display_text(body);
    let avail = clamp_row_width(ui.available_width().min(ui.max_rect().width()));
    let mark_w = if user { 0.0 } else { 22.0 };
    let bubble_avail = (avail - mark_w).max(1.0);
    let wrap = bubble_wrap_width(bubble_avail, BUBBLE_PAD_X);
    let content_w = if markdown && !user {
        crate::markdown::measure_markdown_width(ui, body, wrap)
    } else {
        crate::markdown::measure_text(ui, body, wrap).x
    };
    let inner_w = content_w.max(1.0).min(wrap);
    let mut outer_w = bubble_outer_width(bubble_avail, inner_w, BUBBLE_PAD_X);
    let gap = if user {
        ui.spacing().item_spacing.x.max(0.0)
    } else {
        0.0
    };
    // Always keep `gap` out of the bubble. The spacer below is the rest of the
    // row, so a typical pane reserves item_spacing even when the 84% cap
    // already fits, and a narrow pane does not subtract the gap twice.
    outer_w = clamp_bubble_outer(bubble_avail, outer_w, gap);
    let inner_w = inner_w.min((outer_w - BUBBLE_PAD_X * 2.0).max(1.0));
    // Pad with spaces, not Frame inner_margin: egui clips the rounded fill
    // against the content origin, which ate the first glyphs on Windows 2.10.6.
    let error = !user && is_error_reply(body);
    let frame = egui::Frame::NONE
        .fill(if user {
            crate::theme::bubble_user()
        } else if error {
            // A failed turn must not read as an answer: faint red wash plus a red hairline.
            crate::theme::blend_color(crate::theme::bubble_assistant(), crate::theme::offline(), 0.08)
        } else {
            crate::theme::bubble_assistant()
        })
        .stroke(if error {
            egui::Stroke::new(1.0_f32, crate::theme::blend_color(crate::theme::border(), crate::theme::offline(), 0.6))
        } else {
            egui::Stroke::NONE
        })
        .corner_radius(crate::theme::USER_BUBBLE_RADIUS)
        .inner_margin(egui::Margin::ZERO);
    let paint_inner = |ui: &mut egui::Ui| {
        ui.set_width(outer_w);
        ui.set_max_width(outer_w);
        ui.add_space(BUBBLE_PAD_Y);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(BUBBLE_PAD_X);
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                ui.set_max_width(inner_w);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                if user || !markdown {
                    let job = crate::markdown::wrapped_job(
                        ui,
                        body,
                        inner_w,
                        crate::theme::fg(),
                    );
                    ui.add(egui::Label::new(job).wrap().selectable(true));
                } else {
                    crate::markdown::show(ui, body);
                }
            });
            ui.add_space(BUBBLE_PAD_X);
        });
        ui.add_space(BUBBLE_PAD_Y);
    };
    let mut resp = None;
    if user {
        ui.scope(|ui| {
            ui.set_max_width(avail);
            ui.horizontal(|ui| {
                ui.set_max_width(avail);
                let lead = (avail - outer_w).max(0.0);
                if lead > 0.0 {
                    ui.add_space(lead);
                }
                ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                    ui.set_max_width(outer_w);
                    resp = Some(frame.show(ui, paint_inner).response);
                });
            });
        });
        return resp.expect("speech bubble");
    }
    ui.scope(|ui| {
        ui.set_max_width(avail);
        ui.horizontal_top(|ui| {
            ui.set_max_width(avail);
            ui.spacing_mut().item_spacing.x = 6.0;
            let mark = crate::theme::mark(ui.ctx());
            let (mark_rect, _) =
                ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            ui.painter().image(
                mark.id(),
                mark_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                crate::theme::muted(),
            );
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                ui.set_max_width(outer_w);
                resp = Some(frame.show(ui, paint_inner).response);
            });
        });
    });
    resp.expect("speech bubble")
}

pub(super) struct MsgActsPaint {
    pub act: ChatBlockAct,
    /// Union of the Copy and Reply hit rects. Layout tests read it.
    #[cfg(test)]
    pub row: egui::Rect,
}

/// Hit width of one quiet label button. Matches [`crate::theme::felt_label_button`]
/// with a zero minimum size and the chrome font.
fn label_hit_width(ui: &egui::Ui, label: &str) -> f32 {
    let font = egui::FontId::proportional(crate::theme::FONT_CHROME);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, egui::Color32::PLACEHOLDER));
    let pad = ui.style().spacing.button_padding.x;
    galley.size().x + pad * 2.0
}

/// Quiet actions under a bubble. Your own messages also get Edit.
pub(super) fn msg_act_labels(user: bool) -> &'static [&'static str] {
    if user {
        &["Copy", "Edit", "Reply"]
    } else {
        &["Copy", "Reply"]
    }
}

/// Copy, the gaps, and Reply (plus Edit on your own message). A short user
/// bubble is narrower than this row.
pub(super) fn msg_acts_row_width(ui: &egui::Ui, user: bool) -> f32 {
    let gap = ui.spacing().item_spacing.x.max(0.0);
    let labels = msg_act_labels(user);
    labels
        .iter()
        .map(|l| {
            if *l == "Copy" {
                crate::theme::copy_hit_width(ui)
            } else {
                label_hit_width(ui, l)
            }
        })
        .sum::<f32>()
        + gap * labels.len().saturating_sub(1) as f32
}

pub(super) fn paint_msg_acts(
    ui: &mut egui::Ui,
    user: bool,
    body: &str,
    avail: f32,
    align_w: f32,
) -> MsgActsPaint {
    let mut act = ChatBlockAct::None;
    #[cfg(test)]
    let mut bounds: Option<egui::Rect> = None;
    let mut paint = |ui: &mut egui::Ui| {
        for &label in msg_act_labels(user) {
            let flash_id = ui.next_auto_id();
            let shown = if label == "Copy" {
                crate::theme::copy_flash_label(ui.ctx(), flash_id)
            } else {
                label
            };
            let min_w = if label == "Copy" {
                crate::theme::copy_hit_width(ui)
            } else {
                0.0
            };
            let resp = crate::theme::felt_label_button(
                ui,
                shown,
                egui::Color32::TRANSPARENT,
                crate::theme::muted(),
                6.0,
                egui::vec2(min_w, 0.0),
                None,
                false,
            );
            let resp = if label == "Edit" {
                resp.on_hover_text("Put this message back in the composer to fix and send again")
            } else {
                resp
            };
            if resp.clicked() {
                if label == "Copy" {
                    crate::theme::mark_copy_clicked(ui.ctx(), resp.id);
                }
                act = match label {
                    "Copy" => ChatBlockAct::Copy(body.to_string()),
                    "Edit" => ChatBlockAct::Edit(body.to_string()),
                    _ => ChatBlockAct::Reply(body.to_string()),
                };
            }
            #[cfg(test)]
            {
                bounds = Some(match bounds {
                    Some(rect) => rect.union(resp.rect),
                    None => resp.rect,
                });
            }
        }
    };
    // User bubbles sit on the right. Copy+Reply is wider than a short bubble
    // ("hey"). A fixed 96px floor still let Reply draw past the window.
    // Size the lead from the real row so Reply ends on the bubble's right edge.
    let gap = ui.spacing().item_spacing.x.max(0.0);
    let acts_w = msg_acts_row_width(ui, user);
    ui.scope(|ui| {
        ui.set_max_width(avail);
        ui.horizontal(|ui| {
            ui.set_max_width(avail);
            ui.spacing_mut().item_spacing.x = 0.0;
            if user {
                let anchor = align_w.max(acts_w);
                let lead = (avail - anchor).max(0.0);
                if lead > 0.0 {
                    ui.add_space(lead);
                }
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                paint(ui);
            });
        });
    });
    MsgActsPaint {
        act,
        #[cfg(test)]
        row: bounds.unwrap_or(egui::Rect::NOTHING),
    }
}

/// A cabin result (GL-05): the plain reply bubble, upright and in the body
/// colour, the same on the newest row and on an old one.
pub(super) fn paint_result_bubble(ui: &mut egui::Ui, body: &str) -> egui::Response {
    paint_speech_bubble(ui, body, false, true)
}

pub(super) fn paint_thought_bubble(ui: &mut egui::Ui, body: &str) -> egui::Response {
    let body = crate::markdown::display_text(body);
    let avail = clamp_row_width(ui.available_width().min(ui.max_rect().width()));
    let wrap = (avail - 8.0).max(1.0);
    let frame = egui::Frame::NONE
        .fill(egui::Color32::TRANSPARENT)
        .inner_margin(egui::Margin::symmetric(0, 2));
    let mut resp = None;
    ui.scope(|ui| {
        ui.set_max_width(avail);
        ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
            ui.set_max_width(avail);
            resp = Some(
                frame
                    .show(ui, |ui| {
                        ui.set_max_width(wrap);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        ui.add(
                            egui::Label::new(
                                RichText::new(body)
                                    .size(crate::theme::FONT_META)
                                    .italics()
                                    .color(crate::theme::subtle()),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    })
                    .response,
            );
        });
    });
    resp.expect("thought bubble")
}

/// Temp-data key for per-row heights. Pane width is part of the key so a
/// resize does not reuse wrap heights measured at another width.
/// Height cache for the live turn's rows, keyed like stored rows by pane width.
pub(super) fn live_row_height_id(thread_id: &str, pane_w: f32) -> egui::Id {
    egui::Id::new(("cabin-live-row-h", thread_id, pane_width_bucket(pane_w)))
}

/// Which block a live row starts on. A text block has its own fold slot; a tool
/// row (slot 0) is named by its first call. A cached height is reused only for
/// the same row, so the next turn's rows never inherit this turn's heights.
pub(super) fn live_row_ident(b: &LiveBlock) -> u64 {
    if b.fold_slot != 0 {
        b.fold_slot
    } else {
        egui::Id::new(("cabin-live-tool-row", b.tool_id.as_str())).value()
    }
}

/// Everything besides its own text that sets a live row's height: its fold, its
/// neighbours' folds, and whether it carries the Copy / Reply acts. A change here
/// (a click, Minimize all, a newer reply) repaints the row instead of reusing it.
pub(super) fn live_row_shape(
    fold: ThoughtFold,
    prev_expanded: bool,
    next_expanded: bool,
    next_drawn_thought: bool,
    last_say: bool,
) -> u8 {
    u8::from(prev_expanded)
        | u8::from(next_expanded) << 1
        | u8::from(next_drawn_thought) << 2
        | u8::from(last_say) << 3
        | u8::from(fold.paints_body()) << 4
        | u8::from(fold.paints_row()) << 5
}

pub(super) fn chat_row_height_id(thread_id: &str, pane_w: f32) -> egui::Id {
    egui::Id::new(("cabin-chat-row-h", thread_id, pane_width_bucket(pane_w)))
}

pub(super) fn pane_width_bucket(pane_w: f32) -> i32 {
    pane_w.round() as i32
}

/// egui id salt for one transcript row. Thread and index stay put when a
/// culled neighbor skips `paint_chat_block`, so selection and hover do not
/// slide onto the next visible row.
pub(super) fn chat_row_id_salt(thread_id: &str, index: usize) -> (&str, usize) {
    (thread_id, index)
}

pub(super) fn chat_row_outside_clip(
    origin: egui::Pos2,
    width: f32,
    height: f32,
    clip: egui::Rect,
) -> bool {
    let row = egui::Rect::from_min_size(origin, egui::vec2(width.max(1.0), height));
    row.max.y <= clip.min.y
        || row.min.y >= clip.max.y
        || row.max.x <= clip.min.x
        || row.min.x >= clip.max.x
}

/// The composer field paints no frame of its own, only a 2px side inset.
#[cfg(feature = "fx")]
impl Cabin {
    /// GPU glow behind the pill while a reply streams, on wgpu with the Settings switch on.
    fn paint_composer_glow(&self, ui: &egui::Ui, pill: egui::Rect) {
        if !self.cfg.composer_glow || !crate::fx::renderer_is_wgpu() {
            return;
        }
        // Keep painting while streaming or while the 200ms settle to idle α is in flight.
        let settle = crate::theme::animate_selection_secs(
            ui,
            egui::Id::new("composer-glow-stream"),
            self.running,
            grokhub_core::GLOW_SETTLE_SECS,
        );
        if self.running || settle > 0.01 {
            crate::fx::paint_composer_glow_at(ui, pill, self.running);
        }
    }
}

fn composer_field_frame() -> egui::Frame {
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(2, 0))
}

/// Keep a cached row's height when it sits fully outside the clip.
/// Returns true when the caller should skip painting that row.
pub(super) fn reserve_offscreen_chat_row(ui: &mut egui::Ui, cached_h: f32) -> bool {
    if cached_h <= 0.0 {
        return false;
    }
    let width = ui.available_width().max(1.0);
    if !chat_row_outside_clip(ui.cursor().min, width, cached_h, ui.clip_rect()) {
        return false;
    }
    ui.add_space(cached_h);
    true
}

/// Stored-thought fold key for one view. Only thoughts fold; other rows get 0.
fn view_fold_key(view: &ChatView) -> u64 {
    if view.kind == ChatKind::Thought {
        thought_body_key(&view.body)
    } else {
        0
    }
}

/// `kind` is `"slot"` while the thought is a live block, `"body"` once it is stored.
/// The body key is [`thought_body_key`], so a collapse survives that handoff.
pub(super) fn thought_fold_id(thread_id: &str, kind: &str, key: u64) -> egui::Id {
    egui::Id::new(("cabin-thought-fold", thread_id, kind, key))
}

#[cfg(test)]
pub(super) fn read_thought_fold(ctx: &egui::Context, id: egui::Id) -> ThoughtFold {
    ctx.data(|d| d.get_temp(id)).unwrap_or_default()
}

pub(super) fn write_thought_fold(ctx: &egui::Context, id: egui::Id, fold: ThoughtFold) {
    ctx.data_mut(|d| d.insert_temp(id, fold));
}

pub(super) fn session_thoughts_collapsed_id(thread_id: &str) -> egui::Id {
    egui::Id::new(("cabin-session-thoughts-collapsed", thread_id))
}

pub(super) fn read_session_thoughts_collapsed(ctx: &egui::Context, thread_id: &str) -> bool {
    ctx.data(|d| d.get_temp(session_thoughts_collapsed_id(thread_id)))
        .unwrap_or(false)
}

pub(super) fn write_session_thoughts_collapsed(ctx: &egui::Context, thread_id: &str, on: bool) {
    ctx.data_mut(|d| d.insert_temp(session_thoughts_collapsed_id(thread_id), on));
}

pub(super) fn resolve_thought_fold(
    ctx: &egui::Context,
    id: egui::Id,
    start_collapsed: bool,
) -> ThoughtFold {
    let stored = ctx.data(|d| d.get_temp(id));
    grokhub_core::effective_thought_fold(stored, start_collapsed)
}

/// Expand writes only this thought. Collapse asks the caller to fold the session.
fn note_thought_fold_click(
    ctx: &egui::Context,
    thread_id: &str,
    slot: Option<u64>,
    body_key: u64,
    before: ThoughtFold,
    after: ThoughtFold,
    collapse_session: &mut bool,
) {
    let Some(act) = grokhub_core::thought_fold_transition(before, after) else {
        return;
    };
    match act {
        grokhub_core::ThoughtFoldAct::Minimize => *collapse_session = true,
        grokhub_core::ThoughtFoldAct::Expand => {
            if let Some(slot) = slot {
                write_thought_fold(
                    ctx,
                    thought_fold_id(thread_id, "slot", slot),
                    ThoughtFold::Expanded,
                );
            }
            write_thought_fold(
                ctx,
                thought_fold_id(thread_id, "body", body_key),
                ThoughtFold::Expanded,
            );
        }
        grokhub_core::ThoughtFoldAct::Hide => {
            if let Some(slot) = slot {
                write_thought_fold(
                    ctx,
                    thought_fold_id(thread_id, "slot", slot),
                    ThoughtFold::Hidden,
                );
            }
            write_thought_fold(
                ctx,
                thought_fold_id(thread_id, "body", body_key),
                ThoughtFold::Hidden,
            );
        }
    }
}

fn minimize_session_thoughts(
    ctx: &egui::Context,
    thread_id: &str,
    stored: &[ChatView],
    live: &[LiveBlock],
) {
    write_session_thoughts_collapsed(ctx, thread_id, true);
    for view in stored.iter().filter(|v| v.kind == ChatKind::Thought) {
        write_thought_fold(
            ctx,
            thought_fold_id(thread_id, "body", thought_body_key(&view.body)),
            ThoughtFold::Minimized,
        );
    }
    for block in live.iter().filter(|b| b.kind == LiveKind::Thought) {
        write_thought_fold(
            ctx,
            thought_fold_id(thread_id, "slot", block.fold_slot),
            ThoughtFold::Minimized,
        );
        write_thought_fold(
            ctx,
            thought_fold_id(thread_id, "body", thought_body_key(&block.body)),
            ThoughtFold::Minimized,
        );
    }
}

/// One quiet collapse control. Expanded shows a down chevron; minimized shows a right chevron.
/// Same toggle as Minimize: the thought body hides, the short row stays. Hide is not painted.
pub(super) fn paint_thought_fold_buttons(ui: &mut egui::Ui, fold: ThoughtFold) -> ThoughtFold {
    let mut fold = fold;
    for label in thought_fold_controls(fold) {
        let resp = paint_thought_collapse_hit(ui, label, fold.paints_body());
        if resp.clicked() {
            if let Some(act) = thought_control_act(label) {
                fold = fold.apply(act);
            }
        }
    }
    fold
}

fn paint_thought_collapse_hit(ui: &mut egui::Ui, label: &str, expanded: bool) -> egui::Response {
    let color = crate::theme::subtle();
    let font = egui::FontId::proportional(11.0);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, color));
    let chev = 8.0_f32;
    let gap = 3.0;
    let pad = egui::vec2(2.0, 1.0);
    let size = egui::vec2(
        pad.x * 2.0 + chev + gap + galley.size().x,
        (galley.size().y + pad.y * 2.0).max(14.0),
    );
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 3.0, crate::theme::hover());
    }
    let cy = rect.center().y;
    paint_thought_chevron(
        ui.painter(),
        egui::pos2(rect.min.x + pad.x, cy),
        chev,
        expanded,
        color,
    );
    ui.painter().galley(
        egui::pos2(
            rect.min.x + pad.x + chev + gap,
            cy - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    crate::theme::pointing(resp)
}

fn paint_thought_chevron(
    painter: &egui::Painter,
    origin: egui::Pos2,
    size: f32,
    down: bool,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.1_f32, color);
    let mid_y = origin.y;
    let left = origin.x;
    let right = origin.x + size;
    let top = mid_y - size * 0.32;
    let bot = mid_y + size * 0.32;
    if down {
        painter.line_segment(
            [egui::pos2(left, top), egui::pos2(left + size * 0.5, bot)],
            stroke,
        );
        painter.line_segment(
            [egui::pos2(left + size * 0.5, bot), egui::pos2(right, top)],
            stroke,
        );
    } else {
        painter.line_segment(
            [egui::pos2(left + 1.0, top), egui::pos2(right - 1.0, mid_y)],
            stroke,
        );
        painter.line_segment(
            [egui::pos2(right - 1.0, mid_y), egui::pos2(left + 1.0, bot)],
            stroke,
        );
    }
}

pub(super) fn paint_chat_block(
    ui: &mut egui::Ui,
    block: &ChatView,
    thought_label: bool,
    thought_acts: bool,
    thought_fold: ThoughtFold,
) -> ChatBlockPaint {
    paint_chat_block_with(ui, block, thought_label, thought_acts, thought_fold, true)
}

/// A progress reply mid-turn is its own bubble, but only the turn's last
/// reply carries Copy / Reply, so a busy turn is not a ladder of buttons.
pub(super) fn paint_chat_block_with(
    ui: &mut egui::Ui,
    block: &ChatView,
    thought_label: bool,
    thought_acts: bool,
    thought_fold: ThoughtFold,
    reply_acts: bool,
) -> ChatBlockPaint {
    let avail = clamp_row_width(ui.available_width().min(ui.max_rect().width()));
    let bubble_w = crate::markdown::bubble_width(avail);
    if !thought_fold_draws(block.kind, thought_fold) {
        return ChatBlockPaint {
            act: ChatBlockAct::None,
            drawn: false,
            thought_fold,
        };
    }
    match block.kind {
        ChatKind::User => {
            let resp = paint_speech_bubble(ui, &block.body, true, false);
            ChatBlockPaint {
                act: paint_msg_acts(ui, true, &block.body, avail, resp.rect.width()).act,
                drawn: true,
                thought_fold,
            }
        }
        ChatKind::Assistant => {
            let resp = paint_speech_bubble(ui, &block.body, false, true);
            let act = if reply_acts {
                paint_msg_acts(ui, false, &block.body, avail, resp.rect.width()).act
            } else {
                ChatBlockAct::None
            };
            ChatBlockPaint {
                act,
                drawn: true,
                thought_fold,
            }
        }
        ChatKind::Result => {
            // GL-05: a slash or system result keeps this one bubble as it ages.
            // It never takes the thought frame, its slanted text, or its label.
            let resp = paint_result_bubble(ui, &block.body);
            let act = if reply_acts {
                paint_msg_acts(ui, false, &block.body, avail, resp.rect.width()).act
            } else {
                ChatBlockAct::None
            };
            ChatBlockPaint {
                act,
                drawn: true,
                thought_fold,
            }
        }
        ChatKind::Thought => {
            let mut fold = thought_fold;
            let mut act = ChatBlockAct::None;
            // Paint the fold we were given. A click is stored for the next frame
            // so minimize and expand do not draw both layouts at once.
            if fold.paints_body() {
                ui.horizontal(|ui| {
                    if thought_label {
                        ui.label(
                            RichText::new("Thought process")
                                .size(crate::theme::FONT_META)
                                .italics()
                                .color(crate::theme::muted()),
                        );
                    }
                    fold = paint_thought_fold_buttons(ui, fold);
                });
                ui.add_space(4.0);
                let resp = paint_thought_bubble(ui, &block.body);
                if thought_acts {
                    act = paint_msg_acts(ui, false, &block.body, avail, resp.rect.width()).act;
                }
            } else {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(THOUGHT_ROW_LABEL)
                            .size(crate::theme::FONT_META)
                            .italics()
                            .color(crate::theme::muted()),
                    );
                    fold = paint_thought_fold_buttons(ui, fold);
                });
            }
            ChatBlockPaint {
                act,
                drawn: true,
                thought_fold: fold,
            }
        }
        ChatKind::Tool => {
            let title = if block.title.is_empty() {
                "Work"
            } else {
                block.title.as_str()
            };
            let rows = decode_tool_rows(&block.body);
            let failed = rows
                .as_ref()
                .is_some_and(|r| r.iter().any(|x| tool_status_failed(&x.status)));
            egui::CollapsingHeader::new(
                RichText::new(title)
                    .size(crate::theme::FONT_META)
                    .color(tool_header_color(failed)),
            )
            .id_salt(("chat-tool", title, block.body.as_str()))
            .default_open(false)
            .show(ui, |ui| {
                ui.set_max_width(bubble_w);
                match rows.as_deref() {
                    Some(rows) => paint_tool_rows(ui, rows),
                    None => {
                        ui.label(
                            RichText::new(&block.body)
                                .size(crate::theme::FONT_META)
                                .color(crate::theme::muted()),
                        );
                    }
                }
            });
            ChatBlockPaint {
                act: ChatBlockAct::None,
                drawn: true,
                thought_fold,
            }
        }
    }
}

impl Cabin {
    pub(super) fn ui_chat(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let empty = self.messages.is_empty();
        if !empty {
            egui::Panel::bottom("composer")
                .frame(
                    egui::Frame::NONE
                        .fill(crate::theme::bg())
                        .inner_margin(egui::Margin {
                            left: 32,
                            right: 32,
                            top: 10,
                            bottom: 22,
                        }),
                )
                .show(ui, |ui| {
                    self.ui_composer_stack(ui);
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin {
                        left: 8,
                        right: 16,
                        top: 12,
                        bottom: 8,
                    }),
            )
            .show(ui, |ui| {
                if empty {
                    self.ui_empty_home(ui);
                    return;
                }
                let avail = clamp_row_width(ui.available_width());
                let pane = avail;
                if self.find.open {
                    let tid = self.visible_thread_id();
                    self.cached_chat_views();
                    let (views, find) = (&self.chat_views, &mut self.find);
                    find.refresh(&tid, views);
                    if find.jump {
                        // A find jump wins over pinning the tail this frame.
                        self.chat_tail_frames = 0;
                    }
                }
                let find_hits = self.find.hits().to_vec();
                let find_row = self.find.current_row();
                let find_jump = self.find.jump;
                let mut find_jumped = false;
                let pin_tail = self.chat_tail_frames > 0;
                let out = egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_width(pane);
                        ui.set_max_width(pane);
                        let thinking = self.thinking_here();
                        let live = thinking && !self.live_blocks.is_empty();
                        let mut act = ChatBlockAct::None;
                        let jump_you = self.jump_last_you;
                        let mut jumped_you = false;
                        let fold_thread = self.visible_thread_id();
                        let start_collapsed = grokhub_core::session_thoughts_start_collapsed(
                            self.cfg.always_collapse_thoughts,
                            read_session_thoughts_collapsed(ui.ctx(), &fold_thread),
                        );
                        let mut collapse_session = false;
                        let mut privacy_revoke: Option<String> = None;
                        let mut skill_hit: Option<super::skill_undo::SkillRow> = None;
                        {
                            let thread_id = fold_thread.clone();
                            let row_h_id = chat_row_height_id(&thread_id, pane);
                            self.cached_chat_views();
                            // SY-04: the newest /privacy bubble gets a ghost Revoke per
                            // live grant. Read the ledger only when that bubble exists.
                            let privacy_at = super::privacy_ui::newest_privacy_row(&self.chat_views);
                            let privacy_grants = if privacy_at.is_some() {
                                self.privacy_revoke_rows()
                            } else {
                                Vec::new()
                            };
                            // The newest /skills changes bubble gets Undo / Restore rows.
                            let skill_at = super::skill_undo::newest_skill_changes_row(&self.chat_views);
                            let skill_rows =
                                if skill_at.is_some() { self.skill_rows_now() } else { Vec::new() };
                            let (views, keys) = (&self.chat_views, &self.chat_view_keys);
                            let shown = if live {
                                views_up_to_last_user(views)
                            } else {
                                views
                            };
                            let fold_ctx = ui.ctx().clone();
                            // Stored thought fold for row `j`, from the cached body key.
                            let fold_at = |j: usize| {
                                resolve_thought_fold(
                                    &fold_ctx,
                                    thought_fold_id(&thread_id, "body", keys[j]),
                                    start_collapsed,
                                )
                            };
                            let last_you_i = shown.iter().rposition(|v| v.kind == ChatKind::User);
                            let prev_heights: Vec<f32> =
                                ui.ctx().data(|d| d.get_temp(row_h_id)).unwrap_or_default();
                            let mut next_heights = Vec::with_capacity(shown.len());
                            for (i, block) in shown.iter().enumerate() {
                                let cached_h = prev_heights.get(i).copied().unwrap_or(0.0);
                                let origin = ui.cursor().min;
                                let row_w = ui.available_width().max(1.0);
                                if reserve_offscreen_chat_row(ui, cached_h) {
                                    // `push_id` still consumes one parent auto-id
                                    // (`advance_cursor_after_rect`). A culled row must
                                    // burn that same slot or the next painted row renumbers.
                                    ui.skip_ahead_auto_ids(1);
                                    next_heights.push(cached_h);
                                    if jump_you && last_you_i == Some(i) {
                                        let slot = egui::Rect::from_min_size(
                                            origin,
                                            egui::vec2(row_w, cached_h),
                                        );
                                        ui.scroll_to_rect(slot, Some(egui::Align::Center));
                                        jumped_you = true;
                                    }
                                    if find_jump && find_row == Some(i) {
                                        let slot = egui::Rect::from_min_size(
                                            origin,
                                            egui::vec2(row_w, cached_h),
                                        );
                                        ui.scroll_to_rect(slot, Some(egui::Align::Center));
                                        find_jumped = true;
                                    }
                                    continue;
                                }
                                // Neighbour folds only matter for a row that paints.
                                let is_thought = |j: usize| {
                                    shown.get(j).is_some_and(|v| v.kind == ChatKind::Thought)
                                };
                                let prev_expanded = i
                                    .checked_sub(1)
                                    .filter(|&p| is_thought(p))
                                    .is_some_and(|p| fold_at(p).paints_body());
                                let next_fold = Some(i + 1).filter(|&n| is_thought(n)).map(fold_at);
                                let next_expanded = next_fold.is_some_and(|f| f.paints_body());
                                let next_drawn_thought = next_fold.is_some_and(|f| f.paints_row());
                                let fold = if block.kind == ChatKind::Thought {
                                    fold_at(i)
                                } else {
                                    ThoughtFold::Expanded
                                };
                                let y0 = ui.cursor().min.y;
                                // Copy / Reply sit under the last reply before the next ask.
                                let reply_acts = block.kind != ChatKind::Assistant
                                    || !shown[i + 1..]
                                        .iter()
                                        .take_while(|v| v.kind != ChatKind::User)
                                        .any(|v| v.kind == ChatKind::Assistant);
                                let privacy_here = (privacy_at == Some(i) && !privacy_grants.is_empty())
                                    || (skill_at == Some(i) && !skill_rows.is_empty());
                                let painted = ui
                                    .push_id(chat_row_id_salt(&thread_id, i), |ui| {
                                        let mut p = paint_chat_block_with(
                                            ui,
                                            block,
                                            thought_shows_label(prev_expanded),
                                            thought_shows_acts(next_expanded),
                                            fold,
                                            reply_acts && !privacy_here,
                                        );
                                        if privacy_here {
                                            if skill_at == Some(i) {
                                                skill_hit =
                                                    super::skill_undo::paint_skill_undo_rows(ui, &skill_rows);
                                            } else {
                                                privacy_revoke = super::privacy_ui::paint_privacy_revokes(
                                                    ui,
                                                    &privacy_grants,
                                                );
                                            }
                                            if reply_acts {
                                                let avail = clamp_row_width(
                                                    ui.available_width().min(ui.max_rect().width()),
                                                );
                                                let w = crate::markdown::bubble_width(avail);
                                                p.act = paint_msg_acts(ui, false, &block.body, avail, w).act;
                                            }
                                        }
                                        p
                                    });
                                if jump_you && last_you_i == Some(i) {
                                    ui.scroll_to_rect(painted.response.rect, Some(egui::Align::Center));
                                    jumped_you = true;
                                }
                                if find_hits.contains(&i) {
                                    paint_find_mark(ui, painted.response.rect, find_row == Some(i));
                                    if find_jump && find_row == Some(i) {
                                        ui.scroll_to_rect(
                                            painted.response.rect,
                                            Some(egui::Align::Center),
                                        );
                                        find_jumped = true;
                                    }
                                }
                                let painted = painted.inner;
                                if block.kind == ChatKind::Thought {
                                    note_thought_fold_click(
                                        ui.ctx(),
                                        &thread_id,
                                        None,
                                        keys[i],
                                        fold,
                                        painted.thought_fold,
                                        &mut collapse_session,
                                    );
                                }
                                match painted.act {
                                    ChatBlockAct::None => {}
                                    other => act = other,
                                }
                                if painted.drawn {
                                    ui.add_space(cluster_gap(
                                        block.kind == ChatKind::Thought,
                                        next_drawn_thought,
                                    ));
                                }
                                next_heights.push((ui.cursor().min.y - y0).max(0.0));
                            }
                            ui.ctx().data_mut(|d| d.insert_temp(row_h_id, next_heights));
                        }
                        if let Some(id) = privacy_revoke {
                            self.revoke_from_privacy(&id);
                        }
                        if let Some(row) = skill_hit {
                            self.skill_row_clicked(&row);
                        }
                        if jumped_you {
                            self.jump_last_you = false;
                        }
                        if find_jumped {
                            self.find.jump = false;
                        }
                        if live {
                            match self.paint_live_blocks(
                                ui,
                                thinking,
                                start_collapsed,
                                &mut collapse_session,
                            ) {
                                ChatBlockAct::None => {}
                                other => act = other,
                            }
                            if stretch_saved_skill(
                                self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
                            ) {
                                ui.label(
                                    RichText::new(SKILL_SAVED_NOTE)
                                        .size(12.0)
                                        .color(crate::theme::muted()),
                                );
                            }
                        } else if self.thinking_here() {
                            // A finished turn keeps its tools inline in the transcript.
                            self.paint_tool_cards(ui);
                        }
                        if collapse_session {
                            let stored = self.cached_chat_views().to_vec();
                            minimize_session_thoughts(
                                ui.ctx(),
                                &fold_thread,
                                &stored,
                                &self.live_blocks,
                            );
                        }
                        if let Some(code) = crate::markdown::take_code_copy(ui.ctx()) {
                            act = ChatBlockAct::Copy(code);
                        }
                        match act {
                            ChatBlockAct::Copy(body) => {
                                ui.ctx().copy_text(body);
                            }
                            ChatBlockAct::Reply(body) => {
                                self.composer =
                                    append_composer(&self.composer, &quote_for_reply(&body));
                                if !self.composer.ends_with('\n') {
                                    self.composer.push('\n');
                                }
                                self.composer_want_focus = true;
                            }
                            ChatBlockAct::Edit(body) => {
                                self.composer = edit_into_composer(&self.composer, &body);
                                self.composer_want_focus = true;
                                self.status = "Edit and send to ask again".into();
                            }
                            ChatBlockAct::None => {}
                        }
                        if self.chrome_here() {
                            self.paint_approval_stack(ui);
                        }
                        self.paint_try_again(ui);
                        if pin_tail {
                            // The transcript is laid out now, so the bottom is a real place.
                            ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                        }
                    });
                self.chat_tail_frames = self.chat_tail_frames.saturating_sub(1);
                if self.find.open {
                    self.paint_find_bar(&ctx, out.inner_rect);
                }
                if scrolled_off_tail(
                    out.state.offset.y,
                    out.content_size.y,
                    out.inner_rect.height(),
                    CHAT_TAIL_SLACK,
                ) {
                    match self.jump_to_latest(&ctx, out.inner_rect) {
                        ChatJump::Latest => self.pin_chat_tail(),
                        ChatJump::LastYou => self.jump_last_you = true,
                        ChatJump::None => {}
                    }
                }
            });
    }

    /// Floating find box at the top right of the transcript.
    pub(super) fn paint_find_bar(&mut self, ctx: &egui::Context, pane: egui::Rect) {
        let w = 340.0_f32.min((pane.width() - 16.0).max(200.0));
        let pos = egui::pos2(pane.max.x - w - 8.0, pane.min.y + 6.0);
        let label = self.find.label();
        let has_hits = !self.find.hits().is_empty();
        let mut step: Option<bool> = None;
        let mut close = false;
        egui::Area::new(egui::Id::new("chat-find"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(crate::theme::panel())
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .corner_radius(crate::theme::MENU_RADIUS)
                    .shadow(crate::theme::sheet_shadow())
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.set_width(w - 16.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let edit = egui::TextEdit::singleline(&mut self.find.query)
                                .id(egui::Id::new("chat-find-input"))
                                .hint_text(
                                    RichText::new("Find in this chat").color(crate::theme::muted()),
                                )
                                .desired_width((w - 16.0 - 170.0).max(80.0))
                                .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2)));
                            let resp = ui.add(edit);
                            if self.find.want_focus {
                                resp.request_focus();
                                self.find.want_focus = false;
                            }
                            let (enter, shift, esc) = ui.input(|i| {
                                (
                                    i.key_pressed(egui::Key::Enter),
                                    i.modifiers.shift,
                                    i.key_pressed(egui::Key::Escape),
                                )
                            });
                            let owns_keys = resp.has_focus() || resp.lost_focus();
                            if owns_keys && enter {
                                step = Some(!shift);
                                resp.request_focus();
                            }
                            if owns_keys && esc {
                                close = true;
                            }
                            self.find.focused = resp.has_focus();
                            ui.label(
                                RichText::new(&label)
                                    .size(crate::theme::FONT_TIP)
                                    .color(crate::theme::muted()),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let x = crate::theme::felt_label_button(
                                        ui,
                                        "×",
                                        egui::Color32::TRANSPARENT,
                                        crate::theme::muted(),
                                        6.0,
                                        egui::vec2(22.0, 22.0),
                                        None,
                                        false,
                                    )
                                    .on_hover_text("Close (Esc)");
                                    if x.clicked() {
                                        close = true;
                                    }
                                    let ink = if has_hits {
                                        crate::theme::fg()
                                    } else {
                                        crate::theme::subtle()
                                    };
                                    let down = crate::icons::paint_bar_icon(
                                        ui,
                                        crate::icons::BarIcon::ArrowDown,
                                        22.0,
                                        ink,
                                    )
                                    .on_hover_text("Next (Enter)");
                                    if down.clicked() && has_hits {
                                        step = Some(true);
                                    }
                                    let up = crate::icons::paint_bar_icon(
                                        ui,
                                        crate::icons::BarIcon::ArrowUp,
                                        22.0,
                                        ink,
                                    )
                                    .on_hover_text("Previous (Shift+Enter)");
                                    if up.clicked() && has_hits {
                                        step = Some(false);
                                    }
                                },
                            );
                        });
                    });
            });
        if let Some(forward) = step {
            self.find.step(forward);
        }
        if close {
            self.find.close();
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        }
    }

    /// One down-arrow: click = latest. Last you is secondary / long-press only.
    pub(super) fn jump_to_latest(&self, ctx: &egui::Context, pane: egui::Rect) -> ChatJump {
        const HIT: f32 = 32.0;
        let size = egui::vec2(HIT, HIT);
        let pos = egui::pos2(
            pane.center().x - size.x * 0.5,
            (pane.max.y - size.y - 10.0).max(pane.min.y),
        );
        let has_last_you = last_user_text(&self.messages).is_some();
        egui::Area::new(egui::Id::new("chat-jump"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                let resp = crate::icons::paint_bar_icon(
                    ui,
                    crate::icons::BarIcon::ArrowDown,
                    28.0,
                    crate::theme::fg(),
                );
                let tip = if has_last_you {
                    "Jump to latest. Right-click or hold for Last you."
                } else {
                    "Jump to latest"
                };
                let resp = resp.on_hover_text(tip);
                let hold_id = egui::Id::new("chat-jump-hold");
                let used_id = egui::Id::new("chat-jump-used");
                if has_last_you && resp.is_pointer_button_down_on() {
                    let now = ui.input(|i| i.time);
                    let start = ui.ctx().data(|d| d.get_temp::<f64>(hold_id)).unwrap_or(now);
                    ui.ctx().data_mut(|d| d.insert_temp(hold_id, start));
                    if now - start >= 0.45 {
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(used_id, true);
                            d.remove::<f64>(hold_id);
                        });
                        return ChatJump::LastYou;
                    }
                } else {
                    ui.ctx().data_mut(|d| d.remove::<f64>(hold_id));
                }
                if has_last_you
                    && (resp.secondary_clicked()
                        || (resp.clicked() && ui.input(|i| i.modifiers.alt)))
                {
                    ui.ctx().data_mut(|d| d.remove::<bool>(used_id));
                    return ChatJump::LastYou;
                }
                if resp.clicked() {
                    let used = ui.ctx().data(|d| d.get_temp::<bool>(used_id)).unwrap_or(false);
                    if used {
                        ui.ctx().data_mut(|d| d.insert_temp(used_id, false));
                        return ChatJump::None;
                    }
                    return ChatJump::Latest;
                }
                ChatJump::None
            })
            .inner
    }

    /// Open a chat on its newest message, not wherever the last one was scrolled to.
    pub(super) fn pin_chat_tail(&mut self) {
        self.chat_tail_frames = CHAT_TAIL_FRAMES;
    }

    /// Sending from the composer follows your own message down. A night job or a phone
    /// task calls `send_scheduled_chat` directly, so it cannot yank the pane out of your reading.
    /// Only the user's own typing comes through here, so this is also where a typed
    /// `/skills undo` is marked as theirs (`typed_send`).
    pub(super) fn send_from_composer(&mut self, text: String) {
        self.pin_chat_tail();
        self.heartbeat_user_sent();
        self.harness.typed_send = true;
        self.send_chat(text);
        self.harness.typed_send = false;
    }

    pub(super) fn paint_live_blocks(
        &mut self,
        ui: &mut egui::Ui,
        _thinking: bool,
        start_collapsed: bool,
        collapse_session: &mut bool,
    ) -> ChatBlockAct {
        let mut act = ChatBlockAct::None;
        let thread_id = self.visible_thread_id();
        let n = self.live_blocks.len();
        self.live_keys.resize(n, (0, usize::MAX, 0));
        // A long agent turn holds hundreds of blocks. Like stored rows, one scrolled
        // out of the pane reserves its last height instead of re-laying its markdown.
        let row_h_id = live_row_height_id(&thread_id, ui.available_width());
        let prev_heights: Vec<(u64, u8, f32)> =
            ui.ctx().data(|d| d.get_temp(row_h_id)).unwrap_or_default();
        let mut next_heights = vec![(0_u64, 0_u8, 0.0_f32); n];
        let mut skip_to = 0;
        for (i, b) in self.live_blocks.iter().enumerate() {
            if i < skip_to {
                continue;
            }
            // Back-to-back calls share one row, so a busy turn is not a wall of tool names.
            let run = if b.kind == LiveKind::Tool {
                self.live_blocks[i..]
                    .iter()
                    .take_while(|x| x.kind == LiveKind::Tool)
                    .count()
            } else {
                1
            };
            skip_to = i + run;
            let this_thought = b.kind == LiveKind::Thought;
            let prev_thought = i
                .checked_sub(1)
                .and_then(|p| self.live_blocks.get(p))
                .is_some_and(|v| v.kind == LiveKind::Thought);
            let next_thought = self
                .live_blocks
                .get(i + 1)
                .is_some_and(|v| v.kind == LiveKind::Thought);
            let prev_expanded = prev_thought
                && resolve_thought_fold(
                    ui.ctx(),
                    thought_fold_id(&thread_id, "slot", self.live_blocks[i - 1].fold_slot),
                    start_collapsed,
                )
                .paints_body();
            let next_expanded = next_thought
                && resolve_thought_fold(
                    ui.ctx(),
                    thought_fold_id(&thread_id, "slot", self.live_blocks[i + 1].fold_slot),
                    start_collapsed,
                )
                .paints_body();
            let next_drawn_thought = next_thought
                && resolve_thought_fold(
                    ui.ctx(),
                    thought_fold_id(&thread_id, "slot", self.live_blocks[i + 1].fold_slot),
                    start_collapsed,
                )
                .paints_row();
            let slot_id = thought_fold_id(&thread_id, "slot", b.fold_slot);
            let (fold, body_key) = if this_thought {
                // Only the thought still streaming grows. Rekey a block when it does.
                let cached = &mut self.live_keys[i];
                if cached.0 != b.fold_slot || cached.1 != b.body.len() {
                    *cached = (b.fold_slot, b.body.len(), thought_body_key(&b.body));
                }
                let body_id = thought_fold_id(&thread_id, "body", cached.2);
                let stored = ui.ctx().data(|d| {
                    d.get_temp::<ThoughtFold>(slot_id)
                        .or_else(|| d.get_temp(body_id))
                });
                (
                    grokhub_core::effective_thought_fold(stored, start_collapsed),
                    cached.2,
                )
            } else {
                (ThoughtFold::Expanded, 0)
            };
            let body_id = thought_fold_id(&thread_id, "body", body_key);
            // Copy / Reply sit under the last reply only.
            let last_say = b.kind == LiveKind::Say
                && !self.live_blocks[i + 1..]
                    .iter()
                    .any(|x| x.kind == LiveKind::Say);
            let ident = live_row_ident(b);
            let shape = live_row_shape(
                fold,
                prev_expanded,
                next_expanded,
                next_drawn_thought,
                last_say,
            );
            // The row still streaming changes height every delta; always paint it.
            let growing = skip_to >= n;
            let cached_h = prev_heights
                .get(i)
                .filter(|(id, sh, _)| *id == ident && *sh == shape && !growing)
                .map_or(0.0, |(_, _, h)| *h);
            if reserve_offscreen_chat_row(ui, cached_h) {
                // Same auto-id slot `push_id` below would consume.
                ui.skip_ahead_auto_ids(1);
                next_heights[i] = (ident, shape, cached_h);
                continue;
            }
            let y0 = ui.cursor().min.y;
            let tool_cards = &self.tool_cards;
            let blocks = &self.live_blocks;
            let row_act = ui
                .push_id(("cabin-live-row", ident), |ui| {
                    let mut act = ChatBlockAct::None;
                    let mut drawn = true;
                    match b.kind {
                        LiveKind::Thought => {
                            let view = ChatView {
                                kind: ChatKind::Thought,
                                title: "Thought".into(),
                                body: b.body.clone(),
                            };
                            let painted = paint_chat_block(
                                ui,
                                &view,
                                thought_shows_label(prev_expanded),
                                thought_shows_acts(next_expanded),
                                fold,
                            );
                            note_thought_fold_click(
                                ui.ctx(),
                                &thread_id,
                                Some(b.fold_slot),
                                body_key,
                                fold,
                                painted.thought_fold,
                                collapse_session,
                            );
                            // A growing live thought keeps an explicit fold on the current body key.
                            if let Some(explicit) =
                                ui.ctx().data(|d| d.get_temp::<ThoughtFold>(slot_id))
                            {
                                write_thought_fold(ui.ctx(), body_id, explicit);
                            }
                            drawn = painted.drawn;
                            act = painted.act;
                        }
                        LiveKind::Say => {
                            let view = ChatView {
                                kind: ChatKind::Assistant,
                                title: String::new(),
                                body: b.body.clone(),
                            };
                            act = paint_chat_block_with(
                                ui,
                                &view,
                                false,
                                false,
                                ThoughtFold::Expanded,
                                last_say,
                            )
                            .act;
                        }
                        LiveKind::Tool => {
                            // Borrow each card. A clone per frame copied its diff and screenshot.
                            let cards: Vec<std::borrow::Cow<'_, ToolCard>> = blocks[i..i + run]
                                .iter()
                                .map(|b| match tool_cards.iter().find(|c| c.id == b.tool_id) {
                                    Some(card) => std::borrow::Cow::Borrowed(card),
                                    None => std::borrow::Cow::Owned(ToolCard {
                                        id: b.tool_id.clone(),
                                        title: b.tool_title.clone(),
                                        kind: String::new(),
                                        status: b.tool_status.clone(),
                                        detail: b.tool_detail.clone(),
                                        diff: String::new(),
                                        image_data_url: None,
                                        raw_input: String::new(),
                                    }),
                                })
                                .collect();
                            paint_tool_group(ui, &cards);
                        }
                    }
                    if drawn {
                        ui.add_space(cluster_gap(this_thought, next_drawn_thought));
                    }
                    act
                })
                .inner;
            match row_act {
                ChatBlockAct::None => {}
                other => act = other,
            }
            next_heights[i] = (ident, shape, (ui.cursor().min.y - y0).max(0.0));
        }
        ui.ctx().data_mut(|d| d.insert_temp(row_h_id, next_heights));
        act
    }

    pub(super) fn paint_tool_cards(&self, ui: &mut egui::Ui) {
        if self.tool_cards.is_empty() {
            return;
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new(
            RichText::new("Work")
                .size(crate::theme::FONT_META)
                .color(crate::theme::muted()),
        )
        .id_salt("chat-work-tree")
        .default_open(false)
        .show(ui, |ui| {
            for card in &self.tool_cards {
                paint_tool_card_body(ui, card);
                ui.add_space(6.0);
            }
            let cards: Vec<&ToolCard> = self.tool_cards.iter().collect();
            super::harness_ui::paint_click_marker(ui, &cards);
        });
    }
    /// One needs-attention line, then the hard card, Grant full, the permission ask, and elicit.
    /// Always the left-aligned chat-column stack: the empty chat lays the composer out
    /// centered and justified, and cards must not inherit that (SY-01).
    pub(super) fn paint_approval_stack(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            let n = self.decisions_waiting();
            if n > 0 {
                let line = crate::motion::needs_attention_summary(n);
                ui.add(
                    egui::Label::new(RichText::new(line).size(12.0).color(crate::theme::muted()))
                        .wrap(),
                );
            }
            self.paint_harness_cards(ui);
            self.paint_perm_ask(ui);
            self.paint_elicit_ask(ui);
        });
    }

    pub(super) fn paint_perm_ask(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.perm_ask.clone() else {
            self.perm_always_confirm = None;
            return;
        };
        if !always_confirm_matches_rpc(self.perm_always_confirm.as_ref(), &p.rpc_id) {
            self.perm_always_confirm = None;
        }
        ui.add_space(8.0);
        let ask_id = egui::Id::new(("perm-ask-motion", p.rpc_id.as_str()));
        let enter_t = crate::motion::approval_enter_t(ui, ask_id, true);
        let y = crate::motion::approval_y(enter_t, false);
        let avail = ui.available_rect_before_wrap();
        let slot = avail.translate(egui::vec2(0.0, y));
        let eyebrow = super::harness_ui::perm_card_eyebrow(&p);
        ui.scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| {
            ui.multiply_opacity(enter_t.clamp(0.0, 1.0));
            let column = ui.available_width();
            let outer = super::harness_ui::approval_card_width(column);
            let inner = super::harness_ui::approval_card_inner(column, 1.0);
            let card_slot = egui::Rect::from_min_size(slot.min, egui::vec2(outer, slot.height()));
            let hover_t =
                crate::motion::approval_hover_t(ui, ask_id, ui.rect_contains_pointer(card_slot));
            let fill = crate::theme::blend_color(
                egui::Color32::TRANSPARENT,
                crate::motion::HOVER_BG,
                hover_t,
            );
            let framed = egui::Frame::NONE
                .fill(fill)
                .corner_radius(crate::theme::CHROME_RADIUS)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                ui.set_min_width(inner);
                ui.set_max_width(inner);
                ui.label(
                    RichText::new(eyebrow)
                        .size(12.0)
                        .color(crate::theme::muted()),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Grok wants permission")
                        .size(14.0)
                        .color(crate::theme::fg()),
                );
                // A parsed command paints even when it is `[ -f … ]` or `{ …; }`.
                // Only a leftover title dump is hidden.
                let action = if p.action.trim().is_empty() {
                    let title = p.title.trim();
                    if title.starts_with('{') || title.starts_with('[') {
                        ""
                    } else {
                        title
                    }
                } else {
                    p.action.trim()
                };
                if !action.is_empty() {
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(action)
                                .size(13.0)
                                .monospace()
                                .color(crate::theme::fg()),
                        )
                        .wrap(),
                    );
                }
                if !p.reason.trim().is_empty() && p.reason.trim() != action {
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&p.reason)
                                .size(13.0)
                                .color(crate::theme::muted()),
                        )
                        .wrap(),
                    );
                }
                if !self.perm_queue.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("{} more waiting", self.perm_queue.len()))
                            .size(12.0)
                            .color(crate::theme::muted()),
                    );
                }
                ui.add_space(8.0);
                let overlay = self.palette_open || self.nav == Nav::Settings || self.find.focused;
                let key = perm_key(
                    bare_press(ui, egui::Key::Enter),
                    bare_press(ui, egui::Key::Escape),
                    !self.composer.trim().is_empty(),
                    overlay || overlay_over_chat(ui.ctx()),
                );
                // One press answers one card, not the next one queued behind it.
                match key {
                    Some(PermKey::Allow) => {
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                    }
                    Some(PermKey::Deny) => {
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
                    }
                    None => false,
                };
                let mut answered = false;
                ui.horizontal(|ui| {
                    if crate::cards::white_pill(ui, "Allow") || key == Some(PermKey::Allow) {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission(p.rpc_id.clone(), true);
                        }
                        answered = true;
                    } else if crate::cards::ghost_pill(ui, "Deny") || key == Some(PermKey::Deny) {
                        // Grok's own reject option: "denied", not "User cancelled".
                        if let Some(h) = &self.acp {
                            let _ = h.reject_permission(&p);
                        }
                        answered = true;
                    }
                    if !answered
                        && self.perm_always_confirm.is_none()
                        && crate::cards::ghost_pill(ui, "Always")
                    {
                        self.perm_always_confirm = Some(p.rpc_id.clone());
                    }
                });
                if answered {
                    self.next_perm_ask();
                    return;
                }
                if always_confirm_matches_rpc(self.perm_always_confirm.as_ref(), &p.rpc_id) {
                    ui.add_space(8.0);
                    if let Some(act) =
                        paint_confirm_sheet(ui, always_session_spec(), ALWAYS_CONFIRM_LINE2)
                    {
                        match act {
                            ConfirmAct::Confirm => {
                                self.set_permission_mode(PermissionMode::AlwaysApprove);
                                // Always covers the asks already waiting, too.
                                let queued: Vec<_> = self.perm_queue.drain(..).collect();
                                if let Some(h) = &self.acp {
                                    let _ = h.answer_permission_always(p.rpc_id.clone());
                                    for q in queued {
                                        let _ = h.answer_permission_always(q.rpc_id);
                                    }
                                }
                                self.perm_ask = None;
                                self.perm_always_confirm = None;
                                self.status = "Permission always-approve".into();
                            }
                            ConfirmAct::Cancel => {
                                self.perm_always_confirm = None;
                            }
                        }
                    }
                }
            });
            let time = ui.ctx().input(|i| i.time) as f32;
            crate::motion::paint_thinking_rim(
                ui.painter(),
                framed.response.rect,
                self.running,
                time,
            );
            // Primary one-shot breath envelope (Allow row); composer_breath while running.
            let _primary = crate::motion::one_shot_breath(
                ui.ctx().input(|i| i.time),
                ui.ctx().input(|i| i.time) - f64::from(crate::motion::APPROVAL_ENTER_SECS),
                crate::motion::reduced_motion(ui),
            );
            let _live = crate::icons::composer_breath(time);
        });
    }

    pub(super) fn paint_elicit_ask(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.elicit_ask.clone() else {
            return;
        };
        ui.add_space(8.0);
        let column = ui.available_width();
        let inner = super::harness_ui::approval_card_inner(column, 1.0);
        egui::Frame::NONE
            .fill(egui::Color32::TRANSPARENT)
            .corner_radius(crate::theme::CHROME_RADIUS)
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(inner);
                ui.set_max_width(inner);
                ui.label(
                    RichText::new(format!("{} wants input", p.server_name))
                        .size(14.0)
                        .color(crate::theme::fg()),
                );
                if !p.message.trim().is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(&p.message)
                            .size(13.0)
                            .color(crate::theme::muted()),
                    );
                }
                if p.mode == "url" && !p.url.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(&p.url)
                            .size(12.0)
                            .color(crate::theme::muted()),
                    );
                }
                if p.field_name.is_some() {
                    ui.add_space(6.0);
                    let hint = if p.field_title.is_empty() {
                        "Value"
                    } else {
                        p.field_title.as_str()
                    };
                    let mut edit = egui::TextEdit::singleline(&mut self.elicit_draft)
                        .hint_text(crate::theme::hint(hint))
                        .desired_width(ui.available_width());
                    if p.secret {
                        edit = edit.password(true);
                    }
                    ui.add(edit);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let accept_label = if p.mode == "url" { "Open" } else { "Accept" };
                    if crate::cards::white_pill(ui, accept_label) {
                        if p.mode == "url" && !p.url.is_empty() {
                            let _ = crate::desktop::open_url(&p.url);
                        }
                        if p.secret {
                            let held = self.elicit_draft.clone();
                            self.hold_secret(&held);
                        }
                        let content = p
                            .field_name
                            .as_ref()
                            .map(|name| serde_json::json!({ name: self.elicit_draft.trim() }));
                        if let Some(h) = &self.acp {
                            let _ = h.answer_elicit(p.rpc_id.clone(), "accept", content);
                        }
                        self.elicit_ask = None;
                        self.elicit_draft.clear();
                    }
                    if crate::cards::ghost_pill(ui, "Decline") {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_elicit(p.rpc_id.clone(), "decline", None);
                        }
                        self.elicit_ask = None;
                        self.elicit_draft.clear();
                    }
                    if crate::cards::ghost_pill(ui, "Cancel") {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_elicit(p.rpc_id.clone(), "cancel", None);
                        }
                        self.elicit_ask = None;
                        self.elicit_draft.clear();
                    }
                });
            });
    }

    pub(super) fn paint_try_again(&mut self, ui: &mut egui::Ui) {
        if !self.try_again || self.running {
            return;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Credit limit")
                    .size(13.0)
                    .color(crate::theme::muted()),
            );
            if crate::cards::white_pill(ui, "Try Again") {
                self.run_slash(Slash::Retry);
            }
        });
    }

    pub(super) fn ui_empty_home(&mut self, ui: &mut egui::Ui) {
        let greet_on = should_paint_greeting(self.messages.is_empty(), self.scratch())
            && !self.greeting.is_empty();
        let pulse_on = self.pulse_should_paint();
        let avail = ui.available_rect_before_wrap();
        let pane_w =
            crate::cards::composer_pill_w(ui.ctx().content_rect().width()).min(avail.width());
        let side = empty_home_side_gap(avail.width(), pane_w);
        let composer_top = empty_home_composer_top(avail.height(), crate::theme::QUERY_MIN_H);
        let mark_h = if greet_on { 40.0 + 12.0 } else { 0.0 };
        let greet_h = if greet_on {
            greeting_galley_h(ui, &self.greeting, pane_w) + mark_h
        } else {
            0.0
        };
        // The deck moved to Pulse (2.10.91). Settings → Behavior can bring it back.
        let feed_n = if pulse_on && self.cfg.home_deck {
            home_feed_count(&self.updates, &self.cfg.feed_pulse, now_ms())
        } else {
            0
        };
        let feed_h = collapsed_stack_h(feed_n);
        let device_on = pulse_on && device_glance(self.hub_on, self.last_frame_url.as_deref()).is_some();
        let device_h = if device_on { 26.0 } else { 0.0 };
        let block_h = greet_h;
        let greet_top = empty_home_greet_top(composer_top, block_h, 12.0);
        if greet_on || pulse_on {
            let greet_rect = egui::Rect::from_min_size(
                egui::pos2(avail.left() + side, avail.top() + greet_top),
                egui::vec2(pane_w, block_h.max(1.0)),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(greet_rect), |ui| {
                ui.set_min_size(greet_rect.size());
                ui.with_layout(
                    egui::Layout::top_down_justified(egui::Align::Center),
                    |ui| {
                        ui.set_width(pane_w);
                        if greet_on {
                            let mark = crate::theme::mark(ui.ctx());
                            ui.add(
                                egui::Image::from_texture(&mark)
                                    .fit_to_exact_size(egui::vec2(40.0, 40.0)),
                            );
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new(&self.greeting)
                                    .font(crate::theme::title_font(crate::theme::GREET_HERO))
                                    .color(crate::theme::muted()),
                            );
                        }
                    },
                );
            });
        }
        let composer_h = (avail.height() - composer_top).max(crate::theme::QUERY_MIN_H + 96.0);
        let rect = egui::Rect::from_min_size(
            egui::pos2(avail.left() + side, avail.top() + composer_top),
            egui::vec2(pane_w, composer_h),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_min_size(egui::vec2(pane_w, crate::theme::QUERY_MIN_H + 96.0));
            ui.with_layout(
                egui::Layout::top_down_justified(egui::Align::Center),
                |ui| {
                    ui.set_width(pane_w);
                    self.ui_composer_stack(ui);
                    if self.chrome_here() {
                        self.paint_approval_stack(ui);
                    }
                    self.paint_try_again(ui);
                    if pulse_on && (feed_n > 0 || device_on) {
                        let below = feed_h + device_h + if feed_n > 0 && device_on { 4.0 } else { 0.0 };
                        let gap = ui.available_height();
                        ui.add_space(((gap - below) * 0.5).max(0.0));
                        if feed_n > 0 {
                            self.paint_update_feed(ui, pane_w);
                        }
                        if device_on {
                            if feed_n > 0 {
                                ui.add_space(4.0);
                            }
                            self.paint_device_glance_row(ui, pane_w);
                        }
                    }
                },
            );
        });
        self.paint_home_deck_over_chat(ui);
    }

    /// Export, view plan, and the recommended fork offer. Not slash-only.
    pub(super) fn paint_session_tools(&mut self, ui: &mut egui::Ui) {
        let plan = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.plan_body.clone())
            .unwrap_or_default();
        let (turns, estimate) = self.session_size();
        let tokens = if self.grok_usage.context_tokens_used > 0 {
            self.grok_usage.context_used().min(u64::from(u32::MAX)) as u32
        } else {
            estimate
        };
        let why = fork_offer_why(turns, tokens, CONTEXT_BUDGET_TOKENS);
        if plan.trim().is_empty() && why.is_none() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if !plan.trim().is_empty() {
                let label = if self.plan_open { "Hide plan" } else { "View plan" };
                if crate::cards::ghost_pill(ui, label) {
                    self.plan_open = !self.plan_open;
                }
            }
            if let Some(why) = why {
                ui.label(
                    RichText::new(why)
                        .size(crate::theme::FONT_META)
                        .color(crate::theme::muted()),
                );
                if crate::cards::ghost_pill(ui, "Fork") {
                    self.run_slash(Slash::Fork);
                }
            }
        });
        if why.is_some() && !self.fork_explainer_seen {
            ui.add_space(4.0);
            egui::Frame::NONE
                .fill(crate::theme::elevated())
                .corner_radius(12.0)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("How fork works")
                            .size(crate::theme::FONT_BODY)
                            .strong()
                            .color(crate::theme::fg()),
                    );
                    ui.label(
                        RichText::new(FORK_EXPLAINER)
                            .size(crate::theme::FONT_META)
                            .color(crate::theme::muted()),
                    );
                    if crate::cards::ghost_pill(ui, "Got it") {
                        self.dismiss_fork_explainer();
                    }
                });
        }
        if self.plan_open && !plan.trim().is_empty() {
            ui.add_space(4.0);
            egui::Frame::NONE
                .fill(crate::theme::elevated())
                .corner_radius(12.0)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Plan")
                            .size(crate::theme::FONT_BODY)
                            .strong()
                            .color(crate::theme::fg()),
                    );
                    ui.label(
                        RichText::new(plan)
                            .size(crate::theme::FONT_META)
                            .color(crate::theme::fg()),
                    );
                });
        }
    }

    /// Compact, Copy session, and Export. Titlebar, immediately left of minimize.
    pub(super) fn paint_session_actions_menu(&mut self, ui: &mut egui::Ui) -> egui::Rect {
        let resp = titlebar_chrome_btn(ui, ChromeBtn::Menu)
            .on_hover_text(crate::titlebar::titlebar_chrome_tip(ChromeBtn::Menu, false));
        let placed = resp.rect;
        let id = ui.make_persistent_id("session-actions-menu");
        if titlebar_chrome_hit(&resp) {
            egui::Popup::toggle_id(ui.ctx(), id);
        }
        let (compact_on, copy_on, export_on) =
            session_menu_enabled(!self.messages.is_empty(), self.grok_usage.is_empty());
        egui::Popup::new(id, ui.ctx().clone(), &resp, ui.layer_id())
            .open_memory(None)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .align(egui::RectAlign::BOTTOM_START)
            .align_alternatives(&[])
            .layout(egui::Layout::top_down_justified(egui::Align::LEFT))
            .show(|ui| {
                ui.set_min_width(176.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                if session_menu_row(ui, "Compact", compact_on) {
                    self.run_slash(Slash::Compact);
                }
                if session_menu_row(ui, "Copy session", copy_on) {
                    if let Some(t) = self.threads.get(self.thread_idx) {
                        ui.ctx().copy_text(crate::threads::export_markdown(t));
                        self.status = "Copied session".into();
                    }
                }
                if session_menu_row(ui, "Export", export_on) {
                    self.run_slash(Slash::Export);
                }
            });
        placed
    }

    pub(super) fn ui_composer_stack(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.vertical_centered_justified(|ui| {
            let col_w = crate::cards::composer_pill_w(ui.ctx().content_rect().width());
            ui.set_max_width(col_w);
            for slot in composer_stack_order() {
                match slot {
                    ComposerStackSlot::AuthBanner => {
                        let grok_missing = grokhub_acp::find_grok().is_none();
                        let need_login = grokhub_acp::grok_cli_key().is_none() && !self.has_key();
                        let cabin_oauth = self
                            .secrets
                            .oauth
                            .as_ref()
                            .is_some_and(|t| !t.access_token.trim().is_empty());
                        let get_started = grokhub_core::should_show_get_started_now(
                            !grok_missing,
                            cabin_oauth,
                            self.cfg.get_started_done,
                            grokhub_acp::grok_cli_key().is_some(),
                            self.official_cli_session,
                        );
                        if !get_started
                            && !self.grok_install_wait
                            && (grok_missing || need_login)
                        {
                            ui.horizontal(|ui| {
                                crate::cards::settings_note(
                                    ui,
                                    if grok_missing {
                                        "Install Grok Build (x.ai/cli), then grok login."
                                    } else {
                                        "Run grok login. Chat, Imagine, and Fast chips use that token."
                                    },
                                );
                                if crate::cards::ghost_pill(ui, "Settings") {
                                    self.nav = Nav::Settings;
                                }
                            });
                        }
                    }
                    ComposerStackSlot::ContextBar => {
                        if !self.grok_usage.is_empty() {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let used = self.grok_usage.context_used();
                                let window = self.grok_usage.context_window().max(1);
                                let frac = (used as f32 / window as f32).clamp(0.0, 1.0);
                                let line = grok_context_line(&self.grok_usage);
                                let bar_w = ui.available_width().max(48.0);
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(bar_w, 14.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(rect, 4.0, crate::theme::elevated());
                                let mut fill = rect;
                                fill.set_width((rect.width() * frac).max(2.0));
                                ui.painter().rect_filled(fill, 4.0, crate::theme::nav_active());
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    line,
                                    egui::FontId::proportional(11.0),
                                    crate::theme::muted(),
                                );
                            });
                            ui.add_space(4.0);
                        }
                    }
                    ComposerStackSlot::SessionTools => {
                        self.paint_session_tools(ui);
                    }
                    ComposerStackSlot::LiveWork => {
                        self.paint_live_work(ui);
                    }
                    ComposerStackSlot::SlashPalette => {
                        let hits = filter_slash_hits(&self.composer, &self.grok_commands);
                        if !hits.is_empty() {
                            let first = hits.first().map(|s| s.cmd.as_str()).unwrap_or("");
                            let n = hits.len();
                            let changed = self.slash_filter_n != n || self.slash_filter_first != first;
                            self.slash_pick = slash_pick_retain(self.slash_pick, changed, n);
                            self.slash_filter_n = n;
                            self.slash_filter_first = first.to_string();
                            ui.label(
                                RichText::new("↑↓  Tab accepts")
                                    .size(crate::theme::FONT_META)
                                    .color(crate::theme::subtle()),
                            );
                            ui.add_space(4.0);
                            egui::Frame::NONE
                                .fill(crate::theme::surface())
                                .corner_radius(crate::theme::CHROME_RADIUS)
                                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                                .inner_margin(egui::Margin::same(8))
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .max_height(148.0)
                                        .auto_shrink([false, true])
                                        .show(ui, |ui| {
                                            for (i, s) in hits.iter().enumerate() {
                                                let on = i == self.slash_pick;
                                                let row = format!("{}  {}", s.cmd, s.hint);
                                                let fill = if on {
                                                    crate::theme::nav_active()
                                                } else {
                                                    egui::Color32::TRANSPARENT
                                                };
                                                if crate::theme::felt_label_button(
                                                    ui,
                                                    &row,
                                                    fill,
                                                    if on {
                                                        crate::theme::fg()
                                                    } else {
                                                        crate::theme::muted()
                                                    },
                                                    8.0,
                                                    egui::vec2(ui.available_width(), 28.0),
                                                    None,
                                                    false,
                                                )
                                                .clicked()
                                                {
                                                    if let Some(t) = slash_pick_take(
                                                        &mut self.composer,
                                                        &s.insert,
                                                        s.run_on_pick,
                                                    ) {
                                                        self.send_chat(t);
                                                    }
                                                }
                                            }
                                        });
                                });
                            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                                self.slash_pick = slash_pick_step(self.slash_pick, hits.len(), 1);
                            } else if ui.input_mut(|i| {
                                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                            }) {
                                self.slash_pick = slash_pick_step(self.slash_pick, hits.len(), -1);
                            } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab))
                            {
                                let s = &hits[self.slash_pick.min(hits.len() - 1)];
                                if let Some(t) =
                                    slash_pick_take(&mut self.composer, &s.insert, s.run_on_pick)
                                {
                                    self.send_chat(t);
                                }
                            }
                            ui.add_space(6.0);
                        } else {
                            self.slash_pick = 0;
                            self.slash_filter_n = 0;
                            self.slash_filter_first.clear();
                        }
                    }
                    ComposerStackSlot::Chips => {
                        ui.add_space(6.0);
                        let chips = self.composer_chips();
                        if let Some(act) = crate::cards::quick_chip_row(ui, &chips) {
                            self.take_chip_act(act, &chips);
                        }
                    }
                    ComposerStackSlot::Attach => {
            self.ui_attach_chip(ui, PlusTarget::Chat);
                    }
                    ComposerStackSlot::Voice => {
            self.paint_voice_mode_row(ui);
                    }
                    ComposerStackSlot::Pill => {
            let pill_w = crate::cards::composer_pill_w(ui.ctx().content_rect().width());
            let cap = pill_w.min(ui.available_width()).max(360.0);
            ui.set_width(cap);
            ui.set_max_width(cap);
            let session_now = self.session_mode.as_str().to_string();
            let perm_now = self.permission_mode.as_str().to_string();
            let effort_now = self.cfg.reasoning_effort.clone();
            let row = crate::cards::session_row(ui, &session_now, &perm_now, &effort_now);
            if let Some(mode) = row.mode {
                if let Some(m) = SessionMode::parse(&mode) {
                    if m == SessionMode::Plan {
                        self.select_plan_without_rename();
                    } else if m == SessionMode::Ask && self.running {
                        self.set_session_mode(m);
                        self.status = "btw — side ask, main run continues".into();
                    } else {
                        self.confirm = None;
                        if self.running {
                            self.halt_in_flight();
                        }
                        self.set_session_mode(m);
                        self.acp = None;
                        self.acp_spawn_rx = None;
                        if let Some(t) = self.threads.get_mut(self.thread_idx) {
                            t.grok_session = None;
                        }
                        self.persist_idle_key = self.persist_idle_now();
                        self.status = if m == SessionMode::Ask {
                            "btw — look-safe side ask".into()
                        } else {
                            format!("Session {}", m.as_str())
                        };
                    }
                }
            }
            if let Some(perm) = row.perm {
                if let Some(p) = PermissionMode::parse(&perm) {
                    if p == PermissionMode::AlwaysApprove
                        && self.permission_mode != PermissionMode::AlwaysApprove
                    {
                        self.arm_session_always();
                    } else {
                    self.confirm = None;
                    if self.running {
                        self.halt_in_flight();
                    }
                    self.set_permission_mode(p);
                    self.acp = None;
                    self.acp_spawn_rx = None;
                    if let Some(t) = self.threads.get_mut(self.thread_idx) {
                        t.grok_session = None;
                    }
                    self.persist_idle_key = self.persist_idle_now();
                    self.status = format!("Permission {}", p.as_str());
                    }
                }
            }
            if let Some(effort) = row.effort {
                if let Some(e) = grokhub_core::parse_reasoning_effort(&effort) {
                    self.confirm = None;
                    if self.running {
                        self.halt_in_flight();
                    }
                    self.cfg.reasoning_effort = e.to_string();
                    self.acp = None;
                    self.acp_spawn_rx = None;
                    if let Some(t) = self.threads.get_mut(self.thread_idx) {
                        t.grok_session = None;
                    }
                    self.persist_cfg();
                    self.persist_idle_key = self.persist_idle_now();
                    self.status = format!("Effort {}", grokhub_core::effort_label(e));
                }
            }
            let composer_id = egui::Id::new("chat-composer");
            let focused = ui.memory(|m| m.has_focus(composer_id));
            let rows = (self.composer.matches('\n').count() + 1).clamp(1, 6);
            let pad_l = 20.0;
            let pad_r = 24.0;
            let pill_h = if rows == 1 {
                crate::theme::QUERY_MIN_H
            } else {
                (rows as f32 * 22.0 + 28.0).max(crate::theme::QUERY_MIN_H)
            };
            ui.allocate_ui_with_layout(
                egui::vec2(cap, pill_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_size(egui::vec2(cap, pill_h));
                    let pill_rect =
                        egui::Rect::from_min_size(ui.max_rect().min, egui::vec2(cap, pill_h));
                    #[cfg(feature = "fx")]
                    self.paint_composer_glow(ui, pill_rect);
                    ui.painter().rect(
                        pill_rect,
                        crate::theme::QUERY_RADIUS,
                        crate::theme::elevated(),
                        crate::theme::composer_chrome_stroke(focused),
                        egui::StrokeKind::Middle,
                    );
                    let inner = (cap - pad_l - pad_r).max(200.0);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(pad_l);
                    let plus = crate::icons::paint_bar_icon(
                        ui,
                        crate::icons::BarIcon::Plus,
                        22.0,
                        crate::theme::muted(),
                    )
                    .on_hover_text("Upload a file or paste clipboard");
                    if plus.clicked() {
                        self.open_plus(PlusTarget::Chat, plus.rect.left_bottom());
                    }
                    if self.composer_want_focus {
                        ui.memory_mut(|m| m.request_focus(composer_id));
                        self.composer_want_focus = false;
                    }
                    let focused = ui.memory(|m| m.has_focus(composer_id));
                    if let Some(t) = take_focused_composer(ui, &mut self.composer, focused) {
                        self.send_typed(ui, t);
                    }
                    ui.add_space(8.0);
                    let cluster = crate::cards::composer_go_cluster_w();
                    let go_sz = crate::cards::composer_go_hit_w();
                    let mid = crate::cards::composer_mid_w(inner);
                    let (mut mic_rect, mut go_rect) = (egui::Rect::NOTHING, egui::Rect::NOTHING);
                    ui.allocate_ui_with_layout(
                        egui::vec2(mid, pill_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let text_w = (mid - 8.0 - 22.0).max(80.0);
                            let edit = egui::ScrollArea::vertical()
                                .id_salt("chat-composer-scroll")
                                .max_width(text_w)
                                .max_height(pill_h)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.set_min_height(pill_h);
                                    ui.set_max_height(pill_h);
                                    let hint_align = if rows == 1 {
                                        egui::Align::Center
                                    } else {
                                        egui::Align::Min
                                    };
                                    ui.add(
                                        egui::TextEdit::multiline(&mut self.composer)
                                            .id(composer_id)
                                            .desired_width(text_w)
                                            .desired_rows(rows)
                                            .min_size(egui::vec2(
                                                text_w,
                                                if rows == 1 { pill_h } else { 0.0 },
                                            ))
                                            .frame(composer_field_frame())
                                            .vertical_align(hint_align)
                                            .font(egui::FontId::new(
                                                15.0,
                                                egui::FontFamily::Proportional,
                                            ))
                                            .text_color(crate::theme::fg())
                                            .hint_text("")
                                            .return_key(Some(egui::KeyboardShortcut::new(
                                                egui::Modifiers::COMMAND,
                                                egui::Key::Enter,
                                            ))),
                                    )
                                })
                                .inner;
                            if self.composer.is_empty() && !edit.has_focus() {
                                let ghost = composer_hint_ink();
                                let hint_font =
                                    egui::FontId::new(15.0, egui::FontFamily::Proportional);
                                let hint = ui.fonts_mut(|f| {
                                    f.layout_no_wrap("Ask anything".to_string(), hint_font, ghost)
                                });
                                let hint_pos = egui::pos2(
                                    edit.rect.left() + 2.0,
                                    edit.rect.center().y - hint.size().y * 0.5,
                                );
                                ui.painter().galley(hint_pos, hint, ghost);
                            }
                            if let Some(t) =
                                take_focused_composer(ui, &mut self.composer, edit.has_focus())
                            {
                                self.send_typed(ui, t);
                            }
                            mic_rect = self.paint_voice_mic(ui, 22.0);
                        },
                    );
                    ui.add_space(8.0);
                    let ready = !self.composer.trim().is_empty();
                    let go = composer_go(self.thinking_here(), ready);
                    ui.allocate_ui_with_layout(
                        egui::vec2(go_sz, pill_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let send = crate::icons::paint_bar_icon(
                                ui,
                                match go {
                                    ComposerGo::Stop => crate::icons::BarIcon::Stop,
                                    ComposerGo::Send => crate::icons::BarIcon::Send,
                                    ComposerGo::Idle => crate::icons::BarIcon::ArrowUp,
                                },
                                match go {
                                    ComposerGo::Idle => 22.0,
                                    ComposerGo::Send | ComposerGo::Stop => 28.0,
                                },
                                match go {
                                    ComposerGo::Idle => crate::theme::muted(),
                                    ComposerGo::Send | ComposerGo::Stop => crate::theme::fg(),
                                },
                            )
                            .on_hover_text(self.go_tip_here());
                            go_rect = send.rect;
                            let go_hit = send.clicked()
                                || (send.is_pointer_button_down_on()
                                    && ui.input(|i| i.pointer.primary_pressed()));
                            match go {
                                ComposerGo::Stop => {
                                    if go_hit {
                                        self.run_slash(Slash::Stop);
                                    }
                                }
                                ComposerGo::Send | ComposerGo::Idle => {
                                    if go_hit {
                                        let t = std::mem::take(&mut self.composer);
                                        self.send_from_composer(t);
                                    }
                                }
                            }
                        },
                    );
                    debug_assert!(
                        8.0 + 22.0 + 8.0 + go_sz <= cluster,
                        "mic + Send/Stop must fit the reserved cluster"
                    );
                    self.composer_geom = Some((pill_rect, mic_rect, go_rect));
                },
            );
                    }
                }
            }
            });
    }

    /// Visible turns and the token estimate behind the fork offer. Scanning the whole
    /// transcript every frame cost more than painting it, so key it like the chat views.
    /// `messages_rev` catches an in-place edit that keeps the count and the last length.
    pub(super) fn session_size(&mut self) -> (usize, u32) {
        let key = (
            self.visible_thread_id(),
            self.messages.len(),
            self.messages.last().map_or(0, |m| m.1.len()),
            self.messages_rev,
            self.messages_body_mark(),
        );
        let cached = &self.session_size.0;
        let tail_only = cached.0 == key.0 && cached.1 == key.1 && cached.4 == key.4;
        if tail_only && self.tail_streaming_here() {
            // Only the streaming reply grew. The hint catches up when the turn ends.
            return self.session_size.1;
        }
        if self.session_size.0 != key {
            let msgs = || self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str()));
            let size = (
                visible_turn_count_from(msgs()),
                estimate_messages_from(msgs()),
            );
            self.session_size = (key, size);
        }
        self.session_size.1
    }

    /// A turn is streaming on this tab: only its last message grows, and the pane
    /// paints it from live blocks. Transcript caches can hold until it stops.
    fn tail_streaming_here(&self) -> bool {
        self.thinking_here() && !self.live_blocks.is_empty()
    }

    pub(super) fn cached_chat_views(&mut self) -> &[ChatView] {
        let tid = self.visible_thread_id();
        let n = self.messages.len();
        let last = self.messages.last().map(|m| m.1.len()).unwrap_or(0);
        let rev = self.messages_rev;
        let body = self.messages_body_mark();
        if self.chat_view_tid == tid
            && self.chat_view_n == n
            && self.chat_view_last == last
            && self.chat_view_rev == rev
            && self.chat_view_body == body
        {
            return &self.chat_views;
        }
        // Only the last message changed (`live_tail_mut`): a stream delta. Any edit
        // before it, or a swapped transcript, rebuilds every view.
        let tail_only = self.chat_view_tid == tid
            && self.chat_view_n == n
            && self.chat_view_body == body
            && !self.chat_views.is_empty();
        if tail_only && self.tail_streaming_here() {
            // The pane paints this turn from live blocks and shows views only up to
            // the last ask. Decoding, scrubbing, and rekeying the whole turn on every
            // delta was thrown away. The stretch rebuilds once when the turn ends.
            return &self.chat_views;
        }
        let refs: Vec<(&str, &str)> = self
            .messages
            .iter()
            .map(|m| (m.0.as_str(), m.1.as_str()))
            .collect();
        let kept = if tail_only {
            refresh_last_stretch(&mut self.chat_views, &refs)
        } else {
            self.chat_views = visible_chat_refs(refs.iter().copied());
            0
        };
        // `thought_body_key` scrubs and hashes a whole thought. Key each view once here,
        // not every thought in the transcript on every frame.
        self.chat_view_keys.truncate(kept);
        let from = self.chat_view_keys.len();
        self.chat_view_keys.extend(self.chat_views[from..].iter().map(view_fold_key));
        self.chat_view_tid = tid;
        self.chat_view_n = n;
        self.chat_view_last = last;
        self.chat_view_rev = rev;
        self.chat_view_body = body;
        &self.chat_views
    }
}
