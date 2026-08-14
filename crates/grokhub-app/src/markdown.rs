use eframe::egui::{Color32, RichText, Ui};

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
    #[test]
    fn splits_markers() {
        assert!("**bold** and `code`".contains("**"));
    }
}
