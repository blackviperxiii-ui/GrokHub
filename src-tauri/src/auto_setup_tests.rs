use grokhub_core::autotune::{GpuFact, OsLook};

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

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-auto-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn no_chats() -> bool {
    false
}

fn some_chats() -> bool {
    true
}

fn tuner<'a>(
    settings: &'a mut ShellSettings,
    session: &'a mut Session,
    dir: &Path,
    device: DeviceProfile,
) -> Tuner<'a> {
    Tuner {
        settings,
        session,
        dir: dir.to_path_buf(),
        has_chats: &no_chats,
        tray: true,
        device: Some(device),
        generation: 1,
    }
}

#[test]
fn the_desktop_settings_read_and_write_and_the_rest_are_not_here() {
    let dir = temp_dir("map");
    let (mut settings, mut session) = (ShellSettings::default(), Session::default());
    let mut t = tuner(&mut settings, &mut session, &dir, beast());
    let cases = [
        (Key::TextSize, Value::Pct(130)),
        (Key::ReduceMotion, Value::Bool(true)),
        (Key::Theme, Value::pick("light")),
        (Key::Theme, Value::pick("dark")),
        (Key::Theme, Value::pick("system")),
        (Key::CloseToTray, Value::Bool(false)),
        (Key::QuietHours, Value::Hours(23, 8)),
        (Key::Notifications, Value::pick("important")),
        (Key::Notifications, Value::pick("quiet")),
        (Key::Notifications, Value::pick("all")),
        (Key::Suggestions, Value::Bool(false)),
        (Key::Personality, Value::pick("direct")),
        (Key::Approvals, Value::pick("risky")),
    ];
    for (key, value) in cases {
        assert!(t.set(key, &value), "{key:?}");
        assert_eq!(t.get(key), Some(value), "{key:?}");
    }
    for key in [
        Key::Effects,
        Key::LocalModel,
        Key::PromptCleanup,
        Key::DreamHour,
        Key::GameCompanion,
        Key::GameNudges,
        Key::ResumeCode,
        Key::ChatObjectives,
        Key::DockPanels,
        Key::CollapseThoughts,
    ] {
        assert_eq!(t.get(key), None, "{key:?}");
        assert!(!t.set(key, &Value::Bool(true)), "{key:?}");
    }
    assert!(!t.set(Key::QuietHours, &Value::Hours(5, 5)));
    t.tray = false;
    assert_eq!(t.get(Key::CloseToTray), None);
    assert!(!t.set(Key::CloseToTray, &Value::Bool(true)));
    assert_eq!(settings.text_size, 130);
    assert_eq!(
        (
            settings.behavior.quiet_start.as_str(),
            settings.behavior.quiet_end.as_str()
        ),
        ("23:00", "08:00")
    );
    assert_eq!(settings.personality, "concise-pro");
    assert_eq!(settings.approvals, Approvals::Auto);
    assert_eq!(
        (settings.notifications, settings.notify_important),
        (true, false)
    );
}

#[test]
fn auto_setup_keeps_a_chosen_voice_and_always_approve() {
    let dir = temp_dir("limits");
    let mut settings = ShellSettings {
        personality: "viper".into(),
        approvals: Approvals::Always,
        ..ShellSettings::default()
    };
    let mut session = Session::default();
    let mut t = tuner(&mut settings, &mut session, &dir, beast());
    assert!(!t.set(Key::Personality, &Value::pick("friendly")));
    assert!(!t.set(Key::Approvals, &Value::pick("ask")));
    assert_eq!(t.get(Key::Personality), Some(Value::pick("viper")));
    assert_eq!(settings.approvals, Approvals::Always);
    // The default voice is stored empty, as the Personality page keeps it.
    let mut fresh = ShellSettings {
        personality: "concise-pro".into(),
        ..ShellSettings::default()
    };
    let mut t = tuner(&mut fresh, &mut session, &dir, beast());
    assert!(t.set(Key::Personality, &Value::pick("friendly")));
    assert_eq!(fresh.personality, "");
}

#[test]
fn first_run_on_a_surface_asks_sets_it_up_and_undo_puts_text_back() {
    let dir = temp_dir("surface");
    let (mut settings, mut session) = (ShellSettings::default(), Session::default());
    let mut t = tuner(&mut settings, &mut session, &dir, surface());
    assert!(t.first_run());
    // Launch passes wait for the first run.
    t.act(Act::Pass { back: false });
    assert_eq!(t.settings.text_size, 100);
    t.act(Act::Start { pace: Pace::Quick });
    let mut asked = 0;
    while let Some(q) = t.reply(None).question {
        asked += 1;
        let first = q.choices[0].value.to_string();
        t.act(Act::Answer {
            id: q.id,
            value: Some(first),
        });
        assert!(asked <= 3, "a quick setup asks three at most");
    }
    let r = t.reply(None);
    assert!(r.summary && r.first_run);
    assert_eq!(r.text_size, 150);
    assert!(r.reduce_motion);
    assert!(
        r.made.iter().any(|l| l.text == "Text size: 150%"),
        "{:?}",
        r.made
    );
    t.act(Act::Undo { key: Key::TextSize });
    assert_eq!(t.settings.text_size, 100);
    assert!(!t.reply(None).made.iter().any(|l| l.key == Key::TextSize));
    t.act(Act::Finish);
    assert!(!t.first_run());
    // The undone setting is the person's: the next pass leaves it alone.
    t.generation = 2;
    t.act(Act::Pass { back: false });
    assert_eq!(t.settings.text_size, 100);
    let r = t.reply(None);
    let row = r.rows.iter().find(|r| r.key == Key::TextSize).expect("row");
    assert_eq!(
        (row.label, row.text.as_str()),
        ("Text size", "100%. You set this.")
    );
    assert!(!row.undo);
    // A new launch reads what this one kept.
    let mut later = Session::default();
    let mut t = tuner(&mut settings, &mut later, &dir, surface());
    assert!(!t.first_run());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_profile_with_chats_skips_the_first_run_and_is_tuned_at_launch() {
    let dir = temp_dir("chats");
    let (mut settings, mut session) = (ShellSettings::default(), Session::default());
    let mut t = tuner(&mut settings, &mut session, &dir, surface());
    t.has_chats = &some_chats;
    assert!(!t.first_run());
    t.act(Act::Pass { back: false });
    assert_eq!(t.settings.text_size, 150);
    let note = t.reply(None).note.expect("a note");
    assert!(!note.offer);
    assert_eq!(note.line.key, Key::TextSize);
    // A note shows once.
    assert_eq!(t.reply(None).note, None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn chat_asks_change_text_size_undo_it_and_say_why() {
    let dir = temp_dir("chat");
    let (mut settings, mut session) = (ShellSettings::default(), Session::default());
    let mut t = tuner(&mut settings, &mut session, &dir, beast());
    t.has_chats = &some_chats;
    let mut say = |text: &str| t.act(Act::Chat { text: text.into() });
    assert_eq!(
        say("make the text bigger").as_deref(),
        Some("Text size is 115% now.")
    );
    assert_eq!(say("undo that").as_deref(), Some("Text size is 100% now."));
    assert_eq!(
        say("why is the text so big").as_deref(),
        Some("Text size is 100% because you set it that way.")
    );
    assert_eq!(
        say("undo that").as_deref(),
        Some("There's nothing for me to undo.")
    );
    assert_eq!(say("make a bigger image"), None);
    assert_eq!(say("dark mode").as_deref(), Some("Theme is dark now."));
    assert_eq!(t.settings.theme, Theme::OpsDark);
    assert_eq!(
        t.act(Act::Set {
            key: Key::TextSize,
            value: Value::Pct(130),
        })
        .as_deref(),
        Some("Text size is 130% now.")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn acts_read_from_the_page() {
    let read = |json: &str| serde_json::from_str::<Act>(json).expect(json);
    assert_eq!(
        read(r#"{"act":"pass","back":true}"#),
        Act::Pass { back: true }
    );
    assert_eq!(
        read(r#"{"act":"start","pace":"detailed"}"#),
        Act::Start {
            pace: Pace::Detailed
        }
    );
    assert_eq!(
        read(r#"{"act":"answer","id":"talk"}"#),
        Act::Answer {
            id: Qid::Talk,
            value: None
        }
    );
    assert_eq!(
        read(r#"{"act":"set","key":"textSize","value":{"t":"pct","v":130}}"#),
        Act::Set {
            key: Key::TextSize,
            value: Value::Pct(130)
        }
    );
    assert_eq!(read(r#"{"act":"handBackAll"}"#), Act::HandBackAll);
}
