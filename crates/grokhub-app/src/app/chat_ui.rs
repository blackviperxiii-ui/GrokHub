//! Chat pane, composer, and bubbles.

use super::*;

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
    SlashPalette,
    Chips,
    Attach,
    Voice,
    Pill,
}

pub(super) fn composer_stack_order() -> &'static [ComposerStackSlot] {
    &[
        ComposerStackSlot::AuthBanner,
        ComposerStackSlot::ContextBar,
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
    let galley = ui.fonts(|f| {
        f.layout(
            text.to_string(),
            font,
            crate::theme::muted(),
            wrap_w.max(1.0),
        )
    });
    galley.size().y.max(crate::theme::GREET_HERO)
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

/// Enter sends. Control+Enter is left for TextEdit (`return_key`) to insert a newline.
pub(super) fn take_focused_composer(
    ui: &mut egui::Ui,
    composer: &mut String,
    focused: bool,
) -> Option<String> {
    if !focused {
        return None;
    }
    let (enter, control) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.modifiers.ctrl || i.modifiers.command,
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
}

pub(super) struct ChatBlockPaint {
    act: ChatBlockAct,
    drawn: bool,
    thought_fold: ThoughtFold,
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
    let content = crate::markdown::measure_text(ui, body, wrap);
    let inner_w = content.x.max(1.0).min(wrap);
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
    let frame = egui::Frame::none()
        .fill(if user {
            crate::theme::bubble_user()
        } else {
            crate::theme::bubble_assistant()
        })
        .rounding(crate::theme::USER_BUBBLE_RADIUS)
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

pub(super) fn paint_msg_acts(
    ui: &mut egui::Ui,
    user: bool,
    body: &str,
    avail: f32,
    align_w: f32,
) -> ChatBlockAct {
    let mut act = ChatBlockAct::None;
    let mut paint = |ui: &mut egui::Ui| {
        let copy = crate::theme::felt_label_button(
            ui,
            "Copy",
            egui::Color32::TRANSPARENT,
            crate::theme::muted(),
            6.0,
            egui::vec2(0.0, 0.0),
            None,
            false,
        );
        if copy.clicked() {
            act = ChatBlockAct::Copy(body.to_string());
        }
        let reply = crate::theme::felt_label_button(
            ui,
            "Reply",
            egui::Color32::TRANSPARENT,
            crate::theme::muted(),
            6.0,
            egui::vec2(0.0, 0.0),
            None,
            false,
        );
        if reply.clicked() {
            act = ChatBlockAct::Reply(body.to_string());
        }
    };
    ui.scope(|ui| {
        ui.set_max_width(avail);
        ui.horizontal(|ui| {
            ui.set_max_width(avail);
            if user {
                ui.add_space((avail - align_w.max(96.0)).max(0.0));
            }
            paint(ui);
        });
    });
    act
}

pub(super) fn paint_thought_bubble(ui: &mut egui::Ui, body: &str) -> egui::Response {
    let body = crate::markdown::display_text(body);
    let avail = clamp_row_width(ui.available_width().min(ui.max_rect().width()));
    let wrap = (avail - 8.0).max(1.0);
    let frame = egui::Frame::none()
        .fill(egui::Color32::TRANSPARENT)
        .inner_margin(egui::Margin::symmetric(0.0, 2.0));
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

/// `kind` is `"slot"` while the thought is a live block, `"body"` once it is stored.
/// The body key is [`thought_body_key`], so a collapse survives that handoff.
pub(super) fn thought_fold_id(thread_id: &str, kind: &str, key: u64) -> egui::Id {
    egui::Id::new(("cabin-thought-fold", thread_id, kind, key))
}

pub(super) fn read_thought_fold(ctx: &egui::Context, id: egui::Id) -> ThoughtFold {
    ctx.data(|d| d.get_temp(id)).unwrap_or_default()
}

pub(super) fn write_thought_fold(ctx: &egui::Context, id: egui::Id, fold: ThoughtFold) {
    ctx.data_mut(|d| d.insert_temp(id, fold));
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
    let galley = ui.fonts(|f| f.layout_no_wrap(label.to_owned(), font, color));
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
                act: paint_msg_acts(ui, true, &block.body, avail, resp.rect.width()),
                drawn: true,
                thought_fold,
            }
        }
        ChatKind::Assistant => {
            let resp = paint_speech_bubble(ui, &block.body, false, true);
            ChatBlockPaint {
                act: paint_msg_acts(ui, false, &block.body, avail, resp.rect.width()),
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
                    act = paint_msg_acts(ui, false, &block.body, avail, resp.rect.width());
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
            egui::CollapsingHeader::new(
                RichText::new(title)
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            )
            .id_salt(("chat-tool", title, block.body.as_str()))
            .default_open(false)
            .show(ui, |ui| {
                ui.set_max_width(bubble_w);
                ui.label(
                    RichText::new(&block.body)
                        .size(crate::theme::FONT_META)
                        .color(crate::theme::subtle()),
                );
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
    pub(super) fn ui_chat(&mut self, ctx: &egui::Context) {
        let empty = self.messages.is_empty();
        if !empty {
            egui::TopBottomPanel::bottom("composer")
                .frame(
                    egui::Frame::none()
                        .fill(crate::theme::bg())
                        .inner_margin(egui::Margin {
                            left: 32.0,
                            right: 32.0,
                            top: 10.0,
                            bottom: 22.0,
                        }),
                )
                .show(ctx, |ui| {
                    self.ui_composer_stack(ui);
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin {
                        left: 8.0,
                        right: 16.0,
                        top: 12.0,
                        bottom: 8.0,
                    }),
            )
            .show(ctx, |ui| {
                if empty {
                    self.ui_empty_home(ui);
                    return;
                }
                let avail = clamp_row_width(ui.available_width());
                let pane = avail;
                let pin_tail = self.chat_tail_frames > 0;
                let out = egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_width(pane);
                        ui.set_max_width(pane);
                        let thinking = self.thinking_here();
                        let live = !self.live_blocks.is_empty();
                        let mut act = ChatBlockAct::None;
                        let jump_you = self.jump_last_you;
                        let mut jumped_you = false;
                        {
                            let thread_id = self.visible_thread_id();
                            let row_h_id = chat_row_height_id(&thread_id, pane);
                            let views = self.cached_chat_views();
                            let shown = if live {
                                views_up_to_last_user(views)
                            } else {
                                views
                            };
                            let last_you_i = shown.iter().rposition(|v| v.kind == ChatKind::User);
                            let prev_heights: Vec<f32> =
                                ui.ctx().data(|d| d.get_temp(row_h_id)).unwrap_or_default();
                            let mut next_heights = Vec::with_capacity(shown.len());
                            for (i, block) in shown.iter().enumerate() {
                                let prev_thought = i
                                    .checked_sub(1)
                                    .and_then(|p| shown.get(p))
                                    .filter(|v| v.kind == ChatKind::Thought);
                                let next_thought =
                                    shown.get(i + 1).filter(|v| v.kind == ChatKind::Thought);
                                let prev_expanded = prev_thought.is_some_and(|v| {
                                    read_thought_fold(
                                        ui.ctx(),
                                        thought_fold_id(
                                            &thread_id,
                                            "body",
                                            thought_body_key(&v.body),
                                        ),
                                    )
                                    .paints_body()
                                });
                                let next_expanded = next_thought.is_some_and(|v| {
                                    read_thought_fold(
                                        ui.ctx(),
                                        thought_fold_id(
                                            &thread_id,
                                            "body",
                                            thought_body_key(&v.body),
                                        ),
                                    )
                                    .paints_body()
                                });
                                let next_drawn_thought = next_thought.is_some_and(|v| {
                                    read_thought_fold(
                                        ui.ctx(),
                                        thought_fold_id(
                                            &thread_id,
                                            "body",
                                            thought_body_key(&v.body),
                                        ),
                                    )
                                    .paints_row()
                                });
                                let fold = if block.kind == ChatKind::Thought {
                                    read_thought_fold(
                                        ui.ctx(),
                                        thought_fold_id(
                                            &thread_id,
                                            "body",
                                            thought_body_key(&block.body),
                                        ),
                                    )
                                } else {
                                    ThoughtFold::Expanded
                                };
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
                                    continue;
                                }
                                let y0 = ui.cursor().min.y;
                                let painted = ui
                                    .push_id(chat_row_id_salt(&thread_id, i), |ui| {
                                        paint_chat_block(
                                            ui,
                                            block,
                                            thought_shows_label(prev_expanded),
                                            thought_shows_acts(next_expanded),
                                            fold,
                                        )
                                    });
                                if jump_you && last_you_i == Some(i) {
                                    ui.scroll_to_rect(painted.response.rect, Some(egui::Align::Center));
                                    jumped_you = true;
                                }
                                let painted = painted.inner;
                                if block.kind == ChatKind::Thought {
                                    write_thought_fold(
                                        ui.ctx(),
                                        thought_fold_id(
                                            &thread_id,
                                            "body",
                                            thought_body_key(&block.body),
                                        ),
                                        painted.thought_fold,
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
                        if jumped_you {
                            self.jump_last_you = false;
                        }
                        if live {
                            match self.paint_live_blocks(ui, thinking) {
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
                        } else {
                            self.paint_tool_cards(ui);
                        }
                        if thinking {
                            let phase = self.run_phase_here();
                            paint_running(
                                ui,
                                chat_run_label(phase),
                                &chat_run_hint(phase, &self.run_action_here()),
                            );
                        }
                        match act {
                            ChatBlockAct::Copy(body) => {
                                ui.ctx().copy_text(body);
                                self.status = "Copied".into();
                            }
                            ChatBlockAct::Reply(body) => {
                                self.composer =
                                    append_composer(&self.composer, &quote_for_reply(&body));
                                if !self.composer.ends_with('\n') {
                                    self.composer.push('\n');
                                }
                                self.composer_want_focus = true;
                            }
                            ChatBlockAct::None => {}
                        }
                        self.paint_perm_ask(ui);
                        self.paint_elicit_ask(ui);
                        self.paint_try_again(ui);
                        if pin_tail {
                            // The transcript is laid out now, so the bottom is a real place.
                            ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                        }
                    });
                self.chat_tail_frames = self.chat_tail_frames.saturating_sub(1);
                if scrolled_off_tail(
                    out.state.offset.y,
                    out.content_size.y,
                    out.inner_rect.height(),
                    CHAT_TAIL_SLACK,
                ) {
                    match self.jump_to_latest(ctx, out.inner_rect) {
                        ChatJump::Latest => self.pin_chat_tail(),
                        ChatJump::LastYou => self.jump_last_you = true,
                        ChatJump::None => {}
                    }
                }
            });
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
    pub(super) fn send_from_composer(&mut self, text: String) {
        self.pin_chat_tail();
        self.send_chat(text);
    }

    pub(super) fn paint_live_blocks(&self, ui: &mut egui::Ui, _thinking: bool) -> ChatBlockAct {
        let mut act = ChatBlockAct::None;
        let thread_id = self.visible_thread_id();
        for (i, b) in self.live_blocks.iter().enumerate() {
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
                && read_thought_fold(
                    ui.ctx(),
                    thought_fold_id(&thread_id, "slot", self.live_blocks[i - 1].fold_slot),
                )
                .paints_body();
            let next_expanded = next_thought
                && read_thought_fold(
                    ui.ctx(),
                    thought_fold_id(&thread_id, "slot", self.live_blocks[i + 1].fold_slot),
                )
                .paints_body();
            let next_drawn_thought = next_thought
                && read_thought_fold(
                    ui.ctx(),
                    thought_fold_id(&thread_id, "slot", self.live_blocks[i + 1].fold_slot),
                )
                .paints_row();
            let mut drawn = true;
            match b.kind {
                LiveKind::Thought => {
                    let view = ChatView {
                        kind: ChatKind::Thought,
                        title: "Thought".into(),
                        body: b.body.clone(),
                    };
                    let slot_id = thought_fold_id(&thread_id, "slot", b.fold_slot);
                    let fold = read_thought_fold(ui.ctx(), slot_id);
                    let painted = paint_chat_block(
                        ui,
                        &view,
                        thought_shows_label(prev_expanded),
                        thought_shows_acts(next_expanded),
                        fold,
                    );
                    write_thought_fold(ui.ctx(), slot_id, painted.thought_fold);
                    // Same text key the stored transcript reads after live_blocks is cleared.
                    write_thought_fold(
                        ui.ctx(),
                        thought_fold_id(&thread_id, "body", thought_body_key(&b.body)),
                        painted.thought_fold,
                    );
                    drawn = painted.drawn;
                    match painted.act {
                        ChatBlockAct::None => {}
                        other => act = other,
                    }
                }
                LiveKind::Say => {
                    let view = ChatView {
                        kind: ChatKind::Assistant,
                        title: String::new(),
                        body: b.body.clone(),
                    };
                    match paint_chat_block(ui, &view, false, false, ThoughtFold::Expanded).act {
                        ChatBlockAct::None => {}
                        other => act = other,
                    }
                }
                LiveKind::Tool => {
                    let card = self
                        .tool_cards
                        .iter()
                        .find(|c| c.id == b.tool_id)
                        .cloned()
                        .unwrap_or_else(|| ToolCard {
                            id: b.tool_id.clone(),
                            title: b.tool_title.clone(),
                            kind: String::new(),
                            status: b.tool_status.clone(),
                            detail: b.tool_detail.clone(),
                            diff: String::new(),
                            image_data_url: None,
                        });
                    paint_one_tool_card(ui, &card);
                }
            }
            if drawn {
                ui.add_space(cluster_gap(this_thought, next_drawn_thought));
            }
        }
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
        egui::Frame::none()
            .fill(egui::Color32::TRANSPARENT)
            .rounding(crate::theme::CHROME_RADIUS)
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
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
                    ui.label(RichText::new(action).size(13.0).color(crate::theme::fg()));
                }
                if !p.reason.trim().is_empty() && p.reason.trim() != action {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(&p.reason)
                            .size(13.0)
                            .color(crate::theme::muted()),
                    );
                }
                ui.add_space(8.0);
                let key = perm_key(
                    ui.input(|i| i.key_pressed(egui::Key::Enter)),
                    ui.input(|i| i.key_pressed(egui::Key::Escape)),
                    !self.composer.trim().is_empty(),
                    self.palette_open || self.nav == Nav::Settings,
                );
                ui.horizontal(|ui| {
                    if crate::cards::white_pill(ui, "Allow") || key == Some(PermKey::Allow) {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission(p.rpc_id.clone(), true);
                        }
                        self.perm_ask = None;
                        self.perm_always_confirm = None;
                    }
                    if crate::cards::ghost_pill(ui, "Deny") || key == Some(PermKey::Deny) {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission(p.rpc_id.clone(), false);
                        }
                        self.perm_ask = None;
                        self.perm_always_confirm = None;
                    }
                    if self.perm_always_confirm.is_none() && crate::cards::ghost_pill(ui, "Always") {
                        self.perm_always_confirm = Some(p.rpc_id.clone());
                    }
                });
                if always_confirm_matches_rpc(self.perm_always_confirm.as_ref(), &p.rpc_id) {
                    ui.add_space(8.0);
                    if let Some(act) =
                        paint_confirm_sheet(ui, always_session_spec(), ALWAYS_CONFIRM_LINE2)
                    {
                        match act {
                            ConfirmAct::Confirm => {
                                self.set_permission_mode(PermissionMode::AlwaysApprove);
                                if let Some(h) = &self.acp {
                                    let _ = h.answer_permission_always(p.rpc_id.clone());
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
    }

    pub(super) fn paint_elicit_ask(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.elicit_ask.clone() else {
            return;
        };
        ui.add_space(8.0);
        egui::Frame::none()
            .fill(egui::Color32::TRANSPARENT)
            .rounding(crate::theme::CHROME_RADIUS)
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
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
                        .hint_text(hint)
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
                            #[cfg(windows)]
                            let _ = std::process::Command::new("cmd")
                                .args(["/C", "start", "", &p.url])
                                .spawn();
                            #[cfg(not(windows))]
                            let _ = std::process::Command::new("xdg-open").arg(&p.url).spawn();
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
            crate::cards::composer_pill_w(ui.ctx().screen_rect().width()).min(avail.width());
        let side = empty_home_side_gap(avail.width(), pane_w);
        let composer_top = empty_home_composer_top(avail.height(), crate::theme::QUERY_MIN_H);
        let mark_h = if greet_on { 40.0 + 12.0 } else { 0.0 };
        let greet_h = if greet_on {
            greeting_galley_h(ui, &self.greeting, pane_w) + mark_h
        } else {
            0.0
        };
        let pulse_h = if pulse_on {
            pulse_card_h(self.collect_pulse_rows().len())
        } else {
            0.0
        };
        let lane_h = if pulse_on { 26.0 } else { 0.0 };
        let device_on = pulse_on && device_glance(self.hub_on, self.last_frame_url.as_deref()).is_some();
        let device_h = if device_on { 26.0 } else { 0.0 };
        let pulse_gap = if pulse_on && greet_on { 8.0 } else { 0.0 };
        let block_h = greet_h + pulse_gap + pulse_h + lane_h + device_h;
        let greet_top = empty_home_greet_top(composer_top, block_h, 12.0);
        if greet_on || pulse_on {
            let greet_rect = egui::Rect::from_min_size(
                egui::pos2(avail.left() + side, avail.top() + greet_top),
                egui::vec2(pane_w, block_h.max(1.0)),
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(greet_rect), |ui| {
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
                        if pulse_on {
                            if greet_on {
                                ui.add_space(8.0);
                            }
                            self.paint_lane_chip(ui);
                            ui.add_space(4.0);
                            self.paint_empty_pulse(ui, pane_w);
                            if device_on {
                                ui.add_space(4.0);
                                self.paint_device_glance_row(ui, pane_w);
                            }
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
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_min_size(egui::vec2(pane_w, crate::theme::QUERY_MIN_H + 96.0));
            ui.with_layout(
                egui::Layout::top_down_justified(egui::Align::Center),
                |ui| {
                    ui.set_width(pane_w);
                    self.ui_composer_stack(ui);
                    self.paint_perm_ask(ui);
                    self.paint_elicit_ask(ui);
                    self.paint_try_again(ui);
                },
            );
        });
    }

    pub(super) fn ui_composer_stack(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.vertical_centered_justified(|ui| {
            let col_w = crate::cards::composer_pill_w(ui.ctx().screen_rect().width());
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
                            let used = self.grok_usage.context_used();
                            let window = self.grok_usage.context_window().max(1);
                            let frac = (used as f32 / window as f32).clamp(0.0, 1.0);
                            let line = grok_context_line(&self.grok_usage);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 14.0),
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
                            ui.add_space(4.0);
                        }
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
                            egui::Frame::none()
                                .fill(crate::theme::surface())
                                .rounding(crate::theme::CHROME_RADIUS)
                                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                                .inner_margin(egui::Margin::same(8.0))
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
            if self.messages.is_empty() {
            ui.add_space(6.0);
            let chips = self.composer_chips(true);
            if let Some(act) = crate::cards::quick_chip_row(ui, &chips) {
                self.take_chip_act(act, &chips);
            }
            } else {
                let chips = self.composer_chips(false);
                if !chips.is_empty() {
                    ui.add_space(6.0);
                    if let Some(act) = crate::cards::quick_chip_row(ui, &chips) {
                        self.take_chip_act(act, &chips);
                    }
                }
            }
                    }
                    ComposerStackSlot::Attach => {
            self.ui_attach_chip(ui, PlusTarget::Chat);
                    }
                    ComposerStackSlot::Voice => {
            self.paint_voice_mode_row(ui);
                    }
                    ComposerStackSlot::Pill => {
            let pill_w = crate::cards::composer_pill_w(ui.ctx().screen_rect().width());
            let cap = pill_w.min(ui.available_width()).max(360.0);
            ui.set_width(cap);
            ui.set_max_width(cap);
            let session_now = self.session_mode.as_str().to_string();
            let perm_now = self.permission_mode.as_str().to_string();
            let effort_now = self.cfg.reasoning_effort.clone();
            let row = crate::cards::session_row(ui, &session_now, &perm_now, &effort_now);
            if let Some(mode) = row.mode {
                if let Some(m) = SessionMode::parse(&mode) {
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
                    self.status = format!("Session {}", m.as_str());
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
            ui.allocate_ui_with_layout(
                egui::vec2(cap, crate::theme::QUERY_MIN_H),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(cap);
                    ui.set_max_width(cap);
            egui::Frame::none()
                .fill(crate::theme::elevated())
                .rounding(crate::theme::QUERY_RADIUS)
                .stroke(crate::theme::composer_chrome_stroke(focused))
                .inner_margin(egui::Margin::same(7.0))
                .show(ui, |ui| {
                    let inner = (cap - 14.0).max(200.0);
                    ui.set_width(inner);
                    ui.set_max_width(inner);
                    ui.set_min_height(crate::theme::QUERY_MIN_H - 16.0);
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.horizontal(|ui| {
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
                        if let Some(t) =
                            take_focused_composer(ui, &mut self.composer, focused)
                        {
                            self.send_from_composer(t);
                        }
                        let cluster = crate::cards::composer_go_cluster_w();
                        let go_sz = crate::cards::composer_go_hit_w();
                        let mid = crate::cards::composer_mid_w(inner);
                        let rows = (self.composer.matches('\n').count() + 1).min(8);
                        let bar_h = crate::theme::QUERY_MIN_H - 16.0;
                        ui.allocate_ui_with_layout(
                            egui::vec2(mid, bar_h),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let text_w = (ui.available_width() - cluster + go_sz).max(80.0);
                                let edit = ui.add(
                                    egui::TextEdit::multiline(&mut self.composer)
                                        .id(composer_id)
                                        .desired_width(text_w)
                                        .desired_rows(rows)
                                        .frame(false)
                                        .hint_text("Ask anything")
                                        .return_key(Some(egui::KeyboardShortcut::new(
                                            egui::Modifiers::COMMAND,
                                            egui::Key::Enter,
                                        ))),
                                );
                                if let Some(t) = take_focused_composer(
                                    ui,
                                    &mut self.composer,
                                    edit.has_focus(),
                                ) {
                                    self.send_from_composer(t);
                                }
                                self.paint_voice_mic(ui, 22.0);
                            },
                        );
                        let ready = !self.composer.trim().is_empty();
                        let go = composer_go(self.running, ready);
                        ui.allocate_ui_with_layout(
                            egui::vec2(go_sz, bar_h),
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
                                        ComposerGo::Send | ComposerGo::Stop => {
                                            crate::theme::fg()
                                        }
                                    },
                                )
                                .on_hover_text(composer_go_tip(self.running));
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
                    });
                });
                },
            );
                    }
                }
            }
            });
    }

    pub(super) fn cached_chat_views(&mut self) -> &[ChatView] {
        let tid = self.visible_thread_id();
        let n = self.messages.len();
        let last = self.messages.last().map(|m| m.1.len()).unwrap_or(0);
        if self.chat_view_tid == tid && self.chat_view_n == n && self.chat_view_last == last {
            return &self.chat_views;
        }
        let refs: Vec<(&str, &str)> = self
            .messages
            .iter()
            .map(|m| (m.0.as_str(), m.1.as_str()))
            .collect();
        if self.chat_view_tid == tid && self.chat_view_n == n && !self.chat_views.is_empty() {
            refresh_last_stretch(&mut self.chat_views, &refs);
        } else {
            self.chat_views = visible_chat_refs(refs.iter().copied());
        }
        self.chat_view_tid = tid;
        self.chat_view_n = n;
        self.chat_view_last = last;
        &self.chat_views
    }
}
