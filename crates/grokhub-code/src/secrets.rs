//! Code tab slice 5b: per-project secrets for deploy runs.
//!
//! Only the secret NAMES are written to disk, in `code-secrets.json` in the
//! app config folder, keyed by the project's repo key (never inside the repo,
//! so they can't be committed). The VALUES live in the OS keychain under the
//! app's service, account `code-secret:<repoKey>:<NAME>`. A value is read only
//! to put it in a child process's environment at spawn time, and every line of
//! that child's output passes through [`redact`] before it reaches the UI, the
//! activity log or the agent. Nothing here ever returns a value to the window.

use crate::runs::repo_key;
use grokhub_core::redact::{redact_held_secrets, redact_secrets};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// The keychain service: the same one the shell's API key uses.
pub const SERVICE: &str = grokhub_core::brand::INTERNAL_ID;

/// Names only, in the app config folder.
pub const FILE: &str = "code-secrets.json";
/// What a secret value is replaced with in any output.
pub const MARKER: &str = "[redacted]";
/// Shorter values can't be hidden in output without hiding ordinary words,
/// so they aren't accepted.
pub const MIN_VALUE_CHARS: usize = 4;
/// The longest name accepted.
pub const MAX_NAME_CHARS: usize = 64;
pub const NEEDS_CONFIRM: &str = "Deleting a secret needs a confirmation first.";

/// Names a deploy secret may not take: they would change how the child runs.
const RESERVED: &[&str] = &["PATH", "HOME", "SHELL", "TERM", "GIT_TERMINAL_PROMPT"];
const RESERVED_PREFIXES: &[&str] = &["LD_", "DYLD_"];

/// Where values are kept. The app uses [`Keychain`]; tests use an in-memory vault.
pub trait Vault {
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    /// Deleting a value that isn't there is not an error.
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// The OS keychain, under the same service as the shell's API key.
pub struct Keychain;

/// A keychain failure in plain words, without the library's own wording.
fn keychain_message(err: keyring::Error) -> String {
    match err {
        // The keychain is looked for once, the first time it is used: one
        // that starts later is not seen until the app starts again.
        keyring::Error::NoDefaultStore => {
            "No system keychain is running. Start one, then restart the app.".into()
        }
        keyring::Error::TooLong(_, limit) => {
            format!("That is too long for the system keychain (its limit is {limit}).")
        }
        _ => "The system keychain is not available.".into(),
    }
}

fn entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account).map_err(keychain_message)
}

impl Vault for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        match entry(account)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(keychain_message(err)),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        entry(account)?
            .set_password(value)
            .map_err(keychain_message)
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(keychain_message(err)),
        }
    }
}

/// What the window sees: names, never values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretList {
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    entries: Vec<Named>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Named {
    repo_key: String,
    name: String,
}

static WRITE: Mutex<()> = Mutex::new(());

/// The keychain account for one project's secret.
pub fn account(repo_key: &str, name: &str) -> String {
    format!("code-secret:{repo_key}:{name}")
}

/// An env var name: a letter or `_` first, then letters, digits and `_`, up to
/// [`MAX_NAME_CHARS`]. Names that steer how a program runs are refused.
pub fn name_ok(name: &str) -> Result<(), String> {
    let bad = || {
        format!(
            "\"{name}\" isn't a secret name the app uses. Use letters, digits and _ (like DEPLOY_TOKEN), starting with a letter or _."
        )
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return Err(bad()),
    }
    if name.chars().count() > MAX_NAME_CHARS
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(bad());
    }
    let upper = name.to_ascii_uppercase();
    if RESERVED.contains(&upper.as_str()) || RESERVED_PREFIXES.iter().any(|p| upper.starts_with(p))
    {
        return Err(format!(
            "{name} changes how programs run, so it can't be a deploy secret."
        ));
    }
    Ok(())
}

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

fn load(config_dir: &Path) -> Store {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(config_dir: &Path, store: &Store) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

fn names_in(store: &Store, repo_key: &str) -> Vec<String> {
    let mut names: Vec<String> = store
        .entries
        .iter()
        .filter(|e| e.repo_key == repo_key)
        .map(|e| e.name.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// This project's secret names, sorted. Values are never read here.
pub fn list(config_dir: &Path, repo_key: &str) -> SecretList {
    SecretList {
        names: names_in(&load(config_dir), repo_key),
    }
}

/// Add a secret, or replace the value of one that's already there.
pub fn set(
    config_dir: &Path,
    vault: &dyn Vault,
    repo_key: &str,
    name: &str,
    value: &str,
) -> Result<SecretList, String> {
    let name = name.trim();
    name_ok(name)?;
    // A pasted value often carries a trailing newline; spaces at either end are
    // never part of a token.
    let value = value.trim();
    if value.is_empty() {
        return Err("Paste the secret's value first.".into());
    }
    if value.chars().count() < MIN_VALUE_CHARS {
        return Err(format!(
            "A secret needs at least {MIN_VALUE_CHARS} characters so it can be hidden in output."
        ));
    }
    let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut store = load(config_dir);
    let known = store
        .entries
        .iter()
        .any(|e| e.repo_key == repo_key && e.name == name);
    vault.set(&account(repo_key, name), value)?;
    if !known {
        store.entries.push(Named {
            repo_key: repo_key.to_string(),
            name: name.to_string(),
        });
        if let Err(err) = save(config_dir, &store) {
            // Don't leave a value in the keychain that no list shows.
            let _ = vault.delete(&account(repo_key, name));
            return Err(err);
        }
    }
    Ok(SecretList {
        names: names_in(&store, repo_key),
    })
}

/// Remove a secret's value from the keychain and its name from the list.
/// Refused unless `confirmed` is true.
pub fn delete(
    config_dir: &Path,
    vault: &dyn Vault,
    repo_key: &str,
    name: &str,
    confirmed: bool,
) -> Result<SecretList, String> {
    if !confirmed {
        return Err(NEEDS_CONFIRM.into());
    }
    let name = name.trim();
    let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut store = load(config_dir);
    if !store
        .entries
        .iter()
        .any(|e| e.repo_key == repo_key && e.name == name)
    {
        return Err(format!("There's no secret called {name} in this project."));
    }
    vault.delete(&account(repo_key, name))?;
    store
        .entries
        .retain(|e| !(e.repo_key == repo_key && e.name == name));
    save(config_dir, &store)?;
    Ok(SecretList {
        names: names_in(&store, repo_key),
    })
}

/// A project's secrets read for one child process. The values stay in this
/// struct: it has no `Serialize` and its `Debug` prints names only.
pub struct Loaded {
    pairs: Vec<(String, String)>,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.pairs.iter().map(|(n, _)| n.as_str()).collect();
        f.debug_struct("Loaded").field("names", &names).finish()
    }
}

impl Loaded {
    pub fn names(&self) -> Vec<String> {
        self.pairs.iter().map(|(n, _)| n.clone()).collect()
    }

    /// Set each secret as `NAME=value` in the child's environment.
    pub fn inject(&self, cmd: &mut Command) {
        for (name, value) in &self.pairs {
            cmd.env(name, value);
        }
    }

    /// Hide every loaded value, then the usual token patterns.
    pub fn redact(&self, text: &str) -> String {
        let held: Vec<String> = self.pairs.iter().map(|(_, v)| v.clone()).collect();
        redact(text, &held)
    }

    /// The values, for redacting text built elsewhere (the activity log).
    pub fn held(&self) -> Vec<String> {
        self.pairs.iter().map(|(_, v)| v.clone()).collect()
    }
}

/// Redacts a child's output as it streams. With values to hide, output is
/// held back to the end of each line. With none, only an unfinished key or
/// character is held. Either way a value or a key that is split across two
/// reads is still hidden whole.
#[derive(Debug, Default)]
pub struct Shield {
    held: Vec<String>,
    pending: Vec<u8>,
}

/// A line this long with no newline is let out, up to any value or key that
/// is still unfinished at its end.
const SHIELD_LINE_LIMIT: usize = 32 * 1024;
/// An unfinished key is held whole up to this length. A longer run with no
/// break in it is no key, and is let out.
const KEY_MAX: usize = 4096;
/// What a key starts with. Keep in step with `grokhub_core::redact`.
const KEY_PREFIXES: [&str; 7] = ["sk-", "xai-", "ghp_", "github_pat_", "gho_", "ghu_", "ghs_"];
/// The words whose next word is hidden.
const BEARER_WORDS: [&str; 2] = ["Bearer ", "bearer "];

/// How much of `bytes` is whole characters: one that is unfinished at the
/// end is left out.
fn whole_chars(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        _ => bytes.len(),
    }
}

/// A character a key can be made of (the same set the redactor reads).
fn key_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'+' | b'/' | b'=')
}

/// Where a `needle` begins that starts before `cut` and runs past it, whole
/// or cut short by the end of `hay`.
fn straddling(hay: &[u8], cut: usize, needle: &[u8]) -> Option<usize> {
    let from = cut.saturating_sub(needle.len().saturating_sub(1));
    (from..cut).find(|&at| {
        let n = needle.len().min(hay.len() - at);
        hay[at..at + n] == needle[..n]
    })
}

/// Where a key begins that is still unfinished at `cut`: a key prefix with
/// nothing but key characters up to the cut, or the `Bearer` before them.
fn open_key(hay: &[u8], cut: usize) -> Option<usize> {
    let low = cut.saturating_sub(KEY_MAX);
    let run = hay[low..cut]
        .iter()
        .rposition(|b| !key_char(*b))
        .map_or(low, |i| low + i + 1);
    if let Some(word) = BEARER_WORDS
        .iter()
        .find(|w| hay[..run].ends_with(w.as_bytes()))
    {
        return Some(run - word.len());
    }
    (run..cut).find(|&at| {
        KEY_PREFIXES
            .iter()
            .any(|p| hay[at..cut].starts_with(p.as_bytes()))
    })
}

impl Shield {
    pub fn new(held: Vec<String>) -> Self {
        Self {
            held: hidden(&held),
            pending: Vec::new(),
        }
    }

    /// Move `cut` back until it splits no held value, no key prefix, no
    /// unfinished key and no `Bearer` from its value.
    fn safe_cut(&self, mut cut: usize) -> usize {
        loop {
            let before = cut;
            let needles = self
                .held
                .iter()
                .map(String::as_str)
                .chain(KEY_PREFIXES)
                .chain(BEARER_WORDS);
            for needle in needles {
                if let Some(at) = straddling(&self.pending, cut, needle.as_bytes()) {
                    cut = at;
                }
            }
            if let Some(at) = open_key(&self.pending, cut) {
                cut = at;
            }
            if cut == before {
                return cut;
            }
        }
    }

    fn take(&mut self, len: usize) -> String {
        let head: Vec<u8> = self.pending.drain(..len).collect();
        hide(&String::from_utf8_lossy(&head), &self.held)
    }

    /// Redacted text that is safe to show so far.
    pub fn push(&mut self, text: &str) -> String {
        self.push_bytes(text.as_bytes())
    }

    /// [`Shield::push`] for bytes as they were read: a character that is
    /// split across two reads is put back together.
    pub fn push_bytes(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let line = self
            .pending
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |i| i + 1);
        let long = line == 0 && self.pending.len() > SHIELD_LINE_LIMIT;
        let end = if self.held.is_empty() || long {
            whole_chars(&self.pending)
        } else {
            line
        };
        let cut = self.safe_cut(end);
        self.take(cut)
    }

    /// Whatever is still held back, redacted. Call once the output has ended.
    pub fn finish(&mut self) -> String {
        let all = self.pending.len();
        self.take(all)
    }
}

/// Read every secret this project lists. A listed name with no value in the
/// keychain is an error that names the secret, never a silent skip.
pub fn load_values(config_dir: &Path, vault: &dyn Vault, repo_key: &str) -> Result<Loaded, String> {
    let mut pairs = Vec::new();
    for name in names_in(&load(config_dir), repo_key) {
        match vault.get(&account(repo_key, &name))? {
            Some(value) if !value.trim().is_empty() => pairs.push((name, value)),
            _ => {
                return Err(format!(
                    "{name} has no value in the keychain. Set it again in Settings → Code."
                ))
            }
        }
    }
    Ok(Loaded { pairs })
}

/// Every value this project has in the keychain, for hiding them in output
/// (the terminal). Unlike [`load_values`], a name with no value is skipped:
/// nothing runs with these, they are only redacted.
pub fn held_values(config_dir: &Path, vault: &dyn Vault, repo_key: &str) -> Vec<String> {
    names_in(&load(config_dir), repo_key)
        .into_iter()
        .filter_map(|name| vault.get(&account(repo_key, &name)).ok().flatten())
        .filter(|v| !v.trim().is_empty())
        .collect()
}

/// What to look for in output: each value trimmed, and each line of a value
/// with line breaks as well (a command can print such a value a line at a
/// time). Longest first, so a value that contains another is hidden whole.
pub(crate) fn hidden(held: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for value in held.iter().map(|v| v.trim()).filter(|v| !v.is_empty()) {
        out.push(value.to_string());
        if value.contains('\n') {
            let lines = value
                .lines()
                .map(str::trim)
                .filter(|line| line.chars().count() >= MIN_VALUE_CHARS);
            out.extend(lines.map(str::to_string));
        }
    }
    out.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    out.dedup();
    out
}

/// Replace each of `hidden` with [`MARKER`], then redact the usual token patterns.
fn hide(text: &str, hidden: &[String]) -> String {
    redact_secrets(&redact_held_secrets(text, hidden))
}

/// Replace each held value with [`MARKER`] (longest first, so a value that
/// contains another is hidden whole), then redact the usual token patterns.
pub fn redact(text: &str, held: &[String]) -> String {
    hide(text, &hidden(held))
}

// ---- what the Code tab calls ----

fn key_of(dir: &Path) -> String {
    repo_key(dir, crate::repo::origin_url(dir).as_deref())
}

pub fn list_for(config_dir: &Path, workspace: &Path) -> SecretList {
    list(config_dir, &key_of(workspace))
}

/// Put a secret change in the activity log: the name only, never the value.
pub fn log_change(config_dir: &Path, repo_key: &str, summary: &str, keep: usize) {
    let _ = crate::activity::append(
        config_dir,
        &crate::activity::Entry {
            ts_ms: grokhub_core::now_ms(),
            kind: crate::activity::Kind::Secret,
            repo_key: repo_key.to_string(),
            host: String::new(),
            branch: String::new(),
            command: String::new(),
            mode: String::new(),
            approved_by: "user".into(),
            exit: None,
            summary: summary.to_string(),
        },
        keep,
    );
}

/// [`set`] against the keychain, then log whether the name was added or its
/// value replaced.
pub fn set_logged(
    config_dir: &Path,
    vault: &dyn Vault,
    repo_key: &str,
    name: &str,
    value: &str,
    keep: usize,
) -> Result<SecretList, String> {
    let name = name.trim();
    let existed = names_in(&load(config_dir), repo_key)
        .iter()
        .any(|n| n == name);
    let list = set(config_dir, vault, repo_key, name, value)?;
    let verb = if existed {
        "Replaced the value of"
    } else {
        "Added"
    };
    log_change(
        config_dir,
        repo_key,
        &format!("{verb} secret {name}."),
        keep,
    );
    Ok(list)
}

/// [`delete`] against the keychain, then log it.
pub fn delete_logged(
    config_dir: &Path,
    vault: &dyn Vault,
    repo_key: &str,
    name: &str,
    confirmed: bool,
    keep: usize,
) -> Result<SecretList, String> {
    let list = delete(config_dir, vault, repo_key, name, confirmed)?;
    log_change(
        config_dir,
        repo_key,
        &format!("Deleted secret {}.", name.trim()),
        keep,
    );
    Ok(list)
}

/// [`held_values`] for the workspace's project, from the keychain.
pub fn held_for(config_dir: &Path, workspace: &Path) -> Vec<String> {
    held_values(config_dir, &Keychain, &key_of(workspace))
}

pub fn set_for(
    config_dir: &Path,
    workspace: &Path,
    name: &str,
    value: &str,
    keep: usize,
) -> Result<SecretList, String> {
    set_logged(config_dir, &Keychain, &key_of(workspace), name, value, keep)
}

pub fn delete_for(
    config_dir: &Path,
    workspace: &Path,
    name: &str,
    confirmed: bool,
    keep: usize,
) -> Result<SecretList, String> {
    delete_logged(
        config_dir,
        &Keychain,
        &key_of(workspace),
        name,
        confirmed,
        keep,
    )
}

#[cfg(test)]
#[path = "secrets_tests.rs"]
mod tests;
