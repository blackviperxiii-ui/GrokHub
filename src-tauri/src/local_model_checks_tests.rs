use super::*;
use std::cell::RefCell;

use grokhub_core::local_picker::CpuSimd;
use grokhub_core::local_setup::model_by_id;

const NOW: u64 = 1_800_000_000_000;
const TWELVE_B: &str = "gemma-4-12b-it-abliterated-q4_k";
const RELEASES: &str = r#"[{"id":"huihui-ai/Huihui-Qwen3-14B-abliterated-GGUF"},{"id":"huihui-ai/Qwen3-14B-GGUF"},{"id":"huihui-ai/Huihui-embeddinggemma-300M-abliterated-GGUF"}]"#;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-local-news-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A 24 GB GPU with 32 GB of RAM, running the 12B.
fn gaming_pc(_: &Path) -> Machine {
    Machine {
        profile: Profile {
            gpus_vram_mb: vec![24_576],
            unified: false,
            ram_mb: 32_768,
            free_disk_mb: 200_000,
            simd: CpuSimd::Avx2,
            llama_build: Some(99_999),
        },
        installed: model_by_id(TWELVE_B),
    }
}

fn no_model(dir: &Path) -> Machine {
    Machine {
        installed: None,
        ..gaming_pc(dir)
    }
}

fn hugging_face(url: &str) -> Result<String, String> {
    Ok(if url.contains("author=huihui-ai") {
        RELEASES.into()
    } else {
        "[]".into()
    })
}

fn turned_on(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    set_on(&dir, true).expect("on");
    dir
}

#[test]
fn a_check_offers_the_bigger_catalog_model_and_reads_huihui_ai_only() {
    let asked = RefCell::new(Vec::new());
    let mut get = |url: &str| {
        asked.borrow_mut().push(url.to_string());
        hugging_face(url)
    };
    let run = run_check(&gaming_pc(Path::new("")), &mut get);
    assert_eq!(
        run.upgrade.as_ref().map(Suggestion::title).as_deref(),
        Some("Gemma 4 31B fits this PC")
    );
    assert_eq!(
        run.releases
            .iter()
            .map(Suggestion::title)
            .collect::<Vec<_>>(),
        vec!["New on Hugging Face: Huihui-Qwen3-14B-abliterated-GGUF"]
    );
    let asked = asked.into_inner();
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].starts_with("https://huggingface.co/api/models?author=huihui-ai&"),
        "{}",
        asked[0]
    );
    assert_eq!(run.read.len(), 3);
}

#[test]
fn no_installed_model_reads_nothing_and_says_so() {
    let dir = turned_on("nomodel");
    let mut get = |url: &str| -> Result<String, String> { panic!("asked for {url}") };
    let posted = RefCell::new(0);
    let post = |_| {
        *posted.borrow_mut() += 1;
        Ok(true)
    };
    assert_eq!(
        check_if_due(&dir, NOW, &no_model, &mut get, &post).as_deref(),
        Some("No local model yet. Set one up under Settings → Local AI first.")
    );
    assert_eq!(*posted.borrow(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_upgrade_posts_once_and_releases_wait_for_the_second_check() {
    let dir = turned_on("seed");
    let posted = RefCell::new(Vec::new());
    let post = |card: grokhub_core::UpdateCard| {
        posted.borrow_mut().push(card);
        Ok(true)
    };
    assert_eq!(
        check_if_due(&dir, NOW, &gaming_pc, &mut hugging_face, &post).as_deref(),
        Some("Checked. 1 new for this PC.")
    );
    {
        let cards = posted.borrow();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "Gemma 4 31B fits this PC");
        assert_eq!(
            cards[0].source_id,
            "local-model:catalog:gemma-4-31b-it-abliterated-q4_k"
        );
    }
    let t = lm::load(&dir);
    assert!(t.seeded, "the first check records what was already out");
    assert_eq!(t.last_check_ms, NOW);

    // The same day: not due.
    assert_eq!(
        check_if_due(&dir, NOW + 1, &gaming_pc, &mut hugging_face, &post),
        None
    );

    // A week on, Hugging Face lists the same repos: they were seen while seeding.
    let week = NOW + lm::CHECK_EVERY_MS;
    assert_eq!(
        check_if_due(&dir, week, &gaming_pc, &mut hugging_face, &post).as_deref(),
        Some("Checked. Nothing new that fits this PC better.")
    );
    assert_eq!(posted.borrow().len(), 1);

    // A new repo the week after is posted, as a link only.
    let mut newer = |url: &str| {
        hugging_face(url).map(|body| {
            body.replacen(
                '[',
                r#"[{"id":"huihui-ai/Huihui-Phi-4-14B-abliterated-GGUF"},"#,
                1,
            )
        })
    };
    assert_eq!(
        check_if_due(
            &dir,
            week + lm::CHECK_EVERY_MS,
            &gaming_pc,
            &mut newer,
            &post
        )
        .as_deref(),
        Some("Checked. 1 new for this PC.")
    );
    let cards = posted.borrow();
    let release = cards.last().expect("release card");
    assert_eq!(
        release.title,
        "New on Hugging Face: Huihui-Phi-4-14B-abliterated-GGUF"
    );
    assert_eq!(release.action, None, "a release never downloads from here");
    assert_eq!(
        release.citations,
        vec!["https://huggingface.co/huihui-ai/Huihui-Phi-4-14B-abliterated-GGUF".to_string()]
    );
    drop(cards);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn switching_off_mid_check_posts_nothing() {
    let dir = turned_on("off");
    let run = run_check(&gaming_pc(&dir), &mut hugging_face);
    set_on(&dir, false).expect("off");
    let post = |_: grokhub_core::UpdateCard| -> Result<bool, String> { panic!("posted while off") };
    assert_eq!(finish(&dir, run, NOW, &post), "");
    assert_eq!(lm::load(&dir).suggested.len(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn off_by_default_and_check_now_needs_it_on() {
    let dir = temp_dir("default");
    assert_eq!(
        status(&dir),
        LocalModelNews {
            on: false,
            note: String::new()
        }
    );
    let mut get = |url: &str| -> Result<String, String> { panic!("asked for {url}") };
    let post = |_: grokhub_core::UpdateCard| -> Result<bool, String> { panic!("posted while off") };
    assert_eq!(check_if_due(&dir, NOW, &gaming_pc, &mut get, &post), None);
    assert_eq!(
        check_now(&dir),
        Err("Turn local model suggestions on first.".to_string())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_installed_model_is_the_added_catalog_file() {
    let dir = temp_dir("installed");
    let twelve = model_by_id(TWELVE_B).expect("12B");
    let file = dir.join(Path::new(twelve.file).file_name().expect("name"));
    std::fs::write(&file, b"gguf").expect("model file");
    let pick = serde_json::json!({ "ownFiles": [file.to_string_lossy()] });
    std::fs::write(dir.join("local_ai.json"), pick.to_string()).expect("pick");
    assert_eq!(installed_model(&dir).map(|m| m.id), Some(TWELVE_B));

    // A model file the catalog doesn't list: nothing to compare.
    let other = dir.join("my-own-model.Q4_K_M.gguf");
    std::fs::write(&other, b"gguf").expect("other file");
    let pick = serde_json::json!({ "ownFiles": [other.to_string_lossy()] });
    std::fs::write(dir.join("local_ai.json"), pick.to_string()).expect("pick");
    assert_eq!(installed_model(&dir).map(|m| m.id), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unread_list_says_so_and_the_next_read_still_seeds() {
    let dir = turned_on("unread");
    let posted = RefCell::new(Vec::new());
    let post = |card: grokhub_core::UpdateCard| {
        posted.borrow_mut().push(card.title);
        Ok(true)
    };
    let mut down = |_: &str| -> Result<String, String> { Err("offline".into()) };
    assert_eq!(
        check_if_due(&dir, NOW, &gaming_pc, &mut down, &post).as_deref(),
        Some("Checked. 1 new for this PC. Couldn't read Hugging Face's model list. Trying again next week.")
    );
    assert!(!lm::load(&dir).seeded);

    // A week on the list reads: what was already out is the backlog.
    let week = NOW + lm::CHECK_EVERY_MS;
    assert_eq!(
        check_if_due(&dir, week, &gaming_pc, &mut hugging_face, &post).as_deref(),
        Some("Checked. Nothing new that fits this PC better.")
    );
    assert_eq!(
        *posted.borrow(),
        vec!["Gemma 4 31B fits this PC".to_string()]
    );
    assert!(lm::load(&dir).seeded);

    // Unread again with nothing else to post.
    assert_eq!(
        check_if_due(
            &dir,
            week + lm::CHECK_EVERY_MS,
            &gaming_pc,
            &mut down,
            &post
        )
        .as_deref(),
        Some("Couldn't read Hugging Face's model list. Trying again next week.")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_suggestion_that_could_not_be_posted_comes_back_next_week() {
    let dir = turned_on("postfail");
    let broken = |_: grokhub_core::UpdateCard| -> Result<bool, String> {
        Err("pulse-feed.json: disk full".into())
    };
    assert_eq!(
        check_if_due(&dir, NOW, &gaming_pc, &mut hugging_face, &broken).as_deref(),
        Some("Couldn't add a suggestion to Pulse (pulse-feed.json: disk full). Trying again next week.")
    );
    assert!(lm::load(&dir).suggested.is_empty());

    let posted = RefCell::new(Vec::new());
    let post = |card: grokhub_core::UpdateCard| {
        posted.borrow_mut().push(card.title);
        Ok(true)
    };
    let week = NOW + lm::CHECK_EVERY_MS;
    assert_eq!(
        check_if_due(&dir, week, &gaming_pc, &mut hugging_face, &post).as_deref(),
        Some("Checked. 1 new for this PC.")
    );
    assert_eq!(
        *posted.borrow(),
        vec!["Gemma 4 31B fits this PC".to_string()]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn off_and_on_again_does_not_post_the_same_upgrade_twice() {
    let dir = turned_on("offon");
    let posted = RefCell::new(0);
    let post = |_: grokhub_core::UpdateCard| {
        *posted.borrow_mut() += 1;
        Ok(true)
    };
    assert_eq!(
        check_if_due(&dir, NOW, &gaming_pc, &mut hugging_face, &post).as_deref(),
        Some("Checked. 1 new for this PC.")
    );
    set_on(&dir, false).expect("off");
    set_on(&dir, true).expect("on");
    // Due at once after turning on, and nothing is posted twice.
    assert_eq!(
        check_if_due(&dir, NOW + 1, &gaming_pc, &mut hugging_face, &post).as_deref(),
        Some("Checked. Nothing new that fits this PC better.")
    );
    assert_eq!(*posted.borrow(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pick_is_saved_as_offered_before_it_is_posted() {
    let dir = turned_on("order");
    let seen_at_post = RefCell::new(Vec::new());
    let post = |card: grokhub_core::UpdateCard| {
        // Were the app to stop now, the next check wouldn't post it again.
        seen_at_post.borrow_mut().push(
            lm::load(&dir)
                .suggested
                .contains("catalog:gemma-4-31b-it-abliterated-q4_k"),
        );
        assert_eq!(card.title, "Gemma 4 31B fits this PC");
        Ok(true)
    };
    assert_eq!(
        check_if_due(&dir, NOW, &gaming_pc, &mut hugging_face, &post).as_deref(),
        Some("Checked. 1 new for this PC.")
    );
    assert_eq!(*seen_at_post.borrow(), vec![true]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tick_that_arrives_mid_check_makes_the_running_one_look_again() {
    let gate = RunGate::new();
    assert!(gate.begin(), "the first tick runs");
    // Check now while it runs: it doesn't run itself...
    assert!(!gate.begin());
    assert!(!gate.begin());
    // ...the running one looks once more, then ends.
    assert!(gate.end());
    assert!(!gate.end());
    // Free again.
    assert!(gate.begin());
    assert!(!gate.end());
}

#[test]
fn a_finished_check_sets_the_settings_note_itself() {
    let dir = turned_on("note");
    let post = |_: grokhub_core::UpdateCard| Ok(true);
    assert_eq!(
        check_if_due(&dir, NOW, &gaming_pc, &mut hugging_face, &post).as_deref(),
        Some("Checked. 1 new for this PC.")
    );
    assert_eq!(status(&dir).note, "Checked. 1 new for this PC.");
    set_on(&dir, false).expect("off");
    assert_eq!(note(), "");
    let _ = std::fs::remove_dir_all(&dir);
}
