//! Pulse: one page, two views. Ideas are rows under category headers (an icon,
//! a bold "I can …" line, a short description, a ··· menu). Feed is a column of
//! posts (source, headline, summary, the source's own images, actions).
//! `grokhub_core::pulse` picks and orders cards by rules; a model only writes
//! sentences. Every choice lands in MEMORY.md as one ledger line.

use std::collections::{HashMap, HashSet};
use std::io::Read as _;

use super::*;
use grokhub_core::pulse::{
    self as pc, LedgerEntry, LedgerReason, PulseCategory, PulseType, Ranked,
};
use grokhub_core::{CardReaction, UpdateAction, UpdateCard, UpdateKind};

const ICON: f32 = 32.0;
/// The header's action slot: Feed instructions on Feed, Suggest ideas on
/// Ideas when the list has rows or a suggestion is loading. An empty Ideas
/// list already paints Suggest in the body, so the header slot stays blank.
/// One width, so the Feed | Ideas switch never moves.
pub(super) const HEADER_SLOT_W: f32 = 150.0;
/// Behind a hovered or focused row.
const ROW_HOVER: egui::Color32 = egui::Color32::from_rgb(0x0f, 0x10, 0x12);
/// Keyboard focus: a 2px ring.
pub(super) const FOCUS_RING: egui::Color32 = egui::Color32::from_rgb(0x4a, 0x90, 0xe2);
/// The sign-in line under Suggest ideas.
const NOTE_AMBER: egui::Color32 = egui::Color32::from_rgb(0xf4, 0xb7, 0x40);
const COL_MAX_W: f32 = 680.0;
const THUMB_H: f32 = 96.0;
const THUMB_MAX: usize = 3;
/// A source page is read up to this much looking for its preview image.
const PAGE_CAP: u64 = 512 * 1024;
const IMAGE_CAP: u64 = 4 * 1024 * 1024;
/// Re-read the MEMORY.md ledger at most this often while the page is open.
const LEDGER_TTL: std::time::Duration = std::time::Duration::from_secs(5);

/// Feed is the main view and opens first; Ideas is second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PulseTab {
    #[default]
    Feed,
    Ideas,
}

impl PulseTab {
    /// Left to right in the header switch.
    pub const ORDER: [PulseTab; 2] = [PulseTab::Feed, PulseTab::Ideas];

    pub fn label(self) -> &'static str {
        match self {
            PulseTab::Feed => "Feed",
            PulseTab::Ideas => "Ideas",
        }
    }
}

/// What the Pulse page keeps between frames.
#[derive(Default)]
pub(super) struct PulseView {
    pub tab: PulseTab,
    /// Crossfade progress Feed(0)→Ideas(1); set each frame in ui_pulse.
    pub tab_t: f32,
    /// The Feed instructions sheet is open with this draft.
    pub sheet: Option<String>,
    ledger: Vec<LedgerEntry>,
    ledger_at: Option<std::time::Instant>,
    textures: HashMap<String, Option<egui::TextureHandle>>,
    image_rx: Option<mpsc::Receiver<(String, Option<String>)>>,
    image_tried: HashSet<String>,
    pub rewrite_rx: Option<mpsc::Receiver<String>>,
    /// The Ideas row the keyboard is on (Up/Down or J/K; R, S, D, N, Enter act on it).
    pub focus: Option<String>,
    /// The row under the pointer last frame, and whether its bold line was.
    hover: Option<String>,
    title_hover: Option<String>,
    /// Suggest ideas was pressed with no Grok sign-in: say so under the header.
    pub signin_note: bool,
    /// Source images that failed to download or decode. Their slot is dropped.
    image_failed: HashSet<String>,
}

/// How one Ideas row paints this frame.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RowState {
    pub hovered: bool,
    pub focused: bool,
    pub title_hot: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PulseAct {
    Run(String),
    Snooze(String),
    Dismiss(String),
    NotThis(String),
    Always(String),
    Accept(String),
    Wrong(String),
    Open(String),
    Like(String),
    Discuss(String),
    Link(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Glyph {
    Play,
    Eye,
    Spark,
    Loop,
    Moon,
    Coin,
    People,
    Heart,
    Bag,
    Star,
    List,
}

fn category_tint(cat: PulseCategory) -> egui::Color32 {
    match cat {
        PulseCategory::Financial => egui::Color32::from_rgb(0x3f, 0xb3, 0x7f),
        PulseCategory::Productivity => egui::Color32::from_rgb(0x4a, 0x90, 0xe2),
        PulseCategory::Relationships => egui::Color32::from_rgb(0xe4, 0x6f, 0xa1),
        PulseCategory::Health => egui::Color32::from_rgb(0xf0, 0x6a, 0x4a),
        PulseCategory::Shopping => egui::Color32::from_rgb(0xe5, 0xa2, 0x3b),
        PulseCategory::More => egui::Color32::from_rgb(0x9b, 0x7b, 0xea),
    }
}

/// One fixed color per card type, so the small type word reads at a glance.
pub(super) fn type_color(kind: PulseType) -> egui::Color32 {
    match kind {
        PulseType::Do => egui::Color32::from_rgb(0xe7, 0xe9, 0xea),
        PulseType::Automate => egui::Color32::from_rgb(0x9b, 0x7b, 0xea),
        PulseType::Learn => egui::Color32::from_rgb(0xf4, 0xb7, 0x40),
        PulseType::Watch => egui::Color32::from_rgb(0x4a, 0x90, 0xe2),
        PulseType::Quiet => crate::theme::muted(),
    }
}

fn category_glyph(cat: PulseCategory) -> Glyph {
    match cat {
        PulseCategory::Financial => Glyph::Coin,
        PulseCategory::Productivity => Glyph::List,
        PulseCategory::Relationships => Glyph::People,
        PulseCategory::Health => Glyph::Heart,
        PulseCategory::Shopping => Glyph::Bag,
        PulseCategory::More => Glyph::Star,
    }
}

fn type_glyph(kind: PulseType) -> Glyph {
    match kind {
        PulseType::Do => Glyph::Play,
        PulseType::Watch => Glyph::Eye,
        PulseType::Learn => Glyph::Spark,
        PulseType::Automate => Glyph::Loop,
        PulseType::Quiet => Glyph::Moon,
    }
}

fn feed_glyph(card: &UpdateCard) -> Glyph {
    if card.source_id == pc::QUIET_DIGEST_SOURCE {
        return Glyph::Moon;
    }
    match card.kind {
        UpdateKind::AutomationDone => Glyph::List,
        UpdateKind::ScheduleCreated => Glyph::Loop,
        UpdateKind::Digest => Glyph::Star,
        _ => type_glyph(pc::pulse_type(card)),
    }
}

fn ring(center: egui::Pos2, rx: f32, ry: f32, from: f32, to: f32, n: usize) -> Vec<egui::Pos2> {
    (0..=n)
        .map(|i| {
            let t = from + (to - from) * i as f32 / n as f32;
            center + egui::vec2(rx * t.cos(), ry * t.sin())
        })
        .collect()
}

/// Our own line-art icons on a tinted tile. No emoji font and no borrowed art.
fn paint_glyph(painter: &egui::Painter, rect: egui::Rect, glyph: Glyph, tint: egui::Color32) {
    painter.rect_filled(rect, 9.0, tint.gamma_multiply(0.20));
    let c = rect.center();
    let s = rect.width() * 0.5;
    let stroke = egui::Stroke::new(1.8_f32, tint);
    let tau = std::f32::consts::TAU;
    match glyph {
        Glyph::Play => {
            let pts = vec![
                c + egui::vec2(-0.30 * s, -0.40 * s),
                c + egui::vec2(0.42 * s, 0.0),
                c + egui::vec2(-0.30 * s, 0.40 * s),
            ];
            painter.add(egui::Shape::convex_polygon(pts, tint, egui::Stroke::NONE));
        }
        Glyph::Eye => {
            painter.add(egui::Shape::closed_line(
                ring(c, 0.55 * s, 0.32 * s, 0.0, tau, 28),
                stroke,
            ));
            painter.circle_filled(c, 0.16 * s, tint);
        }
        Glyph::Spark => {
            for (a, b) in [((0.0, -0.55), (0.0, 0.55)), ((-0.55, 0.0), (0.55, 0.0))] {
                painter.line_segment(
                    [
                        c + egui::vec2(a.0 * s, a.1 * s),
                        c + egui::vec2(b.0 * s, b.1 * s),
                    ],
                    stroke,
                );
            }
            for (x, y) in [(-0.28, -0.28), (0.28, 0.28), (-0.28, 0.28), (0.28, -0.28)] {
                painter.line_segment(
                    [
                        c + egui::vec2(x * 0.5 * s, y * 0.5 * s),
                        c + egui::vec2(x * s, y * s),
                    ],
                    stroke,
                );
            }
        }
        Glyph::Loop => {
            painter.add(egui::Shape::line(
                ring(c, 0.42 * s, 0.42 * s, -1.2, 4.4, 26),
                stroke,
            ));
            let tip = c + egui::vec2(0.42 * s * (-1.2_f32).cos(), 0.42 * s * (-1.2_f32).sin());
            let pts = vec![
                tip + egui::vec2(-0.20 * s, -0.14 * s),
                tip + egui::vec2(0.20 * s, -0.02 * s),
                tip + egui::vec2(-0.04 * s, 0.22 * s),
            ];
            painter.add(egui::Shape::convex_polygon(pts, tint, egui::Stroke::NONE));
        }
        Glyph::Moon => {
            painter.circle_filled(c, 0.42 * s, tint);
            painter.circle_filled(
                c + egui::vec2(0.22 * s, -0.16 * s),
                0.36 * s,
                crate::theme::bg(),
            );
        }
        Glyph::Coin => {
            painter.circle_stroke(c, 0.50 * s, stroke);
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                "$",
                egui::FontId::proportional(0.72 * s),
                tint,
            );
        }
        Glyph::List => {
            for dy in [-0.32, 0.0, 0.32] {
                let y = c.y + dy * s;
                painter.circle_filled(egui::pos2(c.x - 0.40 * s, y), 0.07 * s + 0.6, tint);
                painter.line_segment(
                    [egui::pos2(c.x - 0.20 * s, y), egui::pos2(c.x + 0.46 * s, y)],
                    stroke,
                );
            }
        }
        Glyph::People => {
            for dx in [-0.24, 0.26] {
                let head = c + egui::vec2(dx * s, -0.20 * s);
                painter.circle_stroke(head, 0.15 * s, stroke);
                let body = c + egui::vec2(dx * s, 0.42 * s);
                painter.add(egui::Shape::line(
                    ring(body, 0.26 * s, 0.24 * s, std::f32::consts::PI, tau, 12),
                    stroke,
                ));
            }
        }
        Glyph::Heart => {
            painter.circle_filled(c + egui::vec2(-0.20 * s, -0.12 * s), 0.22 * s, tint);
            painter.circle_filled(c + egui::vec2(0.20 * s, -0.12 * s), 0.22 * s, tint);
            let pts = vec![
                c + egui::vec2(-0.41 * s, -0.04 * s),
                c + egui::vec2(0.41 * s, -0.04 * s),
                c + egui::vec2(0.0, 0.46 * s),
            ];
            painter.add(egui::Shape::convex_polygon(pts, tint, egui::Stroke::NONE));
        }
        Glyph::Bag => {
            let body = egui::Rect::from_center_size(
                c + egui::vec2(0.0, 0.14 * s),
                egui::vec2(0.84 * s, 0.62 * s),
            );
            painter.rect_stroke(body, 3.0, stroke, egui::StrokeKind::Middle);
            painter.add(egui::Shape::line(
                ring(
                    c + egui::vec2(0.0, -0.17 * s),
                    0.22 * s,
                    0.24 * s,
                    std::f32::consts::PI,
                    tau,
                    12,
                ),
                stroke,
            ));
        }
        Glyph::Star => {
            let pts: Vec<egui::Pos2> = (0..10)
                .map(|i| {
                    let r = if i % 2 == 0 { 0.52 * s } else { 0.22 * s };
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * tau / 10.0;
                    c + egui::vec2(r * a.cos(), r * a.sin())
                })
                .collect();
            painter.add(egui::Shape::closed_line(pts, stroke));
        }
    }
}

fn icon(ui: &mut egui::Ui, glyph: Glyph, tint: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ICON, ICON), egui::Sense::hover());
    paint_glyph(ui.painter(), rect, glyph, tint);
}

/// Up to `rows` lines, then an ellipsis.
fn clamp_label(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32, rows: usize) {
    let mut job = egui::text::LayoutJob::simple(
        text.to_string(),
        egui::FontId::proportional(size),
        color,
        ui.available_width(),
    );
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');
    ui.add(egui::Label::new(job).selectable(false));
}

fn idea_blurb(card: &UpdateCard) -> String {
    card.body
        .as_deref()
        .filter(|b| !b.trim().is_empty())
        .or(card.details.as_deref())
        .or(card.why.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// The ··· menu on a row or a post. One click, one choice, then it closes.
fn pulse_menu(
    ui: &mut egui::Ui,
    card: &UpdateCard,
    kind: PulseType,
    feed: bool,
    act: &mut Option<PulseAct>,
) {
    let id = card.id.clone();
    let hour = Cabin::local_clock().hour;
    // Ideas rows take R / S / D / N / Enter; the hint sits on the right.
    let pick =
        |ui: &mut egui::Ui, label: &str, key: &str, a: PulseAct, act: &mut Option<PulseAct>| {
            let key = if feed { "" } else { key };
            let button = egui::Button::new(label).shortcut_text(
                RichText::new(key)
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::subtle()),
            );
            if ui.add(button).clicked() {
                *act = Some(a);
                ui.close();
            }
        };
    if !feed {
        pick(
            ui,
            "Run in the background",
            "R",
            PulseAct::Run(id.clone()),
            act,
        );
    }
    pick(
        ui,
        pc::snooze_label(hour),
        "S",
        PulseAct::Snooze(id.clone()),
        act,
    );
    if kind == PulseType::Learn {
        pick(
            ui,
            "That's right, keep it",
            "",
            PulseAct::Accept(id.clone()),
            act,
        );
        pick(ui, "That's wrong", "", PulseAct::Wrong(id.clone()), act);
    } else if !feed {
        pick(ui, "Always do this", "", PulseAct::Always(id.clone()), act);
    }
    pick(ui, "Open", "Enter", PulseAct::Open(id.clone()), act);
    ui.separator();
    pick(ui, "Not this", "N", PulseAct::NotThis(id.clone()), act);
    pick(ui, "Dismiss", "D", PulseAct::Dismiss(id), act);
}

/// A small frameless text button for the row's inline actions.
fn quick_button(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(
        egui::Button::new(RichText::new(label).size(crate::theme::FONT_TIP))
            .frame(false)
            .min_size(egui::vec2(0.0, 18.0)),
    )
    .clicked()
}

/// One Ideas row: icon, bold "I can …" line, two or three lines of detail, the
/// type word, and ···. Hovered or focused, Run · Snooze · Dismiss show inline.
/// Returns the action, the row's rect, and whether the bold line is hovered.
pub(super) fn paint_pulse_row(
    ui: &mut egui::Ui,
    card: &UpdateCard,
    kind: PulseType,
    st: RowState,
) -> (Option<PulseAct>, egui::Rect, bool) {
    let mut act = None;
    let mut title_hover = false;
    let active = st.hovered || st.focused;
    let cat = pc::card_category(card);
    let frame = egui::Frame::NONE
        .fill(if active {
            ROW_HOVER
        } else {
            egui::Color32::TRANSPARENT
        })
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(8, 6));
    let resp = frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.add_space(2.0);
                icon(ui, category_glyph(cat), category_tint(cat));
                ui.add_space(10.0);
                let text_w = (ui.available_width() - 36.0).max(80.0);
                ui.vertical(|ui| {
                    ui.set_width(text_w);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let hot = st.title_hot || st.focused;
                    let mut title = RichText::new(pc::i_can_title(card))
                        .size(crate::theme::FONT_UI)
                        .strong()
                        .color(if hot {
                            egui::Color32::WHITE
                        } else {
                            crate::theme::fg()
                        });
                    if hot {
                        title = title.underline();
                    }
                    let t = ui.add(egui::Label::new(title).wrap().sense(egui::Sense::click()));
                    title_hover = t.hovered();
                    if t.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if t.clicked() {
                        act = Some(PulseAct::Open(card.id.clone()));
                    }
                    let blurb = idea_blurb(card);
                    if !blurb.is_empty() {
                        clamp_label(
                            ui,
                            &blurb,
                            crate::theme::FONT_BODY,
                            crate::theme::muted(),
                            3,
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(kind.label().to_ascii_uppercase())
                                .size(crate::theme::FONT_TIP - 1.0)
                                .strong()
                                .color(type_color(kind)),
                        );
                        if active {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    let dot = |ui: &mut egui::Ui| {
                                        ui.label(
                                            RichText::new("·")
                                                .size(crate::theme::FONT_TIP)
                                                .color(crate::theme::subtle()),
                                        );
                                    };
                                    if quick_button(ui, "Dismiss") {
                                        act = Some(PulseAct::Dismiss(card.id.clone()));
                                    }
                                    dot(ui);
                                    if quick_button(ui, "Snooze") {
                                        act = Some(PulseAct::Snooze(card.id.clone()));
                                    }
                                    dot(ui);
                                    if quick_button(ui, "Run") {
                                        act = Some(PulseAct::Run(card.id.clone()));
                                    }
                                },
                            );
                        }
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    crate::cards::dots_menu(ui, active, |ui| {
                        pulse_menu(ui, card, kind, false, &mut act)
                    });
                });
            });
        })
        .response;
    if st.focused {
        ui.painter().rect_stroke(
            resp.rect,
            10.0,
            egui::Stroke::new(2.0_f32, FOCUS_RING),
            egui::StrokeKind::Outside,
        );
    }
    (act, resp.rect, title_hover)
}

/// "source · age" for the line above a post's title.
pub(super) fn post_age_line(card: &UpdateCard, now: u64) -> String {
    format!(
        "{} · {}",
        post_source(card),
        pc::ago_label(card.created_at, now)
    )
}

/// Star, list, or the card's type glyph. The deck and the feed use the same one.
pub(super) fn paint_feed_icon(ui: &mut egui::Ui, card: &UpdateCard) {
    icon(ui, feed_glyph(card), category_tint(pc::card_category(card)));
}

pub(super) enum FeedPostAct {
    Like,
    Discuss,
    Link(String),
}

/// Title, short takeaway, Read at, images, then Like / Discuss.
/// `thumbs` is empty when the post has no image yet.
pub(super) fn paint_post_body(
    ui: &mut egui::Ui,
    card: &UpdateCard,
    thumbs: &[Thumb],
) -> Option<FeedPostAct> {
    let mut act = None;
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
    let takeaway = grokhub_core::short_takeaway(card);
    if !takeaway.is_empty() {
        clamp_label(
            ui,
            &takeaway,
            crate::theme::FONT_BODY,
            crate::theme::muted(),
            3,
        );
    }
    if let Some(url) = card.citations.first() {
        let host = pc::source_host(url).unwrap_or_else(|| url.clone());
        let link = ui.add(
            egui::Label::new(
                RichText::new(format!("Read at {host}"))
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::link()),
            )
            .sense(egui::Sense::click())
            .selectable(false),
        );
        if link.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if link.clicked() {
            act = Some(FeedPostAct::Link(url.clone()));
        }
    }
    if !thumbs.is_empty() {
        ui.add_space(2.0);
        paint_thumbs(ui, thumbs);
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        if paint_heart_button(ui, card.reaction == Some(CardReaction::Up)) {
            act = Some(FeedPostAct::Like);
        }
        if matches!(card.kind, UpdateKind::Digest | UpdateKind::Suggestion) {
            let talk = ui.add(
                egui::Button::new(RichText::new("Discuss").size(crate::theme::FONT_TIP))
                    .frame(false)
                    .min_size(egui::vec2(0.0, 24.0)),
            );
            if talk.clicked() {
                act = Some(FeedPostAct::Discuss);
            }
        }
    });
    act
}

/// Where a post came from, for the line under its headline.
pub(super) fn post_source(card: &UpdateCard) -> String {
    if let Some(name) = card
        .pulse
        .source_name
        .as_deref()
        .filter(|n| !n.trim().is_empty())
    {
        return name.trim().to_string();
    }
    if let Some(host) = card.citations.first().and_then(|u| pc::source_host(u)) {
        return host;
    }
    match card.kind {
        UpdateKind::AutomationDone => "Automation run".into(),
        UpdateKind::ScheduleCreated => "Automations".into(),
        UpdateKind::Digest => "Daily digest".into(),
        _ => "Cabin".into(),
    }
}

fn paint_heart_button(ui: &mut egui::Ui, on: bool) -> bool {
    let label = if on { "Liked" } else { "Like" };
    let resp = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // Heart plus word, about 60px: hovering either lights both.
            let hot = ui.rect_contains_pointer(egui::Rect::from_min_size(
                ui.cursor().min,
                egui::vec2(60.0, 18.0),
            ));
            let (rect, r1) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
            let tint = if on {
                egui::Color32::from_rgb(0xf0, 0x4e, 0x6a)
            } else if hot {
                crate::theme::fg()
            } else {
                crate::theme::muted()
            };
            let c = rect.center();
            let s = 7.0;
            if on {
                ui.painter()
                    .circle_filled(c + egui::vec2(-0.45 * s, -0.25 * s), 0.5 * s, tint);
                ui.painter()
                    .circle_filled(c + egui::vec2(0.45 * s, -0.25 * s), 0.5 * s, tint);
                let pts = vec![
                    c + egui::vec2(-0.93 * s, -0.08 * s),
                    c + egui::vec2(0.93 * s, -0.08 * s),
                    c + egui::vec2(0.0, 0.95 * s),
                ];
                ui.painter()
                    .add(egui::Shape::convex_polygon(pts, tint, egui::Stroke::NONE));
            } else {
                let stroke = egui::Stroke::new(1.4_f32, tint);
                let mut pts = ring(
                    c + egui::vec2(-0.45 * s, -0.25 * s),
                    0.5 * s,
                    0.5 * s,
                    2.6,
                    6.2,
                    10,
                );
                pts.extend(ring(
                    c + egui::vec2(0.45 * s, -0.25 * s),
                    0.5 * s,
                    0.5 * s,
                    3.2,
                    6.9,
                    10,
                ));
                pts.push(c + egui::vec2(0.0, 0.95 * s));
                pts.push(pts[0]);
                ui.painter().add(egui::Shape::line(pts, stroke));
            }
            let r2 = ui.add(
                egui::Label::new(
                    RichText::new(label)
                        .size(crate::theme::FONT_TIP)
                        .color(tint),
                )
                .sense(egui::Sense::click())
                .selectable(false),
            );
            r1.clicked() || r2.clicked()
        })
        .inner;
    resp
}

impl Cabin {
    /// The Pulse page: header with the Ideas | Feed switch, then the view.
    pub(super) fn ui_pulse(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll_pulse_images();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::symmetric(24, 20)),
            )
            .show(ui, |ui| {
                let full = ui.available_width();
                let col = full.min(COL_MAX_W);
                let pad = ((full - col) * 0.5).max(0.0);
                egui::ScrollArea::vertical()
                    .id_salt("pulse-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.add_space(pad);
                            ui.vertical(|ui| {
                                ui.set_width(col);
                                self.paint_pulse_header(ui);
                                // Wave 2E: Feed↔Ideas crossfade 180ms + shared-axis ±8px.
                                // Only the active tab paints (avoids stacked layout); opacity+x carry the feel.
                                // Header Feed|Ideas control stays put (painted above).
                                let ideas_on = self.pulse_view.tab == PulseTab::Ideas;
                                let tab_t = crate::motion::pulse_tab_t(ui.ctx(), ideas_on);
                                self.pulse_view.tab_t = tab_t;
                                let (opacity, x) = if ideas_on {
                                    (tab_t.clamp(0.0, 1.0), crate::motion::ideas_axis_x(tab_t))
                                } else {
                                    ((1.0 - tab_t).clamp(0.0, 1.0), crate::motion::feed_axis_x(tab_t))
                                };
                                let avail = ui.available_rect_before_wrap();
                                let rect = avail.translate(egui::vec2(x, 0.0));
                                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                    ui.set_min_width(avail.width());
                                    ui.multiply_opacity(opacity.max(0.001));
                                    match self.pulse_view.tab {
                                        PulseTab::Ideas => self.ui_ideas(ui),
                                        PulseTab::Feed => self.ui_pulse_feed(ui),
                                    }
                                });
                                if 0.001 < tab_t && tab_t < 0.999 {
                                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
                                }
                                ui.add_space(24.0);
                            });
                        });
                    });
            });
        self.paint_feed_instructions_sheet(&ctx);
    }

    fn paint_pulse_header(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(pc::PULSE_TITLE)
                    .font(crate::theme::title_font(28.0))
                    .color(crate::theme::fg()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // The tab's action has one fixed-width slot on the right, so the
                // Feed | Ideas switch sits in the same place on both tabs.
                let (slot, _) =
                    ui.allocate_exact_size(egui::vec2(HEADER_SLOT_W, 32.0), egui::Sense::hover());
                let mut cell = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(slot)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                match self.pulse_view.tab {
                    PulseTab::Feed => {
                        if crate::cards::ghost_pill(&mut cell, "Feed instructions") {
                            self.open_feed_instructions();
                        }
                    }
                    PulseTab::Ideas => {
                        // Same emptiness check as ui_ideas: purge once, then group.
                        // Hide this control only when the body will paint the inline
                        // Suggest (empty, and not mid-fetch). Loading keeps
                        // "Suggesting…"; a list that has ideas keeps "Suggest ideas".
                        self.ensure_useful_ideas();
                        let busy = self.ideas_rx.is_some();
                        let empty = self.pulse_groups(now_ms()).is_empty();
                        if busy || !empty {
                            let label = if busy {
                                "Suggesting…"
                            } else {
                                "Suggest ideas"
                            };
                            if crate::cards::ghost_pill(&mut cell, label) && !busy {
                                self.suggest_ideas_pressed();
                            }
                        }
                    }
                }
                ui.add_space(6.0);
                for tab in PulseTab::ORDER.iter().rev() {
                    let on = self.pulse_view.tab == *tab;
                    if crate::cards::felt_segment(ui, tab.label(), on).clicked() {
                        self.pulse_view.tab = *tab;
                    }
                }
            });
        });
        ui.label(
            RichText::new(pc::PULSE_SUBTITLE)
                .size(crate::theme::FONT_BODY)
                .color(crate::theme::muted()),
        );
        // PI-05: clear the signed-out note once Account (or native) is signed in.
        // CLI-on-PATH alone is not "signed in to Grok" for this line.
        if self.pulse_view.signin_note && (self.has_key() || self.cfg.native_engine) {
            self.pulse_view.signin_note = false;
        }
        if self.pulse_view.tab == PulseTab::Ideas && self.pulse_view.signin_note {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(pc::IDEAS_SIGN_IN)
                        .size(crate::theme::FONT_BODY)
                        .color(NOTE_AMBER),
                );
                let open = ui.add(
                    egui::Label::new(
                        RichText::new("Open Settings")
                            .size(crate::theme::FONT_BODY)
                            .underline()
                            .color(crate::theme::link()),
                    )
                    .sense(egui::Sense::click())
                    .selectable(false),
                );
                if open.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if open.clicked() {
                    self.nav = Nav::Settings;
                }
            });
        }
        ui.add_space(14.0);
    }

    /// Nothing to show: placeholder rows while ideas are being found, else the
    /// empty line for this tab and when the heartbeat last ran.
    /// Ideas empty: inline Suggest ideas. Feed empty: Feed instructions link.
    pub(super) fn paint_pulse_silence(&mut self, ui: &mut egui::Ui, feed: bool) {
        ui.add_space(8.0);
        if !feed && self.ideas_rx.is_some() {
            ui.label(
                RichText::new(pc::IDEAS_LOADING)
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::muted()),
            );
            ui.add_space(10.0);
            for _ in 0..3 {
                paint_skeleton_row(ui);
                ui.add_space(14.0);
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        if feed {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(
                    RichText::new(pc::FEED_EMPTY_LEAD)
                        .size(crate::theme::FONT_BODY)
                        .color(crate::theme::muted()),
                );
                let link = ui.add(
                    egui::Label::new(
                        RichText::new(pc::FEED_EMPTY_LINK)
                            .size(crate::theme::FONT_BODY)
                            .underline()
                            .color(crate::theme::link()),
                    )
                    .sense(egui::Sense::click())
                    .selectable(false),
                );
                if link.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if link.clicked() {
                    self.open_feed_instructions();
                }
                ui.label(
                    RichText::new(".")
                        .size(crate::theme::FONT_BODY)
                        .color(crate::theme::muted()),
                );
            });
        } else {
            ui.label(
                RichText::new(pc::IDEAS_EMPTY)
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::muted()),
            );
            ui.add_space(8.0);
            if crate::cards::ghost_pill(ui, "Suggest ideas") {
                self.suggest_ideas_pressed();
            }
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(pc::silence_line(self.last_pulse_ms(), now_ms()))
                .size(crate::theme::FONT_TIP)
                .color(crate::theme::subtle()),
        );
    }

    pub(super) fn last_pulse_ms(&self) -> u64 {
        let p = &self.cfg.feed_pulse;
        p.last_expiry_ms
            .max(p.last_quiet_release_ms)
            .max(p.last_ideas_ms)
    }

    // ------------------------------------------------------------ ranking inputs

    fn pulse_ledger(&mut self) -> Vec<LedgerEntry> {
        let stale = self
            .pulse_view
            .ledger_at
            .is_none_or(|at| at.elapsed() >= LEDGER_TTL);
        if stale {
            self.pulse_view.ledger = pc::parse_ledger(&crate::config::read_memory("MEMORY.md"));
            self.pulse_view.ledger_at = Some(std::time::Instant::now());
        }
        self.pulse_view.ledger.clone()
    }

    fn pulse_skills(&self) -> Vec<String> {
        self.skill_list
            .iter()
            .flat_map(|s| [s.name.clone(), s.trigger.clone()])
            .filter(|s| !s.trim().is_empty())
            .collect()
    }

    fn pulse_open_work(&self) -> Vec<String> {
        self.board
            .iter()
            .filter(|c| {
                !matches!(
                    c.status,
                    grokhub_core::BoardStatus::Done | grokhub_core::BoardStatus::Dismissed
                )
            })
            .map(|c| c.title.clone())
            .collect()
    }

    /// Every visible card, best first, with its score and type.
    #[cfg(test)]
    pub(super) fn pulse_ranked(&mut self, now: u64) -> Vec<Ranked> {
        let ledger = self.pulse_ledger();
        let skills = self.pulse_skills();
        let work = self.pulse_open_work();
        let inputs = pc::PulseInputs {
            ledger: &ledger,
            skills: &skills,
            open_work: &work,
            now_ms: now,
        };
        pc::rank_pulse(&self.updates, &inputs)
    }

    /// The Ideas view: the top group, then one group per category in order.
    pub(super) fn pulse_groups(&mut self, now: u64) -> Vec<(Option<PulseCategory>, Vec<Ranked>)> {
        let ledger = self.pulse_ledger();
        let skills = self.pulse_skills();
        let work = self.pulse_open_work();
        let inputs = pc::PulseInputs {
            ledger: &ledger,
            skills: &skills,
            open_work: &work,
            now_ms: now,
        };
        pc::group_ideas(&self.updates, &inputs)
    }

    /// The Feed view: posts newest first; muted sources stay off.
    pub(super) fn pulse_feed_posts(&mut self, now: u64) -> Vec<UpdateCard> {
        let ledger = self.pulse_ledger();
        let inputs = pc::PulseInputs {
            ledger: &ledger,
            skills: &[],
            open_work: &[],
            now_ms: now,
        };
        let muted = &self.cfg.feed_pulse.muted_sources;
        pc::feed_posts(&self.updates, &inputs)
            .into_iter()
            .filter(|c| c.source_id.is_empty() || !muted.iter().any(|m| m == &c.source_id))
            .collect()
    }

    // ------------------------------------------------------------ feed view

    pub(super) fn ui_pulse_feed(&mut self, ui: &mut egui::Ui) {
        let now = now_ms();
        let posts = self.pulse_feed_posts(now);
        self.kick_pulse_images(&posts);
        if posts.is_empty() {
            self.paint_pulse_silence(ui, true);
            return;
        }
        let mut act = None;
        for card in &posts {
            if let Some(a) = self.paint_pulse_post(ui, card, now) {
                act = Some(a);
            }
            ui.add_space(6.0);
            let w = ui.available_width();
            let (line, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
            ui.painter().rect_filled(line, 0.0, crate::theme::border());
            ui.add_space(10.0);
        }
        if let Some(a) = act {
            self.apply_pulse_act(a);
        }
    }

    /// One post: "source · age" with ··· on top, the headline, the short
    /// takeaway, the source link, its images, then Like / Discuss.
    fn paint_pulse_post(
        &mut self,
        ui: &mut egui::Ui,
        card: &UpdateCard,
        now: u64,
    ) -> Option<PulseAct> {
        let mut act = None;
        let kind = pc::pulse_type(card);
        let thumbs: Vec<Thumb> = card
            .pulse
            .image_urls
            .iter()
            .take(THUMB_MAX)
            .filter_map(|url| self.pulse_thumb(ui.ctx(), url))
            .collect();
        ui.horizontal_top(|ui| {
            paint_feed_icon(ui, card);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(post_age_line(card, now))
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::subtle()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        crate::cards::dots_menu(ui, false, |ui| {
                            pulse_menu(ui, card, kind, true, &mut act)
                        });
                    });
                });
                if let Some(did) = paint_post_body(ui, card, &thumbs) {
                    act = Some(match did {
                        FeedPostAct::Like => PulseAct::Like(card.id.clone()),
                        FeedPostAct::Discuss => PulseAct::Discuss(card.id.clone()),
                        FeedPostAct::Link(url) => PulseAct::Link(url),
                    });
                }
            });
        });
        act
    }

    /// A post's image slot: drawn, still on its way, or gone (`None`) when the
    /// download or decode failed, so a broken image never sits as a placeholder.
    pub(super) fn pulse_thumb(&mut self, ctx: &egui::Context, url: &str) -> Option<Thumb> {
        let name = pc::image_cache_name(url);
        if self.pulse_view.image_failed.contains(&name) {
            return None;
        }
        if let Some(tex) = self.pulse_texture(ctx, url) {
            return Some(Thumb::Ready(tex));
        }
        if pulse_image_path(url).exists() {
            // On disk but not an image.
            self.pulse_view.image_failed.insert(name);
            return None;
        }
        Some(Thumb::Loading)
    }

    #[cfg(test)]
    pub(super) fn pulse_image_failed(&self, url: &str) -> bool {
        self.pulse_view
            .image_failed
            .contains(&pc::image_cache_name(url))
    }

    // ------------------------------------------------------------ real images

    #[cfg(test)]
    pub(super) fn pulse_view_texture_size(&self, url: &str) -> Option<[usize; 2]> {
        self.pulse_view
            .textures
            .get(&pc::image_cache_name(url))
            .and_then(|t| t.as_ref().map(|t| t.size()))
    }

    fn pulse_texture(&mut self, ctx: &egui::Context, url: &str) -> Option<egui::TextureHandle> {
        let name = pc::image_cache_name(url);
        if let Some(tex) = self.pulse_view.textures.get(&name) {
            return tex.clone();
        }
        let path = pulse_image_path(url);
        let bytes = std::fs::read(&path).ok()?;
        let tex = image::load_from_memory(&bytes).ok().map(|img| {
            let img = if img.width() > 480 || img.height() > 480 {
                img.thumbnail(480, 480)
            } else {
                img
            };
            let thumb = img.to_rgba8();
            let size = [thumb.width() as usize, thumb.height() as usize];
            ctx.load_texture(
                format!("pulse-img-{name}"),
                egui::ColorImage::from_rgba_unmultiplied(size, thumb.as_raw()),
                egui::TextureOptions::LINEAR,
            )
        });
        // A file that does not decode stays a placeholder; it is not read again.
        self.pulse_view.textures.insert(name, tex.clone());
        tex
    }

    /// One post at a time, off the UI thread: the source page's own preview
    /// image, cached under the config dir. Unit tests never fetch.
    pub(super) fn kick_pulse_images(&mut self, posts: &[UpdateCard]) {
        if cfg!(test) || self.pulse_view.image_rx.is_some() {
            return;
        }
        let tried = &self.pulse_view.image_tried;
        let next = posts.iter().find(|c| {
            !tried.contains(&c.id)
                && (c
                    .pulse
                    .image_urls
                    .iter()
                    .any(|u| !pulse_image_path(u).exists())
                    || (c.pulse.image_urls.is_empty() && !c.citations.is_empty()))
        });
        let Some(card) = next else {
            return;
        };
        self.pulse_view.image_tried.insert(card.id.clone());
        let id = card.id.clone();
        let known = card.pulse.image_urls.first().cloned();
        let page = card.citations.first().cloned();
        let (tx, rx) = mpsc::channel();
        self.pulse_view.image_rx = Some(rx);
        std::thread::spawn(move || {
            let got = match (known, page) {
                (Some(url), _) => cache_image(&url).then_some(url),
                (None, Some(page)) => source_preview(&page).filter(|url| cache_image(url)),
                (None, None) => None,
            };
            let _ = tx.send((id, got));
        });
    }

    pub(super) fn poll_pulse_images(&mut self) {
        let Some(rx) = self.pulse_view.image_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((id, Some(url))) => {
                self.pulse_view.textures.remove(&pc::image_cache_name(&url));
                if let Some(card) = self.updates.iter_mut().find(|c| c.id == id) {
                    if !card.pulse.image_urls.contains(&url) {
                        card.pulse.image_urls = vec![url];
                        self.persist_updates();
                    }
                }
            }
            Ok((id, None)) => {
                // Nothing came back: drop the slot instead of a forever placeholder.
                if let Some(card) = self.updates.iter().find(|c| c.id == id) {
                    for url in &card.pulse.image_urls {
                        self.pulse_view
                            .image_failed
                            .insert(pc::image_cache_name(url));
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
            Err(mpsc::TryRecvError::Empty) => self.pulse_view.image_rx = Some(rx),
        }
    }

    // ------------------------------------------------------------ keyboard

    /// One key on the Ideas view. Up/Down (or J/K) move the focused row; R runs
    /// it in the background, S snoozes, D dismisses, N is Not this, Enter opens,
    /// Esc lets go. After Snooze, Dismiss, or Not this the next row takes focus.
    pub(super) fn pulse_key(&mut self, key: egui::Key, rows: &[String]) -> Option<PulseAct> {
        if rows.is_empty() {
            self.pulse_view.focus = None;
            return None;
        }
        let at = self
            .pulse_view
            .focus
            .as_ref()
            .and_then(|f| rows.iter().position(|r| r == f));
        let last = rows.len() - 1;
        match key {
            egui::Key::ArrowDown | egui::Key::J => {
                let i = at.map_or(0, |i| (i + 1).min(last));
                self.pulse_view.focus = Some(rows[i].clone());
                None
            }
            egui::Key::ArrowUp | egui::Key::K => {
                let i = at.map_or(0, |i| i.saturating_sub(1));
                self.pulse_view.focus = Some(rows[i].clone());
                None
            }
            egui::Key::Escape => {
                self.pulse_view.focus = None;
                None
            }
            _ => {
                let i = at?;
                let id = rows[i].clone();
                let act = match key {
                    egui::Key::R => PulseAct::Run(id),
                    egui::Key::S => PulseAct::Snooze(id),
                    egui::Key::D => PulseAct::Dismiss(id),
                    egui::Key::N => PulseAct::NotThis(id),
                    egui::Key::Enter => PulseAct::Open(id),
                    _ => return None,
                };
                if matches!(
                    act,
                    PulseAct::Snooze(_) | PulseAct::Dismiss(_) | PulseAct::NotThis(_)
                ) {
                    let next = if i < last { i + 1 } else { i.saturating_sub(1) };
                    self.pulse_view.focus = (next != i).then(|| rows[next].clone());
                }
                Some(act)
            }
        }
    }

    /// Read this frame's row keys. Typing in a box, the palette, or the Feed
    /// instructions sheet keeps them.
    pub(super) fn pulse_keys(&mut self, ctx: &egui::Context, rows: &[String]) -> Option<PulseAct> {
        if rows.is_empty()
            || ctx.egui_wants_keyboard_input()
            || self.pulse_view.sheet.is_some()
            || self.palette_open
        {
            return None;
        }
        let focused = self
            .pulse_view
            .focus
            .as_ref()
            .is_some_and(|f| rows.contains(f));
        let mut keys = vec![
            egui::Key::ArrowDown,
            egui::Key::ArrowUp,
            egui::Key::J,
            egui::Key::K,
        ];
        if focused {
            keys.extend([
                egui::Key::R,
                egui::Key::S,
                egui::Key::D,
                egui::Key::N,
                egui::Key::Enter,
                egui::Key::Escape,
            ]);
        }
        for key in keys {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                return self.pulse_key(key, rows);
            }
        }
        None
    }

    /// Ideas rows report their rect and hover each frame; next frame paints
    /// the hovered one with its inline actions.
    pub(super) fn pulse_row_state(&self, id: &str) -> RowState {
        RowState {
            hovered: self.pulse_view.hover.as_deref() == Some(id),
            focused: self.pulse_view.focus.as_deref() == Some(id),
            title_hot: self.pulse_view.title_hover.as_deref() == Some(id),
        }
    }

    pub(super) fn pulse_row_state_changed(
        &self,
        hover: &Option<String>,
        title: &Option<String>,
    ) -> bool {
        &self.pulse_view.hover != hover || &self.pulse_view.title_hover != title
    }

    pub(super) fn set_pulse_hover(&mut self, row: Option<String>, title: Option<String>) {
        self.pulse_view.hover = row;
        self.pulse_view.title_hover = title;
    }

    // ------------------------------------------------------------ buttons

    pub(super) fn apply_pulse_act(&mut self, act: PulseAct) {
        match act {
            PulseAct::Run(id) => {
                self.pulse_run(&id);
            }
            PulseAct::Snooze(id) => {
                let clock = Self::local_clock();
                self.pulse_snooze_at(&id, clock.now_ms, clock.hour, clock.minute);
            }
            PulseAct::Dismiss(id) => self.pulse_dismiss(&id, &Self::local_day()),
            PulseAct::NotThis(id) => self.pulse_not_this(&id, &Self::local_day()),
            PulseAct::Always(id) => self.pulse_always(&id, &Self::local_day()),
            PulseAct::Accept(id) => self.pulse_accept(&id, &Self::local_day()),
            PulseAct::Wrong(id) => self.pulse_wrong(&id, &Self::local_day()),
            PulseAct::Like(id) => self.pulse_like(&id, &Self::local_day()),
            PulseAct::Open(id) => self.pulse_open(&id),
            PulseAct::Discuss(id) => self.discuss_card(&id),
            PulseAct::Link(url) => {
                self.follow_update_action(Some(UpdateAction::DeepLink { href: url }))
            }
        }
    }

    /// One structured line in MEMORY.md. Likes and dislikes count toward the
    /// next rewrite of the feed instructions.
    fn pulse_note(&mut self, title: &str, reason: LedgerReason, day: &str) {
        let line = pc::ledger_line(day, title, reason);
        if let Err(err) = crate::config::append_memory("MEMORY.md", &line) {
            self.status = err;
        }
        self.pulse_view.ledger_at = None;
        if reason.is_taste() {
            self.pulse_taste_tick();
        }
    }

    fn pulse_card(&self, id: &str) -> Option<UpdateCard> {
        self.updates.iter().find(|c| c.id == id).cloned()
    }

    /// Run: the idea's own action as a `/bg` task. The composer stays free.
    pub(super) fn pulse_run(&mut self, id: &str) -> Option<String> {
        let card = self.pulse_card(id)?;
        let line = pc::run_line(&card);
        if grokhub_core::mark_update_opened(&mut self.updates, id) {
            self.persist_updates();
        }
        self.send_chat(line.clone());
        Some(line)
    }

    /// Snooze: hidden until the next 09:00 local. Returns that unix ms.
    pub(super) fn pulse_snooze_at(&mut self, id: &str, now_ms: u64, hour: u32, minute: u32) -> u64 {
        let until = pc::snooze_until(now_ms, hour, minute);
        if pc::snooze_card(&mut self.updates, id, until) {
            self.persist_updates();
            self.status = format!("{}.", pc::snooze_label(hour).replace("Snooze", "Snoozed"));
        }
        until
    }

    fn pulse_remove(&mut self, card: &UpdateCard) {
        if card.kind == UpdateKind::Idea {
            self.delete_idea(&card.id);
        } else {
            self.dismiss_feed_card(&card.id);
        }
    }

    pub(super) fn pulse_dismiss(&mut self, id: &str, day: &str) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        self.pulse_note(&card.title, LedgerReason::Dismiss, day);
        self.pulse_remove(&card);
        self.heartbeat_card_dismissed();
    }

    /// Not this: a dislike line in the ledger, then the card goes. Cards on the
    /// same topic rank lower from the next pass on.
    pub(super) fn pulse_not_this(&mut self, id: &str, day: &str) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        self.pulse_note(&card.title, LedgerReason::NotThis, day);
        self.pulse_remove(&card);
        self.heartbeat_card_dismissed();
        self.status = "Got it. Less like this.".into();
    }

    pub(super) fn pulse_like(&mut self, id: &str, day: &str) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        if card.reaction == Some(CardReaction::Up) {
            return;
        }
        if let Some(saved) = self.updates.iter_mut().find(|c| c.id == id) {
            saved.reaction = Some(CardReaction::Up);
        }
        self.persist_updates();
        self.pulse_note(&card.title, LedgerReason::Liked, day);
    }

    /// Always do this: an automation idea is scheduled, an offer is taken, and
    /// anything else opens Automations with its action filled in.
    pub(super) fn pulse_always(&mut self, id: &str, day: &str) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        self.pulse_note(&card.title, LedgerReason::Accepted, day);
        match card.kind {
            UpdateKind::AutomateOffer => self.accept_automate_offer(id),
            UpdateKind::Idea if card.idea_kind == Some(grokhub_core::IdeaKind::Automation) => {
                self.apply_idea(id)
            }
            _ => {
                let action = card.idea_action();
                self.night_nl = if action.is_empty() {
                    card.title.clone()
                } else {
                    action
                };
                self.auto_compose = true;
                self.nav = Nav::Night;
            }
        }
    }

    pub(super) fn pulse_accept(&mut self, id: &str, day: &str) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        self.pulse_note(&card.title, LedgerReason::Accepted, day);
        self.apply_idea(id);
    }

    pub(super) fn pulse_wrong(&mut self, id: &str, day: &str) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        self.pulse_note(&card.title, LedgerReason::Wrong, day);
        self.pulse_remove(&card);
    }

    fn pulse_open(&mut self, id: &str) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        if card.kind == UpdateKind::Idea {
            if self.idea_board.open.as_deref() == Some(id) {
                self.idea_board.open = None;
            } else {
                self.open_idea_on_board(id);
            }
        } else {
            self.open_feed_card(id);
        }
    }

    // ------------------------------------------------------------ feed instructions

    pub(super) fn open_feed_instructions(&mut self) {
        let text = pc::feed_instructions_or_default(&self.cfg.feed_instructions).to_string();
        self.pulse_view.sheet = Some(text);
    }

    pub(super) fn save_feed_instructions(&mut self, text: &str) {
        let text: String = text
            .trim()
            .chars()
            .take(pc::FEED_INSTRUCTIONS_CAP)
            .collect();
        self.cfg.feed_instructions = if text == pc::DEFAULT_FEED_INSTRUCTIONS {
            String::new()
        } else {
            text
        };
        self.persist_cfg();
        self.status = "Feed instructions saved. Future posts follow them.".into();
    }

    fn paint_feed_instructions_sheet(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.pulse_view.sheet.clone() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let since = self.cfg.feed_pulse.taste_since_rewrite;
        let left = pc::REWRITE_AFTER.saturating_sub(since).max(1);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            save = true;
        }
        let modal = egui::Modal::new(egui::Id::new("pulse-feed-instructions"))
            .backdrop_color(egui::Color32::from_black_alpha(153))
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border_strong()))
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ctx, |ui| {
                ui.set_width(560.0_f32.min(ctx.content_rect().width() - 80.0));
                // A clear ring on the focused box.
                ui.visuals_mut().selection.stroke = egui::Stroke::new(2.0_f32, FOCUS_RING);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Feed instructions")
                            .font(crate::theme::title_font(22.0))
                            .color(crate::theme::fg()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close = ui
                            .add(
                                egui::Button::new(RichText::new("×").size(18.0))
                                    .frame(false)
                                    .min_size(egui::vec2(28.0, 28.0)),
                            )
                            .on_hover_text("Close (Esc)");
                        if close.clicked() {
                            cancel = true;
                        }
                    });
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Plain words that shape every future post. I tune this myself as I learn what you like and skip, and you can change any of it.")
                        .size(crate::theme::FONT_BODY)
                        .color(crate::theme::muted()),
                );
                ui.add_space(12.0);
                ui.add(
                    egui::TextEdit::multiline(&mut draft)
                        .id(egui::Id::new("pulse-feed-instructions-text"))
                        .desired_rows(14)
                        .desired_width(f32::INFINITY)
                        .char_limit(pc::FEED_INSTRUCTIONS_CAP)
                        .font(egui::FontId::proportional(crate::theme::FONT_BODY)),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "I'll update these after {left} more {}.",
                        if left == 1 { "like or skip" } else { "likes or skips" }
                    ))
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::subtle()),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Ctrl+Enter to save · Esc to cancel")
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::subtle()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::cards::white_pill(ui, "Save") {
                            save = true;
                        }
                        if crate::cards::ghost_pill(ui, "Cancel") {
                            cancel = true;
                        }
                    });
                });
            });
        if save {
            self.save_feed_instructions(&draft);
            self.pulse_view.sheet = None;
        } else if cancel || modal.should_close() {
            self.pulse_view.sheet = None;
        } else {
            self.pulse_view.sheet = Some(draft);
        }
    }

    /// Three likes or dislikes since the last rewrite: ask the model to fold
    /// them into the instructions.
    fn pulse_taste_tick(&mut self) {
        let pulse = &mut self.cfg.feed_pulse;
        pulse.taste_since_rewrite = pulse.taste_since_rewrite.saturating_add(1);
        let due = pulse.taste_since_rewrite >= pc::REWRITE_AFTER;
        self.persist_cfg();
        if due {
            self.spawn_pulse_rewrite();
        }
    }

    pub(super) fn spawn_pulse_rewrite(&mut self) {
        if self.pulse_view.rewrite_rx.is_some() {
            return;
        }
        let ledger = pc::parse_ledger(&crate::config::read_memory("MEMORY.md"));
        let taste = pc::recent_taste(&ledger, pc::REWRITE_AFTER as usize);
        if taste.is_empty() {
            return;
        }
        let prompt = pc::rewrite_prompt(&self.cfg.feed_instructions, &taste);
        let (tx, rx) = mpsc::channel();
        if self.cfg.native_engine {
            self.pulse_view.rewrite_rx = Some(rx);
            self.spawn_native_ideas(prompt, tx);
            return;
        }
        // Unit tests never start it: this machine's Grok login would spend a real call.
        if cfg!(test) || !self.llm_ready() {
            return;
        }
        let key = self.bearer();
        self.pulse_view.rewrite_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(cabin_fast_llm(key, prompt));
        });
    }

    pub(super) fn poll_pulse_rewrite(&mut self) {
        let Some(rx) = self.pulse_view.rewrite_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(reply) => {
                if let Some(text) = pc::parse_rewrite(&reply) {
                    self.cfg.feed_instructions = text;
                    self.cfg.feed_pulse.taste_since_rewrite = 0;
                    self.persist_cfg();
                    self.status =
                        "Feed instructions updated from what you liked and skipped".into();
                }
            }
            Err(mpsc::TryRecvError::Empty) => self.pulse_view.rewrite_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    // ------------------------------------------------------------ migration

    /// Once: the old Home deck and Ideas board move into Pulse. Every card,
    /// pin, and reaction is kept; ideas get a category; an older digest brief
    /// becomes a Focus line in the feed instructions.
    pub(super) fn migrate_pulse_store(&mut self) -> Option<pc::PulseMigration> {
        let out = pc::migrate_to_pulse(&mut self.updates, &mut self.cfg.feed_pulse)?;
        if self.cfg.feed_instructions.trim().is_empty() && !self.cfg.digest_brief.trim().is_empty()
        {
            self.cfg.feed_instructions = pc::instructions_from_brief(&self.cfg.digest_brief);
        }
        self.persist_updates();
        self.persist_cfg();
        Some(out)
    }
}

/// One image slot on a post.
pub(super) enum Thumb {
    Ready(egui::TextureHandle),
    /// Still downloading: a plain block, no icon.
    Loading,
}

/// Placeholder row while ideas are being found: the size of a real row.
fn paint_skeleton_row(ui: &mut egui::Ui) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 52.0), egui::Sense::hover());
    let block = crate::theme::elevated();
    let p = ui.painter();
    p.rect_filled(
        egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 4.0), egui::vec2(ICON, ICON)),
        9.0,
        block,
    );
    let left = rect.left() + 10.0 + ICON + 10.0;
    p.rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(left, rect.top() + 6.0),
            egui::vec2((w * 0.55).min(360.0), 12.0),
        ),
        4.0,
        block,
    );
    p.rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(left, rect.top() + 26.0),
            egui::vec2((w * 0.75).min(480.0), 10.0),
        ),
        4.0,
        block,
    );
}

fn paint_thumbs(ui: &mut egui::Ui, thumbs: &[Thumb]) {
    let gap = 6.0;
    let n = thumbs.len().max(1) as f32;
    let max_w = ui.available_width();
    let one = if thumbs.len() == 1 {
        (max_w * 0.5).min(300.0)
    } else {
        ((max_w - gap * (n - 1.0)) / n).min(170.0)
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for thumb in thumbs {
            let h = if thumbs.len() == 1 {
                (one * 0.56).min(200.0)
            } else {
                THUMB_H
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(one, h), egui::Sense::hover());
            match thumb {
                Thumb::Ready(tex) => {
                    // Cover-crop into the slot, like a link preview.
                    let [tw, th] = tex.size();
                    let (tw, th) = (tw as f32, th as f32);
                    let slot = rect.width() / rect.height();
                    let img = tw / th.max(1.0);
                    let uv = if img > slot {
                        let w = slot / img;
                        egui::Rect::from_min_max(
                            egui::pos2(0.5 - w / 2.0, 0.0),
                            egui::pos2(0.5 + w / 2.0, 1.0),
                        )
                    } else {
                        let h = img / slot;
                        egui::Rect::from_min_max(
                            egui::pos2(0.0, 0.5 - h / 2.0),
                            egui::pos2(1.0, 0.5 + h / 2.0),
                        )
                    };
                    egui::Image::from_texture(tex)
                        .uv(uv)
                        .corner_radius(8.0)
                        .paint_at(ui, rect);
                }
                Thumb::Loading => {
                    ui.painter()
                        .rect_filled(rect, 8.0, crate::theme::elevated());
                }
            }
        }
    });
}

pub(super) fn pulse_image_path(url: &str) -> std::path::PathBuf {
    crate::config::config_dir()
        .join("pulse-images")
        .join(pc::image_cache_name(url))
}

fn fetch_agent() -> ureq::Agent {
    // No redirects: a public URL can't bounce the fetch onto a private host.
    ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .redirects(0)
        .timeout(std::time::Duration::from_secs(8))
        .build()
}

/// The source page's own preview image URL (`og:image`, else `twitter:image`).
fn source_preview(page: &str) -> Option<String> {
    if !grokhub_core::public_http_url(page) {
        return None;
    }
    // EgressGuard (Spike-4c): the card's source link came from the model, so it is chat.
    crate::xai::egress_ok(page, &[grokhub_agent::harness::DataClass::Chat]).ok()?;
    let resp = fetch_agent().get(page).call().ok()?;
    let html_ok = resp.content_type().to_ascii_lowercase().contains("html");
    if !html_ok {
        return None;
    }
    let mut buf = Vec::new();
    resp.into_reader()
        .take(PAGE_CAP)
        .read_to_end(&mut buf)
        .ok()?;
    pc::og_image(&String::from_utf8_lossy(&buf), page)
}

/// Download one source image into the cache. Only a file that decodes is kept.
fn cache_image(url: &str) -> bool {
    let path = pulse_image_path(url);
    if path.exists() {
        return true;
    }
    if !grokhub_core::public_http_url(url) {
        return false;
    }
    if crate::xai::egress_ok(url, &[grokhub_agent::harness::DataClass::Chat]).is_err() {
        return false;
    }
    let Ok(resp) = fetch_agent().get(url).call() else {
        return false;
    };
    let mut buf = Vec::new();
    if resp
        .into_reader()
        .take(IMAGE_CAP)
        .read_to_end(&mut buf)
        .is_err()
        || image::load_from_memory(&buf).is_err()
    {
        return false;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    crate::config::atomic_write(&path, &buf).is_ok()
}
