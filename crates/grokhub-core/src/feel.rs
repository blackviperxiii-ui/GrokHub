//! Click feel for GrokHub on CachyOS — snappy Plasma-adjacent timing, no bounce.

pub const HOVER_GROW: f32 = 0.015;
pub const PRESS_SHRINK: f32 = 0.045;
pub const HOVER_WASH: f32 = 0.10;
pub const PRESS_WASH: f32 = 0.18;
pub const HOVER_SECS: f32 = 0.08;
pub const PRESS_SECS: f32 = 0.05;
/// Selection / knob slide — ~120ms, between egui hover and KDE widget motion.
pub const SELECT_SECS: f32 = 0.12;
pub const HOVER_EXPANSION: f32 = 1.0;
pub const PRESS_EXPANSION: f32 = -1.5;

pub fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn feel_scale(hover_t: f32, press_t: f32) -> f32 {
    let hover = hover_t.clamp(0.0, 1.0);
    let press = press_t.clamp(0.0, 1.0);
    1.0 + HOVER_GROW * hover - PRESS_SHRINK * press
}

pub fn felt_rect(x: f32, y: f32, w: f32, h: f32, scale: f32) -> (f32, f32, f32, f32) {
    let nw = w * scale;
    let nh = h * scale;
    (x + (w - nw) * 0.5, y + (h - nh) * 0.5, nw, nh)
}

pub fn hover_mix(hovered: f32, pressed: f32) -> f32 {
    let hover = hovered.clamp(0.0, 1.0);
    let press = pressed.clamp(0.0, 1.0);
    HOVER_WASH * hover * (1.0 - press) + PRESS_WASH * press
}

pub fn mix_channel(from: u8, toward: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    (from as f32 + (toward as f32 - from as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

pub fn lift_rgb(r: u8, g: u8, b: u8, t: f32, toward_white: bool) -> (u8, u8, u8) {
    let target = if toward_white { 255 } else { 0 };
    (
        mix_channel(r, target, t),
        mix_channel(g, target, t),
        mix_channel(b, target, t),
    )
}

pub fn hover_alpha(base_a: u8, mix: f32) -> u8 {
    if base_a == 0 {
        mix_channel(0, 160, mix)
    } else {
        base_a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_is_identity() {
        assert_eq!(feel_scale(0.0, 0.0), 1.0);
        assert_eq!(hover_mix(0.0, 0.0), 0.0);
        assert_eq!(
            felt_rect(10.0, 20.0, 100.0, 40.0, 1.0),
            (10.0, 20.0, 100.0, 40.0)
        );
    }

    #[test]
    fn hover_grows_press_shrinks() {
        let hover = feel_scale(1.0, 0.0);
        let press = feel_scale(0.0, 1.0);
        let both = feel_scale(1.0, 1.0);
        assert!((hover - 1.015).abs() < 1e-6);
        assert!((press - 0.955).abs() < 1e-6);
        assert!(both < 1.0);
        assert!(both < hover);
        assert!(press < both);
    }

    #[test]
    fn felt_rect_shrinks_toward_center() {
        let (x, y, w, h) = felt_rect(0.0, 0.0, 100.0, 40.0, 0.5);
        assert!((x - 25.0).abs() < 1e-4);
        assert!((y - 10.0).abs() < 1e-4);
        assert!((w - 50.0).abs() < 1e-4);
        assert!((h - 20.0).abs() < 1e-4);
    }

    #[test]
    fn wash_hover_then_press() {
        assert!((hover_mix(1.0, 0.0) - HOVER_WASH).abs() < 1e-6);
        assert!((hover_mix(0.0, 1.0) - PRESS_WASH).abs() < 1e-6);
        assert!((hover_mix(1.0, 1.0) - PRESS_WASH).abs() < 1e-6);
    }

    #[test]
    fn lift_toward_white_or_black() {
        let (r, g, b) = lift_rgb(20, 20, 20, 0.10, true);
        assert!(r > 20 && r < 50);
        assert_eq!(r, g);
        assert_eq!(g, b);
        let (r2, _, _) = lift_rgb(244, 244, 245, 0.10, false);
        assert!(r2 < 244);
        assert_eq!(mix_channel(0, 100, 0.0), 0);
        assert_eq!(mix_channel(0, 100, 1.0), 100);
        assert_eq!(mix_channel(10, 10, 0.5), 10);
    }

    #[test]
    fn transparent_gets_wash_alpha() {
        assert_eq!(hover_alpha(0, 0.0), 0);
        assert!(hover_alpha(0, 0.10) > 8);
        assert!(hover_alpha(0, 0.18) > hover_alpha(0, 0.10));
        assert_eq!(hover_alpha(200, 0.10), 200);
    }

    #[test]
    fn widget_expansion_press_insets() {
        assert!(HOVER_EXPANSION > 0.0);
        assert!(PRESS_EXPANSION < 0.0);
        assert_eq!(HOVER_EXPANSION, 1.0);
        assert_eq!(PRESS_EXPANSION, -1.5);
    }

    #[test]
    fn lerp_endpoints() {
        assert_eq!(lerp_f32(0.0, 100.0, 0.0), 0.0);
        assert_eq!(lerp_f32(0.0, 100.0, 1.0), 100.0);
        assert!((lerp_f32(10.0, 30.0, 0.5) - 20.0).abs() < 1e-6);
    }
}
