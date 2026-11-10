//! One "right now" snapshot both engines read.
//!
//! Each app already knows pieces of this (quiet hours, a running turn, Halt,
//! the game guard, the power source), and each decider used to read its own.
//! The app tells the hub the whole picture with [`super::hub::set_now`], and
//! every decider reads the same [`Snapshot`].

use serde::Serialize;

use super::decide::Situation;

/// What is going on at this moment. All false until the app says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Inside the user's quiet hours.
    pub quiet: bool,
    /// A turn is running, or the user is typing or has a question waiting.
    pub busy: bool,
    /// Halt is on.
    pub halted: bool,
    /// A game is running.
    pub gaming: bool,
    /// The PC runs from its battery.
    pub on_battery: bool,
}

impl Snapshot {
    const QUIET: u8 = 1;
    const BUSY: u8 = 2;
    const HALTED: u8 = 4;
    const GAMING: u8 = 8;
    const ON_BATTERY: u8 = 16;

    /// The snapshot as one byte, so the hub keeps it without a lock.
    pub fn bits(self) -> u8 {
        [
            (self.quiet, Self::QUIET),
            (self.busy, Self::BUSY),
            (self.halted, Self::HALTED),
            (self.gaming, Self::GAMING),
            (self.on_battery, Self::ON_BATTERY),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .fold(0, |bits, (_, bit)| bits | bit)
    }

    pub fn from_bits(bits: u8) -> Self {
        Self {
            quiet: bits & Self::QUIET != 0,
            busy: bits & Self::BUSY != 0,
            halted: bits & Self::HALTED != 0,
            gaming: bits & Self::GAMING != 0,
            on_battery: bits & Self::ON_BATTERY != 0,
        }
    }

    /// What the decision engine holds changes back for. A game counts as
    /// busy: nothing should change under a running game.
    pub fn situation(self) -> Situation {
        Situation {
            quiet: self.quiet,
            busy: self.busy || self.gaming,
            halted: self.halted,
        }
    }
}
