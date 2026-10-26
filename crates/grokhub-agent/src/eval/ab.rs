//! A/B eval harness: same model on control vs variant system prompts, fixed internal tasks.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Deserialize;
use serde_json::Value;

use crate::client::{ClientError, FunctionCall, ResponsesRequest, StreamEvent, TurnOutput};
use crate::gate::PermMode;
use crate::prompt::{system_prompt_with, PromptNeed, PromptProfile};
use crate::{run_unattended, AuthKind, CancelToken, HaltCheck, ModelClient, UnattendedRun, Usage};

use super::metrics::{
    derive_usage, format_report, gate, offline_cost_ticks, recommend, success_within_noise,
    AbComparison, AbGateFailure, ArmMetrics, CapturedRequest, Recommendation, RequestTotals,
    TechniqueDelta,
};
use crate::compact::TOOL_OUTPUT_COMPACT_CONFIG;
use crate::perm::ConfigGuard;

pub const EVAL_TASKS_DIR: &str = "tests/fixtures/eval_tasks";
pub const DEFAULT_MODEL: &str = "grok-4.7";
/// Route class for A/B task runs (shadow routing only).
pub const AB_EVAL_CLASS: &str = "eval:ab";

#[derive(Debug, Clone, Deserialize)]
pub struct TaskFixture {
    pub id: String,
    pub prompt: String,
    /// Later user turns in the same session (each resumes the saved transcript).
    #[serde(default)]
    pub followups: Vec<String>,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub files: Vec<FileSeed>,
    pub script: Vec<ScriptStep>,
    pub checks: Vec<TaskCheck>,
}

fn default_mode() -> String {
    "always".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileSeed {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScriptStep {
    Text {
        content: String,
        #[serde(default)]
        usage: StepUsage,
    },
    Tool {
        name: String,
        arguments: Value,
        #[serde(default)]
        usage: StepUsage,
    },
    /// Several tool calls in one model reply.
    Tools {
        calls: Vec<ToolSeed>,
        #[serde(default)]
        usage: StepUsage,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolSeed {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StepUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
    /// When set (including 0), used as-is; when absent, offline cost is derived.
    #[serde(default)]
    pub cost_in_usd_ticks: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskCheck {
    StopReason { value: String },
    AssistantContains { value: String },
    NoPermissionCards,
    FileExists { path: String },
    FileContains { path: String, value: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmKind {
    Control,
    Variant,
}

/// R6 technique matrix arms. A = [`Technique::Baseline`] (all settings off).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Technique {
    Baseline,
    PrefixCache,
    ToolOutputSummaries,
    DeferredTools,
    BatchedReadOnly,
    AllOn,
}

impl Technique {
    pub const ALL: [Technique; 6] = [
        Technique::Baseline,
        Technique::PrefixCache,
        Technique::ToolOutputSummaries,
        Technique::DeferredTools,
        Technique::BatchedReadOnly,
        Technique::AllOn,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Technique::Baseline => "baseline (all off)",
            Technique::PrefixCache => "R2 prefix cache",
            Technique::ToolOutputSummaries => "R3 tool-output summaries",
            Technique::DeferredTools => "R4 deferred tools",
            Technique::BatchedReadOnly => "R5 batched read-only",
            Technique::AllOn => "all on",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Technique::Baseline => "baseline",
            Technique::PrefixCache => "prefix_cache",
            Technique::ToolOutputSummaries => "tool_output_summaries",
            Technique::DeferredTools => "deferred_tools",
            Technique::BatchedReadOnly => "batched_readonly",
            Technique::AllOn => "all_on",
        }
    }
}

/// Write technique toggles into a temp config dir (never the user's config).
/// Exactly one B-arm setting (or all / none) — files always rewritten so leftover
/// state from a prior arm cannot leak.
pub fn arm_settings(technique: Technique, config_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|err| err.to_string())?;
    let (prefix, summaries, deferred, batched) = match technique {
        Technique::Baseline => (false, false, false, false),
        Technique::PrefixCache => (true, false, false, false),
        Technique::ToolOutputSummaries => (false, true, false, false),
        Technique::DeferredTools => (false, false, true, false),
        Technique::BatchedReadOnly => (false, false, false, true),
        Technique::AllOn => (true, true, true, true),
    };
    write_enabled(config_dir, "prefix_cache.toml", prefix)?;
    write_enabled(config_dir, TOOL_OUTPUT_COMPACT_CONFIG, summaries)?;
    write_enabled(config_dir, "deferred_tools.toml", deferred)?;
    write_enabled(config_dir, "batched_tools.toml", batched)?;
    Ok(())
}

fn write_enabled(config_dir: &Path, name: &str, on: bool) -> Result<(), String> {
    let body = if on {
        "enabled = true\n"
    } else {
        "enabled = false\n"
    };
    std::fs::write(config_dir.join(name), body).map_err(|err| err.to_string())
}

/// How the variant arm builds its system prompt (control uses [`BaselineSystem`]).
pub trait VariantSystem: Send + Sync {
    fn system(&self, workspace: &Path) -> String;
}

/// Production control arm: chat profile, full prompt need.
pub struct BaselineSystem;

impl BaselineSystem {
    pub fn system(workspace: &Path) -> String {
        system_prompt_with(PromptProfile::Chat, "", "", workspace, PromptNeed::FULL)
    }
}

/// Default candidate arm: same layers as control with a stable eval suffix.
pub struct CandidateSuffix;

impl VariantSystem for CandidateSuffix {
    fn system(&self, workspace: &Path) -> String {
        let base = BaselineSystem::system(workspace);
        format!("{base}\n[eval-ab-variant]\n")
    }
}

#[derive(Debug, Clone)]
pub struct TaskRun {
    pub task_id: String,
    pub arm: ArmKind,
    pub success: bool,
    pub detail: String,
    pub wall_ms: u128,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub tool_calls: u64,
    pub cached_tokens: u64,
    pub cost_ticks: i64,
    /// Shape of the task's outbound requests (model calls, prefix, schema bytes).
    pub requests: RequestTotals,
}

#[derive(Debug, Clone)]
pub struct AbReport {
    pub comparison: AbComparison,
    pub tasks: Vec<TaskRun>,
    pub report_markdown: String,
}

impl AbReport {
    pub fn gate(&self) -> Result<(), AbGateFailure> {
        gate(&self.comparison)
    }
}

/// One arm of the R6 technique matrix.
#[derive(Debug, Clone)]
pub struct TechniqueArm {
    pub technique: Technique,
    pub metrics: ArmMetrics,
    pub tasks: Vec<TaskRun>,
    pub delta: TechniqueDelta,
    pub success_within_noise: bool,
    pub recommendation: Recommendation,
}

/// Full R6 matrix: baseline A plus each B technique arm.
#[derive(Debug, Clone)]
pub struct TechniqueMatrixReport {
    pub model: String,
    pub arms: Vec<TechniqueArm>,
}

/// Load every `*.json` task under [`fixtures_dir`].
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(EVAL_TASKS_DIR)
}

pub fn load_tasks() -> Result<Vec<TaskFixture>, String> {
    let dir = fixtures_dir();
    let entries =
        std::fs::read_dir(&dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    let mut tasks = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|err| format!("read {}: {err}", path.display()))?;
        let task: TaskFixture =
            serde_json::from_str(&raw).map_err(|err| format!("parse {}: {err}", path.display()))?;
        tasks.push(task);
    }
    tasks.sort_by(|a, b| a.id.cmp(&b.id));
    if tasks.is_empty() {
        return Err(format!("no eval tasks in {}", dir.display()));
    }
    Ok(tasks)
}

/// Run the full A/B set on one model with the given variant system builder.
pub fn run_ab(model: &str, variant: &dyn VariantSystem) -> Result<AbReport, String> {
    let tasks = load_tasks()?;
    let _class = crate::route::live::ClassScope::enter(AB_EVAL_CLASS);
    let mut control = ArmMetrics::default();
    let mut variant_metrics = ArmMetrics::default();
    let mut runs = Vec::new();
    for task in &tasks {
        for (arm, metrics) in [
            (ArmKind::Control, &mut control),
            (ArmKind::Variant, &mut variant_metrics),
        ] {
            let run = run_one_task(model, task, arm, variant)?;
            metrics.record_task_full(
                run.success,
                run.wall_ms,
                run.input_tokens,
                run.output_tokens,
                run.reasoning_tokens,
                run.tool_calls,
                run.cached_tokens,
                run.cost_ticks,
            );
            metrics.record_requests(&run.requests);
            runs.push(run);
        }
    }
    let comparison = AbComparison {
        control,
        variant: variant_metrics,
        model: model.to_string(),
    };
    let report_markdown = format_full_report(&comparison, &runs);
    Ok(AbReport {
        comparison,
        tasks: runs,
        report_markdown,
    })
}

/// Offline fixed-model technique matrix: A = all settings off; each B arm toggles
/// exactly one technique (plus an all-on arm) via [`arm_settings`] in a temp config.
pub fn run_technique_matrix(
    model: &str,
    fixtures: &[TaskFixture],
) -> Result<TechniqueMatrixReport, String> {
    let _class = crate::route::live::ClassScope::enter(AB_EVAL_CLASS);
    let mut arms = Vec::with_capacity(Technique::ALL.len());
    let mut baseline_metrics = None;
    for technique in Technique::ALL {
        let arm = run_technique_arm(model, fixtures, technique)?;
        if technique == Technique::Baseline {
            baseline_metrics = Some(arm.metrics.clone());
        }
        let baseline = baseline_metrics
            .as_ref()
            .expect("baseline is first in Technique::ALL");
        let delta = TechniqueDelta::between(baseline, &arm.metrics);
        let success_within_noise = success_within_noise(baseline, &arm.metrics);
        let recommendation = if technique == Technique::Baseline {
            Recommendation::Spike
        } else {
            recommend(baseline, &arm.metrics)
        };
        arms.push(TechniqueArm {
            technique,
            metrics: arm.metrics,
            tasks: arm.tasks,
            delta,
            success_within_noise,
            recommendation,
        });
    }
    Ok(TechniqueMatrixReport {
        model: model.to_string(),
        arms,
    })
}

struct TechniqueArmRaw {
    metrics: ArmMetrics,
    tasks: Vec<TaskRun>,
}

fn run_technique_arm(
    model: &str,
    fixtures: &[TaskFixture],
    technique: Technique,
) -> Result<TechniqueArmRaw, String> {
    let cfg = scratch(&format!("tech-{}", technique.id()))?;
    arm_settings(technique, &cfg)?;
    let _guard = ConfigGuard::set(&cfg);
    // Clear test overrides so on-disk arm_settings win.
    #[cfg(test)]
    crate::prefix_cache::set_enabled_for_tests(None);
    crate::compact::set_tool_output_compact_for_tests(None);
    let mut metrics = ArmMetrics::default();
    let mut tasks = Vec::with_capacity(fixtures.len());
    for task in fixtures {
        let run = run_one_task(model, task, ArmKind::Control, &BaselineSystem)?;
        metrics.record_task_full(
            run.success,
            0, // wall omitted from matrix aggregates for deterministic reports
            run.input_tokens,
            run.output_tokens,
            run.reasoning_tokens,
            run.tool_calls,
            run.cached_tokens,
            run.cost_ticks,
        );
        metrics.record_requests(&run.requests);
        tasks.push(run);
    }
    let _ = std::fs::remove_dir_all(&cfg);
    Ok(TechniqueArmRaw { metrics, tasks })
}

/// BaselineSystem also implements VariantSystem so technique arms can reuse
/// [`run_one_task`] with the production control prompt.
impl VariantSystem for BaselineSystem {
    fn system(&self, workspace: &Path) -> String {
        BaselineSystem::system(workspace)
    }
}

fn format_full_report(cmp: &AbComparison, runs: &[TaskRun]) -> String {
    use std::fmt::Write as _;
    let mut out = format_report(cmp);
    out.push_str("## Tasks\n\n");
    for run in runs {
        let arm = arm_label(run.arm);
        let status = if run.success { "ok" } else { "fail" };
        let tokens = run
            .input_tokens
            .saturating_add(run.output_tokens)
            .saturating_add(run.reasoning_tokens);
        let _ = writeln!(
            out,
            "- {} {} {}: {} ({} ms, {} tokens, {} tool calls)",
            run.task_id, arm, status, run.detail, run.wall_ms, tokens, run.tool_calls
        );
    }
    out
}

fn run_one_task(
    model: &str,
    task: &TaskFixture,
    arm: ArmKind,
    variant: &dyn VariantSystem,
) -> Result<TaskRun, String> {
    let dir = scratch(&task.id)?;
    seed_files(&dir, &task.files)?;
    let system = match arm {
        ArmKind::Control => BaselineSystem::system(&dir),
        ArmKind::Variant => variant.system(&dir),
    };
    let mode = perm_mode(&task.mode)?;
    let client = Arc::new(ScriptedTask::new(task.script.clone(), &dir));
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let session = format!("eval-ab-{}-{}", task.id, arm_label(arm));
    drop_session_file(&session);
    let start = Instant::now();
    let prompts = std::iter::once(&task.prompt).chain(&task.followups);
    let mut done = None;
    let mut output_tokens = 0u64;
    let mut reasoning_tokens = 0u64;
    for (turn, prompt) in prompts.enumerate() {
        let run = run_unattended(UnattendedRun {
            client: shared.clone(),
            workspace: dir.clone(),
            model: model.to_string(),
            effort: Some("high".into()),
            system: system.clone(),
            session_id: session.clone(),
            auth_kind: AuthKind::ApiKey,
            prompt: prompt.clone(),
            image: None,
            mode,
            readonly_session: false,
            desktop: false,
            cancel: CancelToken::new(),
            halt: Box::new(NoHalt),
            desktop_ops: None,
            imagine_bearer: String::new(),
            resume: turn > 0,
        });
        // A resumed turn reports the session's running total; keep the last one.
        output_tokens = run.usage.output_tokens;
        reasoning_tokens = run.usage.reasoning_tokens;
        done = Some(run);
    }
    let done = done.expect("a task has at least one prompt");
    let wall_ms = start.elapsed().as_millis();
    crate::tasks::forget_session(&session);
    drop_session_file(&session);
    let (success, detail) = evaluate_checks(&task.checks, &done, &dir);
    let tool_calls = client.tool_calls();
    // Input and cache come from the captured requests, not the fixture's numbers.
    let requests = derive_usage(&client.requests());
    let cost_ticks = offline_cost_ticks(
        requests.input_tokens,
        output_tokens,
        reasoning_tokens,
        requests.cached_tokens,
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(TaskRun {
        task_id: task.id.clone(),
        arm,
        success,
        detail,
        wall_ms,
        input_tokens: requests.input_tokens,
        output_tokens,
        reasoning_tokens,
        tool_calls,
        cached_tokens: requests.cached_tokens,
        cost_ticks,
        requests,
    })
}

/// Remove the saved transcript so a later arm starts fresh (follow-up turns resume it).
fn drop_session_file(session: &str) {
    if let Ok(path) = crate::session::session_file(session) {
        let _ = std::fs::remove_file(path);
    }
}

fn arm_label(arm: ArmKind) -> &'static str {
    match arm {
        ArmKind::Control => "control",
        ArmKind::Variant => "variant",
    }
}

fn perm_mode(raw: &str) -> Result<PermMode, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "always" => Ok(PermMode::Always),
        "ask" => Ok(PermMode::Ask),
        "auto" => Ok(PermMode::Auto),
        other => Err(format!("unknown perm mode: {other}")),
    }
}

fn seed_files(dir: &Path, files: &[FileSeed]) -> Result<(), String> {
    for file in files {
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        std::fs::write(&path, &file.content).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn evaluate_checks(
    checks: &[TaskCheck],
    done: &crate::UnattendedDone,
    dir: &Path,
) -> (bool, String) {
    let mut ok = true;
    let mut parts = Vec::new();
    for check in checks {
        let (pass, msg) = match check {
            TaskCheck::StopReason { value } => {
                let pass = done.stop_reason == *value;
                (
                    pass,
                    format!("stop_reason={} want={}", done.stop_reason, value),
                )
            }
            TaskCheck::AssistantContains { value } => {
                let pass = done.text.contains(value);
                (pass, format!("assistant contains {:?}", value))
            }
            TaskCheck::NoPermissionCards => {
                let pass = done.permission_cards == 0;
                (pass, format!("permission_cards={}", done.permission_cards))
            }
            TaskCheck::FileExists { path } => {
                let pass = dir.join(path).is_file();
                (pass, format!("file exists {path}"))
            }
            TaskCheck::FileContains { path, value } => {
                let pass = std::fs::read_to_string(dir.join(path))
                    .map(|text| text.contains(value))
                    .unwrap_or(false);
                (pass, format!("file {path} contains {:?}", value))
            }
        };
        if !pass {
            ok = false;
        }
        parts.push(format!("{}: {}", if pass { "ok" } else { "fail" }, msg));
    }
    (ok, parts.join("; "))
}

fn scratch(label: &str) -> Result<PathBuf, String> {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("gh-eval-ab-{}-{}-{}", label, std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
}

struct NoHalt;

impl HaltCheck for NoHalt {
    fn halted(&self) -> bool {
        false
    }
}

struct ScriptedTask {
    steps: Mutex<Vec<ScriptStep>>,
    turns: AtomicUsize,
    tool_calls: AtomicUsize,
    requests: Mutex<Vec<CapturedRequest>>,
    workspace: PathBuf,
}

impl ScriptedTask {
    fn new(steps: Vec<ScriptStep>, workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            steps: Mutex::new(steps),
            turns: AtomicUsize::new(0),
            tool_calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn tool_calls(&self) -> u64 {
        u64::try_from(self.tool_calls.load(Ordering::SeqCst)).unwrap_or(u64::MAX)
    }

    fn requests(&self) -> Vec<CapturedRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }
}

/// Render a request in prompt-cache order: leading system messages, the tools
/// array, then the rest of the input. Uses the real wire body, with the scratch
/// workspace path replaced and line endings folded to LF, so two runs (and two
/// platforms) render the same bytes.
pub fn capture_request(req: &ResponsesRequest, workspace: &Path) -> CapturedRequest {
    let mut body = crate::responses_body(req);
    scrub_workspace(&mut body, &workspace.display().to_string());
    let tools = serde_json::to_string(&body["tools"]).unwrap_or_default();
    let input = body["input"].as_array().cloned().unwrap_or_default();
    let lead = input
        .iter()
        .take_while(|item| item["role"] == "system")
        .count();
    let mut parts = Vec::with_capacity(input.len() + 1);
    for item in &input[..lead] {
        parts.push(item.to_string());
    }
    parts.push(tools.clone());
    for item in &input[lead..] {
        parts.push(item.to_string());
    }
    CapturedRequest {
        body: parts.join("\n"),
        schema_bytes: tools.len() as u64,
    }
}

const WORKSPACE_MARK: &str = "<workspace>";

fn scrub_workspace(value: &mut Value, workspace: &str) {
    match value {
        Value::String(text) if text.contains(workspace) || text.contains('\r') => {
            *text = text
                .replace(workspace, WORKSPACE_MARK)
                .replace("\r\n", "\n");
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| scrub_workspace(item, workspace)),
        Value::Object(map) => map
            .values_mut()
            .for_each(|item| scrub_workspace(item, workspace)),
        _ => {}
    }
}

impl ModelClient for ScriptedTask {
    fn stream(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        if cancel.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        self.turns.fetch_add(1, Ordering::SeqCst);
        self.requests
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(capture_request(req, &self.workspace));
        let step = {
            let mut steps = self.steps.lock().unwrap_or_else(|err| err.into_inner());
            if steps.is_empty() {
                ScriptStep::Text {
                    content: "done".into(),
                    usage: StepUsage::default(),
                }
            } else {
                steps.remove(0)
            }
        };
        match step {
            ScriptStep::Text { content, usage } => {
                sink(StreamEvent::TextDelta(content.clone()));
                Ok(TurnOutput {
                    text: content,
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage_from(&usage),
                    ..Default::default()
                })
            }
            ScriptStep::Tool {
                name,
                arguments,
                usage,
            } => {
                self.tool_calls.fetch_add(1, Ordering::SeqCst);
                Ok(TurnOutput {
                    text: String::new(),
                    reasoning: String::new(),
                    calls: vec![FunctionCall {
                        call_id: "eval".into(),
                        name,
                        arguments: arguments.to_string(),
                    }],
                    usage: usage_from(&usage),
                    ..Default::default()
                })
            }
            ScriptStep::Tools { calls, usage } => {
                self.tool_calls.fetch_add(calls.len(), Ordering::SeqCst);
                Ok(TurnOutput {
                    text: String::new(),
                    reasoning: String::new(),
                    calls: calls
                        .into_iter()
                        .enumerate()
                        .map(|(i, call)| FunctionCall {
                            call_id: format!("eval-{i}"),
                            name: call.name,
                            arguments: call.arguments.to_string(),
                        })
                        .collect(),
                    usage: usage_from(&usage),
                    ..Default::default()
                })
            }
        }
    }
}

fn usage_from(step: &StepUsage) -> Usage {
    let cached_tokens = step.cached_tokens;
    let cost_in_usd_ticks = step.cost_in_usd_ticks.unwrap_or_else(|| {
        offline_cost_ticks(
            step.input_tokens,
            step.output_tokens,
            step.reasoning_tokens,
            cached_tokens,
        )
    });
    Usage {
        input_tokens: step.input_tokens,
        output_tokens: step.output_tokens,
        reasoning_tokens: step.reasoning_tokens,
        cost_in_usd_ticks,
        cached_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ab_harness_runs_fixture_set_and_passes_gate() {
        let report = run_ab(DEFAULT_MODEL, &CandidateSuffix).expect("ab run");
        let tasks = load_tasks().expect("fixtures");
        assert_eq!(report.comparison.control.tasks, tasks.len() as u32);
        assert_eq!(report.comparison.variant.tasks, tasks.len() as u32);
        assert!(
            report.comparison.control.success_rate() >= 1.0 - f64::EPSILON,
            "{}",
            report.report_markdown
        );
        assert!(
            report.comparison.variant.success_rate() >= 1.0 - f64::EPSILON,
            "{}",
            report.report_markdown
        );
        report.gate().expect("gate");
        assert!(report.report_markdown.contains("Agent prompt A/B eval"));
        assert_eq!(report.tasks.len(), tasks.len() * 2);
        assert!(report.tasks.iter().all(|run| !run.task_id.is_empty()));
    }

    #[test]
    fn fixture_count_is_about_twenty() {
        let tasks = load_tasks().expect("fixtures");
        assert!(
            (18..=24).contains(&tasks.len()),
            "expected ~20 tasks, got {}",
            tasks.len()
        );
    }

    #[test]
    fn capture_request_puts_system_then_tools_then_turns_and_scrubs_workspace() {
        let ws = Path::new("/tmp/gh-eval-ab-x-1-2");
        let msg = |role: &str, text: &str| crate::client::InputItem::Message {
            role: role.into(),
            content: vec![crate::client::ContentPart::InputText(text.into())],
        };
        let req = ResponsesRequest {
            model: DEFAULT_MODEL.into(),
            effort: None,
            input: vec![
                msg("system", "rules for /tmp/gh-eval-ab-x-1-2"),
                msg("user", "hi"),
            ],
            conversation_id: String::new(),
            tools: vec![serde_json::json!({"name": "read_file"})],
            hosted_search: false,
            call_timeout: None,
        };
        let got = capture_request(&req, ws);
        let lines: Vec<&str> = got.body.split('\n').collect();
        assert_eq!(lines.len(), 3, "{}", got.body);
        assert!(lines[0].contains("rules for <workspace>"), "{}", lines[0]);
        assert_eq!(lines[1], r#"[{"name":"read_file"}]"#);
        assert!(lines[2].contains(r#""text":"hi""#), "{}", lines[2]);
        assert_eq!(got.schema_bytes, 22);
        assert!(!got.body.contains("gh-eval-ab-x"));
    }

    #[test]
    fn arm_settings_toggles_exactly_one_file_per_b_arm() {
        let dir = scratch("arm-settings").expect("scratch");
        arm_settings(Technique::PrefixCache, &dir).expect("write");
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).expect("read");
        assert!(read("prefix_cache.toml").contains("enabled = true"));
        assert!(read(TOOL_OUTPUT_COMPACT_CONFIG).contains("enabled = false"));
        assert!(read("deferred_tools.toml").contains("enabled = false"));
        assert!(read("batched_tools.toml").contains("enabled = false"));
        arm_settings(Technique::AllOn, &dir).expect("all");
        assert!(read("prefix_cache.toml").contains("enabled = true"));
        assert!(read(TOOL_OUTPUT_COMPACT_CONFIG).contains("enabled = true"));
        assert!(read("deferred_tools.toml").contains("enabled = true"));
        assert!(read("batched_tools.toml").contains("enabled = true"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
