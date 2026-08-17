use eframe::egui::{Color32, Label, RichText, TextStyle, TextWrapMode, Ui, Vec2};
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
    ui.style_mut().wrap_mode = Some(TextWrapMode::Wrap);
    let wrap = ui.available_width().max(1.0);
    ui.set_max_width(wrap);
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            wrapping_label(
                ui,
                RichText::new(line)
                    .monospace()
                    .color(crate::theme::muted()),
                wrap,
            );
            continue;
        }
        if in_fence {
            wrapping_label(ui, RichText::new(line).monospace(), wrap);
            continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            wrapping_label(ui, RichText::new(rest).strong(), wrap);
        } else if let Some(rest) = line.strip_prefix("## ") {
            wrapping_label(ui, RichText::new(rest).heading(), wrap);
        } else if let Some(rest) = line.strip_prefix("# ") {
            wrapping_label(ui, RichText::new(rest).heading().strong(), wrap);
        } else if let Some(rest) = line.strip_prefix("- ") {
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(wrap);
                ui.style_mut().wrap_mode = Some(TextWrapMode::Wrap);
                ui.label("·");
                inline(ui, rest, (wrap - 18.0).max(1.0));
            });
        } else if line.is_empty() {
            ui.add_space(6.0);
        } else {
            inline(ui, line, wrap);
        }
    }
}

fn wrapping_label(ui: &mut Ui, text: RichText, wrap: f32) {
    ui.set_max_width(wrap);
    ui.add(Label::new(text).wrap());
}

fn inline(ui: &mut Ui, line: &str, wrap: f32) {
    if !line.contains("**") && !line.contains('`') {
        wrapping_label(ui, RichText::new(line), wrap);
        return;
    }
    ui.allocate_ui_with_layout(
        Vec2::new(wrap, 0.0),
        eframe::egui::Layout::left_to_right(eframe::egui::Align::Min).with_main_wrap(true),
        |ui| {
            ui.set_max_width(wrap);
            ui.style_mut().wrap_mode = Some(TextWrapMode::Wrap);
            let mut rest = line;
            while !rest.is_empty() {
                if let Some(after) = rest.strip_prefix("**") {
                    if let Some(end) = after.find("**") {
                        ui.add(Label::new(RichText::new(&after[..end]).strong()).wrap());
                        rest = &after[end + 2..];
                        continue;
                    }
                }
                if let Some(after) = rest.strip_prefix('`') {
                    if let Some(end) = after.find('`') {
                        ui.add(
                            Label::new(
                                RichText::new(&after[..end])
                                    .monospace()
                                    .color(crate::theme::subtle()),
                            )
                            .wrap(),
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
                ui.add(Label::new(&rest[..next]).wrap());
                rest = &rest[next..];
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{bubble_width, measure_text};
    use grokhub_core::{
        bubble_max_width, bubble_outer_width, bubble_wrap_width, BUBBLE_MAX_FRAC, BUBBLE_PAD_X,
    };

    #[test]
    fn splits_markers() {
        assert!("**bold** and `code`".contains("**"));
    }

    #[test]
    fn fenced_code_body_is_monospace() {
        let src = include_str!("markdown.rs");
        let start = src.find("pub fn show").expect("show");
        let slice = &src[start..start + 1600];
        assert!(slice.contains("in_fence"), "{slice}");
        assert!(
            slice.contains("RichText::new(line).monospace()"),
            "fence body must stay monospace: {slice}"
        );
    }

    #[test]
    fn bubble_cap_is_not_the_forced_width() {
        let cap = bubble_width(800.0);
        assert!((cap - bubble_max_width(800.0)).abs() < 0.1);
        assert!(cap < 800.0);
        assert!(
            (cap - 800.0 * BUBBLE_MAX_FRAC).abs() < 0.1,
            "800px pane must wrap at ~84%, got {cap}"
        );
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

    #[test]
    fn long_sentence_wraps_even_when_available_width_is_huge() {
        with_fonts_ui(|ui| {
            let wrap = bubble_wrap_width(f32::INFINITY, BUBBLE_PAD_X);
            let body = "the clam gods? oh you know... ancient, briny, and extremely picky about their cream-to-broth ratio. they live in the black void between chowder pots, only emerging when someone dares to say manhattan style in their presence.";
            let sz = measure_text(ui, body, wrap);
            assert!(
                sz.x <= wrap + 1.0,
                "sentence must wrap inside {wrap}, got {}",
                sz.x
            );
            assert!(
                sz.y > 28.0,
                "one long sentence must become several lines, height {}",
                sz.y
            );
        });
    }

    fn with_fonts_ui(mut add: impl FnMut(&mut eframe::egui::Ui)) {
        let ctx = eframe::egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            eframe::egui::CentralPanel::default().show(ctx, |ui| add(ui));
        });
    }
}
