//! Pulse: one page, one stream. Ideas are rows (an icon, a bold "I can …"
//! line, a short description, a ··· menu) and feed posts (source, headline,
//! summary, the source's own images, actions) sit in one column, interleaved.
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
/// Keyboard focus: a 2px ring.
pub(super) const FOCUS_RING: egui::Color32 = egui::Color32::from_rgb(0x4a, 0x90, 0xe2);
/// The sign-in line under the header.
const NOTE_AMBER: egui::Color32 = egui::Color32::from_rgb(0xf4, 0xb7, 0x40);
/// Left and right padding of the Pulse page.
const PAGE_PAD_X: f32 = 24.0;
/// The stream never grows wider than this; wider pages center it.
pub(super) const STREAM_MAX_W: f32 = 800.0;
/// Pages narrower than this keep Run on each idea row at rest.
const NARROW_W: f32 = 720.0;
/// Posts narrower than this show two source images, not three.
const POST_THREE_THUMBS_W: f32 = 560.0;
const THUMB_H: f32 = 96.0;
const THUMB_MAX: usize = 3;
/// A source page is read up to this much looking for its preview image.
const PAGE_CAP: u64 = 512 * 1024;
const IMAGE_CAP: u64 = 4 * 1024 * 1024;
/// Re-read the MEMORY.md ledger at most this often while the page is open.
const LEDGER_TTL: std::time::Duration = std::time::Duration::from_secs(5);

/// Pulse's one column at a page width (padding included).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PulseColumn {
    /// The stream's width: the page inside its padding, up to [`STREAM_MAX_W`].
    pub width: f32,
    /// Under 720: idea rows keep Run at rest.
    pub narrow: bool,
}

/// One centered column at every width.
pub(super) fn pulse_column(page_w: f32) -> PulseColumn {
    let inner = (page_w - PAGE_PAD_X * 2.0).max(0.0);
    PulseColumn {
        width: inner.min(STREAM_MAX_W),
        narrow: page_w < NARROW_W,
    }
}

/// One entry in the Pulse stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum StreamItem<I, P> {
    Idea(I),
    Post(P),
}

/// The Pulse stream: an idea, then two posts, then an idea, and so on. Ideas
/// keep their rank order and posts stay newest first; whatever is left of
/// either kind goes at the end in order.
pub(super) fn pulse_stream<I, P>(
    ideas: impl IntoIterator<Item = I>,
    posts: impl IntoIterator<Item = P>,
) -> Vec<StreamItem<I, P>> {
    let mut ideas = ideas.into_iter().peekable();
    let mut posts = posts.into_iter().peekable();
    let mut out = Vec::new();
    while ideas.peek().is_some() || posts.peek().is_some() {
        out.extend(ideas.next().map(StreamItem::Idea));
        out.extend(posts.by_ref().take(2).map(StreamItem::Post));
    }
    out
}

/// The source line under the Pulse header, counted from the same posts the
/// stream paints. With posts showing it counts them, plus what a check is
/// doing right now; a "nothing to check" note never sits over cards. With no
/// posts, only a live note shows here. `live` is whether a source check or
/// summary is still running; a finished one's note waits behind Details.
pub(super) fn feed_header(note: &str, live: bool, shown: usize) -> Option<String> {
    let note = note.trim();
    let busy = live && !note.is_empty() && note != super::pulse_sources_ui::NOTHING_TO_CHECK;
    if shown == 0 {
        return busy.then(|| note.to_string());
    }
    let count = format!(
        "{shown} {} on Pulse",
        if shown == 1 { "post" } else { "posts" }
    );
    Some(if busy {
        format!("{count} · {note}")
    } else {
        count
    })
}

/// The header's Suggest control, and its label while a suggestion runs.
pub(super) fn suggest_label(busy: bool) -> &'static str {
    if busy {
        "Suggesting…"
    } else {
        "Suggest for you"
    }
}

/// Source images a post shows at its width.
pub(super) fn post_thumb_count(post_w: f32) -> usize {
    if post_w < POST_THREE_THUMBS_W {
        2
    } else {
        THUMB_MAX
    }
}

/// What the Pulse page keeps between frames.
#[derive(Default)]
pub(super) struct PulseView {
    /// The page was under 720 wide last frame (set in ui_pulse).
    pub narrow: bool,
    /// The source check note under the header is open (closed by default).
    pub sources_open: bool,
    /// The Feed instructions sheet is open with this draft.
    pub sheet: Option<String>,
    ledger: Vec<LedgerEntry>,
    ledger_at: Option<std::time::Instant>,
    textures: HashMap<String, Option<egui::TextureHandle>>,
    image_rx: Option<mpsc::Receiver<(String, Option<String>)>>,
    image_tried: HashSet<String>,
    pub rewrite_rx: Option<mpsc::Receiver<String>>,
    /// The idea row the keyboard is on (Up/Down or J/K; R, S, D, N, Enter act on it).
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
    /// Narrow page: Run shows at rest, Snooze and Dismiss stay in ···.
    pub compact: bool,
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
    Undo(String),
    NeverAgain(String),
    Retry(String),
    DeleteRecording(String),
    FixIt(String),
    /// A code-idea card's Copy as GitHub issue (card 32 PR D).
    CopyIssue(String),
    RetryNow,
    Source(String, SourceAct),
    /// More or fewer cards of this one's kind, from the ··· menu.
    MoreKind(String),
    LessKind(String),
    /// A backup reminder's one-tap, by chip id.
    BackupChip(String),
    /// Start-my-day brief: run one item's one-tap prompt.
    /// A Start my day one-tap: the brief card's id, the line's prompt and
    /// the card the line came from.
    DayTap(String, String, Option<String>),
    /// Hobby learning-path tip feedback.
    HobbyDone(String),
    HobbySkip(String),
    HobbyTooEasy(String),
    HobbyTooHard(String),
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
/// Automate is amber on the deck (slides 13 / 15); Do stays bright.
pub(super) fn type_color(kind: PulseType) -> egui::Color32 {
    match kind {
        PulseType::Do => egui::Color32::from_rgb(0xe7, 0xe9, 0xea),
        PulseType::Automate => crate::theme::amber(),
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

/// The glyph on a dark tile with a signal-red rim (deck slides 13 / 15).
fn icon(ui: &mut egui::Ui, glyph: Glyph, tint: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ICON, ICON), egui::Sense::hover());
    ui.painter().rect(
        rect,
        crate::ui_kit::recipe::BUTTON_RADIUS,
        crate::theme::void_bg(),
        egui::Stroke::new(crate::ui_kit::recipe::STROKE_W, crate::theme::signal_red()),
        egui::StrokeKind::Inside,
    );
    paint_glyph(ui.painter(), rect.shrink(5.0), glyph, tint);
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
    why: Option<&str>,
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
    let self_review = grokhub_core::self_review::card_target(&card.source_id);
    if !feed && self_review.is_none() {
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
    if let Some(target) = self_review {
        let label = match &target {
            grokhub_core::self_review::CardTarget::Revert(_) => "Revert",
            grokhub_core::self_review::CardTarget::Code(_) => "Save idea",
            _ => "Apply change",
        };
        pick(ui, label, "", PulseAct::Accept(id.clone()), act);
        if matches!(target, grokhub_core::self_review::CardTarget::Code(_)) {
            pick(
                ui,
                "Copy as GitHub issue",
                "",
                PulseAct::CopyIssue(id.clone()),
                act,
            );
        }
    } else if kind == PulseType::Learn {
        pick(
            ui,
            "That's right, keep it",
            "",
            PulseAct::Accept(id.clone()),
            act,
        );
        pick(ui, "That's wrong", "", PulseAct::Wrong(id.clone()), act);
    } else if !feed && card.pulse.proactive.is_none() {
        // A proactive offer has no Always: a hard one never may, and a soft
        // one is a single offer, not a schedule.
        pick(ui, "Always do this", "", PulseAct::Always(id.clone()), act);
    }
    if grokhub_core::is_crash_card(card) && card.prompt.is_some() {
        pick(ui, "Retry", "", PulseAct::Retry(id.clone()), act);
    }
    if grokhub_core::screen_recording_dir(card).is_some() {
        pick(
            ui,
            "Delete recording",
            "",
            PulseAct::DeleteRecording(id.clone()),
            act,
        );
    }
    pick(ui, "Open", "Enter", PulseAct::Open(id.clone()), act);
    ui.separator();
    if grokhub_core::day_brief::is_day_brief_card(&card.source_id) {
        pick(ui, "Not today", "N", PulseAct::NotThis(id.clone()), act);
        pick(ui, "Dismiss", "D", PulseAct::Dismiss(id.clone()), act);
    } else {
        pick(ui, "Not this", "N", PulseAct::NotThis(id.clone()), act);
        pick(ui, "Dismiss", "D", PulseAct::Dismiss(id.clone()), act);
    }
    if let Some(why) = why {
        ui.separator();
        ui.label(
            RichText::new("Why am I seeing this?")
                .size(crate::theme::FONT_TIP)
                .color(crate::theme::muted()),
        );
        ui.label(
            RichText::new(why)
                .size(crate::theme::FONT_TIP)
                .color(crate::theme::subtle()),
        );
        pick(
            ui,
            "More like this",
            "",
            PulseAct::MoreKind(id.clone()),
            act,
        );
        pick(ui, "Less like this", "", PulseAct::LessKind(id), act);
    }
}

/// The inline Run button's word: Send… on a prepared draft (it parks the hard
/// card), Do it on a one-tap suggestion, else Run.
pub(super) fn run_label(card: &UpdateCard) -> &'static str {
    match card.pulse.proactive.as_ref().map(|p| p.route) {
        Some(grokhub_core::proactive::ProactiveRoute::Prepare) => "Send…",
        Some(grokhub_core::proactive::ProactiveRoute::Suggest) => "Do it",
        _ => "Run",
    }
}

/// How an inline row action is drawn: Run is the amber CTA, Snooze is
/// outlined in signal red, Dismiss is quiet text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Quick {
    Primary,
    Outline,
    Quiet,
}

/// One of the row's inline actions, in the Loadout button style.
fn quick_button(ui: &mut egui::Ui, label: &str, style: Quick) -> bool {
    let text = RichText::new(label)
        .size(crate::theme::FONT_TIP)
        .strong()
        .extra_letter_spacing(0.8);
    use crate::ui_kit::recipe::{look, Part, State, BUTTON_RADIUS};
    let button = match style {
        Quick::Primary | Quick::Outline => {
            let state = if style == Quick::Primary {
                State::Accent
            } else {
                State::Default
            };
            let l = look(Part::Button, state);
            egui::Button::new(text.color(l.ink))
                .fill(l.fill)
                .stroke(l.stroke)
        }
        Quick::Quiet => egui::Button::new(text.color(crate::theme::muted())).frame(false),
    };
    ui.add(
        button
            .corner_radius(BUTTON_RADIUS)
            .min_size(egui::vec2(0.0, 22.0)),
    )
    .clicked()
}

/// One Ideas row: icon, bold "I can …" line, two or three lines of detail, the
/// type word, and ···. Hovered or focused, Run · Snooze · Dismiss show inline.
/// On a narrow page Run always shows and Snooze and Dismiss stay in ···.
/// Returns the action, the row's rect, and whether the bold line is hovered.
pub(super) fn paint_pulse_row(
    ui: &mut egui::Ui,
    card: &UpdateCard,
    kind: PulseType,
    st: RowState,
    why: Option<&str>,
) -> (Option<PulseAct>, egui::Rect, bool) {
    let mut act = None;
    let mut title_hover = false;
    let active = st.hovered || st.focused;
    let cat = pc::card_category(card);
    let resp = super::feed_ui::kit_pulse_ideas_row(ui, active, |ui| {
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
                    if active || st.compact {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if !st.compact {
                                if quick_button(ui, "Dismiss", Quick::Quiet) {
                                    act = Some(PulseAct::Dismiss(card.id.clone()));
                                }
                                if quick_button(ui, "Snooze", Quick::Outline) {
                                    act = Some(PulseAct::Snooze(card.id.clone()));
                                }
                            }
                            if quick_button(ui, run_label(card), Quick::Primary) {
                                act = Some(PulseAct::Run(card.id.clone()));
                            }
                        });
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                crate::cards::dots_menu(ui, active, |ui| {
                    pulse_menu(ui, card, kind, false, why, &mut act)
                });
            });
        });
    });
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
    /// Done-for-you Undo (pointer click only).
    Undo,
    /// Done-for-you "Don't do this again" (pointer click only).
    NeverAgain,
    /// Crash card: open the chat the run was in.
    Open,
    /// Crash card: run it again.
    Retry,
    /// Screen recording card: delete its folder (after a confirm).
    DeleteRecording,
    /// Screen or audio report card: run the diagnosis fix plan.
    FixIt(String),
    /// No-route wait card: check the models now.
    RetryNow,
    /// Save / Not interested / More like this on a source post.
    Source(SourceAct),
    HobbyDone,
    HobbySkip,
    HobbyTooEasy,
    HobbyTooHard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SourceAct {
    Save,
    NotInterested,
    MoreLike,
}

/// Open and Retry on a crash card (a run killed from outside, exit 143).
fn paint_crash(ui: &mut egui::Ui, card: &UpdateCard) -> Option<FeedPostAct> {
    if !grokhub_core::is_crash_card(card) || card.status == grokhub_core::UpdateStatus::Dismissed {
        return None;
    }
    let mut act = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if card.action.is_some() && super::change_undo::click_pill(ui, "Open") {
            act = Some(FeedPostAct::Open);
        }
        if card.prompt.is_some() && super::change_undo::click_pill(ui, "Retry") {
            act = Some(FeedPostAct::Retry);
        }
    });
    act
}

/// Retry now on a no-route wait card; the wait also retries on its own.
fn paint_model_wait(ui: &mut egui::Ui, card: &UpdateCard) -> Option<FeedPostAct> {
    if !grokhub_core::is_model_wait_card(card)
        || card.status == grokhub_core::UpdateStatus::Dismissed
    {
        return None;
    }
    super::change_undo::click_pill(ui, "Retry now").then_some(FeedPostAct::RetryNow)
}

/// Open, Fix it, and Delete recording on a screen or audio report card.
fn paint_recording(ui: &mut egui::Ui, card: &UpdateCard) -> Option<FeedPostAct> {
    super::fix_it::paint_diagnosis_report_actions(ui, card)
}

/// The Done-for-you pills, while the card is unanswered.
fn paint_done_for_you(ui: &mut egui::Ui, card: &UpdateCard) -> Option<FeedPostAct> {
    let open = card.kind == UpdateKind::DoneForYou
        && card.done_for_you.as_ref().is_some_and(|d| !d.answered);
    if !open {
        return None;
    }
    let mut act = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if super::change_undo::click_pill(ui, "Undo") {
            act = Some(FeedPostAct::Undo);
        }
        if super::change_undo::click_pill(ui, "Don't do this again") {
            act = Some(FeedPostAct::NeverAgain);
        }
    });
    act
}

/// Done / Skip / Too easy / Too hard on an unanswered hobby learning-path tip.
fn paint_hobby_path(ui: &mut egui::Ui, card: &UpdateCard) -> Option<FeedPostAct> {
    let open = card.pulse.hobby_path.as_ref().is_some_and(|m| !m.answered);
    if !open {
        return None;
    }
    let mut act = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if super::change_undo::click_pill(ui, "Done") {
            act = Some(FeedPostAct::HobbyDone);
        }
        if super::change_undo::click_pill(ui, "Skip") {
            act = Some(FeedPostAct::HobbySkip);
        }
        if super::change_undo::click_pill(ui, "Too easy") {
            act = Some(FeedPostAct::HobbyTooEasy);
        }
        if super::change_undo::click_pill(ui, "Too hard") {
            act = Some(FeedPostAct::HobbyTooHard);
        }
    });
    act
}

/// Title, short takeaway, Read at, images, then Like / Discuss.
/// `thumbs` is empty when the post has no image yet.
/// `looking_more` is true while a More like this check is in flight.
pub(super) fn paint_post_body(
    ui: &mut egui::Ui,
    card: &UpdateCard,
    thumbs: &[Thumb],
    looking_more: bool,
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
    if let Some(topic) = card.pulse.because.as_deref() {
        ui.label(
            RichText::new(why_line(topic))
                .size(crate::theme::FONT_TIP)
                .color(crate::theme::subtle()),
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
        if grokhub_core::pulse_sources::is_source_post(card) {
            if let Some(did) = paint_source_actions(ui, card, looking_more) {
                act = Some(FeedPostAct::Source(did));
            }
        } else if paint_heart_button(ui, card.reaction == Some(CardReaction::Up)) {
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
    if let Some(done) = paint_done_for_you(ui, card) {
        act = Some(done);
    }
    if let Some(hobby) = paint_hobby_path(ui, card) {
        act = Some(hobby);
    }
    if let Some(crash) = paint_crash(ui, card) {
        act = Some(crash);
    }
    if grokhub_core::wind_down::is_wind_down_card(card)
        && card.status != grokhub_core::UpdateStatus::Dismissed
        && super::change_undo::click_pill(ui, "Sleep the PC")
    {
        act = Some(FeedPostAct::Open);
    }
    if grokhub_core::local_model_news::is_upgrade_card(card)
        && card.status != grokhub_core::UpdateStatus::Dismissed
        && super::change_undo::click_pill(ui, "Open in Settings")
    {
        act = Some(FeedPostAct::Open);
    }
    if super::fix_it::is_diagnosis_report_card(card) {
        if let Some(rec) = paint_recording(ui, card) {
            act = Some(rec);
        }
    }
    if let Some(wait) = paint_model_wait(ui, card) {
        act = Some(wait);
    }
    act
}

/// One-tap buttons under a Start my day Digest.
fn paint_day_brief_taps(
    ui: &mut egui::Ui,
    taps: &[(String, String, Option<String>)],
) -> Option<(String, Option<String>)> {
    if taps.is_empty() {
        return None;
    }
    let mut picked = None;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (label, prompt, source) in taps.iter().take(grokhub_core::day_brief::ITEM_CAP) {
            let btn = ui.add(
                egui::Button::new(
                    RichText::new(label)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::fg()),
                )
                .frame(false)
                .min_size(egui::vec2(0.0, 22.0)),
            );
            if btn.clicked() {
                picked = Some((prompt.clone(), source.clone()));
            }
        }
    });
    picked
}

/// Save, Not interested and More like this under a source post. Save reads Saved
/// once the post is in Saved Finds. More like this reads Looking… while a check
/// it started is still running.
fn paint_source_actions(
    ui: &mut egui::Ui,
    card: &UpdateCard,
    looking_more: bool,
) -> Option<SourceAct> {
    let saved = card.reaction == Some(CardReaction::Up);
    let mut act = None;
    for (label, did) in [
        (if saved { "Saved" } else { "Save" }, SourceAct::Save),
        ("Not interested", SourceAct::NotInterested),
        (
            if looking_more {
                "Looking…"
            } else {
                "More like this"
            },
            SourceAct::MoreLike,
        ),
    ] {
        let color =
            if (did == SourceAct::Save && saved) || (did == SourceAct::MoreLike && looking_more) {
                crate::theme::fg()
            } else {
                crate::theme::muted()
            };
        let button = ui.add(
            egui::Button::new(
                RichText::new(label)
                    .size(crate::theme::FONT_TIP)
                    .color(color),
            )
            .frame(false)
            .min_size(egui::vec2(0.0, 24.0)),
        );
        if button.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if button.clicked() {
            act = Some(did);
        }
    }
    act
}

/// "Why: air compressors", under a source post's summary.
pub(super) fn why_line(topic: &str) -> String {
    format!("Why: {}", topic.trim())
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
    if grokhub_core::is_crash_card(card) {
        return "Crash".into();
    }
    if grokhub_core::screen_recording_dir(card).is_some() {
        return "Screen recording".into();
    }
    match card.kind {
        UpdateKind::AutomationDone => "Automation run".into(),
        UpdateKind::ScheduleCreated => "Automations".into(),
        UpdateKind::Digest => "Daily digest".into(),
        _ => grokhub_core::brand::product_name().into(),
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
    /// The Pulse page: the header, then one centered column where ideas and
    /// feed posts share a single stream at every width.
    pub(super) fn ui_pulse(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll_pulse_images();
        let frame = egui::Frame::NONE
            .fill(crate::theme::bg())
            .inner_margin(egui::Margin::symmetric(PAGE_PAD_X as i8, 20));
        let _ = super::dock_ui::page_panel(ui, self.cfg.dock_panels, frame, |ui| {
            let col = pulse_column(ui.available_width() + PAGE_PAD_X * 2.0);
            self.pulse_view.narrow = col.narrow;
            egui::ScrollArea::vertical()
                .id_salt("pulse-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let full = ui.available_width();
                    let width = full.min(col.width);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.add_space(((full - width) * 0.5).max(0.0));
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            self.paint_pulse_header(ui);
                            self.ui_pulse_stream(ui);
                            ui.add_space(24.0);
                        });
                    });
                });
        });
        self.paint_feed_instructions_sheet(&ctx);
    }

    /// The title row with its two actions, then the sign-in note, the brief
    /// and helper offers, and the source line.
    fn paint_pulse_header(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(pc::PULSE_TITLE)
                    .font(crate::theme::title_font(28.0))
                    .color(crate::theme::fg()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if crate::cards::ghost_pill(ui, "Feed instructions") {
                    self.open_feed_instructions();
                }
                let busy = self.ideas_rx.is_some();
                if crate::cards::ghost_pill(ui, suggest_label(busy)) && !busy {
                    self.suggest_ideas_pressed();
                }
            });
        });
        ui.label(
            RichText::new(*pc::PULSE_SUBTITLE)
                .size(crate::theme::FONT_BODY)
                .color(crate::theme::muted()),
        );
        // PI-05: clear the signed-out note once GrokHub's own sign-in or key is there.
        if self.pulse_view.signin_note && self.agent_ready() {
            self.pulse_view.signin_note = false;
        }
        if self.pulse_view.signin_note {
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
        self.paint_brief_offer(ui);
        self.ui_helper_offers(ui);
        ui.add_space(14.0);
    }

    /// One subtle line under the header: how many posts are on Pulse, with
    /// the last source check note behind Details until you open it.
    fn paint_source_status(&mut self, ui: &mut egui::Ui, shown: usize) {
        let note = self.sources.note.trim().to_string();
        let line = feed_header(&note, self.sources_busy(), shown);
        let more = !note.is_empty() && !line.as_deref().is_some_and(|l| l.ends_with(&note));
        if line.is_none() && !more {
            return;
        }
        let open = more && self.pulse_view.sources_open;
        ui.horizontal(|ui| {
            if let Some(line) = &line {
                ui.label(
                    RichText::new(line)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::subtle()),
                );
            }
            if more {
                let toggle = ui.add(
                    egui::Label::new(
                        RichText::new(if open { "Hide details" } else { "Details" })
                            .size(crate::theme::FONT_TIP)
                            .underline()
                            .color(crate::theme::subtle()),
                    )
                    .sense(egui::Sense::click())
                    .selectable(false),
                );
                if toggle.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if toggle.clicked() {
                    self.pulse_view.sources_open = !open;
                }
            }
        });
        if open {
            ui.add(
                egui::Label::new(
                    RichText::new(&note)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::subtle()),
                )
                .wrap(),
            );
        }
        ui.add_space(8.0);
    }

    /// Pulse's one list: ideas in rank order and posts newest first, mixed by
    /// `pulse_stream`. Loading ideas show placeholder rows up top; with
    /// nothing of either kind there is one empty state.
    pub(super) fn ui_pulse_stream(&mut self, ui: &mut egui::Ui) {
        let now = now_ms();
        self.ensure_useful_ideas();
        let ideas: Vec<Ranked> = self
            .pulse_groups(now)
            .into_iter()
            .flat_map(|(_, rows)| rows)
            .collect();
        let posts = self.pulse_feed_posts(now);
        self.kick_pulse_images(&posts);
        self.paint_source_status(ui, posts.len());
        if ideas.is_empty() && (self.ideas_rx.is_some() || posts.is_empty()) {
            self.paint_pulse_silence(ui);
            if posts.is_empty() {
                return;
            }
        }
        let rows: Vec<String> = ideas.iter().map(|r| r.id.clone()).collect();
        let mut pass = self.begin_ideas_pass(ui, &rows);
        let mut post_act = None;
        for item in pulse_stream(&ideas, &posts) {
            match item {
                StreamItem::Idea(row) => self.paint_idea_slot(ui, row, &mut pass),
                StreamItem::Post(card) => {
                    if let Some(a) = self.paint_pulse_post(ui, card, now) {
                        post_act = Some(a);
                    }
                    ui.add_space(crate::theme::SPACING_8);
                }
            }
        }
        self.finish_ideas_pass(ui, pass);
        if let Some(a) = post_act {
            self.apply_pulse_act(a);
        }
    }

    /// Nothing to show: placeholder rows while ideas are being found, else
    /// one empty line for the page and when the heartbeat last ran.
    pub(super) fn paint_pulse_silence(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        if self.ideas_rx.is_some() {
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
            // Poll the ideas channel only while a suggest is in flight.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        ui.label(
            RichText::new(pc::PULSE_EMPTY)
                .size(crate::theme::FONT_BODY)
                .color(crate::theme::muted()),
        );
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

    // ranking inputs

    fn pulse_ledger(&mut self) -> Vec<LedgerEntry> {
        let stale = self
            .pulse_view
            .ledger_at
            .is_none_or(|at| at.elapsed() >= LEDGER_TTL);
        if stale {
            self.pulse_view.ledger = pc::parse_ledger(&memory_tidy::read_pulse_ledger());
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
        let reactions = self.reactions_now().clone();
        let inputs = pc::PulseInputs {
            ledger: &ledger,
            skills: &skills,
            open_work: &work,
            now_ms: now,
            reactions: Some(&reactions),
        };
        pc::rank_pulse(&self.updates, &inputs)
    }

    /// Ideas by rank: the top group, then one group per category in order.
    pub(super) fn pulse_groups(&mut self, now: u64) -> Vec<(Option<PulseCategory>, Vec<Ranked>)> {
        let ledger = self.pulse_ledger();
        let skills = self.pulse_skills();
        let work = self.pulse_open_work();
        let reactions = self.reactions_now().clone();
        let inputs = pc::PulseInputs {
            ledger: &ledger,
            skills: &skills,
            open_work: &work,
            now_ms: now,
            reactions: Some(&reactions),
        };
        pc::group_ideas(&self.updates, &inputs)
    }

    /// Feed posts, newest first; muted sources stay off.
    pub(super) fn pulse_feed_posts(&mut self, now: u64) -> Vec<UpdateCard> {
        let ledger = self.pulse_ledger();
        let reactions = self.reactions_now().clone();
        let inputs = pc::PulseInputs {
            ledger: &ledger,
            skills: &[],
            open_work: &[],
            now_ms: now,
            reactions: Some(&reactions),
        };
        let muted = &self.cfg.feed_pulse.muted_sources;
        let mut posts: Vec<UpdateCard> = pc::feed_posts(&self.updates, &inputs)
            .into_iter()
            .filter(|c| c.source_id.is_empty() || !muted.iter().any(|m| m == &c.source_id))
            .filter(|c| !self.steam_muted(c))
            .collect();
        if self.cfg.daily_drop.active() {
            use grokhub_core::daily_drop::{day_index, is_daily_drop_card};
            use grokhub_core::pulse::{pulse_visible, PULSE_THRESHOLD};
            let today = day_index(now);
            for card in &self.updates {
                if !is_daily_drop_card(card, &self.cfg.daily_drop)
                    || day_index(card.created_at) != today
                    || !pulse_visible(card, now)
                {
                    continue;
                }
                if card.source_id.is_empty() || muted.iter().any(|m| m == &card.source_id) {
                    continue;
                }
                if pc::pulse_score(card, &inputs) >= PULSE_THRESHOLD {
                    continue;
                }
                if posts.iter().any(|p| p.id == card.id) {
                    continue;
                }
                posts.push(card.clone());
            }
            posts.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        }
        posts
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
        let daily_drop = grokhub_core::daily_drop::is_daily_drop_card(card, &self.cfg.daily_drop);
        let why = self.reaction_why(card);
        let thumbs: Vec<Thumb> = card
            .pulse
            .image_urls
            .iter()
            .take(post_thumb_count(ui.available_width()))
            .filter_map(|url| self.pulse_thumb(ui.ctx(), url))
            .collect();
        super::feed_ui::kit_pulse_feed_post(ui, |ui| {
            ui.horizontal_top(|ui| {
                paint_feed_icon(ui, card);
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal(|ui| {
                        if daily_drop {
                            super::feed_ui::kit_daily_drop_badge(ui);
                            ui.add_space(crate::theme::SPACING_4);
                        }
                        ui.label(
                            RichText::new(post_age_line(card, now))
                                .size(crate::theme::FONT_TIP)
                                .color(crate::theme::subtle()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            crate::cards::dots_menu(ui, false, |ui| {
                                pulse_menu(ui, card, kind, true, why.as_deref(), &mut act)
                            });
                        });
                    });
                    if let Some(did) = paint_post_body(ui, card, &thumbs, self.sources.looking_more)
                    {
                        act = Some(match did {
                            FeedPostAct::Like => PulseAct::Like(card.id.clone()),
                            FeedPostAct::Discuss => PulseAct::Discuss(card.id.clone()),
                            FeedPostAct::Link(url) => PulseAct::Link(url),
                            FeedPostAct::Undo => PulseAct::Undo(card.id.clone()),
                            FeedPostAct::NeverAgain => PulseAct::NeverAgain(card.id.clone()),
                            FeedPostAct::Open => PulseAct::Open(card.id.clone()),
                            FeedPostAct::Retry => PulseAct::Retry(card.id.clone()),
                            FeedPostAct::DeleteRecording => {
                                PulseAct::DeleteRecording(card.id.clone())
                            }
                            FeedPostAct::FixIt(id) => PulseAct::FixIt(id),
                            FeedPostAct::RetryNow => PulseAct::RetryNow,
                            FeedPostAct::Source(did) => PulseAct::Source(card.id.clone(), did),
                            FeedPostAct::HobbyDone => PulseAct::HobbyDone(card.id.clone()),
                            FeedPostAct::HobbySkip => PulseAct::HobbySkip(card.id.clone()),
                            FeedPostAct::HobbyTooEasy => PulseAct::HobbyTooEasy(card.id.clone()),
                            FeedPostAct::HobbyTooHard => PulseAct::HobbyTooHard(card.id.clone()),
                        });
                    }
                    if let Some(id) = self.paint_backup_one_taps(ui, card) {
                        act = Some(PulseAct::BackupChip(id));
                    }
                    if grokhub_core::day_brief::is_day_brief_card(&card.source_id) {
                        if let Some((tap, source)) =
                            paint_day_brief_taps(ui, &self.day_brief_taps())
                        {
                            act = Some(PulseAct::DayTap(card.id.clone(), tap, source));
                        }
                    }
                });
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
        // Decode finished with failure: drop the slot (same as a bad download).
        if self
            .pulse_view
            .textures
            .get(&name)
            .is_some_and(|t| t.is_none())
        {
            self.pulse_view.image_failed.insert(name);
            return None;
        }
        // Still downloading, or on disk and decoding off-thread.
        Some(Thumb::Loading)
    }

    #[cfg(test)]
    pub(super) fn pulse_image_failed(&self, url: &str) -> bool {
        self.pulse_view
            .image_failed
            .contains(&pc::image_cache_name(url))
    }

    // real images

    #[cfg(test)]
    pub(super) fn pulse_view_texture_size(&self, url: &str) -> Option<[usize; 2]> {
        self.pulse_view
            .textures
            .get(&pc::image_cache_name(url))
            .and_then(|t| t.as_ref().map(|t| t.size()))
    }

    /// Upload a ready CPU thumb into egui, or kick an off-thread read+decode.
    /// The paint path never calls `fs::read` or `load_from_memory`.
    fn pulse_texture(&mut self, ctx: &egui::Context, url: &str) -> Option<egui::TextureHandle> {
        let name = pc::image_cache_name(url);
        if let Some(tex) = self.pulse_view.textures.get(&name) {
            return tex.clone();
        }
        if let Some(ready) = take_pulse_rgba(&name) {
            let tex = ready.map(|thumb| {
                let size = [thumb.width() as usize, thumb.height() as usize];
                ctx.load_texture(
                    format!("pulse-img-{name}"),
                    egui::ColorImage::from_rgba_unmultiplied(size, thumb.as_raw()),
                    egui::TextureOptions::LINEAR,
                )
            });
            // None stays cached so a bad file is not decoded again.
            self.pulse_view.textures.insert(name, tex.clone());
            return tex;
        }
        let path = pulse_image_path(url);
        if path.exists() {
            kick_pulse_decode(ctx.clone(), name, path);
        }
        None
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

    // keyboard

    /// One key on the idea rows. Up/Down (or J/K) move the focused row; R runs
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
            compact: self.pulse_view.narrow,
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

    // buttons

    pub(super) fn apply_pulse_act(&mut self, act: PulseAct) {
        self.react_to_act(&act);
        match act {
            PulseAct::Run(id) => {
                self.pulse_run(&id);
            }
            PulseAct::Snooze(id) => {
                let clock = Self::local_clock();
                self.pulse_snooze_at(&id, clock.now_ms, clock.hour, clock.minute);
            }
            PulseAct::Dismiss(id) => {
                if self
                    .pulse_card(&id)
                    .is_some_and(|c| grokhub_core::day_brief::is_day_brief_card(&c.source_id))
                {
                    self.day_brief_not_today(&id);
                } else {
                    self.pulse_dismiss(&id, &Self::local_day());
                }
            }
            PulseAct::NotThis(id) => {
                if self
                    .pulse_card(&id)
                    .is_some_and(|c| grokhub_core::day_brief::is_day_brief_card(&c.source_id))
                {
                    self.day_brief_not_today(&id);
                } else {
                    self.pulse_not_this(&id, &Self::local_day());
                }
            }
            PulseAct::DayTap(id, prompt, source) => self.day_brief_tap(&id, prompt, source),
            PulseAct::Always(id) => self.pulse_always(&id, &Self::local_day()),
            PulseAct::Accept(id) => self.pulse_accept(&id, &Self::local_day()),
            PulseAct::Wrong(id) => self.pulse_wrong(&id, &Self::local_day()),
            PulseAct::Like(id) => self.pulse_like(&id, &Self::local_day()),
            PulseAct::Open(id) => self.pulse_open(&id),
            PulseAct::Discuss(id) => self.discuss_card(&id),
            PulseAct::Undo(id) => self.done_for_you_undo(&id),
            PulseAct::NeverAgain(id) => self.done_for_you_never(&id),
            PulseAct::Retry(id) => self.crash_retry(&id),
            PulseAct::DeleteRecording(id) => self.arm_delete_recording(&id),
            PulseAct::FixIt(id) => self.tap_diagnosis_fix(&id),
            PulseAct::CopyIssue(id) => self.copy_code_idea(&id),
            PulseAct::RetryNow => self.retry_model_wait_now(),
            PulseAct::BackupChip(id) => self.tap_backup_chip(&id),
            PulseAct::Source(id, did) => self.source_post_act(&id, did),
            // Recorded by react_to_act; nothing else to do.
            PulseAct::MoreKind(_) | PulseAct::LessKind(_) => {}
            PulseAct::Link(url) => {
                self.follow_update_action(Some(UpdateAction::DeepLink { href: url }))
            }
            PulseAct::HobbyDone(id) => {
                self.hobby_path_feedback(&id, grokhub_core::PathFeedback::Done)
            }
            PulseAct::HobbySkip(id) => {
                self.hobby_path_feedback(&id, grokhub_core::PathFeedback::Skip)
            }
            PulseAct::HobbyTooEasy(id) => {
                self.hobby_path_feedback(&id, grokhub_core::PathFeedback::TooEasy)
            }
            PulseAct::HobbyTooHard(id) => {
                self.hobby_path_feedback(&id, grokhub_core::PathFeedback::TooHard)
            }
        }
    }

    /// One structured line in the Pulse ledger (`memory/pulse-ledger.md`).
    /// Likes and dislikes count toward the next rewrite of the feed instructions.
    pub(super) fn pulse_note(&mut self, title: &str, reason: LedgerReason, day: &str) {
        let line = pc::ledger_line(day, title, reason);
        if let Err(err) = memory_tidy::append_pulse_ledger(&line) {
            self.status = err;
        }
        self.pulse_view.ledger_at = None;
        if let Some(signal) = interests_ui::ledger_signal(reason) {
            self.interests_feedback(title, signal);
        }
        if reason.is_taste() {
            self.pulse_taste_tick();
        }
    }

    pub(super) fn pulse_card(&self, id: &str) -> Option<UpdateCard> {
        self.updates.iter().find(|c| c.id == id).cloned()
    }

    /// Run: the idea's own action as a `/bg` task. The composer stays free.
    pub(super) fn pulse_run(&mut self, id: &str) -> Option<String> {
        if self.pulse_card(id)?.pulse.proactive.is_some() {
            self.proactive_click(id);
            return None;
        }
        if self.pulse_card(id)?.pulse.print_alert.is_some() {
            self.print_alert_run(id);
            return None;
        }
        self.pulse_run_line(id)
    }

    /// The `/bg` line itself, once any gate said yes.
    pub(super) fn pulse_run_line(&mut self, id: &str) -> Option<String> {
        let card = self.pulse_card(id)?;
        // A self-review card is a skill change, not a task: only Apply acts on it.
        if grokhub_core::self_review::card_target(&card.source_id).is_some() {
            self.status = "Use Apply change in the card's menu.".into();
            return None;
        }
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

    pub(super) fn pulse_remove(&mut self, card: &UpdateCard) {
        // Not-this / wrong: treat as turned down.
        self.pulse_remove_dismiss(card, true);
    }

    pub(super) fn pulse_dismiss(&mut self, id: &str, day: &str) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        let used_depth = grokhub_core::used_depth(&self.card_prefs, &card);
        let kind = pc::dismiss_kind(&card, used_depth);
        self.pulse_note(&card.title, kind.ledger_reason(), day);
        if kind.counts_negative() {
            self.nudge_watcher_dismissed(&card);
            self.ask_offer_dismissed(&card);
            self.heartbeat_card_dismissed();
            self.proactive_dismissed(&card);
            self.pulse_remove_dismiss(&card, true);
        } else {
            self.pulse_remove_dismiss(&card, false);
            self.status = "Done.".into();
        }
    }

    /// Remove a Pulse card after dismiss. `turned_down` writes negative topic
    /// memory; used-then-dismissed cards skip that.
    pub(super) fn pulse_remove_dismiss(&mut self, card: &UpdateCard, turned_down: bool) {
        if card.kind == UpdateKind::Idea {
            self.delete_idea_with_feedback(&card.id, turned_down);
        } else {
            self.dismiss_feed_card(&card.id);
        }
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
        self.proactive_not_this(&card);
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
        if self.apply_self_review_card(id) {
            return;
        }
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

    // feed instructions

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
        let ledger = pc::parse_ledger(&memory_tidy::read_pulse_ledger());
        let taste = pc::recent_taste(&ledger, pc::REWRITE_AFTER as usize);
        if taste.is_empty() {
            return;
        }
        let prompt = pc::rewrite_prompt(&self.cfg.feed_instructions, &taste);
        let (tx, rx) = mpsc::channel();
        self.pulse_view.rewrite_rx = Some(rx);
        self.spawn_native_ideas(prompt, tx, None);
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

    // migration

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

/// Off-thread Pulse thumb decode (same shape as Eyes / Imagine stills).
struct PulseTexGate {
    inflight: HashSet<String>,
    /// `None` means the file did not decode; do not retry.
    ready: HashMap<String, Option<image::RgbaImage>>,
}

fn pulse_tex_gate() -> &'static Mutex<PulseTexGate> {
    static G: OnceLock<Mutex<PulseTexGate>> = OnceLock::new();
    G.get_or_init(|| {
        Mutex::new(PulseTexGate {
            inflight: HashSet::new(),
            ready: HashMap::new(),
        })
    })
}

fn take_pulse_rgba(name: &str) -> Option<Option<image::RgbaImage>> {
    let mut g = pulse_tex_gate().lock().ok()?;
    g.ready.remove(name)
}

fn kick_pulse_decode(ctx: egui::Context, name: String, path: std::path::PathBuf) {
    {
        let Ok(mut g) = pulse_tex_gate().lock() else {
            return;
        };
        if g.ready.contains_key(&name) || !g.inflight.insert(name.clone()) {
            return;
        }
    }
    std::thread::spawn(move || {
        let decoded = std::fs::read(&path)
            .ok()
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .map(|img| {
                let img = if img.width() > 480 || img.height() > 480 {
                    img.thumbnail(480, 480)
                } else {
                    img
                };
                img.to_rgba8()
            });
        if let Ok(mut g) = pulse_tex_gate().lock() {
            g.inflight.remove(&name);
            g.ready.insert(name, decoded);
        }
        ctx.request_repaint();
    });
}

/// Tests: block until no thumb decode is in flight, so the next pass uploads it.
#[cfg(test)]
pub(super) fn wait_pulse_decodes() {
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(10) {
        if pulse_tex_gate().lock().is_ok_and(|g| g.inflight.is_empty()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn fetch_agent() -> grokhub_core::http::Agent {
    // No redirects: a public URL can't bounce the fetch onto a private host.
    // `public_only` dials through public_only_resolve, which blocks DNS→private/CGNAT
    // even when the hostname looked public. The facade reads no env proxy:
    // HTTP(S)_PROXY would dial the proxy and skip that check on the target.
    grokhub_core::http::agent(grokhub_core::http::Opts {
        redirects: 0,
        overall: Some(std::time::Duration::from_secs(8)),
        public_only: true,
        ..Default::default()
    })
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

#[cfg(test)]
mod fetch_agent_tests {
    #[test]
    fn fetch_agent_dials_through_the_public_only_resolver() {
        let src = include_str!("pulse_ui.rs");
        let arm = src
            .split("fn fetch_agent()")
            .nth(1)
            .and_then(|rest| rest.split("fn source_preview(").next())
            .expect("fetch_agent");
        // `public_only` dials through public_only_resolve (grokhub_core::http tests).
        assert!(
            arm.contains("grokhub_core::http::agent(") && arm.contains("public_only: true"),
            "Pulse page/image fetches must refuse DNS→private: {arm}"
        );
        assert!(
            arm.contains("redirects: 0,"),
            "Pulse image agent must not auto-follow redirects: {arm}"
        );
        assert!(
            !arm.contains("try_proxy_from_env"),
            "env proxy would skip public_only_resolve on the target: {arm}"
        );
    }
}

#[cfg(test)]
mod pulse_tex_tests {
    use super::*;

    #[test]
    fn pulse_texture_decodes_off_the_ui_thread() {
        let src = include_str!("pulse_ui.rs");
        let tex = src
            .split("fn pulse_texture(")
            .nth(1)
            .and_then(|s| s.split("/// One post at a time").next())
            .expect("pulse_texture");
        assert!(
            !tex.contains("load_from_memory") && !tex.contains("std::fs::read"),
            "Feed first paint must not read or decode thumbs on the UI thread: {tex}"
        );
        assert!(
            tex.contains("take_pulse_rgba") && tex.contains("kick_pulse_decode"),
            "pulse_texture must hand off to the decode gate: {tex}"
        );
        let kick = src
            .split("fn kick_pulse_decode(")
            .nth(1)
            .and_then(|s| s.split("fn fetch_agent(").next())
            .expect("kick_pulse_decode");
        let spawn = kick
            .find("thread::spawn")
            .expect("decode must leave the UI thread");
        let decode = kick.find("load_from_memory").expect("JPEG/PNG decode");
        let read = kick.find("std::fs::read").expect("disk read");
        assert!(
            spawn < read && spawn < decode && kick.contains("inflight"),
            "Pulse cache thumbs must not decode on the UI thread: {kick}"
        );
    }

    #[test]
    fn pulse_thumb_loads_a_cached_file_after_off_thread_decode() {
        let root = std::env::temp_dir().join(format!(
            "grokhub-pulse-tex-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let prev = std::env::var_os("GROKHUB_CONFIG");
        std::env::set_var("GROKHUB_CONFIG", &root);

        let url = "https://example.com/pulse-opt-thumb.png";
        let path = pulse_image_path(url);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::RgbaImage::from_pixel(12, 8, image::Rgba([40, 120, 200, 255]))
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();

        let mut cabin = Cabin::quiet_for_test();
        let ctx = egui::Context::default();
        assert!(
            matches!(cabin.pulse_thumb(&ctx, url), Some(Thumb::Loading)),
            "first paint shows Loading while decode runs off-thread"
        );

        let mut ready = false;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            match cabin.pulse_thumb(&ctx, url) {
                Some(Thumb::Ready(_)) => {
                    ready = true;
                    break;
                }
                Some(Thumb::Loading) => {}
                None => panic!("valid PNG must not mark the thumb failed"),
            }
        }
        assert!(ready, "off-thread decode must yield a Ready thumb");
        assert_eq!(cabin.pulse_view_texture_size(url), Some([12, 8]));

        match prev {
            Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
            None => std::env::remove_var("GROKHUB_CONFIG"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pulse_thumb_drops_a_file_that_does_not_decode() {
        let root = std::env::temp_dir().join(format!(
            "grokhub-pulse-bad-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let prev = std::env::var_os("GROKHUB_CONFIG");
        std::env::set_var("GROKHUB_CONFIG", &root);

        let url = "https://example.com/pulse-bad-thumb.bin";
        let path = pulse_image_path(url);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not-an-image").unwrap();

        let mut cabin = Cabin::quiet_for_test();
        let ctx = egui::Context::default();
        assert!(matches!(cabin.pulse_thumb(&ctx, url), Some(Thumb::Loading)));

        let mut failed = false;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            match cabin.pulse_thumb(&ctx, url) {
                None => {
                    failed = true;
                    break;
                }
                Some(Thumb::Loading) => {}
                Some(Thumb::Ready(_)) => panic!("garbage bytes must not upload a texture"),
            }
        }
        assert!(failed, "bad cache file must drop the thumb slot");
        assert!(cabin.pulse_image_failed(url));

        match prev {
            Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
            None => std::env::remove_var("GROKHUB_CONFIG"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod feed_header_tests {
    use super::*;

    #[test]
    fn the_feed_header_counts_the_posts_it_shows() {
        let nothing = "Nothing to check yet. Add an interest or a source.";
        // Cards on screen: counted, never "nothing to check".
        assert_eq!(
            feed_header(nothing, false, 3).as_deref(),
            Some("3 posts on Pulse")
        );
        assert_eq!(
            feed_header("", false, 1).as_deref(),
            Some("1 post on Pulse")
        );
        // A live check rides along with the count.
        assert_eq!(
            feed_header("Checking 4 sources…", true, 2).as_deref(),
            Some("2 posts on Pulse · Checking 4 sources…")
        );
        // Empty Pulse: the empty state says so; only a live note shows up here.
        assert_eq!(feed_header(nothing, false, 0), None);
        assert_eq!(feed_header("", false, 0), None);
        assert_eq!(
            feed_header("Checking 4 sources…", true, 0).as_deref(),
            Some("Checking 4 sources…")
        );
        // A finished run's result or error stays on the Sources panel, not here.
        let done = "Checked 4 sources. 2 new.";
        assert_eq!(
            feed_header(done, false, 2).as_deref(),
            Some("2 posts on Pulse")
        );
        assert_eq!(feed_header(done, false, 0), None);
    }
}

#[cfg(test)]
mod stream_tests {
    use super::{pulse_stream, StreamItem};

    fn ids(stream: Vec<StreamItem<&str, &str>>) -> Vec<String> {
        stream
            .into_iter()
            .map(|item| match item {
                StreamItem::Idea(id) => format!("i:{id}"),
                StreamItem::Post(id) => format!("p:{id}"),
            })
            .collect()
    }

    #[test]
    fn the_stream_starts_with_an_idea_then_two_posts_and_so_on() {
        assert_eq!(
            ids(pulse_stream(
                ["a", "b", "c"],
                ["1", "2", "3", "4", "5", "6"]
            )),
            ["i:a", "p:1", "p:2", "i:b", "p:3", "p:4", "i:c", "p:5", "p:6"]
        );
        // Ideas run out: the posts that are left follow in order.
        assert_eq!(
            ids(pulse_stream(["a"], ["1", "2", "3", "4"])),
            ["i:a", "p:1", "p:2", "p:3", "p:4"]
        );
        // Posts run out: the ideas that are left follow in order.
        assert_eq!(
            ids(pulse_stream(["a", "b", "c"], ["1"])),
            ["i:a", "p:1", "i:b", "i:c"]
        );
        assert_eq!(ids(pulse_stream([], ["1", "2"])), ["p:1", "p:2"]);
        assert_eq!(ids(pulse_stream(["a", "b"], [])), ["i:a", "i:b"]);
        assert!(pulse_stream::<&str, &str>([], []).is_empty());
    }
}
