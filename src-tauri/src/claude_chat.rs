//! Chats on the user's own Claude plan (`grokhub_agent::claude_plan`):
//! whether Settings allowed it, and which chats were switched to it, in
//! `{config}/claude-plan-chats.json`. No credential is ever kept here;
//! Claude Code signs itself in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use grokhub_agent::claude_plan::stream::Headroom;
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
    /// Each chat's Claude Code session and the folder it lives under, so
    /// the chat goes on with it after the app restarts.
    #[serde(default)]
    pub sessions: BTreeMap<String, SavedSession>,
    /// The plan's newest limit reading from a chat turn, for Settings.
    #[serde(default)]
    pub headroom: Option<SeenHeadroom>,
}

/// A limit reading and when it came (Unix seconds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeenHeadroom {
    #[serde(flatten)]
    pub reading: Headroom,
    pub seen_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSession {
    pub id: String,
    pub cwd: PathBuf,
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
            chats.sessions.clear();
            chats.headroom = None;
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
        } else {
            // Turns on the app's own models come next, which that Claude Code
            // session never saw: switching back on starts a new one.
            chats.sessions.remove(id);
        }
        Ok(())
    })
}

/// The Claude Code session this chat goes on with in `cwd`, if it has one.
pub fn session_of(dir: &Path, id: &str, cwd: &Path) -> Option<String> {
    read(dir)
        .sessions
        .get(id)
        .filter(|saved| saved.cwd == cwd)
        .map(|saved| saved.id.clone())
}

/// Keeps the session a turn started or went on with. A chat no longer on
/// the plan keeps none.
pub fn remember_session(dir: &Path, id: &str, session: &str, cwd: &Path) -> Result<(), String> {
    if read(dir)
        .sessions
        .get(id)
        .is_some_and(|s| s.id == session && s.cwd == cwd)
    {
        return Ok(());
    }
    change(dir, |chats| {
        if chats.chats.iter().any(|c| c == id) {
            chats.sessions.insert(
                id.to_string(),
                SavedSession {
                    id: session.to_string(),
                    cwd: cwd.to_path_buf(),
                },
            );
        }
        Ok(())
    })
    .map(|_| ())
}

/// Keeps a turn's limit reading for Settings, while chats are allowed.
/// `seen_at` is when Claude Code sent it: chats run side by side, so one
/// that ends later with an older reading leaves the newer one in place.
pub fn remember_headroom(dir: &Path, reading: Headroom, seen_at: u64) -> Result<(), String> {
    change(dir, |chats| {
        let newer = chats.headroom.as_ref().is_none_or(|h| h.seen_at <= seen_at);
        if chats.allowed && newer {
            chats.headroom = Some(SeenHeadroom { reading, seen_at });
        }
        Ok(())
    })
    .map(|_| ())
}

/// A message was deleted from this chat: Claude Code's session still holds
/// it, so the next turn starts a new session rather than bring it back.
pub fn forget_session(dir: &Path, id: &str) -> Result<(), String> {
    if !read(dir).sessions.contains_key(id) {
        return Ok(());
    }
    change(dir, |chats| {
        chats.sessions.remove(id);
        Ok(())
    })
    .map(|_| ())
}

/// These chats were deleted: they're no longer switched or kept.
pub fn forget_chats(dir: &Path, ids: &[String]) {
    let held = read(dir);
    if !ids
        .iter()
        .any(|id| held.chats.contains(id) || held.sessions.contains_key(id))
    {
        return;
    }
    let _ = change(dir, |chats| {
        chats.chats.retain(|c| !ids.contains(c));
        chats.sessions.retain(|c, _| !ids.contains(c));
        Ok(())
    });
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
    if !is_on(dir, id) || grokhub_agent::route::user_bench::load(dir).blocks_claude_plan() {
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
                sessions: BTreeMap::new(),
                headroom: None,
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

        // Benching Claude skips Claude Plan CLI execution.
        let bench_cmd =
            grokhub_agent::route::user_bench::parse_chat_command("stop using Claude").unwrap();
        grokhub_agent::route::user_bench::apply_and_save(&dir, bench_cmd, 1).unwrap();
        assert_eq!(turn_cli(&dir, "chat-b", &dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_chat_goes_on_with_its_session_in_the_same_folder_only() {
        let dir = scratch("sessions");
        let (app, other) = (
            Path::new("/home/user/my-app"),
            Path::new("/home/user/other"),
        );
        set_allowed(&dir, true).unwrap();
        // Not on the plan: nothing kept.
        remember_session(&dir, "chat-a", "sid-1", app).unwrap();
        assert_eq!(session_of(&dir, "chat-a", app), None);

        set_chat(&dir, "chat-a", true).unwrap();
        remember_session(&dir, "chat-a", "sid-1", app).unwrap();
        assert_eq!(session_of(&dir, "chat-a", app), Some("sid-1".into()));
        assert_eq!(session_of(&dir, "chat-a", other), None);
        assert_eq!(session_of(&dir, "chat-b", app), None);

        // Switched off and on again: a new session.
        set_chat(&dir, "chat-a", false).unwrap();
        set_chat(&dir, "chat-a", true).unwrap();
        assert_eq!(session_of(&dir, "chat-a", app), None);

        remember_session(&dir, "chat-a", "sid-2", app).unwrap();
        set_allowed(&dir, false).unwrap();
        assert_eq!(read(&dir).sessions, BTreeMap::new());
    }

    #[test]
    fn a_deleted_message_or_chat_lets_go_of_its_session() {
        let dir = scratch("forget");
        let app = Path::new("/home/user/my-app");
        set_allowed(&dir, true).unwrap();
        for id in ["chat-a", "chat-b", "chat-c"] {
            set_chat(&dir, id, true).unwrap();
            remember_session(&dir, id, &format!("sid-{id}"), app).unwrap();
        }
        forget_session(&dir, "chat-a").unwrap();
        assert_eq!(session_of(&dir, "chat-a", app), None);
        assert!(is_on(&dir, "chat-a"), "the chat stays on the plan");

        forget_chats(&dir, &["chat-b".to_string()]);
        assert!(!is_on(&dir, "chat-b"));
        assert_eq!(session_of(&dir, "chat-b", app), None);
        // A deleted chat's late turn doesn't bring it back.
        remember_session(&dir, "chat-b", "sid-late", app).unwrap();
        assert_eq!(session_of(&dir, "chat-b", app), None);
        assert_eq!(session_of(&dir, "chat-c", app), Some("sid-chat-c".into()));
    }

    #[test]
    fn a_store_from_before_sessions_still_reads() {
        let dir = scratch("old-store");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), r#"{"allowed":true,"chats":["chat-a"]}"#).unwrap();
        assert!(is_on(&dir, "chat-a"));
    }

    fn week(used: u8) -> Headroom {
        Headroom {
            window: "seven_day".into(),
            used_pct: Some(used),
            resets_at: Some(1_795_341_600),
            limited: false,
        }
    }

    #[test]
    fn the_newest_limit_reading_is_kept_only_while_allowed() {
        let dir = scratch("headroom");
        remember_headroom(&dir, week(10), 1_000).unwrap();
        assert_eq!(read(&dir).headroom, None, "not allowed yet");

        set_allowed(&dir, true).unwrap();
        remember_headroom(&dir, week(43), 2_000).unwrap();
        remember_headroom(&dir, week(44), 3_000).unwrap();
        // A chat that ends later with a reading from before.
        remember_headroom(&dir, week(40), 2_500).unwrap();
        assert_eq!(
            read(&dir).headroom,
            Some(SeenHeadroom {
                reading: week(44),
                seen_at: 3_000
            })
        );
        let saved = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(
            saved.contains(r#""window": "seven_day""#) && saved.contains(r#""usedPct": 44"#),
            "{saved}"
        );
    }

    #[test]
    fn revoking_moves_every_chat_back() {
        let dir = scratch("revoke");
        set_allowed(&dir, true).unwrap();
        set_chat(&dir, "chat-a", true).unwrap();
        remember_headroom(&dir, week(43), 1_000).unwrap();
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
