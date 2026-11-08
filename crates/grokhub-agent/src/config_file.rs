//! The flat `key = value` files in the config folder (`batched_tools.toml`,
//! `shell_sandbox.toml` and the rest), read one way.
//!
//! `#` starts a comment outside quotes, blank lines are skipped, a value may
//! be quoted, and the last line for a key wins. A switch takes true or false
//! (also on/off, yes/no, 1/0). Anything the reader can't use, such as a typo
//! like `enabled = flase`, a key the file doesn't have, a `[section]` or a
//! line that isn't `key = value`, keeps the default and is reported: once on
//! stderr, and in [`config_warnings`], which the app shows with its status
//! checks so a typo never quietly does nothing.

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

/// One config file's pairs, in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigFile {
    name: String,
    pairs: Vec<(String, String)>,
}

/// The latest warnings for each file, by file name. A file's entry is
/// replaced each time it is read, so a fixed file stops showing.
static WARNINGS: Mutex<BTreeMap<String, Vec<String>>> = Mutex::new(BTreeMap::new());

/// Every config file problem found by the latest read of each file.
pub fn config_warnings() -> Vec<String> {
    WARNINGS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .flatten()
        .cloned()
        .collect()
}

/// `line` up to a `#` that is outside quotes.
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '#') => return &line[..i],
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            _ => {}
        }
    }
    line
}

/// The pairs in `text`, for the file `name`, which takes the keys in `known`.
pub fn parse(name: &str, text: &str, known: &[&str]) -> ConfigFile {
    let mut pairs = Vec::new();
    let mut warnings = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let n = n + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            if line.starts_with('[') && line.ends_with(']') {
                warnings.push(format!(
                    "{name} line {n}: sections like {line} aren't read; put its keys at the top"
                ));
            } else {
                warnings.push(format!(
                    "{name} line {n} isn't key = value, so it was skipped"
                ));
            }
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if key.is_empty() {
            warnings.push(format!("{name} line {n} has no key, so it was skipped"));
            continue;
        }
        if !known.iter().any(|k| k.eq_ignore_ascii_case(&key)) {
            warnings.push(format!(
                "{name} has {key}, which isn't one of its settings ({})",
                known.join(", ")
            ));
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        pairs.push((key, value.trim().to_string()));
    }
    for warning in &warnings {
        say_once(warning);
    }
    WARNINGS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(name.to_string(), warnings);
    ConfigFile {
        name: name.to_string(),
        pairs,
    }
}

/// Print `warning` on stderr the first time it comes up in this process.
fn say_once(warning: &str) {
    static SEEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if seen
        .get_or_insert_with(HashSet::new)
        .insert(warning.to_string())
    {
        eprintln!("grokhub config: {warning}");
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

    /// The value of `key` when it is one of `options` (any case), else
    /// None, reported when the key was set to something else.
    pub fn one_of<'a>(&'a self, key: &str, options: &[&'a str], default: &str) -> Option<&'a str> {
        let value = self.get(key)?;
        match options.iter().find(|o| o.eq_ignore_ascii_case(value)) {
            Some(option) => Some(option),
            None => {
                self.warn(key, value, default);
                None
            }
        }
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

    /// Report that a value was not understood, under this file's entry.
    fn warn(&self, key: &str, value: &str, kept: &str) {
        let warning = format!(
            "{} says {key} = {value}, which isn't a value it takes; kept {kept}",
            self.name
        );
        say_once(&warning);
        let mut all = WARNINGS.lock().unwrap_or_else(|e| e.into_inner());
        let entry = all.entry(self.name.clone()).or_default();
        if !entry.contains(&warning) {
            entry.push(warning);
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

    /// The warnings shown for the file `name`.
    fn shown(name: &str) -> Vec<String> {
        config_warnings()
            .into_iter()
            .filter(|w| w.starts_with(name))
            .collect()
    }

    #[test]
    fn a_typo_keeps_the_default_instead_of_reading_as_on() {
        let off_by_default = parse("t.toml", "enabled = flase\n", &["enabled"]);
        assert!(!off_by_default.flag("enabled", false));
        let on_by_default = parse("t.toml", "enabled = flase\n", &["enabled"]);
        assert!(on_by_default.flag("enabled", true));
        // A real "off" is off, however it's spelled.
        for off in ["false", "FALSE", "\"false\"", "off", "no", "0"] {
            let file = parse("t.toml", &format!("enabled = {off}"), &["enabled"]);
            assert!(!file.flag("enabled", true), "{off}");
        }
        for on in ["true", "On", "'yes'", "1"] {
            let file = parse("t.toml", &format!("enabled = {on}"), &["enabled"]);
            assert!(file.flag("enabled", false), "{on}");
        }
    }

    #[test]
    fn comments_case_quotes_and_the_last_line_are_handled() {
        let file = parse(
            "clean-test.toml",
            "# enabled = false\nEnabled = true # trailing\nbackend = \"summary\"\nenabled = false\nmin = 0\nbad = x\n",
            &["enabled", "backend", "min", "bad"],
        );
        assert!(shown("clean-test.toml").is_empty());
        assert!(!file.flag("enabled", true), "the last line wins");
        assert_eq!(file.get("BACKEND"), Some("summary"));
        assert_eq!(file.count("min", 1, 9), 1, "clamped to the minimum");
        assert_eq!(file.count("bad", 1, 9), 9);
        assert_eq!(file.count("missing", 1, 9), 9);
    }

    #[test]
    fn a_hash_inside_quotes_is_part_of_the_value() {
        let file = parse(
            "quote-test.toml",
            "url = \"https://x/#a\" # note\ncolour = '#fff'\nlist = [\"~/a#b\", 'c'] # end\n",
            &["url", "colour", "list"],
        );
        assert_eq!(file.get("url"), Some("https://x/#a"));
        assert_eq!(file.get("colour"), Some("#fff"));
        assert_eq!(file.get("list"), Some("[\"~/a#b\", 'c']"));
        assert!(shown("quote-test.toml").is_empty());
    }

    #[test]
    fn unknown_keys_sections_and_stray_lines_are_reported_to_the_app() {
        let name = "warn-test-unknown.toml";
        let file = parse(
            name,
            "[shell]\nenabeld = true\njust words\n= 3\nenabled = maybe\n",
            &["enabled"],
        );
        let found = shown(name).join("\n");
        assert!(found.contains("sections like [shell]"), "{found}");
        assert!(
            found.contains("has enabeld, which isn't one of its settings (enabled)"),
            "{found}"
        );
        assert!(found.contains("line 3 isn't key = value"), "{found}");
        assert!(found.contains("line 4 has no key"), "{found}");
        // A value it can't use is reported when it's read.
        assert!(!file.flag("enabled", false));
        let all = shown(name);
        assert_eq!(all.len(), 5, "{all:?}");
        assert!(all.iter().any(|w| w.contains("enabled = maybe")), "{all:?}");
        // A fixed file stops showing.
        let fixed = parse(name, "enabled = true\n", &["enabled"]);
        assert!(fixed.flag("enabled", false));
        assert!(shown(name).is_empty());
    }

    #[test]
    fn a_choice_outside_its_options_keeps_the_default_and_says_so() {
        let name = "warn-test-choice.toml";
        let file = parse(name, "backend = Summry\n", &["backend"]);
        assert_eq!(file.one_of("backend", &["summary", "server"], "auto"), None);
        assert!(shown(name).iter().any(|w| w.contains("kept auto")));
        let file = parse(name, "backend = SERVER\n", &["backend"]);
        assert_eq!(
            file.one_of("backend", &["summary", "server"], "auto"),
            Some("server")
        );
    }
}
