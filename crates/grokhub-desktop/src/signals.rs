//! What the PC says about itself for the wind-down reminder: how long it has
//! had no input, whether a game or a fullscreen app is on, GPU busy by others,
//! and a way to put it to sleep. A signal a reader can't see is `None`, and
//! wind-down treats that as busy (it fails closed). No window title or content
//! is read.
//!
//! - Windows: `GetLastInputInfo`, the game guard's Toolhelp game check,
//!   `SHQueryUserNotificationState` (Direct3D fullscreen or presentation mode),
//!   and PDH `\GPU Engine(*)\Utilization Percentage` (this process left out).
//! - Linux: Mutter's idle monitor over `gdbus`, else `xprintidle`; games from
//!   `/proc` through the game guard; GPU from `nvidia-smi` or AMD
//!   `gpu_busy_percent`. Fullscreen isn't read.
//! - macOS: `ioreg` idle time. Games, fullscreen and GPU aren't read.

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

/// GPU busy by other processes, where this PC can tell. `own_pid` is left out
/// of the total when the reader can split per process.
pub fn external_gpu_pct(own_pid: Option<u32>) -> Option<u8> {
    #[cfg(target_os = "linux")]
    {
        linux_gpu(own_pid)
    }
    #[cfg(windows)]
    {
        windows_gpu(own_pid)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = own_pid;
        None
    }
}

/// NVIDIA through `nvidia-smi` (own share from `pmon`), else AMD sysfs.
#[cfg(target_os = "linux")]
fn linux_gpu(own: Option<u32>) -> Option<u8> {
    let util = run(
        "nvidia-smi",
        &[
            "--query-gpu=utilization.gpu",
            "--format=csv,noheader,nounits",
        ],
    );
    if let Some(util) = util.as_deref() {
        let pmon = match own {
            Some(_)
                if grokhub_core::local_governor::parse_nvidia_util(util)
                    .is_some_and(|t| t > grokhub_core::local_governor::GPU_BUSY_PCT) =>
            {
                run("nvidia-smi", &["pmon", "-c", "1", "-s", "u"])
            }
            _ => None,
        };
        return grokhub_core::wind_down::external_gpu_from_nvidia(util, pmon.as_deref(), own);
    }
    let texts: Vec<String> = std::fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("device/gpu_busy_percent")).ok())
        .collect();
    grokhub_core::wind_down::external_gpu_from_amd_busy(texts.iter().map(String::as_str))
}

/// PDH `\GPU Engine(*)\Utilization Percentage`, kept warm on this thread
/// (handles aren't `Send`).
#[cfg(windows)]
fn windows_gpu(own: Option<u32>) -> Option<u8> {
    use std::cell::RefCell;
    thread_local! {
        static PDH: RefCell<Option<Pdh>> = const { RefCell::new(None) };
    }
    PDH.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Pdh::open();
        }
        slot.as_ref().and_then(|p| p.sample(own))
    })
}

/// A PDH query on `\GPU Engine(*)\Utilization Percentage`.
#[cfg(windows)]
struct Pdh {
    query: windows_sys::Win32::System::Performance::PDH_HQUERY,
    counter: windows_sys::Win32::System::Performance::PDH_HCOUNTER,
}

#[cfg(windows)]
impl Pdh {
    fn open() -> Option<Self> {
        use windows_sys::Win32::System::Performance::{
            PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW,
        };
        let path: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage\0"
            .encode_utf16()
            .collect();
        // SAFETY: out-pointers to locals; the query is closed on failure or in Drop.
        unsafe {
            let mut query = std::ptr::null_mut();
            if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                return None;
            }
            let mut counter = std::ptr::null_mut();
            if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != 0 {
                PdhCloseQuery(query);
                return None;
            }
            // A rate counter needs a first collection to compare against.
            PdhCollectQueryData(query);
            Some(Pdh { query, counter })
        }
    }

    fn sample(&self, own: Option<u32>) -> Option<u8> {
        use windows_sys::Win32::System::Performance::{
            PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_FMT_COUNTERVALUE_ITEM_W,
            PDH_FMT_DOUBLE, PDH_MORE_DATA,
        };
        // SAFETY: the first call sizes the buffer, the second fills it; every
        // item's name points into that buffer, which outlives the loop.
        // No data or a failed read is `None`, never an idle GPU.
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            let sized = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                std::ptr::null_mut(),
            );
            if sized != PDH_MORE_DATA || size == 0 {
                return None;
            }
            let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> =
                Vec::with_capacity((size as usize).div_ceil(item));
            if PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                buf.as_mut_ptr(),
            ) != 0
            {
                return None;
            }
            buf.set_len(count as usize);
            let rows: Vec<(String, f64)> = buf
                .iter()
                .map(|it| {
                    let mut n = 0;
                    while *it.szName.add(n) != 0 {
                        n += 1;
                    }
                    let name = String::from_utf16_lossy(std::slice::from_raw_parts(it.szName, n));
                    (name, it.FmtValue.Anonymous.doubleValue)
                })
                .collect();
            grokhub_core::wind_down::external_gpu_from_pdh(&rows, own)
        }
    }
}

#[cfg(windows)]
impl Drop for Pdh {
    fn drop(&mut self) {
        // SAFETY: the query came from PdhOpenQueryW and is closed once.
        unsafe {
            windows_sys::Win32::System::Performance::PdhCloseQuery(self.query);
        }
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
