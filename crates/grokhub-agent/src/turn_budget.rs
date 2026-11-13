//! Round 3 item 33, first half: a turn budget learned from past runs.
//!
//! A run's step budget is the 90th percentile of the tool steps past
//! successful runs of the same kind took (the ask's topic signature), or of
//! every successful run when that kind has too few, never under [`FLOOR`].
//! Reaching it doesn't stop the run: it gets one redirect, a note to step
//! back, drop what isn't working and finish the shortest way. With too little
//! history there is no step budget. A run has no token cap; [`crate::stall`]
//! catches one that stops getting anywhere.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use grokhub_core::outcome::{topic_signature, OutcomeResult};
use grokhub_core::run_metrics::{read_run_lines, runs_path, RunMetrics};

/// The fewest steps a learned budget allows.
pub const FLOOR: u32 = 12;
/// Successful runs of one kind before that kind gets its own budget.
pub const CLASS_RUNS: usize = 5;
/// Successful runs in all before any budget is learned.
pub const ALL_RUNS: usize = 10;

/// The 90th percentile (nearest rank) of `steps`.
pub fn p90(mut steps: Vec<u32>) -> Option<u32> {
    if steps.is_empty() {
        return None;
    }
    steps.sort_unstable();
    let rank = (steps.len() * 9).div_ceil(10);
    steps.get(rank.saturating_sub(1)).copied()
}

/// `(class, steps)` of each successful run that took a step, one per run
/// with its newest line winning, as `read_runs` folds them but in one pass.
pub fn successes(lines: Vec<RunMetrics>) -> Vec<(String, u32)> {
    let mut newest: HashMap<String, RunMetrics> = HashMap::new();
    for line in lines {
        match newest.get(&line.run) {
            Some(prev) if prev.at > line.at => {}
            _ => {
                newest.insert(line.run.clone(), line);
            }
        }
    }
    newest
        .into_values()
        .filter(|r| r.result == OutcomeResult::Success && r.steps > 0)
        .map(|r| (r.class, r.steps))
        .collect()
}

/// The step budget for an ask of kind `class`, from successful runs.
pub fn learned(runs: &[(String, u32)], class: &str) -> Option<u32> {
    let mine: Vec<u32> = runs
        .iter()
        .filter(|(c, _)| !class.is_empty() && c == class)
        .map(|(_, n)| *n)
        .collect();
    let steps = if mine.len() >= CLASS_RUNS {
        mine
    } else if runs.len() >= ALL_RUNS {
        runs.iter().map(|(_, n)| *n).collect()
    } else {
        return None;
    };
    p90(steps).map(|n| n.max(FLOOR))
}

type Cached = (PathBuf, Option<SystemTime>, u64, Arc<Vec<(String, u32)>>);

/// The successful runs in `config_dir`, read again only when the file changed.
fn runs(config_dir: &Path) -> Arc<Vec<(String, u32)>> {
    static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
    let path = runs_path(config_dir);
    let meta = std::fs::metadata(&path).ok();
    let modified = meta.as_ref().and_then(|m| m.modified().ok());
    let len = meta.as_ref().map_or(0, std::fs::Metadata::len);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, m, l, runs)) = cache.as_ref() {
        if *p == path && *m == modified && *l == len {
            return Arc::clone(runs);
        }
    }
    let runs = Arc::new(successes(read_run_lines(config_dir)));
    *cache = Some((path, modified, len, Arc::clone(&runs)));
    runs
}

/// The step budget for `ask`, from the runs kept in `config_dir`.
pub fn for_ask(config_dir: &Path, ask: &str) -> Option<u32> {
    learned(&runs(config_dir), &topic_signature(ask))
}

/// One run's steps against its learned budget. The redirect comes once.
#[derive(Debug, Default)]
pub struct TurnBudget {
    steps_budget: Option<u32>,
    steps: u32,
    step_redirected: bool,
}

impl TurnBudget {
    pub fn new(steps_budget: Option<u32>) -> Self {
        Self {
            steps_budget,
            ..Self::default()
        }
    }

    /// `n` more tool steps ran: the redirect note when they reach the budget.
    pub fn after_steps(&mut self, n: u32) -> Option<String> {
        self.steps = self.steps.saturating_add(n);
        let budget = self.steps_budget?;
        if self.step_redirected || self.steps < budget {
            return None;
        }
        self.step_redirected = true;
        Some(format!(
            "Redirect: tasks like this one usually finish within {budget} tool steps, and this \
             run has taken {}. Step back before going on: say in a line what's left, drop any \
             approach that isn't working, and finish the shortest way. If something blocks \
             you, stop and say what.",
            self.steps
        ))
    }
}

#[cfg(test)]
#[path = "turn_budget_tests.rs"]
mod tests;
