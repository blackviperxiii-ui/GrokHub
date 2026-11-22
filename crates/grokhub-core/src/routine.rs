//! Routines: saved steps that run without the model (script-first runtime,
//! phase 1). This module is the format only: types, the parser, the
//! validator and template fill. Nothing here runs a step.
//!
//! A routine lives in `{config}/routines/<name>/routine.json`, with its
//! scripts next to the file. Steps run top to bottom, and a jump only goes
//! forward, so there are no loops and a routine reads top to bottom. The
//! design is in `docs/design/script-first-runtime/README.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::recipe::Recipe;

#[path = "routine_command.rs"]
mod command;

/// The one format version this build reads.
pub const VERSION: u32 = 1;
/// The routine's file inside its folder.
pub const FILE: &str = "routine.json";
/// The jump target that ends a run early, with no result.
pub const DONE: &str = "done";
/// Most steps in one routine.
pub const MAX_STEPS: usize = 64;
/// Largest `routine.json` read.
pub const MAX_BYTES: usize = 256 * 1024;
/// Tool arguments that are command lines. A value filled into one is quoted
/// as one shell argument; the rest of the command is fixed when saved.
pub const COMMAND_ARGS: &[(&str, &str)] = &[("run_terminal_command", "command")];

/// A saved list of steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Routine {
    pub version: u32,
    /// The folder name: lowercase letters, digits and dashes.
    pub name: String,
    /// What it does, in the person's words. The model reads this to pick it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<Trigger>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Input>,
    pub steps: Vec<Step>,
    /// The run's answer, shown when the last step finishes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// What starts a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Timer(Timer),
    /// A signed webhook, by the hook's name.
    Webhook(String),
    /// A file in `folder` whose name matches `glob`.
    File {
        folder: String,
        glob: String,
    },
    /// The `run_routine` chat tool.
    Chat,
}

/// A clock trigger: `daily 09:00`, `weekdays 07:30` or `every 15m` / `every 2h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Timer {
    Daily { at_min: u32 },
    Weekdays { at_min: u32 },
    Every { minutes: u32 },
}

impl TryFrom<String> for Timer {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        let bad = || {
            format!("timer \"{text}\": use \"daily 09:00\", \"weekdays 09:00\" or \"every 15m\"")
        };
        let (word, rest) = text.trim().split_once(' ').ok_or_else(bad)?;
        let rest = rest.trim();
        match word {
            "daily" | "weekdays" => {
                let at_min = clock_min(rest).ok_or_else(bad)?;
                Ok(if word == "daily" {
                    Timer::Daily { at_min }
                } else {
                    Timer::Weekdays { at_min }
                })
            }
            "every" => {
                let minutes = if let Some(n) = rest.strip_suffix('m') {
                    n.parse::<u32>().map_err(|_| bad())?
                } else if let Some(n) = rest.strip_suffix('h') {
                    n.parse::<u32>().map_err(|_| bad())?.saturating_mul(60)
                } else {
                    return Err(bad());
                };
                if (1..=24 * 60).contains(&minutes) {
                    Ok(Timer::Every { minutes })
                } else {
                    Err(format!("timer \"{text}\": every 1m to every 24h"))
                }
            }
            _ => Err(bad()),
        }
    }
}

impl From<Timer> for String {
    fn from(t: Timer) -> String {
        let clock = |m: u32| format!("{:02}:{:02}", m / 60, m % 60);
        match t {
            Timer::Daily { at_min } => format!("daily {}", clock(at_min)),
            Timer::Weekdays { at_min } => format!("weekdays {}", clock(at_min)),
            Timer::Every { minutes } if minutes % 60 == 0 => format!("every {}h", minutes / 60),
            Timer::Every { minutes } => format!("every {minutes}m"),
        }
    }
}

/// `HH:MM`, 24-hour, to minutes after midnight.
fn clock_min(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// One value the routine needs from its trigger or the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    #[serde(rename = "type")]
    pub kind: InputKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    String,
    Number,
    Integer,
    Boolean,
}

impl InputKind {
    pub fn name(self) -> &'static str {
        match self {
            InputKind::String => "string",
            InputKind::Number => "number",
            InputKind::Integer => "integer",
            InputKind::Boolean => "boolean",
        }
    }

    /// Whether `v` is a value of this kind.
    pub fn accepts(self, v: &Value) -> bool {
        match self {
            InputKind::String => v.is_string(),
            InputKind::Number => v.is_number(),
            InputKind::Integer => v.is_i64() || v.is_u64(),
            InputKind::Boolean => v.is_boolean(),
        }
    }
}

/// One step. In the file, one key picks the kind: `tool`, `script`,
/// `desktop`, `expect` or `ask`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawStep", into = "RawStep")]
pub struct Step {
    pub id: String,
    pub kind: StepKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepKind {
    /// One registered tool, its arguments filled from templates.
    Tool {
        tool: String,
        args: Map<String, Value>,
    },
    /// A file in the routine's folder, run in the script sandbox. `args`
    /// reach it filled, as JSON.
    Script {
        file: String,
        args: Map<String, Value>,
    },
    /// A saved desktop replay.
    Desktop(Recipe),
    /// A fixed check on an earlier output. On a pass the run goes on; on a
    /// miss it jumps to `otherwise` (a later step or `done`), or, with no
    /// `otherwise`, the step fails.
    Expect {
        expect: Expect,
        otherwise: Option<String>,
    },
    /// A decision point: the model picks one of `branches` (name to the
    /// step it jumps to, or `done`). It never falls through.
    Ask {
        question: String,
        branches: BTreeMap<String, String>,
    },
}

/// What an `expect` step checks. `from` names an earlier output
/// (`df.stdout`, `probe.json.free_mb`, `df.exit_code`, `inputs.mount`).
/// `regex` picks the value out of it (`group`, default the whole match).
/// The value then must equal `equals` and sit within `at_least` /
/// `at_most`. With none of those, the value must not be empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_least: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_most: Option<f64>,
}

/// A step as written in the file, before its one kind is picked.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStep {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    desktop: Option<Recipe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expect: Option<Expect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    args: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "else")]
    otherwise: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branches: Option<BTreeMap<String, String>>,
}

impl TryFrom<RawStep> for Step {
    type Error = String;

    fn try_from(raw: RawStep) -> Result<Self, String> {
        let id = raw.id;
        let kinds: Vec<&str> = [
            ("tool", raw.tool.is_some()),
            ("script", raw.script.is_some()),
            ("desktop", raw.desktop.is_some()),
            ("expect", raw.expect.is_some()),
            ("ask", raw.ask.is_some()),
        ]
        .into_iter()
        .filter_map(|(k, on)| on.then_some(k))
        .collect();
        let [kind] = kinds.as_slice() else {
            return Err(format!(
                "step \"{id}\": give it one of tool, script, desktop, expect or ask (it has {})",
                if kinds.is_empty() {
                    "none".to_string()
                } else {
                    kinds.join(" and ")
                }
            ));
        };
        let only = |key: &str, set: bool, allowed: &[&str]| {
            if set && !allowed.contains(kind) {
                Err(format!(
                    "step \"{id}\": \"{key}\" doesn't go on a {kind} step"
                ))
            } else {
                Ok(())
            }
        };
        only("args", raw.args.is_some(), &["tool", "script"])?;
        only("else", raw.otherwise.is_some(), &["expect"])?;
        only("branches", raw.branches.is_some(), &["ask"])?;
        let args = raw.args.unwrap_or_default();
        let kind = match *kind {
            "tool" => StepKind::Tool {
                tool: raw.tool.unwrap_or_default(),
                args,
            },
            "script" => StepKind::Script {
                file: raw.script.unwrap_or_default(),
                args,
            },
            "desktop" => StepKind::Desktop(raw.desktop.unwrap_or(Recipe {
                screen: None,
                ops: Vec::new(),
            })),
            "expect" => StepKind::Expect {
                expect: raw.expect.ok_or("unreachable")?,
                otherwise: raw.otherwise,
            },
            _ => StepKind::Ask {
                question: raw.ask.unwrap_or_default(),
                branches: raw
                    .branches
                    .ok_or_else(|| format!("step \"{id}\": an ask step needs \"branches\""))?,
            },
        };
        Ok(Step { id, kind })
    }
}

impl From<Step> for RawStep {
    fn from(step: Step) -> RawStep {
        let mut raw = RawStep {
            id: step.id,
            ..RawStep::default()
        };
        let args = |a: Map<String, Value>| (!a.is_empty()).then_some(a);
        match step.kind {
            StepKind::Tool { tool, args: a } => {
                raw.tool = Some(tool);
                raw.args = args(a);
            }
            StepKind::Script { file, args: a } => {
                raw.script = Some(file);
                raw.args = args(a);
            }
            StepKind::Desktop(recipe) => raw.desktop = Some(recipe),
            StepKind::Expect { expect, otherwise } => {
                raw.expect = Some(expect);
                raw.otherwise = otherwise;
            }
            StepKind::Ask { question, branches } => {
                raw.ask = Some(question);
                raw.branches = Some(branches);
            }
        }
        raw
    }
}

impl StepKind {
    /// Whether the run goes on to the next step after this one (an ask
    /// always jumps).
    fn falls_through(&self) -> bool {
        !matches!(self, StepKind::Ask { .. })
    }

    /// The forward jumps this step can take.
    fn jumps(&self) -> Vec<&str> {
        match self {
            StepKind::Expect {
                otherwise: Some(t), ..
            } => vec![t.as_str()],
            StepKind::Ask { branches, .. } => branches.values().map(String::as_str).collect(),
            _ => Vec::new(),
        }
    }

    /// Whether `field` is an output this kind of step gives.
    fn gives(&self, field: &Field) -> bool {
        match self {
            StepKind::Expect { .. } | StepKind::Ask { .. } => *field == Field::Value,
            _ => *field != Field::Value,
        }
    }
}

/// A reference in a template or an `expect`'s `from`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    Input(String),
    Out { step: String, field: Field },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    Stdout,
    Stderr,
    ExitCode,
    /// What an `expect` picked out, or the branch an `ask` took.
    Value,
    /// A path into the step's stdout read as JSON; an empty path is all of it.
    Json(Vec<String>),
}

/// Reads `inputs.<name>`, `<step>.stdout|stderr|exit_code|value` or
/// `<step>.json[.<key>…]`.
pub fn parse_ref(text: &str) -> Result<Ref, String> {
    let text = text.trim();
    let bad = || format!("\"{text}\" isn't something a routine can read");
    let (head, rest) = text.split_once('.').ok_or_else(bad)?;
    if head == "inputs" {
        return ident(rest).then(|| Ref::Input(rest.into())).ok_or_else(bad);
    }
    if !ident(head) {
        return Err(bad());
    }
    let field = match rest {
        "stdout" => Field::Stdout,
        "stderr" => Field::Stderr,
        "exit_code" => Field::ExitCode,
        "value" => Field::Value,
        "json" => Field::Json(Vec::new()),
        _ => {
            let path = rest.strip_prefix("json.").ok_or_else(bad)?;
            let keys: Vec<String> = path.split('.').map(str::to_string).collect();
            if keys.iter().any(String::is_empty) {
                return Err(bad());
            }
            Field::Json(keys)
        }
    };
    Ok(Ref::Out {
        step: head.into(),
        field,
    })
}

impl std::fmt::Display for Ref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ref::Input(name) => write!(f, "inputs.{name}"),
            Ref::Out { step, field } => match field {
                Field::Json(path) if path.is_empty() => write!(f, "{step}.json"),
                Field::Json(path) => write!(f, "{step}.json.{}", path.join(".")),
                other => write!(f, "{step}.{}", field_name(other)),
            },
        }
    }
}

/// How an `expect` came out. Both carry the value it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    Pass(String),
    Miss(String),
}

impl Expect {
    /// Reads `from` out of `values` and checks it. An `Err` is an output
    /// the check can't read (no such output, no regex match, not a number):
    /// the step fails, rather than counting as a miss.
    pub fn check(&self, values: &Values) -> Result<Checked, String> {
        let text = values.get(&parse_ref(&self.from)?)?;
        let value = match &self.regex {
            Some(pattern) => {
                let re = regex::Regex::new(pattern)
                    .map_err(|_| format!("the regex \"{pattern}\" doesn't compile"))?;
                let group = self.group.unwrap_or(0);
                re.captures(&text)
                    .and_then(|c| c.get(group))
                    .map(|m| m.as_str().to_string())
                    .ok_or_else(|| format!("{} has nothing matching \"{pattern}\"", self.from))?
            }
            None => text.trim().to_string(),
        };
        let number = || {
            value
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("{} gave \"{value}\", which isn't a number", self.from))
        };
        let pass = if let Some(want) = &self.equals {
            match want.as_f64() {
                Some(w) => number()? == w,
                None => value == text_of(want),
            }
        } else if self.at_least.is_some() || self.at_most.is_some() {
            let n = number()?;
            self.at_least.is_none_or(|lo| n >= lo) && self.at_most.is_none_or(|hi| n <= hi)
        } else {
            !value.is_empty()
        };
        Ok(if pass {
            Checked::Pass(value)
        } else {
            Checked::Miss(value)
        })
    }
}

/// Lowercase letters, digits and `_`, starting with a letter.
fn ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A routine or webhook name: lowercase letters, digits and dashes.
fn slug(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A template split into its text and its `{{ ref }}` pieces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece<'a> {
    Text(&'a str),
    Ref(Ref),
}

pub fn pieces(template: &str) -> Result<Vec<Piece<'_>>, String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        if open > 0 {
            out.push(Piece::Text(&rest[..open]));
        }
        let after = &rest[open + 2..];
        let close = after
            .find("}}")
            .ok_or_else(|| format!("\"{template}\" opens {{{{ without closing it"))?;
        out.push(Piece::Ref(parse_ref(&after[..close])?));
        rest = &after[close + 2..];
    }
    if rest.contains("}}") {
        return Err(format!("\"{template}\" closes }}}} without opening it"));
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    Ok(out)
}

/// What earlier steps gave, for filling templates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepOut {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i64>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Values {
    pub inputs: Map<String, Value>,
    pub steps: BTreeMap<String, StepOut>,
}

impl Values {
    /// The JSON value behind `r`, for a field that is JSON.
    fn json(&self, r: &Ref) -> Option<Result<Value, String>> {
        match r {
            Ref::Input(name) => Some(
                self.inputs
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("no value for inputs.{name}")),
            ),
            Ref::Out {
                step,
                field: Field::Json(path),
            } => Some(self.step(step).and_then(|out| {
                let mut at: Value = serde_json::from_str(out.stdout.trim())
                    .map_err(|_| format!("{step} didn't print JSON"))?;
                for key in path {
                    let next = match &at {
                        Value::Object(map) => map.get(key).cloned(),
                        Value::Array(items) => key
                            .parse::<usize>()
                            .ok()
                            .and_then(|i| items.get(i).cloned()),
                        _ => None,
                    };
                    at = next.ok_or_else(|| format!("{step}'s JSON has no {}", path.join(".")))?;
                }
                Ok(at)
            })),
            _ => None,
        }
    }

    fn step(&self, step: &str) -> Result<&StepOut, String> {
        self.steps
            .get(step)
            .ok_or_else(|| format!("{step} hasn't run"))
    }

    /// The text `r` stands for.
    pub fn get(&self, r: &Ref) -> Result<String, String> {
        if let Some(v) = self.json(r) {
            return v.map(|v| text_of(&v));
        }
        let Ref::Out { step, field } = r else {
            unreachable!("inputs are JSON")
        };
        let out = self.step(step)?;
        match field {
            Field::Stdout => Ok(out.stdout.clone()),
            Field::Stderr => Ok(out.stderr.clone()),
            Field::ExitCode => out
                .exit_code
                .map(|c| c.to_string())
                .ok_or_else(|| format!("{step} has no exit code")),
            Field::Value => out
                .value
                .clone()
                .ok_or_else(|| format!("{step} has no value")),
            Field::Json(_) => unreachable!("handled above"),
        }
    }
}

/// A JSON value as template text: strings bare, `null` empty, the rest as JSON.
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// How a filled value is quoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    /// As is (messages, file names, the result).
    Plain,
    /// One POSIX shell word (Bash on Linux and macOS).
    Posix,
    /// One PowerShell word (Windows).
    PowerShell,
}

impl Quote {
    /// The quoting for this computer's terminal tool.
    pub fn shell() -> Quote {
        if cfg!(windows) {
            Quote::PowerShell
        } else {
            Quote::Posix
        }
    }

    pub fn apply(self, s: &str) -> String {
        match self {
            Quote::Plain => s.to_string(),
            Quote::Posix => format!("'{}'", s.replace('\'', r"'\''")),
            // Single quotes keep `$`, backticks and `;` literal; only `'`
            // (and its curly forms) need doubling.
            Quote::PowerShell => {
                let mut out = String::with_capacity(s.len() + 2);
                out.push('\'');
                for c in s.chars() {
                    out.push(c);
                    if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
                        out.push(c);
                    }
                }
                out.push('\'');
                out
            }
        }
    }
}

/// Why a template couldn't be filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FillError {
    /// A value it names isn't there, or the template doesn't parse.
    Missing(String),
    /// A value would change what its command does. Never repaired.
    Unsafe(String),
}

impl std::fmt::Display for FillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FillError::Missing(why) | FillError::Unsafe(why) => f.write_str(why),
        }
    }
}

/// `template` with each `{{ ref }}` replaced by its value, quoted by `quote`.
pub fn fill(template: &str, values: &Values, quote: Quote) -> Result<String, FillError> {
    let places = places(template, quote);
    let mut out = String::with_capacity(template.len());
    let mut k = 0;
    for piece in pieces(template).map_err(FillError::Missing)? {
        match piece {
            Piece::Text(t) => out.push_str(t),
            Piece::Ref(r) => {
                let value = values.get(&r).map_err(FillError::Missing)?;
                check_value(
                    &r,
                    &value,
                    quote,
                    places.get(k).copied().unwrap_or_default(),
                )?;
                k += 1;
                out.push_str(&quote.apply(&value));
            }
        }
    }
    Ok(out)
}

/// Where a placeholder sits in a command template, for [`check_value`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// A value that starts with `-` would be read as an option here.
    pub option: bool,
    /// On Windows, a program gets it through its command line (not a
    /// PowerShell cmdlet, which gets the string as it is).
    pub native: bool,
    /// On Windows, that program may be a `.cmd` or `.bat` file that
    /// cmd.exe reads again: it isn't named with `.exe`.
    pub batch: bool,
}

impl Default for Place {
    /// Where nothing is known: check everything.
    fn default() -> Self {
        Place {
            option: true,
            native: true,
            batch: true,
        }
    }
}

/// Each placeholder's [`Place`] in a command template, in order.
pub fn places(template: &str, quote: Quote) -> Vec<Place> {
    command::places(template, quote)
}

/// Refuses a value that would change what its command does: one that
/// starts with `-` where it starts a word (`--upload-pack=...`), and on
/// Windows one that Windows PowerShell can't pass to a program as one
/// argument.
pub fn check_value(r: &Ref, value: &str, quote: Quote, place: Place) -> Result<(), FillError> {
    let unsafe_ = |why: String| Err(FillError::Unsafe(why));
    let native = quote == Quote::PowerShell && place.native;
    match quote {
        Quote::Plain => Ok(()),
        _ if place.option && reads_as_option(value) => unsafe_(format!(
            "{r} starts with \"-\" and would be read as an option; put -- before it in the step, or join it to its flag (--name=...)"
        )),
        // Windows PowerShell 5.1 hands a program `"` unescaped, and wraps a
        // value with a space in quotes that a trailing `\` escapes.
        _ if native && value.contains('"') => {
            unsafe_(format!("{r} has a double quote, which Windows can't pass to a program safely"))
        }
        _ if native && value.ends_with('\\') && value.contains(char::is_whitespace) => {
            unsafe_(format!("{r} has a space and ends with \"\\\", which Windows reads as a quote"))
        }
        // A .cmd or .bat program (mvn, gradlew) runs through cmd.exe, which
        // reads these in a value with no space as its own.
        _ if native && place.batch && value.contains(['&', '|', '<', '>', '^']) => unsafe_(format!(
            "{r} has one of & | < > ^, which cmd.exe can run as a command on Windows"
        )),
        _ => Ok(()),
    }
}

/// A value a command would take as an option: it starts with `-` and isn't
/// a number like `-5`.
fn reads_as_option(value: &str) -> bool {
    value.starts_with('-') && !is_number(value.trim())
}

/// `5`, `-5`, `2.5`, but not `-inf` or `nan`.
fn is_number(word: &str) -> bool {
    word.trim_start_matches(['-', '+'])
        .starts_with(|c: char| c.is_ascii_digit())
        && word.parse::<f64>().is_ok()
}

/// A step's arguments, filled. A command argument ([`COMMAND_ARGS`]) is
/// quoted for `shell`; a string that is exactly one input keeps that
/// input's type, so `"{{inputs.count}}"` stays a number.
pub fn fill_args(
    tool: &str,
    args: &Map<String, Value>,
    values: &Values,
    shell: Quote,
) -> Result<Map<String, Value>, FillError> {
    args.iter()
        .map(|(key, v)| {
            let quote = if is_command(tool, key) {
                shell
            } else {
                Quote::Plain
            };
            Ok((key.clone(), fill_value(v, values, quote)?))
        })
        .collect()
}

fn is_command(tool: &str, key: &str) -> bool {
    COMMAND_ARGS.contains(&(tool, key))
}

fn fill_value(v: &Value, values: &Values, quote: Quote) -> Result<Value, FillError> {
    Ok(match v {
        Value::String(s) => {
            if quote == Quote::Plain
                && let [Piece::Ref(r @ Ref::Input(_))] =
                    pieces(s).map_err(FillError::Missing)?.as_slice()
            {
                return values
                    .json(r)
                    .unwrap_or_else(|| unreachable!("inputs are JSON"))
                    .map_err(FillError::Missing);
            }
            Value::String(fill(s, values, quote)?)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|i| fill_value(i, values, quote))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, i)| Ok((k.clone(), fill_value(i, values, quote)?)))
                .collect::<Result<_, FillError>>()?,
        ),
        other => other.clone(),
    })
}

/// Every string template inside `v`.
fn strings_in(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|i| strings_in(i, out)),
        Value::Object(map) => map.values().for_each(|i| strings_in(i, out)),
        _ => {}
    }
}

impl Routine {
    /// Reads `routine.json` text. Shape errors only; see [`Routine::problems`].
    pub fn parse(text: &str) -> Result<Routine, String> {
        if text.len() > MAX_BYTES {
            return Err(format!("{FILE} is over {} KB", MAX_BYTES / 1024));
        }
        serde_json::from_str(text).map_err(|e| format!("{FILE}: {e}"))
    }

    /// Everything wrong with this routine, in step order; empty when it is
    /// ready to run. `known_tool` says whether a tool name is registered.
    pub fn problems(&self, known_tool: &dyn Fn(&str) -> bool) -> Vec<String> {
        let mut out = Vec::new();
        if self.version != VERSION {
            out.push(format!(
                "version {} isn't one this app reads (use {VERSION})",
                self.version
            ));
        }
        if !slug(&self.name) {
            out.push(format!(
                "name \"{}\": use lowercase letters, digits and dashes",
                self.name
            ));
        }
        self.trigger_problems(&mut out);
        for (name, input) in &self.inputs {
            if !ident(name) {
                out.push(format!(
                    "input \"{name}\": use lowercase letters, digits and _"
                ));
            }
            if let Some(d) = &input.default
                && !input.kind.accepts(d)
            {
                out.push(format!(
                    "input \"{name}\": its default {d} isn't a {}",
                    input.kind.name()
                ));
            }
        }
        if self.steps.is_empty() {
            out.push("it has no steps".into());
        }
        if self.steps.len() > MAX_STEPS {
            out.push(format!(
                "it has {} steps (at most {MAX_STEPS})",
                self.steps.len()
            ));
        }
        let mut seen = BTreeSet::new();
        for step in &self.steps {
            if !ident(&step.id) || step.id == DONE || step.id == "inputs" {
                out.push(format!(
                    "step \"{}\": name it with lowercase letters, digits and _ (not done or inputs)",
                    step.id
                ));
            }
            if !seen.insert(step.id.as_str()) {
                out.push(format!("step \"{}\" is named twice", step.id));
            }
        }
        let flow = Flow::of(&self.steps);
        for (i, step) in self.steps.iter().enumerate() {
            let at = format!("step \"{}\"", step.id);
            self.kind_problems(i, step, known_tool, &mut out);
            if !flow.reached[i] {
                out.push(format!("{at} can never run"));
            }
            for (text, quote) in self.templates(step) {
                self.ref_problems(&at, &text, flow.ran_before[i].as_ref(), quote, &mut out);
            }
        }
        if let Some(result) = &self.result {
            match flow.ran_at_end() {
                Some(ran) => self.ref_problems("result", result, Some(&ran), false, &mut out),
                None => out.push("result never shows: the last step can't finish the run".into()),
            }
        }
        out
    }

    fn trigger_problems(&self, out: &mut Vec<String>) {
        for t in &self.triggers {
            match t {
                Trigger::Webhook(name) if !slug(name) => out.push(format!(
                    "webhook \"{name}\": use lowercase letters, digits and dashes"
                )),
                Trigger::File { folder, glob }
                    if folder.trim().is_empty() || glob.trim().is_empty() =>
                {
                    out.push("a file trigger needs a folder and a glob".into())
                }
                _ => {}
            }
        }
    }

    fn kind_problems(
        &self,
        i: usize,
        step: &Step,
        known_tool: &dyn Fn(&str) -> bool,
        out: &mut Vec<String>,
    ) {
        let at = format!("step \"{}\"", step.id);
        let later =
            |target: &str| target == DONE || self.steps[i + 1..].iter().any(|s| s.id == target);
        for target in step.kind.jumps() {
            if !later(target) {
                out.push(format!(
                    "{at} jumps to \"{target}\", which isn't a later step or done"
                ));
            }
        }
        match &step.kind {
            StepKind::Tool { tool, .. } => {
                if !known_tool(tool) {
                    out.push(format!("{at}: there is no tool named \"{tool}\""));
                }
            }
            StepKind::Script { file, .. } => {
                // A plain name: no folder, and nothing a terminal or a
                // text view would act on (control or direction characters).
                let plain = !file.is_empty()
                    && !file.starts_with('.')
                    && !file.contains(['/', '\\', ':'])
                    && !file.chars().any(|c| {
                        c.is_control() || matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
                    })
                    && file != "routine.json";
                if !plain {
                    out.push(format!(
                        "{at}: script \"{file}\" must be a file name in the routine's folder"
                    ));
                } else if !file.ends_with(".py") {
                    out.push(format!(
                        "{at}: script \"{file}\" must be a Python file (.py)"
                    ));
                }
            }
            StepKind::Desktop(recipe) => {
                if recipe.ops.is_empty() {
                    out.push(format!("{at}: the desktop replay has no steps"));
                }
            }
            StepKind::Expect { expect, .. } => expect_problems(&at, expect, out),
            StepKind::Ask { question, branches } => {
                if question.trim().is_empty() {
                    out.push(format!("{at}: the ask has no question"));
                }
                if branches.len() < 2 {
                    out.push(format!("{at}: an ask needs at least two branches"));
                }
                for name in branches.keys() {
                    if !ident(name) {
                        out.push(format!(
                            "{at}: branch \"{name}\": use lowercase letters, digits and _"
                        ));
                    }
                }
            }
        }
    }

    /// The step's templates, each with whether it is a command line.
    fn templates(&self, step: &Step) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        match &step.kind {
            StepKind::Tool { tool, args } | StepKind::Script { file: tool, args } => {
                for (key, v) in args {
                    let mut texts = Vec::new();
                    strings_in(v, &mut texts);
                    let command = is_command(tool, key);
                    out.extend(texts.into_iter().map(|t| (t, command)));
                }
            }
            // A `from` that doesn't parse is reported once, by the expect check.
            StepKind::Expect { expect, .. } if parse_ref(&expect.from).is_ok() => {
                out.push((format!("{{{{{}}}}}", expect.from), false))
            }
            StepKind::Expect { .. } => {}
            StepKind::Ask { question, .. } => out.push((question.clone(), false)),
            StepKind::Desktop(_) => {}
        }
        out
    }

    /// Checks each reference in `text`. `ran` is the steps sure to have run
    /// by then (`None` when the place can't be reached at all).
    fn ref_problems(
        &self,
        at: &str,
        text: &str,
        ran: Option<&BTreeSet<usize>>,
        command: bool,
        out: &mut Vec<String>,
    ) {
        let list = match pieces(text) {
            Ok(list) => list,
            Err(e) => {
                out.push(format!("{at}: {e}"));
                return;
            }
        };
        if command {
            if let Some(e) = command::problem(text, Quote::shell()) {
                out.push(format!("{at}: {e}"));
            }
            // A default that would be refused on every run (save as routine
            // can make one from `cargo build --release`).
            let places = places(text, Quote::shell());
            let names = list.iter().filter_map(|p| match p {
                Piece::Ref(r) => Some(r),
                Piece::Text(_) => None,
            });
            for (k, r) in names.enumerate() {
                if let Ref::Input(name) = r
                    && places.get(k).is_some_and(|p| p.option)
                    && let Some(Value::String(d)) =
                        self.inputs.get(name).and_then(|i| i.default.as_ref())
                    && reads_as_option(d)
                {
                    out.push(format!(
                        "{at}: inputs.{name} defaults to \"{d}\", which would be read as an option; write it in the step instead"
                    ));
                }
            }
        }
        for piece in list {
            let Piece::Ref(r) = piece else { continue };
            match r {
                Ref::Input(name) => {
                    if !self.inputs.contains_key(&name) {
                        out.push(format!(
                            "{at} uses inputs.{name}, which isn't one of the inputs"
                        ));
                    }
                }
                Ref::Out { step, field } => {
                    let Some(j) = self.steps.iter().position(|s| s.id == step) else {
                        out.push(format!("{at} uses {step}, which isn't a step"));
                        continue;
                    };
                    if !self.steps[j].kind.gives(&field) {
                        out.push(format!(
                            "{at} reads a {} that step \"{step}\" doesn't give",
                            field_name(&field)
                        ));
                    } else if let Some(ran) = ran
                        && !ran.contains(&j)
                    {
                        out.push(format!("{at} uses {step}, which may not have run by then"));
                    }
                }
            }
        }
    }

    /// Reads and checks `<dir>/routine.json`: the shape, that `name`
    /// matches the folder, and that each script is there.
    pub fn load(dir: &Path, known_tool: &dyn Fn(&str) -> bool) -> Result<Routine, Vec<String>> {
        let text = std::fs::read_to_string(dir.join(FILE))
            .map_err(|e| vec![format!("couldn't read {FILE}: {e}")])?;
        let routine = Routine::parse(&text).map_err(|e| vec![e])?;
        let mut out = routine.problems(known_tool);
        let folder = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if folder != routine.name {
            out.push(format!(
                "name \"{}\" doesn't match its folder \"{folder}\"",
                routine.name
            ));
        }
        for step in &routine.steps {
            if let StepKind::Script { file, .. } = &step.kind
                && !file.contains(['/', '\\'])
                && !dir.join(file).is_file()
            {
                out.push(format!(
                    "step \"{}\": script \"{file}\" isn't in the folder",
                    step.id
                ));
            }
        }
        if out.is_empty() {
            Ok(routine)
        } else {
            Err(out)
        }
    }
}

/// Where routines live.
pub fn routines_dir(config: &Path) -> PathBuf {
    config.join("routines")
}

fn field_name(f: &Field) -> &'static str {
    match f {
        Field::Stdout => "stdout",
        Field::Stderr => "stderr",
        Field::ExitCode => "exit_code",
        Field::Value => "value",
        Field::Json(_) => "json",
    }
}

fn expect_problems(at: &str, e: &Expect, out: &mut Vec<String>) {
    if let Err(err) = parse_ref(&e.from) {
        out.push(format!("{at}: {err}"));
    }
    match (&e.regex, e.group) {
        (Some(re), group) => match regex::Regex::new(re) {
            Ok(re) => {
                if let Some(g) = group
                    && g >= re.captures_len()
                {
                    out.push(format!("{at}: the regex has no group {g}"));
                }
            }
            Err(_) => out.push(format!("{at}: the regex \"{re}\" doesn't compile")),
        },
        (None, Some(_)) => out.push(format!("{at}: a group needs a regex")),
        (None, None) => {}
    }
    if let (Some(lo), Some(hi)) = (e.at_least, e.at_most)
        && lo > hi
    {
        out.push(format!("{at}: at_least {lo} is above at_most {hi}"));
    }
    if e.equals.is_some() && (e.at_least.is_some() || e.at_most.is_some()) {
        out.push(format!("{at}: use equals or a range, not both"));
    }
}

/// Which steps can run, and which are sure to have run before each one.
struct Flow {
    reached: Vec<bool>,
    /// For each step, the steps that ran on every path to it.
    ran_before: Vec<Option<BTreeSet<usize>>>,
    last: Option<(usize, bool)>,
}

impl Flow {
    fn of(steps: &[Step]) -> Flow {
        let n = steps.len();
        let mut ran_before: Vec<Option<BTreeSet<usize>>> = vec![None; n];
        if n > 0 {
            ran_before[0] = Some(BTreeSet::new());
        }
        for i in 0..n {
            let Some(here) = ran_before[i].clone() else {
                continue;
            };
            let mut after = here;
            after.insert(i);
            let mut next: Vec<usize> = steps[i]
                .kind
                .jumps()
                .iter()
                .filter_map(|t| steps.iter().position(|s| s.id == *t))
                .filter(|&j| j > i)
                .collect();
            if steps[i].kind.falls_through() && i + 1 < n {
                next.push(i + 1);
            }
            for j in next {
                ran_before[j] = Some(match ran_before[j].take() {
                    None => after.clone(),
                    Some(seen) => seen.intersection(&after).copied().collect(),
                });
            }
        }
        Flow {
            reached: ran_before.iter().map(Option::is_some).collect(),
            last: n.checked_sub(1).map(|l| (l, steps[l].kind.falls_through())),
            ran_before,
        }
    }

    /// The steps sure to have run when the last step finishes the run;
    /// `None` when no run ends that way.
    fn ran_at_end(&self) -> Option<BTreeSet<usize>> {
        let (last, falls) = self.last?;
        let mut ran = self.ran_before[last].clone().filter(|_| falls)?;
        ran.insert(last);
        Some(ran)
    }
}

#[cfg(test)]
#[path = "routine_tests.rs"]
mod tests;
