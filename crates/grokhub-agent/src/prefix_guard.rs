//! Append-only prefix guard: each model request in a session should start with
//! the previous request byte for byte (system, then tools, then the transcript),
//! so the provider's prompt cache can serve it. The guard keeps a hash of every
//! segment of the last request per run, finds the first segment a new request
//! changes, and names why. Breaks with a known cause (compaction, a history
//! trim, a model or effort switch, a loaded tool schema) are expected; any
//! other break logs one
//! structured line. Nothing here panics or changes the request.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use serde_json::Value;

use crate::client::ResponsesRequest;

/// Which part of the request a segment belongs to, in cache order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    System,
    Tools,
    Input,
}

impl Part {
    pub fn as_str(self) -> &'static str {
        match self {
            Part::System => "system",
            Part::Tools => "tools",
            Part::Input => "input",
        }
    }
}

/// Why the prefix changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The transcript was compacted since the last request.
    Compaction,
    /// Older tool outputs were trimmed since the last request (`compact::trim_pass`).
    Trim,
    ModelSwitch,
    EffortSwitch,
    /// Deferred tool schemas were appended to the tools list.
    ToolsAppended,
    /// Nothing explains it: the system prompt, a tool or an old item was rewritten.
    Unexpected,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Compaction => "compaction",
            Reason::Trim => "trim",
            Reason::ModelSwitch => "model_switch",
            Reason::EffortSwitch => "effort_switch",
            Reason::ToolsAppended => "tools_appended",
            Reason::Unexpected => "unexpected",
        }
    }

    pub fn expected(self) -> bool {
        self != Reason::Unexpected
    }
}

/// The first segment where a request stops repeating the one before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixBreak {
    /// Segment index in cache order (system items, then each tool, then input items).
    pub index: usize,
    pub part: Part,
    pub reason: Reason,
}

/// Segment hashes of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    model: String,
    effort: Option<String>,
    segments: Vec<(Part, u64)>,
    /// Hosted tools (`web_search`, `x_search`) at the end of the tools list.
    /// A loaded schema lands just before them.
    hosted: usize,
}

impl Shape {
    pub fn of(req: &ResponsesRequest) -> Self {
        let body = crate::responses_body(req);
        let input = body["input"].as_array().cloned().unwrap_or_default();
        let lead = input
            .iter()
            .take_while(|item| item["role"] == "system")
            .count();
        let tools = body["tools"].as_array().cloned().unwrap_or_default();
        let hosted = tools
            .iter()
            .rev()
            .take_while(|t| t["type"] != "function")
            .count();
        let mut segments = Vec::with_capacity(input.len() + tools.len());
        segments.extend(input[..lead].iter().map(|v| (Part::System, hash(v))));
        segments.extend(tools.iter().map(|v| (Part::Tools, hash(v))));
        segments.extend(input[lead..].iter().map(|v| (Part::Input, hash(v))));
        Self {
            model: req.model.trim().to_string(),
            effort: req.effort.clone(),
            segments,
            hosted,
        }
    }
}

fn hash(value: &Value) -> u64 {
    let mut h = DefaultHasher::new();
    value.to_string().hash(&mut h);
    h.finish()
}

/// Where `next` stops extending `prev`, and why. `None` when `next` starts with
/// every segment of `prev`. `noted` is the rewrite the run announced since
/// `prev` ([`note_compaction`] or [`note_trim`]); it names any break.
pub fn first_break(prev: &Shape, next: &Shape, noted: Option<Reason>) -> Option<PrefixBreak> {
    let index = prev
        .segments
        .iter()
        .zip(&next.segments)
        .take_while(|(a, b)| a == b)
        .count();
    if index == prev.segments.len() {
        return None;
    }
    let part = next
        .segments
        .get(index)
        .or_else(|| prev.segments.get(index))
        .map_or(Part::Input, |(part, _)| *part);
    let reason = if let Some(noted) = noted {
        noted
    } else if prev.model != next.model {
        Reason::ModelSwitch
    } else if prev.effort != next.effort {
        Reason::EffortSwitch
    } else if tools_only_appended(prev, next) {
        Reason::ToolsAppended
    } else {
        Reason::Unexpected
    };
    Some(PrefixBreak {
        index,
        part,
        reason,
    })
}

/// The new request is the old one with extra tool schemas at the end of the
/// tools list (a deferred schema load), just before any hosted tools, and the
/// same items around them.
fn tools_only_appended(prev: &Shape, next: &Shape) -> bool {
    let split = |s: &Shape| {
        let tools: Vec<u64> = s
            .segments
            .iter()
            .filter(|(p, _)| *p == Part::Tools)
            .map(|(_, h)| *h)
            .collect();
        let rest: Vec<(Part, u64)> = s
            .segments
            .iter()
            .filter(|(p, _)| *p != Part::Tools)
            .copied()
            .collect();
        (tools, rest)
    };
    let (old_tools, old_rest) = split(prev);
    let (new_tools, new_rest) = split(next);
    let (head, hosted) = old_tools.split_at(old_tools.len().saturating_sub(prev.hosted));
    new_tools.len() > old_tools.len()
        && next.hosted == prev.hosted
        && new_tools.starts_with(head)
        && new_tools.ends_with(hosted)
        && new_rest.starts_with(&old_rest)
}

struct Guard {
    last: BTreeMap<String, Shape>,
    noted: BTreeMap<String, Reason>,
    breaks: BTreeMap<String, Vec<PrefixBreak>>,
}

static GUARD: Mutex<Guard> = Mutex::new(Guard {
    last: BTreeMap::new(),
    noted: BTreeMap::new(),
    breaks: BTreeMap::new(),
});

/// Keep the guard small: a long-lived cabin opens many runs.
const MAX_RUNS: usize = 256;

/// Compare `req` with the last request of the same run (`key`), remember it, and
/// return the break if there is one. A run's first request has nothing to break.
pub fn observe(key: &str, req: &ResponsesRequest) -> Option<PrefixBreak> {
    let shape = Shape::of(req);
    let mut g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let noted = g.noted.remove(key);
    if g.last.len() >= MAX_RUNS && !g.last.contains_key(key) {
        g.last.clear();
        g.breaks.clear();
    }
    let found = g
        .last
        .insert(key.to_string(), shape.clone())
        .and_then(|prev| first_break(&prev, &shape, noted));
    if let Some(b) = &found {
        if !b.reason.expected() {
            eprintln!(
                "grokhub prefix_break index={} part={} reason={}",
                b.index,
                b.part.as_str(),
                b.reason.as_str()
            );
        }
        let list = g.breaks.entry(key.to_string()).or_default();
        if list.len() < 64 {
            list.push(b.clone());
        }
    }
    found
}

/// The transcript of this run was compacted: its next request may rewrite the prefix.
pub fn note_compaction(key: &str) {
    GUARD
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .noted
        .insert(key.to_string(), Reason::Compaction);
}

/// Older tool outputs of this run were trimmed: its next request may rewrite the
/// prefix. A compaction noted for the same request still names the break.
pub fn note_trim(key: &str) {
    GUARD
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .noted
        .entry(key.to_string())
        .or_insert(Reason::Trim);
}

/// The run key for a loop: one per conversation, depth and subagent, so a
/// subagent's requests never count as breaks in its parent's prefix.
pub fn run_key(conversation_id: &str, depth: u32, agent_id: Option<&str>) -> String {
    format!("{conversation_id}#{depth}#{}", agent_id.unwrap_or(""))
}

/// Breaks recorded for a run (expected ones included), oldest first.
pub fn breaks(key: &str) -> Vec<PrefixBreak> {
    GUARD
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .breaks
        .get(key)
        .cloned()
        .unwrap_or_default()
}

/// Test helper: fail with the break when a run rewrote its prefix for no named reason.
#[cfg(test)]
pub fn assert_no_unexpected_break(key: &str) {
    let unexpected: Vec<PrefixBreak> = breaks(key)
        .into_iter()
        .filter(|b| !b.reason.expected())
        .collect();
    assert!(
        unexpected.is_empty(),
        "unexpected prefix break: {unexpected:?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ContentPart, InputItem};
    use serde_json::json;

    fn msg(role: &str, text: &str) -> InputItem {
        InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    fn req(input: Vec<InputItem>, tools: Vec<Value>) -> ResponsesRequest {
        ResponsesRequest {
            model: "grok-4.7".into(),
            effort: Some("high".into()),
            input,
            conversation_id: "c".into(),
            tools,
            hosted_search: false,
            call_timeout: None,
        }
    }

    fn tool(name: &str) -> Value {
        json!({"type": "function", "name": name})
    }

    #[test]
    fn an_appended_turn_is_not_a_break() {
        let a = req(
            vec![msg("system", "RULES"), msg("user", "hi")],
            vec![tool("read")],
        );
        let mut items = a.input.clone();
        items.push(msg("assistant", "hello"));
        items.push(msg("user", "next"));
        let b = req(items, vec![tool("read")]);
        assert_eq!(first_break(&Shape::of(&a), &Shape::of(&b), None), None);
    }

    #[test]
    fn a_system_edit_is_reported_at_index_zero() {
        let a = req(
            vec![msg("system", "RULES"), msg("user", "hi")],
            vec![tool("read")],
        );
        let b = req(
            vec![msg("system", "RULES v2"), msg("user", "hi")],
            vec![tool("read")],
        );
        assert_eq!(
            first_break(&Shape::of(&a), &Shape::of(&b), None),
            Some(PrefixBreak {
                index: 0,
                part: Part::System,
                reason: Reason::Unexpected,
            })
        );
    }

    #[test]
    fn a_rewritten_old_item_is_reported_with_its_index() {
        // system(0), read(1), write(2), user(3), assistant(4)
        let tools = vec![tool("read"), tool("write")];
        let a = req(
            vec![
                msg("system", "S"),
                msg("user", "u1"),
                msg("assistant", "a1"),
            ],
            tools.clone(),
        );
        let b = req(
            vec![
                msg("system", "S"),
                msg("user", "u1"),
                msg("assistant", "a1 (edited)"),
                msg("user", "u2"),
            ],
            tools,
        );
        let got = first_break(&Shape::of(&a), &Shape::of(&b), None).unwrap();
        assert_eq!(got.index, 4);
        assert_eq!(got.part, Part::Input);
        assert_eq!(got.reason, Reason::Unexpected);
    }

    #[test]
    fn named_reasons_cover_compaction_model_effort_and_tool_loads() {
        let a = req(
            vec![msg("system", "S"), msg("user", "u")],
            vec![tool("read")],
        );
        let shorter = req(
            vec![msg("system", "S"), msg("user", "summary")],
            vec![tool("read")],
        );
        assert_eq!(
            first_break(
                &Shape::of(&a),
                &Shape::of(&shorter),
                Some(Reason::Compaction)
            )
            .map(|b| b.reason),
            Some(Reason::Compaction)
        );
        let mut other_model = shorter.clone();
        other_model.model = "grok-4.7-fast".into();
        assert_eq!(
            first_break(&Shape::of(&a), &Shape::of(&other_model), None).map(|b| b.reason),
            Some(Reason::ModelSwitch)
        );
        let mut other_effort = shorter.clone();
        other_effort.effort = Some("low".into());
        assert_eq!(
            first_break(&Shape::of(&a), &Shape::of(&other_effort), None).map(|b| b.reason),
            Some(Reason::EffortSwitch)
        );
        let mut items = a.input.clone();
        items.push(msg("assistant", "loading"));
        let loaded = req(items, vec![tool("read"), tool("browser")]);
        assert_eq!(
            first_break(&Shape::of(&a), &Shape::of(&loaded), None),
            Some(PrefixBreak {
                index: 2,
                part: Part::Tools,
                reason: Reason::ToolsAppended,
            })
        );
    }

    #[test]
    fn a_load_before_the_hosted_search_tools_is_an_append() {
        let mut a = req(
            vec![msg("system", "S"), msg("user", "u")],
            vec![tool("read")],
        );
        a.hosted_search = true;
        let mut loaded = a.clone();
        loaded.tools.push(tool("browser"));
        let got = first_break(&Shape::of(&a), &Shape::of(&loaded), None).unwrap();
        assert_eq!(got.part, Part::Tools);
        assert_eq!(got.reason, Reason::ToolsAppended);
        // A tool put in front of the old list is still a rewrite.
        let mut front = a.clone();
        front.tools.insert(0, tool("browser"));
        assert_eq!(
            first_break(&Shape::of(&a), &Shape::of(&front), None).map(|b| b.reason),
            Some(Reason::Unexpected)
        );
    }

    #[test]
    fn observe_tracks_each_run_and_takes_the_compaction_note_once() {
        let key = run_key(&format!("guard-observe-{}", std::process::id()), 0, None);
        let a = req(vec![msg("system", "S"), msg("user", "u")], vec![]);
        assert_eq!(
            observe(&key, &a),
            None,
            "a run's first request has no break"
        );
        let edited = req(vec![msg("system", "S2"), msg("user", "u")], vec![]);
        note_compaction(&key);
        assert_eq!(
            observe(&key, &edited).map(|b| b.reason),
            Some(Reason::Compaction)
        );
        let again = req(vec![msg("system", "S3"), msg("user", "u")], vec![]);
        assert_eq!(
            observe(&key, &again).map(|b| b.reason),
            Some(Reason::Unexpected)
        );
        assert_eq!(breaks(&key).len(), 2);
        // A subagent run under the same conversation is tracked on its own.
        let child = run_key(
            &format!("guard-observe-{}", std::process::id()),
            1,
            Some("a1"),
        );
        assert_ne!(child, key);
        assert_eq!(observe(&child, &a), None);
    }

    #[test]
    fn a_noted_trim_names_the_next_break_and_a_compaction_outranks_it() {
        let key = run_key(&format!("guard-trim-{}", std::process::id()), 0, None);
        let output = |text: &str| InputItem::FunctionCallOutput {
            call_id: "c1".into(),
            output: text.into(),
        };
        let a = req(vec![msg("system", "S"), output("FULL LOG")], vec![]);
        assert_eq!(observe(&key, &a), None);
        let trimmed = req(
            vec![msg("system", "S"), output("[Output dropped: …]")],
            vec![],
        );
        note_trim(&key);
        assert_eq!(
            observe(&key, &trimmed),
            Some(PrefixBreak {
                index: 1,
                part: Part::Input,
                reason: Reason::Trim,
            })
        );
        note_compaction(&key);
        note_trim(&key);
        let summary = req(vec![msg("system", "S"), msg("user", "summary")], vec![]);
        assert_eq!(
            observe(&key, &summary).map(|b| b.reason),
            Some(Reason::Compaction)
        );
        assert_eq!(Reason::Trim.as_str(), "trim");
        assert!(Reason::Trim.expected());
    }
}
