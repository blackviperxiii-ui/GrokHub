//! Auto setup: the app reads the machine and the person, sets every setting
//! it can on its own, and keeps tuning as it learns. This crate holds the
//! pure parts both apps share; the readers live in `grokhub_agent::device`.

pub mod chat;
pub mod device;
pub mod os_signals;
pub mod plan;
pub mod questions;
pub mod rules;
pub mod settings;
pub mod store;

pub use device::{DeviceProfile, Display, FormFactor, GpuFact, OsLook, Tier};
