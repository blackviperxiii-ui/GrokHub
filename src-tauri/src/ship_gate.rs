//! The checks on a session's change before it can be committed, as the Tasks
//! panel reads them. Read only: nothing here commits, stages or pushes. The
//! checks themselves are `grokhub_code::commit`'s; this adds what only the
//! shell knows, which is whether a turn of the session runs now.

use std::path::Path;

use crate::code_commit::{self, ShipStatus};
use crate::code_secrets::Keychain;
use crate::code_ship::CodeShip;

/// The agent may still be changing the files the checks read.
pub const IN_TURN: &str = "Wait for the agent to finish its turn.";

/// The checks on the change of `conversation_id` in `folder`, with the
/// project's secret values read from the keychain.
pub fn status(
    config_dir: &Path,
    folder: &Path,
    conversation_id: &str,
    cfg: &CodeShip,
) -> ShipStatus {
    let mut status = code_commit::status_with(config_dir, folder, conversation_id, cfg, &Keychain);
    if crate::helper_asks::in_turn(conversation_id) {
        status.blocked.insert(0, IN_TURN.into());
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_turn_blocks_first_and_its_end_clears_the_block() {
        let base = std::env::temp_dir().join(format!(
            "gh-ship-gate-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        // A folder with no repository: the read stops before the keychain.
        let folder = base.join("plain");
        std::fs::create_dir_all(&folder).unwrap();
        let chat = format!("ship-gate-{}", grokhub_core::now_ms());
        let read = || status(&base.join("cfg"), &folder, &chat, &CodeShip::default());
        assert_eq!(
            read(),
            ShipStatus {
                blocked: vec!["This folder is not a git repository.".into()],
                checks: None,
            }
        );
        crate::helper_asks::turn_started(&chat);
        assert_eq!(
            read().blocked,
            [
                "Wait for the agent to finish its turn.",
                "This folder is not a git repository.",
            ]
        );
        crate::helper_asks::turn_ended(&chat);
        assert_eq!(read().blocked, ["This folder is not a git repository."]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
