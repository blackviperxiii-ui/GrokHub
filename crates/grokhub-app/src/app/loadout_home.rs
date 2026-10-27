//! Loadout home dock (deck slide 2): the companion docked top-right as a
//! still picture, with Objectives, PC loadout and From Pulse beside the chat.
//! Every panel shows real state only: the open mission's checklist, this
//! machine's hardware profile and the Pulse home cards. Narrow windows drop
//! the dock so the chat keeps its column.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use grokhub_core::local_profile::{LocalProfile, Thermal};
use grokhub_core::mission::{MissionCheckItem, MissionCheckStatus, MissionView};
use grokhub_core::{UpdateCard, UpdateStatus};

use super::*;

/// The companion art slot. Swap the file to change the picture.
const COMPANION_PNG: &[u8] = include_bytes!("../../../../brand/companion.png");
/// Below this page width the dock hides and the chat gets the whole page.
/// The default 1100 px window leaves about 840 px beside the rail, so the
/// dock shows there in its compact width.
const DOCK_MIN_PAGE_W: f32 = 780.0;
const ART_H: f32 = 196.0;
const MISSION_REFRESH: Duration = Duration::from_secs(2);
const PROFILE_REFRESH: Duration = Duration::from_secs(120);
const PULSE_LINES: usize = 2;
/// Static Listening waveform, bar heights as a share of the row.
const WAVE: [f32; 9] = [0.35, 0.7, 1.0, 0.5, 0.85, 0.4, 0.95, 0.6, 0.3];

/// Dock width for the page width beside the rail, or `None` when it hides.
pub(super) fn dock_width(page_w: f32) -> Option<f32> {
    if page_w < DOCK_MIN_PAGE_W {
        None
    } else if page_w < 980.0 {
        Some(240.0)
    } else if page_w < 1300.0 {
        Some(280.0)
    } else if page_w < 1700.0 {
        Some(300.0)
    } else {
        Some(340.0)
    }
}

/// Verified and counted checklist items. Cancelled ones don't count.
pub(super) fn objective_progress(items: &[MissionCheckItem]) -> (usize, usize) {
    let counted = items
        .iter()
        .filter(|i| i.status != MissionCheckStatus::Cancelled);
    let total = counted.clone().count();
    let done = counted
        .filter(|i| i.status == MissionCheckStatus::Verified)
        .count();
    (done, total)
}

/// One PC loadout tile: a caption and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PcTile {
    pub label: &'static str,
    pub value: String,
}

fn gb(mb: u64) -> String {
    format!("{} GB", (mb + 512) / 1024)
}

/// Up to four tiles from what the probes actually read; blanks are skipped.
pub(super) fn pc_tiles(p: &LocalProfile) -> Vec<PcTile> {
    let gpu = p
        .gpus
        .iter()
        .find(|g| !g.integrated)
        .or_else(|| p.gpus.first());
    let mut tiles = Vec::new();
    if let Some(g) = gpu.filter(|g| !g.name.trim().is_empty()) {
        let value = if g.vram_mb > 0 && !g.unified {
            format!("{} · {}", g.name.trim(), gb(g.vram_mb))
        } else {
            g.name.trim().to_string()
        };
        tiles.push(PcTile {
            label: "GPU",
            value,
        });
    }
    if let Some(d) = gpu.map(|g| g.driver.trim()).filter(|d| !d.is_empty()) {
        tiles.push(PcTile {
            label: "Driver",
            value: d.to_string(),
        });
    }
    let thermal = match p.thermal {
        Thermal::Unknown => None,
        Thermal::Nominal => Some("Cool"),
        Thermal::Fair => Some("Warm"),
        Thermal::Serious => Some("Hot"),
        Thermal::Critical => Some("Critical"),
    };
    if let Some(t) = thermal {
        tiles.push(PcTile {
            label: "Thermal",
            value: t.into(),
        });
    }
    if p.ram_mb > 0 {
        tiles.push(PcTile {
            label: "RAM",
            value: gb(p.ram_mb),
        });
    }
    if p.free_disk_mb > 0 {
        tiles.push(PcTile {
            label: "Disk free",
            value: gb(p.free_disk_mb),
        });
    }
    tiles.truncate(4);
    tiles
}

/// From Pulse: the newest home cards as (title, kind · age), plus the unread count.
pub(super) fn pulse_lines(
    cards: &[UpdateCard],
    pulse: &grokhub_core::FeedPulse,
    now: u64,
) -> (Vec<(String, String)>, usize) {
    let home = grokhub_core::home_event_cards(cards, pulse, now);
    let unread = home
        .iter()
        .filter(|c| c.status == UpdateStatus::Unread)
        .count();
    let lines = home
        .iter()
        .take(PULSE_LINES)
        .map(|c| {
            let age = grokhub_core::pulse::ago_label(c.created_at, now);
            (c.title.clone(), format!("{} · {age}", c.kind.label()))
        })
        .collect();
    (lines, unread)
}

#[derive(Default)]
struct ProfileSlot {
    profile: Option<LocalProfile>,
    read_at: Option<Instant>,
    reading: bool,
}

fn profile_slot() -> &'static Mutex<ProfileSlot> {
    static SLOT: OnceLock<Mutex<ProfileSlot>> = OnceLock::new();
    SLOT.get_or_init(Default::default)
}

/// The last hardware read; starts a background re-read when it's stale.
fn pc_profile() -> Option<LocalProfile> {
    let mut slot = profile_slot().lock().unwrap_or_else(|e| e.into_inner());
    let stale = slot.read_at.is_none_or(|t| t.elapsed() >= PROFILE_REFRESH);
    if stale && !slot.reading {
        slot.reading = true;
        let _ = std::thread::Builder::new()
            .name("loadout-pc".into())
            .spawn(|| {
                let home = grokhub_core::paths::user_home()
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                let p = super::local_probe::read_profile(&home);
                let mut slot = profile_slot().lock().unwrap_or_else(|e| e.into_inner());
                slot.profile = Some(p);
                slot.read_at = Some(Instant::now());
                slot.reading = false;
            });
    }
    slot.profile.clone()
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    use crate::ui_kit::recipe::{look, Part, State};
    let l = look(Part::Card, State::Default);
    egui::Frame::NONE
        .fill(l.fill)
        .stroke(l.stroke)
        .corner_radius(l.radius)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        })
        .response
}

fn caption(text: &str) -> egui::RichText {
    egui::RichText::new(text.to_uppercase())
        .size(crate::theme::TYPE_CAPTION)
        .extra_letter_spacing(1.5)
        .color(crate::theme::muted())
}

fn header(ui: &mut egui::Ui, title: &str, right: &str) {
    ui.horizontal(|ui| {
        ui.label(caption(title));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(right.to_uppercase())
                    .size(crate::theme::TYPE_CAPTION)
                    .strong()
                    .color(crate::theme::amber()),
            );
        });
    });
    ui.add_space(crate::theme::SPACING_8);
}

fn companion_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("loadout-companion-art");
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(tex);
    }
    let rgba = image::load_from_memory(COMPANION_PNG).ok()?.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let tex = ctx.load_texture(
        "loadout-companion-art",
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

fn paint_wave(ui: &mut egui::Ui, live: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 14.0), egui::Sense::hover());
    let color = if live {
        crate::theme::amber()
    } else {
        crate::theme::muted()
    };
    let step = rect.width() / WAVE.len() as f32;
    for (i, h) in WAVE.iter().enumerate() {
        let x = rect.left() + step * (i as f32 + 0.5);
        let half = rect.height() * h / 2.0;
        ui.painter().line_segment(
            [
                egui::pos2(x, rect.center().y - half),
                egui::pos2(x, rect.center().y + half),
            ],
            egui::Stroke::new(2.0, color),
        );
    }
}

impl Cabin {
    /// Right-hand dock on the chat home. Call before `ui_chat` so the chat
    /// and its composer take the space that's left.
    pub(super) fn ui_loadout_dock(&mut self, ui: &mut egui::Ui) {
        let Some(width) = dock_width(ui.available_width()) else {
            return;
        };
        egui::Panel::right("loadout-dock")
            .exact_size(width)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = crate::theme::SPACING_12;
                        self.dock_companion(ui);
                        self.dock_objectives(ui);
                        dock_pc(ui);
                        self.dock_pulse(ui);
                    });
            });
    }

    fn dock_companion(&mut self, ui: &mut egui::Ui) {
        let name = grokhub_core::brand::companion_name();
        let live = self.voice_is_on();
        let resp = card(ui, |ui| {
            ui.label(caption(&format!("{name} · docked")));
            ui.add_space(crate::theme::SPACING_4);
            let w = ui.available_width();
            let (art, _) = ui.allocate_exact_size(egui::vec2(w, ART_H), egui::Sense::hover());
            ui.painter()
                .rect_filled(art, crate::theme::RADIUS_SM, crate::theme::void_bg());
            // A soft red glow behind the pedestal, as on the deck.
            let glow = art.center() + egui::vec2(0.0, art.height() * 0.12);
            for (r, a) in [
                (0.5, 0.06),
                (0.42, 0.08),
                (0.34, 0.1),
                (0.26, 0.12),
                (0.18, 0.14),
            ] {
                ui.painter().circle_filled(
                    glow,
                    art.height() * r,
                    crate::theme::red_dim().gamma_multiply(a),
                );
            }
            if let Some(tex) = companion_texture(ui.ctx()) {
                let [tw, th] = tex.size().map(|v| v as f32);
                let scale = (art.height() / th).min(art.width() / tw);
                let fit = egui::Rect::from_center_size(art.center(), egui::vec2(tw, th) * scale);
                ui.painter().image(
                    tex.id(),
                    fit,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 4.0, crate::theme::amber());
                ui.label(
                    egui::RichText::new("Here · tap to talk")
                        .size(crate::theme::TYPE_BODY)
                        .strong()
                        .color(crate::theme::fg()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    paint_wave(ui, live);
                    if live {
                        ui.label(caption("Listening"));
                    }
                });
            });
        });
        let click = ui
            .interact(
                resp.rect,
                ui.id().with("loadout-companion"),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("Talk to {name}"));
        if click.clicked() {
            self.listen_voice();
        }
    }

    fn dock_mission(&self, ctx: &egui::Context) -> Option<MissionView> {
        type Cached = (Instant, String, Option<MissionView>);
        let id = egui::Id::new("loadout-mission");
        let trace = self.trace_id();
        if let Some((at, t, view)) = ctx.data(|d| d.get_temp::<Cached>(id)) {
            if t == trace && at.elapsed() < MISSION_REFRESH {
                return view;
            }
        }
        let view = self.mission_view_here();
        ctx.data_mut(|d| d.insert_temp::<Cached>(id, (Instant::now(), trace, view.clone())));
        view
    }

    fn dock_objectives(&self, ui: &mut egui::Ui) {
        let mission = self.dock_mission(ui.ctx());
        card(ui, |ui| {
            let items = mission.as_ref().map_or(&[][..], |m| &m.checklist[..]);
            let (done, total) = objective_progress(items);
            let right = if total > 0 {
                format!("{done} / {total}")
            } else {
                String::new()
            };
            header(ui, "Objectives", &right);
            let Some(m) = mission.as_ref().filter(|_| total > 0) else {
                ui.label(
                    egui::RichText::new(format!(
                        "Nothing on the list yet. Ask {} to plan something.",
                        grokhub_core::brand::companion_name()
                    ))
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::muted()),
                );
                return;
            };
            if !m.title.trim().is_empty() {
                ui.label(
                    egui::RichText::new(m.title.trim())
                        .size(crate::theme::TYPE_BODY)
                        .strong()
                        .color(crate::theme::fg()),
                );
            }
            for item in m
                .checklist
                .iter()
                .filter(|i| i.status != MissionCheckStatus::Cancelled)
                .take(4)
            {
                let verified = item.status == MissionCheckStatus::Verified;
                ui.horizontal(|ui| {
                    let (mark, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    let c = mark.center();
                    let diamond = vec![
                        egui::pos2(c.x, c.y - 5.0),
                        egui::pos2(c.x + 5.0, c.y),
                        egui::pos2(c.x, c.y + 5.0),
                        egui::pos2(c.x - 5.0, c.y),
                    ];
                    let (fill, stroke) = if verified {
                        (crate::theme::amber(), egui::Stroke::NONE)
                    } else {
                        (
                            egui::Color32::TRANSPARENT,
                            egui::Stroke::new(1.0, crate::theme::amber()),
                        )
                    };
                    ui.painter()
                        .add(egui::Shape::convex_polygon(diamond, fill, stroke));
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&item.text)
                                .size(crate::theme::TYPE_BODY)
                                .color(if verified {
                                    crate::theme::muted()
                                } else {
                                    crate::theme::fg()
                                }),
                        )
                        .truncate(),
                    );
                });
            }
            let frac = done as f32 / total as f32;
            let (bar, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 3.0, crate::theme::selected());
            let mut fill = bar;
            fill.set_width(bar.width() * frac);
            ui.painter().rect_filled(fill, 3.0, crate::theme::amber());
        });
    }

    fn dock_pulse(&mut self, ui: &mut egui::Ui) {
        let (lines, unread) = pulse_lines(&self.updates, &self.cfg.feed_pulse, now_ms());
        let resp = card(ui, |ui| {
            let right = if unread > 0 {
                format!("{unread} new")
            } else {
                String::new()
            };
            header(ui, "From Pulse", &right);
            if lines.is_empty() {
                ui.label(
                    egui::RichText::new("All quiet. New finds land here.")
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::muted()),
                );
            }
            for (title, sub) in &lines {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(title)
                                .size(crate::theme::TYPE_BODY)
                                .strong()
                                .color(crate::theme::fg()),
                        )
                        .truncate(),
                    );
                    ui.label(
                        egui::RichText::new(sub)
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::muted()),
                    );
                });
            }
        });
        let open = ui
            .interact(
                resp.rect,
                ui.id().with("loadout-pulse"),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Open Pulse");
        if open.clicked() {
            self.set_nav_id("pulse");
        }
    }
}

fn dock_pc(ui: &mut egui::Ui) {
    let profile = pc_profile();
    card(ui, |ui| {
        header(ui, "PC loadout", "");
        let Some(p) = profile else {
            ui.label(
                egui::RichText::new("Reading this PC…")
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::muted()),
            );
            return;
        };
        let tiles = pc_tiles(&p);
        let gap = crate::theme::SPACING_8;
        let w = (ui.available_width() - gap) / 2.0;
        for row in tiles.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (i, t) in row.iter().enumerate() {
                    let stroke = if i == 0 && t.label == "GPU" {
                        crate::theme::amber()
                    } else {
                        crate::theme::border()
                    };
                    egui::Frame::NONE
                        .fill(crate::theme::void_bg())
                        .stroke(egui::Stroke::new(1.0, stroke))
                        .corner_radius(crate::theme::RADIUS_SM)
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            // Inner margin 8 and a 1 px stroke on each side.
                            ui.set_width(w - 18.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(caption(t.label));
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&t.value)
                                            .size(crate::theme::TYPE_LABEL)
                                            .strong()
                                            .color(crate::theme::fg()),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&t.value);
                            });
                        });
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::local_profile::{CpuInfo, Gpu, GpuVendor, PowerInfo};

    fn item(id: &str, status: MissionCheckStatus) -> MissionCheckItem {
        MissionCheckItem {
            id: id.into(),
            text: format!("step {id}"),
            status,
        }
    }

    fn profile(gpus: Vec<Gpu>) -> LocalProfile {
        LocalProfile {
            os: "linux",
            arch: "x86_64",
            gpus,
            cpu: CpuInfo::default(),
            ram_mb: 32 * 1024,
            free_ram_mb: None,
            power: PowerInfo::default(),
            thermal: Thermal::Fair,
            free_disk_mb: 420 * 1024,
        }
    }

    fn gpu(name: &str, vram_mb: u64, integrated: bool, driver: &str) -> Gpu {
        Gpu {
            vendor: GpuVendor::Other,
            name: name.into(),
            vram_mb,
            free_vram_mb: None,
            integrated,
            unified: false,
            driver: driver.into(),
        }
    }

    #[test]
    fn dock_hides_on_narrow_pages_and_grows_on_wide_ones() {
        assert_eq!(dock_width(700.0), None);
        assert_eq!(dock_width(779.0), None);
        assert_eq!(dock_width(780.0), Some(240.0));
        // The default 1100 x 720 window leaves about 840 px beside the rail.
        assert_eq!(dock_width(840.0), Some(240.0));
        assert_eq!(dock_width(979.0), Some(240.0));
        assert_eq!(dock_width(980.0), Some(280.0));
        // A 1366 px laptop leaves about 1106 px beside the rail.
        assert_eq!(dock_width(1106.0), Some(280.0));
        assert_eq!(dock_width(1660.0), Some(300.0));
        assert_eq!(dock_width(2300.0), Some(340.0));
    }

    #[test]
    fn objectives_count_verified_steps_and_skip_cancelled_ones() {
        let items = vec![
            item("a", MissionCheckStatus::Verified),
            item("b", MissionCheckStatus::Attempted),
            item("c", MissionCheckStatus::Cancelled),
            item("d", MissionCheckStatus::Verified),
            item("e", MissionCheckStatus::Planned),
        ];
        assert_eq!(objective_progress(&items), (2, 4));
        assert_eq!(objective_progress(&[]), (0, 0));
    }

    #[test]
    fn pc_tiles_show_the_discrete_gpu_and_skip_what_was_not_read() {
        let p = profile(vec![
            gpu("Intel UHD 770", 0, true, ""),
            gpu("NVIDIA GeForce RTX 4070", 12 * 1024, false, "560.35.03"),
        ]);
        let tiles: Vec<(&str, String)> = pc_tiles(&p)
            .into_iter()
            .map(|t| (t.label, t.value))
            .collect();
        assert_eq!(
            tiles,
            vec![
                ("GPU", "NVIDIA GeForce RTX 4070 · 12 GB".to_string()),
                ("Driver", "560.35.03".to_string()),
                ("Thermal", "Warm".to_string()),
                ("RAM", "32 GB".to_string()),
            ]
        );

        let mut bare = profile(vec![]);
        bare.thermal = Thermal::Unknown;
        bare.ram_mb = 0;
        let tiles: Vec<(&str, String)> = pc_tiles(&bare)
            .into_iter()
            .map(|t| (t.label, t.value))
            .collect();
        assert_eq!(tiles, vec![("Disk free", "420 GB".to_string())]);
    }

    #[test]
    fn pulse_lines_take_the_newest_two_and_count_unread() {
        let now = 10 * 3_600_000;
        let card = |id: &str, at: u64, status| UpdateCard {
            status,
            ..grokhub_core::automation_done_card(id, &format!("Card {id}"), "", at)
        };
        let cards = vec![
            card("old", now - 3 * 3_600_000, UpdateStatus::Unread),
            card("mid", now - 2 * 3_600_000, UpdateStatus::Opened),
            card("new", now - 120_000, UpdateStatus::Unread),
            card("gone", now - 60_000, UpdateStatus::Dismissed),
        ];
        let (lines, unread) = pulse_lines(&cards, &grokhub_core::FeedPulse::default(), now);
        assert_eq!(
            lines,
            vec![
                ("Card new".to_string(), "Automation · 2m ago".to_string()),
                ("Card mid".to_string(), "Automation · 2h ago".to_string()),
            ]
        );
        assert_eq!(unread, 2);
    }

    #[test]
    fn chat_home_paints_the_dock_before_the_chat() {
        let src = include_str!("mod.rs");
        let chat = src
            .split("Nav::Chat => {")
            .nth(1)
            .and_then(|s| s.split("Nav::Devices").next())
            .expect("Nav::Chat arm");
        let dock = chat
            .find("ui_loadout_dock(ui)")
            .expect("dock on the chat home");
        let page = chat.find("ui_chat(ui)").expect("chat page");
        assert!(
            dock < page,
            "the dock must take its side before the chat: {chat}"
        );
    }

    #[test]
    fn companion_art_decodes() {
        let img = image::load_from_memory(COMPANION_PNG).expect("brand/companion.png decodes");
        assert!(img.width() > 100 && img.height() > 100);
    }
}
