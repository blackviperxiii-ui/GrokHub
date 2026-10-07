//! Spike-8a acceptance tests (design P5). Every path is built with
//! `std::path` and every fixture is written by the test, so they run the
//! same on Linux and Windows.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::fs::{Entry, IndexFs, RealFs};
use super::paths::{HostOs, PlatformDirs};
use super::power::PowerSource;
use super::system_state::SystemProbe;
use super::*;
use crate::harness::{grant_scope, revoke_grant, use_key_store_for, MemoryKeyStore, UserClick};

/// 2026-10-07T12:00:00Z
const NOW: u64 = 1_791_374_400_000;

/// Real disk, but every call is counted and every path remembered.
#[derive(Default)]
struct CountingFs {
    calls: AtomicUsize,
    touched: Mutex<Vec<PathBuf>>,
}

impl CountingFs {
    fn note(&self, p: &Path) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.touched.lock().unwrap().push(p.to_path_buf());
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
    fn touched_text(&self) -> String {
        self.touched.lock().unwrap().iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n")
    }
}

impl IndexFs for CountingFs {
    fn list(&self, dir: &Path) -> std::io::Result<Vec<Entry>> {
        self.note(dir);
        RealFs.list(dir)
    }
    fn stat(&self, path: &Path) -> Option<Entry> {
        self.note(path);
        RealFs.stat(path)
    }
    fn read_head(&self, path: &Path, cap: u64) -> std::io::Result<Vec<u8>> {
        self.note(path);
        RealFs.read_head(path, cap)
    }
    fn copy(&self, from: &Path, to: &Path) -> std::io::Result<u64> {
        self.note(from);
        RealFs.copy(from, to)
    }
}

struct Power(bool);
impl PowerSource for Power {
    fn on_battery(&self) -> bool {
        self.0
    }
}

#[derive(Default)]
struct CountingProbe(AtomicUsize);
impl SystemProbe for CountingProbe {
    fn disk(&self, _path: &Path) -> Option<(u64, u64)> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Some((100_000_000_000, 40_000_000_000))
    }
    fn lines(&self, _program: &str, _args: &[&str]) -> Option<Vec<String>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        None
    }
}

struct Fixture {
    cfg: PathBuf,
    src: PathBuf,
    keys: Arc<MemoryKeyStore>,
    fs: CountingFs,
    probe: CountingProbe,
    dirs: PlatformDirs,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.cfg);
        let _ = std::fs::remove_dir_all(&self.src);
    }
}

impl Fixture {
    fn new(label: &str) -> Self {
        let cfg = crate::harness::test_dir(&format!("idx-{label}-cfg"));
        let src = crate::harness::test_dir(&format!("idx-{label}-src"));
        let keys = Arc::new(MemoryKeyStore::new());
        use_key_store_for(&cfg, keys.clone());
        let home = src.join("home");
        let dirs = PlatformDirs {
            os: HostOs::Linux,
            home: Some(home.clone()),
            appdata: None,
            local_appdata: None,
            program_data: None,
            config_home: Some(home.join(".config")),
            data_home: Some(home.join(".local").join("share")),
            system_data: Vec::new(),
        };
        Self { cfg, src, keys, fs: CountingFs::default(), probe: CountingProbe::default(), dirs }
    }

    fn home(&self) -> PathBuf {
        self.src.join("home")
    }

    fn env<'a>(&'a self, now_ms: u64, quiet: bool, power: &'a Power) -> TickEnv<'a> {
        TickEnv { config_dir: &self.cfg, now_ms, quiet, power, fs: &self.fs, probe: &self.probe, dirs: &self.dirs }
    }

    fn grant(&self, scope: &Scope) -> String {
        grant_scope(&self.cfg, scope, Some(&self.home()), UserClick::from_click()).unwrap().id
    }

    fn nodes(&self) -> usize {
        scope_store(&self.cfg).node_file_count()
    }

    /// Every byte the cabin wrote under its config dir, as lossy text.
    fn config_bytes(&self) -> String {
        let mut out = String::new();
        let mut stack = vec![self.cfg.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push_str(&p.display().to_string());
                    out.push('\n');
                    out.push_str(&String::from_utf8_lossy(&std::fs::read(&p).unwrap_or_default()));
                    out.push('\n');
                }
            }
        }
        out
    }

    fn assert_no_egress(&self) {
        assert!(!hx::egress_path(&self.cfg).exists(), "indexers never write an egress line");
    }
}

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn ran(out: &TickOutcome) -> (usize, usize) {
    match out {
        TickOutcome::Ran { written, known, .. } => (*written, *known),
        other => panic!("expected Ran, got {other:?}"),
    }
}

/// P5: every scope is off on boot, so the indexers read nothing at all.
#[test]
fn all_scopes_off_on_boot_reads_nothing() {
    let fx = Fixture::new("boot");
    write(&fx.home().join("Documents").join("notes.md"), "# Plan\n");
    let mut sched = Scheduler::default();
    let power = Power(false);
    for i in 0..5 {
        assert_eq!(sched.tick(&fx.env(NOW + i * RESCAN_MS, false, &power)), TickOutcome::Idle);
    }
    assert_eq!(fx.fs.calls(), 0, "zero indexer reads: {}", fx.fs.touched_text());
    assert_eq!(fx.probe.0.load(Ordering::SeqCst), 0);
    assert!(!fx.cfg.join("amr").exists(), "nothing written");
    fx.assert_no_egress();
}

/// A `files:<tmp>/Documents` grant indexes only that tree; `.ssh` keys and
/// `secrets.json` inside it are skipped and appear nowhere.
#[test]
fn a_folder_grant_indexes_only_that_tree_and_never_hard_excludes() {
    let fx = Fixture::new("files");
    let docs = fx.home().join("Documents");
    write(&docs.join("notes.md"), "# Zebra Plan Fixture\n\nbody text\n## Budget Fixture\n");
    write(&docs.join("sub").join("todo.txt"), "Groceries fixture\nmilk\n");
    write(&docs.join(".ssh").join("id_ed25519"), "FIXTURE-PRIVATE-KEY-MATERIAL\n");
    write(&docs.join("secrets.json"), "{\"token\":\"FIXTURE-SECRET-VALUE\"}\n");
    write(&docs.join("vault.kdbx"), "FIXTURE-VAULT\n");
    write(&fx.home().join("Pictures").join("outside.md"), "# Outside Fixture\n");
    #[cfg(unix)]
    std::os::unix::fs::symlink(fx.home().join("Pictures"), docs.join("pics-link")).unwrap();
    fx.grant(&Scope::Files(docs.display().to_string()));

    let mut sched = Scheduler::default();
    let out = sched.tick(&fx.env(NOW, false, &Power(false)));
    assert_eq!(ran(&out), (2, 0), "{out:?}");

    let index = ScopeIndex::load(&fx.cfg);
    let key = Scope::Files(docs.display().to_string()).key();
    let lines: Vec<&str> = index.for_scope(&key).iter().map(|f| f.line.as_str()).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("File notes.md · "), "{lines:?}");
    assert!(lines[0].ends_with(" · title: Zebra Plan Fixture · headings: Budget Fixture"), "{lines:?}");
    assert!(lines[1].starts_with("File sub/todo.txt · "), "{lines:?}");
    assert!(lines[1].ends_with(" · title: Groceries fixture"), "{lines:?}");

    let touched = fx.fs.touched_text();
    for never in [".ssh", "id_ed25519", "secrets.json", "vault.kdbx", "Pictures", "outside.md"] {
        assert!(!touched.contains(never), "{never} was read:\n{touched}");
        assert!(!lines.iter().any(|l| l.contains(never)), "{never} in a fact");
    }
    // On disk: nodes are sealed, the span holds counts only.
    let disk = fx.config_bytes();
    for plain in [
        "Zebra Plan Fixture",
        "Groceries fixture",
        "notes.md",
        "FIXTURE-PRIVATE-KEY",
        "FIXTURE-SECRET-VALUE",
        "FIXTURE-VAULT",
        "Outside Fixture",
        "id_ed25519",
        "secrets.json",
    ] {
        assert!(!disk.contains(plain), "{plain} is on disk in plain text");
    }
    let spans = hx::read_spans(&fx.cfg, INDEXER_TRACE).unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].args_redacted, r#"{"scope":"files","tables":[]}"#);
    assert_eq!(spans[0].result, "wrote 2, known 0");
    assert_eq!(spans[0].decision, "allow");
    assert_eq!(spans[0].origin, hx::Origin::Proactive);
    assert!(spans[0].consent_ref.starts_with("g-"));
    fx.assert_no_egress();

    // The same tree again, past the rescan: nothing new.
    let again = sched.tick(&fx.env(NOW + RESCAN_MS, false, &Power(false)));
    assert_eq!(ran(&again), (0, 2));
}

fn firefox_fixture(db: &Path) {
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let c = rusqlite::Connection::open(db).unwrap();
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, hidden INTEGER, last_visit_date INTEGER);
         CREATE TABLE moz_cookies (name TEXT, value TEXT);
         CREATE TABLE moz_formhistory (fieldname TEXT, value TEXT);
         INSERT INTO moz_places (url, title, visit_count, hidden, last_visit_date) VALUES
           ('https://fixture-docs.test/private/path?token=FIXTURE-QUERY-TOKEN', 'Fixture Page Title', 7, 0, 1791300000000000),
           ('https://www.fixture-docs.test/other', 'Other', 3, 0, 1791200000000000),
           ('https://fixture-news.test/', 'News', 2, 0, 1791100000000000),
           ('https://hidden-fixture.test/', 'Hidden', 9, 1, 1791100000000000);
         INSERT INTO moz_cookies VALUES ('sid', 'FIXTURE-COOKIE-VALUE');
         INSERT INTO moz_formhistory VALUES ('email', 'FIXTURE-FORM-VALUE');",
    )
    .unwrap();
}

fn chromium_fixture(db: &Path) {
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let c = rusqlite::Connection::open(db).unwrap();
    // 13_390_000_000_000_000 µs since 1601 ≈ 2025-04.
    c.execute_batch(
        "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, hidden INTEGER, last_visit_time INTEGER);
         CREATE TABLE logins (origin_url TEXT, password_value BLOB);
         CREATE TABLE cookies (host_key TEXT, value TEXT);
         INSERT INTO urls (url, title, visit_count, hidden, last_visit_time) VALUES
           ('https://fixture-chat.test/c/123', 'Chat', 11, 0, 13390000000000000);
         INSERT INTO logins VALUES ('https://fixture-chat.test', 'FIXTURE-PASSWORD');
         INSERT INTO cookies VALUES ('fixture-chat.test', 'FIXTURE-CHROME-COOKIE');",
    )
    .unwrap();
}

/// The browser fixture reads `moz_places` / `urls` only, never a cookie or
/// login table, and only from a temp copy: the original's mtime is unchanged.
#[test]
fn browser_history_reads_history_tables_from_a_temp_copy_only() {
    let fx = Fixture::new("browser");
    let profile = fx.home().join(".mozilla").join("firefox").join("abcd.default-release");
    let places = profile.join("places.sqlite");
    firefox_fixture(&places);
    write(&profile.join("logins.json"), "{\"FIXTURE-LOGINS\":1}");
    write(&profile.join("cookies.sqlite"), "FIXTURE-COOKIE-FILE");
    let chrome_default = fx.home().join(".config").join("google-chrome").join("Default");
    let history = chrome_default.join("History");
    chromium_fixture(&history);
    write(&chrome_default.join("Cookies"), "FIXTURE-COOKIES-FILE");
    write(&chrome_default.join("Login Data"), "FIXTURE-LOGIN-DATA");
    let mtime = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
    let (places_before, history_before) = (mtime(&places), mtime(&history));
    let places_bytes = std::fs::read(&places).unwrap();

    // Instrumented connection: the one read the indexer makes.
    let tmp = fx.cfg.join(INDEXERS_DIR).join("tmp");
    let read = browser_history::read_history(&fx.fs, &places, paths::BrowserKind::Firefox, &tmp).unwrap();
    assert_eq!(read.tables_read, vec!["moz_places".to_string()]);
    assert_eq!(read.denied, 0);
    assert_eq!(read.hosts.get("fixture-docs.test").map(|h| h.0), Some(10));
    assert_eq!(read.hosts.get("hidden-fixture.test"), None);
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "the temp copy is gone");

    fx.grant(&Scope::BrowserHistory("firefox".into()));
    fx.grant(&Scope::BrowserHistory("chrome".into()));
    let mut sched = Scheduler::default();
    let power = Power(false);
    let first = sched.tick(&fx.env(NOW, false, &power));
    let second = sched.tick(&fx.env(NOW, false, &power));
    // Sorted by scope key: chrome, then firefox.
    match (&first, &second) {
        (
            TickOutcome::Ran { kind: "browser_history", written: 1, tables: t1, .. },
            TickOutcome::Ran { kind: "browser_history", written: 2, tables: t2, .. },
        ) => {
            assert_eq!(t1, &vec!["urls".to_string()]);
            assert_eq!(t2, &vec!["moz_places".to_string()]);
        }
        other => panic!("{other:?}"),
    }
    let index = ScopeIndex::load(&fx.cfg);
    let ff: Vec<&str> = index.for_scope("browser_history:firefox").iter().map(|f| f.line.as_str()).collect();
    assert_eq!(
        ff,
        vec![
            "Visited fixture-docs.test 10 times in Firefox, last on 2026-10-06",
            "Visited fixture-news.test 2 times in Firefox, last on 2026-10-04",
        ]
    );
    let ch: Vec<&str> = index.for_scope("browser_history:chrome").iter().map(|f| f.line.as_str()).collect();
    assert_eq!(ch.len(), 1);
    assert!(ch[0].starts_with("Visited fixture-chat.test 11 times in Chrome, last on "), "{ch:?}");

    assert_eq!(mtime(&places), places_before, "the original is never opened");
    assert_eq!(mtime(&history), history_before);
    assert_eq!(std::fs::read(&places).unwrap(), places_bytes, "not even the WAL header changes");
    let touched = fx.fs.touched_text();
    for never in ["logins.json", "cookies.sqlite", "Cookies", "Login Data"] {
        assert!(!touched.lines().any(|l| l.ends_with(never)), "{never} was touched:\n{touched}");
    }
    let disk = fx.config_bytes();
    for plain in [
        "FIXTURE-COOKIE",
        "FIXTURE-PASSWORD",
        "FIXTURE-FORM-VALUE",
        "FIXTURE-QUERY-TOKEN",
        "FIXTURE-LOGIN",
        "fixture-docs.test",
        "Fixture Page Title",
        "private/path",
    ] {
        assert!(!disk.contains(plain), "{plain} is on disk in plain text");
    }
    assert!(!ff.iter().chain(ch.iter()).any(|l| l.contains("private") || l.contains("token") || l.contains("Title")));
    fx.assert_no_egress();
}

/// Revoke: the next tick writes nothing, even with new files to read.
#[test]
fn revoke_means_no_new_nodes_after_one_tick() {
    let fx = Fixture::new("revoke");
    let docs = fx.home().join("Documents");
    write(&docs.join("a.md"), "# A\n");
    let id = fx.grant(&Scope::Files(docs.display().to_string()));
    let mut sched = Scheduler::default();
    let power = Power(false);
    assert_eq!(ran(&sched.tick(&fx.env(NOW, false, &power))), (1, 0));
    // Control: a new file is picked up on the next due tick while granted.
    write(&docs.join("b.md"), "# B\n");
    assert_eq!(ran(&sched.tick(&fx.env(NOW + RESCAN_MS, false, &power))), (1, 1));
    let before = fx.nodes();
    assert_eq!(before, 2);

    write(&docs.join("c.md"), "# C\n");
    assert_eq!(revoke_grant(&fx.cfg, &id), Ok(true));
    let reads = fx.fs.calls();
    assert_eq!(sched.tick(&fx.env(NOW + 2 * RESCAN_MS, false, &power)), TickOutcome::Idle);
    assert_eq!(fx.nodes(), before, "0 new nodes after one tick");
    assert_eq!(fx.fs.calls(), reads, "and nothing read");
    fx.assert_no_egress();
}

/// No keyring: Paused, and no node is written (never plaintext).
#[test]
fn no_keyring_pauses_and_writes_nothing() {
    let fx = Fixture::new("locked");
    let docs = fx.home().join("Documents");
    write(&docs.join("a.md"), "# Locked Fixture\n");
    fx.grant(&Scope::Files(docs.display().to_string()));
    fx.keys.set_available(false);
    use_key_store_for(&fx.cfg, fx.keys.clone());
    let mut sched = Scheduler::default();
    match sched.tick(&fx.env(NOW, false, &Power(false))) {
        TickOutcome::Paused(why) => assert!(why.starts_with("Private data is locked"), "{why}"),
        other => panic!("expected Paused, got {other:?}"),
    }
    assert_eq!(fx.nodes(), 0);
    assert_eq!(fx.fs.calls(), 0, "nothing read while locked");
    assert!(!fx.config_bytes().contains("Locked Fixture"));

    // Unlock: the next tick indexes and the in-memory index is rebuilt.
    fx.keys.set_available(true);
    use_key_store_for(&fx.cfg, fx.keys.clone());
    assert_eq!(ran(&sched.tick(&fx.env(NOW, false, &Power(false)))), (1, 0));
    assert_eq!(ScopeIndex::load(&fx.cfg).facts.values().map(Vec::len).sum::<usize>(), 1);
}

/// On battery or in quiet hours a tick does no work (fake power, fake clock).
#[test]
fn battery_and_quiet_hours_mean_no_tick_work() {
    let fx = Fixture::new("pace");
    fx.grant(&Scope::SystemState);
    let mut sched = Scheduler::default();
    assert_eq!(sched.tick(&fx.env(NOW, false, &Power(true))), TickOutcome::OnBattery);
    let quiet = grokhub_core::organs::quiet_hours_active("23:30", "22:00", "07:00");
    assert!(quiet);
    assert_eq!(sched.tick(&fx.env(NOW, quiet, &Power(false))), TickOutcome::QuietHours);
    assert_eq!(fx.probe.0.load(Ordering::SeqCst), 0);
    assert_eq!(fx.nodes(), 0);
    let day = grokhub_core::organs::quiet_hours_active("12:00", "22:00", "07:00");
    let out = sched.tick(&fx.env(NOW, day, &Power(false)));
    assert_eq!(ran(&out), (1, 0), "/ and home are one disk on the fake probe: {out:?}");
    assert!(fx.probe.0.load(Ordering::SeqCst) > 0);
    let index = ScopeIndex::load(&fx.cfg);
    assert_eq!(
        index.for_scope("system_state").iter().map(|f| f.line.as_str()).collect::<Vec<_>>(),
        vec!["System on 2026-10-07: disk / is 60% used, 40 GB free of 100 GB"],
        "home on the same disk is one line"
    );
}

/// "Forget these": the listed facts are tombstoned, drop out of the index,
/// and the indexer never writes them again.
#[test]
fn forget_these_purges_and_stays_forgotten() {
    let fx = Fixture::new("forget");
    let docs = fx.home().join("Documents");
    write(&docs.join("a.md"), "# A\n");
    write(&docs.join("b.md"), "# B\n");
    fx.grant(&Scope::Files(docs.display().to_string()));
    let key = Scope::Files(docs.display().to_string()).key();
    let mut sched = Scheduler::default();
    let power = Power(false);
    assert_eq!(ran(&sched.tick(&fx.env(NOW, false, &power))), (2, 0));
    let ids: Vec<String> = ScopeIndex::load(&fx.cfg).for_scope(&key).iter().map(|f| f.id.clone()).collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(forget_facts(&fx.cfg, &ids), Ok(2));
    assert_eq!(ScopeIndex::load(&fx.cfg).for_scope(&key), &[] as &[IndexedFact]);
    assert_eq!(forget_facts(&fx.cfg, &["mem-000000000000".to_string()]), Ok(0), "only scope facts");
    assert_eq!(ran(&sched.tick(&fx.env(NOW + RESCAN_MS, false, &power))), (0, 2), "forgotten stays forgotten");
    assert_eq!(ScopeIndex::load(&fx.cfg).for_scope(&key).len(), 0);
}

/// Apps: `.desktop` names (hidden ones skipped) with the cabin's own launch counts.
#[test]
fn apps_index_desktop_entries_with_launch_counts() {
    let fx = Fixture::new("apps");
    let apps = fx.home().join(".local").join("share").join("applications");
    write(&apps.join("firefox.desktop"), "[Desktop Entry]\nType=Application\nName=Firefox\n");
    write(&apps.join("helper.desktop"), "[Desktop Entry]\nType=Application\nName=Helper\nNoDisplay=true\n");
    write(
        &fx.cfg.join(INDEXERS_DIR).join(apps::APP_LAUNCH_LOG),
        "{\"app\":\"firefox.desktop\"}\n{\"app\":\"firefox.desktop\"}\n",
    );
    fx.grant(&Scope::Apps);
    let mut sched = Scheduler::default();
    assert_eq!(ran(&sched.tick(&fx.env(NOW, false, &Power(false)))), (1, 0));
    let index = ScopeIndex::load(&fx.cfg);
    assert_eq!(
        index.for_scope("apps").iter().map(|f| f.line.as_str()).collect::<Vec<_>>(),
        vec!["Installed app: Firefox · opened 2 times from GrokHub"]
    );
}

/// Windows: Start-menu `.lnk` names, nested folders, uninstallers skipped.
#[test]
fn windows_start_menu_apps() {
    let fx = Fixture::new("winapps");
    let roaming = fx.src.join("AppData").join("Roaming");
    let programs = roaming.join("Microsoft").join("Windows").join("Start Menu").join("Programs");
    write(&programs.join("Notepad++.lnk"), "x");
    write(&programs.join("7-Zip").join("7-Zip File Manager.lnk"), "x");
    write(&programs.join("7-Zip").join("Uninstall 7-Zip.lnk"), "x");
    let dirs = PlatformDirs { os: HostOs::Windows, appdata: Some(roaming), ..fx.dirs.clone() };
    let facts = apps::index_apps(&fx.fs, &dirs, &BTreeMap::new());
    let names: Vec<&str> = facts.iter().map(|f| f.line.as_str()).collect();
    assert_eq!(names, vec!["Installed app: Notepad++", "Installed app: 7-Zip File Manager"]);
}

#[test]
fn hard_excludes_cover_password_managers_and_key_files() {
    let auth_file = format!("/home/me/project/{}", concat!("auth", ".json"));
    let grok_config = format!("/home/me/.grok/{}", concat!("config", ".toml"));
    for p in [
        "/home/me/Documents/.ssh/id_ed25519",
        "/home/me/.password-store/bank.gpg",
        "/home/me/Documents/vault.kdbx",
        "/home/me/Documents/id_rsa.pub",
        "/home/me/project/.env.local",
        auth_file.as_str(),
        "/home/me/.config/GrokHub/consent.jsonl",
        "C:\\Users\\me\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Login Data",
        "/home/me/.local/share/keyrings/login.keyring",
        grok_config.as_str(),
    ] {
        assert!(index_excluded(p), "{p}");
    }
    for p in ["/home/me/Documents/notes.md", "/home/me/Documents/keynote.txt", "/home/me/env/readme.md"] {
        assert!(!index_excluded(p), "{p}");
    }
    assert_eq!(node_id("files:/a", "x.md"), node_id("files:/a", "x.md"));
    assert_ne!(node_id("files:/a", "x.md"), node_id("files:/b", "x.md"));
    assert!(node_id("apps", "x").starts_with("scope-") && node_id("apps", "x").len() == 18);
    assert!(!has_reader(&Scope::Calendar) && !has_reader(&Scope::Mail) && has_reader(&Scope::Apps));
}

/// The agent can ask for a scope; asking never writes a grant.
#[test]
fn a_scope_ask_queues_a_card_and_grants_nothing() {
    let fx = Fixture::new("ask");
    let ledger = ConsentLedger::load(&fx.cfg);
    let mut asks = ScopeAsks::default();
    assert!(asks.ask(Scope::Apps, "  To suggest the right app I'd read your installed apps. ", &ledger, NOW));
    assert!(!asks.ask(Scope::Apps, "again", &ledger, NOW + 1), "one card per scope");
    assert!(!asks.ask(Scope::Calendar, "work hours", &ledger, NOW), "no calendar reader, no card");
    assert_eq!(asks.first().map(|a| a.why.as_str()), Some("To suggest the right app I'd read your installed apps."));
    assert_eq!(ConsentLedger::load(&fx.cfg).active().count(), 0, "asking grants nothing");
    assert!(!hx::consent_path(&fx.cfg).exists());
    fx.grant(&Scope::SystemState);
    let ledger = ConsentLedger::load(&fx.cfg);
    assert!(!asks.ask(Scope::SystemState, "disk", &ledger, NOW), "already allowed");
    asks.expire(NOW + hx::APPROVAL_TTL.as_millis() as u64);
    assert!(asks.is_empty(), "timeout is Not now");
    assert!(asks.ask(Scope::Apps, "apps", &ledger, NOW));
    asks.remove(&Scope::Apps);
    assert_eq!(asks.len(), 0);
}
