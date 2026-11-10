//! Automatic scaling of the on-device model: which downloaded chat model fits
//! this task with what is free right now. There is no setting for it; the
//! router decides on every load and the running-model row names the mode.
//!
//! - **Balanced** (the default): the Settings pick, or a lighter model while
//!   the machine saves power or memory, or while less memory is free than the
//!   pick needs.
//! - **Gaming** (a game or another busy GPU app): the lightest model on disk,
//!   on the CPU, so the game keeps the GPU and its frame rate.
//! - **Performance** (on AC, the CPU mostly idle, plenty free, and a hard
//!   task): the biggest model that fits with headroom and still runs fast.
//!
//! Everything here is pure except [`set_room`] / [`room`] (the cabin's live
//! reading) and the per-thread [`Need`] a routed request sets.

use std::cell::Cell;
use std::sync::Mutex;

use grokhub_core::local_setup::LocalModel;

/// How much model a task needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Need {
    /// A quick answer or a desktop step.
    Light,
    /// The app's own background work (a label, a summary, a difficulty):
    /// it uses a model already loaded and never starts or swaps one.
    Background,
    #[default]
    Normal,
    /// Code, debugging, planning, design, math and logic, or a long context.
    Hard,
}

/// The mode the router is in, for the running-model row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Balanced,
    Gaming,
    Performance,
    /// Dynamic is off: the pick answers every task.
    Fixed,
}

impl Mode {
    /// The read-only running-model row.
    pub fn line(self) -> &'static str {
        match self {
            Mode::Balanced => {
                "Mode: Balanced. The model is picked for each task from what's free right now."
            }
            Mode::Gaming => {
                "Mode: Gaming. A lighter model answers on the CPU, so your game keeps the GPU."
            }
            Mode::Performance => {
                "Mode: Performance. The computer is free, so hard tasks get the biggest model that fits."
            }
            Mode::Fixed => {
                "Mode: Your pick. Dynamic is off, so your pick answers every task. A game, a low battery or too little memory still swaps to a lighter one."
            }
        }
    }
}

/// Classes that are hard work for a model.
const HARD_CLASSES: &[&str] = &[
    "chat:reason",
    "plan",
    "design",
    "debug",
    "code:multi-file",
    "prepare:hard",
    "repair:diagnose",
];
/// Classes a small model answers well.
const LIGHT_CLASSES: &[&str] = &["chat:quick", "desktop:soft"];
/// A message this long (a big paste) is hard whatever its class.
pub const HARD_TOKENS: u64 = 8_000;
/// Rule-based difficulty at or over this makes an everyday chat hard.
pub const HARD_DIFFICULTY: f64 = 0.5;

/// The task's need from its class (after aliases), prompt size and text.
pub fn need(class: &str, ctx_tokens: u64, text: &str) -> Need {
    let class = super::policy::class_row(class).map_or(class.trim(), |r| r.class);
    if HARD_CLASSES.contains(&class) || ctx_tokens > HARD_TOKENS {
        return Need::Hard;
    }
    if class.starts_with("background:") {
        return Need::Background;
    }
    if LIGHT_CLASSES.contains(&class) {
        return Need::Light;
    }
    let d = super::difficulty::difficulty(&super::difficulty::DifficultyInput {
        text,
        ctx_tokens,
        ..Default::default()
    });
    if d >= HARD_DIFFICULTY {
        Need::Hard
    } else {
        Need::Normal
    }
}

thread_local! {
    static NEED: Cell<Need> = const { Cell::new(Need::Normal) };
}

/// Sets this thread's [`Need`] until dropped, for the load the request makes.
pub struct NeedGuard(Need);

impl NeedGuard {
    pub fn set(need: Need) -> Self {
        NeedGuard(NEED.with(|n| n.replace(need)))
    }
}

impl Drop for NeedGuard {
    fn drop(&mut self) {
        NEED.with(|n| n.set(self.0));
    }
}

/// The need of the request this thread is loading for.
pub fn current_need() -> Need {
    NEED.with(Cell::get)
}

/// What is free on the machine, from the cabin's sampler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Room {
    /// Free VRAM across the discrete GPUs; `None` without a GPU reading.
    pub free_vram_mb: Option<u64>,
    pub free_ram_mb: u64,
    /// Share of the CPU in use, across all cores.
    pub cpu_busy_pct: Option<u8>,
    pub on_battery: bool,
    /// Apple silicon: the GPU shares RAM, so RAM is the model's memory.
    pub unified: bool,
}

/// Room left above a bigger model before it is worth stepping up to.
pub const HEADROOM_MB: u64 = 2_048;
/// Share of free RAM a model may plan for (the OS and other apps keep the rest).
pub const RAM_SHARE_PCT: u64 = 80;
/// The CPU is free enough for Performance at or under this.
pub const CPU_FREE_PCT: u8 = 35;
/// A bigger model must still generate at least this fast to be worth it.
pub const PERFORMANCE_FLOOR_TOK_S: u32 = 12;
/// A reading older than this is gone (the sampler stopped).
pub const ROOM_STALE_MS: u64 = 30_000;

/// Memory a model may use once the resident model (`resident_mb`) is gone.
pub fn usable_mb(room: &Room, resident_mb: u64) -> u64 {
    room.free_vram_mb.unwrap_or(0) + room.free_ram_mb * RAM_SHARE_PCT / 100 + resident_mb
}

/// `m` fits with `headroom` to spare once the resident model is gone.
pub fn fits(m: &LocalModel, room: &Room, resident_mb: u64, headroom: u64) -> bool {
    m.run_mb + headroom <= usable_mb(room, resident_mb)
}

/// Rough generation speed of `m` with this room: all on the GPU, a mixture
/// of experts with its experts in RAM, or a dense split by the share of it on
/// the GPU.
pub fn est_tok_s(m: &LocalModel, room: &Room, resident_vram_mb: u64) -> u32 {
    let vram = room.free_vram_mb.unwrap_or(0) + resident_vram_mb;
    if vram == 0 {
        return if room.unified {
            m.tok_s.unified
        } else {
            m.tok_s.cpu
        };
    }
    if m.run_mb <= vram {
        return m.tok_s.gpu;
    }
    if m.badges.contains(&"moe") && vram * 3 >= m.run_mb {
        return m.tok_s.gpu * 2 / 3;
    }
    let on_gpu = vram * 100 / m.run_mb.max(1);
    let cpu = u64::from(m.tok_s.cpu);
    let gpu = u64::from(m.tok_s.gpu);
    u32::try_from(cpu + gpu.saturating_sub(cpu) * on_gpu / 100 / 2).unwrap_or(u32::MAX)
}

/// The machine is free enough for a step up: on AC and the CPU mostly idle.
pub fn performance_ready(room: &Room) -> bool {
    !room.on_battery && room.cpu_busy_pct.is_none_or(|c| c <= CPU_FREE_PCT)
}

/// A bigger model loaded for hard work should stop: the computer got busy,
/// went on battery, or no longer has room for it. With no reading it stays.
pub fn step_down_due(m: &LocalModel, scale: &Scale) -> bool {
    let Some(room) = scale.room else {
        return false;
    };
    scale.gaming || scale.saving || !performance_ready(&room) || !fits(m, &room, m.run_mb, 0)
}

static ROOM: Mutex<Option<(Room, u64)>> = Mutex::new(None);

/// The sampler's latest reading.
pub fn set_room(room: Room, now_ms: u64) {
    if let Ok(mut r) = ROOM.lock() {
        *r = Some((room, now_ms));
    }
}

/// The latest reading, unless it is older than [`ROOM_STALE_MS`].
pub fn room(now_ms: u64) -> Option<Room> {
    let r = ROOM.lock().ok()?;
    r.filter(|(_, at)| now_ms.saturating_sub(*at) <= ROOM_STALE_MS)
        .map(|(room, _)| room)
}

#[cfg(test)]
pub(crate) fn clear_room() {
    if let Ok(mut r) = ROOM.lock() {
        *r = None;
    }
}

/// What the governor says about the machine, for the swap planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scale {
    /// A game, a fullscreen app, a deny-list app or a busy GPU.
    pub gaming: bool,
    /// The governor throttles for another reason (battery, heat, power saver,
    /// low memory): lighter models for lighter tasks.
    pub saving: bool,
    pub room: Option<Room>,
}

/// The governor's reasons that mean another app wants the GPU.
pub fn gaming_reason(reason: &str) -> bool {
    grokhub_core::local_governor::game_reason(reason)
        || reason == "another app is keeping the GPU busy"
}

/// [`Scale`] from the governor's level and reason and the live room.
pub fn scale_for(
    level: grokhub_core::local_governor::Level,
    reason: &str,
    room: Option<Room>,
) -> Scale {
    use grokhub_core::local_governor::Level;
    let gaming = gaming_reason(reason);
    Scale {
        gaming,
        saving: !gaming && level == Level::Throttle,
        room,
    }
}

/// [`scale_for`] now.
pub fn scale_now(now_ms: u64) -> Scale {
    scale_for(
        super::local_gov::level(),
        super::local_gov::reason(),
        room(now_ms),
    )
}

#[cfg(test)]
#[path = "local_scale_tests.rs"]
mod tests;
