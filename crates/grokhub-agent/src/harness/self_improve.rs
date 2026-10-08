//! Spike-7 (harness design P7): outcome records from spans, the replay gate
//! in front of every skill patch, the patch marks the revert check reads, and
//! the weekly model call.
//!
//! Nothing here executes a step. A replay re-reads a recorded run and the two
//! versions of `SKILL.md` and decides, dry-run only: no network, no model,
//! no host writes (`reject_live` semantics). The weekly call goes through
//! [`crate::route::call_model`] as `background:review`. Accepted changes are
//! written by the caller through the ChangeLedger (`origin: self_manage`).

use std::path::Path;

use grokhub_core::outcome::{OutcomeResult, OutcomeSignals, OutcomeTokens, TaskOutcome};
use grokhub_core::self_review::{
    build_self_review_digest, parse_self_review, rank_proposals, self_review_system_prompt,
    PatchMark, RankedProposal, RejectedProposal, SkillStats, SELF_REVIEW_CLASS,
};

use crate::client::{ClientError, ContentPart, InputItem, ModelClient};
use crate::harness::changes::{ChangeKind, ChangeLedger, ChangeOp};
use crate::harness::detect::{span_kind, verify_passed, SpanKind};
use crate::harness::span::{append_span, read_spans, Origin, Span};
use crate::route::{call_model, CallTokens, ModelCall};
use crate::CancelToken;

/// Spans of the weekly pass and of refused replays go to this session file.
pub const SELF_REVIEW_SESSION: &str = "self-review";
/// Span tool of a replay.
pub const REPLAY_TOOL: &str = "skill_replay";
/// Span tool of the weekly model call.
pub const SELF_REVIEW_TOOL: &str = "self_review";
/// What a live replay is told.
pub const REPLAY_LIVE_REFUSED: &str = "replays are dry-run only";

/// Words that name a hard-class step. A patch may not add one its run never needed.
const HARD_WORDS: &[&str] = &[
    "send",
    "pay",
    "purchase",
    "buy",
    "delete",
    "remove",
    "erase",
    "wipe",
    "password",
    "credential",
    "token",
    "transfer",
    "format",
    "uninstall",
    "shutdown",
    "reboot",
];

/// One finished task from its spans. `findings` are the detector ids that
/// fired on them; `retry_exhausted` is the ladder's pause.
pub fn outcome_from_spans(
    task: &str,
    signature: &str,
    spans: &[Span],
    findings: &[String],
    retry_exhausted: bool,
    finished_at: u64,
) -> TaskOutcome {
    let sig = OutcomeSignals {
        verify_ok: spans.iter().any(verify_passed),
        findings: findings.to_vec(),
        denied: spans
            .iter()
            .any(|s| s.decision == "deny" && s.origin != Origin::Repair),
        retry_exhausted,
        ..Default::default()
    };
    let mut tokens = OutcomeTokens::default();
    for t in spans.iter().filter_map(|s| s.tokens.as_ref()) {
        tokens.input += t.input;
        tokens.cached += t.cached;
        tokens.out += t.out;
        tokens.reasoning += t.reasoning;
        tokens.cost_ticks += t.cost_ticks;
    }
    let episode = spans
        .iter()
        .rev()
        .find(|s| !s.episode.is_empty())
        .map(|s| s.episode.as_str())
        .unwrap_or("");
    TaskOutcome::finished(task, signature, &sig, finished_at)
        .with_spans(spans.iter().map(Span::span_ref))
        .in_episode(episode)
        .with_tokens(tokens)
}

/// The spans of the newest successful recorded run of `skill`, oldest first.
/// Empty when the skill has no such run on disk.
pub fn last_recorded_run(config_dir: &Path, skill: &str, now: u64) -> Vec<Span> {
    let outcomes = grokhub_core::outcome::read_outcomes(config_dir);
    let Some(run) = outcomes.iter().rev().find(|o| {
        o.skill() == Some(skill)
            && o.effective(now) == OutcomeResult::Success
            && !o.span_ids.is_empty()
    }) else {
        return Vec::new();
    };
    spans_for(config_dir, &run.span_ids)
}

/// The spans named by `{session}:{ts_ms}` ids, in id order. Missing ones are skipped.
pub fn spans_for(config_dir: &Path, ids: &[String]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut loaded: Vec<(String, Vec<Span>)> = Vec::new();
    for id in ids {
        let Some((session, ts)) = id.rsplit_once(':') else {
            continue;
        };
        let Ok(ts) = ts.parse::<u64>() else { continue };
        if !loaded.iter().any(|(s, _)| s == session) {
            loaded.push((
                session.to_string(),
                read_spans(config_dir, session).unwrap_or_default(),
            ));
        }
        let spans = loaded.iter().find(|(s, _)| s == session).map(|(_, v)| v);
        if let Some(span) = spans.and_then(|v| v.iter().find(|s| s.ts_ms == ts)) {
            out.push(span.clone());
        }
    }
    out
}

/// Replay options. Live is refused before anything runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplayOpts {
    pub live: bool,
}

/// Same rule as `eval::reject_live`: this build never replays live.
pub fn reject_live_replay(opts: &ReplayOpts) -> Result<(), String> {
    if opts.live {
        Err(REPLAY_LIVE_REFUSED.into())
    } else {
        Ok(())
    }
}

fn words(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn section<'a>(md: &'a str, head: &str) -> &'a str {
    let Some(start) = md.find(head) else {
        return "";
    };
    let rest = &md[start + head.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end].trim()
}

/// Replay one recorded run against a patched `SKILL.md`, dry-run. The run
/// must have passed; every step tool the old version named, the new one
/// still names; a run that passed its verify script keeps a `## Verify`;
/// and the patch adds no hard-class step the run never took.
pub fn replay_patch(
    run: &[Span],
    old_md: &str,
    new_md: &str,
    opts: &ReplayOpts,
) -> Result<(), String> {
    reject_live_replay(opts)?;
    if run.is_empty() {
        return Err("no recorded run of this skill to replay".into());
    }
    if run
        .iter()
        .any(|s| s.decision == "deny" && s.origin != Origin::Repair)
    {
        return Err("the recorded run was denied a step".into());
    }
    let (old_w, new_w) = (words(old_md), words(new_md));
    let mut tools: Vec<String> = Vec::new();
    for s in run
        .iter()
        .filter(|s| matches!(span_kind(s), SpanKind::Act | SpanKind::Step))
    {
        let tool = s.tool.to_ascii_lowercase();
        if !tools.contains(&tool) {
            tools.push(tool);
        }
    }
    for tool in &tools {
        if old_w.contains(tool) && !new_w.contains(tool) {
            return Err(format!(
                "the patch drops the `{tool}` step the last run needed"
            ));
        }
    }
    if run.iter().any(verify_passed)
        && !section(old_md, "## Verify").is_empty()
        && section(new_md, "## Verify").is_empty()
    {
        return Err("the patch drops the verify check the last run passed".into());
    }
    let run_w: Vec<String> = run
        .iter()
        .flat_map(|s| words(&format!("{} {}", s.tool, s.args_redacted)))
        .collect();
    if let Some(w) = HARD_WORDS.iter().find(|w| {
        new_w.iter().any(|x| x == *w)
            && !old_w.iter().any(|x| x == *w)
            && !run_w.iter().any(|x| x == *w)
    }) {
        return Err(format!(
            "the patch adds a `{w}` step the last run never took"
        ));
    }
    Ok(())
}

/// The gate every skill patch goes through: replay the newest recorded run
/// of `skill`. A refusal writes a deny span (no skill text) and returns the
/// note for the card or status line.
pub fn replay_gate(
    config_dir: &Path,
    skill: &str,
    old_md: &str,
    new_md: &str,
    now: u64,
) -> Result<(), String> {
    let run = last_recorded_run(config_dir, skill, now);
    let verdict = replay_patch(&run, old_md, new_md, &ReplayOpts::default());
    let mut span = Span::soft_allow(
        SELF_REVIEW_SESSION,
        REPLAY_TOOL,
        &serde_json::json!({ "skill": grokhub_core::redact_secrets(skill), "steps": run.len() })
            .to_string(),
        "",
        "",
        crate::harness::AccessMode::Supervised,
        "cabin",
    )
    .from_origin(Origin::SelfManage);
    span.ts_ms = now;
    match &verdict {
        Ok(()) => span.result = "pass".into(),
        Err(why) => {
            span.decision = "deny".into();
            span.result = "fail".into();
            span.claim = format!("Replay failed, patch not applied: {why}");
        }
    }
    let _ = append_span(config_dir, &span);
    verdict
        .map_err(|why| format!("Replay failed, so the change to {skill} was not applied: {why}."))
}

/// The newest self-made patch on each skill that is still in effect, with
/// the version it replaced (create is v1).
pub fn patch_marks(config_dir: &Path) -> Vec<PatchMark> {
    let ledger = ChangeLedger::load_kind(config_dir, ChangeKind::Skill);
    let all = ledger.all();
    let undone = |seq: u64| {
        all.iter()
            .any(|c| c.op == ChangeOp::Undo && c.undoes == Some(seq))
    };
    let mut ids: Vec<&str> = Vec::new();
    for c in all {
        if !ids.contains(&c.id.as_str()) {
            ids.push(&c.id);
        }
    }
    let mut out = Vec::new();
    for id in ids {
        let lines: Vec<_> = all.iter().filter(|c| c.id == id).collect();
        let Some(newest) = lines.last() else { continue };
        if newest.op != ChangeOp::Modify
            || newest.origin != Origin::SelfManage
            || undone(newest.seq)
        {
            continue;
        }
        let version = lines
            .iter()
            .filter(|c| {
                c.seq < newest.seq
                    && matches!(
                        c.op,
                        ChangeOp::Create | ChangeOp::Modify | ChangeOp::Restore
                    )
            })
            .count()
            .max(1);
        out.push(PatchMark {
            skill: id.to_string(),
            at: newest.at,
            version,
        });
    }
    out
}

/// What the weekly call returned after the rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WeeklyPass {
    pub kept: Vec<RankedProposal>,
    pub refused: Vec<RejectedProposal>,
    pub tokens: CallTokens,
}

/// The weekly model call: one routed `background:review` call at the
/// background effort, then the fixed rules. Refused targets are logged as
/// scope findings; the call's tokens go on a span.
pub fn run_weekly(
    config_dir: &Path,
    client: &dyn ModelClient,
    model: &str,
    stats: &[SkillStats],
    now: u64,
    cancel: &CancelToken,
) -> Result<WeeklyPass, ClientError> {
    let input = vec![
        InputItem::Message {
            role: "system".into(),
            content: vec![ContentPart::InputText(self_review_system_prompt().into())],
        },
        InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(build_self_review_digest(stats))],
        },
    ];
    let call = ModelCall::xai(
        model,
        Some(grokhub_core::BACKGROUND_EFFORT),
        SELF_REVIEW_CLASS,
        SELF_REVIEW_SESSION,
        input,
    );
    let routed = call_model(client, &call, cancel)?;
    let (kept, refused) = rank_proposals(parse_self_review(&routed.out.text), stats);
    for r in &refused {
        crate::harness::changes::note_proposal_finding(config_dir, &r.target, &r.why);
    }
    let mut span = Span::soft_allow(
        SELF_REVIEW_SESSION,
        SELF_REVIEW_TOOL,
        &serde_json::json!({ "kept": kept.len(), "refused": refused.len() }).to_string(),
        "ok",
        "",
        crate::harness::AccessMode::Supervised,
        "cabin",
    )
    .from_origin(Origin::SelfManage);
    span.ts_ms = now;
    span.tokens = Some(routed.tokens.clone());
    let _ = append_span(config_dir, &span);
    Ok(WeeklyPass {
        kept,
        refused,
        tokens: routed.tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{StreamEvent, TurnOutput, Usage};
    use crate::harness::detect::fixture_span;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const OLD: &str = "---\nname: weekly-report\n---\n\n# weekly-report\n\n## Steps\n1. open the sheet\n2. click Export\n\n## Verify\ntest -f report.csv\n";

    fn dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("grokhub-self-improve-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn recorded(d: &Path, task: &str, at: u64) -> Vec<Span> {
        let mut click = fixture_span(
            at,
            "click",
            r#"{"x":1}"#,
            "allow",
            "clicked",
            Some(true),
            "",
        );
        click.session_id = "chat-a".into();
        let mut verify = fixture_span(at + 1, "verify_script", "{}", "allow", "pass", None, "");
        verify.session_id = "chat-a".into();
        for s in [&click, &verify] {
            append_span(d, s).unwrap();
        }
        let spans = vec![click, verify];
        let o = outcome_from_spans(task, "skill:weekly-report", &spans, &[], false, at + 2);
        grokhub_core::outcome::append_outcome(d, &o).unwrap();
        spans
    }

    /// Counts every model call. Replays must never make one.
    struct Counting(AtomicUsize, &'static str);

    impl ModelClient for Counting {
        fn stream(
            &self,
            _req: &crate::client::ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(TurnOutput {
                text: self.1.into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: Usage {
                    input_tokens: 120,
                    output_tokens: 30,
                    reasoning_tokens: 4,
                    cost_in_usd_ticks: 50,
                    cached_tokens: 64,
                },
            })
        }
    }

    #[test]
    fn outcome_from_spans_reads_verify_deny_and_tokens() {
        let mut v = fixture_span(5, "verify_script", "{}", "allow", "pass", None, "");
        v.tokens = Some(CallTokens {
            input: 10,
            cached: 2,
            out: 3,
            reasoning: 1,
            cost_ticks: 7,
            ..Default::default()
        });
        v.episode = "ep-1".into();
        let o = outcome_from_spans("chat:1", "skill:x", std::slice::from_ref(&v), &[], false, 9);
        assert_eq!(
            (o.result, o.reasons.clone()),
            (OutcomeResult::Success, vec!["verify_ok".to_string()])
        );
        assert_eq!(o.span_ids, vec![v.span_ref()]);
        assert_eq!(o.episode, "ep-1");
        assert_eq!(
            o.tokens,
            OutcomeTokens {
                input: 10,
                cached: 2,
                out: 3,
                reasoning: 1,
                cost_ticks: 7
            }
        );
        let d = fixture_span(6, "bash", "{}", "deny", "denied", None, "");
        let o = outcome_from_spans(
            "chat:2",
            "skill:x",
            &[v, d],
            &["action_loop".into()],
            true,
            9,
        );
        assert_eq!(
            (o.result, o.reasons),
            (
                OutcomeResult::Failure,
                vec![
                    "finding:action_loop".to_string(),
                    "deny".to_string(),
                    "retry_exhausted".to_string()
                ]
            )
        );
    }

    #[test]
    fn replay_passes_a_patch_that_keeps_the_run_and_refuses_ones_that_break_it() {
        let d = dir("replay");
        let run = recorded(&d, "chat-a:1", 1_000);
        let now = 2_000;
        assert_eq!(last_recorded_run(&d, "weekly-report", now), run);
        let keeps = OLD.replace(
            "2. click Export",
            "2. click Export, then wait for the toast",
        );
        assert_eq!(
            replay_patch(&run, OLD, &keeps, &ReplayOpts::default()),
            Ok(())
        );
        let drops_click = OLD.replace("2. click Export", "2. press Ctrl+E");
        assert_eq!(
            replay_patch(&run, OLD, &drops_click, &ReplayOpts::default()),
            Err("the patch drops the `click` step the last run needed".into())
        );
        let drops_verify = OLD.replace("\n## Verify\ntest -f report.csv\n", "\n");
        assert_eq!(
            replay_patch(&run, OLD, &drops_verify, &ReplayOpts::default()),
            Err("the patch drops the verify check the last run passed".into())
        );
        let adds_send = keeps.replace("toast", "toast\n3. send it to the team");
        assert_eq!(
            replay_patch(&run, OLD, &adds_send, &ReplayOpts::default()),
            Err("the patch adds a `send` step the last run never took".into())
        );
        assert_eq!(
            replay_patch(&[], OLD, &keeps, &ReplayOpts::default()),
            Err("no recorded run of this skill to replay".into())
        );
        assert_eq!(
            replay_patch(&run, OLD, &keeps, &ReplayOpts { live: true }),
            Err(REPLAY_LIVE_REFUSED.into())
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_refused_replay_writes_a_deny_span_without_skill_text() {
        let d = dir("gate");
        recorded(&d, "chat-a:1", 1_000);
        let bad = OLD.replace("2. click Export", "2. press Ctrl+E");
        let err = replay_gate(&d, "weekly-report", OLD, &bad, 3_000).unwrap_err();
        assert_eq!(
            err,
            "Replay failed, so the change to weekly-report was not applied: the patch drops the `click` step the last run needed."
        );
        let spans = read_spans(&d, SELF_REVIEW_SESSION).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(
            (
                spans[0].tool.as_str(),
                spans[0].decision.as_str(),
                spans[0].result.as_str()
            ),
            (REPLAY_TOOL, "deny", "fail")
        );
        assert_eq!(spans[0].origin, Origin::SelfManage);
        assert!(!spans[0].args_redacted.contains("Ctrl+E") && !spans[0].claim.contains("Ctrl+E"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_weekly_pass_makes_one_routed_call_and_replays_make_none() {
        let d = dir("weekly");
        let run = recorded(&d, "chat-a:1", 1_000);
        let client = Counting(
            AtomicUsize::new(0),
            "PATCH_SKILL: skill:weekly-report | Wait for Export | 1. open the sheet 2. click Export 3. wait\nPATCH_SKILL: consent.jsonl | Grant | all\n",
        );
        let stats = vec![SkillStats {
            skill: "weekly-report".into(),
            runs: 5,
            failures: 3,
            ..Default::default()
        }];
        let pass = run_weekly(&d, &client, "grok-4.7", &stats, 5_000, &CancelToken::new()).unwrap();
        assert_eq!(client.0.load(Ordering::SeqCst), 1);
        assert_eq!(pass.kept.len(), 1);
        assert_eq!(
            pass.kept[0].numbers,
            "3 of 5 weekly-report runs needed a fix"
        );
        assert_eq!(
            (pass.tokens.class.as_str(), pass.tokens.effort.as_str()),
            ("background:review", "low")
        );
        assert_eq!(
            (
                pass.tokens.input,
                pass.tokens.cached,
                pass.tokens.out,
                pass.tokens.reasoning,
                pass.tokens.cost_ticks
            ),
            (120, 64, 30, 4, 50)
        );
        let findings = crate::harness::read_scope_findings(&d);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].target, "consent.jsonl");
        assert_eq!(
            findings[0].detail,
            "GrokHub never changes the consent store on its own"
        );
        for _ in 0..3 {
            let new = OLD.replace("2. click Export", "2. click Export\n3. wait");
            replay_gate(&d, "weekly-report", OLD, &new, 6_000).unwrap();
            let _ = replay_patch(&run, OLD, &new, &ReplayOpts::default());
        }
        assert_eq!(
            client.0.load(Ordering::SeqCst),
            1,
            "replays made zero model or network calls"
        );
        let spans = read_spans(&d, SELF_REVIEW_SESSION).unwrap();
        assert_eq!(
            spans[0].tokens.as_ref().map(|t| t.out),
            Some(30),
            "the call's tokens are on its span"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn patch_marks_name_the_version_a_self_patch_replaced() {
        let d = dir("marks");
        let skills = d.join("skills");
        let md = |s: &str| {
            format!("---\nname: weekly-report\n---\n\n# weekly-report\n\n## Steps\n{s}\n")
        };
        let write = |s: String| {
            let p = skills.join("weekly-report");
            move || {
                std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
                std::fs::write(p.join("SKILL.md"), s).map_err(|e| e.to_string())
            }
        };
        crate::harness::record_skill_change(
            &d,
            &skills,
            "weekly-report",
            Origin::User,
            "made",
            write(md("one")),
        )
        .unwrap();
        assert!(patch_marks(&d).is_empty(), "a user change is no self patch");
        crate::harness::record_skill_change(
            &d,
            &skills,
            "weekly-report",
            Origin::SelfManage,
            "nightly",
            write(md("two")),
        )
        .unwrap();
        let marks = patch_marks(&d);
        assert_eq!(marks.len(), 1);
        assert_eq!(
            (marks[0].skill.as_str(), marks[0].version),
            ("weekly-report", 1)
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
