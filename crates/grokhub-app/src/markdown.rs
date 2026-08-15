use eframe::egui::{Color32, RichText, Ui};

/// Lock the chat bubble to a real wrap width.
/// A wrapping label inside `horizontal` collapses to one glyph unless min==max==this.
pub fn bubble_width(available: f32) -> f32 {
    let avail = available.max(0.0);
    let w = avail * 0.78;
    if avail < 240.0 {
        avail
    } else {
        w.clamp(240.0, avail)
    }
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
    use super::bubble_width;

    #[test]
    fn splits_markers() {
        assert!("**bold** and `code`".contains("**"));
    }

    #[test]
    fn bubble_locks_a_real_wrap_width() {
        let w = bubble_width(800.0);
        assert!((w - 624.0).abs() < 0.1, "{w}");
        assert!(w >= 240.0);
        assert!(bubble_width(100.0) <= 100.0);
    }
}
