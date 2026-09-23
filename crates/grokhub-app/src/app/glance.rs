//! Cabin C chrome glances: Coding/Life lane (R6), quiet-hours chip (R9),
//! session map (R7), device glance (R8). Fail soft. No Muse / phone stubs.

use super::*;
use grokhub_core::{last_user_text, QuickChip};

pub(super) const LANE_CODING: &str = "coding";
pub(super) const LANE_LIFE: &str = "life";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CabinLane {
    Coding,
    Life,
}

pub(super) fn persistable_cabin_lane(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        LANE_LIFE => LANE_LIFE.into(),
        _ => LANE_CODING.into(),
    }
}

pub(super) fn cabin_lane(raw: &str) -> CabinLane {
    if persistable_cabin_lane(raw) == LANE_LIFE {
        CabinLane::Life
    } else {
        CabinLane::Coding
    }
}

pub(super) fn cabin_lane_label(lane: CabinLane) -> &'static str {
    match lane {
        CabinLane::Coding => "Coding",
        CabinLane::Life => "Life",
    }
}

pub(super) fn flip_cabin_lane(lane: CabinLane) -> CabinLane {
    match lane {
        CabinLane::Coding => CabinLane::Life,
        CabinLane::Life => CabinLane::Coding,
    }
}

fn chip_leans_life(chip: &QuickChip) -> bool {
    let blob = format!("{} {} {}", chip.id, chip.label, chip.value).to_ascii_lowercase();
    blob.contains("imagine") || blob.contains("__nav:imagine")
}

fn chip_leans_coding(chip: &QuickChip) -> bool {
    let blob = format!("{} {} {}", chip.id, chip.label, chip.value).to_ascii_lowercase();
    matches!(chip.kind, grokhub_core::ChipKind::Shell)
        || blob.contains("ship")
        || blob.contains("fix")
        || blob.contains("skill")
        || blob.contains("__nav:skills")
        || blob.contains("/sh")
}

/// Reorder existing chips. Does not invent tiles or rebrand Imagine as Life.
pub(super) fn bias_chips_for_lane(chips: &mut [QuickChip], lane: CabinLane) {
    chips.sort_by_key(|c| match lane {
        CabinLane::Coding => {
            if chip_leans_coding(c) {
                0
            } else if chip_leans_life(c) {
                2
            } else {
                1
            }
        }
        CabinLane::Life => {
            if chip_leans_life(c) {
                0
            } else if chip_leans_coding(c) {
                2
            } else {
                1
            }
        }
    });
}

pub(super) fn quiet_until_chip(now_hm: &str, start: &str, end: &str) -> Option<String> {
    if !quiet_hours_active(now_hm, start, end) {
        return None;
    }
    Some(format!("Quiet until {end}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionMarkKind {
    LastYou,
    Branch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionMark {
    pub kind: SessionMarkKind,
    pub label: String,
    pub thread_id: String,
}

const MAP_LABEL_CHARS: usize = 28;

fn clip_map_label(s: &str) -> String {
    let t = s.trim();
    let mut out: String = t.chars().take(MAP_LABEL_CHARS).collect();
    if t.chars().count() > MAP_LABEL_CHARS {
        out.push('…');
    }
    out
}

/// Compact History map. Empty when there is no last user turn and no fork markers.
pub(super) fn session_markers(
    threads: &[ChatThread],
    current_idx: usize,
) -> Vec<SessionMark> {
    let mut out = Vec::new();
    if let Some(cur) = threads.get(current_idx) {
        if let Some(text) = last_user_text(&cur.messages) {
            out.push(SessionMark {
                kind: SessionMarkKind::LastYou,
                label: format!("Last you · {}", clip_map_label(&text)),
                thread_id: cur.id.clone(),
            });
        }
    }
    for (i, t) in threads.iter().enumerate() {
        if i == current_idx || t.scratch || !t.grok_fork {
            continue;
        }
        let title = if t.title.trim().is_empty() {
            "Fork"
        } else {
            t.title.trim()
        };
        out.push(SessionMark {
            kind: SessionMarkKind::Branch,
            label: format!("Branch · {}", clip_map_label(title)),
            thread_id: t.id.clone(),
        });
        if out.len() >= 4 {
            break;
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeviceGlance {
    pub share_line: String,
    pub has_thumb: bool,
}

/// Omit when hub is off and no last frame is bound. Never names a phone.
pub(super) fn device_glance(hub_on: bool, last_frame_url: Option<&str>) -> Option<DeviceGlance> {
    let thumb = last_frame_url
        .map(str::trim)
        .filter(|u| !u.is_empty() && u.starts_with("data:image"));
    if !hub_on && thumb.is_none() {
        return None;
    }
    let share_line = if hub_on {
        "Sharing"
    } else {
        "Last frame"
    };
    Some(DeviceGlance {
        share_line: share_line.into(),
        has_thumb: thumb.is_some(),
    })
}

impl Cabin {
    pub(super) fn paint_lane_chip(&mut self, ui: &mut egui::Ui) {
        let lane = cabin_lane(&self.cfg.cabin_lane);
        let label = cabin_lane_label(lane);
        let hit = egui::Frame::none()
            .fill(crate::theme::elevated())
            .rounding(12.0)
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
            .inner_margin(egui::Margin::symmetric(10.0, 4.0))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(label)
                            .size(12.0)
                            .color(crate::theme::muted()),
                    )
                    .sense(egui::Sense::click()),
                )
            })
            .inner;
        if hit.clicked() {
            let next = flip_cabin_lane(lane);
            self.cfg.cabin_lane = persistable_cabin_lane(cabin_lane_label(next));
            self.persist_cfg();
            self.refresh_chips();
        }
    }

    pub(super) fn paint_device_glance_row(&mut self, ui: &mut egui::Ui, pane_w: f32) -> bool {
        let glance = device_glance(self.hub_on, self.last_frame_url.as_deref());
        let Some(glance) = glance else {
            return false;
        };
        let inner_w = pane_w.max(1.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(inner_w, 24.0), egui::Sense::click());
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_max_width(inner_w);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    RichText::new(&glance.share_line)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::muted()),
                );
                if glance.has_thumb {
                    if let Some(url) = self.last_frame_url.as_deref() {
                        paint_frame_thumb(ui, url);
                    }
                }
            });
        });
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.nav = Nav::Devices;
            return true;
        }
        false
    }
}

fn paint_frame_thumb(ui: &mut egui::Ui, url: &str) {
    let id = egui::Id::new(("cabin-device-glance", url));
    if let Some(tex) = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        ui.add(egui::Image::from_texture(&tex).fit_to_exact_size(egui::vec2(18.0, 18.0)));
        return;
    }
    let Some(frame) = grokhub_core::store_frame(url, 1) else {
        return;
    };
    let Some((_, bytes)) = grokhub_core::frame_bytes(&frame) else {
        return;
    };
    let Ok(img) = image::load_from_memory(&bytes) else {
        return;
    };
    let thumb = img.thumbnail(36, 36);
    let rgba = thumb.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let tex = ui.ctx().load_texture(
        "cabin-device-glance",
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    ui.ctx().data_mut(|d| d.insert_temp(id, tex.clone()));
    ui.add(egui::Image::from_texture(&tex).fit_to_exact_size(egui::vec2(18.0, 18.0)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_defaults_to_coding() {
        assert_eq!(persistable_cabin_lane(""), LANE_CODING);
        assert_eq!(persistable_cabin_lane("Life"), LANE_LIFE);
        assert_eq!(cabin_lane("nonsense"), CabinLane::Coding);
        assert_eq!(cabin_lane_label(CabinLane::Coding), "Coding");
        assert_eq!(flip_cabin_lane(CabinLane::Coding), CabinLane::Life);
    }

    #[test]
    fn quiet_chip_hides_when_inactive() {
        assert_eq!(quiet_until_chip("10:00", "22:00", "07:00"), None);
        assert_eq!(
            quiet_until_chip("23:30", "22:00", "07:00"),
            Some("Quiet until 07:00".into())
        );
        assert_eq!(quiet_until_chip("12:00", "00:00", "00:00"), None);
    }

    #[test]
    fn session_map_fails_soft_without_markers() {
        let t = ChatThread::new("Chat", false);
        assert!(session_markers(&[t], 0).is_empty());
    }

    #[test]
    fn session_map_names_last_you_and_fork() {
        let mut cur = ChatThread::new("Now", false);
        cur.id = "cur".into();
        cur.messages_mut()
            .push(("user".into(), "ship the pulse".into()));
        let mut fork = ChatThread::new("Alt path", false);
        fork.id = "fork".into();
        fork.grok_fork = true;
        let marks = session_markers(&[cur, fork], 0);
        assert!(marks
            .iter()
            .any(|m| m.kind == SessionMarkKind::LastYou && m.label.contains("ship the pulse")));
        assert!(marks
            .iter()
            .any(|m| m.kind == SessionMarkKind::Branch && m.label.contains("Alt path")));
    }

    #[test]
    fn device_glance_omits_unbound() {
        assert!(device_glance(false, None).is_none());
        assert!(device_glance(false, Some("")).is_none());
        assert!(device_glance(false, Some("https://example/phone.jpg")).is_none());
        let share = device_glance(true, None).expect("sharing");
        assert_eq!(share.share_line, "Sharing");
        assert!(!share.has_thumb);
        let frame = device_glance(false, Some("data:image/jpeg;base64,xx")).expect("frame");
        assert_eq!(frame.share_line, "Last frame");
        assert!(frame.has_thumb);
        let blob = format!("{} {:?}", share.share_line, frame);
        assert!(
            !blob.to_ascii_lowercase().contains("phone"),
            "device glance must not stub a phone: {blob}"
        );
    }

    #[test]
    fn frame_thumb_caches_the_texture() {
        let src = include_str!("glance.rs");
        let thumb = src
            .split("fn paint_frame_thumb(")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("paint_frame_thumb");
        let get = thumb.find("get_temp").expect("get_temp before decode");
        let load = thumb
            .find("load_from_memory")
            .expect("decode only on cache miss");
        let insert = thumb.find("insert_temp").expect("insert_temp after upload");
        assert!(
            get < load && load < insert && thumb.contains("cabin-device-glance"),
            "empty-home paint must reuse the glance texture: {thumb}"
        );
    }

    #[test]
    fn lane_bias_does_not_invent_imagine_as_life() {
        let mut chips = vec![
            QuickChip {
                id: "ship".into(),
                label: "Ship it".into(),
                value: "Ship a minimal solid slice.".into(),
                kind: grokhub_core::ChipKind::Chat,
                score: 1.0,
                hint: String::new(),
                primary: false,
            },
            QuickChip {
                id: "imagine".into(),
                label: "Open Imagine".into(),
                value: "__nav:imagine".into(),
                kind: grokhub_core::ChipKind::Nav,
                score: 1.0,
                hint: String::new(),
                primary: false,
            },
        ];
        bias_chips_for_lane(&mut chips, CabinLane::Life);
        assert_eq!(chips[0].id, "imagine");
        assert_eq!(chips[0].label, "Open Imagine");
        bias_chips_for_lane(&mut chips, CabinLane::Coding);
        assert_eq!(chips[0].id, "ship");
    }
}
