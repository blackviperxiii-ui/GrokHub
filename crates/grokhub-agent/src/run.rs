//! Agent loop. Turns stop when the model stops calling tools and the goal
//! checklist has nothing open ([`crate::drive`]), or at the caller's turn cap
//! (`max_turns` 0 means none).

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::gate::{self, Decision, Gate, PermAnswer, PermitWait, Waited};
use crate::tasks::TaskHub;
use crate::tools::{self, DesktopOps, ToolCtx, ToolOutput, ToolSet};
use crate::{
    CancelToken, ClientError, ContentPart, FunctionCall, InputItem, ModelClient, ResponsesRequest,
    StreamEvent, Usage,
};

/// Per-session [`ToolSet`] for background / unattended runs (SP2).
///
/// Callers bind before `run_loop` / `NativeEngine::prompt`; `None` on a session
/// keeps today's full `schemas_for` / `dispatch` behavior. Stored here so app
/// callers do not need a `LoopIn` field (that would force edits across eval and
/// chat engine construction sites owned by other cards).
static JOB_TOOL_SETS: OnceLock<Mutex<HashMap<String, ToolSet>>> = OnceLock::new();

fn job_tool_sets() -> &'static Mutex<HashMap<String, ToolSet>> {
    JOB_TOOL_SETS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// RAII token from [`bind_job_tool_set`]. Dropping it does **not** clear the
/// bind: a `background: true` `spawn_subagent` child (`parent/id`) can outlive
/// the parent `run_loop`, and removing the parent entry would fail open to the
/// full tool list. Overwrite with a later bind on the same session id, or call
/// [`clear_job_tool_set`].
pub struct JobToolSetGuard {
    session: String,
}

impl Drop for JobToolSetGuard {
    fn drop(&mut self) {
        let _ = &self.session;
    }
}

/// Bind `set` for `session`. Overwrites a prior bind. Survives the returned
/// guard so detached subagents keep the allowlist. Clears any previously
/// materialized `session/…` descendants so a tighter rebind wins on next lookup.
pub fn bind_job_tool_set(session: impl Into<String>, set: ToolSet) -> JobToolSetGuard {
    let session = session.into();
    if let Ok(mut map) = job_tool_sets().lock() {
        let prefix = format!("{session}/");
        map.retain(|key, _| !key.starts_with(&prefix));
        map.insert(session.clone(), set);
    }
    JobToolSetGuard { session }
}

/// Remove a session's job [`ToolSet`] (tests / explicit teardown).
pub fn clear_job_tool_set(session: &str) {
    if let Ok(mut map) = job_tool_sets().lock() {
        map.remove(session);
    }
}

fn bound_job_tool_set(session: &str) -> Option<ToolSet> {
    let Ok(mut map) = job_tool_sets().lock() else {
        return None;
    };
    if let Some(set) = map.get(session) {
        return Some(set.clone());
    }
    // Child sessions are `parent/id` (nested `parent/id/id2`). Inherit the
    // nearest bound ancestor so spawn_subagent keeps the job allowlist.
    // Materialize onto the child so a later parent clear still leaves the set.
    let mut rest = session;
    while let Some((parent, _)) = rest.rsplit_once('/') {
        if let Some(set) = map.get(parent).cloned() {
            map.insert(session.to_string(), set.clone());
            return Some(set);
        }
        rest = parent;
    }
    None
}

const REPEAT_LIMIT: u32 = 3;
/// What a call repeated [`REPEAT_LIMIT`] times gets back instead of running.
pub const REPEAT_REPLAN: &str = "not run: the same call already ran twice. Stop repeating it. \
Re-read the goal, say in one line what you tried, then pick a different approach \
(another tool, another route or a smaller sub-step) and go on.";
/// Tool calls from a length-truncated model reply are failed unexecuted.
pub const TRUNCATED_TOOL: &str =
    "not run: your reply was cut off at the output limit; resend smaller steps";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    Cancelled,
    Halted,
    MaxTurns,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopEvent {
    Text(String),
    Thought(String),
    Tool {
        id: String,
        name: String,
        status: String,
        detail: String,
        image: Option<String>,
    },
    Usage(Usage),
    /// Context meter for the usage event that follows.
    Meter {
        used: u64,
        limit: u64,
    },
    Compact {
        started: bool,
        usage: Usage,
        error: Option<String>,
    },
    Permission {
        id: String,
        name: String,
        action: String,
        reason: String,
    },
    Elicit(crate::mcp::ElicitView),
    /// A background command, monitor, or subagent row for the existing Tasks list.
    Task {
        id: String,
        title: String,
        done: bool,
    },
    /// Plan text for the existing plan card. `replace` is always true on the wire.
    Plan(String),
    /// A findings card body ([`grokhub_core::findings::Findings::to_body`]),
    /// shown after the reply.
    Findings(String),
}

#[derive(Clone)]
pub struct SteerQueue {
    inner: Arc<Mutex<VecDeque<String>>>,
}

impl SteerQueue {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn push(&self, text: impl Into<String>) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(text);
    }

    pub fn drain(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect()
    }
}

impl Default for SteerQueue {
    fn default() -> Self {
        Self::new()
    }
}

pub trait HaltCheck {
    fn halted(&self) -> bool;
}

pub struct LoopIn<'a> {
    pub client: &'a dyn ModelClient,
    pub workspace: &'a Path,
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub system: &'a str,
    pub conversation_id: &'a str,
    pub max_turns: u32,
    /// Already counted this session. Events and the returned total include it.
    pub usage_base: Usage,
    pub cancel: &'a CancelToken,
    pub steer: &'a SteerQueue,
    pub halt: &'a dyn HaltCheck,
    pub gate: Gate,
    pub desktop: Option<&'a dyn DesktopOps>,
    pub permits: &'a dyn PermitWait,
    /// `None` keeps gate v0. The native engine passes the loaded policy.
    pub perms: Option<&'a crate::perm::Policy>,
    /// Model context window. `0` disables auto-compact.
    pub context_length: u64,
    /// Background commands and monitors for this session. `None` in tests that do not spawn.
    pub tasks: Option<Arc<TaskHub>>,
    /// 0 is the root run. A child of a child is 2 and cannot spawn.
    pub depth: u32,
    /// Set on a subagent so commands it starts die with it.
    pub agent_id: Option<&'a str>,
    /// Shared with worker threads. `None` refuses `spawn_subagent`.
    pub shared_client: Option<Arc<dyn ModelClient + Send + Sync>>,
    pub shared_permits: Option<Arc<dyn PermitWait + Send + Sync>>,
    pub shared_desktop: Option<Arc<dyn DesktopOps + Send + Sync>>,
}

pub struct LoopOut {
    pub stop: StopReason,
    pub usage: Usage,
    /// A summary replaced older history. The caller records a compaction marker.
    pub compacted: bool,
}

fn finish(
    input: &LoopIn<'_>,
    stop: StopReason,
    usage: Usage,
    compacted: bool,
    fire: bool,
) -> LoopOut {
    if fire {
        let event = match &stop {
            StopReason::EndTurn => "Stop",
            StopReason::Error(_) => "StopFailure",
            StopReason::Cancelled | StopReason::Halted | StopReason::MaxTurns => "StopCancelled",
        };
        let _ = crate::hooks::on_stop(input.conversation_id, input.workspace, event, true);
    }
    LoopOut {
        stop,
        usage,
        compacted,
    }
}

enum CompactPass {
    Ok,
    Cancelled,
    Failed,
}

fn run_compact_pass(
    input: &LoopIn<'_>,
    history: &mut Vec<InputItem>,
    modified_paths: &[String],
    usage: &mut Usage,
    on_event: &mut dyn FnMut(LoopEvent),
) -> CompactPass {
    on_event(LoopEvent::Compact {
        started: true,
        usage: usage.clone(),
        error: None,
    });
    crate::hooks::on_compact(input.conversation_id, input.workspace, true);
    let compact_result = crate::compact::compact_after_flush(
        input.workspace,
        input.client,
        input.cancel,
        input.model,
        input.effort,
        input.conversation_id,
        history,
        modified_paths,
    );
    crate::hooks::on_compact(input.conversation_id, input.workspace, false);
    match compact_result {
        Ok(extra) => {
            crate::prefix_guard::note_compaction(&crate::prefix_guard::run_key(
                input.conversation_id,
                input.depth,
                input.agent_id,
            ));
            usage.add(&extra);
            on_event(LoopEvent::Compact {
                started: false,
                usage: usage.clone(),
                error: None,
            });
            emit_usage(on_event, usage, history, input.context_length);
            CompactPass::Ok
        }
        Err(crate::compact::CompactError::Cancelled) => {
            on_event(LoopEvent::Compact {
                started: false,
                usage: usage.clone(),
                error: Some("cancelled".into()),
            });
            CompactPass::Cancelled
        }
        Err(crate::compact::CompactError::Failed(message)) => {
            on_event(LoopEvent::Compact {
                started: false,
                usage: usage.clone(),
                error: Some(message),
            });
            CompactPass::Failed
        }
    }
}

pub fn run_loop(
    input: &LoopIn<'_>,
    history: &mut Vec<InputItem>,
    user_text: &str,
    image: Option<&str>,
    on_event: &mut dyn FnMut(LoopEvent),
) -> LoopOut {
    let first_turn = history.is_empty();
    // Brain 5: the turn's talk style sits in the system prompt's volatile tail.
    let style = (input.depth == 0)
        .then(|| crate::checkpoint::turn_style(input.conversation_id))
        .flatten();
    // Brain 3: so does the playbook the turn's Brief picked.
    let playbook = (input.depth == 0)
        .then(|| crate::checkpoint::turn_playbook(input.conversation_id))
        .flatten();
    let config_dir = crate::perm::config_dir();
    let soft_caps = crate::brain::care_first_enabled(&config_dir);
    let intention_block = (input.depth == 0)
        .then(|| crate::close_check::prompt_block(input.conversation_id))
        .filter(|block| !block.is_empty());
    // Plan→skill wiring: ship/QA, verify-before-done, multi-step follow-through.
    let plan_skill_block = (input.depth == 0)
        .then(|| {
            let todos = crate::session_tools::todos_for(input.conversation_id);
            let plan_text: String = todos
                .iter()
                .map(|t| t.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let pending = todos
                .iter()
                .filter(|t| matches!(t.status.as_str(), "pending" | "in_progress"))
                .count();
            let ctx = crate::plan_skills::PlanSkillCtx {
                plan_text: &plan_text,
                change_text: user_text,
                pending_steps: pending,
                // Close-check owns the marking-done gate; the standing reminder
                // covers verify-before-done for open turns.
                marking_done: false,
            };
            crate::plan_skills::prompt_block(&ctx)
        })
        .filter(|block| !block.is_empty());
    tools::begin_deferred_turn(input.conversation_id);
    scrub_turn_labels(history);
    if crate::prefix_cache::enabled(&config_dir) {
        if first_turn {
            crate::prefix_cache::push_stable_system(history, input.system);
        }
        let already = crate::memory::injected_lines(history);
        let volatile_intentions = match (intention_block.clone(), plan_skill_block.clone()) {
            (Some(a), Some(b)) => Some(format!("{a}\n\n{b}")),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        let volatile = crate::prefix_cache::TurnVolatile::assemble(
            input.workspace,
            user_text,
            first_turn,
            style,
            soft_caps,
            playbook.as_deref(),
            &already,
            volatile_intentions,
        );
        crate::prefix_cache::push_user_turn(history, &volatile, user_text, image);
    } else {
        if first_turn {
            let base = style.map(|s| grokhub_core::talk::restyle_caps(input.system, s, soft_caps));
            let base = base.as_deref().unwrap_or(input.system);
            let base = match &playbook {
                Some(block) => grokhub_core::playbook::with_playbook(base, Some(block)),
                None => base.to_string(),
            };
            let system = match crate::memory::first_turn_injection(input.workspace, user_text) {
                Some(block) if base.trim().is_empty() => block,
                Some(block) => format!("{base}\n\n{block}"),
                None => base.to_string(),
            };
            if !system.trim().is_empty() {
                history.push(InputItem::Message {
                    role: "system".into(),
                    content: vec![ContentPart::InputText(system)],
                });
            }
        } else {
            if let Some(style) = style {
                restyle_history(history, style, soft_caps);
            }
            if input.depth == 0 {
                replaybook_history(history, playbook.as_deref());
            }
            // Later turns add recall only for lines this history has not seen.
            let already = crate::memory::injected_lines(history);
            if let Some(block) =
                crate::memory::followup_injection(input.workspace, user_text, &already)
            {
                history.push(InputItem::Message {
                    role: "system".into(),
                    content: vec![ContentPart::InputText(block)],
                });
            }
        }
        if let Some(block) = intention_block {
            push_intention_block(history, &block);
        }
        if let Some(block) = plan_skill_block.as_deref() {
            push_intention_block(history, block);
        }
        history.push(user_message(user_text, image));
    }
    // Spike-4c: a tool call that carries a recalled line is personal egress.
    let _recall = crate::harness::RecallScope::enter(crate::memory::recall_lines(history));
    input.permits.drain();
    let mut usage = input.usage_base.clone();
    let base_readonly = input.gate.readonly_session;
    let plan_at_start = crate::session_tools::plan_on(input.conversation_id);
    let mut gate = input.gate;
    if plan_at_start {
        gate.readonly_session = true;
    }
    let mut compacted_once = false;
    let mut turns_since_compact = 0u32;
    let mut overflow_compacted = false;
    let mut modified_paths: Vec<String> = Vec::new();
    let mut nested_agents = crate::prompt::NestedAgents::new(input.workspace);
    let mut repeats: HashMap<String, u32> = HashMap::new();
    let mut stop_hook_active = false;
    let mut truncated_streak = 0u32;
    // A subagent's checklist belongs to its parent; plan mode can't do the work.
    let mut drive = (input.depth == 0 && !plan_at_start && !input.conversation_id.is_empty())
        .then(|| crate::drive::Drive::new(&crate::session_tools::todos_for(input.conversation_id)));
    let mut evidence = 0usize;
    let mut close_continued = false;
    let mut close_steps: Vec<String> = Vec::new();
    let mut close_surfaced = false;
    // Brain 2: the turn's Brief and the changes still waiting on a check.
    let brief = drive
        .as_ref()
        .and_then(|_| crate::checkpoint::turn_brief(input.conversation_id));
    let mut fresh = crate::checkpoint::Fresh::default();
    let (mut checkpoint_at, mut checkpoint_due) = (0usize, Vec::<String>::new());
    // Brain 5: a long run in a progress style gets a note at each phase boundary.
    let mut progress = drive
        .as_ref()
        .filter(|_| style.is_some_and(grokhub_core::talk::TalkStyle::progress_notes))
        .map(|_| {
            crate::checkpoint::Progress::new(&crate::session_tools::todos_for(
                input.conversation_id,
            ))
        });
    match crate::hooks::on_user_prompt(input.conversation_id, input.workspace, user_text) {
        crate::hooks::PromptHook::Block { reason } => {
            history.pop();
            return finish(
                input,
                StopReason::Error(format!("Prompt blocked by hook: {reason}")),
                usage,
                compacted_once,
                true,
            );
        }
        crate::hooks::PromptHook::Continue { context } if !context.is_empty() => {
            if let Some(InputItem::Message { content, .. }) = history.last_mut() {
                if let Some(ContentPart::InputText(text)) = content.first_mut() {
                    text.push('\n');
                    text.push_str(&context);
                }
            }
        }
        crate::hooks::PromptHook::Continue { .. } => {}
    }
    let mut step_ended: Option<std::time::Instant> = None;
    let mut turns = 0u32;
    let guard_key =
        crate::prefix_guard::run_key(input.conversation_id, input.depth, input.agent_id);
    loop {
        if input.max_turns != 0 && turns >= input.max_turns {
            return finish(input, StopReason::MaxTurns, usage, compacted_once, true);
        }
        turns = turns.saturating_add(1);
        let turn_top = crate::timing::lap("loop:pre_request");
        if input.cancel.is_cancelled() {
            return finish(input, StopReason::Cancelled, usage, compacted_once, true);
        }
        if stop_for_halt(input) {
            return finish(input, StopReason::Halted, usage, compacted_once, true);
        }
        inject_notices(input, history);
        if let Some(tasks) = &input.tasks {
            let extra = tasks.take_usage();
            if usage_pending(&extra) {
                usage.add(&extra);
                emit_usage(on_event, &usage, history, input.context_length);
            }
        }
        // Compact when the context fills (or before a long-context price step).
        // Re-arm after at least one new turn since the last successful compact.
        let long = crate::route::live::crosses_long_context(
            &crate::perm::config_dir(),
            input.model,
            crate::compact::estimate_input_tokens(history),
        );
        let over = crate::compact::needs_auto_compact(history, input.context_length) || long;
        let may_compact = over && (!compacted_once || turns_since_compact >= 1);
        if may_compact {
            match run_compact_pass(input, history, &modified_paths, &mut usage, on_event) {
                CompactPass::Ok => {
                    compacted_once = true;
                    turns_since_compact = 0;
                }
                CompactPass::Cancelled => {
                    return finish(input, StopReason::Cancelled, usage, compacted_once, true);
                }
                CompactPass::Failed => {}
            }
        }
        let mut wire = history.clone();
        crate::image_budget::apply_image_budget(&mut wire);
        let tools = match bound_job_tool_set(input.conversation_id) {
            Some(set) => tools::schemas_for_job(&gate, &set),
            None => tools::schemas_for(&gate),
        };
        let req = ResponsesRequest {
            model: input.model.to_string(),
            effort: input.effort.map(str::to_string),
            input: wire,
            conversation_id: input.conversation_id.to_string(),
            tools,
            hosted_search: true,
            call_timeout: None,
        };
        // Each request should extend the last one; a rewrite logs its first changed segment.
        crate::prefix_guard::observe(&guard_key, &req);
        let class = crate::route::live::current_class();
        drop(turn_top);
        if let Some(t) = step_ended {
            crate::timing::record("loop:step_gap", t.elapsed());
        }
        let turn = match crate::route::live::stream_routed(
            input.client,
            &req,
            input.cancel,
            &mut |ev| match ev {
                StreamEvent::TextDelta(text) => on_event(LoopEvent::Text(text)),
                StreamEvent::ReasoningDelta(text) => on_event(LoopEvent::Thought(text)),
            },
            class,
        ) {
            Ok(turn) => {
                overflow_compacted = false;
                turn
            }
            Err(ClientError::Cancelled) => {
                return finish(input, StopReason::Cancelled, usage, compacted_once, true);
            }
            Err(err) if err.is_context_length() => {
                if overflow_compacted {
                    return finish(
                        input,
                        StopReason::Error(crate::compact::CONTEXT_STILL_TOO_LARGE.into()),
                        usage,
                        compacted_once,
                        true,
                    );
                }
                match run_compact_pass(input, history, &modified_paths, &mut usage, on_event) {
                    CompactPass::Ok => {
                        compacted_once = true;
                        turns_since_compact = 0;
                        overflow_compacted = true;
                        continue;
                    }
                    CompactPass::Cancelled => {
                        return finish(input, StopReason::Cancelled, usage, compacted_once, true);
                    }
                    CompactPass::Failed => {
                        return finish(
                            input,
                            StopReason::Error(err.to_string()),
                            usage,
                            compacted_once,
                            true,
                        );
                    }
                }
            }
            Err(err) => {
                return finish(
                    input,
                    StopReason::Error(err.to_string()),
                    usage,
                    compacted_once,
                    true,
                );
            }
        };
        step_ended = Some(std::time::Instant::now());
        turns_since_compact = turns_since_compact.saturating_add(1);
        usage.add(&turn.usage);
        emit_usage(on_event, &usage, history, input.context_length);
        // Close-check reads this after the reply is stored on the history.
        let last_assistant = turn.text.clone();
        if !turn.text.is_empty() {
            history.push(InputItem::Message {
                role: "assistant".into(),
                content: vec![ContentPart::InputText(grokhub_core::strip_turn_labels(
                    &turn.text,
                ))],
            });
        }
        if turn.truncated {
            truncated_streak = truncated_streak.saturating_add(1);
            record_turn_truncated(
                input.conversation_id,
                turn.truncate_reason.as_deref(),
                truncated_streak,
            );
            for call in &turn.calls {
                history.push(InputItem::FunctionCall {
                    call_id: call.call_id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                });
                emit_tool(
                    on_event,
                    &tool_id(call),
                    call,
                    "failed",
                    TRUNCATED_TOOL,
                    None,
                );
                // Do not note a tool error: truncation is not a failed tool run.
                history.push(InputItem::FunctionCallOutput {
                    call_id: call.call_id.clone(),
                    output: TRUNCATED_TOOL.into(),
                });
            }
            continue;
        }
        truncated_streak = 0;
        if turn.calls.is_empty() {
            let notes = input.steer.drain();
            if notes.is_empty() {
                let mut drive_blocked = false;
                if let Some(drive) = drive
                    .as_mut()
                    .filter(|_| !crate::session_tools::plan_on(input.conversation_id))
                {
                    let todos = crate::session_tools::todos_for(input.conversation_id);
                    let gaps =
                        crate::checkpoint::Gaps::of(brief.as_ref(), &todos, evidence, &fresh);
                    match drive.check_with(&todos, evidence, &gaps) {
                        crate::drive::Verdict::Push(note) => {
                            history.push(user_message(&note, None));
                            continue;
                        }
                        crate::drive::Verdict::Blocked(text) => {
                            drive_blocked = true;
                            on_event(LoopEvent::Text(format!("\n\n{text}")));
                            history.push(InputItem::Message {
                                role: "assistant".into(),
                                content: vec![ContentPart::InputText(text)],
                            });
                            // Brain 5: a repair or research run still ends with its findings card.
                            if let Some(card) = gaps
                                .findings
                                .then(|| crate::checkpoint::fallback_findings(&todos, &gaps))
                                .flatten()
                            {
                                on_event(LoopEvent::Findings(card.to_body()));
                            }
                        }
                        crate::drive::Verdict::End => {
                            let change = brief.as_ref().is_some_and(|b| {
                                matches!(
                                    b.intent,
                                    crate::brain::Intent::Task | crate::brain::Intent::Repair
                                )
                            });
                            if let Some(text) = change
                                .then(|| crate::drive::unbacked_done(&last_assistant, evidence))
                                .flatten()
                            {
                                on_event(LoopEvent::Text(format!("\n\n{text}")));
                                history.push(InputItem::Message {
                                    role: "assistant".into(),
                                    content: vec![ContentPart::InputText(text.into())],
                                });
                            }
                        }
                    }
                }
                if close_continued {
                    if !close_surfaced
                        && crate::close_check::lists_still_open(input.conversation_id)
                    {
                        if let Some(body) = crate::close_check::steps_card(&close_steps) {
                            on_event(LoopEvent::Findings(body));
                        }
                    }
                    close_surfaced = true;
                } else {
                    // The check borrows usage. finish needs that borrow ended.
                    let close = {
                        let mut close_in = crate::close_check::CloseIn {
                            depth: input.depth,
                            blocked: drive_blocked,
                            conversation_id: input.conversation_id,
                            model: input.model,
                            client: input.client,
                            cancel: input.cancel,
                            drive: drive.as_ref(),
                            last_assistant: &last_assistant,
                            usage: &mut usage,
                        };
                        crate::close_check::before_end(&mut close_in)
                    };
                    match close {
                        crate::close_check::CloseGate::Cancelled => {
                            return finish(
                                input,
                                StopReason::Cancelled,
                                usage,
                                compacted_once,
                                true,
                            );
                        }
                        crate::close_check::CloseGate::Continue { note, steps } => {
                            close_continued = true;
                            close_steps = steps;
                            history.push(user_message(&note, None));
                            continue;
                        }
                        crate::close_check::CloseGate::Proceed(notice) => {
                            if let Some(body) = notice {
                                on_event(LoopEvent::Findings(body));
                            }
                        }
                    }
                }
                if !stop_hook_active {
                    if let Some(reason) =
                        crate::hooks::on_stop(input.conversation_id, input.workspace, "Stop", false)
                    {
                        stop_hook_active = true;
                        history.push(user_message(
                            &format!("Stop hook blocked the turn: {reason}"),
                            None,
                        ));
                        continue;
                    }
                    return finish(input, StopReason::EndTurn, usage, compacted_once, false);
                }
                return finish(input, StopReason::EndTurn, usage, compacted_once, true);
            }
            for note in notes {
                history.push(user_message(&note, None));
            }
            continue;
        }
        let mut always = gate.mode == gate::PermMode::Always;
        for call in &turn.calls {
            history.push(InputItem::FunctionCall {
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            });
        }
        // Per-step file checkpoints: one step id for this tool batch.
        let _ = crate::file_checkpoint::begin_step(input.conversation_id);
        // Spike-2b: a denied hard step stops the rest of this batch.
        let mut batch_failed = false;
        let batched_tools_on = tools::batch::settings(&config_dir).enabled;
        let mut call_i = 0usize;
        while call_i < turn.calls.len() {
            let call = &turn.calls[call_i];
            if input.cancel.is_cancelled() {
                return finish(input, StopReason::Cancelled, usage, compacted_once, true);
            }
            if stop_for_halt(input) {
                return finish(input, StopReason::Halted, usage, compacted_once, true);
            }
            if batch_failed {
                let output = ToolOutput::err(gate::NOT_EXECUTED);
                emit_tool(on_event, &tool_id(call), call, "failed", &output.text, None);
                push_output(history, call, output);
                call_i += 1;
                continue;
            }
            if batched_tools_on {
                let run_len = tools::batch::batch_run_len(&turn.calls, call_i);
                if run_len > 1 {
                    match run_readonly_tool_batch(
                        &turn.calls[call_i..call_i + run_len],
                        input,
                        &mut gate,
                        base_readonly,
                        plan_at_start,
                        &mut always,
                        &mut batch_failed,
                        &mut repeats,
                        &mut nested_agents,
                        &mut evidence,
                        &mut fresh,
                        &mut modified_paths,
                        history,
                        &mut usage,
                        on_event,
                    ) {
                        ReadonlyBatchOutcome::Ran(n) => {
                            call_i += n;
                            if input.cancel.is_cancelled() {
                                return finish(
                                    input,
                                    StopReason::Cancelled,
                                    usage,
                                    compacted_once,
                                    true,
                                );
                            }
                            if stop_for_halt(input) {
                                return finish(
                                    input,
                                    StopReason::Halted,
                                    usage,
                                    compacted_once,
                                    true,
                                );
                            }
                            continue;
                        }
                        ReadonlyBatchOutcome::Fallback => {}
                        ReadonlyBatchOutcome::Stop(reason) => {
                            return finish(input, reason, usage, compacted_once, true);
                        }
                    }
                }
            }
            let key = format!("{}\\n{}", call.name, call.arguments);
            let seen = repeats.entry(key).or_insert(0);
            *seen = seen.saturating_add(1);
            if *seen >= REPEAT_LIMIT {
                let output = ToolOutput::err(REPEAT_REPLAN);
                push_output(history, call, output);
                call_i += 1;
                continue;
            }
            let id = tool_id(call);
            let gate_lap = crate::timing::lap("loop:tool_gate");
            let desk = tools::desk_flags(&call.name, &gate, input.desktop);
            let gate_args = hinted_args(&gate, input.desktop, call);
            let base = gate::decide_with(
                &gate,
                &call.name,
                &gate_args,
                always,
                desk,
                input.workspace,
                input.perms,
            );
            let reviewed = crate::auto_review::review(&crate::auto_review::ReviewIn {
                client: input.client,
                cancel: input.cancel,
                halt: &|| stop_for_halt(input),
                gate: &gate,
                name: &call.name,
                arguments: &call.arguments,
                workspace: input.workspace,
                policy: input.perms,
                latched_always: always,
                desk,
                history,
                conversation_id: input.conversation_id,
                base,
                timeout: crate::auto_review::JUDGE_TIMEOUT,
            });
            if let Some(extra) = &reviewed.judge_usage {
                usage.add(extra);
                emit_usage(on_event, &usage, history, input.context_length);
            }
            if reviewed.cancelled {
                let output = ToolOutput::err(gate::user_cancelled(&call.name));
                emit_tool(on_event, &id, call, "failed", &output.text, None);
                push_output(history, call, output);
                return finish(input, StopReason::Cancelled, usage, compacted_once, true);
            }
            if reviewed.halted {
                let _ = stop_for_halt(input);
                return finish(input, StopReason::Halted, usage, compacted_once, true);
            }
            let constrained = crate::hooks::constrain_tool(crate::hooks::ToolHook {
                session: input.conversation_id,
                workspace: input.workspace,
                attended: gate.attended,
                decision: reviewed.decision,
                ask_reason: reviewed.ask_reason,
                name: &call.name,
                arguments: &call.arguments,
                tool_use_id: &id,
            });
            drop(gate_lap);
            let ask_reason = constrained.ask_reason;
            let pre_context = constrained.context;
            let (output, extra_usage) = match constrained.decision {
                Decision::Refuse(text) => {
                    batch_failed = hard_step(&call.name, &gate_args);
                    crate::hooks::on_permission_denied(
                        input.conversation_id,
                        input.workspace,
                        &call.name,
                        &call.arguments,
                        &id,
                    );
                    let output = ToolOutput::err(text);
                    emit_tool(on_event, &id, call, "in_progress", &call.arguments, None);
                    emit_tool(
                        on_event,
                        &id,
                        call,
                        if output.failed { "failed" } else { "completed" },
                        &output.text,
                        output.image_data_url.clone(),
                    );
                    (output, Usage::default())
                }
                Decision::Ask => {
                    crate::hooks::on_notification(
                        input.conversation_id,
                        input.workspace,
                        "permission_prompt",
                    );
                    on_event(LoopEvent::Permission {
                        id: id.clone(),
                        name: call.name.clone(),
                        action: action_line(&call.name, &call.arguments),
                        reason: ask_reason,
                    });
                    match input
                        .permits
                        .wait(&id, input.cancel, &|| stop_for_halt(input))
                    {
                        Waited::Answer(PermAnswer::Allow) => note_tool(
                            input,
                            &mut gate,
                            base_readonly,
                            plan_at_start,
                            call,
                            &id,
                            on_event,
                            &pre_context,
                            &mut nested_agents,
                        ),
                        Waited::Answer(PermAnswer::Always) => {
                            if input.perms.is_some() {
                                let _ = crate::perm::remember_allow_always(
                                    input.workspace,
                                    &call.name,
                                    &call.arguments,
                                );
                            }
                            always = true;
                            note_tool(
                                input,
                                &mut gate,
                                base_readonly,
                                plan_at_start,
                                call,
                                &id,
                                on_event,
                                &pre_context,
                                &mut nested_agents,
                            )
                        }
                        Waited::Answer(PermAnswer::Deny) => {
                            batch_failed = hard_step(&call.name, &gate_args);
                            crate::hooks::on_permission_denied(
                                input.conversation_id,
                                input.workspace,
                                &call.name,
                                &call.arguments,
                                &id,
                            );
                            let output = ToolOutput::err(gate::user_rejected(&call.name));
                            emit_tool(on_event, &id, call, "failed", &output.text, None);
                            (output, Usage::default())
                        }
                        Waited::Answer(PermAnswer::Cancel) | Waited::Cancelled => {
                            let output = ToolOutput::err(gate::user_cancelled(&call.name));
                            emit_tool(on_event, &id, call, "failed", &output.text, None);
                            push_output(history, call, output);
                            return finish(
                                input,
                                StopReason::Cancelled,
                                usage,
                                compacted_once,
                                true,
                            );
                        }
                        Waited::Halted => {
                            let _ = stop_for_halt(input);
                            return finish(input, StopReason::Halted, usage, compacted_once, true);
                        }
                    }
                }
                Decision::Run => note_tool(
                    input,
                    &mut gate,
                    base_readonly,
                    plan_at_start,
                    call,
                    &id,
                    on_event,
                    &pre_context,
                    &mut nested_agents,
                ),
            };
            if usage_pending(&extra_usage) {
                usage.add(&extra_usage);
                emit_usage(on_event, &usage, history, input.context_length);
            }
            let cancelled = input.cancel.is_cancelled();
            let halted = stop_for_halt(input);
            if !output.failed && !crate::drive::is_session_tool(&call.name) {
                evidence += 1;
                fresh.note(
                    &call.name,
                    &serde_json::from_str(&call.arguments).unwrap_or_default(),
                );
                if let Some(path) =
                    crate::compact::path_from_write_call(&call.name, &call.arguments)
                {
                    if !modified_paths.iter().any(|p| p == &path) {
                        modified_paths.push(path);
                    }
                }
            }
            if !output.failed && call.name == "report_findings" {
                fresh.note_findings();
            }
            push_output(history, call, output);
            if cancelled {
                return finish(input, StopReason::Cancelled, usage, compacted_once, true);
            }
            if halted {
                return finish(input, StopReason::Halted, usage, compacted_once, true);
            }
            call_i += 1;
        }
        if drive.is_some() && evidence >= checkpoint_at + crate::checkpoint::CHECKPOINT_EVERY {
            // A checkpoint between phases: name the Brief's done items still uncovered, once per change.
            checkpoint_at = evidence;
            let due = crate::checkpoint::uncovered(
                brief.as_ref(),
                &crate::session_tools::todos_for(input.conversation_id),
                evidence,
            );
            if !due.is_empty() && due != checkpoint_due {
                let gaps = crate::checkpoint::Gaps {
                    uncovered: due.clone(),
                    ..Default::default()
                };
                history.push(user_message(&format!("Checkpoint: {}", gaps.note()), None));
            }
            checkpoint_due = due;
        }
        if let Some(note) = progress.as_mut().and_then(|p| {
            p.check(
                &crate::session_tools::todos_for(input.conversation_id),
                evidence,
            )
        }) {
            // Shown in the reply, not sent back to the model.
            on_event(LoopEvent::Text(format!("\n\n{note}\n\n")));
        }
        for note in input.steer.drain() {
            history.push(user_message(&note, None));
        }
    }
}

/// A later turn's talk style replaces the earlier one in the stored system
/// message; the stable prefix before it stays cached.
fn restyle_history(
    history: &mut [InputItem],
    style: grokhub_core::talk::TalkStyle,
    soft_caps: bool,
) {
    if let Some(InputItem::Message { role, content }) = history.first_mut() {
        if let Some(ContentPart::InputText(text)) = content.first_mut().filter(|_| role == "system")
        {
            *text = grokhub_core::talk::restyle_caps(text, style, soft_caps);
        }
    }
}

/// Saved-turn labels (`TURN_SAY:` and the rest) are the cabin's transcript
/// format, never chat. A model that sees them echoes them, so no user or
/// assistant message carries one into a request.
fn scrub_turn_labels(history: &mut [InputItem]) {
    for item in history.iter_mut() {
        let InputItem::Message { role, content } = item else {
            continue;
        };
        if role == "system" {
            continue;
        }
        for part in content.iter_mut() {
            if let ContentPart::InputText(text) = part {
                if text.contains("TURN_") {
                    *text = grokhub_core::strip_turn_labels(text);
                }
            }
        }
    }
}

/// A later turn's playbook replaces the earlier one in the stored system
/// prompt; a turn with none drops it.
fn replaybook_history(history: &mut [InputItem], block: Option<&str>) {
    if let Some(InputItem::Message { role, content }) = history.first_mut() {
        if let Some(ContentPart::InputText(text)) = content.first_mut().filter(|_| role == "system")
        {
            *text = grokhub_core::playbook::with_playbook(text, block);
        }
    }
}

/// A hard-class step (money, send, delete, credentials, irreversible OS) or
/// a floor refusal. When one is denied, the rest of its batch does not run.
fn hard_step(name: &str, arguments: &str) -> bool {
    !crate::harness::decide(crate::harness::Step::Tool { name, arguments }).is_allow()
}

/// The arguments the gate reads: a desktop call's own, plus the focused
/// window and the control under a click ([`DesktopOps::hint`]). The call
/// still runs with its own arguments.
fn hinted_args<'c>(
    gate: &Gate,
    desktop: Option<&dyn DesktopOps>,
    call: &'c FunctionCall,
) -> std::borrow::Cow<'c, str> {
    match desktop {
        Some(desk) if gate.desktop && gate::is_desktop(&call.name) => {
            let args: serde_json::Value =
                serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));
            std::borrow::Cow::Owned(desk.hint(&call.name, &args).to_string())
        }
        _ => std::borrow::Cow::Borrowed(&call.arguments),
    }
}

fn emit_usage(
    on_event: &mut dyn FnMut(LoopEvent),
    usage: &Usage,
    history: &[InputItem],
    limit: u64,
) {
    on_event(LoopEvent::Meter {
        used: crate::compact::estimate_input_tokens(history),
        limit,
    });
    on_event(LoopEvent::Usage(usage.clone()));
}

fn push_intention_block(history: &mut Vec<InputItem>, block: &str) {
    if block.is_empty() || history_has_text(history, block) {
        return;
    }
    history.push(InputItem::Message {
        role: "system".into(),
        content: vec![ContentPart::InputText(block.to_string())],
    });
}

fn history_has_text(history: &[InputItem], needle: &str) -> bool {
    history.iter().any(|item| match item {
        InputItem::Message { content, .. } => content
            .iter()
            .any(|part| matches!(part, ContentPart::InputText(text) if text.contains(needle))),
        _ => false,
    })
}

fn user_message(text: &str, image: Option<&str>) -> InputItem {
    let mut content = vec![ContentPart::InputText(text.to_string())];
    if let Some(url) = image.map(str::trim).filter(|s| !s.is_empty()) {
        content.push(ContentPart::InputImage(url.to_string()));
    }
    InputItem::Message {
        role: "user".into(),
        content,
    }
}

fn push_output(history: &mut Vec<InputItem>, call: &FunctionCall, output: ToolOutput) {
    if output.failed {
        // E1: the next routed step thinks one rung harder.
        crate::route::ladder::note_tool_error();
    }
    history.push(InputItem::FunctionCallOutput {
        call_id: call.call_id.clone(),
        output: output.text,
    });
    if let Some(url) = output.image_data_url {
        history.push(InputItem::Message {
            role: "user".into(),
            content: vec![
                ContentPart::InputText(format!("image from {}", call.name)),
                ContentPart::InputImage(url),
            ],
        });
    }
}

/// Trail span for a length-truncated model reply (`turn_truncated`).
fn record_turn_truncated(conversation_id: &str, reason: Option<&str>, streak: u32) {
    let reason = reason.unwrap_or("max_output_tokens");
    let args = serde_json::json!({
        "reason": reason,
        "streak": streak,
    })
    .to_string();
    let span = crate::harness::Span::deny(conversation_id, "turn_truncated", &args, reason, "soft")
        .in_turn(conversation_id, 0);
    let _ = crate::harness::append_span(&crate::perm::config_dir(), &span);
}

fn emit_tool(
    on_event: &mut dyn FnMut(LoopEvent),
    id: &str,
    call: &FunctionCall,
    status: &str,
    detail: &str,
    image: Option<String>,
) {
    on_event(LoopEvent::Tool {
        id: id.to_string(),
        name: call.name.clone(),
        status: status.into(),
        detail: clip(detail, detail_limit(detail)),
        image,
    });
}

fn stop_for_halt(input: &LoopIn<'_>) -> bool {
    let stop = input.halt.halted() || input.tasks.as_ref().is_some_and(|hub| hub.is_halted());
    if stop {
        if let Some(tasks) = &input.tasks {
            tasks.halt_all();
        }
    }
    stop
}

fn inject_notices(input: &LoopIn<'_>, history: &mut Vec<InputItem>) {
    let Some(tasks) = &input.tasks else {
        return;
    };
    for note in tasks.drain_notices() {
        let text = format!("<background-notice>\n{note}\n</background-notice>");
        history.push(user_message(&text, None));
    }
}

fn usage_pending(usage: &Usage) -> bool {
    usage.input_tokens != 0
        || usage.output_tokens != 0
        || usage.reasoning_tokens != 0
        || usage.cost_in_usd_ticks != 0
}

fn note_tool(
    input: &LoopIn<'_>,
    gate: &mut Gate,
    base_readonly: bool,
    plan_at_start: bool,
    call: &FunctionCall,
    id: &str,
    on_event: &mut dyn FnMut(LoopEvent),
    pre_context: &str,
    nested: &mut crate::prompt::NestedAgents,
) -> (ToolOutput, Usage) {
    let _lap = crate::timing::lap("loop:tool_run");
    emit_tool(on_event, id, call, "in_progress", &call.arguments, None);
    let (output, usage) =
        dispatch_tool_call(input, gate, base_readonly, plan_at_start, call, on_event);
    let (output, usage) = finish_tool_output(
        input,
        call,
        id,
        on_event,
        pre_context,
        nested,
        output,
        usage,
    );
    let status = if output.failed { "failed" } else { "completed" };
    emit_tool(
        on_event,
        id,
        call,
        status,
        &output.text,
        output.image_data_url.clone(),
    );
    (output, usage)
}

fn finish_tool_output(
    input: &LoopIn<'_>,
    call: &FunctionCall,
    id: &str,
    _on_event: &mut dyn FnMut(LoopEvent),
    pre_context: &str,
    nested: &mut crate::prompt::NestedAgents,
    mut output: ToolOutput,
    mut usage: Usage,
) -> (ToolOutput, Usage) {
    if !pre_context.is_empty() {
        if !output.text.is_empty() && !output.text.ends_with('\n') {
            output.text.push('\n');
        }
        output.text.push_str(pre_context);
    }
    // Just-in-time nested AGENTS.md / CLAUDE.md: append to tool output only.
    if !output.failed && tools::is_path_tool(&call.name) {
        let args: serde_json::Value =
            serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
        if let Some(raw) = tools::tool_path_arg(&call.name, &args) {
            if let Ok(path) = tools::confine(input.workspace, &raw) {
                let note = nested.notes_for(input.workspace, &path);
                if !note.is_empty() {
                    if !output.text.is_empty() && !output.text.ends_with('\n') {
                        output.text.push('\n');
                    }
                    output.text.push_str(&note);
                }
            }
        }
    }
    let extra = crate::hooks::after_tool(
        input.conversation_id,
        input.workspace,
        &call.name,
        &call.arguments,
        id,
        output.failed,
        &output.text,
    );
    if !extra.is_empty() {
        if !output.text.is_empty() && !output.text.ends_with('\n') {
            output.text.push('\n');
        }
        output.text.push_str(&extra);
    }
    if !output.failed {
        let config_dir = crate::perm::config_dir();
        match crate::compact::maybe_compact_tool_output(
            &config_dir,
            input.workspace,
            input.conversation_id,
            input.client,
            input.cancel,
            input.model,
            input.effort,
            id,
            &call.name,
            &output.text,
        ) {
            Ok(crate::compact::ToolOutputCompactOutcome::Replaced { text, usage: extra }) => {
                output.text = text;
                usage.add(&extra);
            }
            Ok(crate::compact::ToolOutputCompactOutcome::Unchanged) => {}
            Err(crate::compact::CompactError::Cancelled) => {}
            Err(_) => {}
        }
    }
    (output, usage)
}

enum ReadonlyBatchOutcome {
    Ran(usize),
    Fallback,
    Stop(StopReason),
}

/// Gate a consecutive read-only run and dispatch in parallel when every call is `Run`.
#[allow(clippy::too_many_arguments)]
fn run_readonly_tool_batch(
    calls: &[FunctionCall],
    input: &LoopIn<'_>,
    gate: &mut Gate,
    _base_readonly: bool,
    _plan_at_start: bool,
    always: &mut bool,
    _batch_failed: &mut bool,
    repeats: &mut HashMap<String, u32>,
    nested_agents: &mut crate::prompt::NestedAgents,
    evidence: &mut usize,
    fresh: &mut crate::checkpoint::Fresh,
    modified_paths: &mut Vec<String>,
    history: &mut Vec<InputItem>,
    usage: &mut Usage,
    on_event: &mut dyn FnMut(LoopEvent),
) -> ReadonlyBatchOutcome {
    let mut prepared = Vec::with_capacity(calls.len());
    for call in calls {
        let key = format!("{}\\n{}", call.name, call.arguments);
        let seen = repeats.entry(key).or_insert(0);
        if *seen >= REPEAT_LIMIT {
            return ReadonlyBatchOutcome::Fallback;
        }
        let id = tool_id(call);
        let gate_lap = crate::timing::lap("loop:tool_gate");
        let desk = tools::desk_flags(&call.name, gate, input.desktop);
        let gate_args = hinted_args(gate, input.desktop, call);
        let base = gate::decide_with(
            gate,
            &call.name,
            &gate_args,
            *always,
            desk,
            input.workspace,
            input.perms,
        );
        let reviewed = crate::auto_review::review(&crate::auto_review::ReviewIn {
            client: input.client,
            cancel: input.cancel,
            halt: &|| stop_for_halt(input),
            gate,
            name: &call.name,
            arguments: &call.arguments,
            workspace: input.workspace,
            policy: input.perms,
            latched_always: *always,
            desk,
            history,
            conversation_id: input.conversation_id,
            base,
            timeout: crate::auto_review::JUDGE_TIMEOUT,
        });
        if let Some(extra) = &reviewed.judge_usage {
            usage.add(extra);
            emit_usage(on_event, usage, history, input.context_length);
        }
        if reviewed.cancelled {
            return ReadonlyBatchOutcome::Stop(StopReason::Cancelled);
        }
        if reviewed.halted {
            let _ = stop_for_halt(input);
            return ReadonlyBatchOutcome::Stop(StopReason::Halted);
        }
        let constrained = crate::hooks::constrain_tool(crate::hooks::ToolHook {
            session: input.conversation_id,
            workspace: input.workspace,
            attended: gate.attended,
            decision: reviewed.decision,
            ask_reason: reviewed.ask_reason,
            name: &call.name,
            arguments: &call.arguments,
            tool_use_id: &id,
        });
        drop(gate_lap);
        match constrained.decision {
            Decision::Run => {
                *seen = seen.saturating_add(1);
                prepared.push((call, id, constrained.context));
            }
            Decision::Refuse(_) | Decision::Ask => return ReadonlyBatchOutcome::Fallback,
        }
    }

    for (call, id, _) in &prepared {
        emit_tool(on_event, id, call, "in_progress", &call.arguments, None);
    }

    if stop_for_halt(input) {
        let _ = stop_for_halt(input);
        return ReadonlyBatchOutcome::Stop(StopReason::Halted);
    }
    let session = input.conversation_id;
    let workspace = input.workspace;
    let tasks = input.tasks.clone();
    let owner = input.agent_id;
    let cancel = input.cancel.clone();
    let should_stop = || cancel.is_cancelled() || tasks.as_ref().is_some_and(|hub| hub.is_halted());
    let indices: Vec<usize> = (0..calls.len()).collect();
    let raw_outputs = tools::batch::run_parallel(&indices, Some(session), &should_stop, |slot| {
        let call = &calls[slot];
        if should_stop() {
            return ToolOutput::err(gate::user_cancelled(&call.name));
        }
        let ctx = ToolCtx {
            workspace,
            desktop: None,
            stop: &should_stop,
            tasks: tasks.clone(),
            owner,
            session: Some(session),
        };
        let set = bound_job_tool_set(session);
        tools::dispatch_for_job(&ctx, &call.name, &call.arguments, set.as_ref())
    });

    for (slot, (call, id, pre_context)) in prepared.into_iter().enumerate() {
        let output = raw_outputs[slot].clone();
        let (output, extra_usage) = finish_tool_output(
            input,
            call,
            &id,
            on_event,
            &pre_context,
            nested_agents,
            output,
            Usage::default(),
        );
        let status = if output.failed { "failed" } else { "completed" };
        emit_tool(
            on_event,
            &id,
            call,
            status,
            &output.text,
            output.image_data_url.clone(),
        );
        if usage_pending(&extra_usage) {
            usage.add(&extra_usage);
            emit_usage(on_event, usage, history, input.context_length);
        }
        if !output.failed && !crate::drive::is_session_tool(&call.name) {
            *evidence += 1;
            fresh.note(
                &call.name,
                &serde_json::from_str(&call.arguments).unwrap_or_default(),
            );
            if let Some(path) = crate::compact::path_from_write_call(&call.name, &call.arguments) {
                if !modified_paths.iter().any(|p| p == &path) {
                    modified_paths.push(path);
                }
            }
        }
        if !output.failed && call.name == "report_findings" {
            fresh.note_findings();
        }
        push_output(history, call, output);
    }

    ReadonlyBatchOutcome::Ran(calls.len())
}

fn dispatch_tool_call(
    input: &LoopIn<'_>,
    gate: &mut Gate,
    base_readonly: bool,
    plan_at_start: bool,
    call: &FunctionCall,
    on_event: &mut dyn FnMut(LoopEvent),
) -> (ToolOutput, Usage) {
    let attended = gate.attended;
    let session = input.conversation_id.to_string();
    let cancel = input.cancel.clone();
    let tasks = input.tasks.clone();
    let halt = input.halt;
    let ctx = ToolCtx {
        workspace: input.workspace,
        desktop: input.desktop,
        stop: &|| input.cancel.is_cancelled() || stop_for_halt(input),
        tasks: input.tasks.clone(),
        owner: input.agent_id,
        session: Some(input.conversation_id),
    };
    let halted = || halt.halted() || tasks.as_ref().is_some_and(|hub| hub.is_halted());
    if let Some(done) = crate::session_tools::try_run(crate::session_tools::SessionCall {
        name: &call.name,
        arguments: &call.arguments,
        gate,
        session: &session,
        attended,
        cancel: &cancel,
        halted: &halted,
        base_readonly,
        plan_at_start,
        on_event,
    }) {
        return done;
    }
    if let Some(done) = crate::subagent::try_run(crate::subagent::SpawnCall {
        name: &call.name,
        arguments: &call.arguments,
        gate,
        session: &session,
        workspace: input.workspace,
        cancel: &cancel,
        halt,
        tasks: input.tasks.clone(),
        depth: input.depth,
        agent_id: input.agent_id,
        client: input.shared_client.clone(),
        permits: input.shared_permits.clone(),
        desktop: input.shared_desktop.clone(),
        model: input.model,
        effort: input.effort,
        system: input.system,
        max_turns: input.max_turns,
        policy: input.perms,
        on_event,
    }) {
        return done;
    }
    let mut emit_card = |view: crate::mcp::ElicitView| {
        on_event(LoopEvent::Elicit(view));
    };
    let mut wait_card = |eid: &str| crate::mcp::wait_elicit(&session, eid, &cancel, &halted);
    let job_set = bound_job_tool_set(input.conversation_id);
    let output = crate::mcp::with_elicit(attended, &mut emit_card, &mut wait_card, || {
        tools::dispatch_for_job(&ctx, &call.name, &call.arguments, job_set.as_ref())
    });
    (output, Usage::default())
}

fn action_line(name: &str, arguments: &str) -> String {
    let _ = name;
    let value: serde_json::Value =
        serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null);
    let picked = [
        "path",
        "file_path",
        "target_file",
        "command",
        "text",
        "keys",
        "url",
        "prompt",
    ]
    .iter()
    .find_map(|key| {
        value
            .get(*key)
            .and_then(|item| item.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
    });
    clip(picked.unwrap_or(arguments), 180)
}

fn tool_id(call: &FunctionCall) -> String {
    if call.call_id.is_empty() {
        format!("tool-{}", call.name)
    } else {
        call.call_id.clone()
    }
}

fn detail_limit(detail: &str) -> usize {
    if detail.contains("IMAGINE:") {
        4000
    } else {
        180
    }
}

fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{self, Gate, PermAnswer, PermitWait, Waited};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Script {
        turns: Mutex<Vec<ScriptTurn>>,
        seen: Mutex<Vec<Vec<InputItem>>>,
        cancel_on_text: bool,
        steer: Option<SteerQueue>,
    }

    #[derive(Clone)]
    struct ScriptTurn {
        text: String,
        calls: Vec<FunctionCall>,
        usage: Usage,
        truncated: bool,
    }

    impl ScriptTurn {
        fn text(text: impl Into<String>) -> Self {
            Self {
                text: text.into(),
                calls: Vec::new(),
                usage: Usage::default(),
                truncated: false,
            }
        }

        fn calls(calls: Vec<FunctionCall>, usage: Usage) -> Self {
            Self {
                text: String::new(),
                calls,
                usage,
                truncated: false,
            }
        }

        fn truncated_calls(calls: Vec<FunctionCall>) -> Self {
            Self {
                text: String::new(),
                calls,
                usage: Usage::default(),
                truncated: true,
            }
        }
    }

    impl ModelClient for Script {
        fn stream(
            &self,
            req: &ResponsesRequest,
            cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<crate::TurnOutput, ClientError> {
            let n = self.seen.lock().unwrap().len();
            self.seen.lock().unwrap().push(req.input.clone());
            if n == 0 {
                if let Some(steer) = &self.steer {
                    steer.push("please look again");
                }
            }
            if self.cancel_on_text {
                sink(StreamEvent::TextDelta("partial".into()));
                cancel.cancel();
                return Err(ClientError::Cancelled);
            }
            let turn = {
                let mut turns = self.turns.lock().unwrap();
                if turns.is_empty() {
                    ScriptTurn::text("")
                } else {
                    turns.remove(0)
                }
            };
            if !turn.text.is_empty() {
                sink(StreamEvent::TextDelta(turn.text.clone()));
            }
            Ok(crate::TurnOutput {
                text: turn.text,
                reasoning: String::new(),
                calls: turn.calls,
                usage: turn.usage,
                truncated: turn.truncated,
                truncate_reason: turn.truncated.then(|| "max_output_tokens".into()),
            })
        }
    }

    struct NeverHalt;
    impl HaltCheck for NeverHalt {
        fn halted(&self) -> bool {
            false
        }
    }

    struct HaltAfter(AtomicUsize);
    impl HaltCheck for HaltAfter {
        fn halted(&self) -> bool {
            self.0.fetch_add(1, Ordering::SeqCst) >= 1
        }
    }

    struct AlwaysTool {
        calls: AtomicUsize,
    }

    impl ModelClient for AlwaysTool {
        fn stream(
            &self,
            _req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<crate::TurnOutput, ClientError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(crate::TurnOutput {
                text: String::new(),
                reasoning: String::new(),
                calls: vec![FunctionCall {
                    call_id: format!("c{n}"),
                    name: "list_dir".into(),
                    arguments: format!(r#"{{"path":"p{n}"}}"#),
                }],
                usage: Usage::default(),
                ..Default::default()
            })
        }
    }

    fn call(id: &str, name: &str, arguments: &str) -> FunctionCall {
        FunctionCall {
            call_id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    fn usage(input: u64, output: u64, reasoning: u64, cost: i64) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            reasoning_tokens: reasoning,
            cost_in_usd_ticks: cost,
            cached_tokens: 0,
        }
    }

    fn workspace(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-loop-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("note.txt"), "hello native\n").unwrap();
        dir
    }

    fn run(
        script: &Script,
        dir: &std::path::Path,
        max_turns: u32,
        cancel: &CancelToken,
        steer: &SteerQueue,
        halt: &dyn HaltCheck,
    ) -> (LoopOut, Vec<InputItem>, Vec<LoopEvent>) {
        let input = LoopIn {
            client: script,
            workspace: dir,
            model: "grok-4.7",
            effort: Some("low"),
            system: "sys",
            conversation_id: "conv",
            max_turns,
            usage_base: Usage::default(),
            cancel,
            steer,
            halt,
            gate: Gate::phase_readonly(),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let mut events = Vec::new();
        let out = run_loop(&input, &mut history, "read the note", None, &mut |ev| {
            events.push(ev)
        });
        (out, history, events)
    }

    #[test]
    fn child_session_inherits_parent_job_tool_set() {
        let parent = "native-sp2-parent";
        let child = format!("{parent}/agent-1");
        let nested = format!("{child}/agent-2");
        let set = ToolSet::only(["read_file"]);
        let _guard = bind_job_tool_set(parent, set.clone());
        assert_eq!(bound_job_tool_set(parent).as_ref(), Some(&set));
        assert_eq!(bound_job_tool_set(&child).as_ref(), Some(&set));
        assert_eq!(bound_job_tool_set(&nested).as_ref(), Some(&set));
        assert!(bound_job_tool_set("unrelated-session").is_none());
        // A tighter bind on the child wins over the parent.
        let tighter = ToolSet::only(["list_dir"]);
        let _child_guard = bind_job_tool_set(&child, tighter.clone());
        assert_eq!(bound_job_tool_set(&child).as_ref(), Some(&tighter));
        assert_eq!(bound_job_tool_set(&nested).as_ref(), Some(&tighter));
        assert_eq!(bound_job_tool_set(parent).as_ref(), Some(&set));
        clear_job_tool_set(parent);
        clear_job_tool_set(&child);
        clear_job_tool_set(&nested);
    }

    #[test]
    fn detached_child_keeps_tool_set_after_parent_guard_drops() {
        // Security: background spawn_subagent outlives the parent run_loop guard.
        let parent = "native-sp2-detached-parent";
        let child = format!("{parent}/bg-agent");
        let set = ToolSet::only(["read_file", "run_terminal_command"]);
        let guard = bind_job_tool_set(parent, set.clone());
        drop(guard);
        // Parent bind must remain so ancestor walk (and materialize) still work.
        assert_eq!(bound_job_tool_set(parent).as_ref(), Some(&set));
        assert_eq!(bound_job_tool_set(&child).as_ref(), Some(&set));
        // After materialize, clearing the parent must not fail-open the child.
        clear_job_tool_set(parent);
        assert!(bound_job_tool_set(parent).is_none());
        assert_eq!(bound_job_tool_set(&child).as_ref(), Some(&set));
        assert!(!set.allows("write"));
        clear_job_tool_set(&child);
    }

    #[test]
    fn truncated_turn_does_not_run_tool_calls() {
        let dir = workspace("trunc");
        let config = dir.join("config");
        std::fs::create_dir_all(&config).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&config);
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn::truncated_calls(vec![call(
                    "w1",
                    "write",
                    r#"{"path":"big.rs","content":"partial"}"#,
                )]),
                ScriptTurn::text("rewrote in smaller pieces"),
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, events) = run(
            &script,
            &dir,
            0,
            &CancelToken::new(),
            &SteerQueue::new(),
            &NeverHalt,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        let outputs: Vec<_> = history
            .iter()
            .filter_map(|i| match i {
                InputItem::FunctionCallOutput { output, .. } => Some(output.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(outputs, [TRUNCATED_TOOL]);
        assert!(events.iter().any(|ev| matches!(
            ev,
            LoopEvent::Tool { status, detail, .. }
                if status == "failed" && detail.contains("cut off at the output limit")
        )));
        // write never ran: the file is absent.
        assert!(!dir.join("big.rs").exists());
        let spans = std::fs::read_to_string(crate::harness::span_path(&config, "conv")).unwrap();
        assert!(spans.contains("turn_truncated"), "{spans}");
        assert!(spans.contains("max_output_tokens"), "{spans}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn completed_tool_turn_still_runs_as_before() {
        let dir = workspace("trunc-ok");
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn::calls(
                    vec![call("w1", "write", r#"{"path":"ok.rs","content":"full"}"#)],
                    Usage::default(),
                ),
                ScriptTurn::text("done"),
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let permits = Answer {
            answer: PermAnswer::Allow,
            asks: AtomicUsize::new(0),
        };
        let (out, history, _events) = once(
            &script,
            &dir,
            chat(gate::PermMode::Always, true, false),
            None,
            &permits,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        let outputs: Vec<_> = history
            .iter()
            .filter_map(|i| match i {
                InputItem::FunctionCallOutput { output, .. } => Some(output.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            outputs
                .iter()
                .all(|o| !o.contains("cut off at the output limit")),
            "{outputs:?}"
        );
        assert_eq!(std::fs::read_to_string(dir.join("ok.rs")).unwrap(), "full");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_turn_cap_runs_past_fifty_tool_turns_to_the_reply() {
        let dir = workspace("nocap");
        let mut turns: Vec<ScriptTurn> = (0..80)
            .map(|n| ScriptTurn {
                text: String::new(),
                calls: vec![call(
                    &format!("t{n}"),
                    "list_dir",
                    &format!(r#"{{"path":"p{n}"}}"#),
                )],
                usage: Usage::default(),
                truncated: false,
            })
            .collect();
        turns.push(ScriptTurn {
            text: "all done".into(),
            calls: Vec::new(),
            usage: Usage::default(),
            truncated: false,
        });
        let script = Script {
            turns: Mutex::new(turns),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, events) = run(
            &script,
            &dir,
            0,
            &CancelToken::new(),
            &SteerQueue::new(),
            &NeverHalt,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 81);
        let outputs = history
            .iter()
            .filter(|i| matches!(i, InputItem::FunctionCallOutput { .. }))
            .count();
        assert_eq!(outputs, 80);
        assert!(events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Text(t) if t == "all done")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loop_runs_parallel_read_only_tools_and_sums_usage() {
        let dir = workspace("sum");
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![
                        call("a", "read_file", r#"{"target_file":"note.txt"}"#),
                        call("b", "list_dir", "{}"),
                    ],
                    usage: usage(10, 5, 2, 100),
                    truncated: false,
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: usage(7, 3, 1, 50),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, events) = run(
            &script,
            &dir,
            0,
            &CancelToken::new(),
            &SteerQueue::new(),
            &NeverHalt,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(out.usage.input_tokens, 17);
        assert_eq!(out.usage.output_tokens, 8);
        assert_eq!(out.usage.reasoning_tokens, 3);
        assert_eq!(out.usage.cost_in_usd_ticks, 150);
        let outputs: Vec<&str> = history
            .iter()
            .filter_map(|item| match item {
                InputItem::FunctionCallOutput { output, .. } => Some(output.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            outputs.iter().any(|t| t.contains("hello native")),
            "{outputs:?}"
        );
        assert!(outputs.len() >= 2, "{outputs:?}");
        assert!(events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Tool { status, .. } if status == "in_progress")));
        assert!(events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Tool { status, .. } if status == "completed")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loop_refuses_write_injects_steer_and_guards_repeat_cancel_halt_cap() {
        let dir = workspace("guards");
        let write = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("w", "write", r#"{"path":"x"}"#)],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: "stopped".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = run(
            &write,
            &dir,
            4,
            &CancelToken::new(),
            &SteerQueue::new(),
            &NeverHalt,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. } if output.contains(tools::READ_ONLY_PHASE)
        )));
        assert!(!dir.join("x").exists());

        let steer = SteerQueue::new();
        let steer_script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("r", "read_file", r#"{"target_file":"missing.txt"}"#)],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: "after steer".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: Some(steer.clone()),
        };
        let _ = run(
            &steer_script,
            &dir,
            4,
            &CancelToken::new(),
            &steer,
            &NeverHalt,
        );
        let seen = steer_script.seen.lock().unwrap();
        let blob = format!("{:?}", seen.get(1));
        assert!(blob.contains("please look again"), "{blob}");
        drop(seen);

        let repeat = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("1", "read_file", r#"{"target_file":"note.txt"}"#)],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("2", "read_file", r#"{"target_file":"note.txt"}"#)],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("3", "read_file", r#"{"target_file":"note.txt"}"#)],
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = run(
            &repeat,
            &dir,
            10,
            &CancelToken::new(),
            &SteerQueue::new(),
            &NeverHalt,
        );
        // The third identical call re-plans instead of ending the run.
        assert_eq!(out.stop, StopReason::EndTurn);
        let replans: Vec<&str> = history
            .iter()
            .filter_map(|item| match item {
                InputItem::FunctionCallOutput { output, .. } if output.contains(REPEAT_REPLAN) => {
                    Some(output.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(replans.len(), 1);
        let runs = history
            .iter()
            .filter(|item| {
                matches!(item, InputItem::FunctionCallOutput { output, .. } if !output.contains(REPEAT_REPLAN))
            })
            .count();
        assert_eq!(runs, 2);
        assert_eq!(
            repeat.seen.lock().unwrap().len(),
            4,
            "the model got a turn after the re-plan note"
        );

        let cancel_script = Script {
            turns: Mutex::new(vec![ScriptTurn {
                text: "nope".into(),
                calls: Vec::new(),
                usage: Usage::default(),
                truncated: false,
            }]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: true,
            steer: None,
        };
        let (out, _, _) = run(
            &cancel_script,
            &dir,
            4,
            &CancelToken::new(),
            &SteerQueue::new(),
            &NeverHalt,
        );
        assert_eq!(out.stop, StopReason::Cancelled);

        let halt_script = Script {
            turns: Mutex::new(vec![ScriptTurn {
                text: String::new(),
                calls: vec![call("h", "read_file", r#"{"target_file":"note.txt"}"#)],
                usage: Usage::default(),
                truncated: false,
            }]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = run(
            &halt_script,
            &dir,
            4,
            &CancelToken::new(),
            &SteerQueue::new(),
            &HaltAfter(AtomicUsize::new(0)),
        );
        assert_eq!(out.stop, StopReason::Halted);
        assert!(!history
            .iter()
            .any(|item| matches!(item, InputItem::FunctionCallOutput { .. })));

        let cap_client = AlwaysTool {
            calls: AtomicUsize::new(0),
        };
        let input = LoopIn {
            client: &cap_client,
            workspace: &dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: "c",
            max_turns: 2,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &NeverHalt,
            gate: Gate::phase_readonly(),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |_| {});
        assert_eq!(out.stop, StopReason::MaxTurns);
        assert_eq!(cap_client.calls.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct Answer {
        answer: PermAnswer,
        asks: AtomicUsize,
    }

    impl PermitWait for Answer {
        fn wait(
            &self,
            _call_id: &str,
            _cancel: &crate::CancelToken,
            _halted: &dyn Fn() -> bool,
        ) -> Waited {
            self.asks.fetch_add(1, Ordering::SeqCst);
            Waited::Answer(self.answer)
        }
    }

    struct Spy {
        calls: AtomicUsize,
        halted: bool,
        locked: bool,
    }

    impl DesktopOps for Spy {
        fn halted(&self) -> bool {
            self.halted
        }
        fn locked(&self) -> bool {
            self.locked
        }
        fn call(&self, name: &str, _args: &serde_json::Value) -> ToolOutput {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if name == "screenshot" {
                ToolOutput {
                    text: "geom".into(),
                    image_data_url: Some("data:image/png;base64,YQ==".into()),
                    failed: false,
                }
            } else {
                ToolOutput::ok("ok")
            }
        }
    }

    fn chat(mode: gate::PermMode, attended: bool, desktop: bool) -> Gate {
        Gate {
            mode,
            readonly_session: false,
            attended,
            desktop,
        }
    }

    fn once(
        script: &Script,
        dir: &std::path::Path,
        gate: Gate,
        desktop: Option<&dyn DesktopOps>,
        permits: &dyn PermitWait,
    ) -> (LoopOut, Vec<InputItem>, Vec<LoopEvent>) {
        let input = LoopIn {
            client: script,
            workspace: dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: "conv",
            max_turns: 4,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &NeverHalt,
            gate,
            desktop,
            permits,
            perms: None,
            context_length: 0,
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let mut events = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |ev| events.push(ev));
        (out, history, events)
    }

    #[test]
    fn loop_gate_ask_allow_deny_always_and_unattended() {
        let dir = workspace("gate");
        let write_turn = |id: &str, path: &str| ScriptTurn {
            text: String::new(),
            calls: vec![call(
                id,
                "write",
                &format!(r#"{{"path":"{path}","content":"yes"}}"#),
            )],
            usage: Usage::default(),
            truncated: false,
        };
        let done = ScriptTurn {
            text: "done".into(),
            calls: Vec::new(),
            usage: Usage::default(),
            truncated: false,
        };

        let allow = Answer {
            answer: PermAnswer::Allow,
            asks: AtomicUsize::new(0),
        };
        let asking = Script {
            turns: Mutex::new(vec![write_turn("w", "asked.txt"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, _, events) = once(
            &asking,
            &dir,
            chat(gate::PermMode::Ask, true, false),
            None,
            &allow,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(allow.asks.load(Ordering::SeqCst), 1);
        assert!(events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Permission { name, .. } if name == "write")));
        assert_eq!(
            std::fs::read_to_string(dir.join("asked.txt")).unwrap(),
            "yes"
        );

        let auto = Answer {
            answer: PermAnswer::Deny,
            asks: AtomicUsize::new(0),
        };
        let auto_script = Script {
            turns: Mutex::new(vec![write_turn("a", "auto.txt"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, events) = once(
            &auto_script,
            &dir,
            chat(gate::PermMode::Auto, true, false),
            None,
            &auto,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(auto.asks.load(Ordering::SeqCst), 1);
        assert!(events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Permission { .. })));
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. } if output.contains("User rejected the execution for tool `write`")
        )));
        assert!(!dir.join("auto.txt").exists());

        let quiet = Answer {
            answer: PermAnswer::Deny,
            asks: AtomicUsize::new(0),
        };
        let always = Script {
            turns: Mutex::new(vec![write_turn("y", "always.txt"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, _, events) = once(
            &always,
            &dir,
            chat(gate::PermMode::Always, true, false),
            None,
            &quiet,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(quiet.asks.load(Ordering::SeqCst), 0);
        assert!(!events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Permission { .. })));
        assert_eq!(
            std::fs::read_to_string(dir.join("always.txt")).unwrap(),
            "yes"
        );

        let away = Answer {
            answer: PermAnswer::Allow,
            asks: AtomicUsize::new(0),
        };
        let unattended = Script {
            turns: Mutex::new(vec![write_turn("u", "away.txt"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, events) = once(
            &unattended,
            &dir,
            chat(gate::PermMode::Ask, false, false),
            None,
            &away,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(away.asks.load(Ordering::SeqCst), 0);
        assert!(!events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Permission { .. })));
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. }
                if output == "Tool `write` was not executed: Denied by permission policy: deny rule on edit"
        )));
        assert!(!dir.join("away.txt").exists());

        let auto_away = Answer {
            answer: PermAnswer::Deny,
            asks: AtomicUsize::new(0),
        };
        let auto_go = Script {
            turns: Mutex::new(vec![write_turn("g", "autogo.txt"), done]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, _, _) = once(
            &auto_go,
            &dir,
            chat(gate::PermMode::Auto, false, false),
            None,
            &auto_away,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(auto_away.asks.load(Ordering::SeqCst), 0);
        assert!(
            !dir.join("autogo.txt").exists(),
            "unattended Auto denies until Phase 5"
        );

        let escape = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call(
                        "e",
                        "write",
                        r#"{"path":"../nope.txt","content":"x"}"#,
                    )],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = once(
            &escape,
            &dir,
            chat(gate::PermMode::Always, true, false),
            None,
            &quiet,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. } if output.contains("escapes")
        )));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn allow_always_covers_the_rest_of_the_turn() {
        let dir = workspace("latch");
        let permits = Answer {
            answer: PermAnswer::Always,
            asks: AtomicUsize::new(0),
        };
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![
                        call("1", "write", r#"{"path":"a.txt","content":"a"}"#),
                        call("2", "write", r#"{"path":"b.txt","content":"b"}"#),
                    ],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, _, _) = once(
            &script,
            &dir,
            chat(gate::PermMode::Ask, true, false),
            None,
            &permits,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(permits.asks.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "b");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Spike-2b: a batch with a Pay in the middle. The step before it runs,
    /// Pay parks even under Always, and Deny stops the rest.
    #[test]
    fn a_denied_pay_in_a_batch_stops_the_steps_after_it() {
        let dir = workspace("batch-pay");
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![
                        call(
                            "1",
                            "click",
                            r#"{"x":10,"y":20,"label":"Shipping address"}"#,
                        ),
                        call(
                            "2",
                            "click",
                            r#"{"x":30,"y":40,"label":"Pay now","role":"push button"}"#,
                        ),
                        call("3", "click", r#"{"x":50,"y":60,"label":"Done"}"#),
                        call("4", "type", r#"{"text":"thanks"}"#),
                    ],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let spy = Spy {
            calls: AtomicUsize::new(0),
            halted: false,
            locked: false,
        };
        let deny = Answer {
            answer: PermAnswer::Deny,
            asks: AtomicUsize::new(0),
        };
        let (out, history, events) = once(
            &script,
            &dir,
            chat(gate::PermMode::Always, true, true),
            Some(&spy),
            &deny,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(
            spy.calls.load(Ordering::SeqCst),
            1,
            "only the step before Pay ran"
        );
        assert_eq!(
            deny.asks.load(Ordering::SeqCst),
            1,
            "Pay parked once under Always"
        );
        assert!(events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Permission { name, .. } if name == "click")));
        let outputs: Vec<(String, String)> = history
            .iter()
            .filter_map(|item| match item {
                InputItem::FunctionCallOutput { call_id, output } => {
                    Some((call_id.clone(), output.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            outputs,
            vec![
                ("1".to_string(), "ok".to_string()),
                ("2".to_string(), gate::user_rejected("click")),
                (
                    "3".to_string(),
                    "Not executed: earlier action failed".to_string()
                ),
                (
                    "4".to_string(),
                    "Not executed: earlier action failed".to_string()
                ),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Card 18: the desktop reports Slack has focus, so Enter there is a
    /// send and parks under Always; Deny keeps it from being pressed. The
    /// same Enter with no window hint is a plain key.
    struct InSlack(Spy);

    impl DesktopOps for InSlack {
        fn halted(&self) -> bool {
            false
        }
        fn locked(&self) -> bool {
            false
        }
        fn call(&self, name: &str, args: &serde_json::Value) -> ToolOutput {
            assert!(
                args.get("window").is_none(),
                "the gate's hint never reaches the desktop"
            );
            self.0.call(name, args)
        }
        fn hint(&self, _name: &str, args: &serde_json::Value) -> serde_json::Value {
            let mut hinted = args.clone();
            hinted["window"] = "Slack | general | Acme".into();
            hinted
        }
    }

    #[test]
    fn enter_in_a_chat_app_parks_by_the_window_the_desktop_reports() {
        let dir = workspace("enter-slack");
        let script = || Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("1", "key", r#"{"keys":"Return"}"#)],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let slack = InSlack(Spy {
            calls: AtomicUsize::new(0),
            halted: false,
            locked: false,
        });
        let deny = Answer {
            answer: PermAnswer::Deny,
            asks: AtomicUsize::new(0),
        };
        let (out, history, events) = once(
            &script(),
            &dir,
            chat(gate::PermMode::Always, true, true),
            Some(&slack),
            &deny,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(
            deny.asks.load(Ordering::SeqCst),
            1,
            "Enter in Slack parked under Always"
        );
        assert_eq!(
            slack.0.calls.load(Ordering::SeqCst),
            0,
            "the denied Enter was never pressed"
        );
        assert!(events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Permission { name, .. } if name == "key")));
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { call_id, output } if call_id == "1" && *output == gate::user_rejected("key")
        )));
        let plain = Spy {
            calls: AtomicUsize::new(0),
            halted: false,
            locked: false,
        };
        let (_, _, _) = once(
            &script(),
            &dir,
            chat(gate::PermMode::Always, true, true),
            Some(&plain),
            &deny,
        );
        assert_eq!(
            deny.asks.load(Ordering::SeqCst),
            1,
            "no window, no ask: Enter is a plain key"
        );
        assert_eq!(plain.calls.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn desktop_tools_follow_the_switch_halt_and_lock() {
        let dir = workspace("desk");
        let shot = |id: &str| ScriptTurn {
            text: String::new(),
            calls: vec![call(id, "screenshot", "{}")],
            usage: Usage::default(),
            truncated: false,
        };
        let done = ScriptTurn {
            text: "done".into(),
            calls: Vec::new(),
            usage: Usage::default(),
            truncated: false,
        };
        let permits = Answer {
            answer: PermAnswer::Allow,
            asks: AtomicUsize::new(0),
        };

        let off_script = Script {
            turns: Mutex::new(vec![shot("s"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let spy = Spy {
            calls: AtomicUsize::new(0),
            halted: false,
            locked: false,
        };
        let (out, history, _) = once(
            &off_script,
            &dir,
            chat(gate::PermMode::Always, true, false),
            Some(&spy),
            &permits,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(spy.calls.load(Ordering::SeqCst), 0);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. }
                if output.contains(grokhub_core::desktop_mcp::OFF_MSG)
        )));
        let body = crate::responses_body(&crate::ResponsesRequest {
            model: "grok-4.7".into(),
            effort: None,
            input: Vec::new(),
            conversation_id: "c".into(),
            tools: tools::schemas_for(&chat(gate::PermMode::Always, true, false)),
            hosted_search: true,
            call_timeout: None,
        });
        assert!(body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["name"] != "screenshot"));

        let halt_script = Script {
            turns: Mutex::new(vec![shot("h"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let halted = Spy {
            calls: AtomicUsize::new(0),
            halted: true,
            locked: false,
        };
        let (_, history, _) = once(
            &halt_script,
            &dir,
            chat(gate::PermMode::Always, true, true),
            Some(&halted),
            &permits,
        );
        assert_eq!(halted.calls.load(Ordering::SeqCst), 0);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. }
                if output.contains(grokhub_core::desktop_mcp::HALT_MSG)
        )));

        let lock_script = Script {
            turns: Mutex::new(vec![shot("l"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let locked = Spy {
            calls: AtomicUsize::new(0),
            halted: false,
            locked: true,
        };
        let (_, history, _) = once(
            &lock_script,
            &dir,
            chat(gate::PermMode::Always, true, true),
            Some(&locked),
            &permits,
        );
        assert_eq!(locked.calls.load(Ordering::SeqCst), 0);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. }
                if output.contains(grokhub_core::desktop_mcp::LOCK_MSG)
        )));

        let run_script = Script {
            turns: Mutex::new(vec![shot("r"), done]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let live = Spy {
            calls: AtomicUsize::new(0),
            halted: false,
            locked: false,
        };
        let (_, history, _) = once(
            &run_script,
            &dir,
            chat(gate::PermMode::Always, true, true),
            Some(&live),
            &permits,
        );
        assert_eq!(live.calls.load(Ordering::SeqCst), 1);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::Message { content, .. }
                if content.iter().any(|part| matches!(part, ContentPart::InputText(text) if text == "image from screenshot"))
        )));
        let tools = tools::schemas_for(&chat(gate::PermMode::Always, true, true));
        assert!(tools.iter().any(|tool| tool["name"] == "screenshot"));
        assert!(tools.iter().all(|tool| tool["name"] != "list_monitors"));
        let plan = tools::schemas_for(&Gate::phase_readonly());
        assert!(plan
            .iter()
            .all(|tool| tool["name"] != "write" && tool["name"] != "screenshot"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    fn notice_count(items: &[InputItem]) -> usize {
        items
            .iter()
            .filter(|item| {
                matches!(
                    item,
                    InputItem::Message { content, .. }
                        if content.iter().any(|part| matches!(part, ContentPart::InputText(text) if text.contains("<background-notice>")))
                )
            })
            .count()
    }

    #[cfg(unix)]
    fn task_id_in(items: &[InputItem]) -> Option<String> {
        for item in items.iter().rev() {
            let InputItem::FunctionCallOutput { output, .. } = item else {
                continue;
            };
            for line in output.lines() {
                if let Some(rest) = line.trim().strip_prefix("task id:") {
                    let id = rest.trim();
                    if !id.is_empty() {
                        return Some(id.to_string());
                    }
                }
            }
        }
        None
    }

    #[cfg(unix)]
    struct NoticeClient {
        n: AtomicUsize,
        seen: Mutex<Vec<Vec<InputItem>>>,
    }

    #[cfg(unix)]
    impl ModelClient for NoticeClient {
        fn stream(
            &self,
            req: &crate::ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(crate::StreamEvent),
        ) -> Result<crate::TurnOutput, crate::ClientError> {
            self.seen.lock().unwrap().push(req.input.clone());
            let n = self.n.fetch_add(1, Ordering::SeqCst);
            let calls = if n == 0 {
                vec![call(
                    "bg",
                    "run_terminal_command",
                    r#"{"command":"echo hello-notice","is_background":true}"#,
                )]
            } else if n == 1 {
                let id = task_id_in(&req.input).unwrap_or_else(|| "missing".into());
                vec![call(
                    "out",
                    "get_command_or_subagent_output",
                    &format!(r#"{{"task_id":"{id}","timeout_ms":8000}}"#),
                )]
            } else if n == 2 {
                vec![call("r", "read_file", r#"{"target_file":"note.txt"}"#)]
            } else {
                Vec::new()
            };
            Ok(crate::TurnOutput {
                text: if calls.is_empty() {
                    "done".into()
                } else {
                    String::new()
                },
                reasoning: String::new(),
                calls,
                usage: Usage::default(),
                ..Default::default()
            })
        }
    }

    #[cfg(unix)]
    #[test]
    fn notice_is_injected_once_on_the_next_turn() {
        let dir = workspace("notice");
        let sid = format!("notice-{}", std::process::id());
        let hub = crate::tasks::hub_for(&sid);
        hub.reopen();
        let client = NoticeClient {
            n: AtomicUsize::new(0),
            seen: Mutex::new(Vec::new()),
        };
        let input = LoopIn {
            client: &client,
            workspace: &dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: &sid,
            max_turns: 6,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &NeverHalt,
            gate: chat(gate::PermMode::Always, true, false),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: Some(hub),
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |_| {});
        assert_eq!(out.stop, StopReason::EndTurn);
        let seen = client.seen.lock().unwrap();
        assert!(seen.len() >= 4, "turns {}", seen.len());
        assert_eq!(notice_count(&seen[0]), 0, "the first request has no notice");
        assert!(seen.iter().all(|items| notice_count(items) <= 1));
        assert_eq!(notice_count(&seen[2]), 1, "{:?}", seen[2]);
        assert_eq!(
            notice_count(&seen[3]),
            1,
            "a later turn must not copy the notice again"
        );
        let blob = format!("{:?}", seen[3]);
        assert!(blob.contains("hello-notice"), "{blob}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn steer_lands_at_the_next_tool_boundary() {
        let dir = workspace("steer-boundary");
        let steer = SteerQueue::new();
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("r", "read_file", r#"{"target_file":"note.txt"}"#)],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: "after steer".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: Some(steer.clone()),
        };
        let _ = run(&script, &dir, 4, &CancelToken::new(), &steer, &NeverHalt);
        let seen = script.seen.lock().unwrap();
        let first = format!("{:?}", seen.first());
        assert!(!first.contains("please look again"), "{first}");
        let second = format!("{:?}", seen.get(1));
        let output_at = second.find("FunctionCallOutput").expect(&second);
        let steer_at = second.find("please look again").expect(&second);
        assert!(output_at < steer_at, "{second}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn halt_stops_the_run_and_its_tasks_and_monitors() {
        let dir = workspace("halt-tasks");
        let sid = format!(
            "halt-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let hub = crate::tasks::hub_for(&sid);
        hub.reopen();
        let task = hub
            .spawn(&dir, "sleep 30 & echo $! > child.pid; wait")
            .expect("spawn");
        let _mon = hub
            .start_monitor(&dir, None, Some(task), None, 60_000, "watch".into())
            .expect("monitor");
        let pidfile = dir.join("child.pid");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut pid = 0i32;
        while std::time::Instant::now() < deadline {
            if let Ok(text) = std::fs::read_to_string(&pidfile) {
                if let Ok(n) = text.trim().parse::<i32>() {
                    if n > 1 {
                        pid = n;
                        break;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(pid > 1, "child pid was not written");
        let script = Script {
            turns: Mutex::new(vec![ScriptTurn {
                text: String::new(),
                calls: vec![call("r", "read_file", r#"{"target_file":"note.txt"}"#)],
                usage: Usage::default(),
                truncated: false,
            }]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let input = LoopIn {
            client: &script,
            workspace: &dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: &sid,
            max_turns: 4,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &HaltAfter(AtomicUsize::new(0)),
            gate: Gate::phase_readonly(),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: Some(hub.clone()),
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |_| {});
        assert_eq!(out.stop, StopReason::Halted);
        assert!(hub.is_halted());
        assert!(hub.spawn(&dir, "echo still").is_err());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            let alive = unsafe { libc::kill(pid, 0) } == 0;
            if !alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(!alive, "child {pid} still alive");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut notes = Vec::new();
        while std::time::Instant::now() < deadline {
            notes.extend(hub.drain_notices());
            if notes
                .iter()
                .any(|note| note.contains("cancelled") || note.contains("killed"))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            notes
                .iter()
                .any(|note| note.contains("cancelled") || note.contains("killed")),
            "{notes:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bg_ask_refuses_non_readonly_tools() {
        let dir = workspace("bg-ask");
        let cfg = std::env::temp_dir().join(format!("gh-bg-ask-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(&cfg).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        let asks = Answer {
            answer: PermAnswer::Deny,
            asks: AtomicUsize::new(0),
        };
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![
                        call("w", "write", r#"{"path":"no.txt","content":"x"}"#),
                        call(
                            "s",
                            "run_terminal_command",
                            r#"{"command":"echo ran > ran.txt"}"#,
                        ),
                        call("k", "kill_command_or_subagent", r#"{"task_id":"t1"}"#),
                        call(
                            "c",
                            "scheduler_create",
                            r#"{"interval":"10m","prompt":"nope"}"#,
                        ),
                        call("m", "monitor", r#"{"command":"echo hi"}"#),
                    ],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let sid = format!("bg-ask-{}", std::process::id());
        let hub = crate::tasks::hub_for(&sid);
        hub.reopen();
        let input = LoopIn {
            client: &script,
            workspace: &dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: &sid,
            max_turns: 4,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &NeverHalt,
            gate: chat(gate::PermMode::Ask, false, false),
            desktop: None,
            permits: &asks,
            perms: None,
            context_length: 0,
            tasks: Some(hub),
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let mut events = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |ev| events.push(ev));
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(asks.asks.load(Ordering::SeqCst), 0);
        assert!(events
            .iter()
            .all(|ev| !matches!(ev, LoopEvent::Permission { .. })));
        for name in [
            "write",
            "run_terminal_command",
            "kill_command_or_subagent",
            "scheduler_create",
            "monitor",
        ] {
            assert!(
                history.iter().any(|item| matches!(
                    item,
                    InputItem::FunctionCallOutput { output, .. }
                        if output.contains(name) && output.contains("was not executed")
                )),
                "{name} missing from {history:?}"
            );
        }
        assert!(!dir.join("no.txt").exists());
        assert!(!dir.join("ran.txt").exists());
        assert!(!cfg.join("automations.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn pre_tool_hook_deny_blocks_and_allow_does_not_skip_ask() {
        let dir = workspace("hook-gate");
        let session = format!("hook-gate-{}", std::process::id());
        let deny = crate::hooks::install_test_hooks(
            &session,
            vec![crate::hooks::TestHook {
                event: "PreToolUse".into(),
                matcher: "Write".into(),
                command: crate::hooks::test_echo(r#"{"decision":"deny","reason":"nope"}"#, None),
                timeout: std::time::Duration::from_secs(5),
                source_dir: dir.clone(),
            }],
        );
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("w", "write", r#"{"path":"out.txt","content":"yes"}"#)],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let input = LoopIn {
            client: &script,
            workspace: &dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: &session,
            max_turns: 4,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &NeverHalt,
            gate: chat(gate::PermMode::Always, true, false),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let mut events = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |ev| events.push(ev));
        assert_eq!(out.stop, StopReason::EndTurn);
        assert!(!dir.join("out.txt").exists());
        assert!(
            history.iter().any(|item| matches!(
                item,
                InputItem::FunctionCallOutput { output, .. } if output.contains("nope")
            )),
            "{history:?}"
        );
        drop(deny);

        let allow_session = format!("{session}-allow");
        let allow = crate::hooks::install_test_hooks(
            &allow_session,
            vec![crate::hooks::TestHook {
                event: "PreToolUse".into(),
                matcher: String::new(),
                command: crate::hooks::test_echo(r#"{"decision":"allow"}"#, None),
                timeout: std::time::Duration::from_secs(5),
                source_dir: dir.clone(),
            }],
        );
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("w", "write", r#"{"path":"out.txt","content":"yes"}"#)],
                    usage: Usage::default(),
                    truncated: false,
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    truncated: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let input = LoopIn {
            client: &script,
            workspace: &dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: &allow_session,
            max_turns: 4,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &NeverHalt,
            gate: chat(gate::PermMode::Ask, true, false),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |_ev| {});
        assert_eq!(out.stop, StopReason::EndTurn);
        assert!(
            !dir.join("out.txt").exists(),
            "hook allow must not skip the ask gate"
        );
        assert!(
            history.iter().any(|item| matches!(
                item,
                InputItem::FunctionCallOutput { output, .. } if output.contains("rejected")
            )),
            "{history:?}"
        );
        drop(allow);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn run_drive(
        script: &Script,
        dir: &std::path::Path,
        conv: &str,
    ) -> (LoopOut, Vec<InputItem>, Vec<LoopEvent>) {
        run_drive_in(script, dir, conv, Gate::phase_readonly())
    }

    fn run_drive_in(
        script: &Script,
        dir: &std::path::Path,
        conv: &str,
        gate: Gate,
    ) -> (LoopOut, Vec<InputItem>, Vec<LoopEvent>) {
        let (cancel, steer) = (CancelToken::new(), SteerQueue::new());
        let input = LoopIn {
            client: script,
            workspace: dir,
            model: "grok-4.7",
            effort: Some("low"),
            system: "sys",
            conversation_id: conv,
            max_turns: 0,
            usage_base: Usage::default(),
            cancel: &cancel,
            steer: &steer,
            halt: &NeverHalt,
            gate,
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let mut events = Vec::new();
        let out = run_loop(&input, &mut history, "scan my computer", None, &mut |ev| {
            events.push(ev)
        });
        (out, history, events)
    }

    fn turn(text: &str, calls: Vec<FunctionCall>) -> ScriptTurn {
        ScriptTurn {
            text: text.into(),
            calls,
            usage: Usage::default(),
            truncated: false,
        }
    }

    fn user_texts(history: &[InputItem]) -> Vec<String> {
        history
            .iter()
            .filter_map(|item| match item {
                InputItem::Message { role, content } if role == "user" => match content.first() {
                    Some(ContentPart::InputText(t)) => Some(t.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_run_that_stops_early_is_driven_until_the_checklist_is_covered() {
        let dir = workspace("drive");
        let conv = format!("drive-{}", std::process::id());
        let todos = r#"{"todos":[{"id":"svc","content":"failed services","status":"completed"},{"id":"ports","content":"open ports","status":"pending"}]}"#;
        let script = Script {
            turns: Mutex::new(vec![
                turn(
                    "",
                    vec![
                        call("t1", "todo_write", todos),
                        call("r1", "read_file", r#"{"target_file":"note.txt"}"#),
                    ],
                ),
                // Claims it's resolved with "open ports" still open.
                turn("It's resolved.", Vec::new()),
                turn(
                    "",
                    vec![
                        call("r2", "list_dir", "{}"),
                        call(
                            "t2",
                            "todo_write",
                            r#"{"merge":true,"todos":[{"id":"ports","status":"completed"}]}"#,
                        ),
                    ],
                ),
                turn("All covered.", Vec::new()),
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 4);
        assert_eq!(
            user_texts(&history),
            [
                "scan my computer".to_string(),
                "Not done yet: \"open ports\". Continue with the next open item. Mark an item completed only after a tool result shows it. If one is truly blocked (a hard approval was refused, hardware or access is missing, or every approach failed), mark it cancelled and say why.".to_string(),
            ]
        );
        crate::session_tools::invalidate_todos(&conv);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_checklist_marked_done_without_any_check_is_pushed_back() {
        let dir = workspace("drive-proof");
        let conv = format!("drive-proof-{}", std::process::id());
        let todos = r#"{"todos":[{"id":"fw","content":"firewall","status":"completed"}]}"#;
        let script = Script {
            turns: Mutex::new(vec![
                turn("", vec![call("t1", "todo_write", todos)]),
                turn("Firewall is fine.", Vec::new()),
                turn(
                    "",
                    vec![call("r1", "read_file", r#"{"target_file":"note.txt"}"#)],
                ),
                turn("Checked: fine.", Vec::new()),
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 4);
        assert_eq!(
            user_texts(&history)[1],
            "Marked completed without any check this run: \"firewall\". Run a check that shows each one before you finish."
        );
        crate::session_tools::invalidate_todos(&conv);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truly_stuck_run_ends_naming_what_is_blocked() {
        let dir = workspace("drive-stuck");
        let conv = format!("drive-stuck-{}", std::process::id());
        let todos = r#"{"todos":[{"id":"bt","content":"bluetooth errors","status":"pending"}]}"#;
        let mut turns = vec![turn("", vec![call("t1", "todo_write", todos)])];
        turns.extend((0..=crate::drive::STALL_LIMIT).map(|_| turn("I can't.", Vec::new())));
        let script = Script {
            turns: Mutex::new(turns),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, events) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 5);
        let said = "Not done: \"bluetooth errors\" is still open. I stopped after 3 re-plans without progress, so none of it is marked done. If this needs something GrokHub can't do yet, I can note it as a feature request instead.";
        assert!(events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Text(t) if t == &format!("\n\n{said}"))));
        assert!(
            matches!(history.last(), Some(InputItem::Message { role, content }) if role == "assistant" && content.first() == Some(&ContentPart::InputText(said.into())))
        );
        crate::session_tools::invalidate_todos(&conv);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_open_checklist_from_an_earlier_run_does_not_take_over_the_next_ask() {
        let dir = workspace("drive-bleed");
        let conv = format!("drive-bleed-{}", std::process::id());
        let todos = r#"{"todos":[{"id":"bt","content":"bluetooth errors","status":"pending"}]}"#;
        let mut turns = vec![turn("", vec![call("t1", "todo_write", todos)])];
        turns.extend((0..=crate::drive::STALL_LIMIT).map(|_| turn("I can't.", Vec::new())));
        let script = Script {
            turns: Mutex::new(turns),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, _, _) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 5);

        // The checklist is still open; the next, unrelated ask is answered once.
        let script = Script {
            turns: Mutex::new(vec![
                turn("It is 4 pm.", Vec::new()),
                turn("unused", Vec::new()),
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 1);
        assert_eq!(user_texts(&history), ["scan my computer"]);
        crate::session_tools::invalidate_todos(&conv);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn report_findings_posts_the_card_after_the_reply() {
        let dir = workspace("findings");
        let conv = format!("findings-{}", std::process::id());
        let args = r#"{"findings":[{"severity":"high","text":"nextdns can't bind port 53: systemd-resolved owns it"}],"fixes":[{"label":"Fix port 53 conflict","goal":"Free port 53 for nextdns."}]}"#;
        let script = Script {
            turns: Mutex::new(vec![
                turn(
                    "",
                    vec![
                        call("f1", "report_findings", args),
                        call("f2", "report_findings", r#"{"findings":[]}"#),
                    ],
                ),
                turn("Found one problem.", Vec::new()),
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, events) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        let cards: Vec<&String> = events
            .iter()
            .filter_map(|ev| match ev {
                LoopEvent::Findings(b) => Some(b),
                _ => None,
            })
            .collect();
        assert_eq!(
            cards,
            ["Findings from this run\n\n- High: nextdns can't bind port 53: systemd-resolved owns it\n\nOne-tap fixes\n\n- Fix port 53 conflict: Free port 53 for nextdns."]
        );
        let outputs: Vec<&str> = history
            .iter()
            .filter_map(|i| match i {
                InputItem::FunctionCallOutput { output, .. } => Some(output.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            outputs,
            [
                "Findings card posted: 1 findings, 1 one-tap fixes. It shows after your reply.",
                "give 1 to 30 findings"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn brief(
        intent: crate::brain::Intent,
        care: crate::brain::Care,
        done: &[&str],
    ) -> crate::brain::Brief {
        crate::brain::Brief {
            intent,
            care,
            done: done.iter().map(|d| d.to_string()).collect(),
            ..Default::default()
        }
    }

    fn scripted(turns: Vec<ScriptTurn>) -> Script {
        Script {
            turns: Mutex::new(turns),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        }
    }

    #[test]
    fn an_edit_then_stop_is_pushed_back_until_it_is_checked() {
        let dir = workspace("verify-stop");
        let conv = format!("verify-stop-{}", std::process::id());
        crate::checkpoint::set_turn_brief(
            &conv,
            brief(crate::brain::Intent::Task, crate::brain::Care::Casual, &[]),
        );
        let script = scripted(vec![
            turn(
                "",
                vec![call(
                    "w1",
                    "write",
                    r#"{"path":"notes.txt","content":"hi"}"#,
                )],
            ),
            turn("Done.", Vec::new()),
            turn(
                "",
                vec![call("r1", "read_file", r#"{"target_file":"notes.txt"}"#)],
            ),
            turn("Checked: it says hi.", Vec::new()),
        ]);
        let (out, history, _) = run_drive_in(
            &script,
            &dir,
            &conv,
            chat(gate::PermMode::Always, true, false),
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 4);
        assert_eq!(
            user_texts(&history),
            [
                "scan my computer".to_string(),
                "Not verified since your last change to notes.txt. Check it (run the tests or build, re-read the file, look at the result) before you finish.".to_string(),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_edit_then_a_test_run_then_stop_ends() {
        let dir = workspace("verify-tests");
        let conv = format!("verify-tests-{}", std::process::id());
        crate::checkpoint::set_turn_brief(
            &conv,
            brief(crate::brain::Intent::Task, crate::brain::Care::Casual, &[]),
        );
        let script = scripted(vec![
            turn(
                "",
                vec![call(
                    "w1",
                    "write",
                    r#"{"path":"notes.txt","content":"hi"}"#,
                )],
            ),
            turn(
                "",
                vec![call(
                    "s1",
                    "run_terminal_command",
                    r#"{"command":"echo test ok"}"#,
                )],
            ),
            turn("Done, and the test run passed.", Vec::new()),
            turn("unused", Vec::new()),
        ]);
        let (out, history, _) = run_drive_in(
            &script,
            &dir,
            &conv,
            chat(gate::PermMode::Always, true, false),
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 3);
        assert_eq!(user_texts(&history), ["scan my computer"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_careful_repair_must_cover_its_brief_done_list_or_end_naming_it() {
        let dir = workspace("brief-done");
        let conv = format!("brief-done-{}", std::process::id());
        let b = brief(
            crate::brain::Intent::Repair,
            crate::brain::Care::Careful,
            &["the error is gone", "it works again"],
        );
        crate::checkpoint::set_turn_brief(&conv, b.clone());
        // Covers the list and posts its findings card: ends after one push.
        let todos = r#"{"todos":[{"id":"e","content":"Confirm the error is gone","status":"completed"},{"id":"w","content":"It works again after restart","status":"completed"}]}"#;
        let card =
            r#"{"findings":[{"severity":"info","text":"The error is gone after the restart"}]}"#;
        let script = scripted(vec![
            turn(
                "",
                vec![call("r1", "read_file", r#"{"target_file":"note.txt"}"#)],
            ),
            turn("Fixed.", Vec::new()),
            turn(
                "",
                vec![
                    call("t1", "todo_write", todos),
                    call("r2", "list_dir", "{}"),
                    call("f1", "report_findings", card),
                ],
            ),
            turn("Both checked.", Vec::new()),
        ]);
        let (out, history, events) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 4);
        assert_eq!(
            user_texts(&history)[1],
            "This ask is done only when it shows \"the error is gone\", \"it works again\". Put each in your checklist (todo_write) and mark it completed once a tool result shows it, or cancelled with the reason it is blocked. \
             End with the findings card: call report_findings naming each item you checked and its root cause, with a one-tap fix for what is left."
        );
        let cards: Vec<&LoopEvent> = events
            .iter()
            .filter(|e| matches!(e, LoopEvent::Findings(_)))
            .collect();
        assert_eq!(
            cards,
            [&LoopEvent::Findings(
                "Findings from this run\n\n- Info: The error is gone after the restart".into()
            )]
        );
        crate::session_tools::invalidate_todos(&conv);
        // Truly blocked: ends naming what is still open.
        let conv = format!("brief-stuck-{}", std::process::id());
        crate::checkpoint::set_turn_brief(&conv, b);
        let mut turns = vec![turn(
            "",
            vec![call("r1", "read_file", r#"{"target_file":"note.txt"}"#)],
        )];
        turns.extend((0..=crate::drive::STALL_LIMIT).map(|_| turn("I can't.", Vec::new())));
        let script = scripted(turns);
        let (out, history, events) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 5);
        let said = "Not done: \"the error is gone\", \"it works again\", the findings card are still open. I stopped after 3 re-plans without progress, so none of it is marked done. If this needs something GrokHub can't do yet, I can note it as a feature request instead.";
        assert!(
            matches!(history.last(), Some(InputItem::Message { role, content }) if role == "assistant" && content.first() == Some(&ContentPart::InputText(said.into())))
        );
        // It still ends with a findings card naming each item, and a one-tap fix for the rest.
        let card = "Findings from this run\n\n- Medium: Not shown yet: the error is gone\n- Medium: Not shown yet: it works again\n\n\
                    One-tap fixes\n\n- Finish the rest: Finish what the last run left open: the error is gone; it works again.";
        assert_eq!(events.last(), Some(&LoopEvent::Findings(card.into())));
        crate::session_tools::invalidate_todos(&conv);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Replay of a real feature ask that faked "Done" and then looped: the run
    /// must not end on a done claim, must end once, after at most 3 re-plans.
    #[test]
    fn replay_a_feature_ask_that_faked_done_ends_blocked_once() {
        let dir = workspace("fake-done");
        let conv = format!("fake-done-{}", std::process::id());
        crate::checkpoint::set_turn_brief(
            &conv,
            brief(crate::brain::Intent::Task, crate::brain::Care::Careful, &[]),
        );
        let list = |extra: &str| {
            format!(
                r#"{{"todos":[{{"id":"scan","content":"Scan the Steam library","status":"completed"}},{{"id":"wire","content":"Use it for Pulse","status":"completed"}}{extra}]}}"#
            )
        };
        let probe = |id: &str, content: &str| {
            format!(r#",{{"id":"{id}","content":"{content}","status":"pending"}}"#)
        };
        let mut turns = vec![
            turn(
                "",
                vec![call(
                    "t0",
                    "todo_write",
                    r#"{"todos":[{"id":"scan","content":"Scan the Steam library","status":"pending"},{"id":"wire","content":"Use it for Pulse","status":"pending"}]}"#,
                )],
            ),
            turn("", vec![call("t1", "todo_write", &list(""))]),
            turn("Pulse feed updated. <b>Done.</b> ✨", Vec::new()),
        ];
        for (n, (id, content, said)) in [
            ("p1", "Check skill_list", "Error gone."),
            ("p2", "Check scheduler_list", "Blocked permanently."),
            ("p3", "Check automation_list", "Error gone."),
            ("p4", "Diagnose image folders", "Blocked permanently."),
        ]
        .iter()
        .enumerate()
        {
            turns.push(turn(
                "",
                vec![
                    call(&format!("l{n}"), "list_dir", "{}"),
                    call(&format!("w{n}"), "todo_write", &list(&probe(id, content))),
                ],
            ));
            turns.push(turn(said, Vec::new()));
        }
        turns.push(turn("unused", Vec::new()));
        let script = scripted(turns);
        let (out, history, events) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 11);
        // The ask, then one push per stop before the last: the fake Done, then 3 re-plans.
        assert_eq!(user_texts(&history).len(), 5);
        assert_eq!(
            user_texts(&history)[1],
            "Marked completed without any check this run: \"Scan the Steam library\", \"Use it for Pulse\". Run a check that shows each one before you finish."
        );
        let said = "Not done: \"Diagnose image folders\" is still open. I stopped after 3 re-plans without progress, so none of it is marked done. If this needs something GrokHub can't do yet, I can note it as a feature request instead.";
        assert!(
            matches!(history.last(), Some(InputItem::Message { role, content }) if role == "assistant" && content.first() == Some(&ContentPart::InputText(said.into())))
        );
        let status_lines = events
            .iter()
            .filter(|ev| matches!(ev, LoopEvent::Text(t) if t.starts_with("\n\nNot done:")))
            .count();
        assert_eq!(status_lines, 1);
        crate::session_tools::invalidate_todos(&conv);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_change_request_that_claims_done_with_no_tool_run_says_it_is_not_done() {
        let dir = workspace("claim-done");
        let conv = format!("claim-done-{}", std::process::id());
        crate::checkpoint::set_turn_brief(
            &conv,
            brief(crate::brain::Intent::Task, crate::brain::Care::Careful, &[]),
        );
        let script = scripted(vec![
            turn("Pulse feed updated. Done. ✨", Vec::new()),
            turn("unused", Vec::new()),
        ]);
        let (out, history, events) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(script.seen.lock().unwrap().len(), 1);
        let line = "That isn't done yet: nothing ran this turn to make or check the change.";
        assert_eq!(
            events
                .iter()
                .filter(|ev| matches!(ev, LoopEvent::Text(t) if t == &format!("\n\n{line}")))
                .count(),
            1
        );
        assert!(
            matches!(history.last(), Some(InputItem::Message { role, content }) if role == "assistant" && content.first() == Some(&ContentPart::InputText(line.into())))
        );
        crate::session_tools::invalidate_todos(&conv);
        // A plain question with the same words gets no extra line.
        let conv = format!("claim-done-q-{}", std::process::id());
        crate::checkpoint::set_turn_brief(
            &conv,
            brief(
                crate::brain::Intent::Question,
                crate::brain::Care::Casual,
                &[],
            ),
        );
        let script = scripted(vec![turn("Done: it is 4 pm.", Vec::new())]);
        let (_, _, events) = run_drive(&script, &dir, &conv);
        assert!(!events
            .iter()
            .any(|ev| matches!(ev, LoopEvent::Text(t) if t.contains("isn't done yet"))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One turn of a talk-style run, continuing `history`.
    fn talk_turn(
        script: &Script,
        dir: &std::path::Path,
        conv: &str,
        system: &str,
        gate: Gate,
        permits: &dyn PermitWait,
        history: &mut Vec<InputItem>,
        text: &str,
    ) -> (LoopOut, Vec<LoopEvent>) {
        let (cancel, steer) = (CancelToken::new(), SteerQueue::new());
        let input = LoopIn {
            client: script,
            workspace: dir,
            model: "grok-4.7",
            effort: Some("low"),
            system,
            conversation_id: conv,
            max_turns: 0,
            usage_base: Usage::default(),
            cancel: &cancel,
            steer: &steer,
            halt: &NeverHalt,
            gate,
            desktop: None,
            permits,
            perms: None,
            context_length: 0,
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let mut events = Vec::new();
        let out = run_loop(&input, history, text, None, &mut |ev| events.push(ev));
        (out, events)
    }

    fn system_text(history: &[InputItem]) -> String {
        match history.first() {
            Some(InputItem::Message { role, content }) if role == "system" => match content.first()
            {
                Some(ContentPart::InputText(t)) => t.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }

    #[test]
    fn casual_chat_gets_the_short_style_and_a_later_repair_swaps_it_in_place() {
        use grokhub_core::talk::{TalkStyle, VOLATILE_SAFETY_CLOSER};
        let dir = workspace("talk-short");
        let conv = format!("talk-short-{}", std::process::id());
        let voice = grokhub_core::personality::builtin("pirate").prompt_block();
        let system = format!("CABIN RULES\n\n{voice}\n\n{VOLATILE_SAFETY_CLOSER}");
        crate::checkpoint::set_turn_style(&conv, Some(TalkStyle::Short));
        let script = scripted(vec![
            turn("Ahoy! All calm here. What be next?", Vec::new()),
            turn("Fixed.", Vec::new()),
        ]);
        let mut history = Vec::new();
        let (out, events) = talk_turn(
            &script,
            &dir,
            &conv,
            &system,
            Gate::phase_readonly(),
            &gate::ClosedPermits,
            &mut history,
            "hi there",
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        let short = format!(
            "CABIN RULES\n\n{voice}\n\n\
             Talk style for this turn: casual chat. Reply in one to three short sentences, under 60 words, with no headings, lists or preamble.\n\
             Talk style sets length and shape, and its length wins over a personality's; a personality sets tone only. Accuracy and care beat those word caps, so say what is true and what is careful even when that runs past the cap. Approval questions, security warnings and error facts stay plain, and the cabin safety rules win.\n\n\
             {VOLATILE_SAFETY_CLOSER}"
        );
        let seen = script.seen.lock().unwrap()[0].clone();
        assert_eq!(system_text(&seen), short);
        // The reply the model gave fits the short style's cap.
        let reply: String = events
            .iter()
            .filter_map(|e| {
                if let LoopEvent::Text(t) = e {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(reply, "Ahoy! All calm here. What be next?");
        assert!(grokhub_core::talk::fits_cap(TalkStyle::Short, &reply));
        // The next turn is a repair: the stored system message swaps the block, nothing else.
        crate::checkpoint::set_turn_style(&conv, Some(TalkStyle::Findings));
        let _ = talk_turn(
            &script,
            &dir,
            &conv,
            &system,
            Gate::phase_readonly(),
            &gate::ClosedPermits,
            &mut history,
            "wifi broke",
        );
        let repair = short.replace(&TalkStyle::Short.block(), &TalkStyle::Findings.block());
        assert_eq!(system_text(&script.seen.lock().unwrap()[1]), repair);
        crate::checkpoint::set_turn_style(&conv, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two turns whose talk style changes in between, through the real loop.
    fn two_styled_turns(tag: &str, prefix_cache: bool) -> String {
        use grokhub_core::talk::TalkStyle;
        let dir = workspace(tag);
        let conv = format!("{tag}-{}", std::process::id());
        crate::prefix_cache::set_enabled_for_tests(Some(prefix_cache));
        crate::checkpoint::set_turn_style(&conv, Some(TalkStyle::Short));
        let script = scripted(vec![turn("Hi.", Vec::new()), turn("Fixed.", Vec::new())]);
        let mut history = Vec::new();
        for (text, style) in [
            ("hi", TalkStyle::Short),
            ("wifi broke", TalkStyle::Findings),
        ] {
            crate::checkpoint::set_turn_style(&conv, Some(style));
            let (out, _) = talk_turn(
                &script,
                &dir,
                &conv,
                "CABIN RULES",
                Gate::phase_readonly(),
                &gate::ClosedPermits,
                &mut history,
                text,
            );
            assert_eq!(out.stop, StopReason::EndTurn);
        }
        crate::checkpoint::set_turn_style(&conv, None);
        crate::prefix_cache::set_enabled_for_tests(None);
        let _ = std::fs::remove_dir_all(&dir);
        crate::prefix_guard::run_key(&conv, 0, None)
    }

    #[test]
    fn with_prefix_cache_a_style_change_keeps_the_prefix_append_only() {
        let key = two_styled_turns("guard-cache-on", true);
        crate::prefix_guard::assert_no_unexpected_break(&key);
        assert_eq!(crate::prefix_guard::breaks(&key), Vec::new());
    }

    #[test]
    fn without_prefix_cache_a_style_change_is_reported_as_a_system_rewrite() {
        let key = two_styled_turns("guard-cache-off", false);
        assert_eq!(
            crate::prefix_guard::breaks(&key),
            vec![crate::prefix_guard::PrefixBreak {
                index: 0,
                part: crate::prefix_guard::Part::System,
                reason: crate::prefix_guard::Reason::Unexpected,
            }]
        );
    }

    #[test]
    fn a_long_task_gets_progress_notes_only_at_phase_boundaries() {
        let dir = workspace("talk-progress");
        for d in ["a", "b", "c", "d", "e", "f"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let conv = format!("talk-progress-{}", std::process::id());
        crate::checkpoint::set_turn_brief(
            &conv,
            brief(crate::brain::Intent::Task, crate::brain::Care::Careful, &[]),
        );
        crate::checkpoint::set_turn_style(&conv, Some(grokhub_core::talk::TalkStyle::Progress));
        let ls = |id: &str, d: &str| call(id, "list_dir", &format!(r#"{{"path":"{d}"}}"#));
        let mark = |id: &str, item: &str| {
            call(
                id,
                "todo_write",
                &format!(r#"{{"merge":true,"todos":[{{"id":"{item}","status":"completed"}}]}}"#),
            )
        };
        let plan = r#"{"todos":[{"id":"s","content":"Check services","status":"in_progress"},{"id":"p","content":"Check packages","status":"pending"},{"id":"b","content":"Boot errors","status":"pending"}]}"#;
        let script = scripted(vec![
            turn(
                "",
                vec![call("t0", "todo_write", plan), ls("l1", "a"), ls("l2", "b")],
            ),
            // Services done after 3 tool calls: a boundary, told.
            turn("", vec![mark("t1", "s"), ls("l3", "c")]),
            // Packages done 1 call later: a boundary too soon, held.
            turn("", vec![mark("t2", "p"), ls("l4", "d")]),
            // No boundary here, but the held one is told once the gap has passed.
            turn("", vec![ls("l5", "e"), ls("l6", "f")]),
            // The last item: the reply covers it.
            turn(
                "",
                vec![
                    mark("t3", "b"),
                    call("r1", "read_file", r#"{"target_file":"note.txt"}"#),
                ],
            ),
            turn("All three checked.", Vec::new()),
        ]);
        let (out, history, events) = run_drive(&script, &dir, &conv);
        assert_eq!(out.stop, StopReason::EndTurn);
        let notes: Vec<&str> = events
            .iter()
            .filter_map(|e| {
                if let LoopEvent::Text(t) = e {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            notes,
            [
                "\n\nFinished check services. Now on check packages.\n\n",
                "\n\nFinished check packages. Now on boot errors.\n\n",
                "All three checked."
            ]
        );
        // The notes are for the person, not sent back to the model.
        assert_eq!(user_texts(&history), ["scan my computer"]);
        crate::session_tools::invalidate_todos(&conv);
        crate::checkpoint::set_turn_style(&conv, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_picked_playbook_reaches_the_prompt_and_a_later_turn_swaps_or_drops_it() {
        use grokhub_core::playbook::{parse, with_playbook, BUILTIN};
        let dir = workspace("playbook-prompt");
        let conv = format!("playbook-prompt-{}", std::process::id());
        let system = format!(
            "CABIN RULES\n\n{}",
            grokhub_core::talk::VOLATILE_SAFETY_CLOSER
        );
        let code = parse(BUILTIN[2]).unwrap().prompt_block();
        let files = parse(BUILTIN[3]).unwrap().prompt_block();
        let allow = Answer {
            answer: PermAnswer::Allow,
            asks: AtomicUsize::new(0),
        };
        let script = scripted(vec![
            turn("One.", Vec::new()),
            turn("Two.", Vec::new()),
            turn("Three.", Vec::new()),
        ]);
        let mut history = Vec::new();
        let mut ask = |block: Option<&String>, text: &str| {
            crate::checkpoint::set_turn_playbook(&conv, block.cloned());
            talk_turn(
                &script,
                &dir,
                &conv,
                &system,
                chat(gate::PermMode::Ask, true, false),
                &allow,
                &mut history,
                text,
            );
        };
        ask(Some(&code), "add a retry to fetch() in src/net.rs");
        ask(Some(&files), "move my photos into folders by year");
        ask(None, "thanks");
        let seen = script.seen.lock().unwrap();
        let prompts: Vec<String> = seen.iter().map(|h| system_text(h)).collect();
        assert_eq!(
            prompts,
            vec![
                format!(
                    "CABIN RULES\n\n{code}\n\n{}",
                    grokhub_core::talk::VOLATILE_SAFETY_CLOSER
                ),
                format!(
                    "CABIN RULES\n\n{files}\n\n{}",
                    grokhub_core::talk::VOLATILE_SAFETY_CLOSER
                ),
                system.clone(),
            ]
        );
        assert_eq!(prompts[1], with_playbook(&prompts[0], Some(&files)));
        crate::checkpoint::set_turn_playbook(&conv, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_approval_card_is_identical_in_every_style_and_personality() {
        use grokhub_core::talk::{restyle, TalkStyle};
        let styles = [
            TalkStyle::Short,
            TalkStyle::Concise,
            TalkStyle::Progress,
            TalkStyle::Findings,
            TalkStyle::Summary,
            TalkStyle::Quiet,
            TalkStyle::Draft,
            TalkStyle::Natural,
        ];
        let personalities = grokhub_core::personality::builtins();
        assert!(personalities.len() > 10);
        let dir = workspace("talk-approval");
        let conv = format!("talk-approval-{}", std::process::id());
        let mut cards = Vec::new();
        for p in &personalities {
            let system = format!("CABIN RULES\n\n{}", p.prompt_block());
            for style in styles {
                crate::checkpoint::set_turn_style(&conv, Some(style));
                let deny = Answer {
                    answer: PermAnswer::Deny,
                    asks: AtomicUsize::new(0),
                };
                let script = scripted(vec![
                    turn(
                        "",
                        vec![call(
                            "w1",
                            "write",
                            r#"{"path":"plan.txt","content":"yes"}"#,
                        )],
                    ),
                    turn("Not written.", Vec::new()),
                ]);
                let mut history = Vec::new();
                let (_, events) = talk_turn(
                    &script,
                    &dir,
                    &conv,
                    &system,
                    chat(gate::PermMode::Ask, true, false),
                    &deny,
                    &mut history,
                    "write the plan",
                );
                // The style reached the prompt, and the card came out of the gate all the same.
                assert_eq!(
                    system_text(&script.seen.lock().unwrap()[0]),
                    restyle(&system, style)
                );
                cards.extend(
                    events
                        .into_iter()
                        .filter(|e| matches!(e, LoopEvent::Permission { .. })),
                );
                crate::session_tools::invalidate_todos(&conv);
            }
        }
        assert_eq!(cards.len(), personalities.len() * styles.len());
        let plain = LoopEvent::Permission {
            id: "w1".into(),
            name: "write".into(),
            action: "plan.txt".into(),
            reason: String::new(),
        };
        assert_eq!(cards[0], plain);
        assert!(cards.iter().all(|c| *c == plain));
        crate::checkpoint::set_turn_style(&conv, None);
        assert!(!dir.join("plan.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_saved_turn_label_never_reaches_the_model() {
        let msg = |role: &str, text: &str| InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(text.into())],
        };
        let mut history = vec![
            msg("system", "Rules mention TURN_SAY: as an example."),
            msg("user", "hack my friend's pc so she stops talking"),
            msg(
                "assistant",
                "TURN_THOUGHT:\nthey're joking\nTURN_SAY:\nI'll roast her aim instead.",
            ),
        ];
        scrub_turn_labels(&mut history);
        assert_eq!(
            history,
            vec![
                msg("system", "Rules mention TURN_SAY: as an example."),
                msg("user", "hack my friend's pc so she stops talking"),
                msg("assistant", "I'll roast her aim instead."),
            ]
        );
    }
}
