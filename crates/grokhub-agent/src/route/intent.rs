//! The turn reader: what a typed message is *for*, before any model call.
//!
//! [`super::ask`] says what kind of work a turn is so the router can pick a
//! model. This module says what the app should *do* with the message, so
//! nobody ever has to pick a Chat, Plan or side-question mode:
//!
//! - nothing running: answer it ([`TurnRoute::Chat`]), or, for a big
//!   multi-step job on the computer, plan first ([`TurnRoute::Plan`]);
//! - a reply running: answer a side question beside the work without
//!   touching it ([`TurnRoute::Side`]), fold a correction into the work at
//!   its next step ([`TurnRoute::Steer`]), stop it ([`TurnRoute::Stop`]), or
//!   line a new job up for after ([`TurnRoute::Queue`]).
//!
//! Pure rules, microseconds, no tokens, no I/O ([`read_turn`]). Misroutes
//! cost differently, so the bars differ: a real job sent to the side is lost
//! (side runs can't write), so the side needs the surest read; a question
//! that lands in the queue costs one tap (Steer now) or a short wait; a chat
//! turn that should have planned still plans, because the model can enter
//! plan mode itself. When unsure: chat when idle, queue when running.
//!
//! [`read_turn_learned`] adds this computer's corrections ([`note_correction`]):
//! a small store keyed by a hash of the message's *shape* (its first words,
//! question or request, length), never its words.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::ask::{self, AskContext, AskKind};

/// What to do with a typed message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TurnRoute {
    /// Answer it as an ordinary turn.
    Chat,
    /// A big multi-step job: plan first, then work once the plan is approved.
    Plan,
    /// A reply is running: answer this beside it, read-only, without touching it.
    Side,
    /// A reply is running: fold this into the work at its next step.
    Steer,
    /// A reply is running: send this after it ends.
    Queue,
    /// A reply is running: stop it.
    Stop,
}

impl TurnRoute {
    pub const ALL: [TurnRoute; 6] = [
        TurnRoute::Chat,
        TurnRoute::Plan,
        TurnRoute::Side,
        TurnRoute::Steer,
        TurnRoute::Queue,
        TurnRoute::Stop,
    ];

    pub fn id(self) -> &'static str {
        match self {
            TurnRoute::Chat => "chat",
            TurnRoute::Plan => "plan",
            TurnRoute::Side => "side",
            TurnRoute::Steer => "steer",
            TurnRoute::Queue => "queue",
            TurnRoute::Stop => "stop",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.id() == s.trim())
    }

    /// Whether this route can be taken while a reply runs (`true`) or while
    /// nothing runs (`false`).
    pub fn fits(self, running: bool) -> bool {
        match self {
            TurnRoute::Chat | TurnRoute::Plan => !running,
            TurnRoute::Side | TurnRoute::Steer | TurnRoute::Queue | TurnRoute::Stop => running,
        }
    }

    /// The one line the app shows the moment the message is routed, so a
    /// message sent mid-run never looks dropped.
    pub fn ack(self) -> &'static str {
        match self {
            TurnRoute::Chat => "",
            TurnRoute::Plan => "Making a plan first",
            TurnRoute::Side => "Answering on the side",
            TurnRoute::Steer => "Got it, I'll fold that in after this step",
            TurnRoute::Queue => "Queued for after this reply",
            TurnRoute::Stop => "Stopped",
        }
    }
}

/// Everything [`read_turn`] looks at besides the words.
#[derive(Debug, Clone, Copy, Default)]
pub struct TurnCx {
    /// A reply is running in this chat.
    pub running: bool,
    /// The kind of the chat's previous turn, for short follow-ups.
    pub prior: Option<AskKind>,
    /// The running reply is casual talk ([`casual_turn`]), not work. Every
    /// message typed during it then waits as its own turn with its own reply:
    /// nothing is answered only off to the side, steered in or folded.
    pub casual: bool,
}

/// The read of one typed message.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnRead {
    pub route: TurnRoute,
    /// How sure the read is (0..=1).
    pub confidence: f64,
    /// The words to send: `text` with a typed override (`/btw`, `/plan`) taken off.
    pub text: String,
    /// Rule ids for the route record (`turn:side:btw`, `turn:plan:steps`).
    pub rules: Vec<String>,
}

/// A side question needs at least this sure a read; below it the message queues.
pub const SIDE_BAR: f64 = 0.6;
/// Plan score needed to plan first instead of answering.
pub const PLAN_BAR: f64 = 2.5;
/// Longest message read as a side question (a long message is work).
pub const SIDE_MAX_CHARS: usize = 240;

const STOP_WORDS: &[&str] = &[
    "stop",
    "stop it",
    "stop that",
    "stop now",
    "stop stop",
    "please stop",
    "cancel",
    "cancel it",
    "cancel that",
    "wait",
    "wait wait",
    "hold on",
    "hang on",
    "pause",
    "halt",
    "abort",
    "never mind",
    "nevermind",
    "nvm",
    "forget it",
    "no stop",
];

const SIDE_LEADS: &[&str] = &[
    "btw",
    "by the way",
    "quick question",
    "quick q",
    "side question",
    "side note",
    "unrelated",
    "random question",
    "off topic",
    "while you work",
    "while you're working",
    "while that runs",
    "while you wait",
    "just curious",
    "out of curiosity",
];

/// Words that open a question on their own.
const WH_WORDS: &[&str] = &[
    "what", "whats", "what's", "why", "how", "hows", "how's", "when", "where", "wheres", "where's",
    "who", "whos", "who's", "which",
];

/// Words that open a question only when it ends in a question mark
/// ("do the docs next" is work; "do you know the size?" is a question).
const AUX_WORDS: &[&str] = &[
    "is", "isn't", "isnt", "are", "aren't", "does", "doesn't", "do", "did", "didn't", "was",
    "were", "has", "have", "hasn't", "haven't", "will", "should", "shall", "am", "any", "anything",
    "can", "could", "would",
];

const REQUEST_PHRASES: &[&str] = &[
    "can you",
    "could you",
    "would you",
    "will you",
    "can u",
    "could u",
    "please",
    "pls",
    "plz",
    "i need you to",
    "i want you to",
    "i'd like you to",
    "id like you to",
    "help me",
    "let's",
    "lets",
    "go ahead",
    "i need a",
    "i want a",
];

/// Verbs that do work on the computer, code or files.
const WORK_VERBS: &[&str] = &[
    "build",
    "create",
    "make",
    "set",
    "setup",
    "install",
    "uninstall",
    "refactor",
    "migrate",
    "implement",
    "add",
    "write",
    "fix",
    "update",
    "upgrade",
    "convert",
    "rename",
    "move",
    "organize",
    "organise",
    "clean",
    "deploy",
    "configure",
    "rewrite",
    "redesign",
    "overhaul",
    "port",
    "automate",
    "generate",
    "delete",
    "remove",
    "replace",
    "restructure",
    "integrate",
    "scaffold",
    "sort",
    "backup",
    "download",
    "copy",
    "test",
    "debug",
    "optimize",
    "optimise",
    "translate",
    "draft",
    "edit",
    "change",
    "run",
    "start",
    "launch",
    "publish",
    "ship",
];

/// Words that say the job is big.
const SCOPE_WORDS: &[&str] = &[
    "whole",
    "entire",
    "everything",
    "all",
    "full",
    "complete",
    "project",
    "app",
    "application",
    "system",
    "repo",
    "codebase",
    "website",
    "site",
    "pipeline",
    "feature",
    "server",
    "library",
    "folder",
    "folders",
    "files",
    "drive",
    "database",
];

/// Verbs that make a new thing, and the things they make.
const MAKE_VERBS: &[&str] = &[
    "build", "create", "make", "scaffold", "develop", "code", "design",
];
const ARTIFACTS: &[&str] = &[
    "website",
    "site",
    "app",
    "application",
    "bot",
    "system",
    "tool",
    "game",
    "server",
    "dashboard",
    "extension",
    "plugin",
    "api",
    "service",
    "program",
    "launcher",
    "mod",
];

const SEQUENCERS: &[&str] = &[
    "then",
    "after that",
    "afterwards",
    "and finally",
    "finally",
    "first",
    "second",
    "next",
    "step",
    "steps",
    "phase",
];

/// Leads that correct or redirect the running work.
const STEER_LEADS: &[&str] = &[
    "actually",
    "no,",
    "no ",
    "nope",
    "nah",
    "wait,",
    "wait ",
    "instead",
    "rather",
    "not that",
    "not the",
    "don't",
    "dont",
    "do not",
    "stop doing",
    "stop using",
    "use ",
    "make it",
    "make sure",
    "change it",
    "keep it",
    "also ",
    "and also",
    "oh and",
    "oh also",
    "skip",
    "forget the",
    "ignore",
    "only ",
    "remember to",
    "try ",
    "focus on",
    "leave ",
    "don't forget",
    "dont forget",
    "careful",
    "be careful",
    "wrong",
    "that's wrong",
    "thats wrong",
    "go back",
    "undo",
];

const STEER_INNER: &[&str] = &[
    " instead",
    "rather than",
    " not the ",
    "wrong one",
    "that's not",
    "thats not",
    "you missed",
    "you forgot",
    "the other one",
];

/// Leads that line new work up for after the running reply.
const QUEUE_LEADS: &[&str] = &[
    "after that",
    "after this",
    "when you're done",
    "when youre done",
    "when you are done",
    "once you're done",
    "once youre done",
    "once that's done",
    "next,",
    "next ",
    "later",
    "afterwards",
];

const NO_PLAN: &[&str] = &[
    "just answer",
    "just tell me",
    "quick answer",
    "short answer",
    "don't plan",
    "dont plan",
    "no plan",
    "without a plan",
    "just do it",
];

const PLAN_LEADS: &[&str] = &[
    "plan:",
    "make a plan",
    "plan out",
    "plan how",
    "plan this",
    "plan it",
    "plan first",
    "write a plan",
    "come up with a plan",
];

#[derive(Debug, Default)]
struct Shape {
    lower: String,
    words: Vec<String>,
    chars: usize,
    question: bool,
    request: bool,
    imperative: bool,
    work_verbs: usize,
    scope: usize,
    list_items: usize,
    sequencers: usize,
    /// Commas and "and"s: how many parts the ask has.
    parts: usize,
    /// "build … a website", "create a bot".
    makes_thing: bool,
}

fn words_of(lower: &str) -> Vec<String> {
    lower
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_matches('\'').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

fn starts_with_any(lower: &str, leads: &'static [&'static str]) -> Option<&'static str> {
    leads.iter().copied().find(|l| {
        lower.starts_with(l)
            && lower[l.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() || l.ends_with(' ') || l.ends_with(','))
    })
}

fn contains_phrase(lower: &str, words: &[String], phrase: &str) -> bool {
    if phrase.contains(' ') || phrase.contains(',') || phrase.contains(':') {
        lower.contains(phrase)
    } else {
        words.iter().any(|w| w == phrase)
    }
}

fn shape(text: &str) -> Shape {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    let words = words_of(&lower);
    let first = words.first().map(String::as_str).unwrap_or("");
    let marked = lower.ends_with('?') || lower.contains("? ");
    let question = (marked && !REQUEST_PHRASES.iter().any(|p| lower.starts_with(p)))
        || WH_WORDS.contains(&first)
        || (marked && AUX_WORDS.contains(&first))
        || lower.starts_with("tell me")
        || lower.starts_with("any idea")
        || lower.starts_with("do you know")
        || lower.starts_with("remind me");
    let request = REQUEST_PHRASES
        .iter()
        .any(|p| contains_phrase(&lower, &words, p));
    let imperative = WORK_VERBS.contains(&first);
    let mut seen = Vec::new();
    for w in &words {
        if WORK_VERBS.contains(&w.as_str()) && !seen.contains(w) {
            seen.push(w.clone());
        }
    }
    let scope = words
        .iter()
        .filter(|w| SCOPE_WORDS.contains(&w.as_str()))
        .count();
    let list_items = trimmed
        .lines()
        .map(str::trim_start)
        .filter(|l| {
            l.starts_with("- ")
                || l.starts_with("* ")
                || l.starts_with("• ")
                || l.split_once(['.', ')']).is_some_and(|(n, _)| {
                    !n.is_empty() && n.len() <= 2 && n.chars().all(|c| c.is_ascii_digit())
                })
        })
        .count();
    let sequencers = SEQUENCERS
        .iter()
        .filter(|s| contains_phrase(&lower, &words, s))
        .count();
    let parts = lower.matches(',').count() + words.iter().filter(|w| *w == "and").count();
    let makes_thing = words.iter().any(|w| MAKE_VERBS.contains(&w.as_str()))
        && words.iter().any(|w| ARTIFACTS.contains(&w.as_str()));
    Shape {
        chars: trimmed.chars().count(),
        parts,
        makes_thing,
        lower,
        words,
        question,
        request,
        imperative,
        work_verbs: seen.len(),
        scope,
        list_items,
        sequencers,
    }
}

fn bare(lower: &str) -> String {
    lower
        .trim()
        .trim_end_matches(|c: char| c == '!' || c == '.' || c == '?' || c.is_whitespace())
        .replace(',', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A typed override: `/btw <q>` or `/plan <job>`. Returns the route it asks
/// for (if it fits now) and the words after it.
fn typed_override(text: &str) -> Option<(&'static str, String)> {
    let t = text.trim_start();
    for (cmd, id) in [("/btw", "side"), ("/plan", "plan"), ("/side", "side")] {
        if let Some(rest) = t.strip_prefix(cmd) {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                return Some((id, rest.trim().to_string()));
            }
        }
    }
    None
}

/// Read one typed message. Same words and context, same answer.
pub fn read_turn(text: &str, cx: &TurnCx) -> TurnRead {
    let own = ask::own_words(text);
    let mut rules = Vec::new();
    let mut words = text.trim().to_string();
    if let Some((id, rest)) = typed_override(own) {
        if !rest.is_empty() {
            match id {
                "side" if cx.running => {
                    return done(TurnRoute::Side, 1.0, rest, "turn:side:typed", rules);
                }
                "side" => return done(TurnRoute::Chat, 1.0, rest, "turn:chat:typed-side", rules),
                // A typed `/plan` while a reply runs waits its turn with the
                // command kept, so it still plans when it is sent.
                _ if cx.running => {
                    return done(TurnRoute::Queue, 1.0, words, "turn:queue:typed-plan", rules);
                }
                _ => return done(TurnRoute::Plan, 1.0, rest, "turn:plan:typed", rules),
            }
        }
    }
    let s = shape(own);
    if cx.running {
        one_reply_each(read_running(&s, own, &mut words, &mut rules), cx)
    } else {
        read_idle(&s, own, cx, words, rules)
    }
}

/// A casual chat answers every message in the chat, once and in order: a
/// side question, a steer or a new ask typed while a reply runs waits as its
/// own turn. Stop still stops. A typed `/btw` keeps the side card it asked for.
fn one_reply_each(mut read: TurnRead, cx: &TurnCx) -> TurnRead {
    let keep = matches!(read.route, TurnRoute::Stop | TurnRoute::Queue)
        || read.rules.first().is_some_and(|r| r.ends_with(":typed"));
    if cx.running && cx.casual && !keep {
        read.route = TurnRoute::Queue;
        read.rules.insert(0, "turn:queue:casual".into());
    }
    read
}

/// The running turn's ask is casual talk, not work: small talk, a feeling, a
/// quick question. A task, a repair, research, something to make or a
/// background job is work.
pub fn casual_turn(ask: &str) -> bool {
    use crate::brain::{BriefInput, Intent};
    let own = ask::own_words(ask).trim();
    if own.is_empty() {
        return false;
    }
    let brief = crate::brain::rules(&BriefInput {
        text: own,
        ..Default::default()
    });
    match brief {
        Some(b) => matches!(b.intent, Intent::Chat | Intent::Question),
        None => {
            let s = shape(own);
            !(s.request || s.imperative) && s.work_verbs == 0
        }
    }
}

fn done(
    route: TurnRoute,
    confidence: f64,
    text: String,
    rule: &str,
    mut rules: Vec<String>,
) -> TurnRead {
    rules.insert(0, rule.to_string());
    TurnRead {
        route,
        confidence: (confidence * 100.0).round() / 100.0,
        text,
        rules,
    }
}

fn read_running(s: &Shape, own: &str, words: &mut String, rules: &mut Vec<String>) -> TurnRead {
    let lower = s.lower.as_str();
    let text = std::mem::take(words);
    let rules = std::mem::take(rules);
    let b = bare(lower);
    if STOP_WORDS.contains(&b.as_str()) {
        return done(TurnRoute::Stop, 0.95, text, "turn:stop", rules);
    }
    // A side lead ("btw", "quick question") names a side question, unless
    // what follows is a work request ("btw can you refactor the app").
    if let Some(lead) = starts_with_any(lower, SIDE_LEADS) {
        let rest = shape(own.trim_start()[lead.len()..].trim_start_matches([',', ':', ' ', '-']));
        let work = (rest.request || rest.imperative) && rest.work_verbs > 0;
        if !work && s.chars <= SIDE_MAX_CHARS * 2 {
            return done(TurnRoute::Side, 0.9, text, "turn:side:lead", rules);
        }
        return done(
            TurnRoute::Queue,
            0.6,
            text,
            "turn:queue:side-lead-work",
            rules,
        );
    }
    if let Some(_lead) = starts_with_any(lower, QUEUE_LEADS) {
        return done(TurnRoute::Queue, 0.85, text, "turn:queue:lead", rules);
    }
    let steer_lead = starts_with_any(lower, STEER_LEADS).is_some();
    let steer_inner = STEER_INNER.iter().any(|p| lower.contains(p));
    if steer_lead || steer_inner {
        let rule = if steer_lead {
            "turn:steer:lead"
        } else {
            "turn:steer:correction"
        };
        return done(TurnRoute::Steer, 0.8, text, rule, rules);
    }
    // A plain question with no work in it: answered on the side.
    if s.question && !s.request && s.chars <= SIDE_MAX_CHARS {
        let work = s.imperative || (s.work_verbs >= 2 && s.scope > 0);
        if !work {
            let c = if s.lower.ends_with('?') { 0.75 } else { 0.65 };
            return done(TurnRoute::Side, c, text, "turn:side:question", rules);
        }
    }
    // A question asked as a request about facts ("can you tell me what…").
    if s.request
        && s.work_verbs == 0
        && s.chars <= SIDE_MAX_CHARS
        && (lower.contains("tell me") || lower.contains("remind me") || lower.contains("explain"))
    {
        return done(TurnRoute::Side, 0.6, text, "turn:side:tell", rules);
    }
    done(TurnRoute::Queue, 0.5, text, "turn:queue", rules)
}

fn read_idle(s: &Shape, own: &str, cx: &TurnCx, text: String, rules: Vec<String>) -> TurnRead {
    let lower = s.lower.as_str();
    if NO_PLAN.iter().any(|p| lower.contains(p)) {
        return done(TurnRoute::Chat, 0.9, text, "turn:chat:no-plan", rules);
    }
    if starts_with_any(lower, PLAN_LEADS).is_some() {
        return done(TurnRoute::Plan, 0.9, text, "turn:plan:lead", rules);
    }
    let asked = ask::read(
        own,
        &AskContext {
            prior: cx.prior,
            ..AskContext::default()
        },
    );
    let wants_work = s.request || s.imperative;
    let mut score = 0.0;
    let mut why = Vec::new();
    if matches!(asked.kind, AskKind::CodeMulti | AskKind::Design) && wants_work {
        score += 2.0;
        why.push("kind");
    }
    if asked.kind == AskKind::Plan && wants_work && s.work_verbs > 0 {
        score += 1.0;
        why.push("plan-kind");
    }
    if s.work_verbs >= 2 && wants_work {
        score += 1.5;
        why.push("verbs");
    }
    if s.list_items >= 3 && s.work_verbs > 0 {
        score += 1.5;
        why.push("list");
    }
    if s.sequencers >= 1 && s.work_verbs >= 1 && wants_work {
        score += 1.0;
        why.push("steps");
    }
    if s.scope >= 1 && s.work_verbs >= 1 && wants_work {
        score += 1.0;
        why.push("scope");
    }
    if s.makes_thing && wants_work {
        score += 1.5;
        why.push("build");
    }
    if s.parts >= 2 && s.work_verbs >= 1 && wants_work {
        score += 1.0;
        why.push("parts");
    }
    if s.chars > 300 && s.work_verbs > 0 {
        score += 0.5;
        why.push("long");
    }
    if s.question && !s.request {
        score -= 2.0;
    }
    if s.chars < 40 {
        score -= 1.0;
    }
    if score >= PLAN_BAR && wants_work {
        let rule = format!("turn:plan:{}", why.join("+"));
        return done(TurnRoute::Plan, (score / 5.0).min(1.0), text, &rule, rules);
    }
    let c = if score <= 0.0 { 0.9 } else { 1.0 - score / 5.0 };
    done(
        TurnRoute::Chat,
        c.clamp(0.3, 0.95),
        text,
        "turn:chat",
        rules,
    )
}

// ------------------------------------------------------------ learned nudges

/// Corrections this computer saw, under `{config}/models/`.
pub const NUDGE_FILE: &str = "turn_nudges.json";
pub const NUDGE_SCHEMA: u32 = 1;
/// Shapes kept; the oldest goes first.
pub const NUDGE_CAP: usize = 500;
/// Corrections to one route a shape needs before the route flips.
pub const NUDGE_FLIP: u32 = 2;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct NudgeFile {
    pub schema: u32,
    /// Shape hash → corrections per route id, plus when it was last touched.
    pub shapes: BTreeMap<String, Nudge>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Nudge {
    pub to: BTreeMap<String, u32>,
    pub at: u64,
}

static NUDGES: Mutex<Option<(PathBuf, NudgeFile)>> = Mutex::new(None);

pub fn nudge_path(config_dir: &Path) -> PathBuf {
    grokhub_core::model_registry::store::models_dir(config_dir).join(NUDGE_FILE)
}

/// FNV-1a: stable across builds and platforms, unlike the std hasher.
fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The key a message is remembered by: whether a reply ran, its first two
/// words, question or request, and a length bucket. Hashed; no words kept.
pub fn shape_key(text: &str, running: bool) -> String {
    let s = shape(ask::own_words(text));
    let lead: Vec<&str> = s.words.iter().take(2).map(String::as_str).collect();
    let bucket = match s.chars {
        0..=40 => 0,
        41..=120 => 1,
        121..=300 => 2,
        _ => 3,
    };
    let raw = format!(
        "{}|{}|{}|{}|{}",
        u8::from(running),
        lead.join(" "),
        u8::from(s.question),
        u8::from(s.request),
        bucket
    );
    format!("{:016x}", fnv(raw.as_bytes()))
}

fn with_nudges<R>(config_dir: &Path, f: impl FnOnce(&mut NudgeFile) -> R) -> R {
    let path = nudge_path(config_dir);
    let mut g = NUDGES.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_none_or(|(p, _)| *p != path) {
        let file = grokhub_core::model_registry::store::read_json::<NudgeFile>(&path)
            .filter(|f| f.schema == NUDGE_SCHEMA)
            .unwrap_or(NudgeFile {
                schema: NUDGE_SCHEMA,
                shapes: BTreeMap::new(),
            });
        *g = Some((path, file));
    }
    let (_, file) = g.as_mut().expect("nudges loaded above");
    f(file)
}

/// The route this computer learned for a message's shape, if corrections flipped it.
pub fn learned(config_dir: &Path, text: &str, running: bool) -> Option<TurnRoute> {
    let key = shape_key(text, running);
    with_nudges(config_dir, |file| {
        let n = file.shapes.get(&key)?;
        let (id, count) = n.to.iter().max_by_key(|(_, c)| **c)?;
        let route = TurnRoute::parse(id)?;
        (*count >= NUDGE_FLIP && route.fits(running)).then_some(route)
    })
}

/// [`read_turn`], then this computer's corrections. A typed override always stands.
pub fn read_turn_learned(config_dir: &Path, text: &str, cx: &TurnCx) -> TurnRead {
    let mut read = read_turn(text, cx);
    if read.rules.first().is_some_and(|r| r.ends_with(":typed")) {
        return read;
    }
    if let Some(route) = learned(config_dir, text, cx.running) {
        if route != read.route {
            read.route = route;
            read.confidence = 0.8;
            read.rules.insert(0, format!("turn:{}:learned", route.id()));
        }
    }
    one_reply_each(read, cx)
}

/// The user showed `text` belonged on `to` (tapped Tell the main task, Steer
/// now, said "no, just answer" after a plan…). Remembered by shape only.
pub fn note_correction(config_dir: &Path, text: &str, running: bool, to: TurnRoute) {
    if text.trim().is_empty() || !to.fits(running) {
        return;
    }
    let key = shape_key(text, running);
    let snapshot = with_nudges(config_dir, |file| {
        let n = file.shapes.entry(key).or_default();
        *n.to.entry(to.id().to_string()).or_default() += 1;
        n.at = grokhub_core::now_ms();
        while file.shapes.len() > NUDGE_CAP {
            let oldest = file
                .shapes
                .iter()
                .min_by_key(|(_, n)| n.at)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => {
                    file.shapes.remove(&k);
                }
                None => break,
            }
        }
        file.clone()
    });
    let _ = grokhub_core::model_registry::store::write_json(&nudge_path(config_dir), &snapshot);
}

/// "no, just answer" / "make a plan first" right after a routed turn: the
/// route the user is correcting the previous turn to, if any.
pub fn correction_of_previous(text: &str) -> Option<TurnRoute> {
    let b = bare(&text.to_lowercase());
    let to_chat = [
        "no just answer",
        "just answer",
        "just answer me",
        "don't plan",
        "dont plan",
        "no plan",
        "skip the plan",
        "just do it",
        "no just do it",
    ];
    let to_plan = [
        "make a plan first",
        "plan first",
        "plan it first",
        "make a plan",
        "can you plan it first",
    ];
    if to_chat.contains(&b.as_str()) {
        Some(TurnRoute::Chat)
    } else if to_plan.contains(&b.as_str()) {
        Some(TurnRoute::Plan)
    } else {
        None
    }
}

/// A reply right after a plan that says go: the plan runs instead of being
/// planned again. "Go ahead, but skip step 3" still says go; "yes but change
/// step 2" asks for a change and keeps planning.
pub fn approves_plan(text: &str) -> bool {
    let b = bare(&text.to_lowercase());
    const EXACT: [&str; 25] = [
        "yes",
        "go ahead",
        "yes go",
        "yes do it",
        "yep",
        "yeah",
        "y",
        "ok",
        "okay",
        "go",
        "do it",
        "approved",
        "approve",
        "lgtm",
        "looks good",
        "sounds good",
        "ship it",
        "let's go",
        "lets go",
        "let's do it",
        "lets do it",
        "go for it",
        "send it",
        "yes please",
        "make it so",
    ];
    if EXACT.contains(&b.as_str()) {
        return true;
    }
    [
        "go ahead",
        "yes go",
        "yes do it",
        "do it but",
        "do it and",
        "approved",
        "lgtm",
    ]
    .iter()
    .any(|p| b.starts_with(&format!("{p} ")))
}

/// How much of the main task's ask a side answer is shown.
const SIDE_ASK_CLIP: usize = 400;
/// How much of the reply so far a side answer is shown, from its end.
const SIDE_SO_FAR_TAIL: usize = 600;
/// How much of a side answer Tell the main task passes on.
const SIDE_TELL_CLIP: usize = 1_200;

/// The first `max` characters of `text`, with an ellipsis when cut.
pub fn clip_start(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}

/// The last `max` characters of `text`, with an ellipsis when cut.
pub fn clip_end(text: &str, max: usize) -> String {
    let text = text.trim();
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let cut: String = text.chars().skip(n - max).collect();
    format!("…{cut}")
}

/// A side answer's own prompt: the question plus a snapshot of the work in
/// flight (`ask`, and the reply `so_far`), so it can answer about the work
/// without continuing it.
pub fn side_prompt(question: &str, ask: &str, so_far: &str) -> String {
    let mut out = String::from(
        "You are answering a quick side question while the main task in this chat keeps working. \
         Answer only the side question, briefly and plainly. Do not continue, change or redo the \
         main task, and do not start any work of your own. You can read files and look things up; \
         you cannot change anything.\n",
    );
    if !ask.trim().is_empty() {
        out.push_str(&format!(
            "\nThe main task is working on: {}\n",
            clip_start(ask, SIDE_ASK_CLIP)
        ));
    }
    if !so_far.trim().is_empty() {
        out.push_str(&format!(
            "\nIts reply so far ends with: {}\n",
            clip_end(so_far, SIDE_SO_FAR_TAIL)
        ));
    }
    out.push_str(&format!("\nSide question: {}", question.trim()));
    out
}

/// What **Tell the main task** feeds the main work: the side question and
/// the answer it got, so the main task can use both.
pub fn tell_main_text(question: &str, answer: &str) -> String {
    let answer = answer.trim();
    if answer.is_empty() {
        question.trim().to_string()
    } else {
        format!(
            "I asked on the side: {}\nThe answer was: {}\nTake this into account.",
            question.trim(),
            clip_start(answer, SIDE_TELL_CLIP)
        )
    }
}

#[cfg(test)]
#[path = "intent_tests.rs"]
mod intent_tests;
