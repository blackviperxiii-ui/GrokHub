//! The governor's sample step, shared by every shell: one [`Sampler`] per
//! app turns each reading of the machine into a level
//! ([`local_governor`](grokhub_core::local_governor)), hands changes to
//! [`local_gov`](super::local_gov), and runs the sidecar's ticks (a game
//! cutting a GPU server, restarts, scaling, keep-warm, idle unload, the
//! unattended draft speed test). A shell supplies only its readers.
//!
//! [`start`] runs it on its own thread with the readers every desktop has
//! here: power, heat and free memory from [`crate::device`], and the
//! shell's game reader, read every sample so a game frees the GPU within one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use grokhub_core::local_governor::{self as g, Governor, Level, Memory, Rules, Signals, Want};
use grokhub_core::local_profile::Thermal;

use super::{local, local_autoconfig, local_gov, local_scale, local_sidecar, local_spec};

/// Slow signals (power, heat, memory, the app lists) are read every this many samples.
pub const SLOW_EVERY: u32 = 5;

/// The governor and its rules, between samples.
#[derive(Default)]
pub struct Sampler {
    gov: Governor,
    rules: Rules,
    tick: u32,
}

impl Sampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// This sample reads the slow signals too.
    pub fn slow(&self) -> bool {
        self.tick.is_multiple_of(SLOW_EVERY)
    }

    /// The level these signals call for now, applied when it changes. A
    /// known game steps down at its first sample.
    pub fn observe(&mut self, s: &Signals, now_ms: u64) -> Option<Want> {
        let want = g::target(s, &self.rules);
        if let Some(level) = self.gov.observe_want(want, now_ms) {
            local_gov::apply(level, want.map_or("normal use", |w| w.reason));
        }
        want
    }

    /// Local AI is off or not installed: forget any pause so switching on
    /// starts normally, and keep the idle rule running (a speed test can
    /// load the model while the switch is off).
    pub fn idle(&mut self) {
        if self.gov.level != Level::Normal {
            self.gov = Governor::new();
            local_gov::apply(Level::Normal, "normal use");
        }
        local_sidecar::tick_idle_unload();
    }

    /// One sample: `s` from the shell's readers, `room` from the slow tick
    /// (`None` keeps the last). Reloads the rules from `governor.json` on
    /// the slow tick.
    pub fn step(&mut self, config_dir: &Path, s: &Signals, room: Option<local_scale::Room>) {
        let slow = self.slow();
        if slow {
            self.rules = std::fs::read_to_string(config_dir.join("local").join("governor.json"))
                .map(|t| Rules::from_json(&t))
                .unwrap_or_default();
        }
        self.tick = self.tick.wrapping_add(1);
        let now = now_ms();
        let want = self.observe(s, now);
        // Sharing the GPU with a game: when it takes much more VRAM than was
        // free with the model loaded, relaunch into what is left. What this
        // sample sees decides, so battery or heat alone never restarts it.
        if slow
            && local_gov::level() == Level::Throttle
            && want
                .is_some_and(|w| w.level == Level::Throttle && local_scale::gaming_reason(w.reason))
            && local_autoconfig::vram_dropped_since_launch()
        {
            local_gov::request_restart();
        }
        local_sidecar::tick_governor();
        if let Some(room) = room {
            local_scale::set_room(room, now);
        }
        local_sidecar::tick_scale(config_dir);
        local_sidecar::tick_keep_warm(config_dir, local::warm_wanted());
        local_sidecar::tick_idle_unload();
        if slow {
            local_spec::tick_auto_test(config_dir, &quiet(s, want.is_none()));
        }
    }
}

/// What the unattended draft speed test reads from one sample: `calm` when
/// the governor wants nothing slowed.
pub fn quiet(s: &Signals, calm: bool) -> grokhub_core::local_draft::Quiet {
    grokhub_core::local_draft::Quiet {
        calm: calm && local_gov::level() == Level::Normal,
        on_battery: s.power.as_ref().is_some_and(|p| p.battery && !p.on_ac),
        game: s.game == Some(true) || s.fullscreen == Some(true),
        user_idle_secs: s.idle_secs,
        model_idle_secs: local_sidecar::secs_since_use(),
        in_flight: local_sidecar::in_flight(),
        warm_wanted: local::warm_wanted(),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The signals [`start`]'s readers give: `game` every sample, the rest
/// (power, heat, free RAM against `total_mb`) as last read on the slow tick.
pub fn signals(
    game: Option<bool>,
    slow: &SlowRead,
    total_mb: Option<u64>,
    needed: bool,
) -> Signals {
    Signals {
        game,
        power: slow.power,
        thermal: slow.thermal,
        memory: match (slow.free_ram_mb, total_mb) {
            (Some(avail_mb), Some(total_mb)) => Some(Memory { avail_mb, total_mb }),
            _ => None,
        },
        needed,
        ..Signals::default()
    }
}

/// The slow tick's readings for [`signals`].
#[derive(Debug, Clone, Default)]
pub struct SlowRead {
    pub power: Option<grokhub_core::local_profile::PowerInfo>,
    pub thermal: Thermal,
    pub free_ram_mb: Option<u64>,
}

fn read_slow() -> SlowRead {
    #[cfg(target_os = "linux")]
    let thermal = crate::device::thermal_now();
    #[cfg(not(target_os = "linux"))]
    let thermal = Thermal::Unknown;
    SlowRead {
        power: crate::device::power_quick(),
        thermal,
        free_ram_mb: crate::device::free_ram_now(),
    }
}

/// Run the sampler on its own thread, once per process. `game` is the
/// shell's game reader (`None` where it can't tell).
pub fn start(config_dir: PathBuf, game: impl Fn() -> Option<bool> + Send + 'static) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("local-governor".into())
        .spawn(move || {
            local_gov::set_log_dir(config_dir.join("local"));
            // Every launch plans against the memory free right now.
            local_autoconfig::set_free_probe(std::sync::Arc::new(|| local_autoconfig::FreeNow {
                vram_free_mb: crate::device::free_vram_now(),
                ram_free_mb: crate::device::free_ram_now(),
            }));
            run(&config_dir, &game)
        });
    if spawned.is_err() {
        STARTED.store(false, Ordering::SeqCst);
    }
}

fn run(config_dir: &Path, game: &dyn Fn() -> Option<bool>) {
    let mut sampler = Sampler::new();
    let mut slow = SlowRead::default();
    let total_mb = crate::device::ram_mb();
    loop {
        if !(local::switched_on() && local_sidecar::assets_present(config_dir)) {
            sampler.idle();
            std::thread::sleep(Duration::from_secs(10));
            continue;
        }
        let room = sampler.slow().then(|| {
            slow = read_slow();
            slow.free_ram_mb.map(|free_ram_mb| local_scale::Room {
                free_vram_mb: crate::device::free_vram_now(),
                free_ram_mb,
                cpu_busy_pct: crate::device::cpu_busy_pct(),
                on_battery: slow.power.as_ref().is_some_and(|p| p.battery && !p.on_ac),
                // The GPU shares RAM (Apple silicon), from the hardware read.
                unified: local_autoconfig::profile()
                    .is_some_and(|p| p.gpus.iter().any(|g| g.unified)),
            })
        });
        let s = signals(game(), &slow, total_mb, local::warm_wanted());
        sampler.step(config_dir, &s, room.flatten());
        std::thread::sleep(Duration::from_millis(g::interval_ms(s.power.as_ref())));
    }
}

#[cfg(test)]
#[path = "local_sampler_tests.rs"]
mod tests;
