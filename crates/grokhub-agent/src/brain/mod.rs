//! Brain 1: before a run starts, a short Brief says what kind of ask it is,
//! how careful the run must be, and what "done" means. PR 1 only records it
//! (the trail and `/why` show it); nothing reads it to change a reply yet.

mod brief;

pub use brief::{
    brief_for_turn, classify, read_briefs, record_brief, rules, why_line, AskModel, Brief, BriefInput, BriefLine, Care, Intent,
    BRIEF_CLASS, BRIEF_PROMPT,
};
