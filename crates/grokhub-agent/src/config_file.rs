//! The flat `key = value` files in the config folder (`batched_tools.toml`,
//! `shell_sandbox.toml` and the rest), read one way.
//!
//! `#` starts a comment, blank lines are skipped, a value may be quoted, and
//! the last line for a key wins. A switch takes true or false (also on/off,
//! yes/no, 1/0). Anything else, such as a typo like `enabled = flase`, keeps
//! the switch's default and says so once on stderr, rather than quietly
//! reading as on.

use std::collections::HashSet;
use std::sync::Mutex;

/// One config file's pairs, in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigFile {
    name: String,
    pairs: Vec<(String, String)>,
}

/// The pairs in `text`, for the file `name` (used in warnings).
pub fn parse(name: &str, text: &str) -> ConfigFile {
    let pairs = text
        .lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or("").trim();
            let (key, value) = line.split_once('=')?;
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            (!key.is_empty()).then(|| (key, value.trim().to_string()))
        })
        .collect();
    ConfigFile {
        name: name.to_string(),
        pairs,
    }
}

impl ConfigFile {
    /// The value of `key` (any case), last one wins.
    pub fn get(&self, key: &str) -> Option<&str> {
        let key = key.to_ascii_lowercase();
        self.pairs
            .iter()
            .rev()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The switch `key`, or `default` when it's absent or not a yes or no.
    pub fn flag(&self, key: &str, default: bool) -> bool {
        let Some(value) = self.get(key) else {
            return default;
        };
        match parse_bool(value) {
            Some(on) => on,
            None => {
                self.warn(key, value, if default { "on" } else { "off" });
                default
            }
        }
    }

    /// The whole number `key`, at least `min`, or `default`.
    pub fn count(&self, key: &str, min: usize, default: usize) -> usize {
        let Some(value) = self.get(key) else {
            return default;
        };
        match value.parse::<usize>() {
            Ok(n) => n.max(min),
            Err(_) => {
                self.warn(key, value, &default.to_string());
                default
            }
        }
    }

    /// Say once per file, key and value that a value was not understood.
    fn warn(&self, key: &str, value: &str, kept: &str) {
        static SEEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);
        let id = format!("{}\u{0}{key}\u{0}{value}", self.name);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if seen.get_or_insert_with(HashSet::new).insert(id) {
            eprintln!(
                "grokhub config: {} says {key} = {value}, which isn't a value it takes; kept {kept}",
                self.name
            );
        }
    }
}

/// true/false, on/off, yes/no, 1/0, in any case.
pub fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typo_keeps_the_default_instead_of_reading_as_on() {
        let off_by_default = parse("t.toml", "enabled = flase\n");
        assert!(!off_by_default.flag("enabled", false));
        let on_by_default = parse("t.toml", "enabled = flase\n");
        assert!(on_by_default.flag("enabled", true));
        // A real "off" is off, however it's spelled.
        for off in ["false", "FALSE", "\"false\"", "off", "no", "0"] {
            let file = parse("t.toml", &format!("enabled = {off}"));
            assert!(!file.flag("enabled", true), "{off}");
        }
        for on in ["true", "On", "'yes'", "1"] {
            let file = parse("t.toml", &format!("enabled = {on}"));
            assert!(file.flag("enabled", false), "{on}");
        }
    }

    #[test]
    fn comments_case_quotes_and_the_last_line_are_handled() {
        let file = parse(
            "t.toml",
            "# enabled = false\nEnabled = true # trailing\nbackend = \"summary\"\nenabled = false\nmin = 0\nbad = x\n",
        );
        assert!(!file.flag("enabled", true), "the last line wins");
        assert_eq!(file.get("BACKEND"), Some("summary"));
        assert_eq!(file.count("min", 1, 9), 1, "clamped to the minimum");
        assert_eq!(file.count("bad", 1, 9), 9);
        assert_eq!(file.count("missing", 1, 9), 9);
    }
}
