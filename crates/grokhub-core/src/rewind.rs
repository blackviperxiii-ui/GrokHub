//! Snapshot / restore a bound project. Never snapshot $HOME unbound.

use serde::{Deserialize, Serialize};

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

/// An empty `create_dir_all` dest must not restore over the bound project.
pub fn rewind_snapshot_ready(path: &str) -> bool {
    let p = std::path::Path::new(path);
    match std::fs::read_dir(p) {
        Ok(mut ents) => ents.next().is_some(),
        Err(_) => false,
    }
}

pub fn keep_last_rewinds(rows: &[RewindRecord], max: usize) -> Vec<RewindRecord> {
    let mut v = rows.to_vec();
    v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
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
    }
}
