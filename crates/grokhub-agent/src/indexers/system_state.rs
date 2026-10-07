//! `system_state`: a read-only snapshot of disk, services, the boot log and
//! pending updates. Spike-8b's diagnose reuses [`snapshot`]. Only counts and
//! unit names are kept: no log line, path or command output text becomes a fact.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use grokhub_core::amr::Sensitivity;

use super::paths::{HostOs, PlatformDirs};
use super::Fact;

/// How long one read-only command may run before it is dropped.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
/// Most failed units named in one fact.
const UNITS_MAX: usize = 10;

/// Free and total bytes of one disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskUse {
    /// `/`, `home`, or a Windows drive such as `C:`.
    pub label: String,
    pub total: u64,
    pub free: u64,
}

/// What the system looked like. `None` means "not available on this OS".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemSnapshot {
    pub disks: Vec<DiskUse>,
    /// Failed systemd units (Linux).
    pub failed_units: Option<Vec<String>>,
    /// Error-or-worse lines in this boot's journal (Linux), capped at 500.
    pub boot_errors: Option<usize>,
    /// Packages with a newer version in the last synced database (Arch `pacman -Qu`, no network).
    pub pending_updates: Option<usize>,
}

/// Where the snapshot reads from. Tests use a fake.
pub trait SystemProbe: Send + Sync {
    /// (total, free) bytes of the disk holding `path`.
    fn disk(&self, path: &Path) -> Option<(u64, u64)>;
    /// Stdout lines of a read-only command, or `None` if it is missing,
    /// failed to start, or ran past the timeout.
    fn lines(&self, program: &str, args: &[&str]) -> Option<Vec<String>>;
}

/// The real machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsProbe;

impl SystemProbe for OsProbe {
    fn disk(&self, path: &Path) -> Option<(u64, u64)> {
        disk_usage(path)
    }

    fn lines(&self, program: &str, args: &[&str]) -> Option<Vec<String>> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut out = child.stdout.take()?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = std::io::Read::read_to_string(&mut out, &mut buf);
            let _ = tx.send(buf);
        });
        match rx.recv_timeout(COMMAND_TIMEOUT) {
            Ok(text) => {
                let _ = child.wait();
                Some(text.lines().map(str::to_string).filter(|l| !l.trim().is_empty()).collect())
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                None
            }
        }
    }
}

#[cfg(unix)]
fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a NUL-terminated path and `st` a valid out pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let unit = st.f_frsize as u64;
    Some((st.f_blocks as u64 * unit, st.f_bavail as u64 * unit))
}

#[cfg(windows)]
fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and the three out pointers are valid.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free)
    };
    let _ = free;
    (ok != 0).then_some((total, avail))
}

#[cfg(not(any(unix, windows)))]
fn disk_usage(_path: &Path) -> Option<(u64, u64)> {
    None
}

/// The disks worth a line: root and home on Unix, the home drive on Windows.
fn disk_paths(dirs: &PlatformDirs) -> Vec<(String, PathBuf)> {
    match dirs.os {
        HostOs::Windows => {
            // String-based, so the same answer on any host (tests run on Linux too).
            let drive = dirs
                .home
                .as_ref()
                .map(|h| h.display().to_string())
                .filter(|h| h.as_bytes().get(1) == Some(&b':'))
                .map(|h| h[..2].to_ascii_uppercase())
                .unwrap_or_else(|| "C:".into());
            vec![(drive.clone(), PathBuf::from(format!("{drive}\\")))]
        }
        HostOs::Linux | HostOs::MacOs => {
            let mut out = vec![("/".to_string(), PathBuf::from("/"))];
            if let Some(home) = &dirs.home {
                out.push(("home".to_string(), home.clone()));
            }
            out
        }
    }
}

/// Take the snapshot. Read only: statvfs / GetDiskFreeSpaceEx, plus on Linux
/// `systemctl --failed`, `journalctl -p err -b` and `pacman -Qu` (local database only).
pub fn snapshot(probe: &dyn SystemProbe, dirs: &PlatformDirs) -> SystemSnapshot {
    let mut snap = SystemSnapshot::default();
    for (label, path) in disk_paths(dirs) {
        if let Some((total, free)) = probe.disk(&path) {
            // Home on the root disk is the same disk: one line.
            if snap.disks.iter().any(|d| d.total == total && d.free == free) {
                continue;
            }
            snap.disks.push(DiskUse { label, total, free });
        }
    }
    if dirs.os == HostOs::Linux {
        snap.failed_units = probe
            .lines("systemctl", &["--failed", "--no-legend", "--plain", "--no-pager"])
            .map(|lines| lines.iter().filter_map(|l| l.split_whitespace().next().map(str::to_string)).collect());
        snap.boot_errors = probe
            .lines("journalctl", &["-p", "err", "-b", "-q", "--no-pager", "-o", "cat", "-n", "500"])
            .map(|l| l.len());
        snap.pending_updates = probe.lines("pacman", &["-Qu"]).map(|l| l.len());
    }
    snap
}

fn gb(bytes: u64) -> String {
    format!("{:.0} GB", bytes as f64 / 1_000_000_000.0)
}

impl SystemSnapshot {
    /// One fact per line of the snapshot. Each item key is stable, so a later
    /// snapshot with the same numbers writes nothing new.
    pub fn facts(&self, day: &str) -> Vec<Fact> {
        let mut out = Vec::new();
        let mut push = |item: String, line: String| {
            out.push(Fact { item: format!("{day}:{item}"), line, sensitivity: Sensitivity::Personal });
        };
        for d in &self.disks {
            let used = (d.free * 100).checked_div(d.total).map_or(0, |free| 100 - free);
            push(
                format!("disk:{}:{used}", d.label),
                format!("System on {day}: disk {} is {used}% used, {} free of {}", d.label, gb(d.free), gb(d.total)),
            );
        }
        if let Some(units) = &self.failed_units {
            let line = if units.is_empty() {
                format!("System on {day}: no failed services")
            } else {
                let shown: Vec<&str> = units.iter().take(UNITS_MAX).map(String::as_str).collect();
                format!("System on {day}: {} failed services ({})", units.len(), shown.join(", "))
            };
            push(format!("units:{}", units.join(",")), line);
        }
        if let Some(n) = self.boot_errors {
            push(format!("errors:{n}"), format!("System on {day}: {n} errors in this boot's log"));
        }
        if let Some(n) = self.pending_updates {
            push(format!("updates:{n}"), format!("System on {day}: {n} updates waiting"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl SystemProbe for Fake {
        fn disk(&self, path: &Path) -> Option<(u64, u64)> {
            if path == Path::new("/") {
                Some((500_000_000_000, 185_000_000_000))
            } else {
                Some((2_000_000_000_000, 1_000_000_000_000))
            }
        }
        fn lines(&self, program: &str, _args: &[&str]) -> Option<Vec<String>> {
            match program {
                "systemctl" => Some(vec!["bluetooth.service loaded failed failed Bluetooth".into()]),
                "journalctl" => Some(vec!["e1".into(), "e2".into(), "e3".into()]),
                _ => None,
            }
        }
    }

    #[test]
    fn snapshot_keeps_counts_and_unit_names_only() {
        let dirs = PlatformDirs {
            os: HostOs::Linux,
            home: Some(PathBuf::from("/home/you")),
            appdata: None,
            local_appdata: None,
            program_data: None,
            config_home: None,
            data_home: None,
            system_data: Vec::new(),
        };
        let snap = snapshot(&Fake, &dirs);
        assert_eq!(snap.failed_units, Some(vec!["bluetooth.service".to_string()]));
        assert_eq!(snap.boot_errors, Some(3));
        assert_eq!(snap.pending_updates, None, "no pacman: not available, not zero");
        let lines: Vec<String> = snap.facts("2026-10-07").into_iter().map(|f| f.line).collect();
        assert_eq!(
            lines,
            vec![
                "System on 2026-10-07: disk / is 63% used, 185 GB free of 500 GB",
                "System on 2026-10-07: disk home is 50% used, 1000 GB free of 2000 GB",
                "System on 2026-10-07: 1 failed services (bluetooth.service)",
                "System on 2026-10-07: 3 errors in this boot's log",
            ]
        );
    }

    #[test]
    fn windows_snapshot_reads_the_home_drive_and_runs_no_command() {
        struct Win;
        impl SystemProbe for Win {
            fn disk(&self, path: &Path) -> Option<(u64, u64)> {
                (path == Path::new("C:\\")).then_some((1_000_000_000_000, 250_000_000_000))
            }
            fn lines(&self, program: &str, _args: &[&str]) -> Option<Vec<String>> {
                panic!("no command on Windows: {program}")
            }
        }
        let dirs = PlatformDirs {
            os: HostOs::Windows,
            home: Some(PathBuf::from("C:\\Users\\you")),
            appdata: None,
            local_appdata: None,
            program_data: None,
            config_home: None,
            data_home: None,
            system_data: Vec::new(),
        };
        let snap = snapshot(&Win, &dirs);
        assert_eq!(snap.disks, vec![DiskUse { label: "C:".into(), total: 1_000_000_000_000, free: 250_000_000_000 }]);
        assert_eq!(snap.failed_units, None);
    }
}
