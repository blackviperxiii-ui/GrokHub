//! Library (deck slide 11, layout C): the Steam games on this PC as a cover
//! grid with filter tabs and a search, and the picked game's detail column
//! with Play. The games come from the local library `steam.rs` reads. Each
//! cover is the game's real art ([`game_art`]) over a painted cover that
//! stays when a game has none.

mod game_art;

use grokhub_core::mission::{MissionCheckStatus, MissionView};
use grokhub_core::steam::{CoverKind, SteamGame};

use super::loadout_home::{caption, card, card_mark, objective_progress};
use super::*;

/// The detail column's width, and the grid's gap and smallest tile.
const DETAIL_W: f32 = 320.0;
const DETAIL_MIN_W: f32 = 240.0;
const GRID_GAP: f32 = 16.0;
const TILE_MIN_W: f32 = 150.0;
/// Covers keep the deck's tall box art shape.
const TILE_ASPECT: f32 = 1.52;
const PAGE_PAD: f32 = 24.0;
const LIBRARY_VIEW_ID: &str = "library-view";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum LibraryTab {
    #[default]
    All,
    Installed,
    Backlog,
    OnObjective,
}

impl LibraryTab {
    pub(super) const ORDER: [LibraryTab; 4] = [
        LibraryTab::All,
        LibraryTab::Installed,
        LibraryTab::Backlog,
        LibraryTab::OnObjective,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            LibraryTab::All => "All",
            LibraryTab::Installed => "Installed",
            LibraryTab::Backlog => "Backlog",
            LibraryTab::OnObjective => "On objective",
        }
    }
}

/// What the page keeps between frames.
#[derive(Clone, Debug, Default)]
struct LibraryView {
    tab: LibraryTab,
    query: String,
    picked: Option<u32>,
}

/// Deck shots set `GROKHUB_DECK_LIBRARY_PICK` to an app id, so Library opens on
/// that game and the detail header shows its store art without a tile click.
const DECK_PICK_ENV: &str = "GROKHUB_DECK_LIBRARY_PICK";

/// The view on first open: the deck shot's pick, else the first game.
fn first_view() -> LibraryView {
    LibraryView {
        picked: super::loadout_home::deck_switch(DECK_PICK_ENV).and_then(|v| v.parse().ok()),
        ..Default::default()
    }
}

/// The library reads only installed games, so All and Installed list the
/// same ones. Backlog is never played; On objective is named in the open
/// mission's goal or checklist. The search matches the name.
pub(super) fn filter_games<'a>(
    games: &'a [SteamGame],
    tab: LibraryTab,
    query: &str,
    objective: &str,
) -> Vec<&'a SteamGame> {
    let q = query.trim().to_lowercase();
    games
        .iter()
        .filter(|g| match tab {
            LibraryTab::All | LibraryTab::Installed => true,
            LibraryTab::Backlog => g.playtime_min == 0 && g.last_played == 0,
            LibraryTab::OnObjective => names_game(objective, &g.name),
        })
        .filter(|g| q.is_empty() || g.name.to_lowercase().contains(&q))
        .collect()
}

/// Whether `text` names the game, ignoring case.
pub(super) fn names_game(text: &str, name: &str) -> bool {
    let name = name.trim().to_lowercase();
    !name.is_empty() && text.to_lowercase().contains(&name)
}

/// The open mission's words a game can be named in.
fn objective_text(mission: Option<&MissionView>) -> String {
    let Some(m) = mission else {
        return String::new();
    };
    let mut out = format!("{}\n{}", m.title, m.goal);
    for item in &m.checklist {
        if item.status != MissionCheckStatus::Cancelled {
            out.push('\n');
            out.push_str(&item.text);
        }
    }
    out
}

/// Hours played, as the PLAYED tile shows them.
pub(super) fn played_label(minutes: u64) -> String {
    match minutes {
        0 => "Not yet".into(),
        1..=59 => "< 1 h".into(),
        m => format!("{} h", m / 60),
    }
}

/// When the game was last played, from Unix seconds.
pub(super) fn last_played_label(last_played: u64, now_secs: u64) -> String {
    if last_played == 0 {
        return "Never".into();
    }
    let days = now_secs.saturating_sub(last_played) / 86_400;
    match days {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        d if d < 30 => format!("{d} days ago"),
        d if d < 365 => format!("{} mo ago", d / 30),
        d => format!("{} y ago", d / 365),
    }
}

/// Steam's own launch link for a game.
pub(super) fn play_link(app_id: u32) -> String {
    format!("steam://rungameid/{app_id}")
}

/// Tiles across for the grid's width.
pub(super) fn grid_cols(w: f32) -> usize {
    (((w + GRID_GAP) / (TILE_MIN_W + GRID_GAP)).floor() as usize).clamp(1, 6)
}

/// The cover's top colour and motif, picked from the app id so a game keeps
/// its cover. The motifs are the deck's: a sun over peaks, a diamond, a
/// tilted frame over scan lines, and a dome with a dot.
pub(super) fn cover_style(app_id: u32) -> (egui::Color32, u32) {
    const TOPS: [(u8, u8, u8); 6] = [
        (138, 30, 52),
        (120, 74, 26),
        (34, 58, 120),
        (128, 46, 30),
        (92, 36, 140),
        (30, 92, 112),
    ];
    let (r, g, b) = TOPS[(app_id / 4) as usize % TOPS.len()];
    (egui::Color32::from_rgb(r, g, b), app_id % 4)
}

pub(super) fn paint_cover(painter: &egui::Painter, rect: egui::Rect, app_id: u32) {
    paint_cover_at(painter, rect, app_id, rect.width());
}

/// [`paint_cover`] with the motifs sized from `unit` instead of the width,
/// for covers wider than they are tall.
pub(super) fn paint_cover_at(painter: &egui::Painter, rect: egui::Rect, app_id: u32, unit: f32) {
    let (top, motif) = cover_style(app_id);
    let bottom = egui::Color32::from_rgb(14, 14, 16);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| rect.left_top() + egui::vec2(w * x, h * y);
    let amber = crate::theme::amber();
    let red = crate::theme::signal_red();
    let ink = egui::Color32::from_rgb(11, 11, 13);
    let peaks = |painter: &egui::Painter| {
        painter.add(egui::Shape::convex_polygon(
            vec![at(-0.1, 1.0), at(0.32, 0.46), at(0.62, 0.78), at(0.62, 1.0)],
            ink,
            egui::Stroke::NONE,
        ));
        painter.add(egui::Shape::convex_polygon(
            vec![at(0.45, 1.0), at(0.82, 0.54), at(1.1, 0.86), at(1.1, 1.0)],
            ink,
            egui::Stroke::NONE,
        ));
    };
    match motif {
        0 => {
            painter.circle_filled(at(0.72, 0.31), unit * 0.3, amber);
            peaks(&painter);
        }
        1 => {
            let c = at(0.5, 0.35);
            let r = unit * 0.34;
            painter.add(egui::Shape::closed_line(
                vec![
                    c - egui::vec2(0.0, r),
                    c + egui::vec2(r, 0.0),
                    c + egui::vec2(0.0, r),
                    c - egui::vec2(r, 0.0),
                ],
                egui::Stroke::new(unit * 0.025, amber),
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![at(-0.1, 1.0), at(0.5, 0.62), at(1.1, 1.0)],
                ink,
                egui::Stroke::NONE,
            ));
        }
        2 => {
            for i in 0..8 {
                let y = 0.54 + i as f32 * 0.062;
                painter.hline(
                    rect.x_range(),
                    rect.top() + h * y,
                    egui::Stroke::new(1.5, egui::Color32::from_white_alpha(70)),
                );
            }
            let c = at(0.5, 0.3);
            let r = unit * 0.27;
            let (s, co) = (0.22_f32.sin(), 0.22_f32.cos());
            let corner = |dx: f32, dy: f32| c + egui::vec2(dx * co - dy * s, dx * s + dy * co) * r;
            let pts = vec![
                corner(-1.0, -1.0),
                corner(1.0, -1.0),
                corner(1.0, 1.0),
                corner(-1.0, 1.0),
            ];
            painter.add(egui::Shape::convex_polygon(
                pts,
                ink,
                egui::Stroke::new(unit * 0.025, red),
            ));
        }
        _ => {
            painter.circle_filled(at(0.5, 0.95), unit * 0.62, ink);
            painter.circle_filled(at(0.5, 0.73), unit * 0.07, amber);
        }
    }
}

/// One cover in the grid. The picked one gets the red outline.
fn cover_tile(ui: &mut egui::Ui, game: &SteamGame, size: egui::Vec2, picked: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter_at(rect);
    paint_cover(&painter, rect, game.app_id);
    if let Some(cover) = game_art::game_cover(ui.ctx(), game.app_id, CoverKind::Tall) {
        game_art::paint_art(ui, rect, &cover, crate::theme::RADIUS_SM);
    }
    let name = game.name.to_uppercase();
    let font = crate::theme::loadout_font(14.0);
    let galley = ui.fonts_mut(|f| f.layout(name, font, crate::theme::fg(), rect.width() - 24.0));
    painter.galley(
        egui::pos2(rect.left() + 12.0, rect.bottom() - 12.0 - galley.size().y),
        galley,
        crate::theme::fg(),
    );
    let stroke = if picked {
        egui::Stroke::new(2.0, crate::theme::signal_red())
    } else if resp.hovered() {
        egui::Stroke::new(1.0, crate::theme::border_strong())
    } else {
        egui::Stroke::new(1.0, crate::theme::border())
    };
    ui.painter().rect_stroke(
        rect,
        crate::theme::RADIUS_SM,
        stroke,
        egui::StrokeKind::Inside,
    );
    resp.clicked()
}

fn stat_tile(ui: &mut egui::Ui, label: &str, value: &str) {
    card(ui, |ui| {
        ui.label(caption(label));
        ui.label(
            egui::RichText::new(value)
                .font(crate::theme::loadout_font(22.0))
                .color(crate::theme::fg()),
        );
    });
}

impl Cabin {
    /// Opens Library on one game, with the tabs and search cleared.
    pub(super) fn show_in_library(&mut self, ctx: &egui::Context, app_id: u32) {
        let view = LibraryView {
            picked: Some(app_id),
            ..Default::default()
        };
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(LIBRARY_VIEW_ID), view));
        self.set_nav_id("library");
    }

    pub(super) fn ui_library(&mut self, ui: &mut egui::Ui) {
        let id = egui::Id::new(LIBRARY_VIEW_ID);
        let mut view: LibraryView = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_else(first_view);
        let now = now_ms();
        let (found, games) = self.steam_library(now);
        let games = games.to_vec();
        let mission = self.dock_mission(ui.ctx());
        let objective = objective_text(mission.as_ref());
        egui::Frame::NONE
            .fill(crate::theme::bg())
            .inner_margin(egui::Margin::same(PAGE_PAD as i8))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if !found || games.is_empty() {
                    if crate::ui_kit::empty_state(
                        ui,
                        "No Steam library",
                        "Games you install with Steam on this PC show up here.",
                        Some("Open Steam"),
                    ) {
                        let _ = crate::desktop::open_path("steam://");
                    }
                    return;
                }
                self.library_header(ui, &mut view);
                ui.add_space(16.0);
                let shown = filter_games(&games, view.tab, &view.query, &objective);
                if view
                    .picked
                    .is_none_or(|p| !games.iter().any(|g| g.app_id == p))
                {
                    view.picked = games.first().map(|g| g.app_id);
                }
                let full = ui.available_width();
                let detail_w = (full * 0.3).clamp(DETAIL_MIN_W, DETAIL_W);
                let grid_w = (full - detail_w - GRID_GAP).max(TILE_MIN_W);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = GRID_GAP;
                    ui.allocate_ui_with_layout(
                        egui::vec2(grid_w, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(grid_w);
                            if let Some(p) = library_grid(ui, &shown, grid_w, view.picked) {
                                view.picked = Some(p);
                            }
                        },
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(detail_w, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(detail_w);
                            if let Some(game) = games.iter().find(|g| Some(g.app_id) == view.picked)
                            {
                                self.library_detail(ui, game, mission.as_ref());
                            }
                        },
                    );
                });
            });
        ui.ctx().data_mut(|d| d.insert_temp(id, view));
    }

    fn library_header(&mut self, ui: &mut egui::Ui, view: &mut LibraryView) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for tab in LibraryTab::ORDER {
                let on = view.tab == tab;
                if crate::cards::felt_segment(ui, &tab.label().to_uppercase(), on).clicked() {
                    view.tab = tab;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Skills had this rail slot before; it stays one click away.
                if crate::cards::ghost_pill(ui, "Skills") {
                    self.set_nav_id("skills");
                }
                let w = (ui.available_width() - 40.0).clamp(120.0, 300.0);
                egui::Frame::NONE
                    .stroke(egui::Stroke::new(1.0, crate::theme::signal_red()))
                    .corner_radius(crate::theme::RADIUS_SM)
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut view.query)
                                .hint_text("Search library")
                                .frame(egui::Frame::NONE)
                                .desired_width(w),
                        );
                    });
            });
        });
    }

    fn library_detail(
        &mut self,
        ui: &mut egui::Ui,
        game: &SteamGame,
        mission: Option<&MissionView>,
    ) {
        ui.spacing_mut().item_spacing.y = 12.0;
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, w * 0.6), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        paint_cover(&painter, rect, game.app_id);
        if let Some(cover) = game_art::game_cover(ui.ctx(), game.app_id, CoverKind::Wide) {
            game_art::paint_art(ui, rect, &cover, 0.0);
        }
        painter.text(
            egui::pos2(rect.left() + 12.0, rect.bottom() - 12.0),
            egui::Align2::LEFT_BOTTOM,
            game.name.to_uppercase(),
            crate::theme::loadout_font(15.0),
            crate::theme::fg(),
        );
        ui.columns(2, |cols| {
            stat_tile(&mut cols[0], "Played", &played_label(game.playtime_min));
            stat_tile(
                &mut cols[1],
                "Last played",
                &last_played_label(game.last_played, now_ms() / 1000),
            );
        });
        if let Some(m) = mission.filter(|m| names_game(&objective_text(Some(m)), &game.name)) {
            let (done, total) = objective_progress(&m.checklist);
            let step = m
                .checklist
                .iter()
                .find(|i| names_game(&i.text, &game.name))
                .map(|i| i.text.clone())
                .unwrap_or_else(|| m.title.clone());
            let objective = egui::Frame::NONE
                .fill(crate::theme::surface())
                .stroke(egui::Stroke::new(1.0, crate::theme::amber()))
                .corner_radius(crate::theme::RADIUS_SM)
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        card_mark(ui);
                        ui.label(
                            egui::RichText::new("OBJECTIVE")
                                .size(crate::theme::TYPE_CAPTION)
                                .extra_letter_spacing(1.5)
                                .strong()
                                .color(crate::theme::amber()),
                        );
                    });
                    ui.label(
                        egui::RichText::new(step.trim())
                            .size(crate::theme::TYPE_BODY)
                            .strong()
                            .color(crate::theme::fg()),
                    );
                    let frac = if total > 0 {
                        done as f32 / total as f32
                    } else {
                        0.0
                    };
                    let (bar, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 6.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(bar, 3.0, crate::theme::raised());
                    let mut fill = bar;
                    fill.set_width(bar.width() * frac);
                    ui.painter().rect_filled(fill, 3.0, crate::theme::amber());
                });
            crate::motion::icon::note_card(ui, objective.response.rect);
        }
        let size = egui::vec2(w, 40.0);
        if crate::ui_kit::recipe::button(ui, None, "PLAY", true, size, true).clicked() {
            if let Err(err) = crate::desktop::open_path(&play_link(game.app_id)) {
                self.status = err;
            }
        }
        if crate::ui_kit::recipe::button(ui, None, "OPTIMIZE SETTINGS", false, size, true).clicked()
        {
            self.open_fresh_home();
            self.composer = format!("Help me optimize my settings for {}.", game.name);
        }
        ui.label(
            egui::RichText::new(
                "Read from Steam on this PC. Cover art missing here comes from Steam's store.",
            )
            .size(crate::theme::TYPE_CAPTION)
            .color(crate::theme::muted()),
        );
    }
}

/// The cover grid, in a scroll area. Returns the app id of a clicked cover.
fn library_grid(
    ui: &mut egui::Ui,
    shown: &[&SteamGame],
    w: f32,
    picked: Option<u32>,
) -> Option<u32> {
    if shown.is_empty() {
        ui.label(
            egui::RichText::new("Nothing here yet.")
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::muted()),
        );
        return None;
    }
    let cols = grid_cols(w);
    let tile_w = (w - GRID_GAP * (cols as f32 - 1.0)) / cols as f32;
    let size = egui::vec2(tile_w, tile_w * TILE_ASPECT);
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt("library-grid")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(GRID_GAP, GRID_GAP);
            for row in shown.chunks(cols) {
                ui.horizontal(|ui| {
                    for game in row {
                        if cover_tile(ui, game, size, picked == Some(game.app_id)) {
                            clicked = Some(game.app_id);
                        }
                    }
                });
            }
        });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deck_shot_opens_library_on_its_picked_game() {
        let before = std::env::var(DECK_PICK_ENV).ok();
        let before_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
        std::env::set_var("GROKHUB_DECK_SHOT", "1");
        std::env::remove_var(DECK_PICK_ENV);
        assert_eq!(first_view().picked, None, "the first game by default");
        std::env::set_var(DECK_PICK_ENV, " 1086940 ");
        assert_eq!(first_view().picked, Some(1086940));
        std::env::remove_var("GROKHUB_DECK_SHOT");
        assert_eq!(first_view().picked, None, "ignored outside a deck shot");
        match before {
            Some(v) => std::env::set_var(DECK_PICK_ENV, v),
            None => std::env::remove_var(DECK_PICK_ENV),
        }
        if let Some(v) = before_shot {
            std::env::set_var("GROKHUB_DECK_SHOT", v);
        }
    }

    fn game(app_id: u32, name: &str, last_played: u64, playtime_min: u64) -> SteamGame {
        SteamGame {
            app_id,
            name: name.into(),
            last_played,
            playtime_min,
        }
    }

    fn names(list: Vec<&SteamGame>) -> Vec<&str> {
        list.into_iter().map(|g| g.name.as_str()).collect()
    }

    #[test]
    fn tabs_and_search_pick_the_games() {
        let games = vec![
            game(10, "Rift Circuit", 1_700_000_000, 5160),
            game(20, "Hollow Peak", 1_690_000_000, 30),
            game(30, "Night Vault", 0, 0),
        ];
        let goal = "Diamond by Sunday\nWin 3 ranked games in rift circuit";
        assert_eq!(
            names(filter_games(&games, LibraryTab::All, "", goal)),
            vec!["Rift Circuit", "Hollow Peak", "Night Vault"]
        );
        assert_eq!(
            names(filter_games(&games, LibraryTab::Installed, "", goal)),
            vec!["Rift Circuit", "Hollow Peak", "Night Vault"]
        );
        assert_eq!(
            names(filter_games(&games, LibraryTab::Backlog, "", goal)),
            vec!["Night Vault"]
        );
        assert_eq!(
            names(filter_games(&games, LibraryTab::OnObjective, "", goal)),
            vec!["Rift Circuit"]
        );
        assert_eq!(
            names(filter_games(&games, LibraryTab::All, " PEAK ", goal)),
            vec!["Hollow Peak"]
        );
        assert!(filter_games(&games, LibraryTab::OnObjective, "", "").is_empty());
    }

    #[test]
    fn detail_labels_read_like_the_deck() {
        assert_eq!(played_label(0), "Not yet");
        assert_eq!(played_label(45), "< 1 h");
        assert_eq!(played_label(5160), "86 h");
        let now = 1_700_000_000;
        assert_eq!(last_played_label(0, now), "Never");
        assert_eq!(last_played_label(now - 60, now), "Today");
        assert_eq!(last_played_label(now - 86_400, now), "Yesterday");
        assert_eq!(last_played_label(now - 5 * 86_400, now), "5 days ago");
        assert_eq!(last_played_label(now - 90 * 86_400, now), "3 mo ago");
        assert_eq!(last_played_label(now - 800 * 86_400, now), "2 y ago");
        assert_eq!(play_link(1091500), "steam://rungameid/1091500");
    }

    #[test]
    fn the_grid_fits_tiles_and_covers_stay_put() {
        assert_eq!(grid_cols(100.0), 1);
        assert_eq!(grid_cols(332.0), 2);
        assert_eq!(grid_cols(1240.0), 6);
        assert_eq!(grid_cols(5000.0), 6);
        assert_eq!(cover_style(10), cover_style(10));
        assert_eq!(cover_style(0).1, 0);
        assert_eq!(cover_style(5).1, 1);
        assert_ne!(cover_style(0).0, cover_style(4).0);
    }

    #[test]
    fn the_rail_library_opens_the_game_library() {
        let lib = crate::theme::LOADOUT_NAV
            .iter()
            .find(|(_, label)| *label == "Library")
            .expect("Library on the rail");
        assert_eq!(lib.0, "library");
        let mut cabin = Cabin::quiet_for_test();
        cabin.set_nav_id("library");
        assert_eq!(cabin.nav, Nav::Library);
        assert_eq!(cabin.nav_id(), "library");
        cabin.set_nav_id("skills");
        assert_eq!(cabin.nav, Nav::Skills);
    }
}
