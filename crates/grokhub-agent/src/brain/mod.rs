//! Brain 1: before a run starts, a short Brief says what kind of ask it is,
//! how careful the run must be, and what "done" means, and the mode policy
//! table names the row it matches. The trail and `/why` show them; the row's
//! goal and reason steer the turn's routing (Brain 2, `route::goal`).

mod brief;
pub mod playbook;
mod policy;

pub use brief::{
    brief_for_turn, classify, read_briefs, record_brief, rules, why_lines, AskModel, Brief,
    BriefInput, BriefLine, Care, Intent, BRIEF_CLASS, BRIEF_PROMPT,
};
pub use policy::{load_policy, pick_policy, policy_path, PolicyPick};
