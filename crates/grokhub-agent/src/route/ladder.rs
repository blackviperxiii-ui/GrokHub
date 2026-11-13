//! Router R1 effort ladder (§14.3). Each class has a start, a floor and a
//! ceiling on [`EFFORT_LADDER`]. A step starts at the class start (moved one
//! rung by difficulty or by the user's own words), then climbs one rung at a
//! time on trouble (E1–E5) and comes down on clean work (DE1–DE3). The plan
//! calls the de-escalation rules D1–D3; they are `DE1`–`DE3` here so they are
//! not confused with the owner decisions D1–D3.
//!
//! Background classes never move on their own: they run at their start (the
//! same effort they sent before R1) and only DE3 can lower them.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Mutex;

use grokhub_core::model_registry::EFFORT_LADDER;

use super::policy::ClassRow;

/// The class whose floor is High: preparing money, send, delete, credentials or an irreversible OS step.
pub const PREPARE_HARD: &str = "prepare:hard";

pub fn rung(e: &str) -> Option<usize> {
    EFFORT_LADDER
        .iter()
        .position(|l| l.eq_ignore_ascii_case(e.trim()))
}

/// `high` on the ladder: d alone never starts above it, and `prepare:hard` never goes below it.
pub fn high() -> usize {
    rung("high").unwrap_or(4)
}

/// Background work (`background:*`) runs at its class start; everything else is user facing.
pub fn user_facing(class: &str) -> bool {
    !class.trim().starts_with("background:")
}

/// The user's own words about effort, for one episode only. Never saved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Steer {
    #[default]
    None,
    Harder,
    Quicker,
}

const HARDER: &[&str] = &[
    "think hard",
    "think harder",
    "think deeply",
    "think really hard",
    "think it through",
    "take your time",
];
const QUICKER: &[&str] = &[
    "keep it quick",
    "be quick",
    "quick answer",
    "keep it short",
    "just quickly",
];

impl Steer {
    pub fn from_text(text: &str) -> Self {
        let t = text.to_ascii_lowercase();
        if HARDER.iter().any(|p| t.contains(p)) {
            Self::Harder
        } else if QUICKER.iter().any(|p| t.contains(p)) {
            Self::Quicker
        } else {
            Self::None
        }
    }
}

/// Keyword steer, unless care-first is on. Then the Brief's row picks effort.
pub fn steer_for_turn(text: &str, care_first: bool) -> Steer {
    if care_first {
        Steer::None
    } else {
        Steer::from_text(text)
    }
}

/// Care-first holds `chat:default` so a low difficulty score does not drop a rung.
pub fn hold_chat_drops(class: &str, care_first: bool) -> bool {
    care_first && class.trim() == "chat:default"
}

/// A class's rungs, after any accuracy-guard revert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    pub floor: usize,
    pub start: usize,
    pub ceiling: usize,
}

impl Band {
    /// `bump` raises start and floor that many rungs (the guard's revert), never past the ceiling.
    pub fn of(row: &ClassRow, bump: u8) -> Self {
        let ceiling = rung(row.ceiling).unwrap_or(0);
        let up = |r: usize| (r + bump as usize).min(ceiling);
        Self {
            floor: up(rung(row.floor).unwrap_or(0)),
            start: up(rung(row.start).unwrap_or(0)),
            ceiling,
        }
    }

    /// [`Band::of`] with a self-tuned start (R3a) merged over the class table,
    /// clamped inside the floor and ceiling. Floors and ceilings are never
    /// tuned, and `prepare:hard` never starts lower than its row says.
    pub fn tuned(row: &ClassRow, bump: u8, start: Option<&str>) -> Self {
        let mut b = Self::of(row, bump);
        if let Some(r) = start.and_then(rung) {
            let r = r.clamp(b.floor, b.ceiling.max(b.floor));
            b.start = if row.class == PREPARE_HARD {
                r.max(b.start)
            } else {
                r
            };
        }
        b
    }
}

/// Where a step starts: the class start, moved by the user's words or by difficulty.
/// xhigh and max come only from escalation or an explicit "think hard".
pub fn start_rung(band: Band, d: f64, steer: Steer, user: bool) -> (usize, Vec<String>) {
    start_holding(band, d, steer, user, false)
}

/// [`start_rung`]. `hold_drops` skips the low-difficulty drop.
pub fn start_holding(
    band: Band,
    d: f64,
    steer: Steer,
    user: bool,
    hold_drops: bool,
) -> (usize, Vec<String>) {
    if !user {
        return (band.start, Vec::new());
    }
    match steer {
        Steer::Harder => return (band.ceiling.max(band.start), vec!["steer:harder".into()]),
        Steer::Quicker => return (band.floor, vec!["steer:quicker".into()]),
        Steer::None => {}
    }
    if d < 0.3 && band.start > band.floor && !hold_drops {
        (band.start - 1, vec!["d<0.3".into()])
    } else if d > 0.7 && band.start < band.ceiling.min(high()) {
        (band.start + 1, vec!["d>0.7".into()])
    } else {
        (band.start, Vec::new())
    }
}

/// Which way the last change went; clamping rounds the same way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Move {
    #[default]
    Start,
    Up,
    Down,
}

/// What the router saw since the episode's last call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Obs {
    /// A new user message started this turn.
    pub new_turn: bool,
    /// Tools that failed (or a detector finding) since the last call.
    pub tool_errors: u32,
    /// VerifyGate rejects so far in this episode. `None` when no VerifyGate source.
    pub rejects: Option<u32>,
    /// The latest VerifyGate check said VERIFY_OK.
    pub verify_ok: bool,
    /// The newest user text reads as a correction or an undo.
    pub correction: bool,
    /// A low self-check / verify score before claiming done. `None` when no source reports one.
    pub low_check: Option<bool>,
    /// Spend budget used, percent. `None` while no budget exists.
    pub budget_pct: Option<u8>,
    /// DE2: the rung this task's signature is known to succeed at ([`routine_rung`]).
    pub routine: Option<usize>,
    /// A4 and E14: what the agent loop saw since the last call ([`note_tool_run`]).
    pub stage: Stage,
    /// A Flagship or high-stakes turn: no settling below the turn's start (DE1, A4).
    pub keep_start: bool,
}

/// A4 and E14: the agent loop's own signals between two routed calls, typed.
/// Built by [`note_tool_run`] and [`note_out_of_budget`]; a new turn starts empty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stage {
    /// Every tool that ran since the last call was a read or a search and it
    /// worked: the next step only digests tool output.
    pub digest: bool,
    /// The same tool failed twice in a row.
    pub same_tool_failed: bool,
    /// A tool ran again with the same arguments (a loop).
    pub looped: bool,
    /// A test or build run failed after an edit.
    pub check_failed: bool,
    /// [`EXPLORE_LIMIT`] reads or searches in this turn and no edit yet.
    pub explored: bool,
    /// An edit worked or a test or build run passed, and nothing failed.
    pub progress: bool,
    /// E14: the reasoning used up the output budget and left no answer.
    pub out_of_budget: bool,
}

/// A4: read or search calls in one turn, with no edit, that count as a long exploration.
pub const EXPLORE_LIMIT: u32 = 8;
/// A4: steps in a row with a working edit or a passing check before effort settles one rung.
pub const SETTLE_STREAK: u32 = 2;
/// E15: failures at one rung before the episode moves to a stronger model.
pub const NEXT_MODEL_AFTER: u32 = 2;
/// E15: the rule that asks [`super::Router`] for a stronger model.
pub const NEXT_MODEL: &str = "E15:next_model";

/// One episode's effort in one class.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffortState {
    pub rung: usize,
    /// Clean steps in a row.
    pub clean: u32,
    pub rejects: u32,
    /// After a reject, no de-escalation until the next VERIFY_OK.
    pub hold: bool,
    /// The last call came back fine.
    pub last_ok: bool,
    pub moved: Move,
    /// The user message this turn answers (a hash, never the text).
    pub turn: u64,
    /// E15: failures seen at the current rung (a rung change starts it over).
    pub fails: u32,
    /// A4: steps in a row with a working edit or a passing check.
    pub progress: u32,
}

/// The ladder's pick for one call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pick {
    pub rung: usize,
    pub moved: Move,
    pub rules: Vec<String>,
    /// A third reject: VerifyGate's 1a recovery ladder takes over (retry, backtrack, then pause and tell).
    pub recover: bool,
}

/// DE2: successes a task signature needs at one lower rung.
pub const ROUTINE_MIN_SUCCESSES: usize = 3;
/// DE2: how far back those successes may be.
pub const ROUTINE_WINDOW_MS: u64 = 14 * 24 * 60 * 60 * 1000;

/// One task signature's finished runs (Spike-7 outcome records joined with the
/// rung each ran at). Built outside [`advance`]; no I/O here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Routine {
    /// `(rung, finished_at)` of each successful run.
    pub successes: Vec<(usize, u64)>,
    /// The newest failed run: successes before it don't count.
    pub last_failure: Option<u64>,
}

/// DE2 (known routine): the lowest rung below the class start with at least
/// [`ROUTINE_MIN_SUCCESSES`] successes inside [`ROUTINE_WINDOW_MS`] (and since the
/// signature's last failure). Never below the floor; never for `prepare:hard`.
pub fn routine_rung(class: &str, r: &Routine, band: Band, now_ms: u64) -> Option<usize> {
    if class.trim() == PREPARE_HARD {
        return None;
    }
    let since = now_ms
        .saturating_sub(ROUTINE_WINDOW_MS)
        .max(r.last_failure.map(|f| f + 1).unwrap_or(0));
    (band.floor..band.start).find(|rung| {
        r.successes
            .iter()
            .filter(|(x, at)| x == rung && *at >= since && *at <= now_ms)
            .count()
            >= ROUTINE_MIN_SUCCESSES
    })
}

/// Advance one episode's state for the next call. Pure: the caller keeps the state.
pub fn advance(
    prev: Option<&EffortState>,
    class: &str,
    band: Band,
    start: (usize, Vec<String>),
    obs: &Obs,
) -> (EffortState, Pick) {
    advance_holding(prev, class, band, start, obs, false)
}

/// [`advance`]. `hold_drops` skips DE1 and DE2. A budget drop (DE3) still runs.
pub fn advance_holding(
    prev: Option<&EffortState>,
    class: &str,
    band: Band,
    start: (usize, Vec<String>),
    obs: &Obs,
    hold_drops: bool,
) -> (EffortState, Pick) {
    let hard = class.trim() == PREPARE_HARD;
    let user = user_facing(class);
    let fresh = prev.is_none() || obs.new_turn;
    let mut rules: Vec<String> = Vec::new();
    let mut st = match prev {
        Some(p) if !fresh => p.clone(),
        // A new turn starts over at the class start, but a reject's hold carries over.
        Some(p) => EffortState {
            rung: start.0,
            rejects: p.rejects,
            hold: p.hold,
            ..EffortState::default()
        },
        None => EffortState {
            rung: start.0,
            ..EffortState::default()
        },
    };
    st.moved = Move::Start;
    let entry = st.rung;
    if fresh {
        rules.extend(start.1);
    }
    // E15: a failure steps effort up first (same model, so the prompt cache
    // holds). Only a second failure at a rung that can't climb asks for a
    // stronger model.
    let up = |st: &mut EffortState, rules: &mut Vec<String>, id: &str| {
        st.clean = 0;
        st.progress = 0;
        if st.rung < band.ceiling {
            st.rung += 1;
            st.fails = 0;
            st.moved = Move::Up;
            rules.push(id.to_string());
        } else {
            rules.push(format!("{id}:ceiling"));
            st.fails += 1;
            if st.fails >= NEXT_MODEL_AFTER {
                st.fails = 0;
                rules.push(NEXT_MODEL.into());
            }
        }
    };
    // DE1 and A4 never settle a Flagship or high-stakes turn below its start.
    let settle_floor = if obs.keep_start {
        band.floor.max(start.0)
    } else {
        band.floor
    };
    let mut digest = false;
    let steered = rules.iter().any(|r| r.starts_with("steer:"));
    if let Some(r) = obs
        .routine
        .filter(|r| fresh && user && !hard && !st.hold && !steered && !hold_drops && *r < st.rung)
    {
        st.rung = r.max(band.floor);
        st.moved = Move::Down;
        rules.push("DE2".into());
    }
    let mut recover = false;
    if user {
        if fresh && obs.correction {
            up(&mut st, &mut rules, "E3");
        }
        if !fresh {
            // One rung at most for the loop's trouble, named by its plainest cause.
            let g = &obs.stage;
            let trouble = [
                (g.check_failed, "A4:check_failed"),
                (g.same_tool_failed, "A4:same_tool"),
                (g.looped, "A4:loop"),
                (obs.tool_errors > 0, "E1"),
                (g.explored, "A4:explore"),
                (g.out_of_budget, "E14"),
            ]
            .into_iter()
            .find_map(|(hit, id)| hit.then_some(id));
            if let Some(id) = trouble {
                up(&mut st, &mut rules, id);
            } else {
                if st.last_ok {
                    st.clean += 1;
                } else {
                    st.clean = 0;
                }
                st.progress = if g.progress { st.progress + 1 } else { 0 };
                // A step that only reads tool output thinks at low, for this call
                // only, unless an escalation is in effect.
                digest = g.digest && st.rung <= start.0;
            }
        }
        if let Some(n) = obs.rejects.filter(|n| *n > st.rejects) {
            st.rejects = n;
            st.hold = true;
            up(&mut st, &mut rules, "E2");
            if n >= 3 {
                rules.push("E2:recovery".into());
                recover = true;
            }
        }
        if obs.verify_ok {
            st.hold = false;
        }
        if obs.low_check == Some(true) {
            up(&mut st, &mut rules, "E4");
        }
    }
    if hard && st.rung < high().max(band.floor) {
        st.rung = high().max(band.floor);
        st.moved = Move::Up;
        rules.push("E5".into());
    }
    let settles = user && !hard && !st.hold && !hold_drops && st.rung > settle_floor;
    if settles && st.progress >= SETTLE_STREAK {
        // A4: edits that work and checks that pass settle effort one rung.
        st.rung -= 1;
        st.clean = 0;
        st.progress = 0;
        st.moved = Move::Down;
        rules.push("A4:settle".into());
    } else if settles && st.clean >= 3 {
        st.rung -= 1;
        st.clean = 0;
        st.moved = Move::Down;
        rules.push("DE1".into());
    }
    if !hard && obs.budget_pct.is_some_and(|p| p >= 80) {
        // Below a floor would need an ask card, so DE3 stops at the floor.
        let target = if user {
            band.start
        } else {
            rung("low").unwrap_or(0)
        }
        .max(band.floor);
        if target < st.rung {
            st.rung = target;
            st.moved = Move::Down;
            rules.push("DE3".into());
        }
    }
    st.rung = st.rung.clamp(band.floor, band.ceiling.max(band.floor));
    if st.rung != entry {
        st.fails = 0;
    }
    let mut pick = Pick {
        rung: st.rung,
        moved: st.moved,
        rules,
        recover,
    };
    let low = rung("low").unwrap_or(0).max(band.floor);
    if digest && st.moved != Move::Up && !hard && !st.hold && !hold_drops && low < pick.rung {
        // The saved rung stays: the next step that does more than read is back on it.
        pick.rung = low;
        pick.moved = Move::Down;
        pick.rules.push("A4:digest".into());
    }
    (st, pick)
}

/// Clamp a rung to a model's effort list. Going up rounds up to the next
/// offered level; going down (or starting) rounds down. Never below `floor`
/// when the list offers something at or above it. `None` when the list is empty.
pub fn clamp(r: usize, menu: &[String], moved: Move, floor: usize) -> Option<String> {
    let mut offered: Vec<(usize, &String)> = menu
        .iter()
        .filter_map(|m| rung(m).map(|x| (x, m)))
        .collect();
    offered.sort_by_key(|(x, _)| *x);
    let up = || offered.iter().find(|(x, _)| *x >= r).or(offered.last());
    let down = || {
        offered
            .iter()
            .rev()
            .find(|(x, _)| *x <= r)
            .or(offered.first())
    };
    let mut pick = if moved == Move::Up { up() } else { down() };
    if pick.is_some_and(|(x, _)| *x < floor) {
        pick = offered.iter().find(|(x, _)| *x >= floor).or(pick);
    }
    pick.map(|(_, m)| m.to_string())
}

/// Live states, by episode and class. Capped so a long-running cabin stays small.
static STATES: Mutex<BTreeMap<String, EffortState>> = Mutex::new(BTreeMap::new());
const STATES_CAP: usize = 256;

thread_local! {
    static TOOL_ERRORS: Cell<u32> = const { Cell::new(0) };
}

/// E1: a tool on this thread failed. The next user-facing routed call reads it.
pub fn note_tool_error() {
    TOOL_ERRORS.with(|c| c.set(c.get().saturating_add(1)));
}

pub fn take_tool_errors() -> u32 {
    TOOL_ERRORS.with(|c| c.replace(0))
}

/// What one tool run was, for the A4 signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// A read, a listing, a search or a web lookup.
    Lookup,
    /// A file write or edit.
    Edit,
    /// A shell command that runs tests or a build.
    Check,
    Other,
}

/// Shell commands that run tests or a build.
const CHECKS: &[&str] = &[
    "cargo test",
    "cargo nextest",
    "cargo build",
    "cargo check",
    "cargo clippy",
    "npm test",
    "npm run test",
    "npm run build",
    "pnpm test",
    "pnpm build",
    "yarn test",
    "yarn build",
    "npx tsc",
    "tsc ",
    "vitest",
    "npx vitest",
    "jest",
    "npx jest",
    "pytest",
    "python -m pytest",
    "go test",
    "go build",
    "mvn ",
    "gradle",
    "dotnet build",
    "dotnet test",
    "make ",
    "ctest",
];

impl ToolKind {
    pub fn of(name: &str, arguments: &str) -> Self {
        match name {
            "write" | "search_replace" => Self::Edit,
            "web_search" | "web_fetch" => Self::Lookup,
            "run_terminal_command" => {
                let cmd = serde_json::from_str::<serde_json::Value>(arguments)
                    .ok()
                    .and_then(|v| v.get("command")?.as_str().map(str::to_ascii_lowercase))
                    .unwrap_or_default();
                // A trailing space so `make` matches as a word, not `makepkg`.
                let cmd = format!("{} ", cmd.trim());
                if CHECKS
                    .iter()
                    .any(|c| cmd.starts_with(c) || cmd.contains(&format!("&& {c}")))
                {
                    Self::Check
                } else {
                    Self::Other
                }
            }
            n if crate::gate::is_readonly(n) => Self::Lookup,
            _ => Self::Other,
        }
    }
}

/// One turn's tool runs so far, and what they say since the last routed call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StageTrack {
    /// Tool runs since the last routed call, and whether each was a working lookup.
    window: u32,
    window_reads_ok: bool,
    window_failed: bool,
    pending: Stage,
    /// The last tool that ran, and whether it failed.
    last: Option<(String, bool)>,
    /// FNV-1a of each `(name, arguments)` that ran this turn.
    seen: Vec<u64>,
    lookups: u32,
    edited: bool,
}

impl StageTrack {
    /// One tool that ran (never a call the user denied or a gate refused).
    pub fn note(&mut self, name: &str, arguments: &str, failed: bool) {
        let kind = ToolKind::of(name, arguments);
        let g = &mut self.pending;
        if self.window == 0 {
            self.window_reads_ok = true;
        }
        self.window += 1;
        self.window_reads_ok &= kind == ToolKind::Lookup && !failed;
        self.window_failed |= failed;
        if failed && self.last.as_ref().is_some_and(|(n, f)| *f && n == name) {
            g.same_tool_failed = true;
        }
        self.last = Some((name.to_string(), failed));
        let call = turn_hash(&format!("{name}\u{1f}{arguments}"));
        if self.seen.contains(&call) {
            g.looped = true;
        } else if self.seen.len() < 512 {
            self.seen.push(call);
        }
        match kind {
            ToolKind::Lookup => {
                self.lookups += 1;
                if !self.edited && self.lookups.is_multiple_of(EXPLORE_LIMIT) {
                    g.explored = true;
                }
            }
            ToolKind::Edit => {
                self.edited |= !failed;
                g.progress |= !failed;
            }
            ToolKind::Check if failed && self.edited => g.check_failed = true,
            ToolKind::Check => g.progress |= !failed,
            ToolKind::Other => {}
        }
    }

    /// What the next routed call reads; the turn's history stays.
    pub fn take(&mut self) -> Stage {
        let mut g = std::mem::take(&mut self.pending);
        g.digest = self.window > 0 && self.window_reads_ok;
        g.progress &= !self.window_failed;
        self.window = 0;
        self.window_failed = false;
        g
    }
}

thread_local! {
    static STAGE: std::cell::RefCell<StageTrack> = std::cell::RefCell::new(StageTrack::default());
}

/// A4: a tool on this thread ran. A failed run is also E1 ([`note_tool_error`]).
pub fn note_tool_run(name: &str, arguments: &str, failed: bool) {
    if failed {
        note_tool_error();
    }
    STAGE.with(|t| t.borrow_mut().note(name, arguments, failed));
}

/// E14: the model's reasoning used up its output budget and left no answer.
pub fn note_out_of_budget() {
    STAGE.with(|t| t.borrow_mut().pending.out_of_budget = true);
}

/// The stage signals for this thread's next routed call. A new turn starts the
/// turn's history over and reads nothing from the turn before.
pub fn take_stage(new_turn: bool) -> Stage {
    STAGE.with(|t| {
        let mut t = t.borrow_mut();
        if new_turn {
            *t = StageTrack::default();
            Stage::default()
        } else {
            t.take()
        }
    })
}

fn key(episode: &str, class: &str) -> String {
    format!("{episode}\u{1f}{class}")
}

/// FNV-1a of the user's newest text: tells a new turn from a tool step without keeping the text.
pub fn turn_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Advance the saved state for `episode` and `class`.
pub fn step(
    episode: &str,
    class: &str,
    turn: u64,
    band: Band,
    start: (usize, Vec<String>),
    obs: Obs,
) -> Pick {
    step_holding(episode, class, turn, band, start, obs, false)
}

pub fn step_holding(
    episode: &str,
    class: &str,
    turn: u64,
    band: Band,
    start: (usize, Vec<String>),
    mut obs: Obs,
    hold_drops: bool,
) -> Pick {
    let k = key(episode, class);
    let mut states = STATES.lock().unwrap_or_else(|e| e.into_inner());
    let prev = states.get(&k).cloned();
    obs.new_turn = prev.as_ref().is_none_or(|p| p.turn != turn);
    let (mut st, pick) = advance_holding(prev.as_ref(), class, band, start, &obs, hold_drops);
    st.turn = turn;
    st.last_ok = false;
    if states.len() >= STATES_CAP && !states.contains_key(&k) {
        states.clear();
    }
    states.insert(k, st);
    pick
}

/// How the call went (DE1 counts clean steps).
pub fn finish(episode: &str, class: &str, ok: bool) {
    if let Some(st) = STATES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(&key(episode, class))
    {
        st.last_ok = ok;
    }
}

/// This call starts a new turn for `episode` in `class` (or its first call).
pub fn is_new_turn(episode: &str, class: &str, turn: u64) -> bool {
    STATES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key(episode, class))
        .is_none_or(|s| s.turn != turn)
}

/// The learner ([`super::bandit`]) moved this turn's first call to `rung`:
/// the turn's later steps climb from there.
pub fn set_rung(episode: &str, class: &str, rung: usize) {
    if let Some(st) = STATES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(&key(episode, class))
    {
        if st.rung != rung {
            st.rung = rung;
            st.fails = 0;
        }
    }
}

/// The rung this episode is on now, if it has routed a call in `class`.
pub fn current(episode: &str, class: &str) -> Option<usize> {
    STATES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key(episode, class))
        .map(|s| s.rung)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::policy::class_row;

    fn band(class: &str) -> Band {
        Band::of(class_row(class).unwrap(), 0)
    }

    fn name(r: usize) -> &'static str {
        EFFORT_LADDER[r]
    }

    fn go(prev: Option<&EffortState>, class: &str, obs: Obs) -> (EffortState, Pick) {
        let b = band(class);
        advance(
            prev,
            class,
            b,
            start_rung(b, 0.5, Steer::None, user_facing(class)),
            &obs,
        )
    }

    fn step_obs() -> Obs {
        Obs {
            new_turn: false,
            ..Obs::default()
        }
    }

    #[test]
    fn steer_reads_plain_words() {
        assert_eq!(
            Steer::from_text("Please think hard on this one"),
            Steer::Harder
        );
        assert_eq!(
            Steer::from_text("keep it quick, just the name"),
            Steer::Quicker
        );
        assert_eq!(Steer::from_text("rename the file"), Steer::None);
    }

    #[test]
    fn care_first_holds_chat_default_and_ignores_keyword_steer() {
        let chat = band("chat:default");
        let words = "please think hard about this";
        assert!(!hold_chat_drops("chat:default", false));
        assert!(hold_chat_drops("chat:default", true));
        assert!(!hold_chat_drops("chat:quick", true));
        assert!(!hold_chat_drops("plan", true));
        assert_eq!(steer_for_turn(words, false), Steer::Harder);
        assert_eq!(steer_for_turn(words, true), Steer::None);
        assert_eq!(name(start_rung(chat, 0.1, Steer::None, true).0), "low");
        assert_eq!(
            name(start_holding(chat, 0.1, Steer::None, true, true).0),
            "medium"
        );
        assert!(start_holding(chat, 0.1, Steer::None, true, true)
            .1
            .is_empty());
        assert_eq!(
            name(start_holding(chat, 0.9, Steer::None, true, true).0),
            "high"
        );
        let start = start_holding(chat, 0.5, Steer::None, true, true);
        let (mut state, first) = advance_holding(
            None,
            "chat:default",
            chat,
            start.clone(),
            &Obs {
                new_turn: true,
                ..Obs::default()
            },
            true,
        );
        assert_eq!(name(first.rung), "medium");
        for _ in 0..5 {
            state.last_ok = true;
            let (next, pick) = advance_holding(
                Some(&state),
                "chat:default",
                chat,
                start.clone(),
                &step_obs(),
                true,
            );
            state = next;
            assert_eq!(name(pick.rung), "medium");
            assert!(!pick.rules.iter().any(|rule| rule == "DE1"));
        }
        let (raised, up) = advance_holding(
            Some(&state),
            "chat:default",
            chat,
            start.clone(),
            &Obs {
                low_check: Some(true),
                ..step_obs()
            },
            true,
        );
        assert_eq!(name(up.rung), "high");
        let (_, budget) = advance_holding(
            Some(&raised),
            "chat:default",
            chat,
            start,
            &Obs {
                budget_pct: Some(80),
                ..step_obs()
            },
            true,
        );
        assert_eq!(name(budget.rung), "medium");
        assert!(budget.rules.iter().any(|rule| rule == "DE3"));
        let plan = band("plan");
        assert_eq!(
            name(start_holding(plan, 0.0, Steer::None, true, false).0),
            name(start_rung(plan, 0.0, Steer::None, true).0)
        );
    }

    #[test]
    fn start_follows_d_and_steer_and_never_starts_above_high_on_d_alone() {
        let chat = band("chat:default");
        assert_eq!(
            start_rung(chat, 0.5, Steer::None, true),
            (rung("medium").unwrap(), vec![])
        );
        assert_eq!(name(start_rung(chat, 0.1, Steer::None, true).0), "low");
        assert_eq!(name(start_rung(chat, 0.9, Steer::None, true).0), "high");
        assert_eq!(
            name(start_rung(band("plan"), 0.9, Steer::None, true).0),
            "high",
            "d alone never reaches xhigh"
        );
        assert_eq!(name(start_rung(chat, 0.0, Steer::Harder, true).0), "xhigh");
        assert_eq!(name(start_rung(chat, 0.9, Steer::Quicker, true).0), "low");
        // Background runs at its start whatever d says.
        assert_eq!(
            name(start_rung(band("background:summarize"), 0.0, Steer::None, false).0),
            "low"
        );
    }

    #[test]
    fn e1_tool_error_climbs_one_rung() {
        let (s0, p0) = go(
            None,
            "chat:default",
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        );
        assert_eq!(name(p0.rung), "medium");
        let (_, p1) = go(
            Some(&s0),
            "chat:default",
            Obs {
                tool_errors: 3,
                ..step_obs()
            },
        );
        assert_eq!(
            (name(p1.rung), p1.moved, p1.rules.clone()),
            ("high", Move::Up, vec!["E1".to_string()])
        );
    }

    /// Moved from the retired steady-effort tests: with no switch any more,
    /// every failed tool step climbs one rung, up to the class ceiling.
    #[test]
    fn each_failed_tool_step_climbs_one_rung_up_to_the_ceiling() {
        let turn = |errors: &[u32]| {
            let (mut s, _) = go(
                None,
                "chat:default",
                Obs {
                    new_turn: true,
                    ..Obs::default()
                },
            );
            let mut out = Vec::new();
            for n in errors {
                s.last_ok = true;
                let (next, pick) = go(
                    Some(&s),
                    "chat:default",
                    Obs {
                        tool_errors: *n,
                        ..Obs::default()
                    },
                );
                out.push((name(pick.rung), pick.rules));
                s = next;
            }
            out
        };
        let rules = |r: &[&str]| r.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            turn(&[1, 0]),
            vec![("high", rules(&["E1"])), ("high", rules(&[]))]
        );
        assert_eq!(
            turn(&[1, 1, 1, 0]),
            vec![
                ("high", rules(&["E1"])),
                ("xhigh", rules(&["E1"])),
                ("xhigh", rules(&["E1:ceiling"])),
                ("xhigh", rules(&[])),
            ]
        );
    }

    #[test]
    fn e2_rejects_climb_then_name_the_next_model_then_hand_to_recovery() {
        // chat:default starts at medium so two rejects reach the ceiling without
        // also tripping the third-reject recovery path (code:edit-small starts at low).
        let (s0, _) = go(
            None,
            "chat:default",
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        );
        let (s1, p1) = go(
            Some(&s0),
            "chat:default",
            Obs {
                rejects: Some(1),
                ..step_obs()
            },
        );
        assert_eq!(
            (name(p1.rung), p1.rules.clone(), s1.hold),
            ("high", vec!["E2".to_string()], true)
        );
        let (mut s2, p2) = go(
            Some(&s1),
            "chat:default",
            Obs {
                rejects: Some(2),
                ..step_obs()
            },
        );
        assert_eq!((name(p2.rung), p2.recover), ("xhigh", false));
        // E15: one reject at the ceiling holds the model; the second one at
        // that rung asks for a stronger model.
        s2.rejects = 1;
        let (s3, p3) = go(
            Some(&s2),
            "chat:default",
            Obs {
                rejects: Some(2),
                ..step_obs()
            },
        );
        assert_eq!(p3.rules, vec!["E2:ceiling".to_string()]);
        assert_eq!(name(p3.rung), "xhigh", "the ceiling holds");
        let (_, p4) = go(
            Some(&s3),
            "chat:default",
            Obs {
                rejects: Some(3),
                ..step_obs()
            },
        );
        assert_eq!(
            (p4.recover, p4.rules.clone()),
            (
                true,
                vec![
                    "E2:ceiling".to_string(),
                    NEXT_MODEL.into(),
                    "E2:recovery".into()
                ]
            )
        );
        // No VerifyGate source (None) fires nothing.
        let (_, quiet) = go(
            Some(&s0),
            "chat:default",
            Obs {
                rejects: None,
                ..step_obs()
            },
        );
        assert!(!quiet.rules.iter().any(|r| r.starts_with("E2")));
    }

    #[test]
    fn e3_correction_starts_the_next_turn_one_higher() {
        let (s0, _) = go(
            None,
            "chat:default",
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        );
        let (_, p) = go(
            Some(&s0),
            "chat:default",
            Obs {
                new_turn: true,
                correction: true,
                ..Obs::default()
            },
        );
        assert_eq!(
            (name(p.rung), p.rules.clone()),
            ("high", vec!["E3".to_string()])
        );
    }

    #[test]
    fn e4_low_self_check_climbs_and_none_does_nothing() {
        let (s0, _) = go(
            None,
            "chat:default",
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        );
        let (_, p) = go(
            Some(&s0),
            "chat:default",
            Obs {
                low_check: Some(true),
                ..step_obs()
            },
        );
        assert_eq!(
            (name(p.rung), p.rules.clone()),
            ("high", vec!["E4".to_string()])
        );
        let (_, none) = go(
            Some(&s0),
            "chat:default",
            Obs {
                low_check: None,
                ..step_obs()
            },
        );
        assert_eq!(name(none.rung), "medium");
    }

    #[test]
    fn e5_prepare_hard_starts_at_high_even_when_asked_to_be_quick() {
        let b = band(PREPARE_HARD);
        let (_, p) = advance(
            None,
            PREPARE_HARD,
            b,
            start_rung(b, 0.0, Steer::Quicker, true),
            &Obs {
                new_turn: true,
                ..Obs::default()
            },
        );
        assert_eq!(name(p.rung), "high");
        // Even a band whose floor sat lower gets lifted to High by E5.
        let low = Band {
            floor: rung("low").unwrap(),
            start: rung("low").unwrap(),
            ceiling: rung("xhigh").unwrap(),
        };
        let (_, p) = advance(
            None,
            PREPARE_HARD,
            low,
            (low.start, vec![]),
            &Obs {
                new_turn: true,
                ..Obs::default()
            },
        );
        assert_eq!(
            (name(p.rung), p.rules.clone()),
            ("high", vec!["E5".to_string()])
        );
    }

    #[test]
    fn de1_three_clean_steps_step_down_to_the_floor_but_not_after_a_reject_or_in_prepare_hard() {
        let mut s = go(
            None,
            "chat:default",
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        )
        .0;
        let mut rungs = Vec::new();
        for _ in 0..7 {
            s.last_ok = true;
            let (n, p) = go(Some(&s), "chat:default", step_obs());
            rungs.push(name(p.rung));
            s = n;
        }
        assert_eq!(
            rungs,
            vec!["medium", "medium", "low", "low", "low", "low", "low"],
            "DE1 stops at the floor"
        );
        // After a reject: hold until VERIFY_OK.
        let mut s = go(
            None,
            "chat:default",
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        )
        .0;
        s = go(
            Some(&s),
            "chat:default",
            Obs {
                rejects: Some(1),
                ..step_obs()
            },
        )
        .0;
        for _ in 0..4 {
            s.last_ok = true;
            s = go(Some(&s), "chat:default", step_obs()).0;
        }
        assert_eq!(name(s.rung), "high", "held after the reject");
        s.last_ok = true;
        let (s2, p) = go(
            Some(&s),
            "chat:default",
            Obs {
                verify_ok: true,
                ..step_obs()
            },
        );
        assert_eq!(
            (name(p.rung), p.rules.clone()),
            ("medium", vec!["DE1".to_string()])
        );
        assert!(!s2.hold);
        // prepare:hard never steps down.
        let mut s = go(
            None,
            PREPARE_HARD,
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        )
        .0;
        s = go(
            Some(&s),
            PREPARE_HARD,
            Obs {
                tool_errors: 1,
                ..step_obs()
            },
        )
        .0;
        assert_eq!(name(s.rung), "xhigh");
        for _ in 0..5 {
            s.last_ok = true;
            s = go(Some(&s), PREPARE_HARD, step_obs()).0;
        }
        assert_eq!(name(s.rung), "xhigh");
    }

    #[test]
    fn de3_budget_pressure_lowers_background_to_low_and_user_work_to_start_never_below_the_floor() {
        let mut s = go(
            None,
            "chat:default",
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        )
        .0;
        s = go(
            Some(&s),
            "chat:default",
            Obs {
                tool_errors: 1,
                ..step_obs()
            },
        )
        .0;
        s = go(
            Some(&s),
            "chat:default",
            Obs {
                tool_errors: 1,
                ..step_obs()
            },
        )
        .0;
        assert_eq!(name(s.rung), "xhigh");
        let (_, p) = go(
            Some(&s),
            "chat:default",
            Obs {
                budget_pct: Some(85),
                ..step_obs()
            },
        );
        assert_eq!(
            (name(p.rung), p.rules.clone()),
            ("medium", vec!["DE3".to_string()])
        );
        let (_, p) = go(
            Some(&s),
            "chat:default",
            Obs {
                budget_pct: Some(79),
                ..step_obs()
            },
        );
        assert_eq!(name(p.rung), "xhigh");
        let judge = EffortState {
            rung: rung("high").unwrap(),
            ..EffortState::default()
        };
        let (_, p) = go(
            Some(&judge),
            "background:judge",
            Obs {
                budget_pct: Some(90),
                ..step_obs()
            },
        );
        assert_eq!(
            (name(p.rung), p.rules.clone()),
            ("low", vec!["DE3".to_string()])
        );
        let hard = EffortState {
            rung: rung("xhigh").unwrap(),
            ..EffortState::default()
        };
        let (_, p) = go(
            Some(&hard),
            PREPARE_HARD,
            Obs {
                budget_pct: Some(99),
                ..step_obs()
            },
        );
        assert_eq!(name(p.rung), "xhigh", "prepare:hard ignores DE3");
    }

    const DAY: u64 = 24 * 60 * 60 * 1000;

    fn wins(rung_name: &str, days_ago: &[u64], now: u64) -> Routine {
        Routine {
            successes: days_ago
                .iter()
                .map(|d| (rung(rung_name).unwrap(), now - d * DAY))
                .collect(),
            last_failure: None,
        }
    }

    #[test]
    fn de2_three_low_successes_in_14_days_start_the_next_run_at_low() {
        let now = 100 * DAY;
        let b = band("chat:default");
        let r = routine_rung("chat:default", &wins("low", &[1, 5, 13], now), b, now);
        assert_eq!(r, Some(rung("low").unwrap()));
        let (_, p) = go(
            None,
            "chat:default",
            Obs {
                new_turn: true,
                routine: r,
                ..Obs::default()
            },
        );
        assert_eq!(
            (name(p.rung), p.moved, p.rules.clone()),
            ("low", Move::Down, vec!["DE2".to_string()])
        );
        // Two successes, or three spread over 20 days, are not a routine.
        assert_eq!(
            routine_rung("chat:default", &wins("low", &[1, 5], now), b, now),
            None
        );
        assert_eq!(
            routine_rung("chat:default", &wins("low", &[1, 10, 20], now), b, now),
            None
        );
        // A failure after them resets the count.
        let mut failed = wins("low", &[3, 5, 6], now);
        failed.last_failure = Some(now - 2 * DAY);
        assert_eq!(routine_rung("chat:default", &failed, b, now), None);
        // Successes at the start rung or above lower nothing.
        assert_eq!(
            routine_rung("chat:default", &wins("medium", &[1, 2, 3], now), b, now),
            None
        );
    }

    #[test]
    fn de2_never_lowers_prepare_hard_never_goes_below_the_floor_and_waits_out_a_reject() {
        let now = 100 * DAY;
        let hard = band(PREPARE_HARD);
        assert_eq!(
            routine_rung(PREPARE_HARD, &wins("low", &[1, 2, 3], now), hard, now),
            None
        );
        // Even handed a low rung, advance keeps prepare:hard at High.
        let (_, p) = advance(
            None,
            PREPARE_HARD,
            hard,
            (hard.start, vec![]),
            &Obs {
                new_turn: true,
                routine: Some(rung("low").unwrap()),
                ..Obs::default()
            },
        );
        assert_eq!(name(p.rung), "high");
        assert!(!p.rules.contains(&"DE2".to_string()));
        // code:multi-file's floor is medium: successes at low don't count below it.
        let multi = band("code:multi-file");
        assert_eq!(
            routine_rung("code:multi-file", &wins("low", &[1, 2, 3], now), multi, now),
            None
        );
        assert_eq!(
            routine_rung(
                "code:multi-file",
                &wins("medium", &[1, 2, 3], now),
                multi,
                now
            ),
            Some(rung("medium").unwrap())
        );
        // After a reject, the next turn keeps its hold: no DE2 until VERIFY_OK.
        let mut s = go(
            None,
            "chat:default",
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        )
        .0;
        s = go(
            Some(&s),
            "chat:default",
            Obs {
                rejects: Some(1),
                ..step_obs()
            },
        )
        .0;
        let low = Some(rung("low").unwrap());
        let (_, p) = go(
            Some(&s),
            "chat:default",
            Obs {
                new_turn: true,
                routine: low,
                ..Obs::default()
            },
        );
        assert_eq!(name(p.rung), "medium");
        // "think hard" wins over a routine.
        let b = band("chat:default");
        let (_, p) = advance(
            None,
            "chat:default",
            b,
            start_rung(b, 0.5, Steer::Harder, true),
            &Obs {
                new_turn: true,
                routine: low,
                ..Obs::default()
            },
        );
        assert_eq!(name(p.rung), "xhigh");
    }

    #[test]
    fn background_classes_never_escalate() {
        let (s, p) = go(
            None,
            "background:compact",
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        );
        assert_eq!(name(p.rung), "low");
        let (_, p) = go(
            Some(&s),
            "background:compact",
            Obs {
                tool_errors: 4,
                rejects: Some(2),
                correction: true,
                ..step_obs()
            },
        );
        assert_eq!((name(p.rung), p.rules.len()), ("low", 0));
    }

    #[test]
    fn clamp_rounds_up_going_up_and_down_otherwise_and_keeps_the_floor() {
        let menu: Vec<String> = ["low", "high"].iter().map(|s| s.to_string()).collect();
        let medium = rung("medium").unwrap();
        assert_eq!(clamp(medium, &menu, Move::Up, 0).as_deref(), Some("high"));
        assert_eq!(clamp(medium, &menu, Move::Down, 0).as_deref(), Some("low"));
        assert_eq!(clamp(medium, &menu, Move::Start, 0).as_deref(), Some("low"));
        assert_eq!(
            clamp(rung("max").unwrap(), &menu, Move::Up, 0).as_deref(),
            Some("high")
        );
        assert_eq!(
            clamp(rung("none").unwrap(), &menu, Move::Down, 0).as_deref(),
            Some("low")
        );
        // A floor of high is kept even when rounding down would drop below it.
        let menu2: Vec<String> = ["low", "medium", "xhigh"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            clamp(high(), &menu2, Move::Start, high()).as_deref(),
            Some("xhigh")
        );
        assert_eq!(clamp(medium, &[], Move::Up, 0), None);
    }

    #[test]
    fn saved_state_tells_turns_apart_and_finish_counts_clean_steps() {
        let b = band("chat:default");
        let ep = "ladder-state-test";
        let t1 = turn_hash("first ask");
        let p = step(
            ep,
            "chat:default",
            t1,
            b,
            start_rung(b, 0.5, Steer::Harder, true),
            Obs::default(),
        );
        assert_eq!(name(p.rung), "xhigh");
        finish(ep, "chat:default", true);
        let p = step(ep, "chat:default", t1, b, (b.start, vec![]), Obs::default());
        assert_eq!(name(p.rung), "xhigh", "the same turn keeps its steer");
        assert_eq!(current(ep, "chat:default"), Some(rung("xhigh").unwrap()));
        let p = step(
            ep,
            "chat:default",
            turn_hash("next ask"),
            b,
            (b.start, vec![]),
            Obs::default(),
        );
        assert_eq!(name(p.rung), "medium", "a new turn drops the steer");
        note_tool_error();
        note_tool_error();
        assert_eq!((take_tool_errors(), take_tool_errors()), (2, 0));
    }

    fn staged(stage: Stage) -> Obs {
        Obs {
            stage,
            ..step_obs()
        }
    }

    /// Run `steps` after a fresh turn in `class`; each step's call came back fine.
    fn walk(class: &str, keep_start: bool, steps: &[Obs]) -> Vec<(&'static str, Vec<String>)> {
        let (mut s, _) = go(
            None,
            class,
            Obs {
                new_turn: true,
                ..Obs::default()
            },
        );
        let mut out = Vec::new();
        for obs in steps {
            s.last_ok = true;
            let (next, pick) = go(
                Some(&s),
                class,
                Obs {
                    keep_start,
                    ..obs.clone()
                },
            );
            out.push((name(pick.rung), pick.rules));
            s = next;
        }
        out
    }

    fn ids(r: &[&str]) -> Vec<String> {
        r.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn e15_effort_climbs_first_and_two_failures_at_one_rung_ask_for_a_stronger_model() {
        let fail = Obs {
            tool_errors: 1,
            ..step_obs()
        };
        assert_eq!(
            walk(
                "chat:default",
                false,
                &[
                    fail.clone(),
                    fail.clone(),
                    fail.clone(),
                    fail.clone(),
                    fail.clone(),
                    staged(Stage {
                        out_of_budget: true,
                        ..Stage::default()
                    }),
                ]
            ),
            vec![
                ("high", ids(&["E1"])),
                ("xhigh", ids(&["E1"])),
                ("xhigh", ids(&["E1:ceiling"])),
                ("xhigh", ids(&["E1:ceiling", NEXT_MODEL])),
                // The stronger model starts its own count at the same rung.
                ("xhigh", ids(&["E1:ceiling"])),
                ("xhigh", ids(&["E14:ceiling", NEXT_MODEL])),
            ]
        );
        // A rung change starts the count over: one failure at the ceiling,
        // a settle, then a climb back is not two at one rung.
        let ok = staged(Stage {
            progress: true,
            ..Stage::default()
        });
        assert_eq!(
            walk(
                "chat:default",
                false,
                &[
                    fail.clone(),
                    fail.clone(),
                    fail.clone(),
                    ok.clone(),
                    ok,
                    fail.clone(),
                    fail,
                ]
            ),
            vec![
                ("high", ids(&["E1"])),
                ("xhigh", ids(&["E1"])),
                ("xhigh", ids(&["E1:ceiling"])),
                ("xhigh", ids(&[])),
                ("high", ids(&["A4:settle"])),
                ("xhigh", ids(&["E1"])),
                ("xhigh", ids(&["E1:ceiling"])),
            ]
        );
    }

    #[test]
    fn a4_trouble_in_the_loop_climbs_one_rung_named_by_its_cause() {
        let one = |stage: Stage, errors: u32| {
            walk(
                "chat:default",
                false,
                &[Obs {
                    tool_errors: errors,
                    ..staged(stage)
                }],
            )
            .remove(0)
        };
        let cases = [
            (
                Stage {
                    same_tool_failed: true,
                    ..Stage::default()
                },
                2,
                "A4:same_tool",
            ),
            (
                Stage {
                    looped: true,
                    ..Stage::default()
                },
                0,
                "A4:loop",
            ),
            (
                Stage {
                    check_failed: true,
                    ..Stage::default()
                },
                1,
                "A4:check_failed",
            ),
            (
                Stage {
                    explored: true,
                    ..Stage::default()
                },
                0,
                "A4:explore",
            ),
            (
                Stage {
                    out_of_budget: true,
                    ..Stage::default()
                },
                0,
                "E14",
            ),
            // Everything at once is still one rung.
            (
                Stage {
                    same_tool_failed: true,
                    looped: true,
                    check_failed: true,
                    explored: true,
                    out_of_budget: true,
                    ..Stage::default()
                },
                3,
                "A4:check_failed",
            ),
        ];
        for (stage, errors, id) in cases {
            assert_eq!(one(stage, errors), ("high", ids(&[id])), "{id}");
        }
        // The first step of a turn reads nothing from the loop.
        let (_, first) = go(
            None,
            "chat:default",
            Obs {
                new_turn: true,
                stage: Stage {
                    looped: true,
                    ..Stage::default()
                },
                ..Obs::default()
            },
        );
        assert_eq!((name(first.rung), first.rules), ("medium", ids(&[])));
    }

    #[test]
    fn a4_a_step_that_only_reads_tool_output_runs_at_low_for_that_call() {
        let read = staged(Stage {
            digest: true,
            ..Stage::default()
        });
        let fail = Obs {
            tool_errors: 1,
            ..step_obs()
        };
        assert_eq!(
            walk(
                "chat:default",
                false,
                &[read.clone(), step_obs(), fail, read.clone()]
            ),
            vec![
                ("low", ids(&["A4:digest"])),
                // The saved rung never moved, so the next step is back at the start.
                ("medium", ids(&[])),
                ("high", ids(&["E1"])),
                // An escalation in effect holds: no digest drop above the start.
                ("high", ids(&[])),
            ]
        );
        // Never below the lane's floor: debug's floor is medium, prepare:hard's high.
        assert_eq!(
            walk("debug", true, std::slice::from_ref(&read)),
            vec![("medium", ids(&["A4:digest"]))]
        );
        assert_eq!(walk(PREPARE_HARD, true, &[read]), vec![("high", ids(&[]))]);
    }

    #[test]
    fn a4_working_edits_and_passing_checks_settle_one_rung_never_below_a_flagship_start() {
        let fail = Obs {
            tool_errors: 1,
            ..step_obs()
        };
        let ok = staged(Stage {
            progress: true,
            ..Stage::default()
        });
        // chat:default: up to xhigh, then two working steps settle one rung each time.
        assert_eq!(
            walk(
                "chat:default",
                false,
                &[
                    fail.clone(),
                    fail.clone(),
                    ok.clone(),
                    ok.clone(),
                    ok.clone(),
                    ok.clone(),
                    ok.clone(),
                    ok.clone()
                ]
            )
            .into_iter()
            .map(|(r, _)| r)
            .collect::<Vec<_>>(),
            vec!["high", "xhigh", "xhigh", "high", "high", "medium", "medium", "low"]
        );
        // plan is Flagship: it settles back to its start (high) and no lower,
        // and DE1's clean steps don't take it lower either.
        let plan = walk(
            "plan",
            true,
            &[
                fail.clone(),
                ok.clone(),
                ok.clone(),
                ok.clone(),
                ok.clone(),
                step_obs(),
                step_obs(),
                step_obs(),
            ],
        );
        assert_eq!(
            plan.iter().map(|(r, _)| *r).collect::<Vec<_>>(),
            vec!["xhigh", "xhigh", "high", "high", "high", "high", "high", "high"]
        );
        assert_eq!(plan[2].1, ids(&["A4:settle"]));
        // Without keep_start the same steps go down to plan's floor.
        assert_eq!(
            walk("plan", false, &[ok.clone(), ok, step_obs()])
                .into_iter()
                .map(|(r, _)| r)
                .collect::<Vec<_>>(),
            vec!["high", "medium", "medium"]
        );
        // A step with a failure in it breaks the streak.
        assert_eq!(
            walk(
                "chat:default",
                false,
                &[
                    fail.clone(),
                    staged(Stage {
                        progress: true,
                        ..Stage::default()
                    }),
                    fail,
                    staged(Stage {
                        progress: true,
                        ..Stage::default()
                    })
                ]
            )
            .into_iter()
            .map(|(r, _)| r)
            .collect::<Vec<_>>(),
            vec!["high", "high", "xhigh", "xhigh"]
        );
    }

    fn run(track: &mut StageTrack, name: &str, args: &str, failed: bool) {
        track.note(name, args, failed);
    }

    #[test]
    fn stage_track_reads_the_loop_into_typed_signals() {
        assert_eq!(ToolKind::of("read_file", "{}"), ToolKind::Lookup);
        assert_eq!(ToolKind::of("grep", "{}"), ToolKind::Lookup);
        assert_eq!(ToolKind::of("web_search", "{}"), ToolKind::Lookup);
        assert_eq!(ToolKind::of("search_replace", "{}"), ToolKind::Edit);
        assert_eq!(
            ToolKind::of("run_terminal_command", r#"{"command":"cargo test -p app"}"#),
            ToolKind::Check
        );
        assert_eq!(
            ToolKind::of("run_terminal_command", r#"{"command":"cd ui && npm test"}"#),
            ToolKind::Check
        );
        assert_eq!(
            ToolKind::of("run_terminal_command", r#"{"command":"make"}"#),
            ToolKind::Check
        );
        assert_eq!(
            ToolKind::of("run_terminal_command", r#"{"command":"makepkg -si"}"#),
            ToolKind::Other
        );
        assert_eq!(
            ToolKind::of("run_terminal_command", r#"{"command":"ls -la"}"#),
            ToolKind::Other
        );

        let mut t = StageTrack::default();
        assert_eq!(t.take(), Stage::default(), "nothing ran");
        run(&mut t, "read_file", r#"{"path":"a.rs"}"#, false);
        run(&mut t, "grep", r#"{"pattern":"fn main"}"#, false);
        assert_eq!(
            t.take(),
            Stage {
                digest: true,
                ..Stage::default()
            }
        );
        // The same call again is a loop; reads that failed are no digest.
        run(&mut t, "read_file", r#"{"path":"a.rs"}"#, false);
        assert_eq!(
            t.take(),
            Stage {
                digest: true,
                looped: true,
                ..Stage::default()
            }
        );
        run(&mut t, "read_file", r#"{"path":"gone.rs"}"#, true);
        run(&mut t, "read_file", r#"{"path":"gone2.rs"}"#, true);
        assert_eq!(
            t.take(),
            Stage {
                same_tool_failed: true,
                ..Stage::default()
            }
        );
        // A different tool failing in between is not "the same tool twice".
        run(&mut t, "grep", r#"{"pattern":"x"}"#, true);
        run(&mut t, "read_file", r#"{"path":"b.rs"}"#, true);
        assert_eq!(t.take(), Stage::default());
        // Eight lookups and no edit: a long exploration, once per eight.
        let mut t = StageTrack::default();
        for i in 0..7 {
            run(
                &mut t,
                "read_file",
                &format!(r#"{{"path":"f{i}.rs"}}"#),
                false,
            );
        }
        assert!(!t.take().explored);
        run(&mut t, "glob", r#"{"pattern":"*.rs"}"#, false);
        assert_eq!(
            t.take(),
            Stage {
                digest: true,
                explored: true,
                ..Stage::default()
            }
        );
        // An edit that works is progress; a failing test run after it is a check failure.
        run(
            &mut t,
            "search_replace",
            r#"{"path":"a.rs","old":"1","new":"2"}"#,
            false,
        );
        assert_eq!(
            t.take(),
            Stage {
                progress: true,
                ..Stage::default()
            }
        );
        let test = r#"{"command":"cargo test"}"#;
        run(&mut t, "run_terminal_command", test, true);
        assert_eq!(
            t.take(),
            Stage {
                check_failed: true,
                ..Stage::default()
            }
        );
        run(
            &mut t,
            "run_terminal_command",
            r#"{"command":"cargo test -q"}"#,
            false,
        );
        assert_eq!(
            t.take(),
            Stage {
                progress: true,
                ..Stage::default()
            }
        );
        // Lookups after an edit are no exploration, however many.
        for i in 0..16 {
            run(&mut t, "grep", &format!(r#"{{"pattern":"p{i}"}}"#), false);
        }
        assert!(!t.take().explored);
        // A test run that fails before any edit is a plain tool error, not a check failure.
        let mut fresh = StageTrack::default();
        run(&mut fresh, "run_terminal_command", test, true);
        assert_eq!(fresh.take(), Stage::default());
    }

    #[test]
    fn stage_signals_reach_the_next_call_and_a_new_turn_starts_empty() {
        let _ = take_stage(true);
        note_tool_run("read_file", r#"{"path":"a.rs"}"#, false);
        assert_eq!(
            take_stage(false),
            Stage {
                digest: true,
                ..Stage::default()
            }
        );
        note_out_of_budget();
        assert_eq!(
            take_stage(false),
            Stage {
                out_of_budget: true,
                ..Stage::default()
            }
        );
        note_tool_run("read_file", r#"{"path":"a.rs"}"#, true);
        assert_eq!(take_tool_errors(), 1, "a failed run is also E1");
        assert_eq!(
            take_stage(true),
            Stage::default(),
            "a new turn reads nothing"
        );
        // And forgot the turn: the same call is no loop now.
        note_tool_run("read_file", r#"{"path":"a.rs"}"#, false);
        assert!(!take_stage(false).looped);
    }
}
