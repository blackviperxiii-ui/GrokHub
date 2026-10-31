//! Screen readers for a host that has no desktop scan of its own (the Tauri
//! shell): lock-screen titles and the control under a click, from the same
//! AT-SPI walk and `wmctrl` list the cabin's desk scan reads. Each read is
//! bounded, so a hung helper never holds a tool call.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long the lock read may take, as the cabin's desk list.
const LIST_TIMEOUT: Duration = Duration::from_millis(1500);
/// How long the click-target read may take before the click counts as unknown.
pub const HIT_CAP: Duration = Duration::from_millis(300);
/// One lock read serves the calls of one step.
const LOCK_TTL: Duration = Duration::from_millis(500);
/// Larger output is cut, never read whole.
const OUT_CAP: usize = 4 * 1024 * 1024;

static LAST_LOCK: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);

/// Stdout of `cmd` when it exits 0 within `timeout`; otherwise `None`, and
/// a child still running is killed.
fn run_bounded(mut cmd: Command, timeout: Duration) -> Option<String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut rest = stdout.take(OUT_CAP as u64);
        let _ = rest.read_to_end(&mut buf);
        // Drain the rest so the helper can exit instead of blocking on a full pipe.
        let _ = std::io::copy(&mut rest.into_inner(), &mut std::io::sink());
        buf
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().ok()?;
                return status
                    .success()
                    .then(|| String::from_utf8_lossy(&out).into_owned());
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn atspi(timeout: Duration) -> String {
    let mut cmd = Command::new("python3");
    cmd.args(["-c", grokhub_core::ATSPI_PY]);
    run_bounded(cmd, timeout).unwrap_or_default()
}

/// Lock or login window titles on screen now; any one means locked.
/// Reading other windows' titles waits while an anti-cheat runs: then no
/// helper is started and the list is empty.
pub fn lock_titles() -> Vec<String> {
    let guard = crate::game_guard::current();
    if guard
        .blocks(crate::game_guard::Guarded::WindowCheck)
        .is_some()
    {
        return Vec::new();
    }
    if let Ok(last) = LAST_LOCK.lock() {
        if let Some((at, titles)) = last.as_ref() {
            if at.elapsed() < LOCK_TTL {
                return titles.clone();
            }
        }
    }
    let atspi = atspi(LIST_TIMEOUT);
    let wmctrl = crate::resolve_bin("wmctrl")
        .and_then(|bin| {
            let mut cmd = Command::new(bin);
            cmd.arg("-lG");
            run_bounded(cmd, LIST_TIMEOUT)
        })
        .unwrap_or_default();
    let lines: Vec<&str> = atspi.lines().chain(wmctrl.lines()).collect();
    let titles = grokhub_core::lock_check_titles(&lines);
    if let Ok(mut last) = LAST_LOCK.lock() {
        *last = Some((Instant::now(), titles.clone()));
    }
    titles
}

/// The control at a screen point, read-only and capped at [`HIT_CAP`].
/// `None` on a timeout, no python3 or pyatspi, or no control there.
pub fn atspi_target_at(x: i32, y: i32) -> Option<grokhub_core::desktop_mcp::ClickTarget> {
    let rows: Vec<grokhub_core::AtspiRow> = atspi(HIT_CAP)
        .lines()
        .filter_map(grokhub_core::parse_atspi_line)
        .collect();
    grokhub_core::control_at(&rows, x, y).map(|r| grokhub_core::desktop_mcp::ClickTarget {
        label: r.name.clone(),
        role: r.role.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hung_helper_is_killed_at_the_cap() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let started = Instant::now();
        assert_eq!(run_bounded(cmd, Duration::from_millis(200)), None);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn output_is_read_and_a_failure_is_none() {
        let mut echo = Command::new("sh");
        echo.args(["-c", "printf 'role=push-button name=Send x=1 y=2 w=3 h=4'"]);
        assert_eq!(
            run_bounded(echo, Duration::from_secs(5)).as_deref(),
            Some("role=push-button name=Send x=1 y=2 w=3 h=4")
        );
        let mut fail = Command::new("sh");
        fail.args(["-c", "echo nope; exit 2"]);
        assert_eq!(run_bounded(fail, Duration::from_secs(5)), None);
        assert_eq!(
            run_bounded(Command::new("no-such-helper-here"), HIT_CAP),
            None
        );
    }

    #[test]
    fn huge_output_stays_capped() {
        let mut dump = Command::new("head");
        dump.args(["-c", &(OUT_CAP + 1024 * 1024).to_string(), "/dev/zero"]);
        let out = run_bounded(dump, Duration::from_secs(10)).expect("head exits");
        assert_eq!(out.len(), OUT_CAP);
    }
}
