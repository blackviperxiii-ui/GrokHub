//! Composer slash commands. Local — they never go to the model.

mod cmds;
mod effect;
mod parser;
mod registry;

pub use effect::{slash_effect, SlashEffect, SLASH_SAMPLES};
pub use parser::parse_slash;
pub use registry::{SlashEntry, SLASH_REGISTRY};

/// `pause` / `resume` / `stop` on `/workflow`. Matched case-insensitively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowVerb {
    Pause,
    Resume,
    Stop,
}

impl WorkflowVerb {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Stop => "stop",
        }
    }

    pub fn parse(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "pause" => Some(Self::Pause),
            "resume" => Some(Self::Resume),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slash {
    Forget(Option<String>),
    MemoryNote(String),
    MemoryShow,
    /// `/memory dream`: the latest AMR dream report, as chat text.
    MemoryDream,
    /// `/memory prune`: run memory retention now and say what it retired.
    MemoryPrune,
    Recall(String),
    Board,
    /// `/journal` — open the decision backlog / experiment journal.
    Journal,
    Imagine(String),
    Compact,
    Skill(String),
    LearnReflect,
    /// `/learned`, `/learned forget <id>`, `/learned wrong <id>`: what the
    /// learning engine holds. The text after the command, as typed.
    Learned(String),
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
    /// `/privacy`: grants, learning scopes, and what left this computer (Spike-4a). Read only.
    Privacy,
    /// `/diagnose`: read-only checks of disk, memory, services, logs, network and updates (Spike-8b).
    Diagnose,
    /// `/record [what's wrong]` starts a consented screen recording; `/record stop` ends it.
    Record(String),
    RecordStop,
    /// `/audiocheck [input]`: list outputs and inputs, record a short clip and
    /// say what's wrong with it. Changes nothing.
    AudioCheck(String),
    Hub,
    Inhabit(String),
    Rewind,
    /// `/rewind step` lists agent file checkpoints; `/rewind step <n>` restores one.
    RewindStep(Option<u64>),
    Room(String),
    Export,
    /// `/export html`, `/export json`, or `/export md`.
    ExportAs(String),
    Rename(String),
    Context,
    Health,
    /// Weekly state-of-systems briefing (local aggregation; also Sunday night).
    Brief,
    Fix,
    Remember(String),
    Dream,
    HostStatus,
    Import,
    Consult(String),
    Usage,
    Palette,
    Pin,
    Delete,
    Plan,
    AlwaysApprove,
    AutoPerm,
    Sessions,
    Inspect,
    Loop(String),
    GrokSkills,
    /// `/skills changes` (and bare `/skills undo`): what GrokHub changed in
    /// your skills, with an Undo click per skill.
    SkillChanges,
    /// `/skills undo <name>`: put back the version before the newest change.
    SkillUndo(String),
    /// `/skills restore <name>`: bring back a skill that was removed.
    SkillRestore(String),
    /// `/connections changes`: connections {product} added, changed, or turned
    /// off on its own, each with an Undo click.
    ConnectionChanges,
    /// `/automations changes`: automations {product} made or changed on its own.
    AutomationChanges,
    GrokConnectors,
    /// `/hooks` — Connectors tab, Hooks section.
    GrokHooks,
    /// Empty `/workflow` and `/workflows`. Workflows were a CLI feature; the
    /// cabin answers that they are gone.
    GrokWorkflows,
    ImagineVideo(String),
    Goal(String),
    Workflow(String),
    /// `/workflow pause|resume|stop <name-or-run-id>`.
    WorkflowCtl {
        verb: WorkflowVerb,
        target: String,
    },
    /// `/workflow pause|resume|stop` with no target.
    WorkflowUsage,
    RewindFiles,
    Worktree,
    /// Select the btw side-ask pill. Persist id stays `ask`.
    Btw,
    /// Open the stored plan (`/view-plan`, `/show-plan`, `/plan-view`).
    ViewPlan,
    /// `/bg <task>` runs a task beside this chat. Bare `/bg` lists the
    /// background runs; `/bg stop` stops every one.
    Background(String),
    /// `/queue <message>` holds the message until the live reply ends instead of steering it.
    Queue(String),
}

/// Retired cabin-only slashes. The router alone picks the model and effort, so
/// the commands that once changed or showed them are retired too, engine
/// builtins of the same name included. Skill slashes (`/create-skill`) and
/// other engine builtins the cabin does not handle go to the engine as a turn.
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
    RETIRED_SLASH.contains(&cmd.as_str()) || t.to_ascii_lowercase().starts_with("/project binding")
}

/// Command words the cabin no longer has.
const RETIRED_SLASH: &[&str] = &[
    "/approve", "/model", "/m", "/mode", "/models", "/effort", "/why",
];

pub const SLASH_RESULT_PREFIX: &str = "SLASH_RESULT:";

pub fn mark_slash_result(body: &str) -> String {
    format!("{SLASH_RESULT_PREFIX}\n{body}")
}

pub fn strip_slash_result(text: &str) -> &str {
    text.strip_prefix(SLASH_RESULT_PREFIX)
        .map(|s| s.strip_prefix('\n').unwrap_or(s))
        .unwrap_or(text)
}

/// A saved message as the chat shows it: no findings-card tag and no turn
/// labels. Exports and search read this, so a copied chat never carries the
/// app's own markers.
pub fn shown_text(text: &str) -> String {
    let trimmed = text.trim_start();
    let body = if trimmed.starts_with(SLASH_RESULT_PREFIX) {
        strip_slash_result(trimmed)
    } else {
        text
    };
    crate::strip_turn_labels(body)
}

/// Help / recall dumps (and model catalogs saved by older builds) stay on the
/// pane and out of the next model kick.
pub fn is_cabin_slash_turn(role: &str, content: &str) -> bool {
    let t = content.trim_start();
    if t.starts_with(SLASH_RESULT_PREFIX) {
        return true;
    }
    if role == "user" {
        return parse_slash(t).is_some_and(|s| matches!(s, Slash::Help | Slash::Recall(_)));
    }
    if role != "assistant" {
        return false;
    }
    t.starts_with("/help — this list")
        || (t.starts_with("grok-3-mini-fast — ")
            && (t.contains("grok-4.7 — ") || t.contains("grok-4.6 — ")))
}

pub fn slash_kind(s: &Slash) -> &'static str {
    match s {
        Slash::Forget(_) => "forget",
        Slash::MemoryNote(_) => "memory",
        Slash::MemoryShow => "memory_show",
        Slash::MemoryDream => "memory_dream",
        Slash::MemoryPrune => "memory_prune",
        Slash::Recall(_) => "recall",
        Slash::Board => "board",
        Slash::Journal => "journal",
        Slash::Imagine(_) => "imagine",
        Slash::Compact => "compact",
        Slash::Skill(_) => "skill",
        Slash::LearnReflect => "reflect",
        Slash::Learned(_) => "learned",
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
        Slash::Privacy => "privacy",
        Slash::Diagnose => "diagnose",
        Slash::Record(_) => "record",
        Slash::RecordStop => "record_stop",
        Slash::AudioCheck(_) => "audiocheck",
        Slash::Hub => "hub",
        Slash::Inhabit(_) => "inhabit",
        Slash::Rewind => "rewind",
        Slash::RewindStep(_) => "rewind_step",
        Slash::Room(_) => "room",
        Slash::Export => "export",
        Slash::ExportAs(_) => "export",
        Slash::Rename(_) => "rename",
        Slash::Context => "context",
        Slash::Health => "health",
        Slash::Brief => "brief",
        Slash::Fix => "fix",
        Slash::Remember(_) => "remember",
        Slash::Dream => "dream",
        Slash::HostStatus => "host_status",
        Slash::Import => "import",
        Slash::Consult(_) => "consult",
        Slash::Usage => "usage",
        Slash::Palette => "palette",
        Slash::Pin => "pin",
        Slash::Delete => "delete",
        Slash::Plan => "plan",
        Slash::AlwaysApprove => "always_approve",
        Slash::AutoPerm => "auto_perm",
        Slash::Sessions => "sessions",
        Slash::Inspect => "inspect",
        Slash::Loop(_) => "loop",
        Slash::GrokSkills => "grok_skills",
        Slash::SkillChanges => "skill_changes",
        Slash::SkillUndo(_) => "skill_undo",
        Slash::SkillRestore(_) => "skill_restore",
        Slash::ConnectionChanges => "connection_changes",
        Slash::AutomationChanges => "automation_changes",
        Slash::GrokConnectors => "grok_connectors",
        Slash::GrokHooks => "grok_hooks",
        Slash::GrokWorkflows => "grok_workflows",
        Slash::ImagineVideo(_) => "imagine_video",
        Slash::Goal(_) => "goal",
        Slash::Workflow(_) => "workflow",
        Slash::WorkflowCtl { .. } => "workflow_ctl",
        Slash::WorkflowUsage => "workflow_usage",
        Slash::RewindFiles => "rewind_files",
        Slash::Worktree => "worktree",
        Slash::Btw => "btw",
        Slash::ViewPlan => "view_plan",
        Slash::Background(_) => "background",
        Slash::Queue(_) => "queue",
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
    SlashDef {
        cmd: "/help",
        hint: "Show slash commands",
        insert: "/help",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/new",
        hint: "New chat",
        insert: "/new",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/scratch",
        hint: "New scratch chat (no memory)",
        insert: "/scratch",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/clear",
        hint: "Clear current chat",
        insert: "/clear",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/compact",
        hint: "Compact the chat context",
        insert: "/compact",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/context",
        hint: "Show context use",
        insert: "/context",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/health",
        hint: "Updates, failed services and the last dream",
        insert: "/health",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/brief",
        hint: "Weekly systems briefing (local)",
        insert: "/brief",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/fix",
        hint: "Self-heal stuck UI + health pass",
        insert: "/fix",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/memory",
        hint: "Show memory files",
        insert: "/memory ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/learn",
        hint: "Run self-improve reflect",
        insert: "/learn",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/learned",
        hint: "What the app learned, and how sure it is",
        insert: "/learned",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/learn reflect",
        hint: "Run self-improve reflect",
        insert: "/learn reflect",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/imagine",
        hint: "Open Imagine",
        insert: "/imagine ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/export",
        hint: "Export chat (md, html, or json)",
        insert: "/export",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/rename",
        hint: "Rename chat…",
        insert: "/rename ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/pin",
        hint: "Pin or unpin this chat",
        insert: "/pin",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/delete",
        hint: "Delete this chat tab",
        insert: "/delete",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/remember",
        hint: "Save durable memory note",
        insert: "/remember ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/project",
        hint: "Show bound project",
        insert: "/project",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/project bind",
        hint: "Bind a folder as the world",
        insert: "/project bind ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/project new",
        hint: "Create a project",
        insert: "/project new ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/project folder",
        hint: "Create a sidebar folder",
        insert: "/project folder ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/project rename",
        hint: "Rename the selected project",
        insert: "/project rename ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/project move",
        hint: "Add the selected project to a folder",
        insert: "/project move ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/project delete",
        hint: "Remove the project; chats return to History",
        insert: "/project delete",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/board",
        hint: "Open Workboards",
        insert: "/board",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/journal",
        hint: "Open decision journal",
        insert: "/journal",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/skill",
        hint: "Run a skill…",
        insert: "/skill ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/host",
        hint: "Desktop host status",
        insert: "/host",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/recall",
        hint: "Search chats and memory",
        insert: "/recall ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/forget",
        hint: "Remove a memory topic",
        insert: "/forget ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/undo",
        hint: "Rewind the conversation",
        insert: "/undo",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/retry",
        hint: "Re-send last user prompt",
        insert: "/retry",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/stop",
        hint: "Stop generation",
        insert: "/stop",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/sh",
        hint: "Run shell on host",
        insert: "/sh ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "$",
        hint: "Host shell shortcut",
        insert: "$ ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/hub",
        hint: "Device hub status",
        insert: "/hub",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/sync",
        hint: "Sync chats & memory with paired computers",
        insert: "/sync",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/privacy",
        hint: "What left this computer, and your grants",
        insert: "/privacy",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/diagnose",
        hint: "Check this computer (read only)",
        insert: "/diagnose",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/record",
        hint: "Record my screen and diagnose it (say what's wrong)",
        insert: "/record ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/audiocheck",
        hint: "Check my audio: devices, levels, clipping, crackle",
        insert: "/audiocheck",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/send",
        hint: "Send a task to another computer",
        insert: "/send ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/rewind",
        hint: "Rewind the conversation",
        insert: "/rewind",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/rewind step",
        hint: "List or restore agent file checkpoints",
        insert: "/rewind step",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/rewind --files",
        hint: "Restore last project snapshot",
        insert: "/rewind --files",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/btw",
        hint: "Side ask — does not stop a live run",
        insert: "/btw",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/bg",
        hint: "Run a task in the background…",
        insert: "/bg ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/queue",
        hint: "Send after the live reply…",
        insert: "/queue ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/view-plan",
        hint: "Show the plan from Plan mode",
        insert: "/view-plan",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/worktree",
        hint: "Next chat in a git worktree",
        insert: "/worktree",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/room",
        hint: "Speak the room — stage a project",
        insert: "/room ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/dream",
        hint: "Imagine last night’s job",
        insert: "/dream",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/inhabit",
        hint: "Hand this chat to another box",
        insert: "/inhabit ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/update",
        hint: "Open Updates",
        insert: "/update",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/import",
        hint: "Import OpenClaw workspace",
        insert: "/import",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/consult",
        hint: "One-shot consult",
        insert: "/consult ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/usage",
        hint: "Today's usage + model spend",
        insert: "/usage",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/palette",
        hint: "Command palette",
        insert: "/palette",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/plan",
        hint: "Plan mode",
        insert: "/plan",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/always-approve",
        hint: "Skip tool prompts",
        insert: "/always-approve",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/sessions",
        hint: "Open History",
        insert: "/sessions",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/resume",
        hint: "Resume a session",
        insert: "/resume",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/inspect",
        hint: "Inspect this chat's session",
        insert: "/inspect",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/loop",
        hint: "Schedule a /loop…",
        insert: "/loop ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/skills",
        hint: "Skills",
        insert: "/skills",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/skills changes",
        hint: "What {product} changed in your skills",
        insert: "/skills changes",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/skills undo",
        hint: "Undo the newest change to a skill…",
        insert: "/skills undo ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/skills restore",
        hint: "Bring back a removed skill…",
        insert: "/skills restore ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/connections changes",
        hint: "What {product} changed in your connections",
        insert: "/connections changes",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/automations changes",
        hint: "What {product} changed in your automations",
        insert: "/automations changes",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/plugins",
        hint: "Plugins and marketplace",
        insert: "/plugins",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/mcps",
        hint: "MCP servers",
        insert: "/mcps",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/hooks",
        hint: "Hooks",
        insert: "/hooks",
        run_on_pick: true,
    },
    SlashDef {
        cmd: "/imagine-video",
        hint: "Open Imagine video",
        insert: "/imagine-video ",
        run_on_pick: false,
    },
    SlashDef {
        cmd: "/goal",
        hint: "Pin a goal…",
        insert: "/goal ",
        run_on_pick: false,
    },
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
                let want = format!(
                    "{} {}",
                    parts[0].to_ascii_lowercase(),
                    parts[1].to_ascii_lowercase()
                );
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
            hint: crate::brand::filled(s.hint).to_string(),
            insert: s.insert.to_string(),
            run_on_pick: s.run_on_pick,
        }
    }
}

/// Cabin `/learn reflect` owns `/learn`. A Grok extra with the same verb must
/// not appear as an insert-only chip that swallows the click.
fn cabin_owns_slash(cmd: &str) -> bool {
    let cmd = cmd.to_ascii_lowercase();
    SLASH_COMMANDS.iter().any(|s| {
        let c = s.cmd.to_ascii_lowercase();
        c == cmd || c.starts_with(&format!("{cmd} "))
    })
}

pub fn grok_command_hits(names: &[String]) -> Vec<SlashHit> {
    let mut out = Vec::new();
    for name in names {
        let n = name.trim().trim_start_matches('/');
        if n.is_empty() {
            continue;
        }
        let cmd = format!("/{n}");
        if cabin_owns_slash(&cmd) {
            continue;
        }
        out.push(SlashHit {
            hint: "Skill".into(),
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
        assert_eq!(parse_slash("/memory dream"), Some(Slash::MemoryDream));
        assert_eq!(parse_slash("/memory prune"), Some(Slash::MemoryPrune));
        assert_eq!(
            parse_slash("/dream"),
            Some(Slash::Dream),
            "/dream stays the Imagine prompt"
        );
        assert_eq!(parse_slash("/recall pi"), Some(Slash::Recall("pi".into())));
        assert_eq!(
            parse_slash("/forget wifi"),
            Some(Slash::Forget(Some("wifi".into())))
        );
        assert_eq!(parse_slash("/forget"), Some(Slash::Forget(None)));
        assert_eq!(parse_slash("/board"), Some(Slash::Board));
        assert_eq!(parse_slash("/journal"), Some(Slash::Journal));
        assert_eq!(
            parse_slash("/imagine a cabin"),
            Some(Slash::Imagine("a cabin".into()))
        );
        assert_eq!(parse_slash("/compact"), Some(Slash::Compact));
        assert_eq!(parse_slash("/fork"), None, "fork is removed");
        assert_eq!(parse_slash("/btw"), Some(Slash::Btw));
        assert_eq!(parse_slash("/bg"), Some(Slash::Background(String::new())));
        assert_eq!(
            parse_slash("/bg run the tests"),
            Some(Slash::Background("run the tests".into()))
        );
        assert_eq!(
            parse_slash("/background stop"),
            Some(Slash::Background("stop".into()))
        );
        assert_eq!(
            parse_slash("/queue then deploy"),
            Some(Slash::Queue("then deploy".into()))
        );
        assert_eq!(parse_slash("/queue"), None);
        assert_eq!(parse_slash("/view-plan"), Some(Slash::ViewPlan));
        assert_eq!(parse_slash("/show-plan"), Some(Slash::ViewPlan));
        assert_eq!(parse_slash("/plan-view"), Some(Slash::ViewPlan));
        assert_eq!(parse_slash("/rewind --files"), Some(Slash::RewindFiles));
        assert_eq!(parse_slash("/rewind step"), Some(Slash::RewindStep(None)));
        assert_eq!(
            parse_slash("/rewind step 3"),
            Some(Slash::RewindStep(Some(3)))
        );
        assert_eq!(
            parse_slash("/workflow review"),
            Some(Slash::Workflow("review".into()))
        );
        assert_eq!(parse_slash("/worktree"), Some(Slash::Worktree));
        let extra = grok_command_hits(&["create-skill".into(), "help".into()]);
        assert!(extra.iter().any(|h| h.cmd == "/create-skill"));
        assert!(!extra.iter().any(|h| h.cmd == "/help"));
        let hits = filter_slash_hits("/cre", &extra);
        assert!(hits.iter().any(|h| h.cmd == "/create-skill"), "{hits:?}");
        assert_eq!(parse_slash("/learn"), Some(Slash::LearnReflect));
        assert_eq!(parse_slash("/learn reflect"), Some(Slash::LearnReflect));
        assert_eq!(parse_slash("/LEARN"), Some(Slash::LearnReflect));
        assert_eq!(parse_slash("/learn now"), Some(Slash::LearnReflect));
        assert_eq!(parse_slash("/learned"), Some(Slash::Learned(String::new())));
        assert_eq!(
            parse_slash("/learned forget card:suggestion"),
            Some(Slash::Learned("forget card:suggestion".into()))
        );
        assert_eq!(slash_kind(&Slash::Learned(String::new())), "learned");
        let learn = grok_command_hits(&["learn".into(), "create-skill".into()]);
        assert!(!learn.iter().any(|h| h.cmd == "/learn"), "{learn:?}");
        assert!(learn.iter().any(|h| h.cmd == "/create-skill"), "{learn:?}");
        let learn_hits = filter_slash_hits("/learn", &[]);
        assert!(
            learn_hits
                .iter()
                .any(|h| h.cmd == "/learn" && h.run_on_pick),
            "{learn_hits:?}"
        );
        assert_eq!(parse_slash("/update"), Some(Slash::Update));
        assert_eq!(parse_slash("/brief"), Some(Slash::Brief));
        assert_eq!(slash_kind(&Slash::Brief), "brief");
    }

    #[test]
    fn cabin_slash() {
        assert_eq!(parse_slash("/help").as_ref().map(slash_kind), Some("help"));
        assert_eq!(parse_slash("/new"), Some(Slash::New));
        assert_eq!(parse_slash("/scratch"), Some(Slash::Scratch));
        assert_eq!(parse_slash("/undo"), Some(Slash::Undo));
        assert_eq!(parse_slash("/retry"), Some(Slash::Retry));
        assert_eq!(
            parse_slash("/sh ls /tmp"),
            Some(Slash::Sh("ls /tmp".into()))
        );
        assert_eq!(parse_slash("$ echo hi"), Some(Slash::Sh("echo hi".into())));
        assert_eq!(
            parse_slash("/project bind ~/GrokHub-Work"),
            Some(Slash::ProjectBind(Some("~/GrokHub-Work".into())))
        );
        assert_eq!(parse_slash("/project binding ~/GrokHub-Work"), None);
        assert!(unknown_cabin_slash("/project binding ~/GrokHub-Work"));
        assert!(unknown_cabin_slash("/approve"));
        assert!(!unknown_cabin_slash("/help"));
        assert!(!unknown_cabin_slash("hello"));
        assert!(
            !unknown_cabin_slash("/create-skill"),
            "skill slashes must reach the engine"
        );
        assert!(!unknown_cabin_slash("/workflow runs"));
        assert_eq!(
            parse_slash("/loop 30m check deploy")
                .as_ref()
                .map(slash_kind),
            Some("loop")
        );
        assert_eq!(parse_slash("/skills"), Some(Slash::GrokSkills));
        assert_eq!(parse_slash("/skills list"), Some(Slash::GrokSkills));
        assert_eq!(parse_slash("/skills changes"), Some(Slash::SkillChanges));
        assert_eq!(parse_slash("/skills undo"), Some(Slash::SkillChanges));
        assert_eq!(parse_slash("/skills restore"), Some(Slash::SkillChanges));
        assert_eq!(
            parse_slash("/connections changes"),
            Some(Slash::ConnectionChanges)
        );
        assert_eq!(parse_slash("/connections"), Some(Slash::ConnectionChanges));
        assert_eq!(
            parse_slash("/automations changes"),
            Some(Slash::AutomationChanges)
        );
        assert_eq!(
            parse_slash("/automations History"),
            Some(Slash::AutomationChanges)
        );
        assert_eq!(parse_slash("/automations weekly"), None);
        assert_eq!(slash_kind(&Slash::ConnectionChanges), "connection_changes");
        assert_eq!(
            parse_slash("/skills UNDO weekly-report"),
            Some(Slash::SkillUndo("weekly-report".into()))
        );
        assert_eq!(
            parse_slash("/skills restore board status"),
            Some(Slash::SkillRestore("board status".into()))
        );
        assert_eq!(slash_kind(&Slash::SkillUndo("x".into())), "skill_undo");
        assert_eq!(
            slash_kind(&Slash::SkillRestore("x".into())),
            "skill_restore"
        );
        assert_eq!(slash_kind(&Slash::SkillChanges), "skill_changes");
        assert_eq!(parse_slash("/mcps"), Some(Slash::GrokConnectors));
        assert_eq!(
            parse_slash("/imagine-video a cat").as_ref().map(slash_kind),
            Some("imagine_video")
        );
        assert_eq!(
            parse_slash("/goal migrate auth").as_ref().map(slash_kind),
            Some("goal")
        );
        assert_eq!(parse_slash("/resume"), Some(Slash::Sessions));
        assert_eq!(parse_slash("/dashboard"), Some(Slash::Sessions));
        assert_eq!(
            parse_slash("/project ~/GrokHub-Work"),
            Some(Slash::ProjectBind(Some("~/GrokHub-Work".into())))
        );
        assert_eq!(
            parse_slash("/project /tmp/cabin"),
            Some(Slash::ProjectBind(Some("/tmp/cabin".into())))
        );
        assert_eq!(parse_slash("/project typo"), None);
        // Byte 4 falls inside a multi-byte char: no word match, and no panic.
        assert_eq!(parse_slash("/project aé€"), None);
        assert_eq!(parse_slash("/project 日本語"), None);
        assert_eq!(parse_slash("/project bind"), None);
        assert_eq!(
            parse_slash("/project bind ."),
            Some(Slash::ProjectBind(Some(".".into())))
        );
        assert_eq!(
            parse_slash("/project new Night watch"),
            Some(Slash::ProjectNew("Night watch".into()))
        );
        assert_eq!(
            parse_slash("/project folder Cabin"),
            Some(Slash::ProjectFolder("Cabin".into()))
        );
        assert_eq!(
            parse_slash("/project rename Dawn"),
            Some(Slash::ProjectRename("Dawn".into()))
        );
        assert_eq!(
            parse_slash("/project move Cabin"),
            Some(Slash::ProjectMove("Cabin".into()))
        );
        assert_eq!(parse_slash("/project delete"), Some(Slash::ProjectDelete));
        assert_eq!(
            parse_slash("/inhabit cabin-2"),
            Some(Slash::Inhabit("cabin-2".into()))
        );
        assert_eq!(
            parse_slash("/send flash the pi"),
            Some(Slash::Send("flash the pi".into()))
        );
        assert_eq!(slash_kind(&Slash::Update), "update");
        assert_eq!(
            parse_slash("/rename night").as_ref().map(slash_kind),
            Some("rename")
        );
        assert_eq!(parse_slash("/pin"), Some(Slash::Pin));
        assert_eq!(parse_slash("/delete"), Some(Slash::Delete));
        assert_eq!(parse_slash("/close"), Some(Slash::Delete));
        assert_eq!(parse_slash("/host"), Some(Slash::HostStatus));
        assert_eq!(parse_slash("/dream"), Some(Slash::Dream));
        assert_eq!(parse_slash("/host off"), Some(Slash::HostStatus));
        assert_eq!(parse_slash("/tools off"), Some(Slash::HostStatus));
        assert_eq!(parse_slash("/tools on"), Some(Slash::HostStatus));
        assert_eq!(parse_slash("/import"), Some(Slash::Import));
        assert_eq!(
            parse_slash("/consult check the pi"),
            Some(Slash::Consult("check the pi".into()))
        );
        assert_eq!(parse_slash("/usage"), Some(Slash::Usage));
        assert_eq!(parse_slash("/palette"), Some(Slash::Palette));
        assert_eq!(parse_slash("/plan"), Some(Slash::Plan));
        assert_eq!(parse_slash("/always-approve"), Some(Slash::AlwaysApprove));
        assert_eq!(parse_slash("/sessions"), Some(Slash::Sessions));
        assert_eq!(parse_slash("/inspect"), Some(Slash::Inspect));
        assert!(filter_slash_commands("/re")
            .iter()
            .any(|s| s.cmd == "/rename"));
        assert!(filter_slash_commands("/project n")
            .iter()
            .any(|s| s.cmd == "/project new"));
        assert!(filter_slash_commands("hello").is_empty());
        let dump = "/help — this list\n/new — new chat";
        let help = mark_slash_result(dump);
        assert!(is_cabin_slash_turn("assistant", &help));
        assert_eq!(
            strip_slash_result(&help).lines().next(),
            Some("/help — this list")
        );
        assert!(is_cabin_slash_turn("assistant", dump));
        assert!(is_cabin_slash_turn(
            "assistant",
            "grok-3-mini-fast — Grok 3 Mini Fast (chat)\ngrok-4.7 — Grok 4.7 (chat)"
        ));
        assert!(!is_cabin_slash_turn("assistant", "Hello User"));
        assert!(is_cabin_slash_turn("user", "/help"));
    }

    #[test]
    fn workflow_ctl_parses_verbs_run_id_and_missing_target() {
        assert_eq!(
            parse_slash("/workflow PAUSE x"),
            Some(Slash::WorkflowCtl {
                verb: WorkflowVerb::Pause,
                target: "x".into(),
            })
        );
        assert_eq!(
            parse_slash("/workflow resume review"),
            Some(Slash::WorkflowCtl {
                verb: WorkflowVerb::Resume,
                target: "review".into(),
            })
        );
        assert_eq!(
            parse_slash("/workflow stop review-changes-2"),
            Some(Slash::WorkflowCtl {
                verb: WorkflowVerb::Stop,
                target: "review-changes-2".into(),
            })
        );
        assert_eq!(
            parse_slash("/workflow PaUsE review-changes-2"),
            Some(Slash::WorkflowCtl {
                verb: WorkflowVerb::Pause,
                target: "review-changes-2".into(),
            })
        );
        assert_eq!(parse_slash("/workflow pause"), Some(Slash::WorkflowUsage));
        assert_eq!(parse_slash("/workflow RESUME"), Some(Slash::WorkflowUsage));
        assert_eq!(parse_slash("/workflow stop"), Some(Slash::WorkflowUsage));
        assert_eq!(
            parse_slash("/workflow review"),
            Some(Slash::Workflow("review".into()))
        );
        assert_eq!(parse_slash("/workflow"), Some(Slash::GrokWorkflows));
        assert_eq!(parse_slash("/workflows"), Some(Slash::GrokWorkflows));
        assert_eq!(parse_slash("/mcps"), Some(Slash::GrokConnectors));
        assert_eq!(parse_slash("/plugins"), Some(Slash::GrokConnectors));
        assert_eq!(parse_slash("/marketplace"), Some(Slash::GrokConnectors));
        assert_eq!(parse_slash("/connectors"), Some(Slash::GrokConnectors));
        assert_eq!(parse_slash("/hooks"), Some(Slash::GrokHooks));
        assert_eq!(
            parse_slash("/workflow pause review-changes-2")
                .as_ref()
                .map(slash_kind),
            Some("workflow_ctl")
        );
        assert_eq!(slash_kind(&Slash::WorkflowUsage), "workflow_usage");
        assert_eq!(slash_kind(&Slash::GrokWorkflows), "grok_workflows");
        assert_eq!(WorkflowVerb::Pause.as_str(), "pause");
        assert_eq!(WorkflowVerb::Resume.as_str(), "resume");
        assert_eq!(WorkflowVerb::Stop.as_str(), "stop");
        assert!(
            !SLASH_COMMANDS
                .iter()
                .any(|s| s.cmd.starts_with("/workflow")),
            "the palette does not offer workflows"
        );
        assert!(filter_slash_commands("/workflow")
            .iter()
            .all(|s| !s.cmd.starts_with("/workflow")));
        // Still parsed, so a typed /workflow is answered locally instead of going to the engine.
        assert!(!unknown_cabin_slash("/workflow pause review-changes-2"));
        assert!(!unknown_cabin_slash("/workflow pause"));
    }

    #[test]
    fn hooks_slash_parses_to_grok_hooks() {
        assert_eq!(parse_slash("/hooks"), Some(Slash::GrokHooks));
        assert_eq!(parse_slash("/HOOKS"), Some(Slash::GrokHooks));
        assert_eq!(
            parse_slash("/hooks").as_ref().map(slash_kind),
            Some("grok_hooks")
        );
        assert_eq!(slash_kind(&Slash::GrokHooks), "grok_hooks");
        assert_eq!(parse_slash("/mcps"), Some(Slash::GrokConnectors));
        assert_eq!(parse_slash("/plugins"), Some(Slash::GrokConnectors));
        assert_eq!(parse_slash("/marketplace"), Some(Slash::GrokConnectors));
        assert_eq!(parse_slash("/connectors"), Some(Slash::GrokConnectors));
        assert!(!unknown_cabin_slash("/hooks"));
    }

    #[test]
    fn routing_commands_are_gone_and_answer_as_unknown() {
        let lines = [
            "/model grok-4.7",
            "/model",
            "/m grok-4.5",
            "/mode fast",
            "/mode",
            "/models",
            "/effort high",
            "/effort",
            "/why",
            "/WHY models",
            "/why table",
            "/why local",
            "/why not",
        ];
        for line in lines {
            assert_eq!(parse_slash(line), None, "{line}");
            assert!(unknown_cabin_slash(line), "{line}");
        }
        for word in ["/model", "/m", "/mode", "/models", "/effort", "/why"] {
            assert!(registry::lookup(word).is_none(), "{word}");
            assert!(
                !SLASH_COMMANDS
                    .iter()
                    .any(|d| d.cmd.split_whitespace().next() == Some(word)),
                "{word} is in the picker"
            );
            assert!(
                filter_slash_commands(word)
                    .iter()
                    .all(|d| !d.cmd.starts_with(&format!("{word} ")) && d.cmd != word),
                "{word} autocompletes"
            );
        }
        // A skill or another builtin still reaches the engine.
        assert!(!unknown_cabin_slash("/memo"));
        assert!(!unknown_cabin_slash("/create-skill"));
    }

    #[test]
    fn diagnose_slash_is_a_cabin_view() {
        assert_eq!(parse_slash("/diagnose"), Some(Slash::Diagnose));
        assert_eq!(parse_slash("/Diagnose"), Some(Slash::Diagnose));
        assert_eq!(
            parse_slash("/diagnose").as_ref().map(slash_kind),
            Some("diagnose")
        );
        assert!(SLASH_COMMANDS
            .iter()
            .any(|d| d.cmd == "/diagnose" && d.run_on_pick));
        assert!(!unknown_cabin_slash("/diagnose"));
    }

    #[test]
    fn record_slash_takes_a_note_and_stop() {
        assert_eq!(parse_slash("/record"), Some(Slash::Record(String::new())));
        assert_eq!(
            parse_slash("/record video stutters on 4K YouTube"),
            Some(Slash::Record("video stutters on 4K YouTube".into()))
        );
        assert_eq!(parse_slash("/record stop"), Some(Slash::RecordStop));
        assert_eq!(parse_slash("/record Stop"), Some(Slash::RecordStop));
        assert_eq!(
            parse_slash("/record stop").as_ref().map(slash_kind),
            Some("record_stop")
        );
        assert!(SLASH_COMMANDS
            .iter()
            .any(|d| d.cmd == "/record" && d.insert == "/record "));
        assert!(!unknown_cabin_slash("/record"));
    }

    #[test]
    fn audiocheck_slash_takes_an_optional_input() {
        assert_eq!(
            parse_slash("/audiocheck"),
            Some(Slash::AudioCheck(String::new()))
        );
        assert_eq!(
            parse_slash("/audiocheck Blue Yeti"),
            Some(Slash::AudioCheck("Blue Yeti".into()))
        );
        assert_eq!(
            parse_slash("/audiocheck").as_ref().map(slash_kind),
            Some("audiocheck")
        );
        assert!(SLASH_COMMANDS
            .iter()
            .any(|d| d.cmd == "/audiocheck" && d.run_on_pick));
        assert!(!unknown_cabin_slash("/audiocheck"));
    }

    #[test]
    fn privacy_slash_is_a_cabin_view() {
        assert_eq!(parse_slash("/privacy"), Some(Slash::Privacy));
        assert_eq!(parse_slash("/PRIVACY"), Some(Slash::Privacy));
        assert_eq!(
            parse_slash("/privacy").as_ref().map(slash_kind),
            Some("privacy")
        );
        assert!(!unknown_cabin_slash("/privacy"));
    }
}
