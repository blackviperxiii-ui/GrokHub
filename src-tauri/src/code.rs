//! Code tab, slice 1: a read-only look at the workspace's git repo (branch,
//! changed files, one file's diff). Nothing here writes to the repo.
//!
//! git runs without a shell, with external diff tools, text conversion and
//! fsmonitor hooks off, so a repo's own config can't start other programs.

use serde::Serialize;
use std::path::{Component, Path};
use std::process::{Command, Stdio};

/// The most diff text one file sends to the window.
pub const DIFF_CAP: usize = 256 * 1024;
/// The most changed files the list shows.
pub const FILES_CAP: usize = 500;

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    pub path: String,
    /// modified, added, deleted, renamed, untracked, conflict or changed.
    pub status: String,
    /// Staged for the next commit.
    pub staged: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub workspace: String,
    pub is_repo: bool,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<Changed>,
    /// More files changed than [`FILES_CAP`].
    pub more: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub path: String,
    pub text: String,
    pub cut: bool,
}

fn git(dir: &Path, args: &[&str]) -> Result<(i32, String), String> {
    let mut c = Command::new("git");
    c.arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.quotepath=false")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    let out = c
        .output()
        .map_err(|_| "git isn't installed or couldn't start".to_string())?;
    let code = out.status.code().unwrap_or(-1);
    Ok((code, String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// The workspace's repo, or `is_repo: false` when it isn't one.
pub fn repo(dir: &Path) -> Repo {
    let mut repo = Repo {
        workspace: dir.display().to_string(),
        is_repo: false,
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        files: Vec::new(),
        more: false,
    };
    let Ok((0, out)) = git(
        dir,
        &[
            "status",
            "--porcelain=v1",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
    ) else {
        return repo;
    };
    repo.is_repo = true;
    let parsed = parse_status(&out);
    repo.branch = parsed.branch;
    repo.upstream = parsed.upstream;
    repo.ahead = parsed.ahead;
    repo.behind = parsed.behind;
    repo.more = parsed.files.len() > FILES_CAP;
    repo.files = parsed.files.into_iter().take(FILES_CAP).collect();
    repo
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<Changed>,
}

/// `git status --porcelain=v1 --branch -z`.
pub fn parse_status(out: &str) -> Status {
    let mut st = Status::default();
    let mut parts = out.split('\0').filter(|s| !s.is_empty());
    while let Some(rec) = parts.next() {
        if let Some(head) = rec.strip_prefix("## ") {
            parse_head(head, &mut st);
            continue;
        }
        if rec.len() < 4 {
            continue;
        }
        let (xy, path) = rec.split_at(3);
        let mut chars = xy.chars();
        let x = chars.next().unwrap_or(' ');
        let y = chars.next().unwrap_or(' ');
        if x == 'R' || x == 'C' {
            // The old path follows as its own record.
            parts.next();
        }
        st.files.push(Changed {
            path: path.to_string(),
            status: status_word(x, y).into(),
            staged: x != ' ' && x != '?',
        });
    }
    st
}

fn parse_head(head: &str, st: &mut Status) {
    let (names, counts) = match head.split_once(" [") {
        Some((n, c)) => (n, c.trim_end_matches(']')),
        None => (head, ""),
    };
    let names = names.strip_prefix("No commits yet on ").unwrap_or(names);
    if names.starts_with("HEAD (no branch)") {
        st.branch = None;
    } else if let Some((b, up)) = names.split_once("...") {
        st.branch = Some(b.to_string());
        st.upstream = Some(up.to_string());
    } else {
        st.branch = Some(names.to_string());
    }
    for part in counts.split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            st.ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            st.behind = n.parse().unwrap_or(0);
        }
    }
}

fn status_word(x: char, y: char) -> &'static str {
    match (x, y) {
        ('?', '?') => "untracked",
        ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => "conflict",
        ('R', _) => "renamed",
        ('A', _) => "added",
        ('D', _) | (_, 'D') => "deleted",
        ('M', _) | (_, 'M') => "modified",
        _ => "changed",
    }
}

/// A path the diff may read: relative, inside the workspace, no `..`.
pub fn inside(rel: &str) -> Result<&str, String> {
    let p = Path::new(rel);
    let ok = !rel.trim().is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if ok {
        Ok(rel)
    } else {
        Err("That file is outside this workspace.".into())
    }
}

/// One file's changes against the last commit, or the whole file when git
/// doesn't track it yet (read directly, never through `--no-index`). Capped at [`DIFF_CAP`].
pub fn diff(dir: &Path, rel: &str) -> Result<Diff, String> {
    let rel = inside(rel)?;
    let base = [
        "--no-pager",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
    ];
    let (code, mut text) = git(dir, &[&base[..], &["HEAD", "--", rel]].concat())?;
    let tracked = git(dir, &["ls-files", "--error-unmatch", "--", rel])?.0 == 0;
    if !tracked {
        // Untracked (or no commit yet): show the file as all new.
        text = as_new(&dir.join(rel))?;
    } else if code != 0 {
        return Err("git couldn't show this file's changes.".into());
    }
    let cut = text.len() > DIFF_CAP;
    if cut {
        let mut end = DIFF_CAP;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Ok(Diff {
        path: rel.to_string(),
        text,
        cut,
    })
}

/// A file git doesn't track yet, as an all-added diff body.
fn as_new(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|_| "That file couldn't be read.".to_string())?;
    let bytes = &bytes[..bytes.len().min(DIFF_CAP + 1)];
    if bytes.contains(&0) {
        return Ok("New binary file.".into());
    }
    let body = String::from_utf8_lossy(bytes);
    let mut out = String::from("New file, not tracked yet.\n");
    for line in body.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_counts_and_files() {
        let out = "## beta...origin/beta [ahead 2, behind 1]\0 M src/a.rs\0A  new.rs\0?? notes.md\0R  b.rs\0old b.rs\0UU c.rs\0";
        let st = parse_status(out);
        assert_eq!(st.branch.as_deref(), Some("beta"));
        assert_eq!(st.upstream.as_deref(), Some("origin/beta"));
        assert_eq!((st.ahead, st.behind), (2, 1));
        let words: Vec<_> = st
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.status.as_str(), f.staged))
            .collect();
        assert_eq!(
            words,
            vec![
                ("src/a.rs", "modified", false),
                ("new.rs", "added", true),
                ("notes.md", "untracked", false),
                ("b.rs", "renamed", true),
                ("c.rs", "conflict", true),
            ]
        );
    }

    #[test]
    fn parses_fresh_and_detached_heads() {
        assert_eq!(
            parse_status("## No commits yet on main\0")
                .branch
                .as_deref(),
            Some("main")
        );
        assert_eq!(parse_status("## HEAD (no branch)\0").branch, None);
        assert_eq!(parse_status("## work\0").upstream, None);
    }

    #[test]
    fn diff_paths_stay_inside_the_workspace() {
        assert!(inside("src/a.rs").is_ok());
        assert!(inside("../secret").is_err());
        assert!(inside("a/../../b").is_err());
        assert!(inside("").is_err());
        assert!(inside("/etc/passwd").is_err());
        #[cfg(windows)]
        assert!(inside(r"C:\Windows\win.ini").is_err());
    }

    #[test]
    fn reads_a_real_repo() {
        let dir = std::env::temp_dir().join(format!("grokhub-code-tab-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| git(&dir, args).unwrap().0;
        if run(&["init", "-q", "-b", "main"]) != 0 {
            return; // git missing on this runner
        }
        run(&["config", "user.email", "t@example.invalid"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["commit", "-q", "-m", "first"]);
        std::fs::write(dir.join("a.txt"), "two\n").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        let r = repo(&dir);
        assert!(r.is_repo);
        assert_eq!(r.branch.as_deref(), Some("main"));
        let paths: Vec<_> = r.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["a.txt", "b.txt"]);
        let d = diff(&dir, "a.txt").unwrap();
        assert!(
            d.text.contains("-one") && d.text.contains("+two"),
            "{}",
            d.text
        );
        let n = diff(&dir, "b.txt").unwrap();
        assert!(n.text.contains("+new"), "{}", n.text);
        assert!(diff(&dir, "../x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
