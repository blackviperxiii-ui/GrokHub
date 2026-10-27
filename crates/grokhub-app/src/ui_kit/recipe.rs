//! Deck slide 07 state recipes: one look per widget part and state, read from
//! the theme tokens. Default is a red outline, hover lifts to the hover gray,
//! selected to the selected gray, and the accent is amber with dark ink.
//! Buttons round at 8, cards at 12, and outlines are 1.5 px.

use eframe::egui::{self, Color32, Stroke};

/// Outline width for every Signal Red rim.
pub const STROKE_W: f32 = 1.5;
/// Buttons, tabs and nav tiles.
pub const BUTTON_RADIUS: f32 = 8.0;
/// Cards.
pub const CARD_RADIUS: f32 = 12.0;
/// Amber tab underline and nav bar thickness.
pub const MARK_W: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Button,
    Tab,
    Card,
    Nav,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Default,
    Hover,
    Selected,
    Accent,
}

/// The amber marker some selected parts carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    None,
    /// Under a selected tab.
    Underline,
    /// Left of a selected nav item.
    LeftBar,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fill: Color32,
    pub stroke: Stroke,
    pub ink: Color32,
    pub radius: f32,
    pub mark: Mark,
}

/// Selected beats hover; accent is the caller's call.
pub fn state(hovered: bool, selected: bool, accent: bool) -> State {
    if accent {
        State::Accent
    } else if selected {
        State::Selected
    } else if hovered {
        State::Hover
    } else {
        State::Default
    }
}

/// The slide 07 recipe for `part` in `state`, from the active tokens.
pub fn look(part: Part, state: State) -> Look {
    use crate::theme::{amber, fg, hover, muted, raised, selected, signal_red, steel, void_bg};
    let red = Stroke::new(STROKE_W, signal_red());
    let amber_rim = Stroke::new(STROKE_W, amber());
    let clear = Color32::TRANSPARENT;
    let (fill, stroke, ink, mark) = match (part, state) {
        (Part::Button, State::Default) => (clear, red, fg(), Mark::None),
        (Part::Button, State::Hover) => (hover(), red, fg(), Mark::None),
        (Part::Button, State::Selected) => (selected(), red, steel(), Mark::None),
        (Part::Button, State::Accent) => (amber(), Stroke::NONE, void_bg(), Mark::None),
        (Part::Tab, State::Default) => (clear, Stroke::NONE, muted(), Mark::None),
        (Part::Tab, State::Hover) => (hover(), Stroke::NONE, fg(), Mark::None),
        (Part::Tab, State::Selected) => (selected(), Stroke::NONE, steel(), Mark::Underline),
        (Part::Tab, State::Accent) => (clear, amber_rim, amber(), Mark::None),
        (Part::Card, State::Default) => (raised(), red, fg(), Mark::None),
        (Part::Card, State::Hover) => (hover(), red, fg(), Mark::None),
        (Part::Card, State::Selected) => (selected(), red, fg(), Mark::None),
        (Part::Card, State::Accent) => (raised(), amber_rim, fg(), Mark::None),
        (Part::Nav, State::Default) => (
            clear,
            Stroke::new(1.0, crate::theme::border()),
            muted(),
            Mark::None,
        ),
        (Part::Nav, State::Hover) => (hover(), Stroke::NONE, fg(), Mark::None),
        (Part::Nav, State::Selected) => (selected(), Stroke::NONE, fg(), Mark::LeftBar),
        (Part::Nav, State::Accent) => (amber(), Stroke::NONE, void_bg(), Mark::None),
    };
    let radius = if part == Part::Card {
        CARD_RADIUS
    } else {
        BUTTON_RADIUS
    };
    Look {
        fill,
        stroke,
        ink,
        radius,
        mark,
    }
}

/// Fill, rim and marker for `look` on `rect`. The caller paints the label.
pub fn paint(painter: &egui::Painter, rect: egui::Rect, look: &Look) {
    painter.rect_filled(rect, look.radius, look.fill);
    if look.stroke.width > 0.0 {
        painter.rect_stroke(rect, look.radius, look.stroke, egui::StrokeKind::Inside);
    }
    match look.mark {
        Mark::None => {}
        Mark::Underline => {
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 2.0, rect.bottom() - MARK_W),
                egui::pos2(rect.right() - 2.0, rect.bottom()),
            );
            painter.rect_filled(bar, 1.5, crate::theme::amber());
        }
        Mark::LeftBar => {
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left() - MARK_W - 3.0, rect.center().y - 10.0),
                egui::pos2(rect.left() - 3.0, rect.center().y + 10.0),
            );
            painter.rect_filled(bar, 1.5, crate::theme::amber());
        }
    }
}

/// A labelled button in the slide 07 recipe. Same size, id and hover feel
/// as [`crate::theme::felt_label_button_with`]; only the paint differs.
pub fn button(
    ui: &mut egui::Ui,
    id: Option<egui::Id>,
    label: &str,
    accent: bool,
    min_size: egui::Vec2,
    strong: bool,
) -> egui::Response {
    let font = if strong {
        crate::theme::title_font(crate::theme::FONT_CHROME)
    } else {
        egui::FontId::proportional(crate::theme::FONT_CHROME)
    };
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER));
    let pad = ui.style().spacing.button_padding;
    let size = egui::vec2(
        (galley.size().x + pad.x * 2.0).max(min_size.x),
        (galley.size().y + pad.y * 2.0).max(min_size.y),
    );
    let resp = match id {
        Some(id) => {
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.interact(rect, id, egui::Sense::click())
        }
        None => ui.allocate_exact_size(size, egui::Sense::click()).1,
    };
    let (resp, rect, _) = crate::theme::feel_button(ui, resp, Color32::TRANSPARENT);
    let look = look(Part::Button, state(resp.hovered(), false, accent));
    paint(ui.painter(), rect, &look);
    let text_pos = egui::pos2(rect.min.x + pad.x, rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(text_pos, galley, look.ink);
    let resp = crate::theme::pointing(resp);
    let enabled = resp.enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{
        hold_paint_test, set_paint_palette, PaintPalette, OPS_AMBER, OPS_HOVER, OPS_RAISED,
        OPS_SELECTED, OPS_SIGNAL_RED, OPS_VOID,
    };

    #[test]
    fn ops_dark_recipes_match_slide_07() {
        let _paint = hold_paint_test();
        set_paint_palette(PaintPalette::OpsDark);
        let red = Stroke::new(1.5, OPS_SIGNAL_RED);

        let b = look(Part::Button, State::Default);
        assert_eq!(
            (b.fill, b.stroke, b.radius),
            (Color32::TRANSPARENT, red, 8.0)
        );
        assert_eq!(look(Part::Button, State::Hover).fill, OPS_HOVER);
        assert_eq!(look(Part::Button, State::Selected).fill, OPS_SELECTED);
        let cta = look(Part::Button, State::Accent);
        assert_eq!(
            (cta.fill, cta.ink, cta.stroke),
            (OPS_AMBER, OPS_VOID, Stroke::NONE)
        );

        assert_eq!(look(Part::Tab, State::Default).stroke, Stroke::NONE);
        let tab = look(Part::Tab, State::Selected);
        assert_eq!((tab.fill, tab.mark), (OPS_SELECTED, Mark::Underline));
        assert_eq!(
            look(Part::Tab, State::Accent).stroke,
            Stroke::new(1.5, OPS_AMBER)
        );

        let card = look(Part::Card, State::Default);
        assert_eq!(
            (card.fill, card.stroke, card.radius),
            (OPS_RAISED, red, 12.0)
        );
        assert_eq!(look(Part::Card, State::Hover).fill, OPS_HOVER);
        assert_eq!(look(Part::Card, State::Selected).fill, OPS_SELECTED);
        assert_eq!(
            look(Part::Card, State::Accent).stroke,
            Stroke::new(1.5, OPS_AMBER)
        );

        let nav = look(Part::Nav, State::Selected);
        assert_eq!((nav.fill, nav.mark), (OPS_SELECTED, Mark::LeftBar));
        assert_eq!(look(Part::Nav, State::Accent).fill, OPS_AMBER);
    }

    #[test]
    fn selected_beats_hover_and_accent_beats_both() {
        assert_eq!(state(false, false, false), State::Default);
        assert_eq!(state(true, false, false), State::Hover);
        assert_eq!(state(true, true, false), State::Selected);
        assert_eq!(state(true, true, true), State::Accent);
    }
}
