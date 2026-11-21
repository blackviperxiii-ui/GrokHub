//! Runs a routine's steps with no model (script-first runtime, phase 2).
//!
//! Each `tool`, `script` and `desktop` step goes through the same gate as a
//! model's tool call ([`crate::gate::decide_with`]: the hard floor, hard
//! classes, the permission policy and Ask / unattended rules), then through
//! the native tool dispatch. `expect` steps are checked here. The model
//! wakes only to fill a missing input, pick an `ask` step's branch or repair
//! a failed step ([`crate::routine_wake`]); with no model those stop the run
//! with a plain message.
//!
//! A dry run fills and gates every step, top to bottom, and runs nothing.

use std::path::Path;

use grokhub_core::recipe::ComputerOp;
use grokhub_core::routine::{
    self, fill, fill_args, pieces, Checked, Piece, Quote, Ref, Routine, StepKind, StepOut, Values,
};
use serde_json::{json, Map, Value};

use crate::gate::{self, Decision, DeskFlags, Gate, PermMode};
use crate::routine_wake::{Brain, Repair};
use crate::tools::{self, DesktopOps, ToolCtx};

/// Most of a step's output kept in a report line.
const LINE_CHARS: usize = 400;

/// Who answers when the gate asks about a change step.
pub trait Approve {
    /// `true` runs the step once.
    fn approve(&self, step: &str, tool: &str, arguments: &str) -> bool;
}

/// Nobody is there: the gate's Ask becomes a refusal, as in an unattended
/// automation.
pub struct Nobody;

impl Approve for Nobody {
    fn approve(&self, _step: &str, _tool: &str, _arguments: &str) -> bool {
        false
    }
}

pub struct RunSpec<'a> {
    pub routine: &'a Routine,
    /// The routine's folder, where its scripts are.
    pub dir: &'a Path,
    /// Where commands run.
    pub workspace: &'a Path,
    /// Input values from the trigger; defaults fill the rest.
    pub inputs: Map<String, Value>,
    pub dry_run: bool,
    /// `None` runs unattended: change steps are refused.
    pub approve: Option<&'a dyn Approve>,
    /// The app's desktop control; `None` refuses desktop steps.
    pub desktop: Option<&'a dyn DesktopOps>,
    /// Halt: checked before each step.
    pub stop: &'a dyn Fn() -> bool,
    /// The model for fill, branch and repair; `None` stops at those points.
    pub brain: Option<&'a dyn Brain>,
    /// The words that started the run (a chat ask, a webhook's text), which
    /// a fill reads.
    pub words: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The last step finished; the result shows.
    Finished,
    /// A step jumped to `done`: nothing to show.
    Done { at: String },
    /// The run couldn't go on.
    Stopped { at: String, why: String },
    /// The routine can't do this job: a full assistant turn takes over.
    Replan { at: String, why: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// One line per step that ran (or, on a dry run, would run).
    pub lines: Vec<String>,
    pub ended: Ended,
    pub result: Option<String>,
    /// Model calls this run made: fills, branches and repairs.
    pub model_calls: u32,
    pub dry_run: bool,
}

impl Report {
    /// What the person reads.
    pub fn text(&self) -> String {
        let mut out = self.lines.join("\n");
        let tail = match &self.ended {
            Ended::Finished if self.dry_run => "Dry run: nothing ran.".to_string(),
            Ended::Finished => self.result.clone().unwrap_or_else(|| "Finished.".into()),
            Ended::Done { at } => format!("Stopped early at {at}, as planned. Nothing to report."),
            Ended::Stopped { at, why } => format!("Stopped at {at}: {why}"),
            Ended::Replan { at, why } => {
                format!("At {at} this routine couldn't do the job, so it needs a full assistant turn: {why}")
            }
        };
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&tail);
        out
    }
}

/// Why a step didn't finish.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Fail {
    /// The gate, the person or the setup said no. Never repaired.
    Refused(String),
    /// The step broke. The model may repair it.
    Broke(String),
}

/// Where the run goes after a step.
enum Goto {
    Next,
    Step(usize),
    Done,
}

/// A step that finished: its outputs, its report line, and where next.
struct Went {
    out: StepOut,
    line: String,
    goto: Goto,
}

/// Runs (or dry-runs) `spec.routine`.
pub fn run(spec: &RunSpec<'_>) -> Report {
    let mut report = Report {
        lines: Vec::new(),
        ended: Ended::Finished,
        result: None,
        model_calls: 0,
        dry_run: spec.dry_run,
    };
    let mut values = Values::default();
    let missing = match inputs(spec.routine, &spec.inputs) {
        Ok((given, missing)) => {
            values.inputs = given;
            missing
        }
        Err(why) => {
            report.ended = Ended::Stopped {
                at: "inputs".into(),
                why,
            };
            return report;
        }
    };
    if !missing.is_empty() {
        let names = missing
            .iter()
            .map(|m| format!("inputs.{m}"))
            .collect::<Vec<_>>()
            .join(" and ");
        let words = spec.words.map(str::trim).filter(|w| !w.is_empty());
        match (spec.brain, words) {
            _ if spec.dry_run => report.lines.push(format!(
                "inputs: {names} would be filled from the words that start the run"
            )),
            (Some(brain), Some(words)) => {
                report.model_calls += 1;
                match crate::routine_wake::fill(brain, spec.routine, &missing, words) {
                    Ok(filled) => {
                        report
                            .lines
                            .push(format!("inputs: the model filled {names}"));
                        values.inputs.extend(filled);
                    }
                    Err(why) => {
                        report.ended = Ended::Stopped {
                            at: "inputs".into(),
                            why,
                        };
                        return report;
                    }
                }
            }
            _ => {
                report.ended = Ended::Stopped {
                    at: "inputs".into(),
                    why: format!("it needs a value for {names}"),
                };
                return report;
            }
        }
    }
    let gate = Gate {
        mode: PermMode::Ask,
        readonly_session: false,
        attended: spec.approve.is_some(),
        desktop: spec.desktop.is_some(),
    };
    let policy = crate::perm::Policy::load(spec.workspace);
    let runner = Runner {
        spec,
        gate,
        policy: &policy,
    };
    let steps = &spec.routine.steps;
    let mut repaired = std::collections::BTreeSet::new();
    let mut retry: Option<Map<String, Value>> = None;
    let mut i = 0;
    while i < steps.len() {
        let step = &steps[i];
        let stopped = |why: String| Ended::Stopped {
            at: step.id.clone(),
            why,
        };
        if (spec.stop)() {
            report.ended = stopped("halted".into());
            return report;
        }
        if spec.dry_run {
            report.lines.push(format!(
                "{}: {}",
                step.id,
                runner.preview(&step.kind, &values)
            ));
            i += 1;
            continue;
        }
        let (went, calls) = runner.step(i, &values, retry.take());
        report.model_calls += calls;
        match went {
            Ok(Went { out, line, goto }) => {
                report.lines.push(format!("{}: {line}", step.id));
                values.steps.insert(step.id.clone(), out);
                match goto {
                    Goto::Next => i += 1,
                    Goto::Step(j) => i = j,
                    Goto::Done => {
                        report.ended = Ended::Done {
                            at: step.id.clone(),
                        };
                        return report;
                    }
                }
            }
            Err(Fail::Refused(why)) => {
                report.ended = stopped(why);
                return report;
            }
            Err(Fail::Broke(why)) => {
                let brain = spec.brain.filter(|_| {
                    !repaired.contains(&i)
                        && u32::try_from(repaired.len()).unwrap_or(u32::MAX)
                            < crate::routine_wake::MAX_REPAIRS
                });
                let Some(brain) = brain else {
                    let after = if repaired.contains(&i) {
                        " (it still failed after one fix)"
                    } else {
                        ""
                    };
                    report.ended = stopped(format!("{why}{after}"));
                    return report;
                };
                repaired.insert(i);
                report.model_calls += 1;
                match crate::routine_wake::repair(brain, spec.routine, step, &why, &values) {
                    Ok((Repair::Retry(args), fix)) => {
                        report
                            .lines
                            .push(format!("{}: {why}; trying again: {fix}", step.id));
                        retry = Some(args);
                    }
                    Ok((Repair::Skip, fix)) => {
                        report
                            .lines
                            .push(format!("{}: {why}; skipped: {fix}", step.id));
                        i += 1;
                    }
                    Ok((Repair::Stop, fix)) => {
                        report.ended = stopped(format!("{why}. {fix}"));
                        return report;
                    }
                    Ok((Repair::Replan, fix)) => {
                        report.ended = Ended::Replan {
                            at: step.id.clone(),
                            why: fix,
                        };
                        return report;
                    }
                    Err(e) => {
                        report.ended = stopped(format!("{why} ({e})"));
                        return report;
                    }
                }
            }
        }
    }
    if !spec.dry_run
        && let Some(template) = &spec.routine.result
    {
        report.result = Some(
            fill(template, &values, Quote::Plain)
                .unwrap_or_else(|e| format!("(the result couldn't be filled: {e})")),
        );
    }
    report
}

/// The trigger's values, type-checked, plus defaults, and the required
/// inputs still without a value.
fn inputs(
    routine: &Routine,
    given: &Map<String, Value>,
) -> Result<(Map<String, Value>, Vec<String>), String> {
    let mut out = Map::new();
    for (name, v) in given {
        let Some(input) = routine.inputs.get(name) else {
            return Err(format!("this routine has no input named {name}"));
        };
        if !input.kind.accepts(v) {
            return Err(format!(
                "inputs.{name} must be a {}, not {v}",
                input.kind.name()
            ));
        }
        out.insert(name.clone(), v.clone());
    }
    let mut missing = Vec::new();
    for (name, input) in &routine.inputs {
        if out.contains_key(name) {
            continue;
        }
        match &input.default {
            Some(d) => {
                out.insert(name.clone(), d.clone());
            }
            None => missing.push(name.clone()),
        }
    }
    Ok((out, missing))
}

fn out_value(v: String) -> StepOut {
    StepOut {
        value: Some(v),
        ..StepOut::default()
    }
}

fn clip(s: &str) -> String {
    if s.chars().count() <= LINE_CHARS {
        return s.to_string();
    }
    let cut: String = s.chars().take(LINE_CHARS).collect();
    format!("{cut}…")
}

struct Runner<'a> {
    spec: &'a RunSpec<'a>,
    gate: Gate,
    policy: &'a crate::perm::Policy,
}

impl Runner<'_> {
    fn desk(&self) -> Option<DeskFlags> {
        self.spec.desktop.map(|d| DeskFlags {
            halted: d.halted(),
            locked: d.locked(),
        })
    }

    /// Runs step `i`. `retry` holds new arguments from a repair. Returns the
    /// model calls it made (a branch).
    fn step(
        &self,
        i: usize,
        values: &Values,
        retry: Option<Map<String, Value>>,
    ) -> (Result<Went, Fail>, u32) {
        let steps = &self.spec.routine.steps;
        let step = &steps[i];
        let ran = |out: Result<StepOut, Fail>| {
            out.map(|out| Went {
                line: clip(out.stdout.trim()),
                out,
                goto: Goto::Next,
            })
        };
        let goto = |target: &str| {
            if target == routine::DONE {
                Goto::Done
            } else {
                steps
                    .iter()
                    .position(|s| s.id == target)
                    .map_or(Goto::Done, Goto::Step)
            }
        };
        let went = match &step.kind {
            StepKind::Tool { tool, args } => ran(match retry {
                Some(args) => self.call(&step.id, tool, &Value::Object(args).to_string()),
                None => fill_args(tool, args, values, Quote::shell())
                    .map_err(Fail::Broke)
                    .and_then(|filled| {
                        self.call(&step.id, tool, &Value::Object(filled).to_string())
                    }),
            }),
            StepKind::Script { file, args } => {
                let args = retry.as_ref().unwrap_or(args);
                ran(self
                    .script_call(file, args, values)
                    .map_err(Fail::Broke)
                    .and_then(|arguments| self.call(&step.id, tools::ptc::NAME, &arguments)))
            }
            StepKind::Desktop(recipe) => ran(self.desktop(&step.id, &recipe.ops)),
            StepKind::Expect { expect, otherwise } => match expect.check(values) {
                Ok(Checked::Pass(v)) => Ok(Went {
                    line: format!("{} is {v}, as expected", expect.from),
                    out: out_value(v),
                    goto: Goto::Next,
                }),
                Ok(Checked::Miss(v)) => match otherwise {
                    Some(target) => Ok(Went {
                        line: format!("{} is {v}, so on to {target}", expect.from),
                        out: out_value(v),
                        goto: goto(target),
                    }),
                    None => Err(Fail::Broke(format!(
                        "{} is {v}, which isn't what this step expects",
                        expect.from
                    ))),
                },
                Err(e) => Err(Fail::Broke(e)),
            },
            StepKind::Ask { question, branches } => {
                let Some(brain) = self.spec.brain else {
                    return (
                        Err(Fail::Refused(
                            "this step needs the model to choose, and this run has none".into(),
                        )),
                        0,
                    );
                };
                let names: Vec<&str> = branches.keys().map(String::as_str).collect();
                let picked = crate::routine_wake::branch(brain, question, &names, values)
                    .map(|(name, why)| Went {
                        line: format!("the model chose {name}: {why}"),
                        goto: goto(&branches[&name]),
                        out: out_value(name),
                    })
                    .map_err(Fail::Refused);
                return (picked, 1);
            }
        };
        (went, 0)
    }

    /// One gated call through the native dispatch.
    fn call(&self, step: &str, tool: &str, arguments: &str) -> Result<StepOut, Fail> {
        self.cleared(step, tool, arguments).map_err(Fail::Refused)?;
        self.dispatch(tool, arguments, self.next_checks_exit(step))
            .map_err(Fail::Broke)
    }

    /// The gate's answer for one call, with the person's answer for an Ask.
    fn cleared(&self, step: &str, tool: &str, arguments: &str) -> Result<(), String> {
        let decision = gate::decide_with(
            &self.gate,
            tool,
            arguments,
            false,
            self.desk(),
            self.spec.workspace,
            Some(self.policy),
        );
        match decision {
            Decision::Run => Ok(()),
            Decision::Ask => match self.spec.approve {
                Some(a) if a.approve(step, tool, arguments) => Ok(()),
                Some(_) => Err(format!("you didn't allow {tool}")),
                None => Err(gate::unattended_deny(tool)),
            },
            Decision::Refuse(why) => Err(why),
        }
    }

    /// Runs one call. A command's `exit N` first line becomes its exit
    /// code. A non-zero exit fails the step unless `exit_ok` (the next step
    /// checks this step's exit code itself).
    fn dispatch(&self, tool: &str, arguments: &str, exit_ok: bool) -> Result<StepOut, String> {
        let ctx = ToolCtx {
            workspace: self.spec.workspace,
            desktop: None,
            stop: self.spec.stop,
            tasks: None,
            owner: None,
            session: None,
        };
        let out = tools::dispatch(&ctx, tool, arguments);
        let text = grokhub_core::redact_secrets(&out.text);
        let exit = exit_line(tool, &text);
        if out.failed && !(exit_ok && exit.is_some()) {
            return Err(format!("{tool} failed: {}", clip(text.trim())));
        }
        Ok(match exit {
            Some((code, rest)) => StepOut {
                stdout: rest.to_string(),
                exit_code: Some(code),
                ..StepOut::default()
            },
            None => StepOut {
                stdout: text,
                exit_code: Some(0),
                ..StepOut::default()
            },
        })
    }

    /// Whether the step after `step` is an `expect` on `step.exit_code`.
    fn next_checks_exit(&self, step: &str) -> bool {
        let steps = &self.spec.routine.steps;
        let wanted = format!("{step}.exit_code");
        steps
            .iter()
            .position(|s| s.id == step)
            .and_then(|i| steps.get(i + 1))
            .is_some_and(|next| matches!(&next.kind, StepKind::Expect { expect, .. } if expect.from.trim() == wanted))
    }

    /// The script's file, with its filled `args` as `ARGS`, as the body of
    /// the sandboxed `run_tool_script` call.
    fn script_call(
        &self,
        file: &str,
        args: &Map<String, Value>,
        values: &Values,
    ) -> Result<String, String> {
        let body = std::fs::read_to_string(self.spec.dir.join(file))
            .map_err(|e| format!("couldn't read {file}: {e}"))?;
        let filled = Value::Object(fill_args("", args, values, Quote::Plain)?).to_string();
        let encoded = serde_json::to_string(&filled).unwrap_or_else(|_| "\"{}\"".into());
        let script = format!("import json as _json\nARGS = _json.loads({encoded})\n{body}");
        Ok(json!({ "script": script }).to_string())
    }

    fn desktop(&self, step: &str, ops: &[ComputerOp]) -> Result<StepOut, Fail> {
        let Some(desk) = self.spec.desktop else {
            return Err(Fail::Refused(
                "desktop steps need the app's desktop control".into(),
            ));
        };
        let mut said = Vec::new();
        for op in ops {
            let (tool, args) = desktop_call(op).map_err(Fail::Broke)?;
            let arguments = args.to_string();
            self.cleared(step, tool, &arguments)
                .map_err(Fail::Refused)?;
            let out = desk.call(tool, &args);
            if out.failed {
                return Err(Fail::Broke(format!(
                    "{tool} failed: {}",
                    clip(out.text.trim())
                )));
            }
            said.push(out.text);
        }
        Ok(StepOut {
            stdout: said.join("\n"),
            exit_code: Some(0),
            ..StepOut::default()
        })
    }

    /// A dry run's line: what the step would do, filled, and what the gate
    /// says. Outputs of earlier steps don't exist yet, so they stay as
    /// `{{step.field}}`.
    fn preview(&self, kind: &StepKind, values: &Values) -> String {
        let gated = |tool: &str, arguments: &str| match gate::decide_with(
            &self.gate,
            tool,
            arguments,
            false,
            self.desk(),
            self.spec.workspace,
            Some(self.policy),
        ) {
            Decision::Run => "runs".to_string(),
            Decision::Ask if self.spec.approve.is_some() => "asks you first".to_string(),
            Decision::Ask => format!("refused: {}", gate::unattended_deny(tool)),
            Decision::Refuse(why) => format!("refused: {why}"),
        };
        match kind {
            StepKind::Tool { tool, args } => {
                let filled = Value::Object(
                    args.iter()
                        .map(|(k, v)| {
                            let quote =
                                if routine::COMMAND_ARGS.contains(&(tool.as_str(), k.as_str())) {
                                    Quote::shell()
                                } else {
                                    Quote::Plain
                                };
                            (k.clone(), preview_value(v, values, quote))
                        })
                        .collect(),
                );
                let arguments = filled.to_string();
                format!("{tool} {arguments} ({})", gated(tool, &arguments))
            }
            StepKind::Script { file, args } => {
                let args: Map<String, Value> = args
                    .iter()
                    .map(|(k, v)| (k.clone(), preview_value(v, values, Quote::Plain)))
                    .collect();
                match self.script_call(
                    file,
                    &args,
                    &Values {
                        inputs: values.inputs.clone(),
                        ..Values::default()
                    },
                ) {
                    Ok(arguments) => format!(
                        "script {file} {} ({})",
                        Value::Object(args),
                        gated(tools::ptc::NAME, &arguments)
                    ),
                    Err(e) => format!("script {file}: {e}"),
                }
            }
            StepKind::Desktop(recipe) => {
                let first = recipe.ops.first().map(desktop_call);
                match (self.spec.desktop, first) {
                    (None, _) => format!(
                        "{} desktop steps (refused: desktop steps need the app's desktop control)",
                        recipe.ops.len()
                    ),
                    (Some(_), Some(Ok((tool, args)))) => {
                        format!(
                            "{} desktop steps, first {tool} ({})",
                            recipe.ops.len(),
                            gated(tool, &args.to_string())
                        )
                    }
                    (Some(_), Some(Err(e))) => format!("desktop: {e}"),
                    (Some(_), None) => "no desktop steps".into(),
                }
            }
            StepKind::Expect { expect, otherwise } => format!(
                "checks {}; on a miss {}",
                expect.from,
                otherwise
                    .as_deref()
                    .map_or("the run stops".to_string(), |t| format!("goes to {t}"))
            ),
            StepKind::Ask { question, branches } => format!(
                "asks \"{question}\" ({}) (refused: routines can't ask the model yet)",
                branches.keys().cloned().collect::<Vec<_>>().join(" / ")
            ),
        }
    }
}

/// `exit N` on the first line of a command's reply: the code and the rest.
fn exit_line<'t>(tool: &str, text: &'t str) -> Option<(i64, &'t str)> {
    if !gate::is_shell(tool) {
        return None;
    }
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let code = first.trim().strip_prefix("exit ")?.parse().ok()?;
    Some((code, rest))
}

/// A template with inputs filled and step outputs left as `{{ref}}`.
fn preview_text(template: &str, values: &Values, quote: Quote) -> String {
    let Ok(list) = pieces(template) else {
        return template.to_string();
    };
    list.into_iter()
        .map(|p| match p {
            Piece::Text(t) => t.to_string(),
            Piece::Ref(r @ Ref::Input(_)) => values
                .get(&r)
                .map(|v| quote.apply(&v))
                .unwrap_or_else(|_| format!("{{{{{r}}}}}")),
            Piece::Ref(r) => format!("{{{{{r}}}}}"),
        })
        .collect()
}

fn preview_value(v: &Value, values: &Values, quote: Quote) -> Value {
    match v {
        Value::String(s) => Value::String(preview_text(s, values, quote)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|i| preview_value(i, values, quote))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, i)| (k.clone(), preview_value(i, values, quote)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The desktop tool call for one saved op.
fn desktop_call(op: &ComputerOp) -> Result<(&'static str, Value), String> {
    Ok(match op {
        ComputerOp::Click { x, y } => ("click", json!({ "x": x, "y": y })),
        ComputerOp::DoubleClick { x, y } => ("click", json!({ "x": x, "y": y, "double": true })),
        ComputerOp::Move { x, y } => ("move", json!({ "x": x, "y": y })),
        ComputerOp::Type { text } => ("type", json!({ "text": text })),
        ComputerOp::Key { name } => ("key", json!({ "keys": name })),
        ComputerOp::Scroll { dy } => ("scroll", json!({ "dy": dy })),
        other => return Err(format!("a saved {other:?} can't replay in a routine yet")),
    })
}

/// `routine run <name> [--dry-run] [--set key=value]…` from a terminal, in
/// the current folder. Change steps ask on the terminal. Returns the exit
/// code: 0 finished or done, 1 stopped, 2 the routine couldn't load.
pub fn run_cli(args: &[String], config_dir: &Path) -> i32 {
    let mut name = None;
    let mut dry_run = false;
    let mut sets = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dry-run" => dry_run = true,
            "--set" => sets.extend(it.next().cloned()),
            other if name.is_none() && !other.starts_with('-') => name = Some(other.to_string()),
            other => {
                eprintln!("routine run: unknown argument {other}");
                return 2;
            }
        }
    }
    let Some(name) = name else {
        eprintln!("usage: grokhub-hub routine run <name> [--dry-run] [--set key=value]…");
        return 2;
    };
    let dir = routine::routines_dir(config_dir).join(&name);
    let routine = match Routine::load(&dir, &gate::is_known) {
        Ok(r) => r,
        Err(problems) => {
            eprintln!("{} can't run:\n- {}", dir.display(), problems.join("\n- "));
            return 2;
        }
    };
    let inputs = match parse_sets(&routine, &sets) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let workspace = std::env::current_dir().unwrap_or_else(|_| config_dir.to_path_buf());
    let stop = || false;
    let report = run(&RunSpec {
        routine: &routine,
        dir: &dir,
        workspace: &workspace,
        inputs,
        dry_run,
        approve: Some(&Terminal),
        desktop: None,
        stop: &stop,
        brain: None,
        words: None,
    });
    println!("{}", report.text());
    match report.ended {
        Ended::Stopped { .. } => 1,
        _ => 0,
    }
}

/// `--set key=value`, typed by the routine's input kinds.
fn parse_sets(routine: &Routine, sets: &[String]) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for s in sets {
        let (key, raw) = s
            .split_once('=')
            .ok_or_else(|| format!("--set {s}: use key=value"))?;
        let input = routine
            .inputs
            .get(key)
            .ok_or_else(|| format!("--set {s}: this routine has no input named {key}"))?;
        let v = match input.kind {
            routine::InputKind::String => Value::String(raw.into()),
            _ => serde_json::from_str(raw)
                .map_err(|_| format!("--set {s}: {key} must be a {}", input.kind.name()))?,
        };
        out.insert(key.into(), v);
    }
    Ok(out)
}

/// Asks on the terminal before a change step runs.
struct Terminal;

impl Approve for Terminal {
    fn approve(&self, step: &str, tool: &str, arguments: &str) -> bool {
        use std::io::Write;
        eprint!("{step}: run {tool} {arguments}? [y/N] ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes")
    }
}

#[cfg(test)]
#[path = "routine_run_tests.rs"]
mod tests;
