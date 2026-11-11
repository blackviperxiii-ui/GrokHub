//! The cabin Code tab: its rail row and routes, the ask-before-run flow, the
//! diff path guard and secret redaction on run output.

use super::*;
use crate::app::{Cabin, Nav};
use grokhub_code::repo::{Builtin, Changed, Diff, OverriddenBy, Repo};
use std::sync::Mutex;

/// `runs` allows one run at a time per process; tests that run a command take turns.
static RUNS: Mutex<()> = Mutex::new(());

fn temp(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("cabin-code-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

fn drain(rx: &Receiver<Msg>) -> Vec<Msg> {
    rx.try_iter().collect()
}

fn plan(command: &str, needs_confirm: bool, remembered: Option<&str>) -> runs::Plan {
    runs::Plan {
        command: command.into(),
        needs_confirm,
        remembered: remembered.map(str::to_string),
    }
}

#[test]
fn code_sits_third_on_the_rail_and_every_route_opens_it() {
    assert_eq!(crate::theme::LOADOUT_NAV[0], ("home", "Home"));
    assert_eq!(crate::theme::LOADOUT_NAV[1], ("chat", "Chat"));
    assert_eq!(crate::theme::LOADOUT_NAV[2], ("code", "Code"));
    assert_eq!(
        crate::icons::rail_icon_for("code"),
        crate::icons::RailIcon::Code
    );
    let mut cabin = Cabin::quiet_for_test();
    cabin.set_nav_id("code");
    assert_eq!(cabin.nav, Nav::Code);
    assert_eq!(cabin.nav_id(), "code");
    assert_eq!(cabin.rail_id(), "code", "Code lights its own row");
    assert_eq!(super::super::settings::nav_name(Nav::Code), "Code");
    assert_eq!(Cabin::nav_from_id("code"), Nav::Code);

    let mut cabin = Cabin::quiet_for_test();
    cabin.nav = Nav::Chat;
    cabin.run_palette(&egui::Context::default(), "nav:code");
    assert_eq!(cabin.nav, Nav::Code);
    assert_eq!(
        grokhub_core::filter_palette("code"),
        vec![("Code", "nav:code")]
    );
}

#[test]
fn the_code_tab_is_not_served_to_the_lan_host() {
    let lan = include_str!("lan_host.rs");
    assert!(
        !lan.contains("code_tab"),
        "LAN host must not reach the Code tab"
    );
    assert!(
        !lan.contains("Nav::Code"),
        "LAN host must not open the Code tab"
    );
    assert!(
        !lan.contains("grokhub_code"),
        "LAN host must not call Code logic"
    );
}

#[test]
fn an_unremembered_run_asks_a_confirmed_one_runs_and_forget_asks_again() {
    let _g = RUNS.lock().unwrap_or_else(|e| e.into_inner());
    let config = temp("ask-config");
    let ws = temp("ask-ws");
    let (tx, rx) = channel();
    let none = || {};

    // First run: nothing remembered, so it asks and runs nothing.
    run_job(
        &config,
        &ws,
        "echo cabin-code-ok",
        false,
        Vec::new(),
        &tx,
        &none,
    );
    let msgs = drain(&rx);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(
        matches!(&msgs[0], Msg::Ask { command } if command == "echo cabin-code-ok"),
        "{msgs:?}"
    );
    let mut tab = CodeTab::default();
    tab.command = "echo cabin-code-ok".into();
    assert_eq!(tab.begin_run().as_deref(), Some("echo cabin-code-ok"));
    assert!(tab.running);
    for m in msgs {
        tab.apply(m);
    }
    assert!(tab.confirming && !tab.running && tab.needs_confirm);

    // Confirmed: it runs, streams its output and is remembered.
    assert_eq!(tab.begin_run().as_deref(), Some("echo cabin-code-ok"));
    assert!(!tab.confirming);
    run_job(
        &config,
        &ws,
        "echo cabin-code-ok",
        true,
        Vec::new(),
        &tx,
        &none,
    );
    let msgs = drain(&rx);
    assert!(msgs
        .iter()
        .any(|m| matches!(m, Msg::Chunk(t) if t.contains("cabin-code-ok"))));
    for m in msgs {
        tab.apply(m);
    }
    assert!(!tab.running);
    assert!(tab.log.contains("cabin-code-ok"), "{}", tab.log);
    assert_eq!(tab.run_note.as_deref(), Some(CODE_PASSED));
    assert!(!tab.needs_confirm);
    assert_eq!(tab.remembered.as_deref(), Some("echo cabin-code-ok"));

    // Remembered: the next run goes straight through without a yes.
    run_job(
        &config,
        &ws,
        "echo cabin-code-ok",
        false,
        Vec::new(),
        &tx,
        &none,
    );
    let msgs = drain(&rx);
    assert!(
        !msgs.iter().any(|m| matches!(m, Msg::Ask { .. })),
        "{msgs:?}"
    );
    assert!(msgs
        .iter()
        .any(|m| matches!(m, Msg::Ran(Ok(done)) if done.exit == 0)));

    // A different command is not covered by the remembered one.
    run_job(
        &config,
        &ws,
        "echo another-one",
        false,
        Vec::new(),
        &tx,
        &none,
    );
    assert!(matches!(
        drain(&rx).as_slice(),
        [Msg::Ask { command }] if command == "echo another-one"
    ));

    // Forget: the plan asks again, and so does the next run.
    forget_job(&config, &ws, &tx);
    for m in drain(&rx) {
        tab.apply(m);
    }
    assert!(tab.needs_confirm);
    assert_eq!(tab.remembered, None);
    run_job(
        &config,
        &ws,
        "echo cabin-code-ok",
        false,
        Vec::new(),
        &tx,
        &none,
    );
    assert!(matches!(drain(&rx).as_slice(), [Msg::Ask { .. }]));
    let _ = std::fs::remove_dir_all(&config);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn run_output_hides_held_secret_values() {
    let _g = RUNS.lock().unwrap_or_else(|e| e.into_inner());
    let config = temp("redact-config");
    let ws = temp("redact-ws");
    let (tx, rx) = channel();
    let secret = "hush-value-4821";
    run_job(
        &config,
        &ws,
        &format!("echo before {secret} after"),
        true,
        vec![secret.to_string()],
        &tx,
        &|| {},
    );
    let msgs = drain(&rx);
    let mut shown = String::new();
    for m in &msgs {
        match m {
            Msg::Chunk(t) => shown.push_str(t),
            Msg::Ran(Ok(done)) => shown.push_str(&done.output),
            _ => {}
        }
    }
    assert!(!shown.contains(secret), "{shown}");
    assert!(shown.contains("before [redacted] after"), "{shown}");
    let _ = std::fs::remove_dir_all(&config);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn the_diff_refuses_a_path_outside_the_workspace() {
    let ws = temp("guard-ws");
    let (tx, rx) = channel();
    for bad in ["../outside.txt", "a/../../b", ""] {
        diff_job(&ws, bad, 7, &tx);
        match rx.try_recv() {
            Ok(Msg::Diff { pick: 7, result }) => {
                assert_eq!(
                    result,
                    Err("That file is outside this workspace.".into()),
                    "{bad}"
                )
            }
            other => panic!("{bad}: {other:?}"),
        }
    }
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn a_repo_folder_loads_its_changed_files_and_the_built_in_skills() {
    let config = temp("load-config");
    let ws = temp("load-ws");
    let init = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&ws)
        .status()
        .expect("git");
    assert!(init.success());
    std::fs::write(ws.join("notes.txt"), "one\ntwo\n").expect("write");
    let (tx, rx) = channel();
    load_job(&config, &ws, None, Some("notes.txt"), &tx);
    let mut tab = CodeTab::default();
    tab.begin_load(&ws);
    assert!(tab.busy());
    for m in drain(&rx) {
        tab.apply(m);
    }
    assert!(!tab.busy());
    assert!(!tab.wants_load(&ws), "the same folder doesn't load again");
    let repo = tab.repo.clone().expect("repo");
    assert!(repo.is_repo);
    assert_eq!(
        repo.files,
        vec![Changed {
            path: "notes.txt".into(),
            status: "untracked".into(),
            staged: false,
        }]
    );
    let diff = tab
        .diff
        .clone()
        .expect("the open file's diff is read again");
    assert_eq!(diff.text, "New file, not tracked yet.\n+one\n+two\n");
    assert!(!tab.builtins.is_empty(), "built-in skills always list");
    assert_eq!(
        tab.builtins.len(),
        grokhub_agent::BUILTIN_SKILL_BODIES.len()
    );
    assert!(tab.needs_confirm, "nothing is remembered yet");
    assert!(tab.wants_load(&config), "another folder loads again");
    let _ = std::fs::remove_dir_all(&config);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn a_stale_pick_is_dropped_and_a_failed_one_keeps_the_open_diff() {
    let mut tab = CodeTab::default();
    let first = tab.begin_pick();
    let second = tab.begin_pick();
    let d = |p: &str| Diff {
        path: p.into(),
        text: "+x\n".into(),
        cut: false,
    };
    tab.apply(Msg::Diff {
        pick: second,
        result: Ok(d("b.rs")),
    });
    tab.apply(Msg::Diff {
        pick: first,
        result: Ok(d("a.rs")),
    });
    assert_eq!(tab.diff.as_ref().map(|d| d.path.as_str()), Some("b.rs"));
    let third = tab.begin_pick();
    tab.apply(Msg::Diff {
        pick: third,
        result: Err("That file couldn't be read.".into()),
    });
    assert_eq!(tab.diff.as_ref().map(|d| d.path.as_str()), Some("b.rs"));
    assert_eq!(tab.note.as_deref(), Some("That file couldn't be read."));
    assert_eq!(tab.open_path().as_deref(), Some("b.rs"));
}

#[test]
fn the_run_box_keeps_typed_text_and_asks_again_after_an_edit() {
    let mut tab = CodeTab::default();
    assert_eq!(tab.begin_run(), None, "an empty box runs nothing");
    tab.apply(Msg::Plan {
        plan: Ok(plan("cargo test", false, Some("cargo test"))),
        fill: true,
    });
    assert_eq!(tab.command, "cargo test");
    assert!(!tab.needs_confirm);
    tab.command = "cargo test -p one".into();
    tab.edit_command();
    assert!(tab.needs_confirm, "an edited command needs its own yes");
    tab.apply(Msg::Plan {
        plan: Ok(plan("npm test", true, None)),
        fill: true,
    });
    assert_eq!(tab.command, "cargo test -p one", "typed text stays");
    assert_eq!(tab.begin_run().as_deref(), Some("cargo test -p one"));
    assert_eq!(tab.begin_run(), None, "one run at a time");
    tab.apply(Msg::Chunk("partial\n".into()));
    assert_eq!(tab.log, "partial\n");
    tab.apply(Msg::Ran(Ok(runs::RunResult {
        command: "cargo test -p one".into(),
        exit: 3,
        output: "failed\n".into(),
        cut: true,
    })));
    assert_eq!(tab.run_note.as_deref(), Some("Tests exited with 3."));
    assert_eq!(tab.log, format!("failed\n{OUTPUT_CUT}"));
    assert!(!tab.running);
    assert_eq!(tab.begin_run().as_deref(), Some("cargo test -p one"));
    tab.apply(Msg::Ran(Err("A run is already in progress.".into())));
    assert_eq!(
        tab.run_note.as_deref(),
        Some("A run is already in progress.")
    );
    assert!(!tab.running);
}

#[test]
fn rows_read_like_the_desktop_tab() {
    let repo = |branch: Option<&str>, up: Option<&str>, ahead, behind| Repo {
        workspace: "w".into(),
        is_repo: true,
        branch: branch.map(str::to_string),
        upstream: up.map(str::to_string),
        ahead,
        behind,
        files: Vec::new(),
        more: false,
    };
    assert_eq!(
        branch_line(&repo(Some("main"), Some("origin/main"), 2, 1)),
        "On main → origin/main · 2 ahead, 1 behind"
    );
    assert_eq!(branch_line(&repo(Some("dev"), None, 0, 0)), "On dev");
    assert_eq!(branch_line(&repo(None, None, 0, 0)), "Detached HEAD");
    let over = OverriddenBy {
        scope: "project".into(),
        name: "review".into(),
    };
    assert_eq!(override_label(&over), "overridden by project skill review");
    assert_eq!(run_note_for(0), "Tests passed.");
    assert_eq!(run_note_for(101), "Tests exited with 101.");
    let entry = activity::Entry {
        ts_ms: 1,
        kind: activity::Kind::Push,
        repo_key: "k".into(),
        host: "github".into(),
        branch: "code/fix".into(),
        command: "git push".into(),
        mode: "ask".into(),
        approved_by: "user".into(),
        exit: Some(1),
        summary: "rejected".into(),
    };
    assert_eq!(activity_line(&entry), "Push · code/fix · rejected · exit 1");
    let quiet = activity::Entry {
        kind: activity::Kind::Terminal,
        branch: String::new(),
        summary: String::new(),
        command: "shell".into(),
        exit: None,
        ..entry
    };
    assert_eq!(activity_line(&quiet), "Terminal · shell");
    assert_eq!(run_confirm_spec().title, "Run this test command?");
    assert_eq!(run_confirm_spec().primary, "Run tests");
    assert!(!run_confirm_spec().danger);
}

#[test]
fn a_skill_that_left_the_list_closes_and_one_still_listed_stays_open() {
    let skill = |name: &str| Builtin {
        name: name.into(),
        description: String::new(),
        body: "body".into(),
        overridden_by: None,
    };
    let loaded = |names: &[&str]| Msg::Loaded {
        repo: Repo {
            workspace: "w".into(),
            is_repo: false,
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            files: Vec::new(),
            more: false,
        },
        builtins: names.iter().map(|n| skill(n)).collect(),
        plan: Ok(plan("", true, None)),
        activity: Vec::new(),
        diff: None,
    };
    let mut tab = CodeTab::default();
    tab.open_skill = Some("review".into());
    tab.apply(loaded(&["review", "ship"]));
    assert_eq!(tab.open_skill.as_deref(), Some("review"));
    tab.apply(loaded(&["ship"]));
    assert_eq!(tab.open_skill, None);
}
