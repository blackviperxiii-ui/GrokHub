//! Mission control (deck slide 09, layout A): the Objectives page on the
//! rail. Today's objectives are the open mission's checklist, with a weekly
//! streak of days that finished a task; PC loadout is this machine's
//! hardware read; Up next is the Steam games played last. The composer at
//! the bottom starts a chat. Workboards stays one click away.

use std::time::{Duration, Instant};

use grokhub_core::mission::{MissionCheckStatus, MissionView};
use grokhub_core::steam::SteamGame;

use super::library_ui::paint_cover_at;
use super::loadout_home::{
    caption, card, diamond, grouped, header, level_xp, objective_progress, pc_profile, pc_score,
    pc_tile_grid, pc_tile_list, XP_PER_LEVEL,
};
use super::*;

const DAY_MS: u64 = 86_400_000;
const PAGE_PAD: f32 = 24.0;
const GAP: f32 = 16.0;
/// Three columns from here; two (objectives and PC) with Up next below
/// from [`TWO_COL_MIN_W`]; one below that.
const THREE_COL_MIN_W: f32 = 1240.0;
const TWO_COL_MIN_W: f32 = 760.0;
const UP_NEXT_W: f32 = 360.0;
const UP_NEXT_GAMES: usize = 3;
const COMPOSE_H: f32 = 112.0;
const STREAK_REFRESH: Duration = Duration::from_secs(30);
const DRAFT_ID: &str = "mission-control-draft";
/// Starters for the composer. Picking one fills it; Enter sends.
pub(super) const STARTERS: [&str; 4] = [
    "Plan my week",
    "Free up storage",
    "Check my PC",
    "Pick a game for tonight",
];

/// "Ready up, Sam." from the first word of the name, or "Ready up." without one.
pub(super) fn greeting(name: &str) -> String {
    match name.split_whitespace().next() {
        Some(first) => format!("Ready up, {first}."),
        None => "Ready up.".into(),
    }
}

/// `2026-10-28` as `WEDNESDAY · OCT 28`; anything else stays as it came.
pub(super) fn day_caption(ymd: &str) -> String {
    const DAYS: [&str; 7] = [
        "SUNDAY",
        "MONDAY",
        "TUESDAY",
        "WEDNESDAY",
        "THURSDAY",
        "FRIDAY",
        "SATURDAY",
    ];
    const MONTHS: [&str; 12] = [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ];
    let mut parts = ymd.trim().splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (Some(Some(y)), Some(Some(m)), Some(Some(d))) = (parts.next(), parts.next(), parts.next())
    else {
        return ymd.trim().to_uppercase();
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return ymd.trim().to_uppercase();
    }
    // Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let weekday = (days + 4).rem_euclid(7) as usize;
    format!("{} · {} {d}", DAYS[weekday], MONTHS[(m - 1) as usize])
}

/// Which of the last seven days (oldest first, today last) finished a task.
pub(super) fn streak_days(finished_at: &[u64], now: u64) -> [bool; 7] {
    let today = now / DAY_MS;
    let mut out = [false; 7];
    for &ts in finished_at {
        let day = ts / DAY_MS;
        if day <= today && today - day < 7 {
            out[6 - (today - day) as usize] = true;
        }
    }
    out
}

/// The tag at the end of an objective row.
pub(super) fn step_tag(status: MissionCheckStatus) -> &'static str {
    match status {
        MissionCheckStatus::Verified => "DONE",
        MissionCheckStatus::Attempted => "IN PROGRESS",
        MissionCheckStatus::Unverified => "CHECK",
        MissionCheckStatus::Planned | MissionCheckStatus::Cancelled => "",
    }
}

/// Up next: the games played most recently, then the most played.
pub(super) fn up_next(games: &[SteamGame]) -> Vec<SteamGame> {
    let mut out = games.to_vec();
    out.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then(b.playtime_min.cmp(&a.playtime_min))
            .then(a.name.cmp(&b.name))
    });
    out.truncate(UP_NEXT_GAMES);
    out
}

impl Cabin {
    pub(super) fn ui_mission_control(&mut self, ui: &mut egui::Ui) {
        let now = now_ms();
        let mission = self.dock_mission(ui.ctx());
        let (_, games) = self.steam_library(now);
        let next = up_next(games);
        egui::Frame::NONE
            .fill(crate::theme::bg())
            .inner_margin(egui::Margin::same(PAGE_PAD as i8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let done = mission
                    .as_ref()
                    .map_or(0, |m| objective_progress(&m.checklist).0);
                self.mission_header(ui, done);
                ui.add_space(GAP);
                let w = ui.available_width();
                let body_h = (ui.available_height() - COMPOSE_H).max(240.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(w, body_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("mission-control-body")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.mission_body(ui, w, mission.as_ref(), &next, now)
                            });
                    },
                );
                self.mission_compose(ui);
            });
    }

    fn mission_header(&mut self, ui: &mut egui::Ui, done: usize) {
        let name = greeting_name(
            &self.greeting_user_md,
            self.secrets
                .oauth
                .as_ref()
                .and_then(|t| t.name.as_deref())
                .unwrap_or(""),
        );
        let name = if self.cfg.display_name.trim().is_empty() {
            name
        } else {
            self.cfg.display_name.trim().to_string()
        };
        let show_xp = self.home_layout.xp_bar;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label(caption(&day_caption(&Self::local_day())));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(greeting(&name))
                        .font(crate::theme::title_font(28.0))
                        .color(crate::theme::fg()),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if show_xp {
                    let (level, xp, frac) = level_xp(self.dock_finished_tasks(ui.ctx()), done);
                    ui.allocate_ui_with_layout(
                        egui::vec2(UP_NEXT_W.min(ui.available_width()), 48.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                ui.label(caption(&format!("Lvl {level}")));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "{} / {} XP",
                                                grouped(xp),
                                                grouped(level * XP_PER_LEVEL)
                                            ))
                                            .size(crate::theme::TYPE_CAPTION)
                                            .extra_letter_spacing(1.0)
                                            .strong()
                                            .color(crate::theme::amber()),
                                        );
                                    },
                                );
                            });
                            let (bar, _) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 6.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(bar, 3.0, crate::theme::selected());
                            let mut fill = bar;
                            fill.set_width(bar.width() * frac);
                            ui.painter().rect_filled(fill, 3.0, crate::theme::amber());
                        },
                    );
                    ui.add_space(GAP);
                }
                if crate::cards::ghost_pill(ui, "Workboards") {
                    self.set_nav_id("workboard");
                }
            });
        });
    }

    fn mission_body(
        &mut self,
        ui: &mut egui::Ui,
        w: f32,
        mission: Option<&MissionView>,
        next: &[SteamGame],
        now: u64,
    ) {
        ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
        if w >= THREE_COL_MIN_W {
            let side = UP_NEXT_W;
            let col = (w - side - GAP * 2.0) / 2.0;
            ui.horizontal_top(|ui| {
                column(ui, col, |ui| self.mission_objectives(ui, mission, now));
                column(ui, col, |ui| self.mission_pc(ui));
                column(ui, side, |ui| self.mission_up_next(ui, next, false));
            });
        } else if w >= TWO_COL_MIN_W {
            let col = (w - GAP) / 2.0;
            ui.horizontal_top(|ui| {
                column(ui, col, |ui| self.mission_objectives(ui, mission, now));
                column(ui, col, |ui| self.mission_pc(ui));
            });
            self.mission_up_next(ui, next, true);
        } else {
            self.mission_objectives(ui, mission, now);
            self.mission_pc(ui);
            self.mission_up_next(ui, next, true);
        }
    }

    fn mission_objectives(&mut self, ui: &mut egui::Ui, mission: Option<&MissionView>, now: u64) {
        let streak = self.mission_streak(ui.ctx(), now);
        card(ui, |ui| {
            let items = mission.map_or(&[][..], |m| &m.checklist[..]);
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
            match mission.filter(|_| total > 0) {
                None => {
                    ui.label(
                        RichText::new(format!(
                            "Nothing on the list yet. Ask {} to plan something.",
                            grokhub_core::brand::companion_name()
                        ))
                        .size(crate::theme::TYPE_BODY)
                        .color(crate::theme::muted()),
                    );
                }
                Some(m) => objective_rows(ui, m),
            }
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
            ui.label(
                RichText::new("A day counts when it finishes a task.")
                    .size(crate::theme::TYPE_CAPTION)
                    .color(crate::theme::muted()),
            );
        });
    }

    /// Days of the last week that finished a task, re-read every [`STREAK_REFRESH`].
    pub(super) fn mission_streak(&self, ctx: &egui::Context, now: u64) -> [bool; 7] {
        type Cached = (Instant, [bool; 7]);
        let id = egui::Id::new("mission-control-streak");
        if let Some((at, days)) = ctx.data(|d| d.get_temp::<Cached>(id)) {
            if at.elapsed() < STREAK_REFRESH {
                return days;
            }
        }
        let finished: Vec<u64> = grokhub_core::outcome::read_outcomes(&crate::config::config_dir())
            .iter()
            .filter(|o| o.effective(now) == grokhub_core::outcome::OutcomeResult::Success)
            .map(|o| o.finished_at)
            .collect();
        let days = streak_days(&finished, now);
        ctx.data_mut(|d| d.insert_temp::<Cached>(id, (Instant::now(), days)));
        days
    }

    fn mission_pc(&mut self, ui: &mut egui::Ui) {
        let profile = pc_profile();
        card(ui, |ui| {
            let score = profile
                .as_ref()
                .and_then(pc_score)
                .map(|n| format!("{n}%"))
                .unwrap_or_default();
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
            ui.add_space(8.0);
            let size = egui::vec2(140.0, 40.0);
            ui.horizontal(|ui| {
                if crate::ui_kit::recipe::button(ui, None, "TUNE-UP", true, size, true).clicked() {
                    self.open_fresh_home();
                    self.composer = "Check this PC and suggest a tune-up.".into();
                }
                if crate::ui_kit::recipe::button(ui, None, "DETAILS", false, size, true).clicked() {
                    self.set_nav_id("devices");
                }
            });
        });
    }

    fn mission_up_next(&mut self, ui: &mut egui::Ui, next: &[SteamGame], row: bool) {
        ui.label(caption("Up next"));
        if next.is_empty() {
            ui.label(
                RichText::new("Games you play with Steam show up here.")
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::muted()),
            );
            return;
        }
        let mut open = None;
        let w = ui.available_width();
        if row {
            let tile_w = (w - GAP * (UP_NEXT_GAMES as f32 - 1.0)) / UP_NEXT_GAMES as f32;
            ui.horizontal_top(|ui| {
                for g in next {
                    if up_next_tile(ui, g, egui::vec2(tile_w, 120.0)) {
                        open = Some(g.app_id);
                    }
                }
            });
        } else {
            for g in next {
                if up_next_tile(ui, g, egui::vec2(w, 150.0)) {
                    open = Some(g.app_id);
                }
            }
        }
        if let Some(id) = open {
            self.show_in_library(ui.ctx(), id);
        }
    }

    fn mission_compose(&mut self, ui: &mut egui::Ui) {
        let id = egui::Id::new(DRAFT_ID);
        let mut draft: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        ui.add_space(GAP);
        ui.horizontal_wrapped(|ui| {
            for s in STARTERS {
                if crate::ui_kit::chip(ui, s, true, true).clicked() {
                    draft = s.to_string();
                }
            }
        });
        ui.add_space(10.0);
        let mut send = false;
        egui::Frame::NONE
            .fill(crate::theme::surface())
            .stroke(egui::Stroke::new(1.0, crate::theme::signal_red()))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(14, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let field_w = ui.available_width() - 48.0;
                    let edit = ui.add_sized(
                        egui::vec2(field_w, 32.0),
                        egui::TextEdit::singleline(&mut draft)
                            .frame(egui::Frame::NONE)
                            .hint_text("Assign an objective or ask anything…")
                            .vertical_align(egui::Align::Center)
                            .font(egui::FontId::proportional(crate::theme::TYPE_LABEL)),
                    );
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        send = true;
                    }
                    if crate::ui_kit::recipe::button(
                        ui,
                        None,
                        "→",
                        true,
                        egui::vec2(36.0, 36.0),
                        true,
                    )
                    .on_hover_text("Send")
                    .clicked()
                    {
                        send = true;
                    }
                });
            });
        let text = draft.trim().to_string();
        if send && !text.is_empty() {
            draft.clear();
            self.send_home_to_chat(text);
        }
        ui.ctx().data_mut(|d| d.insert_temp(id, draft));
    }
}

fn column(ui: &mut egui::Ui, w: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(w, ui.available_height()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(w);
            ui.spacing_mut().item_spacing.y = GAP;
            add(ui);
        },
    );
}

/// One row per open step; the first one not done is the highlighted one.
fn objective_rows(ui: &mut egui::Ui, m: &MissionView) {
    if !m.title.trim().is_empty() {
        ui.label(
            RichText::new(m.title.trim())
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
    }
    let current = m.checklist.iter().position(|i| {
        i.status != MissionCheckStatus::Verified && i.status != MissionCheckStatus::Cancelled
    });
    for (n, item) in m.checklist.iter().enumerate() {
        if item.status == MissionCheckStatus::Cancelled {
            continue;
        }
        let verified = item.status == MissionCheckStatus::Verified;
        let fill = if Some(n) == current {
            crate::theme::selected()
        } else {
            Color32::TRANSPARENT
        };
        egui::Frame::NONE
            .fill(fill)
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    diamond(ui, !verified);
                    let mut text = RichText::new(&item.text).size(crate::theme::TYPE_LABEL);
                    text = if verified {
                        text.strikethrough().color(crate::theme::muted())
                    } else {
                        text.color(crate::theme::fg())
                    };
                    let tag = step_tag(item.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !tag.is_empty() {
                            let ink = if verified {
                                crate::theme::muted()
                            } else {
                                crate::theme::amber()
                            };
                            ui.label(
                                RichText::new(tag)
                                    .size(crate::theme::TYPE_CAPTION)
                                    .extra_letter_spacing(1.0)
                                    .color(ink),
                            );
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(text).truncate());
                        });
                    });
                });
            });
    }
}

/// Seven boxes, today last: amber for a day that finished a task, grey for
/// one that didn't, and a dashed outline for today until it does.
pub(super) fn paint_streak(ui: &mut egui::Ui, days: &[bool; 7]) {
    let w = ui.available_width();
    let gap = 8.0;
    let box_w = (w - gap * 6.0) / 7.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 40.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    for (i, on) in days.iter().enumerate() {
        let r = egui::Rect::from_min_size(
            rect.min + egui::vec2(i as f32 * (box_w + gap), 0.0),
            egui::vec2(box_w, 40.0),
        );
        if *on {
            p.rect_filled(r, 4.0, crate::theme::amber());
        } else if i == 6 {
            let dash = egui::Stroke::new(1.0, crate::theme::signal_red());
            for side in [
                [r.left_top(), r.right_top()],
                [r.right_top(), r.right_bottom()],
                [r.right_bottom(), r.left_bottom()],
                [r.left_bottom(), r.left_top()],
            ] {
                p.extend(egui::Shape::dashed_line(&side, dash, 4.0, 4.0));
            }
        } else {
            p.rect_filled(r, 4.0, crate::theme::selected());
        }
    }
}

/// A drawn cover with the game's name; true when clicked.
fn up_next_tile(ui: &mut egui::Ui, g: &SteamGame, size: egui::Vec2) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let p = ui.painter_at(rect);
    paint_cover_at(&p, rect, g.app_id, rect.width().min(rect.height() * 1.2));
    p.text(
        rect.left_bottom() + egui::vec2(12.0, -14.0),
        egui::Align2::LEFT_BOTTOM,
        g.name.to_uppercase(),
        crate::theme::title_font(crate::theme::TYPE_LABEL),
        Color32::WHITE,
    );
    if resp.hovered() {
        p.rect_stroke(
            rect,
            8.0,
            egui::Stroke::new(1.5, crate::theme::signal_red()),
            egui::StrokeKind::Inside,
        );
    }
    resp.on_hover_text(format!("Open {} in Library", g.name))
        .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 20_754 * DAY_MS + 5 * 3_600_000;

    fn game(app_id: u32, name: &str, last_played: u64, playtime_min: u64) -> SteamGame {
        SteamGame {
            app_id,
            name: name.into(),
            last_played,
            playtime_min,
        }
    }

    #[test]
    fn the_header_greets_by_first_name_and_names_the_day() {
        assert_eq!(greeting("Sam Rivera"), "Ready up, Sam.");
        assert_eq!(greeting("  "), "Ready up.");
        assert_eq!(day_caption("2026-10-28"), "WEDNESDAY · OCT 28");
        assert_eq!(day_caption("2024-02-29"), "THURSDAY · FEB 29");
        assert_eq!(day_caption("1970-01-01"), "THURSDAY · JAN 1");
        assert_eq!(day_caption("not a day"), "NOT A DAY");
    }

    #[test]
    fn the_streak_marks_days_that_finished_a_task() {
        let finished = [
            NOW,
            NOW - 1_000,
            NOW - 2 * DAY_MS,
            NOW - 6 * DAY_MS,
            NOW - 7 * DAY_MS,
            NOW + DAY_MS,
        ];
        assert_eq!(
            streak_days(&finished, NOW),
            [true, false, false, false, true, false, true]
        );
        assert_eq!(streak_days(&[], NOW), [false; 7]);
    }

    #[test]
    fn rows_tag_their_step_state() {
        assert_eq!(step_tag(MissionCheckStatus::Verified), "DONE");
        assert_eq!(step_tag(MissionCheckStatus::Attempted), "IN PROGRESS");
        assert_eq!(step_tag(MissionCheckStatus::Unverified), "CHECK");
        assert_eq!(step_tag(MissionCheckStatus::Planned), "");
    }

    #[test]
    fn up_next_is_the_games_played_last() {
        let games = [
            game(10, "Old Favourite", 100, 9_000),
            game(20, "Rift Circuit", 900, 5_000),
            game(30, "Never Played", 0, 0),
            game(40, "Hollow Peak", 500, 60),
            game(50, "Starfall Ops", 500, 600),
        ];
        let names: Vec<String> = up_next(&games).into_iter().map(|g| g.name).collect();
        assert_eq!(names, ["Rift Circuit", "Starfall Ops", "Hollow Peak"]);
    }

    #[test]
    fn the_rail_objectives_opens_mission_control() {
        let obj = crate::theme::LOADOUT_NAV
            .iter()
            .find(|(_, label)| *label == "Objectives")
            .expect("Objectives on the rail");
        assert_eq!(obj.0, "objectives");
        assert_eq!(
            crate::icons::rail_icon_for(obj.0),
            crate::icons::RailIcon::Objectives
        );
        let mut cabin = Cabin::quiet_for_test();
        cabin.set_nav_id("objectives");
        assert_eq!(cabin.nav, Nav::Objectives);
        assert_eq!(cabin.nav_id(), "objectives");
        cabin.set_nav_id("workboard");
        assert_eq!(cabin.nav, Nav::Workboard);
    }
}
