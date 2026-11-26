//! Skill quality and ablation eval harness (`--suite skills`, `--ablate-skill`).
//!
//! Replays a fixed prompt set across three arms:
//! - `baseline`: target skill disabled / absent
//! - `full`: full `SKILL.md` active in workspace
//! - `trimmed`: shorter `SKILL.md` variant (or `--skill-override <path>`)
//!
//! Computes per-arm and delta metrics (`pass_rate`, `avg_turns`, `avg_tool_calls`,
//! `input_tokens`, `output_tokens`, `cost_usd`, `wall_ms`, and `net_value`) and
//! writes `<out>.results.json` plus a Markdown summary.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use grokhub_core::wire::AcpEvent;
use serde::{Deserialize, Serialize};

use crate::gate::{Gate, PermMode};
use crate::perm::ConfigGuard;
use crate::{CancelToken, Engine, EngineParts, HaltCheck, ModelClient, NativeEngine, SteerQueue};

use super::coding::results_path;
use super::live::{
    app_system_at, write_atomic, CallDeadline, Capped, Provider, RetryPolicy, Retrying,
    ScriptedPermits, Trace, DEFAULT_CALL_TIMEOUT, TICKS_PER_USD,
};

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillArm {
    Baseline,
    Full,
    Trimmed,
}

impl SkillArm {
    pub fn label(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Full => "full",
            Self::Trimmed => "trimmed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SkillCase {
    pub id: &'static str,
    pub skill_name: &'static str,
    pub prompt: &'static str,
    pub full_skill_md: &'static str,
    pub trimmed_skill_md: &'static str,
    pub required_substrings: &'static [&'static str],
    pub forbidden_substrings: &'static [&'static str],
}

pub fn fixtures() -> &'static [SkillCase] {
    &FIXTURES
}

const FIXTURES: [SkillCase; 6] = [
    SkillCase {
        id: "skill-checklist-release",
        skill_name: "checklist-release",
        prompt: "Prepare the release verification note for v1.4.0.",
        full_skill_md: "---\nname: checklist-release\ndescription: Structured release checklist with rollback plan and SHA verification.\n---\n# Release Checklist\nAlways include these exact section markers in your final note:\n- RELEASE_VERDICT\n- ROLLBACK_PLAN\n- SHA256_VERIFIED\nExplain each item briefly with crisp verification notes and never emit TBD_SKIPPED.\n",
        trimmed_skill_md: "---\nname: checklist-release\ndescription: Structured release checklist with rollback plan and SHA verification.\n---\nInclude exact markers: RELEASE_VERDICT, ROLLBACK_PLAN, SHA256_VERIFIED. Never write TBD_SKIPPED.\n",
        required_substrings: &["RELEASE_VERDICT", "ROLLBACK_PLAN", "SHA256_VERIFIED"],
        forbidden_substrings: &["TBD_SKIPPED"],
    },
    SkillCase {
        id: "skill-json-schema-audit",
        skill_name: "json-schema-audit",
        prompt: "Audit the configuration schema payload and report validation status.",
        full_skill_md: "---\nname: json-schema-audit\ndescription: Audit JSON schemas for required keys and strict property checks.\n---\n# JSON Schema Audit\nWhen auditing a schema payload, report all three exact status markers:\n- SCHEMA_VALID\n- REQUIRED_FIELDS\n- ADDITIONAL_PROPERTIES\nDo not include UNCHECKED anywhere in the output.\n",
        trimmed_skill_md: "---\nname: json-schema-audit\ndescription: Audit JSON schemas for required keys and strict property checks.\n---\nEmit exact markers: SCHEMA_VALID, REQUIRED_FIELDS, ADDITIONAL_PROPERTIES. Avoid UNCHECKED.\n",
        required_substrings: &["SCHEMA_VALID", "REQUIRED_FIELDS", "ADDITIONAL_PROPERTIES"],
        forbidden_substrings: &["UNCHECKED"],
    },
    SkillCase {
        id: "skill-bug-triage-format",
        skill_name: "bug-triage-format",
        prompt: "Summarize the login timeout regression into a triage ticket.",
        full_skill_md: "---\nname: bug-triage-format\ndescription: Format regression triage reports with repro steps, root cause, and severity.\n---\n# Bug Triage Format\nProduce a structured triage report containing these exact labels:\n- REPRO_STEPS\n- ROOT_CAUSE\n- SEVERITY_P1\nNever write WONT_REPRODUCE without verification.\n",
        trimmed_skill_md: "---\nname: bug-triage-format\ndescription: Format regression triage reports with repro steps, root cause, and severity.\n---\nInclude exact labels: REPRO_STEPS, ROOT_CAUSE, SEVERITY_P1. Never write WONT_REPRODUCE.\n",
        required_substrings: &["REPRO_STEPS", "ROOT_CAUSE", "SEVERITY_P1"],
        forbidden_substrings: &["WONT_REPRODUCE"],
    },
    SkillCase {
        id: "skill-sql-migration-guard",
        skill_name: "sql-migration-guard",
        prompt: "Review the add-index migration plan before applying it.",
        full_skill_md: "---\nname: sql-migration-guard\ndescription: Safety guard for SQL schema migrations covering locks, backups, and rollback.\n---\n# SQL Migration Guard\nEvery migration review must explicitly include:\n- DOWN_MIGRATION\n- LOCK_RISK\n- BACKUP_CONFIRMED\nNever output NO_ROLLBACK in an approved review.\n",
        trimmed_skill_md: "---\nname: sql-migration-guard\ndescription: Safety guard for SQL schema migrations covering locks, backups, and rollback.\n---\nInclude exact checks: DOWN_MIGRATION, LOCK_RISK, BACKUP_CONFIRMED. Never output NO_ROLLBACK.\n",
        required_substrings: &["DOWN_MIGRATION", "LOCK_RISK", "BACKUP_CONFIRMED"],
        forbidden_substrings: &["NO_ROLLBACK"],
    },
    SkillCase {
        id: "skill-changelog-entry",
        skill_name: "changelog-entry",
        prompt: "Draft a user-facing release note for the skill telemetry addition.",
        full_skill_md: "---\nname: changelog-entry\ndescription: Write crisp changelog entries covering user impact, verification, and compatibility.\n---\n# Changelog Entry\nEnsure every entry contains these exact headings:\n- USER_IMPACT\n- VERIFICATION\n- BREAKING_NONE\nAvoid writing INTERNAL_JARGON.\n",
        trimmed_skill_md: "---\nname: changelog-entry\ndescription: Write crisp changelog entries covering user impact, verification, and compatibility.\n---\nInclude exact headings: USER_IMPACT, VERIFICATION, BREAKING_NONE. Never write INTERNAL_JARGON.\n",
        required_substrings: &["USER_IMPACT", "VERIFICATION", "BREAKING_NONE"],
        forbidden_substrings: &["INTERNAL_JARGON"],
    },
    SkillCase {
        id: "skill-incident-summary",
        skill_name: "incident-summary",
        prompt: "Write a post-incident summary for the cache invalidation delay.",
        full_skill_md: "---\nname: incident-summary\ndescription: Summarize incidents with UTC timeline, mitigation, and follow-up actions.\n---\n# Incident Summary\nInclude these exact structured sections:\n- TIMELINE_UTC\n- MITIGATION\n- ACTION_ITEMS\nNever include BLAME_USER.\n",
        trimmed_skill_md: "---\nname: incident-summary\ndescription: Summarize incidents with UTC timeline, mitigation, and follow-up actions.\n---\nInclude exact sections: TIMELINE_UTC, MITIGATION, ACTION_ITEMS. Never include BLAME_USER.\n",
        required_substrings: &["TIMELINE_UTC", "MITIGATION", "ACTION_ITEMS"],
        forbidden_substrings: &["BLAME_USER"],
    },
];

pub struct SkillAblationOpts {
    pub model: String,
    pub effort: Option<String>,
    pub k: u32,
    pub only: Option<String>,
    pub provider: Provider,
    pub ablate_skill: Option<String>,
    pub skill_override: Option<String>,
    pub call_timeout: Duration,
    pub retry: RetryPolicy,
    pub deadline: Option<Instant>,
}

impl SkillAblationOpts {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            effort: None,
            k: 1,
            only: None,
            provider: Provider::Grok,
            ablate_skill: None,
            skill_override: None,
            call_timeout: DEFAULT_CALL_TIMEOUT,
            retry: RetryPolicy::default(),
            deadline: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillAttempt {
    pub case_id: String,
    pub skill_name: String,
    pub arm: SkillArm,
    pub round: u32,
    pub pass: bool,
    pub turns: u32,
    pub tool_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_in_usd_ticks: i64,
    pub wall_ms: u128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ArmSummary {
    pub arm: String,
    pub tries: usize,
    pub passed: usize,
    pub pass_rate: f64,
    pub avg_turns: f64,
    pub avg_tool_calls: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub wall_ms: u128,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ArmDelta {
    pub pass_rate_delta: f64,
    pub avg_turns_delta: f64,
    pub avg_tool_calls_delta: f64,
    pub input_tokens_delta: i64,
    pub output_tokens_delta: i64,
    pub cost_usd_delta: f64,
    pub wall_ms_delta: i128,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NetValueSummary {
    pub verdict: String,
    pub pass_delta_pct: f64,
    pub token_overhead: i64,
    pub token_overhead_pct: f64,
    pub turn_overhead: f64,
    pub cost_delta_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trimmed_token_savings: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trimmed_pass_delta_pct: Option<f64>,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillAblationReport {
    pub suite: String,
    pub model: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_filter: Option<String>,
    pub k: u32,
    pub spent_ticks: i64,
    pub budget_ticks: i64,
    pub stopped_by_budget: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
    pub arms: BTreeMap<String, ArmSummary>,
    pub deltas: BTreeMap<String, ArmDelta>,
    pub net_value: NetValueSummary,
    pub attempts: Vec<SkillAttempt>,
}

struct NoHalt;
impl HaltCheck for NoHalt {
    fn halted(&self) -> bool {
        false
    }
}

#[derive(Clone)]
struct CaseSpec {
    id: String,
    skill_name: String,
    prompt: String,
    full_skill_md: Option<String>,
    trimmed_skill_md: Option<String>,
    required_substrings: &'static [&'static str],
    forbidden_substrings: &'static [&'static str],
    require_nonempty_reply: bool,
}

fn resolve_cases(opts: &SkillAblationOpts) -> Vec<CaseSpec> {
    let mut specs: Vec<CaseSpec> = FIXTURES
        .iter()
        .filter(|c| {
            opts.only
                .as_deref()
                .is_none_or(|only| c.id.contains(only) || c.skill_name.contains(only))
        })
        .filter(|c| {
            opts.ablate_skill
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none_or(|target| c.skill_name == target || c.id == target)
        })
        .map(|c| CaseSpec {
            id: c.id.to_string(),
            skill_name: c.skill_name.to_string(),
            prompt: c.prompt.to_string(),
            full_skill_md: Some(c.full_skill_md.to_string()),
            trimmed_skill_md: Some(
                opts.skill_override
                    .clone()
                    .unwrap_or_else(|| c.trimmed_skill_md.to_string()),
            ),
            required_substrings: c.required_substrings,
            forbidden_substrings: c.forbidden_substrings,
            require_nonempty_reply: true,
        })
        .collect();

    if !specs.is_empty() {
        return specs;
    }
    if let Some(target) = opts
        .ablate_skill
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        specs.push(CaseSpec {
            id: format!("skill-{target}"),
            skill_name: target.to_string(),
            prompt: format!(
                "Apply the `{target}` skill to complete a verification checklist and reply."
            ),
            full_skill_md: None,
            trimmed_skill_md: opts.skill_override.clone(),
            required_substrings: &[],
            forbidden_substrings: &[],
            require_nonempty_reply: true,
        });
    }
    specs
}

fn arms_for_case(spec: &CaseSpec) -> Vec<SkillArm> {
    if spec.trimmed_skill_md.is_some() {
        vec![SkillArm::Baseline, SkillArm::Full, SkillArm::Trimmed]
    } else {
        vec![SkillArm::Baseline, SkillArm::Full]
    }
}

fn grade_reply(spec: &CaseSpec, reply: &str) -> Result<(), String> {
    if spec.require_nonempty_reply && reply.trim().is_empty() {
        return Err("empty reply".to_string());
    }
    for req in spec.required_substrings {
        if !reply.contains(req) {
            return Err(format!("missing required marker `{req}`"));
        }
    }
    for bad in spec.forbidden_substrings {
        if reply.contains(bad) {
            return Err(format!("contained forbidden marker `{bad}`"));
        }
    }
    Ok(())
}

fn write_workspace_skill(ws: &Path, skill_name: &str, body: &str) -> std::io::Result<()> {
    let dir = ws.join(".grok").join("skills").join(skill_name);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("SKILL.md"), body)
}

fn run_one_arm(
    client: &Arc<dyn ModelClient + Send + Sync>,
    capped: &Arc<Capped>,
    opts: &SkillAblationOpts,
    spec: &CaseSpec,
    arm: SkillArm,
    round: u32,
) -> SkillAttempt {
    let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "gh-skill-eval-{}-{}-{}-{seq}",
        spec.skill_name,
        arm.label(),
        std::process::id(),
    ));
    let ws = dir.join("ws");
    let cfg = dir.join("cfg");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&ws);
    let _ = std::fs::create_dir_all(&cfg);
    let _cfg = ConfigGuard::set(cfg.clone());

    let mut error = None;
    match arm {
        SkillArm::Baseline => {
            if let Err(err) =
                crate::skills::set_skill_off_in(&cfg.join("skills"), &spec.skill_name, true)
            {
                error = Some(format!("could not disable baseline skill: {err}"));
            }
        }
        SkillArm::Full => {
            if let Some(body) = &spec.full_skill_md {
                if let Err(err) = write_workspace_skill(&ws, &spec.skill_name, body) {
                    error = Some(format!("could not write full skill: {err}"));
                }
            }
        }
        SkillArm::Trimmed => {
            if let Some(body) = &spec.trimmed_skill_md {
                if let Err(err) = write_workspace_skill(&ws, &spec.skill_name, body) {
                    error = Some(format!("could not write trimmed skill: {err}"));
                }
            }
        }
    }
    crate::skills::invalidate_discover_cache();

    let permits = Arc::new(ScriptedPermits::default());
    let conversation_id = format!(
        "skill-eval-{}-{}-{round}-{seq}",
        spec.skill_name,
        arm.label()
    );
    let effort = opts
        .effort
        .clone()
        .or_else(|| crate::route::live::start_effort(crate::route::live::DEFAULT_CLASS));
    let mut engine = NativeEngine::new(EngineParts {
        client: client.clone(),
        workspace: ws.clone(),
        model: opts.model.clone(),
        effort,
        system: app_system_at(&ws, &cfg, None),
        conversation_id: conversation_id.clone(),
        auth_kind: crate::AuthKind::ApiKey,
        max_turns: 0,
        cancel: CancelToken::new(),
        steer: SteerQueue::new(),
        halt: Box::new(NoHalt),
        gate: Gate {
            mode: PermMode::Auto,
            readonly_session: false,
            attended: true,
            desktop: false,
        },
        desktop: None,
        permits,
    });

    let (answers, inbox) = crate::mcp::ElicitInbox::pair();
    crate::mcp::attach_elicit(&conversation_id, inbox);
    let _ = capped.take_usage();
    let start = Instant::now();
    let mut reply = String::new();
    let mut trace = Trace::default();
    if error.is_none() {
        let res = engine.prompt(&spec.prompt, None, &mut |ev| match ev {
            AcpEvent::Text(chunk) => reply.push_str(&chunk),
            AcpEvent::Tool(card) => trace.note(&card),
            AcpEvent::Err(msg) if error.is_none() => error = Some(msg),
            AcpEvent::Elicit(ask) => {
                let id = match &ask.rpc_id {
                    serde_json::Value::String(id) => id.clone(),
                    other => other.to_string(),
                };
                let _ = answers.send(crate::mcp::ElicitNote {
                    id,
                    action: "accept".into(),
                    content: None,
                });
            }
            _ => {}
        });
        if let Err(err) = res {
            error = Some(err);
        } else if capped.out_of_budget() {
            error = Some("budget used up".into());
        }
    }
    crate::mcp::detach_elicit(&conversation_id);
    let wall_ms = start.elapsed().as_millis();
    let usage_log = capped.take_usage();
    let _ = std::fs::remove_dir_all(&dir);

    if error.is_none() {
        error = grade_reply(spec, &reply).err();
    }

    let pass = error.is_none();
    let output_tokens = usage_log
        .usage
        .output_tokens
        .saturating_add(usage_log.usage.reasoning_tokens);

    SkillAttempt {
        case_id: spec.id.clone(),
        skill_name: spec.skill_name.clone(),
        arm,
        round,
        pass,
        turns: usage_log.calls,
        tool_calls: trace.calls.len() as u32,
        input_tokens: usage_log.usage.input_tokens,
        output_tokens,
        cost_in_usd_ticks: usage_log.usage.cost_in_usd_ticks,
        wall_ms,
        error,
    }
}

pub fn run_skill_ablation(
    client: Arc<dyn ModelClient + Send + Sync>,
    opts: &SkillAblationOpts,
    budget_ticks: i64,
) -> SkillAblationReport {
    run_skill_ablation_with(client, opts, budget_ticks, &mut |_, _| {})
}

pub fn run_skill_ablation_with(
    client: Arc<dyn ModelClient + Send + Sync>,
    opts: &SkillAblationOpts,
    budget_ticks: i64,
    progress: &mut dyn FnMut(&SkillAblationReport, &SkillAttempt),
) -> SkillAblationReport {
    let _class = crate::route::live::ClassScope::enter(super::EVAL_CLASS);
    let cases = resolve_cases(opts);
    let k = opts.k.max(1);
    let limited: Arc<dyn ModelClient + Send + Sync> =
        Arc::new(CallDeadline::new(client, opts.call_timeout));
    let capped = Arc::new(Capped::for_provider(limited, opts.provider, budget_ticks));
    let retrying = Arc::new(Retrying::new(capped.clone(), opts.retry));
    let guarded: Arc<dyn ModelClient + Send + Sync> = retrying.clone();

    let mut attempts = Vec::new();
    let mut stopped_by_budget = false;
    let mut stopped = None;

    'rounds: for round in 0..k {
        for spec in &cases {
            for arm in arms_for_case(spec) {
                if capped.spent_ticks() >= budget_ticks {
                    stopped_by_budget = true;
                    break 'rounds;
                }
                if opts.deadline.is_some_and(|d| Instant::now() >= d) {
                    stopped = Some("the deadline came before every try was made".into());
                    break 'rounds;
                }
                let attempt = run_one_arm(&guarded, &capped, opts, spec, arm, round);
                attempts.push(attempt);
                stopped_by_budget = capped.out_of_budget();
                if stopped.is_none() {
                    stopped = retrying.stop_reason();
                }
                let snapshot = build_report(
                    opts,
                    capped.spent_ticks(),
                    budget_ticks,
                    stopped_by_budget,
                    stopped.clone(),
                    attempts.clone(),
                );
                if let Some(last) = snapshot.attempts.last() {
                    progress(&snapshot, last);
                }
                if stopped_by_budget || stopped.is_some() {
                    break 'rounds;
                }
            }
        }
    }

    build_report(
        opts,
        capped.spent_ticks(),
        budget_ticks,
        stopped_by_budget,
        stopped,
        attempts,
    )
}

fn build_report(
    opts: &SkillAblationOpts,
    spent_ticks: i64,
    budget_ticks: i64,
    stopped_by_budget: bool,
    stopped: Option<String>,
    attempts: Vec<SkillAttempt>,
) -> SkillAblationReport {
    let mut arms = BTreeMap::new();
    for arm in [SkillArm::Baseline, SkillArm::Full, SkillArm::Trimmed] {
        if attempts.iter().any(|a| a.arm == arm) {
            arms.insert(arm.label().to_string(), summarize_arm(arm, &attempts));
        }
    }

    let mut deltas = BTreeMap::new();
    let base = arms.get("baseline");
    let full = arms.get("full");
    let trimmed = arms.get("trimmed");

    if let (Some(base), Some(full)) = (base, full) {
        deltas.insert("full_vs_baseline".to_string(), compute_delta(full, base));
    }
    if let (Some(base), Some(trimmed)) = (base, trimmed) {
        deltas.insert(
            "trimmed_vs_baseline".to_string(),
            compute_delta(trimmed, base),
        );
    }
    if let (Some(full), Some(trimmed)) = (full, trimmed) {
        deltas.insert("trimmed_vs_full".to_string(), compute_delta(trimmed, full));
    }

    let net_value = match (base, full) {
        (Some(base), Some(full)) => compute_net_value(base, full, trimmed),
        _ => NetValueSummary {
            verdict: "neutral".to_string(),
            summary: "insufficient arms to compute skill ablation net value".to_string(),
            ..Default::default()
        },
    };

    SkillAblationReport {
        suite: "skills-v1".to_string(),
        model: opts.model.clone(),
        provider: opts.provider.label().to_string(),
        skill_filter: opts.ablate_skill.clone(),
        k: opts.k.max(1),
        spent_ticks,
        budget_ticks,
        stopped_by_budget,
        stopped,
        arms,
        deltas,
        net_value,
        attempts,
    }
}

/// Aggregate per-arm metrics across `attempts`.
pub fn summarize_arm(arm: SkillArm, attempts: &[SkillAttempt]) -> ArmSummary {
    let items: Vec<&SkillAttempt> = attempts.iter().filter(|a| a.arm == arm).collect();
    let tries = items.len();
    if tries == 0 {
        return ArmSummary {
            arm: arm.label().to_string(),
            ..Default::default()
        };
    }
    let passed = items.iter().filter(|a| a.pass).count();
    let total_turns: u64 = items.iter().map(|a| u64::from(a.turns)).sum();
    let total_tools: u64 = items.iter().map(|a| u64::from(a.tool_calls)).sum();
    let input_tokens: u64 = items.iter().map(|a| a.input_tokens).sum();
    let output_tokens: u64 = items.iter().map(|a| a.output_tokens).sum();
    let cost_ticks: i64 = items.iter().map(|a| a.cost_in_usd_ticks).sum();
    let wall_ms: u128 = items.iter().map(|a| a.wall_ms).sum();
    let denom = tries as f64;

    ArmSummary {
        arm: arm.label().to_string(),
        tries,
        passed,
        pass_rate: passed as f64 / denom,
        avg_turns: total_turns as f64 / denom,
        avg_tool_calls: total_tools as f64 / denom,
        input_tokens,
        output_tokens,
        cost_usd: cost_ticks as f64 / TICKS_PER_USD as f64,
        wall_ms,
    }
}

/// Compute signed delta (`arm - base`).
pub fn compute_delta(arm: &ArmSummary, base: &ArmSummary) -> ArmDelta {
    ArmDelta {
        pass_rate_delta: arm.pass_rate - base.pass_rate,
        avg_turns_delta: arm.avg_turns - base.avg_turns,
        avg_tool_calls_delta: arm.avg_tool_calls - base.avg_tool_calls,
        input_tokens_delta: arm.input_tokens as i64 - base.input_tokens as i64,
        output_tokens_delta: arm.output_tokens as i64 - base.output_tokens as i64,
        cost_usd_delta: arm.cost_usd - base.cost_usd,
        wall_ms_delta: arm.wall_ms as i128 - base.wall_ms as i128,
    }
}

/// Compare pass-rate lift against token, turn, and USD cost overhead.
pub fn compute_net_value(
    baseline: &ArmSummary,
    full: &ArmSummary,
    trimmed: Option<&ArmSummary>,
) -> NetValueSummary {
    let pass_delta_pct = (full.pass_rate - baseline.pass_rate) * 100.0;
    let base_tokens = baseline.input_tokens.saturating_add(baseline.output_tokens) as i64;
    let full_tokens = full.input_tokens.saturating_add(full.output_tokens) as i64;
    let token_overhead = full_tokens - base_tokens;
    let token_overhead_pct = if base_tokens > 0 {
        token_overhead as f64 * 100.0 / base_tokens as f64
    } else {
        0.0
    };
    let turn_overhead = full.avg_turns - baseline.avg_turns;
    let cost_delta_usd = full.cost_usd - baseline.cost_usd;

    let (trimmed_token_savings, trimmed_pass_delta_pct) = match trimmed {
        Some(trim) => {
            let trim_tokens = trim.input_tokens.saturating_add(trim.output_tokens) as i64;
            (
                Some(full_tokens - trim_tokens),
                Some((trim.pass_rate - baseline.pass_rate) * 100.0),
            )
        }
        None => (None, None),
    };

    let trimmed_matches_full = trimmed.is_some_and(|trim| {
        trim.pass_rate + 1e-9 >= full.pass_rate
            && trim.input_tokens.saturating_add(trim.output_tokens)
                < full.input_tokens.saturating_add(full.output_tokens)
            && trim.pass_rate > baseline.pass_rate
    });

    let verdict = if trimmed_matches_full {
        "trim_recommended"
    } else if full.pass_rate > baseline.pass_rate {
        "positive"
    } else if full.pass_rate < baseline.pass_rate || token_overhead > 0 || turn_overhead > 0.0 {
        "negative"
    } else {
        "neutral"
    };

    let summary = format!(
        "{verdict}: pass delta {pass_delta_pct:+.1} percentage points vs baseline, \
         token overhead {token_overhead:+} ({token_overhead_pct:+.1}%), \
         turn overhead {turn_overhead:+.2}, cost delta ${cost_delta_usd:+.6}",
    );

    NetValueSummary {
        verdict: verdict.to_string(),
        pass_delta_pct,
        token_overhead,
        token_overhead_pct,
        turn_overhead,
        cost_delta_usd,
        trimmed_token_savings,
        trimmed_pass_delta_pct,
        summary,
    }
}

pub fn results_json(report: &SkillAblationReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string()) + "\n"
}

pub fn write_results(report: &SkillAblationReport, out: &Path) -> std::io::Result<PathBuf> {
    let path = results_path(out);
    let body = results_json(report);
    write_atomic(&path, body.as_bytes())?;
    Ok(path)
}

pub fn render_report(report: &SkillAblationReport) -> String {
    let filter_note = report
        .skill_filter
        .as_deref()
        .map(|s| format!("  ·  skill: {s}"))
        .unwrap_or_default();
    let spent = report.spent_ticks as f64 / TICKS_PER_USD as f64;
    let budget = report.budget_ticks as f64 / TICKS_PER_USD as f64;
    let mut out = format!(
        "# Skill ablation eval\n\n\
         model: {}  ·  provider: {}{}  ·  k: {}  ·  spent ${:.4} of ${:.4}\n\n",
        report.model, report.provider, filter_note, report.k, spent, budget,
    );

    out.push_str("## Per-arm metrics\n\n");
    out.push_str(
        "| arm | tries | pass_rate | avg_turns | avg_tool_calls | \
         input_tokens | output_tokens | cost_usd | wall_ms |\n\
         |---|---:|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for arm_name in ["baseline", "full", "trimmed"] {
        let Some(arm) = report.arms.get(arm_name) else {
            continue;
        };
        out.push_str(&format!(
            "| {} | {} | {:.1}% ({}/{}) | {:.2} | {:.2} | {} | {} | ${:.6} | {} |\n",
            arm.arm,
            arm.tries,
            arm.pass_rate * 100.0,
            arm.passed,
            arm.tries,
            arm.avg_turns,
            arm.avg_tool_calls,
            arm.input_tokens,
            arm.output_tokens,
            arm.cost_usd,
            arm.wall_ms,
        ));
    }

    if !report.deltas.is_empty() {
        out.push_str("\n## Deltas\n\n");
        out.push_str(
            "| comparison | pass_rate | avg_turns | avg_tool_calls | \
             input_tokens | output_tokens | cost_usd | wall_ms |\n\
             |---|---:|---:|---:|---:|---:|---:|---:|\n",
        );
        for key in ["full_vs_baseline", "trimmed_vs_baseline", "trimmed_vs_full"] {
            let Some(d) = report.deltas.get(key) else {
                continue;
            };
            out.push_str(&format!(
                "| {} | {:+.1}% | {:+.2} | {:+.2} | {:+} | {:+} | ${:+.6} | {:+} |\n",
                key,
                d.pass_rate_delta * 100.0,
                d.avg_turns_delta,
                d.avg_tool_calls_delta,
                d.input_tokens_delta,
                d.output_tokens_delta,
                d.cost_usd_delta,
                d.wall_ms_delta,
            ));
        }
    }

    out.push_str("\n## Net Value\n\n");
    out.push_str(&format!(
        "- verdict: {}\n\
         - pass_delta_pct: {:+.1}%\n\
         - token_overhead: {:+} ({:+.1}%)\n\
         - turn_overhead: {:+.2}\n\
         - cost_usd delta: ${:+.6}\n",
        report.net_value.verdict,
        report.net_value.pass_delta_pct,
        report.net_value.token_overhead,
        report.net_value.token_overhead_pct,
        report.net_value.turn_overhead,
        report.net_value.cost_delta_usd,
    ));
    if let Some(savings) = report.net_value.trimmed_token_savings {
        out.push_str(&format!("- trimmed_token_savings: {savings:+}\n"));
    }
    if let Some(pass_delta) = report.net_value.trimmed_pass_delta_pct {
        out.push_str(&format!("- trimmed_pass_delta_pct: {pass_delta:+.1}%\n"));
    }
    out.push_str(&format!("- summary: {}\n", report.net_value.summary));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{
        ClientError, ContentPart, FunctionCall, InputItem, ResponsesRequest, StreamEvent,
        TurnOutput,
    };
    use crate::Usage;

    struct ScriptedSkillClient {
        negative_overhead: bool,
    }

    impl ScriptedSkillClient {
        fn last_skill_output(input: &[InputItem]) -> Option<String> {
            input.iter().rev().find_map(|item| match item {
                InputItem::FunctionCallOutput { output, .. } if output.contains("<skill ") => {
                    Some(output.clone())
                }
                _ => None,
            })
        }

        fn system_text(input: &[InputItem]) -> String {
            input
                .iter()
                .filter_map(|item| match item {
                    InputItem::Message { role, content }
                        if role == "system" || role == "developer" =>
                    {
                        let parts: Vec<&str> = content
                            .iter()
                            .filter_map(|p| match p {
                                ContentPart::InputText(t) => Some(t.as_str()),
                                ContentPart::InputImage(_) => None,
                            })
                            .collect();
                        Some(parts.join("\n"))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn prompt_text(input: &[InputItem]) -> String {
            input
                .iter()
                .find_map(|item| match item {
                    InputItem::Message { role, content } if role == "user" => {
                        content.iter().find_map(|p| match p {
                            ContentPart::InputText(t) => Some(t.clone()),
                            ContentPart::InputImage(_) => None,
                        })
                    }
                    _ => None,
                })
                .unwrap_or_default()
        }

        fn active_fixture(system: &str, prompt: &str) -> Option<&'static SkillCase> {
            FIXTURES
                .iter()
                .find(|fix| system.contains(fix.skill_name) || prompt.contains(fix.prompt))
        }
    }

    impl ModelClient for ScriptedSkillClient {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            let system = Self::system_text(&req.input);
            let prompt = Self::prompt_text(&req.input);
            let fix = Self::active_fixture(&system, &prompt);
            let skill_loaded = Self::last_skill_output(&req.input);

            if let Some(body) = skill_loaded {
                let is_trimmed = !body.contains("# ");
                let text = if let Some(case) = fix {
                    if self.negative_overhead {
                        "UNCHECKED TBD_SKIPPED missing markers".to_string()
                    } else {
                        case.required_substrings.join(" | ")
                    }
                } else {
                    "CUSTOM_SKILL_OK".to_string()
                };
                sink(StreamEvent::TextDelta(text.clone()));
                let (input_tokens, output_tokens, cost_in_usd_ticks) = if is_trimmed {
                    (120, 40, 200_000)
                } else {
                    (200, 60, 400_000)
                };
                return Ok(TurnOutput {
                    text,
                    usage: Usage {
                        input_tokens,
                        output_tokens,
                        cost_in_usd_ticks,
                        ..Default::default()
                    },
                    ..Default::default()
                });
            }

            if let Some(case) = fix {
                if system.contains(case.skill_name) {
                    let call_id = format!("call-{}", case.skill_name);
                    return Ok(TurnOutput {
                        calls: vec![FunctionCall {
                            call_id,
                            name: "skill".to_string(),
                            arguments: format!("{{\"name\":\"{}\"}}", case.skill_name),
                        }],
                        usage: Usage {
                            input_tokens: 150,
                            output_tokens: 30,
                            cost_in_usd_ticks: 100_000,
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
            }

            // Baseline arm (no skill present).
            let text = if self.negative_overhead {
                fix.map(|c| c.required_substrings.join(" "))
                    .unwrap_or_else(|| "baseline pass".to_string())
            } else {
                "baseline missing markers".to_string()
            };
            sink(StreamEvent::TextDelta(text.clone()));
            Ok(TurnOutput {
                text,
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 20,
                    cost_in_usd_ticks: 100_000,
                    ..Default::default()
                },
                ..Default::default()
            })
        }
    }

    #[test]
    fn scripted_skill_ablation_computes_exact_arm_deltas_and_net_value() {
        let client: Arc<dyn ModelClient + Send + Sync> = Arc::new(ScriptedSkillClient {
            negative_overhead: false,
        });
        let opts = SkillAblationOpts::new("grok-test");
        let report = run_skill_ablation(client, &opts, TICKS_PER_USD);

        assert_eq!(report.suite, "skills-v1");
        assert_eq!(report.attempts.len(), 18);

        let base = &report.arms["baseline"];
        let full = &report.arms["full"];
        let trimmed = &report.arms["trimmed"];

        assert_eq!((base.tries, base.passed, base.pass_rate), (6, 0, 0.0));
        assert_eq!((base.avg_turns, base.avg_tool_calls), (1.0, 0.0));
        assert_eq!((base.input_tokens, base.output_tokens), (600, 120));
        assert!((base.cost_usd - 0.00006).abs() < 1e-12);

        assert_eq!((full.tries, full.passed, full.pass_rate), (6, 6, 1.0));
        assert_eq!((full.avg_turns, full.avg_tool_calls), (2.0, 1.0));
        assert_eq!((full.input_tokens, full.output_tokens), (2_100, 540));
        assert!((full.cost_usd - 0.00030).abs() < 1e-12);

        assert_eq!(
            (trimmed.tries, trimmed.passed, trimmed.pass_rate),
            (6, 6, 1.0)
        );
        assert_eq!((trimmed.avg_turns, trimmed.avg_tool_calls), (2.0, 1.0));
        assert_eq!((trimmed.input_tokens, trimmed.output_tokens), (1_620, 420));
        assert!((trimmed.cost_usd - 0.00018).abs() < 1e-12);

        let d_full = &report.deltas["full_vs_baseline"];
        assert_eq!(d_full.pass_rate_delta, 1.0);
        assert_eq!(d_full.avg_turns_delta, 1.0);
        assert_eq!(d_full.avg_tool_calls_delta, 1.0);
        assert_eq!(d_full.input_tokens_delta, 1_500);
        assert_eq!(d_full.output_tokens_delta, 420);
        assert!((d_full.cost_usd_delta - 0.00024).abs() < 1e-12);

        let d_trim_vs_full = &report.deltas["trimmed_vs_full"];
        assert_eq!(d_trim_vs_full.pass_rate_delta, 0.0);
        assert_eq!(d_trim_vs_full.input_tokens_delta, -480);
        assert_eq!(d_trim_vs_full.output_tokens_delta, -120);
        assert!((d_trim_vs_full.cost_usd_delta + 0.00012).abs() < 1e-12);

        assert_eq!(report.net_value.verdict, "trim_recommended");
        assert_eq!(report.net_value.pass_delta_pct, 100.0);
        assert_eq!(report.net_value.token_overhead, 1_920);
        assert_eq!(report.net_value.turn_overhead, 1.0);
        assert_eq!(report.net_value.trimmed_token_savings, Some(600));
        assert_eq!(report.net_value.trimmed_pass_delta_pct, Some(100.0));

        let dir = std::env::temp_dir().join(format!("gh-skill-json-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let json_path = write_results(&report, &dir.join("skills.md")).unwrap();
        let raw = std::fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["suite"], "skills-v1");
        assert_eq!(parsed["arms"]["full"]["pass_rate"], 1.0);
        assert_eq!(parsed["arms"]["trimmed"]["input_tokens"], 1_620);
        assert_eq!(
            parsed["deltas"]["full_vs_baseline"]["input_tokens_delta"],
            1_500
        );
        assert_eq!(parsed["net_value"]["verdict"], "trim_recommended");
        assert!(parsed["arms"]["full"]["avg_turns"].is_number());
        assert!(parsed["arms"]["full"]["avg_tool_calls"].is_number());
        assert!(parsed["arms"]["full"]["cost_usd"].is_number());
        assert!(parsed["arms"]["full"]["wall_ms"].is_number());

        let md = render_report(&report);
        assert!(
            md.contains("| baseline | 6 | 0.0% (0/6) | 1.00 | 0.00 | 600 | 120 |"),
            "{md}"
        );
        assert!(
            md.contains("| full | 6 | 100.0% (6/6) | 2.00 | 1.00 | 2100 | 540 |"),
            "{md}"
        );
        assert!(
            md.contains("| trimmed | 6 | 100.0% (6/6) | 2.00 | 1.00 | 1620 | 420 |"),
            "{md}"
        );
        assert!(
            md.contains("| full_vs_baseline | +100.0% | +1.00 | +1.00 | +1500 | +420 |"),
            "{md}"
        );
        assert!(md.contains("## Net Value"), "{md}");
        assert!(md.contains("verdict: trim_recommended"), "{md}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn negative_net_value_when_skill_adds_overhead_and_regresses() {
        let client: Arc<dyn ModelClient + Send + Sync> = Arc::new(ScriptedSkillClient {
            negative_overhead: true,
        });
        let mut opts = SkillAblationOpts::new("grok-test");
        opts.ablate_skill = Some("checklist-release".into());
        let report = run_skill_ablation(client, &opts, TICKS_PER_USD);

        assert_eq!(report.attempts.len(), 3);
        assert_eq!(report.arms["baseline"].pass_rate, 1.0);
        assert_eq!(report.arms["full"].pass_rate, 0.0);
        assert_eq!(report.net_value.verdict, "negative");
        assert_eq!(report.net_value.pass_delta_pct, -100.0);
        assert_eq!(report.net_value.token_overhead, 320);
        assert_eq!(report.net_value.turn_overhead, 1.0);
    }

    #[test]
    fn skill_override_replaces_trimmed_variant_on_disk() {
        struct CaptureSkillBody;
        impl ModelClient for CaptureSkillBody {
            fn stream(
                &self,
                req: &ResponsesRequest,
                _cancel: &CancelToken,
                sink: &mut dyn FnMut(StreamEvent),
            ) -> Result<TurnOutput, ClientError> {
                if let Some(body) = ScriptedSkillClient::last_skill_output(&req.input) {
                    let marker = if body.contains("OVERRIDE_VARIANT_ACTIVE") {
                        "RELEASE_VERDICT ROLLBACK_PLAN SHA256_VERIFIED"
                    } else {
                        "missing"
                    };
                    sink(StreamEvent::TextDelta(marker.into()));
                    return Ok(TurnOutput {
                        text: marker.into(),
                        usage: Usage {
                            input_tokens: 50,
                            output_tokens: 10,
                            cost_in_usd_ticks: 10_000,
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
                let system = ScriptedSkillClient::system_text(&req.input);
                if system.contains("checklist-release") {
                    return Ok(TurnOutput {
                        calls: vec![FunctionCall {
                            call_id: "c1".into(),
                            name: "skill".into(),
                            arguments: r#"{"name":"checklist-release"}"#.into(),
                        }],
                        usage: Usage {
                            input_tokens: 40,
                            output_tokens: 10,
                            cost_in_usd_ticks: 10_000,
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
                sink(StreamEvent::TextDelta("no skill".into()));
                Ok(TurnOutput {
                    text: "no skill".into(),
                    ..Default::default()
                })
            }
        }

        let mut opts = SkillAblationOpts::new("grok-test");
        opts.ablate_skill = Some("checklist-release".into());
        let override_md = "---\nname: checklist-release\n\
                           description: Custom override\n---\n\
                           OVERRIDE_VARIANT_ACTIVE\n";
        opts.skill_override = Some(override_md.into());
        let report = run_skill_ablation(Arc::new(CaptureSkillBody), &opts, TICKS_PER_USD);
        assert_eq!(report.arms["baseline"].pass_rate, 0.0);
        assert_eq!(report.arms["full"].pass_rate, 0.0);
        assert_eq!(report.arms["trimmed"].pass_rate, 1.0);
    }
}
