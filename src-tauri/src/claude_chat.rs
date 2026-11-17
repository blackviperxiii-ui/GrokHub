//! Chats on the user's own Claude plan (`grokhub_agent::claude_plan`):
//! whether Settings allowed it, and which chats were switched to it, in
//! `{config}/claude-plan-chats.json`. No credential is ever kept here;
//! Claude Code signs itself in.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use grokhub_agent::claude_plan::{self, ClaudePlan};
use serde::{Deserialize, Serialize};

const FILE: &str = "claude-plan-chats.json";

pub const NOT_ALLOWED: &str =
    "Allow chats on your Claude plan in Settings → AI models → Claude (your plan) first.";
pub const NO_CLI: &str = "This chat runs on your Claude plan, but Claude Code didn't answer on this computer, so nothing was sent. Settings → AI models → Claude (your plan) shows what's wrong.";
pub const SIGNED_OUT: &str = "This chat runs on your Claude plan, but Claude Code isn't signed in, so nothing was sent. In a terminal, run claude auth login, then send again.";
pub const NO_SIDE: &str =
    "Side answers don't run on Claude (your plan) yet. Ask it in the chat instead.";
pub const NOT_PLAN: &str = "This chat runs on your Claude plan, but Claude Code is signed in another way now, so nothing was sent. Sign in with your plan (claude auth login), or switch this chat back in its tab menu.";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeChats {
    /// Settings' Allow for chats on the plan.
    pub allowed: bool,
    /// The chats switched to it, by conversation id.
    pub chats: Vec<String>,
}

/// One writer at a time, so two quick switches don't drop one.
static LOCK: Mutex<()> = Mutex::new(());

pub fn read(dir: &Path) -> ClaudeChats {
    std::fs::read(dir.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn change(
    dir: &Path,
    edit: impl FnOnce(&mut ClaudeChats) -> Result<(), String>,
) -> Result<ClaudeChats, String> {
    let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut chats = read(dir);
    edit(&mut chats)?;
    let body = serde_json::to_vec_pretty(&chats).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save: {e}"))?;
    let tmp = dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, body).map_err(|e| format!("Couldn't save: {e}"))?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| format!("Couldn't save: {e}"))?;
    Ok(chats)
}

/// Settings' Allow, or its revoke. Revoking moves every chat back to the
/// app's own models, so allowing again later switches none on by itself.
pub fn set_allowed(dir: &Path, on: bool) -> Result<ClaudeChats, String> {
    change(dir, |chats| {
        chats.allowed = on;
        if !on {
            chats.chats.clear();
        }
        Ok(())
    })
}

/// A chat's tab menu switch.
pub fn set_chat(dir: &Path, id: &str, on: bool) -> Result<ClaudeChats, String> {
    change(dir, |chats| {
        if on && !chats.allowed {
            return Err(NOT_ALLOWED.into());
        }
        chats.chats.retain(|c| c != id);
        if on {
            chats.chats.push(id.to_string());
        }
        Ok(())
    })
}

/// This chat was switched to the plan, and Settings still allows it.
pub fn is_on(dir: &Path, id: &str) -> bool {
    let chats = read(dir);
    chats.allowed && chats.chats.iter().any(|c| c == id)
}

/// How this chat's next turn runs: `None` on the app's own models, the CLI
/// when it's switched to the plan and Claude Code is signed in with one,
/// and why not otherwise. Checked every turn from the chat's folder, so a
/// sign-out or a switch to an API key (the user's or the project's Claude
/// Code settings) sends nothing rather than billing somewhere else.
pub fn turn_cli(dir: &Path, id: &str, workspace: &Path) -> Option<Result<PathBuf, String>> {
    if !is_on(dir, id) {
        return None;
    }
    Some(plan_cli(claude_plan::find(), |cli| {
        claude_plan::detect_in(cli, workspace)
    }))
}

fn plan_cli(cli: Option<PathBuf>, detect: impl Fn(&Path) -> ClaudePlan) -> Result<PathBuf, String> {
    let cli = cli.ok_or(NO_CLI)?;
    match detect(&cli) {
        ClaudePlan::Plan { .. } => Ok(cli),
        ClaudePlan::SignedOut { .. } => Err(SIGNED_OUT.into()),
        ClaudePlan::OtherBilling { .. } => Err(NOT_PLAN.into()),
        ClaudePlan::Missing | ClaudePlan::NoAnswer => Err(NO_CLI.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gh-claude-chats-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_chat_switches_on_only_once_settings_allowed_it() {
        let dir = scratch("allow");
        assert_eq!(read(&dir), ClaudeChats::default());
        assert_eq!(set_chat(&dir, "chat-a", true), Err(NOT_ALLOWED.to_string()));
        assert_eq!(turn_cli(&dir, "chat-a", &dir), None);

        set_allowed(&dir, true).unwrap();
        set_chat(&dir, "chat-a", true).unwrap();
        set_chat(&dir, "chat-a", true).unwrap();
        let both = set_chat(&dir, "chat-b", true).unwrap();
        assert_eq!(
            both,
            ClaudeChats {
                allowed: true,
                chats: vec!["chat-a".into(), "chat-b".into()],
            }
        );
        assert_eq!(read(&dir), both);
        assert_eq!(
            set_chat(&dir, "chat-a", false).unwrap().chats,
            vec!["chat-b".to_string()]
        );
        assert!(is_on(&dir, "chat-b"));
        // Another chat never runs on the plan.
        assert_eq!(turn_cli(&dir, "chat-a", &dir), None);
    }

    #[test]
    fn revoking_moves_every_chat_back() {
        let dir = scratch("revoke");
        set_allowed(&dir, true).unwrap();
        set_chat(&dir, "chat-a", true).unwrap();
        assert_eq!(set_allowed(&dir, false).unwrap(), ClaudeChats::default());
        assert!(!is_on(&dir, "chat-a"));
        assert_eq!(set_allowed(&dir, true).unwrap().chats, Vec::<String>::new());
        assert_eq!(turn_cli(&dir, "chat-a", &dir), None);
    }

    #[test]
    fn a_turn_runs_only_while_claude_code_is_signed_in_with_a_plan() {
        let cli = || Some(PathBuf::from("/home/user/.local/bin/claude"));
        let v = || "2.1.325".to_string();
        assert_eq!(
            plan_cli(cli(), |_| ClaudePlan::Plan { version: v() }),
            Ok(PathBuf::from("/home/user/.local/bin/claude"))
        );
        assert_eq!(
            plan_cli(cli(), |_| ClaudePlan::SignedOut { version: v() }),
            Err(SIGNED_OUT.to_string())
        );
        assert_eq!(
            plan_cli(cli(), |_| ClaudePlan::OtherBilling {
                version: v(),
                method: "api_key".into()
            }),
            Err(NOT_PLAN.to_string())
        );
        assert_eq!(
            plan_cli(cli(), |_| ClaudePlan::NoAnswer),
            Err(NO_CLI.to_string())
        );
        assert_eq!(
            plan_cli(None, |_| ClaudePlan::Plan { version: v() }),
            Err(NO_CLI.to_string())
        );
    }
}
