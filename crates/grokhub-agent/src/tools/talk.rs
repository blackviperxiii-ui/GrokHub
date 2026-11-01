//! Talk tool profile (LO-45): send only the tools the chat's intent needs.
//!
//! A chat whose first Brief is plain talk (chat, creative, or a general,
//! game-help or planning question, sure to at least [`TALK_SURE`]) starts on
//! the Talk profile: the workspace and shell tools in [`TALK_DEMOTED`] leave
//! the eager list and sit in the deferred index, so the model can still load
//! them. Every other chat is Full, exactly as before.
//!
//! The first Brief decides; later talk Briefs change nothing. A Talk chat
//! upgrades once, all at once: a later work Brief, or one `load_tool_schema`
//! of any demoted tool, appends the whole [`TALK_DEMOTED`] list to the end of
//! the tool list in this fixed order. That is one cache miss per chat at most;
//! after it the list is byte-identical turn to turn. Dispatch and the
//! permission gate never read the profile.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use crate::brain::{Brief, Intent};

/// Tools a Talk chat does not send eagerly, in the order an upgrade appends them.
pub const TALK_DEMOTED: [&str; 9] = [
    "read_file",
    "list_dir",
    "grep",
    "glob",
    "get_command_or_subagent_output",
    "write",
    "search_replace",
    "run_terminal_command",
    "diagnose",
];

/// Least Brief confidence (0-100) that may start a chat on Talk.
pub const TALK_SURE: u8 = 80;

/// Question domains that need no workspace or shell tools.
const TALK_QUESTION_DOMAINS: [&str; 3] = ["general", "game-help", "planning"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Today's tool list.
    Full,
    /// Demoted tools are in the index only.
    Talk,
    /// A Talk chat that has appended [`TALK_DEMOTED`] to its tail.
    Upgraded,
}

fn profiles() -> &'static Mutex<BTreeMap<String, Profile>> {
    static MAP: OnceLock<Mutex<BTreeMap<String, Profile>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Whether this Brief is plain talk, sure enough to send the Talk profile.
pub fn wants_talk(brief: &Brief) -> bool {
    let talk = match brief.intent {
        Intent::Chat | Intent::Creative => true,
        Intent::Question => TALK_QUESTION_DOMAINS.contains(&brief.domain.as_str()),
        _ => false,
    };
    talk && brief.confidence >= TALK_SURE
}

pub fn is_demoted(name: &str) -> bool {
    TALK_DEMOTED.contains(&name)
}

/// The chat's profile; a chat with no Brief yet is Full.
pub fn profile(session: &str) -> Profile {
    profiles()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
        .copied()
        .unwrap_or(Profile::Full)
}

/// Record a turn's Brief. The first one sets the profile. Returns true when a
/// later work Brief should upgrade a Talk chat.
pub fn note_brief(session: &str, brief: &Brief) -> bool {
    if session.is_empty() {
        return false;
    }
    let mut map = profiles().lock().unwrap_or_else(|e| e.into_inner());
    match map.get(session) {
        None => {
            let first = if wants_talk(brief) {
                Profile::Talk
            } else {
                Profile::Full
            };
            map.insert(session.to_string(), first);
            false
        }
        Some(Profile::Talk) => !wants_talk(brief),
        Some(_) => false,
    }
}

/// Talk → Upgraded. Returns true only on the one call that changed it.
pub fn mark_upgraded(session: &str) -> bool {
    let mut map = profiles().lock().unwrap_or_else(|e| e.into_inner());
    match map.get_mut(session) {
        Some(p @ Profile::Talk) => {
            *p = Profile::Upgraded;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "talk_tests.rs"]
mod tests;
