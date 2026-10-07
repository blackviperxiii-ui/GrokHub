//! Spike-1a recovery ladder. A finding moves its target one rung: retry once,
//! then backtrack (re-observe, then an alternate target), then pause for the
//! user (a soft park). Every rung is written as a span (`harness_recovery`,
//! origin `repair`).
//!
//! A hard-class step is never retried: it pauses at once. A denied or
//! timed-out hard step stays denied until the user approves a fresh card.

use std::collections::BTreeMap;

use crate::harness::detect::{step_hash, Finding};
use crate::harness::hard::{hard_class, HardClass};
use crate::harness::span::{redact_args, Origin, Span};

/// Span tool for a ladder step.
pub const RECOVERY_TOOL: &str = "harness_recovery";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    Retry,
    Backtrack,
    Pause,
}

impl Rung {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Backtrack => "backtrack",
            Self::Pause => "pause",
        }
    }
}

/// What the ladder decided for one finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LadderStep {
    pub rung: Rung,
    pub detector: String,
    /// Tool plus args hash the rung counts against.
    pub target: String,
    /// Set when the step is hard class: it pauses and never retries.
    pub hard: Option<HardClass>,
    /// The span ids the finding cited.
    pub evidence: Vec<String>,
    /// Why, in one line.
    pub reason: String,
    /// The repair turn's prompt (retry / backtrack). `None` on a pause.
    pub prompt: Option<String>,
}

/// Rungs taken per target this task. A user's own message resets it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ladder {
    tried: BTreeMap<String, u8>,
}

/// The cited span the finding is about: the first act or step it quotes.
fn subject<'a>(finding: &Finding, spans: &'a [Span]) -> Option<&'a Span> {
    finding
        .evidence
        .iter()
        .filter_map(|e| spans.get(e.at))
        .find(|s| s.tool == finding.tool)
}

/// Hard class that keeps the ladder from retrying: any step the finding
/// cites, any hard park or deny in the window that no approval answered (a
/// denied or timed-out hard step needs a fresh approval, so nothing in that
/// turn is retried on its own), or the finding's tool by name.
pub fn hard_target(finding: &Finding, spans: &[Span]) -> Option<HardClass> {
    let class_of = |s: &Span| {
        HardClass::parse(&s.approval_class).or_else(|| hard_class(&s.tool, &s.args_redacted))
    };
    let cited = finding
        .evidence
        .iter()
        .filter_map(|e| spans.get(e.at))
        .find_map(class_of);
    let unanswered = spans.iter().enumerate().find_map(|(i, s)| {
        let class = HardClass::parse(&s.approval_class)?;
        let open = match s.decision.as_str() {
            "deny" => true,
            "park" => !spans[i + 1..]
                .iter()
                .any(|t| t.tool == s.tool && t.decision == "approve"),
            _ => false,
        };
        open.then_some(class)
    });
    cited
        .or(unanswered)
        .or_else(|| hard_class(&finding.tool, "{}"))
}

impl Ladder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The user spoke: every target starts again at Retry.
    pub fn reset(&mut self) {
        self.tried.clear();
    }

    /// Rungs already taken for `target`.
    pub fn tried(&self, target: &str) -> u8 {
        self.tried.get(target).copied().unwrap_or(0)
    }

    /// The next rung for this finding. `spans` is the window the finding came from.
    pub fn next(&mut self, finding: &Finding, spans: &[Span]) -> LadderStep {
        let args = subject(finding, spans)
            .map(|s| s.args_redacted.clone())
            .unwrap_or_default();
        let target = format!("{}#{}", finding.tool, step_hash(&finding.tool, &args));
        let evidence = finding
            .evidence
            .iter()
            .map(|e| e.span.clone())
            .collect::<Vec<_>>();
        let hard = hard_target(finding, spans);
        let n = self.tried.entry(target.clone()).or_insert(0);
        *n = n.saturating_add(1);
        let rung = match (hard, *n) {
            (Some(_), _) => Rung::Pause,
            (None, 1) => Rung::Retry,
            (None, 2) => Rung::Backtrack,
            _ => Rung::Pause,
        };
        let reason = match hard {
            Some(c) => format!(
                "{}: hard-class {} is never retried on its own; it needs your approval",
                finding.detector,
                c.as_str()
            ),
            None => format!("{}: {}", finding.detector, finding.detail),
        };
        let prompt = match rung {
            Rung::Retry => Some(format!(
                "GrokHub's check: {}. Try that step once more, then take a screenshot and say only what changed. \
                 If it is done, run the check and reply VERIFY_OK only when it passes.",
                finding.detail
            )),
            Rung::Backtrack => Some(format!(
                "GrokHub's check: {}. Don't repeat the same step. Take a fresh screenshot first, then try a \
                 different target than before ({} {}).",
                finding.detail,
                finding.tool,
                redact_args(&args)
            )),
            Rung::Pause => None,
        };
        LadderStep {
            rung,
            detector: finding.detector.clone(),
            target,
            hard,
            evidence,
            reason,
            prompt,
        }
    }
}

/// The span for one ladder step. Args carry the detector, target, and cited
/// span ids; never the step's own args.
pub fn ladder_span(session_id: &str, step: &LadderStep) -> Span {
    let args = serde_json::json!({
        "detector": step.detector,
        "target": step.target,
        "evidence": step.evidence,
    })
    .to_string();
    let mut s = Span::deny(
        session_id,
        RECOVERY_TOOL,
        &args,
        step.rung.as_str(),
        step.hard.map_or("soft", |c| c.as_str()),
    );
    s.decision = step.rung.as_str().into();
    s.claim = step.reason.clone();
    s.from_origin(Origin::Repair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::detect::{claimed_click_no_change, unsupported_assurance, Evidence};

    fn span(
        ts: u64,
        tool: &str,
        args: &str,
        decision: &str,
        class: &str,
        ui: Option<bool>,
        claim: &str,
    ) -> Span {
        let mut s = Span::deny("chat-l", tool, args, "ok", class);
        s.ts_ms = ts;
        s.decision = decision.into();
        s.ui_changed = ui;
        s.claim = claim.into();
        s
    }

    fn click_then_claim() -> Vec<Span> {
        vec![
            span(
                1,
                "click",
                r#"{"x":5,"y":9}"#,
                "allow",
                "soft",
                Some(false),
                "grokhub-desktop click",
            ),
            span(
                2,
                "reply",
                "{}",
                "say",
                "soft",
                None,
                "I clicked Save and it worked.",
            ),
        ]
    }

    #[test]
    fn soft_finding_climbs_retry_backtrack_pause_and_reset_starts_over() {
        let spans = click_then_claim();
        let finding = claimed_click_no_change(&spans).remove(0);
        let mut ladder = Ladder::new();
        let a = ladder.next(&finding, &spans);
        assert_eq!(a.rung, Rung::Retry);
        assert_eq!(
            a.target,
            format!("click#{}", step_hash("click", r#"{"x":5,"y":9}"#))
        );
        assert_eq!(a.evidence, vec!["chat-l:1", "chat-l:1", "chat-l:2"]);
        assert!(a
            .prompt
            .as_deref()
            .unwrap()
            .starts_with("GrokHub's check: `click` changed nothing on screen"));
        let b = ladder.next(&finding, &spans);
        assert_eq!(b.rung, Rung::Backtrack);
        assert!(b
            .prompt
            .as_deref()
            .unwrap()
            .contains(r#"different target than before (click {"x":5,"y":9})"#));
        let c = ladder.next(&finding, &spans);
        assert_eq!((c.rung, c.prompt.clone()), (Rung::Pause, None));
        assert_eq!(
            ladder.next(&finding, &spans).rung,
            Rung::Pause,
            "stays paused"
        );
        assert_eq!(ladder.tried(&a.target), 4);
        ladder.reset();
        assert_eq!(ladder.next(&finding, &spans).rung, Rung::Retry);
    }

    #[test]
    fn hard_class_is_never_auto_retried() {
        // A hard send that ran (approved) and then a claim with no UI change.
        let spans = vec![
            span(
                1,
                "type",
                r#"{"chars":12}"#,
                "allow",
                "send",
                Some(false),
                "grokhub-desktop type",
            ),
            span(
                2,
                "reply",
                "{}",
                "say",
                "soft",
                None,
                "Sent it successfully.",
            ),
        ];
        let finding = claimed_click_no_change(&spans).remove(0);
        let mut ladder = Ladder::new();
        for _ in 0..3 {
            let step = ladder.next(&finding, &spans);
            assert_eq!(step.rung, Rung::Pause);
            assert_eq!(step.hard, Some(HardClass::Send));
            assert_eq!(step.prompt, None);
            assert_eq!(
                step.reason,
                "claimed_click_no_change: hard-class send is never retried on its own; it needs your approval"
            );
        }
        // A named hard tool with a soft-looking span is still hard by its name.
        let named = Finding {
            detector: "action_loop".into(),
            tool: "gmail__send_message".into(),
            detail: "loop".into(),
            evidence: vec![Evidence {
                span: "chat-l:7".into(),
                at: 0,
                field: "tool+args".into(),
                quote: String::new(),
            }],
        };
        let spans = vec![span(
            7,
            "gmail__send_message",
            "{}",
            "allow",
            "soft",
            None,
            "",
        )];
        assert_eq!(Ladder::new().next(&named, &spans).rung, Rung::Pause);
    }

    #[test]
    fn denied_or_timed_out_hard_step_waits_for_fresh_approval() {
        for why in ["Jeremy denied", "timed out — fail-closed Deny"] {
            let mut deny = span(1, "hard_send_stub", "{}", "deny", "send", None, "denied");
            deny.result = why.into();
            let spans = vec![
                span(
                    0,
                    "hard_send_stub",
                    "{}",
                    "park",
                    "send",
                    None,
                    "needs Jeremy approve (Send)",
                ),
                deny,
                span(2, "reply", "{}", "say", "soft", None, "I sent the email."),
            ];
            let finding = unsupported_assurance(&spans).remove(0);
            assert_eq!(finding.tool, "hard_send_stub");
            let step = Ladder::new().next(&finding, &spans);
            assert_eq!(step.rung, Rung::Pause, "{why}");
            assert_eq!(step.hard, Some(HardClass::Send));
            assert_eq!(step.prompt, None);
        }
    }

    #[test]
    fn any_finding_in_a_turn_with_an_unanswered_hard_deny_pauses() {
        let mut deny = span(1, "hard_send_stub", "{}", "deny", "send", None, "denied");
        deny.result = "timed out — fail-closed Deny".into();
        let spans = vec![
            deny,
            span(
                1,
                "reply",
                "{}",
                "say",
                "soft",
                None,
                "Done.\nGOAL_COMPLETE",
            ),
        ];
        let done = crate::harness::detect::done_without_criteria(&spans).remove(0);
        assert_eq!(done.tool, "reply");
        let step = Ladder::new().next(&done, &spans);
        assert_eq!((step.rung, step.hard), (Rung::Pause, Some(HardClass::Send)));
        // Approved and ran: the park is answered, so a soft finding may retry.
        let ok = vec![
            span(1, "hard_send_stub", "{}", "park", "send", None, ""),
            span(2, "hard_send_stub", "{}", "approve", "send", None, ""),
            span(3, "click", "{}", "allow", "soft", Some(false), ""),
            span(4, "reply", "{}", "say", "soft", None, "Clicked it."),
        ];
        let f = claimed_click_no_change(&ok).remove(0);
        assert_eq!(Ladder::new().next(&f, &ok).rung, Rung::Retry);
    }

    #[test]
    fn every_rung_is_a_repair_span_with_no_step_args() {
        let spans = vec![
            span(
                1,
                "type",
                r#"{"chars":8}"#,
                "allow",
                "soft",
                Some(false),
                "grokhub-desktop type",
            ),
            span(2, "reply", "{}", "say", "soft", None, "Typed it, done."),
        ];
        let finding = claimed_click_no_change(&spans).remove(0);
        let mut ladder = Ladder::new();
        let rungs: Vec<Span> = (0..3)
            .map(|_| ladder_span("chat-l", &ladder.next(&finding, &spans)))
            .collect();
        let got: Vec<(&str, &str, Origin)> = rungs
            .iter()
            .map(|s| (s.tool.as_str(), s.decision.as_str(), s.origin))
            .collect();
        assert_eq!(
            got,
            vec![
                (RECOVERY_TOOL, "retry", Origin::Repair),
                (RECOVERY_TOOL, "backtrack", Origin::Repair),
                (RECOVERY_TOOL, "pause", Origin::Repair),
            ]
        );
        let target = format!("type#{}", step_hash("type", r#"{"chars":8}"#));
        assert_eq!(
            rungs[0].args_redacted,
            format!(
                r#"{{"detector":"claimed_click_no_change","evidence":["chat-l:1","chat-l:1","chat-l:2"],"target":"{target}"}}"#
            )
        );
        assert_eq!(rungs[2].result, "pause");
        assert_eq!(rungs[2].approval_class, "soft");
    }
}
