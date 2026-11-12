//! The weekly self-review pass, shared by every app on a profile: its
//! last-run guard, the Suggestion cards it posts, and the parts of
//! `self_review.json` it writes. The cabin keeps its own copy of the glue
//! until it is retired; this module uses the cabin's file format and card
//! wording, so either app can run the pass, apply its cards, and see what the
//! other did.
//!
//! The guard is two markers the cabin already leaves:
//! - its scheduler lease (`scheduler-lease.json`): while the cabin's beat is
//!   fresh another app runs no pass (the caller checks
//!   [`grokhub_core::scheduler_lease::shell_may_fire`] first);
//! - `last_day` in `self_review.json`, written before a pass starts.
//!   [`claim_week`] reads it fresh from disk and writes today's date before
//!   the pass, so a cabin that opens later sees the night as done.

use std::path::Path;

use grokhub_core::outcome::TaskOutcome;
use grokhub_core::self_review::{
    self as sr, CodeIdea, DraftCandidate, QuietEntry, RankedProposal, RevertCandidate,
};
use grokhub_core::what_changed as wc;
use grokhub_core::{LocalClock, SkillMd, UpdateCard, UpdateStatus};
use serde_json::{Map, Value};

use super::{span_kind, Span, SpanKind, Version, VersionStatus, WeeklyPass};

/// The weekly pass state in the config dir (the cabin's file).
pub const SELF_REVIEW_FILE: &str = "self_review.json";
/// Saved code ideas kept, newest last.
pub const SAVED_IDEAS_CAP: usize = 50;

/// What one Suggestion card says and what its Apply writes (`md`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProposalCard {
    pub source: String,
    pub skill: String,
    pub title: String,
    pub body: String,
    pub details: String,
    pub md: String,
}

// ---------------------------------------------------------------------------
// self_review.json. Read fresh every time; fields this module doesn't know
// are written back as they were.

fn load(config_dir: &Path) -> Map<String, Value> {
    std::fs::read_to_string(config_dir.join(SELF_REVIEW_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(config_dir: &Path, map: &Map<String, Value>) -> Result<(), String> {
    let text =
        serde_json::to_string_pretty(&Value::Object(map.clone())).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    std::fs::write(config_dir.join(SELF_REVIEW_FILE), text).map_err(|e| e.to_string())
}

fn list<T: serde::de::DeserializeOwned>(map: &Map<String, Value>, key: &str) -> Vec<T> {
    map.get(key)
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn put<T: serde::Serialize>(map: &mut Map<String, Value>, key: &str, value: &T) {
    if let Ok(v) = serde_json::to_value(value) {
        map.insert(key.to_string(), v);
    }
}

/// The local day the pass last started, as either app wrote it.
pub fn last_day(config_dir: &Path) -> Option<String> {
    load(config_dir)
        .get("last_day")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The last-run guard: true when tonight's pass is due and this caller now
/// owns it (today's date is written before it returns). False when it isn't
/// Sunday at or after `night_hour`, or either app already started tonight's.
pub fn claim_week(config_dir: &Path, today: &str, clock: &LocalClock, night_hour: u32) -> bool {
    let mut map = load(config_dir);
    let last = map.get("last_day").and_then(Value::as_str);
    if !sr::self_review_due(last, today, clock, night_hour) {
        return false;
    }
    map.insert("last_day".into(), Value::String(today.to_string()));
    save(config_dir, &map).is_ok()
}

/// One card waiting on the user, in the cabin's `pending` shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Pending {
    pub source: String,
    pub skill: String,
    #[serde(default)]
    pub md: String,
}

pub fn pending(config_dir: &Path) -> Vec<Pending> {
    list(&load(config_dir), "pending")
}

/// What the card for `source` applies, if it is still waiting.
pub fn pending_for(config_dir: &Path, source: &str) -> Option<Pending> {
    pending(config_dir).into_iter().find(|p| p.source == source)
}

/// Remember what a posted card's Apply writes. A newer card for the same
/// source replaces the older one.
pub fn add_pending(config_dir: &Path, p: Pending) -> Result<(), String> {
    let mut map = load(config_dir);
    let mut all: Vec<Pending> = list(&map, "pending");
    all.retain(|q| q.source != p.source);
    all.push(p);
    put(&mut map, "pending", &all);
    save(config_dir, &map)
}

/// The card was applied: forget what it would write.
pub fn remove_pending(config_dir: &Path, source: &str) -> Result<(), String> {
    let mut map = load(config_dir);
    let mut all: Vec<Pending> = list(&map, "pending");
    let before = all.len();
    all.retain(|q| q.source != source);
    if all.len() == before {
        return Ok(());
    }
    put(&mut map, "pending", &all);
    save(config_dir, &map)
}

/// Quiet mode: log the suggestion instead of posting it.
pub fn log_quiet(config_dir: &Path, entry: QuietEntry) -> Result<(), String> {
    let mut map = load(config_dir);
    let mut log: Vec<QuietEntry> = list(&map, "quiet_log");
    sr::add_quiet_entry(&mut log, entry);
    put(&mut map, "quiet_log", &log);
    save(config_dir, &map)
}

/// A code idea the user saved from its card, in the cabin's shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SavedIdea {
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub at: u64,
}

/// A code-idea card's Save: keep it in the local list, change nothing else.
/// A newer save of the same title replaces the older one.
pub fn save_idea(config_dir: &Path, title: &str, text: &str, at: u64) -> Result<(), String> {
    let mut map = load(config_dir);
    let mut ideas: Vec<SavedIdea> = list(&map, "saved_ideas");
    ideas.retain(|i| i.title != title);
    ideas.push(SavedIdea {
        title: title.to_string(),
        text: text.to_string(),
        at,
    });
    let over = ideas.len().saturating_sub(SAVED_IDEAS_CAP);
    ideas.drain(..over);
    put(&mut map, "saved_ideas", &ideas);
    save(config_dir, &map)
}

// ---------------------------------------------------------------------------
// The cards, in the cabin's words.

pub fn revert_card(r: &RevertCandidate) -> ProposalCard {
    ProposalCard {
        source: sr::revert_source(&r.skill),
        skill: r.skill.clone(),
        title: r.title(),
        body: r.numbers(),
        details: format!(
            "{}.\n\nRevert puts back v{} of the skill. /skills changes lists every version.",
            r.numbers(),
            r.version
        ),
        md: String::new(),
    }
}

/// A draft skill from the newest run's steps (tool names only, no args).
pub fn draft_md(cand: &DraftCandidate, spans: &[Span]) -> String {
    let mut steps: Vec<String> = Vec::new();
    for s in spans
        .iter()
        .filter(|s| matches!(span_kind(s), SpanKind::Act | SpanKind::Step))
    {
        if steps.last() != Some(&s.tool) {
            steps.push(s.tool.clone());
        }
    }
    let steps: Vec<String> = steps
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{}. {t}", i + 1))
        .collect();
    grokhub_core::render_skill_md(&SkillMd {
        name: cand.name.clone(),
        description: format!(
            "Done the same way {} times in two weeks: {}",
            cand.runs, cand.topic
        ),
        trigger: cand.topic.clone(),
        instructions: steps.join("\n"),
        ..Default::default()
    })
}

pub fn draft_card(cand: &DraftCandidate, spans: &[Span]) -> ProposalCard {
    let md = draft_md(cand, spans);
    let numbers = format!("You did this {} times in the last two weeks", cand.runs);
    let diff = sr::line_diff("", &md);
    let steps = grokhub_core::parse_skill_md(&md).instructions;
    let details = wc::with_block(
        &wc::from_line_diff(&sr::line_diff("", &steps)),
        &format!("{numbers}. Apply saves it as a new skill you can undo.\n\n{diff}"),
    );
    ProposalCard {
        source: sr::draft_source(&cand.name),
        skill: cand.name.clone(),
        title: format!("Make a skill: {}", cand.name),
        body: numbers,
        details,
        md,
    }
}

/// A kept skill patch: the steps replace the skill's own.
pub fn patch_card(existing: &SkillMd, p: &RankedProposal) -> ProposalCard {
    let mut patched = existing.clone();
    patched.instructions = sr::split_steps(&p.steps);
    if existing.instructions.ends_with('\n') {
        patched.instructions.push('\n');
    }
    let (old, new) = (
        grokhub_core::render_skill_md(existing),
        grokhub_core::render_skill_md(&patched),
    );
    let details = wc::with_block(
        &wc::from_line_diff(&sr::line_diff(
            &existing.instructions,
            &patched.instructions,
        )),
        &format!(
            "{}. Apply changes the skill's steps; you can undo it.\n\n{}",
            p.numbers,
            sr::diff_excerpt(&old, &new, 2)
        ),
    );
    ProposalCard {
        source: sr::patch_source(&p.skill),
        skill: p.skill.clone(),
        title: p.title.clone(),
        body: p.numbers.clone(),
        details,
        md: new,
    }
}

/// A chat-note proposal: Apply writes `tunable/<name>.md` and puts it on trial.
pub fn snippet_card(p: &RankedProposal) -> ProposalCard {
    ProposalCard {
        source: sr::snippet_source(&p.skill),
        skill: p.skill.clone(),
        title: p.title.clone(),
        body: p.numbers.clone(),
        details: format!(
            "{}.\n\nApply adds this note to the instructions of every chat; the safety and approval rules still win:\n\n“{}”\n\nOne chat in five tries it first; {} keeps it only if those runs take fewer steps.",
            p.numbers,
            p.steps,
            grokhub_core::brand::product_name()
        ),
        md: p.steps.clone(),
    }
}

/// A code-change idea: Save keeps it, Copy as GitHub issue copies it redacted.
pub fn code_idea_card(idea: &CodeIdea) -> ProposalCard {
    let text = idea.text();
    ProposalCard {
        source: sr::code_source(&idea.slug),
        skill: idea.slug.clone(),
        title: idea.title.clone(),
        body: idea.evidence.clone(),
        details: format!(
            "{text}\n\n{} never changes its own code. Save idea keeps this in Settings → Your PC → Automations & limits; Copy as GitHub issue puts it on the clipboard with home paths, emails and keys taken out, for you to paste.",
            grokhub_core::brand::product_name()
        ),
        md: text,
    }
}

/// The cards the pass posts before its model call: reverts, then drafts.
/// `skills` are the switched-on skills, by folder name.
pub fn first_cards(
    config_dir: &Path,
    outcomes: &[TaskOutcome],
    skills: &[SkillMd],
    now: u64,
) -> Vec<ProposalCard> {
    let mut out: Vec<ProposalCard> =
        sr::revert_candidates(outcomes, &super::patch_marks(config_dir), now)
            .iter()
            .map(revert_card)
            .collect();
    let have: Vec<String> = skills
        .iter()
        .map(|s| grokhub_core::skill_dir_name(&s.name))
        .collect();
    let have: Vec<&str> = have.iter().map(String::as_str).collect();
    for d in sr::draft_candidates(outcomes, now, &have) {
        let spans = super::spans_for(config_dir, &d.span_ids);
        out.push(draft_card(&d, &spans));
    }
    out
}

/// The cards from the model's answer: kept patches and chat notes in order
/// (a patch for a skill that is gone or off is dropped), then code ideas.
pub fn pass_cards(pass: &WeeklyPass, skills: &[SkillMd]) -> Vec<ProposalCard> {
    let mut out = Vec::new();
    for p in &pass.kept {
        if p.snippet {
            out.push(snippet_card(p));
            continue;
        }
        if let Some(existing) = skills
            .iter()
            .find(|s| grokhub_core::skill_dir_name(&s.name) == p.skill)
        {
            out.push(patch_card(existing, p));
        }
    }
    out.extend(pass.ideas.iter().map(code_idea_card));
    out
}

/// One pass's posting state: the five-card budget, quiet mode and the day
/// quiet mode logs under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    pub left: usize,
    pub quiet: bool,
    pub day: String,
}

impl Budget {
    pub fn new(quiet: bool, day: &str) -> Self {
        Self {
            left: sr::SELF_REVIEW_CAP,
            quiet,
            day: day.to_string(),
        }
    }

    /// Whether any card was spent (the caller saves the feed once).
    pub fn spent(&self) -> bool {
        self.left < sr::SELF_REVIEW_CAP
    }
}

/// Post one card inside the budget, as the cabin does: a live card for the
/// same source is left alone; quiet mode logs it instead and writes nothing
/// to the feed; a posted card's Apply is remembered in `pending`. Returns
/// whether a card landed on `cards`.
pub fn post_card(
    config_dir: &Path,
    cards: &mut Vec<UpdateCard>,
    budget: &mut Budget,
    card: ProposalCard,
    now: u64,
) -> bool {
    if budget.left == 0
        || cards
            .iter()
            .any(|c| c.source_id == card.source && c.status != UpdateStatus::Dismissed)
    {
        return false;
    }
    budget.left -= 1;
    if budget.quiet {
        let _ = log_quiet(
            config_dir,
            QuietEntry {
                day: budget.day.clone(),
                source: card.source,
                title: card.title,
                body: card.body,
            },
        );
        return false;
    }
    let mut update = grokhub_core::suggestion_card(&card.source, &card.title, &card.body, now);
    update.details = Some(card.details);
    let posted = grokhub_core::post_update(cards, update);
    let _ = add_pending(
        config_dir,
        Pending {
            source: card.source,
            skill: card.skill,
            md: card.md,
        },
    );
    posted
}

/// Whether the pass makes its model call: budget left, and a skill that
/// failed this week or a Brain row to ask about.
pub fn wants_model(budget: &Budget, stats: &[sr::SkillStats], brain: bool) -> bool {
    budget.left > 0 && (stats.iter().any(|s| s.failures > 0) || brain)
}

/// What the pass's Brain tweak did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TweakDone {
    /// No budget left: nothing happened.
    Skipped,
    /// Quiet mode logged it; nothing applied.
    Logged,
    /// Applied and on trial: post [`trial_card`] for it.
    Trying(Box<Version>),
    Refused(String),
}

/// The pass's policy or playbook tweak, inside the budget. Quiet mode logs
/// it and applies nothing; otherwise it goes through `brain_tune`.
pub fn brain_step(
    config_dir: &Path,
    tweak: &super::BrainTweak,
    budget: &mut Budget,
    now: u64,
) -> TweakDone {
    if budget.left == 0 {
        return TweakDone::Skipped;
    }
    budget.left -= 1;
    if budget.quiet {
        let _ = log_quiet(
            config_dir,
            QuietEntry {
                day: budget.day.clone(),
                source: tweak.ask.stats.target(),
                title: tweak.proposal.title.clone(),
                body: format!("{}.", tweak.ask.stats.numbers()),
            },
        );
        return TweakDone::Logged;
    }
    match super::apply_brain_tweak(config_dir, &tweak.ask, &tweak.proposal, now) {
        Ok(v) => TweakDone::Trying(Box::new(v)),
        Err(e) => TweakDone::Refused(format!("Self-review tweak not applied: {e}")),
    }
}

/// The feed note for a trial that started or closed, in the cabin's words.
pub fn trial_card(v: &Version) -> UpdateCard {
    let (title, body) = match v.status {
        VersionStatus::Canary => (
            format!("Trying: {}", v.label),
            format!(
                "{}. One run in {} uses it; it stays if it takes fewer {} over {} runs.",
                v.diff,
                100 / u32::from(v.share_pct.max(1)),
                trial_noun(v.target),
                v.min_runs
            ),
        ),
        VersionStatus::Kept => (format!("Kept: {}", v.label), v.note.clone()),
        _ => (format!("Rolled back: {}", v.label), v.note.clone()),
    };
    let source = format!("canary:{}:{}", v.version, v.status.label());
    grokhub_core::router_update_card(&source, &title, &body, v.at)
}

fn trial_noun(m: super::Metric) -> &'static str {
    match m {
        super::Metric::Steps => "steps",
        super::Metric::WallMs => "seconds",
        super::Metric::Tokens => "tokens",
        super::Metric::Cost => "dollars",
    }
}

/// The switched-on skills in `skills_dir`, by name, the list the pass
/// drafts against and patches. A file that holds a secret is skipped.
pub fn cabin_skills(skills_dir: &Path) -> Vec<SkillMd> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(skills_dir) else {
        return out;
    };
    for e in rd.flatten() {
        let Ok(raw) = std::fs::read_to_string(e.path().join("SKILL.md")) else {
            continue;
        };
        if raw.len() > SKILL_MD_MAX || !grokhub_core::skill_safe(&raw) {
            continue;
        }
        let skill = grokhub_core::parse_skill_md(&raw);
        if !crate::skill_is_off_in(skills_dir, &skill.name) {
            out.push(skill);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// A `SKILL.md` larger than this is not read.
const SKILL_MD_MAX: usize = 256 * 1024;

#[cfg(test)]
#[path = "weekly_review_tests.rs"]
mod tests;
