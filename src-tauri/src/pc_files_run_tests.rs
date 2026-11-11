use super::*;
use grokhub_core::downloads_tidy::{FileInfo, Kind};
use grokhub_core::print_alerts::{PrintState, PrinterStatus};
use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};
use std::cell::{Cell, RefCell};

const NOW: u64 = 1_800_000_000_000;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "grokhub-pc-files-run-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn finding(name: &str, bytes: u64) -> Finding {
    Finding {
        file: FileInfo {
            path: PathBuf::from(name),
            name: name.into(),
            bytes,
            modified_ms: 0,
        },
        kind: Kind::Installer,
        why: "installed already".into(),
    }
}

const BIG: u64 = 200 * 1024 * 1024;

#[test]
fn the_weekly_tidy_posts_one_card_then_waits_a_week() {
    let c = dir("tidy-week");
    let scans = Cell::new(0);
    let scan = || {
        scans.set(scans.get() + 1);
        vec![finding("setup.exe", BIG)]
    };
    assert!(tidy_tick(&c, NOW, &scan));
    assert_eq!(scans.get(), 1);
    assert_eq!(dt::load(&c).last_check_ms, NOW);
    let cards = crate::pulse::cards(&c).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "200 MB of old installers in Downloads");
    assert!(!tidy_tick(&c, NOW + 6 * 24 * 3_600_000, &scan));
    assert_eq!(scans.get(), 1, "not due again until a week has passed");
    assert!(tidy_tick(&c, NOW + 8 * 24 * 3_600_000, &|| vec![finding(
        "b.exe", BIG
    )]));
}

#[test]
fn a_small_tidy_result_posts_nothing_but_counts_as_checked() {
    let c = dir("tidy-small");
    assert!(!tidy_tick(&c, NOW, &|| vec![finding("tiny.exe", 1024)]));
    assert_eq!(dt::load(&c).last_check_ms, NOW);
    assert!(crate::pulse::cards(&c).unwrap().is_empty());
}

fn downloads_with_a_duplicate() -> PathBuf {
    let d = dir("downloads");
    std::fs::write(d.join("a.bin"), b"same bytes").unwrap();
    std::fs::write(d.join("a (1).bin"), b"same bytes").unwrap();
    std::fs::write(d.join("keep.txt"), b"mine").unwrap();
    d
}

#[test]
fn reviewing_moves_nothing_and_trashing_takes_only_current_suggestions() {
    let d = downloads_with_a_duplicate();
    let found = scan_dir(&d, &[], NOW);
    let shown = rows(&found);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].name, "a (1).bin");
    assert_eq!(shown[0].kind, "Duplicate");
    assert_eq!(shown[0].why, "Same file as a.bin");
    assert_eq!(shown[0].bytes, 10);
    for name in ["a.bin", "a (1).bin", "keep.txt"] {
        assert!(d.join(name).exists(), "{name} is untouched by a review");
    }

    let sent: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
    let trash = |paths: &[PathBuf]| {
        sent.borrow_mut().extend_from_slice(paths);
        Ok(())
    };
    let err = trash_from(&d, &[], &["keep.txt".into(), "a.bin".into()], &trash).unwrap_err();
    assert_eq!(
        err,
        "None of those files are suggestions any more. Nothing was moved."
    );
    assert!(
        sent.borrow().is_empty(),
        "files that aren't suggestions never reach the Trash"
    );

    let done = trash_from(&d, &[], &["keep.txt".into(), "a (1).bin".into()], &trash).unwrap();
    assert_eq!(done.moved, 1);
    assert_eq!(done.note, "Moved 1 file (1 KB) to the Trash.");
    assert_eq!(*sent.borrow(), vec![d.join("a (1).bin")]);
}

#[test]
fn a_failed_trash_reports_and_changes_nothing() {
    let d = downloads_with_a_duplicate();
    let err = trash_from(&d, &[], &["a (1).bin".into()], &|_| {
        Err("no trash here".into())
    })
    .unwrap_err();
    assert_eq!(err, "no trash here");
    assert!(d.join("a (1).bin").exists());
}

#[test]
fn model_drop_offers_a_new_model_once_and_ignores_what_was_already_there() {
    let c = dir("models-cfg");
    let d = dir("models-dl");
    std::fs::write(d.join("old.stl"), b"x").unwrap();
    std::fs::write(d.join("notes.txt"), b"x").unwrap();
    let mut seen = BTreeSet::new();
    assert_eq!(
        models_tick(&c, &d, &mut seen, true, NOW, None),
        0,
        "the first look only records"
    );
    assert_eq!(models_tick(&c, &d, &mut seen, false, NOW + 20_000, None), 0);
    std::fs::write(d.join("benchy.stl"), b"x").unwrap();
    std::fs::write(d.join("readme.md"), b"x").unwrap();
    assert_eq!(models_tick(&c, &d, &mut seen, false, NOW + 40_000, None), 1);
    assert_eq!(
        models_tick(&c, &d, &mut seen, false, NOW + 60_000, None),
        0,
        "once"
    );
    let cards = crate::pulse::cards(&c).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "New model: benchy.stl");
}

fn run_git_in(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?}");
}

#[test]
fn code_resume_reads_a_repo_with_an_unpushed_commit() {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let root = dir("resume");
    let remote = root.join("remote.git");
    let work = root.join("code").join("proj");
    std::fs::create_dir_all(&work).unwrap();
    run_git_in(
        &root,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    run_git_in(&work, &["init", "-b", "main"]);
    std::fs::write(work.join("a.txt"), "1").unwrap();
    run_git_in(&work, &["add", "."]);
    run_git_in(&work, &["commit", "-m", "one"]);
    run_git_in(
        &work,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    run_git_in(&work, &["push", "-u", "origin", "main"]);
    std::fs::write(work.join("a.txt"), "2").unwrap();
    run_git_in(&work, &["commit", "-am", "two"]);
    let repos = scan_repos(
        &[root.join("code").to_string_lossy().to_string()],
        NOW / 1000,
    );
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].name, "proj");
    assert_eq!(repos[0].ahead, 1, "one commit isn't pushed");
    assert!(repos[0].pending());
}

fn reminder() -> Reminder {
    Reminder {
        lines: vec!["proj has 2 commits not pushed for 5 days".into()],
        push: None,
        tool: None,
    }
}

#[test]
fn the_backup_check_posts_once_a_day_and_never_while_snoozed() {
    let c = dir("backup");
    let asked = Cell::new(0);
    let check = || {
        asked.set(asked.get() + 1);
        Some(reminder())
    };
    assert!(backup_tick(&c, NOW, &check));
    let state = bn::load(&c);
    assert_eq!(state.last_shown_ms, NOW);
    assert!(state.reminder.is_some());
    assert!(!backup_tick(&c, NOW + 3_600_000, &check));
    assert_eq!(
        asked.get(),
        1,
        "a second look the same day doesn't even check"
    );
    let mut s = bn::load(&c);
    s.snooze(NOW + 25 * 3_600_000);
    bn::save(&c, &s).unwrap();
    assert!(!backup_tick(&c, NOW + 26 * 3_600_000, &check));
    assert_eq!(asked.get(), 1);
}

fn status(state: PrintState) -> PrinterStatus {
    PrinterStatus {
        name: "voron".into(),
        kind: PrinterKind::Moonraker,
        state,
        progress_pct: Some(100.0),
        eta_secs: None,
        filename: Some("benchy.gcode".into()),
        message: None,
        camera_path: None,
    }
}

fn printer_setup(name: &str) -> PathBuf {
    let c = dir(name);
    crate::pc_files::set_switch(&c, "printAlerts", true).unwrap();
    crate::pc_files::save_printer(
        &c,
        SavedPrinter {
            endpoint: "192.168.1.50:7125".into(),
            kind: PrinterKind::Moonraker,
            name: "voron".into(),
            enabled: true,
            api_key: None,
        },
    )
    .unwrap();
    c
}

#[test]
fn a_print_that_finishes_posts_one_card_and_the_camera_stays_off_unless_asked() {
    let c = printer_setup("print");
    let snaps = Cell::new(0);
    let snap = |_: &SavedPrinter| {
        snaps.set(snaps.get() + 1);
        Some("http://192.168.1.50:7125/cam.jpg".to_string())
    };
    let now = Cell::new(PrintState::Printing);
    let fetch = |_: &SavedPrinter| Some(status(now.get()));
    assert_eq!(
        print_tick(&c, NOW, &fetch, &snap),
        0,
        "printing is not an alert"
    );
    now.set(PrintState::Complete);
    assert_eq!(print_tick(&c, NOW + 15_000, &fetch, &snap), 1);
    assert_eq!(snaps.get(), 0, "no camera frame without the camera switch");
    assert_eq!(
        print_tick(&c, NOW + 30_000, &fetch, &snap),
        0,
        "one alert per finish"
    );
    let cards = crate::pulse::cards(&c).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "voron finished a print");
    assert!(cards[0].pulse.image_urls.is_empty());
}

#[test]
fn a_failed_print_with_the_camera_on_carries_the_snapshot() {
    let c = printer_setup("print-cam");
    crate::pc_files::set_switch(&c, "printCamera", true).unwrap();
    let now = Cell::new(PrintState::Printing);
    let fetch = |_: &SavedPrinter| Some(status(now.get()));
    let snap = |_: &SavedPrinter| Some("http://192.168.1.50:7125/cam.jpg".to_string());
    print_tick(&c, NOW, &fetch, &snap);
    now.set(PrintState::Error);
    assert_eq!(print_tick(&c, NOW + 15_000, &fetch, &snap), 1);
    let cards = crate::pulse::cards(&c).unwrap();
    assert_eq!(
        cards[0].pulse.image_urls,
        vec!["http://192.168.1.50:7125/cam.jpg".to_string()]
    );
}

#[test]
fn nothing_is_polled_while_the_watch_is_off_or_the_printer_is_off() {
    let c = printer_setup("print-off");
    let fetch = |_: &SavedPrinter| -> Option<PrinterStatus> { panic!("polled") };
    let snap = |_: &SavedPrinter| None;
    crate::pc_files::set_printer_enabled(&c, "192.168.1.50:7125", false).unwrap();
    assert_eq!(print_tick(&c, NOW, &fetch, &snap), 0);
    crate::pc_files::set_printer_enabled(&c, "192.168.1.50:7125", true).unwrap();
    crate::pc_files::set_switch(&c, "printAlerts", false).unwrap();
    assert_eq!(print_tick(&c, NOW, &fetch, &snap), 0);
}

#[test]
fn only_lan_addresses_are_ever_requested() {
    assert!(on_lan("192.168.1.50:7125"));
    assert!(on_lan("http://127.0.0.1:5000/"));
    assert!(!on_lan("8.8.8.8:80"));
    assert!(!on_lan("printer.local:80"));
    let agent = agent();
    assert!(get_text(&agent, "http://8.8.8.8/api/job", None).is_none());
}

#[test]
fn every_job_stays_idle_while_its_switch_is_off_and_while_the_cabin_runs() {
    let c = dir("beat");
    let mut run = Run::default();
    assert_eq!(beat(&c, 7, NOW, &mut run), Did::default());
    assert!(!run.tidy_checked && !run.backup_checked && !run.models_first_done);
    crate::pc_files::set_switch(&c, "backupNudge", true).unwrap();
    write_lease(
        &c,
        &SchedulerLease {
            owner: LeaseOwner::Cabin,
            pid: 99,
            beat_ms: NOW,
        },
    )
    .unwrap();
    assert_eq!(beat(&c, 7, NOW, &mut run), Did::default());
    assert!(!run.backup_checked, "the cabin's fresh beat wins");
}
