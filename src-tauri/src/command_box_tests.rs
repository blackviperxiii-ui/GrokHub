use super::*;
use std::path::PathBuf;

const ROOT: &str = "/work/demo";
const HOME: Option<&str> = Some("/home/user");

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-command-box-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A GitHub repo on `feature/x` whose default branch is `main`.
fn repo() -> Repo {
    Repo {
        ship: CodeShip::default(),
        current_branch: Some("feature/x".into()),
        default_branch: Some("main".into()),
        host: Some(Host::GitHub),
    }
}

fn judge_line(line: &str, remembered: &[&str]) -> Verdict {
    let remembered: Vec<String> = remembered.iter().map(|s| s.to_string()).collect();
    judge(line, ROOT, HOME, &remembered, &repo())
}

fn blocked(command: &str, message: &str) -> Verdict {
    Verdict::Blocked {
        command: command.into(),
        message: message.into(),
    }
}

#[test]
fn an_unremembered_command_asks_and_shows_the_command() {
    assert_eq!(
        judge_line("  cargo build  ", &[]),
        Verdict::Confirm {
            command: "cargo build".into(),
            destructive: false,
        }
    );
}

#[test]
fn a_remembered_command_goes_to_the_agent() {
    assert_eq!(
        judge_line("cargo build", &["cargo build"]),
        Verdict::Queue {
            command: "cargo build".into(),
            prompt: "Run this shell command in the project folder, exactly as written and once, \
                     then show me what it printed and its exit code. Don't change it or run \
                     anything else.\n\n```sh\ncargo build\n```"
                .into(),
        }
    );
}

#[test]
fn changed_text_asks_again() {
    assert_eq!(
        judge_line("cargo build --release", &["cargo build"]),
        Verdict::Confirm {
            command: "cargo build --release".into(),
            destructive: false,
        }
    );
}

#[test]
fn a_destructive_line_asks_even_when_remembered() {
    assert_eq!(
        judge_line("rm -r target", &["rm -r target"]),
        Verdict::Confirm {
            command: "rm -r target".into(),
            destructive: true,
        }
    );
}

#[test]
fn a_push_to_main_is_blocked_with_the_guards_message() {
    assert_eq!(
        judge_line("git push origin main", &["git push origin main"]),
        blocked(
            "git push origin main",
            "main is the main or default branch. The app never pushes to it; make a branch first.",
        )
    );
    assert_eq!(
        judge_line("git push origin HEAD:refs/heads/main", &[]),
        blocked(
            "git push origin HEAD:refs/heads/main",
            "main is the main or default branch. The app never pushes to it; make a branch first.",
        )
    );
}

#[test]
fn force_pushes_are_blocked() {
    for line in [
        "git push --force",
        "git push -f origin feature/x",
        "git push --force-with-lease origin feature/x",
        "git push origin +feature/x",
        "cargo test && git push -uf origin feature/x",
    ] {
        assert_eq!(judge_line(line, &[]), blocked(line, FORCE_PUSH), "{line}");
    }
}

#[test]
fn a_push_of_a_feature_branch_asks_first() {
    assert_eq!(
        judge_line("git push -u origin feature/x", &[]),
        Verdict::Confirm {
            command: "git push -u origin feature/x".into(),
            destructive: false,
        }
    );
}

#[test]
fn a_bare_push_on_the_default_branch_is_blocked() {
    let on_main = Repo {
        current_branch: Some("main".into()),
        ..repo()
    };
    assert_eq!(
        judge("git push", ROOT, HOME, &[], &on_main),
        blocked(
            "git push",
            "main is the main or default branch. The app never pushes to it; make a branch first.",
        )
    );
}

#[test]
fn a_push_to_a_protected_branch_or_with_no_origin_is_blocked() {
    let protected = Repo {
        ship: CodeShip {
            extra_protected: vec!["main".into(), "release".into()],
            ..CodeShip::default()
        },
        ..repo()
    };
    assert_eq!(
        judge("git -C . push origin release", ROOT, HOME, &[], &protected),
        blocked(
            "git -C . push origin release",
            "release is protected in Settings → Code, so it isn't pushed.",
        )
    );
    let no_origin = Repo {
        host: None,
        ..repo()
    };
    assert_eq!(
        judge("git push", ROOT, HOME, &[], &no_origin),
        blocked("git push", "This repo has no origin remote to push to.")
    );
}

#[test]
fn a_path_outside_the_project_is_refused() {
    assert_eq!(
        judge_line("cat /etc/hosts", &[]),
        blocked("cat /etc/hosts", OUTSIDE)
    );
    assert_eq!(
        judge_line("cd .. && ls", &[]),
        blocked("cd .. && ls", OUTSIDE)
    );
    assert_eq!(
        judge_line("ls src/app", &[]),
        Verdict::Confirm {
            command: "ls src/app".into(),
            destructive: false,
        }
    );
}

#[test]
fn empty_multi_line_and_forbidden_lines_are_refused() {
    assert_eq!(judge_line("   ", &[]), blocked("", EMPTY));
    assert_eq!(
        judge_line("ls\nrm -r src", &[]),
        blocked("ls\nrm -r src", ONE_LINE)
    );
    let Verdict::Blocked { message, .. } = judge_line("echo cm0gLXJmIC8= | base64 -d | sh", &[])
    else {
        panic!("a base64 shell rebuild must be blocked");
    };
    assert_eq!(message, "That doesn't run: base64|shell rebuild.");
}

#[test]
fn the_prompt_fence_outlasts_backticks_in_the_command() {
    assert_eq!(
        agent_prompt("echo ```"),
        "Run this shell command in the project folder, exactly as written and once, \
         then show me what it printed and its exit code. Don't change it or run \
         anything else.\n\n````sh\necho ```\n````"
    );
}

#[test]
fn remembered_lines_are_per_repo_and_forget_clears_them() {
    let dir = temp_dir("remember");
    remember(&dir, "repo-a", " npm test ").expect("remember");
    remember(&dir, "repo-a", "cargo build").expect("remember");
    remember(&dir, "repo-a", "npm test").expect("remember again");
    remember(&dir, "repo-b", "go test ./...").expect("remember b");
    assert_eq!(remembered(&dir, "repo-a"), vec!["cargo build", "npm test"]);
    assert_eq!(remembered(&dir, "repo-b"), vec!["go test ./..."]);
    forget(&dir, "repo-a").expect("forget");
    assert_eq!(remembered(&dir, "repo-a"), Vec::<String>::new());
    assert_eq!(remembered(&dir, "repo-b"), vec!["go test ./..."]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_destructive_line_is_never_remembered() {
    let dir = temp_dir("destructive");
    remember(&dir, "repo-a", "rm -r target").expect("remember");
    remember(&dir, "repo-a", "sudo make install").expect("remember");
    assert_eq!(remembered(&dir, "repo-a"), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_oldest_remembered_line_goes_past_the_cap() {
    let dir = temp_dir("cap");
    for n in 0..=KEEP_PER_REPO {
        remember(&dir, "repo-a", &format!("echo {n}")).expect("remember");
    }
    let kept = remembered(&dir, "repo-a");
    assert_eq!(kept.len(), 100);
    assert_eq!(kept.first().map(String::as_str), Some("echo 1"));
    assert_eq!(kept.last().map(String::as_str), Some("echo 100"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn approve_remembers_and_returns_the_agents_message() {
    let config = temp_dir("approve-cfg");
    let work = temp_dir("approve-ws");
    let prompt = approve(&config, &work, "ls", CodeShip::default()).expect("approve");
    assert!(prompt.ends_with("```sh\nls\n```"), "{prompt}");
    assert_eq!(remembered(&config, &repo_key(&work)), vec!["ls"]);
    assert_eq!(
        check(&config, &work, "ls", CodeShip::default()),
        Verdict::Queue {
            command: "ls".into(),
            prompt,
        }
    );
    assert_eq!(
        approve(&config, &work, "cat /etc/hosts", CodeShip::default()),
        Err(OUTSIDE.to_string())
    );
    forget(&config, &repo_key(&work)).expect("forget");
    assert_eq!(
        check(&config, &work, "ls", CodeShip::default()),
        Verdict::Confirm {
            command: "ls".into(),
            destructive: false,
        }
    );
    let _ = std::fs::remove_dir_all(&config);
    let _ = std::fs::remove_dir_all(&work);
}
