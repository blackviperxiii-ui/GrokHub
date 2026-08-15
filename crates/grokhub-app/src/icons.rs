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
    let fill = crate::theme::SURFACE;
    let stroke = Stroke::new(1.5, crate::theme::FG);
    painter.rect_filled(rect, 10.0, fill);
    painter.rect_stroke(rect, 10.0, Stroke::new(1.0, crate::theme::BORDER_STRONG));
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
                painter.circle_filled(Pos2::new(r.left() + 4.0, y), 1.8, crate::theme::FG);
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
                crate::theme::FG,
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
            painter.circle_filled(Pos2::new(c.x, c.y + w * 0.04), w * 0.28, crate::theme::FG);
            painter.circle_filled(
                Pos2::new(c.x - w * 0.18, c.y - w * 0.16),
                w * 0.10,
                crate::theme::FG,
            );
            painter.circle_filled(
                Pos2::new(c.x + w * 0.18, c.y - w * 0.16),
                w * 0.10,
                crate::theme::FG,
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
            painter.circle_filled(c, w * 0.32, crate::theme::FG);
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
                crate::theme::FG,
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
    ArrowUp,
    Search,
    Gear,
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
}

pub fn rail_icon_for(id: &str) -> RailIcon {
    match id {
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
    }
}

pub fn paint_bar_icon(ui: &mut egui::Ui, icon: BarIcon, size: f32, color: egui::Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let painter = ui.painter();
    let c = rect.center();
    let w = rect.width();
    let stroke = Stroke::new(1.6, color);
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
            painter.circle_filled(c, w * 0.46, crate::theme::FG);
            let arrow = Stroke::new(1.8, crate::theme::BG);
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
        BarIcon::Gear => {
            painter.circle_stroke(c, w * 0.16, stroke);
            for i in 0..6 {
                let a = i as f32 * std::f32::consts::TAU / 6.0;
                painter.line_segment(
                    [
                        c + Vec2::new(a.cos() * w * 0.18, a.sin() * w * 0.18),
                        c + Vec2::new(a.cos() * w * 0.32, a.sin() * w * 0.32),
                    ],
                    stroke,
                );
            }
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
    } else if l.contains("think") || l.contains("harder") {
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
        assert_ne!(icon_for_label("Host snapshot"), icon_for_label("Morning brief"));
        assert_ne!(BarIcon::Mic, BarIcon::Send);
        assert_ne!(BarIcon::Plus, BarIcon::Search);
        assert_ne!(BarIcon::Gear, BarIcon::Search);
        assert_ne!(BarIcon::ArrowUp, BarIcon::Send);
        assert_eq!(rail_icon_for("imagine"), RailIcon::Imagine);
        assert_eq!(rail_icon_for("automations"), RailIcon::Clock);
        assert_eq!(rail_icon_for("skills"), RailIcon::Grid);
        assert_eq!(rail_icon_for("workboard"), RailIcon::Folder);
        assert_ne!(RailIcon::Search, RailIcon::Compose);
        assert_ne!(RailIcon::Imagine, RailIcon::Grid);
        let _ = paint_image_mode;
        let _ = paint_plus_at;
    }
}
