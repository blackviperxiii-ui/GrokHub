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
        || l.contains(">>")
        || l.contains("sed -i")
        || l.contains("truncate")
        || l.split_whitespace().any(|w| matches!(w, "mv" | "cp" | "install" | "dd"))
        || l.contains("cat >")
        || l.contains("cat>")
        || l.contains(">")
}

fn write_dest(c: &str) -> Option<String> {
    if let Some(rest) = c.split('>').nth(1) {
        let p = rest.trim().split_whitespace().next().unwrap_or("");
        if !p.is_empty() && p != "&1" && p != "&2" {
            return Some(p.to_string());
        }
    }
    let bits: Vec<&str> = c.split_whitespace().collect();
    for (i, w) in bits.iter().enumerate() {
        if matches!(*w, "tee" | "mv" | "cp") {
            let dest = bits[i + 1..]
                .iter()
                .find(|x| !x.starts_with('-'))
                .copied()
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
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .last()
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
        assert_eq!(last_host_line("a\n  compiling cabin  \n"), "compiling cabin");
        assert_eq!(host_status_line("make", "compiling cabin", 3), "host: compiling cabin");
        assert!(host_status_line("sleep 9", "", 4).contains("4s"));
        let d = unified_diff_cite("/tmp/a", "old\nkeep\n", "new\nkeep\n");
        assert!(d.contains("- old"));
        assert!(d.contains("+ new"));
    }
}
