//! Chat bubbles hug their text and wrap with the chat pane.

pub const BUBBLE_MAX_FRAC: f32 = 0.84;
pub const BUBBLE_PAD_X: f32 = 12.0;
pub const BUBBLE_PAD_Y: f32 = 8.0;
pub const BUBBLE_RADIUS: f32 = 16.0;
/// 8K-wide pane. Above this, ScrollArea is reporting garbage, not a monitor.
const ROW_SANE_MAX: f32 = 8192.0;
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

/// Right-aligned user bubbles sit after a leading gap.
///
/// The returned width always leaves at least `gap` in the row, including when
/// the 84% cap is already narrower than `available - gap`. Callers place
/// `available - width` as that leading space and must not subtract `gap` again.
pub fn clamp_bubble_outer(available: f32, outer: f32, gap: f32) -> f32 {
    let avail = clamp_row_width(available);
    let gap = if gap.is_finite() { gap.max(0.0) } else { 0.0 };
    let room = (avail - gap).max(0.0);
    if !outer.is_finite() || outer <= 0.0 {
        return 0.0;
    }
    outer.min(room).min(bubble_max_width(avail))
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
        assert!(
            wrapped > one + 20.0,
            "wrapped text must grow the bubble height"
        );
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
        let from_huge = bubble_max_width(50_000.0);
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
        assert!(from_huge < ROW_SANE_MAX);
        let wrap = bubble_wrap_width(f32::INFINITY, BUBBLE_PAD_X);
        assert!(wrap <= from_inf - BUBBLE_PAD_X * 2.0 + 0.1);
        assert!(wrap > 200.0);
    }

    #[test]
    fn ultrawide_pane_uses_the_real_width() {
        let pane = clamp_row_width(3180.0);
        assert!(
            (pane - 3180.0).abs() < 0.1,
            "a 3440 ultrawide minus the rail must keep the pane, got {pane}"
        );
        let max = bubble_max_width(3180.0);
        assert!(
            (max - 3180.0 * BUBBLE_MAX_FRAC).abs() < 0.1,
            "thoughts and replies wrap with the window, got {max}"
        );
        assert!(
            max > 2000.0,
            "ultrawide chat must not sit in a 1600px strip, got {max}"
        );
        let mid = clamp_row_width(1660.0);
        assert!(
            (mid - 1660.0).abs() < 0.1,
            "a 1920 pane minus the rail must keep the width, got {mid}"
        );
    }

    #[test]
    fn clamp_keeps_a_long_token_inside_narrow_and_wide_rows() {
        let narrow = clamp_bubble_outer(280.0, 10_000.0, 8.0);
        assert!(
            narrow + 8.0 <= 280.0 + 0.1,
            "narrow row let the bubble past the edge: {narrow}"
        );
        assert!(narrow <= bubble_max_width(280.0) + 0.1);
        let wide_cap = bubble_max_width(1600.0);
        let wide = clamp_bubble_outer(1600.0, wide_cap, 8.0);
        assert!(
            (wide - wide_cap).abs() < 0.1,
            "wide row must keep the 84% cap, got {wide}"
        );
        assert!(wide + 8.0 <= 1600.0 + 0.1);
        // 140 is the shrink case (the cap is the whole row). 280 and 800 are
        // typical panes: the 84% cap already fits, and the leading gap is the
        // slack, not a second subtraction.
        for avail in [140.0_f32, 160.0, 280.0, 800.0, 1600.0] {
            let gap = 8.0;
            let outer = clamp_bubble_outer(avail, 10_000.0, gap);
            let lead = (avail - outer).max(0.0);
            let cap = bubble_max_width(avail);
            assert!(
                outer + gap <= avail + 0.1,
                "width {avail}: bubble {outer} plus gap {gap} left the row"
            );
            assert!(
                lead + 0.1 >= gap,
                "width {avail}: leading gap not reserved, lead {lead}"
            );
            assert!(
                (lead + outer - avail).abs() < 0.1,
                "width {avail}: lead {lead} + outer {outer} must fill the row"
            );
            assert!(outer <= cap + 0.1, "width {avail}: outer {outer} over cap {cap}");
            if cap + 0.5 < avail - gap {
                assert!(
                    (outer - cap).abs() < 0.1,
                    "width {avail}: typical pane must keep the 84% cap, got {outer}"
                );
            }
        }
    }
}
