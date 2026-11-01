//! Paint one Home module at a given size. Missing data → clean "coming soon".

use grokhub_agent::indexers::system_state::DiskUse;
use grokhub_core::home::{lookup, ModuleSize};
use grokhub_core::local_profile::Thermal;
use grokhub_core::mission::MissionView;

use super::super::loadout_home::{
    caption, header, objective_progress, pc_profile, pc_reading, pc_tile_grid, pc_tile_list,
    pulse_lines,
};
use super::super::mission_page::{paint_streak, up_next, up_next_tile};
use super::super::*;
use super::fps::FpsState;

/// Actions a module paint may request (Cabin applies them after the frame).
#[derive(Debug, Clone)]
pub(super) enum ModuleAction {
    OpenChat(String),
    Nav(&'static str),
    /// Settings, opened on this section (a bare `Nav("settings")` lands on Account).
    Settings(SettingsSec),
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
        "usage" => paint_usage(cabin, ui, size, &mut actions),
        "storage" => paint_storage(ui, size, &mut actions),
        "game_mode" => paint_game_mode(cabin, ui, size, &mut actions),
        "connection" => paint_connection(ui, size, &mut actions),
        "updates" => paint_updates(cabin, ui, size, &mut actions),
        "speeds" => paint_speeds(cabin, ui, size, &mut actions),
        "frame_rate" => paint_frame_rate(ui, size),
        "workboard" => paint_nav_stub(ui, size, name, "Open Workboard", "workboard", &mut actions),
        "recent_chats" => paint_nav_stub(ui, size, name, "Open Chat", "chat", &mut actions),
        _ => match module_kind(module_id) {
            ModuleKind::Link { cta, to } => {
                let note = link_note(cabin, module_id);
                paint_link_card(
                    ui,
                    size,
                    name,
                    purpose,
                    note.as_deref(),
                    cta,
                    to,
                    &mut actions,
                )
            }
            // A module with no tile yet says so plainly: no READY badge
            // over a card with nothing on it.
            _ => preview_card(ui, size, name, purpose),
        },
    }
    actions
}

/// What a Home module paints today. `Live` modules draw their own data;
/// `Link` modules have no tile data yet but their data lives on another page,
/// so the card says what it is and opens that page; `Preview` modules aren't
/// built and show a labelled placeholder. The Edit Home picker reads this too,
/// so it never offers a placeholder as if it were ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum ModuleKind {
    Live,
    Link { cta: &'static str, to: LinkTarget },
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum LinkTarget {
    Page(&'static str),
    Settings(SettingsSec),
}

/// Modules with a painter of their own (the arms of `paint_module`).
pub(in crate::app) const LIVE_MODULES: [&str; 14] = [
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
    "recent_chats",
];

pub(in crate::app) fn module_kind(id: &str) -> ModuleKind {
    if LIVE_MODULES.contains(&id) {
        return ModuleKind::Live;
    }
    let link = |cta, to| ModuleKind::Link { cta, to };
    match id {
        "game_library" => link("Open Library", LinkTarget::Page("library")),
        "journal" => link("Open Journal", LinkTarget::Page("journal")),
        "automations" => link("Open Automations", LinkTarget::Page("automations")),
        "personality" => link(
            "Open Personality",
            LinkTarget::Settings(SettingsSec::Personality),
        ),
        "playbooks" => link(
            "Open Playbooks",
            LinkTarget::Settings(SettingsSec::Playbooks),
        ),
        "local_ai" => link(
            "Open model settings",
            LinkTarget::Settings(SettingsSec::Defaults),
        ),
        _ => ModuleKind::Preview,
    }
}

/// One live line for a link card when the app already holds it in memory.
/// Tiles stay light: nothing here reads disk or calls a model.
fn link_note(cabin: &Cabin, id: &str) -> Option<String> {
    match id {
        "journal" => Some(match cabin.journal.len() {
            0 => "No open questions yet".to_string(),
            1 => "1 question in the journal".to_string(),
            n => format!("{n} questions in the journal"),
        }),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_link_card(
    ui: &mut egui::Ui,
    size: ModuleSize,
    name: &str,
    purpose: &str,
    note: Option<&str>,
    cta: &str,
    to: LinkTarget,
    actions: &mut Vec<ModuleAction>,
) {
    let action = match to {
        LinkTarget::Page(id) => ModuleAction::Nav(id),
        LinkTarget::Settings(sec) => ModuleAction::Settings(sec),
    };
    let resp = card(ui, |ui| {
        header(ui, false, name, "", crate::theme::fg());
        if let Some(note) = note {
            ui.label(
                RichText::new(note)
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::fg()),
            );
        }
        if size != ModuleSize::Small {
            ui.label(
                RichText::new(purpose)
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
            );
            ui.add_space(6.0);
            if crate::cards::ghost_pill(ui, cta) {
                actions.push(action.clone());
            }
        } else {
            // No room for a pill: the card itself is the button, and says so.
            ui.label(
                RichText::new(cta)
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::amber()),
            );
        }
    });
    if card_clicked(ui, &resp, name, cta) && actions.is_empty() {
        actions.push(action);
    }
}

/// A module that isn't built yet: labelled Preview so nobody reads it as data.
fn preview_card(ui: &mut egui::Ui, size: ModuleSize, name: &str, purpose: &str) {
    card(ui, |ui| {
        header(ui, false, name, "PREVIEW", crate::theme::muted());
        ui.label(
            RichText::new(if purpose.is_empty() {
                "Not built yet"
            } else {
                purpose
            })
            .size(crate::theme::TYPE_BODY)
            .color(crate::theme::muted()),
        );
        if size != ModuleSize::Small {
            ui.label(
                RichText::new("Not built yet: this is a placeholder.")
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
            );
        }
    });
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

/// Big number, its caption, and an optional second line for one tile.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TileText {
    pub big: String,
    pub caption: String,
    pub detail: Option<String>,
    pub tone: Tone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tone {
    Normal,
    Warn,
    Alert,
    Quiet,
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

/// The Small PC tile: the main GPU's °C when the OS reports it, else the
/// PC's thermal reading.
pub(super) fn gpu_temp_text(temp_c: Option<i32>, thermal: Option<Thermal>) -> TileText {
    match (temp_c, thermal.unwrap_or(Thermal::Unknown)) {
        (Some(c), _) => {
            let (word, tone) = if c >= 85 {
                ("hot", Tone::Alert)
            } else if c >= 70 {
                ("warm", Tone::Warn)
            } else {
                ("cool", Tone::Normal)
            };
            TileText {
                big: format!("{c} °C"),
                caption: format!("Main GPU · {word}"),
                detail: None,
                tone,
            }
        }
        (None, Thermal::Unknown) => TileText {
            big: "—".into(),
            caption: "No temperature sensor found".into(),
            detail: None,
            tone: Tone::Quiet,
        },
        (None, th) => {
            let (word, tone) = match th {
                Thermal::Fair => ("Warm", Tone::Warn),
                Thermal::Serious => ("Hot", Tone::Alert),
                Thermal::Critical => ("Critical", Tone::Alert),
                _ => ("Cool", Tone::Normal),
            };
            TileText {
                big: word.into(),
                caption: "PC sensors · no GPU sensor".into(),
                detail: None,
                tone,
            }
        }
    }
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
                                "Check this PC and suggest a tune-up.".into(),
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
    );
    let resp = card(ui, |ui| {
        header(ui, false, "AI usage", "", crate::theme::fg());
        paint_tile_text(ui, &t, size);
        if size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Budget") {
            actions.push(ModuleAction::Settings(SettingsSec::Behavior));
        }
    });
    if card_clicked(ui, &resp, "home-usage", "Daily token budget") {
        actions.push(ModuleAction::Settings(SettingsSec::Behavior));
    }
}

/// Today's tokens against the daily cap, plus this week's spend when known.
pub(super) fn usage_text(
    used: u64,
    cap: u64,
    week: Option<crate::app::budget_ui::WeekSpend>,
    week_cap_usd: u32,
) -> TileText {
    use grokhub_core::{budget_level, compact_tokens, BudgetLevel};
    let caption = if cap == 0 {
        "Tokens today · no daily cap".to_string()
    } else {
        let pct = used.saturating_mul(100) / cap.max(1);
        format!("Tokens today · {pct}% of {}", compact_tokens(cap))
    };
    let tone = match budget_level(used, cap) {
        Some(BudgetLevel::Over) => Tone::Alert,
        Some(BudgetLevel::Near) => Tone::Warn,
        _ if used == 0 => Tone::Quiet,
        _ => Tone::Normal,
    };
    let detail = week.filter(|w| w.usd > 0.0 || w.pct.is_some()).map(|w| {
        if week_cap_usd > 0 {
            format!("${:.2} of ${week_cap_usd} this week", w.usd)
        } else {
            format!("${:.2} this week", w.usd)
        }
    });
    TileText {
        big: compact_tokens(used),
        caption,
        detail,
        tone,
    }
}

fn gb_bytes(bytes: u64) -> String {
    let gb = bytes as f64 / 1_000_000_000.0;
    if gb >= 1_000.0 {
        format!("{:.1} TB", gb / 1_000.0)
    } else {
        format!("{gb:.0} GB")
    }
}

fn free_tone(d: &DiskUse) -> Tone {
    let pct_free = (d.free * 100).checked_div(d.total).unwrap_or(100);
    if pct_free < 5 {
        Tone::Alert
    } else if pct_free < 15 {
        Tone::Warn
    } else {
        Tone::Normal
    }
}

/// One line per drive, fullest first: label, "120 GB free of 1.0 TB", tone.
pub(super) fn storage_rows(drives: &[DiskUse]) -> Vec<(String, String, Tone)> {
    let mut ds: Vec<&DiskUse> = drives.iter().filter(|d| d.total > 0).collect();
    ds.sort_by_key(|d| (d.free * 1_000).checked_div(d.total).unwrap_or(1_000));
    ds.iter()
        .map(|d| {
            (
                d.label.clone(),
                format!("{} free of {}", gb_bytes(d.free), gb_bytes(d.total)),
                free_tone(d),
            )
        })
        .collect()
}

/// The Small tile: the fullest drive, or the profile's free space before the first read.
pub(super) fn storage_text(drives: &[DiskUse], fallback_free_mb: Option<u64>) -> TileText {
    let fullest = drives
        .iter()
        .filter(|d| d.total > 0)
        .min_by_key(|d| (d.free * 1_000).checked_div(d.total).unwrap_or(1_000));
    match (fullest, fallback_free_mb) {
        (Some(d), _) => {
            let used = 100 - (d.free * 100).checked_div(d.total).unwrap_or(100);
            let n = drives.iter().filter(|d| d.total > 0).count();
            TileText {
                big: format!("{} free", gb_bytes(d.free)),
                caption: format!("{} · {used}% used of {}", d.label, gb_bytes(d.total)),
                detail: (n > 1).then(|| format!("{n} drives · fullest shown")),
                tone: free_tone(d),
            }
        }
        (None, Some(mb)) if mb > 0 => TileText {
            big: format!("{} free", gb_bytes(mb * 1_048_576)),
            caption: "Home drive".into(),
            detail: None,
            tone: Tone::Normal,
        },
        _ => TileText {
            big: "—".into(),
            caption: "Reading drives…".into(),
            detail: None,
            tone: Tone::Quiet,
        },
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
            for (label, value, tone) in rows.iter().take(cap) {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(label)
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::muted()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(value)
                                .size(crate::theme::TYPE_BODY)
                                .strong()
                                .color(tone_color(*tone)),
                        );
                    });
                });
            }
            if rows.len() > cap {
                ui.label(caption(&format!("+{} more", rows.len() - cap)));
            }
        }
        if size != ModuleSize::Small && crate::cards::ghost_pill(ui, "Free up storage") {
            actions.push(ModuleAction::OpenChat("Free up storage on this PC.".into()));
        }
    });
}

/// The Updates & Drivers tile in words: Loadout first, then games, the OS
/// and the GPU driver.
pub(super) fn updates_text(
    loadout_update: bool,
    f: &super::updates::UpdateFacts,
    gpu_driver: Option<&str>,
) -> TileText {
    use super::updates::OsUpdates;
    let mut waiting = Vec::new();
    if loadout_update {
        waiting.push("Loadout".to_string());
    }
    match f.steam.len() {
        0 => {}
        1 => waiting.push(f.steam[0].clone()),
        n => waiting.push(format!("{n} Steam games")),
    }
    if let OsUpdates::Pending(n) = f.os {
        if n > 0 {
            waiting.push(format!(
                "{n} system package{}",
                if n == 1 { "" } else { "s" }
            ));
        }
    }
    let big = match waiting.len() {
        0 => "Up to date".to_string(),
        n => format!("{n} waiting"),
    };
    let caption = if waiting.is_empty() {
        match f.os {
            OsUpdates::NotAllowed => "Loadout and Steam checked · system checks off".into(),
            OsUpdates::Unknown | OsUpdates::NoChecker => "Loadout and Steam checked".into(),
            OsUpdates::Pending(_) => "Loadout, Steam and system checked".into(),
        }
    } else {
        waiting.join(" · ")
    };
    let detail = gpu_driver
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| format!("GPU driver {d}"));
    TileText {
        big,
        caption,
        detail,
        tone: if loadout_update || !f.steam.is_empty() {
            Tone::Warn
        } else {
            Tone::Normal
        },
    }
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
                if crate::cards::ghost_pill(ui, "Turn on in Settings") {
                    actions.push(ModuleAction::Settings(SettingsSec::Behavior));
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
        actions.push(ModuleAction::Settings(SettingsSec::Behavior));
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
    let Some(state) = reading else {
        return TileText {
            big: "Not read".into(),
            caption: "Loadout never hooks into games".into(),
            detail: Some("Xbox Game Bar (Win+G) or Steam's in-game counter show FPS safely".into()),
            tone: Tone::Quiet,
        };
    };
    match state {
        FpsState::Rate(r) if windows => TileText {
            big: format!("{} FPS", r.round() as u32),
            caption: match presenter {
                Some(p) => format!("From {}", p.strip_suffix(".exe").unwrap_or(p)),
                None => "Nothing is drawing frames".into(),
            },
            detail: Some("Counted by Windows, never inside the game".into()),
            tone: if presenter.is_some() {
                Tone::Normal
            } else {
                Tone::Quiet
            },
        },
        FpsState::Rate(r) => TileText {
            big: format!("{} FPS", r.round() as u32),
            caption: "Frames on screen".into(),
            detail: Some("Counted by the kernel, never inside the game".into()),
            tone: Tone::Normal,
        },
        FpsState::Starting => TileText {
            big: "...".into(),
            caption: "Counting frames on screen".into(),
            detail: None,
            tone: Tone::Quiet,
        },
        FpsState::Blocked(FpsBlock::NeedsPermission) if windows => TileText {
            big: "Not read".into(),
            caption: "Needs a one-time admin setup".into(),
            detail: Some("Copy setup, read it, run it in an admin PowerShell".into()),
            tone: Tone::Warn,
        },
        FpsState::Blocked(FpsBlock::NeedsPermission) => TileText {
            big: "Not read".into(),
            caption: "Needs a sudo setup first".into(),
            detail: Some("Copy setup, read it, run it in a terminal".into()),
            tone: Tone::Warn,
        },
        FpsState::Blocked(_) if windows => TileText {
            big: "Not read".into(),
            caption: "Windows didn't start the frame trace".into(),
            detail: Some("Xbox Game Bar (Win+G) or Steam's counter show FPS safely".into()),
            tone: Tone::Quiet,
        },
        FpsState::Blocked(_) => TileText {
            big: "Not read".into(),
            caption: "This driver doesn't report screen frames".into(),
            detail: Some("Steam's in-game FPS counter shows it safely".into()),
            tone: Tone::Quiet,
        },
    }
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
                    .map(|p| p.to_string_lossy().into_owned())
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

/// What the game guard sees, in words. Never reads game data itself.
pub(super) fn game_mode_text(
    guard: grokhub_core::game_guard::GameGuard,
    game: Option<&str>,
    local_paused: bool,
) -> TileText {
    let playing = guard.gaming || guard.anticheat.is_some();
    let big = match (playing, game) {
        (true, Some(name)) => name.to_string(),
        (true, None) => "Game running".to_string(),
        (false, _) => "No game running".to_string(),
    };
    let caption = match (guard.anticheat, guard.idle_anticheat) {
        (Some(ac), _) => format!("{ac} active · Loadout stays hands-off"),
        (None, Some(ac)) => format!("{ac} installed · idle"),
        (None, None) if playing => "Desktop control paused while you play".to_string(),
        (None, None) => "Watching for games by process name only".to_string(),
    };
    let detail = local_paused.then(|| "Local AI paused to keep the GPU free".to_string());
    TileText {
        big,
        caption,
        detail,
        tone: if playing { Tone::Warn } else { Tone::Quiet },
    }
}

fn paint_connection(ui: &mut egui::Ui, size: ModuleSize, actions: &mut Vec<ModuleAction>) {
    let facts = super::live::live_facts();
    let t = connection_text(
        grokhub_agent::route::connectivity::state(),
        grokhub_agent::route::local::switched_on(),
        &facts,
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

/// Online state, local model, and how many sends left this PC in a day.
/// The log counts sends, not bytes, so the tile says sends.
pub(super) fn connection_text(
    net: grokhub_agent::route::connectivity::NetState,
    local_on: bool,
    facts: &super::live::LiveFacts,
) -> TileText {
    use grokhub_agent::route::connectivity::NetState;
    let net_word = match net {
        NetState::Online => "Online",
        NetState::Degraded => "Spotty",
        NetState::Offline => "Offline",
    };
    let local = match (local_on, facts.local_ready) {
        (true, true) => "local AI ready",
        (true, false) => "local AI not ready",
        (false, _) => "local AI off",
    };
    let big = match facts.sends_24h {
        Some(1) => "1 send".to_string(),
        Some(n) => format!("{n} sends"),
        None => "—".to_string(),
    };
    let detail = facts
        .top_dest
        .as_ref()
        .filter(|_| facts.sends_24h.unwrap_or(0) > 0)
        .map(|d| format!("Mostly to {d} · last 24 h"));
    TileText {
        big,
        caption: format!("{net_word} · {local}"),
        detail,
        tone: match net {
            NetState::Online => Tone::Normal,
            NetState::Degraded => Tone::Warn,
            NetState::Offline => Tone::Alert,
        },
    }
}
