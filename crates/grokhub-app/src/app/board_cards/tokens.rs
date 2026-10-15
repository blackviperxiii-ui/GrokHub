//! Card colors and motion timings. No card code hard-codes a color; it asks
//! `card_tokens` for the active mode.

use eframe::egui::Color32;

/// Colors for one paint mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CardTokens {
    pub surface: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub title: Color32,
    pub desc: Color32,
    pub meta: Color32,
    pub live: Color32,
    pub warn: Color32,
    pub danger: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Dark values reuse the theme consts where they exist (`SURFACE`, `BORDER`, `FG`,
/// `SUBTLE`, `MUTED`, `LIVE`); light values are the card spec.
pub(super) const fn card_tokens(dark: bool) -> CardTokens {
    if dark {
        CardTokens {
            surface: crate::theme::SURFACE,
            hover: crate::theme::SURFACE_HOVER,
            border: crate::theme::BORDER,
            title: crate::theme::FG,
            desc: crate::theme::SUBTLE,
            meta: crate::theme::MUTED,
            live: crate::theme::LIVE,
            warn: rgb(0xeab308),
            danger: rgb(0xf4212e),
        }
    } else {
        CardTokens {
            surface: rgb(0xffffff),
            hover: rgb(0xf4f4f5),
            border: rgb(0xe4e4e7),
            title: rgb(0x0f1419),
            desc: rgb(0x536471),
            meta: rgb(0x71767b),
            live: crate::theme::LIGHT_LIVE,
            warn: rgb(0xa16207),
            danger: rgb(0xb91c1c),
        }
    }
}

/// Compact card geometry (logical px).
pub(super) const COMPACT_H: f32 = 56.0;
pub(super) const CARD_RADIUS: f32 = 12.0;
pub(super) const PILL_H: f32 = 36.0;
pub(super) const ACTION_H: f32 = 28.0;

/// Motion timings (seconds). Every one snaps under `motion::reduced_motion`.
pub(super) const HOVER_FILL_SECS: f32 = 0.120;
pub(super) const EXPAND_SECS: f32 = 0.220;
pub(super) const EXPAND_FADE_DELAY_SECS: f32 = 0.060;
pub(super) const COLLAPSE_SECS: f32 = 0.180;
pub(super) const ACTION_MORPH_SECS: f32 = 0.160;
pub(super) const DRAG_SHRINK_SECS: f32 = 0.120;
pub(super) const DRAG_RESTORE_SECS: f32 = 0.140;
pub(super) const DRAG_SCALE: f32 = 0.97;
pub(super) const EMPTY_FADE_IN_SECS: f32 = 0.240;
pub(super) const EMPTY_FADE_DELAY_SECS: f32 = 0.150;
pub(super) const EMPTY_FADE_OUT_SECS: f32 = 0.120;
pub(super) const LAYOUT_SWITCH_SECS: f32 = 0.260;
pub(super) const LAYOUT_STAGGER_SECS: f32 = 0.020;
pub(super) const LAYOUT_STAGGER_MAX_SECS: f32 = 0.200;
