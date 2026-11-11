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
            skipped: vec![],
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

/// A workspace under `dir/home` holding `files`.
fn workspace(dir: &Path, files: &[(&str, &str)]) -> PathBuf {
    let home = dir.join("home");
    let ws = home.join(".openclaw").join("workspace");
    fs::create_dir_all(&ws).unwrap();
    for (name, body) in files {
        fs::write(ws.join(name), body).unwrap();
    }
    home
}

#[test]
fn a_file_over_the_cap_is_left_alone() {
    let (dir, _guard) = temp_cfg("too-big");
    fs::write(dir.join("app.json"), "{}\n").unwrap();
    let path = dir.join("memory").join("MEMORY.md");
    let big = "a line of old notes\n".repeat(52_500);
    assert_eq!(big.len() as u64, FILE_CAP + 1_424);
    assert_eq!(held(&dir), vec![]);
    fs::write(&path, &big).unwrap();
    assert_eq!(held(&dir), vec![("MEMORY.md", Why::TooBig)]);
    let d = Drafts {
        user: "# User\n\n- Call me: Sam\n".into(),
        memory: "# Memory\n\n- x\n".into(),
        ..Drafts::default()
    };
    let saved = commit(&dir, &d, NOW).unwrap();
    assert_eq!(saved.files, vec!["USER.md".to_string()]);
    assert_eq!(saved.skipped, vec!["MEMORY.md".to_string()]);
    assert_eq!(fs::read_to_string(&path).unwrap(), big);
    assert!(!dir.join("memory").join("MEMORY.md.prev").exists());
}

#[test]
fn an_import_already_in_the_drafts_is_not_added_again() {
    let (dir, _guard) = temp_cfg("import-twice");
    fs::write(dir.join("app.json"), "{}\n").unwrap();
    let home = workspace(
        &dir,
        &[
            ("SOUL.md", "Be calm.\r\nNo jokes.\n"),
            ("USER.md", "Likes tea.\nMail sam@example.com\n"),
            ("AGENTS.md", "Workspace rules.\n"),
        ],
    );
    let base = draft(&dir, &Answers::default(), NOW);
    let first = openclaw_import(&home, &base).unwrap();
    assert_eq!(first.note, None);
    assert_eq!(
        first.drafts.memory,
        "# Memory\n\n## From OpenClaw AGENTS.md\n\nWorkspace rules.\n"
    );
    // Pressed again in the same Review: the drafts stay as they are.
    let again = openclaw_import(&home, &first.drafts).unwrap();
    assert_eq!(again.drafts, first.drafts);
    assert_eq!(
        again.note.as_deref(),
        Some("The OpenClaw notes are already in these files; nothing was added.")
    );
    // Saved, then imported again: the saved text (line ends and an email
    // as Save writes them) still counts as already there.
    commit(&dir, &first.drafts, NOW).unwrap();
    assert_eq!(
        read(&dir, "USER.md"),
        "# User\n\n## From OpenClaw USER.md\n\nLikes tea.\nMail [email]\n"
    );
    assert_eq!(
        read(&dir, "SOUL.md"),
        "# Soul\n\n## From OpenClaw SOUL.md\n\nBe calm.\nNo jokes.\n"
    );
    let saved = draft(&dir, &Answers::default(), NOW);
    let second = openclaw_import(&home, &saved).unwrap();
    assert_eq!(second.drafts, saved);
    assert_eq!(
        commit(&dir, &second.drafts, NOW).unwrap().files,
        Vec::<String>::new()
    );
    // A file that changed in the workspace is new text, so it is added.
    fs::write(
        home.join(".openclaw").join("workspace").join("AGENTS.md"),
        "Workspace rules.\nAsk before deleting.\n",
    )
    .unwrap();
    let third = openclaw_import(&home, &saved).unwrap();
    assert_eq!(
        third.drafts.memory,
        "# Memory\n\n## From OpenClaw AGENTS.md\n\nWorkspace rules.\n\n\
         ## From OpenClaw AGENTS.md\n\nWorkspace rules.\nAsk before deleting.\n"
    );
    assert_eq!(third.note, None);
}

#[test]
fn an_import_reads_a_fixed_number_of_files_and_says_what_it_left_out() {
    let (dir, _guard) = temp_cfg("import-many");
    let home = workspace(
        &dir,
        &[("SOUL.md", "Be calm.\n"), ("export.log", "not notes\n")],
    );
    let ws = home.join(".openclaw").join("workspace");
    for i in 0..200 {
        fs::write(ws.join(format!("a{i:03}.md")), format!("note {i}\n")).unwrap();
    }
    let got = openclaw_import(&home, &Drafts::default()).unwrap();
    // The workspace's own files are read first, then notes in name order.
    assert_eq!(
        got.drafts.soul,
        "# Soul\n\n## From OpenClaw SOUL.md\n\nBe calm.\n"
    );
    assert_eq!(got.drafts.memory.matches("## From OpenClaw ").count(), 63);
    assert!(got
        .drafts
        .memory
        .starts_with("# Memory\n\n## From OpenClaw a000.md\n\nnote 0\n"));
    assert!(got
        .drafts
        .memory
        .ends_with("## From OpenClaw a062.md\n\nnote 62\n"));
    assert_eq!(
        got.note.as_deref(),
        Some("Only the first 64 notes files were read; 137 were left out.")
    );
}

#[test]
fn an_import_stops_before_a_file_would_pass_the_cap() {
    let (dir, _guard) = temp_cfg("import-full");
    let home = workspace(
        &dir,
        &[
            ("SOUL.md", "Be calm.\n"),
            ("HABITS.md", "Reads before bed.\n"),
        ],
    );
    let nearly = format!("# Memory\n\n{}", "- old\n".repeat(174_760));
    assert_eq!(nearly.len() as u64, FILE_CAP - 6);
    let base = Drafts {
        memory: nearly.clone(),
        ..Drafts::default()
    };
    let got = openclaw_import(&home, &base).unwrap();
    assert_eq!(got.drafts.memory, nearly);
    assert_eq!(
        got.drafts.soul,
        "# Soul\n\n## From OpenClaw SOUL.md\n\nBe calm.\n"
    );
    assert_eq!(
        got.note.as_deref(),
        Some("MEMORY.md is nearly full; 1 file was left out.")
    );
}

#[test]
fn a_file_that_is_not_text_is_left_alone() {
    let (dir, _guard) = temp_cfg("not-text");
    fs::write(dir.join("app.json"), "{}\n").unwrap();
    let path = dir.join("memory").join("MEMORY.md");
    // One Latin-1 letter: the file is not UTF-8, so its text can't be shown.
    let old: &[u8] = b"# Memory\n\n- caf\xe9\n";
    fs::write(&path, old).unwrap();
    assert_eq!(existing(&dir).memory, "");
    assert_eq!(held(&dir), vec![("MEMORY.md", Why::NotText)]);
    assert_eq!(
        review_note(&dir).as_deref(),
        Some("MEMORY.md is not plain text (UTF-8); Save leaves it alone.")
    );
    let d = Drafts {
        user: "# User\n\n- Call me: Sam\n".into(),
        memory: "# Memory\n\n- x\n".into(),
        ..Drafts::default()
    };
    let saved = commit(&dir, &d, NOW).unwrap();
    assert_eq!(saved.files, vec!["USER.md".to_string()]);
    assert_eq!(saved.skipped, vec!["MEMORY.md".to_string()]);
    assert_eq!(
        saved_note(&dir, &saved).as_deref(),
        Some("MEMORY.md is not plain text (UTF-8); it was left alone.")
    );
    assert_eq!(fs::read(&path).unwrap(), old);
    assert!(!dir.join("memory").join("MEMORY.md.prev").exists());
    // The same Save again leaves nothing to say about USER.md, and the
    // undecodable file is still not a seed to write over.
    let again = commit(&dir, &d, NOW).unwrap();
    assert_eq!(again.files, Vec::<String>::new());
    assert_eq!(again.skipped, vec!["MEMORY.md".to_string()]);
    assert_eq!(fs::read(&path).unwrap(), old);
}

#[test]
fn a_folder_where_a_file_belongs_is_named_and_left_alone() {
    let (dir, _guard) = temp_cfg("not-a-file");
    fs::write(dir.join("app.json"), "{}\n").unwrap();
    let path = dir.join("memory").join("SOUL.md");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert_eq!(held(&dir), vec![("SOUL.md", Why::NotAFile)]);
    assert_eq!(
        review_note(&dir).as_deref(),
        Some("SOUL.md is not a file; Save leaves it alone.")
    );
    let saved = commit(&dir, &draft(&dir, &answers(), NOW), NOW).unwrap();
    assert_eq!(
        saved.files,
        vec!["USER.md".to_string(), "MEMORY.md".to_string()]
    );
    assert_eq!(saved.skipped, vec!["SOUL.md".to_string()]);
    assert_eq!(
        saved_note(&dir, &saved).as_deref(),
        Some("SOUL.md is not a file; it was left alone.")
    );
    assert!(path.is_dir());
    assert_eq!(read(&dir, "MEMORY.md"), MEMORY_WANT);
    assert_eq!(load(&dir).stage, Stage::Done);
}

#[test]
fn files_that_can_be_read_leave_nothing_to_say() {
    let (dir, _guard) = temp_cfg("readable");
    fs::remove_file(dir.join("memory").join("SOUL.md")).unwrap();
    assert_eq!(held(&dir), vec![]);
    assert_eq!(review_note(&dir), None);
    let saved = commit(&dir, &draft(&dir, &answers(), NOW), NOW).unwrap();
    assert_eq!(saved.skipped, Vec::<String>::new());
    assert_eq!(saved_note(&dir, &saved), None);
    // A file that was not there is written, with no `.prev` beside it.
    assert_eq!(read(&dir, "SOUL.md"), SOUL_WANT);
    assert!(!dir.join("memory").join("SOUL.md.prev").exists());
}

#[cfg(unix)]
#[test]
fn a_link_stops_save_before_any_file_is_written() {
    let (dir, _guard) = temp_cfg("link-last");
    fs::write(dir.join("app.json"), "{}\n").unwrap();
    let target = dir.join("elsewhere.md");
    fs::write(&target, "keep\n").unwrap();
    fs::remove_file(dir.join("memory").join("MEMORY.md")).unwrap();
    std::os::unix::fs::symlink(&target, dir.join("memory").join("MEMORY.md")).unwrap();
    // Review names the link before Save is pressed.
    assert_eq!(held(&dir), vec![("MEMORY.md", Why::Link)]);
    assert_eq!(
        review_note(&dir).as_deref(),
        Some("MEMORY.md is a link to another file; Save won't run until its box is empty.")
    );
    let err = commit(&dir, &draft(&dir, &answers(), NOW), NOW).unwrap_err();
    assert_eq!(
        err,
        "MEMORY.md is a link to another file; it was left alone."
    );
    // The files before the link are as they were, and the stage is not done.
    assert_eq!(read(&dir, "USER.md"), "# User\n\nWho you are. Edit this.\n");
    assert_eq!(
        read(&dir, "SOUL.md"),
        "# Soul\n\nWho your assistant is. Edit this.\n"
    );
    assert!(!dir.join("memory").join("USER.md.prev").exists());
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep\n");
    assert_eq!(load(&dir).stage, Stage::Pending);
    // With its box emptied the other two are saved, and the link is not named
    // as skipped: nothing was asked of it.
    let mut d = draft(&dir, &answers(), NOW);
    d.memory = String::new();
    let saved = commit(&dir, &d, NOW).unwrap();
    assert_eq!(
        saved.files,
        vec!["USER.md".to_string(), "SOUL.md".to_string()]
    );
    assert_eq!(saved.skipped, Vec::<String>::new());
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep\n");
}
