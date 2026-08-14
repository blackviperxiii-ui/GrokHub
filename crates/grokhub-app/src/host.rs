use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn run_host(cmd: &str, timeout: Duration) -> String {
    run_host_stream(cmd, timeout, |_| {})
}

pub fn run_host_stream(cmd: &str, timeout: Duration, mut on_line: impl FnMut(&str)) -> String {
    let start = Instant::now();
    let mut child = match Command::new("bash")
        .arg("-lc")
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return format!("$ {cmd}\nspawn failed: {e}"),
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let mut out_buf = String::new();
    if let Some(so) = stdout {
        for line in BufReader::new(so).lines().flatten() {
            on_line(&line);
            out_buf.push_str(&line);
            out_buf.push('\n');
            if start.elapsed() > timeout {
                let _ = child.kill();
                return format!("$ {cmd}\nHOST_RECEIPT: timed out\n{out_buf}");
            }
        }
    }
    let mut err_buf = String::new();
    if let Some(se) = stderr {
        for line in BufReader::new(se).lines().flatten() {
            on_line(&line);
            err_buf.push_str(&line);
            err_buf.push('\n');
        }
    }
    let status = match child.wait() {
        Ok(s) => s.code().unwrap_or(-1),
        Err(_) => -1,
    };
    if start.elapsed() > timeout {
        let _ = child.kill();
        return format!("$ {cmd}\nHOST_RECEIPT: timed out\n{out_buf}");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_ok() {
        let out = run_host("echo grokhub-smoke", Duration::from_secs(5));
        assert!(out.contains("grokhub-smoke"), "{out}");
        assert!(out.contains("exit 0"), "{out}");
        let mut lines = Vec::new();
        let streamed = run_host_stream("printf 'a\\nb\\n'", Duration::from_secs(5), |l| {
            lines.push(l.to_string());
        });
        assert!(streamed.contains("exit 0"), "{streamed}");
        assert!(lines.contains(&"a".to_string()) || streamed.contains("a"), "{streamed:?} {lines:?}");
    }
}
