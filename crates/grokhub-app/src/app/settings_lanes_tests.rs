//! Settings, lanes, confirm sheets and update tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

// ---- Cursor fold: Settings, lanes, confirm sheets and update ----

// Folded from PR #268.
#[test]
fn cabin_lane_flip_reorders_chips() {
    assert_eq!(persistable_cabin_lane(" life "), "life");
    assert_eq!(persistable_cabin_lane("nope"), "coding");
    assert_eq!(cabin_lane("life"), CabinLane::Life);
    assert_eq!(cabin_lane("Coding"), CabinLane::Coding);

    let shell = QuickChip {
        id: "shell".into(),
        label: "Shell".into(),
        value: "/sh".into(),
        kind: ChipKind::Shell,
        score: 1.0,
        hint: String::new(),
        primary: false,
    };
    let imagine = QuickChip {
        id: "imagine".into(),
        label: "Imagine".into(),
        value: "__nav:imagine".into(),
        kind: ChipKind::Nav,
        score: 1.0,
        hint: String::new(),
        primary: false,
    };
    let mut chips = vec![imagine, shell];
    bias_chips_for_lane(&mut chips, CabinLane::Coding);
    assert_eq!(chips[0].kind, ChipKind::Shell);
    assert_eq!(chips[0].value, "/sh");
    bias_chips_for_lane(&mut chips, CabinLane::Life);
    assert_eq!(chips[0].kind, ChipKind::Nav);
    assert_eq!(chips[0].value, "__nav:imagine");
}

// Folded from PR #269.
#[test]
fn destructive_host_confirm_matches_the_rule() {
    assert!(should_confirm_destructive_host("rm foo.txt"));
    assert!(should_confirm_destructive_host("git push --force"));
    assert!(!should_confirm_destructive_host("ls"));
    assert!(!should_confirm_destructive_host("git push origin"));
    assert!(!should_confirm_destructive_host(
        "cp -a '/home/me/.config/GrokHub/rewind/1/.' '/tmp/proj'"
    ));
}

// Folded from PR #271.
#[test]
fn confirm_keys_follow_the_sheet_rule() {
    assert_eq!(
        confirm_key(true, false, false, false, true),
        Some(ConfirmAct::Confirm)
    );
    assert_eq!(
        confirm_key(false, true, false, false, true),
        Some(ConfirmAct::Cancel)
    );
    assert_eq!(
        confirm_key(true, true, false, false, true),
        Some(ConfirmAct::Cancel)
    );
    assert_eq!(confirm_key(true, false, true, false, true), None);
    assert_eq!(confirm_key(true, false, false, true, true), None);
    assert_eq!(confirm_key(true, false, false, false, false), None);
    assert_eq!(confirm_key(false, false, false, false, true), None);
}

// Folded from PR #274.
#[test]
fn always_confirm_stays_only_for_the_same_ask() {
    // The Always sheet stays up only while the Ask on screen still has the armed rpc id.
    let one = serde_json::json!(1);
    let two = serde_json::json!(2);
    assert!(always_confirm_matches_rpc(Some(&one), &one));
    assert!(!always_confirm_matches_rpc(None, &one));
    assert!(!always_confirm_matches_rpc(Some(&one), &two));
}

// Folded from PR #291.
#[test]
fn work_root_uses_the_home_folder() {
    // HOME is process-wide: share the lock with every test that changes it.
    let _g = crate::config::hold_test_config();
    struct RestoreHome {
        prev: Option<String>,
        #[cfg(windows)]
        prev_profile: Option<std::ffi::OsString>,
    }
    impl Drop for RestoreHome {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(h) => unsafe { std::env::set_var("HOME", h) },
                None => unsafe { std::env::remove_var("HOME") },
            }
            #[cfg(windows)]
            match self.prev_profile.take() {
                Some(p) => unsafe { std::env::set_var("USERPROFILE", p) },
                None => unsafe { std::env::remove_var("USERPROFILE") },
            }
        }
    }

    let app = Cabin::quiet_for_test();
    let home_guard = RestoreHome {
        prev: std::env::var("HOME").ok(),
        #[cfg(windows)]
        prev_profile: std::env::var_os("USERPROFILE"),
    };
    #[cfg(windows)]
    unsafe {
        std::env::remove_var("USERPROFILE")
    };
    let home = std::env::temp_dir().join("grokhub-work-root-1054");
    unsafe { std::env::set_var("HOME", &home) };
    let got = app.work_root();
    #[cfg(not(windows))]
    let expect = {
        let home_s = home.display().to_string().trim_end_matches('/').to_string();
        format!("{home_s}/GrokHub-Work")
    };
    #[cfg(windows)]
    let expect = {
        let h = home
            .display()
            .to_string()
            .trim_end_matches(['/', '\\'])
            .to_string();
        let drive = h.len() >= 2 && h.as_bytes()[1] == b':';
        if drive || h.starts_with('\\') || h.contains('\\') {
            format!("{}\\GrokHub-Work", h.replace('/', "\\"))
        } else {
            format!("{h}/GrokHub-Work")
        }
    };
    assert_eq!(got, expect);
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    drop(home_guard);
}

// Folded from PR #293.
#[test]
fn always_arm_opens_the_sheet() {
    let mut app = Cabin::quiet_for_test();
    app.status = "Harbor".into();
    assert!(app.confirm.is_none());
    app.arm_session_always();
    assert!(matches!(app.confirm, Some(ConfirmKind::AlwaysSession)));
    assert_eq!(app.status, "Confirm Always…");
    assert!(matches!(app.permission_mode, PermissionMode::Ask));
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #342.
#[test]
fn bias_chips_for_lane_orders_coding_first() {
    fn chip(id: &str, label: &str, value: &str, kind: ChipKind) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: label.into(),
            value: value.into(),
            kind,
            score: 1.0,
            hint: String::new(),
            primary: false,
        }
    }

    // Life-leaning first in the input so Coding reorder is falsifiable.
    let mut chips = vec![
        chip("imagine", "Open Imagine", "__nav:imagine", ChipKind::Nav),
        chip(
            "plain",
            "Cabin brief",
            "Give me a short cabin brief.",
            ChipKind::Chat,
        ),
        chip(
            "ship",
            "Ship it",
            "Ship a minimal solid slice.",
            ChipKind::Chat,
        ),
    ];

    super::bias_chips_for_lane(&mut chips, super::CabinLane::Coding);
    assert_eq!(
        chips.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["ship", "plain", "imagine"],
        "Coding lane puts coding-leaning chips first: {chips:?}"
    );

    super::bias_chips_for_lane(&mut chips, super::CabinLane::Life);
    assert_eq!(
        chips.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["imagine", "plain", "ship"],
        "Life lane puts life-leaning chips first: {chips:?}"
    );
}

// Folded from PR #347.
#[test]
fn cabin_update_available_false_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.cabin_latest.is_none());
    assert!(!cabin.cabin_overlay_done);
    // Quiet idle: no probe, no spawn — available stays false.
    assert!(!cabin.cabin_update_available());
}

// Folded from PR #348.
#[test]
fn cabin_signed_in_false_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.secrets.oauth.is_none());
    assert!(!cabin.cabin_signed_in());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #352.
#[test]
fn has_key_false_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.secrets.api_key.trim().is_empty());
    assert!(cabin.cfg.api_key.trim().is_empty());
    assert!(cabin.secrets.oauth.is_none());
    // Quiet cabin with empty secrets/api key → has_auth → false. No network.
    assert!(!cabin.has_key());
}

#[test]
fn account_settings_pulls_disk_oauth_and_shows_connected_chrome() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("acct-oauth-ui");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
    // quiet_for_test: no install/update workers that can outlive the test on Windows.
    let mut cabin = Cabin::quiet_for_test();
    assert!(
        cabin.secrets.oauth.is_none(),
        "quiet cabin starts without oauth in memory"
    );
    // Simulate grokhub --oauth writing secrets while the cabin is already open.
    let tokens = grokhub_core::XaiOAuthTokens {
        access_token: "qa-access".into(),
        name: Some("GrokHub QA".into()),
        email: Some("grokhub-qa@agentmail.to".into()),
        connected_at: 1,
        ..Default::default()
    };
    crate::secrets::save(&crate::secrets::Secrets {
        oauth: Some(tokens),
        ..Default::default()
    })
    .expect("save secrets");
    assert!(
        cabin.secrets.oauth.is_none(),
        "in-memory oauth stays empty until Account pulls disk"
    );
    cabin.pull_account_oauth_from_disk();
    assert!(
        cabin.account_oauth_present(),
        "disk oauth must load into Account"
    );
    let chrome = account_connect_chrome(cabin.secrets.oauth.as_ref(), 2, false);
    assert!(chrome.connected);
    assert_eq!(chrome.action, "Sign out");
    assert_eq!(chrome.title, "Connected");
    assert_eq!(chrome.hint, "GrokHub QA · grokhub-qa@agentmail.to");
    // Signed-out chrome stays for the empty case.
    let signed_out = account_connect_chrome(None, 2, false);
    assert_eq!(signed_out.action, "Sign in with Grok");
    let settings = include_str!("settings.rs");
    let account = settings
        .split("SettingsSec::Account => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::Appearance => {").next())
        .expect("Account");
    assert!(
        account.contains("account_auth.title")
            && account.contains("account_auth.hint")
            && account.contains("account_auth.action")
            && account.contains("account_auth.connected"),
        "Account must paint Connect Grok from account_connect_chrome: {account}"
    );
    assert!(
        settings.contains("pull_account_oauth_from_disk"),
        "Settings must pull secrets.json oauth before painting Account"
    );
    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
}

// Folded from PR #354.
#[test]
fn update_pending_now_none_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.cabin_latest.is_none());
    assert_eq!(
        cabin.update_pending_now(),
        UpdatePending::None,
        "quiet idle cabin with no cli/cabin update flags must report none"
    );
}

// Folded from PR #357.
#[test]
fn open_update_overlay_opens_settings_update() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(matches!(cabin.settings_sec, SettingsSec::Account));
    assert!(cabin.chat_job_thread.is_none());
    cabin.open_update_overlay();
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(matches!(cabin.settings_sec, SettingsSec::Update));
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.queued_overlay.is_none());
}

// Folded from PR #362.
#[test]
fn policy_returns_max_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    // Quiet cabin → Policy::max(). No spawn/network.
    assert_eq!(cabin.policy(), Policy::max());
}

// Folded from PR #363.
#[test]
fn console_key_empty_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.secrets.api_key.trim().is_empty());
    assert!(cabin.cfg.api_key.trim().is_empty());
    // Quiet cabin with empty secrets/api_key → empty string. No network.
    assert_eq!(cabin.console_key(), "");
}

// Folded from PR #369.
#[test]
fn apply_compact_status_sets_compacting_and_done() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);

    cabin.apply_compact_status(true, GrokUsage::default(), None);
    assert_eq!(cabin.status, "Compacting…");
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);

    cabin.apply_compact_status(false, GrokUsage::default(), None);
    assert_eq!(cabin.status, "Compacted");
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);

    cabin.apply_compact_status(false, GrokUsage::default(), Some("disk full".into()));
    assert_eq!(cabin.status, "Compact failed: disk full");
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
}

// Folded from PR #370.
#[test]
fn note_token_budget_noop_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert_eq!(cabin.cfg.daily_token_budget, 0);
    let before = cabin.status.clone();
    // Default usage + budget off → take_budget_note is None → early return, no notify.
    cabin.note_token_budget();
    assert_eq!(cabin.status, before);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #403.
#[test]
fn clear_oauth_photo_drops_cached_avatar_state() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.oauth_photo_key = "https://pbs.twimg.com/profile_images/harbor.jpg".into();
    cabin.oauth_photo_busy = true;
    cabin.oauth_profile_tried = true;
    let (_tx, rx) = std::sync::mpsc::channel::<super::OauthPhotoOut>();
    cabin.oauth_photo_rx = Some(rx);
    assert!(cabin.oauth_photo.is_none());
    assert!(!cabin.host_diff_kick);

    cabin.clear_oauth_photo();

    assert!(cabin.oauth_photo.is_none());
    assert!(cabin.oauth_photo_key.is_empty());
    assert!(cabin.oauth_photo_rx.is_none());
    assert!(!cabin.oauth_photo_busy);
    assert!(!cabin.oauth_profile_tried);
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.host_diff_kick);
}

// Folded from PR #411.
#[test]
fn avatar_chrome_returns_menu_fields() {
    let cabin = Cabin::quiet_for_test();
    let chrome = cabin.avatar_chrome();
    // Quiet cabin: empty display_name, no OAuth name, empty USER.md → fallback "You".
    assert_eq!(chrome.name, "You");
    assert!(chrome.picture_path.is_empty());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #414.
#[test]
fn cabin_default_model_id_empty_and_known() {
    // Real fn: empty/whitespace → ""; known chat id → sanitized non-empty. No spawn/network.
    assert_eq!(cabin_default_model_id(""), "");
    assert_eq!(cabin_default_model_id("  \t"), "");
    let id = cabin_default_model_id("grok-4.7");
    assert!(!id.is_empty());
    assert_eq!(id, grokhub_core::sanitize_chat_model("grok-4.7"));
}

// Folded from PR #416.
#[test]
fn fallback_cabin_name_defaults_to_you() {
    let blank = super::fallback_cabin_name("");
    assert!(!blank.is_empty());
    assert_eq!(blank, "You");

    let from_md = super::fallback_cabin_name("Name: Tester\n");
    assert!(!from_md.is_empty());
    assert_eq!(from_md, "Tester");
}

// Folded from PR #417.
#[test]
fn export_dest_joins_under_config_or_project() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("export-dest");
    let _ = std::fs::remove_dir_all(&root);
    let _pin = crate::config::TestConfigDir::set(root.clone());
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };

    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.project_dir.clear();
    assert_eq!(
        cabin.export_dest("export.md"),
        crate::config::config_dir().join("export.md"),
        "empty project_dir lands under config_dir"
    );

    let project = root.join("harbor-work");
    cabin.cfg.project_dir = project.display().to_string();
    assert_eq!(
        cabin.export_dest("export.html"),
        project.join("export.html"),
        "a bound project_dir joins under that path"
    );

    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
    let _ = std::fs::remove_dir_all(&root);
}

// Folded from PR #418.
#[test]
fn always_session_spec_title_primary_not_danger() {
    let spec = always_session_spec();
    assert_eq!(spec.title, ALWAYS_SESSION_TITLE);
    assert_eq!(spec.primary, "Confirm");
    assert!(!spec.danger);
}

// Folded from PR #420.
#[test]
fn cabin_default_model_label_empty_unknown_and_known() {
    // Real fn: empty/unknown pin → "Auto"; known catalog id → its label. No spawn/network.
    assert_eq!(cabin_default_model_label(""), "Auto");
    assert_eq!(cabin_default_model_label("  \t"), "Auto");
    assert_eq!(cabin_default_model_label("grok-4.7"), "Grok 4.7");
}

#[test]
fn native_skills_hooks_settings_listing_comes_from_discovery() {
    let pages = include_str!("pages.rs");
    let engine = include_str!("native_engine.rs");
    let skills = fn_src(pages, "ui_skills");
    let hooks = fn_src(pages, "ui_hooks_section");
    assert!(skills.contains("ensure_native_listing"), "{skills}");
    assert!(skills.contains("native_skills"), "{skills}");
    assert!(!skills.contains("reload_grok_catalog"), "{skills}");
    assert!(hooks.contains("native_hooks"), "{hooks}");
    assert!(hooks.contains("ensure_native_listing"), "{hooks}");
    assert!(engine.contains("discover_skills"), "{engine}");
    assert!(engine.contains("discover_hooks"), "{engine}");
    assert!(!engine.contains("inspect"), "{engine}");
}

#[test]
fn native_auto_keeps_the_permission_card() {
    let acp = include_str!("acp.rs");
    let poll = acp
        .split("fn poll_acp(")
        .nth(1)
        .and_then(|src| src.split("fn finish_acp_turn").next())
        .expect("poll_acp");
    assert!(poll.contains("auto_allows()"), "{poll}");
    assert!(poll.contains("starts_with(\"native-\")"), "{poll}");
    assert!(poll.contains("&& !native_session"), "{poll}");
    let engine = include_str!("native_engine.rs");
    assert!(engine.contains("ExternalCmd::Permission"), "{engine}");
    let kick = fn_src(engine, "kick_native_turn");
    assert!(kick.contains("side_ask_kick = false"), "{kick}");
}

#[test]
fn signin_button_and_keychain_move() {
    let settings = include_str!("settings.rs");
    let oauth = include_str!("oauth.rs");
    assert!(
        settings.contains("account_chrome_now")
            && settings.contains("pull_account_oauth_from_disk")
            && oauth.contains("Sign in with Grok")
            && oauth.contains("Sign out"),
        "Account Connect Grok chrome must pull disk oauth and paint Sign in / Sign out"
    );
    assert!(settings.contains("Connect Grok") || oauth.contains("Connect Grok"));

    struct Mem {
        current: std::sync::Mutex<Option<grokhub_core::ImagineTokens>>,
        legacy: std::sync::Mutex<Option<grokhub_core::ImagineTokens>>,
    }
    impl grokhub_core::OAuthAccountStore for Mem {
        fn load_account(
            &self,
            account: &str,
        ) -> Result<Option<grokhub_core::ImagineTokens>, String> {
            let slot = if account == grokhub_core::XAI_OAUTH_ACCOUNT {
                &self.current
            } else if account == grokhub_core::XAI_OAUTH_LEGACY_ACCOUNT {
                &self.legacy
            } else {
                return Ok(None);
            };
            Ok(slot.lock().unwrap_or_else(|err| err.into_inner()).clone())
        }
        fn save_account(
            &self,
            account: &str,
            tokens: &grokhub_core::ImagineTokens,
        ) -> Result<(), String> {
            if account == grokhub_core::XAI_OAUTH_ACCOUNT {
                *self.current.lock().unwrap_or_else(|err| err.into_inner()) = Some(tokens.clone());
                return Ok(());
            }
            Err("unexpected account".into())
        }
        fn delete_account(&self, account: &str) -> Result<(), String> {
            let slot = if account == grokhub_core::XAI_OAUTH_ACCOUNT {
                &self.current
            } else {
                &self.legacy
            };
            *slot.lock().unwrap_or_else(|err| err.into_inner()) = None;
            Ok(())
        }
    }
    let legacy = grokhub_core::ImagineTokens {
        access_token: "moved".into(),
        refresh_token: Some("r".into()),
        ..grokhub_core::ImagineTokens::default()
    };
    let store = Mem {
        current: std::sync::Mutex::new(None),
        legacy: std::sync::Mutex::new(Some(legacy)),
    };
    let loaded = grokhub_core::load_xai_oauth(&store).unwrap().unwrap();
    assert_eq!(loaded.access_token, "moved");
    assert!(store.legacy.lock().unwrap().is_none());
    assert!(store.current.lock().unwrap().is_some());
}

// Folded from PR #426.
#[test]
fn destructive_host_spec_title_primary_is_danger() {
    let spec = destructive_host_spec();
    assert_eq!(spec.title, HOST_CONFIRM_TITLE);
    assert_eq!(spec.primary, HOST_CONFIRM_PRIMARY);
    assert!(spec.danger);
}

// Folded from PR #427.
#[test]
fn cabin_default_sessions_lists_known_ids() {
    // Real fn: non-empty session list with known ids. No spawn/network.
    let sessions = cabin_default_sessions();
    assert!(!sessions.is_empty());
    let ids: Vec<&str> = sessions.iter().map(|(id, _)| *id).collect();
    for want in ["chat", "plan", "ask"] {
        assert!(ids.contains(&want), "missing session id {want}: {ids:?}");
    }
}

// Folded from PR #428.
#[test]
fn cabin_default_permissions_lists_known_ids() {
    // Real fn: non-empty permission ladder with known ids. No spawn/network.
    let perms = cabin_default_permissions();
    assert!(!perms.is_empty());
    let ids: Vec<&str> = perms.iter().map(|(id, _)| *id).collect();
    for want in ["ask", "auto"] {
        assert!(ids.contains(&want), "missing permission id {want}: {ids:?}");
    }
}

// Folded from PR #429.
#[test]
fn cabin_default_permission_id_empty_unknown_and_known() {
    // Real fn: empty/unknown → ask; known ask/auto stay. No spawn/network.
    assert_eq!(cabin_default_permission_id(""), "ask");
    assert_eq!(cabin_default_permission_id("nope"), "ask");
    assert_eq!(cabin_default_permission_id("ask"), "ask");
    assert_eq!(cabin_default_permission_id("auto"), "auto");
}

// Folded from PR #430.
#[test]
fn cabin_default_session_id_empty_unknown_and_known() {
    // Real fn: empty/unknown → "chat"; known ids stay. No spawn/network.
    assert_eq!(cabin_default_session_id(""), "chat");
    assert_eq!(cabin_default_session_id("nonsense"), "chat");
    assert_eq!(cabin_default_session_id("chat"), "chat");
    assert_eq!(cabin_default_session_id("plan"), "plan");
    assert_eq!(cabin_default_session_id("ask"), "ask");
}

// Folded from PR #431.
#[test]
fn cabin_default_models_lists_known_ids() {
    // Real fn: non-empty chat catalog plus Auto with known ids/labels. No spawn/network.
    let models = cabin_default_models();
    assert!(!models.is_empty());
    let ids: Vec<&str> = models.iter().map(|(id, _)| *id).collect();
    let labels: Vec<&str> = models.iter().map(|(_, label)| *label).collect();
    assert!(ids.contains(&""), "missing Auto id: {ids:?}");
    assert!(labels.contains(&"Auto"), "missing Auto label: {labels:?}");
    for (want_id, want_label) in [
        ("grok-4.7", "Grok 4.7"),
        ("grok-4.6", "Grok 4.6"),
        ("grok-4.3", "Grok 4.3"),
        ("grok-3-mini-fast", "Grok 3 Mini Fast"),
    ] {
        assert!(
            ids.contains(&want_id),
            "missing model id {want_id}: {ids:?}"
        );
        assert!(
            labels.contains(&want_label),
            "missing model label {want_label}: {labels:?}"
        );
    }
}
