//! Park handoff between the `grokhub --mcp-desktop` process (path A) and the
//! cabin window. The MCP process writes a request file and waits; the cabin
//! shows the white hard card and writes the answer. No answer within
//! [`APPROVAL_TTL`](crate::harness::APPROVAL_TTL), a halt, or a closed cabin
//! means Deny.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// One parked desktop call waiting for Jeremy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParkRequest {
    pub id: String,
    pub path: String,
    pub tool: String,
    /// What the card shows (redacted).
    pub action: String,
    pub class: String,
    pub ts_ms: u64,
}

/// `{config_dir}/harness/park`
pub fn park_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("harness").join("park")
}

fn safe_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect()
}

fn req_path(config_dir: &Path, id: &str) -> PathBuf {
    park_dir(config_dir).join(format!("{}.json", safe_id(id)))
}

fn answer_path(config_dir: &Path, id: &str) -> PathBuf {
    park_dir(config_dir).join(format!("{}.answer", safe_id(id)))
}

/// Write the request atomically (temp file, then rename).
pub fn post_park(config_dir: &Path, req: &ParkRequest) -> Result<(), String> {
    let dir = park_dir(config_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let body = serde_json::to_string(req).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{}.tmp", safe_id(&req.id)));
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, req_path(config_dir, &req.id)).map_err(|e| e.to_string())
}

/// Requests with no answer yet, oldest first.
pub fn pending_parks(config_dir: &Path) -> Vec<ParkRequest> {
    let Ok(rd) = fs::read_dir(park_dir(config_dir)) else {
        return Vec::new();
    };
    let mut out: Vec<ParkRequest> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<ParkRequest>(&s).ok())
        .filter(|r| !answer_path(config_dir, &r.id).exists())
        .collect();
    out.sort_by_key(|r| r.ts_ms);
    out
}

/// The cabin's click: `approve` true runs the call once.
pub fn answer_park(config_dir: &Path, id: &str, approve: bool) -> Result<(), String> {
    let dir = park_dir(config_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let word = if approve { "approve" } else { "deny" };
    fs::write(answer_path(config_dir, id), word).map_err(|e| e.to_string())
}

/// Read the answer once and clear both files. Anything but `approve` is Deny.
pub fn take_answer(config_dir: &Path, id: &str) -> Option<bool> {
    let raw = fs::read_to_string(answer_path(config_dir, id)).ok()?;
    clear_park(config_dir, id);
    Some(raw.trim() == "approve")
}

pub fn clear_park(config_dir: &Path, id: &str) {
    let _ = fs::remove_file(req_path(config_dir, id));
    let _ = fs::remove_file(answer_path(config_dir, id));
}

/// Block the MCP call until the cabin answers. Timeout or halt ⇒ Deny (false).
pub fn wait_park(
    config_dir: &Path,
    id: &str,
    ttl: Duration,
    poll: Duration,
    halted: &mut dyn FnMut() -> bool,
) -> bool {
    let start = Instant::now();
    loop {
        if let Some(ok) = take_answer(config_dir, id) {
            return ok;
        }
        if halted() || start.elapsed() >= ttl {
            clear_park(config_dir, id);
            return false;
        }
        std::thread::sleep(poll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(label: &str) -> PathBuf {
        crate::harness::test_dir(&format!("park-{label}"))
    }

    fn req(id: &str) -> ParkRequest {
        ParkRequest {
            id: id.into(),
            path: "A".into(),
            tool: "type".into(),
            action: "rm -f disposable.txt".into(),
            class: "delete".into(),
            ts_ms: 1,
        }
    }

    #[test]
    fn approve_round_trip_clears_files() {
        let dir = temp("approve");
        post_park(&dir, &req("p1")).unwrap();
        assert_eq!(pending_parks(&dir), vec![req("p1")]);
        answer_park(&dir, "p1", true).unwrap();
        assert_eq!(pending_parks(&dir), vec![]);
        assert!(wait_park(&dir, "p1", Duration::from_secs(1), Duration::from_millis(5), &mut || false));
        assert_eq!(fs::read_dir(park_dir(&dir)).unwrap().count(), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn deny_timeout_and_halt_fail_closed() {
        let dir = temp("deny");
        post_park(&dir, &req("p2")).unwrap();
        answer_park(&dir, "p2", false).unwrap();
        assert!(!wait_park(&dir, "p2", Duration::from_secs(1), Duration::from_millis(5), &mut || false));
        post_park(&dir, &req("p3")).unwrap();
        assert!(!wait_park(&dir, "p3", Duration::from_millis(20), Duration::from_millis(5), &mut || false));
        assert_eq!(pending_parks(&dir), vec![]);
        post_park(&dir, &req("p4")).unwrap();
        assert!(!wait_park(&dir, "p4", Duration::from_secs(5), Duration::from_millis(5), &mut || true));
        assert_eq!(fs::read_dir(park_dir(&dir)).unwrap().count(), 0);
        let _ = fs::remove_dir_all(dir);
    }
}
