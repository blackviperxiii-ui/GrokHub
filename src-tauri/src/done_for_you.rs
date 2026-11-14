//! Done for you: GrokHub pauses an automation that keeps failing, on its own,
//! behind one switch in Settings → Pulse (`pauseFailingAutomations` in
//! `app.json`, off by default).
//!
//! One candidate per proactive pass: the enabled job with the longest run of
//! failures, at least [`PAUSE_AFTER`] in a row. It goes through the autonomy
//! ceiling ([`AutoAct::admit`]), then the harness gate and the native
//! `automation_disable` (`hx::run_auto_act`), which writes a change-ledger
//! line. The Done card's Undo is that line. A ceiling miss writes one `ask`
//! span, and the failing job keeps its normal Pulse card, whose Pause it
//! chip runs the same step on a tap ([`pause_from_card`]).
//!
//! It only changes one of the app's own settings, never the computer, and it never builds
//! a hard-class step: the ceiling and the gate both refuse one.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use grokhub_agent::harness::{self as hx, AccessMode, ChangeKind};
use grokhub_core::card_report::{add_receipt, CardReceipt};
use grokhub_core::proactive_auto::{AccessTier, AutoBudget, CeilingCtx, CeilingMiss, PillMode};
use grokhub_core::update_feed::DoneForYou;
use grokhub_core::{AutoAct, AutoCandidate, Automation, UpdateCard};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// The switch's `app.json` key.
pub const SWITCH_KEY: &str = "pauseFailingAutomations";
/// Failed runs in a row before a job can be paused on its own.
pub const PAUSE_AFTER: u32 = 5;
/// The day's budget and the last ask, beside `proactive.json`.
pub const STATE_FILE: &str = "done_for_you.json";
/// The step every pause runs.
pub const TOOL: &str = "automation_disable";
/// The ledger kind a pause writes, as cards store it.
const KIND: &str = "automation";
/// The candidate's scope: the switch itself.
const SCOPE: &str = SWITCH_KEY;
/// The state file is two small fields; anything bigger is not one.
const STATE_CAP: u64 = 16 * 1024;

pub const SWITCH_LABEL: &str = "Pause an automation that keeps failing";
/// The switch's note in Settings → Pulse.
pub fn switch_note() -> String {
    format!(
        "Changes a {} setting, not your computer. Only after {PAUSE_AFTER} failed runs in a row, only when \"When the AI wants to change things\" isn't \"Ask me first\", and only after you've tapped Pause it once yourself. A card says what it did, with Undo.",
        grokhub_core::brand::product_name()
    )
}
/// The receipt a Pause it tap leaves on the card.
pub const PAUSED_RECEIPT: &str = "Paused it. Undo is in Settings > Behavior.";
pub const ALREADY_ANSWERED: &str = "You already answered this card.";
const GONE_CARD: &str = "That card is gone.";
const NOT_PAUSABLE: &str = "This card can't pause anything.";

/// One read-change-write of the state file at a time.
static STATE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct State {
    budget: AutoBudget,
    /// The last miss an `ask` span was written for: `[automation id, miss]`.
    last_ask: Option<(String, String)>,
}

fn state_path(config_dir: &Path) -> PathBuf {
    config_dir.join(STATE_FILE)
}

fn load_state(config_dir: &Path) -> State {
    let path = state_path(config_dir);
    if std::fs::metadata(&path).map_or(true, |m| m.len() > STATE_CAP) {
        return State::default();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_state(config_dir: &Path, state: &State) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string(state).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{STATE_FILE}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, state_path(config_dir)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

fn change_state<T>(config_dir: &Path, f: impl FnOnce(&mut State) -> T) -> T {
    let _lock = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load_state(config_dir);
    let out = f(&mut state);
    if let Err(e) = save_state(config_dir, &state) {
        eprintln!("done for you: {e}");
    }
    out
}

pub fn on(config_dir: &Path) -> bool {
    grokhub_core::app_json::bool_or(config_dir, SWITCH_KEY, false)
}

pub fn set_on(config_dir: &Path, on: bool) -> Result<(), String> {
    grokhub_core::app_json::patch(config_dir, &[(SWITCH_KEY, json!(on))])
}

/// The job a pass may pause, if any: enabled, failing at least
/// [`PAUSE_AFTER`] times in a row, an id the ledger can name as written, and
/// an error no other failing job shares (a route or key error fails every
/// job, and pausing one would not help). Nothing while the model route itself
/// is down. The longest streak wins, ties by id.
pub fn pausable(list: &[Automation], route_ok: bool) -> Option<&Automation> {
    if !route_ok {
        return None;
    }
    let shared = |a: &Automation| {
        let error = a.health.error.trim();
        list.iter()
            .any(|b| b.id != a.id && b.health.failing() && b.health.error.trim() == error)
    };
    list.iter()
        .filter(|a| a.enabled && a.health.failing() && a.health.fail_streak >= PAUSE_AFTER)
        .filter(|a| hx::entry_id(&a.id).is_ok_and(|id| id == a.id))
        .filter(|a| !shared(a))
        .min_by(|a, b| {
            b.health
                .fail_streak
                .cmp(&a.health.fail_streak)
                .then_with(|| a.id.cmp(&b.id))
        })
}

fn step_args(a: &Automation) -> String {
    json!({
        "id": a.id,
        "reason": format!("failed {} runs in a row", a.health.fail_streak),
    })
    .to_string()
}

fn job_name(a: &Automation) -> String {
    let name = a.name.trim();
    if name.is_empty() {
        "An automation".into()
    } else {
        format!("\"{name}\"")
    }
}

fn error_line(a: &Automation) -> String {
    let error = a.health.error.trim().trim_end_matches('.');
    if error.is_empty() {
        "Run failed".into()
    } else {
        error.to_string()
    }
}

fn candidate(a: &Automation) -> AutoCandidate {
    let arguments = step_args(a);
    let n = a.health.fail_streak;
    AutoCandidate {
        key: hx::proactive_key(TOOL, &arguments),
        tool: TOOL.into(),
        hard: hx::step_class(TOOL, &arguments),
        scope: Some(SCOPE.into()),
        desktop: false,
        value: 0.6,
        confidence: 0.85,
        reversibility: if hx::ledger_target(TOOL, &arguments).is_some() {
            1.0
        } else {
            0.0
        },
        undo: hx::step_undo(TOOL, &arguments),
        evidence: f64::from(n),
        summary: format!("Paused {}: it failed {n} times in a row", job_name(a)),
        offer: format!("pause {}", job_name(a)),
        why: format!("Last error: {}. Undo turns it back on.", error_line(a)),
        arguments,
    }
}

/// MindCheck at the pass's own clock.
struct At(u64);

impl hx::Clock for At {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

fn pill(config_dir: &Path) -> PillMode {
    match crate::settings::load(config_dir).approvals {
        crate::settings::Approvals::Ask => PillMode::Ask,
        crate::settings::Approvals::Auto => PillMode::Auto,
        crate::settings::Approvals::Always => PillMode::Always,
    }
}

/// Halt after this run started: auto-acts stay off until the next start.
fn halted(config_dir: &Path) -> bool {
    grokhub_core::desktop_mcp::read_halt_stamp_in(config_dir)
        .is_some_and(|at| at > grokhub_desktop::process_started_ms())
}

fn ceiling_ctx(
    config_dir: &Path,
    key: &str,
    mind: &hx::MindCheck,
    day: &str,
    quiet: bool,
    busy: bool,
) -> CeilingCtx {
    let budget = load_state(config_dir).budget;
    CeilingCtx {
        in_scope: on(config_dir),
        access: AccessTier::Supervised,
        pill: pill(config_dir),
        p_mind: mind.prior(key).map(|p| p.p_mind()),
        mind_asks: mind.mind_route(&hx::Candidate { key, hard: None }) == hx::MindRoute::Ask,
        quiet,
        busy,
        halted: halted(config_dir),
        budget_left: budget.left(day, quiet, busy),
        restore_left: budget.restore_left(day, quiet, busy),
    }
}

/// One proactive pass. Pauses at most one job and returns its Done card.
/// The switch off reads nothing. `busy` is a running chat or a waiting
/// decision; `route_ok` is whether chat has a model route right now.
pub fn tick_one(
    config_dir: &Path,
    pid: u32,
    now_ms: u64,
    offset_min: i32,
    quiet: bool,
    busy: bool,
    route_ok: bool,
) -> Option<UpdateCard> {
    use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};
    if !on(config_dir) || !shell_may_fire(read_lease(config_dir).as_ref(), pid, now_ms) {
        return None;
    }
    let list = crate::automations::load(config_dir).ok()?;
    let job = pausable(&list, route_ok)?.clone();
    let c = candidate(&job);
    let mind = hx::proactive_mind(config_dir, Arc::new(At(now_ms)));
    let day = crate::nudges::local_day(now_ms, offset_min);
    let ctx = ceiling_ctx(config_dir, &c.key, &mind, &day, quiet, busy);
    let act = match AutoAct::admit(c.clone(), &ctx) {
        Ok(act) => act,
        Err(miss) => {
            note_miss(config_dir, &job.id, &c, miss);
            return None;
        }
    };
    let run = crate::automations::with_store(config_dir, |list| {
        // Read again under the lock: the job may have changed since.
        let still = list.ok()?.into_iter().find(|a| a.id == job.id)?;
        if !still.enabled {
            return None;
        }
        let _pin = grokhub_agent::perm::ConfigGuard::set(config_dir);
        Some(hx::run_auto_act(
            config_dir,
            config_dir,
            &act,
            &mind,
            AccessMode::Supervised,
        ))
    })?;
    let hx::AutoRun::Done { change, .. } = run else {
        return None;
    };
    change_state(config_dir, |s| {
        s.budget.spend(&day, act.candidate().undo);
        s.last_ask = None;
    });
    let card = grokhub_core::update_feed::done_for_you_card(
        &c.summary,
        &c.why,
        DoneForYou {
            kind: KIND.into(),
            target: job.id.clone(),
            undo_ref: change.seq,
            mind_key: c.key.clone(),
            answered: false,
        },
        now_ms,
    );
    match crate::pulse::add_post(config_dir, card.clone()) {
        Ok(true) => Some(card),
        Ok(false) => None,
        Err(e) => {
            // The job is paused and its ledger row in Settings > Behavior
            // still offers Undo.
            eprintln!("done for you: {e}");
            None
        }
    }
}

/// One `ask` span per standing miss: the same job held back for the same
/// reason writes nothing new.
fn note_miss(config_dir: &Path, id: &str, c: &AutoCandidate, miss: CeilingMiss) {
    let now = (id.to_string(), miss.as_str().to_string());
    let fresh = change_state(config_dir, |s| {
        if s.last_ask.as_ref() == Some(&now) {
            return false;
        }
        s.last_ask = Some(now.clone());
        true
    });
    if fresh {
        hx::note_proactive(
            config_dir,
            &hx::proactive_span(
                &c.tool,
                &c.arguments,
                hx::DECISION_ASK,
                miss.as_str(),
                AccessMode::Supervised,
            ),
        );
    }
}

/// The id a card's Pause it chip would pause, read back from the card. Only
/// the id is taken from what the card stored, and only for this one step.
fn chip_target(card: &UpdateCard) -> Option<String> {
    let meta = card.pulse.proactive.as_ref()?;
    if meta.tool != TOOL {
        return None;
    }
    let args: serde_json::Value = serde_json::from_str(meta.arguments.as_deref()?).ok()?;
    let id = args.get("id")?.as_str()?;
    hx::entry_id(id).ok().filter(|ok| ok == id)
}

/// Whether a card shows the Pause it chip.
pub fn offers_pause(card: &UpdateCard) -> bool {
    chip_target(card).is_some()
}

fn find_card(config_dir: &Path, card_id: &str) -> Result<UpdateCard, String> {
    crate::pulse::cards(config_dir)?
        .into_iter()
        .find(|c| c.id == card_id)
        .ok_or_else(|| GONE_CARD.to_string())
}

/// The Pause it chip: the user's tap runs the pause once. It writes the
/// `approve` span MindCheck needs before anything can run on its own.
pub fn pause_from_card(config_dir: &Path, card_id: &str, now_ms: u64) -> Result<String, String> {
    let card = find_card(config_dir, card_id)?;
    let id = chip_target(&card).ok_or_else(|| NOT_PAUSABLE.to_string())?;
    let arguments = json!({ "id": id, "reason": "paused from its Pulse card" }).to_string();
    let (run, name) = crate::automations::with_store(config_dir, |list| {
        let job = list?
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| "That automation is gone.".to_string())?;
        if !job.enabled {
            return Err("That automation is already paused.".to_string());
        }
        let _pin = grokhub_agent::perm::ConfigGuard::set(config_dir);
        let run = hx::run_offered_step(
            config_dir,
            config_dir,
            TOOL,
            &arguments,
            AccessMode::Supervised,
        );
        Ok((run, job_name(&job)))
    })?;
    match run {
        hx::AutoRun::Done { .. } => {}
        hx::AutoRun::Failed(why) => return Err(why),
        hx::AutoRun::NoUndo | hx::AutoRun::Asked(_) => return Err(NOT_PAUSABLE.into()),
    }
    let receipt = CardReceipt {
        at_ms: now_ms,
        what: format!("Paused {name}"),
        result: vec![PAUSED_RECEIPT.to_string()],
    };
    crate::pulse::edit_cards(config_dir, &mut |cards| {
        let Some(stored) = cards.iter_mut().find(|c| c.id == card_id) else {
            return 0;
        };
        add_receipt(stored, receipt.clone());
        if let Some(meta) = stored.pulse.proactive.as_mut() {
            meta.arguments = None;
        }
        1
    })?;
    crate::proactive::card_acted(config_dir, &card, crate::proactive::Feedback::Used, now_ms);
    Ok(format!("Paused {name}. Undo is in Settings > Behavior."))
}

/// A Done card's Undo: turns the job back on from the exact ledger line the
/// pause wrote. A newer change on the same job is refused.
pub fn undo(config_dir: &Path, card_id: &str) -> Result<String, String> {
    let card = find_card(config_dir, card_id)?;
    let done = card
        .done_for_you
        .clone()
        .ok_or_else(|| "Nothing to undo on this card.".to_string())?;
    if done.answered {
        return Ok(ALREADY_ANSWERED.into());
    }
    if done.kind != KIND {
        return Err("That kind of change is undone in Settings > Behavior.".into());
    }
    let row = crate::automations::load(config_dir)?
        .into_iter()
        .find(|a| a.id == done.target);
    match row {
        None => {
            mark_answered(config_dir, &done.target, Some(done.undo_ref));
            Ok("That automation is gone, so there is nothing to turn back on.".into())
        }
        Some(a) if a.enabled => {
            mark_answered(config_dir, &done.target, Some(done.undo_ref));
            Ok(format!("{} is already back on.", job_name(&a)))
        }
        Some(_) => crate::changes::undo_seq(
            config_dir,
            ChangeKind::Automation,
            &done.target,
            Some(done.undo_ref),
            true,
        ),
    }
}

/// A ledger Undo of an automation change, from anywhere (the Done card,
/// Settings > Behavior). When it undid a Done for you pause, MindCheck hears
/// an `undo` once, and the card's buttons go away.
pub fn after_undo(config_dir: &Path, kind: ChangeKind, id: &str, reverted_seq: u64) {
    if kind != ChangeKind::Automation {
        return;
    }
    answer_undo(config_dir, id, Some(reverted_seq));
}

/// The job was turned back on by hand: the same signal for an open Done
/// card on it.
pub fn turned_back_on(config_dir: &Path, id: &str) {
    answer_undo(config_dir, id, None);
}

fn answer_undo(config_dir: &Path, id: &str, seq: Option<u64>) {
    let cards = crate::pulse::cards(config_dir).unwrap_or_default();
    let ours = |c: &&UpdateCard| {
        c.done_for_you.as_ref().is_some_and(|d| {
            d.kind == KIND && d.target == id && seq.is_none_or(|s| d.undo_ref == s)
        })
    };
    let seqs: Vec<u64> = match seq {
        Some(s) => vec![s],
        None => cards
            .iter()
            .filter(ours)
            .filter_map(|c| c.done_for_you.as_ref())
            .filter(|d| !d.answered)
            .map(|d| d.undo_ref)
            .collect(),
    };
    if seqs.is_empty() {
        return;
    }
    let spans = hx::read_spans(config_dir, hx::PROACTIVE_TRACE).unwrap_or_default();
    for s in &seqs {
        let undo_ref = format!("{KIND}:{s}");
        let answered = cards.iter().filter(ours).any(|c| {
            c.done_for_you
                .as_ref()
                .is_some_and(|d| d.undo_ref == *s && d.answered)
        });
        let auto = spans
            .iter()
            .find(|sp| sp.decision == hx::DECISION_AUTO && sp.undo_ref == undo_ref);
        if let (Some(auto), false) = (auto, answered) {
            let key = hx::mind_key(auto);
            hx::note_proactive(
                config_dir,
                &hx::answer_span(&key, hx::DECISION_UNDO, AccessMode::Supervised),
            );
        }
        mark_answered(config_dir, id, Some(*s));
    }
}

fn mark_answered(config_dir: &Path, id: &str, seq: Option<u64>) {
    let _ = crate::pulse::edit_cards(config_dir, &mut |cards| {
        let mut changed = 0;
        for c in cards.iter_mut() {
            if let Some(d) = c.done_for_you.as_mut() {
                if d.kind == KIND
                    && d.target == id
                    && seq.is_none_or(|s| d.undo_ref == s)
                    && !d.answered
                {
                    d.answered = true;
                    changed += 1;
                }
            }
        }
        changed
    });
}

/// A Done card's Stop doing this: the switch goes off. No `never` span, so a
/// Pause it tap still works and turning the switch on again picks up as it was.
pub fn stop(config_dir: &Path) -> Result<String, String> {
    set_on(config_dir, false)?;
    Ok(format!(
        "Turned off. {} won't pause automations on its own.",
        grokhub_core::brand::product_name()
    ))
}

/// While the switch is on, what holds it back right now, in plain words.
/// Quiet hours and a busy moment pass on their own, so they are not shown.
pub fn held(config_dir: &Path, now_ms: u64, offset_min: i32) -> Option<String> {
    if !on(config_dir) {
        return None;
    }
    let key = hx::proactive_key(TOOL, "{}");
    let mind = hx::proactive_mind(config_dir, Arc::new(At(now_ms)));
    let day = crate::nudges::local_day(now_ms, offset_min);
    let ctx = ceiling_ctx(config_dir, &key, &mind, &day, false, false);
    let template = AutoCandidate {
        key,
        tool: TOOL.into(),
        arguments: "{}".into(),
        hard: None,
        scope: Some(SCOPE.into()),
        desktop: false,
        value: 0.6,
        confidence: 0.85,
        reversibility: 1.0,
        undo: grokhub_core::learn::Undo::OneClick,
        evidence: f64::from(PAUSE_AFTER),
        summary: String::new(),
        offer: String::new(),
        why: String::new(),
    };
    let miss = grokhub_core::proactive_auto::ceiling_allows(&template, &ctx).err()?;
    held_line(miss)
}

fn held_line(miss: CeilingMiss) -> Option<String> {
    let line = match miss {
        CeilingMiss::Pill => {
            "Waiting: \"When the AI wants to change things\" is on \"Ask me first\"."
        }
        CeilingMiss::NoHistory => {
            "Waiting: tap Pause it on a failing automation's Pulse card once first."
        }
        CeilingMiss::PMind | CeilingMiss::MindCheck => {
            "Waiting: you turned a paused automation back on lately, so it asks instead for now."
        }
        CeilingMiss::Halted => {
            return Some(format!(
                "Waiting: Halt is on. It starts again after {} restarts.",
                grokhub_core::brand::product_name()
            ))
        }
        CeilingMiss::Budget => "Waiting: it already paused 5 automations today.",
        _ => return None,
    };
    Some(line.to_string())
}

#[cfg(test)]
#[path = "done_for_you_tests.rs"]
mod tests;
