//! Pulse page tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

// Pulse (2.10.91): Ideas and the Home deck became one page.

fn pulse_idea(id: &str, title: &str, prompt: &str, created_at: u64) -> grokhub_core::UpdateCard {
    let mut c = grokhub_core::idea_card(id, title, "One short line about it", created_at);
    c.id = id.into();
    c.idea_kind = Some(grokhub_core::IdeaKind::Try);
    c.prompt = Some(prompt.into());
    c
}

/// A window under 720: idea rows keep Run at rest.
const NARROW_W: f32 = 640.0;

fn pulse_texts(cabin: &mut super::Cabin, h: f32) -> Vec<String> {
    pulse_texts_w(cabin, 1400.0, h)
}

fn pulse_texts_w(cabin: &mut super::Cabin, w: f32, h: f32) -> Vec<String> {
    let _paint = crate::theme::hold_paint_test();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w, h),
        )),
        ..Default::default()
    };
    let mut texts = Vec::new();
    for _ in 0..2 {
        texts.clear();
        let out = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_pulse(ui));
        for clipped in &out.shapes {
            collect_shape_text(&clipped.shape, &mut texts);
        }
    }
    texts
}

fn pulse_memory() -> String {
    crate::config::read_memory(grokhub_core::pulse::LEDGER_FILE)
}

fn pulse_ids(cabin: &mut super::Cabin, now: u64) -> Vec<(String, i64, &'static str)> {
    cabin
        .pulse_ranked(now)
        .into_iter()
        .map(|r| (r.id, r.score, r.kind.label()))
        .collect()
}

#[test]
fn pulse_replaces_ideas_on_the_rail_and_old_links_still_land() {
    let mut quiet = QuietCabin::boot("pulse-nav");
    let cabin = &mut quiet.cabin;
    assert!(crate::theme::GROK_NAV.contains(&("pulse", "Pulse")));
    assert!(crate::theme::GROK_NAV
        .iter()
        .all(|(id, label)| *id != "ideas" && *label != "For You"));
    assert_eq!(super::Cabin::nav_from_id("pulse"), super::Nav::Pulse);
    assert_eq!(
        super::Cabin::nav_from_id("ideas"),
        super::Nav::Pulse,
        "an old ideas link lands on Pulse"
    );
    assert_eq!(
        grokhub_core::nav_from_chip_value("__nav:pulse"),
        Some("pulse")
    );
    cabin.set_nav_id("ideas");
    assert_eq!(cabin.nav, super::Nav::Pulse);
    assert_eq!(cabin.nav_id(), "pulse");
    cabin.nav = super::Nav::Chat;
    cabin.run_palette(&egui::Context::default(), "nav:pulse");
    assert_eq!(cabin.nav, super::Nav::Pulse);
    // Empty main chat is chat-first; cards live in Pulse (no Settings deck toggle).
    let home = fn_src(&cabin_src(), "ui_empty_home").to_string();
    assert!(
        !home.contains("home_deck")
            && !home.contains("paint_update_feed")
            && !home.contains("paint_home_deck_over_chat")
            && !home.contains("home_feed_count")
            && home.contains("paint_device_glance_row"),
        "empty home must not paint the Home card deck: {home}"
    );
    let texts = pulse_texts(cabin, 900.0);
    assert_eq!(
        texts.first().map(String::as_str),
        Some("Pulse"),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t == "What I'd do next, from how you use Loadout."),
        "{texts:?}"
    );
    // One stream: no Feed | For You switch, the two actions sit in the header.
    assert!(
        !texts.iter().any(|t| t == "For You" || t == "Feed"),
        "{texts:?}"
    );
    for want in ["Suggest for you", "Feed instructions"] {
        assert_eq!(
            texts.iter().filter(|t| t.as_str() == want).count(),
            1,
            "{want:?} in {texts:?}"
        );
    }
    // Nothing yet: one empty line for the whole page, then the silence line.
    assert_eq!(
        texts
            .iter()
            .filter(|t| t.as_str() == grokhub_core::pulse::PULSE_EMPTY)
            .count(),
        1,
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "No pulse yet. Nothing to do."),
        "{texts:?}"
    );
}

#[test]
fn pulse_ideas_group_by_category_in_a_fixed_order() {
    let mut quiet = QuietCabin::boot("pulse-groups");
    let cabin = &mut quiet.cabin;
    let now = now_ms();
    cabin.updates.clear();
    cabin.board.clear();
    cabin.skill_list.clear();
    let mut bill = pulse_idea("idea-bill", "TXU bill", "pay my TXU bill before Friday", 1);
    bill.pulse.due_at = Some(now + 2 * 60 * 60 * 1000);
    cabin.updates.push(bill);
    cabin.updates.push(pulse_idea(
        "idea-photos",
        "Vacation photos",
        "sort the vacation photos",
        2,
    ));
    cabin.updates.push(pulse_idea(
        "idea-stand",
        "Laptop stand",
        "compare laptop stand prices",
        3,
    ));
    cabin.updates.push(pulse_idea(
        "idea-gym",
        "Gym",
        "book a gym session for Tuesday",
        4,
    ));
    cabin.updates.push(pulse_idea(
        "idea-dinner",
        "Dinner",
        "plan a dinner with family",
        5,
    ));
    cabin
        .updates
        .push(pulse_idea("idea-standup", "Standup", "draft my standup", 6));
    let mut auto = pulse_idea(
        "idea-auto",
        "Summarize the workboard",
        "every weekday at 9 summarize the workboard",
        7,
    );
    auto.idea_kind = Some(grokhub_core::IdeaKind::Automation);
    cabin.updates.push(auto);
    let groups: Vec<(Option<&str>, Vec<String>)> = cabin
        .pulse_groups(now)
        .into_iter()
        .map(|(cat, rows)| {
            (
                cat.map(|c| c.label()),
                rows.into_iter().map(|r| r.id).collect(),
            )
        })
        .collect();
    assert_eq!(
        groups,
        vec![
            (
                None,
                vec![
                    "idea-bill".to_string(),
                    "idea-standup".to_string(),
                    "idea-dinner".to_string()
                ]
            ),
            (Some("Productivity"), vec!["idea-auto".to_string()]),
            (Some("Shopping"), vec!["idea-stand".to_string()]),
            (Some("Health & Fitness"), vec!["idea-gym".to_string()]),
            (Some("More for you"), vec!["idea-photos".to_string()]),
        ],
        "the three strongest lead, then the categories in their fixed order"
    );
    let texts = pulse_texts(cabin, 2400.0);
    let at = |needle: &str| {
        texts
            .iter()
            .position(|t| t == needle)
            .unwrap_or_else(|| panic!("missing {needle:?} in {texts:?}"))
    };
    // The stream keeps that rank order with no section headers between rows.
    let order = [
        at("I can pay your TXU bill before Friday"),
        at("I can draft your standup"),
        at("I can plan a dinner with family"),
        at("I can summarize the workboard automatically"),
        at("I can sort the vacation photos"),
    ];
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "{order:?} in {texts:?}"
    );
    for header in [
        "UP NEXT",
        "PRODUCTIVITY",
        "SHOPPING",
        "HEALTH & FITNESS",
        "MORE FOR YOU",
        "FINANCIAL MANAGEMENT",
    ] {
        assert!(!texts.iter().any(|t| t == header), "{header}: {texts:?}");
    }
    assert!(
        texts.iter().any(|t| t == "AUTOMATE") && texts.iter().any(|t| t == "DO"),
        "{texts:?}"
    );
    assert_eq!(
        texts.iter().filter(|t| t.as_str() == "···").count(),
        7,
        "one ··· menu per row"
    );
}

#[cfg(unix)]
#[test]
fn pulse_run_sends_the_ideas_prompt_as_bg() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("pulse-run");
    cabin
        .updates
        .push(pulse_idea("idea-run", "Checks", "run the checks", 5));
    assert_eq!(
        cabin.pulse_run("idea-run").as_deref(),
        Some("/bg run the checks")
    );
    assert!(!cabin.running, "Run never takes the composer");
    assert_eq!(cabin.bg.runs.len(), 1, "{}", cabin.status);
    assert!(
        cabin.status.starts_with("Background · run the checks"),
        "{}",
        cabin.status
    );
    let child = cabin.bg.runs[0].native_session.clone().expect("native run");
    assert!(poll_until(&mut cabin, 8, |c| c.bg.runs.is_empty()));
    let sent = grokhub_agent::session_file(&child)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    assert!(sent.contains("Task: run the checks"), "{sent}");
    assert_eq!(cabin.updates[0].status, UpdateStatus::Opened);
    end_bg_test(root, cabin);
}

#[test]
fn pulse_snooze_dismiss_and_not_this_change_the_page_and_the_ledger() {
    let mut quiet = QuietCabin::boot("pulse-buttons");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    cabin.board.clear();
    cabin.skill_list.clear();
    cabin.updates.push(pulse_idea(
        "idea-a",
        "Summarize the workboard",
        "summarize the workboard",
        10,
    ));
    cabin.updates.push(pulse_idea(
        "idea-b",
        "Summarize the workboard every Friday",
        "summarize the workboard on Friday",
        11,
    ));
    cabin
        .updates
        .push(pulse_idea("idea-c", "Standup", "draft my standup", 12));
    cabin.updates.push(pulse_idea(
        "idea-d",
        "Vacation photos",
        "sort the vacation photos",
        13,
    ));
    let now = 1_000_000_035_000;

    // Snooze: hidden until the next 09:00 local, to the millisecond.
    let until = cabin.pulse_snooze_at("idea-d", now, 14, 30);
    assert_eq!(until, 1_000_066_620_000);
    assert_eq!(cabin.updates[3].pulse.snoozed_until, 1_000_066_620_000);
    assert!(!cabin
        .pulse_ranked(1_000_066_619_999)
        .iter()
        .any(|r| r.id == "idea-d"));
    assert!(cabin
        .pulse_ranked(1_000_066_620_000)
        .iter()
        .any(|r| r.id == "idea-d"));

    assert_eq!(
        pulse_ids(cabin, now),
        vec![
            ("idea-c".to_string(), 150, "Do"),
            ("idea-b".to_string(), 150, "Do"),
            ("idea-a".to_string(), 150, "Do"),
        ]
    );

    // Not this: a dislike line, the card goes, and its topic sinks next pass.
    cabin.pulse_not_this("idea-a", "2026-10-04");
    assert!(!cabin.updates.iter().any(|c| c.id == "idea-a"));
    assert!(
        pulse_memory().contains(
            "- [2026-10-04] pulse: dismissed \"Summarize the workboard\" reason=not-this"
        ),
        "{}",
        pulse_memory()
    );
    assert_eq!(
        pulse_ids(cabin, now),
        vec![
            ("idea-c".to_string(), 150, "Do"),
            ("idea-b".to_string(), -250, "Quiet")
        ]
    );
    let groups = cabin.pulse_groups(now);
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0]
            .1
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        vec!["idea-c"]
    );
    assert_eq!(cabin.cfg.feed_pulse.taste_since_rewrite, 1);

    // Dismiss unused: the card is removed and the ledger says so (negative).
    cabin.pulse_dismiss("idea-c", "2026-10-04");
    assert!(!cabin.updates.iter().any(|c| c.id == "idea-c"));
    assert!(pulse_memory().contains("- [2026-10-04] pulse: dismissed \"Standup\" reason=dismiss"));
    assert_eq!(
        cabin.cfg.feed_pulse.taste_since_rewrite, 1,
        "a plain dismiss is not a taste line"
    );
    assert_eq!(
        pulse_ids(cabin, now),
        vec![("idea-b".to_string(), -250, "Quiet")]
    );
    assert!(cabin.pulse_groups(now).is_empty());
}

#[test]
fn used_idea_dismiss_and_liked_feed_dismiss_are_not_negative() {
    let mut quiet = QuietCabin::boot("pulse-used-dismiss");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    cabin.board.clear();
    cabin.skill_list.clear();
    cabin.sources.reactions = Some(grokhub_core::reactions::Reactions::default());

    let mut used = pulse_idea("idea-used", "Tidy the workboard", "tidy the workboard", 20);
    used.status = UpdateStatus::Opened;
    cabin.updates.push(used);
    cabin.updates.push(pulse_idea(
        "idea-fresh",
        "Draft a packing list",
        "draft a packing list",
        21,
    ));
    let mut liked = grokhub_core::digest_card("src-liked", "Rust 1.95 ships", "Notes.", 30);
    liked.id = "feed-liked".into();
    liked.reaction = Some(grokhub_core::CardReaction::Up);
    cabin.updates.push(liked);
    let mut discussed = grokhub_core::digest_card("src-talk", "NFL scores", "Notes.", 31);
    discussed.id = "feed-talk".into();
    discussed.discuss_thread = Some("thr-nfl".into());
    cabin.updates.push(discussed);
    let mut fresh = grokhub_core::digest_card("src-fresh", "Nightly backup", "Notes.", 32);
    fresh.id = "feed-fresh".into();
    cabin.updates.push(fresh);

    // Used For You card: ledger reason=used, no Dismissed reaction.
    cabin.react_to_act(&super::pulse_ui::PulseAct::Dismiss("idea-used".into()));
    cabin.pulse_dismiss("idea-used", "2026-10-28");
    assert!(
        pulse_memory().contains("- [2026-10-28] pulse: used \"Tidy the workboard\" reason=used"),
        "{}",
        pulse_memory()
    );
    assert!(!cabin.updates.iter().any(|c| c.id == "idea-used"));
    assert!(
        cabin.reactions_now().log.is_empty(),
        "used+dismissed must not record a negative reaction: {:?}",
        cabin.reactions_now().log
    );

    // Unused For You dismiss stays negative in the ledger and reactions.
    cabin.react_to_act(&super::pulse_ui::PulseAct::Dismiss("idea-fresh".into()));
    cabin.pulse_dismiss("idea-fresh", "2026-10-28");
    assert!(pulse_memory()
        .contains("- [2026-10-28] pulse: dismissed \"Draft a packing list\" reason=dismiss"));
    assert!(
        cabin
            .reactions_now()
            .log
            .iter()
            .any(|r| r.outcome == grokhub_core::reactions::Outcome::Dismissed),
        "unused dismiss records Dismissed: {:?}",
        cabin.reactions_now().log
    );

    // Liked / discussed Feed then dismiss: used (never a bury in the ledger).
    cabin.react_to_act(&super::pulse_ui::PulseAct::Dismiss("feed-liked".into()));
    cabin.pulse_dismiss("feed-liked", "2026-10-28");
    assert!(
        pulse_memory().contains("- [2026-10-28] pulse: used \"Rust 1.95 ships\" reason=used"),
        "{}",
        pulse_memory()
    );
    cabin.react_to_act(&super::pulse_ui::PulseAct::Dismiss("feed-talk".into()));
    cabin.pulse_dismiss("feed-talk", "2026-10-28");
    assert!(pulse_memory().contains("- [2026-10-28] pulse: used \"NFL scores\" reason=used"));

    // Untouched Feed dismiss: dismiss.
    cabin.react_to_act(&super::pulse_ui::PulseAct::Dismiss("feed-fresh".into()));
    cabin.pulse_dismiss("feed-fresh", "2026-10-28");
    assert!(pulse_memory()
        .contains("- [2026-10-28] pulse: dismissed \"Nightly backup\" reason=dismiss"));
}

#[test]
fn pulse_migrates_old_home_and_ideas_data_once() {
    let mut quiet = QuietCabin::boot("pulse-migrate");
    let cabin = &mut quiet.cabin;
    // What 2.10.90 wrote: a pinned Home idea, a board idea, a digest with a
    // source, an automation run, and an idea you had deleted.
    let old = r#"[
  {"id":"idea-pin","kind":"idea","title":"Pay the TXU bill","body":"Due Friday","createdAt":100,"status":"unread","feedPin":true,"prompt":"pay my TXU bill","ideaKind":"reminder","sourceId":"idea-pin"},
  {"id":"idea-board","kind":"idea","title":"Draft the standup","createdAt":101,"status":"opened","prompt":"draft my standup","ideaKind":"try","modified":true,"draft":"draft my standup from git log"},
  {"id":"digest-1","kind":"digest","title":"Rust 1.92 ships","body":"Faster builds.","createdAt":102,"status":"unread","citations":["https://www.rust-lang.org/news/1-92"],"reaction":"up"},
  {"id":"run-1","kind":"automation_done","title":"Backup ran","body":"All good","createdAt":103,"status":"unread","sourceId":"loop-backup","runs":3},
  {"id":"idea-gone","kind":"idea","title":"Gym plan","createdAt":104,"status":"dismissed"}
]"#;
    std::fs::write(crate::feed::path(), old).expect("old updates.json");
    cabin.updates = crate::feed::load();
    assert_eq!(cabin.updates.len(), 5, "older files load unchanged");
    cabin.cfg.digest_brief = "Rust and egui news".into();
    cabin.cfg.feed_instructions.clear();
    cabin.cfg.feed_pulse.pulse_v1 = false;

    let moved = cabin.migrate_pulse_store().expect("first run migrates");
    assert_eq!(
        moved,
        grokhub_core::pulse::PulseMigration {
            ideas: 2,
            feed: 2,
            pinned: 1,
            kept: 5,
        }
    );
    let cat = |id: &str| {
        cabin
            .updates
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.pulse.category.clone())
    };
    assert_eq!(cat("idea-pin").as_deref(), Some("financial"));
    assert_eq!(cat("idea-board").as_deref(), Some("productivity"));
    assert_eq!(cat("idea-gone").as_deref(), Some("health"));
    assert_eq!(cat("digest-1"), None, "feed posts have no category header");
    let pinned = cabin.updates.iter().find(|c| c.id == "idea-pin").unwrap();
    assert!(pinned.feed_pin, "the Home pin is kept");
    let board = cabin.updates.iter().find(|c| c.id == "idea-board").unwrap();
    assert!(board.modified && board.draft.as_deref() == Some("draft my standup from git log"));
    let digest = cabin.updates.iter().find(|c| c.id == "digest-1").unwrap();
    assert_eq!(digest.pulse.source_name.as_deref(), Some("rust-lang.org"));
    assert_eq!(digest.reaction, Some(grokhub_core::CardReaction::Up));
    assert_eq!(
        cabin.cfg.feed_instructions,
        format!(
            "{}\n\nFocus: Rust and egui news",
            grokhub_core::pulse::DEFAULT_FEED_INSTRUCTIONS
        )
    );
    assert!(cabin.cfg.feed_pulse.pulse_v1);
    assert_eq!(cabin.migrate_pulse_store(), None, "it runs once");

    // Both stores were written: a restart sees the migrated data.
    let mut saved = Vec::new();
    for _ in 0..150 {
        saved = crate::feed::load();
        if saved.iter().any(|c| c.pulse.category.is_some())
            && crate::config::load().feed_pulse.pulse_v1
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(saved.len(), 5);
    assert_eq!(
        saved
            .iter()
            .find(|c| c.id == "idea-pin")
            .and_then(|c| c.pulse.category.clone())
            .as_deref(),
        Some("financial")
    );
    let cfg = crate::config::load();
    assert!(cfg.feed_pulse.pulse_v1);
    assert!(cfg.feed_instructions.ends_with("Focus: Rust and egui news"));
    // The pinned Home idea ranks with the one you were working on (rule 3,
    // unfinished work), ahead of the run you repeat and the digest.
    let now = 1_000;
    assert_eq!(
        pulse_ids(cabin, now),
        vec![
            ("idea-board".to_string(), 400, "Do"),
            // A reminder is scheduled by Apply, so it ranks as Automate (base 140).
            ("idea-pin".to_string(), 390, "Automate"),
            ("run-1".to_string(), 230, "Watch"),
            ("digest-1".to_string(), 110, "Watch"),
        ]
    );
}

#[test]
fn pulse_feed_shows_posts_with_cached_source_images_and_a_placeholder() {
    let mut quiet = QuietCabin::boot("pulse-feed");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    let now = now_ms();
    let mut post = grokhub_core::digest_card(
        "digest",
        "Rust 1.92 ships",
        "Faster incremental builds and a new lint.",
        now - 59 * 60 * 1000,
    );
    post.citations = vec!["https://www.rust-lang.org/news/1-92".into()];
    post.pulse.image_urls = vec!["https://www.rust-lang.org/static/og.png".into()];
    cabin.updates.push(post);
    let mut run = grokhub_core::automation_done_card(
        "loop-backup",
        "Backup ran",
        "Nothing new to copy.",
        now - 3 * 60 * 60 * 1000,
    );
    run.pulse.image_urls = vec!["https://cdn.example.com/missing.jpg".into()];
    cabin.updates.push(run);
    // A local fixture stands in for the downloaded og:image.
    let fixture = super::pulse_ui::pulse_image_path("https://www.rust-lang.org/static/og.png");
    std::fs::create_dir_all(fixture.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(8, 4, image::Rgba([200, 80, 40, 255]))
        .save_with_format(&fixture, image::ImageFormat::Png)
        .expect("fixture png");
    pulse_texts(cabin, 1200.0);
    // The thumb decodes off-thread; on a busy runner it can miss the second pass.
    super::pulse_ui::wait_pulse_decodes();
    let texts = pulse_texts(cabin, 1200.0);
    for want in [
        "Rust 1.92 ships",
        "rust-lang.org · 59m ago",
        "Faster incremental builds and a new lint.",
        "Read at rust-lang.org",
        "Backup ran",
        "Automation run · 3h ago",
        "Discuss",
        "Feed instructions",
    ] {
        assert!(
            texts.iter().any(|t| t == want),
            "missing {want:?} in {texts:?}"
        );
    }
    assert_eq!(texts.iter().filter(|t| t.as_str() == "Like").count(), 2);
    let posts: Vec<String> = cabin
        .pulse_feed_posts(now)
        .into_iter()
        .map(|c| c.title)
        .collect();
    assert_eq!(
        posts,
        vec!["Rust 1.92 ships".to_string(), "Backup ran".to_string()]
    );
    // The thumb decodes off the UI thread; a slow runner needs more frames.
    let og = "https://www.rust-lang.org/static/og.png";
    for _ in 0..250 {
        if cabin.pulse_view_texture_size(og).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        pulse_texts(cabin, 1200.0);
    }
    let tex = cabin.pulse_view_texture_size(og);
    assert_eq!(tex, Some([8, 4]), "the cached source image is drawn");
    assert_eq!(
        cabin.pulse_view_texture_size("https://cdn.example.com/missing.jpg"),
        None,
        "no file: a placeholder"
    );
    assert!(
        !cabin.pulse_image_failed("https://cdn.example.com/missing.jpg"),
        "not on disk yet: still loading, so the slot stays"
    );
    assert!(
        !texts.iter().any(|t| t.contains("· Watch")),
        "the source line names the source, not the card type: {texts:?}"
    );

    // Like: a ledger line and the heart stays on.
    let id = cabin.updates[0].id.clone();
    cabin.pulse_like(&id, "2026-10-04");
    cabin.pulse_like(&id, "2026-10-04");
    assert_eq!(
        pulse_memory()
            .matches("pulse: liked \"Rust 1.92 ships\" reason=liked")
            .count(),
        1
    );
    assert!(pulse_texts(cabin, 1200.0).iter().any(|t| t == "Liked"));
}

#[test]
fn pulse_feed_instructions_sheet_edits_the_prompt_for_future_posts() {
    let mut quiet = QuietCabin::boot("pulse-sheet");
    let cabin = &mut quiet.cabin;
    cabin.open_feed_instructions();
    assert_eq!(
        cabin.pulse_view.sheet.as_deref(),
        Some(grokhub_core::pulse::DEFAULT_FEED_INSTRUCTIONS)
    );
    let texts = pulse_texts(cabin, 900.0);
    for want in [
        "Feed instructions",
        "Save",
        "Cancel",
        "×",
        "I'll update these after 3 more likes or skips.",
        "Ctrl+Enter to save · Esc to cancel",
    ] {
        assert!(
            texts.iter().any(|t| t == want),
            "missing {want:?} in {texts:?}"
        );
    }
    assert!(
        cabin.pulse_view.sheet.is_some(),
        "the sheet stays open until Save or Cancel"
    );
    cabin.save_feed_instructions("  Only Rust and egui news. No crypto.  ");
    assert_eq!(
        cabin.cfg.feed_instructions,
        "Only Rust and egui news. No crypto."
    );
    let prompt = grokhub_core::pulse::feed_prompt("steer", "", &cabin.cfg.feed_instructions);
    assert!(
        prompt.ends_with("Only Rust and egui news. No crypto."),
        "{prompt}"
    );
    cabin.save_feed_instructions(grokhub_core::pulse::DEFAULT_FEED_INSTRUCTIONS);
    assert_eq!(
        cabin.cfg.feed_instructions, "",
        "our default is not stored as yours"
    );
    let native = include_str!("native_unattended.rs");
    // rustfmt wraps the three feed_prompt args onto their own lines.
    assert!(
        native.contains("grokhub_core::pulse::feed_prompt(")
            && native.contains("&self.digest_steer,")
            && native.contains("&self.digest_interests,")
            && native.contains("&self.cfg.feed_instructions,"),
        "digest must pass steer, interests, and feed_instructions into feed_prompt"
    );
}

/// Text shapes with their left edge, for "which comes first" checks.
fn pulse_text_at(cabin: &mut super::Cabin, h: f32, events: Vec<egui::Event>) -> Vec<(String, f32)> {
    pulse_text_at_w(cabin, 1400.0, h, events)
}

fn pulse_text_at_w(
    cabin: &mut super::Cabin,
    w: f32,
    h: f32,
    events: Vec<egui::Event>,
) -> Vec<(String, f32)> {
    let _paint = crate::theme::hold_paint_test();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let screen = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(w, h),
    ));
    let mut out = Vec::new();
    // First pass carries the keys; the second paints what they did.
    for events in [events, Vec::new()] {
        let raw = egui::RawInput {
            screen_rect: screen,
            events,
            ..Default::default()
        };
        out.clear();
        let shapes = crate::theme::test_pass(&ctx, raw, |ui| cabin.ui_pulse(ui)).shapes;
        for clipped in &shapes {
            if let egui::Shape::Text(t) = &clipped.shape {
                out.push((t.galley.job.text.clone(), t.pos.x));
            }
        }
    }
    out
}

fn pulse_key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn pulse_header_shows_both_actions_and_no_tabs_at_every_width() {
    let mut quiet = QuietCabin::boot("pulse-header");
    let cabin = &mut quiet.cabin;
    let x = |texts: &[(String, f32)], needle: &str| {
        texts
            .iter()
            .find(|(t, _)| t == needle)
            .map(|(_, x)| *x)
            .unwrap_or_else(|| panic!("missing {needle:?} in {texts:?}"))
    };
    for w in [2560.0, 1100.0, NARROW_W, 400.0] {
        let texts = pulse_text_at_w(cabin, w, 900.0, Vec::new());
        let title = x(&texts, "Pulse");
        let (suggest, steer) = (x(&texts, "Suggest for you"), x(&texts, "Feed instructions"));
        assert!(
            title < suggest && suggest < steer && steer < w,
            "Suggest then Feed instructions, right of the title, on screen at {w}: {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|(t, _)| t == "Feed" || t == "For You" || t == "FROM YOUR FEED"),
            "no tab switch or feed heading at {w}: {texts:?}"
        );
    }
}

#[test]
fn pulse_rows_show_run_snooze_dismiss_on_focus_and_keys_act_on_the_focused_row() {
    let mut quiet = QuietCabin::boot("pulse-keys");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    cabin.board.clear();
    cabin.skill_list.clear();
    cabin.updates.push(pulse_idea(
        "idea-trail",
        "Trail map",
        "print the trail map",
        10,
    ));
    cabin.updates.push(pulse_idea(
        "idea-beach",
        "Beach photos",
        "sort the beach photos",
        11,
    ));
    cabin.updates.push(pulse_idea(
        "idea-city",
        "City guide",
        "save the city guide",
        12,
    ));
    let rows: Vec<String> = cabin
        .pulse_groups(now_ms())
        .into_iter()
        .flat_map(|(_, rows)| rows.into_iter().map(|r| r.id))
        .collect();
    assert_eq!(rows, vec!["idea-city", "idea-beach", "idea-trail"]);
    let count =
        |texts: &[(String, f32)], needle: &str| texts.iter().filter(|(t, _)| t == needle).count();

    // At rest: no inline actions, just ··· on each row.
    let rest = pulse_text_at(cabin, 1200.0, Vec::new());
    assert_eq!(
        (
            count(&rest, "Run"),
            count(&rest, "Snooze"),
            count(&rest, "Dismiss")
        ),
        (0, 0, 0)
    );
    assert_eq!(count(&rest, "···"), 3);

    // Down focuses the first row; its Run · Snooze · Dismiss show inline.
    let down = pulse_text_at(cabin, 1200.0, vec![pulse_key_event(egui::Key::ArrowDown)]);
    assert_eq!(cabin.pulse_view.focus.as_deref(), Some("idea-city"));
    assert_eq!(
        (
            count(&down, "Run"),
            count(&down, "Snooze"),
            count(&down, "Dismiss")
        ),
        (1, 1, 1)
    );
    // J moves down, K back up.
    assert_eq!(cabin.pulse_key(egui::Key::J, &rows), None);
    assert_eq!(cabin.pulse_view.focus.as_deref(), Some("idea-beach"));
    assert_eq!(cabin.pulse_key(egui::Key::K, &rows), None);
    assert_eq!(cabin.pulse_view.focus.as_deref(), Some("idea-city"));
    // R runs the focused row; S snoozes it and focus moves on.
    assert_eq!(
        cabin.pulse_key(egui::Key::R, &rows),
        Some(super::pulse_ui::PulseAct::Run("idea-city".into()))
    );
    assert_eq!(
        cabin.pulse_key(egui::Key::S, &rows),
        Some(super::pulse_ui::PulseAct::Snooze("idea-city".into()))
    );
    assert_eq!(cabin.pulse_view.focus.as_deref(), Some("idea-beach"));

    // D through the page: the focused row goes, the ledger says so, focus moves on.
    let _ = pulse_text_at(cabin, 1200.0, vec![pulse_key_event(egui::Key::D)]);
    assert!(!cabin.updates.iter().any(|c| c.id == "idea-beach"));
    assert!(
        pulse_memory().contains("pulse: dismissed \"Beach photos\" reason=dismiss"),
        "{}",
        pulse_memory()
    );
    assert_eq!(cabin.pulse_view.focus.as_deref(), Some("idea-trail"));

    // N through the page: Not this on the last row, and focus steps back.
    let _ = pulse_text_at(cabin, 1200.0, vec![pulse_key_event(egui::Key::N)]);
    assert!(!cabin.updates.iter().any(|c| c.id == "idea-trail"));
    assert!(
        pulse_memory().contains("pulse: dismissed \"Trail map\" reason=not-this"),
        "{}",
        pulse_memory()
    );
    assert_eq!(cabin.pulse_view.focus.as_deref(), Some("idea-city"));

    // Esc lets go; with nothing focused R does nothing.
    let left = vec!["idea-city".to_string()];
    assert_eq!(cabin.pulse_key(egui::Key::Escape, &left), None);
    assert_eq!(cabin.pulse_view.focus, None);
    assert_eq!(cabin.pulse_key(egui::Key::R, &left), None);
}

#[test]
fn pulse_suggest_signed_out_says_how_to_sign_in_and_loading_shows_placeholder_rows() {
    let mut quiet = QuietCabin::boot("pulse-empty");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    cabin.board.clear();
    let count = |texts: &[String], needle: &str| texts.iter().filter(|t| *t == needle).count();
    let empty = pulse_texts_w(cabin, NARROW_W, 900.0);
    // No ideas and no posts: one empty state, not one per kind.
    assert_eq!(
        count(&empty, grokhub_core::pulse::PULSE_EMPTY),
        1,
        "{empty:?}"
    );
    assert_eq!(
        count(&empty, "No pulse yet. Nothing to do."),
        1,
        "{empty:?}"
    );
    assert!(
        !empty
            .iter()
            .any(|t| t.starts_with("Nothing for you yet") || t.starts_with("No posts yet")),
        "{empty:?}"
    );
    // The header carries the only Suggest for you and Feed instructions.
    assert_eq!(count(&empty, "Suggest for you"), 1, "{empty:?}");
    assert_eq!(count(&empty, "Feed instructions"), 1, "{empty:?}");
    assert!(!empty.iter().any(|t| t.contains("Thinking")), "{empty:?}");
    cabin.suggest_ideas_with(false);
    assert!(cabin.pulse_view.signin_note);
    assert_eq!(cabin.status, "Connect Grok in Settings to get ideas");
    let signed_out = pulse_texts_w(cabin, NARROW_W, 900.0);
    assert!(
        cabin.pulse_view.signin_note,
        "note stays until Account/native signs in"
    );
    for want in ["Sign in to Grok to get ideas.", "Open Settings"] {
        assert!(
            signed_out.iter().any(|t| t == want),
            "missing {want:?} in {signed_out:?}"
        );
    }
    // PI-05: once signed in, the amber line clears without another press.
    cabin.secrets.api_key = "sk-abcdefghijklmnopqrstuv".into();
    let _ = pulse_texts_w(cabin, NARROW_W, 900.0);
    assert!(!cabin.pulse_view.signin_note);
    // While a suggestion call runs: placeholder rows, and no empty line beside them.
    let (_tx, rx) = mpsc::channel::<String>();
    cabin.ideas_rx = Some((rx, Default::default()));
    let loading = pulse_texts_w(cabin, NARROW_W, 900.0);
    assert!(
        loading
            .iter()
            .any(|t| t == "Looking for ideas in your recent work…"),
        "{loading:?}"
    );
    assert!(loading.iter().any(|t| t == "Suggesting…"), "{loading:?}");
    assert!(
        !loading
            .iter()
            .any(|t| t == grokhub_core::pulse::PULSE_EMPTY || t.ends_with("Nothing to do.")),
        "{loading:?}"
    );
    // Loading with posts on the page: the placeholder rows, then the posts.
    let now = now_ms();
    cabin.updates.push(grokhub_core::digest_card(
        "digest",
        "Rust 1.92 ships",
        "Faster incremental builds.",
        now - 60_000,
    ));
    let both = pulse_text_at_w(cabin, NARROW_W, 900.0, Vec::new());
    let at = |needle: &str| {
        both.iter()
            .position(|(t, _)| t == needle)
            .unwrap_or_else(|| panic!("missing {needle:?} in {both:?}"))
    };
    assert!(at("Looking for ideas in your recent work…") < at("Rust 1.92 ships"));
    cabin.ideas_rx = None;
    // Only posts, no ideas: the posts with no empty panel above them.
    let posts_only = pulse_texts_w(cabin, NARROW_W, 900.0);
    assert!(posts_only.iter().any(|t| t == "Rust 1.92 ships"));
    assert!(
        !posts_only
            .iter()
            .any(|t| t == grokhub_core::pulse::PULSE_EMPTY || t.ends_with("Nothing to do.")),
        "{posts_only:?}"
    );
}

#[test]
fn pulse_opened_idea_heading_matches_row_type() {
    use grokhub_core::pulse::PulseType;
    assert_eq!(PulseType::Do.apply_heading(), "What Apply will do");
    assert_eq!(
        PulseType::Automate.apply_heading(),
        "What Apply will schedule"
    );
    assert_eq!(PulseType::Learn.apply_heading(), "What I'll learn");
    let src = include_str!("ideas_ui.rs");
    let open = src
        .split("fn paint_idea_open(")
        .nth(1)
        .and_then(|s| s.split("fn apply_idea_act(").next())
        .unwrap_or(src);
    assert!(
        open.contains("pulse_type(card).apply_heading()"),
        "opened card must use the same type source as the row"
    );
    let skel = include_str!("pulse_ui.rs");
    let paint = skel
        .split("fn paint_skeleton_row(")
        .nth(1)
        .and_then(|s| s.split("fn paint_thumbs(").next())
        .expect("skeleton");
    assert!(
        paint.contains("theme::elevated()") && paint.contains("52.0"),
        "loading Ideas rows are #16181c blocks the size of a row: {paint}"
    );
    let loading = skel
        .split("Thumb::Loading =>")
        .nth(1)
        .and_then(|s| s.split('}').next())
        .expect("loading thumb");
    assert!(
        loading.contains("theme::elevated()") && !loading.to_ascii_lowercase().contains("icon"),
        "loading image is a plain #16181c skeleton with no icon: {loading}"
    );
}

#[test]
fn pulse_is_one_centered_column_at_every_width() {
    use super::pulse_ui::{post_thumb_count, pulse_column, STREAM_MAX_W};
    // Page widths, padding included: one column, capped, narrow under 720.
    for (page_w, width, narrow) in [
        (2560.0, STREAM_MAX_W, false),
        (1600.0, STREAM_MAX_W, false),
        (1100.0, STREAM_MAX_W, false),
        (720.0, 672.0, false),
        (400.0, 352.0, true),
    ] {
        let col = pulse_column(page_w);
        assert_eq!((col.width, col.narrow), (width, narrow), "at {page_w}");
    }
    assert!(pulse_column(719.0).narrow);
    assert_eq!(pulse_column(0.0).width, 0.0);
    // Posts under 560 show two source images, not three.
    assert_eq!(post_thumb_count(559.0), 2);
    assert_eq!(post_thumb_count(560.0), 3);
    assert_eq!(post_thumb_count(STREAM_MAX_W), 3);
}

#[test]
fn the_painted_stream_is_centered_and_never_wider_than_its_cap() {
    use super::pulse_ui::STREAM_MAX_W;
    let mut quiet = QuietCabin::boot("pulse-center");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    let now = now_ms();
    cabin.updates.push(grokhub_core::digest_card(
        "digest",
        "Rust 1.92 ships",
        "Faster incremental builds.",
        now - 60_000,
    ));
    for w in [2560.0_f32, 1600.0, 1100.0, 720.0, 400.0] {
        let texts = pulse_text_pos(cabin, w);
        let title = texts
            .iter()
            .find(|(t, _)| t == "Pulse")
            .map(|(_, p)| p.x)
            .unwrap_or_else(|| panic!("no title at {w}: {texts:?}"));
        let post = texts
            .iter()
            .find(|(t, _)| t == "Rust 1.92 ships")
            .map(|(_, p)| p.x)
            .unwrap_or_else(|| panic!("no post at {w}: {texts:?}"));
        let left = ((w - STREAM_MAX_W) * 0.5).max(24.0);
        assert!(
            (title - left).abs() < 12.0,
            "title {title} near the column's left edge {left} at {w}"
        );
        assert!(
            title < post && post < title + 80.0,
            "the post sits in the same column at {w}: {texts:?}"
        );
    }
}

#[test]
fn suggest_label_says_when_it_is_busy() {
    use super::pulse_ui::suggest_label;
    assert_eq!(suggest_label(false), "Suggest for you");
    assert_eq!(suggest_label(true), "Suggesting…");
}

/// Deck-match 6: Pulse header/cards follow slides 13 and 15 chrome.
#[test]
fn pulse_deck_match_6_chrome_tokens() {
    let _paint = crate::theme::hold_paint_test();
    crate::theme::set_paint_palette(crate::theme::PaintPalette::OpsDark);
    let pulse = include_str!("pulse_ui.rs");
    let ideas = include_str!("ideas_ui.rs");
    assert!(
        pulse.contains("recipe::STROKE_W") && pulse.contains("signal_red()"),
        "idea icon tiles must use the signal-red recipe rim"
    );
    assert!(
        ideas.contains("paint_pulse_row(") && ideas.contains("self.paint_idea_open("),
        "ideas in the stream keep their row and opened-card painters"
    );
    assert_eq!(
        super::pulse_ui::type_color(grokhub_core::pulse::PulseType::Automate),
        crate::theme::amber(),
        "AUTOMATE tag is amber on the deck"
    );
    assert!(
        pulse.contains("Quick::Primary")
            && pulse.contains("Quick::Outline")
            && pulse.contains("Quick::Quiet"),
        "Run / Snooze / Dismiss keep the Loadout button styles"
    );
}

/// Text shapes with their top-left corner.
fn pulse_text_pos(cabin: &mut super::Cabin, w: f32) -> Vec<(String, egui::Pos2)> {
    let _paint = crate::theme::hold_paint_test();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w, 1200.0),
        )),
        ..Default::default()
    };
    let mut out = Vec::new();
    for _ in 0..2 {
        out.clear();
        let shapes = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_pulse(ui)).shapes;
        for clipped in &shapes {
            if let egui::Shape::Text(t) = &clipped.shape {
                out.push((t.galley.job.text.clone(), t.pos));
            }
        }
    }
    out
}

#[test]
fn pulse_mixes_ideas_and_posts_in_one_column_and_narrow_rows_keep_run() {
    let mut quiet = QuietCabin::boot("pulse-stream");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    cabin.board.clear();
    cabin.skill_list.clear();
    cabin.updates.push(pulse_idea(
        "idea-trail",
        "Trail map",
        "print the trail map",
        10,
    ));
    cabin.updates.push(pulse_idea(
        "idea-beach",
        "Beach photos",
        "sort the beach photos",
        11,
    ));
    let now = now_ms();
    for (i, title) in ["Rust 1.92 ships", "egui 0.33 lands", "Steam sale starts"]
        .iter()
        .enumerate()
    {
        cabin.updates.push(grokhub_core::digest_card(
            &format!("digest-{i}"),
            title,
            "Two lines about it.",
            now - (i as u64 + 1) * 60 * 60 * 1000,
        ));
    }
    let at = |texts: &[(String, egui::Pos2)], needle: &str| {
        texts
            .iter()
            .find(|(t, _)| t == needle)
            .map(|(_, p)| *p)
            .unwrap_or_else(|| panic!("missing {needle:?} in {texts:?}"))
    };
    let count = |texts: &[(String, egui::Pos2)], needle: &str| {
        texts.iter().filter(|(t, _)| t == needle).count()
    };

    // An idea, two posts, the next idea, then the post that is left.
    let wide = pulse_text_pos(cabin, 1400.0);
    assert!(!cabin.pulse_view.narrow);
    let order = [
        at(&wide, "I can sort the beach photos"),
        at(&wide, "Rust 1.92 ships"),
        at(&wide, "egui 0.33 lands"),
        at(&wide, "I can print the trail map"),
        at(&wide, "Steam sale starts"),
    ];
    assert!(
        order.windows(2).all(|w| w[0].y < w[1].y),
        "one list, top to bottom: {wide:?}"
    );
    let (min_x, max_x) = order.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(p.x), hi.max(p.x))
    });
    assert!(max_x - min_x < 80.0, "one column, not two: {wide:?}");
    for gone in ["UP NEXT", "FROM YOUR FEED", "Feed", "For You"] {
        assert_eq!(count(&wide, gone), 0, "{gone}: {wide:?}");
    }
    // Rows rest without inline actions; every idea and post keeps its ···.
    assert_eq!((count(&wide, "Run"), count(&wide, "Snooze")), (0, 0));
    assert_eq!(count(&wide, "···"), 5);
    assert_eq!(count(&wide, "Like"), 3);

    // Narrow: Run stays on every idea row; Snooze and Dismiss wait in ···.
    let narrow = pulse_text_pos(cabin, NARROW_W);
    assert!(cabin.pulse_view.narrow);
    assert_eq!(
        (
            count(&narrow, "Run"),
            count(&narrow, "Snooze"),
            count(&narrow, "Dismiss"),
            count(&narrow, "···")
        ),
        (2, 0, 0, 5),
        "{narrow:?}"
    );
    assert!(at(&narrow, "I can sort the beach photos").y < at(&narrow, "Rust 1.92 ships").y);

    // Keys walk the idea rows in stream order and skip the posts.
    let _ = pulse_text_at(cabin, 1200.0, vec![pulse_key_event(egui::Key::ArrowDown)]);
    assert_eq!(cabin.pulse_view.focus.as_deref(), Some("idea-beach"));
    let _ = pulse_text_at(cabin, 1200.0, vec![pulse_key_event(egui::Key::J)]);
    assert_eq!(cabin.pulse_view.focus.as_deref(), Some("idea-trail"));
}

#[test]
fn pulse_source_note_waits_behind_details() {
    assert!(!super::pulse_ui::PulseView::default().sources_open);
    let mut quiet = QuietCabin::boot("pulse-source-note");
    let cabin = &mut quiet.cabin;
    cabin.updates.clear();
    cabin.updates.push(grokhub_core::digest_card(
        "digest",
        "Rust 1.92 ships",
        "Faster incremental builds.",
        now_ms() - 60_000,
    ));
    let note = "Checked 15 sources (15 from the network). Failed: Reddit: HTTP 403 and 2 more.";
    cabin.sources.note = note.into();
    let shut = pulse_texts(cabin, 900.0);
    assert!(shut.iter().any(|t| t == "1 post on Pulse"), "{shut:?}");
    assert!(shut.iter().any(|t| t == "Details"), "{shut:?}");
    assert!(
        !shut.iter().any(|t| t.contains("Failed")),
        "the check note stays tucked away: {shut:?}"
    );
    cabin.pulse_view.sources_open = true;
    let open = pulse_texts(cabin, 900.0);
    assert!(open.iter().any(|t| t == note), "{open:?}");
    assert!(open.iter().any(|t| t == "Hide details"), "{open:?}");
    // No note: just the count, nothing to open.
    cabin.sources.note.clear();
    let plain = pulse_texts(cabin, 900.0);
    assert!(plain.iter().any(|t| t == "1 post on Pulse"), "{plain:?}");
    assert!(
        !plain.iter().any(|t| t == "Details" || t == "Hide details"),
        "{plain:?}"
    );
}

#[test]
fn pulse_offers_the_off_helpers_once_each() {
    use super::helper_offers_ui::HelperOffer;
    let mut quiet = QuietCabin::boot("pulse-helper-offers");
    let cabin = &mut quiet.cabin;
    // Fresh profile: both helpers are off, so Pulse offers both.
    assert!(!cabin.cfg.downloads_tidy);
    assert_eq!(
        cabin.helper_offers(),
        vec![HelperOffer::WindDown, HelperOffer::Tidy]
    );
    let texts = pulse_texts(cabin, 900.0);
    for ask in [
        "Offer to sleep the PC late at night?",
        "Suggest Downloads cleanups?",
    ] {
        assert!(
            texts.iter().any(|t| t == ask),
            "{ask:?} not on Pulse: {texts:?}"
        );
    }
    assert_eq!(
        texts.iter().filter(|t| *t == "Turn on").count(),
        2,
        "{texts:?}"
    );

    // Turn on: the helper is on and the offer goes away.
    cabin.answer_helper_offer(HelperOffer::Tidy, true);
    assert!(cabin.cfg.downloads_tidy);
    assert_eq!(
        cabin.status,
        "Downloads cleanups are on. Turn them off in Settings › Behavior."
    );
    assert_eq!(cabin.helper_offers(), vec![HelperOffer::WindDown]);

    // Not now: still off, never offered again.
    cabin.answer_helper_offer(HelperOffer::WindDown, false);
    assert!(!cabin.wind_down_on());
    assert_eq!(
        cabin.cfg.helper_offers_dismissed,
        vec!["wind_down".to_string()]
    );
    // It survives a restart: the saved config has it.
    cabin.persist_svc.flush();
    assert_eq!(
        crate::config::load().helper_offers_dismissed,
        vec!["wind_down".to_string()]
    );
    assert_eq!(cabin.helper_offers(), Vec::<HelperOffer>::new());
    let texts = pulse_texts(cabin, 900.0);
    assert!(
        !texts.iter().any(|t| t == "Turn on" || t == "Not now"),
        "{texts:?}"
    );

    // Turning wind-down on from the offer flips the same switch as Settings.
    cabin.cfg.helper_offers_dismissed.clear();
    cabin.answer_helper_offer(HelperOffer::WindDown, true);
    assert!(cabin.wind_down_on());
    assert_eq!(
        cabin.status,
        "Late-night wind-down is on. Turn it off in Settings › Behavior."
    );
    cabin.set_wind_on(false);

    // A wind-down switch that can't be saved never claims to be on.
    let file = crate::config::config_dir().join(grokhub_core::wind_down::FILE_NAME);
    let _ = std::fs::remove_file(&file);
    std::fs::create_dir_all(&file).unwrap();
    cabin.status.clear();
    cabin.answer_helper_offer(HelperOffer::WindDown, true);
    assert!(!cabin.status.is_empty());
    assert_ne!(
        cabin.status,
        "Late-night wind-down is on. Turn it off in Settings › Behavior."
    );
    // The switch rolls back, so the offer and Settings still read it as off.
    assert!(!cabin.wind_down_on());
    std::fs::remove_dir_all(&file).unwrap();
    cabin.set_wind_on(false);
    quiet.settle();
}

#[test]
fn behavior_settings_list_the_pc_helpers_first() {
    let src = include_str!("settings.rs");
    let behavior = src
        .split("SettingsSec::Behavior => {")
        .nth(1)
        .expect("Behavior section");
    let helpers = behavior.find("\"PC helpers\"").expect("PC helpers header");
    let tray = behavior
        .find("\"Tray & wall\"")
        .expect("Tray & wall header");
    let wind = behavior
        .find("ui_wind_down_row(ui)")
        .expect("wind-down row");
    let tidy = behavior
        .find("ui_downloads_tidy_row(ui)")
        .expect("tidy row");
    assert!(
        helpers < wind && wind < tidy && tidy < tray,
        "helpers come first"
    );
    assert_eq!(src.matches("ui_wind_down_row(ui)").count(), 1);
    assert_eq!(src.matches("ui_downloads_tidy_row(ui)").count(), 1);
}
