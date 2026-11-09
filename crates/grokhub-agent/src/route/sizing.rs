//! Failover PR 5: how big a turn is, sized before it runs and re-sized as
//! it grows (card §3.3). OpenRouter's free allowance is small, so a long
//! task belongs on a paid route from the start (the plan first, local
//! after paid), not on a free model that runs out mid-task; a turn that
//! outgrows its class while running is upgraded to long so the rest of
//! its picks and failovers treat it as one. The other end of the scale:
//! less challenging work defaults to a local model that is already
//! loaded ([`warm_local_default`]).
//!
//! The estimator is pure: difficulty, the ask read, planned tools, plan
//! mode, context size and the class's learned output length go in, a
//! [`SizeEstimate`] comes out. The per-turn tracker is the one piece of
//! state: what a turn has already done (calls, output, time, and whether a
//! free or on-device model is carrying it) lives in a static map keyed by
//! episode and turn, the same pair the ladder's step count uses. Nothing
//! here is shown and nothing is a setting: the router just knows the size.
//!
//! The scorer that spends this in full (weights, the route ladder) is
//! [`super::score`]; PR 5 lands the estimate, the long-task rules in
//! [`super::Router::choose`] (no free provider models for a long task while
//! a carrier route exists, paid routes ranked first) and the mid-turn
//! upgrade state those rules read on the next call.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use super::log::route_records;

/// A long task expects more than this many tool steps.
pub const LONG_TOOL_STEPS: u32 = 8;
/// A long task expects more than this many output tokens.
pub const LONG_OUTPUT_TOKENS: u64 = 4_000;
/// A long task's context is over this many tokens.
pub const LONG_CTX_TOKENS: u64 = 64_000;
/// A session that has kept more than this many tokens makes its turns long.
pub const LONG_SESSION_TOKENS: u64 = 16_000;
/// Mid-turn: past this many tool steps on a free or on-device model, the
/// turn is upgraded to long.
pub const MID_TURN_TOOL_STEPS: u32 = 3;
/// Mid-turn: past this many ms on a free or on-device model, the turn is
/// upgraded to long.
pub const MID_TURN_MS: u64 = 90_000;
/// The learned output length is re-read from the spans at most this often.
const LEARNED_EVERY_MS: u64 = 60_000;
/// Spans scanned for the learned output lengths.
const LEARNED_SCAN_LINES: usize = 2_000;
/// A class needs at least this many finished calls before its learned
/// output length is trusted over the difficulty guess.
const LEARNED_MIN_SAMPLES: usize = 8;
/// Turns the tracker remembers; a full map starts over, like the ladder's.
const TURNS_CAP: usize = 512;

/// How big a turn is, smallest first: the order is the upgrade order, a
/// turn's size only ever grows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskSize {
    Tiny,
    #[default]
    Short,
    Medium,
    Long,
}

/// What a turn needs from its model. The scorer (a later step) filters on
/// these; PR 5 fills in what the call sites already know.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SizeNeeds {
    pub tools: bool,
    pub vision: bool,
    /// No call site reports audio yet; the field is the scorer's seam.
    pub audio: bool,
    pub long_context: bool,
    pub reasoning: bool,
}

/// The estimate: the size class plus the numbers it came from, so the
/// scorer can weigh them directly instead of re-deriving them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SizeEstimate {
    pub size: TaskSize,
    pub expected_tool_steps: u32,
    pub expected_output_tokens: u64,
    pub needs: SizeNeeds,
}

/// Everything the estimator looks at. Plain values, no I/O: the caller
/// (today, [`super::live::decide`]) gathers them.
#[derive(Debug, Clone, Copy, Default)]
pub struct SizeInput {
    /// The step's difficulty `d` in 0.0..=1.0 ([`super::difficulty`]).
    pub d: f64,
    /// Tools the caller already knows the step will offer or run.
    pub planned_tools: u32,
    pub plan_mode: bool,
    pub needs_tools: bool,
    pub needs_image: bool,
    /// The lane must pick a reasoning model ([`super::lane_needs_reasoning`]).
    pub reasoning: bool,
    pub ctx_tokens: u64,
    /// Tokens this session has kept across turns (its resent history), or 0
    /// for a turn that starts no session.
    pub session_kept_tokens: u64,
    /// This class's learned output length from past spans, when trusted.
    pub learned_output_tokens: Option<u64>,
}

/// Tool steps a step is expected to take. A planned task runs its plan;
/// each planned tool costs about two calls (run it, then read what it did),
/// and a hard ask iterates more. Tools merely offered may still take a
/// couple of rounds.
fn expected_tool_steps(input: &SizeInput) -> u32 {
    if input.plan_mode {
        return LONG_TOOL_STEPS + 2;
    }
    if input.planned_tools > 0 {
        let mut steps = input.planned_tools.saturating_mul(2).saturating_add(1);
        if input.d >= 0.7 {
            steps = steps.saturating_add(2);
        }
        return steps;
    }
    if input.needs_tools {
        return 2;
    }
    0
}

/// Output tokens a step is expected to write: what this class usually
/// writes (learned from its spans) when that is known, else a difficulty
/// guess — a hard ask writes a long answer, past the long line on purpose.
fn expected_output_tokens(input: &SizeInput) -> u64 {
    if let Some(learned) = input.learned_output_tokens {
        return learned;
    }
    if input.d >= 0.7 {
        4_500
    } else if input.d >= 0.4 {
        2_200
    } else if input.d >= 0.2 {
        900
    } else {
        400
    }
}

/// Size a turn before it runs.
pub fn estimate(input: &SizeInput) -> SizeEstimate {
    let steps = expected_tool_steps(input);
    let output = expected_output_tokens(input);
    let size = if steps > LONG_TOOL_STEPS
        || output > LONG_OUTPUT_TOKENS
        || input.ctx_tokens > LONG_CTX_TOKENS
        || input.session_kept_tokens > LONG_SESSION_TOKENS
    {
        TaskSize::Long
    } else if steps > MID_TURN_TOOL_STEPS
        || output > 1_500
        || input.ctx_tokens > LONG_SESSION_TOKENS
        || (input.d >= 0.6 && input.needs_tools)
    {
        TaskSize::Medium
    } else if steps == 0 && input.ctx_tokens <= 4_000 && output <= 512 && input.d < 0.3 {
        TaskSize::Tiny
    } else {
        TaskSize::Short
    };
    SizeEstimate {
        size,
        expected_tool_steps: steps,
        expected_output_tokens: output,
        needs: SizeNeeds {
            tools: input.needs_tools || input.planned_tools > 0 || input.plan_mode,
            vision: input.needs_image,
            audio: false,
            long_context: input.ctx_tokens > LONG_CTX_TOKENS,
            reasoning: input.reasoning,
        },
    }
}

/// Whether a turn of this size defaults to the on-device model when one is
/// already loaded (the warm-local rule, corrected 2026-11-07): less
/// challenging work goes to the warm model instead of paying for cloud or
/// spinning anything up. Under governor throttle — a game or another app
/// keeping the GPU busy — only tiny work defaults there; anything bigger
/// stays on the cloud rather than loading new local work. Medium and long
/// work never defaults to the device: long runs belong to a paid route
/// first, local only after paid.
pub fn warm_local_default(size: Option<TaskSize>, throttled: bool) -> bool {
    match size {
        Some(TaskSize::Tiny) => true,
        Some(TaskSize::Short) => !throttled,
        _ => false,
    }
}

/// One turn's sizing as it runs: the merged estimate, when it started, how
/// many calls it has taken, what it has written, and whether a free or
/// on-device model is carrying it.
#[derive(Debug, Clone, Copy)]
pub struct TurnSizer {
    pub estimate: SizeEstimate,
    pub started_ms: u64,
    pub calls: u32,
    pub output_tokens: u64,
    pub free_or_local: bool,
}

impl TurnSizer {
    pub fn new(estimate: SizeEstimate, now_ms: u64) -> Self {
        TurnSizer {
            estimate,
            started_ms: now_ms,
            calls: 0,
            output_tokens: 0,
            free_or_local: false,
        }
    }

    /// Tool steps so far: every call after the first exists because a tool
    /// ran (or a check sent the turn around again).
    pub fn tool_steps(&self) -> u32 {
        self.calls.saturating_sub(1)
    }

    /// Fold a fresh up-front estimate in: a size only ever grows.
    pub fn merge(&mut self, fresh: SizeEstimate) {
        self.estimate.size = self.estimate.size.max(fresh.size);
        self.estimate.expected_tool_steps = self
            .estimate
            .expected_tool_steps
            .max(fresh.expected_tool_steps);
        self.estimate.expected_output_tokens = self
            .estimate
            .expected_output_tokens
            .max(fresh.expected_output_tokens);
        self.estimate.needs.tools |= fresh.needs.tools;
        self.estimate.needs.vision |= fresh.needs.vision;
        self.estimate.needs.audio |= fresh.needs.audio;
        self.estimate.needs.long_context |= fresh.needs.long_context;
        self.estimate.needs.reasoning |= fresh.needs.reasoning;
    }

    /// What a finished call spent and where it ran.
    pub fn note_served(&mut self, output_tokens: u64, free_or_local: bool) {
        self.output_tokens = self.output_tokens.saturating_add(output_tokens);
        self.free_or_local = free_or_local;
    }

    /// Count this call and re-size the turn. Returns true when this call is
    /// the one that upgraded the turn to long.
    ///
    /// A turn crosses into long two ways. By definition: it has already
    /// written more than a long task writes, or its context has grown past
    /// the long line — on any route. By the card's mid-turn rule: carried by
    /// a free or on-device model, past [`MID_TURN_TOOL_STEPS`] tool steps or
    /// [`MID_TURN_MS`] in, it is upgraded so the rest of the turn moves to a
    /// route that can carry it (the hand-off itself is the failover step's).
    pub fn observe(&mut self, now_ms: u64, ctx_tokens: u64) -> bool {
        self.calls = self.calls.saturating_add(1);
        if self.estimate.size == TaskSize::Long {
            return false;
        }
        let by_definition = self.output_tokens > LONG_OUTPUT_TOKENS || ctx_tokens > LONG_CTX_TOKENS;
        let outgrew_its_route = self.free_or_local
            && (self.tool_steps() > MID_TURN_TOOL_STEPS
                || now_ms.saturating_sub(self.started_ms) > MID_TURN_MS);
        if by_definition || outgrew_its_route {
            self.estimate.size = TaskSize::Long;
            return true;
        }
        false
    }
}

static TURNS: Mutex<BTreeMap<String, (u64, TurnSizer)>> = Mutex::new(BTreeMap::new());

/// The turn's effective estimate at this call: the fresh up-front estimate
/// merged with everything the turn has already done, upgraded to long when
/// it has outgrown its class. `upgraded` is true only on the call where the
/// upgrade lands.
pub fn turn_estimate(
    key: &str,
    turn: u64,
    fresh: SizeEstimate,
    ctx_tokens: u64,
    now_ms: u64,
) -> (SizeEstimate, bool) {
    let mut turns = TURNS.lock().unwrap_or_else(|e| e.into_inner());
    if turns.len() >= TURNS_CAP && !turns.contains_key(key) {
        turns.clear();
    }
    let slot = turns
        .entry(key.to_string())
        .or_insert_with(|| (turn, TurnSizer::new(fresh, now_ms)));
    if slot.0 != turn {
        *slot = (turn, TurnSizer::new(fresh, now_ms));
    }
    slot.1.merge(fresh);
    let upgraded = slot.1.observe(now_ms, ctx_tokens);
    (slot.1.estimate, upgraded)
}

/// Each chat's latest turn and its size, for the spawn policy: a helper's
/// caps follow the size of the turn that starts it.
static LATEST: Mutex<BTreeMap<String, (u64, TaskSize)>> = Mutex::new(BTreeMap::new());

/// The router sized `session`'s turn `turn` as `size`.
pub fn note_latest(session: &str, turn: u64, size: TaskSize) {
    if session.is_empty() {
        return;
    }
    let mut latest = LATEST.lock().unwrap_or_else(|e| e.into_inner());
    if latest.len() >= TURNS_CAP && !latest.contains_key(session) {
        latest.clear();
    }
    latest.insert(session.to_string(), (turn, size));
}

/// `session`'s latest turn and its size, if the router has sized one.
pub fn latest(session: &str) -> Option<(u64, TaskSize)> {
    LATEST
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
        .copied()
}

/// Record what a finished call spent and where it ran, for the turn's next
/// sizing. Unknown or finished turns are ignored.
pub fn note_served(key: &str, turn: u64, output_tokens: u64, free_or_local: bool) {
    let mut turns = TURNS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = turns.get_mut(key) {
        if slot.0 == turn {
            slot.1.note_served(output_tokens, free_or_local);
        }
    }
}

static LEARNED: Mutex<Option<(u64, BTreeMap<String, u64>)>> = Mutex::new(None);

/// A class's learned output length: the third-quartile output of its
/// finished calls in the recent spans, or `None` until at least
/// [`LEARNED_MIN_SAMPLES`] of them are on record. Cached for
/// [`LEARNED_EVERY_MS`], like the week's budget scan.
pub fn learned_output_tokens(config_dir: &Path, class: &str, now_ms: u64) -> Option<u64> {
    {
        let cache = LEARNED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((scanned, map)) = cache.as_ref() {
            if now_ms.saturating_sub(*scanned) < LEARNED_EVERY_MS {
                return map.get(class).copied();
            }
        }
    }
    let mut by_class: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for (_, rec) in route_records(config_dir, LEARNED_SCAN_LINES) {
        if let Some(outcome) = rec.outcome {
            if outcome.ok && outcome.tokens.out > 0 {
                by_class
                    .entry(rec.class.clone())
                    .or_default()
                    .push(outcome.tokens.out);
            }
        }
    }
    let mut map = BTreeMap::new();
    for (class, mut outs) in by_class {
        if outs.len() >= LEARNED_MIN_SAMPLES {
            outs.sort_unstable();
            map.insert(class, outs[outs.len() * 3 / 4]);
        }
    }
    let found = map.get(class).copied();
    *LEARNED.lock().unwrap_or_else(|e| e.into_inner()) = Some((now_ms, map));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> SizeInput {
        SizeInput::default()
    }

    #[test]
    fn a_plain_short_ask_is_tiny_or_short() {
        let est = estimate(&input());
        assert_eq!(est.expected_tool_steps, 0);
        assert_eq!(est.expected_output_tokens, 400);
        assert_eq!(est.size, TaskSize::Tiny);
        let mut i = input();
        i.d = 0.3;
        assert_eq!(estimate(&i).size, TaskSize::Short);
        assert_eq!(estimate(&i).expected_output_tokens, 900);
        // A mid-difficulty ask writes a medium-length answer.
        i.d = 0.5;
        assert_eq!(estimate(&i).size, TaskSize::Medium);
        assert_eq!(estimate(&i).expected_output_tokens, 2_200);
    }

    #[test]
    fn plan_mode_is_long_by_steps() {
        let mut i = input();
        i.plan_mode = true;
        i.needs_tools = true;
        let est = estimate(&i);
        assert!(est.expected_tool_steps > LONG_TOOL_STEPS);
        assert_eq!(est.size, TaskSize::Long);
        assert!(est.needs.tools);
    }

    #[test]
    fn many_planned_tools_are_long_and_few_are_medium() {
        let mut i = input();
        i.planned_tools = 4;
        assert_eq!(estimate(&i).expected_tool_steps, 9);
        assert_eq!(estimate(&i).size, TaskSize::Long);
        i.planned_tools = 1;
        assert_eq!(estimate(&i).expected_tool_steps, 3);
        assert_eq!(estimate(&i).size, TaskSize::Short);
        i.planned_tools = 2;
        assert_eq!(estimate(&i).size, TaskSize::Medium);
    }

    #[test]
    fn a_hard_ask_is_long_by_expected_output() {
        let mut i = input();
        i.d = 0.7;
        let est = estimate(&i);
        assert_eq!(est.expected_output_tokens, 4_500);
        assert_eq!(est.size, TaskSize::Long);
    }

    #[test]
    fn context_and_session_lines_make_a_turn_long() {
        let mut i = input();
        i.ctx_tokens = LONG_CTX_TOKENS + 1;
        let est = estimate(&i);
        assert_eq!(est.size, TaskSize::Long);
        assert!(est.needs.long_context);
        let mut i = input();
        i.session_kept_tokens = LONG_SESSION_TOKENS + 1;
        assert_eq!(estimate(&i).size, TaskSize::Long);
        let mut i = input();
        i.ctx_tokens = LONG_CTX_TOKENS;
        assert_eq!(estimate(&i).size, TaskSize::Medium);
    }

    #[test]
    fn learned_output_beats_the_difficulty_guess() {
        let mut i = input();
        i.learned_output_tokens = Some(5_000);
        assert_eq!(estimate(&i).size, TaskSize::Long);
        i.learned_output_tokens = Some(300);
        i.d = 0.9;
        let est = estimate(&i);
        assert_eq!(est.expected_output_tokens, 300);
        // The output is tiny, but a hard ask is never a tiny task.
        assert_eq!(est.size, TaskSize::Short);
    }

    #[test]
    fn warm_local_takes_less_challenging_work_only() {
        assert!(warm_local_default(Some(TaskSize::Tiny), false));
        assert!(warm_local_default(Some(TaskSize::Tiny), true));
        assert!(warm_local_default(Some(TaskSize::Short), false));
        // Throttled (a game, a busy GPU): short work stays off the device.
        assert!(!warm_local_default(Some(TaskSize::Short), true));
        assert!(!warm_local_default(Some(TaskSize::Medium), false));
        assert!(!warm_local_default(Some(TaskSize::Long), false));
        assert!(!warm_local_default(None, false));
    }

    #[test]
    fn needs_come_from_the_call() {
        let mut i = input();
        i.needs_image = true;
        i.reasoning = true;
        let est = estimate(&i);
        assert!(est.needs.vision);
        assert!(est.needs.reasoning);
        assert!(!est.needs.tools);
        assert!(!est.needs.audio);
    }

    #[test]
    fn mid_turn_upgrade_past_three_tool_steps_on_a_free_route() {
        let mut s = TurnSizer::new(estimate(&input()), 1_000);
        s.note_served(100, true);
        // Calls 1-4: 0..3 tool steps, no upgrade yet.
        for n in 0..4 {
            assert!(!s.observe(1_000 + n, 0), "call {n}");
        }
        // The fifth call is past 3 tool steps on a free model: upgrade.
        assert!(s.observe(2_000, 0));
        assert_eq!(s.estimate.size, TaskSize::Long);
        // And it never fires twice.
        assert!(!s.observe(3_000, 0));
    }

    #[test]
    fn mid_turn_upgrade_after_ninety_seconds_on_a_local_route() {
        let mut s = TurnSizer::new(estimate(&input()), 10_000);
        s.note_served(50, true);
        assert!(!s.observe(10_000 + MID_TURN_MS, 0));
        assert!(s.observe(10_000 + MID_TURN_MS + 1, 0));
    }

    #[test]
    fn a_cloud_route_grows_without_the_mid_turn_upgrade() {
        let mut s = TurnSizer::new(estimate(&input()), 0);
        s.note_served(100, false);
        for n in 0..10 {
            assert!(!s.observe(200_000 + n, 0));
        }
        assert_eq!(s.estimate.size, TaskSize::Tiny);
    }

    #[test]
    fn output_and_context_crossings_upgrade_on_any_route() {
        let mut s = TurnSizer::new(estimate(&input()), 0);
        s.note_served(LONG_OUTPUT_TOKENS + 1, false);
        assert!(s.observe(1, 0));
        let mut s = TurnSizer::new(estimate(&input()), 0);
        assert!(s.observe(1, LONG_CTX_TOKENS + 1));
    }

    #[test]
    fn sizes_only_grow_across_a_turn() {
        let mut s = TurnSizer::new(estimate(&input()), 0);
        let mut hard = input();
        hard.d = 0.9;
        s.merge(estimate(&hard));
        assert_eq!(s.estimate.size, TaskSize::Long);
        s.merge(estimate(&input()));
        assert_eq!(s.estimate.size, TaskSize::Long);
        assert_eq!(s.estimate.expected_output_tokens, 4_500);
    }

    #[test]
    fn turn_estimate_tracks_and_resets_per_turn() {
        let key = "sizing-test-episode\u{1f}chat:default";
        let fresh = estimate(&input());
        let (est, up) = turn_estimate(key, 7, fresh, 0, 1_000);
        assert_eq!(est.size, TaskSize::Tiny);
        assert!(!up);
        note_served(key, 7, LONG_OUTPUT_TOKENS + 1, false);
        let (est, up) = turn_estimate(key, 7, fresh, 0, 2_000);
        assert_eq!(est.size, TaskSize::Long);
        assert!(up);
        // A new turn starts over.
        let (est, up) = turn_estimate(key, 8, fresh, 0, 3_000);
        assert_eq!(est.size, TaskSize::Tiny);
        assert!(!up);
        // A stale turn's served note is ignored.
        note_served(key, 7, LONG_OUTPUT_TOKENS + 1, false);
        let (est, _) = turn_estimate(key, 8, fresh, 0, 4_000);
        assert_eq!(est.size, TaskSize::Tiny);
    }
}
