//! The Mods rail: the games on this PC that Loadout may mod (the Library's
//! Steam games, Battle.net installs and Minecraft, joined with the
//! [`allowlist`]), most recently played first, and the rest under "Can't mod"
//! with their reason. Both shells build their Mods page from this one rail.
//!
//! Read-only: only the launchers' own library files and a game's own mod
//! folders are read, never a game's process or memory.

use std::path::{Path, PathBuf};

use super::allowlist::{self, Check, Entry};
use super::install::Manifest;
use super::{suggest, ModSummary, Page, Source};
use crate::battlenet::BnetGame;
use crate::steam::SteamGame;

/// A game Loadout may mod, as this PC has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModGame {
    pub entry: &'static Entry,
    pub last_played: u64,
    pub playtime_min: u64,
    /// Battle.net install folders (WoW's AddOns live under them).
    pub installs: Vec<PathBuf>,
}

/// A game Loadout won't mod, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CantMod {
    pub key: String,
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rail {
    pub games: Vec<ModGame>,
    pub cant: Vec<CantMod>,
}

impl Rail {
    fn add(
        &mut self,
        check: Check,
        key: String,
        name: String,
        played: (u64, u64),
        install: Option<PathBuf>,
    ) {
        match check {
            Check::Allowed(entry) => {
                if let Some(g) = self.games.iter_mut().find(|g| g.entry.id == entry.id) {
                    g.last_played = g.last_played.max(played.0);
                    g.playtime_min += played.1;
                    g.installs.extend(install);
                } else {
                    self.games.push(ModGame {
                        entry,
                        last_played: played.0,
                        playtime_min: played.1,
                        installs: install.into_iter().collect(),
                    });
                }
            }
            Check::Blocked(reason) => {
                let (key, name) = match allowlist::by_id(&key) {
                    Some(e) => (e.id.to_string(), e.name.to_string()),
                    None => (key, name),
                };
                if !self.cant.iter().any(|c| c.key == key) {
                    self.cant.push(CantMod { key, name, reason });
                }
            }
        }
    }
}

/// The rail: Library games the allowlist allows, most recently played then
/// most played, and the rest under "Can't mod" by name.
pub fn mod_rail(steam: &[SteamGame], bnet: &[BnetGame], minecraft: bool) -> Rail {
    let mut rail = Rail::default();
    for g in steam {
        let check = allowlist::check_steam(g.app_id);
        let key = check
            .entry()
            .map(|e| e.id.to_string())
            .or_else(|| allowlist::by_steam_app(g.app_id).map(|e| e.id.to_string()))
            .unwrap_or_else(|| format!("steam:{}", g.app_id));
        rail.add(
            check,
            key,
            g.name.clone(),
            (g.last_played, g.playtime_min),
            None,
        );
    }
    for g in bnet {
        let check = allowlist::check_bnet(&g.product);
        let key = allowlist::by_bnet_product(&g.product)
            .map(|e| e.id.to_string())
            .unwrap_or_else(|| format!("bnet:{}", g.product));
        rail.add(check, key, g.name(), (0, 0), Some(g.install.clone()));
    }
    if minecraft {
        rail.add(
            allowlist::check("minecraft"),
            "minecraft".into(),
            "Minecraft".into(),
            (0, 0),
            None,
        );
    }
    rail.games.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then(b.playtime_min.cmp(&a.playtime_min))
            .then(
                a.entry
                    .name
                    .to_lowercase()
                    .cmp(&b.entry.name.to_lowercase()),
            )
    });
    rail.cant.sort_by_key(|c| c.name.to_lowercase());
    rail
}

/// The Minecraft launcher's folder, when this PC has one.
pub fn minecraft_dir() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join(".minecraft")),
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".minecraft")),
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Application Support/minecraft")),
    ];
    candidates.into_iter().flatten().find(|d| d.is_dir())
}

/// Results the page may show: adult mods only with 18+ on.
pub fn visible_mods(page: &Page, adult_ok: bool) -> Vec<&ModSummary> {
    page.mods.iter().filter(|m| adult_ok || !m.adult).collect()
}

/// One mod already on this PC, read from the game's own folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub name: String,
    pub detail: String,
    pub link: Option<String>,
}

/// What a game has installed that Loadout didn't put there, read-only:
/// Steam Workshop items and WoW addons.
pub fn installed_mods(game: &ModGame, steam_root: Option<&Path>) -> Vec<Installed> {
    let mut out = Vec::new();
    if let (Some(root), Some(app_id)) = (steam_root, game.entry.steam_app_id) {
        if game.entry.sources.contains(&Source::Workshop) {
            for item in crate::steam::workshop_items(root, app_id) {
                out.push(Installed {
                    name: format!("Workshop item {}", item.id),
                    detail: format!("{:.1} MB, from Steam Workshop", item.size as f64 / 1e6),
                    link: Some(crate::mods::workshop::item_url(&item.id.to_string())),
                });
            }
        }
    }
    for install in &game.installs {
        for flavor in crate::battlenet::wow_flavors(install) {
            let addons = flavor.dir(install).join("Interface").join("AddOns");
            let Ok(dir) = std::fs::read_dir(&addons) else {
                continue;
            };
            let mut names: Vec<String> = dir
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| !n.starts_with("Blizzard_") && !n.starts_with('.'))
                .collect();
            names.sort_by_key(|n| n.to_lowercase());
            out.extend(names.into_iter().map(|name| Installed {
                name,
                detail: format!("Addon, {}", flavor.label()),
                link: None,
            }));
        }
    }
    out
}

/// For you's pools per game: Popular, then Trending (`true`).
pub const POOLS: [(super::Browse, bool); 2] = [
    (super::Browse::Popular, false),
    (super::Browse::Trending, true),
];

/// The source a game's For you pool comes from: its preferred one.
pub fn pool_source(game: &ModGame) -> Option<Source> {
    game.entry.sources.first().copied()
}

/// The rail's games as For you weighs them. `added` is when Loadout first
/// saw a game, 0 when unknown.
pub fn plays(rail: &Rail, added: impl Fn(&str) -> u64) -> Vec<suggest::Play> {
    rail.games
        .iter()
        .map(|g| suggest::Play {
            game: g.entry.id.to_string(),
            playtime_min: g.playtime_min,
            last_played: g.last_played,
            added: added(g.entry.id),
        })
        .collect()
}

/// What this PC has for a game: Loadout's installs, Workshop items and WoW
/// addons, and the categories of the pool's mods it already has.
pub fn have(
    game: &ModGame,
    installs: Vec<Manifest>,
    steam_root: Option<&Path>,
    pool: &[suggest::Candidate],
) -> suggest::Have {
    let mut h = suggest::Have::default();
    for m in installs {
        h.names.insert(m.name.to_lowercase());
        h.keys.insert(m.mod_key);
    }
    if let (Some(root), Some(app_id)) = (steam_root, game.entry.steam_app_id) {
        for item in crate::steam::workshop_items(root, app_id) {
            h.keys
                .insert(suggest::key(Source::Workshop, &item.id.to_string()));
        }
    }
    for row in installed_mods(game, None) {
        h.names.insert(row.name.to_lowercase());
    }
    h.count_categories(game.entry.id, pool);
    h
}

/// The For you cards: discovery's finds and each game's cached Popular and
/// Trending pages, ranked against what the PC has. `finds` gives the finds
/// for the games that get suggestions, `cached` a cached page, `have` what
/// the PC has for a game.
#[allow(clippy::too_many_arguments)]
pub fn for_you(
    rail: &Rail,
    plays: &[suggest::Play],
    prefs: &suggest::Prefs,
    finds: impl FnOnce(&[String]) -> Vec<(String, super::discover::Find)>,
    cached: impl Fn(&str, Source, &super::Query) -> Option<Page>,
    have: impl Fn(&ModGame, &[suggest::Candidate]) -> suggest::Have,
    adult_ok: bool,
    now: u64,
) -> Vec<suggest::Suggestion> {
    let games: Vec<String> = suggest::games(plays, prefs, now)
        .into_iter()
        .map(|(g, _)| g)
        .collect();
    let mut pages = Vec::new();
    for game in &games {
        let Some(source) = rail
            .games
            .iter()
            .find(|g| g.entry.id == game.as_str())
            .and_then(pool_source)
        else {
            continue;
        };
        for (browse, trending) in POOLS {
            if let Some(page) = cached(game, source, &super::Query::browse(browse)) {
                pages.push((game.clone(), source, trending, page));
            }
        }
    }
    let pool = suggest::pool(finds(&games), pages);
    let mut have_all = suggest::Have::default();
    for g in &rail.games {
        have_all.absorb(have(g, &pool));
    }
    suggest::pick(&suggest::rank(
        &pool, plays, &have_all, prefs, adult_ok, now,
    ))
}
