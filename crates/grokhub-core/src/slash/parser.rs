//! Splits a composer line into its command word and argument, then asks the
//! registry before the commands still in the match in `mod.rs`.

use super::{parse_legacy, registry, Slash};

pub fn parse_slash(line: &str) -> Option<Slash> {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix("$ ") {
        let cmd = rest.trim();
        if cmd.is_empty() {
            return None;
        }
        return Some(Slash::Sh(cmd.to_string()));
    }
    if !t.starts_with('/') {
        return None;
    }
    let mut parts = t.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest = parts.next().unwrap_or("").trim();
    if let Some(entry) = registry::lookup(&cmd) {
        return (entry.parse)(rest);
    }
    parse_legacy(&cmd, rest)
}
