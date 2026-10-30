//! Desktop Nyx: the companion in her own small see-through window on the
//! desktop, standing on the holo pedestal ([`crate::pedestal`]) and wearing
//! the outfit the companion file gives her mode
//! ([`grokhub_core::companion`]). Behind the Labs switch
//! `Desktop companion`; the corner box's pop-out puts her on the desktop and
//! Dock back brings her home. Nothing here touches a game: her window is a
//! plain second viewport in this process, and she hides whenever the game
//! guard says gaming or a fullscreen app is in front.
//!
//! The viewport callback runs off the cabin's borrow, so the cabin writes a
//! [`Snapshot`] each frame and the window hands back [`Event`]s. All the
//! decisions (hide, snap, click-through, repaint cadence) are plain
//! functions here so they are tested without a display.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use grokhub_core::companion::Mood;

use super::*;

/// The window in points.
pub(super) const WINDOW_SIZE: Vec2 = Vec2::new(320.0, 480.0);
/// Snap to a monitor edge from this close.
pub(super) const SNAP_PX: f32 = 24.0;
/// Keep at least this much of the window on screen.
pub(super) const KEEP_ON_SCREEN_PX: f32 = 64.0;
/// Come back this long after the game or fullscreen app goes away.
pub(super) const RETURN_AFTER: Duration = Duration::from_secs(4);
/// Repaint cadence while animating, and while watching the cursor to
/// flip click-through.
pub(super) const ANIM_FRAME: Duration = Duration::from_millis(33);
pub(super) const CURSOR_FRAME: Duration = Duration::from_millis(100);
/// Window position is saved this long after it stops moving.
pub(super) const POS_SETTLE: Duration = Duration::from_millis(600);
/// The viewport id and the native title (how the Windows helpers find
/// her HWND).
const VIEWPORT: &str = "desktop-nyx";

/// What the window needs from the cabin each frame.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Snapshot {
    pub mood: Mood,
    /// Adults-only outfits allowed on the desktop right now.
    pub adults_ok: bool,
    pub voice: VoiceLook,
    /// Motion reduced: pedestal still, no bob.
    pub still: bool,
    pub behind: bool,
    pub accent: Option<Color32>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            mood: Mood::HangingOut,
            adults_ok: false,
            voice: VoiceLook::Idle,
            still: false,
            behind: false,
            accent: None,
        }
    }
}

/// How voice shows on the pedestal: listening lights it, thinking flickers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VoiceLook {
    Idle,
    Listening,
    Thinking,
    Speaking,
}

/// What the user did in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    OpenChat,
    DockBack,
    Hide,
    ToggleBehind,
    PushToTalk,
}

/// Shared between the cabin and the viewport callback.
#[derive(Default)]
pub(super) struct Shared {
    pub snap: Snapshot,
    pub events: Vec<Event>,
    /// The window's last known outer position, written by the callback.
    pub outer: Option<Rect>,
    pub monitor: Option<Vec2>,
    /// Pointer over Nyx or the pedestal (not empty window).
    pub over_art: bool,
}

/// Cabin-side state for the desktop window.
pub(super) struct DesktopNyx {
    pub shared: Arc<Mutex<Shared>>,
    /// Hidden for a game or a fullscreen app; when it cleared.
    pub clear_since: Option<Instant>,
    pub hidden_for_game: bool,
    /// Position memory: the last outer position seen and when it changed.
    pub last_outer: Option<(Pos2, Instant)>,
    pub saved_pos: Option<Pos2>,
    pub passthrough: Option<bool>,
    pub capture_excluded: bool,
    /// Frames the window has been shown this run (for first-frame setup).
    pub shown_frames: u32,
}

impl Default for DesktopNyx {
    fn default() -> Self {
        Self {
            shared: Arc::default(),
            clear_since: Some(Instant::now() - RETURN_AFTER),
            hidden_for_game: false,
            last_outer: None,
            saved_pos: None,
            passthrough: None,
            capture_excluded: false,
            shown_frames: 0,
        }
    }
}

/// Whether the window shows this frame. `gaming` is the game guard not
/// clear or a fullscreen app in front. She hides at once and comes back
/// [`RETURN_AFTER`] after the last gaming frame.
pub(super) fn visible_now(
    undocked: bool,
    gaming: bool,
    clear_since: &mut Option<Instant>,
    now: Instant,
) -> bool {
    if !undocked {
        return false;
    }
    if gaming {
        *clear_since = None;
        return false;
    }
    let since = *clear_since.get_or_insert(now);
    now.duration_since(since) >= RETURN_AFTER
}

/// Snap a window at `pos` (outer top-left, points) to the monitor's edges
/// when within [`SNAP_PX`], and keep [`KEEP_ON_SCREEN_PX`] of it on screen.
pub(super) fn snap_position(pos: Pos2, size: Vec2, monitor: Vec2) -> Pos2 {
    let mut p = pos;
    let right = monitor.x - size.x;
    let bottom = monitor.y - size.y;
    if (p.x).abs() <= SNAP_PX {
        p.x = 0.0;
    } else if (p.x - right).abs() <= SNAP_PX {
        p.x = right;
    }
    if (p.y).abs() <= SNAP_PX {
        p.y = 0.0;
    } else if (p.y - bottom).abs() <= SNAP_PX {
        p.y = bottom;
    }
    p.x =
        p.x.clamp(KEEP_ON_SCREEN_PX - size.x, monitor.x - KEEP_ON_SCREEN_PX);
    p.y = p.y.clamp(0.0, (monitor.y - KEEP_ON_SCREEN_PX).max(0.0));
    p
}

/// Where she first appears: bottom right of the monitor with a margin.
pub(super) fn default_position(monitor: Vec2) -> Pos2 {
    Pos2::new(
        (monitor.x - WINDOW_SIZE.x - 24.0).max(0.0),
        (monitor.y - WINDOW_SIZE.y - 56.0).max(0.0),
    )
}

/// How long to wait before the window repaints. Animating parts repaint at
/// 30 fps, a still window only to watch the cursor for click-through, and
/// a hidden window never.
pub(super) fn repaint_after(shown: bool, still: bool, watch_cursor: bool) -> Option<Duration> {
    if !shown {
        None
    } else if !still {
        Some(ANIM_FRAME)
    } else if watch_cursor {
        Some(CURSOR_FRAME)
    } else {
        None
    }
}

/// Mouse passthrough for the window: on while the cursor is over empty
/// pixels, so clicks there reach the desktop; off over Nyx or the pedestal
/// so she can be clicked and dragged. `None` leaves it as it is.
pub(super) fn passthrough_wanted(
    cursor_in_window: Option<bool>,
    over_art: bool,
    supported: bool,
) -> Option<bool> {
    if !supported {
        return Some(false);
    }
    match cursor_in_window {
        None => None,
        Some(false) => Some(true),
        Some(true) => Some(!over_art),
    }
}

/// The opaque area of the picture `tex_size` drawn into `rect`, plus the
/// pedestal disc: a coarse alpha mask so hit-testing needs no pixel reads
/// at runtime.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct HitMask {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<bool>,
}

impl HitMask {
    /// Build from RGBA pixels, one cell per `step` pixels, a cell opaque
    /// when any pixel in it has alpha over 40.
    pub fn from_rgba(w: usize, h: usize, rgba: &[u8], step: usize) -> Self {
        let step = step.max(1);
        let cols = w.div_ceil(step).max(1);
        let rows = h.div_ceil(step).max(1);
        let mut cells = vec![false; cols * rows];
        for y in 0..h {
            for x in 0..w {
                let a = rgba.get((y * w + x) * 4 + 3).copied().unwrap_or(0);
                if a > 40 {
                    cells[(y / step) * cols + x / step] = true;
                }
            }
        }
        Self { cols, rows, cells }
    }

    /// Whether the point `uv` (0..1 across the picture) is opaque.
    pub fn hit(&self, uv: Pos2) -> bool {
        if !(0.0..1.0).contains(&uv.x) || !(0.0..1.0).contains(&uv.y) {
            return false;
        }
        let c = ((uv.x * self.cols as f32) as usize).min(self.cols - 1);
        let r = ((uv.y * self.rows as f32) as usize).min(self.rows - 1);
        self.cells[r * self.cols + c]
    }
}

/// Whether `p` is over Nyx (by her mask drawn in `art`) or the pedestal
/// disc in `frame`.
pub(super) fn over_art(p: Pos2, frame: Rect, art: Rect, mask: Option<&HitMask>) -> bool {
    let l = crate::pedestal::layout(frame);
    let d = (p - l.center) / (l.radius * 1.2);
    if d.x * d.x + d.y * d.y <= 1.0 {
        return true;
    }
    if !art.contains(p) {
        return false;
    }
    match mask {
        Some(m) => m.hit(Pos2::new(
            (p.x - art.left()) / art.width(),
            (p.y - art.top()) / art.height(),
        )),
        // A picture with no transparency (the framed jpgs) is all hit.
        None => true,
    }
}

/// Where the outfit picture sits in the window: standing on the disc, as
/// tall as the room above it allows, never wider than the window.
pub(super) fn art_rect(frame: Rect, tex_size: [usize; 2]) -> Rect {
    super::loadout_home::cutout_rect(frame, tex_size)
}

/// Pedestal glow for the voice state: brighter listening, a flicker
/// thinking, steady otherwise.
pub(super) fn voice_gain(voice: VoiceLook, t: f64, still: bool) -> f32 {
    match voice {
        VoiceLook::Idle => 1.0,
        VoiceLook::Listening => 1.35,
        VoiceLook::Speaking => 1.2,
        VoiceLook::Thinking if still => 1.1,
        VoiceLook::Thinking => 1.0 + 0.18 * ((t * 11.0).sin() as f32 * 0.5 + 0.5),
    }
}

/// Builder for the window: transparent, undecorated, no taskbar entry,
/// not resizable, on top unless the user wants her behind.
pub(super) fn viewport_builder(pos: Pos2, behind: bool) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(grokhub_core::brand::companion_name())
        .with_app_id(VIEWPORT)
        .with_inner_size(WINDOW_SIZE)
        .with_position(pos)
        .with_transparent(true)
        .with_decorations(false)
        .with_resizable(false)
        .with_taskbar(false)
        .with_window_level(if behind {
            egui::WindowLevel::Normal
        } else {
            egui::WindowLevel::AlwaysOnTop
        })
}

pub(super) fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of(VIEWPORT)
}

/// Native readings the window reacts to. Windows has them all; elsewhere
/// she stays clickable and only the game guard hides her.
pub(super) struct Native {
    pub cursor: Option<Pos2>,
    pub fullscreen_app: bool,
    pub passthrough_supported: bool,
}

fn native(ppp: f32) -> Native {
    #[cfg(windows)]
    {
        Native {
            cursor: crate::win_native::cursor_pos()
                .map(|(x, y)| Pos2::new(x as f32 / ppp, y as f32 / ppp)),
            fullscreen_app: crate::win_native::fullscreen_app_in_front(),
            passthrough_supported: true,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = ppp;
        Native {
            cursor: None,
            fullscreen_app: false,
            passthrough_supported: false,
        }
    }
}

impl Cabin {
    /// Each frame: show, hide or update the desktop window.
    pub(super) fn tick_desktop_nyx(&mut self, ctx: &egui::Context) {
        let on = self.cfg.desktop_nyx && self.cfg.desktop_nyx_undocked;
        if !on {
            self.desktop_nyx = None;
            return;
        }
        let now = Instant::now();
        let ppp = ctx.pixels_per_point().max(0.5);
        let nat = native(ppp);
        let guard = grokhub_desktop::game_guard::peek();
        let gaming = !guard.is_clear() || nat.fullscreen_app;
        let mut state = self.desktop_nyx.take().unwrap_or_default();
        let shown = visible_now(on, gaming, &mut state.clear_since, now);
        state.hidden_for_game = !shown;
        if !shown {
            state.shown_frames = 0;
            state.passthrough = None;
            state.capture_excluded = false;
            self.desktop_nyx = Some(state);
            return;
        }
        // The desktop is seen by anyone at the screen: explicit outfits
        // need their own switch on top of adults-only personalities.
        let adults_ok = self.cfg.desktop_nyx_adult && self.explicit_art_ok();
        let snap = Snapshot {
            mood: if adults_ok {
                self.companion_mood()
            } else {
                self.companion_activity()
            },
            adults_ok,
            voice: self.desktop_voice_look(),
            still: crate::motion::reduced_motion_ctx(ctx),
            behind: self.cfg.desktop_nyx_behind,
            accent: grokhub_core::companion::nyx()
                .pedestal_tint
                .as_deref()
                .and_then(grokhub_core::companion::hex_rgb)
                .map(|(r, g, b)| Color32::from_rgb(r, g, b)),
        };
        let (events, outer, monitor, over) = {
            let mut sh = state.shared.lock().unwrap_or_else(|e| e.into_inner());
            sh.snap = snap;
            (
                std::mem::take(&mut sh.events),
                sh.outer,
                sh.monitor,
                sh.over_art,
            )
        };
        let monitor = monitor.unwrap_or(Vec2::new(1920.0, 1080.0));
        let pos = self
            .cfg
            .desktop_nyx_pos
            .map(|[x, y]| Pos2::new(x, y))
            .unwrap_or_else(|| default_position(monitor));
        // Position memory: once the window stops moving, snap and save.
        if let Some(outer) = outer {
            let seen = outer.min;
            match state.last_outer {
                Some((p, _)) if p == seen => {}
                _ => state.last_outer = Some((seen, now)),
            }
            if let Some((p, at)) = state.last_outer {
                if now.duration_since(at) >= POS_SETTLE && Some(p) != state.saved_pos {
                    let snapped = snap_position(p, WINDOW_SIZE, monitor);
                    if snapped != p {
                        ctx.send_viewport_cmd_to(
                            viewport_id(),
                            egui::ViewportCommand::OuterPosition(snapped),
                        );
                    }
                    state.saved_pos = Some(snapped);
                    self.cfg.desktop_nyx_pos = Some([snapped.x, snapped.y]);
                    self.persist_cfg();
                }
            }
        }
        // Click-through over empty pixels, from the global cursor.
        let cursor_in = nat.cursor.map(|c| outer.is_some_and(|r| r.contains(c)));
        if let Some(want) = passthrough_wanted(cursor_in, over, nat.passthrough_supported) {
            if state.passthrough != Some(want) {
                state.passthrough = Some(want);
                ctx.send_viewport_cmd_to(
                    viewport_id(),
                    egui::ViewportCommand::MousePassthrough(want),
                );
            }
        }
        let behind = self.cfg.desktop_nyx_behind;
        let shared = Arc::clone(&state.shared);
        ctx.show_viewport_deferred(
            viewport_id(),
            viewport_builder(pos, behind),
            move |ui, _| {
                paint_window(ui, &shared);
            },
        );
        state.shown_frames = state.shown_frames.saturating_add(1);
        #[cfg(windows)]
        if !state.capture_excluded && state.shown_frames > 2 {
            state.capture_excluded =
                crate::win_native::exclude_from_capture(grokhub_core::brand::companion_name());
        }
        if nat.passthrough_supported && state.passthrough.unwrap_or(false) {
            ctx.request_repaint_after(CURSOR_FRAME);
        }
        self.desktop_nyx = Some(state);
        for ev in events {
            self.on_desktop_nyx_event(ev, ctx);
        }
    }

    fn desktop_voice_look(&self) -> VoiceLook {
        match self.voice_state {
            grokhub_core::VoiceState::Listening | grokhub_core::VoiceState::Hands => {
                VoiceLook::Listening
            }
            grokhub_core::VoiceState::Speaking => VoiceLook::Speaking,
            _ if self.thinking_here() => VoiceLook::Thinking,
            _ => VoiceLook::Idle,
        }
    }

    fn on_desktop_nyx_event(&mut self, ev: Event, ctx: &egui::Context) {
        match ev {
            Event::OpenChat => {
                if !self.window_visible {
                    self.show_from_tray(ctx);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.set_nav_id("chat");
            }
            Event::DockBack => self.dock_nyx_back(),
            Event::Hide => {
                self.cfg.desktop_nyx_undocked = false;
                self.persist_cfg();
            }
            Event::ToggleBehind => {
                self.cfg.desktop_nyx_behind = !self.cfg.desktop_nyx_behind;
                let level = if self.cfg.desktop_nyx_behind {
                    egui::WindowLevel::Normal
                } else {
                    egui::WindowLevel::AlwaysOnTop
                };
                ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::WindowLevel(level));
                self.persist_cfg();
            }
            Event::PushToTalk => self.listen_voice(),
        }
    }

    /// The corner box's pop-out: Nyx moves to the desktop.
    pub(super) fn pop_nyx_out(&mut self) {
        self.cfg.desktop_nyx_undocked = true;
        self.persist_cfg();
    }

    /// Dock back: the window goes and the corner box shows her again.
    pub(super) fn dock_nyx_back(&mut self) {
        self.cfg.desktop_nyx_undocked = false;
        self.persist_cfg();
    }

    /// Tray: Show on desktop / Dock back, whichever applies.
    pub(super) fn toggle_desktop_nyx(&mut self) {
        if !self.cfg.desktop_nyx {
            self.cfg.desktop_nyx = true;
        }
        self.cfg.desktop_nyx_undocked = !self.cfg.desktop_nyx_undocked;
        self.persist_cfg();
    }

    /// Settings → Labs: the switch and its two companions.
    pub(super) fn ui_labs_desktop_nyx(&mut self, ui: &mut egui::Ui) {
        let name = grokhub_core::brand::companion_name();
        crate::ui_kit::section_header(ui, &format!("Desktop {name} (preview)"), None);
        let mut on = self.cfg.desktop_nyx;
        if crate::cards::settings_toggle(
            ui,
            "Desktop companion",
            &format!(
                "Puts a pop-out on {name}'s corner box. Popped out, she stands on a holo pedestal in a small see-through window over your desktop, and hides whenever a game or a fullscreen app is in front."
            ),
            &mut on,
        ) {
            self.cfg.desktop_nyx = on;
            if !on {
                self.cfg.desktop_nyx_undocked = false;
            }
            self.persist_cfg();
        }
        if !self.cfg.desktop_nyx {
            return;
        }
        let mut behind = self.cfg.desktop_nyx_behind;
        if crate::cards::settings_toggle(
            ui,
            "Stay behind windows",
            "On: she sits on the desktop and your apps cover her. Off: she floats above them.",
            &mut behind,
        ) {
            self.cfg.desktop_nyx_behind = behind;
            self.persist_cfg();
        }
        if self.cfg.explicit_personalities {
            let mut adult = self.cfg.desktop_nyx_adult;
            if crate::cards::settings_toggle(
                ui,
                "Allow 18+ outfits on the desktop",
                "Off by default: the desktop is visible to anyone at the screen. Needs adults-only personalities on.",
                &mut adult,
            ) {
                self.cfg.desktop_nyx_adult = adult;
                self.persist_cfg();
            }
        }
        let state = if self.cfg.desktop_nyx_undocked {
            if self.desktop_nyx.as_ref().is_some_and(|s| s.hidden_for_game) {
                "On the desktop, hidden while you game."
            } else {
                "On the desktop. Right-click her for Dock back."
            }
        } else {
            "Docked in the corner box. Use its pop-out to put her on the desktop."
        };
        crate::cards::settings_note(ui, state);
    }
}

/// The window's own UI: a transparent panel with the pedestal and Nyx.
fn paint_window(ui: &mut egui::Ui, shared: &Arc<Mutex<Shared>>) {
    let snap = shared.lock().map(|s| s.snap.clone()).unwrap_or_default();
    let frame = ui.max_rect();
    let t = ui.input(|i| i.time);
    let (outer, monitor) = ui.input(|i| (i.viewport().outer_rect, i.viewport().monitor_size));
    let mut colors = crate::pedestal::colors(snap.accent);
    let gain = voice_gain(snap.voice, t, snap.still);
    if gain != 1.0 {
        colors.glow = colors.glow.gamma_multiply(gain);
        colors.rim = colors
            .rim
            .gamma_multiply(gain.min(1.0) + (gain - 1.0) * 0.5);
    }
    let painter = ui.painter().with_clip_rect(frame);
    crate::pedestal::paint(&painter, frame, t, snap.still, &colors);
    let tex = outfit_texture(ui.ctx(), snap.mood, snap.adults_ok);
    let art = tex
        .as_ref()
        .map(|(tex, _)| art_rect(frame, tex.size()))
        .unwrap_or(frame);
    if let Some((tex, _)) = &tex {
        let bob = if snap.still {
            0.0
        } else {
            ((t * std::f64::consts::TAU / 4.0).sin() as f32) * 3.0
        };
        egui::Image::from_texture(tex)
            .corner_radius(crate::theme::RADIUS_SM)
            .paint_at(ui, art.translate(Vec2::new(0.0, bob)));
    }
    let resp = ui.interact(
        frame,
        ui.id().with("desktop-nyx"),
        egui::Sense::click_and_drag(),
    );
    let pointer = resp.hover_pos();
    let over = pointer
        .is_some_and(|p| over_art(p, frame, art, tex.as_ref().and_then(|(_, m)| m.as_ref())));
    if over {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.drag_started() && over {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    let mut events = Vec::new();
    if resp.clicked() && over {
        events.push(Event::OpenChat);
    }
    resp.context_menu(|ui| {
        ui.set_min_width(170.0);
        if ui.button("Push to talk").clicked() {
            events.push(Event::PushToTalk);
            ui.close();
        }
        let behind = if snap.behind {
            "Float above windows"
        } else {
            "Stay behind windows"
        };
        if ui.button(behind).clicked() {
            events.push(Event::ToggleBehind);
            ui.close();
        }
        ui.separator();
        if ui.button("Dock back").clicked() {
            events.push(Event::DockBack);
            ui.close();
        }
        if ui.button("Hide").clicked() {
            events.push(Event::Hide);
            ui.close();
        }
    });
    if let Ok(mut sh) = shared.lock() {
        sh.events.extend(events);
        sh.outer = outer;
        sh.monitor = monitor;
        sh.over_art = over;
    }
    if let Some(wait) = repaint_after(true, snap.still, false) {
        ui.ctx().request_repaint_after(wait);
    }
}

/// The mode whose embedded picture `outfit` is: `mood` itself when the
/// outfit is worn for it, else the first mode the outfit lists.
fn mood_for_outfit(outfit: &grokhub_core::companion::Outfit, mood: Mood) -> Mood {
    if outfit.moods.contains(&mood) {
        mood
    } else {
        outfit.moods.first().copied().unwrap_or(Mood::HangingOut)
    }
}

/// The outfit picture for `mood` and its hit mask (when it has transparent
/// pixels), cached on the context. Shipped art is embedded; a local adult
/// outfit is read from its file.
fn outfit_texture(
    ctx: &egui::Context,
    mood: Mood,
    adults_ok: bool,
) -> Option<(egui::TextureHandle, Option<HitMask>)> {
    let nyx = grokhub_core::companion::nyx();
    let outfit = nyx.outfit_for(mood, adults_ok);
    let key = format!("desktop-nyx-{}", outfit.id);
    let id = egui::Id::new(key.as_str());
    type Cached = (egui::TextureHandle, Option<HitMask>);
    if let Some(c) = ctx.data(|d| d.get_temp::<Cached>(id)) {
        return Some(c);
    }
    let bytes = super::loadout_home::mood_art(mood_for_outfit(outfit, mood));
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let has_alpha = rgba.as_raw().chunks_exact(4).any(|p| p[3] < 250);
    let mask = has_alpha.then(|| HitMask::from_rgba(w, h, rgba.as_raw(), 8));
    let tex = ctx.load_texture(
        key,
        egui::ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    let cached: Cached = (tex, mask);
    ctx.data_mut(|d| d.insert_temp(id, cached.clone()));
    Some(cached)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn she_hides_at_once_and_comes_back_four_seconds_after_the_game() {
        let t0 = Instant::now();
        let mut since = Some(t0 - RETURN_AFTER);
        assert!(visible_now(true, false, &mut since, t0));
        assert!(!visible_now(false, false, &mut since, t0), "docked");
        assert!(!visible_now(true, true, &mut since, t0), "gaming hides her");
        assert_eq!(since, None);
        assert!(!visible_now(
            true,
            false,
            &mut since,
            t0 + Duration::from_secs(1)
        ));
        assert_eq!(since, Some(t0 + Duration::from_secs(1)));
        assert!(!visible_now(
            true,
            false,
            &mut since,
            t0 + Duration::from_secs(4)
        ));
        assert!(visible_now(
            true,
            false,
            &mut since,
            t0 + Duration::from_secs(5)
        ));
        // A new game mid-return starts the wait over.
        assert!(!visible_now(
            true,
            true,
            &mut since,
            t0 + Duration::from_secs(6)
        ));
        assert!(!visible_now(
            true,
            false,
            &mut since,
            t0 + Duration::from_secs(9)
        ));
    }

    #[test]
    fn the_window_snaps_to_edges_and_stays_on_screen() {
        let mon = Vec2::new(1920.0, 1080.0);
        let size = WINDOW_SIZE;
        assert_eq!(
            snap_position(Pos2::new(10.0, 300.0), size, mon),
            Pos2::new(0.0, 300.0)
        );
        assert_eq!(
            snap_position(Pos2::new(1590.0, 590.0), size, mon),
            Pos2::new(1600.0, 600.0)
        );
        assert_eq!(
            snap_position(Pos2::new(500.0, 500.0), size, mon),
            Pos2::new(500.0, 500.0)
        );
        // Dragged mostly off the right edge: 64 px stay visible.
        assert_eq!(
            snap_position(Pos2::new(1900.0, 500.0), size, mon),
            Pos2::new(1856.0, 500.0)
        );
        assert_eq!(
            snap_position(Pos2::new(-500.0, -50.0), size, mon),
            Pos2::new(-256.0, 0.0)
        );
        assert_eq!(default_position(mon), Pos2::new(1576.0, 544.0));
        assert_eq!(default_position(Vec2::new(300.0, 400.0)), Pos2::ZERO);
    }

    #[test]
    fn repaint_cadence_follows_motion_and_stops_when_hidden() {
        assert_eq!(repaint_after(false, false, true), None);
        assert_eq!(repaint_after(true, false, false), Some(ANIM_FRAME));
        assert_eq!(repaint_after(true, true, true), Some(CURSOR_FRAME));
        assert_eq!(repaint_after(true, true, false), None);
    }

    #[test]
    fn click_through_is_on_over_empty_pixels_only_where_supported() {
        assert_eq!(passthrough_wanted(None, false, true), None);
        assert_eq!(passthrough_wanted(Some(false), false, true), Some(true));
        assert_eq!(passthrough_wanted(Some(true), false, true), Some(true));
        assert_eq!(passthrough_wanted(Some(true), true, true), Some(false));
        assert_eq!(
            passthrough_wanted(Some(false), false, false),
            Some(false),
            "X11"
        );
    }

    #[test]
    fn the_hit_mask_follows_the_alpha_channel() {
        // 4x2 picture: left half opaque, right half clear.
        let mut rgba = vec![0u8; 4 * 2 * 4];
        for y in 0..2 {
            for x in 0..2 {
                rgba[(y * 4 + x) * 4 + 3] = 255;
            }
        }
        let m = HitMask::from_rgba(4, 2, &rgba, 1);
        assert_eq!((m.cols, m.rows), (4, 2));
        assert!(m.hit(Pos2::new(0.1, 0.5)));
        assert!(m.hit(Pos2::new(0.4, 0.9)));
        assert!(!m.hit(Pos2::new(0.6, 0.5)));
        assert!(!m.hit(Pos2::new(1.2, 0.5)), "outside");
        assert!(!m.hit(Pos2::new(0.5, -0.1)));
        let coarse = HitMask::from_rgba(4, 2, &rgba, 2);
        assert_eq!((coarse.cols, coarse.rows), (2, 1));
        assert_eq!(coarse.cells, vec![true, false]);
    }

    #[test]
    fn over_art_counts_the_pedestal_disc_and_the_masked_picture() {
        let frame = Rect::from_min_size(Pos2::ZERO, WINDOW_SIZE);
        let l = crate::pedestal::layout(frame);
        assert!(over_art(l.center, frame, Rect::NOTHING, None), "disc");
        assert!(
            !over_art(Pos2::new(5.0, 5.0), frame, Rect::NOTHING, None),
            "empty corner"
        );
        let art = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(100.0, 200.0));
        assert!(
            over_art(Pos2::new(150.0, 100.0), frame, art, None),
            "no mask: whole picture"
        );
        let mut rgba = vec![0u8; 2 * 4];
        rgba[3] = 255;
        let m = HitMask::from_rgba(2, 1, &rgba, 1);
        assert!(
            over_art(Pos2::new(120.0, 100.0), frame, art, Some(&m)),
            "left half"
        );
        assert!(
            !over_art(Pos2::new(180.0, 100.0), frame, art, Some(&m)),
            "right half clear"
        );
    }

    #[test]
    fn the_viewport_is_transparent_undecorated_and_on_top_unless_behind() {
        let b = viewport_builder(Pos2::new(10.0, 20.0), false);
        assert_eq!(b.transparent, Some(true));
        assert_eq!(b.decorations, Some(false));
        assert_eq!(b.resizable, Some(false));
        assert_eq!(b.taskbar, Some(false));
        assert_eq!(b.inner_size, Some(WINDOW_SIZE));
        assert_eq!(b.position, Some(Pos2::new(10.0, 20.0)));
        assert_eq!(b.window_level, Some(egui::WindowLevel::AlwaysOnTop));
        assert_eq!(
            b.title.as_deref(),
            Some(grokhub_core::brand::companion_name())
        );
        let behind = viewport_builder(Pos2::ZERO, true);
        assert_eq!(behind.window_level, Some(egui::WindowLevel::Normal));
        assert_ne!(viewport_id(), egui::ViewportId::ROOT);
    }

    #[test]
    fn voice_lights_the_pedestal() {
        assert_eq!(voice_gain(VoiceLook::Idle, 1.0, false), 1.0);
        assert_eq!(voice_gain(VoiceLook::Listening, 1.0, false), 1.35);
        assert_eq!(voice_gain(VoiceLook::Speaking, 1.0, false), 1.2);
        let a = voice_gain(VoiceLook::Thinking, 0.0, false);
        let b = voice_gain(VoiceLook::Thinking, 0.14, false);
        assert_ne!(a, b, "thinking flickers");
        assert!((1.0..=1.18).contains(&a) && (1.0..=1.18).contains(&b));
        assert_eq!(
            voice_gain(VoiceLook::Thinking, 0.14, true),
            1.1,
            "still under reduced motion"
        );
    }

    #[test]
    fn the_cabin_only_shows_her_popped_out_with_the_switch_on() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        assert!(!cabin.cfg.desktop_nyx);
        assert!(!cabin.cfg.desktop_nyx_undocked);
        cabin.pop_nyx_out();
        assert!(cabin.cfg.desktop_nyx_undocked);
        cabin.dock_nyx_back();
        assert!(!cabin.cfg.desktop_nyx_undocked);
        cabin.toggle_desktop_nyx();
        assert!(cabin.cfg.desktop_nyx, "the tray turns the switch on");
        assert!(cabin.cfg.desktop_nyx_undocked);
        cabin.toggle_desktop_nyx();
        assert!(!cabin.cfg.desktop_nyx_undocked);
        let ctx = egui::Context::default();
        cabin.tick_desktop_nyx(&ctx);
        assert!(cabin.desktop_nyx.is_none(), "docked: no window state");
    }

    #[test]
    fn events_change_the_settings_they_name() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        let ctx = egui::Context::default();
        cabin.cfg.desktop_nyx = true;
        cabin.cfg.desktop_nyx_undocked = true;
        cabin.on_desktop_nyx_event(Event::ToggleBehind, &ctx);
        assert!(cabin.cfg.desktop_nyx_behind);
        cabin.on_desktop_nyx_event(Event::Hide, &ctx);
        assert!(!cabin.cfg.desktop_nyx_undocked);
        cabin.cfg.desktop_nyx_undocked = true;
        cabin.on_desktop_nyx_event(Event::DockBack, &ctx);
        assert!(!cabin.cfg.desktop_nyx_undocked);
    }
}
