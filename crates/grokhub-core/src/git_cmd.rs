//! The app's own git calls. They run outside the agent's sandbox, so they
//! never run the repository's hooks or its fsmonitor: a sandboxed command
//! can write `.git/hooks` in a workspace, and the app runs git there
//! (worktrees for subagents, checkpoints, update checks).

use std::process::Command;

/// The null device, which git reads as "no hooks folder".
#[cfg(windows)]
const NO_HOOKS: &str = "core.hooksPath=NUL";
#[cfg(not(windows))]
const NO_HOOKS: &str = "core.hooksPath=/dev/null";

/// The arguments that go before any git subcommand: hooks off and
/// fsmonitor off (`git status` runs a configured fsmonitor command).
pub const SAFE_ARGS: [&str; 4] = ["-c", NO_HOOKS, "-c", "core.fsmonitor=false"];

/// `git` with hooks and fsmonitor off. Add arguments as usual.
pub fn git() -> Command {
    let mut cmd = Command::new("git");
    cmd.args(SAFE_ARGS);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_and_fsmonitor_are_off() {
        let cmd = git();
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(cmd.get_program(), "git");
        assert_eq!(args[0], "-c");
        assert!(args[1].starts_with("core.hooksPath="));
        assert_eq!(args[2..], ["-c", "core.fsmonitor=false"]);
    }

    /// The real thing: a planted post-checkout hook doesn't run.
    #[cfg(unix)]
    #[test]
    fn a_planted_hook_does_not_run() {
        use std::os::unix::fs::PermissionsExt;
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("grokhub-git-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            git()
                .current_dir(&dir)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .output()
                .unwrap()
        };
        assert!(run(&["init", "-q"]).status.success());
        assert!(run(&["commit", "-q", "--allow-empty", "-m", "x"])
            .status
            .success());
        let marker = dir.join("hook-ran");
        let hook = dir.join(".git/hooks/post-checkout");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let wt = dir.join("wt");
        let out = run(&["worktree", "add", "-q", "--detach", wt.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!marker.exists(), "the hook ran");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
