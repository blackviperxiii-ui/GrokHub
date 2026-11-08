//! Frame rate for Home's Frame Rate tile in the shell. The OS counts the
//! frames (display flips on Linux, present events on Windows; see
//! `grokhub_agent::indexers::display_fps`), never the game. A background
//! thread reads the counter once a second while Home asks for the tile and
//! closes it shortly after Home stops asking.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_agent::indexers::display_fps::{FlipCounter, FpsBlock};
use grokhub_core::home::text::FpsRead;

const TICK: Duration = Duration::from_secs(1);
/// Home polls this tile every couple of seconds; stop a little after that.
const IDLE: Duration = Duration::from_secs(10);
/// After a failed open, try again this much later (the setup may have run).
const RETRY: Duration = Duration::from_secs(30);

struct Slot {
    read: FpsRead,
    presenter: Option<String>,
    asked: Option<Instant>,
    running: bool,
    failed_at: Option<Instant>,
}

static SLOT: Mutex<Slot> = Mutex::new(Slot {
    read: FpsRead::Starting,
    presenter: None,
    asked: None,
    running: false,
    failed_at: None,
});

/// The latest reading and, on Windows, the app it came from. Starts the
/// reader when it isn't running.
pub fn read() -> (FpsRead, Option<String>) {
    let Ok(mut slot) = SLOT.lock() else {
        return (FpsRead::Starting, None);
    };
    slot.asked = Some(Instant::now());
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
    (slot.read, slot.presenter.clone())
}

fn blocked(block: FpsBlock) -> FpsRead {
    match block {
        FpsBlock::NeedsPermission => FpsRead::NeedsSetup,
        FpsBlock::NoTracepoint | FpsBlock::Unsupported => FpsRead::NoTrace,
    }
}

fn run() {
    let mut counter = match FlipCounter::open() {
        Ok(c) => c,
        Err(block) => {
            if let Ok(mut slot) = SLOT.lock() {
                slot.read = blocked(block);
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
            slot.read = FpsRead::Rate(r);
            slot.presenter = counter.presenter().map(str::to_owned);
        }
        if slot.asked.is_none_or(|t| t.elapsed() >= IDLE) {
            slot.read = FpsRead::Starting;
            slot.presenter = None;
            slot.running = false;
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blocked_counter_says_why_in_tile_terms() {
        assert_eq!(blocked(FpsBlock::NeedsPermission), FpsRead::NeedsSetup);
        assert_eq!(blocked(FpsBlock::NoTracepoint), FpsRead::NoTrace);
        assert_eq!(blocked(FpsBlock::Unsupported), FpsRead::NoTrace);
    }
}
