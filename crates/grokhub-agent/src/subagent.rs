// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Subagent runs. Each child is its own native loop on a worker thread.
//! Explore is read-only. General copies the parent gate and is never looser.
//! Depth 2 means a grandchild cannot spawn. A git worktree is optional and
//! never falls back to the parent tree.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use grokhub_core::wire::{ElicitAsk, PermissionAsk};
use serde_json::{json, Value};

use crate::gate::{Gate, PermitWait};
use crate::run::{HaltCheck, LoopEvent, LoopIn, SteerQueue, StopReason};
use crate::tasks::TaskHub;
use crate::tools::{DesktopOps, ToolOutput};
use crate::{CancelToken, InputItem, ModelClient, Usage};

const DEPTH_LIMIT: u32 = 2;
/// Turns a subagent gets when its parent runs with no cap (`max_turns` 0).
const SUBAGENT_MAX_TURNS: u32 = 50;

pub struct SpawnCall<'a> {
    pub name: &'a str,
    pub arguments: &'a str,
    pub gate: &'a Gate,
    pub session: &'a str,
    pub workspace: &'a Path,
    pub cancel: &'a CancelToken,
    pub halt: &'a dyn HaltCheck,
    pub tasks: Option<Arc<TaskHub>>,
    pub depth: u32,
    pub agent_id: Option<&'a str>,
    pub client: Option<Arc<dyn ModelClient + Send + Sync>>,
    pub permits: Option<Arc<dyn PermitWait + Send + Sync>>,
    pub desktop: Option<Arc<dyn DesktopOps + Send + Sync>>,
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub system: &'a str,
    pub max_turns: u32,
    /// The parent's context window: a child trims, compacts and stops before
    /// the long-context price step the same way.
    pub context_length: u64,
    pub policy: Option<&'a crate::perm::Policy>,
    pub on_event: &'a mut dyn FnMut(LoopEvent),
}

/// Events from background subagents, queued for the cabin's next frame.
/// `session` is always the root native session, so the cabin can hold an event
/// until that thread is open.
pub enum SideEvent {
    Task {
        session: String,
        id: String,
        title: String,
        done: bool,
    },
    Plan {
        session: String,
        text: String,
    },
    Permission(PermissionAsk),
    Elicit(ElicitAsk),
}

impl SideEvent {
    /// Root native session this event belongs to.
    pub fn session(&self) -> &str {
        match self {
            SideEvent::Task { session, .. } | SideEvent::Plan { session, .. } => session,
            SideEvent::Permission(ask) => &ask.session_id,
            SideEvent::Elicit(ask) => &ask.session_id,
        }
    }
}

/// Put events back that the cabin could not show yet (another thread is open).
/// They go to the front, in order, ahead of anything queued since.
pub fn requeue_side_events(events: Vec<SideEvent>) {
    if events.is_empty() {
        return;
    }
    let mut queue = side_queue().lock().unwrap_or_else(|err| err.into_inner());
    for event in events.into_iter().rev() {
        queue.push_front(event);
    }
}

/// `root/s1/s2` -> `root`. Child conversation ids nest under the root session.
fn root_session(session: &str) -> &str {
    session.split('/').next().unwrap_or(session)
}

pub fn drain_side_events() -> Vec<SideEvent> {
    side_queue()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .drain(..)
        .collect()
}

pub fn spawn_schema() -> Value {
    let mut description = String::from(
        "Run a child agent. type is explore (read-only) or general (same permissions as the parent). isolation worktree uses git worktree add. background returns a task id.",
    );
    if crate::route::boundary::enabled(&crate::perm::config_dir()) {
        description.push_str(
            " A general helper's prompt needs a brief, one labeled line each: Goal:, Constraints:, Edge cases:, Done when:, Files:.",
        );
    }
    json!({
        "type": "function",
        "name": "spawn_subagent",
        "description": description,
        "parameters": {
            "type": "object",
            "properties": {
                "prompt": {"type": "string"},
                "description": {"type": "string"},
                "type": {"type": "string"},
                "subagent_type": {"type": "string"},
                "persona": {"type": "string"},
                "background": {"type": "boolean"},
                "run_in_background": {"type": "boolean"},
                "isolation": {"type": "string"}
            },
            "required": ["prompt"],
            "additionalProperties": false
        }
    })
}

pub fn send_schema() -> Value {
    json!({
        "type": "function",
        "name": "send_subagent_message",
        "description": "Steer a running subagent. The message is delivered before its next model turn.",
        "parameters": {
            "type": "object",
            "properties": {
                "subagent_id": {"type": "string"},
                "task_id": {"type": "string"},
                "message": {"type": "string"},
                "text": {"type": "string"}
            },
            "additionalProperties": false
        }
    })
}

pub(crate) fn spawn_is_explore(name: &str, arguments: &str) -> bool {
    if name != "spawn_subagent" {
        return false;
    }
    let Ok(value) = serde_json::from_str::<Value>(arguments) else {
        return false;
    };
    // Worktree isolation writes (git worktree add), so it never takes the read-only shortcut.
    kind_is_explore(&value) && !wants_worktree(&value)
}

fn wants_worktree(value: &Value) -> bool {
    value
        .get("isolation")
        .and_then(|item| item.as_str())
        .is_some_and(|text| text.trim().eq_ignore_ascii_case("worktree"))
}

pub fn try_run(call: SpawnCall<'_>) -> Option<(ToolOutput, Usage)> {
    match call.name {
        "spawn_subagent" => Some(spawn(call)),
        "send_subagent_message" => Some((send(&call), Usage::default())),
        _ => None,
    }
}

struct Spec {
    prompt: String,
    description: String,
    persona: String,
    explore: bool,
    background: bool,
    worktree: bool,
}

fn spawn(call: SpawnCall<'_>) -> (ToolOutput, Usage) {
    if call.depth >= DEPTH_LIMIT {
        return (
            ToolOutput::err("depth limit reached: a subagent of a subagent cannot spawn"),
            Usage::default(),
        );
    }
    let Some(tasks) = call.tasks.clone() else {
        return (
            ToolOutput::err("subagents need a task hub"),
            Usage::default(),
        );
    };
    let Some(client) = call.client.clone() else {
        return (
            ToolOutput::err("subagents need a shared model client"),
            Usage::default(),
        );
    };
    let spec = match parse_spec(call.workspace, call.arguments) {
        Ok(spec) => spec,
        Err(err) => return (ToolOutput::err(err), Usage::default()),
    };
    // Item 34: with boundary routing on, a general helper needs a brief and
    // runs fresh on the cheap lane rather than forking the chat.
    let briefed = !spec.explore
        && !spec.worktree
        && spec.persona.trim().is_empty()
        && crate::route::boundary::enabled(&crate::perm::config_dir());
    if briefed {
        let missing = crate::route::boundary::missing_brief(&spec.prompt);
        if !missing.is_empty() {
            return (
                ToolOutput::err(crate::route::boundary::brief_refusal(&missing)),
                Usage::default(),
            );
        }
    }
    let child_cancel = CancelToken::child_of(call.cancel);
    let steer = SteerQueue::new();
    let wt_cell: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let id_cell = Arc::new(Mutex::new(String::new()));
    let kill_cancel = child_cancel.clone();
    let kill_wt = Arc::clone(&wt_cell);
    let kill_id = Arc::clone(&id_cell);
    let kill_hub = Arc::clone(&tasks);
    let parent_ws = call.workspace.to_path_buf();
    let kill: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        kill_cancel.cancel();
        let id = kill_id
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        if !id.is_empty() {
            kill_hub.kill_owner(&id);
        }
        if let Some(path) = kill_wt
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
        {
            remove_worktree(&parent_ws, &path);
        }
    });
    let id = match tasks.register_agent(call.agent_id.map(str::to_string), &spec.description, kill)
    {
        Ok(id) => id,
        Err(err) => return (ToolOutput::err(err), Usage::default()),
    };
    *id_cell.lock().unwrap_or_else(|err| err.into_inner()) = id.clone();
    remember_steer(&steer_key(&tasks, &id), steer.clone());
    let worktree = if spec.worktree {
        prune_old_worktrees(call.workspace, &crate::perm::config_dir().join("worktrees"));
        match prepare_worktree(call.workspace, call.session, &id) {
            Ok(path) => {
                *wt_cell.lock().unwrap_or_else(|err| err.into_inner()) = Some(path.clone());
                Some(path)
            }
            Err(err) => {
                forget_steer(&steer_key(&tasks, &id));
                let _ = tasks.kill(&id);
                return (ToolOutput::err(err), Usage::default());
            }
        }
    } else {
        None
    };
    let title = title_of(&spec.description, &spec.prompt);
    (call.on_event)(LoopEvent::Task {
        id: id.clone(),
        title: title.clone(),
        done: false,
    });
    let workspace = worktree
        .clone()
        .unwrap_or_else(|| call.workspace.to_path_buf());
    let conversation_id = format!("{}/{}", call.session, id);
    let kind_name = if spec.explore { "explore" } else { "general" };
    // A plain general child of the chat forks it: same system, tools and
    // history, so its first request reuses the chat's cached prefix. An
    // explore child (other tools), a persona or a worktree starts fresh.
    let fork = (call.depth == 0
        && !briefed
        && !spec.explore
        && spec.persona.trim().is_empty()
        && worktree.is_none())
    .then(|| parent_request(call.session))
    .flatten()
    .filter(|req| req.model == call.model);
    let job = ChildJob {
        client: Arc::clone(&client),
        permits: call
            .permits
            .clone()
            .unwrap_or_else(|| Arc::new(crate::gate::ClosedPermits)),
        desktop: call.desktop.clone(),
        workspace,
        model: call.model.to_string(),
        effort: call.effort.map(str::to_string),
        system: if fork.is_some() {
            call.system.to_string()
        } else {
            child_system(call.system, &spec.persona, spec.explore)
        },
        conversation_id,
        parent_session: call.session.to_string(),
        parent_workspace: call.workspace.to_path_buf(),
        trace: crate::harness::provenance::current(),
        max_turns: if call.max_turns == 0 {
            SUBAGENT_MAX_TURNS
        } else {
            call.max_turns
        },
        context_length: call.context_length,
        cancel: child_cancel.clone(),
        steer,
        gate: child_gate(call.gate, spec.explore),
        policy: call.policy.cloned(),
        depth: call.depth.saturating_add(1),
        tasks: Arc::clone(&tasks),
        prompt: if fork.is_some() {
            fork_task(&spec.prompt)
        } else {
            spec.prompt
        },
        agent_id: id.clone(),
        kind_name: kind_name.to_string(),
        title: title.clone(),
        background: spec.background,
        shared_client: client,
        ports: crate::tools::ports::current(),
        origin: crate::harness::current_origin(),
        fork,
        config: crate::perm::pinned_config(),
        class: child_class(crate::route::live::current_class(), briefed),
    };
    if spec.background {
        thread::spawn(move || run_job(job, None));
        return (
            ToolOutput::ok(running_text(&id, worktree.as_deref())),
            Usage::default(),
        );
    }
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || run_job(job, Some(tx)));
    let done = pump(&rx, call.on_event, call.cancel, call.halt, &child_cancel);
    let done = done.unwrap_or_else(|| {
        child_cancel.cancel();
        let _ = tasks.kill(&id);
        ChildDone {
            stop: StopReason::Cancelled,
            usage: Usage::default(),
            text: "subagent ended without a result".into(),
        }
    });
    (call.on_event)(LoopEvent::Task {
        id: id.clone(),
        title,
        done: true,
    });
    // A worktree the helper left clean is removed; one with changes stays for
    // the parent to look at and copy from.
    let kept = worktree.filter(|path| {
        let changed = worktree_changed(path);
        if !changed {
            remove_worktree(call.workspace, path);
        }
        changed
    });
    let text = format_result(&done.text, &id, kept.as_deref(), &done.stop);
    (ToolOutput::ok(text), done.usage)
}

/// The route class a child runs under. A child of a background, scheduled or
/// eval run stays in its parent's class (budget share, pause rules, the 18+
/// guard's background rule); only a chat's briefed helper takes the cheap lane.
fn child_class(parent: &'static str, briefed: bool) -> Option<&'static str> {
    if parent != crate::route::live::DEFAULT_CLASS {
        return Some(parent);
    }
    briefed.then_some(crate::route::boundary::HELPER_CLASS)
}

fn send(call: &SpawnCall<'_>) -> ToolOutput {
    let Some(tasks) = call.tasks.as_ref() else {
        return ToolOutput::err("subagents need a task hub");
    };
    let value = match serde_json::from_str::<Value>(call.arguments) {
        Ok(value) if value.as_object().is_some() => value,
        _ => return ToolOutput::err("send_subagent_message arguments must be a JSON object"),
    };
    let id = text_field(&value, &["subagent_id", "task_id"]);
    if id.is_empty() {
        return ToolOutput::err("subagent_id is required");
    }
    let message = text_field(&value, &["message", "text"]);
    if message.is_empty() {
        return ToolOutput::err("message is required");
    }
    let Some(steer) = lookup_steer(&steer_key(tasks, &id)) else {
        let detail = tasks
            .read_output(&id, 0)
            .err()
            .unwrap_or_else(|| format!("task {id} is not a running subagent"));
        return ToolOutput::err(format!("subagent {id} is not running. {detail}"));
    };
    steer.push(message);
    ToolOutput::ok(format!("sent to {id}"))
}

struct ChildJob {
    client: Arc<dyn ModelClient + Send + Sync>,
    permits: Arc<dyn PermitWait + Send + Sync>,
    desktop: Option<Arc<dyn DesktopOps + Send + Sync>>,
    workspace: PathBuf,
    model: String,
    effort: Option<String>,
    system: String,
    conversation_id: String,
    parent_session: String,
    /// The parent's folder, where a long result is spooled (a worktree can go).
    parent_workspace: PathBuf,
    /// The parent run's origin trace: the child's task text is not yours.
    trace: Option<crate::harness::provenance::Trace>,
    max_turns: u32,
    context_length: u64,
    cancel: CancelToken,
    steer: SteerQueue,
    gate: Gate,
    policy: Option<crate::perm::Policy>,
    depth: u32,
    tasks: Arc<TaskHub>,
    prompt: String,
    agent_id: String,
    kind_name: String,
    title: String,
    background: bool,
    shared_client: Arc<dyn ModelClient + Send + Sync>,
    ports: crate::tools::ports::Ports,
    /// The parent's origin: a child of a scheduled run is still automation.
    origin: crate::harness::Origin,
    /// Set for a fork: the parent's last request, which the child starts from.
    fork: Option<Arc<ParentRequest>>,
    /// The parent thread's pinned config folder: the child reads the same rules and grants.
    config: Option<PathBuf>,
    /// Item 34: the route class a briefed helper runs under.
    class: Option<&'static str>,
}

struct ChildDone {
    stop: StopReason,
    usage: Usage,
    text: String,
}

enum ChildNote {
    Event(LoopEvent),
    Finished(ChildDone),
}

struct HubHalt {
    hub: Arc<TaskHub>,
}

impl HaltCheck for HubHalt {
    fn halted(&self) -> bool {
        self.hub.is_halted()
    }
}

fn run_job(job: ChildJob, tx: Option<Sender<ChildNote>>) {
    let _origin = crate::harness::OriginScope::enter(job.origin);
    let _trace = crate::harness::provenance::enter(job.trace.clone());
    let _config = job.config.clone().map(crate::perm::ConfigGuard::set);
    let mut done = run_child_retrying(&job, &tx);
    done.text = guard_result(
        &job.parent_workspace,
        &job.parent_session,
        &job.agent_id,
        &done.text,
    );
    let cancelled = matches!(done.stop, StopReason::Cancelled | StopReason::Halted);
    if cancelled {
        let _ = job.tasks.append_output(&job.agent_id, &done.text);
        let _ = job.tasks.kill(&job.agent_id);
    } else {
        job.tasks.finish_agent(&job.agent_id, &done.text);
    }
    forget_steer(&steer_key(&job.tasks, &job.agent_id));
    if job.background {
        if !usage_empty(&done.usage) {
            job.tasks.roll_usage(&done.usage);
        }
        push_side(SideEvent::Task {
            session: root_session(&job.parent_session).to_string(),
            id: job.agent_id.clone(),
            title: job.title.clone(),
            done: true,
        });
    }
    if let Some(tx) = tx {
        let _ = tx.send(ChildNote::Finished(done));
    }
}

/// A child that crashes gets one fresh try; a second crash comes back as a
/// failed result the parent can read, not as a silent "no result".
fn run_child_retrying(job: &ChildJob, tx: &Option<Sender<ChildNote>>) -> ChildDone {
    let attempt = || std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_child(job, tx)));
    match attempt() {
        Ok(done) => done,
        Err(_) if job.cancel.is_cancelled() => ChildDone {
            stop: StopReason::Cancelled,
            usage: Usage::default(),
            text: String::new(),
        },
        Err(_) => attempt().unwrap_or_else(|_| ChildDone {
            stop: StopReason::Error(CRASHED_TWICE.into()),
            usage: Usage::default(),
            text: String::new(),
        }),
    }
}

/// What the parent reads when a child crashed on both tries.
const CRASHED_TWICE: &str = "the helper crashed twice and stopped";

fn run_child(job: &ChildJob, tx: &Option<Sender<ChildNote>>) -> ChildDone {
    let _ports = crate::tools::ports::enter(job.ports.clone());
    crate::mcp::alias_elicit(&job.conversation_id, &job.parent_session);
    struct DropAlias(String);
    impl Drop for DropAlias {
        fn drop(&mut self) {
            crate::mcp::unalias_elicit(&self.0);
        }
    }
    let _alias = DropAlias(job.conversation_id.clone());
    crate::hooks::on_subagent_start(&job.parent_session, &job.workspace, &job.kind_name);
    let halt = HubHalt {
        hub: Arc::clone(&job.tasks),
    };
    let agent_id = job.agent_id.clone();
    let _class = job.class.map(crate::route::live::ClassScope::enter);
    let _fork = job
        .fork
        .as_ref()
        .map(|parent| ForkScope::enter(&job.conversation_id, &job.parent_session, parent));
    let mut history = job
        .fork
        .as_ref()
        .map(|parent| parent.input.clone())
        .unwrap_or_default();
    let inherited = history.len();
    let policy = job.policy.clone();
    let desktop = job.desktop.as_ref().map(erase_desktop);
    let input = LoopIn {
        client: job.client.as_ref(),
        workspace: &job.workspace,
        model: &job.model,
        effort: job.effort.as_deref(),
        system: &job.system,
        conversation_id: &job.conversation_id,
        max_turns: job.max_turns,
        usage_base: Usage::default(),
        cancel: &job.cancel,
        steer: &job.steer,
        halt: &halt,
        gate: job.gate,
        desktop,
        permits: job.permits.as_ref(),
        perms: policy.as_ref(),
        context_length: job.context_length,
        tasks: Some(Arc::clone(&job.tasks)),
        depth: job.depth,
        agent_id: Some(&agent_id),
        shared_client: Some(Arc::clone(&job.shared_client)),
        shared_permits: Some(Arc::clone(&job.permits)),
        shared_desktop: job.desktop.clone(),
    };
    let background = job.background;
    let parent_session = job.parent_session.clone();
    let mut out = crate::run_loop(&input, &mut history, &job.prompt, None, &mut |ev| {
        emit_child(ev, background, &parent_session, tx)
    });
    let blocked =
        crate::hooks::on_subagent_stop(&job.parent_session, &job.workspace, &job.kind_name, false);
    // A SubagentStop hook that blocks sends the helper on once with its reason.
    if let Some(reason) = blocked.filter(|_| matches!(out.stop, StopReason::EndTurn)) {
        let again = crate::run_loop(&input, &mut history, &reason, None, &mut |ev| {
            emit_child(ev, background, &parent_session, tx)
        });
        let _ = crate::hooks::on_subagent_stop(
            &job.parent_session,
            &job.workspace,
            &job.kind_name,
            true,
        );
        out.usage.add(&again.usage);
        out.compacted |= again.compacted;
        out.stop = again.stop;
    }
    // A fork's answer is what it added, not the chat it inherited. Once it
    // compacted, the inherited part is a summary and the whole is its own.
    let own = if out.compacted || history.len() < inherited {
        0
    } else {
        inherited
    };
    ChildDone {
        stop: out.stop,
        usage: out.usage,
        text: child_text(&history[own..]),
    }
}

fn emit_child(ev: LoopEvent, background: bool, session: &str, tx: &Option<Sender<ChildNote>>) {
    if !forwardable(&ev) {
        return;
    }
    if background {
        if let Some(side) = side_from(session, ev) {
            push_side(side);
        }
        return;
    }
    if let Some(tx) = tx {
        let _ = tx.send(ChildNote::Event(ev));
    }
}

fn forwardable(ev: &LoopEvent) -> bool {
    matches!(
        ev,
        LoopEvent::Permission { .. }
            | LoopEvent::Elicit(_)
            | LoopEvent::Task { .. }
            | LoopEvent::Plan(_)
    )
}

fn side_from(session: &str, ev: LoopEvent) -> Option<SideEvent> {
    let session = root_session(session);
    Some(match ev {
        LoopEvent::Task { id, title, done } => SideEvent::Task {
            session: session.to_string(),
            id,
            title,
            done,
        },
        LoopEvent::Plan(text) => SideEvent::Plan {
            session: session.to_string(),
            text,
        },
        LoopEvent::Permission {
            id,
            name,
            action,
            reason,
        } => SideEvent::Permission(PermissionAsk {
            rpc_id: serde_json::Value::String(id.clone()),
            session_id: session.to_string(),
            title: name.clone(),
            tool_call_id: id,
            action,
            reason,
            reject_option: Some("denied".into()),
        }),
        LoopEvent::Elicit(view) => SideEvent::Elicit(ElicitAsk {
            rpc_id: serde_json::Value::String(view.id.clone()),
            session_id: session.to_string(),
            tool_call_id: view.id,
            server_name: view.server_name,
            message: view.message,
            mode: view.mode,
            url: view.url,
            elicitation_id: view.elicitation_id,
            field_name: view.field_name,
            field_title: view.field_title,
            secret: view.secret,
            options: view.options,
        }),
        _ => return None,
    })
}

fn pump(
    rx: &Receiver<ChildNote>,
    on_event: &mut dyn FnMut(LoopEvent),
    parent_cancel: &CancelToken,
    halt: &dyn HaltCheck,
    child_cancel: &CancelToken,
) -> Option<ChildDone> {
    loop {
        match rx.recv_timeout(Duration::from_millis(30)) {
            Ok(ChildNote::Event(ev)) => on_event(ev),
            Ok(ChildNote::Finished(done)) => return Some(done),
            Err(RecvTimeoutError::Timeout) => {
                if parent_cancel.is_cancelled() || halt.halted() {
                    child_cancel.cancel();
                }
            }
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
}

fn parse_spec(workspace: &Path, arguments: &str) -> Result<Spec, String> {
    let value = serde_json::from_str::<Value>(arguments)
        .map_err(|err| format!("arguments are not JSON: {err}"))?;
    if value.as_object().is_none() {
        return Err("arguments must be a JSON object".into());
    }
    let prompt = text_field(&value, &["prompt"]);
    if prompt.is_empty() {
        return Err("prompt is required".into());
    }
    let isolation = value
        .get("isolation")
        .and_then(|item| item.as_str())
        .unwrap_or("none")
        .trim()
        .to_ascii_lowercase();
    let worktree = match isolation.as_str() {
        "" | "none" => false,
        "worktree" => true,
        other => return Err(format!("unknown isolation `{other}`")),
    };
    let explore = kind_is_explore(&value);
    if explore && worktree {
        return Err(
            "explore subagents are read-only and do not use worktree isolation; omit isolation or use type general"
                .into(),
        );
    }
    let mut persona = text_field(&value, &["persona"]);
    if let Some(body) = crate::plugins::persona_body(workspace, &persona) {
        persona = body;
    } else if !explore {
        let kind = named_kind(&value);
        if !kind.is_empty() && !kind.eq_ignore_ascii_case("general") {
            if let Some(body) = crate::plugins::persona_body(workspace, &kind) {
                persona = body;
            }
        }
    }
    Ok(Spec {
        prompt,
        description: text_field(&value, &["description"]),
        persona,
        explore,
        background: bool_field(&value, &["background", "run_in_background"]),
        worktree,
    })
}

fn named_kind(value: &Value) -> String {
    ["type", "subagent_type", "agent_type"]
        .iter()
        .find_map(|key| value.get(*key).and_then(|item| item.as_str()))
        .unwrap_or("")
        .trim()
        .to_string()
}

fn kind_is_explore(value: &Value) -> bool {
    let raw = ["type", "subagent_type", "agent_type"]
        .iter()
        .find_map(|key| value.get(*key).and_then(|item| item.as_str()));
    matches!(
        raw.map(str::trim)
            .map(|text| text.to_ascii_lowercase())
            .as_deref(),
        Some("explore")
    )
}

fn text_field(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(|item| item.as_str())
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn bool_field(value: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        value
            .get(*key)
            .and_then(|item| item.as_bool())
            .unwrap_or(false)
    })
}

fn erase_desktop(desktop: &Arc<dyn DesktopOps + Send + Sync>) -> &dyn DesktopOps {
    desktop.as_ref()
}

fn child_gate(parent: &Gate, explore: bool) -> Gate {
    if explore {
        Gate {
            mode: parent.mode,
            readonly_session: true,
            attended: parent.attended,
            desktop: false,
        }
    } else {
        *parent
    }
}

fn child_system(parent: &str, persona: &str, explore: bool) -> String {
    let mut text = String::new();
    // The persona adds to the parent's system text. It never replaces the parent's rules.
    let persona = persona.trim();
    text.push_str(parent);
    if !persona.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(persona);
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if explore {
        text.push_str("You are an explore subagent. You may only use read-only tools.");
    } else {
        text.push_str(
            "You are a general subagent. Follow the same permission rules as the parent.",
        );
    }
    text
}

fn child_text(history: &[InputItem]) -> String {
    let mut last = String::new();
    let mut notes = Vec::new();
    for item in history {
        match item {
            InputItem::Message { role, .. } if role == "assistant" => {
                let text = crate::compact::message_text(item).trim().to_string();
                if !text.is_empty() {
                    last = text;
                }
            }
            InputItem::FunctionCallOutput { output, .. }
                if output.contains("read-only in this phase")
                    || output.contains("Denied by permission policy")
                    || output.contains("depth limit")
                    || output.contains("User rejected")
                    || output.contains("User cancelled")
                    || output.contains("not in plan mode") =>
            {
                notes.push(output.trim().to_string());
            }
            _ => {}
        }
    }
    for note in notes {
        if last.is_empty() {
            last = note;
        } else if !last.contains(note.as_str()) {
            last.push('\n');
            last.push_str(&note);
        }
    }
    if last.chars().count() > 8_000 {
        last = last.chars().take(8_000).collect();
    }
    last
}

fn title_of(description: &str, prompt: &str) -> String {
    let raw = if description.trim().is_empty() {
        prompt
    } else {
        description
    };
    let line = raw.lines().next().unwrap_or("subagent").trim();
    let title: String = line.chars().take(80).collect();
    if title.is_empty() {
        "subagent".into()
    } else {
        title
    }
}

fn running_text(id: &str, worktree: Option<&Path>) -> String {
    let mut text = format!("task id: {id}\nstatus: running");
    if let Some(path) = worktree {
        text.push_str(&format!("\nworktree: {}", path.display()));
    }
    text
}

/// About 3k tokens of a child's result reach the parent; the rest is
/// spooled for a `read_file`.
const RESULT_CHARS: usize = 12_000;

/// A child's result as the parent sees it: capped, with the rest spooled,
/// and fenced, since a child that read a page can pass its words on.
fn guard_result(workspace: &Path, session: &str, id: &str, text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let total = text.chars().count();
    let body = if total > RESULT_CHARS {
        let head: String = text.chars().take(RESULT_CHARS).collect();
        let rest = total - RESULT_CHARS;
        match crate::compact::spool_text(workspace, session, &format!("subagent-{id}"), text) {
            Some(rel) => {
                format!("{head}\n[… {rest} more chars; read_file `{rel}` for the whole result]")
            }
            None => format!("{head}\n[… {rest} more chars clipped]"),
        }
    } else {
        text.to_string()
    };
    crate::tools::fence::fence("subagent", &body)
}

fn format_result(text: &str, id: &str, worktree: Option<&Path>, stop: &StopReason) -> String {
    let mut out = text.trim().to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("status: {}", status_line(stop)));
    out.push('\n');
    out.push_str(&format!("task id: {id}"));
    if let Some(path) = worktree {
        out.push('\n');
        out.push_str(&format!("worktree: {}", path.display()));
    }
    out
}

/// How a child ended, in words the parent can act on.
fn status_line(stop: &StopReason) -> String {
    match stop {
        StopReason::EndTurn => "done".into(),
        StopReason::MaxTurns => {
            "partial (it hit its step limit; this is what it found so far)".into()
        }
        StopReason::Cancelled => "stopped (cancelled before it finished)".into(),
        StopReason::Halted => "stopped (Halt)".into(),
        StopReason::Error(err) => format!("failed ({})", err.trim()),
    }
}

fn usage_empty(usage: &Usage) -> bool {
    usage.input_tokens == 0
        && usage.output_tokens == 0
        && usage.reasoning_tokens == 0
        && usage.cost_in_usd_ticks == 0
}

fn path_part(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if out.is_empty() {
        out.push_str("session");
    }
    out
}

fn prepare_worktree(workspace: &Path, session: &str, id: &str) -> Result<PathBuf, String> {
    let inside = git(workspace, &["rev-parse", "--is-inside-work-tree"])?;
    let stderr = String::from_utf8_lossy(&inside.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&inside.stdout).trim().to_string();
    if !inside.status.success() || stdout != "true" {
        if stderr.is_empty() || stderr.contains("not a git repository") {
            return Err(
                "workspace is not a git repository; worktree isolation was not used".into(),
            );
        }
        return Err(format!("git worktree isolation failed: {stderr}"));
    }
    let dest = crate::perm::config_dir()
        .join("worktrees")
        .join(path_part(session))
        .join(id);
    if dest.exists() {
        return Err(format!("worktree path already exists: {}", dest.display()));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("worktree dir: {err}"))?;
    }
    let added = git(
        workspace,
        &["worktree", "add", "--detach", &dest.display().to_string()],
    )?;
    if !added.status.success() {
        let err = String::from_utf8_lossy(&added.stderr).trim().to_string();
        let _ = std::fs::remove_dir_all(&dest);
        return Err(format!("git worktree add failed: {err}"));
    }
    Ok(dest)
}

fn remove_worktree(workspace: &Path, dest: &Path) {
    let _ = git(
        workspace,
        &["worktree", "remove", "--force", &dest.display().to_string()],
    );
    let _ = std::fs::remove_dir_all(dest);
}

/// True when the helper changed, added or removed a file in its worktree.
/// A worktree git can't read counts as changed, so nothing is lost.
fn worktree_changed(path: &Path) -> bool {
    match git(path, &["status", "--porcelain"]) {
        Ok(out) if out.status.success() => !out.stdout.iter().all(u8::is_ascii_whitespace),
        _ => true,
    }
}

/// Helper worktrees older than this are removed when the next one is made.
const WORKTREE_KEEP: Duration = Duration::from_secs(3 * 24 * 60 * 60);

/// `root/<session>/<id>` folders untouched for [`WORKTREE_KEEP`] are removed,
/// and git forgets them.
fn prune_old_worktrees(workspace: &Path, root: &Path) {
    let Ok(sessions) = std::fs::read_dir(root) else {
        return;
    };
    let mut removed = false;
    for session in sessions.flatten() {
        let Ok(children) = std::fs::read_dir(session.path()) else {
            continue;
        };
        for child in children.flatten() {
            let old = child
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|at| at.elapsed().ok())
                .is_some_and(|age| age > WORKTREE_KEEP);
            if old {
                remove_worktree(workspace, &child.path());
                removed = true;
            }
        }
    }
    if removed {
        let _ = git(workspace, &["worktree", "prune"]);
    }
}

fn git(workspace: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    // Hooks and fsmonitor off: a sandboxed command can plant either in the
    // workspace's repository, and this runs with no sandbox.
    grokhub_core::git_cmd::git()
        .arg("-C")
        .arg(workspace)
        .args(args)
        .output()
        .map_err(|err| format!("git failed to start: {err}"))
}

fn side_queue() -> &'static Mutex<std::collections::VecDeque<SideEvent>> {
    static QUEUE: OnceLock<Mutex<std::collections::VecDeque<SideEvent>>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(std::collections::VecDeque::new()))
}

fn push_side(event: SideEvent) {
    let mut queue = side_queue().lock().unwrap_or_else(|err| err.into_inner());
    if queue.len() >= 256 {
        // Drop an old task or plan row first. A dropped permission or question card
        // would leave a child waiting until cancel or Halt.
        let idx = queue
            .iter()
            .position(|item| matches!(item, SideEvent::Task { .. } | SideEvent::Plan { .. }))
            .unwrap_or(0);
        let _ = queue.remove(idx);
    }
    queue.push_back(event);
}

fn steers() -> &'static Mutex<HashMap<String, SteerQueue>> {
    static MAP: OnceLock<Mutex<HashMap<String, SteerQueue>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Subagent ids (`s1`, `s2`, ...) are only unique inside one session's task hub,
/// so the steer map is keyed by hub as well. One session never steers another's child.
fn steer_key(tasks: &Arc<TaskHub>, id: &str) -> String {
    format!("{:p}/{id}", Arc::as_ptr(tasks))
}

fn remember_steer(id: &str, steer: SteerQueue) {
    steers()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .insert(id.to_string(), steer);
}

fn lookup_steer(id: &str) -> Option<SteerQueue> {
    steers()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(id)
        .cloned()
}

fn forget_steer(id: &str) {
    steers()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .remove(id);
}

/// What a root chat last sent, so a fork can start from exactly that prefix.
struct ParentRequest {
    model: String,
    input: Vec<InputItem>,
    /// The tools array as it went out, so a fork sends the same bytes and its
    /// first request hits the chat's cached prefix past the tool list.
    tools: Vec<Value>,
}

/// Root chats whose last request is kept. Each entry is one chat's context.
const KEPT_REQUESTS: usize = 8;

/// Kept requests by conversation id, newest first.
type ParentRequests = std::collections::VecDeque<(String, Arc<ParentRequest>)>;

fn parent_requests() -> &'static Mutex<ParentRequests> {
    static LIST: OnceLock<Mutex<ParentRequests>> = OnceLock::new();
    LIST.get_or_init(|| Mutex::new(std::collections::VecDeque::new()))
}

/// Keeps a root chat's request input and tools as they went out, newest chat first.
pub fn remember_request(
    conversation_id: &str,
    model: &str,
    input: Vec<InputItem>,
    tools: Vec<Value>,
) {
    let entry = Arc::new(ParentRequest {
        model: model.to_string(),
        input,
        tools,
    });
    let mut list = parent_requests()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    list.retain(|(id, _)| id != conversation_id);
    list.push_front((conversation_id.to_string(), entry));
    list.truncate(KEPT_REQUESTS);
}

fn parent_request(conversation_id: &str) -> Option<Arc<ParentRequest>> {
    parent_requests()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .iter()
        .find(|(id, _)| id == conversation_id)
        .map(|(_, req)| Arc::clone(req))
}

/// Fork conversation ids, the parent chat whose cache key they share, and the
/// parent request they started from.
type Forks = HashMap<String, (String, Arc<ParentRequest>)>;

fn forks() -> &'static Mutex<Forks> {
    static MAP: OnceLock<Mutex<Forks>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The `prompt_cache_key` for a request: a fork uses its parent's, so it
/// lands on the server that already holds the shared prefix.
pub fn cache_key(conversation_id: &str) -> String {
    forks()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(conversation_id)
        .map(|(parent, _)| parent.clone())
        .unwrap_or_else(|| conversation_id.to_string())
}

/// A running fork's tool list: its parent's last one, byte for byte, so the
/// fork's requests share the parent's cached prefix past the tools. The fork
/// runs under the parent's own gate, so the list offers it nothing new.
pub fn fork_tools(conversation_id: &str) -> Option<Vec<Value>> {
    forks()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(conversation_id)
        .map(|(_, parent)| parent.tools.clone())
        .filter(|tools| !tools.is_empty())
}

#[cfg(test)]
fn is_fork(conversation_id: &str) -> bool {
    forks()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .contains_key(conversation_id)
}

/// Registered while a fork runs.
struct ForkScope(String);

impl ForkScope {
    fn enter(child: &str, parent: &str, request: &Arc<ParentRequest>) -> Self {
        forks()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(child.to_string(), (parent.to_string(), Arc::clone(request)));
        Self(child.to_string())
    }
}

impl Drop for ForkScope {
    fn drop(&mut self) {
        forks()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&self.0);
    }
}

/// The user turn a fork gets after the inherited conversation.
fn fork_task(prompt: &str) -> String {
    format!(
        "You are now a fork of this conversation, started for one task. Do only that task, \
         then end with what you found or did: only your final reply goes back.\n\nTask: {}",
        prompt.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_result_is_fenced_and_a_long_one_is_capped_with_the_rest_spooled() {
        let ws = crate::harness::test_dir("child-result");
        assert_eq!(guard_result(&ws, "sess", "s1", "  "), "");
        let short = guard_result(&ws, "sess", "s1", "found it in src/lib.rs");
        assert!(crate::tools::fence::looks_fenced(&short), "{short}");
        assert!(short.contains("found it in src/lib.rs"));

        let long = format!("start {} end-marker", "word ".repeat(5_000));
        let out = guard_result(&ws, "sess", "s2", &long);
        assert!(crate::tools::fence::looks_fenced(&out));
        assert!(out.contains("start"), "the head stays");
        assert!(!out.contains("end-marker"), "the tail is spooled");
        let rel = out
            .split("read_file `")
            .nth(1)
            .and_then(|rest| rest.split('`').next())
            .expect("a spool path");
        assert_eq!(std::fs::read_to_string(ws.join(rel)).unwrap(), long.trim());
        assert!(
            crate::tools::taint::is_tainted(&ws, &ws.join(rel)),
            "reading the spooled result back is fenced too"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }
    use crate::gate::{ClosedPermits, Decision, Gate, PermAnswer, PermMode, PermitWait, Waited};
    use crate::tools::{self, READ_ONLY_PHASE};
    use crate::{ClientError, FunctionCall, InputItem, ResponsesRequest, TurnOutput};
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::time::Instant;

    struct Never;

    impl HaltCheck for Never {
        fn halted(&self) -> bool {
            false
        }
    }

    type StepFn = dyn Fn(&ResponsesRequest, &CancelToken, u32) -> Result<TurnOutput, ClientError>
        + Send
        + Sync;

    struct Fake {
        parent: String,
        counts: Mutex<HashMap<String, u32>>,
        child_tools: Mutex<Vec<Vec<String>>>,
        step: Box<StepFn>,
    }

    impl ModelClient for Fake {
        fn stream(
            &self,
            req: &ResponsesRequest,
            cancel: &CancelToken,
            _sink: &mut dyn FnMut(crate::StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            let n = {
                let mut counts = self.counts.lock().unwrap_or_else(|err| err.into_inner());
                let slot = counts.entry(req.conversation_id.clone()).or_insert(0);
                let n = *slot;
                *slot = slot.saturating_add(1);
                n
            };
            if req.conversation_id != self.parent {
                let names = req
                    .tools
                    .iter()
                    .filter_map(|tool| tool.get("name").and_then(|name| name.as_str()))
                    .map(str::to_string)
                    .collect();
                self.child_tools
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .push(names);
            }
            (self.step)(req, cancel, n)
        }
    }

    struct SeqPerm {
        answers: Mutex<VecDeque<PermAnswer>>,
        asks: AtomicUsize,
    }

    impl PermitWait for SeqPerm {
        fn wait(&self, _call_id: &str, cancel: &CancelToken, halted: &dyn Fn() -> bool) -> Waited {
            self.asks.fetch_add(1, Ordering::SeqCst);
            if cancel.is_cancelled() {
                return Waited::Cancelled;
            }
            if halted() {
                return Waited::Halted;
            }
            let next = self
                .answers
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .pop_front();
            Waited::Answer(next.unwrap_or(PermAnswer::Deny))
        }
    }

    fn unique() -> u64 {
        static N: AtomicU64 = AtomicU64::new(1);
        N.fetch_add(1, Ordering::Relaxed)
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gh-sub-{tag}-{}-{}", std::process::id(), unique()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn session(tag: &str) -> String {
        format!("sub{tag}{}{}", std::process::id(), unique())
    }

    fn call(id: &str, name: &str, args: &str) -> FunctionCall {
        FunctionCall {
            call_id: id.into(),
            name: name.into(),
            arguments: args.into(),
        }
    }

    fn text_turn(text: &str, usage: Usage) -> TurnOutput {
        TurnOutput {
            text: text.into(),
            reasoning: String::new(),
            calls: Vec::new(),
            usage,
            ..Default::default()
        }
    }

    fn calls_turn(calls: Vec<FunctionCall>, usage: Usage) -> TurnOutput {
        TurnOutput {
            text: String::new(),
            reasoning: String::new(),
            calls,
            usage,
            ..Default::default()
        }
    }

    fn spawn_args(prompt: &str, kind: &str, background: bool, isolation: &str) -> String {
        json!({
            "prompt": prompt,
            "type": kind,
            "description": prompt,
            "background": background,
            "isolation": isolation,
        })
        .to_string()
    }

    fn depth_of(parent: &str, conv: &str) -> usize {
        if conv == parent {
            return 0;
        }
        let Some(rest) = conv.strip_prefix(parent) else {
            return 99;
        };
        if !rest.starts_with('/') {
            return 99;
        }
        rest.matches('/').count()
    }

    fn last_output(req: &ResponsesRequest) -> String {
        req.input
            .iter()
            .rev()
            .find_map(|item| match item {
                InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn joined_outputs(items: &[InputItem]) -> String {
        items
            .iter()
            .filter_map(|item| match item {
                InputItem::FunctionCallOutput { output, .. } => Some(output.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn task_line(text: &str, prefix: &str) -> Option<String> {
        text.lines().find_map(|line| {
            line.trim()
                .strip_prefix(prefix)
                .map(str::trim)
                .filter(|rest| !rest.is_empty())
                .map(str::to_string)
        })
    }

    fn chat(mode: PermMode, readonly: bool, attended: bool) -> Gate {
        Gate {
            mode,
            readonly_session: readonly,
            attended,
            desktop: false,
        }
    }

    fn closed() -> Arc<dyn PermitWait + Send + Sync> {
        Arc::new(ClosedPermits)
    }

    fn fake(
        parent: &str,
        step: impl Fn(&ResponsesRequest, &CancelToken, u32) -> Result<TurnOutput, ClientError>
            + Send
            + Sync
            + 'static,
    ) -> Arc<Fake> {
        Arc::new(Fake {
            parent: parent.to_string(),
            counts: Mutex::new(HashMap::new()),
            child_tools: Mutex::new(Vec::new()),
            step: Box::new(step),
        })
    }

    fn run_parent(
        client: &Arc<Fake>,
        dir: &Path,
        session: &str,
        gate: Gate,
        permits: &Arc<dyn PermitWait + Send + Sync>,
        cancel: &CancelToken,
        on_event: &mut dyn FnMut(LoopEvent),
    ) -> (crate::run::LoopOut, Vec<InputItem>) {
        let halt = Never;
        let steer = SteerQueue::new();
        let hub = crate::tasks::hub_for(session);
        hub.reopen();
        let input = LoopIn {
            client: client.as_ref(),
            workspace: dir,
            model: "grok-4.7",
            effort: None,
            system: "parent-system",
            conversation_id: session,
            max_turns: 8,
            usage_base: Usage::default(),
            cancel,
            steer: &steer,
            halt: &halt,
            gate,
            desktop: None,
            permits: permits.as_ref(),
            perms: None,
            context_length: 0,
            tasks: Some(hub),
            depth: 0,
            agent_id: None,
            shared_client: Some(Arc::clone(client) as Arc<dyn ModelClient + Send + Sync>),
            shared_permits: Some(Arc::clone(permits)),
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let out = crate::run_loop(&input, &mut history, "go", None, on_event);
        (out, history)
    }

    fn block_until_cancel(
        cancel: &CancelToken,
        saw: &AtomicBool,
    ) -> Result<TurnOutput, ClientError> {
        let start = Instant::now();
        while !cancel.is_cancelled() {
            if start.elapsed() > Duration::from_secs(5) {
                return Err(ClientError::Protocol(
                    "grandchild did not see cancel".into(),
                ));
            }
            thread::sleep(Duration::from_millis(15));
        }
        saw.store(true, Ordering::SeqCst);
        Err(ClientError::Cancelled)
    }

    #[test]
    fn explore_spawn_runs_in_a_read_only_session() {
        let gate = Gate::phase_readonly();
        let dir = Path::new(".");
        let explore = crate::gate::decide_with(
            &gate,
            "spawn_subagent",
            r#"{"prompt":"look","type":"explore"}"#,
            false,
            None,
            dir,
            None,
        );
        assert!(matches!(explore, Decision::Run));
        let general = crate::gate::decide_with(
            &gate,
            "spawn_subagent",
            r#"{"prompt":"look","subagent_type":"general"}"#,
            false,
            None,
            dir,
            None,
        );
        match general {
            Decision::Refuse(text) => assert!(text.contains(READ_ONLY_PHASE), "{text}"),
            other => panic!("general spawn in a read-only session ran: {other:?}"),
        }
        assert!(spawn_is_explore(
            "spawn_subagent",
            r#"{"prompt":"x","agent_type":"explore"}"#
        ));
        assert!(!spawn_is_explore(
            "spawn_subagent",
            r#"{"prompt":"x","type":"general-purpose"}"#
        ));
    }

    #[test]
    fn depth_limit_blocks_a_grandchild_spawn() {
        let dir = scratch("depth");
        let sid = session("depth");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            let prompt = match depth {
                0 => "CHILD",
                1 => "GRAND",
                _ => "TOO-DEEP",
            };
            if n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args(prompt, "explore", false, "none"),
                    )],
                    Usage::default(),
                ));
            }
            Ok(text_turn("done", Usage::default()))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let text = joined_outputs(&history);
        assert!(
            text.contains("depth limit reached"),
            "grandchild spawn should fail: {text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn explore_child_schema_and_gate_are_read_only() {
        let dir = scratch("explore");
        let sid = session("explore");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("look", "explore", false, "none"),
                    )],
                    Usage::default(),
                ));
            }
            if depth >= 1 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "w",
                        "write",
                        r#"{"path":"secret.txt","content":"no"}"#,
                    )],
                    Usage::default(),
                ));
            }
            Ok(text_turn("done", Usage::default()))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            Gate::phase_readonly(),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let tools = client
            .child_tools
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        assert!(!tools.is_empty(), "explore child made no model call");
        let names = &tools[0];
        for banned in [
            "write",
            "search_replace",
            "run_terminal_command",
            "send_subagent_message",
            "screenshot",
            "kill_command_or_subagent",
        ] {
            assert!(!names.iter().any(|name| name == banned), "{names:?}");
        }
        assert!(names.iter().any(|name| name == "read_file"));
        assert!(names.iter().any(|name| name == "spawn_subagent"));
        assert!(names.iter().any(|name| name == "todo_write"));
        let text = joined_outputs(&history);
        assert!(text.contains(READ_ONLY_PHASE), "{text}");
        assert!(!dir.join("secret.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn general_child_is_never_more_permissive_than_the_parent() {
        let dir = scratch("perm");
        let write_args = r#"{"path":"owned.txt","content":"secret"}"#;

        let sid = session("unattended");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, _cancel, n| {
            if depth_of(&parent_id, &req.conversation_id) == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("write it", "general", false, "none"),
                    )],
                    Usage::default(),
                ));
            }
            Ok(text_turn("done", Usage::default()))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Ask, false, false),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let text = joined_outputs(&history);
        assert!(
            text.contains("Denied by permission policy"),
            "unattended general spawn should deny: {text}"
        );
        assert!(client
            .child_tools
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .is_empty());
        assert!(!dir.join("owned.txt").exists());

        let sid = session("readonly");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, _cancel, n| {
            if depth_of(&parent_id, &req.conversation_id) == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("write it", "general", false, "none"),
                    )],
                    Usage::default(),
                ));
            }
            Ok(text_turn("done", Usage::default()))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, true, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let text = joined_outputs(&history);
        assert!(text.contains(READ_ONLY_PHASE), "{text}");
        assert!(!dir.join("owned.txt").exists());

        let sid = session("ask");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("write it", "general", false, "none"),
                    )],
                    Usage::default(),
                ));
            }
            if depth >= 1 && n == 0 {
                return Ok(calls_turn(
                    vec![call("w", "write", write_args)],
                    Usage::default(),
                ));
            }
            Ok(text_turn("done", Usage::default()))
        });
        let asks = Arc::new(SeqPerm {
            answers: Mutex::new(VecDeque::from([PermAnswer::Allow, PermAnswer::Deny])),
            asks: AtomicUsize::new(0),
        });
        let permits: Arc<dyn PermitWait + Send + Sync> = Arc::clone(&asks) as _;
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Ask, false, true),
            &permits,
            &CancelToken::new(),
            &mut |_| {},
        );
        assert_eq!(
            asks.asks.load(Ordering::SeqCst),
            2,
            "spawn and the child write both ask"
        );
        let text = joined_outputs(&history);
        assert!(
            text.contains("User rejected"),
            "child write should be denied by the person: {text}"
        );
        assert!(!dir.join("owned.txt").exists());
        let tools = client
            .child_tools
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        assert!(
            tools
                .first()
                .is_some_and(|names| names.iter().any(|name| name == "write")),
            "general child should inherit the parent's write tool: {tools:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_cascades_to_child_and_grandchild() {
        let dir = scratch("cancel");
        let sid = session("cancel");
        let parent_id = sid.clone();
        let ready = Arc::new(AtomicBool::new(false));
        let saw = Arc::new(AtomicBool::new(false));
        let ready_step = Arc::clone(&ready);
        let saw_step = Arc::clone(&saw);
        let workspace = dir.clone();
        let client = fake(&sid, move |req, cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth < 2 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("down", "general", false, "none"),
                    )],
                    Usage::default(),
                ));
            }
            if depth >= 2 && n == 0 {
                #[cfg(unix)]
                {
                    return Ok(calls_turn(
                        vec![call(
                            "sh",
                            "run_terminal_command",
                            r#"{"command":"echo $$ > grand.pid; sleep 30","is_background":true}"#,
                        )],
                        Usage::default(),
                    ));
                }
                #[cfg(not(unix))]
                {
                    let _ = &workspace;
                    ready_step.store(true, Ordering::SeqCst);
                    return block_until_cancel(cancel, &saw_step);
                }
            }
            if depth >= 2 {
                #[cfg(unix)]
                {
                    let start = Instant::now();
                    while !workspace.join("grand.pid").exists() {
                        if start.elapsed() > Duration::from_secs(5) {
                            return Err(ClientError::Protocol("pid file missing".into()));
                        }
                        thread::sleep(Duration::from_millis(15));
                    }
                }
                ready_step.store(true, Ordering::SeqCst);
                return block_until_cancel(cancel, &saw_step);
            }
            Ok(text_turn("done", Usage::default()))
        });
        let cancel = CancelToken::new();
        let cancel_bg = cancel.clone();
        let ready_bg = Arc::clone(&ready);
        thread::spawn(move || {
            let start = Instant::now();
            while !ready_bg.load(Ordering::SeqCst) {
                if start.elapsed() > Duration::from_secs(5) {
                    return;
                }
                thread::sleep(Duration::from_millis(15));
            }
            cancel_bg.cancel();
        });
        let (out, _history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &cancel,
            &mut |_| {},
        );
        assert_eq!(out.stop, StopReason::Cancelled);
        assert!(
            saw.load(Ordering::SeqCst),
            "grandchild stream did not observe cancel"
        );
        #[cfg(unix)]
        {
            let pid_text = std::fs::read_to_string(dir.join("grand.pid")).expect("pid file");
            let pid: i32 = pid_text.trim().parse().expect("pid");
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(3) {
                let alive = unsafe { libc::kill(pid, 0) } == 0;
                if !alive {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            let alive = unsafe { libc::kill(pid, 0) } == 0;
            assert!(!alive, "grandchild background process {pid} still alive");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_general_child_forks_the_chat_prefix_and_cache_key() {
        let dir = scratch("fork");
        let sid = session("fork");
        let parent_id = sid.clone();
        // (depth, input, cache key, tools) of every request, in order.
        type Seen = Vec<(usize, Vec<InputItem>, String, Vec<Value>)>;
        let seen: Arc<Mutex<Seen>> = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            log.lock().unwrap().push((
                depth,
                req.input.clone(),
                cache_key(&req.conversation_id),
                req.tools.clone(),
            ));
            match (depth, n) {
                (0, 0) => Ok(calls_turn(
                    vec![
                        call(
                            "g",
                            "spawn_subagent",
                            &spawn_args("find the bug", "general", false, "none"),
                        ),
                        call(
                            "e",
                            "spawn_subagent",
                            &spawn_args("list files", "explore", false, "none"),
                        ),
                    ],
                    Usage::default(),
                )),
                // The fork starts a child of its own first.
                (1, 0)
                    if req.input.iter().any(|item| {
                        crate::compact::message_text(item).contains("find the bug")
                    }) =>
                {
                    Ok(calls_turn(
                        vec![call(
                            "gg",
                            "spawn_subagent",
                            &spawn_args("deeper", "general", false, "none"),
                        )],
                        Usage::default(),
                    ))
                }
                (1, _) => Ok(text_turn("child-done", Usage::default())),
                _ => Ok(text_turn("parent-done", Usage::default())),
            }
        });
        let (out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert!(joined_outputs(&history).contains("child-done"));

        let seen = seen.lock().unwrap();
        let first = seen.iter().find(|(d, _, _, _)| *d == 0).unwrap();
        let (parent, parent_tools) = (&first.1, &first.3);
        let firsts: Vec<_> = seen.iter().filter(|(d, _, _, _)| *d == 1).collect();
        let fork = firsts
            .iter()
            .find(|(_, input, _, _)| input.starts_with(parent))
            .expect("the general child's first request starts with the chat's last one");
        assert!(fork.1.len() > parent.len());
        let task = crate::compact::message_text(fork.1.last().unwrap());
        assert!(
            task.contains("fork") && task.contains("find the bug"),
            "{task}"
        );
        assert_eq!(fork.2, sid, "a fork shares the chat's cache key");
        assert_eq!(
            serde_json::to_string(&fork.3).unwrap(),
            serde_json::to_string(parent_tools).unwrap(),
            "a fork sends the chat's tool list byte for byte"
        );
        let fork_steps: Vec<_> = seen
            .iter()
            .filter(|(d, input, _, _)| *d == 1 && input.starts_with(parent))
            .collect();
        assert!(fork_steps.iter().all(|step| &step.3 == parent_tools));

        let explore = firsts
            .iter()
            .find(|(_, input, _, _)| {
                input
                    .iter()
                    .any(|item| crate::compact::message_text(item).contains("list files"))
            })
            .unwrap();
        assert!(
            !explore.1.starts_with(parent),
            "an explore child starts fresh"
        );
        assert_ne!(explore.2, sid);

        // The fork's own child is a plain subagent: fresh context, own cache key.
        let grand = seen
            .iter()
            .find(|(d, _, _, _)| *d == 2)
            .expect("the fork can start a child");
        assert!(!grand.1.starts_with(parent));
        assert_ne!(grand.2, sid);
        assert!(
            !is_fork(&format!("{sid}/s1")),
            "the fork scope ends with the run"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn child_usage_rolls_into_the_parent() {
        let dir = scratch("usage");
        let sid = session("usage");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("count", "explore", false, "none"),
                    )],
                    Usage {
                        input_tokens: 10,
                        output_tokens: 1,
                        reasoning_tokens: 0,
                        cost_in_usd_ticks: 0,
                        cached_tokens: 0,
                    },
                ));
            }
            if depth >= 1 && n == 0 {
                return Ok(text_turn(
                    "child-done",
                    Usage {
                        input_tokens: 4,
                        output_tokens: 2,
                        reasoning_tokens: 1,
                        cost_in_usd_ticks: 9,
                        cached_tokens: 0,
                    },
                ));
            }
            Ok(text_turn("parent-done", Usage::default()))
        });
        let (out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(out.usage.input_tokens, 14);
        assert_eq!(out.usage.output_tokens, 3);
        assert_eq!(out.usage.reasoning_tokens, 1);
        assert_eq!(out.usage.cost_in_usd_ticks, 9);
        assert!(joined_outputs(&history).contains("child-done"));

        let _ = drain_side_events();
        let sid = session("usagebg");
        let parent_id = sid.clone();
        let release = Arc::new(AtomicBool::new(false));
        let release_step = Arc::clone(&release);
        let client = fake(&sid, move |req, cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("bg", "explore", true, "none"),
                    )],
                    Usage {
                        input_tokens: 2,
                        output_tokens: 0,
                        reasoning_tokens: 0,
                        cost_in_usd_ticks: 0,
                        cached_tokens: 0,
                    },
                ));
            }
            if depth >= 1 {
                let start = Instant::now();
                while !release_step.load(Ordering::SeqCst) {
                    if cancel.is_cancelled() || start.elapsed() > Duration::from_secs(5) {
                        return Err(ClientError::Cancelled);
                    }
                    thread::sleep(Duration::from_millis(15));
                }
                return Ok(text_turn(
                    "bg-used",
                    Usage {
                        input_tokens: 8,
                        output_tokens: 0,
                        reasoning_tokens: 0,
                        cost_in_usd_ticks: 0,
                        cached_tokens: 0,
                    },
                ));
            }
            Ok(text_turn("parent-bg-done", Usage::default()))
        });
        let (out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        assert_eq!(
            out.usage.input_tokens, 2,
            "background usage stays on the hub until the child finishes"
        );
        let id = task_line(&joined_outputs(&history), "task id:").expect("task id");
        release.store(true, Ordering::SeqCst);
        let hub = crate::tasks::hub_for(&sid);
        let start = Instant::now();
        let mut seen = String::new();
        let mut rolled = Usage::default();
        while start.elapsed() < Duration::from_secs(5) {
            if let Ok(text) = hub.read_output(&id, 0) {
                seen.push_str(&text);
            }
            if seen.contains("bg-used") && seen.contains("status: exited") {
                rolled = hub.take_usage();
                if rolled.input_tokens == 8 {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(rolled.input_tokens, 8, "{seen}");
        let _ = drain_side_events();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_child_reads_the_config_folder_its_parent_pinned() {
        let dir = scratch("childcfg");
        let cfg = dir.join("pinned-config");
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        let sid = session("childcfg");
        let parent_id = sid.clone();
        let seen = Arc::new(Mutex::new(Vec::<(usize, PathBuf)>::new()));
        let seen_step = Arc::clone(&seen);
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            seen_step
                .lock()
                .unwrap()
                .push((depth, crate::perm::config_dir()));
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("cfg", "explore", false, "none"),
                    )],
                    Usage::default(),
                ));
            }
            if depth >= 1 {
                return Ok(text_turn("child-done", Usage::default()));
            }
            Ok(text_turn("parent-done", Usage::default()))
        });
        let (out, _) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        let seen = seen.lock().unwrap().clone();
        assert!(seen.iter().any(|(d, _)| *d >= 1), "the child ran: {seen:?}");
        for (depth, dir_seen) in &seen {
            assert_eq!(dir_seen, &cfg, "depth {depth} read another config folder");
        }
        let _ = drain_side_events();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_boundary_routing_a_general_helper_needs_a_brief_and_starts_fresh() {
        let dir = scratch("briefed");
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join(crate::route::boundary::CONFIG), "enabled = true\n").unwrap();
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        let sid = session("briefed");
        let parent_id = sid.clone();
        let brief = "Goal: find the bug\nConstraints: read only\nEdge cases: none\n\
                     Done when: the cause is named\nFiles: src/lib.rs";
        // (depth, input, class) of every request.
        type Seen = Vec<(usize, Vec<InputItem>, &'static str)>;
        let seen: Arc<Mutex<Seen>> = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let brief_args = spawn_args(brief, "general", false, "none");
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            log.lock().unwrap().push((
                depth,
                req.input.clone(),
                crate::route::live::current_class(),
            ));
            match (depth, n) {
                (0, 0) => Ok(calls_turn(
                    vec![call(
                        "bare",
                        "spawn_subagent",
                        &spawn_args("find the bug", "general", false, "none"),
                    )],
                    Usage::default(),
                )),
                (0, 1) => Ok(calls_turn(
                    vec![call("briefed", "spawn_subagent", &brief_args)],
                    Usage::default(),
                )),
                (1, _) => Ok(text_turn("child-done", Usage::default())),
                _ => Ok(text_turn("parent-done", Usage::default())),
            }
        });
        let (out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        let outputs = joined_outputs(&history);
        assert!(outputs.contains("needs a brief"), "{outputs}");
        assert!(outputs.contains("child-done"), "{outputs}");
        let seen = seen.lock().unwrap();
        let parent = &seen.iter().find(|(d, _, _)| *d == 0).unwrap().1;
        let children: Vec<_> = seen.iter().filter(|(d, _, _)| *d == 1).collect();
        assert_eq!(children.len(), 1, "only the briefed helper ran");
        let (_, input, class) = children[0];
        assert!(!input.starts_with(parent), "a briefed helper starts fresh");
        assert_eq!(*class, crate::route::boundary::HELPER_CLASS);
        let _ = drain_side_events();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn background_subagent_output_steer_and_kill() {
        let _ = drain_side_events();
        let dir = scratch("steer");
        let sid = session("steer");
        let parent_id = sid.clone();
        let release = Arc::new(AtomicBool::new(false));
        let release_step = Arc::clone(&release);
        let client = fake(&sid, move |req, cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("wait", "explore", true, "none"),
                    )],
                    Usage::default(),
                ));
            }
            if depth == 0 && n == 1 {
                let id = task_line(&last_output(req), "task id:").unwrap_or_default();
                return Ok(calls_turn(
                    vec![call(
                        "m",
                        "send_subagent_message",
                        &json!({"subagent_id": id, "message": "hello-steer"}).to_string(),
                    )],
                    Usage::default(),
                ));
            }
            if depth == 0 {
                release_step.store(true, Ordering::SeqCst);
                return Ok(text_turn("parent-done", Usage::default()));
            }
            if n == 0 {
                let start = Instant::now();
                while !release_step.load(Ordering::SeqCst) {
                    if cancel.is_cancelled() || start.elapsed() > Duration::from_secs(5) {
                        return Err(ClientError::Cancelled);
                    }
                    thread::sleep(Duration::from_millis(15));
                }
                return Ok(text_turn("", Usage::default()));
            }
            let steered = req
                .input
                .iter()
                .any(|item| crate::compact::message_text(item).contains("hello-steer"));
            assert!(steered, "steer text was not in the child history");
            Ok(text_turn("STEERED", Usage::default()))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let id = task_line(&joined_outputs(&history), "task id:").expect("task id");
        let hub = crate::tasks::hub_for(&sid);
        let start = Instant::now();
        let mut body = String::new();
        while start.elapsed() < Duration::from_secs(5) {
            body.push_str(&hub.read_output(&id, 0).unwrap_or_default());
            if body.contains("STEERED") && body.contains("status: exited") {
                break;
            }
            thread::sleep(Duration::from_millis(15));
        }
        assert!(body.contains("STEERED"), "{body}");
        assert!(body.contains("status: exited"), "{body}");
        let read =
            crate::tools::control::output(Some(&hub), &json!({"task_id": id, "timeout_ms": 0}));
        assert!(!read.failed, "{}", read.text);

        let sid = session("killbg");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("block", "explore", true, "none"),
                    )],
                    Usage::default(),
                ));
            }
            if depth == 0 {
                return Ok(text_turn("parent-done", Usage::default()));
            }
            block_until_cancel(cancel, &AtomicBool::new(false))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let id = task_line(&joined_outputs(&history), "task id:").expect("task id");
        let hub = crate::tasks::hub_for(&sid);
        let killed = crate::tools::control::kill(Some(&hub), &json!({"task_id": id}));
        assert!(!killed.failed, "{}", killed.text);
        let start = Instant::now();
        let mut body = String::new();
        while start.elapsed() < Duration::from_secs(5) {
            body = hub.read_output(&id, 0).unwrap_or_default();
            if body.contains("status: killed") {
                break;
            }
            thread::sleep(Duration::from_millis(15));
        }
        assert!(body.contains("status: killed"), "{body}");
        let _ = drain_side_events();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn worktree_isolation_errors_outside_a_git_repo() {
        let dir = scratch("nowt");
        std::fs::write(dir.join("keep.txt"), "stay\n").unwrap();
        let sid = session("nowt");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, _cancel, n| {
            if depth_of(&parent_id, &req.conversation_id) == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("look", "general", false, "worktree"),
                    )],
                    Usage::default(),
                ));
            }
            Ok(text_turn("done", Usage::default()))
        });
        let guard_dir = scratch("nowtcfg");
        let _guard = crate::perm::ConfigGuard::set(&guard_dir);
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let text = joined_outputs(&history);
        assert!(
            text.contains("workspace is not a git repository; worktree isolation was not used"),
            "{text}"
        );
        assert!(!text.contains("status: running"), "{text}");
        assert_eq!(
            std::fs::read_to_string(dir.join("keep.txt")).unwrap(),
            "stay\n"
        );
        assert!(client
            .child_tools
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&guard_dir);
    }

    #[test]
    fn explore_with_worktree_is_refused_and_skips_the_read_only_shortcut() {
        let args = spawn_args("look", "explore", false, "worktree");
        assert!(!spawn_is_explore("spawn_subagent", &args));
        assert!(spawn_is_explore(
            "spawn_subagent",
            &spawn_args("look", "explore", false, "none")
        ));
        let err = parse_spec(Path::new("."), &args).err().unwrap_or_default();
        assert!(err.contains("do not use worktree isolation"), "{err}");
        // Unattended Ask: the explore+worktree spawn goes through the normal gate and is denied.
        let gate = Gate {
            mode: crate::gate::PermMode::Ask,
            readonly_session: false,
            attended: false,
            desktop: false,
        };
        let decision = crate::gate::decide_with(
            &gate,
            "spawn_subagent",
            &args,
            false,
            None,
            Path::new("."),
            None,
        );
        assert!(
            matches!(decision, crate::gate::Decision::Refuse(_)),
            "{decision:?}"
        );
    }

    #[test]
    fn steer_keys_and_side_events_stay_per_session() {
        let a = crate::tasks::hub_for(&session("steer-a"));
        let b = crate::tasks::hub_for(&session("steer-b"));
        assert_ne!(steer_key(&a, "s1"), steer_key(&b, "s1"));
        assert_eq!(root_session("native-x/s1/s2"), "native-x");
        assert_eq!(root_session("native-x"), "native-x");
        let side = side_from(
            "native-root/s1",
            LoopEvent::Task {
                id: "s2".into(),
                title: "t".into(),
                done: false,
            },
        );
        assert_eq!(side.as_ref().map(SideEvent::session), Some("native-root"));
    }

    #[test]
    fn worktree_isolation_keeps_writes_off_the_parent_tree() {
        let dir = scratch("wt");
        git_repo(&dir);
        let cfg = scratch("wtcfg");
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        let sid = session("wt");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("write", "general", false, "worktree"),
                    )],
                    Usage::default(),
                ));
            }
            if depth >= 1 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "w",
                        "write",
                        r#"{"path":"child.txt","content":"from-child"}"#,
                    )],
                    Usage::default(),
                ));
            }
            Ok(text_turn("done", Usage::default()))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let text = joined_outputs(&history);
        assert!(
            !dir.join("child.txt").exists(),
            "write landed in the parent tree: {text}"
        );
        let path = task_line(&text, "worktree:").expect(&text);
        let wt = PathBuf::from(&path);
        assert_eq!(
            std::fs::read_to_string(wt.join("child.txt")).expect("child file"),
            "from-child"
        );
        assert!(wt.exists(), "a successful subagent keeps its worktree");

        let sid = session("wtcancel");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("block", "general", true, "worktree"),
                    )],
                    Usage::default(),
                ));
            }
            if depth == 0 {
                return Ok(text_turn("done", Usage::default()));
            }
            block_until_cancel(cancel, &AtomicBool::new(false))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let text = joined_outputs(&history);
        let path = task_line(&text, "worktree:").expect(&text);
        let doomed = PathBuf::from(&path);
        assert!(
            doomed.exists(),
            "worktree should exist while the child runs"
        );
        let id = task_line(&text, "task id:").expect(&text);
        let hub = crate::tasks::hub_for(&sid);
        hub.kill(&id).expect("kill");
        let start = Instant::now();
        while doomed.exists() && start.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!doomed.exists(), "cancel removes the worktree");
        assert!(!dir.join("child.txt").exists());
        let _ = std::fs::remove_dir_all(&wt);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    fn git_repo(dir: &Path) {
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-c")
                .arg("safe.directory=*")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "{args:?} {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "test"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.join("README"), "hi\n").unwrap();
        git(&["add", "README"]);
        git(&["commit", "-m", "init"]);
    }

    #[test]
    fn schemas_for_keeps_spawn_and_session_tools_in_plan_mode() {
        // Full tool list: the mode this test checks.
        let _legacy = crate::tools::registry::LegacyPromptGuard::set();
        let plan = tools::schemas_for(&Gate::phase_readonly());
        let names: Vec<&str> = plan
            .iter()
            .filter_map(|tool| tool.get("name").and_then(|name| name.as_str()))
            .collect();
        for kept in [
            "todo_write",
            "ask_user_question",
            "enter_plan_mode",
            "exit_plan_mode",
            "spawn_subagent",
            "read_file",
        ] {
            assert!(names.contains(&kept), "{names:?}");
        }
        assert!(!names.contains(&"send_subagent_message"));
        assert!(!names.contains(&"write"));
        assert!(tools::schemas_for(&chat(PermMode::Always, false, true))
            .iter()
            .any(|tool| tool["name"] == "send_subagent_message"));
    }

    #[test]
    fn plugin_persona_name_expands_only_when_the_bundle_is_trusted() {
        let root = scratch("persona");
        let cfg = root.join("cfg");
        let home = root.join("home");
        let workspace = root.join("ws");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        let _cfg = crate::perm::ConfigGuard::set(&cfg);
        let _home = crate::plugins::HomeGuard::set(&home);
        let src = root.join("src");
        let agents = src.join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::create_dir_all(src.join(".grok-plugin")).unwrap();
        std::fs::write(
            src.join(".grok-plugin").join("plugin.json"),
            r#"{"name":"demo","version":"1.0.0","description":"persona","agents":"agents"}"#,
        )
        .unwrap();
        std::fs::write(
            agents.join("reviewer.md"),
            "---\nname: reviewer\ndescription: Reviews patches\n---\nReview the diff carefully.\n",
        )
        .unwrap();
        crate::plugins::install_path(&src).unwrap();
        let literal = r#"{"prompt":"look","persona":"reviewer"}"#;
        let spec = parse_spec(&workspace, literal).unwrap();
        assert_eq!(spec.persona, "reviewer");
        crate::plugins::trust_plugin(&workspace, "demo").unwrap();
        assert!(crate::plugins::enable_plugin(&workspace, "demo").is_ok());
        let spec = parse_spec(&workspace, literal).unwrap();
        assert!(
            spec.persona.contains("Review the diff carefully."),
            "{}",
            spec.persona
        );
        crate::plugins::disable_plugin("demo").unwrap();
        let spec = parse_spec(&workspace, literal).unwrap();
        assert_eq!(spec.persona, "reviewer");
        crate::plugins::enable_plugin(&workspace, "demo").unwrap();
        let spec = parse_spec(&workspace, r#"{"prompt":"look","type":"reviewer"}"#).unwrap();
        assert!(spec.persona.contains("Review the diff carefully."));
        assert!(!spec.explore);
        let spec = parse_spec(&workspace, r#"{"prompt":"look","persona":"not-a-plugin"}"#).unwrap();
        assert_eq!(spec.persona, "not-a-plugin");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_child_keeps_a_background_parents_route_class() {
        use crate::route::live::DEFAULT_CLASS;
        assert_eq!(
            child_class("background:unattended", false),
            Some("background:unattended")
        );
        assert_eq!(child_class("eval:item", true), Some("eval:item"));
        assert_eq!(child_class(DEFAULT_CLASS, true), Some("chat:quick"));
        assert_eq!(child_class(DEFAULT_CLASS, false), None);

        let dir = scratch("class");
        let cfg = scratch("classcfg");
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        let sid = session("class");
        let parent_id = sid.clone();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let saw = Arc::clone(&seen);
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("look", "explore", false, "none"),
                    )],
                    Usage::default(),
                ));
            }
            if depth >= 1 {
                saw.lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .push(crate::route::live::current_class().to_string());
            }
            Ok(text_turn("done", Usage::default()))
        });
        let _class = crate::route::live::ClassScope::enter(crate::unattended::UNATTENDED_CLASS);
        let _ = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, false),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        assert_eq!(
            *seen.lock().unwrap_or_else(|err| err.into_inner()),
            vec!["background:unattended".to_string()]
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_child_that_crashes_gets_one_fresh_try() {
        let dir = scratch("crash");
        let cfg = scratch("crashcfg");
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        let sid = session("crash");
        let parent_id = sid.clone();
        let crashed = Arc::new(AtomicBool::new(false));
        let once = Arc::clone(&crashed);
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("look", "explore", false, "none"),
                    )],
                    Usage::default(),
                ));
            }
            if depth >= 1 && !once.swap(true, Ordering::SeqCst) {
                panic!("helper blew up");
            }
            if depth >= 1 {
                return Ok(text_turn("found it on the second try", Usage::default()));
            }
            Ok(text_turn("done", Usage::default()))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let text = joined_outputs(&history);
        assert!(crashed.load(Ordering::SeqCst));
        assert!(text.contains("found it on the second try"), "{text}");
        assert_eq!(
            task_line(&text, "status:").as_deref(),
            Some("done"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn stop_reasons_read_as_plain_words() {
        assert_eq!(status_line(&StopReason::EndTurn), "done");
        assert_eq!(
            status_line(&StopReason::MaxTurns),
            "partial (it hit its step limit; this is what it found so far)"
        );
        assert_eq!(
            status_line(&StopReason::Cancelled),
            "stopped (cancelled before it finished)"
        );
        assert_eq!(status_line(&StopReason::Halted), "stopped (Halt)");
        assert_eq!(
            status_line(&StopReason::Error(" provider timed out ".into())),
            "failed (provider timed out)"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_blocking_subagent_stop_hook_sends_the_helper_on_once() {
        let dir = scratch("stophook");
        let cfg = scratch("stophookcfg");
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        let sid = session("stophook");
        let _hooks = crate::hooks::install_test_hooks(
            &sid,
            vec![crate::hooks::TestHook {
                event: "SubagentStop".into(),
                matcher: String::new(),
                command: "echo 'run the tests too' >&2; exit 2".into(),
                timeout: Duration::from_secs(5),
                source_dir: dir.clone(),
            }],
        );
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("look", "explore", false, "none"),
                    )],
                    Usage::default(),
                ));
            }
            if depth >= 1 && n == 0 {
                return Ok(text_turn("first pass", Usage::default()));
            }
            if depth >= 1 {
                let asked = req.input.iter().rev().find_map(|item| match item {
                    InputItem::Message { role, .. } if role == "user" => {
                        Some(crate::compact::message_text(item))
                    }
                    _ => None,
                });
                return Ok(text_turn(
                    &format!("second pass after: {}", asked.unwrap_or_default().trim()),
                    Usage::default(),
                ));
            }
            Ok(text_turn("done", Usage::default()))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let text = joined_outputs(&history);
        assert!(
            text.contains("second pass after: run the tests too"),
            "{text}"
        );
        let child_calls: u32 = client
            .counts
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .filter(|(conv, _)| conv.as_str() != sid)
            .map(|(_, n)| *n)
            .sum();
        assert_eq!(child_calls, 2, "the hook blocks once, not forever");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_clean_worktree_is_removed_and_old_ones_are_pruned() {
        let dir = scratch("wtclean");
        git_repo(&dir);
        let cfg = scratch("wtcleancfg");
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        let stale = cfg.join("worktrees").join("oldsession").join("a9");
        std::fs::create_dir_all(&stale).unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(4 * 24 * 60 * 60);
        std::fs::File::open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let sid = session("wtclean");
        let parent_id = sid.clone();
        let client = fake(&sid, move |req, _cancel, n| {
            let depth = depth_of(&parent_id, &req.conversation_id);
            if depth == 0 && n == 0 {
                return Ok(calls_turn(
                    vec![call(
                        "s",
                        "spawn_subagent",
                        &spawn_args("read", "general", false, "worktree"),
                    )],
                    Usage::default(),
                ));
            }
            Ok(text_turn("nothing to change", Usage::default()))
        });
        let (_out, history) = run_parent(
            &client,
            &dir,
            &sid,
            chat(PermMode::Always, false, true),
            &closed(),
            &CancelToken::new(),
            &mut |_| {},
        );
        let text = joined_outputs(&history);
        assert_eq!(task_line(&text, "worktree:"), None, "{text}");
        let id = task_line(&text, "task id:").expect(&text);
        let made = cfg.join("worktrees").join(path_part(&sid)).join(&id);
        assert!(
            !made.exists(),
            "a clean worktree is removed: {}",
            made.display()
        );
        assert!(
            !stale.exists(),
            "a worktree older than three days is pruned"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cfg);
    }
}
