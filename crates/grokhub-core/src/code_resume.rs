//! "Pick up where you left off" for code projects. Opt-in: the user picks the
//! code folders. Reads git metadata only (branch, changed-path count,
//! ahead/behind, last commit time, merged branch names) through `git`
//! itself; never diffs, file contents or author emails.

use std::path::{Path, PathBuf};

use crate::chips::{ChipKind, QuickChip};

/// Repos read under one picked folder.
pub const REPOS_PER_FOLDER: usize = 40;
/// A merged branch this old is offered for tidying.
pub const STALE_DAYS: u64 = 30;
const DAY_S: u64 = 24 * 60 * 60;

/// What git says about one repo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoStatus {
    pub path: PathBuf,
    pub name: String,
    pub branch: String,
    pub has_upstream: bool,
    pub ahead: u32,
    pub behind: u32,
    /// Changed, staged or untracked paths (a count only).
    pub changed: u32,
    /// Unix seconds of the last commit on HEAD; 0 when none.
    pub last_commit_s: u64,
    /// Local branches merged into the base branch and untouched for
    /// [`STALE_DAYS`].
    pub stale_merged: Vec<String>,
    pub base: String,
    /// Unix seconds of the oldest commit not on the upstream; 0 when none.
    pub oldest_unpushed_s: u64,
}

/// The git calls, every one with optional locks and fsmonitor off so a read
/// never writes to the repo or runs a repo-configured program.
pub fn git_args<'a>(repo: &'a str, args: &[&'a str]) -> Vec<&'a str> {
    let mut out = vec![
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.untrackedCache=false",
        "-C",
        repo,
    ];
    out.extend_from_slice(args);
    out
}

pub const STATUS: &[&str] = &["status", "--porcelain=v2", "--branch"];
pub const LAST_COMMIT: &[&str] = &["log", "-1", "--format=%ct"];
pub const UNPUSHED_TIMES: &[&str] = &["log", "@{upstream}..HEAD", "--format=%ct"];
pub const DEFAULT_BRANCH: &[&str] = &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"];
pub const BRANCHES: &[&str] = &[
    "for-each-ref",
    "--format=%(refname:short)\t%(committerdate:unix)",
    "refs/heads",
];

/// The picked folder itself when it is a repo, else the repos directly in it.
pub fn repo_dirs(folder: &Path) -> Vec<PathBuf> {
    if folder.join(".git").exists() {
        return vec![folder.to_path_buf()];
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut repos: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(".git").exists())
        .collect();
    repos.sort();
    repos.truncate(REPOS_PER_FOLDER);
    repos
}

/// Branch, upstream, ahead/behind and changed-path count from
/// `git status --porcelain=v2 --branch`. Path names are counted, never kept.
pub fn parse_status(text: &str, repo: &mut RepoStatus) {
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            repo.branch = head.trim().to_string();
        } else if line.starts_with("# branch.upstream ") {
            repo.has_upstream = true;
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for part in ab.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    repo.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    repo.behind = n.parse().unwrap_or(0);
                }
            }
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            repo.changed += 1;
        }
    }
}

/// `origin/main` → `main`; no remote HEAD → `main` or `master`, whichever
/// exists locally.
pub fn base_branch(remote_head: Option<&str>, branches: &[(String, u64)]) -> String {
    if let Some(head) = remote_head.map(str::trim).filter(|h| !h.is_empty()) {
        return head.rsplit('/').next().unwrap_or(head).to_string();
    }
    if branches.iter().any(|(b, _)| b == "master") && !branches.iter().any(|(b, _)| b == "main") {
        "master".into()
    } else {
        "main".into()
    }
}

/// Local branches with their last commit time, from [`BRANCHES`].
pub fn parse_branches(text: &str) -> Vec<(String, u64)> {
    text.lines()
        .filter_map(|line| {
            let (name, when) = line.split_once('\t')?;
            Some((name.trim().to_string(), when.trim().parse().ok()?))
        })
        .collect()
}

/// Of `merged` (the output of `git branch --merged <base> --format=…`), the
/// branches other than the base and the current one that are older than
/// [`STALE_DAYS`].
pub fn stale_merged(
    merged: &str,
    branches: &[(String, u64)],
    base: &str,
    current: &str,
    now_s: u64,
) -> Vec<String> {
    let mut out: Vec<String> = merged
        .lines()
        .map(str::trim)
        .filter(|b| !b.is_empty() && *b != base && *b != current)
        .filter(|b| {
            branches
                .iter()
                .find(|(name, _)| name == b)
                .is_some_and(|(_, when)| now_s.saturating_sub(*when) >= STALE_DAYS * DAY_S)
        })
        .map(str::to_string)
        .collect();
    out.sort();
    out
}

/// Read one repo through `git` (`run` returns stdout on success).
pub fn read_repo(
    path: &Path,
    now_s: u64,
    run: &mut dyn FnMut(&[&str]) -> Option<String>,
) -> Option<RepoStatus> {
    let repo = path.to_string_lossy().to_string();
    let mut status = RepoStatus {
        path: path.to_path_buf(),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| repo.clone()),
        ..Default::default()
    };
    parse_status(&run(&git_args(&repo, STATUS))?, &mut status);
    status.last_commit_s = run(&git_args(&repo, LAST_COMMIT))
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let branches = parse_branches(&run(&git_args(&repo, BRANCHES)).unwrap_or_default());
    status.base = base_branch(run(&git_args(&repo, DEFAULT_BRANCH)).as_deref(), &branches);
    if status.ahead > 0 && status.has_upstream {
        status.oldest_unpushed_s = run(&git_args(&repo, UNPUSHED_TIMES))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .min()
            .unwrap_or(0);
    }
    if branches.iter().any(|(b, _)| *b == status.base) {
        let base = status.base.clone();
        let merged_args = [
            "branch",
            "--merged",
            base.as_str(),
            "--format=%(refname:short)",
        ];
        let merged = run(&git_args(&repo, &merged_args)).unwrap_or_default();
        status.stale_merged = stale_merged(&merged, &branches, &status.base, &status.branch, now_s);
    }
    Some(status)
}

impl RepoStatus {
    /// Work waiting: changed files or commits not pushed.
    pub fn pending(&self) -> bool {
        self.changed > 0 || self.ahead > 0
    }

    /// "3 unpushed commits and 2 changed files", or "" when clean.
    pub fn work_line(&self) -> String {
        let mut parts = Vec::new();
        if self.ahead > 0 {
            parts.push(count(self.ahead, "unpushed commit"));
        }
        if self.changed > 0 {
            parts.push(count(self.changed, "changed file"));
        }
        if self.behind > 0 {
            parts.push(format!("{} behind", count(self.behind, "commit")));
        }
        match parts.len() {
            0 => String::new(),
            1 => parts.remove(0),
            _ => {
                let last = parts.pop().unwrap_or_default();
                format!("{} and {last}", parts.join(", "))
            }
        }
    }

    /// The offer: "my-app is on branch feat-x with 3 unpushed commits. Want to continue?"
    pub fn offer_line(&self) -> String {
        format!(
            "{} is on branch {} with {}. Want to continue?",
            self.name,
            self.branch,
            self.work_line()
        )
    }

    /// One-taps: continue as an agent task, show status, open in an editor.
    pub fn chips(&self, editor: Option<&str>) -> Vec<QuickChip> {
        let path = self.path.to_string_lossy();
        let id = |what: &str| format!("resume:{what}:{}", self.name);
        let chip = |id: String, label: String, value: String, kind: ChipKind| QuickChip {
            id,
            label,
            value,
            kind,
            score: 1.0,
            hint: self.offer_line(),
            primary: false,
        };
        let mut out = vec![
            chip(
                id("continue"),
                format!("Continue {}", self.branch),
                format!(
                    "Pick up my work in {path} on branch {}: {}. Look at git status and the recent commits there, then suggest the next step.",
                    self.branch,
                    self.work_line()
                ),
                ChipKind::Chat,
            ),
            chip(
                id("status"),
                format!("{} status", self.name),
                format!("git -C \"{path}\" status -sb"),
                ChipKind::Shell,
            ),
        ];
        if let Some(editor) = editor {
            out.push(chip(
                id("open"),
                format!("Open {} in editor", self.name),
                format!("{editor} \"{path}\""),
                ChipKind::Shell,
            ));
        }
        out
    }

    /// The tidy offer for stale merged branches. Deleting goes through the
    /// agent, so it still asks before it runs.
    pub fn tidy_chip(&self) -> Option<QuickChip> {
        if self.stale_merged.is_empty() {
            return None;
        }
        let n = self.stale_merged.len();
        Some(QuickChip {
            id: format!("resume:tidy:{}", self.name),
            label: format!("Tidy {} in {}", count(n as u32, "merged branch"), self.name),
            value: format!(
                "In {}, these local branches are merged into {} and untouched for {STALE_DAYS}+ days: {}. Delete them with git branch -d, asking me first.",
                self.path.to_string_lossy(),
                self.base,
                self.stale_merged.join(", ")
            ),
            kind: ChipKind::Chat,
            score: 0.8,
            hint: format!("Merged into {} over {STALE_DAYS} days ago", self.base),
            primary: false,
        })
    }
}

fn count(n: u32, what: &str) -> String {
    match (n, what) {
        (1, _) => format!("1 {what}"),
        (_, w) if w.ends_with("ch") => format!("{n} {w}es"),
        _ => format!("{n} {what}s"),
    }
}

/// The repo to offer: pending work, most recently committed first.
pub fn pick(repos: &[RepoStatus]) -> Option<&RepoStatus> {
    repos
        .iter()
        .filter(|r| r.pending() && !r.branch.is_empty() && r.branch != "(detached)")
        .max_by_key(|r| r.last_commit_s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn status(text: &str) -> RepoStatus {
        let mut repo = RepoStatus::default();
        parse_status(text, &mut repo);
        repo
    }

    #[test]
    fn porcelain_v2_gives_branch_counts_and_no_paths() {
        let repo = status(
            "# branch.oid 1111111111111111111111111111111111111111\n\
             # branch.head feat-x\n\
             # branch.upstream origin/feat-x\n\
             # branch.ab +3 -1\n\
             1 .M N... 100644 100644 100644 aaa bbb src/secret-plan.rs\n\
             ? notes.txt\n",
        );
        assert_eq!(
            (
                repo.branch.as_str(),
                repo.has_upstream,
                repo.ahead,
                repo.behind,
                repo.changed
            ),
            ("feat-x", true, 3, 1, 2)
        );
        assert!(!format!("{repo:?}").contains("secret-plan"));
        let clean = status("# branch.oid abc\n# branch.head main\n");
        assert_eq!(
            (clean.ahead, clean.changed, clean.has_upstream),
            (0, 0, false)
        );
        assert!(!clean.pending());
    }

    #[test]
    fn the_offer_names_branch_and_work_and_offers_one_taps() {
        let repo = RepoStatus {
            path: PathBuf::from("/home/user/code/my-app"),
            name: "my-app".into(),
            branch: "feat-x".into(),
            ahead: 3,
            changed: 1,
            ..Default::default()
        };
        assert_eq!(
            repo.offer_line(),
            "my-app is on branch feat-x with 3 unpushed commits and 1 changed file. Want to continue?"
        );
        let chips = repo.chips(Some("code"));
        let labels: Vec<&str> = chips.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["Continue feat-x", "my-app status", "Open my-app in editor"]
        );
        assert_eq!(
            chips[1].value,
            "git -C \"/home/user/code/my-app\" status -sb"
        );
        assert_eq!(chips[1].kind, ChipKind::Shell);
        assert_eq!(chips[2].value, "code \"/home/user/code/my-app\"");
        assert_eq!(repo.chips(None).len(), 2);
        assert_eq!(repo.tidy_chip(), None);
    }

    #[test]
    fn pick_takes_the_latest_repo_with_work() {
        let r = |name: &str, ahead: u32, changed: u32, at: u64| RepoStatus {
            name: name.into(),
            branch: "main".into(),
            ahead,
            changed,
            last_commit_s: at,
            ..Default::default()
        };
        let repos = vec![r("old", 1, 0, 10), r("clean", 0, 0, 99), r("new", 0, 2, 50)];
        assert_eq!(pick(&repos).map(|r| r.name.as_str()), Some("new"));
        assert_eq!(pick(&[r("clean", 0, 0, 1)]), None);
    }

    fn git(dir: &Path, args: &[&str], date: Option<&str>) {
        let mut c = Command::new("git");
        c.args([
            "-c",
            "user.name=user",
            "-c",
            "user.email=user@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        );
        if let Some(d) = date {
            c.env("GIT_COMMITTER_DATE", d).env("GIT_AUTHOR_DATE", d);
        }
        let out = c.output().expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn run_git(args: &[&str]) -> Option<String> {
        let out = Command::new("git")
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[test]
    fn fixture_repos_read_dirty_ahead_behind_and_stale_merged() {
        let root = std::env::temp_dir().join(format!("grokhub-code-resume-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("my-app");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(root.join("not-a-repo")).unwrap();
        let old = "2001-01-01T00:00:00Z";
        git(&repo, &["init", "-q"], None);
        std::fs::write(repo.join("a.txt"), "secret contents").unwrap();
        git(&repo, &["add", "."], Some(old));
        git(&repo, &["commit", "-q", "-m", "one"], Some(old));
        git(&repo, &["branch", "old-fix"], None);
        git(&repo, &["checkout", "-q", "-b", "feat-x"], None);
        git(&repo, &["branch", "-q", "--set-upstream-to=main"], None);
        for i in 0..3 {
            git(
                &repo,
                &["commit", "-q", "--allow-empty", "-m", &format!("w{i}")],
                None,
            );
        }
        git(&repo, &["checkout", "-q", "main"], None);
        git(
            &repo,
            &["commit", "-q", "--allow-empty", "-m", "on main"],
            None,
        );
        git(&repo, &["checkout", "-q", "feat-x"], None);
        std::fs::write(repo.join("a.txt"), "changed secret contents").unwrap();
        std::fs::write(repo.join("b.txt"), "new").unwrap();

        assert_eq!(repo_dirs(&root), vec![repo.clone()]);
        assert_eq!(repo_dirs(&repo), vec![repo.clone()]);
        let now_s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut calls = Vec::new();
        let got = read_repo(&repo, now_s, &mut |args| {
            calls.push(args.join(" "));
            run_git(args)
        })
        .expect("status");
        assert_eq!(
            (
                got.branch.as_str(),
                got.ahead,
                got.behind,
                got.changed,
                got.has_upstream
            ),
            ("feat-x", 3, 1, 2, true)
        );
        assert_eq!(got.base, "main");
        assert!(got.oldest_unpushed_s > 0 && got.oldest_unpushed_s <= got.last_commit_s);
        assert_eq!(got.stale_merged, vec!["old-fix".to_string()]);
        assert!(got.last_commit_s > 0);
        assert!(!format!("{got:?}").contains("secret"));
        assert!(
            calls.iter().all(
                |c| c.starts_with("--no-optional-locks -c core.fsmonitor=false")
                    && !c.contains(" diff")
                    && !c.contains(" show")
                    && !c.contains("%ae")
            ),
            "{calls:?}"
        );
        let tidy = got.tidy_chip().expect("tidy");
        assert_eq!(tidy.label, "Tidy 1 merged branch in my-app");
        assert!(tidy.value.contains("old-fix") && tidy.value.contains("asking me first"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
