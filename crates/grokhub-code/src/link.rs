//! Link a Code project to a local git folder or clone a remote URL.
//! Auth uses the OS credential helper / saved GitHub PAT; tokens never return
//! to the UI. Remote fetch failures leave existing local work alone.

use grokhub_core::proc_util::hide_windows_console;
use grokhub_core::redact::redact_secrets;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Result of linking or refreshing a project's git source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    pub path: String,
    pub remote_url: Option<String>,
    pub preferred_branch: String,
    /// Soft note when clone/fetch failed but local work stayed.
    pub note: Option<String>,
}

/// True when `url` looks like a git remote (HTTPS, SSH, or `git@`).
pub fn looks_like_remote(url: &str) -> bool {
    let u = url.trim();
    if u.is_empty() || u.contains('\0') || u.contains('\n') {
        return false;
    }
    u.starts_with("https://")
        || u.starts_with("http://")
        || u.starts_with("ssh://")
        || u.starts_with("git@")
        || u.starts_with("git://")
}

/// Repo folder name from a remote URL (`owner/name` → `name`).
pub fn repo_name_from_url(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let last = u.rsplit(['/', ':']).next()?.trim();
    if last.is_empty() || last.contains(' ') {
        return None;
    }
    let safe: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_matches('.').trim_matches('-');
    (!safe.is_empty()).then(|| safe.to_string())
}

/// Does this folder have a local or remote `beta` ref?
pub fn has_beta(dir: &Path) -> bool {
    ref_exists(dir, "refs/heads/beta") || ref_exists(dir, "refs/remotes/origin/beta")
}

fn ref_exists(dir: &Path, refname: &str) -> bool {
    git_ok(dir, &["show-ref", "--verify", "--quiet", refname])
}

/// Preferred working branch for a linked repo.
pub fn preferred_branch(dir: &Path, override_branch: Option<&str>) -> String {
    if let Some(b) = override_branch.map(str::trim).filter(|s| !s.is_empty()) {
        return b.to_string();
    }
    if has_beta(dir) {
        return "beta".into();
    }
    if let Some(b) = crate::ship::default_branch(dir) {
        return b;
    }
    crate::ship::current_branch(dir).unwrap_or_else(|| "main".into())
}

/// `origin` URL when present; never rewritten with credentials.
pub fn remote_url(dir: &Path) -> Option<String> {
    crate::repo::origin_url(dir)
}

/// Clone `url` into `dest` (parent must exist). `github_token` is used only for
/// HTTPS GitHub clones and never logged. Existing `dest` is left alone on failure.
pub fn clone_remote(url: &str, dest: &Path, github_token: Option<&str>) -> Result<(), String> {
    grokhub_core::offline::refuse()?;
    if dest.exists() {
        return Err(format!(
            "That folder already exists: {}. Open it instead of cloning again.",
            dest.display()
        ));
    }
    let parent = dest.parent().ok_or_else(|| "Bad clone path.".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;

    let mut c = Command::new("git");
    c.arg("-c")
        .arg("core.fsmonitor=false")
        .args(["clone", "--", &clone_url(url)])
        .arg(dest)
        .envs(github_auth_env(github_token))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    hide_windows_console(&mut c);
    let out = c
        .output()
        .map_err(|_| "git isn't installed or couldn't start".to_string())?;
    if out.status.success() {
        return Ok(());
    }
    // Clone failed: remove a partial dir so local work elsewhere stays clean.
    let _ = std::fs::remove_dir_all(dest);
    let err = String::from_utf8_lossy(&out.stderr);
    let msg = redact_secrets(err.trim());
    Err(if msg.is_empty() {
        "git clone failed.".into()
    } else {
        format!("git clone failed: {msg}")
    })
}

/// Soft checkout of `branch` when it exists locally or on origin. Never deletes
/// local work; returns a note when checkout was skipped.
pub fn checkout_preferred(dir: &Path, branch: &str) -> Option<String> {
    let branch = branch.trim();
    if branch.is_empty() {
        return None;
    }
    if crate::ship::current_branch(dir).as_deref() == Some(branch) {
        return None;
    }
    if ref_exists(dir, &format!("refs/heads/{branch}")) {
        return if git_ok(dir, &["checkout", "--", branch]) || git_ok(dir, &["checkout", branch]) {
            None
        } else {
            Some(format!(
                "Could not check out {branch}; local work was left as-is."
            ))
        };
    }
    if ref_exists(dir, &format!("refs/remotes/origin/{branch}")) {
        return if git_ok(
            dir,
            &[
                "checkout",
                "-b",
                branch,
                "--track",
                &format!("origin/{branch}"),
            ],
        ) {
            None
        } else {
            Some(format!(
                "Could not create local {branch} from origin; local work was left as-is."
            ))
        };
    }
    Some(format!(
        "Branch {branch} was not found yet; staying on the current branch."
    ))
}

/// Read link metadata for an existing folder (no checkout, no network).
pub fn inspect_local(dir: &Path, override_branch: Option<&str>) -> Linked {
    let path = dir.display().to_string();
    let remote = remote_url(dir);
    let preferred = preferred_branch(dir, override_branch);
    let note = if !(dir.join(".git").exists() || dir.join(".git").is_file()) {
        Some("This folder is not a git repo yet; linked as a local project.".into())
    } else {
        None
    };
    Linked {
        path,
        remote_url: remote,
        preferred_branch: preferred,
        note,
    }
}

/// Link an existing folder and soft-check out the preferred branch.
pub fn link_local(dir: &Path, override_branch: Option<&str>) -> Linked {
    let mut linked = inspect_local(dir, override_branch);
    if linked.note.is_none() {
        linked.note = checkout_preferred(dir, &linked.preferred_branch);
    }
    linked
}

/// Clone (or reuse) a remote into `work_root` and return the link result.
pub fn link_remote(
    url: &str,
    work_root: &Path,
    github_token: Option<&str>,
    override_branch: Option<&str>,
) -> Result<Linked, String> {
    let url = url.trim();
    if !looks_like_remote(url) {
        return Err("That doesn't look like a git URL.".into());
    }
    let name = repo_name_from_url(url)
        .ok_or_else(|| "Could not read a repo name from that URL.".to_string())?;
    let dest = unique_dest(work_root, &name);
    if dest.exists() {
        // Reuse: never wipe. Refresh link metadata only.
        let linked = link_local(&dest, override_branch);
        return Ok(Linked {
            remote_url: linked.remote_url.or_else(|| Some(url.to_string())),
            note: Some(format!(
                "Opened the existing folder {}. Local work was kept.",
                dest.display()
            ))
            .or(linked.note),
            ..linked
        });
    }
    clone_remote(url, &dest, github_token)?;
    let mut linked = link_local(&dest, override_branch);
    if linked.remote_url.is_none() {
        linked.remote_url = Some(sanitize_remote_url(url));
    }
    Ok(linked)
}

fn unique_dest(work_root: &Path, name: &str) -> PathBuf {
    let base = work_root.join(name);
    if !base.exists() {
        return base;
    }
    for i in 2..100 {
        let cand = work_root.join(format!("{name}-{i}"));
        if !cand.exists() {
            return cand;
        }
    }
    work_root.join(format!("{name}-{}", grokhub_core::now_ms()))
}

/// Strip credentials from a URL before storing or showing.
pub fn sanitize_remote_url(url: &str) -> String {
    let u = url.trim();
    if let Some(rest) = u.strip_prefix("https://") {
        if let Some(at) = rest.find('@') {
            return format!("https://{}", &rest[at + 1..]);
        }
    }
    if let Some(rest) = u.strip_prefix("http://") {
        if let Some(at) = rest.find('@') {
            return format!("http://{}", &rest[at + 1..]);
        }
    }
    u.to_string()
}

/// The URL a clone uses: no credentials, and `www.github.com` as
/// `github.com` so the token header below applies to it.
fn clone_url(url: &str) -> String {
    let u = sanitize_remote_url(url);
    match u.strip_prefix("https://www.github.com/") {
        Some(rest) => format!("https://github.com/{rest}"),
        None => u,
    }
}

/// Lends the GitHub token to one git call as an HTTPS header for github.com
/// only, through git's environment config. A token in the clone URL would be
/// saved in the clone's `.git/config` (where any tool in the folder reads it)
/// and shown in the process list. SSH and other hosts get nothing and keep
/// the user's own credential helper.
fn github_auth_env(github_token: Option<&str>) -> Vec<(&'static str, String)> {
    let Some(token) = github_token.map(str::trim).filter(|t| !t.is_empty()) else {
        return Vec::new();
    };
    let basic = grokhub_core::encode_b64(format!("x-access-token:{token}").as_bytes());
    vec![
        ("GIT_CONFIG_COUNT", "1".to_string()),
        (
            "GIT_CONFIG_KEY_0",
            "http.https://github.com/.extraheader".to_string(),
        ),
        (
            "GIT_CONFIG_VALUE_0",
            format!("AUTHORIZATION: basic {basic}"),
        ),
    ]
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    let mut c = Command::new("git");
    c.arg("-c")
        .arg("core.fsmonitor=false")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    hide_windows_console(&mut c);
    c.status().map(|s| s.success()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_is_refused_offline_and_makes_no_folder() {
        let _offline = grokhub_core::offline::force_for_thread(true);
        let tmp = std::env::temp_dir().join(format!("link-offline-{}", std::process::id()));
        let dest = tmp.join("app");
        let err = clone_remote("https://github.com/org/app.git", &dest, None).unwrap_err();
        assert_eq!(err, *grokhub_core::offline::OFFLINE_MSG);
        assert!(!tmp.exists());
    }

    #[test]
    fn remote_url_parsing_and_sanitize() {
        assert!(looks_like_remote("https://github.com/org/app.git"));
        assert!(looks_like_remote("git@github.com:org/app.git"));
        assert!(!looks_like_remote("/home/user/app"));
        assert_eq!(
            repo_name_from_url("https://github.com/org/my-app.git").as_deref(),
            Some("my-app")
        );
        assert_eq!(
            sanitize_remote_url("https://x-access-token:secret@github.com/org/app.git"),
            "https://github.com/org/app.git"
        );
        assert_eq!(
            clone_url("https://x-access-token:secret@www.github.com/org/app.git"),
            "https://github.com/org/app.git"
        );
        assert_eq!(
            clone_url("https://gitlab.com/org/app.git"),
            "https://gitlab.com/org/app.git"
        );
    }

    #[test]
    fn the_token_goes_to_github_as_a_header_never_in_the_url() {
        assert_eq!(
            github_auth_env(Some(" sk-abcdefghijklmnopqrstuv ")),
            vec![
                ("GIT_CONFIG_COUNT", "1".to_string()),
                (
                    "GIT_CONFIG_KEY_0",
                    "http.https://github.com/.extraheader".to_string()
                ),
                (
                    "GIT_CONFIG_VALUE_0",
                    "AUTHORIZATION: basic eC1hY2Nlc3MtdG9rZW46c2stYWJjZGVmZ2hpamtsbW5vcHFyc3R1dg=="
                        .to_string()
                ),
            ]
        );
        assert!(github_auth_env(None).is_empty());
        assert!(github_auth_env(Some("  ")).is_empty());
    }

    #[test]
    fn a_clone_with_a_token_saves_no_token_in_its_config() {
        // A local bare repo stands in for GitHub; the header is only lent.
        let tmp = std::env::temp_dir().join(format!("link-token-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let origin = tmp.join("origin.git");
        std::fs::create_dir_all(&origin).unwrap();
        assert!(git_ok(&origin, &["init", "--bare", "-q"]));
        let dest = tmp.join("app");
        clone_remote(
            origin.to_str().unwrap(),
            &dest,
            Some("sk-abcdefghijklmnopqrstuv"),
        )
        .unwrap();
        let config = std::fs::read_to_string(dest.join(".git/config")).unwrap();
        assert!(!config.contains("sk-abcdefghijklmnopqrstuv"), "{config}");
        assert!(!config.contains("extraheader"), "{config}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
