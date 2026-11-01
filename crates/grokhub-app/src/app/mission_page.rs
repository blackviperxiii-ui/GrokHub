//! The Objectives page on the rail, and the details behind Home's objectives
//! card: the open mission's full checklist with the weekly streak of days that
//! finished a task. Home is the one "Ready up" page; this one does not repeat
//! its PC or Up next cards. Workboards stays one click away.

use std::time::{Duration, Instant};

use grokhub_core::mission::{MissionCheckStatus, MissionView};
use grokhub_core::steam::SteamGame;

use super::library_ui::paint_cover_at;
use super::loadout_home::{
    caption, card, diamond, grouped, header, level_xp, objective_progress, xp_hover, XP_PER_LEVEL,
};
use super::*;

const DAY_MS: u64 = 86_400_000;
const PAGE_PAD: f32 = 24.0;
const GAP: f32 = 16.0;
const UP_NEXT_GAMES: usize = 3;
const STREAK_REFRESH: Duration = Duration::from_secs(30);
/// Width of the level bar in the header.
const XP_W: f32 = 360.0;
/// The checklist reads as one column, not a stretched banner.
const OBJECTIVES_MAX_W: f32 = 760.0;
const OBJECTIVES_TITLE: &str = "Objectives";

/// Left inset that centers the Objectives column on a page `page_w` wide.
pub(super) fn objectives_inset(page_w: f32, column_w: f32) -> f32 {
    ((page_w - column_w) / 2.0).max(0.0).floor()
}

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
        egui::Frame::NONE
            .fill(crate::theme::bg())
            .inner_margin(egui::Margin::same(PAGE_PAD as i8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let done = mission
                    .as_ref()
                    .map_or(0, |m| objective_progress(&m.checklist).0);
                // Same width as the checklist, so the XP bar sits above its right edge.
                // The column is centered on wide windows, and its scroll area is the
                // column's own width, so no stray scrollbar sits at the window edge.
                let width = ui.available_width().min(OBJECTIVES_MAX_W);
                let inset = objectives_inset(ui.available_width(), width);
                let page_h = ui.available_height();
                ui.horizontal_top(|ui| {
                    ui.add_space(inset);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, page_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(width);
                            self.mission_header(ui, done);
                            ui.add_space(GAP);
                            egui::ScrollArea::vertical()
                                .id_salt("mission-control-body")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_max_width(width);
                                    self.mission_objectives(ui, mission.as_ref(), now)
                                });
                        },
                    );
                });
            });
    }

    fn mission_header(&mut self, ui: &mut egui::Ui, done: usize) {
        let show_xp = self.home_layout.xp_bar;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label(caption(&day_caption(&Self::local_day())));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(OBJECTIVES_TITLE)
                        .font(crate::theme::title_font(28.0))
                        .color(crate::theme::fg()),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if show_xp {
                    let xp_tally = self.dock_xp(ui.ctx());
                    let (level, xp, frac) = level_xp(xp_tally, done);
                    ui.allocate_ui_with_layout(
                        egui::vec2(XP_W.min(ui.available_width()), 48.0),
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
                            let (bar, bar_resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 6.0),
                                egui::Sense::hover(),
                            );
                            bar_resp.on_hover_text(xp_hover(xp_tally));
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

    fn mission_objectives(&mut self, ui: &mut egui::Ui, mission: Option<&MissionView>, now: u64) {
        let streak = self.mission_streak(ui.ctx(), now);
        mission_card_fill(ui, |ui| {
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
}

fn mission_card_fill(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let h = ui.available_height();
    card(ui, |ui| {
        if h.is_finite() && h > 0.0 {
            ui.set_min_height(h);
        }
        add(ui);
    });
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
pub(super) fn up_next_tile(ui: &mut egui::Ui, g: &SteamGame, size: egui::Vec2) -> bool {
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

    #[test]
    fn objectives_column_centers_on_wide_pages_and_hugs_the_edge_on_narrow() {
        assert_eq!(super::objectives_inset(1336.0, 760.0), 288.0);
        assert_eq!(super::objectives_inset(700.0, 700.0), 0.0);
        assert_eq!(super::objectives_inset(500.0, 760.0), 0.0);
    }
}
