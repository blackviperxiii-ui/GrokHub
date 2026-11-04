//! B5–B7: nearest-neighbour memory for the ask reader ([`super::ask`]).
//!
//! The rules read every typed turn. When the small on-device embedding model
//! ([`crate::memory_embed`]) is on disk, the turn's words are also embedded
//! (microseconds, no tokens) and compared with a bank of labeled turns: the
//! hand-labeled sets in [`super::ask_set`] plus turns whose outcome this
//! computer saw. The `k` closest neighbours vote on the lane and the effort:
//!
//! - too few close neighbours (`knn:thin`): the rules stand;
//! - neighbours agreeing on one rung at [`AGREE`] or more win (`knn:agree`,
//!   `knn:raise`, `knn:lower`), and on the lane too when the rules weren't sure
//!   (`knn:lane`);
//! - otherwise the higher of the two efforts wins (`knn:raise` or
//!   `knn:mixed`): under-thinking costs more than over-thinking;
//! - a stakes turn (A3) is never lowered or moved off its lane: neighbours
//!   may only raise it (`knn:hold` when they'd have lowered it).
//!
//! No model on disk, `memory_embeddings` off, or an 18+ or banned turn: the
//! bank is never read and the read is the rules' alone. The bank on disk
//! (`models/ask_bank.json`) holds embeddings, kinds, lanes, rungs, outcomes
//! and times only, never a word of any turn, capped at [`BANK_CAP`].

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use grokhub_core::adult_guard::{self, Risk};
use grokhub_core::model_registry::store::{models_dir, read_json, write_json};
use grokhub_core::model_registry::EFFORT_LADDER;
use grokhub_core::static_embed::{cosine, StaticModel};
use serde::{Deserialize, Serialize};

use super::ask::{self, AskContext, AskKind, AskRead, Stakes, LOW, STAKES_LANE, XHIGH};
use super::ask_set::{HELD_OUT, SET, STAKES_HELD_OUT, STAKES_SET};

/// The bank of turns this computer learned, under `{config}/models/`.
pub const BANK_FILE: &str = "ask_bank.json";
pub const BANK_SCHEMA: u32 = 1;
/// Learned turns kept; the oldest goes first.
pub const BANK_CAP: usize = 2_000;
/// Neighbours that vote.
pub const K: usize = 50;
/// Cosine below this is no neighbour at all.
pub const SIM_FLOOR: f32 = 0.6;
/// Close neighbours needed before the bank says anything.
pub const MIN_CLOSE: usize = 5;
/// Weighted share of the vote that counts as strong agreement.
pub const AGREE: f64 = 0.7;
/// A rule read at least this sure keeps its lane whatever the neighbours say.
pub const LANE_TRUST: f64 = 0.35;
/// Chats with a turn waiting on its outcome.
const PENDING_CAP: usize = 256;

/// One labeled turn: where it went and how it ended. Never its words.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Unit-length embedding.
    pub v: Vec<f32>,
    pub kind: AskKind,
    pub lane: &'static str,
    /// Rung on [`EFFORT_LADDER`].
    pub effort: usize,
    /// It worked at `effort` (false: `effort` wasn't enough).
    pub ok: bool,
    /// Unix ms; 0 for the shipped labeled sets.
    pub at: u64,
}

impl Entry {
    /// The rung this entry says such a turn needs: the one that worked, or one
    /// above the one that didn't. Never past xhigh, as a read never starts there.
    pub fn vote(&self) -> usize {
        if self.ok {
            self.effort.min(XHIGH)
        } else {
            (self.effort + 1).min(XHIGH)
        }
    }
}

/// What the neighbours did to a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Thin,
    Agree,
    Raise,
    Lower,
    Hold,
    Mixed,
}

impl Verdict {
    pub fn id(self) -> &'static str {
        match self {
            Verdict::Thin => "knn:thin",
            Verdict::Agree => "knn:agree",
            Verdict::Raise => "knn:raise",
            Verdict::Lower => "knn:lower",
            Verdict::Hold => "knn:hold",
            Verdict::Mixed => "knn:mixed",
        }
    }
}

/// The rule id logged when the neighbours moved the lane.
pub const LANE_RULE: &str = "knn:lane";

fn rung(e: &str) -> usize {
    EFFORT_LADDER.iter().position(|r| *r == e).unwrap_or(0)
}

/// The `k` closest entries at or above [`SIM_FLOOR`], closest first.
/// Stakes-lane entries vote only on a stakes turn: their effort came from
/// words this turn doesn't have.
pub fn neighbours<'a>(
    v: &[f32],
    bank: impl IntoIterator<Item = &'a Entry>,
    stakes: bool,
) -> Vec<(f32, &'a Entry)> {
    let mut near: Vec<(f32, &Entry)> = bank
        .into_iter()
        .filter(|e| stakes || e.lane != STAKES_LANE)
        .map(|e| (cosine(v, &e.v), e))
        .filter(|(s, _)| *s >= SIM_FLOOR)
        .collect();
    near.sort_by(|a, b| b.0.total_cmp(&a.0));
    near.truncate(K);
    near
}

/// The heaviest key in `w` and its share of `total`; ties go to the larger key.
fn top<T: Ord + Copy>(w: &BTreeMap<T, f64>, total: f64) -> Option<(T, f64)> {
    w.iter()
        .max_by(|a, b| a.1.total_cmp(b.1).then(a.0.cmp(b.0)))
        .map(|(k, x)| (*k, if total > 0.0 { x / total } else { 0.0 }))
}

/// The rule read blended with its neighbours in `bank` (`v` is the turn's
/// embedding). Pure.
pub fn blend<'a>(
    mut read: AskRead,
    v: &[f32],
    bank: impl IntoIterator<Item = &'a Entry>,
    cx: &AskContext,
) -> AskRead {
    let stakes = read.stakes != Stakes::None;
    let near = neighbours(v, bank, stakes);
    if near.len() < MIN_CLOSE {
        read.rules.push(Verdict::Thin.id().into());
        return read;
    }
    let total: f64 = near.iter().map(|(s, _)| *s as f64).sum();
    let mut lane_moved = false;
    if !stakes && read.confidence < LANE_TRUST {
        let mut lanes: BTreeMap<&str, f64> = BTreeMap::new();
        for (s, e) in &near {
            *lanes.entry(e.lane).or_default() += *s as f64;
        }
        if let Some((lane, share)) = top(&lanes, total)
            && share >= AGREE
            && lane != read.lane()
        {
            let mut kinds: BTreeMap<AskKind, f64> = BTreeMap::new();
            for (s, e) in near.iter().filter(|(_, e)| e.lane == lane) {
                *kinds.entry(e.kind).or_default() += *s as f64;
            }
            if let Some((kind, _)) = top(&kinds, total) {
                read.kind = kind;
                lane_moved = true;
            }
        }
    }
    let mut votes: BTreeMap<usize, f64> = BTreeMap::new();
    for (s, e) in &near {
        *votes.entry(e.vote()).or_default() += *s as f64;
    }
    let r = read.effort;
    // A stakes turn keeps its rung as a floor; a picture is never no-thinking.
    let floor = if stakes {
        r
    } else if cx.needs_image {
        LOW
    } else {
        0
    };
    let strong = top(&votes, total).filter(|(_, share)| *share >= AGREE);
    let (verdict, e) = match strong {
        Some((n, _)) if n > r => (Verdict::Raise, n),
        Some((n, _)) if n < r && stakes => (Verdict::Hold, r),
        Some((n, _)) if n.max(floor) < r => (Verdict::Lower, n.max(floor)),
        Some(_) => (Verdict::Agree, r),
        None => {
            // The weighted median: the lowest rung with half the vote at or below it.
            let mut seen = 0.0;
            let median = votes
                .iter()
                .find(|(_, w)| {
                    seen += **w;
                    seen >= total / 2.0
                })
                .map_or(r, |(n, _)| *n);
            if median > r {
                (Verdict::Raise, median)
            } else {
                (Verdict::Mixed, r)
            }
        }
    };
    read.effort = e;
    if let Some(first) = read.rules.first_mut() {
        *first = format!("ask:{}", read.kind.id());
    }
    if let Some(at) = read.rules.iter().position(|x| x.starts_with("ask:effort:")) {
        read.rules[at] = format!("ask:effort:{}", read.effort_name());
    }
    read.rules.push(verdict.id().into());
    if lane_moved {
        read.rules.push(LANE_RULE.into());
    }
    read
}

// ---------------------------------------------------------------- the seed

/// One shipped labeled row as an entry: the rung the rules pick, kept inside
/// the row's labeled range.
pub fn seed_entry(
    model: &StaticModel,
    row: &(&str, AskKind, &str, &str),
    stakes_row: bool,
) -> Option<Entry> {
    let (text, kind, lo, hi) = *row;
    let v = model.encode(ask::own_words(text))?;
    let effort = ask::read(text, &AskContext::default())
        .effort
        .clamp(rung(lo), rung(hi));
    Some(Entry {
        v,
        kind,
        lane: if stakes_row { STAKES_LANE } else { kind.lane() },
        effort,
        ok: true,
        at: 0,
    })
}

/// Every shipped labeled row (both sets, stakes included), embedded by `model`.
pub fn seed(model: &StaticModel) -> Vec<Entry> {
    let plain = SET.iter().chain(HELD_OUT).map(|r| (r, false));
    let stakes = STAKES_SET.iter().chain(STAKES_HELD_OUT).map(|r| (r, true));
    plain
        .chain(stakes)
        .filter_map(|(r, s)| seed_entry(model, r, s))
        .collect()
}

/// The seed for the model in use, embedded once per model.
static SEED: Mutex<Option<(usize, Arc<Vec<Entry>>)>> = Mutex::new(None);

fn seed_for(model: &Arc<StaticModel>) -> Arc<Vec<Entry>> {
    let id = Arc::as_ptr(model) as usize ^ model.dim();
    let mut slot = SEED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, seed)) = slot.as_ref()
        && *at == id
    {
        return seed.clone();
    }
    let s = Arc::new(seed(model));
    *slot = Some((id, s.clone()));
    s
}

// ---------------------------------------------------------------- the bank on disk

#[derive(Debug, Default, Serialize, Deserialize)]
struct BankFile {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    dim: usize,
    #[serde(default)]
    entries: Vec<DiskEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskEntry {
    /// The embedding as signed bytes (×127), base64.
    v: String,
    kind: String,
    lane: String,
    effort: String,
    ok: bool,
    at: u64,
}

pub fn bank_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(BANK_FILE)
}

fn quantize(v: &[f32]) -> String {
    let bytes: Vec<u8> = v
        .iter()
        .map(|x| (x * 127.0).round().clamp(-127.0, 127.0) as i8 as u8)
        .collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn dequantize(s: &str, dim: usize) -> Option<Vec<f32>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(s).ok()?;
    if bytes.len() != dim {
        return None;
    }
    let mut v: Vec<f32> = bytes.iter().map(|b| *b as i8 as f32 / 127.0).collect();
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return None;
    }
    v.iter_mut().for_each(|x| *x /= norm);
    Some(v)
}

fn lane_of(s: &str) -> Option<&'static str> {
    AskKind::ALL
        .iter()
        .map(|k| k.lane())
        .chain([STAKES_LANE])
        .find(|l| *l == s)
}

impl DiskEntry {
    fn of(e: &Entry) -> Self {
        Self {
            v: quantize(&e.v),
            kind: e.kind.id().into(),
            lane: e.lane.into(),
            effort: EFFORT_LADDER[e.effort.min(EFFORT_LADDER.len() - 1)].into(),
            ok: e.ok,
            at: e.at,
        }
    }

    fn entry(&self, dim: usize) -> Option<Entry> {
        Some(Entry {
            v: dequantize(&self.v, dim)?,
            kind: AskKind::parse(&self.kind)?,
            lane: lane_of(&self.lane)?,
            effort: EFFORT_LADDER.iter().position(|r| *r == self.effort)?,
            ok: self.ok,
            at: self.at,
        })
    }
}

/// The learned turns, oldest first. A file for another model width loads empty.
pub fn load_bank(config_dir: &Path, dim: usize) -> VecDeque<Entry> {
    let file: BankFile = read_json(&bank_path(config_dir)).unwrap_or_default();
    if file.dim != dim {
        return VecDeque::new();
    }
    let mut out: VecDeque<Entry> = file.entries.iter().filter_map(|e| e.entry(dim)).collect();
    while out.len() > BANK_CAP {
        out.pop_front();
    }
    out
}

/// Write the learned turns (tmp file, then rename).
pub fn save_bank(config_dir: &Path, dim: usize, entries: &VecDeque<Entry>) -> Result<(), String> {
    let file = BankFile {
        schema: BANK_SCHEMA,
        dim,
        entries: entries.iter().map(DiskEntry::of).collect(),
    };
    write_json(&bank_path(config_dir), &file)
}

/// The learned bank in memory, keyed by the config folder and model width.
static GROWN: Mutex<Option<(PathBuf, usize, VecDeque<Entry>)>> = Mutex::new(None);

fn with_grown<T>(config_dir: &Path, dim: usize, f: impl FnOnce(&mut VecDeque<Entry>) -> T) -> T {
    let mut slot = GROWN.lock().unwrap_or_else(|e| e.into_inner());
    let fresh = !matches!(slot.as_ref(), Some((d, w, _)) if d == config_dir && *w == dim);
    if fresh {
        *slot = Some((config_dir.to_path_buf(), dim, load_bank(config_dir, dim)));
    }
    let (_, _, bank) = slot.as_mut().expect("loaded above");
    f(bank)
}

/// Add one learned turn, evicting the oldest past [`BANK_CAP`], and save.
pub fn record(config_dir: &Path, entry: Entry) {
    let dim = entry.v.len();
    with_grown(config_dir, dim, |bank| {
        bank.push_back(entry);
        while bank.len() > BANK_CAP {
            bank.pop_front();
        }
        let _ = save_bank(config_dir, dim, bank);
    });
}

// ---------------------------------------------------------------- live turns

/// A chat's turn waiting on its outcome: its embedding and what it ran at.
#[derive(Debug, Clone)]
struct Pending {
    turn: u64,
    v: Vec<f32>,
    read: AskRead,
    /// The rung of the turn's first routed call, then its latest.
    start: Option<usize>,
    last: usize,
    ok: bool,
    cancelled: bool,
    /// C10: a step of the turn hit trouble (a tool error, a failed check, a
    /// loop, a reject, an answer cut off by its budget).
    trouble: bool,
}

static PENDING: Mutex<BTreeMap<String, Pending>> = Mutex::new(BTreeMap::new());

/// What a finished turn teaches: the start rung when it worked without
/// climbing, the rung that finally worked when it climbed, or the rung that
/// wasn't enough when the next turn said it was wrong ("that's wrong", "try
/// again") or the turn hit trouble without climbing. A turn whose last call
/// errored or was cancelled teaches nothing: an outage says nothing about effort.
fn settled(p: &Pending, redo: bool, now_ms: u64) -> Option<Entry> {
    let start = p.start?;
    if p.cancelled || !p.ok {
        return None;
    }
    let ok = !redo && !(p.trouble && p.last <= start);
    Some(Entry {
        v: p.v.clone(),
        kind: p.read.kind,
        lane: p.read.lane(),
        effort: if ok && p.last <= start { start } else { p.last },
        ok,
        at: now_ms,
    })
}

/// The read for one typed chat turn: the rules' `read` blended with the bank.
/// A new turn in `key` settles the one before it. Later calls in the same
/// turn reuse the first call's blend.
pub fn blend_turn(
    config_dir: &Path,
    key: &str,
    text: &str,
    turn: u64,
    cx: &AskContext,
    read: AskRead,
    now_ms: u64,
) -> AskRead {
    let Some(model) = crate::memory_embed::embedder() else {
        return read;
    };
    let words = ask::own_words(text);
    if !key.is_empty() {
        let prev = {
            let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            match pending.get(key) {
                Some(p) if p.turn == turn => return p.read.clone(),
                Some(_) => pending.remove(key),
                None => None,
            }
        };
        let redo = grokhub_core::outcome::is_correction(text) || ask::asks_redo(text);
        if let Some(entry) = prev.and_then(|p| settled(&p, redo, now_ms)) {
            record(config_dir, entry);
        }
    }
    // 18+ and banned turns never touch the bank.
    if adult_guard::check(words) != Risk::Clean {
        return read;
    }
    let Some(v) = model.encode(words) else {
        return read;
    };
    let seed = seed_for(&model);
    let read = with_grown(config_dir, v.len(), |grown| {
        blend(read, &v, seed.iter().chain(grown.iter()), cx)
    });
    if !key.is_empty() {
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        if pending.len() >= PENDING_CAP {
            pending.clear();
        }
        pending.insert(
            key.to_string(),
            Pending {
                turn,
                v,
                read: read.clone(),
                start: None,
                last: 0,
                ok: false,
                cancelled: false,
                trouble: false,
            },
        );
    }
    read
}

/// The rung the ladder picked for a call of `key`'s turn `turn`.
pub fn note_rung(key: &str, turn: u64, rung: usize) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = pending.get_mut(key).filter(|p| p.turn == turn) {
        p.start.get_or_insert(rung);
        p.last = rung;
    }
}

/// C9: the learner moved the turn's first call to `rung`, so that is the
/// rung the turn started at.
pub fn note_start(key: &str, turn: u64, rung: usize) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = pending.get_mut(key).filter(|p| p.turn == turn) {
        p.start = Some(rung);
        p.last = rung;
    }
}

/// C10: a step of `key`'s waiting turn hit trouble.
pub fn note_trouble(key: &str) {
    if let Some(p) = PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(key)
    {
        p.trouble = true;
    }
}

/// How the latest call of `key`'s turn ended.
pub fn note_done(key: &str, ok: bool, cancelled: bool) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = pending.get_mut(key) {
        p.ok = ok;
        p.cancelled |= cancelled;
    }
}

/// Drop `key`'s waiting turn (a holdout turn runs at a fixed rung and teaches nothing).
pub fn forget(key: &str) {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(key);
}
