//! Brain 1: before a run starts, a short Brief says what kind of ask it is,
//! how careful the run must be, and what "done" means, and the mode policy
//! table names the row it matches. The trail shows them; the row's
//! goal and reason steer the turn's routing (Brain 2, `route::goal`).

mod brief;
pub mod instant;
mod local_sort;
pub mod playbook;
mod policy;

#[cfg(test)]
pub(crate) use brief::SYSTEM_DONE_TEXT;
pub use brief::{
    brief_for_turn, classify, classify_effort, effort_word, is_follow_up, is_go_on, is_picture_ask,
    read_briefs, record_brief, rules, AskModel, Brief, BriefInput, BriefLine, Care, Intent,
    BRIEF_CLASS, BRIEF_PROMPT,
};
pub use policy::{
    care_first_enabled, effective_rows, load_policy, pick_policy, policy_path, PolicyPick,
};
