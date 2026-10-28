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

/// A window narrow enough for the compact page (one view behind the switch).
const COMPACT_W: f32 = 640.0;

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
    crate::config::read_memory("MEMORY.md")
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
        .all(|(id, label)| *id != "ideas" && *label != "Ideas"));
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
            .any(|t| t == "What I'd do next, from how you use the cabin."),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "Ideas") && texts.iter().any(|t| t == "Feed"),
        "{texts:?}"
    );
    // Pulse opens on Feed; its empty line says how to steer it (link is separate).
    assert!(
        texts
            .iter()
            .any(|t| t == "No posts yet. Tell me what to watch in "),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t == "Feed instructions"), "{texts:?}");
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
            (Some("More ideas"), vec!["idea-photos".to_string()]),
        ],
        "the three strongest lead, then the categories in their fixed order"
    );
    cabin.pulse_view.tab = super::pulse_ui::PulseTab::Ideas;
    let texts = pulse_texts(cabin, 2400.0);
    let at = |needle: &str| {
        texts
            .iter()
            .position(|t| t == needle)
            .unwrap_or_else(|| panic!("missing {needle:?} in {texts:?}"))
    };
    let order = [
        at("UP NEXT"),
        at("I can pay your TXU bill before Friday"),
        at("I can draft your standup"),
        at("I can plan a dinner with family"),
        at("PRODUCTIVITY"),
        at("I can summarize the workboard automatically"),
        at("SHOPPING"),
        at("HEALTH & FITNESS"),
        at("MORE IDEAS"),
        at("I can sort the vacation photos"),
    ];
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "{order:?} in {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "FINANCIAL MANAGEMENT"),
        "the bill sits in the top group: {texts:?}"
    );
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

    // Dismiss: the card is removed and the ledger says so.
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
    cabin.pulse_view.tab = super::pulse_ui::PulseTab::Feed;
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
    let tex = cabin.pulse_view_texture_size("https://www.rust-lang.org/static/og.png");
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
    cabin.pulse_view.tab = super::pulse_ui::PulseTab::Feed;
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
fn pulse_opens_on_feed_and_the_tabs_read_feed_then_ideas() {
    assert_eq!(
        super::pulse_ui::PulseView::default().tab,
        super::pulse_ui::PulseTab::Feed
    );
    let mut quiet = QuietCabin::boot("pulse-tabs");
    let cabin = &mut quiet.cabin;
    assert_eq!(cabin.pulse_view.tab, super::pulse_ui::PulseTab::Feed);
    assert_eq!(
        super::pulse_ui::PulseTab::ORDER.map(|t| t.label()),
        ["Feed", "Ideas"]
    );
    let x = |texts: &[(String, f32)], needle: &str| {
        texts
            .iter()
            .find(|(t, _)| t == needle)
            .map(|(_, x)| *x)
            .unwrap_or_else(|| panic!("missing {needle:?} in {texts:?}"))
    };
    let feed = pulse_text_at_w(cabin, COMPACT_W, 900.0, Vec::new());
    let (feed_x, ideas_x) = (x(&feed, "Feed"), x(&feed, "Ideas"));
    assert!(feed_x < ideas_x, "Feed sits left of Ideas: {feed:?}");
    assert!(feed.iter().any(|(t, _)| t == "Feed instructions"));
    // The switch stays put when the tab's action changes (one fixed slot).
    cabin.pulse_view.tab = super::pulse_ui::PulseTab::Ideas;
    let ideas = pulse_text_at_w(cabin, COMPACT_W, 900.0, Vec::new());
    assert_eq!(
        (x(&ideas, "Feed"), x(&ideas, "Ideas")),
        (feed_x, ideas_x),
        "the Feed | Ideas switch does not move between tabs"
    );
    assert!(ideas.iter().any(|(t, _)| t == "Suggest ideas"));
    assert!(!ideas.iter().any(|(t, _)| t == "Feed instructions"));
    assert_eq!(super::pulse_ui::HEADER_SLOT_W, 150.0);
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
    cabin.pulse_view.tab = super::pulse_ui::PulseTab::Ideas;
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
    cabin.pulse_view.tab = super::pulse_ui::PulseTab::Ideas;
    let empty = pulse_texts_w(cabin, COMPACT_W, 900.0);
    assert!(
        empty
            .iter()
            .any(|t| t == "No ideas yet. I'll add one when I spot something worth doing."),
        "{empty:?}"
    );
    // Empty Ideas keeps one inline Suggest ideas. The header hides its duplicate.
    let suggest_n = empty
        .iter()
        .filter(|t| t.as_str() == "Suggest ideas")
        .count();
    assert_eq!(
        suggest_n, 1,
        "expected one Suggest ideas, got {suggest_n}: {empty:?}"
    );
    assert!(!empty.iter().any(|t| t.contains("Thinking")), "{empty:?}");
    cabin.suggest_ideas_with(false);
    assert!(cabin.pulse_view.signin_note);
    assert_eq!(cabin.status, "Connect Grok in Settings to get ideas");
    let signed_out = pulse_texts_w(cabin, COMPACT_W, 900.0);
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
    let _ = pulse_texts_w(cabin, COMPACT_W, 900.0);
    assert!(!cabin.pulse_view.signin_note);
    // While a suggestion call runs: placeholder rows, and no empty line beside them.
    let (_tx, rx) = mpsc::channel::<String>();
    cabin.ideas_rx = Some((rx, Default::default()));
    let loading = pulse_texts_w(cabin, COMPACT_W, 900.0);
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
            .any(|t| t.starts_with("No ideas yet") || t.ends_with("Nothing to do.")),
        "{loading:?}"
    );
    cabin.ideas_rx = None;

    // Feed empty: lead + clickable "Feed instructions" + silence line.
    cabin.pulse_view.tab = super::pulse_ui::PulseTab::Feed;
    let feed = pulse_texts_w(cabin, COMPACT_W, 900.0);
    assert!(
        feed.iter()
            .any(|t| t == "No posts yet. Tell me what to watch in "),
        "{feed:?}"
    );
    assert!(feed.iter().any(|t| t == "Feed instructions"), "{feed:?}");
    assert!(
        !feed
            .iter()
            .any(|t| t == "No posts yet. Tell me what to watch in Feed instructions."),
        "empty Feed paints the link separately: {feed:?}"
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
fn pulse_layout_reflows_like_the_deck_breakpoints() {
    use super::pulse_ui::{post_thumb_count, pulse_layout, PulseFit};
    let near = |a: f32, b: f32| (a - b).abs() < 0.05;
    // Ultra-wide: Ideas 3fr, Feed 2fr, both tile 2-up.
    let wide = pulse_layout(2440.0);
    assert_eq!(wide.fit, PulseFit::Split);
    assert!(near(wide.gap, 30.0) && near(wide.ideas_w, 1446.0) && near(wide.feed_w, 964.0));
    assert_eq!((wide.ideas_cols, wide.feed_cols), (2, 2));
    // The 60/40 split starts at 1500.
    let edge = pulse_layout(1500.0);
    assert!(near(edge.gap, 19.5) && near(edge.ideas_w, 888.3) && near(edge.feed_w, 592.2));
    assert_eq!((edge.ideas_cols, edge.feed_cols), (1, 1));
    // Laptop: 1.35fr to 1fr, one card across.
    let laptop = pulse_layout(1300.0);
    assert_eq!(laptop.fit, PulseFit::Split);
    assert!(near(laptop.ideas_w, 737.1) && near(laptop.feed_w, 546.0));
    assert_eq!((laptop.ideas_cols, laptop.feed_cols), (1, 1));
    // The split starts at 1100 with the 16 px minimum gap.
    let tight = pulse_layout(1100.0);
    assert_eq!(tight.fit, PulseFit::Split);
    assert!(near(tight.gap, 16.0) && near(tight.feed_w, 461.3) && near(tight.ideas_w, 622.7));
    assert_eq!(pulse_layout(1099.0).fit, PulseFit::Stacked);
    // Small window: one column, the feed 2-up underneath.
    let small = pulse_layout(900.0);
    assert_eq!(small.fit, PulseFit::Stacked);
    assert_eq!((small.ideas_w, small.feed_cols), (900.0, 2));
    assert_eq!(pulse_layout(720.0).fit, PulseFit::Stacked);
    // Below 720: the switch, one centered column up to 680.
    let phone = pulse_layout(719.0);
    assert_eq!(phone.fit, PulseFit::Compact);
    assert_eq!(phone.ideas_w, 680.0);
    assert_eq!(pulse_layout(552.0).ideas_w, 552.0);
    // Posts under 560 show two source images, not three.
    assert_eq!(post_thumb_count(559.0), 2);
    assert_eq!(post_thumb_count(560.0), 3);
}

#[test]
fn the_deck_frames_land_on_their_layouts_beside_the_rail() {
    use super::pulse_ui::{post_thumb_count, pulse_page_layout, PulseFit};
    let rail = crate::theme::SIDEBAR_W;
    // 2560: the 60/40 split, 24 px padding each side, both columns 2-up.
    let wide = pulse_page_layout(2560.0 - rail);
    assert_eq!(wide.fit, PulseFit::Split);
    assert_eq!((wide.ideas_cols, wide.feed_cols), (2, 2));
    assert!((wide.ideas_w + wide.gap + wide.feed_w - 2472.0).abs() < 0.05);
    // 1440: the laptop split. The Feed column is 568 px, over the 560 px
    // line, so posts keep all three images.
    let laptop = pulse_page_layout(1440.0 - rail);
    assert_eq!(laptop.fit, PulseFit::Split);
    assert!((laptop.feed_w - 567.8).abs() < 0.1, "{}", laptop.feed_w);
    assert_eq!(post_thumb_count(laptop.feed_w), 3);
    // 1024: one column with the feed 2-up below, 936 px inside the padding.
    let tablet = pulse_page_layout(1024.0 - rail);
    assert_eq!(
        (tablet.fit, tablet.ideas_w, tablet.feed_cols),
        (PulseFit::Stacked, 936.0, 2)
    );
    // The narrowest window (720) leaves a 680 px page: the switch.
    let narrow = pulse_page_layout(720.0 - rail);
    assert_eq!((narrow.fit, narrow.ideas_w), (PulseFit::Compact, 632.0));
}

#[test]
fn suggest_goes_short_on_the_one_column_page() {
    use super::pulse_ui::{suggest_label, PulseFit};
    assert_eq!(suggest_label(false, PulseFit::Split), "Suggest ideas");
    assert_eq!(suggest_label(false, PulseFit::Stacked), "Suggest");
    assert_eq!(suggest_label(false, PulseFit::Compact), "Suggest ideas");
    assert_eq!(suggest_label(true, PulseFit::Stacked), "Suggesting…");
}

/// Deck-match 6: Pulse header/cards follow slides 13 and 15 chrome.
#[test]
fn pulse_deck_match_6_chrome_tokens() {
    let _paint = crate::theme::hold_paint_test();
    crate::theme::set_paint_palette(crate::theme::PaintPalette::OpsDark);
    let pulse = include_str!("pulse_ui.rs");
    let ideas = include_str!("ideas_ui.rs");
    assert!(
        pulse.contains("fn pulse_header_tab(") && !pulse.contains("felt_segment(ui, tab.label()"),
        "Feed|Ideas must use outlined recipe tabs, not felt_segment pills"
    );
    assert!(
        pulse.contains("recipe::STROKE_W") && pulse.contains("signal_red()"),
        "idea icon tiles must use the signal-red recipe rim"
    );
    assert!(
        ideas.contains("paint_group_diamond") && ideas.contains("theme::amber()"),
        "Up next / category headers keep amber diamonds"
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
fn pulse_shows_ideas_beside_or_above_the_feed_and_compact_rows_keep_run() {
    let mut quiet = QuietCabin::boot("pulse-fluid");
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

    // Laptop window: Ideas on the left, the feed column on the right, and
    // each column keeps its own action. Rows rest without inline actions.
    let split = pulse_text_pos(cabin, 1400.0);
    assert_eq!(cabin.pulse_view.fit, super::pulse_ui::PulseFit::Split);
    let (ideas, feed) = (at(&split, "UP NEXT"), at(&split, "FROM YOUR FEED"));
    assert!(
        ideas.x + 500.0 < feed.x,
        "feed sits right of ideas: {split:?}"
    );
    assert_eq!(count(&split, "Suggest ideas"), 1, "{split:?}");
    assert_eq!(
        count(&split, "Feed instructions"),
        2,
        "pill and empty-feed link: {split:?}"
    );
    assert_eq!((count(&split, "Run"), count(&split, "Snooze")), (0, 0));
    assert_eq!(count(&split, "···"), 2);

    // Small window: one column, the feed below the ideas.
    let stacked = pulse_text_pos(cabin, 900.0);
    assert_eq!(cabin.pulse_view.fit, super::pulse_ui::PulseFit::Stacked);
    let (ideas, feed) = (at(&stacked, "UP NEXT"), at(&stacked, "FROM YOUR FEED"));
    assert!(ideas.y < feed.y && feed.x < ideas.x, "{stacked:?}");
    assert!(at(&stacked, "I can sort the beach photos").y < feed.y);

    // Compact: one view behind the switch. Run stays on every row; Snooze
    // and Dismiss wait in ···.
    cabin.pulse_view.tab = super::pulse_ui::PulseTab::Ideas;
    let compact = pulse_text_pos(cabin, COMPACT_W);
    assert_eq!(cabin.pulse_view.fit, super::pulse_ui::PulseFit::Compact);
    assert_eq!(count(&compact, "FROM YOUR FEED"), 0, "{compact:?}");
    assert_eq!(
        (
            count(&compact, "Run"),
            count(&compact, "Snooze"),
            count(&compact, "Dismiss"),
            count(&compact, "···")
        ),
        (2, 0, 0, 2),
        "{compact:?}"
    );
    cabin.pulse_view.tab = super::pulse_ui::PulseTab::Feed;
    let compact_feed = pulse_text_pos(cabin, COMPACT_W);
    assert_eq!(count(&compact_feed, "UP NEXT"), 0, "{compact_feed:?}");
    assert_eq!(
        count(&compact_feed, "Feed instructions"),
        2,
        "{compact_feed:?}"
    );
}
