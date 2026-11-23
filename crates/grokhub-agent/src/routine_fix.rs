//! Fixes from repeated repairs (script-first runtime, phase 7). When the
//! same repair worked [`REPEATS`] times on the same version of a routine,
//! the routine is patched with it and offered as a fix, after the patch
//! passes a dry-run replay gate. Nothing is written until the person
//! applies it, and an applied fix goes through the change ledger, so Undo
//! takes it back. A fix to a change step changes the routine's hash, so its
//! consent stops counting until the person allows it again.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use grokhub_core::routine::{self, Routine, StepKind};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::harness::{self, AccessMode, Origin, Span, SELF_REVIEW_SESSION};
use crate::routine_consent::{self, Standing};
use crate::routine_log;
use crate::routine_run::{self, Ended, Fix, RunSpec};

/// Times the same repair must work on one version before it is offered.
pub const REPEATS: usize = 3;
pub const FIXES_FILE: &str = "fixes.json";
/// Span tool of a routine replay.
pub const REPLAY_TOOL: &str = "routine_replay";
const REASON: &str = "fix from repeated repairs";

/// A fix ready to offer: step `step`'s arguments `before`, and `after` as
/// the repair kept running them.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub step: String,
    pub before: Map<String, Value>,
    pub after: Map<String, Value>,
    pub seen: usize,
    pub patched: Routine,
}

/// Fixes the person dismissed or applied (never offered again), and the
/// replay gate's verdicts, so a fix is replayed once.
#[derive(Debug, Default, Serialize, Deserialize)]
struct FixStore {
    #[serde(default)]
    closed: BTreeSet<String>,
    #[serde(default)]
    checked: BTreeMap<String, bool>,
}

fn path(config_dir: &Path) -> PathBuf {
    routine::routines_dir(config_dir).join(FIXES_FILE)
}

fn load(config_dir: &Path) -> FixStore {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn store(config_dir: &Path, s: &FixStore) -> Result<(), String> {
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&path(config_dir), &text)
}

/// One fix to one version of one routine.
fn key(name: &str, hash: &str, fix: &Fix) -> String {
    let args: BTreeMap<&String, &Value> = fix.args.iter().collect();
    let args = serde_json::to_string(&args).unwrap_or_default();
    format!(
        "{name} {hash} {} {}",
        fix.step,
        harness::content_hash(args.as_bytes())
    )
}

fn step_args<'a>(r: &'a Routine, step: &str) -> Option<&'a Map<String, Value>> {
    r.steps
        .iter()
        .find(|s| s.id == step)
        .and_then(|s| match &s.kind {
            StepKind::Tool { args, .. } | StepKind::Script { args, .. } => Some(args),
            _ => None,
        })
}

/// `r` with `fix`'s arguments on its step.
pub fn patch(r: &Routine, fix: &Fix) -> Result<Routine, String> {
    let mut patched = r.clone();
    let step = patched
        .steps
        .iter_mut()
        .find(|s| s.id == fix.step)
        .ok_or_else(|| format!("{} has no step {}", r.name, fix.step))?;
    match &mut step.kind {
        StepKind::Tool { args, .. } | StepKind::Script { args, .. } => *args = fix.args.clone(),
        _ => return Err(format!("step {} takes no arguments", fix.step)),
    }
    Ok(patched)
}

/// The replay gate in front of every fix: the patched routine has no
/// problems, differs from `r` only in that step's arguments, and a dry run
/// of it goes to the end. Dry run only: nothing runs and no model is asked.
/// Every verdict is a span in the self-review session.
pub fn replay_gate(
    config_dir: &Path,
    r: &Routine,
    fix: &Fix,
    patched: &Routine,
    now: u64,
) -> Result<(), String> {
    let verdict = replay(config_dir, r, fix, patched);
    let args = serde_json::json!({ "routine": r.name, "step": fix.step }).to_string();
    let mut span = Span::soft_allow(
        SELF_REVIEW_SESSION,
        REPLAY_TOOL,
        &args,
        "",
        "",
        AccessMode::Supervised,
        "cabin",
    )
    .from_origin(Origin::SelfManage);
    span.ts_ms = now;
    match &verdict {
        Ok(()) => span.result = "pass".into(),
        Err(why) => {
            span.decision = "deny".into();
            span.result = "fail".into();
            span.claim = format!("Replay failed, fix not offered: {why}");
        }
    }
    let _ = harness::append_span(config_dir, &span);
    verdict
}

fn replay(config_dir: &Path, r: &Routine, fix: &Fix, patched: &Routine) -> Result<(), String> {
    let problems = patched.problems(&routine_run::step_tool);
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    // Putting the old arguments back must give `r` again.
    let back = patch(
        patched,
        &Fix {
            step: fix.step.clone(),
            args: step_args(r, &fix.step).cloned().unwrap_or_default(),
        },
    )?;
    if back != *r {
        return Err(format!(
            "it changes more than step {}'s arguments",
            fix.step
        ));
    }
    let dir = routine::routines_dir(config_dir).join(&r.name);
    let stop = || false;
    let report = routine_run::run(&RunSpec {
        routine: patched,
        dir: &dir,
        workspace: &dir,
        inputs: Map::new(),
        dry_run: true,
        approve: None,
        desktop: None,
        stop: &stop,
        brain: None,
        words: None,
    });
    match report.ended {
        Ended::Finished => Ok(()),
        _ => Err(report.text()),
    }
}

/// The fix to offer for `r`, if a repair worked [`REPEATS`] times on this
/// version, the person hasn't closed it, and it passes the replay gate. The
/// gate runs once per fix; its verdict is kept.
pub fn proposal(config_dir: &Path, r: &Routine) -> Option<Proposal> {
    let hash = routine_consent::content_hash(r);
    let mut fixes = load(config_dir);
    let mut counts: Vec<(Fix, usize)> = Vec::new();
    for row in routine_log::read(config_dir) {
        if row.name != r.name || row.hash != hash {
            continue;
        }
        for fix in row.fixes {
            match counts.iter_mut().find(|(f, _)| *f == fix) {
                Some((_, n)) => *n += 1,
                None => counts.push((fix, 1)),
            }
        }
    }
    // The most repeated first; the first seen wins a tie.
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    for (fix, seen) in counts {
        let k = key(&r.name, &hash, &fix);
        if seen < REPEATS || fixes.closed.contains(&k) {
            continue;
        }
        let Ok(patched) = patch(r, &fix) else {
            continue;
        };
        let passed = match fixes.checked.get(&k) {
            Some(passed) => *passed,
            None => {
                let passed =
                    replay_gate(config_dir, r, &fix, &patched, grokhub_core::now_ms()).is_ok();
                fixes.checked.insert(k, passed);
                let _ = store(config_dir, &fixes);
                passed
            }
        };
        if passed {
            return Some(Proposal {
                before: step_args(r, &fix.step).cloned().unwrap_or_default(),
                step: fix.step,
                after: fix.args,
                seen,
                patched,
            });
        }
    }
    None
}

fn find(config_dir: &Path, name: &str) -> Result<Routine, String> {
    crate::routine_triggers::routines(config_dir)
        .into_iter()
        .map(|(_, r)| r)
        .find(|r| r.name == name)
        .ok_or_else(|| format!("No routine named {name}"))
}

fn close(config_dir: &Path, r: &Routine, p: &Proposal) -> Result<(), String> {
    let mut fixes = load(config_dir);
    let fix = Fix {
        step: p.step.clone(),
        args: p.after.clone(),
    };
    fixes
        .closed
        .insert(key(&r.name, &routine_consent::content_hash(r), &fix));
    store(config_dir, &fixes)
}

/// Applies the offered fix to routine `name`, from the person's click. The
/// gate runs again first; the save goes through the change ledger.
pub fn apply(config_dir: &Path, name: &str) -> Result<String, String> {
    let r = find(config_dir, name)?;
    let p = proposal(config_dir, &r).ok_or_else(|| format!("No fix to apply to {name}"))?;
    let fix = Fix {
        step: p.step.clone(),
        args: p.after.clone(),
    };
    replay_gate(config_dir, &r, &fix, &p.patched, grokhub_core::now_ms())?;
    crate::routine_draft::save(config_dir, &p.patched, REASON)?;
    // Undo brings this version back; the fix isn't offered on it again.
    close(config_dir, &r, &p)?;
    Ok(match routine_consent::standing(config_dir, &p.patched) {
        Standing::Edited => format!(
            "Fixed step {} in {name}. Its changes need your OK again before they run on their own.",
            p.step
        ),
        _ => format!("Fixed step {} in {name}.", p.step),
    })
}

/// Stops offering routine `name`'s fix.
pub fn dismiss(config_dir: &Path, name: &str) -> Result<(), String> {
    let r = find(config_dir, name)?;
    let p = proposal(config_dir, &r).ok_or_else(|| format!("No fix to dismiss for {name}"))?;
    close(config_dir, &r, &p)
}

#[cfg(test)]
#[path = "routine_fix_tests.rs"]
mod tests;
