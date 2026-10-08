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
    EFFORT_LADDER.iter().position(|l| l.eq_ignore_ascii_case(e.trim()))
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

const HARDER: &[&str] = &["think hard", "think harder", "think deeply", "think really hard", "think it through", "take your time"];
const QUICKER: &[&str] = &["keep it quick", "be quick", "quick answer", "keep it short", "just quickly"];

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
        Self { floor: up(rung(row.floor).unwrap_or(0)), start: up(rung(row.start).unwrap_or(0)), ceiling }
    }

    /// [`Band::of`] with a self-tuned start (R3a) merged over the class table,
    /// clamped inside the floor and ceiling. Floors and ceilings are never
    /// tuned, and `prepare:hard` never starts lower than its row says.
    pub fn tuned(row: &ClassRow, bump: u8, start: Option<&str>) -> Self {
        let mut b = Self::of(row, bump);
        if let Some(r) = start.and_then(rung) {
            let r = r.clamp(b.floor, b.ceiling.max(b.floor));
            b.start = if row.class == PREPARE_HARD { r.max(b.start) } else { r };
        }
        b
    }
}

/// Where a step starts: the class start, moved by the user's words or by difficulty.
/// xhigh and max come only from escalation or an explicit "think hard".
pub fn start_rung(band: Band, d: f64, steer: Steer, user: bool) -> (usize, Vec<String>) {
    if !user {
        return (band.start, Vec::new());
    }
    match steer {
        Steer::Harder => return (band.ceiling.max(band.start), vec!["steer:harder".into()]),
        Steer::Quicker => return (band.floor, vec!["steer:quicker".into()]),
        Steer::None => {}
    }
    if d < 0.3 && band.start > band.floor {
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
}

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
    let since = now_ms.saturating_sub(ROUTINE_WINDOW_MS).max(r.last_failure.map(|f| f + 1).unwrap_or(0));
    (band.floor..band.start)
        .find(|rung| r.successes.iter().filter(|(x, at)| x == rung && *at >= since && *at <= now_ms).count() >= ROUTINE_MIN_SUCCESSES)
}

/// Advance one episode's state for the next call. Pure: the caller keeps the state.
pub fn advance(prev: Option<&EffortState>, class: &str, band: Band, start: (usize, Vec<String>), obs: &Obs) -> (EffortState, Pick) {
    let hard = class.trim() == PREPARE_HARD;
    let user = user_facing(class);
    let fresh = prev.is_none() || obs.new_turn;
    let mut rules: Vec<String> = Vec::new();
    let mut st = match prev {
        Some(p) if !fresh => p.clone(),
        // A new turn starts over at the class start, but a reject's hold carries over.
        Some(p) => EffortState { rung: start.0, rejects: p.rejects, hold: p.hold, ..EffortState::default() },
        None => EffortState { rung: start.0, ..EffortState::default() },
    };
    st.moved = Move::Start;
    if fresh {
        rules.extend(start.1);
    }
    let up = |st: &mut EffortState, rules: &mut Vec<String>, id: &str| {
        st.clean = 0;
        if st.rung < band.ceiling {
            st.rung += 1;
            st.moved = Move::Up;
            rules.push(id.to_string());
        } else {
            rules.push(format!("{id}:ceiling"));
        }
    };
    let steered = rules.iter().any(|r| r.starts_with("steer:"));
    if let Some(r) = obs.routine.filter(|r| fresh && user && !hard && !st.hold && !steered && *r < st.rung) {
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
            if obs.tool_errors > 0 {
                up(&mut st, &mut rules, "E1");
            } else if st.last_ok {
                st.clean += 1;
            } else {
                st.clean = 0;
            }
        }
        if let Some(n) = obs.rejects.filter(|n| *n > st.rejects) {
            st.rejects = n;
            st.hold = true;
            let at_ceiling = st.rung >= band.ceiling;
            up(&mut st, &mut rules, "E2");
            if n == 2 && at_ceiling {
                // R2 moves to the next-stronger model here.
                rules.push("E2:next_model".into());
            }
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
    if user && !hard && !st.hold && st.clean >= 3 && st.rung > band.floor {
        st.rung -= 1;
        st.clean = 0;
        st.moved = Move::Down;
        rules.push("DE1".into());
    }
    if !hard && obs.budget_pct.is_some_and(|p| p >= 80) {
        // Below a floor would need an ask card, so DE3 stops at the floor.
        let target = if user { band.start } else { rung("low").unwrap_or(0) }.max(band.floor);
        if target < st.rung {
            st.rung = target;
            st.moved = Move::Down;
            rules.push("DE3".into());
        }
    }
    st.rung = st.rung.clamp(band.floor, band.ceiling.max(band.floor));
    let pick = Pick { rung: st.rung, moved: st.moved, rules, recover };
    (st, pick)
}

/// Clamp a rung to a model's effort list. Going up rounds up to the next
/// offered level; going down (or starting) rounds down. Never below `floor`
/// when the list offers something at or above it. `None` when the list is empty.
pub fn clamp(r: usize, menu: &[String], moved: Move, floor: usize) -> Option<String> {
    let mut offered: Vec<(usize, &String)> = menu.iter().filter_map(|m| rung(m).map(|x| (x, m))).collect();
    offered.sort_by_key(|(x, _)| *x);
    let up = || offered.iter().find(|(x, _)| *x >= r).or(offered.last());
    let down = || offered.iter().rev().find(|(x, _)| *x <= r).or(offered.first());
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

fn key(episode: &str, class: &str) -> String {
    format!("{episode}\u{1f}{class}")
}

/// FNV-1a of the user's newest text: tells a new turn from a tool step without keeping the text.
pub fn turn_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Advance the saved state for `episode` and `class`.
pub fn step(episode: &str, class: &str, turn: u64, band: Band, start: (usize, Vec<String>), mut obs: Obs) -> Pick {
    let k = key(episode, class);
    let mut states = STATES.lock().unwrap_or_else(|e| e.into_inner());
    let prev = states.get(&k).cloned();
    obs.new_turn = prev.as_ref().is_none_or(|p| p.turn != turn);
    let (mut st, pick) = advance(prev.as_ref(), class, band, start, &obs);
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
    if let Some(st) = STATES.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&key(episode, class)) {
        st.last_ok = ok;
    }
}

/// This call starts a new turn for `episode` in `class` (or its first call).
pub fn is_new_turn(episode: &str, class: &str, turn: u64) -> bool {
    STATES.lock().unwrap_or_else(|e| e.into_inner()).get(&key(episode, class)).is_none_or(|s| s.turn != turn)
}

/// The rung this episode is on now, if it has routed a call in `class`.
pub fn current(episode: &str, class: &str) -> Option<usize> {
    STATES.lock().unwrap_or_else(|e| e.into_inner()).get(&key(episode, class)).map(|s| s.rung)
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
        advance(prev, class, b, start_rung(b, 0.5, Steer::None, user_facing(class)), &obs)
    }

    fn step_obs() -> Obs {
        Obs { new_turn: false, ..Obs::default() }
    }

    #[test]
    fn steer_reads_plain_words() {
        assert_eq!(Steer::from_text("Please think hard on this one"), Steer::Harder);
        assert_eq!(Steer::from_text("keep it quick, just the name"), Steer::Quicker);
        assert_eq!(Steer::from_text("rename the file"), Steer::None);
    }

    #[test]
    fn start_follows_d_and_steer_and_never_starts_above_high_on_d_alone() {
        let chat = band("chat:default");
        assert_eq!(start_rung(chat, 0.5, Steer::None, true), (rung("medium").unwrap(), vec![]));
        assert_eq!(name(start_rung(chat, 0.1, Steer::None, true).0), "low");
        assert_eq!(name(start_rung(chat, 0.9, Steer::None, true).0), "high");
        assert_eq!(name(start_rung(band("plan"), 0.9, Steer::None, true).0), "high", "d alone never reaches xhigh");
        assert_eq!(name(start_rung(chat, 0.0, Steer::Harder, true).0), "xhigh");
        assert_eq!(name(start_rung(chat, 0.9, Steer::Quicker, true).0), "low");
        // Background runs at its start whatever d says.
        assert_eq!(name(start_rung(band("background:summarize"), 0.0, Steer::None, false).0), "low");
    }

    #[test]
    fn e1_tool_error_climbs_one_rung() {
        let (s0, p0) = go(None, "chat:default", Obs { new_turn: true, ..Obs::default() });
        assert_eq!(name(p0.rung), "medium");
        let (_, p1) = go(Some(&s0), "chat:default", Obs { tool_errors: 3, ..step_obs() });
        assert_eq!((name(p1.rung), p1.moved, p1.rules.clone()), ("high", Move::Up, vec!["E1".to_string()]));
    }

    #[test]
    fn e2_rejects_climb_then_name_the_next_model_then_hand_to_recovery() {
        let (s0, _) = go(None, "code:edit-small", Obs { new_turn: true, ..Obs::default() });
        let (s1, p1) = go(Some(&s0), "code:edit-small", Obs { rejects: Some(1), ..step_obs() });
        assert_eq!((name(p1.rung), p1.rules.clone(), s1.hold), ("high", vec!["E2".to_string()], true));
        let (mut s2, p2) = go(Some(&s1), "code:edit-small", Obs { rejects: Some(2), ..step_obs() });
        assert_eq!((name(p2.rung), p2.recover), ("xhigh", false));
        // At the ceiling a second reject asks for the next model (R2).
        s2.rejects = 1;
        let (s3, p3) = go(Some(&s2), "code:edit-small", Obs { rejects: Some(2), ..step_obs() });
        assert_eq!(p3.rules, vec!["E2:ceiling".to_string(), "E2:next_model".into()]);
        assert_eq!(name(p3.rung), "xhigh", "the ceiling holds");
        let (_, p4) = go(Some(&s3), "code:edit-small", Obs { rejects: Some(3), ..step_obs() });
        assert!(p4.recover && p4.rules.contains(&"E2:recovery".to_string()), "{:?}", p4.rules);
        // No VerifyGate source (None) fires nothing.
        let (_, quiet) = go(Some(&s0), "code:edit-small", Obs { rejects: None, ..step_obs() });
        assert!(!quiet.rules.iter().any(|r| r.starts_with("E2")));
    }

    #[test]
    fn e3_correction_starts_the_next_turn_one_higher() {
        let (s0, _) = go(None, "chat:default", Obs { new_turn: true, ..Obs::default() });
        let (_, p) = go(Some(&s0), "chat:default", Obs { new_turn: true, correction: true, ..Obs::default() });
        assert_eq!((name(p.rung), p.rules.clone()), ("high", vec!["E3".to_string()]));
    }

    #[test]
    fn e4_low_self_check_climbs_and_none_does_nothing() {
        let (s0, _) = go(None, "chat:default", Obs { new_turn: true, ..Obs::default() });
        let (_, p) = go(Some(&s0), "chat:default", Obs { low_check: Some(true), ..step_obs() });
        assert_eq!((name(p.rung), p.rules.clone()), ("high", vec!["E4".to_string()]));
        let (_, none) = go(Some(&s0), "chat:default", Obs { low_check: None, ..step_obs() });
        assert_eq!(name(none.rung), "medium");
    }

    #[test]
    fn e5_prepare_hard_starts_at_high_even_when_asked_to_be_quick() {
        let b = band(PREPARE_HARD);
        let (_, p) = advance(None, PREPARE_HARD, b, start_rung(b, 0.0, Steer::Quicker, true), &Obs { new_turn: true, ..Obs::default() });
        assert_eq!(name(p.rung), "high");
        // Even a band whose floor sat lower gets lifted to High by E5.
        let low = Band { floor: rung("low").unwrap(), start: rung("low").unwrap(), ceiling: rung("xhigh").unwrap() };
        let (_, p) = advance(None, PREPARE_HARD, low, (low.start, vec![]), &Obs { new_turn: true, ..Obs::default() });
        assert_eq!((name(p.rung), p.rules.clone()), ("high", vec!["E5".to_string()]));
    }

    #[test]
    fn de1_three_clean_steps_step_down_to_the_floor_but_not_after_a_reject_or_in_prepare_hard() {
        let mut s = go(None, "chat:default", Obs { new_turn: true, ..Obs::default() }).0;
        let mut rungs = Vec::new();
        for _ in 0..7 {
            s.last_ok = true;
            let (n, p) = go(Some(&s), "chat:default", step_obs());
            rungs.push(name(p.rung));
            s = n;
        }
        assert_eq!(rungs, vec!["medium", "medium", "low", "low", "low", "low", "low"], "DE1 stops at the floor");
        // After a reject: hold until VERIFY_OK.
        let mut s = go(None, "chat:default", Obs { new_turn: true, ..Obs::default() }).0;
        s = go(Some(&s), "chat:default", Obs { rejects: Some(1), ..step_obs() }).0;
        for _ in 0..4 {
            s.last_ok = true;
            s = go(Some(&s), "chat:default", step_obs()).0;
        }
        assert_eq!(name(s.rung), "high", "held after the reject");
        s.last_ok = true;
        let (s2, p) = go(Some(&s), "chat:default", Obs { verify_ok: true, ..step_obs() });
        assert_eq!((name(p.rung), p.rules.clone()), ("medium", vec!["DE1".to_string()]));
        assert!(!s2.hold);
        // prepare:hard never steps down.
        let mut s = go(None, PREPARE_HARD, Obs { new_turn: true, ..Obs::default() }).0;
        s = go(Some(&s), PREPARE_HARD, Obs { tool_errors: 1, ..step_obs() }).0;
        assert_eq!(name(s.rung), "xhigh");
        for _ in 0..5 {
            s.last_ok = true;
            s = go(Some(&s), PREPARE_HARD, step_obs()).0;
        }
        assert_eq!(name(s.rung), "xhigh");
    }

    #[test]
    fn de3_budget_pressure_lowers_background_to_low_and_user_work_to_start_never_below_the_floor() {
        let mut s = go(None, "chat:default", Obs { new_turn: true, ..Obs::default() }).0;
        s = go(Some(&s), "chat:default", Obs { tool_errors: 1, ..step_obs() }).0;
        s = go(Some(&s), "chat:default", Obs { tool_errors: 1, ..step_obs() }).0;
        assert_eq!(name(s.rung), "xhigh");
        let (_, p) = go(Some(&s), "chat:default", Obs { budget_pct: Some(85), ..step_obs() });
        assert_eq!((name(p.rung), p.rules.clone()), ("medium", vec!["DE3".to_string()]));
        let (_, p) = go(Some(&s), "chat:default", Obs { budget_pct: Some(79), ..step_obs() });
        assert_eq!(name(p.rung), "xhigh");
        let judge = EffortState { rung: rung("high").unwrap(), ..EffortState::default() };
        let (_, p) = go(Some(&judge), "background:judge", Obs { budget_pct: Some(90), ..step_obs() });
        assert_eq!((name(p.rung), p.rules.clone()), ("low", vec!["DE3".to_string()]));
        let hard = EffortState { rung: rung("xhigh").unwrap(), ..EffortState::default() };
        let (_, p) = go(Some(&hard), PREPARE_HARD, Obs { budget_pct: Some(99), ..step_obs() });
        assert_eq!(name(p.rung), "xhigh", "prepare:hard ignores DE3");
    }

    const DAY: u64 = 24 * 60 * 60 * 1000;

    fn wins(rung_name: &str, days_ago: &[u64], now: u64) -> Routine {
        Routine { successes: days_ago.iter().map(|d| (rung(rung_name).unwrap(), now - d * DAY)).collect(), last_failure: None }
    }

    #[test]
    fn de2_three_low_successes_in_14_days_start_the_next_run_at_low() {
        let now = 100 * DAY;
        let b = band("chat:default");
        let r = routine_rung("chat:default", &wins("low", &[1, 5, 13], now), b, now);
        assert_eq!(r, Some(rung("low").unwrap()));
        let (_, p) = go(None, "chat:default", Obs { new_turn: true, routine: r, ..Obs::default() });
        assert_eq!((name(p.rung), p.moved, p.rules.clone()), ("low", Move::Down, vec!["DE2".to_string()]));
        // Two successes, or three spread over 20 days, are not a routine.
        assert_eq!(routine_rung("chat:default", &wins("low", &[1, 5], now), b, now), None);
        assert_eq!(routine_rung("chat:default", &wins("low", &[1, 10, 20], now), b, now), None);
        // A failure after them resets the count.
        let mut failed = wins("low", &[3, 5, 6], now);
        failed.last_failure = Some(now - 2 * DAY);
        assert_eq!(routine_rung("chat:default", &failed, b, now), None);
        // Successes at the start rung or above lower nothing.
        assert_eq!(routine_rung("chat:default", &wins("medium", &[1, 2, 3], now), b, now), None);
    }

    #[test]
    fn de2_never_lowers_prepare_hard_never_goes_below_the_floor_and_waits_out_a_reject() {
        let now = 100 * DAY;
        let hard = band(PREPARE_HARD);
        assert_eq!(routine_rung(PREPARE_HARD, &wins("low", &[1, 2, 3], now), hard, now), None);
        // Even handed a low rung, advance keeps prepare:hard at High.
        let (_, p) = advance(None, PREPARE_HARD, hard, (hard.start, vec![]), &Obs { new_turn: true, routine: Some(rung("low").unwrap()), ..Obs::default() });
        assert_eq!(name(p.rung), "high");
        assert!(!p.rules.contains(&"DE2".to_string()));
        // code:multi-file's floor is medium: successes at low don't count below it.
        let multi = band("code:multi-file");
        assert_eq!(routine_rung("code:multi-file", &wins("low", &[1, 2, 3], now), multi, now), None);
        assert_eq!(routine_rung("code:multi-file", &wins("medium", &[1, 2, 3], now), multi, now), Some(rung("medium").unwrap()));
        // After a reject, the next turn keeps its hold: no DE2 until VERIFY_OK.
        let mut s = go(None, "chat:default", Obs { new_turn: true, ..Obs::default() }).0;
        s = go(Some(&s), "chat:default", Obs { rejects: Some(1), ..step_obs() }).0;
        let low = Some(rung("low").unwrap());
        let (_, p) = go(Some(&s), "chat:default", Obs { new_turn: true, routine: low, ..Obs::default() });
        assert_eq!(name(p.rung), "medium");
        // "think hard" wins over a routine.
        let b = band("chat:default");
        let (_, p) = advance(None, "chat:default", b, start_rung(b, 0.5, Steer::Harder, true), &Obs { new_turn: true, routine: low, ..Obs::default() });
        assert_eq!(name(p.rung), "xhigh");
    }

    #[test]
    fn background_classes_never_escalate() {
        let (s, p) = go(None, "background:compact", Obs { new_turn: true, ..Obs::default() });
        assert_eq!(name(p.rung), "low");
        let (_, p) = go(Some(&s), "background:compact", Obs { tool_errors: 4, rejects: Some(2), correction: true, ..step_obs() });
        assert_eq!((name(p.rung), p.rules.len()), ("low", 0));
    }

    #[test]
    fn clamp_rounds_up_going_up_and_down_otherwise_and_keeps_the_floor() {
        let menu: Vec<String> = ["low", "high"].iter().map(|s| s.to_string()).collect();
        let medium = rung("medium").unwrap();
        assert_eq!(clamp(medium, &menu, Move::Up, 0).as_deref(), Some("high"));
        assert_eq!(clamp(medium, &menu, Move::Down, 0).as_deref(), Some("low"));
        assert_eq!(clamp(medium, &menu, Move::Start, 0).as_deref(), Some("low"));
        assert_eq!(clamp(rung("max").unwrap(), &menu, Move::Up, 0).as_deref(), Some("high"));
        assert_eq!(clamp(rung("none").unwrap(), &menu, Move::Down, 0).as_deref(), Some("low"));
        // A floor of high is kept even when rounding down would drop below it.
        let menu2: Vec<String> = ["low", "medium", "xhigh"].iter().map(|s| s.to_string()).collect();
        assert_eq!(clamp(high(), &menu2, Move::Start, high()).as_deref(), Some("xhigh"));
        assert_eq!(clamp(medium, &[], Move::Up, 0), None);
    }

    #[test]
    fn saved_state_tells_turns_apart_and_finish_counts_clean_steps() {
        let b = band("chat:default");
        let ep = "ladder-state-test";
        let t1 = turn_hash("first ask");
        let p = step(ep, "chat:default", t1, b, start_rung(b, 0.5, Steer::Harder, true), Obs::default());
        assert_eq!(name(p.rung), "xhigh");
        finish(ep, "chat:default", true);
        let p = step(ep, "chat:default", t1, b, (b.start, vec![]), Obs::default());
        assert_eq!(name(p.rung), "xhigh", "the same turn keeps its steer");
        assert_eq!(current(ep, "chat:default"), Some(rung("xhigh").unwrap()));
        let p = step(ep, "chat:default", turn_hash("next ask"), b, (b.start, vec![]), Obs::default());
        assert_eq!(name(p.rung), "medium", "a new turn drops the steer");
        note_tool_error();
        note_tool_error();
        assert_eq!((take_tool_errors(), take_tool_errors()), (2, 0));
    }
}
