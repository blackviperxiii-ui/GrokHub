//! Spike-1a cheap two-pass audit over a span trace. Deterministic, no model
//! calls.
//!
//! Pass 1 reads a condensed summary of each span (kind, decision, step hash,
//! `ui_changed`, and what a claim asserts, never its words) and flags the
//! turns worth a look. Pass 2 loads the full spans of only those windows and
//! runs the detectors there.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::harness::detect::{
    action_loop, claimed_click_no_change, claims_done, claims_success, done_without_criteria,
    failed_result, span_kind, step_hash, unsupported_assurance, Finding, SpanKind, LOOP_REPEATS,
    LOOP_WINDOW,
};
use crate::harness::span::{span_path, Span};

/// Pass 1's view of one span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanDigest {
    pub kind: SpanKind,
    pub decision: String,
    pub chat_id: String,
    pub turn: u32,
    pub hash: String,
    pub result_hash: String,
    pub ui_changed: Option<bool>,
    pub failed: bool,
    /// Claim spans: asserts success / done / carries VERIFY_OK.
    pub success: bool,
    pub done: bool,
    pub verify_ok: bool,
    pub verify_pass: bool,
}

impl SpanDigest {
    pub fn of(span: &Span) -> Self {
        let kind = span_kind(span);
        let claim = kind == SpanKind::Claim;
        Self {
            kind,
            decision: span.decision.clone(),
            chat_id: span.chat_id.clone(),
            turn: span.turn,
            hash: step_hash(&span.tool, &span.args_redacted),
            result_hash: step_hash("", &span.result),
            ui_changed: span.ui_changed,
            failed: failed_result(&span.result),
            success: claim && claims_success(&span.claim),
            done: claim && claims_done(&span.claim),
            verify_ok: claim && grokhub_core::verify::has_verify_ok(&span.claim),
            verify_pass: crate::harness::detect::verify_passed(span),
        }
    }
}

/// A run of spans from one chat turn: `spans[start..end]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub chat_id: String,
    pub turn: u32,
    pub start: usize,
    pub end: usize,
    /// Why pass 1 flagged it (detector ids). Empty: not flagged.
    pub flags: Vec<&'static str>,
}

/// Pass 1 counts for one window (kinds, decisions, step-hash repeats).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowSummary {
    pub kinds: BTreeMap<&'static str, usize>,
    pub decisions: BTreeMap<String, usize>,
    pub max_repeat: usize,
}

fn kind_name(k: SpanKind) -> &'static str {
    match k {
        SpanKind::Act => "act",
        SpanKind::Step => "step",
        SpanKind::Claim => "claim",
        SpanKind::Verify => "verify",
        SpanKind::Gate => "gate",
        SpanKind::Recovery => "recovery",
    }
}

pub fn summarize(digests: &[SpanDigest]) -> WindowSummary {
    let mut out = WindowSummary::default();
    let steps: Vec<&SpanDigest> = digests
        .iter()
        .filter(|d| matches!(d.kind, SpanKind::Act | SpanKind::Step))
        .collect();
    for d in digests {
        *out.kinds.entry(kind_name(d.kind)).or_default() += 1;
        *out.decisions.entry(d.decision.clone()).or_default() += 1;
    }
    for end in 0..steps.len() {
        let w = &steps[end.saturating_sub(LOOP_WINDOW - 1)..=end];
        let n = w.iter().filter(|d| d.hash == steps[end].hash).count();
        out.max_repeat = out.max_repeat.max(n);
    }
    out
}

/// Split into per-turn windows and flag the ones a detector could fire on.
/// A turn with only a claim and no harness step is not audited for
/// `unsupported_assurance` (a plain coding chat writes no step spans).
pub fn pass1(digests: &[SpanDigest]) -> Vec<Window> {
    let mut windows: Vec<Window> = Vec::new();
    for (i, d) in digests.iter().enumerate() {
        match windows.last_mut() {
            Some(w) if w.chat_id == d.chat_id && w.turn == d.turn => w.end = i + 1,
            _ => windows.push(Window {
                chat_id: d.chat_id.clone(),
                turn: d.turn,
                start: i,
                end: i + 1,
                flags: Vec::new(),
            }),
        }
    }
    for w in &mut windows {
        let ds = &digests[w.start..w.end];
        let sum = summarize(ds);
        let has_step = ds.iter().any(|d| {
            matches!(
                d.kind,
                SpanKind::Act | SpanKind::Step | SpanKind::Gate | SpanKind::Verify
            )
        });
        let mut last_act_still = false;
        let mut ui_false_claim = false;
        let mut backed = false;
        let mut unbacked_claim = false;
        for d in ds {
            match d.kind {
                SpanKind::Act | SpanKind::Step => {
                    backed |= !d.failed;
                    if d.kind == SpanKind::Act {
                        last_act_still = d.ui_changed == Some(false);
                    }
                }
                SpanKind::Verify => backed |= d.verify_pass,
                SpanKind::Claim => {
                    ui_false_claim |= last_act_still && d.success;
                    unbacked_claim |= !backed && d.success;
                    last_act_still = false;
                    backed = false;
                }
                _ => {}
            }
        }
        if ui_false_claim {
            w.flags
                .push(crate::harness::detect::CLAIMED_CLICK_NO_CHANGE);
        }
        if sum.max_repeat >= LOOP_REPEATS {
            w.flags.push(crate::harness::detect::ACTION_LOOP);
        }
        let verified = ds.iter().any(|d| d.verify_ok || d.verify_pass);
        if ds.iter().any(|d| d.done) && !verified {
            w.flags.push(crate::harness::detect::DONE_WITHOUT_CRITERIA);
        }
        if has_step && unbacked_claim {
            w.flags.push(crate::harness::detect::UNSUPPORTED_ASSURANCE);
        }
    }
    windows
}

/// Pass 2 for one flagged window: the full spans, the flagged detectors only.
pub fn pass2(window: &Window, spans: &[Span]) -> Vec<Finding> {
    let mut out = Vec::new();
    for flag in &window.flags {
        out.extend(match *flag {
            crate::harness::detect::CLAIMED_CLICK_NO_CHANGE => claimed_click_no_change(spans),
            crate::harness::detect::ACTION_LOOP => action_loop(spans),
            crate::harness::detect::DONE_WITHOUT_CRITERIA => done_without_criteria(spans),
            crate::harness::detect::UNSUPPORTED_ASSURANCE => unsupported_assurance(spans),
            _ => Vec::new(),
        });
    }
    out
}

/// One audited window and what pass 2 found in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowAudit {
    pub window: Window,
    pub spans: Vec<Span>,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Audit {
    /// Every window pass 1 saw.
    pub windows: Vec<Window>,
    /// Pass 2 results for the flagged windows only.
    pub flagged: Vec<WindowAudit>,
}

impl Audit {
    pub fn findings(&self) -> impl Iterator<Item = &Finding> {
        self.flagged.iter().flat_map(|w| w.findings.iter())
    }
}

/// Both passes over spans already in memory.
pub fn audit_spans(spans: &[Span]) -> Audit {
    let digests: Vec<SpanDigest> = spans.iter().map(SpanDigest::of).collect();
    let windows = pass1(&digests);
    let flagged = windows
        .iter()
        .filter(|w| !w.flags.is_empty())
        .map(|w| {
            let slice = spans[w.start..w.end].to_vec();
            WindowAudit {
                window: w.clone(),
                findings: pass2(w, &slice),
                spans: slice,
            }
        })
        .collect();
    Audit { windows, flagged }
}

/// The fields pass 1 reads off a span line. Args and results are hashed, the
/// claim is reduced to flags, and nothing else is kept.
#[derive(Deserialize)]
struct DigestLine {
    #[serde(default)]
    tool: String,
    #[serde(default)]
    args_redacted: String,
    #[serde(default)]
    result: String,
    #[serde(default)]
    claim: String,
    #[serde(default)]
    decision: String,
    #[serde(default)]
    chat_id: String,
    #[serde(default)]
    turn: u32,
    #[serde(default)]
    ui_changed: Option<bool>,
}

impl DigestLine {
    /// The slim span pass 1 digests: claim and result stay only as flags and hashes.
    fn digest(self) -> SpanDigest {
        let mut s = Span::deny("", &self.tool, "", &self.result, "soft");
        s.args_redacted = self.args_redacted;
        s.claim = self.claim;
        s.decision = self.decision;
        s.chat_id = self.chat_id;
        s.turn = self.turn;
        s.ui_changed = self.ui_changed;
        SpanDigest::of(&s)
    }
}

/// Both passes over `spans/<session>.jsonl`. Pass 1 parses each line into a
/// digest; pass 2 parses full spans only for the lines in flagged windows.
/// `only` narrows pass 2 to one chat turn (`(chat_id, turn)`).
pub fn audit_file(
    config_dir: &Path,
    session_id: &str,
    only: Option<(&str, u32)>,
) -> Result<Audit, String> {
    let path = span_path(config_dir, session_id);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Audit::default()),
        Err(e) => return Err(e.to_string()),
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut digests = Vec::with_capacity(lines.len());
    for line in &lines {
        let d: DigestLine = serde_json::from_str(line).map_err(|e| e.to_string())?;
        digests.push(d.digest());
    }
    let windows = pass1(&digests);
    let mut flagged = Vec::new();
    for w in windows.iter().filter(|w| !w.flags.is_empty()) {
        if only.is_some_and(|(chat, turn)| w.chat_id != chat || w.turn != turn) {
            continue;
        }
        let mut spans = Vec::with_capacity(w.end - w.start);
        for line in &lines[w.start..w.end] {
            spans.push(serde_json::from_str::<Span>(line).map_err(|e| e.to_string())?);
        }
        flagged.push(WindowAudit {
            window: w.clone(),
            findings: pass2(w, &spans),
            spans,
        });
    }
    Ok(Audit { windows, flagged })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::detect::{
        fixture_action_loop, fixture_claimed_click_no_change, fixture_done_without_criteria,
        fixture_span, fixture_unsupported_assurance,
    };
    use crate::harness::span::append_span;
    use crate::harness::span::REPLY_TOOL;

    fn in_turn(mut v: Vec<Span>, turn: u32) -> Vec<Span> {
        for s in &mut v {
            s.turn = turn;
            s.ts_ms += u64::from(turn) * 100;
        }
        v
    }

    /// Turn 1 clean, 2 claimed click, 3 loop, 4 done without verify, 5 denied send
    /// claimed as sent, 6 a plain chat reply with no harness step.
    fn trace() -> Vec<Span> {
        let clean = vec![
            fixture_span(
                1,
                "click",
                r#"{"x":1,"y":1}"#,
                "allow",
                "clicked",
                Some(true),
                "grokhub-desktop click",
            ),
            fixture_span(
                2,
                REPLY_TOOL,
                "{}",
                "say",
                "",
                None,
                "Clicked it and the menu is open.",
            ),
        ];
        let chat = vec![fixture_span(
            1,
            REPLY_TOOL,
            "{}",
            "say",
            "",
            None,
            "I've updated the README.",
        )];
        [
            in_turn(clean, 1),
            in_turn(fixture_claimed_click_no_change(), 2),
            in_turn(fixture_action_loop(), 3),
            in_turn(fixture_done_without_criteria(), 4),
            in_turn(fixture_unsupported_assurance(), 5),
            in_turn(chat, 6),
        ]
        .concat()
    }

    #[test]
    fn pass1_summarizes_and_flags_only_suspect_turns() {
        let spans = trace();
        let digests: Vec<SpanDigest> = spans.iter().map(SpanDigest::of).collect();
        let windows = pass1(&digests);
        let got: Vec<(u32, usize, usize, Vec<&str>)> = windows
            .iter()
            .map(|w| (w.turn, w.start, w.end, w.flags.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (1, 0, 2, vec![]),
                (2, 2, 4, vec!["claimed_click_no_change"]),
                (3, 4, 9, vec!["action_loop"]),
                (4, 9, 11, vec!["done_without_criteria"]),
                (
                    5,
                    11,
                    14,
                    vec!["done_without_criteria", "unsupported_assurance"]
                ),
                (6, 14, 15, vec![]),
            ]
        );
        let sum = summarize(&digests[4..9]);
        assert_eq!(sum.kinds.get("act"), Some(&5));
        assert_eq!(sum.decisions.get("allow"), Some(&5));
        assert_eq!(sum.max_repeat, 4);
        let sum = summarize(&digests[11..14]);
        assert_eq!(sum.kinds.get("gate"), Some(&2));
        assert_eq!(sum.kinds.get("claim"), Some(&1));
        assert_eq!(sum.max_repeat, 0);
    }

    #[test]
    fn pass2_runs_only_the_flagged_detectors_on_flagged_windows() {
        let audit = audit_spans(&trace());
        let got: Vec<(u32, Vec<&str>)> = audit
            .flagged
            .iter()
            .map(|w| {
                (
                    w.window.turn,
                    w.findings.iter().map(|f| f.detector.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                // The click's allow span backs "it worked"; only the UI check fires.
                (2, vec!["claimed_click_no_change"]),
                (3, vec!["action_loop"]),
                // The click ran, so "updated" is backed; GOAL_COMPLETE still lacks VERIFY_OK.
                (4, vec!["done_without_criteria"]),
                (5, vec!["done_without_criteria", "unsupported_assurance"]),
            ]
        );
        assert_eq!(audit.findings().count(), 5);
        assert_eq!(audit.flagged[0].spans.len(), 2);
    }

    #[test]
    fn audit_file_loads_full_spans_only_for_flagged_windows() {
        let dir = crate::harness::test_dir("audit-file");
        for s in trace() {
            append_span(&dir, &s).unwrap();
        }
        // A digest-only line in clean turn 1: pass 1 reads it, pass 2 would
        // fail to parse it as a full span. The audit must never load it.
        let path = span_path(&dir, "fx");
        let text = std::fs::read_to_string(&path).unwrap();
        let mut lines: Vec<&str> = text.lines().collect();
        let slim =
            r#"{"tool":"scroll","decision":"allow","chat_id":"fx","turn":1,"ui_changed":true}"#;
        lines.insert(1, slim);
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        assert!(
            crate::harness::read_spans(&dir, "fx").is_err(),
            "the slim line is not a full span"
        );

        let audit = audit_file(&dir, "fx", None).unwrap();
        assert_eq!(audit.windows.len(), 6);
        assert_eq!(audit.windows[0].end - audit.windows[0].start, 3);
        assert_eq!(audit.flagged.len(), 4);
        assert_eq!(audit.findings().count(), 5);
        let one = audit_file(&dir, "fx", Some(("fx", 3))).unwrap();
        assert_eq!(one.flagged.len(), 1);
        assert_eq!(one.flagged[0].findings[0].detector, "action_loop");
        assert_eq!(audit_file(&dir, "nobody", None).unwrap(), Audit::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
