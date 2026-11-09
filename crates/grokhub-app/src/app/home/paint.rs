//! Paint one Home module at a given size. Missing data → clean "coming soon".

use grokhub_core::home::text::{
    connection_text, game_mode_text, gpu_temp_text, storage_rows, storage_text, updates_text,
    usage_text,
};
use grokhub_core::home::{
    fit, lookup, ModuleSize, TileAction, TileState, TileText, TileView, Tone,
};
use grokhub_core::mission::MissionView;

use super::super::loadout_home::{
    caption, header, objective_progress, paint_weeks, pc_profile, pc_reading, pc_tile_grid,
    pc_tile_list, pulse_lines,
};
use super::super::mission_page::{goal_summary, paint_streak, up_next, up_next_tile};
use super::super::*;
use super::fps::FpsState;

/// Actions a module paint may request (Cabin applies them after the frame).
#[derive(Debug, Clone)]
pub(super) enum ModuleAction {
    OpenChat(String),
    Nav(&'static str),
    /// Settings, opened on this section (a bare `Nav("settings")` lands on Account).
    Settings(SettingsSec),
    /// A shared-model tile's button, row or tap.
    Tile(TileAction),
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
        "usage" => paint_usage(cabin, ui, size, &mut actions),
        "storage" => paint_storage(ui, size, &mut actions),
        "game_mode" => paint_game_mode(cabin, ui, size, &mut actions),
        "connection" => paint_connection(ui, size, &mut actions),
        "updates" => paint_updates(cabin, ui, size, &mut actions),
        "speeds" => paint_speeds(cabin, ui, size, &mut actions),
        "frame_rate" => paint_frame_rate(ui, size),
        "workboard" => paint_workboard(cabin, ui, size, name, &mut actions),
        _ => match super::views::desktop_view(cabin, module_id) {
            Some(view) => paint_view(ui, name, &view, size, &mut actions),
            None => coming_soon_card(ui, name, ""),
        },
    }
    actions
}

/// Modules with a painter of their own (the arms of `paint_module`). Every
/// other catalog module draws from the shared tile model in `views.rs`.
#[cfg(test)]
pub(in crate::app) const LIVE_MODULES: [&str; 13] = [
    "objectives",
    "up_next",
    "pulse",
    "pc_loadout",
    "streak",
    "usage",
    "storage",
    "game_mode",
    "connection",
    "updates",
    "speeds",
    "frame_rate",
    "workboard",
];

/// One shared-model tile: the big line and caption, then progress, rows and
/// buttons as the size fits. A Small tile is one tap: its own, else its first
/// button's.
fn paint_view(
    ui: &mut egui::Ui,
    name: &str,
    view: &TileView,
    size: ModuleSize,
    actions: &mut Vec<ModuleAction>,
) {
    let room = fit(size);
    let quiet = matches!(view.state, TileState::Unsupported);
    let resp = card(ui, |ui| {
        header(
            ui,
            false,
            name,
            view.tag.as_deref().unwrap_or(""),
            crate::theme::muted(),
        );
        paint_tile_text(ui, &view.text, size);
        if size != ModuleSize::Small {
            if let Some(pct) = view.progress {
                ui.add_space(4.0);
                ui.add(
                    egui::ProgressBar::new(f32::from(pct) / 100.0)
                        .desired_height(4.0)
                        .fill(crate::theme::amber()),
                );
            }
        }
        if room.rows > 0 && !view.rows.is_empty() {
            ui.add_space(6.0);
            for row in view.rows.iter().take(room.rows) {
                let resp = ui
                    .horizontal(|ui| {
                        ui.label(
                            RichText::new(&row.label)
                                .size(crate::theme::TYPE_BODY)
                                .color(crate::theme::fg()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(&row.value)
                                    .size(crate::theme::TYPE_CAPTION)
                                    .color(tone_color(row.tone)),
                            );
                        });
                    })
                    .response;
                if let Some(tap) = &row.tap {
                    if ui
                        .interact(
                            resp.rect,
                            ui.id().with((name, &row.label)),
                            egui::Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        actions.push(ModuleAction::Tile(tap.clone()));
                    }
                }
            }
        }
        if room.buttons > 0 && !view.buttons.is_empty() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for b in view.buttons.iter().take(room.buttons) {
                    let pressed = if b.primary {
                        crate::cards::white_pill(ui, &b.label)
                    } else {
                        crate::cards::ghost_pill(ui, &b.label)
                    };
                    if pressed {
                        actions.push(ModuleAction::Tile(b.action.clone()));
                    }
                }
            });
        }
    });
    let tap = view
        .tap
        .clone()
        .or_else(|| view.buttons.first().map(|b| b.action.clone()));
    if let (Some(tap), false) = (tap, quiet) {
        let tip = view.buttons.first().map_or(name, |b| b.label.as_str());
        if card_clicked(ui, &resp, name, tip) && actions.is_empty() {
            actions.push(ModuleAction::Tile(tap));
        }
    }
}

/// A module card that fills its slot, so stacked slots read as one packed grid
/// instead of a short card with empty space under it.
fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    super::super::loadout_home::card(ui, |ui| {
        ui.set_min_height(ui.available_height());
        add(ui);
    })
}

/// The whole card is a button too, so a Small tile with no room for a pill
/// still goes somewhere. Pills inside keep their own clicks.
fn card_clicked(ui: &mut egui::Ui, resp: &egui::Response, salt: &str, tip: &str) -> bool {
    ui.interact(resp.rect, ui.id().with(salt), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tip)
        .clicked()
}

fn tone_color(t: Tone) -> egui::Color32 {
    match t {
        Tone::Normal => crate::theme::fg(),
        Tone::Warn => crate::theme::amber(),
        Tone::Alert => crate::theme::signal_red(),
        Tone::Quiet => crate::theme::muted(),
    }
}

fn paint_tile_text(ui: &mut egui::Ui, t: &TileText, size: ModuleSize) {
    ui.label(
        RichText::new(&t.big)
            .size(crate::theme::TYPE_TITLE)
            .strong()
            .color(tone_color(t.tone)),
    );
    ui.label(
        RichText::new(&t.caption)
            .size(crate::theme::TYPE_CAPTION)
            .color(crate::theme::muted()),
    );
    if size != ModuleSize::Small {
        if let Some(d) = &t.detail {
            ui.add_space(4.0);
            ui.label(
                RichText::new(d)
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::fg()),
            );
        }
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

fn paint_workboard(
    cabin: &Cabin,
    ui: &mut egui::Ui,
    size: ModuleSize,
    name: &str,
    actions: &mut Vec<ModuleAction>,
) {
    let n = super::super::board_ui::board_counts(&cabin.board);
    let resp = card(ui, |ui| {
        header(ui, false, name, "", crate::theme::fg());
        let rows = [
            ("Doing", n.doing, crate::theme::fg()),
            ("Interrupted", n.interrupted, crate::theme::amber()),
            ("Needs you", n.needs_you, crate::theme::signal_red()),
        ];
        if size == ModuleSize::Small {
            let line = rows
                .iter()
                .map(|(label, count, _)| format!("{count} {}", label.to_lowercase()))
                .collect::<Vec<_>>()
                .join(" · ");
            ui.label(
                RichText::new(line)
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::fg()),
            );
            return;
        }
        for (label, count, tone) in rows {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(count.to_string())
                        .size(crate::theme::TYPE_BODY)
                        .strong()
                        .color(if count > 0 {
                            tone
                        } else {
                            crate::theme::muted()
                        }),
                );
                ui.label(
                    RichText::new(label)
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::muted()),
                );
            });
        }
        if crate::cards::ghost_pill(ui, "Open Workboard") {
            actions.push(ModuleAction::Nav("workboard"));
        }
    });
    if size == ModuleSize::Small && card_clicked(ui, &resp, "workboard-tile", "Open Workboard") {
        actions.push(ModuleAction::Nav("workboard"));
    }
}

fn paint_objectives(
    cabin: &mut Cabin,
    ui: &mut egui::Ui,
    size: ModuleSize,
    actions: &mut Vec<ModuleAction>,
) {
    let mission = cabin.dock_mission(ui.ctx());
    let week = cabin.dock_xp(ui.ctx()).week;
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
                // The goal closest to done, over the chat's steps.
                let goals = cabin.dock_goals(ui.ctx());
                let top = grokhub_core::objectives::today_picks(&goals, now_ms())
                    .into_iter()
                    .find_map(|p| goals.goals.iter().find(|g| g.id == p.id()).cloned());
                if let Some(g) = top {
                    goal_summary(ui, &g);
                    ui.add_space(8.0);
                }
                let cap = if size == ModuleSize::Medium {
                    3
                } else {
                    usize::MAX
                };
                paint_objective_list(ui, mission.as_ref(), cap);
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                paint_weeks(ui, &week);
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
    super::pc_sheet::show_pc_sheet(ui.ctx(), profile.as_ref());
    let resp = card(ui, |ui| {
        let reading = profile.as_ref().and_then(pc_reading).unwrap_or_default();
        match size {
            ModuleSize::Small => {
                header(ui, false, "GPU temp", "", crate::theme::fg());
                let thermal = profile.as_ref().map(|p| p.thermal);
                let t = gpu_temp_text(super::live::live_facts().gpu_temp_c, thermal);
                paint_tile_text(ui, &t, size);
            }
            ModuleSize::Medium | ModuleSize::Large => {
                header(ui, false, "PC loadout", &reading, crate::theme::fg());
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
                        if crate::ui_kit::recipe::button(ui, None, "Tune-up", true, btn, true)
                            .clicked()
                        {
                            actions.push(ModuleAction::OpenChat(
                                grokhub_core::home::TUNE_UP_ASK.into(),
                            ));
                        }
                        if crate::ui_kit::recipe::button(ui, None, "Details", false, btn, true)
                            .clicked()
                        {
                            super::pc_sheet::open_pc_sheet(ui.ctx());
                        }
                    });
                }
            }
        }
    });
    // Medium and Large have their own buttons; the Small tile opens the sheet.
    if size == ModuleSize::Small && card_clicked(ui, &resp, "home-pc", "This PC's hardware") {
        super::pc_sheet::open_pc_sheet(ui.ctx());
    }
}

fn paint_streak_mod(cabin: &mut Cabin, ui: &mut egui::Ui, size: ModuleSize) {
    let week = cabin.dock_xp(ui.ctx()).week;
    card(ui, |ui| {
        header(ui, false, "Weeks in a row", "", crate::theme::amber());
        ui.label(
            RichText::new(week.in_a_row.to_string())
                .size(crate::theme::TYPE_TITLE)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.label(
            RichText::new(grokhub_core::xp::week_target_line(&week))
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
        if size != ModuleSize::Small {
            ui.add_space(4.0);
            paint_streak(ui, &week);
        }
    });
}

fn paint_usage(
    cabin: &Cabin,
    ui: &mut egui::Ui,
    size: ModuleSize,
    actions: &mut Vec<ModuleAction>,
) {
    let t = usage_text(
        grokhub_core::tokens_today(&cabin.usage),
        cabin.cfg.daily_token_budget,
        cabin.harness.router.budget.week,
        cabin.cfg.weekly_spend_cap_usd,
        cabin.harness.router.budget.cache_pct,
        grokhub_agent::cache_watch::FLOOR_PCT,
    );
    let resp = card(ui, |ui| {
        header(ui, false, "AI usage", "", crate::theme::fg());
        paint_tile_text(ui, &t, size);
        // The daily token budget lives on AI & chat → AI models.
        if size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Budget") {
            actions.push(ModuleAction::Settings(SettingsSec::AiModels));
        }
    });
    if card_clicked(ui, &resp, "home-usage", "Daily token budget") {
        actions.push(ModuleAction::Settings(SettingsSec::AiModels));
    }
}

fn paint_storage(ui: &mut egui::Ui, size: ModuleSize, actions: &mut Vec<ModuleAction>) {
    let facts = super::live::live_facts();
    let fallback = pc_profile().map(|p| p.free_disk_mb);
    card(ui, |ui| {
        header(ui, false, "Storage", "", crate::theme::fg());
        let rows = storage_rows(&facts.drives);
        if size == ModuleSize::Small || rows.len() < 2 {
            paint_tile_text(ui, &storage_text(&facts.drives, fallback), size);
        } else {
            let cap = if size == ModuleSize::Medium { 3 } else { 6 };
            for row in rows.iter().take(cap) {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&row.label)
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::muted()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(&row.value)
                                .size(crate::theme::TYPE_BODY)
                                .strong()
                                .color(tone_color(row.tone)),
                        );
                    });
                });
            }
            if rows.len() > cap {
                ui.label(caption(&format!("+{} more", rows.len() - cap)));
            }
        }
        if size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Free up storage") {
            actions.push(ModuleAction::OpenChat(
                grokhub_core::home::FREE_STORAGE_ASK.into(),
            ));
        }
    });
}

fn paint_updates(
    cabin: &mut Cabin,
    ui: &mut egui::Ui,
    size: ModuleSize,
    actions: &mut Vec<ModuleAction>,
) {
    let facts = super::updates::update_facts();
    let loadout = cabin.update_pending_now() != UpdatePending::None;
    let driver = pc_profile().and_then(|p| p.main_gpu().map(|g| g.driver.clone()));
    let t = updates_text(loadout, &facts, driver.as_deref());
    let resp = card(ui, |ui| {
        header(ui, false, "Updates & Drivers", "", crate::theme::fg());
        paint_tile_text(ui, &t, size);
        if size != ModuleSize::Small {
            ui.horizontal(|ui| {
                if crate::cards::ghost_pill(ui, "Loadout update") {
                    actions.push(ModuleAction::Settings(SettingsSec::Update));
                }
                if !facts.steam.is_empty() && crate::cards::ghost_pill(ui, "Library") {
                    actions.push(ModuleAction::Nav("library"));
                }
            });
        }
    });
    if card_clicked(ui, &resp, "home-updates", "Loadout updates") {
        actions.push(ModuleAction::Settings(SettingsSec::Update));
    }
}

/// The Speeds tile in words, from the opt-in internet log. Nothing is
/// measured for Home itself: ping comes from the log's once-a-minute check,
/// download speed only from a test someone tapped.
pub(super) fn speeds_text(f: &super::super::net_log_ui::NetTileFacts, now: u64) -> TileText {
    if !f.on {
        return TileText {
            big: "Off".into(),
            caption: "Turn on the internet log to see ping".into(),
            detail: Some("Nothing is sent until you do".into()),
            tone: Tone::Quiet,
        };
    }
    if f.down {
        return TileText {
            big: "Offline".into(),
            caption: "Neither public DNS server answered".into(),
            detail: None,
            tone: Tone::Alert,
        };
    }
    let (big, tone) = match f.ping_ms {
        Some(ms) => (
            format!("{ms} ms"),
            if ms >= grokhub_core::net_log::SLOW_MS {
                Tone::Warn
            } else {
                Tone::Normal
            },
        ),
        None => ("—".into(), Tone::Quiet),
    };
    let caption = match (f.ping_ms, f.jitter_ms) {
        (Some(_), Some(j)) => format!("Ping · jitter {j} ms"),
        (Some(_), None) => "Ping".into(),
        _ => "First check within a minute".into(),
    };
    let detail = if f.testing {
        Some("Testing download speed…".into())
    } else {
        f.last_speed.map(|(mbps, at)| {
            format!(
                "Down {mbps} Mbps · {}",
                grokhub_core::pulse::ago_label(at, now)
            )
        })
    };
    TileText {
        big,
        caption,
        detail,
        tone,
    }
}

fn paint_speeds(
    cabin: &mut Cabin,
    ui: &mut egui::Ui,
    size: ModuleSize,
    actions: &mut Vec<ModuleAction>,
) {
    let f = cabin.net_tile_facts();
    let t = speeds_text(&f, now_ms());
    let mut test = false;
    let resp = card(ui, |ui| {
        header(ui, false, "Speeds", "", crate::theme::fg());
        paint_tile_text(ui, &t, size);
        if size != ModuleSize::Small {
            if !f.on {
                // The internet log row is on Your PC → App & extras.
                if crate::cards::ghost_pill(ui, "Turn on in Settings") {
                    actions.push(ModuleAction::Settings(SettingsSec::Extras));
                }
            } else if !f.testing && !f.down && crate::cards::ghost_pill(ui, "Test speed") {
                test = true;
            }
        }
    });
    if test {
        cabin.start_speed_test();
    }
    if card_clicked(ui, &resp, "home-speeds", "Internet log settings") {
        actions.push(ModuleAction::Settings(SettingsSec::Extras));
    }
}

/// The Frame Rate tile. On Linux the kernel counts the frames each screen
/// shows (display flips); on Windows the graphics kernel's present events give
/// the busiest app's frame rate. Both entirely outside the game. Elsewhere it
/// says plainly that Loadout doesn't read games and points to the safe
/// built-in counters. `reading` is `None` where there is no reader;
/// `presenter` is the Windows exe the rate came from.
pub(super) fn frame_rate_text(
    reading: Option<FpsState>,
    windows: bool,
    presenter: Option<&str>,
) -> TileText {
    use grokhub_agent::indexers::display_fps::FpsBlock;
    use grokhub_core::home::text::FpsRead;
    let read = reading.map(|state| match state {
        FpsState::Starting => FpsRead::Starting,
        FpsState::Rate(r) => FpsRead::Rate(r),
        FpsState::Blocked(FpsBlock::NeedsPermission) => FpsRead::NeedsSetup,
        FpsState::Blocked(_) => FpsRead::NoTrace,
    });
    grokhub_core::home::text::frame_rate_text(read, windows, presenter)
}

/// The Windows account the setup adds to Performance Log Users.
fn windows_account() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(d) if !d.is_empty() => format!("{d}\\{user}"),
        _ => user,
    }
}

fn paint_frame_rate(ui: &mut egui::Ui, size: ModuleSize) {
    use grokhub_agent::indexers::display_fps as fps;
    let windows = cfg!(windows);
    let reading = (cfg!(target_os = "linux") || windows).then(super::fps::display_fps);
    let presenter = if windows {
        super::fps::presenter()
    } else {
        None
    };
    let t = frame_rate_text(reading, windows, presenter.as_deref());
    let needs_setup = matches!(
        reading,
        Some(FpsState::Blocked(fps::FpsBlock::NeedsPermission))
    );
    card(ui, |ui| {
        header(ui, false, "Frame Rate", "", crate::theme::muted());
        paint_tile_text(ui, &t, size);
        if needs_setup && size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Copy setup") {
            let setup = if windows {
                fps::windows_setup(&windows_account())
            } else {
                let exe = std::env::current_exe()
                    .map(|p| fps::probe_source(&p).to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "/usr/bin/grokhub".into());
                fps::linux_setup(&exe)
            };
            ui.ctx().copy_text(setup);
        }
    })
    .on_hover_text(if windows {
        format!(
            "Loadout counts frames from Windows' own graphics event log (the one PresentMon reads), never from inside the game, so anti-cheat has nothing to see. It shows the busiest app drawing frames, which is your game while you play. The setup (run once in an admin PowerShell, then sign out and back in) adds your account to Windows' built-in Performance Log Users group, which lets you start event traces without admin. Undo: {}",
            fps::windows_undo(&windows_account())
        )
    } else if reading.is_some() {
        "Loadout counts the frames your screen shows from the kernel's display driver, never from inside the game, so anti-cheat has nothing to see. It matches the game's FPS when the game is what's moving on that screen, and tops out at your refresh rate. The setup (needs sudo) installs a frame probe that only your user group can run and gives that one file the kernel's perf-counter permission. The probe opens the frame counters and then drops it, Loadout itself never runs with it, and the system-wide perf setting stays as it is. Undo: sudo rm -f /usr/local/lib/loadout/fps-probe /etc/tmpfiles.d/loadout-fps.conf".to_owned()
    } else {
        "Reading FPS from inside a game can trip anti-cheat (Vanguard, EAC, BattlEye), so Loadout leaves games alone.".to_owned()
    });
    if reading.is_some() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(1));
    }
}

fn paint_game_mode(
    cabin: &mut Cabin,
    ui: &mut egui::Ui,
    size: ModuleSize,
    actions: &mut Vec<ModuleAction>,
) {
    // Process names only (game_guard): no handle to, or read of, any game.
    let guard = grokhub_desktop::game_guard::peek();
    let game = cabin.game_running().and_then(|id| {
        let (_, games) = cabin.steam_library(now_ms());
        games
            .iter()
            .find(|g| g.app_id == id)
            .map(|g| g.name.clone())
    });
    let local_on = grokhub_agent::route::local::switched_on();
    let paused = local_on
        .then(grokhub_agent::route::local_gov::paused)
        .unwrap_or(false);
    let t = game_mode_text(guard, game.as_deref(), paused);
    let on = guard.gaming || guard.anticheat.is_some();
    let resp = card(ui, |ui| {
        header(
            ui,
            false,
            "Game Mode",
            if on { "ON" } else { "OFF" },
            if on {
                crate::theme::amber()
            } else {
                crate::theme::muted()
            },
        );
        paint_tile_text(ui, &t, size);
        if size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Library") {
            actions.push(ModuleAction::Nav("library"));
        }
    });
    if card_clicked(ui, &resp, "home-game-mode", "Open Library") {
        actions.push(ModuleAction::Nav("library"));
    }
}

fn paint_connection(ui: &mut egui::Ui, size: ModuleSize, actions: &mut Vec<ModuleAction>) {
    let facts = super::live::live_facts();
    let t = connection_text(
        grokhub_agent::route::connectivity::state().link(),
        grokhub_agent::route::local::switched_on(),
        &facts.sends(),
    );
    let resp = card(ui, |ui| {
        header(ui, false, "Connection", "", crate::theme::fg());
        paint_tile_text(ui, &t, size);
        if size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Privacy") {
            actions.push(ModuleAction::Settings(SettingsSec::Privacy));
        }
    });
    if card_clicked(ui, &resp, "home-connection", "What left this PC") {
        actions.push(ModuleAction::Settings(SettingsSec::Privacy));
    }
}
