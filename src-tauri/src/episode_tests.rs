use super::*;

fn dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-episode-{label}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn step(dir: &Path, chat: &str, id: &str, tool: &str, decision: &str, result: &str) {
    let mut span = hx::Span::soft_allow(
        chat,
        tool,
        r#"{"x":1}"#,
        result,
        "c",
        hx::AccessMode::Supervised,
        "grok_build",
    );
    span.decision = decision.into();
    span.goal_step = "Turn on Wi-Fi".into();
    hx::append_span(dir, &span.in_episode(id).on_path("E")).expect("span");
}

fn park(dir: &Path, id: &str, n: u32, class: &str) -> String {
    let park_id = format!("{}{id}-{n}", ep::PARK_PREFIX);
    hx::post_park(
        dir,
        &hx::ParkRequest {
            id: park_id.clone(),
            path: "E".into(),
            tool: "click".into(),
            action: "Click Send".into(),
            class: class.into(),
            ts_ms: n as u64,
        },
    )
    .expect("park");
    park_id
}

#[test]
fn the_panel_lists_only_this_episodes_steps_and_counts_them_in_the_header() {
    let d = dir("panel");
    step(&d, "chat", "ep-1", "click", "allow", "click ok");
    step(&d, "chat", "ep-1", "type", "deny", "denied");
    step(&d, "chat", "ep-1", hx::REPLY_TOOL, "allow", "said it");
    step(&d, "chat", "ep-1", ep::EPISODE_TOOL, "begin", "open");
    step(&d, "chat", "ep-2", "click", "allow", "other episode");
    let p = panel(&d, "chat", "ep-1", 0, 4 * 60_000, None, false, false);
    assert_eq!(p.header, "Desktop session · 2 steps · 4 min");
    assert_eq!(
        p.steps,
        vec![
            StepRow {
                n: 1,
                tool: "click".into(),
                decision: "allow".into(),
                result: "click ok".into(),
                goal_step: "Turn on Wi-Fi".into(),
            },
            StepRow {
                n: 2,
                tool: "type".into(),
                decision: "deny".into(),
                result: "denied".into(),
                goal_step: "Turn on Wi-Fi".into(),
            },
        ]
    );
    assert_eq!((p.state.as_str(), p.ended), ("running", None));
    let one = panel(&d, "chat", "ep-1", 0, 0, None, true, true);
    assert_eq!((one.state.as_str(), one.cua), ("paused", true));
}

#[test]
fn an_end_marker_ends_the_panel_and_a_step_result_never_shows_a_secret() {
    let d = dir("end");
    step(
        &d,
        "chat",
        "ep-1",
        "type",
        "allow",
        "typed xai-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA into the box",
    );
    step(&d, "chat", "ep-1", ep::EPISODE_TOOL, "end", "halt");
    let p = panel(&d, "chat", "ep-1", 0, 0, None, false, false);
    assert_eq!(p.ended.as_deref(), Some("halt"));
    assert_eq!(p.state, "ended");
    assert!(!p.steps[0].result.contains("xai-AAAA"), "{:?}", p.steps[0]);
}

#[test]
fn only_this_episodes_cards_show_and_a_credentials_card_hides_the_picture() {
    let d = dir("parks");
    let mine = park(&d, "ep-1", 3, "send");
    let _other = park(&d, "ep-2", 1, "send");
    let shot = Some("data:image/png;base64,AAAA".to_string());
    let p = panel(&d, "chat", "ep-1", 0, 0, shot.clone(), false, false);
    assert_eq!(
        p.parks,
        vec![ParkRow {
            id: mine,
            tool: "click".into(),
            action: "Click Send".into(),
            class: "send".into(),
        }]
    );
    assert_eq!(p.frame, shot, "a send card keeps the picture");
    park(&d, "ep-1", 4, "credentials");
    let hidden = panel(&d, "chat", "ep-1", 0, 0, shot, false, false);
    assert_eq!(hidden.parks.len(), 2);
    assert_eq!(
        hidden.frame, None,
        "no picture while a credentials step waits"
    );
}

#[test]
fn an_answer_reaches_only_this_episodes_waiting_card_once() {
    let d = dir("answer");
    let mine = park(&d, "ep-1", 3, "send");
    let other = park(&d, "ep-2", 1, "send");
    assert_eq!(
        answer(&d, "ep-1", &other, true),
        Err("That step isn't waiting on this episode.".into())
    );
    assert_eq!(hx::take_answer(&d, &other), None, "nothing was written");
    assert_eq!(answer(&d, "ep-1", &mine, false), Ok(()));
    assert_eq!(
        answer(&d, "ep-1", &mine, true),
        Err("That step already has an answer.".into())
    );
    assert_eq!(hx::take_answer(&d, &mine), Some(false));
}

#[test]
fn write_end_denies_open_cards_and_marks_the_episode_stopped() {
    let d = dir("write-end");
    let mine = park(&d, "ep-1", 3, "delete");
    let other = park(&d, "ep-2", 1, "delete");
    write_end(&d, "chat", "ep-1", "stop");
    let left: Vec<String> = hx::pending_parks(&d).into_iter().map(|p| p.id).collect();
    assert_eq!(left, vec![other]);
    assert!(!left.contains(&mine));
    let p = panel(&d, "chat", "ep-1", 0, 0, None, false, false);
    assert_eq!(p.ended.as_deref(), Some("stop"));
    let denied: Vec<(String, String)> = hx::read_spans(&d, "chat")
        .expect("spans")
        .into_iter()
        .filter(|s| s.decision == "deny")
        .map(|s| (s.tool, s.result))
        .collect();
    assert_eq!(
        denied,
        vec![(
            "click".to_string(),
            "episode ended — fail-closed Deny".to_string()
        ),]
    );
}

#[test]
fn the_stops_that_end_an_episode_are_the_engines_words() {
    for stop in [
        "episode_verified",
        "halted",
        "cancelled",
        "episode_idle",
        "episode_unconfirmed",
    ] {
        assert!(ends(stop), "{stop}");
    }
    for stop in ["end_turn", "episode_ladder_pause", "error", "max_turns"] {
        assert!(!ends(stop), "{stop}");
    }
}

struct Desk {
    locked: bool,
    image: Option<String>,
    failed: bool,
}

impl DesktopOps for Desk {
    fn halted(&self) -> bool {
        false
    }
    fn locked(&self) -> bool {
        self.locked
    }
    fn call(&self, _name: &str, _args: &serde_json::Value) -> ToolOutput {
        ToolOutput {
            text: "windows: Settings".into(),
            image_data_url: self.image.clone(),
            failed: self.failed,
        }
    }
}

fn tapped(desk: Desk) -> (FrameTap, FrameSlot) {
    let slot = FrameSlot::default();
    (
        FrameTap {
            inner: Box::new(desk),
            slot: slot.clone(),
        },
        slot,
    )
}

#[test]
fn a_screenshot_lands_in_the_slot_and_the_tool_output_is_unchanged() {
    let (tap, slot) = tapped(Desk {
        locked: false,
        image: Some("data:image/png;base64,AAAA".into()),
        failed: false,
    });
    assert_eq!(slot.get(), None);
    let out = tap.call("screenshot", &serde_json::json!({}));
    assert_eq!(out.text, "windows: Settings");
    assert_eq!(
        out.image_data_url.as_deref(),
        Some("data:image/png;base64,AAAA")
    );
    assert_eq!(slot.get().as_deref(), Some("data:image/png;base64,AAAA"));
    // A click is not a picture: the slot keeps the last frame.
    tap.call("click", &serde_json::json!({"x": 1, "y": 2}));
    assert_eq!(slot.get().as_deref(), Some("data:image/png;base64,AAAA"));
}

#[test]
fn a_locked_screen_an_oversized_picture_or_a_failed_shot_never_reach_the_page() {
    let (tap, slot) = tapped(Desk {
        locked: true,
        image: Some("data:image/png;base64,AAAA".into()),
        failed: false,
    });
    slot.set(Some("old".into()));
    tap.call("screenshot", &serde_json::json!({}));
    assert_eq!(slot.get(), None, "the lock screen clears the old frame");
    let huge = format!("data:image/png;base64,{}", "A".repeat(FRAME_CAP));
    let (tap, slot) = tapped(Desk {
        locked: false,
        image: Some(huge),
        failed: false,
    });
    tap.call("screenshot", &serde_json::json!({}));
    assert_eq!(slot.get(), None);
    let (tap, slot) = tapped(Desk {
        locked: false,
        image: Some("data:image/png;base64,AAAA".into()),
        failed: true,
    });
    slot.set(Some("kept".into()));
    tap.call("screenshot", &serde_json::json!({}));
    assert_eq!(
        slot.get().as_deref(),
        Some("kept"),
        "a failed shot leaves it alone"
    );
}

#[test]
fn the_watcher_emits_when_the_panel_changes_and_marks_a_closed_episode_ended() {
    let d = dir("watch");
    let open = Open {
        id: "ep-1".into(),
        started_ms: grokhub_core::now_ms(),
        turn: 1,
        paused: false,
        cua: false,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let live = Arc::new(Mutex::new(Some(open)));
    let seen: Arc<Mutex<Vec<Panel>>> = Arc::default();
    let handle = {
        let (stop, live, seen, d) = (stop.clone(), live.clone(), seen.clone(), d.clone());
        std::thread::spawn(move || {
            watch(
                &stop,
                &d,
                "chat",
                &|| live.lock().unwrap().clone(),
                &|| None,
                &mut |p| seen.lock().unwrap().push(p.clone()),
            )
        })
    };
    let wait_for = |want: usize| {
        for _ in 0..100 {
            if seen.lock().unwrap().len() >= want {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("watcher emitted {} panels", seen.lock().unwrap().len());
    };
    wait_for(1);
    step(&d, "chat", "ep-1", "click", "allow", "click ok");
    wait_for(2);
    *live.lock().unwrap() = None;
    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap();
    let got = seen.lock().unwrap().clone();
    let states: Vec<(usize, &str)> = got
        .iter()
        .map(|p| (p.steps.len(), p.state.as_str()))
        .collect();
    assert_eq!(states, vec![(0, "running"), (1, "running"), (1, "ended")]);
}

#[test]
fn the_cua_flag_comes_from_the_saved_app_json_and_defaults_off() {
    let d = dir("cua-flag");
    assert!(!cua_flag(&d));
    std::fs::write(d.join("app.json"), r#"{"deviceName":"cabin"}"#).unwrap();
    assert!(!cua_flag(&d));
    std::fs::write(d.join("app.json"), r#"{"cuaDriver":true}"#).unwrap();
    assert!(cua_flag(&d));
    std::fs::write(d.join("app.json"), "not json").unwrap();
    assert!(!cua_flag(&d));
}

#[test]
fn the_cua_gate_registers_only_on_linux_with_both_switches_and_the_hub() {
    let hub = Some(PathBuf::from("/opt/grokhub/grokhub-hub"));
    assert!(!sync_cua(false, true, hub.clone()), "desktop control off");
    assert!(!sync_cua(true, false, hub.clone()), "cuaDriver off");
    assert!(!sync_cua(true, true, None), "no hub binary");
    assert_eq!(sync_cua(true, true, hub), cfg!(target_os = "linux"));
    assert!(!sync_cua(true, true, None));
}

#[test]
fn the_hub_is_found_next_to_the_app_or_in_the_package_lib_dir() {
    let me = Path::new("/usr/bin/loadout");
    assert_eq!(
        hub_candidates(me, false),
        vec![
            PathBuf::from("/usr/bin/grokhub-hub"),
            PathBuf::from("/usr/bin/../lib/loadout/grokhub-hub"),
        ]
    );
    assert_eq!(
        hub_candidates(Path::new("C:/Apps/Loadout/loadout.exe"), true),
        vec![PathBuf::from("C:/Apps/Loadout/grokhub-hub.exe")]
    );
    assert!(hub_candidates(Path::new("/"), false).is_empty());
}

#[test]
fn the_askpass_server_starts_only_with_a_hub_path_on_linux() {
    assert!(!start_askpass(None), "no grokhub-hub, no askpass server");
    if !cfg!(target_os = "linux") {
        assert!(!start_askpass(Some(PathBuf::from(
            "/opt/loadout/grokhub-hub"
        ))));
    }
}

#[test]
fn the_shell_starts_askpass_with_the_bundled_hub_and_registers_cua_with_it() {
    let lib = include_str!("lib.rs");
    let setup = lib
        .split("episode::start_askpass(episode::hub_exe());")
        .count();
    assert_eq!(setup, 2, "setup starts the askpass server exactly once");
    let episode = include_str!("episode.rs");
    assert!(
        episode.contains("grokhub_agent::sudo_pass::install(exe)"),
        "the askpass helper is the bundled grokhub-hub path"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn cua_on_shows_the_hub_with_mcp_cua_and_either_switch_off_removes_it() {
    let hub = PathBuf::from("/opt/loadout/grokhub-hub");
    assert!(sync_cua(true, true, Some(hub)));
    let row = grokhub_agent::mcp::doctor()
        .into_iter()
        .find(|r| r.name == grokhub_core::CUA_MCP_SERVER)
        .expect("the Cua gate row");
    assert_eq!(row.detail, "/opt/loadout/grokhub-hub --mcp-cua");
    assert!(!sync_cua(
        false,
        true,
        Some(PathBuf::from("/opt/loadout/grokhub-hub"))
    ));
    assert!(!sync_cua(
        true,
        false,
        Some(PathBuf::from("/opt/loadout/grokhub-hub"))
    ));
    assert!(grokhub_agent::mcp::doctor()
        .into_iter()
        .all(|r| r.name != grokhub_core::CUA_MCP_SERVER));
}

#[test]
fn the_linux_bundles_list_grokhub_hub_beside_the_fps_probe() {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
    for kind in ["deb", "appimage"] {
        let files = &conf["bundle"]["linux"][kind]["files"];
        assert_eq!(
            files["/usr/lib/loadout/grokhub-hub"], "../target/hub/release/grokhub-hub",
            "{kind} bundle"
        );
        assert_eq!(
            files["/usr/lib/loadout/fps-probe"], "../target/fps-probe/release/loadout-fps-probe",
            "{kind} bundle"
        );
    }
    let script = include_str!("../../scripts/shell-fps-probe.mjs");
    assert!(script.contains("[\"grokhub-hub\", \"hub\"]"));
}
