//! Show-the-write + live host status pill.

pub fn cite_host_path(p: &str) -> String {
    p.trim().to_string()
}

pub fn summarize_write(cmd: &str, stdout: &str) -> Option<String> {
    let c = cmd.trim();
    if c.is_empty() {
        return None;
    }
    let writeish = is_write_cmd(c);
    if !writeish {
        return None;
    }
    let lower = stdout.to_ascii_lowercase();
    if let Some(rest) = lower.split("wrote ").nth(1) {
        if let Some((n, path)) = rest.split_once(" bytes to ") {
            let n = n.trim();
            let path = path.split_whitespace().next().unwrap_or("").trim();
            if !n.is_empty() && !path.is_empty() {
                return Some(format!("wrote {n} bytes to {}", cite_host_path(path)));
            }
        }
    }
    if let Some(dest) = write_dest(c) {
        return Some(format!("wrote to {}", cite_host_path(&dest)));
    }
    Some(format!("wrote via `{}`", c.chars().take(80).collect::<String>()))
}

fn is_write_cmd(c: &str) -> bool {
    let l = c.to_ascii_lowercase();
    l.contains("tee")
        || l.contains("sed -i")
        || l.contains("truncate")
        || l.split_whitespace().any(|w| matches!(w, "mv" | "cp" | "install" | "dd"))
        || redirect_file_dest(c).is_some()
}

fn redirect_file_dest(c: &str) -> Option<String> {
    let bytes = c.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'>' {
            let mut j = i + 1;
            if j < bytes.len() && bytes[j] == b'>' {
                j += 1;
            }
            let rest = c[j..].trim();
            let rest = rest.strip_prefix('&').unwrap_or(rest);
            let tok = rest.split_whitespace().next().unwrap_or("");
            if !tok.is_empty() && !tok.chars().all(|ch| ch.is_ascii_digit()) {
                return Some(tok.to_string());
            }
        }
        i += 1;
    }
    None
}

fn write_dest(c: &str) -> Option<String> {
    if let Some(p) = redirect_file_dest(c) {
        return Some(p);
    }
    let bits: Vec<&str> = c.split_whitespace().collect();
    for (i, w) in bits.iter().enumerate() {
        if *w == "dd" {
            if let Some(of) = bits[i + 1..]
                .iter()
                .find_map(|x| x.strip_prefix("of="))
                .filter(|p| !p.is_empty())
            {
                return Some(of.to_string());
            }
        }
        if matches!(*w, "tee" | "mv" | "cp" | "install") {
            let args: Vec<&str> = bits[i + 1..]
                .iter()
                .copied()
                .filter(|x| !x.starts_with('-'))
                .collect();
            let dest = if *w == "tee" {
                args.first().copied()
            } else {
                args.last().copied()
            }
            .unwrap_or("");
            if !dest.is_empty() {
                return Some(dest.to_string());
            }
        }
    }
    bits.iter()
        .find(|p| p.starts_with("/tmp/") || p.starts_with("/home/"))
        .map(|s| (*s).to_string())
}

pub fn last_host_line(chunk: &str) -> String {
    chunk
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).rfind(|l| !l.is_empty())
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect()
}

pub fn unified_diff_cite(path: &str, before: &str, after: &str) -> String {
    let mut out = format!("diff — {}\n", cite_host_path(path));
    let b: Vec<&str> = before.lines().collect();
    let a: Vec<&str> = after.lines().collect();
    let n = b.len().max(a.len()).min(40);
    for i in 0..n {
        let left = b.get(i).copied().unwrap_or("");
        let right = a.get(i).copied().unwrap_or("");
        if left == right {
            continue;
        }
        if !left.is_empty() {
            out.push_str(&format!("- {left}\n"));
        }
        if !right.is_empty() {
            out.push_str(&format!("+ {right}\n"));
        }
    }
    if out.lines().count() == 1 {
        out.push_str("(no line diff)\n");
    }
    out
}

pub fn host_status_line(cmd: &str, last_line: &str, elapsed_sec: u64) -> String {
    let line = last_host_line(last_line);
    if !line.is_empty() {
        return format!("host: {}", line.chars().take(80).collect::<String>());
    }
    let label: String = cmd.chars().take(56).collect();
    format!("Host: {label}… ({elapsed_sec}s)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_cite_and_pill() {
        assert_eq!(
            summarize_write("cat > /tmp/a.txt", "wrote 12 bytes to /tmp/a.txt").as_deref(),
            Some("wrote 12 bytes to /tmp/a.txt")
        );
        assert_eq!(
            summarize_write("tee /tmp/out", "").as_deref(),
            Some("wrote to /tmp/out")
        );
        assert!(summarize_write("ls /tmp", "").is_none());
        assert!(
            summarize_write("ls 2>&1", "").is_none(),
            "stderr-to-stdout is not a file write"
        );
        assert!(
            summarize_write("echo hi 2>&1", "").is_none(),
            "fd redirects must not cite a write"
        );
        assert_eq!(
            summarize_write("cp src.txt dest.txt", "").as_deref(),
            Some("wrote to dest.txt"),
            "cp must cite the destination, not the source"
        );
        assert_eq!(
            summarize_write("cp -a src.txt dest.txt", "").as_deref(),
            Some("wrote to dest.txt")
        );
        assert_eq!(
            summarize_write("mv old.txt new.txt", "").as_deref(),
            Some("wrote to new.txt"),
            "mv must cite the destination, not the source"
        );
        assert_eq!(
            summarize_write("dd if=/home/j/a of=/tmp/b", "").as_deref(),
            Some("wrote to /tmp/b"),
            "dd must cite of= dest, not if= source"
        );
        assert_eq!(
            summarize_write("install src.bin dest.bin", "").as_deref(),
            Some("wrote to dest.bin"),
            "install must cite the destination, not the source"
        );
        assert_eq!(
            summarize_write("install /home/j/src/foo /tmp/foo", "").as_deref(),
            Some("wrote to /tmp/foo")
        );
        assert_eq!(last_host_line("a\n  compiling cabin  \n"), "compiling cabin");
        assert_eq!(host_status_line("make", "compiling cabin", 3), "host: compiling cabin");
        assert!(host_status_line("sleep 9", "", 4).contains("4s"));
        let d = unified_diff_cite("/tmp/a", "old\nkeep\n", "new\nkeep\n");
        assert!(d.contains("- old"));
        assert!(d.contains("+ new"));
    }
}
