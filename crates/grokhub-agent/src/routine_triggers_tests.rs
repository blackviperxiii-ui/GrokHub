use super::*;
use std::time::{Duration, SystemTime};

const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("routine-trig-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// UTC clock at `ms` after 1 Jan 1970 (a Thursday) 00:00.
fn at(ms: u64) -> LocalClock {
    LocalClock::at_offset(ms, 0)
}

fn save(config: &Path, v: Value) {
    let name = v["name"].as_str().unwrap().to_string();
    let dir = routine::routines_dir(config).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(routine::FILE), v.to_string()).unwrap();
}

fn echo() -> Value {
    json!([{ "id": "say", "tool": "run_terminal_command", "args": { "command": "echo hi" } }])
}

fn touch(path: &Path, secs: u64) {
    std::fs::write(path, "x").unwrap();
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

fn names(starts: &[Start]) -> Vec<(String, String)> {
    starts
        .iter()
        .map(|s| (s.name.clone(), s.why.clone()))
        .collect()
}

#[test]
fn next_slot_reads_each_timer() {
    // Thursday 08:00.
    let thu8 = at(8 * HOUR);
    assert_eq!(next_slot(Timer::Daily { at_min: 9 * 60 }, thu8), 9 * HOUR);
    assert_eq!(
        next_slot(Timer::Daily { at_min: 7 * 60 }, thu8),
        DAY + 7 * HOUR
    );
    assert_eq!(
        next_slot(Timer::Every { minutes: 15 }, thu8),
        8 * HOUR + 15 * MIN
    );
    // Saturday 10:00: weekdays wait for Monday 09:00.
    let sat10 = at(2 * DAY + 10 * HOUR);
    assert_eq!(
        next_slot(Timer::Weekdays { at_min: 9 * 60 }, sat10),
        4 * DAY + 9 * HOUR
    );
}

#[test]
fn a_timer_primes_then_fires_once_per_slot() {
    let config = temp("timer");
    save(
        &config,
        json!({ "version": 1, "name": "morning", "triggers": [{ "timer": "daily 09:00" }], "steps": echo() }),
    );
    // First sight only primes, even past the slot.
    assert_eq!(due(&config, at(10 * HOUR)), Vec::new());
    assert_eq!(
        load_state(&config).routines["morning"].next_run,
        Some(DAY + 9 * HOUR)
    );
    assert_eq!(due(&config, at(DAY + 8 * HOUR)), Vec::new());
    // A missed slot fires once, then waits for the next day.
    let fired = due(&config, at(DAY + 11 * HOUR));
    assert_eq!(
        names(&fired),
        vec![("morning".to_string(), "timer".to_string())]
    );
    assert_eq!(fired[0].inputs, Map::new());
    assert_eq!(fired[0].dir, routine::routines_dir(&config).join("morning"));
    assert_eq!(
        load_state(&config).routines["morning"].next_run,
        Some(2 * DAY + 9 * HOUR)
    );
    assert_eq!(due(&config, at(DAY + 12 * HOUR)), Vec::new());
}

#[test]
fn two_timers_share_the_soonest_slot() {
    let config = temp("timers");
    save(
        &config,
        json!({ "version": 1, "name": "twice", "triggers": [{ "timer": "daily 18:00" }, { "timer": "daily 09:00" }], "steps": echo() }),
    );
    due(&config, at(8 * HOUR));
    assert_eq!(
        load_state(&config).routines["twice"].next_run,
        Some(9 * HOUR)
    );
    assert_eq!(due(&config, at(9 * HOUR)).len(), 1);
    assert_eq!(
        load_state(&config).routines["twice"].next_run,
        Some(18 * HOUR)
    );
}

#[test]
fn a_file_trigger_runs_for_new_and_changed_files_only() {
    let config = temp("file-cfg");
    let inbox = temp("file-inbox");
    save(
        &config,
        json!({
            "version": 1, "name": "csv-in",
            "triggers": [{ "file": { "folder": inbox.to_string_lossy(), "glob": "*.csv" } }],
            "inputs": { "file": { "type": "string" } },
            "steps": [{ "id": "read", "tool": "read_file", "args": { "target_file": "{{inputs.file}}" } }]
        }),
    );
    let old = inbox.join("old.csv");
    touch(&old, 1_000);
    // Files already there when the trigger is first seen don't run.
    assert_eq!(due(&config, at(HOUR)), Vec::new());

    let new = inbox.join("new.csv");
    touch(&new, 2_000);
    touch(&inbox.join("notes.txt"), 2_000);
    touch(&inbox.join(".hidden.csv"), 2_000);
    let fired = due(&config, at(HOUR + MIN));
    let path = new.to_string_lossy().into_owned();
    assert_eq!(names(&fired), vec![("csv-in".to_string(), path.clone())]);
    assert_eq!(
        fired[0].inputs,
        json!({ "file": path }).as_object().unwrap().clone()
    );
    assert_eq!(due(&config, at(HOUR + 2 * MIN)), Vec::new());

    touch(&old, 3_000);
    let fired = due(&config, at(HOUR + 3 * MIN));
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].why, old.to_string_lossy());
    assert_eq!(
        load_state(&config).routines["csv-in"]
            .files
            .as_ref()
            .unwrap()[&*old.to_string_lossy()],
        3_000_000
    );
}

#[test]
fn a_burst_of_files_runs_five_a_tick() {
    let config = temp("burst-cfg");
    let inbox = temp("burst-inbox");
    save(
        &config,
        json!({ "version": 1, "name": "burst", "triggers": [{ "file": { "folder": inbox.to_string_lossy(), "glob": "*" } }], "steps": echo() }),
    );
    due(&config, at(HOUR));
    for i in 0..7 {
        touch(&inbox.join(format!("f{i}")), 5_000);
    }
    let first = due(&config, at(HOUR + MIN));
    assert_eq!(first.len(), FILES_PER_TICK);
    // No `file` input: the run gets no inputs.
    assert_eq!(first[0].inputs, Map::new());
    let rest = due(&config, at(HOUR + 2 * MIN));
    let whys: Vec<String> = rest.iter().map(|s| s.why.clone()).collect();
    assert_eq!(
        whys,
        vec![
            inbox.join("f5").to_string_lossy().into_owned(),
            inbox.join("f6").to_string_lossy().into_owned()
        ]
    );
    assert_eq!(due(&config, at(HOUR + 3 * MIN)), Vec::new());
}

#[test]
fn gone_routines_lose_state_and_broken_ones_are_left_out() {
    let config = temp("gone");
    save(
        &config,
        json!({ "version": 1, "name": "keep", "triggers": [{ "timer": "every 30m" }], "steps": echo() }),
    );
    save(
        &config,
        json!({ "version": 1, "name": "drop", "triggers": [{ "timer": "every 30m" }], "steps": echo() }),
    );
    // A routine with a problem: the tool doesn't exist.
    save(
        &config,
        json!({ "version": 1, "name": "broken", "triggers": [{ "timer": "every 30m" }], "steps": [{ "id": "x", "tool": "no_such_tool", "args": {} }] }),
    );
    let listed: Vec<String> = routines(&config).into_iter().map(|(_, r)| r.name).collect();
    assert_eq!(listed, vec!["drop".to_string(), "keep".to_string()]);
    due(&config, at(0));
    std::fs::remove_dir_all(routine::routines_dir(&config).join("drop")).unwrap();
    due(&config, at(MIN));
    let kept: Vec<String> = load_state(&config).routines.into_keys().collect();
    assert_eq!(kept, vec!["keep".to_string()]);
}

#[test]
fn wildcard_matches_names() {
    assert!(wildcard("*.csv", "sales.csv"));
    assert!(wildcard("report-??.pdf", "report-07.pdf"));
    assert!(wildcard("*", "anything"));
    assert!(wildcard("a*b*c", "aXXbYYc"));
    assert!(!wildcard("*.csv", "sales.csv.tmp"));
    assert!(!wildcard("report-??.pdf", "report-7.pdf"));
    assert!(!wildcard("a*b*c", "aXXbYY"));
}

#[test]
fn a_webhook_finds_its_routine_and_typed_inputs() {
    let config = temp("hook");
    save(
        &config,
        json!({
            "version": 1, "name": "deploy-note",
            "triggers": [{ "webhook": "deploys" }],
            "inputs": { "service": { "type": "string" }, "count": { "type": "integer" } },
            "steps": echo()
        }),
    );
    assert_eq!(for_webhook(&config, "other"), None);
    let (dir, r) = for_webhook(&config, "deploys").unwrap();
    assert_eq!(dir, routine::routines_dir(&config).join("deploy-note"));

    let body = json!({ "service": "api", "count": "three", "extra": 1 });
    let (inputs, words) = webhook_inputs(&r, &body);
    // `count` isn't an integer, so it's left for a fill.
    assert_eq!(
        inputs,
        json!({ "service": "api" }).as_object().unwrap().clone()
    );
    assert_eq!(
        words,
        "{\n  \"count\": \"three\",\n  \"extra\": 1,\n  \"service\": \"api\"\n}"
    );
    let (inputs, words) = webhook_inputs(&r, &json!("api shipped"));
    assert_eq!(inputs, Map::new());
    assert_eq!(words, "api shipped");
}
