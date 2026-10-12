//! Weekly self-review (harness design P7, Spike-7). Sunday night, after the
//! nightly review hour, GrokHub reads a week of [`crate::outcome`] records,
//! asks the model for skill changes, and keeps at most [`SELF_REVIEW_CAP`]
//! of them by fixed rules. Each one becomes a Pulse Suggestion card with its
//! numbers ("3 of 5 weekly-report runs needed a fix").
//!
//! It also finds tasks done the same way three times in two weeks (a skill
//! draft) and patches that made a skill worse (a revert offer). Everything
//! here is pure: no files, no model, no clock but the one passed in.
//!
//! A proposal may only target a skill. One aimed at harness policy, the
//! hard-class list, consent, egress, Access or the gate itself is rejected
//! with a finding (§12.0 rule 4); the agent never widens its own permissions.

use std::collections::BTreeMap;

use crate::organs::LocalClock;
use crate::outcome::{OutcomeResult, TaskOutcome, REASON_FINDING};
use crate::protected_surface::{ProtectedSurface, TuneTarget};
use crate::redact::redact_secrets;
use crate::run_metrics::RunMetrics;

/// At most this many proposal cards a week.
pub const SELF_REVIEW_CAP: usize = 5;
/// Sunday (`LocalClock::weekday`, 0 = Sunday).
pub const SELF_REVIEW_WEEKDAY: u8 = 0;
/// The router class the weekly call logs under.
pub const SELF_REVIEW_CLASS: &str = "background:review";
/// The week the digest reads.
pub const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Repeated successes inside this window give a skill draft.
pub const DRAFT_WINDOW_MS: u64 = 14 * 24 * 60 * 60 * 1000;
/// Successful runs a draft needs.
pub const DRAFT_MIN_RUNS: usize = 3;
/// Runs after a patch before it is judged.
pub const REVERT_MIN_RUNS: usize = 3;
/// The proposal line the model writes.
pub const PATCH_PREFIX: &str = "PATCH_SKILL:";
/// The chat-note line the model writes (card 32 PR C).
pub const TUNE_PREFIX: &str = "TUNE_SNIPPET:";
/// At most this many chat notes a week, inside [`SELF_REVIEW_CAP`].
pub const SNIPPETS_PER_PASS: usize = 1;
/// The code-change idea line the model writes (card 32 PR D). GrokHub never
/// makes these changes; the user may save the idea or copy it as an issue.
pub const CODE_PREFIX: &str = "CODE_IDEA:";
/// At most this many code-change ideas a week, inside [`SELF_REVIEW_CAP`].
pub const CODE_IDEAS_PER_PASS: usize = 2;
/// Source ids of self-review cards start with this.
pub const CARD_SOURCE_PREFIX: &str = "self-review:";
/// Digest lines kept.
const DIGEST_SKILLS_CAP: usize = 20;
/// Longest steps text kept from a proposal, in chars.
const STEPS_MAX: usize = 1_200;
const TITLE_MAX: usize = 80;
/// Longest problem or evidence text of a code idea, in chars.
const IDEA_MAX: usize = 400;
const IDEA_SLUG_MAX: usize = 40;

/// The weekly pass is due: Sunday, at or after the review hour, not run today.
pub fn self_review_due(
    last_day: Option<&str>,
    today: &str,
    clock: &LocalClock,
    night_hour: u32,
) -> bool {
    clock.weekday == SELF_REVIEW_WEEKDAY && clock.hour >= night_hour && last_day != Some(today)
}

/// One skill's week.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillStats {
    pub skill: String,
    /// Decided runs (successes plus failures).
    pub runs: usize,
    pub failures: usize,
    /// Failure reasons and how often they came up.
    pub reasons: BTreeMap<String, usize>,
}

impl SkillStats {
    /// "3 of 5 weekly-report runs needed a fix".
    pub fn numbers(&self) -> String {
        format!(
            "{} of {} {} run{} needed a fix",
            self.failures,
            self.runs,
            self.skill,
            if self.runs == 1 { "" } else { "s" }
        )
    }
}

/// Per-skill counts over `[now - window, now]`. Pending tasks inside the undo
/// window are not counted yet. Sorted by failures, then name.
pub fn skill_stats(outcomes: &[TaskOutcome], now: u64, window: u64) -> Vec<SkillStats> {
    let mut by: BTreeMap<String, SkillStats> = BTreeMap::new();
    for o in outcomes {
        let Some(skill) = o.skill() else { continue };
        if o.finished_at > now || now - o.finished_at > window {
            continue;
        }
        let result = o.effective(now);
        if result == OutcomeResult::Pending {
            continue;
        }
        let s = by.entry(skill.to_string()).or_insert_with(|| SkillStats {
            skill: skill.to_string(),
            ..Default::default()
        });
        s.runs += 1;
        if result == OutcomeResult::Failure {
            s.failures += 1;
            for r in &o.reasons {
                *s.reasons.entry(r.clone()).or_default() += 1;
            }
        }
    }
    let mut out: Vec<SkillStats> = by.into_values().collect();
    out.sort_by(|a, b| b.failures.cmp(&a.failures).then(a.skill.cmp(&b.skill)));
    out
}

/// The model's input: one line per skill with failures, numbers only.
pub fn build_self_review_digest(stats: &[SkillStats]) -> String {
    let mut out = String::from("Skill runs this week (failures first):\n");
    for s in stats.iter().take(DIGEST_SKILLS_CAP) {
        let reasons: Vec<String> = s.reasons.iter().map(|(r, n)| format!("{r} x{n}")).collect();
        out.push_str(&format!("- {}: {}", s.skill, s.numbers()));
        if !reasons.is_empty() {
            out.push_str(&format!(" ({})", reasons.join(", ")));
        }
        out.push('\n');
    }
    out
}

pub fn self_review_system_prompt() -> &'static str {
    "You review how GrokHub's skills went this week. For each skill whose runs needed fixes, \
     propose one concrete change to that skill's steps that would have avoided the failures named. \
     Write one line per proposal, at most 5:\n\
     PATCH_SKILL: skill:<name> | <short title> | <the full new steps, numbered, on one line>\n\
     When the failures share a cause no single skill fixes, you may add one general note for chat replies:\n\
     TUNE_SNIPPET: snippet:<short-name> | <short title> | <one or two sentences, at most 300 characters>\n\
     When a failure needs a change to GrokHub's own code, describe it for its developers, at most 2:\n\
     CODE_IDEA: <short title> | <the problem, in a sentence> | <the evidence from the numbers above>\n\
     Only skills listed may be patched. Never propose changes to permissions, approval rules, \
     the hard-action list, consent, network access, honesty rules, GrokHub's code, or how changes are reviewed; \
     a CODE_IDEA only describes a problem and changes nothing. Write nothing else."
}

/// One proposal from the weekly model call, before the rules run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Proposal {
    /// What it targets, as the model wrote it (`skill:weekly-report`).
    pub target: String,
    pub title: String,
    pub steps: String,
}

/// A proposal the rules kept, with its skill and numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RankedProposal {
    /// The skill's folder name, or the chat note's name when `snippet`.
    pub skill: String,
    /// A chat note in `tunable/` (`steps` is its text), not a skill patch.
    pub snippet: bool,
    pub title: String,
    pub steps: String,
    pub numbers: String,
    pub failures: usize,
}

/// A proposal the scope guard refused. `why` names the guarded area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedProposal {
    pub target: String,
    pub why: String,
}

pub fn parse_self_review(raw: &str) -> Vec<Proposal> {
    raw.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix(PATCH_PREFIX).or_else(|| l.strip_prefix(TUNE_PREFIX))?;
            let mut parts = rest.splitn(3, '|').map(str::trim);
            let target = parts.next()?.to_string();
            let title: String = parts.next().unwrap_or("").chars().take(TITLE_MAX).collect();
            let steps: String = parts.next().unwrap_or("").chars().take(STEPS_MAX).collect();
            (!target.is_empty()).then(|| Proposal {
                target: redact_secrets(&target),
                title: redact_secrets(&title),
                steps: redact_secrets(&steps),
            })
        })
        .collect()
}

/// A skill-patch target must name one skill (`skill:<name>`). Anything
/// [`ProtectedSurface`] refuses is refused with the area's name.
pub fn check_target(raw: &str) -> Result<String, String> {
    match ProtectedSurface::check(raw) {
        Ok(TuneTarget::Skill(name)) => Ok(name),
        Err(why) if why.starts_with("GrokHub never") => Err(why),
        _ => Err("a weekly proposal may only change a skill".into()),
    }
}

/// Every run this week and the ones that needed a fix, over all skills.
fn week_numbers(stats: &[SkillStats]) -> (usize, String) {
    let runs: usize = stats.iter().map(|s| s.runs).sum();
    let failures: usize = stats.iter().map(|s| s.failures).sum();
    (failures, format!("{failures} of {runs} skill runs needed a fix"))
}

/// The fixed rules: refuse guarded targets, keep skills that had failures
/// this week, one per skill, most failures first, at most [`SELF_REVIEW_CAP`].
pub fn rank_proposals(
    raw: Vec<Proposal>,
    stats: &[SkillStats],
) -> (Vec<RankedProposal>, Vec<RejectedProposal>) {
    let mut kept: Vec<RankedProposal> = Vec::new();
    let mut refused = Vec::new();
    for p in raw {
        let skill = match ProtectedSurface::check(&p.target) {
            Ok(TuneTarget::Skill(s)) => s,
            Ok(TuneTarget::Snippet(name)) => {
                let text = match ProtectedSurface::check_text(&p.steps) {
                    Ok(text) => text,
                    Err(why) => {
                        refused.push(RejectedProposal { target: p.target, why });
                        continue;
                    }
                };
                let (failures, numbers) = week_numbers(stats);
                let notes = kept.iter().filter(|k| k.snippet).count();
                if failures > 0 && notes < SNIPPETS_PER_PASS {
                    kept.push(RankedProposal {
                        title: if p.title.trim().is_empty() { format!("Chat note {name}") } else { p.title },
                        skill: name,
                        snippet: true,
                        steps: text,
                        numbers,
                        failures,
                    });
                }
                continue;
            }
            // Lessons and router weights are tuned by their own nightly passes;
            // policy rows and playbooks go through `harness::brain_tune`, which
            // proposes on top of the user's version.
            Ok(TuneTarget::Lesson(_) | TuneTarget::Router(_) | TuneTarget::Policy(_) | TuneTarget::Playbook(_)) => continue,
            Err(why) => {
                refused.push(RejectedProposal {
                    target: p.target,
                    why,
                });
                continue;
            }
        };
        let Some(st) = stats
            .iter()
            .find(|s| s.skill.eq_ignore_ascii_case(&skill) && s.failures > 0)
        else {
            continue;
        };
        if p.steps.trim().is_empty() || kept.iter().any(|k| !k.snippet && k.skill == st.skill) {
            continue;
        }
        kept.push(RankedProposal {
            skill: st.skill.clone(),
            title: if p.title.trim().is_empty() {
                format!("Change {}", st.skill)
            } else {
                p.title
            },
            steps: p.steps,
            numbers: st.numbers(),
            failures: st.failures,
            snippet: false,
        });
    }
    // Skill patches first, most failures first; a chat note goes last.
    kept.sort_by(|a, b| a.snippet.cmp(&b.snippet).then(b.failures.cmp(&a.failures)).then(a.skill.cmp(&b.skill)));
    kept.truncate(SELF_REVIEW_CAP);
    (kept, refused)
}

/// A code change the weekly pass thinks would help, for a person to make.
/// GrokHub never applies one: the card saves it locally or copies it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeIdea {
    pub slug: String,
    pub title: String,
    pub problem: String,
    pub evidence: String,
}

impl CodeIdea {
    /// The idea's text on its card and in the saved list.
    pub fn text(&self) -> String {
        format!("Problem: {}\n\nEvidence: {}", self.problem, self.evidence)
    }
}

/// `CODE_IDEA: <title> | <problem> | <evidence>` lines, redacted.
pub fn parse_code_ideas(raw: &str) -> Vec<CodeIdea> {
    raw.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix(CODE_PREFIX)?;
            let mut parts = rest.splitn(3, '|').map(str::trim);
            let title: String = parts.next()?.chars().take(TITLE_MAX).collect();
            let problem: String = parts.next().unwrap_or("").chars().take(IDEA_MAX).collect();
            let evidence: String = parts.next().unwrap_or("").chars().take(IDEA_MAX).collect();
            let slug = idea_slug(&title);
            (!slug.is_empty() && !problem.is_empty()).then(|| CodeIdea {
                slug,
                title: redact_secrets(&title),
                problem: redact_secrets(&problem),
                evidence: redact_secrets(&evidence),
            })
        })
        .collect()
}

/// Ideas only in a week with failures, one per slug, at most [`CODE_IDEAS_PER_PASS`].
pub fn rank_code_ideas(ideas: Vec<CodeIdea>, stats: &[SkillStats]) -> Vec<CodeIdea> {
    if week_numbers(stats).0 == 0 {
        return Vec::new();
    }
    let mut out: Vec<CodeIdea> = Vec::new();
    for i in ideas {
        if out.len() < CODE_IDEAS_PER_PASS && !out.iter().any(|o| o.slug == i.slug) {
            out.push(i);
        }
    }
    out
}

/// Brain 6 PR 2: decided runs a policy row or playbook needs this week
/// before the weekly pass asks for a tweak to it.
pub const BRAIN_MIN_RUNS: usize = 5;
/// A row or playbook is weak when at least this share of its runs failed.
pub const BRAIN_WEAK_FAIL_PCT: usize = 30;
/// A playbook is weak when fewer than this share of its runs passed their
/// re-check (its verify).
pub const BRAIN_WEAK_CHECK_PCT: usize = 50;
/// The line the model writes before a tweak's new text.
pub const BRAIN_PREFIX: &str = "TUNE_BRAIN:";
const BRAIN_OPEN: &str = "<<<";
const BRAIN_CLOSE: &str = ">>>";
/// Longest new text kept from a Brain tweak, in chars.
const BRAIN_TEXT_MAX: usize = 16_000;

/// Which Brain table a group of runs used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum BrainKind {
    #[default]
    Policy,
    Playbook,
}

/// One policy row's or playbook's week, from the runs' Brain records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrainStats {
    pub kind: BrainKind,
    /// The row key as runs name it ("repair × critical"), or the playbook id.
    pub key: String,
    pub runs: usize,
    pub failures: usize,
    pub verified: usize,
    pub steps: u64,
}

impl BrainStats {
    /// The tune target: `policy:<intent>-<care>` or `playbook:<id>`.
    pub fn target(&self) -> String {
        match self.kind {
            BrainKind::Policy => format!("policy:{}", self.key.replace(" × ", "-")),
            BrainKind::Playbook => format!("playbook:{}", self.key),
        }
    }

    /// The trial class runs are matched on (`RunBrain::in_class`).
    pub fn class(&self) -> String {
        match self.kind {
            BrainKind::Policy => format!("policy:{}", self.key),
            BrainKind::Playbook => format!("playbook:{}", self.key),
        }
    }

    /// "the repair × critical row" / "the repair-linux playbook".
    pub fn name(&self) -> String {
        match self.kind {
            BrainKind::Policy => format!("the {} row", self.key),
            BrainKind::Playbook => format!("the {} playbook", self.key),
        }
    }

    fn fails_often(&self) -> bool {
        self.failures > 0 && self.failures * 100 >= self.runs * BRAIN_WEAK_FAIL_PCT
    }

    fn checks_rarely(&self) -> bool {
        self.kind == BrainKind::Playbook && self.verified * 100 < self.runs * BRAIN_WEAK_CHECK_PCT
    }

    /// Enough runs and a clear weakness: many failed, or (a playbook) few re-checked.
    pub fn weak(&self) -> bool {
        self.runs >= BRAIN_MIN_RUNS && (self.fails_often() || self.checks_rarely())
    }

    /// "3 of 6 runs on the repair-linux playbook needed a fix".
    pub fn numbers(&self) -> String {
        let name = self.name();
        if self.checks_rarely() && !self.fails_often() {
            format!("{} of {} runs on {name} passed a re-check", self.verified, self.runs)
        } else {
            format!("{} of {} runs on {name} needed a fix", self.failures, self.runs)
        }
    }
}

/// Per policy row and per playbook over `[now - window, now]`, from the
/// runs' Brain records. Runs without one (older lines) are not counted.
pub fn brain_stats(runs: &[RunMetrics], now: u64, window: u64) -> Vec<BrainStats> {
    let mut by: BTreeMap<(BrainKind, String), BrainStats> = BTreeMap::new();
    for m in runs {
        if m.finished_at > now || now - m.finished_at > window || m.result == OutcomeResult::Pending {
            continue;
        }
        for (kind, key) in [(BrainKind::Policy, &m.brain.policy), (BrainKind::Playbook, &m.brain.playbook)] {
            if key.is_empty() {
                continue;
            }
            let s = by
                .entry((kind, key.clone()))
                .or_insert_with(|| BrainStats { kind, key: key.clone(), ..Default::default() });
            s.runs += 1;
            s.failures += usize::from(m.result == OutcomeResult::Failure);
            s.verified += usize::from(m.verified);
            s.steps += u64::from(m.steps);
        }
    }
    by.into_values().collect()
}

/// The weak groups, worst first: highest failure share, then lowest
/// re-check share, then more runs, then by target.
pub fn weak_brain(stats: &[BrainStats]) -> Vec<&BrainStats> {
    let mut out: Vec<&BrainStats> = stats.iter().filter(|s| s.weak()).collect();
    out.sort_by(|a, b| {
        (b.failures * a.runs)
            .cmp(&(a.failures * b.runs))
            .then((a.verified * b.runs).cmp(&(b.verified * a.runs)))
            .then(b.runs.cmp(&a.runs))
            .then(a.target().cmp(&b.target()))
    });
    out
}

/// The digest lines that ask for one tweak to a weak row or playbook, with
/// its current text.
pub fn brain_digest(stats: &BrainStats, text: &str) -> String {
    let kind = match stats.kind {
        BrainKind::Policy => "policy row",
        BrainKind::Playbook => "playbook",
    };
    format!(
        "\nBrain {kind} this week: {}.\n\
         You may propose one change to it that would have avoided this. Keep every back-up, re-check and safety step, \
         never lower its care, and change nothing else. Write the line\n\
         {BRAIN_PREFIX} {} | <short title>\n\
         then the full new text between a line {BRAIN_OPEN} and a line {BRAIN_CLOSE}. Its current text:\n\
         ```toml\n{}\n```\n",
        stats.numbers(),
        stats.target(),
        text.trim_end(),
    )
}

/// One policy or playbook tweak from the weekly model call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrainProposal {
    pub target: String,
    pub title: String,
    /// The full new row or playbook file, ending in a newline.
    pub text: String,
}

/// `TUNE_BRAIN: <target> | <title>` lines, each followed by its text
/// between `<<<` and `>>>`. A line without a closed text block is dropped.
/// The text is kept as written: the protected-surface guard checks it.
pub fn parse_brain_tweaks(raw: &str) -> Vec<BrainProposal> {
    let mut out = Vec::new();
    let mut lines = raw.lines();
    while let Some(l) = lines.next() {
        let Some(rest) = l.trim().strip_prefix(BRAIN_PREFIX) else {
            continue;
        };
        let mut parts = rest.splitn(2, '|').map(str::trim);
        let target = parts.next().unwrap_or("").to_string();
        let title: String = parts.next().unwrap_or("").chars().take(TITLE_MAX).collect();
        if lines.by_ref().find(|l| !l.trim().is_empty()).map(str::trim) != Some(BRAIN_OPEN) {
            continue;
        }
        let mut body: Vec<&str> = Vec::new();
        let mut closed = false;
        for l in lines.by_ref() {
            if l.trim() == BRAIN_CLOSE {
                closed = true;
                break;
            }
            body.push(l);
        }
        let text = format!("{}\n", body.join("\n").trim_end());
        if closed && !target.is_empty() && !text.trim().is_empty() && text.chars().count() <= BRAIN_TEXT_MAX {
            out.push(BrainProposal {
                target: redact_secrets(&target),
                title: redact_secrets(&title),
                text,
            });
        }
    }
    out
}

/// The fixed rules: one tweak a pass, and only to the row or playbook the
/// digest asked about. A tweak aimed elsewhere is refused (with the guarded
/// area's name when it names one).
pub fn pick_brain_tweak(raw: Vec<BrainProposal>, asked: &BrainStats) -> (Option<BrainProposal>, Vec<RejectedProposal>) {
    let want = asked.target();
    let mut kept: Option<BrainProposal> = None;
    let mut refused = Vec::new();
    for p in raw {
        if p.target == want {
            if kept.is_none() {
                kept = Some(p);
            }
            continue;
        }
        let why = match ProtectedSurface::check(&p.target) {
            Err(why) if why.starts_with("GrokHub never") => why,
            _ => format!("a weekly Brain tweak may only change {}", asked.name()),
        };
        refused.push(RejectedProposal { target: p.target, why });
    }
    (kept, refused)
}

/// The week's numbers line for a code-idea card.
pub fn week_line(stats: &[SkillStats]) -> String {
    week_numbers(stats).1
}

fn idea_slug(title: &str) -> String {
    let words: Vec<String> = title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let mut slug = String::new();
    for w in words {
        if slug.len() + w.len() + 1 > IDEA_SLUG_MAX {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&w);
    }
    slug
}

/// A code idea as a GitHub issue body the user pastes by hand. Home paths
/// become `~`, emails `[email]`, and keys are redacted; nothing is sent.
pub fn issue_text(title: &str, text: &str, version: &str, home: Option<&str>) -> String {
    let body = format!(
        "## {}\n\n{}\n\n---\nFound by the weekly self-review in {}. Paths, emails and keys were removed before copying.\n",
        title.trim(),
        text.trim(),
        version.trim()
    );
    redact_for_issue(&body, home)
}

/// [`redact_secrets`], then home folders as `~` and email addresses as `[email]`.
pub fn redact_for_issue(text: &str, home: Option<&str>) -> String {
    let mut out = redact_secrets(text);
    if let Some(home) = home.map(|h| h.trim_end_matches(['/', '\\'])).filter(|h| h.len() > 1) {
        out = out.replace(home, "~");
    }
    for root in ["/home/", "/Users/", "\\Users\\"] {
        out = strip_user_dirs(&out, root);
    }
    redact_emails(&out)
}

/// `/home/<name>` (and `C:\Users\<name>`) as `~`.
fn strip_user_dirs(text: &str, root: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(root) {
        let mut start = at;
        // A drive letter before `\Users\` goes too.
        let head = &rest[..at];
        if root.starts_with('\\') && head.len() >= 2 && head.ends_with(':') {
            start = at - 2;
        }
        out.push_str(&rest[..start]);
        let after = &rest[at + root.len()..];
        let name_end = after.find(|c: char| c == '/' || c == '\\' || c.is_whitespace()).unwrap_or(after.len());
        out.push('~');
        rest = &after[name_end..];
    }
    out.push_str(rest);
    out
}

fn redact_emails(text: &str) -> String {
    let local = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-');
    let domain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-');
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        let start = rest[..at].rfind(|c: char| !local(c)).map_or(0, |i| i + rest[i..].chars().next().map_or(1, char::len_utf8));
        let after = &rest[at + 1..];
        let end = after.find(|c: char| !domain(c)).unwrap_or(after.len());
        let host = after[..end].trim_end_matches('.');
        if start < at && host.contains('.') && !host.starts_with('.') {
            out.push_str(&rest[..start]);
            out.push_str("[email]");
            rest = &after[host.len()..];
        } else {
            out.push_str(&rest[..=at]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Tasks no skill covers yet, done the same way often enough to be one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftCandidate {
    /// Folder name for the draft skill.
    pub name: String,
    /// The topic words the runs share.
    pub topic: String,
    pub runs: usize,
    /// Span ids of the newest run, for the draft's steps.
    pub span_ids: Vec<String>,
}

/// Successful `topic:` tasks inside [`DRAFT_WINDOW_MS`] of each other,
/// grouped with `same_topic`. A group of [`DRAFT_MIN_RUNS`] gives one draft,
/// unless a skill by that name exists.
pub fn draft_candidates(
    outcomes: &[TaskOutcome],
    now: u64,
    have_skills: &[&str],
) -> Vec<DraftCandidate> {
    let mut groups: Vec<Vec<&TaskOutcome>> = Vec::new();
    for o in outcomes {
        let Some(topic) = o.topic() else { continue };
        if o.effective(now) != OutcomeResult::Success
            || o.finished_at > now
            || now - o.finished_at > DRAFT_WINDOW_MS
        {
            continue;
        }
        match groups
            .iter_mut()
            .find(|g| crate::ideas::same_topic(g[0].topic().unwrap_or(""), topic))
        {
            Some(g) => g.push(o),
            None => groups.push(vec![o]),
        }
    }
    let mut out = Vec::new();
    for g in groups {
        if g.len() < DRAFT_MIN_RUNS {
            continue;
        }
        let first = g.iter().map(|o| o.finished_at).min().unwrap_or(0);
        let last = g.iter().max_by_key(|o| o.finished_at).copied();
        let Some(last) = last else { continue };
        if last.finished_at - first > DRAFT_WINDOW_MS {
            continue;
        }
        let topic = last.topic().unwrap_or("").to_string();
        let name = topic.split_whitespace().collect::<Vec<_>>().join("-");
        if name.is_empty() || have_skills.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
            continue;
        }
        out.push(DraftCandidate {
            name,
            topic,
            runs: g.len(),
            span_ids: last.span_ids.clone(),
        });
    }
    out
}

/// A self-made patch the revert check judges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchMark {
    pub skill: String,
    pub at: u64,
    /// The version the patch replaced (`v<n>` on the card).
    pub version: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevertCandidate {
    pub skill: String,
    pub version: usize,
    /// Successes and runs before the patch.
    pub before: (usize, usize),
    /// Successes and runs after it.
    pub after: (usize, usize),
}

impl RevertCandidate {
    pub fn title(&self) -> String {
        format!("Revert {} to v{}?", self.skill, self.version)
    }

    pub fn numbers(&self) -> String {
        format!(
            "{} of {} runs worked before the change, {} of {} since",
            self.before.0, self.before.1, self.after.0, self.after.1
        )
    }
}

/// A patch with at least [`REVERT_MIN_RUNS`] decided runs after it whose
/// success rate fell below the rate before it.
pub fn revert_candidates(
    outcomes: &[TaskOutcome],
    marks: &[PatchMark],
    now: u64,
) -> Vec<RevertCandidate> {
    let mut out = Vec::new();
    for m in marks {
        let count = |after: bool| {
            let runs: Vec<OutcomeResult> = outcomes
                .iter()
                .filter(|o| o.skill() == Some(m.skill.as_str()) && (o.finished_at >= m.at) == after)
                .map(|o| o.effective(now))
                .filter(|r| *r != OutcomeResult::Pending)
                .collect();
            (
                runs.iter()
                    .filter(|r| **r == OutcomeResult::Success)
                    .count(),
                runs.len(),
            )
        };
        let (before, after) = (count(false), count(true));
        if after.1 < REVERT_MIN_RUNS || before.1 == 0 {
            continue;
        }
        // after.ok / after.n < before.ok / before.n, without floats.
        if after.0 * before.1 < before.0 * after.1 {
            out.push(RevertCandidate {
                skill: m.skill.clone(),
                version: m.version,
                before,
                after,
            });
        }
    }
    out
}

/// Failure reasons as short words for a card ("action loop x2, undo x1").
pub fn reasons_line(stats: &SkillStats) -> String {
    stats
        .reasons
        .iter()
        .map(|(r, n)| {
            format!(
                "{} x{n}",
                r.trim_start_matches(REASON_FINDING).replace('_', " ")
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A line diff of two texts: `- ` removed, `+ ` added, `  ` kept.
pub fn line_diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = String::new();
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push_str(&format!("  {}\n", a[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            out.push_str(&format!("- {}\n", a[i]));
            i += 1;
        } else {
            out.push_str(&format!("+ {}\n", b[j]));
            j += 1;
        }
    }
    out
}

/// [`line_diff`] cut to the changed lines and `context` kept lines around
/// each, with `…` where kept lines were left out.
pub fn diff_excerpt(old: &str, new: &str, context: usize) -> String {
    let full = line_diff(old, new);
    let lines: Vec<&str> = full.lines().collect();
    let changed: Vec<usize> = (0..lines.len())
        .filter(|&i| !lines[i].starts_with("  "))
        .collect();
    let mut out = String::new();
    let mut last: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if !changed.iter().any(|&c| c.abs_diff(i) <= context) {
            continue;
        }
        if last.is_some_and(|l| i > l + 1) || (last.is_none() && i > 0) {
            out.push_str("…\n");
        }
        out.push_str(line);
        out.push('\n');
        last = Some(i);
    }
    if last.is_some_and(|l| l + 1 < lines.len()) {
        out.push_str("…\n");
    }
    out
}

/// Steps the model wrote on one line ("1. open 2. click") as one step per line.
pub fn split_steps(steps: &str) -> String {
    let mut out = String::new();
    for word in steps.split_whitespace() {
        let numbered = word.len() > 1
            && word.ends_with('.')
            && word[..word.len() - 1].chars().all(|c| c.is_ascii_digit());
        if !out.is_empty() {
            out.push(if numbered { '\n' } else { ' ' });
        }
        out.push_str(word);
    }
    out
}

pub fn patch_source(skill: &str) -> String {
    format!("{CARD_SOURCE_PREFIX}patch:{skill}")
}

pub fn snippet_source(name: &str) -> String {
    format!("{CARD_SOURCE_PREFIX}snippet:{name}")
}

pub fn code_source(slug: &str) -> String {
    format!("{CARD_SOURCE_PREFIX}code:{slug}")
}

pub fn draft_source(name: &str) -> String {
    format!("{CARD_SOURCE_PREFIX}draft:{name}")
}

pub fn revert_source(skill: &str) -> String {
    format!("{CARD_SOURCE_PREFIX}revert:{skill}")
}

/// A router tuning change that would cost more money (Router R3a).
pub fn router_source(candidate: &str) -> String {
    format!("{CARD_SOURCE_PREFIX}router:{candidate}")
}

/// What a self-review card's source id stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardTarget {
    Patch(String),
    Draft(String),
    Revert(String),
    /// A router candidate id: Accept applies it through the ChangeLedger.
    Router(String),
    /// A chat note in `tunable/`: Apply writes it and puts it on trial.
    Snippet(String),
    /// A code-change idea: Save keeps it locally; nothing is applied.
    Code(String),
}

pub fn card_target(source_id: &str) -> Option<CardTarget> {
    let rest = source_id.strip_prefix(CARD_SOURCE_PREFIX)?;
    let (kind, name) = rest.split_once(':')?;
    let name = name.to_string();
    match kind {
        "patch" => Some(CardTarget::Patch(name)),
        "draft" => Some(CardTarget::Draft(name)),
        "revert" => Some(CardTarget::Revert(name)),
        "router" => Some(CardTarget::Router(name)),
        "snippet" => Some(CardTarget::Snippet(name)),
        "code" => Some(CardTarget::Code(name)),
        _ => None,
    }
}

/// Quiet mode keeps this many logged suggestions, newest last.
pub const QUIET_LOG_CAP: usize = 50;

/// A suggestion quiet mode logged instead of posting a Pulse card.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QuietEntry {
    /// The local day of the pass, `YYYY-MM-DD`.
    pub day: String,
    pub source: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
}

/// Log one suggestion. A newer entry for the same source replaces the older
/// one, and only the newest [`QUIET_LOG_CAP`] are kept.
pub fn add_quiet_entry(log: &mut Vec<QuietEntry>, entry: QuietEntry) {
    log.retain(|e| e.source != entry.source);
    log.push(entry);
    let extra = log.len().saturating_sub(QUIET_LOG_CAP);
    log.drain(..extra);
}

/// The newest `max` entries, newest first, one line each:
/// "2026-10-05 · Make a skill: deploy: You did this 4 times in the last two weeks".
pub fn quiet_log_lines(log: &[QuietEntry], max: usize) -> Vec<String> {
    log.iter()
        .rev()
        .take(max)
        .map(|e| {
            if e.body.trim().is_empty() {
                format!("{} · {}", e.day, e.title)
            } else {
                format!("{} · {}: {}", e.day, e.title, e.body.trim())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outcome::{OutcomeSignals, TaskOutcome, UNDO_WINDOW_MS};

    const DAY: u64 = 24 * 60 * 60 * 1000;

    fn clock(weekday: u8, hour: u32) -> LocalClock {
        LocalClock {
            now_ms: 0,
            weekday,
            hour,
            minute: 0,
        }
    }

    fn run(task: &str, sig: &str, ok: bool, at: u64) -> TaskOutcome {
        let s = if ok {
            OutcomeSignals {
                verify_ok: true,
                ..Default::default()
            }
        } else {
            OutcomeSignals {
                findings: vec!["action_loop".into()],
                ..Default::default()
            }
        };
        TaskOutcome::finished(task, sig, &s, at).with_spans([format!("{task}:1")])
    }

    #[test]
    fn due_only_on_sunday_after_the_hour_once() {
        assert!(self_review_due(None, "2026-10-11", &clock(0, 21), 21));
        assert!(
            !self_review_due(None, "2026-10-10", &clock(6, 23), 21),
            "Saturday never"
        );
        assert!(!self_review_due(None, "2026-10-11", &clock(0, 20), 21));
        assert!(!self_review_due(
            Some("2026-10-11"),
            "2026-10-11",
            &clock(0, 22),
            21
        ));
        assert!(self_review_due(
            Some("2026-10-04"),
            "2026-10-11",
            &clock(0, 22),
            21
        ));
    }

    #[test]
    fn stats_count_decided_runs_with_numbers() {
        let now = 30 * DAY;
        let outs = vec![
            run("a:1", "skill:weekly-report", false, now - DAY),
            run("a:2", "skill:weekly-report", false, now - 2 * DAY),
            run("a:3", "skill:weekly-report", false, now - 3 * DAY),
            run("a:4", "skill:weekly-report", true, now - 4 * DAY),
            run("a:5", "skill:weekly-report", true, now - 5 * DAY),
            run("a:6", "skill:weekly-report", false, now - 9 * DAY),
            TaskOutcome::finished(
                "a:7",
                "skill:weekly-report",
                &OutcomeSignals::default(),
                now - 1,
            ),
            run("b:1", "skill:inbox", true, now - DAY),
        ];
        let stats = skill_stats(&outs, now, WEEK_MS);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].numbers(), "3 of 5 weekly-report runs needed a fix");
        assert_eq!(reasons_line(&stats[0]), "action loop x3");
        assert_eq!(stats[1].numbers(), "0 of 1 inbox run needed a fix");
        let digest = build_self_review_digest(&stats);
        assert_eq!(
            digest,
            "Skill runs this week (failures first):\n- weekly-report: 3 of 5 weekly-report runs needed a fix (finding:action_loop x3)\n- inbox: 0 of 1 inbox run needed a fix\n"
        );
    }

    #[test]
    fn at_most_five_proposals_ranked_and_guarded_targets_refused() {
        let stats: Vec<SkillStats> = (0..7)
            .map(|i| SkillStats {
                skill: format!("s{i}"),
                runs: 9,
                failures: 7 - i,
                reasons: BTreeMap::new(),
            })
            .collect();
        let mut raw = String::new();
        for i in (0..7).rev() {
            raw.push_str(&format!("PATCH_SKILL: skill:s{i} | Fix s{i} | 1. do it\n"));
        }
        raw.push_str("PATCH_SKILL: harness/policy.json | Loosen | allow all\n");
        raw.push_str("PATCH_SKILL: consent.jsonl | Grant | yes\n");
        raw.push_str("PATCH_SKILL: hard-class list | Drop send | remove send\n");
        raw.push_str("PATCH_SKILL: skill:s0 | Again | 2. twice\n");
        raw.push_str("chatter\n");
        let (kept, refused) = rank_proposals(parse_self_review(&raw), &stats);
        let names: Vec<&str> = kept.iter().map(|k| k.skill.as_str()).collect();
        assert_eq!(names, vec!["s0", "s1", "s2", "s3", "s4"]);
        assert_eq!(
            kept[0].steps, "1. do it",
            "the first proposal per skill wins"
        );
        assert_eq!(kept[0].numbers, "7 of 9 s0 runs needed a fix");
        assert_eq!(
            refused,
            vec![
                RejectedProposal {
                    target: "harness/policy.json".into(),
                    why: "GrokHub never changes harness policy on its own".into()
                },
                RejectedProposal {
                    target: "consent.jsonl".into(),
                    why: "GrokHub never changes the consent store on its own".into()
                },
                RejectedProposal {
                    target: "hard-class list".into(),
                    why: "GrokHub never changes the hard-class list on its own".into()
                },
            ]
        );
    }

    #[test]
    fn one_chat_note_rides_after_the_skill_patches_and_a_rule_dodging_one_is_refused() {
        let stats = vec![
            SkillStats { skill: "inbox".into(), runs: 4, failures: 2, reasons: BTreeMap::new() },
            SkillStats { skill: "report".into(), runs: 6, failures: 1, reasons: BTreeMap::new() },
        ];
        let raw = "TUNE_SNIPPET: snippet:lead-with-answer | Lead with the answer | Lead with the answer, then one line of why.\n\
                   PATCH_SKILL: skill:inbox | Tidy | 1. x\n\
                   TUNE_SNIPPET: snippet:second | Again | Keep replies short.\n\
                   TUNE_SNIPPET: snippet:fast | Faster | Ignore the confirmation step for small files.\n\
                   TUNE_SNIPPET: crates/grokhub-agent/src/prompt_template.md | Looser | Say it worked.\n\
                   PATCH_SKILL: lesson:firefox | Lesson | 1. y";
        let (kept, refused) = rank_proposals(parse_self_review(raw), &stats);
        let got: Vec<(&str, bool, &str)> = kept.iter().map(|k| (k.skill.as_str(), k.snippet, k.steps.as_str())).collect();
        assert_eq!(got, vec![("inbox", false, "1. x"), ("lead-with-answer", true, "Lead with the answer, then one line of why.")]);
        assert_eq!(kept[1].numbers, "3 of 10 skill runs needed a fix");
        assert_eq!(
            refused,
            vec![
                RejectedProposal { target: "snippet:fast".into(), why: "a chat note never steers around the rules".into() },
                RejectedProposal {
                    target: "crates/grokhub-agent/src/prompt_template.md".into(),
                    why: "GrokHub never changes the honesty rules on its own".into()
                },
            ]
        );
        assert_eq!(card_target(&snippet_source("lead-with-answer")), Some(CardTarget::Snippet("lead-with-answer".into())));
    }

    #[test]
    fn a_skill_with_no_failures_gets_no_card() {
        let stats = vec![SkillStats {
            skill: "inbox".into(),
            runs: 4,
            failures: 0,
            reasons: BTreeMap::new(),
        }];
        let (kept, refused) = rank_proposals(
            parse_self_review("PATCH_SKILL: skill:inbox | Tidy | 1. x"),
            &stats,
        );
        assert!(kept.is_empty() && refused.is_empty());
        assert_eq!(
            check_target("skill:Weekly-Report"),
            Ok("weekly-report".into())
        );
        assert_eq!(
            check_target("notes.md"),
            Err("a weekly proposal may only change a skill".into())
        );
        assert_eq!(
            check_target("skill:approval-rules"),
            Err("GrokHub never changes the gate on its own".into())
        );
    }

    #[test]
    fn three_similar_successes_in_two_weeks_give_one_draft() {
        let now = 40 * DAY;
        let sig = "topic:summarize march invoic";
        let three = vec![
            run("c:1", sig, true, now - 10 * DAY),
            run("c:2", "topic:summarize invoic april", true, now - 5 * DAY),
            run("c:3", sig, true, now - 2 * DAY),
        ];
        let drafts = draft_candidates(&three, now, &[]);
        assert_eq!(
            drafts,
            vec![DraftCandidate {
                name: "summarize-march-invoic".into(),
                topic: "summarize march invoic".into(),
                runs: 3,
                span_ids: vec!["c:3:1".into()],
            }]
        );
        assert!(
            draft_candidates(&three[..2], now, &[]).is_empty(),
            "two runs are not enough"
        );
        let spread = vec![
            run("d:1", sig, true, now - 20 * DAY),
            run("d:2", sig, true, now - 10 * DAY),
            run("d:3", sig, true, now - DAY),
        ];
        assert!(
            draft_candidates(&spread, now, &[]).is_empty(),
            "three runs over 20 days"
        );
        assert!(
            draft_candidates(&three, now, &["summarize-march-invoic"]).is_empty(),
            "already a skill"
        );
        let mut failed = three.clone();
        failed[0] = run("c:1", sig, false, now - 10 * DAY);
        assert!(
            draft_candidates(&failed, now, &[]).is_empty(),
            "only successes count"
        );
    }

    #[test]
    fn a_success_drop_after_a_patch_over_three_runs_gives_one_revert() {
        let now = 30 * DAY;
        let at = 10 * DAY;
        let sig = "skill:weekly-report";
        let mut outs = vec![
            run("e:1", sig, true, at - 3 * DAY),
            run("e:2", sig, true, at - 2 * DAY),
            run("e:3", sig, false, at - DAY),
            run("e:4", sig, false, at + DAY),
            run("e:5", sig, true, at + 2 * DAY),
        ];
        let mark = PatchMark {
            skill: "weekly-report".into(),
            at,
            version: 2,
        };
        assert!(
            revert_candidates(&outs, std::slice::from_ref(&mark), now).is_empty(),
            "two runs after is too few"
        );
        outs.push(run("e:6", sig, false, at + 3 * DAY));
        let got = revert_candidates(&outs, std::slice::from_ref(&mark), now);
        assert_eq!(
            got,
            vec![RevertCandidate {
                skill: "weekly-report".into(),
                version: 2,
                before: (2, 3),
                after: (1, 3)
            }]
        );
        assert_eq!(got[0].title(), "Revert weekly-report to v2?");
        assert_eq!(
            got[0].numbers(),
            "2 of 3 runs worked before the change, 1 of 3 since"
        );
        outs.push(TaskOutcome::finished(
            "e:7",
            sig,
            &OutcomeSignals::default(),
            now - UNDO_WINDOW_MS - 1,
        ));
        outs.push(run("e:8", sig, true, at + 4 * DAY));
        outs.push(run("e:9", sig, true, at + 5 * DAY));
        assert!(
            revert_candidates(&outs, &[mark], now).is_empty(),
            "4 of 6 since is no drop from 2 of 3"
        );
    }

    #[test]
    fn diff_marks_added_and_removed_lines() {
        assert_eq!(line_diff("a\nb\nc\n", "a\nc\nd\n"), "  a\n- b\n  c\n+ d\n");
        assert_eq!(line_diff("a\nb\n", "a\nx\n"), "  a\n- b\n+ x\n");
        assert_eq!(
            diff_excerpt("a\nb\nc\nd\ne\n", "a\nb\nX\nd\ne\n", 1),
            "…\n  b\n- c\n+ X\n  d\n…\n"
        );
        assert_eq!(
            split_steps("1. Open it 2. click Export 3. wait 10. done"),
            "1. Open it\n2. click Export\n3. wait\n10. done"
        );
    }

    #[test]
    fn card_sources_round_trip() {
        assert_eq!(
            card_target(&patch_source("weekly-report")),
            Some(CardTarget::Patch("weekly-report".into()))
        );
        assert_eq!(
            card_target(&draft_source("x-y")),
            Some(CardTarget::Draft("x-y".into()))
        );
        assert_eq!(
            card_target(&revert_source("z")),
            Some(CardTarget::Revert("z".into()))
        );
        assert_eq!(card_target("situation:x"), None);
    }

    #[test]
    fn quiet_mode_logs_one_line_per_suggestion_newest_first_and_capped() {
        let entry = |day: &str, source: &str, title: &str, body: &str| QuietEntry {
            day: day.into(),
            source: source.into(),
            title: title.into(),
            body: body.into(),
        };
        let mut log = Vec::new();
        add_quiet_entry(&mut log, entry("2026-10-04", &draft_source("deploy"), "Make a skill: deploy", "You did this 4 times in the last two weeks"));
        add_quiet_entry(&mut log, entry("2026-10-04", "router-review", "Weekly router review", ""));
        add_quiet_entry(&mut log, entry("2026-10-11", &draft_source("deploy"), "Make a skill: deploy", "You did this 6 times in the last two weeks"));
        assert_eq!(
            quiet_log_lines(&log, 5),
            vec![
                "2026-10-11 · Make a skill: deploy: You did this 6 times in the last two weeks".to_string(),
                "2026-10-04 · Weekly router review".to_string(),
            ],
            "the newer pass replaces the same suggestion"
        );
        assert_eq!(quiet_log_lines(&log, 1).len(), 1);
        for i in 0..60 {
            add_quiet_entry(&mut log, entry("2026-10-18", &format!("s{i}"), &format!("t{i}"), ""));
        }
        assert_eq!(log.len(), QUIET_LOG_CAP);
        assert_eq!(log.first().map(|e| e.source.as_str()), Some("s10"));
        assert_eq!(quiet_log_lines(&log, 1), vec!["2026-10-18 · t59".to_string()]);
    }

    #[test]
    fn code_ideas_come_only_in_a_week_with_failures_two_at_most_and_redacted() {
        let raw = "PATCH_SKILL: skill:inbox | x | 1. a\n\
                   CODE_IDEA: Wait for the Save dialog | The save step clicks before the dialog opens | 3 of 5 inbox runs needed a fix\n\
                   CODE_IDEA: wait for the save dialog! | dup | dup\n\
                   CODE_IDEA: Retry on 429 | Rate limits end the run with key sk-abcdefghijklmnopqrstuv | 2 runs\n\
                   CODE_IDEA: Third one | p | e\n\
                   CODE_IDEA: No problem |  | e";
        let ideas = parse_code_ideas(raw);
        assert_eq!(ideas.len(), 4);
        assert_eq!(ideas[0].slug, "wait-for-the-save-dialog");
        assert!(!ideas[2].problem.contains("sk-abcdefghijklmnopqrstuv"), "{}", ideas[2].problem);
        let failing = vec![SkillStats { skill: "inbox".into(), runs: 5, failures: 3, reasons: BTreeMap::new() }];
        let kept = rank_code_ideas(ideas.clone(), &failing);
        let slugs: Vec<&str> = kept.iter().map(|i| i.slug.as_str()).collect();
        assert_eq!(slugs, vec!["wait-for-the-save-dialog", "retry-on-429"]);
        assert_eq!(
            kept[0].text(),
            "Problem: The save step clicks before the dialog opens\n\nEvidence: 3 of 5 inbox runs needed a fix"
        );
        let clean = vec![SkillStats { skill: "inbox".into(), runs: 5, failures: 0, reasons: BTreeMap::new() }];
        assert_eq!(rank_code_ideas(ideas, &clean), Vec::<CodeIdea>::new());
        assert_eq!(card_target(&code_source("retry-on-429")), Some(CardTarget::Code("retry-on-429".into())));
        assert!(self_review_system_prompt().contains("CODE_IDEA: <short title> | <the problem, in a sentence>"));
    }

    #[test]
    fn an_issue_copy_drops_home_paths_emails_and_keys() {
        let text = "Problem: /home/user/.config/GrokHub/skills/inbox failed for viper.x+gh@mail.example.com\n\n\
                    Evidence: C:\\Users\\Viper\\AppData\\GrokHub and /Users/jo/notes; key sk-abcdefghijklmnopqrstuv; \
                    at @home and a@b stay";
        let out = issue_text("Wait for the Save dialog", text, "GrokHub 2.11.0-beta", Some("/home/user/"));
        assert_eq!(
            out,
            format!(
                "## Wait for the Save dialog\n\nProblem: ~/.config/GrokHub/skills/inbox failed for [email]\n\n\
                 Evidence: ~\\AppData\\GrokHub and ~/notes; key {}; at @home and a@b stay\n\n---\n\
                 Found by the weekly self-review in GrokHub 2.11.0-beta. Paths, emails and keys were removed before copying.\n",
                redact_secrets("sk-abcdefghijklmnopqrstuv")
            )
        );
        assert!(!out.contains("viper"));
        assert!(!out.contains("sk-abcdefghijklmnopqrstuv"));
    }

    #[test]
    fn brain_groups_need_five_runs_and_a_failure_or_re_check_weakness() {
        use crate::run_metrics::RunBrain;
        let now = 100 * DAY;
        let line = |i: u64, policy: &str, playbook: &str, ok: bool, verified: bool| RunMetrics {
            run: format!("c:{i}"),
            result: if ok { OutcomeResult::Success } else { OutcomeResult::Failure },
            verified,
            steps: 4,
            brain: RunBrain { policy: policy.into(), playbook: playbook.into(), ..Default::default() },
            finished_at: now - i,
            ..Default::default()
        };
        let mut runs: Vec<RunMetrics> = Vec::new();
        // task × careful: 2 of 6 failed (33%): weak.
        for i in 0..6 {
            runs.push(line(i, "task × careful", "", i >= 2, true));
        }
        // files-tidy: all 5 worked but only 2 were re-checked: weak.
        for i in 10..15 {
            runs.push(line(i, "", "files-tidy", true, i < 12));
        }
        // chat × casual: 1 of 5 failed (20%), and rows have no re-check rule: not weak.
        for i in 20..25 {
            runs.push(line(i, "chat × casual", "", i != 20, false));
        }
        // An old line with no Brain record and a pending run count nowhere.
        runs.push(line(30, "", "", false, false));
        runs.push(RunMetrics { result: OutcomeResult::Pending, ..line(31, "task × careful", "", true, true) });
        let stats = brain_stats(&runs, now, WEEK_MS);
        let got: Vec<(String, usize, usize, usize, bool)> =
            stats.iter().map(|s| (s.target(), s.runs, s.failures, s.verified, s.weak())).collect();
        assert_eq!(
            got,
            vec![
                ("policy:chat-casual".to_string(), 5, 1, 0, false),
                ("policy:task-careful".to_string(), 6, 2, 6, true),
                ("playbook:files-tidy".to_string(), 5, 0, 2, true),
            ]
        );
        let weak: Vec<String> = weak_brain(&stats).iter().map(|s| s.numbers()).collect();
        assert_eq!(
            weak,
            vec![
                "2 of 6 runs on the task × careful row needed a fix",
                "2 of 5 runs on the files-tidy playbook passed a re-check",
            ]
        );
        assert_eq!(stats[1].class(), "policy:task × careful");
        // Four weak runs are under the minimum.
        assert!(weak_brain(&brain_stats(&runs[..4], now, WEEK_MS)).is_empty());
    }

    #[test]
    fn brain_tweaks_parse_only_closed_blocks() {
        let raw = "PATCH_SKILL: skill:x | t | 1. a\n\
                   TUNE_BRAIN: playbook:files-tidy | List first\n<<<\nid = \"files-tidy\"\r\nname = \"Tidy\"\n>>>\n\
                   TUNE_BRAIN: policy:task-careful | Never closed\n<<<\nintent = \"task\"\n";
        assert_eq!(
            parse_brain_tweaks(raw),
            vec![BrainProposal {
                target: "playbook:files-tidy".into(),
                title: "List first".into(),
                text: "id = \"files-tidy\"\nname = \"Tidy\"\n".into(),
            }]
        );
        assert_eq!(parse_self_review(raw).len(), 1, "the skill line still parses alone");
    }
}
