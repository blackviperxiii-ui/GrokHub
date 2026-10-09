//! Spike-3b acceptance: a fake model (worker, judge and fold) and a fake
//! desktop backend drive the real kernel, gate and span store.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::*;
use crate::client::{
    responses_body, ClientError, ContentPart, FunctionCall, InputItem, ModelClient, ResponsesRequest, StreamEvent,
    TurnOutput, Usage,
};
use crate::gate::{ClosedPermits, Gate, PermMode};
use crate::harness::{done_without_criteria, read_spans, ParkRequest, DONE_WITHOUT_CRITERIA, VERIFY_TOOL};
use crate::run::{HaltCheck, SteerQueue};
use crate::tools::{DesktopOps, ToolOutput};
use crate::CancelToken;

const CHAT: &str = "chat-ep";
const WORKER_THOUGHT: &str = "WORKER-SECRET-REASONING-7f3";
const TYPED: &str = "hunter2-typed-value-91";

/// What the worker does on its `n`th call (0-based).
#[derive(Clone)]
enum Act {
    Click(u32),
    HardClick,
    Type,
    Zoom(&'static str),
    Reads(usize),
    Done,
    /// Done, with a `VERIFY_OK` of its own: no detector finding on a reject.
    DoneSelfChecked,
    Say,
    Shell(&'static str),
}

type Script = Box<dyn Fn(usize) -> Act + Send + Sync>;

struct FakeModel {
    script: Script,
    worker_calls: Mutex<Vec<ResponsesRequest>>,
    judge_calls: Mutex<Vec<ResponsesRequest>>,
    fold_calls: Mutex<Vec<ResponsesRequest>>,
    judge_says: &'static str,
    /// The first this-many checker calls fail to run.
    judge_errors: usize,
    /// From this checker call (0-based) on, it says this instead.
    judge_later: Option<(usize, &'static str)>,
    clock: Arc<AtomicU64>,
    tick_ms: u64,
    steer: Option<(usize, SteerQueue)>,
    answer_at: Option<(usize, Arc<FakeParks>, bool)>,
    /// Stop pressed while this worker call is in flight.
    cancel_at: Option<(usize, CancelToken)>,
}

fn first_text(req: &ResponsesRequest, at: usize) -> String {
    match req.input.get(at) {
        Some(InputItem::Message { content, .. }) => match content.first() {
            Some(ContentPart::InputText(t)) => t.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn call(n: usize, name: &str, args: Value) -> FunctionCall {
    FunctionCall { call_id: format!("c{n}-{name}"), name: name.into(), arguments: args.to_string() }
}

impl ModelClient for FakeModel {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let system = first_text(req, 0);
        let usage = Usage { input_tokens: 1_000, output_tokens: 20, reasoning_tokens: 5, cost_in_usd_ticks: 10, cached_tokens: 0 };
        if system == JUDGE_SYSTEM {
            let n = {
                let mut calls = self.judge_calls.lock().unwrap();
                calls.push(req.clone());
                calls.len() - 1
            };
            if n < self.judge_errors {
                return Err(ClientError::Protocol("checker offline".into()));
            }
            let says = match self.judge_later {
                Some((from, later)) if n >= from => later,
                _ => self.judge_says,
            };
            return Ok(TurnOutput { text: says.into(), reasoning: String::new(), calls: vec![], usage });
        }
        if system == FOLD_SYSTEM {
            self.fold_calls.lock().unwrap().push(req.clone());
            let body = first_text(req, 1);
            let text: String = body.replace('\n', " ").chars().take(80).collect();
            return Ok(TurnOutput { text: format!("fold: {text}"), reasoning: String::new(), calls: vec![], usage });
        }
        let n = {
            let mut calls = self.worker_calls.lock().unwrap();
            calls.push(req.clone());
            calls.len() - 1
        };
        self.clock.fetch_add(self.tick_ms, Ordering::SeqCst);
        if let Some((at, q)) = &self.steer {
            if *at == n {
                q.push("now open the Wi-Fi page");
            }
        }
        if let Some((at, parks, approve)) = &self.answer_at {
            if *at == n {
                parks.answer_all(*approve);
            }
        }
        if let Some((at, token)) = &self.cancel_at {
            if *at == n {
                token.cancel();
                return Err(ClientError::Cancelled);
            }
        }
        // A cache that holds the prefix: everything but the last message.
        let cached = if n == 0 { 0 } else { 800 };
        let usage = Usage { cached_tokens: cached, ..usage };
        let (text, calls) = match (self.script)(n) {
            Act::Click(x) => (String::new(), vec![call(n, "click", json!({"x": x, "y": 40}))]),
            Act::HardClick => (String::new(), vec![call(n, "click", json!({"x": 900, "y": 40, "label": "Send"}))]),
            Act::Type => (String::new(), vec![call(n, "type", json!({"text": TYPED}))]),
            Act::Zoom(id) => (String::new(), vec![call(n, "zoom", json!({"span_ref": id, "n": 1}))]),
            Act::Reads(k) => (
                String::new(),
                (0..k).map(|i| call(n * 100 + i, "read_file", json!({"path": format!("missing-{i}.txt")}))).collect(),
            ),
            Act::Done => ("All set.\nGOAL_COMPLETE".into(), vec![]),
            Act::DoneSelfChecked => ("Saved it.\nVERIFY_OK\nGOAL_COMPLETE".into(), vec![]),
            Act::Say => ("Which network should I pick?".into(), vec![]),
            Act::Shell(cmd) => (String::new(), vec![call(n, "run_terminal_command", json!({"command": cmd}))]),
        };
        Ok(TurnOutput { text, reasoning: WORKER_THOUGHT.into(), calls, usage })
    }
}

#[derive(Default)]
struct FakeDesk {
    calls: Mutex<Vec<(String, Value)>>,
    halt: Arc<AtomicBool>,
    /// Halt once this many clicks ran.
    halt_after_clicks: Option<usize>,
    calls_at_halt: Mutex<Option<usize>>,
    /// Every screenshot is the same frame: no step changes the screen.
    still: bool,
}

impl FakeDesk {
    fn count(&self, name: &str) -> usize {
        self.calls.lock().unwrap().iter().filter(|(n, _)| n == name).count()
    }

    fn total(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl DesktopOps for FakeDesk {
    fn halted(&self) -> bool {
        self.halt.load(Ordering::SeqCst)
    }

    fn locked(&self) -> bool {
        false
    }

    fn call(&self, name: &str, args: &Value) -> ToolOutput {
        let mut calls = self.calls.lock().unwrap();
        calls.push((name.into(), args.clone()));
        let n = calls.len();
        let clicks = calls.iter().filter(|(c, _)| c == "click").count();
        drop(calls);
        if name == "click" && Some(clicks) == self.halt_after_clicks {
            self.halt.store(true, Ordering::SeqCst);
            *self.calls_at_halt.lock().unwrap() = Some(n);
        }
        if name == "screenshot" {
            return ToolOutput {
                text: "windows: Settings, Terminal".into(),
                image_data_url: Some(if self.still { "data:image/png;base64,FRAME".into() } else { format!("data:image/png;base64,FRAME{n}") }),
                failed: false,
            };
        }
        ToolOutput::ok(format!("{name} ok"))
    }
}

struct Halt(Arc<AtomicBool>);

impl HaltCheck for Halt {
    fn halted(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct FakeParks {
    posted: Mutex<Vec<ParkRequest>>,
    answers: Mutex<HashMap<String, bool>>,
    withdrawn: Mutex<Vec<String>>,
}

impl FakeParks {
    fn answer_all(&self, approve: bool) {
        let ids: Vec<String> = self.posted.lock().unwrap().iter().map(|p| p.id.clone()).collect();
        let mut a = self.answers.lock().unwrap();
        for id in ids {
            a.insert(id, approve);
        }
    }
}

impl Parks for FakeParks {
    fn post(&self, req: &ParkRequest) -> bool {
        self.posted.lock().unwrap().push(req.clone());
        true
    }

    fn answer(&self, id: &str) -> Option<bool> {
        self.answers.lock().unwrap().remove(id)
    }

    fn withdraw(&self, id: &str) {
        self.withdrawn.lock().unwrap().push(id.into());
    }
}

impl Parks for Arc<FakeParks> {
    fn post(&self, req: &ParkRequest) -> bool {
        self.as_ref().post(req)
    }

    fn answer(&self, id: &str) -> Option<bool> {
        self.as_ref().answer(id)
    }

    fn withdraw(&self, id: &str) {
        self.as_ref().withdraw(id)
    }
}

struct Rig {
    dir: PathBuf,
    model: FakeModel,
    desk: FakeDesk,
    parks: Arc<FakeParks>,
    steer: SteerQueue,
    clock: Arc<AtomicU64>,
    cancel: CancelToken,
    perms: Option<crate::perm::Policy>,
}

const T0: u64 = 1_800_000_000_000;

fn rig(label: &str, script: Script) -> Rig {
    let clock = Arc::new(AtomicU64::new(T0));
    let dir = crate::harness::test_dir(&format!("episode-{label}"));
    let desk = FakeDesk::default();
    Rig {
        dir,
        model: FakeModel {
            script,
            worker_calls: Mutex::new(vec![]),
            judge_calls: Mutex::new(vec![]),
            fold_calls: Mutex::new(vec![]),
            judge_says: "VERIFY_OK",
            judge_errors: 0,
            judge_later: None,
            clock: Arc::clone(&clock),
            tick_ms: 1_000,
            steer: None,
            answer_at: None,
            cancel_at: None,
        },
        desk,
        parks: Arc::new(FakeParks::default()),
        steer: SteerQueue::new(),
        clock,
        cancel: CancelToken::new(),
        perms: None,
    }
}

impl Rig {
    fn run(&self, ep: &mut Episode, view: &mut EpisodeView) -> EpisodeOut {
        let halt = Halt(Arc::clone(&self.desk.halt));
        let clock = Arc::clone(&self.clock);
        let now = move || clock.load(Ordering::SeqCst);
        let k = KernelIn {
            client: &self.model,
            provider: crate::route::PROVIDER_XAI,
            model: "grok-4.7",
            effort: None,
            system: "cabin rules",
            workspace: &self.dir,
            config_dir: &self.dir,
            gate: Gate { mode: PermMode::Always, readonly_session: false, attended: true, desktop: true },
            access: crate::harness::AccessMode::Supervised,
            desktop: &self.desk,
            parks: &self.parks,
            permits: &ClosedPermits,
            halt: &halt,
            cancel: &self.cancel,
            steer: &self.steer,
            clock: &now,
            held: &[],
            perms: self.perms.as_ref(),
        };
        run_episode(&k, ep, view, &mut |_| {})
    }

    fn episode(&self, goal: &str) -> Episode {
        Episode::begin(&new_episode_id(T0), CHAT, goal, T0, &[])
    }

    fn spans(&self) -> Vec<crate::harness::Span> {
        read_spans(&self.dir, CHAT).unwrap()
    }
}

fn clicks_then_done(steps: usize) -> Script {
    Box::new(move |n| if n < steps { Act::Click(n as u32) } else { Act::Done })
}

fn is_step(s: &crate::harness::Span) -> bool {
    s.claim.starts_with("episode step")
}

#[test]
fn a_25_step_episode_is_one_trace_with_one_shape_per_step() {
    let r = rig("25", clicks_then_done(25));
    let mut ep = r.episode("Turn on Wi-Fi in Settings");
    let mut view = EpisodeView::default();
    let out = r.run(&mut ep, &mut view);
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    let spans = r.spans();
    assert!(spans.len() > 25);
    assert!(spans.iter().all(|s| s.episode == ep.id), "every span carries the episode id");
    let steps: Vec<_> = spans.iter().filter(|s| is_step(s)).collect();
    assert_eq!(steps.len(), 25);
    for s in &steps {
        assert_eq!((s.tool.as_str(), s.decision.as_str(), s.path.as_str()), ("click", "allow", "E"));
        assert_eq!(s.goal_step, "Turn on Wi-Fi in Settings");
        assert_eq!(s.ui_changed, Some(true));
        assert_eq!(s.result, "click ok");
        let t = s.tokens.as_ref().expect("the step's model call is logged");
        assert_eq!((t.class.as_str(), t.input, t.out, t.reasoning, t.cost_ticks), ("episode:step", 1_000, 20, 5, 10));
    }
    assert_eq!(steps[0].tokens.as_ref().unwrap().cached, 0);
    assert_eq!(steps[1].tokens.as_ref().unwrap().cached, 800, "cached tokens are logged per step");
    let check = spans.iter().find(|s| s.tool == VERIFY_TOOL).unwrap();
    assert_eq!((check.result.as_str(), check.claim.as_str()), ("pass", "VERIFY_OK"));
    assert_eq!(check.tokens.as_ref().unwrap().class, "background:judge");
    assert_eq!(check.tokens.as_ref().unwrap().effort, "low");
    let end = spans.last().unwrap();
    assert_eq!((end.tool.as_str(), end.decision.as_str(), end.result.as_str()), (EPISODE_TOOL, "end", "verified"));
    assert_eq!(end.claim, "Desktop session · 25 steps · 0 min");
    assert_eq!(r.desk.count("click"), 25);
    assert_eq!(ep.ended, Some(EpisodeEnd::Verified));
}

#[test]
fn a_hard_step_parks_and_ttl_denies_it_while_the_episode_goes_on() {
    let mut r = rig("hard", Box::new(|n| match n {
        7 => Act::HardClick,
        n if n < 20 => Act::Click(n as u32),
        _ => Act::Done,
    }));
    r.model.tick_ms = 30_000;
    let mut ep = r.episode("Reply to the thread");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    let spans = r.spans();
    let park = spans.iter().find(|s| s.decision == "park").unwrap();
    assert_eq!((park.tool.as_str(), park.approval_class.as_str(), park.claim.as_str()), ("click", "send", "episode step 8"));
    let deny = spans.iter().find(|s| s.decision == "deny").unwrap();
    assert_eq!(deny.result, "hard-class send timed out (300s) — fail-closed Deny");
    assert_eq!(deny.episode, ep.id);
    assert_eq!(r.parks.posted.lock().unwrap().len(), 1);
    assert_eq!(r.parks.posted.lock().unwrap()[0].id, format!("{PARK_PREFIX}{}-8", ep.id));
    assert_eq!(*r.parks.withdrawn.lock().unwrap(), vec![format!("{PARK_PREFIX}{}-8", ep.id)]);
    // The parked Send never reached the desktop; the other 19 clicks did.
    assert_eq!(r.desk.count("click"), 19);
    assert!(r.desk.calls.lock().unwrap().iter().all(|(_, a)| a.get("label").is_none()));
    assert!(ep.parks.is_empty());
}

#[test]
fn halt_at_step_10_denies_every_park_and_the_desktop_gets_nothing_more() {
    let mut r = rig("halt", Box::new(|n| match n {
        2 | 6 => Act::HardClick,
        n => Act::Click(n as u32),
    }));
    // Steps 1-10 with 3 and 7 parked: the 8th click is step 10.
    r.desk.halt_after_clicks = Some(8);
    let mut ep = r.episode("Clean up the downloads");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Halt));
    assert_eq!(ep.steps, 10);
    let at_halt = r.desk.calls_at_halt.lock().unwrap().expect("halted");
    assert_eq!(r.desk.total(), at_halt, "the fake backend got 0 calls after Halt");
    let spans = r.spans();
    let denies: Vec<_> = spans.iter().filter(|s| s.decision == "deny").collect();
    assert_eq!(denies.len(), 2, "both parks denied with a span");
    for d in &denies {
        assert_eq!((d.tool.as_str(), d.result.as_str()), ("click", "halt — fail-closed Deny"));
        assert_eq!(d.episode, ep.id);
    }
    assert_eq!(r.parks.withdrawn.lock().unwrap().len(), 2);
    assert!(ep.parks.is_empty());
    let end = spans.last().unwrap();
    assert_eq!((end.decision.as_str(), end.result.as_str()), ("end", "halt"));
    // A halted episode stays ended.
    r.desk.halt.store(false, Ordering::SeqCst);
    assert_eq!(r.run(&mut ep, &mut EpisodeView::default()).stop, EpisodeStop::Ended(EpisodeEnd::Halt));
    assert_eq!(r.desk.total(), at_halt);
}

#[test]
fn steer_at_step_5_keeps_the_episode_id_and_the_parks() {
    let mut r = rig("steer", Box::new(|n| match n {
        2 => Act::HardClick,
        n if n < 9 => Act::Click(n as u32),
        _ => Act::Done,
    }));
    r.model.steer = Some((4, r.steer.clone()));
    r.model.answer_at = Some((7, Arc::clone(&r.parks), true));
    let mut ep = r.episode("Turn on Wi-Fi in Settings");
    let id = ep.id.clone();
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    assert_eq!(ep.id, id);
    let spans = r.spans();
    assert!(spans.iter().all(|s| s.episode == id));
    let goals: Vec<&str> = spans.iter().filter(|s| is_step(s)).map(|s| s.goal_step.as_str()).collect();
    assert_eq!(goals[4], "Turn on Wi-Fi in Settings", "step 5 was proposed before the Steer landed");
    assert_eq!(goals[5], "now open the Wi-Fi page");
    // The park from step 3 outlived the Steer and was approved later, once.
    assert!(!spans.iter().any(|s| s.decision == "deny"));
    let ran = spans.iter().find(|s| s.decision == "approve").unwrap();
    assert!(ran.hard_approved);
    assert_eq!((ran.tool.as_str(), ran.goal_step.as_str()), ("click", "Turn on Wi-Fi in Settings"));
    let sends: Vec<_> = r.desk.calls.lock().unwrap().iter().filter(|(_, a)| a.get("label").is_some()).cloned().collect();
    assert_eq!(sends.len(), 1);
}

/// The text message (input 2) of every worker call: goal step, note, observation.
fn nows(m: &FakeModel) -> Vec<String> {
    m.worker_calls.lock().unwrap().iter().map(|r| first_text(r, 2)).collect()
}

#[test]
fn a_100_step_episode_runs_to_verified_with_no_pause() {
    let r = rig("hundred", clicks_then_done(100));
    let mut ep = r.episode("Sort the photos");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    assert_eq!(r.desk.count("click"), 100);
    assert_eq!(ep.steps, 100);
    let spans = r.spans();
    assert_eq!(spans.iter().filter(|s| is_step(s)).count(), 100);
    assert!(!spans.iter().any(|s| s.decision == "pause"), "no pause span");
    assert!(!spans.iter().any(|s| s.tool == crate::harness::RECOVERY_TOOL), "every click moved: no re-plan");
    assert!(r.parks.posted.lock().unwrap().is_empty(), "no card");
    assert_eq!(spans.last().unwrap().claim, "Desktop session · 100 steps · 1 min");
}

#[test]
fn an_episode_past_30_minutes_keeps_going() {
    let mut r = rig("long", clicks_then_done(10));
    r.model.tick_ms = 5 * 60_000;
    let mut ep = r.episode("Sort the photos");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    assert_eq!(r.desk.count("click"), 10);
    // Eleven worker calls five minutes apart: 55 minutes.
    assert_eq!(r.spans().last().unwrap().claim, "Desktop session · 10 steps · 55 min");
}

#[test]
fn twenty_clicks_that_change_nothing_replan_twice_quietly_and_verify() {
    let mut r = rig("stall", clicks_then_done(20));
    r.desk.still = true;
    let mut ep = r.episode("Turn on Wi-Fi in Settings");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    assert_eq!(r.desk.count("click"), 20);
    let spans = r.spans();
    assert!(spans.iter().filter(|s| is_step(s)).all(|s| s.ui_changed == Some(false)));
    let replans: Vec<_> = spans.iter().filter(|s| s.tool == crate::harness::RECOVERY_TOOL).collect();
    assert_eq!(replans.len(), 2);
    for s in &replans {
        assert_eq!((s.decision.as_str(), s.result.as_str(), s.approval_class.as_str()), ("replan", "replan", "soft"));
        assert!(s.args_redacted.contains(r#""detector":"no_progress""#), "{}", s.args_redacted);
        assert_eq!(s.claim, "no_progress: 8 steps in a row changed nothing");
    }
    assert!(!spans.iter().any(|s| s.decision == "pause"));
    assert!(r.parks.posted.lock().unwrap().is_empty(), "no card");
    let told: Vec<usize> = nows(&r.model)
        .iter()
        .enumerate()
        .filter(|(_, t)| t.contains(&format!("GrokHub's check: {REPLAN_NOTE}")))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(told, vec![8, 16], "the worker is told to re-plan after the 8th and 16th still click");
    assert_eq!(ep.stall, 4);
}

#[test]
fn a_hard_park_approved_after_the_turn_ended_runs_once_on_the_resume_prompt() {
    let r = rig("resume", Box::new(|n| match n {
        0 => Act::HardClick,
        1 => Act::Say,
        _ => Act::Done,
    }));
    let mut ep = r.episode("Reply to the thread");
    let mut view = EpisodeView::default();
    assert_eq!(r.run(&mut ep, &mut view).stop, EpisodeStop::Waiting);
    assert_eq!(r.desk.count("click"), 0, "parked, not run");
    assert_eq!(ep.parks.len(), 1);
    // The user approves on the card after the turn ended, then the cabin
    // sends the resume prompt: no Steer, same goal step.
    r.parks.answer_all(true);
    let out = r.run(&mut ep, &mut view);
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    let sends: Vec<_> = r.desk.calls.lock().unwrap().iter().filter(|(_, a)| a.get("label").is_some()).cloned().collect();
    assert_eq!(sends.len(), 1, "the approved Send ran once");
    let spans = r.spans();
    let ran: Vec<_> = spans.iter().filter(|s| s.decision == "approve").collect();
    assert_eq!(ran.len(), 1);
    assert_eq!((ran[0].tool.as_str(), ran[0].goal_step.as_str()), ("click", "Reply to the thread"));
    assert!(ran[0].hard_approved);
    let last = nows(&r.model).pop().unwrap();
    assert!(last.starts_with("Step 2 of this episode.\nGoal step: Reply to the thread\n"), "{last}");
    assert!(ep.parks.is_empty());
}

#[test]
fn verify_gets_only_the_goal_and_the_observation_and_a_reject_stays_flagged() {
    let mut r = rig("verify", Box::new(|n| match n {
        0 => Act::Click(5),
        1 => Act::Done,
        _ => Act::Say,
    }));
    r.model.judge_says = "REJECT: the Wi-Fi toggle is still off";
    let mut ep = r.episode("Turn on Wi-Fi in Settings");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Waiting);
    let judge = r.model.judge_calls.lock().unwrap();
    assert_eq!(judge.len(), 1);
    let body = serde_json::to_string(&responses_body(&judge[0])).unwrap();
    assert!(body.contains("Goal:\\nTurn on Wi-Fi in Settings"), "{body}");
    assert!(body.contains("Final observation:\\nwindows: Settings, Terminal\\nscreenshot sha256: "), "{body}");
    assert!(!body.contains(WORKER_THOUGHT), "{body}");
    assert!(!body.contains("All set"), "the worker's own words stay out: {body}");
    assert!(!body.contains("Episode so far"), "the view stays out: {body}");
    assert!(!body.contains("FRAME"), "the screenshot image stays out: {body}");
    assert!(!body.contains("\"tools\":[{"), "{body}");
    assert_eq!(judge[0].effort.as_deref(), Some("low"));
    let spans = r.spans();
    let check = spans.iter().find(|s| s.tool == VERIFY_TOOL).unwrap();
    assert_eq!((check.result.as_str(), check.claim.as_str()), ("fail", "the Wi-Fi toggle is still off"));
    let found = done_without_criteria(&spans);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].detector, DONE_WITHOUT_CRITERIA);
    assert!(found[0].evidence[0].quote.contains("GOAL_COMPLETE"), "{:?}", found[0].evidence);
    // The reject went to the 1a ladder: one retry with the checker's reason.
    let rung = spans.iter().find(|s| s.tool == crate::harness::RECOVERY_TOOL).unwrap();
    assert_eq!(rung.decision, "retry");
    let retry = r.model.worker_calls.lock().unwrap()[2].clone();
    assert!(first_text(&retry, 2).contains("The independent check said: the Wi-Fi toggle is still off"));
    assert_eq!(ep.ended, None);
}

/// The view message (input 1) of every worker call.
fn views(m: &FakeModel) -> Vec<String> {
    m.worker_calls.lock().unwrap().iter().map(|r| first_text(r, 1)).collect()
}

#[test]
fn sixty_steps_stay_under_the_budget_and_the_prefix_holds_between_folds() {
    let budget = 3_000;
    let r = rig("budget", clicks_then_done(60));
    let mut ep = r.episode("Sort the photos");
    let mut view = EpisodeView::new(budget);
    assert_eq!(r.run(&mut ep, &mut view).stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    let views = views(&r.model);
    assert_eq!(views.len(), 61);
    for v in &views {
        assert!(v.len() <= budget, "{} bytes over the {budget}-byte budget", v.len());
    }
    let folds = r.spans().iter().filter(|s| s.decision == "fold").count();
    assert!(folds > 10, "a 3,000-byte view must fold: {folds}");
    let stable = views.windows(2).filter(|w| w[1].starts_with(&w[0])).count();
    assert!(stable >= 60 - folds, "{stable} stable of 60 with {folds} folds");
    // Byte-identical wire prefix (system + view) between two steps with no fold.
    let calls = r.model.worker_calls.lock().unwrap();
    let wire = |req: &ResponsesRequest| serde_json::to_string(&crate::client::input_wire_value(&req.input[..2])).unwrap();
    let (a, b) = (wire(&calls[1]), wire(&calls[2]));
    // Everything up to the end of the older view's text is byte-identical.
    let cut = a.rfind(r#"","type":"input_text"}],"role":"user""#).unwrap();
    assert_eq!(a[..cut], b[..cut]);
    for req in calls.iter() {
        assert_eq!(first_text(req, 0), calls[0].input.first().map(|_| first_text(&calls[0], 0)).unwrap());
    }
    let fold = r.model.fold_calls.lock().unwrap();
    assert!(!fold.is_empty());
    assert_eq!(fold[0].effort.as_deref(), Some("low"));
    let spans = r.spans();
    let f = spans.iter().find(|s| s.decision == "fold").unwrap();
    assert_eq!(f.tokens.as_ref().unwrap().class, "background:compact");
}

#[test]
fn the_default_budget_never_folds_a_short_episode_and_the_prefix_is_identical() {
    let r = rig("default", clicks_then_done(10));
    let mut ep = r.episode("Sort the photos");
    let mut view = EpisodeView::default();
    assert_eq!(view.budget(), VIEW_BUDGET_BYTES);
    assert_eq!(VIEW_BUDGET_BYTES, 48_000);
    r.run(&mut ep, &mut view);
    let views = views(&r.model);
    assert!(views.windows(2).all(|w| w[1].starts_with(&w[0])));
    assert!(r.model.fold_calls.lock().unwrap().is_empty());
}

#[test]
fn zoom_opens_a_fold_into_its_two_children_and_a_step_into_its_raw_span() {
    let r = rig("zoom", Box::new(|n| match n {
        n if n < 12 => Act::Click(n as u32),
        12 => Act::Zoom("f1-2"),
        13 => Act::Zoom("s3"),
        _ => Act::Done,
    }));
    let mut ep = r.episode("Sort the photos");
    let mut view = EpisodeView::new(1_000);
    r.run(&mut ep, &mut view);
    let s1 = view.node("s1").unwrap().clone();
    let s2 = view.node("s2").unwrap().clone();
    assert_eq!(view.zoom("f1-2", 1).unwrap(), format!("[s1] {}\n[s2] {}\n", s1.text, s2.text));
    let spans = r.spans();
    let zooms: Vec<_> = spans.iter().filter(|s| s.tool == ZOOM_TOOL).collect();
    assert_eq!(zooms.len(), 2);
    assert_eq!(zooms[0].decision, "allow");
    assert!(zooms[0].result.starts_with("[s1] #1 goal="), "{}", zooms[0].result);
    let raw: Value = serde_json::from_str(&view.zoom("s3", 1).unwrap()).unwrap();
    assert_eq!((raw["tool"].as_str(), raw["claim"].as_str()), (Some("click"), Some("episode step 3")));
    assert_eq!(raw["episode"].as_str(), Some(ep.id.as_str()));
    // zoom is a read: the desktop never saw it.
    assert_eq!(r.desk.count("zoom"), 0);
    assert_eq!(r.desk.count("click"), 12);
}

#[test]
fn folds_and_spans_never_hold_a_typed_value() {
    let r = rig("typed", Box::new(|n| match n {
        n if n < 30 && n % 3 == 0 => Act::Type,
        n if n < 30 => Act::Click(n as u32),
        _ => Act::Done,
    }));
    let mut ep = r.episode("Fill the form");
    let mut view = EpisodeView::new(1_500);
    r.run(&mut ep, &mut view);
    assert_eq!(r.desk.count("type"), 10, "the value was typed");
    let file = std::fs::read_to_string(crate::harness::span_path(&r.dir, CHAT)).unwrap();
    assert!(!file.contains(TYPED));
    assert!(file.contains(r#"{\"chars\":22}"#), "typed text is kept as its length");
    assert!(!view.render().contains(TYPED));
    let folds = r.model.fold_calls.lock().unwrap();
    assert!(!folds.is_empty());
    for f in folds.iter() {
        let body = serde_json::to_string(&responses_body(f)).unwrap();
        assert!(!body.contains(TYPED), "{body}");
        assert!(!body.contains("FRAME"), "no screenshot in a fold: {body}");
    }
    for n in view.lines() {
        assert!(!n.text.contains(TYPED));
        assert!(n.text.len() <= FOLD_CAP_BYTES);
    }
}

#[test]
fn a_fan_out_runs_at_most_20_reads_and_counts_its_returns() {
    let r = rig("fan", Box::new(|n| if n == 0 { Act::Reads(25) } else { Act::Done }));
    let mut ep = r.episode("Find the config file");
    r.run(&mut ep, &mut EpisodeView::default());
    let spans = r.spans();
    let reads: Vec<_> = spans.iter().filter(|s| s.tool == "read_file").collect();
    assert_eq!(reads.len(), 20);
    assert!(reads.iter().all(|s| s.result.ends_with("(fan-out 20/20 returned)")), "{}", reads[0].result);
    assert_eq!(ep.steps, 20);
    let outs = fan_out(vec![
        Box::new(|| ToolOutput::ok("a")),
        Box::new(|| -> ToolOutput { panic!("worker died") }),
        Box::new(|| ToolOutput::ok("c")),
    ]);
    assert_eq!(outs.len(), 3, "a dead worker still has its row");
    assert_eq!(outs[1], ToolOutput::err(DEAD_WORKER));
    assert_eq!((outs[0].text.as_str(), outs[2].text.as_str()), ("a", "c"));
}

#[test]
fn idle_stall_and_header_are_named_and_progress_resets_the_stall() {
    assert_eq!((EPISODE_IDLE.as_secs(), FANOUT_CAP, STALL_REPLAN), (600, 20, 8));
    let mut ep = Episode::begin("ep-1", CHAT, "Go", T0, &[]);
    assert!(!ep.idle(T0 + 599_999));
    assert!(ep.idle(T0 + 600_000));
    ep.steps = 12;
    assert_eq!(ep.header(T0 + 4 * 60_000 + 5_000), "Desktop session · 12 steps · 4 min");
    assert_eq!(episode_header(1, std::time::Duration::ZERO), "Desktop session · 1 step · 0 min");
    // A screen change or a quiet success moves; no change or a failure doesn't; a park is neutral.
    for _ in 0..7 {
        assert!(!ep.note_progress("allow", Some(false), false));
    }
    assert!(!ep.note_progress("park", None, false));
    assert_eq!(ep.stall, 7);
    assert!(ep.note_progress("deny", None, true), "the 8th step with no progress re-plans");
    assert_eq!(ep.stall, 0);
    assert!(!ep.note_progress("allow", None, true));
    assert!(!ep.note_progress("allow", Some(true), true));
    assert_eq!(ep.stall, 0, "the screen changed");
    assert!(!ep.note_progress("allow", Some(false), false));
    assert!(!ep.note_progress("allow", None, false));
    assert_eq!(ep.stall, 0, "a read with no screen reading that worked moved");
    let held = vec!["hunter2222".to_string()];
    let secret = Episode::begin("ep-2", CHAT, "Log in with hunter2222 and sk-abcdefghijklmnopqrstuv", T0, &held);
    assert_eq!(secret.goal, "Log in with [redacted] and [redacted]");
    let shape = StepShape {
        goal_step: "Go".into(),
        tool: "click".into(),
        decision: "allow".into(),
        ui_changed: Some(false),
        result: "click ok".into(),
    };
    assert_eq!(shape.line(3), r#"#3 goal="Go" tool=click decision=allow ui_changed=false result="click ok""#);
    assert!(new_episode_id(T0).starts_with("ep-1a3185c5000-"));
}

#[test]
fn stop_during_a_worker_call_ends_the_episode_and_denies_its_parks() {
    let mut r = rig("stop-mid-call", Box::new(|n| match n {
        0 => Act::HardClick,
        _ => Act::Click(n as u32),
    }));
    r.model.cancel_at = Some((1, r.cancel.clone()));
    let mut ep = r.episode("Reply to the thread");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Stop));
    assert_eq!(ep.ended, Some(EpisodeEnd::Stop));
    let park = format!("{PARK_PREFIX}{}-1", ep.id);
    assert_eq!(*r.parks.withdrawn.lock().unwrap(), vec![park]);
    let spans = r.spans();
    let deny = spans.iter().find(|s| s.decision == "deny").expect("the parked Send is denied");
    assert_eq!((deny.tool.as_str(), deny.result.as_str()), ("click", "stopped — fail-closed Deny"));
    assert_eq!(r.desk.count("click"), 0, "the parked Send never ran");
    assert!(ep.parks.is_empty());
}

#[test]
fn workspace_deny_rules_hold_inside_an_episode() {
    let mut r = rig("policy", Box::new(|n| match n {
        0 => Act::Shell("echo episode-policy"),
        _ => Act::Done,
    }));
    let mut policy = crate::perm::Policy::empty();
    policy.rules.push(crate::perm::parse_rule("Bash(echo *)", crate::perm::Action::Deny).unwrap());
    r.perms = Some(policy);
    let mut ep = r.episode("Say hi in a terminal");
    r.run(&mut ep, &mut EpisodeView::default());
    let spans = r.spans();
    let step = spans.iter().find(|s| s.tool == "run_terminal_command").expect("the shell step is logged");
    assert_eq!(step.decision, "deny", "Always mode still obeys a deny rule: {}", step.result);
}

#[test]
fn a_long_goal_reaches_the_worker_and_the_checker_whole_and_spans_keep_200() {
    let r = rig("long-goal", clicks_then_done(1));
    let goal = format!("Open Settings, then {} and finally turn on Wi-Fi", "check the network list carefully, ".repeat(10));
    assert!(goal.chars().count() > 300);
    let mut ep = r.episode(&goal);
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    let worker = serde_json::to_string(&responses_body(&r.model.worker_calls.lock().unwrap()[0])).unwrap();
    assert!(worker.contains("finally turn on Wi-Fi"), "the worker sees the end of the goal");
    let judge = serde_json::to_string(&responses_body(&r.model.judge_calls.lock().unwrap()[0])).unwrap();
    assert!(judge.contains("finally turn on Wi-Fi"), "the checker judges the whole goal");
    for s in r.spans() {
        assert!(s.goal_step.chars().count() <= GOAL_CAP, "{}", s.goal_step.chars().count());
    }
    assert_eq!(r.spans().iter().find(|s| is_step(s)).unwrap().goal_step.chars().count(), GOAL_CAP);
}

/// The `harness_recovery` spans in order: (decision, detector, claim).
fn recoveries(spans: &[crate::harness::Span]) -> Vec<(String, String, String)> {
    spans
        .iter()
        .filter(|s| s.tool == crate::harness::RECOVERY_TOOL)
        .map(|s| {
            let args: Value = serde_json::from_str(&s.args_redacted).unwrap();
            (s.decision.clone(), args["detector"].as_str().unwrap_or("").to_string(), s.claim.clone())
        })
        .collect()
}

#[test]
fn a_reject_with_no_finding_replans_with_the_reason_and_later_verifies() {
    let mut r = rig("reject-replan", Box::new(|n| match n {
        0 => Act::Click(5),
        1 => Act::DoneSelfChecked,
        2 => Act::Click(6),
        _ => Act::Done,
    }));
    r.model.judge_says = "REJECT: notes.txt is not saved";
    r.model.judge_later = Some((1, "VERIFY_OK"));
    let mut ep = r.episode("Save notes.txt in the editor");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified), "a reject never leaves it waiting");
    let spans = r.spans();
    assert_eq!(
        recoveries(&spans),
        vec![("replan".to_string(), "verify_reject".to_string(), "verify_reject: notes.txt is not saved".to_string())]
    );
    let replan = spans.iter().find(|s| s.tool == crate::harness::RECOVERY_TOOL).unwrap();
    assert_eq!(replan.origin, crate::harness::Origin::Repair);
    let next = r.model.worker_calls.lock().unwrap()[2].clone();
    assert!(
        first_text(&next, 2).contains(&format!("GrokHub's check: {REPLAN_NOTE} The independent check said: notes.txt is not saved")),
        "{}",
        first_text(&next, 2)
    );
    let checks: Vec<&str> = spans.iter().filter(|s| s.tool == VERIFY_TOOL).map(|s| s.result.as_str()).collect();
    assert_eq!(checks, vec!["fail", "pass"]);
    assert_eq!(r.model.judge_calls.lock().unwrap().len(), 2);
    assert_eq!(ep.ended, Some(EpisodeEnd::Verified));
}

#[test]
fn a_checker_that_cannot_run_is_retried_then_escalated_and_passes() {
    let mut r = rig("checker-escalate", clicks_then_done(1));
    r.model.judge_errors = 2;
    let mut ep = r.episode("Turn on Wi-Fi in Settings");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    assert_eq!(r.model.judge_calls.lock().unwrap().len(), 3);
    let spans = r.spans();
    let rec = recoveries(&spans);
    assert_eq!(rec.len(), 2);
    assert_eq!((rec[0].0.as_str(), rec[0].1.as_str()), ("retry", "checker_error"));
    assert_eq!((rec[1].0.as_str(), rec[1].1.as_str()), ("escalate", "checker_error"));
    assert!(rec[0].2.starts_with("checker_error: ") && rec[0].2.contains("checker offline"), "{}", rec[0].2);
    let check = spans.iter().find(|s| s.tool == VERIFY_TOOL).unwrap();
    assert_eq!(check.result, "pass");
    assert_eq!(check.tokens.as_ref().unwrap().class, "episode:step", "the last try routes as the worker's class");
    assert_eq!(r.model.worker_calls.lock().unwrap().len(), 2, "the worker was not sent back");
}

#[test]
fn a_checker_still_down_after_escalating_replans_as_checker_unavailable() {
    let mut r = rig("checker-down", Box::new(|_| Act::Done));
    r.model.judge_errors = 3;
    let mut ep = r.episode("Turn on Wi-Fi in Settings");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    let rec = recoveries(&r.spans());
    let decisions: Vec<(&str, &str)> = rec.iter().map(|(d, det, _)| (d.as_str(), det.as_str())).collect();
    assert_eq!(decisions, vec![("retry", "checker_error"), ("escalate", "checker_error"), ("replan", "verify_reject")]);
    assert_eq!(rec[2].2, "verify_reject: checker unavailable");
    let next = r.model.worker_calls.lock().unwrap()[1].clone();
    assert!(first_text(&next, 2).contains("The independent check said: checker unavailable"), "{}", first_text(&next, 2));
    assert_eq!(r.model.judge_calls.lock().unwrap().len(), 4, "three tries, then the next claim's check");
}

#[test]
fn the_same_reject_three_times_on_an_unchanged_screen_ends_with_a_named_note() {
    let mut r = rig("same-reject", Box::new(|_| Act::Done));
    r.desk.still = true;
    r.model.judge_says = "REJECT: the Wi-Fi toggle is still off";
    let mut ep = r.episode("Turn on Wi-Fi in Settings");
    let out = r.run(&mut ep, &mut EpisodeView::default());
    assert_eq!(out.stop, EpisodeStop::Ended(EpisodeEnd::Unconfirmed));
    assert_eq!(out.reply, "Couldn't confirm: Turn on Wi-Fi in Settings — checker says the Wi-Fi toggle is still off");
    assert_eq!(ep.ended, Some(EpisodeEnd::Unconfirmed));
    assert_eq!(r.model.judge_calls.lock().unwrap().len(), 3);
    let spans = r.spans();
    let end = spans.last().unwrap();
    assert_eq!((end.tool.as_str(), end.decision.as_str(), end.result.as_str()), (EPISODE_TOOL, "end", "unconfirmed"));
    assert!(r.parks.posted.lock().unwrap().is_empty(), "no card");
    assert_eq!(EpisodeEnd::Unconfirmed.as_str(), "unconfirmed");

    // The same reject while the screen keeps changing keeps going.
    let mut r = rig("same-reject-moving", Box::new(|n| if n % 2 == 0 { Act::Done } else { Act::Click(n as u32) }));
    r.model.judge_says = "REJECT: the Wi-Fi toggle is still off";
    r.model.judge_later = Some((4, "VERIFY_OK"));
    let mut ep = r.episode("Turn on Wi-Fi in Settings");
    assert_eq!(r.run(&mut ep, &mut EpisodeView::default()).stop, EpisodeStop::Ended(EpisodeEnd::Verified));
    assert_eq!(r.model.judge_calls.lock().unwrap().len(), 5);
}

#[test]
fn note_reject_counts_only_the_same_reason_on_the_same_screen() {
    let mut ep = Episode::begin("ep-r", CHAT, "Save notes.txt", T0, &[]);
    assert!(!ep.note_reject("not saved", "aaa"));
    assert!(!ep.note_reject("not saved", "aaa"));
    assert!(!ep.note_reject("not saved", "bbb"), "the screen changed: the count starts over");
    assert!(!ep.note_reject("wrong folder", "bbb"), "another reason starts over");
    assert!(!ep.note_reject("wrong folder", "bbb"));
    assert!(ep.note_reject("wrong folder", "bbb"));
    assert_eq!(ep.last_reject, Some(("wrong folder".into(), "bbb".into(), 3)));
}
