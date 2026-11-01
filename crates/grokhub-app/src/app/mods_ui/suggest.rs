//! "For you": mods ranked on this PC ([`grokhub_core::mods::suggest`]) from
//! the Popular and Trending pages of the games played in the last 60 days.
//! Only those pages are fetched, at most once a day per game and never
//! while Offline only is on. No playtime or mod list leaves the PC. A
//! suggestion opens the mod's drawer; installing still waits for Install.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use eframe::egui;
use grokhub_core::mods::suggest::{self, Candidate, Have, Play, Prefs, Suggestion};
use grokhub_core::mods::{Browse, Query, Source};

use super::{install, jobs, ModGame, ModsView, Rail, Tab};

const CARD_W: f32 = 220.0;

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
    rail.games
        .iter()
        .map(|g| Play {
            game: g.entry.id.to_string(),
            playtime_min: g.playtime_min,
            last_played: g.last_played,
        })
        .collect()
}

/// The source a game's pool comes from: its preferred one.
fn source_of(g: &ModGame) -> Option<Source> {
    g.entry.sources.first().copied()
}

const POOLS: [(Browse, bool); 2] = [(Browse::Popular, false), (Browse::Trending, true)];

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

/// What the user has for a game: Loadout's installs, Workshop items, WoW
/// addons, and what the drawer showed those installs need.
fn have(game: &ModGame, pool: &[Candidate]) -> Have {
    let mut h = Have::default();
    for m in install::installed_now(game.entry.id) {
        if let Some((src, id)) = m.mod_key.split_once(':') {
            if let Some(source) = Source::from_id(src) {
                for f in jobs::files_seen(game.entry.id, source, id) {
                    h.needed
                        .extend(f.deps.iter().map(|d| d.trim().to_lowercase()));
                }
            }
        }
        h.names.insert(m.name.to_lowercase());
        h.keys.insert(m.mod_key);
    }
    if let (Some(root), Some(app_id)) =
        (grokhub_core::steam::detect_root(), game.entry.steam_app_id)
    {
        for item in grokhub_core::steam::workshop_items(&root, app_id) {
            h.keys
                .insert(suggest::key(Source::Workshop, &item.id.to_string()));
        }
    }
    for row in super::installed_mods(game, None) {
        h.names.insert(row.name.to_lowercase());
    }
    for c in pool.iter().filter(|c| c.game == game.entry.id) {
        if h.keys.contains(&suggest::key(c.source, &c.m.id))
            || h.names.contains(&c.m.name.to_lowercase())
        {
            for cat in &c.m.categories {
                *h.categories.entry(cat.to_lowercase()).or_default() += 1;
            }
        }
    }
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
    let mut pool = Vec::new();
    for (game, _) in suggest::games(&plays, &p, now) {
        let Some(g) = rail.games.iter().find(|x| x.entry.id == game) else {
            continue;
        };
        let Some(source) = source_of(g) else {
            continue;
        };
        for (browse, trending) in POOLS {
            if let Some(page) = jobs::cached_page(g.entry.id, source, &Query::browse(browse)) {
                pool.extend(page.mods.into_iter().map(|m| Candidate {
                    game: game.clone(),
                    source,
                    m,
                    trending,
                }));
            }
        }
    }
    let mut have_all = Have::default();
    for g in &rail.games {
        let h = have(g, &pool);
        have_all.keys.extend(h.keys);
        have_all.names.extend(h.names);
        have_all.needed.extend(h.needed);
        for (k, v) in h.categories {
            *have_all.categories.entry(k).or_default() += v;
        }
    }
    let mut out = suggest::rank(&pool, &plays, &have_all, &p, adult_ok, now);
    out.truncate(suggest::MAX_SUGGESTIONS);
    state().ranked = Some((stamp, now_ms, out.clone()));
    out
}

/// The "For you" row: up to eight cards across the games.
pub(super) fn row(ui: &mut egui::Ui, rail: &Rail, view: &mut ModsView, all: &[Suggestion]) {
    if all.is_empty() {
        return;
    }
    ui.label(
        egui::RichText::new("FOR YOU")
            .size(crate::theme::TYPE_CAPTION)
            .extra_letter_spacing(1.2)
            .strong()
            .color(crate::theme::muted()),
    );
    egui::ScrollArea::horizontal()
        .id_salt("mods-for-you")
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for s in all {
                    card(ui, rail, view, s);
                }
            });
        });
    ui.add_space(10.0);
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

fn card(ui: &mut egui::Ui, rail: &Rail, view: &mut ModsView, s: &Suggestion) {
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
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(CARD_W);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&s.c.m.name)
                        .size(crate::theme::TYPE_BODY)
                        .strong()
                        .color(crate::theme::fg()),
                )
                .truncate(),
            );
            let mut line = s.why.line(game_name);
            if !matches!(s.why, suggest::Why::Popular | suggest::Why::Trending) {
                line = format!("{game_name} · {line}");
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(line)
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::muted()),
                )
                .truncate(),
            );
            ui.horizontal(|ui| {
                if crate::cards::ghost_pill(ui, "Open") {
                    open = true;
                }
                if crate::cards::ghost_pill(ui, "Not interested") {
                    let k = suggest::key(s.c.source, &s.c.m.id);
                    change(|p| {
                        p.not_interested.insert(k);
                    });
                }
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
    view.tab = if s.c.trending {
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
            }]
        );
    }
}
