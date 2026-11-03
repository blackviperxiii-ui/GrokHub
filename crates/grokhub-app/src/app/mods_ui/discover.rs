//! Runs mod discovery ([`grokhub_core::mods::discover`]) in the background:
//! a moddable game that shows up on this PC gets its mods looked up at the
//! next sweep, every game gets its top mods weekly, and one deeper page a
//! day keeps new mods turning up. Sweeps run off the UI thread, at most one
//! at a time, never while Offline only is on or the heartbeat is halted.
//! No model is asked anything.

use std::sync::{Mutex, OnceLock};

use eframe::egui;
use grokhub_core::mods::discover::{self, Find, GameRef, Report, Store};

use super::{jobs, Rail};

/// How often the heartbeat checks whether a sweep is due.
const CHECK_EVERY_MS: u64 = 60_000;

#[derive(Default)]
struct State {
    store: Option<Store>,
    running: bool,
    last: Option<Report>,
    checked_ms: u64,
}

fn state() -> std::sync::MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn with_store<T>(f: impl FnOnce(&Store) -> T) -> T {
    let mut s = state();
    f(s
        .store
        .get_or_insert_with(|| Store::load(&crate::config::config_dir())))
}

/// The rail's moddable games as discovery sees them, most recently played
/// first, each with the sources ready for it.
fn game_refs(rail: &Rail) -> Vec<GameRef> {
    let client = jobs::shared_client();
    rail.games
        .iter()
        .filter_map(|g| GameRef::ready(g.entry.id, &client))
        .collect()
}

/// Whether a check is due; marks it checked when it is.
pub(super) fn check_due(now_ms: u64) -> bool {
    let mut s = state();
    if s.running || now_ms.saturating_sub(s.checked_ms) < CHECK_EVERY_MS {
        return false;
    }
    s.checked_ms = now_ms;
    true
}

/// Start a sweep when one is due. Cheap to call often.
pub(super) fn tick(ctx: Option<&egui::Context>, rail: &Rail, offline: bool) {
    if offline {
        return;
    }
    let games = game_refs(rail);
    if games.is_empty() {
        // Nothing to look up yet: the library scan may still be running.
        return;
    }
    let now_ms = grokhub_core::now_ms();
    let mut store = {
        let mut s = state();
        if s.running {
            return;
        }
        let store = s
            .store
            .get_or_insert_with(|| Store::load(&crate::config::config_dir()));
        if !discover::sweep_due(store, &games, now_ms / 1000) {
            return;
        }
        // Cloned only when a sweep starts: the page calls this every frame.
        let store = store.clone();
        s.running = true;
        store
    };
    let ctx = ctx.cloned();
    std::thread::spawn(move || {
        let client = jobs::shared_client();
        let report = discover::sweep(&client, &mut store, &games, now_ms);
        let mut s = state();
        // What the user looked at while the sweep ran stays looked at.
        if let Some(current) = &s.store {
            for (game, at) in &current.viewed {
                let kept = store.viewed.entry(game.clone()).or_default();
                *kept = (*kept).max(*at);
            }
        }
        let _ = store.save(&crate::config::config_dir());
        s.store = Some(store);
        s.last = Some(report);
        s.running = false;
        drop(s);
        super::suggest::forget_ranking();
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
}

/// When Loadout first saw a game, 0 when it hasn't yet.
pub(super) fn added(game: &str) -> u64 {
    with_store(|s| s.seen.get(game).copied().unwrap_or(0))
}

/// A game's new finds, newest first.
pub(super) fn new_finds(game: &str, adult_ok: bool) -> Vec<Find> {
    let now = grokhub_core::now_ms() / 1000;
    with_store(|s| s.new_finds(game, adult_ok, now).into_iter().cloned().collect())
}

/// Every find for these games, for the For you pool.
pub(super) fn finds_for(games: &[String]) -> Vec<(String, Find)> {
    with_store(|s| {
        games
            .iter()
            .flat_map(|g| {
                s.finds
                    .get(g)
                    .into_iter()
                    .flatten()
                    .map(move |f| (g.clone(), f.clone()))
            })
            .collect()
    })
}

/// How many new finds a game has that the user hasn't looked at.
pub(super) fn unseen(game: &str, adult_ok: bool) -> usize {
    let now = grokhub_core::now_ms() / 1000;
    with_store(|s| s.unseen(game, adult_ok, now))
}

/// The user opened a game's new finds.
pub(super) fn mark_viewed(game: &str) {
    let now = grokhub_core::now_ms() / 1000;
    let mut s = state();
    let dir = crate::config::config_dir();
    let running = s.running;
    let store = s.store.get_or_insert_with(|| Store::load(&dir));
    if store.viewed.get(game).is_some_and(|at| *at + 60 > now) {
        return;
    }
    store.viewed.insert(game.to_string(), now);
    // A running sweep keeps this when it saves.
    if !running {
        let _ = store.save(&dir);
    }
}

/// One quiet line under the title: what discovery is doing.
pub(super) fn status_line(offline: bool) -> String {
    if offline {
        return "Discovery waits while Offline only is on.".into();
    }
    let s = state();
    if s.running {
        return "Looking for new mods…".into();
    }
    let Some(store) = &s.store else {
        return String::new();
    };
    let now = grokhub_core::now_ms() / 1000;
    let fresh: usize = store
        .finds
        .values()
        .flatten()
        .filter(|f| f.is_new(now))
        .count();
    summary_line(store.seen.len(), fresh)
}

fn summary_line(games: usize, fresh: usize) -> String {
    match (games, fresh) {
        (0, _) => "Loadout looks up mods for your games on its own.".into(),
        (g, 0) => format!("Watching {g} {} for new mods.", plural(g, "game", "games")),
        (g, f) => format!(
            "{f} new {} across {g} {} in the last two weeks.",
            plural(f, "find", "finds"),
            plural(g, "game", "games")
        ),
    }
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 {
        one
    } else {
        many
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_line_reads_plainly() {
        assert_eq!(
            summary_line(0, 0),
            "Loadout looks up mods for your games on its own."
        );
        assert_eq!(summary_line(1, 0), "Watching 1 game for new mods.");
        assert_eq!(
            summary_line(4, 1),
            "1 new find across 4 games in the last two weeks."
        );
        assert_eq!(
            summary_line(1, 37),
            "37 new finds across 1 game in the last two weeks."
        );
    }
}
