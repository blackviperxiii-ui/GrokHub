//! What each slash command does when the cabin runs it. The cabin's
//! gate-coverage test reads this to check every `Action` keeps its rail.

use super::Slash;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashEffect {
    /// Shows or changes cabin state only.
    Local,
    /// Hands text to a model or agent turn; its tools run under the agent's gate.
    AgentTurn,
    /// Runs something on this computer or a paired one, or changes what may run
    /// without asking. The cabin must route it through a confirm, consent, or
    /// permission path.
    Action,
}

/// Every variant is listed, so a new command can't compile without a choice.
pub fn slash_effect(s: &Slash) -> SlashEffect {
    use SlashEffect::{Action, AgentTurn, Local};
    match s {
        Slash::Sh(_)
        | Slash::AlwaysApprove
        | Slash::AutoPerm
        | Slash::Record(_)
        | Slash::Inhabit(_)
        | Slash::Sync
        | Slash::Update => Action,
        Slash::Retry
        | Slash::Skill(_)
        | Slash::Queue(_)
        | Slash::Send(_)
        | Slash::Background(_)
        | Slash::Consult(_)
        | Slash::Compact
        | Slash::Imagine(_)
        | Slash::ImagineVideo(_)
        | Slash::Loop(_) => AgentTurn,
        // `/audiocheck` records a short clip that stays in memory, and
        // `/project bind` moves the folder host commands are jailed to; neither
        // has a confirm step today.
        Slash::AudioCheck(_) | Slash::ProjectBind(_) => Local,
        Slash::Forget(_)
        | Slash::MemoryNote(_)
        | Slash::MemoryShow
        | Slash::MemoryDream
        | Slash::MemoryPrune
        | Slash::Recall(_)
        | Slash::Board
        | Slash::Journal
        | Slash::LearnReflect
        | Slash::Help
        | Slash::New
        | Slash::Scratch
        | Slash::Clear
        | Slash::Undo
        | Slash::Stop
        | Slash::ProjectClear
        | Slash::ProjectShow
        | Slash::ProjectNew(_)
        | Slash::ProjectFolder(_)
        | Slash::ProjectRename(_)
        | Slash::ProjectMove(_)
        | Slash::ProjectDelete
        | Slash::Privacy
        | Slash::Diagnose
        | Slash::RecordStop
        | Slash::Why
        | Slash::WhyModels
        | Slash::WhyTable
        | Slash::Hub
        | Slash::Rewind
        | Slash::RewindStep(_)
        | Slash::Room(_)
        | Slash::Export
        | Slash::ExportAs(_)
        | Slash::Rename(_)
        | Slash::Context
        | Slash::Health
        | Slash::Brief
        | Slash::Fix
        | Slash::Remember(_)
        | Slash::Mode(_)
        | Slash::Dream
        | Slash::HostStatus
        | Slash::Import
        | Slash::Usage
        | Slash::Models
        | Slash::Palette
        | Slash::Pin
        | Slash::Delete
        | Slash::Plan
        | Slash::Effort
        | Slash::Sessions
        | Slash::Inspect
        | Slash::GrokSkills
        | Slash::SkillChanges
        | Slash::SkillUndo(_)
        | Slash::SkillRestore(_)
        | Slash::ConnectionChanges
        | Slash::AutomationChanges
        | Slash::GrokConnectors
        | Slash::GrokHooks
        | Slash::GrokWorkflows
        | Slash::Model(_)
        | Slash::Goal(_)
        | Slash::Workflow(_)
        | Slash::WorkflowCtl { .. }
        | Slash::WorkflowUsage
        | Slash::RewindFiles
        | Slash::Worktree
        | Slash::Btw
        | Slash::ViewPlan => Local,
    }
}

/// One line per `Slash` variant. Tests check this covers every variant, and the
/// cabin's gate-coverage test runs each `Action` through `run_slash`'s routes.
pub const SLASH_SAMPLES: &[&str] = &[
    "/forget wifi",
    "/memory note prefer nvim",
    "/memory",
    "/memory dream",
    "/memory prune",
    "/recall pi",
    "/board",
    "/journal",
    "/imagine a lighthouse",
    "/compact",
    "/skill tidy",
    "/learn",
    "/update",
    "/help",
    "/new",
    "/scratch",
    "/clear",
    "/undo",
    "/retry",
    "/stop",
    "/sh ls",
    "/project bind ~/code",
    "/project clear",
    "/project show",
    "/project new Pier",
    "/project folder Work",
    "/project rename Pier 2",
    "/project move Work",
    "/project delete",
    "/send check the logs",
    "/sync",
    "/privacy",
    "/diagnose",
    "/record the menu flickers",
    "/record stop",
    "/audiocheck",
    "/why",
    "/why models",
    "/why table",
    "/hub",
    "/inhabit desk",
    "/rewind",
    "/rewind step 2",
    "/room kitchen",
    "/export",
    "/export md",
    "/rename Pier light",
    "/context",
    "/health",
    "/brief",
    "/fix",
    "/remember tabs over spaces",
    "/mode fast",
    "/dream",
    "/host",
    "/import",
    "/consult is this safe",
    "/usage",
    "/models",
    "/palette",
    "/pin",
    "/delete",
    "/plan",
    "/always-approve",
    "/auto",
    "/effort",
    "/sessions",
    "/inspect",
    "/loop check mail",
    "/skills",
    "/skills changes",
    "/skills undo tidy",
    "/skills restore tidy",
    "/connections changes",
    "/automations changes",
    "/connectors",
    "/hooks",
    "/workflows",
    "/model grok-4.7",
    "/imagine-video waves",
    "/goal ship it",
    "/workflow nightly",
    "/workflow pause nightly",
    "/workflow pause",
    "/rewind files",
    "/worktree",
    "/btw",
    "/view-plan",
    "/bg tidy the docs",
    "/queue and then this",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slash::parse_slash;

    fn variant_name(s: &Slash) -> String {
        let dbg = format!("{s:?}");
        dbg.split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn samples_cover_every_slash_variant() {
        // Windows checkouts embed CRLF via include_str.
        let src = include_str!("mod.rs").replace("\r\n", "\n");
        let body = src
            .split("pub enum Slash {")
            .nth(1)
            .and_then(|s| s.split("\n}\n").next())
            .expect("enum Slash");
        let mut variants: Vec<String> = body
            .lines()
            .filter(|l| l.starts_with("    ") && !l.starts_with("     "))
            .map(|l| l.trim())
            .filter(|l| l.starts_with(|c: char| c.is_ascii_uppercase()))
            .map(|l| {
                l.split(|c: char| !c.is_ascii_alphanumeric())
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        variants.sort();
        let mut sampled: Vec<String> = SLASH_SAMPLES
            .iter()
            .map(|line| {
                let s = parse_slash(line).unwrap_or_else(|| panic!("{line} parses"));
                variant_name(&s)
            })
            .collect();
        sampled.sort();
        assert_eq!(sampled, variants);
    }

    #[test]
    fn actions_are_the_commands_that_run_or_unlock_something() {
        let actions: Vec<&str> = SLASH_SAMPLES
            .iter()
            .copied()
            .filter(|line| {
                parse_slash(line).is_some_and(|s| slash_effect(&s) == SlashEffect::Action)
            })
            .collect();
        assert_eq!(
            actions,
            [
                "/update",
                "/sh ls",
                "/sync",
                "/record the menu flickers",
                "/inhabit desk",
                "/always-approve",
                "/auto",
            ]
        );
        assert_eq!(
            slash_effect(&parse_slash("$ rm -rf build").expect("shell")),
            SlashEffect::Action
        );
        assert_eq!(
            slash_effect(&parse_slash("/bg tidy").expect("bg")),
            SlashEffect::AgentTurn
        );
        assert_eq!(slash_effect(&Slash::Help), SlashEffect::Local);
    }
}
