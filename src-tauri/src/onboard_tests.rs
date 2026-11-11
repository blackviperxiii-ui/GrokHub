use std::fs;
use std::path::PathBuf;

use super::*;

/// 2026-01-02 00:00 UTC.
const NOW: u64 = 1_767_312_000_000;

fn temp(tag: &str) -> (PathBuf, grokhub_agent::perm::ConfigGuard) {
    let dir = std::env::temp_dir().join(format!(
        "gh-tauri-onboard-{tag}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("memory")).unwrap();
    let guard = grokhub_agent::perm::ConfigGuard::set(&dir);
    (dir, guard)
}

fn run(dir: &Path, a: Act) -> Reply {
    act(dir, None, "concise-pro", a, NOW).unwrap()
}

#[test]
fn a_fresh_profile_shows_the_stage_and_save_writes_the_files_and_rows() {
    let (dir, _g) = temp("save");
    let first = run(&dir, Act::View);
    assert!(first.pending);
    assert_eq!((first.stage, first.step), (Stage::Pending, Step::Welcome));
    // Auto Setup's Talk answer (direct) is where the soul step starts.
    assert_eq!(first.answers.tone, "direct");
    assert!(first.amr, "a new install has AMR on");
    assert!(!first.openclaw);

    let answers = Answers {
        call_me: "Sam".into(),
        tone: "direct".into(),
        goals: "Finish a 10k run".into(),
        ..Answers::default()
    };
    let kept = run(
        &dir,
        Act::Keep {
            step: Step::Life,
            answers: answers.clone(),
        },
    );
    assert_eq!(
        (kept.step, kept.answers.call_me.as_str()),
        (Step::Life, "Sam")
    );
    let review = run(&dir, Act::Draft { answers });
    let drafts = review.drafts.expect("drafts");
    assert_eq!(drafts.user, "# User\n\n- Call me: Sam\n");
    let done = run(&dir, Act::Save { drafts });
    assert_eq!(
        done.saved,
        Some(Saved {
            files: vec!["USER.md".into(), "SOUL.md".into(), "MEMORY.md".into()],
            skipped: vec![],
            rows: 2,
            amr: true,
        })
    );
    assert!(done.pending, "Done still shows once after Save");
    assert_eq!(done.stage, Stage::Done);
    assert_eq!(
        fs::read_to_string(dir.join("memory").join("SOUL.md")).unwrap(),
        "# Soul\n\n- Tone: direct. Short answers, the point first, no filler.\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("memory").join("MEMORY.md")).unwrap(),
        "# Memory\n\n- 2026-01-02: Goals: Finish a 10k run\n"
    );
    // The next launch does not show it.
    assert!(!run(&dir, Act::View).pending);
}

#[test]
fn skip_closes_it_and_reopen_brings_it_back() {
    let (dir, _g) = temp("skip");
    assert!(!run(&dir, Act::Skip).pending);
    assert!(!run(&dir, Act::View).pending);
    let back = run(&dir, Act::Reopen);
    assert!(back.pending);
    assert_eq!(back.step, Step::Welcome);
}

#[test]
fn openclaw_without_a_workspace_says_so_and_keeps_the_drafts() {
    let (dir, _g) = temp("openclaw");
    let base = Drafts {
        user: "# User\n\n- Call me: Sam\n".into(),
        ..Drafts::default()
    };
    let r = act(
        &dir,
        Some(&dir.join("no-home")),
        "",
        Act::Openclaw {
            drafts: base.clone(),
        },
        NOW,
    )
    .unwrap();
    assert_eq!(r.drafts, Some(base));
    assert_eq!(
        r.note.as_deref(),
        Some("No OpenClaw or Hermes workspace on this computer.")
    );
}

#[test]
fn acts_parse_from_the_page() {
    let a: Act = serde_json::from_str(
        r#"{"act":"keep","step":"soul","answers":{"callMe":"Sam","tone":"playful"}}"#,
    )
    .unwrap();
    assert_eq!(
        a,
        Act::Keep {
            step: Step::Soul,
            answers: Answers {
                call_me: "Sam".into(),
                tone: "playful".into(),
                ..Answers::default()
            },
        }
    );
    let s: Act = serde_json::from_str(r#"{"act":"skip"}"#).unwrap();
    assert_eq!(s, Act::Skip);
}

/// A synthetic OpenClaw workspace under a throwaway home.
fn fixture_home(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "gh-tauri-oc-home-{tag}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let ws = home.join(".openclaw").join("workspace");
    fs::create_dir_all(&ws).unwrap();
    fs::write(ws.join("SOUL.md"), "Be brief and kind.\n").unwrap();
    fs::write(ws.join("USER.md"), "Likes tea.\n").unwrap();
    fs::write(ws.join("TOOLS.md"), "Camera is cam-7.\n").unwrap();
    fs::write(
        ws.join("NOTES.md"),
        "token sk-abcdefghijklmnopqrstuvwxyz0123456789ABCD\n",
    )
    .unwrap();
    home
}

#[test]
fn import_slash_is_a_desktop_command_that_opens_the_review() {
    assert_eq!(
        crate::slash::parse("/import", false),
        Ok(crate::slash::Action::Effect(crate::slash::Effect::Import))
    );
    let hit = crate::slash::hits("/imp", true)
        .into_iter()
        .find(|h| h.cmd == "/import")
        .expect("import row");
    assert_eq!(
        hit.disabled.as_deref(),
        Some(crate::slash::ON_YOUR_COMPUTER)
    );
    assert!(crate::slash::help().contains("/import — Add an OpenClaw workspace for review"));
}

#[test]
fn openclaw_notes_are_appended_for_review_and_nothing_is_written() {
    let (dir, _g) = temp("oc-append");
    let home = fixture_home("append");
    let base = Drafts {
        user: "# User\n\n- Call me: Sam\n".into(),
        ..Drafts::default()
    };
    let r = act(&dir, Some(&home), "", Act::Openclaw { drafts: base }, NOW).unwrap();
    assert!(r.openclaw);
    let d = r.drafts.expect("drafts");
    assert!(
        d.user.starts_with("# User\n\n- Call me: Sam\n"),
        "{}",
        d.user
    );
    assert!(d.user.contains("From OpenClaw USER.md") && d.user.contains("Likes tea."));
    assert!(d.soul.contains("From OpenClaw SOUL.md") && d.soul.contains("Be brief and kind."));
    let all = format!("{}{}{}", d.user, d.soul, d.memory);
    assert!(!all.contains("cam-7"), "TOOLS.md stays out");
    assert!(
        !all.contains("sk-abcdefghijkl"),
        "secret-looking files stay out"
    );
    assert!(
        !dir.join("USER.md").exists(),
        "nothing is written before Save"
    );
    assert!(!dir.join("memory").join("USER.md").exists());
}

#[test]
fn an_existing_user_file_is_unchanged_until_save() {
    let (dir, _g) = temp("oc-keep");
    let home = fixture_home("keep");
    let user_path = dir.join("memory").join("USER.md");
    fs::write(&user_path, "# User\n\n- Real note\n").unwrap();
    let r = act(
        &dir,
        Some(&home),
        "",
        Act::Openclaw {
            drafts: Drafts {
                user: "# User\n\n- Real note\n".into(),
                ..Drafts::default()
            },
        },
        NOW,
    )
    .unwrap();
    assert!(r.drafts.unwrap().user.contains("- Real note\n"));
    assert_eq!(
        fs::read_to_string(&user_path).unwrap(),
        "# User\n\n- Real note\n"
    );
}

const OWN_USER: &str = "# User\n\n- Likes tea\n";
const OWN_SOUL: &str = "- Dry, formal, no jokes.\n";
const OWN_MEMORY: &str = "# Memory\n\n- 2025-12-30: Remember: Allergic to peanuts\n";
const FINISHED: &str = "{\"stage\":\"done\",\"step\":\"done\"}\n";

/// A profile whose Get to know you is finished, holding the person's own
/// three files.
fn finished(tag: &str) -> (PathBuf, grokhub_agent::perm::ConfigGuard) {
    let (dir, guard) = temp(tag);
    for (name, body) in [
        ("USER.md", OWN_USER),
        ("SOUL.md", OWN_SOUL),
        ("MEMORY.md", OWN_MEMORY),
    ] {
        fs::write(dir.join("memory").join(name), body).unwrap();
    }
    fs::write(dir.join("onboard-identity.json"), FINISHED).unwrap();
    (dir, guard)
}

/// What the page sends for /import, with the default voice (friendly): the
/// files as they are, then the workspace's notes.
fn import(dir: &Path, home: &Path) -> Reply {
    let base = act(
        dir,
        Some(home),
        "nyx",
        Act::Draft {
            answers: Answers::default(),
        },
        NOW,
    )
    .unwrap();
    let drafts = base.drafts.expect("drafts");
    act(dir, Some(home), "nyx", Act::Openclaw { drafts }, NOW).unwrap()
}

#[test]
fn import_through_the_shell_adds_only_the_workspace_notes() {
    let (dir, _g) = finished("import");
    let home = fixture_home("import");
    let r = import(&dir, &home);
    assert_eq!(
        r.drafts,
        Some(Drafts {
            user: "# User\n\n- Likes tea\n\n## From OpenClaw USER.md\n\nLikes tea.\n".into(),
            soul: "- Dry, formal, no jokes.\n\n## From OpenClaw SOUL.md\n\nBe brief and kind.\n"
                .into(),
            memory: OWN_MEMORY.into(),
            rows: vec![],
        })
    );
    assert_eq!(r.note, None);
    // Asking opened nothing again and kept nothing: the stage is as it was.
    assert!(!r.pending);
    assert_eq!(r.stage, Stage::Done);
    assert_eq!(
        fs::read_to_string(dir.join("onboard-identity.json")).unwrap(),
        FINISHED
    );
    assert_eq!(
        fs::read_to_string(dir.join("memory").join("SOUL.md")).unwrap(),
        OWN_SOUL
    );
}

#[test]
fn a_second_import_adds_nothing_new() {
    let (dir, _g) = finished("import-twice");
    let home = fixture_home("twice");
    let first = import(&dir, &home).drafts.expect("drafts");
    let done = act(&dir, Some(&home), "nyx", Act::Save { drafts: first }, NOW).unwrap();
    assert_eq!(
        done.saved.map(|s| s.files),
        Some(vec!["USER.md".to_string(), "SOUL.md".to_string()])
    );
    let second = import(&dir, &home);
    assert_eq!(
        second.drafts,
        Some(Drafts {
            user: "# User\n\n- Likes tea\n\n## From OpenClaw USER.md\n\nLikes tea.\n".into(),
            soul: "- Dry, formal, no jokes.\n\n## From OpenClaw SOUL.md\n\nBe brief and kind.\n"
                .into(),
            memory: OWN_MEMORY.into(),
            rows: vec![],
        })
    );
    assert_eq!(
        second.note.as_deref(),
        Some("The OpenClaw notes are already in these files; nothing was added.")
    );
    let drafts = second.drafts.expect("drafts");
    let again = act(&dir, Some(&home), "nyx", Act::Save { drafts }, NOW).unwrap();
    assert_eq!(again.saved.map(|s| s.files), Some(vec![]));
}

#[test]
fn a_file_too_big_to_change_is_named_in_review_and_left_alone_by_save() {
    let (dir, _g) = finished("too-big");
    let path = dir.join("memory").join("MEMORY.md");
    let big = "a line of old notes\n".repeat(52_500);
    fs::write(&path, &big).unwrap();
    let review = run(
        &dir,
        Act::Draft {
            answers: Answers::default(),
        },
    );
    assert_eq!(
        review.note.as_deref(),
        Some("MEMORY.md is too big to change here; Save leaves it alone.")
    );
    let mut drafts = review.drafts.expect("drafts");
    assert_eq!(drafts.memory, "");
    drafts.memory = "# Memory\n\n- x\n".into();
    let done = run(&dir, Act::Save { drafts });
    assert_eq!(
        done.saved,
        Some(Saved {
            files: vec![],
            skipped: vec!["MEMORY.md".into()],
            rows: 0,
            amr: true,
        })
    );
    assert_eq!(
        done.note.as_deref(),
        Some("MEMORY.md is too big to change here; it was left alone.")
    );
    assert_eq!(fs::read_to_string(&path).unwrap().len(), 1_050_000);
    assert!(!dir.join("memory").join("MEMORY.md.prev").exists());
}

#[test]
fn a_tone_the_person_cleared_stays_cleared_when_review_resumes() {
    let (dir, _g) = temp("tone");
    let nyx = |a: Act| act(&dir, None, "nyx", a, NOW).unwrap();
    // Before the soul step the voice picked now is where it starts.
    assert_eq!(nyx(Act::View).answers.tone, "friendly");
    // Past it, an empty tone is the person's answer: Review was reached
    // with the tone cleared, then the app was closed.
    let answers = Answers {
        call_me: "Sam".into(),
        ..Answers::default()
    };
    nyx(Act::Draft { answers });
    let back = nyx(Act::View);
    assert_eq!((back.step, back.answers.tone.as_str()), (Step::Review, ""));
    let again = nyx(Act::Draft {
        answers: back.answers,
    });
    assert_eq!(
        again.drafts.map(|d| d.soul),
        Some(String::new()),
        "no Tone line the person did not pick"
    );
}

#[test]
fn a_file_that_is_not_text_is_named_in_review_and_left_alone_by_save() {
    let (dir, _g) = finished("not-text");
    let path = dir.join("memory").join("USER.md");
    // Saved by an editor in Latin-1: one byte that is not UTF-8.
    let old: &[u8] = b"# User\n\n- Likes caf\xe9s\n";
    fs::write(&path, old).unwrap();
    let home = fixture_home("not-text");
    let review = import(&dir, &home);
    assert_eq!(
        review.note.as_deref(),
        Some("USER.md is not plain text (UTF-8); Save leaves it alone.")
    );
    let drafts = review.drafts.expect("drafts");
    assert_eq!(
        drafts.user,
        "# User\n\n## From OpenClaw USER.md\n\nLikes tea.\n"
    );
    let done = act(&dir, Some(&home), "nyx", Act::Save { drafts }, NOW).unwrap();
    assert_eq!(
        done.saved,
        Some(Saved {
            files: vec!["SOUL.md".into()],
            skipped: vec!["USER.md".into()],
            rows: 0,
            amr: true,
        })
    );
    assert_eq!(
        done.note.as_deref(),
        Some("USER.md is not plain text (UTF-8); it was left alone.")
    );
    assert_eq!(fs::read(&path).unwrap(), old);
    assert!(!dir.join("memory").join("USER.md.prev").exists());
}

#[test]
fn two_saves_at_once_both_finish_and_the_old_text_is_kept() {
    let (dir, _g) = finished("two-saves");
    // AMR off: the three files only.
    fs::write(dir.join("app.json"), "{}\n").unwrap();
    let path = dir.join("memory").join("MEMORY.md");
    let prev = dir.join("memory").join("MEMORY.md.prev");
    for round in 0..60 {
        fs::write(&path, OWN_MEMORY).unwrap();
        let _ = fs::remove_file(&prev);
        let drafts = Drafts {
            memory: format!("# Memory\n\n- round {round}\n"),
            ..Drafts::default()
        };
        let save = || {
            let (dir, drafts) = (dir.clone(), drafts.clone());
            std::thread::spawn(move || act(&dir, None, "nyx", Act::Save { drafts }, NOW))
        };
        let (a, b) = (save(), save());
        let got = [a.join().unwrap(), b.join().unwrap()].map(|r| r.map(|reply| reply.saved));
        // One Save writes the file; the other finds it written.
        let wrote = |files: &[&str]| {
            Ok(Some(Saved {
                files: files.iter().map(|f| f.to_string()).collect(),
                skipped: vec![],
                rows: 0,
                amr: false,
            }))
        };
        assert!(
            got == [wrote(&["MEMORY.md"]), wrote(&[])]
                || got == [wrote(&[]), wrote(&["MEMORY.md"])],
            "round {round}: {got:?}"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("# Memory\n\n- round {round}\n")
        );
        assert_eq!(
            fs::read_to_string(&prev).unwrap(),
            OWN_MEMORY,
            "round {round}"
        );
    }
}
