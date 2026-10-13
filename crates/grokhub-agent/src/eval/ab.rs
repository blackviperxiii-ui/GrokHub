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

use super::metrics::{format_report, gate, AbComparison, AbGateFailure, ArmMetrics};

pub const EVAL_TASKS_DIR: &str = "tests/fixtures/eval_tasks";
pub const DEFAULT_MODEL: &str = "grok-4.7";
/// Route class for A/B task runs (shadow routing only).
pub const AB_EVAL_CLASS: &str = "eval:ab";

#[derive(Debug, Clone, Deserialize)]
pub struct TaskFixture {
    pub id: String,
    pub prompt: String,
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
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StepUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
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
            metrics.record_task(
                run.success,
                run.wall_ms,
                run.input_tokens,
                run.output_tokens,
                run.reasoning_tokens,
                run.tool_calls,
            );
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
    let client = Arc::new(ScriptedTask::new(task.script.clone()));
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let session = format!("eval-ab-{}-{}", task.id, arm_label(arm));
    let start = Instant::now();
    let done = run_unattended(UnattendedRun {
        client: shared,
        workspace: dir.clone(),
        model: model.to_string(),
        effort: Some("high".into()),
        system,
        session_id: session.clone(),
        auth_kind: AuthKind::ApiKey,
        prompt: task.prompt.clone(),
        image: None,
        mode,
        readonly_session: false,
        desktop: false,
        cancel: CancelToken::new(),
        halt: Box::new(NoHalt),
        desktop_ops: None,
        imagine_bearer: String::new(),
        resume: false,
    });
    let wall_ms = start.elapsed().as_millis();
    crate::tasks::forget_session(&session);
    let (success, detail) = evaluate_checks(&task.checks, &done, &dir);
    let tool_calls = client.tool_calls();
    let usage = done.usage;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(TaskRun {
        task_id: task.id.clone(),
        arm,
        success,
        detail,
        wall_ms,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        tool_calls,
    })
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
}

impl ScriptedTask {
    fn new(steps: Vec<ScriptStep>) -> Self {
        Self {
            steps: Mutex::new(steps),
            turns: AtomicUsize::new(0),
            tool_calls: AtomicUsize::new(0),
        }
    }

    fn tool_calls(&self) -> u64 {
        u64::try_from(self.tool_calls.load(Ordering::SeqCst)).unwrap_or(u64::MAX)
    }
}

impl ModelClient for ScriptedTask {
    fn stream(
        &self,
        _req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        if cancel.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        self.turns.fetch_add(1, Ordering::SeqCst);
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
        }
    }
}

fn usage_from(step: &StepUsage) -> Usage {
    Usage {
        input_tokens: step.input_tokens,
        output_tokens: step.output_tokens,
        reasoning_tokens: step.reasoning_tokens,
        cost_in_usd_ticks: 0,
        cached_tokens: 0,
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
            (18..=22).contains(&tasks.len()),
            "expected ~20 tasks, got {}",
            tasks.len()
        );
    }
}
