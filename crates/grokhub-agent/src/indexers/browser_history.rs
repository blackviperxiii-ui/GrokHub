//! `browser_history:<browser>`: which sites the user visits, by host only.
//!
//! The history file is copied to a temp file under the cabin's own folder and
//! opened read-only; the original is never opened. An SQLite authorizer allows
//! reads of one table (`moz_places` or `urls`) and denies every other, so
//! cookies, logins and form data can't be read even by a wrong query. Paths
//! and query strings never leave this module: a fact is a host and a count.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use grokhub_core::amr::Sensitivity;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::{Connection, OpenFlags};

use super::fs::IndexFs;
use super::paths::BrowserKind;
use super::{date_of, index_excluded, Fact};

/// The only tables an indexer may read.
pub const HISTORY_TABLES: &[&str] = &["moz_places", "urls"];
/// Most rows read from one profile.
const ROWS_MAX: u32 = 5000;
/// Most hosts kept from one browser.
pub const HOSTS_MAX: usize = 100;
/// Microseconds from 1601-01-01 (Chromium's epoch) to 1970-01-01.
const CHROMIUM_EPOCH_US: i64 = 11_644_473_600_000_000;

/// One profile's rows, folded by host.
#[derive(Debug, Default)]
pub struct HistoryRead {
    /// host → (visits, last visit, unix ms)
    pub hosts: BTreeMap<String, (u64, u64)>,
    /// Every table the connection read, in order, deduplicated.
    pub tables_read: Vec<String>,
    /// Reads the authorizer refused (always 0 unless a query is wrong).
    pub denied: usize,
}

/// Deletes the temp copy (and any SQLite side files) when dropped.
struct TempCopy(PathBuf);

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        for side in ["-journal", "-wal", "-shm"] {
            let mut p = self.0.clone().into_os_string();
            p.push(side);
            let _ = std::fs::remove_file(PathBuf::from(p));
        }
    }
}

/// `scheme://[user@]host[:port]/…` → lowercase host, http(s) only.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = if host.starts_with('[') {
        host.split(']').next().map(|h| format!("{h}]"))?
    } else {
        host.split(':').next()?.to_string()
    };
    let host = host.trim_start_matches("www.").to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// SQLite's header byte 18/19 is 2 for WAL. A lone copied file in WAL mode
/// can't be opened read-only, so the copy (never the original) is switched
/// to rollback mode. Visits still only in the original's `-wal` are missed.
fn unwal(path: &Path) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let mut head = [0u8; 20];
    if f.read(&mut head)? == 20 && &head[..16] == b"SQLite format 3\0" && (head[18] == 2 || head[19] == 2) {
        f.seek(SeekFrom::Start(18))?;
        f.write_all(&[1, 1])?;
    }
    Ok(())
}

/// Tables read and reads refused, as the authorizer saw them.
type AuthLog = Arc<Mutex<(Vec<String>, usize)>>;

/// Open a (copied) history file read-only with an authorizer that allows
/// reads of `kind`'s one table and denies everything else.
fn open_guarded(path: &Path, kind: BrowserKind) -> Result<(Connection, AuthLog), String> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| e.to_string())?;
    let log: AuthLog = Arc::default();
    let seen = log.clone();
    let allowed = kind.table();
    conn.authorizer(Some(move |ctx: AuthContext<'_>| match ctx.action {
        AuthAction::Read { table_name, .. } => {
            let mut g = seen.lock().unwrap_or_else(|p| p.into_inner());
            if !g.0.iter().any(|t| t == table_name) {
                g.0.push(table_name.to_string());
            }
            if table_name == allowed {
                Authorization::Allow
            } else {
                g.1 += 1;
                Authorization::Deny
            }
        }
        AuthAction::Select | AuthAction::Function { .. } => Authorization::Allow,
        _ => Authorization::Deny,
    }));
    Ok((conn, log))
}

/// Copy `db` into `tmp_dir`, open the copy read-only, read the one history
/// table. The original file is only ever copied.
pub fn read_history(fs: &dyn IndexFs, db: &Path, kind: BrowserKind, tmp_dir: &Path) -> Result<HistoryRead, String> {
    if index_excluded(&db.display().to_string()) {
        return Err("that file is always excluded".into());
    }
    std::fs::create_dir_all(tmp_dir).map_err(|e| e.to_string())?;
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let copy = TempCopy(tmp_dir.join(format!("history-{}-{n}.sqlite", std::process::id())));
    fs.copy(db, &copy.0).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&copy.0, std::fs::Permissions::from_mode(0o600));
    }
    unwal(&copy.0).map_err(|e| e.to_string())?;
    let (conn, log) = open_guarded(&copy.0, kind)?;
    let sql = match kind {
        BrowserKind::Firefox => {
            "SELECT url, visit_count, last_visit_date FROM moz_places WHERE visit_count > 0 AND hidden = 0 ORDER BY visit_count DESC LIMIT ?1"
        }
        BrowserKind::Chromium => {
            "SELECT url, visit_count, last_visit_time FROM urls WHERE visit_count > 0 AND hidden = 0 ORDER BY visit_count DESC LIMIT ?1"
        }
    };
    let mut out = HistoryRead::default();
    {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([ROWS_MAX], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?.unwrap_or(0)))
            })
            .map_err(|e| e.to_string())?;
        for (url, visits, last) in rows.flatten() {
            let Some(host) = host_of(&url) else {
                continue;
            };
            let last_ms = match kind {
                BrowserKind::Firefox => last / 1000,
                BrowserKind::Chromium => (last - CHROMIUM_EPOCH_US) / 1000,
            }
            .max(0) as u64;
            let slot = out.hosts.entry(host).or_insert((0, 0));
            slot.0 += visits.max(0) as u64;
            slot.1 = slot.1.max(last_ms);
        }
    }
    drop(conn);
    let g = log.lock().unwrap_or_else(|p| p.into_inner());
    out.tables_read = g.0.clone();
    out.denied = g.1;
    Ok(out)
}

/// Facts for the busiest hosts across `reads`, most visited first.
pub fn history_facts(browser: &str, reads: &[HistoryRead]) -> Vec<Fact> {
    let mut hosts: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    for r in reads {
        for (host, (visits, last)) in &r.hosts {
            let slot = hosts.entry(host).or_insert((0, 0));
            slot.0 += visits;
            slot.1 = slot.1.max(*last);
        }
    }
    let mut rows: Vec<(&str, (u64, u64))> = hosts.into_iter().collect();
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
    rows.truncate(HOSTS_MAX);
    rows.into_iter()
        .map(|(host, (visits, last))| {
            let times = if visits == 1 { "once".to_string() } else { format!("{visits} times") };
            Fact {
                item: host.to_string(),
                line: format!("Visited {host} {times} in {browser}, last on {}", date_of(last)),
                sensitivity: Sensitivity::Sensitive,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Even a wrong query can't reach the cookie or login tables: the
    /// authorizer refuses it and logs the attempt.
    #[test]
    fn the_authorizer_denies_every_table_but_history() {
        let dir = crate::harness::test_dir("history-auth");
        let db = dir.join("places.sqlite");
        let c = Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE moz_places (url TEXT, visit_count INTEGER, hidden INTEGER, last_visit_date INTEGER);
             CREATE TABLE moz_cookies (name TEXT, value TEXT);
             INSERT INTO moz_places VALUES ('https://a.test/', 2, 0, 0);
             INSERT INTO moz_cookies VALUES ('sid', 'fixture-cookie-value');",
        )
        .unwrap();
        drop(c);
        let (conn, log) = open_guarded(&db, BrowserKind::Firefox).unwrap();
        let n: i64 = conn.query_row("SELECT count(url) FROM moz_places", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let err = conn.query_row("SELECT value FROM moz_cookies", [], |r| r.get::<_, String>(0)).unwrap_err();
        assert_eq!(err.to_string(), "access to moz_cookies.value is prohibited");
        assert!(conn.execute("DELETE FROM moz_places", []).is_err(), "read-only and no writes authorized");
        let g = log.lock().unwrap();
        assert_eq!(g.0, vec!["moz_places".to_string(), "moz_cookies".to_string()]);
        assert_eq!(g.1, 1);
        drop(g);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn host_of_keeps_the_host_only() {
        assert_eq!(host_of("https://www.GitHub.com/me/repo?token=abc#x").as_deref(), Some("github.com"));
        assert_eq!(host_of("http://user:pw@example.org:8080/a").as_deref(), Some("example.org"));
        assert_eq!(host_of("https://[::1]:3000/").as_deref(), Some("[::1]"));
        assert_eq!(host_of("file:///home/me/notes.html"), None);
        assert_eq!(host_of("about:blank"), None);
        assert_eq!(host_of("https:///nohost"), None);
    }

    #[test]
    fn facts_fold_profiles_and_sort_by_visits() {
        let a = HistoryRead {
            hosts: BTreeMap::from([("a.test".into(), (3, 10)), ("b.test".into(), (1, 5))]),
            ..HistoryRead::default()
        };
        let b = HistoryRead { hosts: BTreeMap::from([("b.test".into(), (4, 1_791_342_000_000))]), ..HistoryRead::default() };
        let facts = history_facts("Firefox", &[a, b]);
        let lines: Vec<&str> = facts.iter().map(|f| f.line.as_str()).collect();
        assert_eq!(
            lines,
            vec!["Visited b.test 5 times in Firefox, last on 2026-10-07", "Visited a.test 3 times in Firefox, last on 1970-01-01"]
        );
        assert_eq!(facts[0].item, "b.test");
        assert_eq!(facts[0].sensitivity, Sensitivity::Sensitive);
    }
}
