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
//! local process watch every CPU. Instead [`linux_setup`] installs a probe
//! that only this user's group can run and gives that one file CAP_PERFMON.
//! The probe is the small libc-only `loadout-fps-probe` the Linux bundles ship
//! (see [`probe_source`]), so it starts from any install. The app runs it as
//! `--fps-probe`, asks it for the counts once a second over a pipe, and
//! closing the pipe ends it.
//!
//! On Windows the count comes from the graphics kernel's present events
//! instead (see `present`), again from outside every game.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[cfg(windows)]
mod present;
pub mod setup;

#[cfg(target_os = "linux")]
use grokhub_fps_probe::Counters;
pub use grokhub_fps_probe::{
    drop_privileges, parse_counts, parse_cpu_list, parse_id, run_probe, MAX_CRTCS, PROBE_ARG,
};

/// Where [`linux_setup`] puts the probe.
pub const PROBE_PATH: &str = "/usr/local/lib/loadout/fps-probe";

/// Where the bundles put the probe, relative to the folder the app runs from
/// (`usr/bin` in the AppImage and the deb), then the name it has beside a
/// plain build.
const BUNDLED_PROBES: [&str; 2] = ["../lib/loadout/fps-probe", "loadout-fps-probe"];

/// The file [`linux_setup`] installs as the probe: the bundled probe next to
/// `exe` when there is one, else `exe` itself (the desktop app answers
/// `--fps-probe` too). A full app copy can fail to start once it carries a
/// file capability (secure-exec ignores `LD_LIBRARY_PATH`, and an AppImage's
/// inner binary needs its bundled libraries), so the bundled one comes first.
pub fn probe_source(exe: &Path) -> PathBuf {
    exe.parent()
        .into_iter()
        .flat_map(|dir| BUNDLED_PROBES.iter().map(move |p| dir.join(p)))
        .find(|p| p.is_file())
        .unwrap_or_else(|| exe.to_path_buf())
}

/// The tracefs folders the setup makes traversable (not listable), and the
/// one tracepoint id it makes readable.
const TRACE_DIRS: [&str; 4] = [
    "/sys/kernel/tracing",
    "/sys/kernel/tracing/events",
    "/sys/kernel/tracing/events/drm",
    "/sys/kernel/tracing/events/drm/drm_vblank_event_delivered",
];
const TRACE_ID: &str = "/sys/kernel/tracing/events/drm/drm_vblank_event_delivered/id";
/// The boot-time rule that keeps those modes.
pub const TMPFILES_RULE: &str = "/etc/tmpfiles.d/loadout-fps.conf";

/// The tmpfiles rule's text, with `\n` escapes for `printf`.
fn tmpfiles_text() -> String {
    let mut out: String = TRACE_DIRS
        .iter()
        .map(|d| format!("z {d} 0711 - - -\\n"))
        .collect();
    out.push_str(&format!("z {TRACE_ID} 0444 - - -\\n"));
    out
}

/// The setup, as commands the user pastes into a terminal: a copy of `exe`
/// (from [`probe_source`]) as the probe (root-owned, runnable only by the
/// user's group) with CAP_PERFMON on that file alone, and tracefs opened just
/// far enough to read the one tracepoint id (folders traversable, not
/// listable). The file is copied to a private temp file first, as the user:
/// root can't read inside an AppImage's mount. Re-running it after an update
/// refreshes the probe.
pub fn linux_setup(exe: &str) -> String {
    format!(
        "t=\"$(mktemp)\" && cp {exe} \"$t\"\n\
         sudo install -D -m 0750 -o root -g \"$(id -gn)\" \"$t\" {PROBE_PATH}; rm -f \"$t\"\n\
         sudo setcap cap_perfmon+ep {PROBE_PATH}\n\
         printf '{tmpfiles}' | sudo tee {TMPFILES_RULE}\n\
         sudo systemd-tmpfiles --create {TMPFILES_RULE}",
        exe = shell_quote(exe),
        tmpfiles = tmpfiles_text(),
    )
}

/// The same setup as one root script (run with `sh -e` behind the polkit
/// prompt): `file` is the private copy, `gid` the user's group. Without
/// systemd-tmpfiles it sets the modes itself, until the next boot.
pub fn linux_root_script(file: &str, gid: u32) -> String {
    let dirs = TRACE_DIRS.join(" ");
    format!(
        "install -D -m 0750 -o root -g {gid} {file} {PROBE_PATH}\n\
         setcap cap_perfmon+ep {PROBE_PATH}\n\
         printf '{tmpfiles}' > {TMPFILES_RULE}\n\
         if command -v systemd-tmpfiles >/dev/null 2>&1; then systemd-tmpfiles --create {TMPFILES_RULE} || true; \
         else chmod 0711 {dirs} 2>/dev/null || true; chmod 0444 {TRACE_ID} 2>/dev/null || true; fi",
        file = shell_quote(file),
        tmpfiles = tmpfiles_text(),
    )
}

/// The lines that undo the Linux setup.
pub fn linux_undo() -> String {
    format!("sudo rm -f {PROBE_PATH} {TMPFILES_RULE}")
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The SID of the built-in Performance Log Users group (same in every
/// language, unlike its name).
pub const PERF_LOG_USERS_SID: &str = "S-1-5-32-559";

/// Windows processes whose presents are never the game: the desktop
/// compositor, and the kernel's own process for work it does on its threads.
const NOT_GAMES: [&str; 2] = ["dwm.exe", "System"];

/// The PowerShell lines (run once, as administrator) that put `account` in
/// Performance Log Users. The account is named so an elevated prompt under a
/// different admin still adds the right user.
pub fn windows_setup(account: &str) -> String {
    format!(
        "Add-LocalGroupMember -SID '{PERF_LOG_USERS_SID}' -Member {}\n\
         # Then sign out of Windows and back in.",
        ps_quote(account)
    )
}

/// The line that undoes [`windows_setup`].
pub fn windows_undo(account: &str) -> String {
    format!(
        "Remove-LocalGroupMember -SID '{PERF_LOG_USERS_SID}' -Member {}",
        ps_quote(account)
    )
}

/// The setup as one admin PowerShell script (run behind the UAC prompt).
/// Already being in the group counts as done.
pub fn windows_script(account: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         try {{ Add-LocalGroupMember -SID '{PERF_LOG_USERS_SID}' -Member {} }} \
         catch [Microsoft.PowerShell.Commands.MemberExistsException] {{ }}",
        ps_quote(account)
    )
}

/// The undo as one admin PowerShell script. Not being in the group counts
/// as done.
pub fn windows_undo_script(account: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         try {{ Remove-LocalGroupMember -SID '{PERF_LOG_USERS_SID}' -Member {} }} \
         catch [Microsoft.PowerShell.Commands.PrincipalNotFoundException], \
         [Microsoft.PowerShell.Commands.MemberNotFoundException] {{ }}",
        ps_quote(account)
    )
}

/// The signed-in Windows account as `DOMAIN\user`, from the environment.
pub fn windows_account() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(d) if !d.is_empty() => format!("{d}\\{user}"),
        _ => user,
    }
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The busiest presenter in a round of counts, leaving out Loadout itself and
/// anything in [`NOT_GAMES`]. The two columns are DxgKrnl and DXGI counts of
/// the same presents, so the larger one is the process's count.
pub fn busiest_presenter(
    counts: &HashMap<u32, [u64; 2]>,
    own_pid: u32,
    name: &mut impl FnMut(u32) -> Option<String>,
) -> Option<(u32, u64)> {
    let mut ranked: Vec<(u32, u64)> = counts
        .iter()
        .filter(|(pid, _)| **pid != own_pid && **pid != 0)
        .map(|(pid, c)| (*pid, c[0].max(c[1])))
        .filter(|(_, n)| *n > 0)
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked.into_iter().find(|(pid, _)| {
        name(*pid).is_none_or(|n| !NOT_GAMES.iter().any(|x| n.eq_ignore_ascii_case(x)))
    })
}

/// Why the counter couldn't open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FpsBlock {
    /// Neither Linux nor Windows.
    Unsupported,
    /// The kernel has no DRM flip tracepoint (no DRM driver, or tracing off),
    /// or Windows wouldn't start the present trace for another reason.
    NoTracepoint,
    /// Linux: the tracepoint id or the perf counters need the one-time setup.
    /// Windows: starting a trace needs Performance Log Users.
    NeedsPermission,
}

impl From<grokhub_fps_probe::Block> for FpsBlock {
    fn from(b: grokhub_fps_probe::Block) -> Self {
        match b {
            grokhub_fps_probe::Block::NoTracepoint => FpsBlock::NoTracepoint,
            grokhub_fps_probe::Block::NeedsPermission => FpsBlock::NeedsPermission,
        }
    }
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
    /// The Windows graphics kernel's present events.
    #[cfg(windows)]
    Etw(present::Session),
}

impl FlipCounter {
    /// Counters opened here when the system allows it, else the probe when
    /// the setup has been run.
    pub fn open() -> Result<Self, FpsBlock> {
        #[cfg(target_os = "linux")]
        {
            let source = match Counters::open().map_err(FpsBlock::from) {
                Ok(c) => Source::Direct(c),
                Err(FpsBlock::NeedsPermission) if std::path::Path::new(PROBE_PATH).exists() => {
                    Source::Probe(probe::Probe::spawn()?)
                }
                Err(e) => return Err(e),
            };
            Ok(FlipCounter { source, last: None })
        }
        #[cfg(windows)]
        {
            Ok(FlipCounter {
                source: Source::Etw(present::Session::start()?),
                last: None,
            })
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            Err(FpsBlock::Unsupported)
        }
    }

    /// On Windows, the exe name of the process the last rate came from.
    pub fn presenter(&self) -> Option<&str> {
        match &self.source {
            #[cfg(windows)]
            Source::Etw(s) => s.presenter.as_deref(),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// The busiest screen's frames per second since the last call. The first
    /// call only sets the baseline; `None` too when the probe went away.
    pub fn sample(&mut self) -> Option<f32> {
        match &mut self.source {
            #[cfg(windows)]
            Source::Etw(s) => s.rate(),
            #[allow(unreachable_patterns)]
            _ => self.sample_counts(),
        }
    }

    fn sample_counts(&mut self) -> Option<f32> {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_busiest_presenter_skips_loadout_and_the_compositor() {
        let counts: HashMap<u32, [u64; 2]> = [
            (10, [300, 0]),
            (20, [0, 144]),
            (30, [500, 500]),
            (40, [900, 0]),
        ]
        .into();
        let names: HashMap<u32, &str> = [(10, "dwm.exe"), (20, "game.exe"), (30, "DWM.EXE")].into();
        let mut name = |pid: u32| names.get(&pid).map(|n| n.to_string());
        // 40 is Loadout itself; 30 and 10 are the compositor.
        assert_eq!(
            super::busiest_presenter(&counts, 40, &mut name),
            Some((20, 144))
        );
        assert_eq!(
            super::busiest_presenter(&HashMap::new(), 1, &mut name),
            None
        );
    }

    #[test]
    fn windows_setup_names_the_group_by_sid_and_quotes_the_account() {
        let set = super::windows_setup("PC\\O'Brien");
        assert!(set.contains("Add-LocalGroupMember -SID 'S-1-5-32-559' -Member 'PC\\O''Brien'"));
        assert!(set.contains("sign out"));
        let undo = super::windows_undo("PC\\O'Brien");
        assert!(undo.starts_with("Remove-LocalGroupMember -SID 'S-1-5-32-559'"));
    }

    #[test]
    fn the_busiest_screen_sets_the_rate() {
        assert_eq!(busiest_rate(&[0, 100], &[60, 244], 1.0), Some(144.0));
        assert_eq!(busiest_rate(&[10], &[70], 0.5), Some(120.0));
        assert_eq!(busiest_rate(&[10], &[5], 1.0), None);
        assert_eq!(busiest_rate(&[1], &[2], 0.0), None);
    }

    #[test]
    fn the_bundled_probe_comes_before_the_app() {
        let root = std::env::temp_dir().join(format!("fps-probe-src-{}", std::process::id()));
        let bin = root.join("usr/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("grokhub-shell");
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(probe_source(&exe), exe);
        std::fs::write(bin.join("loadout-fps-probe"), b"").unwrap();
        assert_eq!(probe_source(&exe), bin.join("loadout-fps-probe"));
        std::fs::create_dir_all(root.join("usr/lib/loadout")).unwrap();
        std::fs::write(root.join("usr/lib/loadout/fps-probe"), b"").unwrap();
        assert_eq!(probe_source(&exe), bin.join("../lib/loadout/fps-probe"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_root_script_installs_the_private_copy_without_sudo() {
        let s = linux_root_script("/tmp/loadout-fps-probe-1-2", 1000);
        assert!(s.starts_with(
            "install -D -m 0750 -o root -g 1000 '/tmp/loadout-fps-probe-1-2' /usr/local/lib/loadout/fps-probe\n"
        ));
        assert!(s.contains("setcap cap_perfmon+ep /usr/local/lib/loadout/fps-probe\n"));
        assert!(s.contains("> /etc/tmpfiles.d/loadout-fps.conf\n"));
        assert!(!s.contains("sudo"), "{s}");
        assert!(!s.contains("perf_event_paranoid"));
        assert!(!s.contains("0755") && !s.contains("chmod -R"), "{s}");
        assert_eq!(
            linux_undo(),
            "sudo rm -f /usr/local/lib/loadout/fps-probe /etc/tmpfiles.d/loadout-fps.conf"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn both_linux_forms_parse_as_shell() {
        for script in [
            linux_setup("/opt/Loadout's/grokhub"),
            linux_root_script("/tmp/a b", 1000),
            linux_undo(),
        ] {
            let ok = std::process::Command::new("sh")
                .args(["-n", "-c", &script])
                .status()
                .unwrap()
                .success();
            assert!(ok, "{script}");
        }
    }

    #[test]
    fn the_windows_scripts_treat_no_change_as_done() {
        let set = windows_script("PC\\O'Brien");
        assert!(set.contains("Add-LocalGroupMember -SID 'S-1-5-32-559' -Member 'PC\\O''Brien'"));
        assert!(set.contains("MemberExistsException"));
        let undo = windows_undo_script("PC\\O'Brien");
        assert!(undo.contains("Remove-LocalGroupMember -SID 'S-1-5-32-559'"));
        assert!(undo.contains("MemberNotFoundException"));
    }

    #[test]
    fn setup_scopes_the_grant_to_the_probe() {
        let s = linux_setup("/opt/Loadout's/grokhub");
        assert!(!s.contains("perf_event_paranoid"), "{s}");
        assert!(s.contains("setcap cap_perfmon+ep /usr/local/lib/loadout/fps-probe"));
        assert!(s.contains("-m 0750 -o root"));
        assert!(s.starts_with("t=\"$(mktemp)\" && cp "), "{s}");
        assert!(s.contains("'/opt/Loadout'\\''s/grokhub'"), "{s}");
        assert!(s.contains("drm_vblank_event_delivered 0711"));
        assert!(s.contains("drm_vblank_event_delivered/id 0444"));
        assert!(!s.contains("0755"), "folders stay unlistable: {s}");
        assert!(!s.contains("chmod -R"));
    }
}
