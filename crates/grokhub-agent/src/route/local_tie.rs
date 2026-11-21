//! Ties the on-device model server's life to the app's, so a killed or
//! crashed app never leaves llama-server holding GPU memory.
//!
//! - Linux: the child gets a parent-death signal (`PR_SET_PDEATHSIG`,
//!   SIGKILL). Linux sends it when the spawning *thread* ends, so every spawn
//!   runs on one thread that lives as long as the app.
//! - Windows: the child joins a job object with kill-on-close. Only the app
//!   holds the job's handle, so the system ends the child when the app exits
//!   in any way.
//! - Elsewhere the child is spawned as before.

use std::io;
use std::process::{Child, Command};

/// Spawn `cmd` tied to this process.
pub fn spawn(cmd: Command) -> io::Result<Child> {
    imp::spawn(cmd)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Mutex, OnceLock};

    type Ask = (Command, Sender<io::Result<Child>>);

    pub fn spawn(mut cmd: Command) -> io::Result<Child> {
        // SAFETY: getpid can't fail.
        let parent = unsafe { libc::getpid() };
        // SAFETY: prctl and getppid are async-signal-safe and only touch the
        // child.
        unsafe {
            cmd.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                // The app died before the signal was set: don't start.
                if libc::getppid() != parent {
                    return Err(io::Error::other("the app is gone"));
                }
                Ok(())
            });
        }
        let (back, answer) = mpsc::channel();
        spawner()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .send((cmd, back))
            .map_err(|_| io::Error::other("the spawner thread is gone"))?;
        answer
            .recv()
            .map_err(|_| io::Error::other("the spawner thread is gone"))?
    }

    /// The one thread every tied child is spawned from. It never returns.
    fn spawner() -> &'static Mutex<Sender<Ask>> {
        static SPAWNER: OnceLock<Mutex<Sender<Ask>>> = OnceLock::new();
        SPAWNER.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Ask>();
            let started = std::thread::Builder::new()
                .name("local-model-spawner".into())
                .spawn(move || {
                    for (mut cmd, back) in rx {
                        let _ = back.send(cmd.spawn());
                    }
                });
            if let Err(err) = started {
                // No spawner: the receiver is dropped and each spawn says so.
                eprintln!("local model spawner: {err}");
            }
            Mutex::new(tx)
        })
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command};
    use std::sync::OnceLock;

    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub fn spawn(mut cmd: Command) -> io::Result<Child> {
        let child = cmd.spawn()?;
        if let Some(job) = job() {
            // SAFETY: both handles are live; a failure leaves the child as it was.
            unsafe {
                AssignProcessToJobObject(job as _, child.as_raw_handle() as _);
            }
        }
        Ok(child)
    }

    /// The app's one kill-on-close job, made once and never closed, so the
    /// system closes it (and ends its children) when the app exits.
    fn job() -> Option<isize> {
        static JOB: OnceLock<Option<isize>> = OnceLock::new();
        *JOB.get_or_init(|| {
            // SAFETY: plain Win32 calls on a handle this function owns.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let set = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const _,
                    u32::try_from(std::mem::size_of_val(&limits)).unwrap_or(u32::MAX),
                );
                (set != 0).then_some(job as isize)
            }
        })
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use std::io;
    use std::process::{Child, Command};

    pub fn spawn(mut cmd: Command) -> io::Result<Child> {
        cmd.spawn()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const HELPER_ENV: &str = "GROKHUB_TIE_HELPER";

    fn alive(pid: u32) -> bool {
        // A zombie still has a /proc entry; its state letter is Z.
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| {
                let state = stat
                    .rsplit(')')
                    .next()?
                    .split_whitespace()
                    .next()?
                    .to_string();
                Some(state != "Z")
            })
            .unwrap_or(false)
    }

    /// Run by [`a_tied_child_dies_with_the_app`] in its own process: spawn a
    /// tied `sleep` from a thread that then ends, print its pid, and wait.
    #[test]
    fn tied_child_helper() {
        if std::env::var_os(HELPER_ENV).is_none() {
            return;
        }
        let mut child = std::thread::spawn(|| {
            let mut cmd = Command::new("sleep");
            cmd.arg("60");
            super::spawn(cmd).unwrap()
        })
        .join()
        .unwrap();
        println!("tied-pid={}", child.id());
        std::thread::sleep(Duration::from_secs(60));
        let _ = child.kill();
        let _ = child.wait();
    }

    /// The child outlives the thread that asked for it, and dies once the
    /// app does, even when the app is SIGKILLed.
    #[test]
    fn a_tied_child_dies_with_the_app() {
        let mut app = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "route::local_tie::tests::tied_child_helper",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(HELPER_ENV, "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut out = app.stdout.take().unwrap();
        let mut seen = String::new();
        let started = Instant::now();
        let pid = loop {
            let mut buf = [0u8; 256];
            let n = std::io::Read::read(&mut out, &mut buf).unwrap();
            seen.push_str(&String::from_utf8_lossy(&buf[..n]));
            if let Some(pid) = seen
                .split("tied-pid=")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|p| p.parse::<u32>().ok())
                .filter(|_| {
                    seen.split("tied-pid=")
                        .nth(1)
                        .is_some_and(|r| r.contains('\n'))
                })
            {
                break pid;
            }
            assert!(
                n > 0 && started.elapsed() < Duration::from_secs(30),
                "{seen}"
            );
        };
        std::thread::sleep(Duration::from_millis(300));
        assert!(alive(pid), "the child outlives the thread that spawned it");
        app.kill().unwrap();
        let _ = app.wait();
        let gone = (0..50).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            !alive(pid)
        });
        assert!(gone, "the tied child outlived the app");
    }
}
