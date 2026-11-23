//! What starts a routine (script-first runtime, phase 4): its `timer` and
//! `file` triggers, checked on the scheduler's tick, and its `webhook`
//! trigger, matched by the hook's name. The app's scheduler and inbox call
//! these; the run itself is [`crate::routine_run`].
//!
//! State lives in `{config}/routines/state.json`: each routine's next timer
//! slot and the files a file trigger has already seen. A trigger seen for
//! the first time only primes that state, so turning a routine on never
//! fires it for the past or for files that were already there. A missed
//! slot fires once, as automations do.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use grokhub_core::routine::{self, InputKind, Routine, Timer, Trigger};
use grokhub_core::LocalClock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The trigger state file, in the routines folder.
pub const STATE_FILE: &str = "state.json";
/// Files one file trigger tracks, at most.
pub const MAX_FILES: usize = 500;
/// Runs one routine's file triggers start per tick, at most; the rest wait.
pub const FILES_PER_TICK: usize = 5;
/// The input that receives a changed file's path.
pub const FILE_INPUT: &str = "file";
/// Most of a webhook body a fill reads.
const BODY_WORDS: usize = 8 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TriggerState {
    #[serde(default)]
    pub routines: BTreeMap<String, RoutineState>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineState {
    /// The next timer slot (ms), once primed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run: Option<u64>,
    /// File path to modified time (ms), once primed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<BTreeMap<String, u64>>,
    /// Switched off on the Routines list: no trigger starts it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
}

/// A run a trigger asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct Start {
    pub name: String,
    pub dir: PathBuf,
    pub routine: Routine,
    pub inputs: Map<String, Value>,
    /// What started it, for the report: `timer` or the file's path.
    pub why: String,
}

fn state_path(config_dir: &Path) -> PathBuf {
    routine::routines_dir(config_dir).join(STATE_FILE)
}

pub fn load_state(config_dir: &Path) -> TriggerState {
    std::fs::read_to_string(state_path(config_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(config_dir: &Path, state: &TriggerState) -> Result<(), String> {
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&state_path(config_dir), &text)
}

/// Every routine that loads cleanly, by folder name. A broken one is left
/// out; `routine run` says why.
pub fn routines(config_dir: &Path) -> Vec<(PathBuf, Routine)> {
    all(config_dir)
        .into_iter()
        .filter_map(|(dir, r)| Some((dir, r.ok()?)))
        .collect()
}

/// The next slot after `clock` for `timer`.
pub fn next_slot(timer: Timer, clock: LocalClock) -> u64 {
    let (schedule, at_min) = match timer {
        Timer::Every { minutes } => return clock.now_ms + u64::from(minutes) * 60_000,
        Timer::Daily { at_min } => ("daily", at_min),
        Timer::Weekdays { at_min } => ("weekdays", at_min),
    };
    let job = json!({
        "name": "routine",
        "schedule": schedule,
        "time": format!("{:02}:{:02}", at_min / 60, at_min % 60),
        "instructions": "",
    });
    match serde_json::from_value::<grokhub_core::Automation>(job) {
        Ok(job) => grokhub_core::compute_next_run(&job, clock),
        Err(_) => clock.now_ms + 24 * 3_600_000,
    }
}

/// The runs that timers and file triggers start now, with their state
/// saved. Call it on the scheduler's tick.
pub fn due(config_dir: &Path, clock: LocalClock) -> Vec<Start> {
    let mut state = load_state(config_dir);
    let list = routines(config_dir);
    let mut starts = Vec::new();
    for (dir, r) in &list {
        let row = state.routines.entry(r.name.clone()).or_default();
        if row.off {
            continue;
        }
        let mut timers = Vec::new();
        let mut files: Option<BTreeMap<String, u64>> = None;
        for trigger in &r.triggers {
            match trigger {
                Trigger::Timer(timer) => timers.push(*timer),
                Trigger::File { folder, glob } => files
                    .get_or_insert_with(BTreeMap::new)
                    .extend(scan(&folder_path(folder), glob)),
                Trigger::Webhook(_) | Trigger::Chat => {}
            }
        }
        // Several timers share one next slot: the soonest.
        let next = timers.iter().map(|t| next_slot(*t, clock)).min();
        match row.next_run {
            _ if next.is_none() => row.next_run = None,
            Some(at) if clock.now_ms >= at => {
                row.next_run = next;
                starts.push(start(dir, r, Map::new(), "timer"));
            }
            Some(_) => {}
            None => row.next_run = next,
        }
        let Some(mut now) = files else {
            row.files = None;
            continue;
        };
        // The first scan only primes.
        let Some(seen) = row.files.take() else {
            row.files = Some(now);
            continue;
        };
        let changed: Vec<String> = now
            .iter()
            .filter(|(path, mtime)| seen.get(*path) != Some(*mtime))
            .map(|(path, _)| path.clone())
            .collect();
        for (n, path) in changed.into_iter().enumerate() {
            if n >= FILES_PER_TICK {
                // No run yet: it stays as last seen, so the next tick takes it.
                match seen.get(&path) {
                    Some(old) => now.insert(path, *old),
                    None => now.remove(&path),
                };
                continue;
            }
            let mut inputs = Map::new();
            if r.inputs
                .get(FILE_INPUT)
                .is_some_and(|i| i.kind == InputKind::String)
            {
                inputs.insert(FILE_INPUT.into(), Value::String(path.clone()));
            }
            starts.push(start(dir, r, inputs, &path));
        }
        row.files = Some(now);
    }
    // Routines that are gone lose their state.
    state
        .routines
        .retain(|name, _| list.iter().any(|(_, r)| &r.name == name));
    let _ = save_state(config_dir, &state);
    starts
}

fn start(dir: &Path, r: &Routine, inputs: Map<String, Value>, why: &str) -> Start {
    Start {
        name: r.name.clone(),
        dir: dir.to_path_buf(),
        routine: r.clone(),
        inputs,
        why: why.to_string(),
    }
}

/// `~/x` and relative folders are under the home folder.
fn folder_path(folder: &str) -> PathBuf {
    let folder = folder.trim();
    let home = grokhub_core::paths::user_home().unwrap_or_default();
    match folder
        .strip_prefix("~/")
        .or_else(|| (folder == "~").then_some(""))
    {
        Some(rest) => home.join(rest),
        None if Path::new(folder).is_absolute() => PathBuf::from(folder),
        None => home.join(folder),
    }
}

/// The files directly in `dir` whose names match `glob`, with their
/// modified times (ms), at most [`MAX_FILES`].
pub fn scan(dir: &Path, glob: &str) -> BTreeMap<String, u64> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() || name.starts_with('.') || !wildcard(glob.trim(), &name) {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        out.insert(entry.path().to_string_lossy().into_owned(), mtime);
        if out.len() >= MAX_FILES {
            break;
        }
    }
    out
}

/// `*` and `?` over a file name, case-insensitive on Windows and macOS.
pub fn wildcard(pattern: &str, name: &str) -> bool {
    let fold = |s: &str| {
        if cfg!(any(windows, target_os = "macos")) {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    };
    let (p, n): (Vec<char>, Vec<char>) = (
        fold(pattern).chars().collect(),
        fold(name).chars().collect(),
    );
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// The routines that are switched on.
fn on(config_dir: &Path) -> Vec<(PathBuf, Routine)> {
    let state = load_state(config_dir);
    routines(config_dir)
        .into_iter()
        .filter(|(_, r)| !state.routines.get(&r.name).is_some_and(|s| s.off))
        .collect()
}

/// Every routine folder, by folder name, with the routine or what is wrong
/// with it.
pub fn all(config_dir: &Path) -> Vec<(PathBuf, Result<Routine, Vec<String>>)> {
    let Ok(rd) = std::fs::read_dir(routine::routines_dir(config_dir)) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, Result<Routine, Vec<String>>)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(routine::FILE).is_file())
        .map(|dir| {
            let r = Routine::load(&dir, &crate::routine_run::step_tool);
            (dir, r)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Whether routine `name` is switched on. A new routine is.
pub fn is_on(config_dir: &Path, name: &str) -> bool {
    !load_state(config_dir)
        .routines
        .get(name)
        .is_some_and(|s| s.off)
}

/// Switches routine `name` on or off. Turning it back on primes its
/// triggers again, so it doesn't fire for what happened while it was off.
pub fn set_on(config_dir: &Path, name: &str, on: bool) -> Result<(), String> {
    if !routines(config_dir).iter().any(|(_, r)| r.name == name) {
        return Err(format!("No routine named {name}"));
    }
    let mut state = load_state(config_dir);
    let row = state.routines.entry(name.to_string()).or_default();
    if on && row.off {
        *row = RoutineState::default();
    }
    row.off = !on;
    save_state(config_dir, &state)
}

/// The switched-on routines the `run_routine` chat tool may start, by name.
pub fn for_chat(config_dir: &Path) -> Vec<(PathBuf, Routine)> {
    on(config_dir)
        .into_iter()
        .filter(|(_, r)| r.triggers.contains(&Trigger::Chat))
        .collect()
}

/// The switched-on routine a webhook named `hook` starts, if one listens.
pub fn for_webhook(config_dir: &Path, hook: &str) -> Option<(PathBuf, Routine)> {
    on(config_dir).into_iter().find(|(_, r)| {
        r.triggers
            .iter()
            .any(|t| matches!(t, Trigger::Webhook(name) if name == hook))
    })
}

/// A webhook body as a run's inputs and words. Top-level fields named like
/// an input, of that input's kind, become its value; the body is never a
/// step. The words are the body as text, for a fill.
pub fn webhook_inputs(r: &Routine, body: &Value) -> (Map<String, Value>, String) {
    let mut inputs = Map::new();
    if let Value::Object(fields) = body {
        for (name, input) in &r.inputs {
            if let Some(v) = fields.get(name).filter(|v| input.kind.accepts(v)) {
                inputs.insert(name.clone(), v.clone());
            }
        }
    }
    let mut words = match body {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    if let Some((cut, _)) = words.char_indices().nth(BODY_WORDS) {
        words.truncate(cut);
    }
    (inputs, words)
}

#[cfg(test)]
#[path = "routine_triggers_tests.rs"]
mod tests;
