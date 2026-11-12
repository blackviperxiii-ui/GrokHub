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

pub fn save_pos(
    mut settings: crate::settings::ShellSettings,
    pos: [f64; 2],
    monitor: [f64; 2],
) -> crate::settings::ShellSettings {
    let [x, y] = settled_pos(pos, monitor);
    settings.desktop_nyx_pos = Some([x.round() as i32, y.round() as i32]);
    settings
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

#[cfg(desktop)]
pub fn set_passthrough(app: &tauri::AppHandle, ignore: bool) -> Result<(), String> {
    use tauri::Manager;
    let Some(w) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    w.set_ignore_cursor_events(ignore)
        .map_err(|e| e.to_string())
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
        s = save_pos(s, [10.0, 300.0], mon);
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
