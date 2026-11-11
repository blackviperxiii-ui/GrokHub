//! The Mods page: this PC's moddable games ([`grokhub_core::mods::rail`]),
//! search and browse per game over the sources the allowlist names, each
//! game's New finds, and For you. Desktop only (`capabilities::mods`).
//!
//! Every piece is the core's, the same the cabin's Mods page runs: the rail,
//! the [`ModClient`] with its cache and per-source rate limits, discovery
//! ([`discover`]) and the For you ranking ([`suggest`]). Requests go through
//! [`ModsNet`]: the source's own hosts only, the egress guard on every hop,
//! nothing while Offline only is on. No playtime or mod list leaves the PC.
//!
//! Read-only for now: installing, uninstalling and updates come with the
//! install sheet (Mods 2 of 2).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use grokhub_agent::mods_net::{mod_keys, ModsNet};
use grokhub_core::mods::allowlist::Source;
use grokhub_core::mods::discover::{self, GameRef, Store};
use grokhub_core::mods::rail::{self, ModGame, Rail, POOLS};
use grokhub_core::mods::suggest::{self, Play, Prefs, Suggestion, Why};
use grokhub_core::mods::{Browse, ModClient, ModSummary, Page, Query};
use serde::Serialize;

/// The rail is read again after this long (Battle.net's scan is not free).
const RAIL_TTL_MS: u64 = 60_000;

/// One source a game can be modded from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRow {
    pub id: &'static str,
    pub label: &'static str,
}

/// A game Loadout may mod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRow {
    pub id: &'static str,
    pub name: &'static str,
    /// What Loadout mods there ("Singleplayer only" and the like).
    pub mode_note: String,
    /// Preferred first.
    pub sources: Vec<SourceRow>,
    /// Unix seconds, 0 when never played.
    pub last_played: u64,
    pub playtime_min: u64,
    /// New finds the user hasn't opened yet.
    pub new_count: usize,
    /// For you leaves this game out.
    pub suggestions_hidden: bool,
}

/// A game Loadout won't mod, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CantRow {
    pub key: String,
    pub name: String,
    pub reason: String,
}

/// The rail and the line under the title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamesView {
    /// Most recently played first.
    pub games: Vec<GameRow>,
    /// By name; shown collapsed.
    pub cant: Vec<CantRow>,
    /// What discovery is doing, one quiet line.
    pub status: String,
    pub offline: bool,
}

/// One mod in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModRow {
    pub id: String,
    pub name: String,
    pub author: String,
    pub summary: String,
    pub downloads: u64,
    pub likes: u64,
    /// Unix seconds, 0 when the source didn't say.
    pub updated: u64,
    /// The mod's page on the web (a Workshop item's Steam client link is
    /// turned into its web page).
    pub page_url: String,
    pub icon_url: Option<String>,
    pub categories: Vec<String>,
}

/// One page of results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageView {
    pub mods: Vec<ModRow>,
    pub page: u32,
    pub more: bool,
    /// A plain line when the source failed or Offline only is on.
    pub note: Option<String>,
    /// Shown from the cache rather than asked just now.
    pub cached: bool,
}

/// One For you card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForYouRow {
    pub game: String,
    pub game_name: String,
    pub source: &'static str,
    /// The tab it opens on: `newFinds`, `trending` or `popular`.
    pub tab: &'static str,
    /// Why it shows, in a few words.
    pub why: &'static str,
    #[serde(rename = "mod")]
    pub m: ModRow,
}

/// Shown over cached rows while Offline only is on.
pub fn offline_note() -> String {
    format!(
        "Offline only is on, so this is what {} saw last.",
        grokhub_core::brand::product_name()
    )
}

/// Shown when New finds has nothing yet.
pub fn no_finds() -> String {
    format!(
        "No new finds yet. {} checks for new mods every day and lists them here.",
        grokhub_core::brand::product_name()
    )
}

#[derive(Default)]
struct State {
    client: Option<Arc<ModClient>>,
    rail: Option<(u64, Rail)>,
    fetching: HashSet<String>,
}

fn state() -> std::sync::MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

static SWEEPING: AtomicBool = AtomicBool::new(false);

/// The one client the page shares, built on first use.
pub fn client(config_dir: &Path) -> Arc<ModClient> {
    state()
        .client
        .get_or_insert_with(|| {
            Arc::new(ModClient::new(
                Box::new(ModsNet::new(config_dir.to_path_buf(), !cfg!(test))),
                config_dir,
                mod_keys(config_dir),
            ))
        })
        .clone()
}

/// This PC's rail: Steam, Battle.net and Minecraft, read at most once a
/// minute.
pub fn rail_now(now_ms: u64) -> Rail {
    if let Some((at, rail)) = &state().rail {
        if now_ms.saturating_sub(*at) < RAIL_TTL_MS {
            return rail.clone();
        }
    }
    let steam = grokhub_core::steam::detect_root()
        .map(|root| grokhub_core::steam::read_library(&root))
        .unwrap_or_default();
    let bnet = grokhub_core::battlenet::detect();
    let fresh = rail::mod_rail(&steam, &bnet, rail::minecraft_dir().is_some());
    state().rail = Some((now_ms, fresh.clone()));
    fresh
}

/// A source by its id, when the game can be modded from it.
pub fn game_source(rail: &Rail, game: &str, source: &str) -> Result<(ModGame, Source), String> {
    let g = rail
        .games
        .iter()
        .find(|g| g.entry.id == game)
        .ok_or_else(|| {
            format!(
                "{} doesn't mod that game.",
                grokhub_core::brand::product_name()
            )
        })?;
    let s = Source::from_id(source)
        .filter(|s| g.entry.sources.contains(s))
        .ok_or_else(|| format!("{} isn't a source for {}.", source, g.entry.name))?;
    Ok((g.clone(), s))
}

/// `popular`, `trending` or `updated`.
pub fn browse_of(name: &str) -> Result<Browse, String> {
    match name {
        "popular" => Ok(Browse::Popular),
        "trending" => Ok(Browse::Trending),
        "updated" => Ok(Browse::Updated),
        _ => Err(format!("No {name} list.")),
    }
}

/// A Workshop item's Steam client link as its web page; anything else as is.
fn web_page(url: &str) -> String {
    match url.strip_prefix("steam://url/CommunityFilePage/") {
        Some(id) if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) => {
            format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}")
        }
        _ => url.to_string(),
    }
}

fn mod_row(m: &ModSummary) -> ModRow {
    ModRow {
        id: m.id.clone(),
        name: m.name.clone(),
        author: m.author.clone(),
        summary: m.summary.clone(),
        downloads: m.downloads,
        likes: m.likes,
        updated: m.updated,
        page_url: web_page(&m.page_url),
        icon_url: m.icon_url.clone(),
        categories: m.categories.clone(),
    }
}

fn page_view(page: &Page, no: u32, adult_ok: bool, note: Option<String>, cached: bool) -> PageView {
    PageView {
        mods: rail::visible_mods(page, adult_ok)
            .into_iter()
            .map(mod_row)
            .collect(),
        page: no,
        more: page.has_more(no),
        note,
        cached,
    }
}

/// The rail as the page shows it.
pub fn games_view(
    r: &Rail,
    store: &Store,
    prefs: &Prefs,
    adult_ok: bool,
    offline: bool,
    sweeping: bool,
    now: u64,
) -> GamesView {
    GamesView {
        games: r
            .games
            .iter()
            .map(|g| GameRow {
                id: g.entry.id,
                name: g.entry.name,
                mode_note: g.entry.mode_note(),
                sources: g
                    .entry
                    .sources
                    .iter()
                    .map(|s| SourceRow {
                        id: s.id(),
                        label: s.label(),
                    })
                    .collect(),
                last_played: g.last_played,
                playtime_min: g.playtime_min,
                new_count: store.unseen(g.entry.id, adult_ok, now),
                suggestions_hidden: prefs.hidden_games.contains(g.entry.id),
            })
            .collect(),
        cant: r
            .cant
            .iter()
            .map(|c| CantRow {
                key: c.key.clone(),
                name: c.name.clone(),
                reason: c.reason.clone(),
            })
            .collect(),
        status: discover::status_line(Some(store), offline, sweeping, now),
        offline,
    }
}

/// Search or browse one game on one source: the cache while Offline only is
/// on, else the source (or the cache when it is still fresh).
pub fn search(
    client: &ModClient,
    game: &str,
    source: Source,
    q: &Query,
    adult_ok: bool,
    offline: bool,
    now_ms: u64,
) -> PageView {
    if offline {
        let page = client.cached_search(game, source, q).unwrap_or_default();
        return page_view(&page, q.page, adult_ok, Some(offline_note()), true);
    }
    match client.search(game, source, q, now_ms) {
        Ok(got) => page_view(&got.value, q.page, adult_ok, None, !got.fresh),
        Err(e) => {
            let page = client.cached_search(game, source, q).unwrap_or_default();
            page_view(&page, q.page, adult_ok, Some(e.line()), true)
        }
    }
}

/// One page of a game's new finds on one source; opening it marks them seen.
pub fn new_finds(
    config_dir: &Path,
    game: &str,
    source: Source,
    page: u32,
    adult_ok: bool,
    now: u64,
) -> PageView {
    let mut store = Store::load(config_dir);
    let finds: Vec<_> = store
        .new_finds(game, adult_ok, now)
        .into_iter()
        .cloned()
        .collect();
    let shown = discover::finds_page(&finds, source, page);
    if store.viewed.get(game).is_none_or(|at| *at + 60 <= now) {
        store.viewed.insert(game.to_string(), now);
        // A sweep running now keeps this when it saves.
        if !SWEEPING.load(Ordering::SeqCst) {
            let _ = store.save(config_dir);
        }
    }
    let note = shown.mods.is_empty().then(no_finds);
    page_view(&shown, page, adult_ok, note, false)
}

fn plays(r: &Rail, store: &Store) -> Vec<Play> {
    rail::plays(r, |g| store.seen.get(g).copied().unwrap_or(0))
}

/// The ranked For you cards, from what discovery found and the cached
/// Popular and Trending pages.
pub fn for_you(
    config_dir: &Path,
    client: &ModClient,
    r: &Rail,
    adult_ok: bool,
    steam_root: Option<&Path>,
    now: u64,
) -> Vec<Suggestion> {
    let store = Store::load(config_dir);
    let prefs = Prefs::load(config_dir);
    rail::for_you(
        r,
        &plays(r, &store),
        &prefs,
        |games| {
            games
                .iter()
                .flat_map(|g| {
                    store
                        .finds
                        .get(g)
                        .into_iter()
                        .flatten()
                        .map(move |f| (g.clone(), f.clone()))
                })
                .collect()
        },
        |game, source, q| client.cached_search(game, source, q),
        |game, pool| {
            let installs = grokhub_core::mods::install::installed(config_dir, Some(game.entry.id));
            rail::have(game, installs, steam_root, pool)
        },
        adult_ok,
        now,
    )
}

/// The cards as the page shows them.
pub fn for_you_rows(r: &Rail, picked: &[Suggestion]) -> Vec<ForYouRow> {
    picked
        .iter()
        .map(|s| ForYouRow {
            game: s.c.game.clone(),
            game_name: r
                .games
                .iter()
                .find(|g| g.entry.id == s.c.game)
                .map(|g| g.entry.name.to_string())
                .unwrap_or_default(),
            source: s.c.source.id(),
            tab: if s.c.found > 0 && s.why == Why::NewFind {
                "newFinds"
            } else if s.c.trending {
                "trending"
            } else {
                "popular"
            },
            why: s.why.short(),
            m: mod_row(&s.c.m),
        })
        .collect()
}

/// Change the For you choices and save them.
pub fn change_prefs(config_dir: &Path, f: impl FnOnce(&mut Prefs)) -> Result<(), String> {
    let mut p = Prefs::load(config_dir);
    f(&mut p);
    p.save(config_dir)
}

/// Fetch the Popular and Trending pages of the games For you is due to
/// refresh, each on its own thread, at most once at a time per game.
pub fn refresh_pools(config_dir: PathBuf, client: Arc<ModClient>, r: &Rail, now_ms: u64) {
    let store = Store::load(&config_dir);
    let prefs = Prefs::load(&config_dir);
    let due = suggest::due(&plays(r, &store), &prefs, now_ms / 1000);
    let jobs: Vec<(String, Source)> = {
        let mut s = state();
        due.into_iter()
            .filter_map(|g| {
                let src = r
                    .games
                    .iter()
                    .find(|x| x.entry.id == g)
                    .and_then(rail::pool_source)?;
                s.fetching.insert(g.clone()).then_some((g, src))
            })
            .collect()
    };
    for (game, source) in jobs {
        let (dir, c) = (config_dir.clone(), client.clone());
        std::thread::spawn(move || {
            for (browse, _) in POOLS {
                // A source that needs a key for Trending just has no Trending pool.
                let _ = c.search(&game, source, &Query::browse(browse), now_ms);
            }
            let _ = change_prefs(&dir, |p| {
                p.refreshed.insert(game.clone(), now_ms / 1000);
            });
            state().fetching.remove(&game);
        });
    }
}

/// Start a discovery sweep on its own thread when one is due. Cheap to call
/// often; never while Offline only is on.
pub fn sweep_if_due(config_dir: PathBuf, client: Arc<ModClient>, r: &Rail, now_ms: u64) {
    let games: Vec<GameRef> = r
        .games
        .iter()
        .filter_map(|g| GameRef::ready(g.entry.id, &client))
        .collect();
    if games.is_empty() {
        return;
    }
    let mut store = Store::load(&config_dir);
    if !discover::sweep_due(&store, &games, now_ms / 1000) {
        return;
    }
    if SWEEPING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        discover::sweep(&client, &mut store, &games, now_ms);
        // What the user opened while the sweep ran stays opened.
        for (game, at) in Store::load(&config_dir).viewed {
            let kept = store.viewed.entry(game).or_default();
            *kept = (*kept).max(at);
        }
        let _ = store.save(&config_dir);
        SWEEPING.store(false, Ordering::SeqCst);
    });
}

/// Whether a sweep is running now.
pub fn sweeping() -> bool {
    SWEEPING.load(Ordering::SeqCst)
}

#[cfg(test)]
#[path = "mods_tests.rs"]
mod tests;
