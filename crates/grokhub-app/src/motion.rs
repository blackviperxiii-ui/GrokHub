//! Wave 2E motion polish primitives (Critiquito 2026-10-06).
//! Patterns only — no React deps. Cap ~180–240ms. Honor reduced motion.

use eframe::egui::{self, Color32, Pos2, Vec2};

/// Pulse Feed↔Ideas crossfade duration (seconds).
pub const PULSE_CROSSFADE_SECS: f32 = 0.180;
/// Shared-axis slide for Feed↔Ideas (pixels).
pub const PULSE_AXIS_PX: f32 = 8.0;
/// Ideas row stagger step (seconds).
pub const IDEAS_STAGGER_SECS: f32 = 0.020;
/// Max Ideas rows that stagger.
pub const IDEAS_STAGGER_MAX: usize = 5;

/// Approval / needs-attention enter duration.
pub const APPROVAL_ENTER_SECS: f32 = 0.200;
/// Approval enter y offset (pixels).
pub const APPROVAL_ENTER_Y: f32 = 12.0;
/// Approval exit y bump (pixels).
pub const APPROVAL_EXIT_Y: f32 = 8.0;
/// Hover wash duration.
pub const APPROVAL_HOVER_SECS: f32 = 0.120;
/// Thinking rim breath period (seconds).
pub const THINKING_RIM_SECS: f32 = 1.2;
/// Thinking rim alpha low/high.
pub const THINKING_RIM_A0: f32 = 0.15;
pub const THINKING_RIM_A1: f32 = 0.35;
/// Thinking rim stroke width.
pub const THINKING_RIM_PX: f32 = 2.0;

/// Agent cursor travel duration default.
pub const CURSOR_TRAVEL_SECS: f32 = 0.220;
/// Overshoot past target (pixels), then settle.
pub const CURSOR_OVERSHOOT_MIN: f32 = 4.0;
pub const CURSOR_OVERSHOOT_MAX: f32 = 8.0;
/// Settle after overshoot (seconds).
pub const CURSOR_SETTLE_SECS: f32 = 0.080;
/// Hover-settle peak scale.
pub const CURSOR_HOVER_SCALE: f32 = 1.08;
/// Click-press scale and hold.
pub const CURSOR_CLICK_SCALE: f32 = 0.92;
pub const CURSOR_CLICK_SECS: f32 = 0.060;
/// Arc bulge perpendicular to travel (pixels).
pub const CURSOR_ARC_PX: f32 = 10.0;

/// Hover bg matching Pulse row hover (#0f1012).
pub const HOVER_BG: Color32 = Color32::from_rgb(0x0f, 0x10, 0x12);
/// Cursor fill — white/gray, no neon.
pub const CURSOR_FILL: Color32 = Color32::from_rgb(0xe7, 0xe9, 0xea);
pub const CURSOR_EDGE: Color32 = Color32::from_rgb(0x9a, 0x9e, 0xa2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentCursorPhase {
    Travel,
    HoverSettle,
    ClickPress,
}

#[derive(Clone, Copy, Debug)]
pub struct AgentCursorAnim {
    pub from: Pos2,
    pub to: Pos2,
    pub t0: f64,
    pub duration: f32,
    pub phase: AgentCursorPhase,
}

impl AgentCursorAnim {
    pub fn start_travel(from: Pos2, to: Pos2, now: f64) -> Self {
        Self {
            from,
            to,
            t0: now,
            duration: CURSOR_TRAVEL_SECS,
            phase: AgentCursorPhase::Travel,
        }
    }

    pub fn demo(now: f64, origin: Pos2) -> Self {
        Self::start_travel(origin, origin + Vec2::new(160.0, 72.0), now)
    }
}

/// True when egui animation is disabled (prefers-reduced-motion / animation_time == 0).
pub fn reduced_motion(ui: &egui::Ui) -> bool {
    ui.style().animation_time <= 0.0
}

pub fn reduced_motion_ctx(ctx: &egui::Context) -> bool {
    ctx.global_style().animation_time <= 0.0
}

fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t
}

/// Linear progress 0..1 for an animation that started at `t0` lasting `duration`.
#[cfg(test)]
pub fn progress(now: f64, t0: f64, duration: f32, reduced: bool) -> f32 {
    if reduced || duration <= 0.0 {
        return 1.0;
    }
    ((now - t0) as f32 / duration).clamp(0.0, 1.0)
}

/// Pulse tab crossfade: 0 = Feed fully shown, 1 = Ideas fully shown.
pub fn pulse_tab_t(ctx: &egui::Context, ideas_on: bool) -> f32 {
    if reduced_motion_ctx(ctx) {
        return if ideas_on { 1.0 } else { 0.0 };
    }
    ctx.animate_bool_with_time_and_easing(
        egui::Id::new("pulse-feed-ideas-crossfade"),
        ideas_on,
        PULSE_CROSSFADE_SECS,
        egui::emath::easing::cubic_out,
    )
}

/// Shared-axis x for the Feed layer given ideas progress `t` (Feed→Ideas → −8).
pub fn feed_axis_x(t: f32) -> f32 {
    -PULSE_AXIS_PX * t.clamp(0.0, 1.0)
}

/// Shared-axis x for the Ideas layer (enters from +8 → 0 as t→1).
pub fn ideas_axis_x(t: f32) -> f32 {
    PULSE_AXIS_PX * (1.0 - t.clamp(0.0, 1.0))
}

/// Ideas row enter factor 0..1 with stagger. `tab_t` is pulse_tab_t.
pub fn ideas_row_enter(tab_t: f32, index: usize, reduced: bool) -> f32 {
    if reduced {
        return if tab_t > 0.5 { 1.0 } else { 0.0 };
    }
    let i = index.min(IDEAS_STAGGER_MAX.saturating_sub(1));
    let delay = i as f32 * IDEAS_STAGGER_SECS;
    let span = (PULSE_CROSSFADE_SECS - delay).max(0.001);
    let local = ((tab_t * PULSE_CROSSFADE_SECS) - delay) / span;
    ease_out_cubic(local.clamp(0.0, 1.0))
}

/// Approval card enter 0..1 (ease-out). `show` true while the card is up.
pub fn approval_enter_t(ui: &egui::Ui, id: egui::Id, show: bool) -> f32 {
    if reduced_motion(ui) {
        return if show { 1.0 } else { 0.0 };
    }
    ui.ctx().animate_bool_with_time_and_easing(
        id.with("approval-enter"),
        show,
        APPROVAL_ENTER_SECS,
        egui::emath::easing::cubic_out,
    )
}

/// Y offset for approval enter (12→0) or exit (+8 while fading).
pub fn approval_y(enter_t: f32, exiting: bool) -> f32 {
    if exiting {
        (1.0 - enter_t) * APPROVAL_EXIT_Y
    } else {
        (1.0 - enter_t) * APPROVAL_ENTER_Y
    }
}

/// Hover bg blend 0..1 over 120ms.
pub fn approval_hover_t(ui: &egui::Ui, id: egui::Id, hovered: bool) -> f32 {
    if reduced_motion(ui) {
        return if hovered { 1.0 } else { 0.0 };
    }
    ui.ctx().animate_bool_with_time_and_easing(
        id.with("approval-hover"),
        hovered,
        APPROVAL_HOVER_SECS,
        egui::emath::easing::quadratic_out,
    )
}

/// Thinking rim alpha oscillating 0.15↔0.35.
pub fn thinking_rim_alpha(time_secs: f32) -> f32 {
    let phase = time_secs * std::f32::consts::TAU / THINKING_RIM_SECS;
    let w = (phase.sin() + 1.0) * 0.5;
    THINKING_RIM_A0 + (THINKING_RIM_A1 - THINKING_RIM_A0) * w
}

/// One-shot breath envelope 0..1..0 over ~0.6s from `t0` (for primary action).
pub fn one_shot_breath(now: f64, t0: f64, reduced: bool) -> f32 {
    if reduced {
        return 0.0;
    }
    let t = ((now - t0) as f32 / 0.60).clamp(0.0, 1.0);
    // Rise then fall.
    if t < 0.45 {
        ease_out_cubic(t / 0.45)
    } else {
        1.0 - ease_in_cubic((t - 0.45) / 0.55)
    }
}

/// Position + scale for the agent cursor stub at `now`.
pub fn cursor_sample(anim: &AgentCursorAnim, now: f64, reduced: bool) -> (Pos2, f32, bool) {
    let dur = if reduced { 0.0001 } else { anim.duration.max(0.0001) };
    let raw = ((now - anim.t0) as f32 / dur).clamp(0.0, 1.0);
    match anim.phase {
        AgentCursorPhase::Travel => {
            let _t = ease_out_cubic(raw);
            // Overshoot: push past `to` by 4–8px along travel, then settle in last settle window.
            let delta = anim.to - anim.from;
            let len = delta.length().max(1.0);
            let dir = delta / len;
            let overshoot = ((CURSOR_OVERSHOOT_MIN + CURSOR_OVERSHOOT_MAX) * 0.5).clamp(
                CURSOR_OVERSHOOT_MIN,
                CURSOR_OVERSHOOT_MAX,
            );
            let settle_frac = (CURSOR_SETTLE_SECS / dur).clamp(0.05, 0.4);
            let pos = if raw < 1.0 - settle_frac {
                let u = raw / (1.0 - settle_frac);
                let e = ease_out_cubic(u);
                let along = anim.from + dir * (len + overshoot) * e;
                let perp = Vec2::new(-dir.y, dir.x);
                let arc = (4.0 * u * (1.0 - u)) * CURSOR_ARC_PX;
                along + perp * arc
            } else {
                let u = (raw - (1.0 - settle_frac)) / settle_frac;
                let from = anim.to + dir * overshoot;
                from.lerp(anim.to, ease_out_cubic(u))
            };
            (pos, 1.0, raw < 1.0)
        }
        AgentCursorPhase::HoverSettle => {
            // 1.0 → 1.08 → 1.0 over duration
            let scale = if raw < 0.5 {
                1.0 + (CURSOR_HOVER_SCALE - 1.0) * ease_out_cubic(raw * 2.0)
            } else {
                CURSOR_HOVER_SCALE
                    - (CURSOR_HOVER_SCALE - 1.0) * ease_out_cubic((raw - 0.5) * 2.0)
            };
            (anim.to, scale, raw < 1.0)
        }
        AgentCursorPhase::ClickPress => {
            let scale = if raw < 1.0 {
                CURSOR_CLICK_SCALE
            } else {
                1.0
            };
            (anim.to, scale, raw < 1.0)
        }
    }
}

/// Advance phase when travel completes: Travel → HoverSettle → ClickPress → done (None).
pub fn cursor_advance(anim: &mut AgentCursorAnim, now: f64, reduced: bool) -> bool {
    let (_, _, live) = cursor_sample(anim, now, reduced);
    if live {
        return true;
    }
    match anim.phase {
        AgentCursorPhase::Travel => {
            anim.phase = AgentCursorPhase::HoverSettle;
            anim.t0 = now;
            anim.duration = 0.200;
            true
        }
        AgentCursorPhase::HoverSettle => {
            anim.phase = AgentCursorPhase::ClickPress;
            anim.t0 = now;
            anim.duration = CURSOR_CLICK_SECS;
            true
        }
        AgentCursorPhase::ClickPress => false,
    }
}

/// Paint the white/gray circle + triangle pointer. No neon trail.
pub fn paint_agent_cursor(painter: &egui::Painter, pos: Pos2, scale: f32) {
    let r = 5.5 * scale;
    painter.circle_filled(pos, r, CURSOR_FILL);
    painter.circle_stroke(pos, r, egui::Stroke::new(1.0, CURSOR_EDGE));
    let tip = pos + Vec2::new(0.0, r + 7.0 * scale);
    let left = pos + Vec2::new(-4.5 * scale, r * 0.2);
    let right = pos + Vec2::new(4.5 * scale, r * 0.2);
    painter.add(egui::Shape::convex_polygon(
        vec![tip, left, right],
        CURSOR_FILL,
        egui::Stroke::new(1.0, CURSOR_EDGE),
    ));
}

/// Drive + paint an optional agent cursor; clears when finished. Requests repaint while live.
pub fn tick_agent_cursor(
    ui: &egui::Ui,
    anim: &mut Option<AgentCursorAnim>,
) {
    let Some(mut cur) = anim.take() else {
        return;
    };
    let now = ui.ctx().input(|i| i.time);
    let reduced = reduced_motion(ui);
    let live = cursor_advance(&mut cur, now, reduced);
    let (pos, scale, _) = cursor_sample(&cur, now, reduced);
    paint_agent_cursor(ui.painter(), pos, scale);
    if live {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
        *anim = Some(cur);
    }
}

/// Nizam-style summary when `n` things need a decision.
pub fn needs_attention_summary(n: usize) -> String {
    match n {
        0 => "Nothing needs a decision".into(),
        1 => "1 thing needs a decision".into(),
        _ => format!("{n} things need a decision"),
    }
}

/// Paint a 2px white thinking rim around `rect` when `thinking`.
pub fn paint_thinking_rim(painter: &egui::Painter, rect: egui::Rect, thinking: bool, time_secs: f32) {
    if !thinking {
        return;
    }
    let a = (thinking_rim_alpha(time_secs) * 255.0).round() as u8;
    let stroke = egui::Stroke::new(THINKING_RIM_PX, Color32::from_white_alpha(a));
    painter.rect_stroke(rect, 8.0, stroke, egui::StrokeKind::Outside);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_literal_durations() {
        assert!((PULSE_CROSSFADE_SECS - 0.180).abs() < 0.0001);
        assert!((APPROVAL_ENTER_SECS - 0.200).abs() < 0.0001);
        assert!((CURSOR_SETTLE_SECS - 0.080).abs() < 0.0001);
        assert!((CURSOR_CLICK_SECS - 0.060).abs() < 0.0001);
        assert!((IDEAS_STAGGER_SECS - 0.020).abs() < 0.0001);
        assert_eq!(IDEAS_STAGGER_MAX, 5);
        assert!((PULSE_AXIS_PX - 8.0).abs() < 0.01);
        assert!((APPROVAL_ENTER_Y - 12.0).abs() < 0.01);
        assert!((APPROVAL_HOVER_SECS - 0.120).abs() < 0.0001);
    }

    #[test]
    fn cursor_colors_are_white_gray_not_neon() {
        assert_eq!(CURSOR_FILL, Color32::from_rgb(0xe7, 0xe9, 0xea));
        assert_eq!(CURSOR_EDGE, Color32::from_rgb(0x9a, 0x9e, 0xa2));
        // No saturated green/cyan neon.
        assert!(CURSOR_FILL.g() < 240 || CURSOR_FILL.r() > 200);
        assert_ne!(CURSOR_FILL, Color32::from_rgb(0x00, 0xff, 0x88));
    }

    #[test]
    fn hover_bg_is_0f1012() {
        assert_eq!(HOVER_BG, Color32::from_rgb(0x0f, 0x10, 0x12));
    }

    #[test]
    fn overshoot_range_4_to_8() {
        assert!((CURSOR_OVERSHOOT_MIN - 4.0).abs() < 0.01);
        assert!((CURSOR_OVERSHOOT_MAX - 8.0).abs() < 0.01);
        let mid = (CURSOR_OVERSHOOT_MIN + CURSOR_OVERSHOOT_MAX) * 0.5;
        assert!((4.0..=8.0).contains(&mid));
    }

    #[test]
    fn reduced_progress_snaps() {
        assert_eq!(progress(1.0, 0.0, 0.2, true), 1.0);
        assert_eq!(progress(0.05, 0.0, 0.2, false), 0.25);
    }

    #[test]
    fn feed_ideas_axis_signs() {
        assert!((feed_axis_x(0.0) - 0.0).abs() < 0.01);
        assert!((feed_axis_x(1.0) - (-8.0)).abs() < 0.01);
        assert!((ideas_axis_x(0.0) - 8.0).abs() < 0.01);
        assert!((ideas_axis_x(1.0) - 0.0).abs() < 0.01);
    }

    #[test]
    fn ideas_stagger_orders_rows() {
        let a = ideas_row_enter(0.3, 0, false);
        let b = ideas_row_enter(0.3, 4, false);
        assert!(a > b, "earlier rows enter first: {a} vs {b}");
        assert_eq!(ideas_row_enter(1.0, 0, true), 1.0);
        assert_eq!(ideas_row_enter(0.0, 0, true), 0.0);
    }

    #[test]
    fn thinking_rim_alpha_in_range() {
        for i in 0..20 {
            let a = thinking_rim_alpha(i as f32 * 0.1);
            assert!((THINKING_RIM_A0 - 0.001..=THINKING_RIM_A1 + 0.001).contains(&a));
        }
    }

    #[test]
    fn needs_attention_copy() {
        assert_eq!(needs_attention_summary(0), "Nothing needs a decision");
        assert_eq!(needs_attention_summary(1), "1 thing needs a decision");
        assert_eq!(needs_attention_summary(3), "3 things need a decision");
    }

    #[test]
    fn cursor_travel_then_advances() {
        let mut anim = AgentCursorAnim::start_travel(
            Pos2::new(0.0, 0.0),
            Pos2::new(100.0, 0.0),
            0.0,
        );
        let (p0, s0, live0) = cursor_sample(&anim, 0.0, false);
        assert!(live0);
        assert!((s0 - 1.0).abs() < 0.01);
        assert!(p0.x >= 0.0);
        // Far past duration → advance phases
        assert!(cursor_advance(&mut anim, 10.0, false));
        assert_eq!(anim.phase, AgentCursorPhase::HoverSettle);
        assert!(cursor_advance(&mut anim, 20.0, false));
        assert_eq!(anim.phase, AgentCursorPhase::ClickPress);
        assert!(!cursor_advance(&mut anim, 30.0, false));
    }

    #[test]
    fn click_press_scale_literal() {
        assert!((CURSOR_CLICK_SCALE - 0.92).abs() < 0.001);
        assert!((CURSOR_HOVER_SCALE - 1.08).abs() < 0.001);
    }
}
