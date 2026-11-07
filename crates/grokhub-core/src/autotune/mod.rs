//! Auto setup: the app reads the machine and the person, sets every setting
//! it can on its own, and keeps tuning as it learns. This crate holds the
//! pure parts both apps share; the readers live in `grokhub_agent::device`.

pub mod device;
pub mod os_signals;

pub use device::{DeviceProfile, Display, FormFactor, GpuFact, OsLook, Tier};
