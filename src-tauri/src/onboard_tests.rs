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
