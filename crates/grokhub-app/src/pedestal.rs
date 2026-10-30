//! The holo pedestal the desktop companion stands on, drawn with the egui
//! painter rather than an image so it takes the theme's colours and scales
//! to any window. A flat elliptical disc with a bright rim, two thin rings
//! turning slowly in opposite directions, a soft light cone that fades out
//! around the companion's knees with faint scanlines drifting through it,
//! and a soft dark ellipse underneath so it reads on light wallpapers too.
//! Nothing here is fully holographic: the companion herself stays solid.
//!
//! Everything that moves is a pure function of the time `t`, and with
//! motion reduced every moving part sits still, so a caller can repaint at
//! whatever rate it likes (or not at all) without the drawing drifting.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

/// Seconds for the outer ring to turn once. The inner ring turns the other
/// way, a little faster.
pub const OUTER_RING_SECS: f64 = 14.0;
pub const INNER_RING_SECS: f64 = 9.0;
/// Seconds for the scanlines to drift one spacing step.
pub const SCANLINE_SECS: f64 = 1.6;
/// Space between scanlines in the light cone, in points.
pub const SCANLINE_STEP: f32 = 6.0;
/// Dashes on each ring and how much of a dash period is drawn.
const RING_DASHES: usize = 18;
const RING_DUTY: f32 = 0.55;
/// Points along each dash.
const DASH_POINTS: usize = 6;

/// The pedestal's colours. [`colors`] builds them from the theme, with the
/// companion's own tint when its file names one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PedestalColors {
    /// Bright rim and ring colour.
    pub rim: Color32,
    /// Light cone at its base (alpha fades to 0 at the top).
    pub glow: Color32,
    /// Disc fill.
    pub disc: Color32,
    /// Scanlines in the cone.
    pub scanline: Color32,
    /// Soft shadow under the disc.
    pub shadow: Color32,
}

/// Colours from the theme accent, or from the companion's `pedestal_tint`.
pub fn colors(tint: Option<Color32>) -> PedestalColors {
    let accent = tint.unwrap_or_else(crate::theme::amber);
    colors_from(accent)
}

/// Colours for one accent colour, independent of the live theme.
pub fn colors_from(accent: Color32) -> PedestalColors {
    let [r, g, b, _] = accent.to_array();
    PedestalColors {
        rim: Color32::from_rgb(r, g, b),
        glow: Color32::from_rgba_unmultiplied(r, g, b, 72),
        disc: Color32::from_rgba_unmultiplied(r / 5, g / 5, b / 5, 200),
        scanline: Color32::from_rgba_unmultiplied(r, g, b, 34),
        shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 110),
    }
}

/// Where the parts go inside a rect, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Centre of the disc.
    pub center: Pos2,
    /// Disc half width and half height.
    pub radius: Vec2,
    /// Top of the light cone (the knees).
    pub cone_top: f32,
    /// Half width of the cone at its top.
    pub cone_top_half_w: f32,
    /// Where the companion's feet go: the disc's visual top edge.
    pub feet: Pos2,
}

/// The disc sits at the bottom of `rect`, about 84% of its width, flattened
/// to the look of a platform seen from standing height. The cone climbs to
/// a bit under half the rect, where the companion's knees are.
pub fn layout(rect: Rect) -> Layout {
    let rx = (rect.width() * 0.42).max(8.0);
    let ry = (rx * 0.28).max(3.0);
    let center = Pos2::new(rect.center().x, rect.bottom() - ry - 6.0);
    Layout {
        center,
        radius: Vec2::new(rx, ry),
        cone_top: (rect.bottom() - rect.height() * 0.46).min(center.y - ry),
        cone_top_half_w: rx * 0.62,
        feet: Pos2::new(center.x, center.y - ry * 0.35),
    }
}

/// Rotation of the outer and inner rings at `t` seconds, in radians. Still
/// at zero with motion reduced.
pub fn ring_angles(t: f64, still: bool) -> (f32, f32) {
    if still {
        return (0.0, 0.0);
    }
    let tau = std::f64::consts::TAU;
    let outer = (t / OUTER_RING_SECS).rem_euclid(1.0) * tau;
    let inner = -(t / INNER_RING_SECS).rem_euclid(1.0) * tau;
    (outer as f32, inner as f32)
}

/// How far the scanlines have drifted down into their next step, 0..1.
pub fn scanline_phase(t: f64, still: bool) -> f32 {
    if still {
        0.0
    } else {
        (t / SCANLINE_SECS).rem_euclid(1.0) as f32
    }
}

/// How long to wait before the next repaint while the pedestal animates:
/// about 15 fps is plenty for rings this slow. `None` with motion reduced,
/// since nothing moves.
pub fn repaint_after(still: bool) -> Option<std::time::Duration> {
    (!still).then(|| std::time::Duration::from_millis(66))
}

/// Draw the pedestal into `rect` at time `t`.
pub fn paint(painter: &egui::Painter, rect: Rect, t: f64, still: bool, c: &PedestalColors) {
    let l = layout(rect);
    paint_shadow(painter, &l, c);
    paint_cone(painter, &l, scanline_phase(t, still), c);
    painter.add(egui::Shape::ellipse_filled(l.center, l.radius, c.disc));
    // Back half of the rim brighter, like light catching the far edge.
    painter.add(egui::Shape::ellipse_stroke(
        l.center,
        l.radius,
        Stroke::new(1.5, c.rim.gamma_multiply(0.55)),
    ));
    let back = ellipse_points(
        l.center,
        l.radius,
        std::f32::consts::PI,
        std::f32::consts::TAU,
        24,
    );
    painter.add(egui::Shape::line(back, Stroke::new(2.0, c.rim)));
    let (outer, inner) = ring_angles(t, still);
    paint_ring(
        painter,
        l.center,
        l.radius * 1.14,
        outer,
        Stroke::new(1.0, c.rim.gamma_multiply(0.7)),
    );
    paint_ring(
        painter,
        l.center,
        l.radius * 0.72,
        inner,
        Stroke::new(1.0, c.rim.gamma_multiply(0.45)),
    );
}

fn paint_shadow(painter: &egui::Painter, l: &Layout, c: &PedestalColors) {
    let center = l.center + Vec2::new(0.0, l.radius.y * 0.45);
    for (scale, alpha) in [(1.35, 0.25), (1.2, 0.45), (1.08, 0.8)] {
        painter.add(egui::Shape::ellipse_filled(
            center,
            l.radius * scale,
            c.shadow.gamma_multiply(alpha),
        ));
    }
}

/// The light cone: a trapezoid whose colour fades to nothing at the top,
/// plus the scanlines drifting down through it.
fn paint_cone(painter: &egui::Painter, l: &Layout, phase: f32, c: &PedestalColors) {
    let base_y = l.center.y - l.radius.y * 0.2;
    if l.cone_top >= base_y {
        return;
    }
    let mut mesh = egui::Mesh::with_texture(egui::TextureId::default());
    let clear = Color32::from_rgba_unmultiplied(c.glow.r(), c.glow.g(), c.glow.b(), 0);
    mesh.colored_vertex(Pos2::new(l.center.x - l.radius.x * 0.92, base_y), c.glow);
    mesh.colored_vertex(Pos2::new(l.center.x + l.radius.x * 0.92, base_y), c.glow);
    mesh.colored_vertex(Pos2::new(l.center.x + l.cone_top_half_w, l.cone_top), clear);
    mesh.colored_vertex(Pos2::new(l.center.x - l.cone_top_half_w, l.cone_top), clear);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));

    for (y, half_w, fade) in scanlines(l, phase, base_y) {
        let color = c.scanline.gamma_multiply(fade);
        painter.line_segment(
            [
                Pos2::new(l.center.x - half_w, y),
                Pos2::new(l.center.x + half_w, y),
            ],
            Stroke::new(1.0, color),
        );
    }
}

/// Scanline rows in the cone: (y, half width at that height, strength
/// 0..1 fading towards the top). `phase` slides them down one step.
pub fn scanlines(l: &Layout, phase: f32, base_y: f32) -> Vec<(f32, f32, f32)> {
    let span = base_y - l.cone_top;
    if span <= SCANLINE_STEP {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut y = l.cone_top + phase * SCANLINE_STEP;
    while y < base_y {
        let up = (base_y - y) / span; // 1 at the top, 0 at the base
        let half_w = l.radius.x * 0.92 + (l.cone_top_half_w - l.radius.x * 0.92) * up;
        out.push((y, half_w, (1.0 - up).powf(1.5)));
        y += SCANLINE_STEP;
    }
    out
}

/// A dashed ellipse: [`RING_DASHES`] dashes, each [`RING_DUTY`] of its
/// period, starting at `angle`.
fn paint_ring(painter: &egui::Painter, center: Pos2, radius: Vec2, angle: f32, stroke: Stroke) {
    let period = std::f32::consts::TAU / RING_DASHES as f32;
    for i in 0..RING_DASHES {
        let a0 = angle + i as f32 * period;
        let a1 = a0 + period * RING_DUTY;
        painter.add(egui::Shape::line(
            ellipse_points(center, radius, a0, a1, DASH_POINTS),
            stroke,
        ));
    }
}

/// `n` points along an ellipse from angle `a0` to `a1`.
pub fn ellipse_points(center: Pos2, radius: Vec2, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    let n = n.max(2);
    (0..n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / (n - 1) as f32;
            Pos2::new(center.x + radius.x * a.cos(), center.y + radius.y * a.sin())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_disc_sits_at_the_bottom_and_the_cone_reaches_the_knees() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(320.0, 480.0));
        let l = layout(rect);
        assert_eq!(l.radius, Vec2::new(134.4, 37.632));
        assert_eq!(l.center.x, 170.0);
        assert!((l.center.y - (500.0 - 37.632 - 6.0)).abs() < 1e-3);
        // Knees: 46% of the height up from the bottom.
        assert!((l.cone_top - (500.0 - 220.8)).abs() < 1e-3);
        assert!(l.cone_top < l.center.y - l.radius.y);
        assert!(l.cone_top_half_w < l.radius.x);
        assert!(l.feet.y < l.center.y && l.feet.y > l.center.y - l.radius.y);
        // A tiny rect still gives a drawable disc.
        let tiny = layout(Rect::from_min_size(Pos2::ZERO, Vec2::new(4.0, 4.0)));
        assert_eq!(tiny.radius, Vec2::new(8.0, 3.0));
    }

    #[test]
    fn rings_turn_opposite_ways_and_hold_still_with_motion_reduced() {
        let (o0, i0) = ring_angles(0.0, false);
        assert_eq!((o0, i0), (0.0, 0.0));
        let (o, i) = ring_angles(OUTER_RING_SECS / 4.0, false);
        assert!((o - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!(i < 0.0, "inner ring turns the other way");
        let (o_full, _) = ring_angles(OUTER_RING_SECS, false);
        assert!(o_full.abs() < 1e-5, "wraps every turn");
        assert_eq!(ring_angles(123.4, true), (0.0, 0.0));
        assert_eq!(scanline_phase(SCANLINE_SECS / 2.0, false), 0.5);
        assert_eq!(scanline_phase(SCANLINE_SECS / 2.0, true), 0.0);
        assert_eq!(repaint_after(true), None);
        assert_eq!(
            repaint_after(false),
            Some(std::time::Duration::from_millis(66))
        );
    }

    #[test]
    fn scanlines_fill_the_cone_narrowing_and_fading_upwards() {
        let l = layout(Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 480.0)));
        let base_y = l.center.y - l.radius.y * 0.2;
        let rows = scanlines(&l, 0.0, base_y);
        assert!(rows.len() > 20, "{}", rows.len());
        assert_eq!(rows[0].0, l.cone_top);
        assert!(rows[0].1 <= l.cone_top_half_w + 1e-3);
        assert!(rows[0].2 < 0.01, "top row is nearly invisible");
        let last = rows.last().unwrap();
        assert!(last.0 < base_y);
        assert!(last.1 > rows[0].1, "wider near the base");
        assert!(last.2 > rows[0].2, "stronger near the base");
        let shifted = scanlines(&l, 0.5, base_y);
        assert!((shifted[0].0 - (l.cone_top + SCANLINE_STEP * 0.5)).abs() < 1e-5);
        let flat = Layout {
            cone_top: base_y - 2.0,
            ..l
        };
        assert!(scanlines(&flat, 0.0, base_y).is_empty());
    }

    #[test]
    fn colours_follow_the_accent() {
        let c = colors_from(Color32::from_rgb(0xff, 0xb0, 0x20));
        assert_eq!(c.rim, Color32::from_rgb(0xff, 0xb0, 0x20));
        assert_eq!(c.glow.a(), 72);
        assert_eq!(c.scanline.a(), 34);
        assert_eq!(
            c.disc,
            Color32::from_rgba_unmultiplied(0x33, 0x23, 0x06, 200)
        );
        assert_eq!(c.shadow, Color32::from_rgba_unmultiplied(0, 0, 0, 110));
        let blue = colors_from(Color32::from_rgb(0x5a, 0xd1, 0xff));
        assert_ne!(blue.rim, c.rim);
        assert_eq!(
            colors(Some(Color32::from_rgb(1, 2, 3))).rim,
            Color32::from_rgb(1, 2, 3)
        );
        assert_eq!(colors(None).rim, {
            let [r, g, b, _] = crate::theme::amber().to_array();
            Color32::from_rgb(r, g, b)
        });
    }

    #[test]
    fn ellipse_points_run_from_one_angle_to_the_other() {
        let pts = ellipse_points(
            Pos2::new(10.0, 10.0),
            Vec2::new(4.0, 2.0),
            0.0,
            std::f32::consts::PI,
            3,
        );
        assert_eq!(pts.len(), 3);
        assert!((pts[0].x - 14.0).abs() < 1e-5 && (pts[0].y - 10.0).abs() < 1e-5);
        assert!((pts[1].x - 10.0).abs() < 1e-5 && (pts[1].y - 12.0).abs() < 1e-5);
        assert!((pts[2].x - 6.0).abs() < 1e-5 && (pts[2].y - 10.0).abs() < 1e-4);
        assert_eq!(
            ellipse_points(Pos2::ZERO, Vec2::splat(1.0), 0.0, 1.0, 0).len(),
            2
        );
    }

    #[test]
    fn painting_adds_shapes_without_panicking() {
        egui::__run_test_ui(|ui| {
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 480.0));
            let c = colors_from(Color32::from_rgb(0xff, 0xb0, 0x20));
            paint(ui.painter(), rect, 3.3, false, &c);
            paint(ui.painter(), rect, 3.3, true, &c);
            paint(
                ui.painter(),
                Rect::from_min_size(Pos2::ZERO, Vec2::splat(2.0)),
                0.0,
                false,
                &c,
            );
        });
    }
}
