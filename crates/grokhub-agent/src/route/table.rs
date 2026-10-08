//! Router R2a routing table: Auto picks each class's model and start effort
//! from a generated, versioned table instead of a preference written in code.
//! Stored at `{config}/models/routing_table.json`; the last
//! [`TABLE_KEEP_VERSIONS`] versions stay beside it as `routing_table.v<n>.json`,
//! and every new version is a ChangeLedger line (kind `model`, origin
//! `self_manage`) whose Undo puts the previous file back byte-identical.
//!
//! Inputs: usable profiles, registry state, entitlement and cost class (only
//! `included` here; R2b adds the others) and a small built-in eval set per
//! class. The eval prompts are fixed synthetic text compiled in, never user
//! data, and every item is scored mechanically (exact, regex, JSON, tool-call
//! shape), so the VerifyGate checker isn't needed. Evals go through
//! `guard_egress`, only on `included` routes, as class `background:eval`
//! (origin `self_manage`), inside [`EVAL_TOKEN_CAP_PER_BUILD`] and
//! [`EVAL_BUILDS_PER_DAY`]. A pair the cap left unscored ranks by price and
//! speed only, with `quality: null`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use grokhub_core::model_registry::probe::{ProbeCall, ProbeKind, ProbeReply, ProbeTransport, DAY_MS};
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::store::{models_dir, read_json};
use grokhub_core::model_registry::cost_class::{probe_class, CostClass};
use grokhub_core::model_registry::{ModelState, Registry};

use super::ladder::{self, Band, Move};
use super::policy::{self, class_row, Fit, CLASS_TABLE};
use crate::harness::{private_write, record_change, ChangeKind, ChangeTarget, Origin};

pub const TABLE_SCHEMA: u32 = 1;
pub const TABLE_FILE: &str = "routing_table.json";
/// The ledger id of the table.
pub const TABLE_ID: &str = "routing_table";
pub const TABLE_KEEP_VERSIONS: usize = 5;
pub const EVAL_CACHE_FILE: &str = "eval_cache.json";
pub const EVAL_SET_VERSION: u32 = 1;
pub const EVAL_ITEMS_PER_CLASS: usize = 5;
pub const EVAL_TOKEN_CAP_PER_BUILD: u64 = 60_000;
pub const EVAL_BUILDS_PER_DAY: u32 = 1;
pub const EVAL_MAX_OUTPUT_TOKENS: u64 = 512;
/// Reasoning tokens assumed for an eval call before one has reported some.
pub const EVAL_REASONING_ALLOWANCE: u64 = 512;
pub const EVAL_TIMEOUT_SECS: u64 = 60;
pub const EVAL_CLASS: &str = "background:eval";
/// What an unscored pair counts as when it ranks by metadata only.
pub const QUALITY_PRIOR: f64 = 0.5;

/// Speed vs quality per class (no user control yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedQuality {
    Speed,
    Balanced,
    Quality,
}

/// (cost weight, latency weight) per setting.
pub const W_SPEED: (f64, f64) = (0.3, 0.4);
pub const W_BALANCED: (f64, f64) = (0.3, 0.15);
pub const W_QUALITY: (f64, f64) = (0.1, 0.0);

impl SpeedQuality {
    /// `Speed` for `background:*` and `chat:quick`; `Balanced` for everyday
    /// chat, small edits, desktop steps and computer checks; `Quality` for the rest.
    pub fn of(class: &str) -> Self {
        match class {
            c if c.starts_with("background:") || c == "chat:quick" => Self::Speed,
            "chat:default" | "code:edit-small" | "desktop:soft" | "repair:diagnose" => Self::Balanced,
            _ => Self::Quality,
        }
    }

    pub fn weights(self) -> (f64, f64) {
        match self {
            Self::Speed => W_SPEED,
            Self::Balanced => W_BALANCED,
            Self::Quality => W_QUALITY,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Speed => "speed",
            Self::Balanced => "balanced",
            Self::Quality => "quality",
        }
    }

    fn best(self) -> &'static str {
        match self {
            Self::Speed => "the fastest good fit",
            Self::Balanced => "the best balance",
            Self::Quality => "the strongest pick",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableRow {
    pub model: String,
    /// The start rung for this model in this class (clamped to its menu).
    pub effort: Option<String>,
    /// Share of the class's eval items passed, or `null` when unscored.
    pub quality: Option<f64>,
    pub est_cost_usd: Option<f64>,
    pub p50_latency_ms: Option<u64>,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassTable {
    pub speed_quality: SpeedQuality,
    pub ranked: Vec<TableRow>,
}

/// `{schema: 1, version, built_at, registry_hash, profiles_hash, eval_set_version, classes}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoutingTable {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub built_at: u64,
    #[serde(default)]
    pub registry_hash: String,
    #[serde(default)]
    pub profiles_hash: String,
    #[serde(default)]
    pub eval_set_version: u32,
    #[serde(default)]
    pub classes: BTreeMap<String, ClassTable>,
}

impl RoutingTable {
    /// The ranked rows for a call class (after aliases). Empty when unlisted.
    pub fn rows(&self, class: &str) -> &[TableRow] {
        let c = class_row(class).map(|r| r.class).unwrap_or(class);
        self.classes.get(c).map(|t| t.ranked.as_slice()).unwrap_or(&[])
    }

    /// The row `model` has in `class`, if any.
    pub fn row(&self, class: &str, model: &str) -> Option<&TableRow> {
        self.rows(class).iter().find(|r| r.model == model)
    }

    /// Same ranking: same models, efforts and order in every class.
    pub fn same_ranking(&self, other: &RoutingTable) -> bool {
        let key = |t: &RoutingTable| -> Vec<(String, Vec<String>)> {
            t.classes.iter().map(|(c, ct)| (c.clone(), ct.ranked.iter().map(|r| format!("{}@{:?}", r.model, r.effort)).collect())).collect()
        };
        key(self) == key(other)
    }
}

// ------------------------------------------------------------------ eval set

/// How an eval reply is scored. Every check is mechanical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// The trimmed reply equals this, ignoring case and a final period.
    Exact(&'static str),
    /// The reply matches this regex.
    Regex(&'static str),
    /// The reply parses as JSON equal to this.
    Json(&'static str),
    /// Exactly one call of this tool with these JSON arguments.
    Tool(&'static str, &'static str),
}

impl Check {
    pub fn how(self) -> &'static str {
        match self {
            Self::Exact(_) => "exact match",
            Self::Regex(_) => "regex",
            Self::Json(_) => "JSON equal",
            Self::Tool(..) => "tool-call shape",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalItem {
    pub class: &'static str,
    pub prompt: &'static str,
    pub check: Check,
}

const fn item(class: &'static str, prompt: &'static str, check: Check) -> EvalItem {
    EvalItem { class, prompt, check }
}

use Check::{Exact, Json, Regex as Re, Tool};

/// Five fixed synthetic items per class. Never user data.
pub const EVAL_SET: &[EvalItem] = &[
    item("background:classify", "Label this line as bug, feature or question. Reply with the label only: The save button crashes the app.", Exact("bug")),
    item("background:classify", "Label this line as bug, feature or question. Reply with the label only: Could we add a dark theme?", Exact("feature")),
    item("background:classify", "Label this line as bug, feature or question. Reply with the label only: How do I export a report?", Exact("question")),
    item("background:classify", "Is this text positive or negative? Reply with one word: The update made everything slower and broke my layout.", Exact("negative")),
    item("background:classify", "Reply with the language of this sentence in English, one word: Le chat dort sur le canapé.", Exact("french")),
    item("background:triage", "Pick the most urgent of these and reply with its letter only: A) typo in a footer B) server is down for all users C) new logo idea", Exact("b")),
    item("background:triage", "Reply with high, medium or low only. Priority of: the nightly backup failed twice in a row.", Exact("high")),
    item("background:triage", "Reply with high, medium or low only. Priority of: a button is two pixels off on one page.", Exact("low")),
    item("background:triage", "Reply with yes or no only. Does this need a human today: the disk is 99% full on the build server.", Exact("yes")),
    item("background:triage", "Reply with the number of items in this list only: apples, pears, plums, figs.", Exact("4")),
    item("background:redact", "Replace the email address with [EMAIL] and reply with the line only: Write to alex.sample@example.com for access.", Exact("Write to [EMAIL] for access")),
    item("background:redact", "Replace the phone number with [PHONE] and reply with the line only: Call 555-0100 after noon.", Exact("Call [PHONE] after noon")),
    item("background:redact", "Replace the card number with [CARD] and reply with the line only: Card 4111 1111 1111 1111 expires soon.", Exact("Card [CARD] expires soon")),
    item("background:redact", "Replace the name Taylor Example with [NAME] and reply with the line only: Taylor Example joined the call.", Exact("[NAME] joined the call")),
    item("background:redact", "Replace the IP address with [IP] and reply with the line only: Blocked 203.0.113.7 at the gate.", Exact("Blocked [IP] at the gate")),
    item("background:summarize", "Summarize in at most eight words: The meeting moved from Tuesday to Thursday because the room was booked.", Re(r"(?i)thursday")),
    item("background:summarize", "Give the main point in one short sentence: Sales rose 10 percent in March after the price cut.", Re(r"(?i)(sales|10)")),
    item("background:summarize", "Reply with a title of at most five words for: a guide to watering indoor plants in winter.", Re(r"(?i)(water|plant)")),
    item("background:summarize", "List the two cities named, comma separated, nothing else: We flew from Oslo to Lisbon.", Re(r"(?i)^\s*oslo\s*,\s*lisbon\s*\.?\s*$")),
    item("background:summarize", "Reply with the total only: three boxes of four pens each.", Exact("12")),
    item("background:judge", "Does this answer the question? Reply PASS or FAIL only. Q: What is 2+2? A: 4", Exact("pass")),
    item("background:judge", "Does this answer the question? Reply PASS or FAIL only. Q: Capital of Japan? A: Osaka", Exact("fail")),
    item("background:judge", "Reply PASS if the code returns the sum of a and b, else FAIL: fn add(a: i32, b: i32) -> i32 { a - b }", Exact("fail")),
    item("background:judge", "Reply PASS if the list is sorted ascending, else FAIL: 1, 3, 5, 9", Exact("pass")),
    item("background:judge", "Reply PASS if the JSON is valid, else FAIL: {\"a\": 1, \"b\": [2, 3]}", Exact("pass")),
    item("background:compact", "Fold these two notes into one line of at most twelve words: opened the file. fixed the typo on line 3.", Re(r"(?i)typo")),
    item("background:compact", "Keep only the decision, one sentence: We talked a lot, then chose blue for the header.", Re(r"(?i)blue")),
    item("background:compact", "Reply with the open task only: done: build. done: test. open: write the release note.", Re(r"(?i)release note")),
    item("background:compact", "Shorten to at most six words: The deployment finished without any errors at all today.", Re(r"(?i)deploy")),
    item("background:compact", "Reply with the file names only, comma separated: edited main.rs and lib.rs, read README.md.", Re(r"(?i)main\.rs\s*,\s*lib\.rs")),
    item("background:memory", "Reply with a JSON object with key fact holding the preference only: I always want answers in metric units.", Re(r"(?i)metric")),
    item("background:memory", "Is this worth remembering long term? Reply yes or no only: My favorite editor is Helix.", Exact("yes")),
    item("background:memory", "Is this worth remembering long term? Reply yes or no only: It is raining right now.", Exact("no")),
    item("background:memory", "Reply with the person's preferred name only: Please call me Sam, not Samuel.", Exact("sam")),
    item("background:memory", "Reply with the time zone only: I work from Berlin, so CET.", Re(r"(?i)cet")),
    item("background:dream", "Merge these duplicate notes into one, reply with the note only: likes tea. likes tea in the morning.", Re(r"(?i)tea")),
    item("background:dream", "Reply with the stale note's letter only: A) uses Rust daily B) used Perl once in 2009 C) writes docs weekly", Exact("b")),
    item("background:dream", "Reply yes or no only. Do these two notes conflict: prefers dark mode / prefers light mode", Exact("yes")),
    item("background:dream", "Reply with the count of distinct topics only: rust, cooking, rust, travel", Exact("3")),
    item("background:dream", "Reply with JSON only: an object with key keep set to true.", Json(r#"{"keep": true}"#)),
    item("chat:quick", "What is 7 times 8? Reply with the number only.", Exact("56")),
    item("chat:quick", "What is the capital of Canada? One word.", Exact("ottawa")),
    item("chat:quick", "Spell the word cat backwards, letters only.", Exact("tac")),
    item("chat:quick", "How many days are in a leap year? Number only.", Exact("366")),
    item("chat:quick", "Which is larger, 0.7 or 0.65? Reply with the number only.", Exact("0.7")),
    item("chat:default", "Reply with a JSON object where name is \"grokhub\" and stars is 5.", Json(r#"{"name": "grokhub", "stars": 5}"#)),
    item("chat:default", "Convert 2 hours and 30 minutes to minutes. Number only.", Exact("150")),
    item("chat:default", "Put these in alphabetical order, comma separated: pear, apple, mango.", Re(r"(?i)^\s*apple\s*,\s*mango\s*,\s*pear\s*\.?\s*$")),
    item("chat:default", "Call the get_weather tool for Paris.", Tool("get_weather", r#"{"city": "Paris"}"#)),
    item("chat:default", "If a train leaves at 9:40 and the trip takes 50 minutes, when does it arrive? Reply as HH:MM.", Re(r"10:30")),
    item("code:edit-small", "Fix the bug and reply with the fixed line only: let total = a - b; // should add", Re(r"let total = a \+ b;")),
    item("code:edit-small", "Rename the variable x to count and reply with the line only: let x = items.len();", Re(r"let count = items\.len\(\);")),
    item("code:edit-small", "Reply with the Rust expression only that checks if n is even.", Re(r"n\s*%\s*2\s*==\s*0")),
    item("code:edit-small", "Call the edit_file tool to change main.rs.", Tool("edit_file", r#"{"path": "main.rs"}"#)),
    item("code:edit-small", "What does this print? Reply with the output only: println!(\"{}\", 3 + 4 * 2);", Exact("11")),
    item("plan", "List exactly three numbered steps to make tea, one per line.", Re(r"(?s)1\..*2\..*3\.")),
    item("plan", "Reply with JSON only: an object with key steps holding a list of two short strings for boiling an egg.", Re(r#"(?s)"steps"\s*:\s*\["#)),
    item("plan", "Which comes first when shipping a fix: write the test, or tag the release? Reply with test or tag only.", Exact("test")),
    item("plan", "Estimate: 3 tasks of 2 hours each and 1 task of 4 hours. Total hours, number only.", Exact("10")),
    item("plan", "Call the create_task tool with the title Write report.", Tool("create_task", r#"{"title": "Write report"}"#)),
    item("design", "Name the color of the hex code #FF0000 in one word.", Exact("red")),
    item("design", "Reply with the CSS property only that sets the space inside a border.", Exact("padding")),
    item("design", "Reply with JSON only: an object with key width set to 320 and key height set to 240.", Json(r#"{"width": 320, "height": 240}"#)),
    item("design", "Which contrasts more with black text: white or dark gray? One word.", Exact("white")),
    item("design", "Reply with the HTML tag only for the largest heading.", Re(r"(?i)^\s*<?h1>?\s*\.?\s*$")),
    item("code:multi-file", "A function moves from a.rs to b.rs. Which file needs a new use line, a.rs callers or b.rs? Reply a.rs or b.rs only.", Exact("a.rs")),
    item("code:multi-file", "Reply with JSON only: an object with key files listing main.rs and lib.rs.", Re(r#"(?s)"files"\s*:\s*\[\s*"main\.rs"\s*,\s*"lib\.rs"\s*\]"#)),
    item("code:multi-file", "Call the read_file tool for src/lib.rs.", Tool("read_file", r#"{"path": "src/lib.rs"}"#)),
    item("code:multi-file", "Renaming a public function: how many places change if it is defined once and called in 3 files? Number only.", Exact("4")),
    item("code:multi-file", "Reply with the Rust keyword only that makes an item visible to other modules.", Exact("pub")),
    item("debug", "Why does this panic? Reply with index or overflow only: let v = vec![1]; v[3];", Exact("index")),
    item("debug", "Reply with the error kind only, one word: thread 'main' panicked at 'called Option::unwrap() on a None value'", Re(r"(?i)(none|unwrap|option)")),
    item("debug", "Call the run_tests tool for the parser crate.", Tool("run_tests", r#"{"crate": "parser"}"#)),
    item("debug", "An off-by-one: for i in 0..=len reads one past the end. Reply with the fixed range only.", Re(r"0\s*\.\.\s*len\b")),
    item("debug", "Reply with JSON only: an object with key cause set to \"null pointer\".", Json(r#"{"cause": "null pointer"}"#)),
    item("desktop:soft", "Call the click tool on the button labeled Save.", Tool("click", r#"{"target": "Save"}"#)),
    item("desktop:soft", "Call the type_text tool with the text hello.", Tool("type_text", r#"{"text": "hello"}"#)),
    item("desktop:soft", "Which key closes a dialog without saving, Enter or Escape? One word.", Exact("escape")),
    item("desktop:soft", "Reply with JSON only: an object with key x set to 100 and key y set to 200.", Json(r#"{"x": 100, "y": 200}"#)),
    item("desktop:soft", "Call the open_app tool for Calculator.", Tool("open_app", r#"{"name": "Calculator"}"#)),
    item("prepare:hard", "Before sending money, what must happen first: ask the user or send it? Reply ask or send only.", Exact("ask")),
    item("prepare:hard", "Reply with JSON only: an object with key action set to \"delete\" and key confirmed set to false.", Json(r#"{"action": "delete", "confirmed": false}"#)),
    item("prepare:hard", "Call the draft_email tool with the subject Invoice.", Tool("draft_email", r#"{"subject": "Invoice"}"#)),
    item("prepare:hard", "Is deleting a folder reversible without a backup? Reply yes or no only.", Exact("no")),
    item("prepare:hard", "Reply with the total only: pay 3 invoices of 125 each.", Exact("375")),
    item("repair:diagnose", "Disk usage is 98%. Reply with the likely problem in two words or fewer.", Re(r"(?i)(disk|space|full)")),
    item("repair:diagnose", "Call the check_service tool for the printer service.", Tool("check_service", r#"{"name": "printer"}"#)),
    item("repair:diagnose", "Reply with JSON only: an object with key ok set to false and key reason set to \"no network\".", Json(r#"{"ok": false, "reason": "no network"}"#)),
    item("repair:diagnose", "A service restarts every minute. Reply with crash or idle only.", Exact("crash")),
    item("repair:diagnose", "Which is the safe first step: read the logs or reinstall the system? Reply logs or reinstall only.", Exact("logs")),
];

/// The class's items.
pub fn eval_items(class: &str) -> Vec<&'static EvalItem> {
    EVAL_SET.iter().filter(|i| i.class == class).collect()
}

/// Every fixed text an eval can send, for the no-user-data check.
pub fn eval_texts() -> Vec<&'static str> {
    EVAL_SET.iter().map(|i| i.prompt).collect()
}

fn tool_schema(name: &str, args: &str) -> Value {
    let keys: Vec<String> = serde_json::from_str::<Value>(args).ok().and_then(|v| v.as_object().map(|o| o.keys().cloned().collect())).unwrap_or_default();
    let props: serde_json::Map<String, Value> = keys.iter().map(|k| (k.clone(), json!({"type": "string"}))).collect();
    json!([{
        "type": "function",
        "name": name,
        "description": "A fixed eval tool.",
        "parameters": {"type": "object", "properties": props, "required": keys, "additionalProperties": false}
    }])
}

/// The wire body of one eval call: a Responses request built only from the item.
pub fn eval_call(model: &str, effort: Option<&str>, item: &EvalItem) -> ProbeCall {
    let mut b = json!({
        "model": model,
        "input": [{"role": "user", "content": item.prompt}],
        "max_output_tokens": EVAL_MAX_OUTPUT_TOKENS,
        "store": false,
    });
    if let Some(e) = effort {
        b["reasoning"] = json!({"effort": e});
    }
    if let Check::Tool(name, args) = item.check {
        b["tools"] = tool_schema(name, args);
    }
    ProbeCall { model: model.into(), effort: effort.map(str::to_string), kind: ProbeKind::Eval, body: b, timeout_secs: EVAL_TIMEOUT_SECS }
}

fn norm(text: &str) -> String {
    text.trim().trim_end_matches('.').trim().to_ascii_lowercase()
}

/// Did the reply pass the item's check?
pub fn passes(check: Check, reply: &ProbeReply) -> bool {
    match check {
        Check::Exact(want) => norm(&reply.text) == norm(want),
        Check::Regex(re) => Regex::new(re).is_ok_and(|r| r.is_match(reply.text.trim())),
        Check::Json(want) => {
            let got = serde_json::from_str::<Value>(reply.text.trim().trim_start_matches("```json").trim_matches('`').trim());
            got.ok().is_some_and(|g| serde_json::from_str::<Value>(want).is_ok_and(|w| w == g))
        }
        Check::Tool(name, args) => {
            reply.tool_calls.len() == 1
                && reply.tool_calls[0].0 == name
                && serde_json::from_str::<Value>(&reply.tool_calls[0].1).ok() == serde_json::from_str::<Value>(args).ok()
        }
    }
}

// ----------------------------------------------------------------- the cache

/// Eval scores, cached by (profile `source_hash`, eval set version, effort),
/// so a rebuild only re-runs pairs that changed. `models/eval_cache.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EvalCache {
    /// Days since the epoch (UTC) of `builds_today`.
    #[serde(default)]
    pub day: u64,
    #[serde(default)]
    pub builds_today: u32,
    /// Key → class → (passed, total).
    #[serde(default)]
    pub scores: BTreeMap<String, BTreeMap<String, (u32, u32)>>,
    /// Key → reply latencies (ms) of its eval calls.
    #[serde(default)]
    pub latencies: BTreeMap<String, Vec<u64>>,
}

pub fn cache_key(source_hash: &str, effort: Option<&str>) -> String {
    format!("{source_hash}|v{EVAL_SET_VERSION}|{}", effort.unwrap_or("none"))
}

impl EvalCache {
    /// The class's quality for a pair, once every item ran.
    pub fn quality(&self, key: &str, class: &str) -> Option<f64> {
        let (passed, total) = *self.scores.get(key)?.get(class)?;
        (total as usize >= EVAL_ITEMS_PER_CLASS).then(|| round(passed as f64 / total as f64, 3))
    }

    pub fn p50(&self, key: &str) -> Option<u64> {
        let mut l = self.latencies.get(key)?.clone();
        if l.is_empty() {
            return None;
        }
        l.sort_unstable();
        Some(l[(l.len() - 1) / 2])
    }

    /// Spend one of today's eval builds, or `false` when today's is spent.
    pub fn take_build(&mut self, now_ms: u64) -> bool {
        let day = now_ms / DAY_MS;
        if day != self.day {
            self.day = day;
            self.builds_today = 0;
        }
        if self.builds_today >= EVAL_BUILDS_PER_DAY {
            return false;
        }
        self.builds_today += 1;
        true
    }
}

fn round(x: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (x * f).round() / f
}

// ------------------------------------------------------------------ building

/// What the table is built from. No I/O behind it.
pub struct TableInputs<'a> {
    pub reg: &'a Registry,
    pub profiles: &'a BTreeMap<String, ModelProfile>,
}

/// Sha-256 over every profile's `source_hash`.
pub fn profiles_hash(profiles: &BTreeMap<String, ModelProfile>) -> String {
    let mut h = Sha256::new();
    for (id, p) in profiles {
        h.update(id.as_bytes());
        h.update(p.source_hash.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The table needs this from a model for a class: long episodes (desktop
/// steps) need prompt caching, and tool-using classes need tools.
fn class_fit(class: &str) -> Fit {
    let needs_tools = !class.starts_with("background:") && class != "chat:quick";
    Fit { needs_tools, ..Fit::default() }
}

/// The start rung for `model` in `class`, clamped to its menu inside the
/// class band. `None` when it can't reach the class floor (a `prepare:hard`
/// row is never below High). `Some(None)` sends no effort.
fn row_effort(class: &str, profile: Option<&ModelProfile>, reg: &Registry, model: &str) -> Option<Option<String>> {
    let row = class_row(class)?;
    let band = Band::of(row, 0);
    let menu = profile.and_then(|p| p.metadata.efforts.clone()).or_else(|| reg.get(model).and_then(|r| r.meta.efforts.clone()));
    let Some(menu) = menu.filter(|m| !m.is_empty()) else {
        // No effort menu: it can't reach High, so it never prepares a hard action.
        return (class != ladder::PREPARE_HARD).then_some(None);
    };
    let picked = ladder::clamp(band.start, &menu, Move::Start, band.floor)?;
    let r = ladder::rung(&picked)?;
    if r < band.floor {
        return None;
    }
    // `minimal` and `max` are not sent: they go out as low and xhigh.
    Some(Some(grokhub_core::parse_reasoning_effort(&picked).map(str::to_string).unwrap_or(picked)))
}

/// The models a class may rank: usable profile, live or degraded, in the plan pool (`included`), and it fits.
fn table_candidates<'a>(inp: &'a TableInputs<'_>, class: &str) -> Vec<&'a String> {
    inp.reg
        .models
        .iter()
        .filter(|(id, r)| {
            matches!(r.state, ModelState::Live | ModelState::Degraded)
                && inp.profiles.get(*id).is_some_and(|p| p.usable)
                && policy::fits_any_cost(inp.reg, inp.profiles, id, class_fit(class), u64::MAX / 2)
                // Evals spend only the plan pool: never a key, a Fast variant or a premium route.
                && probe_class(inp.reg, inp.reg.entitlement.credential, id) == CostClass::Included
                && (class != "desktop:soft" || inp.profiles.get(*id).is_some_and(|p| p.probe.caching == Some(true)))
        })
        .map(|(id, _)| id)
        .collect()
}

/// The (model, effort) pairs to evaluate and the classes each covers.
pub fn eval_pairs(inp: &TableInputs<'_>) -> Vec<(String, Option<String>, Vec<&'static str>)> {
    let mut pairs: BTreeMap<(String, Option<String>), Vec<&'static str>> = BTreeMap::new();
    for row in CLASS_TABLE {
        for id in table_candidates(inp, row.class) {
            if let Some(effort) = row_effort(row.class, inp.profiles.get(id), inp.reg, id) {
                pairs.entry((id.clone(), effort)).or_default().push(row.class);
            }
        }
    }
    pairs.into_iter().map(|((m, e), c)| (m, e, c)).collect()
}

/// Build the table. Pure: the clock is `now_ms`, and the scores come from `cache`.
pub fn build_table(inp: &TableInputs<'_>, cache: &EvalCache, now_ms: u64) -> RoutingTable {
    let mut classes = BTreeMap::new();
    for row in CLASS_TABLE {
        let sq = SpeedQuality::of(row.class);
        let (wc, wl) = sq.weights();
        let mut rows: Vec<TableRow> = Vec::new();
        for id in table_candidates(inp, row.class) {
            let p = inp.profiles.get(id);
            let Some(effort) = row_effort(row.class, p, inp.reg, id) else {
                continue;
            };
            let key = cache_key(&p.map(|p| p.source_hash.clone()).unwrap_or_default(), effort.as_deref());
            let meta = &inp.reg.get(id).map(|r| r.meta.clone()).unwrap_or_default();
            let probe_ms = p.and_then(|p| p.probe.efforts.iter().find(|t| Some(&t.effort) == effort.as_ref()).and_then(|t| t.reply_ms));
            rows.push(TableRow {
                model: id.clone(),
                effort,
                quality: cache.quality(&key, row.class),
                est_cost_usd: policy::expected_cost_usd(&meta.prices, row.class, 0).map(|c| round(c, 6)),
                p50_latency_ms: cache.p50(&key).or(probe_ms),
                why: String::new(),
            });
        }
        let max_cost = rows.iter().filter_map(|r| r.est_cost_usd).fold(0.0, f64::max);
        let max_lat = rows.iter().filter_map(|r| r.p50_latency_ms).max().unwrap_or(0) as f64;
        let score = |r: &TableRow| {
            let nc = match r.est_cost_usd {
                Some(c) if max_cost > 0.0 => c / max_cost,
                Some(_) => 0.0,
                None => 1.0,
            };
            let nl = match r.p50_latency_ms {
                Some(l) if max_lat > 0.0 => l as f64 / max_lat,
                Some(_) => 0.0,
                None => 0.5,
            };
            r.quality.unwrap_or(QUALITY_PRIOR) - wc * nc - wl * nl
        };
        rows.sort_by(|a, b| {
            score(b)
                .total_cmp(&score(a))
                .then(a.est_cost_usd.unwrap_or(f64::MAX).total_cmp(&b.est_cost_usd.unwrap_or(f64::MAX)))
                .then(a.model.cmp(&b.model))
        });
        for (i, r) in rows.iter_mut().enumerate() {
            r.why = match (i, r.quality) {
                (0, _) => format!("{} for {} in your plan", sq.best(), row.plain),
                (_, None) => format!("ranked by price and speed for {} until its check runs", row.plain),
                _ => format!("choice {} for {}", i + 1, row.plain),
            };
        }
        classes.insert(row.class.to_string(), ClassTable { speed_quality: sq, ranked: rows });
    }
    RoutingTable {
        schema: TABLE_SCHEMA,
        version: 0,
        built_at: now_ms,
        registry_hash: inp.reg.hash.clone(),
        profiles_hash: profiles_hash(inp.profiles),
        eval_set_version: EVAL_SET_VERSION,
        classes,
    }
}

/// What an eval run needs from the outside world.
pub struct EvalEnv<'a> {
    pub transport: &'a mut dyn ProbeTransport,
    /// The egress guard for the xAI destination; `false` stops the run.
    pub guard: &'a mut dyn FnMut() -> bool,
    /// Called after each call (the agent writes a `background:eval` span).
    pub on_call: &'a mut dyn FnMut(&ProbeCall, &ProbeReply),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvalRun {
    pub calls: u32,
    pub tokens: u64,
    /// The token cap stopped the run with pairs left unscored.
    pub capped: bool,
}

fn estimate(call: &ProbeCall) -> u64 {
    (call.body.to_string().len() as u64).div_ceil(4) + 16
}

/// Run the evals the cache doesn't have yet, inside the token cap. Only
/// `included` routes are candidates, so nothing here touches a metered route.
pub fn run_evals(inp: &TableInputs<'_>, cache: &mut EvalCache, env: EvalEnv<'_>) -> EvalRun {
    let mut run = EvalRun::default();
    let mut max_reasoning = EVAL_REASONING_ALLOWANCE;
    for (model, effort, classes) in eval_pairs(inp) {
        let hash = inp.profiles.get(&model).map(|p| p.source_hash.clone()).unwrap_or_default();
        let key = cache_key(&hash, effort.as_deref());
        for class in classes {
            if cache.quality(&key, class).is_some() {
                continue;
            }
            let items = eval_items(class);
            let calls: Vec<ProbeCall> = items.iter().map(|i| eval_call(&model, effort.as_deref(), i)).collect();
            let need: u64 = calls.iter().map(|c| estimate(c) + EVAL_MAX_OUTPUT_TOKENS + max_reasoning).sum();
            if run.tokens + need > EVAL_TOKEN_CAP_PER_BUILD {
                run.capped = true;
                continue;
            }
            let mut passed = 0;
            for (item, call) in items.iter().zip(&calls) {
                if !(env.guard)() {
                    return run;
                }
                let reply = env.transport.send(call).unwrap_or_default();
                run.calls += 1;
                run.tokens += reply.input_tokens + reply.output_tokens + reply.reasoning_tokens;
                max_reasoning = max_reasoning.max(reply.reasoning_tokens);
                (env.on_call)(call, &reply);
                if (200..300).contains(&reply.status) {
                    cache.latencies.entry(key.clone()).or_default().push(reply.latency_ms);
                    if passes(item.check, &reply) {
                        passed += 1;
                    }
                }
            }
            cache.scores.entry(key.clone()).or_default().insert(class.to_string(), (passed, items.len() as u32));
        }
    }
    run
}

// ------------------------------------------------------------------- storage

pub fn table_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(TABLE_FILE)
}

pub fn eval_cache_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(EVAL_CACHE_FILE)
}

/// The saved table, or an empty one.
pub fn load_table(config_dir: &Path) -> RoutingTable {
    read_json(&table_path(config_dir)).unwrap_or_default()
}

/// `routing_table.json` as one ChangeLedger target, so Undo puts back the exact bytes.
pub struct TableFile<'a> {
    pub path: &'a Path,
}

impl ChangeTarget for TableFile<'_> {
    fn kind(&self) -> ChangeKind {
        ChangeKind::Model
    }

    fn id(&self) -> Result<String, String> {
        Ok(TABLE_ID.into())
    }

    fn file(&self) -> Result<PathBuf, String> {
        Ok(self.path.to_path_buf())
    }

    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        match fs::read(self.path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn put(&self, bytes: Option<&[u8]>) -> Result<(), String> {
        match bytes {
            Some(b) => private_write(self.path, b),
            None => match fs::remove_file(self.path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            },
        }
    }

    fn label_of(&self, _bytes: &[u8]) -> String {
        "routing table".into()
    }
}

/// Write `table` as the current version when its ranking differs from the
/// saved one (`Ok(false)` writes nothing). The old file moves to
/// `routing_table.v<n>.json` (the newest [`TABLE_KEEP_VERSIONS`] stay), and
/// the write is one ChangeLedger line with Undo.
pub fn write_table(config_dir: &Path, table: &mut RoutingTable, reason: &str) -> Result<bool, String> {
    let path = table_path(config_dir);
    let old: Option<RoutingTable> = read_json(&path);
    if old.as_ref().is_some_and(|o| o.same_ranking(table) && o.schema == table.schema) {
        return Ok(false);
    }
    table.version = old.as_ref().map(|o| o.version.saturating_add(1)).unwrap_or(1);
    let text = serde_json::to_string_pretty(&table).map_err(|e| e.to_string())?;
    let dir = models_dir(config_dir);
    let reason = format!("routing table v{}: {reason}", table.version);
    record_change(config_dir, &TableFile { path: &path }, Origin::SelfManage, &reason, || {
        if let Some(o) = &old {
            let _ = fs::copy(&path, dir.join(format!("routing_table.v{}.json", o.version)));
        }
        private_write(&path, text.as_bytes())
    })?;
    let mut kept: Vec<(u32, PathBuf)> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let v = name.strip_prefix("routing_table.v")?.strip_suffix(".json")?;
            Some((v.parse().ok()?, e.path()))
        })
        .collect();
    kept.sort_by_key(|(v, _)| std::cmp::Reverse(*v));
    for (_, p) in kept.into_iter().skip(TABLE_KEEP_VERSIONS) {
        let _ = fs::remove_file(p);
    }
    Ok(true)
}

/// What a rebuild did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rebuilt {
    pub written: bool,
    pub version: u32,
    pub eval: EvalRun,
    /// Models that ranked in a class for the first time.
    pub joined: Vec<String>,
}

/// Rebuild the table when the registry or profiles changed (or `force`, the
/// Refresh models button), running due evals through `eval` once a day.
pub fn rebuild(config_dir: &Path, eval: Option<EvalEnv<'_>>, force: bool, now_ms: u64) -> Rebuilt {
    let (reg, profiles) = (grokhub_core::model_registry::store::load_registry(config_dir), grokhub_core::model_registry::profile::read_profiles(config_dir));
    let old = load_table(config_dir);
    let inp = TableInputs { reg: &reg, profiles: &profiles };
    let unchanged = old.registry_hash == reg.hash && old.profiles_hash == profiles_hash(&profiles) && old.eval_set_version == EVAL_SET_VERSION;
    if unchanged && !force {
        return Rebuilt { version: old.version, ..Rebuilt::default() };
    }
    let mut cache: EvalCache = read_json(&eval_cache_path(config_dir)).unwrap_or_default();
    let mut run = EvalRun::default();
    if let Some(env) = eval {
        if !eval_pairs(&inp).is_empty() && cache.take_build(now_ms) {
            run = run_evals(&inp, &mut cache, env);
        }
    }
    let _ = grokhub_core::model_registry::store::write_json(&eval_cache_path(config_dir), &cache);
    let mut table = build_table(&inp, &cache, now_ms);
    let had: Vec<&String> = old.classes.values().flat_map(|c| c.ranked.iter().map(|r| &r.model)).collect();
    let mut joined: Vec<String> = table.classes.values().flat_map(|c| c.ranked.iter().map(|r| r.model.clone())).filter(|m| !old.classes.is_empty() && !had.contains(&m)).collect();
    joined.sort();
    joined.dedup();
    let why = if joined.is_empty() { "the model list or a profile changed".to_string() } else { format!("{} joined", joined.join(", ")) };
    let written = write_table(config_dir, &mut table, &why).unwrap_or(false);
    Rebuilt { written, version: if written { table.version } else { old.version }, eval: run, joined }
}

/// "How Auto picks": one line per class with its top model@effort and setting.
pub fn how_auto_picks(table: &RoutingTable) -> Vec<String> {
    CLASS_TABLE
        .iter()
        .map(|row| {
            let ct = table.classes.get(row.class);
            let top = ct.and_then(|c| c.ranked.first()).map(|r| match &r.effort {
                Some(e) => format!("{}@{e}", r.model),
                None => r.model.clone(),
            });
            let sq = ct.map(|c| c.speed_quality).unwrap_or(SpeedQuality::of(row.class));
            format!("{} → {} ({})", row.class, top.unwrap_or_else(|| "today's model".into()), sq.word())
        })
        .collect()
}

/// `/why table`: the same list, plus the version and when it was built.
pub fn why_table_text(table: &RoutingTable, now_ms: u64) -> String {
    if table.classes.is_empty() {
        return "No routing table yet. It builds after the model list refreshes (30 s after start, then every 6 hours).".into();
    }
    let mut out = vec![format!("Routing table v{} · built {}", table.version, grokhub_core::pulse::ago_label(table.built_at, now_ms))];
    out.extend(how_auto_picks(table));
    out.join("\n")
}
