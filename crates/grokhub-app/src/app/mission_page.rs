//! The Objectives page on the rail, and the details behind Home's objectives
//! card: Today (the open chat's next step and the goals worth a look), My
//! goals and This season (see [`goals`]). Home is the one "Ready up" page; this one does not repeat
//! its PC or Up next cards. Workboards has its own rail row; this page does
//! not point to it.

use grokhub_core::mission::{MissionCheckStatus, MissionView};
use grokhub_core::steam::SteamGame;
use grokhub_core::xp::WeekView;

use super::library_ui::paint_cover_at;
use super::loadout_home::{caption, diamond, paint_level_bar};
use super::*;

mod goals;

pub(super) use goals::goal_summary;

const GAP: f32 = 16.0;
const UP_NEXT_GAMES: usize = 3;
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
        let mission = self.dock_mission(ui.ctx());
        egui::Frame::NONE
            .fill(crate::theme::bg())
            .inner_margin(egui::Margin::same(crate::theme::page_margin(ui.ctx()) as i8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
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
                            self.mission_header(ui);
                            ui.add_space(GAP);
                            egui::ScrollArea::vertical()
                                .id_salt("mission-control-body")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_max_width(width);
                                    self.mission_objectives(ui, mission.as_ref())
                                });
                        },
                    );
                });
            });
    }

    fn mission_header(&mut self, ui: &mut egui::Ui) {
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
                    let xp = self.dock_xp(ui.ctx());
                    ui.allocate_ui_with_layout(
                        egui::vec2(XP_W.min(ui.available_width()), 48.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.add_space(16.0);
                            paint_level_bar(ui, &xp, true);
                        },
                    );
                    ui.add_space(GAP);
                }
            });
        });
    }

    /// Today, My goals and This season, one card under the other.
    fn mission_objectives(&mut self, ui: &mut egui::Ui, mission: Option<&MissionView>) {
        let goals = self.dock_goals(ui.ctx());
        self.objectives_today(ui, mission, &goals);
        ui.add_space(GAP);
        self.objectives_mine(ui, &goals);
        ui.add_space(GAP);
        self.objectives_season(ui, &goals);
    }
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
            .inner_margin(egui::Margin::symmetric(12, 8))
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

/// This week's seven boxes, Monday first: amber for a day you earned XP, a
/// dashed outline for today until you do, grey for the rest.
pub(super) fn paint_streak(ui: &mut egui::Ui, w: &WeekView) {
    let width = ui.available_width();
    let gap = 8.0;
    let box_w = (width - gap * 6.0) / 7.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    for (i, on) in w.days.iter().enumerate() {
        let r = egui::Rect::from_min_size(
            rect.min + egui::vec2(i as f32 * (box_w + gap), 0.0),
            egui::vec2(box_w, 40.0),
        );
        if *on {
            p.rect_filled(r, 4.0, crate::theme::amber());
        } else if i == w.today {
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
    fn objectives_no_longer_renders_a_workboards_control_and_the_workboards_tab_still_exists() {
        let src = include_str!("mission_page.rs");
        let code = src.split("#[cfg(test)]").next().expect("code");
        assert!(
            !code.contains("ghost_pill(ui, \"Workboards\")"),
            "Workboards has its own rail row"
        );
        assert!(
            !code.contains("set_nav_id(\"workboard\")"),
            "Objectives must not point at Workboards"
        );
        assert!(
            crate::theme::LOADOUT_NAV.contains(&("workboard", "Workboards")),
            "Workboards stays on the rail"
        );
        let mut cabin = Cabin::quiet_for_test();
        cabin.set_nav_id("workboard");
        assert_eq!(cabin.nav, Nav::Workboard);
        assert_eq!(cabin.rail_id(), "workboard");
    }

    #[test]
    fn objectives_column_centers_on_wide_pages_and_hugs_the_edge_on_narrow() {
        assert_eq!(super::objectives_inset(1336.0, 760.0), 288.0);
        assert_eq!(super::objectives_inset(700.0, 700.0), 0.0);
        assert_eq!(super::objectives_inset(500.0, 760.0), 0.0);
    }
}
