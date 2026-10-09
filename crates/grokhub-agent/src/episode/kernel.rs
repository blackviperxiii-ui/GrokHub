//! The episode kernel loop: observe → propose tool → gate → execute (or
//! park) → observe → claim. Each step the worker starts fresh from the
//! episode view; nothing grows a transcript.

use std::path::Path;

use serde_json::{json, Value};

use super::verify::{verify_call, verify_call_at, verify_gate, Observation, Verdict, CHECKER_UNAVAILABLE, ESCALATED_CLASS};
use super::view::{zoom_schema, EpisodeView, Folder, ZOOM_TOOL};
use super::lessons;
use super::loops::{self, LoopEntry, Outcome, REPEAT_CALL};
use super::{Episode, EpisodeEnd, OpenPark, StepShape, FANOUT_CAP, GOAL_CAP, PARK_PREFIX, REPLAN_NOTE, STALL_REPLAN, STEP_UP_STEPS};
use crate::client::{ContentPart, FunctionCall, InputItem, ModelClient, Usage};
use crate::gate::{self, Decision, Gate, PermAnswer, PermitWait, Waited};
use crate::harness::{
    self, action_loop, append_span, desk_args, done_without_criteria, hard_class, ladder_span, post_park, AccessMode,
    Finding, GateOutcome, HardClass, LadderStep, Origin, ParkRequest, Rung, Span, Step, APPROVAL_TTL, RECOVERY_TOOL,
    VERIFY_TOOL,
};
use crate::route::{call_model, CallTokens, ModelCall, BACKGROUND_EFFORT, CLASS_COMPACT, CLASS_EPISODE};
use crate::run::{HaltCheck, LoopEvent, SteerQueue};
use crate::tools::{self, DesktopOps, ToolCtx, ToolOutput};
use crate::CancelToken;
use grokhub_core::desktop_mcp::CAPTURE_FAILED_HEAD;

/// What every worker step is told, after the cabin's own system prompt.
/// Fixed text, so the prompt prefix stays cacheable.
pub const EPISODE_RULES: &str = "You are on a supervised desktop session the user is watching. \
Each step you see the episode so far, the current goal step and the latest observation. \
Call one or a few desktop tools to make progress. A step that sends, pays, deletes, types a credential or cannot be undone waits for the user's click. \
When the goal is met, reply GOAL_COMPLETE with no tool calls; an independent check decides whether it is done.";

/// Detector name on the re-plan span when the checker rejects and no
/// detector finding matches, or the checker could not run.
pub const VERIFY_REJECT: &str = "verify_reject";
/// Detector name on the spans for a checker call that could not run.
pub const CHECKER_ERROR: &str = "checker_error";

/// A fan-out read whose worker died returns this, so it can't hide.
pub const DEAD_WORKER: &str = "worker returned nothing";
/// A read past [`FANOUT_CAP`] in one fan-out.
pub const FANOUT_CAPPED: &str = "not run: one fan-out runs at most 20 reads";
const PARKED: &str = "parked: waiting for your approval";
/// Opens the reply when the worker has nothing left but a parked step.
pub const WAITING_ON_YOU: &str = "Waiting on you: ";
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
    /// The workspace's permission rules (`.grok` deny / ask), as in `prompt`.
    pub perms: Option<&'a crate::perm::Policy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EpisodeStop {
    Ended(EpisodeEnd),
    /// The 1a ladder paused: a hard-class step or a failed screenshot.
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
    /// The note that replaces the worker's last words when the episode ends
    /// unconfirmed.
    end_note: Option<String>,
    /// A loop re-planned in this batch: 1a's `action_loop` finding on the
    /// same steps is already handled.
    looped: bool,
}

/// Drive one turn of an episode. It returns when the episode ends, the
/// ladder pauses, the worker waits on the user, or it errors. No step count
/// or wall time stops it. Call again with the same `ep` and `view`
/// for the next turn.
pub fn run_episode(
    k: &KernelIn<'_>,
    ep: &mut Episode,
    view: &mut EpisodeView,
    on_event: &mut dyn FnMut(LoopEvent),
) -> EpisodeOut {
    let mut run = Run { k, ep, view, usage: Usage::default(), tokens: None, obs: None, end_note: None, looped: false };
    if run.ep.trail.is_empty() {
        let goal = format!("goal: {}", run.ep.goal.chars().take(GOAL_CAP).collect::<String>());
        run.write(run.ep.marker("begin", "open", &goal));
        run.recall_lessons();
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
        if run.obs.is_none() {
            run.obs = run.observe();
        }
        let turn = match run.ask_worker() {
            Ok(turn) => turn,
            // Stop or Halt mid-call: the top of the loop ends the episode
            // and denies its parks.
            Err(_) if k.halt.halted() || k.cancel.is_cancelled() => continue,
            Err(err) => return run.out(EpisodeStop::Error(err), String::new()),
        };
        on_event(LoopEvent::Usage(run.usage.clone()));
        if turn.calls.is_empty() {
            match run.claim(&turn.text) {
                Some(stop) => {
                    let reply = run.end_note.take().unwrap_or(turn.text);
                    if !reply.trim().is_empty() {
                        on_event(LoopEvent::Text(reply.clone()));
                    }
                    return run.out(stop, reply);
                }
                None => continue,
            }
        }
        run.batch(&turn.calls, on_event);
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
        span.goal_step = span.goal_step.chars().take(GOAL_CAP).collect();
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
        let failed = shape.decision == "deny" || shape.result.starts_with("failed");
        let stalled = self.ep.note_progress(&shape.decision, shape.ui_changed, failed);
        let looped = (shape.decision != "park")
            .then(|| {
                let outcome = match (failed, shape.ui_changed) {
                    (true, _) => Outcome::Failed,
                    (false, Some(false)) => Outcome::Still,
                    _ => Outcome::Moved,
                };
                loops::note_call(&mut self.ep.ring, LoopEntry::new(&shape.tool, args_redacted, outcome, &shape.result))
            })
            .flatten();
        if let Some(reason) = looped {
            self.ep.stall = 0;
            self.looped = true;
            self.replan(REPEAT_CALL, reason);
        } else if stalled {
            self.replan("no_progress", format!("no_progress: {STALL_REPLAN} steps in a row changed nothing"));
        }
    }

    /// A loop or [`STALL_REPLAN`] steps that moved nothing: a quiet repair
    /// span and a re-plan note for the next step. No card, nothing counted,
    /// no pause.
    fn replan(&mut self, detector: &str, reason: String) {
        let step = LadderStep {
            rung: Rung::Replan,
            detector: detector.into(),
            target: format!("episode#{}", self.ep.id),
            hard: None,
            evidence: Vec::new(),
            reason,
            prompt: Some(REPLAN_NOTE.into()),
        };
        let span = ladder_span(&self.ep.chat_id, &step);
        self.write(span);
        self.ep.note = Some(REPLAN_NOTE.into());
        self.ep.step_up = STEP_UP_STEPS;
    }

    /// A screenshot through the same gate as any step. Not a step itself;
    /// `None` when the gate would not run it without asking.
    fn observe(&self) -> Option<Observation> {
        if self.k.halt.halted() || self.k.desktop.halted() {
            return None;
        }
        let desk = tools::desk_flags("screenshot", &self.k.gate, Some(self.k.desktop));
        match gate::decide_with(&self.k.gate, "screenshot", "{}", false, desk, self.k.workspace, self.k.perms) {
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
        for park in self.ep.parks.iter().filter(|p| p.waiting) {
            now.push_str(&waiting_line(park));
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
            .with_tools(self.worker_tools())
            .in_session(&self.ep.chat_id);
        call.provider = self.k.provider.into();
        // After a re-plan the router goes one model and one rung up for a few
        // calls, then the episode is back on its own model. No prompt.
        call.step_up = self.ep.step_up > 0;
        let routed = call_model(self.k.client, &call, self.k.cancel).map_err(|e| e.to_string())?;
        if call.step_up {
            if self.ep.step_up == STEP_UP_STEPS {
                let t = &routed.tokens;
                let note = format!("stepped up to {} {} for {STEP_UP_STEPS} steps", t.model, t.effort);
                self.write(self.ep.marker("step_up", &t.model, note.trim()));
            }
            self.ep.step_up -= 1;
        }
        self.usage.add(&routed.out.usage);
        self.tokens = Some(routed.tokens);
        self.ep.note = None;
        Ok(routed.out)
    }

    /// No tools: a claim. `GOAL_COMPLETE` goes to VerifyGate; anything else
    /// waits on the user. `None` means the worker goes on: a reject re-plans
    /// with the checker's reason (through the ladder when a detector matches)
    /// and never waits. The same reject on an unchanged screen
    /// [`super::SAME_REJECT_END`] times ends the episode with a named note.
    fn claim(&mut self, text: &str) -> Option<EpisodeStop> {
        let mut reply = Span::reply(&self.ep.chat_id, text, self.k.held);
        reply.tokens = self.tokens.take();
        if !self.ep.parks.is_empty() {
            // A parked step is part of the goal: no check yet. The episode
            // waits (it doesn't end) and the reply names what it waits on.
            self.write(reply);
            self.end_note = Some(parked_reply(text, &self.ep.parks));
            return Some(EpisodeStop::Waiting);
        }
        if !grokhub_core::verify::has_goal_complete(text) {
            self.write(reply);
            return Some(EpisodeStop::Waiting);
        }
        let fin = self.observe().unwrap_or_else(|| Observation {
            text: "no observation: the gate did not allow a screenshot".into(),
            ..Observation::default()
        });
        self.write(reply);
        let (verdict, tokens) = self.check(&fin);
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
                if self.ep.note_reject(&why, &fin.hash) {
                    return Some(self.unconfirmed(&why));
                }
                if why == CHECKER_UNAVAILABLE {
                    return self.verify_replan(&why);
                }
                match done_without_criteria(&self.ep.trail).pop() {
                    Some(finding) => self.ladder(finding, Some(&why)),
                    None => self.verify_replan(&why),
                }
            }
        }
    }

    /// VerifyGate with fallbacks. A call that can't run is tried once more,
    /// then once up the route ladder ([`ESCALATED_CLASS`]); each
    /// retry writes a quiet `checker_error` span. Still nothing: a reject
    /// with [`CHECKER_UNAVAILABLE`].
    fn check(&mut self, fin: &Observation) -> (Verdict, Option<CallTokens>) {
        for i in 0..3 {
            let call = match i {
                2 => verify_call_at(self.k.model, ESCALATED_CLASS, self.k.effort, &self.ep.goal, fin, &self.ep.id),
                _ => verify_call(self.k.model, &self.ep.goal, fin, &self.ep.id),
            };
            match verify_gate(self.k.client, &call, self.k.cancel) {
                Ok((v, t)) => {
                    self.usage.add(&usage_of(&t));
                    return (v, Some(t));
                }
                Err(_) if self.k.cancel.is_cancelled() || self.k.halt.halted() => break,
                Err(e) => {
                    let next = match i {
                        0 => "retry",
                        1 => "escalate",
                        _ => continue,
                    };
                    let args = json!({"detector": CHECKER_ERROR, "target": VERIFY_TOOL}).to_string();
                    let mut span = Span::deny(&self.ep.chat_id, RECOVERY_TOOL, &args, next, "soft");
                    span.decision = next.into();
                    span.claim = format!("{CHECKER_ERROR}: {}", e.to_string().chars().take(200).collect::<String>());
                    self.write(span.from_origin(Origin::Repair));
                }
            }
        }
        (Verdict::Reject(CHECKER_UNAVAILABLE.into()), None)
    }

    /// The checker rejected with no detector finding, or could not run: a
    /// quiet re-plan span and the re-plan note with its reason for the next
    /// step. No card, no pause.
    fn verify_replan(&mut self, why: &str) -> Option<EpisodeStop> {
        let step = LadderStep {
            rung: Rung::Replan,
            detector: VERIFY_REJECT.into(),
            target: format!("{VERIFY_TOOL}#{}", self.ep.id),
            hard: None,
            evidence: Vec::new(),
            reason: format!("{VERIFY_REJECT}: {why}"),
            prompt: Some(REPLAN_NOTE.into()),
        };
        let span = ladder_span(&self.ep.chat_id, &step);
        self.write(span);
        self.ep.note = Some(format!("{REPLAN_NOTE} The independent check said: {why}"));
        self.ep.step_up = STEP_UP_STEPS;
        None
    }

    /// The same reject on an unchanged screen too many times: end with a
    /// note naming the goal and the reason. No card, no pause.
    fn unconfirmed(&mut self, why: &str) -> EpisodeStop {
        let goal: String = self.ep.goal.chars().take(GOAL_CAP).collect();
        self.end_note = Some(format!("Couldn't confirm: {goal} — checker says {why}"));
        self.end(EpisodeEnd::Unconfirmed);
        EpisodeStop::Ended(EpisodeEnd::Unconfirmed)
    }

    /// One ladder rung for a finding: a repair note for the next step, or a pause.
    fn ladder(&mut self, finding: Finding, why: Option<&str>) -> Option<EpisodeStop> {
        // E1: a detector finding lifts the worker's next step one rung.
        crate::route::ladder::note_tool_error();
        let step = self.ep.ladder.next(&finding, &self.ep.trail);
        let span = ladder_span(&self.ep.chat_id, &step);
        self.write(span);
        if step.rung == Rung::Replan {
            self.ep.step_up = STEP_UP_STEPS;
        }
        match step.rung {
            Rung::Retry | Rung::Backtrack | Rung::Replan => {
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
        if std::mem::take(&mut self.looped) {
            for f in action_loop(&self.ep.trail) {
                let key = finding_key(&f);
                if !self.ep.seen_findings.contains(&key) {
                    self.ep.seen_findings.push(key);
                }
            }
            return None;
        }
        let finding = action_loop(&self.ep.trail).into_iter().find(|f| !self.ep.seen_findings.contains(&finding_key(f)))?;
        self.ep.seen_findings.push(finding_key(&finding));
        self.ladder(finding, None)
    }

    fn end(&mut self, why: EpisodeEnd) {
        self.ep.ended = Some(why);
        self.learn(why);
        let header = self.ep.header(self.now());
        let span = self.ep.marker("end", why.as_str(), &header);
        self.write(span);
    }

    /// The best past lessons for this goal go at the top of the view, and a
    /// quiet `lesson_used` marker names each one.
    fn recall_lessons(&mut self) {
        let (block, used) = lessons::past_lessons(&lessons::load(self.k.config_dir), &self.ep.goal, self.k.held);
        self.view.set_lessons(&block);
        for l in used {
            let claim = format!("Used lesson: {}", lessons::lesson_line(&l));
            self.write(self.ep.marker("lesson_used", &l.episode_id, &claim));
            self.ep.used_lessons.push(l.episode_id);
        }
    }

    /// An episode that had to recover writes one lesson, and a quiet
    /// `lesson` marker names it. A trivial run writes nothing. Lessons it
    /// started with count a failure when it still didn't finish.
    fn learn(&mut self, why: EpisodeEnd) {
        let _ = lessons::note_used(self.k.config_dir, &self.ep.used_lessons, why);
        let Some(lesson) = lessons::derive(self.ep, why, self.now(), self.k.held) else {
            return;
        };
        let stored = lessons::append(self.k.config_dir, &lesson).is_ok();
        let line = lessons::lesson_line(&lesson);
        let span = self.ep.marker("lesson", if stored { "saved" } else { "not saved" }, &line);
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

    /// Answered parks run once or are denied. One past the TTL is denied for
    /// now (not run) and keeps waiting: a late Approve still runs it once.
    fn settle_parks(&mut self, on_event: &mut dyn FnMut(LoopEvent)) {
        let now = self.now();
        for mut park in std::mem::take(&mut self.ep.parks) {
            let answer = self.k.parks.answer(&park.id);
            let expired = now.saturating_sub(park.parked_ms) >= APPROVAL_TTL.as_millis() as u64;
            let deny = match answer {
                Some(true) => None,
                Some(false) => Some("denied".to_string()),
                None => {
                    if expired && !park.waiting {
                        // Fail-closed for this step only: it is not run, the
                        // card stays, and the rest of the goal goes on.
                        park.waiting = true;
                        let shape = StepShape {
                            goal_step: park.goal_step.clone(),
                            tool: park.tool.clone(),
                            decision: "deny".into(),
                            ui_changed: None,
                            result: timed_out(&park),
                        };
                        self.record(&shape, &park.args_redacted, park.class.as_str(), &format!("s{}w", park.step));
                    }
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
                    if out.failed {
                        crate::route::ladder::note_tool_error();
                    }
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
    fn batch(&mut self, calls: &[FunctionCall], on_event: &mut dyn FnMut(LoopEvent)) {
        // Only reads every gate allows fan out; a denied or asked read takes
        // the one-at-a-time path, which answers it.
        let reads = calls.len() > 1
            && calls.iter().all(|c| is_read(&c.name) && matches!(self.verdict(c), GateOutcome::Allow));
        if reads {
            self.fan(calls, on_event);
            return;
        }
        for (i, call) in calls.iter().enumerate() {
            if self.k.halt.halted() || self.k.cancel.is_cancelled() {
                return;
            }
            if !self.step(call, on_event) {
                for rest in &calls[i + 1..] {
                    emit(on_event, rest, "failed", gate::NOT_EXECUTED, None);
                }
                break;
            }
        }
    }

    fn fan(&mut self, calls: &[FunctionCall], on_event: &mut dyn FnMut(LoopEvent)) {
        let take = calls.len().min(FANOUT_CAP);
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
            if out.failed {
                crate::route::ladder::note_tool_error();
            }
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
        if out.failed {
            crate::route::ladder::note_tool_error();
        }
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
        match gate::decide_with(&self.k.gate, &call.name, &call.arguments, false, desk, self.k.workspace, self.k.perms) {
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
            action: action.clone(),
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
            action,
            class,
            parked_ms: now,
            goal_step: self.ep.goal_step.clone(),
            waiting: false,
        });
    }
}

/// The step span's result when a park passes the approval TTL unanswered.
fn timed_out(park: &OpenPark) -> String {
    format!(
        "hard-class {} unanswered after {}s — not run; waiting on your card",
        park.class.as_str(),
        APPROVAL_TTL.as_secs()
    )
}

/// The worker's line, each step, for a park past the TTL.
fn waiting_line(park: &OpenPark) -> String {
    format!(
        "Waiting on the user: `{}` (step {}) runs only after they approve its card. \
Do the other parts of the goal that don't depend on it; don't retry it.\n",
        park.action, park.step
    )
}

/// The reply when the worker stops with steps still parked: what it said
/// (without the done marker) and the steps it waits on.
fn parked_reply(said: &str, parks: &[OpenPark]) -> String {
    let said: Vec<&str> = said
        .lines()
        .filter(|l| !l.trim().starts_with("GOAL_COMPLETE") && !l.trim().starts_with("VERIFY_OK"))
        .collect();
    let names: Vec<String> = parks.iter().map(|p| format!("\u{201c}{}\u{201d}", p.action)).collect();
    let card = if parks.len() == 1 { "its card" } else { "their cards" };
    let waiting = format!("{WAITING_ON_YOU}{}. Approve {card} and I'll run it once and go on.", names.join("; "));
    let said = said.join("\n");
    if said.trim().is_empty() {
        waiting
    } else {
        format!("{}\n\n{waiting}", said.trim())
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

/// One `action_loop` finding, by detector and its first step.
fn finding_key(f: &Finding) -> String {
    format!("{}:{}", f.detector, f.evidence.first().map_or("", |e| e.span.as_str()))
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

/// What a step line and its span keep of a result: the text, never the
/// image. A "no capture backend works" note is kept whole so the pause names
/// every backend's error.
fn result_text(out: &ToolOutput) -> String {
    let cap = if out.text.contains(CAPTURE_FAILED_HEAD) { super::CAPTURE_NOTE_CAP } else { super::STEP_RESULT_CAP };
    let t: String = out.text.chars().take(cap).collect();
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
