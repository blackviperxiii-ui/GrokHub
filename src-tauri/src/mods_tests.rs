use super::*;

use grokhub_core::mods::allowlist::UNKNOWN_REASON;
use grokhub_core::mods::{Fetch, Keys, Reply, Request, SourceError};
use grokhub_core::steam::SteamGame;

const SEARCH: &str =
    include_str!("../../crates/grokhub-core/tests/fixtures/mods/modrinth_search.json");
const NOW: u64 = 1_800_000_000;

/// Answers every Modrinth search with the fixture and counts what it was asked.
struct Fake(Arc<Mutex<Vec<String>>>);

impl Fetch for Fake {
    fn send(&self, req: &Request) -> Result<Reply, SourceError> {
        self.0.lock().unwrap().push(req.url.clone());
        if req.url.contains("/v2/search") {
            Ok(Reply::ok(SEARCH))
        } else {
            Ok(Reply {
                status: 404,
                ..Reply::default()
            })
        }
    }
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loadout-mods-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fake_client(dir: &Path) -> (ModClient, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let c = ModClient::new(Box::new(Fake(log.clone())), dir, Keys::default());
    (c, log)
}

fn steam(app_id: u32, name: &str, days_ago: u64, hours: u64) -> SteamGame {
    SteamGame {
        app_id,
        name: name.into(),
        last_played: if days_ago == u64::MAX {
            0
        } else {
            NOW - days_ago * 86_400
        },
        playtime_min: hours * 60,
    }
}

/// A Library with two moddable games, one never played, and one Loadout
/// hasn't checked.
fn library() -> Rail {
    rail::mod_rail(
        &[
            steam(413150, "Stardew Valley", 9, 40),
            steam(999_999_001, "Some Unknown Game", 1, 2),
            steam(892970, "Valheim", 2, 10),
            steam(294100, "RimWorld", u64::MAX, 0),
        ],
        &[],
        true,
    )
}

#[test]
fn the_rail_puts_the_last_played_first_and_the_rest_under_cant_mod() {
    let dir = temp("rail");
    let r = library();
    let view = games_view(
        &r,
        &Store::load(&dir),
        &Prefs::load(&dir),
        false,
        false,
        false,
        NOW,
    );
    let ids: Vec<&str> = view.games.iter().map(|g| g.id).collect();
    assert_eq!(ids, ["valheim", "stardew", "minecraft", "rimworld"]);
    let mc = &view.games[2];
    assert_eq!(
        mc.sources.iter().map(|s| s.id).collect::<Vec<_>>(),
        ["modrinth", "curseforge"]
    );
    assert_eq!(view.games[1].playtime_min, 40 * 60);
    assert_eq!(
        view.cant,
        [CantRow {
            key: "steam:999999001".into(),
            name: "Some Unknown Game".into(),
            reason: UNKNOWN_REASON.into(),
        }]
    );
    assert_eq!(
        view.status,
        "Loadout looks up mods for your games on its own."
    );
}

#[test]
fn a_source_the_game_doesnt_use_is_refused() {
    let r = library();
    assert!(game_source(&r, "minecraft", "modrinth").is_ok());
    assert!(game_source(&r, "stardew", "modrinth").is_err());
    assert!(game_source(&r, "steam:999999001", "nexus").is_err());
    assert!(browse_of("trending").is_ok());
    assert!(browse_of("installed").is_err());
}

#[test]
fn a_search_returns_the_sources_rows() {
    let dir = temp("search");
    let (c, log) = fake_client(&dir);
    let view = search(
        &c,
        "minecraft",
        Source::Modrinth,
        &Query::search("sodium"),
        false,
        false,
        NOW * 1000,
    );
    let names: Vec<&str> = view.mods.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Sodium", "Fabric API"]);
    assert_eq!(view.note, None);
    assert!(!view.cached);
    assert!(!view.more);
    assert_eq!(view.mods[0].author, "jellysquid3");
    assert!(view.mods[0].page_url.starts_with("https://modrinth.com/"));
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[test]
fn offline_only_shows_the_cache_and_asks_nothing() {
    let dir = temp("offline");
    let (c, log) = fake_client(&dir);
    let q = Query::browse(Browse::Popular);
    let empty = search(
        &c,
        "minecraft",
        Source::Modrinth,
        &q,
        false,
        true,
        NOW * 1000,
    );
    assert!(empty.mods.is_empty());
    assert_eq!(empty.note.as_deref(), Some(OFFLINE_NOTE));
    assert!(log.lock().unwrap().is_empty());
    // Seen once online, the page stays while Offline only is on.
    search(
        &c,
        "minecraft",
        Source::Modrinth,
        &q,
        false,
        false,
        NOW * 1000,
    );
    let kept = search(
        &c,
        "minecraft",
        Source::Modrinth,
        &q,
        false,
        true,
        NOW * 1000,
    );
    assert_eq!(kept.mods.len(), 2);
    assert!(kept.cached);
    assert_eq!(kept.note.as_deref(), Some(OFFLINE_NOTE));
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[test]
fn no_new_finds_says_when_loadout_looks() {
    let dir = temp("finds");
    let view = new_finds(&dir, "stardew", Source::Nexus, 0, false, NOW);
    assert!(view.mods.is_empty());
    assert_eq!(view.note.as_deref(), Some(NO_FINDS));
    assert_eq!(Store::load(&dir).viewed.get("stardew"), Some(&NOW));
}

#[test]
fn for_you_ranks_the_cached_pools_and_not_for_me_sticks() {
    let dir = temp("foryou");
    let (c, _) = fake_client(&dir);
    let r = rail::mod_rail(&[], &[], true);
    // Minecraft is on the rail but never played: nothing to suggest.
    assert!(for_you(&dir, &c, &r, false, None, NOW).is_empty());

    let mut r = r;
    r.games[0].last_played = NOW - 86_400;
    r.games[0].playtime_min = 600;
    c.search(
        "minecraft",
        Source::Modrinth,
        &Query::browse(Browse::Popular),
        NOW * 1000,
    )
    .unwrap();
    let rows = for_you_rows(&r, &for_you(&dir, &c, &r, false, None, NOW));
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|s| s.game == "minecraft"
        && s.game_name == "Minecraft: Java Edition"
        && s.source == "modrinth"
        && s.tab == "popular"));

    let first = rows[0].m.id.clone();
    change_prefs(&dir, |p| {
        p.not_interested
            .insert(suggest::key(Source::Modrinth, &first));
    })
    .unwrap();
    let after = for_you_rows(&r, &for_you(&dir, &c, &r, false, None, NOW));
    assert_eq!(after.len(), 1);
    assert_ne!(after[0].m.id, first);

    change_prefs(&dir, |p| {
        p.hidden_games.insert("minecraft".into());
    })
    .unwrap();
    assert!(for_you(&dir, &c, &r, false, None, NOW).is_empty());
}

#[test]
fn a_workshop_link_opens_its_web_page() {
    assert_eq!(
        web_page("steam://url/CommunityFilePage/123"),
        "https://steamcommunity.com/sharedfiles/filedetails/?id=123"
    );
    assert_eq!(
        web_page("steam://url/CommunityFilePage/1;rm"),
        "steam://url/CommunityFilePage/1;rm"
    );
    assert_eq!(
        web_page("https://modrinth.com/mod/sodium"),
        "https://modrinth.com/mod/sodium"
    );
}

#[test]
fn tests_never_build_a_dialing_client() {
    let dir = temp("client");
    let c = client(&dir);
    let err = c
        .search(
            "minecraft",
            Source::Modrinth,
            &Query::browse(Browse::Popular),
            NOW * 1000,
        )
        .unwrap_err();
    assert!(matches!(err, SourceError::Net(_)), "{err:?}");
}
