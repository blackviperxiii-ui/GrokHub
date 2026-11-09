//! `loadout-fps-probe --fps-probe`: the frame-rate probe Home's Frame Rate
//! setup installs with CAP_PERFMON. Any other way in drops every capability
//! and exits, so the file is useless for anything but counting flips.

fn main() {
    let mut argv = std::env::args().skip(1);
    if argv.next().as_deref() == Some(grokhub_fps_probe::PROBE_ARG) && argv.next().is_none() {
        std::process::exit(grokhub_fps_probe::run_probe());
    }
    grokhub_fps_probe::drop_privileges();
    eprintln!("usage: loadout-fps-probe {}", grokhub_fps_probe::PROBE_ARG);
    std::process::exit(64);
}
