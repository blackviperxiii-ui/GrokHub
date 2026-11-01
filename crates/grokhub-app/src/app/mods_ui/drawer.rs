//! The open mod's drawer: what it is, its description as plain text, its
//! files and what they need, and a link to its page on its own site.
//! Installing comes later; the button shows but does nothing yet.

use eframe::egui;
use grokhub_core::mods::{Download, ModFile, ModSummary, Source};

use super::jobs;

pub(super) enum Act {
    None,
    Close,
    Failed(String),
}

/// How many files the drawer lists, newest first as the source sends them.
const FILES_SHOWN: usize = 6;

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(crate::theme::TYPE_CAPTION)
            .extra_letter_spacing(1.2)
            .strong()
            .color(crate::theme::muted()),
    );
}

/// "4.2 MB" or "880 KB".
pub(super) fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1e6)
    } else {
        format!("{} KB", bytes.div_ceil(1_000))
    }
}

/// What getting a file would take, in a few words.
pub(super) fn file_line(f: &ModFile) -> String {
    let mut parts = Vec::new();
    if !f.version.is_empty() {
        parts.push(f.version.clone());
    }
    if f.size > 0 {
        parts.push(size_label(f.size));
    }
    match f.download {
        Download::Direct(_) => {}
        Download::Browser(_) => parts.push("download on the site".into()),
        Download::Steam(_) => parts.push("subscribe in Steam".into()),
    }
    parts.join(" · ")
}

pub(super) fn show(
    ui: &mut egui::Ui,
    game_id: &'static str,
    source: Source,
    m: &ModSummary,
    offline: bool,
) -> Act {
    let mut act = Act::None;
    egui::Frame::NONE
        .fill(crate::theme::surface())
        .stroke(egui::Stroke::new(1.0, crate::theme::border()))
        .corner_radius(crate::theme::RADIUS_SM)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&m.name)
                        .size(crate::theme::TYPE_TITLE)
                        .strong()
                        .color(crate::theme::fg()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::cards::ghost_pill(ui, "Close") {
                        act = Act::Close;
                    }
                });
            });
            ui.label(
                egui::RichText::new(format!("by {} on {}", m.author, source.label()))
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
            );
            ui.add_space(8.0);
            let site = format!("Open on {}", source.label());
            if !m.page_url.is_empty() && crate::cards::ghost_pill(ui, &site) {
                if let Err(e) = super::open_link(&m.page_url) {
                    act = Act::Failed(e);
                }
            }
            ui.add_enabled_ui(false, |ui| {
                let _ = crate::ui_kit::recipe::button(
                    ui,
                    None,
                    "INSTALL (COMING SOON)",
                    false,
                    egui::vec2(ui.available_width(), 34.0),
                    true,
                );
            });
            ui.add_space(8.0);
            if offline {
                ui.label(
                    egui::RichText::new("Offline only is on.")
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                );
                return;
            }
            let detail = jobs::detail(ui.ctx(), game_id, source, &m.id);
            let files = jobs::files(ui.ctx(), game_id, source, &m.id);
            if detail.busy || files.busy {
                ui.spinner();
            }
            if detail.error.is_some() || files.error.is_some() {
                let err = detail.error.or(files.error).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(err)
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::muted()),
                    );
                    if crate::cards::ghost_pill(ui, "Retry") {
                        jobs::retry_mod(game_id, source, &m.id);
                    }
                });
            }
            egui::ScrollArea::vertical()
                .id_salt("mods-drawer")
                .max_height((ui.available_height() - 40.0).max(160.0))
                .show(ui, |ui| {
                    if let Some(files) = &files.value {
                        files_list(ui, files);
                    }
                    let text = detail
                        .value
                        .as_ref()
                        .map(|d| d.description.as_str())
                        .filter(|t| !t.trim().is_empty())
                        .unwrap_or(m.summary.as_str());
                    if !text.is_empty() {
                        ui.add_space(8.0);
                        caption(ui, "ABOUT");
                        ui.label(
                            egui::RichText::new(text)
                                .size(crate::theme::TYPE_BODY)
                                .color(crate::theme::fg()),
                        );
                    }
                });
        });
    act
}

fn files_list(ui: &mut egui::Ui, files: &[ModFile]) {
    if files.is_empty() {
        return;
    }
    caption(ui, "FILES");
    for f in files.iter().take(FILES_SHOWN) {
        let name = if f.name.is_empty() {
            &f.file_name
        } else {
            &f.name
        };
        ui.label(
            egui::RichText::new(name)
                .size(crate::theme::TYPE_BODY)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.label(
            egui::RichText::new(file_line(f))
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
    }
    let deps: Vec<&str> = files
        .iter()
        .find(|f| f.primary)
        .or(files.first())
        .map(|f| f.deps.iter().map(String::as_str).collect())
        .unwrap_or_default();
    if !deps.is_empty() {
        ui.add_space(6.0);
        caption(ui, "NEEDS");
        ui.label(
            egui::RichText::new(deps.join(", "))
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::fg()),
        );
    }
}
