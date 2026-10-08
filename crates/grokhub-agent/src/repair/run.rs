//! Run one probe: no shell, a hard timeout, capped output, then redaction.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::probes::{ProbeSpec, LOG_LINE_CAP, OUTPUT_CAP};

/// What a probe printed, before redaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeRun {
    /// It finished. `code` is `None` when a signal ended it.
    Done { code: Option<i32>, stdout: String, stderr: String },
    /// It ran past its timeout and was killed.
    TimedOut,
    /// The program isn't there or couldn't start.
    Missing(String),
}

/// Run `spec` with `std::process::Command` (no shell). Killed at `spec.timeout`;
/// each stream keeps at most [`OUTPUT_CAP`] bytes.
pub fn run_spec(spec: &ProbeSpec) -> ProbeRun {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Stable English output for the parsers.
        .env("LC_ALL", "C")
        .env("LANG", "C");
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => return ProbeRun::Missing(e.to_string()),
    };
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let start = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if start.elapsed() >= spec.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return ProbeRun::TimedOut;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return ProbeRun::Missing(e.to_string()),
        }
    };
    ProbeRun::Done {
        code,
        stdout: collect(stdout),
        stderr: collect(stderr),
    }
}

type Sink = (Arc<Mutex<Vec<u8>>>, std::thread::JoinHandle<()>);

/// Read a pipe on its own thread so a chatty probe can't block on a full pipe.
/// Bytes past the cap are read and dropped.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> Option<Sink> {
    let mut pipe = pipe?;
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buf);
    let handle = std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if let Ok(mut b) = sink.lock() {
                let room = OUTPUT_CAP.saturating_sub(b.len());
                b.extend_from_slice(&chunk[..n.min(room)]);
            }
        }
    });
    Some((buf, handle))
}

fn collect(sink: Option<Sink>) -> String {
    let Some((buf, handle)) = sink else {
        return String::new();
    };
    let _ = handle.join();
    let bytes = buf.lock().map(|b| b.clone()).unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Secrets and personal data out, before output reaches a span or the model.
/// Log probes also keep only their last [`LOG_LINE_CAP`] lines.
pub fn redact_output(text: &str, is_log: bool) -> String {
    let text = grokhub_core::redact_secrets(text);
    let (text, _) = grokhub_core::redact_pii(&text);
    if !is_log {
        return text;
    }
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let keep = lines.len().saturating_sub(LOG_LINE_CAP);
    lines[keep..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repair::probes::ProbeId;

    /// A program that hangs for about 30 s, with no shell.
    fn hanging(timeout: Duration) -> ProbeSpec {
        let (program, argv): (&str, Vec<&str>) = if cfg!(windows) {
            ("ping", vec!["-n", "30", "127.0.0.1"])
        } else {
            ("sleep", vec!["30"])
        };
        ProbeSpec {
            id: ProbeId::DiskUsage,
            program: program.into(),
            argv: argv.into_iter().map(String::from).collect(),
            timeout,
            needs_admin: false,
        }
    }

    #[test]
    fn a_hanging_probe_is_killed_at_its_timeout() {
        let start = Instant::now();
        assert_eq!(run_spec(&hanging(Duration::from_millis(300))), ProbeRun::TimedOut);
        let took = start.elapsed();
        assert!(took >= Duration::from_millis(300), "{took:?}");
        assert!(took < Duration::from_secs(10), "{took:?}");
    }

    #[test]
    fn a_missing_program_reports_missing() {
        let mut spec = hanging(Duration::from_secs(1));
        spec.program = "grokhub-no-such-probe-program".into();
        assert!(matches!(run_spec(&spec), ProbeRun::Missing(_)));
    }

    #[test]
    fn journal_lines_lose_keys_and_emails_and_keep_twenty() {
        let mut log = String::new();
        for i in 0..25 {
            log.push_str(&format!("Oct 07 10:00:{i:02} box app[1]: line {i}\n"));
        }
        log.push_str("Oct 07 10:01:00 box sync[9]: auth failed for jeremy@example.com with key sk-abcdefghijklmnopqrstuv\n");
        let out = redact_output(&log, true);
        assert_eq!(out.lines().count(), 20);
        assert!(!out.contains("sk-abcdefghijklmnopqrstuv"), "{out}");
        assert!(!out.contains("jeremy@example.com"), "{out}");
        assert!(out.ends_with("auth failed for [email] with key [redacted]"), "{out}");
        assert!(out.starts_with("Oct 07 10:00:06 box app[1]: line 6"), "{out}");
    }
}
