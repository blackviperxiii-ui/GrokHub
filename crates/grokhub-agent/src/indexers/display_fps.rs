//! Frames the screen shows, counted by the kernel. Each time the compositor
//! (or a fullscreen game in direct scanout) puts a new frame on a display, the
//! DRM core delivers a flip event and fires the `drm:drm_vblank_event_delivered`
//! tracepoint. A perf counter on that tracepoint, filtered per CRTC, counts
//! those events from inside the kernel: nothing touches the game process, so
//! anti-cheat has nothing to see.
//!
//! Counting mode only (no samples, no ring buffer). It needs system-wide perf
//! counters (`kernel.perf_event_paranoid` at 0 or lower, or CAP_PERFMON) and a
//! readable tracepoint id under tracefs, which [`LINUX_SETUP`] grants once.

use std::time::Instant;

/// CRTCs counted. amdgpu has at most 6, i915 4.
pub const MAX_CRTCS: u32 = 6;

const TRACEPOINT_IDS: [&str; 2] = [
    "/sys/kernel/tracing/events/drm/drm_vblank_event_delivered/id",
    "/sys/kernel/debug/tracing/events/drm/drm_vblank_event_delivered/id",
];

/// The one-time permission, as commands the user pastes into a terminal.
/// Loadout never runs them itself. Works in bash, zsh and fish.
pub const LINUX_SETUP: &str = "printf 'kernel.perf_event_paranoid = 0\\n' | sudo tee /etc/sysctl.d/60-loadout-fps.conf\n\
printf 'z /sys/kernel/tracing 0755 - - -\\nz /sys/kernel/tracing/events 0755 - - -\\nz /sys/kernel/tracing/events/drm 0755 - - -\\nz /sys/kernel/tracing/events/drm/drm_vblank_event_delivered 0755 - - -\\nz /sys/kernel/tracing/events/drm/drm_vblank_event_delivered/id 0444 - - -\\n' | sudo tee /etc/tmpfiles.d/loadout-fps.conf\n\
sudo sysctl -p /etc/sysctl.d/60-loadout-fps.conf && sudo systemd-tmpfiles --create /etc/tmpfiles.d/loadout-fps.conf";

/// Why the counter couldn't open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FpsBlock {
    /// Not Linux.
    Unsupported,
    /// The kernel has no DRM flip tracepoint (no DRM driver, or tracing off).
    NoTracepoint,
    /// The tracepoint id or the perf counters need the one-time permission.
    NeedsPermission,
}

/// Frames per second of the busiest CRTC between two reads of the counters,
/// or `None` when no time passed or the counts went backwards.
pub fn busiest_rate(prev: &[u64], now: &[u64], secs: f64) -> Option<f32> {
    if secs <= 0.0 || prev.len() != now.len() {
        return None;
    }
    let mut best: Option<u64> = None;
    for (a, b) in prev.iter().zip(now) {
        let d = b.checked_sub(*a)?;
        best = Some(best.map_or(d, |x| x.max(d)));
    }
    best.map(|d| (d as f64 / secs) as f32)
}

/// The tracepoint id from tracefs text.
pub fn parse_id(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// CPUs in a `/sys/devices/system/cpu/online` list such as `0-7,10,12-13`.
pub fn parse_cpu_list(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in text.trim().split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                    out.extend(a..=b.min(a + 4096));
                }
            }
            None => {
                if let Ok(c) = part.parse() {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Open counters for each CRTC on each CPU, read as one count per CRTC.
pub struct FlipCounter {
    #[cfg(target_os = "linux")]
    fds: Vec<(usize, std::os::fd::OwnedFd)>,
    crtcs: usize,
    last: Option<(Instant, Vec<u64>)>,
}

impl FlipCounter {
    pub fn open() -> Result<Self, FpsBlock> {
        #[cfg(target_os = "linux")]
        {
            linux::open()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(FpsBlock::Unsupported)
        }
    }

    /// The busiest screen's frames per second since the last call. The first
    /// call only sets the baseline.
    pub fn sample(&mut self) -> Option<f32> {
        let now = Instant::now();
        let counts = self.counts();
        let rate = self.last.as_ref().and_then(|(at, prev)| {
            busiest_rate(prev, &counts, now.duration_since(*at).as_secs_f64())
        });
        self.last = Some((now, counts));
        rate
    }

    fn counts(&self) -> Vec<u64> {
        #[allow(unused_mut)]
        let mut out = vec![0u64; self.crtcs];
        #[cfg(target_os = "linux")]
        for (crtc, fd) in &self.fds {
            out[*crtc] += linux::read_count(fd);
        }
        out
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    use super::*;

    const PERF_TYPE_TRACEPOINT: u32 = 2;
    const PERF_FLAG_FD_CLOEXEC: libc::c_ulong = 8;
    /// `_IOW('$', 6, char *)` on 64-bit targets.
    const PERF_EVENT_IOC_SET_FILTER: libc::c_ulong = 0x4008_2406;

    /// `struct perf_event_attr` up to `PERF_ATTR_SIZE_VER0` (64 bytes).
    #[repr(C)]
    #[derive(Default)]
    struct PerfEventAttr {
        kind: u32,
        size: u32,
        config: u64,
        sample_period: u64,
        sample_type: u64,
        read_format: u64,
        flags: u64,
        wakeup_events: u32,
        bp_type: u32,
        config1: u64,
    }

    pub(super) fn open() -> Result<FlipCounter, FpsBlock> {
        let id = tracepoint_id()?;
        let cpus = std::fs::read_to_string("/sys/devices/system/cpu/online")
            .map(|t| parse_cpu_list(&t))
            .unwrap_or_default();
        if cpus.is_empty() {
            return Err(FpsBlock::NoTracepoint);
        }
        let mut fds = Vec::new();
        let mut filtered = true;
        'crtc: for crtc in 0..MAX_CRTCS {
            for &cpu in &cpus {
                let fd = perf_open(id, cpu)?;
                let filter = std::ffi::CString::new(format!("crtc == {crtc}"))
                    .map_err(|_| FpsBlock::NoTracepoint)?;
                // SAFETY: `fd` is an open perf event and `filter` a valid C string
                // that outlives the call.
                let set = unsafe {
                    libc::ioctl(
                        fd.as_raw_fd(),
                        PERF_EVENT_IOC_SET_FILTER as _,
                        filter.as_ptr(),
                    )
                };
                if set != 0 {
                    // No event filters in this kernel: count every screen together.
                    filtered = false;
                    fds.clear();
                    break 'crtc;
                }
                fds.push((crtc as usize, fd));
            }
        }
        if !filtered {
            for &cpu in &cpus {
                fds.push((0, perf_open(id, cpu)?));
            }
        }
        Ok(FlipCounter {
            fds,
            crtcs: if filtered { MAX_CRTCS as usize } else { 1 },
            last: None,
        })
    }

    fn tracepoint_id() -> Result<u64, FpsBlock> {
        let mut denied = false;
        for path in TRACEPOINT_IDS {
            match std::fs::read_to_string(path) {
                Ok(text) => return parse_id(&text).ok_or(FpsBlock::NoTracepoint),
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied = true,
                Err(_) => {}
            }
        }
        // A locked tracefs looks like a missing file to a normal user.
        let tracefs_locked = std::fs::read_dir("/sys/kernel/tracing").is_err();
        if denied || tracefs_locked {
            Err(FpsBlock::NeedsPermission)
        } else {
            Err(FpsBlock::NoTracepoint)
        }
    }

    fn perf_open(id: u64, cpu: u32) -> Result<OwnedFd, FpsBlock> {
        let attr = PerfEventAttr {
            kind: PERF_TYPE_TRACEPOINT,
            size: std::mem::size_of::<PerfEventAttr>() as u32,
            config: id,
            ..PerfEventAttr::default()
        };
        // SAFETY: `attr` is a valid perf_event_attr of the size it declares; pid -1
        // with a CPU counts that CPU system-wide; group_fd -1 starts no group.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_perf_event_open,
                &attr as *const PerfEventAttr,
                -1 as libc::pid_t,
                cpu as libc::c_int,
                -1 as libc::c_int,
                PERF_FLAG_FD_CLOEXEC,
            )
        };
        if fd < 0 {
            let err = std::io::Error::last_os_error().raw_os_error();
            return Err(match err {
                Some(libc::EACCES) | Some(libc::EPERM) => FpsBlock::NeedsPermission,
                _ => FpsBlock::NoTracepoint,
            });
        }
        // SAFETY: the syscall returned a new file descriptor that we now own.
        Ok(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) })
    }

    pub(super) fn read_count(fd: &OwnedFd) -> u64 {
        let mut n = 0u64;
        // SAFETY: reading 8 bytes into a u64 from a counting perf event.
        let got = unsafe { libc::read(fd.as_raw_fd(), &mut n as *mut u64 as *mut libc::c_void, 8) };
        if got == 8 {
            n
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_busiest_screen_sets_the_rate() {
        assert_eq!(busiest_rate(&[0, 100], &[60, 244], 1.0), Some(144.0));
        assert_eq!(busiest_rate(&[10], &[70], 0.5), Some(120.0));
        assert_eq!(busiest_rate(&[10], &[5], 1.0), None);
        assert_eq!(busiest_rate(&[1], &[2], 0.0), None);
    }

    #[test]
    fn cpu_lists_and_ids_parse() {
        assert_eq!(parse_cpu_list("0-3,6,8-9\n"), vec![0, 1, 2, 3, 6, 8, 9]);
        assert_eq!(parse_cpu_list(""), Vec::<u32>::new());
        assert_eq!(parse_id("1234\n"), Some(1234));
        assert_eq!(parse_id("x"), None);
    }

    #[test]
    fn setup_only_opens_the_one_tracepoint_id() {
        assert!(LINUX_SETUP.contains("perf_event_paranoid = 0"));
        assert!(LINUX_SETUP.contains("drm_vblank_event_delivered/id 0444"));
        assert!(!LINUX_SETUP.contains("chmod -R"));
    }
}
