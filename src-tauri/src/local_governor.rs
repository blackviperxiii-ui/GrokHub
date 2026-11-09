//! The live governor in the shell: the shared sampler
//! (`grokhub_agent::route::local_sampler`) with the shell's game reader, so
//! a game unloads the on-device model here too (within one 2 s sample) and
//! the idle timer, scaling and restarts run without a request to wake them.

use std::path::PathBuf;

/// Start the sampler once (desktop builds).
pub fn start(config_dir: PathBuf) {
    grokhub_agent::route::local_sampler::start(config_dir, game);
}

/// A game runs: Steam's running game, a known game exe or launcher, a
/// deny-list app, or exclusive fullscreen. Reads process names and Steam's
/// own files only, so no anti-cheat sees it. `None` where there's no reader.
fn game() -> Option<bool> {
    #[cfg(any(target_os = "linux", windows))]
    {
        Some(grokhub_desktop::game_guard::current().gaming)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}
