use super::*;
use eframe::egui;

fn cabin_src() -> String {
    concat!(
        include_str!("mod.rs"),
        include_str!("persist.rs"),
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
        include_str!("jobs.rs"),
        include_str!("chips.rs"),
        include_str!("voice.rs"),
        include_str!("threads_nav.rs"),
    )
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
        let spawn = pick.find("thread::spawn").expect("picker leaves the UI thread");
        let file = pick.find("pick_file()").expect("native picker");
        let install = pick.find("install_profile_picture").expect("copy into cabin config");
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
        let install = pick.find("install_profile_picture").expect("copy into cabin config");
        let before = pick.find("profile_pick_current").expect("cancel check");
        assert!(
            file < before && before < install,
            "Remove must cancel the pick before it installs a picture: {pick}"
        );
    }

    /// A writer that already holds the disk lock samples the slot only after a newer
    /// publish. The older generation must not be what gets saved.
    fn stale_after_newer_publish(
        older: super::AppConfig,
        newer: super::AppConfig,
    ) -> super::AppConfig {
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
        let persist = src
            .split("fn persist_cfg(")
            .nth(1)
            .and_then(|s| s.split("fn persist_if_dirty(").next())
            .expect("persist_cfg");
        let check = persist.find("cfg_if_current").expect("generation check");
        let save = persist.find("config::save").expect("save");
        assert!(
            persist.contains("publish_cfg")
                && persist.contains("thread::spawn")
                && persist.contains("persist_io")
                && persist.contains("api_key.clear")
                && check < save,
            "a config save must write the latest generation, not the clone it spawned with: {persist}"
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
            let [a, b] = range.sorted();
            assert_eq!(a.index, 0);
            assert_eq!(b.index, 7);
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
                && speech.contains("allocate_exact_size(egui::vec2(16.0, 16.0)"),
            "bubble pad must sit inside the fill, not get clipped by rounded inner_margin: {speech}"
        );
    }

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
                    resp.rect.width() <= grokhub_core::bubble_max_width(800.0) + 8.0,
                    "pane column {}",
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
            super::thought_fold_id(
                thread,
                "body",
                grokhub_core::thought_body_key(&stored.body),
            ),
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
                let body = "## Heading\n\n- bullet one\n- bullet two\n\nClosing line.";
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
                    "/very/long/path/to/grokhub/lib/systemd/status\" 2>/dev/null && echo ok "
                        .repeat(6);
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
        let block = src
            .split("fn paint_chat_block(")
            .nth(1)
            .and_then(|s| s.split("fn screen_from_rows(").next())
            .expect("paint_chat_block");
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
            .split("return resp.expect(\"speech bubble\");")
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
            chat.contains("paint_running") && chat.contains("chat_run_label"),
            "a running pulse must show while the agent is working: {chat}"
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
        let show = src
            .split("fn poll_session_show(")
            .nth(1)
            .and_then(|s| s.split("fn poll_acp_spawn(").next())
            .expect("poll_session_show");
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
        let voice = src
            .split("fn poll_voice(")
            .nth(1)
            .and_then(|s| s.split("fn poll_tray(").next())
            .expect("poll_voice");
        assert!(
            voice.contains("fold_stream_fields") && !voice.contains("content.clone()"),
            "a voice token must not clone an 8MB transcript to append a delta: {voice}"
        );
        assert!(
            voice.contains("persist_idle_key") && voice.contains("self.persist()"),
            "a live voice delta must not clone every thread 2s later — bump the idle key so persist_bg skips: {voice}"
        );
    }

    fn with_fonts_ui(mut add: impl FnMut(&mut egui::Ui)) {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| add(ui));
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
        fn copy_id(skip_earlier: bool, salt: bool) -> egui::Id {
            let ctx = egui::Context::default();
            let mut id = egui::Id::NULL;
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    for i in 0..2 {
                        if skip_earlier && i == 0 {
                            ui.add_space(20.0);
                            if salt {
                                ui.skip_ahead_auto_ids(1);
                            }
                            continue;
                        }
                        let button = |ui: &mut egui::Ui| ui.button("Copy");
                        let resp = if salt {
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
            "egui auto ids still shift when a row is skipped; the salt is what holds them"
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
        assert!(!super::project_row_active(
            true,
            false,
            super::Nav::Chat
        ));
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
            .and_then(|s| s.split("fn cached_chat_views(").next())
            .expect("rail-history");
        assert!(
            rail.contains("project_folder_history_row"),
            "project History must skip unlisted empty drafts: {rail}"
        );
        let row = include_str!("../threads.rs");
        assert!(
            row.contains("fn project_folder_history_row") && row.contains("empty_chat_draft"),
            "project History rows must use empty_chat_draft"
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
        grokhub_core::palette_forget_stale_walk(&mut files, &mut files_q, &mut files_root, "buriexx", "/place");
        assert!(files.is_empty(), "a query change clears the last hits");
        assert!(
            files_q.is_empty() && files_root.is_empty(),
            "the finished key must be forgotten or a revert matches the empty list"
        );
        assert!(
            !grokhub_core::palette_search_is_saved(&files_q, &files_root, "buried", "/place"),
            "reverting to the earlier query must walk again so the file hits come back"
        );
        grokhub_core::palette_forget_stale_walk(&mut files, &mut files_q, &mut files_root, "buried", "/place");
        assert!(
            files_q.is_empty() && files.is_empty(),
            "revert still has no finished key, so tick kicks a real walk: {files_q:?} {files:?}"
        );
    }

    #[test]
    fn picking_a_palette_file_opens_it() {
        let action = grokhub_core::palette_row_action(
            &[],
            &["nested/deep/buried.txt".to_string()],
            "/place",
            0,
        )
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
                && update.contains("Install Grok Build CLI")
                && update.contains("show_cli_install")
                && update.contains("cabin_notice")
                && update.contains("cli_notice")
                && update.contains("settings_action")
                && !update.contains("if let Some(label) = update_label")
                && !update.contains("else if !show_cli_install"),
            "Update is one control for CLI and cabin, always visible, Install when missing: {update}"
        );
        assert!(
            settings.contains("cabin_update_notice")
                && settings.contains("cli_update_notice")
                && settings.contains("should_update_cli_alpha")
                && settings.contains("queue_combined_update")
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
            ask.contains("p.reason")
                && ask.contains("p.action")
                && src.contains("fn paint_try_again("),
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
            always_click < set_always
                && ask[always_click..set_always].contains("perm_always_confirm"),
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
            always.contains("acp_spawn_rx = None"),
            "/always during handshake must drop the in-flight Ask agent: {always}"
        );
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
            .and_then(|s| s.split("Slash::Effort(").next())
            .expect("AutoPerm");
        assert!(
            auto.contains("acp_spawn_rx = None"),
            "/auto during handshake must drop the in-flight Ask agent: {auto}"
        );
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
        let effort = src
            .split("Slash::Effort(level)")
            .nth(1)
            .and_then(|s| s.split("Slash::Sessions").next())
            .expect("Effort");
        assert!(
            effort.contains("cfg.reasoning_effort") && effort.contains("parse_reasoning_effort"),
            "/effort must set reasoning_effort directly: {effort}"
        );
        assert!(
            !effort.contains("cfg.mode"),
            "/effort must not rewrite legacy cfg.mode: {effort}"
        );
        let appearance = src
            .split("SettingsSec::Appearance => {")
            .nth(1)
            .and_then(|s| s.split("SettingsSec::Behavior => {").next())
            .expect("Appearance");
        assert!(
            appearance.contains("self.persist_cfg()")
                && !appearance.contains("save = true")
                && !appearance.contains("self.persist()")
                && !appearance.contains("persist_snap"),
            "Appearance must not clone every thread just to write app.json: {appearance}"
        );
        let behavior = src
            .split("SettingsSec::Behavior => {")
            .nth(1)
            .and_then(|s| s.split("SettingsSec::Update => {").next())
            .expect("Behavior");
        assert!(
            behavior.contains("self.persist_cfg()")
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
            plan.contains("acp_spawn_rx = None"),
            "/plan during handshake must drop the in-flight Ask agent: {plan}"
        );
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
        let row = src
            .split("let row = crate::cards::session_row")
            .nth(1)
            .and_then(|s| s.split("ui.allocate_ui_with_layout").next())
            .expect("session_row");
        assert_eq!(
            row.matches("acp_spawn_rx = None").count(),
            3,
            "session/permission/effort row must drop an in-flight handshake: {row}"
        );
        assert_eq!(
            row.matches("grok_session = None").count(),
            3,
            "session/permission/effort row must session/new so mode takes: {row}"
        );
        assert_eq!(
            row.matches("persist_idle_key").count(),
            3,
            "session/permission/effort row must not clone every thread — bump the idle key so persist_bg skips: {row}"
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
            live.contains("grok_sessions_inflight")
                && live.contains("persist_rx")
                && live.contains("inspect_rx")
                && live.contains("grok_catalog_rx")
                && live.contains("history_rx")
                && live.contains("mem_restore_rx")
                && live.contains("mem_file_rx")
                && live.contains("recall_rx")
                && live.contains("sync_rx")
                && live.contains("inhabit_rx")
                && live.contains("reflect_rx")
                && live.contains("session_show_rx")
                && live.contains("import_rx")
                && live.contains("acp_spawn_rx")
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
                && live.contains("doctor_line_busy"),
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
        let teach = fn_src(&src, "teach_watched_routine");
        let finish = fn_src(&src, "finish_acp_turn");
        let apply = fn_src(&src, "apply_single_turn");
        assert!(
            teach.contains("user_asked_to_schedule")
                && !finish.contains("save_schedule")
                && !apply.contains("save_schedule"),
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
            src.contains("ignore_close_request")
                && src.contains("self.want_quit"),
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
        let persist = format!("{}{}", fn_src(&src, "persist"), fn_src(&src, "persist_snap"));
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
    fn greeting_and_chips_use_grok_cli() {
        let src = cabin_src();
        let greet = src
            .split("fn spawn_greeting_llm(")
            .nth(1)
            .and_then(|s| s.split("fn poll_goals(").next())
            .expect("spawn_greeting_llm");
        assert!(
            greet.contains("cabin_fast_llm") && greet.contains("find_grok"),
            "greeting Fast must run through grok -p when cabin OAuth is empty: {greet}"
        );
        let fast = src
            .split("fn cabin_fast_llm(")
            .nth(1)
            .and_then(|s| s.split("fn mode_status_line(").next())
            .expect("cabin_fast_llm");
        assert!(
            fast.contains("CABIN_FAST_MODEL") && fast.contains("grok_cli_key"),
            "chips/greeting use grok-4.7 via grok login: {fast}"
        );
        assert!(
            fast.contains("CABIN_FAST_FALLBACK"),
            "chips/greeting Fast must fall back if 4.1 Fast is empty: {fast}"
        );
        let chips = src
            .split("fn spawn_chip_llm(")
            .nth(1)
            .and_then(|s| s.split("fn apply_chip(").next())
            .expect("spawn_chip_llm");
        assert!(
            chips.contains("cabin_fast_llm") && chips.contains("find_grok"),
            "chips Fast must run through grok -p when cabin OAuth is empty: {chips}"
        );
        let ready = src
            .split("fn llm_ready(")
            .nth(1)
            .and_then(|s| s.split("fn grok_cwd(").next())
            .expect("llm_ready");
        assert!(
            ready.contains("find_grok"),
            "llm_ready must count the Grok Build CLI: {ready}"
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
        let ensure = fn_src(&src, "ensure_acp");
        assert!(
            ensure.contains("grok_session") && ensure.contains("session_id"),
            "new ACP sessions must bind onto the cabin thread: {ensure}"
        );
        assert!(
            ensure.contains("h.session_id != id") && ensure.contains("return Ok(())"),
            "ACP reuse is exact session id; a live handle with no resume must not be dropped (exit 143): {ensure}"
        );
        assert!(
            ensure.contains("explain_handshake_error") && ensure.contains("spawn(None)"),
            "a dead grok session id must retry session/new without resume: {ensure}"
        );
        assert!(
            ensure.contains("is_session_cwd_error") && ensure.contains("t.grok_cwd"),
            "session/load in a foreign worktree must fail closed, not spawn(None) into the bound tree: {ensure}"
        );
        assert!(
            ensure.contains("unknown_cwd"),
            "a History file-only session must not spawn(None) into the bound tree: {ensure}"
        );
        assert!(
            ensure.contains("session/load refused") && ensure.contains("no worktree"),
            "a History file-only session must not session/load into the bound tree: {ensure}"
        );
        assert!(
            ensure.contains("chat_job_thread"),
            "ACP handshake must bind the job thread, not whichever tab is visible: {ensure}"
        );
        assert!(
            ensure.contains("if grok_login.is_some()") && ensure.contains("(grok_login, None)"),
            "grok login must not also inject a console XAI_API_KEY: {ensure}"
        );
        assert!(
            ensure.contains("find_grok") && ensure.contains("Grok Build CLI is not on PATH"),
            "Ask ACP handshake must fail closed without grok: {ensure}"
        );
        let ensure_spawn = ensure
            .find("thread::spawn")
            .expect("handshake must leave the UI thread");
        let ensure_sess = ensure.find("spawn_session").expect("spawn_session");
        assert!(
            ensure_spawn < ensure_sess,
            "ACP handshake must not freeze the cabin: {ensure}"
        );
        assert!(
            !ensure.contains("bearer()"),
            "ACP spawn must not pass Imagine bearer (JWT) as XAI_API_KEY: {ensure}"
        );
        assert!(
            ensure.contains("console_key")
                && ensure.contains("grok_cli_key")
                && ensure.contains("xai_env"),
            "ACP auth is grok login; XAI_API_KEY is the secrets console key: {ensure}"
        );
        assert!(
            ensure.contains("parse_reasoning_effort") && ensure.contains("cfg.reasoning_effort"),
            "ACP spawn must pass composer reasoning effort to grok agent: {ensure}"
        );
        assert!(
            !ensure.contains("agent_reasoning_effort_for_mode(&self.cfg.mode)"),
            "ACP effort must not route through legacy cfg.mode ladder: {ensure}"
        );
        let bearer = fn_src(&src, "bearer");
        assert!(
            bearer.contains("grok_cli_key")
                && bearer.find("grok_cli_key").unwrap()
                    < bearer.find("oauth_usable").unwrap_or(usize::MAX),
            "Imagine/ACP bearer prefers grok login over cabin OAuth: {bearer}"
        );
        assert!(
            bearer.contains("refresh_grok_login"),
            "grok login JWT must refresh before Imagine 401s: {bearer}"
        );
        assert!(
            bearer.contains("} else {") && bearer.contains("return k;"),
            "a dead grok login JWT must fall through to console key, not keep the expired token: {bearer}"
        );
        assert!(
            bearer.contains("hard_expired"),
            "skew-stale grok login must still be used while refresh is off the UI thread: {bearer}"
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
            persist.contains("snap.secrets = Some") && persist.contains("persist_io"),
            "foreground persist must write secrets under persist_io: {persist}"
        );
        let persist_spawn = persist
            .find("thread::spawn")
            .expect("persist must leave the UI thread");
        let persist_write = persist.find("write_persist_disk").expect("persist writes");
        assert!(
            persist_spawn < persist_write && persist.contains("io.lock()"),
            "foreground persist must not freeze the cabin writing threads.json: {persist}"
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
                && kick.contains("spawn_grok_p_stream")
                && kick.contains("image"),
            "a plus-button still must ride the Grok Build turn: {kick}"
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
        let cwd = format!("{}{}", fn_src(&src, "grok_cwd"), fn_src(&src, "grok_cli_cwd"));
        assert!(
            cwd.contains("cabin_session_cwd") && cwd.contains("self.grok_cwd()"),
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
            .and_then(|s| s.split("fn send_grok_slash(").next())
            .expect("apply_single_turn");
        assert!(
            saved.contains("session_saved")
                && saved.contains("grok_sessions_loaded = false")
                && saved.contains("reload_grok_sessions"),
            "a dialogue session must show in History after it is created: {saved}"
        );
        let inspect = src
            .split("Slash::Inspect =>")
            .nth(1)
            .and_then(|s| s.split("Slash::ProjectBind").next())
            .expect("inspect");
        assert!(
            inspect.contains("grok_cwd") && !inspect.contains("current_dir"),
            "/inspect must use the bound tree or work root, not the cabin process cwd: {inspect}"
        );
        let inspect_spawn = inspect
            .find("thread::spawn")
            .expect("inspect must leave the UI thread");
        let inspect_json = inspect.find("inspect_json").expect("inspect_json");
        assert!(
            inspect_spawn < inspect_json,
            "/inspect must not block the cabin on grok inspect: {inspect}"
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
            bind.contains("acp_spawn_rx = None"),
            "/project bind during handshake must drop the in-flight agent: {bind}"
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
            clear.contains("acp_spawn_rx = None"),
            "/project clear during handshake must drop the in-flight agent: {clear}"
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
            sidebar.contains("acp_spawn_rx = None") && sidebar.contains("self.acp = None"),
            "sidebar bind during handshake must drop the in-flight agent: {sidebar}"
        );
        assert!(
            sidebar.contains("halt_in_flight") && !sidebar.contains("self.acp.is_some()"),
            "sidebar bind during handshake must halt, not only drop a live ACP handle: {sidebar}"
        );
        assert!(
            sidebar.contains("grok_cwd = None") && sidebar.contains("grok_session = None"),
            "sidebar bind must forget the thread worktree or the next send stays in a History tree: {sidebar}"
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
            room.contains("acp_spawn_rx = None"),
            "/room during handshake must drop the in-flight agent: {room}"
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
        let ext = src
            .split("fn run_grok_extension(")
            .nth(1)
            .and_then(|s| s.split("fn doctor_text(").next())
            .expect("run_grok_extension");
        assert!(
            ext.contains("grok_cwd") && !ext.contains("current_dir"),
            "Connectors inspect must use grok_cwd, not `.`: {ext}"
        );
        let ext_spawn = ext
            .find("thread::spawn")
            .expect("extension must leave the UI thread");
        let ext_out = ext.find("grok_stdout").expect("grok_stdout");
        assert!(
            ext_spawn < ext_out,
            "Connectors inspect/mcp/plugin must not freeze the cabin: {ext}"
        );
        let fast = src
            .split("fn cabin_fast_llm(")
            .nth(1)
            .and_then(|s| s.split("fn mode_status_line(").next())
            .expect("cabin_fast_llm");
        assert!(
            fast.contains("resolve_acp_cwd") && !fast.contains("current_dir"),
            "grok -p fallback must not inherit the overlay cwd: {fast}"
        );
        let open = src
            .split("fn open_grok_session(")
            .nth(1)
            .and_then(|s| s.split("fn ensure_acp(").next())
            .expect("open_grok_session");
        assert!(
            open.contains("show_session") && open.contains("read_file_capped"),
            "opening a grok session must load the transcript: {open}"
        );
        assert!(
            open.contains("read_file_capped") && !open.contains("read_to_string"),
            "opening a grok session must not slurp a huge markdown dump: {open}"
        );
        assert!(
            open.contains("grok_cwd"),
            "History open must remember the session worktree: {open}"
        );
        let open_spawn = open
            .find("thread::spawn")
            .expect("show_session must leave the UI thread");
        let open_show = open.find("show_session").expect("show_session");
        let open_read = open.find("read_file_capped").expect("read_file_capped");
        let open_find = open.find("find_grok").expect("find_grok");
        assert!(
            open_spawn < open_show && open_spawn < open_read && open_spawn < open_find,
            "opening a grok session must not block on grok export/show: {open}"
        );
        assert!(
            open.contains("apply_switch_thread") && open.contains("self.persist()"),
            "opening a grok session must not clone every thread twice: {open}"
        );
        let reload = src
            .split("fn reload_grok_sessions(")
            .nth(1)
            .and_then(|s| s.split("fn poll_grok_sessions(").next())
            .expect("reload_grok_sessions");
        let spawn = reload
            .find("thread::spawn")
            .expect("reload must leave the UI thread");
        let list = reload
            .find("list_sessions")
            .expect("reload lists grok sessions");
        assert!(
            spawn < list,
            "History must list grok sessions off the UI thread: {reload}"
        );
        assert!(
            !reload.contains("discover_session_files"),
            "History must not walk disk (subagents) — grok sessions list only: {reload}"
        );
        let kick = fn_src(&src, "kick_imagine");
        assert!(
            kick.contains("bearer()")
                && kick.contains("console_key()")
                && !kick.contains("has_key()"),
            "Imagine prefers a console API key, then grok login: {kick}"
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
            kick.matches("bearer()").count(),
            1,
            "Imagine must not refresh grok login twice on the UI thread: {kick}"
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
        let spawn_poll = src
            .split("fn poll_acp_spawn(")
            .nth(1)
            .and_then(|s| s.split("fn open_grok_session(").next())
            .expect("poll_acp_spawn");
        let spawn_ok = spawn_poll
            .split("Ok(Ok(h))")
            .nth(1)
            .and_then(|s| s.split("Ok(Err(e))").next())
            .expect("spawn ok");
        assert!(
            spawn_ok.contains("grok_session") && spawn_ok.contains("self.persist()"),
            "handshake must persist the session id before the first turn: {spawn_ok}"
        );
        assert!(
            spawn_ok.contains("chat_job_thread"),
            "handshake stamp must follow the job thread, not whichever tab is visible: {spawn_ok}"
        );
        let spawn_drop = spawn_poll
            .split("TryRecvError::Disconnected")
            .nth(1)
            .and_then(|s| s.split("fn open_grok_session").next())
            .expect("spawn disconnected");
        assert!(
            spawn_drop.contains("apply_job_fail") && spawn_drop.contains("self.persist()"),
            "a dropped handshake must persist the fail turn or persist_bg waits 2s: {spawn_drop}"
        );
        assert!(
            spawn_drop.contains("fail_ask_without_acp") && spawn_drop.contains("uses_acp"),
            "Ask handshake death must deny the turn, not fall through to grok -p: {spawn_drop}"
        );
        let spawn_err = spawn_poll
            .split("Ok(Err(e))")
            .nth(1)
            .and_then(|s| s.split("TryRecvError::Empty").next())
            .expect("spawn err");
        assert!(
            spawn_err.contains("fail_ask_without_acp") && spawn_err.contains("uses_acp"),
            "Ask ACP spawn fail must deny, not start grok -p: {spawn_err}"
        );
        let show = src
            .split("fn poll_session_show(")
            .nth(1)
            .and_then(|s| s.split("fn poll_acp_spawn(").next())
            .expect("poll_session_show");
        assert!(
            show.contains("persist_bg") && show.contains("parse_session_markdown"),
            "History show must persist the transcript, not wait for the next idle tick: {show}"
        );
    }

    #[test]
    fn hide_pending_grok_sessions_drops_in_flight_deletes() {
        let a = grokhub_acp::split_session_row("01a01b0f-7e06-74b1-8f22-5236c9d57d45  Keep");
        let b = grokhub_acp::split_session_row("01a01b0f-7e06-74b1-8f22-5236c9d57d46  Drop");
        let mut pending = std::collections::HashSet::new();
        pending.insert(b.id.clone());
        let shown = super::hide_pending_grok_sessions(vec![a.clone(), b], &pending);
        assert_eq!(shown.len(), 1, "{shown:?}");
        assert_eq!(shown[0].id, a.id);
        assert_eq!(
            super::hide_pending_grok_sessions(vec![a.clone()], &std::collections::HashSet::new())
                .len(),
            1
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
            rail.contains("grok_sessions") && rail.contains("OpenGrok"),
            "sidebar History must be grok sessions list, not cabin leftover Chat tabs: {rail}"
        );
        assert!(
            !rail.contains("discover_session_files") && !rail.contains("rail_history_order"),
            "sidebar History must not walk session dirs or cabin threads: {rail}"
        );
        assert!(
            rail.contains("TabAct::DeleteGrok") && rail.contains("button(\"Delete\")"),
            "sidebar Grok session rows must offer Delete: {rail}"
        );
        assert!(
            rail.contains("reload_grok_sessions"),
            "sidebar History must load Grok sessions so names can appear: {rail}"
        );
        assert!(
            rail.contains("delete_grok_history") || rail.contains("TabAct::DeleteGrok(id)"),
            "sidebar DeleteGrok must drop the session from the list: {rail}"
        );
        assert!(
            rail.contains("pending_grok_deletes"),
            "sidebar must hide a session while grok sessions delete is still running: {rail}"
        );
        let page = src
            .split("crate::cards::section_label(ui, \"Grok Build sessions\")")
            .nth(1)
            .and_then(|s| s.split("fn ui_board(").next())
            .expect("history grok section");
        assert!(
            page.contains("s.title") && page.contains("grok_sessions"),
            "History page must paint grok sessions list titles: {page}"
        );
        assert!(
            page.contains("Delete") && page.contains("delete_grok_history"),
            "History page must delete Grok sessions from the list: {page}"
        );
        assert!(
            page.contains("pending_grok_deletes"),
            "History page must hide a session while grok sessions delete is still running: {page}"
        );
        let forget = src
            .split("fn forget_grok_build_session(")
            .nth(1)
            .and_then(|s| s.split("fn delete_grok_history(").next())
            .expect("forget_grok_build_session");
        let del = forget.find("delete_session").expect("forget deletes");
        let list = forget
            .find("list_sessions")
            .expect("forget lists after delete");
        assert!(
            del < list,
            "History delete must run grok sessions delete before listing or the row comes back: {forget}"
        );
        let delh = src
            .split("fn delete_grok_history(")
            .nth(1)
            .and_then(|s| s.split("fn reload_grok_sessions(").next())
            .expect("delete_grok_history");
        assert!(
            delh.contains("forget_grok_build_session") && !delh.contains("reload_grok_sessions"),
            "Delete must not list until grok sessions delete finishes: {delh}"
        );
        let dta = src
            .split("fn delete_thread_at")
            .nth(1)
            .and_then(|s| s.split("fn delete_all_history").next())
            .expect("delete_thread_at");
        assert!(
            dta.contains("forget_grok_build_session") && !dta.contains("reload_grok_sessions"),
            "deleting a linked tab must not list until grok sessions delete finishes: {dta}"
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
            deleted.contains("forget_grok_build_session"),
            "deleting a History chat must drop the attached Grok Build session: {deleted}"
        );
        let title = src
            .split("fn thread_rail_title(")
            .nth(1)
            .and_then(|s| s.split("fn forget_grok_build_session(").next())
            .expect("thread_rail_title");
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
            .split("fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame)")
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
            .and_then(|s| s.split("fn refresh_eyes(").next())
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
        let beat = src
            .split("fn tick_heartbeat")
            .nth(1)
            .and_then(|s| s.split("fn tick_anticipate").next())
            .expect("tick_heartbeat");
        assert!(
            beat.contains("self.tick_loops()") && beat.contains("self.tick_night()"),
            "the Night slot owns both schedulers — a 09:00 automation must still fire: {beat}"
        );
        let night = src
            .split("fn tick_night(")
            .nth(1)
            .and_then(|s| s.split("fn poll_night_check(").next())
            .expect("tick_night");
        assert!(
            night.contains("last_auto_tick") && !night.contains("last_night_tick"),
            "automations need their own debounce or an idle loop list starves them: {night}"
        );
        let loops = src
            .split("fn tick_loops(")
            .nth(1)
            .and_then(|s| s.split("fn poll_grok_loop(").next())
            .expect("tick_loops");
        assert!(
            loops.contains("last_night_tick") && !loops.contains("last_auto_tick"),
            "loops keep their own debounce: {loops}"
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
            .and_then(|s| s.split("fn poll_voice(").next())
            .expect("show_from_tray");
        assert!(
            show.contains("apply_saved_geom"),
            "Show cabin must restore size and position: {show}"
        );
        let exit = src
            .split("fn on_exit(")
            .nth(1)
            .and_then(|s| s.split("fn update(").next())
            .expect("on_exit");
        assert!(
            exit.contains("config::save") && exit.contains("cfg.window")
                || exit.contains("config::save(&cfg)"),
            "SIGTERM must write the remembered window: {exit}"
        );
        let flush = src
            .split("fn flush_window(")
            .nth(1)
            .and_then(|s| s.split("fn persist(").next())
            .expect("flush_window");
        let flush_spawn = flush
            .find("thread::spawn")
            .expect("geom flush must leave the UI thread");
        let flush_save = flush
            .find("config::save")
            .expect("geom flush writes app.json");
        assert!(
            flush_spawn < flush_save && flush.contains("persist_io"),
            "window geom must not freeze the cabin writing app.json: {flush}"
        );
        let bg = src
            .split("fn persist_bg(")
            .nth(1)
            .and_then(|s| s.split("\n    fn ").next())
            .expect("persist_bg");
        let spawn = bg
            .find("thread::spawn")
            .expect("persist_bg must leave the UI thread");
        let save = bg
            .find("write_persist_disk")
            .expect("periodic persist must write on the worker");
        assert!(
            spawn < save,
            "periodic persist must write after spawn: {bg}"
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
    fn refresh_eyes_captures_off_the_ui_thread() {
        let src = cabin_src();
        let eyes = src
            .split("fn refresh_eyes")
            .nth(1)
            .and_then(|s| s.split("fn halt_work").next())
            .expect("refresh_eyes");
        let spawn = eyes
            .find("thread::spawn")
            .expect("Eyes Scan grim must leave the UI thread");
        let shot = eyes.find("capture_data_url").expect("screen capture");
        assert!(spawn < shot, "Eyes Scan must not block the cabin: {eyes}");
        assert!(
            eyes.contains("lock_titles") && eyes.contains("should_send_screenshot"),
            "Eyes Scan lock gates stay on the UI thread: {eyes}"
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
        let kick = src
            .split("fn kick_model(")
            .nth(1)
            .and_then(|s| s.split("fn upsert_stream_assistant").next())
            .expect("kick_model");
        assert!(
            kick.contains("uses_acp")
                && kick.contains("ensure_acp")
                && kick.contains("prompt_with_image")
                && kick.contains("fail_ask_without_acp"),
            "Ask must start ACP so Allow / Deny can show: {kick}"
        );
        assert!(
            kick.contains("spawn_grok_p_stream") && kick.contains("grok_p_rx"),
            "Auto/Always stay on grok -p: {kick}"
        );
        assert!(
            kick.contains("parse_reasoning_effort") && kick.contains("cfg.reasoning_effort"),
            "grok -p must use the Effort dropdown, not the leftover mode ladder: {kick}"
        );
        assert!(
            kick.contains("cabin_has_session"),
            "do not --resume a ~/.grok session id into isolated cabin GROK_HOME: {kick}"
        );
        assert!(
            kick.contains("grok_user_home = user_home"),
            "new GrokHub chats must use ~/.grok so Grok has this desktop: {kick}"
        );
        assert!(
            kick.contains("apply_job_fail"),
            "session/new failure must land in the chat, not only the 72-char status clip: {kick}"
        );
        assert!(
            kick.contains("pending_kick")
                && kick.contains("kick_cap_rx")
                && kick.contains("grok_p_rx"),
            "kick_model must wait for the off-thread frame and grok -p instead of blocking: {kick}"
        );
        let ask_kick = fn_src(&src, "kick_model");
        let sched_gate = ask_kick
            .find("!self.scheduled_perm")
            .expect("scheduled Ask must skip ACP");
        let ask_gate = ask_kick
            .find("uses_acp")
            .expect("Ask permission must choose ACP");
        let grok_p = ask_kick
            .find("spawn_grok_p_stream")
            .expect("Auto/Always grok -p");
        assert!(
            sched_gate < ask_gate && ask_gate < grok_p,
            "scheduled Ask must skip ACP before headless grok -p: {ask_kick}"
        );
        let ask_arm = &ask_kick[ask_gate..grok_p];
        assert!(
            ask_arm.contains("ensure_acp")
                && ask_arm.contains("prompt_with_image")
                && ask_arm.contains("fail_ask_without_acp")
                && ask_arm.contains("return")
                && !ask_arm.contains("spawn_grok_p_stream"),
            "Ask + ACP down must deny and must not sandbox-off grok -p: {ask_arm}"
        );
        assert!(
            ask_kick.contains("scheduled_flags")
                && ask_kick.contains("composer_headless_flags")
                && ask_kick.contains("self.session_mode")
                && ask_kick[grok_p..].contains("spawn_grok_p_stream"),
            "Look + Auto/Always stay on grok -p with look-only flags: {ask_kick}"
        );
        assert!(
            ask_kick.contains("apply_skill_follow") && ask_kick.contains("active_skill_follow"),
            "selecting a skill must inject the follow block into grok -p / ACP: {ask_kick}"
        );
    }

    #[test]
    fn scheduled_night_loop_and_phone_inherit_permission_mode() {
        let src = cabin_src();
        let fire_loop = fn_src(&src, "fire_loop");
        assert!(
            fire_loop.contains("scheduled_args") && fire_loop.contains("permission_mode"),
            "loop spawn must read the composer PermissionMode pill: {fire_loop}"
        );
        assert!(
            !fire_loop.contains("\"--always-approve\""),
            "Ask must not silent always-approve a loop: {fire_loop}"
        );
        let fire_night = fn_src(&src, "fire_night");
        assert!(
            fire_night.contains("send_scheduled_chat"),
            "night chat must inherit PermissionMode, not a separate yolo path: {fire_night}"
        );
        let send_at = fire_night
            .find("send_scheduled_chat")
            .expect("night send");
        let ran_at = fire_night
            .rfind("mark_auto_ran")
            .expect("night mark ran");
        assert!(
            send_at < ran_at,
            "night must mark ran after a live kick, not before: {fire_night}"
        );
        assert!(
            fire_night.contains("self.running")
                && fire_night.contains("pending_kick")
                && fire_night.contains("grok_p_rx"),
            "night marks ran only after a live kick: {fire_night}"
        );
        let send_block = &fire_night[send_at..];
        assert!(
            send_block.contains("mark_auto_ran") && send_block.contains("mark_auto_skipped"),
            "a night send that did not start a live kick must skip, not retry every 5s: {fire_night}"
        );
        let inbox = fn_src(&src, "drain_inbox");
        assert!(
            inbox.contains("send_scheduled_chat"),
            "phone /v1/task must inherit PermissionMode: {inbox}"
        );
        let anticipate = fn_src(&src, "tick_anticipate");
        assert!(
            anticipate.contains("send_scheduled_chat"),
            "heartbeat anticipate must inherit PermissionMode: {anticipate}"
        );
        let kick = fn_src(&src, "kick_model");
        assert!(
            kick.contains("scheduled_perm")
                && kick.contains("scheduled_flags")
                && kick.contains("composer_headless_flags"),
            "kick_model must map Auto/Always and fail-close scheduled Ask: {kick}"
        );
        assert!(
            kick.contains("perm_always_confirm = None"),
            "a kick that clears perm_ask must also drop Always confirm: {kick}"
        );
        let scheduled = fn_src(&src, "send_scheduled_chat");
        assert!(
            scheduled.contains("scheduled_perm = true")
                && scheduled.contains("send_chat")
                && scheduled.contains("scheduled_flags")
                && scheduled.contains("scheduled_args"),
            "scheduled enqueue must pass scheduled_args / scheduled_flags: {scheduled}"
        );
        let fail_ask = fn_src(&src, "fail_ask_without_acp");
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
        let kick_err = fn_src(&src, "kick_model");
        assert!(
            kick_err.contains("scheduled_perm = false"),
            "a failed grok -p spawn must drop scheduled_perm: {kick_err}"
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
            spawn < pick
                && pick < load
                && upload.contains("pick_rx")
                && !upload.contains("apply_path"),
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
        assert!(
            !queue.contains("send_chat")
                && queue.contains("chat_job_thread")
                && queue.contains("push_bound_msg")
                && queue.contains("kick_model"),
            "Queue Run must kick the origin thread, not send_chat on the visible tab: {queue}"
        );
        assert!(
            src.contains("finish_hub_dispatch"),
            "phone dispatch must complete the hub task so GET /v1/results can see it"
        );
        assert!(
            src.contains("hub_dispatch_ok(&text)"),
            "GOAL_BLOCKED must not complete a phone task as done"
        );
        assert!(
            src.contains("visible_goal_step_on_continue"),
            "a background goal continue must not bump the visible tab step"
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
            "Stop / tray halt must complete a claimed phone task"
        );
        assert!(
            src.contains("self.finish_hub_dispatch(worker_gone_status(), false)"),
            "a dropped worker must fail the claimed phone task"
        );
        assert!(
            src.contains("inbox_claim_ready") && src.contains("requeue_claimed_for"),
            "do not claim a phone task without auth, and unstick claimed rows on boot"
        );
        let inbox = src
            .split("fn drain_inbox")
            .nth(1)
            .and_then(|s| s.split("fn finish_hub_dispatch").next())
            .expect("drain_inbox");
        assert!(
            inbox.contains("pending_hub_task.is_some()"),
            "do not claim a second phone task while one is still pending: {inbox}"
        );
        assert!(
            inbox.contains("land_on_real_chat"),
            "a claimed phone task must not land on Scratch: {inbox}"
        );
        assert!(
            inbox.contains("self.can_agent()") && !inbox.contains("self.llm_ready()"),
            "OAuth-only must not claim a phone task — send_chat needs Grok Build: {inbox}"
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
            fire_night.contains("night_unauth_should_skip")
                && fire_night.contains("mark_auto_skipped"),
            "missing OAuth must skip the night slot: {fire_night}"
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
            fire_night.contains("land_on_real_chat"),
            "a night chat job must not land on Scratch: {fire_night}"
        );
        let agent = fire_night
            .find("self.can_agent()")
            .expect("night chat needs Grok Build");
        assert!(
            agent < bump && fire_night.contains("replay.is_none()"),
            "OAuth-only must not burn a night chat slot — send_chat needs Grok Build: {fire_night}"
        );
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
            compact.contains("compact_keep_start_from") && !compact.contains("content.clone()"),
            "/compact must drain dropped turns without cloning an 8MB pane: {compact}"
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
            queued.contains("queue_combined_update")
                && !queued.contains("update_cmds_for")
                && !queued.contains("config::save")
                && !queued.contains("persist_snap"),
            "/update must use the same pending plan as the chip: {queued}"
        );
        assert!(
            src.contains("settings_update_hint")
                && src.contains("queue_combined_update")
                && src.contains("UPDATE_CHECK_EVERY"),
            "one Update control must describe CLI-then-cabin and recheck on the 2h interval: {src}"
        );
        assert!(
            src.contains("Install Grok Build CLI")
                && src.contains("queue_grok_cli_install")
                && src.contains("begin_grok_install_force")
                && src.contains("should_show_manual_cli_install")
                && src.contains("cli_installing"),
            "Settings → Update and Get Started hide Install when grok is present or an alpha install is in progress: {src}"
        );
        let queued_cli = src
            .split("fn queue_combined_update(")
            .nth(1)
            .and_then(|s| s.split("fn note_combined_update_landed(").next())
            .expect("queue_combined_update");
        assert!(
            queued_cli.contains("combined_update_cmds")
                && queued_cli.contains("start_overlay_update")
                && queued_cli.contains("pending_for_manual_update")
                && queued_cli.contains("UpdatePending::Cli")
                && queued_cli.contains("UpdatePending::Both")
                && queued_cli.contains("cabin_skipped")
                && queued_cli.contains("self.open_update_overlay()")
                && queued_cli.find("self.open_update_overlay()").unwrap()
                    < queued_cli.find("combined_update_cmds").unwrap()
                && queued_cli.contains("self.persist_cfg()")
                && queued_cli.contains("overlay_clone_usable")
                && !queued_cli.contains("config::save")
                && !queued_cli.contains("persist_snap")
                && !queued_cli.contains("Set Settings → source")
                && !queued_cli.contains("--stable")
                && !queued_cli.contains("begin_grok_install")
                && !queued_cli.contains("combined_update_hint(UpdatePending::None)"),
            "the one Update control runs pending steps, or both when the probe missed: {queued_cli}"
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
            "boot must check GitHub Latest and CLI alpha for the in-app chip: {boot}"
        );
        assert!(
            src.contains("titlebar_update_chip")
                && src.contains("update_chip_label")
                && src.contains("queue_combined_update")
                && !src.contains(concat!("Update ", "available")),
            "the titlebar chip names CLI, cabin, or both and runs that update: {src}"
        );
        let flush_p = src
            .split("fn flush_projects(")
            .nth(1)
            .and_then(|s| s.split("fn bind_project_id").next())
            .expect("flush_projects");
        let flush_spawn = flush_p
            .find("thread::spawn")
            .expect("folder click must leave the UI thread");
        let flush_save = flush_p.find("save_projects").expect("save_projects");
        assert!(
            flush_spawn < flush_save && flush_p.contains("persist_io"),
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
            "{}{}{}{}{}{}{}",
            fn_src(&src, "stage_new_project"),
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
            .and_then(|s| s.split("fn stage_new_project(").next())
            .expect("apply_project_menu");
        assert!(
            menu.contains("self.flush_projects()")
                && !menu.contains("self.persist()")
                && !menu.contains("persist_snap"),
            "Remove from folder must not clone every thread just to write projects.json: {menu}"
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
            pinned.contains("accessed_ms"),
            "pin must bump accessed_ms or /sync LWW can drop the pin: {pinned}"
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
            spawn_goal.contains("visible_turn_count")
                || spawn_goal.contains("is_workload_user"),
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
            boot.contains("leftover_empty_thread"),
            "boot must drop leftover empty Chat tabs: {boot}"
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
            send_auth.contains("can_agent") && send_auth.contains("kick_model(true)"),
            "typed send must go through Grok Build ACP: {send_auth}"
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
            halt_flight.contains("kill_pid") && halt_flight.contains("grok_p_pid"),
            "Stop must SIGTERM the grok -p child: {halt_flight}"
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
            skills_ui.contains("reload_grok_catalog") && skills_ui.contains("load_grok_catalog")
                || skills_ui.contains("reload_grok_catalog"),
            "Skills must load Grok Build inspect/MCP/plugins: {skills_ui}"
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
        assert!(
            skills_ui.contains("Marketplace")
                && skills_ui.contains("plugin")
                && skills_ui.contains("mcp"),
            "Connectors must show MCP, plugins, and marketplace: {skills_ui}"
        );
        assert!(
            skills_ui.contains("grok_user_stdout_timeout") || src.contains("run_grok_user_cmd"),
            "install/enable must run grok off the UI thread"
        );
        let reload = src
            .split("fn reload_grok_catalog(")
            .nth(1)
            .and_then(|s| s.split("fn poll_grok_catalog(").next())
            .expect("reload_grok_catalog");
        let spawn = reload.find("thread::spawn").expect("catalog spawn");
        let load = reload.find("load_grok_catalog").expect("load_grok_catalog");
        assert!(
            spawn < load,
            "Skills catalog must not freeze the cabin on grok inspect: {reload}"
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
            import[..merge_read].contains("write_memory")
                && import[..merge_read].contains("mem_body"),
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
            (ready.contains("write_cli_auth_if_needed")
                || ready.contains("sync_cli_auth_from_oauth"))
                && (ready.contains("get_started_done = true")
                    || ready.contains("mark_get_started_done")),
            "Connect Grok must sign in grok alpha when CLI is empty: {ready}"
        );
        let boot = src
            .split("pub fn new(hidden: bool)")
            .nth(1)
            .and_then(|s| s.split("fn apply_saved_geom(").next())
            .expect("Cabin::new");
        assert!(
            boot.contains("write_cli_auth_if_needed") || boot.contains("sync_cli_auth_from_oauth"),
            "upgrade: existing cabin OAuth must fill empty grok auth.json: {boot}"
        );
        assert!(
            boot.contains("begin_ensure_grok_alpha")
                && !boot.contains("grok_cli_known_good()")
                && boot.contains("silence_windows_hard_errors"),
            "first launch and reinstall must ensure CLI alpha in the background: {boot}"
        );
        assert!(
            boot.contains("grok_cli_key")
                && boot.contains("mark_get_started_done")
                && boot.contains("official_cli_session"),
            "existing grok login must skip Get Started on upgrade unless this session is installing alpha: {boot}"
        );
        let started = src
            .split("fn ui_get_started(")
            .nth(1)
            .and_then(|s| s.split("fn ui_settings(").next())
            .expect("ui_get_started");
        assert!(
            src.contains("if !self.ui_get_started(ctx)"),
            "first-run sheet must replace empty-cabin chat so it paints on Windows"
        );
        let paint = src
            .split("self.ui_titlebar(ctx);")
            .nth(1)
            .and_then(|s| s.split("if self.palette_open").next())
            .expect("titlebar then panes");
        let settings_nav = paint
            .find("if self.nav == Nav::Settings")
            .expect("Settings after Get Started skip");
        let skip = paint
            .find("if !self.ui_get_started(ctx)")
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
            started.contains("Installing Grok Build CLI (alpha)"),
            "install wait copy must say alpha: {started}"
        );
        assert!(
            started.contains("should_show_get_started_now") && started.contains("start_oauth"),
            "Get Started must use cabin device-code OAuth: {started}"
        );
        assert!(
            started.contains("official_cli_session"),
            "official alpha install this session must still open Get Started after grok lands: {started}"
        );
        assert!(
            started.contains("should_show_cli_install_wait"),
            "Get Started wait sheet must use the official alpha wait predicate: {started}"
        );
        assert!(
            started.contains("oauth_err") && started.contains("oauth_busy"),
            "Get Started must show OAuth errors and not restart an in-flight wait: {started}"
        );
        assert!(
            started.contains("Install Grok Build CLI")
                && started.contains("queue_grok_cli_install")
                && started.contains("should_show_manual_cli_install"),
            "first-run Install hides when grok is present or an alpha install is already scheduled: {started}"
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
        let kick = format!(
            "{}{}",
            fn_src(&src, "kick_model"),
            fn_src(&src, "poll_single")
        );
        assert!(
            kick.contains("spawn_grok_p_stream") && kick.contains("is_sigterm_status"),
            "kick_model uses grok -p and must not surface leader SIGTERM as a chat error: {kick}"
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
            anticipate.contains("self.can_agent()") && !anticipate.contains("self.llm_ready()"),
            "OAuth-only must not burn an anticipate slot — send_chat needs Grok Build: {anticipate}"
        );
        let start_hub = format!("{}{}", fn_src(&src, "start_hub"), fn_src(&src, "bind_lan_hub"));
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
        let eyes = src
            .split("fn refresh_eyes")
            .nth(1)
            .and_then(|s| s.split("fn halt_work").next())
            .expect("refresh_eyes");
        let store = eyes.find("store_hub_frame").expect("eyes store");
        assert!(
            eyes[..store].contains("should_send_screenshot")
                || eyes[..store].contains("lock_blocks_hands"),
            "Eyes Scan must not put a lock-screen frame on the hub: {eyes}"
        );
        assert!(
            eyes[..store].contains("lock_titles"),
            "Eyes Scan must see lock windows that collect_rows drops: {eyes}"
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
            undo.contains("followup_step = 0") && undo.contains("active_skill_follow = None"),
            "/undo must reset followup budget like /clear: {undo}"
        );
        assert!(
            undo.contains("stamp_current_access") || undo.contains("accessed_ms"),
            "/undo must bump accessed_ms or /sync LWW can restore the undone turn: {undo}"
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
            settings_save.contains("sync_hub_voice"),
            "Settings Save must refresh the hub voice mint key: {settings_save}"
        );
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
            !sync.contains("t.messages.clone()") && sync.contains("write_persist_disk"),
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
            send[queued..].contains("self.persist_hub()")
                && !send[queued..].contains("persist_snap"),
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
            inhabit.contains("inhabit_claim_allowed") && inhabit.contains("to_id"),
            "/inhabit must name a real peer and skip headphones-as-phone: {inhabit}"
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
            greet.contains("greeting_prompt"),
            "greeting Fast prompt must be built from borrowed USER/MEMORY: {greet}"
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
            "phone task completion must hit hub-state.json without cloning every thread: {finish}"
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
            "slash consult during a phone job must fail the dispatch: {consult}"
        );
        let consult_out = src
            .split("Ok(JobOut::Consult(detail))")
            .nth(1)
            .and_then(|s| s.split("Ok(JobOut::HostLine").next())
            .expect("Consult");
        assert!(
            !consult_out.contains("finish_hub_dispatch"),
            "consult must not complete a phone task as the consult reply: {consult_out}"
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
            "finish the phone task before starting consult: {chat_consult}"
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
            host_done.contains("parse_computer_op")
                && !host_done.contains("parse_computer_cmd_loose"),
            "a leftover type cargo in last_host must not be labeled COMPUTER_RESULT: {host_done}"
        );
        assert!(
            host_done.contains("append_host_trajectory")
                && host_done.contains("trim_job_result_dumps"),
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
            run_cmds.contains("!is_rewind_copy_cmd")
                && run_cmds.contains("host_cmd_leaves_project"),
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
            slice.contains("paint_perm_ask"),
            "empty home must still show a live permission bar: {slice}"
        );
        assert!(
            slice.contains("paint_empty_pulse")
                && slice.contains("pulse_should_paint")
                && slice.contains("empty_home_composer_top"),
            "signed-in empty home paints the pulse under the greeting: {slice}"
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
                let card = grokhub_acp::ToolCard {
                    id: "t1".into(),
                    title: "run_terminal_cmd".into(),
                    kind: "execute".into(),
                    status: "completed".into(),
                    detail: "ran ls -la and printed a long listing that must stay hidden".into(),
                    diff: String::new(),
                    image_data_url: None,
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
            bar.contains("Compact") && bar.contains("Slash::Compact"),
            "Compact sits on the existing context bar: {bar}"
        );
        assert!(
            src.contains("Copy session")
                && src.contains("\"Export\"")
                && src.contains("View plan")
                && src.contains("How fork works")
                && src.contains("fork_offer_why"),
            "session export, view plan, and fork chrome must be in the cabin"
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
            leave.contains("s.halt()") && leave.contains("Voice off"),
            "leave voice must close the socket: {leave}"
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
            .and_then(|s| s.split("fn refresh_eyes(").next())
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
    fn other_chip_threads_skip_current_and_scratch() {
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
        let others = super::collect_other_chip_threads(&[current, prev, scratch], "cur");
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].title, "Night cabin");
        assert_eq!(others[0].last_user, "paint the wall");
    }

    #[test]
    fn chat_composer_pins_stop_on_the_right() {
        let src = cabin_src();
        let start = src.find("ComposerStackSlot::Pill =>").expect("pill arm");
        let pill = &src[start..start + 10000];
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
            home.contains("allocate_new_ui")
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
    fn nightly_review_stays_quiet() {
        let src = cabin_src();
        let tick = src
            .split("fn tick_review(")
            .nth(1)
            .and_then(|s| s.split("fn review_digest(").next())
            .expect("tick_review");
        assert!(
            !tick.contains("send_chat")
                && !tick.contains("Nav::Chat")
                && !tick.contains("self.running"),
            "tick_review must not open Chat or take the composer: {tick}"
        );
        let spawn = src
            .split("fn spawn_review(")
            .nth(1)
            .and_then(|s| s.split("fn poll_review(").next())
            .expect("spawn_review");
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
            .split("fn review_digest(")
            .nth(1)
            .and_then(|s| s.split("fn spawn_review(").next())
            .expect("review_digest");
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
            night.contains("review_status_line"),
            "Suggested header shows Reviewed today / due tonight: {night}"
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
            .split("checkbox")
            .nth(1)
            .and_then(|s| s.split("ui.vertical").next())
            .expect("loop enable");
        assert!(
            enable.contains(".changed()") && enable.contains("persist_loops"),
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
        let fire = src
            .split("fn fire_loop(")
            .nth(1)
            .and_then(|s| s.split("fn tick_night(").next())
            .expect("fire_loop");
        assert!(
            fire.contains("grok_user_stdout_timeout")
                && fire.contains("-p")
                && fire.contains("--verbatim")
                && fire.contains("thread::spawn")
                && fire.contains("scheduled_args")
                && fire.contains("permission_mode")
                && !fire.contains("\"--always-approve\""),
            "loop Run must inherit the PermissionMode pill — no silent always-approve: {fire}"
        );
        let skills = src
            .split("fn ui_skills(")
            .nth(1)
            .and_then(|s| s.split("fn project_row_active(").next())
            .expect("ui_skills");
        assert!(
            skills.contains("Marketplace")
                && skills.contains("MCP servers")
                && skills.contains("Grok Build skills"),
            "Skills and Connectors must show Grok Build skills, MCP, and marketplace: {skills}"
        );
        assert!(
            skills.contains("plugin install") || skills.contains("\"install\""),
            "Marketplace Install must call grok plugin install: {skills}"
        );
        assert!(
            skills.contains("mcp") && skills.contains("add") && skills.contains("doctor"),
            "Connectors must expose grok mcp add/doctor: {skills}"
        );
        assert!(
            skills.contains("uninstall") && skills.contains("plugin") && skills.contains("update"),
            "Connectors must expose grok plugin uninstall/update: {skills}"
        );
        assert!(
            skills.contains("Suggested")
                && skills.contains("merge_suggested_skills")
                && skills.contains("add_suggested_skill"),
            "Skills Suggested tiles must Add via save_skill: {skills}"
        );
        assert!(
            skills.contains("GITHUB_TILES")
                && skills.contains("Save PAT")
                && skills.contains("run_connector")
                && skills.contains("github_token")
                && !skills.contains("create_pr")
                && !skills.contains("outlook")
                && !skills.contains("gmail"),
            "Connectors GitHub tiles + PAT must stay read-only: {skills}"
        );
        let add_skill = fn_src(&src, "add_suggested_skill");
        assert!(
            add_skill.contains("skill_from_suggestion")
                && add_skill.contains("save_skill")
                && add_skill.contains("thread::spawn")
                && add_skill.contains("persist_suggestions"),
            "Suggested Add must write SKILL.md off the UI thread: {add_skill}"
        );
        let slash = fn_src(&src, "send_grok_slash");
        assert!(
            slash.contains("uses_acp")
                && slash.contains("fail_ask_without_acp")
                && slash.contains("composer_headless_flags")
                && slash.contains("self.session_mode")
                && !slash.contains("SessionMode::Chat")
                && !slash.contains("true,\n            false,"),
            "/workflow /compact /rewind must honor the PermissionMode pill: {slash}"
        );
    }

    #[test]
    fn stream_deltas_do_not_grow_without_bound() {
        let src = cabin_src();
        let poll = fn_src(&src, "poll_single");
        assert!(
            poll.contains("GrokPEvent::Thought")
                && poll.contains("GrokPEvent::Text")
                && poll.contains("push_stream_capped")
                && poll.contains("IMAGE_FILE_CAP"),
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
        let follow = fn_src(&src, "send_followup_turn");
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
            follow.contains("FOLLOWUP_MAX_STEPS") && follow.contains("followup_step"),
            "auto-follow is capped per user turn: {follow}"
        );
        assert!(
            follow.contains("kick_model(false)") && !follow.contains("send_chat("),
            "follow-up kicks a quiet continue, not send_chat: {follow}"
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
            !mid.contains("send_chat")
                && !mid.contains("Nav::Chat")
                && !mid.contains("self.running"),
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
            side.contains("composer_want_focus = true") && side.contains("OpenGrok"),
            "sidebar History clicks must focus the composer: {side}"
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
            house.contains("stamp_current_access") && house.contains("Nav::Chat"),
            "Housekeep stamps access while sitting on Chat: {house}"
        );
        let idle = src
            .split("HeartbeatAct::Reflect =>")
            .nth(1)
            .and_then(|s| s.split("HeartbeatAct::Anticipate =>").next())
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
            "pulse must not re-center the chip row: {home}"
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
        let pulse = include_str!("pulse.rs");
        assert!(
            pulse.contains("allocate_exact_size")
                && pulse.contains("pulse_row_label")
                && pulse.contains(".wrap()")
                && !pulse.contains(".truncate()")
                && !pulse.contains("status_chip"),
            "pulse rows wrap the full line inside the reserved slot: {pulse}"
        );
        assert!(
            home.contains("paint_lane_chip") && home.contains("paint_device_glance_row"),
            "empty home paints Coding/Life and a fail-soft device glance: {home}"
        );
        assert!(
            !home.contains("\"Personal\"") && !home.contains("Nav::"),
            "lane chrome must not add a Personal rail or nav id: {home}"
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
                && hist.contains("LastYou")
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
            .and_then(|s| s.split("Slash::Effort(").next())
            .expect("AutoPerm");
        assert!(
            auto.contains("self.confirm = None"),
            "/auto must drop a session Always overlay: {auto}"
        );
        let row = src
            .split("let row = crate::cards::session_row")
            .nth(1)
            .and_then(|s| s.split("ui.allocate_ui_with_layout").next())
            .expect("session_row");
        let perm = row
            .split("if let Some(perm) = row.perm")
            .nth(1)
            .and_then(|s| s.split("if let Some(effort) = row.effort").next())
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
        let acp = include_str!("acp.rs");
        assert!(
            acp.contains("note_live_grok_session")
                && acp.contains("request_grok_sessions_refresh")
                && acp.contains("history_list_refresh_due")
                && acp.contains("grok_sessions_refresh_pending"),
            "Windows History must index live sessions and re-list when CLI lags"
        );
        let chips = include_str!("../cards.rs");
        let chip_row = chips
            .split("pub fn quick_chip_row(")
            .nth(1)
            .and_then(|s| s.split("pub fn tab_pill(").next())
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
    for label in [
        "Default model",
        "Reasoning effort",
        "Permission",
        "Session mode",
        "Always collapse",
    ] {
        assert!(defaults.contains(label), "missing {label}: {defaults}");
    }
    assert_eq!(defaults.matches("settings_dropdown").count(), 4);
    assert!(defaults.contains("settings_toggle"));
    assert!(defaults.contains("parse_reasoning_effort"));
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
