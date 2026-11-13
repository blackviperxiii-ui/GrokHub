//! The Code tab's activity log: one JSON line per push, PR, new branch,
//! blocked attempt, deploy, secret change, editor save, terminal session,
//! commit or waived review finding, in `code-activity.jsonl` in the config
//! folder. Commands and summaries pass through the secret redactor (and any
//! held project secret values) before they are written.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const FILE: &str = "code-activity.jsonl";
/// The newest entries the Code tab shows.
pub const SHOW: usize = 50;
/// Lines kept when the setting is unreadable.
pub const KEEP_DEFAULT: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Push,
    Pr,
    Branch,
    Blocked,
    Deploy,
    Secret,
    /// A file saved from the editor (path and size only).
    Edit,
    /// A terminal opened or closed (never what was typed or shown).
    Terminal,
    /// A commit made from the Tasks panel.
    Commit,
    /// A review finding the person waived.
    Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub ts_ms: u64,
    pub kind: Kind,
    pub repo_key: String,
    /// `github` or `other`.
    pub host: String,
    pub branch: String,
    /// The exact command, as shown in the confirm.
    pub command: String,
    /// `ask` or `autoBeta`.
    pub mode: String,
    /// `user` (confirmed), `autoBeta` (no confirm needed) or `none` (blocked).
    pub approved_by: String,
    pub exit: Option<i32>,
    pub summary: String,
}

static WRITE: Mutex<()> = Mutex::new(());

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

/// Add one entry, then keep only the newest `keep` lines.
pub fn append(config_dir: &Path, entry: &Entry, keep: usize) -> Result<(), String> {
    append_held(config_dir, entry, keep, &[])
}

/// [`append`] for a run that had project secrets in its environment: each held
/// value is replaced with the redaction marker before the line is written.
pub fn append_held(
    config_dir: &Path,
    entry: &Entry,
    keep: usize,
    held: &[String],
) -> Result<(), String> {
    let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let clean = Entry {
        command: crate::secrets::redact(&entry.command, held),
        summary: crate::secrets::redact(&entry.summary, held),
        ..entry.clone()
    };
    let line = serde_json::to_string(&clean).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let file = path(config_dir);
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .map_err(|e| e.to_string())?;
        writeln!(f, "{line}").map_err(|e| e.to_string())?;
    }
    let keep = keep.max(1);
    let raw = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() > keep {
        let tail = lines[lines.len() - keep..].join("\n") + "\n";
        let tmp = config_dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, tail).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The newest `limit` entries that pass `wanted`, newest first. Lines this
/// build can't read are skipped.
fn newest(config_dir: &Path, limit: usize, wanted: impl Fn(&Entry) -> bool) -> Vec<Entry> {
    let Ok(raw) = std::fs::read_to_string(path(config_dir)) else {
        return Vec::new();
    };
    raw.lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .filter(wanted)
        .take(limit)
        .collect()
}

/// The newest `limit` entries of every project, newest first.
pub fn list(config_dir: &Path, limit: usize) -> Vec<Entry> {
    newest(config_dir, limit, |_| true)
}

/// The newest `limit` entries of the project with this key, newest first.
pub fn list_for_key(config_dir: &Path, repo_key: &str, limit: usize) -> Vec<Entry> {
    newest(config_dir, limit, |e| e.repo_key == repo_key)
}

/// What the Code tab shows: the newest `limit` entries of the project in
/// `workspace`, newest first.
pub fn list_for(config_dir: &Path, workspace: &Path, limit: usize) -> Vec<Entry> {
    let key = crate::runs::repo_key(workspace, crate::repo::origin_url(workspace).as_deref());
    list_for_key(config_dir, &key, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("code-activity-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn entry(n: u64, summary: &str) -> Entry {
        Entry {
            ts_ms: n,
            kind: Kind::Push,
            repo_key: "k".into(),
            host: "github".into(),
            branch: "code/fix".into(),
            command: "git push --porcelain origin HEAD:refs/heads/code/fix".into(),
            mode: "ask".into(),
            approved_by: "user".into(),
            exit: Some(0),
            summary: summary.into(),
        }
    }

    #[test]
    fn newest_first_and_capped() {
        let d = dir("cap");
        assert!(list(&d, SHOW).is_empty());
        for n in 1..=5 {
            append(&d, &entry(n, &format!("push {n}")), 3).unwrap();
        }
        let raw = std::fs::read_to_string(d.join(FILE)).unwrap();
        assert_eq!(raw.lines().count(), 3);
        let got: Vec<u64> = list(&d, SHOW).iter().map(|e| e.ts_ms).collect();
        assert_eq!(got, vec![5, 4, 3]);
        let two: Vec<u64> = list(&d, 2).iter().map(|e| e.ts_ms).collect();
        assert_eq!(two, vec![5, 4]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_project_sees_its_own_newest_entries_only() {
        let d = dir("project");
        // Two projects take turns: "a" writes the odd times, "b" the even ones.
        for n in 1..=120 {
            let mut e = entry(n, &format!("push {n}"));
            e.repo_key = if n % 2 == 1 { "a" } else { "b" }.into();
            append(&d, &e, KEEP_DEFAULT).unwrap();
        }
        let got = list_for_key(&d, "a", SHOW);
        assert_eq!(got.len(), 50);
        assert!(got.iter().all(|e| e.repo_key == "a"));
        let times: Vec<u64> = got.iter().map(|e| e.ts_ms).collect();
        let newest_odd: Vec<u64> = (0..50).map(|i| 119 - 2 * i).collect();
        assert_eq!(times, newest_odd);
        assert_eq!((times[0], times[49]), (119, 21));
        assert!(list_for_key(&d, "c", SHOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_token_never_reaches_the_file() {
        let d = dir("redact");
        let token = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
        let mut e = entry(1, &format!("remote said {token}"));
        e.command = format!("git push https://{token}@github.com/o/r");
        append(&d, &e, KEEP_DEFAULT).unwrap();
        let raw = std::fs::read_to_string(d.join(FILE)).unwrap();
        assert!(!raw.contains(token), "{raw}");
        assert!(
            !raw.contains("abcdefghijklmnopqrstuvwxyz0123456789"),
            "{raw}"
        );
        let got = list(&d, 1);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].branch, "code/fix");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_line_shape_is_camel_case() {
        let line = serde_json::to_string(&entry(7, "ok")).unwrap();
        assert_eq!(
            line,
            r#"{"tsMs":7,"kind":"push","repoKey":"k","host":"github","branch":"code/fix","command":"git push --porcelain origin HEAD:refs/heads/code/fix","mode":"ask","approvedBy":"user","exit":0,"summary":"ok"}"#
        );
    }
}
