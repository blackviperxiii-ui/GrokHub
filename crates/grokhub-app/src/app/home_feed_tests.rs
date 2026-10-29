//! Home, feed and update cards tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

#[test]
fn empty_home_paints_faint_greeting() {
    let src = cabin_src();
    let slice = src
        .split("fn ui_empty_home")
        .nth(1)
        .and_then(|s| s.split("fn ui_composer_stack(").next())
        .expect("empty home");
    assert!(
        slice.contains("self.greeting"),
        "new chats paint a greeting blurb: {slice}"
    );
    assert!(
        slice.contains("GREET_HERO") && slice.contains("title_font"),
        "greeting is the empty-home hero, not a 56px wordmark: {slice}"
    );
    assert!(
        slice.contains("theme::mark"),
        "empty home paints a quiet Grok mark above the greeting: {slice}"
    );
    assert!(
        !slice.contains("italics"),
        "greeting is regular/medium weight: {slice}"
    );
    assert!(
        slice.contains("paint_approval_stack"),
        "empty home must still show a live permission bar: {slice}"
    );
    assert!(
            slice.contains("pulse_should_paint")
                && slice.contains("empty_home_composer_top")
                && slice.contains("paint_device_glance_row")
                && !slice.contains("paint_update_feed")
                && !slice.contains("paint_home_deck_over_chat")
                && !slice.contains("home_feed_count")
                && !slice.contains("collapsed_stack_h")
                && !slice.contains("paint_empty_pulse")
                && !slice.contains("paint_lane_chip")
                && !slice.contains("No updates"),
            "signed-in empty home is chat-first: greeting, composer, device glance; no Home card deck: {slice}"
        );
    assert!(
        !slice.contains("weather")
            && !slice.contains("Outlook")
            && !slice.contains("Gmail")
            && !slice.contains("calendar"),
        "empty-home pulse must not invent mail or weather: {slice}"
    );
    let greet = slice.find("self.greeting").expect("greeting");
    let composer = slice.find("ui_composer_stack").expect("composer");
    assert!(greet < composer, "greeting sits above the chat box");
    let greet_label = &slice[greet..composer];
    assert!(
        greet_label.contains(".color(crate::theme::fg())") && !greet_label.contains("muted()"),
        "the empty-chat headline uses full-contrast ink, not the dim muted tone: {greet_label}"
    );
    assert!(
        !slice.contains("Native Grok Build cabin"),
        "empty home is the greeting, not a product tagline: {slice}"
    );
    assert!(
        !slice.contains("RichText::new(\"GrokHub\")"),
        "empty home must not paint a GrokHub wordmark: {slice}"
    );
    assert!(
        !slice.contains("device_name"),
        "empty home must not paint the hostname: {slice}"
    );
    assert!(
        slice.contains("empty_home_composer_top") && slice.contains("empty_home_greet_top"),
        "greeting sits in the title-to-composer gap; the chat box stays on the midline: {slice}"
    );
    assert!(
        slice.contains("greeting_galley_h") || slice.contains("fonts(|"),
        "wrapped greeting height must drive vertical placement: {slice}"
    );
    assert!(
        !slice.contains("* 0.38"),
        "empty-home composer sits in the vertical center, not the upper third: {slice}"
    );
    assert!(
        slice.contains("12.0") && !slice.contains("add_space(28.0)"),
        "greeting-to-composer gap stays tight: {slice}"
    );
    let chips = src.find("ComposerStackSlot::Chips =>").expect("chips arm");
    let chips = &src[chips..chips + 700];
    assert!(
        chips.contains("add_space(6.0)"),
        "chips sit a tight gap under the pill: {chips}"
    );
    assert!(
        chips.contains("composer_chips()") && !chips.contains("messages.is_empty()"),
        "suggestion chips stay up mid-thread, not only on an empty chat: {chips}"
    );
    assert_eq!(super::empty_home_side_gap(1800.0, 800.0), 500.0);
    assert_eq!(super::empty_home_side_gap(700.0, 800.0), 0.0);
    assert_eq!(super::empty_home_composer_top(800.0, 60.0), 370.0);
    assert_eq!(super::empty_home_greet_top(370.0, 40.0, 12.0), 159.0);
    let short = super::empty_home_greet_top(370.0, 40.0, 12.0);
    let wrapped = super::empty_home_greet_top(370.0, 100.0, 12.0);
    assert!(
        wrapped < short,
        "a wrapped greeting rises so it stays centered in the gap: {wrapped} vs {short}"
    );
    assert!(
        (wrapped + 50.0 - (370.0 - 12.0) * 0.5).abs() < 0.5,
        "wrapped greeting midpoint is the title-to-composer midpoint: {wrapped}"
    );
    assert_eq!(super::empty_home_greet_top(80.0, 90.0, 12.0), 0.0);
    assert!(
        slice.contains("empty_home_side_gap"),
        "empty-home column must be centered in leftover width, not left-packed: {slice}"
    );
}

#[test]
fn empty_home_pulse_stays_off_scratch_and_off_about() {
    let src = cabin_src();
    let home = src
        .split("fn ui_empty_home")
        .nth(1)
        .and_then(|s| s.split("fn ui_composer_stack(").next())
        .expect("empty home");
    assert!(
        home.contains("pulse_should_paint") && !home.contains("vertical_centered_justified"),
        "the home slot must not re-center the chip row: {home}"
    );
    assert!(
        !home.contains("grok_tile("),
        "full grok_tiles shove the composer off a short cabin: {home}"
    );
    let about = src.split("SettingsSec::About").nth(1).unwrap_or("");
    assert!(
        !about.contains("usage_line") && !about.contains("paint_empty_pulse"),
        "About must not grow today's buckets: {about}"
    );
    assert!(
        src.contains("Nav::Workboard") && src.contains("Nav::Night"),
        "pulse clicks reuse Workboard and Automations"
    );
    assert!(
        home.contains("paint_device_glance_row")
            && home.contains("pulse_should_paint")
            && !home.contains("paint_update_feed")
            && !home.contains("paint_home_deck_over_chat")
            && !home.contains("collapsed_stack_h")
            && !home.contains("home_feed_count")
            && !home.contains("stacked_feed_h")
            && !home.contains("paint_lane_chip")
            && !home.contains("No updates"),
        "empty home keeps device glance and does not paint the Home card deck: {home}"
    );
    assert!(
        !home.contains("\"Personal\"") && !home.contains("Nav::"),
        "home chrome must not add a Personal rail or nav id: {home}"
    );
    let feed = include_str!("feed_ui.rs");
    assert!(
        feed.contains("note_automation_done")
            && feed.contains("note_schedule_created")
            && feed.contains("close_home_card")
            && feed.contains("apply_feed_act")
            && !feed.contains("paint_update_feed")
            && !feed.contains("paint_home_deck_over_chat")
            && !feed.contains("HOME_STACK_SHOW")
            && !feed.contains("Order::Foreground")
            && !feed.contains("No updates")
            && !feed.contains("interest_update"),
        "feed posting and card actions stay; Home deck paint is gone: {feed}"
    );
    let night = include_str!("night.rs");
    let poll = night
        .split("fn poll_grok_loop(")
        .nth(1)
        .and_then(|s| s.split("fn fire_loop(").next())
        .expect("poll_grok_loop");
    assert!(
        poll.contains("note_automation_done"),
        "a finished /loop posts automation_done: {poll}"
    );
    let commit = night
        .split("fn commit_schedule(")
        .nth(1)
        .and_then(|s| s.split("fn ui_night(").next())
        .expect("commit_schedule");
    assert!(
        commit.contains("note_schedule_created"),
        "saving a schedule posts schedule_created: {commit}"
    );
    let title = src
        .split("fn ui_titlebar(")
        .nth(1)
        .and_then(|s| s.split("fn ui_sidebar(").next())
        .expect("titlebar");
    assert!(
        title.contains("quiet_until_chip") && title.contains("SettingsSec::Behavior"),
        "quiet-hours chip is titlebar chrome; Behavior stays SoT: {title}"
    );
    let hist = src
        .split("fn ui_history(")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("history");
    assert!(
        hist.contains("session_markers")
            && hist.contains("jump_last_you")
            && hist.contains("apply_switch_thread")
            && !hist.contains("self.thread_idx = i"),
        "History map must swap the visible thread, not only the index: {hist}"
    );
    let reserved = src
        .split("if reserve_offscreen_chat_row(ui, cached_h)")
        .nth(1)
        .and_then(|s| s.split("let y0 = ui.cursor().min.y").next())
        .expect("reserved last-you");
    assert!(
        reserved.contains("scroll_to_rect")
            && reserved.contains("jump_you")
            && reserved.contains("last_you_i"),
        "Last you must scroll a reserved off-screen row: {reserved}"
    );
    let auto = src
        .split("Slash::AutoPerm =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Effort =>").next())
        .expect("AutoPerm");
    assert!(
        auto.contains("self.confirm = None"),
        "/auto must drop a session Always overlay: {auto}"
    );
    // rustfmt may break `let row = crate::cards::session_row(...)` across lines.
    let row = src
        .split("crate::cards::session_row(")
        .nth(1)
        .and_then(|s| s.split("ui.allocate_ui_with_layout").next())
        .expect("session_row");
    let perm = row
        .split("if let Some(perm) = row.perm")
        .nth(1)
        .and_then(|s| s.split("if row.why").next())
        .expect("perm pills");
    let ask_auto = perm
        .split("self.arm_session_always();")
        .nth(1)
        .expect("Ask/Auto after Always");
    assert!(
        ask_auto.contains("self.confirm = None"),
        "Auto/Ask must disarm the session Always overlay: {ask_auto}"
    );
}

#[test]
fn feed_pulse_and_fresh_home_stay_off_the_review() {
    let src = cabin_src();
    let house = src
        .split("HeartbeatAct::Housekeep =>")
        .nth(1)
        .and_then(|s| s.split("HeartbeatAct::Inbox =>").next())
        .expect("housekeep");
    assert!(!house.contains("stamp_current_access"), "{house}");
    let pulse = src
        .split("JobKind::PulseRefresh =>")
        .nth(1)
        .and_then(|s| s.split("JobKind::ModelRegistry =>").next())
        .expect("pulse refresh job");
    assert!(
        pulse.contains("tick_feed_pulse") && !pulse.contains("stamp_current_access"),
        "expiry, quiet release, and the digest clock run on the Pulse refresh job without moving History: {pulse}"
    );
    let tick = src
        .split("fn tick_review(")
        .nth(1)
        .and_then(|s| s.split("fn review_digest(").next())
        .expect("tick_review");
    assert!(
        !tick.contains("tick_feed_pulse") && !tick.contains("expire_ideas"),
        "the feed pulse must not run from the nightly review: {tick}"
    );
    let spawn = src
        .split("fn spawn_review(")
        .nth(1)
        .and_then(|s| s.split("fn poll_review(").next())
        .expect("spawn_review");
    assert!(
        !spawn.contains("tick_feed_pulse")
            && !spawn.contains("expire_ideas")
            && !spawn.contains("suggestions_from_sessions"),
        "spawn_review stays the transcript review: {spawn}"
    );
    let show = src
        .split("fn show_from_tray(")
        .nth(1)
        .and_then(|s| s.split("fn note_window_resume(").next())
        .expect("show_from_tray");
    assert!(
        show.contains("open_fresh_home") && show.contains("apply_saved_geom"),
        "tray show opens a fresh chat and restores the window: {show}"
    );
    let fresh = fn_src(&src, "open_fresh_home");
    assert!(
        fresh.contains("new_thread(false)")
            && fresh.contains("park_fresh_chat")
            && fresh.contains("resume_needs_fresh_chat")
            && !fresh.contains("halt_in_flight")
            && !fresh.contains("drop_leaving_thread_chrome")
            && !fresh.contains("delete_thread")
            && !fresh.contains("pinned = false")
            && !fresh.contains("No updates")
            && !fresh.contains("Nothing queued"),
        "fresh home keeps the previous chat and does not stop a live reply: {fresh}"
    );
    let park = fn_src(&src, "park_fresh_chat");
    assert!(
        park.contains("self.messages.clone()")
            && !park.contains("halt_in_flight")
            && !park.contains("drop_leaving_thread_chrome")
            && !park.contains("delete_thread")
            && !park.contains("pinned = false"),
        "parking a fresh chat leaves the running thread alone: {park}"
    );
    let pulse = include_str!("feed_ui.rs");
    let body = pulse
        .split("fn tick_feed_pulse(")
        .nth(1)
        .and_then(|s| s.split("fn digest_taste(").next())
        .expect("tick_feed_pulse");
    assert!(
        body.contains("read_memory(\"USER.md\")")
            && body.contains("read_memory(\"MEMORY.md\")")
            && body.contains("read_memory(\"SOUL.md\")")
            && !body.contains("last_review_day")
            && !body.contains("suggestions_from_sessions")
            && !body.contains("spawn_review")
            && !body.contains("notify::")
            && !body.contains("send_chat")
            && !body.contains("send_scheduled")
            && !body.contains("grok_chat"),
        "digest authoring stays off transcripts, chat, and pings: {body}"
    );
    let ping = pulse
        .split("fn release_situation_ping(")
        .nth(1)
        .and_then(|s| s.split("fn apply_feed_act(").next())
        .expect("release_situation_ping");
    assert!(
        ping.contains("notify::ping")
            && ping.contains("quiet_now")
            && ping.contains("window_focused")
            && ping.contains("brand::product_name()"),
        "only the situation card pings, and only when the window is away and quiet hours are off: {ping}"
    );
    assert!(
        !pulse.contains("No updates") && !pulse.contains("Nothing queued"),
        "the feed must not paint an empty label"
    );
    let ideas = include_str!("ideas_ui.rs")
        .split("fn ui_ideas(")
        .nth(1)
        .and_then(|s| s.split("fn paint_idea_open(").next())
        .expect("ui_ideas");
    assert!(
        ideas.contains("Suggest for you") && ideas.contains("maybe_suggest_ideas(true)"),
        "the For You button asks the model for ideas now, not a disk reload: {ideas}"
    );
    let offer = pulse
        .split("fn accept_automate_offer(")
        .nth(1)
        .and_then(|s| s.split("fn ensure_useful_ideas(").next())
        .expect("accept_automate_offer");
    let seed = pulse
        .split("fn offer_seed(")
        .nth(1)
        .and_then(|s| s.split("fn open_offer_on_automations(").next())
        .expect("offer_seed");
    assert!(
        offer.contains("self.offer_seed(id)")
            && offer.contains("route_schedule")
            && !offer.contains("c.body")
            && seed.contains("automate_accept_seed(c)")
            && seed.contains("c.title.clone()")
            && !seed.contains("c.body"),
        "Accept seeds the schedule from the preview, else the title, never the canned offer body: {offer} {seed}"
    );
    let build = pulse
        .split("fn build_idea(")
        .nth(1)
        .and_then(|s| s.split("fn accept_automate_offer(").next())
        .expect("build_idea");
    assert!(
        build.contains("idea_todo_title(")
            && build.contains("file_idea_todo(&mut self.board, &task, \"\")")
            && !build.contains("idea.body.clone()"),
        "Idea Accept files the task line, not the raw message: {build}"
    );
    assert!(
        pulse.contains("FeedAct::Discuss")
            && pulse.contains("FeedAct::Dismiss")
            && pulse.contains("FeedAct::Open")
            && !pulse.contains("FeedAct::Archive")
            && pulse.contains("fn close_home_card(")
            && !pulse.contains("fn paint_feed_card(")
            && !pulse.contains("fn paint_update_feed("),
        "feed card acts and close_home_card remain; Home deck paint is gone: {pulse}"
    );
    let offer_open = pulse
        .split("fn open_offer_on_automations(")
        .nth(1)
        .and_then(|s| s.split("fn accept_automate_offer(").next())
        .expect("open_offer_on_automations");
    assert!(
        offer_open.contains("self.night_nl = seed")
            && offer_open.contains("self.auto_compose = true")
            && offer_open.contains("Nav::Night")
            && !offer_open.contains("commit_schedule")
            && !offer_open.contains("route_schedule"),
        "an offer click opens the schedule box and does not save it: {offer_open}"
    );
}

#[test]
fn feed_accept_files_one_todo() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("idea-accept");
    let card = grokhub_core::idea_card("src", "More F1", "from the brief", 1);
    let id = card.id.clone();
    cabin.updates = vec![card];

    cabin.build_idea(&id);

    assert_eq!(cabin.board.len(), 1);
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert_eq!(cabin.board[0].title, "Cover F1");
    release_isolated(&root, cabin);
}

#[test]
fn discuss_card_opens_one_local_chat() {
    let mut quiet = QuietCabin::boot("discuss");
    let root = quiet.boot.root.clone();
    let cabin = &mut quiet.cabin;
    cabin.updates.push(grokhub_core::UpdateCard {
        id: "idea-harbor".into(),
        kind: grokhub_core::UpdateKind::Idea,
        title: "Cover F1".into(),
        body: Some("the night race".into()),
        created_at: 1,
        status: grokhub_core::UpdateStatus::Unread,
        action: None,
        expires_at: None,
        held: false,
        citations: Vec::new(),
        reaction: None,
        discuss_thread: None,
        built: false,
        board_id: None,
        why: Some("because the tide turned".into()),
        feed_pin: false,
        feed_kept: false,
        prompt: None,
        idea_kind: None,
        details: None,
        draft: None,
        modified: false,
        source_id: String::new(),
        skill: None,
        runs: 1,
        dismissed_at: 0,
        done_for_you: None,
        pulse: Default::default(),
    });
    cabin.discuss_card("idea-harbor");
    let open_id = cabin
        .threads
        .iter()
        .find(|t| t.title == "Discuss · Cover F1")
        .expect("idea pop thread")
        .id
        .clone();
    let open = cabin.threads.iter().find(|t| t.id == open_id).unwrap();
    assert!(open.background);
    assert_ne!(cabin.threads[cabin.thread_idx].id, open_id);
    assert!(cabin.idea_board.open.is_some() && cabin.nav == super::Nav::Pulse);
    assert_eq!(
        cabin
            .threads
            .iter()
            .filter(|t| t.title == "Discuss · Cover F1")
            .count(),
        1
    );
    assert!(
        crate::threads::chat_section_indices(&cabin.threads, None, false)
            .iter()
            .all(|&i| cabin.threads[i].id != open_id)
    );
    let body = cabin
        .threads
        .iter()
        .find(|t| t.id == open_id)
        .unwrap()
        .messages
        .iter()
        .find(|m| m.0 == "assistant")
        .map(|m| m.1.clone())
        .unwrap_or_default();
    assert!(
        body.contains("Nothing runs until you press Apply"),
        "the card chat opens with a short line; the card itself shows the details, got {body}"
    );
    assert_eq!(cabin.updates[0].status, grokhub_core::UpdateStatus::Opened);
    assert!(cabin.updates[0].discuss_thread.is_some());
    assert!(!cabin.running);
    wait_for("the discuss card must be saved opened", || {
        std::fs::read_to_string(root.join("updates.json"))
            .unwrap_or_default()
            .contains("idea-harbor")
    });
    quiet.settle();
}

#[test]
fn home_x_on_a_digest_keeps_it_in_pulse() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("home-x-digest");
    let mut cabin = Cabin::quiet_for_test();
    let card = grokhub_core::digest_card("week", "Week notes", "rolled up", 2);
    let id = card.id.clone();
    cabin.updates.push(card);
    let on_home = |cabin: &Cabin| {
        cabin
            .updates
            .iter()
            .any(|c| c.id == id && !c.pulse.off_home)
    };
    assert!(on_home(&cabin), "the digest starts visible for home");
    cabin.apply_feed_act(Some(FeedAct::Dismiss(id.clone())));
    wait_tree_contains(&cfg.root, "offHome");
    assert!(!on_home(&cabin), "× sets off_home");
    assert!(
        grokhub_core::visible_digests(&cabin.updates)
            .iter()
            .any(|c| c.id == id),
        "Pulse still lists the digest"
    );
    assert!(grokhub_core::archived_digests(&cabin.updates).is_empty());
    let kept = cabin
        .updates
        .iter()
        .find(|c| c.id == id)
        .expect("digest kept");
    assert_eq!(kept.status, grokhub_core::UpdateStatus::Unread);
    assert!(kept.pulse.off_home);
    drop(cfg);
}

// Landed from PR #133.
#[test]
fn discuss_card_opens_a_local_chat() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("discuss-card");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.discuss_card("missing");
    assert!(cabin.threads.is_empty());
    assert!(!cabin.running);
    let card = grokhub_core::idea_card("harbor", "Harbor lamp", "fold the charts", 1);
    let id = card.id.clone();
    cabin.updates.push(card);
    cabin.discuss_card(&id);
    assert!(cabin.idea_board.open.is_some() && cabin.nav == super::Nav::Pulse);
    assert!(!cabin.running);
    let thread_id = cabin
        .threads
        .iter()
        .find(|t| t.title == "Discuss · Harbor lamp")
        .expect("discuss thread")
        .id
        .clone();
    {
        let thread = cabin.threads.iter().find(|t| t.id == thread_id).unwrap();
        assert!(thread.background);
        assert!(thread.title_locked);
        assert!(thread
            .messages
            .iter()
            .any(|(_, body)| body.contains("Nothing runs until you press Apply")));
    }
    assert_ne!(cabin.threads[cabin.thread_idx].id, thread_id);
    let stuck = cabin.updates.iter().find(|c| c.id == id).expect("card");
    assert_eq!(stuck.status, grokhub_core::UpdateStatus::Opened);
    assert!(stuck.discuss_thread.is_some());
}

// Landed from PR #140.
#[test]
fn feed_open_routes_an_idea_and_dismiss_removes_it() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("feed-open");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.open_feed_card("missing");
    assert!(matches!(cabin.nav, Nav::Chat));
    let card = grokhub_core::idea_card("harbor", "Harbor lamp", "fold the charts", 1);
    let id = card.id.clone();
    cabin.updates.push(card);
    cabin.updates.last_mut().unwrap().feed_pin = true;
    cabin.open_feed_card(&id);
    assert!(cabin.idea_board.open.is_some() && cabin.nav == super::Nav::Pulse);
    assert!(cabin
        .threads
        .iter()
        .any(|t| t.background && t.title == "Discuss · Harbor lamp"));
    assert_ne!(
        cabin.threads[cabin.thread_idx].title,
        "Discuss · Harbor lamp"
    );
    cabin.open_feed_card(&id);
    assert_eq!(
        cabin
            .threads
            .iter()
            .filter(|t| t.title == "Discuss · Harbor lamp")
            .count(),
        1
    );
    cabin.dismiss_feed_card(&id);
    let kept = cabin
        .updates
        .iter()
        .find(|c| c.id == id)
        .expect("stays on the board");
    assert!(!kept.feed_pin && kept.feed_kept);
    assert!(!cabin.running);
}

// Landed from PR #157.
#[test]
fn feed_pulse_ticks_without_a_chat() {
    let _cfg = crate::config::hold_test_config();
    let prev_config = std::env::var("GROKHUB_CONFIG").ok();
    let root = crate::config::test_config_root("feed-pulse");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    // Whole-day quiet so Housekeep holds the digest clock and does not post a card.
    cabin.cfg.quiet_start = "00:00".into();
    cabin.cfg.quiet_end = "23:59".into();
    let before = cabin.cfg.feed_pulse.clone();
    assert!(!cabin.running);
    assert!(
        cabin.updates.is_empty() && !grokhub_core::feed_visible(&cabin.updates),
        "an empty feed stays hidden"
    );

    cabin.tick_feed_pulse();
    assert_ne!(
        cabin.cfg.feed_pulse, before,
        "one tick advances the stored FeedPulse"
    );
    assert!(!cabin.running, "the feed pulse must not start a run");
    assert!(
        cabin.updates.is_empty() && !grokhub_core::feed_visible(&cabin.updates),
        "an empty feed stays hidden"
    );

    let advanced = cabin.cfg.feed_pulse.clone();
    cabin.tick_feed_pulse();
    assert!(!cabin.running, "a second tick does not start a run");
    assert_eq!(
        cabin.cfg.feed_pulse, advanced,
        "a second tick does not start a run"
    );
    assert!(
        cabin.updates.is_empty() && !grokhub_core::feed_visible(&cabin.updates),
        "an empty feed stays hidden"
    );

    let io = cabin.persist_io.clone();
    let path = root.join("app.json");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if std::fs::read_to_string(&path)
            .map(|body| body.contains("feedPulse"))
            .unwrap_or(false)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _disk = std::mem::ManuallyDrop::new(io.lock().unwrap_or_else(|e| e.into_inner()));
    match prev_config {
        Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
        None => std::env::remove_var("GROKHUB_CONFIG"),
    }
}

// Landed from PR #159.
#[test]
fn housekeep_keeps_ideas_until_newer_ones_push_them_out() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("idea-expiry");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    // Heartbeat must not ask the real model from a test.
    cabin.cfg.feed_pulse.last_ideas_ms = grokhub_core::now_ms();
    assert!(!cabin.running);
    assert!(
        !grokhub_core::feed_visible(&cabin.updates),
        "an empty feed stays hidden"
    );

    let now = grokhub_core::now_ms();
    let stale_at = now
        .saturating_sub(grokhub_core::IDEA_TTL_MS)
        .saturating_sub(1);
    let stale = grokhub_core::idea_card("old", "Stale idea", "aged out", stale_at);
    let fresh = grokhub_core::idea_card("new", "Fresh idea", "still good", now);
    assert!(!stale.kind.event() && !fresh.kind.event());
    crate::feed::save(&[stale.clone(), fresh.clone()]).expect("updates.json");
    cabin.updates = crate::feed::load();

    cabin.tick_feed_pulse();
    assert!(
        cabin.ideas_rx.is_none(),
        "housekeep must not start the ideas model call"
    );

    assert!(
        cabin
            .updates
            .iter()
            .all(|card| card.status != grokhub_core::UpdateStatus::Dismissed),
        "housekeep expiry is the age path, not a dismiss"
    );
    assert!(
        cabin.updates.iter().any(|card| card.id == stale.id),
        "ideas no longer time out; a newer card pushes out the oldest instead"
    );
    assert!(
        cabin.updates.iter().any(|card| card.id == fresh.id),
        "a fresh idea stays"
    );
    assert!(!cabin.running);
    assert!(
        grokhub_core::visible_updates(&cabin.updates).is_empty(),
        "ideas must not invent event rows or take the paint cap of 3"
    );
    assert_eq!(
        grokhub_core::visible_ideas(&cabin.updates)
            .iter()
            .filter(|card| card.id == fresh.id)
            .count(),
        1
    );

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #166.
#[test]
fn heartbeat_pulse_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("heartbeat_pulse_stays_off_a_run");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    // Heartbeat must not ask the real model from a test.
    cabin.cfg.feed_pulse.last_ideas_ms = grokhub_core::now_ms();
    cabin.hub_on = false;
    cabin.automations.clear();
    cabin.running = false;
    cabin.last_activity = std::time::Instant::now();
    cabin.last_heartbeat = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(30))
        .expect("age last_heartbeat");

    cabin.tick_heartbeat();

    assert!(
        cabin.ideas_rx.is_none(),
        "heartbeat must not start the ideas model call"
    );
    assert!(!cabin.running);
    assert!(cabin.pending_hub_task.is_none());
    assert!(cabin.night_check_rx.is_none());
    assert!(cabin.last_heartbeat.elapsed() < std::time::Duration::from_secs(5));

    let stamped = cabin.last_heartbeat;
    cabin.tick_heartbeat();

    assert!(
        cabin.ideas_rx.is_none(),
        "heartbeat must not start the ideas model call"
    );
    assert!(!cabin.running);
    assert!(cabin.pending_hub_task.is_none());
    assert!(cabin.night_check_rx.is_none());
    assert_eq!(cabin.last_heartbeat, stamped);
}

// Landed from PR #177.
#[test]
fn build_idea_files_one_todo() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("build-idea");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.board.clear();
    cabin.updates = vec![grokhub_core::UpdateCard {
        id: "idea-1".into(),
        kind: grokhub_core::UpdateKind::Idea,
        title: "Ship the harbor".into(),
        body: Some(String::new()),
        created_at: 0,
        status: grokhub_core::UpdateStatus::Unread,
        action: None,
        expires_at: None,
        held: false,
        citations: Vec::new(),
        reaction: None,
        discuss_thread: None,
        built: false,
        board_id: None,
        why: None,
        feed_pin: false,
        feed_kept: false,
        prompt: None,
        idea_kind: None,
        details: None,
        draft: None,
        modified: false,
        source_id: String::new(),
        skill: None,
        runs: 1,
        dismissed_at: 0,
        done_for_you: None,
        pulse: Default::default(),
    }];

    cabin.build_idea("nope");
    assert!(cabin.board.is_empty());
    assert!(!cabin.running);

    cabin.build_idea("idea-1");
    assert_eq!(cabin.board.len(), 1);
    assert_eq!(
        cabin.board[0].title,
        grokhub_core::idea_todo_title("Ship the harbor", "")
    );
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert!(cabin.updates[0].built);
    assert_eq!(
        cabin.updates[0].board_id.as_deref(),
        Some(cabin.board[0].id.as_str())
    );
    assert!(!cabin.running);

    cabin.build_idea("idea-1");
    assert_eq!(cabin.board.len(), 1);
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    assert!(!cabin.running);
}

// Landed from PR #179.
#[test]
fn react_and_archive_stay_on_the_card() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("react-archive");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.updates = vec![
        feed_card("idea-1", grokhub_core::UpdateKind::Idea, false),
        feed_card("digest-1", grokhub_core::UpdateKind::Digest, true),
        feed_card("event-1", grokhub_core::UpdateKind::AutomationDone, false),
    ];

    cabin.close_home_card("digest-1");
    let digest = cabin
        .updates
        .iter()
        .find(|card| card.id == "digest-1")
        .expect("digest stays");
    assert_ne!(digest.status, grokhub_core::UpdateStatus::Dismissed);
    assert!(digest.pulse.off_home);
    assert!(digest.held);
    assert!(!cabin.running);

    cabin.close_home_card("idea-1");
    let idea = cabin
        .updates
        .iter()
        .find(|card| card.id == "idea-1")
        .expect("idea stays");
    assert_ne!(idea.status, grokhub_core::UpdateStatus::Dismissed);
    assert!(!idea.pulse.off_home);
}

// Landed from PR #180.
#[test]
fn feed_actions_open_pages_without_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("feed-actions");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.new_thread(false);
    cabin.live_mut().push(("user".into(), "keep".into()));
    let older = cabin.threads[cabin.thread_idx].id.clone();
    cabin.new_thread(false);
    let newer = cabin.threads[cabin.thread_idx].id.clone();
    assert_ne!(older, newer);
    assert!(!cabin.running);

    cabin.follow_update_action(Some(UpdateAction::OpenWorkboard));
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    assert!(!cabin.running);
    let nav_before_unknown = cabin.nav;

    cabin.follow_update_action(Some(UpdateAction::OpenSession {
        thread_id: "missing-session".into(),
    }));
    assert!(cabin.nav == nav_before_unknown);
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    assert!(!cabin.running);

    cabin.follow_update_action(Some(UpdateAction::OpenSession {
        thread_id: older.clone(),
    }));
    assert_eq!(cabin.threads[cabin.thread_idx].id, older);
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert!(!cabin.running);

    cabin.follow_update_action(Some(UpdateAction::OpenWorkboard));
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    assert!(!cabin.running);

    cabin.follow_update_action(Some(UpdateAction::OpenAutomations));
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert!(!cabin.running);

    cabin.follow_update_action(Some(UpdateAction::DeepLink {
        href: "https://example.test/harbor".into(),
    }));
    assert_eq!(cabin.status, "https://example.test/harbor");
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert!(!cabin.running);

    let status_before_blank = cabin.status.clone();
    cabin.follow_update_action(Some(UpdateAction::DeepLink { href: "   ".into() }));
    assert_eq!(cabin.status, status_before_blank);
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert!(!cabin.running);

    let nav_before_none = cabin.nav;
    let status_before_none = cabin.status.clone();
    cabin.follow_update_action(None);
    assert!(cabin.nav == nav_before_none);
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert_eq!(cabin.status, status_before_none);
    assert!(!cabin.running);
}

// Landed from PR #181.
fn offer_card(id: &str, title: &str, status: UpdateStatus) -> UpdateCard {
    UpdateCard {
        id: id.to_string(),
        kind: UpdateKind::AutomateOffer,
        title: title.to_string(),
        body: None,
        created_at: 1,
        status,
        action: None,
        expires_at: None,
        held: false,
        citations: Vec::new(),
        reaction: None,
        discuss_thread: None,
        built: false,
        board_id: None,
        why: None,
        feed_pin: false,
        feed_kept: false,
        prompt: None,
        idea_kind: None,
        details: None,
        draft: None,
        modified: false,
        source_id: String::new(),
        skill: None,
        runs: 1,
        dismissed_at: 0,
        done_for_you: None,
        pulse: Default::default(),
    }
}

fn card_status(cabin: &Cabin, id: &str) -> UpdateStatus {
    cabin
        .updates
        .iter()
        .find(|card| card.id == id)
        .map(|card| card.status)
        .unwrap_or_else(|| panic!("missing card {id}"))
}

#[test]
fn automate_offer_files_a_daily_job() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("automate-offer");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.automations.clear();
    cabin.updates = vec![
        offer_card("plain", "what is rust", UpdateStatus::Unread),
        offer_card(
            "daily",
            "every day at 9, summarize the board",
            UpdateStatus::Unread,
        ),
        offer_card(
            "gone",
            "every day at 9, summarize the board",
            UpdateStatus::Dismissed,
        ),
    ];

    cabin.accept_automate_offer("missing");
    assert!(cabin.automations.is_empty());
    assert!(!cabin.running);

    cabin.accept_automate_offer("gone");
    assert!(cabin.automations.is_empty());
    assert!(!cabin.running);
    assert_eq!(card_status(&cabin, "gone"), UpdateStatus::Dismissed);

    cabin.accept_automate_offer("plain");
    assert_eq!(card_status(&cabin, "plain"), UpdateStatus::Opened);
    assert!(cabin.automations.is_empty());
    assert!(!cabin.running);

    cabin.accept_automate_offer("daily");
    assert_eq!(card_status(&cabin, "daily"), UpdateStatus::Opened);
    assert_eq!(cabin.automations.len(), 1);
    assert!(!cabin.running);

    cabin.accept_automate_offer("daily");
    assert_eq!(cabin.automations.len(), 2);
    assert!(!cabin.running);

    // Repeated-ask offer: human title + editable schedule preview in `prompt`.
    let mut preview = offer_card(
        "repeat",
        "You ask this most evenings around 7 PM, want me to do it automatically?",
        UpdateStatus::Unread,
    );
    preview.prompt = Some("every day at 7pm, check patch notes".into());
    preview.source_id = "askoffer:patch-note@h19".into();
    cabin.automations.clear();
    cabin.updates.push(preview);
    cabin.accept_automate_offer("repeat");
    assert_eq!(cabin.automations.len(), 1);
    assert_eq!(cabin.automations[0].schedule, "daily");
    assert_eq!(cabin.automations[0].time, "19:00");
    assert_eq!(cabin.automations[0].name, "check patch notes");

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn ask_offer_dismiss_persists_in_ledger() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("ask-offer-dismiss");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    let mut card = offer_card(
        "repeat",
        "You ask this most evenings around 7 PM, want me to do it automatically?",
        UpdateStatus::Unread,
    );
    card.source_id = "askoffer:patch-note@h19".into();
    card.prompt = Some("every day at 7pm, check patch notes".into());
    cabin.ask_offer_dismissed(&card);
    let ledger = super::super::ask_offers_ui::load_ask_offer_ledger();
    assert!(ledger.dismissed.contains("patch-note@h19"));

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn feed_offer_click_opens_the_schedule_box_without_saving() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("feed-offer-open");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.automations.clear();
    cabin.updates = vec![offer_card(
        "daily",
        "every day at 9, summarize the board",
        UpdateStatus::Unread,
    )];
    cabin.open_feed_card("daily");
    assert!(cabin.auto_compose);
    assert_eq!(cabin.night_nl, "every day at 9, summarize the board");
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert!(cabin.automations.is_empty());
    assert_eq!(card_status(&cabin, "daily"), UpdateStatus::Opened);
    cabin.open_feed_card("daily");
    assert!(cabin.automations.is_empty());

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

/// The first card on the feed that `pick` matches, or a failure naming `site`.
fn posted_card(cabin: &Cabin, site: &str, pick: impl Fn(&UpdateCard) -> bool) -> UpdateCard {
    cabin
        .updates
        .iter()
        .find(|c| pick(c))
        .cloned()
        .unwrap_or_else(|| panic!("{site} posted no card: {:?}", cabin.updates))
}

/// Fns in this crate's live code that build or post a Home card.
fn app_feed_post_sites() -> std::collections::BTreeSet<String> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sites = std::collections::BTreeSet::new();
    let mut dirs = vec![src];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".rs") || name.ends_with("tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
            let live = text.split("\n#[cfg(test)]\nmod ").next().unwrap();
            let mut current = String::new();
            for line in live.lines() {
                let decl = line
                    .trim_start()
                    .trim_start_matches("pub(super) ")
                    .trim_start_matches("pub(crate) ")
                    .trim_start_matches("pub ");
                if let Some(rest) = decl.strip_prefix("fn ") {
                    current = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                }
                let posts = line.contains("post_feed_card(") || line.contains("post_update(");
                if (posts && current != "post_feed_card")
                    || line.contains("-> grokhub_core::UpdateCard")
                    || line.contains("-> UpdateCard")
                {
                    sites.insert(current.clone());
                }
            }
        }
    }
    sites
}

/// Every Home card the cabin posts names its item: one row per post site,
/// driven through the site where a test cabin can, else built from the same
/// call the site makes. A new site with no row fails here.
#[test]
fn every_home_card_the_cabin_posts_names_its_item() {
    use grokhub_agent::route::heal::{joined_messages, pause_msg};
    use grokhub_agent::route::tune;
    use grokhub_core::proactive::{proactive_card, Candidate, CandidateSource, ProactiveRoute};
    let _g = config::hold_test_config();
    let root = config::test_config_root("feed-names");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let _pin = config::TestConfigDir::set(root.clone());
    let mut cabin = Cabin::quiet_for_test();
    let now = now_ms();
    let mut rows: Vec<(&str, UpdateCard, &str)> = Vec::new();

    cabin.note_automation_done("auto-1", "Nightly backup", "Copied 3 files.");
    rows.push((
        "note_automation_done",
        posted_card(&cabin, "note_automation_done", |c| c.source_id == "auto-1"),
        "Nightly backup",
    ));
    cabin.note_schedule_created("sched-1", "Morning brief", "every day at 8:00");
    rows.push((
        "note_schedule_created",
        posted_card(&cabin, "note_schedule_created", |c| {
            c.source_id == "sched-1"
        }),
        "Morning brief",
    ));
    cabin.automations.push(health_job("job-7"));
    cabin.note_auto_failed("job-7", "The disk is full.");
    rows.push((
        "note_auto_failed",
        posted_card(&cabin, "note_auto_failed", |c| c.title.ends_with(" failed")),
        "Board summary",
    ));
    cabin
        .live_mut()
        .push(("user".into(), "Index ~/Projects".into()));
    cabin.post_turn_crash();
    rows.push((
        "post_turn_crash",
        posted_card(&cabin, "post_turn_crash", grokhub_core::is_crash_card),
        "Index ~/Projects",
    ));
    cabin.deliver_heal(super::router_ui::HealOut {
        msgs: [
            vec![pause_msg("grok-4.7", "It failed 3 times in a row.", &[])],
            joined_messages(&["grok-4.8".to_string()]),
        ]
        .concat(),
        cleared: Vec::new(),
    });
    rows.push((
        "deliver_heal",
        posted_card(&cabin, "deliver_heal", |c| c.title.starts_with("Paused:")),
        "grok-4.7",
    ));
    rows.push((
        "deliver_heal",
        posted_card(&cabin, "deliver_heal", |c| c.source_id == "new:grok-4.8"),
        "grok-4.8",
    ));
    let promoted = tune::Candidate {
        id: "chat:default#3".into(),
        class: "chat:default".into(),
        change: Some(tune::Change::Order {
            model: "grok-4-fast".into(),
        }),
        stage: tune::Stage::Watch,
        promoted_at: Some(1_791_298_800_000),
        baseline: Some(tune::Snap {
            n: 400,
            pass_pct: 85.0,
            rework_pct: 4.0,
            cost_per_step: 0.0100,
            p50_ms: 2_000,
            p95_ms: 4_000,
        }),
        promoted_snap: Some(tune::Snap {
            n: 60,
            pass_pct: 92.0,
            rework_pct: 3.0,
            cost_per_step: 0.0088,
            p50_ms: 1_600,
            p95_ms: 3_200,
        }),
        ..tune::Candidate::default()
    };
    cabin.post_why_cards(
        &tune::TuneState {
            candidates: vec![promoted],
            ..tune::TuneState::default()
        },
        &["chat:default#3".to_string()],
    );
    rows.push((
        "post_why_cards",
        posted_card(&cabin, "post_why_cards", |c| {
            c.source_id.starts_with("router-why:")
        }),
        "grok-4-fast",
    ));
    cabin.post_screen_done(
        "Screen recording 0:42: video stutters on 4K",
        std::path::Path::new("rec-1"),
        "",
        Err("No xAI key.".into()),
    );
    rows.push((
        "post_screen_done",
        posted_card(&cabin, "post_screen_done", |c| {
            c.source_id.starts_with("screenrec:")
        }),
        "video stutters on 4K",
    ));
    let report = grokhub_core::audio_check::AudioReport {
        title: "Yeti input clipping".into(),
        card_title: "Audio check: Yeti input clipping at -0.2 dBFS".into(),
        summary: "Lower the gain.".into(),
        text: "Yeti input clipping at -0.2 dBFS.".into(),
    };
    cabin.finish_audio_check("", Ok(("Yeti".into(), report)));
    rows.push((
        "finish_audio_check",
        posted_card(&cabin, "finish_audio_check", |c| {
            c.source_id.starts_with("audiocheck:")
        }),
        "Yeti",
    ));
    let sam = Candidate::soft(
        CandidateSource::Mail,
        "reply to Sam's email",
        "sam reply",
        0.9,
        0.8,
        "Sam asked about Friday.",
    );
    assert_eq!(cabin.post_proactive(vec![sam], now), 1);
    rows.push((
        "post_proactive",
        posted_card(&cabin, "post_proactive", |c| c.pulse.proactive.is_some()),
        "Sam's email",
    ));
    let revert = grokhub_core::self_review::RevertCandidate {
        skill: "inbox-zero".into(),
        version: 2,
        before: (9, 10),
        after: (3, 10),
    };
    let (title, body) = (revert.title(), revert.numbers());
    cabin.harness.self_review.budget = 1;
    cabin.post_self_review_card(
        super::self_review_ui::ProposalCard {
            source: "revert:inbox-zero".into(),
            skill: "inbox-zero",
            title: &title,
            body: &body,
            details: String::new(),
            md: "",
        },
        now,
    );
    rows.push((
        "post_self_review_card",
        posted_card(&cabin, "post_self_review_card", |c| {
            c.source_id == "revert:inbox-zero"
        }),
        "inbox-zero",
    ));
    cabin.post_router_review(now);
    rows.push((
        "post_router_review",
        posted_card(&cabin, "post_router_review", |c| {
            c.source_id == "router-review"
        }),
        "router review",
    ));
    cabin
        .live_mut()
        .push(("user".into(), "Summarize inbox".into()));
    cabin.start_model_wait("chat:default", "grok-4.7", now);
    rows.push((
        "start_model_wait",
        posted_card(&cabin, "start_model_wait", grokhub_core::is_model_wait_card),
        "grok-4.7",
    ));
    std::fs::write(root.join("projects.json"), "{\"torn").unwrap();
    let _: Vec<serde_json::Value> =
        config::load_json(&root.join("projects.json"), config::JSON_STORE_CAP);
    cabin.poll_recovered_stores();
    rows.push((
        "poll_recovered_stores",
        posted_card(&cabin, "poll_recovered_stores", |c| {
            c.source_id.starts_with("recovered:")
        }),
        "Projects list",
    ));
    cabin.add_interest("Rust");
    cabin.post_source_run(super::pulse_sources_ui::SourceRun {
        items: vec![grokhub_core::pulse_sources::SourceItem {
            title: "Rust 1.99 released".into(),
            url: "https://blog.rust-lang.org/2026/10/09/Rust-1.99.html".into(),
            kind: grokhub_core::pulse_sources::SourceKind::HackerNews,
            source_name: "Hacker News".into(),
            published_ms: now,
            summary: "812 points, 301 comments on Hacker News.".into(),
            image: None,
            topic: Some("Rust".into()),
            engagement: 0,
        }],
        errors: Vec::new(),
        fetched: 1,
        asked: 1,
        at_ms: now,
    });
    rows.push((
        "finish_source_run",
        posted_card(&cabin, "finish_source_run", |c| {
            c.source_id.starts_with("source:")
        }),
        "Rust 1.99",
    ));
    rows.push((
        "finish_wishlist",
        grokhub_core::steam_wishlist::Alert {
            app_id: 4242,
            name: "Fake Quest".into(),
            price: grokhub_core::steam_wishlist::Price {
                initial: 1999,
                final_price: 999,
                discount: 50,
                initial_text: "$19.99".into(),
                final_text: "$9.99".into(),
            },
            new_low: true,
        }
        .card(now),
        "Fake Quest",
    ));
    rows.push((
        "finish_wind_down",
        grokhub_core::wind_down::Offer::Late { idle_min: 45 }.card(
            &grokhub_core::LocalClock {
                now_ms: now,
                weekday: 3,
                hour: 2,
                minute: 0,
            },
            grokhub_core::wind_down::Voice::Plain,
        ),
        "Sleep the PC",
    ));
    rows.push((
        "finish_achievements",
        grokhub_core::steam_achievements::Nudge {
            app_id: 1145360,
            game: "Hades".into(),
            kind: grokhub_core::steam_achievements::Kind::Close,
            done: 48,
            total: 50,
            left: vec![grokhub_core::steam_achievements::Left {
                name: "Is There No Escape?".into(),
                percent: Some(21.4),
            }],
        }
        .card(now),
        "Hades",
    ));
    rows.push((
        "take_tidy_scan",
        grokhub_core::downloads_tidy::card(
            &[grokhub_core::downloads_tidy::Finding {
                file: grokhub_core::downloads_tidy::FileInfo {
                    path: "/home/user/Downloads/old.iso".into(),
                    name: "old.iso".into(),
                    bytes: 900 * 1024 * 1024,
                    modified_ms: 0,
                },
                kind: grokhub_core::downloads_tidy::Kind::BigArchive,
                why: "Not touched in 60 days".into(),
            }],
            now,
        )
        .expect("tidy card"),
        "Downloads",
    ));
    rows.push((
        "show_backup_reminder",
        grokhub_core::backup_nudge::Reminder {
            lines: vec!["my-app has 2 commits on feat-x not pushed for 4 days.".into()],
            push: None,
            tool: None,
        }
        .card(now),
        "my-app",
    ));
    rows.push((
        "note_probe",
        grokhub_core::net_log::insight_card(
            &[&grokhub_core::net_log::Outage {
                start_ms: 2 * 60 * 60 * 1000,
                end_ms: Some(3 * 60 * 60 * 1000),
                offset_min: 0,
            }],
            now,
        )
        .expect("insight"),
        "internet",
    ));
    rows.push((
        "finish_release_radar",
        grokhub_core::release_radar::Upcoming {
            app_id: 9,
            name: "Fake Quest: Frontier".into(),
            base: "Fake Quest".into(),
            kind: grokhub_core::release_radar::Kind::Dlc,
            date_text: "14 Nov, 2026".into(),
            date_ms: Some(1),
        }
        .radar_card(now),
        "Fake Quest: Frontier",
    ));
    rows.push((
        "post_due_reminders",
        grokhub_core::release_radar::out_today_card(
            9,
            &grokhub_core::release_radar::Reminder {
                name: "Fake Quest: Frontier".into(),
                date_ms: 1,
            },
            now,
        ),
        "Fake Quest: Frontier",
    ));
    rows.push((
        "post_companion",
        grokhub_core::game_companion::Companion {
            game: grokhub_core::steam::SteamGame {
                app_id: 7,
                name: "Fake Quest".into(),
                last_played: 1,
                playtime_min: 60,
            },
            news: Some(Vec::new()),
            note: None,
        }
        .companion_card(now),
        "Fake Quest",
    ));
    rows.push((
        "finish_local_models",
        grokhub_core::local_model_news::Suggestion::Release {
            repo: "ggml-org/Qwen3-14B-GGUF".into(),
            run_mb: 10_020,
            budget_mb: 16_384,
            mem: grokhub_core::local_picker::Memory::Gpu,
            like: None,
        }
        .card(now),
        "Qwen3-14B",
    ));
    rows.push((
        "post_model_offer",
        grokhub_core::model_drop::ModelOffer {
            file: grokhub_core::model_drop::ModelFile {
                path: "/home/user/Downloads/bracket.stl".into(),
                name: "bracket.stl".into(),
                bytes: 2048,
            },
            dims: Some([60.0, 20.0, 40.0]),
            thumb: None,
        }
        .card(None, now),
        "bracket.stl",
    ));
    rows.push((
        "tick_daily_drop",
        grokhub_core::pulse_sources::item_card(
            &grokhub_core::pulse_sources::Ranked {
                item: grokhub_core::pulse_sources::SourceItem {
                    title: "Daily drop story".into(),
                    url: "https://hn.test/daily-drop".into(),
                    kind: grokhub_core::pulse_sources::SourceKind::HackerNews,
                    source_name: "Hacker News".into(),
                    published_ms: now,
                    summary: "A ranked pick for the overnight drop.".into(),
                    image: None,
                    topic: None,
                    engagement: 0,
                },
                score: 80,
                because: None,
            },
            now,
        ),
        "Daily drop story",
    ));
    rows.push((
        "model_wait_card",
        super::model_wait_ui::model_wait_card(
            &["grok-4.6".into()],
            "grok-4.6",
            "9:05 PM",
            "dream time",
            now,
        ),
        "grok-4.6",
    ));
    // Sites a test cabin can't reach without a live run: the card from the same call.
    let guard_text =
        "Auto was succeeding less on everyday chat, so it now starts that work one step higher.";
    let guard = super::router_ui::guard_card("chat:default", guard_text, now);
    rows.push(("guard_card", guard.clone(), "everyday chat"));
    rows.push(("poll_guard", guard, "everyday chat"));
    let run = super::background::BgRun {
        id: 1,
        thread_id: "t-bg".into(),
        title: "Sync ~/Photos".into(),
        origin: grokhub_core::BgOrigin::User,
        rx: None,
        say: String::new(),
        action: String::new(),
        started: std::time::Instant::now(),
        end: None,
        session: String::new(),
        resumed: None,
        fork_hold: false,
        native_session: None,
        automation: None,
    };
    let crashed = super::background::bg_crash_card(&run);
    rows.push(("bg_crash_card", crashed.clone(), "Sync ~/Photos"));
    rows.push(("poll_bg_runs", crashed, "Sync ~/Photos"));
    let change = grokhub_agent::harness::Change {
        seq: 3,
        at: now,
        kind: "model".into(),
        id: "chat:default".into(),
        op: grokhub_agent::harness::ChangeOp::Modify,
        origin: Default::default(),
        reason: String::new(),
        before_hash: String::new(),
        after_hash: String::new(),
        undoes: None,
        label: String::new(),
    };
    rows.push((
        "model_change_note",
        super::change_undo::model_change_note(&change),
        "chat:default",
    ));
    let trial = grokhub_agent::harness::Version {
        version: "skill:save-page@0123456789ab".into(),
        label: "Use ctrl+s in save-page".into(),
        diff: "+1 -3 lines".into(),
        share_pct: 20,
        min_runs: 10,
        status: grokhub_agent::harness::VersionStatus::Kept,
        note: "Kept: Use ctrl+s in save-page, 18% fewer steps over 12 runs".into(),
        at: now,
        ..Default::default()
    };
    cabin.post_trial_note(&trial);
    rows.push((
        "post_trial_note",
        posted_card(&cabin, "post_trial_note", |c| {
            c.source_id.starts_with("canary:")
        }),
        "save-page",
    ));
    let trying = grokhub_agent::harness::Version {
        status: grokhub_agent::harness::VersionStatus::Canary,
        note: String::new(),
        ..trial
    };
    rows.push((
        "trial_card",
        super::canary_ui::trial_card(&trying),
        "save-page",
    ));
    rows.push((
        "poll_self_changes",
        grokhub_core::self_change_card("skill", "inbox-zero", "added", "", now),
        "inbox-zero",
    ));
    // Model dream morning notice (amr_memory::post_dream_card).
    rows.push((
        "post_dream_card",
        grokhub_core::automation_done_card(
            "dream-model",
            "Memory dream",
            "Proposed 1 USER line.",
            now,
        ),
        "Memory dream",
    ));
    let done = grokhub_core::DoneForYou {
        kind: "connection".into(),
        target: "notes".into(),
        undo_ref: 4,
        mind_key: "proactive:connection_disable".into(),
        answered: false,
    };
    rows.push((
        "tick_auto_act",
        grokhub_core::done_for_you_card(
            "Turned off the notes connection",
            "It failed every start.",
            done,
            now,
        ),
        "notes connection",
    ));
    let notes = Candidate::soft(
        CandidateSource::SystemState,
        "turn off the notes connection",
        "turn off the notes connection",
        0.8,
        0.9,
        "It failed every start.",
    );
    rows.push((
        "offer_card",
        proactive_card(&notes, ProactiveRoute::Suggest, now),
        "notes connection",
    ));
    rows.push((
        "offer_card",
        proactive_card(&notes, ProactiveRoute::ICan, now),
        "notes connection",
    ));
    let gpu_snap = grokhub_core::nudge_watchers::WatcherSnapshot {
        gpu: Some(grokhub_core::nudge_watchers::GpuDriverSnap {
            gpu_name: "Test GPU".into(),
            installed: "1.0".into(),
            offered: "2.0".into(),
            download_url: "https://example.com/driver".into(),
        }),
        ..Default::default()
    };
    let nudge = grokhub_core::nudge_watchers::detect_gpu_driver(&gpu_snap).expect("gpu nudge");
    let print_alert = grokhub_core::AlertResult {
        kind: grokhub_core::AlertKind::Failed,
        printer_key: "k1".into(),
        status: grokhub_core::PrinterStatus {
            name: "Shop K2".into(),
            kind: grokhub_core::PrinterKind::Moonraker,
            state: grokhub_core::PrintState::Error,
            progress_pct: Some(42.0),
            eta_secs: None,
            filename: Some("bracket.gcode".into()),
            message: None,
            camera_path: None,
        },
        camera_enabled: false,
    };
    rows.push((
        "tick_print_alerts",
        grokhub_core::alert_card(&print_alert, now),
        "Shop K2",
    ));
    rows.push((
        "tick_nudge_watchers",
        grokhub_core::nudge_watcher_card(&nudge, now),
        "Test GPU",
    ));
    let mut health_session = None;
    grokhub_core::pc_health::note_sample(
        &mut health_session,
        grokhub_core::SensorSample {
            at_ms: now,
            cpu_temp_c: Some(70.0),
            gpu_temp_c: Some(75.0),
            cpu_util_pct: Some(40),
            gpu_util_pct: Some(85),
            cpu_clock_mhz: Some(4000),
            gpu_clock_mhz: Some(1800),
            game_label: Some("Hades".into()),
        },
        now,
    );
    grokhub_core::pc_health::note_sample(
        &mut health_session,
        grokhub_core::SensorSample {
            at_ms: now + 70_000,
            cpu_temp_c: Some(72.0),
            gpu_temp_c: Some(78.0),
            cpu_util_pct: Some(42),
            gpu_util_pct: Some(88),
            cpu_clock_mhz: Some(4100),
            gpu_clock_mhz: Some(1850),
            game_label: Some("Hades".into()),
        },
        now + 70_000,
    );
    let health =
        grokhub_core::pc_health::summarize(health_session.as_ref().expect("session"), now + 70_000);
    rows.push((
        "tick_pc_health",
        grokhub_core::summary_card(&health, now),
        "Hades",
    ));
    rows.push((
        "tick_ask_offers",
        grokhub_core::build_offer_card(
            &grokhub_core::AskCluster {
                key: "patch-note@h19".into(),
                example: "check patch notes".into(),
                count: 3,
                peak_hour: 19,
                weekday_share_pct: 40,
                kind: grokhub_core::OfferKind::Automation,
            },
            now,
        ),
        "patch notes",
    ));
    let rust_step = grokhub_core::next_step(
        grokhub_core::HobbyId::Rust,
        &grokhub_core::HobbyProgress::default(),
    )
    .expect("rust step");
    rows.push((
        "tick_hobby_paths",
        grokhub_core::hobby_path_card(grokhub_core::HobbyId::Rust, rust_step, now),
        "Rust",
    ));

    for (site, card, name) in &rows {
        let text = format!("{} {}", card.title, card.body.as_deref().unwrap_or(""));
        assert!(text.contains(name), "{site}: {name:?} not in {text:?}");
        assert!(grokhub_core::names_its_item(card), "{site}: {card:?}");
        let mut feed = Vec::new();
        assert!(
            grokhub_core::post_update(&mut feed, card.clone()),
            "{site}: the feed refused a named card"
        );
    }
    let covered: std::collections::BTreeSet<String> =
        rows.iter().map(|(site, ..)| site.to_string()).collect();
    let missing: Vec<String> = app_feed_post_sites()
        .difference(&covered)
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "Home card sites with no row in this test: {missing:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// Folded from PR #434.
#[test]
fn greeting_galley_h_drives_empty_home_greet_top() {
    // Real empty-home placement path: `greeting_galley_h` + mark → `empty_home_greet_top`.
    // Quiet cabin only: no greeting model call.
    let mut cabin = Cabin::quiet_for_test();
    cabin.greeting = "Hello from empty home".into();
    let ctx = egui::Context::default();
    // The greeting is set in the app's bundled fonts. `install_fonts` runs once
    // per process, so in the full suite this fresh context would get none.
    crate::theme::install_fonts_on(&ctx);
    let _ = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
        let mark_h = 40.0 + 12.0;
        let short_h = super::greeting_galley_h(ui, &cabin.greeting, 480.0);
        assert!(
            short_h >= crate::theme::GREET_HERO,
            "galley height is at least GREET_HERO: {short_h}"
        );
        let composer_top = super::empty_home_composer_top(800.0, crate::theme::QUERY_MIN_H);
        let short_top = super::empty_home_greet_top(composer_top, short_h + mark_h, 12.0);
        assert!(
            short_top >= 0.0,
            "greeting top stays non-negative: {short_top}"
        );
        let long = "word ".repeat(80);
        let long_h = super::greeting_galley_h(ui, &long, 200.0);
        assert!(
            long_h > short_h,
            "narrow wrap grows galley height: {long_h} vs {short_h}"
        );
        let long_top = super::empty_home_greet_top(composer_top, long_h + mark_h, 12.0);
        assert!(
            long_top < short_top,
            "taller wrapped greeting rises: {long_top} vs {short_top}"
        );
    });
    assert!(!cabin.running, "galley height must not start a run");
    assert!(
        cabin.chat_job_thread.is_none(),
        "galley height must not spawn a chat job"
    );
}

#[test]
fn a_fresh_launch_opens_on_home_with_a_clean_chat_draft_behind_it() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("launch-home");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut old = crate::threads::ChatThread::new("Yesterday", false);
    old.messages = std::sync::Arc::new(vec![("user".into(), "hi".into())]);
    let mut cabin = Cabin::quiet_cabin(vec![old], 0);
    cabin.open_launch_page();
    assert_eq!(cabin.nav, Nav::Home);
    assert!(cabin.messages.is_empty(), "Chat waits with an empty draft");
    assert_eq!(cabin.threads.len(), 2, "yesterday's chat is kept");
    let prev_shot = std::env::var("GROKHUB_DECK_SHOT").ok();
    std::env::set_var("GROKHUB_DECK_SHOT", "1");
    cabin.open_launch_page();
    match prev_shot {
        Some(v) => std::env::set_var("GROKHUB_DECK_SHOT", v),
        None => std::env::remove_var("GROKHUB_DECK_SHOT"),
    }
    assert_eq!(
        cabin.nav,
        Nav::Chat,
        "deck shots still open on the Hangout chat"
    );
    release_isolated(&root, cabin);
}
