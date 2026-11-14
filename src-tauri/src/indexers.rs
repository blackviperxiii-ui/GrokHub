//! The local indexers, while the cabin is closed: the scopes allowed in
//! Settings → Permissions are read on a low-priority thread, one scope a
//! look, each at most every 10 minutes (`grokhub_agent::indexers::Scheduler`,
//! which also stops on battery, in quiet hours and while private data is
//! locked). Facts land sealed in the memory repo, and Permissions shows them.
//!
//! The shell holds the indexers while:
//! - the cabin is open (its scheduler lease is fresh), so only one app reads;
//! - the window hasn't reported its clock (quiet hours need it);
//! - Halt holds background work (`night::halted`);
//! - a chat is running, a decision waits or a game is on;
//! - the chat you have open is a scratch chat, which learns nothing from you.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_agent::indexers::fs::{IndexFs, RealFs};
use grokhub_agent::indexers::paths::PlatformDirs;
use grokhub_agent::indexers::power::{OsPower, PowerSource};
use grokhub_agent::indexers::system_state::{OsProbe, SystemProbe};
use grokhub_agent::indexers::{Scheduler, TickEnv, TickOutcome};
use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};

/// How often the thread looks. The scheduler's rescan time bounds real reads.
const EVERY: Duration = Duration::from_secs(60);

/// Why a look read nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    CabinOpen,
    NoClock,
    Halted,
    Busy,
    Scratch,
}

/// What one look did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Held(Hold),
    Ticked(TickOutcome),
}

impl Step {
    /// New facts landed: Permissions should show them.
    pub fn learned(&self) -> bool {
        matches!(self, Step::Ticked(TickOutcome::Ran { written, .. }) if *written > 0)
    }
}

/// The look's inputs that come from the app, not the disk.
pub struct Cx<'a> {
    pub config_dir: &'a Path,
    pub pid: u32,
    pub now: u64,
    /// The window's UTC offset in minutes, once it has reported it.
    pub offset_min: Option<i32>,
    /// A chat is running, a decision waits or a game is on.
    pub busy: bool,
}

/// The machine the scheduler reads. Tests pass fakes.
pub struct Machine<'a> {
    pub power: &'a dyn PowerSource,
    pub fs: &'a dyn IndexFs,
    pub probe: &'a dyn SystemProbe,
    pub dirs: &'a PlatformDirs,
}

/// One long-lived scheduler, so one look runs at a time and each scope's
/// last read is kept between looks.
#[derive(Debug, Default)]
pub struct Driver {
    sched: Scheduler,
}

impl Driver {
    pub fn step(&mut self, cx: &Cx<'_>, machine: &Machine<'_>) -> Step {
        if !shell_may_fire(read_lease(cx.config_dir).as_ref(), cx.pid, cx.now) {
            return Step::Held(Hold::CabinOpen);
        }
        let Some(offset) = cx.offset_min else {
            return Step::Held(Hold::NoClock);
        };
        if crate::night::halted(cx.config_dir, cx.now) {
            return Step::Held(Hold::Halted);
        }
        if cx.busy {
            return Step::Held(Hold::Busy);
        }
        // The chat Chat has open, as the window last set it.
        let settings = crate::settings::load(cx.config_dir);
        if grokhub_agent::scratch::is_scratch(&settings.last_chat) {
            return Step::Held(Hold::Scratch);
        }
        let clock = crate::scheduler::local_clock(cx.now, offset);
        let env = TickEnv {
            config_dir: cx.config_dir,
            now_ms: cx.now,
            quiet: crate::proactive::is_quiet(&settings.behavior, clock.hour, clock.minute),
            power: machine.power,
            fs: machine.fs,
            probe: machine.probe,
            dirs: machine.dirs,
        };
        Step::Ticked(self.sched.tick(&env))
    }
}

/// Start the thread. `busy` is a running chat, a waiting decision or a game;
/// `learned` runs after a look that wrote new facts.
pub fn start<B, L>(config_dir: PathBuf, busy: B, learned: L)
where
    B: Fn() -> bool + Send + 'static,
    L: Fn() + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("indexers".into())
        .spawn(move || {
            grokhub_agent::indexers::power::lower_thread_priority();
            let dirs = PlatformDirs::from_env();
            let mut driver = Driver::default();
            loop {
                std::thread::sleep(EVERY);
                let cx = Cx {
                    config_dir: &config_dir,
                    pid: std::process::id(),
                    now: grokhub_core::now_ms(),
                    offset_min: crate::scheduler::utc_offset(),
                    busy: busy(),
                };
                let machine = Machine {
                    power: &OsPower,
                    fs: &RealFs,
                    probe: &OsProbe,
                    dirs: &dirs,
                };
                if driver.step(&cx, &machine).learned() {
                    learned();
                }
            }
        });
}

#[cfg(test)]
#[path = "indexers_tests.rs"]
mod tests;
