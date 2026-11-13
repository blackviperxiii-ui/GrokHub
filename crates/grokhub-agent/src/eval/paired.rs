//! Paired A/B statistics for the live eval's `--ab` runs.
//!
//! Both arms run the same tasks, so each task is compared with itself: the
//! per-task difference (on minus off) in solve rate, billed cost and model
//! calls, then the mean of those differences with a 95% interval from
//! Student's t. The decision metric is billed dollars per solved task, cache
//! included, never tokens: cutting tokens can raise the bill.
//!
//! A verdict needs at least [`MIN_PAIRED_TASKS`] paired tasks and a cost
//! interval narrow enough to see the run's minimum detectable effect (set
//! before the run); otherwise the report says there is no verdict and why.

/// Fewer paired tasks than this give no verdict.
pub const MIN_PAIRED_TASKS: usize = 3;
/// The default minimum detectable effect: a 10% change in cost per try.
pub const DEFAULT_MDE: f64 = 0.10;

/// One task's tries in one arm.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ArmTally {
    pub tries: u32,
    pub solved: u32,
    pub cost_ticks: i64,
    pub calls: u32,
}

impl ArmTally {
    pub fn add(&mut self, solved: bool, cost_ticks: i64, calls: u32) {
        self.tries += 1;
        self.solved += u32::from(solved);
        self.cost_ticks += cost_ticks;
        self.calls += calls;
    }

    fn per_try(&self, total: f64) -> f64 {
        if self.tries == 0 {
            0.0
        } else {
            total / f64::from(self.tries)
        }
    }

    pub fn solve_rate(&self) -> f64 {
        self.per_try(f64::from(self.solved))
    }

    pub fn cost_per_try(&self) -> f64 {
        self.per_try(self.cost_ticks as f64)
    }

    pub fn calls_per_try(&self) -> f64 {
        self.per_try(f64::from(self.calls))
    }
}

/// One task with tries in both arms.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub task: String,
    pub on: ArmTally,
    pub off: ArmTally,
}

/// A mean difference and its two-sided 95% interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    pub mean: f64,
    pub low: f64,
    pub high: f64,
}

impl Interval {
    pub fn half_width(&self) -> f64 {
        (self.high - self.low) / 2.0
    }
}

/// Two-sided 95% critical value of Student's t with `df` degrees of
/// freedom. Between table rows it takes the row below, so the interval is
/// never narrower than the true one.
pub fn t95(df: usize) -> f64 {
    const TABLE: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
        2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
        2.052, 2.048, 2.045, 2.042,
    ];
    match df {
        0 => f64::INFINITY,
        1..=30 => TABLE[df - 1],
        31..=40 => 2.042,
        41..=60 => 2.021,
        61..=120 => 2.000,
        _ => 1.980,
    }
}

/// Mean of `xs` with its 95% interval; None with fewer than two values.
pub fn mean_interval(xs: &[f64]) -> Option<Interval> {
    let n = xs.len();
    if n < 2 {
        return None;
    }
    let mean = xs.iter().sum::<f64>() / n as f64;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    let half = t95(n - 1) * (var / n as f64).sqrt();
    Some(Interval {
        mean,
        low: mean - half,
        high: mean + half,
    })
}

/// What the run supports saying about the switch.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Too few tasks ran in both arms.
    TooFewTasks { have: usize },
    /// The cost interval is wider than the effect the run was set to see.
    TooNoisy { half_width: f64, mde: f64 },
    /// On solves fewer tasks; cost doesn't matter then.
    SolvesFewer { by: Interval },
    /// On costs less per try (share of off's cost), solving no fewer.
    Cheaper { share: Interval },
    /// On costs more per try (share of off's cost).
    Costlier { share: Interval },
    /// No cost change as large as the minimum detectable effect.
    NoChange { share: Interval, mde: f64 },
}

/// The paired summary of an `--ab` run.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub pairs: Vec<Pair>,
    pub on: ArmTally,
    pub off: ArmTally,
    /// On minus off, per task: solve rate (0 to 1), cost per try (ticks), model calls per try.
    pub solve: Option<Interval>,
    pub cost: Option<Interval>,
    pub calls: Option<Interval>,
    pub verdict: Verdict,
}

/// Billed ticks per solved task, None when nothing was solved.
pub fn cost_per_solved(arm: &ArmTally) -> Option<f64> {
    (arm.solved > 0).then(|| arm.cost_ticks as f64 / f64::from(arm.solved))
}

/// Compare the arms task by task. `mde` is the smallest cost change, as a
/// share of off's cost per try, the run is meant to detect.
pub fn summarize(pairs: Vec<Pair>, mde: f64) -> Summary {
    let mut on = ArmTally::default();
    let mut off = ArmTally::default();
    for p in &pairs {
        for (total, arm) in [(&mut on, &p.on), (&mut off, &p.off)] {
            total.tries += arm.tries;
            total.solved += arm.solved;
            total.cost_ticks += arm.cost_ticks;
            total.calls += arm.calls;
        }
    }
    let deltas = |f: fn(&ArmTally) -> f64| -> Vec<f64> {
        pairs.iter().map(|p| f(&p.on) - f(&p.off)).collect()
    };
    let solve = mean_interval(&deltas(ArmTally::solve_rate));
    let cost = mean_interval(&deltas(ArmTally::cost_per_try));
    let calls = mean_interval(&deltas(ArmTally::calls_per_try));
    let base = pairs.iter().map(|p| p.off.cost_per_try()).sum::<f64>() / pairs.len().max(1) as f64;
    let verdict = verdict(pairs.len(), solve, cost, base, mde);
    Summary {
        pairs,
        on,
        off,
        solve,
        cost,
        calls,
        verdict,
    }
}

fn verdict(
    tasks: usize,
    solve: Option<Interval>,
    cost: Option<Interval>,
    base: f64,
    mde: f64,
) -> Verdict {
    let (Some(solve), Some(cost)) = (solve, cost) else {
        return Verdict::TooFewTasks { have: tasks };
    };
    if tasks < MIN_PAIRED_TASKS {
        return Verdict::TooFewTasks { have: tasks };
    }
    if solve.high < 0.0 {
        return Verdict::SolvesFewer { by: solve };
    }
    if base <= 0.0 {
        // Nothing was billed in the off arm: no share to judge cost by.
        return Verdict::TooNoisy {
            half_width: f64::INFINITY,
            mde,
        };
    }
    let share = Interval {
        mean: cost.mean / base,
        low: cost.low / base,
        high: cost.high / base,
    };
    if share.half_width() > mde {
        return Verdict::TooNoisy {
            half_width: share.half_width(),
            mde,
        };
    }
    if share.high < 0.0 {
        Verdict::Cheaper { share }
    } else if share.low > 0.0 {
        Verdict::Costlier { share }
    } else {
        Verdict::NoChange { share, mde }
    }
}

fn pct(x: f64) -> String {
    format!("{:+.1}%", x * 100.0)
}

/// One plain line saying what the run supports.
pub fn verdict_line(v: &Verdict) -> String {
    match v {
        Verdict::TooFewTasks { have } => format!(
            "No verdict: {have} task(s) ran in both arms, and a verdict needs at least {MIN_PAIRED_TASKS}."
        ),
        Verdict::TooNoisy { half_width, mde } if half_width.is_finite() => format!(
            "No verdict: the cost interval is ±{:.1}%, wider than the {:.0}% change this run was set to see. Run more tasks or tries.",
            half_width * 100.0,
            mde * 100.0
        ),
        Verdict::TooNoisy { .. } => {
            "No verdict: the off arm billed nothing, so there is no cost to compare.".into()
        }
        Verdict::SolvesFewer { by } => format!(
            "On solves fewer tasks ({} solve rate, 95% CI {} to {}): keep it off whatever it saves.",
            pct(by.mean),
            pct(by.low),
            pct(by.high)
        ),
        Verdict::Cheaper { share } => format!(
            "On is cheaper: {} cost per try (95% CI {} to {}), solving no fewer tasks.",
            pct(share.mean),
            pct(share.low),
            pct(share.high)
        ),
        Verdict::Costlier { share } => format!(
            "On costs more: {} cost per try (95% CI {} to {}).",
            pct(share.mean),
            pct(share.low),
            pct(share.high)
        ),
        Verdict::NoChange { share, mde } => format!(
            "No cost change of {:.0}% or more ({} cost per try, 95% CI {} to {}).",
            mde * 100.0,
            pct(share.mean),
            pct(share.low),
            pct(share.high)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(task: &str, on: (u32, u32, i64, u32), off: (u32, u32, i64, u32)) -> Pair {
        let tally = |(tries, solved, cost_ticks, calls)| ArmTally {
            tries,
            solved,
            cost_ticks,
            calls,
        };
        Pair {
            task: task.into(),
            on: tally(on),
            off: tally(off),
        }
    }

    #[test]
    fn the_interval_uses_students_t_and_never_runs_narrow() {
        assert_eq!(t95(1), 12.706);
        assert_eq!(t95(30), 2.042);
        assert_eq!(t95(35), 2.042, "between rows it takes the row below");
        assert!(t95(1000) > 1.96);
        assert_eq!(mean_interval(&[1.0]), None);
        // Mean 2, sample sd 1, n 3: half width 4.303 / sqrt(3).
        let i = mean_interval(&[1.0, 2.0, 3.0]).unwrap();
        assert!((i.mean - 2.0).abs() < 1e-12);
        assert!((i.half_width() - 4.303 / 3f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn too_few_tasks_give_no_verdict() {
        let s = summarize(
            vec![
                pair("a", (3, 3, 60, 6), (3, 3, 100, 9)),
                pair("b", (3, 3, 60, 6), (3, 3, 100, 9)),
            ],
            DEFAULT_MDE,
        );
        assert_eq!(s.verdict, Verdict::TooFewTasks { have: 2 });
        assert!(verdict_line(&s.verdict).starts_with("No verdict"));
    }

    #[test]
    fn a_noisy_run_gives_no_verdict_even_when_the_mean_looks_good() {
        // On is 30% cheaper on average, but the tasks disagree wildly.
        let s = summarize(
            vec![
                pair("a", (1, 1, 10, 1), (1, 1, 100, 1)),
                pair("b", (1, 1, 190, 1), (1, 1, 100, 1)),
                pair("c", (1, 1, 10, 1), (1, 1, 100, 1)),
            ],
            DEFAULT_MDE,
        );
        assert!(
            matches!(s.verdict, Verdict::TooNoisy { .. }),
            "{:?}",
            s.verdict
        );
    }

    #[test]
    fn a_steady_saving_reads_as_cheaper_and_reports_cost_per_solved_task() {
        let s = summarize(
            vec![
                pair("a", (3, 3, 240, 9), (3, 3, 300, 12)),
                pair("b", (3, 3, 243, 9), (3, 3, 300, 12)),
                pair("c", (3, 3, 237, 9), (3, 3, 300, 12)),
                pair("d", (3, 3, 240, 9), (3, 3, 300, 12)),
            ],
            DEFAULT_MDE,
        );
        let Verdict::Cheaper { share } = s.verdict else {
            panic!("{:?}", s.verdict);
        };
        assert!((share.mean + 0.2).abs() < 1e-9, "{share:?}");
        assert_eq!(cost_per_solved(&s.on), Some(80.0));
        assert_eq!(cost_per_solved(&s.off), Some(100.0));
        assert!((s.calls.unwrap().mean + 1.0).abs() < 1e-9);
    }

    #[test]
    fn solving_fewer_wins_over_any_saving() {
        let s = summarize(
            vec![
                pair("a", (3, 1, 30, 3), (3, 3, 300, 9)),
                pair("b", (3, 1, 30, 3), (3, 3, 300, 9)),
                pair("c", (3, 0, 30, 3), (3, 3, 300, 9)),
            ],
            DEFAULT_MDE,
        );
        assert!(
            matches!(s.verdict, Verdict::SolvesFewer { .. }),
            "{:?}",
            s.verdict
        );
        assert_eq!(cost_per_solved(&ArmTally::default()), None);
    }

    #[test]
    fn a_tight_interval_around_zero_reads_as_no_change() {
        let s = summarize(
            vec![
                pair("a", (2, 2, 201, 4), (2, 2, 200, 4)),
                pair("b", (2, 2, 199, 4), (2, 2, 200, 4)),
                pair("c", (2, 2, 200, 4), (2, 2, 200, 4)),
            ],
            DEFAULT_MDE,
        );
        assert!(
            matches!(s.verdict, Verdict::NoChange { .. }),
            "{:?}",
            s.verdict
        );
    }
}
