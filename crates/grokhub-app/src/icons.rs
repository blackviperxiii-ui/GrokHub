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
    let fill = crate::theme::surface();
    let stroke = Stroke::new(1.5_f32, crate::theme::fg());
    painter.rect_filled(rect, 10.0, fill);
    painter.rect_stroke(rect, 10.0, Stroke::new(1.0_f32, crate::theme::border_strong()));
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
    let stroke = Stroke::new(1.8_f32, color);
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

pub fn paint_bar_icon(ui: &mut egui::Ui, icon: BarIcon, size: f32, color: egui::Color32) -> egui::Response {
    let (_rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let (resp, rect, wash) = crate::theme::feel_response(ui, resp, egui::Color32::TRANSPARENT);
    let painter = ui.painter();
    if wash.a() > 0 {
        painter.circle_filled(rect.center(), rect.width() * 0.55, wash);
    }
    let c = rect.center();
    let w = rect.width();
    let stroke = Stroke::new(1.6_f32, color);
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
        BarIcon::Mic => {
            let cap = egui::Rect::from_center_size(
                Pos2::new(c.x, c.y - w * 0.08),
                Vec2::new(w * 0.32, w * 0.40),
            );
            painter.rect_filled(cap, 7.0, color);
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.22, c.y + w * 0.02),
                    Pos2::new(c.x - w * 0.22, c.y + w * 0.10),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + w * 0.22, c.y + w * 0.02),
                    Pos2::new(c.x + w * 0.22, c.y + w * 0.10),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.22, c.y + w * 0.10),
                    Pos2::new(c.x + w * 0.22, c.y + w * 0.10),
                ],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x, c.y + w * 0.10), Pos2::new(c.x, c.y + w * 0.26)],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.14, c.y + w * 0.26),
                    Pos2::new(c.x + w * 0.14, c.y + w * 0.26),
                ],
                stroke,
            );
        }
        BarIcon::Send => {
            painter.circle_filled(c, w * 0.46, crate::theme::fg());
            let arrow = Stroke::new(1.8_f32, crate::theme::bg());
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
        BarIcon::Stop => {
            painter.circle_filled(c, w * 0.46, crate::theme::fg());
            let pad = w * 0.18;
            painter.rect_filled(
                egui::Rect::from_center_size(c, Vec2::splat(w - pad * 2.0)),
                2.0,
                crate::theme::bg(),
            );
        }
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
}
