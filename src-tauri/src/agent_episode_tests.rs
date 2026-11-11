//! A scripted model and a fixture desktop drive the real session, engine,
//! episode kernel, gate and span store. Nothing touches the real screen.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use grokhub_agent::episode::JUDGE_SYSTEM;
use grokhub_agent::{
    CancelToken, ClientError, ContentPart, DesktopOps, FunctionCall, InputItem, ModelClient,
    PermMode, ResponsesRequest, StreamEvent, ToolOutput, TurnOutput,
};
use serde_json::json;

use super::*;

const CHAT: &str = "ep-shell-chat";
const GOAL: &str = "Turn on Wi-Fi in Settings";

#[derive(Clone, Copy)]
enum Act {
    Click,
    Send,
    Done,
}

struct Model {
    script: Vec<Act>,
    calls: AtomicUsize,
    /// Called with the worker call's index, before it answers.
    on_call: Box<dyn Fn(usize) + Send + Sync>,
}

impl Model {
    fn new(script: Vec<Act>) -> Arc<Self> {
        Arc::new(Self {
            script,
            calls: AtomicUsize::new(0),
            on_call: Box::new(|_| {}),
        })
    }
}

fn first_text(req: &ResponsesRequest) -> String {
    match req.input.first() {
        Some(InputItem::Message { content, .. }) => match content.first() {
            Some(ContentPart::InputText(t)) => t.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

impl ModelClient for Model {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        if first_text(req) == *JUDGE_SYSTEM {
            return Ok(TurnOutput {
                text: "VERIFY_OK".into(),
                ..TurnOutput::default()
            });
        }
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        (self.on_call)(n);
        let act = self.script.get(n).copied().unwrap_or(Act::Done);
        let call = |name: &str, args: serde_json::Value| FunctionCall {
            call_id: format!("c{n}-{name}"),
            name: name.into(),
            arguments: args.to_string(),
        };
        Ok(match act {
            Act::Click => TurnOutput {
                calls: vec![call("click", json!({"x": 10 + n, "y": 40}))],
                ..TurnOutput::default()
            },
            Act::Send => TurnOutput {
                calls: vec![call("click", json!({"x": 900, "y": 40, "label": "Send"}))],
                ..TurnOutput::default()
            },
            Act::Done => TurnOutput {
                text: "All set.\nGOAL_COMPLETE".into(),
                ..TurnOutput::default()
            },
        })
    }
}

static DESK_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The fixture driver: records each call and answers screenshots with a frame.
struct Fixture;

impl DesktopOps for Fixture {
    fn halted(&self) -> bool {
        false
    }
    fn locked(&self) -> bool {
        false
    }
    fn call(&self, name: &str, args: &serde_json::Value) -> ToolOutput {
        let mut log = DESK_LOG.lock().expect("desk log");
        log.push(format!("{name} {}", args["label"].as_str().unwrap_or("")));
        let n = log.len();
        drop(log);
        if name == "screenshot" {
            return ToolOutput {
                text: "windows: Settings".into(),
                image_data_url: Some(format!("data:image/png;base64,FRAME{n}")),
                failed: false,
            };
        }
        ToolOutput::ok(format!("{name} ok"))
    }
}

fn fixture() -> Box<dyn DesktopOps> {
    Box::new(Fixture)
}

fn config(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-episode-run-{label}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("config dir");
    dir
}

fn route(client: Arc<Model>, mode: PermMode) -> Route {
    Route {
        client,
        auth_kind: AuthKind::ApiKey,
        workspace: std::env::temp_dir(),
        mode,
        desktop: true,
        session: SessionMode::Chat,
        voice: String::new(),
        voice_session: None,
        suggestions: false,
    }
}

fn desk_calls() -> Vec<String> {
    DESK_LOG.lock().expect("desk log").clone()
}

fn reset() {
    DESK_LOG.lock().expect("desk log").clear();
}

/// The tests share the fixture's statics, so they run one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn spans(config: &std::path::Path, id: &str) -> Vec<grokhub_agent::harness::Span> {
    grokhub_agent::harness::read_spans(config, CHAT)
        .expect("spans")
        .into_iter()
        .filter(|s| s.episode == id)
        .collect()
}

#[test]
fn an_episode_runs_observe_act_verify_for_two_steps_and_closes_verified() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("two-steps");
    let session = Session::spawn_with(CHAT, cfg.clone(), fixture);
    session.arm_episode(true);
    let model = Model::new(vec![Act::Click, Act::Click, Act::Done]);
    let stop = session.prompt(
        GOAL,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(stop, Ok("episode_verified".to_string()));
    let clicks = desk_calls()
        .iter()
        .filter(|c| c.starts_with("click"))
        .count();
    assert_eq!(clicks, 2);
    assert!(
        desk_calls().iter().any(|c| c.starts_with("screenshot")),
        "it looked first"
    );
    // The episode closed with the verified end; the panel reads two steps.
    assert!(session.episode().is_none());
    let all = grokhub_agent::harness::read_spans(&cfg, CHAT).expect("spans");
    let id = all
        .iter()
        .find(|s| !s.episode.is_empty())
        .expect("episode id")
        .episode
        .clone();
    let panel = crate::episode::panel(&cfg, CHAT, &id, 0, 0, session.frame(), false, false);
    assert_eq!(panel.header, "Desktop session · 2 steps · 0 min");
    assert_eq!(panel.ended.as_deref(), Some("verified"));
    assert!(
        panel
            .frame
            .as_deref()
            .is_some_and(|f| f.starts_with("data:image/png;base64,FRAME")),
        "the thumbnail is the last screenshot: {:?}",
        panel.frame
    );
}

#[test]
fn a_send_step_waits_for_an_ok_in_always_and_a_deny_ends_it_with_nothing_sent() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("deny");
    let session = Session::spawn_with(CHAT, cfg.clone(), fixture);
    session.arm_episode(true);
    let model = Model::new(vec![Act::Click, Act::Send, Act::Done]);
    let first = session.prompt(
        GOAL,
        Vec::new(),
        route(Arc::clone(&model), PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(
        first,
        Ok("end_turn".to_string()),
        "the turn waits, it does not end"
    );
    let open = session
        .episode()
        .expect("the episode stays open on the card");
    let panel = crate::episode::panel(
        &cfg,
        CHAT,
        &open.id,
        open.started_ms,
        open.started_ms,
        None,
        false,
        false,
    );
    assert_eq!(panel.parks.len(), 1, "{panel:?}");
    assert_eq!(panel.parks[0].tool, "click");
    assert_eq!(panel.parks[0].class, "send");
    assert!(
        !desk_calls().iter().any(|c| c == "click Send"),
        "nothing was sent while it waited: {:?}",
        desk_calls()
    );
    crate::episode::answer(&cfg, &open.id, &panel.parks[0].id, false).expect("deny");
    let second = session.prompt(
        "Continue",
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(second, Ok("episode_verified".to_string()));
    assert!(
        !desk_calls().iter().any(|c| c == "click Send"),
        "{:?}",
        desk_calls()
    );
    let denied: Vec<String> = spans(&cfg, &open.id)
        .into_iter()
        .filter(|s| s.decision == "deny" && s.tool == "click")
        .map(|s| s.result)
        .collect();
    assert!(denied.contains(&"denied".to_string()), "{denied:?}");
    assert!(crate::episode::answer(&cfg, &open.id, "epk-nope", true).is_err());
}

#[cfg(desktop)]
#[test]
fn halt_mid_episode_stops_before_the_next_step_and_closes_it() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("halt");
    let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
    session.arm_episode(true);
    // Halt, as the top bar sends it, lands while the second step is thinking.
    let (halter, dir) = (Arc::clone(&session), cfg.clone());
    let model = Arc::new(Model {
        script: vec![Act::Click, Act::Click, Act::Click, Act::Done],
        calls: AtomicUsize::new(0),
        on_call: Box::new(move |n| {
            if n == 1 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                assert_eq!(halt(&dir, [&halter]), vec![CHAT.to_string()]);
            }
        }),
    });
    let stop = session.prompt(
        GOAL,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(stop, Ok("halted".to_string()));
    let clicks = desk_calls()
        .iter()
        .filter(|c| c.starts_with("click"))
        .count();
    assert_eq!(clicks, 1, "{:?}", desk_calls());
    assert!(session.episode().is_none());
    let last = grokhub_agent::harness::read_spans(&cfg, CHAT)
        .expect("spans")
        .into_iter()
        .last()
        .expect("a span");
    assert_eq!(
        (
            last.tool.as_str(),
            last.decision.as_str(),
            last.result.as_str()
        ),
        ("episode", "end", "halt")
    );
}

#[test]
fn pause_keeps_the_episode_open_and_stop_ends_it_and_denies_its_cards() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("pause-stop");
    let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
    session.arm_episode(true);
    assert!(!session.pause_episode(), "nothing runs yet");
    assert!(!session.stop_episode(), "nothing is open yet");
    // The second worker call pauses the run mid-step.
    let pauser = Arc::clone(&session);
    let model = Arc::new(Model {
        script: vec![Act::Click, Act::Click, Act::Click],
        calls: AtomicUsize::new(0),
        on_call: Box::new(move |n| {
            if n == 1 {
                assert!(pauser.pause_episode());
            }
        }),
    });
    let stop = session.prompt(
        GOAL,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(stop, Ok("cancelled".to_string()));
    let open = session.episode().expect("a paused episode stays open");
    assert!(open.paused);
    // A card left open, then Stop with nothing running: it ends and denies the card.
    let card = format!("{}{}-9", grokhub_agent::episode::PARK_PREFIX, open.id);
    grokhub_agent::harness::post_park(
        &cfg,
        &grokhub_agent::harness::ParkRequest {
            id: card.clone(),
            path: "E".into(),
            tool: "click".into(),
            action: "Click Send".into(),
            class: "send".into(),
            ts_ms: 1,
        },
    )
    .expect("park");
    assert!(session.stop_episode());
    assert!(session.episode().is_none());
    assert!(grokhub_agent::harness::pending_parks(&cfg).is_empty());
    let end = spans(&cfg, &open.id).into_iter().last().expect("end span");
    assert_eq!(
        (
            end.tool.as_str(),
            end.decision.as_str(),
            end.result.as_str()
        ),
        ("episode", "end", "stop")
    );
}

#[test]
fn desktop_control_off_runs_a_plain_turn_with_no_episode() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("off");
    let session = Session::spawn_with(CHAT, cfg.clone(), fixture);
    session.arm_episode(true);
    let model = Model::new(vec![Act::Done]);
    let mut r = route(model, PermMode::Always);
    r.desktop = false;
    let stop = session.prompt("hello", Vec::new(), r, Box::new(|_| {}));
    assert_eq!(stop, Ok("end_turn".to_string()));
    assert!(session.episode().is_none());
    assert!(desk_calls().is_empty());
    assert!(grokhub_agent::harness::read_spans(&cfg, CHAT)
        .expect("spans")
        .iter()
        .all(|s| s.episode.is_empty()));
}

#[test]
fn an_unarmed_chat_keeps_the_plain_gate_and_arming_lasts_until_the_session_ends() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("armed");
    let session = Session::spawn_with(CHAT, cfg.clone(), fixture);
    assert!(!session.episode_armed());
    let plain = session.prompt(
        "hello",
        Vec::new(),
        route(Model::new(vec![Act::Done]), PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(plain, Ok("end_turn".to_string()));
    assert!(
        session.episode().is_none(),
        "desktop on alone opens no episode"
    );
    session.arm_episode(true);
    let model = Model::new(vec![Act::Click, Act::Done]);
    let ran = session.prompt(
        GOAL,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(ran, Ok("episode_verified".to_string()));
    assert!(!session.episode_armed(), "a finished session disarms");
    assert!(session.episode().is_none());
}
