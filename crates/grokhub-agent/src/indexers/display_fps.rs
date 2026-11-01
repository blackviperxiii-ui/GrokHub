//! Frames the screen shows, counted by the kernel. Each time the compositor
//! (or a fullscreen game in direct scanout) puts a new frame on a display, the
//! DRM core delivers a flip event and fires the `drm:drm_vblank_event_delivered`
//! tracepoint. A perf counter on that tracepoint, filtered per CRTC, counts
//! those events from inside the kernel: nothing touches the game process, so
//! anti-cheat has nothing to see.
//!
//! Counting mode only (no samples, no ring buffer). It needs system-wide perf
//! counters and a readable tracepoint id under tracefs. The system-wide perf
//! setting (`kernel.perf_event_paranoid`) is never lowered: that would let any
//! local process watch every CPU. Instead [`linux_setup`] copies Loadout to a
//! probe that only this user's group can run and gives that one file
//! CAP_PERFMON. The app runs it as `--fps-probe`, asks it for the counts once
//! a second over a pipe, and closing the pipe ends it.

use std::time::Instant;

/// CRTCs counted. amdgpu has at most 6, i915 4.
pub const MAX_CRTCS: u32 = 6;

#[cfg(target_os = "linux")]
const TRACEPOINT_IDS: [&str; 2] = [
    "/sys/kernel/tracing/events/drm/drm_vblank_event_delivered/id",
    "/sys/kernel/debug/tracing/events/drm/drm_vblank_event_delivered/id",
];

/// The argument that runs the binary as the counting probe and nothing else.
pub const PROBE_ARG: &str = "--fps-probe";

/// Where [`linux_setup`] puts the probe.
pub const PROBE_PATH: &str = "/usr/local/lib/loadout/fps-probe";

/// The setup, as commands the user pastes into a terminal: a copy of `exe` as
/// the probe (root-owned, runnable only by the user's group) with CAP_PERFMON
/// on that file alone, and tracefs opened just far enough to read the one
/// tracepoint id (folders traversable, not listable). Re-running it after an
/// update refreshes the probe.
pub fn linux_setup(exe: &str) -> String {
    let dirs = [
        "/sys/kernel/tracing",
        "/sys/kernel/tracing/events",
        "/sys/kernel/tracing/events/drm",
        "/sys/kernel/tracing/events/drm/drm_vblank_event_delivered",
    ];
    let mut tmpfiles: String = dirs
        .iter()
        .map(|d| format!("z {d} 0711 - - -\\n"))
        .collect();
    tmpfiles
        .push_str("z /sys/kernel/tracing/events/drm/drm_vblank_event_delivered/id 0444 - - -\\n");
    format!(
        "sudo install -D -m 0750 -o root -g \"$(id -gn)\" {exe} {PROBE_PATH}\n\
         sudo setcap cap_perfmon+ep {PROBE_PATH}\n\
         printf '{tmpfiles}' | sudo tee /etc/tmpfiles.d/loadout-fps.conf\n\
         sudo systemd-tmpfiles --create /etc/tmpfiles.d/loadout-fps.conf",
        exe = shell_quote(exe),
    )
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

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
    source: Source,
    last: Option<(Instant, Vec<u64>)>,
}

enum Source {
    /// Counters this process opened itself (the system already allows it).
    #[cfg(target_os = "linux")]
    Direct(Counters),
    /// The CAP_PERFMON probe, asked over a pipe.
    #[cfg(target_os = "linux")]
    Probe(probe::Probe),
}

#[cfg(target_os = "linux")]
pub(crate) struct Counters {
    fds: Vec<(usize, std::os::fd::OwnedFd)>,
    crtcs: usize,
}

#[cfg(target_os = "linux")]
impl Counters {
    fn counts(&self) -> Vec<u64> {
        let mut out = vec![0u64; self.crtcs];
        for (crtc, fd) in &self.fds {
            out[*crtc] += linux::read_count(fd);
        }
        out
    }
}

impl FlipCounter {
    /// Counters opened here when the system allows it, else the probe when
    /// the setup has been run.
    pub fn open() -> Result<Self, FpsBlock> {
        #[cfg(target_os = "linux")]
        {
            let source = match linux::open() {
                Ok(c) => Source::Direct(c),
                Err(FpsBlock::NeedsPermission) if std::path::Path::new(PROBE_PATH).exists() => {
                    Source::Probe(probe::Probe::spawn()?)
                }
                Err(e) => return Err(e),
            };
            Ok(FlipCounter { source, last: None })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(FpsBlock::Unsupported)
        }
    }

    /// The busiest screen's frames per second since the last call. The first
    /// call only sets the baseline; `None` too when the probe went away.
    pub fn sample(&mut self) -> Option<f32> {
        let now = Instant::now();
        let counts = self.counts()?;
        let rate = self.last.as_ref().and_then(|(at, prev)| {
            busiest_rate(prev, &counts, now.duration_since(*at).as_secs_f64())
        });
        self.last = Some((now, counts));
        rate
    }

    fn counts(&mut self) -> Option<Vec<u64>> {
        match &mut self.source {
            #[cfg(target_os = "linux")]
            Source::Direct(c) => Some(c.counts()),
            #[cfg(target_os = "linux")]
            Source::Probe(p) => p.ask(),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

/// Counts as the probe writes them: one number per CRTC, space separated.
pub fn parse_counts(line: &str) -> Option<Vec<u64>> {
    let out: Option<Vec<u64>> = line.split_whitespace().map(|n| n.parse().ok()).collect();
    out.filter(|v| !v.is_empty())
}

/// The body of `--fps-probe`: open the counters, then answer each line on
/// stdin with the counts, until stdin closes. Exit code 2 when the counters
/// won't open.
pub fn run_probe() -> i32 {
    #[cfg(target_os = "linux")]
    {
        use std::io::{BufRead, Write};
        let Ok(counters) = linux::open() else {
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
/// The probe copy carries CAP_PERFMON as a file capability, so whatever runs
/// it gets that capability at exec. `main` calls this first thing on every
/// path except a bare `--fps-probe`, and the probe calls it as soon as its
/// counters are open, so the app proper never runs privileged. If the drop
/// fails while capabilities are held, the process exits rather than go on.
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
mod probe {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

    use super::{parse_counts, FpsBlock, PROBE_ARG, PROBE_PATH};

    pub(super) struct Probe {
        child: Child,
        stdin: ChildStdin,
        out: BufReader<ChildStdout>,
    }

    impl Probe {
        /// Starts the probe and checks it answers.
        pub(super) fn spawn() -> Result<Self, FpsBlock> {
            let mut child = Command::new(PROBE_PATH)
                .arg(PROBE_ARG)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| FpsBlock::NeedsPermission)?;
            let (Some(stdin), Some(out)) = (child.stdin.take(), child.stdout.take()) else {
                let _ = child.kill();
                let _ = child.wait();
                return Err(FpsBlock::NeedsPermission);
            };
            let mut probe = Probe {
                child,
                stdin,
                out: BufReader::new(out),
            };
            match probe.ask() {
                Some(_) => Ok(probe),
                None => Err(FpsBlock::NeedsPermission),
            }
        }

        pub(super) fn ask(&mut self) -> Option<Vec<u64>> {
            self.stdin.write_all(b"\n").ok()?;
            self.stdin.flush().ok()?;
            let mut line = String::new();
            self.out.read_line(&mut line).ok()?;
            parse_counts(&line)
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
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

    pub(super) fn open() -> Result<Counters, FpsBlock> {
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
        Ok(Counters {
            fds,
            crtcs: if filtered { MAX_CRTCS as usize } else { 1 },
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
    fn setup_scopes_the_grant_to_the_probe() {
        let s = linux_setup("/opt/Loadout's/grokhub");
        assert!(!s.contains("perf_event_paranoid"), "{s}");
        assert!(s.contains("setcap cap_perfmon+ep /usr/local/lib/loadout/fps-probe"));
        assert!(s.contains("-m 0750 -o root"));
        assert!(s.contains("'/opt/Loadout'\\''s/grokhub'"), "{s}");
        assert!(s.contains("drm_vblank_event_delivered 0711"));
        assert!(s.contains("drm_vblank_event_delivered/id 0444"));
        assert!(!s.contains("0755"), "folders stay unlistable: {s}");
        assert!(!s.contains("chmod -R"));
    }

    #[test]
    fn probe_lines_parse() {
        assert_eq!(parse_counts("1 0 240\n"), Some(vec![1, 0, 240]));
        assert_eq!(parse_counts(""), None);
        assert_eq!(parse_counts("1 x"), None);
    }
}
