//! Electron visual system — true black / gray / white. No warm cabin brown.

use eframe::egui::{
    self, Color32, ColorImage, FontFamily, FontId, Stroke, TextStyle, TextureHandle, TextureOptions,
};

pub const BG: Color32 = Color32::from_rgb(0x09, 0x09, 0x09);
pub const SURFACE: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
pub const PANEL: Color32 = Color32::from_rgb(0x17, 0x17, 0x17);
pub const ELEVATED: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
pub const FG: Color32 = Color32::from_rgb(0xf5, 0xf5, 0xf5);
pub const MUTED: Color32 = Color32::from_rgb(0xa3, 0xa3, 0xa3);
pub const SUBTLE: Color32 = Color32::from_rgb(0x73, 0x73, 0x73);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
pub const BUBBLE_USER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const BUBBLE_ASSISTANT: Color32 = PANEL;
pub const LIVE: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);
pub const SETUP: Color32 = Color32::from_rgb(0xea, 0xb3, 0x08);
pub const OFFLINE: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
pub const SIDEBAR_W: f32 = 240.0;
pub const TITLEBAR_H: f32 = 40.0;

pub const WORKSPACE: &[(&str, &str)] = &[
    ("chat", "Agent"),
    ("history", "History"),
    ("imagine", "Imagine"),
    ("workboard", "Workboard"),
    ("settings", "Settings"),
];

pub const TOOLS: &[(&str, &str)] = &[
    ("skills", "Skills and Connectors"),
    ("automations", "Automations"),
    ("command", "Command"),
    ("queue", "Queue"),
    ("devices", "Devices"),
    ("memory", "Memory"),
    ("eyes", "Eyes"),
];

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

pub fn apply(ctx: &egui::Context) {
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
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x2a, 0x2a, 0x2a);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2a, 0x2a, 0x2a);
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
    style.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Heading, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace));
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.visuals = ctx.style().visuals.clone();
    ctx.set_style(style);
}

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
    fn electron_palette() {
        assert_eq!(BG, Color32::from_rgb(9, 9, 9));
        assert_eq!(SURFACE, Color32::from_rgb(17, 17, 17));
        assert_eq!(PANEL, Color32::from_rgb(23, 23, 23));
        assert_eq!(FG, Color32::from_rgb(245, 245, 245));
        assert_eq!(WORKSPACE[0], ("chat", "Agent"));
        assert_eq!(TOOLS[1], ("automations", "Automations"));
        assert_eq!(TOOLS[3], ("queue", "Queue"));
        assert!(TOOLS.iter().all(|(id, _)| *id != "connectors"));
        assert_eq!(stage_subtitle("history"), "Past chats");
        assert_eq!(stage_subtitle("imagine"), "Images");
        assert_eq!(stage_subtitle("connectors"), "GitHub");
    }
}
