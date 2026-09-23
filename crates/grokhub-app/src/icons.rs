//! Painted catalog icons. Unicode glyphs miss in the default cabin font.

use eframe::egui::{self, Pos2, Sense, Stroke, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileIcon {
    Sun,
    Host,
    List,
    Image,
    Github,
    Check,
    Board,
    Bolt,
    Moon,
    Connect,
    Think,
    Help,
    Chat,
}

pub fn paint_icon(ui: &mut egui::Ui, icon: TileIcon, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let fill = crate::theme::elevated();
    let stroke = Stroke::new(crate::theme::ICON_STROKE, crate::theme::fg());
    painter.rect_filled(rect, 6.0, fill);
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, crate::theme::border()));
    let r = rect.shrink(size * 0.22);
    let c = r.center();
    let w = r.width();
    match icon {
        TileIcon::Sun => {
            painter.circle_stroke(c, w * 0.18, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let inner = w * 0.28;
                let outer = w * 0.46;
                painter.line_segment(
                    [
                        c + Vec2::new(a.cos() * inner, a.sin() * inner),
                        c + Vec2::new(a.cos() * outer, a.sin() * outer),
                    ],
                    stroke,
                );
            }
        }
        TileIcon::Host => {
            painter.rect_stroke(r, 3.0, stroke);
            let p = Pos2::new(r.left() + 5.0, r.center().y);
            painter.line_segment([p, Pos2::new(p.x + 5.0, p.y + 4.0)], stroke);
            painter.line_segment([p, Pos2::new(p.x + 5.0, p.y - 4.0)], stroke);
            painter.line_segment(
                [Pos2::new(p.x + 8.0, r.bottom() - 6.0), Pos2::new(r.right() - 6.0, r.bottom() - 6.0)],
                stroke,
            );
        }
        TileIcon::List => {
            for i in 0..3 {
                let y = r.top() + 5.0 + i as f32 * (w * 0.28);
                painter.circle_filled(Pos2::new(r.left() + 4.0, y), 1.8, crate::theme::fg());
                painter.line_segment(
                    [Pos2::new(r.left() + 10.0, y), Pos2::new(r.right() - 3.0, y)],
                    stroke,
                );
            }
        }
        TileIcon::Image => {
            painter.rect_stroke(r, 3.0, stroke);
            painter.circle_filled(
                Pos2::new(r.left() + w * 0.28, r.top() + w * 0.28),
                2.4,
                crate::theme::fg(),
            );
            painter.line_segment(
                [
                    Pos2::new(r.left() + 3.0, r.bottom() - 5.0),
                    Pos2::new(r.center().x, r.center().y + 2.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(r.center().x, r.center().y + 2.0),
                    Pos2::new(r.right() - 3.0, r.bottom() - 6.0),
                ],
                stroke,
            );
        }
        TileIcon::Github => {
            painter.circle_filled(Pos2::new(c.x, c.y + w * 0.04), w * 0.28, crate::theme::fg());
            painter.circle_filled(
                Pos2::new(c.x - w * 0.18, c.y - w * 0.16),
                w * 0.10,
                crate::theme::fg(),
            );
            painter.circle_filled(
                Pos2::new(c.x + w * 0.18, c.y - w * 0.16),
                w * 0.10,
                crate::theme::fg(),
            );
            painter.line_segment(
                [
                    Pos2::new(c.x, c.y + w * 0.28),
                    Pos2::new(c.x, c.y + w * 0.42),
                ],
                stroke,
            );
        }
        TileIcon::Check => {
            painter.circle_stroke(c, w * 0.40, stroke);
            painter.line_segment(
                [Pos2::new(c.x - 5.0, c.y), Pos2::new(c.x - 1.0, c.y + 4.0)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x - 1.0, c.y + 4.0), Pos2::new(c.x + 6.0, c.y - 4.0)],
                stroke,
            );
        }
        TileIcon::Board => {
            painter.rect_stroke(r, 3.0, stroke);
            painter.line_segment(
                [Pos2::new(r.center().x, r.top()), Pos2::new(r.center().x, r.bottom())],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(r.left(), r.center().y), Pos2::new(r.right(), r.center().y)],
                stroke,
            );
        }
        TileIcon::Bolt => {
            let pts = [
                Pos2::new(c.x + 2.0, r.top()),
                Pos2::new(c.x - 4.0, c.y + 1.0),
                Pos2::new(c.x + 1.0, c.y + 1.0),
                Pos2::new(c.x - 2.0, r.bottom()),
            ];
            painter.line_segment([pts[0], pts[1]], stroke);
            painter.line_segment([pts[1], pts[2]], stroke);
            painter.line_segment([pts[2], pts[3]], stroke);
        }
        TileIcon::Moon => {
            painter.circle_filled(c, w * 0.32, crate::theme::fg());
            painter.circle_filled(
                Pos2::new(c.x + w * 0.14, c.y - w * 0.08),
                w * 0.26,
                fill,
            );
        }
        TileIcon::Connect => {
            painter.circle_stroke(Pos2::new(c.x - 5.0, c.y), 4.0, stroke);
            painter.circle_stroke(Pos2::new(c.x + 5.0, c.y), 4.0, stroke);
            painter.line_segment(
                [Pos2::new(c.x - 1.0, c.y), Pos2::new(c.x + 1.0, c.y)],
                stroke,
            );
        }
        TileIcon::Think => {
            painter.circle_stroke(c + Vec2::new(0.0, -w * 0.06), w * 0.26, stroke);
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.10, c.y + w * 0.18),
                    Pos2::new(c.x + w * 0.10, c.y + w * 0.18),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.08, c.y + w * 0.28),
                    Pos2::new(c.x + w * 0.08, c.y + w * 0.28),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.05, c.y + w * 0.38),
                    Pos2::new(c.x + w * 0.05, c.y + w * 0.38),
                ],
                stroke,
            );
        }
        TileIcon::Help => {
            painter.circle_stroke(c, w * 0.40, stroke);
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(size * 0.42),
                crate::theme::fg(),
            );
        }
        TileIcon::Chat => {
            painter.rect_stroke(r.shrink(1.0), 6.0, stroke);
            painter.line_segment(
                [
                    Pos2::new(r.left() + 6.0, r.bottom() - 2.0),
                    Pos2::new(r.left() + 4.0, r.bottom() + 2.0),
                ],
                stroke,
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarIcon {
    Plus,
    Mic,
    Send,
    Stop,
    ArrowUp,
    ArrowDown,
    Search,
}

/// grok.com rail — 20px stroke-2 square-cap icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailIcon {
    Search,
    Compose,
    Imagine,
    Clock,
    Grid,
    Folder,
    Chat,
    File,
}

pub fn rail_icon_for(id: &str) -> RailIcon {
    match id {
        "chat" => RailIcon::Chat,
        "imagine" => RailIcon::Imagine,
        "automations" => RailIcon::Clock,
        "skills" | "connectors" => RailIcon::Grid,
        "workboard" => RailIcon::Folder,
        "history" => RailIcon::Clock,
        "search" => RailIcon::Search,
        "new" => RailIcon::Compose,
        _ => RailIcon::Chat,
    }
}

pub fn paint_rail_icon(ui: &mut egui::Ui, icon: RailIcon, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_rail_icon_at(ui.painter(), rect, icon, color);
}

pub fn paint_rail_icon_at(painter: &egui::Painter, rect: egui::Rect, icon: RailIcon, color: egui::Color32) {
    let c = rect.center();
    let w = rect.width();
    let stroke = Stroke::new(crate::theme::ICON_STROKE, color);
    match icon {
        RailIcon::Search => {
            painter.circle_stroke(Pos2::new(c.x - 1.0, c.y - 1.0), w * 0.22, stroke);
            painter.line_segment(
                [
                    Pos2::new(c.x + w * 0.10, c.y + w * 0.10),
                    Pos2::new(c.x + w * 0.24, c.y + w * 0.24),
                ],
                stroke,
            );
        }
        RailIcon::Compose => {
            let r = rect.shrink(w * 0.22);
            painter.rect_stroke(r, 3.0, stroke);
            painter.line_segment(
                [Pos2::new(c.x, r.top() + 3.0), Pos2::new(c.x, r.bottom() - 3.0)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(r.left() + 3.0, c.y), Pos2::new(r.right() - 3.0, c.y)],
                stroke,
            );
        }
        RailIcon::Imagine => {
            let r = rect.shrink(w * 0.20);
            painter.rect_stroke(r, 3.0, stroke);
            painter.circle_filled(
                Pos2::new(r.left() + w * 0.22, r.top() + w * 0.20),
                1.6,
                color,
            );
            painter.line_segment(
                [
                    Pos2::new(r.left() + 2.0, r.bottom() - 3.0),
                    Pos2::new(c.x, c.y + 1.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x, c.y + 1.0),
                    Pos2::new(r.right() - 2.0, r.bottom() - 3.0),
                ],
                stroke,
            );
        }
        RailIcon::Clock => {
            painter.circle_stroke(c, w * 0.32, stroke);
            painter.line_segment([c, Pos2::new(c.x, c.y - w * 0.16)], stroke);
            painter.line_segment([c, Pos2::new(c.x + w * 0.14, c.y + w * 0.08)], stroke);
        }
        RailIcon::Grid => {
            let s = w * 0.16;
            let g = w * 0.10;
            for row in 0..2 {
                for col in 0..2 {
                    let p = Pos2::new(
                        c.x - s - g * 0.5 + col as f32 * (s * 2.0 + g),
                        c.y - s - g * 0.5 + row as f32 * (s * 2.0 + g),
                    );
                    painter.rect_stroke(
                        egui::Rect::from_center_size(p, Vec2::splat(s * 2.0)),
                        2.0,
                        stroke,
                    );
                }
            }
        }
        RailIcon::Folder => {
            let r = rect.shrink(w * 0.20);
            painter.rect_stroke(
                egui::Rect::from_min_max(
                    Pos2::new(r.left(), r.top() + 4.0),
                    Pos2::new(r.right(), r.bottom()),
                ),
                3.0,
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(r.left(), r.top() + 4.0),
                    Pos2::new(r.left() + 4.0, r.top()),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(r.left() + 4.0, r.top()),
                    Pos2::new(c.x + 1.0, r.top()),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 1.0, r.top()),
                    Pos2::new(c.x + 4.0, r.top() + 4.0),
                ],
                stroke,
            );
        }
        RailIcon::Chat => {
            let r = rect.shrink(w * 0.20);
            painter.rect_stroke(r, 5.0, stroke);
            painter.line_segment(
                [
                    Pos2::new(r.left() + 4.0, r.bottom()),
                    Pos2::new(r.left() + 2.0, r.bottom() + 3.0),
                ],
                stroke,
            );
        }
        RailIcon::File => {
            let r = rect.shrink(w * 0.22);
            painter.rect_stroke(r, 2.0, stroke);
            painter.line_segment(
                [
                    Pos2::new(r.right() - 5.0, r.top()),
                    Pos2::new(r.right(), r.top() + 5.0),
                ],
                stroke,
            );
        }
    }
}

pub fn paint_folder_caret(ui: &mut egui::Ui, open: bool, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 12.0), Sense::hover());
    let c = rect.center();
    let stroke = Stroke::new(1.4_f32, color);
    if open {
        ui.painter().line_segment(
            [Pos2::new(c.x - 3.0, c.y - 1.0), Pos2::new(c.x, c.y + 2.0)],
            stroke,
        );
        ui.painter().line_segment(
            [Pos2::new(c.x, c.y + 2.0), Pos2::new(c.x + 3.0, c.y - 1.0)],
            stroke,
        );
    } else {
        ui.painter().line_segment(
            [Pos2::new(c.x - 1.0, c.y - 3.0), Pos2::new(c.x + 2.0, c.y)],
            stroke,
        );
        ui.painter().line_segment(
            [Pos2::new(c.x + 2.0, c.y), Pos2::new(c.x - 1.0, c.y + 3.0)],
            stroke,
        );
    }
}

/// How far a composer Stop or mic has eased this frame.
/// `scale` is the glyph size, `fill` is how solid the live wash is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposerMotion {
    pub scale: f32,
    pub fill: f32,
    pub press: f32,
}

/// Mic while the voice line is idle, open, or speaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicMood {
    Idle,
    Live,
    Speaking,
}

/// Short ease: fast at the start, settled at the end. Input is 0..1.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// 0..1 breath so a live control keeps moving after the hover ease settles.
pub fn composer_breath(time_secs: f32) -> f32 {
    let phase = time_secs * std::f32::consts::TAU * 1.7;
    (phase.sin() + 1.0) * 0.5
}

/// Stop mark side as a fraction of the disc diameter.
/// A halt glyph is about a third of the disc. The old mark was most of it.
pub fn stop_mark_ratio(press_t: f32) -> f32 {
    0.34 - 0.05 * press_t.clamp(0.0, 1.0)
}

/// Shared feel, amplified so a 28px disc actually travels, plus live and speech.
pub fn composer_motion(
    hover_t: f32,
    press_t: f32,
    live_t: f32,
    speak_phase: f32,
) -> ComposerMotion {
    let hover = ease_out_cubic(hover_t);
    let press = ease_out_cubic(press_t);
    let live = ease_out_cubic(live_t);
    let speak = speak_phase.clamp(0.0, 1.0);
    let feel = grokhub_core::feel_scale(hover, press);
    let travel = 1.0 + (feel - 1.0) * 2.6;
    let scale = (travel * (1.0 + 0.04 * live) * (1.0 + 0.06 * speak * live)).clamp(0.86, 1.16);
    let fill = (0.18 * hover + 0.55 * press + 0.62 * live + 0.38 * speak * live).clamp(0.0, 1.0);
    ComposerMotion { scale, fill, press }
}

fn composer_pointer_ease(ui: &egui::Ui, resp: &egui::Response) -> (f32, f32) {
    let hover_t = ui.ctx().animate_bool_with_time(
        resp.id.with("feel-h"),
        resp.hovered(),
        grokhub_core::HOVER_SECS,
    );
    let press_t = ui.ctx().animate_bool_with_time(
        resp.id.with("feel-p"),
        resp.is_pointer_button_down_on(),
        grokhub_core::PRESS_SECS,
    );
    (hover_t, press_t)
}

fn channel_moving(t: f32) -> bool {
    t > 0.0 && t < 1.0
}

/// Another frame only while the pointer is on the control, it is pressed,
/// the control is live, or a channel is still easing back to rest.
fn follow_composer_frame(ui: &egui::Ui, resp: &egui::Response, live: bool, channels: &[f32]) {
    let moving = channels.iter().copied().any(channel_moving);
    if resp.hovered() || resp.is_pointer_button_down_on() || live || moving {
        ui.ctx().request_repaint();
    }
}

fn paint_stop_glyph(painter: &egui::Painter, rect: egui::Rect, motion: ComposerMotion) {
    let c = rect.center();
    let radius = rect.width() * 0.34 * motion.scale;
    let disc = crate::theme::send_on();
    let ink = crate::theme::send_on_ink();
    let ring_a = (36.0 + 160.0 * motion.fill).clamp(0.0, 255.0) as u8;
    painter.circle_stroke(
        c,
        radius * (1.22 + 0.08 * motion.fill),
        Stroke::new(
            1.15 + 0.7 * motion.fill,
            egui::Color32::from_rgba_unmultiplied(disc.r(), disc.g(), disc.b(), ring_a),
        ),
    );
    painter.circle_filled(c, radius, disc);
    let side = radius * 2.0 * stop_mark_ratio(motion.press);
    let mark = egui::Rect::from_center_size(c, Vec2::splat(side));
    let round = side * (0.28 + 0.10 * (1.0 - motion.press));
    painter.rect_filled(mark, round, ink);
}

fn mic_ink(mood: MicMood, fill: f32) -> egui::Color32 {
    let t = ease_out_cubic(fill);
    match mood {
        MicMood::Idle => {
            crate::theme::blend_color(crate::theme::muted(), crate::theme::fg(), t * 0.85)
        }
        MicMood::Live | MicMood::Speaking => crate::theme::blend_color(
            crate::theme::muted(),
            crate::theme::live(),
            (0.35 + 0.65 * t).clamp(0.0, 1.0),
        ),
    }
}

fn paint_mic_glyph(
    painter: &egui::Painter,
    rect: egui::Rect,
    ink: egui::Color32,
    motion: ComposerMotion,
) {
    let c = rect.center();
    let s = rect.width() * motion.scale;
    let head = egui::Rect::from_center_size(
        Pos2::new(c.x, c.y - s * 0.06),
        Vec2::new(s * 0.30, s * 0.46),
    );
    let rounding = head.width() * 0.5;
    let stroke = Stroke::new(1.35 + 0.55 * motion.fill, ink);
    if motion.fill > 0.02 {
        let a = (motion.fill * 210.0).clamp(0.0, 255.0) as u8;
        painter.rect_filled(
            head,
            rounding,
            egui::Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), a),
        );
    }
    painter.rect_stroke(head, rounding, stroke);
    let cradle_y = c.y + s * 0.16;
    painter.add(egui::epaint::QuadraticBezierShape::from_points_stroke(
        [
            Pos2::new(c.x - s * 0.26, cradle_y),
            Pos2::new(c.x, c.y + s * 0.34),
            Pos2::new(c.x + s * 0.26, cradle_y),
        ],
        false,
        egui::Color32::TRANSPARENT,
        stroke,
    ));
    painter.line_segment(
        [
            Pos2::new(c.x, c.y + s * 0.34),
            Pos2::new(c.x, c.y + s * 0.42),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(c.x - s * 0.14, c.y + s * 0.42),
            Pos2::new(c.x + s * 0.14, c.y + s * 0.42),
        ],
        stroke,
    );
    if motion.fill > 0.35 {
        let halo_a = (motion.fill * 90.0).clamp(0.0, 255.0) as u8;
        painter.circle_stroke(
            c,
            s * 0.48,
            Stroke::new(
                1.0_f32,
                egui::Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), halo_a),
            ),
        );
    }
}

/// Composer halt disc. Same paint on Linux and Windows.
/// `running` is the reply. Idle (`running == false`, pointer off) sits still.
pub fn paint_composer_stop(
    ui: &mut egui::Ui,
    size: f32,
    running: bool,
) -> (egui::Response, ComposerMotion) {
    let (alloc, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let (resp, _, _) = crate::theme::feel_response(ui, resp, egui::Color32::TRANSPARENT);
    let (hover_t, press_t) = composer_pointer_ease(ui, &resp);
    let live_t = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("glyph-live"), running, 0.16);
    let breath = if running {
        composer_breath(ui.ctx().input(|i| i.time) as f32)
    } else {
        0.0
    };
    let motion = composer_motion(hover_t, press_t, live_t, breath);
    paint_stop_glyph(ui.painter(), alloc, motion);
    follow_composer_frame(ui, &resp, running, &[hover_t, press_t, live_t]);
    (resp, motion)
}

/// Composer mic. `mood` eases the fill from idle through listening into speech.
pub fn paint_composer_mic(
    ui: &mut egui::Ui,
    size: f32,
    mood: MicMood,
) -> (egui::Response, ComposerMotion) {
    let (alloc, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let (resp, _, _) = crate::theme::feel_response(ui, resp, egui::Color32::TRANSPARENT);
    let (hover_t, press_t) = composer_pointer_ease(ui, &resp);
    let live = !matches!(mood, MicMood::Idle);
    let live_t = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("glyph-live"), live, 0.16);
    let speak_target = matches!(mood, MicMood::Speaking);
    let speak_t = ui.ctx().animate_bool_with_time(
        resp.id.with("glyph-speak"),
        speak_target,
        grokhub_core::HOVER_SECS,
    );
    let pulsing = live || channel_moving(live_t) || channel_moving(speak_t);
    let breath = if pulsing {
        composer_breath(ui.ctx().input(|i| i.time) as f32)
    } else {
        0.0
    };
    let speak_phase = speak_t * breath + (1.0 - speak_t) * breath * 0.35 * live_t;
    let motion = composer_motion(hover_t, press_t, live_t, speak_phase);
    paint_mic_glyph(ui.painter(), alloc, mic_ink(mood, motion.fill), motion);
    follow_composer_frame(
        ui,
        &resp,
        live,
        &[hover_t, press_t, live_t, speak_t],
    );
    (resp, motion)
}

pub fn paint_bar_icon(
    ui: &mut egui::Ui,
    icon: BarIcon,
    size: f32,
    color: egui::Color32,
) -> egui::Response {
    match icon {
        // Stop is only chosen while a reply is running (`composer_go`).
        BarIcon::Stop => return paint_composer_stop(ui, size, true).0,
        BarIcon::Mic => return paint_composer_mic(ui, size, MicMood::Idle).0,
        BarIcon::Plus | BarIcon::Send | BarIcon::ArrowUp | BarIcon::ArrowDown | BarIcon::Search => {}
    }
    let (_rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let (resp, rect, wash) = crate::theme::feel_response(ui, resp, egui::Color32::TRANSPARENT);
    let painter = ui.painter();
    if wash.a() > 0 {
        painter.circle_filled(rect.center(), rect.width() * 0.55, wash);
    }
    let optical = match icon {
        BarIcon::Send => crate::theme::ICON_ACTION,
        _ => crate::theme::ICON_CHROME,
    };
    let glyph = rect.shrink(((rect.width() - optical) * 0.5).max(0.0));
    let c = glyph.center();
    let w = glyph.width();
    let stroke = Stroke::new(crate::theme::ICON_STROKE, color);
    match icon {
        BarIcon::Plus => {
            painter.line_segment(
                [Pos2::new(c.x, c.y - w * 0.22), Pos2::new(c.x, c.y + w * 0.22)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x - w * 0.22, c.y), Pos2::new(c.x + w * 0.22, c.y)],
                stroke,
            );
        }
        BarIcon::Send => {
            painter.circle_filled(c, w * 0.46, crate::theme::send_on());
            let arrow = Stroke::new(1.8_f32, crate::theme::send_on_ink());
            painter.line_segment(
                [Pos2::new(c.x, c.y + w * 0.16), Pos2::new(c.x, c.y - w * 0.16)],
                arrow,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.12, c.y - w * 0.02),
                    Pos2::new(c.x, c.y - w * 0.16),
                ],
                arrow,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + w * 0.12, c.y - w * 0.02),
                    Pos2::new(c.x, c.y - w * 0.16),
                ],
                arrow,
            );
        }
        // Painted above, before this allocate. Kept so the match stays exhaustive.
        BarIcon::Mic | BarIcon::Stop => {}
        BarIcon::ArrowUp => {
            // grok.com Submit: M6 11L12 5M12 5L18 11M12 5V19 square-cap
            painter.line_segment(
                [Pos2::new(c.x, c.y + w * 0.22), Pos2::new(c.x, c.y - w * 0.22)],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.20, c.y - w * 0.02),
                    Pos2::new(c.x, c.y - w * 0.22),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + w * 0.20, c.y - w * 0.02),
                    Pos2::new(c.x, c.y - w * 0.22),
                ],
                stroke,
            );
        }
        BarIcon::ArrowDown => {
            painter.line_segment(
                [Pos2::new(c.x, c.y - w * 0.22), Pos2::new(c.x, c.y + w * 0.22)],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.20, c.y + w * 0.02),
                    Pos2::new(c.x, c.y + w * 0.22),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + w * 0.20, c.y + w * 0.02),
                    Pos2::new(c.x, c.y + w * 0.22),
                ],
                stroke,
            );
        }
        BarIcon::Search => {
            painter.circle_stroke(Pos2::new(c.x - 1.0, c.y - 1.0), w * 0.22, stroke);
            painter.line_segment(
                [
                    Pos2::new(c.x + w * 0.10, c.y + w * 0.10),
                    Pos2::new(c.x + w * 0.22, c.y + w * 0.22),
                ],
                stroke,
            );
        }
    }
    resp
}

pub fn icon_for_label(label: &str) -> TileIcon {
    let l = label.to_ascii_lowercase();
    if l.contains("connect") {
        TileIcon::Connect
    } else if l.contains("host") || l.contains("machine") || l.contains("dawn") {
        TileIcon::Host
    } else if l.contains("imagine") || l.contains("draw") || l.contains("image") {
        TileIcon::Image
    } else if l.contains("think") || l.contains("harder") || l.contains("max") {
        TileIcon::Think
    } else if l.contains("help") || l.contains("what can") {
        TileIcon::Help
    } else if l.contains("github") {
        TileIcon::Github
    } else if l.contains("board") || l.contains("task") || l.contains("triage") {
        TileIcon::List
    } else if l.contains("brief") || l.contains("morning") {
        TileIcon::Sun
    } else if l.contains("night") || l.contains("moon") {
        TileIcon::Moon
    } else if l.contains("verify") || l.contains("check") {
        TileIcon::Check
    } else if l.contains("heartbeat") || l.contains("health") {
        TileIcon::Bolt
    } else {
        TileIcon::Chat
    }
}

/// Landscape glyph for the Imagine Image-mode pill — no catalog-card chrome.
pub fn paint_image_mode(ui: &mut egui::Ui, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let r = rect.shrink(size * 0.08);
    let stroke = Stroke::new(1.4_f32, color);
    painter.rect_stroke(r, 2.0, stroke);
    painter.circle_filled(
        Pos2::new(r.left() + r.width() * 0.28, r.top() + r.height() * 0.32),
        size * 0.08,
        color,
    );
    painter.line_segment(
        [
            Pos2::new(r.left() + 2.0, r.bottom() - 3.0),
            Pos2::new(r.center().x, r.center().y + 1.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(r.center().x, r.center().y + 1.0),
            Pos2::new(r.right() - 2.0, r.bottom() - 4.0),
        ],
        stroke,
    );
}

pub fn paint_video_mode(ui: &mut egui::Ui, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let stroke = Stroke::new(1.4_f32, color);
    let body = egui::Rect::from_center_size(
        Pos2::new(rect.center().x - size * 0.08, rect.center().y),
        Vec2::new(size * 0.52, size * 0.40),
    );
    painter.rect_stroke(body, 2.0, stroke);
    painter.line_segment(
        [
            Pos2::new(body.right(), body.top() + 2.0),
            Pos2::new(rect.right() - 2.0, body.top() - 1.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(body.right(), body.bottom() - 2.0),
            Pos2::new(rect.right() - 2.0, body.bottom() + 1.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(rect.right() - 2.0, body.top() - 1.0),
            Pos2::new(rect.right() - 2.0, body.bottom() + 1.0),
        ],
        stroke,
    );
}

pub fn paint_agent_mode(ui: &mut egui::Ui, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let stroke = Stroke::new(1.4_f32, color);
    let c = rect.center();
    painter.circle_stroke(Pos2::new(c.x, c.y - size * 0.06), size * 0.22, stroke);
    painter.circle_filled(Pos2::new(c.x - size * 0.07, c.y - size * 0.08), 1.2, color);
    painter.circle_filled(Pos2::new(c.x + size * 0.07, c.y - size * 0.08), 1.2, color);
    painter.line_segment(
        [
            Pos2::new(c.x - size * 0.06, c.y + size * 0.02),
            Pos2::new(c.x + size * 0.06, c.y + size * 0.02),
        ],
        stroke,
    );
}

pub fn paint_style_auto(ui: &mut egui::Ui, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let stroke = Stroke::new(1.3_f32, color);
    let r = rect.shrink(size * 0.12);
    painter.rect_stroke(r, 2.0, stroke);
    let inset = r.shrink(size * 0.10);
    painter.rect_stroke(inset, 1.0, Stroke::new(1.0_f32, color));
}

pub fn paint_aspect_rect(ui: &mut egui::Ui, aspect: u8, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let stroke = Stroke::new(1.4_f32, color);
    let (w, h) = match aspect % 5 {
        0 => (size * 0.28, size * 0.46),
        1 => (size * 0.46, size * 0.30),
        2 => (size * 0.42, size * 0.42),
        3 => (size * 0.24, size * 0.48),
        4 => (size * 0.50, size * 0.28),
        other => {
            let _ = other;
            (size * 0.42, size * 0.42)
        }
    };
    painter.rect_stroke(egui::Rect::from_center_size(rect.center(), Vec2::new(w, h)), 1.5, stroke);
}

pub fn paint_menu_caret(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 10.0), Sense::hover());
    let c = rect.center();
    let stroke = Stroke::new(1.4_f32, color);
    ui.painter().line_segment(
        [Pos2::new(c.x - 3.0, c.y - 1.0), Pos2::new(c.x, c.y + 2.0)],
        stroke,
    );
    ui.painter().line_segment(
        [Pos2::new(c.x, c.y + 2.0), Pos2::new(c.x + 3.0, c.y - 1.0)],
        stroke,
    );
}

pub fn paint_plus_at(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let w = rect.width();
    let stroke = Stroke::new(1.6_f32, color);
    painter.line_segment(
        [Pos2::new(c.x, c.y - w * 0.18), Pos2::new(c.x, c.y + w * 0.18)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(c.x - w * 0.18, c.y), Pos2::new(c.x + w * 0.18, c.y)],
        stroke,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_icons_are_distinct() {
        assert_eq!(icon_for_label("Connect Grok"), TileIcon::Connect);
        assert_eq!(icon_for_label("Open Imagine"), TileIcon::Image);
        assert_eq!(icon_for_label("Think Harder"), TileIcon::Think);
        assert_eq!(icon_for_label("Go Max"), TileIcon::Think);
        assert_ne!(icon_for_label("Host snapshot"), icon_for_label("Morning brief"));
        assert_ne!(BarIcon::Mic, BarIcon::Send);
        assert_ne!(BarIcon::Plus, BarIcon::Search);
        assert_ne!(BarIcon::ArrowUp, BarIcon::Send);
        assert_ne!(BarIcon::ArrowDown, BarIcon::ArrowUp);
        assert_ne!(BarIcon::Stop, BarIcon::Send);
        assert_eq!(rail_icon_for("chat"), RailIcon::Chat);
        assert_eq!(rail_icon_for("imagine"), RailIcon::Imagine);
        assert_eq!(rail_icon_for("automations"), RailIcon::Clock);
        assert_eq!(rail_icon_for("skills"), RailIcon::Grid);
        assert_eq!(rail_icon_for("workboard"), RailIcon::Folder);
        assert_ne!(RailIcon::Search, RailIcon::Compose);
        assert_ne!(RailIcon::Imagine, RailIcon::Grid);
        let _ = paint_image_mode;
        let _ = paint_video_mode;
        let _ = paint_agent_mode;
        let _ = paint_style_auto;
        let _ = paint_aspect_rect;
        let _ = paint_menu_caret;
        let _ = paint_plus_at;
        let _ = paint_folder_caret;
        assert_ne!(RailIcon::File, RailIcon::Folder);
    }

    #[test]
    fn composer_stop_mark_is_a_small_squircle() {
        assert!(stop_mark_ratio(0.0) < 0.42);
        assert!(stop_mark_ratio(1.0) < stop_mark_ratio(0.0));
        let src = include_str!("icons.rs");
        let stop = src
            .split("pub fn paint_composer_stop(")
            .nth(1)
            .and_then(|s| s.split("pub fn paint_composer_mic(").next())
            .expect("paint_composer_stop");
        assert!(
            stop.contains("feel_response")
                && stop.contains("animate_bool_with_time")
                && stop.contains("composer_motion")
                && stop.contains("paint_stop_glyph")
                && stop.contains("running")
                && !stop.contains("\"glyph-live\"), true"),
            "stop eases live only while a reply runs: {stop}"
        );
        let mic = src
            .split("pub fn paint_composer_mic(")
            .nth(1)
            .and_then(|s| s.split("pub fn paint_bar_icon(").next())
            .expect("paint_composer_mic");
        let ink = src
            .split("fn mic_ink(")
            .nth(1)
            .and_then(|s| s.split("fn paint_mic_glyph(").next())
            .expect("mic_ink");
        assert!(
            mic.contains("feel_response")
                && mic.contains("animate_bool_with_time")
                && mic.contains("composer_motion")
                && mic.contains("paint_mic_glyph"),
            "mic uses shared feel and a short ease: {mic}"
        );
        assert!(
            ink.contains("theme::live()"),
            "a live mic eases into the live color: {ink}"
        );
        assert!(
            src.contains("BarIcon::Stop => return paint_composer_stop")
                && src.contains("BarIcon::Mic => return paint_composer_mic"),
            "bar icons share the composer paint"
        );
    }

    #[test]
    fn composer_stop_and_mic_motion_changes_across_frames() {
        let rest = composer_motion(0.0, 0.0, 0.0, 0.0);
        let mid = composer_motion(0.45, 0.0, 0.0, 0.0);
        let hover = composer_motion(1.0, 0.0, 0.0, 0.0);
        assert!(rest.scale < mid.scale && mid.scale < hover.scale);
        assert!(rest.fill < mid.fill && mid.fill < hover.fill);
        let press = composer_motion(1.0, 1.0, 0.0, 0.0);
        assert!(press.scale < hover.scale);
        assert!(press.fill > hover.fill);
        let listen = composer_motion(0.0, 0.0, 0.55, 0.0);
        let speak_a = composer_motion(0.0, 0.0, 1.0, 0.15);
        let speak_b = composer_motion(0.0, 0.0, 1.0, 0.85);
        assert!(listen.scale < speak_a.scale);
        assert!((speak_a.scale - speak_b.scale).abs() > 0.01);
        assert!((speak_a.fill - speak_b.fill).abs() > 0.01);

        let ctx = egui::Context::default();
        let mut hover_scales = Vec::new();
        for step in 0..5 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 240.0),
                )),
                time: Some(step as f64 / 60.0),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                let hover_t = ctx.animate_bool_with_time(
                    egui::Id::new("composer-hover-ease"),
                    step > 0,
                    grokhub_core::HOVER_SECS,
                );
                hover_scales.push(composer_motion(hover_t, 0.0, 0.0, 0.0).scale);
            });
        }
        assert!(
            hover_scales.windows(2).any(|w| w[1] - w[0] > 1e-3),
            "hover ease must move across frames: {hover_scales:?}"
        );

        let ctx = egui::Context::default();
        let mut stop_scales = Vec::new();
        let mut mic_scales = Vec::new();
        let mut mic_fills = Vec::new();
        for step in 0..6 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                time: Some(step as f64 * 0.05),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (_, stop) = paint_composer_stop(ui, 28.0, true);
                    let (_, mic) = paint_composer_mic(ui, 22.0, MicMood::Speaking);
                    stop_scales.push(stop.scale);
                    mic_scales.push(mic.scale);
                    mic_fills.push(mic.fill);
                });
            });
        }
        assert!(
            stop_scales.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-3),
            "stop motion must change across frames: {stop_scales:?}"
        );
        assert!(
            mic_scales.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-3),
            "mic motion must change across frames: {mic_scales:?}"
        );
        assert!(
            mic_fills.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-3),
            "mic fill must change across frames, not a static color: {mic_fills:?}"
        );
    }

    #[test]
    fn composer_idle_stop_and_mic_sit_still() {
        let ctx = egui::Context::default();
        let mut stop_scales = Vec::new();
        let mut mic_scales = Vec::new();
        let mut delays = Vec::new();
        for step in 0..8 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                time: Some(step as f64 * 0.05),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (_, stop) = paint_composer_stop(ui, 28.0, false);
                    let (_, mic) = paint_composer_mic(ui, 22.0, MicMood::Idle);
                    stop_scales.push(stop.scale);
                    mic_scales.push(mic.scale);
                });
            });
            let delay = out
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .map(|v| v.repaint_delay)
                .expect("root viewport");
            delays.push(delay);
        }
        assert!(
            stop_scales.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-4),
            "idle stop scale must sit still: {stop_scales:?}"
        );
        assert!(
            mic_scales.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-4),
            "idle mic scale must sit still: {mic_scales:?}"
        );
        assert_eq!(
            delays.last().copied(),
            Some(std::time::Duration::MAX),
            "idle controls must not ask for another frame: {delays:?}"
        );
    }
}
