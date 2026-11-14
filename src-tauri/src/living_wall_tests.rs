use std::cell::Cell;
use std::path::{Path, PathBuf};

use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};
use grokhub_core::{ImagineWall, WALL_GIF_EVERY_MS, WALL_SEEDS};

use super::*;

/// 2026-10-07 12:00 UTC.
const NOON: u64 = 1_791_374_400_000;
const PID: u32 = 4242;

struct Dir(PathBuf);

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let root = std::env::temp_dir().join(format!(
        "shell-living-wall-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    Dir(root)
}

/// The switch on, as the cabin saved it.
fn wall_on(config: &Path) {
    std::fs::write(config.join("app.json"), r#"{"imagineWall": true}"#).unwrap();
}

fn cx(config: &Path, now: u64) -> Cx<'_> {
    Cx {
        config_dir: config,
        pid: PID,
        now,
        offset_min: Some(0),
        busy: false,
        signed_in: true,
    }
}

/// A paint that writes both frames into the wall folder and counts its calls.
fn painter<'a>(
    config: &'a Path,
    calls: &'a Cell<usize>,
) -> impl Fn(&WallSeed, &str, &dyn Fn() -> bool) -> Result<WallGif, String> + 'a {
    move |seed, id, _stop| {
        calls.set(calls.get() + 1);
        let wall = crate::imagine::wall_dir(config);
        std::fs::create_dir_all(&wall).unwrap();
        let a = wall.join(format!("{id}_a.png"));
        let b = wall.join(format!("{id}_b.png"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        Ok(WallGif {
            id: id.to_string(),
            title: seed.title.to_string(),
            prompt: seed.prompt.to_string(),
            created_ms: NOON,
            path_a: a.display().to_string(),
            path_b: b.display().to_string(),
            tall: seed.tall,
        })
    }
}

fn wall(config: &Path) -> ImagineWall {
    let raw = std::fs::read_to_string(config.join("imagine-wall.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn seed_wall(config: &Path, last_ms: u64, gifs: Vec<WallGif>) {
    let raw = serde_json::to_string(&ImagineWall { last_ms, gifs }).unwrap();
    std::fs::write(config.join("imagine-wall.json"), raw).unwrap();
}

fn cover(i: u64, wall: &Path) -> WallGif {
    let a = wall.join(format!("gen-{i}_a.png"));
    std::fs::write(&a, b"old").unwrap();
    WallGif {
        id: format!("gen-{i}"),
        title: format!("Old {i}"),
        prompt: "old".into(),
        created_ms: i,
        path_a: a.display().to_string(),
        path_b: a.display().to_string(),
        tall: false,
    }
}

#[test]
fn off_until_turned_on_and_a_cabin_on_keeps_painting() {
    let config = dir("switch");
    assert_eq!(view(&config), View { on: false });
    let calls = Cell::new(0);
    assert_eq!(
        look(&cx(&config, NOON), &painter(&config, &calls)),
        Look::Held(Hold::Off)
    );
    assert_eq!(calls.get(), 0);
    assert!(!config.join("imagine-wall.json").exists());

    std::fs::write(
        config.join("app.json"),
        r#"{"imagineWall": true, "theme": "dark"}"#,
    )
    .unwrap();
    assert!(is_on(&config));
    assert_eq!(set(&config, false).unwrap(), View { on: false });
    let raw = std::fs::read_to_string(config.join("app.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(saved["imagineWall"], json!(false));
    assert_eq!(saved["theme"], json!("dark"));
    assert_eq!(set(&config, true).unwrap(), View { on: true });
}

#[test]
fn the_first_look_paints_a_fresh_seed_and_pins_it() {
    let config = dir("first");
    wall_on(&config);
    let calls = Cell::new(0);
    let Look::Painted(gif) = look(&cx(&config, NOON), &painter(&config, &calls)) else {
        panic!("expected a cover");
    };
    assert_eq!(calls.get(), 1);
    assert_eq!(gif.id, format!("{NOON:x}"));
    let titles: Vec<&str> = WALL_SEEDS.iter().map(|s| s.title).collect();
    assert!(titles.contains(&gif.title.as_str()), "{}", gif.title);
    let saved = wall(&config);
    assert_eq!(saved.last_ms, NOON);
    assert_eq!(saved.gifs, vec![gif]);
}

#[test]
fn a_title_already_on_the_wall_is_not_painted_again() {
    let config = dir("fresh");
    wall_on(&config);
    let roll = grokhub_core::pick_fresh_seed(NOON, &[]);
    let wdir = crate::imagine::wall_dir(&config);
    std::fs::create_dir_all(&wdir).unwrap();
    let mut old = cover(1, &wdir);
    old.title = roll.title.into();
    seed_wall(&config, NOON - WALL_GIF_EVERY_MS, vec![old]);
    let calls = Cell::new(0);
    let Look::Painted(gif) = look(&cx(&config, NOON), &painter(&config, &calls)) else {
        panic!("expected a cover");
    };
    assert_ne!(gif.title, roll.title);
}

#[test]
fn the_next_cover_waits_three_hours_and_quiet_hours_hold_it() {
    let config = dir("due");
    wall_on(&config);
    seed_wall(&config, NOON, Vec::new());
    let calls = Cell::new(0);
    let paint = painter(&config, &calls);
    let almost = NOON + WALL_GIF_EVERY_MS - 1;
    assert_eq!(look(&cx(&config, almost), &paint), Look::Held(Hold::NotDue));
    assert_eq!(calls.get(), 0);

    let mut settings = crate::settings::load(&config);
    settings.behavior.quiet_start = "00:00".into();
    settings.behavior.quiet_end = "23:59".into();
    crate::settings::save(&config, &settings).unwrap();
    let due = NOON + WALL_GIF_EVERY_MS;
    assert_eq!(look(&cx(&config, due), &paint), Look::Held(Hold::Quiet));
    assert_eq!(calls.get(), 0);

    settings.behavior.quiet_start.clear();
    settings.behavior.quiet_end.clear();
    crate::settings::save(&config, &settings).unwrap();
    assert!(matches!(look(&cx(&config, due), &paint), Look::Painted(_)));
    assert_eq!(calls.get(), 1);
    assert_eq!(wall(&config).last_ms, due);
}

#[test]
fn each_hold_paints_nothing() {
    let config = dir("holds");
    wall_on(&config);
    let calls = Cell::new(0);
    let paint = painter(&config, &calls);
    let held = |cx: &Cx<'_>| look(cx, &paint);

    assert_eq!(
        held(&Cx {
            busy: true,
            ..cx(&config, NOON)
        }),
        Look::Held(Hold::Busy)
    );
    assert_eq!(
        held(&Cx {
            signed_in: false,
            ..cx(&config, NOON)
        }),
        Look::Held(Hold::NoSignIn)
    );
    assert_eq!(
        held(&Cx {
            offset_min: None,
            ..cx(&config, NOON)
        }),
        Look::Held(Hold::NoClock)
    );

    let mut settings = crate::settings::load(&config);
    settings.last_chat = grokhub_agent::scratch::shell_id(true, "1");
    crate::settings::save(&config, &settings).unwrap();
    assert_eq!(held(&cx(&config, NOON)), Look::Held(Hold::Scratch));
    settings.last_chat = grokhub_agent::scratch::shell_id(false, "1");
    crate::settings::save(&config, &settings).unwrap();

    let halt = config.join(grokhub_core::desktop_mcp::HALT_STAMP_FILE);
    std::fs::write(&halt, (NOON - 60_000).to_string()).unwrap();
    assert_eq!(held(&cx(&config, NOON)), Look::Held(Hold::Halted));
    std::fs::remove_file(&halt).unwrap();

    write_lease(
        &config,
        &SchedulerLease {
            owner: LeaseOwner::Cabin,
            pid: 99,
            beat_ms: NOON - 1_000,
        },
    )
    .unwrap();
    assert_eq!(held(&cx(&config, NOON)), Look::Held(Hold::CabinOpen));
    std::fs::remove_file(config.join(grokhub_core::scheduler_lease::LEASE_FILE)).unwrap();

    {
        let _making = crate::imagine::start().unwrap();
        assert_eq!(held(&cx(&config, NOON)), Look::Held(Hold::Making));
    }
    assert_eq!(calls.get(), 0);
    assert!(!config.join("imagine-wall.json").exists());
    // Every hold cleared: it paints.
    assert!(matches!(held(&cx(&config, NOON)), Look::Painted(_)));
    assert_eq!(calls.get(), 1);
}

#[test]
fn twenty_covers_stay_and_the_oldest_leaves_with_its_file() {
    let config = dir("evict");
    wall_on(&config);
    let wdir = crate::imagine::wall_dir(&config);
    std::fs::create_dir_all(&wdir).unwrap();
    let old: Vec<WallGif> = (0..20).map(|i| cover(i, &wdir)).collect();
    seed_wall(&config, NOON - WALL_GIF_EVERY_MS, old);
    let calls = Cell::new(0);
    assert!(matches!(
        look(&cx(&config, NOON), &painter(&config, &calls)),
        Look::Painted(_)
    ));
    let saved = wall(&config);
    assert_eq!(saved.gifs.len(), 20);
    assert_eq!(saved.gifs[0].id, "gen-1");
    assert_eq!(saved.gifs[19].id, format!("{NOON:x}"));
    assert!(!wdir.join("gen-0_a.png").exists());
    assert!(wdir.join("gen-1_a.png").exists());
}

#[test]
fn a_failed_cover_tries_again_in_fifteen_minutes() {
    let config = dir("failed");
    wall_on(&config);
    let fail = |_: &WallSeed, _: &str, _: &dyn Fn() -> bool| -> Result<WallGif, String> {
        Err("HTTP 500".into())
    };
    assert_eq!(
        look(&cx(&config, NOON), &fail),
        Look::Failed("HTTP 500".into())
    );
    let saved = wall(&config);
    assert_eq!(saved.last_ms, NOON - WALL_GIF_EVERY_MS + 15 * 60 * 1000);
    assert!(saved.gifs.is_empty());

    let calls = Cell::new(0);
    let paint = painter(&config, &calls);
    let soon = NOON + 15 * 60 * 1000;
    assert_eq!(
        look(&cx(&config, soon - 1), &paint),
        Look::Held(Hold::NotDue)
    );
    assert!(matches!(look(&cx(&config, soon), &paint), Look::Painted(_)));
    // The failed run let go of Imagine: a Make can start.
    assert!(crate::imagine::start().is_ok());
}
