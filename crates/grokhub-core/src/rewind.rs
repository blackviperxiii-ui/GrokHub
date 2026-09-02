//! Snapshot / restore a bound project. Never snapshot $HOME unbound.

use serde::{Deserialize, Serialize};

use crate::project::{expand_project_root, is_under_project};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RewindRecord {
    pub job_id: String,
    pub path: String,
    pub root: String,
    pub created_at: u64,
    pub method: String,
}

const SENSITIVE_HOME_LEAVES: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".kube",
    ".config/GrokHub",
];

fn rewind_sensitive(root: &str, home: &str) -> bool {
    let r = normalize(root);
    let h = normalize(home);
    if h.is_empty() {
        return false;
    }
    SENSITIVE_HOME_LEAVES.iter().any(|leaf| {
        let p = format!("{h}/{leaf}");
        r == p || r.starts_with(&format!("{p}/"))
    })
}

pub fn rewind_allowed(root: &str, home: &str) -> bool {
    let expanded = crate::project::expand_project_root(root, Some(home));
    let r = normalize(&expanded);
    let h = normalize(home);
    if r.is_empty() || r == "/" || r == h {
        return false;
    }
    if rewind_sensitive(&r, &h) {
        return false;
    }
    r.starts_with(&format!("{h}/")) || r.starts_with("/tmp/") || r == "/tmp"
}

pub fn rewind_dest(config_root: &str, job_id: &str) -> String {
    let root = config_root.trim_end_matches('/');
    format!("{root}/rewind/{job_id}")
}

pub fn rewind_restore_matches(record_root: &str, current_root: &str) -> bool {
    let rec = normalize(record_root);
    let cur = normalize(current_root);
    !rec.is_empty() && rec == cur
}

/// Host must actually copy before a rewind row is recorded.
pub fn rewind_can_queue(host_on: bool, running: bool) -> bool {
    host_on && !running
}

/// Restore must not claim success when the host job cannot start.
pub fn rewind_blocked_reason(host_on: bool, running: bool) -> Option<&'static str> {
    if !host_on {
        Some("Host off — /host on")
    } else if running {
        Some("Busy — wait, then rewind")
    } else {
        None
    }
}

/// An empty `create_dir_all` dest must not restore over the bound project.
pub fn rewind_snapshot_ready(path: &str) -> bool {
    let p = std::path::Path::new(path);
    match std::fs::read_dir(p) {
        Ok(mut ents) => ents.next().is_some(),
        Err(_) => false,
    }
}

pub fn rewind_copy_cmd(src: &str, dest: &str) -> String {
    let src = src.replace('\'', r#"'"'"'"#);
    let dest = dest.replace('\'', r#"'"'"'"#);
    format!("cp -a '{src}/.' '{dest}'")
}

/// `run_cmds` already snapshots; a rewind `cp` must not start a nested host job.
pub fn is_rewind_copy_cmd(cmd: &str) -> bool {
    parse_rewind_copy(cmd).is_some()
}

pub fn is_rewind_copy_cmd_in(cmd: &str, project_root: &str, home: Option<&str>) -> bool {
    let Some((src, dest)) = parse_rewind_copy(cmd) else {
        return false;
    };
    let src_store = is_cabin_rewind_store(&src);
    let dest_store = is_cabin_rewind_store(&dest);
    if src_store == dest_store {
        return false;
    }
    let project_side = if dest_store { &src } else { &dest };
    let root = expand_project_root(project_root, home);
    if root.is_empty() {
        return false;
    }
    is_under_project(&expand_project_root(project_side, home), &root)
}

fn is_cabin_rewind_store(path: &str) -> bool {
    let p = path.trim().trim_end_matches('/').trim_end_matches("/.");
    p.contains("/.config/GrokHub/rewind/") || p.ends_with("/.config/GrokHub/rewind")
}

fn parse_rewind_copy(cmd: &str) -> Option<(String, String)> {
    let rest = cmd.trim().strip_prefix("cp -a ")?;
    let args = single_quoted_args(rest);
    if args.len() != 2 {
        return None;
    }
    let src = args[0].trim();
    let dest = args[1].trim();
    if !src.ends_with("/.") || dest.is_empty() {
        return None;
    }
    if !src.contains("/rewind/") && !dest.contains("/rewind/") {
        return None;
    }
    Some((src.to_string(), dest.to_string()))
}

fn single_quoted_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\'' {
            if in_q {
                if chars[i..].starts_with(&['\'', '"', '\'', '"', '\'']) {
                    cur.push('\'');
                    i += 5;
                    continue;
                }
                out.push(std::mem::take(&mut cur));
                in_q = false;
            } else {
                in_q = true;
            }
            i += 1;
            continue;
        }
        if in_q {
            cur.push(chars[i]);
        }
        i += 1;
    }
    out
}

pub fn keep_last_rewinds(rows: &[RewindRecord], max: usize) -> Vec<RewindRecord> {
    let mut v = rows.to_vec();
    v.sort_by_key(|r| std::cmp::Reverse(r.created_at));
    v.truncate(max.max(1));
    v
}

fn normalize(p: &str) -> String {
    let t = p.trim();
    if t.is_empty() {
        return String::new();
    }
    let out = t.trim_end_matches('/');
    if out.is_empty() {
        "/".into()
    } else {
        out.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuse_home_and_keep_five() {
        assert!(!rewind_allowed("/home/jeremy", "/home/jeremy"));
        assert!(!rewind_allowed("/", "/home/jeremy"));
        assert!(rewind_allowed("/home/jeremy/GrokHub-Work", "/home/jeremy"));
        assert!(rewind_allowed("/tmp/lab", "/home/jeremy"));
        assert!(!rewind_allowed("/home/jeremy/.ssh", "/home/jeremy"));
        assert!(!rewind_allowed("/home/jeremy/.ssh/id_ed25519", "/home/jeremy"));
        assert!(!rewind_allowed("/home/jeremy/.gnupg", "/home/jeremy"));
        assert!(!rewind_allowed("/home/jeremy/.aws", "/home/jeremy"));
        assert!(!rewind_allowed("/home/jeremy/.kube/config", "/home/jeremy"));
        assert!(!rewind_allowed("/home/jeremy/.config/GrokHub", "/home/jeremy"));
        assert_eq!(
            rewind_dest("/home/jeremy/.config/GrokHub/", "job-1"),
            "/home/jeremy/.config/GrokHub/rewind/job-1"
        );
        let rows = (0..7)
            .map(|i| RewindRecord {
                job_id: format!("j{i}"),
                path: format!("/r/{i}"),
                root: "/proj".into(),
                created_at: i,
                method: "copy".into(),
            })
            .collect::<Vec<_>>();
        let kept = keep_last_rewinds(&rows, 5);
        assert_eq!(kept.len(), 5);
        assert_eq!(kept[0].job_id, "j6");
        assert!(rewind_restore_matches("/home/j/proj", "/home/j/proj/"));
        assert!(!rewind_restore_matches("/home/j/proj-a", "/home/j/proj-b"));
        assert!(
            rewind_allowed("~/GrokHub-Work", "/home/jeremy"),
            "settings may store a tilde-bound project"
        );
        assert!(rewind_can_queue(true, false));
        assert!(
            !rewind_can_queue(false, false),
            "host off must not record an empty snapshot"
        );
        assert!(!rewind_can_queue(true, true));
        assert_eq!(
            rewind_blocked_reason(false, false),
            Some("Host off — /host on")
        );
        assert_eq!(
            rewind_blocked_reason(true, true),
            Some("Busy — wait, then rewind")
        );
        assert_eq!(rewind_blocked_reason(true, false), None);
        let empty = std::env::temp_dir().join(format!(
            "grokhub-rewind-empty-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        assert!(
            !rewind_snapshot_ready(&empty.to_string_lossy()),
            "an empty dest must not restore over the project"
        );
        std::fs::write(empty.join("kept.txt"), "ok").unwrap();
        assert!(rewind_snapshot_ready(&empty.to_string_lossy()));
        let _ = std::fs::remove_dir_all(&empty);
        let copy = rewind_copy_cmd("/home/j/.config/GrokHub/rewind/rw1", "/home/j/proj");
        assert!(is_rewind_copy_cmd(&copy));
        assert!(!is_rewind_copy_cmd("uname -a"));
        assert!(
            !is_rewind_copy_cmd("cp -a /proj/. /tmp/other"),
            "only cabin rewind dests count"
        );
        assert!(
            !is_rewind_copy_cmd("cp -a /tmp/rewind/evil/. /etc"),
            "an unquoted cp that merely mentions /rewind/ must not skip the project gate"
        );
        assert!(
            !is_rewind_copy_cmd_in(
                &rewind_copy_cmd("/home/j/.config/GrokHub/rewind/rw1", "/etc"),
                "/home/j/proj",
                Some("/home/j")
            ),
            "a quoted rewind cp whose dest is outside the bound tree must still be gated"
        );
        assert!(
            !is_rewind_copy_cmd_in(
                &rewind_copy_cmd("/home/j/proj", "/tmp/rewind/stolen"),
                "/home/j/proj",
                Some("/home/j")
            ),
            "a snapshot into a fake /tmp/rewind folder must not skip the project gate"
        );
        assert!(
            !is_rewind_copy_cmd_in(
                &rewind_copy_cmd("/home/j/proj", "/tmp/evil/GrokHub/rewind/stolen"),
                "/home/j/proj",
                Some("/home/j")
            ),
            "a dest that only contains /GrokHub/rewind/ is not the cabin store"
        );
        assert!(is_rewind_copy_cmd_in(
            &rewind_copy_cmd("/home/j/.config/GrokHub/rewind/rw1", "/home/j/proj"),
            "/home/j/proj",
            Some("/home/j")
        ));
        assert!(
            is_rewind_copy_cmd_in(
                &rewind_copy_cmd("/home/j/proj", "/home/j/.config/GrokHub/rewind/rw1"),
                "/home/j/proj",
                Some("/home/j")
            ),
            "cabin snapshots copy the bound tree into GrokHub/rewind"
        );
    }
}
