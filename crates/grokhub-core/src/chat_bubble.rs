//! Chat bubbles hug their text and wrap with the chat pane.

pub const BUBBLE_MAX_FRAC: f32 = 0.84;
pub const BUBBLE_PAD_X: f32 = 12.0;
pub const BUBBLE_PAD_Y: f32 = 8.0;
pub const BUBBLE_RADIUS: f32 = 16.0;
const ROW_SANE_MAX: f32 = 1600.0;
const ROW_FALLBACK: f32 = 640.0;
const ROW_MIN: f32 = 160.0;

/// Scroll areas sometimes report infinite or huge `available_width`. Treat those as a normal pane.
pub fn clamp_row_width(available: f32) -> f32 {
    if !available.is_finite() || available <= 0.0 {
        ROW_FALLBACK
    } else {
        available.min(ROW_SANE_MAX)
    }
}

/// Wrap cap for a bubble on this row. Long text wraps here; short text must not stretch to it.
pub fn bubble_max_width(available: f32) -> f32 {
    let avail = clamp_row_width(available);
    if avail < ROW_MIN {
        avail
    } else {
        (avail * BUBBLE_MAX_FRAC).clamp(ROW_MIN, avail)
    }
}

pub fn bubble_wrap_width(available: f32, pad_x: f32) -> f32 {
    (bubble_max_width(available) - pad_x * 2.0).max(1.0)
}

/// Outer bubble width: hug `content_width`, never exceed the row cap.
pub fn bubble_outer_width(available: f32, content_width: f32, pad_x: f32) -> f32 {
    let max_w = bubble_max_width(available);
    let inner_max = (max_w - pad_x * 2.0).max(0.0);
    let inner = content_width.clamp(0.0, inner_max);
    (inner + pad_x * 2.0).min(max_w)
}

/// Outer height grows with wrapped lines plus padding.
pub fn bubble_outer_height(content_height: f32, pad_y: f32) -> f32 {
    content_height.max(0.0) + pad_y * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_message_hugs_instead_of_stretching_the_row() {
        let w = bubble_outer_width(800.0, 42.0, BUBBLE_PAD_X);
        assert!(
            w < 120.0,
            "a short 'Hi' must not be a {w}px slab across the chat"
        );
        assert!(w >= 42.0 + BUBBLE_PAD_X * 2.0 - 0.5);
        assert!(w < bubble_max_width(800.0));
    }

    #[test]
    fn long_message_uses_the_pane_and_grows_taller() {
        let max = bubble_max_width(800.0);
        assert!(
            (max - 800.0 * BUBBLE_MAX_FRAC).abs() < 0.1,
            "an 800px pane wraps at ~84%, got {max}"
        );
        assert!(
            max < 800.0,
            "the bubble must stay inside the pane, got {max}"
        );
        let wide = bubble_max_width(1600.0);
        assert!(
            (wide - 1600.0 * BUBBLE_MAX_FRAC).abs() < 0.1,
            "a 1600px pane must grow with the window, got {wide}"
        );
        let w = bubble_outer_width(800.0, 2400.0, BUBBLE_PAD_X);
        assert!((w - max).abs() < 0.1, "got {w} want {max}");
        let one = bubble_outer_height(18.0, BUBBLE_PAD_Y);
        let wrapped = bubble_outer_height(18.0 * 4.0, BUBBLE_PAD_Y);
        assert!(wrapped > one + 20.0, "wrapped text must grow the bubble height");
        assert!((wrapped - (72.0 + BUBBLE_PAD_Y * 2.0)).abs() < 0.1);
    }

    #[test]
    fn wrap_width_leaves_room_for_padding() {
        let wrap = bubble_wrap_width(800.0, BUBBLE_PAD_X);
        assert!(wrap < bubble_max_width(800.0));
        assert!((wrap - (bubble_max_width(800.0) - BUBBLE_PAD_X * 2.0)).abs() < 0.1);
        assert!(bubble_max_width(100.0) <= 100.0);
    }

    #[test]
    fn unbounded_scroll_width_still_stays_in_a_pane() {
        let from_inf = bubble_max_width(f32::INFINITY);
        let from_huge = bubble_max_width(12_000.0);
        let fallback = bubble_max_width(ROW_FALLBACK);
        assert!(
            (from_inf - fallback).abs() < 0.1,
            "infinite available_width must use the fallback pane, got {from_inf}"
        );
        assert!(from_inf < ROW_FALLBACK);
        assert!(
            (from_huge - ROW_SANE_MAX * BUBBLE_MAX_FRAC).abs() < 0.1,
            "huge scroll width must use the sane row, got {from_huge}"
        );
        assert!(from_huge < 1600.0);
        let wrap = bubble_wrap_width(f32::INFINITY, BUBBLE_PAD_X);
        assert!(wrap <= from_inf - BUBBLE_PAD_X * 2.0 + 0.1);
        assert!(wrap > 200.0);
    }
}
