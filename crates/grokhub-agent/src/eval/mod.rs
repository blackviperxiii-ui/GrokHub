//! Phase 16 eval harness. Dry-run is the default and uses scripted model clients
//! plus the fake CLI agent. `--live` is opt-in: it needs `GROKHUB_EVAL_API_KEY`
//! and `--budget-usd` (at most $0.20), stays off default PR CI, and runs a
//! budgeted live probe under [`live::Capped`].

use grokhub_core::wire::AcpEvent;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::client::{
    ClientError, ContentPart, FunctionCall, InputItem, ResponsesRequest, StreamEvent, TurnOutput,
};
use crate::events::Engine;
use crate::gate::{ClosedPermits, Gate, PermMode};
use crate::perm::ConfigGuard;
use crate::tools::ImagineApi;
use crate::{
    AuthKind, CancelToken, EngineParts, HaltCheck, ModelClient, NativeEngine, SteerQueue, Usage,
    XaiClient, DEFAULT_MODEL,
};

#[cfg(unix)]
mod scan;

pub mod ab;
pub mod care_first;
pub mod close_check;
pub mod coding;
pub mod efficiency;
pub mod live;
pub mod live_guard;
pub mod live_shape;
pub mod longmem;
pub mod memory_index;
pub mod metrics;
pub mod naturalness;
pub mod paired;
pub mod recall_set;
pub mod skills;
pub mod sycophancy;
pub mod turn_endings;

pub use ab::{load_tasks, run_technique_matrix, Technique, TechniqueArm, TechniqueMatrixReport};
pub use metrics::Recommendation;

pub const SUITE_ITEMS: [&str; 10] = [
    "xvfb-desktop",
    "repo-bugfix",
    "ask-refusal",
    "background-halt",
    "mcp-tool",
    "compaction",
    "tool-output-compact",
    "imagine",
    "system-scan",
    "close-check",
];

const SKIP_XVFB: &str = "skipped: no Xvfb";
const NO_FAKE_MCP: &str = "not verified: fake_mcp binary not found";
const IMAGINE_PROMPT: &str = "a red square";
const LIVE_KEY: &str = "GROKHUB_EVAL_API_KEY";
const ERR_NO_KEY: &str = "refusing --live: GROKHUB_EVAL_API_KEY is not set";
const ERR_NO_BUDGET: &str =
    "refusing --live: --budget-usd is required and must satisfy 0 < amount <= 0.20";
const ERR_BAD_BUDGET: &str = "refusing --live: --budget-usd must satisfy 0 < amount <= 0.20";
/// Hard dollar cap for the parity harness `--live` path (whole cents).
pub const LIVE_BUDGET_CAP_CENTS: u32 = 20;
/// Live probe prompt: a short reply keeps spend under the cap.
const LIVE_REPLY_PROMPT: &str = "Reply with exactly the word pong and nothing else.";

static IMAGINE_SENDS: AtomicU64 = AtomicU64::new(0);
static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);
static SUITE: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub struct Opts {
    pub live: bool,
    /// Whole cents. `20` is the $0.20 cap. Empty unless `--budget-usd` parsed.
    pub budget_cents: Option<u32>,
    pub out: Option<PathBuf>,
}

#[derive(Debug)]
pub struct ItemResult {
    pub item: &'static str,
    pub engine: &'static str,
    pub success: bool,
    pub turns: u32,
    /// Measured for the example's stderr line. The dry-run report writes `dry-run`.
    pub wall_ms: u128,
    pub status: &'static str,
    pub verified: bool,
}

/// Spend recorded by a live suite under the dollar/token budget cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveSpend {
    pub spent_ticks: i64,
    pub budget_ticks: i64,
}

/// Result of an opt-in `--live` run.
#[derive(Debug)]
pub struct LiveSuite {
    pub items: Vec<ItemResult>,
    pub spend: LiveSpend,
}

/// Sends recorded by the dry-run Imagine path. Stays zero because that path
/// only builds the request body.
pub fn imagine_send_count() -> u64 {
    IMAGINE_SENDS.load(Ordering::SeqCst)
}

/// Parse harness flags. `--live` is gated here (key + budget) before a client
/// is built; dry-run stays the default.
pub fn parse_args(args: &[String]) -> Result<Opts, String> {
    let key = std::env::var(LIVE_KEY).ok();
    parse_args_with_key(args, key.as_deref())
}

/// `parse_args` with the key value passed in, so tests never touch the
/// process environment.
fn parse_args_with_key(args: &[String], key: Option<&str>) -> Result<Opts, String> {
    let mut live = false;
    let mut dry = false;
    let mut budget: Option<Result<u32, String>> = None;
    let mut out = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--live" {
            live = true;
        } else if arg == "--dry-run" {
            dry = true;
        } else if arg == "--help" || arg == "-h" {
            return Err(help_text());
        } else if arg == "--budget-usd" || arg == "--out" {
            let Some(value) = args.get(i + 1) else {
                return Err(missing_value(arg, live));
            };
            if value.starts_with("--") {
                return Err(missing_value(arg, live));
            }
            i += 1;
            match arg.as_str() {
                "--budget-usd" => budget = Some(parse_budget_token(value)),
                "--out" => out = Some(PathBuf::from(value)),
                _ => {}
            }
        } else if let Some(value) = arg.strip_prefix("--budget-usd=") {
            budget = Some(parse_budget_token(value));
        } else if let Some(value) = arg.strip_prefix("--out=") {
            out = Some(PathBuf::from(value));
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
        i += 1;
    }
    if live && dry {
        return Err("refusing --live: do not pass --dry-run with --live".into());
    }
    if live {
        if !key.is_some_and(|value| !value.trim().is_empty()) {
            return Err(ERR_NO_KEY.into());
        }
        match budget {
            None => return Err(ERR_NO_BUDGET.into()),
            Some(Err(err)) => return Err(err),
            Some(Ok(_)) => {}
        }
    } else if let Some(Err(err)) = budget {
        return Err(err);
    }
    Ok(Opts {
        live,
        budget_cents: budget.and_then(|item| item.ok()),
        out,
    })
}

/// Live is opt-in. `parse_args` already required the key and budget; this
/// stays so callers can refuse a dry-run that accidentally set `--live` after
/// parse, and so dry-run callers keep a single gate. Always `Ok` once opts
/// are valid.
pub fn reject_live(opts: &Opts) -> Result<(), String> {
    let _ = opts;
    Ok(())
}

/// Whole cents → xAI cost ticks for the parity live cap.
pub fn live_budget_ticks(budget_cents: u32) -> i64 {
    let cents = budget_cents.min(LIVE_BUDGET_CAP_CENTS) as i64;
    cents.saturating_mul(live::TICKS_PER_USD / 100)
}

/// The route class eval items log (not in the §14.3 table, so the shadow keeps today's effort).
pub const EVAL_CLASS: &str = "eval:item";

pub fn run_suite(opts: &Opts) -> Vec<ItemResult> {
    if opts.live {
        return run_live_suite(opts).items;
    }
    let _lock = SUITE.lock().unwrap_or_else(|err| err.into_inner());
    let cfg = scratch("cfg");
    let _guard = cfg.as_ref().map(ConfigGuard::set);
    // Router R0: eval runs log their shadow routes under their own class.
    let _class = crate::route::live::ClassScope::enter(EVAL_CLASS);
    let mut items = Vec::with_capacity(SUITE_ITEMS.len());
    items.push(measure("xvfb-desktop", "native", desktop_item));
    items.push(measure("repo-bugfix", "native", repo_bugfix_native));
    items.push(measure("ask-refusal", "native", ask_native));
    items.push(measure("background-halt", "native", background_halt_native));
    items.push(measure("mcp-tool", "native", || mcp_native(cfg.as_deref())));
    items.push(measure("compaction", "native", compact_native));
    items.push(measure(
        "tool-output-compact",
        "native",
        tool_output_compact_native,
    ));
    items.push(measure("imagine", "native", imagine_native));
    items.push(measure("system-scan", "native", system_scan_native));
    items.push(measure("close-check", "native", close_check::run_fixtures));
    if let Ok(report) = ab::run_ab(ab::DEFAULT_MODEL, &ab::CandidateSuffix) {
        let _ = &report.comparison;
        let _ = report.tasks.len();
        let _ = report.report_markdown.len();
        let _ = report.gate();
    }
    if let Ok(care) = ab::run_care_first_ab() {
        let _ = care_first::status_line(&care);
        let _ = ab::care_first_default_on(false, care.cost_ratio, care.p95_ratio, care.words_ratio);
        let _ = ab::CARE_FIRST_SHIPS_ON;
    }
    cleanup_sessions();
    if let Some(dir) = cfg {
        let _ = std::fs::remove_dir_all(dir);
    }
    items
}

/// Opt-in live path: builds an [`XaiClient`] from `GROKHUB_EVAL_API_KEY` and
/// runs under the dollar/token budget cap. Never called from default PR CI.
pub fn run_live_suite(opts: &Opts) -> LiveSuite {
    let key = std::env::var(LIVE_KEY).unwrap_or_default();
    let budget_ticks = live_budget_ticks(opts.budget_cents.unwrap_or(0));
    let client = Arc::new(XaiClient::new(
        key.trim(),
        AuthKind::ApiKey,
        Duration::from_secs(120),
    ));
    run_live_suite_with(client, budget_ticks)
}

/// Live suite with an injected client so offline tests never dial the network.
pub fn run_live_suite_with(
    client: Arc<dyn ModelClient + Send + Sync>,
    budget_ticks: i64,
) -> LiveSuite {
    let _lock = SUITE.lock().unwrap_or_else(|err| err.into_inner());
    let cfg = scratch("live-cfg");
    let _guard = cfg.as_ref().map(ConfigGuard::set);
    let _class = crate::route::live::ClassScope::enter(EVAL_CLASS);
    let capped = Arc::new(live::Capped::for_provider(
        client,
        live::Provider::Grok,
        budget_ticks,
    ));
    let mut items = Vec::with_capacity(SUITE_ITEMS.len());
    items.push(measure("xvfb-desktop", "native", desktop_item));
    items.push(live_reply_item(Arc::clone(&capped), budget_ticks));
    items.push(skipped_live("ask-refusal"));
    items.push(skipped_live("background-halt"));
    items.push(skipped_live("mcp-tool"));
    items.push(skipped_live("compaction"));
    items.push(skipped_live("tool-output-compact"));
    items.push(measure("imagine", "native", imagine_native));
    items.push(measure("system-scan", "native", system_scan_native));
    items.push(measure("close-check", "native", close_check::run_fixtures));
    let spent_ticks = capped.spent_ticks();
    let spend = LiveSpend {
        spent_ticks,
        budget_ticks,
    };
    cleanup_sessions();
    if let Some(dir) = cfg {
        let _ = std::fs::remove_dir_all(dir);
    }
    LiveSuite { items, spend }
}

fn skipped_live(item: &'static str) -> ItemResult {
    ItemResult {
        item,
        engine: "native",
        success: true,
        turns: 0,
        wall_ms: 0,
        status: "skipped: scripted dry-run only",
        verified: false,
    }
}

fn live_reply_item(capped: Arc<live::Capped>, budget_ticks: i64) -> ItemResult {
    let start = Instant::now();
    if capped.out_of_budget() || budget_ticks <= 0 {
        return ItemResult {
            item: "repo-bugfix",
            engine: "native",
            success: false,
            turns: 0,
            wall_ms: start.elapsed().as_millis(),
            status: "skipped: budget",
            verified: false,
        };
    }
    let Some(dir) = scratch("live-reply") else {
        return ItemResult {
            item: "repo-bugfix",
            engine: "native",
            success: false,
            turns: 0,
            wall_ms: start.elapsed().as_millis(),
            status: "failed: scratch",
            verified: false,
        };
    };
    let _guard = ConfigGuard::set(dir.join("cfg"));
    let _ = std::fs::create_dir_all(dir.join("cfg"));
    let shared: Arc<dyn ModelClient + Send + Sync> = capped.clone();
    let mut engine = NativeEngine::new(EngineParts {
        client: shared,
        workspace: dir.clone(),
        model: DEFAULT_MODEL.into(),
        effort: Some("low".into()),
        system: String::new(),
        conversation_id: "eval-live-reply".into(),
        auth_kind: AuthKind::ApiKey,
        max_turns: 4,
        cancel: CancelToken::new(),
        steer: SteerQueue::new(),
        halt: Box::new(NoHalt),
        gate: Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: false,
            desktop: false,
        },
        desktop: None,
        permits: Arc::new(ClosedPermits),
    });
    let mut reply = String::new();
    let mut stop = String::new();
    let _ = engine.prompt(LIVE_REPLY_PROMPT, None, &mut |ev| match ev {
        AcpEvent::Text(chunk) => reply.push_str(&chunk),
        AcpEvent::Done { stop_reason } => stop = stop_reason,
        AcpEvent::Err(msg) if stop.is_empty() => stop = format!("error:{msg}"),
        _ => {}
    });
    crate::tasks::forget_session("eval-live-reply");
    let spent = capped.spent_ticks();
    let over = spent > budget_ticks;
    let pong = reply.to_ascii_lowercase().contains("pong");
    let ended = stop == "end_turn" && !reply.trim().is_empty();
    let _ = std::fs::remove_dir_all(&dir);
    let status = if over {
        "failed: over budget"
    } else if pong || ended {
        "live ok"
    } else if capped.out_of_budget() || stop.contains("cancel") {
        "skipped: budget"
    } else {
        "failed: live"
    };
    ItemResult {
        item: "repo-bugfix",
        engine: "native",
        success: status == "live ok",
        turns: 1,
        wall_ms: start.elapsed().as_millis(),
        status,
        verified: status == "live ok",
    }
}

/// Render the R6 technique matrix as deterministic markdown (no wall times).
pub fn render_markdown(report: &TechniqueMatrixReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# R6 technique A/B eval\n\nModel: {} (fixed; offline scripted replay)\n",
        report.model
    );
    out.push_str("## Method\n\n");
    let tasks = report.arms.first().map_or(0, |arm| arm.metrics.tasks);
    let _ = writeln!(
        out,
        "A = every optional cost technique off. Each B arm enables exactly one technique via a temp \
config dir (`tool_output_compact.toml`, `deferred_tools.toml`, `batched_tools.toml`, \
`prompt_diet.toml`), then an all-on arm of the first three. The stable prompt prefix is always on, in every arm. Same {tasks} \
fixture tasks and scripted model replies in every arm. No network, no paid runs.\n"
    );
    out.push_str(
        "Input usage is measured, not scripted: the replay captures every outbound request \
(system messages, then the tools array, then the other input items, as the wire body sends \
them) and estimates tokens as bytes / 4. The part of a request that repeats the start of \
the previous request in the same task counts as cached, the way a provider prefix cache \
would hit; the rest is billable. Output and reasoning tokens still come from the fixtures. \
Cost uses the offline tick model in `metrics::offline_cost_ticks` (1 tick / billable input \
+ 4 / output + 4 / reasoning). The scratch workspace path is replaced, terminal output (sh or \
PowerShell, so it differs by platform) counts as a fixed placeholder, and wall time is \
omitted, so the report is byte-stable.\n\n",
    );
    out.push_str(
        "Columns: **Prefix-stable** = bytes shared with the previous request / bytes of every \
request after a task's first. **Schema B/req** = tools-array bytes per model call. \
**Calls/task** = model calls per task. **Tools/call** = tool calls per model call.\n\n",
    );
    out.push_str(
        "Recommendation rule: **adopt** only if cost per solved task falls and success \
delta is within ±1 task (noise); **reject** if success leaves noise or cost rises; \
otherwise **spike** (success within noise, cost flat — needs richer/live measurement \
before flipping any default).\n\n",
    );
    out.push_str("## Arms\n\n");
    out.push_str(
        "| Arm | Success | Cost/solved | Δ cost/solved | In tokens | Cached | Prefix-stable | Schema B/req | Calls/task | Tools/call | Noise | Rec |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    let baseline = report
        .arms
        .iter()
        .find(|arm| arm.technique == Technique::Baseline)
        .map(|arm| &arm.metrics);
    for arm in &report.arms {
        let m = &arm.metrics;
        let is_base = arm.technique == Technique::Baseline;
        let dash = |v: String| if is_base { "—".to_string() } else { v };
        let noise = if arm.success_within_noise {
            "yes"
        } else {
            "no"
        };
        let _ = writeln!(
            out,
            "| {} | {:.2}% ({}/{}) | {} | {} | {} | {} | {:.2}% | {:.0} | {:.2} | {:.2} | {} | {} |",
            arm.technique.label(),
            m.success_rate() * 100.0,
            m.successes,
            m.tasks,
            format_cost(m.cost_per_solved_task()),
            dash(cost_delta_with_pct(arm.delta.cost_per_solved_task, baseline)),
            m.input_tokens,
            m.cached_tokens,
            m.prefix_stable_ratio() * 100.0,
            m.schema_bytes_per_request(),
            m.model_calls_per_task(),
            m.tool_calls_per_model_call(),
            dash(noise.into()),
            dash(arm.recommendation.as_str().into()),
        );
    }
    out.push_str("\n## Per-technique notes\n\n");
    for arm in &report.arms {
        let Some(base) = baseline else { break };
        let _ = writeln!(out, "- {}", technique_note(arm, base));
    }
    out.push_str("\n## Verdict\n\n");
    out.push_str(
        "Recommendations above follow the rule mechanically from the measured requests. \
They price prompt bytes only: the replay cannot see output quality, real tokenizer \
counts, provider cache pricing (cached input is billed at 0 here), or wall time. \
No default is flipped by this report; the user decides which settings to turn on.\n",
    );
    out
}

/// One line per arm: what moved against the baseline, and why when it did not.
fn technique_note(arm: &ab::TechniqueArm, base: &metrics::ArmMetrics) -> String {
    let m = &arm.metrics;
    let label = arm.technique.label();
    if arm.technique == Technique::Baseline {
        return format!(
            "**{label}**: {}/{} success, {} model calls, {} input tokens of which {} cached \
({:.2}% prefix-stable), {:.0} schema bytes per request, cost/solved {}.",
            m.successes,
            m.tasks,
            m.model_calls,
            m.input_tokens,
            m.cached_tokens,
            m.prefix_stable_ratio() * 100.0,
            m.schema_bytes_per_request(),
            format_cost(m.cost_per_solved_task())
        );
    }
    let mut moved = Vec::new();
    if m.input_tokens != base.input_tokens {
        moved.push(format!(
            "input tokens {} → {}",
            base.input_tokens, m.input_tokens
        ));
    }
    if m.prefix_stable_bytes != base.prefix_stable_bytes || m.followup_bytes != base.followup_bytes
    {
        moved.push(format!(
            "prefix-stable {:.2}% → {:.2}%",
            base.prefix_stable_ratio() * 100.0,
            m.prefix_stable_ratio() * 100.0
        ));
    }
    if m.schema_bytes != base.schema_bytes {
        moved.push(format!(
            "schema bytes/request {:.0} → {:.0}",
            base.schema_bytes_per_request(),
            m.schema_bytes_per_request()
        ));
    }
    if m.model_calls != base.model_calls {
        moved.push(format!(
            "model calls {} → {}",
            base.model_calls, m.model_calls
        ));
    }
    let moved = if moved.is_empty() {
        format!(
            "no request byte, call or schema change on this fixture set. {}",
            flat_reason(arm.technique)
        )
    } else {
        moved.join("; ")
    };
    format!(
        "**{label}**: **{}** (Δ cost/solved {}). {moved}",
        arm.recommendation.as_str(),
        cost_delta_with_pct(arm.delta.cost_per_solved_task, Some(base))
    )
}

/// Why a technique can leave the offline requests unchanged.
fn flat_reason(technique: Technique) -> &'static str {
    match technique {
        Technique::ToolOutputSummaries => {
            "Fixture tool outputs are a few bytes, below the size that triggers a summary."
        }
        Technique::BatchedReadOnly => {
            "R5 runs consecutive read-only calls from one reply in parallel; the model still \
decides how many calls a reply holds, so requests and model calls stay the same and only \
wall time (not measured here) can move."
        }
        _ => "",
    }
}

fn cost_delta_with_pct(delta: f64, baseline: Option<&metrics::ArmMetrics>) -> String {
    let base = baseline.map_or(f64::NAN, |b| b.cost_per_solved_task());
    let abs = format_cost_delta(delta);
    if !delta.is_finite() || !base.is_finite() || base == 0.0 || delta.abs() < 1e-12 {
        return abs;
    }
    format!("{abs} ({:+.2}%)", delta / base * 100.0)
}

fn format_cost(v: f64) -> String {
    if !v.is_finite() {
        "inf".into()
    } else if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
    }
}

fn format_cost_delta(v: f64) -> String {
    if !v.is_finite() {
        "inf".into()
    } else if v.abs() < 1e-12 {
        "0".into()
    } else if (v - v.round()).abs() < 1e-9 {
        format!("{:+}", v.round() as i64)
    } else {
        format!("{v:+.2}")
    }
}

pub fn render_report(items: &[ItemResult]) -> String {
    render_report_with(items, None)
}

/// Dry-run report, or a live report when `spend` is set.
pub fn render_report_with(items: &[ItemResult], spend: Option<LiveSpend>) -> String {
    let live = spend.is_some();
    let mut out = String::new();
    out.push_str("# Native parity v1\n\n");
    out.push_str("## Method\n\n");
    if live {
        out.push_str(
            "Phase 16 `--live` run against the native engine under a hard dollar/token \
budget cap. Offline dry-run is the default; this live path is opt-in and never \
runs on default PR CI.\n\n",
        );
        if let Some(spend) = spend {
            out.push_str(&format!(
                "Budget: {} ticks ({:.2} USD cap). Spent: {} ticks.\n\n",
                spend.budget_ticks,
                spend.budget_ticks as f64 / live::TICKS_PER_USD as f64,
                spend.spent_ticks,
            ));
        }
        out.push_str(
            "`--live` requires `GROKHUB_EVAL_API_KEY` and `--budget-usd` with \
`0 < amount <= 0.20`. ",
        );
        out.push_str(&credential_boundary());
        out.push_str(
            " Model calls go through `live::Capped`, which refuses a request the \
rest of the budget cannot cover.\n\n",
        );
        out.push_str(
            "Suite: an Xvfb desktop probe, one budgeted live reply probe (listed as \
repo-bugfix), scripted dry-run-only rows skipped, Imagine request build only, \
system-scan, and close-check.\n\n",
        );
    } else {
        out.push_str(
            "Phase 16 runs one fixed suite against the native engine. \
Dry-run is the default (`--dry-run`, and the mode when no mode flag is given). \
The native engine uses scripted fake model clients. \
Dry-run uses no network and no credentials.\n\n",
        );
        out.push_str(
            "Wall time is measured for each item and printed on the example's stderr. \
This file records wall time as `dry-run`, and pool % and cost as `0`, so two \
dry-runs are byte-identical.\n\n",
        );
        out.push_str(
            "`--live` is opt-in: it needs `GROKHUB_EVAL_API_KEY` and `--budget-usd` \
with `0 < amount <= 0.20`, stays off default PR CI, and was not run for this report. ",
        );
        out.push_str(&credential_boundary());
        out.push_str("\n\n");
        out.push_str(
            "Suite items: an Xvfb desktop probe, a temp repo bug fix with a test, an Ask-mode \
refusal, background plus Halt, an MCP tool call through a fake stdio server, compaction \
of a long transcript, large tool-output compaction with a spooled re-read path, an \
Imagine call that only builds the request, and a close-check that skips a single \
reply and runs once when an intention is still open.\n\n",
        );
    }
    out.push_str("This report does not switch the default engine.\n\n");
    out.push_str("## Results\n\n");
    out.push_str("| Item | Engine | Success | Turns | Wall time | Pool % | Cost | Status |\n");
    out.push_str("|---|---|---|---|---|---|---|---|\n");
    for item in items {
        let success = if item.success { "yes" } else { "no" };
        let wall = if live {
            format!("{}ms", item.wall_ms)
        } else {
            "dry-run".into()
        };
        out.push_str(&format!(
            "| {} | {} | {success} | {} | {wall} | 0 | 0 | {} |\n",
            item.item, item.engine, item.turns, item.status
        ));
    }
    out.push_str("\n## GAPS\n\n");
    let gaps = if live {
        live_gap_lines(items, spend)
    } else {
        gap_lines(items)
    };
    if gaps.is_empty() {
        out.push_str("- none\n");
    } else {
        for line in gaps {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push_str("\nNo default was switched.\n");
    out
}

/// Sentence that names the credential files this harness does not open.
pub fn credential_boundary() -> String {
    format!(
        "The key is read only from GROKHUB_EVAL_API_KEY. This harness does not read {}, {}, {}, or a keychain, and it does not read the app's stored credentials.",
        concat!("~/.grok/", "auth", ".json"),
        concat!("config", ".toml"),
        concat!("secrets", ".json"),
    )
}

fn help_text() -> String {
    "eval [--dry-run] [--live --budget-usd AMOUNT] [--out PATH]\n\
dry-run is the default and uses no network\n\
--live is opt-in (never on default PR CI): requires GROKHUB_EVAL_API_KEY and 0 < --budget-usd <= 0.20"
        .into()
}

fn missing_value(flag: &str, live: bool) -> String {
    if flag == "--budget-usd" && live {
        ERR_NO_BUDGET.into()
    } else {
        format!("missing value for {flag}")
    }
}

fn parse_budget_token(raw: &str) -> Result<u32, String> {
    match cents_of(raw) {
        Ok(cents) if cents > 0 && cents <= 20 => Ok(cents),
        Ok(cents) if cents > 20 => Err(format!(
            "refusing --live: --budget-usd {raw} is above the 0.20 cap"
        )),
        _ => Err(ERR_BAD_BUDGET.into()),
    }
}

fn cents_of(raw: &str) -> Result<u32, ()> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('+') {
        return Err(());
    }
    if raw.starts_with('-') {
        return Err(());
    }
    if !raw
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return Err(());
    }
    let mut parts = raw.split('.');
    let whole = parts.next().unwrap_or("");
    let frac = parts.next().unwrap_or("");
    if parts.next().is_some() || frac.len() > 2 {
        return Err(());
    }
    if whole.is_empty() && frac.is_empty() {
        return Err(());
    }
    if !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(());
    }
    let dollars: u32 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| ())?
    };
    let mut cents: u32 = if frac.is_empty() {
        0
    } else {
        frac.parse().map_err(|_| ())?
    };
    if frac.len() == 1 {
        cents = cents.saturating_mul(10);
    }
    dollars
        .checked_mul(100)
        .and_then(|base| base.checked_add(cents))
        .ok_or(())
}

fn measure(
    item: &'static str,
    engine: &'static str,
    work: impl FnOnce() -> (&'static str, u32),
) -> ItemResult {
    let start = Instant::now();
    let (status, turns) = work();
    ItemResult {
        item,
        engine,
        success: success_of(status),
        turns,
        wall_ms: start.elapsed().as_millis(),
        status,
        verified: verified_of(status),
    }
}

fn success_of(status: &str) -> bool {
    matches!(
        status,
        "fixed"
            | "refused"
            | "halted"
            | "echoed"
            | "compacted"
            | "spooled"
            | "request built"
            | "scripted reply"
            | "scripted tool"
            | "probe ok"
            | "covered"
            | "checked"
            | "live ok"
            | "skipped: scripted dry-run only"
    )
}

fn verified_of(status: &str) -> bool {
    !status.starts_with("skipped:") && !status.starts_with("not verified:")
}

/// Gaps that hold for every dry-run, whatever the rows say.
pub const STANDING_GAPS: [&str; 2] = [
    "- live: not run. Pass `--live` with GROKHUB_EVAL_API_KEY and `--budget-usd` at most $0.20 for the budgeted live probe; default PR CI stays offline.",
    "- pool % and cost: written as 0 for every dry-run row. They were not measured.",
];

const DESKTOP_PROBE_GAP: &str = "- xvfb-desktop: the probe only checks that an X display answers (`xdpyinfo`). The desktop tools were not driven.";

fn gap_lines(items: &[ItemResult]) -> Vec<String> {
    let mut lines: Vec<String> = STANDING_GAPS.iter().map(|line| line.to_string()).collect();
    if items
        .iter()
        .any(|item| item.item == "xvfb-desktop" && item.status == "probe ok")
    {
        lines.push(DESKTOP_PROBE_GAP.to_string());
    }
    for name in SUITE_ITEMS {
        let Some(native) = items
            .iter()
            .find(|item| item.item == name && item.engine == "native")
        else {
            lines.push(format!("- {name}: missing native result"));
            continue;
        };
        if native.status == SKIP_XVFB {
            lines.push(format!("- {name}: {SKIP_XVFB}"));
            continue;
        }
        let mut parts = Vec::new();
        if !native.verified {
            parts.push(format!("native {}", native.status));
        } else if !native.success {
            parts.push(format!("native failed ({})", native.status));
        }
        if !parts.is_empty() {
            lines.push(format!("- {name}: {}", parts.join("; ")));
        }
    }
    lines
}

fn live_gap_lines(items: &[ItemResult], spend: Option<LiveSpend>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(spend) = spend {
        if spend.spent_ticks > spend.budget_ticks {
            lines.push(format!(
                "- budget: spent {} ticks over the {} tick cap",
                spend.spent_ticks, spend.budget_ticks
            ));
        }
    }
    if items
        .iter()
        .any(|item| item.item == "xvfb-desktop" && item.status == "probe ok")
    {
        lines.push(DESKTOP_PROBE_GAP.to_string());
    }
    for item in items {
        if item.status.starts_with("skipped:") || !item.verified {
            lines.push(format!("- {}: {}", item.item, item.status));
        } else if !item.success {
            lines.push(format!("- {}: failed ({})", item.item, item.status));
        }
    }
    lines
}

#[cfg(unix)]
use scan::system_scan_native;

#[cfg(not(unix))]
fn system_scan_native() -> (&'static str, u32) {
    ("skipped: the fake host needs a Unix shell", 0)
}

fn desktop_item() -> (&'static str, u32) {
    (desktop_status(), 0)
}

fn desktop_status() -> &'static str {
    if !command_on_path("Xvfb") || !display_set() {
        return SKIP_XVFB;
    }
    if probe_current_display() {
        "probe ok"
    } else {
        SKIP_XVFB
    }
}

fn display_set() -> bool {
    std::env::var("DISPLAY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn command_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&path).any(|dir| dir.join(&file).is_file() || dir.join(name).is_file())
}

fn probe_current_display() -> bool {
    let mut cmd = Command::new("xdpyinfo");
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() > Duration::from_secs(2) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return false,
        }
    }
}

fn repo_bugfix_native() -> (&'static str, u32) {
    let Some(dir) = scratch("bugfix") else {
        return ("failed: scratch", 0);
    };
    if write_bugfix_fixture(&dir).is_err() {
        return ("failed: scratch", 0);
    }
    let client = Arc::new(Scripted::bugfix());
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let done = run_native(
        &dir,
        "eval-bugfix",
        shared,
        PermMode::Always,
        "fix the failing test",
    );
    let turns = client.turns();
    let edited = bugfix_edited(&dir);
    let fixed = edited && fixture_passes(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    if done.stop != "end_turn" {
        ("failed: turn did not end", turns)
    } else if !edited {
        ("failed: edit not applied", turns)
    } else if !fixed {
        ("failed: fixture test", turns)
    } else {
        ("fixed", turns)
    }
}

fn ask_native() -> (&'static str, u32) {
    let Some(dir) = scratch("ask") else {
        return ("failed: scratch", 0);
    };
    let client = Arc::new(Scripted::ask());
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let done = run_native(&dir, "eval-ask", shared, PermMode::Ask, "write the secret");
    let blob = client.blob();
    let refused = done.perms == 0
        && done.stop == "end_turn"
        && !dir.join("secret.txt").exists()
        && blob.contains(&crate::gate::unattended_deny("write"));
    let turns = client.turns();
    let _ = std::fs::remove_dir_all(&dir);
    if refused {
        ("refused", turns)
    } else {
        ("failed: deny", turns)
    }
}

fn background_halt_native() -> (&'static str, u32) {
    let Some(dir) = scratch("halt") else {
        return ("failed: scratch", 0);
    };
    let blocked = Arc::new(AtomicBool::new(false));
    let client = Arc::new(Scripted::halt(Arc::clone(&blocked)));
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let cancel = CancelToken::new();
    let cancel_main = cancel.clone();
    let dir_run = dir.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let _guard = ConfigGuard::set(dir_run.join("cfg"));
        let _ = std::fs::create_dir_all(dir_run.join("cfg"));
        let done = run_native_cancel(
            &dir_run,
            "eval-bg-halt",
            shared,
            PermMode::Always,
            "start a background sleep and wait",
            cancel,
        );
        let _ = tx.send(done);
    });
    // Windows CI can take well over 5s to spawn the background command and
    // reach the second model turn. Wait for that turn (blocked) before halt.
    let start = Instant::now();
    while !blocked.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(30) {
        thread::sleep(Duration::from_millis(10));
    }
    let blob_now = client.blob();
    let alive_before = command_is_live(&dir, "eval-bg-halt", &blob_now);
    let pids = workspace_pids(&dir);
    // Halt the background task only. `halt_session` also cancels the run's
    // CancelToken, which wakes Scripted::halt and lets the worker forget the
    // session before Windows can read `status: killed` from task_text.
    crate::tasks::hub_for("eval-bg-halt").halt_all();
    let stopped = command_is_stopped("eval-bg-halt", &blob_now, &pids);
    cancel_main.cancel();
    let done = rx.recv_timeout(Duration::from_secs(5)).ok();
    let _ = worker.join();
    #[cfg(unix)]
    {
        for pid in &pids {
            force_kill(*pid);
        }
    }
    crate::tasks::forget_session("eval-bg-halt");
    let turns = client.turns();
    let blob = client.blob();
    let stop_ok = done
        .as_ref()
        .is_some_and(|item| item.stop == "cancelled" || item.stop == "halted");
    let _ = std::fs::remove_dir_all(&dir);
    if blocked.load(Ordering::SeqCst)
        && alive_before
        && stopped
        && stop_ok
        && blob.contains("task id:")
    {
        ("halted", turns)
    } else {
        ("failed: halt", turns)
    }
}

fn mcp_native(cfg: Option<&Path>) -> (&'static str, u32) {
    let Some(cfg) = cfg else {
        return ("failed: scratch", 0);
    };
    let Some(server) = sibling_bin("fake_mcp") else {
        return (NO_FAKE_MCP, 0);
    };
    let _guard = ConfigGuard::set(cfg);
    if write_mcp_config(cfg, &server).is_err() {
        return ("failed: mcp", 0);
    }
    crate::mcp::shutdown_all();
    crate::mcp::invalidate();
    let Some(dir) = scratch("mcp") else {
        clear_mcp(cfg);
        return ("failed: scratch", 0);
    };
    let client = Arc::new(Scripted::mcp());
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let done = run_native(
        &dir,
        "eval-mcp",
        shared,
        PermMode::Always,
        "call the echo tool",
    );
    let blob = client.blob();
    let turns = client.turns();
    clear_mcp(cfg);
    let _ = std::fs::remove_dir_all(&dir);
    if done.stop == "end_turn" && blob.contains("pid=") && blob.contains("args=") {
        ("echoed", turns)
    } else {
        ("failed: mcp", turns)
    }
}

fn tool_output_compact_native() -> (&'static str, u32) {
    crate::compact::eval_tool_output_compact_native()
}

fn compact_native() -> (&'static str, u32) {
    let Some(dir) = scratch("compact") else {
        return ("failed: scratch", 0);
    };
    let history = long_history();
    // The window a Grok chat trims in: its own, capped at the 200k price step.
    let window =
        crate::compact::trim_window(crate::DEFAULT_MODEL, crate::models::GROK_47_CONTEXT_LENGTH);
    if !crate::compact::needs_auto_compact(&history, window) {
        let _ = std::fs::remove_dir_all(&dir);
        return ("failed: compact", 0);
    }
    let client = Arc::new(Scripted::compact());
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let mut engine = native_engine(&dir, "eval-compact", shared, PermMode::Always, 4);
    engine.resume(history, Usage::default());
    let mut text = String::new();
    let mut started = false;
    let mut finished = false;
    let _ = engine.prompt("continue", None, &mut |ev| match ev {
        AcpEvent::Text(chunk) => text.push_str(&chunk),
        AcpEvent::Compact {
            started: true,
            error: None,
            ..
        } => started = true,
        AcpEvent::Compact {
            started: false,
            error: None,
            ..
        } => finished = true,
        _ => {}
    });
    let turns = client.turns();
    crate::tasks::forget_session("eval-compact");
    let _ = std::fs::remove_dir_all(&dir);
    if started && finished && text.contains("continued") {
        ("compacted", turns)
    } else {
        ("failed: compact", turns)
    }
}

fn imagine_native() -> (&'static str, u32) {
    let api = NopImagine;
    let before = imagine_send_count();
    let built = build_imagine_request(&api);
    let sends = imagine_send_count().saturating_sub(before);
    match built {
        Ok(body)
            if sends == 0
                && body.get("prompt").and_then(|value| value.as_str()) == Some(IMAGINE_PROMPT) =>
        {
            ("request built", 1)
        }
        _ => ("failed: imagine", 1),
    }
}

/// Build the Imagine generation body. `api` is not called.
pub fn build_imagine_request(api: &dyn ImagineApi) -> Result<Value, String> {
    let _ = api;
    let expect =
        grokhub_core::imagine_generation_body(IMAGINE_PROMPT, "", 1, "1k", "1:1", "", "url");
    let body = crate::tools::dry_imagine_body(&imagine_args())?;
    if body != expect {
        return Err("imagine body did not match the media builder".into());
    }
    Ok(body)
}

fn imagine_args() -> Value {
    json!({
        "prompt": IMAGINE_PROMPT,
        "n": 1,
        "resolution": "1k",
        "aspect_ratio": "1:1",
    })
}

struct NopImagine;

impl ImagineApi for NopImagine {
    fn post_json(&self, _path: &str, _body: &Value) -> Result<Value, String> {
        IMAGINE_SENDS.fetch_add(1, Ordering::SeqCst);
        Err("dry-run must not send".into())
    }

    fn get_json(&self, _path: &str) -> Result<Value, String> {
        IMAGINE_SENDS.fetch_add(1, Ordering::SeqCst);
        Err("dry-run must not send".into())
    }

    fn download(&self, _url: &str) -> Result<Vec<u8>, String> {
        IMAGINE_SENDS.fetch_add(1, Ordering::SeqCst);
        Err("dry-run must not send".into())
    }
}

struct NativeDone {
    stop: String,
    perms: u32,
}

fn run_native(
    dir: &Path,
    id: &str,
    client: Arc<dyn ModelClient + Send + Sync>,
    mode: PermMode,
    prompt: &str,
) -> NativeDone {
    run_native_cancel(dir, id, client, mode, prompt, CancelToken::new())
}

fn run_native_cancel(
    dir: &Path,
    id: &str,
    client: Arc<dyn ModelClient + Send + Sync>,
    mode: PermMode,
    prompt: &str,
    cancel: CancelToken,
) -> NativeDone {
    let done = crate::run_unattended(crate::UnattendedRun {
        client,
        workspace: dir.to_path_buf(),
        model: "grok-4.7".into(),
        effort: Some("high".into()),
        system: String::new(),
        session_id: id.to_string(),
        auth_kind: AuthKind::ApiKey,
        prompt: prompt.to_string(),
        image: None,
        mode,
        readonly_session: false,
        desktop: false,
        cancel,
        halt: Box::new(NoHalt),
        desktop_ops: None,
        resume: false,
    });
    NativeDone {
        stop: done.stop_reason,
        perms: done.permission_cards,
    }
}

fn native_engine(
    dir: &Path,
    id: &str,
    client: Arc<dyn ModelClient + Send + Sync>,
    mode: PermMode,
    max_turns: u32,
) -> NativeEngine {
    NativeEngine::new(EngineParts {
        client,
        workspace: dir.to_path_buf(),
        model: "grok-4.7".into(),
        effort: Some("high".into()),
        system: String::new(),
        conversation_id: id.to_string(),
        auth_kind: AuthKind::ApiKey,
        max_turns,
        cancel: CancelToken::new(),
        steer: SteerQueue::new(),
        halt: Box::new(NoHalt),
        gate: Gate {
            mode,
            readonly_session: false,
            attended: false,
            desktop: false,
        },
        desktop: None,
        permits: Arc::new(ClosedPermits),
    })
}

struct NoHalt;

impl HaltCheck for NoHalt {
    fn halted(&self) -> bool {
        false
    }
}

enum Kind {
    Bugfix,
    Ask,
    Halt { blocked: Arc<AtomicBool> },
    Mcp,
    Compact,
}

struct Scripted {
    kind: Kind,
    n: AtomicUsize,
    blob: Mutex<String>,
}

impl Scripted {
    fn bugfix() -> Self {
        Self::new(Kind::Bugfix)
    }
    fn ask() -> Self {
        Self::new(Kind::Ask)
    }
    fn halt(blocked: Arc<AtomicBool>) -> Self {
        Self::new(Kind::Halt { blocked })
    }
    fn mcp() -> Self {
        Self::new(Kind::Mcp)
    }
    fn compact() -> Self {
        Self::new(Kind::Compact)
    }
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            n: AtomicUsize::new(0),
            blob: Mutex::new(String::new()),
        }
    }
    fn turns(&self) -> u32 {
        u32::try_from(self.n.load(Ordering::SeqCst)).unwrap_or(u32::MAX)
    }
    fn blob(&self) -> String {
        self.blob
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }
    fn save_blob(&self, req: &ResponsesRequest) {
        *self.blob.lock().unwrap_or_else(|err| err.into_inner()) = request_blob(req);
    }
}

impl ModelClient for Scripted {
    fn stream(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let n = self.n.fetch_add(1, Ordering::SeqCst);
        if matches!(self.kind, Kind::Compact) && is_summary(req) {
            return Ok(text_turn("<summary>kept the plan</summary>"));
        }
        if let Kind::Halt { blocked } = &self.kind {
            if n > 0 {
                self.save_blob(req);
                blocked.store(true, Ordering::SeqCst);
                let start = Instant::now();
                loop {
                    if cancel.is_cancelled() {
                        return Err(ClientError::Cancelled);
                    }
                    // Ceiling covers hub.halt_all + the 5s stop poll before cancel.
                    if start.elapsed() > Duration::from_secs(12) {
                        return Err(ClientError::Protocol("halt did not arrive".into()));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
        if n == 0 {
            return Ok(match &self.kind {
                Kind::Bugfix => tool_turn(
                    "edit",
                    "search_replace",
                    &json!({
                        "file_path": BUGFIX_LIB.0,
                        "old_string": BUGFIX_EDIT.0,
                        "new_string": BUGFIX_EDIT.1,
                    })
                    .to_string(),
                ),
                Kind::Ask => tool_turn(
                    "w",
                    "write",
                    &json!({"path": "secret.txt", "content": "nope"}).to_string(),
                ),
                Kind::Halt { .. } => tool_turn(
                    "bg",
                    "run_terminal_command",
                    &json!({"command": bg_command(), "is_background": true}).to_string(),
                ),
                Kind::Mcp => tool_turn("m", "evalmcp__echo", "{}"),
                Kind::Compact => {
                    sink(StreamEvent::TextDelta("continued".into()));
                    text_turn("continued")
                }
            });
        }
        self.save_blob(req);
        let text = match self.kind {
            Kind::Bugfix => "fixed",
            Kind::Ask => "refused",
            Kind::Mcp => "echoed",
            Kind::Compact => "continued",
            Kind::Halt { .. } => "halted",
        };
        sink(StreamEvent::TextDelta(text.into()));
        Ok(text_turn(text))
    }
}

fn tool_turn(id: &str, name: &str, arguments: &str) -> TurnOutput {
    TurnOutput {
        text: String::new(),
        reasoning: String::new(),
        calls: vec![FunctionCall {
            call_id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }],
        usage: Usage::default(),
        ..Default::default()
    }
}

fn text_turn(text: &str) -> TurnOutput {
    TurnOutput {
        text: text.to_string(),
        reasoning: String::new(),
        calls: Vec::new(),
        usage: Usage::default(),
        ..Default::default()
    }
}

fn is_summary(req: &ResponsesRequest) -> bool {
    req.input
        .iter()
        .any(|item| crate::compact::message_text(item).contains("faithful, concise summary"))
}

fn request_blob(req: &ResponsesRequest) -> String {
    let mut out = String::new();
    for item in &req.input {
        match item {
            InputItem::Message { content, .. } => {
                for part in content {
                    if let ContentPart::InputText(text) = part {
                        out.push_str(text);
                        out.push('\n');
                    }
                }
            }
            InputItem::FunctionCallOutput { output, .. } => {
                out.push_str(output);
                out.push('\n');
            }
            InputItem::FunctionCall {
                name, arguments, ..
            } => {
                out.push_str(name);
                out.push(' ');
                out.push_str(arguments);
                out.push('\n');
            }
            InputItem::ServerCompaction { .. } | InputItem::Reasoning { .. } => {}
        }
    }
    out
}

fn long_history() -> Vec<InputItem> {
    vec![InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText("x".repeat(900_000))],
    }]
}

// The fixture is a two-file shell project on unix and a PowerShell one on
// Windows, so the "run the tests" step works on both CI runners.
#[cfg(unix)]
const BUGFIX_LIB: (&str, &str) = ("lib.sh", "add() {\n  echo $(( $1 - $2 ))\n}\n");
#[cfg(unix)]
const BUGFIX_TEST: (&str, &str) = (
    "test.sh",
    "#!/bin/sh\nset -eu\n. ./lib.sh\nresult=$(add 2 3)\ntest \"$result\" = 5\nprintf '%s\\n' ok\n",
);
#[cfg(unix)]
const BUGFIX_EDIT: (&str, &str) = ("echo $(( $1 - $2 ))", "echo $(( $1 + $2 ))");

#[cfg(not(unix))]
const BUGFIX_LIB: (&str, &str) = ("lib.ps1", "function add($a, $b) { $a - $b }\n");
#[cfg(not(unix))]
const BUGFIX_TEST: (&str, &str) = (
    "test.ps1",
    ". ./lib.ps1\nif ((add 2 3) -ne 5) { exit 1 }\nWrite-Output ok\n",
);
#[cfg(not(unix))]
const BUGFIX_EDIT: (&str, &str) = ("$a - $b", "$a + $b");

fn write_bugfix_fixture(dir: &Path) -> Result<(), String> {
    std::fs::write(dir.join(BUGFIX_LIB.0), BUGFIX_LIB.1).map_err(|err| err.to_string())?;
    std::fs::write(dir.join(BUGFIX_TEST.0), BUGFIX_TEST.1).map_err(|err| err.to_string())
}

fn bugfix_edited(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(BUGFIX_LIB.0)).is_ok_and(|text| text.contains(BUGFIX_EDIT.1))
}

fn fixture_passes(dir: &Path) -> bool {
    #[cfg(unix)]
    let mut cmd = {
        let mut cmd = Command::new("sh");
        cmd.arg(BUGFIX_TEST.0);
        cmd
    };
    #[cfg(not(unix))]
    let mut cmd = {
        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ]);
        cmd.arg(BUGFIX_TEST.0);
        cmd
    };
    let Ok(output) = cmd.current_dir(dir).output() else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).contains("ok")
}

fn write_mcp_config(cfg: &Path, server: &Path) -> Result<(), String> {
    std::fs::create_dir_all(cfg).map_err(|err| err.to_string())?;
    let body = json!({
        "mcpServers": {
            "evalmcp": {
                "command": server.display().to_string(),
                "args": []
            }
        }
    });
    std::fs::write(cfg.join("mcp.json"), body.to_string()).map_err(|err| err.to_string())
}

fn clear_mcp(cfg: &Path) {
    let _ = std::fs::remove_file(cfg.join("mcp.json"));
    crate::mcp::shutdown_all();
    crate::mcp::invalidate();
}

fn bg_command() -> &'static str {
    // A single plain command. `$`, redirects, and `;` do not split cleanly, and
    // an unsplittable shell call is refused while unattended even in Always mode.
    #[cfg(unix)]
    {
        "sleep 30"
    }
    #[cfg(not(unix))]
    {
        "ping -n 31 127.0.0.1"
    }
}

fn task_id_from(blob: &str) -> Option<String> {
    let line = blob.lines().find(|line| line.contains("task id:"))?;
    let rest = line.split("task id:").nth(1)?.trim();
    let id = rest.split_whitespace().next()?.trim();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn task_text(session: &str, blob: &str) -> Option<String> {
    let id = task_id_from(blob)?;
    crate::tasks::hub_for(session).read_output(&id, 0).ok()
}

fn command_is_live(dir: &Path, session: &str, blob: &str) -> bool {
    let running = task_text(session, blob).is_some_and(|text| text.contains("status: running"));
    if !running {
        return false;
    }
    #[cfg(unix)]
    {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            let pids = workspace_pids(dir);
            if !pids.is_empty() && pids.iter().copied().any(process_alive) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        true
    }
}

fn command_is_stopped(session: &str, blob: &str, pids: &[u32]) -> bool {
    #[cfg(unix)]
    {
        let _ = (session, blob);
        if pids.is_empty() {
            return false;
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if pids.iter().copied().all(|pid| !process_alive(pid)) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        pids.iter().copied().all(|pid| !process_alive(pid))
    }
    #[cfg(not(unix))]
    {
        let _ = pids;
        let start = Instant::now();
        // Generous for slow Windows CI; returns as soon as status: killed appears.
        while start.elapsed() < Duration::from_secs(5) {
            if task_text(session, blob).is_some_and(|text| text.contains("status: killed")) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

fn workspace_pids(dir: &Path) -> Vec<u32> {
    #[cfg(unix)]
    {
        let Ok(canon) = dir.canonicalize() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        let self_pid = std::process::id();
        let mut pids = Vec::new();
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(pid) = name.parse::<u32>() else {
                continue;
            };
            if pid == 0 || pid == self_pid {
                continue;
            }
            let Ok(cwd) = std::fs::read_link(entry.path().join("cwd")) else {
                continue;
            };
            if cwd == canon {
                pids.push(pid);
            }
        }
        pids
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Vec::new()
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let pid = pid as i32;
    if pid <= 0 {
        return false;
    }
    // Signal 0 checks that the process exists. It does not deliver a signal.
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(unix)]
fn force_kill(pid: u32) {
    if pid > 0 && process_alive(pid) {
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    }
}

fn cleanup_sessions() {
    for id in [
        "eval-bugfix",
        "eval-ask",
        "eval-bg-halt",
        "eval-mcp",
        "eval-compact",
    ] {
        crate::tasks::forget_session(id);
    }
    crate::mcp::shutdown_all();
}

fn sibling_bin(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    exe.ancestors().take(8).find_map(|dir| {
        let candidate = dir.join(&file);
        candidate.is_file().then_some(candidate)
    })
}

fn scratch(label: &str) -> Option<PathBuf> {
    let n = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("gh-eval-{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    struct Rec {
        posts: AtomicUsize,
        gets: AtomicUsize,
        downloads: AtomicUsize,
    }

    impl ImagineApi for Rec {
        fn post_json(&self, _path: &str, _body: &Value) -> Result<Value, String> {
            self.posts.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        }
        fn get_json(&self, _path: &str) -> Result<Value, String> {
            self.gets.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        }
        fn download(&self, _url: &str) -> Result<Vec<u8>, String> {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            Err("not sent".into())
        }
    }

    #[test]
    fn dry_run_is_the_default() {
        let opts = parse_args(&[]).expect("default");
        assert!(!opts.live);
        assert_eq!(opts.budget_cents, None);
        assert!(opts.out.is_none());
        let flagged = parse_args(&["--dry-run".into()]).expect("dry-run");
        assert!(!flagged.live);
    }

    #[test]
    fn live_refuses_without_api_key() {
        let err = parse_args_with_key(&["--live".into()], None).expect_err("no key");
        assert_eq!(err, "refusing --live: GROKHUB_EVAL_API_KEY is not set");
    }

    #[test]
    fn live_refuses_when_api_key_is_empty() {
        let err = parse_args_with_key(
            &["--live".into(), "--budget-usd".into(), "0.20".into()],
            Some("   "),
        )
        .expect_err("empty");
        assert_eq!(err, "refusing --live: GROKHUB_EVAL_API_KEY is not set");
    }

    #[test]
    fn live_refuses_with_key_but_no_budget() {
        let err = parse_args_with_key(&["--live".into()], Some("sanctioned-test-key"))
            .expect_err("no budget");
        assert_eq!(
            err,
            "refusing --live: --budget-usd is required and must satisfy 0 < amount <= 0.20"
        );
    }

    #[test]
    fn live_refuses_budget_above_cap() {
        let err = parse_args_with_key(
            &["--live".into(), "--budget-usd".into(), "0.25".into()],
            Some("sanctioned-test-key"),
        )
        .expect_err("cap");
        assert_eq!(
            err,
            "refusing --live: --budget-usd 0.25 is above the 0.20 cap"
        );
    }

    #[test]
    fn live_refuses_budget_zero() {
        let err = parse_args_with_key(
            &["--live".into(), "--budget-usd".into(), "0".into()],
            Some("sanctioned-test-key"),
        )
        .expect_err("zero");
        assert_eq!(
            err,
            "refusing --live: --budget-usd must satisfy 0 < amount <= 0.20"
        );
    }

    #[test]
    fn live_refuses_budget_negative() {
        let err = parse_args_with_key(
            &["--live".into(), "--budget-usd".into(), "-1".into()],
            Some("sanctioned-test-key"),
        )
        .expect_err("neg");
        assert_eq!(
            err,
            "refusing --live: --budget-usd must satisfy 0 < amount <= 0.20"
        );
    }

    #[test]
    fn live_with_key_and_budget_is_accepted() {
        let opts = parse_args_with_key(
            &["--live".into(), "--budget-usd".into(), "0.20".into()],
            Some("sanctioned-test-key"),
        )
        .expect("parsed");
        assert!(opts.live);
        assert_eq!(opts.budget_cents, Some(20));
        reject_live(&opts).expect("live ready");
        assert_eq!(live_budget_ticks(20), live::TICKS_PER_USD / 5);
    }

    #[test]
    fn live_suite_with_fake_client_stays_under_budget() {
        struct Pong;
        impl ModelClient for Pong {
            fn stream(
                &self,
                _req: &ResponsesRequest,
                _cancel: &CancelToken,
                sink: &mut dyn FnMut(StreamEvent),
            ) -> Result<TurnOutput, ClientError> {
                sink(StreamEvent::TextDelta("pong".into()));
                Ok(TurnOutput {
                    text: "pong".into(),
                    usage: Usage {
                        input_tokens: 100,
                        cached_tokens: 0,
                        output_tokens: 1,
                        reasoning_tokens: 0,
                        cost_in_usd_ticks: live::TICKS_PER_USD / 100,
                    },
                    ..Default::default()
                })
            }
        }
        let budget = live_budget_ticks(20);
        let suite = run_live_suite_with(Arc::new(Pong), budget);
        assert_eq!(suite.items.len(), SUITE_ITEMS.len());
        assert!(suite.spend.spent_ticks <= suite.spend.budget_ticks);
        let reply = suite
            .items
            .iter()
            .find(|item| item.item == "repo-bugfix")
            .expect("live probe");
        assert_eq!(reply.status, "live ok");
        assert!(reply.verified);
        let report = render_report_with(&suite.items, Some(suite.spend));
        assert!(report.contains("opt-in"));
        assert!(report.contains("Capped"));
        assert!(!report.contains("not implemented"));
        assert!(report.contains("| repo-bugfix | native | yes |"));
    }

    #[test]
    fn imagine_dry_run_sends_nothing() {
        let api = Rec {
            posts: AtomicUsize::new(0),
            gets: AtomicUsize::new(0),
            downloads: AtomicUsize::new(0),
        };
        let body = build_imagine_request(&api).expect("body");
        let expect =
            grokhub_core::imagine_generation_body("a red square", "", 1, "1k", "1:1", "", "url");
        assert_eq!(body, expect);
        assert_eq!(body["prompt"], json!("a red square"));
        assert_eq!(body["n"], json!(1));
        assert_eq!(api.posts.load(Ordering::SeqCst), 0);
        assert_eq!(api.gets.load(Ordering::SeqCst), 0);
        assert_eq!(api.downloads.load(Ordering::SeqCst), 0);
        assert_eq!(imagine_send_count(), 0);
    }

    #[test]
    fn dry_run_report_lists_every_item_and_is_byte_identical() {
        let opts = parse_args(&[]).expect("dry-run");
        assert!(!opts.live);
        let first = run_suite(&opts);
        let second = run_suite(&opts);
        let report = render_report(&first);
        let again = render_report(&second);
        assert_eq!(report.as_bytes(), again.as_bytes());
        assert_eq!(first.len(), SUITE_ITEMS.len());
        for name in SUITE_ITEMS {
            let needle_native = format!("| {name} | native |");
            assert_eq!(report.matches(&needle_native).count(), 1, "{report}");
        }
        assert!(
            !report.contains("| cli |") && !report.contains("fake-acp"),
            "{report}"
        );
        assert!(report.contains("## GAPS\n"));
        let gaps = report.split("## GAPS\n\n").nth(1).expect("gaps body");
        for line in STANDING_GAPS {
            assert_eq!(gaps.matches(line).count(), 1, "{line}");
        }
        assert!(gaps.starts_with("- live: not run."), "{gaps}");
        assert!(!gaps.contains("- none"));
        assert!(report.contains("No default was switched.\n"));
        assert!(report.contains("GROKHUB_EVAL_API_KEY"));
        assert!(report.contains("0.20"));
        assert!(report.contains(&credential_boundary()));
        assert!(report.contains("| dry-run |"));
        assert!(!report.contains("gh-eval"));
        assert!(!report.contains("/tmp"));
        assert!(report.contains("| imagine | native | yes | 1 | dry-run | 0 | 0 | request built |"));
        assert_eq!(imagine_send_count(), 0);
        assert!(
            report.contains("| repo-bugfix | native | yes |"),
            "{report}"
        );
        assert!(
            report.contains("| ask-refusal | native | yes |"),
            "{report}"
        );
        assert!(
            report.contains("| background-halt | native | yes |"),
            "{report}"
        );
        assert!(report.contains("| compaction | native | yes |"), "{report}");
        assert!(report.contains("| mcp-tool | native | yes |"), "{report}");
        if cfg!(unix) {
            assert!(
                report.contains("| system-scan | native | yes | 6 |"),
                "{report}"
            );
            assert!(!report.contains("- system-scan:"), "{report}");
        } else {
            assert!(
                report
                    .contains("- system-scan: native skipped: the fake host needs a Unix shell\n"),
                "{report}"
            );
        }
        if report.contains(SKIP_XVFB) {
            assert!(report.contains("- xvfb-desktop: skipped: no Xvfb\n"));
        }
    }

    #[test]
    fn ab_prompt_eval_passes_gate() {
        let report = ab::run_ab(ab::DEFAULT_MODEL, &ab::CandidateSuffix).expect("ab eval");
        report
            .gate()
            .expect("variant must not regress success rate");
    }
}
