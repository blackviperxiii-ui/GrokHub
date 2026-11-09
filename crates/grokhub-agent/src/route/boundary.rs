//! Round 3 item 34: the router moves effort only at a boundary, and general
//! helpers run briefed on the cheap lane.
//!
//! A boundary is a new user turn or a compaction. Between two of them the
//! effort the run started the segment with is sent on every call, so a
//! mid-run climb or drop doesn't change the request. The ladder still learns
//! from every step; its new rung is used from the next boundary. The class
//! floor always wins, so a hard step never goes out below High, and a model
//! change (a fallback) starts a new hold.
//!
//! With it on, a general helper (no persona, no worktree) also starts fresh
//! on the router's cheap lane instead of forking the chat, and its prompt
//! must carry a brief: goal, constraints, edge cases, done-when and files.
//!
//! Off until the eval shows it pays: `enabled = true` in
//! `boundary_routing.toml` turns both on. Nothing shows in the UI.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use grokhub_core::model_registry::EFFORT_LADDER;

pub const CONFIG: &str = "boundary_routing.toml";
/// The route class a briefed helper runs under: the router's cheap lane.
pub const HELPER_CLASS: &str = "chat:quick";
/// The rule a held effort adds to the route record.
pub const HELD_RULE: &str = "boundary:held";
/// The brief a general helper's prompt must carry, as line labels.
pub const BRIEF: [&str; 5] = ["goal", "constraints", "edge cases", "done when", "files"];
/// Keys kept at once before the oldest go.
const KEYS: usize = 512;

/// Turn boundary routing on after a paired live eval passed it, unless the
/// user's own [`CONFIG`] already says either way: their word stands. True
/// when this wrote the file.
pub fn adopt_after_pass(config_dir: &Path) -> Result<bool, String> {
    let path = config_dir.join(CONFIG);
    if path.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    std::fs::write(
        &path,
        "# Turned on by a paired live eval (--ab boundary_routing) that passed it.\nenabled = true\n",
    )
    .map_err(|e| e.to_string())?;
    Ok(true)
}

/// Whether boundary routing is on (`enabled = true` in [`CONFIG`]).
pub fn enabled(config_dir: &Path) -> bool {
    std::fs::read_to_string(config_dir.join(CONFIG))
        .map(|text| parse_enabled(&text))
        .unwrap_or(false)
}

fn parse_enabled(text: &str) -> bool {
    crate::config_file::parse(CONFIG, text, &["enabled"]).flag("enabled", false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Held {
    turn: u64,
    generation: u32,
    model: String,
    effort: Option<String>,
}

#[derive(Default)]
struct State {
    generations: HashMap<String, u32>,
    held: HashMap<String, Held>,
}

fn state() -> &'static Mutex<State> {
    static STATE: std::sync::OnceLock<Mutex<State>> = std::sync::OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}

/// The chat `key` compacted: its next call starts a new segment.
pub fn note_compaction(key: &str) {
    if key.is_empty() {
        return;
    }
    let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
    if s.generations.len() >= KEYS && !s.generations.contains_key(key) {
        s.generations.clear();
    }
    let g = s.generations.entry(key.to_string()).or_insert(0);
    *g = g.wrapping_add(1);
}

fn rung(effort: Option<&str>) -> Option<usize> {
    let e = effort?;
    EFFORT_LADDER
        .iter()
        .position(|l| l.eq_ignore_ascii_case(e.trim()))
}

/// The effort to send for this call of segment (`key`, `turn`) under
/// `slot` (the chat and its class): the segment's first effort while the
/// turn, compaction and model stay the same and it is not under `floor`,
/// else `effort`, which then starts the hold. True when it held.
pub fn hold(
    slot: &str,
    key: &str,
    turn: u64,
    model: &str,
    effort: Option<String>,
    floor: Option<usize>,
) -> (Option<String>, bool) {
    let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
    let generation = s.generations.get(key).copied().unwrap_or(0);
    if let Some(h) = s.held.get(slot) {
        let same = h.turn == turn && h.generation == generation && h.model == model;
        let above_floor = match (floor, rung(h.effort.as_deref())) {
            (Some(f), Some(r)) => r >= f,
            (Some(_), None) => h.effort.is_none() && effort.is_none(),
            (None, _) => true,
        };
        if same && above_floor {
            let held = h.effort.clone();
            let moved = held != effort;
            return (held, moved);
        }
    }
    if s.held.len() >= KEYS && !s.held.contains_key(slot) {
        s.held.clear();
    }
    s.held.insert(
        slot.to_string(),
        Held {
            turn,
            generation,
            model: model.to_string(),
            effort: effort.clone(),
        },
    );
    (effort, false)
}

/// The brief labels `prompt` lacks (`Goal:`, `Done when:` and so on, one per
/// line, any case).
pub fn missing_brief(prompt: &str) -> Vec<&'static str> {
    let labels: Vec<String> = prompt
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches(['-', '*', '#', ' ']);
            let (label, _) = line.split_once(':')?;
            Some(label.trim().to_ascii_lowercase().replace(['-', '_'], " "))
        })
        .collect();
    BRIEF
        .iter()
        .copied()
        .filter(|want| !labels.iter().any(|l| l == want))
        .collect()
}

/// The refusal for a helper spawned without its brief.
pub fn brief_refusal(missing: &[&str]) -> String {
    format!(
        "a general helper needs a brief in its prompt, one labeled line each: \
         Goal:, Constraints:, Edge cases:, Done when:, Files:. Missing: {}.",
        missing.join(", ")
    )
}

/// Forget every hold and compaction count (tests).
#[cfg(test)]
pub fn reset() {
    let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
    s.generations.clear();
    s.held.clear();
}

#[cfg(test)]
#[path = "boundary_tests.rs"]
mod tests;
