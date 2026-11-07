//! This machine's hardware readers now live in `grokhub_agent::device`, so
//! the desktop app reads the same profile. The cabin keeps these names.

pub(super) use grokhub_agent::device::{
    free_ram_now, gpu_temp_c, nvidia_free_vram_now, read_profile,
};
#[cfg(target_os = "linux")]
pub(super) use grokhub_agent::device::{power_now, thermal_now};
