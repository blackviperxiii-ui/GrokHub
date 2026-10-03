// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Read-only `gh` invocations, exact-matched. Anything else fails closed to the judge.

/// First `n` non-flag tokens after the head.
/// Space-separated flag values are not modeled; one landing here can only make a match fail, never allow more.
fn nonflag_tokens(inner: &[String], n: usize) -> Vec<&str> {
    inner
        .iter()
        .skip(1)
        .filter(|word| !word.starts_with('-'))
        .take(n)
        .map(String::as_str)
        .collect()
}

/// Read-only `gh` invocations, exact-matched; anything else (`pr merge`, `api`, aliases) fails closed to the model.
pub(super) fn gh_subcommand_is_read_only(inner: &[String]) -> bool {
    let toks = nonflag_tokens(inner, 2);
    match toks.as_slice() {
        [group, sub] => matches!(
            (*group, *sub),
            (
                "pr" | "issue" | "release" | "run" | "workflow" | "repo" | "gist",
                "view" | "list" | "status" | "checks" | "diff"
            ) | ("auth", "status")
        ),
        ["status"] => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::gh_subcommand_is_read_only;

    fn words(cmd: &str) -> Vec<String> {
        cmd.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn read_only_gh_subcommands_match() {
        for cmd in [
            "gh pr view 42 --json state",
            "gh pr checks 42",
            "gh pr list --limit 10",
            "gh pr diff 1234",
            "gh run view 42 --log",
            "gh issue list",
            "gh release list",
            "gh repo view example/repo",
            "gh auth status",
            "gh status",
            "gh pr status",
            "GH pr view 1",
            "/usr/bin/gh pr view 1",
        ] {
            assert!(gh_subcommand_is_read_only(&words(cmd)), "{cmd}");
        }
    }

    #[test]
    fn mutating_gh_subcommands_fail_closed() {
        for cmd in [
            "gh pr merge 42 --squash",
            "gh pr create --title x",
            "gh pr close 1",
            "gh repo delete example/repo",
            "gh api repos/example/repo --method DELETE",
            "gh release create v1",
            "gh workflow run deploy.yml",
            "gh alias set co 'pr checkout'",
            "gh",
            "gh pr --repo owner/x view",
        ] {
            assert!(!gh_subcommand_is_read_only(&words(cmd)), "{cmd}");
        }
    }
}
