//! Instant answers (LO-43): a short, plain ask with one obvious read-only
//! answer runs that tool and replies from a fixed template, with no model call.
//!
//! The table is phrases, not patterns: after light clean-up (case, a polite
//! "hey" or "can you", a trailing "please") the whole ask must equal one of a
//! row's phrases, and the turn's Brief must be a question or a task it is
//! sure of. Every listed phrase passes that Brief (a test holds it); asks the
//! Brief sorts as chat ("show my skills", "disk space") are not listed.
//! Anything else, a second clause, a long ask, a background run, an image or
//! a desktop episode, falls through to the model as before. The engine still
//! runs the gate and the PreToolUse hooks on the picked call, and any answer
//! but Run, a missing permission or a failed tool hands the turn to the model.
//!
//! Only tools the gate always runs are listed. Opening apps (needs approval),
//! panels and temperatures are out of scope.

use crate::repair::Os;

use super::{Brief, Intent};

/// The Brief has to be at least this sure of the ask.
pub const MIN_CONFIDENCE: u8 = 80;
/// Longer asks go to the model.
pub const MAX_WORDS: usize = 10;

/// One instant answer: the asks it takes, the call it makes, and the reply.
#[derive(Debug, PartialEq, Eq)]
pub struct Row {
    pub tool: &'static str,
    /// Whole asks after [`normalize`]. Lowercase, single spaces.
    pub phrases: &'static [&'static str],
    /// The call's JSON arguments on Linux; `None` means no instant answer there.
    pub linux: Option<&'static str>,
    /// The same on Windows.
    pub windows: Option<&'static str>,
    /// The reply; `{out}` is the tool's output.
    pub reply: &'static str,
    /// What the tool says when there is nothing to list, and the reply for it.
    pub empty: Option<(&'static str, &'static str)>,
}

pub const ROWS: &[Row] = &[
    Row {
        tool: "diagnose",
        phrases: &[
            "how much disk space do i have",
            "how much disk space do i have left",
            "how much disk space is left",
            "how much disk space is free",
            "how much free disk space do i have",
            "how much space is left on my disk",
            "how much space is left on my drive",
            "how much space is left on my computer",
            "how much space is left on my pc",
            "how much free space is on my disk",
            "how much free space is on my drive",
            "how full is my disk",
            "how full is my drive",
            "is my disk full",
            "is my drive full",
            "what is my disk usage",
        ],
        linux: Some(r#"{"probes":["disk_usage"]}"#),
        windows: Some(r#"{"probes":["volumes"]}"#),
        reply: "{out}",
        empty: None,
    },
    // Windows has no memory probe, so RAM asks there go to the model. "How
    // much RAM do I have" asks for the installed total, which the probe does
    // not report, so it is not listed.
    Row {
        tool: "diagnose",
        phrases: &[
            "how much ram is free",
            "how much ram is left",
            "how much ram am i using",
            "how much ram is being used",
            "how much free ram do i have",
            "how much memory is free",
            "how much memory is left",
            "how much memory am i using",
            "how much memory is being used",
            "how much free memory do i have",
            "what is my ram usage",
            "what is my memory usage",
        ],
        linux: Some(r#"{"probes":["memory"]}"#),
        windows: None,
        reply: "{out}",
        empty: None,
    },
    Row {
        tool: "local_ai_status",
        phrases: &[
            "what is the local ai status",
            "what is my local ai status",
            "is local ai on",
            "is my local ai on",
            "is local ai running",
            "is my local ai running",
            "is the local model loaded",
            "is my local model loaded",
            "is the local model running",
            "is my local model running",
        ],
        linux: Some("{}"),
        windows: Some("{}"),
        reply: "Here's where Local AI stands:\n\n{out}",
        empty: None,
    },
    Row {
        tool: "automation_list",
        phrases: &[
            "list my automations",
            "list automations",
            "what automations do i have",
            "which automations do i have",
            "what automations are set up",
            "what are my automations",
        ],
        linux: Some("{}"),
        windows: Some("{}"),
        reply: "Your automations:\n\n{out}",
        empty: Some(("no automations", "You don't have any automations yet.")),
    },
    // "What are my skills" can be a question about the person, so only
    // asks that name a list or an install are here.
    Row {
        tool: "skill_list",
        phrases: &[
            "list my skills",
            "list skills",
            "what skills are installed",
            "which skills are installed",
            "what skills do i have installed",
        ],
        linux: Some("{}"),
        windows: Some("{}"),
        reply: "Your skills:\n\n{out}",
        empty: Some(("no skills", "You don't have any skills yet.")),
    },
    // Plain "connections" can mean the network, so the asks are narrow.
    Row {
        tool: "connection_list",
        phrases: &[
            "list my connections",
            "list connections",
            "what connections do i have",
            "list my mcp servers",
            "list mcp servers",
            "what mcp servers do i have",
            "which mcp servers do i have",
        ],
        linux: Some("{}"),
        windows: Some("{}"),
        reply: "Your connections:\n\n{out}",
        empty: Some(("no connections", "You don't have any connections yet.")),
    },
];

/// What the turn looks like besides its text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Turn {
    pub image: bool,
    /// A scheduled, proactive, self-manage or `/bg` run.
    pub background: bool,
    /// The turn belongs to a desktop episode.
    pub episode: bool,
}

/// The call an instant answer makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    pub tool: &'static str,
    pub args: &'static str,
    row: &'static Row,
}

impl Pick {
    /// The chat reply for the tool's output.
    pub fn reply(&self, output: &str) -> String {
        let out = output.trim();
        if let Some((marker, reply)) = self.row.empty {
            if out == marker {
                return reply.to_string();
            }
        }
        self.row.reply.replace("{out}", out)
    }
}

/// The instant answer for this ask, or `None` to let the model answer.
pub fn route(text: &str, brief: &Brief, turn: Turn, os: Os) -> Option<Pick> {
    if turn.image || turn.background || turn.episode {
        return None;
    }
    if !matches!(brief.intent, Intent::Question | Intent::Task) || brief.confidence < MIN_CONFIDENCE
    {
        return None;
    }
    if text.split_whitespace().count() > MAX_WORDS || !one_clause(text) {
        return None;
    }
    let ask = normalize(text)?;
    let row = ROWS.iter().find(|r| r.phrases.contains(&ask.as_str()))?;
    let args = match os {
        Os::Linux => row.linux,
        Os::Windows => row.windows,
    }?;
    Some(Pick {
        tool: row.tool,
        args,
        row,
    })
}

/// Words and marks that join a second ask on ("and clean my downloads").
fn one_clause(text: &str) -> bool {
    const JOINERS: &[&str] = &[
        "and", "then", "or", "but", "also", "after", "before", "if", "plus", "while", "so",
        "because", "unless", "when", "&",
    ];
    let lower = text.to_lowercase();
    if lower.contains([',', ';', ':', '\n', '+']) {
        return false;
    }
    !lower.split_whitespace().any(|w| JOINERS.contains(&w))
}

/// The ask as the table spells it, or `None` when it carries anything but
/// letters, digits, spaces and a closing `?`, `!` or `.`.
pub fn normalize(text: &str) -> Option<String> {
    const LEAD: &[&[&str]] = &[
        &["hey"],
        &["hi"],
        &["yo"],
        &["ok"],
        &["okay"],
        &["please"],
        &["pls"],
        &["grok"],
        &["can", "you"],
        &["could", "you"],
        &["would", "you"],
        &["will", "you"],
    ];
    const TRAIL: &[&[&str]] = &[
        &["please"],
        &["pls"],
        &["for", "me"],
        &["thanks"],
        &["thank", "you"],
    ];
    let lower = text.trim().to_lowercase();
    let body = lower.trim_end_matches(['?', '!', '.', ' ']);
    if !body.chars().all(|c| c.is_alphanumeric() || c == ' ') {
        return None;
    }
    let words: Vec<&str> = body.split_whitespace().collect();
    let mut start = 0;
    while let Some(p) = LEAD.iter().find(|p| words[start..].starts_with(p)) {
        start += p.len();
    }
    let mut end = words.len();
    while let Some(p) = TRAIL.iter().find(|p| words[start..end].ends_with(p)) {
        end -= p.len();
    }
    let mut ask = words[start..end].to_vec();
    // "list all my skills" asks the same as "list my skills".
    if ask.starts_with(&["list", "all"]) {
        ask.remove(1);
    }
    (!ask.is_empty()).then(|| ask.join(" "))
}

#[cfg(test)]
#[path = "instant_tests.rs"]
mod tests;
