//! Catch a looping episode sooner than the stall backstop. A short ring of
//! (tool, normalized args, result class) per episode: an identical call that
//! fails twice, the same call that changes nothing three times, or the same
//! failure three times in a row of calls re-plans the worker. Read-only on
//! the ring; the kernel writes the span.

use std::collections::VecDeque;

use serde_json::Value;

/// Steps the ring keeps.
pub const LOOP_RING: usize = 8;
/// An identical call failing this many times re-plans.
pub const SAME_FAILED_CALL: usize = 2;
/// The same call that moves nothing, or the same failure result from any
/// calls, this many times re-plans.
pub const SAME_REPEAT: usize = 3;
/// Detector name on a loop re-plan span.
pub const REPEAT_CALL: &str = "repeat_call";

/// How a step came back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The screen changed, or there was no reading and it worked.
    Moved,
    /// It ran and the screen stayed the same.
    Still,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopEntry {
    /// Tool and normalized args.
    pub call: String,
    /// "click 'Save'": what a span names.
    pub label: String,
    pub outcome: Outcome,
    /// The normalized result text.
    pub result: String,
}

/// One-line whitespace, lowercase, keys in order.
fn normalize(text: &str) -> String {
    let text = match serde_json::from_str::<Value>(text) {
        Ok(v) => v.to_string(),
        Err(_) => text.to_string(),
    };
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// "click 'Save'", "click at 120,40", or the bare tool.
pub fn call_label(tool: &str, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or(Value::Null);
    let named = ["label", "name", "target", "title", "text", "key", "keys", "command", "path", "url"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str).filter(|s| !s.trim().is_empty()));
    if let Some(n) = named {
        let n: String = n.trim().chars().take(40).collect();
        return format!("{tool} '{n}'");
    }
    match (v.get("x").and_then(Value::as_i64), v.get("y").and_then(Value::as_i64)) {
        (Some(x), Some(y)) => format!("{tool} at {x},{y}"),
        _ => tool.to_string(),
    }
}

impl LoopEntry {
    pub fn new(tool: &str, args_redacted: &str, outcome: Outcome, result: &str) -> Self {
        let result: String = normalize(result).chars().take(120).collect();
        Self { call: format!("{tool}\u{1f}{}", normalize(args_redacted)), label: call_label(tool, args_redacted), outcome, result }
    }
}

/// Push a step. `Some(reason)` when it closes a loop; the ring then starts over.
pub fn note_call(ring: &mut VecDeque<LoopEntry>, entry: LoopEntry) -> Option<String> {
    if ring.len() >= LOOP_RING {
        ring.pop_front();
    }
    ring.push_back(entry);
    let last = ring.back()?;
    let same_call: Vec<&LoopEntry> = ring.iter().filter(|e| e.call == last.call && e.outcome != Outcome::Moved).collect();
    let failed_calls = same_call.iter().filter(|e| e.outcome == Outcome::Failed).count();
    let reason = if last.outcome == Outcome::Failed && failed_calls >= SAME_FAILED_CALL {
        Some(format!("replan: {} failed {failed_calls}×", last.label))
    } else if last.outcome != Outcome::Moved && same_call.len() >= SAME_REPEAT {
        let how = if last.outcome == Outcome::Failed { "failed" } else { "changed nothing" };
        Some(format!("replan: {} {how} {}×", last.label, same_call.len()))
    } else if last.outcome == Outcome::Failed {
        let n = ring.iter().filter(|e| e.outcome == Outcome::Failed && e.result == last.result).count();
        (n >= SAME_REPEAT).then(|| {
            let r: String = last.result.chars().take(60).collect();
            format!("replan: {n} steps failed with '{r}'")
        })
    } else {
        None
    };
    if reason.is_some() {
        ring.clear();
    }
    reason
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(ring: &mut VecDeque<LoopEntry>, tool: &str, args: &str, outcome: Outcome, result: &str) -> Option<String> {
        note_call(ring, LoopEntry::new(tool, args, outcome, result))
    }

    #[test]
    fn an_identical_failing_click_replans_on_the_second_try() {
        let mut ring = VecDeque::new();
        let save = r#"{"x":120,"y":40,"label":"Save"}"#;
        assert_eq!(push(&mut ring, "click", save, Outcome::Failed, "failed: no window"), None);
        // Same call, keys in another order and spaced differently.
        let again = r#"{ "label": "Save", "y": 40, "x": 120 }"#;
        assert_eq!(push(&mut ring, "click", again, Outcome::Failed, "failed: no window").as_deref(), Some("replan: click 'Save' failed 2×"));
        assert!(ring.is_empty(), "the ring starts over after a re-plan");
    }

    #[test]
    fn the_same_click_that_changes_nothing_replans_on_the_third() {
        let mut ring = VecDeque::new();
        let save = r#"{"label":"Save"}"#;
        assert_eq!(push(&mut ring, "click", save, Outcome::Still, "click ok"), None);
        assert_eq!(push(&mut ring, "scroll", r#"{"dy":3}"#, Outcome::Moved, "scroll ok"), None);
        assert_eq!(push(&mut ring, "click", save, Outcome::Still, "click ok"), None);
        assert_eq!(push(&mut ring, "click", save, Outcome::Still, "click ok").as_deref(), Some("replan: click 'Save' changed nothing 3×"));
    }

    #[test]
    fn the_same_failure_from_different_calls_replans_on_the_third() {
        let mut ring = VecDeque::new();
        for (i, x) in [10, 20].iter().enumerate() {
            let args = format!(r#"{{"x":{x},"y":40}}"#);
            assert_eq!(push(&mut ring, "click", &args, Outcome::Failed, "failed: Window  not found"), None, "call {i}");
        }
        assert_eq!(
            push(&mut ring, "click", r#"{"x":30,"y":40}"#, Outcome::Failed, "failed: window not found").as_deref(),
            Some("replan: 3 steps failed with 'failed: window not found'")
        );
    }

    #[test]
    fn varied_progress_never_replans() {
        let mut ring = VecDeque::new();
        for i in 0..20 {
            let args = format!(r#"{{"x":{i},"y":40}}"#);
            assert_eq!(push(&mut ring, "click", &args, Outcome::Moved, "click ok"), None);
        }
        // The same scroll that keeps moving the page is progress too.
        for _ in 0..10 {
            assert_eq!(push(&mut ring, "scroll", r#"{"dy":3}"#, Outcome::Moved, "scroll ok"), None);
        }
        // Different calls that change nothing are the stall backstop's, not a loop.
        for i in 0..10 {
            let args = format!(r#"{{"x":{i},"y":90}}"#);
            assert_eq!(push(&mut ring, "click", &args, Outcome::Still, "click ok"), None);
        }
        assert_eq!(ring.len(), LOOP_RING);
    }

    #[test]
    fn labels_name_the_target_or_the_spot() {
        assert_eq!(call_label("click", r#"{"x":5,"y":40}"#), "click at 5,40");
        assert_eq!(call_label("key", r#"{"key":"ctrl+s"}"#), "key 'ctrl+s'");
        assert_eq!(call_label("screenshot", "{}"), "screenshot");
    }
}
