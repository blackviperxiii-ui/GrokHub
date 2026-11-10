//! What the app learned, in words a person can check.
//!
//! Both apps show the same lines from here: the desktop app as a list in
//! Settings, the cabin as the answer to `/learned`. Each line carries its
//! subject id, so the user's Forget and "That's wrong" name exactly one
//! belief.

use serde::{Deserialize, Serialize};

use super::belief::{Belief, Beliefs, Shape, Standing};
use super::decide::{Area, Dial, TRUST_USAGE};
use super::ladder::{step, Step, Undo};
use super::signal::Subject;

/// Faded evidence below this is not worth a line.
pub const MIN_EVIDENCE: f32 = 0.5;

/// Where a belief stands on the ladder, or why it is off it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Still counting.
    Watching,
    /// Sure enough to offer something.
    Suggesting,
    /// Sure enough to make a one-click-undo change.
    Acting,
    /// A measured number. It informs choices and decides nothing alone.
    Measuring,
    /// The user said it is wrong. It never counts again.
    Wrong,
    /// The user said it is always so.
    Pinned,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Watching => "still watching",
            Stage::Suggesting => "sure enough to suggest",
            Stage::Acting => "sure enough to act",
            Stage::Measuring => "a measurement",
            Stage::Wrong => "marked wrong",
            Stage::Pinned => "you said always",
        }
    }
}

/// One belief as a line.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// The subject id, e.g. `card:suggestion`. Forget and Wrong take it.
    pub key: String,
    /// What the line is about, e.g. `Suggestion cards`.
    pub title: String,
    /// What the app concluded, e.g. `You take these about 8 times in 10.`
    pub says: String,
    /// How many times it was seen, after fading, rounded.
    pub evidence: u32,
    /// 0 to 100: how sure the app is.
    pub sure: u8,
    pub stage: Stage,
    pub last_ms: u64,
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub rows: Vec<Row>,
    /// Single quick chips the app keeps a count for. Their ids are hashes,
    /// so they are counted here and not listed.
    pub single_chips: usize,
    /// The user has learning paused: nothing new is counted.
    pub paused: bool,
}

fn words(id: &str) -> String {
    id.replace(['-', '_', ':', '/'], " ")
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn in_ten(rate: f32) -> u32 {
    (rate * 10.0).round() as u32
}

fn title(area: &str, id: &str) -> String {
    match area {
        "chips" => format!("Quick chips on {}", words(id)),
        "card" => format!("{} cards", capital(&words(id))),
        "card-group" => format!("Cards from {}", words(id)),
        "skill" => format!("Skill {id}"),
        "habit" => match id.rsplit_once('/') {
            Some((skill, part)) => format!(
                "Skill {skill} {}",
                super::habit::Part::parse(part).map_or(part, |p| p.when())
            ),
            None => format!("Habit {id}"),
        },
        "reply" => format!("Plan reply: {}", plan_reply(id)),
        "trait" => format!(
            "One Pulse card {}",
            id.split_once('/').map_or(id, |(kind, _)| kind)
        ),
        "model" => match id.rsplit_once('/') {
            Some((model, "run-ms")) => format!("How long {model} takes"),
            Some((model, "run-usd")) => format!("What {model} costs"),
            _ => format!("Model {id}"),
        },
        _ => capital(&words(&format!("{area} {id}"))),
    }
}

/// A plan reply's words, as on its chip.
fn plan_reply(id: &str) -> String {
    match id.strip_prefix("plan/").unwrap_or(id) {
        "go" => "Yes, go".into(),
        "more" => "Tell me more".into(),
        "step" => "More on a step".into(),
        "change" => "Change this".into(),
        "go-but" => "Go, but\u{2026}".into(),
        "not-now" => "Not now".into(),
        other => words(other),
    }
}

fn says(area: &str, id: &str, belief: &Belief) -> String {
    if belief.standing == Standing::Wrong {
        return "You said this is wrong. It is not counted or used.".into();
    }
    if belief.standing == Standing::Pinned {
        return "You said this is always so. Nothing counts against it.".into();
    }
    if let Some(mean) = belief.level() {
        return match id.rsplit_once('/') {
            Some((_, "run-ms")) => format!("A run takes about {:.1} seconds.", mean / 1000.0),
            Some((_, "run-usd")) => format!("A run costs about ${mean:.3}."),
            _ => format!("The average is {mean:.2}."),
        };
    }
    let n = in_ten(belief.rate().unwrap_or(0.0));
    match area {
        "chips" | "chip" => format!("You tap one about {n} times in 10 when they show."),
        "card" | "card-group" => format!("You take these about {n} times in 10."),
        "reply" => format!("You pick this about {n} times in 10 when a plan waits."),
        "trait" => format!("You like cards with it about {n} times in 10."),
        "model" | "skill" => format!("Runs go well about {n} times in 10."),
        "habit" => format!("About {n} in 10 of its runs happen then."),
        _ => format!("It goes your way about {n} times in 10."),
    }
}

fn stage(belief: &Belief, now_ms: u64) -> Stage {
    if belief.standing == Standing::Wrong {
        return Stage::Wrong;
    }
    if belief.standing == Standing::Pinned {
        return Stage::Pinned;
    }
    if matches!(belief.shape, Shape::Level { .. }) {
        return Stage::Measuring;
    }
    match step(
        belief.confidence(now_ms),
        belief.evidence(now_ms),
        Undo::OneClick,
    ) {
        Step::Watch => Stage::Watching,
        Step::Suggest => Stage::Suggesting,
        Step::Act => Stage::Acting,
    }
}

/// The lines for `beliefs` at `now_ms`: the surest first, what the user
/// marked wrong last. Forgotten and faded-out beliefs have no line.
pub fn view(beliefs: &Beliefs, now_ms: u64) -> View {
    let mut out = View::default();
    for (subject, belief) in beliefs.iter() {
        let wrong = belief.standing == Standing::Wrong;
        if !wrong && belief.evidence(now_ms) < MIN_EVIDENCE {
            continue;
        }
        let (area, id) = subject.as_str().split_once(':').unwrap_or_default();
        if area == "chip" && !wrong {
            out.single_chips += 1;
            continue;
        }
        out.rows.push(Row {
            key: subject.as_str().to_string(),
            title: title(area, id),
            says: says(area, id, belief),
            evidence: belief.evidence(now_ms).round() as u32,
            sure: (belief.confidence(now_ms) * 100.0).round() as u8,
            stage: stage(belief, now_ms),
            last_ms: belief.last_ms,
        });
    }
    out.rows.sort_by(|a, b| {
        (a.stage == Stage::Wrong)
            .cmp(&(b.stage == Stage::Wrong))
            .then(b.sure.cmp(&a.sure))
            .then(a.key.cmp(&b.key))
    });
    out
}

pub const NOTHING_YET: &str =
    "Nothing learned yet. It builds up as you use quick chips, cards and runs.";
pub const HOW_TO_CHANGE: &str = "Forget one with `/learned forget <id>`, rule one out with `/learned wrong <id>`, make one a rule with `/learned always <id>`, or clear all of it with `/learned forget all`. `/learned pause` stops counting until `/learned resume`.";
pub const PAUSED_NOTE: &str =
    "Learning is paused. Nothing new is counted until `/learned resume`.\n\n";
pub const CANT_PIN: &str = "That line is a measurement, so it can't be made a rule.";

/// The lines as Markdown, for the cabin's chat.
pub fn text(view: &View) -> String {
    let paused = if view.paused { PAUSED_NOTE } else { "" };
    if view.rows.is_empty() && view.single_chips == 0 {
        return format!("{paused}{NOTHING_YET}");
    }
    let mut out = format!("{paused}**What I've learned**\n");
    for row in &view.rows {
        let proof = match row.stage {
            Stage::Wrong | Stage::Pinned => String::new(),
            _ => format!(
                " Seen {} times, {}% sure, {}.",
                row.evidence,
                row.sure,
                row.stage.label()
            ),
        };
        out.push_str(&format!(
            "- **{}**: {}{proof} `{}`\n",
            row.title, row.says, row.key
        ));
    }
    if view.single_chips > 0 {
        out.push_str(&format!(
            "- Counts for {} single quick chips, which rank the chip rows.\n",
            view.single_chips
        ));
    }
    out.push('\n');
    out.push_str(HOW_TO_CHANGE);
    out
}

/// What the user typed after `/learned`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    List,
    Forget(Subject),
    Wrong(Subject),
    ForgetAll,
    /// "Always": the line holds whatever is counted.
    Always(Subject),
    Pause,
    Resume,
    /// The dials and the newest decisions.
    Decisions,
    /// Set how far one area may go.
    Trust(Area, Dial),
}

pub const USAGE: &str = "Use `/learned`, `/learned forget <id>`, `/learned wrong <id>`, `/learned always <id>`, `/learned forget all`, `/learned pause`, `/learned resume`, `/learned decisions` or `/learned trust <area> <watch|offer|act>`. The id is the code at the end of each line.";

impl Command {
    pub fn parse(rest: &str) -> Result<Self, String> {
        let mut parts = rest.split_whitespace();
        let (verb, id, extra) = (parts.next(), parts.next(), parts.next());
        let verb = verb.map(str::to_ascii_lowercase);
        if verb.as_deref() == Some("trust") && parts.next().is_none() {
            let area = id.and_then(Area::parse);
            let dial = extra.and_then(Dial::parse);
            return area
                .zip(dial)
                .map(|(area, dial)| Command::Trust(area, dial))
                .ok_or_else(|| TRUST_USAGE.to_string());
        }
        if extra.is_some() {
            return Err(USAGE.into());
        }
        let subject = |id: &str| Subject::parse(id).ok_or_else(|| USAGE.to_string());
        match (verb.as_deref(), id) {
            (None, _) => Ok(Command::List),
            (Some("decisions"), None) => Ok(Command::Decisions),
            (Some("forget"), Some(all)) if all.eq_ignore_ascii_case("all") => {
                Ok(Command::ForgetAll)
            }
            (Some("forget"), Some(id)) => subject(id).map(Command::Forget),
            (Some("wrong"), Some(id)) => subject(id).map(Command::Wrong),
            (Some("always"), Some(id)) => subject(id).map(Command::Always),
            (Some("pause"), None) => Ok(Command::Pause),
            (Some("resume"), None) => Ok(Command::Resume),
            _ => Err(USAGE.into()),
        }
    }
}

/// Runs a `/learned` line on the app's engine and returns what to show.
pub fn run(rest: &str, now_ms: u64) -> String {
    let command = match Command::parse(rest) {
        Ok(command) => command,
        Err(usage) => return usage,
    };
    let done = match &command {
        Command::List => None,
        Command::Decisions => Some(super::hub::decisions_text(now_ms)),
        Command::Trust(area, dial) => {
            super::hub::set_dial(*area, *dial);
            Some(format!(
                "{} is set to {}.",
                area.label(),
                match dial {
                    Dial::Watch => "watch only",
                    Dial::Offer => "offer only",
                    Dial::Act => "act when it's sure",
                }
            ))
        }
        Command::Forget(key) => {
            super::hub::forget(key);
            Some(format!(
                "Forgot `{}`. It starts fresh from here.",
                key.as_str()
            ))
        }
        Command::Wrong(key) => {
            super::hub::mark_wrong(key);
            Some(format!(
                "Marked `{}` wrong. It won't be counted or used again.",
                key.as_str()
            ))
        }
        Command::Always(key) => Some(if super::hub::pin(key) {
            format!(
                "`{}` is a rule now. Nothing counts against it until you forget it.",
                key.as_str()
            )
        } else {
            CANT_PIN.to_string()
        }),
        Command::Pause | Command::Resume => {
            let on = command == Command::Pause;
            Some(match super::hub::set_paused(on) {
                Ok(()) if on => {
                    "Learning is paused. Nothing new is counted until `/learned resume`."
                        .to_string()
                }
                Ok(()) => "Learning is back on.".to_string(),
                Err(err) => format!("Couldn't change that: {err}"),
            })
        }
        Command::ForgetAll => Some(match super::hub::forget_all() {
            Ok(()) => "Forgot everything the app learned.".to_string(),
            Err(err) => format!("Couldn't clear what the app learned: {err}"),
        }),
    };
    done.unwrap_or_else(|| text(&super::hub::view(now_ms)))
}

/// A button on the desktop app's list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Act {
    Forget { key: String },
    Wrong { key: String },
    Always { key: String },
    ForgetAll,
    Pause,
    Resume,
}

pub const NO_SUCH_LINE: &str = "That line is gone. Open the page again.";

/// Runs a button on the app's engine and returns the list as it is after.
pub fn act(act: Act, now_ms: u64) -> Result<View, String> {
    let subject = |key: &str| Subject::parse(key).ok_or_else(|| NO_SUCH_LINE.to_string());
    match act {
        Act::Forget { key } => super::hub::forget(&subject(&key)?),
        Act::Wrong { key } => super::hub::mark_wrong(&subject(&key)?),
        Act::Always { key } => {
            if !super::hub::pin(&subject(&key)?) {
                return Err(CANT_PIN.into());
            }
        }
        Act::ForgetAll => super::hub::forget_all()?,
        Act::Pause => super::hub::set_paused(true)?,
        Act::Resume => super::hub::set_paused(false)?,
    }
    Ok(super::hub::view(now_ms))
}
