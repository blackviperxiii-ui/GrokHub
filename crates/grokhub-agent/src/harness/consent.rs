//! ConsentLedger (Spike-4a): per-source and per-destination grants.
//!
//! `{config_dir}/consent.jsonl`, append-only. A grant line is written only from
//! a user's click in the cabin ([`UserClick`]); revoking appends the same grant
//! with `revoked_at` set, and the last line per id wins. The agent never widens
//! its own permissions (harness design §12.0 rule 4): no tool, slash command,
//! automation, or model reply writes here, and the hard floor refuses shell
//! commands that name this file.
//!
//! Scopes (P5) are all off: a scope reads as on only while a user grant for it
//! is active. Spike-8a's indexers (`crate::indexers`) read a scope only while it is.
//!
//! Spike-4b: every line is sealed at rest (`at_rest`, key in the OS keyring).
//! With the keyring down or the key missing the ledger reads as locked: no
//! grant applies (fail closed) and nothing is written, never as plain text.
//! A revoke is sticky: once an id has a revoked line, no later copy of its
//! grant line brings it back.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::harness::at_rest::{self, Locked, AAD_CONSENT};
use crate::harness::egress::DataClass;

/// File name under the cabin config dir.
pub const CONSENT_FILE: &str = "consent.jsonl";
/// Largest ledger the cabin reads. A bigger file reads as empty (fail closed).
const CONSENT_CAP: u64 = 1024 * 1024;

/// One grant. Exactly one of `source` / `destination` is set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grant {
    pub id: String,
    /// A learning scope key (`calendar`, `files:/home/me/Documents`, …).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// An egress destination (`hub`, or a host such as `example.org`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub destination: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub data_classes: Vec<DataClass>,
    pub granted_at: u64,
    /// Always `user`. A line with any other value is ignored.
    pub by: String,
    #[serde(default)]
    pub revoked_at: Option<u64>,
}

impl Grant {
    pub fn active(&self) -> bool {
        self.by == "user" && self.revoked_at.is_none()
    }
}

/// Proof that a user's click in the cabin asked for a grant. Build it only in
/// a click handler (Settings → Permissions). A source-scanning test in
/// grokhub-app fails if `UserClick::from_click` shows up anywhere else.
#[derive(Debug)]
pub struct UserClick(());

impl UserClick {
    pub fn from_click() -> Self {
        Self(())
    }
}

/// Learning scopes (P5). Every one is off until the user grants it.
/// The screen is not here: it stays the desktop switch (Access).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// One folder, never all of `$HOME` in one grant.
    Files(String),
    Apps,
    BrowserHistory(String),
    Calendar,
    Mail,
    SystemState,
}

/// Scope kinds in the order `/privacy` lists them.
pub const SCOPE_KINDS: &[(&str, &str)] = &[
    ("files", "Files in a folder"),
    ("apps", "Installed apps"),
    ("browser_history", "Browser history"),
    ("calendar", "Calendar"),
    ("mail", "Mail"),
    ("system_state", "System state"),
];

/// Never indexed, whatever the grant says (P5 hard excludes).
pub const SCOPE_HARD_EXCLUDES: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".kube",
    ".config/GrokHub",
    ".grok",
    "secrets.json",
    "Cookies",
    "Login Data",
    "logins.json",
];

impl Scope {
    pub fn key(&self) -> String {
        match self {
            Self::Files(dir) => format!("files:{dir}"),
            Self::Apps => "apps".into(),
            Self::BrowserHistory(b) => format!("browser_history:{b}"),
            Self::Calendar => "calendar".into(),
            Self::Mail => "mail".into(),
            Self::SystemState => "system_state".into(),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        let key = key.trim();
        if let Some(dir) = key.strip_prefix("files:") {
            return (!dir.trim().is_empty()).then(|| Self::Files(dir.trim().to_string()));
        }
        if let Some(b) = key.strip_prefix("browser_history:") {
            return (!b.trim().is_empty()).then(|| Self::BrowserHistory(b.trim().to_string()));
        }
        match key {
            "apps" => Some(Self::Apps),
            "calendar" => Some(Self::Calendar),
            "mail" => Some(Self::Mail),
            "system_state" => Some(Self::SystemState),
            _ => None,
        }
    }
}

/// True when a path touches a hard exclude (`.ssh`, browser `Cookies`, …).
pub fn scope_excluded(path: &str) -> bool {
    // Case-blind: Windows and macOS paths are (`.SSH`, `cookies`), and
    // excluding a little more on Linux fails closed.
    let p = path.replace('\\', "/").to_lowercase();
    SCOPE_HARD_EXCLUDES.iter().map(|x| x.to_lowercase()).any(|x| {
        p == x
            || p.ends_with(&format!("/{x}"))
            || p.contains(&format!("/{x}/"))
            || p.starts_with(&format!("{x}/"))
    })
}

/// Why a scope cannot be granted, if it can't. `home` is the user's home dir.
pub fn scope_refusal(scope: &Scope, home: Option<&Path>) -> Option<String> {
    let Scope::Files(dir) = scope else {
        return None;
    };
    let d = dir.replace('\\', "/");
    let d = d.trim_end_matches('/');
    if d.is_empty() || !(d.starts_with('/') || d.as_bytes().get(1) == Some(&b':')) {
        return Some("files: needs one absolute folder".into());
    }
    if let Some(home) = home {
        let h = home.display().to_string().replace('\\', "/");
        let h = h.trim_end_matches('/');
        // The home folder itself, or any folder that holds it (`/home`).
        if d == h || h.starts_with(&format!("{d}/")) {
            return Some("files: one folder at a time, never all of your home folder".into());
        }
    }
    if scope_excluded(d) {
        return Some("files: that folder is always excluded".into());
    }
    None
}

/// The folded ledger: one current line per grant id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsentLedger {
    grants: Vec<Grant>,
    /// Why the sealed ledger could not be read. Then no grant applies.
    locked: Option<Locked>,
    /// Lines that did not open (damaged, edited, or plaintext in a sealed file).
    unreadable: usize,
    /// The keyring hasn't answered yet (UI read); read again next frame.
    pending: bool,
}

impl ConsentLedger {
    /// No grants: what a fresh config reads as.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Read `{config_dir}/consent.jsonl`. Missing, unreadable, or oversized
    /// reads as empty (fail closed). Bad lines are skipped. May wait on the
    /// OS keyring once; off the UI thread use this, on it use [`Self::load_now`].
    pub fn load(config_dir: &Path) -> Self {
        Self::load_with(config_dir, true)
    }

    /// Like [`Self::load`] but never waits on the keyring: until it has
    /// answered, the ledger is empty and [`Self::pending`] is true.
    pub fn load_now(config_dir: &Path) -> Self {
        Self::load_with(config_dir, false)
    }

    fn load_with(config_dir: &Path, wait: bool) -> Self {
        let path = consent_path(config_dir);
        let Ok(f) = fs::File::open(&path) else {
            return Self::empty();
        };
        if f.metadata().map(|m| m.len() > CONSENT_CAP).unwrap_or(true) {
            return Self::empty();
        }
        let mut text = String::new();
        if f.take(CONSENT_CAP).read_to_string(&mut text).is_err() {
            return Self::empty();
        }
        let read = at_rest::read_sealed_jsonl(config_dir, &text, AAD_CONSENT, wait);
        let mut ledger = Self::from_lines(&read.lines.join("\n"));
        ledger.locked = read.locked;
        ledger.unreadable = read.unreadable;
        ledger.pending = read.pending;
        ledger
    }

    fn from_lines(text: &str) -> Self {
        let mut grants: Vec<Grant> = Vec::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let Ok(g) = serde_json::from_str::<Grant>(line) else {
                continue;
            };
            if g.by != "user" || g.id.is_empty() {
                continue;
            }
            match grants.iter_mut().find(|x| x.id == g.id) {
                // Sticky revoke: a replayed grant line can't undo a revoke.
                Some(slot) if slot.revoked_at.is_some() && g.revoked_at.is_none() => {}
                Some(slot) => *slot = g,
                None => grants.push(g),
            }
        }
        Self { grants, ..Self::default() }
    }

    /// Why the ledger is closed, if it is. Then no grant applies.
    pub fn locked(&self) -> Option<&Locked> {
        self.locked.as_ref()
    }

    /// Ledger lines that did not open (damaged or edited).
    pub fn unreadable(&self) -> usize {
        self.unreadable
    }

    /// True while a UI read waits for the keyring's first answer.
    pub fn pending(&self) -> bool {
        self.pending
    }

    /// Every grant, revoked ones included, oldest first.
    pub fn all(&self) -> &[Grant] {
        &self.grants
    }

    pub fn active(&self) -> impl Iterator<Item = &Grant> {
        self.grants.iter().filter(|g| g.active())
    }

    /// An active grant for `dest` that covers every class in `data`.
    pub fn destination_grant(&self, dest: &str, data: &[DataClass]) -> Option<&Grant> {
        self.active().find(|g| {
            !g.destination.is_empty()
                && g.destination.eq_ignore_ascii_case(dest)
                && data.iter().all(|c| g.data_classes.contains(c))
        })
    }

    /// An active grant for exactly this scope.
    pub fn scope_grant(&self, scope: &Scope) -> Option<&Grant> {
        let key = scope.key();
        self.active().find(|g| g.source == key)
    }
}

pub fn consent_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CONSENT_FILE)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn grant_id(seed: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let digest = Sha256::digest(format!("{nanos}:{}:{seed}", std::process::id()).as_bytes());
    let hex: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("g-{hex}")
}

/// Seal and append one line. Locked ⇒ nothing is written (no plaintext fallback).
fn append_line(config_dir: &Path, g: &Grant) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let line = serde_json::to_string(g).map_err(|e| e.to_string())?;
    let line = grokhub_core::redact_secrets(&line);
    at_rest::append_sealed_line(config_dir, &consent_path(config_dir), AAD_CONSENT, &line)
        .map_err(|why| why.message())
}

/// Grant one destination for these data classes. Click only.
pub fn grant_destination(
    config_dir: &Path,
    dest: &str,
    data: &[DataClass],
    _click: UserClick,
) -> Result<Grant, String> {
    let dest = dest.trim().to_ascii_lowercase();
    if dest.is_empty() || data.is_empty() {
        return Err("a destination grant needs a destination and data classes".into());
    }
    let g = Grant {
        id: grant_id(&dest),
        source: String::new(),
        destination: dest,
        scope: "send".into(),
        data_classes: data.to_vec(),
        granted_at: now_ms(),
        by: "user".into(),
        revoked_at: None,
    };
    append_line(config_dir, &g)?;
    Ok(g)
}

/// Grant one learning scope. Click only. `home` guards `files:` grants.
pub fn grant_scope(
    config_dir: &Path,
    scope: &Scope,
    home: Option<&Path>,
    _click: UserClick,
) -> Result<Grant, String> {
    if let Some(why) = scope_refusal(scope, home) {
        return Err(why);
    }
    let key = scope.key();
    let g = Grant {
        id: grant_id(&key),
        source: key,
        destination: String::new(),
        scope: "read".into(),
        data_classes: vec![DataClass::Personal],
        granted_at: now_ms(),
        by: "user".into(),
        revoked_at: None,
    };
    append_line(config_dir, &g)?;
    Ok(g)
}

/// Revoke a grant. Returns false when no active grant has that id. Revoking
/// only narrows, so it needs no click proof.
pub fn revoke_grant(config_dir: &Path, id: &str) -> Result<bool, String> {
    let ledger = ConsentLedger::load(config_dir);
    if let Some(why) = ledger.locked() {
        return Err(why.message());
    }
    let Some(g) = ledger.active().find(|g| g.id == id).cloned() else {
        return Ok(false);
    };
    let mut gone = g;
    gone.revoked_at = Some(now_ms());
    append_line(config_dir, &gone)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::at_rest::{is_sealed_line, use_key_store_for, MemoryKeyStore};
    use crate::harness::egress::{DataClass, HUB_DEST};

    struct DirGuard(PathBuf);
    impl Drop for DirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn dir(label: &str) -> (PathBuf, DirGuard) {
        let p = crate::harness::test_dir(&format!("consent-{label}"));
        (p.clone(), DirGuard(p))
    }

    /// A test dir plus its in-memory keyring, so a test can take it away.
    fn keyed_dir(label: &str) -> (PathBuf, std::sync::Arc<MemoryKeyStore>, DirGuard) {
        let (d, g) = dir(label);
        let store = std::sync::Arc::new(MemoryKeyStore::new());
        use_key_store_for(&d, store.clone());
        (d, store, g)
    }

    #[test]
    fn fresh_config_has_no_grants_and_every_scope_is_off() {
        let (d, _g) = dir("fresh");
        let ledger = ConsentLedger::load(&d);
        assert_eq!(ledger, ConsentLedger::empty());
        assert_eq!(ledger.active().count(), 0);
        for scope in [
            Scope::Files("/home/me/Documents".into()),
            Scope::Apps,
            Scope::BrowserHistory("firefox".into()),
            Scope::Calendar,
            Scope::Mail,
            Scope::SystemState,
        ] {
            assert_eq!(ledger.scope_grant(&scope), None, "{}", scope.key());
        }
        assert_eq!(ledger.destination_grant(HUB_DEST, &[DataClass::Chat]), None);
        assert!(!consent_path(&d).exists(), "loading must not create the ledger");
    }

    #[test]
    fn grant_then_revoke_a_destination() {
        let (d, _g) = dir("grant-revoke");
        let data = [DataClass::Chat, DataClass::Personal];
        let g = grant_destination(&d, "Hub", &data, UserClick::from_click()).unwrap();
        assert!(g.id.starts_with("g-") && g.id.len() == 14, "{}", g.id);
        assert_eq!(g.destination, "hub");
        assert_eq!(g.by, "user");
        let ledger = ConsentLedger::load(&d);
        assert_eq!(ledger.destination_grant("hub", &data).map(|x| x.id.clone()), Some(g.id.clone()));
        assert_eq!(ledger.destination_grant("hub", &[DataClass::Chat]).map(|x| x.id.clone()), Some(g.id.clone()));
        assert_eq!(ledger.destination_grant("hub", &[DataClass::Sensitive]), None);
        assert_eq!(ledger.destination_grant("example.org", &[DataClass::Chat]), None);

        assert_eq!(revoke_grant(&d, &g.id), Ok(true));
        assert_eq!(revoke_grant(&d, &g.id), Ok(false), "already revoked");
        let after = ConsentLedger::load(&d);
        assert_eq!(after.destination_grant("hub", &data), None);
        assert_eq!(after.all().len(), 1);
        assert!(after.all()[0].revoked_at.is_some());
        let text = fs::read_to_string(consent_path(&d)).unwrap();
        assert_eq!(text.lines().count(), 2, "append-only: grant line + revoke line\n{text}");
    }

    #[test]
    fn scope_grants_refuse_home_and_hard_excludes() {
        let (d, _g) = dir("scope");
        let home = Path::new("/home/me");
        let all_home = Scope::Files("/home/me/".into());
        assert_eq!(
            grant_scope(&d, &all_home, Some(home), UserClick::from_click()).unwrap_err(),
            "files: one folder at a time, never all of your home folder"
        );
        assert_eq!(
            scope_refusal(&Scope::Files("/home".into()), Some(home)).as_deref(),
            Some("files: one folder at a time, never all of your home folder"),
            "a folder that holds the home folder is the home folder too"
        );
        assert_eq!(scope_refusal(&Scope::Files("/home/meadow".into()), Some(home)), None);
        assert_eq!(
            scope_refusal(&Scope::Files("/home/me/.ssh".into()), Some(home)).as_deref(),
            Some("files: that folder is always excluded")
        );
        assert_eq!(
            scope_refusal(&Scope::Files("Documents".into()), Some(home)).as_deref(),
            Some("files: needs one absolute folder")
        );
        let docs = Scope::Files("/home/me/Documents".into());
        let g = grant_scope(&d, &docs, Some(home), UserClick::from_click()).unwrap();
        assert_eq!(g.source, "files:/home/me/Documents");
        let ledger = ConsentLedger::load(&d);
        assert!(ledger.scope_grant(&docs).is_some());
        assert_eq!(ledger.scope_grant(&Scope::Files("/home/me/Pictures".into())), None);
        assert_eq!(ledger.scope_grant(&Scope::Calendar), None);
        assert!(scope_excluded("/home/me/.mozilla/firefox/x/Cookies"));
        assert!(!scope_excluded("/home/me/Documents/notes.md"));
        assert_eq!(Scope::parse("browser_history:firefox"), Some(Scope::BrowserHistory("firefox".into())));
        assert_eq!(Scope::parse("screen"), None, "the screen stays the desktop switch");
    }

    #[test]
    fn lines_not_by_the_user_never_count() {
        let text = concat!(
            r#"{"id":"g-1","destination":"hub","scope":"send","data_classes":["chat","personal"],"granted_at":1,"by":"agent"}"#,
            "\n",
            "not json\n",
            r#"{"id":"g-2","destination":"hub","scope":"send","data_classes":["chat"],"granted_at":2,"by":"user"}"#,
            "\n"
        );
        let ledger = ConsentLedger::from_lines(text);
        assert_eq!(ledger.all().len(), 1);
        assert_eq!(ledger.all()[0].id, "g-2");
        assert_eq!(ledger.destination_grant("hub", &[DataClass::Chat, DataClass::Personal]), None);
        assert!(ledger.destination_grant("hub", &[DataClass::Chat]).is_some());
    }

    #[test]
    fn the_ledger_is_sealed_and_a_locked_ledger_grants_nothing() {
        let (d, store, _g) = keyed_dir("sealed");
        let data = [DataClass::Chat, DataClass::Personal];
        let g = grant_destination(&d, HUB_DEST, &data, UserClick::from_click()).unwrap();
        let text = fs::read_to_string(consent_path(&d)).unwrap();
        assert!(text.lines().all(is_sealed_line), "{text}");
        assert!(!text.contains("hub") && !text.contains(&g.id) && !text.contains("user"), "{text}");
        assert!(ConsentLedger::load(&d).destination_grant(HUB_DEST, &data).is_some());

        // The keyring goes away (next start): no grant applies, nothing is written.
        store.set_available(false);
        use_key_store_for(&d, store.clone());
        let locked = ConsentLedger::load(&d);
        assert_eq!(locked.locked(), Some(&Locked::Unavailable));
        assert_eq!(locked.destination_grant(HUB_DEST, &data), None);
        assert_eq!(locked.active().count(), 0);
        let why = grant_scope(&d, &Scope::Calendar, None, UserClick::from_click()).unwrap_err();
        assert!(why.starts_with("Private data is locked"), "{why}");
        assert_eq!(revoke_grant(&d, &g.id), Err(Locked::Unavailable.message()));
        assert_eq!(fs::read_to_string(consent_path(&d)).unwrap(), text, "nothing written, nothing dropped");

        // It comes back: the grant is still there.
        store.set_available(true);
        use_key_store_for(&d, store.clone());
        assert!(ConsentLedger::load(&d).destination_grant(HUB_DEST, &data).is_some());
    }

    #[test]
    fn a_legacy_plaintext_ledger_keeps_working_and_is_sealed_on_the_next_write() {
        let (d, store, _g) = keyed_dir("legacy");
        let old = r#"{"id":"g-0000000000aa","destination":"hub","scope":"send","data_classes":["chat","personal"],"granted_at":1,"by":"user"}"#;
        fs::write(consent_path(&d), format!("{old}\n")).unwrap();
        let ledger = ConsentLedger::load(&d);
        assert_eq!(ledger.locked(), None);
        assert_eq!(ledger.active().count(), 1, "existing grants still count");
        assert!(!d.join(crate::harness::KEY_ID_FILE).exists(), "reading makes no key");

        // Without a keyring even the old plaintext ledger reads as locked.
        store.set_available(false);
        use_key_store_for(&d, store.clone());
        assert_eq!(ConsentLedger::load(&d).locked(), Some(&Locked::Unavailable));
        assert_eq!(ConsentLedger::load(&d).active().count(), 0);
        store.set_available(true);
        use_key_store_for(&d, store.clone());

        assert_eq!(revoke_grant(&d, "g-0000000000aa"), Ok(true));
        let text = fs::read_to_string(consent_path(&d)).unwrap();
        assert_eq!(text.lines().count(), 2, "the old line is kept, sealed in place\n{text}");
        assert!(text.lines().all(is_sealed_line), "{text}");
        let after = ConsentLedger::load(&d);
        assert_eq!(after.all().len(), 1);
        assert_eq!(after.active().count(), 0);
    }

    #[test]
    fn a_replayed_grant_line_cannot_undo_a_revoke() {
        let grant = r#"{"id":"g-1","destination":"hub","scope":"send","data_classes":["chat"],"granted_at":1,"by":"user"}"#;
        let revoke = r#"{"id":"g-1","destination":"hub","scope":"send","data_classes":["chat"],"granted_at":1,"by":"user","revoked_at":5}"#;
        let ledger = ConsentLedger::from_lines(&[grant, revoke, grant].join("\n"));
        assert_eq!(ledger.all().len(), 1);
        assert_eq!(ledger.active().count(), 0);
    }
}
