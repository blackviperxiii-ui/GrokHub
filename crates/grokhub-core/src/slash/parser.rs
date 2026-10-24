//! Splits a composer line into its command word and argument, then looks the
//! command up in the registry. Unknown commands parse to `None`.

use super::{registry, Slash};

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
    registry::lookup(&cmd).and_then(|entry| (entry.parse)(rest))
}
