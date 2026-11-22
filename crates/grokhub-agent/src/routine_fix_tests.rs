use super::*;
use serde_json::json;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("routine-fix-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Grants are sealed with the learned-tier key: a test keyring.
    crate::harness::use_key_store_for(
        &dir,
        std::sync::Arc::new(crate::harness::MemoryKeyStore::new()),
    );
    dir
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

/// Saves `v` as a routine and returns it as loaded.
fn save(config: &Path, v: Value) -> Routine {
    let r: Routine = serde_json::from_value(v).unwrap();
    crate::routine_draft::save(config, &r, "test").unwrap();
    r
}

fn reader(config: &Path) -> Routine {
    save(
        config,
        json!({
            "version": 1, "name": "notes", "triggers": ["chat"],
            "inputs": { "file": { "type": "string", "default": "notes.txt" } },
            "steps": [{ "id": "read", "tool": "read_file", "args": { "target_file": "{{inputs.file}}" } }],
            "result": "{{read.stdout}}"
        }),
    )
}

fn moved() -> Fix {
    Fix {
        step: "read".into(),
        args: obj(json!({ "target_file": "in/{{inputs.file}}" })),
    }
}

/// Logs a run of `r` whose repairs were `fixes`.
fn ran(config: &Path, r: &Routine, fixes: Vec<Fix>) {
    routine_log::append(
        config,
        &routine_log::RunRow {
            name: r.name.clone(),
            at_ms: 1,
            why: "timer".into(),
            ok: true,
            model_calls: 1,
            line: "x".into(),
            hash: routine_consent::content_hash(r),
            fixes,
        },
    )
    .unwrap();
}

fn replays(config: &Path) -> Vec<(String, String)> {
    harness::read_spans(config, SELF_REVIEW_SESSION)
        .unwrap()
        .into_iter()
        .filter(|s| s.tool == REPLAY_TOOL)
        .map(|s| (s.result, s.claim))
        .collect()
}

#[test]
fn a_fix_is_offered_after_the_same_repair_works_three_times() {
    let config = temp("offer");
    let r = reader(&config);
    ran(&config, &r, vec![moved()]);
    ran(&config, &r, vec![moved()]);
    // A different repair, and runs of another version, don't count.
    ran(
        &config,
        &r,
        vec![Fix {
            step: "read".into(),
            args: obj(json!({ "target_file": "x" })),
        }],
    );
    let mut other = r.clone();
    other.about = "older".into();
    ran(&config, &other, vec![moved()]);
    assert_eq!(proposal(&config, &r), None);
    assert_eq!(replays(&config), Vec::new());

    ran(&config, &r, vec![moved()]);
    let p = proposal(&config, &r).unwrap();
    assert_eq!(p.step, "read");
    assert_eq!(p.before, obj(json!({ "target_file": "{{inputs.file}}" })));
    assert_eq!(p.after, obj(json!({ "target_file": "in/{{inputs.file}}" })));
    assert_eq!(p.seen, 3);
    assert_eq!(p.patched, patch(&r, &moved()).unwrap());
    // The gate ran once; asking again reuses its verdict.
    assert_eq!(proposal(&config, &r).map(|p| p.seen), Some(3));
    assert_eq!(replays(&config), vec![("pass".to_string(), String::new())]);
}

#[test]
fn a_fix_that_fails_the_replay_gate_is_never_offered() {
    let config = temp("gate");
    let r = reader(&config);
    let bad = Fix {
        step: "read".into(),
        args: obj(json!({ "target_file": "{{inputs.nope}}" })),
    };
    for _ in 0..3 {
        ran(&config, &r, vec![bad.clone()]);
    }
    assert_eq!(proposal(&config, &r), None);
    assert_eq!(proposal(&config, &r), None);
    let spans = replays(&config);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].0, "fail");
    assert!(
        spans[0].1.starts_with("Replay failed, fix not offered: "),
        "{}",
        spans[0].1
    );
    assert!(spans[0].1.contains("inputs.nope"), "{}", spans[0].1);
}

#[test]
fn the_gate_refuses_a_patch_that_changes_more_than_the_step() {
    let config = temp("more");
    let r = reader(&config);
    let mut patched = patch(&r, &moved()).unwrap();
    patched.about = "sneaky".into();
    assert_eq!(
        replay_gate(&config, &r, &moved(), &patched, 5),
        Err("it changes more than step read's arguments".to_string())
    );
    assert_eq!(
        patch(
            &r,
            &Fix {
                step: "nope".into(),
                args: Map::new()
            }
        ),
        Err("notes has no step nope".to_string())
    );
}

#[test]
fn apply_saves_the_fix_and_undo_takes_it_back_for_good() {
    let config = temp("apply");
    let r = reader(&config);
    for _ in 0..3 {
        ran(&config, &r, vec![moved()]);
    }
    assert_eq!(
        apply(&config, "notes").unwrap(),
        "Fixed step read in notes."
    );
    let now = find(&config, "notes").unwrap();
    assert_eq!(now, patch(&r, &moved()).unwrap());
    // The new version has no repairs yet.
    assert_eq!(proposal(&config, &now), None);
    crate::routine_draft::undo(&config, "notes", harness::UndoAsk::from_click()).unwrap();
    assert_eq!(find(&config, "notes").unwrap(), r);
    // Back on the old version, the applied fix isn't offered again.
    assert_eq!(proposal(&config, &r), None);
    assert_eq!(
        apply(&config, "notes"),
        Err("No fix to apply to notes".to_string())
    );
}

#[test]
fn a_fix_to_a_change_step_needs_consent_again() {
    let config = temp("consent");
    let r = save(
        &config,
        json!({
            "version": 1, "name": "say", "triggers": ["chat"],
            "steps": [{ "id": "say", "tool": "run_terminal_command", "args": { "command": "ech hi" } }]
        }),
    );
    routine_consent::grant(&config, &r, 1).unwrap();
    let fix = Fix {
        step: "say".into(),
        args: obj(json!({ "command": "echo hi" })),
    };
    for _ in 0..3 {
        ran(&config, &r, vec![fix.clone()]);
    }
    assert_eq!(
        apply(&config, "say").unwrap(),
        "Fixed step say in say. Its changes need your OK again before they run on their own."
    );
    let now = find(&config, "say").unwrap();
    assert_eq!(routine_consent::standing(&config, &now), Standing::Edited);
}

#[test]
fn a_dismissed_fix_stays_away() {
    let config = temp("dismiss");
    let r = reader(&config);
    for _ in 0..3 {
        ran(&config, &r, vec![moved()]);
    }
    dismiss(&config, "notes").unwrap();
    assert_eq!(proposal(&config, &r), None);
    ran(&config, &r, vec![moved()]);
    assert_eq!(proposal(&config, &r), None);
    assert_eq!(
        dismiss(&config, "notes"),
        Err("No fix to dismiss for notes".to_string())
    );
}
