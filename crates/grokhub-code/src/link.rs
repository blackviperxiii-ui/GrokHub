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

/// `origin`'s URL when present, without the credentials it may carry (set
/// by hand, or added by an `insteadOf` rule), so it can be shown and saved.
pub fn remote_url(dir: &Path) -> Option<String> {
    crate::repo::origin_url(dir).map(|url| sanitize_remote_url(&url))
}

/// The URL `origin` is set to in `dir`'s own config, as cloned (before any
/// `insteadOf` rule rewrites it).
fn configured_origin(dir: &Path) -> Option<String> {
    let mut c = Command::new("git");
    c.arg("-c")
        .arg("core.fsmonitor=false")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    hide_windows_console(&mut c);
    let out = c.output().ok().filter(|o| o.status.success())?;
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!url.is_empty()).then_some(url)
}

/// One repo's remote, whatever form names it: no scheme, credentials,
/// `.git` or trailing slash, in lower case (`git@github.com:Org/App.git`
/// and `https://github.com/org/app` are the same remote).
fn remote_key(url: &str) -> String {
    let u = sanitize_remote_url(url).to_ascii_lowercase();
    let rest = match u.split_once("://") {
        Some((_, rest)) => rest.to_string(),
        // `git@host:path`
        None => u.replacen(':', "/", 1),
    };
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if host == "www.github.com" {
        "github.com"
    } else {
        host
    };
    let path = path.trim_end_matches('/');
    let path = path
        .strip_suffix(".git")
        .unwrap_or(path)
        .trim_end_matches('/');
    format!("{host}{path}")
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
    let dest = match clone_dest(work_root, &name, url) {
        Dest::Reuse(dest) => {
            // Reuse: never wipe. Refresh link metadata only.
            let linked = link_local(&dest, override_branch);
            return Ok(Linked {
                remote_url: linked.remote_url.or_else(|| Some(sanitize_remote_url(url))),
                note: Some(format!(
                    "Opened the existing folder {}. Local work was kept.",
                    dest.display()
                )),
                ..linked
            });
        }
        Dest::Clone(dest) => dest,
    };
    clone_remote(url, &dest, github_token)?;
    let mut linked = link_local(&dest, override_branch);
    if linked.remote_url.is_none() {
        linked.remote_url = Some(sanitize_remote_url(url));
    }
    Ok(linked)
}

/// Where [`link_remote`] puts a remote.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Dest {
    /// A folder that already holds a clone of this remote.
    Reuse(PathBuf),
    /// A folder that isn't there yet.
    Clone(PathBuf),
}

/// `name`, `name-2`, … under `work_root`: the first that already holds a
/// clone of `url`, else the first that isn't there. A folder of that name
/// holding anything else is never cloned into or opened as this remote.
fn clone_dest(work_root: &Path, name: &str, url: &str) -> Dest {
    let want = remote_key(url);
    let mut free = None;
    for i in 1..100 {
        let cand = match i {
            1 => work_root.join(name),
            _ => work_root.join(format!("{name}-{i}")),
        };
        if !cand.exists() {
            free.get_or_insert(cand);
        } else if cand.join(".git").exists()
            && configured_origin(&cand).is_some_and(|got| remote_key(&got) == want)
        {
            return Dest::Reuse(cand);
        }
    }
    Dest::Clone(
        free.unwrap_or_else(|| work_root.join(format!("{name}-{}", grokhub_core::now_ms()))),
    )
}

/// Query names whose value is a credential (`?access_token=…`).
const SECRET_QUERY: &[&str] = &["token", "key", "secret", "pass", "auth", "sig", "cred"];

/// Strip credentials from a URL before storing or showing: an HTTP(S)
/// URL's user and password, any other URL's password (an SSH user such as
/// `git@` stays), and query values named for a credential.
pub fn sanitize_remote_url(url: &str) -> String {
    let u = url.trim();
    let Some((scheme, rest)) = u.split_once("://") else {
        return u.to_string();
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let host = match authority.rsplit_once('@') {
        Some((_, host))
            if scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http") =>
        {
            host.to_string()
        }
        Some((user, host)) => {
            let user = user.split_once(':').map_or(user, |(user, _)| user);
            format!("{user}@{host}")
        }
        None => authority.to_string(),
    };
    let (path, fragment) = tail
        .split_once('#')
        .map_or((tail, None), |(p, f)| (p, Some(f)));
    let tail = match path.split_once('?') {
        Some((path, query)) => {
            let kept: Vec<&str> = query
                .split('&')
                .filter(|pair| {
                    let name = pair.split('=').next().unwrap_or("").to_ascii_lowercase();
                    !SECRET_QUERY.iter().any(|word| name.contains(word))
                })
                .collect();
            if kept.is_empty() {
                path.to_string()
            } else {
                format!("{path}?{}", kept.join("&"))
            }
        }
        None => path.to_string(),
    };
    match fragment {
        Some(fragment) => format!("{scheme}://{host}{tail}#{fragment}"),
        None => format!("{scheme}://{host}{tail}"),
    }
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

    fn sh_git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn folders(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A folder that already holds a clone of the same remote is opened as
    /// it is: no clone, so no network, and its local work stays.
    #[test]
    fn linking_a_url_whose_clone_is_already_there_reuses_it() {
        let _offline = grokhub_core::offline::force_for_thread(true);
        let root = std::env::temp_dir().join(format!("link-reuse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let existing = root.join("app");
        std::fs::create_dir_all(&existing).unwrap();
        sh_git(&existing, &["init", "-q"]);
        sh_git(
            &existing,
            &[
                "remote",
                "add",
                "origin",
                "https://user:sk-abcdefghijklmnopqrstuv@github.com/Org/App/",
            ],
        );
        std::fs::write(existing.join("work.txt"), "local work\n").unwrap();
        let got = link_remote("https://github.com/org/app.git", &root, None, None);
        let names = folders(&root);
        let _ = std::fs::remove_dir_all(&root);
        let linked = got.expect("the clone that is there is opened");
        assert_eq!(linked.path, existing.display().to_string());
        assert_eq!(
            linked.remote_url.as_deref(),
            Some("https://github.com/Org/App/")
        );
        assert_eq!(
            linked.note,
            Some(format!(
                "Opened the existing folder {}. Local work was kept.",
                existing.display()
            ))
        );
        assert_eq!(names, vec!["app".to_string()]);
    }

    /// Linking the same URL twice opens the first clone again; a folder of
    /// that name that holds another repo still gets a clone of its own.
    #[cfg(unix)]
    #[test]
    fn linking_a_remote_twice_opens_the_first_clone_and_another_repo_gets_a_new_name() {
        let _online = grokhub_core::offline::force_for_thread(false);
        let root = std::env::temp_dir().join(format!("link-twice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bare = root.join("src.git");
        let seed = root.join("seed");
        sh_git(
            &root,
            &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
        );
        sh_git(
            &root,
            &[
                "clone",
                "-q",
                bare.to_str().unwrap(),
                seed.to_str().unwrap(),
            ],
        );
        std::fs::write(seed.join("a.txt"), "x\n").unwrap();
        sh_git(&seed, &["add", "a.txt"]);
        sh_git(&seed, &["commit", "-qm", "x"]);
        sh_git(&seed, &["push", "-q", "origin", "HEAD:main"]);
        // The GitHub URLs below are cloned from local bare repos.
        let other = root.join("other.git");
        sh_git(
            &root,
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                other.to_str().unwrap(),
            ],
        );
        sh_git(&seed, &["push", "-q", other.to_str().unwrap(), "HEAD:main"]);
        let gitconfig = root.join("gitconfig");
        std::fs::write(
            &gitconfig,
            format!(
                "[url \"{}\"]\n\tinsteadOf = https://github.com/org/app.git\n[url \"{}\"]\n\tinsteadOf = https://github.com/other/app.git\n[protocol \"file\"]\n\tallow = always\n",
                bare.display(),
                other.display()
            ),
        )
        .unwrap();
        let old = std::env::var_os("GIT_CONFIG_GLOBAL");
        // SAFETY: tests run one at a time in this process, so nothing else
        // reads the environment while git's global config points here.
        unsafe { std::env::set_var("GIT_CONFIG_GLOBAL", &gitconfig) };
        let work = root.join("work");
        let first = link_remote("https://github.com/org/app.git", &work, None, None);
        if let Ok(first) = &first {
            std::fs::write(Path::new(&first.path).join("local-work.txt"), "mine\n").unwrap();
        }
        let second = link_remote("https://github.com/org/app.git", &work, None, None);
        let third = link_remote("https://github.com/other/app.git", &work, None, None);
        match old {
            Some(old) => unsafe { std::env::set_var("GIT_CONFIG_GLOBAL", old) },
            None => unsafe { std::env::remove_var("GIT_CONFIG_GLOBAL") },
        }
        let names = folders(&work);
        let kept = work.join("app").join("local-work.txt").exists();
        let _ = std::fs::remove_dir_all(&root);
        let (first, second, third) = (first.unwrap(), second.unwrap(), third.unwrap());
        let app = work.join("app").display().to_string();
        assert_eq!(first.path, app);
        assert_eq!(first.note, None);
        assert_eq!(second.path, app);
        assert_eq!(
            second.note,
            Some(format!(
                "Opened the existing folder {app}. Local work was kept."
            ))
        );
        assert!(kept, "the first clone's local work stays");
        assert_eq!(third.path, work.join("app-2").display().to_string());
        assert_eq!(names, vec!["app".to_string(), "app-2".to_string()]);
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
