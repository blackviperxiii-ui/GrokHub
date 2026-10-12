//! Brain 6 PR 1: policy rows and playbook steps as tunable surfaces of the
//! bounded self-improvement loop (card 32).
//!
//! A tweak names its target as `policy:<intent>-<care>` or `playbook:<id>`
//! and carries the user version it was built on (the SHA-256 of the user's
//! file, empty when there is none). [`propose`] runs the protected-surface
//! guard and its Brain locks, brings any hand edit into the ChangeLedger as
//! the user's own version, refuses a tweak whose base is no longer the
//! user's file, then writes the user's file through the ledger
//! (`origin: self_manage`) and starts a trial on it. The trial is judged on
//! the runs whose metrics line names that row or playbook; a worse one is
//! put back through the ledger, and a hand edit since is never written over.
//!
//! Local only: no network, no model call.

use std::fs;
use std::path::{Path, PathBuf};

use grokhub_core::brain_policy::{parse_policy, policy_text, PolicyRow};
use grokhub_core::protected_surface::{ProtectedSurface, TuneTarget};

use crate::harness::canary::{start_trial_at_share, Metric, Version};
use crate::harness::changes::{
    content_hash, entry_id, note_proposal_finding, private_write, read_capped, record_change,
    record_outside_edit, ChangeKind, ChangeTarget,
};
use crate::harness::self_improve::guard_span;
use crate::harness::span::{append_span, Origin};

/// The ledger id of the user's policy file.
pub const POLICY_ID: &str = "brain_policy";
/// A tuned row or playbook applies to every matching run once written.
pub const BRAIN_SHARE_PCT: u8 = 100;
/// The ledger reason on a hand edit brought in before a tweak.
pub const OUTSIDE_EDIT: &str = "your edit";

/// The user's policy file, `{config}/brain/brain_policy.toml`.
pub struct PolicyTarget<'a> {
    pub config_dir: &'a Path,
}

impl ChangeTarget for PolicyTarget<'_> {
    fn kind(&self) -> ChangeKind {
        ChangeKind::Policy
    }

    fn id(&self) -> Result<String, String> {
        Ok(POLICY_ID.into())
    }

    fn file(&self) -> Result<PathBuf, String> {
        Ok(crate::brain::policy_path(self.config_dir))
    }

    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        read_capped(&self.file()?)
    }

    fn put(&self, bytes: Option<&[u8]>) -> Result<(), String> {
        put_file(&self.file()?, bytes)
    }
}

/// A user playbook, `{config}/playbooks/<id>.toml`. Gone means the shipped
/// one (when there is one) applies.
pub struct PlaybookTarget<'a> {
    pub config_dir: &'a Path,
    pub id: &'a str,
}

impl ChangeTarget for PlaybookTarget<'_> {
    fn kind(&self) -> ChangeKind {
        ChangeKind::Playbook
    }

    /// The playbook id. One the redactor reads as a secret
    /// (`task-general-copy` holds `sk-`) is kept with dots for dashes, which
    /// no key pattern matches; playbook ids never hold a dot, so the target
    /// built from either form is the same file.
    fn id(&self) -> Result<String, String> {
        let id = self.playbook_id()?;
        Ok(entry_id(&id).unwrap_or_else(|_| id.replace('-', ".")))
    }

    fn file(&self) -> Result<PathBuf, String> {
        Ok(crate::brain::playbook::playbooks_dir(self.config_dir)
            .join(format!("{}.toml", self.playbook_id()?)))
    }

    fn label_of(&self, bytes: &[u8]) -> String {
        grokhub_core::playbook::parse(&String::from_utf8_lossy(bytes))
            .map(|p| p.name)
            .unwrap_or_default()
    }

    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        read_capped(&self.file()?)
    }

    fn put(&self, bytes: Option<&[u8]>) -> Result<(), String> {
        put_file(&self.file()?, bytes)
    }
}

impl PlaybookTarget<'_> {
    /// The playbook's own id, from it or its ledger id.
    fn playbook_id(&self) -> Result<String, String> {
        let id = self.id.replace('.', "-");
        if grokhub_core::playbook::id_ok(&id) {
            Ok(id)
        } else {
            Err(format!("no playbook `{}`", self.id))
        }
    }
}

fn put_file(path: &Path, bytes: Option<&[u8]>) -> Result<(), String> {
    match bytes {
        Some(b) => private_write(path, b),
        None if path.exists() => fs::remove_file(path).map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

/// One proposed tweak.
#[derive(Debug, Clone, Copy)]
pub struct Tweak<'a> {
    /// `policy:<intent>-<care>` or `playbook:<id>`.
    pub target: &'a str,
    /// The user version it was built on ([`user_version`]).
    pub base: &'a str,
    /// The new row (one `[[row]]` table) or the new playbook file.
    pub text: &'a str,
    /// What notes call it ("Linux repair checks ports before services").
    pub label: &'a str,
    pub metric: Metric,
}

enum Surface {
    Policy(String),
    Playbook(String),
}

fn surface(target: &str) -> Result<Surface, String> {
    match ProtectedSurface::check(target)? {
        TuneTarget::Policy(key) => Ok(Surface::Policy(key)),
        TuneTarget::Playbook(id) => Ok(Surface::Playbook(id)),
        _ => Err("only a policy row or a playbook is tuned here".into()),
    }
}

fn file_hash(target: &dyn ChangeTarget) -> Result<String, String> {
    Ok(target
        .read()?
        .as_deref()
        .map(content_hash)
        .unwrap_or_default())
}

/// The user version a tweak on `target` builds on: the SHA-256 of the
/// user's file now, empty when there is none.
pub fn user_version(config_dir: &Path, target: &str) -> Result<String, String> {
    match surface(target)? {
        Surface::Policy(_) => file_hash(&PolicyTarget { config_dir }),
        Surface::Playbook(id) => file_hash(&PlaybookTarget {
            config_dir,
            id: &id,
        }),
    }
}

/// A refused tweak: a deny span and a scope finding, nothing written.
fn refuse(config_dir: &Path, target: &str, why: String, now: u64) -> Result<Version, String> {
    let _ = append_span(config_dir, &guard_span(target, &why, now));
    note_proposal_finding(config_dir, target, &why);
    Err(why)
}

/// Check, version and write one tweak, and start its trial.
pub fn propose(config_dir: &Path, tweak: &Tweak, now: u64) -> Result<Version, String> {
    match surface(tweak.target) {
        Ok(Surface::Policy(key)) => propose_row(config_dir, &key, tweak, now),
        Ok(Surface::Playbook(id)) => propose_playbook(config_dir, &id, tweak, now),
        Err(why) => refuse(config_dir, tweak.target, why, now),
    }
}

/// Bring a hand edit into the ledger, then refuse a tweak built on an older
/// user version.
fn on_user_version(
    config_dir: &Path,
    target: &dyn ChangeTarget,
    base: &str,
    what: &str,
) -> Result<(), String> {
    record_outside_edit(config_dir, target, OUTSIDE_EDIT)?;
    if file_hash(target)? != base {
        return Err(format!(
            "{what} changed since this tweak was proposed, so it was not applied; GrokHub proposes again on your version"
        ));
    }
    Ok(())
}

fn propose_row(config_dir: &Path, key: &str, tweak: &Tweak, now: u64) -> Result<Version, String> {
    let new = match parse_policy(tweak.text) {
        Ok(rows) if rows.len() == 1 => rows[0].clone(),
        Ok(_) => {
            return refuse(
                config_dir,
                tweak.target,
                "a policy tweak names exactly one row".into(),
                now,
            )
        }
        Err(why) => return refuse(config_dir, tweak.target, why, now),
    };
    let (rows, _) = crate::brain::load_policy(config_dir);
    let base_row = rows.iter().find(|r| r.key() == key);
    if let Err(why) = ProtectedSurface::check_policy_row(key, base_row, &new) {
        return refuse(config_dir, tweak.target, why, now);
    }
    let target = PolicyTarget { config_dir };
    on_user_version(config_dir, &target, tweak.base, "Your policy file")?;
    let before = read_text(&target)?;
    let mut user: Vec<PolicyRow> = if before.trim().is_empty() {
        Vec::new()
    } else {
        parse_policy(&before).map_err(|e| {
            format!("Your policy file is ignored ({e}), so GrokHub won't write over it")
        })?
    };
    match user.iter_mut().find(|r| r.key() == key) {
        Some(r) => *r = new,
        None => user.push(new),
    }
    let after = policy_text(&user)?;
    write_and_try(
        config_dir,
        &target,
        &format!("policy:{key}"),
        tweak,
        (&before, &after),
    )
}

fn propose_playbook(
    config_dir: &Path,
    id: &str,
    tweak: &Tweak,
    now: u64,
) -> Result<Version, String> {
    let (books, _) = crate::brain::playbook::load(config_dir);
    let Some(base) = books.into_iter().find(|l| l.playbook.id == id) else {
        return refuse(config_dir, tweak.target, format!("no playbook `{id}`"), now);
    };
    let checked = ProtectedSurface::check_playbook(&base.playbook, tweak.text)
        .and_then(|p| crate::brain::playbook::lock_check(&p));
    if let Err(why) = checked {
        return refuse(config_dir, tweak.target, why, now);
    }
    let target = PlaybookTarget { config_dir, id };
    on_user_version(
        config_dir,
        &target,
        tweak.base,
        &format!("Your playbook {}", base.playbook.name),
    )?;
    write_and_try(
        config_dir,
        &target,
        &format!("playbook:{id}"),
        tweak,
        (&base.text, tweak.text),
    )
}

fn read_text(target: &dyn ChangeTarget) -> Result<String, String> {
    Ok(target
        .read()?
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default())
}

/// Write `after` through the ledger as GrokHub's and start its trial.
fn write_and_try(
    config_dir: &Path,
    target: &dyn ChangeTarget,
    class: &str,
    tweak: &Tweak,
    (before, after): (&str, &str),
) -> Result<Version, String> {
    let reason = format!("self-tune: {}", tweak.label.trim());
    let change = record_change(config_dir, target, Origin::SelfManage, &reason, || {
        target.put(Some(after.as_bytes()))
    })?
    .ok_or_else(|| format!("{class} already says that"))?;
    start_trial_at_share(
        config_dir,
        &change,
        class,
        tweak.label,
        tweak.metric,
        (before, after),
        BRAIN_SHARE_PCT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::canary::{read_versions, settle_trials, VersionStatus};
    use crate::harness::changes::{ChangeLedger, ChangeOp};
    use crate::harness::read_scope_findings;
    use crate::harness::self_improve::{GUARD_TOOL, SELF_REVIEW_SESSION};
    use crate::harness::span::read_spans;
    use grokhub_core::outcome::OutcomeResult;
    use grokhub_core::run_metrics::{Arm, RunBrain, RunMetrics};

    const DAY: u64 = 24 * 60 * 60 * 1000;

    fn dirs(label: &str) -> (PathBuf, PathBuf) {
        let root = crate::harness::test_dir(label);
        let skills = root.join("skills");
        fs::create_dir_all(&skills).unwrap();
        (root, skills)
    }

    /// Repair Linux as shipped, with LF line endings.
    fn linux() -> String {
        grokhub_core::playbook::BUILTIN[1].replace("\r\n", "\n")
    }

    /// Repair Linux with the listening-ports check moved to the front.
    fn ports_first() -> String {
        let ports = "[[phase.step]]\nname = \"List listening ports (ss -tuln) and name what owns each\"\n\n";
        let first = "[[phase.step]]\nname = \"List failed services\"";
        let text = linux();
        assert!(text.contains(ports) && text.contains(first));
        text.replace(ports, "")
            .replacen(first, &format!("{ports}{first}"), 1)
    }

    fn repair_row(depth: &str) -> String {
        let rows = grokhub_core::brain_policy::builtin_policy();
        let mut row = grokhub_core::brain_policy::pick_row(&rows, "repair", "critical")
            .unwrap()
            .clone();
        row.depth = depth.into();
        policy_text(&[row]).unwrap()
    }

    fn brain(policy: &str, playbook: &str) -> RunBrain {
        RunBrain {
            intent: "repair".into(),
            care: "critical".into(),
            domain: "system".into(),
            policy: policy.into(),
            playbook: playbook.into(),
        }
    }

    /// `before` runs ahead of the trial at `base_steps`, then `canary` runs on it at `canary_steps`.
    fn runs(
        v: &Version,
        b: &RunBrain,
        (before, base_steps): (usize, u32),
        (canary, canary_steps): (usize, u32),
    ) -> Vec<RunMetrics> {
        let line = |i: usize, steps: u32, on: bool, at: u64| RunMetrics {
            run: format!("chat-r:{i}"),
            class: "topic:repair".into(),
            result: OutcomeResult::Success,
            steps,
            version: if on { v.version.clone() } else { String::new() },
            arm: on.then_some(Arm::Canary),
            brain: b.clone(),
            finished_at: at,
            at,
            ..Default::default()
        };
        let mut out: Vec<RunMetrics> = (0..before)
            .map(|i| line(i, base_steps, false, v.started_at - 1_000 + i as u64))
            .collect();
        out.extend(
            (0..canary).map(|i| line(100 + i, canary_steps, true, v.started_at + 1 + i as u64)),
        );
        out
    }

    fn file(p: &Path) -> Option<String> {
        fs::read_to_string(p).ok()
    }

    #[test]
    fn a_playbook_step_tweak_runs_as_a_trial_and_is_kept_with_a_named_note() {
        let (root, skills) = dirs("brain-tune-keep");
        let target = "playbook:repair-linux";
        assert_eq!(user_version(&root, target).unwrap(), "", "no user copy yet");
        let new = ports_first();
        let tweak = Tweak {
            target,
            base: "",
            text: &new,
            label: "Linux repair checks ports before services",
            metric: Metric::Steps,
        };
        let v = propose(&root, &tweak, 1).unwrap();
        assert_eq!(
            (
                v.kind.as_str(),
                v.id.as_str(),
                v.class.as_str(),
                v.share_pct,
                v.status
            ),
            (
                "playbook",
                "repair-linux",
                "playbook:repair-linux",
                100,
                VersionStatus::Canary
            )
        );
        assert_eq!(v.diff, "+0 -0 lines", "a moved step adds and drops no line");
        let path = root.join("playbooks").join("repair-linux.toml");
        assert_eq!(
            file(&path).as_deref(),
            Some(new.as_str()),
            "the user copy holds the tweak"
        );
        let line = ChangeLedger::load_kind(&root, ChangeKind::Playbook)
            .all()
            .last()
            .cloned()
            .unwrap();
        assert_eq!(
            (line.op, line.origin, line.reason.as_str()),
            (
                ChangeOp::Create,
                Origin::SelfManage,
                "self-tune: Linux repair checks ports before services"
            )
        );
        let b = brain("repair × critical", "repair-linux");
        let all = runs(&v, &b, (12, 9), (9, 7));
        assert!(
            settle_trials(&root, &skills, &all, v.started_at + 100).is_empty(),
            "9 runs and days left: still running"
        );
        let closed = settle_trials(&root, &skills, &all, v.started_at + 7 * DAY);
        assert_eq!(
            (closed[0].status, closed[0].note.as_str()),
            (
                VersionStatus::Kept,
                "Kept: Linux repair checks ports before services, 22% fewer steps over 9 runs"
            )
        );
        assert_eq!(
            file(&path).as_deref(),
            Some(new.as_str()),
            "kept: the tweak stays"
        );
        let (books, _) = crate::brain::playbook::load(&root);
        let picked = crate::brain::playbook::pick(&books, "repair", "system", "linux").unwrap();
        assert_eq!(
            picked.phases[0].steps[0].name,
            "List listening ports (ss -tuln) and name what owns each"
        );
    }

    #[test]
    fn a_policy_row_tweak_that_does_worse_is_rolled_back_to_the_users_file() {
        let (root, skills) = dirs("brain-tune-rollback");
        let user = "[[row]]\nintent = \"chat\"\ncare = \"casual\"\ngoal = \"cost\"\nreason = \"keep it cheap\"\ndepth = \"none\"\nask = \"act\"\nstop = \"answered\"\ntalk = \"one line\"\n";
        let path = crate::brain::policy_path(&root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, user).unwrap();
        let target = "policy:repair-critical";
        let base = user_version(&root, target).unwrap();
        assert_eq!(base, content_hash(user.as_bytes()));
        let row = repair_row("probe twice before any fix");
        let tweak = Tweak {
            target,
            base: &base,
            text: &row,
            label: "Repair probes twice before fixing",
            metric: Metric::Steps,
        };
        let v = propose(&root, &tweak, 1).unwrap();
        assert_eq!(
            (v.kind.as_str(), v.id.as_str(), v.class.as_str()),
            ("policy", "brain_policy", "policy:repair × critical")
        );
        let (rows, note) = crate::brain::load_policy(&root);
        assert_eq!(note, None);
        let repair = grokhub_core::brain_policy::pick_row(&rows, "repair", "critical").unwrap();
        assert_eq!(repair.depth, "probe twice before any fix");
        let chat = grokhub_core::brain_policy::pick_row(&rows, "chat", "casual").unwrap();
        assert_eq!(chat.reason, "keep it cheap", "the user's own row stays");
        let ledger = ChangeLedger::load_kind(&root, ChangeKind::Policy);
        let ops: Vec<(ChangeOp, Origin)> = ledger.all().iter().map(|c| (c.op, c.origin)).collect();
        assert_eq!(
            ops,
            vec![
                (ChangeOp::Create, Origin::User),
                (ChangeOp::Modify, Origin::SelfManage)
            ],
            "the user's file is a version first"
        );
        let all = runs(
            &v,
            &brain("repair × critical", "repair-linux"),
            (10, 10),
            (10, 12),
        );
        let closed = settle_trials(&root, &skills, &all, v.started_at + 100);
        assert_eq!(
            (closed[0].status, closed[0].note.as_str()),
            (
                VersionStatus::RolledBack,
                "Rolled back: Repair probes twice before fixing, 20% more steps over 10 runs"
            )
        );
        assert_eq!(
            file(&path).as_deref(),
            Some(user),
            "the user's file is back byte for byte"
        );
        let last = ChangeLedger::load_kind(&root, ChangeKind::Policy)
            .all()
            .last()
            .cloned()
            .unwrap();
        assert_eq!(
            (last.op, last.origin, last.undoes),
            (ChangeOp::Undo, Origin::SelfManage, Some(2))
        );
    }

    #[test]
    fn a_tweak_built_on_an_old_user_version_is_never_applied_over_the_newer_edit() {
        let (root, skills) = dirs("brain-tune-stale");
        let target = "playbook:repair-linux";
        let old_base = user_version(&root, target).unwrap();
        // The user edits Repair Linux in Settings: a ledger line by the user.
        let mine = linux().replace("summary = \"", "summary = \"Mine. ");
        crate::brain::playbook::save(&root, &mine).unwrap();
        let path = root.join("playbooks").join("repair-linux.toml");
        let lines = ChangeLedger::load_kind(&root, ChangeKind::Playbook);
        assert_eq!(
            lines
                .all()
                .iter()
                .map(|c| (c.op, c.origin))
                .collect::<Vec<_>>(),
            vec![(ChangeOp::Create, Origin::User)]
        );
        let new = ports_first();
        let tweak = Tweak {
            target,
            base: &old_base,
            text: &new,
            label: "Linux repair checks ports before services",
            metric: Metric::Steps,
        };
        assert_eq!(
            propose(&root, &tweak, 1).unwrap_err(),
            "Your playbook Repair Linux changed since this tweak was proposed, so it was not applied; GrokHub proposes again on your version"
        );
        assert_eq!(
            file(&path).as_deref(),
            Some(mine.as_str()),
            "the user's edit stays"
        );
        assert!(read_versions(&root).is_empty(), "no trial");
        // Proposed again on the user's version: the tweak builds on it.
        let on_mine = ports_first().replace("summary = \"", "summary = \"Mine. ");
        let base = user_version(&root, target).unwrap();
        assert_eq!(base, content_hash(mine.as_bytes()));
        let v = propose(
            &root,
            &Tweak {
                base: &base,
                text: &on_mine,
                ..tweak
            },
            2,
        )
        .unwrap();
        assert_eq!(file(&path).as_deref(), Some(on_mine.as_str()));
        // A hand edit during the trial is the user's version: a worse trial ends without touching it.
        let by_hand = format!("{on_mine}\n# my note\n");
        fs::write(&path, &by_hand).unwrap();
        let all = runs(
            &v,
            &brain("repair × critical", "repair-linux"),
            (10, 10),
            (10, 30),
        );
        let closed = settle_trials(&root, &skills, &all, v.started_at + 100);
        assert_eq!(
            (closed[0].status, closed[0].note.as_str()),
            (VersionStatus::Ended, "")
        );
        assert_eq!(file(&path).as_deref(), Some(by_hand.as_str()));
        // A hand edit to the policy file before a tweak is versioned as the user's.
        let ppath = crate::brain::policy_path(&root);
        let pbase = user_version(&root, "policy:repair-critical").unwrap();
        fs::create_dir_all(ppath.parent().unwrap()).unwrap();
        let hand = repair_row("my own depth");
        fs::write(&ppath, &hand).unwrap();
        let row = repair_row("probe twice before any fix");
        let err = propose(
            &root,
            &Tweak {
                target: "policy:repair-critical",
                base: &pbase,
                text: &row,
                label: "Probe twice",
                metric: Metric::Steps,
            },
            3,
        )
        .unwrap_err();
        assert_eq!(err, "Your policy file changed since this tweak was proposed, so it was not applied; GrokHub proposes again on your version");
        assert_eq!(file(&ppath).as_deref(), Some(hand.as_str()));
        let p = ChangeLedger::load_kind(&root, ChangeKind::Policy);
        assert_eq!(
            p.all()
                .iter()
                .map(|c| (c.op, c.origin, c.reason.as_str()))
                .collect::<Vec<_>>(),
            vec![(ChangeOp::Create, Origin::User, OUTSIDE_EDIT)]
        );
        assert_eq!(p.all()[0].after_hash, content_hash(hand.as_bytes()));
    }

    #[test]
    fn protected_tweaks_are_refused_logged_and_write_nothing() {
        let (root, _) = dirs("brain-tune-guard");
        let text = linux();
        let pb = "playbook:repair-linux";
        let restore = "[[phase.step]]\nname = \"Take a restore point before the first change\"\n\n";
        let reinstall = "name = \"Reinstall packages with missing files\"\n";
        let backup = text.lines().find(|l| l.starts_with("backup = ")).unwrap();
        let critical = {
            let rows = grokhub_core::brain_policy::builtin_policy();
            let mut row = grokhub_core::brain_policy::pick_row(&rows, "repair", "critical")
                .unwrap()
                .clone();
            row.care = "casual".into();
            policy_text(&[row]).unwrap()
        };
        let cases: Vec<(&str, String, &str)> = vec![
            (pb, text.replace(restore, ""), "GrokHub never removes the \"Take a restore point before the first change\" step from Repair Linux on its own"),
            (
                pb,
                text.replace("\n[[phase.step]]\nname = \"Re-check with the same probe\"\n", "\n"),
                "GrokHub never removes the \"Re-check with the same probe\" step from Repair Linux on its own",
            ),
            (pb, text.replace(backup, "backup = \"\""), "GrokHub never empties the back-up or check of Repair Linux on its own"),
            (
                pb,
                text.replace(reinstall, &format!("{reinstall}tier = \"safe\"\n")),
                "GrokHub never lowers the care of Repair Linux on its own: \"Reinstall packages with missing files\" was risky, asks first",
            ),
            ("policy:repair-critical", critical, "GrokHub never lowers the repair × critical row on its own"),
            ("policy:approval-critical", String::new(), "GrokHub never changes the gate on its own"),
            ("playbook:hard-class-steps", String::new(), "GrokHub never changes the hard-class list on its own"),
            ("playbook:honesty", String::new(), "GrokHub never changes the honesty rules on its own"),
            ("playbook:canary", String::new(), "GrokHub never changes the evaluator and rollback on its own"),
            ("skill:weekly-report", String::new(), "only a policy row or a playbook is tuned here"),
        ];
        let n = cases.len();
        for (target, new, why) in &cases {
            let tweak = Tweak {
                target,
                base: "",
                text: new,
                label: "x",
                metric: Metric::Steps,
            };
            assert_eq!(propose(&root, &tweak, 7).unwrap_err(), *why, "{target}");
        }
        // The agent's hard-class lock runs on a tweak too.
        let general = grokhub_core::playbook::BUILTIN[0].replace("\r\n", "\n");
        let unsafe_step = general.replacen(
            "name = \"Check the change before the next one\"",
            "name = \"Check the change before the next one\"\n\n[[phase.step]]\nname = \"Do it\"\ntier = \"safe\"\nrun = \"rm -rf ~/Downloads/old\"",
            1,
        );
        let t = Tweak {
            target: "playbook:task-general",
            base: "",
            text: &unsafe_step,
            label: "x",
            metric: Metric::Steps,
        };
        assert_eq!(
            propose(&root, &t, 7).unwrap_err(),
            "step \"Do it\" is a hard-class action (Delete) and must be marked risky; it still asks you first"
        );
        let guards: Vec<_> = read_spans(&root, SELF_REVIEW_SESSION)
            .unwrap()
            .into_iter()
            .filter(|s| s.tool == GUARD_TOOL)
            .collect();
        assert_eq!(guards.len(), n + 1, "one deny span per refusal");
        assert!(guards
            .iter()
            .all(|s| s.decision == "deny" && s.origin == Origin::SelfManage));
        assert_eq!(read_scope_findings(&root).len(), n + 1);
        assert!(
            !root.join("playbooks").exists() && !crate::brain::policy_path(&root).exists(),
            "nothing written"
        );
        assert!(ChangeLedger::load_kind(&root, ChangeKind::Playbook)
            .all()
            .is_empty());
        assert!(ChangeLedger::load_kind(&root, ChangeKind::Policy)
            .all()
            .is_empty());
        assert!(read_versions(&root).is_empty());
    }
}
