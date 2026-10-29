//! Secrets, skills and memory tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

// ---- Cursor fold: Secrets, skills and memory ----

// Folded from PR #358.
#[test]
fn hold_secret_skips_short_and_keeps_one() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.secret_hold.is_empty());
    // Short value (<4 chars) is skipped; no network.
    cabin.hold_secret("ab");
    cabin.hold_secret("abc");
    assert!(cabin.secret_hold.is_empty());
    // Longer secret held once; duplicate ignored.
    let held = "held-token-xyz";
    cabin.hold_secret(held);
    assert_eq!(cabin.secret_hold.len(), 1);
    cabin.hold_secret(held);
    assert_eq!(cabin.secret_hold.len(), 1);
}

// Folded from PR #364.
#[test]
fn mark_get_started_done_sets_flag() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("mark-get-started-done");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.cfg.get_started_done);
    assert!(cabin.chat_job_thread.is_none());
    cabin.mark_get_started_done();
    assert!(cabin.cfg.get_started_done);
    cabin.mark_get_started_done();
    assert!(cabin.cfg.get_started_done);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());

    std::env::remove_var("GROKHUB_CONFIG");
    let _ = std::fs::remove_dir_all(&root);
}

// Folded from PR #366.
#[test]
fn scrub_transcript_redacts_held_secret() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.secret_hold.is_empty());
    // Fake placeholder only — no real credentials, no network.
    let held = "fake-secret-token-xyz";
    cabin.hold_secret(held);
    assert_eq!(cabin.secret_hold.as_slice(), &[held.to_string()]);
    let scrubbed = cabin.scrub_transcript(format!("CONNECTOR_RESULT used {held} then ok"));
    assert!(
        !scrubbed.contains(held),
        "held secret must leave the transcript: {scrubbed}"
    );
    assert!(
        scrubbed.contains("[redacted]"),
        "scrub must insert [redacted]: {scrubbed}"
    );
    assert!(
        scrubbed.contains("CONNECTOR_RESULT"),
        "non-secret text must survive: {scrubbed}"
    );
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #368.
#[test]
fn remember_skill_upserts_and_sorts() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.skill_list.is_empty());
    let mk = |name: &str, desc: &str| SkillMd {
        name: name.into(),
        description: desc.into(),
        slash: "/x".into(),
        trigger: "t".into(),
        instructions: "1. `echo ok`".into(),
        pitfalls: String::new(),
        verify: String::new(),
        runs: 0,
    };
    // Push out of order; remember_skill sorts on insert.
    cabin.remember_skill(mk("zeta-skill", "first zeta"));
    cabin.remember_skill(mk("alpha-skill", "first alpha"));
    assert_eq!(cabin.skill_list.len(), 2);
    assert_eq!(
        cabin
            .skill_list
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha-skill", "zeta-skill"]
    );
    // Same name upserts in place; list size and sort order stay put.
    cabin.remember_skill(mk("zeta-skill", "updated zeta"));
    assert_eq!(cabin.skill_list.len(), 2);
    assert_eq!(cabin.skill_list[0].name, "alpha-skill");
    assert_eq!(cabin.skill_list[1].name, "zeta-skill");
    assert_eq!(cabin.skill_list[1].description, "updated zeta");
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #373.
#[test]
fn mem_file_idx_maps_soul_user_memory() {
    assert_eq!(Cabin::mem_file_idx("SOUL.md"), Some(0));
    assert_eq!(Cabin::mem_file_idx("USER.md"), Some(1));
    assert_eq!(Cabin::mem_file_idx("MEMORY.md"), Some(2));
    assert_eq!(Cabin::mem_file_idx("NOTES.md"), None);
    assert_eq!(Cabin::mem_file_idx(""), None);
}

// Folded from PR #386.
#[test]
fn scrub_live_blocks_noop_without_secrets() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.secret_hold.is_empty());

    let plain = "plain live text, no hold yet";
    cabin.live_blocks.push(grokhub_core::LiveBlock {
        kind: grokhub_core::LiveKind::Say,
        body: plain.to_string(),
        tool_id: String::new(),
        tool_title: String::new(),
        tool_status: String::new(),
        tool_detail: String::new(),
        fold_slot: 2,
    });
    cabin.live_keys = vec![(2, plain.len(), 5)];
    // Empty secret_hold → early return; body and keys stay put. No spawn/network.
    cabin.scrub_live_blocks();
    assert_eq!(cabin.live_blocks[0].body, plain);
    assert_eq!(cabin.live_keys, vec![(2, plain.len(), 5)]);
    assert!(cabin.secret_hold.is_empty());

    // Hold a secret, put it in the live body, scrub must redact.
    let secret = "held-live-token";
    cabin.hold_secret(secret);
    cabin.live_blocks[0].body = format!("leak {secret} now");
    cabin.scrub_live_blocks();
    assert!(!cabin.live_blocks[0].body.contains(secret));
    assert!(cabin.live_blocks[0].body.contains("[redacted]"));
    assert!(cabin.live_keys.is_empty());
}

fn weekly_report_skill() -> SkillMd {
    SkillMd {
        name: "weekly-report".into(),
        description: "Build the weekly report.".into(),
        slash: "/weekly-report".into(),
        trigger: "the user asks for the weekly report".into(),
        instructions: "1. Open report.md\n2. Fill this week's numbers".into(),
        pitfalls: String::new(),
        verify: String::new(),
        runs: 2,
    }
}

/// The skill-write threads append to the ledger; wait for `n` lines.
fn wait_ledger_lines(root: &std::path::Path, n: usize) -> grokhub_agent::harness::ChangeLedger {
    for _ in 0..400 {
        let ledger = grokhub_agent::harness::ChangeLedger::load(root);
        if ledger.all().len() >= n {
            return ledger;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("ledger never reached {n} lines");
}

/// Spike-7: a skill patch only lands when a replay of the skill's last
/// recorded run passes, so the patch tests record one successful run first.
fn record_weekly_report_run(root: &std::path::Path) {
    let mut verify = grokhub_agent::harness::fixture_span(
        1_000,
        "verify_script",
        "{}",
        "allow",
        "pass",
        None,
        "",
    );
    verify.session_id = "chat-weekly".into();
    grokhub_agent::harness::append_span(root, &verify).unwrap();
    let o = grokhub_agent::harness::outcome_from_spans(
        "chat-weekly:1",
        "skill:weekly-report",
        std::slice::from_ref(&verify),
        &[],
        false,
        1_001,
    );
    grokhub_core::outcome::append_outcome(root, &o).unwrap();
}

const WEEKLY_PATCH: &str =
    "SUGGEST_SKILL_PATCH: weekly-report | the user asks for the weekly report | 1. Open report.md 2. Fill the numbers 3. Save a copy as PDF";

#[test]
fn nightly_patch_keeps_the_prior_version_and_typed_undo_puts_it_back() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("skill-undo-typed");
    record_weekly_report_run(&root);
    let path = skills::save_skill(&weekly_report_skill()).expect("save");
    let v1 = std::fs::read(&path).unwrap();
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.apply_review_skill_patches(WEEKLY_PATCH);
    let ledger = wait_ledger_lines(&root, 1);
    let c = &ledger.all()[0];
    assert_eq!(
        (c.id.as_str(), c.op.as_str(), c.origin.as_str()),
        ("weekly-report", "modify", "self_manage")
    );
    assert_eq!(
        c.reason,
        "nightly review: 1. Open report.md 2. Fill the numbers 3. Save a copy as PDF"
    );
    assert_eq!(c.before_hash, grokhub_agent::harness::content_hash(&v1));
    let v2 = std::fs::read(&path).unwrap();
    assert_ne!(v2, v1, "the patch landed");
    assert_eq!(c.after_hash, grokhub_agent::harness::content_hash(&v2));
    let kept = grokhub_agent::harness::skill_history_dir(&root, "weekly-report");
    let kept_v1: Vec<Vec<u8>> = std::fs::read_dir(&kept)
        .unwrap()
        .flatten()
        .map(|e| std::fs::read(e.path()).unwrap())
        .collect();
    assert!(
        kept_v1.contains(&v1),
        "the prior version is kept before the write"
    );
    cabin.send_from_composer("/skills undo weekly-report".into());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        v1,
        "undo puts back the exact bytes"
    );
    assert_eq!(
        cabin.status,
        "Undid the newest change to weekly-report. The version before it is back."
    );
    assert_eq!(
        cabin
            .skill_list
            .iter()
            .find(|s| s.name == "weekly-report")
            .map(|s| s.instructions.as_str()),
        Some("1. Open report.md\n2. Fill this week's numbers\n"),
        "the cabin's list reads the restored file"
    );
    let ledger = grokhub_agent::harness::ChangeLedger::load(&root);
    assert_eq!(ledger.all().len(), 2);
    assert_eq!(
        (ledger.all()[1].op.as_str(), ledger.all()[1].origin.as_str()),
        ("undo", "user")
    );
    assert_eq!(ledger.all()[1].undoes, Some(1));
    // The same patch next night is not written again on its own.
    cabin.apply_review_skill_patches(WEEKLY_PATCH);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(std::fs::read(&path).unwrap(), v1);
    assert_eq!(
        grokhub_agent::harness::ChangeLedger::load(&root)
            .all()
            .len(),
        2
    );
    assert_eq!(
        cabin
            .skill_list
            .iter()
            .find(|s| s.name == "weekly-report")
            .map(|s| s.instructions.as_str()),
        Some("1. Open report.md\n2. Fill this week's numbers\n"),
        "the cabin's list reads the restored file"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The model never undoes a skill change on its own: the same slash from a
/// night job, a review reply, or any send that is not the user's typing only
/// posts the `/skills changes` rows, and the file stays.
#[test]
fn the_model_cannot_undo_a_skill_change_on_its_own() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("skill-undo-model");
    record_weekly_report_run(&root);
    let path = skills::save_skill(&weekly_report_skill()).expect("save");
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.apply_review_skill_patches(WEEKLY_PATCH);
    wait_ledger_lines(&root, 1);
    let patched = std::fs::read(&path).unwrap();
    cabin.send_scheduled_chat("/skills undo weekly-report".into());
    assert_eq!(cabin.status, super::skill_undo::UNDO_NEEDS_YOU);
    cabin.send_chat("/skills undo weekly-report".into());
    cabin.send_chat("/skills restore weekly-report".into());
    cabin.apply_review_reply(Ok("/skills undo weekly-report\nUNDO weekly-report".into()));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        patched,
        "nothing but the user undoes"
    );
    let ledger = grokhub_agent::harness::ChangeLedger::load(&root);
    assert_eq!(ledger.all().len(), 1, "no undo line: {:?}", ledger.all());
    let shown = cabin
        .messages
        .iter()
        .filter(|m| m.1.contains(super::skill_undo::SKILL_CHANGES_HEAD.as_str()))
        .count();
    assert_eq!(shown, 3, "each refused undo shows the rows instead");
    assert!(!cabin.harness.typed_send, "the typed mark never sticks");
    let rows = cabin.skill_rows_now();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "weekly-report");
    assert_eq!(rows[0].act, super::skill_undo::SkillAct::Undo);
    let _ = std::fs::remove_dir_all(&root);
}

/// Outside tests, an `UndoAsk` is built only in `skill_undo.rs`, and only the
/// composer send sets `typed_send`.
#[test]
fn only_typing_or_a_click_builds_an_undo_ask() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .to_path_buf();
    let mut hits = Vec::new();
    let mut stack = vec![crates.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if name != "target" && name != "tests" {
                    stack.push(path);
                }
                continue;
            }
            if !name.ends_with(".rs") || name == "tests.rs" || name.ends_with("_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let live = text.split("#[cfg(test)]").next().unwrap_or("");
            for needle in [
                "UndoAsk::from_click(",
                "UndoAsk::from_typing(",
                "typed_send = true",
            ] {
                // chat_ui.rs has test helpers above the composer send, so the
                // typed mark is counted in the whole file.
                let hay = if needle == "typed_send = true" {
                    text.as_str()
                } else {
                    live
                };
                for _ in hay.matches(needle) {
                    let rel = path
                        .strip_prefix(&crates)
                        .unwrap_or(&path)
                        .display()
                        .to_string()
                        .replace('\\', "/");
                    hits.push(format!("{rel}: {needle}"));
                }
            }
        }
    }
    hits.sort();
    assert_eq!(
        hits,
        vec![
            "grokhub-app/src/app/change_history_ui.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/change_undo.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/chat_ui.rs: typed_send = true",
            "grokhub-app/src/app/repair_ui.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/self_review_ui.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/skill_undo.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/skill_undo.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/skill_undo.rs: UndoAsk::from_typing(",
            "grokhub-app/src/app/skill_undo.rs: UndoAsk::from_typing(",
        ]
    );
}

#[test]
fn a_learned_skill_undone_by_click_is_removed_and_restore_brings_it_back() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("skill-undo-create");
    let mut cabin = Cabin::quiet_for_test();
    let mut skill = weekly_report_skill();
    skill.name = "board-status".into();
    skill.slash = "/board-status".into();
    skill.verify = "test -f report.md".into();
    cabin.commit_proposed_skill(skill);
    let ledger = wait_ledger_lines(&root, 1);
    assert_eq!(ledger.all()[0].op.as_str(), "create");
    let folder = skills::skill_folder("board-status");
    let v1 = std::fs::read(folder.join("SKILL.md")).unwrap();
    assert!(folder.join("scripts/verify.sh").exists());
    let rows = cabin.skill_rows_now();
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].label.starts_with("board-status · added "),
        "{}",
        rows[0].label
    );
    cabin.skill_row_clicked(&rows[0]);
    assert!(
        !folder.exists(),
        "undoing a created skill removes its folder"
    );
    assert!(!cabin.skill_list.iter().any(|s| s.name == "board-status"));
    assert_eq!(
        cabin.status,
        "Removed board-status: it was new. Its text is kept, and /skills restore board-status brings it back."
    );
    let kept = grokhub_agent::harness::skill_history_dir(&root, "board-status");
    let kept: Vec<Vec<u8>> = std::fs::read_dir(&kept)
        .unwrap()
        .flatten()
        .map(|e| std::fs::read(e.path()).unwrap())
        .collect();
    assert_eq!(kept, vec![v1.clone()], "its text stays in history");
    let rows = cabin.skill_rows_now();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].act, super::skill_undo::SkillAct::Restore);
    cabin.send_from_composer("/skills restore board-status".into());
    assert_eq!(std::fs::read(folder.join("SKILL.md")).unwrap(), v1);
    assert!(
        folder.join("scripts/verify.sh").exists(),
        "verify scripts follow the restored file"
    );
    assert!(cabin.skill_list.iter().any(|s| s.name == "board-status"));
    assert_eq!(cabin.status, "Restored board-status from its kept copy.");
    let _ = std::fs::remove_dir_all(&root);
}

// Folded from PR #392.
#[test]
fn apply_review_skill_patches_noop_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.skill_list.is_empty());
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    // Empty / irrelevant raw → parse_suggest_skill_patches empty → early return.
    cabin.apply_review_skill_patches("");
    cabin.apply_review_skill_patches("just prose, no SUGGEST_SKILL_PATCH line");
    assert!(
        cabin.skill_list.is_empty(),
        "idle review patches must not invent skills"
    );
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
}

// Folded from PR #394.
#[test]
fn commit_proposed_skill_remembers_in_list() {
    // Spawned save_skill follows GROKHUB_CONFIG (not thread-local TestConfigDir).
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("commit-skill");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.skill_list.is_empty());
    let proposed = SkillMd {
        name: "harbor-tidy".into(),
        description: "Tidy the harbor desk".into(),
        slash: "/harbor-tidy".into(),
        trigger: "tidy harbor".into(),
        instructions: "1. `echo ok`".into(),
        pitfalls: String::new(),
        verify: String::new(),
        runs: 0,
    };
    // Empty skill_list → prefer_patch misses → remember_skill inserts as-is.
    cabin.commit_proposed_skill(proposed);
    assert_eq!(cabin.skill_list.len(), 1);
    assert_eq!(cabin.skill_list[0].name, "harbor-tidy");
    assert_eq!(cabin.skill_list[0].slash, "/harbor-tidy");
    assert_eq!(cabin.skill_name, "harbor-tidy");
    assert!(cabin.skill_body.contains("harbor-tidy"));
    assert_eq!(cabin.status, "Wrote skill harbor-tidy");
    assert!(
        cabin
            .messages
            .iter()
            .any(|(r, c)| r == "user" && c == SKILL_SAVED_MARK),
        "commit pushes SKILL_SAVED_MARK onto the live transcript"
    );
    assert!(cabin.chat_job_thread.is_none());

    // The spawned save_skill resolves config_dir() when it runs. Let it land
    // under the test root before GROKHUB_CONFIG goes away.
    wait_for(
        "commit_proposed_skill writes SKILL.md under the test root",
        || {
            std::fs::read_dir(root.join("skills"))
                .map(|rd| rd.flatten().any(|e| e.path().join("SKILL.md").is_file()))
                .unwrap_or(false)
        },
    );
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}
