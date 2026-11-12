//! Mod discovery: Loadout keeps looking for mods on its own, so the Mods
//! page always has something new to show. Three kinds of lookup, all over
//! the sanctioned sources the allowlist names for a game and nothing else:
//!
//! - **New game.** The first time a moddable game shows up on this PC, its
//!   Popular, Trending and Updated pages are fetched at the next sweep.
//! - **Weekly top mods.** Every moddable game gets its Popular and Trending
//!   pages again once a week.
//! - **Explore.** Once a day a game gets one deeper page (Popular pages 2 to
//!   5, then Updated), rotating through its sources, so mods past the first
//!   page keep turning up.
//!
//! What a sweep finds is kept per game on disk ([`Store`]), capped, with when
//! each mod was first seen, so the page can show new finds and the For you
//! row can draw from more than today's first page. A sweep sends at most
//! [`MAX_REQUESTS`] requests and goes through [`ModClient`], so the allowlist,
//! the cache and each source's rate limit still apply. No model is asked
//! anything: discovery costs no tokens.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::allowlist::{self, Source};
use super::client::{search_ready, ModClient};
use super::source::{Browse, ModSummary, Page, Query, SourceError, SourceId};

const DAY: u64 = 24 * 60 * 60;
/// How often the app runs a sweep while it is open.
pub const SWEEP_EVERY_SECS: u64 = 15 * 60;
/// The shortest gap between sweeps while new games wait.
pub const NEW_GAME_GAP_SECS: u64 = 60;
/// Top mods are fetched again this often per game.
pub const WEEKLY_SECS: u64 = 7 * DAY;
/// One deeper page per game at most this often.
pub const EXPLORE_SECS: u64 = DAY;
/// At most this many requests per sweep.
pub const MAX_REQUESTS: usize = 8;
/// Sources a game's sweep uses, its preferred ones first.
const MAX_SOURCES: usize = 2;
/// Mods kept per game.
pub const MAX_FINDS: usize = 200;
/// A find counts as new this long after Loadout first saw it.
pub const NEW_SECS: u64 = 14 * DAY;
/// Summaries are cut to this many characters on disk.
const SUMMARY_CHARS: usize = 160;

/// What a lookup is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NewGame,
    Weekly,
    Explore,
}

/// One game's lookups for one reason: they all run, or the game waits for
/// the next sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub game: String,
    pub reason: Reason,
    pub jobs: Vec<(Source, Query)>,
}

/// One mod Loadout found for a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Find {
    pub source: SourceId,
    pub m: ModSummary,
    /// Unix seconds Loadout first saw it.
    pub first_seen: u64,
    /// Unix seconds a sweep last returned it.
    pub last_seen: u64,
    /// Seen on a Trending page.
    #[serde(default)]
    pub trending: bool,
}

impl Find {
    pub fn source(&self) -> Source {
        self.source.source()
    }

    pub fn is_new(&self, now: u64) -> bool {
        now.saturating_sub(self.first_seen) < NEW_SECS
    }
}

/// What discovery remembers between runs. Times are Unix seconds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    /// Game id → when Loadout first saw it on this PC.
    #[serde(default)]
    pub seen: BTreeMap<String, u64>,
    /// Game id → when its top mods were last fetched.
    #[serde(default)]
    pub weekly: BTreeMap<String, u64>,
    /// Game id → when it last got a deeper page.
    #[serde(default)]
    pub explored: BTreeMap<String, u64>,
    /// Game id → how many deeper pages it has had.
    #[serde(default)]
    pub step: BTreeMap<String, u32>,
    /// Game id → when the user last looked at its new finds.
    #[serde(default)]
    pub viewed: BTreeMap<String, u64>,
    #[serde(default)]
    pub finds: BTreeMap<String, Vec<Find>>,
    /// When the last sweep ran.
    #[serde(default)]
    pub last_sweep: u64,
}

/// A game a sweep may look at: its allowlist id and the sources ready for
/// it, preferred first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameRef {
    pub id: String,
    pub sources: Vec<Source>,
}

impl GameRef {
    /// The game's sanctioned sources that can search with these keys, or
    /// `None` when the allowlist doesn't allow the game.
    pub fn ready(id: &str, client: &ModClient) -> Option<Self> {
        let entry = allowlist::check(id).entry()?;
        let sources: Vec<Source> = entry
            .sources
            .iter()
            .copied()
            .filter(|s| search_ready(*s, client.keys()).is_ok())
            .take(MAX_SOURCES)
            .collect();
        Some(Self {
            id: entry.id.to_string(),
            sources,
        })
    }
}

/// The deeper page for a game's `step`th explore.
fn explore_query(step: u32, sources: &[Source]) -> (Source, Query) {
    // Popular pages 2..=5, then Updated pages 1 and 2: six steps a source.
    const STEPS: u32 = 6;
    let source = sources[(step / STEPS) as usize % sources.len()];
    let i = step % STEPS;
    let mut q = if i < 4 {
        Query::browse(Browse::Popular)
    } else {
        Query::browse(Browse::Updated)
    };
    q.page = if i < 4 { i + 1 } else { i - 4 };
    (source, q)
}

fn due(at: Option<&u64>, every: u64, now: u64) -> bool {
    at.is_none_or(|t| now.saturating_sub(*t) >= every)
}

/// What the next sweep should fetch, in order: new games, then weekly top
/// mods, then one deeper page each. `games` comes most played first, so
/// they go first when the budget runs out. A group that doesn't fit whole
/// waits; at most [`MAX_REQUESTS`] jobs in all.
pub fn plan(games: &[GameRef], store: &Store, now: u64) -> Vec<Group> {
    let mut groups = Vec::new();
    for g in games.iter().filter(|g| !g.sources.is_empty()) {
        let jobs = if !store.seen.contains_key(&g.id) {
            let jobs = g
                .sources
                .iter()
                .flat_map(|s| Browse::ALL.map(|b| (*s, Query::browse(b))))
                .collect();
            Some((Reason::NewGame, jobs))
        } else if due(store.weekly.get(&g.id), WEEKLY_SECS, now) {
            let jobs = g
                .sources
                .iter()
                .flat_map(|s| [Browse::Popular, Browse::Trending].map(|b| (*s, Query::browse(b))))
                .collect();
            Some((Reason::Weekly, jobs))
        } else if due(store.explored.get(&g.id), EXPLORE_SECS, now) {
            let step = store.step.get(&g.id).copied().unwrap_or(0);
            Some((Reason::Explore, vec![explore_query(step, &g.sources)]))
        } else {
            None
        };
        if let Some((reason, jobs)) = jobs {
            groups.push(Group {
                game: g.id.clone(),
                reason,
                jobs,
            });
        }
    }
    // New games first, then weekly, then explore; within each, the caller's order.
    groups.sort_by_key(|g| match g.reason {
        Reason::NewGame => 0,
        Reason::Weekly => 1,
        Reason::Explore => 2,
    });
    let mut left = MAX_REQUESTS;
    groups.retain(|g| {
        let fits = g.jobs.len() <= left;
        if fits {
            left -= g.jobs.len();
        }
        fits
    });
    groups
}

fn cut(s: &str) -> String {
    if s.chars().count() <= SUMMARY_CHARS {
        return s.to_string();
    }
    let mut out: String = s.chars().take(SUMMARY_CHARS - 1).collect();
    out.push('…');
    out
}

impl Store {
    /// Note a page of results for a game: new mods are added with `now` as
    /// when they were first seen, known ones are refreshed. Over
    /// [`MAX_FINDS`], the mods no sweep has returned for longest go first.
    pub fn record(&mut self, game: &str, source: Source, page: &Page, trending: bool, now: u64) {
        let finds = self.finds.entry(game.to_string()).or_default();
        let sid = SourceId::from(source);
        for m in &page.mods {
            let mut m = m.clone();
            m.summary = cut(&m.summary);
            match finds.iter_mut().find(|f| f.source == sid && f.m.id == m.id) {
                Some(f) => {
                    f.m = m;
                    f.last_seen = now;
                    f.trending |= trending;
                }
                None => finds.push(Find {
                    source: sid,
                    m,
                    first_seen: now,
                    last_seen: now,
                    trending,
                }),
            }
        }
        if finds.len() > MAX_FINDS {
            finds.sort_by(|a, b| {
                b.last_seen
                    .cmp(&a.last_seen)
                    .then(b.m.downloads.cmp(&a.m.downloads))
            });
            finds.truncate(MAX_FINDS);
        }
    }

    /// Mark a group done.
    pub fn finish(&mut self, group: &Group, now: u64) {
        let game = group.game.clone();
        match group.reason {
            Reason::NewGame => {
                self.seen.insert(game.clone(), now);
                self.weekly.insert(game.clone(), now);
                self.explored.insert(game, now);
            }
            Reason::Weekly => {
                self.weekly.insert(game, now);
            }
            Reason::Explore => {
                self.explored.insert(game.clone(), now);
                *self.step.entry(game).or_default() += 1;
            }
        }
    }

    /// A game's finds, newest first then most downloaded, adult ones only
    /// with 18+ on.
    pub fn new_finds(&self, game: &str, adult_ok: bool, now: u64) -> Vec<&Find> {
        let mut out: Vec<&Find> = self
            .finds
            .get(game)
            .into_iter()
            .flatten()
            .filter(|f| f.is_new(now) && (adult_ok || !f.m.adult))
            .collect();
        out.sort_by(|a, b| {
            b.first_seen
                .cmp(&a.first_seen)
                .then(b.m.downloads.cmp(&a.m.downloads))
                .then(a.m.id.cmp(&b.m.id))
        });
        out
    }

    /// How many of a game's new finds turned up since the user last looked.
    pub fn unseen(&self, game: &str, adult_ok: bool, now: u64) -> usize {
        let since = self.viewed.get(game).copied().unwrap_or(0);
        self.new_finds(game, adult_ok, now)
            .into_iter()
            .filter(|f| f.first_seen > since)
            .count()
    }

    pub fn path(config_dir: &Path) -> PathBuf {
        super::cache::mods_dir(config_dir).join("discover.json")
    }

    pub fn load(config_dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(config_dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        let path = Self::path(config_dir);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }
}

/// What one sweep did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Requests the sources answered (cache hits count too).
    pub pages: usize,
    /// Mods not seen before.
    pub found: usize,
    /// Games whose first lookup finished.
    pub new_games: Vec<String>,
    /// Set when a source asked Loadout to slow down or couldn't be reached;
    /// the rest waits for the next sweep.
    pub stopped: Option<String>,
}

/// Run one sweep now: plan, fetch through `client`, record, mark done. A
/// rate limit or a network failure ends the sweep and leaves that game for
/// next time; a page a source can't serve (no key, not found) still counts
/// as done so it isn't asked again every sweep.
pub fn sweep(client: &ModClient, store: &mut Store, games: &[GameRef], now_ms: u64) -> Report {
    let now = now_ms / 1000;
    let mut report = Report::default();
    store.last_sweep = now;
    'groups: for group in plan(games, store, now) {
        for (source, q) in &group.jobs {
            match client.search(&group.game, *source, q, now_ms) {
                Ok(got) => {
                    report.pages += 1;
                    let before = store.finds.get(&group.game).map_or(0, Vec::len);
                    store.record(
                        &group.game,
                        *source,
                        &got.value,
                        q.browse == Browse::Trending,
                        now,
                    );
                    let after = store.finds.get(&group.game).map_or(0, Vec::len);
                    report.found += after.saturating_sub(before);
                }
                Err(e @ (SourceError::RateLimited(_) | SourceError::Net(_))) => {
                    report.stopped = Some(e.line());
                    break 'groups;
                }
                Err(_) => {}
            }
        }
        store.finish(&group, now);
        if group.reason == Reason::NewGame {
            report.new_games.push(group.game.clone());
        }
    }
    report
}

/// Whether a sweep is due: not run for [`SWEEP_EVERY_SECS`], or a moddable
/// game has shown up that Loadout hasn't looked at yet (then at most once a
/// [`NEW_GAME_GAP_SECS`], so a backlog of new games drains gently).
pub fn sweep_due(store: &Store, games: &[GameRef], now: u64) -> bool {
    let since = now.saturating_sub(store.last_sweep);
    since >= SWEEP_EVERY_SECS
        || (since >= NEW_GAME_GAP_SECS
            && games
                .iter()
                .any(|g| !g.sources.is_empty() && !store.seen.contains_key(&g.id)))
}

/// One page of a game's new finds on one source, as a results page.
pub fn finds_page(finds: &[Find], source: Source, page: u32) -> Page {
    let per = super::source::PAGE_SIZE as usize;
    let mine: Vec<ModSummary> = finds
        .iter()
        .filter(|f| f.source() == source)
        .map(|f| f.m.clone())
        .collect();
    Page {
        total: Some(mine.len() as u64),
        mods: mine
            .into_iter()
            .skip(page as usize * per)
            .take(per)
            .collect(),
    }
}

/// One quiet line under the Mods title: what discovery is doing. `store` is
/// `None` before it has loaded.
pub fn status_line(store: Option<&Store>, offline: bool, running: bool, now: u64) -> String {
    if offline {
        return "Discovery waits while Offline only is on.".into();
    }
    if running {
        return "Looking for new mods…".into();
    }
    let Some(store) = store else {
        return String::new();
    };
    let fresh = store
        .finds
        .values()
        .flatten()
        .filter(|f| f.is_new(now))
        .count();
    summary_line(store.seen.len(), fresh)
}

fn summary_line(games: usize, fresh: usize) -> String {
    let plural = |n: usize, one: &'static str, many: &'static str| if n == 1 { one } else { many };
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

#[cfg(test)]
mod status_tests {
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
        assert_eq!(
            status_line(None, true, true, 0),
            "Discovery waits while Offline only is on."
        );
        assert_eq!(status_line(None, false, true, 0), "Looking for new mods…");
        assert_eq!(status_line(None, false, false, 0), "");
    }
}

#[cfg(test)]
#[path = "discover_tests.rs"]
mod tests;
