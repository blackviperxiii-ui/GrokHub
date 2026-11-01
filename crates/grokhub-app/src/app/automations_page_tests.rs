//! Automations page and scheduled run tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

#[test]
fn a_failed_scheduled_turn_marks_the_job_and_posts_one_card() {
    let root = config::test_config_root("auto-health");
    let _ = std::fs::remove_dir_all(&root);
    let _pin = config::TestConfigDir::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.automations = vec![health_job("a1")];
    let cards_before = app.updates.len();

    // A run that settles no automation touches no job.
    app.settle_scheduled_run(None, &grokhub_core::BgEnd::Failed("other run".into()), "");
    assert!(app.automations[0].health.is_clear());

    app.settle_scheduled_run(
        Some("a1"),
        &grokhub_core::BgEnd::Failed("Error: credit limit".into()),
        "",
    );
    assert_eq!(
        app.automations[0].health.outcome,
        grokhub_core::AutoOutcome::Failed
    );
    assert_eq!(app.automations[0].health.error, "credit limit");
    assert_eq!(app.updates.len(), cards_before + 1);
    assert!(app
        .updates
        .iter()
        .any(|c| c.title == "Board summary failed"));

    // A second failure in the streak updates the job, not the feed.
    app.settle_scheduled_run(Some("a1"), &grokhub_core::BgEnd::Failed("again".into()), "");
    assert_eq!(app.automations[0].health.fail_streak, 2);
    assert_eq!(app.updates.len(), cards_before + 1);

    // A halt is not a failure and not a success.
    app.settle_scheduled_run(Some("a1"), &grokhub_core::BgEnd::Stopped, "");
    assert_eq!(
        app.automations[0].health.outcome,
        grokhub_core::AutoOutcome::Stopped
    );
    assert_eq!(app.automations[0].health.fail_streak, 2);

    app.settle_scheduled_run(Some("a1"), &grokhub_core::BgEnd::Done, "");
    assert!(!app.automations[0].health.failing());
    assert_eq!(app.automations[0].health.fail_streak, 0);

    // A run with no automation touches no job.
    app.settle_scheduled_run(None, &grokhub_core::BgEnd::Failed("x".into()), "");
    assert_eq!(app.automations[0].health.fail_streak, 0);
}

#[test]
fn scheduled_turns_settle_on_every_turn_end() {
    let src = cabin_src();
    let posted = fn_src(&src, "post_finished_bg_runs");
    assert!(
        posted.contains("BgOrigin::Scheduled") && posted.contains("settle_scheduled_run"),
        "{posted}"
    );
    let fire = fn_src(&src, "fire_night");
    assert!(fire.contains("start_scheduled_run"), "{fire}");
    assert!(fire.contains("note_auto_failed"), "{fire}");
    let start = fn_src(&src, "start_scheduled_run");
    assert!(
        start.contains("BgOrigin::Scheduled") && start.contains("run.automation = Some("),
        "{start}"
    );
    let page = fn_src(&src, "ui_scheduled_automations");
    assert!(page.contains("automation_health_line"), "{page}");
}

thread_local! {
    /// `GROKHUB_CONFIG` as it was before [`pin_auto_config`] set it.
    static PREV_CONFIG_ENV: std::cell::RefCell<Option<Option<std::ffi::OsString>>> =
        const { std::cell::RefCell::new(None) };
}

fn pin_auto_config(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
    let root = crate::config::test_config_root(label);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    PREV_CONFIG_ENV.with(|p| *p.borrow_mut() = Some(std::env::var_os("GROKHUB_CONFIG")));
    std::env::set_var("GROKHUB_CONFIG", &root);
    (crate::config::TestConfigDir::set(root.clone()), root)
}

/// Put the process-wide variable back as it was. Unsetting it would send
/// every later test in this process (plain `cargo test`) to the real config.
fn restore_config_env() {
    match PREV_CONFIG_ENV.with(|p| p.borrow_mut().take()) {
        Some(Some(v)) => std::env::set_var("GROKHUB_CONFIG", v),
        Some(None) => std::env::remove_var("GROKHUB_CONFIG"),
        None => {}
    }
}

fn failing_price_job() -> grokhub_core::Automation {
    let mut a = health_job("auto-price");
    a.name = "Sample price check".into();
    a.schedule = "weekdays".into();
    a.time = "12:00".into();
    a.instructions = "Check the saved laptop-stand price and note any drop.".into();
    a.run_count = 7;
    a.health = grokhub_core::AutoHealth {
        outcome: grokhub_core::AutoOutcome::Failed,
        error: "Sample error: page did not load".into(),
        fail_streak: 2,
    };
    a
}

fn healthy_morning_job() -> grokhub_core::Automation {
    let mut a = health_job("auto-morning");
    a.name = "Sample morning brief".into();
    a.schedule = "daily".into();
    a.time = "07:30".into();
    a.instructions = "Summarize the workboard and anything that changed overnight.".into();
    a.run_count = 12;
    a
}

fn collect_text_rects(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(text) => {
            out.push((text.galley.job.text.clone(), text.visual_bounding_rect()));
        }
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_text_rects(shape, out);
            }
        }
        _ => {}
    }
}

struct AutoPage {
    ctx: egui::Context,
    frame: u32,
}

impl AutoPage {
    fn new() -> Self {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        Self { ctx, frame: 0 }
    }

    fn step(&mut self, cabin: &mut Cabin, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 900.0),
            )),
            time: Some(f64::from(self.frame) / 60.0),
            predicted_dt: 1.0 / 60.0,
            events,
            ..Default::default()
        };
        let out = crate::theme::test_pass(&self.ctx, raw, |ui| {
            cabin.ui_night(ui);
        });
        self.frame += 1;
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            collect_text_rects(&clipped.shape, &mut texts);
        }
        texts
    }

    fn settle(&mut self, cabin: &mut Cabin) -> Vec<(String, egui::Rect)> {
        self.step(cabin, Vec::new());
        self.step(cabin, Vec::new())
    }

    fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn click(&mut self, cabin: &mut Cabin, pos: egui::Pos2) -> Vec<(String, egui::Rect)> {
        self.step(
            cabin,
            vec![egui::Event::PointerMoved(pos), Self::press(pos, true)],
        );
        let up = self.step(cabin, vec![Self::press(pos, false)]);
        let rest = self.step(cabin, Vec::new());
        let mut all = up;
        all.extend(rest);
        all
    }

    fn confirm_key(&mut self, cabin: &mut Cabin, key: egui::Key) {
        let _ = self.overlay_frame(
            cabin,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }

    /// The confirm Area sizes itself on the first frame and paints on the next.
    fn overlay_texts(&mut self, cabin: &mut Cabin) -> Vec<String> {
        let _ = self.overlay_frame(cabin, Vec::new());
        self.overlay_frame(cabin, Vec::new())
    }

    fn overlay_frame(&mut self, cabin: &mut Cabin, events: Vec<egui::Event>) -> Vec<String> {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 900.0),
            )),
            time: Some(f64::from(self.frame) / 60.0),
            predicted_dt: 1.0 / 60.0,
            events,
            ..Default::default()
        };
        let out = crate::theme::test_pass(&self.ctx, raw, |ui| {
            cabin.paint_confirm_overlay(ui.ctx());
        });
        self.frame += 1;
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            collect_shape_text(&clipped.shape, &mut texts);
        }
        texts
    }
}

fn label_rect(texts: &[(String, egui::Rect)], label: &str) -> egui::Rect {
    texts
        .iter()
        .find(|(t, _)| t == label)
        .map(|(_, r)| *r)
        .unwrap_or_else(|| panic!("missing {label:?} in {texts:?}"))
}

fn count_label(texts: &[(String, egui::Rect)], label: &str) -> usize {
    texts.iter().filter(|(t, _)| t == label).count()
}

// Names keep the brief's case-sensitive filters (`Retry`, `Run`, `View_last`, `Remove`).
#[test]
#[allow(non_snake_case)]
fn failed_scheduled_row_primary_label_is_Retry() {
    assert_eq!(super::night::scheduled_primary_label(true), "Retry");
    assert_eq!(super::night::scheduled_primary_label(false), "Run");
    let src = include_str!("night.rs");
    let page = src
        .split("fn ui_scheduled_automations(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .expect("ui_scheduled_automations");
    assert!(
        page.contains("scheduled_primary_label(health.is_some())")
            && page.contains("job_row_menu")
            && !page.contains("\"Retry\""),
        "the paint asks scheduled_primary_label, which is Retry when failing: {page}"
    );
    let label = src
        .split("fn scheduled_primary_label(")
        .nth(1)
        .and_then(|s| s.split("\nfn ").next())
        .expect("scheduled_primary_label");
    assert!(
        label.contains("\"Retry\"") && label.contains("\"Run\""),
        "{label}"
    );

    let _hold = crate::config::hold_test_config();
    let _paint = crate::theme::hold_paint_test();
    let (_pin, root) = pin_auto_config("auto-retry-label");
    let mut cabin = Cabin::quiet_for_test();
    cabin.automations = vec![failing_price_job(), healthy_morning_job()];
    cabin.grok_loops.clear();
    let mut page = AutoPage::new();
    let texts = page.settle(&mut cabin);
    assert_eq!(count_label(&texts, "Retry"), 1, "{texts:?}");
    assert!(
        count_label(&texts, "Run") >= 1,
        "the healthy row still says Run: {texts:?}"
    );
    assert_eq!(count_label(&texts, "View last run"), 1, "{texts:?}");
    let health = texts
        .iter()
        .position(|(t, _)| t.starts_with("Failed 2 times"));
    let view = texts.iter().position(|(t, _)| t == "View last run");
    assert!(
        health.is_some() && view.is_some() && health < view,
        "{texts:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
    restore_config_env();
}

#[test]
#[allow(non_snake_case)]
fn healthy_scheduled_row_still_says_Run() {
    let _hold = crate::config::hold_test_config();
    let _paint = crate::theme::hold_paint_test();
    let (_pin, root) = pin_auto_config("auto-healthy-run");
    let mut cabin = Cabin::quiet_for_test();
    cabin.automations = vec![healthy_morning_job()];
    cabin.grok_loops.clear();
    let mut page = AutoPage::new();
    let texts = page.settle(&mut cabin);
    assert_eq!(count_label(&texts, "Run"), 1, "{texts:?}");
    assert_eq!(count_label(&texts, "Retry"), 0, "{texts:?}");
    assert_eq!(
        count_label(&texts, "View last run"),
        0,
        "healthy rows have no link: {texts:?}"
    );
    assert_eq!(
        count_label(&texts, "Remove"),
        0,
        "Remove is not an inline pill: {texts:?}"
    );
    assert_eq!(count_label(&texts, "···"), 1, "{texts:?}");
    let _ = std::fs::remove_dir_all(&root);
    restore_config_env();
}

#[test]
#[allow(non_snake_case)]
fn failing_row_paints_View_last_run_and_opens_output() {
    let _hold = crate::config::hold_test_config();
    let _paint = crate::theme::hold_paint_test();
    let (_pin, root) = pin_auto_config("auto-view-last");
    let mut cabin = Cabin::quiet_for_test();
    cabin.nav = super::Nav::Night;
    cabin.automations = vec![failing_price_job()];
    cabin.grok_loops.clear();
    let mut card =
        grokhub_core::BoardCard::new("Sample price check", "Sample error: page did not load", "");
    card.id = "card-price".into();
    card.status = grokhub_core::BoardStatus::FollowUp;
    card.automation = Some("auto-price".into());
    card.report = "Page did not load — last reply".into();
    card.updated_ms = 50;
    cabin.board.push(card);
    let mut page = AutoPage::new();
    let texts = page.settle(&mut cabin);
    let link = label_rect(&texts, "View last run");
    let after = page.click(&mut cabin, link.center());
    assert_eq!(
        cabin.nav,
        super::Nav::Workboard,
        "View last run opens the workboard"
    );
    assert_eq!(cabin.board_view.open.as_deref(), Some("card-price"));
    assert!(
        cabin.status.contains("Last run") && cabin.status.contains("Sample price check"),
        "{}",
        cabin.status
    );
    assert!(
        after.iter().any(|(t, _)| t == "View last run") || cabin.board_view.open.is_some(),
        "the click landed on the link"
    );
    let _ = std::fs::remove_dir_all(&root);
    restore_config_env();
}

#[test]
#[allow(non_snake_case)]
fn View_last_run_opens_background_history_when_there_is_no_card() {
    let _hold = crate::config::hold_test_config();
    let (_pin, root) = pin_auto_config("auto-view-bg");
    let mut cabin = Cabin::quiet_for_test();
    cabin.nav = super::Nav::Night;
    cabin.automations = vec![failing_price_job()];
    let idx = cabin.ensure_background_history_thread();
    let post = grokhub_core::bg_result_post(
        &grokhub_core::bg_task_title(&cabin.automations[0].instructions),
        &grokhub_core::BgEnd::Failed("Sample error: page did not load".into()),
        "The page returned 500.",
    );
    cabin.threads[idx]
        .messages_mut()
        .push(("assistant".into(), post));
    cabin.open_last_scheduled_run(0);
    assert_eq!(cabin.nav, super::Nav::Chat);
    assert_eq!(
        cabin.threads[cabin.thread_idx].title,
        crate::threads::BACKGROUND_THREAD_TITLE
    );
    assert!(
        cabin
            .messages
            .iter()
            .any(|(_, b)| b.contains("The page returned 500.")),
        "the Background chat shows the last reply: {:?}",
        cabin.messages
    );
    assert!(cabin.board_view.open.is_none());
    let _ = std::fs::remove_dir_all(&root);
    restore_config_env();
}

#[test]
#[allow(non_snake_case)]
fn View_last_run_sheet_shows_the_stored_error() {
    let _hold = crate::config::hold_test_config();
    let _paint = crate::theme::hold_paint_test();
    let (_pin, root) = pin_auto_config("auto-view-sheet");
    let mut cabin = Cabin::quiet_for_test();
    cabin.automations = vec![failing_price_job()];
    cabin.open_last_scheduled_run(0);
    match cabin.confirm.clone() {
        Some(super::confirm::ConfirmKind::LastRun { title, body }) => {
            assert_eq!(title, "Sample price check");
            assert!(body.contains("Sample error: page did not load"), "{body}");
        }
        other => panic!("expected a last-run sheet, got {other:?}"),
    }
    let mut page = AutoPage::new();
    let texts = page.overlay_texts(&mut cabin);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Sample error: page did not load")),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t.contains("Last run")), "{texts:?}");
    let _ = std::fs::remove_dir_all(&root);
    restore_config_env();
}

#[test]
#[allow(non_snake_case)]
fn automations_rows_keep_Remove_behind_the_dots_menu() {
    let src = include_str!("night.rs");
    for name in ["ui_scheduled_automations", "ui_night"] {
        let body = src
            .split(&format!("fn {name}("))
            .nth(1)
            .and_then(|s| s.split("\n    fn ").next())
            .unwrap_or_else(|| panic!("missing {name}"));
        assert!(
            !body.contains("ghost_pill(ui, \"Remove\")"),
            "{name} still paints an inline Remove: {body}"
        );
        assert!(
            body.contains("job_row_menu"),
            "{name} should use the ··· row menu: {body}"
        );
    }
    let menu = src
        .split("fn job_row_menu(")
        .nth(1)
        .and_then(|s| s.split("\nimpl ").next())
        .expect("job_row_menu");
    assert!(
        menu.contains("dots_menu")
            && menu.contains("\"Remove\"")
            && menu.contains("job_primary_pill"),
        "{menu}"
    );
    let sched = src
        .split("fn ui_scheduled_automations(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .expect("scheduled");
    let health_at = sched.find("RichText::new(line)").expect("health line");
    let view_at = sched.find("\"View last run\"").expect("view link");
    assert!(
        health_at < view_at,
        "View last run is painted after the health line"
    );
    assert!(sched[view_at..].contains(".size(12.0)"), "{sched}");

    let _hold = crate::config::hold_test_config();
    let _paint = crate::theme::hold_paint_test();
    let (_pin, root) = pin_auto_config("auto-dots-menu");
    let mut cabin = Cabin::quiet_for_test();
    cabin.automations = vec![failing_price_job()];
    cabin.grok_loops = vec![grokhub_core::new_loop(
        "2h".into(),
        "Sample: check the build status and post a short note".into(),
        1,
    )];
    cabin.grok_loops[0].id = "loop-sample".into();
    let mut page = AutoPage::new();
    let texts = page.settle(&mut cabin);
    assert_eq!(
        count_label(&texts, "Remove"),
        0,
        "closed menus hide Remove: {texts:?}"
    );
    assert_eq!(
        count_label(&texts, "···"),
        2,
        "scheduled and loop each have ···: {texts:?}"
    );
    let menu_at = label_rect(&texts, "···");
    let opened = page.click(&mut cabin, menu_at.center());
    assert!(
        count_label(&opened, "Remove") >= 1,
        "··· opens Remove: {opened:?}"
    );
    let remove_at = label_rect(&opened, "Remove");
    let _ = page.click(&mut cabin, remove_at.center());
    match cabin.confirm.clone() {
        Some(super::confirm::ConfirmKind::RemoveJob { title, id, kind }) => {
            assert!(title.contains("Sample"), "{title}");
            assert!(!id.is_empty());
            assert!(
                kind == super::confirm::RemoveJobKind::Scheduled
                    || kind == super::confirm::RemoveJobKind::Loop
            );
        }
        other => panic!("Remove in ··· should arm confirm, got {other:?}"),
    }
    assert_eq!(cabin.automations.len(), 1, "the menu only asks");
    assert_eq!(cabin.grok_loops.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
    restore_config_env();
}

#[test]
#[allow(non_snake_case)]
fn healthy_Run_uses_ghost_pill_and_Retry_stays_white() {
    let src = include_str!("night.rs");
    let pill = src
        .split("fn job_primary_pill(")
        .nth(1)
        .and_then(|s| s.split("\nimpl ").next())
        .expect("job_primary_pill");
    let retry = pill.find("\"Retry\"").expect("Retry branch");
    let white = pill.find("white_pill").expect("filled Retry");
    let ghost = pill.find("ghost_pill").expect("ghost Run");
    assert!(
        retry < white && white < ghost,
        "Retry is filled white; Run falls through to ghost: {pill}"
    );
    let menu = src
        .split("fn job_row_menu(")
        .nth(1)
        .and_then(|s| s.split("fn job_primary_pill(").next())
        .expect("job_row_menu");
    assert!(
        menu.contains("job_primary_pill(ui, primary)"),
        "both lists share the pill helper: {menu}"
    );
    assert!(
        !menu.contains("white_pill") && !menu.contains("ghost_pill"),
        "{menu}"
    );
    let loops = src
        .split("fn ui_night(")
        .nth(1)
        .and_then(|s| s.split("pub(super) fn ui_scheduled_automations(").next())
        .expect("ui_night");
    assert!(
        loops.contains("job_row_menu(ui, \"Run\")"),
        "loops always Run, and the helper makes that a ghost: {loops}"
    );
    let sched = src
        .split("fn ui_scheduled_automations(")
        .nth(1)
        .and_then(|s| s.split("pub(super) fn open_last_scheduled_run(").next())
        .expect("scheduled");
    assert!(
        sched.contains("scheduled_primary_label(health.is_some())")
            && sched.contains("job_row_menu(ui, primary)"),
        "scheduled Retry/Run goes through the same helper: {sched}"
    );
}

#[test]
fn automations_intro_says_loops_repeat_on_an_interval() {
    let src = include_str!("night.rs");
    let page = src
        .split("fn ui_night(")
        .nth(1)
        .and_then(|s| s.split("pub(super) fn ui_scheduled_automations(").next())
        .expect("ui_night");
    let line = "Loops repeat on an interval. Scheduled jobs run at a clock time. Stop a job when its work is done.";
    assert!(page.contains(line), "{page}");
    assert!(
        !page.contains("Interval prompts run as Grok Build"),
        "the intro dropped the jargon: {page}"
    );
}

#[test]
#[allow(non_snake_case)]
fn automations_Suggested_line_is_from_your_recent_work() {
    let src = include_str!("night.rs");
    let page = src
        .split("fn ui_night(")
        .nth(1)
        .and_then(|s| s.split("pub(super) fn ui_scheduled_automations(").next())
        .expect("ui_night");
    let suggested = page
        .split("section_label(ui, \"Suggested\")")
        .nth(1)
        .expect("Suggested");
    assert!(
        suggested.contains("Suggested from your recent work"),
        "{suggested}"
    );
    assert!(
        !page.contains("review_status_line")
            && !page.contains("Review due tonight")
            && !page.contains("Reviewed today"),
        "Suggested no longer uses the nightly review status: {page}"
    );
    let review = include_str!("../../../grokhub-core/src/review.rs");
    assert!(
        review.contains("pub fn review_status_line(")
            && review.contains("\"Review due tonight\"")
            && review.contains("\"Reviewed today\""),
        "night review still has its own status line"
    );
}

#[test]
fn automations_job_rows_use_enabled_switch_not_a_checkbox() {
    let src = include_str!("night.rs");
    let loops = src
        .split("fn ui_night(")
        .nth(1)
        .and_then(|s| s.split("pub(super) fn ui_scheduled_automations(").next())
        .expect("ui_night");
    let sched = src
        .split("fn ui_scheduled_automations(")
        .nth(1)
        .and_then(|s| s.split("pub(super) fn open_last_scheduled_run(").next())
        .expect("scheduled");
    for (name, body) in [("loops", loops), ("scheduled", sched)] {
        assert!(
            !body.contains("checkbox"),
            "{name} still paints a checkbox: {body}"
        );
        assert!(
            body.contains("enabled_switch"),
            "{name} should use the compact switch: {body}"
        );
    }
    assert!(
        loops.contains("persist_loops"),
        "toggling a loop still persists: {loops}"
    );
    assert!(
        sched.contains("toggled = true") && sched.contains("persist_automations"),
        "toggling a scheduled job still refreshes its next slot: {sched}"
    );
    let cards = include_str!("../cards.rs");
    let sw = cards
        .split("pub fn enabled_switch(")
        .nth(1)
        .and_then(|s| s.split("fn paint_switch(").next())
        .expect("enabled_switch");
    assert!(
        sw.contains("28.0")
            && sw.contains("16.0")
            && sw.contains("on_hover_text(\"Enabled\")")
            && sw.contains("finish_switch"),
        "{sw}"
    );
    let finish = cards
        .split("fn finish_switch(")
        .nth(1)
        .and_then(|s| s.split("pub fn switch_access_name(").next())
        .expect("finish_switch");
    assert!(
        finish.contains("switch_access_name") && finish.contains("WidgetType::Checkbox"),
        "the switch keeps an On/Off name: {finish}"
    );
}

#[test]
#[allow(non_snake_case)]
fn automations_page_paints_plain_intro_and_Suggested_from_recent_work() {
    let _hold = crate::config::hold_test_config();
    let _paint = crate::theme::hold_paint_test();
    let (_pin, root) = pin_auto_config("auto-copy");
    let mut cabin = Cabin::quiet_for_test();
    cabin.automations = vec![healthy_morning_job()];
    cabin.grok_loops.clear();
    let mut page = AutoPage::new();
    let texts = page.settle(&mut cabin);
    let blob: String = texts
        .iter()
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        blob.contains("Loops repeat on an interval. Scheduled jobs run at a clock time. Stop a job when its work is done."),
        "{blob}"
    );
    assert!(blob.contains("Suggested from your recent work"), "{blob}");
    assert!(
        !blob.contains("Review due tonight") && !blob.contains("Reviewed today"),
        "{blob}"
    );
    assert!(
        !blob.contains("15s pulse") && !blob.contains("Interval prompts"),
        "{blob}"
    );
    let _ = std::fs::remove_dir_all(&root);
    restore_config_env();
}

#[test]
fn remove_job_confirm_names_the_title_and_confirming_removes() {
    let _hold = crate::config::hold_test_config();
    let _paint = crate::theme::hold_paint_test();
    let (_pin, root) = pin_auto_config("auto-remove-confirm");
    let mut cabin = Cabin::quiet_for_test();
    cabin.automations = vec![failing_price_job(), healthy_morning_job()];
    cabin.grok_loops = vec![grokhub_core::new_loop(
        "2h".into(),
        "Sample: check the build status and post a short note".into(),
        1,
    )];
    cabin.grok_loops[0].id = "loop-sample".into();
    cabin.arm_remove_job(
        super::confirm::RemoveJobKind::Scheduled,
        "auto-price".into(),
        "Sample price check".into(),
    );
    let mut page = AutoPage::new();
    let texts = page.overlay_texts(&mut cabin);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Remove '") && t.contains("Sample price check")),
        "{texts:?}"
    );
    page.confirm_key(&mut cabin, egui::Key::Escape);
    assert!(cabin.confirm.is_none(), "Esc drops the sheet");
    assert_eq!(cabin.automations.len(), 2, "cancel keeps the job");
    assert!(cabin.automations.iter().any(|a| a.id == "auto-price"));

    cabin.arm_remove_job(
        super::confirm::RemoveJobKind::Scheduled,
        "auto-price".into(),
        "Sample price check".into(),
    );
    page.confirm_key(&mut cabin, egui::Key::Enter);
    assert!(cabin.confirm.is_none());
    assert!(
        cabin.automations.iter().all(|a| a.id != "auto-price"),
        "confirming removes the scheduled job"
    );
    assert!(cabin.automations.iter().any(|a| a.id == "auto-morning"));
    assert_eq!(cabin.status, "Automation removed");

    cabin.arm_remove_job(
        super::confirm::RemoveJobKind::Loop,
        "loop-sample".into(),
        "Sample: check the build status and post a short note".into(),
    );
    page.confirm_key(&mut cabin, egui::Key::Escape);
    assert_eq!(cabin.grok_loops.len(), 1, "cancel keeps the loop");
    cabin.arm_remove_job(
        super::confirm::RemoveJobKind::Loop,
        "loop-sample".into(),
        "Sample: check the build status and post a short note".into(),
    );
    page.confirm_key(&mut cabin, egui::Key::Enter);
    assert!(cabin.grok_loops.is_empty(), "confirming removes the loop");
    assert_eq!(cabin.status, "Loop removed");
    let _ = std::fs::remove_dir_all(&root);
    restore_config_env();
}
