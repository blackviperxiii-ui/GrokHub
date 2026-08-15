//! Cabin chrome measured from live grok.com dark (`scheme-dark`, 2026-08-15).
//! Recreated in egui — no grok.com JS, no webview.

use eframe::egui::{
    self, Color32, ColorImage, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    TextureHandle, TextureOptions,
};
use std::sync::atomic::{AtomicBool, Ordering};

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
/// `--border-l1` ~8% white on base
pub const BORDER: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
/// `--border-l2` ~14% white
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
/// `--sidebar-accent` `240 5% 26%`
pub const NAV_ACTIVE: Color32 = Color32::from_rgb(0x3f, 0x3f, 0x46);
pub const BUBBLE_USER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const BUBBLE_ASSISTANT: Color32 = PANEL;
pub const LIVE: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);
pub const SETUP: Color32 = Color32::from_rgb(0xea, 0xb3, 0x08);
pub const OFFLINE: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
pub const SIDEBAR_W: f32 = 260.0;
pub const TITLEBAR_H: f32 = 28.0;
/// `.query-bar` measured max-width
pub const QUERY_MAX_W: f32 = 800.0;
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
pub const WORDMARK: f32 = 56.0;
/// grok.com/imagine `h1.text-[22px].leading-7`
pub const IMAGINE_TITLE: f32 = 22.0;
/// gap from h1 to `.query-bar` on /imagine
pub const IMAGINE_GAP: f32 = 32.0;
/// measured Imagine query-bar width
pub const IMAGINE_BAR_W: f32 = 768.0;
/// grok.com/imagine `.query-bar` measured height
pub const IMAGINE_BAR_H: f32 = 94.0;
/// Imagine `.query-bar` `border-radius: 20px` — not the chat pill
pub const IMAGINE_BAR_RADIUS: f32 = 20.0;
/// Imagine Upload / Submit `size-9`
pub const IMAGINE_HIT: f32 = 36.0;
/// grok.com/imagine masonry short tile (~230)
pub const IMAGINE_TILE_SHORT: f32 = 230.0;
/// grok.com/imagine masonry tall tile (~345)
pub const IMAGINE_TILE_TALL: f32 = 345.0;

/// Live grok.com primary rail. Settings is an avatar menu, not a row.
pub const GROK_NAV: &[(&str, &str)] = &[
    ("imagine", "Imagine"),
    ("automations", "Automations"),
    ("skills", "Skills and Connectors"),
];

/// Cabin-only panes. Opened from the avatar settings menu.
pub const CABIN_MENU: &[(&str, &str)] = &[
    ("settings", "Settings"),
    ("workboard", "Workboard"),
    ("memory", "Memory"),
    ("devices", "Devices"),
    ("command", "Command"),
    ("queue", "Queue"),
    ("eyes", "Eyes"),
];

pub const WORKSPACE: &[(&str, &str)] = GROK_NAV;
pub const TOOLS: &[(&str, &str)] = CABIN_MENU;

#[allow(dead_code)]
pub fn stage_subtitle(id: &str) -> &'static str {
    match id {
        "history" => "Past chats",
        "imagine" => "Images",
        "workboard" => "Pinned tasks",
        "skills" => "Personal skills and connectors",
        "automations" => "Scheduled tasks",
        "command" => "Overview",
        "queue" => "Background jobs",
        "settings" => "Preferences",
        "devices" => "Paired computers",
        "memory" => "SOUL / USER / MEMORY",
        "eyes" => "Windshield",
        "connectors" => "GitHub",
        _ => "GrokHub",
    }
}

fn install_inter(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Ok(regular) = std::fs::read("/usr/share/fonts/truetype/macos/Inter-Regular.ttf") {
        fonts
            .font_data
            .insert("inter".into(), FontData::from_owned(regular));
        if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
            fam.insert(0, "inter".into());
        }
    }
    if let Ok(medium) = std::fs::read("/usr/share/fonts/truetype/macos/Inter-Medium.ttf") {
        fonts
            .font_data
            .insert("inter-medium".into(), FontData::from_owned(medium));
        if let Some(fam) = fonts.families.get_mut(&FontFamily::Proportional) {
            fam.insert(0, "inter-medium".into());
        }
    }
    let mut bold_stack = Vec::new();
    if let Ok(bold) = std::fs::read("/usr/share/fonts/truetype/macos/Inter-Bold.ttf") {
        fonts
            .font_data
            .insert("inter-bold".into(), FontData::from_owned(bold));
        bold_stack.push("inter-bold".into());
    }
    if fonts.font_data.contains_key("inter") {
        bold_stack.push("inter".into());
    }
    if let Some(prop) = fonts.families.get(&FontFamily::Proportional) {
        for name in prop {
            if !bold_stack.contains(name) {
                bold_stack.push(name.clone());
            }
        }
    }
    if !bold_stack.is_empty() {
        fonts
            .families
            .insert(FontFamily::Name("inter-bold".into()), bold_stack);
    }
    let mono = std::fs::read("/usr/share/fonts/truetype/macos/JetBrainsMono-Regular.ttf")
        .or_else(|_| std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"));
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

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.override_text_color = Some(FG);
    visuals.panel_fill = SURFACE;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = ELEVATED;
    visuals.code_bg_color = ELEVATED;
    visuals.hyperlink_color = FG;
    visuals.warn_fg_color = MUTED;
    visuals.error_fg_color = FG;
    visuals.selection.bg_fill = ELEVATED;
    visuals.selection.stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = SURFACE;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.bg_fill = ELEVATED;
    visuals.widgets.inactive.weak_bg_fill = ELEVATED;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, FG);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x29, 0x29, 0x29);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x29, 0x29, 0x29);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, FG);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.active.bg_fill = ELEVATED;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, FG);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.open.bg_fill = ELEVATED;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, FG);
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_rounding = 8.0.into();
    visuals.menu_rounding = 8.0.into();
    visuals.widgets.noninteractive.rounding = 8.0.into();
    visuals.widgets.inactive.rounding = 8.0.into();
    visuals.widgets.hovered.rounding = 8.0.into();
    visuals.widgets.active.rounding = 8.0.into();
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(TextStyle::Small, FontId::new(FONT_META, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Body, FontId::new(FONT_UI, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Button, FontId::new(FONT_CHROME, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Heading, FontId::new(FONT_UI, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace));
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.visuals = ctx.style().visuals.clone();
    ctx.set_style(style);
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
    fn grok_com_chrome_tokens() {
        assert_eq!(BG, Color32::from_rgb(5, 5, 5));
        assert_eq!(SURFACE, Color32::from_rgb(20, 20, 20));
        assert_eq!(PANEL, Color32::from_rgb(33, 33, 33));
        assert_eq!(FG, Color32::from_rgb(252, 252, 252));
        assert_eq!(MUTED, Color32::from_rgb(158, 158, 158));
        assert_eq!(QUERY_MAX_W, 800.0);
        assert_eq!(QUERY_MIN_H, 60.0);
        assert_eq!(QUERY_RADIUS, 160.0);
        assert_eq!(HIT, 40.0);
        assert_eq!(NAV_ROW_H, 40.0);
        assert_eq!(FONT_UI, 15.0);
        assert_eq!(FONT_CHROME, 14.0);
        assert_eq!(IMAGINE_TITLE, 22.0);
        assert_eq!(IMAGINE_GAP, 32.0);
        assert_eq!(IMAGINE_BAR_W, 768.0);
        assert_eq!(IMAGINE_BAR_H, 94.0);
        assert_eq!(IMAGINE_BAR_RADIUS, 20.0);
        assert_eq!(IMAGINE_HIT, 36.0);
        assert_eq!(IMAGINE_TILE_SHORT, 230.0);
        assert_eq!(IMAGINE_TILE_TALL, 345.0);
        assert_ne!(IMAGINE_BAR_RADIUS, QUERY_RADIUS);
        assert_eq!(GROK_NAV[0], ("imagine", "Imagine"));
        assert!(GROK_NAV.iter().all(|(id, _)| *id != "settings"));
        assert_eq!(CABIN_MENU[0], ("settings", "Settings"));
        assert!(TOOLS.iter().all(|(id, _)| *id != "connectors"));
        assert_eq!(stage_subtitle("history"), "Past chats");
        assert_eq!(stage_subtitle("imagine"), "Images");
        assert_eq!(stage_subtitle("connectors"), "GitHub");
        assert_eq!(title_font(40.0).size, 40.0);
    }
}
