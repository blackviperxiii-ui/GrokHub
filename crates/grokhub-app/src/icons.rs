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
    Search,
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
    }
}
