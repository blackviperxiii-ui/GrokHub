//! Modular Home / Mission Control catalog, slot grid, and layout schema.
//!
//! Persist (`home_layout.json`) lives in the cabin (`grokhub-app`); this crate
//! owns the pure registry, breakpoints, and serde shape so UI and tests share
//! one source of truth. Adding a module = one paint file + one registry line.

mod config;
mod layout;
mod registry;
pub mod text;
pub mod tiles;
pub mod updates;
mod view;

pub use config::{
    HomeLayout, HomePick, HomePreset, HOME_LAYOUT_FILE, HOME_LAYOUT_VERSION, HOME_PRESETS,
};
pub use layout::{
    place_slots, Breakpoint, ModuleSize, PlacedSlot, SlotId, GAP, MEDIUM_MIN_W, NARROW_L_AS_M,
    WIDE_MIN_W,
};
pub use registry::{
    lookup, modules_fitting, modules_for_category, Category, DataStatus, ModuleEntry, REGISTRY,
};
pub use view::{
    fit, is_stale, refresh_secs, Fit, TileAction, TileButton, TileRow, TileState, TileText,
    TileView, Tone,
};

/// The PC tile's Tune-up button.
pub const TUNE_UP_ASK: &str = "Check this PC and suggest a tune-up.";
/// The storage tile's button.
pub const FREE_STORAGE_ASK: &str = "Free up storage on this PC.";
/// The chips under the Home composer.
pub const HOME_CHIPS: [&str; 4] = [
    "Fix my stutter",
    "Find a duo tonight",
    "Free up storage",
    "Plan my week",
];

/// Every fixed ask a Home button or chip sends to a new chat. Each one must
/// sort by the rules to work (never a guess), so a press never lands on
/// small talk; `grokhub-agent`'s Brief tests check that.
pub const BUTTON_ASKS: [&str; 6] = [
    TUNE_UP_ASK,
    FREE_STORAGE_ASK,
    HOME_CHIPS[0],
    HOME_CHIPS[1],
    HOME_CHIPS[2],
    HOME_CHIPS[3],
];
