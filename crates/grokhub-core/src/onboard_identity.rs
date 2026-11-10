//! Get to know you: the stage after Auto Setup's first run that drafts
//! `USER.md`, `SOUL.md` and `MEMORY.md` (and, when AMR is on, Personal rows)
//! from a few short answers. Nothing is written until the person reviews the
//! drafts and presses Save; the apps call the writer in
//! `grokhub_agent::onboard`. This module is pure: answers in, drafts out.
//!
//! v1 asks and lets the person edit. It imports nothing from chat exports or
//! connected accounts.

use serde::{Deserialize, Serialize};

use crate::adult_guard::{self, Risk};
use crate::learn_notes::is_placeholder;
use crate::pii::redact_recall;

/// Where the half-done answers wait between launches, under the config dir.
pub const PROGRESS_FILE: &str = "onboard-identity.json";

/// Longest answer kept, in characters (a note line is the same).
pub const ANSWER_MAX: usize = 200;

/// The heading over lines added to a file that already had the person's words.
pub const ADDED_HEADING: &str = "## From get to know you";

/// The steps, in order. `Done` shows after Save.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    #[default]
    Welcome,
    About,
    Soul,
    Life,
    Review,
    Done,
}

/// Whether the stage still waits for the person.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    #[default]
    Pending,
    Done,
    Skipped,
}

/// The person's answers, as typed. Every field may stay empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Answers {
    pub name: String,
    pub call_me: String,
    pub about: String,
    /// `friendly`, `direct`, `playful`, or empty.
    pub tone: String,
    /// "Always" and "never" habits for the assistant.
    pub soul_notes: String,
    pub goals: String,
    pub interests: String,
    pub routines: String,
    pub apps: String,
    /// How they like things done.
    pub prefs: String,
    /// Anything else to keep.
    pub remember: String,
}

/// One Personal row to add when AMR is on. `group` is the key Settings →
/// Personal uses: `about`, `likes`, `routines`, `needs` or `thinking`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub group: String,
    pub text: String,
}

/// What Save writes: the exact file text and the rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Drafts {
    pub user: String,
    pub soul: String,
    pub memory: String,
    pub rows: Vec<Row>,
}

/// The three files as they are on disk now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Existing {
    pub user: String,
    pub soul: String,
    pub memory: String,
}

/// The progress file: where the person is and what they typed so far.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Progress {
    pub stage: Stage,
    pub step: Step,
    pub answers: Answers,
    /// Opened again from Settings, so it shows even over real files.
    pub reopened: bool,
}

/// The groups a row may go under.
pub const ROW_GROUPS: [&str; 5] = ["about", "likes", "routines", "needs", "thinking"];

/// The seed lines the apps write on first launch.
const MEMORY_SEED_LINE: &str = "Long-term notes.";

/// Whether `doc` is empty or still the first-launch seed.
pub fn is_seed(doc: &str) -> bool {
    if is_placeholder(doc) {
        return true;
    }
    let body: Vec<&str> = doc
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    body == [MEMORY_SEED_LINE]
}

/// One safe line from an answer: trimmed, joined onto one line, clipped,
/// with emails, phone numbers and secrets masked. Adult text is dropped.
pub fn clean(text: &str) -> String {
    let joined = text
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    let clipped: String = joined.chars().take(ANSWER_MAX).collect();
    let (masked, _) = redact_recall(clipped.trim());
    if adult_guard::check(&masked) != Risk::Clean {
        return String::new();
    }
    masked
}

/// The tone Auto Setup's Talk answer or a personality id stands for, so the
/// soul step starts from it. Empty for a voice the person made.
pub fn tone_for_personality(id: &str) -> &'static str {
    match id {
        "" | "nyx" | "default" => "friendly",
        "concise-pro" => "direct",
        "sarcastic-sidekick" => "playful",
        _ => "",
    }
}

fn tone_line(tone: &str) -> Option<&'static str> {
    Some(match tone {
        "friendly" => "Tone: friendly. Warm, plain words and a little encouragement.",
        "direct" => "Tone: direct. Short answers, the point first, no filler.",
        "playful" => "Tone: playful. Light jokes, still helpful.",
        _ => return None,
    })
}

fn labelled(out: &mut Vec<String>, label: &str, value: &str) {
    let value = clean(value);
    if !value.is_empty() {
        out.push(format!("{label}: {value}"));
    }
}

/// `existing` with `lines` added: a seed file is replaced by `heading` and
/// the lines; a file with the person's own words keeps every one of them and
/// gets the lines under [`ADDED_HEADING`]. No lines leaves it unchanged.
fn merged(existing: &str, heading: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        return existing.to_string();
    }
    let bullets: String = lines.iter().map(|l| format!("- {l}\n")).collect();
    if is_seed(existing) {
        format!("{heading}\n\n{bullets}")
    } else {
        format!("{}\n\n{ADDED_HEADING}\n\n{bullets}", existing.trim_end())
    }
}

/// The drafts the Review step shows for `answers`. `day` is `YYYY-MM-DD`
/// for the dated `MEMORY.md` lines.
pub fn drafts(answers: &Answers, existing: &Existing, day: &str) -> Drafts {
    let a = answers;
    let mut user = Vec::new();
    labelled(&mut user, "Name", &a.name);
    labelled(&mut user, "Call me", &a.call_me);
    labelled(&mut user, "About me", &a.about);
    labelled(&mut user, "Interests", &a.interests);
    labelled(&mut user, "Apps I use", &a.apps);
    labelled(&mut user, "I like things done", &a.prefs);

    let mut soul = Vec::new();
    if let Some(line) = tone_line(&a.tone) {
        soul.push(line.to_string());
    }
    labelled(&mut soul, "Always / never", &a.soul_notes);

    let mut memory = Vec::new();
    labelled(&mut memory, "Goals", &a.goals);
    labelled(&mut memory, "Routines", &a.routines);
    labelled(&mut memory, "Remember", &a.remember);
    let memory: Vec<String> = memory.into_iter().map(|l| format!("{day}: {l}")).collect();

    Drafts {
        user: merged(&existing.user, "# User", &user),
        soul: merged(&existing.soul, "# Soul", &soul),
        memory: merged(&existing.memory, "# Memory", &memory),
        rows: rows(a),
    }
}

/// The Personal rows for `answers`, one per answered question that is about
/// the person (the soul answers are about the assistant and stay out).
pub fn rows(a: &Answers) -> Vec<Row> {
    let mut out = Vec::new();
    let mut add = |group: &str, label: &str, value: &str| {
        let value = clean(value);
        if !value.is_empty() {
            out.push(Row {
                group: group.into(),
                text: format!("{label}{value}"),
            });
        }
    };
    add("about", "My name is ", &a.name);
    add("about", "Call me ", &a.call_me);
    add("about", "", &a.about);
    add("likes", "Interests: ", &a.interests);
    add("likes", "Apps I use: ", &a.apps);
    add("routines", "", &a.routines);
    add("needs", "Goals: ", &a.goals);
    add("thinking", "", &a.prefs);
    out
}

/// The drafts the person sent back from Review, made safe again: each file
/// line by line, rows one clean line under a known group.
pub fn reviewed(d: &Drafts) -> Drafts {
    let file = |body: &str| -> String {
        let mut out: String = body
            .lines()
            .map(|l| redact_recall(l).0)
            .collect::<Vec<_>>()
            .join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out
    };
    Drafts {
        user: file(&d.user),
        soul: file(&d.soul),
        memory: file(&d.memory),
        rows: d
            .rows
            .iter()
            .filter(|r| ROW_GROUPS.contains(&r.group.as_str()))
            .map(|r| Row {
                group: r.group.clone(),
                text: clean(&r.text),
            })
            .filter(|r| !r.text.is_empty())
            .collect(),
    }
}

/// Whether the stage shows: not finished or skipped, and either opened from
/// Settings, left part way through, or `USER.md` and `SOUL.md` are still seeds.
pub fn pending(progress: &Progress, existing: &Existing) -> bool {
    progress.stage == Stage::Pending
        && (progress.reopened
            || progress.step != Step::Welcome
            || (is_seed(&existing.user) && is_seed(&existing.soul)))
}

#[cfg(test)]
#[path = "onboard_identity_tests.rs"]
mod tests;
