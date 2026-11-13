//! The push and the pull request from the agent window's Tasks panel, after
//! the commit made there. Each is the person's click on a confirm that shows
//! the exact command, and each runs every check below again inside the call
//! that acts; what the page showed is never trusted.
//!
//! What is pushed is the commit the Tasks panel made, by its id, never
//! `HEAD`: HEAD must still be that commit on the branch it was made on, the
//! commits it adds on top of the session's start must be the ones on record,
//! the folder must hold nothing it left out, its review must be the one it
//! was made from or a review of the commit itself, with every blocker
//! waived, no helper work may be open, and the repository's own git config
//! must name no program to run and no other push address. A pull request
//! also needs the push to have gone through, and right before gh starts,
//! origin's branch must still be that commit: gh opens the pull request for
//! whatever the branch on origin holds. The settings are read as Ask, so
//! every push and pull request asks, whatever they say.
//!
//! Every git call that reads the project for these checks is one of
//! `grokhub_agent::ship_tree`'s, with replace refs and grafts off.

use crate::commit::unread;
use crate::ship::{self, Action, CodeShip, Plan, Probe, ShipMode};
use grokhub_agent::review::{self, Shipped};
use grokhub_agent::ship_tree::{self, same_folder};
use std::path::Path;

/// No commit was made from the Tasks panel in this session.
pub const NOT_COMMITTED: &str =
    "Commit the change here first: only the commit made here is pushed.";
const OTHER_FOLDER: &str = "The commit was made in another project folder.";
const HEAD_MOVED: &str = "HEAD moved since the commit was made here; push from your terminal.";
const HISTORY: &str =
    "The history under the commit changed since it was made; push from your terminal.";
const FILES_LEFT: &str =
    "Files changed since the commit. Commit them, or put them back, before a push.";
const NO_REVIEW: &str = "The review the commit was made from is no longer on record.";
const OTHER_REVIEW: &str = "The review on record is not the one the commit was made from.";
const NOT_PUSHED: &str = "Push the commit first: the pull request is opened for it.";

fn counted(count: u32, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Runs every check of a push, or of a pull request, of the session's
/// commit in `folder`. The commit's record when they all pass, else the
/// sentence of the first that does not.
fn gate(folder: &Path, session: &str, action: Action) -> Result<Shipped, String> {
    // 1. The commit made here, in this repository.
    let shipped = review::shipped_for(session)
        .map_err(|err| format!("The commit's record can't be read: {err}"))?
        .ok_or(NOT_COMMITTED)?;
    let (top, head) = ship_tree::head(folder).map_err(|err| unread(&err))?;
    if !same_folder(Path::new(&shipped.project), &top) {
        return Err(OTHER_FOLDER.into());
    }
    // 7, first: a program the repository's config names stops every read
    // below. It is read again right before git or gh is spawned.
    if let Some(key) = ship_tree::untrusted_config(&top).map_err(|err| unread(&err))? {
        return Err(ship::untrusted_sentence(&key));
    }
    // 2. HEAD is that commit, on its branch.
    if head != shipped.sha {
        return Err(HEAD_MOVED.into());
    }
    if ship_tree::branch(&top)
        .map_err(|err| unread(&err))?
        .as_deref()
        != Some(&shipped.branch)
    {
        return Err(format!(
            "The commit was made on {}, which is not checked out now.",
            shipped.branch
        ));
    }
    // 3. It adds exactly the commits on record on top of the session's start.
    let since = match ship_tree::commits_since(&top, &shipped.base, &shipped.sha) {
        Ok(commits) => commits,
        Err(err) if err == ship_tree::NOT_AN_ANCESTOR => return Err(HISTORY.into()),
        Err(err) => return Err(unread(&err)),
    };
    if !since
        .iter()
        .map(|commit| &commit.sha)
        .eq(shipped.commits.iter())
    {
        return Err(HISTORY.into());
    }
    // 4. Nothing in the folder was left out of it.
    let snap = ship_tree::snapshot(folder).map_err(|err| unread(&err))?;
    if snap.tree != shipped.tree {
        return Err(FILES_LEFT.into());
    }
    // 5. Its review is the one it was made from, or a review of the commit
    // itself ("Review again" after the commit), every blocker waived.
    let review = review::review_for(session)
        .map_err(|err| format!("The review can't be read: {err}"))?
        .ok_or(NO_REVIEW)?;
    let made_from = review.head == shipped.parent;
    let of_commit = review.head == shipped.sha
        && review.base == shipped.base
        && review.commits == shipped.commits;
    if !same_folder(Path::new(&review.project), &top)
        || review.tree != shipped.tree
        || !(made_from || of_commit)
    {
        return Err(OTHER_REVIEW.into());
    }
    let open = review.open_blockers();
    if open > 0 {
        return Err(format!("The review found {}.", counted(open, "blocker")));
    }
    // 6. No helper work is open. The shell adds the agent's own turn.
    grokhub_agent::settled(session, folder)?;
    // 9. A pull request is opened for a commit that was pushed.
    if action == Action::Pr && !shipped.pushed {
        return Err(NOT_PUSHED.into());
    }
    Ok(shipped)
}

/// A plan that does not run, and why.
fn refused(folder: &Path, cfg: &CodeShip, action: Action, why: String) -> Plan {
    Plan {
        action,
        branch: ship::current_branch(folder),
        command: String::new(),
        needs_confirm: true,
        blocked: Some(why),
        mode: ShipMode::Ask,
        auto_base: cfg.auto_base.clone(),
        app_made: false,
        pr_url: None,
        skips_hook: false,
    }
}

/// What the Tasks panel's push, or its pull request, of the session's
/// commit in `folder` would run, with the exact command the confirm shows.
/// Read only.
pub fn plan(
    config_dir: &Path,
    folder: &Path,
    session: &str,
    cfg: &CodeShip,
    action: Action,
) -> Plan {
    plan_with(config_dir, folder, session, cfg, action, None)
}

pub(crate) fn plan_with(
    config_dir: &Path,
    folder: &Path,
    session: &str,
    cfg: &CodeShip,
    action: Action,
    probe: Probe,
) -> Plan {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    match gate(folder, session, action) {
        Ok(shipped) => ship::plan_at_with(config_dir, folder, cfg, action, &shipped.sha, probe),
        Err(why) => refused(folder, cfg, action, why),
    }
}

/// Pushes the session's commit in `folder`, or opens its pull request,
/// after the person's yes. Every check runs again here; `confirmed` must be
/// true and `command` the exact command the confirm showed. A push that
/// went through is kept on the commit's record, which a pull request needs.
pub fn push_with(
    config_dir: &Path,
    folder: &Path,
    session: &str,
    cfg: &CodeShip,
    action: Action,
    confirmed: bool,
    command: Option<&str>,
) -> Result<ship::Shipped, String> {
    push_probed(
        config_dir, folder, session, cfg, action, confirmed, command, None,
    )
}

pub(crate) fn push_probed(
    config_dir: &Path,
    folder: &Path,
    session: &str,
    cfg: &CodeShip,
    action: Action,
    confirmed: bool,
    command: Option<&str>,
    probe: Probe,
) -> Result<ship::Shipped, String> {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let shipped = gate(folder, session, action)?;
    // gh opens the pull request for what origin's branch holds, so it must
    // be the commit made here. Read over the network, so only for a yes.
    if action == Action::Pr && confirmed {
        if ship::origin_branch(folder, &shipped.branch)?.as_deref() != Some(shipped.sha.as_str()) {
            return Err(format!(
                "{} on origin is not the commit made here; open the pull request from your \
                 terminal.",
                shipped.branch
            ));
        }
    }
    let mut done = ship::ship_at_with(
        config_dir,
        folder,
        cfg,
        action,
        &shipped.sha,
        confirmed,
        command,
        probe,
    )?;
    if action == Action::Push && done.exit == 0 && !shipped.pushed {
        let pushed = Shipped {
            pushed: true,
            ..shipped
        };
        if let Err(err) = review::note_shipped(session, &pushed) {
            done.summary.push_str(&format!(
                " Its record could not be kept ({err}), so open the pull request from your terminal."
            ));
        }
    }
    Ok(done)
}

#[cfg(test)]
#[path = "push_tests.rs"]
mod tests;
