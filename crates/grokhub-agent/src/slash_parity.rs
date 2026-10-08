// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.
//! Every Grok CLI builtin slash command, in pager menu order, and what a
//! native thread does with it.
//!
//! The names are `builtin_commands()` in `xai-grok-pager` `slash/commands/mod.rs`.
//! A row is `native:` when the cabin handles it on a native thread, or `N/A:`
//! when that command stays a pager or account feature. CLI threads never read
//! this table.

/// One CLI slash command and its native-thread disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashParity {
    pub command: &'static str,
    pub disposition: &'static str,
}

/// Pager builtin names, in menu order. `quit` is the exit command's name.
const CLI_SLASH_COMMANDS: &[&str] = &[
    "tutorial",
    "settings",
    "dashboard",
    "workflows",
    "plugins",
    "btw",
    "voice",
    "new",
    "effort",
    "context-window",
    "model",
    "context",
    "compact",
    "fork",
    "resume",
    "loop",
    "plan",
    "view-plan",
    "remember",
    "memory",
    "flush",
    "dream",
    "recap",
    "rewind",
    "jump",
    "expand",
    "edit-prompt",
    "queue",
    "session-info",
    "share",
    "rename",
    "history",
    "transcript",
    "export",
    "copy",
    "find",
    "usage",
    "tasks",
    "skills",
    "mcps",
    "hooks",
    "marketplace",
    "workflow",
    "personas",
    "config-agents",
    "theme",
    "auto",
    "always-approve",
    "vim-mode",
    "multiline",
    "compact-mode",
    "timestamps",
    "toggle-mouse-reporting",
    "minimal",
    "fullscreen",
    "timeline",
    "cd",
    "imagine",
    "imagine-video",
    "docs",
    "release-notes",
    "announcements",
    "feedback",
    "privacy",
    "doctor",
    "import-claude",
    "login",
    "logout",
    "home",
    "delete",
    "help",
    "quit",
    "gboom",
    "debug",
];

const SLASH_PARITY: &[SlashParity] = &[
    row("tutorial", "N/A: pager onboarding"),
    row("settings", "native: open Settings"),
    row("dashboard", "native: History"),
    row("workflows", "native: Skills, Workflows section"),
    row("plugins", "native: Connectors"),
    row("btw", "native: side ask"),
    row(
        "voice",
        "N/A: the cabin mic is separate from the CLI voice command",
    ),
    row("new", "native: new chat"),
    row("effort", "native: effort picker"),
    row(
        "context-window",
        "native: context length in the status line",
    ),
    row("model", "native: model catalog"),
    row("context", "native: turn and token status"),
    row("compact", "native: compact the native session"),
    row("fork", "N/A: fork is not in the cabin"),
    row("resume", "native: History"),
    row("loop", "native: Automations"),
    row("plan", "native: plan mode"),
    row("view-plan", "native: open the stored plan"),
    row("remember", "native: append MEMORY.md and index it"),
    row("memory", "native: Memory page"),
    row("flush", "native: write pending memory before compaction"),
    row("dream", "native: one low-effort rewrite of MEMORY.md"),
    row("recap", "native: last user lines in the status line"),
    row("rewind", "N/A: native sessions are append-only"),
    row("jump", "N/A: pager transcript jump"),
    row("expand", "N/A: pager transcript expand"),
    row("edit-prompt", "N/A: pager prompt editor"),
    row("queue", "native: hold the next send"),
    row("session-info", "native: session, skills, hooks, and MCP"),
    row("share", "N/A: CLI share link"),
    row("rename", "native: rename the thread"),
    row("history", "native: History"),
    row("transcript", "native: session id and path"),
    row("export", "native: export the chat"),
    row("copy", "native: last reply in the status line"),
    row("find", "N/A: pager find"),
    row("usage", "native: cabin usage"),
    row("tasks", "native: task cards on the running turn"),
    row("skills", "native: Skills"),
    row("mcps", "native: Connectors"),
    row("hooks", "native: Hooks"),
    row("marketplace", "native: Connectors"),
    row("workflow", "N/A: workflows stay on the Grok CLI"),
    row("personas", "N/A: CLI personas"),
    row("config-agents", "N/A: CLI agent config"),
    row("theme", "N/A: pager theme; Appearance is separate"),
    row("auto", "native: permission auto"),
    row("always-approve", "native: always approve"),
    row("vim-mode", "N/A: pager vim"),
    row("multiline", "N/A: pager input mode"),
    row("compact-mode", "N/A: pager compact display"),
    row("timestamps", "N/A: pager timestamps"),
    row("toggle-mouse-reporting", "N/A: pager mouse reporting"),
    row("minimal", "N/A: pager screen mode"),
    row("fullscreen", "N/A: pager screen mode"),
    row("timeline", "N/A: pager timeline"),
    row("cd", "N/A: bind a project with /project"),
    row("imagine", "native: Imagine"),
    row("imagine-video", "native: Imagine video"),
    row("docs", "N/A: CLI docs"),
    row("release-notes", "N/A: CLI release notes"),
    row("announcements", "N/A: CLI announcements"),
    row("feedback", "N/A: CLI feedback"),
    row("privacy", "native: cabin /privacy (grants, scopes, egress log)"),
    row("doctor", "native: session, skills, hooks, and MCP"),
    row("import-claude", "N/A: cabin /import is OpenClaw"),
    row("login", "N/A: sign in from Settings"),
    row("logout", "N/A: sign out from Settings"),
    row("home", "N/A: pager home"),
    row("delete", "native: delete the thread"),
    row("help", "native: slash help"),
    row("quit", "N/A: close the window"),
    row("gboom", "N/A: pager easter egg"),
    row("debug", "N/A: pager debug toggles"),
];

const fn row(command: &'static str, disposition: &'static str) -> SlashParity {
    SlashParity {
        command,
        disposition,
    }
}

/// The full table, pager menu order, one row per CLI command.
pub fn slash_parity() -> &'static [SlashParity] {
    SLASH_PARITY
}

/// A CLI slash the cabin parser does not already own.
///
/// `None` when the line is not a slash, the cabin already parses it, or the
/// token is not in the CLI builtin list (`/create-skill`, `/bg`, and similar
/// stay on their existing path).
pub fn unparsed_native_slash(line: &str) -> Option<UnparsedSlash> {
    let line = line.trim();
    if !line.starts_with('/') || grokhub_core::parse_slash(line).is_some() {
        return None;
    }
    let (cmd, args) = command_and_args(line)?;
    Some(match cmd.as_str() {
        "flush" if args.is_empty() => UnparsedSlash::Flush,
        "flush" => UnparsedSlash::Note("native: /flush takes no arguments"),
        "context-window" => UnparsedSlash::ContextWindow,
        "copy" => UnparsedSlash::Copy,
        "tasks" => UnparsedSlash::Tasks,
        "history" => UnparsedSlash::History,
        "transcript" => UnparsedSlash::Transcript,
        "recap" => UnparsedSlash::Recap,
        "settings" => UnparsedSlash::Settings,
        "memory" => UnparsedSlash::Memory,
        "model" => UnparsedSlash::ModelCatalog,
        "effort" => UnparsedSlash::EffortHint,
        "remember" => UnparsedSlash::Note("native: /remember needs a note"),
        "rename" => UnparsedSlash::Note("native: /rename needs a title"),
        "queue" => UnparsedSlash::Note("native: /queue needs a message"),
        other => UnparsedSlash::Note(disposition_of(other)),
    })
}

fn disposition_of(command: &str) -> &'static str {
    SLASH_PARITY
        .iter()
        .find(|row| row.command == command)
        .map(|row| row.disposition)
        .unwrap_or("N/A: not a cabin command")
}

fn command_and_args(line: &str) -> Option<(String, &str)> {
    let rest = line.trim().strip_prefix('/')?;
    let mut parts = rest.splitn(2, char::is_whitespace);
    let raw = parts.next()?.trim();
    if raw.is_empty() {
        return None;
    }
    let mut cmd = raw.to_ascii_lowercase();
    if cmd == "full" {
        cmd = "fullscreen".to_string();
    }
    if !CLI_SLASH_COMMANDS.contains(&cmd.as_str()) {
        return None;
    }
    let args = parts.next().unwrap_or("").trim();
    Some((cmd, args))
}

/// Cheap native handling for a CLI slash the cabin parser left alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnparsedSlash {
    Flush,
    ContextWindow,
    Copy,
    Tasks,
    History,
    Transcript,
    Recap,
    Settings,
    Memory,
    ModelCatalog,
    EffortHint,
    /// Show this disposition. It starts with `native:` or `N/A:`.
    Note(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_table_covers_every_cli_command_once() {
        let rows = slash_parity();
        assert_eq!(rows.len(), CLI_SLASH_COMMANDS.len());
        assert_eq!(rows.len(), 74);
        let mut seen = std::collections::BTreeSet::new();
        for (row, expected) in rows.iter().zip(CLI_SLASH_COMMANDS.iter().copied()) {
            assert_eq!(row.command, expected);
            assert!(seen.insert(row.command), "duplicate {}", row.command);
            let rest = row
                .disposition
                .strip_prefix("native: ")
                .or_else(|| row.disposition.strip_prefix("N/A: "));
            assert!(
                rest.is_some_and(|text| !text.trim().is_empty()),
                "{}",
                row.disposition
            );
        }
        assert_eq!(seen.len(), CLI_SLASH_COMMANDS.len());
    }

    #[test]
    fn unparsed_slash_skips_cabin_commands_and_unknown_tokens() {
        assert!(unparsed_native_slash("/compact").is_none());
        assert!(unparsed_native_slash("/remember the harbor").is_none());
        assert!(unparsed_native_slash("/bg write a file").is_none());
        assert!(unparsed_native_slash("/create-skill").is_none());
        assert!(unparsed_native_slash("hello").is_none());
        assert_eq!(unparsed_native_slash("/flush"), Some(UnparsedSlash::Flush));
        assert_eq!(
            unparsed_native_slash("/flush now"),
            Some(UnparsedSlash::Note("native: /flush takes no arguments"))
        );
        assert!(matches!(
            unparsed_native_slash("/voice"),
            Some(UnparsedSlash::Note(text)) if text.starts_with("N/A:")
        ));
        assert_eq!(
            unparsed_native_slash("/fork"),
            Some(UnparsedSlash::Note("N/A: fork is not in the cabin"))
        );
        assert_eq!(
            unparsed_native_slash("/full"),
            unparsed_native_slash("/fullscreen")
        );
    }
}
