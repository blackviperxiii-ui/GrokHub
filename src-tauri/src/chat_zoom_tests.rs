use super::*;
use std::cell::Cell;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-zoom-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sessions")).expect("temp dir");
    dir
}

const MIN: u64 = 60 * 1000;

/// Writes a saved chat the way the engine does: a header, then per turn the
/// ask, the reply and a usage line (with `at` when the turn has a time).
fn write_chat(dir: &Path, id: &str, turns: &[(&str, &str, Option<u64>)]) {
    let msg = |role: &str, text: &str| {
        serde_json::json!({"type": "message", "role": role,
            "content": [{"type": "input_text", "text": text}]})
        .to_string()
    };
    let mut out = vec![
        serde_json::json!({"id": id, "title": "Chat", "created": 1_000,
        "cwd": "/work", "model": "grok-4.3", "engine": "native", "renamed": false})
        .to_string(),
    ];
    for (ask, reply, at) in turns {
        out.push(msg("user", ask));
        out.push(msg("assistant", reply));
        let mut usage = serde_json::json!({"type": "usage", "input_tokens": 1, "output_tokens": 1,
            "reasoning_tokens": 0, "cost_in_usd_ticks": 0, "cached_tokens": 0, "meter": "pool"});
        if let Some(at) = at {
            usage["at"] = serde_json::json!(at);
        }
        out.push(usage.to_string());
    }
    std::fs::write(
        dir.join("sessions").join(format!("{id}.jsonl")),
        out.join("\n") + "\n",
    )
    .expect("write chat");
}

fn line(role: Role, text: &str) -> Line {
    Line {
        role,
        text: text.into(),
    }
}

fn turn(at: Option<u64>) -> Turn {
    Turn {
        first: 0,
        last: 0,
        at,
        hash: String::new(),
    }
}

/// Unit vectors along one axis each, so two different axes are orthogonal.
fn axis(i: usize) -> Option<Vec<f32>> {
    let mut v = vec![0.0; 4];
    v[i] = 1.0;
    Some(v)
}

#[test]
fn each_ask_starts_a_turn_and_lines_before_the_first_ask_are_one_turn() {
    let lines = vec![
        line(Role::Assistant, "Hi, I'm here."),
        line(Role::User, "plan the trip"),
        line(Role::Tools, "web_search"),
        line(Role::Assistant, "Here's a plan."),
        line(Role::User, "book it"),
        line(Role::Assistant, "Booked."),
    ];
    let got = turns(&lines, &[Some(5), None]);
    let ranges: Vec<(usize, usize, Option<u64>)> =
        got.iter().map(|t| (t.first, t.last, t.at)).collect();
    assert_eq!(ranges, vec![(0, 0, None), (1, 3, Some(5)), (4, 5, None)]);
    assert_eq!(got[1].hash.len(), 16);
    assert_ne!(got[1].hash, got[2].hash);
}

#[test]
fn a_quiet_gap_of_half_an_hour_starts_a_new_group() {
    let turns: Vec<Turn> = [0, MIN, 31 * MIN, 32 * MIN]
        .into_iter()
        .map(|at| turn(Some(at)))
        .collect();
    let vecs = vec![axis(0); 4];
    assert_eq!(segment(&turns, &vecs, MODEL_FLOOR), vec![0..2, 2..4]);
    // 29 minutes is not a gap.
    let turns: Vec<Turn> = [0, 29 * MIN].into_iter().map(|at| turn(Some(at))).collect();
    assert_eq!(segment(&turns, &vecs[..2], MODEL_FLOOR), vec![0..2]);
}

#[test]
fn a_topic_shift_closes_a_group_only_once_it_has_three_turns() {
    let turns = vec![turn(None); 6];
    let shifted = vec![axis(0), axis(0), axis(0), axis(1), axis(1), axis(1)];
    assert_eq!(segment(&turns, &shifted, MODEL_FLOOR), vec![0..3, 3..6]);
    let early = vec![axis(0), axis(0), axis(1), axis(1), axis(1), axis(1)];
    assert_eq!(segment(&turns, &early, MODEL_FLOOR), vec![0..6]);
}

#[test]
fn a_group_closes_at_forty_turns() {
    let turns = vec![turn(None); 45];
    let vecs = vec![axis(2); 45];
    assert_eq!(segment(&turns, &vecs, MODEL_FLOOR), vec![0..40, 40..45]);
}

#[test]
fn words_vectors_match_on_shared_stems_and_skip_stopwords() {
    let a = word_vector("Booking flights to Lisbon").unwrap();
    let b = word_vector("booked flight lisbon").unwrap();
    let c = word_vector("fix the rust build error").unwrap();
    assert!(cosine(&a, &b) > 0.99, "{}", cosine(&a, &b));
    assert!(cosine(&a, &c) < WORDS_FLOOR, "{}", cosine(&a, &c));
    assert_eq!(word_vector("the and of"), None);
}

#[test]
fn a_packed_vector_comes_back_within_rounding() {
    let v = unit(vec![0.3, -0.5, 0.1, 0.8]).unwrap();
    let back = unpack(&pack(&v)).unwrap();
    assert!(cosine(&v, &back) > 0.999, "{}", cosine(&v, &back));
    assert_eq!(pack(&v).len(), 8, "4 int8 bytes as base64");
}

#[test]
fn a_group_card_names_its_topic_from_its_own_words() {
    let lines = vec![
        line(Role::User, "Plan a weekend trip to Lisbon\nwith trains"),
        line(Role::Tools, "web_search"),
        line(Role::Assistant, "Here are three options."),
        line(Role::User, "Which Lisbon hotels are near the river?"),
        line(Role::Assistant, "Two near the river."),
    ];
    let turns = turns(&lines, &[Some(10 * MIN), Some(12 * MIN)]);
    let group = group_of(&lines, &turns);
    assert_eq!(
        group,
        Group {
            first_line: 0,
            last_line: 4,
            title: "Lisbon hotels near river".into(),
            summary: "Plan a weekend trip to Lisbon".into(),
            first_at: Some(10 * MIN),
            last_at: Some(12 * MIN),
            messages: 4,
        }
    );
    let long = "word ".repeat(40);
    assert_eq!(clip(long.trim(), SUMMARY_MAX).chars().count(), SUMMARY_MAX);
    assert!(clip(long.trim(), SUMMARY_MAX).ends_with('…'));
}

#[test]
fn only_turns_after_the_first_change_are_embedded_again() {
    let lines = |asks: &[&str]| -> Vec<Line> {
        asks.iter()
            .flat_map(|ask| [line(Role::User, ask), line(Role::Assistant, "ok")])
            .collect()
    };
    let calls = Cell::new(0);
    let embed = |text: &str| {
        calls.set(calls.get() + 1);
        word_vector(text)
    };
    let first = lines(&["lisbon trip", "lisbon hotels", "lisbon trains"]);
    let (groups, cache) = fold(&first, &[], Kind::Words, &[], &embed);
    assert_eq!(calls.get(), 3);
    assert_eq!(groups.len(), 1);
    // Reopened as is: nothing is embedded.
    calls.set(0);
    let (again, _) = fold(&first, &[], Kind::Words, &cache, &embed);
    assert_eq!((calls.get(), again), (0, groups));
    // A new message: only it is embedded.
    calls.set(0);
    let grown = lines(&[
        "lisbon trip",
        "lisbon hotels",
        "lisbon trains",
        "lisbon food",
    ]);
    fold(&grown, &[], Kind::Words, &cache, &embed);
    assert_eq!(calls.get(), 1);
    // The second ask deleted: every turn from it on is embedded again.
    calls.set(0);
    let cut = lines(&["lisbon trip", "lisbon trains"]);
    fold(&cut, &[], Kind::Words, &cache, &embed);
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_saved_chat_folds_by_gap_and_its_groups_survive_a_reopen() {
    let dir = temp_dir("saved");
    let id = "shell-zoom-a";
    write_chat(
        &dir,
        id,
        &[
            ("plan the lisbon trip", "Here's a plan.", Some(100 * MIN)),
            (
                "lisbon hotels near the river",
                "Two hotels.",
                Some(102 * MIN),
            ),
            ("fix the rust build", "Fixed.", Some(200 * MIN)),
            ("rust build is green now", "Good.", None),
        ],
    );
    let file = std::fs::File::options()
        .write(true)
        .open(dir.join("sessions").join(format!("{id}.jsonl")))
        .unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_millis(300 * MIN))
        .unwrap();
    drop(file);
    let zoom = zoom_with(&dir, id, None).unwrap();
    let shape: Vec<_> = zoom
        .groups
        .iter()
        .map(|g| {
            (
                g.first_line,
                g.last_line,
                g.summary.as_str(),
                g.first_at,
                g.last_at,
                g.messages,
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![
            (
                0,
                3,
                "plan the lisbon trip",
                Some(100 * MIN),
                Some(102 * MIN),
                4
            ),
            (
                4,
                7,
                "fix the rust build",
                Some(200 * MIN),
                Some(200 * MIN),
                4
            ),
        ]
    );
    assert_eq!(
        (zoom.lines, zoom.compacted, zoom.created, zoom.updated),
        (8, 0, 1_000, 300 * MIN),
        "updated is the chat file's own change time"
    );
    let cache: Cache =
        serde_json::from_str(&std::fs::read_to_string(cache_path(&dir, id)).unwrap()).unwrap();
    assert_eq!(
        (cache.version, cache.kind, cache.turns.len()),
        (1, Kind::Words, 4)
    );
    assert!(cache.turns.iter().all(|t| t.vec.is_some()));
    // Reopened (a restart reads the same file): the same groups.
    assert_eq!(zoom_with(&dir, id, None).unwrap().groups, zoom.groups);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_scratch_chat_is_grouped_but_never_cached() {
    let dir = temp_dir("scratch");
    let id = "shell-scratch-zoom-b";
    write_chat(&dir, id, &[("private notes", "Noted.", Some(MIN))]);
    let zoom = zoom_with(&dir, id, None).unwrap();
    assert_eq!(zoom.groups.len(), 1);
    assert!(!cache_path(&dir, id).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleting_a_chat_deletes_its_zoom_cache() {
    let dir = temp_dir("delete");
    let id = "shell-zoom-c";
    write_chat(&dir, id, &[("lisbon trip", "Sure.", Some(MIN))]);
    zoom_with(&dir, id, None).unwrap();
    assert!(cache_path(&dir, id).exists());
    history::delete(&dir, id).unwrap();
    assert!(!cache_path(&dir, id).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cache_made_the_other_way_is_not_reused() {
    let dir = temp_dir("kind");
    let id = "shell-zoom-d";
    write_cache(
        &dir,
        id,
        Kind::Model,
        vec![CachedTurn {
            hash: "x".into(),
            vec: None,
        }],
    );
    assert!(read_cache(&dir, id, Kind::Words).is_empty());
    assert_eq!(read_cache(&dir, id, Kind::Model).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_chat_that_isnt_the_apps_is_refused() {
    let dir = temp_dir("refuse");
    assert_eq!(
        zoom_with(&dir, "cabin-1", None),
        Err(history::NOT_SHELL.to_string())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn turns_saved_by_the_engine_carry_their_time_into_the_groups() {
    use grokhub_agent::{ContentPart, InputItem, Usage};
    let dir = temp_dir("engine");
    let id = "shell-zoom-e";
    let msg = |role: &str, text: &str| InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(text.into())],
    };
    let before = grokhub_core::now_ms();
    {
        let _config = ConfigGuard::set(&dir);
        for (ask, reply) in [("lisbon trip", "Sure."), ("lisbon hotels", "Two.")] {
            let items = [msg("user", ask), msg("assistant", reply)];
            grokhub_agent::record_turn(id, "/work", "grok-4.3", &items, &Usage::default(), "", ask)
                .unwrap();
        }
    }
    let after = grokhub_core::now_ms();
    let zoom = zoom_with(&dir, id, None).unwrap();
    assert_eq!(zoom.groups.len(), 1, "{:?}", zoom.groups);
    let group = &zoom.groups[0];
    assert_eq!(
        (group.first_line, group.last_line, group.messages),
        (0, 3, 4)
    );
    let (first, last) = (group.first_at.unwrap(), group.last_at.unwrap());
    assert!(
        before <= first && first <= last && last <= after,
        "{first}..{last} not in {before}..{after}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_turns_hash_is_the_same_in_every_run() {
    let lines = vec![
        line(Role::User, "lisbon trip"),
        line(Role::Assistant, "Sure."),
    ];
    // FNV-1a over "u\0lisbon trip\0a\0Sure.\0", worked out outside the app.
    assert_eq!(turns(&lines, &[])[0].hash, "8cd215dc746ddf90");
}

#[test]
fn a_reopen_groups_by_the_cached_vectors_not_fresh_ones() {
    let lines: Vec<Line> = (0..6)
        .flat_map(|i| {
            [
                line(Role::User, &format!("ask {i}")),
                line(Role::Assistant, "ok"),
            ]
        })
        .collect();
    // Fresh vectors would all point one way, so nothing would split.
    let embed = |_: &str| axis(0);
    let (fresh, _) = fold(&lines, &[], Kind::Model, &[], &embed);
    assert_eq!(fresh.len(), 1);
    // The cache says the topic turned at the fourth ask.
    let cached: Vec<CachedTurn> = turns(&lines, &[])
        .into_iter()
        .enumerate()
        .map(|(i, turn)| CachedTurn {
            hash: turn.hash,
            vec: axis(usize::from(i >= 3)).map(|v| pack(&v)),
        })
        .collect();
    let (groups, _) = fold(&lines, &[], Kind::Model, &cached, &embed);
    let ranges: Vec<(usize, usize)> = groups.iter().map(|g| (g.first_line, g.last_line)).collect();
    assert_eq!(ranges, vec![(0, 5), (6, 11)]);
}

#[test]
fn a_words_chat_splits_where_its_topic_changes() {
    let lines: Vec<Line> = [
        "lisbon trip",
        "lisbon hotels",
        "lisbon trains",
        "fix the rust build",
        "rust build error",
        "rust build is green",
    ]
    .iter()
    .flat_map(|ask| [line(Role::User, ask), line(Role::Assistant, "ok")])
    .collect();
    let (groups, _) = fold(&lines, &[], Kind::Words, &[], &word_vector);
    let shape: Vec<_> = groups
        .iter()
        .map(|g| (g.first_line, g.last_line, g.summary.as_str()))
        .collect();
    assert_eq!(
        shape,
        vec![(0, 5, "lisbon trip"), (6, 11, "fix the rust build")]
    );
}

#[test]
fn unspaced_scripts_match_on_shared_character_pairs() {
    let a = word_vector("東京の天気を教えて").unwrap();
    let b = word_vector("明日の東京の天気は").unwrap();
    let c = word_vector("ラストのビルドが失敗した").unwrap();
    assert!(cosine(&a, &b) > WORDS_FLOOR, "{}", cosine(&a, &b));
    assert!(cosine(&a, &c) < WORDS_FLOOR, "{}", cosine(&a, &c));
}
