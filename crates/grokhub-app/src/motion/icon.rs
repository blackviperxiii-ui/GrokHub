//! One pose for every glyph the cabin draws: rail, titlebar, cards and bar
//! buttons. A glyph moves only on an event (hover, press, or becoming
//! active), each channel eases with egui's own animator, and egui asks for a
//! frame only while a channel is moving, so an idle cabin repaints nothing.
//! Reduced motion rests every glyph.

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

/// Hover ease for a glyph, and for a card lifting its icon.
pub const HOVER_SECS: f32 = 0.14;
/// Press squeeze.
pub const PRESS_SECS: f32 = 0.08;
/// Scale pop when a glyph becomes active or selected.
pub const POP_SECS: f32 = 0.18;
/// How far a hovered glyph (or a hovered card's icon) rises.
pub const LIFT_PX: f32 = 2.0;
/// How far a nudging glyph slides toward its direction.
pub const NUDGE_PX: f32 = 2.0;
/// Glyph scale while pressed.
pub const PRESS_SCALE: f32 = 0.88;
/// Peak of the becoming-active pop.
pub const POP_SCALE: f32 = 1.18;

/// What a glyph does while the pointer is on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kick {
    /// Rise straight up.
    Lift,
    /// Turn by this many radians, clockwise (a gear, a clock, the close ×).
    Turn(f32),
    /// Slide toward this direction (send, arrows, minimize).
    Nudge(Vec2),
}

/// The events a glyph answers to this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cue {
    pub hovered: bool,
    pub pressed: bool,
    /// Selected or current. Turning on pops the glyph once.
    pub active: bool,
}

impl Cue {
    pub fn of(resp: &egui::Response, active: bool) -> Self {
        Self {
            hovered: resp.hovered(),
            pressed: resp.is_pointer_button_down_on(),
            active,
        }
    }
}

/// Where a glyph sits this frame, relative to its resting rect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub offset: Vec2,
    pub scale: f32,
    /// Radians, clockwise.
    pub angle: f32,
    /// 0..1 toward the accent, for card icons.
    pub tint: f32,
}

impl Pose {
    pub const REST: Self = Self {
        offset: Vec2::ZERO,
        scale: 1.0,
        angle: 0.0,
        tint: 0.0,
    };

    /// Where `p` lands when the glyph centred on `c` takes this pose.
    pub fn map(&self, c: Pos2, p: Pos2) -> Pos2 {
        let d = (p - c) * self.scale;
        let (s, co) = self.angle.sin_cos();
        c + self.offset + Vec2::new(d.x * co - d.y * s, d.x * s + d.y * co)
    }

    /// `base` leaned toward `accent` by this pose's tint.
    pub fn ink(&self, base: Color32, accent: Color32) -> Color32 {
        crate::theme::blend_color(base, accent, self.tint.clamp(0.0, 1.0))
    }
}

/// 0 → 1 → 0 over the pop: up and back, resting at both ends.
pub fn pop_envelope(t: f32) -> f32 {
    if t <= 0.0 || t >= 1.0 {
        0.0
    } else {
        (std::f32::consts::PI * t).sin()
    }
}

/// The pose for eased channels: `hover` and `press` are 0..1, `pop` is the
/// [`pop_envelope`] of the becoming-active channel.
pub fn pose_at(kick: Kick, hover: f32, press: f32, pop: f32) -> Pose {
    let h = hover.clamp(0.0, 1.0);
    let p = press.clamp(0.0, 1.0);
    let (offset, angle) = match kick {
        Kick::Lift => (Vec2::new(0.0, -LIFT_PX * h), 0.0),
        Kick::Turn(rad) => (Vec2::ZERO, rad * h),
        Kick::Nudge(dir) => (dir.normalized() * NUDGE_PX * h, 0.0),
    };
    Pose {
        offset,
        scale: 1.0 - (1.0 - PRESS_SCALE) * p + (POP_SCALE - 1.0) * pop.clamp(0.0, 1.0),
        angle,
        tint: h,
    }
}

/// Ease `cue` into a pose. Reduced motion rests the glyph and only snaps the
/// tint, so a hovered card still reads as hovered.
pub fn pose(ui: &egui::Ui, id: egui::Id, cue: Cue, kick: Kick) -> Pose {
    if crate::motion::reduced_motion(ui) {
        return Pose {
            tint: if cue.hovered { 1.0 } else { 0.0 },
            ..Pose::REST
        };
    }
    let ctx = ui.ctx();
    let hover = ctx.animate_bool_with_time_and_easing(
        id.with("icon-h"),
        cue.hovered,
        HOVER_SECS,
        egui::emath::easing::cubic_out,
    );
    let press = ctx.animate_bool_with_time_and_easing(
        id.with("icon-p"),
        cue.pressed,
        PRESS_SECS,
        egui::emath::easing::quadratic_out,
    );
    // Linear clock; the envelope gives the pop its shape. Only turning on pops.
    let on = ctx.animate_bool_with_time(id.with("icon-a"), cue.active, POP_SECS);
    let pop = if cue.active { pop_envelope(on) } else { 0.0 };
    pose_at(kick, hover, press, pop)
}

/// Paint a Phosphor glyph into `rect` in `pose`. The size is kept to whole
/// points so a pop reuses a few cached font sizes instead of new ones per frame.
pub fn paint_glyph(painter: &egui::Painter, rect: Rect, glyph: &str, color: Color32, pose: Pose) {
    let size = ((rect.height() * 0.78).max(10.0) * pose.scale).round();
    let galley = painter.layout_no_wrap(glyph.to_owned(), egui::FontId::proportional(size), color);
    let c = rect.center() + pose.offset;
    let pos = c - galley.size() * 0.5;
    let shape = egui::epaint::TextShape::new(pos, galley, color)
        .with_angle_and_anchor(pose.angle, egui::Align2::CENTER_CENTER);
    painter.add(shape);
}

/// Card rects painted this pass and last pass, so an icon painted inside a
/// card (before the card knows it is hovered) can ask about the card.
#[derive(Clone, Debug, Default)]
struct CardRects {
    pass: u64,
    now: Vec<Rect>,
    prev: Vec<Rect>,
}

fn card_rects<R>(ctx: &egui::Context, f: impl FnOnce(&mut CardRects) -> R) -> R {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let rects = d.get_temp_mut_or_default::<CardRects>(egui::Id::new("motion-card-rects"));
        if rects.pass != pass {
            let last = std::mem::take(&mut rects.now);
            rects.prev = if rects.pass + 1 == pass {
                last
            } else {
                Vec::new()
            };
            rects.pass = pass;
        }
        f(rects)
    })
}

/// A card calls this with its rect once it is laid out.
pub fn note_card(ui: &egui::Ui, rect: Rect) {
    card_rects(ui.ctx(), |r| r.now.push(rect));
}

/// True when the pointer is on the card (from last pass) that holds `icon`.
pub fn card_hovered(ui: &egui::Ui, icon: Rect) -> bool {
    let cards = card_rects(ui.ctx(), |r| r.prev.clone());
    cards
        .iter()
        .any(|card| card.contains_rect(icon) && ui.rect_contains_pointer(*card))
}

/// A card's own icon: it lifts [`LIFT_PX`] and leans to the accent while the
/// card is hovered. The card itself stays still.
pub fn card_icon_pose(ui: &egui::Ui, id: egui::Id, icon: Rect) -> Pose {
    host_pose(ui, id, icon, false, Kick::Lift)
}

/// A glyph inside a chip or card it doesn't own: it answers the pointer on
/// that host, and pops when `active` turns on.
pub fn host_pose(ui: &egui::Ui, id: egui::Id, icon: Rect, active: bool, kick: Kick) -> Pose {
    let cue = Cue {
        hovered: card_hovered(ui, icon),
        pressed: false,
        active,
    };
    pose(ui, id, cue, kick)
}

/// `rect` moved and scaled by `pose` about its centre (no turn), for glyphs
/// drawn from strokes rather than a font.
pub fn posed_rect(rect: Rect, pose: Pose) -> Rect {
    Rect::from_center_size(rect.center() + pose.offset, rect.size() * pose.scale)
}

/// 0..1 between two states of a glyph (a caret closed → open), eased, or
/// snapped with reduced motion.
pub fn toggle_t(ui: &egui::Ui, id: egui::Id, on: bool) -> f32 {
    if crate::motion::reduced_motion(ui) {
        return if on { 1.0 } else { 0.0 };
    }
    ui.ctx()
        .animate_bool_with_time_and_easing(id, on, HOVER_SECS, egui::emath::easing::cubic_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(time: f64, pointer: Option<Pos2>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))),
            time: Some(time),
            // No pointer: no events at all, since any input event asks egui for a frame.
            events: pointer.map_or(Vec::new(), |p| vec![egui::Event::PointerMoved(p)]),
            ..Default::default()
        }
    }

    fn delay(out: &egui::FullOutput) -> std::time::Duration {
        out.viewport_output[&egui::ViewportId::ROOT].repaint_delay
    }

    #[test]
    fn durations_and_distances_stay_short() {
        assert_eq!(HOVER_SECS, 0.14);
        assert_eq!(PRESS_SECS, 0.08);
        assert_eq!(POP_SECS, 0.18);
        assert_eq!(LIFT_PX, 2.0);
        assert_eq!(NUDGE_PX, 2.0);
        assert_eq!(crate::motion::SETTINGS_SCROLL_SECS, 0.2);
        // Every event stays within 200 ms; the card lift is the ≤150 ms ease.
        for (secs, cap) in [(HOVER_SECS, 0.15), (PRESS_SECS, 0.2), (POP_SECS, 0.2)] {
            assert!(secs <= cap, "{secs}");
        }
    }

    #[test]
    fn pose_at_rest_and_at_full_hover() {
        assert_eq!(pose_at(Kick::Lift, 0.0, 0.0, 0.0), Pose::REST);
        let lifted = pose_at(Kick::Lift, 1.0, 0.0, 0.0);
        assert_eq!(lifted.offset, Vec2::new(0.0, -2.0));
        assert_eq!((lifted.scale, lifted.angle, lifted.tint), (1.0, 0.0, 1.0));
        let turned = pose_at(Kick::Turn(0.5), 1.0, 0.0, 0.0);
        assert_eq!((turned.offset, turned.angle), (Vec2::ZERO, 0.5));
        let nudged = pose_at(Kick::Nudge(Vec2::new(3.0, -4.0)), 1.0, 0.0, 0.0);
        assert!((nudged.offset - Vec2::new(1.2, -1.6)).length() < 1e-5);
        let pressed = pose_at(Kick::Lift, 0.0, 1.0, 0.0);
        assert!((pressed.scale - 0.88).abs() < 1e-6);
        let popped = pose_at(Kick::Lift, 0.0, 0.0, 1.0);
        assert!((popped.scale - 1.18).abs() < 1e-6);
    }

    #[test]
    fn the_pop_rises_and_returns() {
        assert_eq!(pop_envelope(0.0), 0.0);
        assert_eq!(pop_envelope(1.0), 0.0);
        assert!((pop_envelope(0.5) - 1.0).abs() < 1e-6);
        assert!(pop_envelope(0.25) > 0.7 && pop_envelope(0.25) < 0.71);
    }

    #[test]
    fn map_turns_and_scales_about_the_centre() {
        let c = Pos2::new(10.0, 10.0);
        let quarter = Pose {
            angle: std::f32::consts::FRAC_PI_2,
            ..Pose::REST
        };
        let p = quarter.map(c, Pos2::new(14.0, 10.0));
        assert!((p - Pos2::new(10.0, 14.0)).length() < 1e-4, "{p:?}");
        let grown = Pose {
            scale: 1.5,
            offset: Vec2::new(0.0, -2.0),
            ..Pose::REST
        };
        assert_eq!(grown.map(c, Pos2::new(14.0, 10.0)), Pos2::new(16.0, 8.0));
    }

    /// Hover eases over HOVER_SECS, asks for frames only while it moves, and
    /// then sits still with no further repaint.
    #[test]
    fn hover_eases_in_its_time_then_stops_asking_for_frames() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("timing-probe");
        let run = |time: f64, hovered: bool| {
            let mut got = Pose::REST;
            let out = crate::theme::test_pass(&ctx, raw(time, None), |ui| {
                let cue = Cue {
                    hovered,
                    ..Cue::default()
                };
                got = pose(ui, id, cue, Kick::Lift);
            });
            (got, delay(&out))
        };
        // egui asks for frames while it warms up (fonts, first layout).
        for warm in 0..3 {
            run(f64::from(warm) * 0.1, false);
        }
        let (rest, rest_delay) = run(1.0, false);
        assert_eq!(rest, Pose::REST);
        assert_eq!(
            rest_delay,
            std::time::Duration::MAX,
            "idle asks for nothing"
        );
        // Hover starts: the first frame of the ease asks for the next one.
        let (start, start_delay) = run(1.01, true);
        assert!(start.offset.y > -2.0, "{start:?}");
        assert_eq!(start_delay, std::time::Duration::ZERO);
        let (mid, mid_delay) = run(1.0 + f64::from(HOVER_SECS) * 0.5, true);
        assert!(mid.offset.y < -0.5 && mid.offset.y > -2.0, "{mid:?}");
        assert_eq!(mid_delay, std::time::Duration::ZERO);
        let (done, _) = run(1.0 + f64::from(HOVER_SECS) + 0.01, true);
        assert_eq!(done.offset, Vec2::new(0.0, -2.0));
        let (held, held_delay) = run(2.0, true);
        assert_eq!(held.offset, Vec2::new(0.0, -2.0));
        assert_eq!(
            held_delay,
            std::time::Duration::MAX,
            "a settled hover is idle"
        );
    }

    #[test]
    fn becoming_active_pops_once_and_rests() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("pop-probe");
        let scale_at = |time: f64, active: bool| {
            let mut got = 0.0;
            let _ = crate::theme::test_pass(&ctx, raw(time, None), |ui| {
                let cue = Cue {
                    active,
                    ..Cue::default()
                };
                got = pose(ui, id, cue, Kick::Lift).scale;
            });
            got
        };
        // Already active on the first frame (the page you start on): no pop.
        assert_eq!(scale_at(0.0, true), 1.0);
        assert_eq!(scale_at(0.5, false), 1.0);
        assert_eq!(scale_at(1.0, false), 1.0);
        scale_at(2.0, true);
        let peak = scale_at(2.0 + f64::from(POP_SECS) * 0.5, true);
        assert!(peak > 1.15, "mid-pop {peak}");
        assert_eq!(scale_at(2.0 + f64::from(POP_SECS) + 0.01, true), 1.0);
    }

    #[test]
    fn reduced_motion_rests_every_glyph_and_asks_for_no_frame() {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let id = egui::Id::new("reduced-probe");
        for (i, cue) in [
            Cue::default(),
            Cue {
                hovered: true,
                pressed: true,
                active: true,
            },
        ]
        .into_iter()
        .enumerate()
        {
            // egui asks for frames while it warms up (fonts, first layout).
            for step in 0..5 {
                let mut got = Pose::REST;
                let out =
                    crate::theme::test_pass(&ctx, raw(i as f64 + step as f64 * 0.03, None), |ui| {
                        got = pose(ui, id, cue, Kick::Turn(1.0));
                    });
                let want = Pose {
                    tint: if cue.hovered { 1.0 } else { 0.0 },
                    ..Pose::REST
                };
                assert_eq!(got, want);
                if step > 2 {
                    assert_eq!(delay(&out), std::time::Duration::MAX);
                }
            }
        }
    }

    /// A card's icon lifts when the pointer is anywhere on the card, from the
    /// card's rect the pass before; the card rect itself is not moved.
    #[test]
    fn a_hovered_card_lifts_only_its_icon() {
        let ctx = egui::Context::default();
        let card = Rect::from_min_size(Pos2::new(20.0, 20.0), egui::vec2(200.0, 120.0));
        let icon = Rect::from_min_size(Pos2::new(34.0, 34.0), egui::vec2(28.0, 28.0));
        let frame = |time: f64, pointer: Option<Pos2>| {
            let mut got = (false, Pose::REST);
            let _ = crate::theme::test_pass(&ctx, raw(time, pointer), |ui| {
                got = (
                    card_hovered(ui, icon),
                    card_icon_pose(ui, egui::Id::new("card-icon"), icon),
                );
                note_card(ui, card);
            });
            got
        };
        // The first pass has no card rect yet.
        assert!(!frame(0.0, Some(Pos2::new(180.0, 120.0))).0);
        // Pointer on the card's far corner, nowhere near the icon.
        let (on, _) = frame(0.1, Some(Pos2::new(180.0, 120.0)));
        assert!(on);
        let (_, lifted) = frame(
            0.1 + f64::from(HOVER_SECS) + 0.01,
            Some(Pos2::new(180.0, 120.0)),
        );
        assert_eq!(lifted.offset, Vec2::new(0.0, -LIFT_PX));
        assert_eq!(lifted.tint, 1.0);
        let (off, _) = frame(1.0, Some(Pos2::new(300.0, 250.0)));
        assert!(!off);
        let (_, rest) = frame(2.0, Some(Pos2::new(300.0, 250.0)));
        assert_eq!(rest, Pose::REST);
    }
}
