//! Desktop Nyx: a transparent, undecorated always-on-top window for the
//! companion on the desktop (parity with the cabin's `desktop_nyx`). Off by
//! default behind Labs → Desktop companion (`desktopNyx`). The page is
//! `nyx-desk.html`; click-through uses `set_ignore_cursor_events` so empty
//! pixels pass through and the figure stays clickable.

use grokhub_core::brand;
use grokhub_core::companion::{self, Mood};
use serde::{Deserialize, Serialize};

/// Webview label and HTML entry (must match Vite's `nyx-desk` page).
pub const LABEL: &str = "desktop-nyx";
pub const PAGE: &str = "nyx-desk.html";

/// Sent to every page after any desktop-companion change, with the new
/// [`State`]: the dock, Labs and the desk page all follow it.
pub const EVENT: &str = "desktop-nyx://changed";

/// The tray menu's "Nyx on desktop" item.
pub const TRAY_ID: &str = "desktop-nyx";

/// Window size in logical points (cabin `WINDOW_SIZE`).
pub const WIDTH: f64 = 320.0;
pub const HEIGHT: f64 = 480.0;

/// Builder flags the cabin viewport uses, as a plain struct so tests can
/// check them without a display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSpec {
    pub transparent: bool,
    pub decorations: bool,
    pub resizable: bool,
    pub skip_taskbar: bool,
    pub always_on_top: bool,
}

/// Spec for the desktop window. `behind` flips always-on-top off.
pub fn window_spec(behind: bool) -> WindowSpec {
    WindowSpec {
        transparent: true,
        decorations: false,
        resizable: false,
        skip_taskbar: true,
        always_on_top: !behind,
    }
}

/// Default outer position: bottom-right of a monitor with a margin.
pub fn default_position(monitor_w: f64, monitor_h: f64) -> [f64; 2] {
    [
        (monitor_w - WIDTH - 24.0).max(0.0),
        (monitor_h - HEIGHT - 56.0).max(0.0),
    ]
}

/// Snap near edges and keep 64 px on screen (cabin `snap_position`).
pub fn snap_position(pos: [f64; 2], monitor: [f64; 2]) -> [f64; 2] {
    const SNAP: f64 = 24.0;
    const KEEP: f64 = 64.0;
    let (mut x, mut y) = (pos[0], pos[1]);
    let (mw, mh) = (monitor[0], monitor[1]);
    let right = mw - WIDTH;
    let bottom = mh - HEIGHT;
    if x.abs() <= SNAP {
        x = 0.0;
    } else if (x - right).abs() <= SNAP {
        x = right;
    }
    if y.abs() <= SNAP {
        y = 0.0;
    } else if (y - bottom).abs() <= SNAP {
        y = bottom;
    }
    x = x.clamp(KEEP - WIDTH, mw - KEEP);
    y = y.clamp(0.0, (mh - KEEP).max(0.0));
    [x, y]
}

/// Round-trip helper: snap then keep for settings.
pub fn settled_pos(pos: [f64; 2], monitor: [f64; 2]) -> [f64; 2] {
    snap_position(pos, monitor)
}

/// Menu label for an outfit id (`hanging-out` → "Hanging out").
pub fn outfit_label(id: &str) -> String {
    let mut s = id.replace('-', " ");
    if let Some(first) = s.get(..1) {
        let up = first.to_ascii_uppercase();
        s.replace_range(..1, &up);
    }
    s
}

/// One wardrobe row the UI can pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutfitChoice {
    pub id: String,
    pub label: String,
    pub file: String,
    pub adult: bool,
}

/// Adults-only outfits on the desktop need both gates (cabin spirit).
pub fn adults_ok(settings: &crate::settings::ShellSettings) -> bool {
    settings.adults_on() && settings.desktop_nyx_adult
}

/// Outfits the wardrobe may show right now.
pub fn outfit_choices(adults_ok: bool) -> Vec<OutfitChoice> {
    companion::nyx()
        .outfits
        .iter()
        .filter(|o| adults_ok || !o.adult)
        .map(|o| OutfitChoice {
            id: o.id.clone(),
            label: outfit_label(&o.id),
            file: o.file.clone(),
            adult: o.adult,
        })
        .collect()
}

/// The picture to show: pinned when allowed, else the hanging-out default.
pub fn shown_outfit(pin: Option<&str>, adults_ok: bool) -> &companion::Outfit {
    let nyx = companion::nyx();
    if let Some(id) = pin {
        if let Some(o) = nyx.outfit(id).filter(|o| adults_ok || !o.adult) {
            return o;
        }
    }
    nyx.outfit_for(Mood::HangingOut, adults_ok)
}

/// Best-effort: Wayland often drops transparency and always-on-top.
pub fn effects_likely(wayland: bool) -> Effects {
    Effects {
        transparent: !wayland,
        on_top: !wayland,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Effects {
    pub transparent: bool,
    pub on_top: bool,
}

/// Full state the dock, Labs and desk page share.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub enabled: bool,
    pub undocked: bool,
    pub behind: bool,
    pub adult: bool,
    pub adults_ok: bool,
    pub outfit: Option<String>,
    pub outfit_file: String,
    pub outfits: Vec<OutfitChoice>,
    pub pos: Option<[i32; 2]>,
    pub effects: Effects,
    pub companion: String,
}

pub fn state_of(settings: &crate::settings::ShellSettings, wayland: bool) -> State {
    let adults = adults_ok(settings);
    let shown = shown_outfit(settings.desktop_nyx_outfit.as_deref(), adults);
    State {
        enabled: settings.desktop_nyx,
        undocked: settings.desktop_nyx_undocked,
        behind: settings.desktop_nyx_behind,
        adult: settings.desktop_nyx_adult,
        adults_ok: adults,
        outfit: settings.desktop_nyx_outfit.clone(),
        outfit_file: shown.file.clone(),
        outfits: outfit_choices(adults),
        pos: settings.desktop_nyx_pos,
        effects: effects_likely(wayland),
        companion: brand::companion_name().to_string(),
    }
}

/// Patch fields from the UI; returns the next settings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub enabled: Option<bool>,
    pub undocked: Option<bool>,
    pub behind: Option<bool>,
    pub adult: Option<bool>,
}

pub fn apply_patch(
    mut settings: crate::settings::ShellSettings,
    patch: Patch,
) -> crate::settings::ShellSettings {
    if let Some(on) = patch.enabled {
        settings.desktop_nyx = on;
        if !on {
            settings.desktop_nyx_undocked = false;
        }
    }
    if let Some(u) = patch.undocked {
        settings.desktop_nyx_undocked = u && settings.desktop_nyx;
    }
    if let Some(b) = patch.behind {
        settings.desktop_nyx_behind = b;
    }
    if let Some(a) = patch.adult {
        settings.desktop_nyx_adult = a;
    }
    settings
}

pub fn set_outfit(
    mut settings: crate::settings::ShellSettings,
    id: Option<String>,
) -> Result<crate::settings::ShellSettings, String> {
    match id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => {
            settings.desktop_nyx_outfit = None;
            Ok(settings)
        }
        Some(id) => {
            let adults = adults_ok(&settings);
            let o = companion::nyx()
                .outfit(id)
                .filter(|o| adults || !o.adult)
                .ok_or_else(|| format!("Outfit {id} isn't available."))?;
            settings.desktop_nyx_outfit = Some(o.id.clone());
            Ok(settings)
        }
    }
}

/// Saves where she was dropped. `pos` and the monitor's `origin` are desktop
/// coordinates; snapping and keeping her on screen work within that monitor.
pub fn save_pos(
    mut settings: crate::settings::ShellSettings,
    pos: [f64; 2],
    origin: [f64; 2],
    size: [f64; 2],
) -> crate::settings::ShellSettings {
    let [x, y] = settled_pos([pos[0] - origin[0], pos[1] - origin[1]], size);
    settings.desktop_nyx_pos = Some([
        (x + origin[0]).round() as i32,
        (y + origin[1]).round() as i32,
    ]);
    settings
}

/// The tray item's words, the same as the old cabin's: "Nyx on desktop".
pub fn tray_label() -> String {
    format!("{} on desktop", brand::companion_name())
}

/// What the tray item shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayItem {
    pub text: String,
    pub enabled: bool,
    pub checked: bool,
}

/// Enabled only with Labs → Desktop companion on; checked while she is out
/// on the desktop. (Later, a game hiding her leaves it checked: she is still
/// out, only hidden.)
pub fn tray_item(settings: &crate::settings::ShellSettings) -> TrayItem {
    TrayItem {
        text: tray_label(),
        enabled: settings.desktop_nyx,
        checked: settings.desktop_nyx && settings.desktop_nyx_undocked,
    }
}

/// A tray click: pop her out, or dock her when she is out. `None` while the
/// Labs switch is off, so a stale click never turns the Lab on.
pub fn tray_toggle(
    settings: crate::settings::ShellSettings,
) -> Option<crate::settings::ShellSettings> {
    if !settings.desktop_nyx {
        return None;
    }
    let out = !settings.desktop_nyx_undocked;
    Some(apply_patch(
        settings,
        Patch {
            undocked: Some(out),
            ..Patch::default()
        },
    ))
}

#[cfg(desktop)]
static TRAY_ITEM: std::sync::OnceLock<tauri::menu::CheckMenuItem<tauri::Wry>> =
    std::sync::OnceLock::new();

/// Keeps the tray item so every change can update it.
#[cfg(desktop)]
pub fn start_tray(item: tauri::menu::CheckMenuItem<tauri::Wry>) {
    let _ = TRAY_ITEM.set(item);
}

/// Writes the settings' answer into the tray item. A click flips the tick
/// itself, so a click that changes nothing must call this to undo it.
#[cfg(desktop)]
pub fn refresh_tray(settings: &crate::settings::ShellSettings) {
    if let Some(item) = TRAY_ITEM.get() {
        let t = tray_item(settings);
        let _ = item.set_enabled(t.enabled);
        let _ = item.set_checked(t.checked);
    }
}

/// Every desktop-companion change ends here: the tray item and every page.
#[cfg(desktop)]
pub fn publish(app: &tauri::AppHandle, settings: &crate::settings::ShellSettings) -> State {
    use tauri::Emitter;
    refresh_tray(settings);
    let state = state_of(settings, wayland_now());
    let _ = app.emit(EVENT, &state);
    state
}

/// Create or focus the desktop window. No-op when the Labs switch is off.
#[cfg(desktop)]
pub fn show_window(
    app: &tauri::AppHandle,
    settings: &crate::settings::ShellSettings,
) -> Result<(), String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    if !(settings.desktop_nyx && settings.desktop_nyx_undocked) {
        return hide_window(app);
    }
    let spec = window_spec(settings.desktop_nyx_behind);
    if let Some(existing) = app.get_webview_window(LABEL) {
        let _ = existing.set_always_on_top(spec.always_on_top);
        existing.show().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(PAGE.into()))
        .title(brand::companion_name())
        .inner_size(WIDTH, HEIGHT)
        .transparent(spec.transparent)
        .decorations(spec.decorations)
        .resizable(spec.resizable)
        .skip_taskbar(spec.skip_taskbar)
        .always_on_top(spec.always_on_top)
        .shadow(false)
        .visible(true);
    if let Some([x, y]) = settings.desktop_nyx_pos {
        builder = builder.position(f64::from(x), f64::from(y));
    } else {
        let [x, y] = default_position(1920.0, 1080.0);
        builder = builder.position(x, y);
    }
    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(desktop)]
pub fn hide_window(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window(LABEL) {
        w.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// How often the desk watch looks at the cursor (the cabin's frame).
pub const CURSOR_FRAME: std::time::Duration = std::time::Duration::from_millis(100);

/// A dragged window counts as dropped once it has been still this long.
pub const POS_SETTLE: std::time::Duration = std::time::Duration::from_millis(600);

/// Most boxes her page may report; anything past it is dropped.
pub const MAX_HIT_BOXES: usize = 8;

/// A part of her page that takes the mouse (her stage, an open menu), in
/// CSS pixels from the window's top-left.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct HitBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The page's boxes, keeping only real ones and at most [`MAX_HIT_BOXES`].
pub fn clean_hit(boxes: Vec<HitBox>) -> Vec<HitBox> {
    boxes
        .into_iter()
        .filter(|b| [b.x, b.y, b.w, b.h].iter().all(|v| v.is_finite()) && b.w > 0.0 && b.h > 0.0)
        .take(MAX_HIT_BOXES)
        .collect()
}

/// A point on the desktop and the scale it was read at (desktop pixels per
/// CSS pixel).
pub type Scaled = ([f64; 2], f64);

fn unscale((at, scale): Scaled) -> [f64; 2] {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    [at[0] / scale, at[1] / scale]
}

/// Whether the cursor is over one of her boxes, in a window whose page
/// starts at `origin`. Each point carries its own scale: macOS reads the
/// cursor at the main screen's scale and the window at its own.
pub fn over_her(cursor: Scaled, origin: Scaled, boxes: &[HitBox]) -> bool {
    let [cx, cy] = unscale(cursor);
    let [ox, oy] = unscale(origin);
    let (x, y) = (cx - ox, cy - oy);
    boxes
        .iter()
        .any(|b| x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h)
}

/// Whether her window should let clicks through: yes over empty pixels, no
/// over her. When the cursor or the window can't be read (Wayland reads
/// both as 0,0, so it passes `None`), or the page hasn't reported her boxes
/// yet, never: she stays clickable.
pub fn passthrough_wanted(
    cursor: Option<Scaled>,
    origin: Option<Scaled>,
    boxes: &[HitBox],
) -> bool {
    match (cursor, origin) {
        (Some(c), Some(o)) if !boxes.is_empty() => !over_her(c, o, boxes),
        _ => false,
    }
}

/// Where a dragged window was dropped, in logical desktop pixels, with the
/// monitor it landed on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Landing {
    pub pos: [f64; 2],
    pub origin: [f64; 2],
    pub size: [f64; 2],
}

/// A drag paused longer than [`POS_SETTLE`] but resumed within this still
/// counts as the same drag.
pub const DRAG_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Her window's moves during a drag, until it has been still for
/// [`POS_SETTLE`]. Every other move (the window placed at open, a window
/// manager restacking it, Windows parking a minimized window at -32000, a
/// monitor unplugged) is not hers to save.
#[derive(Debug, Default)]
pub struct Moves {
    dragging: bool,
    last: Option<(std::time::Instant, Landing)>,
    settled_at: Option<std::time::Instant>,
    saved: Option<[f64; 2]>,
}

impl Moves {
    /// A drag starts: until she settles, moves are where she is being put.
    pub fn drag(&mut self) {
        self.dragging = true;
    }

    pub fn note(&mut self, drop: Landing, at: std::time::Instant) {
        let resumed = self
            .settled_at
            .is_some_and(|then| at.saturating_duration_since(then) <= DRAG_GRACE);
        if self.dragging || resumed {
            self.dragging = true;
            self.last = Some((at, drop));
        }
    }

    /// The spot to save, once, after the window has been still long enough
    /// and only when it differs from the last one saved. Settling ends the
    /// drag.
    pub fn settled(&mut self, now: std::time::Instant) -> Option<Landing> {
        let (at, drop) = self.last?;
        if now.saturating_duration_since(at) < POS_SETTLE {
            return None;
        }
        self.last = None;
        self.dragging = false;
        self.settled_at = Some(now);
        if self.saved == Some(drop.pos) {
            return None;
        }
        self.saved = Some(drop.pos);
        Some(drop)
    }
}

/// What the desk watch keeps between frames while her window is open.
#[cfg(desktop)]
#[derive(Debug, Default)]
struct Desk {
    boxes: Vec<HitBox>,
    passthrough: Option<bool>,
    moves: Moves,
}

#[cfg(desktop)]
static DESK: std::sync::Mutex<Desk> = std::sync::Mutex::new(Desk {
    boxes: Vec::new(),
    passthrough: None,
    moves: Moves {
        dragging: false,
        last: None,
        settled_at: None,
        saved: None,
    },
});

#[cfg(desktop)]
fn desk() -> std::sync::MutexGuard<'static, Desk> {
    DESK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Her page's boxes; the next frame of the desk watch applies them.
#[cfg(desktop)]
pub fn set_hit(boxes: Vec<HitBox>) {
    desk().boxes = clean_hit(boxes);
}

/// Her window moved. Only moves during a drag count (see [`Moves`]), never
/// a minimize. On Wayland the window can't read where it is, so nothing is
/// saved.
#[cfg(desktop)]
pub fn note_moved(window: &tauri::Window, pos: tauri::PhysicalPosition<i32>) {
    if wayland_now() || window.is_minimized().unwrap_or(false) {
        return;
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    let logical = |x: f64, y: f64| unscale(([x, y], scale));
    let (origin, size) = window
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let (at, size) = (m.position(), m.size());
            (
                logical(f64::from(at.x), f64::from(at.y)),
                logical(f64::from(size.width), f64::from(size.height)),
            )
        })
        .unwrap_or(([0.0, 0.0], [1920.0, 1080.0]));
    let drop = Landing {
        pos: logical(f64::from(pos.x), f64::from(pos.y)),
        origin,
        size,
    };
    desk().moves.note(drop, std::time::Instant::now());
}

/// A press on her: the system moves the window until the button comes up.
#[cfg(desktop)]
pub fn start_drag(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let Some(w) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    desk().moves.drag();
    w.start_dragging().map_err(|e| e.to_string())
}

/// One frame of the desk watch: click-through follows the cursor, so she
/// can be clicked again after the pointer crossed an empty pixel (once the
/// window lets clicks through, her page hears no pointer at all). Returns a
/// dropped position to save. The desk lock is never held across a window
/// call: those wait on the main thread, which takes the lock on a move.
#[cfg(desktop)]
pub fn tick(app: &tauri::AppHandle) -> Option<Landing> {
    use tauri::Manager;
    let Some(w) = app.get_webview_window(LABEL) else {
        *desk() = Desk::default();
        return None;
    };
    let (boxes, had) = {
        let d = desk();
        (d.boxes.clone(), d.passthrough)
    };
    let scale = w.scale_factor().unwrap_or(1.0);
    // macOS reads the cursor at the main screen's scale.
    let cursor_scale = if cfg!(target_os = "macos") {
        app.primary_monitor()
            .ok()
            .flatten()
            .map_or(scale, |m| m.scale_factor())
    } else {
        scale
    };
    let cursor = (!wayland_now())
        .then(|| app.cursor_position().ok())
        .flatten()
        .map(|p| ([p.x, p.y], cursor_scale));
    let origin = w
        .inner_position()
        .ok()
        .map(|p| ([f64::from(p.x), f64::from(p.y)], scale));
    let want = passthrough_wanted(cursor, origin, &boxes);
    let applied = (had != Some(want) && w.set_ignore_cursor_events(want).is_ok()).then_some(want);
    let mut d = desk();
    if applied.is_some() {
        d.passthrough = applied;
    }
    d.moves.settled(std::time::Instant::now())
}

#[cfg(desktop)]
pub fn apply_behind(app: &tauri::AppHandle, behind: bool) -> Result<(), String> {
    use tauri::Manager;
    let Some(w) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    w.set_always_on_top(!behind).map_err(|e| e.to_string())
}

/// Wayland session probe used for the Settings note.
pub fn wayland_now() -> bool {
    crate::capabilities::wayland_session(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ShellSettings;

    fn labs_on(undocked: bool) -> ShellSettings {
        ShellSettings {
            desktop_nyx: true,
            desktop_nyx_undocked: undocked,
            ..ShellSettings::default()
        }
    }

    fn item(enabled: bool, checked: bool) -> TrayItem {
        TrayItem {
            text: "Nyx on desktop".into(),
            enabled,
            checked,
        }
    }

    fn hit(x: f64, y: f64, w: f64, h: f64) -> HitBox {
        HitBox { x, y, w, h }
    }

    #[test]
    fn clicks_pass_through_only_beside_her() {
        let stage = [hit(40.0, 100.0, 240.0, 360.0)];
        // A window at (1000, 500) on a 2x screen: page (50, 150) is desktop (1100, 800).
        let at = Some(([1000.0, 500.0], 2.0));
        let cursor = |x, y| Some(([x, y], 2.0));
        assert!(!passthrough_wanted(cursor(1100.0, 800.0), at, &stage));
        assert!(passthrough_wanted(cursor(1010.0, 510.0), at, &stage));
        // The right edge is outside: 40 + 240 = 280 page px = desktop 1560.
        assert!(passthrough_wanted(cursor(1560.0, 800.0), at, &stage));
        assert!(!passthrough_wanted(cursor(1559.0, 800.0), at, &stage));
        // An open menu takes the mouse too.
        let with_menu = [stage[0], hit(0.0, 0.0, 120.0, 80.0)];
        assert!(!passthrough_wanted(cursor(1010.0, 510.0), at, &with_menu));
    }

    #[test]
    fn the_cursor_and_the_window_may_be_read_at_different_scales() {
        // macOS: the cursor at a 2x main screen's scale, her window on a 1x
        // monitor at (2000, 100). Page (50, 150) is the cursor (4100, 500).
        let stage = [hit(40.0, 100.0, 240.0, 360.0)];
        assert!(over_her(
            ([4100.0, 500.0], 2.0),
            ([2000.0, 100.0], 1.0),
            &stage
        ));
        assert!(!over_her(
            ([4100.0, 500.0], 1.0),
            ([2000.0, 100.0], 1.0),
            &stage
        ));
    }

    #[test]
    fn she_stays_clickable_when_nothing_can_be_read() {
        let stage = [hit(40.0, 100.0, 240.0, 360.0)];
        let origin = Some(([0.0, 0.0], 1.0));
        assert!(
            !passthrough_wanted(None, origin, &stage),
            "no cursor (Wayland)"
        );
        assert!(
            !passthrough_wanted(Some(([5.0, 5.0], 1.0)), None, &stage),
            "no window position"
        );
        assert!(
            !passthrough_wanted(Some(([5.0, 5.0], 1.0)), origin, &[]),
            "no boxes yet"
        );
        // A bad scale reads as 1.
        assert!(over_her(([50.0, 150.0], 0.0), ([0.0, 0.0], 1.0), &stage));
        assert!(over_her(
            ([50.0, 150.0], f64::NAN),
            ([0.0, 0.0], 1.0),
            &stage
        ));
    }

    #[test]
    fn the_page_boxes_are_checked_at_the_boundary() {
        let boxes: Vec<HitBox> =
            serde_json::from_str(r#"[{"x":1,"y":2,"w":3,"h":4},{"x":0,"y":0,"w":0,"h":9}]"#)
                .expect("the page's JSON");
        assert_eq!(clean_hit(boxes), vec![hit(1.0, 2.0, 3.0, 4.0)]);
        assert_eq!(clean_hit(vec![hit(f64::INFINITY, 0.0, 5.0, 5.0)]), vec![]);
        assert_eq!(clean_hit(vec![hit(0.0, 0.0, 5.0, 5.0); 20]).len(), 8);
    }

    #[test]
    fn a_drag_is_saved_once_after_she_has_been_still() {
        let ms = std::time::Duration::from_millis;
        let screen = |x, y| Landing {
            pos: [x, y],
            origin: [0.0, 0.0],
            size: [1920.0, 1080.0],
        };
        let t0 = std::time::Instant::now();
        let mut moves = Moves::default();
        moves.note(screen(1576.0, 544.0), t0);
        assert_eq!(
            moves.settled(t0 + ms(5000)),
            None,
            "placed at open: not a drag"
        );
        moves.drag();
        moves.note(screen(10.0, 20.0), t0);
        moves.note(screen(30.0, 40.0), t0 + ms(100));
        assert_eq!(moves.settled(t0 + ms(600)), None, "still moving");
        assert_eq!(
            moves.settled(t0 + ms(700)),
            Some(screen(30.0, 40.0)),
            "the last spot wins"
        );
        assert_eq!(moves.settled(t0 + ms(2000)), None, "once");
        // A drag that pauses and goes on within 2 s is the same drag.
        moves.note(screen(30.0, 40.0), t0 + ms(2500));
        assert_eq!(
            moves.settled(t0 + ms(3500)),
            None,
            "the same spot again is not saved"
        );
        moves.note(screen(50.0, 60.0), t0 + ms(4000));
        assert_eq!(moves.settled(t0 + ms(4700)), Some(screen(50.0, 60.0)));
        // Long after the drop, the system moving her is not saved.
        moves.note(screen(-32000.0, -32000.0), t0 + ms(9000));
        assert_eq!(moves.settled(t0 + ms(10_000)), None, "a move with no drag");
        assert_eq!(DRAG_GRACE.as_secs(), 2);
        assert_eq!(CURSOR_FRAME.as_millis(), 100);
        assert_eq!(POS_SETTLE.as_millis(), 600);
    }

    #[test]
    fn a_drop_on_another_monitor_stays_there() {
        let s = ShellSettings::default();
        // Right of a 1920-wide primary: 580 px into the second monitor.
        let right = save_pos(s.clone(), [2500.0, 300.0], [1920.0, 0.0], [1920.0, 1080.0]);
        assert_eq!(right.desktop_nyx_pos, Some([2500, 300]));
        // Left of it, snapped to that monitor's left edge.
        let left = save_pos(
            s.clone(),
            [-1910.0, 300.0],
            [-1920.0, 0.0],
            [1920.0, 1080.0],
        );
        assert_eq!(left.desktop_nyx_pos, Some([-1920, 300]));
        // Above it, kept on that monitor.
        let above = save_pos(s, [500.0, -1070.0], [0.0, -1080.0], [1920.0, 1080.0]);
        assert_eq!(above.desktop_nyx_pos, Some([500, -1080]));
    }

    #[test]
    fn the_tray_item_reads_nyx_on_desktop_and_waits_for_labs() {
        assert_eq!(tray_item(&ShellSettings::default()), item(false, false));
        assert_eq!(EVENT, "desktop-nyx://changed");
        assert_eq!(TRAY_ID, "desktop-nyx");
    }

    #[test]
    fn the_tray_item_is_checked_only_while_she_is_out() {
        assert_eq!(tray_item(&labs_on(false)), item(true, false));
        assert_eq!(tray_item(&labs_on(true)), item(true, true));
        let stale = ShellSettings {
            desktop_nyx_undocked: true,
            ..ShellSettings::default()
        };
        assert_eq!(tray_item(&stale), item(false, false), "Labs off wins");
    }

    #[test]
    fn the_tray_toggle_pops_her_out_then_docks_her_and_never_flips_labs() {
        assert_eq!(tray_toggle(ShellSettings::default()), None);
        let start = ShellSettings {
            desktop_nyx_pos: Some([100, 200]),
            desktop_nyx_behind: true,
            desktop_nyx_outfit: Some("hanging-out".into()),
            ..labs_on(false)
        };
        let out = tray_toggle(start).expect("Labs is on");
        assert!(out.desktop_nyx);
        assert!(out.desktop_nyx_undocked);
        let back = tray_toggle(out).expect("Labs is on");
        assert!(back.desktop_nyx);
        assert!(!back.desktop_nyx_undocked);
        assert_eq!(back.desktop_nyx_pos, Some([100, 200]));
        assert!(back.desktop_nyx_behind);
        assert_eq!(back.desktop_nyx_outfit.as_deref(), Some("hanging-out"));
    }

    #[test]
    fn the_tray_item_follows_dock_and_labs_off() {
        let off = apply_patch(
            labs_on(true),
            Patch {
                enabled: Some(false),
                ..Patch::default()
            },
        );
        assert_eq!(tray_item(&off), item(false, false));
        let docked = apply_patch(
            labs_on(true),
            Patch {
                undocked: Some(false),
                ..Patch::default()
            },
        );
        assert_eq!(tray_item(&docked), item(true, false));
    }

    #[test]
    fn the_window_spec_is_transparent_undecorated_and_on_top_unless_behind() {
        let on = window_spec(false);
        assert!(on.transparent);
        assert!(!on.decorations);
        assert!(!on.resizable);
        assert!(on.skip_taskbar);
        assert!(on.always_on_top);
        assert!(!window_spec(true).always_on_top);
        assert_eq!(LABEL, "desktop-nyx");
        assert_eq!(PAGE, "nyx-desk.html");
        assert_eq!((WIDTH, HEIGHT), (320.0, 480.0));
    }

    #[test]
    fn position_round_trips_through_snap() {
        let mon = [1920.0, 1080.0];
        assert_eq!(snap_position([10.0, 300.0], mon), [0.0, 300.0]);
        assert_eq!(snap_position([1590.0, 590.0], mon), [1600.0, 600.0]);
        assert_eq!(snap_position([500.0, 500.0], mon), [500.0, 500.0]);
        assert_eq!(
            settled_pos([1900.0, 500.0], mon),
            [1856.0, 500.0],
            "64 px stay on screen"
        );
        assert_eq!(default_position(1920.0, 1080.0), [1576.0, 544.0]);
        let mut s = ShellSettings::default();
        s = save_pos(s, [10.0, 300.0], [0.0, 0.0], mon);
        assert_eq!(s.desktop_nyx_pos, Some([0, 300]));
    }

    #[test]
    fn off_by_default_and_patch_clears_undock_when_switched_off() {
        let s = ShellSettings::default();
        assert!(!s.desktop_nyx);
        assert!(!s.desktop_nyx_undocked);
        assert!(!s.desktop_nyx_behind);
        assert!(!s.desktop_nyx_adult);
        assert!(s.desktop_nyx_outfit.is_none());
        assert!(s.desktop_nyx_pos.is_none());
        let on = apply_patch(
            s,
            Patch {
                enabled: Some(true),
                undocked: Some(true),
                ..Patch::default()
            },
        );
        assert!(on.desktop_nyx && on.desktop_nyx_undocked);
        let off = apply_patch(
            on,
            Patch {
                enabled: Some(false),
                ..Patch::default()
            },
        );
        assert!(!off.desktop_nyx && !off.desktop_nyx_undocked);
    }

    #[test]
    fn adult_outfits_need_both_gates_and_wardrobe_filters() {
        let mut s = ShellSettings {
            explicit_personalities: true,
            adult_confirmed: true,
            desktop_nyx_adult: false,
            ..Default::default()
        };
        assert!(!adults_ok(&s));
        let safe = outfit_choices(false);
        assert!(safe.iter().all(|o| !o.adult));
        assert!(safe.iter().any(|o| o.id == "hanging-out"));
        assert!(!safe.iter().any(|o| o.id == "raunchy"));
        s.desktop_nyx_adult = true;
        assert!(adults_ok(&s));
        let all = outfit_choices(true);
        assert!(all.iter().any(|o| o.id == "raunchy"));
        assert_eq!(outfit_label("hanging-out"), "Hanging out");
        assert_eq!(outfit_label("viper"), "Viper");
    }

    #[test]
    fn pinned_outfit_wins_unless_adult_and_blocked() {
        assert_eq!(shown_outfit(Some("gaming"), false).id, "gaming");
        assert_eq!(
            shown_outfit(Some("raunchy"), false).id,
            "hanging-out",
            "blocked adult falls back"
        );
        assert_eq!(shown_outfit(Some("raunchy"), true).id, "raunchy");
        assert_eq!(shown_outfit(None, false).id, "hanging-out");
        let mut s = ShellSettings::default();
        s = set_outfit(s, Some("gaming".into())).expect("gaming");
        assert_eq!(s.desktop_nyx_outfit.as_deref(), Some("gaming"));
        s = set_outfit(s, None).expect("auto");
        assert!(s.desktop_nyx_outfit.is_none());
        assert!(set_outfit(s, Some("raunchy".into())).is_err());
    }

    #[test]
    fn wayland_marks_effects_unlikely() {
        assert_eq!(
            effects_likely(false),
            Effects {
                transparent: true,
                on_top: true
            }
        );
        assert_eq!(
            effects_likely(true),
            Effects {
                transparent: false,
                on_top: false
            }
        );
    }

    #[test]
    fn state_names_the_companion_and_switch_key_fields() {
        let s = ShellSettings::default();
        let st = state_of(&s, false);
        assert!(!st.enabled);
        assert!(!st.undocked);
        assert_eq!(st.companion, brand::companion_name());
        assert!(!st.outfit_file.is_empty());
        assert!(st.outfit_file.starts_with("brand/"));
    }
}
