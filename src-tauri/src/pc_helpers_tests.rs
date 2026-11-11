use super::*;
use grokhub_core::LocalClock;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "grokhub-pc-helpers-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_fresh_install_has_the_cabins_defaults() {
    let d = dir("defaults");
    let h = read(&d);
    assert!(!h.focus_help, "focus help starts off");
    assert!(!h.wind_down, "wind-down starts off");
    assert_eq!(h.auto_sleep_min, None);
    assert!(
        h.rhythm,
        "the rhythm learns until it is turned off, as in the cabin"
    );
    assert!(!h.rhythm_games);
    assert_eq!(h.rhythm_minutes, 0);
    assert_eq!(
        h.rhythm_summary,
        "Still learning. A few days of use fills this in."
    );
    let labels: Vec<_> = h
        .sleep_choices
        .iter()
        .map(|c| (c.minutes, c.label.as_str()))
        .collect();
    assert_eq!(
        labels,
        [
            (15, "15 min"),
            (30, "30 min"),
            (60, "1 hr"),
            (120, "2 hr"),
            (240, "4 hr")
        ]
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn every_switch_round_trips_in_the_cabins_files() {
    let d = dir("roundtrip");
    assert!(set_switch(&d, "focusHelp", true).unwrap().focus_help);
    assert!(set_switch(&d, "windDown", true).unwrap().wind_down);
    assert!(!set_switch(&d, "rhythm", false).unwrap().rhythm);
    assert!(set_switch(&d, "rhythmGames", true).unwrap().rhythm_games);
    // The cabin reads the same files.
    assert!(grokhub_core::app_json::bool_or(&d, "focusHelp", false));
    assert!(wind_down::load(&d).on);
    let r = rhythm::load(&d.join("rhythm.json"));
    assert!(!r.on && r.games_on);
    let h = read(&d);
    assert!(h.focus_help && h.wind_down && !h.rhythm && h.rhythm_games);
    for (key, now) in [
        ("focusHelp", false),
        ("windDown", false),
        ("rhythm", true),
        ("rhythmGames", false),
    ] {
        set_switch(&d, key, now).unwrap();
    }
    let h = read(&d);
    assert!(!h.focus_help && !h.wind_down && h.rhythm && !h.rhythm_games);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn focus_help_leaves_the_rest_of_app_json_alone() {
    let d = dir("keys");
    std::fs::write(
        d.join("app.json"),
        r#"{"quietStart":"22:30","suggestions":true}"#,
    )
    .unwrap();
    set_switch(&d, "focusHelp", true).unwrap();
    let map = grokhub_core::app_json::read(&d).unwrap();
    assert_eq!(map["quietStart"], "22:30");
    assert_eq!(map["suggestions"], true);
    assert_eq!(map["focusHelp"], true);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_broken_app_json_is_not_overwritten_and_the_switch_stays_off() {
    let d = dir("broken");
    std::fs::write(d.join("app.json"), "{oops").unwrap();
    let err = set_switch(&d, "focusHelp", true).unwrap_err();
    assert_eq!(err, "app.json isn't readable, so nothing was changed.");
    assert_eq!(
        std::fs::read_to_string(d.join("app.json")).unwrap(),
        "{oops"
    );
    assert!(!read(&d).focus_help);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn picking_an_idle_time_turns_wind_down_on_and_only_known_times_are_taken() {
    let d = dir("sleep");
    let h = set_auto_sleep(&d, 60).unwrap();
    assert!(h.wind_down);
    assert_eq!(h.auto_sleep_min, Some(60));
    let t = wind_down::load(&d);
    assert!(
        t.on && t.setup_offered,
        "the cabin won't offer set-up again"
    );
    assert_eq!(
        set_auto_sleep(&d, 45).unwrap_err(),
        "45 minutes isn't one of the idle times."
    );
    assert_eq!(
        read(&d).auto_sleep_min,
        Some(60),
        "a refused time changes nothing"
    );
    // Turning it off keeps the time for next time, as the cabin does.
    let h = set_switch(&d, "windDown", false).unwrap();
    assert!(!h.wind_down);
    assert_eq!(h.auto_sleep_min, Some(60));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_unknown_switch_is_refused() {
    let d = dir("unknown");
    assert_eq!(
        set_switch(&d, "dreamTime", true).unwrap_err(),
        "Unknown PC helper: dreamTime"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_rhythm_summary_and_forget() {
    let d = dir("rhythm");
    // 2027-01-15 00:00 UTC, a Friday. Evenings 18:00-22:59 over four weeks.
    let friday = 1_800_000_000_000 - (1_800_000_000_000 % 86_400_000);
    let path = d.join("rhythm.json");
    let mut r = Rhythm::default();
    for day in 0..28u64 {
        for minute in 18 * 60..23 * 60 {
            let at = friday + day * 86_400_000 + u64::from(minute as u32) * 60_000;
            let (weekday, hour) = rhythm::local_slot(at, 0);
            r.note_active(&LocalClock {
                now_ms: at,
                weekday,
                hour,
                minute: minute as u32 % 60,
            });
        }
    }
    rhythm::save(&path, &r).unwrap();
    let h = read(&d);
    assert_eq!(h.rhythm_minutes, 28 * 300);
    assert!(
        h.rhythm_summary.starts_with("Usually on: 6 PM–10 PM."),
        "{}",
        h.rhythm_summary
    );
    let h = forget_rhythm(&d).unwrap();
    assert_eq!(h.rhythm_minutes, 0);
    assert_eq!(
        h.rhythm_summary,
        "Still learning. A few days of use fills this in."
    );
    assert!(h.rhythm, "the switch stays");
    let _ = std::fs::remove_dir_all(&d);
}
