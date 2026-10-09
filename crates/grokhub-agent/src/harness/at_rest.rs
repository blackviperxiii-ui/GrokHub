//! Spike-4b: the learned tier at rest (harness design §12 P4).
//!
//! What is sealed: `consent.jsonl`, `egress.jsonl` (and its roll), and AMR
//! nodes marked `personal` or `sensitive` (`amr/nodes/<id>.sealed`). The
//! curated plain tier (SOUL / USER / MEMORY, `plain` AMR nodes) stays
//! `cat`-able. Each sealed line is ChaCha20-Poly1305 (`ring`) with a fresh
//! random 96-bit nonce and a per-file associated-data label, so a line cannot
//! be moved between files and any edit fails to open.
//!
//! The 32-byte key lives only in the OS keyring (Secret Service on Linux,
//! Credential Manager on Windows, Keychain on macOS) under service `GrokHub`,
//! account `learned-tier-key`. It is made on the first sealed write, never
//! overwritten, and never written to a file, a log, a ledger or a span.
//! `{config}/learned-key.id` holds only a short hash of it, so a different key
//! is caught instead of read as damage.
//!
//! Fail closed: no keyring, a missing key or the wrong key means learned data
//! is neither read nor written ([`Locked`]). There is no plaintext fallback.
//! Legacy plaintext files (from before 4b) are sealed in place on the first
//! write once a key exists; until then they keep reading as before when the
//! keyring answers. Nothing is ever dropped: unreadable lines stay on disk.
//!
//! Only `grokhub` itself talks to the OS keyring: `main` calls
//! [`use_os_keyring`]. Until then the store is "not available", so tests never
//! reach a real keyring; they register a [`MemoryKeyStore`].

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Every sealed line starts with this. Anything else in a sealed file is not ours.
pub const SEALED_PREFIX: &str = "gh-sealed:v1:";
/// OS keyring service and account for the learned-tier key.
pub const KEY_SERVICE: &str = "GrokHub";
pub const KEY_ACCOUNT: &str = "learned-tier-key";
/// `{config}/learned-key.id`: a 16-hex hash of the key, never the key.
pub const KEY_ID_FILE: &str = "learned-key.id";
const KEY_LOCK_FILE: &str = "learned-key.lock";
const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
/// How long a keyring failure or "no key yet" answer is reused before asking again.
const RECHECK: Duration = Duration::from_secs(30);
/// Largest sealed JSONL file scanned for sealed lines (matches the ledger cap).
const FILE_CAP: u64 = 1024 * 1024;
/// Largest file sealed in place: a rolled `egress.1.jsonl` is just over 1 MiB.
const MIGRATE_CAP: u64 = 2 * 1024 * 1024;

/// Associated data per file kind.
pub const AAD_CONSENT: &str = "grokhub:consent:v1";
pub const AAD_EGRESS: &str = "grokhub:egress:v1";

/// Why the learned tier is closed. No variant carries key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locked {
    /// The OS keyring did not answer (no Secret Service, locked, denied…).
    Unavailable,
    /// Sealed data or a key id is here, but the keyring has no key.
    Missing,
    /// The keyring key is not the one this data was sealed with.
    WrongKey,
    /// Another GrokHub process is making the key right now.
    Busy,
    /// The key is fine but the file could not be written (disk, permissions).
    Unwritable,
}

/// Which OS keyring the copy names. Picked with `cfg` at build time
/// ([`KeyringOs::current`]); the other values exist so every OS's words can be
/// tested on any OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyringOs {
    /// Linux and the other Unix desktops: Secret Service (GNOME Keyring, KWallet).
    Linux,
    Windows,
    MacOs,
}

impl KeyringOs {
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// The store's name on this OS, for messages.
pub fn keyring_name() -> &'static str {
    keyring_name_for(KeyringOs::current())
}

/// The store's name on `os`. Windows and macOS never say "Secret Service".
pub fn keyring_name_for(os: KeyringOs) -> &'static str {
    match os {
        KeyringOs::Windows => "Windows Credential Manager",
        KeyringOs::MacOs => "macOS Keychain",
        KeyringOs::Linux => "Secret Service keyring",
    }
}

impl Locked {
    /// One plain sentence for Settings, `/privacy` and `/recall`.
    pub fn message(&self) -> String {
        self.message_for(KeyringOs::current())
    }

    /// [`Self::message`] as it reads on `os`.
    pub fn message_for(&self, os: KeyringOs) -> String {
        let what = "GrokHub won't read or save grants, the send log or private memory until then, and never saves them as plain text.";
        let name = keyring_name_for(os);
        match self {
            Self::Unavailable => format!("Private data is locked: GrokHub can't reach your {name}. {what}"),
            Self::Missing => {
                format!("Private data is locked: its key is missing from your {name}. {what} Nothing was deleted.")
            }
            Self::WrongKey => format!(
                "Private data is locked: the key in your {name} doesn't match this data. {what} Nothing was deleted."
            ),
            Self::Busy => "Private data is busy: another GrokHub window is setting up its key. Try again in a moment.".into(),
            Self::Unwritable => {
                "Private data wasn't saved: GrokHub couldn't write the file. Nothing was saved as plain text.".into()
            }
        }
    }

    /// Short form for a status line after a click.
    pub fn short(&self) -> &'static str {
        match self {
            Self::Unavailable => "private data is locked (no keyring)",
            Self::Missing => "private data is locked (key missing)",
            Self::WrongKey => "private data is locked (wrong key)",
            Self::Busy => "private data is busy, try again",
            Self::Unwritable => "private data couldn't be written",
        }
    }
}

/// Where the key lives. `load` = `Ok(None)` when there is no entry,
/// `Err` when the store itself can't be used. `create` must not be called
/// over an existing entry.
pub trait KeyStore: Send + Sync {
    fn load(&self) -> Result<Option<Zeroizing<String>>, String>;
    fn create(&self, key_b64: &str) -> Result<(), String>;
    /// True when a call can wait on the OS (D-Bus, an unlock prompt).
    fn may_block(&self) -> bool {
        true
    }
}

/// The OS keyring through `keyring` 4 (v1 mode).
pub struct OsKeyring;

impl KeyStore for OsKeyring {
    fn load(&self) -> Result<Option<Zeroizing<String>>, String> {
        let entry = keyring::Entry::new(KEY_SERVICE, KEY_ACCOUNT).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(text) => Ok(Some(Zeroizing::new(text))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn create(&self, key_b64: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(KEY_SERVICE, KEY_ACCOUNT).map_err(|e| e.to_string())?;
        entry.set_password(key_b64).map_err(|e| e.to_string())
    }
}

/// In-memory store for tests and the hermetic CI run. Never touches the OS.
#[derive(Default)]
pub struct MemoryKeyStore {
    key: Mutex<Option<Zeroizing<String>>>,
    down: AtomicBool,
}

impl MemoryKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store that answers like a keyring that isn't there.
    pub fn unavailable() -> Self {
        let s = Self::default();
        s.down.store(true, Ordering::SeqCst);
        s
    }

    pub fn set_available(&self, up: bool) {
        self.down.store(!up, Ordering::SeqCst);
    }

    /// Drop the key, as if someone deleted the keyring entry.
    pub fn forget(&self) {
        *self.key.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Replace the key (tests: a different key than the data was sealed with).
    pub fn replace(&self, key_b64: &str) {
        *self.key.lock().unwrap_or_else(|e| e.into_inner()) = Some(Zeroizing::new(key_b64.to_string()));
    }
}

impl KeyStore for MemoryKeyStore {
    fn load(&self) -> Result<Option<Zeroizing<String>>, String> {
        if self.down.load(Ordering::SeqCst) {
            return Err("keyring not available".into());
        }
        Ok(self.key.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    fn create(&self, key_b64: &str) -> Result<(), String> {
        if self.down.load(Ordering::SeqCst) {
            return Err("keyring not available".into());
        }
        let mut slot = self.key.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(Zeroizing::new(key_b64.to_string()));
        }
        Ok(())
    }

    fn may_block(&self) -> bool {
        false
    }
}

/// A store that is never there: what runs until `main` installs the OS one.
struct NoStore;

impl KeyStore for NoStore {
    fn load(&self) -> Result<Option<Zeroizing<String>>, String> {
        Err("no key store installed".into())
    }
    fn create(&self, _key_b64: &str) -> Result<(), String> {
        Err("no key store installed".into())
    }
    fn may_block(&self) -> bool {
        false
    }
}

/// The open key. `Debug` shows the id only.
pub struct LearnedKey {
    key: LessSafeKey,
    id: String,
}

impl std::fmt::Debug for LearnedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LearnedKey({})", self.id)
    }
}

impl LearnedKey {
    fn from_b64(text: &str) -> Option<Self> {
        let raw = Zeroizing::new(base64::engine::general_purpose::STANDARD.decode(text.trim()).ok()?);
        if raw.len() != KEY_LEN {
            return None;
        }
        let key = LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, &raw).ok()?);
        Some(Self { key, id: key_id(&raw) })
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

fn key_id(raw: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b"grokhub-learned-key-id:v1:");
    h.update(raw);
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Seal `plain` under `aad`. One line, no newline.
pub fn seal_with(key: &LearnedKey, aad: &str, plain: &[u8]) -> Result<String, String> {
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new().fill(&mut nonce).map_err(|_| "no random source".to_string())?;
    let mut buf = Zeroizing::new(plain.to_vec());
    key.key
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(aad.as_bytes()), &mut *buf)
        .map_err(|_| "seal failed".to_string())?;
    let mut out = Vec::with_capacity(NONCE_LEN + buf.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&buf);
    Ok(format!("{SEALED_PREFIX}{}", base64::engine::general_purpose::STANDARD.encode(out)))
}

/// Open one sealed line. `None` for anything that is not an intact line
/// sealed under this key and `aad` (tampered, truncated, other file, other key).
pub fn open_with(key: &LearnedKey, aad: &str, line: &str) -> Option<Zeroizing<Vec<u8>>> {
    let body = line.trim().strip_prefix(SEALED_PREFIX)?;
    let raw = base64::engine::general_purpose::STANDARD.decode(body).ok()?;
    if raw.len() < NONCE_LEN + TAG_LEN {
        return None;
    }
    let (nonce, ct) = raw.split_at(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
    let mut buf = Zeroizing::new(ct.to_vec());
    let plain_len = key.key.open_in_place(nonce, Aad::from(aad.as_bytes()), &mut buf).ok()?.len();
    buf.truncate(plain_len);
    Some(buf)
}

pub fn is_sealed_line(line: &str) -> bool {
    line.trim_start().starts_with(SEALED_PREFIX)
}

/// A key store plus its cache. One per process by default; tests register
/// their own per config dir.
pub struct Keys {
    store: Arc<dyn KeyStore>,
    cache: Mutex<KeyCache>,
    fetching: AtomicBool,
}

#[derive(Clone)]
enum KeyCache {
    Unknown,
    Have(Arc<LearnedKey>, Instant),
    Absent(Instant),
    Down(Instant),
}

impl Keys {
    pub fn new(store: Arc<dyn KeyStore>) -> Arc<Self> {
        Arc::new(Self { store, cache: Mutex::new(KeyCache::Unknown), fetching: AtomicBool::new(false) })
    }

    fn cached(&self) -> KeyCache {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, c: KeyCache) {
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = c;
    }

    /// Ask the store. `Err(())` = the store is down.
    fn fetch(&self) -> Result<Option<Arc<LearnedKey>>, ()> {
        match self.store.load() {
            Ok(Some(text)) => match LearnedKey::from_b64(&text) {
                Some(k) => {
                    let k = Arc::new(k);
                    self.set(KeyCache::Have(k.clone(), Instant::now()));
                    Ok(Some(k))
                }
                // A keyring entry that isn't a 32-byte key can't open anything.
                None => {
                    self.set(KeyCache::Down(Instant::now()));
                    Err(())
                }
            },
            Ok(None) => {
                self.set(KeyCache::Absent(Instant::now()));
                Ok(None)
            }
            Err(_) => {
                self.set(KeyCache::Down(Instant::now()));
                Err(())
            }
        }
    }

    /// The key, from cache when fresh. `wait = false` never blocks on a store
    /// that may block: it starts a background fetch and returns `None`.
    fn key(self: &Arc<Self>, wait: bool) -> Option<Result<Option<Arc<LearnedKey>>, ()>> {
        match self.cached() {
            KeyCache::Have(k, _) => return Some(Ok(Some(k))),
            KeyCache::Absent(at) if at.elapsed() < RECHECK => return Some(Ok(None)),
            KeyCache::Down(at) if at.elapsed() < RECHECK => return Some(Err(())),
            _ => {}
        }
        if wait || !self.store.may_block() {
            return Some(self.fetch());
        }
        if !self.fetching.swap(true, Ordering::SeqCst) {
            let me = self.clone();
            std::thread::spawn(move || {
                let _ = me.fetch();
                me.fetching.store(false, Ordering::SeqCst);
            });
        }
        None
    }
}

fn default_slot() -> &'static RwLock<Arc<Keys>> {
    static SLOT: OnceLock<RwLock<Arc<Keys>>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(Keys::new(Arc::new(NoStore))))
}

/// Per-config-dir key stores (tests).
type Overrides = Mutex<Vec<(PathBuf, Arc<Keys>)>>;

fn overrides() -> &'static Overrides {
    static O: OnceLock<Overrides> = OnceLock::new();
    O.get_or_init(|| Mutex::new(Vec::new()))
}

/// Forget the last keyring answer for this config dir so the next read asks
/// the keyring again now instead of after `RECHECK` (Settings → Permissions →
/// Try again, SB-02). It only drops a cached answer: a locked tier stays
/// locked until the keyring itself answers with the right key.
pub fn recheck_keyring(config_dir: &Path) {
    keys_for(config_dir).set(KeyCache::Unknown);
}

/// `grokhub` calls this once at start: the learned-tier key lives in the OS keyring.
pub fn use_os_keyring() {
    set_default_key_store(Arc::new(OsKeyring));
}

/// Replace the process-wide store (and drop its cached key).
pub fn set_default_key_store(store: Arc<dyn KeyStore>) {
    *default_slot().write().unwrap_or_else(|e| e.into_inner()) = Keys::new(store);
}

/// Use `store` for this config dir only (tests, so parallel tests don't share a lock state).
pub fn use_key_store_for(config_dir: &Path, store: Arc<dyn KeyStore>) {
    let mut o = overrides().lock().unwrap_or_else(|e| e.into_inner());
    o.retain(|(d, _)| d != config_dir);
    o.push((config_dir.to_path_buf(), Keys::new(store)));
}

fn keys_for(config_dir: &Path) -> Arc<Keys> {
    if let Some((_, k)) = overrides()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(d, _)| d == config_dir)
    {
        return k.clone();
    }
    default_slot().read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn key_id_path(config_dir: &Path) -> PathBuf {
    config_dir.join(KEY_ID_FILE)
}

fn stored_key_id(config_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(key_id_path(config_dir)).ok()?;
    let id = text.trim().to_string();
    (!id.is_empty()).then_some(id)
}

/// True when this config dir already holds sealed learned data.
pub fn has_sealed_data(config_dir: &Path) -> bool {
    let sealed_file = |p: PathBuf| -> bool {
        let Ok(f) = fs::File::open(p) else {
            return false;
        };
        let mut text = String::new();
        if f.take(FILE_CAP).read_to_string(&mut text).is_err() {
            return false;
        }
        text.lines().any(is_sealed_line)
    };
    if sealed_file(config_dir.join(super::consent::CONSENT_FILE))
        || sealed_file(config_dir.join(super::egress::EGRESS_FILE))
        || sealed_file(config_dir.join("egress.1.jsonl"))
    {
        return true;
    }
    fs::read_dir(config_dir.join("amr").join("nodes"))
        .map(|rd| {
            rd.flatten()
                .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("sealed"))
        })
        .unwrap_or(false)
}

/// The key for reading, or why not. `Ok(None)`: no key exists and nothing
/// here is sealed, so there is nothing to open (fresh or legacy config).
/// `None`: the answer isn't ready yet (`wait = false` on a blocking store).
pub fn read_key(config_dir: &Path, wait: bool) -> Option<Result<Option<Arc<LearnedKey>>, Locked>> {
    let _lap = crate::timing::lap("harness:keyring_read");
    let keys = keys_for(config_dir);
    Some(match keys.key(wait)? {
        Err(()) => Err(Locked::Unavailable),
        Ok(Some(k)) => match stored_key_id(config_dir) {
            Some(id) if id != k.id => Err(Locked::WrongKey),
            _ => Ok(Some(k)),
        },
        Ok(None) => {
            if stored_key_id(config_dir).is_some() || has_sealed_data(config_dir) {
                Err(Locked::Missing)
            } else {
                Ok(None)
            }
        }
    })
}

/// The key for writing. Makes it on first use (only when nothing here is
/// sealed and no key id is recorded), stores it in the keyring, reads it
/// back, then records its id. Never replaces a keyring entry.
pub fn write_key(config_dir: &Path) -> Result<Arc<LearnedKey>, Locked> {
    let _lap = crate::timing::lap("harness:keyring_write");
    let keys = keys_for(config_dir);
    // Writes re-ask the keyring once the cached answer is stale, so a key
    // deleted from the keyring stops new sealing within `RECHECK`.
    let have = match keys.cached() {
        KeyCache::Have(k, at) if at.elapsed() < RECHECK => Some(k),
        KeyCache::Down(at) if at.elapsed() < RECHECK => return Err(Locked::Unavailable),
        _ => match keys.fetch() {
            Ok(k) => k,
            Err(()) => return Err(Locked::Unavailable),
        },
    };
    if let Some(k) = have {
        match stored_key_id(config_dir) {
            Some(id) if id != k.id => return Err(Locked::WrongKey),
            Some(_) => {}
            None => record_key_id(config_dir, &k.id).map_err(|_| Locked::Unwritable)?,
        }
        return Ok(k);
    }
    if stored_key_id(config_dir).is_some() || has_sealed_data(config_dir) {
        return Err(Locked::Missing);
    }
    fs::create_dir_all(config_dir).map_err(|_| Locked::Unwritable)?;
    let lock = config_dir.join(KEY_LOCK_FILE);
    // A lock left by a crash goes stale; key setup takes well under this.
    let stale = fs::metadata(&lock)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > RECHECK);
    if stale {
        let _ = fs::remove_file(&lock);
    }
    if OpenOptions::new().write(true).create_new(true).open(&lock).is_err() {
        return Err(Locked::Busy);
    }
    let made = make_key(&keys, config_dir);
    let _ = fs::remove_file(&lock);
    made
}

fn make_key(keys: &Keys, config_dir: &Path) -> Result<Arc<LearnedKey>, Locked> {
    let mut raw = Zeroizing::new([0u8; KEY_LEN]);
    SystemRandom::new().fill(&mut *raw).map_err(|_| Locked::Unavailable)?;
    let text = Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(*raw));
    keys.store.create(&text).map_err(|_| Locked::Unavailable)?;
    // Read it back: whatever the keyring holds now is the key (another
    // process may have won a race; never seal with an unverified key).
    let k = match keys.fetch() {
        Ok(Some(k)) => k,
        _ => return Err(Locked::Unavailable),
    };
    record_key_id(config_dir, &k.id).map_err(|_| Locked::Unwritable)?;
    Ok(k)
}

fn record_key_id(config_dir: &Path, id: &str) -> Result<(), String> {
    write_private(&key_id_path(config_dir), &format!("{id}\n"))
}

/// Write a whole file through a 0600 temp file and a rename.
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("no parent dir")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

/// What a sealed JSONL file reads as.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SealedRead {
    /// Plaintext JSON lines, in file order.
    pub lines: Vec<String>,
    /// Set when sealed lines (or the whole file) could not be opened.
    pub locked: Option<Locked>,
    /// Sealed lines that did not open under the key (damaged or edited), plus
    /// plaintext lines found in a sealed file (never ours, never trusted).
    pub unreadable: usize,
    /// The keyring hasn't answered yet (non-blocking read).
    pub pending: bool,
}

/// Read a JSONL file of the learned tier. A legacy file (no sealed line) is
/// read as plaintext when the keyring answers; a sealed file needs the key.
pub fn read_sealed_jsonl(config_dir: &Path, text: &str, aad: &str, wait: bool) -> SealedRead {
    let _lap = crate::timing::lap("harness:sealed_read");
    let rows: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if rows.is_empty() {
        return SealedRead::default();
    }
    let Some(key) = read_key(config_dir, wait) else {
        return SealedRead { pending: true, ..SealedRead::default() };
    };
    let key = match key {
        Ok(k) => k,
        Err(why) => return SealedRead { locked: Some(why), ..SealedRead::default() },
    };
    let sealed = rows.iter().any(|l| is_sealed_line(l));
    if !sealed {
        return SealedRead { lines: rows.iter().map(|l| l.to_string()).collect(), ..SealedRead::default() };
    }
    let Some(key) = key else {
        return SealedRead { locked: Some(Locked::Missing), ..SealedRead::default() };
    };
    let mut out = SealedRead::default();
    for row in rows {
        if !is_sealed_line(row) {
            out.unreadable += 1;
            continue;
        }
        match open_with(&key, aad, row).and_then(|p| String::from_utf8(p.to_vec()).ok()) {
            Some(line) => out.lines.push(line),
            None => out.unreadable += 1,
        }
    }
    out
}

/// Append one plaintext line sealed. A legacy plaintext file is sealed in
/// place first (every line kept, order kept) so no plaintext is left behind.
/// Fails closed: no key means nothing is written.
pub fn append_sealed_line(config_dir: &Path, path: &Path, aad: &str, line: &str) -> Result<(), Locked> {
    let _lap = crate::timing::lap("harness:sealed_append");
    let key = write_key(config_dir)?;
    migrate_file(&key, path, aad).map_err(|_| Locked::Unwritable)?;
    let sealed = seal_with(&key, aad, line.as_bytes()).map_err(|_| Locked::Unwritable)?;
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path).map_err(|_| Locked::Unwritable)?;
    writeln!(f, "{sealed}").map_err(|_| Locked::Unwritable)
}

/// Seal every plaintext line of `path` in place. No-op when the file is
/// missing or already fully sealed. Over the cap it is left alone (and its
/// readers already treat it as empty).
pub fn migrate_file(key: &LearnedKey, path: &Path, aad: &str) -> Result<bool, String> {
    let Ok(mut f) = fs::File::open(path) else {
        return Ok(false);
    };
    if f.metadata().map(|m| m.len() > MIGRATE_CAP).unwrap_or(true) {
        return Ok(false);
    }
    // Every write seals, so a file whose first line is sealed was migrated
    // already; this check reads one small chunk, not the whole log.
    let mut head = [0u8; 64];
    let n = f.read(&mut head).map_err(|e| e.to_string())?;
    let head = String::from_utf8_lossy(&head[..n]);
    if head.trim_start().is_empty() || is_sealed_line(head.trim_start()) {
        return Ok(false);
    }
    let mut text = head.into_owned();
    f.take(MIGRATE_CAP).read_to_string(&mut text).map_err(|e| e.to_string())?;
    let mut out = String::with_capacity(text.len() * 2);
    for row in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if is_sealed_line(row) {
            out.push_str(row);
        } else {
            out.push_str(&seal_with(key, aad, row.as_bytes())?);
        }
        out.push('\n');
    }
    write_private(path, &out)?;
    Ok(true)
}

/// `amr/nodes/<id>.sealed` through the same key: the core AMR store's sealer.
#[derive(Debug, Clone)]
pub struct LearnedVault {
    config_dir: PathBuf,
}

impl LearnedVault {
    pub fn new(config_dir: &Path) -> Self {
        Self { config_dir: config_dir.to_path_buf() }
    }
}

impl grokhub_core::amr::Sealer for LearnedVault {
    fn seal(&self, aad: &str, plain: &str) -> Result<String, String> {
        let key = write_key(&self.config_dir).map_err(|l| l.message())?;
        seal_with(&key, aad, plain.as_bytes())
    }

    fn open(&self, aad: &str, sealed: &str) -> Result<String, String> {
        match read_key(&self.config_dir, true) {
            Some(Ok(Some(key))) => open_with(&key, aad, sealed)
                .and_then(|p| String::from_utf8(p.to_vec()).ok())
                .ok_or_else(|| "a private memory could not be opened (damaged or edited)".to_string()),
            Some(Ok(None)) => Err(Locked::Missing.message()),
            Some(Err(why)) => Err(why.message()),
            None => Err(Locked::Unavailable.message()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::amr::Sealer;

    struct DirGuard(PathBuf);
    impl Drop for DirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A test dir with its own in-memory keyring (returned so tests can flip it).
    fn dir(label: &str) -> (PathBuf, Arc<MemoryKeyStore>, DirGuard) {
        let p = crate::harness::test_dir(&format!("at-rest-{label}"));
        let store = Arc::new(MemoryKeyStore::new());
        use_key_store_for(&p, store.clone());
        (p.clone(), store, DirGuard(p))
    }

    #[test]
    fn seal_then_open_round_trips_and_hides_the_text() {
        let (d, _s, _g) = dir("round");
        let key = write_key(&d).unwrap();
        let plain = r#"{"id":"g-1","destination":"hub","by":"user"}"#;
        let line = seal_with(&key, AAD_CONSENT, plain.as_bytes()).unwrap();
        assert!(line.starts_with("gh-sealed:v1:"), "{line}");
        assert!(!line.contains("hub") && !line.contains("g-1"), "{line}");
        // nonce 12 + text + tag 16, base64.
        let raw = base64::engine::general_purpose::STANDARD.decode(&line["gh-sealed:v1:".len()..]).unwrap();
        assert_eq!(raw.len(), 12 + plain.len() + 16);
        assert_eq!(&*open_with(&key, AAD_CONSENT, &line).unwrap(), plain.as_bytes());
        let again = seal_with(&key, AAD_CONSENT, plain.as_bytes()).unwrap();
        assert_ne!(line, again, "a fresh nonce every time");
        // The key id is recorded; the key itself is nowhere on disk.
        let id = fs::read_to_string(d.join(KEY_ID_FILE)).unwrap();
        assert_eq!(id.trim(), key.id());
        assert_eq!(id.trim().len(), 16);
        assert_eq!(format!("{key:?}"), format!("LearnedKey({})", key.id()));
    }

    #[test]
    fn tampered_or_moved_ciphertext_is_rejected() {
        let (d, _s, _g) = dir("tamper");
        let key = write_key(&d).unwrap();
        let line = seal_with(&key, AAD_CONSENT, b"{\"by\":\"user\"}").unwrap();
        let body = &line[SEALED_PREFIX.len()..];
        let mut raw = base64::engine::general_purpose::STANDARD.decode(body).unwrap();
        for at in [0, 12, raw.len() - 1] {
            let mut bad = raw.clone();
            bad[at] ^= 0x01;
            let bad = format!("{SEALED_PREFIX}{}", base64::engine::general_purpose::STANDARD.encode(&bad));
            assert_eq!(open_with(&key, AAD_CONSENT, &bad), None, "flipped byte {at}");
        }
        raw.truncate(raw.len() - 1);
        let cut = format!("{SEALED_PREFIX}{}", base64::engine::general_purpose::STANDARD.encode(&raw));
        assert_eq!(open_with(&key, AAD_CONSENT, &cut), None, "truncated");
        assert_eq!(open_with(&key, AAD_EGRESS, &line), None, "a consent line can't be read as egress");
        assert_eq!(open_with(&key, AAD_CONSENT, "gh-sealed:v1:not base64!"), None);
        assert_eq!(open_with(&key, AAD_CONSENT, "{\"by\":\"user\"}"), None, "plaintext is not sealed");
    }

    #[test]
    fn no_keyring_fails_closed_and_writes_nothing() {
        let (d, store, _g) = dir("down");
        store.set_available(false);
        let path = d.join("consent.jsonl");
        assert_eq!(append_sealed_line(&d, &path, AAD_CONSENT, "{\"by\":\"user\"}"), Err(Locked::Unavailable));
        assert!(!path.exists(), "no plaintext fallback");
        assert!(!d.join(KEY_ID_FILE).exists());
        assert_eq!(read_key(&d, true).unwrap().unwrap_err(), Locked::Unavailable);
        let vault = LearnedVault::new(&d);
        assert_eq!(vault.seal("a", "b").unwrap_err(), Locked::Unavailable.message());
        assert!(Locked::Unavailable.message().starts_with("Private data is locked: GrokHub can't reach your "));
    }

    #[test]
    fn missing_or_wrong_key_fails_closed_and_never_replaces_the_key() {
        let (d, store, _g) = dir("missing");
        let path = d.join("egress.jsonl");
        append_sealed_line(&d, &path, AAD_EGRESS, "{\"dest\":\"api.x.ai\"}").unwrap();
        let before = fs::read_to_string(&path).unwrap();
        store.forget();
        use_key_store_for(&d, store.clone()); // a restart: nothing cached
        assert_eq!(read_key(&d, true).unwrap().unwrap_err(), Locked::Missing);
        assert_eq!(append_sealed_line(&d, &path, AAD_EGRESS, "{\"dest\":\"x.ai\"}"), Err(Locked::Missing));
        assert_eq!(store.load().unwrap(), None, "no new key over sealed data");
        assert_eq!(fs::read_to_string(&path).unwrap(), before, "nothing dropped, nothing added");
        let other = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
        store.replace(&other);
        use_key_store_for(&d, store.clone());
        assert_eq!(read_key(&d, true).unwrap().unwrap_err(), Locked::WrongKey);
        let read = read_sealed_jsonl(&d, &before, AAD_EGRESS, true);
        assert_eq!(read, SealedRead { locked: Some(Locked::WrongKey), ..SealedRead::default() });
    }

    #[test]
    fn legacy_plaintext_reads_then_is_sealed_in_place_on_first_write() {
        let (d, _s, _g) = dir("legacy");
        let path = d.join("consent.jsonl");
        let old = "{\"id\":\"g-old\",\"by\":\"user\"}\nnot json but kept\n";
        fs::write(&path, old).unwrap();
        let read = read_sealed_jsonl(&d, old, AAD_CONSENT, true);
        assert_eq!(read.lines, vec!["{\"id\":\"g-old\",\"by\":\"user\"}", "not json but kept"]);
        assert_eq!(read.locked, None);
        assert!(!d.join(KEY_ID_FILE).exists(), "reading makes no key");
        append_sealed_line(&d, &path, AAD_CONSENT, "{\"id\":\"g-new\",\"by\":\"user\"}").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(text.lines().all(is_sealed_line), "{text}");
        assert!(!text.contains("g-old") && !text.contains("kept"), "{text}");
        let read = read_sealed_jsonl(&d, &text, AAD_CONSENT, true);
        assert_eq!(
            read.lines,
            vec!["{\"id\":\"g-old\",\"by\":\"user\"}", "not json but kept", "{\"id\":\"g-new\",\"by\":\"user\"}"]
        );
        // A plaintext line slipped into a sealed file is never trusted.
        fs::write(&path, format!("{text}{{\"id\":\"g-forged\",\"by\":\"user\"}}\n")).unwrap();
        let read = read_sealed_jsonl(&d, &fs::read_to_string(&path).unwrap(), AAD_CONSENT, true);
        assert_eq!(read.lines.len(), 3);
        assert_eq!(read.unreadable, 1);
    }

    #[test]
    fn a_blocking_store_never_blocks_the_ui_read() {
        struct Slow(MemoryKeyStore);
        impl KeyStore for Slow {
            fn load(&self) -> Result<Option<Zeroizing<String>>, String> {
                std::thread::sleep(Duration::from_millis(50));
                self.0.load()
            }
            fn create(&self, k: &str) -> Result<(), String> {
                self.0.create(k)
            }
        }
        let p = crate::harness::test_dir("at-rest-slow");
        let _g = DirGuard(p.clone());
        use_key_store_for(&p, Arc::new(Slow(MemoryKeyStore::new())));
        let path = p.join("consent.jsonl");
        append_sealed_line(&p, &path, AAD_CONSENT, "{\"by\":\"user\"}").unwrap();
        // A fresh Keys (as after a restart) answers "not yet" without waiting.
        use_key_store_for(&p, Arc::new(Slow(MemoryKeyStore::new())));
        let text = fs::read_to_string(&path).unwrap();
        let first = read_sealed_jsonl(&p, &text, AAD_CONSENT, false);
        assert!(first.pending, "{first:?}");
        assert!(first.lines.is_empty());
    }

    #[test]
    fn private_memory_nodes_seal_with_the_learned_key() {
        use grokhub_core::amr::{AmrError, AmrStore, NodeDraft, NodeType, Sensitivity};
        let (d, store, _g) = dir("amr");
        let amr = AmrStore::at(d.join("amr")).with_sealer(Arc::new(LearnedVault::new(&d)));
        amr.init().unwrap();
        let draft = NodeDraft {
            id: "fact-home".into(),
            node_type: NodeType::Fact,
            created: "2026-10-07T00:00:00Z".into(),
            updated: "2026-10-07T00:00:00Z".into(),
            source: "user".into(),
            confidence: 0.9,
            tags: vec!["home".into()],
            body: "Home harbor is Pier 9.".into(),
            sensitivity: Sensitivity::Personal,
            consent_ref: String::new(),
        };
        amr.remember(&draft).unwrap();
        let raw = fs::read_to_string(d.join("amr/nodes/fact-home.sealed")).unwrap();
        assert!(is_sealed_line(raw.trim()) && !raw.contains("Pier"), "{raw}");
        assert_eq!(amr.recall("pier 9").len(), 1);
        assert!(has_sealed_data(&d));

        // A sealed node renamed onto another id doesn't open (the id is bound in).
        fs::copy(d.join("amr/nodes/fact-home.sealed"), d.join("amr/nodes/fact-boat.sealed")).unwrap();
        assert_eq!(amr.recall_report("pier 9").locked, 1);
        fs::remove_file(d.join("amr/nodes/fact-boat.sealed")).unwrap();

        store.set_available(false);
        use_key_store_for(&d, store.clone());
        let report = amr.recall_report("pier 9");
        assert_eq!((report.hits.len(), report.locked), (0, 1));
        assert_eq!(report.why, Some(Locked::Unavailable.message()));
        let next = NodeDraft { id: "fact-boat".into(), ..draft };
        assert_eq!(amr.remember(&next), Err(AmrError::Paused(Locked::Unavailable.message())));
        assert!(!d.join("amr/nodes/fact-boat.sealed").exists() && !d.join("amr/nodes/fact-boat.md").exists());
    }
}
