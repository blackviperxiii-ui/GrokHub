//! Profile, avatar and devices tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

#[test]
fn avatar_menu_hides_email_and_uses_saved_name_and_picture() {
    let email = "user@example.com";
    let picture = "/home/user/.config/GrokHub/profile.png";
    let menu = super::avatar_menu("Tester", picture, Some("OAuth Name"), "Name: Tester\n");
    assert_eq!(menu.name, "Tester");
    assert_eq!(menu.picture_path, picture);
    assert!(!menu.name.contains(email) && !menu.picture_path.contains(email));
    assert!(!menu.name.contains('@') && !menu.picture_path.contains('@'));

    let fallback = super::avatar_menu("", "", Some("OAuth Name"), "");
    assert_eq!(fallback.name, "OAuth Name");
    assert!(fallback.picture_path.is_empty());

    let blank = super::avatar_menu("   ", "", Some("GrokHub"), "Name: Tester\n");
    assert_eq!(blank.name, "Tester");
    assert!(!blank.name.contains(email));

    let src = cabin_src();
    let paint = fn_src(&src, "ui_settings_menu");
    assert!(
        paint.contains("chrome.name")
            && paint.contains("chrome.picture_path")
            && paint.contains("cabin_avatar")
            && !paint.contains("email"),
        "the avatar menu must paint the saved name and picture path, not the email: {paint}"
    );
    let avatar = src
        .split("fn cabin_avatar(")
        .nth(1)
        .and_then(|s| s.split("fn ui_sidebar(").next())
        .expect("cabin_avatar");
    assert!(
        !avatar.contains("email"),
        "the rail avatar must not paint an email line: {avatar}"
    );
    let account = src
        .split("SettingsSec::Account => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::Appearance => {").next())
        .expect("Account");
    assert!(
        account.contains("display_name")
            && account.contains("Profile picture")
            && !account.contains("email")
            && !account.contains(".email"),
        "Account sets the name and picture and does not show the email: {account}"
    );
    let pick = src
        .split("fn pick_profile_picture(")
        .nth(1)
        .and_then(|s| s.split("fn clear_profile_picture(").next())
        .expect("pick_profile_picture");
    let spawn = pick
        .find("thread::spawn")
        .expect("picker leaves the UI thread");
    let file = pick.find("pick_file()").expect("native picker");
    let install = pick
        .find("install_profile_picture")
        .expect("copy into cabin config");
    assert!(
        spawn < file && file < install,
        "the picture picker and decode must not run on the UI thread: {pick}"
    );
}

#[test]
fn remove_ignores_an_in_flight_picture_pick() {
    let started = 1_u64;
    let cleared = super::next_pick_token(started);
    assert!(super::profile_pick_current(started, started));
    assert!(!super::profile_pick_current(cleared, started));
    assert!(!super::profile_pick_current(0, 0));
    let wrapped = super::next_pick_token(u64::MAX);
    assert_eq!(wrapped, 1);
    assert!(super::profile_pick_current(wrapped, wrapped));

    let src = cabin_src();
    let clear = src
        .split("fn clear_profile_picture(")
        .nth(1)
        .and_then(|s| s.split("fn poll_profile_pick(").next())
        .expect("clear_profile_picture");
    assert!(
        clear.contains("profile_pick_rx = None") && clear.contains("next_pick_token"),
        "Remove must drop the in-flight pick and bump its token: {clear}"
    );
    let poll = src
        .split("fn poll_profile_pick(")
        .nth(1)
        .and_then(|s| s.split("fn poll_profile_photo(").next())
        .expect("poll_profile_pick");
    let gate = poll.find("profile_pick_current").expect("token gate");
    let apply = poll.find("profile_picture").expect("writes the path");
    assert!(
        gate < apply,
        "a late Chosen must be ignored before it restores the path: {poll}"
    );
    let pick = src
        .split("fn pick_profile_picture(")
        .nth(1)
        .and_then(|s| s.split("fn clear_profile_picture(").next())
        .expect("pick_profile_picture");
    let file = pick.find("pick_file()").expect("native picker");
    let install = pick
        .find("install_profile_picture")
        .expect("copy into cabin config");
    let before = pick.find("profile_pick_current").expect("cancel check");
    assert!(
        file < before && before < install,
        "Remove must cancel the pick before it installs a picture: {pick}"
    );
}

/// A writer that already holds the disk lock samples the slot only after a newer
/// publish. The older generation must not be what gets saved.
fn stale_after_newer_publish(older: super::AppConfig, newer: super::AppConfig) -> super::AppConfig {
    use std::sync::{Arc, Barrier, Mutex};
    let slot = Arc::new(Mutex::new(super::CfgSlot {
        gen: 0,
        cfg: super::AppConfig::default(),
    }));
    let io = Arc::new(Mutex::new(()));
    let older_gen = {
        let mut g = slot.lock().expect("slot");
        super::publish_cfg(&mut g, older)
    };
    let held = io.lock().expect("disk");
    let slot_w = Arc::clone(&slot);
    let io_w = Arc::clone(&io);
    let gate = Arc::new(Barrier::new(2));
    let gate_w = Arc::clone(&gate);
    let worker = std::thread::spawn(move || {
        let _disk = io_w.lock().expect("disk");
        gate_w.wait();
        let g = slot_w.lock().expect("slot");
        super::cfg_if_current(&g, older_gen)
    });
    let newer_gen = {
        let mut g = slot.lock().expect("slot");
        super::publish_cfg(&mut g, newer)
    };
    drop(held);
    gate.wait();
    assert!(worker.join().expect("writer").is_none());
    let g = slot.lock().expect("slot");
    super::cfg_if_current(&g, newer_gen).expect("newer config")
}

#[test]
fn name_save_does_not_overwrite_a_newer_picture() {
    let name = super::AppConfig {
        display_name: "Tester".into(),
        api_key: "sk-secret".into(),
        ..super::AppConfig::default()
    };
    let picture = super::AppConfig {
        display_name: "Tester".into(),
        profile_picture: "/cfg/profile.png".into(),
        ..super::AppConfig::default()
    };
    let current = stale_after_newer_publish(name, picture);
    assert_eq!(current.display_name, "Tester");
    assert_eq!(current.profile_picture, "/cfg/profile.png");
    assert!(current.api_key.is_empty());

    let src = cabin_src();
    let persist = fn_src(&src, "persist_cfg");
    assert!(
        persist.contains("publish_cfg")
            && persist.contains("submit_settings")
            && persist.contains("persist_svc")
            && persist.contains("api_key.clear")
            && !persist.contains("thread::spawn"),
        "a config save must schedule PersistService with the latest generation: {persist}"
    );
    let worker = fn_src(&src, "persist_service_worker");
    let check = worker.find("cfg_if_current").expect("generation check");
    let save = worker.find("config::save").expect("save");
    assert!(
        worker.contains("io.lock()") && check < save,
        "PersistService must write the latest generation under persist_io: {worker}"
    );
}

#[test]
fn picture_save_does_not_overwrite_a_newer_name() {
    let picture = super::AppConfig {
        display_name: "Old".into(),
        profile_picture: "/cfg/profile.png".into(),
        ..super::AppConfig::default()
    };
    let name = super::AppConfig {
        display_name: "Tester".into(),
        profile_picture: "/cfg/profile.png".into(),
        ..super::AppConfig::default()
    };
    let current = stale_after_newer_publish(picture, name);
    assert_eq!(current.display_name, "Tester");
    assert_eq!(current.profile_picture, "/cfg/profile.png");
}

#[test]
fn devices_pair_url_is_not_a_placeholder() {
    let url = super::discover_hub_pair_url(18766);
    assert!(url.starts_with("http://"), "{url}");
    assert!(url.contains(":18766"), "{url}");
    assert!(!url.contains("<lan>"), "{url}");
}

#[test]
fn devices_lan_address_does_not_need_hostname_i() {
    // No default route (offline CI) is None; anything else is a real IPv4.
    if let Some(ip) = super::routed_ipv4() {
        assert!(ip.parse::<std::net::Ipv4Addr>().is_ok(), "{ip}");
    }
    let src = cabin_src();
    let now = src
        .split("fn hostname_i_now()")
        .nth(1)
        .and_then(|s| s.split("\nfn kick_hostname(").next())
        .expect("hostname_i_now");
    let pick = now.find("pick_lan_ipv4").expect("LAN pick");
    let routed = now.find("routed_ipv4()").expect("routed fallback");
    assert!(
        pick < routed,
        "Windows and macOS have no `hostname -I`; Devices must still show a LAN address: {now}"
    );
    // Windows reads its adapters instead of running a `hostname -I` that always fails.
    let unix = src
        .split("#[cfg(not(windows))]\nfn lan_candidates_now()")
        .nth(1)
        .and_then(|s| s.split("\n}\n").next())
        .expect("unix lan_candidates_now");
    assert!(unix.contains(r#"cmd.arg("-I")"#), "{unix}");
    let win = src
        .split("#[cfg(windows)]\nfn lan_candidates_now()")
        .nth(1)
        .and_then(|s| s.split("\n}\n").next())
        .expect("windows lan_candidates_now");
    assert!(
        win.contains("win_native::adapter_ipv4s()") && !win.contains("Command::new"),
        "{win}"
    );
}

#[test]
fn devices_hostname_must_not_block_the_ui() {
    let src = cabin_src();
    let host = src
        .split("fn hostname_i()")
        .nth(1)
        .and_then(|s| s.split("\nfn discover_hub_pair_url(").next())
        .expect("hostname_i");
    assert!(
        host.contains("run_limited("),
        "hostname -I on Devices paint must time out: {host}"
    );
    assert!(
        host.contains("thread::spawn") && host.contains("inflight"),
        "stale hostname -I must refresh off the UI thread: {host}"
    );
    assert!(
        !host.contains(".output()"),
        "hostname -I must not block Devices paint: {host}"
    );
    let disc = src
        .split("fn discover_hub_pair_url(")
        .nth(1)
        .and_then(|s| s.split("\n#[cfg(test)]").next())
        .expect("discover_hub_pair_url");
    assert!(
        disc.contains("hostname_i()"),
        "Devices pair URL must use the timed hostname helper: {disc}"
    );
}

#[test]
fn clear_profile_picture_removes_the_saved_picture() {
    let mut quiet = QuietCabin::boot("clear-picture");
    let dest = quiet.boot.root.join("profile.png");
    let cabin = &mut quiet.cabin;
    std::fs::write(&dest, b"not-a-real-png").expect("picture");
    cabin.cfg.profile_picture = dest.display().to_string();
    cabin.clear_profile_picture();
    assert!(cabin.cfg.profile_picture.is_empty());
    assert_eq!(cabin.status, "Saved");
    assert!(cabin.profile_photo.is_none());
    wait_for("Remove must delete profile.png", || !dest.exists());
    quiet.settle();
}

// Landed from PR #173.
#[test]
fn clear_profile_picture_saves_without_a_dialog() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("clear-profile");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.cfg.profile_picture = "profile.png".into();
    cabin.clear_profile_picture();

    assert!(cabin.cfg.profile_picture.is_empty());
    assert!(cabin.profile_photo.is_none());
    assert_eq!(cabin.status, "Saved");
    assert!(!cabin.running);
}

// Landed from PR #225.
#[test]
fn profile_name_clips_at_sixty_four() {
    assert_eq!(super::clip_profile_name("  Ada  "), "Ada");
    assert_eq!(super::clip_profile_name("   "), "");
    let long: String = "a".repeat(70);
    let clipped = super::clip_profile_name(&long);
    assert_eq!(clipped.chars().count(), 64);
    assert_eq!(clipped, "a".repeat(64));
}


#[test]
fn devices_signed_out_send_task_shows_explanation_and_signin_link() {
    let mut quiet = QuietCabin::boot("devices-signed-out");
    let cabin = &mut quiet.cabin;
    assert!(!cabin.has_key());
    cabin.set_nav_id("devices");
    assert_eq!(cabin.nav, Nav::Devices);

    let src = cabin_src();
    let devices = fn_src(&src, "ui_devices");
    assert!(
        devices.contains("has_key()")
            && devices.contains("Sign in with Grok in Settings")
            && devices.contains("set_nav_id(\"settings\")"),
        "Devices page must disable task send when signed out with a reason and sign-in link: {devices}"
    );
}

#[test]
fn session_menu_hidden_when_no_session() {
    let mut quiet = QuietCabin::boot("no-session-menu");
    let cabin = &mut quiet.cabin;
    assert!(cabin.messages.is_empty());
    assert!(cabin.threads[cabin.thread_idx]
        .grok_session
        .as_deref()
        .unwrap_or("")
        .is_empty());

    let ctx = egui::Context::default();
    let raw = egui::RawInput::default();
    let mut menu_rect = egui::Rect::EVERYTHING;
    let _ = crate::theme::test_pass(&ctx, raw, |ui| {
        menu_rect = cabin.paint_session_actions_menu(ui);
    });
    assert_eq!(
        menu_rect,
        egui::Rect::NOTHING,
        "the session menu must be hidden when there is no active session"
    );
}
