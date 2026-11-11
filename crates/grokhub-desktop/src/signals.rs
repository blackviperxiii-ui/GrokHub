//! What the PC says about itself for the wind-down reminder: how long it has
//! had no input, whether a game or a fullscreen app is on, and a way to put it
//! to sleep. A signal a reader can't see is `None`, and wind-down treats that
//! as busy (it fails closed). No window title or content is read.
//!
//! - Windows: `GetLastInputInfo`, the game guard's Toolhelp game check, and
//!   `SHQueryUserNotificationState` (Direct3D fullscreen or presentation mode).
//! - Linux: Mutter's idle monitor over `gdbus`, else `xprintidle`; games from
//!   `/proc` through the game guard. Fullscreen isn't read.
//! - macOS: `ioreg` idle time. Games and fullscreen aren't read.

#[cfg(unix)]
use std::process::{Command, Stdio};

/// Seconds since the last keyboard or mouse input.
pub fn idle_secs() -> Option<u64> {
    #[cfg(windows)]
    {
        win_idle_secs()
    }
    #[cfg(target_os = "macos")]
    {
        let out = run("ioreg", &["-c", "IOHIDSystem", "-d", "4"])?;
        grokhub_core::local_governor::parse_ioreg_idle_secs(&out)
    }
    #[cfg(target_os = "linux")]
    {
        let mutter = run(
            "gdbus",
            &[
                "call",
                "--session",
                "--dest",
                "org.gnome.Mutter.IdleMonitor",
                "--object-path",
                "/org/gnome/Mutter/IdleMonitor/Core",
                "--method",
                "org.gnome.Mutter.IdleMonitor.GetIdletime",
            ],
        )
        .and_then(|s| parse_idle_ms(&s));
        mutter
            .or_else(|| run("xprintidle", &[]).and_then(|s| parse_idle_ms(&s)))
            .map(|ms| ms / 1_000)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Whether a game is running, where this PC can tell.
pub fn game() -> Option<bool> {
    #[cfg(windows)]
    {
        crate::game_guard::windows_game()
    }
    #[cfg(target_os = "linux")]
    {
        Some(crate::game_guard::current().gaming)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        None
    }
}

/// Whether a fullscreen app owns the screen, where this PC can tell. On
/// Windows only Direct3D fullscreen and presentation mode count: plain "busy"
/// is also what a maximized window of this app reports.
pub fn fullscreen() -> Option<bool> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::SHQueryUserNotificationState;
        let mut state = 0;
        // SAFETY: an out-pointer to a local i32.
        let ok = unsafe { SHQueryUserNotificationState(&mut state) } >= 0;
        ok.then_some(matches!(state, 3 | 4))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Ask the OS to sleep. Windows sleeps rather than hibernates unless the
/// user's own power settings say otherwise.
pub fn sleep_pc() -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Power::SetSuspendState;
        // SAFETY: plain flags, no pointers.
        if !unsafe { SetSuspendState(false, false, false) } {
            return Err("Windows didn't sleep. Another app may be keeping it awake.".into());
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        run_ok("pmset", &["sleepnow"])
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        run_ok("systemctl", &["suspend"])
    }
}

#[cfg(unix)]
fn run_ok(cmd: &str, args: &[&str]) -> Result<(), String> {
    match Command::new(cmd).args(args).stdin(Stdio::null()).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(format!("{cmd} refused to sleep the PC.")),
        Err(e) => Err(format!("Couldn't run {cmd}: {e}")),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The milliseconds in `(uint64 4200,)` (gdbus) or `4200` (xprintidle).
#[cfg(any(target_os = "linux", test))]
fn parse_idle_ms(text: &str) -> Option<u64> {
    // gdbus replies "(uint64 4200,)": the type name carries digits too,
    // so the idle time is the last run of digits, not the first.
    text.split(|c: char| !c.is_ascii_digit())
        .rfind(|run| !run.is_empty())?
        .parse()
        .ok()
}

#[cfg(windows)]
fn win_idle_secs() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: an out-pointer to a local struct with cbSize set.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    // SAFETY: no arguments.
    let now = unsafe { GetTickCount() };
    Some(u64::from(now.wrapping_sub(info.dwTime)) / 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_idle_reply_parses_from_gdbus_and_xprintidle() {
        assert_eq!(parse_idle_ms("(uint64 4200,)\n"), Some(4200));
        assert_eq!(parse_idle_ms("125000\n"), Some(125_000));
        assert_eq!(parse_idle_ms("no idle monitor"), None);
        assert_eq!(parse_idle_ms(""), None);
    }
}
