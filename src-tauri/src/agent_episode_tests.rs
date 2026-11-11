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
    /// A click at the Send button's point with no label of the model's own:
    /// only the driver's hint says what is there.
    BareSend,
    /// Types a delete command and Enter.
    TypeRm,
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
            JUDGED.lock().expect("judged").push(
                serde_json::to_string(&grokhub_agent::responses_body(req)).unwrap_or_default(),
            );
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
            Act::BareSend => TurnOutput {
                calls: vec![call("click", json!({"x": 900, "y": 40}))],
                ..TurnOutput::default()
            },
            Act::TypeRm => TurnOutput {
                calls: vec![call("type", json!({"text": "rm -rf ~/Documents\n"}))],
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
/// What the independent check was sent, one entry per check.
static JUDGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

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
    /// What the real driver reads beside a call: the control under a click
    /// at the Send button's point, and the window typing goes to.
    fn hint(&self, name: &str, args: &serde_json::Value) -> serde_json::Value {
        let mut hinted = args.clone();
        match name {
            "click" if args["x"] == json!(900) => {
                hinted["_target"] =
                    json!({"label": "Send", "role": "push button", "window": "Mail"});
            }
            "type" => hinted["window"] = json!("Terminal"),
            _ => {}
        }
        hinted
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
    JUDGED.lock().expect("judged").clear();
}

fn judged() -> Vec<String> {
    JUDGED.lock().expect("judged").clone()
}

/// The episode's panel as the page would read it now.
fn panel_of(cfg: &std::path::Path, open: &crate::episode::Open) -> crate::episode::Panel {
    crate::episode::panel(
        cfg,
        CHAT,
        &open.id,
        open.started_ms,
        open.started_ms,
        None,
        false,
        false,
    )
}

/// The episode's begin and end markers, in order.
fn markers(cfg: &std::path::Path, id: &str) -> Vec<(String, String)> {
    spans(cfg, id)
        .into_iter()
        .filter(|s| s.tool == grokhub_agent::episode::EPISODE_TOOL)
        .filter(|s| matches!(s.decision.as_str(), "begin" | "end"))
        .map(|s| (s.decision, s.result))
        .collect()
}

/// The goal step each gated step of the episode ran under.
fn goal_steps(cfg: &std::path::Path, id: &str) -> Vec<(String, String, String)> {
    spans(cfg, id)
        .into_iter()
        .filter(|s| matches!(s.decision.as_str(), "allow" | "park" | "approve" | "deny"))
        .filter(|s| {
            use grokhub_agent::harness::{RECOVERY_TOOL, REPLY_TOOL, VERIFY_TOOL};
            !matches!(
                s.tool.as_str(),
                grokhub_agent::episode::EPISODE_TOOL | VERIFY_TOOL | REPLY_TOOL | RECOVERY_TOOL
            )
        })
        .map(|s| (s.tool, s.decision, s.goal_step))
        .collect()
}

fn park_files(cfg: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(grokhub_agent::harness::park_dir(cfg))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
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

#[test]
fn allow_with_nothing_running_resumes_and_runs_the_step_once_with_the_goal_kept() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("allow-resume");
    let session = Session::spawn_with(CHAT, cfg.clone(), fixture);
    session.arm_episode(true);
    let model = Model::new(vec![
        Act::Click,
        Act::Send,
        Act::Done,
        Act::Click,
        Act::Done,
    ]);
    let first = session.prompt(
        GOAL,
        Vec::new(),
        route(Arc::clone(&model), PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(first, Ok("end_turn".to_string()));
    let open = session.episode().expect("it waits on the card");
    let card = panel_of(&cfg, &open).parks[0].id.clone();
    // Allow, with no reply running: the window is told to send the resume.
    assert_eq!(session.answer_episode(&card, true), Ok(true));
    assert!(!desk_calls().iter().any(|c| c == "click Send"));
    let second = session.prompt(
        RESUME_TEXT,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(second, Ok("episode_verified".to_string()));
    let sends = desk_calls().iter().filter(|c| *c == "click Send").count();
    assert_eq!(sends, 1, "{:?}", desk_calls());
    // "Continue" is not a goal: every step, the one after the resume too,
    // ran under the goal the person gave.
    let step =
        |tool: &str, decision: &str| (tool.to_string(), decision.to_string(), GOAL.to_string());
    assert_eq!(
        goal_steps(&cfg, &open.id),
        vec![
            step("click", "allow"),
            step("click", "park"),
            step("click", "approve"),
            step("click", "allow"),
        ]
    );
    assert_eq!(
        markers(&cfg, &open.id),
        vec![
            ("begin".to_string(), "open".to_string()),
            ("end".to_string(), "verified".to_string())
        ]
    );
}

#[test]
fn after_an_allow_a_message_that_is_not_the_resume_still_steers() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("allow-steer");
    let session = Session::spawn_with(CHAT, cfg.clone(), fixture);
    session.arm_episode(true);
    let model = Model::new(vec![Act::Send, Act::Done, Act::Click, Act::Done]);
    let first = session.prompt(
        GOAL,
        Vec::new(),
        route(Arc::clone(&model), PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(first, Ok("end_turn".to_string()));
    let open = session.episode().expect("it waits on the card");
    let card = panel_of(&cfg, &open).parks[0].id.clone();
    assert_eq!(session.answer_episode(&card, true), Ok(true));
    assert_eq!(
        session.answer_episode(&card, false),
        Err("That step already has an answer.".to_string())
    );
    // The person typed something else instead: it is a new goal step, not
    // swallowed as the resume.
    let second = session.prompt(
        "Open the Bluetooth page instead",
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(second, Ok("episode_verified".to_string()));
    let step =
        |decision: &str, goal: &str| ("click".to_string(), decision.to_string(), goal.to_string());
    assert_eq!(
        goal_steps(&cfg, &open.id),
        vec![
            step("park", GOAL),
            step("approve", GOAL),
            step("allow", "Open the Bluetooth page instead"),
        ]
    );
}

#[cfg(desktop)]
#[test]
fn allow_then_idle_halt_then_a_message_sends_nothing() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("allow-halt");
    let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
    session.arm_episode(true);
    let model = Model::new(vec![Act::Send, Act::Done, Act::Done]);
    let first = session.prompt(
        GOAL,
        Vec::new(),
        route(Arc::clone(&model), PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(first, Ok("end_turn".to_string()));
    let open = session.episode().expect("it waits on the card");
    let card = panel_of(&cfg, &open).parks[0].id.clone();
    assert_eq!(session.answer_episode(&card, true), Ok(true));
    assert_eq!(
        park_files(&cfg),
        vec![format!("{card}.answer"), format!("{card}.json")]
    );
    // Halt with no reply running: nobody was mid-reply, and the session is over.
    assert_eq!(halt(&cfg, [&session]), Vec::<String>::new());
    assert!(session.episode().is_none(), "Halt ends an idle session");
    assert!(!session.episode_armed());
    assert_eq!(park_files(&cfg), Vec::<String>::new());
    let ended = panel_of(&cfg, &open);
    assert_eq!(ended.ended.as_deref(), Some("halt"));
    assert_eq!(ended.parks, Vec::new());
    // The next message is asked after the stamp's millisecond.
    std::thread::sleep(std::time::Duration::from_millis(5));
    let next = session.prompt(
        "what did you do so far?",
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(next, Ok("end_turn".to_string()));
    assert!(
        !desk_calls().iter().any(|c| c.starts_with("click")),
        "the allowed Send ran after Halt: {:?}",
        desk_calls()
    );
    assert_eq!(
        markers(&cfg, &open.id),
        vec![
            ("begin".to_string(), "open".to_string()),
            ("end".to_string(), "halt".to_string())
        ]
    );
    assert!(spans(&cfg, &open.id)
        .iter()
        .all(|s| s.decision != "approve"));
}

#[cfg(desktop)]
#[test]
fn an_allow_after_halt_is_refused_and_a_deny_still_answers() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("halt-answer");
    let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
    assert_eq!(halt(&cfg, [&session]), Vec::<String>::new());
    // A session armed after the Halt still parks its hard step.
    std::thread::sleep(std::time::Duration::from_millis(5));
    session.arm_episode(true);
    let model = Model::new(vec![Act::Send, Act::Done]);
    let first = session.prompt(
        GOAL,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(first, Ok("end_turn".to_string()));
    let open = session.episode().expect("it waits on the card");
    let card = panel_of(&cfg, &open).parks[0].id.clone();
    assert_eq!(
        session.answer_episode(&card, true),
        Err(
            "Desktop control was halted (Ctrl+Alt+H). Open Loadout again to use the screen."
                .to_string()
        )
    );
    assert_eq!(park_files(&cfg), vec![format!("{card}.json")]);
    assert_eq!(session.answer_episode(&card, false), Ok(false));
}

#[test]
fn pause_then_continue_keeps_the_goal_and_the_cards() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("pause-continue");
    let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
    session.arm_episode(true);
    // The Send parks; Pause lands while the next step is being thought up.
    let pauser = Arc::clone(&session);
    let model = Arc::new(Model {
        script: vec![Act::Send, Act::Click, Act::Click, Act::Done, Act::Done],
        calls: AtomicUsize::new(0),
        on_call: Box::new(move |n| {
            if n == 1 {
                assert!(pauser.pause_episode());
            }
        }),
    });
    let first = session.prompt(
        GOAL,
        Vec::new(),
        route(Arc::clone(&model), PermMode::Always),
        Box::new(|_| {}),
    );
    // The page reads a paused turn as stopped, so nothing queued behind it sends.
    assert_eq!(first, Ok("cancelled".to_string()));
    let open = session.episode().expect("a paused episode stays open");
    assert!(open.paused);
    assert_eq!(
        desk_calls()
            .iter()
            .filter(|c| c.starts_with("click"))
            .count(),
        0
    );
    let paused = panel_of(&cfg, &open);
    assert_eq!(paused.ended, None);
    assert_eq!(paused.parks.len(), 1, "the card still waits: {paused:?}");
    assert_eq!(
        markers(&cfg, &open.id),
        vec![("begin".to_string(), "open".to_string())]
    );
    let second = session.prompt(
        RESUME_TEXT,
        Vec::new(),
        route(Arc::clone(&model), PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(
        second,
        Ok("end_turn".to_string()),
        "it waits on the card again"
    );
    assert_eq!(
        session.episode().map(|o| (o.id, o.paused)),
        Some((open.id.clone(), false))
    );
    assert_eq!(panel_of(&cfg, &open).parks.len(), 1);
    // Deny the card and go on: the check is asked about the goal the person gave.
    let card = panel_of(&cfg, &open).parks[0].id.clone();
    assert_eq!(session.answer_episode(&card, false), Ok(false));
    let third = session.prompt(
        "Skip the send and finish",
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(third, Ok("episode_verified".to_string()));
    let step =
        |decision: &str, goal: &str| ("click".to_string(), decision.to_string(), goal.to_string());
    assert_eq!(
        goal_steps(&cfg, &open.id),
        vec![step("park", GOAL), step("allow", GOAL), step("deny", GOAL),]
    );
    assert_eq!(
        markers(&cfg, &open.id),
        vec![
            ("begin".to_string(), "open".to_string()),
            ("end".to_string(), "verified".to_string())
        ]
    );
    let checks = judged();
    assert_eq!(checks.len(), 1);
    assert!(checks[0].contains(GOAL), "{}", checks[0]);
}

#[test]
fn a_card_names_the_control_or_the_typed_command() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("card-text");
    let session = Session::spawn_with(CHAT, cfg.clone(), fixture);
    session.arm_episode(true);
    let model = Model::new(vec![Act::BareSend, Act::TypeRm, Act::Done]);
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let said = Arc::clone(&seen);
    let stop = session.prompt(
        GOAL,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(move |event| {
            if let UiEvent::Text { text } = event {
                said.lock().expect("said").push(text);
            }
        }),
    );
    assert_eq!(stop, Ok("end_turn".to_string()));
    let open = session.episode().expect("it waits on the cards");
    let cards: Vec<(String, String, String)> = panel_of(&cfg, &open)
        .parks
        .into_iter()
        .map(|p| (p.tool, p.class, p.action))
        .collect();
    assert_eq!(
        cards,
        vec![
            (
                "click".to_string(),
                "send".to_string(),
                "Loadout wants to click Send in Mail".to_string()
            ),
            (
                "type".to_string(),
                "delete".to_string(),
                "rm -rf ~/Documents".to_string()
            ),
        ]
    );
    assert_eq!(
        seen.lock().expect("said").concat(),
        "All set.\n\nWaiting on you: \u{201c}Loadout wants to click Send in Mail\u{201d}; \u{201c}rm -rf ~/Documents\u{201d}. Approve their cards and I'll run it once and go on."
    );
    assert!(
        !desk_calls()
            .iter()
            .any(|c| c.starts_with("click") || c.starts_with("type")),
        "{:?}",
        desk_calls()
    );
}

#[test]
fn switching_desktop_control_off_mid_session_stops_it() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("switch-off");
    let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
    session.arm_episode(true);
    // Settings turns the switch off while the second step is being thought up.
    let switch = Arc::clone(&session);
    let model = Arc::new(Model {
        script: vec![Act::Click, Act::Click, Act::Click, Act::Done],
        calls: AtomicUsize::new(0),
        on_call: Box::new(move |n| {
            if n == 1 {
                assert!(switch.desktop_off(), "a session was open");
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
    let clicks = desk_calls()
        .iter()
        .filter(|c| c.starts_with("click"))
        .count();
    assert_eq!(clicks, 1, "{:?}", desk_calls());
    assert!(session.episode().is_none());
    assert!(
        !session.episode_armed(),
        "it does not come back with the switch"
    );
}

#[test]
fn the_switch_going_off_ends_an_idle_session_and_disarms_an_armed_chat() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("switch-off-idle");
    let session = Session::spawn_with(CHAT, cfg.clone(), fixture);
    session.arm_episode(true);
    assert!(!session.desktop_off(), "armed, but nothing was open");
    assert!(!session.episode_armed());
    session.arm_episode(true);
    let model = Model::new(vec![Act::Send, Act::Done]);
    let first = session.prompt(
        GOAL,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(first, Ok("end_turn".to_string()));
    let open = session.episode().expect("it waits on the card");
    assert!(session.desktop_off());
    assert!(session.episode().is_none());
    assert!(!session.episode_armed());
    assert_eq!(park_files(&cfg), Vec::<String>::new());
    assert_eq!(panel_of(&cfg, &open).ended.as_deref(), Some("stop"));
}

#[test]
fn a_chat_on_a_desktop_turn_or_session_says_so() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("on-desktop");
    let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
    assert!(!session.on_desktop());
    // A plain turn with desktop control on: on the desktop while it runs.
    let (probe, during) = (Arc::clone(&session), Arc::new(Mutex::new(Vec::new())));
    let saw = Arc::clone(&during);
    let model = Arc::new(Model {
        script: vec![Act::Done],
        calls: AtomicUsize::new(0),
        on_call: Box::new(move |_| saw.lock().expect("saw").push(probe.on_desktop())),
    });
    let plain = session.prompt(
        "hello",
        Vec::new(),
        route(Arc::clone(&model), PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(plain, Ok("end_turn".to_string()));
    assert_eq!(*during.lock().expect("saw"), vec![true]);
    assert!(!session.on_desktop(), "over with the turn");
    // The same turn with the switch off never is.
    let mut off = route(model, PermMode::Always);
    off.desktop = false;
    let plain = session.prompt("hello again", Vec::new(), off, Box::new(|_| {}));
    assert_eq!(plain, Ok("end_turn".to_string()));
    assert_eq!(*during.lock().expect("saw"), vec![true, false]);
    // Armed, and then open on a card: on the desktop between messages too.
    session.arm_episode(true);
    assert!(session.on_desktop());
    let waits = session.prompt(
        GOAL,
        Vec::new(),
        route(Model::new(vec![Act::Send, Act::Done]), PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(waits, Ok("end_turn".to_string()));
    assert!(session.episode().is_some());
    assert!(session.on_desktop());
    assert!(session.stop_episode());
    assert!(!session.on_desktop());
}

#[test]
fn an_allow_during_the_last_model_call_runs_the_step_in_that_turn() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("allow-mid-turn");
    let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
    session.arm_episode(true);
    // The card is on screen while the reply is still being written: the
    // person clicks Allow during the call that only says it is done.
    let (clicker, dir) = (Arc::clone(&session), cfg.clone());
    let told: Arc<Mutex<Vec<Result<bool, String>>>> = Arc::default();
    let heard = Arc::clone(&told);
    let model = Arc::new(Model {
        script: vec![Act::Send, Act::Done, Act::Done],
        calls: AtomicUsize::new(0),
        on_call: Box::new(move |n| {
            if n == 1 {
                let open = clicker.episode().expect("open");
                let card = panel_of(&dir, &open).parks[0].id.clone();
                heard
                    .lock()
                    .expect("told")
                    .push(clicker.answer_episode(&card, true));
            }
        }),
    });
    let stop = session.prompt(
        GOAL,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    // A reply was running, so the window is not asked to send anything.
    assert_eq!(*told.lock().expect("told"), vec![Ok(false)]);
    assert_eq!(stop, Ok("episode_verified".to_string()));
    assert!(!session.take_resume_owed(), "the loop took the answer");
    let sends = desk_calls().iter().filter(|c| *c == "click Send").count();
    assert_eq!(sends, 1, "{:?}", desk_calls());
    assert!(session.episode().is_none());
    assert_eq!(park_files(&cfg), Vec::<String>::new());
}

#[test]
fn an_allow_after_the_loops_last_look_is_owed_a_resume_when_the_reply_ends() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("allow-late");
    let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
    session.arm_episode(true);
    let model = Model::new(vec![Act::Send, Act::Done, Act::Click, Act::Done]);
    // The Allow lands when the loop has returned and the reply is being
    // wrapped up: the chat is still busy, and no loop will look again.
    let (clicker, dir) = (Arc::clone(&session), cfg.clone());
    let told: Arc<Mutex<Vec<Result<bool, String>>>> = Arc::default();
    let heard = Arc::clone(&told);
    let first = session.prompt(
        GOAL,
        Vec::new(),
        route(Arc::clone(&model), PermMode::Always),
        Box::new(move |event| {
            if let UiEvent::Done { .. } = event {
                let open = clicker.episode().expect("open");
                let card = panel_of(&dir, &open).parks[0].id.clone();
                heard
                    .lock()
                    .expect("told")
                    .push(clicker.answer_episode(&card, true));
            }
        }),
    );
    assert_eq!(first, Ok("end_turn".to_string()));
    assert_eq!(*told.lock().expect("told"), vec![Ok(false)]);
    assert!(!desk_calls().iter().any(|c| c == "click Send"));
    let open = session.episode().expect("it waits on the resume");
    // The end of the reply owes the window the resume, once.
    assert!(session.take_resume_owed());
    assert!(!session.take_resume_owed());
    assert!(crate::episode::resume_panel(&cfg, CHAT, &open, None).resume);
    assert!(!panel_of(&cfg, &open).resume);
    // The window sends the resume: the step runs once and the goal is kept.
    let second = session.prompt(
        RESUME_TEXT,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(second, Ok("episode_verified".to_string()));
    let step = |decision: &str| ("click".to_string(), decision.to_string(), GOAL.to_string());
    assert_eq!(
        goal_steps(&cfg, &open.id),
        vec![step("park"), step("approve"), step("allow")]
    );
}

#[test]
fn a_stop_from_the_composer_after_a_pause_ends_the_session() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    // Pause then the reply's own Stop, and the other way round: the engine
    // ends the session either way, so the shell must not keep it as paused.
    for (label, pause_first) in [("pause-then-stop", true), ("stop-then-pause", false)] {
        reset();
        let cfg = config(label);
        let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
        session.arm_episode(true);
        let presser = Arc::clone(&session);
        let model = Arc::new(Model {
            script: vec![Act::Click, Act::Click, Act::Click],
            calls: AtomicUsize::new(0),
            on_call: Box::new(move |n| {
                if n == 1 {
                    if pause_first {
                        assert!(presser.pause_episode());
                        presser.cancel();
                    } else {
                        presser.cancel();
                        assert!(presser.pause_episode());
                    }
                }
            }),
        });
        let stop = session.prompt(
            GOAL,
            Vec::new(),
            route(model, PermMode::Always),
            Box::new(|_| {}),
        );
        assert_eq!(stop, Ok("cancelled".to_string()), "{label}");
        let all = grokhub_agent::harness::read_spans(&cfg, CHAT).expect("spans");
        let end = all.last().expect("a span");
        assert_eq!(
            (
                end.tool.as_str(),
                end.decision.as_str(),
                end.result.as_str()
            ),
            ("episode", "end", "stop"),
            "{label}"
        );
        assert!(
            session.episode().is_none(),
            "{label}: the shell kept a session the engine ended"
        );
        assert!(!session.episode_armed(), "{label}");
        assert!(!session.on_desktop(), "{label}");
        assert!(
            !session.resume_due.load(Ordering::SeqCst),
            "{label}: a later Continue would be read as a resume"
        );
    }
}

#[test]
fn a_paused_session_with_an_allowed_step_waits_for_the_person() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let cfg = config("allow-paused");
    let session = Arc::new(Session::spawn_with(CHAT, cfg.clone(), fixture));
    session.arm_episode(true);
    let pauser = Arc::clone(&session);
    let model = Arc::new(Model {
        script: vec![Act::Send, Act::Click, Act::Done, Act::Done],
        calls: AtomicUsize::new(0),
        on_call: Box::new(move |n| {
            if n == 1 {
                assert!(pauser.pause_episode());
            }
        }),
    });
    // The Allow lands as the paused turn is wrapped up.
    let (clicker, dir) = (Arc::clone(&session), cfg.clone());
    let first = session.prompt(
        GOAL,
        Vec::new(),
        route(Arc::clone(&model), PermMode::Always),
        Box::new(move |event| {
            if let UiEvent::Done { .. } = event {
                let open = clicker.episode().expect("open");
                let card = panel_of(&dir, &open).parks[0].id.clone();
                assert_eq!(clicker.answer_episode(&card, true), Ok(false));
            }
        }),
    );
    assert_eq!(first, Ok("cancelled".to_string()));
    assert_eq!(session.episode().map(|o| o.paused), Some(true));
    // Pause was the person's word: nothing is sent for them.
    assert!(!session.take_resume_owed());
    assert!(!desk_calls().iter().any(|c| c == "click Send"));
    // Their own Continue runs the allowed step once.
    let second = session.prompt(
        RESUME_TEXT,
        Vec::new(),
        route(model, PermMode::Always),
        Box::new(|_| {}),
    );
    assert_eq!(second, Ok("episode_verified".to_string()));
    let sends = desk_calls().iter().filter(|c| *c == "click Send").count();
    assert_eq!(sends, 1, "{:?}", desk_calls());
}
