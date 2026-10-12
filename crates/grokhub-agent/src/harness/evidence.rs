//! Proof-carrying run evidence: a compact operational receipt derived from
//! harness spans (and optional outcome / recovery), never a second log.
//!
//! Distinct from [`super::detect::Evidence`] (detector quotes) and Pulse
//! `TopicEvidence`. UI copy may say "Evidence"; the type is [`RunEvidence`].

use std::collections::BTreeMap;

use grokhub_core::outcome::{OutcomeResult, TaskOutcome};

use crate::harness::span::{Span, REPLY_TOOL, VERIFY_TOOL};

/// Cap on action / check / change lines shown in one receipt.
pub const EVIDENCE_LINES_CAP: usize = 12;

/// One turn's user-facing proof receipt. Append-only at the source (spans /
/// outcomes); this view is rebuilt, never mutated in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEvidence {
    pub chat_id: String,
    pub turn: u32,
    pub ts_ms: u64,
    /// What GrokHub was trying to do. `"unavailable"` when no goal was logged.
    pub goal: String,
    pub goal_detail: String,
    /// Checklist-style status lines (verify, findings, outcome).
    pub plan: Vec<String>,
    /// Human-readable tool summaries (not reasoning).
    pub actions: Vec<String>,
    /// Verification and measurement results.
    pub checks: Vec<String>,
    /// Files / UI / ledger changes observed.
    pub changes: Vec<String>,
    /// `verified`, `inferred`, `untested`, or `mocked`.
    pub confidence: String,
    /// Undo / retry / manual next step, or `"unavailable"`.
    pub recovery: String,
    /// True when tools ran successfully but the goal was not verified.
    pub tool_ran_not_verified: bool,
}

fn is_typing(tool: &str) -> bool {
    tool.rsplit("__").next().unwrap_or(tool) == "type"
}

fn typed_chars(span: &Span) -> u64 {
    serde_json::from_str::<serde_json::Value>(&span.args_redacted)
        .ok()
        .and_then(|v| v.get("chars").and_then(|n| n.as_u64()))
        .unwrap_or(0)
}

fn turn_steps<'a>(spans: &'a [Span], chat_id: &str, turn: u32) -> Vec<&'a Span> {
    spans
        .iter()
        .filter(|s| s.chat_id == chat_id && s.turn == turn && s.tool != REPLY_TOOL)
        .collect()
}

fn push_capped(out: &mut Vec<String>, line: String) {
    if out.len() < EVIDENCE_LINES_CAP {
        out.push(line);
    } else if out.len() == EVIDENCE_LINES_CAP {
        out.push(format!("+{} more", 1));
    } else if let Some(last) = out.last_mut() {
        if let Some(n) = last
            .strip_prefix('+')
            .and_then(|s| s.strip_suffix(" more"))
            .and_then(|n| n.parse::<usize>().ok())
        {
            *last = format!("+{} more", n + 1);
        }
    }
}

/// Derive a receipt for one turn. `None` when the turn has no harness step.
/// Prefer calling after turn end so verify / outcome / recovery are present.
pub fn run_evidence(
    spans: &[Span],
    chat_id: &str,
    turn: u32,
    outcome: Option<&TaskOutcome>,
    recovery: Option<&str>,
) -> Option<RunEvidence> {
    let steps = turn_steps(spans, chat_id, turn);
    if steps.is_empty() {
        return None;
    }

    let mut tools: Vec<(&str, usize, u64)> = Vec::new();
    let mut decisions: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut changed, mut still) = (0usize, 0usize);
    let mut verify: Option<bool> = None;
    let mut undo_refs: Vec<String> = Vec::new();
    let mut goal = String::new();
    let mut goal_detail = String::new();
    let mut ts_ms = 0u64;

    for s in &steps {
        ts_ms = ts_ms.max(s.ts_ms);
        match tools.iter_mut().find(|t| t.0 == s.tool) {
            Some(t) => {
                t.1 += 1;
                t.2 += typed_chars(s);
            }
            None => tools.push((s.tool.as_str(), 1, typed_chars(s))),
        }
        *decisions.entry(s.decision.as_str()).or_default() += 1;
        match s.ui_changed {
            Some(true) => changed += 1,
            Some(false) => still += 1,
            None => {}
        }
        if s.tool == VERIFY_TOOL {
            let pass = crate::harness::detect::verify_passed(s);
            verify = Some(verify.unwrap_or(true) && pass);
        }
        if !s.undo_ref.is_empty() && !undo_refs.contains(&s.undo_ref) {
            undo_refs.push(s.undo_ref.clone());
        }
        if goal.is_empty() {
            if let Some(route) = s.route.as_ref() {
                if !route.goal.is_empty() {
                    goal = route.goal.clone();
                    goal_detail = route.goal_reason.clone();
                }
            }
        }
        if goal.is_empty() && !s.goal_step.is_empty() {
            goal = s.goal_step.clone();
        }
    }

    if goal.is_empty() {
        goal = "unavailable".into();
    }

    let mut actions = Vec::new();
    for (tool, n, chars) in &tools {
        let line = if is_typing(tool) {
            format!("{tool} ×{n} ({chars} chars)")
        } else {
            format!("{tool} ×{n}")
        };
        push_capped(&mut actions, line);
    }
    if actions.is_empty() {
        actions.push("unavailable".into());
    }

    let mut findings: Vec<String> = Vec::new();
    let turn_spans: Vec<Span> = spans
        .iter()
        .filter(|s| s.chat_id == chat_id && s.turn == turn)
        .cloned()
        .collect();
    for f in crate::harness::audit::audit_spans(&turn_spans).findings() {
        if !findings.contains(&f.detector) {
            findings.push(f.detector.clone());
        }
    }

    let mut checks = Vec::new();
    let verify_line = match verify {
        Some(true) => "Verify: pass (goal check)".to_string(),
        Some(false) => "Verify: fail (goal check)".to_string(),
        None => "Verify: none (tool ran ≠ goal verified)".to_string(),
    };
    checks.push(verify_line);
    if changed + still == 0 {
        checks.push("Screen change: not measured".into());
    } else {
        checks.push(format!("Screen change: yes {changed}, no {still}"));
    }
    if findings.is_empty() {
        checks.push("Findings: none".into());
    } else {
        for f in &findings {
            push_capped(&mut checks, format!("Finding: {f}"));
        }
    }

    let mut changes = Vec::new();
    if changed > 0 {
        changes.push(format!("UI changed on {changed} step(s)"));
    }
    for u in &undo_refs {
        push_capped(&mut changes, format!("Change ledger: {u}"));
    }
    if changes.is_empty() {
        changes.push("unavailable".into());
    }

    let mut plan = Vec::new();
    match verify {
        Some(true) => plan.push("Goal check: completed".into()),
        Some(false) => plan.push("Goal check: failed".into()),
        None => plan.push("Goal check: not verified".into()),
    }
    for f in &findings {
        plan.push(format!("Detector {f}: failed"));
    }
    if let Some(o) = outcome {
        let status = match o.result {
            OutcomeResult::Success => "completed",
            OutcomeResult::Failure => "failed",
            OutcomeResult::Pending => "pending",
        };
        plan.push(format!("Outcome: {status}"));
        for r in &o.reasons {
            if plan.len() < EVIDENCE_LINES_CAP {
                plan.push(format!("Reason: {r}"));
            }
        }
    } else {
        plan.push("Outcome: unavailable".into());
    }
    if decisions.values().any(|&n| n > 0) {
        let denied = decisions.get("deny").copied().unwrap_or(0)
            + decisions.get("refuse").copied().unwrap_or(0);
        let parked = decisions.get("park").copied().unwrap_or(0);
        if denied > 0 {
            plan.push(format!("Denied / refused: {denied}"));
        }
        if parked > 0 {
            plan.push(format!("Parked for approval: {parked}"));
        }
    }

    // A successful tool is not proof the user outcome succeeded.
    let goal_verified = verify == Some(true)
        || matches!(outcome.map(|o| o.result), Some(OutcomeResult::Success));
    let tool_ran_not_verified = !goal_verified;
    let confidence = if goal_verified {
        "verified"
    } else if verify == Some(false)
        || matches!(outcome.map(|o| o.result), Some(OutcomeResult::Failure))
        || !findings.is_empty()
    {
        "untested"
    } else if !tools.is_empty() {
        "inferred"
    } else {
        "untested"
    }
    .to_string();

    let recovery = recovery
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            outcome.and_then(|o| match o.result {
                OutcomeResult::Failure => Some(format!(
                    "Retry or correct: {}",
                    o.reasons.first().map(String::as_str).unwrap_or("see checks")
                )),
                OutcomeResult::Pending => {
                    Some("Wait for undo window, or undo a change if needed".into())
                }
                OutcomeResult::Success => None,
            })
        })
        .unwrap_or_else(|| "unavailable".into());

    Some(RunEvidence {
        chat_id: chat_id.into(),
        turn,
        ts_ms,
        goal,
        goal_detail,
        plan,
        actions,
        checks,
        changes,
        confidence,
        recovery,
        tool_ran_not_verified,
    })
}

/// One-line header for the collapsed Evidence control.
pub fn evidence_summary(ev: &RunEvidence) -> String {
    let goal = if ev.goal == "unavailable" {
        "no goal logged"
    } else {
        ev.goal.as_str()
    };
    format!("Evidence · Turn {} · {} · {goal}", ev.turn, ev.confidence)
}

/// Newest turn with harness steps for `chat_id`, if any.
pub fn latest_evidence_turn(spans: &[Span], chat_id: &str) -> Option<u32> {
    spans
        .iter()
        .filter(|s| s.chat_id == chat_id && s.tool != REPLY_TOOL)
        .map(|s| s.turn)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::access::AccessMode;
    use crate::harness::detect::fixture_span;
    use crate::harness::hard::HardClass;
    use grokhub_core::outcome::{OutcomeSignals, TaskOutcome};

    fn at(mut s: Span, ts: u64, turn: u32, chat: &str) -> Span {
        s.ts_ms = ts;
        s.chat_id = chat.into();
        s.session_id = chat.into();
        s.turn = turn;
        s
    }

    fn click(ui: bool) -> Span {
        Span::soft_allow(
            "c1",
            "click",
            r#"{"x":1,"y":2}"#,
            "clicked",
            "click OK",
            AccessMode::Supervised,
            "grok_build",
        )
        .with_ui_changed(Some(ui))
    }

    #[test]
    fn empty_turn_has_no_evidence() {
        assert!(run_evidence(&[], "c1", 1, None, None).is_none());
        let reply = at(Span::reply("c1", "hi", &[]), 1, 1, "c1");
        assert!(run_evidence(&[reply], "c1", 1, None, None).is_none());
    }

    #[test]
    fn successful_verify_marks_verified_and_keeps_sections() {
        let mut verify = fixture_span(5, VERIFY_TOOL, "{}", "allow", "pass", None, "");
        verify = at(verify, 5, 1, "c1");
        let mut routed = at(click(true), 1, 1, "c1");
        routed.route = Some(Box::new(crate::route::log::RouteRecord {
            goal: "quality".into(),
            goal_reason: "mistakes can break the system".into(),
            reason: "prepare:hard".into(),
            ..Default::default()
        }));
        let o = TaskOutcome::finished(
            "c1:1",
            "topic:test",
            &OutcomeSignals {
                verify_ok: true,
                ..Default::default()
            },
            9,
        );
        let ev = run_evidence(&[routed, verify], "c1", 1, Some(&o), None).unwrap();
        assert_eq!(ev.goal, "quality");
        assert_eq!(ev.goal_detail, "mistakes can break the system");
        assert_eq!(ev.confidence, "verified");
        assert!(!ev.tool_ran_not_verified);
        assert!(ev.actions.iter().any(|a| a.starts_with("click")));
        assert!(ev.checks.iter().any(|c| c.contains("Verify: pass")));
        assert!(ev.plan.iter().any(|p| p.contains("completed")));
        assert!(evidence_summary(&ev).contains("verified"));
    }

    #[test]
    fn tools_without_verify_are_inferred_not_success() {
        let s = at(click(false), 2, 3, "c1");
        let ev = run_evidence(&[s], "c1", 3, None, None).unwrap();
        assert_eq!(ev.confidence, "inferred");
        assert!(ev.tool_ran_not_verified);
        assert!(ev.checks.iter().any(|c| c.contains("tool ran")));
        assert_eq!(ev.goal, "unavailable");
        assert_eq!(ev.recovery, "unavailable");
    }

    #[test]
    fn failed_outcome_exposes_recovery_and_does_not_claim_success() {
        let park = at(
            Span::hard_park("c1", "run_command", r#"{"command":"rm"}"#, HardClass::Delete),
            4,
            2,
            "c1",
        );
        let o = TaskOutcome::finished(
            "c1:2",
            "topic:x",
            &OutcomeSignals {
                findings: vec!["action_loop".into()],
                retry_exhausted: true,
                ..Default::default()
            },
            9,
        );
        let ev = run_evidence(
            &[park],
            "c1",
            2,
            Some(&o),
            Some("Retry once, then pause for you"),
        )
        .unwrap();
        assert_ne!(ev.confidence, "verified");
        assert!(ev.tool_ran_not_verified);
        assert_eq!(ev.recovery, "Retry once, then pause for you");
        assert!(ev.plan.iter().any(|p| p.contains("failed")));
    }

    #[test]
    fn latest_turn_skips_reply_only() {
        let a = at(click(true), 1, 1, "c1");
        let b = at(click(false), 2, 4, "c1");
        let reply = at(Span::reply("c1", "done", &[]), 3, 5, "c1");
        assert_eq!(latest_evidence_turn(&[a, b, reply], "c1"), Some(4));
    }
}
