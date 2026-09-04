//! Cabin chrome measured from live grok.com dark (`scheme-dark`, 2026-08-15).
//! Recreated in egui — no grok.com JS, no webview.

use eframe::egui::{
    self, Color32, ColorImage, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    TextureHandle, TextureOptions,
};
use grokhub_core::{
    feel_scale, felt_rect, hover_alpha, hover_mix, lift_rgb, mix_channel, os_prefers_dark,
    HOVER_EXPANSION, HOVER_SECS, PRESS_EXPANSION, PRESS_SECS, SELECT_SECS,
};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub fn title_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("inter-bold".into()))
}

/// `--surface-base` / body `rgb(5,5,5)`
pub const BG: Color32 = Color32::from_rgb(0x05, 0x05, 0x05);
/// `--surface-l1` `0 0% 8%`
pub const SURFACE: Color32 = Color32::from_rgb(0x14, 0x14, 0x14);
/// `--surface-l2` `0 0% 13%`
pub const PANEL: Color32 = Color32::from_rgb(0x21, 0x21, 0x21);
/// query-bar `oklab(0.193 / 0.75)` over base
pub const ELEVATED: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
/// `--fg-primary` `rgb(252,252,252)`
pub const FG: Color32 = Color32::from_rgb(0xfc, 0xfc, 0xfc);
/// `--fg-secondary` `0 0% 62%`
pub const MUTED: Color32 = Color32::from_rgb(0x9e, 0x9e, 0x9e);
/// `--fg-tertiary` `0 0% 52%`
pub const SUBTLE: Color32 = Color32::from_rgb(0x85, 0x85, 0x85);
/// Empty-home hero greeting (grok.com, not a 56px product wordmark)
pub const GREET_HERO: f32 = 32.0;
/// `--border-l1` ~8% white on base
pub const BORDER: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
/// `--border-l2` ~14% white
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
/// `--sidebar-accent` `240 5% 26%`
pub const NAV_ACTIVE: Color32 = Color32::from_rgb(0x3f, 0x3f, 0x46);
pub const BUBBLE_USER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const LIVE: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);
pub const SETUP: Color32 = Color32::from_rgb(0xea, 0xb3, 0x08);
pub const OFFLINE: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
/// grok.com light `--surface-base` (System when the desktop is light).
pub const LIGHT_BG: Color32 = Color32::from_rgb(0xf4, 0xf4, 0xf5);
pub const LIGHT_SURFACE: Color32 = Color32::from_rgb(0xee, 0xee, 0xf0);
pub const LIGHT_PANEL: Color32 = Color32::from_rgb(0xe4, 0xe4, 0xe7);
pub const LIGHT_ELEVATED: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const LIGHT_FG: Color32 = Color32::from_rgb(0x0a, 0x0a, 0x0a);
pub const LIGHT_MUTED: Color32 = Color32::from_rgb(0x73, 0x73, 0x73);
pub const LIGHT_SUBTLE: Color32 = Color32::from_rgb(0x8a, 0x8a, 0x8a);
pub const LIGHT_BORDER: Color32 = Color32::from_rgb(0xe4, 0xe4, 0xe7);
pub const LIGHT_BORDER_STRONG: Color32 = Color32::from_rgb(0xd4, 0xd4, 0xd8);
pub const LIGHT_NAV_ACTIVE: Color32 = Color32::from_rgb(0xe4, 0xe4, 0xe7);
pub const LIGHT_BUBBLE_USER: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xea);
pub const LIGHT_HOVER: Color32 = Color32::from_rgb(0xda, 0xda, 0xdd);

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
    panel()
}
pub fn live() -> Color32 {
    LIVE
}
pub fn setup() -> Color32 {
    SETUP
}
pub fn offline() -> Color32 {
    OFFLINE
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
    let scheme = cmd_stdout(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "color-scheme"],
    );
    let gtk = std::env::var("GTK_THEME").unwrap_or_default();
    let xfce = cmd_stdout("xfconf-query", &["-c", "xsettings", "-p", "/Net/ThemeName"]);
    os_prefers_dark(&scheme, &gtk, &xfce)
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
pub const TITLEBAR_H: f32 = 36.0;
/// `[data-testid=chat-input]` `min-h-[60px]`
pub const QUERY_MIN_H: f32 = 60.0;
/// `.query-bar` computed `border-radius: 160px`
pub const QUERY_RADIUS: f32 = 160.0;
/// Attach / Submit `h-10 w-10 rounded-full`
pub const HIT: f32 = 40.0;
/// Rail / chrome row (`h-10`, `--font-size-chrome`)
pub const NAV_ROW_H: f32 = 40.0;
pub const FONT_UI: f32 = 15.0;
pub const FONT_CHROME: f32 = 14.0;
pub const FONT_META: f32 = 13.0;
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
];

/// Cabin-only panes. Opened from the avatar settings menu.
pub const CABIN_MENU: &[(&str, &str)] = &[
    ("history", "History"),
    ("settings", "Settings"),
    ("workboard", "Workboard"),
    ("memory", "Memory"),
    ("devices", "Devices"),
    ("queue", "Queue"),
];

#[allow(dead_code)]
pub fn stage_subtitle(id: &str) -> &'static str {
    match id {
        "history" => "Past chats",
        "chat" => "Recent chat",
        "imagine" => "Images",
        "workboard" => "Pinned tasks",
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
    if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
        fam.insert(0, "inter-medium".into());
        fam.insert(0, "inter".into());
    }
    fonts.families.insert(
        FontFamily::Name("inter-bold".into()),
        vec![
            "inter-bold".into(),
            "inter-medium".into(),
            "inter".into(),
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
    let hover = if dark {
        Color32::from_rgb(0x29, 0x29, 0x29)
    } else {
        LIGHT_HOVER
    };
    visuals.dark_mode = dark;
    visuals.override_text_color = Some(fg());
    visuals.panel_fill = surface();
    visuals.window_fill = panel();
    visuals.extreme_bg_color = bg();
    visuals.faint_bg_color = elevated();
    visuals.code_bg_color = elevated();
    visuals.hyperlink_color = fg();
    visuals.warn_fg_color = setup();
    visuals.error_fg_color = offline();
    visuals.selection.bg_fill = elevated();
    visuals.selection.stroke = Stroke::new(1.0_f32, border_strong());
    visuals.widgets.noninteractive.bg_fill = panel();
    visuals.widgets.noninteractive.weak_bg_fill = surface();
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, muted());
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border());
    visuals.widgets.inactive.bg_fill = elevated();
    visuals.widgets.inactive.weak_bg_fill = elevated();
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, fg());
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, border());
    let press = if dark {
        Color32::from_rgb(0x1f, 0x1f, 0x1f)
    } else {
        Color32::from_rgb(0xc8, 0xc8, 0xcc)
    };
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, fg());
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, border_strong());
    visuals.widgets.hovered.expansion = HOVER_EXPANSION;
    visuals.widgets.active.bg_fill = press;
    visuals.widgets.active.weak_bg_fill = press;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, fg());
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, border_strong());
    visuals.widgets.active.expansion = PRESS_EXPANSION;
    visuals.widgets.open.bg_fill = elevated();
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, fg());
    visuals.window_stroke = Stroke::new(1.0_f32, border());
    visuals.window_rounding = 12.0.into();
    visuals.menu_rounding = 12.0.into();
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.popup_shadow = egui::Shadow {
        offset: egui::vec2(0.0, 8.0),
        blur: 16.0,
        spread: 0.0,
        color: Color32::from_black_alpha(80),
    };
    visuals.widgets.noninteractive.rounding = 10.0.into();
    visuals.widgets.inactive.rounding = 10.0.into();
    visuals.widgets.hovered.rounding = 10.0.into();
    visuals.widgets.active.rounding = 10.0.into();
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(TextStyle::Small, FontId::new(FONT_META, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Body, FontId::new(FONT_UI, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Button, FontId::new(FONT_CHROME, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Heading, FontId::new(FONT_HEADING, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace));
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.handle_min_length = 24.0;
    style.visuals = ctx.style().visuals.clone();
    ctx.set_style(style);
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

/// Animated on/off for toggles and segment selection (~120ms on CachyOS).
pub fn animate_selection(ui: &egui::Ui, id: egui::Id, on: bool) -> f32 {
    ui.ctx().animate_bool_with_time(id, on, SELECT_SECS)
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
    let (resp, rect, fill) = feel_response(ui, resp, base_fill);
    ui.painter().rect_filled(rect, rounding, fill);
    if let Some(s) = stroke {
        ui.painter().rect_stroke(rect, rounding, s);
    }
    ui.painter()
        .galley(rect.min + pad, galley, text_color);
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
    let (resp, rect, wash) = feel_response(ui, resp, Color32::TRANSPARENT);
    if wash.a() > 0 {
        ui.painter()
            .rect_filled(rect, 6.0, wash);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(font_size),
        if resp.hovered() {
            fg()
        } else {
            text_color
        },
    );
    pointing(resp)
}

pub fn lift_fill(fill: Color32, mix: f32) -> Color32 {
    let toward_white = !USE_LIGHT.load(Ordering::Relaxed);
    if fill.a() == 0 {
        let a = hover_alpha(0, mix);
        return if toward_white {
            Color32::from_white_alpha(a)
        } else {
            Color32::from_black_alpha(a)
        };
    }
    let (r, g, b) = lift_rgb(fill.r(), fill.g(), fill.b(), mix, toward_white);
    Color32::from_rgba_unmultiplied(r, g, b, fill.a())
}

pub fn feel_response(
    ui: &egui::Ui,
    resp: egui::Response,
    fill: Color32,
) -> (egui::Response, egui::Rect, Color32) {
    let hovered = resp.hovered();
    let pressed = resp.is_pointer_button_down_on();
    let id = resp.id;
    let base = resp.rect;
    let resp = pointing(resp);
    let hover_t = ui
        .ctx()
        .animate_bool_with_time(id.with("feel-h"), hovered, HOVER_SECS);
    let press_t = ui
        .ctx()
        .animate_bool_with_time(id.with("feel-p"), pressed, PRESS_SECS);
    let scale = feel_scale(hover_t, press_t);
    let mix = hover_mix(hover_t, press_t);
    let (x, y, w, h) = felt_rect(base.min.x, base.min.y, base.width(), base.height(), scale);
    let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
    (resp, rect, lift_fill(fill, mix))
}

#[allow(dead_code)]
pub fn mark(ctx: &egui::Context) -> TextureHandle {
    let bytes = include_bytes!("../assets/grokhub-32.png");
    let img = image::load_from_memory(bytes).expect("grokhub mark");
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    ctx.load_texture(
        "grokhub-mark",
        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        TextureOptions::LINEAR,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_color_endpoints() {
        assert_eq!(
            blend_color(Color32::from_rgb(0, 0, 0), Color32::from_rgb(100, 100, 100), 0.0),
            Color32::from_rgb(0, 0, 0)
        );
        assert_eq!(
            blend_color(Color32::from_rgb(0, 0, 0), Color32::from_rgb(100, 100, 100), 1.0),
            Color32::from_rgb(100, 100, 100)
        );
    }

    #[test]
    #[allow(clippy::assertions_on_constants)] // pins design constants
    fn grok_com_chrome_tokens() {
        assert_eq!(BG, Color32::from_rgb(5, 5, 5));
        assert_eq!(SURFACE, Color32::from_rgb(20, 20, 20));
        assert_eq!(PANEL, Color32::from_rgb(33, 33, 33));
        assert_eq!(FG, Color32::from_rgb(252, 252, 252));
        assert_eq!(MUTED, Color32::from_rgb(158, 158, 158));
        assert_eq!(GREET_HERO, 32.0);
        assert!(GREET_HERO > FONT_HEADING);
        assert_eq!(QUERY_MIN_H, 60.0);
        assert_eq!(QUERY_RADIUS, 160.0);
        assert_eq!(HIT, 40.0);
        assert_eq!(NAV_ROW_H, 40.0);
        assert_eq!(FONT_UI, 15.0);
        assert_eq!(FONT_CHROME, 14.0);
        assert_eq!(FONT_HEADING, 22.0);
        assert_eq!(TITLEBAR_H, 36.0);
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
        assert_eq!(CABIN_MENU[0], ("history", "History"));
        assert_eq!(CABIN_MENU[1], ("settings", "Settings"));
        assert!(CABIN_MENU.iter().all(|(id, _)| *id != "command"));
        assert!(CABIN_MENU.iter().all(|(id, _)| *id != "connectors"));
        assert_eq!(stage_subtitle("history"), "Past chats");
        assert_eq!(stage_subtitle("chat"), "Recent chat");
        assert_eq!(stage_subtitle("imagine"), "Images");
        assert_eq!(stage_subtitle("connectors"), "MCP / skills / plugins");
        assert_eq!(title_font(40.0).size, 40.0);
        set_paint_dark(true);
        assert_eq!(bg(), BG);
        set_paint_dark(false);
        assert_eq!(bg(), LIGHT_BG);
        assert_eq!(fg(), LIGHT_FG);
        set_paint_dark(true);
        assert_eq!(bg(), BG);
    }

    #[test]
    fn lift_fill_washes_transparent() {
        set_paint_dark(true);
        assert_eq!(lift_fill(Color32::TRANSPARENT, 0.0).a(), 0);
        let hover = lift_fill(Color32::TRANSPARENT, 0.10);
        assert!(hover.a() > 8);
        let solid = lift_fill(Color32::from_rgb(20, 20, 20), 0.10);
        assert!(solid.r() > 20);
        set_paint_dark(false);
        let light = lift_fill(Color32::from_rgb(244, 244, 245), 0.10);
        assert!(light.r() < 244);
        set_paint_dark(true);
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
}
