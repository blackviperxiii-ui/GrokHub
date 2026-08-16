use eframe::egui::{Color32, RichText, TextStyle, Ui, Vec2};
use grokhub_core::bubble_max_width;

/// Cap for wrapping. Short bubbles hug via `bubble_outer_width`, they do not stretch to this.
pub fn bubble_width(available: f32) -> f32 {
    bubble_max_width(available)
}

pub fn measure_text(ui: &Ui, text: &str, wrap: f32) -> Vec2 {
    let font = TextStyle::Body.resolve(ui.style());
    let wrap = wrap.max(1.0);
    if text.is_empty() {
        return Vec2::new(0.0, ui.text_style_height(&TextStyle::Body));
    }
    ui.fonts(|f| f.layout(text.to_owned(), font, Color32::WHITE, wrap))
        .size()
}

pub fn show(ui: &mut Ui, text: &str) {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("### ") {
            ui.label(RichText::new(rest).strong());
        } else if let Some(rest) = line.strip_prefix("## ") {
            ui.label(RichText::new(rest).heading());
        } else if let Some(rest) = line.strip_prefix("# ") {
            ui.label(RichText::new(rest).heading().strong());
        } else if let Some(rest) = line.strip_prefix("- ") {
            ui.horizontal(|ui| {
                ui.label("·");
                inline(ui, rest);
            });
        } else if line.starts_with("```") {
            ui.label(RichText::new(line).monospace().color(Color32::from_rgb(0xa3, 0xa3, 0xa3)));
        } else if line.is_empty() {
            ui.add_space(6.0);
        } else {
            inline(ui, line);
        }
    }
}

fn inline(ui: &mut Ui, line: &str) {
    ui.horizontal_wrapped(|ui| {
        let mut rest = line;
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix("**") {
                if let Some(end) = after.find("**") {
                    ui.label(RichText::new(&after[..end]).strong());
                    rest = &after[end + 2..];
                    continue;
                }
            }
            if let Some(after) = rest.strip_prefix('`') {
                if let Some(end) = after.find('`') {
                    ui.label(
                        RichText::new(&after[..end])
                            .monospace()
                            .color(Color32::from_rgb(0xd4, 0xd4, 0xd4)),
                    );
                    rest = &after[end + 1..];
                    continue;
                }
            }
            let next = rest
                .find("**")
                .into_iter()
                .chain(rest.find('`'))
                .min()
                .unwrap_or(rest.len())
                .max(1);
            ui.label(&rest[..next]);
            rest = &rest[next..];
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{bubble_width, measure_text};
    use grokhub_core::{bubble_max_width, bubble_outer_width, bubble_wrap_width, BUBBLE_PAD_X};

    #[test]
    fn splits_markers() {
        assert!("**bold** and `code`".contains("**"));
    }

    #[test]
    fn bubble_cap_is_not_the_forced_width() {
        let cap = bubble_width(800.0);
        assert!((cap - bubble_max_width(800.0)).abs() < 0.1);
        assert!(cap < 800.0);
        assert!(bubble_width(100.0) <= 100.0);
        let hugged = bubble_outer_width(800.0, 40.0, BUBBLE_PAD_X);
        assert!(hugged < 120.0);
        assert!(hugged < cap);
    }

    #[test]
    fn measured_short_line_is_narrower_than_the_row_cap() {
        with_fonts_ui(|ui| {
            let wrap = bubble_wrap_width(800.0, BUBBLE_PAD_X);
            let sz = measure_text(ui, "Hi", wrap);
            let outer = bubble_outer_width(800.0, sz.x, BUBBLE_PAD_X);
            assert!(outer < 160.0, "short bubble {outer} content {}", sz.x);
            assert!(sz.y > 8.0);
        });
    }

    #[test]
    fn measured_long_line_wraps_and_grows_taller() {
        with_fonts_ui(|ui| {
            let wrap = bubble_wrap_width(800.0, BUBBLE_PAD_X);
            let short = measure_text(ui, "Hi", wrap);
            let long = measure_text(ui, &"word ".repeat(80), wrap);
            assert!(long.x <= wrap + 1.0);
            assert!(long.y > short.y * 2.0, "long y {} short y {}", long.y, short.y);
            let outer = bubble_outer_width(800.0, long.x, BUBBLE_PAD_X);
            let cap = bubble_max_width(800.0);
            assert!(outer <= cap + 1.0, "outer {outer} cap {cap}");
            assert!(outer > cap * 0.85, "wrapped bubble too skinny {outer}");
        });
    }

    fn with_fonts_ui(mut add: impl FnMut(&mut eframe::egui::Ui)) {
        let ctx = eframe::egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            eframe::egui::CentralPanel::default().show(ctx, |ui| add(ui));
        });
    }
}
