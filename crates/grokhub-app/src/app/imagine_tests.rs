//! Imagine tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

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
    const NO_KEY: &str = "Sign in with Grok in Settings → AI models, or add a console API key.";
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

    let expected = "Sign in with Grok in Settings → AI models, or add a console API key.";
    assert_eq!(cabin.status, expected);
    assert_eq!(cabin.imagine_error, expected);
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
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
