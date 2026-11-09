//! Frame rate for the Home tile: on Linux the kernel counts display flips, on
//! Windows the graphics kernel's present events (see
//! `grokhub_agent::indexers::display_fps`). Never from inside a game. A background thread reads the
//! counters once a second while the tile is painted, and closes them five
//! seconds after Home stops painting.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_agent::indexers::display_fps::setup::{self, Outcome};
use grokhub_agent::indexers::display_fps::{FlipCounter, FpsBlock};
use grokhub_core::home::text::FpsSetup;

const TICK: Duration = Duration::from_secs(1);
const IDLE: Duration = Duration::from_secs(5);
/// After a failed open, try again this much later (the user may have just run
/// the setup).
const RETRY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum FpsState {
    Starting,
    Blocked(FpsBlock),
    Rate(f32),
    /// How the last one-tap setup went, while frames still aren't counted.
    Note(FpsSetup),
    /// The setup is waiting on the OS password or admin prompt.
    SettingUp,
}

struct Slot {
    state: FpsState,
    /// Windows: the exe the rate came from.
    presenter: Option<String>,
    painted: Option<Instant>,
    running: bool,
    failed_at: Option<Instant>,
    note: Option<FpsSetup>,
    setting_up: bool,
    /// Copy setup was pressed somewhere without a clipboard at hand; the
    /// tile copies it on its next paint.
    copy: bool,
}

static SLOT: Mutex<Slot> = Mutex::new(Slot {
    state: FpsState::Starting,
    presenter: None,
    painted: None,
    running: false,
    failed_at: None,
    note: None,
    setting_up: false,
    copy: false,
});

/// Runs the one-tap setup (or its undo) on its own thread: it waits on the
/// OS prompt. The tile shows how it went, and the reader tries again at once.
pub(super) fn start_setup(undo: bool) {
    let Ok(mut slot) = SLOT.lock() else {
        return;
    };
    if slot.setting_up {
        return;
    }
    slot.setting_up = true;
    drop(slot);
    let spawned = std::thread::Builder::new()
        .name("home-fps-setup".into())
        .spawn(move || {
            let outcome = setup::run(undo);
            if let Ok(mut slot) = SLOT.lock() {
                slot.note = if undo { None } else { note_for(&outcome) };
                slot.failed_at = None;
                slot.setting_up = false;
            }
        });
    if spawned.is_err()
        && let Ok(mut slot) = SLOT.lock()
    {
        slot.setting_up = false;
    }
}

fn note_for(outcome: &Outcome) -> Option<FpsSetup> {
    match outcome {
        Outcome::Done | Outcome::Unsupported => None,
        Outcome::SignOut => Some(FpsSetup::SignOut),
        Outcome::Dismissed => Some(FpsSetup::Dismissed),
        Outcome::NoPrompt => Some(FpsSetup::NoPrompt),
        Outcome::Failed(_) => Some(FpsSetup::Failed),
    }
}

/// Asks the tile to copy the setup steps on its next paint.
pub(super) fn queue_copy() {
    if let Ok(mut slot) = SLOT.lock() {
        slot.copy = true;
    }
}

/// Whether a copy was asked for since the last paint.
pub(super) fn take_copy() -> bool {
    SLOT.lock()
        .map(|mut s| std::mem::take(&mut s.copy))
        .unwrap_or(false)
}

/// The latest reading. Marks the tile as painted and starts the reader.
pub(super) fn display_fps() -> FpsState {
    let Ok(mut slot) = SLOT.lock() else {
        return FpsState::Starting;
    };
    slot.painted = Some(Instant::now());
    let may_try = slot.failed_at.is_none_or(|t| t.elapsed() >= RETRY);
    if !slot.running && may_try {
        slot.running = true;
        if std::thread::Builder::new()
            .name("home-fps".into())
            .spawn(run)
            .is_err()
        {
            slot.running = false;
        }
    }
    if slot.setting_up {
        return FpsState::SettingUp;
    }
    match (slot.state, slot.note) {
        (FpsState::Blocked(_) | FpsState::Starting, Some(n)) => FpsState::Note(n),
        (state, _) => state,
    }
}

/// Windows: the exe the latest rate came from.
pub(super) fn presenter() -> Option<String> {
    SLOT.lock().ok().and_then(|s| s.presenter.clone())
}

fn run() {
    let mut counter = match FlipCounter::open() {
        Ok(c) => c,
        Err(block) => {
            if let Ok(mut slot) = SLOT.lock() {
                slot.state = FpsState::Blocked(block);
                slot.failed_at = Some(Instant::now());
                slot.running = false;
            }
            return;
        }
    };
    counter.sample();
    loop {
        std::thread::sleep(TICK);
        let rate = counter.sample();
        let Ok(mut slot) = SLOT.lock() else {
            return;
        };
        slot.failed_at = None;
        if let Some(r) = rate {
            slot.note = None;
            slot.state = FpsState::Rate(r);
            slot.presenter = counter.presenter().map(str::to_owned);
        }
        if slot.painted.is_none_or(|t| t.elapsed() >= IDLE) {
            slot.state = FpsState::Starting;
            slot.presenter = None;
            slot.running = false;
            return;
        }
    }
}
