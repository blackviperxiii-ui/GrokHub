//! Spike-2a: the six Cua Driver cursor motions as travel paths for the agent
//! cursor marker. Pure geometry; the cabin picks one style from a constant
//! (no user setting) and reduced motion snaps before any of this runs.

/// Cua Driver's cursor motion styles (`cua-driver-rs` 0.34.0 names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorMotion {
    /// Curved travel, a small overshoot, then a settle. Cua's default.
    SignatureArc,
    /// Straight travel that springs past the target and rings down.
    SpringSettle,
    /// Slow approach, then a pull onto the target.
    Magnetic,
    /// A wide swoop with no overshoot.
    CometSwoop,
    /// Classic for short hops, the signature arc for long travel.
    Adaptive,
    /// Straight ease-out travel.
    Classic,
}

/// Every style, in Cua's order.
pub const CURSOR_MOTIONS: [CursorMotion; 6] = [
    CursorMotion::SignatureArc,
    CursorMotion::SpringSettle,
    CursorMotion::Magnetic,
    CursorMotion::CometSwoop,
    CursorMotion::Adaptive,
    CursorMotion::Classic,
];

/// Below this travel (pixels) `Adaptive` goes straight.
pub const ADAPTIVE_SHORT_PX: f32 = 120.0;

impl CursorMotion {
    /// The name Cua Driver uses for this style.
    pub fn cua_name(self) -> &'static str {
        match self {
            Self::SignatureArc => "signature_arc",
            Self::SpringSettle => "spring_settle",
            Self::Magnetic => "magnetic",
            Self::CometSwoop => "comet_swoop",
            Self::Adaptive => "adaptive",
            Self::Classic => "classic",
        }
    }
}

/// The cabin's motion constants a path needs (`motion.rs` owns the values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TravelShape {
    /// Bulge perpendicular to travel (pixels).
    pub arc_px: f32,
    /// Push past the target before settling (pixels).
    pub overshoot_px: f32,
    /// Share of the travel spent settling back onto the target.
    pub settle_frac: f32,
}

fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t
}

fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Point on the travel path at progress `u` (0..1). `u >= 1` is exactly `to`.
pub fn travel_point(motion: CursorMotion, from: (f32, f32), to: (f32, f32), u: f32, shape: TravelShape) -> (f32, f32) {
    if u >= 1.0 {
        return to;
    }
    let u = u.max(0.0);
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = if len > 0.0 { (dx / len, dy / len) } else { (0.0, 0.0) };
    // Perpendicular to travel, for arcs.
    let (px, py) = (-uy, ux);
    let along = |s: f32| (from.0 + dx * s, from.1 + dy * s);
    match motion {
        CursorMotion::Adaptive => {
            let pick = if len < ADAPTIVE_SHORT_PX { CursorMotion::Classic } else { CursorMotion::SignatureArc };
            travel_point(pick, from, to, u, shape)
        }
        CursorMotion::Classic => along(ease_out_cubic(u)),
        CursorMotion::SignatureArc => {
            let settle = shape.settle_frac.clamp(0.05, 0.4);
            let over = (to.0 + ux * shape.overshoot_px, to.1 + uy * shape.overshoot_px);
            if u < 1.0 - settle {
                let v = u / (1.0 - settle);
                let e = ease_out_cubic(v);
                let arc = 4.0 * v * (1.0 - v) * shape.arc_px;
                let reach = len + shape.overshoot_px;
                (from.0 + ux * reach * e + px * arc, from.1 + uy * reach * e + py * arc)
            } else {
                let v = ease_out_cubic((u - (1.0 - settle)) / settle);
                (over.0 + (to.0 - over.0) * v, over.1 + (to.1 - over.1) * v)
            }
        }
        CursorMotion::SpringSettle => {
            // Rings past the target and back, damped to rest at u = 1.
            let s = 1.0 - (-6.0 * u).exp() * (3.0 * std::f32::consts::PI * u).cos() * (1.0 - u);
            along(s)
        }
        CursorMotion::Magnetic => {
            let s = if u < 0.6 { 0.55 * ease_out_cubic(u / 0.6) } else { 0.55 + 0.45 * ease_in_cubic((u - 0.6) / 0.4) };
            along(s)
        }
        CursorMotion::CometSwoop => {
            let s = ease_in_out_cubic(u);
            let arc = (std::f32::consts::PI * u).sin() * shape.arc_px * 2.5;
            let (x, y) = along(s);
            (x + px * arc, y + py * arc)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHAPE: TravelShape = TravelShape { arc_px: 10.0, overshoot_px: 6.0, settle_frac: 0.36 };

    #[test]
    fn every_style_starts_at_from_and_ends_exactly_at_the_target() {
        let from = (10.0, 20.0);
        let to = (250.0, 140.0);
        for m in CURSOR_MOTIONS {
            assert_eq!(travel_point(m, from, to, 1.0, SHAPE), to, "{}", m.cua_name());
            assert_eq!(travel_point(m, from, to, 1.7, SHAPE), to, "{}", m.cua_name());
            let (x, y) = travel_point(m, from, to, 0.0, SHAPE);
            assert!((x - from.0).abs() < 0.01 && (y - from.1).abs() < 0.01, "{}: ({x},{y})", m.cua_name());
            let (x, y) = travel_point(m, from, to, 0.999, SHAPE);
            assert!((x - to.0).abs() < 1.0 && (y - to.1).abs() < 1.0, "{} lands near the end: ({x},{y})", m.cua_name());
        }
    }

    #[test]
    fn styles_take_different_paths() {
        let from = (0.0, 0.0);
        let to = (200.0, 0.0);
        let mid: Vec<(f32, f32)> = CURSOR_MOTIONS.iter().map(|m| travel_point(*m, from, to, 0.5, SHAPE)).collect();
        // Classic and the spring stay on the line; the arcs bulge off it.
        assert_eq!(mid[5].1, 0.0);
        assert_eq!(mid[1].1, 0.0);
        assert!(mid[0].1 > 5.0, "signature arc bulges: {:?}", mid[0]);
        assert!(mid[3].1 > mid[0].1, "comet swoops wider than the arc: {:?}", mid[3]);
        // Adaptive is the arc on long travel and classic on a short hop.
        assert_eq!(mid[4], mid[0]);
        let short = (60.0, 0.0);
        assert_eq!(
            travel_point(CursorMotion::Adaptive, from, short, 0.5, SHAPE),
            travel_point(CursorMotion::Classic, from, short, 0.5, SHAPE)
        );
        // Magnetic hangs back mid-travel, then snaps in.
        assert!(mid[2].0 < mid[5].0, "magnetic {:?} vs classic {:?}", mid[2], mid[5]);
        // The spring passes the target before it settles.
        let peak = (1..100).map(|i| travel_point(CursorMotion::SpringSettle, from, to, i as f32 / 100.0, SHAPE).0).fold(0.0, f32::max);
        assert!(peak > 200.0, "spring overshoots: {peak}");
    }

    #[test]
    fn cua_names_are_the_driver_names() {
        let names: Vec<&str> = CURSOR_MOTIONS.iter().map(|m| m.cua_name()).collect();
        assert_eq!(names, ["signature_arc", "spring_settle", "magnetic", "comet_swoop", "adaptive", "classic"]);
    }
}
