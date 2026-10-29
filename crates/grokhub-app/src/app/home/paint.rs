//! Paint one Home module at a given size. Missing data → clean "coming soon".

use grokhub_core::home::{lookup, DataStatus, ModuleSize};
use grokhub_core::local_profile::Thermal;
use grokhub_core::mission::MissionView;

use super::super::loadout_home::{
    caption, header, objective_progress, pc_profile, pc_score, pc_tile_grid, pc_tile_list,
    pulse_lines,
};
use super::super::mission_page::{paint_streak, up_next, up_next_tile};
use super::super::*;

/// Actions a module paint may request (Cabin applies them after the frame).
#[derive(Debug, Clone)]
pub(super) enum ModuleAction {
    OpenChat(String),
    Nav(&'static str),
}

pub(super) fn paint_module(
    cabin: &mut Cabin,
    ui: &mut egui::Ui,
    module_id: &str,
    size: ModuleSize,
) -> Vec<ModuleAction> {
    let mut actions = Vec::new();
    let entry = lookup(module_id);
    let name = entry.map(|e| e.name).unwrap_or(module_id);
    let data = entry.map(|e| e.data).unwrap_or(DataStatus::New);
    let purpose = entry.map(|e| e.purpose).unwrap_or("");

    if !entry.map(|e| e.supports(size)).unwrap_or(false) {
        coming_soon_card(ui, name, "This size is not offered");
        return actions;
    }

    match module_id {
        "objectives" => paint_objectives(cabin, ui, size, &mut actions),
        "up_next" => paint_up_next(cabin, ui, size),
        "pulse" => paint_pulse(cabin, ui, size, &mut actions),
        "pc_loadout" => paint_pc_loadout(ui, size, &mut actions),
        "streak" => paint_streak_mod(cabin, ui, size),
        "usage" => paint_usage(ui, size, &mut actions),
        "storage" => paint_storage(ui, size, &mut actions),
        "game_mode" => paint_game_mode(ui),
        "connection" => paint_connection(ui, size, &mut actions),
        "workboard" => paint_nav_stub(ui, size, name, "Open Workboard", "workboard", &mut actions),
        "recent_chats" => paint_nav_stub(ui, size, name, "Open Chat", "chat", &mut actions),
        _ if data == DataStatus::New => coming_soon_card(ui, name, purpose),
        _ => ready_stub(ui, size, name, purpose, data),
    }
    actions
}

/// A module card that fills its slot, so stacked slots read as one packed grid
/// instead of a short card with empty space under it.
fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    super::super::loadout_home::card(ui, |ui| {
        ui.set_min_height(ui.available_height());
        add(ui);
    });
}

/// Shown under empty streak tiles so seven grey boxes explain themselves.
const STREAK_EMPTY: &str = "Finish an objective today to start your streak.";

fn streak_hint(ui: &mut egui::Ui, days: usize) {
    if days == 0 {
        ui.add_space(4.0);
        ui.label(
            RichText::new(STREAK_EMPTY)
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
    }
}

fn coming_soon_card(ui: &mut egui::Ui, name: &str, detail: &str) {
    card(ui, |ui| {
        header(ui, false, name, "SOON", crate::theme::muted());
        ui.label(
            RichText::new(if detail.is_empty() {
                "Coming soon"
            } else {
                detail
            })
            .size(crate::theme::TYPE_BODY)
            .color(crate::theme::muted()),
        );
    });
}

fn ready_stub(ui: &mut egui::Ui, size: ModuleSize, name: &str, purpose: &str, data: DataStatus) {
    card(ui, |ui| {
        let tag = if size == ModuleSize::Small {
            data.label()
        } else {
            ""
        };
        header(
            ui,
            false,
            name,
            tag,
            if data == DataStatus::Partial {
                crate::theme::amber()
            } else {
                crate::theme::muted()
            },
        );
        if size == ModuleSize::Small {
            ui.label(
                RichText::new(data.label().to_uppercase())
                    .size(crate::theme::TYPE_TITLE)
                    .strong()
                    .color(crate::theme::fg()),
            );
        }
        ui.label(
            RichText::new(purpose)
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
    });
}

fn paint_nav_stub(
    ui: &mut egui::Ui,
    size: ModuleSize,
    name: &str,
    cta: &str,
    nav: &'static str,
    actions: &mut Vec<ModuleAction>,
) {
    card(ui, |ui| {
        header(ui, false, name, "", crate::theme::fg());
        if size != ModuleSize::Small {
            if crate::cards::ghost_pill(ui, cta) {
                actions.push(ModuleAction::Nav(nav));
            }
        } else if ui
            .add(
                egui::Label::new(RichText::new(cta).color(crate::theme::amber()))
                    .sense(egui::Sense::click()),
            )
            .clicked()
        {
            actions.push(ModuleAction::Nav(nav));
        }
    });
}

fn paint_objectives(
    cabin: &mut Cabin,
    ui: &mut egui::Ui,
    size: ModuleSize,
    actions: &mut Vec<ModuleAction>,
) {
    let mission = cabin.dock_mission(ui.ctx());
    let now = now_ms();
    let streak = cabin.mission_streak(ui.ctx(), now);
    card(ui, |ui| {
        let items = mission.as_ref().map_or(&[][..], |m| &m.checklist[..]);
        let (done, total) = objective_progress(items);
        let right = if total > 0 {
            format!("{done} / {total}")
        } else {
            String::new()
        };
        header(
            ui,
            true,
            "Today's objectives",
            &right,
            crate::theme::amber(),
        );
        match size {
            ModuleSize::Small => {
                if total == 0 {
                    ui.label(
                        RichText::new("No objectives")
                            .size(crate::theme::TYPE_BODY)
                            .color(crate::theme::muted()),
                    );
                } else {
                    ui.label(
                        RichText::new(format!("{done}/{total}"))
                            .size(crate::theme::TYPE_TITLE)
                            .strong()
                            .color(crate::theme::fg()),
                    );
                }
            }
            ModuleSize::Medium | ModuleSize::Large => {
                // Above the list: the slot clips, so a long checklist can't hide it.
                if crate::cards::ghost_pill(ui, "Details") {
                    actions.push(ModuleAction::Nav("objectives"));
                }
                ui.add_space(4.0);
                let cap = if size == ModuleSize::Medium {
                    3
                } else {
                    usize::MAX
                };
                paint_objective_list(ui, mission.as_ref(), cap);
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                let days = streak.iter().filter(|d| **d).count();
                ui.horizontal(|ui| {
                    ui.label(caption("Weekly streak"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let unit = if days == 1 { "DAY" } else { "DAYS" };
                        ui.label(
                            RichText::new(format!("{days} {unit}"))
                                .size(crate::theme::TYPE_CAPTION)
                                .extra_letter_spacing(1.5)
                                .strong()
                                .color(crate::theme::amber()),
                        );
                    });
                });
                paint_streak(ui, &streak);
                streak_hint(ui, days);
            }
        }
    });
}

fn paint_objective_list(ui: &mut egui::Ui, mission: Option<&MissionView>, cap: usize) {
    let Some(m) = mission else {
        ui.label(
            RichText::new(format!(
                "Nothing on the list yet. Ask {} to plan something.",
                grokhub_core::brand::companion_name()
            ))
            .size(crate::theme::TYPE_BODY)
            .color(crate::theme::muted()),
        );
        return;
    };
    let (_, total) = objective_progress(&m.checklist);
    if total == 0 {
        ui.label(
            RichText::new("No objectives · Assign one below")
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::muted()),
        );
        return;
    }
    let mut shown = 0;
    for item in &m.checklist {
        if matches!(
            item.status,
            grokhub_core::mission::MissionCheckStatus::Cancelled
        ) {
            continue;
        }
        if shown >= cap {
            break;
        }
        ui.label(
            RichText::new(item.text.trim())
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::fg()),
        );
        shown += 1;
    }
}

fn paint_up_next(cabin: &mut Cabin, ui: &mut egui::Ui, size: ModuleSize) {
    let now = now_ms();
    let (_, games) = cabin.steam_library(now);
    let next = up_next(games);
    // Slot height before the card eats it — Wide S column is tall; Medium/Narrow strips are short.
    let slot_h = ui.available_height();
    let mut open = None;
    card(ui, |ui| {
        header(
            ui,
            false,
            "Up next",
            if next.is_empty() { "" } else { "QUEUED" },
            crate::theme::amber(),
        );
        if next.is_empty() {
            ui.label(
                RichText::new("Games you play with Steam show up here.")
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::muted()),
            );
            return;
        }
        let w = ui.available_width();
        let gap = 8.0;
        let n = next.len().max(1) as f32;
        // Slide 09: cover art, not text names. Stack when the slot is tall
        // (Wide right column / Medium+); strip horizontally when short.
        let stack = match size {
            ModuleSize::Medium | ModuleSize::Large => true,
            ModuleSize::Small => slot_h > 220.0,
        };
        if stack {
            let tile_h = ((ui.available_height() - gap * (n - 1.0)) / n).clamp(72.0, 150.0);
            for g in &next {
                if up_next_tile(ui, g, egui::vec2(w, tile_h)) {
                    open = Some(g.app_id);
                }
                ui.add_space(gap);
            }
        } else {
            let tile_w = ((w - gap * (n - 1.0)) / n).max(48.0);
            let tile_h = ui.available_height().clamp(56.0, 100.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for g in &next {
                    if up_next_tile(ui, g, egui::vec2(tile_w, tile_h)) {
                        open = Some(g.app_id);
                    }
                }
            });
        }
    });
    if let Some(id) = open {
        cabin.show_in_library(ui.ctx(), id);
    }
}

fn paint_pulse(
    cabin: &mut Cabin,
    ui: &mut egui::Ui,
    size: ModuleSize,
    actions: &mut Vec<ModuleAction>,
) {
    let (lines, _) = pulse_lines(&cabin.updates, &cabin.cfg.feed_pulse, now_ms());
    card(ui, |ui| {
        header(ui, false, "Pulse · For You", "", crate::theme::fg());
        if lines.is_empty() {
            ui.label(
                RichText::new("Pulse is warming up")
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::muted()),
            );
        } else {
            let n = match size {
                ModuleSize::Small => 1,
                ModuleSize::Medium => 2,
                ModuleSize::Large => 4,
            };
            for (title, _) in lines.iter().take(n) {
                ui.label(
                    RichText::new(title)
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::fg()),
                );
            }
        }
        if size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Open Pulse") {
            actions.push(ModuleAction::Nav("pulse"));
        }
    });
}

fn paint_pc_loadout(ui: &mut egui::Ui, size: ModuleSize, actions: &mut Vec<ModuleAction>) {
    let profile = pc_profile();
    card(ui, |ui| {
        let score = profile
            .as_ref()
            .and_then(pc_score)
            .map(|n| format!("{n}%"))
            .unwrap_or_default();
        match size {
            ModuleSize::Small => {
                header(ui, false, "GPU temp", "", crate::theme::fg());
                let temp: String = profile
                    .as_ref()
                    .map(|p| match p.thermal {
                        Thermal::Unknown => "—",
                        Thermal::Nominal => "OK",
                        Thermal::Fair => "Warm",
                        Thermal::Serious => "Hot",
                        Thermal::Critical => "Critical",
                    })
                    .unwrap_or("—")
                    .to_string();
                ui.label(
                    RichText::new(temp)
                        .size(crate::theme::TYPE_TITLE)
                        .strong()
                        .color(crate::theme::fg()),
                );
                ui.label(
                    RichText::new(if score.is_empty() {
                        "Sensors unavailable"
                    } else {
                        "Thermal"
                    })
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
                );
            }
            ModuleSize::Medium | ModuleSize::Large => {
                header(ui, false, "PC loadout", &score, crate::theme::fg());
                match profile {
                    None => {
                        ui.label(
                            RichText::new("Reading this PC…")
                                .size(crate::theme::TYPE_BODY)
                                .color(crate::theme::muted()),
                        );
                    }
                    Some(p) => pc_tile_grid(ui, &pc_tile_list(&p)),
                }
                // Slide 09 shows the actions under the tiles at Medium too;
                // skip them only when the slot has no room left.
                if size == ModuleSize::Large || ui.available_height() >= 48.0 {
                    ui.add_space(8.0);
                    let btn = egui::vec2(140.0, 40.0);
                    ui.horizontal(|ui| {
                        if crate::ui_kit::recipe::button(ui, None, "TUNE-UP", true, btn, true)
                            .clicked()
                        {
                            actions.push(ModuleAction::OpenChat(
                                "Check this PC and suggest a tune-up.".into(),
                            ));
                        }
                        if crate::ui_kit::recipe::button(ui, None, "DETAILS", false, btn, true)
                            .clicked()
                        {
                            actions.push(ModuleAction::Nav("devices"));
                        }
                    });
                }
            }
        }
    });
}

fn paint_streak_mod(cabin: &mut Cabin, ui: &mut egui::Ui, size: ModuleSize) {
    let now = now_ms();
    let streak = cabin.mission_streak(ui.ctx(), now);
    let days = streak.iter().filter(|d| **d).count();
    card(ui, |ui| {
        header(ui, false, "Streak", "", crate::theme::amber());
        ui.label(
            RichText::new(format!("{days}"))
                .size(crate::theme::TYPE_TITLE)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.label(
            RichText::new(if days == 1 { "DAY" } else { "DAYS" })
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
        if size != ModuleSize::Small {
            paint_streak(ui, &streak);
            streak_hint(ui, days);
        }
    });
}

fn paint_usage(ui: &mut egui::Ui, size: ModuleSize, actions: &mut Vec<ModuleAction>) {
    card(ui, |ui| {
        header(ui, false, "AI usage", "", crate::theme::fg());
        ui.label(
            RichText::new("—")
                .size(crate::theme::TYPE_TITLE)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.label(
            RichText::new("No usage yet today")
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
        if size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Budget") {
            actions.push(ModuleAction::Nav("settings"));
        }
    });
}

fn paint_storage(ui: &mut egui::Ui, size: ModuleSize, actions: &mut Vec<ModuleAction>) {
    let profile = pc_profile();
    let free = profile
        .as_ref()
        .filter(|p| p.free_disk_mb > 0)
        .map(|p| format!("{:.0} GB", p.free_disk_mb as f32 / 1024.0))
        .unwrap_or_else(|| "—".into());
    card(ui, |ui| {
        header(ui, false, "Storage", "", crate::theme::fg());
        ui.label(
            RichText::new(format!("FREE {free}"))
                .size(if size == ModuleSize::Small {
                    crate::theme::TYPE_BODY
                } else {
                    crate::theme::TYPE_TITLE
                })
                .strong()
                .color(crate::theme::fg()),
        );
        if size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Free up storage") {
            actions.push(ModuleAction::OpenChat("Free up storage on this PC.".into()));
        }
    });
}

fn paint_game_mode(ui: &mut egui::Ui) {
    card(ui, |ui| {
        header(ui, false, "Game Mode", "OFF", crate::theme::muted());
        ui.label(
            RichText::new("No game running")
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::muted()),
        );
    });
}

fn paint_connection(ui: &mut egui::Ui, size: ModuleSize, actions: &mut Vec<ModuleAction>) {
    card(ui, |ui| {
        header(ui, false, "Connection", "", crate::theme::fg());
        ui.label(
            RichText::new("0 B")
                .size(crate::theme::TYPE_TITLE)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.label(
            RichText::new("SENT TO CLOUD · LOCAL ONLY")
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
        if size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Privacy") {
            actions.push(ModuleAction::Nav("settings"));
        }
    });
}
