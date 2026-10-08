//! The episode kernel loop: observe → propose tool → gate → execute (or
//! park) → observe → claim. Each step the worker starts fresh from the
//! episode view; nothing grows a transcript.

use std::path::Path;

use serde_json::{json, Value};

use super::verify::{verify_gate, Observation, Verdict};
use super::view::{zoom_schema, EpisodeView, Folder, ZOOM_TOOL};
use super::{CapHit, Episode, EpisodeEnd, OpenPark, StepShape, FANOUT_CAP, PARK_PREFIX};
use crate::client::{ContentPart, FunctionCall, InputItem, ModelClient, Usage};
use crate::gate::{self, Decision, Gate, PermAnswer, PermitWait, Waited};
use crate::harness::{
    self, action_loop, append_span, desk_args, done_without_criteria, hard_class, ladder_span, post_park, AccessMode,
    Finding, GateOutcome, HardClass, LadderStep, ParkRequest, Rung, Span, Step, APPROVAL_TTL, VERIFY_TOOL,
};
use crate::route::{call_model, CallTokens, ModelCall, BACKGROUND_EFFORT, CLASS_COMPACT, CLASS_EPISODE};
use crate::run::{HaltCheck, LoopEvent, SteerQueue};
use crate::tools::{self, DesktopOps, ToolCtx, ToolOutput};
use crate::CancelToken;

/// What every worker step is told, after the cabin's own system prompt.
/// Fixed text, so the prompt prefix stays cacheable.
pub const EPISODE_RULES: &str = "You are on a supervised desktop session the user is watching. \
Each step you see the episode so far, the current goal step and the latest observation. \
Call one or a few desktop tools to make progress. A step that sends, pays, deletes, types a credential or cannot be undone waits for the user's click. \
When the goal is met, reply GOAL_COMPLETE with no tool calls; an independent check decides whether it is done.";

/// A fan-out read whose worker died returns this, so it can't hide.
pub const DEAD_WORKER: &str = "worker returned nothing";
/// A read past [`FANOUT_CAP`] in one fan-out.
pub const FANOUT_CAPPED: &str = "not run: one fan-out runs at most 20 reads";
const PARKED: &str = "parked: waiting for your approval";
/// Tools only the chat run loop can serve.
const LOOP_ONLY: &[&str] = &[
    "spawn_subagent",
    "send_subagent_message",
    "todo_write",
    "ask_user_question",
    "enter_plan_mode",
    "exit_plan_mode",
];

/// Where hard steps wait for the user. The cabin answers them on its card.
pub trait Parks {
    fn post(&self, req: &ParkRequest) -> bool;
    fn answer(&self, id: &str) -> Option<bool>;
    fn withdraw(&self, id: &str);
}

/// Park files under `{config_dir}/harness/park`, the same handoff path A uses.
pub struct FileParks<'a>(pub &'a Path);

impl Parks for FileParks<'_> {
    fn post(&self, req: &ParkRequest) -> bool {
        post_park(self.0, req).is_ok()
    }

    fn answer(&self, id: &str) -> Option<bool> {
        harness::take_answer(self.0, id)
    }

    fn withdraw(&self, id: &str) {
        harness::clear_park(self.0, id);
    }
}

pub struct KernelIn<'a> {
    pub client: &'a dyn ModelClient,
    pub provider: &'a str,
    pub model: &'a str,
    /// The worker's effort, as the caller already chose it. Background
    /// calls use [`BACKGROUND_EFFORT`].
    pub effort: Option<&'a str>,
    pub system: &'a str,
    pub workspace: &'a Path,
    /// Spans go to `{config_dir}/spans`.
    pub config_dir: &'a Path,
    pub gate: Gate,
    pub access: AccessMode,
    pub desktop: &'a dyn DesktopOps,
    pub parks: &'a dyn Parks,
    pub permits: &'a dyn PermitWait,
    pub halt: &'a dyn HaltCheck,
    pub cancel: &'a CancelToken,
    pub steer: &'a SteerQueue,
    /// Wall clock in ms (a test clock in tests).
    pub clock: &'a dyn Fn() -> u64,
    /// Secrets the user typed this session.
    pub held: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EpisodeStop {
    Ended(EpisodeEnd),
    /// A long-run limit: the cabin shows the pause card.
    Paused(CapHit),
    /// The 1a ladder paused (a rejected claim or a loop it can't repair).
    LadderPause(LadderStep),
    /// The worker answered without tools and without claiming done. The
    /// episode stays open for the user's next message.
    Waiting,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeOut {
    pub stop: EpisodeStop,
    pub usage: Usage,
    /// The worker's last words (the reply bubble).
    pub reply: String,
}

/// Run reads side by side and count what comes back: a worker that panics
/// returns [`DEAD_WORKER`] in its slot, never a missing row.
pub fn fan_out<'a>(jobs: Vec<Box<dyn FnOnce() -> ToolOutput + Send + 'a>>) -> Vec<ToolOutput> {
    std::thread::scope(|s| {
        let handles: Vec<_> = jobs.into_iter().map(|job| s.spawn(job)).collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| ToolOutput::err(DEAD_WORKER)))
            .collect()
    })
}

/// The span for one step, in the fixed shape, tagged with the episode.
pub fn step_span(ep: &Episode, shape: &StepShape, args_redacted: &str, class: &str, tokens: Option<CallTokens>) -> Span {
    let mut s = Span::deny(&ep.chat_id, &shape.tool, args_redacted, &shape.result, class);
    s.decision = shape.decision.clone();
    s.claim = format!("episode step {}", ep.steps);
    s.driver = "native".into();
    s.ui_changed = shape.ui_changed;
    s.goal_step = shape.goal_step.clone();
    s.hard_approved = shape.decision == "approve";
    s.tokens = tokens;
    s.in_episode(&ep.id)
}

struct RouteFolder<'a> {
    k: &'a KernelIn<'a>,
    conversation: &'a str,
}

impl Folder for RouteFolder<'_> {
    fn fold(&self, left: &str, right: &str) -> Result<(String, Option<CallTokens>), String> {
        let input = vec![
            InputItem::Message {
                role: "system".into(),
                content: vec![ContentPart::InputText(super::view::FOLD_SYSTEM.into())],
            },
            InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText(format!("Older:\n{left}\n\nNewer:\n{right}"))],
            },
        ];
        let mut call = ModelCall::xai(self.k.model, Some(BACKGROUND_EFFORT), CLASS_COMPACT, self.conversation, input);
        call.provider = self.k.provider.into();
        let routed = call_model(self.k.client, &call, self.k.cancel).map_err(|e| e.to_string())?;
        Ok((routed.out.text, Some(routed.tokens)))
    }
}

struct Run<'a, 'b> {
    k: &'a KernelIn<'a>,
    ep: &'b mut Episode,
    view: &'b mut EpisodeView,
    usage: Usage,
    /// Tokens of the worker call that proposed the next step; the first
    /// step span of the batch carries them.
    tokens: Option<CallTokens>,
    obs: Option<Observation>,
}

/// Drive one turn of an episode. It returns when the episode ends, pauses,
/// waits on the user, or errors. Call again with the same `ep` and `view`
/// for the next turn.
pub fn run_episode(
    k: &KernelIn<'_>,
    ep: &mut Episode,
    view: &mut EpisodeView,
    on_event: &mut dyn FnMut(LoopEvent),
) -> EpisodeOut {
    let mut run = Run { k, ep, view, usage: Usage::default(), tokens: None, obs: None };
    if run.ep.trail.is_empty() {
        let goal = format!("goal: {}", run.ep.goal);
        run.write(run.ep.marker("begin", "open", &goal));
    }
    if let Some(end) = run.ep.ended {
        return run.out(EpisodeStop::Ended(end), String::new());
    }
    loop {
        if k.halt.halted() {
            run.end_with_parks_denied(EpisodeEnd::Halt, "halt — fail-closed Deny");
            return run.out(EpisodeStop::Ended(EpisodeEnd::Halt), String::new());
        }
        if k.cancel.is_cancelled() {
            run.end_with_parks_denied(EpisodeEnd::Stop, "stopped — fail-closed Deny");
            return run.out(EpisodeStop::Ended(EpisodeEnd::Stop), String::new());
        }
        run.settle_parks(on_event);
        for note in k.steer.drain() {
            run.ep.steer(&note, k.held);
        }
        if let Some(stop) = run.check_cap() {
            return run.out(stop, String::new());
        }
        if run.obs.is_none() {
            run.obs = run.observe();
        }
        let turn = match run.ask_worker() {
            Ok(turn) => turn,
            Err(err) => return run.out(EpisodeStop::Error(err), String::new()),
        };
        on_event(LoopEvent::Usage(run.usage.clone()));
        if turn.calls.is_empty() {
            match run.claim(&turn.text) {
                Some(stop) => {
                    if !turn.text.trim().is_empty() {
                        on_event(LoopEvent::Text(turn.text.clone()));
                    }
                    return run.out(stop, turn.text);
                }
                None => continue,
            }
        }
        if let Some(stop) = run.batch(&turn.calls, on_event) {
            return run.out(stop, String::new());
        }
        if k.halt.halted() {
            continue;
        }
        if let Some(stop) = run.check_loop() {
            return run.out(stop, String::new());
        }
    }
}

impl Run<'_, '_> {
    fn out(&self, stop: EpisodeStop, reply: String) -> EpisodeOut {
        EpisodeOut { stop, usage: self.usage.clone(), reply }
    }

    fn now(&self) -> u64 {
        (self.k.clock)()
    }

    fn write(&mut self, span: Span) {
        let mut span = span.on_path("E").in_turn(&self.ep.chat_id, self.ep.turn).in_episode(&self.ep.id);
        if span.access.is_empty() {
            span.access = self.k.access.as_str().into();
        }
        let _ = append_span(self.k.config_dir, &span);
        self.ep.trail.push(span);
    }

    /// Record a step: its span, its view line, and the fold calls it caused.
    fn record(&mut self, shape: &StepShape, args_redacted: &str, class: &str, view_id: &str) {
        let tokens = self.tokens.take();
        let span = step_span(self.ep, shape, args_redacted, class, tokens);
        let raw = serde_json::to_string(&span).unwrap_or_default();
        self.write(span);
        let line = shape.line(self.ep.steps);
        let conversation = self.ep.id.clone();
        let folds = {
            let folder = RouteFolder { k: self.k, conversation: &conversation };
            self.view.push(view_id, &line, &raw, self.k.held, &folder)
        };
        for fold in folds {
            if let Some(t) = &fold.tokens {
                self.usage.add(&usage_of(t));
            }
            let mut s = self.ep.marker("fold", &fold.id, &format!("folded {} and {}", fold.children[0], fold.children[1]));
            s.tokens = fold.tokens;
            self.write(s);
        }
        self.ep.last_ms = self.now();
    }

    fn check_cap(&mut self) -> Option<EpisodeStop> {
        let hit = self.ep.cap_hit(self.now())?;
        if self.ep.paused != Some(hit) {
            self.ep.paused = Some(hit);
            let span = self.ep.marker("pause", hit.key(), &hit.question());
            self.write(span);
        }
        Some(EpisodeStop::Paused(hit))
    }

    /// A screenshot through the same gate as any step. Not a step itself;
    /// `None` when the gate would not run it without asking.
    fn observe(&self) -> Option<Observation> {
        if self.k.halt.halted() || self.k.desktop.halted() {
            return None;
        }
        let desk = tools::desk_flags("screenshot", &self.k.gate, Some(self.k.desktop));
        match gate::decide_with(&self.k.gate, "screenshot", "{}", false, desk, self.k.workspace, None) {
            Decision::Run => Some(Observation::from_output(&self.k.desktop.call("screenshot", &json!({})))),
            _ => None,
        }
    }

    fn input(&self) -> Vec<InputItem> {
        let system = if self.k.system.trim().is_empty() {
            super::kernel::EPISODE_RULES.to_string()
        } else {
            format!("{}\n\n{EPISODE_RULES}", self.k.system)
        };
        let mut now = format!("Step {} of this episode.\nGoal step: {}\n", self.ep.steps + 1, self.ep.goal_step);
        if let Some(note) = &self.ep.note {
            now.push_str(&format!("GrokHub's check: {note}\n"));
        }
        let mut content = Vec::new();
        match &self.obs {
            Some(obs) => {
                now.push_str(&format!("Latest observation:\n{}", obs.line()));
                content.push(ContentPart::InputText(now));
                if let Some(img) = &obs.image {
                    content.push(ContentPart::InputImage(img.clone()));
                }
            }
            None => {
                now.push_str("Latest observation: none yet (take a screenshot).");
                content.push(ContentPart::InputText(now));
            }
        }
        vec![
            InputItem::Message { role: "system".into(), content: vec![ContentPart::InputText(system)] },
            InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText(self.view.render())] },
            InputItem::Message { role: "user".into(), content },
        ]
    }

    /// The native tool set the gate allows, minus what needs the run loop
    /// itself (subagents, plan mode, todos), plus `zoom`.
    fn worker_tools(&self) -> Vec<Value> {
        let mut t: Vec<Value> = tools::schemas_for(&self.k.gate)
            .into_iter()
            .filter(|s| {
                let name = s.get("name").and_then(|n| n.as_str()).unwrap_or("");
                !LOOP_ONLY.contains(&name)
            })
            .collect();
        t.push(zoom_schema());
        t
    }

    fn ask_worker(&mut self) -> Result<crate::client::TurnOutput, String> {
        let mut call = ModelCall::xai(self.k.model, self.k.effort, CLASS_EPISODE, &self.ep.id, self.input())
            .with_tools(self.worker_tools());
        call.provider = self.k.provider.into();
        let routed = call_model(self.k.client, &call, self.k.cancel).map_err(|e| e.to_string())?;
        self.usage.add(&routed.out.usage);
        self.tokens = Some(routed.tokens);
        self.ep.note = None;
        Ok(routed.out)
    }

    /// No tools: a claim. `GOAL_COMPLETE` goes to VerifyGate; anything else
    /// waits on the user. `None` means the ladder sent the worker back.
    fn claim(&mut self, text: &str) -> Option<EpisodeStop> {
        let mut reply = Span::reply(&self.ep.chat_id, text, self.k.held);
        reply.tokens = self.tokens.take();
        if !grokhub_core::verify::has_goal_complete(text) {
            self.write(reply);
            return Some(EpisodeStop::Waiting);
        }
        let fin = self.observe().unwrap_or_else(|| Observation {
            text: "no observation: the gate did not allow a screenshot".into(),
            ..Observation::default()
        });
        let verdict = verify_gate(self.k.client, self.k.model, &self.ep.goal, &fin, &self.ep.id, self.k.cancel);
        let (verdict, tokens) = match verdict {
            Ok((v, t)) => {
                self.usage.add(&usage_of(&t));
                (v, Some(t))
            }
            Err(e) => (Verdict::Reject(format!("the checker could not run: {e}")), None),
        };
        self.write(reply);
        let mut check = Span::deny(&self.ep.chat_id, VERIFY_TOOL, "{}", "", "soft");
        check.decision = "allow".into();
        check.tokens = tokens;
        match verdict {
            Verdict::Ok => {
                check.result = "pass".into();
                check.claim = "VERIFY_OK".into();
                self.write(check);
                self.end(EpisodeEnd::Verified);
                Some(EpisodeStop::Ended(EpisodeEnd::Verified))
            }
            Verdict::Reject(why) => {
                check.result = "fail".into();
                check.claim = why.clone();
                self.write(check);
                match done_without_criteria(&self.ep.trail).pop() {
                    Some(finding) => self.ladder(finding, Some(&why)),
                    None => Some(EpisodeStop::Waiting),
                }
            }
        }
    }

    /// One ladder rung for a finding: a repair note for the next step, or a pause.
    fn ladder(&mut self, finding: Finding, why: Option<&str>) -> Option<EpisodeStop> {
        let step = self.ep.ladder.next(&finding, &self.ep.trail);
        let span = ladder_span(&self.ep.chat_id, &step);
        self.write(span);
        match step.rung {
            Rung::Retry | Rung::Backtrack => {
                let mut note = step.prompt.clone().unwrap_or_default();
                if let Some(why) = why {
                    note.push_str(&format!(" The independent check said: {why}"));
                }
                self.ep.note = Some(note);
                None
            }
            Rung::Pause => Some(EpisodeStop::LadderPause(step)),
        }
    }

    /// 1a still catches looping steps mid-episode.
    fn check_loop(&mut self) -> Option<EpisodeStop> {
        let finding = action_loop(&self.ep.trail).into_iter().find(|f| {
            let key = format!("{}:{}", f.detector, f.evidence.first().map_or("", |e| e.span.as_str()));
            !self.ep.seen_findings.contains(&key)
        })?;
        let key = format!("{}:{}", finding.detector, finding.evidence.first().map_or("", |e| e.span.as_str()));
        self.ep.seen_findings.push(key);
        self.ladder(finding, None)
    }

    fn end(&mut self, why: EpisodeEnd) {
        self.ep.ended = Some(why);
        let header = self.ep.header(self.now());
        let span = self.ep.marker("end", why.as_str(), &header);
        self.write(span);
    }

    /// Halt or Stop: every open park is denied with a span, then the episode ends.
    fn end_with_parks_denied(&mut self, why: EpisodeEnd, reason: &str) {
        for park in std::mem::take(&mut self.ep.parks) {
            let _ = self.k.parks.answer(&park.id);
            self.k.parks.withdraw(&park.id);
            let span = Span::deny(&self.ep.chat_id, &park.tool, &park.args_redacted, reason, park.class.as_str());
            let mut span = span.in_episode(&self.ep.id);
            span.goal_step = park.goal_step.clone();
            self.write(span);
        }
        self.end(why);
    }

    /// Answered parks run once or are denied; expired ones fail closed.
    fn settle_parks(&mut self, on_event: &mut dyn FnMut(LoopEvent)) {
        let now = self.now();
        for park in std::mem::take(&mut self.ep.parks) {
            let answer = self.k.parks.answer(&park.id);
            let expired = now.saturating_sub(park.parked_ms) >= APPROVAL_TTL.as_millis() as u64;
            let deny = match (answer, expired) {
                (Some(true), _) => None,
                (Some(false), _) => Some("denied".to_string()),
                (None, true) => {
                    self.k.parks.withdraw(&park.id);
                    Some(format!("hard-class {} timed out ({}s) — fail-closed Deny", park.class.as_str(), APPROVAL_TTL.as_secs()))
                }
                (None, false) => {
                    self.ep.parks.push(park);
                    continue;
                }
            };
            let id = format!("s{}r", park.step);
            match deny {
                Some(reason) => {
                    let shape = StepShape {
                        goal_step: park.goal_step.clone(),
                        tool: park.tool.clone(),
                        decision: "deny".into(),
                        ui_changed: None,
                        result: reason,
                    };
                    self.record(&shape, &park.args_redacted, park.class.as_str(), &id);
                }
                None => {
                    let call = FunctionCall { call_id: park.id.clone(), name: park.tool.clone(), arguments: park.args.clone() };
                    emit(on_event, &call, "in_progress", "", None);
                    let (out, ui) = self.execute(&call);
                    emit(on_event, &call, if out.failed { "failed" } else { "completed" }, &out.text, out.image_data_url.clone());
                    let shape = StepShape {
                        goal_step: park.goal_step.clone(),
                        tool: park.tool.clone(),
                        decision: "approve".into(),
                        ui_changed: ui,
                        result: result_text(&out),
                    };
                    self.record(&shape, &park.args_redacted, park.class.as_str(), &id);
                }
            }
        }
    }

    /// Run a desktop or read tool once (the gate already said yes) and
    /// observe after a desktop act.
    fn execute(&mut self, call: &FunctionCall) -> (ToolOutput, Option<bool>) {
        let stop = || self.k.halt.halted();
        let ctx = ToolCtx { workspace: self.k.workspace, desktop: Some(self.k.desktop), stop: &stop, tasks: None, owner: None };
        let out = tools::dispatch(&ctx, &call.name, &call.arguments);
        if call.name == "screenshot" {
            self.obs = Some(Observation::from_output(&out));
            return (out, None);
        }
        if !gate::is_desktop(&call.name) || self.k.halt.halted() {
            return (out, None);
        }
        let before = self.obs.as_ref().map(|o| o.hash.clone());
        let after = self.observe();
        let changed = match (&before, &after) {
            (Some(b), Some(a)) => Some(*b != a.hash),
            _ => None,
        };
        if after.is_some() {
            self.obs = after;
        }
        (out, changed)
    }

    /// One model turn's calls. Independent reads fan out; everything else
    /// runs in order. A parked or denied hard step stops the rest.
    fn batch(&mut self, calls: &[FunctionCall], on_event: &mut dyn FnMut(LoopEvent)) -> Option<EpisodeStop> {
        let reads = calls.len() > 1 && calls.iter().all(|c| is_read(&c.name));
        if reads {
            return self.fan(calls, on_event);
        }
        for (i, call) in calls.iter().enumerate() {
            if self.k.halt.halted() || self.k.cancel.is_cancelled() {
                return None;
            }
            if let Some(stop) = self.check_cap() {
                return Some(stop);
            }
            if !self.step(call, on_event) {
                for rest in &calls[i + 1..] {
                    emit(on_event, rest, "failed", gate::NOT_EXECUTED, None);
                }
                break;
            }
        }
        None
    }

    fn fan(&mut self, calls: &[FunctionCall], on_event: &mut dyn FnMut(LoopEvent)) -> Option<EpisodeStop> {
        let room = self.ep.cap_room();
        let take = calls.len().min(FANOUT_CAP).min(room);
        let workspace = self.k.workspace;
        let view = &*self.view;
        let jobs: Vec<Box<dyn FnOnce() -> ToolOutput + Send + '_>> = calls[..take]
            .iter()
            .map(|c| {
                let (name, args) = (c.name.clone(), c.arguments.clone());
                let job: Box<dyn FnOnce() -> ToolOutput + Send> = if name == ZOOM_TOOL {
                    let opened = zoom(view, &args);
                    Box::new(move || opened)
                } else {
                    Box::new(move || tools::execute(workspace, &name, &args))
                };
                job
            })
            .collect();
        for c in &calls[..take] {
            emit(on_event, c, "in_progress", "", None);
        }
        let outs = fan_out(jobs);
        let returned = outs.iter().filter(|o| o.text != DEAD_WORKER).count();
        for (c, out) in calls[..take].iter().zip(outs) {
            self.ep.steps += 1;
            emit(on_event, c, if out.failed { "failed" } else { "completed" }, &out.text, None);
            let shape = StepShape {
                goal_step: self.ep.goal_step.clone(),
                tool: c.name.clone(),
                decision: "allow".into(),
                ui_changed: None,
                result: format!("{} (fan-out {returned}/{take} returned)", result_text(&out)),
            };
            let id = format!("s{}", self.ep.steps);
            self.record(&shape, &arg_line(&c.name, &c.arguments), "soft", &id);
        }
        for c in &calls[take..] {
            emit(on_event, c, "failed", FANOUT_CAPPED, None);
        }
        self.check_cap()
    }

    /// Gate one call and run, park, or deny it. False stops the batch.
    fn step(&mut self, call: &FunctionCall, on_event: &mut dyn FnMut(LoopEvent)) -> bool {
        self.ep.steps += 1;
        let n = self.ep.steps;
        let id = format!("s{n}");
        let args_redacted = arg_line(&call.name, &call.arguments);
        let goal_step = self.ep.goal_step.clone();
        let shape = |decision: &str, ui: Option<bool>, result: String| StepShape {
            goal_step: goal_step.clone(),
            tool: call.name.clone(),
            decision: decision.into(),
            ui_changed: ui,
            result,
        };
        emit(on_event, call, "in_progress", "", None);
        match self.verdict(call) {
            GateOutcome::Refuse { reason } => {
                emit(on_event, call, "failed", &reason, None);
                let class = hard_class(&call.name, &call.arguments).map_or("soft", |c| c.as_str());
                self.record(&shape("deny", None, reason), &args_redacted, class, &id);
                class == "soft"
            }
            GateOutcome::Park { hard: Some(class), .. } => {
                self.park(call, class, n, &args_redacted);
                emit(on_event, call, "completed", PARKED, None);
                self.record(&shape("park", None, PARKED.into()), &args_redacted, class.as_str(), &id);
                false
            }
            GateOutcome::Park { hard: None, reason, .. } => {
                if !self.ask(call, &reason, on_event) {
                    emit(on_event, call, "failed", &gate::user_rejected(&call.name), None);
                    self.record(&shape("deny", None, gate::user_rejected(&call.name)), &args_redacted, "soft", &id);
                    return true;
                }
                self.run_allowed(call, &shape, &args_redacted, &id, on_event);
                true
            }
            GateOutcome::Allow => {
                self.run_allowed(call, &shape, &args_redacted, &id, on_event);
                true
            }
        }
    }

    fn run_allowed(
        &mut self,
        call: &FunctionCall,
        shape: &dyn Fn(&str, Option<bool>, String) -> StepShape,
        args_redacted: &str,
        id: &str,
        on_event: &mut dyn FnMut(LoopEvent),
    ) {
        let (out, ui) = if call.name == ZOOM_TOOL {
            (zoom(self.view, &call.arguments), None)
        } else {
            self.execute(call)
        };
        emit(on_event, call, if out.failed { "failed" } else { "completed" }, &out.text, out.image_data_url.clone());
        self.record(&shape("allow", ui, result_text(&out)), args_redacted, "soft", id);
    }

    /// `harness::decide` first (the only hard gate; a desktop call is also
    /// read by its args, as on path A), then the soft gate for Ask / Auto /
    /// Always. `zoom` is a read: decide only, it never runs anything.
    fn verdict(&self, call: &FunctionCall) -> GateOutcome {
        let hard = harness::decide(Step::Tool { name: &call.name, arguments: &call.arguments });
        if !hard.is_allow() {
            return hard;
        }
        if gate::is_desktop(&call.name) {
            let args: Value = serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}));
            let desk = harness::decide(Step::Desk { tool: &call.name, args: &args });
            if !desk.is_allow() {
                return desk;
            }
        }
        if call.name == ZOOM_TOOL {
            return GateOutcome::Allow;
        }
        let desk = tools::desk_flags(&call.name, &self.k.gate, Some(self.k.desktop));
        match gate::decide_with(&self.k.gate, &call.name, &call.arguments, false, desk, self.k.workspace, None) {
            Decision::Run => GateOutcome::Allow,
            Decision::Ask => GateOutcome::Park { reason: format!("permission ask for `{}`", call.name), hard: None, needs_jeremy: false },
            Decision::Refuse(reason) => GateOutcome::Refuse { reason },
        }
    }

    /// A soft ask: the cabin's own permission card, like any native step.
    fn ask(&mut self, call: &FunctionCall, reason: &str, on_event: &mut dyn FnMut(LoopEvent)) -> bool {
        on_event(LoopEvent::Permission {
            id: call.call_id.clone(),
            name: call.name.clone(),
            action: arg_line(&call.name, &call.arguments),
            reason: reason.into(),
        });
        let halt = self.k.halt;
        matches!(
            self.k.permits.wait(&call.call_id, self.k.cancel, &|| halt.halted()),
            Waited::Answer(PermAnswer::Allow | PermAnswer::Always)
        )
    }

    fn park(&mut self, call: &FunctionCall, class: HardClass, step: u32, args_redacted: &str) {
        let now = self.now();
        let id = format!("{PARK_PREFIX}{}-{step}", self.ep.id);
        let action = match class {
            HardClass::Credentials => {
                harness::credential_action(&serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({})))
            }
            _ => format!("{} {args_redacted}", call.name),
        };
        let req = ParkRequest {
            id: id.clone(),
            path: "E".into(),
            tool: call.name.clone(),
            action,
            class: class.as_str().into(),
            ts_ms: now,
        };
        if !self.k.parks.post(&req) {
            return;
        }
        self.ep.parks.push(OpenPark {
            id,
            step,
            tool: call.name.clone(),
            args: call.arguments.clone(),
            args_redacted: args_redacted.into(),
            class,
            parked_ms: now,
            goal_step: self.ep.goal_step.clone(),
        });
    }
}

impl Episode {
    /// Steps left before the step cap.
    fn cap_room(&self) -> usize {
        self.step_cap.saturating_sub(self.steps) as usize
    }
}

fn is_read(name: &str) -> bool {
    name == ZOOM_TOOL || gate::is_readonly(name)
}

fn zoom(view: &EpisodeView, arguments: &str) -> ToolOutput {
    let args: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
    let id = args.get("span_ref").and_then(|v| v.as_str()).unwrap_or("");
    let n = args.get("n").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
    match view.zoom(id, n) {
        Ok(text) => ToolOutput::ok(text),
        Err(e) => ToolOutput::err(e),
    }
}

/// Args as a span keeps them: typed text as its length, credentials and
/// secret-shaped values redacted, the target hint dropped.
fn arg_line(name: &str, arguments: &str) -> String {
    let args: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
    if gate::is_desktop(name) {
        desk_args(name, &args)
    } else {
        harness::redact_args(arguments)
    }
}

/// What a step line and its span keep of a result: the text, never the image.
fn result_text(out: &ToolOutput) -> String {
    let t: String = out.text.chars().take(super::STEP_RESULT_CAP).collect();
    if out.failed && !t.starts_with("failed") {
        format!("failed: {t}")
    } else {
        t
    }
}

fn usage_of(t: &CallTokens) -> Usage {
    Usage {
        input_tokens: t.input,
        output_tokens: t.out,
        reasoning_tokens: t.reasoning,
        cost_in_usd_ticks: t.cost_ticks,
        cached_tokens: t.cached,
    }
}

fn emit(on_event: &mut dyn FnMut(LoopEvent), call: &FunctionCall, status: &str, detail: &str, image: Option<String>) {
    on_event(LoopEvent::Tool {
        id: if call.call_id.is_empty() { format!("tool-{}", call.name) } else { call.call_id.clone() },
        name: call.name.clone(),
        status: status.into(),
        detail: detail.chars().take(180).collect(),
        image,
    });
}
