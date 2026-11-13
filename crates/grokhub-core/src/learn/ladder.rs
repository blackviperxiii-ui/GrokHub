//! The one rule for what a confidence allows.
//!
//! Auto setup already acts at 0.8 and offers at 0.5
//! ([`crate::autotune::plan`]); every decider that reads beliefs uses the
//! same two lines. A change that needs a restore point instead of a single
//! undo asks for more: [`ACT_RESTORE_AT`] and [`RESTORE_MIN_EVIDENCE`].
//!
//! This is only the confidence rule. Whether a kind of change may ever be
//! made without asking (deleting files, spending, sending anything off the
//! PC, loosening a permission) is the harness's rule, and nothing here can
//! widen it: such a change is [`Undo::Never`] and stops at an offer.

pub use crate::autotune::plan::{ACT_AT, SUGGEST_AT};

/// Sure enough to make a change that needs a restore point to take back.
pub const ACT_RESTORE_AT: f32 = 0.9;
/// Evidence a restore-point change needs, however sure the numbers look.
pub const RESTORE_MIN_EVIDENCE: f32 = 10.0;

/// How a change is taken back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undo {
    /// One click puts everything back.
    OneClick,
    /// A saved restore point puts it back.
    Restore,
    /// It can't be taken back.
    Never,
}

/// What the app may do with a belief.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    /// Keep counting, show nothing.
    Watch,
    /// Offer it once.
    Suggest,
    /// Make the change and say so.
    Act,
}

/// The step a belief's confidence and evidence allow for a change that is
/// taken back by `undo`.
pub fn step(confidence: f32, evidence: f32, undo: Undo) -> Step {
    if confidence < SUGGEST_AT {
        return Step::Watch;
    }
    let act = match undo {
        Undo::OneClick => confidence >= ACT_AT,
        Undo::Restore => confidence >= ACT_RESTORE_AT && evidence >= RESTORE_MIN_EVIDENCE,
        Undo::Never => false,
    };
    if act {
        Step::Act
    } else {
        Step::Suggest
    }
}
