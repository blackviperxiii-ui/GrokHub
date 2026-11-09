//! Auto-tune: a small online bandit per device × model × mode that learns
//! which slot count, drafter and micro-batch run fastest on this machine.
//!
//! The planner's own launch is the first promoted arm. The planner proposes
//! the other arms that pass a shadow check (they need no more memory than
//! the plan has: no more slots than it sized the KV for, and a drafter only
//! where one fits). One of them at a time runs as a canary. It takes effect
//! at the next launch the app makes anyway (a cold start or a relaunch
//! already due), never through a restart of its own. A canary that beats
//! the promoted arm by [`PROMOTE_MARGIN`] over [`CANARY_SAMPLES`] chat
//! replies is promoted; otherwise it rolls back. An arm that leaves less
//! than [`HEADROOM_TARGET_MB`] of VRAM free after loading is banned and
//! rolled back at once. The reward is decode speed less the time to read a
//! 1k-token prompt (both from the server's own timings). Under Throttle
//! nothing is tuned: the sharing flags decide that launch.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// One way to launch: slots, the drafter on or off, the micro-batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Arm {
    pub slots: u32,
    pub draft: bool,
    pub ubatch: u32,
}

impl Arm {
    pub fn label(&self) -> String {
        format!(
            "np{}-{}-ub{}",
            self.slots,
            if self.draft { "draft" } else { "plain" },
            self.ubatch
        )
    }
}

/// The engine's own micro-batch, and the smaller one tried against it.
pub const UBATCH_DEFAULT: u32 = 512;
pub const UBATCH_SMALL: u32 = 256;
/// Chat replies a canary runs before it is judged.
pub const CANARY_SAMPLES: u32 = 6;
/// How much better a canary must do to be promoted.
pub const PROMOTE_MARGIN: f64 = 1.05;
/// Free VRAM an arm must leave once loaded.
pub const HEADROOM_TARGET_MB: u64 = 512;
/// Replies shorter than this say little about decode speed.
pub const MIN_REPLY_TOKENS: u64 = 32;
pub const TUNE_FILE: &str = "local_tune.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub n: u32,
    pub mean: f64,
}

/// One device × model × mode.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    #[serde(default)]
    pub arms: BTreeMap<String, Stats>,
    #[serde(default)]
    pub promoted: Option<String>,
    #[serde(default)]
    pub canary: Option<String>,
    #[serde(default)]
    pub banned: BTreeSet<String>,
}

/// Every arm around the planner's `base` that passes the shadow check:
/// at most the slots it sized memory for, a drafter only when it planned
/// one, and the smaller micro-batch only when the engine takes the flag.
pub fn candidates(base: Arm, has_ubatch: bool) -> Vec<Arm> {
    let mut out = Vec::new();
    for slots in [1, 2, 4].into_iter().filter(|s| *s <= base.slots.max(1)) {
        for draft in [false, true].into_iter().filter(|d| !d || base.draft) {
            for ubatch in [UBATCH_DEFAULT, UBATCH_SMALL] {
                if ubatch != base.ubatch && !has_ubatch {
                    continue;
                }
                out.push(Arm {
                    slots,
                    draft,
                    ubatch,
                });
            }
        }
    }
    if !out.contains(&base) {
        out.push(base);
    }
    out
}

/// A reply's reward: tokens a second, less the seconds a 1k-token prompt takes.
pub fn reward(tok_s: f64, prompt_tok_s: f64) -> f64 {
    let read_1k = if prompt_tok_s > 0.0 {
        1_000.0 / prompt_tok_s
    } else {
        0.0
    };
    tok_s - 2.0 * read_1k
}

impl Record {
    /// The arm the next launch runs: the canary while one runs, else the
    /// promoted arm, else the planner's own.
    pub fn current(&self, cands: &[Arm], base: Arm) -> Arm {
        let find = |l: &Option<String>| {
            l.as_ref()
                .filter(|l| !self.banned.contains(*l))
                .and_then(|l| cands.iter().find(|a| &a.label() == l).copied())
        };
        find(&self.canary)
            .or_else(|| find(&self.promoted))
            .unwrap_or(base)
    }

    fn stats(&self, arm: &str) -> Stats {
        self.arms.get(arm).copied().unwrap_or_default()
    }

    /// The next arm to try: an untried one first, then the best upper bound
    /// that could beat the promoted arm.
    fn next_canary(&self, cands: &[Arm], promoted: &str) -> Option<String> {
        let open: Vec<String> = cands
            .iter()
            .map(Arm::label)
            .filter(|l| l != promoted && !self.banned.contains(l))
            .collect();
        if let Some(l) = open.iter().find(|l| self.stats(l).n == 0) {
            return Some(l.clone());
        }
        let total: f64 = self
            .arms
            .values()
            .map(|s| f64::from(s.n))
            .sum::<f64>()
            .max(1.0);
        let bar = self.stats(promoted).mean * PROMOTE_MARGIN;
        open.into_iter()
            .map(|l| {
                let s = self.stats(&l);
                let ucb = s.mean + 2.0 * (total.ln() / f64::from(s.n.max(1))).sqrt();
                (l, ucb)
            })
            .filter(|(_, ucb)| *ucb > bar)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(l, _)| l)
    }

    /// A launch of `arm` came up with `headroom_mb` of VRAM free (`None`:
    /// unknown, or on the CPU). Too little bans it and rolls back.
    pub fn started(&mut self, arm: &str, base: &str, headroom_mb: Option<u64>, cands: &[Arm]) {
        if self.promoted.is_none() {
            self.promoted = Some(base.to_string());
        }
        if headroom_mb.is_some_and(|h| h < HEADROOM_TARGET_MB) {
            self.banned.insert(arm.to_string());
            if self.canary.as_deref() == Some(arm) {
                self.canary = None;
            }
            if self.promoted.as_deref() == Some(arm) {
                self.promoted = Some(base.to_string());
            }
        }
        if self.canary.is_none() {
            let promoted = self.promoted.clone().unwrap_or_default();
            self.canary = self.next_canary(cands, &promoted);
        }
    }

    /// One chat reply on `arm`. A canary with enough replies is judged:
    /// promoted when it beats the promoted arm, else rolled back.
    pub fn observe(&mut self, arm: &str, r: f64, cands: &[Arm]) {
        let s = self.arms.entry(arm.to_string()).or_default();
        s.n += 1;
        s.mean += (r - s.mean) / f64::from(s.n);
        let n = s.n;
        if self.canary.as_deref() != Some(arm) || n < CANARY_SAMPLES {
            return;
        }
        let promoted = self.promoted.clone().unwrap_or_default();
        let held = self.stats(&promoted);
        if held.n == 0 || self.stats(arm).mean > held.mean * PROMOTE_MARGIN {
            self.promoted = Some(arm.to_string());
        }
        let promoted = self.promoted.clone().unwrap_or_default();
        self.canary = self.next_canary(cands, &promoted);
    }
}

pub fn load(dir: &Path) -> BTreeMap<String, Record> {
    std::fs::read_to_string(dir.join(TUNE_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(dir: &Path, all: &BTreeMap<String, Record>) {
    let Ok(json) = serde_json::to_string_pretty(all) else {
        return;
    };
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(TUNE_FILE), json);
}

/// Which arm a planned launch runs, carried on it to [`started`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub key: String,
    pub arm: Arm,
    pub base: Arm,
    pub cands: Vec<Arm>,
}

/// The launch the server runs now and where its record lives.
struct Running {
    dir: PathBuf,
    tag: Tag,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

/// The arm the next launch for `key` runs.
pub fn current(dir: &Path, key: &str, cands: &[Arm], base: Arm) -> Arm {
    load(dir).get(key).map_or(base, |r| r.current(cands, base))
}

/// A launch tagged `tag` is up with `headroom_mb` VRAM free; its chat
/// replies count for its arm from now on.
pub fn started(dir: &Path, tag: &Tag, headroom_mb: Option<u64>) {
    let mut all = load(dir);
    all.entry(tag.key.clone()).or_default().started(
        &tag.arm.label(),
        &tag.base.label(),
        headroom_mb,
        &tag.cands,
    );
    save(dir, &all);
    *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) = Some(Running {
        dir: dir.to_path_buf(),
        tag: tag.clone(),
    });
}

/// One chat reply's timings from the running server.
pub fn observe(tok_s: f64, prompt_tok_s: f64, reply_tokens: u64) {
    if reply_tokens < MIN_REPLY_TOKENS || !tok_s.is_finite() || tok_s <= 0.0 {
        return;
    }
    let run = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(run) = run.as_ref() else {
        return;
    };
    let t = &run.tag;
    let mut all = load(&run.dir);
    let rec = all.entry(t.key.clone()).or_default();
    if rec.promoted.is_none() {
        rec.promoted = Some(t.base.label());
    }
    rec.observe(&t.arm.label(), reward(tok_s, prompt_tok_s), &t.cands);
    save(&run.dir, &all);
}

#[cfg(test)]
#[path = "local_tune_tests.rs"]
mod tests;
