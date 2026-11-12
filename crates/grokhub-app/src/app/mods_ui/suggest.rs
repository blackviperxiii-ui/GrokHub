//! "For you": five mods ranked on this PC ([`grokhub_core::mods::suggest`])
//! from what discovery has found ([`super::discover`]) and the Popular and
//! Trending pages of the games played in the last 60 days (or added in the
//! last two weeks). Those pages are fetched at most once a day per game and
//! never while Offline only is on. No playtime or mod list leaves the PC. A
//! suggestion opens the mod's drawer; installing still waits for Install.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use eframe::egui;
use grokhub_core::mods::rail::{pool_source as source_of, POOLS};
use grokhub_core::mods::suggest::{self, Candidate, Have, Play, Prefs, Suggestion};
use grokhub_core::mods::{Query, Source};

use super::{discover, install, jobs, ModGame, ModsView, Rail, Tab};

const CARD_MIN_W: f32 = 170.0;
const CARD_MAX_W: f32 = 280.0;
const CARD_GAP: f32 = 12.0;

#[derive(Default)]
struct State {
    prefs: Option<Prefs>,
    fetching: HashSet<String>,
    /// The last ranking, its inputs' stamp, and when it was made.
    ranked: Option<(String, u64, Vec<Suggestion>)>,
}

/// Rank again at most this often; a click or a finished fetch ranks at once.
const RANK_EVERY_MS: u64 = 30_000;

fn state() -> std::sync::MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn prefs() -> Prefs {
    let mut s = state();
    s.prefs
        .get_or_insert_with(|| Prefs::load(&crate::config::config_dir()))
        .clone()
}

fn change(f: impl FnOnce(&mut Prefs)) {
    let mut p = prefs();
    f(&mut p);
    let _ = p.save(&crate::config::config_dir());
    let mut st = state();
    st.prefs = Some(p);
    st.ranked = None;
}

pub(super) fn plays(rail: &Rail) -> Vec<Play> {
    grokhub_core::mods::rail::plays(rail, discover::added)
}

/// Rank again on the next frame (discovery found something).
pub(super) fn forget_ranking() {
    state().ranked = None;
}

/// Fetch the due games' pools on a thread, then mark them fetched.
fn refresh(ctx: &egui::Context, rail: &Rail, due: Vec<String>) {
    let jobs_due: Vec<(String, Source)> = {
        let mut s = state();
        due.into_iter()
            .filter(|g| s.fetching.insert(g.clone()))
            .filter_map(|g| {
                let src = rail
                    .games
                    .iter()
                    .find(|x| x.entry.id == g)
                    .and_then(source_of)?;
                Some((g, src))
            })
            .collect()
    };
    for (game, source) in jobs_due {
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let c = jobs::shared_client();
            let now = grokhub_core::now_ms();
            for (browse, _) in POOLS {
                // A source that needs a key for Trending just has no Trending pool.
                let _ = c.search(&game, source, &Query::browse(browse), now);
            }
            change(|p| {
                p.refreshed.insert(game.clone(), now / 1000);
            });
            let mut st = state();
            st.fetching.remove(&game);
            st.ranked = None;
            drop(st);
            ctx.request_repaint();
        });
    }
}

/// What the user has for a game, plus what the drawer showed its installs
/// need.
fn have(game: &ModGame, pool: &[Candidate]) -> Have {
    let installs = install::installed_now(game.entry.id);
    let mut needed = HashSet::new();
    for m in &installs {
        if let Some((src, id)) = m.mod_key.split_once(':') {
            if let Some(source) = Source::from_id(src) {
                for f in jobs::files_seen(game.entry.id, source, id) {
                    needed.extend(f.deps.iter().map(|d| d.trim().to_lowercase()));
                }
            }
        }
    }
    let root = grokhub_core::steam::detect_root();
    let mut h = grokhub_core::mods::rail::have(game, installs, root.as_deref(), pool);
    h.needed.extend(needed);
    h
}

/// The ranked suggestions now, from cached pages; fetches what is due.
pub(super) fn suggestions(
    ctx: &egui::Context,
    rail: &Rail,
    adult_ok: bool,
    offline: bool,
) -> Vec<Suggestion> {
    let p = prefs();
    let plays = plays(rail);
    let now_ms = grokhub_core::now_ms();
    let now = now_ms / 1000;
    if !offline {
        refresh(ctx, rail, suggest::due(&plays, &p, now));
    }
    let stamp = format!(
        "{}|{adult_ok}|{}",
        install::generation(),
        plays
            .iter()
            .map(|p| format!("{}:{}:{}", p.game, p.playtime_min, p.last_played))
            .collect::<Vec<_>>()
            .join(",")
    );
    if let Some((s, at, out)) = &state().ranked {
        if *s == stamp && now_ms.saturating_sub(*at) < RANK_EVERY_MS {
            return out.clone();
        }
    }
    let out = grokhub_core::mods::rail::for_you(
        rail,
        &plays,
        &p,
        discover::finds_for,
        jobs::cached_page,
        have,
        adult_ok,
        now,
    );
    state().ranked = Some((stamp, now_ms, out.clone()));
    out
}

/// The "For you" row: up to five cards side by side, sized to fit.
pub(super) fn row(ui: &mut egui::Ui, rail: &Rail, view: &mut ModsView, all: &[Suggestion]) {
    if all.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("For you")
                .size(crate::theme::TYPE_LABEL)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.label(
            egui::RichText::new("Picked from what you play")
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
    });
    ui.add_space(8.0);
    let n = all.len().min(suggest::MAX_SUGGESTIONS);
    let w = card_width(ui.available_width(), n);
    let wrap = egui::Layout::left_to_right(egui::Align::TOP).with_main_wrap(true);
    ui.with_layout(wrap, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(CARD_GAP, CARD_GAP);
        for s in all.iter().take(n) {
            card(ui, rail, view, s, w);
        }
    });
    ui.add_space(16.0);
}

/// Each card's inner width so `n` cards fill the row, within bounds.
pub(super) fn card_width(avail: f32, n: usize) -> f32 {
    let n = n.max(1) as f32;
    // 24 = the card's inner margin and stroke on both sides.
    ((avail - CARD_GAP * (n - 1.0)) / n - 24.0).clamp(CARD_MIN_W, CARD_MAX_W)
}

/// Under a game: hide its suggestions, or bring them back.
pub(super) fn toggle(ui: &mut egui::Ui, game: &ModGame) {
    let id = game.entry.id;
    if prefs().hidden_games.contains(id) {
        if crate::cards::ghost_pill(ui, "Show suggestions for this game") {
            change(|p| {
                p.hidden_games.remove(id);
            });
        }
    } else if crate::cards::ghost_pill(ui, "Hide suggestions for this game") {
        change(|p| {
            p.hidden_games.insert(id.to_string());
        });
    }
}

fn card(ui: &mut egui::Ui, rail: &Rail, view: &mut ModsView, s: &Suggestion, w: f32) {
    let game_name = rail
        .games
        .iter()
        .find(|g| g.entry.id == s.c.game)
        .map(|g| g.entry.name)
        .unwrap_or_default();
    let mut open = false;
    let resp = egui::Frame::NONE
        .fill(crate::theme::surface())
        .stroke(egui::Stroke::new(1.0, crate::theme::border()))
        .corner_radius(crate::theme::RADIUS_SM)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            // The row wraps left to right; a card's own lines stack.
            ui.vertical(|ui| {
                ui.set_width(w);
                ui.set_max_width(w);
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(game_name)
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::amber()),
                    )
                    .truncate(),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&s.c.m.name)
                            .size(crate::theme::TYPE_LABEL)
                            .strong()
                            .color(crate::theme::fg()),
                    )
                    .truncate(),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(s.why.short())
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::muted()),
                    )
                    .truncate(),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if crate::cards::ghost_pill(ui, "View") {
                        open = true;
                    }
                    if crate::cards::ghost_pill(ui, "Not for me") {
                        let k = suggest::key(s.c.source, &s.c.m.id);
                        change(|p| {
                            p.not_interested.insert(k);
                        });
                    }
                });
            });
        })
        .response
        .interact(egui::Sense::click());
    if open || resp.clicked() {
        open_in(view, s);
    }
}

/// Show a suggestion in its game: its source, the tab it came from, and its
/// drawer open.
fn open_in(view: &mut ModsView, s: &Suggestion) {
    view.pick(s.c.game.clone());
    view.source = Some(s.c.source);
    view.tab = if s.c.found > 0 && s.why == suggest::Why::NewFind {
        Tab::NewFinds
    } else if s.c.trending {
        Tab::Trending
    } else {
        Tab::Popular
    };
    view.open = Some(s.c.m.id.clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_suggestion_shows_its_drawer_in_its_game() {
        let s = Suggestion {
            c: Candidate {
                game: "minecraft".into(),
                source: Source::CurseForge,
                m: grokhub_core::mods::ModSummary {
                    id: "394468".into(),
                    name: "Sodium".into(),
                    ..Default::default()
                },
                trending: true,
                found: 0,
            },
            score: 1.0,
            why: suggest::Why::Trending,
        };
        let mut view = ModsView {
            picked: Some("stardew".into()),
            query: "old".into(),
            ..Default::default()
        };
        open_in(&mut view, &s);
        assert_eq!(view.picked.as_deref(), Some("minecraft"));
        assert_eq!(view.source, Some(Source::CurseForge));
        assert_eq!(view.tab, Tab::Trending);
        assert_eq!(view.open.as_deref(), Some("394468"));
        assert!(view.query.is_empty(), "a new game starts with no search");
    }

    #[test]
    fn a_new_find_opens_on_the_new_finds_tab() {
        let s = Suggestion {
            c: Candidate {
                game: "stardew".into(),
                source: Source::Nexus,
                m: grokhub_core::mods::ModSummary {
                    id: "1".into(),
                    ..Default::default()
                },
                trending: true,
                found: 1_700_000_000,
            },
            score: 1.0,
            why: suggest::Why::NewFind,
        };
        let mut view = ModsView::default();
        open_in(&mut view, &s);
        assert_eq!(view.tab, Tab::NewFinds);
        assert_eq!(view.open.as_deref(), Some("1"));
    }

    #[test]
    fn five_cards_share_the_row_within_bounds() {
        // 1100 wide: (1100 - 4 * 12) / 5 - 24 = 186.4.
        assert!((card_width(1100.0, 5) - 186.4).abs() < 0.01);
        assert_eq!(card_width(600.0, 5), CARD_MIN_W, "narrow: wraps instead");
        assert_eq!(
            card_width(1600.0, 2),
            CARD_MAX_W,
            "few cards stay card-sized"
        );
    }

    #[test]
    fn plays_come_from_the_rail() {
        let rail = super::super::mod_rail(
            &[grokhub_core::steam::SteamGame {
                app_id: 413150,
                name: "Stardew Valley".into(),
                last_played: 1_700_000_000,
                playtime_min: 600,
            }],
            &[],
            false,
        );
        assert_eq!(
            plays(&rail),
            [Play {
                game: "stardew".into(),
                playtime_min: 600,
                last_played: 1_700_000_000,
                added: 0,
            }]
        );
    }
}
