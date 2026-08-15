//! Composer slash commands. Local — they never go to the model.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slash {
    Approve { yolo: bool },
    ApproveRisky,
    ApproveAll,
    Forget(Option<String>),
    MemoryNote(String),
    MemoryShow,
    Recall(String),
    Board,
    Imagine(String),
    Compact,
    Skill(String),
    LearnReflect,
    Update,
    Help,
    New,
    Scratch,
    Clear,
    Undo,
    Retry,
    Stop,
    Sh(String),
    Host { on: bool },
    ProjectBind(Option<String>),
    ProjectClear,
    ProjectShow,
    ProjectNew(String),
    ProjectFolder(String),
    ProjectRename(String),
    ProjectMove(String),
    Send(String),
    Sync,
    Hub,
    Inhabit(String),
    Rewind,
    Room(String),
    Export,
    Rename(String),
    Context,
    Health,
    Fix,
    Remember(String),
    Mode(String),
    Dream,
    Tools { on: bool },
    HostStatus,
    Import,
    Consult(String),
    Usage,
    Models,
    Palette,
}

pub fn parse_slash(line: &str) -> Option<Slash> {
    let t = line.trim();
    if t.starts_with("$ ") {
        let cmd = t[2..].trim();
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
    match cmd.as_str() {
        "/approve" => match rest {
            "off" | "yolo" => Some(Slash::Approve { yolo: true }),
            "on" | "supervised" => Some(Slash::Approve { yolo: false }),
            "risky" => Some(Slash::ApproveRisky),
            "all" => Some(Slash::ApproveAll),
            _ => None,
        },
        "/forget" => Some(Slash::Forget(if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        })),
        "/memory" => {
            if rest.eq_ignore_ascii_case("show") || rest.is_empty() {
                return Some(Slash::MemoryShow);
            }
            let note = rest
                .strip_prefix("note")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())?;
            Some(Slash::MemoryNote(note.to_string()))
        }
        "/recall" if !rest.is_empty() => Some(Slash::Recall(rest.to_string())),
        "/board" => Some(Slash::Board),
        "/imagine" if !rest.is_empty() => Some(Slash::Imagine(rest.to_string())),
        "/compact" => Some(Slash::Compact),
        "/skill" if !rest.is_empty() => Some(Slash::Skill(rest.to_string())),
        "/learn" if rest.eq_ignore_ascii_case("reflect") => Some(Slash::LearnReflect),
        "/update" => Some(Slash::Update),
        "/help" => Some(Slash::Help),
        "/new" => Some(Slash::New),
        "/scratch" => Some(Slash::Scratch),
        "/clear" => Some(Slash::Clear),
        "/undo" => Some(Slash::Undo),
        "/retry" => Some(Slash::Retry),
        "/stop" => Some(Slash::Stop),
        "/sh" if !rest.is_empty() => Some(Slash::Sh(rest.to_string())),
        "/host" => match rest {
            "on" | "enable" => Some(Slash::Host { on: true }),
            "off" | "disable" => Some(Slash::Host { on: false }),
            _ => Some(Slash::HostStatus),
        },
        "/tools" => match rest {
            "on" | "enable" => Some(Slash::Tools { on: true }),
            "off" | "disable" => Some(Slash::Tools { on: false }),
            _ => Some(Slash::HostStatus),
        },
        "/rename" if !rest.is_empty() => Some(Slash::Rename(rest.to_string())),
        "/context" => Some(Slash::Context),
        "/health" => Some(Slash::Health),
        "/fix" => Some(Slash::Fix),
        "/remember" if !rest.is_empty() => Some(Slash::Remember(rest.to_string())),
        "/mode" if !rest.is_empty() => resolve_mode_arg(rest).map(Slash::Mode),
        "/dream" => Some(Slash::Dream),
        "/import" => Some(Slash::Import),
        "/consult" if !rest.is_empty() => Some(Slash::Consult(rest.to_string())),
        "/usage" => Some(Slash::Usage),
        "/models" => Some(Slash::Models),
        "/palette" => Some(Slash::Palette),
        "/project" => {
            if rest.eq_ignore_ascii_case("clear") || rest.eq_ignore_ascii_case("unbind") {
                Some(Slash::ProjectClear)
            } else if rest.is_empty() || rest.eq_ignore_ascii_case("show") {
                Some(Slash::ProjectShow)
            } else if let Some(name) = rest.strip_prefix("new ") {
                let name = name.trim();
                if name.is_empty() {
                    None
                } else {
                    Some(Slash::ProjectNew(name.to_string()))
                }
            } else if let Some(name) = rest.strip_prefix("folder ") {
                let name = name.trim();
                if name.is_empty() {
                    None
                } else {
                    Some(Slash::ProjectFolder(name.to_string()))
                }
            } else if let Some(name) = rest.strip_prefix("rename ") {
                let name = name.trim();
                if name.is_empty() {
                    None
                } else {
                    Some(Slash::ProjectRename(name.to_string()))
                }
            } else if let Some(name) = rest.strip_prefix("move ") {
                let name = name.trim();
                if name.is_empty() {
                    None
                } else {
                    Some(Slash::ProjectMove(name.to_string()))
                }
            } else {
                let path = rest
                    .strip_prefix("bind")
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(rest);
                Some(Slash::ProjectBind(Some(path.to_string())))
            }
        }
        "/send" if !rest.is_empty() => Some(Slash::Send(rest.to_string())),
        "/sync" => Some(Slash::Sync),
        "/hub" => Some(Slash::Hub),
        "/inhabit" if !rest.is_empty() => Some(Slash::Inhabit(rest.to_string())),
        "/rewind" => Some(Slash::Rewind),
        "/room" if !rest.is_empty() => Some(Slash::Room(rest.to_string())),
        "/export" => Some(Slash::Export),
        _ => None,
    }
}

pub fn slash_kind(s: &Slash) -> &'static str {
    match s {
        Slash::Approve { yolo: true } => "approve_off",
        Slash::Approve { yolo: false } => "approve_on",
        Slash::ApproveRisky => "approve_risky",
        Slash::ApproveAll => "approve_all",
        Slash::Forget(_) => "forget",
        Slash::MemoryNote(_) => "memory",
        Slash::MemoryShow => "memory_show",
        Slash::Recall(_) => "recall",
        Slash::Board => "board",
        Slash::Imagine(_) => "imagine",
        Slash::Compact => "compact",
        Slash::Skill(_) => "skill",
        Slash::LearnReflect => "reflect",
        Slash::Update => "update",
        Slash::Help => "help",
        Slash::New => "new",
        Slash::Scratch => "scratch",
        Slash::Clear => "clear",
        Slash::Undo => "undo",
        Slash::Retry => "retry",
        Slash::Stop => "stop",
        Slash::Sh(_) => "sh",
        Slash::Host { on: true } => "host_on",
        Slash::Host { on: false } => "host_off",
        Slash::ProjectBind(_) => "project_bind",
        Slash::ProjectClear => "project_clear",
        Slash::ProjectShow => "project_show",
        Slash::ProjectNew(_) => "project_new",
        Slash::ProjectFolder(_) => "project_folder",
        Slash::ProjectRename(_) => "project_rename",
        Slash::ProjectMove(_) => "project_move",
        Slash::Send(_) => "send",
        Slash::Sync => "sync",
        Slash::Hub => "hub",
        Slash::Inhabit(_) => "inhabit",
        Slash::Rewind => "rewind",
        Slash::Room(_) => "room",
        Slash::Export => "export",
        Slash::Rename(_) => "rename",
        Slash::Context => "context",
        Slash::Health => "health",
        Slash::Fix => "fix",
        Slash::Remember(_) => "remember",
        Slash::Mode(_) => "mode",
        Slash::Dream => "dream",
        Slash::Tools { on: true } => "tools_on",
        Slash::Tools { on: false } => "tools_off",
        Slash::HostStatus => "host_status",
        Slash::Import => "import",
        Slash::Consult(_) => "consult",
        Slash::Usage => "usage",
        Slash::Models => "models",
        Slash::Palette => "palette",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashDef {
    pub cmd: &'static str,
    pub hint: &'static str,
    pub insert: &'static str,
    pub run_on_pick: bool,
}

pub const SLASH_COMMANDS: &[SlashDef] = &[
    SlashDef { cmd: "/help", hint: "Show slash commands", insert: "/help", run_on_pick: true },
    SlashDef { cmd: "/new", hint: "New chat", insert: "/new", run_on_pick: true },
    SlashDef { cmd: "/scratch", hint: "New scratch chat (no memory)", insert: "/scratch", run_on_pick: true },
    SlashDef { cmd: "/clear", hint: "Clear current chat", insert: "/clear", run_on_pick: true },
    SlashDef { cmd: "/compact", hint: "Compact older turns", insert: "/compact", run_on_pick: true },
    SlashDef { cmd: "/context", hint: "Show context budget", insert: "/context", run_on_pick: true },
    SlashDef { cmd: "/health", hint: "Run install/session health pass", insert: "/health", run_on_pick: true },
    SlashDef { cmd: "/fix", hint: "Self-heal stuck UI + health pass", insert: "/fix", run_on_pick: true },
    SlashDef { cmd: "/memory", hint: "Show memory files", insert: "/memory ", run_on_pick: false },
    SlashDef { cmd: "/learn reflect", hint: "Run self-improve reflect", insert: "/learn reflect", run_on_pick: true },
    SlashDef { cmd: "/mode", hint: "Set mode…", insert: "/mode ", run_on_pick: false },
    SlashDef { cmd: "/imagine", hint: "Open Imagine", insert: "/imagine ", run_on_pick: false },
    SlashDef { cmd: "/export", hint: "Export chat markdown", insert: "/export", run_on_pick: true },
    SlashDef { cmd: "/rename", hint: "Rename chat…", insert: "/rename ", run_on_pick: false },
    SlashDef { cmd: "/remember", hint: "Save durable memory note", insert: "/remember ", run_on_pick: false },
    SlashDef { cmd: "/project", hint: "Show bound project", insert: "/project", run_on_pick: true },
    SlashDef { cmd: "/project bind", hint: "Bind a folder as the world", insert: "/project bind ", run_on_pick: false },
    SlashDef { cmd: "/project new", hint: "Create a project", insert: "/project new ", run_on_pick: false },
    SlashDef { cmd: "/project folder", hint: "Create a sidebar folder", insert: "/project folder ", run_on_pick: false },
    SlashDef { cmd: "/project rename", hint: "Rename the selected project", insert: "/project rename ", run_on_pick: false },
    SlashDef { cmd: "/project move", hint: "Add the selected project to a folder", insert: "/project move ", run_on_pick: false },
    SlashDef { cmd: "/board", hint: "Open the Workboard", insert: "/board", run_on_pick: true },
    SlashDef { cmd: "/skill", hint: "Run a skill…", insert: "/skill ", run_on_pick: false },
    SlashDef { cmd: "/host", hint: "Desktop host status", insert: "/host", run_on_pick: true },
    SlashDef { cmd: "/approve off", hint: "YOLO — run without confirm", insert: "/approve off", run_on_pick: true },
    SlashDef { cmd: "/recall", hint: "Search chats and memory", insert: "/recall ", run_on_pick: false },
    SlashDef { cmd: "/forget", hint: "Remove a memory topic", insert: "/forget ", run_on_pick: false },
    SlashDef { cmd: "/undo", hint: "Drop last assistant turn", insert: "/undo", run_on_pick: true },
    SlashDef { cmd: "/retry", hint: "Re-send last user prompt", insert: "/retry", run_on_pick: true },
    SlashDef { cmd: "/stop", hint: "Stop generation", insert: "/stop", run_on_pick: true },
    SlashDef { cmd: "/sh", hint: "Run shell on host", insert: "/sh ", run_on_pick: false },
    SlashDef { cmd: "$", hint: "Host shell shortcut", insert: "$ ", run_on_pick: false },
    SlashDef { cmd: "/hub", hint: "Device hub status", insert: "/hub", run_on_pick: true },
    SlashDef { cmd: "/sync", hint: "Sync chats & memory with paired computers", insert: "/sync", run_on_pick: true },
    SlashDef { cmd: "/send", hint: "Send a task to another computer", insert: "/send ", run_on_pick: false },
    SlashDef { cmd: "/rewind", hint: "Restore last job snapshot", insert: "/rewind", run_on_pick: true },
    SlashDef { cmd: "/room", hint: "Speak the room — stage a project", insert: "/room ", run_on_pick: false },
    SlashDef { cmd: "/dream", hint: "Imagine last night’s job", insert: "/dream", run_on_pick: true },
    SlashDef { cmd: "/inhabit", hint: "Hand this Grok to another box", insert: "/inhabit ", run_on_pick: false },
    SlashDef { cmd: "/update", hint: "Overlay install", insert: "/update", run_on_pick: true },
    SlashDef { cmd: "/import", hint: "Import OpenClaw workspace", insert: "/import", run_on_pick: true },
    SlashDef { cmd: "/consult", hint: "One-shot consult", insert: "/consult ", run_on_pick: false },
    SlashDef { cmd: "/usage", hint: "Today's usage", insert: "/usage", run_on_pick: true },
    SlashDef { cmd: "/models", hint: "Grok catalog", insert: "/models", run_on_pick: true },
    SlashDef { cmd: "/palette", hint: "Command palette", insert: "/palette", run_on_pick: true },
];

pub fn filter_slash_commands(draft: &str) -> Vec<&'static SlashDef> {
    let t = draft.trim_start();
    if !t.starts_with('/') && !t.starts_with('$') {
        return vec![];
    }
    let parts: Vec<&str> = t.split_whitespace().collect();
    if parts.len() > 2 {
        return vec![];
    }
    let needle = t.to_ascii_lowercase();
    SLASH_COMMANDS
        .iter()
        .filter(|s| {
            let c = s.cmd.to_ascii_lowercase();
            if c.starts_with(&needle) {
                return true;
            }
            if needle.starts_with(&format!("{c} ")) {
                return true;
            }
            if parts.len() == 2 {
                let want = format!("{} {}", parts[0].to_ascii_lowercase(), parts[1].to_ascii_lowercase());
                return c.starts_with(&want);
            }
            false
        })
        .take(12)
        .collect()
}

pub fn resolve_mode_arg(arg: &str) -> Option<String> {
    let a = arg.trim().to_ascii_lowercase();
    let mapped = match a.as_str() {
        "auto" | "adaptive" | "smart" => "auto",
        "fast" => "fast",
        "balance" | "balanced" => "balanced",
        "think" | "thinking" | "expert" => "think",
        "heavy" | "max" | "deep" => "max",
        "build" => "think",
        _ => return None,
    };
    Some(mapped.into())
}

pub fn slash_help() -> String {
    [
        "/help — this list",
        "/new — new chat",
        "/scratch — new scratch chat (no memory)",
        "/clear — clear this chat",
        "/compact — keep last 8 turns",
        "/undo — drop last assistant turn",
        "/retry — re-send last user prompt",
        "/stop — halt the current job",
        "/approve off — YOLO",
        "/approve on — supervised",
        "/approve risky — confirm destructive only",
        "/sh <cmd> — run on this box",
        "/host on|off — host tools",
        "/project bind <path> — bound tree is the world",
        "/project new <name> — create a project",
        "/project folder <name> — create a sidebar folder",
        "/project rename <name> — rename the selected project",
        "/project move <folder>|root — add the selected project to a folder",
        "/board — open the Workboard",
        "/skill <name> — run a skill",
        "/memory note <fact> — write MEMORY.md",
        "/recall <q> — search memory",
        "/forget <topic> — drop matching memory lines",
        "/imagine <prompt>",
        "/update — overlay install",
        "/send <task> — task this box",
        "/hub — devices / pair",
        "/inhabit <peer> — hand this Grok to another box (not the phone)",
        "/rewind — restore last project snapshot",
        "/room <name> — speak the room",
        "/export — write this chat as markdown",
        "/rename <title> — name this chat",
        "/context — context budget",
        "/health — doctor",
        "/fix — halt + doctor",
        "/remember <fact> — write MEMORY.md",
        "/mode auto|fast|balance|think|max — Auto routes; Fast mini; Balance 4.3; Think 4.6 high; Max 4.6 xhigh",
        "/dream — Imagine last night",
        "/tools on|off — host tools",
        "/import — OpenClaw workspace",
        "/consult <q> — one-shot consult",
        "/usage — today's buckets",
        "/models — Grok catalog",
        "/palette — command palette",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_yolo() {
        assert_eq!(parse_slash("/approve off"), Some(Slash::Approve { yolo: true }));
        assert_eq!(parse_slash("/approve on"), Some(Slash::Approve { yolo: false }));
        assert_eq!(parse_slash("/approve risky"), Some(Slash::ApproveRisky));
        assert_eq!(parse_slash("hello"), None);
    }

    #[test]
    fn memory_and_recall() {
        assert_eq!(
            parse_slash("/memory note prefer nvim"),
            Some(Slash::MemoryNote("prefer nvim".into()))
        );
        assert_eq!(parse_slash("/recall pi"), Some(Slash::Recall("pi".into())));
        assert_eq!(parse_slash("/forget wifi"), Some(Slash::Forget(Some("wifi".into()))));
        assert_eq!(parse_slash("/forget"), Some(Slash::Forget(None)));
        assert_eq!(parse_slash("/board"), Some(Slash::Board));
        assert_eq!(parse_slash("/imagine a cabin"), Some(Slash::Imagine("a cabin".into())));
        assert_eq!(parse_slash("/compact"), Some(Slash::Compact));
        assert_eq!(parse_slash("/learn reflect"), Some(Slash::LearnReflect));
        assert_eq!(parse_slash("/update"), Some(Slash::Update));
    }

    #[test]
    fn electron_parity_slash() {
        assert_eq!(parse_slash("/help").as_ref().map(slash_kind), Some("help"));
        assert_eq!(parse_slash("/new"), Some(Slash::New));
        assert_eq!(parse_slash("/scratch"), Some(Slash::Scratch));
        assert_eq!(parse_slash("/undo"), Some(Slash::Undo));
        assert_eq!(parse_slash("/retry"), Some(Slash::Retry));
        assert_eq!(parse_slash("/sh ls /tmp"), Some(Slash::Sh("ls /tmp".into())));
        assert_eq!(parse_slash("$ echo hi"), Some(Slash::Sh("echo hi".into())));
        assert_eq!(parse_slash("/project bind ~/GrokHub-Work"), Some(Slash::ProjectBind(Some("~/GrokHub-Work".into()))));
        assert_eq!(parse_slash("/project new Night watch"), Some(Slash::ProjectNew("Night watch".into())));
        assert_eq!(parse_slash("/project folder Cabin"), Some(Slash::ProjectFolder("Cabin".into())));
        assert_eq!(parse_slash("/project rename Dawn"), Some(Slash::ProjectRename("Dawn".into())));
        assert_eq!(parse_slash("/project move Cabin"), Some(Slash::ProjectMove("Cabin".into())));
        assert_eq!(parse_slash("/inhabit cabin-2"), Some(Slash::Inhabit("cabin-2".into())));
        assert_eq!(parse_slash("/send flash the pi"), Some(Slash::Send("flash the pi".into())));
        assert_eq!(slash_kind(&Slash::Update), "update");
        assert_eq!(parse_slash("/rename night").as_ref().map(slash_kind), Some("rename"));
        assert_eq!(parse_slash("/host"), Some(Slash::HostStatus));
        assert_eq!(parse_slash("/mode max"), Some(Slash::Mode("max".into())));
        assert_eq!(parse_slash("/mode think"), Some(Slash::Mode("think".into())));
        assert_eq!(parse_slash("/mode balance"), Some(Slash::Mode("balanced".into())));
        assert_eq!(parse_slash("/mode balanced"), Some(Slash::Mode("balanced".into())));
        assert_eq!(parse_slash("/dream"), Some(Slash::Dream));
        assert_eq!(parse_slash("/tools off"), Some(Slash::Tools { on: false }));
        assert_eq!(parse_slash("/import"), Some(Slash::Import));
        assert_eq!(
            parse_slash("/consult check the pi"),
            Some(Slash::Consult("check the pi".into()))
        );
        assert_eq!(parse_slash("/usage"), Some(Slash::Usage));
        assert_eq!(parse_slash("/models"), Some(Slash::Models));
        assert_eq!(parse_slash("/palette"), Some(Slash::Palette));
        assert!(slash_help().contains("/import"));
        assert!(slash_help().contains("/consult"));
        assert!(slash_help().contains("/project new"));
        assert!(slash_help().contains("/project folder"));
        assert!(slash_help().contains("/board"));
        assert!(slash_help().contains("/skill"));
        assert!(slash_help().contains("/mode auto|fast|balance|think|max"));
        assert!(filter_slash_commands("/re").iter().any(|s| s.cmd == "/rename"));
        assert!(filter_slash_commands("/project n").iter().any(|s| s.cmd == "/project new"));
        assert!(filter_slash_commands("hello").is_empty());
    }
}
