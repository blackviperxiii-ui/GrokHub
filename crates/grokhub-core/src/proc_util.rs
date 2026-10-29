//! Small child-process helpers: hide Windows consoles, read a SIGTERM exit,
//! run a probe with a deadline.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Hide a Windows console for spawned console tools (powershell, MCP servers).
///
/// `grokhub.exe` is `windows_subsystem = "windows"`. Spawning a console-subsystem
/// binary without `CREATE_NO_WINDOW` allocates a visible terminal. Closing that
/// window kills the child with `STATUS_CONTROL_C_EXIT`.
///
/// Also silences the loader MessageBox (missing DLL / bad image) so a broken
/// child becomes one cabin error instead of a looping system dialog.
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

/// Run `cmd` and return its stdout if it exits successfully within
/// `timeout`; otherwise kill it and return `None`. Keeps at most `cap` bytes.
/// stdout is drained on a side thread so a full pipe can't stall the child.
pub fn output_within(cmd: &mut Command, timeout: Duration, cap: u64) -> Option<String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_windows_console(cmd);
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(cap).read_to_end(&mut buf);
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        buf
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(15));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let buf = reader.join().ok()?;
    status
        .success()
        .then(|| String::from_utf8_lossy(&buf).into_owned())
}

/// SIGTERM (128+15). The GUI/leader kills `grok agent stdio` this way.
pub fn is_sigterm_status(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("exit 143")
        || l.contains("signal 15")
        || l.contains("sigterm")
        || (l.contains("agent closed") && l.contains("143"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        #[cfg(windows)]
        let mut c = {
            let mut c = Command::new("cmd");
            c.args(["/C", script]);
            c
        };
        #[cfg(not(windows))]
        let mut c = {
            let mut c = Command::new("sh");
            c.args(["-c", script]);
            c
        };
        c.current_dir(std::env::temp_dir());
        c
    }

    #[test]
    fn output_within_returns_stdout_of_a_quick_command() {
        let out = output_within(&mut shell("echo probe-ok"), Duration::from_secs(10), 64);
        assert_eq!(out.as_deref().map(str::trim), Some("probe-ok"));
        assert_eq!(
            output_within(&mut shell("exit 3"), Duration::from_secs(10), 64),
            None
        );
    }

    #[test]
    fn output_within_kills_a_stalled_command() {
        #[cfg(windows)]
        let stall = "ping -n 30 127.0.0.1 >NUL";
        #[cfg(not(windows))]
        let stall = "sleep 30";
        let start = Instant::now();
        let out = output_within(&mut shell(stall), Duration::from_millis(300), 64);
        assert_eq!(out, None);
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{:?}",
            start.elapsed()
        );
    }
    #[test]
    fn windows_hard_errors_are_silenced_with_set_error_mode() {
        let src = include_str!("proc_util.rs");
        let body = src
            .split(concat!("pub fn silence_windows_", "hard_errors"))
            .nth(1)
            .and_then(|s| s.split("\n}").next())
            .expect("silence_windows_hard_errors");
        assert!(body.contains(concat!("SetError", "Mode(MODE)")), "{body}");
        assert!(
            body.contains(concat!("SEM_FAILCRITICAL", "ERRORS | ")),
            "{body}"
        );
    }
}
