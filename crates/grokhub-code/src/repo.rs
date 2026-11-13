//! Code tab: a read-only look at the workspace's git repo (branch, changed
//! files, one file's diff) and the app's built-in skills. Nothing here writes
//! to the repo or edits a skill.
//!
//! git runs without a shell, with external diff tools, text conversion and
//! fsmonitor hooks off, so a repo's own config can't start other programs.

use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::{plugins, SkillSource, BUILTIN_SKILL_BODIES};
use serde::Serialize;
use std::io::Read;
use std::path::{Component, Path};
use std::process::{Command, Stdio};

/// The most diff text one file sends to the window.
pub const DIFF_CAP: usize = 256 * 1024;
/// The most changed files the list shows.
pub const FILES_CAP: usize = 500;
const UNREADABLE: &str = "That file couldn't be read.";

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

/// `origin`'s URL, if this folder is a repo with that remote.
pub fn origin_url(dir: &Path) -> Option<String> {
    let Ok((0, out)) = git(dir, &["remote", "get-url", "origin"]) else {
        return None;
    };
    let url = out.trim();
    (!url.is_empty()).then(|| url.to_string())
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

/// A file git doesn't track yet, as an all-added diff body. Only a plain
/// file is read: a link is not followed (it may lead out of the workspace,
/// or to a device that never ends), and a folder or pipe has no text.
fn as_new(path: &Path) -> Result<String, String> {
    if !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
        return Err(UNREADABLE.into());
    }
    let file = std::fs::File::open(path).map_err(|_| UNREADABLE.to_string())?;
    as_new_from(file)
}

/// [`as_new`] for anything that can be read. It reads one byte past
/// [`DIFF_CAP`] and no further, however big the file is.
fn as_new_from(reader: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    reader
        .take(DIFF_CAP as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| UNREADABLE.to_string())?;
    if bytes.contains(&0) {
        return Ok("New binary file.".into());
    }
    let body = String::from_utf8_lossy(&bytes);
    let mut out = String::from("New file, not tracked yet.\n");
    for line in body.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// A same-name project, user, cabin or plugin skill that shadows a built-in.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverriddenBy {
    /// "project", "user", "cabin" or "plugin".
    pub scope: String,
    pub name: String,
}

/// One built-in skill, always listed, even when a same-name skill overrides it.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Builtin {
    pub name: String,
    pub description: String,
    /// The skill body, without the YAML front matter.
    pub body: String,
    pub overridden_by: Option<OverriddenBy>,
}

/// The app's built-in skills, in ship order, with whatever currently overrides
/// each one (existing skill resolution). A built-in can't be deleted; removing
/// the override brings it back.
pub fn builtins(workspace: &Path, home: Option<&Path>, config_dir: &Path) -> Vec<Builtin> {
    let _config = ConfigGuard::set(config_dir);
    let found =
        grokhub_agent::discover_all_skills(workspace, home, &plugins::skill_dirs(workspace));
    BUILTIN_SKILL_BODIES
        .iter()
        .filter_map(|(folder, text)| {
            let (name, description, body) = parse_builtin(folder, text)?;
            let overridden_by = found.iter().find_map(|skill| {
                if skill.name != name || skill.source == SkillSource::Builtin {
                    return None;
                }
                Some(OverriddenBy {
                    scope: skill.source.as_str().to_string(),
                    name: skill.name.clone(),
                })
            });
            Some(Builtin {
                name,
                description,
                body,
                overridden_by,
            })
        })
        .collect()
}

fn parse_builtin(folder: &str, text: &str) -> Option<(String, String, String)> {
    let (yaml, body) = split_frontmatter(text).unwrap_or(("", text));
    let mut name = folder.to_string();
    let mut description = String::new();
    for line in yaml.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "name" => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    name = trimmed.to_string();
                }
            }
            "description" => description = value.trim().to_string(),
            _ => {}
        }
    }
    Some((name, description, body.trim().to_string()))
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let rest = text.strip_prefix("---")?;
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;
    let (yaml, after) = rest.split_once("\n---")?;
    let body = after
        .strip_prefix("\r\n")
        .or_else(|| after.strip_prefix('\n'))
        .unwrap_or(after);
    Some((yaml.trim_end_matches('\r'), body))
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

    /// Hands out `left` bytes of `a` and counts them.
    struct Counted {
        left: usize,
        given: usize,
    }

    impl Read for Counted {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.left);
            buf[..n].fill(b'a');
            self.left -= n;
            self.given += n;
            Ok(n)
        }
    }

    #[test]
    fn an_untracked_file_is_read_only_one_byte_past_the_diff_cap() {
        let mut big = Counted {
            left: 4 * DIFF_CAP,
            given: 0,
        };
        let text = as_new_from(&mut big).unwrap();
        assert_eq!(big.given, DIFF_CAP + 1);
        assert_eq!(big.given, 262_145);
        let head = "New file, not tracked yet.\n+";
        assert!(text.starts_with(head), "{}", &text[..40]);
        assert_eq!(text.len(), head.len() + DIFF_CAP + 1 + 1);
        assert_eq!(
            as_new_from(&b"one\ntwo\n"[..]).unwrap(),
            "New file, not tracked yet.\n+one\n+two\n"
        );
        assert_eq!(as_new_from(&b"a\0b"[..]).unwrap(), "New binary file.");
    }

    #[cfg(unix)]
    #[test]
    fn an_untracked_link_is_not_read_through() {
        let root = std::env::temp_dir().join(format!(
            "grokhub-code-tab-link-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let (dir, outside) = (root.join("repo"), root.join("outside"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let run = |args: &[&str]| git(&dir, args).unwrap().0;
        if run(&["init", "-q", "-b", "main"]) != 0 {
            return; // git missing on this runner
        }
        run(&["config", "user.email", "t@example.invalid"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["commit", "-q", "-m", "first"]);
        std::fs::write(outside.join("private.txt"), "not for the diff\n").unwrap();
        std::os::unix::fs::symlink(outside.join("private.txt"), dir.join("link.txt")).unwrap();
        std::os::unix::fs::symlink("/dev/zero", dir.join("endless")).unwrap();
        std::fs::write(dir.join("plain.txt"), "new\n").unwrap();
        assert_eq!(
            diff(&dir, "link.txt"),
            Err("That file couldn't be read.".to_string())
        );
        assert_eq!(
            diff(&dir, "endless"),
            Err("That file couldn't be read.".to_string())
        );
        assert_eq!(
            diff(&dir, "plain.txt").unwrap().text,
            "New file, not tracked yet.\n+new\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-code-builtins-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["cfg", "home", "ws/.git"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        dir
    }

    #[test]
    fn lists_the_five_built_ins_in_order_with_bodies_and_no_override() {
        let root = scratch("list");
        let (cfg, home, ws) = (root.join("cfg"), root.join("home"), root.join("ws"));
        let rows = builtins(&ws, Some(&home), &cfg);
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "qa-walkthrough",
                "av-qa-walkthrough",
                "create-verification-skill",
                "maintain-verification-skill",
                "follow-through",
            ]
        );
        let follow = rows.iter().find(|r| r.name == "follow-through").unwrap();
        assert_eq!(
            follow.description,
            "Ledger, check-ins, stall detection, status updates; proven done and handoff. /follow-through"
        );
        assert!(
            follow.body.starts_with("# Follow Through"),
            "{}",
            follow.body
        );
        for row in &rows {
            assert!(
                !row.body.starts_with("---"),
                "{} body starts with front matter: {}",
                row.name,
                &row.body[..row.body.len().min(40)]
            );
            assert_eq!(row.overridden_by, None, "{}", row.name);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_same_named_project_skill_overrides_and_removing_it_clears_the_flag() {
        let root = scratch("override");
        let (cfg, home, ws) = (root.join("cfg"), root.join("home"), root.join("ws"));
        let none = builtins(&ws, Some(&home), &cfg);
        let follow = none.iter().find(|r| r.name == "follow-through").unwrap();
        assert_eq!(follow.overridden_by, None);

        let skill = ws.join(".grok/skills/follow-through/SKILL.md");
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        std::fs::write(
            &skill,
            "---\nname: follow-through\ndescription: project override\n---\nProject body.\n",
        )
        .unwrap();
        let over = builtins(&ws, Some(&home), &cfg);
        let follow = over.iter().find(|r| r.name == "follow-through").unwrap();
        assert_eq!(
            follow.overridden_by,
            Some(OverriddenBy {
                scope: "project".into(),
                name: "follow-through".into(),
            })
        );
        // The row still lists the built-in's own description and body.
        assert_eq!(
            follow.description,
            "Ledger, check-ins, stall detection, status updates; proven done and handoff. /follow-through"
        );
        assert!(
            follow.body.starts_with("# Follow Through"),
            "{}",
            follow.body
        );

        // Drop the whole project skills tree so the discover cache key changes
        // (directory mtimes can stay in the same second as the write).
        std::fs::remove_dir_all(ws.join(".grok")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let again = builtins(&ws, Some(&home), &cfg);
        let follow = again.iter().find(|r| r.name == "follow-through").unwrap();
        assert_eq!(follow.overridden_by, None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
