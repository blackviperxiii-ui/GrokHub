//! The composer's slash commands. Typing `/` lists what the shell can run;
//! the cabin's other commands show greyed with where they live. A slash line
//! never reaches the model: it runs here and comes back as an effect the
//! chat applies (a new chat, a mode, a view, or a short notice).

use serde::Serialize;

/// Greyed rows from the cabin's list carry this reason.
pub const IN_THE_CABIN: &str = "In the cabin";
/// A row the paired browser can't run.
pub const ON_YOUR_COMPUTER: &str = "On your computer";
pub const NOT_YET: &str = "Not in this app yet";

/// One command the shell runs.
pub struct ShellCmd {
    pub cmd: &'static str,
    pub hint: &'static str,
    /// What Tab or a click puts in the composer.
    pub insert: &'static str,
    /// Picking it runs it at once (it takes no text).
    pub run_on_pick: bool,
    /// Only the window on this computer can run it.
    pub desktop_only: bool,
    /// Listed but not runnable yet.
    pub ready: bool,
}

const fn row(cmd: &'static str, hint: &'static str, insert: &'static str) -> ShellCmd {
    ShellCmd {
        cmd,
        hint,
        insert,
        run_on_pick: insert.len() == cmd.len(),
        desktop_only: false,
        ready: true,
    }
}

pub const SHELL_SLASH: &[ShellCmd] = &[
    row("/help", "Show these commands", "/help"),
    row("/new", "New chat", "/new"),
    row("/clear", "Clear this chat", "/clear"),
    row("/chat", "Chat mode", "/chat"),
    row(
        "/plan",
        "Plan mode: read only until you approve the plan",
        "/plan",
    ),
    row("/btw", "Side ask: looks, changes nothing", "/btw"),
    row("/remember", "Save a memory note…", "/remember "),
    ShellCmd {
        desktop_only: true,
        ..row("/settings", "Open Settings", "/settings")
    },
    row("/memory", "Open Memory", "/memory"),
    row("/automations", "Open Automations", "/automations"),
    row("/connectors", "Open Connectors", "/connectors"),
    ShellCmd {
        ready: false,
        ..row("/history", "Past chats", "/history")
    },
];

/// One row of the composer's command list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub cmd: String,
    pub hint: String,
    pub insert: String,
    pub run_on_pick: bool,
    /// Why it can't run here, when it can't.
    pub disabled: Option<String>,
}

/// What a slash line asks the chat to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Effect {
    /// `/new` and `/clear`: start a fresh chat.
    NewChat,
    /// `/chat`, `/plan`, `/btw`. Text after the command is sent in that mode.
    Mode {
        mode: &'static str,
        text: Option<String>,
    },
    /// `/settings`, `/memory`, `/automations`, `/connectors`.
    Open { view: &'static str },
    /// A line to show in the chat log: help or a saved note.
    Notice { text: String },
}

/// What `run` needs done outside this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Effect(Effect),
    Remember(String),
}

/// The command word, when `line` is a slash command at all. `/usr/bin` and
/// `/ hi` are messages, not commands.
pub fn command_word(line: &str) -> Option<String> {
    let t = line.trim_start();
    let word = t.split_whitespace().next()?;
    let name = word.strip_prefix('/')?;
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(word.to_ascii_lowercase())
}

/// Whether the composer should run `line` instead of sending it.
pub fn is_command(line: &str) -> bool {
    command_word(line).is_some()
}

fn find(cmd: &str) -> Option<&'static ShellCmd> {
    SHELL_SLASH.iter().find(|c| c.cmd == cmd)
}

fn shell_hit(c: &ShellCmd, remote: bool) -> Hit {
    let disabled = if !c.ready {
        Some(NOT_YET.to_string())
    } else if remote && c.desktop_only {
        Some(ON_YOUR_COMPUTER.to_string())
    } else {
        None
    };
    Hit {
        cmd: c.cmd.into(),
        hint: c.hint.into(),
        insert: c.insert.into(),
        run_on_pick: c.run_on_pick,
        disabled,
    }
}

/// The list for a draft: the shell's commands that match, then the cabin's
/// matching commands greyed. Empty when the draft isn't a command.
pub fn hits(draft: &str, remote: bool) -> Vec<Hit> {
    let t = draft.trim_start();
    if !t.starts_with('/') || t.split_whitespace().count() > 1 || t.ends_with(' ') {
        return Vec::new();
    }
    let needle = t.to_ascii_lowercase();
    let mut out: Vec<Hit> = SHELL_SLASH
        .iter()
        .filter(|c| c.cmd.starts_with(&needle))
        .map(|c| shell_hit(c, remote))
        .collect();
    for cabin in grokhub_core::filter_slash_hits(t, &[]) {
        let word = cabin.cmd.split_whitespace().next().unwrap_or("");
        if find(word).is_some() || out.iter().any(|h| h.cmd == cabin.cmd) {
            continue;
        }
        out.push(Hit {
            cmd: cabin.cmd,
            hint: cabin.hint,
            insert: cabin.insert,
            run_on_pick: false,
            disabled: Some(IN_THE_CABIN.to_string()),
        });
    }
    out.truncate(12);
    out
}

/// The `/help` text: the shell's commands, one per line.
pub fn help() -> String {
    let mut lines: Vec<String> = SHELL_SLASH
        .iter()
        .filter(|c| c.ready)
        .map(|c| format!("{} — {}", c.insert.trim_end(), c.hint.trim_end_matches('…')))
        .collect();
    lines.push("Other commands run in the cabin.".into());
    lines.join("\n")
}

/// Reads a slash line. `remote` is a paired browser.
pub fn parse(line: &str, remote: bool) -> Result<Action, String> {
    let Some(word) = command_word(line) else {
        return Err("That isn't a command.".into());
    };
    let rest = line.trim_start()[word.len()..].trim();
    let Some(cmd) = find(&word) else {
        if grokhub_core::parse_slash(line).is_some()
            || !grokhub_core::filter_slash_hits(&word, &[]).is_empty()
        {
            return Err(format!("{word} runs in the cabin, not here."));
        }
        return Err(format!("Unknown command {word}. Type /help for the list."));
    };
    if !cmd.ready {
        return Err(format!("{word} isn't in this app yet."));
    }
    if remote && cmd.desktop_only {
        return Err(format!("{word} works on your computer."));
    }
    let text = (!rest.is_empty()).then(|| rest.to_string());
    let effect = match word.as_str() {
        "/help" => Effect::Notice { text: help() },
        "/new" | "/clear" => Effect::NewChat,
        "/chat" => Effect::Mode { mode: "chat", text },
        "/plan" => Effect::Mode { mode: "plan", text },
        "/btw" => Effect::Mode { mode: "btw", text },
        "/settings" => Effect::Open { view: "settings" },
        "/memory" => Effect::Open { view: "memory" },
        "/automations" => Effect::Open {
            view: "automations",
        },
        "/connectors" => Effect::Open { view: "connectors" },
        "/remember" => {
            return match text {
                Some(note) => Ok(Action::Remember(note)),
                None => Err("Type the note after /remember.".into()),
            }
        }
        _ => return Err(format!("Unknown command {word}. Type /help for the list.")),
    };
    Ok(Action::Effect(effect))
}

/// Runs what `parse` read. A note goes to `workspace`'s memory with its
/// secrets redacted.
pub fn apply(action: Action, workspace: &std::path::Path) -> Result<Effect, String> {
    match action {
        Action::Effect(effect) => Ok(effect),
        Action::Remember(note) => Ok(Effect::Notice {
            text: crate::memory::add(workspace, &note, false)?,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.cmd.as_str()).collect()
    }

    #[test]
    fn pl_lists_plan_and_cabin_commands_come_back_greyed() {
        let hits = hits("/pl", false);
        assert_eq!(hits[0].cmd, "/plan");
        assert_eq!(hits[0].disabled, None);
        assert!(hits[0].run_on_pick);
        // Every row past the shell's own is the cabin's, greyed.
        for hit in &hits[1..] {
            assert_eq!(hit.disabled.as_deref(), Some(IN_THE_CABIN), "{}", hit.cmd);
        }
        let all = super::hits("/", false);
        assert!(cmds(&all).starts_with(&["/help", "/new", "/clear"]));
        assert!(all.len() <= 12);
    }

    #[test]
    fn a_cabin_only_command_is_listed_greyed_and_not_run() {
        let hits = hits("/diag", false);
        assert_eq!(cmds(&hits), vec!["/diagnose"]);
        assert_eq!(hits[0].disabled.as_deref(), Some(IN_THE_CABIN));
        assert_eq!(
            parse("/diagnose", false),
            Err("/diagnose runs in the cabin, not here.".into())
        );
    }

    #[test]
    fn unknown_commands_error_and_paths_are_not_commands() {
        assert_eq!(
            parse("/nope", false),
            Err("Unknown command /nope. Type /help for the list.".into())
        );
        assert!(!is_command("/usr/bin is slow"));
        assert!(!is_command("/ hi"));
        assert!(!is_command("hello /plan"));
        assert!(is_command("  /Plan write tests"));
        assert!(hits("/plan now", false).is_empty());
        assert!(hits("hello", false).is_empty());
    }

    #[test]
    fn modes_views_and_new_chat() {
        assert_eq!(
            parse("/plan", false),
            Ok(Action::Effect(Effect::Mode {
                mode: "plan",
                text: None
            }))
        );
        assert_eq!(
            parse("/BTW what does this repo do", false),
            Ok(Action::Effect(Effect::Mode {
                mode: "btw",
                text: Some("what does this repo do".into())
            }))
        );
        assert_eq!(parse("/clear", false), Ok(Action::Effect(Effect::NewChat)));
        assert_eq!(
            parse("/connectors", true),
            Ok(Action::Effect(Effect::Open { view: "connectors" }))
        );
        assert_eq!(
            parse("/remember   ", false),
            Err("Type the note after /remember.".into())
        );
        assert_eq!(
            parse("/remember ship on fridays", false),
            Ok(Action::Remember("ship on fridays".into()))
        );
    }

    #[test]
    fn the_browser_cant_open_settings() {
        assert_eq!(
            parse("/settings", true),
            Err("/settings works on your computer.".into())
        );
        let hit = hits("/sett", true)
            .into_iter()
            .find(|h| h.cmd == "/settings")
            .expect("settings row");
        assert_eq!(hit.disabled.as_deref(), Some(ON_YOUR_COMPUTER));
    }

    #[test]
    fn routing_commands_are_unknown_here() {
        for line in [
            "/model grok-x",
            "/model",
            "/mode fast",
            "/models",
            "/effort high",
            "/why",
        ] {
            let word = line.split_whitespace().next().expect("word");
            assert_eq!(
                parse(line, false),
                Err(format!("Unknown command {word}. Type /help for the list.")),
                "{line}"
            );
            assert_eq!(parse(line, true), parse(line, false), "{line}");
        }
        for draft in ["/model", "/mode", "/models", "/effort", "/why"] {
            assert_eq!(cmds(&hits(draft, false)), Vec::<&str>::new(), "{draft}");
        }
        assert!(!help().contains("/model"));
    }

    #[test]
    fn history_waits_and_help_lists_what_runs() {
        assert_eq!(
            parse("/history", false),
            Err("/history isn't in this app yet.".into())
        );
        let help = help();
        assert!(help.contains("/plan — Plan mode"));
        assert!(help.contains("/remember — Save a memory note"));
        assert!(!help.contains("/history"));
    }

    fn workspace(name: &str) -> (std::path::PathBuf, grokhub_agent::perm::ConfigGuard) {
        let root = std::env::temp_dir().join(format!(
            "grokhub-shell-slash-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let (config, ws) = (root.join("cfg"), root.join("ws"));
        std::fs::create_dir_all(&config).expect("cfg");
        std::fs::create_dir_all(&ws).expect("ws");
        (ws, grokhub_agent::perm::ConfigGuard::set(&config))
    }

    #[test]
    fn remember_stores_the_note_with_its_secret_redacted() {
        let (ws, _guard) = workspace("remember");
        let action = parse(
            "/remember the deploy key is sk-abcdefghijklmnopqrstuv",
            false,
        )
        .expect("parse");
        let effect = apply(action, &ws).expect("apply");
        assert!(matches!(effect, Effect::Notice { .. }));
        let notes = crate::memory::load(&ws).expect("load");
        assert!(notes.workspace.contains("the deploy key is [redacted]"));
        assert!(!notes.workspace.contains("sk-abcdefghijklmnopqrstuv"));
    }
}
