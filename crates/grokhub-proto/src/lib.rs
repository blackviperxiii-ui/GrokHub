//! Hub wire types and pure helpers. No I/O, no platform deps, so this crate
//! builds for every cabin target, `wasm32-unknown-unknown` included.
//! `grokhub-core` re-exports these modules at their old paths.

#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod hub_sync;
pub mod inhabit;

/// What `--doctor` and paired computers probe. Changing it is a protocol break.
pub const HUB_KIND: &str = "grokhub-hub-v1";
