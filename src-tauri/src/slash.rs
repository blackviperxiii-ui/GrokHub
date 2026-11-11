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
    row("/chat", "Just answer the next message", "/chat"),
    row("/plan", "Plan the next message first", "/plan"),
    row("/btw", "Ask on the side while a reply runs", "/btw"),
    row("/remember", "Save a memory note…", "/remember "),
    ShellCmd {
        desktop_only: true,
        ..row("/settings", "Open Settings", "/settings")
    },
    row("/memory", "Open Memory", "/memory"),
    row("/automations", "Open Automations", "/automations"),
    row("/connectors", "Open Connectors", "/connectors"),
    ShellCmd {
        desktop_only: true,
        ..row("/import", "Add an OpenClaw workspace for review", "/import")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/rename", "Rename this chat…", "/rename ")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/pin", "Pin or unpin this chat", "/pin")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/delete", "Delete this chat (asks first)", "/delete")
    },
    row("/sessions", "Past chats", "/sessions"),
    ShellCmd {
        desktop_only: true,
        ..row(
            "/record",
            "Record my screen and diagnose it (say what's wrong)",
            "/record ",
        )
    },
    ShellCmd {
        desktop_only: true,
        ..row(
            "/audiocheck",
            "Check my audio: devices, levels, clipping, crackle",
            "/audiocheck",
        )
    },
    ShellCmd {
        desktop_only: true,
        ..row("/retry", "Ask the last message again", "/retry")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/undo", "Undo the reply in progress", "/undo")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/stop", "Stop the reply in progress", "/stop")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/compact", "Summarize older turns to free room", "/compact")
    },
    row("/context", "How full this chat's context is", "/context"),
    row("/usage", "Tokens and cost in this chat", "/usage"),
    ShellCmd {
        desktop_only: true,
        ..row("/rewind", "Look back at the agent's file edits", "/rewind")
    },
    ShellCmd {
        desktop_only: true,
        ..row(
            "/rewind files",
            "Put the last edited files back (asks first)",
            "/rewind files",
        )
    },
    ShellCmd {
        desktop_only: true,
        ..row(
            "/rewind step",
            "List edit steps, or restore one",
            "/rewind step",
        )
    },
    ShellCmd {
        desktop_only: true,
        ..row("/project", "Show the bound project", "/project")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/project new", "Make a project…", "/project new ")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/project bind", "Work in a folder…", "/project bind ")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/project clear", "Unbind the project", "/project clear")
    },
    ShellCmd {
        desktop_only: true,
        ..row(
            "/project rename",
            "Rename the bound project…",
            "/project rename ",
        )
    },
    ShellCmd {
        desktop_only: true,
        ..row("/project move", "Move it into a folder…", "/project move ")
    },
    ShellCmd {
        desktop_only: true,
        ..row(
            "/project folder",
            "Make a project folder…",
            "/project folder ",
        )
    },
    ShellCmd {
        desktop_only: true,
        ..row(
            "/project delete",
            "Remove the bound project (asks first)",
            "/project delete",
        )
    },
    ShellCmd {
        desktop_only: true,
        ..row("/export", "Save this chat to your Chats folder", "/export")
    },
    ShellCmd {
        desktop_only: true,
        ..row(
            "/export html",
            "Save this chat as a web page",
            "/export html",
        )
    },
    ShellCmd {
        desktop_only: true,
        ..row("/export json", "Save this chat as JSON", "/export json")
    },
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
    /// `/import`: open the review of an OpenClaw workspace's notes. Nothing
    /// is written until the person saves.
    Import,
    /// A line to show in the chat log: help or a saved note.
    Notice { text: String },
    /// `/export`: the chat saves this conversation and shows where it went.
    Export,
    /// `/export html` and `/export json`.
    ExportAs { format: &'static str },
    /// `/retry`: ask the last message again.
    Retry,
    /// `/undo`: take back the reply in progress.
    Undo,
    /// `/stop`: stop the reply in progress.
    Stop,
    /// `/compact`: have the engine summarize the older turns.
    Compact,
    /// A destructive command waiting for its yes: show `prompt`, and run
    /// `yes` when the person agrees.
    Confirm { prompt: String, yes: String },
    /// `/delete yes` removed this chat.
    Deleted { id: String, title: String },
}

/// What `run` needs done outside this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Effect(Effect),
    Remember(String),
    /// A command that acts on the saved chat or the project list.
    Op(Op),
}

/// What `slash_ops` runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Rename(String),
    Pin,
    Delete { yes: bool },
    Sessions,
    Context,
    Usage,
    RewindNone,
    RewindList,
    RewindStep(u64),
    RewindFiles { yes: bool },
    Project(crate::projects::Op),
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
        .filter(|c| c.ready && !c.cmd.contains(' '))
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
    let word = match word.as_str() {
        "/title" => "/rename".to_string(),
        "/close" => "/delete".to_string(),
        other => other.to_string(),
    };
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
        "/import" => Effect::Import,
        "/export" => match grokhub_core::ChatExport::parse(rest) {
            Some(grokhub_core::ChatExport::Markdown) => Effect::Export,
            Some(grokhub_core::ChatExport::Html) => Effect::ExportAs { format: "html" },
            Some(grokhub_core::ChatExport::Json) => Effect::ExportAs { format: "json" },
            None => return Err(grokhub_core::EXPORT_FORMATS_HINT.into()),
        },
        "/retry" => Effect::Retry,
        "/undo" => Effect::Undo,
        "/stop" => Effect::Stop,
        "/compact" => Effect::Compact,
        "/rename" | "/pin" | "/delete" | "/sessions" | "/context" | "/usage" | "/rewind"
        | "/project" => return op(line, &word).map(Action::Op),
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

/// A line ending in `yes` for a destructive command, without the yes.
fn split_yes(line: &str) -> (String, bool) {
    let t = line.trim();
    let lower = t.to_ascii_lowercase();
    if let Some(head) = lower.strip_suffix(" yes") {
        if matches!(
            head.trim(),
            "/delete"
                | "/close"
                | "/project delete"
                | "/rewind files"
                | "/rewind --files"
                | "/rewind --code"
        ) {
            return (t[..head.len()].to_string(), true);
        }
    }
    (t.to_string(), false)
}

const PROJECT_USAGE: &str =
    "Use /project new <name>, show, bind <folder>, clear, rename <name>, move <folder>, folder <name> or delete.";

/// Reads the commands that act on a saved chat or the project list, through
/// the registry the cabin parses with.
fn op(line: &str, word: &str) -> Result<Op, String> {
    use crate::projects::Op as P;
    use grokhub_core::Slash;
    let (line, yes) = split_yes(line);
    let parsed = grokhub_core::parse_slash(&line);
    Ok(match parsed {
        Some(Slash::Rename(title)) => Op::Rename(title),
        Some(Slash::Pin) => Op::Pin,
        Some(Slash::Delete) => Op::Delete { yes },
        Some(Slash::Sessions) => Op::Sessions,
        Some(Slash::Context) => Op::Context,
        Some(Slash::Usage) => Op::Usage,
        Some(Slash::Rewind) => Op::RewindNone,
        Some(Slash::RewindStep(None)) => Op::RewindList,
        Some(Slash::RewindStep(Some(n))) => Op::RewindStep(n),
        Some(Slash::RewindFiles) => Op::RewindFiles { yes },
        Some(Slash::ProjectShow) => Op::Project(P::Show),
        Some(Slash::ProjectNew(name)) => Op::Project(P::New(name)),
        Some(Slash::ProjectBind(Some(path))) => Op::Project(P::Bind(path)),
        Some(Slash::ProjectClear) => Op::Project(P::Clear),
        Some(Slash::ProjectRename(name)) => Op::Project(P::Rename(name)),
        Some(Slash::ProjectMove(folder)) => Op::Project(P::Move(folder)),
        Some(Slash::ProjectFolder(name)) => Op::Project(P::Folder(name)),
        Some(Slash::ProjectDelete) => Op::Project(P::Delete { yes }),
        _ if word == "/rename" => return Err("Type the new name after /rename.".into()),
        _ => return Err(PROJECT_USAGE.into()),
    })
}

/// Runs what `parse` read. A note goes to `workspace`'s memory with its
/// secrets redacted.
pub fn apply(action: Action, workspace: &std::path::Path) -> Result<Effect, String> {
    match action {
        Action::Effect(effect) => Ok(effect),
        Action::Remember(note) => Ok(Effect::Notice {
            text: crate::memory::add(workspace, &note, false)?,
        }),
        Action::Op(_) => Err("That command needs a saved chat.".into()),
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
    fn export_runs_here_in_markdown_and_not_in_the_browser() {
        for line in ["/export", "/export md", "/EXPORT Markdown"] {
            assert_eq!(
                parse(line, false),
                Ok(Action::Effect(Effect::Export)),
                "{line}"
            );
        }
        assert_eq!(
            parse("/export html", false),
            Ok(Action::Effect(Effect::ExportAs { format: "html" }))
        );
        assert_eq!(
            parse("/export", true),
            Err("/export works on your computer.".into())
        );
        let hit = hits("/exp", false)
            .into_iter()
            .find(|h| h.cmd == "/export")
            .expect("export row");
        assert_eq!(hit.disabled, None);
        assert!(hit.run_on_pick);
        assert!(help().contains("/export — Save this chat"));
    }

    #[test]
    fn history_waits_and_help_lists_what_runs() {
        assert_eq!(
            parse("/history", false),
            Err("/history isn't in this app yet.".into())
        );
        let help = help();
        assert!(help.contains("/plan — Plan the next message first"));
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

    fn op_of(line: &str) -> Op {
        match parse(line, false) {
            Ok(Action::Op(op)) => op,
            other => panic!("{line}: {other:?}"),
        }
    }

    fn effect_of(line: &str) -> Effect {
        match parse(line, false) {
            Ok(Action::Effect(effect)) => effect,
            other => panic!("{line}: {other:?}"),
        }
    }

    #[test]
    fn chat_commands_read_the_same_as_the_cabin_reads_them() {
        use crate::projects::Op as P;
        // The cabin's own fixtures: each line parses to this `Slash` there.
        assert_eq!(
            grokhub_core::parse_slash("/pin"),
            Some(grokhub_core::Slash::Pin)
        );
        assert_eq!(
            grokhub_core::parse_slash("/delete"),
            Some(grokhub_core::Slash::Delete)
        );
        assert_eq!(op_of("/rename Pier light"), Op::Rename("Pier light".into()));
        assert_eq!(op_of("/title Pier light"), Op::Rename("Pier light".into()));
        assert_eq!(
            parse("/rename", false),
            Err("Type the new name after /rename.".into())
        );
        assert_eq!(op_of("/pin"), Op::Pin);
        assert_eq!(op_of("/sessions"), Op::Sessions);
        assert_eq!(op_of("/context"), Op::Context);
        assert_eq!(op_of("/usage"), Op::Usage);
        assert_eq!(effect_of("/retry"), Effect::Retry);
        assert_eq!(effect_of("/undo"), Effect::Undo);
        assert_eq!(effect_of("/stop"), Effect::Stop);
        assert_eq!(effect_of("/compact"), Effect::Compact);
        assert_eq!(op_of("/rewind"), Op::RewindNone);
        assert_eq!(op_of("/rewind step"), Op::RewindList);
        assert_eq!(op_of("/rewind step 3"), Op::RewindStep(3));
        assert_eq!(op_of("/rewind files"), Op::RewindFiles { yes: false });
        assert_eq!(op_of("/rewind --files"), Op::RewindFiles { yes: false });
        assert_eq!(op_of("/project"), Op::Project(P::Show));
        assert_eq!(op_of("/project show"), Op::Project(P::Show));
        assert_eq!(
            op_of("/project new Night watch"),
            Op::Project(P::New("Night watch".into()))
        );
        assert_eq!(
            op_of("/project bind ~/code"),
            Op::Project(P::Bind("~/code".into()))
        );
        assert_eq!(
            op_of("/project ~/code"),
            Op::Project(P::Bind("~/code".into()))
        );
        assert_eq!(op_of("/project clear"), Op::Project(P::Clear));
        assert_eq!(
            op_of("/project rename Dawn"),
            Op::Project(P::Rename("Dawn".into()))
        );
        assert_eq!(
            op_of("/project move Cabin"),
            Op::Project(P::Move("Cabin".into()))
        );
        assert_eq!(
            op_of("/project folder Cabin"),
            Op::Project(P::Folder("Cabin".into()))
        );
        assert_eq!(
            op_of("/project delete"),
            Op::Project(P::Delete { yes: false })
        );
        assert_eq!(
            parse("/project nope", false),
            Err(PROJECT_USAGE.to_string())
        );
        assert_eq!(
            parse("/project bind", false),
            Err(PROJECT_USAGE.to_string())
        );
    }

    #[test]
    fn destructive_commands_change_nothing_without_a_yes() {
        use crate::projects::Op as P;
        assert_eq!(op_of("/delete"), Op::Delete { yes: false });
        assert_eq!(op_of("/delete please"), Op::Delete { yes: false });
        assert_eq!(op_of("/delete yes"), Op::Delete { yes: true });
        assert_eq!(op_of("/CLOSE Yes"), Op::Delete { yes: true });
        assert_eq!(op_of("/rewind files yes"), Op::RewindFiles { yes: true });
        assert_eq!(
            op_of("/project delete yes"),
            Op::Project(P::Delete { yes: true })
        );
        // A yes on a command that doesn't ask is just text it ignores.
        assert_eq!(op_of("/rename yes"), Op::Rename("yes".into()));
    }

    #[test]
    fn export_takes_md_html_and_json() {
        assert_eq!(effect_of("/export"), Effect::Export);
        assert_eq!(effect_of("/export md"), Effect::Export);
        assert_eq!(
            effect_of("/export html"),
            Effect::ExportAs { format: "html" }
        );
        assert_eq!(
            effect_of("/EXPORT JSON"),
            Effect::ExportAs { format: "json" }
        );
        assert_eq!(
            parse("/export xml", false),
            Err("Export takes md, html, or json".into())
        );
    }

    #[test]
    fn the_browser_gets_only_the_read_only_commands() {
        for line in ["/context", "/usage", "/sessions"] {
            assert!(parse(line, true).is_ok(), "{line}");
        }
        for line in [
            "/rename x",
            "/pin",
            "/delete",
            "/retry",
            "/undo",
            "/stop",
            "/compact",
            "/rewind",
            "/rewind files",
            "/project",
            "/project delete",
            "/export html",
        ] {
            let word = line.split_whitespace().next().expect("word");
            assert_eq!(
                parse(line, true),
                Err(format!("{word} works on your computer.")),
                "{line}"
            );
        }
    }

    #[test]
    fn none_of_these_say_in_the_cabin() {
        for word in [
            "/rename",
            "/pin",
            "/delete",
            "/sessions",
            "/retry",
            "/undo",
            "/rewind",
            "/compact",
            "/context",
            "/usage",
            "/stop",
            "/project",
            "/export",
        ] {
            let rows = hits(word, false);
            let row = rows
                .iter()
                .find(|h| h.cmd == word)
                .unwrap_or_else(|| panic!("{word} is listed"));
            assert_eq!(row.disabled, None, "{word}");
            assert!(
                rows.iter().all(
                    |h| h.disabled.as_deref() != Some(IN_THE_CABIN) || !h.cmd.starts_with(word)
                ),
                "{word}"
            );
        }
        let rewind = hits("/rewind", false);
        assert_eq!(
            cmds(&rewind),
            vec!["/rewind", "/rewind files", "/rewind step"]
        );
        let project = hits("/proj", false);
        assert_eq!(
            cmds(&project),
            vec![
                "/project",
                "/project new",
                "/project bind",
                "/project clear",
                "/project rename",
                "/project move",
                "/project folder",
                "/project delete"
            ]
        );
        assert!(help().contains("/delete — Delete this chat (asks first)"));
        assert!(!help().contains("/project new"));
    }
}
