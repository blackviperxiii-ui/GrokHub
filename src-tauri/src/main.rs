// No console window behind the shell in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The frame-rate setup can install this binary as the probe (when no
    // bundled probe is found), with CAP_PERFMON on the file. A bare
    // `--fps-probe` only counts flips; every other path drops whatever
    // capabilities the exec gave first, so the shell never runs privileged.
    let mut argv = std::env::args().skip(1);
    if argv.next().as_deref() == Some(grokhub_fps_probe::PROBE_ARG) && argv.next().is_none() {
        std::process::exit(grokhub_fps_probe::run_probe());
    }
    grokhub_fps_probe::drop_privileges();
    grokhub_shell::run()
}
