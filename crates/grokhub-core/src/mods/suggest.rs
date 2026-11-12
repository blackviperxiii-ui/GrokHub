//! Mod suggestions, ranked on this PC from what the user plays, what they
//! already have and what's popular. Nothing here leaves the machine: the
//! candidates are the Popular and Trending pages the Mods page already
//! caches, and the play data only weighs them.
//!
//! A game's weight is its playtime (log hours) halved every 14 days since it
//! was last played. A mod's score is that weight times its popularity
//! percentile in its game's pool, plus freshness, plus how many of its
//! categories the user's installed mods share, plus a boost when something
//! installed needs it. Installed mods, mods marked Not interested, games
//! whose suggestions are hidden and (with 18+ off) adult mods never show.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{ModSummary, Source};

/// Only games played this recently get suggestions.
pub const PLAY_WINDOW_SECS: u64 = 60 * 24 * 60 * 60;
/// At most this many games get a pool.
pub const MAX_GAMES: usize = 5;
/// At most this many suggestions show.
pub const MAX_SUGGESTIONS: usize = 5;
/// At most this many of them from one game, while other games have picks.
pub const PER_GAME: usize = 2;
/// A game Loadout found this recently counts a little before it is played.
pub const NEW_GAME_SECS: u64 = 14 * 24 * 60 * 60;
/// The weight a new, unplayed game gets.
const NEW_GAME_WEIGHT: f64 = 0.5;
/// A mod discovery found this recently gets a small lift.
const NEW_FIND_DAYS: f64 = 7.0;
/// A game's pool is fetched again at most once a day.
pub const REFRESH_SECS: u64 = 24 * 60 * 60;
const HALF_LIFE_DAYS: f64 = 14.0;
const FRESH_DAYS: f64 = 30.0;

/// One game as the user plays it. Times are Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Play {
    pub game: String,
    pub playtime_min: u64,
    pub last_played: u64,
    /// When Loadout first saw the game on this PC, 0 when unknown.
    pub added: u64,
}

/// How much a game counts: log playtime, halved every 14 days idle. Zero
/// outside the 60-day window, except that a game added in the last 14 days
/// and not played since gets a small weight so its mods can show.
pub fn game_weight(p: &Play, now: u64) -> f64 {
    if p.last_played == 0 || now.saturating_sub(p.last_played) > PLAY_WINDOW_SECS {
        let new =
            p.added > 0 && p.added > p.last_played && now.saturating_sub(p.added) < NEW_GAME_SECS;
        return if new { NEW_GAME_WEIGHT } else { 0.0 };
    }
    let hours = p.playtime_min as f64 / 60.0;
    let idle_days = now.saturating_sub(p.last_played) as f64 / 86_400.0;
    (1.0 + hours).ln().max(0.1) * 0.5f64.powf(idle_days / HALF_LIFE_DAYS)
}

/// The games that get suggestions, heaviest first: played in the window,
/// suggestions not hidden, at most [`MAX_GAMES`].
pub fn games(plays: &[Play], prefs: &Prefs, now: u64) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = plays
        .iter()
        .filter(|p| !prefs.hidden_games.contains(&p.game))
        .map(|p| (p.game.clone(), game_weight(p, now)))
        .filter(|(_, w)| *w > 0.0)
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out.truncate(MAX_GAMES);
    out
}

/// Which of those games' pools are due a fetch: never fetched, or not for a day.
pub fn due(plays: &[Play], prefs: &Prefs, now: u64) -> Vec<String> {
    games(plays, prefs, now)
        .into_iter()
        .map(|(g, _)| g)
        .filter(|g| {
            prefs
                .refreshed
                .get(g)
                .is_none_or(|at| now.saturating_sub(*at) >= REFRESH_SECS)
        })
        .collect()
}

/// One mod in a game's pool.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub game: String,
    pub source: Source,
    pub m: ModSummary,
    /// From the Trending page rather than Popular.
    pub trending: bool,
    /// When discovery first found it (Unix seconds), 0 when it came from a page.
    pub found: u64,
}

/// What the user has, for the affinity and dependency terms.
#[derive(Debug, Clone, Default)]
pub struct Have {
    /// `source:id` of installed mods.
    pub keys: HashSet<String>,
    /// Installed mods' names, lowercase (Workshop items and addons have no key).
    pub names: HashSet<String>,
    /// Categories of installed mods, lowercase, with counts.
    pub categories: HashMap<String, u32>,
    /// Ids or names (lowercase) installed mods need.
    pub needed: HashSet<String>,
}

impl Have {
    /// Count the categories of the pool's mods this game already has (by key
    /// or name), for "Like mods you have".
    pub fn count_categories(&mut self, game: &str, pool: &[Candidate]) {
        for c in pool.iter().filter(|c| c.game == game) {
            if self.keys.contains(&key(c.source, &c.m.id))
                || self.names.contains(&c.m.name.to_lowercase())
            {
                for cat in &c.m.categories {
                    *self.categories.entry(cat.to_lowercase()).or_default() += 1;
                }
            }
        }
    }

    /// Fold one game's [`Have`] into the whole PC's.
    pub fn absorb(&mut self, other: Have) {
        self.keys.extend(other.keys);
        self.names.extend(other.names);
        self.needed.extend(other.needed);
        for (k, v) in other.categories {
            *self.categories.entry(k).or_default() += v;
        }
    }
}

/// The pool For you ranks: discovery's finds first (so a mod keeps when it
/// was found), then each game's cached Popular and Trending pages
/// (`(game, source, trending, page)`).
pub fn pool(
    finds: Vec<(String, super::discover::Find)>,
    pages: impl IntoIterator<Item = (String, Source, bool, super::Page)>,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = finds
        .into_iter()
        .map(|(game, f)| Candidate {
            game,
            source: f.source(),
            trending: f.trending,
            found: f.first_seen,
            m: f.m,
        })
        .collect();
    for (game, source, trending, page) in pages {
        out.extend(page.mods.into_iter().map(|m| Candidate {
            game: game.clone(),
            source,
            m,
            trending,
            found: 0,
        }));
    }
    out
}

/// Why a suggestion shows, in a few words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    Needed,
    LikeYours,
    NewFind,
    Trending,
    Popular,
}

impl Why {
    pub fn line(self, game: &str) -> String {
        match self {
            Why::Needed => "Something you have needs it".into(),
            Why::LikeYours => "Like mods you have".into(),
            Why::NewFind => format!("New find for {game}"),
            Why::Trending => format!("Trending for {game}"),
            Why::Popular => format!("Popular for {game}"),
        }
    }

    /// The same reason without the game, for a card that already names it.
    pub fn short(self) -> &'static str {
        match self {
            Why::Needed => "Something you have needs it",
            Why::LikeYours => "Like mods you have",
            Why::NewFind => "New find",
            Why::Trending => "Trending now",
            Why::Popular => "Popular",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub c: Candidate,
    pub score: f64,
    pub why: Why,
}

pub fn key(source: Source, id: &str) -> String {
    format!("{}:{id}", source.id())
}

/// Rank the pool. Ties keep a stable order (game, then source, then id).
pub fn rank(
    pool: &[Candidate],
    plays: &[Play],
    have: &Have,
    prefs: &Prefs,
    adult_ok: bool,
    now: u64,
) -> Vec<Suggestion> {
    let weights: HashMap<String, f64> = games(plays, prefs, now).into_iter().collect();
    let mut seen = HashSet::new();
    let pool: Vec<&Candidate> = pool
        .iter()
        .filter(|c| weights.contains_key(&c.game))
        .filter(|c| adult_ok || !c.m.adult)
        .filter(|c| {
            let k = key(c.source, &c.m.id);
            !have.keys.contains(&k)
                && !have.names.contains(&c.m.name.to_lowercase())
                && !prefs.not_interested.contains(&k)
                && seen.insert(k)
        })
        .collect();
    // Popularity as a percentile within each game's pool, so a niche game's
    // top mod counts as much as a huge one's.
    let mut by_game: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for c in &pool {
        by_game.entry(&c.game).or_default().push(c.m.downloads);
    }
    for v in by_game.values_mut() {
        v.sort_unstable();
    }
    let now_secs = now as f64;
    let mut out: Vec<Suggestion> = pool
        .into_iter()
        .map(|c| {
            let counts = &by_game[c.game.as_str()];
            let below = counts.partition_point(|d| *d < c.m.downloads);
            let pct = if counts.len() > 1 {
                below as f64 / (counts.len() - 1) as f64
            } else {
                1.0
            };
            let age_days = if c.m.updated == 0 {
                f64::INFINITY
            } else {
                (now_secs - c.m.updated as f64).max(0.0) / 86_400.0
            };
            let fresh = if age_days < FRESH_DAYS {
                0.5 * (1.0 - age_days / FRESH_DAYS)
            } else {
                0.0
            };
            let total: u32 = have.categories.values().sum();
            let shared: u32 =
                c.m.categories
                    .iter()
                    .filter_map(|cat| have.categories.get(&cat.to_lowercase()))
                    .sum();
            let affinity = if total == 0 {
                0.0
            } else {
                shared as f64 / total as f64
            };
            let needed = have.needed.contains(&c.m.id.to_lowercase())
                || have.needed.contains(&c.m.name.to_lowercase());
            let boost = if needed { 1.5 } else { 0.0 };
            let trend = if c.trending { 0.1 } else { 0.0 };
            let found_days = if c.found == 0 {
                f64::INFINITY
            } else {
                (now_secs - c.found as f64).max(0.0) / 86_400.0
            };
            let found = if found_days < NEW_FIND_DAYS {
                0.3 * (1.0 - found_days / NEW_FIND_DAYS)
            } else {
                0.0
            };
            let score = weights[&c.game] * (pct + fresh + affinity + boost + trend + found);
            let why = if needed {
                Why::Needed
            } else if affinity > 0.0 {
                Why::LikeYours
            } else if found > 0.0 {
                Why::NewFind
            } else if c.trending {
                Why::Trending
            } else {
                Why::Popular
            };
            Suggestion {
                c: c.clone(),
                score,
                why,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.c.game.cmp(&b.c.game))
            .then_with(|| a.c.source.id().cmp(b.c.source.id()))
            .then_with(|| a.c.m.id.cmp(&b.c.m.id))
    });
    out
}

/// The row: the best [`MAX_SUGGESTIONS`], at most [`PER_GAME`] from one
/// game while another game has a pick left. Spread picks come first, then
/// the held-back ones fill any slot left.
pub fn pick(ranked: &[Suggestion]) -> Vec<Suggestion> {
    let mut out: Vec<&Suggestion> = Vec::new();
    let mut per: HashMap<&str, usize> = HashMap::new();
    let mut held = Vec::new();
    for s in ranked {
        if out.len() == MAX_SUGGESTIONS {
            break;
        }
        let n = per.entry(&s.c.game).or_default();
        if *n < PER_GAME {
            *n += 1;
            out.push(s);
        } else {
            held.push(s);
        }
    }
    // Fewer games than slots: fill from what was held back, best first.
    for s in held {
        if out.len() == MAX_SUGGESTIONS {
            break;
        }
        out.push(s);
    }
    out.into_iter().cloned().collect()
}

/// What the user told the suggestions, and when each game's pool was fetched.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefs {
    /// `source:id` of mods marked Not interested. Kept for good.
    #[serde(default)]
    pub not_interested: BTreeSet<String>,
    /// Games whose suggestions are hidden.
    #[serde(default)]
    pub hidden_games: BTreeSet<String>,
    /// Game id → Unix seconds of its last pool fetch.
    #[serde(default)]
    pub refreshed: BTreeMap<String, u64>,
}

fn prefs_path(config_dir: &Path) -> PathBuf {
    super::cache::mods_dir(config_dir).join("prefs.json")
}

impl Prefs {
    pub fn load(config_dir: &Path) -> Self {
        std::fs::read_to_string(prefs_path(config_dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        let path = prefs_path(config_dir);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;
    const NOW: u64 = 1_800_000_000;

    fn play(game: &str, hours: u64, days_ago: u64) -> Play {
        Play {
            game: game.into(),
            playtime_min: hours * 60,
            last_played: NOW - days_ago * DAY,
            added: 0,
        }
    }

    fn cand(game: &str, id: &str, downloads: u64) -> Candidate {
        Candidate {
            game: game.into(),
            source: Source::Nexus,
            m: ModSummary {
                id: id.into(),
                name: format!("Mod {id}"),
                downloads,
                ..Default::default()
            },
            trending: false,
            found: 0,
        }
    }

    fn ids(s: &[Suggestion]) -> Vec<&str> {
        s.iter().map(|x| x.c.m.id.as_str()).collect()
    }

    #[test]
    fn the_game_you_play_most_and_latest_ranks_first() {
        let plays = [play("stardew", 300, 1), play("skyrimse", 20, 40)];
        let pool = [
            cand("skyrimse", "s1", 1_000_000),
            cand("stardew", "d1", 1_000),
        ];
        let out = rank(
            &pool,
            &plays,
            &Have::default(),
            &Prefs::default(),
            false,
            NOW,
        );
        assert_eq!(ids(&out), ["d1", "s1"]);
        assert!(game_weight(&plays[0], NOW) > game_weight(&plays[1], NOW));
        // A week idle halves less than two weeks idle.
        assert!(game_weight(&play("x", 10, 7), NOW) > game_weight(&play("x", 10, 14), NOW));
        assert_eq!(
            game_weight(&play("x", 900, 61), NOW),
            0.0,
            "outside 60 days"
        );
    }

    #[test]
    fn installed_hidden_and_adult_mods_never_show() {
        let plays = [play("stardew", 10, 1)];
        let mut adult = cand("stardew", "a", 50);
        adult.m.adult = true;
        let mut named = cand("stardew", "n", 40);
        named.m.name = "Lookup Anything".into();
        let pool = [
            cand("stardew", "1", 100),
            cand("stardew", "2", 90),
            adult,
            named,
        ];
        let mut have = Have::default();
        have.keys.insert("nexus:1".into());
        have.names.insert("lookup anything".into());
        let mut prefs = Prefs::default();
        prefs.not_interested.insert("nexus:2".into());
        assert!(rank(&pool, &plays, &have, &prefs, false, NOW).is_empty());
        assert_eq!(ids(&rank(&pool, &plays, &have, &prefs, true, NOW)), ["a"]);
        prefs.hidden_games.insert("stardew".into());
        assert!(rank(&pool, &plays, &have, &prefs, true, NOW).is_empty());
    }

    #[test]
    fn what_you_have_needs_and_likes_rises() {
        let plays = [play("skyrimse", 50, 2)];
        let mut ui = cand("skyrimse", "ui", 10);
        ui.m.categories = vec!["User Interface".into()];
        let pool = [
            cand("skyrimse", "big", 1_000_000),
            cand("skyrimse", "skse", 5),
            ui,
        ];
        let mut have = Have::default();
        have.needed.insert("skse".into());
        have.categories.insert("user interface".into(), 2);
        let out = rank(&pool, &plays, &have, &Prefs::default(), false, NOW);
        assert_eq!(ids(&out), ["skse", "ui", "big"]);
        assert_eq!(out[0].why, Why::Needed);
        assert_eq!(out[1].why, Why::LikeYours);
        assert_eq!(out[2].why.line("Skyrim"), "Popular for Skyrim");
    }

    #[test]
    fn ties_keep_a_stable_order_and_duplicates_show_once() {
        let plays = [play("stardew", 10, 1)];
        let pool = [
            cand("stardew", "b", 5),
            cand("stardew", "a", 5),
            cand("stardew", "a", 5),
        ];
        let out = rank(
            &pool,
            &plays,
            &Have::default(),
            &Prefs::default(),
            false,
            NOW,
        );
        assert_eq!(ids(&out), ["a", "b"]);
    }

    #[test]
    fn only_recent_games_are_fetched_and_at_most_daily() {
        let plays: Vec<Play> = (0..8)
            .map(|i| play(&format!("g{i}"), 10 + i, 1))
            .chain([play("old", 900, 61), play("never", 900, 0)])
            .collect();
        let mut never = plays.clone();
        never.last_mut().unwrap().last_played = 0;
        let mut prefs = Prefs::default();
        let due_now = due(&never, &prefs, NOW);
        assert_eq!(due_now.len(), MAX_GAMES);
        assert!(!due_now.iter().any(|g| g == "old" || g == "never"));
        for g in &due_now {
            prefs.refreshed.insert(g.clone(), NOW);
        }
        assert!(
            due(&never, &prefs, NOW + DAY - 1).is_empty(),
            "not again today"
        );
        assert_eq!(due(&never, &prefs, NOW + DAY).len(), MAX_GAMES);
        let idle = [play("old", 900, 61)];
        assert!(
            due(&idle, &Prefs::default(), NOW).is_empty(),
            "no fetch for idle games"
        );
    }

    #[test]
    fn the_row_is_five_and_no_game_takes_more_than_two_while_others_wait() {
        let plays = [play("a", 900, 1), play("b", 10, 1), play("c", 5, 1)];
        let mut pool: Vec<Candidate> = (0..6)
            .map(|i| cand("a", &format!("a{i}"), 1_000 - i))
            .collect();
        pool.push(cand("b", "b0", 10));
        pool.push(cand("c", "c0", 10));
        let ranked = rank(
            &pool,
            &plays,
            &Have::default(),
            &Prefs::default(),
            false,
            NOW,
        );
        assert_eq!(ranked.len(), 8);
        let row = pick(&ranked);
        assert_eq!(row.len(), MAX_SUGGESTIONS);
        let games: Vec<&str> = row.iter().map(|s| s.c.game.as_str()).collect();
        assert_eq!(games.iter().filter(|g| **g == "b").count(), 1);
        assert_eq!(games.iter().filter(|g| **g == "c").count(), 1);
        assert_eq!(
            ids(&row),
            ["a0", "a1", "b0", "c0", "a2"],
            "two from a, then b and c, then a fills the last slot"
        );
        // One game alone still fills all five.
        let only_a: Vec<Candidate> = pool.iter().filter(|c| c.game == "a").cloned().collect();
        let row = pick(&rank(
            &only_a,
            &plays,
            &Have::default(),
            &Prefs::default(),
            false,
            NOW,
        ));
        assert_eq!(ids(&row), ["a0", "a1", "a2", "a3", "a4"]);
    }

    #[test]
    fn a_new_unplayed_game_gets_a_small_weight_for_two_weeks() {
        let mut p = play("new", 0, 0);
        p.last_played = 0;
        assert_eq!(game_weight(&p, NOW), 0.0, "unknown add time");
        p.added = NOW - DAY;
        assert_eq!(game_weight(&p, NOW), 0.5);
        p.added = NOW - 15 * DAY;
        assert_eq!(game_weight(&p, NOW), 0.0, "past two weeks");
        let mut idle = play("idle", 900, 90);
        idle.added = NOW - DAY;
        assert_eq!(
            game_weight(&idle, NOW),
            0.5,
            "reinstalled after a long break"
        );
    }

    #[test]
    fn a_fresh_find_rises_and_says_so() {
        let plays = [play("stardew", 10, 1)];
        let mut fresh = cand("stardew", "new", 10);
        fresh.found = NOW - DAY;
        let mut stale = cand("stardew", "seen", 12);
        stale.found = NOW - 30 * DAY;
        // "same" is as popular as the new find; only the find's lift differs.
        let pool = [cand("stardew", "same", 10), fresh, stale];
        let out = rank(
            &pool,
            &plays,
            &Have::default(),
            &Prefs::default(),
            false,
            NOW,
        );
        assert_eq!(ids(&out), ["seen", "new", "same"]);
        assert_eq!(out[1].why, Why::NewFind);
        assert_eq!(
            out[1].why.line("Stardew Valley"),
            "New find for Stardew Valley"
        );
        assert_eq!(out[0].why, Why::Popular, "a month-old find is just popular");
    }

    #[test]
    fn not_interested_sticks_on_disk() {
        let dir = std::env::temp_dir().join(format!("mods-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut p = Prefs::load(&dir);
        assert_eq!(p, Prefs::default());
        p.not_interested.insert("nexus:12604".into());
        p.hidden_games.insert("wow".into());
        p.save(&dir).unwrap();
        assert_eq!(Prefs::load(&dir), p);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
