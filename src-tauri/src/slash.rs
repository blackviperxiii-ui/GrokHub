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
            "Record my screen as stills, 2 minutes at most (say what's wrong)",
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
    row("/recall", "Search chats and memory…", "/recall "),
    ShellCmd {
        desktop_only: true,
        ..row("/forget", "Remove a memory topic (asks first)…", "/forget ")
    },
    row("/memory note", "Save a memory note…", "/memory note "),
    ShellCmd {
        desktop_only: true,
        ..row(
            "/memory prune",
            "Retire stale learned notes (asks first)",
            "/memory prune",
        )
    },
    row(
        "/memory dream",
        "Show the latest memory dream",
        "/memory dream",
    ),
    ShellCmd {
        desktop_only: true,
        ..row("/dream", "Dream over your memory now", "/dream")
    },
    ShellCmd {
        desktop_only: true,
        ..row(
            "/learned",
            "What the app learned, and how sure it is",
            "/learned",
        )
    },
    row("/skill", "Run a skill…", "/skill "),
    row("/skills", "Open Skills", "/skills"),
    row(
        "/skills changes",
        "What the app changed in your skills",
        "/skills changes",
    ),
    ShellCmd {
        desktop_only: true,
        ..row(
            "/skills undo",
            "Undo the newest change to a skill…",
            "/skills undo ",
        )
    },
    ShellCmd {
        desktop_only: true,
        ..row(
            "/skills restore",
            "Bring back a removed skill…",
            "/skills restore ",
        )
    },
    row(
        "/connections changes",
        "What the app changed in your connections",
        "/connections changes",
    ),
    row(
        "/automations changes",
        "What the app changed in your automations",
        "/automations changes",
    ),
    row(
        "/privacy",
        "What left this computer, and your grants",
        "/privacy",
    ),
    row("/brief", "The latest weekly systems briefing", "/brief"),
    row(
        "/health",
        "Updates, failed services and the last dream",
        "/health",
    ),
    ShellCmd {
        desktop_only: true,
        ..row("/send", "Run a task on its own, read only…", "/send ")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/bg", "Run a task in the background, or list them…", "/bg ")
    },
    row("/queue", "Send after the live reply ends…", "/queue "),
    row("/loop", "Schedule a repeating job…", "/loop "),
    row("/board", "Open Workboards", "/board"),
    ShellCmd {
        desktop_only: true,
        ..row("/goal", "Pin a goal, or show or clear it…", "/goal ")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/fix", "Health check, then Settings", "/fix")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/diagnose", "Check this computer (read only)", "/diagnose")
    },
    row("/consult", "One-shot consult…", "/consult "),
    row("/inspect", "Open Connectors: tools and servers", "/inspect"),
    row("/hooks", "Open Connectors", "/hooks"),
    row("/workflow", "Workflows", "/workflow"),
    row("/worktree", "Worktrees", "/worktree"),
    row("/view-plan", "Show the plan from Plan mode", "/view-plan"),
    row(
        "/always-approve",
        "Where approvals are set",
        "/always-approve",
    ),
    row("/auto", "Where approvals are set", "/auto"),
    ShellCmd {
        desktop_only: true,
        ..row("/hub", "Open Devices", "/hub")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/host", "This computer's engine status", "/host")
    },
    ShellCmd {
        desktop_only: true,
        ..row("/update", "Open Updates", "/update")
    },
    row("/palette", "Command palette", "/palette"),
    ShellCmd {
        ready: false,
        ..row("/sh", "Run a shell line…", "/sh ")
    },
    ShellCmd {
        ready: false,
        ..row("/scratch", "New scratch chat (no memory)", "/scratch")
    },
    ShellCmd {
        ready: false,
        ..row("/sync", "Sync with paired computers", "/sync")
    },
    ShellCmd {
        ready: false,
        ..row(
            "/inhabit",
            "Hand this chat to another computer…",
            "/inhabit ",
        )
    },
    ShellCmd {
        ready: false,
        ..row("/room", "Speak the room…", "/room ")
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
    /// `/queue <message>`: held until the live reply ends, or sent at once.
    Queue { text: String },
    /// `/palette`: open the command palette.
    Palette,
    /// `/view-plan`: show the latest plan from Plan mode.
    ViewPlan,
    /// `/loop <words>`: save the words as an automation, then show the page.
    Loop { seed: String },
    /// A page opens, and `text` says why.
    OpenNote { view: &'static str, text: String },
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
    Delete {
        yes: bool,
    },
    Sessions,
    Context,
    Usage,
    RewindNone,
    RewindList,
    RewindStep(u64),
    RewindFiles {
        yes: bool,
    },
    Project(crate::projects::Op),
    Recall(String),
    Forget {
        topic: Option<String>,
        yes: bool,
    },
    MemoryPrune {
        yes: bool,
    },
    MemoryDream,
    Dream,
    Learned(String),
    Skill(String),
    SkillChanges,
    SkillUndo(String),
    SkillRestore(String),
    ConnectionChanges,
    AutomationChanges,
    Privacy,
    Brief,
    Health,
    Diagnose,
    Fix,
    Goal(String),
    HostStatus,
    /// `/send <task>`: an unattended read-only run.
    Send(String),
    /// `/bg [task|stop]`.
    Bg(String),
    /// `/consult <question>`.
    Consult(String),
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
        "/background" => "/bg".to_string(),
        "/resume" | "/dashboard" | "/agents-dashboard" => "/sessions".to_string(),
        "/status" | "/info" | "/session-info" | "/doctor" => "/inspect".to_string(),
        "/plugins" | "/marketplace" | "/mcps" => "/connectors".to_string(),
        "/show-plan" | "/plan-view" => "/view-plan".to_string(),
        "/yolo" => "/always-approve".to_string(),
        "/workflows" => "/workflow".to_string(),
        other => other.to_string(),
    };
    if word == "/connections" {
        return match grokhub_core::parse_slash(line) {
            Some(grokhub_core::Slash::ConnectionChanges) => Ok(Action::Op(Op::ConnectionChanges)),
            _ => Err("Use /connections changes.".into()),
        };
    }
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
    if let Some(done) = more(line, &word, rest, remote) {
        return done;
    }
    let effect = match word.as_str() {
        "/help" => Effect::Notice { text: help() },
        "/new" | "/clear" => Effect::NewChat,
        "/chat" => Effect::Mode { mode: "chat", text },
        "/plan" => Effect::Mode { mode: "plan", text },
        "/btw" => Effect::Mode { mode: "btw", text },
        "/settings" => Effect::Open { view: "settings" },
        "/memory" => Effect::Open { view: "memory" },
        "/skills" => Effect::Open { view: "skills" },
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
                | "/memory prune"
        ) {
            return (t[..head.len()].to_string(), true);
        }
    }
    (t.to_string(), false)
}

const FORGET_USAGE: &str = "Use /forget <topic>, or /forget to empty MEMORY.md.";

/// The memory, learning, skill and ledger commands. `None` when `word` isn't
/// one of them. The ones that change anything are refused in a paired browser
/// and answer with a question first; the same line with `yes` runs them.
fn more(line: &str, word: &str, rest: &str, remote: bool) -> Option<Result<Action, String>> {
    use grokhub_core::Slash;
    let mine = |op: Op| Some(Ok(Action::Op(op)));
    let on_computer = || Some(Err(format!("{word} works on your computer.")));
    match word {
        "/recall" if rest.is_empty() => Some(Err("Type what to recall after /recall.".into())),
        "/recall" => mine(Op::Recall(rest.to_string())),
        "/forget" => {
            let (yes, topic) = match rest.strip_prefix("--yes") {
                Some(after) if after.is_empty() || after.starts_with(' ') => (true, after.trim()),
                _ => (false, rest),
            };
            if rest.contains('\n') {
                return Some(Err(FORGET_USAGE.into()));
            }
            mine(Op::Forget {
                topic: (!topic.is_empty()).then(|| topic.to_string()),
                yes,
            })
        }
        "/memory" if rest.is_empty() => None,
        "/memory" => {
            let (line, yes) = split_yes(line);
            match grokhub_core::parse_slash(&line) {
                Some(Slash::MemoryShow) => {
                    Some(Ok(Action::Effect(Effect::Open { view: "memory" })))
                }
                Some(Slash::MemoryDream) => mine(Op::MemoryDream),
                Some(Slash::MemoryPrune) if remote => on_computer(),
                Some(Slash::MemoryPrune) => mine(Op::MemoryPrune { yes }),
                Some(Slash::MemoryNote(note)) => Some(Ok(Action::Remember(note))),
                _ => Some(Err(
                    "Use /memory, /memory show, note <text>, prune or dream.".into(),
                )),
            }
        }
        "/dream" => mine(Op::Dream),
        "/learned" => mine(Op::Learned(rest.to_string())),
        "/skill" if rest.is_empty() => Some(Err("Type the skill's name after /skill.".into())),
        "/skill" => mine(Op::Skill(rest.to_string())),
        "/skills" if rest.is_empty() => None,
        "/skills" => match grokhub_core::parse_slash(line) {
            Some(Slash::SkillChanges) => mine(Op::SkillChanges),
            Some(Slash::SkillUndo(_) | Slash::SkillRestore(_)) if remote => on_computer(),
            Some(Slash::SkillUndo(name)) => mine(Op::SkillUndo(name)),
            Some(Slash::SkillRestore(name)) => mine(Op::SkillRestore(name)),
            _ => Some(Ok(Action::Effect(Effect::Open { view: "skills" }))),
        },
        "/automations" if rest.is_empty() => None,
        "/automations" => match grokhub_core::parse_slash(line) {
            Some(Slash::AutomationChanges) => mine(Op::AutomationChanges),
            _ => Some(Err("Use /automations changes.".into())),
        },
        "/privacy" => mine(Op::Privacy),
        "/brief" => mine(Op::Brief),
        "/health" => mine(Op::Health),
        _ => extras(word, rest),
    }
}

/// What the cabin's retired CLI features answer.
pub const WORKFLOWS_GONE: &str = "Workflows were a grok CLI feature and are not in this app";
pub const WORKTREE_GONE: &str = "There is no /worktree command. Chats run in the bound project \
     folder; only helpers work in worktrees of their own";
pub const PERMISSION_IN_SETTINGS: &str =
    "Permissions are set in Settings → Defaults, under When the AI wants to change things";

/// The background-work, view and routing commands. `None` when `word`
/// isn't one of them.
fn extras(word: &str, rest: &str) -> Option<Result<Action, String>> {
    let mine = |op: Op| Some(Ok(Action::Op(op)));
    let fx = |e: Effect| Some(Ok(Action::Effect(e)));
    let open = |view: &'static str| fx(Effect::Open { view });
    match word {
        "/send" if rest.is_empty() => Some(Err("Type the task after /send.".into())),
        "/send" => mine(Op::Send(rest.to_string())),
        "/bg" => mine(Op::Bg(rest.to_string())),
        "/queue" if rest.is_empty() => Some(Err("Type the message after /queue.".into())),
        "/queue" => fx(Effect::Queue {
            text: rest.to_string(),
        }),
        "/loop" if rest.is_empty() => open("automations"),
        "/loop" => fx(Effect::Loop {
            seed: rest.to_string(),
        }),
        "/board" => open("workboard"),
        "/goal" => mine(Op::Goal(rest.to_string())),
        "/fix" => mine(Op::Fix),
        "/diagnose" => mine(Op::Diagnose),
        "/consult" if rest.is_empty() => Some(Err("Type the question after /consult.".into())),
        "/consult" => mine(Op::Consult(rest.to_string())),
        "/inspect" | "/hooks" => open("connectors"),
        "/workflow" => fx(Effect::Notice {
            text: WORKFLOWS_GONE.into(),
        }),
        "/worktree" => fx(Effect::Notice {
            text: WORKTREE_GONE.into(),
        }),
        "/view-plan" => fx(Effect::ViewPlan),
        "/always-approve" | "/auto" => fx(Effect::OpenNote {
            view: "settings",
            text: PERMISSION_IN_SETTINGS.into(),
        }),
        "/hub" => open("devices"),
        "/host" => mine(Op::HostStatus),
        "/update" => open("updates"),
        "/palette" => fx(Effect::Palette),
        _ => None,
    }
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
        let hits = hits("/jour", false);
        assert_eq!(cmds(&hits), vec!["/journal"]);
        assert_eq!(hits[0].disabled.as_deref(), Some(IN_THE_CABIN));
        assert_eq!(
            parse("/journal", false),
            Err("/journal runs in the cabin, not here.".into())
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

    #[test]
    fn memory_learning_skill_and_ledger_commands_read_like_the_cabin() {
        assert_eq!(
            op_of("/recall ferry times"),
            Op::Recall("ferry times".into())
        );
        assert_eq!(
            parse("/recall", false),
            Err("Type what to recall after /recall.".into())
        );
        assert_eq!(
            op_of("/forget wifi"),
            Op::Forget {
                topic: Some("wifi".into()),
                yes: false
            }
        );
        assert_eq!(
            op_of("/forget --yes wifi"),
            Op::Forget {
                topic: Some("wifi".into()),
                yes: true
            }
        );
        assert_eq!(
            op_of("/forget"),
            Op::Forget {
                topic: None,
                yes: false
            }
        );
        // A topic that merely ends in the word yes is still a topic.
        assert_eq!(
            op_of("/forget say yes"),
            Op::Forget {
                topic: Some("say yes".into()),
                yes: false
            }
        );
        assert_eq!(op_of("/memory prune"), Op::MemoryPrune { yes: false });
        assert_eq!(op_of("/memory prune yes"), Op::MemoryPrune { yes: true });
        assert_eq!(op_of("/memory dream"), Op::MemoryDream);
        assert_eq!(
            parse("/memory note prefer nvim", false),
            Ok(Action::Remember("prefer nvim".into()))
        );
        assert_eq!(effect_of("/memory show"), Effect::Open { view: "memory" });
        assert_eq!(effect_of("/memory"), Effect::Open { view: "memory" });
        assert_eq!(op_of("/dream"), Op::Dream);
        assert_eq!(op_of("/learned forget n1"), Op::Learned("forget n1".into()));
        assert_eq!(op_of("/skill tidy"), Op::Skill("tidy".into()));
        assert_eq!(effect_of("/skills"), Effect::Open { view: "skills" });
        assert_eq!(op_of("/skills changes"), Op::SkillChanges);
        assert_eq!(op_of("/skills undo tidy"), Op::SkillUndo("tidy".into()));
        assert_eq!(
            op_of("/skills restore tidy"),
            Op::SkillRestore("tidy".into())
        );
        assert_eq!(op_of("/connections changes"), Op::ConnectionChanges);
        assert_eq!(op_of("/connections"), Op::ConnectionChanges);
        assert_eq!(op_of("/automations changes"), Op::AutomationChanges);
        assert_eq!(
            effect_of("/automations"),
            Effect::Open {
                view: "automations"
            }
        );
        assert_eq!(
            parse("/connections nope", false),
            Err("Use /connections changes.".into())
        );
        assert_eq!(op_of("/privacy"), Op::Privacy);
        assert_eq!(op_of("/brief"), Op::Brief);
        assert_eq!(op_of("/health"), Op::Health);
    }

    #[test]
    fn the_browser_can_read_but_not_change_memory_or_skills() {
        for line in [
            "/recall x",
            "/memory dream",
            "/skills changes",
            "/privacy",
            "/health",
            "/brief",
        ] {
            assert!(parse(line, true).is_ok(), "{line}");
        }
        for line in [
            "/forget wifi",
            "/dream",
            "/learned",
            "/memory prune",
            "/skills undo tidy",
            "/skills restore tidy",
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
    fn these_commands_are_listed_ready_and_the_rest_of_learn_stays_in_the_cabin() {
        for word in [
            "/recall", "/forget", "/dream", "/learned", "/skills", "/privacy", "/brief", "/health",
        ] {
            let row = hits(word, false)
                .into_iter()
                .find(|h| h.cmd == word)
                .unwrap_or_else(|| panic!("{word} is listed"));
            assert_eq!(row.disabled, None, "{word}");
        }
        assert_eq!(
            cmds(&hits("/skills", false)),
            vec![
                "/skills",
                "/skills changes",
                "/skills undo",
                "/skills restore"
            ]
        );
        assert_eq!(
            parse("/learn reflect", false),
            Err("/learn runs in the cabin, not here.".into())
        );
        let help = help();
        assert!(help.contains("/recall — Search chats and memory"));
        assert!(!help.contains("/skills undo"));
    }

    #[test]
    fn background_work_commands_read_their_words() {
        assert_eq!(
            parse("/send", false),
            Err("Type the task after /send.".into())
        );
        assert_eq!(
            parse("/send audit the logs", false),
            Ok(Action::Op(Op::Send("audit the logs".into())))
        );
        assert_eq!(
            parse("/background", false),
            Ok(Action::Op(Op::Bg(String::new())))
        );
        assert_eq!(
            parse("/bg stop", false),
            Ok(Action::Op(Op::Bg("stop".into())))
        );
        assert_eq!(
            parse("/bg", true),
            Err("/bg works on your computer.".into())
        );
        assert_eq!(
            parse("/queue", false),
            Err("Type the message after /queue.".into())
        );
        assert_eq!(
            parse("/queue then run the tests", true),
            Ok(Action::Effect(Effect::Queue {
                text: "then run the tests".into()
            }))
        );
        assert_eq!(
            parse("/loop every day at 9 summarize my inbox", false),
            Ok(Action::Effect(Effect::Loop {
                seed: "every day at 9 summarize my inbox".into()
            }))
        );
        assert_eq!(
            parse("/loop", false),
            Ok(Action::Effect(Effect::Open {
                view: "automations"
            }))
        );
        assert_eq!(
            parse("/consult", false),
            Err("Type the question after /consult.".into())
        );
        assert_eq!(
            parse("/consult tabs or spaces", false),
            Ok(Action::Op(Op::Consult("tabs or spaces".into())))
        );
    }

    #[test]
    fn view_and_routing_commands_open_what_the_cabin_opens() {
        let open = |view| Ok(Action::Effect(Effect::Open { view }));
        assert_eq!(parse("/board", false), open("workboard"));
        assert_eq!(parse("/inspect", false), open("connectors"));
        assert_eq!(parse("/doctor", false), open("connectors"));
        assert_eq!(parse("/hooks", false), open("connectors"));
        assert_eq!(parse("/mcps", false), open("connectors"));
        assert_eq!(parse("/hub", false), open("devices"));
        assert_eq!(parse("/update", false), open("updates"));
        assert_eq!(parse("/resume", false), Ok(Action::Op(Op::Sessions)));
        assert_eq!(parse("/palette", true), Ok(Action::Effect(Effect::Palette)));
        assert_eq!(
            parse("/show-plan", false),
            Ok(Action::Effect(Effect::ViewPlan))
        );
        for line in ["/auto", "/always-approve", "/yolo"] {
            assert_eq!(
                parse(line, false),
                Ok(Action::Effect(Effect::OpenNote {
                    view: "settings",
                    text: PERMISSION_IN_SETTINGS.into()
                })),
                "{line}"
            );
        }
        assert_eq!(parse("/diagnose", false), Ok(Action::Op(Op::Diagnose)));
        assert_eq!(
            parse("/diagnose", true),
            Err("/diagnose works on your computer.".into())
        );
        assert_eq!(parse("/fix", false), Ok(Action::Op(Op::Fix)));
        assert_eq!(parse("/host", false), Ok(Action::Op(Op::HostStatus)));
        assert_eq!(
            parse("/goal ship the harbor", false),
            Ok(Action::Op(Op::Goal("ship the harbor".into())))
        );
    }

    #[test]
    fn the_browser_cant_open_devices() {
        assert_eq!(
            parse("/hub", true),
            Err("/hub works on your computer.".into())
        );
        let hit = hits("/hub", true)
            .into_iter()
            .find(|h| h.cmd == "/hub")
            .expect("hub row");
        assert_eq!(hit.disabled.as_deref(), Some(ON_YOUR_COMPUTER));
    }

    #[test]
    fn the_browser_schedules_and_queues_and_is_refused_the_desktop_only_commands() {
        assert_eq!(
            parse("/queue then run the tests", true),
            Ok(Action::Effect(Effect::Queue {
                text: "then run the tests".into()
            }))
        );
        assert_eq!(
            parse("/loop every day at 9 summarize my inbox", true),
            Ok(Action::Effect(Effect::Loop {
                seed: "every day at 9 summarize my inbox".into()
            }))
        );
        assert_eq!(
            parse("/loop", true),
            Ok(Action::Effect(Effect::Open {
                view: "automations"
            }))
        );
        assert_eq!(
            parse("/board", true),
            Ok(Action::Effect(Effect::Open { view: "workboard" }))
        );
        for (line, word) in [
            ("/send audit the logs", "/send"),
            ("/bg stop", "/bg"),
            ("/background index the repo", "/bg"),
            ("/goal ship the harbor", "/goal"),
            ("/fix", "/fix"),
            ("/host", "/host"),
            ("/update", "/update"),
        ] {
            assert_eq!(
                parse(line, true),
                Err(format!("{word} works on your computer.")),
                "{line}"
            );
            let hit = hits(word, true)
                .into_iter()
                .find(|h| h.cmd == word)
                .expect(word);
            assert_eq!(hit.disabled.as_deref(), Some(ON_YOUR_COMPUTER), "{word}");
        }
    }

    #[test]
    fn retired_cli_features_answer_in_the_cabins_words() {
        for line in ["/workflow", "/workflows", "/workflow pause nightly"] {
            assert_eq!(
                parse(line, false),
                Ok(Action::Effect(Effect::Notice {
                    text: "Workflows were a grok CLI feature and are not in this app".into()
                })),
                "{line}"
            );
        }
        assert_eq!(
            parse("/worktree", false),
            Ok(Action::Effect(Effect::Notice {
                text: "There is no /worktree command. Chats run in the bound project folder; \
                       only helpers work in worktrees of their own"
                    .into()
            }))
        );
    }

    #[test]
    fn what_waits_on_another_card_is_greyed_not_in_the_cabin() {
        for cmd in ["/sh", "/scratch", "/sync", "/inhabit", "/room"] {
            let hit = hits(cmd, false)
                .into_iter()
                .find(|h| h.cmd == cmd)
                .expect(cmd);
            assert_eq!(hit.disabled.as_deref(), Some(NOT_YET), "{cmd}");
            assert_eq!(
                parse(cmd, false),
                Err(format!("{cmd} isn't in this app yet.")),
                "{cmd}"
            );
        }
    }

    #[test]
    fn the_scope_commands_are_runnable_rows() {
        for cmd in [
            "/send",
            "/bg",
            "/queue",
            "/loop",
            "/board",
            "/goal",
            "/fix",
            "/diagnose",
            "/consult",
            "/inspect",
            "/hooks",
            "/workflow",
            "/worktree",
            "/view-plan",
            "/always-approve",
            "/auto",
            "/hub",
            "/host",
            "/update",
            "/palette",
        ] {
            let hit = hits(cmd, false)
                .into_iter()
                .find(|h| h.cmd == cmd)
                .expect(cmd);
            assert_eq!(hit.disabled, None, "{cmd}");
        }
        assert!(help().contains("/palette — Command palette"));
    }
}
