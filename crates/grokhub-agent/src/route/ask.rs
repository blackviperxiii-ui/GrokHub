//! The ask reader: what one chat turn is, on this computer, before any model
//! call. Every user turn used to route as `chat:default` with a keyword count
//! for difficulty, so "hi" and "debug this deadlock across three crates" got
//! the same model and nearly the same effort. [`read`] looks at the typed
//! words (and only those, never stored) and says:
//!
//! - the **kind** of work ([`AskKind`]): a greeting, a quick fact, everyday
//!   chat, creative writing, an explanation, research, a small code edit, a
//!   multi-file change, debugging, planning, design, or math and logic;
//! - the **lane**: the class-table row whose band and model needs fit that
//!   kind (`chat:quick`, `code:edit-small`, `debug`, `chat:reason`, …), so the
//!   capability ranking picks a cheap fast model, the coding model or the
//!   flagship for it;
//! - a calibrated **difficulty** `d` in 0.0..=1.0;
//! - the **effort** to start at, a rung on
//!   [`grokhub_core::model_registry::EFFORT_LADDER`].
//!
//! Pure, deterministic and microseconds: no model in the loop, no tokens, no
//! I/O. The research behind it (difficulty-aware routing, adaptive thinking
//! budgets, the overthinking curves) is in the router research notes; the
//! short version is that most turns need little reasoning, a few need a lot,
//! and the cheap signals that tell them apart are task type, code and error
//! text, constraints, math, and length. Effort changes freely between turns;
//! the model changes reluctantly ([`super::live`] keeps a long chat on its
//! model so the prompt cache stays warm).
//!
//! A short follow-up with no signals of its own ("and the second one?",
//! "now add tests") keeps the kind of the turn before it in the same chat.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

/// The class every typed chat turn arrives as; only it gets a read.
pub const CHAT_CLASS: &str = "chat:default";

/// What the turn asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AskKind {
    /// "hi", "thanks", "gg", "ok cool".
    Greeting,
    /// A short factual question with one answer.
    QuickFact,
    /// Everyday conversation, advice, opinions, gaming talk.
    Chat,
    /// Stories, poems, lyrics, jokes, role-play, names.
    Creative,
    /// Explain, compare, teach, walk through.
    Explain,
    /// Needs current or outside facts (news, prices, patch notes, reviews).
    Research,
    /// One function, one line, one snippet.
    CodeSmall,
    /// A change across files, a module, a whole feature.
    CodeMulti,
    /// Something is broken: errors, crashes, stack traces, "why does".
    Debug,
    /// Plans, roadmaps, strategy, step-by-step approaches.
    Plan,
    /// System, API, schema or UI design.
    Design,
    /// Math, logic, proofs, puzzles, algorithms.
    Math,
}

impl AskKind {
    pub const ALL: [AskKind; 12] = [
        AskKind::Greeting,
        AskKind::QuickFact,
        AskKind::Chat,
        AskKind::Creative,
        AskKind::Explain,
        AskKind::Research,
        AskKind::CodeSmall,
        AskKind::CodeMulti,
        AskKind::Debug,
        AskKind::Plan,
        AskKind::Design,
        AskKind::Math,
    ];

    /// Stable id for route records (`ask:debug`).
    pub fn id(self) -> &'static str {
        match self {
            AskKind::Greeting => "greeting",
            AskKind::QuickFact => "quick_fact",
            AskKind::Chat => "chat",
            AskKind::Creative => "creative",
            AskKind::Explain => "explain",
            AskKind::Research => "research",
            AskKind::CodeSmall => "code_small",
            AskKind::CodeMulti => "code_multi",
            AskKind::Debug => "debug",
            AskKind::Plan => "plan",
            AskKind::Design => "design",
            AskKind::Math => "math",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == s.trim())
    }

    /// The class-table row whose band and model needs fit this kind.
    pub fn lane(self) -> &'static str {
        match self {
            AskKind::Greeting | AskKind::QuickFact => "chat:quick",
            AskKind::Chat | AskKind::Creative | AskKind::Explain | AskKind::Research => {
                "chat:default"
            }
            AskKind::CodeSmall => "code:edit-small",
            AskKind::CodeMulti => "code:multi-file",
            AskKind::Debug => "debug",
            AskKind::Plan => "plan",
            AskKind::Design => "design",
            AskKind::Math => "chat:reason",
        }
    }
}

/// Everything [`read`] looks at besides the words.
#[derive(Debug, Clone, Copy, Default)]
pub struct AskContext {
    /// Tokens the request carries (history included).
    pub ctx_tokens: u64,
    /// The turn has an image.
    pub needs_image: bool,
    /// Tools the plan already lists for the turn.
    pub planned_tools: u32,
    /// The chat is in plan mode.
    pub plan_mode: bool,
    /// The kind of the previous turn in this chat, for short follow-ups.
    pub prior: Option<AskKind>,
}

/// A3: what a turn puts at risk, from its own words.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Stakes {
    #[default]
    None,
    /// A destructive or irreversible step: `rm -rf`, dropping or truncating a
    /// table, a force push, a hard reset, deleting a branch, a production
    /// deploy or migration.
    Destructive,
    /// Security or crypto: authentication, password hashing, encryption,
    /// keys and secrets, CVEs and vulnerabilities, permissions.
    Security,
}

impl Stakes {
    pub fn id(self) -> Option<&'static str> {
        match self {
            Stakes::None => None,
            Stakes::Destructive => Some("destructive"),
            Stakes::Security => Some("security"),
        }
    }
}

/// The lane a high-stakes turn takes: Flagship, and never below High.
pub const STAKES_LANE: &str = super::ladder::PREPARE_HARD;

/// The read of one turn.
#[derive(Debug, Clone, PartialEq)]
pub struct AskRead {
    pub kind: AskKind,
    /// A3: what the turn puts at risk. Anything but `None` takes [`STAKES_LANE`].
    pub stakes: Stakes,
    /// Difficulty, rounded to hundredths.
    pub d: f64,
    /// The rung to start at (index on `EFFORT_LADDER`).
    pub effort: usize,
    /// How sure the read is (0..=1): the winning score's margin.
    pub confidence: f64,
    /// Rule ids for the route record (`ask:debug`, `ask:effort:high`, `ask:follow`).
    pub rules: Vec<String>,
}

impl AskRead {
    pub fn lane(&self) -> &'static str {
        if self.stakes == Stakes::None {
            self.kind.lane()
        } else {
            STAKES_LANE
        }
    }

    pub fn effort_name(&self) -> &'static str {
        rung_name(self.effort)
    }
}

const NONE: usize = 0;
pub(crate) const LOW: usize = 2;
const MEDIUM: usize = 3;
const HIGH: usize = 4;
pub(crate) const XHIGH: usize = 5;

fn rung_name(r: usize) -> &'static str {
    grokhub_core::model_registry::EFFORT_LADDER
        .get(r)
        .copied()
        .unwrap_or("medium")
}

// ---------------------------------------------------------------- signals

const GREETINGS: &[&str] = &[
    "hi",
    "hey",
    "hello",
    "yo",
    "sup",
    "hiya",
    "howdy",
    "thanks",
    "thank",
    "thx",
    "ty",
    "ok",
    "okay",
    "k",
    "kk",
    "cool",
    "nice",
    "great",
    "awesome",
    "lol",
    "lmao",
    "haha",
    "gg",
    "ggs",
    "wp",
    "np",
    "yes",
    "yep",
    "yeah",
    "yup",
    "no",
    "nope",
    "nah",
    "bye",
    "cya",
    "gn",
    "gm",
    "morning",
    "night",
    "sweet",
    "perfect",
    "got",
    "it",
    "good",
    "bro",
    "dude",
    "man",
    "much",
    "you",
    "u",
    "there",
    "what's",
    "whats",
    "up",
    "wassup",
    "love",
    "it!",
    "appreciate",
    "cheers",
    "sure",
    "alright",
    "bet",
    "word",
    "dope",
    "sick",
    "fire",
    "based",
    "w",
    "l",
    "rip",
    "oof",
    "mb",
    "my",
    "bad",
    "sorry",
    "kinda",
    "a",
    "the",
    "again",
    "so",
    "very",
    "really",
    "all",
];

const FACT_OPENERS: &[&str] = &[
    "what is ",
    "what's ",
    "whats ",
    "what are ",
    "who is ",
    "who's ",
    "who was ",
    "who won ",
    "when is ",
    "when was ",
    "when did ",
    "where is ",
    "where's ",
    "how many ",
    "how much ",
    "how old ",
    "how tall ",
    "how long is ",
    "how far ",
    "define ",
    "meaning of ",
    "what does ",
    "which is ",
    "is it ",
    "convert ",
    "translate ",
    "spell ",
    "capital of ",
];

const CREATIVE: &[&str] = &[
    "write a story",
    "write me a story",
    "short story",
    "a poem",
    "write a poem",
    "haiku",
    "limerick",
    "lyrics",
    "a song",
    "rap about",
    "a joke",
    "tell me a joke",
    "roleplay",
    "role-play",
    "role play",
    "pretend you",
    "pretend to be",
    "in character",
    "fanfic",
    "fan fiction",
    "backstory",
    "lore for",
    "name ideas",
    "name my",
    "name for my",
    "names for",
    "gamertag",
    "a caption",
    "a slogan",
    "a tagline",
    "a bio for",
    "write a tweet",
    "write a post",
    "dialogue",
    "a scene",
    "creative",
];

const EXPLAIN: &[&str] = &[
    "explain",
    "how does",
    "how do ",
    "how is ",
    "what's the difference",
    "difference between",
    "compare",
    "vs ",
    " versus ",
    "pros and cons",
    "walk me through",
    "teach me",
    "help me understand",
    "eli5",
    "in detail",
    "break down",
    "break it down",
    "overview of",
    "summarize",
    "summary of",
    "tl;dr",
    "tldr",
    "what happens when",
    "is it worth",
    "should i ",
    "which should",
    "recommend",
];

const RESEARCH: &[&str] = &[
    "latest",
    "news",
    "today",
    "this week",
    "right now",
    "currently",
    "current ",
    "patch notes",
    "release date",
    "price of",
    "how much does",
    "cost of",
    "reviews",
    "look up",
    "search for",
    "search the web",
    "google ",
    "find me",
    "find out",
    "sources",
    "cite",
    "meta ",
    "tier list",
    "best build",
    "best loadout",
    "best settings",
    "2025",
    "2026",
    "2027",
];

const CODE_WORDS: &[&str] = &[
    "function",
    "method",
    "class ",
    "struct",
    "enum ",
    "variable",
    "regex",
    "script",
    "snippet",
    "code",
    "compile",
    "rust",
    "python",
    "javascript",
    "typescript",
    "golang",
    " go ",
    "java ",
    "c++",
    "c#",
    "kotlin",
    "swift",
    "lua",
    "bash",
    "powershell",
    "sql",
    "query",
    "json",
    "yaml",
    "toml",
    "html",
    "css",
    "react",
    "tauri",
    "cargo",
    "npm",
    "pip ",
    "api",
    "endpoint",
    "unit test",
    "tests for",
    "lint",
    "clippy",
    "git ",
    "commit",
    "pull request",
    "merge conflict",
    "dockerfile",
    "docker",
    "kubernetes",
    "terraform",
    "shader",
    "addon",
    "weakaura",
];

const CODE_TOKENS: &[&str] = &[
    "fn ",
    "def ",
    "let ",
    "const ",
    "var ",
    "impl ",
    "pub ",
    "import ",
    "#include",
    "return ",
    "=>",
    "->",
    "::",
    "();",
    "){",
    ") {",
    "};",
    "</",
    "/>",
    "async ",
    "await ",
    "self.",
    "this.",
    "println!",
    "console.log",
    "printf",
    "#[",
    "@override",
    "lambda",
    "elif ",
    "else {",
    "useState",
    "SELECT ",
    "INSERT ",
    "WHERE ",
];

const CODE_EXTS: &[&str] = &[
    ".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".go", ".java", ".kt", ".cpp", ".cc", ".c", ".h",
    ".hpp", ".cs", ".rb", ".php", ".swift", ".lua", ".sh", ".ps1", ".sql", ".toml", ".yaml",
    ".yml", ".json", ".html", ".css", ".scss", ".vue", ".svelte", ".zig", ".dart",
];

const EDIT_VERBS: &[&str] = &[
    "write a function",
    "write a script",
    "write code",
    "write me a",
    "implement",
    "add a ",
    "add an ",
    "add tests",
    "add support",
    "rename",
    "change the",
    "convert this",
    "rewrite",
    "refactor",
    "make it",
    "update the",
    "fix the typo",
    "one-liner",
    "one liner",
    "snippet",
    "example of",
    "how do i ",
    "how to ",
];

const MULTI: &[&str] = &[
    "across",
    "codebase",
    "code base",
    "whole repo",
    "the repo",
    "repository",
    "project-wide",
    "every file",
    "all files",
    "several files",
    "multiple files",
    "many files",
    "multi-file",
    "modules",
    "crates",
    "packages",
    "monorepo",
    "end to end",
    "end-to-end",
    "full app",
    "whole app",
    "entire app",
    "from scratch",
    "build me an app",
    "build an app",
    "build a website",
    "build a game",
    "migrate",
    "migration",
    "port this",
    "port it",
    "rewrite the",
    "feature",
    "integration",
];

const BROKEN: &[&str] = &[
    "error",
    "errors",
    "bug",
    "buggy",
    "broken",
    "broke",
    "crash",
    "crashes",
    "crashing",
    "crashed",
    "fails",
    "failing",
    "failed",
    "failure",
    "doesn't work",
    "doesn't respond",
    "doesnt respond",
    "not responding",
    "stopped working",
    "stops working",
    "keeps crashing",
    "does not work",
    "doesnt work",
    "not working",
    "won't",
    "wont ",
    "can't get",
    "cant get",
    "stuck",
    "hangs",
    "freezes",
    "freezing",
    "stutter",
    "stuttering",
    "lag spikes",
    "fps drop",
    "black screen",
    "blue screen",
    "bsod",
    "segfault",
    "segmentation fault",
    "panic",
    "panicked",
    "exception",
    "traceback",
    "stack trace",
    "undefined",
    "null pointer",
    "nullpointer",
    "deadlock",
    "race condition",
    "memory leak",
    "leaks",
    "regression",
    "flaky",
    "wrong output",
    "unexpected",
    "debug",
    "diagnose",
    "troubleshoot",
    "figure out why",
    "why does",
    "why is",
    "why isn't",
    "why won't",
    "why do i",
    "what's wrong",
    "whats wrong",
    "fix this",
    "fix it",
    "fix my",
];

const TRACE: &[&str] = &[
    "traceback (most recent call last)",
    "panicked at",
    "error[e",
    "exception in thread",
    "at line ",
    " line ",
    "stack backtrace",
    "core dumped",
    "exit code",
    "exit status",
    "errno",
    "typeerror",
    "valueerror",
    "keyerror",
    "attributeerror",
    "referenceerror",
    "syntaxerror",
    "nullreferenceexception",
    "cannot find",
    "could not find",
    "not found",
    "unresolved",
    "mismatched types",
    "borrow of",
    "cannot borrow",
    "lifetime",
    "segmentation fault",
    "0x0000",
    "warning:",
    "error:",
];

const PLAN: &[&str] = &[
    "plan",
    "roadmap",
    "strategy",
    "step by step",
    "step-by-step",
    "steps to",
    "how should i",
    "how would you",
    "what's the best way",
    "best way to",
    "approach",
    "outline",
    "milestones",
    "checklist",
    "timeline",
    "prioritize",
    "trade-off",
    "tradeoff",
    "decide between",
    "workflow",
    "game plan",
    "progression path",
    "leveling guide",
];

const DESIGN: &[&str] = &[
    "design",
    "architecture",
    "architect",
    "system design",
    "schema",
    "data model",
    "api design",
    "interface for",
    "wireframe",
    "mockup",
    "mock-up",
    "layout",
    "ui for",
    "ux",
    "user flow",
    "state machine",
    "protocol",
    "spec for",
    "rfc",
];

const MATH: &[&str] = &[
    "prove",
    "proof",
    "theorem",
    "lemma",
    "solve",
    "equation",
    "integral",
    "derivative",
    "differentiate",
    "integrate",
    "limit of",
    "matrix",
    "eigen",
    "probability",
    "expected value",
    "combinatorics",
    "how many ways",
    "permutation",
    "combination",
    "modulo",
    "prime",
    "factor",
    "polynomial",
    "logarithm",
    "geometry",
    "triangle",
    "algorithm",
    "complexity",
    "big o",
    "big-o",
    "optimize",
    "optimal",
    "dp ",
    "dynamic programming",
    "graph theory",
    "riddle",
    "puzzle",
    "logic puzzle",
    "brain teaser",
    "calculate",
    "compute",
    "dps calc",
    "damage calc",
    "breakpoint",
];

const HARDNESS: &[&str] = &[
    "carefully",
    "thoroughly",
    "rigorous",
    "rigorously",
    "edge case",
    "edge cases",
    "corner case",
    "production",
    "performance",
    "concurrency",
    "concurrent",
    "thread-safe",
    "thread safe",
    "lock-free",
    "async",
    "distributed",
    "security",
    "vulnerability",
    "exploit",
    "memory safety",
    "unsafe",
    "lifetime",
    "generic",
    "trait bound",
    "macro",
    "compiler",
    "kernel",
    "driver",
    "low-level",
    "low level",
    "assembly",
    "optimize",
    "benchmark",
    "olympiad",
    "imo ",
    "putnam",
    "competition",
    "nontrivial",
    "non-trivial",
    "tricky",
    "subtle",
    "hard",
    "complex",
    "difficult",
];

const FOLLOW: &[&str] = &[
    "and ",
    "also",
    "now ",
    "then ",
    "what about",
    "how about",
    "same for",
    "do it",
    "do that",
    "again",
    "the other",
    "the second",
    "the first",
    "the last",
    "next one",
    "that one",
    "this one",
    "more",
    "another",
    "instead",
    "it ",
    "that ",
    "those",
    "them",
    "keep going",
    "continue",
    "go on",
    "and?",
    "why?",
    "how?",
    "really?",
    "example?",
    "shorter",
    "longer",
];

const CORRECTION: &[&str] = &[
    "that's wrong",
    "thats wrong",
    "that is wrong",
    "not right",
    "incorrect",
    "still broken",
    "still doesn't",
    "still doesnt",
    "still not",
    "still fails",
    "still failing",
    "didn't work",
    "didnt work",
    "did not work",
    "same error",
    "you're wrong",
    "youre wrong",
    "no that's",
    "no, that",
    "wrong answer",
    "try again",
];

// ---------------------------------------------------------------- stakes

/// Destructive or irreversible, matched as written (lowercase).
const DESTRUCTIVE: &[&str] = &[
    "rm -rf",
    "rm -fr",
    "drop table",
    "drop database",
    "drop schema",
    "truncate table",
    "git push --force",
    "git push -f",
    "push --force",
    "--force-with-lease",
    "force push",
    "force-push",
    "git reset --hard",
    "reset --hard",
    "git branch -d",
    "push origin --delete",
    "git clean -fd",
    "mkfs",
    "dd if=",
    "wipe the disk",
    "format the drive",
];

/// Words that delete, for "delete the old branch".
const DELETE_WORDS: &[&str] = &["delete", "deleting", "remove", "removing", "nuke"];
/// Words that ship to production, for "run the migration in prod".
const SHIP_WORDS: &[&str] = &[
    "deploy",
    "deploys",
    "deploying",
    "deployment",
    "migrate",
    "migrating",
    "migration",
    "migrations",
    "rollout",
    "rollback",
    "push",
    "pushing",
    "ship",
    "shipping",
];
/// Words that wipe data, for "drop the users table".
const WIPE_WORDS: &[&str] = &["drop", "dropping", "truncate", "wipe", "wiping", "purge"];
const DATA_WORDS: &[&str] = &["table", "tables", "database", "databases", "db", "schema"];

/// Security or crypto, matched as written (lowercase).
const SECURITY: &[&str] = &[
    "authentication",
    "authenticate",
    "authorization",
    "oauth",
    "jwt",
    "access token",
    "auth token",
    "session token",
    "2fa",
    "csrf",
    "xss",
    "sql injection",
    "privilege escalation",
    "cve",
    "cve-",
    "vulnerability",
    "vulnerabilities",
    "security hole",
    "security flaw",
    "security bug",
    "security issue",
    "security risk",
    "security review",
    "security audit",
    "encryption",
    "encrypt",
    "decrypt",
    "bcrypt",
    "argon2",
    "scrypt",
    "pbkdf2",
    "api key",
    "api keys",
    "private key",
    "secret key",
    "ssh key",
    "signing key",
    "secrets",
    "credentials",
    "permissions",
    "chmod",
    "sudoers",
];

/// Password words that make a turn security work when they come with one of [`PASSWORD_WITH`].
const PASSWORD_WITH: &[&str] = &[
    "hash",
    "hashes",
    "hashed",
    "hashing",
    "salt",
    "salted",
    "store",
    "storing",
    "stored",
    "plaintext",
    "encrypt",
    "leak",
    "leaked",
];

/// `a` and `b` within `gap` words of each other.
fn near(tokens: &[&str], a: &[&str], b: &[&str], gap: usize) -> bool {
    tokens.iter().enumerate().any(|(i, w)| {
        a.contains(w)
            && tokens[i.saturating_sub(gap)..(i + gap + 1).min(tokens.len())]
                .iter()
                .any(|x| b.contains(x))
    })
}

/// A3: what a turn puts at risk. Casual words never count: "drop me a line",
/// "the production value of a movie", "truncate this string".
pub fn stakes(text: &str) -> Stakes {
    let raw = own_words(text);
    let t = raw.to_lowercase();
    let tokens: Vec<&str> = t
        .split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .collect();
    let destructive = DESTRUCTIVE.iter().any(|w| t.contains(w))
        // SQL keeps its capitals: `TRUNCATE users`.
        || raw.contains("TRUNCATE ")
        || near(&tokens, DELETE_WORDS, &["branch", "branches"], 4)
        || near(&tokens, WIPE_WORDS, DATA_WORDS, 3)
        || near(&tokens, &["prod", "production"], SHIP_WORDS, 5);
    if destructive {
        return Stakes::Destructive;
    }
    let security = SECURITY.iter().any(|w| hits(&t, &tokens, &[w]) > 0)
        || near(&tokens, &["password", "passwords"], PASSWORD_WITH, 3);
    if security {
        Stakes::Security
    } else {
        Stakes::None
    }
}

/// "Try again", "regenerate": the user wants another answer to the same ask,
/// so the next one starts a rung higher (E3). Only a short turn that is just that.
pub fn asks_redo(text: &str) -> bool {
    const REDO: &[&str] = &[
        "try again",
        "regenerate",
        "redo that",
        "redo it",
        "another answer",
        "one more try",
    ];
    let t = own_words(text).trim().to_lowercase();
    t.chars().count() <= 60 && REDO.iter().any(|w| t.contains(w))
}

// ---------------------------------------------------------------- features

/// Phrase match: a needle with a space, symbol or more than four letters is a
/// substring; a short word must be a whole token (`ok` never fires in `book`).
fn hits(t: &str, tokens: &[&str], needles: &[&str]) -> u32 {
    needles
        .iter()
        .filter(|w| {
            let w = **w;
            if w.contains(' ') || !w.chars().all(char::is_alphanumeric) {
                t.contains(w)
            } else if w.len() > 4 {
                // Starts a word, so "solve" never fires in "resolve".
                t.match_indices(w).any(|(i, _)| {
                    t[..i]
                        .chars()
                        .next_back()
                        .is_none_or(|c| !c.is_alphanumeric())
                })
            } else {
                tokens.contains(&w)
            }
        })
        .count() as u32
}

#[derive(Debug, Default, Clone, Copy)]
struct Features {
    chars: usize,
    lines: usize,
    questions: u32,
    fenced: u32,
    inline_code: u32,
    code_tokens: u32,
    code_words: u32,
    paths: u32,
    trace: u32,
    broken: u32,
    edit: u32,
    multi: u32,
    plan: u32,
    design: u32,
    math: u32,
    math_symbols: u32,
    digits: u32,
    creative: u32,
    explain: u32,
    research: u32,
    hard: u32,
    constraints: u32,
    steps: u32,
    follow: u32,
    correction: bool,
    proof: bool,
    greeting_only: bool,
    fact_opener: bool,
}

fn features(text: &str) -> Features {
    let raw = text.trim();
    let t = raw.to_lowercase();
    let tokens: Vec<&str> = t
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '!'))
        .filter(|w| !w.is_empty())
        .collect();
    let bare: Vec<String> = tokens
        .iter()
        .map(|w| w.trim_matches(|c: char| c == '!' || c == '\''))
        .map(str::to_string)
        .collect();
    let mut f = Features {
        chars: raw.chars().count(),
        lines: raw.lines().filter(|l| !l.trim().is_empty()).count(),
        questions: raw.matches('?').count() as u32,
        fenced: raw.matches("```").count().div_ceil(2) as u32,
        inline_code: (raw
            .matches('`')
            .count()
            .saturating_sub(raw.matches("```").count() * 3)
            / 2) as u32,
        ..Features::default()
    };
    f.code_tokens = CODE_TOKENS.iter().filter(|w| raw.contains(**w)).count() as u32;
    f.code_words = hits(&t, &tokens, CODE_WORDS);
    f.paths = raw
        .split_whitespace()
        .filter(|w| {
            let w = w.trim_matches(|c: char| "`'\"(),:;".contains(c));
            let lw = w.to_ascii_lowercase();
            CODE_EXTS
                .iter()
                .any(|e| lw.ends_with(e) && lw.len() > e.len())
                || ((w.contains('/') || w.contains('\\'))
                    && w.contains('.')
                    && !lw.starts_with("http"))
        })
        .count() as u32;
    f.trace = hits(&t, &tokens, TRACE);
    f.broken = hits(&t, &tokens, BROKEN);
    f.edit = hits(&t, &tokens, EDIT_VERBS);
    f.multi = hits(&t, &tokens, MULTI);
    f.plan = hits(&t, &tokens, PLAN);
    f.design = hits(&t, &tokens, DESIGN);
    f.math = hits(&t, &tokens, MATH);
    f.math_symbols = raw
        .chars()
        .filter(|c| "=+*/^√∑∫π≤≥≠±×÷".contains(*c))
        .count() as u32
        + raw.matches("\\frac").count() as u32
        + raw.matches("\\sum").count() as u32;
    f.digits = raw.chars().filter(char::is_ascii_digit).count() as u32;
    f.creative = hits(&t, &tokens, CREATIVE);
    f.explain = hits(&t, &tokens, EXPLAIN);
    f.research = hits(&t, &tokens, RESEARCH);
    f.hard = hits(&t, &tokens, HARDNESS);
    f.constraints = raw
        .lines()
        .filter(|l| {
            let l = l.trim_start();
            l.starts_with("- ")
                || l.starts_with("* ")
                || l.starts_with("• ")
                || l.chars().next().is_some_and(|c| c.is_ascii_digit())
                    && (l.contains(". ") || l.contains(") "))
        })
        .count() as u32
        + hits(
            &t,
            &tokens,
            &[
                "must ",
                "make sure",
                "without ",
                "don't ",
                "do not ",
                "only ",
                "never ",
                "always ",
            ],
        );
    f.steps = hits(
        &t,
        &tokens,
        &[
            "then",
            "after that",
            "first",
            "finally",
            "next,",
            "afterwards",
        ],
    );
    f.follow = hits(&t, &tokens, FOLLOW);
    f.correction = CORRECTION.iter().any(|w| t.contains(w));
    f.proof = ["prove", "proof", "theorem", "lemma"]
        .iter()
        .any(|w| t.contains(w));
    f.greeting_only = !bare.is_empty()
        && bare.len() <= 6
        && f.chars <= 48
        && bare
            .iter()
            .all(|w| GREETINGS.contains(&w.as_str()) || w.is_empty());
    f.fact_opener = FACT_OPENERS
        .iter()
        .any(|o| t.starts_with(o) || t.contains(&format!(" {o}")));
    f
}

// ---------------------------------------------------------------- the read

fn score(f: &Features, cx: &AskContext) -> BTreeMap<AskKind, f64> {
    let code = (f.fenced * 3 + f.code_tokens.min(6) + f.paths.min(4) * 2 + f.inline_code.min(3))
        as f64
        + f.code_words.min(4) as f64 * 2.0;
    let codey = code >= 2.0;
    let mut s = BTreeMap::new();
    let short = f.chars <= 90 && f.lines <= 2;

    s.insert(AskKind::Greeting, if f.greeting_only { 10.0 } else { 0.0 });
    s.insert(
        AskKind::QuickFact,
        if f.fact_opener && short && !codey && f.broken == 0 {
            4.0 + f64::from(f.questions.min(1))
                - (f.research + f.math + f.explain + f.creative) as f64 * 2.5
        } else {
            0.0
        },
    );
    s.insert(AskKind::Chat, 1.5);
    s.insert(AskKind::Creative, f.creative as f64 * 3.0);
    s.insert(
        AskKind::Explain,
        f.explain as f64 * 2.0
            + if f.chars > 160 && f.questions > 0 {
                0.5
            } else {
                0.0
            },
    );
    s.insert(
        AskKind::Research,
        f.research as f64 * 1.8 - if codey { 2.0 } else { 0.0 },
    );
    s.insert(
        AskKind::CodeSmall,
        if codey || f.edit > 0 && f.code_words > 0 {
            code * 0.9 + f.edit as f64 * 2.0 - f.paths.saturating_sub(2) as f64 * 2.5
        } else {
            0.0
        },
    );
    s.insert(
        AskKind::CodeMulti,
        if codey || f.code_words > 0 || f.multi >= 2 {
            f.multi as f64 * 3.0 + f.paths.saturating_sub(1) as f64 * 2.5 + code * 0.4
        } else {
            0.0
        },
    );
    s.insert(
        AskKind::Debug,
        f.broken as f64 * 2.5 + f.trace as f64 * 2.0 + if codey { 1.5 } else { 0.0 },
    );
    s.insert(AskKind::Plan, f.plan as f64 * 2.4 + f.steps as f64 * 0.5);
    s.insert(AskKind::Design, f.design as f64 * 2.6);
    s.insert(
        AskKind::Math,
        f.math as f64 * 2.5
            + if f.math_symbols >= 2 && f.digits >= 2 && !codey {
                2.5
            } else {
                0.0
            },
    );
    if cx.plan_mode {
        *s.entry(AskKind::Plan).or_default() += 4.0;
    }
    s
}

/// Difficulty for a kind, from structure, not just length.
fn difficulty(kind: AskKind, f: &Features, cx: &AskContext) -> f64 {
    let base = match kind {
        AskKind::Greeting => 0.0,
        AskKind::QuickFact => 0.08,
        AskKind::Chat => 0.2,
        AskKind::Creative => 0.25,
        AskKind::Research => 0.35,
        AskKind::Explain => 0.35,
        AskKind::CodeSmall => 0.35,
        AskKind::Plan => 0.55,
        AskKind::Design => 0.6,
        AskKind::Debug => 0.55,
        AskKind::Math => 0.45,
        AskKind::CodeMulti => 0.65,
    };
    if kind == AskKind::Greeting {
        return 0.0;
    }
    let mut d = base;
    d += match f.chars {
        0..=120 => 0.0,
        121..=400 => 0.04,
        401..=1_500 => 0.08,
        1_501..=6_000 => 0.12,
        _ => 0.16,
    };
    d += 0.04 * f.hard.min(4) as f64;
    d += 0.03 * f.constraints.min(5) as f64;
    d += 0.03 * f.steps.min(3) as f64;
    d += 0.02 * f.questions.saturating_sub(1).min(3) as f64;
    d += 0.03 * f.paths.min(4) as f64;
    if f.trace > 0 && matches!(kind, AskKind::Debug) {
        d += 0.05 + 0.02 * f.trace.min(4) as f64;
    }
    if matches!(kind, AskKind::Math) {
        if f.math_symbols >= 4 || f.math >= 3 {
            d += 0.08;
        }
        if f.proof {
            d += 0.1;
        }
        if f.chars <= 40 {
            // "solve 3x + 7 = 22": short drills don't need deep thought.
            d -= 0.15;
        }
    }
    d += 0.03 * cx.planned_tools.min(4) as f64;
    if cx.plan_mode {
        d += 0.1;
    }
    if cx.ctx_tokens > 120_000 {
        d += 0.06;
    } else if cx.ctx_tokens > 40_000 {
        d += 0.03;
    }
    if f.correction {
        d += 0.1;
    }
    (d.clamp(0.0, 1.0) * 100.0).round() / 100.0
}

/// The effort a kind starts at, moved by difficulty.
///
/// Cheap by default and steep where it pays: the overthinking results show
/// accuracy flat or falling past a modest budget on easy items and rising
/// with budget only on genuinely multi-step ones (math, debugging, design).
fn effort(kind: AskKind, d: f64, f: &Features) -> usize {
    match kind {
        AskKind::Greeting => NONE,
        AskKind::QuickFact => {
            if d < 0.2 {
                NONE
            } else {
                LOW
            }
        }
        AskKind::Chat | AskKind::Creative => {
            if d >= 0.55 {
                MEDIUM
            } else {
                LOW
            }
        }
        AskKind::Research | AskKind::Explain => {
            if d >= 0.7 {
                HIGH
            } else if d >= 0.3 {
                MEDIUM
            } else {
                LOW
            }
        }
        AskKind::CodeSmall => {
            if d >= 0.65 {
                HIGH
            } else if d >= 0.3 || f.fenced > 0 {
                MEDIUM
            } else {
                LOW
            }
        }
        AskKind::Plan | AskKind::Design => {
            if d >= 0.55 {
                HIGH
            } else {
                MEDIUM
            }
        }
        AskKind::Math => {
            if d >= 0.9 {
                XHIGH
            } else if d >= 0.5 {
                HIGH
            } else if d >= 0.3 {
                MEDIUM
            } else {
                LOW
            }
        }
        AskKind::CodeMulti | AskKind::Debug => {
            if d >= 0.9 {
                XHIGH
            } else if d >= 0.5 {
                HIGH
            } else {
                MEDIUM
            }
        }
    }
}

/// A short turn with nothing of its own that leans on the turn before it.
fn is_follow_up(f: &Features, best: f64, best_kind: AskKind) -> bool {
    (f.chars <= 120 && (f.follow > 0 || f.correction) && best < 4.0)
        || (f.correction && f.chars <= 200)
        || (f.chars <= 40 && best_kind == AskKind::Chat && !f.greeting_only)
}

/// Read one turn. Same words and context, same answer.
/// The app's own notes that can lead a typed turn: a workboard resume block
/// and an active skill's steps. Each is followed by a blank line, then the
/// words the user typed.
const RESUME_HEAD: &str = "Resuming workboard card ";
const SKILL_HEAD: &str = "Active skill ";
const SKILL_LAST: &str = "\n## Verify\n";

/// The words the user typed: `text` without a leading resume or skill block.
/// Those blocks describe the work, not the ask, so the read never weighs them.
pub fn own_words(text: &str) -> &str {
    let t = text.trim_start();
    let body = if t.starts_with(RESUME_HEAD) {
        Some(t)
    } else if t.starts_with(SKILL_HEAD) {
        t.find(SKILL_LAST).map(|i| &t[i + SKILL_LAST.len()..])
    } else {
        None
    };
    body.and_then(|b| b.split_once("\n\n"))
        .map_or(text, |(_, rest)| rest.trim_start())
}

pub fn read(text: &str, cx: &AskContext) -> AskRead {
    let text = own_words(text);
    let f = features(text);
    let scores = score(&f, cx);
    let mut ranked: Vec<(AskKind, f64)> = scores.into_iter().collect();
    // Highest score; on a tie the heavier kind wins (later in the enum is
    // more demanding), so a tie never under-serves.
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(b.0.cmp(&a.0)));
    let (mut kind, best) = ranked[0];
    let second = ranked.get(1).map_or(0.0, |r| r.1);
    let mut rules = Vec::new();
    // Code that is broken is debugging, not an edit; a broken multi-file
    // change stays multi-file only when it clearly spans files.
    if kind == AskKind::CodeSmall && (f.broken >= 2 || f.trace >= 1) {
        kind = AskKind::Debug;
    }
    if kind == AskKind::Debug && f.multi >= 2 && f.paths >= 3 {
        kind = AskKind::CodeMulti;
    }
    let mut confidence = if best <= 0.0 {
        0.0
    } else {
        ((best - second) / best).clamp(0.0, 1.0)
    };
    if let Some(prior) = cx.prior.filter(|p| *p != AskKind::Greeting) {
        if is_follow_up(&f, best, kind) && !f.greeting_only {
            // Keep the chat's kind: "and the other one?" after a debug turn is debugging.
            if prior != kind {
                rules.push("ask:follow".to_string());
            }
            kind = prior;
            confidence = confidence.max(0.5);
        }
    }
    let d = difficulty(kind, &f, cx);
    let mut e = effort(kind, d, &f);
    if cx.needs_image && e < LOW {
        // Reading a picture is never a no-thinking turn.
        e = LOW;
    }
    let stakes = stakes(text);
    if let Some(id) = stakes.id() {
        // A3: a wrong answer here costs data or safety: Flagship, at least High.
        e = e.max(HIGH);
        rules.push(format!("ask:stakes:{id}"));
    }
    rules.insert(0, format!("ask:{}", kind.id()));
    rules.push(format!("ask:effort:{}", rung_name(e)));
    AskRead {
        kind,
        stakes,
        d,
        effort: e,
        confidence: (confidence * 100.0).round() / 100.0,
        rules,
    }
}

// ---------------------------------------------------------------- chat memory

/// The last kind per chat, for follow-ups. Bounded; ids only, never text.
static PRIOR: Mutex<Option<PriorKinds>> = Mutex::new(None);
const PRIOR_CAP: usize = 256;

#[derive(Default)]
struct PriorKinds {
    map: BTreeMap<String, AskKind>,
    order: VecDeque<String>,
}

/// The kind the previous turn in `session` was read as.
pub fn prior(session: &str) -> Option<AskKind> {
    if session.is_empty() {
        return None;
    }
    let g = PRIOR.lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref().and_then(|p| p.map.get(session).copied())
}

/// Remember `kind` as this chat's latest.
pub fn remember(session: &str, kind: AskKind) {
    if session.is_empty() {
        return;
    }
    let mut g = PRIOR.lock().unwrap_or_else(|e| e.into_inner());
    let p = g.get_or_insert_with(PriorKinds::default);
    if p.map.insert(session.to_string(), kind).is_none() {
        p.order.push_back(session.to_string());
        while p.order.len() > PRIOR_CAP {
            if let Some(old) = p.order.pop_front() {
                p.map.remove(&old);
            }
        }
    }
}

// ---------------------------------------------------------------- "what model are you?"

/// What a turn asks about the route itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RouteQuestion {
    pub model: bool,
    pub effort: bool,
}

impl RouteQuestion {
    pub fn any(self) -> bool {
        self.model || self.effort
    }
}

const MODEL_Q: &[&str] = &[
    "what model",
    "which model",
    "what llm",
    "which llm",
    "what ai are you",
    "which ai are you",
    "what ai is this",
    "which ai is this",
    "what ai model",
    "model are you",
    "model is this",
    "model is answering",
    "model are you using",
    "model am i talking",
    "model is running",
    "model did you use",
    "model was used",
    "who made you",
    "who built you",
    "who trained you",
    "are you grok",
    "are you gpt",
    "are you chatgpt",
    "are you claude",
    "are you gemini",
    "are you llama",
    "are you local",
    "running locally",
    "on my computer or",
    "what version are you",
    "which version are you",
    "what are you running on",
    "what's powering you",
    "whats powering you",
    "what powers you",
];

const EFFORT_Q: &[&str] = &[
    "what effort",
    "which effort",
    "effort level",
    "reasoning effort",
    "reasoning level",
    "thinking level",
    "how hard are you thinking",
    "how hard did you think",
    "how much did you think",
    "how much are you thinking",
    "how long did you think",
    "are you thinking hard",
    "are you reasoning",
    "did you reason",
    "thinking mode",
    "reasoning mode",
    "what mode are you",
    "which mode are you",
];

/// Does the turn ask which model answers, or at what effort? Only then does
/// the chat learn it (Settings and the chat never show it on their own).
pub fn route_question(text: &str) -> RouteQuestion {
    let t = own_words(text).to_lowercase();
    RouteQuestion {
        model: MODEL_Q.iter().any(|q| t.contains(q)),
        effort: EFFORT_Q.iter().any(|q| t.contains(q)),
    }
}

/// The note the answering model reads when the turn asks about the route:
/// the model this turn was actually sent to and the effort it was sent with.
pub fn route_note(model_desc: &str, effort: Option<&str>) -> String {
    let effort = match effort.map(str::trim).filter(|e| !e.is_empty()) {
        Some("none") => "with reasoning off (no reasoning effort)".to_string(),
        Some(e) => format!("at {} reasoning effort", grokhub_core::effort_label(e)),
        None => "with no reasoning effort setting (this model doesn't take one)".to_string(),
    };
    format!(
        "{NOTE_OPENS}The user asked about the model or effort. This turn is answered by {model_desc}, {effort}. The app picks both on its own for every turn. Say so plainly and truthfully if asked; don't mention this note."
    )
}

const NOTE_OPENS: &str = "[Route] ";

/// Put `note` on the newest user message, inside its `<cabin-volatile>` tail
/// so the cached prefix stays the same. A note already there is replaced, so
/// a fallback can correct it. Nothing else in the request changes.
pub fn add_route_note(input: &mut [crate::client::InputItem], note: &str) {
    use crate::client::{ContentPart, InputItem};
    use crate::prefix_cache::{split_user_turn, user_message_text};
    let Some(content) = input.iter_mut().rev().find_map(|item| match item {
        InputItem::Message { role, content } if role == "user" => Some(content),
        _ => None,
    }) else {
        return;
    };
    let Some(ContentPart::InputText(text)) = content
        .iter_mut()
        .find(|c| matches!(c, ContentPart::InputText(_)))
    else {
        return;
    };
    let (volatile, user) = match split_user_turn(text) {
        Some((v, u)) => (v.to_string(), u.to_string()),
        None => (String::new(), text.clone()),
    };
    let mut kept: Vec<&str> = volatile
        .lines()
        .filter(|l| !l.starts_with(NOTE_OPENS))
        .collect();
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    let mut block = kept.join("\n");
    if !block.is_empty() {
        block.push('\n');
    }
    block.push_str(note.trim());
    *text = user_message_text(&block, &user);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(text: &str) -> AskKind {
        read(text, &AskContext::default()).kind
    }

    fn e(text: &str) -> &'static str {
        read(text, &AskContext::default()).effort_name()
    }

    #[test]
    fn greetings_and_quick_facts_think_little() {
        assert_eq!(k("hey"), AskKind::Greeting);
        assert_eq!(k("thanks!"), AskKind::Greeting);
        assert_eq!(k("gg wp"), AskKind::Greeting);
        assert_eq!(e("hi there"), "none");
        assert_eq!(k("What is the capital of Canada?"), AskKind::QuickFact);
        assert_eq!(e("What is the capital of Canada?"), "none");
        assert_eq!(read("hi", &AskContext::default()).d, 0.0);
    }

    #[test]
    fn code_kinds_split_by_scope_and_breakage() {
        assert_eq!(
            k("Write a Python function that reverses a string"),
            AskKind::CodeSmall
        );
        assert_eq!(
            k("I get this error:\n```\nthread 'main' panicked at src/main.rs:10:5\n```\nwhy?"),
            AskKind::Debug
        );
        assert_eq!(
            k("Refactor the auth flow across the codebase: move src/auth.rs, src/session.rs and src/api/login.rs into a new crate and update every caller"),
            AskKind::CodeMulti
        );
    }

    #[test]
    fn hard_work_gets_high_effort_and_easy_work_does_not() {
        let hard = read(
            "Debug a deadlock: two async tasks lock a Mutex in src/a.rs and src/b.rs in opposite order under load. It's subtle and only in production. Make sure the fix is lock-free and handles edge cases, then add tests.",
            &AskContext::default(),
        );
        assert_eq!(hard.kind, AskKind::Debug);
        assert!(hard.d >= 0.9, "d = {}", hard.d);
        assert_eq!(hard.effort_name(), "xhigh");
        assert_eq!(e("Tell me a joke about creepers"), "low");
        assert_eq!(
            k("Prove that the square root of 2 is irrational."),
            AskKind::Math
        );
        assert_eq!(e("Prove that the square root of 2 is irrational."), "high");
    }

    #[test]
    fn plans_designs_research_and_explanations() {
        assert_eq!(
            k("Make me a plan to get to Diamond in ranked this season, step by step"),
            AskKind::Plan
        );
        assert_eq!(
            k("Design the database schema for a mod manager"),
            AskKind::Design
        );
        assert_eq!(
            k("What are the patch notes for today's update?"),
            AskKind::Research
        );
        assert_eq!(
            k("Explain the difference between TCP and UDP for game netcode"),
            AskKind::Explain
        );
    }

    #[test]
    fn follow_ups_keep_the_chat_kind_and_corrections_lift_difficulty() {
        let cx = AskContext {
            prior: Some(AskKind::Debug),
            ..AskContext::default()
        };
        let r = read("and the other one?", &cx);
        assert_eq!(r.kind, AskKind::Debug);
        assert!(r.rules.contains(&"ask:follow".to_string()));
        let c = read("still broken, same error", &cx);
        assert_eq!(c.kind, AskKind::Debug);
        // A greeting after debugging is still a greeting.
        assert_eq!(read("thanks!", &cx).kind, AskKind::Greeting);
    }

    #[test]
    fn reads_are_deterministic_and_record_rule_ids_only() {
        let t = "Implement a binary search in Rust with tests";
        let a = read(t, &AskContext::default());
        assert_eq!(a, read(t, &AskContext::default()));
        assert_eq!(a.rules[0], format!("ask:{}", a.kind.id()));
        assert!(a.rules.iter().all(|r| r.starts_with("ask:")));
        assert!(a.rules.iter().all(|r| !r.contains("binary")));
    }

    #[test]
    fn lanes_name_class_table_rows() {
        for k in AskKind::ALL {
            assert!(
                super::super::policy::class_row(k.lane()).is_some(),
                "{} has no row",
                k.lane()
            );
            assert_eq!(AskKind::parse(k.id()), Some(k));
        }
    }

    #[test]
    fn route_questions_are_spotted_and_answered_truthfully() {
        assert_eq!(
            route_question("btw what model are you?"),
            RouteQuestion {
                model: true,
                effort: false
            }
        );
        assert!(route_question("What reasoning effort is this on?").effort);
        assert!(!route_question("what's the best model kit for a gundam").any());
        let n = route_note("`grok-4.7` by xAI", Some("high"));
        assert!(n.contains("`grok-4.7` by xAI"));
        assert!(n.contains("High reasoning effort"));
        assert!(route_note("x", None).contains("doesn't take one"));
    }

    #[test]
    fn the_route_note_rides_the_volatile_tail_and_replaces_itself() {
        use crate::client::{ContentPart, InputItem};
        let msg = |t: &str| InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(t.into())],
        };
        let text = |items: &[InputItem]| match items.last() {
            Some(InputItem::Message { content, .. }) => match &content[0] {
                ContentPart::InputText(t) => t.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        let mut input = vec![msg("earlier"), msg("what model are you?")];
        add_route_note(&mut input, "[Route] one");
        assert_eq!(
            text(&input),
            "<cabin-volatile>\n[Route] one\n</cabin-volatile>\n\nwhat model are you?"
        );
        // The visible words are untouched; the first message is untouched.
        assert_eq!(
            crate::prefix_cache::visible_user_text(&text(&input)),
            "what model are you?"
        );
        assert_eq!(text(&input[..1]), "earlier");
        add_route_note(&mut input, "[Route] two");
        assert_eq!(
            text(&input),
            "<cabin-volatile>\n[Route] two\n</cabin-volatile>\n\nwhat model are you?"
        );
        // An existing volatile block keeps its lines.
        let mut input = vec![msg(&crate::prefix_cache::user_message_text(
            "Time: 9am",
            "which effort?",
        ))];
        add_route_note(&mut input, "[Route] three");
        assert_eq!(
            text(&input),
            "<cabin-volatile>\nTime: 9am\n[Route] three\n</cabin-volatile>\n\nwhich effort?"
        );
    }

    #[test]
    fn prior_kinds_are_bounded() {
        remember("ask-test-a", AskKind::Plan);
        assert_eq!(prior("ask-test-a"), Some(AskKind::Plan));
        for i in 0..(PRIOR_CAP + 5) {
            remember(&format!("ask-test-fill-{i}"), AskKind::Chat);
        }
        assert_eq!(prior("ask-test-a"), None);
        assert_eq!(prior(""), None);
    }

    #[test]
    fn app_blocks_ahead_of_a_turn_are_not_read() {
        let resume = "Resuming workboard card w-1 \"Fix the tray icon\" after a restart, from its checkpoint.\n\
             Goal: Fix the blurry tray icon\nNext: Patch the DPI scaling\nRefs: src/tray.rs\n\
             Pick up at Next and do not redo done steps.";
        let turn = grokhub_core::skill::apply_skill_follow(
            "Thanks, where is my save folder?",
            Some(resume),
        );
        assert_eq!(own_words(&turn), "Thanks, where is my save folder?");
        let skill = grokhub_core::skill::SkillMd {
            name: "tidy".into(),
            instructions: "1. Read src/main.rs\n\n2. Refactor the parser".into(),
            pitfalls: "Don't break the build".into(),
            verify: "cargo test passes".into(),
            ..Default::default()
        };
        let block = grokhub_core::skill::skill_follow_block(&skill);
        let turn = grokhub_core::skill::apply_skill_follow("hey there", Some(&block));
        assert_eq!(own_words(&turn), "hey there");
        let cx = AskContext::default();
        assert_eq!(read(&turn, &cx).kind, AskKind::Greeting);
        // Plain turns, and a turn that only mentions a card, are read whole.
        assert_eq!(
            own_words("fix the parser\n\nthen run tests"),
            "fix the parser\n\nthen run tests"
        );
        assert_eq!(
            own_words("Resuming workboard card notes"),
            "Resuming workboard card notes"
        );
        // A model question hidden in the block is not the user's question.
        let turn = grokhub_core::skill::apply_skill_follow(
            "ok go",
            Some("Resuming workboard card w-2 \"Ask what model you are\"."),
        );
        assert!(!route_question(&turn).any());
    }

    #[test]
    fn a3_destructive_turns_take_the_flagship_lane_at_high_or_more() {
        let cases = [
            (
                "write a bash script that runs rm -rf on my old build folders",
                "code_small",
            ),
            ("DROP TABLE users; is that safe to run?", "chat"),
            (
                "how do I TRUNCATE the sessions table in postgres",
                "explain",
            ),
            ("git push --force to main after the rebase", "code_small"),
            ("should I force push my feature branch", "explain"),
            (
                "git reset --hard to the last commit and lose my changes",
                "code_small",
            ),
            ("delete the old release branch on the remote", "chat"),
            ("run the database migration in prod tonight", "chat"),
            ("deploy the new build to production", "chat"),
        ];
        for (text, kind) in cases {
            let r = read(text, &AskContext::default());
            assert_eq!(
                (r.kind.id(), r.stakes, r.lane()),
                (kind, Stakes::Destructive, "prepare:hard"),
                "{text}"
            );
            assert!(r.effort >= HIGH, "{text}: {}", r.effort_name());
            assert!(r.rules.contains(&"ask:stakes:destructive".to_string()));
            assert_eq!(
                super::super::capabilities::BaselineLane::of(r.lane()),
                super::super::capabilities::BaselineLane::Flagship
            );
        }
        // A multi-file turn that is also destructive keeps its kind, takes the
        // stakes lane and keeps its own higher start.
        let hard = read(
            "the migration to production fails with a deadlock across src/db.rs, src/a.rs and src/b.rs, it's subtle, check edge cases carefully",
            &AskContext::default(),
        );
        assert_eq!(
            (hard.kind, hard.stakes, hard.lane(), hard.effort_name()),
            (
                AskKind::CodeMulti,
                Stakes::Destructive,
                "prepare:hard",
                "xhigh"
            )
        );
    }

    #[test]
    fn a3_security_turns_take_the_flagship_lane_at_high_or_more() {
        for text in [
            "how should I hash passwords in rust",
            "add JWT authentication to my axum api",
            "is CVE-2024-3094 a vulnerability in my xz version?",
            "encrypt the save files with AES",
            "where do I keep my api key in a tauri app",
            "fix the file permissions on my ssh folder",
            "chmod 777 on the web root, ok?",
        ] {
            let r = read(text, &AskContext::default());
            assert_eq!(
                (r.stakes, r.lane()),
                (Stakes::Security, "prepare:hard"),
                "{text}"
            );
            assert!(r.effort >= HIGH, "{text}: {}", r.effort_name());
            assert!(r.rules.contains(&"ask:stakes:security".to_string()));
        }
    }

    #[test]
    fn a3_casual_words_are_not_stakes() {
        for (text, lane, effort) in [
            ("drop me a text when you get home", "chat:default", "low"),
            ("what's the production value of a movie like dune", "chat:quick", "none"),
            ("the drop rate on this boss is awful", "chat:default", "low"),
            ("truncate this string to 10 characters in python", "code:edit-small", "medium"),
            ("how do I hash a string in python", "code:edit-small", "medium"),
            ("my security camera app keeps logging me out", "chat:default", "low"),
            ("what's the key to winning more ranked games", "chat:quick", "none"),
            ("delete my old screenshots folder", "chat:default", "low"),
            ("we shipped the production build of the mod last week, any feedback?", "chat:default", "low"),
            ("Refactor the auth flow across the codebase: move src/auth.rs, src/session.rs and src/api/login.rs into a new crate and update every caller", "code:multi-file", "high"),
        ] {
            let r = read(text, &AskContext::default());
            assert_eq!(
                (r.stakes, r.lane(), r.effort_name()),
                (Stakes::None, lane, effort),
                "{text}"
            );
            assert!(!r.rules.iter().any(|x| x.starts_with("ask:stakes")));
        }
        assert_eq!(stakes(""), Stakes::None);
    }

    #[test]
    fn asks_redo_reads_short_retries_only() {
        assert!(asks_redo("try again"));
        assert!(asks_redo("Regenerate that please"));
        assert!(asks_redo("nah, redo it"));
        assert!(!asks_redo("thanks, that worked"));
        assert!(!asks_redo(
            "the installer said to try again later, what does error 0x80070005 mean on windows 11"
        ));
    }
}
