use super::*;

const USER_SEED: &str = "# User\n\nWho you are. Edit this.\n";
const SOUL_SEED: &str = "# Soul\n\nWho your assistant is. Edit this.\n";
const MEMORY_SEED: &str = "# Memory\n\nLong-term notes.\n";

fn seeds() -> Existing {
    Existing {
        user: USER_SEED.into(),
        soul: SOUL_SEED.into(),
        memory: MEMORY_SEED.into(),
    }
}

/// A made-up person.
fn sample() -> Answers {
    Answers {
        name: "Sam Example".into(),
        call_me: "Sam".into(),
        about: "Night-shift nurse who games on weekends".into(),
        tone: "direct".into(),
        soul_notes: "Never use emoji".into(),
        goals: "Finish a 10k run".into(),
        interests: "chess, trail running".into(),
        routines: "Gym before work".into(),
        apps: "Steam, a notes app".into(),
        prefs: "Short lists over long paragraphs".into(),
        remember: "Allergic to peanuts".into(),
    }
}

#[test]
fn seeds_are_replaced_by_the_answers_in_the_right_files() {
    let d = drafts(&sample(), &seeds(), "2026-01-02");
    assert_eq!(
        d.user,
        "# User\n\n\
         - Name: Sam Example\n\
         - Call me: Sam\n\
         - About me: Night-shift nurse who games on weekends\n\
         - Interests: chess, trail running\n\
         - Apps I use: Steam, a notes app\n\
         - I like things done: Short lists over long paragraphs\n"
    );
    assert_eq!(
        d.soul,
        "# Soul\n\n\
         - Tone: direct. Short answers, the point first, no filler.\n\
         - Always / never: Never use emoji\n"
    );
    assert_eq!(
        d.memory,
        "# Memory\n\n\
         - 2026-01-02: Goals: Finish a 10k run\n\
         - 2026-01-02: Routines: Gym before work\n\
         - 2026-01-02: Remember: Allergic to peanuts\n"
    );
}

#[test]
fn rows_go_under_the_personal_groups() {
    let got: Vec<(String, String)> = rows(&sample())
        .into_iter()
        .map(|r| (r.group, r.text))
        .collect();
    let want: Vec<(String, String)> = [
        ("about", "My name is Sam Example"),
        ("about", "Call me Sam"),
        ("about", "Night-shift nurse who games on weekends"),
        ("likes", "Interests: chess, trail running"),
        ("likes", "Apps I use: Steam, a notes app"),
        ("routines", "Gym before work"),
        ("needs", "Goals: Finish a 10k run"),
        ("thinking", "Short lists over long paragraphs"),
    ]
    .iter()
    .map(|(g, t)| (g.to_string(), t.to_string()))
    .collect();
    assert_eq!(got, want);
}

#[test]
fn no_answers_leave_every_file_as_it_was() {
    let d = drafts(&Answers::default(), &seeds(), "2026-01-02");
    assert_eq!(
        (d.user.as_str(), d.soul.as_str(), d.memory.as_str()),
        (USER_SEED, SOUL_SEED, MEMORY_SEED)
    );
    assert!(d.rows.is_empty());
}

#[test]
fn a_file_with_real_words_keeps_them_and_gets_the_new_lines_below() {
    let existing = Existing {
        user: "# User\n\n- Likes tea\n".into(),
        ..seeds()
    };
    let a = Answers {
        call_me: "Sam".into(),
        ..Answers::default()
    };
    let d = drafts(&a, &existing, "2026-01-02");
    assert_eq!(
        d.user,
        "# User\n\n- Likes tea\n\n## From get to know you\n\n- Call me: Sam\n"
    );
}

#[test]
fn emails_numbers_and_secrets_are_masked_and_lines_stay_single() {
    assert_eq!(clean("  write to sam@example.com  "), "write to [email]");
    assert_eq!(clean("key sk-abcdefghijklmnopqrstuv"), "key [redacted]");
    assert_eq!(clean("chess\nrunning\n\n"), "chess; running");
    assert_eq!(clean(&"a".repeat(300)).chars().count(), ANSWER_MAX);
    let d = reviewed(&Drafts {
        user: "# User\n- mail sam@example.com".into(),
        soul: String::new(),
        memory: String::new(),
        rows: vec![
            Row {
                group: "about".into(),
                text: "Call me\nSam".into(),
            },
            Row {
                group: "waiting".into(),
                text: "not a group you can add to".into(),
            },
            Row {
                group: "likes".into(),
                text: "   ".into(),
            },
        ],
    });
    assert_eq!(d.user, "# User\n- mail [email]\n");
    assert_eq!(d.soul, "");
    assert_eq!(
        d.rows,
        vec![Row {
            group: "about".into(),
            text: "Call me; Sam".into(),
        }]
    );
}

#[test]
fn seeds_and_pending() {
    assert!(is_seed(""));
    assert!(is_seed(USER_SEED));
    assert!(is_seed(SOUL_SEED));
    assert!(is_seed(MEMORY_SEED));
    assert!(!is_seed("# User\n\n- Likes tea\n"));
    let fresh = Progress::default();
    assert!(pending(&fresh, &seeds()));
    let written = Existing {
        soul: "# Soul\n\n- Be brief\n".into(),
        ..seeds()
    };
    assert!(!pending(&fresh, &written));
    // Part way through, or opened again from Settings, it shows anyway.
    let resumed = Progress {
        step: Step::Soul,
        ..Progress::default()
    };
    assert!(pending(&resumed, &written));
    let reopened = Progress {
        reopened: true,
        ..Progress::default()
    };
    assert!(pending(&reopened, &written));
    for stage in [Stage::Done, Stage::Skipped] {
        let p = Progress {
            stage,
            ..Progress::default()
        };
        assert!(!pending(&p, &seeds()), "{stage:?}");
    }
}

#[test]
fn the_talk_answer_picks_the_starting_tone() {
    assert_eq!(tone_for_personality(""), "friendly");
    assert_eq!(tone_for_personality("nyx"), "friendly");
    assert_eq!(tone_for_personality("concise-pro"), "direct");
    assert_eq!(tone_for_personality("sarcastic-sidekick"), "playful");
    assert_eq!(tone_for_personality("my-own-voice"), "");
}

#[test]
fn progress_round_trips_as_camel_case_json() {
    let p = Progress {
        stage: Stage::Pending,
        step: Step::Life,
        answers: Answers {
            call_me: "Sam".into(),
            ..Answers::default()
        },
        reopened: false,
    };
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"step\":\"life\""), "{json}");
    assert!(json.contains("\"callMe\":\"Sam\""), "{json}");
    assert_eq!(serde_json::from_str::<Progress>(&json).unwrap(), p);
    assert_eq!(
        serde_json::from_str::<Progress>("{}").unwrap(),
        Progress::default()
    );
}

#[test]
fn the_engine_only_hears_ids_from_onboarding() {
    use crate::learn::signal::Kind;
    use crate::learn::sources::{onboard_saved, onboard_skipped, trait_id};
    let saved = onboard_saved("direct", ["Chess", " ", "trail running"], 7);
    let got: Vec<(Kind, String)> = saved
        .iter()
        .map(|s| (s.kind, s.subject.as_str().to_string()))
        .collect();
    let chess = format!("trait:{}", trait_id("interest:chess").unwrap());
    let running = format!("trait:{}", trait_id("interest:trail running").unwrap());
    assert_eq!(
        got,
        vec![
            (Kind::Accepted, "onboard:done".to_string()),
            (Kind::SetByUser, "tone:direct".to_string()),
            (Kind::Liked, chess.clone()),
            (Kind::Liked, running),
        ]
    );
    assert!(
        chess.starts_with("trait:interest/") && !chess.contains("chess"),
        "{chess}"
    );
    // A made-up tone word is not an id the engine keeps.
    assert_eq!(onboard_saved("Sam Example", [], 7).len(), 1);
    let skipped = onboard_skipped(9);
    assert_eq!(skipped.len(), 1);
    assert_eq!(
        (skipped[0].kind, skipped[0].subject.as_str()),
        (Kind::Declined, "onboard:skipped")
    );
}
