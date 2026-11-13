//! The frame-rate probe on its own: counts the frames each screen shows from
//! the kernel's DRM flip tracepoint, with nothing but libc.
//!
//! It is its own tiny binary so the one-time setup can give it CAP_PERFMON
//! as a file capability and it still starts from any install. A file with
//! capabilities runs in secure-exec mode, which ignores `LD_LIBRARY_PATH`, so
//! a copy of the whole app (an AppImage's inner binary needs its bundled
//! libraries) may not start at all; this one needs only the C library.
//!
//! The probe opens the counters, drops every capability, then answers each
//! line on stdin with the counts, one number per CRTC, until stdin closes.

/// The argument that runs a binary as the counting probe and nothing else.
pub const PROBE_ARG: &str = "--fps-probe";

/// CRTCs counted. amdgpu has at most 6, i915 4.
pub const MAX_CRTCS: u32 = 6;

#[cfg(target_os = "linux")]
const TRACEPOINT_IDS: [&str; 2] = [
    "/sys/kernel/tracing/events/drm/drm_vblank_event_delivered/id",
    "/sys/kernel/debug/tracing/events/drm/drm_vblank_event_delivered/id",
];

/// Why the counters wouldn't open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    /// No DRM flip tracepoint (no DRM driver, or tracing off).
    NoTracepoint,
    /// The tracepoint id or the perf counters need the one-time setup.
    NeedsPermission,
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

/// Counts as the probe writes them: one number per CRTC, space separated.
pub fn parse_counts(line: &str) -> Option<Vec<u64>> {
    let out: Option<Vec<u64>> = line.split_whitespace().map(|n| n.parse().ok()).collect();
    out.filter(|v| !v.is_empty())
}

/// Open counters for each CRTC on each CPU, read as one count per CRTC.
#[cfg(target_os = "linux")]
pub struct Counters {
    fds: Vec<(usize, std::os::fd::OwnedFd)>,
    crtcs: usize,
}

#[cfg(target_os = "linux")]
impl Counters {
    /// Opens the counters when this process may (the system allows it, or
    /// this file carries CAP_PERFMON).
    pub fn open() -> Result<Self, Block> {
        linux::open()
    }

    pub fn counts(&self) -> Vec<u64> {
        let mut out = vec![0u64; self.crtcs];
        for (crtc, fd) in &self.fds {
            out[*crtc] += linux::read_count(fd);
        }
        out
    }
}

/// The body of `--fps-probe`: open the counters, then answer each line on
/// stdin with the counts, until stdin closes. Exit code 2 when the counters
/// won't open.
pub fn run_probe() -> i32 {
    #[cfg(target_os = "linux")]
    {
        use std::io::{BufRead, Write};
        let Ok(counters) = Counters::open() else {
            return 2;
        };
        // The counters stay readable once open; nothing after this needs
        // CAP_PERFMON, so the probe answers the pipe with no privilege.
        drop_privileges();
        let stdin = std::io::stdin();
        let mut out = std::io::stdout().lock();
        for line in stdin.lock().lines() {
            if line.is_err() {
                break;
            }
            let text: Vec<String> = counters.counts().iter().map(u64::to_string).collect();
            if writeln!(out, "{}", text.join(" ")).is_err() || out.flush().is_err() {
                break;
            }
        }
        0
    }
    #[cfg(not(target_os = "linux"))]
    {
        2
    }
}

/// Drop every Linux capability this process holds, unless it runs as root.
///
/// A probe file carries CAP_PERFMON, so whatever runs it gets that
/// capability at exec. Apps that can be run as the probe call this first
/// thing on every other path, and the probe calls it as soon as its counters
/// are open. If the drop fails while capabilities are held, the process
/// exits rather than go on.
pub fn drop_privileges() {
    #[cfg(target_os = "linux")]
    caps::drop_all();
}

#[cfg(target_os = "linux")]
mod caps {
    const VERSION_3: u32 = 0x2008_0522;

    #[repr(C)]
    struct Header {
        version: u32,
        pid: libc::c_int,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }

    fn get() -> Option<[Data; 2]> {
        let mut header = Header {
            version: VERSION_3,
            pid: 0,
        };
        let mut data = [Data::default(); 2];
        // SAFETY: capget writes two Data entries for version 3.
        let rc = unsafe { libc::syscall(libc::SYS_capget, &mut header, data.as_mut_ptr()) };
        (rc == 0).then_some(data)
    }

    /// Whether this process holds any capability at all.
    pub(super) fn held() -> bool {
        get().is_none_or(|d| {
            d.iter()
                .any(|e| e.effective | e.permitted | e.inheritable != 0)
        })
    }

    pub(super) fn drop_all() {
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 || !held() {
            return;
        }
        let mut header = Header {
            version: VERSION_3,
            pid: 0,
        };
        let data = [Data::default(); 2];
        // SAFETY: capset reads two Data entries for version 3; prctl takes
        // plain integer arguments.
        let ok = unsafe {
            libc::prctl(
                libc::PR_CAP_AMBIENT,
                libc::PR_CAP_AMBIENT_CLEAR_ALL,
                0,
                0,
                0,
            );
            libc::syscall(libc::SYS_capset, &mut header, data.as_ptr()) == 0
        };
        if !ok || held() {
            std::process::exit(3);
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn dropping_leaves_no_capability() {
            super::drop_all();
            // SAFETY: geteuid has no preconditions.
            if unsafe { libc::geteuid() } != 0 {
                assert!(!super::held());
            }
        }
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

    pub(super) fn open() -> Result<Counters, Block> {
        let id = tracepoint_id()?;
        let cpus = std::fs::read_to_string("/sys/devices/system/cpu/online")
            .map(|t| parse_cpu_list(&t))
            .unwrap_or_default();
        if cpus.is_empty() {
            return Err(Block::NoTracepoint);
        }
        let mut fds = Vec::new();
        let mut filtered = true;
        'crtc: for crtc in 0..MAX_CRTCS {
            for &cpu in &cpus {
                let fd = perf_open(id, cpu)?;
                let filter = std::ffi::CString::new(format!("crtc == {crtc}"))
                    .map_err(|_| Block::NoTracepoint)?;
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
        Ok(Counters {
            fds,
            crtcs: if filtered { MAX_CRTCS as usize } else { 1 },
        })
    }

    fn tracepoint_id() -> Result<u64, Block> {
        let mut denied = false;
        for path in TRACEPOINT_IDS {
            match std::fs::read_to_string(path) {
                Ok(text) => return parse_id(&text).ok_or(Block::NoTracepoint),
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied = true,
                Err(_) => {}
            }
        }
        // A locked tracefs looks like a missing file to a normal user.
        let tracefs_locked = std::fs::read_dir("/sys/kernel/tracing").is_err();
        if denied || tracefs_locked {
            Err(Block::NeedsPermission)
        } else {
            Err(Block::NoTracepoint)
        }
    }

    fn perf_open(id: u64, cpu: u32) -> Result<OwnedFd, Block> {
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
                Some(libc::EACCES) | Some(libc::EPERM) => Block::NeedsPermission,
                _ => Block::NoTracepoint,
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
    fn cpu_lists_and_ids_parse() {
        assert_eq!(parse_cpu_list("0-3,6,8-9\n"), vec![0, 1, 2, 3, 6, 8, 9]);
        assert_eq!(parse_cpu_list(""), Vec::<u32>::new());
        assert_eq!(parse_id("1234\n"), Some(1234));
        assert_eq!(parse_id("x"), None);
    }

    #[test]
    fn probe_lines_parse() {
        assert_eq!(parse_counts("1 0 240\n"), Some(vec![1, 0, 240]));
        assert_eq!(parse_counts(""), None);
        assert_eq!(parse_counts("1 x"), None);
    }
}
