//! Offline proof for the chip engine, with no model call and nothing for a
//! person to run.
//!
//! **Replay.** A scripted month of a person with steady habits: four times of
//! day, two steps each, the second following the first, and one step in ten
//! swapped for a random one. At each step the engine ranks the pool, the
//! person does their next step (from the row when it is there, another way
//! when not), and the engine learns as it would in the app. Recall@3 and
//! MRR@3 are read over the last three weeks and compared with a frecency
//! ranking over the same rows, so both show chips equally often.
//!
//! **Speed.** Ranking 200 candidates against a full log stays well inside a
//! frame, and the whole chip memory stays small.

use crate::chip_rank::{rank_quick_chips, rank_quick_chips_from, RankCtx, CHIP_ROW_MAX};
use crate::chip_store::{
    log_chip_outcome, log_chips_shown, outcome_signals, shown_signals, CHIP_LOG_MAX,
};
use crate::chips::{remember_chip_click, ChipKind, ChipMemory, QuickChip};
use crate::learn::{Beliefs, Signal};

const HOUR_MS: u64 = 60 * 60 * 1000;
const DAYS: u64 = 30;
const WARMUP_DAYS: u64 = 9;
/// Frecency's half-life, as in the engine.
const HALF_LIFE_MS: f64 = 14.0 * 24.0 * HOUR_MS as f64;

fn act(i: usize) -> QuickChip {
    QuickChip {
        id: format!("act-{i}"),
        label: format!("Step {i}"),
        value: format!("Do step {i}."),
        kind: ChipKind::Chat,
        score: 10.0,
        hint: String::new(),
        primary: false,
    }
}

/// Deterministic noise for the script.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

/// The month: (time, hour, step).
fn script() -> Vec<(u64, u8, usize)> {
    const ROUTINE: [(u8, [usize; 2]); 4] = [(8, [0, 1]), (12, [2, 3]), (18, [4, 5]), (22, [6, 7])];
    let mut rng = Rng(7);
    let mut out = Vec::new();
    for day in 0..DAYS {
        for (hour, steps) in ROUTINE {
            for (k, &step) in steps.iter().enumerate() {
                let step = if rng.next().is_multiple_of(10) {
                    (rng.next() % 8) as usize
                } else {
                    step
                };
                let at = day * 24 * HOUR_MS + u64::from(hour) * HOUR_MS + k as u64 * 60_000;
                out.push((at, hour, step));
            }
        }
    }
    out
}

/// Frecency alone: past uses, halved every two weeks.
fn frecency_row(history: &[(u64, usize)], now: u64, pool: usize) -> Vec<usize> {
    let mut score = vec![0.0f64; pool];
    for &(at, step) in history {
        score[step] += 0.5f64.powf(now.saturating_sub(at) as f64 / HALF_LIFE_MS);
    }
    let mut order: Vec<usize> = (0..pool).collect();
    order.sort_by(|&a, &b| score[b].total_cmp(&score[a]).then(a.cmp(&b)));
    order.truncate(CHIP_ROW_MAX);
    order
}

#[derive(Default)]
struct Score {
    steps: u32,
    hits: u32,
    rr: f64,
}

impl Score {
    fn add(&mut self, row: &[usize], target: usize) {
        self.steps += 1;
        if let Some(rank) = row.iter().position(|&s| s == target) {
            self.hits += 1;
            self.rr += 1.0 / (rank + 1) as f64;
        }
    }

    fn recall(&self) -> f64 {
        f64::from(self.hits) / f64::from(self.steps.max(1))
    }

    fn mrr(&self) -> f64 {
        self.rr / f64::from(self.steps.max(1))
    }
}

#[test]
fn the_engine_beats_frecency_on_a_month_of_steady_habits() {
    let pool: Vec<QuickChip> = (0..8).map(act).collect();
    let id_of = |c: &QuickChip| c.id.trim_start_matches("act-").parse::<usize>().unwrap();
    let mut mem = ChipMemory::default();
    // The learning engine's beliefs, told what the running app tells them.
    let mut beliefs = Beliefs::default();
    let hear = |beliefs: &mut Beliefs, told: Vec<Signal>| {
        for s in &told {
            beliefs.apply(s);
        }
    };
    let mut history: Vec<(u64, usize)> = Vec::new();
    let (mut engine, mut base) = (Score::default(), Score::default());
    let mut held_back = 0;
    for (i, (at, hour, target)) in script().into_iter().enumerate() {
        let ctx = RankCtx {
            surface: "composer",
            hour,
            now_ms: at,
            seed: i as u64,
            max: CHIP_ROW_MAX,
        };
        let row = rank_quick_chips_from(&beliefs, &mut mem, pool.clone(), ctx);
        let scored = at >= WARMUP_DAYS * 24 * HOUR_MS;
        if row.is_empty() {
            // Held back (or gated): neither ranking is counted for this step.
            held_back += 1;
        } else {
            let told = shown_signals(&mut mem, "composer", &row, at);
            hear(&mut beliefs, told);
            let shown: Vec<usize> = row.iter().map(id_of).collect();
            if scored {
                engine.add(&shown, target);
                base.add(&frecency_row(&history, at, pool.len()), target);
            }
            if shown.contains(&target) {
                let told = outcome_signals(&mut mem, "composer", &pool[target], true, at + 1_000);
                hear(&mut beliefs, told);
            }
        }
        // The step is done either way: from the row, or another way.
        remember_chip_click(&mut mem, &pool[target], None, at + 1_000, hour);
        history.push((at, target));
    }
    assert!(engine.steps >= 120, "too few scored rows: {}", engine.steps);
    assert!(held_back > 0, "the holdout should take a few boundaries");
    let (r, b) = (engine.recall(), base.recall());
    let (m, bm) = (engine.mrr(), base.mrr());
    assert!(
        r >= b + 0.10,
        "Recall@3 engine {r:.2} vs frecency {b:.2} (MRR {m:.2} vs {bm:.2})"
    );
    assert!(m > bm, "MRR@3 engine {m:.2} vs frecency {bm:.2}");
    // The steps the habit predicts should mostly be on the row.
    assert!(r >= 0.6, "Recall@3 {r:.2}");
}

#[test]
fn ranking_two_hundred_chips_fits_in_a_frame_and_memory_stays_small() {
    let mut mem = ChipMemory::default();
    let pool: Vec<QuickChip> = (0..200)
        .map(|i| QuickChip {
            score: (i % 17) as f32,
            ..act(i)
        })
        .collect();
    // A full history: every chip used, the log at its cap.
    for (i, c) in pool.iter().enumerate() {
        remember_chip_click(&mut mem, c, None, i as u64 * 1_000, (i % 24) as u8);
    }
    for i in 0..CHIP_LOG_MAX as u64 {
        let row: Vec<QuickChip> = (0..3)
            .map(|k| pool[((i * 3 + k) % 200) as usize].clone())
            .collect();
        log_chips_shown(&mut mem, "composer", &row, 300_000 + i);
        if i % 4 == 0 {
            log_chip_outcome(&mut mem, "composer", &row[0], true, 300_000 + i);
        }
    }
    assert_eq!(mem.events.len(), CHIP_LOG_MAX);
    let mut times = Vec::new();
    for s in 0..200u64 {
        let ctx = RankCtx {
            surface: "home",
            hour: 9,
            now_ms: 10_000_000 + s,
            seed: s,
            max: CHIP_ROW_MAX,
        };
        let start = std::time::Instant::now();
        let row = rank_quick_chips(&mut mem, pool.clone(), ctx);
        times.push(start.elapsed());
        assert!(row.len() <= CHIP_ROW_MAX);
    }
    times.sort();
    let p99 = times[times.len() * 99 / 100];
    // An optimized build must stay under a millisecond; a test build is far
    // slower, so it gets room without hiding a real slowdown. A shared
    // Windows runner has gone past 40 ms in a test build, so it gets 100.
    let budget = if cfg!(debug_assertions) {
        std::time::Duration::from_millis(100)
    } else {
        std::time::Duration::from_millis(1)
    };
    assert!(p99 < budget, "p99 {p99:?} over {budget:?}");
    let bytes = serde_json::to_vec(&mem).unwrap().len();
    assert!(bytes < 5 * 1024 * 1024, "chip memory {bytes} bytes");
}
