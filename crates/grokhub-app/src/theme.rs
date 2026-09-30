//! Official Grok dark tokens (grok.com / iOS / Android, 2026-09).
//! Recreated in egui — no grok.com JS, no webview.
//! Dark-first: OLED canvas, quiet chrome. Composer stays a Grok column; chat text is fluid.

use eframe::egui::{
    self, Color32, ColorImage, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    TextureHandle, TextureOptions,
};
use grokhub_core::{
    clamp_rect_to_slot, feel_lift, feel_scale, felt_rect, hover_alpha, hover_mix, lift_rgb,
    mix_channel,
    os_prefers_dark, HOVER_EXPANSION, HOVER_SECS, HOVER_WASH, PRESS_EXPANSION, PRESS_SECS,
    SELECT_SECS,
};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub fn title_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("inter-bold".into()))
}

/// Official Grok canvas — true black, not elevated IDE gray.
pub const BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
/// Surface / user bubble / composer fill `#16181C`.
pub const SURFACE: Color32 = Color32::from_rgb(0x16, 0x18, 0x1c);
/// Quiet lifted chrome (menus, sheets) `#1A1A1A`.
pub const PANEL: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
/// Same family as surface — input and bubble share one plane.
pub const ELEVATED: Color32 = Color32::from_rgb(0x16, 0x18, 0x1c);
/// Surface hover `#1C1F23`.
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x1c, 0x1f, 0x23);
/// Official Grok dark hover / selection `#1C1F23` — not a white or cream wash.
pub const HOVER: Color32 = Color32::from_rgb(0x1c, 0x1f, 0x23);
/// Text primary `#E7E9EA`.
pub const FG: Color32 = Color32::from_rgb(0xe7, 0xe9, 0xea);
/// Text secondary `#71767B`.
pub const MUTED: Color32 = Color32::from_rgb(0x71, 0x76, 0x7b);
/// Meta / thought — one step quieter than secondary.
pub const SUBTLE: Color32 = Color32::from_rgb(0x5c, 0x61, 0x66);
/// Empty-home greeting — title 20–28, not a product wordmark.
pub const GREET_HERO: f32 = 28.0;
/// Hairline `#2F3336`.
pub const BORDER: Color32 = Color32::from_rgb(0x2f, 0x33, 0x36);
/// Slightly stronger hairline for an active ring.
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x3d, 0x43, 0x48);
/// Selected rail row — surface hover, not a colored accent.
pub const NAV_ACTIVE: Color32 = Color32::from_rgb(0x1c, 0x1f, 0x23);
pub const BUBBLE_USER: Color32 = Color32::from_rgb(0x16, 0x18, 0x1c);
/// Citation / link only. Never chrome.
pub const LINK: Color32 = Color32::from_rgb(0x1d, 0x9b, 0xf0);
/// Send/on inverse fill.
pub const SEND_ON: Color32 = Color32::WHITE;
pub const SEND_ON_INK: Color32 = Color32::BLACK;
pub const LIVE: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);
/// Live on a light surface: `#22C55E` is ~2.3:1 on white; green-700 is ~5:1.
pub const LIGHT_LIVE: Color32 = Color32::from_rgb(0x15, 0x80, 0x3d);
pub const SETUP: Color32 = Color32::from_rgb(0xea, 0xb3, 0x08);
/// Selected Always stroke only — dark OLED, ≥3:1 on `#16181C`.
pub const ALWAYS_AMBER_DARK: Color32 = Color32::from_rgb(0xe8, 0xa8, 0x38);
/// Selected Always stroke only — light surface, ≥3:1 on elevated white.
pub const ALWAYS_AMBER_LIGHT: Color32 = Color32::from_rgb(0xb8, 0x6e, 0x00);
pub const OFFLINE: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
/// grok.com light `--surface-base` (System when the desktop is light).
pub const LIGHT_BG: Color32 = Color32::from_rgb(0xf4, 0xf4, 0xf5);
pub const LIGHT_SURFACE: Color32 = Color32::from_rgb(0xee, 0xee, 0xf0);
pub const LIGHT_PANEL: Color32 = Color32::from_rgb(0xe4, 0xe4, 0xe7);
pub const LIGHT_ELEVATED: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const LIGHT_FG: Color32 = Color32::from_rgb(0x0a, 0x0a, 0x0a);
pub const LIGHT_MUTED: Color32 = Color32::from_rgb(0x73, 0x73, 0x73);
pub const LIGHT_SUBTLE: Color32 = Color32::from_rgb(0x8a, 0x8a, 0x8a);
/// Hairline one step darker than `LIGHT_PANEL` so card and sheet edges still show on it.
pub const LIGHT_BORDER: Color32 = Color32::from_rgb(0xd4, 0xd4, 0xd8);
pub const LIGHT_BORDER_STRONG: Color32 = Color32::from_rgb(0xb4, 0xb4, 0xbb);
pub const LIGHT_NAV_ACTIVE: Color32 = Color32::from_rgb(0xe4, 0xe4, 0xe7);
pub const LIGHT_BUBBLE_USER: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xea);
pub const LIGHT_HOVER: Color32 = Color32::from_rgb(0xda, 0xda, 0xdd);
/// Text selection and picked list rows. A cool slate so a selected span reads on
/// the composer fill (hover alone is ~1.07:1 there).
pub const SELECTION: Color32 = Color32::from_rgb(0x2a, 0x3a, 0x4d);
pub const LIGHT_SELECTION: Color32 = Color32::from_rgb(0xcf, 0xdf, 0xf2);

static USE_LIGHT: AtomicBool = AtomicBool::new(false);
static LAST_PAINT: AtomicU8 = AtomicU8::new(255);

struct OsDarkCache {
    at: Instant,
    dark: bool,
    inflight: bool,
}

static OS_DARK: Mutex<Option<OsDarkCache>> = Mutex::new(None);

fn tok(dark: Color32, light: Color32) -> Color32 {
    if USE_LIGHT.load(Ordering::Relaxed) {
        light
    } else {
        dark
    }
}

pub fn bg() -> Color32 {
    tok(BG, LIGHT_BG)
}
pub fn surface() -> Color32 {
    tok(SURFACE, LIGHT_SURFACE)
}
pub fn panel() -> Color32 {
    tok(PANEL, LIGHT_PANEL)
}
pub fn elevated() -> Color32 {
    tok(ELEVATED, LIGHT_ELEVATED)
}
pub fn hover() -> Color32 {
    tok(HOVER, LIGHT_HOVER)
}
pub fn fg() -> Color32 {
    tok(FG, LIGHT_FG)
}
pub fn muted() -> Color32 {
    tok(MUTED, LIGHT_MUTED)
}
pub fn subtle() -> Color32 {
    tok(SUBTLE, LIGHT_SUBTLE)
}
pub fn border() -> Color32 {
    tok(BORDER, LIGHT_BORDER)
}
pub fn border_strong() -> Color32 {
    tok(BORDER_STRONG, LIGHT_BORDER_STRONG)
}
pub fn nav_active() -> Color32 {
    tok(NAV_ACTIVE, LIGHT_NAV_ACTIVE)
}
pub fn bubble_user() -> Color32 {
    tok(BUBBLE_USER, LIGHT_BUBBLE_USER)
}
pub fn bubble_assistant() -> Color32 {
    bubble_user()
}
pub fn selection() -> Color32 {
    tok(SELECTION, LIGHT_SELECTION)
}
pub fn surface_hover() -> Color32 {
    tok(SURFACE_HOVER, LIGHT_HOVER)
}
pub fn link() -> Color32 {
    tok(LINK, LINK)
}
pub fn send_on() -> Color32 {
    tok(SEND_ON, LIGHT_FG)
}
pub fn send_on_ink() -> Color32 {
    tok(SEND_ON_INK, LIGHT_ELEVATED)
}
pub fn live() -> Color32 {
    tok(LIVE, LIGHT_LIVE)
}
pub fn setup() -> Color32 {
    SETUP
}
/// Selected Always ring. Idle Always never uses this.
pub fn always_amber() -> Color32 {
    tok(ALWAYS_AMBER_DARK, ALWAYS_AMBER_LIGHT)
}
pub fn offline() -> Color32 {
    OFFLINE
}

/// Code keyword. Muted blue-violet so a reply does not read as a rainbow.
pub fn code_keyword() -> Color32 {
    tok(
        Color32::from_rgb(0xc6, 0x92, 0xf0),
        Color32::from_rgb(0x7c, 0x3a, 0xb8),
    )
}
pub fn code_string() -> Color32 {
    tok(
        Color32::from_rgb(0x9e, 0xce, 0x86),
        Color32::from_rgb(0x2f, 0x7d, 0x32),
    )
}
pub fn code_number() -> Color32 {
    tok(
        Color32::from_rgb(0xe8, 0xb0, 0x74),
        Color32::from_rgb(0xa6, 0x55, 0x00),
    )
}
pub fn code_comment() -> Color32 {
    subtle()
}
/// Code block well. Sits one step below the bubble fill.
pub fn code_well() -> Color32 {
    tok(BG, LIGHT_ELEVATED)
}

/// Quiet sheet elevation. Light is a soft drop; dark is a faint lift on OLED `#000`.
pub fn sheet_shadow() -> egui::Shadow {
    if USE_LIGHT.load(Ordering::Relaxed) {
        egui::Shadow {
            offset: egui::vec2(0.0, 2.0),
            blur: 8.0,
            spread: 0.0,
            color: Color32::from_black_alpha(28),
        }
    } else {
        egui::Shadow {
            offset: egui::vec2(0.0, 2.0),
            blur: 10.0,
            spread: 0.0,
            color: Color32::from_white_alpha(14),
        }
    }
}

/// Agent / catalog card title — Inter SemiBold 16.
pub fn card_title_font() -> FontId {
    title_font(FONT_SECTION)
}

/// Composer pill hairline. Focus lifts to the strong border, never warning amber.
pub fn composer_chrome_stroke(focused: bool) -> Stroke {
    Stroke::new(1.0_f32, if focused { border_strong() } else { border() })
}

#[cfg(test)]
pub static PAINT_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Tests that flip `USE_LIGHT` must hold this across set + assert so parallel
/// `apply` / `set_paint_dark` cannot restore dark between the two.
#[cfg(test)]
pub fn hold_paint_test() -> std::sync::MutexGuard<'static, ()> {
    PAINT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
pub fn set_paint_dark(dark: bool) {
    USE_LIGHT.store(!dark, Ordering::Relaxed);
    LAST_PAINT.store(255, Ordering::SeqCst);
}

pub fn desktop_prefers_dark() -> bool {
    if let Ok(v) = std::env::var("GROKHUB_COLOR_SCHEME") {
        return os_prefers_dark(&v, "", "");
    }
    if let Ok(g) = OS_DARK.lock() {
        if let Some(c) = g.as_ref() {
            let hit = c.dark;
            let fresh = c.at.elapsed().as_secs() < 30;
            let busy = c.inflight;
            drop(g);
            if !fresh && !busy {
                kick_os_dark();
            }
            return hit;
        }
    }
    let dark = probe_os_dark();
    if let Ok(mut g) = OS_DARK.lock() {
        *g = Some(OsDarkCache {
            at: Instant::now(),
            dark,
            inflight: false,
        });
    }
    dark
}

fn kick_os_dark() {
    if let Ok(mut g) = OS_DARK.lock() {
        if let Some(c) = g.as_mut() {
            if c.inflight {
                return;
            }
            c.inflight = true;
        }
    }
    std::thread::spawn(|| {
        let dark = probe_os_dark();
        if let Ok(mut g) = OS_DARK.lock() {
            *g = Some(OsDarkCache {
                at: Instant::now(),
                dark,
                inflight: false,
            });
        }
    });
}

fn probe_os_dark() -> bool {
    #[cfg(windows)]
    {
        crate::win_native::apps_use_light_theme()
            .map(|light| !light)
            .unwrap_or(true)
    }
    #[cfg(not(windows))]
    {
        let scheme = cmd_stdout(
            "gsettings",
            &["get", "org.gnome.desktop.interface", "color-scheme"],
        );
        let gtk = std::env::var("GTK_THEME").unwrap_or_default();
        let xfce = cmd_stdout("xfconf-query", &["-c", "xsettings", "-p", "/Net/ThemeName"]);
        os_prefers_dark(&scheme, &gtk, &xfce)
    }
}

fn cmd_stdout(bin: &str, args: &[&str]) -> String {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    crate::desktop::run_limited(cmd, Duration::from_millis(400))
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
        .trim()
        .trim_matches('\'')
        .to_string()
}

pub const SIDEBAR_W: f32 = 260.0;
/// Centered composer / empty-home column (official Grok ~720–800). Chat text uses the pane.
pub const CHAT_COL_W: f32 = 768.0;
/// Soft chat-bubble radius. User and assistant share it. Thought has none.
pub const USER_BUBBLE_RADIUS: f32 = 20.0;
/// Quiet chrome (rail rows, sheets, menus). Composer + user bubble stay large.
pub const CHROME_RADIUS: f32 = 6.0;
/// Catalog / agent card — Fluent card radius, not a chat pill.
pub const CARD_RADIUS: f32 = 12.0;
/// Catalog icon well that holds a 20px Fluent glyph.
pub const TILE_ICON: f32 = 28.0;
/// Rail / composer chrome glyph.
pub const ICON_CHROME: f32 = 16.0;
/// Primary action glyph (Send, card well).
pub const ICON_ACTION: f32 = 20.0;
/// Hairline painted icons (denser than a 1.5 sketch).
pub const ICON_STROKE: f32 = 1.25;
pub const TITLEBAR_H: f32 = 40.0;
/// `[data-testid=chat-input]` `min-h-[60px]`
pub const QUERY_MIN_H: f32 = 60.0;
/// `.query-bar` computed `border-radius: 160px`
pub const QUERY_RADIUS: f32 = 160.0;

/// Cap the composer / empty-home column. Transcript layout uses the full pane.
pub fn chat_col_w(avail: f32) -> f32 {
    if !avail.is_finite() || avail <= 0.0 {
        CHAT_COL_W
    } else {
        avail.min(CHAT_COL_W)
    }
}
/// Attach / Submit `h-10 w-10 rounded-full`
pub const HIT: f32 = 40.0;
/// Rail / chrome row (`h-10`, `--font-size-chrome`)
pub const NAV_ROW_H: f32 = 40.0;
pub const FONT_UI: f32 = 15.0;
pub const FONT_CHROME: f32 = 14.0;
pub const FONT_META: f32 = 13.0;
/// Card / section title — Inter SemiBold.
pub const FONT_SECTION: f32 = 16.0;
/// Card body / quieter session chrome.
pub const FONT_BODY: f32 = 13.0;
/// Tips and meta.
pub const FONT_TIP: f32 = 12.0;
/// Settings / pane titles — larger than Body.
pub const FONT_HEADING: f32 = 22.0;
/// grok.com/imagine `h1.text-[22px].leading-7`
pub const IMAGINE_TITLE: f32 = 22.0;
/// gap from h1 to `.query-bar` on /imagine
pub const IMAGINE_GAP: f32 = 32.0;
/// measured Imagine query-bar width
pub const IMAGINE_BAR_W: f32 = 768.0;
/// Imagine `.query-bar` `border-radius: 20px` — not the chat pill
pub const IMAGINE_BAR_RADIUS: f32 = 20.0;
/// Imagine Upload / Submit `size-9`
pub const IMAGINE_HIT: f32 = 36.0;
/// grok.com/imagine masonry short tile (~230)
pub const IMAGINE_TILE_SHORT: f32 = 230.0;
/// grok.com/imagine masonry tall tile (~345)
pub const IMAGINE_TILE_TALL: f32 = 345.0;
/// Cover GIF cycle — two stills crossfade like grok.com inspiration MP4s.
pub const IMAGINE_FRAME_MS: u64 = 1600;

/// Live grok.com primary rail. Settings is an avatar menu, not a row.
pub const GROK_NAV: &[(&str, &str)] = &[
    ("chat", "Chat"),
    ("imagine", "Imagine"),
    ("automations", "Automations"),
    ("skills", "Skills and Connectors"),
    ("workboard", "Workboards"),
    ("ideas", "Ideas"),
];

/// Avatar-menu destinations besides Help / Sign in / Sign out.
/// Leftover panes stay reachable from slash, palette, and the sidebar.
pub const CABIN_MENU: &[(&str, &str)] = &[("settings", "Settings")];

#[allow(dead_code)]
pub fn stage_subtitle(id: &str) -> &'static str {
    match id {
        "history" => "Past chats",
        "chat" => "Recent chat",
        "imagine" => "Images",
        "workboard" => "Tasks and plans",
        "ideas" => "Worth doing",
        "skills" => "Personal skills and connectors",
        "automations" => "Grok Build /loop scheduler",
        "command" => "Overview",
        "queue" => "Background jobs",
        "settings" => "Preferences",
        "devices" => "Paired computers",
        "memory" => "SOUL / USER / MEMORY",
        "eyes" => "Computer-use frames",
        "connectors" => "MCP / skills / plugins",
        _ => "GrokHub",
    }
}

fn install_inter(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf")),
    );
    fonts.font_data.insert(
        "inter-medium".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-Medium.ttf")),
    );
    fonts.font_data.insert(
        "inter-bold".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
    );
    // The latin statics are subset (no →, ✓, ≥, Cyrillic). The full face backs them.
    fonts.font_data.insert(
        "inter-full".into(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-Full-Regular.ttf")),
    );
    if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
        fam.insert(0, "inter-full".into());
        fam.insert(0, "inter-medium".into());
        fam.insert(0, "inter".into());
    }
    fonts.families.insert(
        FontFamily::Name("inter-bold".into()),
        vec![
            "inter-bold".into(),
            "inter-medium".into(),
            "inter".into(),
            "inter-full".into(),
        ],
    );
    let mono = std::fs::read("/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf")
        .or_else(|_| std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"))
        .or_else(|_| std::fs::read("/usr/share/fonts/truetype/macos/JetBrainsMono-Regular.ttf"));
    if let Ok(mono) = mono {
        fonts
            .font_data
            .insert("mono".into(), FontData::from_owned(mono));
        if let Some(fam) = fonts.families.get_mut(&FontFamily::Monospace) {
            fam.insert(0, "mono".into());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn install_fonts(ctx: &egui::Context) {
    static FONTS: AtomicBool = AtomicBool::new(false);
    if !FONTS.swap(true, Ordering::SeqCst) {
        install_inter(ctx);
    }
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    install_fonts(ctx);
    #[cfg(test)]
    let _paint = hold_paint_test();
    USE_LIGHT.store(!dark, Ordering::Relaxed);
    let flag = if dark { 1 } else { 0 };
    if LAST_PAINT.swap(flag, Ordering::SeqCst) == flag {
        return;
    }
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let hover = hover();
    visuals.dark_mode = dark;
    visuals.override_text_color = Some(fg());
    visuals.panel_fill = bg();
    visuals.window_fill = panel();
    visuals.extreme_bg_color = bg();
    visuals.faint_bg_color = surface();
    visuals.code_bg_color = surface();
    visuals.hyperlink_color = link();
    visuals.warn_fg_color = setup();
    visuals.error_fg_color = offline();
    visuals.selection.bg_fill = selection();
    visuals.selection.stroke = Stroke::new(1.0_f32, border_strong());
    visuals.widgets.noninteractive.bg_fill = surface();
    visuals.widgets.noninteractive.weak_bg_fill = bg();
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, muted());
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border());
    visuals.widgets.inactive.bg_fill = surface();
    visuals.widgets.inactive.weak_bg_fill = surface();
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, fg());
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, border());
    let press = if dark {
        SURFACE_HOVER
    } else {
        Color32::from_rgb(0xc8, 0xc8, 0xcc)
    };
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, fg());
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, border());
    visuals.widgets.hovered.expansion = HOVER_EXPANSION;
    visuals.widgets.active.bg_fill = press;
    visuals.widgets.active.weak_bg_fill = press;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, fg());
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, border());
    visuals.widgets.active.expansion = PRESS_EXPANSION;
    visuals.widgets.open.bg_fill = panel();
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, fg());
    visuals.window_stroke = Stroke::new(1.0_f32, border());
    visuals.window_rounding = CHROME_RADIUS.into();
    visuals.menu_rounding = CHROME_RADIUS.into();
    visuals.window_shadow = sheet_shadow();
    visuals.popup_shadow = sheet_shadow();
    visuals.widgets.noninteractive.rounding = CHROME_RADIUS.into();
    visuals.widgets.inactive.rounding = CHROME_RADIUS.into();
    visuals.widgets.hovered.rounding = CHROME_RADIUS.into();
    visuals.widgets.active.rounding = CHROME_RADIUS.into();
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(FONT_META, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(FONT_UI, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(FONT_CHROME, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(FONT_HEADING, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.0, FontFamily::Monospace),
    );
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.handle_min_length = 24.0;
    style.visuals = ctx.style().visuals.clone();
    ctx.set_style(style);
}

/// TextEdit placeholder. egui bakes `override_text_color` into a plain hint galley,
/// so an uncolored hint paints in full `fg()` and reads as typed text.
pub fn hint(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(muted())
}

pub fn pointing(resp: egui::Response) -> egui::Response {
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn blend_color(from: Color32, to: Color32, t: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        mix_channel(from.r(), to.r(), t),
        mix_channel(from.g(), to.g(), t),
        mix_channel(from.b(), to.b(), t),
        mix_channel(from.a(), to.a(), t),
    )
}

/// Animated on/off for toggles and segment selection. Same ease as button hover.
pub fn animate_selection(ui: &egui::Ui, id: egui::Id, on: bool) -> f32 {
    ui.ctx().animate_bool_with_time_and_easing(
        id,
        on,
        SELECT_SECS,
        egui::emath::easing::quadratic_out,
    )
}

fn button_channel(ui: &egui::Ui, id: egui::Id, on: bool, secs: f32) -> f32 {
    ui.ctx().animate_bool_with_time_and_easing(
        id,
        on,
        secs,
        egui::emath::easing::quadratic_out,
    )
}

/// Quiet control fill. Same corner on every labeled button.
pub fn paint_quiet_chrome(painter: &egui::Painter, rect: egui::Rect, fill: egui::Color32) {
    painter.rect_filled(rect, 8.0, fill);
}

#[derive(Clone, Debug, Default)]
struct GlidePick {
    hover: Option<egui::Rect>,
    selected: Option<egui::Rect>,
    rows: Vec<egui::Rect>,
}

fn glide_id(group: &str) -> egui::Id {
    egui::Id::new(("cabin-glide", group))
}

/// Paint last frame's highlight before the row's labels, so it sits behind them.
pub fn glide_paint(ui: &egui::Ui, group: &str) {
    let id = glide_id(group);
    ui.data_mut(|d| d.insert_temp(id.with("pick"), GlidePick::default()));
    let Some(rect) = ui.data(|d| d.get_temp::<egui::Rect>(id.with("rect"))) else {
        return;
    };
    let alpha = ui.data(|d| d.get_temp::<f32>(id.with("alpha"))).unwrap_or(0.0);
    if alpha < 0.02 || rect.width() < 1.0 {
        return;
    }
    let base = nav_active();
    let fill = egui::Color32::from_rgba_unmultiplied(
        base.r(),
        base.g(),
        base.b(),
        (base.a() as f32 * alpha).round() as u8,
    );
    ui.painter().rect_filled(rect, 8.0, fill);
}

/// Remember a row item. Hover wins over the resting selection.
pub fn glide_candidate(ui: &egui::Ui, group: &str, rect: egui::Rect, hovered: bool, selected: bool) {
    let id = glide_id(group).with("pick");
    let visible = rect.width() >= 1.0 && rect.height() >= 1.0 && ui.clip_rect().intersects(rect);
    // In place: every rail row calls this, and a copy out and back per row adds up.
    ui.data_mut(|d| {
        let pick = d.get_temp_mut_or_default::<GlidePick>(id);
        if visible {
            pick.rows.push(rect);
        }
        if hovered {
            pick.hover = Some(rect);
        }
        if selected && pick.selected.is_none() {
            pick.selected = Some(rect);
        }
    });
}

fn rect_gap(rect: egui::Rect, pointer: egui::Pos2) -> f32 {
    let dx = (rect.min.x - pointer.x).max(pointer.x - rect.max.x).max(0.0);
    let dy = (rect.min.y - pointer.y).max(pointer.y - rect.max.y).max(0.0);
    dx.hypot(dy)
}

fn same_glide_row(a: egui::Rect, b: egui::Rect) -> bool {
    (a.min.x - b.min.x).abs() < 0.5
        && (a.min.y - b.min.y).abs() < 0.5
        && (a.width() - b.width()).abs() < 0.5
        && (a.height() - b.height()).abs() < 0.5
}

/// Closest real row to the pointer. A near tie stays on `stick` so the bar does not flicker.
fn nearest_glide_row(
    rows: &[egui::Rect],
    pointer: egui::Pos2,
    stick: Option<egui::Rect>,
) -> Option<egui::Rect> {
    let mut best: Option<(egui::Rect, f32)> = None;
    let mut stick_dist = None;
    for rect in rows {
        if rect.width() < 1.0 || rect.height() < 1.0 {
            continue;
        }
        let dist = rect_gap(*rect, pointer);
        if stick.is_some_and(|kept| same_glide_row(kept, *rect)) {
            stick_dist = Some(dist);
        }
        match best {
            Some((_, nearer)) if dist >= nearer => {}
            _ => best = Some((*rect, dist)),
        }
    }
    let (rect, dist) = best?;
    if let (Some(kept), Some(kept_dist)) = (stick, stick_dist) {
        if kept_dist <= dist + 8.0 {
            return Some(kept);
        }
    }
    Some(rect)
}

/// Move the highlight toward this frame's target. The paint shows it next frame.
pub fn glide_aim(ui: &egui::Ui, group: &str) {
    let id = glide_id(group);
    let pick = ui
        .data(|d| d.get_temp::<GlidePick>(id.with("pick")))
        .unwrap_or_default();
    let stick = ui.data(|d| d.get_temp::<egui::Rect>(id.with("last")));
    let target = if let Some(hover) = pick.hover {
        Some(hover)
    } else if pick.rows.len() > 1 {
        // A gap in the rail (search field, section header, plus) is not a row.
        // Stay on the option nearest the pointer instead of the selected page.
        match ui.input(|i| i.pointer.hover_pos()) {
            Some(pos) if ui.clip_rect().contains(pos) => {
                nearest_glide_row(&pick.rows, pos, stick).or(pick.selected)
            }
            _ => pick.selected,
        }
    } else {
        pick.hover.or(pick.selected)
    };
    let show = ui.ctx().animate_bool_with_time_and_easing(
        id.with("show"),
        target.is_some(),
        0.12,
        egui::emath::easing::quadratic_out,
    );
    let last = ui
        .data(|d| d.get_temp::<egui::Rect>(id.with("last")))
        .unwrap_or(egui::Rect::NOTHING);
    let goal = target.unwrap_or(last);
    if goal.width() < 1.0 {
        return;
    }
    let secs = 0.16;
    let x = ui.ctx().animate_value_with_time(id.with("x"), goal.min.x, secs);
    let y = ui.ctx().animate_value_with_time(id.with("y"), goal.min.y, secs);
    let w = ui.ctx().animate_value_with_time(id.with("w"), goal.width(), secs);
    let h = ui.ctx().animate_value_with_time(id.with("h"), goal.height(), secs);
    let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
    ui.data_mut(|d| {
        d.insert_temp(id.with("rect"), rect);
        d.insert_temp(id.with("alpha"), show);
        if target.is_some() {
            d.insert_temp(id.with("last"), goal);
        }
    });
}

/// Soft shadow under a rising button. Resting controls stay flat.
pub fn paint_button_shadow(painter: &egui::Painter, rect: egui::Rect, hover_t: f32, press_t: f32) {
    let t = (hover_t * (1.0 - 0.45 * press_t)).clamp(0.0, 1.0);
    if t < 0.04 {
        return;
    }
    let alpha = (56.0 * t) as u8;
    let shadow = rect.translate(egui::vec2(0.0, 2.0 + 2.0 * t));
    painter.rect_filled(
        shadow,
        rect.height().min(20.0) * 0.5,
        egui::Color32::from_black_alpha(alpha),
    );
}

/// Painted label button with hover grow / press shrink (Plasma-style pointer feedback).
pub fn felt_label_button(
    ui: &mut egui::Ui,
    label: &str,
    base_fill: Color32,
    text_color: Color32,
    rounding: f32,
    min_size: egui::Vec2,
    stroke: Option<Stroke>,
    strong: bool,
) -> egui::Response {
    let font = if strong {
        title_font(FONT_CHROME)
    } else {
        FontId::proportional(FONT_CHROME)
    };
    let galley = ui.fonts(|f| f.layout_no_wrap(label.to_owned(), font, text_color));
    let pad = ui.style().spacing.button_padding;
    let size = egui::vec2(
        (galley.size().x + pad.x * 2.0).max(min_size.x),
        (galley.size().y + pad.y * 2.0).max(min_size.y),
    );
    let (_rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let (resp, rect, fill) = feel_button(ui, resp, base_fill);
    ui.painter().rect_filled(rect, rounding, fill);
    if let Some(s) = stroke {
        ui.painter().rect_stroke(rect, rounding, s);
    }
    // Centre vertically: a layout can hand the pill more height than label + pad.
    let text_pos = egui::pos2(rect.min.x + pad.x, rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(text_pos, galley, text_color);
    pointing(resp)
}

/// Compact square hit for sidebar `+` and similar chrome.
pub fn felt_icon_hit(
    ui: &mut egui::Ui,
    label: &str,
    size: f32,
    text_color: Color32,
    font_size: f32,
) -> egui::Response {
    let (_rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let (resp, rect, wash) = feel_button(ui, resp, Color32::TRANSPARENT);
    if wash.a() > 0 {
        ui.painter().rect_filled(rect, 6.0, wash);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(font_size),
        if resp.hovered() { fg() } else { text_color },
    );
    pointing(resp)
}

pub fn lift_fill(fill: Color32, mix: f32) -> Color32 {
    let light = USE_LIGHT.load(Ordering::Relaxed);
    if fill.a() == 0 {
        if mix <= 0.0 {
            return Color32::TRANSPARENT;
        }
        if light {
            return Color32::from_black_alpha(hover_alpha(0, mix));
        }
        let t = (mix / HOVER_WASH).clamp(0.0, 1.0);
        return Color32::from_rgba_unmultiplied(
            HOVER.r(),
            HOVER.g(),
            HOVER.b(),
            mix_channel(0, 255, t),
        );
    }
    if light {
        let (r, g, b) = lift_rgb(fill.r(), fill.g(), fill.b(), mix, false);
        return Color32::from_rgba_unmultiplied(r, g, b, fill.a());
    }
    Color32::from_rgba_unmultiplied(
        mix_channel(fill.r(), HOVER.r(), mix),
        mix_channel(fill.g(), HOVER.g(), mix),
        mix_channel(fill.b(), HOVER.b(), mix),
        fill.a(),
    )
}

fn feel_motion(
    ui: &egui::Ui,
    resp: egui::Response,
    fill: Color32,
    rise: bool,
) -> (egui::Response, egui::Rect, Color32, f32, f32) {
    let hovered = resp.hovered();
    let pressed = resp.is_pointer_button_down_on();
    let focused = resp.has_focus();
    let id = resp.id;
    let base = resp.rect;
    let resp = pointing(resp);
    let hover_t = button_channel(ui, id.with("feel-h"), hovered, HOVER_SECS);
    let press_t = button_channel(ui, id.with("feel-p"), pressed, PRESS_SECS);
    let focus_t = button_channel(ui, id.with("feel-f"), focused, SELECT_SECS);
    let scale = feel_scale(hover_t, press_t) + 0.01 * focus_t;
    let mix = hover_mix(hover_t, press_t) + grokhub_core::FOCUS_WASH * focus_t;
    let (x, y, w, h) = felt_rect(base.min.x, base.min.y, base.width(), base.height(), scale);
    let lift = if rise { feel_lift(hover_t, press_t) } else { 0.0 };
    let rect = egui::Rect::from_min_size(egui::pos2(x, y - lift), egui::vec2(w, h));
    (resp, rect, lift_fill(fill, mix), hover_t, press_t)
}

pub fn feel_response(
    ui: &egui::Ui,
    resp: egui::Response,
    fill: Color32,
) -> (egui::Response, egui::Rect, Color32) {
    let (resp, rect, fill, _, _) = feel_motion(ui, resp, fill, false);
    (resp, rect, fill)
}

/// Pill, tab, rail, and chrome controls. Same scale as cards, plus the deck's rise and shadow.
pub fn feel_button(
    ui: &egui::Ui,
    resp: egui::Response,
    fill: Color32,
) -> (egui::Response, egui::Rect, Color32) {
    let (resp, rect, fill, _, _) = feel_motion(ui, resp, fill, true);
    (resp, rect, fill)
}

/// Same clock and wash as [`feel_response`], clamped to the widget slot.
/// Cards and the Imagine wall use this. Buttons use [`feel_button`].
pub fn feel_response_in_slot(
    ui: &egui::Ui,
    resp: egui::Response,
    fill: Color32,
) -> (egui::Response, egui::Rect, Color32) {
    let slot = resp.rect;
    let (resp, felt, color) = feel_response(ui, resp, fill);
    let (x, y, w, h) = clamp_rect_to_slot(
        slot.min.x,
        slot.min.y,
        slot.width(),
        slot.height(),
        felt.min.x,
        felt.min.y,
        felt.width(),
        felt.height(),
    );
    let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
    (resp, rect, color)
}

/// Hover plate alpha is the 0.12s amount. Partial plates are premultiplied,
/// so the target is the theme hover color. Full hover is the card background.
pub fn veil_over(base: Color32, veil: Color32) -> Color32 {
    if veil.a() == 0 {
        return base;
    }
    let t = veil.a() as f32 / 255.0;
    let toward = hover();
    Color32::from_rgba_unmultiplied(
        mix_channel(base.r(), toward.r(), t),
        mix_channel(base.g(), toward.g(), t),
        mix_channel(base.b(), toward.b(), t),
        base.a(),
    )
}

fn mark_image() -> &'static ColorImage {
    static IMG: OnceLock<ColorImage> = OnceLock::new();
    IMG.get_or_init(|| {
        let bytes = include_bytes!("../assets/grokhub-32.png");
        let img = image::load_from_memory(bytes).expect("grokhub mark");
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
    })
}

pub fn mark(ctx: &egui::Context) -> TextureHandle {
    let id = egui::Id::new("grokhub-mark");
    if let Some(tex) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return tex;
    }
    let tex = ctx.load_texture("grokhub-mark", mark_image().clone(), TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_edges_and_selection_stay_visible() {
        // A border that equals the panel fill draws nothing on it.
        assert_ne!(LIGHT_BORDER, LIGHT_PANEL);
        assert_ne!(LIGHT_BORDER, LIGHT_BG);
        // Hover is ~1.07:1 on the composer; selection must not reuse it.
        assert_ne!(SELECTION, HOVER);
        assert_ne!(LIGHT_SELECTION, LIGHT_HOVER);
        let _paint = hold_paint_test();
        set_paint_dark(true);
        assert_eq!(selection(), SELECTION);
        set_paint_dark(false);
        assert_eq!(selection(), LIGHT_SELECTION);
        set_paint_dark(true);
    }

    #[test]
    fn live_green_is_readable_on_light() {
        assert_ne!(LIGHT_LIVE, LIVE);
        let _paint = hold_paint_test();
        set_paint_dark(false);
        assert_eq!(live(), LIGHT_LIVE);
        set_paint_dark(true);
        assert_eq!(live(), LIVE);
    }

    #[test]
    fn inter_full_backs_the_subset_statics() {
        let src = include_str!("theme.rs");
        let fonts = src
            .split("fn install_inter(")
            .nth(1)
            .and_then(|s| s.split("pub fn install_fonts(").next())
            .expect("install_inter");
        assert!(
            fonts.contains("Inter-Full-Regular.ttf") && fonts.matches("\"inter-full\"").count() >= 3,
            "→, ✓ and ≥ are not in the latin subset; the full face must back both families: {fonts}"
        );
    }

    #[test]
    fn glide_gap_stays_on_the_near_row() {
        let selected = egui::Rect::from_min_size(egui::pos2(8.0, 4.0), egui::vec2(200.0, 28.0));
        let above = egui::Rect::from_min_size(egui::pos2(8.0, 40.0), egui::vec2(200.0, 28.0));
        let below = egui::Rect::from_min_size(egui::pos2(8.0, 120.0), egui::vec2(200.0, 28.0));
        let rows = [selected, above, below];
        let near_below = nearest_glide_row(&rows, egui::pos2(40.0, 108.0), None).unwrap();
        assert_eq!(near_below, below, "a gap just above the next row stays there");
        let near_above = nearest_glide_row(&rows, egui::pos2(40.0, 76.0), None).unwrap();
        assert_eq!(near_above, above, "a gap just under a row stays on that row");
        assert_ne!(near_below, selected);
        let held = nearest_glide_row(&rows, egui::pos2(40.0, 94.0), Some(above)).unwrap();
        assert_eq!(held, above, "the midpoint keeps the row the pointer already reached");
    }

    #[test]
    fn blend_color_endpoints() {
        assert_eq!(
            blend_color(
                Color32::from_rgb(0, 0, 0),
                Color32::from_rgb(100, 100, 100),
                0.0
            ),
            Color32::from_rgb(0, 0, 0)
        );
        assert_eq!(
            blend_color(
                Color32::from_rgb(0, 0, 0),
                Color32::from_rgb(100, 100, 100),
                1.0
            ),
            Color32::from_rgb(100, 100, 100)
        );
    }

    #[test]
    #[allow(clippy::assertions_on_constants)] // pins design constants
    fn grok_com_chrome_tokens() {
        let _paint = hold_paint_test();
        assert_eq!(BG, Color32::from_rgb(0, 0, 0));
        assert_eq!(SURFACE, Color32::from_rgb(0x16, 0x18, 0x1c));
        assert_eq!(PANEL, Color32::from_rgb(0x1a, 0x1a, 0x1a));
        assert_eq!(SURFACE_HOVER, Color32::from_rgb(0x1c, 0x1f, 0x23));
        assert_eq!(HOVER, Color32::from_rgb(0x1c, 0x1f, 0x23));
        assert_eq!(FG, Color32::from_rgb(0xe7, 0xe9, 0xea));
        assert_eq!(MUTED, Color32::from_rgb(0x71, 0x76, 0x7b));
        assert_eq!(BORDER, Color32::from_rgb(0x2f, 0x33, 0x36));
        assert_eq!(LINK, Color32::from_rgb(0x1d, 0x9b, 0xf0));
        assert_eq!(SEND_ON, Color32::WHITE);
        assert_eq!(GREET_HERO, 28.0);
        assert!(GREET_HERO > FONT_HEADING);
        assert!(GREET_HERO <= 28.0);
        assert_eq!(CHAT_COL_W, 768.0);
        assert!(CHAT_COL_W >= 720.0 && CHAT_COL_W <= 800.0);
        assert_eq!(chat_col_w(1800.0), CHAT_COL_W);
        assert_eq!(chat_col_w(600.0), 600.0);
        assert_eq!(USER_BUBBLE_RADIUS, 20.0);
        assert!(USER_BUBBLE_RADIUS < QUERY_RADIUS);
        assert_eq!(CHROME_RADIUS, 6.0);
        assert_eq!(CARD_RADIUS, 12.0);
        assert_eq!(TILE_ICON, 28.0);
        assert_eq!(ICON_CHROME, 16.0);
        assert_eq!(ICON_ACTION, 20.0);
        assert_eq!(ICON_STROKE, 1.25);
        assert!(ICON_CHROME < ICON_ACTION);
        assert!(ICON_ACTION < TILE_ICON);
        assert_eq!(FONT_SECTION, 16.0);
        assert_eq!(FONT_BODY, 13.0);
        assert_eq!(FONT_TIP, 12.0);
        assert!(FONT_TIP < FONT_BODY);
        assert!(FONT_SECTION > FONT_CHROME);
        assert_eq!(ALWAYS_AMBER_DARK, Color32::from_rgb(0xe8, 0xa8, 0x38));
        assert_eq!(ALWAYS_AMBER_LIGHT, Color32::from_rgb(0xb8, 0x6e, 0x00));
        assert_ne!(ALWAYS_AMBER_DARK, SETUP);
        set_paint_dark(true);
        assert_eq!(always_amber(), ALWAYS_AMBER_DARK);
        set_paint_dark(false);
        assert_eq!(always_amber(), ALWAYS_AMBER_LIGHT);
        set_paint_dark(true);
        assert_eq!(composer_chrome_stroke(false).color, border());
        assert_eq!(composer_chrome_stroke(true).color, border_strong());
        assert_ne!(composer_chrome_stroke(true).color, always_amber());
        set_paint_dark(true);
        let dark_sheet = sheet_shadow();
        assert!(dark_sheet.blur > 0.0);
        assert_ne!(dark_sheet.color, always_amber());
        set_paint_dark(false);
        let light_sheet = sheet_shadow();
        assert!(light_sheet.blur > 0.0);
        assert_ne!(light_sheet.color, ALWAYS_AMBER_LIGHT);
        set_paint_dark(true);
        assert!(CHROME_RADIUS < CARD_RADIUS);
        assert!(CARD_RADIUS < USER_BUBBLE_RADIUS);
        assert!(CHROME_RADIUS < USER_BUBBLE_RADIUS);
        assert_eq!(QUERY_MIN_H, 60.0);
        assert_eq!(QUERY_RADIUS, 160.0);
        assert_eq!(HIT, 40.0);
        assert_eq!(NAV_ROW_H, 40.0);
        assert_eq!(FONT_UI, 15.0);
        assert_eq!(FONT_CHROME, 14.0);
        assert_eq!(FONT_HEADING, 22.0);
        assert_eq!(TITLEBAR_H, 40.0);
        assert!(TITLEBAR_H >= HIT - 4.0, "titlebar must fit chrome hits");
        assert!(include_bytes!("../assets/fonts/Inter-Regular.ttf").len() > 1000);
        assert_eq!(
            &include_bytes!("../assets/fonts/Inter-Regular.ttf")[..4],
            &[0x00, 0x01, 0x00, 0x00]
        );
        assert!(FONT_HEADING > FONT_UI);
        assert_eq!(IMAGINE_TITLE, 22.0);
        assert_eq!(IMAGINE_GAP, 32.0);
        assert_eq!(IMAGINE_BAR_W, 768.0);
        assert_eq!(IMAGINE_BAR_RADIUS, 20.0);
        assert_eq!(IMAGINE_HIT, 36.0);
        assert_eq!(IMAGINE_TILE_SHORT, 230.0);
        assert_eq!(IMAGINE_TILE_TALL, 345.0);
        assert_eq!(IMAGINE_FRAME_MS, 1600);
        assert_ne!(IMAGINE_BAR_RADIUS, QUERY_RADIUS);
        assert_eq!(GROK_NAV[0], ("chat", "Chat"));
        assert_eq!(GROK_NAV[1], ("imagine", "Imagine"));
        assert!(GROK_NAV.iter().all(|(id, _)| *id != "settings"));
        let skills = GROK_NAV
            .iter()
            .position(|(id, _)| *id == "skills")
            .expect("skills rail");
        assert_eq!(
            GROK_NAV.get(skills + 1),
            Some(&("workboard", "Workboards")),
            "Workboards sits immediately under Skills and Connectors"
        );
        assert_eq!(CABIN_MENU, &[("settings", "Settings")]);
        for gone in [
            "history",
            "workboard",
            "memory",
            "devices",
            "queue",
            "command",
            "connectors",
        ] {
            assert!(
                CABIN_MENU.iter().all(|(id, _)| *id != gone),
                "{gone} must not sit in the avatar menu"
            );
        }
        assert_eq!(stage_subtitle("history"), "Past chats");
        assert_eq!(stage_subtitle("chat"), "Recent chat");
        assert_eq!(stage_subtitle("imagine"), "Images");
        assert_eq!(stage_subtitle("connectors"), "MCP / skills / plugins");
        assert_eq!(title_font(40.0).size, 40.0);
        set_paint_dark(true);
        assert_eq!(bg(), BG);
        assert_eq!(surface(), SURFACE);
        assert_eq!(bubble_user(), BUBBLE_USER);
        assert_eq!(bubble_assistant(), BUBBLE_USER);
        assert_eq!(link(), LINK);
        assert_eq!(send_on(), SEND_ON);
        assert_eq!(hover(), HOVER);
        set_paint_dark(false);
        assert_eq!(bg(), LIGHT_BG);
        assert_eq!(fg(), LIGHT_FG);
        assert_eq!(bubble_assistant(), LIGHT_BUBBLE_USER);
        assert_eq!(hover(), LIGHT_HOVER);
        set_paint_dark(true);
        assert_eq!(bg(), BG);
        assert_eq!(hover(), HOVER);
    }

    #[test]
    fn lift_fill_washes_transparent() {
        let _paint = hold_paint_test();
        set_paint_dark(true);
        assert_eq!(lift_fill(Color32::TRANSPARENT, 0.0).a(), 0);
        let wash = lift_fill(Color32::TRANSPARENT, HOVER_WASH);
        assert_eq!(wash, HOVER);
        assert!(wash.r() < 40 && wash.g() < 40 && wash.b() < 40);
        let cream = Color32::from_white_alpha(wash.a());
        assert_ne!(wash, cream, "dark hover must not be a white/cream wash");
        let solid = lift_fill(Color32::from_rgb(0x16, 0x18, 0x1c), 0.10);
        assert!(
            solid.r() < 40,
            "solid dark hover must stay on {HOVER:?}, not lift toward white"
        );
        set_paint_dark(false);
        assert_eq!(hover(), LIGHT_HOVER);
        let light = lift_fill(Color32::from_rgb(244, 244, 245), 0.10);
        assert!(light.r() < 244);
        set_paint_dark(true);
        assert_eq!(hover(), HOVER);
    }

    #[test]
    fn veil_over_tints_the_card_without_clearing_it() {
        let _paint = hold_paint_test();
        set_paint_dark(true);
        let rest = elevated();
        assert_eq!(veil_over(rest, Color32::TRANSPARENT), rest);
        let plate = lift_fill(Color32::TRANSPARENT, HOVER_WASH);
        assert_eq!(plate.a(), 255, "the hover plate itself is opaque");
        let painted = veil_over(rest, plate);
        assert_eq!(painted.a(), rest.a());
        assert_eq!(painted.r(), HOVER.r());
        assert_ne!(painted, Color32::TRANSPARENT);
        let half = lift_fill(Color32::TRANSPARENT, HOVER_WASH * 0.5);
        let mid = veil_over(rest, half);
        assert_eq!(mid.a(), rest.a());
        assert!(mid.r() > rest.r() && mid.r() < HOVER.r());
    }

    #[test]
    fn os_dark_probe_must_time_out() {
        let src = include_str!("theme.rs");
        let cmd = src
            .split("fn cmd_stdout(")
            .nth(1)
            .and_then(|s| s.split("\npub const SIDEBAR_W").next())
            .expect("cmd_stdout");
        assert!(
            cmd.contains("run_limited("),
            "gsettings/xfconf on the UI thread must time out: {cmd}"
        );
        assert!(
            !cmd.contains(".output()"),
            "os dark probe must not block paint: {cmd}"
        );
        let dark = src
            .split("pub fn desktop_prefers_dark(")
            .nth(1)
            .and_then(|s| s.split("fn probe_os_dark(").next())
            .expect("desktop_prefers_dark");
        assert!(
            dark.contains("as_secs()") && dark.contains("30"),
            "os dark must not spawn gsettings on every paint: {dark}"
        );
        assert!(
            dark.contains("thread::spawn") && dark.contains("inflight"),
            "stale gsettings must refresh off the UI thread: {dark}"
        );
    }

    #[test]
    fn mark_caches_decoded_pixels_and_gpu_texture() {
        let src = include_str!("theme.rs");
        let mark = src
            .split("fn mark_image(")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("mark");
        assert!(
            mark.contains("get_or_init") && mark.contains("OnceLock"),
            "decode grokhub-32.png once: {mark}"
        );
        assert!(
            mark.contains("get_temp") && mark.contains("insert_temp"),
            "reuse the GPU texture across paints: {mark}"
        );
        let paint = mark.split("pub fn mark(").nth(1).expect("mark paint");
        assert!(
            !paint.contains("load_from_memory"),
            "paint must not decode the PNG: {paint}"
        );
    }
}
