//! Remember cabin size and position across launches.

use serde::{Deserialize, Serialize};

pub const WIN_DEFAULT_W: f32 = 1100.0;
pub const WIN_DEFAULT_H: f32 = 720.0;
pub const WIN_MIN_W: f32 = 720.0;
pub const WIN_MIN_H: f32 = 480.0;
const WIN_MAX: f32 = 8192.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowGeom {
    #[serde(default)]
    pub x: Option<f32>,
    #[serde(default)]
    pub y: Option<f32>,
    #[serde(default = "default_w")]
    pub w: f32,
    #[serde(default = "default_h")]
    pub h: f32,
    #[serde(default)]
    pub maximized: bool,
}

fn default_w() -> f32 {
    WIN_DEFAULT_W
}

fn default_h() -> f32 {
    WIN_DEFAULT_H
}

impl Default for WindowGeom {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            w: WIN_DEFAULT_W,
            h: WIN_DEFAULT_H,
            maximized: false,
        }
    }
}

pub fn clamp_geom(g: WindowGeom) -> WindowGeom {
    WindowGeom {
        x: g.x.filter(|v| v.is_finite()),
        y: g.y.filter(|v| v.is_finite()),
        w: g.w.clamp(WIN_MIN_W, WIN_MAX),
        h: g.h.clamp(WIN_MIN_H, WIN_MAX),
        maximized: g.maximized,
    }
}

pub fn launch_size(g: &WindowGeom) -> [f32; 2] {
    let g = clamp_geom(*g);
    [g.w, g.h]
}

pub fn launch_pos(g: &WindowGeom) -> Option<[f32; 2]> {
    match (g.x, g.y) {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Some([x, y]),
        _ => None,
    }
}

/// Close-to-tray unmaps the window. Do not persist that withdrawn frame.
pub fn remember_geom(
    visible: bool,
    maximized: bool,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    prev: WindowGeom,
) -> Option<WindowGeom> {
    if !visible {
        return None;
    }
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let (w, h, x, y) = if maximized {
        (
            prev.w,
            prev.h,
            prev.x.unwrap_or(x),
            prev.y.unwrap_or(y),
        )
    } else {
        (w, h, x, y)
    };
    Some(clamp_geom(WindowGeom {
        x: Some(x),
        y: Some(y),
        w,
        h,
        maximized,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cabin_is_1100_by_720() {
        let g = WindowGeom::default();
        assert_eq!(launch_size(&g), [1100.0, 720.0]);
        assert!(launch_pos(&g).is_none());
        assert!(!g.maximized);
    }

    #[test]
    fn hidden_cabin_does_not_overwrite_geometry() {
        let prev = WindowGeom {
            x: Some(40.0),
            y: Some(80.0),
            w: 1280.0,
            h: 800.0,
            maximized: false,
        };
        assert!(
            remember_geom(false, false, 0.0, 0.0, 100.0, 100.0, prev).is_none(),
            "Close-to-tray must not save a withdrawn frame"
        );
    }

    #[test]
    fn visible_cabin_remembers_size_and_position() {
        let g = remember_geom(
            true,
            false,
            120.0,
            64.0,
            1400.0,
            900.0,
            WindowGeom::default(),
        )
        .expect("visible");
        assert_eq!(launch_size(&g), [1400.0, 900.0]);
        assert_eq!(launch_pos(&g), Some([120.0, 64.0]));
        assert!(!g.maximized);
    }

    #[test]
    fn tiny_size_bumps_to_the_minimum() {
        let g = remember_geom(true, false, 0.0, 0.0, 10.0, 10.0, WindowGeom::default())
            .expect("visible");
        assert_eq!(launch_size(&g), [720.0, 480.0]);
    }

    #[test]
    fn zero_size_frame_does_not_overwrite() {
        let prev = WindowGeom {
            x: Some(40.0),
            y: Some(80.0),
            w: 1280.0,
            h: 800.0,
            maximized: false,
        };
        assert!(
            remember_geom(true, false, 0.0, 0.0, 0.0, 0.0, prev).is_none(),
            "first-frame 0x0 must not clobber a remembered cabin"
        );
    }

    #[test]
    fn maximized_keeps_the_restored_size() {
        let prev = WindowGeom {
            x: Some(20.0),
            y: Some(30.0),
            w: 1000.0,
            h: 700.0,
            maximized: false,
        };
        let g = remember_geom(true, true, 0.0, 0.0, 1920.0, 1080.0, prev).expect("visible");
        assert!(g.maximized);
        assert_eq!(launch_size(&g), [1000.0, 700.0]);
        assert_eq!(launch_pos(&g), Some([20.0, 30.0]));
    }
}
