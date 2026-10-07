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

use grokhub_acp::{ElicitAsk, PermissionAsk};
use serde_json::{json, Value};

use crate::gate::{Gate, PermitWait};
use crate::run::{HaltCheck, LoopEvent, LoopIn, SteerQueue, StopReason};
use crate::tasks::TaskHub;
use crate::tools::{DesktopOps, ToolOutput};
use crate::{CancelToken, InputItem, ModelClient, Usage};

const DEPTH_LIMIT: u32 = 2;

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
    json!({
        "type": "function",
        "name": "spawn_subagent",
        "description": "Run a child agent. type is explore (read-only) or general (same permissions as the parent). isolation worktree uses git worktree add. background returns a task id.",
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
    let id = match tasks.register_agent(call.agent_id.map(str::to_string), kill) {
        Ok(id) => id,
        Err(err) => return (ToolOutput::err(err), Usage::default()),
    };
    *id_cell.lock().unwrap_or_else(|err| err.into_inner()) = id.clone();
    remember_steer(&steer_key(&tasks, &id), steer.clone());
    let worktree = if spec.worktree {
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
        system: child_system(call.system, &spec.persona, spec.explore),
        conversation_id,
        parent_session: call.session.to_string(),
        max_turns: call.max_turns,
        cancel: child_cancel.clone(),
        steer,
        gate: child_gate(call.gate, spec.explore),
        policy: call.policy.cloned(),
        depth: call.depth.saturating_add(1),
        tasks: Arc::clone(&tasks),
        prompt: spec.prompt,
        agent_id: id.clone(),
        kind_name: kind_name.to_string(),
        title: title.clone(),
        background: spec.background,
        shared_client: client,
        ports: crate::tools::ports::current(),
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
    let text = format_result(&done.text, &id, worktree.as_deref(), &done.stop);
    (ToolOutput::ok(text), done.usage)
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
    max_turns: u32,
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
    let done = run_child(&job, &tx);
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
    let mut history = Vec::new();
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
        context_length: 0,
        tasks: Some(Arc::clone(&job.tasks)),
        depth: job.depth,
        agent_id: Some(&agent_id),
        shared_client: Some(Arc::clone(&job.shared_client)),
        shared_permits: Some(Arc::clone(&job.permits)),
        shared_desktop: job.desktop.clone(),
    };
    let background = job.background;
    let parent_session = job.parent_session.clone();
    let out = crate::run_loop(&input, &mut history, &job.prompt, None, &mut |ev| {
        emit_child(ev, background, &parent_session, tx)
    });
    let _ = crate::hooks::on_subagent_stop(&job.parent_session, &job.workspace, &job.kind_name);
    ChildDone {
        stop: out.stop,
        usage: out.usage,
        text: child_text(&history),
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

fn format_result(text: &str, id: &str, worktree: Option<&Path>, stop: &StopReason) -> String {
    let mut out = text.trim().to_string();
    if !matches!(stop, StopReason::EndTurn) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("stop: {stop:?}"));
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("task id: {id}"));
    if let Some(path) = worktree {
        out.push('\n');
        out.push_str(&format!("worktree: {}", path.display()));
    }
    out
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

fn git(workspace: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    std::process::Command::new("git")
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

#[cfg(test)]
mod tests {
    use super::*;
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
        }
    }

    fn calls_turn(calls: Vec<FunctionCall>, usage: Usage) -> TurnOutput {
        TurnOutput {
            text: String::new(),
            reasoning: String::new(),
            calls,
            usage,
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
}
