use std::path::{Path, PathBuf};
use std::sync::Arc;

use grokhub_agent::harness::{use_key_store_for, MemoryKeyStore};
use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};

use super::*;

/// 2026-10-07 12:00 UTC.
const NOON: u64 = 1_791_374_400_000;
const PID: u32 = 4242;
const HOME: &str = "/home/you";

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
        "shell-indexers-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    use_key_store_for(&root, Arc::new(MemoryKeyStore::new()));
    Dir(root)
}

struct Power(bool);

impl PowerSource for Power {
    fn on_battery(&self) -> bool {
        self.0
    }
}

/// A notes folder with one file and a granted `files` scope on it.
fn granted_notes(config: &Path) -> PathBuf {
    let notes = config.join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(
        notes.join("plan.md"),
        "# Zebra Plan Fixture\n\nbody\n## Budget Fixture\n",
    )
    .unwrap();
    let said = crate::scopes::allow(
        config,
        Some(Path::new(HOME)),
        "files",
        &notes.display().to_string(),
    );
    assert!(said.starts_with("Files in notes allowed."), "{said}");
    notes
}

fn look(driver: &mut Driver, config: &Path, now: u64, busy: bool, battery: bool) -> Step {
    let dirs = PlatformDirs::from_env();
    let power = Power(battery);
    driver.step(
        &Cx {
            config_dir: config,
            pid: PID,
            now,
            offset_min: Some(0),
            busy,
        },
        &Machine {
            power: &power,
            fs: &RealFs,
            probe: &OsProbe,
            dirs: &dirs,
        },
    )
}

/// Chat opens `id`, as the window's chat load does.
fn open_chat(config: &Path, id: &str) {
    let mut settings = crate::settings::load(config);
    settings.last_chat = id.into();
    crate::settings::save(config, &settings).unwrap();
}

fn facts(config: &Path) -> usize {
    grokhub_agent::indexers::ScopeIndex::load(config)
        .facts
        .values()
        .map(Vec::len)
        .sum()
}

#[test]
fn a_granted_folder_is_read_once_and_permissions_shows_it() {
    let config = dir("read");
    granted_notes(&config);
    let mut driver = Driver::default();
    let first = look(&mut driver, &config, NOON, false, false);
    let Step::Ticked(TickOutcome::Ran { kind, written, .. }) = &first else {
        panic!("expected a read, got {first:?}");
    };
    assert_eq!((*kind, *written), ("files", 1));
    assert!(first.learned());
    assert_eq!(facts(&config), 1);
    let shown = crate::scopes::view(&config, NOON);
    let files = shown.rows.iter().find(|r| r.kind == "files").unwrap();
    let lines = &files.granted[0].learned.lines;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("Zebra Plan Fixture")));

    // The scope was just read: nothing is due for 10 minutes.
    let next = look(&mut driver, &config, NOON + 60_000, false, false);
    assert_eq!(next, Step::Ticked(TickOutcome::Idle));
    assert!(!next.learned());
}

#[test]
fn each_hold_reads_nothing() {
    let config = dir("holds");
    granted_notes(&config);
    let mut driver = Driver::default();
    assert_eq!(
        look(&mut driver, &config, NOON, true, false),
        Step::Held(Hold::Busy)
    );
    open_chat(&config, &grokhub_agent::scratch::shell_id(true, "1"));
    assert_eq!(
        look(&mut driver, &config, NOON, false, false),
        Step::Held(Hold::Scratch)
    );
    // Back in a plain chat, even with the scratch chat's turn saved last.
    {
        let _config = grokhub_agent::perm::ConfigGuard::set(&*config);
        let say = |role: &str, text: &str| grokhub_agent::InputItem::Message {
            role: role.into(),
            content: vec![grokhub_agent::ContentPart::InputText(text.into())],
        };
        grokhub_agent::record_turn(
            &grokhub_agent::scratch::shell_id(true, "1"),
            HOME,
            "grok-test",
            &[say("user", "try this"), say("assistant", "Done.")],
            &grokhub_agent::Usage::default(),
            "",
            "try this",
        )
        .unwrap();
    }
    open_chat(&config, &grokhub_agent::scratch::shell_id(false, "1"));

    std::fs::write(
        config.join(grokhub_core::desktop_mcp::HALT_STAMP_FILE),
        (NOON - 60_000).to_string(),
    )
    .unwrap();
    assert_eq!(
        look(&mut driver, &config, NOON, false, false),
        Step::Held(Hold::Halted)
    );
    std::fs::remove_file(config.join(grokhub_core::desktop_mcp::HALT_STAMP_FILE)).unwrap();

    write_lease(
        &config,
        &SchedulerLease {
            owner: LeaseOwner::Cabin,
            pid: 99,
            beat_ms: NOON - 1_000,
        },
    )
    .unwrap();
    assert_eq!(
        look(&mut driver, &config, NOON, false, false),
        Step::Held(Hold::CabinOpen)
    );
    std::fs::remove_file(config.join(grokhub_core::scheduler_lease::LEASE_FILE)).unwrap();

    let dirs = PlatformDirs::from_env();
    let no_clock = driver.step(
        &Cx {
            config_dir: &config,
            pid: PID,
            now: NOON,
            offset_min: None,
            busy: false,
        },
        &Machine {
            power: &Power(false),
            fs: &RealFs,
            probe: &OsProbe,
            dirs: &dirs,
        },
    );
    assert_eq!(no_clock, Step::Held(Hold::NoClock));
    assert_eq!(facts(&config), 0, "a held look reads nothing");

    // Nothing holds now, and the first read happens.
    let read = look(&mut driver, &config, NOON, false, false);
    assert!(read.learned(), "{read:?}");
}

#[test]
fn battery_and_quiet_hours_come_from_the_scheduler() {
    let config = dir("quiet");
    granted_notes(&config);
    let mut driver = Driver::default();
    assert_eq!(
        look(&mut driver, &config, NOON, false, true),
        Step::Ticked(TickOutcome::OnBattery)
    );
    let mut settings = crate::settings::load(&config);
    settings.behavior.quiet_start = "11:00".into();
    settings.behavior.quiet_end = "13:00".into();
    crate::settings::save(&config, &settings).unwrap();
    assert_eq!(
        look(&mut driver, &config, NOON, false, false),
        Step::Ticked(TickOutcome::QuietHours)
    );
    assert_eq!(facts(&config), 0);
    // 13:30 is past quiet hours.
    let later = look(&mut driver, &config, NOON + 90 * 60_000, false, false);
    assert!(later.learned(), "{later:?}");
}

#[test]
fn only_written_facts_count_as_learned() {
    let ran = |written| {
        Step::Ticked(TickOutcome::Ran {
            kind: "files",
            written,
            known: 0,
            stopped: false,
            tables: vec![],
        })
    };
    assert!(ran(2).learned());
    assert!(!ran(0).learned());
    assert!(!Step::Ticked(TickOutcome::Paused("locked".into())).learned());
    assert!(!Step::Held(Hold::Busy).learned());
}
