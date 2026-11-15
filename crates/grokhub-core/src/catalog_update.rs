//! The signed model catalog. Each release ships `local_catalog.toml` beside
//! its assets, listed in the release's `SHA256SUMS`, which is signed with the
//! release key. An older app saves a newer catalog from the newest release
//! of its channel, so new models reach it without an app update.
//!
//! Nothing unsigned is ever used: the three files are checked when saved and
//! again every time the app starts, and a catalog that doesn't parse whole,
//! or isn't newer than the built-in one, is ignored.

use std::path::{Path, PathBuf};

use crate::release_verify::{self as rv, Release, ReleaseAsset};

/// The catalog's name in a release and on disk.
pub const FILE: &str = "local_catalog.toml";

/// A catalog this big is not a catalog.
pub const MAX_BYTES: u64 = 1 << 20;

/// How often the app looks for a newer catalog.
pub const CHECK_EVERY_MS: u64 = 24 * 60 * 60 * 1000;
/// The update channel (its receipt word) the saved catalog came from.
const CHANNEL_FILE: &str = "channel";

const STAMP: &str = "checked_ms";

/// `{config}/catalog`: the saved catalog with its sums and signature.
pub fn dir(config_dir: &Path) -> PathBuf {
    config_dir.join("catalog")
}

/// Check a catalog against signed sums with `pubkey`, then parse it. Its
/// version, or why it can't be used.
pub fn check(pubkey: &str, text: &str, sums: &str, sig: &str) -> Result<u32, String> {
    if text.len() as u64 > MAX_BYTES {
        return Err("the catalog is too big".into());
    }
    rv::verify_asset(Some(pubkey), sums, Some(sig), FILE, text.as_bytes())
        .map_err(|r| r.reason())?;
    catalog_version(text)
        .ok_or_else(|| "the catalog doesn't load in this version of the app".into())
}

/// The version of a catalog that loads whole in this app. Parsing keeps its
/// strings for the run, so a text already parsed isn't parsed again: the
/// daily look reads the same saved and downloaded text day after day.
fn catalog_version(text: &str) -> Option<u32> {
    use std::hash::{Hash, Hasher};
    static SEEN: std::sync::Mutex<Vec<(u64, Option<u32>)>> = std::sync::Mutex::new(Vec::new());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    let key = h.finish();
    if let Some(&(_, v)) = SEEN.lock().ok()?.iter().find(|(k, _)| *k == key) {
        return v;
    }
    let v = crate::local_setup::parse_catalog(text).map(|(v, _)| v);
    if let Ok(mut seen) = SEEN.lock() {
        if seen.len() >= 8 {
            seen.remove(0);
        }
        seen.push((key, v));
    }
    v
}

fn read(dir: &Path) -> Option<(String, String, String)> {
    let r = |n: &str| std::fs::read_to_string(dir.join(n)).ok();
    Some((r(FILE)?, r(rv::SUMS_NAME)?, r(rv::SIG_NAME)?))
}

/// The saved catalog's text, when it still verifies with `pubkey` and is
/// newer than `bundled`.
pub fn saved_with(config_dir: &Path, pubkey: &str, bundled: u32) -> Option<String> {
    let (text, sums, sig) = read(&dir(config_dir))?;
    let v = check(pubkey, &text, &sums, &sig).ok()?;
    (v > bundled).then_some(text)
}

/// [`saved_with`] for the release key and the built-in catalog. Never
/// `None`-to-`Some` without a key: an unsigned build uses only its own.
pub fn saved(config_dir: &Path) -> Option<String> {
    let pubkey = rv::RELEASE_PUBKEY?;
    // The built-in version, read without the saved one (no recursion).
    saved_with(config_dir, pubkey, crate::local_setup::bundled_version())
}

/// The three release assets a catalog update needs, when the release has
/// them at this repo's own download addresses.
pub fn assets(rel: &Release) -> Option<[&ReleaseAsset; 3]> {
    let found = [
        rel.asset(FILE)?,
        rel.asset(rv::SUMS_NAME)?,
        rel.asset(rv::SIG_NAME)?,
    ];
    found
        .iter()
        .all(|a| rv::download_url_ok(&a.url, &a.name))
        .then_some(found)
}

/// Save a downloaded catalog when it verifies and is newer than what this
/// app uses now (`current`). `Ok(Some(v))` once saved; it is used from the
/// next start. Files are written whole beside their final names first.
pub fn adopt(
    config_dir: &Path,
    pubkey: &str,
    current: u32,
    channel: &str,
    text: &str,
    sums: &str,
    sig: &str,
) -> Result<Option<u32>, String> {
    let v = check(pubkey, text, sums, sig)?;
    if v <= current {
        return Ok(None);
    }
    let dir = dir(config_dir);
    save_all(
        &dir,
        [
            (FILE, text),
            (rv::SUMS_NAME, sums),
            (rv::SIG_NAME, sig),
            (CHANNEL_FILE, channel),
        ],
    )
    .inspect_err(|_| {
        // A disk or file-lock failure, not a bad catalog: look again next time.
        let _ = std::fs::remove_file(dir.join(STAMP));
    })?;
    Ok(Some(v))
}

/// Write every file beside its final name, then rename them all, so a full
/// disk stops the save before any saved file is replaced.
fn save_all(dir: &Path, files: [(&str, &str); 4]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("catalog folder: {e}"))?;
    for (name, body) in files {
        std::fs::write(dir.join(format!("{name}.part")), body)
            .map_err(|e| format!("{name}: {e}"))?;
    }
    for (name, _) in files {
        std::fs::rename(dir.join(format!("{name}.part")), dir.join(name))
            .map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}

/// A capped GET, `(url, byte cap) -> body`, that refuses a body over the cap.
pub type Get<'a> = &'a mut dyn FnMut(&str, u64) -> Result<Vec<u8>, String>;

/// `SHA256SUMS` and its signature are a few KB.
pub const TEXT_CAP: u64 = 64 * 1024;

/// Fetch `rel`'s catalog with its signed sums and save it when it verifies
/// and is newer than what this app uses now. `Ok(None)` when the release has
/// no catalog at its own download addresses (nothing is fetched then), or
/// its catalog isn't newer.
pub fn refresh(
    pubkey: &str,
    rel: &Release,
    config_dir: &Path,
    channel: &str,
    get: Get,
) -> Result<Option<u32>, String> {
    // Releases made before the catalog shipped have none: nothing to do.
    let Some([cat, sums, sig]) = assets(rel) else {
        return Ok(None);
    };
    let mut text = |asset: &ReleaseAsset, cap: u64| -> Result<String, String> {
        String::from_utf8(get(&asset.url, cap)?).map_err(|_| format!("{} isn't text", asset.name))
    };
    let sums = text(sums, TEXT_CAP)?;
    let sig = text(sig, TEXT_CAP)?;
    let cat = text(cat, MAX_BYTES)?;
    adopt(
        config_dir,
        pubkey,
        current_version(config_dir),
        channel,
        &cat,
        &sums,
        &sig,
    )
}

/// The catalog version this app runs with now: the saved one when it
/// applies, else the built-in one.
pub fn current_version(config_dir: &Path) -> u32 {
    let bundled = crate::local_setup::bundled_version();
    saved(config_dir)
        .and_then(|t| crate::local_setup::parse_catalog(&t))
        .map_or(bundled, |(v, _)| v.max(bundled))
}

/// Whether a day has passed since the last look. A missing stamp is due, and
/// so is one later than now (the clock was set back).
pub fn due(config_dir: &Path, now_ms: u64) -> bool {
    std::fs::read_to_string(dir(config_dir).join(STAMP))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .is_none_or(|last| last > now_ms || now_ms - last >= CHECK_EVERY_MS)
}

/// Drop the saved catalog and its stamp, so the built-in one is used from
/// the next start and the next look runs at once. For a change of update
/// channel: a catalog from the old channel's releases mustn't stay.
pub fn forget(config_dir: &Path) {
    let _ = std::fs::remove_dir_all(dir(config_dir));
}

/// The update channel the saved catalog came from.
pub fn saved_channel(config_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir(config_dir).join(CHANNEL_FILE)).ok()?;
    Some(text.trim().to_string())
}

/// Before the catalog is read: [`forget`] a saved one that came from
/// another channel (however the channel changed), or that no longer applies
/// (half saved, changed on disk, or not newer than the built-in one), so the
/// next look runs at once. `true` when it forgot one.
pub fn tidy(config_dir: &Path, channel: &str) -> bool {
    let bundled = crate::local_setup::bundled_version();
    let Some(pubkey) = rv::RELEASE_PUBKEY else {
        // A build without the release key never uses a saved catalog.
        let had = dir(config_dir).join(FILE).exists();
        forget(config_dir);
        return had;
    };
    tidy_with(config_dir, channel, pubkey, bundled)
}

/// [`tidy`] with the key and the built-in version passed in.
pub fn tidy_with(config_dir: &Path, channel: &str, pubkey: &str, bundled: u32) -> bool {
    if !dir(config_dir).join(FILE).exists() {
        return false;
    }
    let stale = saved_channel(config_dir).as_deref() != Some(channel)
        || saved_with(config_dir, pubkey, bundled).is_none();
    if stale {
        forget(config_dir);
    }
    stale
}

/// Note a look, found or not, so a failure also waits a day.
pub fn stamp(config_dir: &Path, now_ms: u64) {
    let dir = dir(config_dir);
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join(STAMP), now_ms.to_string());
    }
}

#[cfg(test)]
#[path = "catalog_update_tests.rs"]
mod tests;
