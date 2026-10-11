//! Brain 1: before a run starts, a short Brief says what kind of ask it is,
//! how careful the run must be, and what "done" means, and the mode policy
//! table names the row it matches. Phase 1 only records them (the trail and
//! `/why` show them); nothing reads them to change a reply yet.

mod brief;
mod policy;

pub use brief::{
    brief_for_turn, classify, read_briefs, record_brief, rules, why_lines, AskModel, Brief, BriefInput, BriefLine, Care,
    Intent, BRIEF_CLASS, BRIEF_PROMPT,
};
pub use policy::{load_policy, pick_policy, policy_path, PolicyPick};
