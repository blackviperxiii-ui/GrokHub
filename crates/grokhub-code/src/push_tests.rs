use super::*;
use crate::activity::{self as code_activity, Kind as ActKind};
use crate::commit::{self, Draft};
use crate::secrets::Vault;
use crate::ship::{Host, Hosts, Pr};
use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::review::Review;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

/// No project secrets: the scan only reads shapes and names here.
struct NoVault;

impl Vault for NoVault {
    fn get(&self, _account: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn set(&self, _account: &str, _value: &str) -> Result<(), String> {
        Ok(())
    }
    fn delete(&self, _account: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Plain git in `dir`, for building a fixture.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "safe.directory=*", "-c", "init.defaultBranch=main"])
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "{args:?} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A config folder, a local bare `origin`, a project that pushed `main`
/// there and works on `work/one`, and a session that began in it.
struct Fixture {
    base: PathBuf,
    cfg: PathBuf,
    origin: PathBuf,
    project: PathBuf,
    session: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Pushing to a local folder is pushing to a host that is not GitHub.
fn cfg() -> CodeShip {
    CodeShip {
        hosts: Hosts {
            github: true,
            other: true,
        },
        ..CodeShip::default()
    }
}

fn fixture(name: &str) -> Fixture {
    static SERIAL: AtomicU32 = AtomicU32::new(0);
    let base = std::env::temp_dir().join(format!(
        "grokhub-push-{name}-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&base);
    let origin = base.join("origin.git");
    let project = base.join("my-app");
    std::fs::create_dir_all(&project).unwrap();
    git(&base, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    git(&project, &["init", "-q", "-b", "main"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "test"]);
    git(&project, &["config", "commit.gpgsign", "false"]);
    std::fs::write(project.join("README"), "hi\n").unwrap();
    git(&project, &["add", "README"]);
    git(&project, &["commit", "-q", "-m", "init"]);
    git(
        &project,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&project, &["push", "-q", "origin", "main"]);
    git(&project, &["remote", "set-head", "origin", "main"]);
    git(&project, &["switch", "-q", "-c", "work/one"]);
    let fx = Fixture {
        cfg: base.join("cfg"),
        origin,
        project,
        session: format!("push-{name}"),
        base,
    };
    let _config = ConfigGuard::set(&fx.cfg);
    review::note_base(&fx.session, &fx.project);
    fx
}

fn write(fx: &Fixture, file: &str, text: &str) {
    let path = fx.project.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn record(fx: &Fixture, name: &str, text: &str) {
    let dir = fx.cfg.join("helper-work").join(&fx.session);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), text).unwrap();
}

/// A review of the change as it is now, with this many blockers.
fn reviewed(fx: &Fixture, blockers: u32) {
    let snap = ship_tree::snapshot(&fx.project).unwrap();
    let base = {
        let _config = ConfigGuard::set(&fx.cfg);
        review::base_for(&fx.session).unwrap().expect("a base")
    };
    let commits = ship_tree::commits_since(&snap.top, &base.head, &snap.head).unwrap();
    let body = match blockers {
        0 => "Findings from this run\n\n- Info: README:1 nothing needs fixing".to_string(),
        n => format!(
            "Findings from this run\n\n{}",
            vec!["- High: src/a.txt:1 the total is off by one"; n as usize].join("\n")
        ),
    };
    let review = Review {
        project: snap.top.to_string_lossy().into_owned(),
        base: base.head,
        head: snap.head.clone(),
        commits: commits.into_iter().map(|commit| commit.sha).collect(),
        tree: snap.tree.clone(),
        at_ms: 1_700_000_000_000,
        plan_given: true,
        body,
        blockers,
        cut: false,
        waived: Vec::new(),
    };
    record(fx, "review.json", &serde_json::to_string(&review).unwrap());
}

/// The session's change, one new file, reviewed clean and committed from
/// the Tasks panel. The commit's id.
fn committed(fx: &Fixture) -> String {
    write(fx, "src/a.txt", "one\n");
    reviewed(fx, 0);
    let draft = Draft::default();
    let status = commit::status_for(&fx.cfg, &fx.project, &fx.session, &cfg(), &NoVault, &draft);
    assert_eq!(status.blocked, Vec::<String>::new());
    let confirm = status
        .commit
        .and_then(|plan| plan.confirm)
        .expect("a confirm");
    commit::commit_with(
        &fx.cfg,
        &fx.project,
        &fx.session,
        &cfg(),
        &NoVault,
        &draft,
        true,
        Some(&confirm),
    )
    .unwrap()
    .sha
}

fn head(fx: &Fixture) -> String {
    git(&fx.project, &["rev-parse", "HEAD"])
}

/// What `refs/heads/<branch>` names in the bare origin.
fn on_origin(fx: &Fixture, branch: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(&fx.origin)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .unwrap();
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The origin holds an object with this id.
fn origin_has(fx: &Fixture, sha: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(&fx.origin)
        .args(["cat-file", "-e", sha])
        .status()
        .unwrap()
        .success()
}

fn plan_of(fx: &Fixture, action: Action) -> Plan {
    plan(&fx.cfg, &fx.project, &fx.session, &cfg(), action)
}

fn push(fx: &Fixture, confirmed: bool, command: Option<&str>) -> Result<ship::Shipped, String> {
    push_with(
        &fx.cfg,
        &fx.project,
        &fx.session,
        &cfg(),
        Action::Push,
        confirmed,
        command,
    )
}

fn shipped(fx: &Fixture) -> Shipped {
    let _config = ConfigGuard::set(&fx.cfg);
    review::shipped_for(&fx.session)
        .unwrap()
        .expect("a commit on record")
}

fn push_command(sha: &str) -> String {
    format!(
        "git push --porcelain --no-recurse-submodules --no-signed origin {sha}:refs/heads/work/one"
    )
}

/// The sentence a plan and a push both give, and that origin got nothing.
fn refused_with(fx: &Fixture, sha: &str, why: &str) {
    let plan = plan_of(fx, Action::Push);
    assert_eq!(plan.blocked.as_deref(), Some(why));
    assert_eq!(plan.command, "");
    assert_eq!(
        push(fx, true, Some(&push_command(sha))),
        Err(why.to_string())
    );
    assert_eq!(on_origin(fx, "work/one"), None);
}

// -- the push -----------------------------------------------------------------

#[test]
fn the_commit_made_here_is_pushed_by_its_id_after_a_yes_to_the_exact_command() {
    let fx = fixture("push");
    let sha = committed(&fx);
    let plan = plan_of(&fx, Action::Push);
    assert_eq!(plan.command, push_command(&sha));
    assert_eq!(plan.blocked, None);
    assert!(plan.needs_confirm);
    assert_eq!(plan.mode, ShipMode::Ask);
    assert_eq!(plan.branch.as_deref(), Some("work/one"));

    // No yes, or a yes to another command, pushes nothing.
    assert_eq!(
        push(&fx, false, None),
        Err("This needs a confirmation first.".to_string())
    );
    assert_eq!(
        push(
            &fx,
            true,
            Some("git push --porcelain origin HEAD:refs/heads/work/one")
        ),
        Err(
            "The branch changed since you confirmed. Check the command and confirm again."
                .to_string()
        )
    );
    assert_eq!(on_origin(&fx, "work/one"), None);
    assert!(!shipped(&fx).pushed);

    let done = push(&fx, true, Some(&push_command(&sha))).unwrap();
    assert_eq!(done.exit, 0, "{}", done.output);
    // The grafts file the push is told not to read brings no hint.
    assert!(!done.output.contains("hint:"), "{}", done.output);
    assert_eq!(done.command, push_command(&sha));
    assert_eq!(
        done.summary,
        format!("Pushed {} to work/one on origin.", &sha[..8])
    );
    assert_eq!(on_origin(&fx, "work/one").as_deref(), Some(sha.as_str()));
    assert!(shipped(&fx).pushed);
    let log = code_activity::list(&fx.cfg, 10);
    assert_eq!(log[0].kind, ActKind::Push);
    assert_eq!(log[0].approved_by, "user");
    assert_eq!(log[0].mode, "ask");
    assert_eq!(log[0].command, push_command(&sha));
    assert_eq!(log[1].kind, ActKind::Commit);
}

#[test]
fn a_session_with_no_commit_made_here_pushes_nothing() {
    let fx = fixture("none");
    // The agent's own commit is not one made from the Tasks panel.
    write(&fx, "src/a.txt", "one\n");
    git(&fx.project, &["add", "src/a.txt"]);
    git(&fx.project, &["commit", "-q", "-m", "agent"]);
    let sha = head(&fx);
    refused_with(
        &fx,
        &sha,
        "Commit the change here first: only the commit made here is pushed.",
    );
}

#[test]
fn a_commit_made_in_another_folder_is_not_pushed_from_this_one() {
    let fx = fixture("folder");
    let sha = committed(&fx);
    let mut other = shipped(&fx);
    other.project = fx.base.join("elsewhere").to_string_lossy().into_owned();
    record(&fx, "ship.json", &serde_json::to_string(&other).unwrap());
    refused_with(&fx, &sha, "The commit was made in another project folder.");
}

// -- gate 2: HEAD is the commit, on its branch --------------------------------

#[test]
fn a_commit_on_top_of_the_one_made_here_is_never_pushed() {
    let fx = fixture("moved");
    let sha = committed(&fx);
    write(&fx, "src/b.txt", "two\n");
    git(&fx.project, &["add", "src/b.txt"]);
    git(&fx.project, &["commit", "-q", "-m", "after"]);
    refused_with(
        &fx,
        &sha,
        "HEAD moved since the commit was made here; push from your terminal.",
    );
}

#[test]
fn a_same_tree_commit_with_another_parent_moved_onto_the_branch_is_refused() {
    // Attack B2: the tree id is the same, and HEAD carries a side history.
    let fx = fixture("b2");
    let sha = committed(&fx);
    let side_tree = git(&fx.project, &["rev-parse", "HEAD^{tree}"]);
    let side = git(&fx.project, &["commit-tree", &side_tree, "-m", "side"]);
    let merge = git(
        &fx.project,
        &[
            "commit-tree",
            &side_tree,
            "-p",
            &sha,
            "-p",
            &side,
            "-m",
            "same tree",
        ],
    );
    git(&fx.project, &["update-ref", "refs/heads/work/one", &merge]);
    refused_with(
        &fx,
        &sha,
        "HEAD moved since the commit was made here; push from your terminal.",
    );
    assert!(!origin_has(&fx, &side));
}

#[test]
fn another_branch_at_the_same_commit_is_not_pushed() {
    let fx = fixture("branch");
    let sha = committed(&fx);
    git(&fx.project, &["switch", "-q", "-c", "work/two"]);
    let why = "The commit was made on work/one, which is not checked out now.";
    assert_eq!(plan_of(&fx, Action::Push).blocked.as_deref(), Some(why));
    assert_eq!(
        push(&fx, true, Some(&push_command(&sha))),
        Err(why.to_string())
    );
    assert_eq!(on_origin(&fx, "work/two"), None);
    assert_eq!(on_origin(&fx, "work/one"), None);
}

// -- gate 3: the commits it adds are the ones on record ------------------------

#[test]
fn a_history_cut_under_the_commit_is_refused() {
    let fx = fixture("shallow");
    let sha = committed(&fx);
    // A shallow mark takes the commit's parents away: the session's start
    // is no longer under it.
    std::fs::write(fx.project.join(".git/shallow"), format!("{sha}\n")).unwrap();
    refused_with(
        &fx,
        &sha,
        "The history under the commit changed since it was made; push from your terminal.",
    );
}

#[test]
fn a_record_that_lists_other_commits_is_refused() {
    let fx = fixture("listed");
    let sha = committed(&fx);
    let mut wrong = shipped(&fx);
    wrong
        .commits
        .push("0123456789abcdef0123456789abcdef01234567".into());
    record(&fx, "ship.json", &serde_json::to_string(&wrong).unwrap());
    refused_with(
        &fx,
        &sha,
        "The history under the commit changed since it was made; push from your terminal.",
    );
}

#[test]
fn a_grafts_file_and_a_replace_ref_do_not_widen_what_is_pushed() {
    // Attack B3 and the grafts hole: each would give the commit a side
    // history in git's eyes. The checks and the push both read the history
    // the commits record, so the push sends the commit and nothing else.
    let fx = fixture("grafts");
    let sha = committed(&fx);
    let parent = git(&fx.project, &["rev-parse", "HEAD^"]);
    let tree = git(&fx.project, &["rev-parse", "HEAD^{tree}"]);
    let side = git(&fx.project, &["commit-tree", &tree, "-m", "side"]);
    std::fs::write(
        fx.project.join(".git/info/grafts"),
        format!("{sha} {parent} {side}\n"),
    )
    .unwrap();
    let swap = git(
        &fx.project,
        &[
            "commit-tree",
            &tree,
            "-p",
            &parent,
            "-p",
            &side,
            "-m",
            "swap",
        ],
    );
    git(&fx.project, &["replace", &sha, &swap]);

    let plan = plan_of(&fx, Action::Push);
    assert_eq!(plan.blocked, None);
    assert_eq!(plan.command, push_command(&sha));
    let done = push(&fx, true, Some(&push_command(&sha))).unwrap();
    assert_eq!(done.exit, 0, "{}", done.output);
    assert_eq!(on_origin(&fx, "work/one").as_deref(), Some(sha.as_str()));
    assert!(!origin_has(&fx, &side), "the side commit was sent");
    assert!(!origin_has(&fx, &swap), "the replacement was sent");
}

// -- gate 4: nothing in the folder was left out --------------------------------

#[test]
fn a_file_changed_after_the_commit_is_in_the_way_of_a_push() {
    let fx = fixture("left");
    let sha = committed(&fx);
    write(&fx, "src/a.txt", "two\n");
    refused_with(
        &fx,
        &sha,
        "Files changed since the commit. Commit them, or put them back, before a push.",
    );
    // Putting the file back makes the push go.
    write(&fx, "src/a.txt", "one\n");
    assert_eq!(plan_of(&fx, Action::Push).blocked, None);
}

// -- gate 5: the review it was made from, every blocker waived -----------------

#[test]
fn a_review_that_is_gone_another_or_with_an_open_blocker_is_refused() {
    let fx = fixture("review");
    let sha = committed(&fx);
    let dir = fx.cfg.join("helper-work").join(&fx.session);
    let kept = std::fs::read_to_string(dir.join("review.json")).unwrap();

    std::fs::remove_file(dir.join("review.json")).unwrap();
    refused_with(
        &fx,
        &sha,
        "The review the commit was made from is no longer on record.",
    );

    // A review that names another HEAD than the commit or its parent.
    let mut other: Review = serde_json::from_str(&kept).unwrap();
    other.head = "1".repeat(40);
    record(&fx, "review.json", &serde_json::to_string(&other).unwrap());
    refused_with(
        &fx,
        &sha,
        "The review on record is not the one the commit was made from.",
    );
    // One of another tree.
    let mut other: Review = serde_json::from_str(&kept).unwrap();
    other.tree = "2".repeat(40);
    record(&fx, "review.json", &serde_json::to_string(&other).unwrap());
    refused_with(
        &fx,
        &sha,
        "The review on record is not the one the commit was made from.",
    );

    // The same review, with a blocker nobody waived.
    let mut open: Review = serde_json::from_str(&kept).unwrap();
    open.body = "Findings from this run\n\n- High: src/a.txt:1 the total is off by one".into();
    open.blockers = 1;
    record(&fx, "review.json", &serde_json::to_string(&open).unwrap());
    refused_with(&fx, &sha, "The review found 1 blocker.");

    // Waived, it pushes.
    open.waived = vec![0];
    record(&fx, "review.json", &serde_json::to_string(&open).unwrap());
    assert_eq!(plan_of(&fx, Action::Push).blocked, None);
}

/// Correctness M2: "Review again" after the commit reviews the commit
/// itself: its HEAD is the commit, its tree and its session commits the
/// commit's. That review is of exactly what the push sends, so it pushes;
/// before, it ended every push from the panel.
#[test]
fn a_review_of_the_commit_made_here_lets_it_push() {
    let fx = fixture("review-after");
    let sha = committed(&fx);
    reviewed(&fx, 0);
    let review = {
        let _config = ConfigGuard::set(&fx.cfg);
        review::review_for(&fx.session).unwrap().unwrap()
    };
    assert_eq!(
        (review.head.as_str(), review.commits.clone()),
        (sha.as_str(), vec![sha.clone()])
    );
    let plan = plan_of(&fx, Action::Push);
    assert_eq!(plan.blocked, None);
    assert_eq!(plan.command, push_command(&sha));
    let done = push(&fx, true, Some(&push_command(&sha))).unwrap();
    assert_eq!(done.exit, 0, "{}", done.output);
    assert_eq!(on_origin(&fx, "work/one").as_deref(), Some(sha.as_str()));

    // A blocker found in that review still stops the push.
    reviewed(&fx, 1);
    assert_eq!(
        plan_of(&fx, Action::Push).blocked.as_deref(),
        Some("The review found 1 blocker.")
    );
}

// -- gate 6: the project is in the middle of nothing ---------------------------

#[test]
fn a_project_in_the_middle_of_a_bisect_pushes_nothing() {
    let fx = fixture("busy");
    let sha = committed(&fx);
    std::fs::write(fx.project.join(".git/BISECT_LOG"), "").unwrap();
    refused_with(
        &fx,
        &sha,
        "The project is in the middle of a merge or rebase, or has unresolved conflicts. Finish \
         or abort it first.",
    );
}

// -- gate 7: no program in the repository's own config -------------------------

#[test]
fn a_program_set_in_the_repositorys_config_after_the_commit_never_runs() {
    // Attack B4: a local push runs `remote.origin.receivepack` as a program.
    let fx = fixture("b4");
    let sha = committed(&fx);
    let marker = fx.base.join("ran");
    let program = format!("touch '{}'; git-receive-pack", marker.display());
    git(
        &fx.project,
        &["config", "remote.origin.receivepack", &program],
    );
    let why = "This repository's own git config sets remote.origin.receivepack; push from your \
               terminal.";
    refused_with(&fx, &sha, why);
    // Right before the spawn it is read again, also past the panel's checks.
    assert_eq!(
        ship::ship_at(
            &fx.cfg,
            &fx.project,
            &cfg(),
            Action::Push,
            &sha,
            true,
            Some(&push_command(&sha)),
        ),
        Err(why.to_string())
    );
    assert!(!marker.exists(), "the configured program ran");
    assert_eq!(on_origin(&fx, "work/one"), None);
}

/// Trust m1: a `pushurl` set after the commit would send the confirmed push
/// of "origin" to another repository, and `pushInsteadOf` would too.
#[test]
fn a_push_address_set_in_the_repositorys_config_after_the_commit_is_refused() {
    let fx = fixture("pushurl");
    let sha = committed(&fx);
    let other = fx.base.join("other.git");
    git(&fx.base, &["init", "-q", "--bare", "other.git"]);
    git(
        &fx.project,
        &["config", "remote.origin.pushurl", other.to_str().unwrap()],
    );
    refused_with(
        &fx,
        &sha,
        "This repository's own git config sets remote.origin.pushurl; push from your terminal.",
    );
    assert!(
        !origin_has_in(&other, &sha),
        "the other repository got the commit"
    );
    git(&fx.project, &["config", "--unset", "remote.origin.pushurl"]);
    let key = format!("url.{}.pushInsteadOf", other.display());
    git(&fx.project, &["config", &key, fx.origin.to_str().unwrap()]);
    let plan = plan_of(&fx, Action::Push);
    assert_eq!(
        plan.blocked,
        Some(format!(
            "This repository's own git config sets url.{}.pushinsteadof; push from your terminal.",
            other.display()
        ))
    );
    assert!(!origin_has_in(&other, &sha));
}

/// The repository at `dir` holds an object with this id.
fn origin_has_in(dir: &Path, sha: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["cat-file", "-e", sha])
        .status()
        .unwrap()
        .success()
}

#[test]
fn a_push_sends_no_tag_and_runs_no_hook_whatever_the_repositorys_config_says() {
    // M6: `push.followTags` in the project's config would send the tag.
    let fx = fixture("m6");
    let sha = committed(&fx);
    git(&fx.project, &["config", "push.followTags", "true"]);
    git(&fx.project, &["tag", "-a", "v1", "-m", "v1"]);
    let hooks = fx.base.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let marker = fx.base.join("hook-ran");
    let hook = hooks.join("pre-push");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    git(
        &fx.project,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );

    let done = push(&fx, true, Some(&push_command(&sha))).unwrap();
    assert_eq!(done.exit, 0, "{}", done.output);
    assert_eq!(on_origin(&fx, "work/one").as_deref(), Some(sha.as_str()));
    assert_eq!(git(&fx.origin, &["tag", "--list"]), "");
    assert!(!marker.exists(), "the pre-push hook ran");
}

// -- gates 8 and 9: always asked, and a pull request after the push ------------

const GITHUB_NO_PR: Probe = Some((Host::GitHub, Pr::None));

#[test]
fn a_pull_request_waits_for_the_push_and_always_asks_even_in_auto_beta() {
    let fx = fixture("pr");
    let sha = committed(&fx);
    let auto = CodeShip {
        mode: ShipMode::AutoBeta,
        ..cfg()
    };
    let pr_plan = || {
        plan_with(
            &fx.cfg,
            &fx.project,
            &fx.session,
            &auto,
            Action::Pr,
            GITHUB_NO_PR,
        )
    };
    let pr_run = |confirmed: bool, command: Option<&str>| {
        push_probed(
            &fx.cfg,
            &fx.project,
            &fx.session,
            &auto,
            Action::Pr,
            confirmed,
            command,
            GITHUB_NO_PR,
        )
    };
    let not_pushed = "Push the commit first: the pull request is opened for it.";
    assert_eq!(pr_plan().blocked.as_deref(), Some(not_pushed));
    assert_eq!(pr_run(false, None), Err(not_pushed.to_string()));

    // Auto-beta on GitHub, and the push still asks.
    let push_plan = plan_with(
        &fx.cfg,
        &fx.project,
        &fx.session,
        &auto,
        Action::Push,
        GITHUB_NO_PR,
    );
    assert!(push_plan.needs_confirm);
    assert_eq!(push_plan.mode, ShipMode::Ask);
    assert_eq!(
        push_probed(
            &fx.cfg,
            &fx.project,
            &fx.session,
            &auto,
            Action::Push,
            false,
            None,
            GITHUB_NO_PR,
        ),
        Err("This needs a confirmation first.".to_string())
    );
    let done = push_probed(
        &fx.cfg,
        &fx.project,
        &fx.session,
        &auto,
        Action::Push,
        true,
        Some(&push_command(&sha)),
        GITHUB_NO_PR,
    )
    .unwrap();
    assert_eq!(done.exit, 0, "{}", done.output);
    assert_eq!(code_activity::list(&fx.cfg, 1)[0].approved_by, "user");

    let plan = pr_plan();
    assert_eq!(plan.blocked, None);
    assert_eq!(
        plan.command,
        "gh pr create --base beta --head work/one --fill"
    );
    assert!(plan.needs_confirm);
    assert_eq!(plan.mode, ShipMode::Ask);
    // Without the yes, gh is never started.
    assert_eq!(
        pr_run(false, None),
        Err("This needs a confirmation first.".to_string())
    );
    assert_eq!(
        pr_run(
            true,
            Some("gh pr create --base main --head work/one --fill")
        ),
        Err(
            "The branch changed since you confirmed. Check the command and confirm again."
                .to_string()
        )
    );
}

/// Trust M1: after the push, a commit nobody reviewed is put on origin's
/// branch from a side ref; HEAD here is still the commit made here and
/// nothing is left to commit. The pull request would carry that commit, so
/// the yes is refused before gh starts.
#[test]
fn a_pull_request_is_refused_when_origins_branch_is_not_the_commit_made_here() {
    let fx = fixture("pr-remote");
    let sha = committed(&fx);
    let auto = cfg();
    let pr_command = "gh pr create --base beta --head work/one --fill";
    let pr_run = || {
        push_probed(
            &fx.cfg,
            &fx.project,
            &fx.session,
            &auto,
            Action::Pr,
            true,
            Some(pr_command),
            GITHUB_NO_PR,
        )
    };
    let done = push(&fx, true, Some(&push_command(&sha))).unwrap();
    assert_eq!(done.exit, 0, "{}", done.output);

    // A commit on top, made by plumbing and pushed from a side ref.
    let tree = git(&fx.project, &["rev-parse", "HEAD^{tree}"]);
    let sneaked = git(
        &fx.project,
        &["commit-tree", &tree, "-p", &sha, "-m", "install.sh"],
    );
    git(&fx.project, &["update-ref", "refs/side/x", &sneaked]);
    git(
        &fx.project,
        &["push", "-q", "origin", "refs/side/x:refs/heads/work/one"],
    );
    git(&fx.project, &["update-ref", "-d", "refs/side/x"]);
    assert_eq!(head(&fx), sha);
    assert_eq!(
        on_origin(&fx, "work/one").as_deref(),
        Some(sneaked.as_str())
    );
    let why = "work/one on origin is not the commit made here; open the pull request from your \
               terminal.";
    assert_eq!(pr_run(), Err(why.to_string()));
    assert!(code_activity::list(&fx.cfg, 10)
        .iter()
        .all(|entry| entry.kind != ActKind::Pr));

    // A branch that is gone from origin is refused the same way.
    git(&fx.origin, &["update-ref", "-d", "refs/heads/work/one"]);
    assert_eq!(pr_run(), Err(why.to_string()));
}

#[test]
fn origins_branch_is_not_read_while_offline_only_is_on() {
    let fx = fixture("pr-offline");
    let sha = committed(&fx);
    let done = push(&fx, true, Some(&push_command(&sha))).unwrap();
    assert_eq!(done.exit, 0, "{}", done.output);
    assert_eq!(
        crate::ship::origin_branch(&fx.project, "work/one"),
        Ok(Some(sha.clone()))
    );
    let _offline = grokhub_core::offline::force_for_thread(true);
    let refused = crate::ship::origin_branch(&fx.project, "work/one").unwrap_err();
    assert!(grokhub_core::offline::is_refusal(&refused), "{refused}");
}
