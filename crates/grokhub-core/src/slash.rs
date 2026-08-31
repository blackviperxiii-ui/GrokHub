//! Composer slash commands. Local — they never go to the model.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slash {
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
    ProjectBind(Option<String>),
    ProjectClear,
    ProjectShow,
    ProjectNew(String),
    ProjectFolder(String),
    ProjectRename(String),
    ProjectMove(String),
    ProjectDelete,
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
    HostStatus,
    Import,
    Consult(String),
    Usage,
    Models,
    Palette,
    Pin,
    Delete,
    Plan,
    AlwaysApprove,
    AutoPerm,
    Effort(String),
    Sessions,
    Inspect,
    Loop(String),
    GrokSkills,
    GrokConnectors,
    Model(String),
    ImagineVideo(String),
    Goal(String),
    Fork,
    Workflow(String),
    RewindFiles,
    Worktree,
}

fn looks_like_bind_path(s: &str) -> bool {
    s.starts_with('/') || s.starts_with('~') || s.starts_with('.')
}

fn strip_bind_word(rest: &str) -> Option<&str> {
    let rest = rest.trim();
    if rest.len() < 4 || !rest[..4].eq_ignore_ascii_case("bind") {
        return None;
    }
    let after = &rest[4..];
    if after.is_empty() {
        return Some("");
    }
    if after.starts_with(char::is_whitespace) {
        return Some(after.trim());
    }
    None
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
        "/approve" => None,
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
        "/imagine" => Some(Slash::Imagine(rest.to_string())),
        "/imagine-video" => Some(Slash::ImagineVideo(rest.to_string())),
        "/loop" => Some(Slash::Loop(rest.to_string())),
        "/fork" => Some(Slash::Fork),
        "/workflow" if rest.is_empty() => Some(Slash::GrokConnectors),
        "/workflow" if !rest.is_empty() => Some(Slash::Workflow(rest.to_string())),
        "/worktree" => Some(Slash::Worktree),
        "/skills" => Some(Slash::GrokSkills),
        "/plugins" | "/marketplace" | "/mcps" | "/hooks" | "/connectors" | "/workflows" => {
            Some(Slash::GrokConnectors)
        }
        "/model" | "/m" if !rest.is_empty() => Some(Slash::Model(rest.to_string())),
        "/goal" => Some(Slash::Goal(rest.to_string())),
        "/dashboard" | "/agents-dashboard" => Some(Slash::Sessions),
        "/mem" => {
            if rest.is_empty() || rest.eq_ignore_ascii_case("show") {
                Some(Slash::MemoryShow)
            } else {
                Some(Slash::Remember(rest.to_string()))
            }
        }
        "/title" if !rest.is_empty() => Some(Slash::Rename(rest.to_string())),
        "/status" | "/info" | "/session-info" | "/doctor" => Some(Slash::Inspect),
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
        "/host" | "/tools" => Some(Slash::HostStatus),
        "/rename" if !rest.is_empty() => Some(Slash::Rename(rest.to_string())),
        "/pin" => Some(Slash::Pin),
        "/delete" | "/close" => Some(Slash::Delete),
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
        "/plan" => Some(Slash::Plan),
        "/always-approve" | "/yolo" => Some(Slash::AlwaysApprove),
        "/auto" => Some(Slash::AutoPerm),
        "/effort" if !rest.is_empty() => Some(Slash::Effort(rest.to_string())),
        "/sessions" | "/resume" => Some(Slash::Sessions),
        "/inspect" => Some(Slash::Inspect),
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
            } else if rest.eq_ignore_ascii_case("delete") {
                Some(Slash::ProjectDelete)
            } else if let Some(name) = rest.strip_prefix("move ") {
                let name = name.trim();
                if name.is_empty() {
                    None
                } else {
                    Some(Slash::ProjectMove(name.to_string()))
                }
            } else if let Some(path) = strip_bind_word(rest) {
                if path.is_empty() {
                    None
                } else {
                    Some(Slash::ProjectBind(Some(path.to_string())))
                }
            } else if looks_like_bind_path(rest) {
                Some(Slash::ProjectBind(Some(rest.to_string())))
            } else {
                None
            }
        }
        "/send" if !rest.is_empty() => Some(Slash::Send(rest.to_string())),
        "/sync" => Some(Slash::Sync),
        "/hub" => Some(Slash::Hub),
        "/inhabit" if !rest.is_empty() => Some(Slash::Inhabit(rest.to_string())),
        "/rewind" if rest == "--files" || rest == "--code" || rest == "files" => {
            Some(Slash::RewindFiles)
        }
        "/rewind" => Some(Slash::Rewind),
        "/room" if !rest.is_empty() => Some(Slash::Room(rest.to_string())),
        "/export" => Some(Slash::Export),
        _ => None,
    }
}

/// Retired cabin-only slashes. Grok Build skills (`/create-skill`) and CLI
/// builtins the cabin does not handle must reach `grok -p`.
pub fn unknown_cabin_slash(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with('/') {
        return false;
    }
    if parse_slash(t).is_some() {
        return false;
    }
    let cmd = t
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    cmd == "/approve" || t.to_ascii_lowercase().starts_with("/project binding")
}

pub const SLASH_RESULT_PREFIX: &str = "SLASH_RESULT:";

pub fn mark_slash_result(body: &str) -> String {
    format!("{SLASH_RESULT_PREFIX}\n{body}")
}

pub fn strip_slash_result(text: &str) -> &str {
    let t = text
        .strip_prefix(SLASH_RESULT_PREFIX)
        .map(|s| s.strip_prefix('\n').unwrap_or(s))
        .unwrap_or(text);
    t
}

/// Help / models / recall dumps stay on the pane and out of the next model kick.
pub fn is_cabin_slash_turn(role: &str, content: &str) -> bool {
    let t = content.trim_start();
    if t.starts_with(SLASH_RESULT_PREFIX) {
        return true;
    }
    if role == "user" {
        return parse_slash(t).is_some_and(|s| {
            matches!(s, Slash::Help | Slash::Models | Slash::Recall(_))
        });
    }
    if role != "assistant" {
        return false;
    }
    t.starts_with("/help — this list")
        || (t.starts_with("grok-3-mini-fast — ") && t.contains("grok-4.6 — "))
}

pub fn slash_kind(s: &Slash) -> &'static str {
    match s {
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
        Slash::ProjectBind(_) => "project_bind",
        Slash::ProjectClear => "project_clear",
        Slash::ProjectShow => "project_show",
        Slash::ProjectNew(_) => "project_new",
        Slash::ProjectFolder(_) => "project_folder",
        Slash::ProjectRename(_) => "project_rename",
        Slash::ProjectMove(_) => "project_move",
        Slash::ProjectDelete => "project_delete",
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
        Slash::HostStatus => "host_status",
        Slash::Import => "import",
        Slash::Consult(_) => "consult",
        Slash::Usage => "usage",
        Slash::Models => "models",
        Slash::Palette => "palette",
        Slash::Pin => "pin",
        Slash::Delete => "delete",
        Slash::Plan => "plan",
        Slash::AlwaysApprove => "always_approve",
        Slash::AutoPerm => "auto_perm",
        Slash::Effort(_) => "effort",
        Slash::Sessions => "sessions",
        Slash::Inspect => "inspect",
        Slash::Loop(_) => "loop",
        Slash::GrokSkills => "grok_skills",
        Slash::GrokConnectors => "grok_connectors",
        Slash::Model(_) => "model",
        Slash::ImagineVideo(_) => "imagine_video",
        Slash::Goal(_) => "goal",
        Slash::Fork => "fork",
        Slash::Workflow(_) => "workflow",
        Slash::RewindFiles => "rewind_files",
        Slash::Worktree => "worktree",
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
    SlashDef { cmd: "/compact", hint: "Compact Grok context", insert: "/compact", run_on_pick: true },
    SlashDef { cmd: "/context", hint: "Show Grok Build context", insert: "/context", run_on_pick: true },
    SlashDef { cmd: "/health", hint: "Run install/session health pass", insert: "/health", run_on_pick: true },
    SlashDef { cmd: "/fix", hint: "Self-heal stuck UI + health pass", insert: "/fix", run_on_pick: true },
    SlashDef { cmd: "/memory", hint: "Show memory files", insert: "/memory ", run_on_pick: false },
    SlashDef { cmd: "/learn reflect", hint: "Run self-improve reflect", insert: "/learn reflect", run_on_pick: true },
    SlashDef { cmd: "/mode", hint: "Set mode…", insert: "/mode ", run_on_pick: false },
    SlashDef { cmd: "/imagine", hint: "Open Imagine", insert: "/imagine ", run_on_pick: false },
    SlashDef { cmd: "/export", hint: "Export chat markdown", insert: "/export", run_on_pick: true },
    SlashDef { cmd: "/rename", hint: "Rename chat…", insert: "/rename ", run_on_pick: false },
    SlashDef { cmd: "/pin", hint: "Pin or unpin this chat", insert: "/pin", run_on_pick: true },
    SlashDef { cmd: "/delete", hint: "Delete this chat tab", insert: "/delete", run_on_pick: true },
    SlashDef { cmd: "/remember", hint: "Save durable memory note", insert: "/remember ", run_on_pick: false },
    SlashDef { cmd: "/project", hint: "Show bound project", insert: "/project", run_on_pick: true },
    SlashDef { cmd: "/project bind", hint: "Bind a folder as the world", insert: "/project bind ", run_on_pick: false },
    SlashDef { cmd: "/project new", hint: "Create a project", insert: "/project new ", run_on_pick: false },
    SlashDef { cmd: "/project folder", hint: "Create a sidebar folder", insert: "/project folder ", run_on_pick: false },
    SlashDef { cmd: "/project rename", hint: "Rename the selected project", insert: "/project rename ", run_on_pick: false },
    SlashDef { cmd: "/project move", hint: "Add the selected project to a folder", insert: "/project move ", run_on_pick: false },
    SlashDef { cmd: "/project delete", hint: "Remove the selected project from the sidebar", insert: "/project delete", run_on_pick: true },
    SlashDef { cmd: "/board", hint: "Open the Workboard", insert: "/board", run_on_pick: true },
    SlashDef { cmd: "/skill", hint: "Run a skill…", insert: "/skill ", run_on_pick: false },
    SlashDef { cmd: "/host", hint: "Desktop host status", insert: "/host", run_on_pick: true },
    SlashDef { cmd: "/recall", hint: "Search chats and memory", insert: "/recall ", run_on_pick: false },
    SlashDef { cmd: "/forget", hint: "Remove a memory topic", insert: "/forget ", run_on_pick: false },
    SlashDef { cmd: "/undo", hint: "Rewind Grok conversation", insert: "/undo", run_on_pick: true },
    SlashDef { cmd: "/retry", hint: "Re-send last user prompt", insert: "/retry", run_on_pick: true },
    SlashDef { cmd: "/stop", hint: "Stop generation", insert: "/stop", run_on_pick: true },
    SlashDef { cmd: "/sh", hint: "Run shell on host", insert: "/sh ", run_on_pick: false },
    SlashDef { cmd: "$", hint: "Host shell shortcut", insert: "$ ", run_on_pick: false },
    SlashDef { cmd: "/hub", hint: "Device hub status", insert: "/hub", run_on_pick: true },
    SlashDef { cmd: "/sync", hint: "Sync chats & memory with paired computers", insert: "/sync", run_on_pick: true },
    SlashDef { cmd: "/send", hint: "Send a task to another computer", insert: "/send ", run_on_pick: false },
    SlashDef { cmd: "/rewind", hint: "Rewind Grok conversation", insert: "/rewind", run_on_pick: true },
    SlashDef { cmd: "/rewind --files", hint: "Restore last project snapshot", insert: "/rewind --files", run_on_pick: true },
    SlashDef { cmd: "/fork", hint: "Fork this Grok session", insert: "/fork", run_on_pick: true },
    SlashDef { cmd: "/worktree", hint: "Next chat in a git worktree", insert: "/worktree", run_on_pick: true },
    SlashDef { cmd: "/workflow", hint: "Launch a Grok workflow…", insert: "/workflow ", run_on_pick: false },
    SlashDef { cmd: "/room", hint: "Speak the room — stage a project", insert: "/room ", run_on_pick: false },
    SlashDef { cmd: "/dream", hint: "Imagine last night’s job", insert: "/dream", run_on_pick: true },
    SlashDef { cmd: "/inhabit", hint: "Hand this Grok to another box", insert: "/inhabit ", run_on_pick: false },
    SlashDef { cmd: "/update", hint: "Overlay install", insert: "/update", run_on_pick: true },
    SlashDef { cmd: "/import", hint: "Import OpenClaw workspace", insert: "/import", run_on_pick: true },
    SlashDef { cmd: "/consult", hint: "One-shot consult", insert: "/consult ", run_on_pick: false },
    SlashDef { cmd: "/usage", hint: "Today's usage + Grok spend", insert: "/usage", run_on_pick: true },
    SlashDef { cmd: "/models", hint: "Grok catalog", insert: "/models", run_on_pick: true },
    SlashDef { cmd: "/palette", hint: "Command palette", insert: "/palette", run_on_pick: true },
    SlashDef { cmd: "/plan", hint: "Grok Build plan mode", insert: "/plan", run_on_pick: true },
    SlashDef { cmd: "/always-approve", hint: "Skip Grok tool prompts", insert: "/always-approve", run_on_pick: true },
    SlashDef { cmd: "/sessions", hint: "List Grok Build sessions", insert: "/sessions", run_on_pick: true },
    SlashDef { cmd: "/resume", hint: "Resume a Grok Build session", insert: "/resume", run_on_pick: true },
    SlashDef { cmd: "/inspect", hint: "Inspect Grok Build config", insert: "/inspect", run_on_pick: true },
    SlashDef { cmd: "/loop", hint: "Schedule a Grok /loop…", insert: "/loop ", run_on_pick: false },
    SlashDef { cmd: "/skills", hint: "Grok Build skills", insert: "/skills", run_on_pick: true },
    SlashDef { cmd: "/plugins", hint: "Grok Build plugins and marketplace", insert: "/plugins", run_on_pick: true },
    SlashDef { cmd: "/mcps", hint: "Grok Build MCP servers", insert: "/mcps", run_on_pick: true },
    SlashDef { cmd: "/model", hint: "Set grok -p --model…", insert: "/model ", run_on_pick: false },
    SlashDef { cmd: "/imagine-video", hint: "Open Imagine video", insert: "/imagine-video ", run_on_pick: false },
    SlashDef { cmd: "/goal", hint: "Pin a Grok goal…", insert: "/goal ", run_on_pick: false },
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashHit {
    pub cmd: String,
    pub hint: String,
    pub insert: String,
    pub run_on_pick: bool,
}

impl From<&SlashDef> for SlashHit {
    fn from(s: &SlashDef) -> Self {
        Self {
            cmd: s.cmd.to_string(),
            hint: s.hint.to_string(),
            insert: s.insert.to_string(),
            run_on_pick: s.run_on_pick,
        }
    }
}

pub fn grok_command_hits(names: &[String]) -> Vec<SlashHit> {
    let mut out = Vec::new();
    for name in names {
        let n = name.trim().trim_start_matches('/');
        if n.is_empty() {
            continue;
        }
        let cmd = format!("/{n}");
        if SLASH_COMMANDS.iter().any(|s| s.cmd.eq_ignore_ascii_case(&cmd)) {
            continue;
        }
        out.push(SlashHit {
            hint: "Grok Build".into(),
            insert: format!("{cmd} "),
            cmd,
            run_on_pick: false,
        });
    }
    out
}

pub fn filter_slash_hits(draft: &str, extra: &[SlashHit]) -> Vec<SlashHit> {
    let mut hits: Vec<SlashHit> = filter_slash_commands(draft)
        .into_iter()
        .map(SlashHit::from)
        .collect();
    let needle = draft.trim_start().to_ascii_lowercase();
    if needle.starts_with('/') {
        for e in extra {
            let c = e.cmd.to_ascii_lowercase();
            if (c.starts_with(&needle) || needle.starts_with(&format!("{c} ")))
                && !hits.iter().any(|h| h.cmd.eq_ignore_ascii_case(&e.cmd))
            {
                hits.push(e.clone());
            }
        }
    }
    hits.truncate(12);
    hits
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
        "/new — new chat (new Grok Build session)",
        "/scratch — new scratch chat (no memory; /forget and Memory Save stay off)",
        "/clear — clear this chat",
        "/compact — compact Grok context (also trims the painted pane)",
        "/undo — rewind Grok conversation (alias /rewind)",
        "/retry — re-send last user prompt",
        "/stop — halt the current Grok Build turn",
        "/sh <cmd> — run a local shell (you, not the agent)",
        "/host — Grok Build CLI status",
        "/plan — plan mode (Grok Build)",
        "/always-approve — skip tool permission prompts",
        "/auto — auto-approve safe tools",
        "/effort <none|minimal|low|medium|high|xhigh|max> — reasoning effort (composer dropdown too)",
        "/sessions — Grok Build sessions",
        "/resume — same as /sessions (Grok /resume)",
        "/inspect — grok inspect --json against ~/.grok",
        "/loop [30m] <prompt> — Grok Build interval scheduler",
        "every weekday at 9, <task> — clock job on the cabin pulse (Automations page)",
        "/skills — skills catalog: cabin skills and the Grok Build list",
        "/plugins /marketplace /mcps — connectors",
        "/model <id> — grok -p --model",
        "/imagine-video <prompt> — Imagine video",
        "/goal <objective> — pin a Grok goal",
        "Grok skill slashes such as /create-skill go to grok -p.",
        "/project bind <path> — bound tree is the world (ACP cwd)",
        "/project new <name> — create a project",
        "/project folder <name> — create a sidebar folder",
        "/project rename <name> — rename the selected project",
        "/project move <folder>|root — add the selected project to a folder",
        "/project delete — remove the selected project from the sidebar",
        "/board — open the Workboard",
        "/skill <name> — run a skill",
        "/memory note <fact> — write MEMORY.md",
        "/recall <q> — search memory, learned insights, and chats",
        "/forget <topic> — drop memory lines that mention the topic (whole words)",
        "/imagine <prompt>",
        "/update — overlay install (GUI only). Restart on Settings. `grok update` updates the agent.",
        "/send <task> — task this box",
        "/sync — merge chats and memory with paired computers",
        "/hub — devices / pair",
        "/inhabit <peer> — hand this Grok to another box (not the phone)",
        "/rewind — rewind Grok conversation; /rewind --files restores the last project snapshot",
        "/fork — fork the Grok session into a new chat tab",
        "/worktree — next chat starts in a git worktree",
        "/workflow <name> — launch a Grok Build workflow",
        "/room <name> — speak the room",
        "/export — write this chat as markdown",
        "/rename <title> — name this chat (permanent)",
        "/pin — pin or unpin this chat",
        "/delete — delete this chat tab",
        "/context — Grok Build context (server tokens + reasoning; visible turns as fallback)",
        "/health — doctor",
        "/fix — halt + doctor",
        "/remember <fact> — write MEMORY.md",
        "/mode auto|fast|balance|think|max — legacy composer ladder (use Effort dropdown / /effort)",
        "/dream — Imagine last night",
        "/tools — same as /host",
        "/import — OpenClaw workspace",
        "/consult <q> — one-shot consult",
        "/usage — today's cabin buckets, tokens spent today, and the last Grok Build turn",
        "/models — Grok catalog",
        "/palette — command palette",
        "Enter sends; Ctrl+Enter newline. Send becomes Stop while a reply runs.",
        "Mode pill: Chat / Plan / Ask. Permission: Ask / Auto / Always-approve. Both pills are remembered; Always-approve resets to Ask on the next launch. Effort: Low / Medium / High / Extra High. Grok Build runs the agent.",
        "Settings → Behavior: close to tray, living wall, quiet hours, automations a day, host commands an hour.",
        "Appearance: Dark, Light, System. Interactive chat is grok agent stdio (ACP). Night and phone use grok -p. Halt is session/cancel.",
        "Voice: OAuth for STT/TTS; duplex streams PCM with a console key. Desktop control is Grok Build computer-use — Halt cancels the ACP turn.",
        "install.sh installs the Grok Build CLI (grok) from https://x.ai/cli. Settings shows grok --version. Cabin overlay updates the GUI and installs grok if missing; grok update updates the agent.",
        "× to tray; a pinned taskbar click or second grokhub raises the cabin.",
        "Pulse every 15s. Hidden idle waits for the pulse.",
        "Devices pair URL is a LAN IPv4. Expired pair codes hide and rotate. Hub complete is owner-only.",
        "Chat rail opens the last-accessed thread.",
        "Tool calls, diffs, and desk frames render in the pane. Permission prompts Allow / Deny — Enter allows and Esc denies when the composer is empty. User bubbles sit on the right.",
        "Five chips sit centered over the composer.",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_gone() {
        assert_eq!(parse_slash("/approve off"), None);
        assert_eq!(parse_slash("/approve on"), None);
        assert_eq!(parse_slash("/approve risky"), None);
        assert_eq!(parse_slash("/approve"), None);
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
        assert_eq!(parse_slash("/fork"), Some(Slash::Fork));
        assert_eq!(parse_slash("/rewind --files"), Some(Slash::RewindFiles));
        assert_eq!(parse_slash("/workflow review"), Some(Slash::Workflow("review".into())));
        assert_eq!(parse_slash("/worktree"), Some(Slash::Worktree));
        let extra = grok_command_hits(&["create-skill".into(), "help".into()]);
        assert!(extra.iter().any(|h| h.cmd == "/create-skill"));
        assert!(!extra.iter().any(|h| h.cmd == "/help"));
        let hits = filter_slash_hits("/cre", &extra);
        assert!(hits.iter().any(|h| h.cmd == "/create-skill"), "{hits:?}");
        assert_eq!(parse_slash("/learn reflect"), Some(Slash::LearnReflect));
        assert_eq!(parse_slash("/update"), Some(Slash::Update));
    }

    #[test]
    fn cabin_slash() {
        assert_eq!(parse_slash("/help").as_ref().map(slash_kind), Some("help"));
        assert_eq!(parse_slash("/new"), Some(Slash::New));
        assert_eq!(parse_slash("/scratch"), Some(Slash::Scratch));
        assert_eq!(parse_slash("/undo"), Some(Slash::Undo));
        assert_eq!(parse_slash("/retry"), Some(Slash::Retry));
        assert_eq!(parse_slash("/sh ls /tmp"), Some(Slash::Sh("ls /tmp".into())));
        assert_eq!(parse_slash("$ echo hi"), Some(Slash::Sh("echo hi".into())));
        assert_eq!(parse_slash("/project bind ~/GrokHub-Work"), Some(Slash::ProjectBind(Some("~/GrokHub-Work".into()))));
        assert_eq!(parse_slash("/project binding ~/GrokHub-Work"), None);
        assert!(unknown_cabin_slash("/project binding ~/GrokHub-Work"));
        assert!(unknown_cabin_slash("/approve"));
        assert!(!unknown_cabin_slash("/help"));
        assert!(!unknown_cabin_slash("hello"));
        assert!(
            !unknown_cabin_slash("/create-skill"),
            "Grok skill slashes must reach grok -p"
        );
        assert!(!unknown_cabin_slash("/workflow runs"));
        assert_eq!(parse_slash("/loop 30m check deploy").as_ref().map(slash_kind), Some("loop"));
        assert_eq!(parse_slash("/skills"), Some(Slash::GrokSkills));
        assert_eq!(parse_slash("/mcps"), Some(Slash::GrokConnectors));
        assert_eq!(parse_slash("/model grok-4.6").as_ref().map(slash_kind), Some("model"));
        assert_eq!(parse_slash("/m grok-4.5").as_ref().map(slash_kind), Some("model"));
        assert_eq!(parse_slash("/imagine-video a cat").as_ref().map(slash_kind), Some("imagine_video"));
        assert_eq!(parse_slash("/goal migrate auth").as_ref().map(slash_kind), Some("goal"));
        assert_eq!(parse_slash("/resume"), Some(Slash::Sessions));
        assert_eq!(parse_slash("/dashboard"), Some(Slash::Sessions));
        assert_eq!(parse_slash("/project ~/GrokHub-Work"), Some(Slash::ProjectBind(Some("~/GrokHub-Work".into()))));
        assert_eq!(parse_slash("/project /tmp/cabin"), Some(Slash::ProjectBind(Some("/tmp/cabin".into()))));
        assert_eq!(parse_slash("/project typo"), None);
        assert_eq!(parse_slash("/project bind"), None);
        assert_eq!(
            parse_slash("/project bind ."),
            Some(Slash::ProjectBind(Some(".".into())))
        );
        assert_eq!(parse_slash("/project new Night watch"), Some(Slash::ProjectNew("Night watch".into())));
        assert_eq!(parse_slash("/project folder Cabin"), Some(Slash::ProjectFolder("Cabin".into())));
        assert_eq!(parse_slash("/project rename Dawn"), Some(Slash::ProjectRename("Dawn".into())));
        assert_eq!(parse_slash("/project move Cabin"), Some(Slash::ProjectMove("Cabin".into())));
        assert_eq!(parse_slash("/project delete"), Some(Slash::ProjectDelete));
        assert_eq!(parse_slash("/inhabit cabin-2"), Some(Slash::Inhabit("cabin-2".into())));
        assert_eq!(parse_slash("/send flash the pi"), Some(Slash::Send("flash the pi".into())));
        assert_eq!(slash_kind(&Slash::Update), "update");
        assert_eq!(parse_slash("/rename night").as_ref().map(slash_kind), Some("rename"));
        assert_eq!(parse_slash("/pin"), Some(Slash::Pin));
        assert_eq!(parse_slash("/delete"), Some(Slash::Delete));
        assert_eq!(parse_slash("/close"), Some(Slash::Delete));
        assert_eq!(parse_slash("/host"), Some(Slash::HostStatus));
        assert_eq!(parse_slash("/mode max"), Some(Slash::Mode("max".into())));
        assert_eq!(parse_slash("/mode think"), Some(Slash::Mode("think".into())));
        assert_eq!(parse_slash("/mode balance"), Some(Slash::Mode("balanced".into())));
        assert_eq!(parse_slash("/mode balanced"), Some(Slash::Mode("balanced".into())));
        assert_eq!(parse_slash("/dream"), Some(Slash::Dream));
        assert_eq!(parse_slash("/host off"), Some(Slash::HostStatus));
        assert_eq!(parse_slash("/tools off"), Some(Slash::HostStatus));
        assert_eq!(parse_slash("/tools on"), Some(Slash::HostStatus));
        assert!(!slash_help().contains("/approve"));
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
        assert!(slash_help().contains("/project delete"));
        assert!(slash_help().contains("/board"));
        assert!(slash_help().contains("/skill"));
        assert!(slash_help().contains("/plan — plan mode"));
        assert!(slash_help().contains("/always-approve"));
        assert!(slash_help().contains("Pulse every 15s"));
        assert!(slash_help().contains("Grok Build computer-use"));
        assert!(slash_help().contains("Devices pair URL is a LAN IPv4"));
        assert!(slash_help().contains("Mode pill: Chat / Plan / Ask"));
        assert!(slash_help().contains("centered over the composer"));
        assert!(slash_help().contains("Hidden idle waits for the pulse"));
        assert!(slash_help().contains("Chat rail opens the last-accessed thread"));
        assert!(slash_help().contains("Tool calls, diffs, and desk frames"));
        assert!(slash_help().contains("User bubbles sit on the right"));
        assert!(slash_help().contains("compact Grok context"));
        assert!(slash_help().contains("/forget and Memory Save stay off"));
        assert!(slash_help().contains("/skill <name> — run a skill"));
        assert!(slash_help().contains("/sync — merge chats and memory"));
        assert!(slash_help().contains("Expired pair codes hide and rotate"));
        assert!(slash_help().contains("x.ai/cli"));
        assert!(slash_help().contains("pinned taskbar click"));
        assert_eq!(parse_slash("/plan"), Some(Slash::Plan));
        assert_eq!(parse_slash("/always-approve"), Some(Slash::AlwaysApprove));
        assert_eq!(parse_slash("/sessions"), Some(Slash::Sessions));
        assert_eq!(parse_slash("/inspect"), Some(Slash::Inspect));
        assert!(slash_help().contains("/loop"));
        assert!(slash_help().contains("/create-skill"));
        assert!(filter_slash_commands("/re").iter().any(|s| s.cmd == "/rename"));
        assert!(filter_slash_commands("/project n").iter().any(|s| s.cmd == "/project new"));
        assert!(filter_slash_commands("hello").is_empty());
        let help = mark_slash_result(&slash_help());
        assert!(is_cabin_slash_turn("assistant", &help));
        assert_eq!(
            strip_slash_result(&help).lines().next(),
            Some("/help — this list")
        );
        assert!(is_cabin_slash_turn("assistant", &slash_help()));
        assert!(is_cabin_slash_turn(
            "assistant",
            "grok-3-mini-fast — Grok 3 Mini Fast (chat)\ngrok-4.6 — Grok 4.6 (chat)"
        ));
        assert!(!is_cabin_slash_turn("assistant", "Hello Viper"));
        assert!(is_cabin_slash_turn("user", "/help"));
    }
}
