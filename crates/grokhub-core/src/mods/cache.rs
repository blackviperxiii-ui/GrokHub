//! What keeps browsing from hammering a source: an on-disk cache under
//! `{config}/mods/cache/` (lists live 6 h, details and files 24 h) and a
//! token bucket per source. Both take the time as an argument, so tests run
//! on a fake clock.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::allowlist::Source;

/// How long a search or browse page stays fresh.
pub const LIST_TTL_SECS: u64 = 6 * 60 * 60;
/// How long a mod's detail or file list stays fresh.
pub const DETAIL_TTL_SECS: u64 = 24 * 60 * 60;

/// The mods folder inside the config folder.
pub fn mods_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("mods")
}

#[derive(Serialize, Deserialize)]
struct Stored<T> {
    at: u64,
    value: T,
}

/// A JSON file per key, named by the key's SHA-256 so no key text (a search)
/// becomes a file name.
#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            dir: mods_dir(config_dir).join("cache"),
        }
    }

    fn path(&self, key: &str) -> PathBuf {
        let hash = hex::encode(Sha256::digest(key.as_bytes()));
        self.dir.join(format!("{}.json", &hash[..32]))
    }

    /// The value and when it was stored, however old.
    pub fn get_any<T: DeserializeOwned>(&self, key: &str) -> Option<(T, u64)> {
        let bytes = std::fs::read(self.path(key)).ok()?;
        let s: Stored<T> = serde_json::from_slice(&bytes).ok()?;
        Some((s.value, s.at))
    }

    /// The value while it is younger than `ttl` seconds at `now`.
    pub fn get<T: DeserializeOwned>(&self, key: &str, ttl: u64, now: u64) -> Option<T> {
        self.get_any(key)
            .filter(|(_, at)| *at <= now && now - *at < ttl)
            .map(|(v, _)| v)
    }

    pub fn put<T: Serialize>(&self, key: &str, value: &T, now: u64) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec(&Stored { at: now, value }).map_err(|e| e.to_string())?;
        let path = self.path(key);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| e.to_string())
    }
}

/// A token bucket: `capacity` requests at once, refilling `per_min` a minute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bucket {
    capacity: f64,
    per_ms: f64,
    tokens: f64,
    at_ms: u64,
}

impl Bucket {
    pub fn new(capacity: u32, per_min: u32, now_ms: u64) -> Self {
        Self {
            capacity: f64::from(capacity),
            per_ms: f64::from(per_min) / 60_000.0,
            tokens: f64::from(capacity),
            at_ms: now_ms,
        }
    }

    /// Take one token, or say how many seconds until one is there.
    pub fn take(&mut self, now_ms: u64) -> Result<(), u64> {
        let elapsed = now_ms.saturating_sub(self.at_ms) as f64;
        self.tokens = (self.tokens + elapsed * self.per_ms).min(self.capacity);
        self.at_ms = self.at_ms.max(now_ms);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            let wait_ms = (1.0 - self.tokens) / self.per_ms;
            Err((wait_ms / 1000.0).ceil().max(1.0) as u64)
        }
    }

    /// Hold every request until `until_ms` (a source said its limit is used up).
    pub fn pause_until(&mut self, until_ms: u64) {
        self.tokens = 0.0;
        self.at_ms = self.at_ms.max(until_ms);
    }
}

/// Burst and per-minute rate for each source, well under their published limits.
pub fn limits(source: Source) -> (u32, u32) {
    match source {
        // Nexus: 20,000 a day / 500 an hour after that, per user.
        Source::Nexus => (6, 20),
        Source::Workshop => (6, 30),
        Source::CurseForge => (6, 30),
        // The list is one big request; details are small.
        Source::Thunderstore => (4, 12),
        // Modrinth: 300 a minute per IP.
        Source::Modrinth => (8, 60),
    }
}

/// One bucket per source.
#[derive(Debug, Default)]
pub struct Buckets(HashMap<Source, Bucket>);

impl Buckets {
    pub fn take(&mut self, source: Source, now_ms: u64) -> Result<(), u64> {
        let (cap, per_min) = limits(source);
        self.0
            .entry(source)
            .or_insert_with(|| Bucket::new(cap, per_min, now_ms))
            .take(now_ms)
    }

    pub fn pause(&mut self, source: Source, until_ms: u64, now_ms: u64) {
        let (cap, per_min) = limits(source);
        self.0
            .entry(source)
            .or_insert_with(|| Bucket::new(cap, per_min, now_ms))
            .pause_until(until_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mods-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn cached_values_expire_after_their_ttl() {
        let root = dir("ttl");
        let c = Cache::new(&root);
        assert_eq!(c.get::<Vec<u32>>("k", 10, 100), None);
        c.put("k", &vec![1, 2], 100).unwrap();
        assert_eq!(c.get::<Vec<u32>>("k", 10, 105), Some(vec![1, 2]));
        assert_eq!(c.get::<Vec<u32>>("k", 10, 110), None);
        assert_eq!(c.get_any::<Vec<u32>>("k"), Some((vec![1, 2], 100)));
        assert_eq!(
            c.get::<Vec<u32>>("k", 10, 50),
            None,
            "a stamp from the future is stale"
        );
        let file = std::fs::read_dir(root.join("mods").join("cache"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name();
        assert!(!file.to_string_lossy().contains('k'));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_bucket_refills_on_the_clock() {
        let mut b = Bucket::new(2, 60, 0);
        assert_eq!(b.take(0), Ok(()));
        assert_eq!(b.take(0), Ok(()));
        assert_eq!(b.take(0), Err(1));
        assert_eq!(b.take(500), Err(1));
        assert_eq!(b.take(1000), Ok(()));
        assert_eq!(b.take(1000), Err(1));
        assert_eq!(b.take(10_000), Ok(()));
        assert_eq!(b.take(10_000), Ok(()), "refills up to capacity");
        assert_eq!(b.take(10_000), Err(1));
        b.pause_until(70_000);
        assert_eq!(b.take(20_000), Err(1));
        assert_eq!(b.take(71_000), Ok(()));
    }

    #[test]
    fn each_source_has_its_own_bucket() {
        let mut b = Buckets::default();
        for _ in 0..4 {
            assert!(b.take(Source::Thunderstore, 0).is_ok());
        }
        assert!(b.take(Source::Thunderstore, 0).is_err());
        assert!(b.take(Source::Modrinth, 0).is_ok());
        b.pause(Source::Modrinth, 60_000, 0);
        assert!(b.take(Source::Modrinth, 30_000).is_err());
    }
}
