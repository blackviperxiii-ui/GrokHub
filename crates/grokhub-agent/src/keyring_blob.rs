//! Split a large keyring secret across numbered entries so Windows Credential
//! Manager's 2560 UTF-16-unit password limit does not reject Grok OAuth JSON.
//!
//! Layout under one service:
//! - `account` — legacy / small single-entry secret
//! - `account#n` — chunk count (decimal) when the value is split
//! - `account.0` … `account.{n-1}` — UTF-16-sized chunks
//!
//! Reads prefer a non-empty single entry, then the chunk layout. Writes that
//! fit the platform limit stay as one entry and clear any leftover chunks;
//! larger values replace the single entry with chunks.

/// Windows Credential Manager limit reported by `keyring` (UTF-16 code units).
pub const PLATFORM_UTF16_LIMIT: usize = 2560;

/// Max UTF-16 units per chunk (headroom under [`PLATFORM_UTF16_LIMIT`]).
pub const CHUNK_UTF16_MAX: usize = 2400;

/// One named keyring slot under a fixed service. The OS backend uses
/// `keyring`; tests use an in-memory map.
pub trait Backend {
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// System keychain entries for `service`, with caller-chosen error wording.
pub struct OsBackend {
    service: &'static str,
    map_err: fn(keyring::Error) -> String,
}

impl OsBackend {
    pub const fn new(service: &'static str, map_err: fn(keyring::Error) -> String) -> Self {
        Self { service, map_err }
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(self.service, account).map_err(self.map_err)
    }
}

impl Backend for OsBackend {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        match self.entry(account)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err((self.map_err)(err)),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.entry(account)?
            .set_password(value)
            .map_err(self.map_err)
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err((self.map_err)(err)),
        }
    }
}

fn count_account(base: &str) -> String {
    format!("{base}#n")
}

fn chunk_account(base: &str, index: usize) -> String {
    format!("{base}.{index}")
}

/// UTF-16 code units in `s` (same measure keyring uses on Windows).
pub fn utf16_units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Split `s` so each piece is at most `max` UTF-16 units. Empty input yields
/// one empty chunk.
pub fn split_utf16(s: &str, max: usize) -> Vec<String> {
    assert!(max > 0, "chunk max must be positive");
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut units = 0usize;
    for ch in s.chars() {
        let u = ch.len_utf16();
        if units + u > max && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            units = 0;
        }
        // A single scalar larger than `max` still goes in its own chunk; the
        // platform would refuse it either way.
        current.push(ch);
        units += u;
    }
    if chunks.is_empty() || !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Delete `base.from` … leftover higher indexes. Leaves the count entry alone.
fn clear_chunk_tail(backend: &dyn Backend, base: &str, from: usize) -> Result<(), String> {
    // Bound the scan so a corrupt count cannot loop forever.
    const SCAN_CAP: usize = 64;
    for i in from..SCAN_CAP {
        let name = chunk_account(base, i);
        match backend.get(&name)? {
            Some(_) => backend.delete(&name)?,
            None if i > from => break,
            None => {}
        }
    }
    Ok(())
}

fn clear_chunks(backend: &dyn Backend, base: &str) -> Result<(), String> {
    clear_chunk_tail(backend, base, 0)?;
    backend.delete(&count_account(base))?;
    Ok(())
}

/// Read the full secret for `account`. A legacy single entry wins over chunks.
pub fn get(backend: &dyn Backend, account: &str) -> Result<Option<String>, String> {
    if let Some(v) = backend.get(account)? {
        if !v.trim().is_empty() {
            return Ok(Some(v));
        }
    }
    let Some(raw) = backend.get(&count_account(account))? else {
        return Ok(None);
    };
    let n: usize = match raw.trim().parse() {
        Ok(0) | Err(_) => return Ok(None),
        Ok(n) => n,
    };
    if n > 64 {
        return Err(format!(
            "The system keychain entry for {account} has too many chunks ({n})."
        ));
    }
    let mut out = String::new();
    for i in 0..n {
        let chunk = backend.get(&chunk_account(account, i))?.ok_or_else(|| {
            format!("The system keychain entry for {account} is missing chunk {i}.")
        })?;
        out.push_str(&chunk);
    }
    if out.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

/// Store `value` under `account`, splitting when it exceeds the platform limit.
pub fn set(backend: &dyn Backend, account: &str, value: &str) -> Result<(), String> {
    if utf16_units(value) <= PLATFORM_UTF16_LIMIT {
        backend.set(account, value)?;
        clear_chunks(backend, account)?;
        return Ok(());
    }
    let chunks = split_utf16(value, CHUNK_UTF16_MAX);
    for (i, chunk) in chunks.iter().enumerate() {
        backend.set(&chunk_account(account, i), chunk)?;
    }
    backend.set(&count_account(account), &chunks.len().to_string())?;
    // Drop a stale single entry so readers do not prefer an old short blob.
    backend.delete(account)?;
    clear_chunk_tail(backend, account, chunks.len())?;
    Ok(())
}

/// Remove the single entry and every chunk for `account`.
pub fn delete(backend: &dyn Backend, account: &str) -> Result<(), String> {
    backend.delete(account)?;
    clear_chunks(backend, account)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Mem(Mutex<HashMap<String, String>>);

    impl Backend for Mem {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, value: &str) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(account.to_string(), value.to_string());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    #[test]
    fn a_small_secret_stays_as_one_entry() {
        let store = Mem::default();
        set(&store, "shell-xai-oauth", "short-token").unwrap();
        assert_eq!(
            store.get("shell-xai-oauth").unwrap().as_deref(),
            Some("short-token")
        );
        assert!(store.get("shell-xai-oauth#n").unwrap().is_none());
        assert_eq!(
            get(&store, "shell-xai-oauth").unwrap().as_deref(),
            Some("short-token")
        );
    }

    #[test]
    fn a_secret_over_the_windows_limit_round_trips_in_chunks() {
        let store = Mem::default();
        // ASCII: one char == one UTF-16 unit. Past 2560 so a single entry would fail.
        let secret: String = (0..PLATFORM_UTF16_LIMIT + 80)
            .map(|i| char::from(b'A' + (i % 26) as u8))
            .collect();
        assert!(utf16_units(&secret) > PLATFORM_UTF16_LIMIT);
        set(&store, "shell-xai-oauth", &secret).unwrap();
        assert!(store.get("shell-xai-oauth").unwrap().is_none());
        let n: usize = store
            .get("shell-xai-oauth#n")
            .unwrap()
            .expect("count")
            .parse()
            .unwrap();
        assert!(n >= 2, "expected split, got {n} chunks");
        for i in 0..n {
            let chunk = store
                .get(&format!("shell-xai-oauth.{i}"))
                .unwrap()
                .expect("chunk");
            assert!(utf16_units(&chunk) <= CHUNK_UTF16_MAX);
        }
        assert_eq!(
            get(&store, "shell-xai-oauth").unwrap().as_deref(),
            Some(secret.as_str())
        );
    }

    #[test]
    fn a_legacy_single_entry_is_still_readable() {
        let store = Mem::default();
        store
            .set("shell-xai-oauth", r#"{"access_token":"at-legacy"}"#)
            .unwrap();
        assert_eq!(
            get(&store, "shell-xai-oauth").unwrap().as_deref(),
            Some(r#"{"access_token":"at-legacy"}"#)
        );
    }

    #[test]
    fn delete_clears_both_layouts() {
        let store = Mem::default();
        let secret: String = "x".repeat(PLATFORM_UTF16_LIMIT + 10);
        set(&store, "xai-oauth", &secret).unwrap();
        delete(&store, "xai-oauth").unwrap();
        assert!(get(&store, "xai-oauth").unwrap().is_none());
        assert!(store.0.lock().unwrap().is_empty());

        store.set("xai-oauth", "alone").unwrap();
        delete(&store, "xai-oauth").unwrap();
        assert!(get(&store, "xai-oauth").unwrap().is_none());
    }

    #[test]
    fn shrinking_below_the_limit_collapses_back_to_one_entry() {
        let store = Mem::default();
        let big: String = "b".repeat(PLATFORM_UTF16_LIMIT + 40);
        set(&store, "shell-xai-oauth", &big).unwrap();
        set(&store, "shell-xai-oauth", "now-small").unwrap();
        assert_eq!(
            store.get("shell-xai-oauth").unwrap().as_deref(),
            Some("now-small")
        );
        assert!(store.get("shell-xai-oauth#n").unwrap().is_none());
        assert!(store.get("shell-xai-oauth.0").unwrap().is_none());
        assert_eq!(
            get(&store, "shell-xai-oauth").unwrap().as_deref(),
            Some("now-small")
        );
    }

    #[test]
    fn split_utf16_keeps_each_chunk_under_the_cap() {
        let s: String = "é".repeat(100); // BMP, one UTF-16 unit each
        let parts = split_utf16(&s, 30);
        assert!(parts.len() >= 4);
        for p in &parts {
            assert!(utf16_units(p) <= 30);
        }
        assert_eq!(parts.concat(), s);
    }
}
