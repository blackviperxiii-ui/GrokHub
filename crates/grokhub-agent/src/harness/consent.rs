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
//! is active. Nothing reads a scope yet; the indexers arrive in Spike-8.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    ("files", "Files in one folder"),
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
    let p = path.replace('\\', "/");
    SCOPE_HARD_EXCLUDES.iter().any(|x| {
        p == *x
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
        if d == h.trim_end_matches('/') {
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
}

impl ConsentLedger {
    /// No grants: what a fresh config reads as.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Read `{config_dir}/consent.jsonl`. Missing, unreadable, or oversized
    /// reads as empty (fail closed). Bad lines are skipped.
    pub fn load(config_dir: &Path) -> Self {
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
        Self::from_lines(&text)
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
                Some(slot) => *slot = g,
                None => grants.push(g),
            }
        }
        Self { grants }
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

fn append_line(config_dir: &Path, g: &Grant) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let line = serde_json::to_string(g).map_err(|e| e.to_string())?;
    let line = grokhub_core::redact_secrets(&line);
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(consent_path(config_dir)).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
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
}
