//! Every slash command, as a table. Each `cmds/*.rs` file declares its
//! commands with `register_slash!`; list them in [`SLASH_REGISTRY`].

use super::{cmds, Slash, SlashEffect};

/// One registered command: its names, what running it does, and its parser.
pub struct SlashEntry {
    /// `/name` and any aliases, lowercase.
    pub names: &'static [&'static str],
    /// The strongest effect any line it parses can have (`/record stop` is
    /// `Local`, but `/record` starts a recording, so the entry is `Action`).
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

pub const SLASH_REGISTRY: &[SlashEntry] = &[
    cmds::help::HELP,
    cmds::memory::FORGET,
    cmds::memory::MEMORY,
    cmds::memory::MEM,
    cmds::project::PROJECT,
    cmds::changes::SKILLS,
    cmds::changes::CONNECTIONS,
    cmds::changes::AUTOMATIONS,
    cmds::changes::WORKFLOW,
    cmds::review::WHY,
    cmds::review::REWIND,
    cmds::review::EXPORT,
    cmds::actions::RECORD,
    cmds::chat::BTW,
    cmds::chat::BG,
    cmds::chat::QUEUE,
    cmds::chat::VIEW_PLAN,
    cmds::chat::TITLE,
    cmds::chat::COMPACT,
    cmds::chat::NEW,
    cmds::chat::SCRATCH,
    cmds::chat::CLEAR,
    cmds::chat::UNDO,
    cmds::chat::RETRY,
    cmds::chat::STOP,
    cmds::chat::RENAME,
    cmds::chat::PIN,
    cmds::chat::DELETE,
    cmds::chat::CONTEXT,
    cmds::views::BOARD,
    cmds::views::JOURNAL,
    cmds::views::WORKTREE,
    cmds::views::WORKFLOWS,
    cmds::views::PLUGINS,
    cmds::views::HOOKS,
    cmds::views::DASHBOARD,
    cmds::views::STATUS,
    cmds::views::LEARN,
    cmds::views::HOST,
    cmds::views::HEALTH,
    cmds::views::BRIEF,
    cmds::views::FIX,
    cmds::views::DREAM,
    cmds::views::IMPORT,
    cmds::views::USAGE,
    cmds::views::MODELS,
    cmds::views::PALETTE,
    cmds::views::EFFORT,
    cmds::views::SESSIONS,
    cmds::views::INSPECT,
    cmds::views::PRIVACY,
    cmds::views::DIAGNOSE,
    cmds::views::HUB,
    cmds::turns::RECALL,
    cmds::turns::IMAGINE,
    cmds::turns::IMAGINE_VIDEO,
    cmds::turns::LOOP,
    cmds::turns::GOAL,
    cmds::turns::SKILL,
    cmds::turns::REMEMBER,
    cmds::turns::CONSULT,
    cmds::turns::SEND,
    cmds::turns::AUDIOCHECK,
    cmds::turns::ROOM,
    cmds::actions::UPDATE,
    cmds::actions::SH,
    cmds::actions::PLAN,
    cmds::actions::ALWAYS_APPROVE,
    cmds::actions::AUTO,
    cmds::actions::SYNC,
    cmds::actions::INHABIT,
    cmds::settings::MODEL,
    cmds::settings::MODE,
];

pub(super) fn lookup(cmd: &str) -> Option<&'static SlashEntry> {
    SLASH_REGISTRY.iter().find(|e| e.names.contains(&cmd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slash::{parse_slash, slash_effect, SLASH_COMMANDS};

    #[test]
    fn registered_commands_parse_their_sample_and_keep_their_effect() {
        let mut seen = std::collections::HashSet::new();
        for e in SLASH_REGISTRY {
            for name in e.names {
                assert!(name.starts_with('/'), "{name}");
                assert_eq!(*name, name.to_ascii_lowercase(), "{name}");
                assert!(seen.insert(*name), "{name} registered twice");
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
        assert!(lookup("/helpme").is_none());
    }

    #[test]
    fn every_picker_row_and_retired_verb_resolves_as_before() {
        for row in SLASH_COMMANDS {
            let word = row.cmd.split_whitespace().next().unwrap_or("");
            if !word.starts_with('/') {
                continue; // `$ <cmd>` is the shell prefix, not a command word
            }
            assert!(
                lookup(word).is_some(),
                "picker row {word} has no registry entry"
            );
        }
        assert_eq!(parse_slash("/approve on"), None);
        assert_eq!(parse_slash("/approve"), None);
        assert_eq!(parse_slash("/connections nope"), None);
        assert_eq!(parse_slash("/why nope"), None);
        assert_eq!(parse_slash("/project nope"), None);
        assert_eq!(parse_slash("/project new  "), None);
    }
}
