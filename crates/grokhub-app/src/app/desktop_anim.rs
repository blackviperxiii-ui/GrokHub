//! Idle life for the desktop companion ([`super::desktop_nyx`]): a slow
//! breath through the torso, a float bob over the pedestal, a blink every
//! few seconds when the outfit has an eyes-closed patch, and a scanline
//! crossfade when her outfit changes. Everything is a function of time so
//! it is tested without a display and sits still under reduced motion.

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

/// One breath in and out.
pub const BREATH_SECS: f64 = 4.0;
/// How far the chest and head rise at the top of a breath, as a share of
/// the picture's height.
pub const BREATH_LIFT: f32 = 0.012;
/// How much wider the chest gets at the top of a breath.
pub const BREATH_SWELL: f32 = 0.006;
/// The float bob over the pedestal: amplitude and period.
pub const BOB_PX: f32 = 3.0;
pub const BOB_SECS: f64 = 5.0;
/// Blinks come every 3 to 7 seconds and last 120 ms.
pub const BLINK_MIN_SECS: f64 = 3.0;
pub const BLINK_MAX_SECS: f64 = 7.0;
pub const BLINK_SECS: f64 = 0.12;
/// An outfit change crossfades over this long with a scanline sweep.
pub const FADE_SECS: f64 = 0.45;

/// Where the picture bends: `uv` is the share of the picture's height
/// from the top (0 at the hair, 1 at the feet). Below the hips nothing
/// moves; the chest rises and swells; the head rides on top.
const UV_HIPS: f32 = 0.56;
const UV_CHEST: f32 = 0.30;

/// The breath phase, 0 at rest to 1 fully in. Still: 0.
pub fn breath(t: f64, still: bool) -> f32 {
    if still {
        return 0.0;
    }
    let s = (t * std::f64::consts::TAU / BREATH_SECS).sin() as f32;
    s * 0.5 + 0.5
}

/// The float bob in points, positive down. Still: 0.
pub fn bob(t: f64, still: bool) -> f32 {
    if still {
        return 0.0;
    }
    ((t * std::f64::consts::TAU / BOB_SECS).sin() as f32) * BOB_PX
}

/// The picture split into bands for breathing: for each row, its `uv.y`
/// in the texture, its `y` on screen and how far each side of the row's
/// centre is pushed out (`x_scale` of 1 is as drawn).
#[derive(Clone, Debug, PartialEq)]
pub struct Rows {
    pub uv: [f32; 4],
    pub y: [f32; 4],
    pub x_scale: [f32; 4],
}

/// Rows of `art` at breath phase `b`: feet and hips fixed, chest and head
/// lifted by `b * BREATH_LIFT` of the height, chest swollen by
/// `b * BREATH_SWELL`.
pub fn breath_rows(art: Rect, b: f32) -> Rows {
    let h = art.height();
    let lift = b * BREATH_LIFT * h;
    let y_at = |uv: f32| art.top() + uv * h;
    Rows {
        uv: [0.0, UV_CHEST, UV_HIPS, 1.0],
        y: [
            y_at(0.0) - lift,
            y_at(UV_CHEST) - lift,
            y_at(UV_HIPS),
            y_at(1.0),
        ],
        x_scale: [1.0, 1.0 + b * BREATH_SWELL, 1.0, 1.0],
    }
}

/// The picture as a textured mesh bent by [`breath_rows`], tinted (and
/// faded) by `tint`.
pub fn breath_mesh(tex: egui::TextureId, art: Rect, b: f32, tint: Color32) -> egui::Mesh {
    let rows = breath_rows(art, b);
    let mut mesh = egui::Mesh::with_texture(tex);
    let cx = art.center().x;
    let half = art.width() * 0.5;
    for i in 0..4 {
        let w = half * rows.x_scale[i];
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(cx - w, rows.y[i]),
            uv: egui::pos2(0.0, rows.uv[i]),
            color: tint,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(cx + w, rows.y[i]),
            uv: egui::pos2(1.0, rows.uv[i]),
            color: tint,
        });
    }
    for band in 0..3u32 {
        let a = band * 2;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    mesh
}

/// How much the head and eyes have risen at breath phase `b` for a
/// picture `art` tall, in points (positive up).
pub fn head_lift(art: Rect, b: f32) -> f32 {
    b * BREATH_LIFT * art.height()
}

/// The gap before blink number `i`: a fixed pseudo-random spread between
/// [`BLINK_MIN_SECS`] and [`BLINK_MAX_SECS`], the same on every run.
pub fn blink_gap(i: u32) -> f64 {
    let x = (f64::from(i) + 1.0) * 12.9898;
    let frac = (x.sin() * 43758.5453).fract().abs();
    BLINK_MIN_SECS + (BLINK_MAX_SECS - BLINK_MIN_SECS) * frac
}

/// When the blink that is current or next at time `t` starts, walked from
/// the first blink: the reference the [`BlinkCursor`] tests check against.
#[cfg(test)]
pub fn blink_start(t: f64) -> f64 {
    let mut cursor = BlinkCursor::default();
    cursor.start_at(t)
}

/// The blink that was current or next the last time the companion was
/// painted: its index and start. A frame steps it forward from there, so
/// after hours up it costs one gap per blink that passed, not one per
/// blink since launch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlinkCursor {
    index: u32,
    start: f64,
    /// When the blink before `index` ended; a `t` before it rewinds.
    floor: f64,
}

impl Default for BlinkCursor {
    fn default() -> Self {
        Self {
            index: 0,
            start: blink_gap(0),
            floor: 0.0,
        }
    }
}

impl BlinkCursor {
    /// When the blink that is current or next at `t` starts, stepping from
    /// the cached blink with [`blink_gap`].
    pub fn start_at(&mut self, t: f64) -> f64 {
        self.start_at_with(t, blink_gap)
    }

    /// [`Self::start_at`] with the gap table injected, so a test can count
    /// how many gaps one frame asks for.
    pub fn start_at_with(&mut self, t: f64, gap: impl Fn(u32) -> f64) -> f64 {
        if t < self.floor {
            *self = Self {
                index: 0,
                start: gap(0),
                floor: 0.0,
            };
        }
        while self.start + BLINK_SECS <= t {
            self.floor = self.start + BLINK_SECS;
            self.index += 1;
            self.start += gap(self.index);
        }
        self.start
    }

    /// Whether the eyes are closed at `t`. Still: never.
    pub fn closed(&mut self, t: f64, still: bool) -> bool {
        if still {
            return false;
        }
        let start = self.start_at(t);
        t >= start && t < start + BLINK_SECS
    }

    /// Seconds until the eyes next change, for the repaint schedule.
    pub fn next_change(&mut self, t: f64) -> f64 {
        let start = self.start_at(t);
        if t < start {
            start - t
        } else {
            start + BLINK_SECS - t
        }
    }
}

/// Where an outfit patch sits on screen: its anchor and size are in the
/// outfit picture's own pixels, mapped into `art` and lifted with the head.
pub fn patch_rect(
    art: Rect,
    tex_size: [usize; 2],
    anchor: Pos2,
    patch_size: [usize; 2],
    lift: f32,
) -> Rect {
    let sx = art.width() / tex_size[0].max(1) as f32;
    let sy = art.height() / tex_size[1].max(1) as f32;
    let min = Pos2::new(art.left() + anchor.x * sx, art.top() + anchor.y * sy - lift);
    Rect::from_min_size(
        min,
        Vec2::new(patch_size[0] as f32 * sx, patch_size[1] as f32 * sy),
    )
}

/// The crossfade's progress at `t` for a change that began at `start`,
/// eased, 0 to 1. Still: 1 at once.
pub fn fade_k(t: f64, start: f64, still: bool) -> f32 {
    if still {
        return 1.0;
    }
    let x = ((t - start) / FADE_SECS).clamp(0.0, 1.0) as f32;
    x * x * (3.0 - 2.0 * x)
}

/// Whether a fade begun at `start` is still running at `t`.
pub fn fading(t: f64, start: f64) -> bool {
    t - start < FADE_SECS
}

/// The bright scanline band sweeping down `art` during a fade at
/// progress `k`: a tenth of the height tall, from above the top to below
/// the bottom so it enters and leaves cleanly.
pub fn scan_band(art: Rect, k: f32) -> Rect {
    let h = art.height() * 0.1;
    let y = art.top() - h + k * (art.height() + h);
    Rect::from_min_size(Pos2::new(art.left(), y), Vec2::new(art.width(), h))
}

/// A white tint at `alpha` for fading a picture in or out.
pub fn fade_tint(alpha: f32) -> Color32 {
    Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breathing_is_a_four_second_sine_and_stops_under_reduced_motion() {
        assert!((breath(0.0, false) - 0.5).abs() < 1e-5);
        assert!(
            (breath(1.0, false) - 1.0).abs() < 1e-5,
            "top of the breath at a quarter"
        );
        assert!(
            (breath(3.0, false) - 0.0).abs() < 1e-5,
            "out at three quarters"
        );
        assert!(
            (breath(4.0, false) - breath(0.0, false)).abs() < 1e-5,
            "4 s period"
        );
        assert_eq!(breath(1.0, true), 0.0);
        assert_eq!(bob(1.25, false), BOB_PX);
        assert_eq!(bob(1.25, true), 0.0);
    }

    #[test]
    fn the_torso_lifts_and_swells_above_the_hips_only() {
        let art = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(100.0, 200.0));
        let rest = breath_rows(art, 0.0);
        assert_eq!(rest.y, [20.0, 80.0, 132.0, 220.0]);
        assert_eq!(rest.x_scale, [1.0; 4]);
        let full = breath_rows(art, 1.0);
        assert_eq!(full.uv, rest.uv);
        assert_eq!(full.y[2], 132.0, "hips fixed");
        assert_eq!(full.y[3], 220.0, "feet fixed");
        assert!((full.y[0] - (20.0 - 2.4)).abs() < 1e-4, "head up 1.2 %");
        assert!((full.y[1] - (80.0 - 2.4)).abs() < 1e-4, "chest up the same");
        assert!((full.x_scale[1] - 1.006).abs() < 1e-5, "chest swells");
        assert_eq!(full.x_scale[0], 1.0);
        assert!((head_lift(art, 1.0) - 2.4).abs() < 1e-4);
        let mesh = breath_mesh(egui::TextureId::default(), art, 1.0, Color32::WHITE);
        assert_eq!(mesh.vertices.len(), 8);
        assert_eq!(mesh.indices.len(), 18, "three bands, two triangles each");
        assert_eq!(mesh.vertices[7].pos, Pos2::new(110.0, 220.0));
        assert_eq!(mesh.vertices[7].uv, egui::pos2(1.0, 1.0));
    }

    #[test]
    fn blinks_come_every_three_to_seven_seconds_for_120_ms() {
        for i in 0..200 {
            let g = blink_gap(i);
            assert!((BLINK_MIN_SECS..=BLINK_MAX_SECS).contains(&g), "{i}: {g}");
        }
        assert_ne!(blink_gap(0), blink_gap(1), "not a metronome");
        let first = blink_start(0.0);
        assert_eq!(first, blink_gap(0));
        let mut eyes = BlinkCursor::default();
        assert!(!eyes.closed(first - 0.01, false));
        assert!(eyes.closed(first, false));
        assert!(eyes.closed(first + 0.11, false));
        assert!(!eyes.closed(first + 0.13, false));
        assert!(!eyes.closed(first, true), "still: eyes stay open");
        let second = blink_start(first + 0.2);
        assert!((second - (first + blink_gap(1))).abs() < 1e-9);
        assert!((eyes.next_change(0.0) - first).abs() < 1e-9);
        assert!((eyes.next_change(first + 0.05) - 0.07).abs() < 1e-9);
        // A long session still answers quickly and stays in step.
        let late = blink_start(3600.0);
        assert!((3600.0 - BLINK_SECS..3600.0 + BLINK_MAX_SECS).contains(&late));
    }

    #[test]
    fn a_frame_a_week_in_steps_the_cursor_instead_of_walking_every_blink() {
        use std::cell::Cell;
        let week = 7.0 * 24.0 * 3600.0;
        let gaps = Cell::new(0u32);
        let counted = |i: u32| {
            gaps.set(gaps.get() + 1);
            blink_gap(i)
        };
        let mut cursor = BlinkCursor::default();
        assert_eq!(cursor.start_at_with(week, counted), blink_start(week));
        // Warming up walks the week once (about 121k blinks at 5 s each).
        let warm = gaps.get();
        assert!((100_000..150_000).contains(&warm), "{warm}");
        // The next frame, 16 ms on, asks for no gap at all.
        gaps.set(0);
        assert_eq!(
            cursor.start_at_with(week + 0.016, counted),
            blink_start(week + 0.016)
        );
        assert_eq!(gaps.get(), 0);
        // A frame a minute later asks for one gap per blink that passed.
        gaps.set(0);
        assert_eq!(
            cursor.start_at_with(week + 60.0, counted),
            blink_start(week + 60.0)
        );
        assert!((8..=20).contains(&gaps.get()), "{}", gaps.get());
        let start = blink_start(week + 60.0);
        assert_eq!(
            cursor.closed(week + 60.0, false),
            (start..start + BLINK_SECS).contains(&(week + 60.0))
        );
        assert!(cursor.next_change(week + 60.0) > 0.0);
        // Time running backwards (a clock reset) rewinds to the first blink.
        assert_eq!(cursor.start_at(0.0), blink_gap(0));
    }

    #[test]
    fn a_patch_lands_where_the_outfit_pixels_say_and_rides_the_breath() {
        let art = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(200.0, 400.0));
        let r = patch_rect(art, [400, 800], Pos2::new(100.0, 200.0), [80, 40], 0.0);
        assert_eq!(
            r,
            Rect::from_min_size(Pos2::new(50.0, 200.0), Vec2::new(40.0, 20.0))
        );
        let lifted = patch_rect(art, [400, 800], Pos2::new(100.0, 200.0), [80, 40], 2.4);
        assert!((lifted.top() - 197.6).abs() < 1e-4);
    }

    #[test]
    fn the_crossfade_eases_over_450_ms_and_sweeps_a_scanline() {
        assert_eq!(fade_k(10.0, 10.0, false), 0.0);
        assert!((fade_k(10.225, 10.0, false) - 0.5).abs() < 1e-5, "half way");
        assert_eq!(fade_k(10.45, 10.0, false), 1.0);
        assert_eq!(fade_k(10.0, 10.0, true), 1.0, "still: no fade");
        assert!(fading(10.4, 10.0));
        assert!(!fading(10.5, 10.0));
        let art = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(200.0, 400.0));
        let start = scan_band(art, 0.0);
        assert_eq!(start.bottom(), art.top(), "enters from above");
        let end = scan_band(art, 1.0);
        assert_eq!(end.top(), art.bottom(), "leaves below");
        assert_eq!(scan_band(art, 0.5).center().y, art.center().y);
        assert_eq!(fade_tint(1.0), Color32::WHITE);
        assert_eq!(fade_tint(0.0).a(), 0);
        assert_eq!(fade_tint(0.5).a(), 128);
    }
}
