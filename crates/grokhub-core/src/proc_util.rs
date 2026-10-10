//! Small child-process helpers: hide Windows consoles, stop a process tree,
//! read a SIGTERM exit.

use std::process::Command;

/// Hide a Windows console for spawned CLI tools (`grok.exe`, powershell).
///
/// `grokhub.exe` is `windows_subsystem = "windows"`. Spawning a console-subsystem
/// binary without `CREATE_NO_WINDOW` allocates a visible terminal. Closing that
/// window kills the child with `STATUS_CONTROL_C_EXIT`.
///
/// Also silences the loader MessageBox (missing DLL / bad image) so a broken
/// `grok.exe` becomes one cabin error instead of a looping system dialog.
pub fn hide_windows_console(cmd: &mut Command) {
    silence_windows_hard_errors();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

/// Process-wide: do not show Windows critical-error / missing-DLL dialogs.
/// Children inherit this. Safe to call from any thread, including Linux (no-op).
pub fn silence_windows_hard_errors() {
    #[cfg(windows)]
    {
        const SEM_FAILCRITICALERRORS: u32 = 0x0001;
        const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
        const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
        const MODE: u32 = SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX;
        #[link(name = "kernel32")]
        extern "system" {
            fn SetErrorMode(u_mode: u32) -> u32;
            fn SetThreadErrorMode(dw_new_mode: u32, lp_old_mode: *mut u32) -> i32;
        }
        unsafe {
            SetErrorMode(MODE);
            let mut old = 0u32;
            let _ = SetThreadErrorMode(MODE, &mut old);
        }
    }
}

/// SIGTERM (128+15). The GUI/leader kills `grok agent stdio` this way.
pub fn is_sigterm_status(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("exit 143")
        || l.contains("signal 15")
        || l.contains("sigterm")
        || (l.contains("agent closed") && l.contains("143"))
}

/// Stop a headless `grok -p` run. Windows has no `kill`, so Stop used to leave grok running there.
pub fn kill_pid(pid: u32) {
    #[cfg(windows)]
    {
        let mut kill = Command::new("taskkill");
        kill.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        hide_windows_console(&mut kill);
        let _ = kill.status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
        std::thread::sleep(std::time::Duration::from_millis(80));
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_pid_stops_a_running_child() {
        #[cfg(windows)]
        let mut child = Command::new("ping")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn ping");
        #[cfg(not(windows))]
        let mut child = Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn sleep");
        kill_pid(child.id());
        // `taskkill` can take a few seconds to start on a loaded Windows runner.
        let start = std::time::Instant::now();
        while child.try_wait().expect("try_wait").is_none() {
            if start.elapsed() > std::time::Duration::from_secs(15) {
                let _ = child.kill();
                panic!("kill_pid left the child running");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let src = include_str!("proc_util.rs");
        let body = src
            .split("pub fn kill_pid(")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("kill_pid");
        assert!(
            body.contains("taskkill")
                && body.contains("/T")
                && body.contains("hide_windows_console"),
            "Windows has no `kill`; Stop must taskkill the grok tree: {body}"
        );
    }
}
