//! Small child-process helpers: hide Windows consoles, read a SIGTERM exit.

use std::process::Command;

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
    #[test]
    fn windows_hard_errors_are_silenced_with_set_error_mode() {
        let src = include_str!("proc_util.rs");
        let body = src
            .split(concat!("pub fn silence_windows_", "hard_errors"))
            .nth(1)
            .and_then(|s| s.split("\n}").next())
            .expect("silence_windows_hard_errors");
        assert!(body.contains(concat!("SetError", "Mode(MODE)")), "{body}");
        assert!(body.contains(concat!("SEM_FAILCRITICAL", "ERRORS | ")), "{body}");
    }
}
