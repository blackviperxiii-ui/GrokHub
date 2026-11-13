//! The checks on a session's change before it can be committed, as the Tasks
//! panel reads them, and the clicks that act on them: Commit and Waive, then
//! Push and Open PR for the commit made there. The checks and the steps
//! themselves are `grokhub_code::commit`'s and `grokhub_code::push`'s; this
//! adds what only the shell knows, which is whether a turn of the session
//! runs now. While one runs, nothing is committed, waived, pushed or opened.

use std::path::Path;

use crate::code_commit::{self, Committed, Draft, ShipStatus};
use crate::code_push;
use crate::code_secrets::Keychain;
use crate::code_ship::{self, Action, CodeShip, Plan};

/// The agent may still be changing the files the checks read.
pub const IN_TURN: &str = "Wait for the agent to finish its turn.";

/// The checks on the change of `conversation_id` in `folder`, with the
/// project's secret values read from the keychain.
pub fn status(
    config_dir: &Path,
    folder: &Path,
    conversation_id: &str,
    cfg: &CodeShip,
    draft: &Draft,
) -> ShipStatus {
    let mut status =
        code_commit::status_for(config_dir, folder, conversation_id, cfg, &Keychain, draft);
    if crate::helper_asks::in_turn(conversation_id) {
        status.blocked.insert(0, IN_TURN.into());
        if let Some(plan) = status.commit.as_mut() {
            plan.confirm = None;
        }
    }
    status
}

/// Commit: the session's change becomes one commit, after the person's yes
/// to the exact text the confirm showed. Every check runs again first.
pub fn commit(
    config_dir: &Path,
    folder: &Path,
    conversation_id: &str,
    cfg: &CodeShip,
    draft: &Draft,
    confirmed: bool,
    command: Option<&str>,
) -> Result<Committed, String> {
    if crate::helper_asks::in_turn(conversation_id) {
        return Err(IN_TURN.into());
    }
    let done = code_commit::commit_with(
        config_dir,
        folder,
        conversation_id,
        cfg,
        &Keychain,
        draft,
        confirmed,
        command,
    );
    crate::task_feed::touch(conversation_id);
    done
}

/// Waive: the person ships with the blocker at `index` of the review of the
/// tree `tree`, written at `at_ms`, open.
pub fn waive(
    config_dir: &Path,
    folder: &Path,
    conversation_id: &str,
    cfg: &CodeShip,
    tree: &str,
    at_ms: u64,
    index: u32,
) -> Result<(), String> {
    if crate::helper_asks::in_turn(conversation_id) {
        return Err(IN_TURN.into());
    }
    let done = code_commit::waive(config_dir, folder, conversation_id, cfg, tree, at_ms, index);
    crate::task_feed::touch(conversation_id);
    done
}

/// What the Tasks panel's push of the session's commit, or its PR, would
/// run. Read only.
pub fn push_plan(
    config_dir: &Path,
    folder: &Path,
    conversation_id: &str,
    cfg: &CodeShip,
    action: Action,
) -> Plan {
    let mut plan = code_push::plan(config_dir, folder, conversation_id, cfg, action);
    if crate::helper_asks::in_turn(conversation_id) {
        plan.blocked = Some(IN_TURN.into());
        plan.needs_confirm = true;
    }
    plan
}

/// Push or Open PR: after the person's yes to the exact command the confirm
/// showed. Every check runs again first.
pub fn push(
    config_dir: &Path,
    folder: &Path,
    conversation_id: &str,
    cfg: &CodeShip,
    action: Action,
    confirmed: bool,
    command: Option<&str>,
) -> Result<code_ship::Shipped, String> {
    if crate::helper_asks::in_turn(conversation_id) {
        return Err(IN_TURN.into());
    }
    let done = code_push::push_with(
        config_dir,
        folder,
        conversation_id,
        cfg,
        action,
        confirmed,
        command,
    );
    crate::task_feed::touch(conversation_id);
    done
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
        let read = || {
            status(
                &base.join("cfg"),
                &folder,
                &chat,
                &CodeShip::default(),
                &Draft::default(),
            )
        };
        assert_eq!(
            read(),
            ShipStatus {
                blocked: vec!["This folder is not a git repository.".into()],
                checks: None,
                commit: None,
                shipped: None,
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

    #[test]
    fn commit_and_waive_wait_for_the_turn_and_then_run_every_check() {
        let base = std::env::temp_dir().join(format!(
            "gh-ship-gate-act-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let folder = base.join("plain");
        std::fs::create_dir_all(&folder).unwrap();
        let cfg = base.join("cfg");
        let chat = format!("ship-gate-act-{}", grokhub_core::now_ms());
        let commit_it = || {
            commit(
                &cfg,
                &folder,
                &chat,
                &CodeShip::default(),
                &Draft::default(),
                true,
                Some("commit"),
            )
        };
        let waive_it = || waive(&cfg, &folder, &chat, &CodeShip::default(), "abc", 1, 0);
        crate::helper_asks::turn_started(&chat);
        assert_eq!(
            commit_it(),
            Err("Wait for the agent to finish its turn.".to_string())
        );
        assert_eq!(
            waive_it(),
            Err("Wait for the agent to finish its turn.".to_string())
        );
        crate::helper_asks::turn_ended(&chat);
        assert_eq!(
            commit_it(),
            Err("This folder is not a git repository.".to_string())
        );
        assert_eq!(
            waive_it(),
            Err("There is no review to waive a finding of.".to_string())
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn push_and_pr_wait_for_the_turn_and_then_run_every_check() {
        let base = std::env::temp_dir().join(format!(
            "gh-ship-gate-push-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let folder = base.join("plain");
        std::fs::create_dir_all(&folder).unwrap();
        let cfg = base.join("cfg");
        let chat = format!("ship-gate-push-{}", grokhub_core::now_ms());
        let none = "Commit the change here first: only the commit made here is pushed.";
        crate::helper_asks::turn_started(&chat);
        for action in [Action::Push, Action::Pr] {
            let plan = push_plan(&cfg, &folder, &chat, &CodeShip::default(), action);
            assert_eq!(plan.blocked.as_deref(), Some(IN_TURN));
            assert_eq!(plan.command, "");
            assert_eq!(
                push(
                    &cfg,
                    &folder,
                    &chat,
                    &CodeShip::default(),
                    action,
                    true,
                    Some("git push")
                ),
                Err("Wait for the agent to finish its turn.".to_string())
            );
        }
        crate::helper_asks::turn_ended(&chat);
        for action in [Action::Push, Action::Pr] {
            let plan = push_plan(&cfg, &folder, &chat, &CodeShip::default(), action);
            assert_eq!(plan.blocked.as_deref(), Some(none));
            assert_eq!(plan.mode, code_ship::ShipMode::Ask);
            assert_eq!(
                push(
                    &cfg,
                    &folder,
                    &chat,
                    &CodeShip::default(),
                    action,
                    true,
                    Some("git push")
                ),
                Err(none.to_string())
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
