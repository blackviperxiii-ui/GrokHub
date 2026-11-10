use super::*;
use crate::perm::ConfigGuard;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

/// 2026-01-02 00:00 UTC.
const NOW: u64 = 1_767_312_000_000;

fn temp_cfg(tag: &str) -> (PathBuf, ConfigGuard) {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("gh-onboard-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("memory")).unwrap();
    for (name, body) in [
        ("USER.md", "# User\n\nWho you are. Edit this.\n"),
        ("SOUL.md", "# Soul\n\nWho your assistant is. Edit this.\n"),
        ("MEMORY.md", "# Memory\n\nLong-term notes.\n"),
    ] {
        fs::write(dir.join("memory").join(name), body).unwrap();
    }
    let guard = ConfigGuard::set(&dir);
    (dir, guard)
}

/// A made-up person.
fn answers() -> Answers {
    Answers {
        name: "Sam Example".into(),
        call_me: "Sam".into(),
        tone: "playful".into(),
        goals: "Finish a 10k run".into(),
        interests: "chess, trail running".into(),
        routines: "Gym before work".into(),
        prefs: "Short lists".into(),
        ..Answers::default()
    }
}

fn read(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join("memory").join(name)).unwrap()
}

fn personal_rows(dir: &Path) -> Vec<(String, String)> {
    let store = crate::memory::amr_store_at(dir);
    let (rows, _) = grokhub_core::amr::memory_rows(&store);
    let mut out: Vec<(String, String)> = rows
        .into_iter()
        .map(|r| (r.group().label().to_string(), r.text))
        .collect();
    out.sort();
    out
}

const USER_WANT: &str = "# User\n\n- Name: Sam Example\n- Call me: Sam\n- Interests: chess, trail running\n- I like things done: Short lists\n";
const SOUL_WANT: &str = "# Soul\n\n- Tone: playful. Light jokes, still helpful.\n";
const MEMORY_WANT: &str =
    "# Memory\n\n- 2026-01-02: Goals: Finish a 10k run\n- 2026-01-02: Routines: Gym before work\n";

#[test]
fn save_with_amr_on_writes_the_three_files_and_the_personal_rows() {
    // No app.json: a new install, so AMR is on.
    let (dir, _guard) = temp_cfg("amr-on");
    assert!(pending(&dir));
    keep(&dir, Step::Review, &answers()).unwrap();
    let drafts = draft(&dir, &answers(), NOW);
    let saved = commit(&dir, &drafts, NOW).unwrap();
    assert_eq!(
        saved,
        Saved {
            files: vec!["USER.md".into(), "SOUL.md".into(), "MEMORY.md".into()],
            rows: 6,
            amr: true,
        }
    );
    assert_eq!(read(&dir, "USER.md"), USER_WANT);
    assert_eq!(read(&dir, "SOUL.md"), SOUL_WANT);
    assert_eq!(read(&dir, "MEMORY.md"), MEMORY_WANT);
    // The seeds are one step back.
    assert_eq!(
        read(&dir, "USER.md.prev"),
        "# User\n\nWho you are. Edit this.\n"
    );
    let want: Vec<(String, String)> = [
        ("About you", "Call me Sam"),
        ("About you", "My name is Sam Example"),
        ("How you like things done", "Short lists"),
        ("What you like", "Interests: chess, trail running"),
        ("What you need", "Goals: Finish a 10k run"),
        ("Your routines", "Gym before work"),
    ]
    .iter()
    .map(|(g, t)| (g.to_string(), t.to_string()))
    .collect();
    assert_eq!(personal_rows(&dir), want);
    // The next chat's identity block sees the rows and the soul.
    let block = crate::identity_blocks(&dir);
    assert!(block.contains("Call me Sam"), "{block}");
    assert!(block.contains("Tone: playful"), "{block}");
    // Done, and the typed answers are gone from the progress file.
    let p = load(&dir);
    assert_eq!((p.stage, p.step), (Stage::Done, Step::Done));
    assert_eq!(p.answers, Answers::default());
    let raw = fs::read_to_string(dir.join(oi::PROGRESS_FILE)).unwrap();
    assert!(!raw.contains("Sam"), "{raw}");
    assert!(!pending(&dir));
}

#[test]
fn save_with_amr_off_writes_the_three_files_and_no_rows() {
    let (dir, _guard) = temp_cfg("amr-off");
    // An app.json without the key keeps the legacy file backend.
    fs::write(dir.join("app.json"), "{}\n").unwrap();
    let saved = commit(&dir, &draft(&dir, &answers(), NOW), NOW).unwrap();
    assert_eq!((saved.rows, saved.amr), (0, false));
    assert_eq!(saved.files.len(), 3);
    assert_eq!(read(&dir, "USER.md"), USER_WANT);
    assert_eq!(read(&dir, "SOUL.md"), SOUL_WANT);
    assert_eq!(read(&dir, "MEMORY.md"), MEMORY_WANT);
    assert_eq!(personal_rows(&dir), Vec::<(String, String)>::new());
    // USER.md is the user block.
    assert!(crate::identity_blocks(&dir).contains("- Call me: Sam"));
}

#[test]
fn an_edited_review_is_what_lands_and_secrets_do_not() {
    let (dir, _guard) = temp_cfg("edited");
    fs::write(dir.join("app.json"), "{}\n").unwrap();
    let mut d = draft(&dir, &answers(), NOW);
    d.user = "# User\n\n- Call me: S\n- Mail: sam@example.com\n".into();
    d.soul = String::new();
    commit(&dir, &d, NOW).unwrap();
    assert_eq!(
        read(&dir, "USER.md"),
        "# User\n\n- Call me: S\n- Mail: [email]\n"
    );
    // An emptied draft leaves its file alone.
    assert_eq!(
        read(&dir, "SOUL.md"),
        "# Soul\n\nWho your assistant is. Edit this.\n"
    );
}

#[test]
fn skip_and_reopen() {
    let (dir, _guard) = temp_cfg("skip");
    keep(&dir, Step::About, &answers()).unwrap();
    assert_eq!(load(&dir).step, Step::About);
    assert_eq!(load(&dir).answers.call_me, "Sam");
    skip(&dir, NOW).unwrap();
    assert_eq!(load(&dir).stage, Stage::Skipped);
    assert!(!pending(&dir));
    // Skipping wrote nothing to the files.
    assert_eq!(read(&dir, "USER.md"), "# User\n\nWho you are. Edit this.\n");
    // Opened again from Settings over real files, it shows.
    fs::write(
        dir.join("memory").join("USER.md"),
        "# User\n\n- Likes tea\n",
    )
    .unwrap();
    reopen(&dir).unwrap();
    let p = load(&dir);
    assert_eq!(
        (p.stage, p.step, p.reopened),
        (Stage::Pending, Step::Welcome, true)
    );
    assert!(pending(&dir));
}

#[cfg(unix)]
#[test]
fn a_linked_file_is_never_written_through() {
    let (dir, _guard) = temp_cfg("link");
    fs::write(dir.join("app.json"), "{}\n").unwrap();
    let target = dir.join("elsewhere.md");
    fs::write(&target, "keep\n").unwrap();
    fs::remove_file(dir.join("memory").join("USER.md")).unwrap();
    std::os::unix::fs::symlink(&target, dir.join("memory").join("USER.md")).unwrap();
    let err = commit(&dir, &draft(&dir, &answers(), NOW), NOW).unwrap_err();
    assert!(err.contains("USER.md is a link"), "{err}");
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep\n");
}

#[test]
fn openclaw_files_are_added_below_for_review_and_tools_stay_out() {
    let (dir, _guard) = temp_cfg("openclaw");
    let home = dir.join("home");
    assert_eq!(openclaw_preview(&home, &Drafts::default()), None);
    let ws = home.join(".openclaw").join("workspace");
    fs::create_dir_all(&ws).unwrap();
    fs::write(ws.join("SOUL.md"), "Be calm.").unwrap();
    fs::write(ws.join("TOOLS.md"), "shell: yes").unwrap();
    fs::write(ws.join("NOTES.md"), "token sk-abcdefghijklmnopqrstuv").unwrap();
    fs::write(ws.join("HABITS.md"), "Reads before bed.").unwrap();
    let base = draft(&dir, &answers(), NOW);
    let got = openclaw_preview(&home, &base).unwrap();
    assert_eq!(got.user, base.user);
    assert_eq!(
        got.soul,
        format!(
            "{}\n\n## From OpenClaw SOUL.md\n\nBe calm.\n",
            SOUL_WANT.trim_end()
        )
    );
    assert_eq!(
        got.memory,
        format!(
            "{}\n\n## From OpenClaw HABITS.md\n\nReads before bed.\n",
            MEMORY_WANT.trim_end()
        )
    );
    // A seed is replaced by the import, not kept above it.
    let seeds = draft(&dir, &Answers::default(), NOW);
    let got = openclaw_preview(&home, &seeds).unwrap();
    assert_eq!(got.soul, "# Soul\n\n## From OpenClaw SOUL.md\n\nBe calm.\n");
}
