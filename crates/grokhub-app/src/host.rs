use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub fn run_host(cmd: &str, timeout: Duration) -> String {
    run_host_stream(cmd, timeout, None, |_| {})
}

pub fn run_host_stream(
    cmd: &str,
    timeout: Duration,
    cancel: Option<&AtomicBool>,
    mut on_line: impl FnMut(&str),
) -> String {
    let start = Instant::now();
    let mut spawn = Command::new("bash");
    spawn
        .arg("-lc")
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        spawn.process_group(0);
    }
    let mut child = match spawn.spawn() {
        Ok(c) => c,
        Err(e) => return format!("$ {cmd}\nspawn failed: {e}"),
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    if let Some(so) = stdout {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(so).lines().flatten() {
                if tx.send((false, line)).is_err() {
                    break;
                }
            }
        });
    }
    if let Some(se) = stderr {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(se).lines().flatten() {
                if tx.send((true, line)).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);

    let mut out_buf = String::new();
    let mut err_buf = String::new();
    let cancelled = || cancel.is_some_and(|c| c.load(Ordering::SeqCst));

    loop {
        if cancelled() {
            kill_host(&mut child);
            return format!("$ {cmd}\nHOST_RECEIPT: halted\n{out_buf}");
        }
        if start.elapsed() > timeout {
            kill_host(&mut child);
            return format!("$ {cmd}\nHOST_RECEIPT: timed out\n{out_buf}");
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((is_err, line)) => {
                on_line(&line);
                if is_err {
                    err_buf.push_str(&line);
                    err_buf.push('\n');
                } else {
                    out_buf.push_str(&line);
                    out_buf.push('\n');
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(Some(_)) = child.try_wait() {
                    while let Ok((is_err, line)) = rx.try_recv() {
                        on_line(&line);
                        if is_err {
                            err_buf.push_str(&line);
                            err_buf.push('\n');
                        } else {
                            out_buf.push_str(&line);
                            out_buf.push('\n');
                        }
                    }
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = child.wait();
                break;
            }
        }
    }

    if cancelled() {
        kill_host(&mut child);
        return format!("$ {cmd}\nHOST_RECEIPT: halted\n{out_buf}");
    }
    let status = match child.try_wait() {
        Ok(Some(s)) => s.code().unwrap_or(-1),
        _ => match child.wait() {
            Ok(s) => s.code().unwrap_or(-1),
            Err(_) => -1,
        },
    };
    format!(
        "$ {cmd}\nexit {status} · {}ms\n{out_buf}{}",
        start.elapsed().as_millis(),
        if err_buf.is_empty() {
            String::new()
        } else {
            format!("[stderr]\n{err_buf}")
        }
    )
}

fn kill_host(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_ok() {
        let out = run_host("echo grokhub-smoke", Duration::from_secs(5));
        assert!(out.contains("grokhub-smoke"), "{out}");
        assert!(out.contains("exit 0"), "{out}");
        let mut lines = Vec::new();
        let streamed = run_host_stream("printf 'a\\nb\\n'", Duration::from_secs(5), None, |l| {
            lines.push(l.to_string());
        });
        assert!(streamed.contains("exit 0"), "{streamed}");
        assert!(lines.contains(&"a".to_string()) || streamed.contains("a"), "{streamed:?} {lines:?}");
    }

    #[test]
    fn halt_kills_a_sleeping_host_cmd() {
        use std::sync::Arc;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = stop.clone();
        let started = Instant::now();
        let handle = std::thread::spawn(move || {
            run_host_stream("sleep 8", Duration::from_secs(20), Some(&stop_t), |_| {})
        });
        std::thread::sleep(Duration::from_millis(250));
        stop.store(true, Ordering::SeqCst);
        let out = handle.join().expect("host thread");
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "halt left sleep running for {:?}",
            started.elapsed()
        );
        assert!(out.contains("halted"), "{out}");
    }
}
