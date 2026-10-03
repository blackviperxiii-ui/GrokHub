// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Background commands and monitors for one native session.
//! Output is a 1 MiB tail. Completion and monitor lines are queued once
//! and injected at the start of the next model turn.

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::tools::shell::{spawn_background, WaitPoll};
use crate::CancelToken;

const RING_CAP: usize = 1024 * 1024;
const NOTICE_TAIL: usize = 2_000;
const POLL: Duration = Duration::from_millis(20);
pub const MONITOR_CAP_MS: u64 = 10 * 60 * 60 * 1000;
const WAIT_CAP_MS: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Running,
    Exited(i32),
    Killed,
}

struct Ring {
    buf: VecDeque<u8>,
    start: u64,
}

impl Ring {
    fn new() -> Self {
        Self {
            buf: VecDeque::new(),
            start: 0,
        }
    }

    fn push(&mut self, data: &[u8]) {
        for &byte in data {
            if self.buf.len() == RING_CAP {
                self.buf.pop_front();
                self.start = self.start.saturating_add(1);
            }
            self.buf.push_back(byte);
        }
    }

    fn end(&self) -> u64 {
        self.start.saturating_add(self.buf.len() as u64)
    }

    /// Bytes from `abs` to the end. `true` when the ring already dropped the start.
    fn slice_from(&self, abs: u64) -> (bool, Vec<u8>) {
        let from = abs.max(self.start);
        let truncated = abs < self.start;
        let idx = usize::try_from(from.saturating_sub(self.start)).unwrap_or(usize::MAX);
        let bytes = self.buf.iter().skip(idx).copied().collect();
        (truncated, bytes)
    }
}

struct TaskSlot {
    ring: Arc<Mutex<Ring>>,
    read_at: AtomicU64,
    phase: Mutex<Phase>,
    kill: Arc<dyn Fn() + Send + Sync>,
    /// Set when this task was started by a monitor. Halt stops it with the rest.
    owned_by_monitor: bool,
}

struct MonitorSlot {
    stop: Arc<AtomicBool>,
    task_id: Option<String>,
}

struct HubInner {
    session_id: String,
    seq: u64,
    tasks: HashMap<String, Arc<TaskSlot>>,
    monitors: HashMap<String, MonitorSlot>,
    notices: VecDeque<String>,
    halted: bool,
}

/// One session's background commands, monitors, and pending notices.
pub struct TaskHub {
    inner: Mutex<HubInner>,
}

impl TaskHub {
    fn new(session_id: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(HubInner {
                session_id: session_id.into(),
                seq: 0,
                tasks: HashMap::new(),
                monitors: HashMap::new(),
                notices: VecDeque::new(),
                halted: false,
            }),
        })
    }

    pub fn session_id(&self) -> String {
        self.lock().session_id.clone()
    }

    pub fn is_halted(&self) -> bool {
        self.lock().halted
    }

    /// A new user turn may start tasks again. Processes killed by Halt stay dead.
    pub fn reopen(&self) {
        self.lock().halted = false;
    }

    pub fn spawn(self: &Arc<Self>, cwd: &Path, command: &str) -> Result<String, String> {
        self.spawn_owned(cwd, command, false)
    }

    fn spawn_owned(
        self: &Arc<Self>,
        cwd: &Path,
        command: &str,
        owned_by_monitor: bool,
    ) -> Result<String, String> {
        let command = command.trim().to_string();
        if command.is_empty() {
            return Err("command is required".into());
        }
        if self.is_halted() {
            return Err("halted".into());
        }
        let cwd = cwd
            .canonicalize()
            .map_err(|err| format!("workspace is not available: {err}"))?;
        let id = {
            let mut hub = self.lock();
            if hub.halted {
                return Err("halted".into());
            }
            hub.seq = hub.seq.saturating_add(1);
            format!("t{}", hub.seq)
        };
        let proc = spawn_background(&cwd, &command)?;
        if self.is_halted() {
            proc.kill_tree();
            return Err("halted".into());
        }
        let slot = Arc::new(TaskSlot {
            ring: Arc::new(Mutex::new(Ring::new())),
            read_at: AtomicU64::new(0),
            phase: Mutex::new(Phase::Running),
            kill: proc.killer(),
            owned_by_monitor,
        });
        self.lock().tasks.insert(id.clone(), Arc::clone(&slot));
        let notice_hub = Arc::clone(self);
        let task_id = id.clone();
        thread::spawn(move || supervise(proc, slot, notice_hub, task_id));
        Ok(id)
    }

    pub fn read_output(&self, id: &str, timeout_ms: u64) -> Result<String, String> {
        let timeout_ms = timeout_ms.min(WAIT_CAP_MS);
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let slot = self.task(id)?;
            let phase = *slot.phase.lock().unwrap_or_else(|err| err.into_inner());
            if !matches!(phase, Phase::Running) || timeout_ms == 0 || Instant::now() >= deadline {
                return Ok(format_output(&slot, phase));
            }
            thread::sleep(POLL);
        }
    }

    /// Kill a background command or stop a monitor. Unknown ids error.
    pub fn kill(&self, id: &str) -> Result<String, String> {
        if let Some(slot) = self.lock().tasks.get(id).cloned() {
            kill_slot(&slot);
            return Ok(format!("killed {id}"));
        }
        if self.stop_monitor(id) {
            return Ok(format!("stopped monitor {id}"));
        }
        let known = self.known_ids();
        if known.is_empty() {
            Err(format!("task {id} not found"))
        } else {
            Err(format!(
                "task {id} not found. Known ids: {}",
                known.join(", ")
            ))
        }
    }

    pub fn start_monitor(
        self: &Arc<Self>,
        cwd: &Path,
        command: Option<String>,
        task_id: Option<String>,
        pattern: Option<String>,
        timeout_ms: u64,
        description: String,
    ) -> Result<String, String> {
        if self.is_halted() {
            return Err("halted".into());
        }
        let pattern = match pattern.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(text) => Some(regex::Regex::new(text).map_err(|err| format!("pattern: {err}"))?),
        };
        let launched = if let Some(command) = command.filter(|c| !c.trim().is_empty()) {
            Some(self.spawn_owned(cwd, &command, true)?)
        } else {
            None
        };
        let watched = launched
            .clone()
            .or(task_id.filter(|id| !id.trim().is_empty()));
        let Some(watched) = watched else {
            return Err("monitor needs a command or a task_id".into());
        };
        if launched.is_none() && self.task(&watched).is_err() {
            return Err(format!("task {watched} not found"));
        }
        let id = {
            let mut hub = self.lock();
            hub.seq = hub.seq.saturating_add(1);
            format!("m{}", hub.seq)
        };
        let stop = Arc::new(AtomicBool::new(false));
        self.lock().monitors.insert(
            id.clone(),
            MonitorSlot {
                stop: Arc::clone(&stop),
                task_id: Some(watched.clone()),
            },
        );
        let timeout_ms = if timeout_ms == 0 {
            MONITOR_CAP_MS
        } else {
            timeout_ms.min(MONITOR_CAP_MS)
        };
        let hub = Arc::clone(self);
        let monitor_id = id.clone();
        let label = if description.trim().is_empty() {
            monitor_id.clone()
        } else {
            format!("{monitor_id} ({})", description.trim())
        };
        let from_start = launched.is_some();
        thread::spawn(move || {
            watch_lines(
                hub, monitor_id, label, watched, pattern, stop, timeout_ms, from_start,
            )
        });
        Ok(id)
    }

    /// Kill every command and monitor. Later spawns fail until [`reopen`].
    pub fn halt_all(&self) {
        let (tasks, monitors) = {
            let mut hub = self.lock();
            hub.halted = true;
            let tasks: Vec<_> = hub.tasks.values().cloned().collect();
            let monitors: Vec<_> = hub.monitors.values().map(|m| Arc::clone(&m.stop)).collect();
            (tasks, monitors)
        };
        for stop in monitors {
            stop.store(true, Ordering::SeqCst);
        }
        for slot in tasks {
            kill_slot(&slot);
        }
    }

    /// Notices queued since the last drain. Each one is returned once.
    pub fn drain_notices(&self) -> Vec<String> {
        self.lock().notices.drain(..).collect()
    }

    fn push_notice(&self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.lock().notices.push_back(text);
    }

    fn task(&self, id: &str) -> Result<Arc<TaskSlot>, String> {
        self.lock()
            .tasks
            .get(id)
            .cloned()
            .ok_or_else(|| format!("task {id} not found"))
    }

    fn stop_monitor(&self, id: &str) -> bool {
        let (stop, task_id) = {
            let hub = self.lock();
            let Some(slot) = hub.monitors.get(id) else {
                return false;
            };
            (Arc::clone(&slot.stop), slot.task_id.clone())
        };
        stop.store(true, Ordering::SeqCst);
        if let Some(task_id) = task_id {
            if let Some(slot) = self.lock().tasks.get(&task_id).cloned() {
                if slot.owned_by_monitor {
                    kill_slot(&slot);
                }
            }
        }
        true
    }

    fn known_ids(&self) -> Vec<String> {
        let hub = self.lock();
        let mut ids: Vec<_> = hub
            .tasks
            .keys()
            .cloned()
            .chain(hub.monitors.keys().cloned())
            .collect();
        ids.sort();
        ids
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubInner> {
        self.inner.lock().unwrap_or_else(|err| err.into_inner())
    }
}

fn kill_slot(slot: &TaskSlot) {
    let mut phase = slot.phase.lock().unwrap_or_else(|err| err.into_inner());
    if matches!(*phase, Phase::Running) {
        (slot.kill)();
        *phase = Phase::Killed;
    }
}

fn format_output(slot: &TaskSlot, phase: Phase) -> String {
    let ring = slot.ring.lock().unwrap_or_else(|err| err.into_inner());
    let cursor = slot.read_at.load(Ordering::SeqCst);
    let end = ring.end();
    let (truncated, bytes) = ring.slice_from(cursor);
    slot.read_at.store(end, Ordering::SeqCst);
    drop(ring);
    let mut body = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        body = format!("(earlier output dropped)\n{body}");
    }
    let head = match phase {
        Phase::Running => "status: running".to_string(),
        Phase::Exited(code) => format!("status: exited\nexit_code: {code}"),
        Phase::Killed => "status: killed".to_string(),
    };
    if body.is_empty() {
        head
    } else {
        format!("{head}\noutput:\n{body}")
    }
}

fn supervise(
    mut proc: crate::tools::shell::BgProc,
    slot: Arc<TaskSlot>,
    hub: Arc<TaskHub>,
    id: String,
) {
    let stdout = proc.stdout.take();
    let stderr = proc.stderr.take();
    let out_ring = Arc::clone(&slot.ring);
    let err_ring = Arc::clone(&slot.ring);
    let out_thread = thread::spawn(move || read_pipe(stdout, out_ring));
    let err_thread = thread::spawn(move || read_pipe(stderr, err_ring));
    let mut failed = None;
    let phase = loop {
        if hub.is_halted()
            || matches!(
                *slot.phase.lock().unwrap_or_else(|e| e.into_inner()),
                Phase::Killed
            )
        {
            proc.kill_tree();
            let _ = reap(&mut proc, Duration::from_secs(2));
            break Phase::Killed;
        }
        match proc.poll() {
            WaitPoll::Running => thread::sleep(POLL),
            WaitPoll::Exited(code) => {
                proc.disarm();
                break Phase::Exited(code);
            }
            WaitPoll::Failed(err) => {
                proc.kill_tree();
                failed = Some(err);
                break Phase::Killed;
            }
        }
    };
    let _ = out_thread.join();
    let _ = err_thread.join();
    let tail = notice_tail(&slot);
    let text = match phase {
        Phase::Exited(code) => {
            if tail.is_empty() {
                format!("Background task {id} finished with exit {code}.")
            } else {
                format!("Background task {id} finished with exit {code}.\n{tail}")
            }
        }
        Phase::Killed => match failed {
            Some(err) => format!("Background task {id} was killed.\n{err}"),
            None => format!("Background task {id} was killed."),
        },
        Phase::Running => format!("Background task {id} finished."),
    };
    // The notice is queued before the phase flips, so a waiter that sees
    // a finished status can drain this notice in the same turn.
    hub.push_notice(text);
    let mut current = slot.phase.lock().unwrap_or_else(|err| err.into_inner());
    if matches!(*current, Phase::Running) {
        *current = phase;
    }
}

fn reap(proc: &mut crate::tools::shell::BgProc, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        match proc.poll() {
            WaitPoll::Running if Instant::now() < deadline => thread::sleep(POLL),
            WaitPoll::Running => return false,
            _ => return true,
        }
    }
}

fn read_pipe(pipe: Option<Box<dyn Read + Send>>, ring: Arc<Mutex<Ring>>) {
    let Some(mut pipe) = pipe else {
        return;
    };
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => ring
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(&buf[..n]),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn notice_tail(slot: &TaskSlot) -> String {
    let ring = slot.ring.lock().unwrap_or_else(|err| err.into_inner());
    let bytes: Vec<u8> = ring.buf.iter().copied().collect();
    let raw = String::from_utf8_lossy(&bytes);
    let text = raw.trim();
    if text.chars().count() <= NOTICE_TAIL {
        return text.to_string();
    }
    let skip = text.chars().count() - NOTICE_TAIL;
    text.chars().skip(skip).collect()
}

fn watch_lines(
    hub: Arc<TaskHub>,
    id: String,
    label: String,
    task_id: String,
    pattern: Option<regex::Regex>,
    stop: Arc<AtomicBool>,
    timeout_ms: u64,
    from_start: bool,
) {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut cursor = if from_start {
        0
    } else {
        hub.task(&task_id)
            .map(|slot| {
                slot.ring
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .end()
            })
            .unwrap_or(0)
    };
    let mut pending = String::new();
    loop {
        if stop.load(Ordering::SeqCst) || hub.is_halted() {
            hub.push_notice(format!("Monitor {label} cancelled."));
            break;
        }
        if Instant::now() >= deadline {
            hub.push_notice(format!("Monitor {label} ended after the time limit."));
            break;
        }
        let Ok(slot) = hub.task(&task_id) else {
            hub.push_notice(format!(
                "Monitor {label} ended because task {task_id} is gone."
            ));
            break;
        };
        let phase = *slot.phase.lock().unwrap_or_else(|err| err.into_inner());
        let chunk = {
            let ring = slot.ring.lock().unwrap_or_else(|err| err.into_inner());
            let end = ring.end();
            let (_, bytes) = ring.slice_from(cursor);
            cursor = end;
            String::from_utf8_lossy(&bytes).into_owned()
        };
        if !chunk.is_empty() {
            pending.push_str(&chunk);
            while let Some(pos) = pending.find('\n') {
                let line: String = pending.drain(..=pos).collect();
                emit_line(&hub, &label, &pattern, line.trim_end_matches(['\n', '\r']));
            }
        }
        if !matches!(phase, Phase::Running) {
            if !pending.is_empty() {
                emit_line(
                    &hub,
                    &label,
                    &pattern,
                    pending.trim_end_matches(['\n', '\r']),
                );
                pending.clear();
            }
            let why = match phase {
                Phase::Exited(code) => format!("exit {code}"),
                Phase::Killed => "killed".into(),
                Phase::Running => "done".into(),
            };
            hub.push_notice(format!("Monitor {label} ended ({why})."));
            break;
        }
        thread::sleep(POLL);
    }
    hub.lock().monitors.remove(&id);
    let _ = task_id;
}

fn emit_line(hub: &TaskHub, label: &str, pattern: &Option<regex::Regex>, line: &str) {
    if line.is_empty() {
        return;
    }
    let hit = match pattern {
        None => true,
        Some(re) => re.is_match(line),
    };
    if hit {
        hub.push_notice(format!("Monitor {label}: {line}"));
    }
}

struct World {
    hubs: HashMap<String, Arc<TaskHub>>,
    children: HashMap<String, Vec<String>>,
    cancels: HashMap<String, CancelToken>,
}

fn world() -> &'static Mutex<World> {
    static WORLD: std::sync::OnceLock<Mutex<World>> = std::sync::OnceLock::new();
    WORLD.get_or_init(|| {
        Mutex::new(World {
            hubs: HashMap::new(),
            children: HashMap::new(),
            cancels: HashMap::new(),
        })
    })
}

fn lock_world() -> std::sync::MutexGuard<'static, World> {
    world().lock().unwrap_or_else(|err| err.into_inner())
}

pub fn hub_for(session_id: &str) -> Arc<TaskHub> {
    let mut world = lock_world();
    if let Some(hub) = world.hubs.get(session_id) {
        return Arc::clone(hub);
    }
    let hub = TaskHub::new(session_id);
    world.hubs.insert(session_id.to_string(), Arc::clone(&hub));
    hub
}

/// Remember a `/bg` engine so Halt and session delete stop it with the parent.
pub fn link_child(parent: &str, child: &str) {
    if parent.is_empty() || child.is_empty() || parent == child {
        return;
    }
    let mut world = lock_world();
    let kids = world.children.entry(parent.to_string()).or_default();
    if !kids.iter().any(|id| id == child) {
        kids.push(child.to_string());
    }
}

pub fn watch_cancel(session_id: &str, cancel: CancelToken) {
    lock_world().cancels.insert(session_id.to_string(), cancel);
}

pub fn halt_session(session_id: &str) {
    let (hub, cancel) = {
        let world = lock_world();
        (
            world.hubs.get(session_id).cloned(),
            world.cancels.get(session_id).cloned(),
        )
    };
    if let Some(cancel) = cancel {
        cancel.cancel();
    }
    if let Some(hub) = hub {
        hub.halt_all();
    }
}

/// Halt: cancel every native run and stop every background task and monitor,
/// foreground and `/bg` alike. A later user turn reopens its own hub.
pub fn halt_all_sessions() {
    for id in live_session_ids() {
        halt_session(&id);
    }
}

/// Every session with a task hub or a registered cancel token.
fn live_session_ids() -> Vec<String> {
    let world = lock_world();
    let mut ids: Vec<String> = world.hubs.keys().cloned().collect();
    for id in world.cancels.keys() {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

/// Stop this session's tasks and every `/bg` engine linked to it.
pub fn halt_tree(session_id: &str) {
    let kids = lock_world().children.remove(session_id).unwrap_or_default();
    halt_session(session_id);
    for kid in kids {
        halt_session(&kid);
        crate::session::cancel_token(&kid);
    }
}

pub fn forget_session(session_id: &str) {
    halt_tree(session_id);
    let mut world = lock_world();
    world.hubs.remove(session_id);
    world.cancels.remove(session_id);
}

/// `true` when a monitor call does not start a command.
pub fn monitor_watch_only(arguments: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return true;
    };
    value
        .get("command")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-task-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ring_keeps_one_mib_tail() {
        let mut ring = Ring::new();
        let chunk = vec![b'a'; 64 * 1024];
        for _ in 0..(RING_CAP / chunk.len()) {
            ring.push(&chunk);
        }
        assert_eq!(ring.buf.len(), RING_CAP);
        ring.push(b"XYZ");
        assert_eq!(ring.buf.len(), RING_CAP);
        let tail: Vec<u8> = ring.buf.iter().rev().take(3).copied().collect();
        assert_eq!(tail, vec![b'Z', b'Y', b'X']);
        let (truncated, bytes) = ring.slice_from(0);
        assert!(truncated);
        assert_eq!(bytes.len(), RING_CAP);
        assert_eq!(&bytes[bytes.len() - 3..], b"XYZ");
    }

    #[cfg(unix)]
    #[test]
    fn background_lifecycle_output_exit_and_notice_once() {
        let dir = scratch("life");
        let hub = TaskHub::new("life");
        remember(&hub);
        let id = hub.spawn(&dir, "printf 'hello-bg\\n'").expect("spawn");
        let text = hub.read_output(&id, 5_000).expect("output");
        assert!(text.contains("status: exited"), "{text}");
        assert!(text.contains("exit_code: 0"), "{text}");
        assert!(text.contains("hello-bg"), "{text}");
        let again = hub.read_output(&id, 0).expect("second");
        assert!(
            !again.contains("hello-bg"),
            "second read must be new output only: {again}"
        );
        let notes = wait_notice(&hub);
        assert!(
            notes
                .iter()
                .any(|n| n.contains(&id) && n.contains("exit 0")),
            "{notes:?}"
        );
        assert!(hub.drain_notices().is_empty(), "a notice is injected once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn kill_stops_a_grandchild() {
        let dir = scratch("tree");
        let hub = TaskHub::new("tree");
        remember(&hub);
        let id = hub
            .spawn(
                &dir,
                "bash -c 'sleep 60 & echo $! > grand.pid; wait' & echo $! > child.pid; wait",
            )
            .expect("spawn");
        let grand = wait_pid(&dir.join("grand.pid"));
        let child = wait_pid(&dir.join("child.pid"));
        assert!(grand > 1 && child > 1, "grand {grand} child {child}");
        let own = unsafe { libc::getpgrp() };
        assert_ne!(i64::from(own), grand);
        hub.kill(&id).expect("kill");
        assert_dead(grand);
        assert_dead(child);
        let text = hub.read_output(&id, 2_000).unwrap();
        assert!(text.contains("status: killed"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn monitor_emits_matching_lines_and_cancel_stops_it() {
        let dir = scratch("mon");
        let hub = TaskHub::new("mon");
        remember(&hub);
        let id = hub
            .start_monitor(
                &dir,
                Some("printf 'alpha\\nbeta\\n'; sleep 30".into()),
                None,
                Some("beta".into()),
                10_000,
                "lines".into(),
            )
            .expect("monitor");
        let notes = wait_until(&hub, |n| n.iter().any(|line| line.contains("beta")));
        assert!(notes.iter().any(|n| n.contains("beta")), "{notes:?}");
        assert!(notes.iter().all(|n| !n.contains("alpha")), "{notes:?}");
        hub.kill(&id).expect("cancel");
        let stopped = wait_until(&hub, |n| n.iter().any(|line| line.contains("cancelled")));
        assert!(
            stopped.iter().any(|n| n.contains("cancelled")),
            "{stopped:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn halt_all_reaches_unlinked_sessions_and_bare_cancels() {
        let dir = scratch("halt-all");
        let a = format!("native-halt-all-a-{}", std::process::id());
        let b = format!("native-halt-all-b-{}", std::process::id());
        let c = format!("native-halt-all-c-{}", std::process::id());
        let hub_a = hub_for(&a);
        let hub_b = hub_for(&b);
        hub_a
            .spawn(&dir, "sleep 60 & echo $! > a.pid; wait")
            .unwrap();
        hub_b
            .spawn(&dir, "sleep 60 & echo $! > b.pid; wait")
            .unwrap();
        let pid_a = wait_pid(&dir.join("a.pid"));
        let pid_b = wait_pid(&dir.join("b.pid"));
        let cancel = CancelToken::new();
        watch_cancel(&c, cancel.clone());
        let ids = live_session_ids();
        for id in [&a, &b, &c] {
            assert!(ids.contains(id), "{id} missing from {ids:?}");
        }
        // Halt only this test's sessions, the same way `halt_all_sessions` does for each id.
        for id in ids.iter().filter(|id| [&a, &b, &c].contains(id)) {
            halt_session(id);
        }
        assert!(hub_a.is_halted());
        assert!(hub_b.is_halted());
        assert!(cancel.is_cancelled());
        assert_dead(pid_a);
        assert_dead(pid_b);
        forget_session(&a);
        forget_session(&b);
        forget_session(&c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn halt_kills_tasks_monitors_and_linked_children() {
        let dir = scratch("halt");
        let parent = format!("native-halt-{}", std::process::id());
        let child = format!("native-halt-child-{}", std::process::id());
        let parent_hub = hub_for(&parent);
        let child_hub = hub_for(&child);
        let parent_task = parent_hub
            .spawn(&dir, "sleep 60 & echo $! > p.pid; wait")
            .unwrap();
        let child_task = child_hub
            .spawn(&dir, "sleep 60 & echo $! > c.pid; wait")
            .unwrap();
        let _ = parent_task;
        let _ = child_task;
        let mon = child_hub
            .start_monitor(
                &dir,
                Some("sleep 60".into()),
                None,
                None,
                60_000,
                String::new(),
            )
            .unwrap();
        let pid_p = wait_pid(&dir.join("p.pid"));
        let pid_c = wait_pid(&dir.join("c.pid"));
        let cancel = CancelToken::new();
        watch_cancel(&child, cancel.clone());
        link_child(&parent, &child);
        halt_tree(&parent);
        assert!(cancel.is_cancelled());
        assert!(parent_hub.is_halted());
        assert!(child_hub.is_halted());
        assert_dead(pid_p);
        assert_dead(pid_c);
        assert!(child_hub.spawn(&dir, "echo no").is_err());
        let _ = mon;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn kill_stops_a_grandchild() {
        let dir = scratch("tree-win");
        let hub = TaskHub::new("tree-win");
        remember(&hub);
        let script = concat!(
            "$c = Start-Process -PassThru -WindowStyle Hidden -FilePath powershell ",
            "-ArgumentList '-NoProfile','-Command','$g = Start-Process -PassThru -WindowStyle Hidden ping -ArgumentList ''-n'',''40'',''127.0.0.1''; Set-Content -LiteralPath grand.pid $g.Id; Wait-Process -Id $g.Id'; ",
            "Set-Content -LiteralPath 'child.pid' -Value $c.Id; ",
            "Wait-Process -Id $c.Id"
        );
        let id = hub.spawn(&dir, script).expect("spawn");
        let grand = wait_pid(&dir.join("grand.pid"));
        let child = wait_pid(&dir.join("child.pid"));
        assert!(grand > 1 && child > 1, "grand {grand} child {child}");
        hub.kill(&id).expect("kill");
        assert_dead(grand);
        assert_dead(child);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn remember(hub: &Arc<TaskHub>) {
        lock_world().hubs.insert(hub.session_id(), Arc::clone(hub));
    }

    #[cfg(unix)]
    fn wait_notice(hub: &TaskHub) -> Vec<String> {
        wait_until(hub, |notes| !notes.is_empty())
    }

    #[cfg(unix)]
    fn wait_until(hub: &TaskHub, pred: impl Fn(&[String]) -> bool) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let notes = hub.drain_notices();
            if pred(&notes) {
                return notes;
            }
            if Instant::now() >= deadline {
                return notes;
            }
            thread::sleep(POLL);
        }
    }

    fn wait_pid(path: &Path) -> i64 {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(n) = text.trim().parse::<i64>() {
                    if n > 1 {
                        return n;
                    }
                }
            }
            thread::sleep(POLL);
        }
        panic!("pid was not written to {}", path.display());
    }

    fn assert_dead(pid: i64) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while process_alive(pid) && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        assert!(!process_alive(pid), "pid {pid} still alive");
    }

    #[cfg(unix)]
    fn process_alive(pid: i64) -> bool {
        if pid <= 0 {
            return false;
        }
        let rc = unsafe { libc::kill(pid as i32, 0) };
        if rc == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    #[cfg(windows)]
    fn process_alive(pid: i64) -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        if pid <= 0 {
            return false;
        }
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok != 0 && code == STILL_ACTIVE as u32
        }
    }
}
