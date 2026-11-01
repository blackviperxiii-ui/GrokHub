//! Settings → Privacy & data (shell parity): the Offline only switch, what
//! left this computer this week, where the data lives, and whether personal
//! memory is sealed. Offline only is the same `offline.toml` the cabin reads,
//! saved with `offline::set_in` (a temp file and a rename), and the page reads
//! it again each time it loads, so a flip in the cabin shows here on focus.
//! The week comes from the egress log, which records every off-device call
//! but not its size, so it counts calls. Know me stays in the cabin: its
//! indexer runs there only.

use std::collections::HashMap;
use std::path::Path;

use grokhub_agent::harness::{self as hx, Locked};
use grokhub_core::offline;
use serde::Serialize;

const DAY_MS: u64 = 86_400_000;
/// Hosts listed under the week's count; the rest are still counted.
const TOP_HOSTS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub host: String,
    pub calls: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub offline_only: bool,
    /// An on-device model is ready to take the chat while Offline only is on.
    pub local_ready: bool,
    /// "3 calls sent off-device this week".
    pub sent: String,
    /// The busiest hosts this week, most calls first.
    pub hosts: Vec<Host>,
    /// Personal memory and the logs are sealed with the keyring.
    pub encrypted: bool,
    pub encrypted_note: String,
    pub data_folder: String,
    /// Open data folder works here (desktops only).
    pub can_open_folder: bool,
}

/// Why sealed data can't open, as the cabin says it.
fn lock_text(why: &Locked) -> &'static str {
    match why {
        Locked::Unavailable => "Private data is unavailable",
        Locked::Missing => "Private data key is missing",
        Locked::WrongKey => "Private data key doesn't match",
        Locked::Busy => "Private data is being prepared",
        Locked::Unwritable => "Private data can't be saved",
    }
}

/// The Encrypted memory row from a keyring answer (`None`: not answered yet).
pub fn sealed(answer: Option<Option<Locked>>) -> (bool, String) {
    match answer {
        Some(None) => (
            true,
            "Personal memory and the logs are sealed with your keyring.".into(),
        ),
        Some(Some(why)) => (false, format!("{}.", lock_text(&why))),
        None => (false, "Your keyring hasn't answered yet.".into()),
    }
}

/// Asks the keyring once; may wait on it, so callers run it off the UI thread.
pub fn keyring(config_dir: &Path) -> Option<Option<Locked>> {
    hx::read_key(config_dir, true).map(|read| read.err())
}

pub fn sent_line(calls: Option<u32>) -> String {
    match calls {
        None => "The off-device log is sealed and your keyring is locked.".into(),
        Some(1) => "1 call sent off-device this week".into(),
        Some(n) => format!("{n} calls sent off-device this week"),
    }
}

/// Calls per host for the seven UTC days ending today, most first, then by name.
pub fn week_hosts(lines: &[hx::EgressLine], now_ms: u64) -> Vec<Host> {
    let today = now_ms / DAY_MS;
    let mut by_host: HashMap<&str, u32> = HashMap::new();
    for line in lines {
        let day = line.ts_ms / DAY_MS;
        if day <= today && today - day < 7 {
            *by_host.entry(line.dest.as_str()).or_default() += 1;
        }
    }
    let mut hosts: Vec<Host> = by_host
        .into_iter()
        .map(|(host, calls)| Host {
            host: host.to_string(),
            calls,
        })
        .collect();
    hosts.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.host.cmp(&b.host)));
    hosts
}

/// The page, with the keyring's answer already in hand. Reads `offline.toml`
/// again first, so this process follows a flip the cabin made.
pub fn page(config_dir: &Path, lock: Option<Option<Locked>>, now_ms: u64) -> Page {
    offline::load_from(config_dir);
    let read = hx::read_egress_report(config_dir);
    let mut hosts = week_hosts(&read.lines, now_ms);
    let calls = match read.locked {
        Some(_) => None,
        None => Some(hosts.iter().map(|h| h.calls).sum()),
    };
    hosts.truncate(TOP_HOSTS);
    let (encrypted, encrypted_note) = sealed(lock);
    Page {
        offline_only: offline::enabled(),
        local_ready: grokhub_agent::route::local::installed(),
        sent: sent_line(calls),
        hosts,
        encrypted,
        encrypted_note,
        data_folder: config_dir.display().to_string(),
        can_open_folder: cfg!(desktop),
    }
}

/// Saves Offline only to `{config_dir}/offline.toml` and applies it to this
/// process, then reloads the connector slots so online ones pause or return.
pub fn set_offline(config_dir: &Path, on: bool) -> Result<(), String> {
    offline::set_in(config_dir, on)?;
    grokhub_agent::mcp::invalidate();
    Ok(())
}

/// Opens the GrokHub config folder in the file manager.
pub fn open_data_folder(config_dir: &Path) -> Result<(), String> {
    crate::capabilities::needs(cfg!(desktop), "Opening the data folder")?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = std::process::Command::new(opener)
        .arg(config_dir)
        .spawn()
        .map_err(|e| format!("Couldn't open {}: {e}", config_dir.display()))?;
    // Reap the helper so it never lingers as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-28 at 05:00 UTC.
    const NOW: u64 = 20_754 * DAY_MS + 5 * 3_600_000;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-privacy-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Leaves Offline only off for the rest of this test process.
    struct OffAfter(std::path::PathBuf);

    impl Drop for OffAfter {
        fn drop(&mut self) {
            let _ = offline::set_in(&self.0, false);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn line(ts_ms: u64, dest: &str) -> hx::EgressLine {
        serde_json::from_value(serde_json::json!({
            "ts_ms": ts_ms,
            "dest": dest,
            "data_classes": [],
        }))
        .expect("an egress line")
    }

    #[test]
    fn offline_only_writes_offline_toml_and_refuses_a_guarded_call_after() {
        let dir = temp_dir("offline");
        let _off = OffAfter(dir.clone());
        assert_eq!(offline::check_url("https://api.x.ai/v1/responses"), Ok(()));

        set_offline(&dir, true).expect("on");
        assert_eq!(
            std::fs::read_to_string(dir.join("offline.toml"))
                .ok()
                .as_deref(),
            Some("offline_only = true\n")
        );
        assert_eq!(
            offline::check_url("https://api.x.ai/v1/responses").map_err(|e| e.host),
            Err("api.x.ai".to_string())
        );
        // This computer stays reachable: the local model server is loopback.
        assert_eq!(offline::check_url("http://127.0.0.1:8080/v1"), Ok(()));
        assert!(page(&dir, Some(None), NOW).offline_only);

        set_offline(&dir, false).expect("off");
        assert_eq!(
            std::fs::read_to_string(dir.join("offline.toml"))
                .ok()
                .as_deref(),
            Some("offline_only = false\n")
        );
        assert_eq!(offline::check_url("https://api.x.ai/v1/responses"), Ok(()));
    }

    #[test]
    fn the_page_follows_a_flip_the_cabin_wrote() {
        let dir = temp_dir("cabin-flip");
        let _off = OffAfter(dir.clone());
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("offline.toml"), "offline_only = true\n").expect("write");
        assert!(page(&dir, Some(None), NOW).offline_only);
        std::fs::write(dir.join("offline.toml"), "offline_only = false\n").expect("write");
        assert!(!page(&dir, Some(None), NOW).offline_only);
    }

    #[test]
    fn a_new_install_has_sent_nothing() {
        let dir = temp_dir("empty");
        let got = page(&dir, Some(None), NOW);
        assert_eq!(
            (got.sent.as_str(), got.hosts, got.encrypted),
            ("0 calls sent off-device this week", vec![], true)
        );
        assert_eq!(got.data_folder, dir.display().to_string());
    }

    #[test]
    fn the_week_counts_calls_per_host_and_drops_older_ones() {
        let lines = [
            line(NOW, "api.x.ai"),
            line(NOW - 3_600_000, "api.x.ai"),
            line(NOW - 6 * DAY_MS, "hn.algolia.com"),
            line(NOW - 2 * DAY_MS, "api.github.com"),
            line(NOW - 2 * DAY_MS, "hn.algolia.com"),
            // Eight days back, and a clock ahead of today, count for nothing.
            line(NOW - 8 * DAY_MS, "old.example.com"),
            line(NOW + 2 * DAY_MS, "future.example.com"),
        ];
        assert_eq!(
            week_hosts(&lines, NOW),
            vec![
                Host {
                    host: "api.x.ai".into(),
                    calls: 2
                },
                Host {
                    host: "hn.algolia.com".into(),
                    calls: 2
                },
                Host {
                    host: "api.github.com".into(),
                    calls: 1
                },
            ]
        );
        assert_eq!(sent_line(Some(1)), "1 call sent off-device this week");
        assert_eq!(sent_line(Some(12)), "12 calls sent off-device this week");
        assert_eq!(
            sent_line(None),
            "The off-device log is sealed and your keyring is locked."
        );
    }

    #[test]
    fn encrypted_memory_reads_the_keyring_answer() {
        assert_eq!(
            sealed(Some(None)),
            (
                true,
                "Personal memory and the logs are sealed with your keyring.".to_string()
            )
        );
        assert_eq!(
            sealed(Some(Some(Locked::Unavailable))),
            (false, "Private data is unavailable.".to_string())
        );
        assert_eq!(
            sealed(None),
            (false, "Your keyring hasn't answered yet.".to_string())
        );
    }
}
