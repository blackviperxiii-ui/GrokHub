//! Linux frame rate for the Home tile, counted by the kernel from display flips
//! (see `grokhub_agent::indexers::display_fps`). A background thread reads the
//! counters once a second while the tile is painted, and closes them five
//! seconds after Home stops painting.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_agent::indexers::display_fps::{FlipCounter, FpsBlock};

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
}

struct Slot {
    state: FpsState,
    painted: Option<Instant>,
    running: bool,
    failed_at: Option<Instant>,
}

static SLOT: Mutex<Slot> = Mutex::new(Slot {
    state: FpsState::Starting,
    painted: None,
    running: false,
    failed_at: None,
});

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
    slot.state
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
            slot.state = FpsState::Rate(r);
        }
        if slot.painted.is_none_or(|t| t.elapsed() >= IDLE) {
            slot.state = FpsState::Starting;
            slot.running = false;
            return;
        }
    }
}
