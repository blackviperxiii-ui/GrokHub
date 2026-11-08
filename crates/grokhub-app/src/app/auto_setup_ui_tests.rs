use grokhub_core::autotune::{GpuFact, OsLook};

use super::super::setup_wizard::SetupStep;
use super::*;

/// A Surface Pro 7 on battery with Windows text size at 150 %.
fn surface() -> DeviceProfile {
    DeviceProfile {
        os: "windows".into(),
        model: "Surface Pro 7".into(),
        gpus: vec![GpuFact {
            name: "Intel(R) Iris(R) Plus Graphics".into(),
            vendor: "intel".into(),
            vram_mb: 128,
            integrated: true,
            unified: false,
        }],
        ram_mb: 7_936,
        cpu_logical: 8,
        cpu_physical: 4,
        battery: true,
        battery_pct: Some(64),
        free_disk_mb: 60_000,
        look: OsLook {
            text_scale_pct: Some(150),
            ..OsLook::default()
        },
        clock_12h: true,
        ..DeviceProfile::default()
    }
}

/// A 9070 XT desktop with Steam.
fn beast() -> DeviceProfile {
    DeviceProfile {
        os: "linux".into(),
        gpus: vec![GpuFact {
            name: "AMD Radeon RX 9070 XT".into(),
            vendor: "amd".into(),
            vram_mb: 16_368,
            integrated: false,
            unified: false,
        }],
        ram_mb: 63_400,
        cpu_logical: 32,
        cpu_physical: 16,
        on_ac: true,
        steam: true,
        ..DeviceProfile::default()
    }
}

fn cabin_in(name: &str) -> (Cabin, crate::config::TestConfigDir, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("grokhub-auto-setup-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("dir");
    let pin = crate::config::TestConfigDir::set(root.clone());
    (Cabin::quiet_for_test(), pin, root)
}

#[test]
fn every_setting_reads_and_writes_through_the_cabin_config() {
    let (mut app, _pin, root) = cabin_in("map");
    for s in auto_settings::all() {
        assert!(app.get(s.key).is_some(), "{:?}", s.key);
    }
    let cases = [
        (Key::TextSize, Value::Pct(130)),
        (Key::ReduceMotion, Value::Bool(true)),
        (Key::Theme, Value::pick("light")),
        (Key::Theme, Value::pick("system")),
        (Key::Theme, Value::pick("dark")),
        (Key::Effects, Value::Bool(false)),
        (Key::CloseToTray, Value::Bool(false)),
        (Key::PromptCleanup, Value::Bool(true)),
        (Key::QuietHours, Value::Hours(23, 8)),
        (Key::DreamHour, Value::Hour(5)),
        (Key::Notifications, Value::pick("quiet")),
        (Key::Notifications, Value::pick("all")),
        (Key::GameCompanion, Value::Bool(true)),
        (Key::GameNudges, Value::Bool(true)),
        (Key::ResumeCode, Value::Bool(true)),
        (Key::ChatObjectives, Value::Bool(true)),
        (Key::DockPanels, Value::Bool(true)),
        (Key::CollapseThoughts, Value::Bool(true)),
        (Key::Suggestions, Value::Bool(false)),
        (Key::Personality, Value::pick("direct")),
        (Key::Approvals, Value::pick("risky")),
    ];
    for (key, value) in cases {
        assert!(app.set(key, &value), "{key:?}");
        assert_eq!(app.get(key), Some(value), "{key:?}");
    }
    assert_eq!(app.cfg.text_size, 130);
    assert_eq!(
        (app.cfg.quiet_start.as_str(), app.cfg.quiet_end.as_str()),
        ("23:00", "08:00")
    );
    assert_eq!(app.cfg.personality, "concise-pro");
    assert_eq!(app.cfg.permission_mode, "auto");
    assert!(!app.cfg.imagine_wall && !app.cfg.composer_glow);
    assert!(app.cfg.nudge_watchers.steam_patch && app.cfg.nudge_watchers.gpu_driver);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn auto_setup_never_turns_ai_off_or_overrides_a_voice_or_always_approve() {
    let (mut app, _pin, root) = cabin_in("limits");
    app.cfg.local_model = true;
    assert!(!app.set(Key::LocalModel, &Value::Bool(false)));
    assert!(app.cfg.local_model);
    app.cfg.personality = "viper".into();
    assert!(!app.set(Key::Personality, &Value::pick("friendly")));
    assert_eq!(app.cfg.personality, "viper");
    app.cfg.permission_mode = "always-approve".into();
    assert!(!app.set(Key::Approvals, &Value::pick("ask")));
    assert_eq!(app.cfg.permission_mode, "always-approve");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn first_run_on_a_surface_sets_it_up_and_undo_puts_text_back() {
    let (mut app, _pin, root) = cabin_in("surface");
    app.auto_pass_on(surface(), Moment::FirstRun);
    assert_eq!(app.cfg.text_size, 150);
    assert!(app.cfg.reduce_motion);
    assert!(!app.cfg.imagine_wall);
    assert!(app.setup.auto.made.iter().any(|c| c.key == Key::TextSize));
    assert_eq!(change_line(&app.setup.auto.made[0]), "Text size: 150%");
    assert_eq!(app.auto_undo(Key::TextSize), Some(Value::Pct(100)));
    assert_eq!(app.cfg.text_size, 100);
    assert!(!app.setup.auto.made.iter().any(|c| c.key == Key::TextSize));
    // The undone setting is the user's: the next pass leaves it alone.
    app.auto_pass_on(surface(), Moment::Launch);
    assert_eq!(app.cfg.text_size, 100);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn questions_skip_what_the_pc_says_and_a_fast_first_answer_keeps_it_short() {
    let (mut app, _pin, root) = cabin_in("questions");
    app.auto_store().pace = Pace::Detailed;
    app.setup.step = Some(SetupStep::Questions);
    app.auto_next_question(beast());
    assert_eq!(app.setup.auto.question, Some(Qid::Talk));
    app.setup.auto.shown_at = None;
    app.auto_record_answer(Qid::Talk, Some("direct"));
    assert_eq!(app.auto_store().pace, Pace::Quick);
    let mut asked = vec![Qid::Talk];
    for _ in 0..8 {
        app.auto_next_question(beast());
        let Some(q) = app.setup.auto.question else {
            break;
        };
        assert_ne!(q, Qid::Games, "Steam already answers it");
        asked.push(q);
        let first = auto_q_first(q);
        app.auto_record_answer(q, Some(first));
    }
    assert!(asked.len() <= 3, "{asked:?}");
    assert_eq!(app.setup.step, Some(SetupStep::Summary));
    assert_eq!(app.cfg.personality, "concise-pro");
    // The local model stays as it was; the summary offers its setup step.
    assert!(!app.cfg.local_model);
    let _ = std::fs::remove_dir_all(root);
}

fn auto_q_first(id: Qid) -> &'static str {
    grokhub_core::autotune::questions::question(id).choices[0].value
}

#[test]
fn chat_asks_change_text_size_undo_it_and_say_why() {
    let (mut app, _pin, root) = cabin_in("chat");
    let ask = |t: &str| auto_chat::read_setting_ask(t).expect(t);
    assert_eq!(
        app.answer_setting_ask(ask("make the text bigger")),
        "Text size is 115% now."
    );
    assert_eq!(app.cfg.text_size, 115);
    assert_eq!(
        app.answer_setting_ask(ask("undo that")),
        "Text size is 100% now."
    );
    assert_eq!(app.cfg.text_size, 100);
    assert_eq!(
        app.answer_setting_ask(ask("why is the text so big")),
        "Text size is 100% because you set it that way."
    );
    assert_eq!(
        app.answer_setting_ask(ask("undo that")),
        "There's nothing for me to undo."
    );
    assert!(!app.try_setting_ask("make a bigger image"));
    assert!(app.try_setting_ask("dark mode"));
    let last = app.live_mut().last().cloned().expect("reply");
    assert_eq!(last.0, "assistant");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn not_now_rests_an_offer_and_the_text_size_sets_the_zoom() {
    let (mut app, _pin, root) = cabin_in("offer");
    let offer = Change {
        key: Key::GameCompanion,
        from: Some(Value::Bool(false)),
        to: Value::Bool(true),
        rule: "steam_found",
        asked: false,
        reason: "Steam is on this PC.".into(),
    };
    assert_eq!(offer_line(&offer), "Turn on game companion?");
    app.auto_dismiss(&offer);
    assert!(app
        .auto_store()
        .resting("steam_found", grokhub_core::now_ms()));
    app.cfg.text_size = 130;
    let ctx = egui::Context::default();
    app.tick_auto_setup(&ctx);
    // egui takes a new zoom at the next frame.
    let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
    out.textures_delta.clear();
    assert!(
        (ctx.zoom_factor() - 1.3).abs() < 1e-6,
        "{}",
        ctx.zoom_factor()
    );
    let _ = std::fs::remove_dir_all(root);
}
