use super::*;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("routine-log-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn report(ended: Ended, result: Option<&str>, model_calls: u32) -> Report {
    Report {
        lines: vec!["1. say: ok".to_string()],
        ended,
        result: result.map(str::to_string),
        model_calls,
        dry_run: false,
    }
}

#[test]
fn a_row_keeps_the_last_line_and_whether_it_finished() {
    let done = row(
        "morning",
        "timer",
        &report(Ended::Finished, Some("3 new files"), 1),
        42,
    )
    .unwrap();
    assert_eq!(
        done,
        RunRow {
            name: "morning".into(),
            at_ms: 42,
            why: "timer".into(),
            ok: true,
            model_calls: 1,
            line: "3 new files".into(),
        }
    );
    let stopped = Ended::Stopped {
        at: "say".into(),
        why: "refused".into(),
    };
    let r = row("morning", "chat", &report(stopped, None, 0), 43).unwrap();
    assert!(!r.ok);
    assert_eq!(r.line, "Stopped at say: refused");
    let replan = Ended::Replan {
        at: "say".into(),
        why: "no rows".into(),
    };
    assert!(!row("m", "chat", &report(replan, None, 2), 1).unwrap().ok);
    let early = Ended::Done { at: "say".into() };
    assert!(row("m", "chat", &report(early, None, 0), 1).unwrap().ok);
}

#[test]
fn a_dry_run_is_not_logged_and_long_lines_are_clipped() {
    let mut dry = report(Ended::Finished, None, 0);
    dry.dry_run = true;
    assert_eq!(row("m", "chat", &dry, 1), None);

    let long = "x".repeat(250);
    let r = row("m", "chat", &report(Ended::Finished, Some(&long), 0), 1).unwrap();
    assert_eq!(r.line, format!("{}…", "x".repeat(200)));
}

#[test]
fn a_secret_in_the_last_line_is_redacted() {
    let r = row(
        "m",
        "chat",
        &report(Ended::Finished, Some("key sk-abcdefghijklmnopqrstuv"), 0),
        1,
    )
    .unwrap();
    assert!(!r.line.contains("sk-abcdefghijklmnopqrstuv"), "{}", r.line);
}

#[test]
fn stats_add_up_runs_and_model_calls_per_routine() {
    let config = temp("stats");
    let mk = |name: &str, at_ms: u64, calls: u32, ok: bool| RunRow {
        name: name.into(),
        at_ms,
        why: "timer".into(),
        ok,
        model_calls: calls,
        line: format!("run at {at_ms}"),
    };
    append(&config, &mk("a", 1, 2, true)).unwrap();
    append(&config, &mk("b", 2, 0, true)).unwrap();
    append(&config, &mk("a", 3, 1, false)).unwrap();
    let s = stats(&config);
    assert_eq!(
        s.keys().cloned().collect::<Vec<_>>(),
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(s["a"].runs, 2);
    assert_eq!(s["a"].model_calls, 3);
    assert_eq!(s["a"].last, Some(mk("a", 3, 1, false)));
    assert_eq!(s["b"].runs, 1);
    assert_eq!(stats(&temp("none")), BTreeMap::new());
}

#[test]
fn the_log_keeps_the_newest_lines() {
    let config = temp("keep");
    let dir = routine::routines_dir(&config);
    std::fs::create_dir_all(&dir).unwrap();
    let line = |i: u64| {
        serde_json::to_string(&RunRow {
            name: "a".into(),
            at_ms: i,
            why: "timer".into(),
            ok: true,
            model_calls: 0,
            line: String::new(),
        })
        .unwrap()
    };
    let text: String = (0..KEEP as u64).map(|i| line(i) + "\n").collect();
    std::fs::write(dir.join(RUNS_FILE), text).unwrap();
    let mut next = read(&config)[0].clone();
    next.at_ms = 9_999;
    append(&config, &next).unwrap();
    let rows = read(&config);
    assert_eq!(rows.len(), KEEP);
    assert_eq!(rows[0].at_ms, 1);
    assert_eq!(rows[KEEP - 1].at_ms, 9_999);
}
