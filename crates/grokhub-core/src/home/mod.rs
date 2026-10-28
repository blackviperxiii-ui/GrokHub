//! Modular Home / Mission Control catalog, slot grid, and layout schema.
//!
//! Persist (`home_layout.json`) lives in the cabin (`grokhub-app`); this crate
//! owns the pure registry, breakpoints, and serde shape so UI and tests share
//! one source of truth. Adding a module = one paint file + one registry line.

mod config;
mod layout;
mod registry;

pub use config::{HomeLayout, HomePreset, HOME_LAYOUT_FILE, HOME_LAYOUT_VERSION};
pub use layout::{
    place_slots, Breakpoint, ModuleSize, PlacedSlot, SlotId, GAP, MEDIUM_MIN_W, NARROW_L_AS_M,
    WIDE_MIN_W,
};
pub use registry::{
    lookup, modules_fitting, modules_for_category, Category, DataStatus, ModuleEntry, REGISTRY,
};
