//! Slash commands that parse through a table instead of the match in `mod.rs`.
//! Each `cmds/*.rs` file declares its commands with `register_slash!`.

use super::{cmds, Slash, SlashEffect};

/// One registered command: its names, what running it does, and its parser.
pub struct SlashEntry {
    /// `/name` and any aliases, lowercase.
    pub names: &'static [&'static str],
    pub effect: SlashEffect,
    /// A line that parses to this command. Tests run it through `parse_slash`.
    pub sample: &'static str,
    /// Parses the argument after the command word (already trimmed).
    pub parse: fn(&str) -> Option<Slash>,
}

/// Declares a `SlashEntry` const in a `cmds/*.rs` file. List it in
/// [`SLASH_REGISTRY`] to make `parse_slash` use it.
macro_rules! register_slash {
    ($(#[$doc:meta])* $id:ident {
        names: [$($name:literal),+ $(,)?],
        effect: $effect:ident,
        sample: $sample:literal,
        parse: $parse:expr $(,)?
    }) => {
        $(#[$doc])*
        pub(in crate::slash) const $id: $crate::slash::SlashEntry = $crate::slash::SlashEntry {
            names: &[$($name),+],
            effect: $crate::slash::SlashEffect::$effect,
            sample: $sample,
            parse: $parse,
        };
    };
}
pub(super) use register_slash;

pub const SLASH_REGISTRY: &[SlashEntry] = &[cmds::help::HELP];

pub(super) fn lookup(cmd: &str) -> Option<&'static SlashEntry> {
    SLASH_REGISTRY.iter().find(|e| e.names.contains(&cmd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slash::{parse_legacy, parse_slash, slash_effect};

    #[test]
    fn registered_commands_parse_their_sample_and_keep_their_effect() {
        let mut seen = std::collections::HashSet::new();
        for e in SLASH_REGISTRY {
            for name in e.names {
                assert!(name.starts_with('/'), "{name}");
                assert_eq!(*name, name.to_ascii_lowercase(), "{name}");
                assert!(seen.insert(*name), "{name} registered twice");
                assert_eq!(
                    parse_legacy(name, ""),
                    None,
                    "{name} is still in the old match"
                );
            }
            let s = parse_slash(e.sample).unwrap_or_else(|| panic!("{} parses", e.sample));
            assert_eq!(slash_effect(&s), e.effect, "{}", e.sample);
        }
    }

    #[test]
    fn help_parses_through_the_registry() {
        assert_eq!(lookup("/help").map(|e| e.names), Some(&["/help"][..]));
        assert_eq!(parse_slash("/help"), Some(Slash::Help));
        assert_eq!(parse_slash("  /HELP  "), Some(Slash::Help));
        assert_eq!(parse_slash("/help me please"), Some(Slash::Help));
        assert_eq!(parse_legacy("/help", ""), None);
        assert!(lookup("/helpme").is_none());
    }
}
