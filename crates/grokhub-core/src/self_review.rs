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
use crate::redact::redact_secrets;

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
/// Source ids of self-review cards start with this.
pub const CARD_SOURCE_PREFIX: &str = "self-review:";
/// Digest lines kept.
const DIGEST_SKILLS_CAP: usize = 20;
/// Longest steps text kept from a proposal, in chars.
const STEPS_MAX: usize = 1_200;
const TITLE_MAX: usize = 80;

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
     Only skills listed may be targets. Never propose changes to permissions, approval rules, \
     the hard-action list, consent, network access, or how changes are reviewed. \
     Write nothing else."
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
    pub skill: String,
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
            let rest = l.trim().strip_prefix(PATCH_PREFIX)?;
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

/// Areas a proposal may never touch, by the words that name them.
const GUARDED: &[(&[&str], &str)] = &[
    (&["consent"], "the consent store"),
    (&["egress", "network"], "egress"),
    (
        &[
            "hard-class",
            "hard_class",
            "hardclass",
            "hard class",
            "hard.rs",
            "hard-list",
        ],
        "the hard-class list",
    ),
    (&["harness", "policy", "polic"], "harness policy"),
    (
        &["app.json", "access", "permission", "always", "yolo"],
        "Access",
    ),
    (
        &[
            "gate",
            "decide",
            "approval",
            "self-review",
            "self_review",
            "outcome",
            "replay",
        ],
        "the gate",
    ),
    (&[".grok"], "the Grok CLI home"),
];

/// A proposal target must name one skill (`skill:<name>`). Anything aimed at
/// a guarded area is refused with the area's name.
pub fn check_target(raw: &str) -> Result<String, String> {
    let t = raw.trim().to_ascii_lowercase();
    for (words, why) in GUARDED {
        if words.iter().any(|w| t.contains(w)) {
            return Err(format!("GrokHub never changes {why} on its own"));
        }
    }
    let name = t.strip_prefix("skill:").map(str::trim).unwrap_or("");
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' '));
    if ok {
        Ok(name.to_string())
    } else {
        Err("a weekly proposal may only change a skill".into())
    }
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
        let skill = match check_target(&p.target) {
            Ok(s) => s,
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
        if p.steps.trim().is_empty() || kept.iter().any(|k| k.skill == st.skill) {
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
        });
    }
    kept.sort_by(|a, b| b.failures.cmp(&a.failures).then(a.skill.cmp(&b.skill)));
    kept.truncate(SELF_REVIEW_CAP);
    (kept, refused)
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

pub fn draft_source(name: &str) -> String {
    format!("{CARD_SOURCE_PREFIX}draft:{name}")
}

pub fn revert_source(skill: &str) -> String {
    format!("{CARD_SOURCE_PREFIX}revert:{skill}")
}

/// What a self-review card's source id stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardTarget {
    Patch(String),
    Draft(String),
    Revert(String),
}

pub fn card_target(source_id: &str) -> Option<CardTarget> {
    let rest = source_id.strip_prefix(CARD_SOURCE_PREFIX)?;
    let (kind, name) = rest.split_once(':')?;
    let name = name.to_string();
    match kind {
        "patch" => Some(CardTarget::Patch(name)),
        "draft" => Some(CardTarget::Draft(name)),
        "revert" => Some(CardTarget::Revert(name)),
        _ => None,
    }
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
}
