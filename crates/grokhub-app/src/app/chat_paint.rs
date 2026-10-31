//! Transcript chrome the chat pane paints without a `Cabin`: the running
//! pulse, tool cards and their grouped rows, and the eyes' last-frame
//! texture (decoded off the UI thread).

use super::*;

pub(super) fn paint_running(ui: &mut egui::Ui, label: &str, hint: &str) {
    crate::cards::paint_run_pulse(ui, label, hint)
}

/// `Grep · 3 matches…` while it runs. The header says what the call touched,
/// not just its tool name.
pub(super) fn tool_header_text(label: &str, running: bool) -> String {
    if running {
        format!("{label}…")
    } else {
        label.to_string()
    }
}
pub(super) fn tool_header_color(failed: bool) -> Color32 {
    if failed {
        crate::cards::chip_tone_color(crate::cards::ChipTone::Offline)
    } else {
        crate::theme::muted()
    }
}

pub(super) fn paint_one_tool_card(ui: &mut egui::Ui, card: &ToolCard) {
    let label = tool_group_label([(
        card.title.as_str(),
        card.status.as_str(),
        card.detail.as_str(),
    )]);
    egui::CollapsingHeader::new(
        RichText::new(tool_header_text(&label, tool_status_running(&card.status)))
            .size(crate::theme::FONT_META)
            .color(tool_header_color(tool_status_failed(&card.status))),
    )
    .id_salt(("tool-card", card.id.as_str()))
    .default_open(false)
    .show(ui, |ui| {
        paint_tool_card_body(ui, card);
        harness_ui::paint_click_marker(ui, &[card]);
    });
}

/// A run of back-to-back tool calls is one quiet row. Open it for each call.
pub(super) fn paint_tool_group(ui: &mut egui::Ui, cards: &[std::borrow::Cow<'_, ToolCard>]) {
    let [first, ..] = cards else {
        return;
    };
    if cards.len() == 1 {
        paint_one_tool_card(ui, first);
        return;
    }
    let label = tool_group_label(
        cards
            .iter()
            .map(|c| (c.title.as_str(), c.status.as_str(), c.detail.as_str())),
    );
    let running = cards.iter().any(|c| tool_status_running(&c.status));
    let failed = cards.iter().any(|c| tool_status_failed(&c.status));
    egui::CollapsingHeader::new(
        RichText::new(tool_header_text(&label, running))
            .size(crate::theme::FONT_META)
            .color(tool_header_color(failed)),
    )
    .id_salt(("tool-group", first.id.as_str()))
    .default_open(false)
    .show(ui, |ui| {
        for card in cards {
            paint_tool_card_body(ui, card);
            ui.add_space(4.0);
        }
        let refs: Vec<&ToolCard> = cards.iter().map(|c| c.as_ref()).collect();
        harness_ui::paint_click_marker(ui, &refs);
    });
}

/// Stored tool rows of a finished turn: one line each, a failure marked.
pub(super) fn paint_tool_rows(ui: &mut egui::Ui, rows: &[grokhub_core::ToolRow]) {
    for row in rows {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(tool_display_title(&row.title))
                    .size(12.0)
                    .color(crate::theme::fg()),
            );
            if tool_status_failed(&row.status) {
                ui.label(
                    RichText::new(row.status.trim())
                        .size(12.0)
                        .color(tool_header_color(true)),
                );
            }
        });
        if !crate::cards::paint_native_media_card(ui, &row.title, &row.detail) {
            let detail = row.detail.trim();
            if !detail.is_empty() && !detail.starts_with('{') && !detail.starts_with('[') {
                ui.label(
                    RichText::new(detail.chars().take(160).collect::<String>())
                        .size(12.0)
                        .color(crate::theme::muted()),
                );
            }
        }
        ui.add_space(4.0);
    }
}

pub(super) fn paint_tool_card_body(ui: &mut egui::Ui, card: &ToolCard) {
    egui::Frame::NONE
        .fill(egui::Color32::TRANSPARENT)
        .corner_radius(crate::theme::CHROME_RADIUS)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&card.title)
                        .size(13.0)
                        .color(crate::theme::fg()),
                );
                crate::cards::status_chip(
                    ui,
                    &card.status,
                    if card.status == "failed" {
                        crate::cards::ChipTone::Offline
                    } else if card.is_computer_use() {
                        crate::cards::ChipTone::Live
                    } else {
                        crate::cards::ChipTone::Mute
                    },
                );
            });
            let painted = crate::cards::paint_native_media_card(ui, &card.title, &card.detail);
            if !painted
                && !card.diff.is_empty()
                && !card.diff.trim().starts_with('{')
                && !card.diff.trim().starts_with('[')
            {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(card.diff.chars().take(800).collect::<String>())
                        .size(12.0)
                        .monospace()
                        .color(crate::theme::muted()),
                );
            } else if !painted
                && !card.detail.is_empty()
                && !card.detail.trim().starts_with('{')
                && !card.detail.trim().starts_with('[')
                && !grokhub_core::tool_detail_is_status(&card.detail)
            {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(card.detail.chars().take(120).collect::<String>())
                        .size(12.0)
                        .color(crate::theme::muted()),
                );
            }
            if let Some(url) = card.image_data_url.as_deref() {
                if let Some((tex, size)) = eyes_frame_tex(ui.ctx(), url) {
                    let max_w = ui.available_width().min(360.0);
                    let shown = crate::cards::framed_preview(ui, &tex, size, max_w);
                    harness_ui::remember_work_frame(ui, &card.id, shown, size);
                }
            }
        });
}

pub(super) fn eyes_frame_tex(
    ctx: &egui::Context,
    url: &str,
) -> Option<(TextureHandle, [usize; 2])> {
    if url.len() > FRAME_CAP {
        return None;
    }
    let key: String = url.chars().take(48).collect();
    let id = egui::Id::new(("eyes-frame", url.len(), key.as_str()));
    if let Some(hit) = ctx.data(|d| d.get_temp::<(TextureHandle, [usize; 2])>(id)) {
        return Some(hit);
    }
    let cache_key = format!("{}:{key}", url.len());
    if let Some(img) = take_eyes_rgba(&cache_key) {
        let size = [img.width() as usize, img.height() as usize];
        let tex = ctx.load_texture(
            "eyes-last-frame",
            ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
            TextureOptions::LINEAR,
        );
        let hit = (tex, size);
        ctx.data_mut(|d| d.insert_temp(id, hit.clone()));
        return Some(hit);
    }
    kick_eyes_tex(ctx.clone(), cache_key, url.to_string());
    None
}

struct EyesTexGate {
    inflight: HashSet<String>,
    ready: HashMap<String, image::RgbaImage>,
}

fn eyes_tex_gate() -> &'static Mutex<EyesTexGate> {
    static G: OnceLock<Mutex<EyesTexGate>> = OnceLock::new();
    G.get_or_init(|| {
        Mutex::new(EyesTexGate {
            inflight: HashSet::new(),
            ready: HashMap::new(),
        })
    })
}

pub(super) fn take_eyes_rgba(key: &str) -> Option<image::RgbaImage> {
    let mut g = eyes_tex_gate().lock().ok()?;
    g.ready.remove(key)
}

pub(super) fn kick_eyes_tex(ctx: egui::Context, key: String, url: String) {
    {
        let Ok(mut g) = eyes_tex_gate().lock() else {
            return;
        };
        if g.ready.contains_key(&key) || !g.inflight.insert(key.clone()) {
            return;
        }
    }
    std::thread::spawn(move || {
        let frame = PresenceFrame {
            data_url: url,
            at: 0,
        };
        let decoded = frame_bytes(&frame).and_then(|(_, buf)| {
            if (buf.len() as u64) > IMAGE_FILE_CAP {
                return None;
            }
            if !crate::desktop::image_pixels_ok_for_bytes(&buf) {
                return None;
            }
            image::load_from_memory(&buf).ok().map(|img| img.to_rgba8())
        });
        if let Ok(mut g) = eyes_tex_gate().lock() {
            g.inflight.remove(&key);
            if let Some(img) = decoded {
                g.ready.insert(key, img);
            }
        }
        ctx.request_repaint();
    });
}
