//! The compact card row (S2): status icon, title, and elapsed timer on row 1;
//! a one-line description, the chat's model, and presence dots on row 2. No
//! border until hover; the fill eases from surface to hover over 120 ms.

use super::tokens::{card_tokens, CardTokens, CARD_RADIUS, COMPACT_H, HOVER_FILL_SECS};
use eframe::egui;
use grokhub_core::{BoardCard, BoardStatus};

const PAD_X: f32 = 12.0;
const PAD_Y: f32 = 10.0;
const GAP: f32 = 8.0;
const ICON: f32 = 16.0;
/// Presence chips shrunk to dots, and how far each one overlaps the last.
const DOT: f32 = 14.0;
const DOT_STEP: f32 = DOT - 4.0;
const TITLE_SIZE: f32 = 14.0;
const DESC_SIZE: f32 = 12.0;
const TIMER_SIZE: f32 = 12.0;
const MODEL_SIZE: f32 = 11.0;
const MODEL_ALPHA: f32 = 0.7;
/// A card being dragged leaves its slot this faint until the drop.
const DRAGGED_SLOT_ALPHA: f32 = 0.45;
/// Longest description kept before the painter truncates to the row width.
pub(in crate::app) const DESC_MAX_CHARS: usize = 140;

/// The 16 px glyph at the start of row 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum StatusIcon {
    Spinner,
    Dot,
    Check,
    Alert,
    Retry,
}

pub(in crate::app) fn status_icon(status: BoardStatus) -> StatusIcon {
    match status {
        BoardStatus::InProgress => StatusIcon::Spinner,
        BoardStatus::Done => StatusIcon::Check,
        BoardStatus::Blocked => StatusIcon::Alert,
        BoardStatus::FollowUp => StatusIcon::Retry,
        BoardStatus::Proposed
        | BoardStatus::Approved
        | BoardStatus::Staged
        | BoardStatus::Todo
        | BoardStatus::Dismissed => StatusIcon::Dot,
    }
}

/// `mm:ss` under an hour, then `1h 4m`, then whole days (`3d`).
pub(in crate::app) fn elapsed_label(elapsed_ms: u64) -> String {
    let secs = elapsed_ms / 1000;
    if secs < 3600 {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    } else if secs < 86_400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// How soon the timer label next changes, so the board only wakes for it.
fn elapsed_tick(elapsed_ms: u64) -> Option<std::time::Duration> {
    if elapsed_ms < 3_600_000 {
        Some(std::time::Duration::from_secs(1))
    } else if elapsed_ms < 86_400_000 {
        Some(std::time::Duration::from_secs(60))
    } else {
        None
    }
}

/// The first non-blank line, inner whitespace collapsed, capped at
/// `DESC_MAX_CHARS` with an ellipsis.
pub(in crate::app) fn one_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let flat = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= DESC_MAX_CHARS {
        return flat;
    }
    let mut cut: String = flat.chars().take(DESC_MAX_CHARS - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// Row 2's text: the card's detail, or a Follow up card's latest report.
pub(in crate::app) fn card_desc(card: &BoardCard) -> String {
    let detail = one_line(&card.detail);
    if detail.is_empty() {
        one_line(&card.report)
    } else {
        detail
    }
}

/// What one compact card shows this frame.
pub(in crate::app) struct CompactRow<'a> {
    pub card: &'a BoardCard,
    /// The model the card's chat runs on; `None` when it has no chat.
    pub model: Option<&'a str>,
    /// The card's chat has a turn running now: the spinner turns.
    pub live: bool,
    /// This card is the one being dragged; its slot fades until the drop.
    pub dragging: bool,
    pub now_ms: u64,
    /// Chat / notes / automation chips, drawn as small dots.
    pub chips: &'a [(char, egui::Color32, egui::Color32)],
}

/// Paints one compact card at full row width and `COMPACT_H` tall. It burns
/// exactly one parent auto id (`allocate_space`), so an off-screen skip must
/// burn one too. The returned response clicks and drags under
/// `("board-card", id)`.
pub(in crate::app) fn paint_compact_card(ui: &mut egui::Ui, row: &CompactRow) -> egui::Response {
    let card = row.card;
    let t = card_tokens(ui.visuals().dark_mode);
    let width = ui.available_width().max(1.0);
    let (_, rect) = ui.allocate_space(egui::vec2(width, COMPACT_H));
    let resp = ui.interact(
        rect,
        egui::Id::new(("board-card", card.id.as_str())),
        egui::Sense::click_and_drag(),
    );
    let label = format!(
        "{}, {}",
        card.title,
        crate::app::board_ui::board_status_label(card.status)
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let reduced = crate::motion::reduced_motion(ui);
    let hovered = resp.hovered() && !row.dragging;
    let hover_t = if reduced {
        if hovered {
            1.0
        } else {
            0.0
        }
    } else {
        ui.ctx()
            .animate_bool_with_time(resp.id.with("hover"), hovered, HOVER_FILL_SECS)
    };
    let mut painter = ui.painter_at(rect);
    if row.dragging {
        painter.multiply_opacity(DRAGGED_SLOT_ALPHA);
    }
    painter.rect_filled(rect, CARD_RADIUS, lerp_color(t.surface, t.hover, hover_t));
    if hover_t > 0.0 {
        painter.rect_stroke(
            rect.shrink(0.5),
            CARD_RADIUS,
            egui::Stroke::new(1.0, t.border.gamma_multiply(hover_t)),
            egui::StrokeKind::Inside,
        );
    }

    let inner = rect.shrink2(egui::vec2(PAD_X, PAD_Y));
    let row1_y = inner.min.y + ICON * 0.5;
    let row2_y = inner.max.y - DESC_SIZE * 0.5 - 1.0;
    let text_x = inner.min.x + ICON + GAP;

    // Row 1: icon · title · timer.
    paint_status_icon(
        ui,
        &painter,
        egui::pos2(inner.min.x + ICON * 0.5, row1_y),
        status_icon(card.status),
        &t,
        row.live && !reduced,
    );
    let mut title_max_x = inner.max.x;
    if card.updated_ms > 0 {
        let elapsed = row.now_ms.saturating_sub(card.updated_ms);
        let timer = painter.layout_no_wrap(
            elapsed_label(elapsed),
            egui::FontId::monospace(TIMER_SIZE),
            t.meta,
        );
        let pos = egui::pos2(inner.max.x - timer.size().x, row1_y - timer.size().y * 0.5);
        title_max_x = pos.x - GAP;
        painter.galley(pos, timer, t.meta);
        if let Some(after) = elapsed_tick(elapsed) {
            ui.ctx().request_repaint_after(after);
        }
    }
    let fresh_room = if card.fresh { GAP + 6.0 } else { 0.0 };
    let title = one_row(
        &painter,
        &card.title,
        crate::theme::title_font(TITLE_SIZE),
        t.title,
        (title_max_x - text_x - fresh_room).max(0.0),
    );
    let title_w = title.size().x;
    painter.galley(
        egui::pos2(text_x, row1_y - title.size().y * 0.5),
        title,
        t.title,
    );
    if card.fresh {
        // Unread report or reply: a small live dot after the title.
        painter.circle_filled(egui::pos2(text_x + title_w + GAP, row1_y), 3.0, t.live);
    }

    // Row 2: description · model · presence dots.
    let mut desc_max_x = inner.max.x;
    if !row.chips.is_empty() {
        let w = DOT + DOT_STEP * (row.chips.len() - 1) as f32;
        let x0 = inner.max.x - w;
        for (i, &(ch, bg, fg)) in row.chips.iter().enumerate() {
            let c = egui::pos2(x0 + DOT * 0.5 + DOT_STEP * i as f32, row2_y);
            painter.circle_filled(c, DOT * 0.5, bg);
            painter.circle_stroke(c, DOT * 0.5, egui::Stroke::new(1.0, t.surface));
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                ch.to_string(),
                egui::FontId::proportional(8.5),
                fg,
            );
        }
        desc_max_x = x0 - GAP;
    }
    if let Some(model) = row.model.filter(|m| !m.is_empty()) {
        let ink = t.meta.gamma_multiply(MODEL_ALPHA);
        let g = painter.layout_no_wrap(
            model.to_string(),
            egui::FontId::proportional(MODEL_SIZE),
            ink,
        );
        // The description keeps at least a third of the row; the model gives way.
        if desc_max_x - g.size().x - GAP - text_x >= (inner.width() / 3.0) {
            let pos = egui::pos2(desc_max_x - g.size().x, row2_y - g.size().y * 0.5);
            desc_max_x = pos.x - GAP;
            painter.galley(pos, g, ink);
        }
    }
    let desc = card_desc(card);
    if !desc.is_empty() {
        let g = one_row(
            &painter,
            &desc,
            egui::FontId::proportional(DESC_SIZE),
            t.desc,
            (desc_max_x - text_x).max(0.0),
        );
        painter.galley(egui::pos2(text_x, row2_y - g.size().y * 0.5), g, t.desc);
    }
    resp
}

/// One line of text, cut with an ellipsis at `max_w`.
fn one_row(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(font, color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
    painter.layout_job(job)
}

fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    egui::Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn paint_status_icon(
    ui: &egui::Ui,
    painter: &egui::Painter,
    c: egui::Pos2,
    icon: StatusIcon,
    t: &CardTokens,
    turning: bool,
) {
    use std::f32::consts::{FRAC_PI_2, TAU};
    let r = ICON * 0.5 - 1.5;
    match icon {
        StatusIcon::Spinner => {
            // Turns only while the card's chat has a turn running; a parked
            // In progress card shows the arc still.
            let spin = if turning {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(33));
                (ui.input(|i| i.time) as f32 * TAU * 0.8) % TAU
            } else {
                0.0
            };
            let a0 = -FRAC_PI_2 + spin;
            paint_arc(
                painter,
                c,
                r - 0.5,
                a0,
                a0 + TAU * 0.75,
                egui::Stroke::new(1.6, t.live),
            );
        }
        StatusIcon::Dot => {
            painter.circle_filled(c, 3.5, t.meta);
        }
        StatusIcon::Check => {
            painter.circle_stroke(c, r, egui::Stroke::new(1.4, t.live));
            let ink = egui::Stroke::new(1.6, t.live);
            painter.line_segment([c + egui::vec2(-3.0, 0.2), c + egui::vec2(-0.8, 2.4)], ink);
            painter.line_segment([c + egui::vec2(-0.8, 2.4), c + egui::vec2(3.2, -2.2)], ink);
        }
        StatusIcon::Alert => {
            painter.circle_stroke(c, r, egui::Stroke::new(1.4, t.danger));
            let ink = egui::Stroke::new(1.6, t.danger);
            painter.line_segment([c + egui::vec2(0.0, -3.4), c + egui::vec2(0.0, 0.8)], ink);
            painter.circle_filled(c + egui::vec2(0.0, 3.0), 0.95, t.danger);
        }
        StatusIcon::Retry => {
            // ↺: an open ring with its arrowhead at the top end.
            let a0 = -FRAC_PI_2 + 0.5;
            let a1 = a0 + TAU * 0.8;
            paint_arc(painter, c, r - 0.5, a0, a1, egui::Stroke::new(1.5, t.warn));
            let tip = c + egui::vec2(a0.cos(), a0.sin()) * (r - 0.5);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    tip + egui::vec2(-0.6, -3.0),
                    tip + egui::vec2(2.6, 0.2),
                    tip + egui::vec2(-1.6, 1.6),
                ],
                t.warn,
                egui::Stroke::NONE,
            ));
        }
    }
}

fn paint_arc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    a0: f32,
    a1: f32,
    stroke: egui::Stroke,
) {
    let steps = (((a1 - a0).abs() * radius).ceil() as i32).clamp(6, 48);
    let points = (0..=steps)
        .map(|i| {
            let a = a0 + (a1 - a0) * (i as f32 / steps as f32);
            center + egui::vec2(a.cos(), a.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(points, stroke));
}
