use grokhub_core::TEXT_FILE_CAP;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn push_host_line(buf: &mut String, line: &str, cap: usize) -> bool {
    if buf.len() >= cap {
        return false;
    }
    buf.push_str(line);
    buf.push('\n');
    if buf.len() <= cap {
        return true;
    }
    buf.truncate(cap);
    while !buf.is_empty() && !buf.is_char_boundary(buf.len()) {
        buf.pop();
    }
    false
}

pub fn host_working_dir(project_dir: &str) -> Option<String> {
    let root = grokhub_core::expand_project_root(
        project_dir,
        std::env::var("HOME").ok().as_deref(),
    );
    if root.is_empty() {
        return None;
    }
    let path = Path::new(&root);
    if path.is_dir() {
        return Some(root);
    }
    std::fs::create_dir_all(path).ok()?;
    path.is_dir().then_some(root)
}

pub fn resolve_host_cite_path(project_dir: &str, cited: &str) -> String {
    let cited = cited.trim();
    if cited.is_empty() {
        return String::new();
    }
    let expanded = grokhub_core::expand_project_root(
        cited,
        std::env::var("HOME").ok().as_deref(),
    );
    if expanded.starts_with('/') {
        return expanded;
    }
    match host_working_dir(project_dir) {
        Some(root) => format!(
            "{}/{}",
            root.trim_end_matches('/'),
            expanded.trim_start_matches("./")
        ),
        None => expanded,
    }
}

pub fn run_host(cmd: &str, timeout: Duration) -> String {
    run_host_stream(cmd, timeout, None, None, |_| {})
}

pub fn run_host_stream(
    cmd: &str,
    timeout: Duration,
    cancel: Option<&AtomicBool>,
    cwd: Option<&str>,
    mut on_line: impl FnMut(&str),
) -> String {
    let start = Instant::now();
    let mut spawn = Command::new("bash");
    spawn
        .arg("-lc")
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd.filter(|d| !d.is_empty()) {
        spawn.current_dir(dir);
    }
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
            for line in BufReader::new(so.take(TEXT_FILE_CAP as u64 + 1)).lines().flatten() {
                if tx.send((false, line)).is_err() {
                    break;
                }
            }
        });
    }
    if let Some(se) = stderr {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(se.take(TEXT_FILE_CAP as u64 + 1)).lines().flatten() {
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
                let room = if is_err { &mut err_buf } else { &mut out_buf };
                if !push_host_line(room, &line, TEXT_FILE_CAP) {
                    kill_host(&mut child);
                    return format!("$ {cmd}\nHOST_RECEIPT: output capped\n{out_buf}");
                }
                on_line(&line);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(Some(_)) = child.try_wait() {
                    while let Ok((is_err, line)) = rx.try_recv() {
                        let room = if is_err { &mut err_buf } else { &mut out_buf };
                        if !push_host_line(room, &line, TEXT_FILE_CAP) {
                            kill_host(&mut child);
                            return format!("$ {cmd}\nHOST_RECEIPT: output capped\n{out_buf}");
                        }
                        on_line(&line);
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
        let streamed = run_host_stream("printf 'a\\nb\\n'", Duration::from_secs(5), None, None, |l| {
            lines.push(l.to_string());
        });
        assert!(streamed.contains("exit 0"), "{streamed}");
        assert!(lines.contains(&"a".to_string()) || streamed.contains("a"), "{streamed:?} {lines:?}");
    }

    #[test]
    fn host_working_dir_uses_an_existing_bound_tree() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let dir = std::path::PathBuf::from(&home).join(format!(
            "grokhub-host-cwd-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp project");
        let path = dir.to_string_lossy().into_owned();
        assert_eq!(host_working_dir(""), None);
        assert_eq!(host_working_dir("   "), None);
        assert_eq!(host_working_dir(&path), Some(path.clone()));
        let missing = format!("{path}/missing-bound");
        assert_eq!(
            host_working_dir(&missing),
            Some(missing.clone()),
            "a missing bound tree must be created, not inherit the cabin process cwd"
        );
        assert!(Path::new(&missing).is_dir());
        let rest = path.trim_start_matches(&format!("{home}/"));
        assert_eq!(
            host_working_dir(&format!("~/{rest}")),
            Some(path.clone()),
            "tilde bound paths must expand before cwd"
        );
        assert_eq!(
            host_working_dir(&format!("$HOME/{rest}")),
            Some(path),
            "$HOME bound paths must expand before cwd"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_host_cite_path_joins_relative_writes_to_the_bound_tree() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let dir = std::path::PathBuf::from(&home).join(format!(
            "grokhub-host-cite-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp project");
        let path = dir.to_string_lossy().into_owned();
        assert_eq!(resolve_host_cite_path("", "notes.md"), "notes.md");
        assert_eq!(
            resolve_host_cite_path(&path, "notes.md"),
            format!("{path}/notes.md")
        );
        assert_eq!(
            resolve_host_cite_path(&path, "/tmp/abs.txt"),
            "/tmp/abs.txt"
        );
        let rest = path.trim_start_matches(&format!("{home}/"));
        assert_eq!(
            resolve_host_cite_path(&format!("~/{rest}"), "notes.md"),
            format!("{path}/notes.md"),
            "tilde bound trees must expand before joining a relative write"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_host_stream_starts_in_the_bound_cwd() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-host-pwd-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp project");
        let path = dir.to_string_lossy().into_owned();
        let out = run_host_stream("pwd", Duration::from_secs(5), None, Some(path.as_str()), |_| {});
        let canon = std::fs::canonicalize(&dir).unwrap_or(dir.clone());
        assert!(
            out.contains(&canon.to_string_lossy().into_owned()) || out.contains(&path),
            "host shell must start in the bound project: {out}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn halt_kills_a_sleeping_host_cmd() {
        use std::sync::Arc;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = stop.clone();
        let started = Instant::now();
        let handle = std::thread::spawn(move || {
            run_host_stream("sleep 8", Duration::from_secs(20), Some(&stop_t), None, |_| {})
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

    #[test]
    fn run_host_stream_caps_a_huge_dump() {
        let src = include_str!("host.rs");
        let stream = src
            .split("pub fn run_host_stream(")
            .nth(1)
            .and_then(|s| s.split("fn kill_host(").next())
            .expect("run_host_stream");
        assert!(
            stream.contains("TEXT_FILE_CAP"),
            "a huge host dump must not grow the receipt without bound: {stream}"
        );
        let take = stream.find("take(TEXT_FILE_CAP").expect("pipe take");
        let lines = stream.find(".lines()").expect("line split");
        assert!(
            take < lines && stream.matches("take(TEXT_FILE_CAP").count() >= 2,
            "a newline-free host dump must not slurp the whole pipe into one line: {stream}"
        );
        let out = run_host(
            "python3 -c \"print('x'*200000)\"",
            Duration::from_secs(5),
        );
        assert!(
            out.len() <= grokhub_core::TEXT_FILE_CAP + 256,
            "capped host receipt stayed huge: {}",
            out.len()
        );
        assert!(
            out.contains("output capped") || !out.contains(&"x".repeat(200000)),
            "huge host stdout must not land in the receipt: {out}"
        );
    }
}
