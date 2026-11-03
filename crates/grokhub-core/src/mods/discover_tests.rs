use super::*;
use crate::mods::source::Keys;
use crate::mods::testing::{fixture, temp_config, Fake};

const NOW: u64 = 1_800_000_000;

fn game(id: &str, sources: &[Source]) -> GameRef {
    GameRef {
        id: id.into(),
        sources: sources.to_vec(),
    }
}

fn summary(id: &str, downloads: u64) -> ModSummary {
    ModSummary {
        id: id.into(),
        name: format!("Mod {id}"),
        downloads,
        ..Default::default()
    }
}

fn page(ids: &[(&str, u64)]) -> Page {
    Page {
        mods: ids.iter().map(|(id, d)| summary(id, *d)).collect(),
        total: None,
    }
}

fn shape(groups: &[Group]) -> Vec<(String, Reason, usize)> {
    groups
        .iter()
        .map(|g| (g.game.clone(), g.reason, g.jobs.len()))
        .collect()
}

#[test]
fn a_new_game_gets_every_browse_page_on_each_source() {
    let games = [game("stardew", &[Source::Nexus, Source::CurseForge])];
    let groups = plan(&games, &Store::default(), NOW);
    assert_eq!(shape(&groups), [("stardew".into(), Reason::NewGame, 6)]);
    assert_eq!(
        groups[0].jobs[..3],
        [
            (Source::Nexus, Query::browse(Browse::Popular)),
            (Source::Nexus, Query::browse(Browse::Trending)),
            (Source::Nexus, Query::browse(Browse::Updated)),
        ]
    );
}

#[test]
fn seen_games_get_top_mods_weekly_and_one_deeper_page_daily() {
    let games = [game("stardew", &[Source::Nexus])];
    let mut store = Store::default();
    let new = plan(&games, &store, NOW).remove(0);
    store.finish(&new, NOW);
    assert!(
        plan(&games, &store, NOW + 60).is_empty(),
        "nothing right after"
    );
    let day = plan(&games, &store, NOW + DAY);
    assert_eq!(shape(&day), [("stardew".into(), Reason::Explore, 1)]);
    let mut p2 = Query::browse(Browse::Popular);
    p2.page = 1;
    assert_eq!(day[0].jobs, [(Source::Nexus, p2)]);
    store.finish(&day[0], NOW + DAY);
    let mut p3 = Query::browse(Browse::Popular);
    p3.page = 2;
    assert_eq!(
        plan(&games, &store, NOW + 2 * DAY)[0].jobs,
        [(Source::Nexus, p3)],
        "the next explore goes one page deeper"
    );
    let week = plan(&games, &store, NOW + WEEKLY_SECS);
    assert_eq!(shape(&week), [("stardew".into(), Reason::Weekly, 2)]);
    assert_eq!(
        week[0].jobs,
        [
            (Source::Nexus, Query::browse(Browse::Popular)),
            (Source::Nexus, Query::browse(Browse::Trending)),
        ]
    );
}

#[test]
fn explore_walks_deeper_pages_then_updated_then_the_next_source() {
    let both = [Source::Nexus, Source::CurseForge];
    let got: Vec<(Source, Browse, u32)> = (0..8)
        .map(|step| {
            let (s, q) = explore_query(step, &both);
            (s, q.browse, q.page)
        })
        .collect();
    assert_eq!(
        got,
        [
            (Source::Nexus, Browse::Popular, 1),
            (Source::Nexus, Browse::Popular, 2),
            (Source::Nexus, Browse::Popular, 3),
            (Source::Nexus, Browse::Popular, 4),
            (Source::Nexus, Browse::Updated, 0),
            (Source::Nexus, Browse::Updated, 1),
            (Source::CurseForge, Browse::Popular, 1),
            (Source::CurseForge, Browse::Popular, 2),
        ]
    );
}

#[test]
fn a_sweep_stays_inside_its_budget_new_games_first() {
    let mut store = Store::default();
    store.seen.insert("old".into(), NOW - 30 * DAY);
    store.weekly.insert("old".into(), NOW - 30 * DAY);
    let games = [
        game("old", &[Source::Nexus]),
        game("a", &[Source::Nexus, Source::CurseForge]),
        game("b", &[Source::Nexus]),
        game("c", &[Source::Nexus]),
        game("nokey", &[]),
    ];
    let groups = plan(&games, &store, NOW);
    assert_eq!(
        shape(&groups),
        [
            ("a".into(), Reason::NewGame, 6),
            ("old".into(), Reason::Weekly, 1 + 1),
        ],
        "b and c wait: whole groups only, {MAX_REQUESTS} requests at most"
    );
    let total: usize = groups.iter().map(|g| g.jobs.len()).sum();
    assert!(total <= MAX_REQUESTS);
}

#[test]
fn finds_keep_first_seen_and_drop_the_stalest_over_the_cap() {
    let mut store = Store::default();
    store.record("stardew", Source::Nexus, &page(&[("1", 10)]), false, NOW);
    store.record(
        "stardew",
        Source::Nexus,
        &page(&[("1", 20), ("2", 5)]),
        true,
        NOW + DAY,
    );
    let f = &store.finds["stardew"];
    assert_eq!(f.len(), 2);
    assert_eq!((f[0].first_seen, f[0].last_seen), (NOW, NOW + DAY));
    assert_eq!(f[0].m.downloads, 20, "refreshed");
    assert!(f[0].trending && f[1].trending);
    // Same id on another source is another mod.
    store.record(
        "stardew",
        Source::CurseForge,
        &page(&[("1", 1)]),
        false,
        NOW,
    );
    assert_eq!(store.finds["stardew"].len(), 3);

    let mut big = Store::default();
    let stale: Vec<(String, u64)> = (0..MAX_FINDS).map(|i| (format!("s{i}"), 1)).collect();
    let stale: Vec<(&str, u64)> = stale.iter().map(|(s, d)| (s.as_str(), *d)).collect();
    big.record("g", Source::Nexus, &page(&stale), false, NOW);
    big.record("g", Source::Nexus, &page(&[("fresh", 1)]), false, NOW + DAY);
    let kept = &big.finds["g"];
    assert_eq!(kept.len(), MAX_FINDS);
    assert!(kept.iter().any(|f| f.m.id == "fresh"));
}

#[test]
fn long_summaries_are_cut_on_disk() {
    let mut store = Store::default();
    let mut p = page(&[("1", 1)]);
    p.mods[0].summary = "x".repeat(500);
    store.record("g", Source::Nexus, &p, false, NOW);
    let s = &store.finds["g"][0].m.summary;
    assert_eq!(s.chars().count(), SUMMARY_CHARS);
    assert!(s.ends_with('…'));
}

#[test]
fn new_finds_are_newest_first_and_unseen_counts_since_the_last_look() {
    let mut store = Store::default();
    let mut adult = page(&[("a", 1)]);
    adult.mods[0].adult = true;
    store.record(
        "g",
        Source::Nexus,
        &page(&[("old", 9)]),
        false,
        NOW - NEW_SECS,
    );
    store.record(
        "g",
        Source::Nexus,
        &page(&[("x", 1), ("y", 5)]),
        false,
        NOW - DAY,
    );
    store.record("g", Source::Nexus, &page(&[("z", 1)]), false, NOW);
    store.record("g", Source::Nexus, &adult, false, NOW);
    let ids = |v: Vec<&Find>| v.iter().map(|f| f.m.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(store.new_finds("g", false, NOW)), ["z", "y", "x"]);
    assert_eq!(ids(store.new_finds("g", true, NOW)), ["a", "z", "y", "x"]);
    assert_eq!(store.unseen("g", false, NOW), 3);
    store.viewed.insert("g".into(), NOW - DAY);
    assert_eq!(store.unseen("g", false, NOW), 1);
    assert_eq!(store.unseen("nothing", false, NOW), 0);
}

#[test]
fn a_new_game_triggers_a_sweep_soon_but_not_every_frame() {
    let mut store = Store {
        last_sweep: NOW,
        ..Default::default()
    };
    let games = [game("stardew", &[Source::Nexus])];
    assert!(!sweep_due(&store, &games, NOW + 10));
    assert!(sweep_due(&store, &games, NOW + NEW_GAME_GAP_SECS));
    store.seen.insert("stardew".into(), NOW);
    assert!(!sweep_due(&store, &games, NOW + NEW_GAME_GAP_SECS));
    assert!(sweep_due(&store, &games, NOW + SWEEP_EVERY_SECS));
    assert!(!sweep_due(&store, &[game("nokey", &[])], NOW + 120));
}

#[test]
fn a_sweep_fetches_records_and_marks_a_new_game() {
    let fake = Fake::new().on("/v2/search", fixture("modrinth_search.json"));
    let sent = fake.log();
    let dir = temp_config("discover-sweep");
    let c = ModClient::new(Box::new(fake), &dir, Keys::default());
    let mc = GameRef::ready("minecraft", &c).expect("minecraft is allowed");
    assert_eq!(mc.sources, [Source::Modrinth], "CurseForge needs a key");
    let mut store = Store::default();
    let r = sweep(&c, &mut store, std::slice::from_ref(&mc), NOW * 1000);
    assert_eq!(r.pages, 3);
    assert_eq!(r.found, 2);
    assert_eq!(r.new_games, ["minecraft"]);
    assert_eq!(r.stopped, None);
    assert_eq!(sent.lock().unwrap().len(), 3);
    assert_eq!(store.seen.get("minecraft"), Some(&NOW));
    assert_eq!(store.last_sweep, NOW);
    let ids: Vec<&str> = store.finds["minecraft"]
        .iter()
        .map(|f| f.m.id.as_str())
        .collect();
    assert_eq!(ids, ["AANobbMI", "P7dR8mSH"]);
    assert!(store.finds["minecraft"][0].trending);
    // Nothing else is due straight after.
    let again = sweep(&c, &mut store, &[mc], NOW * 1000 + 1000);
    assert_eq!(again, Report::default());
    assert_eq!(sent.lock().unwrap().len(), 3);

    store.save(&dir).unwrap();
    assert_eq!(Store::load(&dir), store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn blocked_games_never_reach_the_network() {
    let fake = Fake::new();
    let sent = fake.log();
    let c = ModClient::new(
        Box::new(fake),
        &temp_config("discover-blocked"),
        Keys::default(),
    );
    assert_eq!(GameRef::ready("apex", &c), None);
    assert_eq!(GameRef::ready("not-a-game", &c), None);
    // Even a hand-made ref for a blocked game sends nothing.
    let mut store = Store::default();
    let r = sweep(
        &c,
        &mut store,
        &[game("apex", &[Source::Nexus])],
        NOW * 1000,
    );
    assert_eq!(r.pages, 0);
    assert!(sent.lock().unwrap().is_empty());
}

#[test]
fn a_rate_limit_ends_the_sweep_and_leaves_the_game_for_next_time() {
    let fake = Fake::new().reply(
        "/v2/search",
        crate::mods::source::Reply {
            status: 429,
            headers: vec![("retry-after".into(), "30".into())],
            ..Default::default()
        },
    );
    let c = ModClient::new(
        Box::new(fake),
        &temp_config("discover-429"),
        Keys::default(),
    );
    let mc = GameRef::ready("minecraft", &c).unwrap();
    let mut store = Store::default();
    let r = sweep(&c, &mut store, &[mc], NOW * 1000);
    assert!(r.stopped.is_some());
    assert!(r.new_games.is_empty());
    assert!(
        !store.seen.contains_key("minecraft"),
        "tried again next sweep"
    );
}
