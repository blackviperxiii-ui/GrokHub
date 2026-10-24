use super::pages::BoardAct;
use super::*;
use eframe::egui;
use grokhub_core::ChatRunPhase;
use grokhub_core::UpdateAction;
use grokhub_core::REVIEW_NIGHT_HOUR;
use grokhub_core::{ProjectKind, ProjectNode};
use grokhub_core::{UpdateCard, UpdateKind, UpdateStatus};
use std::sync::Arc;

#[path = "test_support_tests.rs"]
mod test_support_tests;
use test_support_tests::*;

#[path = "composer_attach_tests.rs"]
mod composer_attach_tests;

#[path = "background_poll_tests.rs"]
mod background_poll_tests;

#[path = "local_indexer_tests.rs"]
mod local_indexer_tests;

#[path = "pulse_page_tests.rs"]
mod pulse_page_tests;

#[path = "background_run_tests.rs"]
mod background_run_tests;

#[path = "self_work_tests.rs"]
mod self_work_tests;

#[path = "step_search_tests.rs"]
mod step_search_tests;

#[path = "board_ideas_tests.rs"]
mod board_ideas_tests;

#[path = "idle_chat_state_tests.rs"]
mod idle_chat_state_tests;

fn cabin_src() -> String {
    // Windows CI checkouts often embed CRLF via include_str; source-inspection
    // asserts match on \n, so normalize once here.
    concat!(
        include_str!("mod.rs"),
        include_str!("persist.rs"),
        include_str!("persist_service.rs"),
        include_str!("acp.rs"),
        include_str!("chat_kick.rs"),
        include_str!("palette.rs"),
        include_str!("settings.rs"),
        include_str!("plus.rs"),
        include_str!("projects.rs"),
        include_str!("slash.rs"),
        include_str!("oauth.rs"),
        include_str!("imagine.rs"),
        include_str!("night.rs"),
        include_str!("chat_ui.rs"),
        include_str!("sidebar.rs"),
        include_str!("pages.rs"),
        include_str!("connectors_ui.rs"),
        include_str!("marketplace_ui.rs"),
        include_str!("jobs.rs"),
        include_str!("chips.rs"),
        include_str!("voice.rs"),
        include_str!("threads_nav.rs"),
        include_str!("ideas_ui.rs"),
        include_str!("board_ui.rs"),
        include_str!("background.rs"),
    )
    .replace("\r\n", "\n")
    .replace("pub(super) ", "")
}

/// One `fn name(` body in the concatenated cabin, stopping at the next same-indent fn.
fn fn_src<'a>(src: &'a str, name: &str) -> &'a str {
    let needle = format!("fn {name}(");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("missing fn {name}"));
    let after = &src[start..];
    let rest = &after[needle.len()..];
    let end = rest
        .find("\n    fn ")
        .or_else(|| rest.find("\nfn "))
        .unwrap_or(rest.len());
    &after[..needle.len() + end]
}

#[test]
fn cabin_src_strips_crlf_so_windows_checkout_matches_lf_asserts() {
    // Simulate a Windows autocrlf checkout: the Halt-guard snippet must still
    // match the LF needle used by heartbeat_halt_skips_every_organ_that_starts_work.
    let crlf = "if !halted {\r\n                        self.follow_feed_lookup();";
    let norm = crlf.replace("\r\n", "\n");
    assert!(
        norm.contains("if !halted {\n                        self.follow_feed_lookup();"),
        "CRLF must normalize before the Halt-digest assert"
    );
    assert!(
        !cabin_src().contains('\r'),
        "cabin_src must not leave CR for source-inspection tests"
    );
}

#[test]
fn avatar_menu_hides_email_and_uses_saved_name_and_picture() {
    let email = "jeremy@example.com";
    let picture = "/home/jeremy/.config/GrokHub/profile.png";
    let menu = super::avatar_menu("Viper", picture, Some("OAuth Name"), "Name: Jeremy\n");
    assert_eq!(menu.name, "Viper");
    assert_eq!(menu.picture_path, picture);
    assert!(!menu.name.contains(email) && !menu.picture_path.contains(email));
    assert!(!menu.name.contains('@') && !menu.picture_path.contains('@'));

    let fallback = super::avatar_menu("", "", Some("OAuth Name"), "");
    assert_eq!(fallback.name, "OAuth Name");
    assert!(fallback.picture_path.is_empty());

    let blank = super::avatar_menu("   ", "", Some("GrokHub"), "Name: Jeremy\n");
    assert_eq!(blank.name, "Jeremy");
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
        display_name: "Viper".into(),
        api_key: "sk-secret".into(),
        ..super::AppConfig::default()
    };
    let picture = super::AppConfig {
        display_name: "Viper".into(),
        profile_picture: "/cfg/profile.png".into(),
        ..super::AppConfig::default()
    };
    let current = stale_after_newer_publish(name, picture);
    assert_eq!(current.display_name, "Viper");
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
        display_name: "Viper".into(),
        profile_picture: "/cfg/profile.png".into(),
        ..super::AppConfig::default()
    };
    let current = stale_after_newer_publish(picture, name);
    assert_eq!(current.display_name, "Viper");
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
fn rename_focus_selects_the_placeholder() {
    egui::__run_test_ui(|ui| {
        let mut buf = String::from("Project");
        let edit = ui.add(egui::TextEdit::singleline(&mut buf));
        select_all_edit(ui, edit.id, &buf);
        let state = egui::TextEdit::load_state(ui.ctx(), edit.id).expect("edit state");
        let range = state.cursor.char_range().expect("selection");
        let [a, b] = range.sorted_cursors();
        assert_eq!(a.index, egui::text::CharIndex(0));
        assert_eq!(b.index, egui::text::CharIndex(7));
    });
}

#[test]
fn short_assistant_bubble_hugs_the_text() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 200.0), |ui| {
            ui.set_max_width(800.0);
            let resp = super::paint_speech_bubble(ui, "Hi", false, false);
            assert!(
                resp.rect.width() < 200.0,
                "short assistant bubble stretched to {}",
                resp.rect.width()
            );
            assert!(resp.rect.width() > 24.0);
        });
    });
}

#[test]
fn short_user_bubble_hugs_the_text() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 200.0), |ui| {
            ui.set_max_width(800.0);
            let resp = super::paint_speech_bubble(ui, "Hi", true, false);
            assert!(
                resp.rect.width() < 200.0,
                "short bubble stretched to {}",
                resp.rect.width()
            );
            assert!(resp.rect.width() > 24.0);
            assert!(resp.rect.height() > 20.0);
        });
    });
}

fn with_cabin_theme_ui(mut add: impl FnMut(&mut egui::Ui)) {
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        let ctx = ui.ctx().clone();
        crate::theme::apply(&ctx, true);
        egui::CentralPanel::default().show(ui, |ui| add(ui));
    });
}

#[test]
fn short_user_reply_stays_inside_the_viewport() {
    // Cabin button padding makes Copy+Reply wider than a "hey" bubble.
    // The row used to floor the lead at 96px and draw Reply past the pane.
    for width in [360.0_f32, 480.0, 800.0, 1100.0] {
        with_cabin_theme_ui(|ui| {
            ui.allocate_ui(egui::vec2(width, 280.0), |ui| {
                ui.set_max_width(width);
                let row = ui.max_rect();
                assert!(
                    (row.width() - width).abs() < 1.0,
                    "harness row {} != requested {width}",
                    row.width()
                );
                let bubble = super::paint_speech_bubble(ui, "hey", true, false);
                let acts = super::paint_msg_acts(ui, true, "hey", row.width(), bubble.rect.width());
                assert!(
                    bubble.rect.max.x <= row.max.x + 1.0,
                    "width {width}: bubble past the right {} > {}",
                    bubble.rect.max.x,
                    row.max.x
                );
                assert!(
                    bubble.rect.min.x >= row.min.x - 0.5,
                    "width {width}: bubble past the left {} < {}",
                    bubble.rect.min.x,
                    row.min.x
                );
                assert!(
                    bubble.rect.min.y >= row.min.y - 0.5,
                    "width {width}: bubble past the top {} < {}",
                    bubble.rect.min.y,
                    row.min.y
                );
                assert!(
                    acts.row.width() > 40.0 && acts.row.height() > 8.0,
                    "width {width}: Copy/Reply row missing {:?}",
                    acts.row
                );
                assert!(
                    acts.row.max.x <= row.max.x + 1.0,
                    "width {width}: Reply past the right {} > {} (row w {})",
                    acts.row.max.x,
                    row.max.x,
                    acts.row.width()
                );
                assert!(
                    acts.row.min.x >= row.min.x - 0.5,
                    "width {width}: Copy past the left {} < {}",
                    acts.row.min.x,
                    row.min.x
                );
                assert!(
                    acts.row.min.y >= row.min.y - 0.5,
                    "width {width}: action row past the top {} < {}",
                    acts.row.min.y,
                    row.min.y
                );
                assert!(
                    acts.row.max.x <= bubble.rect.max.x + 1.0,
                    "width {width}: Reply extends past the bubble {} > {}",
                    acts.row.max.x,
                    bubble.rect.max.x
                );
                assert!(
                    (bubble.rect.max.x - row.max.x).abs() < 8.0,
                    "width {width}: short user bubble left the right edge, bubble {} row {}",
                    bubble.rect.max.x,
                    row.max.x
                );
            });
        });
    }
    with_cabin_theme_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 420.0), |ui| {
            ui.set_max_width(800.0);
            let row = ui.max_rect();
            let body = "word ".repeat(48);
            let bubble = super::paint_speech_bubble(ui, &body, true, false);
            let acts = super::paint_msg_acts(ui, true, &body, row.width(), bubble.rect.width());
            assert!(
                bubble.rect.height() > 36.0,
                "long user text must still wrap, height {}",
                bubble.rect.height()
            );
            assert!(
                bubble.rect.max.x <= row.max.x + 1.0 && acts.row.max.x <= row.max.x + 1.0,
                "long user row left the pane bubble {} acts {} row {}",
                bubble.rect.max.x,
                acts.row.max.x,
                row.max.x
            );
            assert!(
                (acts.row.min.x - bubble.rect.min.x).abs() < 4.0,
                "wide bubble actions must stay under the bubble, acts {} bubble {}",
                acts.row.min.x,
                bubble.rect.min.x
            );
        });
    });
}

fn row_ends_with_short_fragment(row: &str, words: &[&str]) -> Option<String> {
    let last = row.split_whitespace().last().unwrap_or("");
    let letters = last.chars().count();
    if letters == 0 || letters > 2 {
        return None;
    }
    if words.contains(&last) {
        return None;
    }
    let fragment = words.iter().any(|word| {
        word.chars().count() > letters && (word.starts_with(last) || word.ends_with(last))
    });
    if fragment {
        Some(last.to_owned())
    } else {
        None
    }
}

#[test]
fn ordinary_words_stay_whole_inside_a_narrow_bubble() {
    let body = "harbor light stays on the dock tonight";
    let words: Vec<&str> = body.split_whitespace().collect();
    with_fonts_ui(|ui| {
        let wrap = grokhub_core::bubble_wrap_width(280.0, grokhub_core::BUBBLE_PAD_X);
        let job = crate::markdown::wrapped_job(ui, body, wrap, egui::Color32::WHITE);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let rows: Vec<String> = galley.rows.iter().map(|row| row.text()).collect();
        for row in &rows {
            assert!(
                    row_ends_with_short_fragment(row, &words).is_none(),
                    "galley row ends with a 1- or 2-letter fragment of a longer word: {row:?} in {rows:?}"
                );
        }
        for word in &words {
            assert!(
                rows.iter().any(|row| row.contains(word)),
                "word {word} was split across galley rows: {rows:?}"
            );
        }
        ui.allocate_ui(egui::vec2(280.0, 420.0), |ui| {
            ui.set_max_width(280.0);
            let row = ui.max_rect();
            let user = super::paint_speech_bubble(ui, body, true, false);
            let assistant = super::paint_speech_bubble(ui, body, false, true);
            assert!(
                user.rect.max.x <= row.max.x + 1.0 && user.rect.height() > 16.0,
                "user bubble {} x {} h {}",
                user.rect.max.x,
                row.max.x,
                user.rect.height()
            );
            assert!(
                assistant.rect.max.x <= row.max.x + 1.0 && assistant.rect.height() > 16.0,
                "assistant bubble {} x {} h {}",
                assistant.rect.max.x,
                row.max.x,
                assistant.rect.height()
            );
        });
    });
}

#[test]
fn short_user_bubble_sits_on_the_right() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 200.0), |ui| {
            ui.set_max_width(800.0);
            let row = ui.max_rect();
            let resp = super::paint_speech_bubble(ui, "Hi", true, false);
            assert!(
                resp.rect.width() < 200.0,
                "short bubble stretched to {}",
                resp.rect.width()
            );
            assert!(
                (row.max.x - resp.rect.max.x).abs() < 8.0,
                "user bubble max.x {} not near row max.x {}",
                resp.rect.max.x,
                row.max.x
            );
        });
    });
}

#[test]
fn assistant_bubble_sits_flush_and_keeps_inner_pad() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 400.0), |ui| {
                ui.set_max_width(800.0);
                let row = ui.max_rect();
                let body = "The service page listing a Cummins QSX15 https://example.com/emea/documents/Service/02_Generators/PowerSource%20manual.pdf and a 500-hour kit.";
                let resp = super::paint_speech_bubble(ui, body, false, true);
                let lead = resp.rect.min.x - row.min.x;
                assert!(
                    lead < 40.0,
                    "assistant bubble shoved right of the pane: lead {lead} row {} bubble {}",
                    row.min.x,
                    resp.rect.min.x
                );
                assert!(
                    resp.rect.min.x + 0.5 >= row.min.x,
                    "assistant bubble clipped off the left: {} < {}",
                    resp.rect.min.x,
                    row.min.x
                );
                assert!(
                    resp.rect.max.x <= row.max.x + 1.0,
                    "assistant bubble overflowed the right: {} > {}",
                    resp.rect.max.x,
                    row.max.x
                );
                assert!(
                    resp.rect.width() > 400.0,
                    "long assistant reply must use the pane, got {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.height() > 36.0,
                    "long URL must wrap inside the bubble, height {}",
                    resp.rect.height()
                );
            });
    });
    let speech = include_str!("chat_ui.rs")
        .split("fn paint_speech_bubble(")
        .nth(1)
        .and_then(|s| s.split("fn paint_msg_acts(").next())
        .expect("paint_speech_bubble");
    assert!(
        speech.contains("inner_margin(egui::Margin::ZERO)")
            && speech.contains("add_space(BUBBLE_PAD_Y)")
            && speech.contains("add_space(BUBBLE_PAD_X)")
            && speech.contains("allocate_exact_size(egui::vec2(REPLY_MARK_W, REPLY_MARK_W)")
            && include_str!("chat_ui.rs").contains("const REPLY_MARK_W: f32 = 16.0;"),
        "bubble pad must sit inside the fill, not get clipped by rounded inner_margin: {speech}"
    );
}

#[test]
fn long_assistant_bubble_wraps_instead_of_one_line() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 400.0), |ui| {
            ui.set_max_width(800.0);
            let body = "word ".repeat(80);
            let resp = super::paint_speech_bubble(ui, &body, false, true);
            assert!(
                resp.rect.width() <= grokhub_core::bubble_max_width(800.0) + 8.0,
                "bubble {}",
                resp.rect.width()
            );
            assert!(
                resp.rect.width() > 500.0,
                "an 800px pane must not use a 440px column, got {}",
                resp.rect.width()
            );
            assert!(
                resp.rect.height() > 48.0,
                "wrapped bubble height {}",
                resp.rect.height()
            );
        });
    });
}

#[test]
fn long_thought_wraps_instead_of_truncating() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 500.0), |ui| {
            ui.set_max_width(800.0);
            let body = "word ".repeat(80);
            let resp = super::paint_thought_bubble(ui, &body);
            assert!(
                resp.rect.width() <= 800.0 + 8.0,
                "thought spilled the pane: {}",
                resp.rect.width()
            );
            assert!(
                resp.rect.height() > 48.0,
                "thought stayed one clipped line, height {}",
                resp.rect.height()
            );
        });
    });
    let src = cabin_src();
    let thought = src
        .split("ChatKind::Thought => {")
        .nth(1)
        .and_then(|s| s.split("ChatKind::Tool => {").next())
        .expect("thought arm");
    assert!(
        thought.contains("paint_thought_bubble") || thought.contains("paint_speech_bubble"),
        "thoughts must wrap through the speech bubble path: {thought}"
    );
    assert!(
        !thought.contains("if open"),
        "thought body must stay visible after the turn, not collapse to a badge: {thought}"
    );
    assert!(
        thought.contains("paints_body"),
        "expand, minimize, and hide stay on the existing thought arm: {thought}"
    );
    let impl_src = src.as_str();
    assert_eq!(
        impl_src.matches("ChatKind::Thought => {").count(),
        1,
        "one thought renderer"
    );
}

#[test]
fn thought_body_stays_visible_when_idle() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 400.0), |ui| {
                ui.set_max_width(800.0);
                let block = grokhub_core::ChatView {
                    kind: grokhub_core::ChatKind::Thought,
                    title: "Thought".into(),
                    body: "I'll start by checking which desktop environment and session-restore setup you already have, then wire window size and position into that boot path. After that I'll confirm the restored geometry.".into(),
                };
                let closed = ui
                    .scope(|ui| {
                        let _ = super::paint_chat_block(
                            ui,
                            &block,
                            true,
                            false,
                            grokhub_core::ThoughtFold::Expanded,
                        );
                    })
                    .response;
                assert!(
                    closed.rect.height() > 36.0,
                    "idle thought hid the body, height {}",
                    closed.rect.height()
                );
            });
    });
}

#[test]
fn thought_fold_paints_expand_minimize_and_hide() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 700.0), |ui| {
                ui.set_max_width(800.0);
                let thought = grokhub_core::ChatView {
                    kind: grokhub_core::ChatKind::Thought,
                    title: "Thought".into(),
                    body: "I'll start by checking which desktop environment and session-restore setup you already have, then wire window size and position into that boot path. After that I'll confirm the restored geometry and say what changed.".into(),
                };
                let reply = grokhub_core::ChatView {
                    kind: grokhub_core::ChatKind::Assistant,
                    title: String::new(),
                    body: "Window size and position restore through the session you already have.".into(),
                };
                let height = |ui: &mut egui::Ui, block: &grokhub_core::ChatView, fold| {
                    let y0 = ui.cursor().min.y;
                    let _ = super::paint_chat_block(ui, block, true, false, fold);
                    ui.cursor().min.y - y0
                };
                let expanded = height(ui, &thought, grokhub_core::ThoughtFold::Expanded);
                let minimized = height(ui, &thought, grokhub_core::ThoughtFold::Minimized);
                let hidden = height(ui, &thought, grokhub_core::ThoughtFold::Hidden);
                let reply_hidden = height(ui, &reply, grokhub_core::ThoughtFold::Hidden);
                let reply_open = height(ui, &reply, grokhub_core::ThoughtFold::Expanded);
                assert!(
                    expanded > 48.0,
                    "expanded thought must show the body, height {expanded}"
                );
                assert!(
                    minimized + 24.0 < expanded,
                    "minimized thought must be one short row, minimized {minimized} expanded {expanded}"
                );
                assert!(
                    minimized < 56.0,
                    "minimized thought grew past one row, height {minimized}"
                );
                assert!(
                    hidden < 4.0,
                    "hidden thought was still drawn, height {hidden}"
                );
                assert!(
                    (reply_hidden - reply_open).abs() < 1.0 && reply_open > 20.0,
                    "hiding a thought must not hide the reply: hidden-fold {reply_hidden} expanded-fold {reply_open}"
                );
            });
    });
}

#[test]
fn thought_fold_handoff_keeps_minimize_on_the_stored_body() {
    let ctx = egui::Context::default();
    let mut live = Vec::new();
    grokhub_core::append_thought(&mut live, "Need a snapshot");
    let slot = live[0].fold_slot;
    grokhub_core::append_thought(&mut live, " of the restore path.");
    assert_eq!(slot, live[0].fold_slot);
    let thread = "thread-a";
    super::write_thought_fold(
        &ctx,
        super::thought_fold_id(thread, "slot", live[0].fold_slot),
        grokhub_core::ThoughtFold::Minimized,
    );
    super::write_thought_fold(
        &ctx,
        super::thought_fold_id(
            thread,
            "body",
            grokhub_core::thought_body_key(&live[0].body),
        ),
        grokhub_core::ThoughtFold::Minimized,
    );
    let views = grokhub_core::visible_chat(&[
        ("user".into(), "check the session".into()),
        (
            "assistant".into(),
            grokhub_core::merge_thinking(&live[0].body, "I'll look at the session."),
        ),
    ]);
    let stored = views
        .iter()
        .find(|v| v.kind == grokhub_core::ChatKind::Thought)
        .expect("stored thought");
    let got = super::read_thought_fold(
        &ctx,
        super::thought_fold_id(thread, "body", grokhub_core::thought_body_key(&stored.body)),
    );
    assert_eq!(got, grokhub_core::ThoughtFold::Minimized);
    let fresh = super::read_thought_fold(
        &ctx,
        super::thought_fold_id(
            thread,
            "body",
            grokhub_core::thought_body_key("a brand new thought"),
        ),
    );
    assert_eq!(fresh, grokhub_core::ThoughtFold::Expanded);
    let reply = views
        .iter()
        .find(|v| v.kind == grokhub_core::ChatKind::Assistant)
        .expect("reply");
    assert!(grokhub_core::thought_fold_draws(
        reply.kind,
        grokhub_core::ThoughtFold::Minimized
    ));
    let src = cabin_src();
    let live = src
        .split("fn paint_live_blocks(")
        .nth(1)
        .and_then(|s| s.split("fn paint_tool_cards(").next())
        .expect("paint_live_blocks");
    assert!(
        live.contains("fold_slot") && live.contains("thought_body_key"),
        "live folds must be copied onto the stored body key: {live}"
    );
}

#[test]
fn long_sentence_stays_inside_the_pane_on_a_wide_row() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(1600.0, 500.0), |ui| {
                ui.set_max_width(1600.0);
                let body = "the clam gods? oh you know... ancient, briny, and extremely picky about their cream-to-broth ratio. they live in the black void between chowder pots, only emerging when someone dares to say manhattan style in their presence. knock twice and offer a saltine or they won't even open up.";
                let resp = super::paint_speech_bubble(ui, body, false, true);
                assert!(
                    resp.rect.width() <= grokhub_core::bubble_max_width(1600.0) + 8.0,
                    "wide pane stretched the bubble to {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.width() > 500.0,
                    "a wide pane must not squeeze the reply, got {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.height() > 28.0,
                    "long sentence must wrap, height {}",
                    resp.rect.height()
                );
            });
    });
}

#[test]
fn markdown_reply_grows_past_plain_measure() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 900.0), |ui| {
                ui.set_max_width(800.0);
                let body = "# Title\n## Heading\n\n- bullet one\n- bullet two\n\n```sh\nls -la\n```\n\nClosing line.";
                let wrap = grokhub_core::bubble_wrap_width(800.0, grokhub_core::BUBBLE_PAD_X);
                let measured = crate::markdown::measure_text(ui, body, wrap);
                let mut md_h = 0.0;
                ui.allocate_ui(egui::vec2(wrap, 800.0), |ui| {
                    let r = ui
                        .scope(|ui| {
                            ui.set_max_width(wrap);
                            crate::markdown::show(ui, body);
                        })
                        .response;
                    md_h = r.rect.height();
                });
                let resp = super::paint_speech_bubble(ui, body, false, true);
                assert!(
                    md_h > measured.y + 2.0,
                    "fixture must be taller as markdown than plain measure: md {md_h} plain {}",
                    measured.y
                );
                assert!(
                    resp.rect.height() + 2.0 >= md_h,
                    "markdown bubble clipped: painted {} markdown {}",
                    resp.rect.height(),
                    md_h
                );
            });
    });
}

#[test]
fn long_user_bubble_stays_inside_the_row() {
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(480.0, 400.0), |ui| {
            ui.set_max_width(480.0);
            let row = ui.max_rect();
            let body =
                "/very/long/path/to/grokhub/lib/systemd/status\" 2>/dev/null && echo ok ".repeat(6);
            let resp = super::paint_speech_bubble(ui, &body, true, false);
            assert!(
                resp.rect.min.x + 0.5 >= row.min.x,
                "user bubble clipped off the left: {} < {}",
                resp.rect.min.x,
                row.min.x
            );
            assert!(
                resp.rect.max.x <= row.max.x + 1.0,
                "user bubble overflowed the right: {} > {}",
                resp.rect.max.x,
                row.max.x
            );
            assert!(
                resp.rect.width() <= grokhub_core::bubble_max_width(480.0) + 8.0,
                "bubble {}",
                resp.rect.width()
            );
        });
    });
}

#[test]
fn unbroken_user_text_stays_inside_narrow_and_wide_rows() {
    let body = "a".repeat(480);
    // 140 shrinks to leave item_spacing. 800 is a typical pane: the 84% cap
    // already fits, and the leading gap must still be there.
    for width in [140.0_f32, 280.0, 480.0, 800.0, 1600.0] {
        with_fonts_ui(|ui| {
            ui.allocate_ui(egui::vec2(width, 800.0), |ui| {
                ui.set_max_width(width);
                let row = ui.max_rect();
                let gap = ui.spacing().item_spacing.x.max(0.0);
                assert!(
                    (row.width() - width).abs() < 1.0,
                    "harness row {} != requested {width}",
                    row.width()
                );
                let wrap = grokhub_core::bubble_wrap_width(width, grokhub_core::BUBBLE_PAD_X);
                let measured = crate::markdown::measure_text(ui, &body, wrap);
                assert!(
                    measured.x <= wrap + 1.0,
                    "width {width}: unwrapped measure {} > wrap {wrap} height {}",
                    measured.x,
                    measured.y
                );
                let resp = super::paint_speech_bubble(ui, &body, true, false);
                let lead = resp.rect.min.x - row.min.x;
                assert!(
                    resp.rect.min.x + 0.5 >= row.min.x,
                    "width {width}: bubble left {} < row {}",
                    resp.rect.min.x,
                    row.min.x
                );
                assert!(
                    lead + 1.0 >= gap,
                    "width {width}: leading gap {gap} not reserved, lead {lead}"
                );
                assert!(
                    resp.rect.max.x <= row.max.x + 1.0,
                    "width {width}: user bubble ran past the row {} > {} (bubble w {})",
                    resp.rect.max.x,
                    row.max.x,
                    resp.rect.width()
                );
                assert!(
                    row.max.x - resp.rect.max.x <= 1.0,
                    "width {width}: stray hole on the right, bubble max {} row {}",
                    resp.rect.max.x,
                    row.max.x
                );
                assert!(
                    resp.rect.width() <= width + 1.0,
                    "width {width}: bubble wider than the row {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.height() > 36.0,
                    "width {width}: unbroken text stayed one line, height {}",
                    resp.rect.height()
                );
            });
        });
    }
}

#[test]
fn failed_turns_are_marked_as_errors() {
    assert!(super::is_error_reply("Error: Ask is fail-closed"));
    assert!(super::is_error_reply("  Error: 502 upstream"));
    assert!(!super::is_error_reply("The error was in main.rs"));
    assert!(!super::is_error_reply("Errors: none"));
}

#[test]
fn chat_blocks_offer_copy_and_reply() {
    let src = cabin_src();
    let start = src.find("fn paint_msg_acts").expect("paint_msg_acts");
    let slice = &src[start..start + 2800];
    assert!(slice.contains("Copy"), "{slice}");
    assert!(slice.contains("Reply"), "{slice}");
    assert!(src.contains("fn paint_chat_block"), "{src}");
    assert!(src.contains("ChatBlockAct::Copy"));
    assert!(src.contains("ChatBlockAct::Reply"));
    assert!(src.contains("quote_for_reply"));
    assert!(src.contains("composer_want_focus"));
    assert!(src.contains("copy_text"));
    let block = fn_src(&src, "paint_chat_block");
    assert!(
        !block.contains("resp.hovered()"),
        "Copy/Reply must stay visible when the pointer leaves the bubble: {block}"
    );
    assert!(
        src.contains("selectable(true)"),
        "chat bubble text must be selectable for copy: {}",
        src[src.find("fn paint_speech_bubble").unwrap_or(0)..]
            .get(..400)
            .unwrap_or("")
    );
    let bubble = src.find("fn paint_speech_bubble").expect("speech bubble");
    let bubble_fn = &src[bubble..bubble + 1800];
    assert!(
        !bubble_fn.contains("vec2(row_w, 0.0)"),
        "a zero-height row clips the thread: {bubble_fn}"
    );
    assert!(
        !bubble_fn.contains("set_clip_rect"),
        "clip_rect on the row hides wrapped text: {bubble_fn}"
    );
    assert!(
        !bubble_fn.contains("right_to_left"),
        "RTL user rows clip long lines off the left: {bubble_fn}"
    );
    assert!(
        !bubble_fn.contains("row_h"),
        "a measured-height lock clips markdown: {bubble_fn}"
    );
    let assistant = src
        .split("// Assistant row: the mark sits at the top left")
        .nth(1)
        .and_then(|s| s.split("fn paint_msg_acts").next())
        .expect("assistant bubble");
    assert!(
        assistant.contains("horizontal_top"),
        "assistant mark must sit at the top of multi-line replies: {assistant}"
    );
    assert!(
        !assistant.contains("ui.horizontal("),
        "ui.horizontal centers the mark mid-block: {assistant}"
    );
    let speech = src
        .split("fn paint_speech_bubble(")
        .nth(1)
        .and_then(|s| s.split("fn paint_msg_acts(").next())
        .expect("paint_speech_bubble");
    assert!(
        speech.contains("bubble_assistant()") && speech.contains("USER_BUBBLE_RADIUS"),
        "assistant replies must sit in a bubble: {speech}"
    );
    let chat = src
        .split("fn ui_chat(")
        .nth(1)
        .and_then(|s| s.split("fn ui_empty_home(").next())
        .expect("ui_chat");
    assert!(
        chat.contains("available_width") && chat.contains("set_max_width(pane)"),
        "thread uses the CentralPanel pane: {chat}"
    );
    assert!(
        !chat.contains("chat_col_w") && !chat.contains("empty_home_side_gap"),
        "conversation must use the full chat pane, not a centered Grok column: {chat}"
    );
    assert!(
        !chat.contains("composer_pill_w"),
        "bubbles must not lock to the composer pill: {chat}"
    );
    assert!(
        chat.contains("cached_chat_views") && !chat.contains("visible_chat(&pairs)"),
        "idle chat must not clone the whole transcript every paint: {chat}"
    );
    assert!(
        chat.contains("paint_live_blocks") && chat.contains("views_up_to_last_user"),
        "tools must sit in the live turn, not always under the last bubble: {chat}"
    );
    assert!(
            !chat.contains("paint_running")
                && !chat.contains("chat_run_label")
                && !chat.contains("background_pill"),
            "2.10.87: no Thinking dot or Background button in the transcript; the composer glow shows a live turn: {chat}"
        );
    assert!(
        !chat.contains("run_slash(Slash::Stop)"),
        "the transcript running row must not paint a Stop: {chat}"
    );
    assert!(
        chat.contains("cluster_gap"),
        "consecutive thoughts must cluster tighter than chat: {chat}"
    );
    assert!(
        chat.contains("chat_row_height_id")
            && chat.contains("push_id(chat_row_id_salt")
            && chat.contains("skip_ahead_auto_ids(1)"),
        "offscreen rows must key height by pane width and keep a stable row id: {chat}"
    );
    let running = src
        .split("fn paint_running(")
        .nth(1)
        .and_then(|s| s.split("fn paint_one_tool_card(").next())
        .expect("paint_running");
    assert!(
        running.contains("paint_run_pulse") && !running.contains("Stop"),
        "transcript running chrome is the labeled live pulse without a Stop: {running}"
    );
    assert!(
        !running.contains("vec2(2.0, 16.0)"),
        "a blinking caret is not the in-progress indicator: {running}"
    );
    let attach = src
        .split("fn ui_attach_chip(")
        .nth(1)
        .and_then(|s| s.split("fn work_root(").next())
        .expect("ui_attach_chip");
    assert!(
            !attach.contains("paint_run_pulse") && !attach.contains("thinking_here"),
            "the running line above the composer is gone, including when the pane is scrolled: {attach}"
        );
    assert!(
        !attach.contains("clip_status") && !attach.contains("self.status"),
        "the status line above the composer is gone: {attach}"
    );
    assert!(
        chat.contains("scroll_to_cursor") && chat.contains("chat_tail_frames"),
        "a chat opens on its newest message, not where the last one was left: {chat}"
    );
    assert!(
        chat.contains("scrolled_off_tail") && chat.contains("jump_to_latest"),
        "scrolled up, the pane owes you a way back down: {chat}"
    );
    assert!(
        chat.contains("chat-jump")
            && chat.contains("BarIcon::ArrowDown")
            && chat.contains("ChatJump::Latest")
            && !chat.contains("chat-jump-last-you")
            && !chat.contains("chat-jump-latest")
            && !chat.contains("ghost_pill(ui, \"Last you\")")
            && !chat.contains("white_pill(ui, \"Jump to latest\")"),
        "one down-arrow jump Area, no overlapping Last you / Jump to latest pills: {chat}"
    );
    let switch = src
        .split("fn apply_switch_thread(")
        .nth(1)
        .and_then(|s| s.split("fn stamp_current_access(").next())
        .expect("apply_switch_thread");
    assert!(
        switch.contains("pin_chat_tail"),
        "the pane keeps the offset of the chat you left unless the swap re-pins it: {switch}"
    );
    assert!(
        !switch.contains("stamp_current_access") && !switch.contains("accessed_ms"),
        "opening a History row is not activity and must not move it: {switch}"
    );
    let show = fn_src(&src, "poll_session_show");
    assert!(
        show.contains("pin_chat_tail"),
        "a Grok transcript that lands after the click must bring the pane with it: {show}"
    );
    let composer_send = src
        .split("fn send_from_composer(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .expect("send_from_composer");
    assert!(
        composer_send.contains("pin_chat_tail") && composer_send.contains("send_chat"),
        "your own message follows itself down: {composer_send}"
    );
    let night = src
        .split("fn fire_night(")
        .nth(1)
        .and_then(|s| s.split("fn tick_review(").next())
        .expect("fire_night");
    assert!(
        !night.contains("send_from_composer"),
        "a night job must not yank the pane out of what you were reading: {night}"
    );
}

#[test]
fn cached_chat_views_do_not_clone_the_thread_on_stream_delta() {
    let src = cabin_src();
    let cache = fn_src(&src, "cached_chat_views");
    assert!(
        !cache.contains("m.1.clone()") && !cache.contains("role.clone()"),
        "a stream delta must not clone every message to rebuild chat views: {cache}"
    );
    assert!(
            cache.contains("refresh_last_stretch") || cache.contains("visible_chat_refs"),
            "last-message growth must refresh the trailing stretch without a full transcript clone: {cache}"
        );
}

fn with_fonts_ui(mut add: impl FnMut(&mut egui::Ui)) {
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| add(ui));
    });
}

#[test]
fn chat_row_height_cache_misses_on_resize() {
    let ctx = egui::Context::default();
    let narrow = super::chat_row_height_id("thread-a", 720.0);
    ctx.data_mut(|d| d.insert_temp(narrow, vec![40.0_f32, 88.0]));
    let same_width: Option<Vec<f32>> =
        ctx.data(|d| d.get_temp(super::chat_row_height_id("thread-a", 720.2)));
    assert_eq!(same_width.unwrap(), vec![40.0, 88.0]);
    let wide: Option<Vec<f32>> =
        ctx.data(|d| d.get_temp(super::chat_row_height_id("thread-a", 1100.0)));
    assert!(
        wide.is_none(),
        "a wider pane must not reuse heights wrapped at 720"
    );
    let other: Option<Vec<f32>> =
        ctx.data(|d| d.get_temp(super::chat_row_height_id("thread-b", 720.0)));
    assert!(other.is_none(), "another thread must not share row heights");
}

#[test]
fn skipped_row_advances_by_cached_height_only() {
    with_fonts_ui(|ui| {
        let y0 = ui.cursor().min.y;
        assert!(!super::reserve_offscreen_chat_row(ui, 36.0));
        assert!(
            (ui.cursor().min.y - y0).abs() < 0.01,
            "an on-screen row must still be painted"
        );
        let clip = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(200.0, 40.0));
        ui.set_clip_rect(clip);
        ui.add_space(80.0);
        let y1 = ui.cursor().min.y;
        assert!(super::reserve_offscreen_chat_row(ui, 36.0));
        assert!(
            (ui.cursor().min.y - y1 - 36.0).abs() < 0.01,
            "skip must reserve the cached height and nothing more, delta {}",
            ui.cursor().min.y - y1
        );
    });
}

#[test]
fn culled_row_does_not_steal_the_next_rows_widget_id() {
    fn copy_id(skip_earlier: bool, use_stable_id: bool) -> egui::Id {
        let ctx = egui::Context::default();
        let mut id = egui::Id::NULL;
        let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                for i in 0..2 {
                    if skip_earlier && i == 0 {
                        ui.add_space(20.0);
                        if use_stable_id {
                            ui.skip_ahead_auto_ids(1);
                        }
                        continue;
                    }
                    let button = |ui: &mut egui::Ui| ui.button("Copy");
                    let resp = if use_stable_id {
                        ui.push_id(super::chat_row_id_salt("thr", i), button).inner
                    } else {
                        button(ui)
                    };
                    if i == 1 {
                        id = resp.id;
                    }
                }
            });
        });
        id
    }
    let stable_full = copy_id(false, true);
    let stable_skip = copy_id(true, true);
    assert_eq!(
        stable_full, stable_skip,
        "push_id by thread+index must survive a culled neighbor"
    );
    assert_ne!(
        copy_id(false, false),
        copy_id(true, false),
        "egui auto ids still shift when a row is skipped; the stable id is what holds them"
    );
}

#[test]
fn click_other_project_stays_on_this_pane() {
    assert!(!super::click_project_opens_board(false));
}

#[test]
fn click_bound_project_stays_on_chat() {
    assert!(!super::click_project_opens_board(true));
}

#[test]
fn selected_project_stays_lit_while_filtering_chats() {
    assert!(super::project_row_active(true, true, super::Nav::Chat));
    assert!(super::project_row_active(true, true, super::Nav::Workboard));
    assert!(super::project_row_active(true, true, super::Nav::History));
    assert!(!super::project_row_active(true, false, super::Nav::Chat));
    assert!(!super::project_row_active(false, true, super::Nav::Chat));
}

#[test]
fn project_click_does_not_steal_chat_and_delete_releases_chats() {
    let src = cabin_src();
    let bind = src
        .split("fn bind_project_id(")
        .nth(1)
        .and_then(|s| s.split("fn make_project(").next())
        .expect("bind_project_id");
    assert!(
        !bind.contains("self.nav = Nav::Workboard"),
        "project click must not open the workboard: {bind}"
    );
    assert!(
        bind.contains("click_project_opens_board") && bind.contains("Nav::Chat"),
        "a project selected from the board returns to chat: {bind}"
    );
    let tree_at = bind.find("if tree_changed").expect("tree_changed");
    let halt_at = bind.find("halt_in_flight").expect("halt_in_flight");
    assert!(
        tree_at < halt_at,
        "restoring the same project filter must not halt a live reply: {bind}"
    );
    let drop_proj = src
        .split("fn remove_project_id(")
        .nth(1)
        .and_then(|s| s.split("fn apply_project_menu(").next())
        .expect("remove_project_id");
    assert!(
        drop_proj.contains("release_project_chats")
            && !drop_proj.contains("messages.clear")
            && !drop_proj.contains("threads.clear"),
        "delete must unassign chats without wiping transcripts: {drop_proj}"
    );
    let created = src
        .split("fn new_thread")
        .nth(1)
        .and_then(|s| s.split("fn begin_chat_rename").next())
        .expect("new_thread");
    assert!(
        created.contains("project_id"),
        "new chat while a project is selected must file into that folder: {created}"
    );
    let rail = src
        .split("id_salt(\"rail-history\")")
        .nth(1)
        .and_then(|s| s.split("fn page_nav(").next())
        .expect("rail-history");
    assert!(
        rail.contains("chat_section_indices") && !rail.contains("project_sel"),
        "the chat section lists cabin chats and does not read the project selection: {rail}"
    );
    let projects = src
        .split("RichText::new(\"Projects\")")
        .nth(1)
        .and_then(|s| s.split("id_salt(\"rail-history\")").next())
        .expect("project section");
    assert!(
        projects.contains("project_section_chat_indices") && projects.contains("paint_tree_gutter"),
        "chats nest under the folder or project they belong to: {projects}"
    );
    assert!(
        projects.contains("activate_project_row") && projects.contains("ProjectKind::Project"),
        "a project row is a chat: clicking it opens that chat: {projects}"
    );
    let activate = fn_src(&src, "activate_project_row");
    assert!(
        activate.contains("open_project_chat")
            && activate.contains("ProjectKind::Project")
            && activate.contains("n.open = !n.open")
            && activate.contains("ProjectKind::Folder"),
        "folder click only toggles open; project click opens the chat: {activate}"
    );
    assert!(
        projects.contains("RailIcon::Folder")
            && projects.contains("RailIcon::Chat")
            && !projects.contains("RailIcon::File"),
        "a folder looks like a folder and a project looks like a chat: {projects}"
    );
    let caret = projects.split("paint_folder_caret").next().unwrap_or("");
    assert!(
            projects.contains("paint_folder_caret")
                && caret.contains("ProjectKind::Folder")
                && !projects.contains("n.open = !n.open;\n                        }\n                        self.touch_projects"),
            "only a folder collapses, and that click does not open a chat: {projects}"
        );
    let paint = fn_src(&src, "paint_section_chats");
    assert!(
        paint.contains("TabAct::Switch") && !paint.contains("project_sel"),
        "clicking a listed chat must not change the other section: {paint}"
    );
    let filed = fn_src(&src, "new_chat_under");
    assert!(
        filed.contains("new_thread")
            && !filed.contains("threads.clear")
            && !filed.contains("messages.clear"),
        "a new chat under a folder must not wipe History: {filed}"
    );
    let row = include_str!("../threads.rs");
    assert!(
        row.contains("fn project_folder_history_row") && row.contains("empty_chat_draft"),
        "project History rows must use empty_chat_draft"
    );
    let chat_fn = row
        .split("fn chat_section_indices(")
        .nth(1)
        .and_then(|s| s.split("pub fn project_section_chat_indices").next())
        .expect("chat_section_indices");
    assert!(
        !chat_fn.contains("selected"),
        "the chat section has no project filter argument: {chat_fn}"
    );
}

#[test]
fn health_opens_the_about_page() {
    assert_eq!(super::health_settings_sec(), super::SettingsSec::About);
}

#[test]
fn about_paints_the_version() {
    let src = cabin_src();
    let impl_src = src.as_str();
    let about = impl_src
        .split("SettingsSec::About => {")
        .nth(1)
        .expect("about body");
    let about = about
        .split("if let Some(s) = next_sec")
        .next()
        .unwrap_or(about);
    assert!(
        about.contains("CARGO_PKG_VERSION"),
        "About must show grokhub --version: {about}"
    );
    assert!(
        about.contains("FONT_HEADING"),
        "version is a heading, not a muted note: {about}"
    );
    assert!(
        !about.contains("usage_line") && !about.contains("catalog_line"),
        "About must not paint today-stats or the model catalog: {about}"
    );
}

#[test]
fn a_nav_action_opens_the_page_it_names() {
    let src = cabin_src();
    let run = src
        .split("fn run_palette(")
        .nth(1)
        .and_then(|s| s.split("fn open_palette(").next())
        .unwrap_or(&src);
    assert!(
        run.contains("\"nav:command\" => self.nav = Nav::Command"),
        "Command is a real page — a chip that names it must not land on Chat: {run}"
    );
    assert!(
        super::Cabin::nav_from_id("command") == super::Nav::Command,
        "the chip id and the page have to agree"
    );
    assert!(
        super::Cabin::nav_from_id("ideas") == super::Nav::Pulse,
        "an ideas chip must open Ideas, not Chat"
    );
    assert!(
        grokhub_core::nav_from_chip_value("__nav:ideas") == Some("ideas"),
        "command chips name Ideas as __nav:ideas"
    );
    assert!(
        super::Cabin::nav_from_id("eyes") == super::Nav::Chat,
        "Desk is gone — its id lands on chat"
    );
    assert!(super::Cabin::nav_from_id("nonsense") == super::Nav::Chat);
}

#[test]
fn palette_search_walks_nested_files_off_the_ui_thread() {
    let src = cabin_src();
    let tick = src
        .split("fn tick_palette_search(")
        .nth(1)
        .and_then(|s| s.split("fn kick_palette_search(").next())
        .expect("tick_palette_search");
    assert!(
        !tick.contains("search_place") && tick.contains("palette_search_is_saved"),
        "a saved empty palette result must not walk again, and not on the UI thread: {tick}"
    );
    assert!(
        tick.contains("palette_forget_stale_walk") && tick.contains("palette_search_is_saved"),
        "a query change must forget the last finished key or a revert keeps the empty list: {tick}"
    );
    let open = src
        .split("fn open_palette(")
        .nth(1)
        .and_then(|s| s.split("fn run_palette(").next())
        .expect("open_palette");
    assert!(
        open.contains("palette_file_rx = None"),
        "reopen must drop a leftover walk or it can block the next search: {open}"
    );
    let kick = src
        .split("fn kick_palette_search(")
        .nth(1)
        .and_then(|s| s.split("fn poll_palette_search(").next())
        .expect("kick_palette_search");
    assert!(
        kick.contains("thread::spawn") && kick.contains("search_place"),
        "palette search walks the current place off the UI thread: {kick}"
    );
    assert!(
        src.contains("RailIcon::Search") && src.contains("self.open_palette()"),
        "Search stays the palette — no second search page"
    );
}

#[test]
fn palette_query_revert_forgets_empty_saved_hits() {
    let mut files = vec!["nested/deep/buried.txt".to_string()];
    let mut files_q = "buried".to_string();
    let mut files_root = "/place".to_string();
    grokhub_core::palette_forget_stale_walk(
        &mut files,
        &mut files_q,
        &mut files_root,
        "buriexx",
        "/place",
    );
    assert!(files.is_empty(), "a query change clears the last hits");
    assert!(
        files_q.is_empty() && files_root.is_empty(),
        "the finished key must be forgotten or a revert matches the empty list"
    );
    assert!(
        !grokhub_core::palette_search_is_saved(&files_q, &files_root, "buried", "/place"),
        "reverting to the earlier query must walk again so the file hits come back"
    );
    grokhub_core::palette_forget_stale_walk(
        &mut files,
        &mut files_q,
        &mut files_root,
        "buried",
        "/place",
    );
    assert!(
        files_q.is_empty() && files.is_empty(),
        "revert still has no finished key, so tick kicks a real walk: {files_q:?} {files:?}"
    );
}

#[test]
fn picking_a_palette_file_opens_it() {
    let action =
        grokhub_core::palette_row_action(&[], &["nested/deep/buried.txt".to_string()], "/place", 0)
            .expect("file row");
    let shown = grokhub_core::palette_file_shown(&action).expect("picked path");
    assert!(
        shown.ends_with("nested/deep/buried.txt") && !shown.contains(".."),
        "the pick must be the nested file, not a status-only label: {shown}"
    );
    assert_eq!(grokhub_core::palette_file_shown("file:"), None);
    assert_eq!(grokhub_core::palette_file_shown("nav:chat"), None);
    let src = cabin_src();
    let run = src
        .split("fn run_palette(")
        .nth(1)
        .and_then(|s| s.split("fn run_slash_line(").next())
        .expect("run_palette");
    assert!(
        run.contains("palette_file_shown") && run.contains("desktop::open_path"),
        "picking a palette file must open it, not only write status: {run}"
    );
}

#[test]
fn settings_drops_cabin_tabs() {
    let src = cabin_src();
    let settings = src
        .split("fn ui_settings(")
        .nth(1)
        .and_then(|s| s.split("fn add_automation_seed(").next())
        .expect("ui_settings");
    assert!(
        !settings.contains("section_label(ui, \"Cabin\")"),
        "Cabin group is gone from Settings: {settings}"
    );
    assert!(
        !settings.contains("Cabin eyes"),
        "Cabin eyes toggle is gone: {settings}"
    );
    // The Host tab itself is gone; `always_permission_keeps_the_acp_session` proves
    // the variant no longer exists anywhere in the file.
    assert!(
        settings.contains("(SettingsSec::Behavior, \"Behavior\")"),
        "the tabs that remain are the ones with a home: {settings}"
    );
    assert!(
        !settings.contains("section_label(ui, \"Data\")")
            && !settings.contains("settings_nav(ui, \"GitHub\""),
        "GitHub is connector-managed — Settings must not keep a Data/GitHub tab: {settings}"
    );
    let account = settings
        .split("SettingsSec::Account => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::Appearance => {").next())
        .expect("Account");
    assert!(
        !account.contains("Install Grok Build CLI")
            && !account.contains("Console key")
            && !account.contains("Device name")
            && !account.contains("Imagine override"),
        "Account is OAuth connect/sign-out only: {account}"
    );
    assert!(
        !settings.contains("Automations a day")
            && !settings.contains("Host commands")
            && !settings.contains("Quiet hours start"),
        "Behavior dropped a-day/host caps and split quiet clocks: {settings}"
    );
    assert!(
        settings.contains("settings_dropdown") && settings.contains("Quiet hours"),
        "Quiet hours is one dropdown: {settings}"
    );
    let update = settings
        .split("SettingsSec::Update => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::About => {").next())
        .expect("Update");
    assert!(
        !update.contains("settings_update_note()")
            && !update.contains("Source clone")
            && !update.contains("Install overlay")
            && !update.contains("Update Grok Build CLI")
            && !update.contains("show_cli_update")
            && update.contains("update_label")
            && update.contains("update_hint")
            && !update.contains("Install Grok Build CLI")
            && !update.contains("show_cli_install")
            && update.contains("cabin_notice")
            && !update.contains("cli_notice")
            && update.contains("settings_action")
            && !update.contains("if let Some(label) = update_label"),
        "Update is one always-visible control for the cabin, never the CLI: {update}"
    );
    assert!(
        settings.contains("cabin_update_notice")
            && !settings.contains("cli_update_notice")
            && !settings.contains("should_update_cli_alpha")
            && settings.contains("queue_cabin_update")
            && settings.contains("settings_update_label")
            && settings.contains("settings_update_hint"),
        "Settings Update stays visible; titlebar chip still hides when current: {settings}"
    );
    assert!(
        !account.contains("Update CLI")
            && !account.contains("cabin_update_notice")
            && !account.contains("cli_update_notice")
            && !account.contains("GitHub Latest")
            && !account.contains("Update Grok Build CLI")
            && !account.contains("update_chip_label"),
        "cabin notify must not live on Account: {account}"
    );
}

#[test]
fn about_section_opens_update() {
    assert_eq!(
        super::settings_group_home(super::SettingsGroup::About),
        super::SettingsSec::Update
    );
}

#[test]
fn overlay_update_skips_chat() {
    let v = grokhub_core::overlay_update_begin(2);
    assert!(v.stay_on_update);
    assert!(!v.posts_chat);
    let done = grokhub_core::overlay_update_finish(true, 50);
    assert!(!done.posts_chat);
    assert!(done.stay_on_update);
    assert!(done.can_restart);
    assert!(grokhub_core::overlay_update_can_restart(true, false));
    assert!(!grokhub_core::overlay_update_can_restart(true, true));
}

#[test]
fn general_section_opens_account() {
    assert_eq!(
        super::settings_group_home(super::SettingsGroup::General),
        super::SettingsSec::Account
    );
}

#[test]
fn slash_arrows_move_and_clamp() {
    assert_eq!(super::slash_pick_step(0, 5, 1), 1);
    assert_eq!(super::slash_pick_step(0, 5, -1), 0);
    assert_eq!(super::slash_pick_step(4, 5, 1), 4);
    assert_eq!(super::slash_pick_step(9, 3, 0), 2);
}

#[test]
fn tab_accept_runs_on_pick() {
    let mut composer = "/fi".into();
    let run = super::slash_pick_take(&mut composer, "/fix", true);
    assert_eq!(run.as_deref(), Some("/fix"));
    assert!(composer.is_empty());
}

#[test]
fn tab_accept_stays_for_args() {
    let mut composer = "/proj".into();
    let run = super::slash_pick_take(&mut composer, "/project bind ", false);
    assert!(run.is_none());
    assert_eq!(composer, "/project bind ");
}

#[test]
fn always_permission_keeps_the_acp_session() {
    let src = cabin_src();
    let ask = src
        .split("fn paint_perm_ask(")
        .nth(1)
        .and_then(|s| s.split("fn ui_empty_home").next())
        .expect("paint_perm_ask");
    assert!(
        !ask.contains("self.acp = None"),
        "Always on a live prompt must not drop the ACP session: {ask}"
    );
    assert!(
        ask.contains("p.reason") && ask.contains("p.action") && src.contains("fn paint_try_again("),
        "the Ask card says the action on one line, and hook reasons still paint: {ask}"
    );
    assert!(
        !ask.contains("!action.starts_with('{')"),
        "a bracket test or brace group must still paint on the Ask card: {ask}"
    );
    assert!(
        src.contains("fn paint_elicit_ask(")
            && src.contains("answer_elicit")
            && src.contains(".password(")
            && src.contains("redact_held_secrets")
            && src.contains("fn scrub_live_blocks("),
        "a secret elicit is masked and its value stays out of the transcript: {src}"
    );
    let finish = src
        .split("fn finish_acp_turn(")
        .nth(1)
        .and_then(|s| s.split("fn poll_host_diff(").next())
        .expect("finish_acp_turn");
    assert!(
        finish.contains("scrub_transcript") && finish.contains("scrub_live_blocks"),
        "held secrets must leave the live transcript and TTS: {finish}"
    );
    let grok_p = src
        .split("fn poll_single(")
        .nth(1)
        .and_then(|s| s.split("fn apply_single_turn(").next())
        .expect("poll_single");
    assert!(
        grok_p.contains("redact_held_secrets") && grok_p.contains("scrub_live_blocks"),
        "the grok-p pump must scrub held secrets: {grok_p}"
    );
    assert!(
        ask.contains("perm_key(")
            && ask.contains("PermKey::Allow")
            && ask.contains("PermKey::Deny"),
        "the shortcut sheet promises Enter / Esc on a permission card: {ask}"
    );
    assert!(
        ask.contains("perm_always_confirm")
            && ask.contains("paint_confirm_sheet")
            && ask.contains("always_session_spec")
            && ask.contains("ALWAYS_CONFIRM_LINE2")
            && ask.contains("ConfirmAct::Confirm")
            && ask.contains("ConfirmAct::Cancel"),
        "Ask Always is a second beat that names session skip and scheduled inherit: {ask}"
    );
    let always_click = ask
        .find("ghost_pill(ui, \"Always\")")
        .expect("Always ghost");
    let set_always = ask
        .find("set_permission_mode(PermissionMode::AlwaysApprove)")
        .expect("Always mode");
    assert!(
        always_click < set_always && ask[always_click..set_always].contains("perm_always_confirm"),
        "Always on the Ask card must confirm before flipping the pill: {ask}"
    );
    assert!(
        ask.contains("always_confirm_matches_rpc") && ask.contains("p.rpc_id"),
        "Always confirm must drop when the prompt/rpc_id changes: {ask}"
    );
    assert!(
        ask.contains("self.composer"),
        "Enter must send a typed follow-up instead of approving a tool: {ask}"
    );
    let poll = src
        .split("fn poll_acp(")
        .nth(1)
        .and_then(|s| s.split("fn finish_acp_turn").next())
        .expect("poll_acp");
    assert!(
        poll.contains("auto_allows()"),
        "Auto permission must answer ACP prompts, not only Always: {poll}"
    );
    assert!(
        poll.contains("answer_permission_always"),
        "Always must answer allow-always, not allow-once: {poll}"
    );
    assert!(
        poll.contains("perm_always_confirm = None"),
        "a replacement Ask must drop the Always confirm beat: {poll}"
    );
    let err = poll.split("AcpEvent::Err").nth(1).expect("acp err");
    let classify = err
        .find("classify_stream_error")
        .expect("classify 1.0.13 errors");
    let drop_acp = err.find("self.acp = None").expect("drop acp");
    assert!(
        classify < drop_acp,
        "transient 5xx / truncation must not drop ACP: {err}"
    );
    let always = src
        .split("Slash::AlwaysApprove =>")
        .nth(1)
        .and_then(|s| s.split("Slash::AutoPerm =>").next())
        .expect("AlwaysApprove");
    assert!(
        always.contains("grok_session = None"),
        "/always must session/new or Ask vs Always does not take: {always}"
    );
    assert!(
        always.contains("persist_idle_key") && !always.contains("self.persist()"),
        "/always must not clone every thread — bump the idle key so persist_bg skips: {always}"
    );
    let auto = src
        .split("Slash::AutoPerm =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Effort =>").next())
        .expect("AutoPerm");
    assert!(
        auto.contains("grok_session = None"),
        "/auto must session/new or permission mode does not take: {auto}"
    );
    assert!(
        auto.contains("persist_idle_key") && !auto.contains("self.persist()"),
        "/auto must not clone every thread — bump the idle key so persist_bg skips: {auto}"
    );
    let mode = src
        .split("Slash::Mode(mode)")
        .nth(1)
        .and_then(|s| s.split("Slash::Dream").next())
        .expect("Mode");
    assert!(
        mode.contains("self.persist_cfg()")
            && !mode.contains("self.persist()")
            && !mode.contains("persist_snap"),
        "/mode must not clone every thread just to write app.json: {mode}"
    );
    let appearance = src
        .split("SettingsSec::Appearance => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::Behavior => {").next())
        .expect("Appearance");
    assert!(
        appearance.contains("self.choose_theme(")
            && !appearance.contains("save = true")
            && !appearance.contains("self.persist()")
            && !appearance.contains("persist_snap"),
        "Appearance must not clone every thread just to write app.json: {appearance}"
    );
    let choose_theme = fn_src(&src, "choose_theme");
    assert!(
        choose_theme.contains("self.persist_cfg()") && !choose_theme.contains("self.persist()"),
        "choose_theme must write app.json without cloning every thread: {choose_theme}"
    );
    let set_close_to_tray = fn_src(&src, "set_close_to_tray");
    assert!(
        set_close_to_tray.contains("self.persist_cfg()")
            && !set_close_to_tray.contains("self.persist()"),
        "set_close_to_tray must write app.json without cloning every thread: {set_close_to_tray}"
    );
    let set_living_wall = fn_src(&src, "set_living_wall");
    assert!(
        set_living_wall.contains("self.persist_cfg()")
            && !set_living_wall.contains("self.persist()"),
        "set_living_wall must write app.json without cloning every thread: {set_living_wall}"
    );
    let behavior = src
        .split("SettingsSec::Behavior => {")
        .nth(1)
        .and_then(|s| s.split("SettingsSec::Update => {").next())
        .expect("Behavior");
    assert!(
            behavior.contains("self.set_close_to_tray(")
                && behavior.contains("self.set_living_wall(")
                && behavior.contains("self.persist_cfg()")
                && !behavior.contains("save = true")
                && !behavior.contains("self.persist()")
                && !behavior.contains("persist_snap"),
            "Close to tray, Living wall, and quiet hours must not clone every thread to write app.json: {behavior}"
        );
    assert!(
        behavior.contains("settings_dropdown")
            && behavior.contains("quiet_hours_menu")
            && behavior.contains("quiet_start_buf")
            && behavior.contains("quiet_end_buf")
            && !behavior.contains("cap_auto_buf")
            && !behavior.contains("cap_host_buf"),
        "quiet hours is one dropdown; a-day/host caps are gone: {behavior}"
    );
    let saved = src
        .split("fn save_settings(")
        .nth(1)
        .and_then(|s| s.split("fn ui_settings(").next())
        .expect("save_settings");
    assert!(
        saved.contains("normalize_hm") && saved.contains("cap_from_text"),
        "a typo in a clock or a cap must keep the old value, not switch the guard off: {saved}"
    );
    assert!(
        saved.contains("quiet_start_buf")
            && saved.contains("&self.cfg.quiet_start")
            && !saved.contains("default_quiet_start"),
        "Save must keep the last good clock, not the factory window: {saved}"
    );
    // Split so these assertions are not their own counter-examples.
    let gone = ["Host", "Voice", "Night", "Imagine", "Github"]
        .iter()
        .map(|s| format!("SettingsSec{}{s}", "::"))
        .chain(std::iter::once(format!("SettingsGroup{}Cabin", "::")))
        .chain(std::iter::once(format!("SettingsGroup{}Data", "::")))
        .find(|needle| src.contains(needle));
    assert_eq!(
        gone, None,
        "unreachable Settings sections are gone, not left painting into the void"
    );
    let plan = src
        .split("Slash::Plan =>")
        .nth(1)
        .and_then(|s| s.split("Slash::AlwaysApprove =>").next())
        .expect("Plan");
    assert!(
        plan.contains("grok_session = None"),
        "/plan must session/new or Chat vs Plan does not take: {plan}"
    );
    assert!(
        plan.contains("persist_idle_key") && !plan.contains("self.persist()"),
        "/plan must not clone every thread — bump the idle key so persist_bg skips: {plan}"
    );
    assert!(
        plan.contains("halt_in_flight"),
        "/plan mid-turn must halt or Thinking sticks after the agent is dropped: {plan}"
    );
    // rustfmt may break `let row = crate::cards::session_row(...)` across lines.
    let row = src
        .split("crate::cards::session_row(")
        .nth(1)
        .and_then(|s| s.split("ui.allocate_ui_with_layout").next())
        .expect("session_row");
    assert_eq!(
        row.matches("grok_session = None").count(),
        2,
        "session/permission row must session/new so mode takes: {row}"
    );
    assert_eq!(
            row.matches("persist_idle_key").count(),
            2,
            "session/permission row must not clone every thread — bump the idle key so persist_bg skips: {row}"
        );
    assert!(
        row.contains("select_plan_without_rename") && !row.contains("t.title ="),
        "clicking Plan must switch mode without writing the chat title: {row}"
    );
    let plan_pick = src
        .split("fn select_plan_without_rename(")
        .nth(1)
        .and_then(|s| s.split("fn hold_chat_name_for_plan(").next())
        .expect("select_plan_without_rename");
    assert!(
            plan_pick.contains("hold_chat_name_for_plan")
                && plan_pick.contains("grok_session = None")
                && plan_pick.contains("set_session_mode(SessionMode::Plan)")
                && !plan_pick.contains("t.title ="),
            "selecting Plan keeps the session-id clear and does not write the thread title: {plan_pick}"
        );
    let hold = src
        .split("fn hold_chat_name_for_plan(")
        .nth(1)
        .and_then(|s| s.split("fn thread_rail_title(").next())
        .expect("hold_chat_name_for_plan");
    assert!(
        hold.contains("title_after_selecting_plan")
            && hold.contains("history_label_after_plan")
            && !hold.contains("t.title ="),
        "Plan must keep the thread title and the History label already on screen: {hold}"
    );
    assert!(
        plan.contains("hold_chat_name_for_plan"),
        "/plan must keep the chat name when it clears the session id: {plan}"
    );
}

#[test]
fn selecting_plan_does_not_change_the_thread_title() {
    assert_eq!(
        grokhub_core::cli_history::title_after_selecting_plan("Night watch"),
        "Night watch"
    );
    assert_eq!(
        grokhub_core::cli_history::preferred_history_title(
            "Night watch",
            false,
            Some("Plan"),
            Some("abc")
        ),
        "Night watch"
    );
    assert_eq!(
        grokhub_core::cli_history::history_label_after_plan("Night watch", "fix the dock"),
        "Night watch"
    );
}

#[test]
fn slash_pick_resets_when_the_list_changes() {
    assert_eq!(super::slash_pick_retain(2, true, 4), 0);
    assert_eq!(super::slash_pick_retain(2, false, 4), 2);
    assert_eq!(super::slash_pick_retain(9, false, 3), 2);
    assert_eq!(super::slash_pick_retain(1, true, 0), 0);
}

#[test]
#[allow(clippy::assertions_on_constants)] // pins design constants
fn idle_visible_cabin_does_not_spin() {
    assert!(!super::wants_live_repaint(
        false, false, false, true, false, false
    ));
    assert!(!super::wants_live_repaint(
        false, false, false, false, false, false
    ));
    assert!(super::wants_live_repaint(
        true, false, false, true, false, false
    ));
    assert!(super::wants_live_repaint(
        false, false, false, false, false, true
    ));
    assert!(super::HIDDEN_HEARTBEAT_MS > 80);
    assert_eq!(
        grokhub_core::heartbeat_repaint_ms(
            false,
            false,
            grokhub_core::HEARTBEAT_MS,
            super::HIDDEN_HEARTBEAT_MS
        ),
        grokhub_core::HEARTBEAT_MS
    );
    assert_eq!(
        grokhub_core::heartbeat_repaint_ms(
            false,
            true,
            grokhub_core::HEARTBEAT_MS,
            super::HIDDEN_HEARTBEAT_MS
        ),
        super::HIDDEN_HEARTBEAT_MS,
        "hidden idle must wake for tray Quit, not the 15s pulse"
    );
    assert_eq!(
        grokhub_core::heartbeat_repaint_ms(
            true,
            true,
            grokhub_core::HEARTBEAT_MS,
            super::HIDDEN_HEARTBEAT_MS
        ),
        80
    );
    let src = cabin_src();
    let live = src
        .split("let live = wants_live_repaint(")
        .nth(1)
        .and_then(|s| s.split("ctx.request_repaint_after").next())
        .expect("wants_live_repaint call");
    assert!(
            live.contains("persist_rx")
                && live.contains("history_rx")
                && live.contains("mem_restore_rx")
                && live.contains("mem_file_rx")
                && live.contains("recall_rx")
                && live.contains("sync_rx")
                && live.contains("inhabit_rx")
                && live.contains("reflect_rx")
                && live.contains("session_show_rx")
                && live.contains("import_rx")
                && live.contains("recipe_desk_rx")
                && live.contains("host_diff_rx")
                && live.contains("pick_rx")
                && live.contains("pick_list_rx")
                && live.contains("oauth_start_rx")
                && live.contains("oauth_poll_rx")
                && live.contains("greeting_busy")
                && live.contains("greeting_files_rx")
                && live.contains("night_check_rx")
                && live.contains("eyes_cap_rx")
                && live.contains("grok_p_rx"),
            "History listing / inspect / greeting / night check / Eyes capture / plus-upload / Settings doctor must not wait on the 15s heartbeat: {live}"
        );
}

#[test]
fn show_cabin_keeps_the_tray_icon() {
    let src = cabin_src();
    let show = fn_src(&src, "show_from_tray");
    assert!(
        !show.contains("drop_off_thread"),
        "Show cabin must not tear down the tray icon: {show}"
    );
    assert!(
        show.contains("ensure_tray_spawn"),
        "Show cabin should keep a live tray: {show}"
    );
    assert!(
        src.contains("StartDrag") && src.contains("titlebar_should_start_drag"),
        "undecorated cabin must drag from the titlebar body"
    );
    assert!(
        src.contains("force_x11_for_close_to_tray")
            || include_str!("../main.rs").contains("force_x11_for_close_to_tray"),
        "winit 0.30 must drop WAYLAND_DISPLAY so × can unmap"
    );
    assert!(
        src.contains("hidden_window_tick"),
        "a pinned taskbar click must raise the hidden cabin instead of re-unmapping it"
    );
    let hide = src
        .split("fn unmap_to_tray")
        .nth(1)
        .and_then(|s| s.split("fn ensure_tray_spawn").next())
        .expect("unmap_to_tray");
    assert!(
            hide.contains("tray_saw_unfocused = false"),
            "× must clear the focus-raise latch so the next focused frame does not map the cabin: {hide}"
        );
    assert!(
        hide.contains("persist_if_dirty") && !hide.contains("self.persist()"),
        "hide to tray must not clone every thread when idle persist already wrote: {hide}"
    );
    assert!(
        hide.contains("hide_cabin"),
        "Windows × must cloak and leave the taskbar, not minimize: {hide}"
    );
    let tick = src
        .split("hidden_window_tick(")
        .nth(1)
        .and_then(|s| s.split("match").next())
        .expect("hidden_window_tick call");
    assert!(
        tick.contains("tray_saw_unfocused"),
        "taskbar raise waits until the hidden cabin actually lost focus: {tick}"
    );
    assert!(
        src.contains("hidden_raise_ready") && src.contains("reapply_unmap"),
        "× must not flash back from a FocusLost/FocusGained bounce or Visible(false) spam"
    );
    let stay = src
        .split("HiddenTick::StayHidden =>")
        .nth(1)
        .and_then(|s| s.split("ctx.request_repaint_after").next())
        .expect("StayHidden");
    assert!(
        stay.contains("hide_cabin"),
        "leftover focus after × must re-unmap the Windows taskbar stub: {stay}"
    );
    let finish = fn_src(&src, "finish_acp_turn");
    let apply = fn_src(&src, "apply_single_turn");
    assert!(
        !finish.contains("save_schedule") && !apply.contains("save_schedule"),
        "ordinary replies that mention every day at / heartbeat every must not become live jobs"
    );
    let saver = format!(
        "{}{}",
        fn_src(&src, "save_schedule"),
        fn_src(&src, "commit_schedule")
    );
    assert!(
        saver.contains("route_schedule") && saver.contains("ScheduleRoute::Clock"),
        "`every day at 9` must keep its hour instead of becoming a 1d loop: {saver}"
    );
    assert!(
            saver.contains("persist_loops")
                && saver.contains("persist_automations")
                && !saver.contains("self.persist()"),
            "a saved job must not clone every thread 2s later — the persist helpers bump the idle key: {saver}"
        );
    assert!(
        src.contains("ignore_close_request") && src.contains("self.want_quit"),
        "sticky close_requested must not hide after a taskbar raise, and tray Quit must still exit"
    );
    assert!(
        include_str!("../main.rs").contains("try_claim_cabin"),
        "a second grokhub from the taskbar must raise the running cabin and exit"
    );
    assert!(
        cabin_src().contains("honor_cabin_raise(self.want_quit)"),
        "Restart must not CancelClose when a sibling spawn writes cabin.raise"
    );
    assert!(
        show.contains("CancelClose"),
        "Show cabin must clear a sticky close so the window does not hide again"
    );
    let restart = src
        .split("fn restart_after_update")
        .nth(1)
        .and_then(|s| s.split("fn start_overlay_update").next())
        .expect("restart_after_update");
    let spawn_at = restart.find("restart_system").expect("restart_system");
    let drop_at = restart.find("drop_tray");
    assert!(
        drop_at.is_some_and(|d| d > spawn_at),
        "dropping the tray before spawn leaves a headless cabin when restart fails: {restart}"
    );
}

#[test]
fn ui_date_spawns_must_time_out() {
    let src = cabin_src();
    let date = src
        .split("fn date_out(")
        .nth(1)
        .and_then(|s| s.split("\n    fn local_clock()").next())
        .expect("date_out");
    assert!(
        date.contains("run_limited("),
        "date_out must kill a hung date: {date}"
    );
    let clock = src
        .split("fn local_clock()")
        .nth(1)
        .and_then(|s| s.split("\n    fn local_day()").next())
        .expect("local_clock");
    assert!(
        clock.contains("date_out(") && !clock.contains(".output()"),
        "local_clock must use the timed date helper: {clock}"
    );
    assert!(
        clock.contains("CLOCK_TTL"),
        "chips and greeting must not spawn date on every paint: {clock}"
    );
    assert!(
        clock.contains("thread::spawn") && clock.contains("inflight"),
        "stale date must refresh off the UI thread: {clock}"
    );
    let day = src
        .split("fn local_day()")
        .nth(1)
        .and_then(|s| s.split("\n    fn tick_heartbeat").next())
        .expect("local_day");
    assert!(
        day.contains("date_out(") && !day.contains(".output()"),
        "local_day must use the timed date helper: {day}"
    );
    assert!(
        day.contains("thread::spawn") && day.contains("inflight"),
        "stale local_day must refresh off the UI thread: {day}"
    );
    let roll = fn_src(&src, "roll_today");
    assert!(
        roll.contains("local_day(") && !roll.contains(".output()"),
        "roll_today must reuse the cached day, not spawn date on the UI thread: {roll}"
    );
    assert!(
        roll.contains("persist_usage")
            && roll.contains("persist_idle_key")
            && !roll.contains("self.persist()"),
        "day rollover must not clone every thread just to write usage.json: {roll}"
    );
}

#[test]
fn persist_does_not_hold_hub_lock_across_disk() {
    let src = cabin_src();
    let persist = format!(
        "{}{}",
        fn_src(&src, "persist"),
        fn_src(&src, "persist_snap")
    );
    assert!(
        persist.contains("self.hub.clone()") && !persist.contains("state_for_disk"),
        "persist must not clone hub snapshot/last_frame on the UI thread: {persist}"
    );
    assert!(
        !persist.contains("if let Ok(st) = self.hub.lock()"),
        "persist must not hold hub.lock() across save_hub_state: {persist}"
    );
    let write = fn_src(&src, "write_persist_disk");
    let lock = write.find("hub.lock").expect("worker hub lock");
    let save = write.find("save_hub_state").expect("save_hub_state");
    assert!(
        lock < save && write.contains("state_for_disk(&st)"),
        "persist worker must clone hub state then drop the lock before hub-state.json: {write}"
    );
}

#[test]
fn greeting_and_chips_use_the_native_fast_model() {
    let src = cabin_src();
    let native = include_str!("native_unattended.rs").replace("\r\n", "\n");
    let greet = fn_src(&src, "spawn_greeting_llm");
    assert!(greet.contains("spawn_native_greeting"), "{greet}");
    let chips = fn_src(&src, "spawn_chip_llm");
    assert!(chips.contains("spawn_native_chips"), "{chips}");
    let fast = fn_src(&native, "fast_text");
    assert!(
        fast.contains("CABIN_FAST_MODEL") && fast.contains("CABIN_FAST_FALLBACK"),
        "chips/greeting use the fast model and fall back if it is empty: {fast}"
    );
    for body in [greet, chips, fast] {
        assert!(
            !body.contains("find_grok") && !body.contains("grok_cli_key"),
            "{body}"
        );
    }
    let ready = fn_src(&src, "llm_ready");
    assert!(
        !ready.contains("find_grok"),
        "llm_ready never counts the Grok Build CLI: {ready}"
    );
    let chip = src
        .split("fn apply_chip(")
        .nth(1)
        .and_then(|s| s.split("fn nav_from_id").next())
        .expect("apply_chip");
    let chip_spawn = chip
        .find("thread::spawn")
        .expect("chip save must leave the UI thread");
    let chip_save = chip.find("save_chips").expect("save_chips");
    assert!(
        chip_spawn < chip_save && chip.contains("persist_io"),
        "chip click must not freeze the cabin writing chips.json: {chip}"
    );
    assert!(
        chip.contains("send_chat(chip.value)"),
        "/learn chip click must use the typed send path, not a silent parse miss: {chip}"
    );
}

#[test]
fn grok_login_powers_history_and_imagine() {
    let src = cabin_src();
    assert!(
        !src.contains("fn ensure_acp("),
        "the engine starts through ensure_native_engine only"
    );
    let bearer = fn_src(&src, "bearer");
    assert!(
        !bearer.contains("grok_cli_key") && !bearer.contains("refresh_grok_login"),
        "the Imagine/chips bearer never reads or refreshes the Grok CLI login: {bearer}"
    );
    assert!(
        bearer.contains("refresh_cabin_oauth") && !bearer.contains("ensure_access"),
        "cabin OAuth refresh HTTP must leave the UI thread: {bearer}"
    );
    assert!(
        bearer.contains("console_key()"),
        "Imagine/ACP console-key fallback must read secrets.json: {bearer}"
    );
    let disk = fn_src(&src, "write_persist_disk");
    assert!(
        disk.contains("secrets::save"),
        "persist must write the console key to secrets.json: {disk}"
    );
    assert!(
        disk.contains("if let Some(s) = &snap.secrets") || disk.contains("snap.secrets"),
        "idle persist must not write secrets.json from a stale snap: {disk}"
    );
    assert!(
        src.contains("migrate_console_key"),
        "boot must move a leftover app.json console key into secrets.json"
    );
    assert!(
        src.contains("secrets::ensure_private"),
        "boot must rewrite a world-readable leftover secrets.json"
    );
    assert!(
        src.contains("secrets::console_key") && src.contains("migrate_console_key"),
        "Console key lives in secrets.json; Settings must not keep a leftover app.json field"
    );
    assert!(
        !fn_src(&src, "ui_settings").contains("Console key"),
        "Settings must not paint a Console key editor"
    );
    let settings_save = fn_src(&src, "save_settings");
    assert!(
        settings_save.contains("api_key.clear"),
        "Settings Save must not keep a leftover console key on cfg: {settings_save}"
    );
    assert!(
        settings_save.contains("self.persist()") && !settings_save.contains("secrets::save"),
        "Settings Save must not freeze the cabin writing secrets.json: {settings_save}"
    );
    assert!(
            settings_save.contains("self.persist_cfg()")
                && settings_save.contains("self.flush_projects()")
                && settings_save.contains("self.persist_hub()")
                && settings_save.contains("self.persist_secrets()")
                && settings_save.contains("tree_changed"),
            "Settings Save must not clone every thread when the worktree did not change: {settings_save}"
        );
    let persist_if = src
        .split("fn persist_if_dirty")
        .nth(1)
        .and_then(|s| s.split("fn persist_secrets").next())
        .expect("persist_if_dirty");
    assert!(
        persist_if.contains("persist_idle_key")
            && persist_if.contains("persist_cfg")
            && persist_if.contains("self.persist()"),
        "hide/quit must skip the thread clone when idle persist already wrote: {persist_if}"
    );
    let persist_secrets = src
        .split("fn persist_secrets(")
        .nth(1)
        .and_then(|s| s.split("fn persist_usage").next())
        .expect("persist_secrets");
    let secrets_spawn = persist_secrets
        .find("thread::spawn")
        .expect("secrets write must leave the UI thread");
    let secrets_save = persist_secrets
        .find("secrets::save")
        .expect("secrets::save");
    assert!(
        secrets_spawn < secrets_save
            && persist_secrets.contains("persist_io")
            && !persist_secrets.contains("self.persist()"),
        "Settings Save must not freeze the cabin writing secrets.json: {persist_secrets}"
    );
    let persist_usage = fn_src(&src, "persist_usage");
    let usage_spawn = persist_usage
        .find("thread::spawn")
        .expect("usage write must leave the UI thread");
    let usage_save = persist_usage.find("save_usage").expect("save_usage");
    assert!(
        usage_spawn < usage_save
            && persist_usage.contains("persist_io")
            && !persist_usage.contains("self.persist()"),
        "night usage must not freeze the cabin writing usage.json: {persist_usage}"
    );
    for name in [
        "persist_loops",
        "persist_suggestions",
        "persist_automations",
        "persist_updates",
    ] {
        let body = fn_src(&src, name);
        assert!(
            body.contains("persist_io") && body.contains("thread::spawn"),
            "{name} must take persist_io off the UI thread so FullSnap cannot tear JSON: {body}"
        );
    }
    let poll_err = fn_src(&src, "poll_persist_err");
    assert!(
        poll_err.contains("persist_idle_key.clear"),
        "a failed persist must clear the idle key so the next tick retries: {poll_err}"
    );
    let persist_svc_src = include_str!("persist_service.rs");
    assert!(
        persist_svc_src.contains("note_persist_err")
            && persist_svc_src.contains("persist lock poisoned"),
        "FullSnap must surface a poisoned persist_io instead of claiming disk is clean: {persist_svc_src}"
    );
    assert!(
        !include_str!("change_undo.rs").contains("salt: &str"),
        "egui id namespace param must not be named salt (CodeQL hard-coded salt FP)"
    );
    let bg = src
        .split("fn persist_idle_now(")
        .nth(1)
        .and_then(|s| s.split("\n    fn poll_persist").next())
        .expect("persist_bg");
    assert!(
        !bg.contains("secrets.api_key.len"),
        "idle persist must not race a just-saved console key: {bg}"
    );
    assert!(
        bg.contains("grok_session") && bg.contains("grok_cwd"),
        "idle persist must notice a handshake session stamp: {bg}"
    );
    let snap = src
        .split("fn persist_snap(")
        .nth(1)
        .and_then(|s| s.split("fn persist_bg(").next())
        .expect("persist_snap");
    assert!(
        snap.contains("secrets: None"),
        "idle persist_snap must omit secrets.json: {snap}"
    );
    assert!(
            !snap.contains("msgs.clone()"),
            "persist_snap must copy the live pane into the thread once, not again into PersistSnap.msgs: {snap}"
        );
    assert!(
        snap.contains("t.messages = self.messages.clone()")
            && snap.contains("self.threads.clone()"),
        "persist must share the live pane Arc, then bump other threads: {snap}"
    );
    assert!(
        !snap.contains("parked_last") && !snap.contains("live_last"),
        "persist_snap must not recopy bodies when the live pane already is the parked Arc: {snap}"
    );
    assert!(
        snap.contains("self.hub.clone()") && !snap.contains("state_for_disk"),
        "persist_snap must not clone hub last_frame/snapshot on the UI thread: {snap}"
    );
    assert!(
        disk.contains("current_thread") && disk.contains("save_chat"),
        "persist must write chat.json from the snapped thread, not a second 8MB clone: {disk}"
    );
    let persist = src
        .split("fn persist(&mut self)")
        .nth(1)
        .and_then(|s| s.split("fn persist_snap(").next())
        .expect("persist");
    assert!(
        persist.contains("snap.secrets = Some") && persist.contains("submit_full"),
        "foreground persist must hand secrets to PersistService: {persist}"
    );
    assert!(
        persist.contains("persist_svc") && !persist.contains("thread::spawn"),
        "foreground persist must leave the UI thread via PersistService: {persist}"
    );
    let svc = fn_src(&src, "persist_service_worker");
    assert!(
        svc.contains("write_persist_disk_in_order") && svc.contains("io.lock()"),
        "PersistService worker must take persist_io before threads.json: {svc}"
    );
    assert!(
            persist.contains("persist_idle_key") && persist.contains("persist_idle_now"),
            "persist must bump the idle key or persist_bg clones every thread again 2s later: {persist}"
        );
    assert!(
        bearer.contains("persist_io") && bearer.contains("secrets::save"),
        "OAuth refresh must take persist_io before writing secrets.json: {bearer}"
    );
    let bearer_spawn = bearer
        .find("thread::spawn")
        .expect("oauth persist must leave the UI thread");
    let bearer_save = bearer.find("secrets::save").expect("oauth persist writes");
    assert!(
        bearer_spawn < bearer_save,
        "OAuth refresh must not freeze the cabin writing secrets.json: {bearer}"
    );
    let kick = fn_src(&src, "kick_model");
    assert!(
        kick.contains("next_chat_image")
            && kick.contains("kick_native_turn(&last_user, image.as_deref()"),
        "a plus-button still must ride the native turn: {kick}"
    );
    assert!(
        kick.contains("consume_attach") && kick.contains("attach_url"),
        "follow-up kicks must leave the attached image for the next send: {kick}"
    );
    let send_attach = src
        .split("fn send_chat(")
        .nth(1)
        .and_then(|s| s.split("fn send_followup_turn").next())
        .expect("send_chat attach");
    assert!(
        send_attach.contains("attach_prompt_line") && send_attach.contains("attach_name"),
        "the visible user turn must mention the attached still: {send_attach}"
    );
    let cwd = fn_src(&src, "grok_cwd");
    assert!(
            cwd.contains("cabin_session_cwd"),
            "ACP cwd must be the bound project or ~/GrokHub-Work, and History must list that same directory: {cwd}"
        );
    assert!(
        !cwd.contains("unwrap_or_else(|_| self.grok_cwd())"),
        "listing HOME drops a dialogue session stored under the chat cwd: {cwd}"
    );
    assert!(
        !cwd.contains("current_dir"),
        "unbound ACP must not inherit the overlay or cargo tree cwd: {cwd}"
    );
    assert!(
        !cwd.contains("ensure_session_cwd"),
        "ACP cwd lookup must not probe disk on the UI thread: {cwd}"
    );
    let saved = src
        .split("fn apply_single_turn(")
        .nth(1)
        .and_then(|s| s.split("\n    pub(super) fn ").next())
        .expect("apply_single_turn");
    assert!(
        saved.contains("session_saved")
            && (saved.contains("grok_session = Some")
                || saved.contains("bind_reported_grok_session")),
        "a dialogue session stays on the cabin chat, which is History: {saved}"
    );
    assert!(
        !saved.contains("reload_grok_sessions"),
        "a finished headless turn must not rebuild History from grok sessions list: {saved}"
    );
    let bind = src
        .split("Slash::ProjectBind(path)")
        .nth(1)
        .and_then(|s| s.split("Slash::ProjectClear").next())
        .expect("project bind");
    assert!(
        bind.contains("resolve_bind_path"),
        "/project bind . must not inherit the cabin process cwd: {bind}"
    );
    assert!(
        bind.contains("halt_in_flight") && !bind.contains("self.acp.is_some()"),
        "/project bind during handshake must halt, not only drop a live ACP handle: {bind}"
    );
    assert!(
            bind.contains("grok_cwd = None") && bind.contains("grok_session = None"),
            "/project bind must forget the thread worktree or the next send stays in a History tree: {bind}"
        );
    assert!(
        bind.contains("self.persist_cfg()")
            && bind.contains("self.flush_projects()")
            && bind.contains("self.persist()")
            && bind.contains("tree_changed"),
        "/project bind to the current tree must not clone every thread: {bind}"
    );
    let clear = src
        .split("Slash::ProjectClear =>")
        .nth(1)
        .and_then(|s| s.split("Slash::ProjectShow =>").next())
        .expect("ProjectClear handshake");
    assert!(
        clear.contains("self.acp = None"),
        "/project clear must drop the engine so the next turn starts in the new tree: {clear}"
    );
    assert!(
        clear.contains("halt_in_flight") && !clear.contains("self.acp.is_some()"),
        "/project clear during handshake must halt, not only drop a live ACP handle: {clear}"
    );
    assert!(
            clear.contains("grok_cwd = None") && clear.contains("grok_session = None"),
            "/project clear must forget the thread worktree or the next send stays in a History tree: {clear}"
        );
    let sidebar = src
        .split("fn bind_project_id(")
        .nth(1)
        .and_then(|s| s.split("fn make_project(").next())
        .expect("bind_project_id");
    assert!(
        sidebar.contains("self.acp = None"),
        "sidebar bind must drop the engine so the next turn starts in the new tree: {sidebar}"
    );
    assert!(
        sidebar.contains("halt_in_flight") && !sidebar.contains("self.acp.is_some()"),
        "sidebar bind during handshake must halt, not only drop a live ACP handle: {sidebar}"
    );
    assert!(
        !sidebar.contains("grok_session = None") && !sidebar.contains("messages.clear"),
        "sidebar bind must keep the chat transcript and its headless session: {sidebar}"
    );
    let bind_spawn = sidebar
        .find("thread::spawn")
        .expect("bind mkdir must leave the UI thread");
    let bind_mkdir = sidebar.find("create_dir_all").expect("create_dir_all");
    assert!(
        bind_spawn < bind_mkdir,
        "sidebar bind must not freeze the cabin creating the project folder: {sidebar}"
    );
    assert!(
        sidebar.contains("self.persist_cfg()")
            && sidebar.contains("self.persist()")
            && sidebar.contains("tree_changed"),
        "re-clicking a bound project must not clone every thread just to write app.json: {sidebar}"
    );
    let room = src
        .split("Slash::Room(name)")
        .nth(1)
        .and_then(|s| s.split("Slash::Export =>").next())
        .expect("Room");
    assert!(
        room.contains("self.acp = None"),
        "/room must drop the engine so the next turn starts in the new tree: {room}"
    );
    assert!(
        room.contains("halt_in_flight") && !room.contains("self.acp.is_some()"),
        "/room during handshake must halt, not only drop a live ACP handle: {room}"
    );
    assert!(
        room.contains("grok_cwd = None") && room.contains("grok_session = None"),
        "/room must forget the thread worktree or the next send stays in a History tree: {room}"
    );
    assert!(
        room.contains("self.persist_cfg()")
            && room.contains("self.flush_projects()")
            && room.contains("self.persist()")
            && room.contains("tree_changed"),
        "/room to the current tree must not clone every thread: {room}"
    );
    let open = fn_src(&src, "open_grok_session");
    assert!(
        open.contains("grok_cwd"),
        "History open must remember the session worktree: {open}"
    );
    assert!(
        open.contains("apply_switch_thread") && open.contains("self.persist()"),
        "opening a grok session must not clone every thread twice: {open}"
    );
    let show = fn_src(&src, "kick_session_show");
    let show_spawn = show
        .find("thread::spawn")
        .expect("the transcript read must leave the UI thread");
    let show_read = show.find("read_file_capped").expect("read_file_capped");
    assert!(
        show_spawn < show_read
            && !show.contains("read_to_string")
            && !show.contains("find_grok")
            && !show.contains("show_session"),
        "opening a CLI-era session reads its saved file off the UI thread, never the CLI: {show}"
    );
    let open_body = src
        .split("fn open_grok_session(")
        .nth(1)
        .and_then(|s| s.split("fn kick_session_show(").next())
        .expect("open_grok_session body");
    assert!(
        open_body.contains("kick_session_show") && open_body.contains("grok_show_pending = true"),
        "opening a session bound by pin or rename must still load the transcript: {open_body}"
    );
    let kick = fn_src(&src, "kick_imagine");
    assert!(
        kick.contains("imagine_cred()")
            && kick.contains("console_key()")
            && !kick.contains("self.bearer()")
            && !kick.contains("has_key()"),
        "Imagine uses imagine_cred (keychain, then Account, then console key): {kick}"
    );
    assert!(
        kick.contains("bump_usage(&mut self.usage, \"imagine\")"),
        "the imagine bucket has to count something for /usage to mean anything: {kick}"
    );
    let tokens = src
        .split("fn merge_grok_usage(")
        .nth(1)
        .and_then(|s| s.split("fn persist_usage(").next())
        .expect("merge_grok_usage");
    assert!(
        tokens.contains("token_delta") && tokens.contains("add_tokens"),
        "Grok reports session totals — the day must bank the delta: {tokens}"
    );
    // Split so this assertion is not its own counter-example.
    let direct = format!("self.grok_usage{}", ".merge(&");
    assert!(
        !src.contains(&direct),
        "every usage merge goes through merge_grok_usage or the day loses tokens"
    );
    assert_eq!(
        kick.matches("self.bearer()").count(),
        0,
        "Imagine must not fall back to CLI login via bearer(): {kick}"
    );
    let cred = fn_src(&src, "imagine_cred");
    assert!(
        cred.contains("native_account_access")
            && cred.contains("imagine_oauth_preferred")
            && cred.contains("choose_imagine_bearer"),
        "imagine_cred must try Imagine keychain, then Account, then console key: {cred}"
    );
    let imag = src
        .split("fn ui_imagine(")
        .nth(1)
        .and_then(|s| s.split("fn ui_imagine_bar(").next())
        .expect("ui_imagine");
    assert!(
        imag.contains("imagine_stage_visible") && imag.contains("imagine_stage("),
        "Imagine must paint a generating/result box: {imag}"
    );
    assert!(
        imag.contains("imagine_masonry") && imag.contains("imagine-scroll"),
        "the photogif wall stays reachable by scrolling under the generating box: {imag}"
    );
    assert!(
        imag.contains("ImagineToolboxDock::Bottom")
            && imag.contains("imagine-lightbox")
            && imag.contains("start_imagine_save")
            && imag.contains("play_imagine_media")
            && imag.contains("stage_hit.play")
            && imag.contains("imagine_is_video_path"),
        "send docks the chat box; generated stills expand and save: {imag}"
    );
    assert!(
        imag.contains("!grokhub_core::imagine_is_video_path(&last)"),
        "a ready video must play, not open the still lightbox: {imag}"
    );
    assert!(
        src.contains("pin_generation_to_wall") && src.contains("wall_gif_from_generation"),
        "generated stills must land on the Imagine wall"
    );
    let poll = src
        .split("fn poll_acp(")
        .nth(1)
        .and_then(|s| s.split("fn finish_acp_turn(").next())
        .expect("poll_acp");
    assert!(
        poll.contains("grok_session") && poll.contains("session_id"),
        "ACP Ready must stamp the grok session id: {poll}"
    );
    assert!(
        poll.contains("cancelled") && poll.contains("!self.running"),
        "session/cancel Done must not finish a live or redirected turn: {poll}"
    );
    assert!(
        poll.contains("answer_permission") && poll.contains("AcpEvent::Err"),
        "ACP Err must deny leftover Ask or the next send hangs: {poll}"
    );
    assert!(
        poll.contains("chat_job_thread"),
        "ACP Ready must stamp the job thread, not whichever tab is visible: {poll}"
    );
    let done = poll
        .split("AcpEvent::Done")
        .nth(1)
        .and_then(|s| s.split("AcpEvent::Err").next())
        .expect("AcpEvent::Done");
    assert!(
        done.contains("mem::take")
            && !done.contains("stream_buf.clone()")
            && !done.contains("thought_buf.clone()"),
        "ACP Done must take the stream buffers, not clone an 8MB complete on the UI thread: {done}"
    );
    let err = poll.split("AcpEvent::Err").nth(1).expect("err arm");
    assert!(
        !err.contains("grok_session = None")
            && err.contains("self.acp = None")
            && err.contains("maybe_continue_ptt"),
        "agent exit must keep the attached Grok Build session id and resume PTT: {err}"
    );
    let show = fn_src(&src, "poll_session_show");
    assert!(
        show.contains("persist_bg") && show.contains("parse_session_markdown"),
        "History show must persist the transcript, not wait for the next idle tick: {show}"
    );
}

#[test]
fn history_rail_uses_session_names_and_can_delete() {
    let src = cabin_src();
    let rail = src
        .split("id_salt(\"rail-history\")")
        .nth(1)
        .and_then(|s| s.split("fn cached_chat_views(").next())
        .expect("rail-history");
    assert!(
        rail.contains("chat_section_indices") && rail.contains("TabAct::Switch"),
        "sidebar History is cabin chats, not grok sessions list: {rail}"
    );
    assert!(
        !rail.contains("discover_session_files") && !rail.contains("reload_grok_sessions"),
        "sidebar History must not walk session dirs or relist the CLI: {rail}"
    );
    assert!(
        rail.contains("TabAct::Delete") && rail.contains("button(\"Delete\")"),
        "sidebar chat rows must offer Delete: {rail}"
    );
    assert!(
        rail.contains("session_list_order")
            && rail.contains("\"Unpin\"")
            && rail.contains("\"Pin\"")
            && rail.contains("\"Rename\""),
        "sidebar chat rows must pin and rename: {rail}"
    );
    assert!(
        rail.contains("is_background_history_title"),
        "sidebar History must drop workboard summarize and similar: {rail}"
    );
    let side = include_str!("sidebar.rs");
    assert!(
        !side.contains("rail-archived")
            && !side.contains("RichText::new(\"Archived\")")
            && !side.contains("No archived chats"),
        "sidebar must not paint an Archived section: {side}"
    );
    let page = src
        .split("crate::cards::section_label(ui, \"Chats\")")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("history chats section");
    assert!(
        page.contains("thread_rail_title") && page.contains("chat_section_indices"),
        "History page must paint cabin chats: {page}"
    );
    assert!(
        page.contains("Delete") && page.contains("delete_thread_at"),
        "History page must delete the cabin chat: {page}"
    );
    assert!(
        page.contains("is_background_history_title"),
        "History page must not list workboard summarize: {page}"
    );
    assert!(
        !page.contains("Archived chats")
            && !page.contains("No archived chats")
            && !page.contains("\"Unarchive\""),
        "History page must not paint an Archived drawer: {page}"
    );
    let dta = fn_src(&src, "delete_thread_at");
    assert!(
        dta.contains("delete_session") && !dta.contains("grok_user_stdout"),
        "deleting a chat drops its native session and never runs the CLI: {dta}"
    );
    assert!(
        page.contains("self.nav = Nav::History"),
        "deleting a History chat must keep the See all pane: {page}"
    );
    let hist = src
        .split("fn ui_history(")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("ui_history");
    assert!(
        hist.contains("Delete all") && hist.contains("delete_all_history"),
        "History See all must offer Delete all: {hist}"
    );
    let poll = src
        .split("fn poll_single(")
        .nth(1)
        .and_then(|s| s.split("fn upsert_stream_assistant(").next())
        .expect("poll_single");
    assert!(
        poll.contains("apply_auto_title"),
        "a finished turn must name the tab from the session, not leave Chat: {poll}"
    );
    assert!(
        poll.contains("GrokPEvent::Usage")
            && poll.contains("GrokPEvent::Compact")
            && poll.contains("thinking_status")
            && poll.contains("turn_footer"),
        "1.0.13 stream must paint usage, compact, and a turn footer: {poll}"
    );
    assert!(
        poll.contains("GrokPEvent::Recovering") && poll.contains("apply_compact_status"),
        "1.0.13 truncation/5xx recovery and compact errors must not kill the turn: {poll}"
    );
    assert!(
        poll.contains("retry_status_line"),
        "1.0.14 retry status must show a short reason: {poll}"
    );
    assert!(
        poll.contains("scheduled_perm = false"),
        "a finished or failed grok -p turn must drop scheduled_perm: {poll}"
    );
    let deleted = src
        .split("fn delete_thread_at")
        .nth(1)
        .and_then(|s| s.split("fn send_chat").next())
        .expect("delete_thread_at");
    assert!(
        deleted.contains("grokhub_agent::delete_session"),
        "deleting a History chat must drop its native session: {deleted}"
    );
    let title = fn_src(&src, "thread_rail_title");
    assert!(
        title.contains("preferred_history_title"),
        "rail titles must prefer the Grok Build session name: {title}"
    );
}

#[test]
fn refresh_chips_does_not_rebuild_every_frame() {
    let src = cabin_src();
    let chips = src
        .split("fn refresh_chips(")
        .nth(1)
        .and_then(|s| s.split("fn spawn_chip_llm(").next())
        .expect("refresh_chips");
    assert!(
        chips.contains("chip_paint_key") && chips.contains("return;"),
        "chips must not clone the transcript and walk other threads on every paint: {chips}"
    );
    let pairs = chips.find("chat_pairs").expect("chip chat_pairs");
    assert!(
        chips[..pairs].contains("self.running") && chips[..pairs].contains("return"),
        "a growing stream must not clone the transcript to rebuild chips: {chips}"
    );
    assert!(
        chips.contains("chip_chat_pairs") || chips.contains("chip_scan"),
        "chip rebuild must not clone an 8MB complete into chat_pairs: {chips}"
    );
    assert!(
        chips.contains("host_on: false"),
        "empty chips must not inject HOST_CMD host_chips: {chips}"
    );
    assert!(
        chips.contains("grok_connected: self.cabin_signed_in()"),
        "Connect chip must follow signed-in, not a PATH grok: {chips}"
    );
    assert!(
        !chips.contains("grok_connected: self.llm_ready()"),
        "PATH grok must not hide Connect Grok: {chips}"
    );
}

#[test]
fn imagine_visit_ranks_home_chips() {
    let src = cabin_src();
    let update = src
        .split("fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame)")
        .nth(1)
        .and_then(|s| s.split("fn ui_sidebar(").next())
        .expect("update");
    let tick = update
        .find("tick_home_surface")
        .expect("tick_home_surface in update");
    let chips = update
        .find("refresh_chips")
        .expect("refresh_chips in update");
    assert!(
        tick < chips,
        "Imagine/Skills visits must record last_surface before chips rank: {update}"
    );
    let tick_fn = src
        .split("fn tick_home_surface(")
        .nth(1)
        .and_then(|s| s.split("fn cabin_signed_in(").next())
        .expect("tick_home_surface");
    assert!(
        tick_fn.contains("home_surface_from_nav") && tick_fn.contains("remember_home_surface"),
        "sidebar / palette / slash page visits must rank Imagine and Skills: {tick_fn}"
    );
    assert!(
        tick_fn.contains("nav_id()"),
        "surface must follow the live page, not only a chip click: {tick_fn}"
    );
}

#[test]
fn speak_reply_does_not_clone_an_8mb_complete() {
    let src = cabin_src();
    let speak = src
        .split("fn speak_reply(")
        .nth(1)
        .and_then(|s| s.split("fn poll_eyes_cap(").next())
        .expect("speak_reply");
    let clone = speak.find("to_string()").expect("tts clone");
    assert!(
        speak[..clone].contains("TEXT_FILE_CAP")
            || speak[..clone].contains("chip_scan")
            || speak[..clone].contains("take_ui"),
        "voice speak must not clone an 8MB complete onto the UI thread: {speak}"
    );
    assert!(
        speak.contains("voice_tts_script"),
        "TTS must strip THINKING: / thoughts before grok_tts: {speak}"
    );
}

#[test]
fn the_night_slot_runs_loops_and_clock_time_automations() {
    let src = cabin_src();
    let job = src
        .split("JobKind::Automations =>")
        .nth(1)
        .and_then(|s| s.split("JobKind::PulseRefresh =>").next())
        .expect("automations job");
    assert!(
        job.contains("self.tick_loops() || self.tick_night()"),
        "the Automations job owns both schedulers, loops first, and a 09:00 automation \
             still fires when no loop is due: {job}"
    );
    // The governor sets the cadence: neither keeps a private clock, so an idle
    // loop list cannot starve automations and nothing is scheduled twice.
    let night = src
        .split("fn tick_night(")
        .nth(1)
        .and_then(|s| s.split("fn poll_night_check(").next())
        .expect("tick_night");
    let loops = src
        .split("fn tick_loops(")
        .nth(1)
        .and_then(|s| s.split("fn poll_grok_loop(").next())
        .expect("tick_loops");
    for body in [night, loops] {
        assert!(
            !body.contains("last_auto_tick")
                && !body.contains("last_night_tick")
                && !body.contains(".elapsed()"),
            "no self-timing left in the night schedulers: {body}"
        );
    }
    assert!(
        !src.contains("last_auto_tick") && !src.contains("last_night_tick"),
        "the old debounce clocks are gone"
    );
}

#[test]
fn periodic_persist_leaves_the_ui_thread() {
    let src = cabin_src();
    let beat = src
        .split("fn tick_heartbeat")
        .nth(1)
        .and_then(|s| s.split("fn tick_anticipate").next())
        .expect("tick_heartbeat");
    assert!(
        beat.contains("persist_bg(") && !beat.contains("self.persist()"),
        "2s housekeep persist must not block the cabin: {beat}"
    );
    let paint = src
        .split("self.flush_window(ctx)")
        .nth(1)
        .and_then(|s| s.split("next_heartbeat_wait_ms").next())
        .expect("update persist");
    assert!(
        paint.contains("persist_bg(") && !paint.contains("self.persist()"),
        "2s paint persist must not block the cabin: {paint}"
    );
    let apply = src
        .split("fn apply_saved_geom(")
        .nth(1)
        .and_then(|s| s.split("fn capture_window(").next())
        .expect("apply_saved_geom");
    assert!(
        apply.contains("InnerSize") && apply.contains("OuterPosition"),
        "launch must apply the remembered inner size and outer position: {apply}"
    );
    let capture = src
        .split("fn capture_window(")
        .nth(1)
        .and_then(|s| s.split("fn flush_window(").next())
        .expect("capture_window");
    assert!(
        capture.contains("geom_can_remember") && capture.contains("apply_saved_geom"),
        "first frames must restore size/position, not clobber app.json: {capture}"
    );
    let show = src
        .split("fn show_from_tray(")
        .nth(1)
        .and_then(|s| s.split("fn poll_tray(").next())
        .expect("show_from_tray");
    assert!(
        show.contains("apply_saved_geom"),
        "Show cabin must restore size and position: {show}"
    );
    let exit = src
        .split("fn on_exit(")
        .nth(1)
        .and_then(|s| s.split("fn logic(").next())
        .expect("on_exit");
    assert!(
        exit.contains("config::save") && exit.contains("cfg.window")
            || exit.contains("config::save(&cfg)"),
        "SIGTERM must write the remembered window: {exit}"
    );
    let flush = fn_src(&src, "flush_window");
    assert!(
        flush.contains("submit_window_cfg")
            && flush.contains("persist_svc")
            && !flush.contains("thread::spawn"),
        "window geom must not freeze the cabin writing app.json: {flush}"
    );
    let bg = fn_src(&src, "persist_bg");
    assert!(
        bg.contains("submit_full") && bg.contains("persist_svc") && !bg.contains("thread::spawn"),
        "persist_bg must leave the UI thread via PersistService: {bg}"
    );
    let svc = include_str!("persist_service.rs").replace("\r\n", "\n");
    assert!(
        svc.contains("thread::spawn") && svc.contains("write_persist_disk_in_order"),
        "PersistService must own the one worker that writes threads.json: {svc}"
    );
    assert!(
        bg.contains("persist_idle_key") && bg.contains("return;"),
        "idle 2s persist must not clone every thread on the UI thread: {bg}"
    );
    let snap = bg.find("persist_snap").expect("persist_snap");
    assert!(
        bg[..snap].contains("self.running") && bg[..snap].contains("return"),
        "a growing stream must not clone every thread to persist an 8MB bubble: {bg}"
    );
    assert!(
        !bg[..snap].contains("geom_dirty"),
        "window drag must not clone every thread — flush_window owns geom: {bg}"
    );
    let idle_key = src
        .split("fn persist_idle_now(")
        .nth(1)
        .and_then(|s| s.split("fn persist_bg(").next())
        .expect("persist idle key");
    assert!(
            !idle_key.contains("projects_dirty"),
            "folder click must not clone every thread twice — persist_idle_key must ignore the dirty flag: {idle_key}"
        );
}

#[test]
fn chat_capture_leaves_the_ui_thread() {
    let src = cabin_src();
    let cap = src
        .split("fn capture_cabin_frame_this_turn")
        .nth(1)
        .and_then(|s| s.split("fn apply_job_fail").next())
        .expect("capture_cabin_frame_this_turn");
    let spawn = cap
        .find("thread::spawn")
        .expect("chat grim must leave the UI thread");
    let shot = cap.find("capture_data_url").expect("screen capture");
    let rows = cap.find("collect_rows").expect("desk scan");
    assert!(
        spawn < rows && rows < shot,
        "send/HostDone capture must not block the cabin: {cap}"
    );
    let ask_kick = fn_src(&src, "kick_model");
    assert!(
        ask_kick.contains("pending_kick") && ask_kick.contains("kick_cap_rx"),
        "kick_model must wait for the off-thread frame instead of blocking: {ask_kick}"
    );
    assert!(
        ask_kick
            .trim_end()
            .trim_end_matches('}')
            .trim_end()
            .ends_with(
                "self.kick_native_turn(&last_user, image.as_deref(), &raw_ask, &thread_label);"
            ),
        "every turn goes to the native engine: {ask_kick}"
    );
    for gone in [
        "spawn_grok_p_stream",
        "uses_acp",
        "scheduled_flags",
        "route_gb_turn(",
    ] {
        assert!(!ask_kick.contains(gone), "{gone}: {ask_kick}");
    }
    assert!(
        ask_kick.contains("apply_skill_follow") && ask_kick.contains("active_skill_follow"),
        "selecting a skill must inject the follow block into grok -p / ACP: {ask_kick}"
    );
}

#[test]
fn scheduled_night_loop_and_send_inherit_permission_mode() {
    let src = cabin_src();
    let fire_loop = fn_src(&src, "fire_loop");
    let native_src = include_str!("native_unattended.rs").replace("\r\n", "\n");
    let native_loop = fn_src(&native_src, "spawn_native_loop");
    assert!(
        fire_loop.contains("spawn_native_loop")
            && native_loop.contains("scheduled_gate(self, self.session_mode)"),
        "loop spawn must read the composer PermissionMode pill: {native_loop}"
    );
    let gate = fn_src(&native_src, "scheduled_gate");
    assert!(
        gate.contains("cabin.permission_mode") && gate.contains("attended: false"),
        "{gate}"
    );
    assert!(
        !fire_loop.contains("\"--always-approve\""),
        "Ask must not silent always-approve a loop: {fire_loop}"
    );
    let fire_night = fn_src(&src, "fire_night");
    let start = fn_src(&src, "start_scheduled_run");
    assert!(
        start.contains("start_bg_task"),
        "night runs in its own background process: {start}"
    );
    let bg = fn_src(&src, "native_bg_gate");
    assert!(
        bg.contains("self.permission_mode") && bg.contains("attended: false"),
        "a background run must inherit PermissionMode, not a separate yolo path: {bg}"
    );
    let send_at = fire_night.find("start_scheduled_run").expect("night start");
    let ran_at = fire_night.rfind("mark_auto_ran").expect("night mark ran");
    assert!(
        send_at < ran_at,
        "night must mark ran after the run started, not before: {fire_night}"
    );
    let send_block = &fire_night[send_at..];
    assert!(
        send_block.contains("Ok(_)")
            && send_block.contains("mark_auto_ran")
            && send_block.contains("mark_auto_skipped"),
        "a night run that did not start must skip, not retry every 5s: {fire_night}"
    );
    let inbox = fn_src(&src, "drain_inbox");
    assert!(
        inbox.contains("send_scheduled_chat"),
        "a queued /send task must inherit PermissionMode: {inbox}"
    );
    let anticipate = fn_src(&src, "tick_anticipate");
    assert!(
        anticipate.contains("send_scheduled_chat"),
        "heartbeat anticipate must inherit PermissionMode: {anticipate}"
    );
    let native_src = include_str!("native_engine.rs").replace("\r\n", "\n");
    let gate = fn_src(&native_src, "native_gate");
    assert!(
        gate.contains("PermissionMode::Ask => PermMode::Ask")
            && gate.contains("PermissionMode::Auto => PermMode::Auto")
            && gate.contains("PermissionMode::AlwaysApprove => PermMode::Always")
            && gate.contains("attended: !self.scheduled_perm"),
        "native turns must map Auto/Always and run scheduled Ask unattended (fail-closed): {gate}"
    );
    let kick = fn_src(&src, "kick_model");
    assert!(
        kick.contains("perm_always_confirm = None"),
        "a kick that clears perm_ask must also drop Always confirm: {kick}"
    );
    let scheduled = fn_src(&src, "send_scheduled_chat");
    assert!(
        scheduled.contains("scheduled_perm = true") && scheduled.contains("send_chat"),
        "scheduled enqueue must run the turn unattended: {scheduled}"
    );
    let fail_ask = fn_src(&src, "fail_turn_start");
    assert!(
        fail_ask.contains("maybe_continue_ptt"),
        "Ask deny must resume PTT: {fail_ask}"
    );
    assert!(
        fail_ask.contains("scheduled_perm = false"),
        "Ask deny must drop scheduled_perm so the next typed Ask uses ACP: {fail_ask}"
    );
    let finish = fn_src(&src, "finish_acp_turn");
    assert!(
        finish.contains("scheduled_perm = false"),
        "a finished chat turn must drop scheduled_perm so the next typed Ask uses ACP: {finish}"
    );
    let halt = fn_src(&src, "halt_in_flight");
    assert!(
        halt.contains("scheduled_perm = false"),
        "Stop must drop scheduled_perm: {halt}"
    );
    let kick_err = fn_src(&native_src, "kick_native_turn");
    assert!(
        kick_err.matches("self.fail_turn_start(").count() == 3,
        "a native turn that fails to start drops scheduled_perm and resumes PTT: {kick_err}"
    );
    let poll_acp = fn_src(&src, "poll_acp");
    let err = poll_acp
        .split("AcpEvent::Err")
        .nth(1)
        .expect("AcpEvent::Err");
    assert!(
        err.contains("maybe_continue_ptt"),
        "fatal ACP Err must resume PTT: {err}"
    );
}

#[test]
fn plus_upload_does_not_rescan_the_folder_every_frame() {
    let src = cabin_src();
    let overlay = src
        .split("fn ui_plus_overlays(")
        .nth(1)
        .and_then(|s| s.split("fn ui_imagine_overlays(").next())
        .expect("ui_plus_overlays");
    assert!(
        overlay.contains("cached_pick_entries") && !overlay.contains("Self::pick_entries"),
        "Upload window must not read_dir every paint: {overlay}"
    );
    let cache = src
        .split("fn cached_pick_entries(")
        .nth(1)
        .and_then(|s| s.split("fn ui_plus_overlays(").next())
        .expect("cached_pick_entries");
    assert!(
        cache.contains("pick_cache") && cache.contains("pick_entries("),
        "folder listing must reuse the last scan until pick_dir changes: {cache}"
    );
    let cache_spawn = cache
        .find("thread::spawn")
        .expect("listing must leave the UI thread");
    let cache_walk = cache.find("pick_entries(").expect("pick_entries");
    assert!(
        cache_spawn < cache_walk,
        "Upload folder listing must not read_dir on the UI thread: {cache}"
    );
    let upload = src
        .split("PlusAct::Upload =>")
        .nth(1)
        .and_then(|s| s.split("PlusAct::Paste =>").next())
        .expect("Upload");
    let spawn = upload.find("thread::spawn").expect("picker worker");
    let pick = upload.find("pick_file()").expect("native picker");
    let load = upload.find("plus_from_path").expect("decode off-thread");
    assert!(
        spawn < pick && pick < load && upload.contains("pick_rx") && !upload.contains("apply_path"),
        "zenity/kdialog and JPEG decode must not freeze the cabin on plus-upload: {upload}"
    );
    let paste = src
        .split("PlusAct::Paste =>")
        .nth(1)
        .and_then(|s| s.split("fn poll_pick(").next())
        .expect("Paste");
    let paste_spawn = paste.find("thread::spawn").expect("clipboard worker");
    let clip = paste.find("clipboard_image()").expect("clipboard image");
    assert!(
        paste_spawn < clip
            && paste.contains("clipboard_once")
            && paste.contains("plus_from_path")
            && !paste.contains("apply_path"),
        "xclip/wl-paste and JPEG decode must not freeze the cabin on plus-paste: {paste}"
    );
    let poll = src
        .split("fn poll_pick(")
        .nth(1)
        .and_then(|s| s.split("fn apply_clipboard(").next())
        .expect("poll_pick");
    assert!(
        poll.contains("apply_plus_ready")
            && poll.contains("file_pick")
            && !poll.contains("load_image_data_url"),
        "plus-upload worker must land the still or fall back to the in-app picker: {poll}"
    );
    assert!(
        src.contains("self.poll_pick()"),
        "plus-upload worker must be polled each frame"
    );
    assert!(
        overlay.contains("start_plus_path") && !overlay.contains("apply_path"),
        "in-app Upload clicks must decode off the UI thread: {overlay}"
    );
}

#[test]
fn recipe_reshoot_leaves_the_ui_thread() {
    let src = cabin_src();
    let replay = src
        .split("fn replay_recipe(")
        .nth(1)
        .and_then(|s| s.split("fn speak_reply").next())
        .expect("replay_recipe");
    let spawn = replay
        .find("thread::spawn")
        .expect("recipe replay must leave the UI thread");
    let rows = replay.find("collect_rows").expect("desk scan");
    let shot = replay.find("capture_data_url").expect("screen capture");
    assert!(
        spawn < rows && spawn < shot,
        "recipe replay must not block the cabin: {replay}"
    );
    assert!(
        replay.contains("lock_titles") && replay.contains("lock_blocks_hands"),
        "recipe reshoot must still gate on lock windows: {replay}"
    );
}

#[test]
fn live_room_captures_off_the_ui_thread() {
    let src = cabin_src();
    let live = src
        .split("fn live_room")
        .nth(1)
        .and_then(|s| s.split("fn tick_mid_thought").next())
        .expect("live_room");
    let spawn = live
        .find("thread::spawn")
        .expect("grim/ffmpeg must leave the UI thread");
    let shot = live.find("capture_data_url").expect("screen capture");
    let cam = live.find("capture_webcam").expect("webcam");
    assert!(
        spawn < shot && spawn < cam,
        "presence capture must not block the cabin: {live}"
    );
    assert!(
        live.contains("try_recv") && live.contains("live_cap_rx"),
        "UI thread must apply one in-flight frame without stacking grim: {live}"
    );
    assert!(
        live.contains("collect_rows")
            && live.contains("lock_titles")
            && live.contains("should_send_screenshot"),
        "lock and title gates stay on the UI thread: {live}"
    );
    assert!(
        !live.contains("webcam_url") && !live.contains("cam:"),
        "live room must not land an unbounded webcam data URL on the UI thread: {live}"
    );
}

#[test]
fn chat_side_effects_keep_the_origin_thread() {
    let src = cabin_src();
    assert!(
        src.contains("let origin = self.chat_job_thread.take()"),
        "host/connector/imagine after Chat must rebind the origin tab"
    );
    let agent_job = src
        .split("struct AgentJob")
        .nth(1)
        .and_then(|s| s.split("fn listen_turn").next())
        .expect("AgentJob");
    assert!(
        agent_job.contains("thread_id"),
        "Queue jobs must remember the origin thread: {agent_job}"
    );
    let listen = src
        .split("fn listen_turn(")
        .nth(1)
        .and_then(|s| s.split("fn fit_rail_label").next())
        .expect("listen_turn");
    let wav_read = listen.find("std::fs::read(&wav)").expect("wav read");
    assert!(
        listen.contains("IMAGE_FILE_CAP")
            && listen.find("IMAGE_FILE_CAP").expect("wav cap") < wav_read,
        "voice STT must not slurp a huge wav: {listen}"
    );
    let queue = fn_src(&src, "ui_agents");
    let queued = fn_src(&src, "start_queued_job");
    assert!(
        !queue.contains("send_chat")
            && queue.contains("kick_model")
            && queued.contains("chat_job_thread")
            && queued.contains("push_bound_msg")
            && !queued.contains("kick_model"),
        "Queue Run must kick the origin thread, not send_chat on the visible tab: {queue} {queued}"
    );
    assert!(
        src.contains("finish_hub_dispatch"),
        "a queued /send task must complete its hub row"
    );
    assert!(
        src.contains("hub_dispatch_ok(&text)"),
        "GOAL_BLOCKED must not complete a queued task as done"
    );
    assert!(
        src.contains("oauth_access_live"),
        "expired OAuth without refresh must not hide a console key"
    );
    assert!(
        src.contains("next_oauth_poll_secs"),
        "Settings OAuth must honor interval and slow_down"
    );
    let cmds = src
        .split("fn run_cmds")
        .nth(1)
        .and_then(|s| s.split("fn run_connector").next())
        .expect("run_cmds");
    assert!(
        cmds.contains("if self.chat_job_thread.is_none()"),
        "run_cmds must not retarget a job that started on another tab"
    );
    assert!(
        cmds.contains("push_bound_msg"),
        "blocked host receipts must stay on the job thread"
    );
    assert!(
        cmds.contains("host_working_dir(&self.cfg.project_dir)")
            && cmds.contains("run_host_stream"),
        "host shell must start in the bound project, not the cabin process cwd: {cmds}"
    );
    assert!(
        cmds.contains("parse_computer_op") && !cmds.contains("parse_computer_cmd_loose"),
        "HOST_CMD / Command-pane type cargo must stay shell, not desktop type-in: {cmds}"
    );
    let ret = cmds.find("return blocked;").expect("return blocked");
    let rewind = cmds.rfind("is_rewind_copy_cmd").expect("rewind copy");
    assert!(
        ret < rewind && cmds[ret..rewind].contains("self.persist()"),
        "mixed blocked+allowed host must persist block receipts before spawn: {cmds}"
    );
    let host_done = fn_src(&src, "poll_job");
    assert!(
        host_done.contains("pending_connectors") && host_done.contains("self.kick_model(false)"),
        "an all-blocked host plan must still kick the model after connectors"
    );
    let halt = src
        .split("fn halt_work")
        .nth(1)
        .and_then(|s| s.split("fn drain_inbox").next())
        .expect("halt_work");
    assert!(
        halt.contains("finish_hub_dispatch"),
        "Stop / tray halt must complete a claimed /send task"
    );
    assert!(
        src.contains("self.finish_hub_dispatch(worker_gone_status(), false)"),
        "a dropped worker must fail the claimed /send task"
    );
    assert!(
        src.contains("requeue_claimed_for"),
        "unstick claimed rows on boot"
    );
    let inbox = src
        .split("fn drain_inbox")
        .nth(1)
        .and_then(|s| s.split("fn finish_hub_dispatch").next())
        .expect("drain_inbox");
    assert!(
        inbox.contains("pending_hub_task.is_some()"),
        "do not claim a second /send task while one is still pending: {inbox}"
    );
    assert!(
        inbox.contains("land_on_real_chat"),
        "a claimed /send task must not land on Scratch: {inbox}"
    );
    assert!(
        src.contains("night_counts_run"),
        "a night replay that did not start must not consume the slot"
    );
    let fire_night = src
        .split("fn fire_night")
        .nth(1)
        .and_then(|s| s.split("fn tick_review").next())
        .expect("fire_night");
    assert!(
        fire_night.contains("mark_auto_skipped"),
        "a skipped night slot is marked: {fire_night}"
    );
    let counts = fire_night
        .find("night_counts_run")
        .expect("night_counts_run");
    assert!(
        fire_night[counts..].contains("mark_auto_skipped"),
        "a missing night recipe must skip the slot, not hammer every 5s: {fire_night}"
    );
    let bump = fire_night.find("bump_usage").expect("night usage");
    let usage_save = fire_night
        .find("persist_usage")
        .expect("night persist_usage");
    assert!(
        bump < usage_save && !fire_night.contains("self.persist()"),
        "a night replay must stamp usage.json without cloning every thread: {fire_night}"
    );
    assert!(
        !fire_night.contains("land_on_real_chat") && !fire_night.contains("self.nav"),
        "a night job runs in the background and never opens the chat page: {fire_night}"
    );
    assert!(!fire_night.contains("can_agent"), "{fire_night}");
    let night_check = src
        .split("fn poll_night_check")
        .nth(1)
        .and_then(|s| s.split("fn spawn_night_check").next())
        .expect("poll_night_check");
    assert!(
        night_check.contains("night_check_may_fire"),
        "a finished night check must not halt a live job: {night_check}"
    );
    let send = src
        .split("fn send_chat")
        .nth(1)
        .and_then(|s| s.split("fn send_followup_turn").next())
        .expect("send_chat");
    let redirect = send
        .split("ChatSendKind::Redirect")
        .nth(1)
        .and_then(|s| s.split("ChatSendKind::Fresh").next())
        .expect("redirect");
    assert!(
        !redirect.contains("content.clone()"),
        "redirect must not clone the transcript to read the last user turn: {redirect}"
    );
    let slash_at = send.find("parse_slash").expect("parse_slash");
    let kind_at = send.find("chat_send_kind").expect("chat_send_kind");
    assert!(
        slash_at < kind_at,
        "/compact during a live job must stay local, not become a redirect: {send}"
    );
    assert!(
        send.contains("unknown_cabin_slash") && send.contains("Unknown command"),
        "unknown /project binding must stay local, not go to Grok: {send}"
    );
    let compact = src
        .split("Slash::Compact =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Skill").next())
        .expect("Compact");
    assert!(
        compact.contains("stamp_current_access") || compact.contains("accessed_ms"),
        "/compact must bump accessed_ms or /sync LWW can restore the dropped turns: {compact}"
    );
    assert!(
        !compact.contains("content.clone()"),
        "/compact must not clone an 8MB pane: {compact}"
    );
    let pushed = send.find("live_mut().push").expect("user turn");
    let saved = send.find("self.persist()").expect("send persist");
    assert!(
        send[pushed..saved].contains("stamp_current_access")
            || send[pushed..saved].contains("accessed_ms"),
        "a sent turn must bump accessed_ms or /sync LWW can drop it: {send}"
    );
    let fail = src
        .split("fn apply_job_fail")
        .nth(1)
        .and_then(|s| s.split("fn queue_update").next())
        .expect("apply_job_fail");
    assert!(
        fail.contains("accessed_ms") || fail.contains("stamp_current_access"),
        "a job error on the origin thread must bump accessed_ms or /sync LWW can drop it: {fail}"
    );
    assert!(
        fail.contains("apply_job_error") && !fail.contains("content.clone()"),
        "a job error must not clone an 8MB pane to replace the last assistant: {fail}"
    );
    let queued = src
        .split("fn queue_update(")
        .nth(1)
        .and_then(|s| s.split("fn restart_after_update").next())
        .expect("queue_update");
    assert!(
        queued.contains("queue_cabin_update")
            && !queued.contains("update_cmds_for")
            && !queued.contains("config::save")
            && !queued.contains("persist_snap"),
        "/update must use the same pending plan as the chip: {queued}"
    );
    assert!(
        src.contains("settings_update_hint")
            && src.contains("queue_cabin_update")
            && src.contains("UPDATE_CHECK_EVERY"),
        "one Update control must describe the cabin update and recheck on the 2h interval: {src}"
    );
    for gone in [
        "Install Grok Build CLI",
        "queue_grok_cli_install",
        "begin_grok_install",
    ] {
        assert!(
            !src.contains(gone),
            "GrokHub never installs the Grok Build CLI: {gone}"
        );
    }
    let queued_cli = src
        .split("fn queue_cabin_update(")
        .nth(1)
        .and_then(|s| s.split("fn note_cabin_update_landed(").next())
        .expect("queue_cabin_update");
    assert!(
        queued_cli.contains("update_cmds_for_host_in(src.as_deref(), cfg!(windows), channel)")
            && queued_cli.contains("start_overlay_update")
            && !queued_cli.contains("UpdatePending")
            && !queued_cli.contains("grok update")
            && queued_cli.contains("self.open_update_overlay()")
            && queued_cli.find("self.open_update_overlay()").unwrap()
                < queued_cli.find("update_cmds_for_host_in").unwrap()
            && queued_cli.contains("self.persist_cfg()")
            && queued_cli.contains("overlay_clone_usable")
            && !queued_cli.contains("config::save")
            && !queued_cli.contains("persist_snap")
            && !queued_cli.contains("Set Settings → source")
            && !queued_cli.contains("--stable")
            && !queued_cli.contains("begin_grok_install")
            && !queued_cli.contains("update_hint(UpdatePending::None)"),
        "the one Update control overlays the cabin, even when the probe missed: {queued_cli}"
    );
    let overlay = src
        .split("fn start_overlay_update(")
        .nth(1)
        .and_then(|s| s.split("fn drain_queued_update(").next())
        .expect("start_overlay_update");
    let settings_at = overlay.find("Nav::Settings").expect("settings");
    let running_at = overlay.find("if self.running").expect("running");
    assert!(
        settings_at < running_at
            && overlay.contains("queued_overlay")
            && src.contains("drain_queued_update"),
        "a busy chip click must open Settings and queue the update: {overlay}"
    );
    let boot = src
        .split("pub fn new(hidden: bool)")
        .nth(1)
        .and_then(|s| s.split("fn apply_saved_geom(").next())
        .expect("Cabin::new");
    assert!(
        boot.contains("begin_update_probe"),
        "boot must check GitHub Latest for the in-app chip: {boot}"
    );
    assert!(
        src.contains("titlebar_update_chip")
            && src.contains("update_chip_label")
            && src.contains("queue_cabin_update")
            && !src.contains(concat!("Update ", "available")),
        "the titlebar chip names CLI, cabin, or both and runs that update: {src}"
    );
    let flush_p = src
        .split("fn flush_projects(")
        .nth(1)
        .and_then(|s| s.split("fn bind_project_id").next())
        .expect("flush_projects");
    assert!(
        flush_p.contains("submit_projects")
            && flush_p.contains("persist_svc")
            && !flush_p.contains("thread::spawn"),
        "folder click must not freeze the cabin writing projects.json: {flush_p}"
    );
    let drop_proj = src
        .split("fn remove_project_id(")
        .nth(1)
        .and_then(|s| s.split("fn apply_project_menu(").next())
        .expect("remove_project_id");
    assert!(
            drop_proj.contains("self.flush_projects()")
                && drop_proj.contains("self.persist()")
                && drop_proj.contains("out.unbound"),
            "deleting an unbound project must not clone every thread just to write projects.json: {drop_proj}"
        );
    let folders = format!(
        "{}{}{}{}{}{}",
        fn_src(&src, "make_folder"),
        fn_src(&src, "stage_new_folder"),
        fn_src(&src, "begin_proj_rename"),
        fn_src(&src, "cancel_proj_rename"),
        fn_src(&src, "finish_proj_rename"),
        fn_src(&src, "move_sel_to_folder_name"),
    );
    assert!(
            folders.contains("self.flush_projects()")
                && !folders.contains("self.persist()")
                && !folders.contains("persist_snap"),
            "folder create/rename/move must not clone every thread just to write projects.json: {folders}"
        );
    let menu = src
        .split("fn apply_project_menu(")
        .nth(1)
        .and_then(|s| s.split("fn make_folder(").next())
        .expect("apply_project_menu");
    assert!(
        menu.contains("self.flush_projects()")
            && !menu.contains("self.persist()")
            && !menu.contains("persist_snap"),
        "Remove from folder must not clone every thread just to write projects.json: {menu}"
    );
    let overlays = fn_src(&src, "ui_project_overlays");
    assert!(
        !overlays.contains("New project") && !src.contains("New project or folder"),
        "the projects section does not offer New project"
    );
    assert!(
        src.contains("on_hover_text(\"New folder\")") && src.contains("self.stage_new_folder()"),
        "projects + creates a folder"
    );
    let rename = src
        .split("Slash::ProjectRename")
        .nth(1)
        .and_then(|s| s.split("Slash::ProjectMove").next())
        .expect("ProjectRename");
    assert!(
        rename.contains("self.flush_projects()")
            && !rename.contains("self.persist()")
            && !rename.contains("persist_snap"),
        "/project rename must not clone every thread just to write projects.json: {rename}"
    );
    let overlay = fn_src(&src, "ui_project_overlays");
    assert!(
        overlay.contains("self.flush_projects()")
            && !overlay.contains("self.persist()")
            && !overlay.contains("persist_snap"),
        "Add to folder must not clone every thread just to write projects.json: {overlay}"
    );
    let renamed = src
        .split("fn rename_thread")
        .nth(1)
        .and_then(|s| s.split("fn pin_thread").next())
        .expect("rename_thread");
    assert!(
        renamed.contains("accessed_ms"),
        "rename must bump accessed_ms or /sync LWW can drop the new title: {renamed}"
    );
    let pinned = src
        .split("fn pin_thread")
        .nth(1)
        .and_then(|s| s.split("fn delete_thread_at").next())
        .expect("pin_thread");
    assert!(
        pinned.contains("accessed_ms") && pinned.contains("pinned_ms"),
        "pin must bump accessed_ms and record pinned_ms or /sync LWW can drop the pin: {pinned}"
    );
    let goal = src
        .split("fn apply_thread_goal")
        .nth(1)
        .and_then(|s| s.split("fn spawn_thread_goal").next())
        .expect("apply_thread_goal");
    assert!(
        goal.contains("accessed_ms"),
        "auto-title must bump accessed_ms or /sync LWW can drop the new name: {goal}"
    );
    assert!(
        goal.contains("self.persist()") && !goal.contains("threads::save"),
        "auto-title must not freeze the cabin writing threads.json: {goal}"
    );
    let spawn_goal = src
        .split("fn spawn_thread_goal_on(")
        .nth(1)
        .and_then(|s| s.split("fn refresh_chips(").next())
        .expect("spawn_thread_goal_on");
    assert!(
        spawn_goal.contains("visible_turn_count") || spawn_goal.contains("is_workload_user"),
        "auto-title must ignore HOST_RESULT or a Command-pane job names the thread: {spawn_goal}"
    );
    assert!(
        spawn_goal.contains("chip_chat_pairs") || spawn_goal.contains("chip_scan"),
        "auto-title must not clone an 8MB complete into chat_pairs: {spawn_goal}"
    );
    let created = src
        .split("fn new_thread")
        .nth(1)
        .and_then(|s| s.split("fn begin_chat_rename").next())
        .expect("new_thread");
    assert!(
        created.contains("flush_visible_goal"),
        "/new must persist the left tab's goal before clearing it: {created}"
    );
    assert!(
        created.contains("drop_leaving_thread_chrome"),
        "/new must drop plus-attach, followup budget, and skill follow: {created}"
    );
    assert!(
        created.contains("reuse_empty_thread_idx"),
        "/new must reuse an empty Chat instead of stacking leftover tabs: {created}"
    );
    assert!(
        created.contains("grok_session = None"),
        "New chat must forget the last ACP session id so the next send is session/new: {created}"
    );
    assert!(
            created.contains("grok_cwd = None"),
            "New chat must forget the last worktree or the next send session/new stays in a History tree: {created}"
        );
    assert!(
        created.contains("self.persist()"),
        "forgetting the ACP session on New chat must hit disk or restart reloads Chat 1: {created}"
    );
    assert!(
        created.contains("composer_want_focus = true"),
        "New chat must put the cursor in the composer: {created}"
    );
    assert!(
            created.contains("apply_switch_thread") && !created.contains("self.switch_thread("),
            "/new reuse must not clone every thread twice — switch without persist_bg, then persist once: {created}"
        );
    assert!(
        created.contains("self.messages.clone()") && created.contains("Arc::new"),
        "/new must share the leaving pane Arc, not clone an 8MB HOST_RESULT: {created}"
    );
    let boot = src
        .split("pub fn new(hidden: bool)")
        .nth(1)
        .and_then(|s| s.split("fn apply_saved_geom(").next())
        .expect("Cabin::new");
    assert!(
        boot.contains("ensure_memory_seeds") && boot.contains("default_device_name"),
        "first run must seed Memory files and a device name: {boot}"
    );
    assert!(
            boot.contains("keep_loaded_thread"),
            "boot must drop leftover empty Chat tabs and keep a chat that still has a Grok session: {boot}"
        );
    assert!(
        !boot.contains("threads::save"),
        "boot leftover drop must not freeze the cabin writing threads.json: {boot}"
    );
    assert!(
        boot.contains("persist_bg"),
        "boot leftover drop must persist off-thread or restart restores empty Chat tabs: {boot}"
    );
    assert!(
        src.contains("leftover_empty_thread"),
        "History/boot must hide leftover empty Chat rows"
    );
    let switched = src
        .split("fn switch_thread")
        .nth(1)
        .and_then(|s| s.split("fn open_recent_chat").next())
        .expect("switch_thread");
    assert!(
        switched.contains("drop_leaving_thread_chrome"),
        "switching tabs must not send the previous tab's image or skill follow: {switched}"
    );
    assert!(
        switched.contains("persist_bg") && !switched.contains("self.persist()"),
        "tab switch must not freeze the cabin writing threads.json: {switched}"
    );
    assert!(
        switched.contains("apply_switch_thread"),
        "tab switch persist_bg must share the pane swap with /new reuse: {switched}"
    );
    assert!(
        switched.contains("composer_want_focus = true"),
        "opening a sidebar chat must put the cursor in the composer: {switched}"
    );
    assert!(
        switched.contains("self.messages.clone()")
            && switched.contains("live_mut")
            && switched.contains("Arc::make_mut"),
        "tab switch must share the parked pane Arc; first mutation copy-on-writes: {switched}"
    );
    let chrome = src
        .split("fn drop_leaving_thread_chrome")
        .nth(1)
        .and_then(|s| s.split("fn pick_entries").next())
        .expect("drop_leaving_thread_chrome");
    assert!(
        chrome.contains("hands_attach = false") && chrome.contains("eyes_attach = false"),
        "leaving a tab must not leave windshield/hands armed for the next tab: {chrome}"
    );
    assert!(
            chrome.contains("self.acp = None") && chrome.contains("tool_cards.clear()"),
            "leaving a tab must drop the ACP handle so New chat does not reuse the last session: {chrome}"
        );
    assert!(
            chrome.contains("halt_in_flight"),
            "dropping the ACP handle on tab switch must halt or the cabin stays Thinking on Chat 2: {chrome}"
        );
    assert!(
        chrome.contains("last_receipt_ok = None"),
        "leaving a tab must not put the next composer into the other tab's error chips: {chrome}"
    );
    let deleted = fn_src(&src, "delete_thread_at");
    assert!(
        deleted.contains("drop_leaving_thread_chrome"),
        "deleting the visible tab must drop plus-attach and followup budget: {deleted}"
    );
    let verify = src
        .split("fn run_skill_verify")
        .nth(1)
        .and_then(|s| s.split("fn replay_saved_recipe").next())
        .expect("run_skill_verify");
    let pushed = verify.find("push_bound_msg").expect("VERIFY_RESULT push");
    let saved = verify.find("self.persist()").expect("verify persist");
    assert!(
        pushed < saved,
        "VERIFY_RESULT must hit disk or a restart drops it: {verify}"
    );
    assert!(
        verify.contains("host_working_dir") && verify.contains("run_verify"),
        "skill verify must run in the bound project, not the cabin cwd: {verify}"
    );
    let spawn = verify
        .find("thread::spawn")
        .expect("skill verify must leave the UI thread");
    let run = verify.find("run_verify").expect("run_verify");
    assert!(
        spawn < run,
        "HostDone verify must not block the cabin for 12s: {verify}"
    );
    let kick = fn_src(&src, "kick_model");
    assert!(
        kick.contains("verify_rx"),
        "kick_model must wait for off-thread verify before the follow-up turn: {kick}"
    );
    assert!(
        !kick.contains("t.messages.clone()") && !kick.contains("kick_messages_for_job"),
        "kick_model must not clone the transcript to read the last user turn: {kick}"
    );
    let reflect = src
        .split("fn run_reflect")
        .nth(1)
        .and_then(|s| s.split("fn run_skill_verify").next())
        .expect("run_reflect");
    assert!(
        reflect.contains("kick_messages_for_job")
            || (reflect.contains("chat_job_thread") && reflect.contains("self.threads")),
        "/learn reflect must read the origin thread, not only the visible tab: {reflect}"
    );
    assert!(
        !reflect.contains("t.messages.clone()") && !reflect.contains("content.clone()"),
        "/learn reflect must not clone an 8MB transcript to harvest facts: {reflect}"
    );
    let mem = reflect
        .find("read_memory(\"MEMORY.md\")")
        .expect("reflect memory");
    assert!(
        reflect[..mem].contains("write_memory") && reflect[..mem].contains("mem_body"),
        "/learn reflect must flush the Memory editor before surgical edit: {reflect}"
    );
    let mem_spawn = reflect[..mem]
        .rfind("thread::spawn")
        .expect("reflect memory must leave the UI thread");
    let flush = reflect[..mem].find("write_memory").expect("reflect flush");
    assert!(
        mem_spawn > flush && reflect.contains("reflect_rx"),
        "idle reflect must not freeze the cabin slurping MEMORY.md: {reflect}"
    );
    let insights = reflect.find("extract_insights").expect("reflect insights");
    assert!(
        reflect[insights..].contains("save_learning")
            && reflect[insights..].contains("persist_io")
            && !reflect[insights..].contains("persist_snap")
            && !reflect[insights..].contains("self.persist()"),
        "/learn reflect must persist insights without cloning every thread: {reflect}"
    );
    let impl_src = src.as_str();
    assert!(
        !impl_src.contains("fn take_over_desktop")
            && !impl_src.contains("white_pill(ui, \"Take over\")"),
        "Desk Take over is gone — Grok Build computer-use runs from chat"
    );
    assert!(
        !crate::theme::CABIN_MENU.iter().any(|(id, _)| *id == "eyes"),
        "Desk must not sit in the cabin menu"
    );
    assert!(
        !crate::theme::CABIN_MENU
            .iter()
            .any(|(id, _)| *id == "command"),
        "Command is not a cabin menu row"
    );
    for gone in ["history", "workboard", "memory", "devices", "queue"] {
        assert!(
            !crate::theme::CABIN_MENU.iter().any(|(id, _)| *id == gone),
            "{gone} must not sit in the avatar menu"
        );
    }
    let menu = fn_src(&src, "ui_settings_menu");
    assert!(
        menu.contains("CABIN_MENU")
            && menu.contains("\"Help\"")
            && menu.contains("\"Sign out\"")
            && menu.contains("\"Connect Grok\""),
        "avatar menu must keep Settings, Help, and Sign in / Sign out: {menu}"
    );
    assert!(
        !menu.contains("\"History\"")
            && !menu.contains("\"Workboard\"")
            && !menu.contains("\"Memory\"")
            && !menu.contains("\"Devices\"")
            && !menu.contains("\"Queue\""),
        "avatar menu must not hardcode leftover panes: {menu}"
    );
    assert!(
        menu.contains("chrome.name")
            && menu.contains("chrome.picture_path")
            && !menu.contains("email"),
        "avatar menu paints the saved name and picture path, not the email: {menu}"
    );
    let replay = src
        .split("fn replay_recipe(")
        .nth(1)
        .and_then(|s| s.split("fn speak_reply").next())
        .expect("replay_recipe");
    assert!(
        replay.contains("run_cmds") && !replay.contains("run_computer_op("),
        "recipe replay must use host gates, not raw desktop ops: {replay}"
    );
    assert!(
        replay.contains("self.running"),
        "recipe replay must report whether host actually started: {replay}"
    );
    let reshoot = replay.split("ReplayOp::Reshoot").nth(1).expect("reshoot");
    assert!(
        reshoot.contains("lock_blocks_hands"),
        "recipe reshoot must not capture a lock screen: {reshoot}"
    );
    assert!(
        reshoot.contains("lock_titles"),
        "recipe reshoot must see lock windows that collect_rows drops: {reshoot}"
    );
    let saved_replay = src
        .split("fn replay_saved_recipe")
        .nth(1)
        .and_then(|s| s.split("fn replay_recipe(").next())
        .expect("replay_saved_recipe");
    assert!(
        saved_replay.contains("self.replay_recipe()") && !saved_replay.contains("true"),
        "night must not count a blocked recipe replay as started: {saved_replay}"
    );
    let send_auth = src
        .split("fn send_chat")
        .nth(1)
        .and_then(|s| s.split("fn send_followup_turn").next())
        .expect("send_chat auth");
    assert!(
        send_auth.contains("persist_user_turn(self.agent_ready())")
            && send_auth.contains("kick_model(true)"),
        "typed send must check the native sign-in, then kick the turn: {send_auth}"
    );
    let gate = send_auth.find("persist_user_turn").expect("send auth");
    assert!(
        send_auth[gate..].contains("hands_attach = false")
            && send_auth[gate..].contains("eyes_attach = false"),
        "auth-fail send must disarm leftover take-over flags: {send_auth}"
    );
    assert!(
        send_auth[gate..].contains("speak_next = false"),
        "auth-fail send must not leave TTS armed for the next reply: {send_auth}"
    );
    let last_user = src
        .split("fn last_user_on_job")
        .nth(1)
        .and_then(|s| s.split("fn commit_proposed_skill").next())
        .expect("last_user_on_job");
    assert!(
        (last_user.contains("last_user_for_job") || last_user.contains("last_user_scan"))
            && last_user.contains("self.threads")
            && !last_user.contains("t.messages.clone()"),
        "skill draft after host must not clone every thread: {last_user}"
    );
    assert!(
        !last_user.contains("content.clone()"),
        "skill draft after host must not clone an 8MB complete to read the last user: {last_user}"
    );
    let halt_flight = src
        .split("fn halt_in_flight")
        .nth(1)
        .and_then(|s| s.split("fn apply_assistant_snapshot").next())
        .expect("halt_in_flight");
    assert!(
        halt_flight.contains("speak_next = false"),
        "Stop must cancel a pending voice speak: {halt_flight}"
    );
    assert!(
        halt_flight.contains("scheduled_perm = false"),
        "Stop must drop scheduled_perm so the next typed Ask uses ACP: {halt_flight}"
    );
    assert!(
        halt_flight.contains("perm_ask = None"),
        "Stop must drop the permission bar or Allow continues the cancelled turn: {halt_flight}"
    );
    assert!(
        halt_flight.contains("answer_permission"),
        "Stop must deny leftover Ask or the next send hangs on the unanswered RPC: {halt_flight}"
    );
    assert!(
        halt_flight.contains("h.cancel()")
            && !halt_flight.contains("kill_pid")
            && !halt_flight.contains("grok_p_pid"),
        "Stop cancels the native turn; there is no grok -p child to kill: {halt_flight}"
    );
    assert!(
            halt_flight.contains("try_recv"),
            "Stop must drain leftover ACP tokens so they do not paint on the next prompt: {halt_flight}"
        );
    let halt_persist = halt_flight.find("self.persist()").expect("halt persist");
    assert!(
        halt_flight[..halt_persist].contains("accessed_ms")
            || halt_flight[..halt_persist].contains("stamp_current_access"),
        "halt must bump accessed_ms or /sync LWW can restore the dropped assistant: {halt_flight}"
    );
    assert!(
        halt_flight.contains("stamp_current_access") && halt_flight.contains("accessed_ms"),
        "halt must stamp the origin thread when it is not the visible tab: {halt_flight}"
    );
    assert!(
        !halt_flight.contains("t.messages.clone()") && !halt_flight.contains("content.clone()"),
        "Stop must not clone an 8MB transcript to drop one trailing assistant: {halt_flight}"
    );
    assert!(
        halt_flight.contains("imagine_pending = false"),
        "Stop must clear Imagine pending or a later job error paints on the stage: {halt_flight}"
    );
    let host_done_facts = src
        .split("Ok(JobOut::HostDone(block))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::Connector").next())
        .expect("HostDone facts");
    assert!(host_done_facts.contains("HOST_DIFF:"), "HOST_DIFF push");
    assert!(
        host_done_facts.contains("resolve_host_cite_path"),
        "HOST_DIFF must read the write from the bound tree, not the cabin cwd: {host_done_facts}"
    );
    let after_cite = host_done_facts.find("bump_usage").expect("host usage");
    let diff_spawn = host_done_facts[after_cite..]
        .find("thread::spawn")
        .expect("HOST_DIFF worker")
        + after_cite;
    let diff_read = host_done_facts[after_cite..]
        .find("read_text_capped")
        .expect("HOST_DIFF read")
        + after_cite;
    assert!(
        diff_spawn < diff_read && !host_done_facts.contains("read_to_string"),
        "HOST_DIFF must not slurp a huge host write on the UI thread: {host_done_facts}"
    );
    let host_diff_poll = src
        .split("fn poll_host_diff(")
        .nth(1)
        .and_then(|s| s.split("fn finish_host_diff_kick(").next())
        .expect("poll_host_diff");
    assert!(
        host_diff_poll.contains("HOST_DIFF") || host_diff_poll.contains("push_bound_msg"),
        "HOST_DIFF must land in the transcript: {host_diff_poll}"
    );
    assert!(
        host_diff_poll.contains("self.persist()"),
        "HOST_DIFF must persist after the cite push: {host_diff_poll}"
    );
    let deleted = src
        .split("fn delete_thread_at")
        .nth(1)
        .and_then(|s| s.split("fn send_chat").next())
        .expect("delete_thread_at");
    assert!(
        deleted.contains("goal.step") && deleted.contains("self.goal_step"),
        "deleting the visible tab must adopt the next tab's goal step: {deleted}"
    );
    assert!(
        src.contains("let goal_step = threads.get(thread_idx)"),
        "boot must restore the current thread's goal step, not always 0"
    );
    let skills_ui = src
        .split("fn ui_skills")
        .nth(1)
        .and_then(|s| s.split("fn project_row_active(").next())
        .expect("ui_skills");
    assert!(
        skills_ui.contains("ensure_native_listing") && !skills_ui.contains("reload_grok_catalog"),
        "Skills reads the native engine's listing, never grok inspect: {skills_ui}"
    );
    assert!(
            skills_ui.contains("Cabin skills") && skills_ui.contains("self.skill_list"),
            "the skills the cabin follows and writes must be listed, not only the Grok catalog: {skills_ui}"
        );
    assert!(
        skills_ui.contains("skills::list_skills()"),
        "Refresh must re-read ~/.config/GrokHub/skills, not just the Grok catalog: {skills_ui}"
    );
    assert!(
        skills_ui.contains("skill_use_in_chat_prompt"),
        "Use in chat must send a Grok skill slash, not Follow skill: {skills_ui}"
    );
    let connectors = fn_src(include_str!("connectors_ui.rs"), "ui_settings_connectors");
    assert!(
        connectors.contains("ConnectorsTab::Marketplace") && connectors.contains("ui_marketplace"),
        "Settings, Connectors must show the cabin's MCP servers and the marketplace: {connectors}"
    );
    assert!(
        !src.contains("fn run_grok_user_cmd") && !src.contains("fn reload_grok_catalog"),
        "install and enable never run grok"
    );
    let skill_slash = src
        .split("Slash::Skill(name)")
        .nth(1)
        .and_then(|s| s.split("Slash::LearnReflect").next())
        .expect("Skill slash");
    assert!(
        skill_slash.contains("skill_use_in_chat_prompt") && skill_slash.contains("send_chat"),
        "/skill must run the skill, not only open the editor: {skill_slash}"
    );
    let send = src
        .split("fn send_chat")
        .nth(1)
        .and_then(|s| s.split("fn send_followup_turn").next())
        .expect("send_chat attach");
    assert!(
        send.contains("kick_model(true)"),
        "typed send must consume the plus-button image: {send}"
    );
    let retry = src
        .split("fn kick_model_retry")
        .nth(1)
        .and_then(|s| s.split("fn policy(").next())
        .expect("kick_model_retry");
    assert!(
        retry.contains("match_skill") && retry.contains("skill_follow_block"),
        "/retry must re-inject the skill follow that halt_in_flight cleared: {retry}"
    );
    let host_done = src
        .split("Ok(JobOut::HostDone(block))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::Connector").next())
        .expect("HostDone attach");
    assert!(
        host_done.contains("kick_model(false)"),
        "HostDone must not steal the attached image: {host_done}"
    );
    assert!(
        host_done.contains("watch_once")
            && host_done.contains("teachable_steps")
            && !host_done.contains("save_schedule"),
        "watching a host run must not save a job or the internal rewind snapshot: {host_done}"
    );
    let pushed = host_done.find("push_bound_msg").expect("host result");
    let recipe = host_done.find("save_recipe").expect("host recipe");
    let saved = host_done.find("self.persist()").expect("host persist");
    assert!(
        pushed < saved && saved < recipe,
        "HOST_RESULT must hit disk before recipe/skill side effects: {host_done}"
    );
    let bump = host_done.find("bump_usage").expect("host usage persist");
    assert!(
        bump < saved,
        "HostDone must persist host usage or persist_bg skips the bump: {host_done}"
    );
    assert_eq!(
        host_done.matches("self.persist()").count(),
        1,
        "HostDone must not clone every thread twice: {host_done}"
    );
    assert!(
        host_done.contains("lock_titles"),
        "HostDone capture must see lock windows that collect_rows drops: {host_done}"
    );
    assert!(
        host_done.contains("eyes_attach = true") && host_done.contains("hands_attach = true"),
        "after COMPUTER_CMD, HostDone must re-arm eyes and hands for the next shot: {host_done}"
    );
    let host_scan = host_done.find("collect_rows").expect("HostDone desk scan");
    let host_spawn = host_done
        .find("thread::spawn")
        .expect("HostDone desk scan worker");
    assert!(
        host_spawn < host_scan,
        "HostDone AT-SPI must not freeze the cabin: {host_done}"
    );
    let import = src
        .split("fn import_openclaw")
        .nth(1)
        .and_then(|s| s.split("fn run_consult").next())
        .expect("import_openclaw");
    assert!(
        import.contains("merge_imported_memory"),
        "/import must merge MEMORY.md instead of last-file-wins: {import}"
    );
    assert!(
        import.contains("mem_name") && import.contains("mem_body"),
        "/import must reload the Memory editor onto MEMORY.md: {import}"
    );
    let merge_read = import
        .find("read_memory(\"MEMORY.md\")")
        .expect("import memory");
    assert!(
        import[..merge_read].contains("write_memory") && import[..merge_read].contains("mem_body"),
        "/import must flush the Memory editor before merging MEMORY.md: {import}"
    );
    let dest_arm = import
        .split("import_memory_file")
        .nth(1)
        .expect("import files");
    let dest_write = dest_arm
        .find("write_memory(&dest")
        .expect("import dest write");
    assert!(
        dest_arm[..dest_write].contains("read_memory(&dest)"),
        "/import must not rotate .prev when SOUL/USER already match disk: {dest_arm}"
    );
    let after_loop = import
        .split("if imported > 0")
        .nth(1)
        .expect("import persist memory");
    assert!(
        after_loop.contains("read_memory(\"MEMORY.md\")")
            && after_loop.contains("write_memory(\"MEMORY.md\""),
        "/import must not rotate MEMORY.md.prev when the merge is unchanged: {after_loop}"
    );
    assert!(
        import.contains("read_text_capped") && !import.contains("read_to_string"),
        "/import must not slurp huge OpenClaw files on the UI thread: {import}"
    );
    let import_spawn = import
        .find("thread::spawn")
        .expect("import must leave the UI thread");
    let import_walk = import.find("read_dir").expect("import walks OpenClaw");
    assert!(
        import_spawn < import_walk,
        "/import must not walk OpenClaw on the UI thread: {import}"
    );
    let sign_out = src
        .split("fn sign_out_oauth")
        .nth(1)
        .and_then(|s| s.split("fn poll_oauth_photo").next())
        .expect("sign_out_oauth");
    assert!(
        sign_out.contains("oauth_pending = None"),
        "Sign out during device-code poll must not reconnect when the browser finishes: {sign_out}"
    );
    assert!(
        sign_out.contains("oauth_start_rx") && sign_out.contains("oauth_poll_rx"),
        "Sign out must drop in-flight OAuth HTTP: {sign_out}"
    );
    assert!(
        sign_out.contains("persist_io") && sign_out.contains("secrets::save"),
        "Sign out must not freeze the cabin writing secrets.json: {sign_out}"
    );
    assert!(
        sign_out.contains("imagine_pending = false"),
        "Sign out must clear Imagine pending or a later job error paints on the stage: {sign_out}"
    );
    assert!(
        !sign_out.contains("auth.json"),
        "Sign out must not wipe ~/.grok/auth.json: {sign_out}"
    );
    let start_o = src
        .split("fn start_oauth(")
        .nth(1)
        .and_then(|s| s.split("fn poll_oauth(").next())
        .expect("start_oauth");
    assert!(
        start_o.contains("oauth_pending") && start_o.contains("oauth_poll_rx"),
        "Connect must not restart an in-flight device-code wait: {start_o}"
    );
    let start_spawn = start_o.find("thread::spawn").expect("start_oauth spawn");
    let start_dev = start_o.find("start_device").expect("start_device");
    assert!(
        start_spawn < start_dev,
        "Connect Grok OAuth must not freeze the cabin on device-code HTTP: {start_o}"
    );
    let poll_o = src
        .split("fn poll_oauth(")
        .nth(1)
        .and_then(|s| s.split("fn clear_oauth_photo(").next())
        .expect("poll_oauth");
    let poll_spawn = poll_o.find("thread::spawn").expect("poll_oauth spawn");
    let poll_dev = poll_o.find("poll_device").expect("poll_device");
    assert!(
        poll_spawn < poll_dev,
        "OAuth poll must not freeze the cabin on token HTTP: {poll_o}"
    );
    assert!(
        poll_o.contains("persist_io") && poll_o.contains("secrets::save"),
        "OAuth Ready must not freeze the cabin writing secrets.json: {poll_o}"
    );
    let ready = src
        .split("PollStatus::Ready")
        .nth(1)
        .and_then(|s| s.split("PollStatus::Expired").next())
        .expect("oauth ready");
    assert!(
        !ready.contains("write_cli_auth_if_needed")
            && !ready.contains("sync_cli_auth_from_oauth")
            && (ready.contains("get_started_done = true")
                || ready.contains("mark_get_started_done")),
        "Connect Grok signs GrokHub in and never writes the CLI's auth.json: {ready}"
    );
    let boot = src
        .split("pub fn new(hidden: bool)")
        .nth(1)
        .and_then(|s| s.split("fn apply_saved_geom(").next())
        .expect("Cabin::new");
    assert!(
        !boot.contains("write_cli_auth_if_needed")
            && !boot.contains("sync_cli_auth_from_oauth")
            && !boot.contains("begin_ensure_grok_alpha")
            && !boot.contains("grok_cli_key")
            && !boot.contains("official_cli_session")
            && boot.contains("silence_windows_hard_errors"),
        "boot never installs, signs in, or reads the Grok Build CLI: {boot}"
    );
    let started = src
        .split("fn ui_get_started(")
        .nth(1)
        .and_then(|s| s.split("fn ui_settings(").next())
        .expect("ui_get_started");
    assert!(
        src.contains("if !self.ui_get_started(ui)"),
        "first-run sheet must replace empty-cabin chat so it paints on Windows"
    );
    let paint = src
        .split("self.ui_titlebar(ui);")
        .nth(1)
        .and_then(|s| s.split("if self.palette_open").next())
        .expect("titlebar then panes");
    let settings_nav = paint
        .find("if self.nav == Nav::Settings")
        .expect("Settings after Get Started skip");
    let skip = paint
        .find("if !self.ui_get_started(ui)")
        .expect("get started skip");
    assert!(
        settings_nav > skip
            && paint[skip..settings_nav].contains("ui_sidebar")
            && !paint[settings_nav..].contains("ui_sidebar"),
        "Latest chip must open Settings on top of Get Started: {paint}"
    );
    assert!(
        started.contains("get_started_oauth_error"),
        "Get Started must not paint leftover wall/install status as an OAuth error: {started}"
    );
    let poll = src
        .split("fn poll_oauth(")
        .nth(1)
        .and_then(|s| s.split("fn clear_oauth_photo(").next())
        .expect("poll_oauth");
    assert!(
        poll.matches("oauth_error_status").count() >= 3
            && poll.contains("PollStatus::Expired")
            && poll.contains("PollStatus::Denied"),
        "Get Started must show live device-code start/poll/deny failures: {poll}"
    );
    assert!(
        started.contains("egui::CentralPanel::default()"),
        "Get Started must paint in CentralPanel, not a first-frame Area: {started}"
    );
    assert!(
        !started.contains("egui::Area::new"),
        "Windows first-run Area over chat does not paint: {started}"
    );
    assert!(
        started.contains("ui_wants_get_started") && started.contains("start_oauth"),
        "Get Started must use cabin device-code OAuth: {started}"
    );
    assert!(
        started.contains("oauth_err") && started.contains("oauth_busy"),
        "Get Started must show OAuth errors and not restart an in-flight wait: {started}"
    );
    assert!(
        !started.contains("Installing Grok Build CLI")
            && !started.contains("queue_grok_cli_install")
            && !started.contains("official_cli_session")
            && !started.contains("should_show_cli_install_wait"),
        "Get Started never installs or waits on the Grok Build CLI: {started}"
    );
    let photo = src
        .split("fn kick_oauth_photo(")
        .nth(1)
        .and_then(|s| s.split("fn kick_model(").next())
        .expect("kick_oauth_photo");
    let photo_spawn = photo
        .find("thread::spawn")
        .expect("avatar fetch must leave the UI thread");
    let photo_decode = photo.find("oauth_photo_image").expect("avatar JPEG decode");
    assert!(
        photo_spawn < photo_decode,
        "OAuth avatar JPEG must not decode on the UI thread: {photo}"
    );
    let photo_poll = src
        .split("fn poll_oauth_photo(")
        .nth(1)
        .and_then(|s| s.split("fn kick_oauth_photo(").next())
        .expect("poll_oauth_photo");
    assert!(
        photo_poll.contains("persist_io") && photo_poll.contains("secrets::save"),
        "OAuth profile enrich must not freeze the cabin writing secrets.json: {photo_poll}"
    );
    let kick = fn_src(&src, "kick_model");
    assert!(
        kick.contains("kick_native_turn") && !kick.contains("spawn_grok_p_stream"),
        "kick_model runs the native turn, never grok -p: {kick}"
    );
    let cap_fn = src
        .split("fn capture_cabin_frame_this_turn")
        .nth(1)
        .and_then(|s| s.split("fn apply_job_fail").next())
        .expect("capture_cabin_frame_this_turn");
    assert!(
        cap_fn.contains("lock_titles"),
        "leftover capture helper must still see lock windows: {cap_fn}"
    );
    let anticipate = fn_src(&src, "tick_anticipate");
    let bump = anticipate.find("bump_usage").expect("anticipate usage");
    let gate = anticipate
        .find("anticipate_consumes_slot")
        .expect("anticipate auth");
    assert!(
        gate < bump,
        "anticipate must not burn quota before auth: {anticipate}"
    );
    assert!(
        anticipate.contains("scratch()"),
        "anticipate must not burn a slot on Scratch: {anticipate}"
    );
    assert!(
        anticipate.contains("anticipate_consumes_slot(self.agent_ready())")
            && !anticipate.contains("self.llm_ready()"),
        "a signed-out cabin must not burn an anticipate slot: {anticipate}"
    );
    let start_hub = format!(
        "{}{}",
        fn_src(&src, "start_hub"),
        fn_src(&src, "bind_lan_hub")
    );
    assert!(
        start_hub.contains("start_hub_rotates_pair"),
        "Start share must rotate an expired leftover code: {start_hub}"
    );
    let start_err = start_hub.split("Err(e)").nth(1).expect("start hub err");
    assert!(
        start_err.contains("sharing = false"),
        "Start share must not leave sharing on when serve_lan fails: {start_hub}"
    );
    assert!(
        start_hub.contains("lan_bind_in_use")
            && start_hub.contains("grokhub-hub.service")
            && start_hub.contains("serve_lan"),
        "Start share must take :18766 from grokhub-hub.service and retry serve_lan: {start_hub}"
    );
    assert!(
        start_hub.contains("self.persist_hub()") && !start_hub.contains("persist_snap"),
        "Start share must not clone every thread just to stamp sharing: {start_hub}"
    );
    let live = src
        .split("fn live_room")
        .nth(1)
        .and_then(|s| s.split("fn tick_mid_thought").next())
        .expect("live_room");
    let title = live.find("last_window_title").expect("live title");
    let gate = live.find("should_send_screenshot").expect("live gate");
    assert!(
        live.contains("collect_rows") && title < gate,
        "presence stream must refresh the foreground title before sending a frame: {live}"
    );
    assert!(
        live.contains("lock_titles"),
        "presence stream must see lock windows that collect_rows drops: {live}"
    );
    let devices = src
        .split("fn ui_devices")
        .nth(1)
        .and_then(|s| s.split("fn ui_memory").next())
        .expect("ui_devices");
    assert!(
        devices.contains("pair_code_is_live") && devices.contains("devices_shows_pair_code"),
        "Devices must hide a dead pair code and any code while the hub is off: {devices}"
    );
    let new_code = devices
        .split("New code")
        .nth(1)
        .and_then(|s| s.split("empty_prompt_tile").next())
        .expect("new code");
    let rotated = new_code.find("rotate_pair").expect("rotate");
    assert!(
        new_code[rotated..].contains("self.persist_hub()"),
        "New code must persist the rotated pair before a restart: {new_code}"
    );
    let expired = devices
        .split("rotate_pair")
        .nth(1)
        .and_then(|s| s.split("New code").next())
        .expect("expired rotate");
    assert!(
        expired.contains("self.persist_hub()"),
        "an expired pair rotate must persist or restart shows the dead code: {expired}"
    );
    let clear = src
        .split("Slash::Clear =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Undo =>").next())
        .expect("Clear");
    assert!(
        clear.contains("halt_in_flight"),
        "/clear during a job must halt or the stream refills the pane: {clear}"
    );
    assert!(
        clear.contains("followup_step = 0") && clear.contains("active_skill_follow = None"),
        "/clear must reset followup budget and skill follow with the pane: {clear}"
    );
    assert!(
        clear.contains("stamp_current_access") || clear.contains("accessed_ms"),
        "/clear must bump accessed_ms or /sync LWW can restore the cleared turns: {clear}"
    );
    assert!(
        clear.contains("drop_leaving_thread_chrome") && clear.contains("grok_session = None"),
        "/clear must drop ACP and forget the session id or the next send loads Chat 1: {clear}"
    );
    assert!(
            clear.contains("grok_cwd = None"),
            "/clear must forget the worktree or the next send session/new stays in a History tree: {clear}"
        );
    let help = src
        .split("Slash::Help =>")
        .nth(1)
        .and_then(|s| s.split("Slash::New =>").next())
        .expect("Help");
    assert!(
        help.contains("stamp_current_access") || help.contains("accessed_ms"),
        "/help must bump accessed_ms or /sync LWW can drop the help turn: {help}"
    );
    assert!(
        help.contains("mark_slash_result") || help.contains("SLASH_RESULT"),
        "/help must not become the next model turn: {help}"
    );
    let models = src
        .split("Slash::Models =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Palette =>").next())
        .expect("Models");
    assert!(
        models.contains("stamp_current_access") || models.contains("accessed_ms"),
        "/models must bump accessed_ms or /sync LWW can drop the catalog turn: {models}"
    );
    assert!(
        models.contains("mark_slash_result") || models.contains("SLASH_RESULT"),
        "/models must not become the next model turn: {models}"
    );
    let undo = src
        .split("Slash::Undo =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Retry =>").next())
        .expect("Undo");
    assert!(
            undo.contains("halt_in_flight")
                && undo.contains("REWIND_NA")
                && !undo.contains("live_mut().remove")
                && !undo.contains("send_grok_slash"),
            "/undo stops a live reply; a finished one stays, since the native session is append-only: {undo}"
        );
    let forget = src
        .split("Slash::Forget")
        .nth(1)
        .and_then(|s| s.split("Slash::MemoryShow").next())
        .expect("Forget");
    assert!(
        forget.contains("self.scratch()") && forget.contains("no memory writes"),
        "/forget on Scratch must not wipe MEMORY.md: {forget}"
    );
    let note = src
        .split("Slash::MemoryNote")
        .nth(1)
        .and_then(|s| s.split("Slash::Board").next())
        .expect("MemoryNote");
    let append = note.find("append_memory").expect("append");
    assert!(
        note[..append].contains("write_memory") && note[..append].contains("mem_body"),
        "/remember must flush the Memory editor before appending to disk: {note}"
    );
    let append_spawn = note
        .find("thread::spawn")
        .expect("remember append must leave the UI thread");
    assert!(
        append_spawn < append,
        "/remember must not freeze the cabin appending MEMORY.md: {note}"
    );
    let topic = forget.split("Some(q)").nth(1).expect("forget topic");
    let read = topic
        .find("read_memory(\"MEMORY.md\")")
        .expect("forget read");
    assert!(
        topic[..read].contains("write_memory") && topic[..read].contains("mem_body"),
        "/forget topic must flush the Memory editor before editing disk: {topic}"
    );
    let forget_spawn = topic[..read]
        .rfind("thread::spawn")
        .expect("forget slurp must leave the UI thread");
    assert!(
        forget_spawn < read,
        "/forget topic must not freeze the cabin slurping MEMORY.md: {topic}"
    );
    let memory_ui = src
        .split("fn ui_memory")
        .nth(1)
        .and_then(|s| s.split("fn save_settings").next())
        .expect("ui_memory");
    assert!(
        memory_ui.contains("self.scratch()") && memory_ui.contains("no memory writes"),
        "Memory Save on Scratch must not write MEMORY.md: {memory_ui}"
    );
    let restore = memory_ui
        .split("ghost_pill(ui, \"Restore\")")
        .nth(1)
        .and_then(|s| s.split("Reflect").next())
        .expect("memory restore");
    let restore_spawn = restore
        .find("thread::spawn")
        .expect("restore must leave the UI thread");
    let restore_fn = restore.find("restore_memory").expect("restore_memory");
    assert!(
        restore_spawn < restore_fn,
        "Memory Restore must not freeze the cabin reading MEMORY.md.prev: {restore}"
    );
    assert!(
        memory_ui.contains("self.open_memory_file(name)"),
        "the Memory tabs and a History hit open a file the same way: {memory_ui}"
    );
    let tabs = src
        .split("fn open_memory_file(")
        .nth(1)
        .and_then(|s| s.split("fn ui_memory(").next())
        .expect("open_memory_file");
    assert!(
        tabs.contains("if self.mem_name == name") && tabs.contains("return;"),
        "re-opening the file already in the editor must keep unsaved edits: {tabs}"
    );
    let flush = tabs.find("write_memory").expect("flush leaving memory");
    let switch = tabs.find("mem_name = name").expect("switch name");
    assert!(
        flush < switch && tabs.contains("scratch()"),
        "Memory tab switch must flush the leaving file like thread switch: {tabs}"
    );
    assert!(
        tabs[..flush].contains("read_memory"),
        "Memory tab switch must not rotate .prev when the leaving file is unchanged: {tabs}"
    );
    assert!(
        tabs.contains("thread::spawn"),
        "Memory tab switch must flush off the UI thread: {tabs}"
    );
    assert!(
        tabs.contains("memory_updated_at")
            && tabs.contains("mem_file_rx")
            && tabs.contains("mem_cache_at"),
        "Memory tab switch must not slurp SOUL.md on every click after the first miss: {tabs}"
    );
    assert!(
        tabs.contains("read_memory(name)") && tabs.contains("mem_cache_at[i] == 0"),
        "first Memory tab miss must still read on-thread so tests stay deterministic: {tabs}"
    );
    let save = memory_ui
        .split("white_pill(ui, \"Save\")")
        .nth(1)
        .expect("memory save");
    let write = save.find("write_memory").expect("save write");
    assert!(
        save[..write].contains("read_memory") && save[..write].contains("mem_body"),
        "Memory Save must not rotate .prev when the file is unchanged: {save}"
    );
    assert!(
        save.contains("thread::spawn"),
        "Memory Save must not freeze the cabin writing MEMORY.md: {save}"
    );
    let settings_save = src
        .split("fn save_settings")
        .nth(1)
        .and_then(|s| s.split("fn ui_settings").next())
        .expect("save_settings");
    assert!(
        settings_save.contains("upsert_bound") && settings_save.contains("touch_projects"),
        "Settings Save must keep the sidebar selection on the bound path: {settings_save}"
    );
    let settings_spawn = settings_save
        .find("thread::spawn")
        .expect("settings mkdir must leave the UI thread");
    let settings_mkdir = settings_save
        .find("create_dir_all")
        .expect("create_dir_all");
    assert!(
        settings_spawn < settings_mkdir,
        "Settings Save must not freeze the cabin creating the bound folder: {settings_save}"
    );
    let hub_name = settings_save.find("device_name").expect("hub device name");
    let saved = settings_save
        .find("self.persist()")
        .expect("settings persist");
    assert!(
        hub_name < saved,
        "Settings Save must persist hub device_name or restart keeps the old name: {settings_save}"
    );
    let unbound = src
        .split("Slash::ProjectClear =>")
        .nth(1)
        .and_then(|s| s.split("Slash::ProjectShow =>").next())
        .expect("ProjectClear");
    assert!(
        unbound.contains("project_sel = None") && unbound.contains("touch_projects"),
        "/project clear must drop the sidebar selection: {unbound}"
    );
    let export = src
        .split("Slash::Export =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Recall").next())
        .expect("Export");
    let flushed = export.find("self.persist()").expect("export persist");
    let wrote = export.find("export_markdown").expect("export_markdown");
    assert!(
        flushed < wrote,
        "/export must flush the live pane before writing the thread file: {export}"
    );
    assert!(
        export.contains("expand_home"),
        "/export must expand ~ in the bound project or it writes a literal tilde folder: {export}"
    );
    let export_spawn = export
        .find("thread::spawn")
        .expect("export write must leave the UI thread");
    let export_write = export.find("fs::write").expect("export.md");
    assert!(
        export_spawn < export_write && export_spawn < wrote,
        "/export must not freeze the cabin formatting an 8MB thread into markdown: {export}"
    );
    let recall = src
        .split("Slash::Recall(q)")
        .nth(1)
        .and_then(|s| s.split("fn kick_model_retry").next())
        .expect("Recall");
    let mem = recall
        .find("read_memory(\"SOUL.md\")")
        .expect("recall soul");
    assert!(
        recall[..mem].contains("write_memory")
            && recall[..mem].contains("mem_body")
            && recall[..mem].contains("scratch()"),
        "/recall must flush the Memory editor before searching disk: {recall}"
    );
    assert!(
        recall[..mem].contains("thread::spawn"),
        "/recall must slurp SOUL/USER/MEMORY off the UI thread: {recall}"
    );
    assert!(
        recall.contains("learning") && recall.contains("(\"learned\", insights)"),
        "what the cabin learned by itself is memory too — /recall must search it: {recall}"
    );
    assert!(
        recall.contains("dedupe_hits") && !recall.contains("hits.sort()"),
        "sorting hits alphabetically buries the memory line under chat titles: {recall}"
    );
    let recall_poll = src
        .split("fn poll_recall(")
        .nth(1)
        .and_then(|s| s.split("fn poll_session_show").next())
        .expect("poll_recall");
    assert!(
        recall_poll.contains("stamp_current_access") || recall_poll.contains("accessed_ms"),
        "/recall must bump accessed_ms or /sync LWW can drop the recall turn: {recall_poll}"
    );
    assert!(
        recall_poll.contains("mark_slash_result") || recall_poll.contains("SLASH_RESULT"),
        "/recall must not become the next model turn: {recall_poll}"
    );
    let sync = src
        .split("fn sync_hub(&mut self)")
        .nth(1)
        .and_then(|s| s.split("fn local_clock").next())
        .expect("sync_hub");
    assert!(
        sync.contains("merge_hub_snapshots"),
        "/sync must merge the hub snapshot, not replace peer threads: {sync}"
    );
    let flushed = sync.find("persist_snap").expect("sync persist");
    let built = sync.find("build_hub_snapshot").expect("build snapshot");
    assert!(
        flushed < built,
        "/sync must flush the live pane before publishing threads: {sync}"
    );
    let merged = sync.find("st.snapshot =").expect("store merge");
    assert!(
        sync[merged..].contains("self.persist_hub()"),
        "/sync must persist the merged snapshot or a restart drops peer LWW: {sync}"
    );
    let poll = src
        .split("fn poll_sync(")
        .nth(1)
        .and_then(|s| s.split("fn date_out").next())
        .expect("poll_sync");
    assert!(
        poll.contains("persist_hub") && !poll.contains("persist_snap"),
        "/sync inbound must not clone every thread just to flush hub-state.json: {poll}"
    );
    let mem_write = sync.find("write_memory").expect("sync memory flush");
    assert!(
        mem_write < built && sync.contains("mem_body") && sync.contains("scratch()"),
        "/sync must flush the Memory editor before publishing files: {sync}"
    );
    let sync_spawn = sync.find("thread::spawn").expect("sync worker");
    let sync_read = sync.find("read_memory(n)").expect("sync memory slurp");
    assert!(
        sync_spawn < sync_read,
        "/sync must slurp SOUL/USER/MEMORY off the UI thread: {sync}"
    );
    assert!(
        sync.contains("submit_full") && sync.contains("persist_done"),
        "/sync must schedule PersistService before egress: {sync}"
    );
    let thread_rows = sync
        .split("let threads = snap")
        .nth(1)
        .and_then(|s| s.split("let skills = skills").next())
        .expect("sync threads");
    assert!(
        thread_rows.contains("accessed_ms") && !thread_rows.contains("now_ms()"),
        "/sync must not stamp every thread now or local stale data wins LWW: {thread_rows}"
    );
    assert!(
        !sync.contains("t.messages.clone()") && sync.contains("snap.clone()"),
        "/sync must reuse the persist snap instead of cloning every thread twice: {sync}"
    );
    let push = src
        .split("fn push_bound_msg")
        .nth(1)
        .and_then(|s| s.split("fn apply_live_assistant").next())
        .expect("push_bound_msg");
    assert!(
        push.contains("IMAGE_FILE_CAP") || push.contains("take_ui_text"),
        "host/consult receipts must not land a huge body in the transcript: {push}"
    );
    assert!(
        push.contains("accessed_ms"),
        "background job writes must bump accessed_ms or /sync LWW drops the new messages: {push}"
    );
    assert!(
        !push.contains("t.messages.clone()"),
        "host receipts must not clone every thread: {push}"
    );
    let snap = src
        .split("fn apply_assistant_snapshot")
        .nth(1)
        .and_then(|s| s.split("fn push_bound_msg").next())
        .expect("apply_assistant_snapshot");
    assert!(
            snap.contains("accessed_ms"),
            "background stream writes must bump accessed_ms or /sync LWW drops the new messages: {snap}"
        );
    assert!(
        !snap.contains("t.messages.clone()"),
        "stream deltas must not clone every thread: {snap}"
    );
    let mem_rows = sync
        .split("let mem = ")
        .nth(1)
        .and_then(|s| s.split("let mut snap = self.persist_snap").next())
        .expect("sync mem");
    assert!(
        mem_rows.contains("memory_updated_at") && !mem_rows.contains("now_ms()"),
        "/sync must not stamp MEMORY.md now or stale local wins LWW: {mem_rows}"
    );
    let skill_rows = sync
        .split("let skills = self")
        .nth(1)
        .and_then(|s| s.split("let autos = self").next())
        .expect("sync skills");
    assert!(
        skill_rows.contains("skill_updated_at") && !skill_rows.contains("now_ms()"),
        "/sync must not stamp every skill now or local stale data wins LWW: {skill_rows}"
    );
    let inbound = src
        .split("fn apply_inbound_snapshot")
        .nth(1)
        .and_then(|s| s.split("fn push_presence").next())
        .expect("apply_inbound_snapshot");
    assert!(
        inbound.contains("mem_body") && inbound.contains("mem_name"),
        "inbound MEMORY.md must refresh the open Memory editor: {inbound}"
    );
    assert!(
            inbound.contains("mem_cache_body") && inbound.contains("mem_file_idx"),
            "inbound MEMORY.md must refresh the Memory tab cache or a later click shows a stale slurp: {inbound}"
        );
    let wrote = inbound.find("write_memory").expect("inbound write");
    assert!(
        inbound[..wrote].contains("read_memory"),
        "inbound must not rotate .prev when the merged file is unchanged: {inbound}"
    );
    let inbound_spawn = inbound
        .find("thread::spawn")
        .expect("inbound write must leave the UI thread");
    assert!(
        inbound_spawn < wrote,
        "inbound MEMORY.md must not freeze the cabin writing markdown: {inbound}"
    );
    assert!(
        !inbound.contains("snapshot.clone()") && !inbound.contains("from_value"),
        "/sync inbound must not clone the hub snapshot on the UI thread: {inbound}"
    );
    let send = src
        .split("fn dispatch_send")
        .nth(1)
        .and_then(|s| s.split("fn sync_hub(&mut self)").next())
        .expect("dispatch_send");
    let queued = send.find("enqueue_local").expect("enqueue");
    assert!(
        send[queued..].contains("self.persist_hub()") && !send[queued..].contains("persist_snap"),
        "/send must persist a queued hub task without cloning every thread: {send}"
    );
    let inhabit = src
        .split("fn queue_inhabit")
        .nth(1)
        .and_then(|s| s.split("fn rewind_project").next())
        .expect("queue_inhabit");
    let staged = inhabit.find("inhabit = Some").expect("stage inhabit");
    assert!(
        inhabit[staged..].contains("self.persist_hub()") && !inhabit.contains("persist_snap"),
        "/inhabit must persist the staged bundle without cloning every thread: {inhabit}"
    );
    assert!(
        inhabit.contains("No paired peer named") && inhabit.contains("to_id"),
        "/inhabit must name a real peer: {inhabit}"
    );
    let soul = inhabit
        .find("read_memory(\"SOUL.md\")")
        .expect("inhabit soul");
    assert!(
        inhabit[..soul].contains("write_memory") && inhabit[..soul].contains("mem_body"),
        "/inhabit must flush the Memory editor before packing SOUL.md: {inhabit}"
    );
    let soul_spawn = inhabit[..soul]
        .rfind("thread::spawn")
        .expect("inhabit soul must leave the UI thread");
    let flush = inhabit[..soul].find("write_memory").expect("inhabit flush");
    assert!(
        soul_spawn > flush && inhabit.contains("inhabit_rx"),
        "/inhabit must not freeze the cabin packing a 1MB SOUL.md: {inhabit}"
    );
    let greet = src
        .split("fn refresh_greeting")
        .nth(1)
        .and_then(|s| s.split("fn spawn_greeting_llm").next())
        .expect("refresh_greeting");
    let user = greet.find("read_memory(\"USER.md\")").expect("greet user");
    assert!(
        greet[..user].contains("write_memory")
            && greet[..user].contains("mem_body")
            && greet[..user].contains("scratch()"),
        "empty-chat greeting must flush the Memory editor before reading USER/MEMORY: {greet}"
    );
    assert!(
        greet[..user].contains("memory_updated_at"),
        "empty-chat greeting must not slurp USER/MEMORY on every paint: {greet}"
    );
    assert!(
        greet.contains("greeting_files_rx") && greet.contains("greeting_user_at == 0"),
        "mtime-changed USER/MEMORY must leave the UI thread after the first miss: {greet}"
    );
    let greet_spawn = greet
        .find("thread::spawn")
        .expect("greeting flush must leave the UI thread");
    let greet_write = greet.find("write_memory").expect("greeting write_memory");
    assert!(
        greet_spawn < greet_write,
        "empty-chat greeting must not freeze the cabin writing MEMORY.md: {greet}"
    );
    assert!(
        !greet.contains("device_name"),
        "hostname must not paint as the empty-home greeting: {greet}"
    );
    assert!(
        greet.contains("as_str()")
            && !greet.contains("greeting_user_md.clone()")
            && !greet.contains("greeting_memory_md.clone()"),
        "empty-chat greeting must not clone USER/MEMORY every paint: {greet}"
    );
    assert!(
        greet.contains("local_greeting") && !greet.contains("spawn_greeting_llm"),
        "empty-home greeting must stay local (no Fast-model paraphrase on paint): {greet}"
    );
    let dream = src
        .split("fn run_dream")
        .nth(1)
        .and_then(|s| s.split("fn dispatch_send").next())
        .expect("run_dream");
    assert!(
        dream.contains("visible_host_receipts") && dream.contains("dream_rewind_id"),
        "/dream must use this tab's host receipts, not cabin-global last_receipts: {dream}"
    );
    let key = dream.find("llm_ready").expect("dream auth");
    let push = dream.find("live_mut().push").expect("dream push");
    assert!(
        key < push && dream.contains("self.running"),
        "/dream must not persist a turn when Imagine cannot start: {dream}"
    );
    assert!(
        dream.contains("stamp_current_access") || dream.contains("accessed_ms"),
        "/dream must bump accessed_ms or /sync LWW can drop the dream turn: {dream}"
    );
    let night = src
        .split("fn last_night_hint")
        .nth(1)
        .and_then(|s| s.split("fn mark_auto_ran").next())
        .expect("last_night_hint");
    assert!(
        night.contains("visible_host_receipts") && !night.contains("last_receipts"),
        "greeting last-night must not mix another tab's receipts: {night}"
    );
    let vis = src
        .split("fn visible_host_receipts(")
        .nth(1)
        .and_then(|s| s.split("fn dream_rewind_id").next())
        .expect("visible_host_receipts");
    assert!(
        vis.contains("thread_host_receipts_from") && !vis.contains("content.clone()"),
        "empty-chat greeting must not clone an 8MB transcript to read host receipts: {vis}"
    );
    let context = src
        .split("Slash::Context =>")
        .nth(1)
        .and_then(|s| s.split("Slash::Health =>").next())
        .expect("Context");
    assert!(
            context.contains("visible_turn_count")
                && context.contains("estimate_messages")
                && context.contains("grok_usage")
                && context.contains("grok_context_line")
                && !context.contains("content.clone()"),
            "/context must prefer Grok Build server tokens without cloning an 8MB transcript: {context}"
        );
    let finish = src
        .split("fn finish_hub_dispatch")
        .nth(1)
        .and_then(|s| s.split("fn hide_to_tray").next())
        .expect("finish_hub_dispatch");
    assert!(
        finish.contains("self.pending_hub_task.clone()")
            && finish.contains("clear_pending_after_complete"),
        "do not drop pending_hub_task before the hub mutex is held"
    );
    let complete_at = finish.find("complete_task").expect("complete_task");
    let persist_at = finish
        .find("self.persist_hub()")
        .expect("persist hub complete");
    assert!(
        complete_at < persist_at && !finish.contains("persist_snap"),
        "/send task completion must hit hub-state.json without cloning every thread: {finish}"
    );
    let host_done = src
        .split("Ok(JobOut::HostDone(block))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::Connector").next())
        .expect("HostDone");
    assert!(
        host_done.contains("pending_connectors"),
        "queued connectors must run after host before the next kick_model"
    );
    let consult = src
        .split("fn run_consult")
        .nth(1)
        .and_then(|s| s.split("fn open_palette").next())
        .expect("run_consult");
    assert!(
        consult.contains("if self.running") && consult.contains("halt_in_flight"),
        "consult must not drop a finished parent reply: {consult}"
    );
    assert!(
        consult.contains("if self.chat_job_thread.is_none()"),
        "consult must stay on the origin thread: {consult}"
    );
    assert!(
        consult.contains("Interrupted by consult"),
        "slash consult during a /send job must fail the dispatch: {consult}"
    );
    let consult_out = src
        .split("Ok(JobOut::Consult(detail))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::HostLine").next())
        .expect("Consult");
    assert!(
        !consult_out.contains("finish_hub_dispatch"),
        "consult must not complete a /send task as the consult reply: {consult_out}"
    );
    assert!(
        consult_out.contains("status.clear()") || consult_out.contains("status ="),
        "consult must not leave the status bar on Consult… after the reply lands: {consult_out}"
    );
    let slash_consult = src
        .split("Slash::Consult")
        .nth(1)
        .and_then(|s| s.split("Slash::Usage").next())
        .expect("Slash::Consult");
    assert!(
        slash_consult.contains("run_consult"),
        "typed /consult must use the consult worker: {slash_consult}"
    );
    let chat_consult = fn_src(&src, "run_consult");
    let finish_at = chat_consult.find("finish_hub_dispatch");
    let run_at = chat_consult.find("grok_chat");
    assert!(
        finish_at.is_some_and(|f| run_at.is_some_and(|r| f < r)),
        "finish the /send task before starting consult: {chat_consult}"
    );
    let rewind = src
        .split("fn rewind_project")
        .nth(1)
        .and_then(|s| s.split("fn snapshot_project").next())
        .expect("rewind_project");
    let restoring = rewind.find("Restoring").expect("Restoring");
    let blocked = rewind.find("rewind_blocked_reason").expect("rewind gate");
    assert!(
        blocked < restoring && !rewind.contains("Restored"),
        "/rewind must not claim Restored before cp finishes or when host cannot start: {rewind}"
    );
    let queued = rewind.find("queue_sh").expect("queue restore");
    assert!(
        queued < restoring && rewind[queued..restoring].contains("self.running"),
        "/rewind must not claim Restoring when host did not start: {rewind}"
    );
    let took = rewind.find("took one").expect("first snapshot");
    let snap_q = rewind.rfind("queue_sh").expect("queue snapshot");
    assert!(
        snap_q < took && rewind[snap_q..took].contains("self.running"),
        "/rewind must not claim a snapshot started when host did not start: {rewind}"
    );
    assert!(
            rewind.contains("rewind_copy_cmd"),
            "/rewind restore must copy snapshot contents into the project, not nest the dest folder: {rewind}"
        );
    assert!(
        rewind.contains("expand_home"),
        "/rewind must expand ~ before quoting the bound tree: {rewind}"
    );
    let snap = src
        .split("fn snapshot_project")
        .nth(1)
        .and_then(|s| s.split("fn doctor_text").next())
        .expect("snapshot_project");
    assert!(
        snap.contains("rewind_blocked_reason")
            && snap.contains("rewind_copy_cmd")
            && !snap.contains("run_cmds"),
        "snapshot must record a dest and return the cp, not nest run_cmds: {snap}"
    );
    assert!(
        snap.contains("expand_home"),
        "snapshot must expand ~ before quoting the bound tree: {snap}"
    );
    let snap_spawn = snap
        .find("thread::spawn")
        .expect("rewind index must leave the UI thread");
    let snap_write = snap.find("save_rewinds").expect("save_rewinds");
    assert!(
        snap_spawn < snap_write,
        "snapshot must not freeze the cabin writing rewind.json: {snap}"
    );
    assert!(
        src.contains("is_rewind_copy_cmd"),
        "host jobs must prepend a snapshot instead of nesting run_cmds"
    );
    assert!(
        src.contains("expand_home(&restore_bound_path"),
        "boot must expand a tilde-bound project or rewind quotes a literal ~ folder"
    );
    let dream_id = src
        .split("fn dream_rewind_id")
        .nth(1)
        .and_then(|s| s.split("fn run_dream").next())
        .expect("dream_rewind_id");
    assert!(
        dream_id.contains("expand_home"),
        "/dream rewind cite must expand ~ before matching the snapshot root: {dream_id}"
    );
    let host_done = src
        .split("Ok(JobOut::HostDone(block))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::Connector").next())
        .expect("HostDone");
    assert!(
        host_done.contains("job_is_scratch"),
        "HostDone must use the origin thread scratch flag: {host_done}"
    );
    assert!(
        host_done.contains("parse_computer_op") && !host_done.contains("parse_computer_cmd_loose"),
        "a leftover type cargo in last_host must not be labeled COMPUTER_RESULT: {host_done}"
    );
    assert!(
        host_done.contains("append_host_trajectory") && host_done.contains("trim_job_result_dumps"),
        "HostDone must record a trajectory line and trim old tool dumps: {host_done}"
    );
    let traj = src
        .split("fn append_host_trajectory(")
        .nth(1)
        .and_then(|s| s.split("fn trim_job_result_dumps").next())
        .expect("append_host_trajectory");
    let traj_spawn = traj
        .find("thread::spawn")
        .expect("trajectory must leave the UI thread");
    let traj_write = traj.find("append_trajectory").expect("append_trajectory");
    assert!(
        traj_spawn < traj_write,
        "HostDone must not freeze the cabin rewriting a 2MB trajectory.jsonl: {traj}"
    );
    let trim = src
        .split("fn trim_job_result_dumps(")
        .nth(1)
        .and_then(|s| s.split("fn queue_sh(").next())
        .expect("trim_job_result_dumps");
    let est = trim
        .find("should_trim_result_bodies")
        .expect("estimate before clone");
    assert!(
        trim.contains("trim_result_bodies_in_place") && !trim.contains("content.clone()"),
        "result trim must rewrite old dumps in place, not clone an 8MB pane: {trim}"
    );
    assert!(
        trim[est..].contains("trim_result_bodies_in_place"),
        "result trim must estimate borrowed tokens before touching dumps: {trim}"
    );
    let commit = src
        .split("fn commit_proposed_skill(")
        .nth(1)
        .and_then(|s| s.split("fn apply_review_skill_patches").next())
        .expect("commit_proposed_skill");
    let commit_spawn = commit
        .find("thread::spawn")
        .expect("skill write must leave the UI thread");
    let commit_save = commit.find("save_skill").expect("save_skill");
    assert!(
        commit_spawn < commit_save && !commit.contains("list_skills"),
        "HostDone must not freeze the cabin writing SKILL.md: {commit}"
    );
    let verify = src
        .split("fn apply_verify_result(")
        .nth(1)
        .and_then(|s| s.split("fn replay_saved_recipe(").next())
        .expect("apply_verify_result");
    let verify_spawn = verify
        .find("thread::spawn")
        .expect("verify skill write must leave the UI thread");
    let verify_save = verify.find("save_skill").expect("save_skill");
    assert!(
        verify_spawn < verify_save,
        "verify pass must not freeze the cabin writing SKILL.md: {verify}"
    );
    let ran = src
        .split("fn mark_auto_ran(")
        .nth(1)
        .and_then(|s| s.split("fn mark_auto_skipped(").next())
        .expect("mark_auto_ran");
    let ran_spawn = ran
        .find("thread::spawn")
        .expect("night save must leave the UI thread");
    let ran_save = ran.find("night::save").expect("night::save");
    assert!(
        ran_spawn < ran_save,
        "night run stamp must not freeze the cabin writing automations.json: {ran}"
    );
    let run_cmds = src
        .split("fn run_cmds")
        .nth(1)
        .and_then(|s| s.split("fn run_connector").next())
        .expect("run_cmds");
    assert!(
        run_cmds.contains("mint_host_halt") && !run_cmds.contains("host_halt.store(false"),
        "a new host job must not clear the previous job's halt flag: {run_cmds}"
    );
    assert!(
        run_cmds.contains("!is_rewind_copy_cmd") && run_cmds.contains("host_cmd_leaves_project"),
        "cabin rewind copies must run when YOLO is off: {run_cmds}"
    );
    assert!(
        run_cmds.contains("AlwaysApprove") && !run_cmds.contains("cfg.yolo"),
        "bound-tree jail follows the Always pill, not leftover app.json yolo: {run_cmds}"
    );
    let impl_src = src.as_str();
    assert!(
        !impl_src.contains("if let Some(plan) = plan_from_text"),
        "Chat complete must not parse HOST_CMD / COMPUTER_CMD; Grok Build owns tools"
    );
}

#[test]
fn mode_status_does_not_treat_ladder_default_as_auto_pin() {
    assert_eq!(
        super::mode_status_line("auto", "grok-3-mini-fast"),
        "Mode auto — routes Fast / Balance / Think / Max"
    );
    assert_eq!(
        super::mode_status_line("auto", "grok-4.6"),
        "Mode auto — routes Fast / Balance / Think / Max"
    );
    assert_eq!(
        super::mode_status_line("auto", "grok-4.7"),
        "Mode auto — routes Fast / Balance / Think / Max"
    );
    assert_eq!(
        super::mode_status_line("auto", "grok-3"),
        "Mode auto → grok-3"
    );
    assert_eq!(
        super::mode_status_line("think", "grok-3"),
        "Mode think → grok-4.7 · high"
    );
    assert_eq!(
        super::mode_status_line("max", ""),
        "Mode max → grok-4.7 · xhigh"
    );
}

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
    assert!(
        slice.contains("muted()"),
        "greeting uses secondary paint, not a washed-out whisper"
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
#[allow(clippy::assertions_on_constants)] // pins design constants
fn rail_footer_is_reserved() {
    assert_eq!(super::RAIL_FOOTER_H, 52.0);
    assert!(super::PALETTE_LIST_H < 400.0);
}

#[test]
fn rail_chat_title_stays_short() {
    assert_eq!(
        grokhub_core::display_tab_title("chowder and food interest and cho"),
        "chowder"
    );
    with_fonts_ui(|ui| {
        let painted = super::fit_rail_label(ui, "chowder and food interest and cho", 72.0);
        assert!(
            painted.chars().count() < 20,
            "rail label must not run off the pill: {painted}"
        );
        assert!(painted.ends_with('…') || painted == "chowder", "{painted}");
    });
}

#[test]
fn appearance_tab_offers_light() {
    let ids: Vec<&str> = grokhub_core::appearance_choices()
        .iter()
        .copied()
        .map(grokhub_core::theme_id)
        .collect();
    assert_eq!(ids, vec!["dark", "light", "system"]);
    assert_eq!(
        grokhub_core::parse_theme("light"),
        grokhub_core::ThemeChoice::Light
    );
    assert!(!grokhub_core::resolve_dark(
        grokhub_core::ThemeChoice::Light,
        true
    ));
    assert!(grokhub_core::resolve_dark(
        grokhub_core::ThemeChoice::Dark,
        false
    ));
    assert!(!grokhub_core::resolve_dark(
        grokhub_core::ThemeChoice::System,
        false
    ));
}

#[test]
fn presence_ring_drops_a_huge_frame() {
    let src = cabin_src();
    let push = src
        .split("fn push_presence(")
        .nth(1)
        .and_then(|s| s.split("fn live_room(").next())
        .expect("push_presence");
    assert!(
        push.contains("FRAME_CAP"),
        "live presence must not keep an 8MB JPEG data URL for ten minutes: {push}"
    );
    assert!(
        push.contains("PRESENCE_RING_MAX")
            || (push.contains("presence_ring.len()") && push.contains("32")),
        "a 10-minute ring of FRAME_CAP JPEGs can still OOM live Eyes: {push}"
    );
}

#[test]
fn last_frame_url_drops_a_huge_capture() {
    let src = cabin_src();
    let impl_src = src.as_str();
    let remember = impl_src
        .split("fn remember_last_frame(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .expect("remember_last_frame");
    assert!(
        remember.contains("FRAME_CAP"),
        "last_frame_url must not keep an 8MB grim data URL: {remember}"
    );
    let assigns = impl_src.matches("last_frame_url = Some").count();
    assert!(
        assigns <= 1,
        "every last-frame write must go through remember_last_frame, found {assigns}"
    );
    let hub_frame = impl_src
        .split("fn store_hub_frame(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .expect("store_hub_frame");
    let parse = hub_frame.find("store_frame").expect("parse jpeg");
    let lock = hub_frame.find("hub.lock").expect("hub lock");
    assert!(
        parse < lock && hub_frame.contains("install_frame"),
        "cabin must not decode a 400KB JPEG under hub.lock(): {hub_frame}"
    );
}

#[test]
fn eyes_frame_tex_rejects_a_huge_frame() {
    let src = cabin_src();
    let tex = src
        .split("fn eyes_frame_tex(")
        .nth(1)
        .and_then(|s| s.split("fn project_row_active(").next())
        .expect("eyes_frame_tex");
    let cap = tex
        .find("IMAGE_FILE_CAP")
        .expect("size check before decode");
    let decode = tex.find("load_from_memory").expect("decode");
    let spawn = tex
        .find("thread::spawn")
        .expect("decode must leave the UI thread");
    assert!(
        spawn < decode && cap < decode,
        "Eyes last-frame paint must not decode a huge JPEG on the UI thread: {tex}"
    );
    assert!(
        tex.contains("image_pixels_ok") || tex.contains("IMAGE_PIXEL_CAP"),
        "Eyes last-frame paint must not decode a pixel bomb on the UI thread: {tex}"
    );
}

#[test]
fn thought_uses_live_theme_tokens() {
    let src = cabin_src();
    let start = src.find("ChatKind::Thought =>").expect("thought");
    let slice = &src[start..start + 1600];
    assert!(slice.contains("theme::muted()"), "{slice}");
    assert!(
        slice.contains("Thought process"),
        "thinking must be marked as thought process, not a chat bubble: {slice}"
    );
    assert!(!slice.contains("theme::MUTED"));
    assert!(!slice.contains("theme::SUBTLE"));
    let bubble = src
        .split("fn paint_thought_bubble(")
        .nth(1)
        .and_then(|s| s.split("fn paint_chat_block(").next())
        .expect("paint_thought_bubble");
    assert!(
        bubble.contains("theme::subtle()"),
        "thought words must be darker than chat fg: {bubble}"
    );
    assert!(
        !bubble.contains("theme::fg()"),
        "thought words must not use chat fg: {bubble}"
    );
    assert!(
        bubble.contains("TRANSPARENT"),
        "thoughts stay flush on the canvas, no elevated card: {bubble}"
    );
    assert!(
        !bubble.contains("theme::surface()"),
        "thoughts must not sit on a surface card: {bubble}"
    );
    assert!(
        !bubble.contains("USER_BUBBLE_RADIUS") && !bubble.contains("bubble_assistant()"),
        "thought process is not a chat bubble: {bubble}"
    );
}

#[test]
fn tool_work_starts_collapsed() {
    let src = cabin_src();
    let tools = src
        .split("fn paint_tool_cards(")
        .nth(1)
        .and_then(|s| s.split("fn paint_running(").next())
        .expect("paint_tool_cards");
    assert!(
        tools.contains("CollapsingHeader") && tools.contains("default_open(false)"),
        "finished work must sit in a collapsed tree: {tools}"
    );
    assert!(
        tools.contains("Work"),
        "the collapsed tree is labeled Work: {tools}"
    );
    let live = src
        .split("fn paint_one_tool_card(")
        .nth(1)
        .and_then(|s| s.split("fn paint_tool_card_body(").next())
        .expect("paint_one_tool_card");
    assert!(
        live.contains("CollapsingHeader") && live.contains("default_open(false)"),
        "live tool calls must start collapsed: {live}"
    );
    let block = src
        .split("ChatKind::Tool => {")
        .nth(1)
        .and_then(|s| s.split("fn screen_from_rows(").next())
        .expect("tool arm");
    assert!(
        block.contains("CollapsingHeader") && block.contains("default_open(false)"),
        "Hands / tool rows must start collapsed: {block}"
    );
    with_fonts_ui(|ui| {
        ui.allocate_ui(egui::vec2(800.0, 400.0), |ui| {
            ui.set_max_width(800.0);
            let card = grokhub_core::wire::ToolCard {
                id: "t1".into(),
                title: "run_terminal_cmd".into(),
                kind: "execute".into(),
                status: "completed".into(),
                detail: "ran ls -la and printed a long listing that must stay hidden".into(),
                diff: String::new(),
                image_data_url: None,
                raw_input: String::new(),
            };
            let body_h = ui
                .scope(|ui| {
                    super::paint_tool_card_body(ui, &card);
                })
                .response
                .rect
                .height();
            let closed_h = ui
                .scope(|ui| {
                    super::paint_one_tool_card(ui, &card);
                })
                .response
                .rect
                .height();
            assert!(
                closed_h + 16.0 < body_h,
                "collapsed tool still showed the body: closed {closed_h} body {body_h}"
            );
        });
    });
}

#[test]
fn composer_stack_drops_approve_slots() {
    let src = cabin_src();
    let start = src.find("fn ui_composer_stack").expect("composer stack");
    let end = src[start..]
        .find("fn ui_devices")
        .map(|i| start + i)
        .unwrap_or(src.len());
    let stack = &src[start..end];
    assert!(!stack.contains("SkillApprove"), "{stack}");
    assert!(!stack.contains("SaveAsSkill"), "{stack}");
    assert!(!stack.contains("HostPlan"), "{stack}");
    let bar = stack
        .split("ComposerStackSlot::ContextBar =>")
        .nth(1)
        .and_then(|s| s.split("ComposerStackSlot::SlashPalette").next())
        .expect("context bar");
    assert!(
        bar.contains("rect_filled") && bar.contains("grok_context_line"),
        "the context usage bar stays above the composer: {bar}"
    );
    assert!(
        bar.contains("paint_connectivity_status_chip"),
        "connectivity chip sits in the context bar above the composer: {bar}"
    );
    let chip = fn_src(&src, "paint_connectivity_status_chip");
    assert!(
        chip.contains("global_ui_phase") && chip.contains("badge(ui"),
        "connectivity chip uses the U1 kit badge: {chip}"
    );
    assert!(
        !bar.contains("ghost_pill(ui, \"Compact\")") && !bar.contains("Slash::Compact"),
        "Compact leaves the context bar for the titlebar menu: {bar}"
    );
    let tools = stack
        .split("ComposerStackSlot::SessionTools =>")
        .nth(1)
        .and_then(|s| s.split("ComposerStackSlot::SlashPalette").next())
        .expect("session tools");
    assert!(
        !tools.contains("Copy session") && !tools.contains("\"Export\""),
        "Copy session and Export leave the composer: {tools}"
    );
    assert!(src.contains("View plan"), "view plan stays on the thread");
    assert!(
        !src.contains("How fork works") && !src.contains("fork_offer_why"),
        "the fork offer is gone from the thread"
    );
    let menu = src
        .split("fn paint_session_actions_menu")
        .nth(1)
        .and_then(|s| s.split("fn ui_composer_stack").next())
        .expect("session menu");
    assert!(
        menu.contains("ChromeBtn::Menu")
            && menu.contains("titlebar_chrome_hit")
            && !menu.contains("resp.clicked()")
            && menu.contains("\"Compact\"")
            && menu.contains("Slash::Compact")
            && menu.contains("\"Copy session\"")
            && menu.contains("export_markdown")
            && menu.contains("Copied session")
            && menu.contains("\"Export\"")
            && menu.contains("Slash::Export"),
        "the titlebar menu keeps Compact, Copy session, and Export: {menu}"
    );
    let title = src
        .split("fn ui_titlebar(")
        .nth(1)
        .and_then(|s| s.split("fn nav_row(").next())
        .expect("titlebar");
    let min = title.find("ChromeBtn::Minimize").expect("minimize");
    let ham = title.find("paint_session_actions_menu").expect("menu");
    let drag = title.find("titlebar_should_start_drag").expect("drag");
    assert!(
        min < ham && ham < drag,
        "hamburger is allocated immediately after minimize (RTL: directly to its left)"
    );
    assert_eq!(
        super::session_menu_enabled(false, true),
        (false, false, false)
    );
    assert_eq!(super::session_menu_enabled(true, true), (true, true, true));
    assert_eq!(
        super::session_menu_enabled(false, false),
        (true, false, false),
        "usage alone still offers Compact, not Copy or Export"
    );
    let send = fn_src(&src, "send_chat");
    assert!(
        send.contains("btw_queues_without_interrupt") && send.contains("side_ask_queue"),
        "btw must queue a side ask instead of halting the live run: {send}"
    );
    let order = super::composer_stack_order();
    assert_eq!(
        order,
        &[
            super::ComposerStackSlot::AuthBanner,
            super::ComposerStackSlot::ContextBar,
            super::ComposerStackSlot::SessionTools,
            super::ComposerStackSlot::LiveWork,
            super::ComposerStackSlot::SlashPalette,
            super::ComposerStackSlot::Attach,
            super::ComposerStackSlot::Voice,
            super::ComposerStackSlot::Pill,
            super::ComposerStackSlot::Chips,
        ]
    );
}

#[test]
fn composer_chips_stay_visible_mid_thread() {
    let src = include_str!("chips.rs");
    let body = src
        .split("fn composer_chips(")
        .nth(1)
        .and_then(|s| s.split("fn take_chip_act(").next())
        .expect("composer_chips");
    assert!(
        body.contains("self.visible_chips.clone()"),
        "mid-chat uses the ranked pool, including the habit/static fallback: {body}"
    );
    assert!(
        !body.contains("Vec::new()") && !body.contains("if home"),
        "a thread with messages must not clear ranked chips: {body}"
    );
    assert!(
        body.contains("skill_offer_chip"),
        "skill offer still inserts on the same row: {body}"
    );
    let ui = include_str!("chat_ui.rs");
    let chips = ui
        .split("ComposerStackSlot::Chips =>")
        .nth(1)
        .and_then(|s| s.split("ComposerStackSlot::Attach =>").next())
        .expect("chips slot");
    assert!(
        chips.contains("composer_chips()") && chips.contains("quick_chip_row"),
        "{chips}"
    );
    assert!(
        !chips.contains("messages.is_empty()") && !chips.contains("is_empty()"),
        "chips paint when the thread has messages: {chips}"
    );
}

#[test]
fn chips_sit_below_the_composer_pill() {
    let order = super::composer_stack_order();
    let chips = order
        .iter()
        .position(|s| *s == super::ComposerStackSlot::Chips);
    let pill = order
        .iter()
        .position(|s| *s == super::ComposerStackSlot::Pill)
        .expect("pill");
    assert!(chips.is_some(), "chips belong below the composer pill");
    assert!(chips.unwrap() > pill);
    let voice = order
        .iter()
        .position(|s| *s == super::ComposerStackSlot::Voice)
        .expect("voice");
    assert!(voice < pill, "voice indicator sits above the composer pill");
}

#[test]
fn voice_mode_has_indicator_and_stop() {
    let src = cabin_src();
    let listen = src
        .split("fn listen_voice(")
        .nth(1)
        .and_then(|s| s.split("fn voice_is_on(").next())
        .expect("listen_voice");
    assert!(
        listen.contains("voice_is_on()") && listen.contains("self.leave_voice()"),
        "mic / Ctrl+G must leave a live voice session: {listen}"
    );
    assert!(
        listen.contains("listen_turn") && !listen.contains("voice_ws::start"),
        "Hey Grok is PTT on every platform — duplex must not own the mic: {listen}"
    );
    let leave = src
        .split("fn leave_voice(")
        .nth(1)
        .and_then(|s| s.split("fn paint_voice_mode_row(").next())
        .expect("leave_voice");
    assert!(
        leave.contains("Voice off"),
        "leave voice must say Voice off: {leave}"
    );
    let row = src
        .split("fn paint_voice_mode_row(")
        .nth(1)
        .and_then(|s| s.split("fn paint_voice_mic(").next())
        .expect("paint_voice_mode_row");
    assert!(
        row.contains("voice_mode_row")
            && row.contains("leave_voice")
            && row.contains("voice_strip_visible")
            && row.contains("VoiceState::Ready"),
        "voice chrome must paint the indicator and Stop: {row}"
    );
    let mic = src
        .split("fn paint_voice_mic(")
        .nth(1)
        .and_then(|s| s.split("fn capture_cabin_frame_this_turn(").next())
        .expect("paint_voice_mic");
    assert!(
        mic.contains("paint_composer_mic")
            && mic.contains("MicMood::Speaking")
            && mic.contains("Leave voice"),
        "live mic must ease while speaking and read as leave: {mic}"
    );
    let stack = src
        .split("fn ui_composer_stack")
        .nth(1)
        .and_then(|s| s.split("fn ui_devices").next())
        .expect("composer stack");
    assert!(
        stack.contains("ComposerStackSlot::Voice") && stack.contains("paint_voice_mode_row"),
        "chat composer must show the voice indicator: {stack}"
    );
    let imagine = src
        .split("fn ui_imagine(")
        .nth(1)
        .and_then(|s| s.split("fn ui_imagine_bar(").next())
        .expect("ui_imagine");
    assert!(
        imagine.contains("paint_voice_mode_row"),
        "Imagine must show the same voice indicator: {imagine}"
    );
    let voice_job = src
        .split("Ok(JobOut::Voice(t))")
        .nth(1)
        .and_then(|s| s.split("Ok(JobOut::UpdateProgress").next())
        .expect("JobOut::Voice");
    assert!(
        !voice_job.contains("voice_state = VoiceState::Idle")
            && voice_job.contains("voice_state_after_ptt_stt")
            && voice_job.contains("ptt_after_stt")
            && voice_job.contains("maybe_continue_ptt"),
        "PTT must not idle after one listen: {voice_job}"
    );
    let speak = src
        .split("fn speak_reply(")
        .nth(1)
        .and_then(|s| s.split("fn poll_eyes_cap(").next())
        .expect("speak_reply");
    assert!(
        speak.contains("voice_hold_rx") && speak.contains("maybe_continue_ptt"),
        "TTS done must re-arm listen while the line is open: {speak}"
    );
    let start = src
        .split("fn start_ptt_listen(")
        .nth(1)
        .and_then(|s| s.split("fn maybe_continue_ptt(").next())
        .expect("start_ptt_listen");
    assert!(
        start.contains("listen_turn") && !start.contains("leave_voice"),
        "the next utterance must listen without re-entering voice: {start}"
    );
    let perm = fn_src(&src, "set_permission_mode");
    assert!(
        perm.contains("persistable_permission_mode"),
        "Always must not persist: {perm}"
    );
}

#[test]
fn other_chip_threads_skip_current_scratch_and_background() {
    let mut current = crate::threads::ChatThread::new("Now", false);
    current.id = "cur".into();
    current
        .messages_mut()
        .push(("user".into(), "this chat".into()));
    let mut prev = crate::threads::ChatThread::new("Night cabin", false);
    prev.id = "prev".into();
    prev.messages_mut()
        .push(("user".into(), "paint the wall".into()));
    prev.messages_mut()
        .push(("assistant".into(), "I can sketch the first coat.".into()));
    let mut scratch = crate::threads::ChatThread::new("Scratch", true);
    scratch.id = "scr".into();
    scratch
        .messages_mut()
        .push(("user".into(), "ignore me".into()));
    // A Follow up card's chat is scheduled work: no "Continue Follow up · …" chip.
    let mut follow = crate::threads::ChatThread::new("Follow up · Host snapshot", false);
    follow.id = "fu".into();
    follow.background = true;
    follow
        .messages_mut()
        .push(("user".into(), "which ports are new".into()));
    follow
        .messages_mut()
        .push(("assistant".into(), "Disk at 91% on /home.".into()));
    let others = super::collect_other_chip_threads(&[current, prev, scratch, follow], "cur");
    assert_eq!(others.len(), 1);
    assert_eq!(others[0].title, "Night cabin");
    assert_eq!(others[0].last_user, "paint the wall");
}

#[test]
fn chat_composer_pins_stop_on_the_right() {
    let src = cabin_src();
    let start = src.find("ComposerStackSlot::Pill =>").expect("pill arm");
    // 15_000: rustfmt wrapping pushed Stop / primary_pressed past the old 12_000 window.
    // Keep Stop near the end of the arm; do not grow logic before the Stop handler.
    let pill = &src[start..start + 15000];
    assert!(
        pill.contains("composer_go_cluster_w()"),
        "Fast + mic + Stop need a reserved strip: {pill}"
    );
    assert!(
        pill.contains("composer_mid_w(") && pill.contains("composer_go_hit_w("),
        "Plus/mid/Stop widths come from the window pill, not inflated available: {pill}"
    );
    let stop = pill.find("ComposerGo::Stop").expect("stop glyph");
    let edit = pill.find("TextEdit::multiline").expect("composer field");
    assert!(
        edit < stop,
        "Send/Stop is the last sibling after an exact-width mid strip"
    );
    assert!(
        pill.contains("is_pointer_button_down_on"),
        "Stop must halt on press; click-release is eaten by the shrink feel: {pill}"
    );
    assert!(
        pill.contains("primary_pressed"),
        "go press is edge-triggered so holding Send does not immediately Stop: {pill}"
    );
    assert!(
        !pill.contains("- 180.0"),
        "180px left Fast as the pill's right edge on a 900-wide cabin"
    );
    let home = src
        .split("fn ui_empty_home")
        .nth(1)
        .and_then(|s| s.split("fn ui_composer_stack(").next())
        .expect("empty home");
    let cap = home.find("composer_pill_w").expect("pane cap");
    let after = &home[cap..];
    assert!(
        after.contains("self.greeting"),
        "greeting paints inside the capped column"
    );
    assert!(
        home.contains("scope_builder")
            && home.contains("empty_home_side_gap")
            && home.contains("top_down_justified"),
        "empty-home cluster is a tight centered column, not a full-height justified fill: {home}"
    );
    assert!(
        !home.contains("vertical_centered_justified"),
        "vertical_centered_justified fills leftover height and drops the chips: {home}"
    );
    let stack = src
        .find("for slot in composer_stack_order()")
        .expect("stack");
    let cap = &src[stack.saturating_sub(280)..stack];
    assert!(
        cap.contains("composer_pill_w("),
        "chip row must not stretch the centered column past the pane: {cap}"
    );
}

#[test]
fn composer_go_disc_stays_inside_the_pill_beside_the_mic() {
    let _paint = crate::theme::hold_paint_test();
    let states = ["send", "stop", "idle"];
    for width in [1280.0_f32, 900.0, 1920.0] {
        for row_n in [1_usize, 3] {
            for state in states {
                let mut cabin = quiet_cabin();
                let thread = crate::threads::ChatThread::new("Chat", false);
                let tid = thread.id.clone();
                cabin.threads = vec![thread];
                cabin.thread_idx = 0;
                cabin.composer = match (state, row_n) {
                    ("idle", 1) | ("stop", 1) => String::new(),
                    ("idle", _) | ("stop", _) => "\n\n".into(),
                    (_, 1) => "hello".into(),
                    _ => "one\ntwo\nthree".into(),
                };
                let rows = (cabin.composer.matches('\n').count() + 1).clamp(1, 6);
                assert_eq!(rows, row_n, "{state} at {width} did not build {row_n} rows");
                if state == "stop" {
                    cabin.running = true;
                    cabin.chat_job_thread = Some(tid);
                } else {
                    cabin.running = false;
                    cabin.chat_job_thread = None;
                }
                assert_eq!(
                    cabin.thinking_here(),
                    state == "stop",
                    "{state} rows={row_n} width={width}: thinking_here={}",
                    cabin.thinking_here()
                );
                if state != "stop" {
                    assert!(
                        cabin.composer.trim().is_empty() != (state == "send"),
                        "{state} rows={row_n} width={width}: composer={:?}",
                        cabin.composer
                    );
                }
                let ctx = egui::Context::default();
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 760.0),
                    )),
                    ..Default::default()
                };
                for _ in 0..2 {
                    let _ = crate::theme::test_pass(&ctx, raw.clone(), |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            cabin.ui_composer_stack(ui);
                        });
                    });
                }
                let (pill, mic, go) = cabin.composer_geom.expect("composer geom");
                assert!(
                        go.right() <= pill.right() - 24.0 + 0.5,
                        "{state} rows={row_n} width={width}: go.right()={} pill.right()={} (go.right()-pill.right()={})",
                        go.right(),
                        pill.right(),
                        go.right() - pill.right()
                    );
                assert!(
                    mic.right() + 8.0 <= go.left() + 0.5,
                    "{state} rows={row_n} width={width}: mic.right()={} go.left()={}",
                    mic.right(),
                    go.left()
                );
                assert!(
                    go.left() > mic.left(),
                    "{state} rows={row_n} width={width}: go.left()={} mic.left()={}",
                    go.left(),
                    mic.left()
                );
            }
        }
    }
}

#[test]
fn nightly_review_stays_quiet() {
    let src = cabin_src();
    let tick = src
        .split("fn tick_review(")
        .nth(1)
        .and_then(|s| s.split("fn review_chat_digest(").next())
        .expect("tick_review");
    assert!(
        !tick.contains("send_chat")
            && !tick.contains("Nav::Chat")
            && !tick.contains("self.running"),
        "tick_review must not open Chat or take the composer: {tick}"
    );
    let native_src = include_str!("native_unattended.rs").replace("\r\n", "\n");
    assert!(fn_src(&src, "spawn_review").contains("spawn_native_review"));
    let spawn = fn_src(&native_src, "spawn_native_review");
    assert!(
        !spawn.contains("send_chat") && !spawn.contains("Nav::Chat"),
        "spawn_review must not dump the review into chat: {spawn}"
    );
    assert!(
        !spawn.contains("self.running"),
        "spawn_review leaves the user chat free: {spawn}"
    );
    assert!(
        spawn.contains("model_for_mode(\"balanced\")"),
        "nightly review forces Balance: {spawn}"
    );
    let spawn_at = spawn
        .find("thread::spawn")
        .expect("review HTTP must leave the UI thread");
    let write = spawn.find("write_memory").expect("flush memory");
    let traj = spawn.find("read_trajectory").expect("trajectory digest");
    assert!(
        spawn_at < write && spawn_at < traj && spawn.contains("mem_body"),
        "nightly review must flush Memory and slurp trajectory off the UI thread: {spawn}"
    );
    assert!(
            !spawn[..spawn_at].contains("scratch()"),
            "Scratch is a chat tab — unsaved Memory editor edits must still reach the nightly digest: {spawn}"
        );
    let digest_fn = src
        .split("fn review_chat_digest(")
        .nth(1)
        .and_then(|s| s.split("fn spawn_review(").next())
        .expect("review_chat_digest");
    assert!(
            digest_fn.contains("thread_host_receipts")
                && !digest_fn.contains("last_receipts")
                && !digest_fn.contains("last_host"),
            "nightly review must take host receipts from the digested threads, not cabin-global last_host: {digest_fn}"
        );
    assert!(
        digest_fn.contains("digest_line_from")
            && !digest_fn.contains("content.clone()")
            && !digest_fn.contains("text.clone()"),
        "nightly review must not clone an 8MB complete into the digest: {digest_fn}"
    );
    let apply = fn_src(&src, "apply_review_reply");
    assert!(
        !apply.contains("send_chat") && !apply.contains("Nav::Chat"),
        "applying suggestions stays off the chat: {apply}"
    );
    let held = apply.split("Err(e)").nth(1).expect("review held");
    assert!(
        held.contains("last_review_day") && held.contains("save_suggestions"),
        "a held nightly review must not retry every heartbeat: {apply}"
    );
    assert!(
        apply.contains("thread::spawn") && apply.contains("save_suggestions"),
        "nightly review must not freeze the cabin writing suggestions.json: {apply}"
    );
    assert!(
        apply.contains("merge_suggestion_store"),
        "a partial nightly review must not wipe the other suggestion grids: {apply}"
    );
    assert!(
        apply.contains("CABIN_GITHUB_TOOLS") && !apply.contains("&[]"),
        "nightly review must drop already-wired GitHub tools: {apply}"
    );
    assert!(
        apply.contains("prune_live_suggestions"),
        "a successful review must drop wired GitHub tiles already sitting in the store: {apply}"
    );
    let wall = src
        .split("fn poll_wall(")
        .nth(1)
        .and_then(|s| s.split("fn tick_wall(").next())
        .expect("poll_wall");
    let ok_wall = wall
        .split("Ok(Ok(gif))")
        .nth(1)
        .and_then(|s| s.split("Ok(Err(e))").next())
        .expect("wall ok");
    let ok_spawn = ok_wall
        .find("thread::spawn")
        .expect("wall cover save must leave the UI thread");
    let ok_save = ok_wall.find("save_wall").expect("ok save_wall");
    assert!(
        ok_spawn < ok_save
            && ok_wall.contains("persist_io")
            && !ok_wall.contains("persist_snap")
            && !ok_wall.contains("self.persist()"),
        "a new wall cover must not clone every thread just to write imagine-wall.json: {wall}"
    );
    let held_wall = wall.split("Ok(Err(e))").nth(1).expect("wall held");
    let wall_spawn = held_wall
        .find("thread::spawn")
        .expect("wall save must leave the UI thread");
    let wall_save = held_wall.find("save_wall").expect("save_wall");
    assert!(
        wall_spawn < wall_save && held_wall.contains("persist_io"),
        "a held wall cover must not freeze the cabin writing imagine-wall.json: {wall}"
    );
    let tick_wall = src
        .split("fn tick_wall(")
        .nth(1)
        .and_then(|s| s.split("fn kick_wall(").next())
        .expect("tick_wall");
    assert!(
            tick_wall.contains("self.has_key()") && !tick_wall.contains("self.llm_ready()"),
            "first-run Get Started must not kick a wall cover just because grok is on disk: {tick_wall}"
        );
    assert!(
        apply.contains("apply_review_skill_patches"),
        "nightly review must patch existing skills from SUGGEST_SKILL_PATCH: {apply}"
    );
    assert!(src.contains("self.tick_review()"));
    assert!(
        src.contains("fn tick_session_suggestions(")
            && src.contains("suggestions_from_sessions")
            && src.contains("last_session_suggest_day")
            && src.contains("self.tick_session_suggestions()"),
        "quiet daily session suggestions must feed Suggestions: {src}"
    );
    assert!(
        src.contains("if !night_fired && !self.running"),
        "Review waits if Night just fired or chat is running"
    );
    let history = src
        .split("fn ui_history(")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("ui_history");
    assert!(
        history.contains("tick_history_search") && history.contains("open_history_hit"),
        "History searches as you type and a hit opens its source: {history}"
    );
    let debounce = src
        .split("fn tick_history_search(")
        .nth(1)
        .and_then(|s| s.split("fn kick_history_search(").next())
        .expect("tick_history_search");
    assert!(
        debounce.contains("HISTORY_TYPE_DELAY") && debounce.contains("history_rx.is_some()"),
        "typing must not spawn a walk of every thread per keystroke: {debounce}"
    );
    assert!(
            debounce.contains("self.history_hits.clear()"),
            "a new query must drop the previous needle's hits so a click cannot open the wrong thread: {debounce}"
        );
    let poll = src
        .split("fn poll_history_search(")
        .nth(1)
        .and_then(|s| s.split("fn poll_mem_restore(").next())
        .expect("poll_history_search");
    assert!(
        poll.contains("q == self.history_q"),
        "a finished walk must not install hits for a query the box no longer holds: {poll}"
    );
    let search = src
        .split("fn kick_history_search(")
        .nth(1)
        .and_then(|s| s.split("fn open_history_hit(").next())
        .expect("history search");
    assert!(
        search.contains("write_memory")
            && search.contains("mem_body")
            && search.contains("scratch()"),
        "History Search must flush the Memory editor before reading disk: {search}"
    );
    assert!(
        search.contains("thread::spawn"),
        "History Search must flush MEMORY.md off the UI thread: {search}"
    );
    assert!(
        search.contains("TEXT_FILE_CAP") || search.contains("search_thread_body"),
        "History Search must not join every 8MB thread on the UI thread: {search}"
    );
    assert!(
        search.contains("thread_idx") && search.contains("self.messages"),
        "History Search must include the live pane, not only persisted thread copies: {search}"
    );
    assert!(
        !search.contains("content.clone()"),
        "History Search must not clone an 8MB pane to include the live tab: {search}"
    );
    let soul = search
        .find("read_memory(\"SOUL.md\")")
        .expect("history soul");
    assert!(
        search[..soul].contains("thread::spawn") && search.contains("history_rx"),
        "History Search must slurp SOUL/USER/MEMORY off the UI thread: {search}"
    );
    assert!(
        search.contains("search_corpus_tagged")
            && search.contains("format!(\"thread:{}\", t.id)")
            && search.contains("mem:MEMORY.md"),
        "every hit must carry the thread or file it came from: {search}"
    );
    let hit = src
        .split("fn open_history_hit(")
        .nth(1)
        .and_then(|s| s.split("fn open_memory_file(").next())
        .expect("open_history_hit");
    assert!(
        hit.contains("open_memory_file") && hit.contains("Nav::Memory"),
        "a memory hit opens that file in the editor: {hit}"
    );
    assert!(
        hit.contains("switch_thread") && hit.contains("Nav::Chat"),
        "a chat hit opens the thread it came from: {hit}"
    );
    assert!(
        hit.contains("That chat is gone"),
        "a hit for a deleted thread must say so, not open the wrong chat: {hit}"
    );
    let board = fn_src(&src, "ui_board");
    assert!(
        board.contains("self.flush_board()")
            && !board.contains("self.persist()")
            && !board.contains("persist_snap"),
        "Workboard add/status must not clone every thread just to write board.json: {board}"
    );
    let flush_b = fn_src(&src, "flush_board");
    assert!(
            flush_b.contains("persist_idle_now") && flush_b.contains("save_board"),
            "Workboard flush must bump the idle key or persist_bg clones every thread 2s later: {flush_b}"
        );
    let native_src = include_str!("native_engine.rs").replace("\r\n", "\n");
    let kick = fn_src(&native_src, "kick_native_turn");
    let prompt_at = kick.find("prompt_with_image").expect("prompt");
    let note_at = kick
        .find("note_inflight_card")
        .expect("run start files a Workboards card");
    assert!(
        prompt_at < note_at,
        "Doing card is filed only after the prompt succeeds: {kick}"
    );
    assert!(
        kick.contains("self.fail_turn_start(")
            && fn_src(&src, "fail_turn_start").contains("abandon_turn_card")
            && fn_src(&src, "poll_acp").contains("abandon_turn_card")
            && fn_src(&src, "poll_single").contains("abandon_turn_card"),
        "prompt and stream failures undo the Doing card"
    );
    let finish = fn_src(&src, "finish_acp_turn");
    assert!(
        finish.contains("settle_turn_card"),
        "run complete settles that card: {finish}"
    );
    let note = fn_src(&src, "note_inflight_card");
    assert!(
        note.contains("upsert_inflight_card") && note.contains("flush_board"),
        "inflight hook writes workboard.json, not a full thread persist: {note}"
    );
    let settled = fn_src(&src, "settle_turn_card");
    assert!(
        settled.contains("settle_inflight_card")
            && settled.contains("apply_assistant_work_marks")
            && settled.contains("flush_board"),
        "complete moves doing to done and applies WORK_PIN lines: {settled}"
    );
    let menu = fn_src(&src, "board_card_menu");
    assert!(
        board.contains("Workboards")
            && board.contains("paint_board_card")
            && menu.contains("Open chat")
            && menu.contains("Archive"),
        "Workboards page is the kanban, with a thread link in each card's menu: {board}"
    );
    let open = fn_src(&src, "open_board_thread");
    assert!(
        open.contains("switch_thread") && open.contains("Nav::Chat"),
        "Open chat leaves the board on the rail and shows the linked thread: {open}"
    );
    let night = format!(
        "{}{}",
        fn_src(&src, "ui_night"),
        fn_src(&src, "ui_scheduled_automations")
    );
    assert!(
        night.contains("merge_suggested_autos"),
        "Loops Suggested uses learned tiles first: {night}"
    );
    assert!(
        night.contains("Suggested from your recent work"),
        "Suggested says these tiles come from recent work: {night}"
    );
    assert!(
        !night.contains("review_status_line"),
        "night review status stays off the Automations Suggested line: {night}"
    );
    assert!(
        night.contains("/loop") && night.contains("New job") && night.contains("grok_loops"),
        "Automations page still owns the Grok Build /loop list: {night}"
    );
    assert!(
        !night.contains("Follow along")
            && !night.contains("Teach this once")
            && !night.contains("teach_watched_routine")
            && !night.contains("watch_once")
            && !night.contains("teach_nl")
            && night.contains("New job")
            && night.contains("Suggested")
            && night.contains("Loops")
            && !night.contains("thread::spawn"),
        "Automations must drop Follow along / Teach this once: {night}"
    );
    assert!(
        night.contains("ui_scheduled_automations") && night.contains("self.automations"),
        "Automations page must also show the clock jobs the pulse fires: {night}"
    );
    let sched = fn_src(&src, "ui_scheduled_automations");
    assert!(
        sched.contains("automation_summary_line")
            && sched.contains("fire_night")
            && sched.contains("persist_automations")
            && !sched.contains("self.persist()"),
        "a clock job needs its schedule, Run, Remove, and an off-thread persist: {sched}"
    );
    assert!(
        sched.contains("ensure_automation_schedule"),
        "re-enabling a paused job has to find its next slot: {sched}"
    );
    let enable = night
        .split("enabled_switch")
        .nth(1)
        .and_then(|s| s.split("ui.vertical").next())
        .expect("loop enable");
    assert!(
        enable.contains("persist_loops"),
        "toggling a loop must persist enabled before restart: {enable}"
    );
    assert!(
            night.contains("persist_loops") && !night.contains("self.persist()"),
            "removing a loop must not clone every thread 2s later — persist_loops bumps the idle key: {night}"
        );
    let added = src
        .split("fn add_automation_seed(")
        .nth(1)
        .and_then(|s| s.split("fn ui_night(").next())
        .expect("add_automation_seed");
    assert!(
        added.contains("save_schedule") && added.contains("persist_loops"),
        "Add must route the seed and persist loops.json off the UI thread: {added}"
    );
    assert!(
        added.contains("dismiss_accepted_auto") && added.contains("persist_suggestions"),
        "Accept must drop the Suggested tile from store + persist every time: {added}"
    );
    assert!(
        !added.contains("self.persist()"),
        "Add must not clone every thread 2s later: {added}"
    );
    assert!(
        !night.contains("take(40)"),
        "loop and scheduled titles wrap — no mid-word take(40): {night}"
    );
    let fire = fn_src(&src, "fire_loop");
    let unattended = include_str!("native_unattended.rs").replace("\r\n", "\n");
    let native_loop = fn_src(&unattended, "spawn_native_loop");
    let gate = fn_src(&unattended, "scheduled_gate");
    assert!(
        fire.contains("spawn_native_loop")
            && native_loop.contains("thread::spawn")
            && native_loop.contains("scheduled_gate(self")
            && gate.contains("cabin.permission_mode")
            && gate.contains("attended: false")
            && gate.contains("mode: perm,"),
        "loop Run must inherit the PermissionMode pill — no silent always-approve: {fire}"
    );
    let skills = src
        .split("fn ui_skills(")
        .nth(1)
        .and_then(|s| s.split("fn tick_history_search(").next())
        .expect("ui_skills");
    assert!(
        skills.contains("Project and user skills")
            && skills.contains("native_skills")
            && !skills.contains("Grok Build skills")
            && !skills.contains("grok_catalog"),
        "Skills shows the native engine's skills, never the CLI catalog: {skills}"
    );
    let connectors = include_str!("connectors_ui.rs")
        .split("#[cfg(test)]")
        .next()
        .expect("connectors body");
    assert!(
        connectors.contains("\"This cabin\"")
            && !connectors.contains("\"Grok Build\"")
            && !connectors.contains("run_grok_user_cmd")
            && !connectors.contains("run_mcp_doctor"),
        "Settings, Connectors lists the cabin's own servers and never runs the CLI: {connectors}"
    );
    assert!(
        !skills.contains("merge_suggested_skills") && !skills.contains("add_suggested_skill"),
        "Suggested skills live on the Ideas board, not the Skills page: {skills}"
    );
    let github = fn_src(&src, "ui_github_connector");
    assert!(
        github.contains("GITHUB_TILES")
            && github.contains("Save PAT")
            && github.contains("run_connector")
            && github.contains("github_token")
            && !github.contains("create_pr")
            && !github.contains("outlook")
            && !github.contains("gmail"),
        "Connectors GitHub tiles + PAT must stay read-only: {github}"
    );
    let add_skill = fn_src(&src, "add_suggested_skill");
    assert!(
        add_skill.contains("skill_from_suggestion")
            && add_skill.contains("save_skill")
            && add_skill.contains("thread::spawn"),
        "Suggested Add must write SKILL.md off the UI thread: {add_skill}"
    );
    assert!(
        !src.contains("fn send_grok_slash("),
        "no slash is sent to the engine as a literal CLI command"
    );
    let skills = src
        .split("fn ui_skills(")
        .nth(1)
        .and_then(|s| s.split("fn project_row_active(").next())
        .expect("ui_skills");
    assert!(
        !skills.contains("workflow_status_live") && !skills.contains("send_workflow_ctl"),
        "Workflows were a CLI feature; the Skills page has no Runs strip: {skills}"
    );
}

#[test]
fn stream_deltas_do_not_grow_without_bound() {
    let src = cabin_src();
    let poll = fn_src(&src, "poll_single");
    let ingest = fn_src(&src, "ingest_stream_chunk");
    assert!(
        poll.contains("GrokPEvent::Thought")
            && poll.contains("GrokPEvent::Text")
            && poll.contains("ingest_stream_chunk")
            && ingest.contains("push_stream_capped")
            && ingest.contains("IMAGE_FILE_CAP"),
        "live grok -p thought and text deltas must not grow stream buffers without bound: {poll}"
    );
    let snap = fn_src(&src, "apply_assistant_snapshot");
    assert!(
        snap.contains("take_ui_text") && snap.contains("IMAGE_FILE_CAP"),
        "a huge complete reply must not land in the transcript unbounded: {snap}"
    );
    let live = fn_src(&src, "apply_live_assistant");
    assert!(
            live.contains("merge_thinking_capped") && live.contains("TEXT_FILE_CAP"),
            "live thought+stream merge must not copy an 8MB stream into the transcript every delta: {live}"
        );
    let finish = fn_src(&src, "finish_acp_turn");
    assert!(
        finish.contains("take_ui_text") && finish.contains("IMAGE_FILE_CAP"),
        "complete grok -p text must be capped before the UI thread merges: {finish}"
    );
    let apply = fn_src(&src, "apply_single_turn");
    assert!(
        apply.contains("merge_thinking_capped") && apply.contains("TEXT_FILE_CAP"),
        "single-turn merge must stay under TEXT_FILE_CAP: {apply}"
    );
}

#[test]
fn chat_arm_checks_stream_end_followup() {
    let src = cabin_src();
    let apply = fn_src(&src, "apply_single_turn");
    let finish = fn_src(&src, "finish_acp_turn");
    let drain = fn_src(&src, "drain_followup_queue");
    let poll = fn_src(&src, "poll_single");
    assert!(
        finish.contains("take_ui_text") && finish.contains("IMAGE_FILE_CAP"),
        "complete must not strip/merge a 64MB worker body on the UI thread: {finish}"
    );
    assert!(
        apply.contains("drain_followup_queue"),
        "stream-end follow-up belongs on the grok -p complete path: {apply}"
    );
    assert!(
        drain.contains("send_chat"),
        "a queued composer follow-up still uses the typed send path: {drain}"
    );
    assert!(
            poll.contains("mem::take")
                && !poll.contains("stream_buf.clone()")
                && !poll.contains("thought_buf.clone()"),
            "disconnect complete must take the stream buffers, not clone an 8MB complete on the UI thread: {poll}"
        );
    assert!(
        finish.contains("self.persist()") && finish.contains("chat_job_thread"),
        "complete must persist the origin thread: {finish}"
    );
    let mid = fn_src(&src, "tick_mid_thought");
    assert!(
        !mid.contains("send_chat") && !mid.contains("send_followup_turn"),
        "MidThought must not auto-continue chat: {mid}"
    );
}

#[test]
fn mid_thought_stays_out_of_chat() {
    let src = cabin_src();
    let impl_src = src.as_str();
    assert!(
        !impl_src.contains("You sit down. Last night"),
        "MidThought must not inject a fake assistant turn"
    );
    let mid = fn_src(&src, "tick_mid_thought");
    assert!(
        !mid.contains("send_chat") && !mid.contains("Nav::Chat") && !mid.contains("self.running"),
        "MidThought stays quiet: {mid}"
    );
    assert!(
        mid.contains("continue_thread_hint"),
        "MidThought folds Continue {{title}} into the greeting path: {mid}"
    );
    let hint = fn_src(&src, "last_night_hint");
    assert!(
        !hint.contains("messages.push") && !hint.contains("send_chat"),
        "last-night context stays in the greeting: {hint}"
    );
    assert!(
        hint.contains("continue_hint"),
        "empty last-night falls back to continue hint: {hint}"
    );
    assert!(
        src.contains("last_night: &last_night")
            || src.contains("last_night: &self.last_night_hint()")
    );
    assert!(src.contains("self.tick_mid_thought()"));
}

#[test]
fn chat_rail_reuses_empty_draft() {
    let src = cabin_src();
    let theme = include_str!("../theme.rs");
    let chat = theme.find("(\"chat\", \"Chat\")").expect("chat rail");
    let imagine = theme
        .find("(\"imagine\", \"Imagine\")")
        .expect("imagine rail");
    assert!(chat < imagine, "Chat sits above Imagine on the rail");
    let set_nav = fn_src(&src, "set_nav_id");
    let chat_arm = set_nav
        .split("\"chat\" =>")
        .nth(1)
        .and_then(|s| s.split("_ =>").next())
        .expect("chat arm");
    assert!(
        chat_arm.contains("self.new_thread(false)"),
        "Chat rail click reuses or starts one empty draft: {chat_arm}"
    );
    assert!(
        !chat_arm.contains("open_recent_chat"),
        "Chat rail must not jump to last-access; History is for old convos: {chat_arm}"
    );
    let created = src
        .split("fn new_thread")
        .nth(1)
        .and_then(|s| s.split("fn begin_chat_rename").next())
        .expect("new_thread");
    assert!(
        created.contains("reuse_empty_thread_idx") && created.contains("has_session"),
        "Chat must reuse an empty no-session draft instead of stacking Chats: {created}"
    );
    assert!(
        created.contains("composer_want_focus = true"),
        "Chat rail must put the cursor in the composer: {created}"
    );
    let side = fn_src(&src, "ui_sidebar");
    assert!(
        !side.contains("\"New chat\"") && !side.contains("RailIcon::Compose"),
        "sidebar must not keep a separate New chat button: {side}"
    );
    assert!(
        side.contains("apply_tab_act"),
        "sidebar History clicks go through the shared chat-row action: {side}"
    );
    let tab_act = fn_src(&src, "apply_tab_act");
    assert!(
        tab_act.contains("composer_want_focus = true"),
        "sidebar History clicks must focus the composer: {tab_act}"
    );
    let palette = src
        .split("fn run_palette(")
        .nth(1)
        .and_then(|s| s.split("fn run_slash_line(").next())
        .expect("run_palette");
    assert!(
        palette.contains("\"nav:chat\"") && palette.contains("self.new_thread(false)"),
        "palette Chat uses the same empty-draft reuse as the rail: {palette}"
    );
    assert!(
        palette.contains("file:")
            && palette.contains("palette_file_shown")
            && palette.contains("desktop::open_path"),
        "picking a palette file must open it, not only write status: {palette}"
    );
    let land = src
        .split("fn land_on_real_chat(")
        .nth(1)
        .and_then(|s| s.split("fn new_thread(").next())
        .expect("land_on_real_chat");
    assert!(
        land.contains("scratch()") && land.contains("apply_switch_thread"),
        "background chat must leave Scratch for the last real thread: {land}"
    );
    assert!(
        land.contains("most_recently_accessed_index") && !land.contains("self.persist()"),
        "leaving Scratch for a night/inbox job must not clone every thread twice: {land}"
    );
    let house = src
        .split("HeartbeatAct::Housekeep =>")
        .nth(1)
        .and_then(|s| s.split("HeartbeatAct::Inbox =>").next())
        .expect("housekeep");
    assert!(
        !house.contains("stamp_current_access"),
        "sitting on a chat is not activity and must not move that History row: {house}"
    );
    let idle = src
        .split("JobKind::Reflect =>")
        .nth(1)
        .and_then(|s| s.split("JobKind::Indexers =>").next())
        .expect("idle reflect");
    assert!(
        idle.contains("scratch()"),
        "idle reflect must not consume the slot on Scratch: {idle}"
    );
    let mut older = crate::threads::ChatThread::new("Older", false);
    older.accessed_ms = 1_000;
    let mut newer = crate::threads::ChatThread::new("Night cabin", false);
    newer.accessed_ms = 8_000;
    let mut scratch = crate::threads::ChatThread::new("Scratch", true);
    scratch.accessed_ms = 9_000;
    assert_eq!(
        crate::threads::most_recently_accessed_index(&[older, newer, scratch]),
        Some(1)
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
        .and_then(|s| s.split("fn nav_row(").next())
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
fn cabin_2109_must_ships() {
    let src = cabin_src();
    let night = fn_src(&src, "ui_night");
    assert!(
        !night.contains("Follow along") && !night.contains("Teach this once"),
        "{night}"
    );
    assert!(
        night.contains("New job")
            && night.contains("Loops")
            && night.contains("Suggested")
            && night.contains("ui_scheduled_automations"),
        "{night}"
    );
    let added = fn_src(&src, "add_automation_seed");
    assert!(
        added.contains("dismiss_accepted_auto") && added.contains("persist_suggestions"),
        "{added}"
    );
    let session = fn_src(&src, "tick_session_suggestions");
    assert!(
        session.contains("suggestions_from_sessions")
            && session.contains("persist_suggestions")
            && session.contains("last_session_suggest_day")
            && !session.contains("send_chat"),
        "{session}"
    );
    let review = fn_src(&src, "tick_review");
    assert!(review.contains("tick_session_suggestions"), "{review}");
    let chips = include_str!("chips.rs");
    let chip_row = chips
        .split("pub fn quick_chip_row(")
        .nth(1)
        .and_then(|s| s.split("// quick_chip_row_end").next())
        .expect("quick_chip_row");
    assert!(
        chip_row.contains("with_main_wrap(false)")
            && chip_row.contains("fluid_chip_count")
            && chip_row.contains("layout_chip_label")
            && chip_row.contains("CHIP_ROW_H")
            && chip_row.contains("chip_row_visible_w")
            && !chip_row.contains("CHIP_CLUSTER_H")
            && !chip_row.contains("with_main_wrap(true)"),
        "chips are one fixed line, ellipsized, and drop overflow inside Ask anything"
    );
}

#[test]
fn settings_cabin_defaults_section() {
    let settings = include_str!("settings.rs");
    assert!(
        settings.contains("(SettingsSec::Defaults, \"Cabin defaults\")"),
        "Cabin defaults is a Settings section"
    );
    let defaults = settings
        .split("SettingsSec::Defaults => {")
        .nth(1)
        .and_then(|s| s.split("if let Some(s) = next_sec").next())
        .expect("defaults arm");
    for label in ["ROW_MODEL", "ROW_PERMISSION", "ROW_SESSION", "ROW_COLLAPSE"] {
        assert!(defaults.contains(label), "missing {label}: {defaults}");
    }
    assert_eq!(defaults.matches("settings_dropdown").count(), 3);
    assert!(defaults.contains("settings_toggle"));
    assert!(
        !defaults.contains("effort"),
        "R1: no effort row: {defaults}"
    );
    assert!(defaults.contains("cabin_default_model_id"));
    assert!(defaults.contains("set_permission_mode"));
    assert!(defaults.contains("set_session_mode"));
    assert!(defaults.contains("always_collapse_thoughts"));
    assert!(defaults.contains("persist_cfg"));
    assert!(!defaults.contains("always-approve"));
    assert!(!defaults.contains("grok_tile"));
    assert!(!defaults.contains("quick_chip_row"));
    let cards = include_str!("../cards.rs");
    assert!(
        cards.contains("pub fn session_row(") && cards.contains("(\"always-approve\", \"Always\")"),
        "composer pills stay the live session controls, including Always"
    );
}

#[test]
fn settings_permissions_editor_lists_rules_and_grants() {
    let settings = include_str!("settings.rs");
    assert!(settings.contains("(SettingsSec::Permissions, \"Permissions\")"));
    assert!(settings.contains("SettingsSec::Permissions => \"Permissions\""));
    assert!(settings.contains("SettingsSec::Permissions => self.ui_permission_editor(ui)"));
    assert!(settings.contains("fn ui_permission_editor"));
    assert!(settings.contains("load_rules"));
    assert!(settings.contains("load_grants"));
    assert!(settings.contains("load_claude_project"));
    assert!(settings.contains("remember_allow_always"));
    let chat = include_str!("chat_ui.rs");
    assert!(!chat.contains("ui_permission_editor"));
    assert!(!chat.contains("SettingsSec::Permissions"));
    let defaults = settings
        .split("SettingsSec::Defaults => {")
        .nth(1)
        .and_then(|slice| slice.split("if let Some(s) = next_sec").next())
        .expect("defaults arm");
    // Model, Permission and Session mode. R1 removed the effort dropdown.
    assert_eq!(defaults.matches("settings_dropdown").count(), 3);
}

#[test]
fn session_thought_collapse_stays_on_one_thread() {
    let ctx = egui::Context::default();
    let a = "thread-a";
    let b = "thread-b";
    assert!(!super::read_session_thoughts_collapsed(&ctx, a));
    assert!(!super::read_session_thoughts_collapsed(&ctx, b));
    let one = grokhub_core::thought_body_key("need a snapshot");
    let two = grokhub_core::thought_body_key("of the restore path");
    let bee = grokhub_core::thought_body_key("session b stays open");
    let id = |thread, key| super::thought_fold_id(thread, "body", key);
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(a, one), false),
        grokhub_core::ThoughtFold::Expanded
    );
    super::write_session_thoughts_collapsed(&ctx, a, true);
    super::write_thought_fold(&ctx, id(a, one), grokhub_core::ThoughtFold::Minimized);
    super::write_thought_fold(&ctx, id(a, two), grokhub_core::ThoughtFold::Minimized);
    assert!(super::read_session_thoughts_collapsed(&ctx, a));
    assert!(!super::read_session_thoughts_collapsed(&ctx, b));
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(b, bee), false),
        grokhub_core::ThoughtFold::Expanded,
        "session B stays expanded when Always collapse is off"
    );
    let fresh = grokhub_core::thought_body_key("a new thought in A");
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(a, fresh), true),
        grokhub_core::ThoughtFold::Minimized,
        "a new thought in a collapsed session arrives folded"
    );
    super::write_thought_fold(&ctx, id(a, one), grokhub_core::ThoughtFold::Expanded);
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(a, one), true),
        grokhub_core::ThoughtFold::Expanded
    );
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(a, two), true),
        grokhub_core::ThoughtFold::Minimized,
        "expand opens one thought"
    );
    assert!(super::read_session_thoughts_collapsed(&ctx, a));
    assert_eq!(
        super::resolve_thought_fold(&ctx, id(b, bee), true),
        grokhub_core::ThoughtFold::Minimized,
        "Always collapse starts every session folded"
    );
    let chat = include_str!("chat_ui.rs");
    assert_eq!(
        grokhub_core::thought_fold_controls(grokhub_core::ThoughtFold::Expanded),
        &["Collapse"]
    );
    assert_eq!(
        grokhub_core::thought_fold_controls(grokhub_core::ThoughtFold::Minimized),
        &["Expand"]
    );
    assert!(grokhub_core::thought_fold_controls(grokhub_core::ThoughtFold::Hidden).is_empty());
    assert!(
        chat.contains("minimize_session_thoughts")
            && chat.contains("session_thoughts_start_collapsed")
            && !chat.contains("paint_thought_fold_buttons(ui, \"Hide\")"),
        "quiet Collapse/Expand stays; Hide is not painted"
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
            && ping.contains("GrokHub"),
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
        ideas.contains("Suggest ideas") && ideas.contains("maybe_suggest_ideas(true)"),
        "the Ideas button asks the model for ideas now, not a disk reload: {ideas}"
    );
    let offer = pulse
        .split("fn accept_automate_offer(")
        .nth(1)
        .and_then(|s| s.split("fn ensure_useful_ideas(").next())
        .expect("accept_automate_offer");
    assert!(
        offer.contains("c.title.clone()")
            && offer.contains("route_schedule")
            && !offer.contains("c.body"),
        "Accept seeds the schedule from the title, not the canned offer body: {offer}"
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

/// Sidebar History order: pins last-pinned-first, then last used.
fn history_row_titles(cabin: &Cabin) -> Vec<String> {
    let listed = crate::threads::chat_section_indices(
        &cabin.threads,
        Some(cabin.thread_idx),
        cabin.messages.is_empty(),
    );
    let keys: Vec<crate::threads::SessionSortKey> = listed
        .iter()
        .map(|&i| {
            let t = &cabin.threads[i];
            crate::threads::SessionSortKey {
                pinned: t.pinned,
                pinned_ms: t.pinned_ms,
                accessed_ms: t.accessed_ms,
                list_rank: 0,
            }
        })
        .collect();
    crate::threads::session_list_order(&keys)
        .into_iter()
        .map(|pos| cabin.threads[listed[pos]].title.clone())
        .collect()
}

fn spoke(title: &str, accessed_ms: u64) -> crate::threads::ChatThread {
    let mut thread = crate::threads::ChatThread::new(title, false);
    thread.accessed_ms = accessed_ms;
    thread
        .messages_mut()
        .push(("user".into(), format!("keep {title}")));
    thread
}

#[test]
fn opening_a_history_row_keeps_its_place() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("history-click");
    let _ = std::fs::remove_dir_all(&root);
    let _cfg = RestoreEnv::set("GROKHUB_CONFIG", &root);
    let _tray = RestoreEnv::set("GROKHUB_TRAY", "0");

    let mut last_pin = spoke("Last pin", 1);
    last_pin.pinned = true;
    last_pin.pinned_ms = 80;
    let mut earlier_pin = spoke("Earlier pin", 9_000);
    earlier_pin.pinned = true;
    earlier_pin.pinned_ms = 20;
    let newer = spoke("Newer chat", 500);
    let older = spoke("Older chat", 100);
    let earlier_idx = 1;
    let newer_idx = 2;
    let older_idx = 3;
    let mut cabin = Cabin::quiet_cabin(vec![last_pin, earlier_pin, newer, older], newer_idx);
    assert!(!cabin.running, "the cabin is quiet before the click");

    let before = history_row_titles(&cabin);
    assert_eq!(
        before,
        ["Last pin", "Earlier pin", "Newer chat", "Older chat"],
        "pins stay last-pinned-first, then last used: {before:?}"
    );
    let opened_at = before.iter().position(|t| t == "Older chat").unwrap();
    assert_ne!(opened_at, 0, "the chat we open is not already first");
    let older_accessed = cabin.threads[older_idx].accessed_ms;
    let earlier_pin_ms = cabin.threads[earlier_idx].pinned_ms;
    let earlier_accessed = cabin.threads[earlier_idx].accessed_ms;

    cabin.switch_thread(older_idx);
    let after_click = history_row_titles(&cabin);
    assert_eq!(
        after_click, before,
        "switch_thread is the History click and must not move the row: {after_click:?}"
    );
    assert_eq!(cabin.thread_idx, older_idx);
    assert_eq!(
        cabin.threads[older_idx].accessed_ms, older_accessed,
        "the click must not bump accessed_ms"
    );

    // send_chat stamps the open thread with stamp_current_access. That is the
    // sent-turn bump. The click above did not call it.
    cabin.stamp_current_access();
    let after_stamp = history_row_titles(&cabin);
    assert_eq!(
        after_stamp,
        ["Last pin", "Earlier pin", "Older chat", "Newer chat"],
        "a sent turn moves that chat up, under the pins: {after_stamp:?}"
    );
    let stamped_at = after_stamp.iter().position(|t| t == "Older chat").unwrap();
    assert!(stamped_at < opened_at, "the messaged chat moved up");
    assert!(cabin.threads[older_idx].accessed_ms > older_accessed);

    let pins_before: Vec<_> = after_stamp
        .iter()
        .filter(|title| cabin.threads.iter().any(|t| t.title == **title && t.pinned))
        .cloned()
        .collect();
    assert_eq!(pins_before, ["Last pin", "Earlier pin"]);
    cabin.switch_thread(earlier_idx);
    let after_pin_click = history_row_titles(&cabin);
    let pins_after: Vec<_> = after_pin_click.iter().take(2).cloned().collect::<Vec<_>>();
    assert_eq!(
        pins_after, pins_before,
        "clicking a pinned chat leaves the pin group last-pinned-first: {after_pin_click:?}"
    );
    assert!(cabin.threads[earlier_idx].pinned);
    assert_eq!(cabin.threads[earlier_idx].pinned_ms, earlier_pin_ms);
    assert_eq!(
        cabin.threads[earlier_idx].accessed_ms, earlier_accessed,
        "clicking a pin is not activity"
    );
    assert_eq!(
        after_pin_click, after_stamp,
        "the pin click does not reorder History"
    );

    let start = std::time::Instant::now();
    while cabin.persist_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(5) {
        cabin.poll_persist();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(&root);
}

// Landed from PR #89.
#[test]
fn queue_rows_label_done_failed_and_running() {
    let rows = [
        ("job-done", "Flash the pi", true),
        ("job-fail", "Failed · boot", false),
        ("job-run", "Write the image", false),
    ];
    let painted: Vec<(&str, String)> = rows
        .iter()
        .map(|(id, title, done)| {
            let st = super::pages::queue_task_label(title, *done);
            (st, format!("{st} · {id}"))
        })
        .collect();
    assert_eq!(
        painted,
        vec![
            ("done", "done · job-done".into()),
            ("failed", "failed · job-fail".into()),
            ("running", "running · job-run".into()),
        ]
    );
}

#[test]
fn park_fresh_chat_keeps_the_running_reply_and_opens_an_empty_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("park-fresh");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut live = crate::threads::ChatThread::new("Night watch", false);
    live.pinned = true;
    live.messages = std::sync::Arc::new(vec![
        ("user".into(), "keep this reply".into()),
        ("assistant".into(), "still writing".into()),
    ]);
    let live_id = live.id.clone();
    let mut cabin = super::Cabin::quiet_for_test();
    cabin.threads = vec![live];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(live_id.clone());
    cabin.park_fresh_chat();

    assert!(cabin.running, "parking must not halt the reply");
    assert_eq!(cabin.chat_job_thread.as_deref(), Some(live_id.as_str()));
    assert!(
        cabin.messages.is_empty(),
        "the open pane must be an empty chat"
    );
    assert_ne!(
        cabin.threads[cabin.thread_idx].id, live_id,
        "the empty chat must not be the thread that is still replying"
    );
    let old = cabin
        .threads
        .iter()
        .find(|t| t.id == live_id)
        .expect("the running thread stays in the list");
    assert!(old.pinned);
    assert_eq!(old.title, "Night watch");
    assert_eq!(
        old.messages.as_slice(),
        [
            ("user".into(), "keep this reply".into()),
            ("assistant".into(), "still writing".into()),
        ]
    );

    let io = cabin.persist_io.clone();
    drop(cabin);
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(3) {
        if io.try_lock().is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(30));
            if io.try_lock().is_ok() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

/// A live turn of `rows` reply/thought rows, each followed by a tool call.
fn live_turn(rows: usize, tag: &str, lines: usize) -> Vec<grokhub_core::LiveBlock> {
    let mut blocks = Vec::new();
    for r in 0..rows {
        let body = (0..lines)
            .map(|l| {
                format!("{tag} row {r} line {l}: the dock wraps and the rail keeps its footer.")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if r % 2 == 0 {
            grokhub_core::append_say(&mut blocks, &body);
        } else {
            grokhub_core::append_thought(&mut blocks, &body);
        }
        grokhub_core::append_tool(
            &mut blocks,
            &format!("{tag}-t{r}"),
            "Edit src/dock.rs",
            "completed",
            "",
        );
    }
    blocks
}

/// Paint the live turn with only the top `clip_h` px visible.
/// Returns the turn's height and the pane width it was laid out at.
fn paint_live_clipped(ctx: &egui::Context, cabin: &mut super::Cabin, clip_h: f32) -> (f32, f32) {
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 4000.0),
        )),
        ..Default::default()
    };
    let (mut height, mut width) = (0.0, 0.0);
    let _ = crate::theme::test_pass(ctx, raw, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            let top = ui.cursor().min;
            width = ui.available_width();
            ui.set_clip_rect(egui::Rect::from_min_size(top, egui::vec2(900.0, clip_h)));
            let mut collapse = false;
            let _ = cabin.paint_live_blocks(ui, true, false, &mut collapse);
            height = ui.cursor().min.y - top.y;
        });
    });
    (height, width)
}

#[test]
fn offscreen_live_rows_reserve_their_height_and_rekey_on_a_new_turn() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("live-cull");
    let tid = cabin.visible_thread_id();
    cabin.live_blocks = live_turn(20, "first", 3);
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    // Cold: nothing cached, every row lays out.
    let (cold, pane_w) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    let row_h = |ctx: &egui::Context| -> Vec<(u64, u8, f32)> {
        ctx.data(|d| d.get_temp(super::live_row_height_id(&tid, pane_w)))
            .unwrap_or_default()
    };
    let cold_rows = row_h(&ctx);
    assert_eq!(
        cold_rows.len(),
        cabin.live_blocks.len(),
        "one slot per live block"
    );
    // Warm: rows below the clip reserve their cached height instead.
    let (warm, _) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    assert!(cold > 1000.0, "the turn must run past the clip: {cold}");
    assert!(
        (warm - cold).abs() < 0.5,
        "culled rows must reserve exactly their painted height: cold {cold} warm {warm}"
    );
    assert_eq!(
        row_h(&ctx),
        cold_rows,
        "a steady turn keeps its row heights"
    );
    for (i, b) in cabin.live_blocks.iter().enumerate() {
        let (ident, _, h) = cold_rows[i];
        if b.kind == grokhub_core::LiveKind::Tool
            && i > 0
            && cabin.live_blocks[i - 1].kind == b.kind
        {
            continue;
        }
        assert_eq!(
            ident,
            super::live_row_ident(b),
            "row {i} is keyed by its own block"
        );
        assert!(h > 0.0, "row {i} has a height");
    }

    // Minimize all while the turn streams: off-screen thoughts shrink too, so they
    // must re-measure instead of reserving their expanded height.
    for b in cabin
        .live_blocks
        .iter()
        .filter(|b| b.kind == grokhub_core::LiveKind::Thought)
    {
        super::write_thought_fold(
            &ctx,
            super::thought_fold_id(&tid, "slot", b.fold_slot),
            grokhub_core::ThoughtFold::Minimized,
        );
    }
    let (folded, _) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    let (folded_truth, _) = paint_live_clipped(&ctx, &mut cabin, 10_000.0);
    assert!(
        folded < cold,
        "minimized thoughts are shorter: {folded} vs {cold}"
    );
    assert!(
        (folded - folded_truth).abs() < 0.5,
        "a fold change must repaint off-screen rows: clipped {folded} painted {folded_truth}"
    );
    for b in cabin
        .live_blocks
        .iter()
        .filter(|b| b.kind == grokhub_core::LiveKind::Thought)
    {
        super::write_thought_fold(
            &ctx,
            super::thought_fold_id(&tid, "slot", b.fold_slot),
            grokhub_core::ThoughtFold::Expanded,
        );
    }

    // The row still streaming grows below the clip and must still be measured.
    let last_i = cabin
        .live_blocks
        .iter()
        .rposition(|b| b.kind != grokhub_core::LiveKind::Tool)
        .expect("text row");
    cabin.live_blocks.truncate(last_i + 1);
    let (before, _) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    let grown_body = "\n\nmore streamed text for the tail row.".repeat(12);
    cabin.live_blocks[last_i].body.push_str(&grown_body);
    let (after, _) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    assert!(
        after > before + 100.0,
        "the growing tail row must repaint off-screen: before {before} after {after}"
    );

    // Next turn: same row indices, new blocks and much shorter rows. No stale height.
    cabin.live_blocks = live_turn(20, "second", 1);
    let (reused, _) = paint_live_clipped(&ctx, &mut cabin, 200.0);
    // Ground truth: nothing clipped, so every row paints and measures itself.
    let (painted, _) = paint_live_clipped(&ctx, &mut cabin, 10_000.0);
    assert!(
        (reused - painted).abs() < 0.5,
        "a new turn must not reserve the last turn's heights: reused {reused} painted {painted}"
    );
    assert!(
        painted < cold,
        "the second turn's rows are shorter: {painted} vs {cold}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn transcript_caches_hold_while_the_turn_streams_and_catch_up_after() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("stream-hold");
    let mut t = crate::threads::ChatThread::new("Dock", false);
    t.messages = std::sync::Arc::new(vec![
        ("user".into(), "fix the dock".into()),
        (
            "assistant".into(),
            "THINKING:\nThe dock wraps early.\n\nFixed it.".into(),
        ),
        ("user".into(), "and the rail".into()),
        (
            "assistant".into(),
            "THINKING:\nCheck the rail.\n\nOn it".into(),
        ),
    ]);
    cabin.threads = vec![t];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    let _ = cabin.cached_chat_views();

    // A turn streams here. The first delta unshares the transcript from its thread,
    // so the views rebuild once.
    cabin.running = true;
    cabin.chat_job_thread = Some(cabin.visible_thread_id());
    grokhub_core::append_say(&mut cabin.live_blocks, "On it, the rail");
    if let Some(last) = cabin.live_tail_mut().last_mut() {
        last.1.push_str(", the rail");
    }
    let streaming = cabin.cached_chat_views().to_vec();
    // Later deltas only grow the last message, and the pane paints it from live
    // blocks: the views do not rebuild on them.
    if let Some(last) = cabin.live_tail_mut().last_mut() {
        last.1
            .push_str(" now keeps its footer and the dock stops wrapping.");
    }
    assert_eq!(
        cabin.cached_chat_views(),
        &streaming[..],
        "stream deltas skip the view rebuild"
    );

    // An edit before the last message is not a stream delta: rebuild even mid-turn.
    cabin.live_mut()[1].1 = "THINKING:\nThe dock wraps late.\n\nFixed it.".into();
    let mid = cabin.cached_chat_views().to_vec();
    assert!(
        mid.iter().any(|v| v.body.contains("wraps late")),
        "an earlier edit must rebuild the views while a turn streams: {mid:?}"
    );

    // The turn ends: the tail catches up once.
    if let Some(last) = cabin.live_tail_mut().last_mut() {
        last.1.push_str(" Done.");
    }
    cabin.running = false;
    let after = cabin.cached_chat_views().to_vec();
    assert!(
        after.last().is_some_and(|v| v.body.ends_with("Done.")),
        "the finished reply must reach the views: {after:?}"
    );
    assert!(
        after.iter().any(|v| v.body.contains("wraps late")),
        "the earlier edit survives the tail refresh: {after:?}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn live_row_height_cache_is_per_thread_and_width() {
    let a = super::live_row_height_id("thread-a", 720.0);
    assert_eq!(a, super::live_row_height_id("thread-a", 720.2));
    assert_ne!(a, super::live_row_height_id("thread-a", 1100.0));
    assert_ne!(a, super::live_row_height_id("thread-b", 720.0));
    assert_ne!(
        a,
        super::chat_row_height_id("thread-a", 720.0),
        "live rows must not share the stored rows' height list"
    );
}

#[test]
fn chat_view_fold_keys_follow_the_views_as_the_chat_changes() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("view-keys");
    let expect = |cabin: &mut super::Cabin| -> Vec<u64> {
        cabin
            .cached_chat_views()
            .iter()
            .map(|v| {
                if v.kind == grokhub_core::ChatKind::Thought {
                    grokhub_core::thought_body_key(&v.body)
                } else {
                    0
                }
            })
            .collect()
    };
    let mut dock = crate::threads::ChatThread::new("Dock", false);
    dock.messages = std::sync::Arc::new(vec![
        ("user".into(), "fix the dock".into()),
        (
            "assistant".into(),
            "THINKING:\nThe dock wraps early.\n\nHOST_CMD ls".into(),
        ),
        ("user".into(), "HOST_RESULT\nsrc".into()),
        (
            "assistant".into(),
            "THINKING:\nNow patch it.\n\nDone.".into(),
        ),
    ]);
    let mut other = crate::threads::ChatThread::new("Other", false);
    other.messages = std::sync::Arc::new(vec![
        ("user".into(), "hello".into()),
        (
            "assistant".into(),
            "THINKING:\nA different thought.\n\nHi.".into(),
        ),
    ]);
    cabin.threads = vec![dock, other];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();

    let first = expect(&mut cabin);
    assert!(
        first.iter().any(|&k| k != 0),
        "the dock chat has thoughts: {first:?}"
    );
    assert_eq!(cabin.chat_view_keys, first, "keys are built with the views");

    // A stream delta grows the last message: only the trailing stretch rekeys.
    if let Some(last) = cabin.live_mut().last_mut() {
        last.1 = "THINKING:\nNow patch it and run the tests.\n\nDone.".into();
    }
    let grown = expect(&mut cabin);
    assert_ne!(grown, first, "the grown thought has a new key");
    assert_eq!(cabin.chat_view_keys, grown, "a grown thought rekeys");

    cabin
        .live_mut()
        .push(("user".into(), "and the rail".into()));
    cabin.live_mut().push((
        "assistant".into(),
        "THINKING:\nCheck the rail.\n\nFixed.".into(),
    ));
    let turn = expect(&mut cabin);
    assert_eq!(cabin.chat_view_keys, turn, "a new turn keys its views");

    cabin.apply_switch_thread(1);
    let switched = expect(&mut cabin);
    assert_eq!(
        cabin.chat_view_keys, switched,
        "another chat rebuilds its keys"
    );
    release_isolated(&root, cabin);
}

#[test]
fn in_place_edit_refreshes_chat_views() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("view-rev");
    let last = "THINKING:\nNow patch it.\n\nDone.";
    let mut dock = crate::threads::ChatThread::new("Dock", false);
    dock.messages = std::sync::Arc::new(vec![
        ("user".into(), "fix the dock".into()),
        (
            "assistant".into(),
            "THINKING:\nThe dock wraps early.\n\nDone early.".into(),
        ),
        ("user".into(), "and the rail".into()),
        ("assistant".into(), last.into()),
    ]);
    cabin.threads = vec![dock];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();

    let fold_keys = |views: &[grokhub_core::ChatView]| -> Vec<u64> {
        views
            .iter()
            .map(|v| {
                if v.kind == grokhub_core::ChatKind::Thought {
                    grokhub_core::thought_body_key(&v.body)
                } else {
                    0
                }
            })
            .collect()
    };

    let _ = cabin.cached_chat_views();
    let before_keys = cabin.chat_view_keys.clone();
    let before_rev = cabin.messages_rev;
    assert!(
        before_keys.iter().any(|&k| k != 0),
        "the first turn has a thought: {before_keys:?}"
    );

    let count = cabin.messages.len();
    let last_len = cabin.messages.last().map(|m| m.1.len()).unwrap_or(0);
    cabin.live_mut()[1].1 =
        "THINKING:\nThe title clips past the rail and the dock wraps.\n\nDone early.".into();
    assert_eq!(
        cabin.messages.len(),
        count,
        "the edit keeps the message count"
    );
    assert_eq!(
        cabin.messages.last().map(|m| m.1.len()).unwrap_or(0),
        last_len,
        "the edit keeps the last message length"
    );
    assert_eq!(cabin.messages_rev, before_rev.wrapping_add(1));

    let fresh = grokhub_core::visible_chat(&cabin.messages);
    let fresh_keys = fold_keys(&fresh);
    assert_ne!(fresh_keys, before_keys, "the earlier thought must rekey");
    assert!(
        fresh.iter().any(|v| {
            v.kind == grokhub_core::ChatKind::Thought && v.body.contains("clips past the rail")
        }),
        "the edited thought must be visible: {fresh:?}"
    );
    assert_eq!(cabin.cached_chat_views(), fresh.as_slice());
    assert_eq!(cabin.chat_view_keys, fresh_keys);
    assert_eq!(cabin.chat_view_rev, cabin.messages_rev);
    release_isolated(&root, cabin);
}

#[test]
fn idle_chat_caches_keep_the_message_revision() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("view-idle");
    let mut dock = crate::threads::ChatThread::new("Dock", false);
    dock.messages = std::sync::Arc::new(vec![
        ("user".into(), "fix the dock".into()),
        (
            "assistant".into(),
            "THINKING:\nThe dock wraps early.\n\nDone.".into(),
        ),
    ]);
    cabin.threads = vec![dock];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();

    let _ = cabin.cached_chat_views();
    let rev = cabin.messages_rev;
    let view_rev = cabin.chat_view_rev;
    let keys = cabin.chat_view_keys.clone();
    let views = cabin.chat_views.clone();
    let views_ptr = cabin.chat_views.as_ptr();
    assert_eq!(cabin.cached_chat_views().as_ptr(), views_ptr);
    assert_eq!(cabin.messages_rev, rev);
    assert_eq!(cabin.chat_view_rev, view_rev);
    assert_eq!(cabin.chat_view_keys, keys);
    assert_eq!(cabin.chat_views, views);
    release_isolated(&root, cabin);
}

#[test]
fn scrub_live_blocks_clears_live_keys_when_redaction_keeps_length() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("live-key-redact");
    let secret = "TenCharSec";
    assert_eq!(secret.len(), "[redacted]".len());
    let body = format!("look at {secret} please");
    let len = body.len();
    cabin.live_blocks.push(grokhub_core::LiveBlock {
        kind: grokhub_core::LiveKind::Thought,
        body,
        tool_id: String::new(),
        tool_title: String::new(),
        tool_status: String::new(),
        tool_detail: String::new(),
        fold_slot: 4,
    });
    cabin.live_keys = vec![(4, len, 99)];
    cabin.secret_hold.push(secret.to_string());
    cabin.scrub_live_blocks();
    assert!(
        cabin.live_keys.is_empty(),
        "same-length [redacted] must not keep the cached thought key"
    );
    assert_eq!(cabin.live_blocks[0].body.len(), len);
    assert!(cabin.live_blocks[0].body.contains("[redacted]"));
    assert!(!cabin.live_blocks[0].body.contains(secret));
    cabin.live_keys = vec![(4, len, 7)];
    cabin.scrub_live_blocks();
    assert_eq!(
        cabin.live_keys,
        vec![(4, len, 7)],
        "a scrub that does not change a body must leave the keys"
    );
    release_isolated(&root, cabin);
}

#[test]
fn selecting_plan_leaves_the_title_and_sets_plan() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("plan-title");
    let mut thread = crate::threads::ChatThread::new("Night watch", false);
    thread.messages = std::sync::Arc::new(vec![("user".into(), "dock".into())]);
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.session_mode = SessionMode::Chat;

    cabin.select_plan_without_rename();

    assert_eq!(cabin.threads[0].title, "Night watch");
    assert_eq!(cabin.session_mode, SessionMode::Plan);
    release_isolated(&root, cabin);
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
fn folder_click_toggles_and_project_click_opens_the_chat() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("project-row");
    cabin.projects = vec![
        ProjectNode {
            id: "fold".into(),
            name: "Apps".into(),
            kind: ProjectKind::Folder,
            path: String::new(),
            parent: None,
            open: false,
        },
        ProjectNode {
            id: "proj".into(),
            name: "Lab".into(),
            kind: ProjectKind::Project,
            path: "/tmp/grokhub-proof-lab".into(),
            parent: Some("fold".into()),
            open: false,
        },
    ];
    let before = cabin.threads.len();
    cabin.activate_project_row("fold");
    assert!(cabin.projects.iter().find(|n| n.id == "fold").unwrap().open);
    assert_eq!(
        cabin.threads.len(),
        before,
        "a folder click does not open a chat"
    );

    cabin.activate_project_row("proj");
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(
        cabin
            .threads
            .iter()
            .any(|t| t.project_id.as_deref() == Some("proj")),
        "a project click opens that project's chat"
    );
    assert!(
        cabin.projects.iter().find(|n| n.id == "fold").unwrap().open,
        "opening a project chat does not collapse the folder"
    );
    release_isolated(&root, cabin);
}

#[test]
fn queue_run_starts_the_origin_thread() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("queue-run");
    let visible = crate::threads::ChatThread::new("Visible", false);
    let mut origin = crate::threads::ChatThread::new("Origin", false);
    let origin_id = origin.id.clone();
    origin.messages = std::sync::Arc::new(Vec::new());
    cabin.threads = vec![visible, origin];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.agents.push(AgentJob {
        title: "Flash the pi".into(),
        status: "queued".into(),
        prompt: "write the image".into(),
        thread_id: origin_id.clone(),
    });

    assert!(cabin.start_queued_job(0));
    assert!(!cabin.running, "the state change does not spawn grok");
    assert_eq!(cabin.agents[0].status, "running");
    assert_eq!(cabin.chat_job_thread.as_deref(), Some(origin_id.as_str()));
    assert!(matches!(cabin.nav, Nav::Chat));
    let origin = cabin.threads.iter().find(|t| t.id == origin_id).unwrap();
    assert_eq!(
        origin.messages.as_slice(),
        [("user".into(), "write the image".into())]
    );
    assert!(cabin.threads[0].messages.is_empty());
    release_isolated(&root, cabin);
}

#[test]
fn shell_echo_lands_on_the_open_chat() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("shell-echo");
    let dir = root.join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    cabin.cfg.host_on = true;
    cabin.cfg.project_dir = dir.display().to_string();
    let thread = crate::threads::ChatThread::new("Shell", false);
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();

    cabin.queue_sh("echo grokhub-proof".into());
    // PowerShell on a loaded Windows runner can sit past 5s before the first
    // line. Same 45s budget as `echo_ok`. The product host cap is 90s.
    let budget = std::time::Duration::from_secs(if cfg!(windows) { 45 } else { 5 });
    let start = std::time::Instant::now();
    while start.elapsed() < budget {
        cabin.poll_job();
        let so_far = cabin
            .messages
            .iter()
            .map(|m| m.1.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if so_far.contains("grokhub-proof") {
            break;
        }
        if !cabin.running {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    cabin.poll_job();
    cabin.halt_in_flight();
    assert!(
        !cabin.running,
        "echo should finish, status {}",
        cabin.status
    );
    let text = cabin
        .messages
        .iter()
        .map(|m| m.1.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("grokhub-proof"),
        "the shell output should land on the chat: {text}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn session_menu_sits_left_of_minimize() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("menu-place");
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 80.0),
        )),
        ..Default::default()
    };
    let mut menu = egui::Rect::NOTHING;
    let mut mini = egui::Rect::NOTHING;
    let _ = crate::theme::test_pass(&ctx, raw, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                mini = titlebar_chrome_btn(ui, ChromeBtn::Minimize).rect;
                menu = cabin.paint_session_actions_menu(ui);
            });
        });
    });
    assert!(
        menu.right() <= mini.left() + 0.5,
        "the session menu must sit left of Minimize: menu {menu:?} minimize {mini:?}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn imagine_auth_source_never_names_cli_login() {
    let auth = include_str!("../imagine_auth.rs");
    let cabin = cabin_src();
    let kick = fn_src(&cabin, "kick_imagine");
    for src in [auth, kick] {
        for banned in [
            concat!("grok_cli_", "key"),
            concat!("auth", ".json"),
            concat!("refresh_grok_", "login"),
            concat!("cli-chat-", "proxy"),
        ] {
            assert!(!src.contains(banned), "{banned} in imagine auth path");
        }
    }
}

#[test]
fn kick_imagine_empty_stays_idle_and_no_key_refuses() {
    const NO_KEY: &str = "Sign in with Grok in Settings → Account, or add a console API key.";
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("imagine-nokey");
    // Imagine never uses the Grok CLI login; clear the cabin stores this path reads.
    cabin.secrets.oauth = None;
    cabin.imagine_native.tokens = None;
    assert!(cabin.console_key().trim().is_empty(), "no console key");
    assert!(cabin.secrets.oauth.is_none(), "no Account oauth");
    assert!(cabin.imagine_native.tokens.is_none(), "no Imagine keychain");

    cabin.imagine_prompt.clear();
    cabin.kick_imagine();
    assert!(!cabin.running, "an empty prompt must not start Imagine");
    assert!(cabin.rx.is_none(), "an empty prompt must not spawn a job");
    assert!(cabin.imagine_error.is_empty());

    cabin.imagine_prompt = "harbor at dusk".into();
    cabin.kick_imagine();
    assert!(!cabin.running, "no key must not start Imagine");
    assert_eq!(cabin.status, NO_KEY);
    assert_eq!(cabin.imagine_error, NO_KEY);
    assert!(cabin.rx.is_none(), "no-key refusal must not spawn a job");
    release_isolated(&root, cabin);
}

#[test]
fn update_queues_while_a_job_is_running() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("update-queue");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = super::Cabin::quiet_for_test();
    cabin.running = true;
    cabin.start_overlay_update(vec!["echo grokhub-update-queued".into()]);
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Update);
    assert_eq!(
        cabin.status,
        "Update queued — it starts when this job finishes."
    );
    assert_eq!(
        cabin.queued_overlay,
        Some(vec!["echo grokhub-update-queued".into()])
    );
    assert!(cabin.running);
    assert!(cabin.rx.is_none(), "a busy cabin must not start the update");
    drop(cabin);
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #91.
#[test]
fn kick_imagine_local_send_stores_harbor_url() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    struct RestoreEnv {
        config: Option<String>,
        tray: Option<String>,
        proxies: Vec<(String, String)>,
    }
    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            crate::xai::set_imagine_base_override(None);
            match self.config.take() {
                Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
            match self.tray.take() {
                Some(v) => std::env::set_var("GROKHUB_TRAY", v),
                None => std::env::remove_var("GROKHUB_TRAY"),
            }
            for (k, v) in self.proxies.drain(..) {
                std::env::set_var(k, v);
            }
        }
    }

    let _cfg = crate::config::hold_test_config();
    let mut restore = RestoreEnv {
        config: std::env::var("GROKHUB_CONFIG").ok(),
        tray: std::env::var("GROKHUB_TRAY").ok(),
        proxies: Vec::new(),
    };
    for key in [
        "http_proxy",
        "https_proxy",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "all_proxy",
        "ALL_PROXY",
    ] {
        if let Ok(v) = std::env::var(key) {
            restore.proxies.push((key.to_string(), v));
            std::env::remove_var(key);
        }
    }
    let root = crate::config::test_config_root("imagine-local");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_TRAY", "0");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let origin = format!("http://127.0.0.1:{port}");
    crate::xai::set_imagine_base_override(Some(&origin));

    let hits: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = hits.clone();
    listener.set_nonblocking(true).expect("nonblocking");
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .expect("timeout");
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 8192];
                    let header_end = loop {
                        let n = stream.read(&mut tmp).expect("request headers");
                        assert!(n > 0, "eof before imagine headers");
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                        assert!(buf.len() < 1_000_000, "imagine headers too large");
                    };
                    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                    let mut len = 0usize;
                    for line in head.lines() {
                        if let Some(rest) =
                            line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            len = rest.trim().parse().expect("content-length");
                        }
                    }
                    while buf.len() < header_end + len {
                        let n = stream.read(&mut tmp).expect("request body");
                        assert!(n > 0, "eof before imagine body");
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let body =
                        String::from_utf8_lossy(&buf[header_end..header_end + len]).to_string();
                    recorded
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((head, body));
                    let json = r#"{"data":[{"url":"http://127.0.0.1/harbor.png"}]}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                        json.len()
                    );
                    stream.write_all(resp.as_bytes()).expect("response");
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("accept: {e}"),
            }
        }
    });

    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.secrets.api_key = "xai-local-console".into();
    cabin.cfg.api_key.clear();
    cabin.imagine_prompt = "harbor at dusk".into();
    assert!(!cabin.running, "imagine send must start from an idle cabin");
    cabin.kick_imagine();
    assert!(
        cabin.running,
        "console key and prompt must start the imagine send, status={}",
        cabin.status
    );

    let started = Instant::now();
    while cabin.running && started.elapsed() < Duration::from_secs(8) {
        cabin.poll_job();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !cabin.running,
        "running stayed stuck: status={} err={}",
        cabin.status, cabin.imagine_error
    );
    assert_eq!(
        cabin.imagine_last, "http://127.0.0.1/harbor.png",
        "cabin must keep the URL the local server returned, status={} err={}",
        cabin.status, cabin.imagine_error
    );
    assert!(
        cabin
            .messages
            .iter()
            .any(|(_, text)| text.contains("http://127.0.0.1/harbor.png")),
        "transcript must keep the image URL: {:?}",
        cabin.messages
    );

    let got = hits.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert_eq!(got.len(), 1, "expected one imagine POST, got {got:?}");
    let (head, body) = &got[0];
    assert!(
        !head.to_ascii_lowercase().contains("api.x.ai") && !body.contains("api.x.ai"),
        "request host must not be api.x.ai: {head}"
    );
    let host = head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("host:"))
        .unwrap_or("");
    assert!(
        host.contains("127.0.0.1"),
        "POST host must be the local server: {host}"
    );
    assert!(
        head.lines().next().unwrap_or("").starts_with("POST "),
        "imagine send must POST: {head}"
    );
    let json: serde_json::Value = serde_json::from_str(body).expect("json body");
    assert_eq!(json["model"], "grok-imagine-image-2.0");
    let prompt = json["prompt"].as_str().unwrap_or("");
    assert!(
        prompt.contains("harbor at dusk"),
        "prompt must include harbor at dusk: {prompt}"
    );
    let _ = std::fs::write(
        "/opt/cursor/artifacts/imagine-local-post.json",
        body.as_bytes(),
    );
    let _ = restore;
}

#[test]
fn send_from_composer_runs_help_and_refuses_without_grok() {
    let mut quiet = QuietCabin::boot("send-help");
    let cabin = &mut quiet.cabin;
    cabin.send_from_composer("/help".into());
    let help = cabin
        .messages
        .iter()
        .find(|m| m.0 == "assistant" && m.1.contains("/help — this list"));
    assert!(
        help.is_some(),
        " /help must land on the open chat, got {:?}",
        cabin.messages
    );
    assert!(
        !cabin.running,
        "a local slash must not start a run: {}",
        cabin.status
    );

    let before = cabin.messages.len();
    cabin.send_from_composer("paint the harbor".into());
    assert!(!cabin.running, "no sign-in must refuse before a run starts");
    // The booted cabin is on the native engine, so it names GrokHub's own sign-in.
    assert_eq!(cabin.status, "Sign in with Grok or add an API key.");
    assert_eq!(
        cabin.messages.len(),
        before,
        "the refused line must not be written onto the chat"
    );
    assert!(
        cabin
            .messages
            .iter()
            .all(|m| !m.1.contains("paint the harbor")),
        "the refused prompt must stay off the transcript"
    );
    quiet.settle();
}

#[test]
fn run_slash_pin_flips_the_pin() {
    let mut quiet = QuietCabin::boot("slash-pin");
    let cabin = &mut quiet.cabin;
    let idx = cabin.thread_idx;
    let title = cabin.threads[idx].title.clone();
    assert!(!cabin.threads[idx].pinned);
    cabin.run_slash(grokhub_core::Slash::Pin);
    assert!(cabin.threads[idx].pinned, " /pin must pin the open chat");
    assert!(cabin.threads[idx].pinned_ms > 0);
    assert_eq!(cabin.status, format!("Pinned {title}"));
    quiet.settle();
}

#[test]
fn pin_thread_flips_the_pin() {
    let mut quiet = QuietCabin::boot("pin-thread");
    let cabin = &mut quiet.cabin;
    let idx = cabin.thread_idx;
    let title = cabin.threads[idx].title.clone();
    assert!(!cabin.threads[idx].pinned);
    cabin.pin_thread(idx);
    assert!(
        cabin.threads[idx].pinned,
        "the pin control must pin this chat"
    );
    assert!(cabin.threads[idx].pinned_ms > 0);
    assert_eq!(cabin.status, format!("Pinned {title}"));
    quiet.settle();
}

#[test]
fn composer_and_history_omit_archived_idle_chrome() {
    let src = cabin_src();
    let toast = fn_src(&src, "paint_archive_toast");
    assert!(
        !toast.contains("idle chat")
            && !toast.contains("idle chats")
            && !toast.contains("auto_archive_note")
            && !toast.contains("ghost_pill(ui, \"Archived\")")
            && !toast.contains("Nav::History"),
        "composer archive toast must not show idle-archive chrome: {toast}"
    );
    assert!(
        toast.contains("ghost_pill(ui, \"Undo\")"),
        "manual archive Undo toast stays: {toast}"
    );
    let live = src
        .split("ComposerStackSlot::LiveWork =>")
        .nth(1)
        .and_then(|s| s.split("ComposerStackSlot::SlashPalette =>").next())
        .expect("LiveWork slot");
    assert!(
        live.contains("paint_archive_toast") && !live.contains("idle chat"),
        "LiveWork must not embed idle-archive labels: {live}"
    );
    let hist = src
        .split("fn ui_history(")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("ui_history");
    assert!(
        !hist.contains("history_archived")
            && !hist.contains("\"Archived\"")
            && !hist.contains("Archived chats"),
        "History must not offer an Archived view: {hist}"
    );
    let side = include_str!("sidebar.rs");
    assert!(
        !side.contains("rail-archived")
            && !side.contains("RichText::new(\"Archived\")")
            && !side.contains("No archived chats"),
        "sidebar must not list Archived chats: {side}"
    );
    let kick = fn_src(&src, "kick_history_search");
    assert!(
        kick.contains("is_archived") && kick.contains("continue"),
        "History search must skip archived chats: {kick}"
    );
    let archive_row = fn_src(&src, "ui_chat_archive_row");
    assert!(
        !archive_row.contains("move to Archived")
            && archive_row.contains("leave History and search")
            && archive_row.contains("still on disk")
            && archive_row.contains("no Archived drawer")
            && !archive_row.contains("open the chat or Unarchive from its menu"),
        "Settings auto-archive must not promise an Archived drawer or post-leave recovery: {archive_row}"
    );
    let recall = src
        .split("Slash::Recall(q)")
        .nth(1)
        .and_then(|s| s.split("fn kick_model_retry").next())
        .expect("Recall");
    assert!(
        recall.contains("is_archived") && recall.contains("continue"),
        "/recall must skip archived chats like History search: {recall}"
    );
}

#[test]
fn archive_hides_from_main_list_and_reply_restores() {
    let mut quiet = QuietCabin::boot("chat-archive");
    let cabin = &mut quiet.cabin;
    cabin.threads.clear();
    let mut keep = crate::threads::ChatThread::new("Keep", false);
    keep.messages_mut().push(("user".into(), "stay".into()));
    keep.accessed_ms = 100;
    let mut done = crate::threads::ChatThread::new("Done chat", false);
    done.messages_mut().push(("user".into(), "old".into()));
    done.accessed_ms = 50;
    cabin.threads = vec![keep, done];
    cabin.thread_idx = 1;
    cabin.messages = cabin.threads[1].messages.clone();
    cabin.archive_thread_at(1);
    assert!(crate::threads::is_archived(&cabin.threads[1]));
    assert_eq!(
        crate::threads::chat_section_indices(&cabin.threads, Some(1), false),
        vec![0]
    );
    assert_eq!(cabin.status, "Archived Done chat");
    assert_eq!(
        cabin.archive_undo.as_deref(),
        Some(cabin.threads[1].id.as_str())
    );
    cabin.unarchive_on_reply();
    assert!(!crate::threads::is_archived(&cabin.threads[1]));
    assert_eq!(cabin.status, "Restored Done chat to chats");
    assert_eq!(
        crate::threads::chat_section_indices(&cabin.threads, Some(1), false),
        vec![0, 1]
    );
    quiet.settle();
}

#[test]
fn auto_archive_15_day_idle_skips_13_day_pinned_and_busy() {
    let mut quiet = QuietCabin::boot("auto-archive");
    let cabin = &mut quiet.cabin;
    let now = 30 * crate::threads::DAY_MS;
    cabin.cfg.chat_archive_days = 14;
    cabin.threads.clear();
    let mut idle15 = crate::threads::ChatThread::new("Fifteen", false);
    idle15.messages_mut().push(("user".into(), "hi".into()));
    idle15.accessed_ms = now - 15 * crate::threads::DAY_MS;
    let mut idle13 = crate::threads::ChatThread::new("Thirteen", false);
    idle13.messages_mut().push(("user".into(), "hi".into()));
    idle13.accessed_ms = now - 13 * crate::threads::DAY_MS;
    let mut pinned = crate::threads::ChatThread::new("Pinned", false);
    pinned.messages_mut().push(("user".into(), "hi".into()));
    pinned.accessed_ms = now - 20 * crate::threads::DAY_MS;
    pinned.pinned = true;
    let mut busy = crate::threads::ChatThread::new("Busy", false);
    busy.messages_mut().push(("user".into(), "hi".into()));
    busy.accessed_ms = now - 20 * crate::threads::DAY_MS;
    let busy_id = busy.id.clone();
    cabin.threads = vec![idle15, idle13, pinned, busy];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(busy_id);
    let n = cabin.run_auto_archive(now);
    assert_eq!(n, 1, "only the 15-day idle chat archives");
    assert!(crate::threads::is_archived(&cabin.threads[0]));
    assert!(!crate::threads::is_archived(&cabin.threads[1]));
    assert!(!crate::threads::is_archived(&cabin.threads[2]));
    assert!(!crate::threads::is_archived(&cabin.threads[3]));
    assert_eq!(cabin.status, "Archived 1 idle chat");
    quiet.settle();
}

#[test]
fn persist_keeps_threads_in_the_dir_it_was_scheduled_with() {
    let _g = crate::config::hold_test_config();
    let first = crate::config::test_config_root("persist-pin-a");
    let second = crate::config::test_config_root("persist-pin-b");
    let prev = std::env::var_os("GROKHUB_CONFIG");
    struct Restore {
        prev: Option<std::ffi::OsString>,
        roots: Vec<std::path::PathBuf>,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            restore_env("GROKHUB_CONFIG", self.prev.take());
            for root in &self.roots {
                let _ = std::fs::remove_dir_all(root);
            }
        }
    }
    let _restore = Restore {
        prev,
        roots: vec![first.clone(), second.clone()],
    };
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &first);
    let mut cabin = super::Cabin::quiet_for_test();
    let mut thread = crate::threads::ChatThread::new("PinnedLeak", false);
    thread.pinned = true;
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    let io = cabin.persist_io.clone();
    let hold = io.lock().unwrap_or_else(|e| e.into_inner());
    cabin.persist();
    std::env::set_var("GROKHUB_CONFIG", &second);
    drop(hold);
    let scheduled = first.join("threads.json");
    let moved = second.join("threads.json");
    let mut landed = String::new();
    for _ in 0..80 {
        if let Ok(body) = std::fs::read_to_string(&scheduled) {
            if body.contains("PinnedLeak") {
                landed = body;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let leaked = std::fs::read_to_string(&moved).unwrap_or_default();
    assert!(
        landed.contains("PinnedLeak"),
        "scheduled persist must write threads.json into the captured config dir; the later dir has {leaked}"
    );
    assert!(
        !leaked.contains("PinnedLeak"),
        "a later GROKHUB_CONFIG must not receive that threads.json: {leaked}"
    );
}

#[test]
fn apply_board_act_add_files_one_todo() {
    let mut quiet = QuietCabin::boot("board-add");
    let cabin = &mut quiet.cabin;
    assert!(cabin.board.is_empty());
    cabin.board_title = "Cover the dock".into();
    cabin.board_notes = "night shift".into();
    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Add)));
    assert_eq!(cabin.board.len(), 1);
    assert_eq!(cabin.board[0].title, "Cover the dock");
    assert_eq!(cabin.board[0].detail, "night shift");
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert_eq!(
        cabin.board[0].status.column(),
        Some(grokhub_core::KanbanColumn::Todo)
    );
    assert!(cabin.board_title.is_empty());
    quiet.settle();
}

#[test]
fn apply_plus_ready_image_and_pasted_text() {
    let mut quiet = QuietCabin::boot("plus-ready");
    let cabin = &mut quiet.cabin;
    cabin.apply_plus_ready(
        grokhub_core::PlusTarget::Chat,
        super::plus::PlusReady {
            kind: grokhub_core::AttachKind::Image,
            name: "harbor.png".into(),
            raw: "/tmp/harbor.png".into(),
            image_url: Some("data:image/png;base64,aGFyYm9y".into()),
            text: None,
        },
    );
    assert_eq!(cabin.attach_name.as_deref(), Some("harbor.png"));
    assert_eq!(
        cabin.attach_url.as_deref(),
        Some("data:image/png;base64,aGFyYm9y")
    );
    assert_eq!(
        cabin.status, "Attached harbor.png — sends with the next message",
        "an image attach must show the chip status"
    );
    assert!(!cabin.running);
    assert!(
        cabin.composer.is_empty(),
        "an image must not dump into the composer"
    );

    cabin.apply_plus_ready(
        grokhub_core::PlusTarget::Chat,
        super::plus::PlusReady {
            kind: grokhub_core::AttachKind::Text,
            name: "notes.txt".into(),
            raw: "notes.txt".into(),
            image_url: None,
            text: Some("pasted harbor line".into()),
        },
    );
    assert!(
        cabin.composer.is_empty(),
        "a file must stay attached, not pasted, got {}",
        cabin.composer
    );
    assert_eq!(cabin.attach_tray.len(), 2);
    assert_eq!(cabin.attach_name.as_deref(), Some("harbor.png"));
    assert_eq!(cabin.status, "File notes.txt — the agent will read it");
    assert!(!cabin.running);
    quiet.settle();
}

#[test]
fn run_slash_help_export_and_clear() {
    let mut quiet = QuietCabin::boot("slash-help-export-clear");
    let cabin = &mut quiet.cabin;
    cabin.run_slash(grokhub_core::Slash::Help);
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| m.0 == "assistant" && m.1.contains("/help — this list")),
        " /help must write the help list on the open chat, got {:?}",
        cabin.messages
    );
    assert!(!cabin.running);

    cabin.messages = std::sync::Arc::new(vec![("user".into(), "harbor export line".into())]);
    cabin.cfg.project_dir.clear();
    cabin.run_slash(grokhub_core::Slash::Export);
    let path = cabin
        .status
        .strip_prefix("Wrote ")
        .unwrap_or("")
        .to_string();
    assert!(
        path.ends_with("export.md"),
        " /export must name export.md, got {}",
        cabin.status
    );
    wait_for("export.md must contain the chat line", || {
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .contains("harbor export line")
    });

    cabin.run_slash(grokhub_core::Slash::Clear);
    assert!(
        cabin.messages.is_empty(),
        " /clear must empty the transcript, got {:?}",
        cabin.messages
    );
    assert_eq!(cabin.status, "Cleared");
    assert!(!cabin.running);
    quiet.settle();
}

#[test]
fn apply_board_act_move_archive_restore_and_link() {
    let mut quiet = QuietCabin::boot("board-acts");
    let cabin = &mut quiet.cabin;
    cabin.board_title = "Move the buoy".into();
    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Add)));
    let id = cabin.board[0].id.clone();
    let thread = cabin.threads[cabin.thread_idx].id.clone();

    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Move {
        id: id.clone(),
        status: grokhub_core::BoardStatus::InProgress,
    })));
    assert_eq!(
        cabin.board[0].status.column(),
        Some(grokhub_core::KanbanColumn::Doing)
    );

    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Archive(id.clone()))));
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Dismissed);
    assert!(cabin.board[0].status.column().is_none());

    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Restore(id.clone()))));
    assert_eq!(
        cabin.board[0].status.column(),
        Some(grokhub_core::KanbanColumn::Todo)
    );

    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Link(id.clone()))));
    assert_eq!(cabin.board[0].thread_id.as_deref(), Some(thread.as_str()));

    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Unlink(id))));
    assert!(cabin.board[0].thread_id.is_none());
    quiet.settle();
}

#[test]
fn add_automation_seed_daily_clock_and_loop() {
    let mut quiet = QuietCabin::boot("auto-seed");
    let root = quiet.boot.root.clone();
    let cabin = &mut quiet.cabin;
    cabin.add_automation_seed("every day at 9, summarize the board");
    assert_eq!(
        cabin.automations.len(),
        1,
        "a daily clock must land in automations"
    );
    assert!(
        cabin.grok_loops.is_empty(),
        "a clock time must not become a /loop"
    );
    assert!(
        cabin.status.contains("Automation added") && cabin.status.contains("09:00"),
        "the cabin must show the 09:00 automation, got {}",
        cabin.status
    );
    wait_for("automations.json must record the daily clock", || {
        std::fs::read_to_string(root.join("automations.json"))
            .unwrap_or_default()
            .contains("09")
    });

    cabin.add_automation_seed("/loop 30m check deploy");
    assert_eq!(cabin.grok_loops.len(), 1);
    assert_eq!(cabin.automations.len(), 1, "the clock row must stay");
    assert!(
        cabin.status.contains("Loop added") && cabin.status.contains("30m"),
        " /loop must show the interval, got {}",
        cabin.status
    );
    wait_for("loops.json must record the 30m loop", || {
        std::fs::read_to_string(root.join("loops.json"))
            .unwrap_or_default()
            .contains("30m")
    });
    quiet.settle();
}

#[test]
fn take_chip_act_nav_and_dismiss() {
    let mut quiet = QuietCabin::boot("chips");
    let cabin = &mut quiet.cabin;
    let nav = grokhub_core::QuickChip {
        id: "nav-board".into(),
        label: "Workboard".into(),
        value: "__nav:workboard".into(),
        kind: grokhub_core::ChipKind::Nav,
        score: 1.0,
        hint: String::new(),
        primary: false,
    };
    assert!(matches!(cabin.nav, super::Nav::Chat));
    cabin.take_chip_act(crate::cards::ChipRowAct::Apply(0), &[nav]);
    assert!(matches!(cabin.nav, super::Nav::Workboard));

    let dismiss = grokhub_core::QuickChip {
        id: "nav-ideas".into(),
        label: "Ideas".into(),
        value: "__nav:ideas".into(),
        kind: grokhub_core::ChipKind::Nav,
        score: 1.0,
        hint: String::new(),
        primary: false,
    };
    cabin.chip_busy = true;
    cabin.take_chip_act(crate::cards::ChipRowAct::Dismiss(0), &[dismiss]);
    assert!(cabin.chip_dismissed.iter().any(|d| d == "nav-ideas"));
    assert!(cabin.chip_dismissed.iter().any(|d| d == "__nav:ideas"));
    assert!(
        cabin.visible_chips.iter().all(|c| c.id != "nav-ideas"),
        "dismiss must drop that chip from the row, got {:?}",
        cabin
            .visible_chips
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>()
    );
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    quiet.settle();
}

#[test]
fn open_memory_file_flushes_the_editor_you_left() {
    let mut quiet = QuietCabin::boot("memory-file");
    let cabin = &mut quiet.cabin;
    assert_eq!(cabin.mem_name, "SOUL.md");
    cabin.mem_body = "typed soul line for the switch".into();
    cabin.open_memory_file("USER.md");
    assert_eq!(cabin.mem_name, "USER.md");
    assert!(
        cabin.mem_body.contains("Who you are"),
        "the next file must show in the editor, got {}",
        cabin.mem_body
    );
    assert!(
        !cabin.mem_body.contains("typed soul line"),
        "switching tabs must not wipe the unsaved line into the next file"
    );
    wait_for("leaving SOUL must flush the unsaved typing", || {
        crate::config::read_memory("SOUL.md").contains("typed soul line for the switch")
    });
    assert_eq!(cabin.mem_name, "USER.md");
    quiet.settle();
}

#[test]
fn set_session_mode_ask_and_chat() {
    let mut quiet = QuietCabin::boot("session-mode");
    quiet.settle();
    let app_json = quiet.boot.root.join("app.json");
    let cabin = &mut quiet.cabin;
    cabin.set_session_mode(grokhub_core::wire::SessionMode::Ask);
    assert_eq!(cabin.session_mode, grokhub_core::wire::SessionMode::Ask);
    assert_eq!(cabin.cfg.session_mode, "ask");
    wait_for("Ask must be saved", || {
        std::fs::read_to_string(&app_json)
            .unwrap_or_default()
            .contains("\"sessionMode\": \"ask\"")
    });

    cabin.set_session_mode(grokhub_core::wire::SessionMode::Chat);
    assert_eq!(cabin.session_mode, grokhub_core::wire::SessionMode::Chat);
    assert_eq!(cabin.cfg.session_mode, "chat");
    wait_for("Chat must be saved", || {
        std::fs::read_to_string(&app_json)
            .unwrap_or_default()
            .contains("\"sessionMode\": \"chat\"")
    });
    quiet.settle();
}

#[test]
fn delete_thread_at_removes_that_row() {
    let mut quiet = QuietCabin::boot("delete-thread");
    let cabin = &mut quiet.cabin;
    let keep = cabin.threads[cabin.thread_idx].title.clone();
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Harbor", false));
    let idx = cabin.threads.len() - 1;
    cabin.threads[idx].messages = std::sync::Arc::new(vec![("user".into(), "harbor row".into())]);
    cabin.delete_thread_at(idx);
    assert!(
        cabin.threads.iter().all(|t| t.title != "Harbor"),
        "deleting that chat must remove its sidebar row, left {:?}",
        cabin
            .threads
            .iter()
            .map(|t| t.title.as_str())
            .collect::<Vec<_>>()
    );
    assert!(cabin.threads.iter().any(|t| t.title == keep));
    assert_eq!(cabin.status, "Deleted Harbor");
    quiet.settle();
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

#[test]
fn make_folder_saves_harbor_notes() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("make-folder");
    let mut cabin = Cabin::quiet_for_test();
    cabin.make_folder("Harbor notes");
    let saved = wait_file_has(&crate::store::projects_path(), "Harbor notes");
    join_persist(&cabin.persist_svc);
    let folders: Vec<_> = cabin
        .projects
        .iter()
        .filter(|n| n.kind == ProjectKind::Folder && n.name == "Harbor notes")
        .collect();
    assert_eq!(folders.len(), 1, "one folder row named Harbor notes");
    assert!(
        cabin.status.starts_with("Folder "),
        "status {status}",
        status = cabin.status
    );
    assert!(
        saved.contains("Harbor notes"),
        "projects file must contain the folder name"
    );
    drop(cfg);
}

#[test]
fn delete_all_history_clears_seeded_chats() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("delete-all");
    let mut cabin = Cabin::quiet_for_test();
    let mut pier = crate::threads::ChatThread::new("Pier light", false);
    pier.messages_mut()
        .push(("user".into(), "bring the lamp".into()));
    let mut salt = crate::threads::ChatThread::new("Salt lane", false);
    salt.messages_mut()
        .push(("user".into(), "walk the lane".into()));
    cabin.threads = vec![pier, salt];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.delete_all_history();
    wait_tree_contains(&cfg.root, "Chat");
    join_persist(&cabin.persist_svc);
    let saved = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(
        !saved.contains("Pier light") && !saved.contains("Salt lane"),
        "the pre-delete snapshot from halt_in_flight must not land last: {saved}"
    );
    assert!(
        cabin.threads.iter().all(|t| t.title != "Pier light"),
        "Pier light must be gone"
    );
    assert!(
        cabin.threads.iter().all(|t| t.title != "Salt lane"),
        "Salt lane must be gone"
    );
    assert_eq!(cabin.status, "Deleted all chats");
    assert_eq!(cabin.threads.len(), 1, "the fresh chat is the only row");
    assert_eq!(cabin.threads[0].title, "Chat");
    assert!(
        cabin.threads[0].messages.is_empty(),
        "the fresh chat has no transcript"
    );
    drop(cfg);
}

#[test]
fn poisoned_persist_io_surfaces_error_and_retries_idle() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-poison");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Pier", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    // Poison the shared mutex the PersistService worker takes before disk I/O.
    let io = cabin.persist_io.clone();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _g = io.lock().unwrap();
        panic!("poison persist_io");
    }));
    cabin.persist_idle_key.clear();
    cabin.persist_bg();
    let start = std::time::Instant::now();
    while cabin.persist_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(3) {
        cabin.poll_persist();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cabin.poll_persist_err();
    assert!(
        cabin.status.contains("persist lock poisoned") || cabin.status.contains("Could not save"),
        "poisoned lock must surface via persist_err, got status={status}",
        status = cabin.status
    );
    assert!(
        cabin.persist_idle_key.is_empty(),
        "failed write must clear the idle key so the next tick retries"
    );
    drop(cfg);
}

#[test]
fn persist_worker_survives_a_panicking_job_then_writes() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-panic-survive");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Panic pier", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "before panic".into())]);
    // First job panics inside the worker; catch_unwind must keep the thread up.
    super::persist_panic_next();
    cabin.persist();
    cabin.persist_svc.flush();
    cabin.poll_persist_err();
    assert!(
        cabin.status.contains("persist worker panicked") || cabin.status.contains("Could not save"),
        "panicked job must surface via persist_err, got status={status}",
        status = cabin.status
    );
    // Second job must still land on disk through the same worker.
    cabin.status.clear();
    cabin.persist_idle_key.clear();
    if let Ok(mut slot) = cabin.persist_err.lock() {
        *slot = None;
    }
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "after panic".into())]);
    cabin.persist();
    cabin.persist_svc.flush();
    cabin.poll_persist_err();
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(
        threads.contains("after panic") || threads.contains("Panic pier"),
        "worker must still write after a panicked job: {threads}"
    );
    assert!(
        !cabin.status.contains("persist worker panicked"),
        "successful write must not leave the panic status: {status}",
        status = cabin.status
    );
    drop(cfg);
}

#[test]
fn persist_writes_directly_when_the_worker_is_gone() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-dead-worker");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Dead worker dock", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "no worker".into())]);
    cabin.persist_svc = super::dead_persist_service(
        cabin.persist_io.clone(),
        std::sync::Arc::new(std::sync::Mutex::new(PersistMark::default())),
    );
    cabin.persist();
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(threads.contains("Dead worker dock"), "{threads}");
    cabin.poll_persist_err();
    assert!(
        !cabin.status.contains("Could not save"),
        "a successful fallback write must not report failure: {status}",
        status = cabin.status
    );
    // A panic inside the fallback is caught and reported, not propagated.
    super::persist_panic_next();
    cabin.persist();
    cabin.poll_persist_err();
    assert!(
        cabin
            .status
            .starts_with("Could not save chats: persist worker panicked"),
        "got status={status}",
        status = cabin.status
    );
    drop(cfg);
}

#[test]
fn an_older_persist_snapshot_never_overwrites_a_newer_one() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-order");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Pier light", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    cabin.projects_dirty = true;
    let mut old = cabin.persist_snap();
    old.secrets = Some(cabin.secrets.clone());
    let old_gen = cabin.next_persist_gen();
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.projects_dirty = false;
    let new = cabin.persist_snap();
    let new_gen = cabin.next_persist_gen();
    assert_eq!((old_gen, new_gen), (1, 2));
    assert!(old.projects.is_some() && new.projects.is_none());
    assert!(new.secrets.is_none());
    let mark = std::sync::Arc::new(std::sync::Mutex::new(PersistMark::default()));
    // The newer worker takes persist_io first, the older one second.
    let _ = write_persist_disk_in_order(&cfg.root, &new, new_gen, &mark);
    let _ = write_persist_disk_in_order(&cfg.root, &old, old_gen, &mark);
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(threads.contains("\"title\": \"Chat\""), "{threads}");
    assert!(!threads.contains("Pier light"), "{threads}");
    // Only the older snapshot carried projects and secrets, so those still land.
    assert!(crate::store::projects_path().is_file());
    assert!(cfg.root.join("secrets.json").is_file());
    // A second older write of the same generation changes nothing.
    std::fs::remove_file(crate::store::projects_path()).expect("projects");
    let _ = write_persist_disk_in_order(&cfg.root, &old, old_gen, &mark);
    assert!(!crate::store::projects_path().exists());
    let again = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert_eq!(again, threads);
    drop(cfg);
}

#[test]
fn persist_service_queues_out_of_order_full_snaps() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("persist-svc-order");
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads = vec![crate::threads::ChatThread::new("Pier light", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    cabin.projects_dirty = true;
    let mut old = cabin.persist_snap();
    old.secrets = Some(cabin.secrets.clone());
    let old_gen = cabin.next_persist_gen();
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.projects_dirty = false;
    let new = cabin.persist_snap();
    let new_gen = cabin.next_persist_gen();
    assert_eq!((old_gen, new_gen), (1, 2));
    // Newer gen first, then older — through the actor, not the free fn.
    cabin.persist_svc.submit_full(
        cfg.root.clone(),
        new,
        new_gen,
        None,
        cabin.persist_err.clone(),
    );
    cabin.persist_svc.submit_full(
        cfg.root.clone(),
        old,
        old_gen,
        None,
        cabin.persist_err.clone(),
    );
    join_persist(&cabin.persist_svc);
    let threads = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(threads.contains("\"title\": \"Chat\""), "{threads}");
    assert!(!threads.contains("Pier light"), "{threads}");
    assert!(crate::store::projects_path().is_file());
    assert!(cfg.root.join("secrets.json").is_file());
    drop(cfg);
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

// Landed from PR #94.
#[test]
fn rename_thread_sets_locked_title() {
    let _g = config::hold_test_config();
    let root = config::test_config_root("rename-thread");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads.push(ChatThread::new("Chat", false));
    cabin.rename_thread(0, "Harbor watch");
    assert_eq!(cabin.threads[0].title, "Harbor watch");
    assert!(cabin.threads[0].title_locked);
    assert_eq!(cabin.status, "Renamed Harbor watch");
    let _io = cabin.persist_io.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #95.
#[test]
fn command_palette_opens_and_navigates() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.open_palette();
    assert!(cabin.palette_open);
    assert!(cabin.palette_q.is_empty());
    cabin.run_palette(&egui::Context::default(), "nav:board");
    assert!(matches!(cabin.nav, Nav::Workboard));
    assert!(!cabin.palette_open);
    cabin.open_palette();
    cabin.run_palette(&egui::Context::default(), "nav:history");
    assert!(matches!(cabin.nav, Nav::History));
}

// Landed from PR #97.
#[test]
fn remove_project_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("remove-project");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.remove_project_id("missing");
    assert_eq!(cabin.status, "Project not found");
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    cabin.projects = vec![
        ProjectNode {
            id: "f1".into(),
            name: "Notes".into(),
            kind: ProjectKind::Folder,
            path: String::new(),
            parent: None,
            open: true,
        },
        ProjectNode {
            id: "p1".into(),
            name: "Harbor".into(),
            kind: ProjectKind::Project,
            path: String::new(),
            parent: None,
            open: true,
        },
    ];
    cabin.project_sel = Some("p1".into());
    assert!(cabin.cfg.project_dir.is_empty());

    cabin.remove_project_id("p1");
    assert_eq!(cabin.status, "Removed Harbor");
    assert_eq!(cabin.projects.len(), 1);
    assert_eq!(cabin.projects[0].id, "f1");
    assert_eq!(cabin.projects[0].name, "Notes");
    assert!(matches!(cabin.projects[0].kind, ProjectKind::Folder));
    assert!(cabin.project_sel.is_none());
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #98.
#[test]
fn rename_folder_and_move_project_into_folder() {
    let mut app = Cabin::quiet_for_test();
    app.make_folder("Harbor notes");
    let harbor_id = app
        .projects
        .iter()
        .find(|n| n.name == "Harbor notes")
        .expect("Harbor notes")
        .id
        .clone();
    app.begin_proj_rename(harbor_id.clone(), "Dock notes".to_string());
    app.finish_proj_rename();
    let renamed = app
        .projects
        .iter()
        .find(|n| n.id == harbor_id)
        .expect("renamed folder");
    assert_eq!(renamed.name, "Dock notes");
    assert_eq!(app.status, "Renamed Dock notes");

    app.move_sel_to_folder_name("Lab");
    assert_eq!(app.status, "Select a project first");

    app.make_folder("Lab");
    app.make_project("Pier", None);
    let lab_id = app
        .projects
        .iter()
        .find(|n| n.name == "Lab")
        .expect("Lab")
        .id
        .clone();
    let pier_id = app
        .projects
        .iter()
        .find(|n| n.name == "Pier")
        .expect("Pier")
        .id
        .clone();
    app.project_sel = Some(pier_id.clone());
    app.move_sel_to_folder_name("Lab");
    assert_eq!(app.status, "Added to Lab");
    let lab_open = app
        .projects
        .iter()
        .find(|n| n.id == lab_id)
        .expect("Lab")
        .open;
    assert!(lab_open);
    let pier_parent = app
        .projects
        .iter()
        .find(|n| n.id == pier_id)
        .expect("Pier")
        .parent
        .clone();
    assert_eq!(pier_parent.as_deref(), Some(lab_id.as_str()));
}

// Landed from PR #99.
#[test]
fn clear_chat_attach_clears_name_url_and_status() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.attach_name = Some("harbor.png".into());
    cabin.attach_url = Some("file://harbor.png".into());
    cabin.status = "Attached harbor.png".into();
    cabin.clear_chat_attach();
    assert!(cabin.attach_name.is_none());
    assert!(cabin.attach_url.is_none());
    assert!(cabin.status.is_empty());
}

// Landed from PR #100.
#[test]
fn sign_out_oauth_clears_the_session() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("sign-out");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.imagine_pending = true;
    cabin.sign_out_oauth();
    assert_eq!(cabin.status, "Signed out");
    assert!(cabin.secrets.oauth.is_none());
    assert!(!cabin.imagine_pending);
    assert!(cabin.oauth_pending.is_none());
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #101.
#[test]
fn new_thread_opens_another_chat_when_the_current_one_has_a_message() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("new-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin
            .threads
            .push(crate::threads::ChatThread::new("Harbor", false));
        cabin.thread_idx = 0;
    }
    let line = std::sync::Arc::new(vec![("user".into(), "paint the harbor".into())]);
    for t in &mut cabin.threads {
        t.messages = line.clone();
    }
    cabin.messages = line;
    let before = cabin.threads.len();
    cabin.new_thread(false);
    assert_eq!(cabin.threads.len(), before + 1);
    assert_eq!(cabin.status, "New chat");
    assert!(cabin.messages.is_empty());
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #102.
#[test]
fn open_history_hit_opens_pier_and_a_memory_file() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("history-open");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads.clear();
    let mut harbor = crate::threads::ChatThread::new("Harbor", false);
    let mut pier = crate::threads::ChatThread::new("Pier", false);
    let line = std::sync::Arc::new(vec![("user".into(), "paint the harbor".into())]);
    harbor.messages = line.clone();
    pier.messages = line.clone();
    let pier_id = pier.id.clone();
    cabin.threads.push(harbor);
    cabin.threads.push(pier);
    cabin.thread_idx = 0;
    cabin.messages = line;
    cabin.open_history_hit(&format!("thread:{pier_id}"));
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Pier");
    assert!(matches!(cabin.nav, Nav::Chat));
    cabin.open_history_hit("thread:missing");
    assert_eq!(cabin.status, "That chat is gone");
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Pier");
    cabin.open_history_hit("mem:USER.md");
    assert!(matches!(cabin.nav, Nav::Memory));
    assert_eq!(cabin.status, "USER.md");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #103.
#[test]
fn set_permission_mode_saves_auto_and_always() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("permission-mode");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.set_permission_mode(PermissionMode::Auto);
    assert_eq!(cabin.permission_mode, PermissionMode::Auto);
    assert_eq!(cabin.cfg.permission_mode, "auto");
    cabin.set_permission_mode(PermissionMode::AlwaysApprove);
    assert_eq!(cabin.permission_mode, PermissionMode::AlwaysApprove);
    assert_eq!(cabin.cfg.permission_mode, "ask");
    cabin.set_permission_mode(PermissionMode::Ask);
    assert_eq!(cabin.permission_mode, PermissionMode::Ask);
    assert_eq!(cabin.cfg.permission_mode, "ask");
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn halt_work_stops_a_running_turn() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("halt-work");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.running = true;
    cabin.imagine_pending = true;
    cabin.halt_work("Stopped");
    assert!(!cabin.running);
    assert!(!cabin.imagine_pending);
    assert_eq!(cabin.status, "Stopped");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #104.
#[test]
fn save_settings_stores_quiet_hours_and_clears_the_key() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("save-settings");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.api_key = "secret-key".into();
    cabin.quiet_start_buf = "22:00".into();
    cabin.quiet_end_buf = "07:00".into();
    cabin.cap_auto_buf = "12".into();
    cabin.cap_host_buf = "4".into();
    cabin.save_settings();
    assert_eq!(cabin.status, "Saved");
    assert!(cabin.cfg.api_key.is_empty());
    assert_eq!(cabin.cfg.quiet_start, "22:00");
    assert_eq!(cabin.cfg.quiet_end, "07:00");
    assert_eq!(cabin.cfg.daily_auto_cap, 12);
    assert_eq!(cabin.cfg.host_hour_cap, 4);
    std::env::remove_var("GROKHUB_CONFIG");
}

/// Router R1: `/effort <level>` changes nothing and says effort is automatic.
#[test]
fn effort_slash_leaves_the_config_byte_identical_and_says_effort_is_automatic() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("effort-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    crate::config::save(&cabin.cfg).expect("save");
    let app_json = crate::config::config_dir().join("app.json");
    let before = std::fs::read(&app_json).expect("app.json");
    for line in ["/effort high", "/effort xhigh", "/effort banana", "/effort"] {
        cabin.status.clear();
        cabin.run_slash_line(line);
        assert_eq!(cabin.status, "Effort is automatic now, see /why", "{line}");
        assert_eq!(
            std::fs::read(&app_json).expect("app.json"),
            before,
            "{line} must not touch app.json"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

/// Router R1: there is no effort control in the composer, Settings or the slash list.
#[test]
fn no_effort_selector_anywhere() {
    // The composer row returns a mode, a permission, and the Auto chip's /why click. Nothing else.
    let crate::cards::SessionRowOut {
        mode: _,
        perm: _,
        why: _,
    } = crate::cards::SessionRowOut {
        mode: None,
        perm: None,
        why: false,
    };
    for (id, label) in crate::cards::composer_modes()
        .iter()
        .chain(crate::cards::permission_modes())
    {
        assert!(
            !id.contains("effort")
                && !grokhub_core::REASONING_EFFORTS
                    .iter()
                    .any(|(_, l)| l == label),
            "{id}"
        );
    }
    assert!(super::settings::DEFAULTS_ROWS
        .iter()
        .all(|r| !r.to_ascii_lowercase().contains("effort")));
    assert!(grokhub_core::SLASH_COMMANDS
        .iter()
        .all(|d| d.cmd != "/effort"));
    let help = grokhub_core::slash_help();
    assert!(help.contains("/effort — effort is automatic now"), "{help}");
    assert!(!help.contains("/effort <"), "{help}");
    let job = crate::cards::auto_chip_job("Medium");
    assert_eq!(job.text, "Auto · Medium");
}

// Landed from PR #106. `/health` is one chat block since card 10.
#[test]
fn health_slash_posts_updates_failed_services_and_the_last_dream_in_one_block() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("health-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/health");
    let empty = last_chat_text(&cabin);
    assert!(
        empty.contains("Health\n\nUpdates: none pending.\nServices: ")
            && empty.ends_with("\nLast dream: none yet. GrokHub dreams once a night after the review, in memory repo mode."),
        "{empty}"
    );
    assert!(
        cabin.status.starts_with("Health: no updates · "),
        "{}",
        cabin.status
    );

    let dreams = root.join("amr").join("dreams");
    std::fs::create_dir_all(&dreams).unwrap();
    std::fs::write(
        dreams.join("2026-10-08.md"),
        "# Memory dream 2026-10-08\n\n## Merged\n\n- Kept `n1` \"Uses pnpm\", merged `n2` \"uses pnpm\" because 80% shared words.\n\n## Retired\n\n- Retired `n9` \"old token path\" because confidence 0.2.\n",
    )
    .unwrap();
    cabin.cabin_latest = Some("v99.0.0".into());
    let failing = grokhub_core::mark_automation_failed(health_job("a1"), "disk full");
    cabin.automations = vec![failing];
    cabin.run_slash_line("/health");
    let block = last_chat_text(&cabin);
    assert!(
        block.contains("Health\n\nUpdates: GrokHub 99.0.0 is ready. Run /update.\nServices: "),
        "{block}"
    );
    assert!(
        block.contains("\n- Automation Board summary: Last run failed — disk full\n"),
        "{block}"
    );
    assert!(
        block.ends_with("\nLast dream: 2026-10-08, 1 merged, 1 retired."),
        "{block}"
    );
    assert!(
        cabin.status.starts_with("Health: 1 update · "),
        "{}",
        cabin.status
    );
    assert!(
        !cabin.status.ends_with("services ok"),
        "a failed automation is a failed service: {}",
        cabin.status
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #107.
#[test]
fn undo_retry_and_context_on_an_empty_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("undo-retry");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.messages = std::sync::Arc::new(Vec::new());
    if let Some(t) = cabin.threads.get_mut(cabin.thread_idx) {
        t.messages = cabin.messages.clone();
    }
    cabin.run_slash_line("/undo");
    assert_eq!(cabin.status, "Nothing to undo");
    assert!(!cabin.running);
    cabin.run_slash_line("/retry");
    assert_eq!(cabin.status, "Nothing to retry");
    assert!(!cabin.running);
    cabin.run_slash_line("/context");
    assert_eq!(cabin.status, "0 turns · 0 tokens · 0% · pin none");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #108.
#[test]
fn mode_slash_sets_think_and_auto() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("mode-slash");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.model.clear();
    cabin.run_slash_line("/mode think");
    assert_eq!(cabin.cfg.mode, "think");
    assert_eq!(cabin.status, "Mode think → grok-4.7 · high");
    cabin.cfg.model.clear();
    cabin.run_slash_line("/mode auto");
    assert_eq!(cabin.cfg.mode, "auto");
    assert_eq!(
        cabin.status,
        "Mode auto — routes Fast / Balance / Think / Max"
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #109.
#[test]
fn usage_and_host_slashes_set_status_without_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("usage-host");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/usage");
    assert!(cabin.status.starts_with("today "));
    assert!(cabin.status.contains(" · chat "));
    assert!(cabin.status.contains(" · imagine "));
    assert!(!cabin.running);
    cabin.run_slash_line("/host");
    assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #110.
#[test]
fn board_slash_opens_the_workboard_and_scratch_blocks_memory() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("board-scratch");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.board.clear();
    cabin.run_slash_line("/board");
    assert!(matches!(cabin.nav, Nav::Workboard));
    assert_eq!(cabin.status, "0 cards");
    cabin.threads = vec![crate::threads::ChatThread::new("Scratch", true)];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.run_slash_line("/remember harbor note");
    assert_eq!(cabin.status, "Scratch — no memory writes");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #112.
#[test]
fn fix_opens_about_and_dream_refuses_without_login() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("fix-dream");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("HOME", &root);
    std::env::set_var("GROKHUB_GROK", "/tmp/grokhub-no-such-grok");
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.api_key.clear();
    cabin.running = true;
    cabin.imagine_pending = true;
    cabin.run_slash_line("/fix");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(matches!(cabin.settings_sec, SettingsSec::About));
    assert!(!cabin.running, "fix must halt a live job");
    assert!(!cabin.imagine_pending, "fix must clear a pending imagine");
    assert_eq!(cabin.status, cabin.doctor_text());
    assert!(
        !cabin.llm_ready(),
        "this test must have no key and no grok binary"
    );
    cabin.run_slash_line("/dream");
    assert_eq!(cabin.status, "Sign in with Grok or add an API key.");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.imagine_want_focus);
    std::env::remove_var("GROKHUB_CONFIG");
    std::env::remove_var("GROKHUB_GROK");
}

// Landed from PR #113.
#[test]
fn remember_appends_memory_and_refuses_a_secret() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("remember-write");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body.clear();
    cabin.run_slash_line("/remember harbor light");
    assert_eq!(cabin.status, "Wrote MEMORY.md");
    assert!(
        cabin.mem_body.contains("harbor light"),
        "memory body was {}",
        cabin.mem_body
    );
    let before = cabin.mem_body.clone();
    cabin.run_slash_line("/remember sk-abcdefghijklmnopqrst");
    assert_eq!(cabin.status, "Secrets never in markdown");
    assert_eq!(cabin.mem_body, before);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #114.
#[test]
fn memory_goal_model_loop_forget_and_empty_imagine() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("memory-goal");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = "harbor light\nkeep the dock\n".into();
    cabin.run_slash_line("/memory");
    assert!(matches!(cabin.nav, Nav::Memory));
    assert_eq!(cabin.status, "Memory");
    cabin.run_slash_line("/goal migrate auth");
    assert_eq!(cabin.cfg.goal_pin, "migrate auth");
    assert_eq!(cabin.status, "Goal: migrate auth");
    cabin.run_slash_line("/goal");
    assert_eq!(cabin.status, "Goal: migrate auth");
    cabin.run_slash_line("/goal clear");
    assert!(cabin.cfg.goal_pin.is_empty());
    assert_eq!(cabin.status, "Goal cleared");
    cabin.run_slash_line("/model grok-4.7");
    assert_eq!(cabin.cfg.model, "grok-4.7");
    assert_eq!(cabin.status, "grok --model grok-4.7");
    cabin.run_slash_line("/loop");
    assert!(matches!(cabin.nav, Nav::Night));
    assert!(cabin.auto_compose);
    cabin.run_slash_line("/forget harbor");
    assert_eq!(cabin.status, "Forgot harbor");
    assert!(!cabin.mem_body.to_ascii_lowercase().contains("harbor"));
    assert!(cabin.mem_body.contains("keep the dock"));
    cabin.run_slash_line("/forget");
    assert_eq!(cabin.status, "Forgot MEMORY.md");
    assert!(cabin.mem_body.is_empty());
    cabin.imagine_prompt.clear();
    cabin.run_slash_line("/imagine");
    assert!(matches!(cabin.nav, Nav::Imagine));
    assert!(cabin.imagine_want_focus);
    assert!(!cabin.running);
    assert!(!cabin.imagine_pending);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #115.
#[test]
fn skill_hub_help_and_clear() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("skill-hub");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.hub_on = false;
    cabin.skill_list.clear();
    cabin.run_slash_line("/skill missing-harbor");
    assert_eq!(cabin.status, "No skill missing-harbor");
    assert!(matches!(cabin.nav, Nav::Chat), "{:?}", cabin.nav);
    cabin.run_slash_line("/hub");
    assert!(matches!(cabin.nav, Nav::Devices));
    assert_eq!(cabin.status, "Start share on Devices");
    assert!(!cabin.hub_on);
    cabin.run_slash_line("/help");
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| m.0 == "assistant" && m.1.contains("/clear")),
        "help did not land on the transcript"
    );
    cabin.running = true;
    cabin.imagine_pending = true;
    cabin.live_mut().push(("user".into(), "harbor".into()));
    cabin.run_slash_line("/clear");
    assert!(!cabin.running, "clear must halt a live job");
    assert!(!cabin.imagine_pending);
    assert!(cabin.messages.is_empty());
    assert_eq!(cabin.status, "Cleared");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #116.
#[test]
fn rename_pin_and_delete_the_open_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("rename-pin");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/scratch");
    assert_eq!(cabin.threads.len(), 1);
    cabin.run_slash_line("/rename Harbor");
    assert_eq!(cabin.status, "Renamed Harbor");
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Harbor");
    assert!(cabin.threads[cabin.thread_idx].title_locked);
    cabin.run_slash_line("/pin");
    assert_eq!(cabin.status, "Pinned Harbor");
    assert!(cabin.threads[cabin.thread_idx].pinned);
    cabin.run_slash_line("/pin");
    assert_eq!(cabin.status, "Unpinned Harbor");
    assert!(!cabin.threads[cabin.thread_idx].pinned);
    cabin.run_slash_line("/delete");
    assert_eq!(cabin.status, "Chat deleted");
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].title, "Chat");
    assert!(!cabin.threads[0].scratch);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #117.
#[test]
fn export_writes_a_chat_and_empty_video_does_not_send() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("export-video");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/scratch");
    cabin.cfg.project_dir.clear();
    cabin.run_slash_line("/export");
    assert!(
        cabin.status.starts_with("Wrote ") && cabin.status.ends_with("export.md"),
        "export status was {}",
        cabin.status
    );
    cabin.imagine_prompt.clear();
    cabin.run_slash_line("/imagine-video");
    assert!(matches!(cabin.nav, Nav::Imagine));
    assert!(matches!(
        cabin.imagine_kind,
        grokhub_core::ImagineKind::Video
    ));
    assert!(cabin.imagine_want_focus);
    assert!(cabin.imagine_prompt.is_empty());
    assert!(!cabin.running);
    assert!(!cabin.imagine_pending);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #118.
#[test]
fn voice_without_login_stays_off_and_profile_clears() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("voice-profile");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let old_path = std::env::var_os("PATH");
    std::env::set_var("PATH", "");
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.api_key.clear();
    cabin.listen_voice();
    assert_eq!(cabin.status, "Connect Grok OAuth for STT/TTS.");
    assert!(!cabin.running, "voice must not start a listen job");
    cabin.cfg.profile_picture = "harbor.png".into();
    cabin.clear_profile_picture();
    assert!(cabin.cfg.profile_picture.is_empty());
    assert_eq!(cabin.status, "Saved");
    match old_path {
        Some(p) => std::env::set_var("PATH", p),
        None => std::env::remove_var("PATH"),
    }
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #119.
#[test]
fn scratch_btw_worktree_and_plan() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("scratch-btw");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.running = false;
    cabin.run_slash_line("/view-plan");
    assert_eq!(cabin.status, "No plan yet — use Plan mode");
    assert!(!cabin.plan_open);
    cabin.run_slash_line("/btw");
    assert!(matches!(cabin.session_mode, SessionMode::Ask));
    assert_eq!(cabin.cfg.session_mode, "ask");
    assert_eq!(cabin.status, "btw — look-safe side ask");
    cabin.run_slash_line("/scratch");
    assert_eq!(cabin.status, "Scratch — no memory writes");
    assert!(cabin.scratch());
    assert!(cabin.composer_want_focus);
    cabin.run_slash_line("/worktree");
    assert_eq!(
        cabin.status,
        "Worktrees were a Grok Build CLI feature; chats run in the bound project folder"
    );
    cabin.threads[cabin.thread_idx].grok_session = Some("sess-harbor".into());
    let (count, idx) = (cabin.threads.len(), cabin.thread_idx);
    cabin.run_slash_line("/fork");
    assert_eq!(cabin.threads.len(), count, "/fork no longer opens a chat");
    assert_eq!(cabin.thread_idx, idx);
    assert!(!cabin.threads[idx].grok_fork);
    assert_eq!(
        cabin.status,
        "Worktrees were a Grok Build CLI feature; chats run in the bound project folder",
        "/fork leaves the status alone"
    );
    cabin.threads[cabin.thread_idx].plan_body = "harbor steps".into();
    cabin.run_slash_line("/view-plan");
    assert!(cabin.plan_open);
    assert_eq!(cabin.status, "View plan");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #120.
#[test]
fn import_without_openclaw_and_consult_without_login() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("import-consult");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("HOME", &root);
    std::env::set_var("GROKHUB_GROK", "/tmp/grokhub-no-such-grok");
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.api_key.clear();
    cabin.run_slash_line("/import");
    assert_eq!(cabin.status, "Importing OpenClaw…");
    let start = std::time::Instant::now();
    while cabin.status == "Importing OpenClaw…"
        && start.elapsed() < std::time::Duration::from_secs(2)
    {
        cabin.poll_import_openclaw();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        cabin.status,
        "No OpenClaw workspace (~/.openclaw/workspace)"
    );
    assert!(!matches!(cabin.nav, Nav::Memory));
    assert!(
        !cabin.llm_ready(),
        "consult must have no key and no grok binary"
    );
    cabin.run_slash_line("/consult harbor");
    assert_eq!(cabin.status, "Sign in with Grok or add an API key.");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
    std::env::remove_var("GROKHUB_GROK");
}

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
        "I prefer the harbor light on at night".into(),
    ));
    cabin.run_reflect();
    wait_reflect(&mut cabin);
    assert_eq!(cabin.status, "Reflected MEMORY.md");
    assert_eq!(cabin.mem_body, "I prefer the harbor light on at night\n");
    cabin.run_slash(grokhub_core::Slash::MemoryNote(
        "the quay lantern is green".into(),
    ));
    assert_eq!(cabin.status, "Wrote MEMORY.md");
    cabin.run_slash(grokhub_core::Slash::Forget(Some("quay".into())));
    assert_eq!(cabin.status, "Forgot quay");
    cabin.run_slash_line("/recall harbor");
    let body = wait_recall(&mut cabin);
    assert!(
        body.contains("MEMORY.md:1: I prefer the harbor light on at night"),
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

fn last_chat_text(cabin: &Cabin) -> String {
    cabin
        .messages
        .last()
        .map(|m| m.1.clone())
        .unwrap_or_default()
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

// Landed from PR #122.
#[test]
fn skills_connectors_and_sessions_without_grok() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("skills-sessions");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", "/tmp/grokhub-no-such-grok");
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/skills");
    assert!(matches!(cabin.nav, Nav::Skills));
    cabin.run_slash_line("/connectors");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    cabin.run_slash_line("/dashboard");
    assert!(matches!(cabin.nav, Nav::History));
    assert_eq!(cabin.status, "History");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
    std::env::remove_var("GROKHUB_GROK");
}

// Landed from PR #123.
#[test]
fn sync_writes_a_local_hub_snapshot() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("sync-local");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    // Spike-4a: /sync publishes chats and memory, so it needs the hub grant
    // (the Settings → Permissions Allow click) or a hard-card click.
    let grant = grokhub_agent::harness::grant_destination(
        &root,
        grokhub_agent::harness::HUB_DEST,
        grokhub_agent::harness::HUB_SYNC_DATA,
        grokhub_agent::harness::UserClick::from_click(),
    )
    .unwrap();
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.device_name = "harbor".into();
    // SY-03: /sync only sends when a computer is paired.
    pair_test_peer(&cabin, "p1");
    cabin.run_slash_line("/sync");
    assert_eq!(cabin.status, "Syncing…");
    assert!(cabin.sync_rx.is_some());
    cabin.run_slash_line("/sync");
    assert_eq!(cabin.status, "Syncing…");
    // Windows ACL + antivirus can make the sync worker's first secrets/app.json
    // writes take longer than a tight 2s poll. Wait for the channel, not a wall clock.
    let start = std::time::Instant::now();
    let budget = std::time::Duration::from_secs(8);
    while cabin.sync_rx.is_some() && start.elapsed() < budget {
        cabin.poll_sync();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        cabin.sync_rx.is_none(),
        " /sync must land the hub snapshot; status={:?} after {:?}",
        cabin.status,
        start.elapsed()
    );
    // SY-03: one result line in the chat; the pane stays on Chat.
    assert!(matches!(cabin.nav, Nav::Chat), "nav={:?}", cabin.nav);
    assert_eq!(cabin.status, "Synced chats and memory to 1 computer.");
    let (role, body) = cabin.messages.last().cloned().expect("result line");
    assert_eq!(role, "assistant");
    assert!(
        grokhub_core::is_cabin_slash_turn(&role, &body),
        "stays out of the next model kick"
    );
    assert_eq!(
        grokhub_core::strip_slash_result(&body),
        "Synced chats and memory to 1 computer."
    );
    assert!(cabin.hub.lock().unwrap().snapshot.is_some());
    let log = grokhub_agent::harness::read_egress(&root);
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].dest, "hub");
    assert_eq!(log[0].grant_id, grant.id);
    assert_eq!(log[0].basis, "grant");
    let spans = harness_spans(&root);
    let sent = spans
        .iter()
        .find(|s| s.tool == "hub_sync")
        .expect("hub_sync span");
    assert_eq!(sent.decision, "allow");
    assert_eq!(sent.consent_ref, grant.id);
    assert_eq!(sent.path, "egress");
    assert_eq!(log[0].span_id, sent.span_ref());
    std::env::remove_var("GROKHUB_CONFIG");
}

/// Every span line under `{root}/spans/`, any session.
fn harness_spans(root: &std::path::Path) -> Vec<grokhub_agent::harness::Span> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root.join("spans"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            out.push(serde_json::from_str(line).expect("span line"));
        }
    }
    out.sort_by_key(|s: &grokhub_agent::harness::Span| s.ts_ms);
    out
}

/// Pair a fake computer with the cabin's hub (no network: just the peer row).
fn pair_test_peer(cabin: &Cabin, id: &str) {
    cabin
        .hub
        .lock()
        .unwrap()
        .peers
        .push(grokhub_core::state::Peer {
            id: id.into(),
            name: format!("box-{id}"),
            token: format!("t-{id}"),
            last_seen: 0,
        });
}

fn wait_sync(cabin: &mut Cabin) {
    let start = std::time::Instant::now();
    while cabin.sync_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(8) {
        cabin.poll_sync();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

// Spike-4a: chats and memory to a not-yet-granted destination are hard Send.
#[test]
fn sync_without_a_hub_grant_parks_a_hard_send_card_and_approve_sends_once() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("sync-park");
    std::fs::create_dir_all(&root).unwrap();
    // A paired computer and no grant: still the hard Send card (SY-03).
    pair_test_peer(&cabin, "p1");
    cabin.run_slash_line("/sync");
    assert!(cabin.sync_rx.is_none(), "nothing leaves without a grant");
    let park = cabin.harness.park.clone().expect("hard card");
    assert_eq!(
        park.source,
        super::harness_ui::ParkSource::Egress("hub".into())
    );
    assert_eq!(park.class, grokhub_agent::HardClass::Send);
    assert_eq!(park.path, "egress");
    assert_eq!(park.tool, "hub_sync");
    assert_eq!(park.action, "/sync → paired computers (chats, memory)");
    assert_eq!(cabin.hard_waiting(), 1);
    assert!(cabin.hub.lock().unwrap().snapshot.is_none());
    assert!(hx::read_egress(&root).is_empty());
    // The same hard card: Enter never approves, Esc denies.
    assert_eq!(hx::hard_card_key(true, false, false), None);
    assert_eq!(
        hx::hard_card_key(false, true, false),
        Some(hx::HardAnswer::Deny)
    );

    cabin.resolve_hard_park(false, "Jeremy denied (Esc)");
    assert!(cabin.sync_rx.is_none());
    assert!(cabin.harness.park.is_none());
    assert_eq!(cabin.status, "Denied · Send");
    assert!(hx::read_egress(&root).is_empty());

    cabin.run_slash_line("/sync");
    assert!(cabin.harness.park.is_some());
    cabin.resolve_hard_park(true, "");
    assert!(cabin.sync_rx.is_some(), "Approve sends once");
    wait_sync(&mut cabin);
    assert_eq!(cabin.status, "Synced chats and memory to 1 computer.");
    assert!(cabin.hub.lock().unwrap().snapshot.is_some());
    let log = hx::read_egress(&root);
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].dest, "hub");
    assert_eq!(log[0].grant_id, "approved-once");
    assert_eq!(log[0].basis, "approved_once");
    assert_eq!(
        hx::ConsentLedger::load(&root),
        hx::ConsentLedger::empty(),
        "a hard card never writes a standing grant"
    );
    let spans = harness_spans(&root);
    let decisions: Vec<&str> = spans.iter().map(|s| s.decision.as_str()).collect();
    assert_eq!(decisions, vec!["park", "deny", "park", "approve", "allow"]);
    assert!(spans
        .iter()
        .all(|s| s.tool == "hub_sync" && s.path == "egress"));
    assert_eq!(spans[4].consent_ref, "approved-once");
    assert_eq!(log[0].span_id, spans[4].span_ref());
    assert!(hx::approval_gate_violation(&spans).is_empty());

    // Still no grant: the next /sync asks again.
    cabin.run_slash_line("/sync");
    assert!(cabin.sync_rx.is_none());
    assert!(cabin.harness.park.is_some());
    release_isolated(&root, cabin);
}

#[test]
fn revoking_the_hub_grant_blocks_the_next_sync_and_drops_the_share() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("sync-revoke");
    std::fs::create_dir_all(&root).unwrap();
    pair_test_peer(&cabin, "p1");
    let g = hx::grant_destination(
        &root,
        hx::HUB_DEST,
        hx::HUB_SYNC_DATA,
        hx::UserClick::from_click(),
    )
    .unwrap();
    cabin.harness.consent = None;
    assert!(cabin
        .consent()
        .destination_grant("hub", hx::HUB_SYNC_DATA)
        .is_some());
    cabin.hub.lock().unwrap().snapshot = Some(std::sync::Arc::new(
        serde_json::json!({"kind": "grokhub-hub-v1"}),
    ));
    cabin.revoke_hub_grant(&g.id);
    assert_eq!(
        cabin.status,
        "Sync to paired computers revoked. /sync asks again."
    );
    assert!(
        cabin.hub.lock().unwrap().snapshot.is_none(),
        "revoke stops serving the old share"
    );
    assert_eq!(cabin.consent().active().count(), 0);
    cabin.run_slash_line("/sync");
    assert!(cabin.sync_rx.is_none());
    assert_eq!(
        cabin.harness.park.as_ref().map(|p| p.source.clone()),
        Some(super::harness_ui::ParkSource::Egress("hub".into()))
    );
    assert!(hx::read_egress(&root).is_empty());
    release_isolated(&root, cabin);
}

#[test]
fn privacy_slash_lists_grants_scopes_and_egress_without_content() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("privacy-slash");
    std::fs::create_dir_all(&root).unwrap();
    let before = cabin.messages.len();
    assert_eq!(
        hx::guard_egress(
            &root,
            &hx::EgressReq::new("https://api.x.ai/v1/responses", &[hx::DataClass::Chat])
        ),
        hx::GateOutcome::Allow
    );
    cabin.run_slash_line("/privacy");
    assert!(!cabin.running, "/privacy never reaches the model");
    let start = std::time::Instant::now();
    while cabin.harness.privacy_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(4)
    {
        cabin.poll_privacy();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(cabin.messages.len(), before + 1);
    let (role, body) = cabin.messages.last().cloned().unwrap();
    assert_eq!(role, "assistant");
    assert!(
        grokhub_core::is_cabin_slash_turn(&role, &body),
        "stays out of the next model kick"
    );
    let text = grokhub_core::strip_slash_result(&body);
    assert!(
        text.starts_with("/privacy — what leaves this computer\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "- Off: Sync to paired computers (/sync asks each time) · Files in a folder · "
        ),
        "{text}"
    );
    assert!(text.contains("Granted folders, apps, browser history and system state are read on this computer only, never on battery or in quiet hours; calendar and mail aren't read yet."), "{text}");
    assert!(
        text.contains("- api.x.ai · 1 time · chats · default · last just now"),
        "{text}"
    );
    release_isolated(&root, cabin);
}

// SY-03: nothing paired means nothing to send. No card, no egress line, no
// span, with or without the grant; one plain result line instead.
#[test]
fn sync_with_no_paired_computer_posts_a_line_and_sends_nothing() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("sync-no-peers");
    std::fs::create_dir_all(&root).unwrap();
    assert!(cabin.hub.lock().unwrap().peers.is_empty());
    let before = cabin.messages.len();
    cabin.run_slash_line("/sync");
    assert!(cabin.harness.park.is_none(), "no peer: no hard card");
    assert_eq!(cabin.hard_waiting(), 0);
    assert!(cabin.sync_rx.is_none(), "no peer: nothing is sent");
    assert_eq!(
        cabin.status,
        "Nothing paired yet. Start share to pair a computer."
    );
    assert_eq!(cabin.messages.len(), before + 1);
    let (role, body) = cabin.messages.last().cloned().unwrap();
    assert!(grokhub_core::is_cabin_slash_turn(&role, &body));
    assert_eq!(
        grokhub_core::strip_slash_result(&body),
        "Nothing paired yet. Start share to pair a computer."
    );
    assert!(hx::read_egress(&root).is_empty(), "no peer: no egress line");
    assert!(harness_spans(&root).is_empty(), "no peer: no span");
    assert!(cabin.hub.lock().unwrap().snapshot.is_none());

    // With the grant it is the same: nobody to send to.
    hx::grant_destination(
        &root,
        hx::HUB_DEST,
        hx::HUB_SYNC_DATA,
        hx::UserClick::from_click(),
    )
    .unwrap();
    cabin.harness.consent = None;
    cabin.run_slash_line("/sync");
    assert!(cabin.harness.park.is_none());
    assert!(cabin.sync_rx.is_none());
    assert!(hx::read_egress(&root).is_empty());
    assert!(harness_spans(&root).is_empty());
    assert!(cabin.hub.lock().unwrap().snapshot.is_none());

    // A card approved after the last computer unpaired sends nothing either.
    hx::revoke_grant(&root, &cabin.consent().active().next().unwrap().id.clone()).unwrap();
    cabin.harness.consent = None;
    pair_test_peer(&cabin, "p1");
    cabin.run_slash_line("/sync");
    assert!(
        cabin.harness.park.is_some(),
        "a peer and no grant parks the hard Send card"
    );
    cabin.hub.lock().unwrap().peers.clear();
    cabin.resolve_hard_park(true, "");
    assert!(cabin.sync_rx.is_none());
    assert!(hx::read_egress(&root).is_empty());
    assert_eq!(
        cabin.status,
        "Nothing paired yet. Start share to pair a computer."
    );
    release_isolated(&root, cabin);
}

// SY-05/SY-06: Permissions groups the trust rows under "Leaving this computer"
// and the rule editor under "Command rules". Allow is filled; Revoke is a ghost.
#[test]
fn permissions_page_groups_trust_rows_and_rules_with_a_ghost_revoke() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("perm-headings");
    std::fs::create_dir_all(&root).unwrap();
    struct Painted {
        texts: Vec<(String, egui::Pos2)>,
        filled: Vec<egui::Rect>,
    }
    fn walk(shape: &egui::Shape, p: &mut Painted) {
        match shape {
            egui::Shape::Text(t) => p.texts.push((
                t.galley.text().to_string(),
                t.pos + t.galley.rect.center().to_vec2(),
            )),
            egui::Shape::Rect(r) if r.fill == crate::theme::fg() => p.filled.push(r.rect),
            egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, p)),
            _ => {}
        }
    }
    let paint = |cabin: &mut Cabin| {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 2400.0),
            )),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| cabin.ui_permission_editor(ui));
        });
        let mut p = Painted {
            texts: Vec::new(),
            filled: Vec::new(),
        };
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut p);
        }
        p
    };
    let p = paint(&mut cabin);
    let at = |p: &Painted, want: &str| {
        p.texts.iter().position(|t| t.0 == want).unwrap_or_else(|| {
            panic!(
                "{want} not painted: {:?}",
                p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
            )
        })
    };
    let order = [
        "Leaving this computer",
        super::privacy_ui::PRIVACY_NOTE,
        "Sync to paired computers",
        "Allow",
        "Command rules",
        "Deny wins over ask, and ask wins over allow. Dangerous commands still ask, including after Allow always.",
        "Rule",
    ];
    let idx: Vec<usize> = order.iter().map(|w| at(&p, w)).collect();
    assert!(
        idx.windows(2).all(|w| w[0] < w[1]),
        "order {order:?} got {idx:?}"
    );
    let ys: Vec<f32> = idx.iter().map(|&i| p.texts[i].1.y).collect();
    assert!(
        ys[4] > ys[2] && ys[6] > ys[5],
        "headings sit above their rows: {ys:?}"
    );
    let allow = p.texts[at(&p, "Allow")].1;
    assert!(
        !p.filled.iter().any(|r| r.contains(allow)),
        "SB-08: Allow is an outline, not filled"
    );

    hx::grant_destination(
        &root,
        hx::HUB_DEST,
        hx::HUB_SYNC_DATA,
        hx::UserClick::from_click(),
    )
    .unwrap();
    cabin.harness.consent = None;
    let p = paint(&mut cabin);
    let revoke = p.texts[at(&p, "Revoke")].1;
    assert!(
        !p.filled.iter().any(|r| r.contains(revoke)),
        "Revoke is a ghost, not filled"
    );
    assert!(
        p.texts
            .iter()
            .any(|t| t.0.ends_with("Sends chats, memory.")),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    release_isolated(&root, cabin);
}

// SY-10: both Permissions headings open their group with SECTION_HEAD_GAP of
// room above (same heading size), so "Command rules" no longer sits as close
// to the Sync row as rows sit to each other.
#[test]
fn permissions_headings_get_room_above_them() {
    let rows = include_str!("privacy_ui.rs")
        .split("fn ui_privacy_rows(")
        .nth(1)
        .and_then(|s| s.split("fn privacy_revoke_rows(").next())
        .expect("ui_privacy_rows");
    assert!(rows.contains("section_heading(ui, LEAVING_HEAD)"), "{rows}");
    let src = cabin_src();
    let editor = fn_src(&src, "ui_permission_editor");
    assert!(
        editor.contains("section_heading(ui, super::privacy_ui::RULES_HEAD)"),
        "{editor}"
    );
    assert!(
        !editor.contains("section_label(ui, super::privacy_ui::RULES_HEAD)"),
        "{editor}"
    );
    assert!((8.0..=12.0).contains(&crate::cards::SECTION_HEAD_GAP));

    // Painted: the gap from the Sync row to "Command rules" beats the
    // row-to-row gap by at least SECTION_HEAD_GAP (before: 21px vs 15px).
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("perm-head-gap");
    std::fs::create_dir_all(&root).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 2400.0),
        )),
        ..Default::default()
    };
    let out = crate::theme::test_pass(&ctx, input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| cabin.ui_permission_editor(ui));
    });
    let mut tops: Vec<(String, f32, f32)> = Vec::new();
    for clipped in &out.shapes {
        if let egui::Shape::Text(t) = &clipped.shape {
            tops.push((
                t.galley.text().to_string(),
                t.pos.y + t.galley.rect.top(),
                t.pos.y + t.galley.rect.bottom(),
            ));
        }
    }
    let find = |want: &str| {
        tops.iter()
            .find(|t| t.0 == want)
            .cloned()
            .unwrap_or_else(|| panic!("{want}: {tops:?}"))
    };
    let hub_hint = tops
        .iter()
        .find(|t| t.0.starts_with("Off. /sync asks"))
        .cloned()
        .expect("hub hint");
    let rules = find("Command rules");
    let gap = rules.1 - hub_hint.2;
    // Row rhythm: a help line to the next row title in the same group.
    let note = tops
        .iter()
        .find(|t| t.0.starts_with("Deny wins over ask"))
        .cloned()
        .expect("rules note");
    let row_gap = find("Rule").1 - note.2;
    assert!(
        gap - row_gap >= crate::cards::SECTION_HEAD_GAP - 0.5,
        "Command rules sits {gap}px under the Sync row; rows sit {row_gap}px apart"
    );
    release_isolated(&root, cabin);
}

// GL-05: aged slash and system results keep the result bubble. They never
// paint in the thought frame and never carry the "Thought process" label or
// its fold control, even with the session's thoughts collapsed.
#[test]
fn aged_slash_results_never_render_in_the_thought_frame() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("gl05-result-style");
    std::fs::create_dir_all(&root).unwrap();
    let report = super::privacy_ui::privacy_report(
        &grokhub_agent::harness::ConsentLedger::empty(),
        &grokhub_agent::harness::EgressRead::default(),
        None,
        false,
        1_000 * 86_400_000,
    );
    cabin.live_mut().push(("user".into(), "hello".into()));
    cabin.live_mut().push(("assistant".into(), "Hi.".into()));
    cabin.live_mut().push((
        "assistant".into(),
        mark_slash_result("Synced chats and memory to 1 computer."),
    ));
    cabin
        .live_mut()
        .push(("assistant".into(), mark_slash_result(&report)));
    cabin.live_mut().push((
        "assistant".into(),
        mark_slash_result("Sync to paired computers revoked. /sync asks again."),
    ));
    let views = cabin.cached_chat_views().to_vec();
    let kinds: Vec<ChatKind> = views.iter().map(|v| v.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ChatKind::User,
            ChatKind::Assistant,
            ChatKind::Result,
            ChatKind::Result,
            ChatKind::Result
        ]
    );
    // The aged /privacy report is still the one the ghost Revoke goes under.
    assert_eq!(super::privacy_ui::newest_privacy_row(&views), Some(3));

    let paint = |view: &ChatView, fold: ThoughtFold| {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1400.0),
            )),
            ..Default::default()
        };
        let mut drawn = false;
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                drawn =
                    super::chat_ui::paint_chat_block_with(ui, view, true, true, fold, true).drawn;
            });
        });
        let mut texts: Vec<(String, bool)> = Vec::new();
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, bool)>) {
            match shape {
                egui::Shape::Text(t) => {
                    let italic = t.galley.job.sections.iter().any(|s| s.format.italics);
                    out.push((t.galley.text().to_string(), italic));
                }
                egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, out)),
                _ => {}
            }
        }
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut texts);
        }
        (drawn, texts)
    };
    for view in views.iter().filter(|v| v.kind == ChatKind::Result) {
        // Expanded is what the pane passes; Minimized and Hidden are what an aged,
        // collapsed thought would get. A result ignores all three.
        for fold in [
            ThoughtFold::Expanded,
            ThoughtFold::Minimized,
            ThoughtFold::Hidden,
        ] {
            let (drawn, texts) = paint(view, fold);
            assert!(drawn, "a result is never folded away");
            for (t, italic) in &texts {
                assert!(!t.contains("Thought process"), "{t}");
                assert_ne!(
                    t,
                    grokhub_core::THOUGHT_ROW_LABEL,
                    "no thought row: {texts:?}"
                );
                assert_ne!(t, "Collapse", "no thought fold control: {texts:?}");
                assert_ne!(t, "Expand", "no thought fold control: {texts:?}");
                assert!(!italic, "a result is upright, not thought italics: {t}");
            }
            let first = view.body.lines().next().unwrap_or_default();
            assert!(
                texts.iter().any(|(t, _)| t.contains(first)),
                "{first} painted: {texts:?}"
            );
        }
    }
    // The pane paints a result with the result bubble, not the thought bubble.
    let src = include_str!("chat_ui.rs");
    let arm = src
        .split("ChatKind::Result => {")
        .nth(1)
        .and_then(|s| s.split("ChatKind::Thought => {").next())
        .expect("Result arm");
    assert!(arm.contains("paint_result_bubble"), "{arm}");
    assert!(
        !arm.contains("paint_thought_bubble")
            && !arm.contains("Thought process")
            && !arm.contains("italics"),
        "{arm}"
    );
    release_isolated(&root, cabin);
}

// GL-05 guard: aged model reasoning still collapses exactly as before: the
// "Thought process" label and Collapse while expanded, the "Thought" row and
// Expand when minimized, nothing when hidden.
#[test]
fn aged_model_reasoning_still_collapses() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("gl05-thought");
    std::fs::create_dir_all(&root).unwrap();
    cabin.live_mut().push(("user".into(), "fix it".into()));
    cabin.live_mut().push((
        "assistant".into(),
        grokhub_core::merge_thinking("Plan: read the file first.", "Looking now."),
    ));
    cabin
        .live_mut()
        .push(("assistant".into(), "Fixed the typo in main.rs.".into()));
    cabin.live_mut().push((
        "assistant".into(),
        mark_slash_result("Synced chats and memory to 1 computer."),
    ));
    let views = cabin.cached_chat_views().to_vec();
    let kinds: Vec<ChatKind> = views.iter().map(|v| v.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ChatKind::User,
            ChatKind::Thought,
            ChatKind::Thought,
            ChatKind::Assistant,
            ChatKind::Result
        ]
    );
    let thought = &views[1];
    let paint = |fold: ThoughtFold| {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1400.0),
            )),
            ..Default::default()
        };
        let mut painted = None;
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                painted = Some(super::chat_ui::paint_chat_block_with(
                    ui, thought, true, true, fold, true,
                ));
            });
        });
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            if let egui::Shape::Text(t) = &clipped.shape {
                texts.push(t.galley.text().to_string());
            }
        }
        (painted.expect("painted").drawn, texts)
    };
    let (drawn, texts) = paint(ThoughtFold::Expanded);
    assert!(drawn);
    assert!(texts.iter().any(|t| t == "Thought process"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Collapse"), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Plan: read the file first.")),
        "{texts:?}"
    );
    let (drawn, texts) = paint(ThoughtFold::Minimized);
    assert!(drawn);
    assert!(
        texts.iter().any(|t| t == grokhub_core::THOUGHT_ROW_LABEL),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t == "Expand"), "{texts:?}");
    assert!(
        !texts
            .iter()
            .any(|t| t.contains("Plan: read the file first.")),
        "{texts:?}"
    );
    let (drawn, _) = paint(ThoughtFold::Hidden);
    assert!(!drawn, "a hidden thought still paints nothing");
    release_isolated(&root, cabin);
}

// SY-04: the ghost Revoke under /privacy clears the grant and drops the share.
// Only a pointer click answers it; Enter and typed text never do.
#[test]
fn revoke_from_privacy_clears_the_grant_on_a_click_only() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("privacy-revoke");
    std::fs::create_dir_all(&root).unwrap();
    let g = hx::grant_destination(
        &root,
        hx::HUB_DEST,
        hx::HUB_SYNC_DATA,
        hx::UserClick::from_click(),
    )
    .unwrap();
    cabin.harness.consent = None;
    cabin.hub.lock().unwrap().snapshot = Some(std::sync::Arc::new(
        serde_json::json!({"kind": "grokhub-hub-v1"}),
    ));
    let rows = cabin.privacy_revoke_rows();
    assert_eq!(
        rows,
        vec![super::privacy_ui::PrivacyRevoke {
            id: g.id.clone(),
            label: "Sync to paired computers".to_string(),
            detail: None,
        }]
    );

    // Slash text cannot revoke (or grant): it is not a click.
    cabin.run_slash_line("/privacy revoke");
    cabin.harness.consent = None;
    assert_eq!(cabin.consent().active().count(), 1);

    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 400.0));
    let pass = |events: Vec<egui::Event>| {
        let mut hit = None;
        let mut texts = Vec::new();
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                hit = super::privacy_ui::paint_privacy_revokes(ui, &rows, None);
            });
        });
        let mut revoke_at = None;
        for clipped in &out.shapes {
            if let egui::Shape::Text(t) = &clipped.shape {
                let label = t.galley.text().to_string();
                if label == "Revoke" {
                    revoke_at = Some(t.pos + t.galley.rect.center().to_vec2());
                }
                texts.push(label);
            }
        }
        (hit, revoke_at, texts)
    };
    let (hit, at, texts) = pass(vec![]);
    assert_eq!(hit, None);
    assert!(
        texts.iter().all(|t| !t.contains(&g.id)),
        "no grant id on screen: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "Sync to paired computers"),
        "{texts:?}"
    );
    let at = at.expect("Revoke pill");
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let (hit, _, _) = pass(vec![enter, egui::Event::Text("revoke".into())]);
    assert_eq!(hit, None, "keys never revoke");
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = pass(vec![egui::Event::PointerMoved(at)]);
    let _ = pass(vec![press(true)]);
    let (hit, _, _) = pass(vec![press(false)]);
    assert_eq!(
        hit.as_deref(),
        Some(g.id.as_str()),
        "a click names the grant"
    );

    let before = cabin.messages.len();
    cabin.revoke_from_privacy(&g.id);
    assert_eq!(cabin.consent().active().count(), 0, "the grant is gone");
    assert!(hx::ConsentLedger::load(&root)
        .destination_grant(hx::HUB_DEST, hx::HUB_SYNC_DATA)
        .is_none());
    assert!(
        cabin.hub.lock().unwrap().snapshot.is_none(),
        "revoke drops the share"
    );
    assert_eq!(
        cabin.status,
        "Sync to paired computers revoked. /sync asks again."
    );
    assert_eq!(cabin.messages.len(), before + 1);
    assert!(
        cabin.privacy_revoke_rows().is_empty(),
        "no grant, no Revoke"
    );
    // An unknown or already revoked id is a no-op.
    cabin.revoke_from_privacy(&g.id);
    assert_eq!(cabin.messages.len(), before + 1);
    release_isolated(&root, cabin);
}

// Spike-4b: Settings → Permissions paints the scope rows under "What GrokHub
// can read". Texts with their centers, and which rects are filled.
struct ScopePaint {
    texts: Vec<(String, egui::Pos2)>,
    filled: Vec<egui::Rect>,
}

impl ScopePaint {
    fn has(&self, want: &str) -> bool {
        self.texts.iter().any(|t| t.0 == want)
    }
    fn at(&self, want: &str) -> egui::Pos2 {
        self.texts
            .iter()
            .find(|t| t.0 == want)
            .unwrap_or_else(|| {
                panic!(
                    "{want} not painted: {:?}",
                    self.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
                )
            })
            .1
    }
    /// The pill (`Allow` / `Revoke`) on the same row as `title`.
    fn pill_by(&self, title: &str, pill: &str) -> egui::Pos2 {
        let y = self.at(title).y;
        self.texts
            .iter()
            .filter(|t| t.0 == pill)
            .min_by(|a, b| (a.1.y - y).abs().total_cmp(&(b.1.y - y).abs()))
            .map(|t| t.1)
            .unwrap_or_else(|| panic!("no {pill} by {title}"))
    }
}

fn paint_permissions(
    ctx: &egui::Context,
    cabin: &mut Cabin,
    events: Vec<egui::Event>,
) -> ScopePaint {
    fn walk(shape: &egui::Shape, p: &mut ScopePaint) {
        match shape {
            egui::Shape::Text(t) => p.texts.push((
                t.galley.text().to_string(),
                t.pos + t.galley.rect.center().to_vec2(),
            )),
            egui::Shape::Rect(r) if r.fill == crate::theme::fg() => p.filled.push(r.rect),
            egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, p)),
            _ => {}
        }
    }
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 2400.0),
        )),
        events,
        ..Default::default()
    };
    let out = crate::theme::test_pass(ctx, input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| cabin.ui_permission_editor(ui));
    });
    let mut p = ScopePaint {
        texts: Vec::new(),
        filled: Vec::new(),
    };
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut p);
    }
    p
}

fn click_at(ctx: &egui::Context, cabin: &mut Cabin, at: egui::Pos2) {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = paint_permissions(ctx, cabin, vec![egui::Event::PointerMoved(at)]);
    let _ = paint_permissions(ctx, cabin, vec![press(true)]);
    let _ = paint_permissions(ctx, cabin, vec![press(false)]);
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn scope_rows_are_all_off_under_what_grokhub_can_read() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-rows");
    std::fs::create_dir_all(&root).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    let ys: Vec<f32> = [
        "Leaving this computer",
        "Sync to paired computers",
        "What GrokHub can read",
        super::scope_ui::SCOPES_NOTE,
        "Files in a folder",
        "Installed apps",
        "Browser history",
        "Calendar",
        "Mail",
        "System state",
        "Command rules",
    ]
    .iter()
    .map(|w| p.at(w).y)
    .collect();
    assert!(ys.windows(2).all(|w| w[0] < w[1]), "rows in order: {ys:?}");
    for (kind, label) in grokhub_agent::harness::SCOPE_KINDS {
        let allow = p.pill_by(label, "Allow");
        // The folder row's field, dialog button and Allow share a line under its hint.
        let reach = if *kind == "files" { 56.0 } else { 24.0 };
        assert!(
            (allow.y - p.at(label).y).abs() < reach,
            "{kind}: Allow sits on its row"
        );
        // SB-08: an outline Allow, so the off rows read as the calm default.
        assert!(
            !p.filled.iter().any(|r| r.contains(allow)),
            "{kind}: Allow is an outline, not filled"
        );
    }
    assert!(
        !p.filled
            .iter()
            .any(|r| r.contains(p.pill_by("Sync to paired computers", "Allow"))),
        "Sync's Allow too"
    );
    assert!(!p.has("Revoke"), "nothing granted, nothing to revoke");
    assert!(
        p.has("Off. Your calendar events, read through Grok Build's Google Calendar connector."),
        "plain hints"
    );
    assert!(
        p.has("Off. Your mail, read through Grok Build's Gmail connector."),
        "the hint names where mail is read"
    );
    // SB-04: the folder row offers the native dialog next to the typed path.
    let choose = p.pill_by("Files in a folder", super::scope_ui::CHOOSE_FOLDER);
    assert!(
        (choose.y - p.pill_by("Files in a folder", "Allow").y).abs() < 2.0,
        "the dialog button sits by the folder's Allow"
    );
    assert!(
        !root.join("consent.jsonl").exists(),
        "painting writes nothing"
    );
    release_isolated(&root, cabin);
}

#[test]
fn a_scope_is_granted_by_a_pointer_click_only_and_revoked_from_settings_or_privacy() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-grant");
    std::fs::create_dir_all(&root).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let _ = paint_permissions(&ctx, &mut cabin, vec![]);

    // Keys never grant: walk focus over every widget and press Enter and Space.
    for _ in 0..40 {
        let _ = paint_permissions(&ctx, &mut cabin, vec![key(egui::Key::Tab)]);
        let _ = paint_permissions(&ctx, &mut cabin, vec![key(egui::Key::Enter)]);
        let _ = paint_permissions(&ctx, &mut cabin, vec![key(egui::Key::Space)]);
    }
    assert_eq!(
        hx::ConsentLedger::load(&root).active().count(),
        0,
        "keyboard never grants"
    );
    // Neither does slash text.
    cabin.run_slash_line("/privacy allow apps");
    assert_eq!(hx::ConsentLedger::load(&root).active().count(), 0);

    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    click_at(&ctx, &mut cabin, p.pill_by("Installed apps", "Allow"));
    assert_eq!(
        cabin.status,
        "Installed apps allowed. GrokHub learns from it on this computer. Revoke it here any time."
    );
    let ledger = hx::ConsentLedger::load(&root);
    let g = ledger
        .scope_grant(&hx::Scope::Apps)
        .cloned()
        .expect("apps granted");
    assert_eq!(ledger.active().count(), 1, "one click, one scope");
    let raw = std::fs::read_to_string(root.join("consent.jsonl")).unwrap();
    assert!(
        !raw.contains("apps") && !raw.contains(&g.id),
        "the ledger is sealed: {raw}"
    );

    // The row turns into a ghost Revoke; the others stay off.
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    let revoke = p.pill_by("Installed apps", "Revoke");
    assert!(
        !p.filled.iter().any(|r| r.contains(revoke)),
        "Revoke is a ghost"
    );
    assert!(p
        .texts
        .iter()
        .any(|t| t.0.starts_with("On since ") && t.0.ends_with("how often you open them.")));
    assert!(p.has("Off. Your mail, read through Grok Build's Gmail connector."));

    // A folder: typed, then clicked. Home itself is refused even with a click.
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() {
        cabin.harness.scope_folder = home.clone();
        let p = paint_permissions(&ctx, &mut cabin, vec![]);
        click_at(&ctx, &mut cabin, p.pill_by("Files in a folder", "Allow"));
        assert_eq!(
            cabin.status,
            "Not allowed: files: one folder at a time, never all of your home folder"
        );
        let p = paint_permissions(&ctx, &mut cabin, vec![]);
        assert!(
            p.has(&cabin.status),
            "the refusal shows in Settings, not only on the hidden status line"
        );
        assert!(p.at(&cabin.status).y > p.at("What GrokHub can read").y);
    }
    cabin.harness.scope_folder = "/srv/notes/".into();
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    click_at(&ctx, &mut cabin, p.pill_by("Files in a folder", "Allow"));
    assert_eq!(
        cabin.status,
        "Files in notes allowed. GrokHub learns from it on this computer. Revoke it here any time."
    );
    assert!(cabin.harness.scope_folder.is_empty());
    // SB-03 / SB-06: the folder name titles the row, the path is in the hint,
    // and the next folder row reads "Add a folder".
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(
        p.has("Files in notes") && p.has(super::scope_ui::ADD_FOLDER),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    assert!(p
        .texts
        .iter()
        .any(|t| t.0.starts_with("On since ") && t.0.ends_with(". /srv/notes")));
    assert!(!p.has("Files in a folder"));

    // /privacy lists both by plain name, and its ghost Revoke works for scopes.
    cabin.harness.consent = None;
    let rows = cabin.privacy_revoke_rows();
    let names: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(names, vec!["Installed apps", "Files in notes"]);
    assert_eq!(
        rows[1].detail.as_deref(),
        Some("/srv/notes"),
        "the whole path rides along for hover"
    );
    let report = super::privacy_ui::privacy_report(
        &hx::ConsentLedger::load(&root),
        &hx::EgressRead::default(),
        None,
        false,
        g.granted_at + 60_000,
    );
    assert!(
        report.contains("\n- Installed apps: on since 1m ago\n- Files in notes: on since "),
        "{report}"
    );
    assert!(report.contains("\n- Off: Sync to paired computers (/sync asks each time) · Browser history · Calendar · Mail · System state\n"), "{report}");
    assert!(
        !report.contains("Read apps") && !report.contains(&g.id),
        "{report}"
    );
    let files_id = rows[1].id.clone();
    let before = cabin.messages.len();
    cabin.revoke_from_privacy(&files_id);
    assert_eq!(
        cabin.status,
        "Files in notes revoked. It's off until you allow it again."
    );
    assert_eq!(cabin.messages.len(), before + 1);

    // Settings Revoke.
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    click_at(&ctx, &mut cabin, p.pill_by("Installed apps", "Revoke"));
    assert_eq!(
        cabin.status,
        "Installed apps revoked. It's off until you allow it again."
    );
    assert_eq!(hx::ConsentLedger::load(&root).active().count(), 0);
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(!p.has("Revoke"), "all off again");
    release_isolated(&root, cabin);
}

#[test]
fn locked_private_data_shows_one_message_and_grants_or_syncs_nothing() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-locked");
    std::fs::create_dir_all(&root).unwrap();
    hx::use_key_store_for(
        &root,
        std::sync::Arc::new(hx::MemoryKeyStore::unavailable()),
    );
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    let msg = hx::Locked::Unavailable.message();
    let next =
        super::privacy_ui::lock_next_step_short(&hx::Locked::Unavailable, hx::KeyringOs::current());
    assert!(
        msg.starts_with("Private data is locked: GrokHub can't reach your "),
        "{msg}"
    );
    assert!(
        p.has(&msg),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    assert_eq!(
        p.texts.iter().filter(|t| t.0 == msg).count(),
        1,
        "said once"
    );
    assert!(
        p.at(&msg).y < p.at("Sync to paired computers").y,
        "said at the top, above every row it holds"
    );
    // SB-01: every Allow is disabled, so a click does nothing at all.
    cabin.status.clear();
    click_at(&ctx, &mut cabin, p.pill_by("Calendar", "Allow"));
    assert_eq!(cabin.status, "", "a disabled Allow takes no click");
    click_at(
        &ctx,
        &mut cabin,
        p.pill_by("Sync to paired computers", "Allow"),
    );
    assert_eq!(cabin.status, "");
    assert!(
        !root.join("consent.jsonl").exists(),
        "no plaintext fallback"
    );
    assert!(!root.join(hx::KEY_ID_FILE).exists());

    pair_test_peer(&cabin, "p1");
    cabin.run_slash_line("/sync");
    assert!(
        cabin.harness.park.is_none() && cabin.sync_rx.is_none(),
        "nothing is sent"
    );
    assert_eq!(cabin.status, format!("Not synced. {msg} {next}"));
    let ledger = hx::ConsentLedger::load(&root);
    let lock = super::privacy_ui::private_lock(&root, &ledger, true);
    let report = super::privacy_ui::privacy_report(
        &ledger,
        &hx::read_egress_report(&root),
        lock.as_ref(),
        false,
        1_000,
    );
    assert!(
        report.contains(&format!("\n\n{msg}\n{next}\n\nGrants\n")),
        "{report}"
    );
    assert!(!root.join("egress.jsonl").exists());
    release_isolated(&root, cabin);
}

// SB-01: while locked, each "Allow" (and the folder dialog button) is painted
// faded by egui's disabled look; returns (label, opacity) for each pill text.
// The opacity is the painted text colour's alpha (egui multiplies it by
// `disabled_alpha`). Only rows above "Command rules" count.
fn pill_opacity(
    ctx: &egui::Context,
    cabin: &mut Cabin,
    events: Vec<egui::Event>,
) -> (Vec<(String, f32)>, Vec<String>) {
    fn walk(
        shape: &egui::Shape,
        pills: &mut Vec<(String, f32, f32)>,
        all: &mut Vec<(String, f32)>,
    ) {
        match shape {
            egui::Shape::Text(t) => {
                let text = t.galley.text().to_string();
                if [
                    "Allow",
                    "Revoke",
                    super::scope_ui::CHOOSE_FOLDER,
                    super::privacy_ui::TRY_AGAIN,
                ]
                .contains(&text.as_str())
                {
                    pills.push((text.clone(), t.fallback_color.a() as f32 / 255.0, t.pos.y));
                }
                all.push((text, t.pos.y));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, pills, all)),
            _ => {}
        }
    }
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 2400.0),
        )),
        events,
        ..Default::default()
    };
    let out = crate::theme::test_pass(ctx, input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| cabin.ui_permission_editor(ui));
    });
    let (mut pills, mut all) = (Vec::new(), Vec::new());
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut pills, &mut all);
    }
    let rules = all
        .iter()
        .find(|t| t.0 == "Command rules")
        .map(|t| t.1)
        .unwrap_or(f32::MAX);
    let pills = pills
        .into_iter()
        .filter(|p| p.2 < rules)
        .map(|p| (p.0, p.1))
        .collect();
    (pills, all.into_iter().map(|t| t.0).collect())
}

#[test]
fn locked_state_disables_every_allow_with_the_lock_on_hover() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-locked-disabled");
    std::fs::create_dir_all(&root).unwrap();
    hx::use_key_store_for(
        &root,
        std::sync::Arc::new(hx::MemoryKeyStore::unavailable()),
    );
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    ctx.global_style_mut(|s| s.interaction.tooltip_delay = 0.0);
    let (pills, texts) = pill_opacity(&ctx, &mut cabin, vec![]);
    let allows: Vec<f32> = pills
        .iter()
        .filter(|p| p.0 == "Allow")
        .map(|p| p.1)
        .collect();
    assert_eq!(allows.len(), 7, "six scope rows and Sync: {pills:?}");
    assert!(
        allows.iter().all(|o| *o < 1.0),
        "every Allow takes the disabled look: {pills:?}"
    );
    let choose = pills
        .iter()
        .find(|p| p.0 == super::scope_ui::CHOOSE_FOLDER)
        .expect("dialog button");
    assert!(
        choose.1 < 1.0,
        "the folder dialog is disabled too: {pills:?}"
    );
    let retry = pills
        .iter()
        .find(|p| p.0 == super::privacy_ui::TRY_AGAIN)
        .expect("Try again");
    assert!(
        (retry.1 - 1.0).abs() < f32::EPSILON,
        "Try again stays live: {pills:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "Locked: keyring unavailable"),
        "no hover yet"
    );

    // Hover a disabled Allow: the lock shows.
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    let at = p.pill_by("Calendar", "Allow");
    let mut shown = false;
    for _ in 0..4 {
        let (_, texts) = pill_opacity(&ctx, &mut cabin, vec![egui::Event::PointerMoved(at)]);
        shown |= texts.iter().any(|t| t == "Locked: keyring unavailable");
    }
    assert!(shown, "hover names the lock");
    assert_eq!(
        super::privacy_ui::lock_hover(&hx::Locked::Unavailable),
        "Locked: keyring unavailable"
    );

    // A click on any of them writes nothing and says nothing new.
    cabin.status.clear();
    for title in [
        "Calendar",
        "Installed apps",
        "Sync to paired computers",
        "Files in a folder",
    ] {
        click_at(&ctx, &mut cabin, p.pill_by(title, "Allow"));
    }
    assert_eq!(cabin.status, "");
    assert!(
        cabin.harness.scope_pick_rx.is_none(),
        "the dialog never opened"
    );
    assert!(!root.join("consent.jsonl").exists());

    // Unlocked, the same pills are live.
    hx::use_key_store_for(&root, std::sync::Arc::new(hx::MemoryKeyStore::new()));
    cabin.retry_keyring();
    let (pills, _) = pill_opacity(&ctx, &mut cabin, vec![]);
    assert!(
        pills
            .iter()
            .filter(|p| p.0 == "Allow")
            .all(|p| (p.1 - 1.0).abs() < f32::EPSILON),
        "{pills:?}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn try_again_asks_the_keyring_again_and_refreshes_permissions() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("scope-try-again");
    std::fs::create_dir_all(&root).unwrap();
    let store = std::sync::Arc::new(hx::MemoryKeyStore::unavailable());
    hx::use_key_store_for(&root, store.clone());
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let msg = hx::Locked::Unavailable.message();
    let step =
        super::privacy_ui::lock_next_step(&hx::Locked::Unavailable, hx::KeyringOs::current());
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(
        p.has(&msg) && p.has(&step),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    let retry = p.pill_by(&step, super::privacy_ui::TRY_AGAIN);
    assert!(
        (retry.y - p.at(&step).y).abs() < 24.0,
        "Try again sits on the next-step line"
    );

    // The keyring comes back, but its "down" answer is reused for 30 s, so
    // the page alone stays locked.
    store.set_available(true);
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(p.has(&msg), "the cached answer still holds");

    // Try again asks now, and the page refreshes: no lock, live Allow.
    click_at(
        &ctx,
        &mut cabin,
        p.pill_by(&step, super::privacy_ui::TRY_AGAIN),
    );
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(
        !p.has(&msg) && !p.has(&step),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    assert_eq!(
        cabin.status,
        "Your keyring answered. Private data is open again."
    );
    let (pills, _) = pill_opacity(&ctx, &mut cabin, vec![]);
    assert!(
        pills
            .iter()
            .filter(|p| p.0 == "Allow")
            .all(|p| (p.1 - 1.0).abs() < f32::EPSILON),
        "{pills:?}"
    );
    // It granted nothing on its own.
    assert_eq!(hx::ConsentLedger::load(&root).active().count(), 0);

    // Down again: Try again says it is still locked, and when it checked.
    store.set_available(false);
    cabin.retry_keyring();
    let p = paint_permissions(&ctx, &mut cabin, vec![]);
    assert!(p.has(&msg));
    assert!(
        p.has("Still locked. Checked just now."),
        "{:?}",
        p.texts.iter().map(|t| &t.0).collect::<Vec<_>>()
    );
    release_isolated(&root, cabin);
}

// SB-05: /privacy names each scope once: on ones as bullets with a since-time,
// the rest on one "Off:" line, all under one Grants heading.
#[test]
fn privacy_lists_each_scope_once_under_one_grants_heading() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, cabin) = isolated_cabin("privacy-once");
    std::fs::create_dir_all(&root).unwrap();
    let home = std::path::Path::new("/home/someone");
    for scope in [
        hx::Scope::Calendar,
        hx::Scope::Files("/home/someone/Documents/notes".into()),
    ] {
        hx::grant_scope(&root, &scope, Some(home), hx::UserClick::from_click()).unwrap();
    }
    let ledger = hx::ConsentLedger::load(&root);
    let at = ledger
        .active()
        .map(|g| g.granted_at)
        .max()
        .unwrap_or_default();
    let report = super::privacy_ui::privacy_report(
        &ledger,
        &hx::EgressRead::default(),
        None,
        false,
        at + 60_000,
    );
    let grants = report
        .split("\nGrants\n")
        .nth(1)
        .and_then(|r| r.split("\n\n").next())
        .expect("Grants");
    assert_eq!(
        grants,
        [
            "- Calendar: on since 1m ago",
            "- Files in notes: on since 1m ago",
            "- Off: Sync to paired computers (/sync asks each time) · Installed apps · Browser history · Mail · System state",
            "- Screen: \"Let Grok control the desktop\" in Settings → Cabin defaults (off)",
            "Granted folders, apps, browser history and system state are read on this computer only, never on battery or in quiet hours; calendar and mail aren't read yet.",
        ]
        .join("\n")
    );
    for word in [
        "Calendar",
        "Files in",
        "Installed apps",
        "Browser history",
        "Mail",
        "System state",
        "Sync to paired computers",
    ] {
        assert_eq!(report.matches(word).count(), 1, "{word} once: {report}");
    }
    assert_eq!(report.matches("Grants").count(), 1, "one heading: {report}");
    assert!(
        !report.contains(super::scope_ui::SCOPES_HEAD),
        "no second grants heading: {report}"
    );
    assert!(
        !report.contains("/home/someone/Documents"),
        "the short label, not the path: {report}"
    );
    release_isolated(&root, cabin);
}

// Landed from PR #124.
#[test]
fn room_binds_a_work_tree_and_rewind_files_needs_a_project() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("room");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home_s = home.display().to_string().trim_end_matches('/').to_string();
    let prev_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &home_s);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/rewind --files");
    assert_eq!(cabin.status, "Bind a project first — /project bind");
    assert!(!cabin.running);
    cabin.run_slash_line("/room harbor");
    let bound = format!("{home_s}/GrokHub-Work/harbor");
    assert_eq!(cabin.cfg.project_dir, bound);
    assert!(cabin.project_sel.is_some());
    assert_eq!(cabin.status, format!("Room harbor → {bound}"));
    assert!(!cabin.running);
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| m.0 == "user" && m.1.contains("blocked: outside bound project")),
        "room host script should stay inside the host rail: {:?}",
        cabin.messages
    );
    let dir = std::path::PathBuf::from(&bound);
    let start = std::time::Instant::now();
    while !dir.is_dir() && start.elapsed() < std::time::Duration::from_secs(2) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(dir.is_dir(), "room directory was not created");
    std::env::remove_var("GROKHUB_CONFIG");
    match prev_home {
        Some(h) => std::env::set_var("HOME", h),
        None => std::env::remove_var("HOME"),
    }
}

// Landed from PR #125.
#[test]
fn inhabit_refuses_a_missing_peer() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("inhabit");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/inhabit cabin-2");
    assert_eq!(cabin.status, "No paired peer named cabin-2");
    assert!(!cabin.running);
    assert!(cabin.inhabit_rx.is_none());
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #126.
#[test]
fn rewind_and_compact_stay_closed_without_grok() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("rewind-compact");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", "/tmp/grokhub-no-such-grok");
    let mut cabin = Cabin::quiet_for_test();
    cabin.live_mut().push(("user".into(), "harbor".into()));
    cabin.live_mut().push(("assistant".into(), "light".into()));
    cabin.run_slash_line("/rewind");
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| m.0 == "assistant" && m.1 == "light"),
        "rewind dropped the reply without a grok agent: {:?}",
        cabin.messages
    );
    assert!(!cabin.running);
    assert_eq!(
        cabin.status, "N/A on native threads: the session is append-only.",
        "rewind is not offered on native threads"
    );
    assert_ne!(cabin.status, "Rewinding Grok conversation…");
    cabin.live_mut().clear();
    for i in 0..9 {
        cabin.live_mut().push(("user".into(), format!("turn-{i}")));
    }
    cabin.run_slash_line("/compact");
    assert!(
        cabin.messages.iter().any(|m| m.1 == "turn-0"),
        "compact dropped history without a grok agent: {:?}",
        cabin.messages
    );
    assert_eq!(cabin.messages.len(), 9);
    assert!(!cabin.running);
    assert_ne!(cabin.status, "Compacting…", "no chat, nothing to compact");
    std::env::remove_var("GROKHUB_CONFIG");
    std::env::remove_var("GROKHUB_GROK");
}

// Landed from PR #127.
#[test]
fn project_bind_show_and_clear() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("project-bind");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home_s = home.display().to_string().trim_end_matches('/').to_string();
    let prev_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &home_s);
    let dock = format!("{}/dock", root.display().to_string().trim_end_matches('/'));
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/project");
    assert_eq!(cabin.status, "No bound project");
    cabin.run_slash_line(&format!("/project bind {dock}"));
    assert_eq!(cabin.cfg.project_dir, dock);
    assert!(cabin.project_sel.is_some());
    assert_eq!(cabin.status, format!("Bound {dock}"));
    assert!(!cabin.running);
    let dir = std::path::PathBuf::from(&dock);
    let start = std::time::Instant::now();
    while !dir.is_dir() && start.elapsed() < std::time::Duration::from_secs(2) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(dir.is_dir(), "bind did not create the project directory");
    cabin.run_slash_line("/project show");
    assert_eq!(cabin.status, format!("Project {dock}"));
    cabin.run_slash_line("/project clear");
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(cabin.project_sel.is_none());
    assert_eq!(cabin.status, "Unbound — full desktop");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
    match prev_home {
        Some(h) => std::env::set_var("HOME", h),
        None => std::env::remove_var("HOME"),
    }
}

// Landed from PR #128.
#[test]
fn models_and_inspect_without_grok() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("models-inspect");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", "/tmp/grokhub-no-such-grok");
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/models");
    assert!(
        cabin
            .messages
            .iter()
            .any(|m| { m.0 == "assistant" && m.1.contains("grok-4.7 — Grok 4.7 (chat)") }),
        "models catalog did not land: {:?}",
        cabin.messages
    );
    assert!(!cabin.running);
    cabin.run_slash_line("/inspect");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
    std::env::remove_var("GROKHUB_GROK");
}

// Landed from PR #129.
#[test]
fn permission_slashes_arm_always_and_set_auto() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("permission-slash");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert!(matches!(cabin.permission_mode, PermissionMode::Ask));
    cabin.run_slash_line("/always-approve");
    assert_eq!(cabin.status, "Confirm Always…");
    assert!(matches!(
        cabin.confirm,
        Some(super::confirm::ConfirmKind::AlwaysSession)
    ));
    assert!(matches!(cabin.permission_mode, PermissionMode::Ask));
    assert!(!cabin.running);
    cabin.apply_session_always();
    assert!(matches!(
        cabin.permission_mode,
        PermissionMode::AlwaysApprove
    ));
    assert_eq!(cabin.cfg.permission_mode, "ask");
    assert_eq!(cabin.status, "Permission always-approve");
    assert!(cabin.confirm.is_none());
    cabin.run_slash_line("/always-approve");
    assert!(matches!(cabin.permission_mode, PermissionMode::Ask));
    assert_eq!(cabin.cfg.permission_mode, "ask");
    assert_eq!(cabin.status, "Permission ask");
    cabin.run_slash_line("/auto");
    assert!(matches!(cabin.permission_mode, PermissionMode::Auto));
    assert_eq!(cabin.cfg.permission_mode, "auto");
    assert_eq!(cabin.status, "Permission auto");
    assert!(cabin.confirm.is_none());
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #130.
#[test]
fn learn_reflects_nothing_new_and_scratch_refuses() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("learn-reflect");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/learn");
    assert_eq!(cabin.status, "Reflecting…");
    assert!(cabin.reflect_rx.is_some());
    cabin.run_slash_line("/learn");
    assert_eq!(cabin.status, "Reflecting…");
    let start = std::time::Instant::now();
    while cabin.reflect_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(2) {
        cabin.poll_reflect();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(cabin.status, "Reflect: nothing new");
    assert!(cabin.reflect_rx.is_none());
    assert!(!cabin.running);
    cabin.run_slash_line("/scratch");
    cabin.run_slash_line("/learn");
    assert_eq!(cabin.status, "Scratch — no reflect");
    assert!(cabin.reflect_rx.is_none());
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #131.
#[test]
fn appearance_theme_tray_and_wall_save() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("appearance");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert_eq!(cabin.cfg.theme, "dark");
    assert!(cabin.cfg.close_to_tray);
    assert!(cabin.cfg.imagine_wall);
    cabin.choose_theme(grokhub_core::ThemeChoice::Light);
    assert_eq!(cabin.cfg.theme, "light");
    assert_eq!(cabin.status, "Saved");
    cabin.choose_theme(grokhub_core::ThemeChoice::Light);
    assert_eq!(cabin.cfg.theme, "light");
    cabin.choose_theme(grokhub_core::ThemeChoice::System);
    assert_eq!(cabin.cfg.theme, "system");
    assert_eq!(cabin.status, "Saved");
    cabin.set_close_to_tray(false);
    assert!(!cabin.cfg.close_to_tray);
    assert_eq!(cabin.status, "Saved");
    cabin.set_living_wall(false);
    assert!(!cabin.cfg.imagine_wall);
    assert_eq!(cabin.status, "Saved");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #132.
#[test]
fn missing_recipe_does_not_replay() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("missing-recipe");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.replay_saved_recipe("harbor"));
    assert_eq!(cabin.status, "No recipe harbor");
    assert!(cabin.recipe_desk_rx.is_none());
    assert!(!cabin.running);
    assert!(!cabin.replay_saved_recipe("last"));
    assert_eq!(cabin.status, "No recipe last");
    assert!(cabin.recipe_desk_rx.is_none());
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
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

// Landed from PR #134.
#[test]
fn composer_slash_runs_and_plain_text_refuses_without_grok() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("composer-send");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let missing = root.join("no-grok");
    let prev_grok = std::env::var_os("GROKHUB_GROK");
    std::env::set_var("GROKHUB_GROK", &missing);
    let mut cabin = Cabin::quiet_for_test();
    cabin.send_from_composer("/help".into());
    assert!(!cabin.running);
    assert!(
        cabin
            .messages
            .iter()
            .any(|(_, body)| body.contains("/clear")),
        "help lands on the open chat"
    );
    let before = cabin.messages.len();
    cabin.send_from_composer("hello harbor".into());
    assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
    assert!(!cabin.running);
    assert_eq!(cabin.messages.len(), before);
    assert!(cabin.messages.iter().all(|(role, _)| role != "user"));
    match prev_grok {
        Some(v) => std::env::set_var("GROKHUB_GROK", v),
        None => std::env::remove_var("GROKHUB_GROK"),
    }
}

// Landed from PR #135.
#[test]
fn board_add_move_archive_restore_and_link() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("board-acts");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    let thread_id = cabin.threads[cabin.thread_idx].id.clone();
    cabin.board_title = "Harbor lamp".into();
    cabin.board_notes = "fold the charts".into();
    assert!(cabin.apply_board_act(Some(BoardAct::Add)));
    assert_eq!(cabin.board.len(), 1);
    let id = cabin.board[0].id.clone();
    assert_eq!(cabin.board[0].title, "Harbor lamp");
    assert_eq!(cabin.board[0].detail, "fold the charts");
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert!(cabin.board_title.is_empty());
    assert!(!cabin.board_compose);
    assert!(cabin.board[0].thread_id.is_none());
    assert!(cabin.apply_board_act(Some(BoardAct::Move {
        id: id.clone(),
        status: grokhub_core::BoardStatus::InProgress,
    })));
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::InProgress);
    assert!(cabin.apply_board_act(Some(BoardAct::Archive(id.clone()))));
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Dismissed);
    assert!(cabin.apply_board_act(Some(BoardAct::Restore(id.clone()))));
    assert_eq!(cabin.board[0].status, grokhub_core::BoardStatus::Todo);
    assert!(cabin.apply_board_act(Some(BoardAct::Link(id.clone()))));
    assert_eq!(
        cabin.board[0].thread_id.as_deref(),
        Some(thread_id.as_str())
    );
    assert!(cabin.apply_board_act(Some(BoardAct::Unlink(id))));
    assert!(cabin.board[0].thread_id.is_none());
    assert!(!cabin.running);
}

#[test]
fn background_model_calls_run_at_low_effort() {
    assert_eq!(grokhub_core::BACKGROUND_EFFORT, "low");
    let src = cabin_src() + include_str!("feed_ui.rs") + include_str!("native_unattended.rs");
    // Thread titles use built-in topics_from_turns (no model). Remaining
    // background asks still pin BACKGROUND_EFFORT.
    let goal = fn_src(&src, "spawn_thread_goal_on");
    assert!(
        goal.contains("topics_from_turns") && !goal.contains("BACKGROUND_EFFORT"),
        "thread titles must be local, not a low-effort model ask: {goal}"
    );
    for name in ["fast_once", "spawn_native_review", "spawn_native_loop"] {
        let body = fn_src(&src, name);
        assert!(
            body.contains("BACKGROUND_EFFORT"),
            "{name} runs on its own and must use low effort: {body}"
        );
    }
    // Chips, the greeting, ideas and the feed digest go through `fast_once`.
    for name in [
        "spawn_native_chips",
        "spawn_native_greeting",
        "spawn_native_ideas",
        "spawn_native_digest",
    ] {
        let body = fn_src(&src, name);
        assert!(
            body.contains("fast_once") || body.contains("fast_text"),
            "{name}: {body}"
        );
    }
}

#[test]
fn workboard_delete_removes_the_card_and_keeps_its_chat() {
    use super::pages::BoardAct;
    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    let thread_id = cabin.threads[cabin.thread_idx].id.clone();
    cabin.board_title = "Harbor lamp".into();
    assert!(cabin.apply_board_act(Some(BoardAct::Add)));
    cabin.board_title = "Tide chart".into();
    assert!(cabin.apply_board_act(Some(BoardAct::Add)));
    let id = cabin.board[0].id.clone();
    assert!(cabin.apply_board_act(Some(BoardAct::Link(id.clone()))));
    cabin.board_view.open = Some(id.clone());
    cabin
        .board_view
        .composers
        .insert(id.clone(), "half a thought".into());
    // A turn running in the card's chat has to stop first.
    cabin.running = true;
    cabin.chat_job_thread = Some(thread_id.clone());
    assert!(!cabin.apply_board_act(Some(BoardAct::Delete(id.clone()))));
    assert_eq!(cabin.board.len(), 2);
    assert!(cabin
        .board_view
        .note
        .as_ref()
        .is_some_and(|(n, _)| n == &id));
    cabin.running = false;
    cabin.chat_job_thread = None;
    assert!(cabin.apply_board_act(Some(BoardAct::Delete(id.clone()))));
    assert_eq!(cabin.board.len(), 1);
    assert!(cabin.board.iter().all(|c| c.id != id));
    assert!(cabin.board_view.open.is_none());
    assert!(cabin.board_view.composers.is_empty() && cabin.board_view.note.is_none());
    assert!(
        cabin.threads.iter().any(|t| t.id == thread_id),
        "the chat stays in History"
    );
    assert!(
        !cabin.apply_board_act(Some(BoardAct::Delete(id))),
        "already gone"
    );
    let archived = cabin.board[0].id.clone();
    assert!(cabin.apply_board_act(Some(BoardAct::Archive(archived.clone()))));
    assert!(cabin.apply_board_act(Some(BoardAct::Delete(archived))));
    assert!(
        cabin.board.is_empty(),
        "an archived card can be deleted too"
    );
    let src = cabin_src();
    let menu = fn_src(&src, "board_card_menu");
    assert!(
        menu.contains("BoardAct::Delete") && menu.contains("menu_button(\"Delete\""),
        "Delete sits behind a second step in the card menu: {menu}"
    );
}

// Landed from PR #136.
#[test]
fn memory_switch_flushes_the_file_you_left() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("memory-switch");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = "harbor light".into();
    cabin.open_memory_file("SOUL.md");
    assert_eq!(cabin.mem_name, "SOUL.md");
    assert_eq!(cabin.mem_body, "");
    assert!(!cabin.running);
    // Read the file under this test's own root: the flush pins the directory it
    // was scheduled in, so the check must not depend on the process env later.
    let flushed = root.join("memory").join("MEMORY.md");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !std::fs::read_to_string(&flushed)
        .unwrap_or_default()
        .contains("harbor light")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "MEMORY.md was not flushed"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("memory").join("SOUL.md")).unwrap_or_default(),
        ""
    );
}

// Landed from PR #137.
#[test]
fn night_add_lands_a_daily_job_and_a_loop() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("night-add");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.add_automation_seed("every day at 9, summarize the board");
    assert_eq!(cabin.status, "Automation added · daily at 09:00");
    assert_eq!(cabin.automations.len(), 1);
    assert_eq!(cabin.automations[0].schedule, "daily");
    assert_eq!(cabin.automations[0].time, "09:00");
    assert!(cabin.grok_loops.is_empty());
    cabin.add_automation_seed("/loop 30m check deploy");
    assert_eq!(cabin.status, "Loop added · every 30m");
    assert_eq!(cabin.grok_loops.len(), 1);
    assert_eq!(cabin.grok_loops[0].interval, "30m");
    cabin.add_automation_seed("what is rust");
    assert_eq!(
        cabin.status,
        "Need `/loop 30m …`, `every 2h …`, or `every day at 9 …`"
    );
    assert_eq!(cabin.automations.len(), 1);
    assert_eq!(cabin.grok_loops.len(), 1);
    assert!(!cabin.running);
}

// Landed from PR #138.
#[test]
fn chip_nav_changes_page_and_dismiss_drops_it() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("chip-acts");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    let chips = vec![grokhub_core::QuickChip {
        id: "nav-ideas".into(),
        label: "Ideas".into(),
        value: "__nav:ideas".into(),
        kind: grokhub_core::ChipKind::Nav,
        score: 1.0,
        hint: String::new(),
        primary: false,
    }];
    cabin.take_chip_act(crate::cards::ChipRowAct::Apply(0), &chips);
    assert!(matches!(cabin.nav, Nav::Pulse));
    assert!(!cabin.running);
    cabin.take_chip_act(crate::cards::ChipRowAct::Dismiss(0), &chips);
    assert!(cabin.chip_dismissed.iter().any(|d| d == "nav-ideas"));
    assert!(cabin.chip_dismissed.iter().any(|d| d == "__nav:ideas"));
}

// Landed from PR #139.
#[test]
fn attach_sets_the_chip_and_paste_lands_in_the_composer() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("plus-attach");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.apply_plus_ready(
        grokhub_core::PlusTarget::Chat,
        PlusReady {
            kind: grokhub_core::AttachKind::Image,
            name: "harbor.png".into(),
            raw: "/tmp/harbor.png".into(),
            image_url: Some("data:image/png;base64,aGk=".into()),
            text: None,
        },
    );
    assert_eq!(cabin.attach_name.as_deref(), Some("harbor.png"));
    assert_eq!(
        cabin.attach_url.as_deref(),
        Some("data:image/png;base64,aGk=")
    );
    assert_eq!(
        cabin.status,
        "Attached harbor.png — sends with the next message"
    );
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());
    cabin.composer = "hello".into();
    cabin.apply_clipboard(grokhub_core::PlusTarget::Chat, "harbor light\n");
    assert_eq!(cabin.composer, "hello\nharbor light");
    assert_eq!(cabin.status, "Pasted clipboard");
    cabin.apply_plus_ready(
        grokhub_core::PlusTarget::Chat,
        PlusReady {
            kind: grokhub_core::AttachKind::Text,
            name: "note.txt".into(),
            raw: "/tmp/note.txt".into(),
            image_url: None,
            text: Some("fold the charts".into()),
        },
    );
    assert_eq!(cabin.composer, "hello\nharbor light");
    assert_eq!(cabin.attach_tray.len(), 2);
    assert_eq!(cabin.attach_name.as_deref(), Some("harbor.png"));
    assert_eq!(cabin.status, "File note.txt — the agent will read it");
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());
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

// Landed from PR #141.
#[test]
fn session_pills_save_chat_and_ask() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("session-pills");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.set_session_mode(SessionMode::Chat);
    assert_eq!(cabin.session_mode, SessionMode::Chat);
    assert_eq!(cabin.cfg.session_mode, "chat");
    cabin.set_session_mode(SessionMode::Ask);
    assert_eq!(cabin.session_mode, SessionMode::Ask);
    assert_eq!(cabin.cfg.session_mode, "ask");
    assert!(!cabin.running);
}

// Landed from PR #142.
#[test]
fn open_board_thread_opens_the_linked_chat() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("board-open");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.open_board_thread("missing");
    assert_eq!(cabin.status, "Linked chat is gone");
    assert!(cabin.threads.is_empty());
    assert!(!cabin.running);
    cabin.new_thread(false);
    let id = cabin.threads[cabin.thread_idx].id.clone();
    cabin.nav = Nav::Workboard;
    cabin.open_board_thread(&id);
    assert!(matches!(cabin.nav, Nav::Chat));
    assert_eq!(cabin.threads[cabin.thread_idx].id, id);
    assert!(!cabin.running);
}

// Landed from PR #143.
#[test]
fn store_session_plan_opens_once_and_keeps_the_first() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("session-plan");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.store_session_plan("   ", true);
    assert!(cabin.threads.is_empty());
    assert!(!cabin.plan_open);
    cabin.new_thread(false);
    cabin.store_session_plan("  fold the charts  ", false);
    assert!(cabin.plan_open);
    assert_eq!(cabin.threads[cabin.thread_idx].plan_body, "fold the charts");
    cabin.store_session_plan("other plan", false);
    assert_eq!(cabin.threads[cabin.thread_idx].plan_body, "fold the charts");
    cabin.store_session_plan("other plan", true);
    assert_eq!(cabin.threads[cabin.thread_idx].plan_body, "other plan");
    assert!(cabin.plan_open);
    assert!(!cabin.running);
}

// Landed from PR #144.
#[test]
fn board_edit_fills_the_form_and_save_writes_it() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("board-edit");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.apply_board_act(Some(BoardAct::Edit("missing".into()))));
    assert!(!cabin.board_compose);
    cabin.new_thread(false);
    let thread_id = cabin.threads[cabin.thread_idx].id.clone();
    cabin.board_title = "Harbor lamp".into();
    cabin.board_notes = "fold the charts".into();
    assert!(cabin.apply_board_act(Some(BoardAct::Add)));
    let id = cabin.board[0].id.clone();
    assert!(!cabin.apply_board_act(Some(BoardAct::Edit(id.clone()))));
    assert!(cabin.board_compose);
    assert_eq!(cabin.board_edit.as_deref(), Some(id.as_str()));
    assert_eq!(cabin.board_title, "Harbor lamp");
    assert_eq!(cabin.board_notes, "fold the charts");
    assert!(!cabin.board_link);
    cabin.board_title = "  Harbor dock  ".into();
    cabin.board_notes = "  tie the lines  ".into();
    cabin.board_link = true;
    assert!(cabin.apply_board_act(Some(BoardAct::Save(id))));
    assert_eq!(cabin.board[0].title, "Harbor dock");
    assert_eq!(cabin.board[0].detail, "tie the lines");
    assert_eq!(
        cabin.board[0].thread_id.as_deref(),
        Some(thread_id.as_str())
    );
    assert!(!cabin.board_compose);
    assert!(cabin.board_edit.is_none());
    assert!(cabin.board_title.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #145.
#[test]
fn bubble_reply_quotes_into_the_composer() {
    fn reply_like_the_button(cabin: &mut Cabin, body: &str) {
        cabin.composer = append_composer(&cabin.composer, &quote_for_reply(body));
        if !cabin.composer.ends_with('\n') {
            cabin.composer.push('\n');
        }
        cabin.composer_want_focus = true;
    }

    let mut plain = Cabin::quiet_for_test();
    plain.composer = "notes for later".into();
    assert!(!plain.running);
    assert!(!plain.composer_want_focus);
    reply_like_the_button(&mut plain, "Ship the harbor");
    assert_eq!(plain.composer, "notes for later\n> Ship the harbor\n");
    assert!(plain.composer.starts_with("notes for later\n"));
    assert!(plain.composer_want_focus);
    assert!(!plain.running);

    let mut marked = Cabin::quiet_for_test();
    marked.composer = "notes for later".into();
    assert!(!marked.running);
    assert!(!marked.composer_want_focus);
    reply_like_the_button(&mut marked, "> already quoted");
    assert_eq!(marked.composer, "notes for later\n> > already quoted\n");
    assert!(marked.composer.starts_with("notes for later\n"));
    assert!(marked.composer_want_focus);
    assert!(!marked.running);
}

// Landed from PR #146.
#[test]
fn grok_history_open_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("grok-hist");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let id = "sess-quiet-known";
    let title = "Known cabin title";
    let mut cabin = super::Cabin::quiet_for_test();
    cabin
        .grok_sessions
        .push(grokhub_core::cli_history::GrokSession {
            id: id.to_string(),
            title: title.to_string(),
            path: None,
            cwd: None,
            cabin: false,
        });

    cabin.open_grok_session(id);
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].grok_session.as_deref(), Some(id));
    assert_eq!(cabin.threads[0].title, title);
    assert!(cabin.nav == super::Nav::Chat);
    assert_eq!(cabin.status, format!("Opened {title}"));
    assert!(!cabin.running);

    cabin.open_grok_session(id);
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].grok_session.as_deref(), Some(id));
    assert!(!cabin.running);
}

// Landed from PR #147.
#[test]
fn grok_tasks_and_commands_stay_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("grok-tasks");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.apply_grok_task("task-1".into(), "Draft the note".into(), false);
    assert_eq!(
        cabin.grok_tasks,
        vec![("task-1".into(), "Draft the note".into(), false)]
    );
    assert!(!cabin.running);

    cabin.apply_grok_task("task-1".into(), "Note is filed".into(), true);
    assert_eq!(
        cabin.grok_tasks,
        vec![("task-1".into(), "Note is filed".into(), true)]
    );
    assert!(!cabin.running);

    let owned = grokhub_core::filter_slash_commands("/")
        .first()
        .expect("cabin slash")
        .cmd
        .to_string();
    let foreign = "create-skill".to_string();
    let names = vec![foreign.clone(), "   ".into(), String::new(), owned.clone()];
    let hits = grokhub_core::grok_command_hits(&names);
    cabin.apply_grok_commands(names);
    assert_eq!(cabin.grok_commands, hits);
    assert!(
        cabin
            .grok_commands
            .iter()
            .any(|h| h.cmd == format!("/{foreign}")),
        "a name the cabin does not own stays"
    );
    assert!(
        cabin.grok_commands.iter().all(|h| {
            let name = h.cmd.trim_start_matches('/').trim();
            !name.is_empty() && h.cmd != owned
        }),
        "a blank name and a cabin-owned slash are dropped"
    );
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #148.
#[test]
fn chip_apply_mode_and_help_stay_off_a_run() {
    struct RestoreEnv148 {
        config: Option<std::ffi::OsString>,
        grok: Option<std::ffi::OsString>,
    }
    impl Drop for RestoreEnv148 {
        fn drop(&mut self) {
            match self.config.take() {
                Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
            match self.grok.take() {
                Some(v) => std::env::set_var("GROKHUB_GROK", v),
                None => std::env::remove_var("GROKHUB_GROK"),
            }
        }
    }

    fn chip(id: &str, value: &str, kind: super::ChipKind) -> super::QuickChip {
        super::QuickChip {
            id: id.into(),
            label: id.into(),
            value: value.into(),
            kind,
            score: 1.0,
            hint: String::new(),
            primary: false,
        }
    }

    fn settle(cabin: &super::Cabin) {
        let _io = cabin.persist_io.lock().unwrap_or_else(|e| e.into_inner());
    }

    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("chip-apply");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    let _restore = RestoreEnv148 {
        config: std::env::var_os("GROKHUB_CONFIG"),
        grok: std::env::var_os("GROKHUB_GROK"),
    };
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", "/no/such/grok-binary-xyz");

    let mut mode = super::Cabin::quiet_for_test();
    mode.apply_chip(chip("mode-fast", "__mode:fast", super::ChipKind::Mode));
    assert_eq!(mode.cfg.mode, "fast");
    assert_eq!(
        mode.status,
        super::mode_status_line("fast", &mode.cfg.model)
    );
    assert!(!mode.running);

    let mut typed_help = super::Cabin::quiet_for_test();
    typed_help.send_from_composer("/help".into());
    let mut help = super::Cabin::quiet_for_test();
    help.apply_chip(chip("help", "/help", super::ChipKind::Chat));
    assert_eq!(help.status, typed_help.status);
    assert!(!help.running);
    assert!(!typed_help.running);
    let help_body = super::mark_slash_result(&super::slash_help());
    assert_eq!(
        help.messages
            .last()
            .map(|(role, text)| (role.as_str(), text.as_str())),
        Some(("assistant", help_body.as_str()))
    );
    assert_eq!(help.messages.last(), typed_help.messages.last());

    let sentence = "Tell me about the harbor.";
    let mut typed_chat = super::Cabin::quiet_for_test();
    typed_chat.send_from_composer(sentence.into());
    let mut chat = super::Cabin::quiet_for_test();
    chat.apply_chip(chip("say", sentence, super::ChipKind::Chat));
    assert_eq!(
        chat.status, typed_chat.status,
        "a plain chip must refuse with the composer install/connect status"
    );
    assert_eq!(chat.status, grokhub_core::XAI_NEED_SIGNIN);
    assert!(!chat.running);
    assert!(!typed_chat.running);
    assert!(chat.messages.is_empty());
    assert!(typed_chat.messages.is_empty());

    settle(&mode);
    settle(&typed_help);
    settle(&help);
    settle(&typed_chat);
    settle(&chat);
}

// Landed from PR #150.
#[test]
fn set_nav_id_opens_each_page() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("set-nav");
    let _ = std::fs::create_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(matches!(cabin.nav, super::Nav::Chat));

    cabin.set_nav_id("settings");
    assert!(matches!(cabin.nav, super::Nav::Settings));
    assert!(matches!(cabin.settings_back, super::Nav::Chat));
    assert!(matches!(cabin.settings_sec, super::SettingsSec::Account));
    assert!(!cabin.running);
    cabin.set_nav_id("settings");
    assert!(matches!(cabin.nav, super::Nav::Settings));
    assert!(matches!(cabin.settings_back, super::Nav::Chat));
    assert!(matches!(cabin.settings_sec, super::SettingsSec::Account));
    assert!(!cabin.running);

    cabin.set_nav_id("history");
    assert!(matches!(cabin.nav, super::Nav::History));
    assert!(!cabin.running);

    cabin.set_nav_id("journal");
    assert!(matches!(cabin.nav, super::Nav::Journal));
    assert!(!cabin.running);

    cabin.set_nav_id("imagine");
    assert!(matches!(cabin.nav, super::Nav::Imagine));
    assert!(cabin.imagine_want_focus);
    assert!(!cabin.running);

    cabin.set_nav_id("workboard");
    assert!(matches!(cabin.nav, super::Nav::Workboard));
    assert!(!cabin.running);

    cabin.set_nav_id("skills");
    assert!(matches!(cabin.nav, super::Nav::Skills));
    assert!(!cabin.running);

    cabin.set_nav_id("automations");
    assert!(matches!(cabin.nav, super::Nav::Night));
    assert!(!cabin.running);

    cabin.set_nav_id("command");
    assert!(matches!(cabin.nav, super::Nav::Command));
    assert!(!cabin.running);

    cabin.set_nav_id("queue");
    assert!(matches!(cabin.nav, super::Nav::Agents));
    assert!(!cabin.running);

    cabin.set_nav_id("devices");
    assert!(matches!(cabin.nav, super::Nav::Devices));
    assert!(!cabin.running);

    cabin.set_nav_id("memory");
    assert!(matches!(cabin.nav, super::Nav::Memory));
    assert!(!cabin.running);

    cabin.set_nav_id("connectors");
    assert!(matches!(cabin.nav, super::Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    assert!(!cabin.running);

    let threads_before = cabin.threads.len();
    let idx_before = cabin.thread_idx;
    cabin.composer_want_focus = false;
    cabin.chat_tail_frames = 0;
    cabin.set_nav_id("eyes");
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert_eq!(cabin.threads.len(), threads_before);
    assert_eq!(cabin.thread_idx, idx_before);
    assert!(cabin.composer_want_focus);
    assert_eq!(cabin.chat_tail_frames, grokhub_core::CHAT_TAIL_FRAMES);
    assert!(!cabin.running);

    let threads_before = cabin.threads.len();
    let status_before = cabin.status.clone();
    cabin.set_nav_id("not-a-page");
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert_eq!(cabin.threads.len(), threads_before);
    assert_eq!(cabin.status, status_before);
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);

    let draft = crate::threads::ChatThread::new("Chat", false);
    let id = draft.id.clone();
    cabin.threads.push(draft);
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(Vec::new());
    let n = cabin.threads.len();
    let io = cabin.persist_io.clone();
    let block = io.lock().unwrap_or_else(|e| e.into_inner());
    cabin.set_nav_id("chat");
    drop(block);
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert_eq!(cabin.threads.len(), n);
    assert_eq!(cabin.threads[0].id, id);
    assert!(cabin.messages.is_empty());
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);

    std::thread::sleep(std::time::Duration::from_millis(100));
    drop(io.lock().unwrap_or_else(|e| e.into_inner()));
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #151.
#[test]
fn project_menu_rename_move_and_delete() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("proj-menu");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    let work = root.join("work");
    create_folder(&mut cabin.projects, "fold-menu", "Notes", None).expect("folder");
    create_project(
        &mut cabin.projects,
        "proj-menu",
        "Harbor",
        Some("fold-menu"),
        &work.display().to_string(),
    )
    .expect("project");
    let name = cabin
        .projects
        .iter()
        .find(|n| n.id == "proj-menu")
        .expect("seeded project")
        .name
        .clone();
    assert_eq!(
        cabin
            .projects
            .iter()
            .find(|n| n.id == "proj-menu")
            .and_then(|n| n.parent.clone())
            .as_deref(),
        Some("fold-menu")
    );

    let io = cabin.persist_io.clone();
    let _held = io.lock().unwrap_or_else(|e| e.into_inner());

    cabin.apply_project_menu("proj-menu".into(), ProjectMenuAct::Rename);
    assert_eq!(cabin.proj_rename.as_deref(), Some("proj-menu"));
    assert_eq!(cabin.proj_rename_buf, name);
    assert!(!cabin.running);

    cabin.apply_project_menu("proj-menu".into(), ProjectMenuAct::AddToFolder);
    assert_eq!(cabin.proj_add_for.as_deref(), Some("proj-menu"));
    assert_eq!(cabin.project_sel.as_deref(), Some("proj-menu"));
    assert!(cabin.proj_ignore_close);
    assert!(!cabin.running);

    cabin.apply_project_menu("proj-menu".into(), ProjectMenuAct::RemoveFromFolder);
    assert!(cabin
        .projects
        .iter()
        .find(|n| n.id == "proj-menu")
        .and_then(|n| n.parent.as_ref())
        .is_none());
    assert_eq!(cabin.status, "Moved to Projects");
    assert!(!cabin.running);

    cabin.apply_project_menu("proj-menu".into(), ProjectMenuAct::Delete);
    assert!(cabin.projects.iter().all(|n| n.id != "proj-menu"));
    assert!(!cabin.running);

    let projects = cabin.projects.clone();
    let status = cabin.status.clone();
    let proj_rename = cabin.proj_rename.clone();
    let proj_rename_buf = cabin.proj_rename_buf.clone();
    let running = cabin.running;
    cabin.apply_project_menu("missing-proj".into(), ProjectMenuAct::Rename);
    assert_eq!(cabin.projects, projects);
    assert_eq!(cabin.status, status);
    assert_eq!(cabin.proj_rename, proj_rename);
    assert_eq!(cabin.proj_rename_buf, proj_rename_buf);
    assert_eq!(cabin.running, running);
    assert!(!cabin.running);

    std::thread::sleep(std::time::Duration::from_millis(150));
    drop(_held);
    std::thread::sleep(std::time::Duration::from_millis(150));
    let _done = io.lock().unwrap_or_else(|e| e.into_inner());
    drop(_done);
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #152.
#[test]
fn followup_queue_drains_one_and_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("followup-queue");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", root.join("missing-grok"));

    let mut composer = Cabin::quiet_for_test();
    assert!(!composer.running);
    composer.send_chat("/help".into());
    let help_status = composer.status.clone();

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.drain_followup_queue();
    assert!(cabin.followup_queue.is_empty());
    assert!(!cabin.running);

    let second = "second line stays queued".to_string();
    cabin.followup_queue = vec!["/help".into(), second.clone()];
    cabin.drain_followup_queue();
    assert_eq!(cabin.followup_queue, vec![second]);
    assert_eq!(cabin.status, help_status);
    assert_eq!(cabin.messages.as_ref(), composer.messages.as_ref());
    assert!(!cabin.running);

    cabin.followup_queue = vec!["/approve".into()];
    cabin.drain_followup_queue();
    assert_eq!(cabin.status, "Unknown command — /help");
    assert!(!cabin.running);

    let sentence = "plain sentence with no grok".to_string();
    cabin.followup_queue = vec![sentence.clone()];
    cabin.drain_followup_queue();
    assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
    assert!(
        cabin
            .messages
            .iter()
            .all(|(_, body)| !body.contains(&sentence)),
        "the plain sentence must stay off the chat"
    );
    assert!(!cabin.running);

    std::env::remove_var("GROKHUB_GROK");
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn workflow_slashes_say_workflows_are_gone_and_never_touch_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("workflow-usage");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    for with_thread in [false, true] {
        let mut cabin = Cabin::quiet_for_test();
        if with_thread {
            cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
            cabin.thread_idx = 0;
        }
        cabin.running = true;
        for line in [
            "/workflow pause",
            "/workflow pause review-2",
            "/workflow review",
            "/workflow",
            "/workflows",
        ] {
            cabin.status.clear();
            cabin.run_slash_line(line);
            assert_eq!(
                cabin.status, "Workflows were a Grok Build CLI feature and are not in GrokHub",
                "{line} (thread: {with_thread})"
            );
            assert!(cabin.running, "{line} must not touch the live turn");
            assert!(matches!(cabin.nav, Nav::Chat), "{line}");
        }
    }

    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/mcps");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    cabin.run_slash_line("/plugins");
    assert!(matches!(cabin.nav, Nav::Settings));
    cabin.run_slash_line("/marketplace");
    assert!(matches!(cabin.nav, Nav::Settings));
    cabin.run_slash_line("/connectors");
    assert!(matches!(cabin.nav, Nav::Settings));
    cabin.run_slash_line("/hooks");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.running);

    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn hooks_slash_opens_connectors_on_the_hooks_section() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("hooks-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_GROK", root.join("missing-grok"));

    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/mcps");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    assert!(!cabin.scroll_to_hooks);
    cabin.run_slash_line("/plugins");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.scroll_to_hooks);
    cabin.run_slash_line("/marketplace");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.scroll_to_hooks);
    cabin.run_slash_line("/connectors");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.scroll_to_hooks);
    cabin.run_slash_line("/hooks");
    assert!(matches!(cabin.nav, Nav::Settings));
    assert_eq!(cabin.settings_sec, SettingsSec::Connectors);
    assert!(cabin.scroll_to_hooks);

    std::env::remove_var("GROKHUB_GROK");
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn connectors_and_hooks_do_not_write_grok_home() {
    let src = cabin_src();
    let mut blob = String::new();
    for name in ["ui_hooks_section", "ui_settings_connectors"] {
        blob.push_str(fn_src(&src, name));
        blob.push('\n');
    }
    for forbidden in [
        "config.toml",
        "mcp_credentials.json",
        "trusted_folders",
        "fs::write",
        "File::create",
        "OpenOptions",
        "grok_user_stdout",
    ] {
        assert!(
            !blob.contains(forbidden),
            "{forbidden} must not appear in the connectors code"
        );
    }
    let hooks = fn_src(&src, "ui_hooks_section");
    assert!(hooks.contains("No hooks in ~/.grok/hooks or this project's .grok/hooks."));
    assert!(hooks.contains("set_folder_trust"), "{hooks}");
    assert!(hooks.contains("scroll_to_me") && hooks.contains("scroll_to_hooks"));
    assert!(hooks.contains("Align::TOP"));
    assert!(!hooks.contains("ghost_pill") && !hooks.contains("white_pill"));
}

// Landed from PR #153.
#[test]
fn thinking_status_shows_context_when_usage_is_present() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("thinking-status");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert_eq!(cabin.thinking_status(), "Thinking…");

    let usage = GrokUsage {
        context_tokens_used: 26000,
        context_window_tokens: 500000,
        ..GrokUsage::default()
    };
    cabin.grok_usage = usage.clone();
    let line = grok_context_line(&usage);
    assert_eq!(cabin.thinking_status(), format!("Thinking… {line}"));
    assert!(!cabin.running);
}

// Landed from PR #156.
#[test]
fn shell_chip_stays_off_a_run_when_host_is_blocked() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("shell-chip");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let project = root.join("proj");
    std::fs::create_dir_all(&project).expect("project");
    let project_dir = project.to_string_lossy().into_owned();
    let cmd = "cat /etc/passwd".to_string();
    assert!(
        grokhub_core::host_cmd_leaves_project(&cmd, &project_dir),
        "cat /etc/passwd must leave the bound project"
    );

    let chip = grokhub_core::QuickChip {
        id: "shell".into(),
        label: "Shell".into(),
        value: cmd,
        kind: grokhub_core::ChipKind::Shell,
        score: 1.0,
        hint: String::new(),
        primary: false,
    };

    let mut off = super::Cabin::quiet_for_test();
    off.cfg.host_on = false;
    off.cfg.project_dir = project_dir.clone();
    off.permission_mode = grokhub_core::wire::PermissionMode::Ask;
    off.running = false;
    off.apply_chip(chip.clone());
    assert_eq!(off.status, "Host off — /host on");
    assert!(!off.running);

    let mut ask = super::Cabin::quiet_for_test();
    ask.cfg.host_on = true;
    ask.cfg.project_dir = project_dir;
    ask.permission_mode = grokhub_core::wire::PermissionMode::Ask;
    ask.running = false;
    ask.apply_chip(chip);
    assert!(
        !ask.running,
        "running became true; the command was inside the project"
    );
    let chat = ask
        .messages
        .iter()
        .map(|(_, body)| body.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        chat.contains("blocked: outside bound project"),
        "chat missing outside-project block: {chat}"
    );
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

// Landed from PR #158.
#[test]
fn composer_action_chips_stay_off_a_run() {
    let _hold = config::hold_test_config();
    let root = config::test_config_root("composer-action-chips");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(!cabin.running);

    const SENT: &str = grokhub_core::XAI_NEED_SIGNIN;

    fn click(cabin: &mut Cabin, label: &str) {
        cabin.refresh_chips();
        let chips = cabin.composer_chips();
        let chip = chips
            .iter()
            .find(|c| c.label == label)
            .cloned()
            .unwrap_or_else(|| {
                let labels: Vec<_> = chips.iter().map(|c| c.label.as_str()).collect();
                panic!("missing {label} among {labels:?}");
            });
        assert!(matches!(chip.kind, ChipKind::Chat), "{label}");
        let before = cabin.messages.len();
        let value = chip.value.clone();
        cabin.apply_chip(chip);
        assert!(!cabin.running, "{label}");
        assert!(matches!(cabin.nav, Nav::Chat));
        assert_eq!(cabin.messages.len(), before, "{label} transcript");
        assert!(
            cabin.messages.iter().all(|(_, line)| line != SENT),
            "{label} transcript gained the status line"
        );
        if cabin.composer.is_empty() {
            assert_eq!(cabin.status, SENT, "{label}");
        } else {
            assert_eq!(cabin.composer, value, "{label}");
        }
    }

    cabin.composer = "paint the north wall".into();
    click(&mut cabin, "Expand & send");

    cabin.messages = Arc::new(vec![
        ("user".into(), "We started the cabin wall.".into()),
        (
            "assistant".into(),
            "The north wall has one coat already.".into(),
        ),
    ]);
    cabin.composer.clear();
    cabin.chip_paint_key.clear();
    click(&mut cabin, "Continue");

    cabin.messages = Arc::new(vec![
        ("user".into(), "Look at the cabin logs.".into()),
        (
            "assistant".into(),
            "I'll check the cabin logs and then run a short probe.".into(),
        ),
    ]);
    cabin.composer.clear();
    cabin.chip_paint_key.clear();
    click(&mut cabin, "Finish — run tools now");
    click(&mut cabin, "Finish the job");
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

// Landed from PR #160.
#[test]
fn worktree_stays_off_a_send() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("worktree-send");
    let _ = std::fs::create_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.new_thread(false);
    let seeded = cabin
        .threads
        .get_mut(cabin.thread_idx)
        .expect("quiet cabin needs one thread before worktree");
    seeded.grok_session = Some("sess".into());

    cabin.run_slash(super::Slash::Worktree);
    assert_eq!(
        cabin.status,
        "Worktrees were a Grok Build CLI feature; chats run in the bound project folder"
    );
    assert!(!cabin.running);
    assert_eq!(
        cabin.threads[cabin.thread_idx].grok_session.as_deref(),
        Some("sess"),
        "/worktree leaves the chat's session alone"
    );
}

// Landed from PR #161.
#[test]
fn model_slash_saves_without_a_run() {
    let _hold = config::hold_test_config();
    let root = config::test_config_root("model-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.run_slash(Slash::Model("grok-4.7".into()));

    assert_eq!(cabin.cfg.model, "grok-4.7");
    assert_eq!(cabin.status, "grok --model grok-4.7");
    assert!(!cabin.running);

    let path = root.join("app.json");
    let started = Instant::now();
    let mut body = String::new();
    while started.elapsed() < Duration::from_secs(3) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            body = text;
            if body.contains("\"model\": \"grok-4.7\"") {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        path.starts_with(&root),
        "model file must stay under the test config root"
    );
    assert!(
        body.contains("\"model\": \"grok-4.7\""),
        "model was not persisted under {}: {body}",
        path.display()
    );
    assert_eq!(config::load().model, "grok-4.7");

    std::env::remove_var("GROKHUB_CONFIG");
    let _ = std::fs::remove_dir_all(&root);
}

// Landed from PR #162.
fn collect_shape_text(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_shape_text(shape, out);
            }
        }
        _ => {}
    }
}

#[test]
fn empty_queue_keeps_nothing_queued_label() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("empty-queue");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.followup_queue.is_empty());

    let chips = cabin.composer_chips();
    assert!(
        chips.is_empty(),
        "a quiet cabin with an empty follow-up queue has an empty chip row"
    );

    let mut texts = Vec::new();
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            assert!(crate::cards::quick_chip_row(ui, &chips).is_none());
            let layer = ui.layer_id();
            ui.ctx().graphics(|layers| {
                if let Some(list) = layers.get(layer) {
                    for clipped in list.all_entries() {
                        collect_shape_text(&clipped.shape, &mut texts);
                    }
                }
            });
        });
    });

    assert_eq!(crate::cards::CHIP_EMPTY_LABEL, "Nothing queued");
    assert_eq!(
        texts,
        vec![crate::cards::CHIP_EMPTY_LABEL.to_string()],
        "an empty follow-up queue shows exactly the empty-queue label"
    );
    assert!(!cabin.running);
    assert!(cabin.followup_queue.is_empty());

    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #163.
#[test]
fn hub_and_memory_show_stay_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("hub-memory");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.run_slash(Slash::Hub);
    assert!(matches!(cabin.nav, Nav::Devices));
    assert_eq!(cabin.status, "Start share on Devices");
    assert!(!cabin.hub_on);
    assert!(!cabin.running);
    let sharing = cabin.hub.lock().expect("hub").sharing;
    assert!(!sharing);

    cabin.hub_on = true;
    cabin.run_slash(Slash::Hub);
    assert_eq!(cabin.status, "Hub sharing");
    assert!(!cabin.running);
    let sharing = cabin.hub.lock().expect("hub").sharing;
    assert!(!sharing);

    cabin.run_slash(Slash::MemoryShow);
    assert!(matches!(cabin.nav, Nav::Memory));
    assert_eq!(cabin.status, "Memory");
    assert!(!cabin.running);

    cabin
        .threads
        .push(crate::threads::ChatThread::new("Scratch", true));
    cabin.thread_idx = cabin.threads.len() - 1;
    assert!(cabin.scratch());
    cabin.mem_body = "sentinel-memory".into();
    let mem_body = cabin.mem_body.clone();
    cabin.run_slash(Slash::Forget(None));
    assert_eq!(cabin.status, "Scratch — no memory writes");
    assert_eq!(cabin.mem_body, mem_body);
    assert!(!cabin.running);
}

// Landed from PR #164.
#[test]
fn palette_opens_and_diagnostics_stay_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("palette-diag");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.open_palette();
    assert!(cabin.palette_open);
    assert!(cabin.palette_focus);
    assert!(cabin.palette_q.is_empty());
    assert!(cabin.palette_file_rx.is_none());
    assert!(!cabin.settings_menu_open);
    assert!(!cabin.running);

    // Copy diagnostics puts the bundle on the clipboard, not in the status line.
    let ctx = egui::Context::default();
    let out = crate::theme::test_pass(&ctx, Default::default(), |_| {
        cabin.run_palette(&ctx, "diag");
    });
    assert!(!cabin.palette_open);
    assert_eq!(cabin.status, "Diagnostics copied");
    let copied: Vec<&String> = out
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(copied.len(), 1, "{copied:?}");
    assert!(
        copied[0].starts_with("app GrokHub\nversion "),
        "{}",
        copied[0]
    );
    assert!(
        copied[0].contains(env!("CARGO_PKG_VERSION")),
        "{}",
        copied[0]
    );
    assert!(!cabin.running);

    cabin.run_palette(&egui::Context::default(), "nav:night");
    assert!(matches!(cabin.nav, Nav::Night));
    assert!(!cabin.palette_open);
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #165.
fn grok_process_ids() -> Vec<u32> {
    let mut ids = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return ids;
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(pid_str) = name.to_str() else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let comm_is_grok = comm.trim() == "grok";
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let argv0 = cmdline.split(|b| *b == 0).next().unwrap_or(&[]);
        let exe_is_grok = std::path::Path::new(std::str::from_utf8(argv0).unwrap_or(""))
            .file_name()
            .and_then(|s| s.to_str())
            == Some("grok");
        if comm_is_grok || exe_is_grok {
            ids.push(pid);
        }
    }
    ids.sort_unstable();
    ids
}

#[test]
fn help_slash_lists_commands_without_a_run() {
    struct RestoreConfig(Option<String>);
    impl Drop for RestoreConfig {
        fn drop(&mut self) {
            match self.0.take() {
                Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
        }
    }

    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("help-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    let _restore = RestoreConfig(std::env::var("GROKHUB_CONFIG").ok());
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    assert!(!cabin.running, "help starts from an idle cabin");
    let before = cabin.messages.len();
    let grok_before = grok_process_ids();
    cabin.run_slash(Slash::Help);
    let gained: Vec<&(String, String)> = cabin.messages.iter().skip(before).collect();
    assert!(
        matches!(
            gained.as_slice(),
            [(role, text)] if role == "assistant" && text.contains("/help — this list")
        ),
        "transcript must gain one assistant line containing /help — this list, got {gained:?}"
    );
    assert!(!cabin.running, "help must not start a run");
    assert_eq!(
        grok_before,
        grok_process_ids(),
        "help must not start a grok process"
    );
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

// Landed from PR #167.
#[test]
fn unknown_slash_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("unknown-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }

    let status = cabin.status.clone();
    let transcript = cabin.messages.clone();
    let running = cabin.running;

    cabin.send_chat(String::new());
    assert_eq!(cabin.status, status);
    assert_eq!(&*cabin.messages, &*transcript);
    assert_eq!(cabin.running, running);

    cabin.send_chat("/not-a-command".to_string());
    assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
    assert_eq!(&*cabin.messages, &*transcript);
    assert!(!cabin.running);
    let transcript = cabin.messages.clone();

    cabin.send_chat("/approve".to_string());
    assert_eq!(cabin.status, "Unknown command — /help");
    assert_eq!(&*cabin.messages, &*transcript);
    assert!(!cabin.running);
}

// Landed from PR #168.
#[test]
fn slash_pick_fills_or_returns_the_command() {
    let mut composer = String::new();
    let run = super::slash_pick_take(&mut composer, "/help", true);
    assert_eq!(run.as_deref(), Some("/help"));
    assert!(composer.is_empty());

    let mut composer = String::new();
    let run = super::slash_pick_take(&mut composer, "/model ", false);
    assert!(run.is_none());
    assert_eq!(composer, "/model ");

    assert_eq!(super::slash_pick_step(0, 3, 1), 1);
    assert_eq!(super::slash_pick_step(2, 3, 1), 2);
    assert_eq!(super::slash_pick_step(0, 3, -1), 0);
    assert_eq!(super::slash_pick_step(0, 0, 1), 0);

    assert_eq!(super::slash_pick_retain(2, true, 5), 0);
    assert_eq!(super::slash_pick_retain(2, false, 5), 2);
}

// Landed from PR #169.
#[test]
fn settings_section_titles_match_the_page() {
    assert_eq!(
        super::settings::settings_sec_title(super::SettingsSec::Account),
        "Account"
    );
    assert_eq!(
        super::settings::settings_sec_title(super::SettingsSec::Appearance),
        "Appearance"
    );
    assert_eq!(
        super::settings::settings_sec_title(super::SettingsSec::Behavior),
        "Behavior"
    );
    assert_eq!(
        super::settings::settings_sec_title(super::SettingsSec::Update),
        "Update"
    );
    assert_eq!(
        super::settings::settings_sec_title(super::SettingsSec::About),
        "About"
    );
    assert!(matches!(
        super::settings::settings_group_home(super::SettingsGroup::General),
        super::SettingsSec::Account
    ));
    assert!(matches!(
        super::settings::settings_group_home(super::SettingsGroup::About),
        super::SettingsSec::Update
    ));
}

// Landed from PR #170.
#[test]
fn a_turn_that_cannot_start_stays_off_a_run() {
    struct RestoreConfig170(Option<String>);
    impl Drop for RestoreConfig170 {
        fn drop(&mut self) {
            match self.0.take() {
                Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
        }
    }

    let _cfg = crate::config::hold_test_config();
    let _restore = RestoreConfig170(std::env::var("GROKHUB_CONFIG").ok());
    let root = crate::config::test_config_root("ask-denied");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    for detail in ["native engine is not running", "timeout"] {
        cabin.running = true;
        cabin.pending_kick = Some(true);
        cabin.chat_job_thread = Some("job".into());
        cabin.fail_turn_start(detail);
        assert!(cabin.status.contains(detail), "{detail}: {}", cabin.status);
        assert!(!cabin.running);
        assert!(cabin.pending_kick.is_none());
        assert!(cabin.chat_job_thread.is_none());
    }
}

// Landed from PR #171.
#[test]
fn history_hit_opens_memory_or_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("history-hit");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.open_history_hit("mem:MEMORY.md");
    assert!(matches!(cabin.nav, Nav::Memory));
    assert_eq!(cabin.status, "MEMORY.md");
    assert_eq!(cabin.mem_name, "MEMORY.md");
    assert!(!cabin.running);

    cabin.open_history_hit("thread:nope");
    assert_eq!(cabin.status, "That chat is gone");
    assert!(!cabin.running);

    cabin.new_thread(false);
    cabin.new_thread(false);
    let id = cabin
        .threads
        .get(cabin.thread_idx)
        .map(|t| t.id.clone())
        .expect("seeded thread");
    cabin.open_history_hit(&format!("thread:{id}"));
    assert_eq!(cabin.threads[cabin.thread_idx].id, id);
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(!cabin.running);
}

// Landed from PR #172.
#[test]
fn delete_all_history_leaves_one_empty_chat() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("delete-all-history");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.new_thread(false);
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "note".into())]);
    cabin.new_thread(false);
    cabin.cfg.goal_pin = "harbor".into();
    let seeded = cabin
        .threads
        .iter()
        .position(|t| !t.messages.is_empty())
        .expect("seeded message");
    cabin.threads[seeded].grok_session = Some("sess".into());
    assert!(cabin.threads.len() >= 2);
    assert!(cabin
        .threads
        .iter()
        .any(|t| t.grok_session.as_deref() == Some("sess")));
    assert_eq!(cabin.cfg.goal_pin, "harbor");
    assert!(!cabin.running);

    cabin.delete_all_history();

    assert_eq!(cabin.status, "Deleted all chats");
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].title, "Chat");
    assert!(cabin.threads[0].messages.is_empty());
    assert!(cabin.messages.is_empty());
    assert!(cabin.cfg.goal_pin.is_empty());
    assert!(cabin.grok_sessions.is_empty());
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
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

// Landed from PR #174.
#[test]
fn leaving_a_chat_clears_attach_and_asks() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("leave-chrome");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("isolated config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.running = false;
    cabin.attach_name = Some("shot.png".into());
    cabin.hands_attach = true;
    cabin.eyes_attach = true;
    cabin.elicit_draft = "name the shot".into();
    cabin.perm_ask = Some(grokhub_core::wire::PermissionAsk {
        rpc_id: serde_json::Value::Null,
        session_id: "sess".into(),
        title: "Run".into(),
        tool_call_id: "tool".into(),
        action: "shot.png".into(),
        reason: "attach".into(),
        reject_option: None,
    });
    cabin.elicit_ask = Some(grokhub_core::wire::ElicitAsk {
        rpc_id: serde_json::Value::Null,
        session_id: "sess".into(),
        tool_call_id: "tool".into(),
        server_name: "form".into(),
        message: "need a name".into(),
        mode: "form".into(),
        url: String::new(),
        elicitation_id: "elicit".into(),
        field_name: Some("name".into()),
        field_title: "Name".into(),
        secret: false,
    });

    cabin.drop_leaving_thread_chrome();

    assert!(cabin.attach_name.is_none());
    assert!(!cabin.hands_attach);
    assert!(!cabin.eyes_attach);
    assert!(cabin.elicit_draft.is_empty());
    assert!(cabin.perm_ask.is_none());
    assert!(cabin.elicit_ask.is_none());
    assert!(cabin.acp.is_none());
    assert!(!cabin.running);

    std::env::remove_var("GROKHUB_CONFIG");
}

fn perm_ask_for_test(id: u64, title: &str) -> grokhub_core::wire::PermissionAsk {
    grokhub_core::wire::PermissionAsk {
        rpc_id: serde_json::json!(id),
        session_id: "sess".into(),
        title: title.into(),
        tool_call_id: format!("tool-{id}"),
        action: title.into(),
        reason: String::new(),
        reject_option: Some("reject-once".into()),
    }
}

#[test]
fn a_second_ask_waits_its_turn_instead_of_cancelling_the_first() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("perm-queue");
    cabin.running = true;
    cabin.show_perm_ask(perm_ask_for_test(1, "Get-CimInstance"));
    cabin.perm_always_confirm = Some(serde_json::json!(1));
    cabin.show_perm_ask(perm_ask_for_test(2, "send_feedback"));
    assert_eq!(
        cabin.perm_ask.as_ref().map(|p| p.title.as_str()),
        Some("Get-CimInstance"),
        "a parallel ask must not replace (and cancel) the one on screen"
    );
    assert_eq!(
        cabin.perm_always_confirm,
        Some(serde_json::json!(1)),
        "a queued ask leaves the Always beat for the card on screen"
    );
    assert_eq!(cabin.perm_queue.len(), 1);
    cabin.next_perm_ask();
    assert_eq!(
        cabin.perm_ask.as_ref().map(|p| p.title.as_str()),
        Some("send_feedback")
    );
    assert!(cabin.perm_always_confirm.is_none());
    assert!(cabin.perm_queue.is_empty());
    cabin.show_perm_ask(perm_ask_for_test(3, "third"));
    cabin.withdraw_perm_asks();
    assert!(cabin.perm_ask.is_none() && cabin.perm_queue.is_empty());
    cabin.show_perm_ask(perm_ask_for_test(4, "four"));
    cabin.show_perm_ask(perm_ask_for_test(5, "five"));
    cabin.halt_in_flight();
    assert!(
        cabin.perm_ask.is_none() && cabin.perm_queue.is_empty(),
        "Stop withdraws the queued asks too"
    );

    let src = cabin_src();
    let ask = src
        .split("fn paint_perm_ask(")
        .nth(1)
        .and_then(|s| s.split("fn paint_elicit_ask(").next())
        .expect("paint_perm_ask");
    // The card and the inbox answer through one function.
    let answer = src
        .split("fn answer_perm_at(")
        .nth(1)
        .and_then(|s| s.split("fn paint_perm_ask(").next())
        .expect("answer_perm_at");
    assert!(ask.contains("self.answer_perm_at(0, allow,"), "{ask}");
    let ask = format!("{ask}{answer}");
    assert!(
        ask.contains("reject_permission(&p)")
            && !ask.contains("answer_permission(p.rpc_id.clone(), false)"),
        "Deny selects Grok's reject option; `cancelled` reads as \"User cancelled\": {ask}"
    );
    assert!(
        ask.contains("next_perm_ask()") && ask.contains("perm_queue.drain(..)"),
        "an answer brings up the next ask, and Always covers the queued ones: {ask}"
    );
    assert!(
        ask.contains("bare_press(ui, egui::Key::Escape)")
            && ask.contains("overlay_over_chat(")
            && !ask.contains("key_pressed(egui::Key::Escape)"),
        "only a bare Esc with nothing over the chat denies: {ask}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn only_a_bare_key_with_nothing_over_the_chat_answers_the_card() {
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    // egui marks a repeat itself: a second press with no release in between.
    let pressed = |ctx: &egui::Context, ev: egui::Event| {
        let mut hit = (false, false);
        let input = egui::RawInput {
            events: vec![ev],
            ..Default::default()
        };
        let _ = crate::theme::test_pass(ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                hit = (
                    super::bare_press(ui, egui::Key::Escape),
                    super::bare_press(ui, egui::Key::Enter),
                );
            });
        });
        hit
    };
    let esc = egui::Context::default();
    assert_eq!(
        pressed(&esc, key(egui::Key::Escape, egui::Modifiers::NONE)),
        (true, false)
    );
    assert_eq!(
        pressed(&esc, key(egui::Key::Escape, egui::Modifiers::NONE)),
        (false, false),
        "a held Esc's repeats must not deny the next card"
    );
    let enter = egui::Context::default();
    assert_eq!(
        pressed(&enter, key(egui::Key::Enter, egui::Modifiers::NONE)),
        (false, true)
    );
    assert_eq!(
        pressed(
            &egui::Context::default(),
            key(egui::Key::Escape, egui::Modifiers::SHIFT)
        ),
        (false, false),
        "a chord is another shortcut's key"
    );
    let renamed = egui::Context::default();
    let mut left = true;
    let input = egui::RawInput {
        events: vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        ..Default::default()
    };
    let _ = crate::theme::test_pass(&renamed, input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            super::drop_key(ui, egui::Key::Escape);
            left = super::bare_press(ui, egui::Key::Escape);
        });
    });
    assert!(
        !left,
        "Esc that cancelled a rename must not also deny the card"
    );
    let side = include_str!("sidebar.rs");
    assert_eq!(
        side.matches("drop_key(ui, egui::Key::Escape)").count(),
        2,
        "chat and project rename both keep their Esc: {side}"
    );

    let ctx = egui::Context::default();
    let frame = |area: Option<&str>| {
        let mut over = false;
        let _ = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            if let Some(id) = area {
                egui::Area::new(egui::Id::new(id))
                    .order(egui::Order::Foreground)
                    .show(&ctx, |ui| ui.label("menu"));
            }
            over = super::overlay_over_chat(&ctx);
        });
        over
    };
    frame(Some("chat-jump"));
    assert!(!frame(Some("chat-jump")), "the jump pill is not an overlay");
    frame(Some("plus-menu"));
    assert!(frame(None), "Esc that closes a menu belongs to the menu");
    frame(None);
    assert!(!frame(None));
}

#[test]
fn escape_closes_plus_folder_avatar_shortcuts_and_upload() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("esc-overlays");
    let esc = || egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let ctx = egui::Context::default();

    cabin.plus_menu = Some(grokhub_core::PlusTarget::Chat);
    cabin.plus_anchor = egui::Pos2::ZERO;
    cabin.plus_ignore_close = true;
    let _ = crate::theme::test_pass(&ctx, esc(), |ui| {
        cabin.ui_plus_overlays(ui.ctx());
    });
    assert!(cabin.plus_menu.is_none(), "Esc closes the plus menu");
    assert!(
        !cabin.plus_ignore_close,
        "the open-click guard stays, and Esc clears it"
    );

    cabin.file_pick = Some(grokhub_core::PlusTarget::Chat);
    cabin.pick_dir = root.display().to_string();
    let _ = crate::theme::test_pass(&ctx, esc(), |ui| {
        cabin.ui_plus_overlays(ui.ctx());
    });
    assert!(cabin.file_pick.is_none(), "Esc closes Upload");

    cabin.proj_add_for = Some("pid".into());
    cabin.proj_menu_pos = egui::Pos2::ZERO;
    cabin.proj_ignore_close = true;
    let _ = crate::theme::test_pass(&ctx, esc(), |ui| {
        cabin.ui_project_overlays(ui.ctx());
    });
    assert!(cabin.proj_add_for.is_none(), "Esc closes Add to folder");
    assert!(!cabin.proj_ignore_close);

    cabin.settings_menu_open = true;
    cabin.settings_menu_ignore = true;
    let _ = crate::theme::test_pass(&ctx, esc(), |ui| {
        cabin.ui_settings_menu(ui.ctx());
    });
    assert!(!cabin.settings_menu_open, "Esc closes the avatar menu");
    assert!(!cabin.settings_menu_ignore);

    cabin.shortcuts_open = true;
    let _ = crate::theme::test_pass(&ctx, esc(), |ui| {
        cabin.paint_shortcuts(ui.ctx());
    });
    assert!(!cabin.shortcuts_open, "Esc closes Shortcuts");

    release_isolated(&root, cabin);
}

// Landed from PR #175.
#[test]
fn clear_slash_empties_the_chat() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("clear-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    let idx = cabin.thread_idx;
    let thread_id = cabin.threads[idx].id.clone();
    {
        let thread = &mut cabin.threads[idx];
        thread.messages_mut().push(("user".into(), "hello".into()));
        thread.grok_session = Some("sess".into());
    }
    cabin.messages = cabin.threads[idx].messages.clone();
    assert!(!cabin.running);

    cabin.run_slash(Slash::Clear);

    assert_eq!(cabin.status, "Cleared");
    assert!(cabin.messages.is_empty());
    let thread = cabin
        .threads
        .iter()
        .find(|t| t.id == thread_id)
        .expect("seeded thread");
    assert!(thread.messages.is_empty());
    assert!(thread.grok_session.as_deref().unwrap_or("").is_empty());
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #176.
#[test]
fn context_slash_counts_an_empty_chat() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("context-slash");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    if cabin.threads.get(cabin.thread_idx).is_none() {
        cabin.new_thread(false);
    }
    assert!(cabin.grok_usage.is_empty());
    assert!(cabin.cfg.goal_pin.is_empty());
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(super::Slash::Context);

    assert_eq!(cabin.status, "0 turns · 0 tokens · 0% · pin none");
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());
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

// Landed from PR #178.
#[test]
fn plan_and_empty_video_stay_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("plan-video");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.new_thread(false);
    {
        let thread = cabin
            .threads
            .get_mut(cabin.thread_idx)
            .expect("seeded thread");
        thread.grok_session = Some("sess".into());
    }

    cabin.run_slash(super::Slash::Plan);
    assert_eq!(cabin.status, "Plan mode — Grok Build will plan first");
    assert!(matches!(cabin.session_mode, super::SessionMode::Plan));
    assert_eq!(cabin.cfg.session_mode, "plan");
    assert!(cabin.acp.is_none());
    let session = cabin
        .threads
        .get(cabin.thread_idx)
        .and_then(|t| t.grok_session.as_deref())
        .unwrap_or("");
    assert!(session.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(super::Slash::ImagineVideo(String::new()));
    assert!(matches!(cabin.nav, super::Nav::Imagine));
    assert!(matches!(cabin.imagine_kind, super::ImagineKind::Video));
    assert!(cabin.imagine_want_focus);
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

fn feed_card(id: &str, kind: grokhub_core::UpdateKind, held: bool) -> grokhub_core::UpdateCard {
    grokhub_core::UpdateCard {
        id: id.to_string(),
        kind,
        title: id.to_string(),
        body: None,
        created_at: 1,
        status: grokhub_core::UpdateStatus::Unread,
        action: None,
        expires_at: None,
        held,
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

// Landed from PR #182.
#[test]
fn empty_imagine_opens_without_a_run() {
    struct RestoreEnv182 {
        config: Option<String>,
        tray: Option<String>,
    }
    impl Drop for RestoreEnv182 {
        fn drop(&mut self) {
            match self.config.take() {
                Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
            match self.tray.take() {
                Some(v) => std::env::set_var("GROKHUB_TRAY", v),
                None => std::env::remove_var("GROKHUB_TRAY"),
            }
        }
    }

    let _cfg = crate::config::hold_test_config();
    let restore = RestoreEnv182 {
        config: std::env::var("GROKHUB_CONFIG").ok(),
        tray: std::env::var("GROKHUB_TRAY").ok(),
    };
    let root = crate::config::test_config_root("empty-imagine");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    std::env::set_var("GROKHUB_TRAY", "0");

    let mut cabin = Cabin::quiet_for_test();
    let kind_before = cabin.imagine_kind;
    assert!(cabin.imagine_prompt.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(Slash::Imagine(String::new()));
    assert!(matches!(cabin.nav, Nav::Imagine));
    assert!(cabin.imagine_want_focus);
    assert!(cabin.imagine_prompt.is_empty());
    assert_eq!(cabin.imagine_kind, kind_before);
    assert!(!cabin.running);

    cabin.run_slash(Slash::Imagine("   ".into()));
    assert!(matches!(cabin.nav, Nav::Imagine));
    assert!(cabin.imagine_want_focus);
    assert!(cabin.imagine_prompt.is_empty());
    assert_eq!(cabin.imagine_kind, kind_before);
    assert!(!cabin.running);

    drop(restore);
}

// Landed from PR #183.
#[test]
fn view_plan_opens_a_stored_plan() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("view-plan");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);

    cabin.threads[cabin.thread_idx].plan_body.clear();
    cabin.run_slash(super::Slash::ViewPlan);
    assert_eq!(cabin.status, "No plan yet — use Plan mode");
    assert!(!cabin.plan_open);
    assert!(!cabin.running);

    cabin.threads[cabin.thread_idx].plan_body = " \n\t ".into();
    cabin.run_slash(super::Slash::ViewPlan);
    assert_eq!(cabin.status, "No plan yet — use Plan mode");
    assert!(!cabin.plan_open);
    assert!(!cabin.running);

    cabin.threads[cabin.thread_idx].plan_body = "Ship the harbor".into();
    cabin.run_slash(super::Slash::ViewPlan);
    assert!(cabin.plan_open);
    assert_eq!(cabin.status, "View plan");
    assert!(!cabin.running);
}

// Landed from PR #184.
#[test]
fn rewind_files_asks_for_a_bind() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("rewind-bind");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if !cabin.cfg.project_dir.is_empty() {
        cabin.cfg.project_dir = String::new();
    }
    cabin.run_slash(Slash::RewindFiles);
    assert_eq!(cabin.status, "Bind a project first — /project bind");
    assert!(
        cabin.rewind_rows.is_empty(),
        "empty project dir must not snapshot"
    );
    assert!(!cabin.running, "empty project dir must not queue a shell");
    assert!(cabin.cfg.project_dir.is_empty());
}

// Landed from PR #185.
#[test]
fn goal_pin_sets_and_clears() {
    let _hold = config::hold_test_config();
    let root = config::test_config_root("goal-pin");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let _pin = config::TestConfigDir::set(root.clone());

    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.goal_pin.clear();
    assert!(cabin.cfg.goal_pin.is_empty());
    assert!(!cabin.running);

    let pin_file = root.join("app.json");
    let wait_pin = move |want: &str| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut last = String::new();
        while std::time::Instant::now() < deadline {
            last = std::fs::read_to_string(&pin_file)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|v| {
                    v.get("goalPin")
                        .and_then(|p| p.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            if last == want {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("persisted goal_pin {last:?}, want {want:?}");
    };

    for raw in ["", "   ", "\t", "\n  ", "status", "STATUS", "Status"] {
        cabin.run_slash(Slash::Goal(raw.to_string()));
        assert_eq!(cabin.status, "No goal pin", "input {raw:?}");
        assert!(cabin.cfg.goal_pin.is_empty(), "input {raw:?}");
        assert!(!cabin.running);
    }

    cabin.run_slash(Slash::Goal("Ship the harbor".to_string()));
    assert_eq!(cabin.cfg.goal_pin, "Ship the harbor");
    assert_eq!(cabin.status, "Goal: Ship the harbor");
    assert!(!cabin.running);
    wait_pin("Ship the harbor");

    for raw in ["status", "STATUS", "Status"] {
        cabin.run_slash(Slash::Goal(raw.to_string()));
        assert_eq!(cabin.status, "Goal: Ship the harbor", "input {raw:?}");
        assert_eq!(cabin.cfg.goal_pin, "Ship the harbor");
        assert!(!cabin.running);
    }

    for raw in ["clear", "CLEAR", "Clear"] {
        if cabin.cfg.goal_pin.is_empty() {
            cabin.run_slash(Slash::Goal("Ship the harbor".to_string()));
            assert_eq!(cabin.cfg.goal_pin, "Ship the harbor");
            wait_pin("Ship the harbor");
        }
        cabin.run_slash(Slash::Goal(raw.to_string()));
        assert!(cabin.cfg.goal_pin.is_empty(), "input {raw:?}");
        assert_eq!(cabin.status, "Goal cleared", "input {raw:?}");
        assert!(!cabin.running);
        wait_pin("");
    }

    cabin.run_slash(Slash::Goal("status".to_string()));
    assert_eq!(cabin.status, "No goal pin");
    assert!(cabin.cfg.goal_pin.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #186.
#[test]
fn btw_sets_a_side_ask() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("btw-side-ask");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.run_slash(Slash::Btw);

    assert!(matches!(cabin.session_mode, SessionMode::Ask));
    assert_eq!(cabin.cfg.session_mode, "ask");
    assert!(!cabin.running);
    assert_eq!(cabin.status, "btw — look-safe side ask");
    assert_eq!(cabin_default_session_id(&cabin.cfg.session_mode), "ask");
    let persisted = cabin
        .cfg_slot
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .cfg
        .session_mode
        .clone();
    assert_eq!(persisted, "ask");
}

// Landed from PR #187.
#[test]
fn project_show_and_clear_stay_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("project-show");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(Slash::ProjectShow);
    assert_eq!(cabin.status, "No bound project");
    assert!(!cabin.running);

    cabin.cfg.project_dir = r"D:\Work\Harbor".to_string();
    cabin.run_slash(Slash::ProjectShow);
    assert_eq!(cabin.status, r"Project D:\Work\Harbor");
    assert!(!cabin.running);

    cabin.new_thread(false);
    {
        let thread = cabin
            .threads
            .get_mut(cabin.thread_idx)
            .expect("seeded thread");
        thread.grok_session = Some("sess".into());
        thread.grok_cwd = Some(r"D:\Work\Harbor".into());
    }
    cabin.project_sel = Some("bound".into());

    cabin.run_slash(Slash::ProjectClear);
    assert!(cabin.cfg.project_dir.is_empty());
    assert_eq!(cabin.project_sel, None);
    let thread = cabin.threads.get(cabin.thread_idx).expect("seeded thread");
    assert_eq!(thread.grok_session, None);
    assert_eq!(thread.grok_cwd, None);
    assert_eq!(cabin.status, "Unbound — full desktop");
    assert!(!cabin.running);
}

// Landed from PR #188.
#[test]
fn missing_skill_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("missing-skill");
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash(grokhub_core::Slash::Skill("harbor".into()));
    assert_eq!(cabin.status, "No skill harbor");
    assert!(matches!(cabin.nav, super::Nav::Chat));
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());
}

// Landed from PR #189.
#[test]
fn empty_undo_and_retry_stay_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("empty-undo-retry");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(super::Slash::Undo);
    assert_eq!(cabin.status, "Nothing to undo");
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(super::Slash::Retry);
    assert_eq!(cabin.status, "Nothing to retry");
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #190.
#[test]
fn project_acts_need_a_selection() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("project-need-sel");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    assert!(cabin.project_sel.is_none());
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(Slash::ProjectRename("Harbor".into()));
    assert_eq!(cabin.status, "Select a project first");
    assert!(cabin.project_sel.is_none());
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(Slash::ProjectMove("Notes".into()));
    assert_eq!(cabin.status, "Select a project first");
    assert!(cabin.project_sel.is_none());
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(Slash::ProjectDelete);
    assert_eq!(cabin.status, "Select a project first");
    assert!(cabin.project_sel.is_none());
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #191.
#[test]
fn forget_clears_memory_off_a_run() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("forget-off-run");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.new_thread(false);
    cabin.mem_name = "MEMORY.md".into();
    cabin.mem_body = "wifi stays until forget".into();
    assert!(!cabin.scratch());
    assert!(!cabin.running);

    cabin.run_slash(Slash::Forget(None));

    assert_eq!(cabin.status, "Forgot MEMORY.md");
    assert!(cabin.mem_name == "MEMORY.md" && cabin.mem_body.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #192.
#[test]
fn stage_new_folder_asks_for_a_name() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("stage-folder");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.stage_new_folder();

    assert_eq!(cabin.projects.len(), 1);
    let id = cabin.projects[0].id.clone();
    assert_eq!(cabin.projects[0].name, "Folder");
    assert!(matches!(cabin.projects[0].kind, ProjectKind::Folder));
    assert!(!cabin
        .projects
        .iter()
        .any(|n| matches!(n.kind, ProjectKind::Project)));
    assert_eq!(cabin.proj_staged.as_deref(), Some(id.as_str()));
    assert_eq!(cabin.proj_rename.as_deref(), Some(id.as_str()));
    assert_eq!(cabin.status, "Name this folder");
    assert!(!cabin.running);
}

// Landed from PR #193.
#[test]
fn forget_topic_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("forget-topic");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.new_thread(false);
    cabin.mem_name = "MEMORY.md".into();
    let note = "harbor light at dusk".to_string();
    cabin.mem_body = note.clone();

    cabin.run_slash(grokhub_core::Slash::Forget(Some("harbor".into())));

    assert_eq!(cabin.mem_body, grokhub_core::forget_topic(&note, "harbor"));
    assert_eq!(cabin.status, "Forgot harbor");
    assert!(!cabin.running);
    assert!(cabin.nav == super::Nav::Chat);
}

// Landed from PR #194.
#[test]
fn cancel_staged_folder_drops_it() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("cancel-folder");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.stage_new_folder();
    assert_eq!(cabin.projects.len(), 1);
    assert!(matches!(cabin.projects[0].kind, super::ProjectKind::Folder));
    assert_eq!(cabin.projects[0].name, "Folder");
    assert!(cabin.proj_staged.is_some());
    assert_eq!(cabin.status, "Name this folder");
    assert!(!cabin.running);

    cabin.cancel_proj_rename();
    assert!(cabin.projects.is_empty());
    assert!(cabin.proj_staged.is_none());
    assert!(cabin.proj_rename.is_none());
    assert_eq!(cabin.status, "Name this folder");
    assert!(!cabin.running);
}

// Landed from PR #196.
#[test]
fn finish_staged_folder_renames_it() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("finish-folder");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.stage_new_folder();
    cabin.proj_rename_buf.clear();
    cabin.finish_proj_rename();
    assert_eq!(cabin.status, "need a name");
    assert!(cabin.projects.is_empty());
    assert!(cabin.proj_staged.is_none());
    assert!(!cabin.running);

    cabin.stage_new_folder();
    cabin.proj_rename_buf = "Harbor".into();
    cabin.finish_proj_rename();
    assert_eq!(cabin.status, "Renamed Harbor");
    assert!(
        matches!(
            cabin.projects.as_slice(),
            [node]
                if node.name == "Harbor"
                    && node.path.is_empty()
                    && matches!(node.kind, grokhub_core::ProjectKind::Folder)
        ),
        "status={} projects={}",
        cabin.status,
        cabin.projects.len()
    );
    assert!(cabin
        .projects
        .iter()
        .all(|node| !matches!(node.kind, grokhub_core::ProjectKind::Project)));
    assert!(cabin.proj_staged.is_none());
    assert!(cabin.proj_rename.is_none());
    assert!(!cabin.running);
}

// Landed from PR #197.
#[allow(clippy::too_many_lines)]
fn quiet_cabin() -> Cabin {
    let cfg = AppConfig::default();
    let persist_io = std::sync::Arc::new(std::sync::Mutex::new(()));
    let persist_svc = PersistService::start(
        persist_io.clone(),
        std::sync::Arc::new(std::sync::Mutex::new(PersistMark::default())),
    );
    Cabin {
        nav: super::Nav::Chat,
        cfg: cfg.clone(),
        composer: String::new(),
        messages: std::sync::Arc::new(Vec::new()),
        messages_rev: 0,
        messages_body_rev: 0,
        status: String::new(),
        running: false,
        turn_retried: false,
        host_halt: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx: None,
        chat_job_thread: None,
        inflight_open: false,
        hub: std::sync::Arc::new(std::sync::Mutex::new(grokhub_core::HubState::empty())),
        hub_on: false,
        hub_port: grokhub_core::DEFAULT_PORT,
        task_prompt: String::new(),
        mem_name: String::new(),
        mem_body: String::new(),
        mem_cache_at: [0, 0, 0],
        mem_cache_body: [String::new(), String::new(), String::new()],
        last_persist: std::time::Instant::now(),
        persist_idle_key: String::new(),
        persist_rx: None,
        persist_io,
        persist_svc,
        setup: super::setup_wizard::SetupWizard::default(),
        persist_gen: 0,
        persist_err: std::sync::Arc::new(std::sync::Mutex::new(None)),
        cfg_slot: std::sync::Arc::new(std::sync::Mutex::new(super::CfgSlot { gen: 0, cfg })),
        board: Vec::new(),
        board_title: String::new(),
        board_notes: String::new(),
        journal: Vec::new(),
        journal_view: super::journal_ui::JournalView::default(),
        imagine_prompt: String::new(),
        imagine_last: String::new(),
        skill_name: String::new(),
        skill_body: String::new(),
        skill_list: Vec::new(),
        eyes_text: String::new(),
        last_host: Vec::new(),
        last_frame_url: None,
        hands_attach: false,
        eyes_attach: false,
        speak_next: false,
        verify_ok_turn: false,
        verify_chip: String::new(),
        reflect_diff: String::new(),
        last_activity: std::time::Instant::now(),
        reflected_idle: false,
        last_recipe: None,
        update_pct: None,
        update_can_restart: false,
        secrets: crate::secrets::Secrets::default(),
        threads: Vec::new(),
        thread_idx: 0,
        oauth_pending: None,
        oauth_next_poll: std::time::Instant::now(),
        oauth_start_rx: None,
        oauth_poll_rx: None,
        host_hour_count: 0,
        host_hour_at: std::time::Instant::now(),
        host_reserved: 0,
        plan_pending: None,
        tray: None,
        tray_rx: None,
        window_visible: true,
        resume_fresh: false,
        saw_minimized: false,
        ideas_filled: false,
        ideas_rx: None,
        tray_saw_unfocused: false,
        tray_hid_at: std::time::Instant::now(),
        want_quit: false,
        told_tray: false,
        pending_hub_task: None,
        automations: Vec::new(),
        grok_loops: Vec::new(),
        updates: Vec::new(),
        daily_drop_file: None,
        card_prefs: grokhub_core::CardPrefs::default(),
        grok_loop_rx: None,
        night_nl: String::new(),
        watch_once: false,
        watched_steps: Vec::new(),
        chat_tail_frames: 0,
        cap_auto_buf: String::new(),
        cap_host_buf: String::new(),
        quiet_start_buf: String::new(),
        quiet_end_buf: String::new(),
        history_q: String::new(),
        history_q_seen: String::new(),
        history_q_at: None,
        history_hits: Vec::new(),
        archive_undo: None,
        chat_archive_day: None,
        last_receipt_ok: None,
        beta_auto_off_checked_at: None,
        last_receipts: Vec::new(),
        try_again: false,
        last_rewind_id: None,
        rewind_rows: Vec::new(),
        host_live: String::new(),
        daily_auto_used: 0,
        daily_auto_day: String::new(),
        slash_pick: 0,
        slash_filter_n: 0,
        slash_filter_first: String::new(),
        last_window_title: String::new(),
        voice_orb: String::new(),
        governor: grokhub_core::Governor::cabin(),
        last_heartbeat: std::time::Instant::now(),
        pace: Default::default(),
        pace_traced: Vec::new(),
        night_check_rx: None,
        learning: grokhub_core::LearningState::default(),
        suggestions: grokhub_core::SuggestionStore::default(),
        review_rx: None,
        review_busy: false,
        digest_rx: None,
        digest_busy: false,
        digest_pending: None,
        digest_wants_lookup: false,
        digest_steer: String::new(),
        digest_interests: String::new(),
        situation_ping: None,
        window_focused: true,
        offer_repeated: false,
        usage: grokhub_core::UsageDay::default(),
        palette_open: false,
        palette_q: String::new(),
        palette_pick: 0,
        palette_focus: false,
        palette_files: Vec::new(),
        palette_files_q: String::new(),
        palette_files_root: String::new(),
        palette_file_rx: None,
        palette_steps: Vec::new(),
        palette_steps_q: String::new(),
        palette_step_rx: None,
        shortcuts_open: false,
        active_skill_follow: None,
        card_notes_follow: None,
        board_notes_edit: None,
        last_anticipate_ms: 0,
        proactive: Default::default(),
        last_proactive_ms: 0,
        goal_step: 0,
        followup_step: 0,
        stream_buf: String::new(),
        thought_buf: String::new(),
        chat_views: Vec::new(),
        chat_view_keys: Vec::new(),
        chat_view_tid: String::new(),
        chat_view_n: 0,
        chat_view_last: 0,
        chat_view_rev: 0,
        chat_view_body: (0, 0),
        presence_ring: Vec::new(),
        voice_state: grokhub_core::VoiceState::Idle,
        voice_ready_at: None,
        voice_hold_rx: None,
        cmd_line: String::new(),
        cmd_hist: Vec::new(),
        agents: Vec::new(),
        last_live: std::time::Instant::now(),
        live_cap_rx: None,
        eyes_cap_rx: None,
        kick_cap_rx: None,
        pending_kick: None,
        kick_frame: None,
        kick_skip: false,
        recipe_cap_rx: None,
        recipe_desk_rx: None,
        host_diff_rx: None,
        host_diff_kick: false,
        verify_rx: None,
        hotkeys: None,
        hotkey_hey: 0,
        hotkey_halt: 0,
        sidebar_q: String::new(),
        rename_idx: None,
        rename_buf: String::new(),
        rename_focus: false,
        rename_lock: None,
        chip_memory: grokhub_core::ChipMemory::default(),
        chip_dismissed: Vec::new(),
        idea_board: Default::default(),
        pulse_view: Default::default(),
        agent_cursor: None,
        board_view: Default::default(),
        llm_chips: Vec::new(),
        visible_chips: Vec::new(),
        chip_rx: None,
        chip_busy: false,
        chip_fp: String::new(),
        chip_paint_key: String::new(),
        chip_llm_at: 0,
        greeting: String::new(),
        greeting_fp: String::new(),
        greeting_user_at: 0,
        greeting_memory_at: 0,
        greeting_user_md: String::new(),
        greeting_memory_md: String::new(),
        greeting_files_rx: None,
        greeting_flush_name: String::new(),
        greeting_flush_len: 0,
        greeting_llm_fp: String::new(),
        greeting_rx: None,
        greeting_busy: false,
        greeting_llm_at: 0,
        continue_hint: String::new(),
        skill_q: String::new(),
        pending_connectors: Vec::new(),
        auto_compose: false,
        board_compose: false,
        board_edit: None,
        board_link: false,
        settings_menu_open: false,
        settings_menu_ignore: false,
        win_max: false,
        geom_dirty: false,
        geom_applied: false,
        geom_apply_frames: 0,
        imagine_want_focus: false,
        composer_want_focus: false,
        settings_sec: super::SettingsSec::Account,
        market: Default::default(),
        picker: Default::default(),
        playbooks: Default::default(),
        interests: Default::default(),
        sources: Default::default(),
        settings_back: super::Nav::Chat,
        imagine_aspect: 0,
        imagine_quality: false,
        imagine_kind: grokhub_core::ImagineKind::Image,
        imagine_style: 0,
        imagine_video_res: 0,
        imagine_video_dur: 0,
        imagine_video_audio: false,
        imagine_aspect_open: false,
        imagine_style_open: false,
        imagine_menu_ignore: false,
        imagine_style_anchor: egui::Rect::NOTHING,
        imagine_aspect_anchor: egui::Rect::NOTHING,
        imagine_expand: false,
        imagine_job_prompt: String::new(),
        imagine_error: String::new(),
        imagine_pending: false,
        imagine_native: super::imagine::ImagineNative::default(),
        imagine_save_rx: None,
        goal_rx: None,
        goal_busy: false,
        goal_stale: false,
        wall: grokhub_core::ImagineWall::default(),
        wall_rx: None,
        logic_input_pass: None,
        wall_busy: false,
        attach_url: None,
        attach_name: None,
        attach_path: None,
        attach_kind: None,
        attach_tray: super::attach_tray::AttachTray::default(),
        attach_ingest: None,
        attach_thumb_cache: Vec::new(),
        attach_pick_rx: None,
        imagine_ref: None,
        plus_menu: None,
        plus_anchor: egui::Pos2::ZERO,
        plus_ignore_close: false,
        file_pick: None,
        pick_rx: None,
        drop_extra: 0,
        pick_list_rx: None,
        pick_dir: String::new(),
        pick_cache: None,
        projects: Vec::new(),
        project_sel: None,
        proj_menu_pos: egui::Pos2::ZERO,
        proj_add_for: None,
        proj_rename: None,
        proj_rename_buf: String::new(),
        proj_rename_focus: false,
        proj_rename_lock: None,
        proj_staged: None,
        proj_ignore_close: false,
        projects_dirty: false,
        oauth_photo: None,
        oauth_photo_key: String::new(),
        oauth_photo_rx: None,
        oauth_photo_busy: false,
        oauth_profile_tried: false,
        profile_photo: None,
        profile_photo_key: String::new(),
        profile_photo_rx: None,
        profile_photo_busy: false,
        profile_pick_rx: None,
        profile_pick_token: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        profile_file_io: std::sync::Arc::new(std::sync::Mutex::new(())),
        cabin_latest: None,
        last_update_probe: None,
        update_probe_rx: None,
        cabin_overlay_done: false,
        queued_overlay: None,
        update_cabin_note: None,
        acp: None,
        grok_p_rx: None,
        grok_usage: grokhub_core::wire::GrokUsage::default(),
        tokens_seen: (0, 0, 0),
        grok_commands: Vec::new(),
        grok_tasks: Vec::new(),
        followup_queue: Vec::new(),
        side_ask_queue: Vec::new(),
        bg: super::background::BgWork::default(),
        side_ask_kick: false,
        plan_open: false,
        tool_cards: Vec::new(),
        panels: super::panels::PanelStore::default(),
        live_blocks: Vec::new(),
        live_keys: Vec::new(),
        turn_log: Vec::new(),
        thought_seam: false,
        say_seam: false,
        desk_frame: None,
        perm_ask: None,
        perm_queue: std::collections::VecDeque::new(),
        perm_always_confirm: None,
        confirm: None,
        jump_last_you: false,
        jump_turn: None,
        find: super::chat_ui::ChatFind::default(),
        elicit_ask: None,
        elicit_draft: String::new(),
        secret_hold: Vec::new(),
        session_mode: grokhub_core::wire::SessionMode::Chat,
        permission_mode: grokhub_core::wire::PermissionMode::Ask,
        harness: Default::default(),
        auto_act: Default::default(),
        scheduled_perm: false,
        grok_sessions: Vec::new(),
        history_rx: None,
        mem_restore_rx: None,
        mem_file_rx: None,
        recall_rx: None,
        sync_rx: None,
        inhabit_rx: None,
        reflect_rx: None,
        amr_imported: false,
        dream_day: None,
        amr_prune: amr_memory::AmrPrune::default(),
        session_show_rx: None,
        import_rx: None,
        native_skills: Vec::new(),
        native_hooks: Vec::new(),
        native_listing_cwd: String::new(),
        native_hooks_trusted: false,
        connector_note: String::new(),
        screen_rec: None,
        screen_diag_rx: None,
        audio_check: None,
        scroll_to_hooks: false,
        composer_geom: None,
        composer_ghost: ghost::ComposerGhost::default(),
        dock_state: super::dock_ui::default_dock_state(),
        toasts: super::status_toasts::cabin_toasts(),
        status_toasted: Default::default(),
    }
}

fn settle_project_flush(cabin: &Cabin) {
    let start = std::time::Instant::now();
    loop {
        if cabin.persist_io.try_lock().is_err() {
            let _guard = cabin.persist_io.lock().unwrap_or_else(|e| e.into_inner());
            return;
        }
        if start.elapsed() > std::time::Duration::from_millis(500) {
            return;
        }
        std::thread::yield_now();
    }
}

#[test]
fn make_folder_names_a_folder() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("make-folder");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = quiet_cabin();
    assert!(!cabin.running);
    assert!(cabin.projects.is_empty());

    cabin.make_folder("");
    assert_eq!(cabin.status, "need a folder name");
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    cabin.make_folder("Harbor");
    assert_eq!(cabin.status, "Folder Harbor");
    assert_eq!(cabin.projects.len(), 1);
    assert!(matches!(
        cabin.projects[0].kind,
        grokhub_core::ProjectKind::Folder
    ));
    assert_eq!(cabin.projects[0].name, "Harbor");
    assert!(cabin.projects[0].path.is_empty());
    assert!(!cabin
        .projects
        .iter()
        .any(|n| matches!(n.kind, grokhub_core::ProjectKind::Project)));
    assert!(!cabin.running);

    settle_project_flush(&cabin);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #198.
#[test]
fn new_chat_under_folder_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("folder-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.projects.push(ProjectNode {
        id: "f1".into(),
        name: "Notes".into(),
        kind: ProjectKind::Folder,
        path: String::new(),
        parent: None,
        open: false,
    });
    cabin.apply_project_menu("f1".into(), ProjectMenuAct::NewChat);

    assert_eq!(cabin.status, "New chat");
    assert_eq!(cabin.projects.len(), 1);
    let folder = &cabin.projects[0];
    assert_eq!(folder.id, "f1");
    assert_eq!(folder.name, "Notes");
    assert!(folder.path.is_empty());
    assert!(folder.parent.is_none());
    assert!(folder.open);
    assert!(matches!(folder.kind, ProjectKind::Folder));
    assert!(cabin
        .projects
        .iter()
        .all(|n| matches!(n.kind, ProjectKind::Folder)));
    assert_eq!(cabin.project_sel.as_deref(), Some("f1"));
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(cabin.composer_want_focus);
    assert!(!cabin.running);
    assert!(cabin.cfg.goal_pin.is_empty());
    let thread = cabin.threads.get(cabin.thread_idx).expect("current thread");
    assert_eq!(thread.title, "Chat");
    assert!(!thread.scratch);
    assert_eq!(thread.project_id.as_deref(), Some("f1"));

    std::env::remove_var("GROKHUB_CONFIG");
    let _ = std::fs::remove_dir_all(&root);
}

// Landed from PR #199.
#[test]
fn remove_from_folder_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("remove-folder");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.projects = vec![
        ProjectNode {
            id: "f1".into(),
            name: "Notes".into(),
            kind: ProjectKind::Folder,
            path: String::new(),
            parent: None,
            open: true,
        },
        ProjectNode {
            id: "p1".into(),
            name: "Harbor".into(),
            kind: ProjectKind::Project,
            path: String::new(),
            parent: Some("f1".into()),
            open: true,
        },
    ];

    let status = cabin.status.clone();
    cabin.apply_project_menu("f1".into(), ProjectMenuAct::RemoveFromFolder);
    assert_eq!(cabin.status, status);
    let harbor = cabin
        .projects
        .iter()
        .find(|n| n.id == "p1")
        .expect("harbor");
    assert_eq!(harbor.parent.as_deref(), Some("f1"));
    assert!(matches!(harbor.kind, ProjectKind::Project));
    assert!(!cabin.running);

    cabin.apply_project_menu("p1".into(), ProjectMenuAct::RemoveFromFolder);
    assert_eq!(cabin.status, "Moved to Projects");
    let harbor = cabin
        .projects
        .iter()
        .find(|n| n.id == "p1")
        .expect("harbor");
    assert!(harbor.parent.is_none());
    assert!(matches!(harbor.kind, ProjectKind::Project));
    assert!(harbor.path.is_empty());
    assert!(cabin
        .projects
        .iter()
        .any(|n| { n.id == "f1" && n.name == "Notes" && matches!(n.kind, ProjectKind::Folder) }));
    assert_eq!(
        cabin
            .projects
            .iter()
            .filter(|n| matches!(n.kind, ProjectKind::Project))
            .count(),
        1
    );
    assert!(!cabin.running);
}

// Landed from PR #200.
#[test]
fn add_to_folder_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("add-folder");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.projects.push(ProjectNode {
        id: "p1".into(),
        name: "Harbor".into(),
        kind: ProjectKind::Project,
        path: String::new(),
        parent: None,
        open: true,
    });
    cabin.project_sel = Some("p1".into());

    cabin.move_sel_to_folder_name("Notes");
    assert_eq!(cabin.status, "No folder Notes");
    let harbor = cabin
        .projects
        .iter()
        .find(|n| n.id == "p1")
        .expect("harbor");
    assert!(matches!(harbor.kind, ProjectKind::Project));
    assert!(harbor.parent.is_none());
    assert!(!cabin.running);

    cabin.projects.push(ProjectNode {
        id: "f1".into(),
        name: "Notes".into(),
        kind: ProjectKind::Folder,
        path: String::new(),
        parent: None,
        open: false,
    });
    cabin.move_sel_to_folder_name("Notes");
    assert_eq!(cabin.status, "Added to Notes");
    let harbor = cabin
        .projects
        .iter()
        .find(|n| n.id == "p1")
        .expect("harbor");
    assert!(matches!(harbor.parent.as_deref(), Some("f1")));
    assert!(harbor.path.is_empty());
    let folder = cabin.projects.iter().find(|n| n.id == "f1").expect("notes");
    assert!(matches!(folder.kind, ProjectKind::Folder));
    assert!(folder.open);
    let projects = cabin
        .projects
        .iter()
        .filter(|n| matches!(n.kind, ProjectKind::Project))
        .count();
    let folders = cabin
        .projects
        .iter()
        .filter(|n| matches!(n.kind, ProjectKind::Folder))
        .count();
    assert_eq!((projects, folders), (1, 1));
    assert!(!cabin.running);
}

// Landed from PR #201.
#[test]
fn bind_empty_project_stays_off_a_run() {
    let _cfg = crate::config::hold_test_config();
    let root = crate::config::test_config_root("bind-empty");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.projects = vec![
        ProjectNode {
            id: "f1".into(),
            name: "Notes".into(),
            kind: ProjectKind::Folder,
            path: String::new(),
            parent: None,
            open: true,
        },
        ProjectNode {
            id: "p1".into(),
            name: "Harbor".into(),
            kind: ProjectKind::Project,
            path: String::new(),
            parent: None,
            open: true,
        },
    ];
    assert!(!cabin.running);

    let status = cabin.status.clone();
    let project_sel = cabin.project_sel.clone();
    let projects = cabin.projects.clone();

    cabin.bind_project_id("missing");
    assert_eq!(cabin.status, status);
    assert_eq!(cabin.project_sel, project_sel);
    assert!(cabin.projects == projects);
    assert!(!cabin.running);

    cabin.bind_project_id("f1");
    assert_eq!(cabin.status, status);
    assert_eq!(cabin.project_sel, project_sel);
    assert!(cabin.projects == projects);
    assert!(!cabin.running);
    let folder = cabin
        .projects
        .iter()
        .find(|n| n.id == "f1")
        .expect("folder");
    assert!(matches!(folder.kind, ProjectKind::Folder) && folder.open);

    let threads = cabin.threads.len();
    cabin.nav = Nav::Workboard;
    cabin.bind_project_id("p1");
    assert_eq!(cabin.status, "Bound Harbor");
    assert_eq!(cabin.project_sel.as_deref(), Some("p1"));
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(!cabin.running);
    let folder = cabin
        .projects
        .iter()
        .find(|n| n.id == "f1")
        .expect("folder");
    assert!(matches!(folder.kind, ProjectKind::Folder) && folder.open);
    assert_eq!(
        cabin
            .projects
            .iter()
            .filter(|n| matches!(n.kind, ProjectKind::Project))
            .count(),
        1
    );
    assert_eq!(cabin.threads.len(), threads);
}

// Landed from PR #202.
#[test]
fn remove_project_returns_chats() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("remove-chats");
    let prev = std::env::var("GROKHUB_CONFIG").ok();
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(!cabin.running);

    cabin.projects.push(ProjectNode {
        id: "p1".into(),
        name: "Harbor".into(),
        kind: ProjectKind::Project,
        path: String::new(),
        parent: None,
        open: true,
    });
    assert!(cabin.projects.iter().any(|n| {
        n.id == "p1"
            && n.name == "Harbor"
            && n.path.is_empty()
            && n.parent.is_none()
            && n.open
            && matches!(n.kind, ProjectKind::Project)
    }));

    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let idx = cabin.thread_idx;
    let thread_id = cabin.threads[idx].id.clone();
    cabin.threads[idx].project_id = Some("p1".into());

    cabin.remove_project_id("p1");

    assert_eq!(cabin.status, "Removed Harbor · chats back in History");
    assert!(cabin.projects.is_empty());
    let thread = cabin
        .threads
        .iter()
        .find(|t| t.id == thread_id)
        .expect("current thread");
    assert!(thread.project_id.is_none());
    assert!(!cabin.running);
    assert!(cabin.cfg.project_dir.is_empty());

    match prev {
        Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
        None => std::env::remove_var("GROKHUB_CONFIG"),
    }
}

// Landed from PR #203.
#[test]
fn empty_skill_tile_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("empty-skill-tile");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let tile = grokhub_core::LearnedSuggestion {
        kind: grokhub_core::SuggestionKind::Skill,
        title: String::new(),
        body: String::new(),
        name: None,
        seed: None,
        trigger: None,
        instructions: None,
        provider: None,
        tool: None,
    };
    assert!(matches!(tile.kind, grokhub_core::SuggestionKind::Skill));

    let mut cabin = super::Cabin::quiet_for_test();
    assert!(cabin.suggestions.skills.is_empty());
    assert!(!cabin.running);
    assert!(matches!(cabin.nav, super::Nav::Chat));

    cabin.add_suggested_skill(&tile);

    assert_eq!(cabin.status, "Need a cabin-real skill name and steps");
    assert!(cabin.suggestions.skills.is_empty());
    assert!(!cabin.running);
    assert!(matches!(cabin.nav, super::Nav::Chat));
}

// Landed from PR #204.
#[test]
fn remove_bound_project_unbinds() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("unbind-remove");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    let path = "/tmp/grokhub-unbound-harbor";
    cabin.projects.push(ProjectNode {
        id: "p1".into(),
        name: "Harbor".into(),
        kind: ProjectKind::Project,
        path: path.into(),
        parent: None,
        open: true,
    });
    assert!(matches!(cabin.projects[0].kind, ProjectKind::Project));
    cabin.cfg.project_dir = path.into();
    cabin.project_sel = Some("p1".into());
    assert!(cabin
        .threads
        .iter()
        .all(|t| !matches!(t.project_id.as_deref(), Some("p1"))));
    assert!(!cabin.running);

    cabin.remove_project_id("p1");

    assert_eq!(cabin.status, "Removed Harbor · unbound");
    assert!(cabin.projects.is_empty());
    assert!(cabin.project_sel.is_none());
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #205.
#[test]
fn full_loop_list_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("full-loop-list");
    let _ = std::fs::create_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.grok_loops = (0..50)
        .map(|_| grokhub_core::new_loop("30m".into(), "check".into(), 0))
        .collect();
    cabin.add_automation_seed("/loop 30m check deploy");
    assert_eq!(cabin.status, "Maximum 50 scheduled loops");
    assert_eq!(cabin.grok_loops.len(), 50);
    assert!(cabin.automations.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #206.
#[test]
fn remove_folder_returns_its_project() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("drop-folder");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.projects = vec![
        ProjectNode {
            id: "f1".into(),
            name: "Notes".into(),
            kind: ProjectKind::Folder,
            path: String::new(),
            parent: None,
            open: true,
        },
        ProjectNode {
            id: "p1".into(),
            name: "Harbor".into(),
            kind: ProjectKind::Project,
            path: String::new(),
            parent: Some("f1".into()),
            open: true,
        },
    ];
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(cabin.project_sel.is_none());
    assert!(!cabin
        .threads
        .iter()
        .any(|t| t.project_id.as_deref() == Some("f1")));
    cabin.remove_project_id("f1");
    assert_eq!(cabin.status, "Removed Notes");
    assert!(cabin.projects.iter().all(|n| n.id != "f1"));
    assert!(!cabin
        .projects
        .iter()
        .any(|n| matches!(n.kind, ProjectKind::Folder)));
    assert_eq!(cabin.projects.len(), 1);
    let harbor = &cabin.projects[0];
    assert_eq!(harbor.id, "p1");
    assert_eq!(harbor.name, "Harbor");
    assert!(matches!(harbor.kind, ProjectKind::Project));
    assert!(harbor.path.is_empty());
    assert!(harbor.parent.is_none());
    assert!(!cabin.running);
    assert!(cabin.cfg.project_dir.is_empty());
}

// Landed from PR #207.
#[test]
fn full_automation_list_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("full-auto");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.automations = (0..50)
        .map(|i| grokhub_core::Automation {
            id: format!("job-{i}"),
            name: "Job".into(),
            schedule: "daily".into(),
            time: "09:00".into(),
            times: Vec::new(),
            instructions: "check".into(),
            heartbeat_every_min: 0,
            check_command: String::new(),
            enabled: true,
            last_run: None,
            next_run: None,
            run_count: 0,
            health: Default::default(),
        })
        .collect();
    cabin.add_automation_seed("every day at 9, summarize the board");
    assert_eq!(cabin.status, "Maximum 50 scheduled automations");
    assert_eq!(cabin.automations.len(), 50);
    assert!(cabin.grok_loops.is_empty());
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #209.
#[test]
fn imagine_while_busy_stays_halted() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("imagine-busy");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.imagine_prompt = "a red boat".into();
    cabin.running = true;
    cabin.kick_imagine();
    assert_eq!(cabin.status, "Halt the live job before Imagine, or wait.");
    assert!(cabin.running);
    assert!(cabin.imagine_error.is_empty());
}

// Landed from PR #210.
#[test]
fn empty_update_plan_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("empty-update-plan");
    let prev = std::env::var("GROKHUB_CONFIG").ok();
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut app = Cabin::quiet_for_test();
    app.start_overlay_update(Vec::new());
    assert_eq!(app.status, "Update plan empty");
    assert!(matches!(app.nav, Nav::Settings));
    assert!(matches!(app.settings_sec, SettingsSec::Update));
    assert!(!app.running);
    assert!(app.queued_overlay.is_none());
    match prev {
        Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
        None => std::env::remove_var("GROKHUB_CONFIG"),
    }
}

// Landed from PR #212.
#[test]
fn imagine_without_key_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("imagine-key");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.secrets.oauth = None;
    cabin.imagine_native.tokens = None;
    cabin.imagine_prompt = "a red boat".into();

    cabin.kick_imagine();

    let expected = "Sign in with Grok in Settings → Account, or add a console API key.";
    assert_eq!(cabin.status, expected);
    assert_eq!(cabin.imagine_error, expected);
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #213.
#[test]
fn busy_update_stays_queued() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("busy_update_stays_queued");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.running = true;
    cabin.start_overlay_update(vec!["echo hi".into()]);
    assert_eq!(
        cabin.status,
        "Update queued — it starts when this job finishes."
    );
    assert_eq!(cabin.queued_overlay, Some(vec!["echo hi".into()]));
    assert!(cabin.running);
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(matches!(cabin.settings_sec, SettingsSec::Update));
}

// Landed from PR #214.
#[test]
fn rename_chat_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("rename-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    let prev = std::env::var("GROKHUB_CONFIG").ok();
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let thread_idx = cabin.thread_idx;
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.running);

    cabin.rename_thread(thread_idx, "");
    assert_eq!(cabin.status, "Kept Chat");
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.threads[thread_idx].title_locked);

    cabin.rename_thread(thread_idx, "Harbor");
    assert_eq!(cabin.status, "Renamed Harbor");
    assert_eq!(cabin.threads[thread_idx].title, "Harbor");
    assert!(cabin.threads[thread_idx].title_locked);
    assert!(!cabin.running);

    match prev {
        Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
        None => std::env::remove_var("GROKHUB_CONFIG"),
    }
}

// Landed from PR #215.
#[test]
fn pin_chat_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("pin-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert!(!cabin.running);

    cabin.pin_thread(cabin.thread_idx);
    assert_eq!(cabin.status, "Pinned Chat");
    assert!(cabin.threads[cabin.thread_idx].pinned);
    assert!(!cabin.running);

    cabin.pin_thread(cabin.thread_idx);
    assert_eq!(cabin.status, "Unpinned Chat");
    assert!(!cabin.threads[cabin.thread_idx].pinned);
    assert!(!cabin.running);
}

// Landed from PR #216.
#[test]
fn fresh_home_keeps_the_old_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("fresh-home");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "hello".into())]);

    cabin.open_fresh_home();

    assert_eq!(cabin.status, "New chat");
    assert!(cabin.messages.is_empty());
    let kept = cabin.threads.iter().enumerate().any(|(i, thread)| {
        i != cabin.thread_idx
            && thread
                .messages
                .iter()
                .any(|(role, line)| role == "user" && line == "hello")
    });
    assert!(kept);
    assert!(!cabin.running);
    assert!(matches!(cabin.nav, Nav::Chat));
}

// Landed from PR #217.
#[test]
fn delete_one_chat_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("delete-one-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.threads[cabin.thread_idx].title = "Harbor".into();
    cabin.messages = Arc::new(vec![("user".into(), "hello".into())]);
    cabin.new_thread(false);

    let harbor = cabin
        .threads
        .iter()
        .position(|t| t.title == "Harbor")
        .expect("Harbor");
    cabin.delete_thread_at(harbor);

    assert_eq!(cabin.status, "Deleted Harbor");
    assert!(cabin.threads.iter().all(|t| t.title != "Harbor"));
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());

    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #218.
#[test]
fn scratch_chat_stays_off_a_run() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("scratch-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.new_thread(true);
    assert_eq!(cabin.status, "Scratch — no memory writes");
    let thread = cabin.threads.get(cabin.thread_idx).expect("current thread");
    assert_eq!(thread.title, "Scratch");
    assert!(thread.scratch);
    assert!(!cabin.running);
}

// Landed from PR #219.
#[test]
fn delete_last_chat_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("delete-last");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.delete_thread_at(0);
    assert_eq!(cabin.status, "Chat deleted");
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].title, "Chat");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #220.
#[test]
fn land_on_real_chat_leaves_scratch() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("land-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.new_thread(true);
    let current = &cabin.threads[cabin.thread_idx];
    assert_eq!(current.title, "Scratch");
    assert!(current.scratch);
    assert!(cabin
        .threads
        .iter()
        .enumerate()
        .any(|(i, t)| { i != cabin.thread_idx && t.title == "Chat" && !t.scratch }));
    assert_eq!(cabin.status, "Scratch — no memory writes");
    cabin.land_on_real_chat();
    let current = &cabin.threads[cabin.thread_idx];
    assert_eq!(current.title, "Chat");
    assert!(!current.scratch);
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(cabin
        .threads
        .iter()
        .enumerate()
        .any(|(i, t)| { i != cabin.thread_idx && t.title == "Scratch" && t.scratch }));
    assert_eq!(cabin.status, "Scratch — no memory writes");
    assert!(!cabin.running);
}

// Landed from PR #221.
#[test]
fn plan_pill_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("plan-pill");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert!(!cabin.threads[cabin.thread_idx].title_locked);
    cabin.select_plan_without_rename();
    assert_eq!(cabin.status, "Session plan");
    assert!(matches!(cabin.session_mode, SessionMode::Plan));
    assert_eq!(cabin.cfg.session_mode, "plan");
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert!(!cabin.threads[cabin.thread_idx].title_locked);
    assert!(cabin.acp.is_none());
    assert!(!cabin.running);
}

// Landed from PR #222.
#[test]
fn recent_chat_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("recent-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let idx = cabin.thread_idx;
    assert_eq!(cabin.threads[idx].title, "Chat");
    assert_eq!(cabin.status, "New chat");
    cabin.open_recent_chat();
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert_eq!(cabin.thread_idx, idx);
    assert!(cabin.composer_want_focus);
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);
    assert_eq!(cabin.threads.len(), 1);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #223.
#[test]
fn begin_rename_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("begin-rename");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let thread_idx = cabin.thread_idx;
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.threads[thread_idx].title_locked);
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);
    cabin.begin_chat_rename(thread_idx);
    assert_eq!(cabin.rename_idx, Some(thread_idx));
    assert!(cabin.rename_focus);
    assert_eq!(cabin.rename_buf, "Chat");
    assert_eq!(cabin.rename_lock, Some("Chat".into()));
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.threads[thread_idx].title_locked);
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);
}

// Landed from PR #224.
#[test]
fn imagine_page_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    app.set_nav_id("imagine");
    assert_eq!(app.nav, Nav::Imagine);
    assert!(app.imagine_want_focus);
    assert_eq!(app.nav_id(), "imagine");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    app.set_nav_id("history");
    assert_eq!(app.nav, Nav::History);
    assert_eq!(app.nav_id(), "history");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
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

// Landed from PR #226.
#[test]
fn switch_chat_keeps_the_line() {
    let _cfg = crate::config::hold_test_config();
    let root = crate::config::test_config_root("switch-chat-keeps");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut app = Cabin::quiet_for_test();
    let mut held = crate::threads::ChatThread::new("Chat", false);
    held.messages = std::sync::Arc::new(vec![("user".into(), "older line".into())]);
    app.threads.push(held);
    app.thread_idx = 0;
    app.messages = app.threads[0].messages.clone();
    app.new_thread(false);
    let left = app.thread_idx;
    assert!(left > 0);
    app.live_mut().push(("user".into(), "harbor line".into()));
    app.apply_switch_thread(0);
    assert_eq!(app.thread_idx, 0);
    assert!(app.messages.iter().all(|(_, c)| c != "harbor line"));
    assert!(app.threads[left]
        .messages
        .iter()
        .any(|(_, c)| c == "harbor line"));
    assert!(app.composer_want_focus);
    assert!(app.rename_idx.is_none());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    assert_eq!(app.status, "New chat");

    std::env::remove_var("GROKHUB_CONFIG");
    let _ = std::fs::remove_dir_all(&root);
}

// Landed from PR #227.
#[test]
fn stage_project_asks_for_a_name() {
    let mut app = Cabin::quiet_for_test();
    let before = app.projects.len();
    app.stage_new_folder();
    assert_eq!(app.status, "Name this folder");
    assert_eq!(app.projects.len(), before + 1);
    let id = app.proj_staged.clone().expect("staged");
    assert_eq!(app.proj_rename.as_deref(), Some(id.as_str()));
    assert!(app.proj_rename_buf.is_empty());
    assert!(app.proj_rename_focus);
    assert!(app.proj_rename_lock.is_none());
    let node = app.projects.iter().find(|n| n.id == id).expect("node");
    assert_eq!(node.name, "Folder");
    assert!(node.path.is_empty());
    assert_eq!(node.kind, ProjectKind::Folder);
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Landed from PR #228.
#[test]
fn blank_project_name_drops_it() {
    let mut app = Cabin::quiet_for_test();
    app.stage_new_folder();
    let id = app.proj_staged.clone().expect("staged");
    app.finish_proj_rename();
    assert_eq!(app.status, "need a name");
    assert!(app.proj_staged.is_none());
    assert!(app.proj_rename.is_none());
    assert!(!app.proj_rename_focus);
    assert!(app.projects.iter().all(|n| n.id != id));
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Landed from PR #230.
#[test]
fn cancel_project_drops_the_staged_one() {
    let mut app = Cabin::quiet_for_test();
    app.stage_new_folder();
    let id = app.proj_staged.clone().expect("staged");
    app.cancel_proj_rename();
    assert!(app.proj_staged.is_none());
    assert!(app.proj_rename.is_none());
    assert!(!app.proj_rename_focus);
    assert!(app.proj_rename_lock.is_none());
    assert!(app.proj_rename_buf.is_empty());
    assert!(app.projects.iter().all(|n| n.id != id));
    assert_eq!(app.status, "Name this folder");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

/// Projects “+” paints Cancel next to Name. That control calls the same
/// `cancel_proj_rename` Esc uses, which drops the staged folder.
#[test]
fn projects_add_cancel_drops_the_staged_folder() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("proj-add-cancel");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.stage_new_folder();
    let id = cabin.proj_staged.clone().expect("staged folder");
    assert_eq!(cabin.projects.len(), 1);
    assert_eq!(cabin.projects[0].name, "Folder");
    assert_eq!(cabin.projects[0].kind, ProjectKind::Folder);
    assert_eq!(cabin.proj_rename.as_deref(), Some(id.as_str()));
    assert_eq!(cabin.status, "Name this folder");

    cabin.cancel_proj_rename();
    assert!(cabin.projects.is_empty(), "Cancel drops the staged folder");
    assert!(cabin.projects.iter().all(|n| n.id != id));
    assert!(cabin.proj_staged.is_none());
    assert!(cabin.proj_rename.is_none());
    assert!(cabin.proj_rename_buf.is_empty());
    assert!(!cabin.proj_rename_focus);
    assert!(cabin.proj_rename_lock.is_none());
    assert_eq!(cabin.status, "Name this folder");
    assert!(!cabin.running);

    let side = include_str!("sidebar.rs");
    let row = side
        .split("hint_text(crate::theme::hint(\"Name\"))")
        .nth(1)
        .and_then(|s| s.split("self.finish_proj_rename()").next())
        .expect("proj rename row");
    assert!(
        row.contains("ghost_pill(ui, \"Cancel\")"),
        "the rename row paints Cancel beside Name: {row}"
    );
    assert!(
        row.contains("egui::Key::Escape"),
        "Esc still cancels the rename: {row}"
    );
    assert_eq!(
        row.matches("self.cancel_proj_rename()").count(),
        2,
        "Cancel click and Esc both call cancel_proj_rename: {row}"
    );
}

/// Name and Cancel share one centered row: pill height, filled field, white focus stroke.
#[test]
fn projects_rename_row_lines_up_with_cancel() {
    let side = include_str!("sidebar.rs");
    let row = side
        .split("if self.proj_rename.as_deref()")
        .nth(1)
        .and_then(|s| s.split("self.finish_proj_rename()").next())
        .expect("proj rename row");
    assert!(
        row.contains("Layout::left_to_right(egui::Align::Center)"),
        "rename row is vertically centered: {row}"
    );
    assert!(
        row.contains(".min_size(egui::vec2(edit_w, 30.0))"),
        "Name field is the pill height: {row}"
    );
    assert!(
        row.contains("Margin::symmetric(") && row.contains("10, 6"),
        "Name field has ~10px left padding: {row}"
    );
    assert!(
        row.contains("crate::theme::elevated()"),
        "Name field uses the filter fill: {row}"
    );
    assert!(
        row.contains("corner_radius(15.0)"),
        "Name field is full radius: {row}"
    );
    assert!(
        row.contains("crate::theme::muted()"),
        "Name hint is muted: {row}"
    );
    assert!(
        row.contains("2.0_f32") && row.contains("crate::theme::fg()"),
        "staged Name field draws a white focus stroke: {row}"
    );
    assert!(
        row.contains("text_cursor.stroke"),
        "staged Name field shows a caret: {row}"
    );
    assert!(
        row.contains("edit.request_focus()"),
        "staged Name field takes focus: {row}"
    );
    assert!(
        row.contains("Layout::right_to_left(egui::Align::Center)"),
        "Cancel's right edge meets the rail inset: {row}"
    );
    assert!(row.contains("cancel_w"), "{row}");
}

// Landed from PR #231.
#[test]
fn make_project_keeps_harbor() {
    let mut app = Cabin::quiet_for_test();
    let before = app.projects.len();
    app.make_project("Harbor", None);
    assert_eq!(app.status, "Project Harbor");
    assert_eq!(app.projects.len(), before + 1);
    let node = app
        .projects
        .iter()
        .find(|n| n.name == "Harbor")
        .expect("harbor");
    assert_eq!(node.kind, ProjectKind::Project);
    assert!(!node.path.trim().is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Landed from PR #232.
#[test]
fn empty_project_name_is_refused() {
    let mut app = Cabin::quiet_for_test();
    let before = app.projects.len();
    app.make_project("   ", None);
    assert_eq!(app.status, "need a project name");
    assert_eq!(app.projects.len(), before);
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

#[test]
fn edit_puts_your_message_back_without_dropping_a_draft() {
    assert_eq!(
        super::chat_ui::edit_into_composer("", "fix the tests"),
        "fix the tests"
    );
    assert_eq!(
        super::chat_ui::edit_into_composer("  \n", "fix the tests"),
        "fix the tests"
    );
    assert_eq!(
        super::chat_ui::edit_into_composer("half typed\n", "fix the tests"),
        "half typed\n\nfix the tests"
    );
    assert_eq!(
        super::chat_ui::msg_act_labels(true),
        &["Copy", "Edit", "Reply"]
    );
    assert_eq!(super::chat_ui::msg_act_labels(false), &["Copy", "Reply"]);
    let src = cabin_src();
    let chat = fn_src(&src, "ui_chat");
    assert!(chat.contains("ChatBlockAct::Edit(body)"), "{chat}");
    assert!(chat.contains("edit_into_composer"), "{chat}");
}

#[test]
fn user_act_row_with_edit_stays_inside_a_narrow_pane() {
    for width in [300.0_f32, 360.0, 800.0] {
        let ctx = egui::Context::default();
        let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
            let ctx = ui.ctx().clone();
            crate::theme::apply(&ctx, true);
            egui::CentralPanel::default().show(ui, |ui| {
                ui.allocate_ui(egui::vec2(width, 200.0), |ui| {
                    ui.set_max_width(width);
                    let row = ui.max_rect();
                    let bubble = super::chat_ui::paint_speech_bubble(ui, "hey", true, false);
                    let acts = super::chat_ui::paint_msg_acts(
                        ui,
                        true,
                        "hey",
                        row.width(),
                        bubble.rect.width(),
                    );
                    assert!(
                        acts.row.min.x >= row.min.x - 0.5,
                        "width {width}: {:?}",
                        acts.row
                    );
                    assert!(
                        acts.row.max.x <= row.max.x + 1.0,
                        "width {width}: {:?}",
                        acts.row
                    );
                    let want = super::chat_ui::msg_acts_row_width(ui, true);
                    assert!(
                        acts.row.width() + 1.0 >= want,
                        "width {width}: Edit row clipped {} < {want}",
                        acts.row.width()
                    );
                });
            });
        });
    }
}

fn find_views(bodies: &[(&str, &str)]) -> Vec<ChatView> {
    grokhub_core::visible_chat_refs(bodies.iter().copied())
}

#[test]
fn chat_find_starts_on_the_newest_hit_and_wraps() {
    let views = find_views(&[
        ("user", "build the cabin"),
        ("assistant", "Building it."),
        ("user", "now test"),
        ("assistant", "Tests pass. Build is green."),
    ]);
    let mut find = super::chat_ui::ChatFind::default();
    find.toggle();
    assert!(find.open && find.want_focus);
    find.query = "BUILD".into();
    find.refresh("t1", &views);
    let hits = find.hits().to_vec();
    assert_eq!(hits.len(), 3, "{hits:?}");
    assert_eq!(find.current_row(), hits.last().copied());
    assert!(find.jump, "a new query scrolls to its hit");
    assert_eq!(find.label(), "3 of 3");
    find.jump = false;
    find.step(true);
    assert_eq!(find.current_row(), Some(hits[0]));
    assert!(find.jump);
    find.step(false);
    assert_eq!(find.label(), "3 of 3");
    // Same query, same transcript: no recount, pick stays.
    find.step(false);
    find.refresh("t1", &views);
    assert_eq!(find.label(), "2 of 3");
    find.query = "zzz".into();
    find.refresh("t1", &views);
    assert_eq!(find.label(), "No matches");
    assert!(find.current_row().is_none());
    find.close();
    assert!(!find.open && !find.focused);
    assert!(find.hits().is_empty(), "a closed find marks nothing");
}

#[test]
fn chat_find_is_ctrl_f_on_chat_and_owns_enter_over_a_permission_card() {
    let src = cabin_src();
    assert!(src.contains("egui::Key::F"), "Ctrl+F must open find");
    assert!(src.contains("self.find.toggle()"));
    let chat = fn_src(&src, "ui_chat");
    assert!(chat.contains("paint_find_bar"), "{chat}");
    assert!(chat.contains("find_jump && find_row == Some(i)"), "{chat}");
    assert!(
        src.contains("self.palette_open || self.nav == Nav::Settings || self.find.focused"),
        "Enter in the find box must not allow a tool"
    );
    let confirm = include_str!("confirm.rs");
    assert!(
        confirm.contains("self.find.focused"),
        "Enter in the find box must not confirm a sheet"
    );
    assert!(grokhub_core::shortcut_help().contains("Ctrl+F"));
}

#[test]
fn chat_find_bar_paints_and_steps_with_the_arrows() {
    let mut app = Cabin::quiet_for_test();
    app.find.toggle();
    app.find.query = "x".into();
    let views = find_views(&[("user", "x one"), ("assistant", "x two")]);
    app.find.refresh("t", &views);
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        let ctx = ui.ctx().clone();
        crate::theme::apply(&ctx, true);
        app.paint_find_bar(
            &ctx,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
        );
    });
    assert!(app.find.open);
    assert_eq!(app.find.label(), "2 of 2");
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

/// Corrupt settings, keys, Home feed and hub files: each loads defaults, keeps its
/// bytes in a `.corrupt-<ms>` copy, and gets one Home card naming it and the copy.
#[test]
fn corrupt_stores_load_defaults_keep_a_copy_and_post_a_named_card() {
    let _g = config::hold_test_config();
    let root = config::test_config_root("corrupt-stores");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let _pin = config::TestConfigDir::set(root.clone());
    let mut cabin = Cabin::quiet_for_test();
    let _ = grokhub_core::recovered::drain();
    let files = [
        ("app.json", "{\"deviceName\": \"cab", "Settings file"),
        ("secrets.json", "\u{0}\u{1}garbage", "Keys file"),
        ("updates.json", "[{\"id\": 3,", "Home feed"),
        ("hub-state.json", "not json", "Hub pairing file"),
    ];
    for (name, bytes, _) in files {
        std::fs::write(root.join(name), bytes).unwrap();
    }

    let cfg = config::load();
    assert_eq!(
        cfg.device_name,
        config::default_device_name(),
        "settings fell back to defaults"
    );
    assert_eq!(
        crate::secrets::load().api_key,
        "",
        "keys fell back to empty"
    );
    assert!(
        crate::feed::load().is_empty(),
        "the feed fell back to empty"
    );
    assert!(grokhub_core::load_hub_state(&config::hub_state_path()).is_none());
    // Checked before the cards post: posting saves a fresh updates.json on a
    // background thread, which could land before a later check.
    for (name, _, _) in files {
        assert!(!root.join(name).exists(), "{name} was moved aside");
    }
    cabin.poll_recovered_stores();

    for (name, bytes, label) in files {
        let copies: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&format!("{name}.corrupt-")))
            })
            .collect();
        assert_eq!(copies.len(), 1, "{name}: {copies:?}");
        assert_eq!(
            std::fs::read_to_string(&copies[0]).unwrap(),
            bytes,
            "{name}: the copy keeps the bytes"
        );
        let title = format!("{label} was unreadable");
        let cards: Vec<&UpdateCard> = cabin.updates.iter().filter(|c| c.title == title).collect();
        assert_eq!(cards.len(), 1, "{name}: {:?}", cabin.updates);
        let body = format!(
            "Restored defaults, kept a copy at {}.",
            grokhub_core::recovered::home_short(&copies[0])
        );
        assert_eq!(cards[0].body.as_deref(), Some(body.as_str()), "{name}");
    }
    // Draining again posts nothing new.
    let before = cabin.updates.len();
    cabin.poll_recovered_stores();
    assert_eq!(cabin.updates.len(), before);
    let _ = std::fs::remove_dir_all(&root);
}

fn health_job(id: &str) -> grokhub_core::Automation {
    grokhub_core::Automation {
        id: id.into(),
        name: "Board summary".into(),
        schedule: "daily".into(),
        time: "09:00".into(),
        times: Vec::new(),
        instructions: "summarize the board".into(),
        heartbeat_every_min: 0,
        check_command: String::new(),
        enabled: true,
        last_run: None,
        next_run: None,
        run_count: 0,
        health: Default::default(),
    }
}

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

fn pin_auto_config(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
    let root = crate::config::test_config_root(label);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    (crate::config::TestConfigDir::set(root.clone()), root)
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
    std::env::remove_var("GROKHUB_CONFIG");
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
    std::env::remove_var("GROKHUB_CONFIG");
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
    std::env::remove_var("GROKHUB_CONFIG");
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
    std::env::remove_var("GROKHUB_CONFIG");
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
    std::env::remove_var("GROKHUB_CONFIG");
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
    std::env::remove_var("GROKHUB_CONFIG");
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
    std::env::remove_var("GROKHUB_CONFIG");
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
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn token_budget_warns_once_and_holds_scheduled_work() {
    let root = config::test_config_root("token-budget");
    let _ = std::fs::remove_dir_all(&root);
    let _pin = config::TestConfigDir::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    // Quiet all day so the test never pings the desktop.
    app.cfg.quiet_start = "00:00".into();
    app.cfg.quiet_end = "23:59".into();
    app.cfg.daily_token_budget = 1_000;
    app.cfg.budget_pauses_scheduled = true;
    app.usage.day = Cabin::local_day();

    let mut u = GrokUsage {
        input_tokens: 500,
        ..Default::default()
    };
    app.merge_grok_usage(&u);
    assert!(!app.status.contains("Token budget"), "{}", app.status);
    assert!(!app.budget_holds_scheduled());

    u.input_tokens = 850;
    app.merge_grok_usage(&u);
    assert!(
        app.status.starts_with("Token budget at 80%"),
        "{}",
        app.status
    );
    app.status.clear();
    u.input_tokens = 900;
    app.merge_grok_usage(&u);
    assert!(app.status.is_empty(), "near warns once: {}", app.status);

    u.input_tokens = 1_200;
    app.merge_grok_usage(&u);
    assert!(
        app.status.starts_with("Token budget used up"),
        "{}",
        app.status
    );
    assert!(
        app.status.contains("waits until tomorrow"),
        "{}",
        app.status
    );
    assert!(app.budget_holds_scheduled());
    app.cfg.budget_pauses_scheduled = false;
    assert!(!app.budget_holds_scheduled(), "pause off: warn only");
    app.cfg.budget_pauses_scheduled = true;
    app.cfg.daily_token_budget = 0;
    assert!(!app.budget_holds_scheduled(), "no budget: never hold");

    let (title, body) = super::token_budget_notice(
        grokhub_core::BudgetLevel::Over,
        "1.2k of 1000 tokens today (120%)",
        false,
    );
    assert_eq!(title, "Token budget used up");
    assert!(!body.contains("tomorrow"), "{body}");
}

#[test]
fn over_budget_holds_night_loops_and_anticipate() {
    let src = cabin_src();
    for f in ["tick_night", "tick_loops"] {
        let body = fn_src(&src, f);
        assert!(
            body.contains("self.budget_holds_scheduled()"),
            "{f}: {body}"
        );
    }
    let anticipate_hold = src.matches("if self.budget_holds_scheduled() {").count();
    assert!(
        anticipate_hold >= 3,
        "night, loops, and anticipate: {anticipate_hold}"
    );
    let settings = include_str!("settings.rs");
    assert!(settings.contains("Daily token budget"));
    assert!(settings.contains("Pause scheduled work over budget"));
    let old: config::AppConfig = serde_json::from_str("{}").expect("empty app.json loads");
    assert_eq!(old.daily_token_budget, 0);
    assert!(old.budget_pauses_scheduled);
    let saved = serde_json::to_string(&config::AppConfig::default()).unwrap();
    assert!(
        !saved.contains("dailyTokenBudget") && !saved.contains("daily_token_budget"),
        "off writes nothing"
    );
}

#[test]
fn dropping_files_says_what_happens() {
    use super::plus::{drop_extra_note, drop_hint_line};
    assert_eq!(
        drop_hint_line(false, 1),
        "Drop to attach to your next message"
    );
    assert_eq!(
        drop_hint_line(true, 1),
        "Drop to use as the Imagine reference"
    );
    assert!(drop_hint_line(false, 3).ends_with("3 files"));
    assert_eq!(drop_extra_note(0), None);
    assert!(drop_extra_note(1).unwrap().contains("1 more file"));
    assert!(drop_extra_note(4).unwrap().contains("4 more file"));
    let app = Cabin::quiet_for_test();
    let ctx = egui::Context::default();
    let mut input = egui::RawInput::default();
    input.hovered_files.push(egui::HoveredFile::default());
    let out = crate::theme::test_pass(&ctx, input, |ui| {
        let ctx = ui.ctx().clone();
        crate::theme::apply(&ctx, true);
        app.paint_drop_hint(&ctx);
    });
    assert!(
        !out.shapes.is_empty(),
        "hovering a file paints the drop hint"
    );
    let quiet = crate::theme::test_pass(&ctx, egui::RawInput::default(), |ui| {
        let ctx = ui.ctx().clone();
        app.paint_drop_hint(&ctx);
    });
    assert!(quiet.shapes.is_empty(), "no hover, no overlay");
    let src = cabin_src();
    assert!(fn_src(&src, "take_dropped_attach").contains("self.drop_extra"));
}

#[test]
fn export_writes_html_and_json_next_to_export_md() {
    let mut quiet = QuietCabin::boot("slash-export-formats");
    let cabin = &mut quiet.cabin;
    cabin.messages = std::sync::Arc::new(vec![
        ("user".into(), "harbor <b>export</b> line".into()),
        ("assistant".into(), "1. **one**\n2. two".into()),
    ]);
    cabin.cfg.project_dir.clear();

    cabin.run_slash_line("/export html");
    let html_path = cabin
        .status
        .strip_prefix("Wrote ")
        .unwrap_or("")
        .to_string();
    assert!(html_path.ends_with("export.html"), "{}", cabin.status);
    wait_for("export.html must hold the escaped chat", || {
        let page = std::fs::read_to_string(&html_path).unwrap_or_default();
        page.contains("harbor &lt;b&gt;export&lt;/b&gt; line")
            && page.contains("<strong>one</strong>")
    });

    cabin.run_slash_line("/export json");
    let json_path = cabin
        .status
        .strip_prefix("Wrote ")
        .unwrap_or("")
        .to_string();
    assert!(json_path.ends_with("export.json"), "{}", cabin.status);
    wait_for("export.json must parse and keep both turns", || {
        std::fs::read_to_string(&json_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .is_some_and(|v| v["messages"].as_array().is_some_and(|m| m.len() == 2))
    });

    cabin.run_slash_line("/export md");
    assert!(cabin.status.ends_with("export.md"), "{}", cabin.status);
    cabin.run_slash_line("/export pdf");
    assert_eq!(cabin.status, grokhub_core::EXPORT_FORMATS_HINT);
    assert!(!cabin.running);
}

// ---- Folded Cursor test drafts (#259, #266, #433–#446, reviewed 2026-10-04). Each test notes its source PR. ----

// Folded from PR #266. Test only: `append_composer` already drops whitespace-only text, so the PR's code guard was redundant.
#[test]
fn chat_paste_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    app.composer = "chat line".into();
    app.imagine_prompt = "sunset".into();
    app.apply_clipboard(PlusTarget::Chat, "over the harbor\n");
    assert_eq!(app.composer, "chat line\nover the harbor");
    assert_eq!(app.imagine_prompt, "sunset");
    assert_eq!(app.status, "Pasted clipboard");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    app.apply_clipboard(PlusTarget::Chat, "   \n");
    assert_eq!(app.composer, "chat line\nover the harbor");
    assert_eq!(app.imagine_prompt, "sunset");
    assert_eq!(app.status, "Pasted clipboard");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #259. Test only, for the same reason as #266.
#[test]
fn imagine_paste_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    app.composer = "chat line".into();
    app.imagine_prompt = "sunset".into();
    app.apply_clipboard(PlusTarget::Imagine, "over the harbor\n");
    assert_eq!(app.imagine_prompt, "sunset\nover the harbor");
    assert_eq!(app.composer, "chat line");
    assert_eq!(app.status, "Pasted clipboard");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    app.apply_clipboard(PlusTarget::Imagine, "   \n");
    assert_eq!(app.imagine_prompt, "sunset\nover the harbor");
    assert_eq!(app.status, "Pasted clipboard");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #433. The Err path saves suggestions on a worker thread, which
// can't see a thread-local test dir: point GROKHUB_CONFIG at a temp root and wait
// for the save, so the test never writes the real suggestions.json.
#[test]
fn apply_review_reply_err_stays_idle() {
    let _hold = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("review-reply-err");
    std::fs::create_dir_all(&root).expect("config root");
    assert!(cabin.suggestions.last_review_day.is_none());
    assert!(!cabin.host_diff_kick);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());

    // Err only: Ok would parse suggestions and spawn more work.
    cabin.apply_review_reply(Err("timeout".into()));

    assert_eq!(cabin.status, "Nightly review held — timeout");
    assert_eq!(
        cabin.suggestions.last_review_day.as_deref(),
        Some(Cabin::local_day().as_str())
    );
    assert!(!cabin.host_diff_kick);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
    let saved = root.join("suggestions.json");
    let start = std::time::Instant::now();
    while !saved.exists() && start.elapsed() < std::time::Duration::from_secs(3) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let body = std::fs::read_to_string(&saved).expect("held review saved under the test root");
    assert!(body.contains(&Cabin::local_day()), "{body}");
    release_isolated(&root, cabin);
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

// Folded from PR #436.
#[test]
fn name_project_keeps_harbor() {
    let mut app = Cabin::quiet_for_test();
    app.stage_new_folder();
    let id = app.proj_staged.clone().expect("staged");
    app.proj_rename_buf = "Harbor".into();
    app.finish_proj_rename();
    assert_eq!(app.status, "Renamed Harbor");
    assert!(app.proj_staged.is_none());
    assert!(app.proj_rename.is_none());
    assert!(!app.proj_rename_focus);
    assert!(app.proj_rename_buf.is_empty());
    let node = app.projects.iter().find(|n| n.id == id).expect("named");
    assert_eq!(node.name, "Harbor");
    assert_eq!(node.kind, ProjectKind::Folder);
    assert!(node.path.is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #441, plus the matching and other-project rows (an idle-only
// check would pass against a stub that always returns None).
#[test]
fn dream_rewind_id_matches_only_this_project() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.rewind_rows.is_empty());
    assert_eq!(cabin.dream_rewind_id(), None);
    cabin.cfg.project_dir = "/tmp/harbor-proj".into();
    cabin.rewind_rows.push(RewindRecord {
        job_id: "job-7".into(),
        path: "/tmp/harbor-snap".into(),
        root: "/tmp/harbor-proj".into(),
        created_at: 1,
        method: "copy".into(),
    });
    assert_eq!(cabin.dream_rewind_id(), Some("job-7"));
    cabin.cfg.project_dir = "/tmp/other-proj".into();
    assert_eq!(cabin.dream_rewind_id(), None);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #442.
#[test]
fn update_probe_poll_clears_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.last_update_probe = Some(std::time::Instant::now());
    let ctx = egui::Context::default();
    cabin.poll_update_probe(&ctx);
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.update_probe_rx.is_none());
    let (tx, rx) = std::sync::mpsc::channel::<crate::update::UpdateProbe>();
    cabin.update_probe_rx = Some(rx);
    cabin.poll_update_probe(&ctx);
    assert!(cabin.update_probe_rx.is_some());
    assert_eq!(cabin.status, "Harbor");
    drop(tx);
    cabin.poll_update_probe(&ctx);
    assert!(cabin.update_probe_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #443.
#[test]
fn palette_search_poll_clears_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_palette_search();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.palette_file_rx.is_none());
    let (tx, rx) = std::sync::mpsc::channel::<(String, String, Vec<String>)>();
    cabin.palette_file_rx = Some(rx);
    cabin.poll_palette_search();
    assert!(cabin.palette_file_rx.is_some());
    assert_eq!(cabin.status, "Harbor");
    drop(tx);
    cabin.poll_palette_search();
    assert!(cabin.palette_file_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #446, plus the two commands that do flip it (idle-only would
// pass against a stub).
#[test]
fn note_cabin_update_landed_flips_only_for_its_steps() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.last_host.is_empty());
    cabin.note_cabin_update_landed();
    assert!(!cabin.cabin_overlay_done);
    cabin.last_host = vec!["ls -la".into()];
    cabin.note_cabin_update_landed();
    assert!(!cabin.cabin_overlay_done);
    cabin.last_host = vec!["grok update".into()];
    cabin.note_cabin_update_landed();
    assert!(
        !cabin.cabin_overlay_done,
        "a CLI update is not the cabin's step"
    );
    cabin.last_host = vec!["git -C ~/GrokHub pull --ff-only".into()];
    cabin.note_cabin_update_landed();
    assert!(cabin.cabin_overlay_done);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// ---- Folded Cursor test drafts (#251–#431, base v2.10.54). Each test notes its source PR. ----

// ---- Cursor fold: Connectors and Grok Build CLI polls ----

// Folded from PR #323.
#[test]
fn single_poll_reports_a_drop() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("single-poll-drop");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    let _pin = crate::config::TestConfigDir::set(root.clone());

    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert!(cabin.stream_buf.is_empty() && cabin.thought_buf.is_empty());

    cabin.poll_single();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.grok_p_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<GrokPEvent>();
    cabin.grok_p_rx = Some(rx);
    cabin.running = true;
    cabin.chat_job_thread = Some(cabin.visible_thread_id());
    cabin.poll_single();
    assert!(cabin.grok_p_rx.is_some());
    assert!(cabin.running);
    assert_eq!(cabin.status, "Harbor");

    // Empty buffers → Disconnected else arm (not finish_acp_turn).
    assert!(cabin.stream_buf.is_empty() && cabin.thought_buf.is_empty());
    drop(tx);
    cabin.poll_single();
    assert!(cabin.grok_p_rx.is_none());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert_eq!(cabin.status, "The engine stopped without a reply");
    assert!(cabin.stream_buf.is_empty() && cabin.thought_buf.is_empty());

    let _ = std::fs::remove_dir_all(&root);
}

// Folded from PR #326.
#[test]
fn acp_poll_stays_off_without_handle() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert!(cabin.acp.is_none());
    cabin.poll_acp();
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.acp.is_none());
}

// ---- Cursor fold: Voice and hotkeys ----

// Folded from PR #254.
// Voice is on Ready and the cabin is not running, so leaving voice must not halt a run.
#[test]
fn leave_voice_while_idle_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    app.voice_state = grokhub_core::VoiceState::Ready;
    app.voice_orb = "live".into();
    app.leave_voice();
    assert!(matches!(app.voice_state, grokhub_core::VoiceState::Idle));
    assert_eq!(app.voice_orb, "idle");
    assert_eq!(app.status, "Voice off");
    assert!(app.voice_hold_rx.is_none());
    assert!(app.voice_ready_at.is_none());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #263.
#[test]
fn voice_hold_while_idle_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    app.voice_hold_rx = Some(rx);
    app.voice_state = VoiceState::Idle;
    app.poll_voice_hold();
    assert!(app.voice_hold_rx.is_some());
    assert!(matches!(app.voice_state, VoiceState::Idle));
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    drop(tx);
    app.poll_voice_hold();
    assert!(app.voice_hold_rx.is_none());
    assert!(matches!(app.voice_state, VoiceState::Idle));
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #290.
#[test]
fn voice_mode_is_on_when_ready() {
    let mut app = Cabin::quiet_for_test();
    app.voice_state = VoiceState::Idle;
    assert!(!app.voice_is_on());
    app.voice_state = VoiceState::Ready;
    assert!(app.voice_is_on());
    app.voice_state = VoiceState::Idle;
    assert!(!app.voice_is_on());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #307.
#[test]
fn global_hotkeys_poll_stays_off_without_manager() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    // quiet_for_test leaves hotkeys None — early return before listen_voice / halt_work.
    assert!(cabin.hotkeys.is_none());
    cabin.poll_global_hotkeys();
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(matches!(cabin.voice_state, VoiceState::Idle));
    assert!(!cabin.voice_is_on());
    assert!(cabin.chat_job_thread.is_none());
}

// ---- Cursor fold: Secrets, skills and memory ----

// Folded from PR #358.
#[test]
fn hold_secret_skips_short_and_keeps_one() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.secret_hold.is_empty());
    // Short value (<4 chars) is skipped; no network.
    cabin.hold_secret("ab");
    cabin.hold_secret("abc");
    assert!(cabin.secret_hold.is_empty());
    // Longer secret held once; duplicate ignored.
    let held = "held-token-xyz";
    cabin.hold_secret(held);
    assert_eq!(cabin.secret_hold.len(), 1);
    cabin.hold_secret(held);
    assert_eq!(cabin.secret_hold.len(), 1);
}

// Folded from PR #364.
#[test]
fn mark_get_started_done_sets_flag() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("mark-get-started-done");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.cfg.get_started_done);
    assert!(cabin.chat_job_thread.is_none());
    cabin.mark_get_started_done();
    assert!(cabin.cfg.get_started_done);
    cabin.mark_get_started_done();
    assert!(cabin.cfg.get_started_done);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());

    std::env::remove_var("GROKHUB_CONFIG");
    let _ = std::fs::remove_dir_all(&root);
}

// Folded from PR #366.
#[test]
fn scrub_transcript_redacts_held_secret() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.secret_hold.is_empty());
    // Fake placeholder only — no real credentials, no network.
    let held = "fake-secret-token-xyz";
    cabin.hold_secret(held);
    assert_eq!(cabin.secret_hold.as_slice(), &[held.to_string()]);
    let scrubbed = cabin.scrub_transcript(format!("CONNECTOR_RESULT used {held} then ok"));
    assert!(
        !scrubbed.contains(held),
        "held secret must leave the transcript: {scrubbed}"
    );
    assert!(
        scrubbed.contains("[redacted]"),
        "scrub must insert [redacted]: {scrubbed}"
    );
    assert!(
        scrubbed.contains("CONNECTOR_RESULT"),
        "non-secret text must survive: {scrubbed}"
    );
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #368.
#[test]
fn remember_skill_upserts_and_sorts() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.skill_list.is_empty());
    let mk = |name: &str, desc: &str| SkillMd {
        name: name.into(),
        description: desc.into(),
        slash: "/x".into(),
        trigger: "t".into(),
        instructions: "1. `echo ok`".into(),
        pitfalls: String::new(),
        verify: String::new(),
        runs: 0,
    };
    // Push out of order; remember_skill sorts on insert.
    cabin.remember_skill(mk("zeta-skill", "first zeta"));
    cabin.remember_skill(mk("alpha-skill", "first alpha"));
    assert_eq!(cabin.skill_list.len(), 2);
    assert_eq!(
        cabin
            .skill_list
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha-skill", "zeta-skill"]
    );
    // Same name upserts in place; list size and sort order stay put.
    cabin.remember_skill(mk("zeta-skill", "updated zeta"));
    assert_eq!(cabin.skill_list.len(), 2);
    assert_eq!(cabin.skill_list[0].name, "alpha-skill");
    assert_eq!(cabin.skill_list[1].name, "zeta-skill");
    assert_eq!(cabin.skill_list[1].description, "updated zeta");
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #373.
#[test]
fn mem_file_idx_maps_soul_user_memory() {
    assert_eq!(Cabin::mem_file_idx("SOUL.md"), Some(0));
    assert_eq!(Cabin::mem_file_idx("USER.md"), Some(1));
    assert_eq!(Cabin::mem_file_idx("MEMORY.md"), Some(2));
    assert_eq!(Cabin::mem_file_idx("NOTES.md"), None);
    assert_eq!(Cabin::mem_file_idx(""), None);
}

// Folded from PR #386.
#[test]
fn scrub_live_blocks_noop_without_secrets() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.secret_hold.is_empty());

    let plain = "plain live text, no hold yet";
    cabin.live_blocks.push(grokhub_core::LiveBlock {
        kind: grokhub_core::LiveKind::Say,
        body: plain.to_string(),
        tool_id: String::new(),
        tool_title: String::new(),
        tool_status: String::new(),
        tool_detail: String::new(),
        fold_slot: 2,
    });
    cabin.live_keys = vec![(2, plain.len(), 5)];
    // Empty secret_hold → early return; body and keys stay put. No spawn/network.
    cabin.scrub_live_blocks();
    assert_eq!(cabin.live_blocks[0].body, plain);
    assert_eq!(cabin.live_keys, vec![(2, plain.len(), 5)]);
    assert!(cabin.secret_hold.is_empty());

    // Hold a secret, put it in the live body, scrub must redact.
    let secret = "held-live-token";
    cabin.hold_secret(secret);
    cabin.live_blocks[0].body = format!("leak {secret} now");
    cabin.scrub_live_blocks();
    assert!(!cabin.live_blocks[0].body.contains(secret));
    assert!(cabin.live_blocks[0].body.contains("[redacted]"));
    assert!(cabin.live_keys.is_empty());
}

/// One config root for both this thread (`TestConfigDir`) and the skill-write
/// threads (`GROKHUB_CONFIG`), with one skill saved the way the cabin saves it.
fn pin_skill_config(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
    let root = crate::config::test_config_root(label);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    (crate::config::TestConfigDir::set(root.clone()), root)
}

fn weekly_report_skill() -> SkillMd {
    SkillMd {
        name: "weekly-report".into(),
        description: "Build the weekly report.".into(),
        slash: "/weekly-report".into(),
        trigger: "the user asks for the weekly report".into(),
        instructions: "1. Open report.md\n2. Fill this week's numbers".into(),
        pitfalls: String::new(),
        verify: String::new(),
        runs: 2,
    }
}

/// The skill-write threads append to the ledger; wait for `n` lines.
fn wait_ledger_lines(root: &std::path::Path, n: usize) -> grokhub_agent::harness::ChangeLedger {
    for _ in 0..400 {
        let ledger = grokhub_agent::harness::ChangeLedger::load(root);
        if ledger.all().len() >= n {
            return ledger;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("ledger never reached {n} lines");
}

/// Spike-7: a skill patch only lands when a replay of the skill's last
/// recorded run passes, so the patch tests record one successful run first.
fn record_weekly_report_run(root: &std::path::Path) {
    let mut verify = grokhub_agent::harness::fixture_span(
        1_000,
        "verify_script",
        "{}",
        "allow",
        "pass",
        None,
        "",
    );
    verify.session_id = "chat-weekly".into();
    grokhub_agent::harness::append_span(root, &verify).unwrap();
    let o = grokhub_agent::harness::outcome_from_spans(
        "chat-weekly:1",
        "skill:weekly-report",
        std::slice::from_ref(&verify),
        &[],
        false,
        1_001,
    );
    grokhub_core::outcome::append_outcome(root, &o).unwrap();
}

const WEEKLY_PATCH: &str =
    "SUGGEST_SKILL_PATCH: weekly-report | the user asks for the weekly report | 1. Open report.md 2. Fill the numbers 3. Save a copy as PDF";

#[test]
fn nightly_patch_keeps_the_prior_version_and_typed_undo_puts_it_back() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("skill-undo-typed");
    record_weekly_report_run(&root);
    let path = skills::save_skill(&weekly_report_skill()).expect("save");
    let v1 = std::fs::read(&path).unwrap();
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.apply_review_skill_patches(WEEKLY_PATCH);
    let ledger = wait_ledger_lines(&root, 1);
    let c = &ledger.all()[0];
    assert_eq!(
        (c.id.as_str(), c.op.as_str(), c.origin.as_str()),
        ("weekly-report", "modify", "self_manage")
    );
    assert_eq!(
        c.reason,
        "nightly review: 1. Open report.md 2. Fill the numbers 3. Save a copy as PDF"
    );
    assert_eq!(c.before_hash, grokhub_agent::harness::content_hash(&v1));
    let v2 = std::fs::read(&path).unwrap();
    assert_ne!(v2, v1, "the patch landed");
    assert_eq!(c.after_hash, grokhub_agent::harness::content_hash(&v2));
    let kept = grokhub_agent::harness::skill_history_dir(&root, "weekly-report");
    let kept_v1: Vec<Vec<u8>> = std::fs::read_dir(&kept)
        .unwrap()
        .flatten()
        .map(|e| std::fs::read(e.path()).unwrap())
        .collect();
    assert!(
        kept_v1.contains(&v1),
        "the prior version is kept before the write"
    );
    cabin.send_from_composer("/skills undo weekly-report".into());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        v1,
        "undo puts back the exact bytes"
    );
    assert_eq!(
        cabin.status,
        "Undid the newest change to weekly-report. The version before it is back."
    );
    assert_eq!(
        cabin
            .skill_list
            .iter()
            .find(|s| s.name == "weekly-report")
            .map(|s| s.instructions.as_str()),
        Some("1. Open report.md\n2. Fill this week's numbers\n"),
        "the cabin's list reads the restored file"
    );
    let ledger = grokhub_agent::harness::ChangeLedger::load(&root);
    assert_eq!(ledger.all().len(), 2);
    assert_eq!(
        (ledger.all()[1].op.as_str(), ledger.all()[1].origin.as_str()),
        ("undo", "user")
    );
    assert_eq!(ledger.all()[1].undoes, Some(1));
    // The same patch next night is not written again on its own.
    cabin.apply_review_skill_patches(WEEKLY_PATCH);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(std::fs::read(&path).unwrap(), v1);
    assert_eq!(
        grokhub_agent::harness::ChangeLedger::load(&root)
            .all()
            .len(),
        2
    );
    assert_eq!(
        cabin
            .skill_list
            .iter()
            .find(|s| s.name == "weekly-report")
            .map(|s| s.instructions.as_str()),
        Some("1. Open report.md\n2. Fill this week's numbers\n"),
        "the cabin's list reads the restored file"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The model never undoes a skill change on its own: the same slash from a
/// night job, a review reply, or any send that is not the user's typing only
/// posts the `/skills changes` rows, and the file stays.
#[test]
fn the_model_cannot_undo_a_skill_change_on_its_own() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("skill-undo-model");
    record_weekly_report_run(&root);
    let path = skills::save_skill(&weekly_report_skill()).expect("save");
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = skills::list_skills();
    cabin.apply_review_skill_patches(WEEKLY_PATCH);
    wait_ledger_lines(&root, 1);
    let patched = std::fs::read(&path).unwrap();
    cabin.send_scheduled_chat("/skills undo weekly-report".into());
    assert_eq!(cabin.status, super::skill_undo::UNDO_NEEDS_YOU);
    cabin.send_chat("/skills undo weekly-report".into());
    cabin.send_chat("/skills restore weekly-report".into());
    cabin.apply_review_reply(Ok("/skills undo weekly-report\nUNDO weekly-report".into()));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        patched,
        "nothing but the user undoes"
    );
    let ledger = grokhub_agent::harness::ChangeLedger::load(&root);
    assert_eq!(ledger.all().len(), 1, "no undo line: {:?}", ledger.all());
    let shown = cabin
        .messages
        .iter()
        .filter(|m| m.1.contains(super::skill_undo::SKILL_CHANGES_HEAD))
        .count();
    assert_eq!(shown, 3, "each refused undo shows the rows instead");
    assert!(!cabin.harness.typed_send, "the typed mark never sticks");
    let rows = cabin.skill_rows_now();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "weekly-report");
    assert_eq!(rows[0].act, super::skill_undo::SkillAct::Undo);
    let _ = std::fs::remove_dir_all(&root);
}

/// Outside tests, an `UndoAsk` is built only in `skill_undo.rs`, and only the
/// composer send sets `typed_send`.
#[test]
fn only_typing_or_a_click_builds_an_undo_ask() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .to_path_buf();
    let mut hits = Vec::new();
    let mut stack = vec![crates.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if name != "target" && name != "tests" {
                    stack.push(path);
                }
                continue;
            }
            if !name.ends_with(".rs") || name == "tests.rs" || name.ends_with("_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let live = text.split("#[cfg(test)]").next().unwrap_or("");
            for needle in [
                "UndoAsk::from_click(",
                "UndoAsk::from_typing(",
                "typed_send = true",
            ] {
                // chat_ui.rs has test helpers above the composer send, so the
                // typed mark is counted in the whole file.
                let hay = if needle == "typed_send = true" {
                    text.as_str()
                } else {
                    live
                };
                for _ in hay.matches(needle) {
                    let rel = path
                        .strip_prefix(&crates)
                        .unwrap_or(&path)
                        .display()
                        .to_string()
                        .replace('\\', "/");
                    hits.push(format!("{rel}: {needle}"));
                }
            }
        }
    }
    hits.sort();
    assert_eq!(
        hits,
        vec![
            "grokhub-app/src/app/change_history_ui.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/change_undo.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/chat_ui.rs: typed_send = true",
            "grokhub-app/src/app/repair_ui.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/self_review_ui.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/skill_undo.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/skill_undo.rs: UndoAsk::from_click(",
            "grokhub-app/src/app/skill_undo.rs: UndoAsk::from_typing(",
            "grokhub-app/src/app/skill_undo.rs: UndoAsk::from_typing(",
        ]
    );
}

#[test]
fn a_learned_skill_undone_by_click_is_removed_and_restore_brings_it_back() {
    let _g = crate::config::hold_test_config();
    let (_pin, root) = pin_skill_config("skill-undo-create");
    let mut cabin = Cabin::quiet_for_test();
    let mut skill = weekly_report_skill();
    skill.name = "board-status".into();
    skill.slash = "/board-status".into();
    skill.verify = "test -f report.md".into();
    cabin.commit_proposed_skill(skill);
    let ledger = wait_ledger_lines(&root, 1);
    assert_eq!(ledger.all()[0].op.as_str(), "create");
    let folder = skills::skill_folder("board-status");
    let v1 = std::fs::read(folder.join("SKILL.md")).unwrap();
    assert!(folder.join("scripts/verify.sh").exists());
    let rows = cabin.skill_rows_now();
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].label.starts_with("board-status · added "),
        "{}",
        rows[0].label
    );
    cabin.skill_row_clicked(&rows[0]);
    assert!(
        !folder.exists(),
        "undoing a created skill removes its folder"
    );
    assert!(!cabin.skill_list.iter().any(|s| s.name == "board-status"));
    assert_eq!(
        cabin.status,
        "Removed board-status: it was new. Its text is kept, and /skills restore board-status brings it back."
    );
    let kept = grokhub_agent::harness::skill_history_dir(&root, "board-status");
    let kept: Vec<Vec<u8>> = std::fs::read_dir(&kept)
        .unwrap()
        .flatten()
        .map(|e| std::fs::read(e.path()).unwrap())
        .collect();
    assert_eq!(kept, vec![v1.clone()], "its text stays in history");
    let rows = cabin.skill_rows_now();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].act, super::skill_undo::SkillAct::Restore);
    cabin.send_from_composer("/skills restore board-status".into());
    assert_eq!(std::fs::read(folder.join("SKILL.md")).unwrap(), v1);
    assert!(
        folder.join("scripts/verify.sh").exists(),
        "verify scripts follow the restored file"
    );
    assert!(cabin.skill_list.iter().any(|s| s.name == "board-status"));
    assert_eq!(cabin.status, "Restored board-status from its kept copy.");
    let _ = std::fs::remove_dir_all(&root);
}

// Folded from PR #392.
#[test]
fn apply_review_skill_patches_noop_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.skill_list.is_empty());
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    // Empty / irrelevant raw → parse_suggest_skill_patches empty → early return.
    cabin.apply_review_skill_patches("");
    cabin.apply_review_skill_patches("just prose, no SUGGEST_SKILL_PATCH line");
    assert!(
        cabin.skill_list.is_empty(),
        "idle review patches must not invent skills"
    );
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
}

// Folded from PR #394.
#[test]
fn commit_proposed_skill_remembers_in_list() {
    // Spawned save_skill follows GROKHUB_CONFIG (not thread-local TestConfigDir).
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("commit-skill");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.skill_list.is_empty());
    let proposed = SkillMd {
        name: "harbor-tidy".into(),
        description: "Tidy the harbor desk".into(),
        slash: "/harbor-tidy".into(),
        trigger: "tidy harbor".into(),
        instructions: "1. `echo ok`".into(),
        pitfalls: String::new(),
        verify: String::new(),
        runs: 0,
    };
    // Empty skill_list → prefer_patch misses → remember_skill inserts as-is.
    cabin.commit_proposed_skill(proposed);
    assert_eq!(cabin.skill_list.len(), 1);
    assert_eq!(cabin.skill_list[0].name, "harbor-tidy");
    assert_eq!(cabin.skill_list[0].slash, "/harbor-tidy");
    assert_eq!(cabin.skill_name, "harbor-tidy");
    assert!(cabin.skill_body.contains("harbor-tidy"));
    assert_eq!(cabin.status, "Wrote skill harbor-tidy");
    assert!(
        cabin
            .messages
            .iter()
            .any(|(r, c)| r == "user" && c == SKILL_SAVED_MARK),
        "commit pushes SKILL_SAVED_MARK onto the live transcript"
    );
    assert!(cabin.chat_job_thread.is_none());

    // The spawned save_skill resolves config_dir() when it runs. Let it land
    // under the test root before GROKHUB_CONFIG goes away.
    wait_for(
        "commit_proposed_skill writes SKILL.md under the test root",
        || {
            std::fs::read_dir(root.join("skills"))
                .map(|rd| rd.flatten().any(|e| e.path().join("SKILL.md").is_file()))
                .unwrap_or(false)
        },
    );
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

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
    struct RestoreHome {
        prev: Option<String>,
        #[cfg(windows)]
        prev_profile: Option<std::ffi::OsString>,
    }
    impl Drop for RestoreHome {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(h) => std::env::set_var("HOME", h),
                None => std::env::remove_var("HOME"),
            }
            #[cfg(windows)]
            match self.prev_profile.take() {
                Some(p) => std::env::set_var("USERPROFILE", p),
                None => std::env::remove_var("USERPROFILE"),
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
    std::env::remove_var("USERPROFILE");
    let home = std::env::temp_dir().join("grokhub-work-root-1054");
    std::env::set_var("HOME", &home);
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
    std::env::set_var("GROKHUB_CONFIG", &root);
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
    let chrome = account_connect_chrome(cabin.secrets.oauth.as_ref());
    assert!(chrome.connected);
    assert_eq!(chrome.action, "Sign out");
    assert_eq!(chrome.title, "Connected");
    assert_eq!(chrome.hint, "GrokHub QA · grokhub-qa@agentmail.to");
    // Signed-out chrome stays for the empty case.
    let signed_out = account_connect_chrome(None);
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
    std::env::remove_var("GROKHUB_CONFIG");
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
    // Quiet cabin: empty display_name, no OAuth name, empty USER.md → fallback "Grok".
    assert_eq!(chrome.name, "Grok");
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
fn fallback_cabin_name_defaults_to_grok() {
    let blank = super::fallback_cabin_name("");
    assert!(!blank.is_empty());
    assert_eq!(blank, "Grok");

    let from_md = super::fallback_cabin_name("Name: Jeremy\n");
    assert!(!from_md.is_empty());
    assert_eq!(from_md, "Jeremy");
}

// Folded from PR #417.
#[test]
fn export_dest_joins_under_config_or_project() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("export-dest");
    let _ = std::fs::remove_dir_all(&root);
    let _pin = crate::config::TestConfigDir::set(root.clone());
    std::env::set_var("GROKHUB_CONFIG", &root);

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

    std::env::remove_var("GROKHUB_CONFIG");
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
        settings.contains("account_connect_chrome")
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
        ("grok-3", "Grok 3"),
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

// ---- Cursor fold: Feed, pulse and home ----

// Folded from PR #275.
#[test]
fn pulse_paints_only_on_an_empty_signed_in_chat() {
    assert!(should_paint_pulse(true, false, true));
    assert!(!should_paint_pulse(false, false, true));
    assert!(!should_paint_pulse(true, true, true));
    assert!(!should_paint_pulse(true, false, false));
}

// Folded from PR #330.
#[test]
fn roll_today_stays_put_on_same_day() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("roll-today-same-day");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    let today = Cabin::local_day();
    cabin.usage.day = today.clone();
    cabin.roll_today();
    assert_eq!(
        cabin.usage.day, today,
        "same-day roll must leave usage.day alone"
    );
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    drop(cabin);
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Folded from PR #375.
#[test]
fn tick_home_surface_remembers_nav() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.chip_memory.last_surface.is_none());
    cabin.set_nav_id("imagine");
    cabin.tick_home_surface();
    assert_eq!(cabin.chip_memory.last_surface.as_deref(), Some("imagine"));
    let stamped = cabin.chip_memory.updated_at;
    assert!(stamped > 0);
    // Same surface again is a no-op — updated_at stays put.
    cabin.tick_home_surface();
    assert_eq!(cabin.chip_memory.last_surface.as_deref(), Some("imagine"));
    assert_eq!(cabin.chip_memory.updated_at, stamped);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #376.
#[test]
fn remember_last_frame_keeps_short_url() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.last_frame_url.is_none());
    // Short URL sticks; over FRAME_CAP is rejected. No store_hub_frame / live_room / network.
    cabin.remember_last_frame("data:image/jpeg;base64,abc");
    assert_eq!(
        cabin.last_frame_url.as_deref(),
        Some("data:image/jpeg;base64,abc")
    );
    cabin.last_frame_url = None;
    let huge = "x".repeat(FRAME_CAP + 1);
    cabin.remember_last_frame(&huge);
    assert!(cabin.last_frame_url.is_none());
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #378.
#[test]
fn push_presence_keeps_short_url() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.presence_ring.is_empty());
    assert!(cabin.last_frame_url.is_none());
    assert!(cabin.rx.is_none());
    assert!(cabin.reflect_rx.is_none());

    let short = "data:image/jpeg;base64,abc".to_string();
    cabin.push_presence(short.clone());
    assert_eq!(cabin.presence_ring.len(), 1);
    assert_eq!(cabin.presence_ring[0].1, short);
    assert!(
        cabin.last_frame_url.is_none(),
        "must not call store_hub_frame/remember_last_frame"
    );

    let oversized = "x".repeat(FRAME_CAP + 1);
    cabin.push_presence(oversized);
    assert_eq!(cabin.presence_ring.len(), 1);
    assert_eq!(cabin.presence_ring[0].1, short);

    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(cabin.reflect_rx.is_none());
}

// Folded from PR #379.
#[test]
fn tool_header_color_failed_vs_ok() {
    assert_eq!(
        super::tool_header_color(true),
        crate::cards::chip_tone_color(crate::cards::ChipTone::Offline),
        "failed tool header uses Offline chip tone"
    );
    assert_eq!(
        super::tool_header_color(false),
        crate::theme::muted(),
        "ok tool header uses muted"
    );
}

// Folded from PR #406.
#[test]
fn device_glance_none_when_hub_off() {
    // Free fn: hub_off + no thumb → None; hub_on → Some("Sharing"). Stay off network/spawn.
    assert!(device_glance(false, None).is_none());
    let share = device_glance(true, None).expect("sharing");
    assert_eq!(share.share_line, "Sharing");
    assert!(!share.has_thumb);
}

// Folded from PR #409. Empty main chat no longer counts a Home deck; ranking stays for Pulse floors.
#[test]
fn home_event_cards_empty_and_one_event() {
    let pulse = grokhub_core::FeedPulse::default();
    assert!(grokhub_core::home_event_cards(&[], &pulse, 0).is_empty());

    let cards = vec![feed_card("event-1", UpdateKind::AutomationDone, false)];
    assert_eq!(grokhub_core::home_event_cards(&cards, &pulse, 0).len(), 1);
}

/// The card menu is gone (2.10.86). A run hidden from Home by an older build
/// still stays off Home, still merges, and Automations still undoes it.
#[test]
fn legacy_hidden_run_stays_off_home_until_undo() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("card-menu-hide");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let _pin = crate::config::TestConfigDir::set(root.clone());

    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.quiet_start = "00:00".into();
    cabin.cfg.quiet_end = "00:00".into();
    cabin.note_automation_done(
        "zephyr-src-9f3a",
        "ZephyrHideTitle9f3a",
        "ZephyrHideBody9f3a",
    );
    let id = cabin
        .updates
        .iter()
        .find(|card| card.source_id == "zephyr-src-9f3a")
        .map(|card| card.id.clone())
        .expect("run card");
    assert_eq!(
        grokhub_core::home_event_cards(&cabin.updates, &cabin.cfg.feed_pulse, now_ms()).len(),
        1
    );

    // Hidden by an older build's card menu: the source sits in muted_sources.
    assert!(!id.is_empty());
    grokhub_core::hide_home_source(&mut cabin.cfg.feed_pulse, "zephyr-src-9f3a");
    cabin.persist_cfg();
    assert!(
        grokhub_core::home_event_cards(&cabin.updates, &cabin.cfg.feed_pulse, now_ms()).is_empty(),
        "a hidden run stays off Home"
    );
    assert!(
        cabin
            .updates
            .iter()
            .any(|card| card.source_id == "zephyr-src-9f3a"),
        "the card still updates in the store"
    );
    cabin.note_automation_done(
        "zephyr-src-9f3a",
        "ZephyrHideTitle9f3a",
        "ZephyrHideBody9f3a again",
    );
    let stored = cabin
        .updates
        .iter()
        .find(|card| card.source_id == "zephyr-src-9f3a")
        .expect("merged");
    assert!(
        stored.runs >= 2,
        "hidden runs still merge, runs={}",
        stored.runs
    );
    assert!(
        grokhub_core::home_event_cards(&cabin.updates, &cabin.cfg.feed_pulse, now_ms()).is_empty()
    );
    assert_eq!(
        grokhub_core::automation_home_note(&cabin.cfg.feed_pulse, "zephyr-src-9f3a"),
        Some(grokhub_core::HOME_HIDDEN_NOTE)
    );
    let night = include_str!("night.rs");
    assert!(
        night.contains("HOME_HIDDEN_NOTE") && night.contains("undo_hide_automation_from_home"),
        "Automations shows Hidden from Home · Undo"
    );
    assert!(grokhub_core::HOME_HIDDEN_NOTE.contains("Undo"));
    let feed = include_str!("feed_ui.rs");
    assert!(
        feed.contains("fn close_home_card("),
        "close_home_card stays"
    );
    assert!(!feed.contains("Popup"), "feed module has no card menu");
    assert!(!feed.contains("secondary_clicked"), "no right-click menu");
    assert!(!feed.contains("\"⋯\""), "no ⋯ button");
    assert!(
        !feed.contains("fn paint_feed_card("),
        "Home deck paint is gone"
    );

    let app_json = root.join("app.json");
    let mut saw_hide = false;
    for _ in 0..100 {
        if std::fs::read_to_string(&app_json)
            .unwrap_or_default()
            .contains("zephyr-src-9f3a")
        {
            saw_hide = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(saw_hide, "muted source persisted in app.json");

    cabin.undo_hide_automation_from_home("zephyr-src-9f3a");
    assert_eq!(
        grokhub_core::home_event_cards(&cabin.updates, &cabin.cfg.feed_pulse, now_ms()).len(),
        1
    );
    let mut saw_undo = false;
    for _ in 0..100 {
        let body = std::fs::read_to_string(&app_json).unwrap_or_default();
        if !body.contains("zephyr-src-9f3a") {
            saw_undo = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(saw_undo, "Undo drops the source from app.json");

    let signals = std::fs::read_to_string(root.join("card_signals.jsonl")).unwrap_or_default();
    assert!(
        !signals.contains("\"event\":\"hidden\""),
        "nothing hides from a card now: {signals}"
    );
    assert!(signals.contains("\"event\":\"unhidden\""), "{signals}");
    assert!(!signals.contains("ZephyrHideTitle9f3a"), "{signals}");
    assert!(!signals.contains("ZephyrHideBody9f3a"), "{signals}");

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn reset_all_clears_what_home_learned() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("home-learned-reset");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let _pin = crate::config::TestConfigDir::set(root.clone());

    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.quiet_start = "00:00".into();
    cabin.cfg.quiet_end = "00:00".into();
    cabin.note_automation_done(
        "learn-src",
        "ZephyrLedger snapshot",
        "ZephyrBodyShouldStayOut",
    );
    let id = cabin
        .updates
        .iter()
        .find(|card| card.source_id == "learn-src")
        .map(|card| card.id.clone())
        .expect("run card");
    cabin.open_feed_card(&id);
    cabin.note_automation_done(
        "learn-src-b",
        "Quarterly vendor audit",
        "ZephyrBodyShouldStayOut",
    );
    let other = cabin
        .updates
        .iter()
        .find(|card| card.source_id == "learn-src-b")
        .map(|card| card.id.clone())
        .expect("second run card");
    cabin.dismiss_feed_card(&other);

    let prefs_path = root.join("card_prefs.json");
    let body = std::fs::read_to_string(&prefs_path).unwrap_or_default();
    assert!(
        body.contains("zephyrledger") && body.contains("snapshot"),
        "title keywords landed in card_prefs.json: {body}"
    );
    assert!(!body.contains("ZephyrBodyShouldStayOut"), "{body}");
    let signals_before =
        std::fs::read_to_string(root.join("card_signals.jsonl")).unwrap_or_default();
    assert!(
        signals_before.contains("\"event\":\"opened\""),
        "{signals_before}"
    );
    assert!(
        signals_before.contains("\"event\":\"rejected\""),
        "{signals_before}"
    );
    assert!(!signals_before.contains("ZephyrLedger"), "{signals_before}");

    cabin.reset_home_learned();
    let cleared = std::fs::read_to_string(&prefs_path).unwrap_or_default();
    let parsed: grokhub_core::CardPrefs =
        serde_json::from_str(&cleared).expect("empty prefs still parse");
    assert!(parsed.kinds.is_empty(), "{cleared}");
    assert!(parsed.groups.is_empty(), "{cleared}");
    assert!(parsed.topics.is_empty(), "{cleared}");
    let loaded = grokhub_core::load_card_prefs(&root);
    assert!(loaded.kinds.is_empty() && loaded.groups.is_empty() && loaded.topics.is_empty());
    let signals_after =
        std::fs::read_to_string(root.join("card_signals.jsonl")).unwrap_or_default();
    assert_eq!(signals_before, signals_after);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Folded from PR #410.
#[test]
fn drop_extra_note_none_for_zero_extras() {
    // Free fn: 0 → None; 1 → singular wording; n>1 → mentions n. Stay off spawn/network.
    assert_eq!(drop_extra_note(0), None);
    let one = drop_extra_note(1).expect("singular");
    assert!(
        one.contains("1 more file"),
        "one extra uses singular wording: {one}"
    );
    let many = drop_extra_note(4).expect("plural");
    assert!(many.contains("4 more file"), "n>1 extras mention n: {many}");
}

// Folded from PR #423.
#[test]
fn composer_hint_ink_blends_subtle_and_muted() {
    let ink = super::composer_hint_ink();
    assert_ne!(ink.a(), 0, "hint ink is not fully transparent: {ink:?}");
    assert_eq!(
        ink,
        crate::theme::blend_color(crate::theme::subtle(), crate::theme::muted(), 0.5),
        "hint ink is the subtle+muted mid blend"
    );
}

// ---- Cursor fold: Clock, day and host shells ----

// Folded from PR #387.
#[test]
fn day_now_returns_yyyy_mm_dd() {
    // Associated fn: shells `date +%F`, or falls back to 1970-01-01. No network/xAI.
    let day = Cabin::day_now();
    let ok = day.len() == 10
        && day.as_bytes()[4] == b'-'
        && day.as_bytes()[7] == b'-'
        && day
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    assert!(
        ok,
        "day_now must be YYYY-MM-DD (or the 1970-01-01 fallback), got {day:?}"
    );
}

// Folded from PR #390.
#[test]
fn clock_now_returns_local_clock() {
    // Associated fn: shells `date` (or noon Monday fallback). No network/xAI.
    let clock = Cabin::clock_now();
    assert!(
        clock.weekday <= 6 && clock.hour <= 23 && clock.minute <= 59,
        "LocalClock out of range (or expect weekday=1 hour=12 minute=0 fallback): weekday={} hour={} minute={}",
        clock.weekday,
        clock.hour,
        clock.minute
    );
}

// Folded from PR #393.
#[test]
fn local_clock_returns_cached_or_fresh() {
    // Associated fn: cache hit or `clock_now` via `date` (or noon Monday fallback).
    // No network/xAI. Call twice so the second path is cached-or-fresh.
    let a = Cabin::local_clock();
    let b = Cabin::local_clock();
    for (label, clock) in [("first", a), ("second", b)] {
        assert!(
            clock.weekday <= 6 && clock.hour <= 23 && clock.minute <= 59,
            "{label} LocalClock out of range (or expect weekday=1 hour=12 minute=0 fallback): weekday={} hour={} minute={}",
            clock.weekday,
            clock.hour,
            clock.minute
        );
    }
}

// Folded from PR #395.
#[test]
fn date_out_formats_via_date() {
    // Associated fn: shells `date` with fmt; empty on failure. No network/xAI.
    let out = Cabin::date_out("+%F");
    let ok = out.is_empty()
        || (out.len() == 10
            && out.as_bytes()[4] == b'-'
            && out.as_bytes()[7] == b'-'
            && out
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit()));
    assert!(
        ok,
        "date_out(+%F) must be YYYY-MM-DD shaped or empty on failure, got {out:?}"
    );
}

// Folded from PR #396.
#[test]
fn kick_local_clock_stays_off_network() {
    // Associated fn: background `date` refresh into LAST_CLOCK. No grok/xAI/network.
    let cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());

    if let Ok(mut g) = super::LAST_CLOCK.lock() {
        *g = None;
    }
    Cabin::kick_local_clock();

    let clock = (0..80)
        .find_map(|_| {
            if let Ok(g) = super::LAST_CLOCK.lock() {
                if let Some((_, clock, inflight)) = g.as_ref() {
                    if !*inflight {
                        return Some(*clock);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            None
        })
        .expect("kick_local_clock should cache a LocalClock (date shell OK)");

    assert!(
        clock.weekday <= 6 && clock.hour <= 23 && clock.minute <= 59,
        "cached LocalClock out of range: weekday={} hour={} minute={}",
        clock.weekday,
        clock.hour,
        clock.minute
    );
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #397.
#[test]
fn kick_local_day_stays_off_network() {
    let cabin = Cabin::quiet_for_test();
    // Seed LAST_DAY so kick marks inflight and refreshes via `day_now` / `date +%F`.
    let _ = Cabin::local_day();
    Cabin::kick_local_day();
    // Date shell OK; wait for the worker to rewrite the cache. No grok/xAI.
    let mut day = String::new();
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        day = Cabin::local_day();
        let shaped = day.len() == 10
            && day.as_bytes()[4] == b'-'
            && day.as_bytes()[7] == b'-'
            && day
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
        if shaped {
            break;
        }
    }
    let ok = day.len() == 10
        && day.as_bytes()[4] == b'-'
        && day.as_bytes()[7] == b'-'
        && day
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    assert!(
        ok,
        "kick_local_day must refresh/cache YYYY-MM-DD (date shell OK), got {day:?}"
    );
    assert!(!cabin.running, "kick_local_day must stay off grok/xAI");
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
}

// Folded from PR #401.
#[test]
fn hostname_i_now_returns_string() {
    // `hostname -I` on Linux, the adapter list on Windows, or the routed address: every
    // word is an IP address (IPv6 included on Linux). Stays off the network.
    let out = hostname_i_now();
    for word in out.split_whitespace() {
        assert!(
            word.parse::<std::net::IpAddr>().is_ok(),
            "{word:?} in {out:?}"
        );
    }
}

// Folded from PR #402.
#[test]
fn chip_hour_returns_0_through_23() {
    // Associated fn: hour from local_clock (cached date shell or noon fallback). No network/xAI.
    let hour = Cabin::chip_hour();
    assert!(
        (0..=23).contains(&hour),
        "chip_hour must be 0..=23 (or noon fallback hour=12), got {hour}"
    );
}

#[test]
fn halt_shortcut_is_ctrl_alt_h_not_the_task_manager_key() {
    let src = include_str!("mod.rs");
    // The hotkey halts the live turn and every background run.
    let halt = src
        .split("self.halt_everything(\"Stopped\");")
        .next()
        .and_then(|s| s.rsplit("if ctx.input(").next())
        .expect("in-app halt shortcut");
    assert!(
        halt.contains("i.modifiers.command")
            && halt.contains("i.modifiers.alt")
            && halt.contains("!i.modifiers.shift")
            && halt.contains("egui::Key::H"),
        "Ctrl+Alt+H halts: {halt}"
    );
    assert!(
        !src.contains("i.modifiers.shift && i.key_pressed(egui::Key::Escape)"),
        "Ctrl+Shift+Esc opens Task Manager on Windows and never reaches the app"
    );
}

#[test]
fn imagine_aspect_pill_drives_every_request() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.imagine_aspect = 4;
    match cabin.imagine_call() {
        super::imagine::ImagineCall::Generate { aspect, .. } => assert_eq!(aspect, "16:9"),
        _ => panic!("Image mode generates"),
    }
    cabin.imagine_native.image_op = 1;
    match cabin.imagine_call() {
        super::imagine::ImagineCall::Edit { aspect, .. } => assert_eq!(aspect, "16:9"),
        _ => panic!("Edit mode edits"),
    }
    cabin.imagine_kind = grokhub_core::ImagineKind::Video;
    cabin.imagine_aspect = 3;
    match cabin.imagine_call() {
        super::imagine::ImagineCall::Video { aspect, .. } => assert_eq!(aspect, "9:16"),
        _ => panic!("Video mode makes video"),
    }
    let src = include_str!("imagine.rs");
    let ui = src
        .split("fn ui_imagine(")
        .nth(1)
        .and_then(|s| s.split("fn ui_imagine_bar(").next())
        .expect("ui_imagine");
    assert!(
        !ui.contains("small_button") && !ui.contains("ui_imagine_account"),
        "no raw button rows above the Imagine composer: {ui}"
    );
}

#[test]
fn imagine_more_finishes_agent_count_and_results_chrome() {
    let src = include_str!("imagine.rs");
    let more = src
        .split("fn ui_imagine_more(")
        .nth(1)
        .and_then(|s| s.split("fn ui_imagine_results(").next())
        .expect("ui_imagine_more");
    assert!(
        more.contains("ui_imagine_more_agent")
            && more.contains("Character sprite still")
            && more.contains("ui_imagine_more_count")
            && more.contains("clamp(1, 10)")
            && !more.contains("ImagineKind::Agent => {}"),
        "Agent More must offer model + 1–10 count, not stay empty: {more}"
    );
    assert!(
        !more.contains("&[\"1\", \"2\", \"3\", \"4\"]") && !more.contains("clamp(1, 4)"),
        "image count UI must reach the API's 1–10 range: {more}"
    );
    let results = src
        .split("fn ui_imagine_results(")
        .nth(1)
        .and_then(|s| s.split("fn imagine_more_title(").next())
        .expect("ui_imagine_results");
    assert!(
        results.contains("ghost_pill(ui, \"Save\")")
            && results.contains("ghost_pill(ui, \"Open folder\")")
            && !results.contains("small_button"),
        "Results must use cabin pills, not raw egui buttons: {results}"
    );
    let bar = src
        .split("fn ui_imagine_bar(")
        .nth(1)
        .and_then(|s| s.split("fn poll_wall(").next())
        .expect("ui_imagine_bar");
    assert!(
        !bar.contains("if self.imagine_kind != ImagineKind::Video")
            || !bar
                .split("imagine-aspect-hit")
                .nth(1)
                .expect("aspect")
                .split("imagine-more-hit")
                .next()
                .expect("before more")
                .contains("ui.end_row()"),
        "More must sit with Style/Aspect, not alone on a forced second row"
    );
    let stage_wire = src
        .split("fn ui_imagine(")
        .nth(1)
        .and_then(|s| s.split("fn ui_imagine_bar(").next())
        .expect("ui_imagine");
    assert!(
        stage_wire.contains("stage_hit.retry") && stage_wire.contains("kick_imagine()"),
        "stage Retry must re-kick Imagine: {stage_wire}"
    );
    let mut cabin = Cabin::quiet_for_test();
    cabin.imagine_kind = grokhub_core::ImagineKind::Agent;
    cabin.imagine_native.count = 7;
    cabin.imagine_native.image_model = 1;
    match cabin.imagine_call() {
        super::imagine::ImagineCall::Generate { n, model, .. } => {
            assert_eq!(n, 7);
            assert_eq!(model, "grok-imagine-image-quality");
        }
        _ => panic!("Agent must generate stills"),
    }
    cabin.imagine_native.count = 10;
    match cabin.imagine_call() {
        super::imagine::ImagineCall::Generate { n, .. } => assert_eq!(n, 10),
        _ => panic!("count 10"),
    }

    // Paint Agent More and a failed stage so the finished chrome is visible in-process.
    let _cfg = crate::config::hold_test_config();
    let root = crate::config::test_config_root("imagine-more-paint");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.nav = Nav::Imagine;
    cabin.imagine_kind = grokhub_core::ImagineKind::Agent;
    cabin.imagine_native.more_open = true;
    cabin.imagine_native.more_anchor =
        egui::Rect::from_min_size(egui::pos2(200.0, 400.0), egui::vec2(80.0, 36.0));
    cabin.imagine_native.count = 3;
    let _paint = crate::theme::hold_paint_test();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 900.0),
        )),
        ..Default::default()
    };
    let mut texts = Vec::new();
    for _ in 0..2 {
        texts.clear();
        let out = crate::theme::test_pass(&ctx, raw.clone(), |ui| {
            cabin.ui_imagine(ui);
            cabin.ui_imagine_overlays(ui.ctx());
        });
        for clipped in &out.shapes {
            collect_shape_text(&clipped.shape, &mut texts);
        }
    }
    let blob = texts.join(" | ");
    assert!(
        blob.contains("Character sprite still")
            && blob.contains("Model")
            && blob.contains("Images")
            && blob.contains("3"),
        "Agent More must paint model + count: {blob}"
    );
    cabin.imagine_native.more_open = false;
    cabin.imagine_error = "network interrupted".into();
    cabin.imagine_last.clear();
    texts.clear();
    let out = crate::theme::test_pass(&ctx, raw, |ui| cabin.ui_imagine(ui));
    for clipped in &out.shapes {
        collect_shape_text(&clipped.shape, &mut texts);
    }
    let fail = texts.join(" | ");
    assert!(
        fail.contains("Imagine failed") && fail.contains("Retry"),
        "failed stage must offer Retry: {fail}"
    );
}

#[test]
fn small_meta_text_is_readable_and_search_shows_its_key() {
    assert_eq!(
        crate::theme::SUBTLE,
        egui::Color32::from_rgb(0x8b, 0x90, 0x96)
    );
    assert_eq!(super::sidebar::SEARCH_KEY_HINT, "Ctrl+K");
    assert!(grokhub_core::shortcut_help().contains("Ctrl+K"));
}

#[test]
fn the_palette_closes_on_navigation_and_uses_the_sidebar_names() {
    let mut quiet = QuietCabin::boot("palette-close");
    let cabin = &mut quiet.cabin;
    let _paint = crate::theme::hold_paint_test();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1400.0, 900.0),
        )),
        ..Default::default()
    };
    cabin.nav = super::Nav::Chat;
    cabin.open_palette();
    let mut texts = Vec::new();
    for _ in 0..2 {
        texts.clear();
        let out = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_palette(ui.ctx()));
        for clipped in &out.shapes {
            collect_shape_text(&clipped.shape, &mut texts);
        }
    }
    for want in [
        "Search pages and commands",
        "Automations",
        "Skills and Connectors",
        "Pulse",
    ] {
        assert!(
            texts.iter().any(|t| t == want),
            "missing {want:?} in {texts:?}"
        );
    }
    assert!(
        !texts
            .iter()
            .any(|t| t == "Palette" || t == "Go to…" || t == "Night"),
        "{texts:?}"
    );
    assert!(cabin.palette_open);
    let _ = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_palette(ui.ctx()));
    assert!(cabin.palette_open, "nothing changed: it stays open");
    // A sidebar click changes the page underneath: the palette goes.
    cabin.nav = super::Nav::Pulse;
    let _ = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_palette(ui.ctx()));
    assert!(!cabin.palette_open);
    // A click outside it closes it too.
    cabin.open_palette();
    let _ = crate::theme::test_pass(&ctx, raw.clone(), |ui| cabin.ui_palette(ui.ctx()));
    let at = egui::pos2(1350.0, 850.0);
    let click = egui::RawInput {
        events: vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        ..raw.clone()
    };
    let _ = crate::theme::test_pass(&ctx, click, |ui| cabin.ui_palette(ui.ctx()));
    let release = egui::RawInput {
        events: vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..raw
    };
    let _ = crate::theme::test_pass(&ctx, release, |ui| cabin.ui_palette(ui.ctx()));
    assert!(!cabin.palette_open, "an outside click closes the palette");
}

#[test]
fn motion_wave2e_pulse_header_control_unmoved() {
    let pulse = include_str!("pulse_ui.rs");
    assert!(
        pulse.contains("HEADER_SLOT_W") && pulse.contains("felt_segment"),
        "Feed|Ideas must keep the fixed header slot"
    );
    assert!(
        pulse.contains("pulse_tab_t") && pulse.contains("ideas_axis_x"),
        "Pulse must use Wave 2E crossfade helpers"
    );
}

#[test]
fn motion_wave2e_module_wired() {
    let main = include_str!("../main.rs");
    assert!(
        main.contains("mod motion;"),
        "motion module must be declared"
    );
    let motion = include_str!("../motion.rs");
    assert!(motion.contains("PULSE_CROSSFADE_SECS: f32 = 0.180"));
    assert!(motion.contains("APPROVAL_ENTER_SECS: f32 = 0.200"));
    assert!(motion.contains("CURSOR_SETTLE_SECS: f32 = 0.080"));
    assert!(
        motion.contains("no neon")
            || motion.contains("No neon")
            || motion.contains("no neon trail")
            || motion.contains("No neon trail")
    );
}
/// One composer frame wired like the Chat pill: the send check runs before and
/// after a multiline field whose `return_key` is Command+Enter.
fn composer_key_frame(
    ctx: &egui::Context,
    composer: &mut String,
    modifiers: egui::Modifiers,
    keys: &[egui::Key],
) -> Vec<String> {
    let id = egui::Id::new("composer-key-frame");
    let mut events = vec![egui::Event::ModifiersChanged(modifiers)];
    events.extend(keys.iter().map(|&key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }));
    let raw = egui::RawInput {
        events,
        ..Default::default()
    };
    let mut sent = Vec::new();
    let _ = crate::theme::test_pass(ctx, raw, |ui| {
        ui.memory_mut(|m| m.request_focus(id));
        let focused = ui.memory(|m| m.has_focus(id));
        if let Some(t) = super::take_focused_composer(ui, composer, focused) {
            sent.push(t);
        }
        let edit = ui.add(egui::TextEdit::multiline(composer).id(id).return_key(Some(
            egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Enter),
        )));
        if let Some(t) = super::take_focused_composer(ui, composer, edit.has_focus()) {
            sent.push(t);
        }
    });
    sent
}

#[test]
fn shift_enter_breaks_the_composer_line_and_enter_sends() {
    let ctx = egui::Context::default();
    let mut composer = String::from("first line");
    // Settle focus with a frame that presses nothing.
    assert!(composer_key_frame(&ctx, &mut composer, egui::Modifiers::NONE, &[]).is_empty());

    let sent = composer_key_frame(
        &ctx,
        &mut composer,
        egui::Modifiers::SHIFT,
        &[egui::Key::Enter],
    );
    assert!(sent.is_empty(), "Shift+Enter must not send: {sent:?}");
    assert_eq!(composer, "first line\n");

    // Ctrl on Linux and Windows sets both bits, as winit reports it.
    let ctrl = egui::Modifiers::CTRL | egui::Modifiers::COMMAND;
    let sent = composer_key_frame(&ctx, &mut composer, ctrl, &[egui::Key::Enter]);
    assert!(sent.is_empty(), "Ctrl+Enter must not send: {sent:?}");
    assert_eq!(composer, "first line\n\n");

    composer = String::from("send me");
    let sent = composer_key_frame(
        &ctx,
        &mut composer,
        egui::Modifiers::NONE,
        &[egui::Key::Enter],
    );
    assert_eq!(sent, vec!["send me".to_string()]);
    assert_eq!(composer, "");

    // Alt+Enter still sends (it queues while a reply runs).
    composer = String::from("queue me");
    let sent = composer_key_frame(
        &ctx,
        &mut composer,
        egui::Modifiers::ALT,
        &[egui::Key::Enter],
    );
    assert_eq!(sent, vec!["queue me".to_string()]);

    let src = cabin_src();
    let take = fn_src(&src, "take_focused_composer");
    assert!(take.contains("shift_enter_as_newline"), "{take}");
}

#[test]
fn ctrl_comma_opens_settings_and_the_palette_lists_shortcuts() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.nav = Nav::Chat;
    cabin.palette_open = true;
    let ctx = egui::Context::default();
    let comma = egui::RawInput {
        events: vec![
            egui::Event::ModifiersChanged(egui::Modifiers::COMMAND),
            egui::Event::Key {
                key: egui::Key::Comma,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            },
        ],
        ..Default::default()
    };
    let _ = crate::theme::test_pass(&ctx, comma, |ui| {
        let ctx = ui.ctx().clone();
        cabin.logic_input_actions(&ctx);
    });
    assert!(matches!(cabin.nav, Nav::Settings));
    assert!(!cabin.palette_open);

    cabin.shortcuts_open = false;
    cabin.run_palette(&ctx, "shortcuts");
    assert!(cabin.shortcuts_open);
}
#[test]
fn labs_beta_channel_toggle_is_wired() {
    let settings = include_str!("settings.rs");
    assert!(
        settings.contains("Beta channel")
            && settings.contains("queue_channel_switch")
            && settings.contains("queue_release_channel_switch")
            && settings.contains("channel_labs_status")
            && settings.contains("channel_windows_note")
            && settings.contains("maybe_auto_off_beta_channel")
            && settings.contains("channel_auto_off_note"),
        "Labs must wire the Beta channel toggle, status line, Windows note, and auto-off"
    );
    let app = include_str!("mod.rs");
    assert!(
        app.contains("fn queue_channel_switch")
            && app.contains("fn queue_release_channel_switch")
            && app.contains("release_channel_switch_cmds")
            && app.contains("channel_switch_cmds")
            && app.contains("SettingsSec::Labs")
            && app.contains("fn maybe_auto_off_beta_channel")
            && app.contains("try_auto_off_beta_channel"),
        "App must queue a channel switch from Labs and auto-off when tips match"
    );
    let update = include_str!("../update.rs");
    assert!(
        update.contains("channel_switch_shell")
            && update.contains("--channel")
            && update.contains("CHANNEL_WINDOWS_NOTE")
            && update.contains("try_auto_off_beta_channel")
            && update.contains("write_installed_channel"),
        "Update helper must build the install.sh --channel plan with rollback and auto-off"
    );
}

fn wait_diagnose(cabin: &mut Cabin) {
    let start = std::time::Instant::now();
    while cabin.harness.diagnose_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(4)
    {
        cabin.poll_diagnose();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn wifi_intent_without_system_state_runs_no_probe_and_posts_the_ask() {
    use grokhub_agent::harness as hx;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("diagnose-intent");
    std::fs::create_dir_all(&root).unwrap();
    let before = cabin.messages.len();
    cabin.send_chat("my wifi doesn't work".into());
    assert!(!cabin.running, "a diagnose intent never reaches the model");
    wait_diagnose(&mut cabin);
    assert_eq!(cabin.messages.len(), before + 2);
    assert_eq!(
        cabin.messages[before],
        ("user".into(), "my wifi doesn't work".into())
    );
    assert_eq!(
        cabin.messages[before + 1],
        ("assistant".into(), grokhub_agent::repair::SCOPE_ASK.into())
    );
    let trace = cabin
        .threads
        .get(cabin.thread_idx)
        .map(|t| t.id.clone())
        .unwrap_or_else(|| "session".into());
    let spans = hx::read_spans(&root, &trace).unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].tool, "diagnose");
    assert_eq!(spans[0].decision, "deny");
    assert_eq!(spans[0].origin, hx::Origin::Repair);
    let ask = cabin
        .harness
        .indexer
        .asks
        .first()
        .expect("the Spike-8a ask card is queued");
    assert_eq!(ask.scope, hx::Scope::SystemState);
    assert_eq!(ask.why, grokhub_agent::repair::SCOPE_ASK_WHY);
    assert_eq!(cabin.harness.indexer.asks.len(), 1);
    release_isolated(&root, cabin);
}

#[test]
fn diagnose_slash_without_system_state_is_a_slash_result_with_the_ask() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("diagnose-slash");
    std::fs::create_dir_all(&root).unwrap();
    let before = cabin.messages.len();
    cabin.run_slash_line("/diagnose");
    assert!(!cabin.running);
    wait_diagnose(&mut cabin);
    assert_eq!(cabin.messages.len(), before + 1);
    let (role, body) = cabin.messages.last().cloned().unwrap();
    assert_eq!(role, "assistant");
    assert!(
        grokhub_core::is_cabin_slash_turn(&role, &body),
        "stays out of the next model kick"
    );
    assert!(body.ends_with(grokhub_agent::repair::SCOPE_ASK), "{body}");
    release_isolated(&root, cabin);
}

#[test]
fn diagnose_answers_in_the_chat_that_asked_after_a_switch() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("diagnose-switch");
    std::fs::create_dir_all(&root).unwrap();
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Asked here", false));
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Moved here", false));
    let (asked, moved) = (cabin.threads.len() - 2, cabin.threads.len() - 1);
    cabin.apply_switch_thread(asked);
    cabin.run_slash_line("/diagnose");
    cabin.apply_switch_thread(moved);
    let before = cabin.messages.len();
    wait_diagnose(&mut cabin);
    assert!(cabin.harness.diagnose_rx.is_none(), "the diagnose finished");
    assert_eq!(
        cabin.messages.len(),
        before,
        "the chat on screen gets nothing"
    );
    cabin.apply_switch_thread(asked);
    let (role, body) = cabin.messages.last().cloned().unwrap();
    assert_eq!(role, "assistant");
    assert!(body.ends_with(grokhub_agent::repair::SCOPE_ASK), "{body}");
    release_isolated(&root, cabin);
}
