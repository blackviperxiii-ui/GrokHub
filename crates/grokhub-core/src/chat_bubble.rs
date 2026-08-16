//! Chat bubbles hug their text and wrap instead of stretching across the row.

pub const BUBBLE_MAX_FRAC: f32 = 0.72;
pub const BUBBLE_PAD_X: f32 = 14.0;
pub const BUBBLE_PAD_Y: f32 = 10.0;
pub const BUBBLE_RADIUS: f32 = 18.0;

/// Hard cap for a bubble on this row. Long text wraps here; short text must not stretch to it.
pub fn bubble_max_width(available: f32) -> f32 {
    let avail = available.max(0.0);
    if avail < 160.0 {
        avail
    } else {
        (avail * BUBBLE_MAX_FRAC).clamp(160.0, avail)
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
    fn long_message_caps_at_the_row_and_grows_taller() {
        let max = bubble_max_width(800.0);
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
}
