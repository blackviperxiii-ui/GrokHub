//! Recall and AMR memory tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

// Landed from PR #121.
#[test]
fn recall_finds_a_memory_line_and_reports_a_miss() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("recall");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = "harbor light\n".into();
    cabin.run_slash_line("/recall harbor");
    assert_eq!(cabin.status, "Recalling…");
    let start = std::time::Instant::now();
    while !cabin.messages.iter().any(|m| m.1.contains("harbor light"))
        && start.elapsed() < std::time::Duration::from_secs(2)
    {
        cabin.poll_recall();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| m.0 == "assistant" && m.1.contains("MEMORY.md:1: harbor light")),
        "recall missed the memory line: {:?}",
        cabin.messages
    );
    cabin.run_slash_line("/recall zzznone");
    let start = std::time::Instant::now();
    while !cabin
        .messages
        .iter()
        .any(|m| m.1.contains("No recall for zzznone"))
        && start.elapsed() < std::time::Duration::from_secs(2)
    {
        cabin.poll_recall();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| m.0 == "assistant" && m.1.contains("No recall for zzznone")),
        "recall miss did not land: {:?}",
        cabin.messages
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

fn wait_recall(cabin: &mut super::Cabin) -> String {
    let start = std::time::Instant::now();
    while cabin.recall_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(2) {
        cabin.poll_recall();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        cabin.recall_rx.is_none(),
        "recall did not finish: {}",
        cabin.status
    );
    cabin
        .messages
        .iter()
        .rev()
        .find(|m| m.0 == "assistant")
        .map(|m| m.1.clone())
        .unwrap_or_default()
}

#[test]
fn recall_legacy_finds_memory_line_and_skips_amr() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("recall-legacy-amr");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert_eq!(
        cabin.cfg.memory_backend,
        grokhub_core::amr::MemoryBackend::Legacy
    );
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = "harbor light\n".into();
    assert!(!root.join("amr").exists());
    cabin.run_slash_line("/recall harbor");
    assert_eq!(cabin.status, "Recalling…");
    let body = wait_recall(&mut cabin);
    assert!(
        body.contains("MEMORY.md:1: harbor light"),
        "legacy recall missed the memory line: {body}"
    );
    assert!(
        !root.join("amr").exists(),
        "legacy recall must not create amr/"
    );

    let store = grokhub_core::amr::AmrStore::at(root.join("amr"));
    store.init().unwrap();
    store
        .remember(&grokhub_core::amr::NodeDraft {
            id: "fact-harbor".into(),
            node_type: grokhub_core::amr::NodeType::Fact,
            created: "2026-10-07T00:00:00Z".into(),
            updated: "2026-10-07T00:00:00Z".into(),
            source: "user".into(),
            confidence: 0.9,
            tags: vec!["dock".into()],
            body: "harbor lamp from amr\n".into(),
            sensitivity: grokhub_core::amr::Sensitivity::Plain,
            consent_ref: String::new(),
        })
        .unwrap();
    cabin.run_slash_line("/recall harbor");
    let body = wait_recall(&mut cabin);
    assert!(
        body.contains("MEMORY.md:1: harbor light"),
        "legacy recall dropped the memory line: {body}"
    );
    assert!(
        !body.contains("amr:"),
        "legacy recall returned an AMR node: {body}"
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn recall_amr_returns_seeded_node_not_legacy_memory() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("recall-amr");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = "harbor light\n".into();
    assert!(!root.join("amr").exists());
    cabin.run_slash_line("/recall zzznone");
    assert_eq!(cabin.status, "Recalling…");
    let miss = wait_recall(&mut cabin);
    assert_eq!(miss, "SLASH_RESULT:\nNo recall for zzznone");
    assert!(root.join("amr").join("nodes").is_dir());
    assert!(root.join("amr").join("edges").is_dir());
    assert!(root.join("amr").join("dreams").is_dir());
    let readme = std::fs::read_to_string(root.join("amr").join("README.md")).unwrap();
    assert!(
        readme.contains("amr_schema: 1"),
        "first AMR recall must init the store: {readme}"
    );

    let store = grokhub_core::amr::AmrStore::at(root.join("amr"));
    store
        .remember(&grokhub_core::amr::NodeDraft {
            id: "pref-harbor".into(),
            node_type: grokhub_core::amr::NodeType::Preference,
            created: "2026-10-07T00:00:00Z".into(),
            updated: "2026-10-07T00:00:00Z".into(),
            source: "user".into(),
            confidence: 0.8,
            tags: vec!["dock".into()],
            body: "pier light stays on\n".into(),
            sensitivity: grokhub_core::amr::Sensitivity::Plain,
            consent_ref: String::new(),
        })
        .unwrap();
    cabin.run_slash_line("/recall pier");
    let body = wait_recall(&mut cabin);
    assert_eq!(body, "SLASH_RESULT:\namr:pref-harbor: pier light stays on");
    assert!(
        !body.contains("MEMORY.md"),
        "AMR recall returned legacy memory: {body}"
    );
    assert!(!body.contains("harbor light"), "{body}");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Spike-4b: a private AMR note is sealed on disk, /recall opens it with the
// keyring key, and a locked keyring leaves one plain line instead.
#[test]
fn recall_opens_private_notes_and_says_when_they_are_locked() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("recall-sealed");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    let store_keys = std::sync::Arc::new(hx::MemoryKeyStore::new());
    hx::use_key_store_for(&root, store_keys.clone());
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    let store = grokhub_core::amr::AmrStore::at(root.join("amr"))
        .with_sealer(std::sync::Arc::new(hx::LearnedVault::new(&root)));
    store.init().unwrap();
    store
        .remember(&grokhub_core::amr::NodeDraft {
            id: "fact-home".into(),
            node_type: grokhub_core::amr::NodeType::Fact,
            created: "2026-10-07T00:00:00Z".into(),
            updated: "2026-10-07T00:00:00Z".into(),
            source: "user".into(),
            confidence: 0.8,
            tags: vec![],
            body: "pier nine is home\n".into(),
            sensitivity: grokhub_core::amr::Sensitivity::Personal,
            consent_ref: String::new(),
        })
        .unwrap();
    let raw = std::fs::read_to_string(root.join("amr/nodes/fact-home.sealed")).unwrap();
    assert!(!raw.contains("pier"), "{raw}");
    cabin.run_slash_line("/recall pier");
    assert_eq!(
        wait_recall(&mut cabin),
        "SLASH_RESULT:\namr:fact-home: pier nine is home"
    );

    store_keys.set_available(false);
    hx::use_key_store_for(&root, store_keys.clone());
    cabin.run_slash_line("/recall pier");
    let body = wait_recall(&mut cabin);
    assert_eq!(
        body,
        format!(
            "SLASH_RESULT:\n1 private note not searched. {}",
            hx::Locked::Unavailable.message()
        )
    );
    assert_eq!(super::slash::locked_recall_line(0, None), None);
    assert_eq!(
        super::slash::locked_recall_line(2, None).as_deref(),
        Some("2 private notes not searched. Private memory is locked.")
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

fn amr_node_files(root: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root.join("amr").join("nodes"))
        .map(|read| {
            read.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".md") || name.ends_with(".sealed"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn wait_reflect(cabin: &mut super::Cabin) {
    let start = std::time::Instant::now();
    while cabin.reflect_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(2) {
        cabin.poll_reflect();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        cabin.reflect_rx.is_none(),
        "reflect did not finish: {}",
        cabin.status
    );
}

// AMR M1: a reflect addition is one node and no MEMORY.md line; /recall reads
// the node and the old MEMORY.md line (dual-read); /forget tombstones it.
#[test]
fn amr_reflect_writes_one_node_and_recall_reads_both_stores() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("amr-reflect");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    crate::config::write_memory("MEMORY.md", "old harbor line\n").unwrap();
    let memory_before = crate::config::read_memory("MEMORY.md");
    let user_before = crate::config::read_memory("USER.md");
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    cabin.new_thread(false);
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = memory_before.clone();
    cabin.live_mut().push((
        "user".into(),
        "I prefer the harbor light on at night".into(),
    ));
    let insights_before = cabin.learning.insights.len();

    cabin.run_reflect();
    wait_reflect(&mut cabin);
    assert_eq!(cabin.status, "Reflected into the memory repo");
    assert_eq!(
        cabin.reflect_diff,
        "+ I prefer the harbor light on at night"
    );
    let id = grokhub_core::amr::line_id("I prefer the harbor light on at night");
    assert_eq!(amr_node_files(&root), vec![format!("{id}.md")]);
    let node = std::fs::read_to_string(root.join("amr/nodes").join(format!("{id}.md"))).unwrap();
    let node = grokhub_core::amr::Node::from_markdown(&node).unwrap();
    assert_eq!(node.node_type, grokhub_core::amr::NodeType::Preference);
    assert_eq!(node.source, format!("chat:{}", cabin.visible_thread_id()));
    assert_eq!(node.tags, vec!["reflect".to_string()]);
    assert_eq!(
        crate::config::read_memory("MEMORY.md"),
        memory_before,
        "no new MEMORY.md line"
    );
    assert_eq!(
        crate::config::read_memory("USER.md"),
        user_before,
        "USER.md is not written by M1"
    );
    assert_eq!(cabin.mem_body, memory_before);
    assert_eq!(
        cabin.learning.insights.len(),
        insights_before,
        "single write: no insight either"
    );

    // A second reflect over the same chat adds nothing.
    cabin.run_reflect();
    wait_reflect(&mut cabin);
    assert_eq!(cabin.status, "Reflect: nothing new");
    assert_eq!(amr_node_files(&root).len(), 1);

    cabin.run_slash_line("/recall harbor");
    let body = wait_recall(&mut cabin);
    assert_eq!(
        body,
        format!(
            "SLASH_RESULT:\namr:{id}: I prefer the harbor light on at night\nMEMORY.md:1: old harbor line\nMEMORY.md: old harbor line \nChat: I prefer the harbor light on at night"
        )
    );

    cabin.run_slash(grokhub_core::Slash::Forget(Some("harbor".into())));
    assert_eq!(cabin.status, "Forgot harbor: 1 note in the memory repo");
    assert!(
        root.join("amr/nodes").join(format!("{id}.md")).is_file(),
        "the node stays on disk"
    );
    assert!(root
        .join("amr/nodes")
        .join(format!("{id}.tombstone"))
        .is_file());
    cabin.run_slash_line("/recall harbor");
    // The node and the MEMORY.md line are gone; only the chat itself still says it.
    let after = wait_recall(&mut cabin);
    let lines: Vec<&str> = after.lines().collect();
    assert_eq!(lines.len(), 2, "{after}");
    assert_eq!(lines[0], "SLASH_RESULT:");
    assert!(
        lines[1].starts_with("Chat: I prefer the harbor light on at night"),
        "{after}"
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

// Scratch chats write no node and can't forget one.
#[test]
fn amr_scratch_writes_nothing_and_blocks_forget() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("amr-scratch");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    cabin.new_thread(false);
    cabin.run_slash(grokhub_core::Slash::MemoryNote(
        "the quay lantern is green".into(),
    ));
    assert_eq!(cabin.status, "Remembered in the memory repo");
    assert_eq!(amr_node_files(&root).len(), 1);

    cabin.new_thread(true);
    assert!(cabin.scratch());
    cabin
        .live_mut()
        .push(("user".into(), "I prefer the scratch harbor at dawn".into()));
    cabin.run_reflect();
    assert_eq!(cabin.status, "Scratch — no reflect");
    cabin.run_slash(grokhub_core::Slash::MemoryNote("scratch dock note".into()));
    assert_eq!(cabin.status, "Scratch — no memory writes");
    cabin.run_slash(grokhub_core::Slash::Forget(Some("quay".into())));
    assert_eq!(cabin.status, "Scratch — no memory writes");
    assert_eq!(
        amr_node_files(&root).len(),
        1,
        "{:?}",
        amr_node_files(&root)
    );
    let id = grokhub_core::amr::line_id("the quay lantern is green");
    assert!(!root
        .join("amr/nodes")
        .join(format!("{id}.tombstone"))
        .exists());
    std::env::remove_var("GROKHUB_CONFIG");
}

// A secret in a saved MEMORY.md line is redacted in the node; a personal line
// with no keyring key is paused and leaves no file anywhere.
#[test]
fn amr_save_redacts_secrets_and_pauses_personal_lines_without_a_key() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("amr-save");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    crate::config::write_memory("MEMORY.md", "keep this line\n").unwrap();
    hx::use_key_store_for(
        &root,
        std::sync::Arc::new(hx::MemoryKeyStore::unavailable()),
    );
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    cabin.new_thread(false);
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body =
        "keep this line\nthe dock key is sk-abcdefghijklmnopqrstuv\nmy email is ada@example.com\n"
            .into();

    cabin.save_memory_amr();
    assert_eq!(
        cabin.status,
        "Saved 1 to the memory repo. 1 private paused (key locked), not saved."
    );
    assert_eq!(cabin.mem_body, "keep this line\n");
    let id = grokhub_core::amr::line_id("the dock key is [redacted]");
    assert_eq!(amr_node_files(&root), vec![format!("{id}.md")]);
    let disk = std::fs::read_to_string(root.join("amr/nodes").join(format!("{id}.md"))).unwrap();
    assert!(!disk.contains("sk-abcdefghijklmnopqrstuv"), "{disk}");
    assert!(disk.contains("the dock key is [redacted]\n"), "{disk}");
    let pid = grokhub_core::amr::line_id("my email is ada@example.com");
    assert!(!root.join("amr/nodes").join(format!("{pid}.md")).exists());
    assert!(!root
        .join("amr/nodes")
        .join(format!("{pid}.sealed"))
        .exists());
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(crate::config::read_memory("MEMORY.md"), "keep this line\n");

    cabin.run_slash(grokhub_core::Slash::MemoryNote(
        "my email is ada@example.com".into(),
    ));
    assert!(
        cabin.status.starts_with("Learning paused: "),
        "{}",
        cabin.status
    );
    assert!(
        cabin.status.ends_with(". Nothing was written."),
        "{}",
        cabin.status
    );
    assert_eq!(amr_node_files(&root).len(), 1);
    std::env::remove_var("GROKHUB_CONFIG");
}

// A line the memory repo refuses stays in MEMORY.md instead of vanishing.
#[test]
fn amr_save_keeps_a_line_the_store_could_not_write() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("amr-save-fail");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    crate::config::write_memory("MEMORY.md", "keep this line\n").unwrap();
    // `amr` is a file, so the store can't open.
    std::fs::write(root.join("amr"), "not a folder").unwrap();
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    cabin.new_thread(false);
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = "keep this line\nprefer nvim\n".into();
    cabin.save_memory_amr();
    assert!(cabin.status.starts_with("Memory repo:"), "{}", cabin.status);
    assert_eq!(cabin.mem_body, "keep this line\nprefer nvim\n");
    let start = std::time::Instant::now();
    while crate::config::read_memory("MEMORY.md") != "keep this line\nprefer nvim\n"
        && start.elapsed() < std::time::Duration::from_secs(2)
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        crate::config::read_memory("MEMORY.md"),
        "keep this line\nprefer nvim\n"
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

// AMR M2 through the cabin: the first AMR read imports insights and chip
// preferences once, writes the report, and posts one chat line.
#[test]
fn amr_first_recall_imports_learning_and_chips_once() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("amr-import");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    let seed = |cabin: &mut Cabin| {
        cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
        cabin.learning.insights = [
            ("pref:editor", "prefer nvim for quick edits"),
            ("fact:ferry", "the harbor ferry leaves at nine"),
            ("fact:dock", "dock seven holds the spare sails"),
            ("pref:style:short", "They want short replies."),
            ("fact:quay", "the quay lantern is kept in the shed"),
            ("project:keel", "project keel ships on fridays"),
            ("fact:tide", "high tide is the best time to launch"),
        ]
        .iter()
        .map(|(key, text)| grokhub_core::LearningInsight {
            key: (*key).into(),
            text: (*text).into(),
            hits: 1,
        })
        .collect();
        cabin.chip_memory.last_slash = Some("/imagine".into());
        cabin.chip_memory.last_surface = Some("skills".into());
        cabin.chip_memory.hits = serde_json::from_value(serde_json::json!([
            {"key": "chat:plan", "label": "Plan the week", "value": "typed words", "kind": "chat", "uses": 3, "picks": 3}
        ]))
        .unwrap();
    };
    let mut cabin = Cabin::quiet_for_test();
    seed(&mut cabin);
    cabin.new_thread(false);
    crate::store::save_learning(&cabin.learning).unwrap();
    let learning_json = root.join("learning.json");
    let learning_before = std::fs::read(&learning_json).unwrap();
    cabin.run_slash_line("/recall ferry");
    let body = wait_recall(&mut cabin);
    let id = &amr_node_files(&root)
        .into_iter()
        .find(|f| {
            std::fs::read_to_string(root.join("amr/nodes").join(f))
                .unwrap()
                .contains("ferry")
        })
        .unwrap();
    assert_eq!(
        body,
        format!(
            "SLASH_RESULT:\namr:{}: the harbor ferry leaves at nine",
            id.trim_end_matches(".md")
        )
    );
    let status: Vec<&String> = cabin
        .messages
        .iter()
        .filter(|m| m.1.contains("Memory import:"))
        .map(|m| &m.1)
        .collect();
    assert_eq!(status.len(), 1, "{:?}", cabin.messages);
    assert!(
        status[0].ends_with("Memory import: 10 imported, 0 skipped (learning_state 7, chips 3)."),
        "{}",
        status[0]
    );
    assert_eq!(amr_node_files(&root).len(), 10);
    let dreams: Vec<_> = std::fs::read_dir(root.join("amr/dreams"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(dreams.len(), 1);
    let report = std::fs::read_to_string(dreams[0].path()).unwrap();
    assert!(report.contains("\n10 imported, 0 skipped.\n"), "{report}");
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(
        std::fs::read(&learning_json).unwrap(),
        learning_before,
        "legacy files are not edited"
    );

    // A new session runs the import again and finds nothing new.
    let mut again = Cabin::quiet_for_test();
    seed(&mut again);
    again.new_thread(false);
    again.run_slash_line("/recall tide");
    let body = wait_recall(&mut again);
    assert!(
        body.contains(": high tide is the best time to launch"),
        "{body}"
    );
    assert!(!again
        .messages
        .iter()
        .any(|m| m.1.contains("Memory import:")));
    assert_eq!(amr_node_files(&root).len(), 10);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Legacy stays legacy: a reflect and a note write MEMORY.md and never make amr/.
#[test]
fn legacy_reflect_and_note_never_create_amr() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("legacy-no-amr");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    cabin.mem_name = "MEMORY.md".into();
    cabin.live_mut().push((
        "user".into(),
        "remember that the harbor light stays on at night".into(),
    ));
    cabin.run_reflect();
    wait_reflect(&mut cabin);
    assert_eq!(cabin.status, "Reflected MEMORY.md");
    let day = Cabin::local_day();
    assert_eq!(
        cabin.mem_body,
        format!("# Memory\n\n- {day}: the harbor light stays on at night\n")
    );
    cabin.run_slash(grokhub_core::Slash::MemoryNote(
        "the quay lantern is green".into(),
    ));
    assert_eq!(cabin.status, "Wrote MEMORY.md");
    cabin.run_slash(grokhub_core::Slash::Forget(Some("quay".into())));
    assert_eq!(cabin.status, "Forgot quay");
    cabin.run_slash_line("/recall harbor");
    let body = wait_recall(&mut cabin);
    assert!(
        body.contains(&format!(
            "MEMORY.md:3: - {day}: the harbor light stays on at night"
        )),
        "{body}"
    );
    assert!(!root.join("amr").exists(), "legacy must never create amr/");
    std::env::remove_var("GROKHUB_CONFIG");
}

fn amr_dream_cabin(label: &str) -> (std::path::PathBuf, Cabin) {
    let root = crate::config::test_config_root(label);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    cabin.running = false;
    cabin.composer.clear();
    (root, cabin)
}

/// Card 12: memory retention runs once a day on its own (no dream, any hour)
/// and `/memory prune` answers in the chat with what it retired.
#[test]
fn amr_retention_prunes_daily_apart_from_dream_and_memory_prune_reports() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = amr_dream_cabin("amr-retention");
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    assert_eq!(cabin.cfg.amr_retention_days, 90);
    let store = super::amr_memory::amr_store_at(&root);
    store.init().unwrap();
    let now = grokhub_core::now_ms();
    let day = 86_400_000u64;
    for (id, body, confidence, age_days) in [
        ("old-guess", "Maybe the ferry leaves at six", 0.2, 200),
        ("mid-guess", "Perhaps the tram runs late", 0.2, 40),
        ("old-fact", "Lives near the north harbor", 0.9, 200),
    ] {
        let stamp = grokhub_core::oauth::unix_ms_to_rfc3339(now - age_days * day);
        store
            .remember(&grokhub_core::amr::NodeDraft {
                id: id.into(),
                node_type: grokhub_core::amr::NodeType::Fact,
                created: stamp.clone(),
                updated: stamp,
                source: "user".into(),
                confidence,
                tags: vec![],
                body: format!("{body}\n"),
                sensitivity: grokhub_core::amr::Sensitivity::Plain,
                consent_ref: String::new(),
            })
            .unwrap();
    }
    let files_before = amr_node_files(&root);
    let wait = |cabin: &mut Cabin| {
        for _ in 0..200 {
            cabin.poll_amr_prune();
            if !cabin.status.starts_with("Running") && cabin.status.starts_with("Memory retention")
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("retention never finished: {}", cabin.status);
    };

    let chat_before = cabin.messages.len();
    cabin.tick_amr_retention("2026-10-09");
    wait(&mut cabin);
    assert_eq!(
        cabin.status,
        "Memory retention: retired 1 unsure note not updated in 90 days, kept 2."
    );
    assert_eq!(
        cabin.messages.len(),
        chat_before,
        "the daily run posts no chat line"
    );
    assert!(store.is_forgotten("old-guess"));
    assert!(!store.is_forgotten("mid-guess") && !store.is_forgotten("old-fact"));
    assert_eq!(cabin.dream_day, None, "no dream ran");
    assert!(!root
        .join("amr")
        .join("dreams")
        .join("2026-10-09.md")
        .exists());
    assert_eq!(
        amr_node_files(&root),
        files_before,
        "nothing removed from disk"
    );
    cabin.status.clear();
    cabin.tick_amr_retention("2026-10-09");
    cabin.poll_amr_prune();
    assert_eq!(cabin.status, "", "once a day");

    cabin.cfg.amr_retention_days = 0;
    cabin.run_slash_line("/memory prune");
    wait(&mut cabin);
    assert!(
        last_chat_text(&cabin).ends_with("Memory retention: keeping everything (2 notes)."),
        "{}",
        last_chat_text(&cabin)
    );

    cabin.cfg.amr_retention_days = 30;
    cabin.run_slash_line("/memory prune");
    wait(&mut cabin);
    assert!(
        last_chat_text(&cabin)
            .ends_with("Memory retention: retired 1 unsure note not updated in 30 days, kept 1."),
        "{}",
        last_chat_text(&cabin)
    );
    assert!(store.is_forgotten("mid-guess") && !store.is_forgotten("old-fact"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn amr_dream_runs_once_a_night_and_memory_dream_prints_it() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = amr_dream_cabin("amr-dream");
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    crate::config::write_memory("USER.md", "- Likes tea\n").unwrap();
    crate::config::write_memory("SOUL.md", "Be brief.\n").unwrap();
    let user_before = crate::config::read_memory("USER.md");
    let soul_before = crate::config::read_memory("SOUL.md");
    assert_eq!(user_before, "- Likes tea\n");

    cabin.run_slash_line("/memory dream");
    assert!(
        last_chat_text(&cabin).contains("No dream yet."),
        "{}",
        last_chat_text(&cabin)
    );

    let store = super::amr_memory::amr_store_at(&root);
    store.init().unwrap();
    let now = grokhub_core::now_ms();
    let stamp = grokhub_core::oauth::unix_ms_to_rfc3339(now);
    for (id, body, confidence) in [
        ("quay-a", "The quay lantern is green at night", 0.9),
        ("quay-b", "the quay lantern is green at night.", 0.5),
    ] {
        store
            .remember(&grokhub_core::amr::NodeDraft {
                id: id.into(),
                node_type: grokhub_core::amr::NodeType::Fact,
                created: stamp.clone(),
                updated: stamp.clone(),
                source: "user".into(),
                confidence,
                tags: vec![],
                body: format!("{body}\n"),
                sensitivity: grokhub_core::amr::Sensitivity::Plain,
                consent_ref: String::new(),
            })
            .unwrap();
    }
    let files_before = amr_node_files(&root);

    assert!(
        cabin
            .dream_tonight("2026-10-07", REVIEW_NIGHT_HOUR - 1, now)
            .is_none(),
        "not before the review hour"
    );
    cabin.running = true;
    assert!(
        cabin
            .dream_tonight("2026-10-07", REVIEW_NIGHT_HOUR, now)
            .is_none(),
        "never during a turn"
    );
    cabin.running = false;
    assert_eq!(cabin.dream_day, None, "a busy night is tried again");
    cabin
        .dream_tonight("2026-10-07", REVIEW_NIGHT_HOUR, now)
        .expect("due")
        .join()
        .unwrap();
    assert!(
        cabin.dream_tonight("2026-10-07", 23, now).is_none(),
        "once a night"
    );

    let report =
        std::fs::read_to_string(root.join("amr").join("dreams").join("2026-10-07.md")).unwrap();
    assert!(
        report.starts_with("# Memory dream 2026-10-07\n"),
        "{report}"
    );
    assert!(report.contains("- Merged: 1\n"), "{report}");
    assert!(report.contains("Kept `quay-a`"), "{report}");
    assert!(store.is_forgotten("quay-b") && !store.is_forgotten("quay-a"));
    assert_eq!(amr_node_files(&root), files_before, "nothing removed");
    assert_eq!(
        crate::config::read_memory("USER.md"),
        user_before,
        "USER.md is never written"
    );
    assert_eq!(
        crate::config::read_memory("SOUL.md"),
        soul_before,
        "SOUL.md is never written"
    );

    cabin.run_slash_line("/memory dream");
    assert_eq!(cabin.status, "Memory dream");
    let shown = last_chat_text(&cabin);
    assert!(
        shown.contains("What changed: 0 added, 1 merged, 0 retired\n- Merged: \"the quay lantern is green at night.\"\n\n# Memory dream 2026-10-07"),
        "the What changed block sits above the report: {shown}"
    );
    assert!(shown.contains("# Memory dream 2026-10-07"), "{shown}");
    assert!(shown.contains("merged `quay-b`"), "{shown}");

    // A new session the same night finds the report and does not run again.
    let mut later = Cabin::quiet_for_test();
    later.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    later.running = false;
    later.composer.clear();
    assert!(later
        .dream_tonight("2026-10-07", REVIEW_NIGHT_HOUR, now)
        .is_none());
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn amr_dream_halt_skips_the_night() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = amr_dream_cabin("amr-dream-halt");
    cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
    cabin.halt_everything("Stopped");
    let now = grokhub_core::now_ms();
    assert!(cabin
        .dream_tonight("2026-10-07", REVIEW_NIGHT_HOUR, now)
        .is_none());
    assert_eq!(
        cabin.dream_day.as_deref(),
        Some("2026-10-07"),
        "the night is skipped, not retried"
    );
    assert!(!root
        .join("amr")
        .join("dreams")
        .join("2026-10-07.md")
        .exists());
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn legacy_never_dreams_or_creates_amr() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = amr_dream_cabin("legacy-no-dream");
    let now = grokhub_core::now_ms();
    assert!(cabin
        .dream_tonight("2026-10-07", REVIEW_NIGHT_HOUR, now)
        .is_none());
    let wednesday = LocalClock {
        now_ms: now,
        weekday: 3,
        hour: 21,
        minute: 0,
    };
    assert!(cabin
        .night_passes("2026-10-07", &wednesday, now)
        .1
        .is_none());
    cabin.run_slash_line("/memory dream");
    assert!(last_chat_text(&cabin).contains("No dream yet."));
    assert!(!root.join("amr").exists(), "legacy must never create amr/");
    std::env::remove_var("GROKHUB_CONFIG");
}
