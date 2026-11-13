//! Chat's task outcomes and run metrics, written where the cabin writes
//! them so the weekly self-review and skill-change trials judge runs from
//! either app. The recording itself is shared in
//! `grokhub_agent::harness::chat_record`; this is the desktop app's wiring:
//! which skill a message ran, how long runs are kept, and the three moments
//! (turn end, a "that was wrong" reply, a skill undo).

use std::path::Path;

use grokhub_agent::harness::chat_record as cr;
use grokhub_agent::harness::weekly_review as wr;
use grokhub_core::outcome::TaskOutcome;

/// Days a run-metrics line is kept: app.json `amrRetentionDays` (the
/// cabin's key, snake case too), 90 when unset; 0 keeps everything.
pub fn keep_days(config_dir: &Path) -> u32 {
    let Ok(map) = grokhub_core::app_json::read(config_dir) else {
        return 90;
    };
    map.get("amrRetentionDays")
        .or_else(|| map.get("amr_retention_days"))
        .and_then(|n| n.as_u64())
        .map_or(90, |n| n.min(u64::from(u32::MAX)) as u32)
}

/// The task a message asks for: the switched-on skill it runs, else its topic.
pub fn signature(config_dir: &Path, ask: &str) -> String {
    let skills = wr::cabin_skills(&config_dir.join("skills"));
    let skill = grokhub_core::match_skill(ask, &skills)
        .map(|s| s.name.clone())
        .unwrap_or_default();
    cr::task_signature(&skill, ask)
}

/// A chat turn that started at `started` finished: one outcome and one
/// metrics line for its steps. A turn with no steps writes nothing.
pub fn turn_end(
    config_dir: &Path,
    chat: &str,
    ask: &str,
    started: u64,
    now: u64,
) -> Option<TaskOutcome> {
    let turn = cr::latest_step_turn(config_dir, chat, started)?;
    let sig = signature(config_dir, ask);
    cr::record_turn(
        config_dir,
        chat,
        turn,
        &sig,
        false,
        now,
        keep_days(config_dir),
    )
}

/// The person's new message, before it runs: "that was wrong" fails the
/// chat's last task.
pub fn said(config_dir: &Path, chat: &str, text: &str, now: u64) -> Option<TaskOutcome> {
    cr::user_said(config_dir, chat, text, now, keep_days(config_dir))
}

/// A skill change was undone: that skill's tasks from the day before fail.
pub fn skill_undone(config_dir: &Path, skill: &str, now: u64) -> usize {
    cr::after_undo(config_dir, skill, now, keep_days(config_dir))
}

#[cfg(test)]
#[path = "chat_trials_tests.rs"]
mod tests;
