//! Runtime integrity checking, archival extraction, and disk state for synced
//! upstream vstack skills (`blackviperxiii-ui/vstack`).
//!
//! Pure storage and archive logic lives here so it compiles and tests cleanly
//! without network access; `grokhub-agent::vstack_sync` handles the HTTPS fetches
//! behind Offline only and EgressGuard.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::{Archive, EntryType};

pub const VSTACK_REPO_OWNER: &str = "blackviperxiii-ui";
pub const VSTACK_REPO_NAME: &str = "vstack";
pub const VSTACK_REPO_URL: &str = "https://github.com/blackviperxiii-ui/vstack";
pub const VSTACK_BRANCH: &str = "main";
/// Periodic background check interval (6 hours).
pub const VSTACK_SYNC_INTERVAL_MS: u64 = 6 * 60 * 60 * 1000;
/// Cap downloaded upstream tarball at 32 MiB.
pub const VSTACK_ARCHIVE_CAP: usize = 32 * 1024 * 1024;
/// Cap any single extracted file at 4 MiB.
pub const VSTACK_ENTRY_CAP: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VstackState {
    pub repo: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub commit: String,
    pub bundle_sha256: String,
    pub skill_count: usize,
    pub last_sync_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VstackCheckMeta {
    pub last_check_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VstackStatus {
    pub repo: String,
    /// `"synced"` when a validated on-disk sync is active; `"bundled"` on fallback.
    pub source: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub skill_count: usize,
}

pub fn root_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("vstack")
}

pub fn skills_dir(config_dir: &Path) -> PathBuf {
    root_dir(config_dir).join("skills")
}

pub fn state_path(config_dir: &Path) -> PathBuf {
    root_dir(config_dir).join("state.json")
}

pub fn check_path(config_dir: &Path) -> PathBuf {
    root_dir(config_dir).join("check.json")
}

pub fn is_sha40(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_skill_dir_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_rel_segment(seg: &str) -> bool {
    !seg.is_empty()
        && seg != "."
        && seg != ".."
        && !seg.starts_with('.')
        && !seg.contains('\\')
        && !seg.contains(':')
        && !seg.bytes().any(|b| b == 0 || b < 0x20)
}

pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim().strip_prefix('v').unwrap_or(raw.trim());
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub fn read_state(config_dir: &Path) -> Option<VstackState> {
    let raw = fs::read_to_string(state_path(config_dir)).ok()?;
    let parsed: VstackState = serde_json::from_str(&raw).ok()?;
    if parsed.repo != VSTACK_REPO_URL || !is_sha40(&parsed.commit) || parsed.skill_count == 0 {
        return None;
    }
    Some(parsed)
}

pub fn read_check_meta(config_dir: &Path) -> Option<VstackCheckMeta> {
    let raw = fs::read_to_string(check_path(config_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Write check timestamp and optional error without altering `state.json` or
/// `skills/`.
pub fn record_check_result(
    config_dir: &Path,
    now_ms: u64,
    last_error: Option<String>,
) -> Result<(), String> {
    let root = root_dir(config_dir);
    fs::create_dir_all(&root).map_err(|e| format!("create vstack dir: {e}"))?;
    let meta = VstackCheckMeta {
        last_check_ms: now_ms,
        last_error,
    };
    let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    atomic_write(&check_path(config_dir), json.as_bytes())
}

/// True when no upstream check happened within [`VSTACK_SYNC_INTERVAL_MS`].
pub fn sync_due(config_dir: &Path, now_ms: u64) -> bool {
    let last = read_check_meta(config_dir)
        .map(|m| m.last_check_ms)
        .or_else(|| read_state(config_dir).map(|s| s.last_sync_ms));
    match last {
        Some(ts) => now_ms.saturating_sub(ts) >= VSTACK_SYNC_INTERVAL_MS,
        None => true,
    }
}

/// Returns the active on-disk synced `skills` directory if it is present, valid,
/// and not older (by semver) than the bundled fallback release tag.
pub fn active_synced_skills_dir(config_dir: &Path, bundled_tag: &str) -> Option<PathBuf> {
    let state = read_state(config_dir)?;
    if let (Some(bundled_v), Some(synced_v)) =
        (parse_semver(bundled_tag), parse_semver(&state.version))
    {
        if bundled_v > synced_v {
            return None;
        }
    }
    let dir = skills_dir(config_dir);
    if !dir.is_dir() {
        return None;
    }
    let rd = fs::read_dir(&dir).ok()?;
    let mut count = 0usize;
    for ent in rd.flatten() {
        let path = ent.path();
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if !valid_skill_dir_name(&name) {
            continue;
        }
        if path.join("SKILL.md").is_file() {
            count += 1;
        }
    }
    if count == 0 || count != state.skill_count {
        return None;
    }
    Some(dir)
}

/// Compute status for UI display, using active synced state when valid or the
/// bundled fallback snapshot otherwise.
pub fn status(
    config_dir: &Path,
    bundled_tag: &str,
    bundled_commit: &str,
    bundled_skill_count: usize,
) -> VstackStatus {
    let check = read_check_meta(config_dir);
    if active_synced_skills_dir(config_dir, bundled_tag).is_some() {
        if let Some(state) = read_state(config_dir) {
            return VstackStatus {
                repo: state.repo,
                source: "synced".to_string(),
                version: state.version,
                tag: state.tag,
                commit: state.commit,
                last_sync_ms: Some(state.last_sync_ms),
                last_check_ms: check
                    .as_ref()
                    .map(|c| c.last_check_ms)
                    .or(Some(state.last_sync_ms)),
                last_error: check.and_then(|c| c.last_error),
                skill_count: state.skill_count,
            };
        }
    }
    VstackStatus {
        repo: VSTACK_REPO_URL.to_string(),
        source: "bundled".to_string(),
        version: bundled_tag.to_string(),
        tag: Some(bundled_tag.to_string()),
        commit: bundled_commit.to_string(),
        last_sync_ms: None,
        last_check_ms: check.as_ref().map(|c| c.last_check_ms),
        last_error: check.and_then(|c| c.last_error),
        skill_count: bundled_skill_count,
    }
}

/// Parse the `sha` field from GitHub's `/commits/main` API response.
pub fn parse_github_commit_sha(body: &[u8]) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("invalid commit JSON: {e}"))?;
    let sha = v
        .get("sha")
        .and_then(|s| s.as_str())
        .map(str::trim)
        .ok_or_else(|| "commit response missing `sha`".to_string())?;
    if !is_sha40(sha) {
        return Err("commit `sha` is not a valid 40-character hash".to_string());
    }
    Ok(sha.to_string())
}

/// Optional best-effort tag resolution from `/tags` response when a tag points
/// to `commit`.
pub fn match_github_tag_for_commit(body: &[u8], commit: &str) -> Option<String> {
    let tags: Vec<serde_json::Value> = serde_json::from_slice(body).ok()?;
    for item in tags {
        let sha = item
            .get("commit")
            .and_then(|c| c.get("sha"))
            .and_then(|s| s.as_str())?;
        if sha == commit {
            if let Some(name) = item.get("name").and_then(|s| s.as_str()) {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[derive(Debug)]
struct ParsedArchive {
    version: String,
    license: Option<Vec<u8>>,
    skills: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    bundle_sha256: String,
}

fn has_valid_frontmatter(raw: &str) -> bool {
    let trimmed = raw.trim_start_matches('\u{feff}').trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return false;
    };
    let Some(rest) = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))
    else {
        return false;
    };
    rest.contains("\n---")
}

fn unpack_and_verify(tar_gz: &[u8], expected_commit: &str) -> Result<ParsedArchive, String> {
    if !is_sha40(expected_commit) {
        return Err("invalid upstream commit SHA".to_string());
    }
    if tar_gz.is_empty() || tar_gz.len() > VSTACK_ARCHIVE_CAP {
        return Err("upstream archive size out of bounds".to_string());
    }
    let decoder = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|e| format!("open upstream archive: {e}"))?;

    let mut root_prefix: Option<String> = None;
    let mut version_text: Option<String> = None;
    let mut license_bytes: Option<Vec<u8>> = None;
    let mut plugin_name: Option<String> = None;
    let mut skills: BTreeMap<String, BTreeMap<String, Vec<u8>>> = BTreeMap::new();

    for entry in entries {
        let mut entry = entry.map_err(|e| format!("read archive entry: {e}"))?;
        let kind = entry.header().entry_type();
        if kind == EntryType::Directory
            || kind.is_pax_global_extensions()
            || kind.is_pax_local_extensions()
            || kind.is_gnu_longname()
        {
            continue;
        }
        if kind != EntryType::Regular {
            return Err(format!(
                "unsafe non-regular entry in upstream archive ({kind:?})"
            ));
        }
        let raw_path = entry
            .path()
            .map_err(|e| format!("invalid archive path: {e}"))?
            .to_path_buf();
        let path_str = raw_path
            .to_str()
            .ok_or_else(|| "non-UTF-8 path in upstream archive".to_string())?
            .replace('\\', "/");
        if path_str.starts_with('/') {
            return Err("absolute path in upstream archive".to_string());
        }
        let mut parts = path_str.split('/');
        let first = parts.next().unwrap_or("");
        if first.is_empty() || first == "." || first == ".." {
            return Err("invalid archive root folder".to_string());
        }
        if let Some(ref expected) = root_prefix {
            if first != expected {
                return Err("archive contains multiple top-level directories".to_string());
            }
        } else {
            root_prefix = Some(first.to_string());
        }
        let rest: Vec<&str> = parts.collect();
        if rest.is_empty() {
            continue;
        }
        if rest.iter().any(|s| *s == ".." || *s == "." || s.is_empty()) {
            return Err(format!("unsafe path segment in archive: {path_str}"));
        }
        let size = entry
            .header()
            .size()
            .map_err(|e| format!("entry size: {e}"))?;
        if size as usize > VSTACK_ENTRY_CAP {
            return Err(format!("archive entry too large: {path_str}"));
        }
        let mut buf = Vec::with_capacity(size as usize);
        entry
            .by_ref()
            .take((VSTACK_ENTRY_CAP + 1) as u64)
            .read_to_end(&mut buf)
            .map_err(|e| format!("read {path_str}: {e}"))?;
        if buf.len() > VSTACK_ENTRY_CAP {
            return Err(format!("archive entry exceeded cap: {path_str}"));
        }

        match rest.as_slice() {
            ["VERSION"] => {
                if let Ok(s) = String::from_utf8(buf) {
                    let trimmed = s.trim().to_string();
                    if !trimmed.is_empty() {
                        version_text = Some(trimmed);
                    }
                }
            }
            ["LICENSE"] => {
                license_bytes = Some(buf);
            }
            ["plugin.json"]
            | [".claude-plugin", "plugin.json"]
            | [".cursor-plugin", "plugin.json"] => {
                if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&buf) {
                    if let Some(name) = val.get("name").and_then(|n| n.as_str()) {
                        plugin_name = Some(name.to_string());
                    }
                }
            }
            ["skills", skill, tail @ ..] if !tail.is_empty() => {
                if !valid_skill_dir_name(skill) {
                    return Err(format!("invalid upstream skill folder name: {skill}"));
                }
                if !tail.iter().all(|seg| valid_rel_segment(seg)) {
                    return Err(format!("invalid path inside skill {skill}"));
                }
                let rel = tail.join("/");
                if rel == "SKILL.md" {
                    let text = std::str::from_utf8(&buf)
                        .map_err(|_| format!("{skill}/SKILL.md is not valid UTF-8"))?;
                    if buf.contains(&0) || !has_valid_frontmatter(text) {
                        return Err(format!("{skill}/SKILL.md missing valid frontmatter"));
                    }
                }
                skills
                    .entry((*skill).to_string())
                    .or_default()
                    .insert(rel, buf);
            }
            _ => {}
        }
    }

    if let Some(name) = plugin_name {
        if name != VSTACK_REPO_NAME {
            return Err(format!("unexpected plugin manifest name `{name}`"));
        }
    }
    if skills.is_empty() {
        return Err("upstream archive contained no valid skills".to_string());
    }
    for (skill, files) in &skills {
        if !files.contains_key("SKILL.md") {
            return Err(format!("upstream skill `{skill}` has no SKILL.md"));
        }
    }

    let mut hasher = Sha256::new();
    for (skill, files) in &skills {
        for (rel, data) in files {
            hasher.update(format!("{skill}/{rel}").as_bytes());
            hasher.update(b"\0");
            hasher.update(data);
            hasher.update(b"\0");
        }
    }
    let bundle_sha256 = hex::encode(hasher.finalize());
    let version = version_text.unwrap_or_else(|| expected_commit[..12].to_string());

    Ok(ParsedArchive {
        version,
        license: license_bytes,
        skills,
        bundle_sha256,
    })
}

/// Integrity-check an upstream tarball and atomically install its `skills/`
/// snapshot under `{config_dir}/vstack/`.
///
/// Any failure before the final swap leaves existing synced skills untouched.
pub fn install_archive(
    config_dir: &Path,
    tar_gz: &[u8],
    commit: &str,
    tag: Option<String>,
    now_ms: u64,
) -> Result<VstackState, String> {
    let parsed = unpack_and_verify(tar_gz, commit)?;
    let root = root_dir(config_dir);
    fs::create_dir_all(&root).map_err(|e| format!("create vstack root: {e}"))?;

    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let staging = root.join(format!(".staging-{}-{seq}", std::process::id()));
    let staged_skills = staging.join("skills");
    if let Err(err) = write_staged_tree(&staged_skills, &parsed.skills) {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }

    let live = skills_dir(config_dir);
    let backup = root.join(format!(".backup-{}-{seq}", std::process::id()));
    let had_live = live.exists();
    if had_live {
        if let Err(err) = fs::rename(&live, &backup) {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("prepare skill swap: {err}"));
        }
    }
    if let Err(err) = fs::rename(&staged_skills, &live) {
        if had_live {
            let _ = fs::rename(&backup, &live);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("install synced skills: {err}"));
    }

    let state = VstackState {
        repo: VSTACK_REPO_URL.to_string(),
        version: parsed.version,
        tag,
        commit: commit.to_string(),
        bundle_sha256: parsed.bundle_sha256,
        skill_count: parsed.skills.len(),
        last_sync_ms: now_ms,
    };
    let json = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    if let Err(err) = atomic_write(&state_path(config_dir), json.as_bytes()) {
        // Restore previous skills directory if metadata couldn't be persisted.
        let _ = fs::remove_dir_all(&live);
        if had_live {
            let _ = fs::rename(&backup, &live);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }
    if let Some(license) = parsed.license {
        let _ = atomic_write(&root.join("LICENSE-upstream"), &license);
    }
    let _ = fs::remove_dir_all(&backup);
    let _ = fs::remove_dir_all(&staging);
    let _ = record_check_result(config_dir, now_ms, None);
    Ok(state)
}

fn write_staged_tree(
    root: &Path,
    skills: &BTreeMap<String, BTreeMap<String, Vec<u8>>>,
) -> Result<(), String> {
    for (skill, files) in skills {
        for (rel, data) in files {
            let mut dest = root.join(skill);
            for seg in rel.split('/') {
                dest.push(seg);
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("stage {skill}/{rel}: {e}"))?;
            }
            fs::write(&dest, data).map_err(|e| format!("write {skill}/{rel}: {e}"))?;
        }
    }
    Ok(())
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp-{}-{n}", std::process::id()));
    fs::write(&tmp, data).map_err(|e| format!("write {}: {e}", path.display()))?;
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("rename {}: {err}", path.display()));
    }
    Ok(())
}

/// Remove invalid `skills/` directory entries safely is handled by swap; helper
/// lists active skill folder names sorted.
pub fn synced_skill_names(config_dir: &Path, bundled_tag: &str) -> BTreeSet<String> {
    let Some(dir) = active_synced_skills_dir(config_dir, bundled_tag) else {
        return BTreeSet::new();
    };
    let Ok(rd) = fs::read_dir(dir) else {
        return BTreeSet::new();
    };
    rd.flatten()
        .filter_map(|ent| {
            let path = ent.path();
            let name = ent.file_name().to_string_lossy().into_owned();
            (valid_skill_dir_name(&name) && path.join("SKILL.md").is_file()).then_some(name)
        })
        .collect()
}

#[doc(hidden)]
pub fn tar_gz_for_test(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    {
        let mut ar = tar::Builder::new(&mut gz);
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            ar.append_data(&mut header, *path, *data)
                .map_err(|e| e.to_string())?;
        }
        ar.finish().map_err(|e| e.to_string())?;
    }
    gz.finish().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;

    const COMMIT_A: &str = "11cca98b4a55da3118f51e749ac62faefbc21b3b";
    const COMMIT_B: &str = "22cca98b4a55da3118f51e749ac62faefbc21b3b";

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-vstack-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        {
            let mut ar = tar::Builder::new(&mut gz);
            for (path, data) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                ar.append_data(&mut header, *path, *data).unwrap();
            }
            ar.finish().unwrap();
        }
        gz.finish().unwrap()
    }

    #[test]
    fn valid_archive_installs_updates_and_removes_deleted_skills() {
        let cfg = scratch("install");
        let root = format!("blackviperxiii-ui-vstack-{COMMIT_A}");
        let ar1 = make_tar_gz(&[
            (&format!("{root}/VERSION"), b"1.4.0\n"),
            (&format!("{root}/LICENSE"), b"MIT\n"),
            (
                &format!("{root}/plugin.json"),
                br#"{"name":"vstack","version":"1.4.0"}"#,
            ),
            (
                &format!("{root}/skills/alpha/SKILL.md"),
                b"---\nname: alpha\ndescription: A.\n---\nBody A1\n",
            ),
            (
                &format!("{root}/skills/beta/SKILL.md"),
                b"---\nname: beta\ndescription: B.\n---\nBody B\n",
            ),
        ]);
        let st1 = install_archive(&cfg, &ar1, COMMIT_A, Some("v1.4.0".into()), 10_000).unwrap();
        assert_eq!(st1.version, "1.4.0");
        assert_eq!(st1.skill_count, 2);
        assert!(active_synced_skills_dir(&cfg, "v1.4.0").is_some());
        assert_eq!(
            fs::read_to_string(skills_dir(&cfg).join("alpha/SKILL.md")).unwrap(),
            "---\nname: alpha\ndescription: A.\n---\nBody A1\n"
        );

        // Update alpha, delete beta, add gamma.
        let root2 = format!("blackviperxiii-ui-vstack-{COMMIT_B}");
        let ar2 = make_tar_gz(&[
            (&format!("{root2}/VERSION"), b"1.4.1\n"),
            (
                &format!("{root2}/skills/alpha/SKILL.md"),
                b"---\nname: alpha\ndescription: A.\n---\nBody A2\n",
            ),
            (
                &format!("{root2}/skills/gamma/SKILL.md"),
                b"---\nname: gamma\ndescription: G.\n---\nBody G\n",
            ),
        ]);
        let st2 = install_archive(&cfg, &ar2, COMMIT_B, None, 20_000).unwrap();
        assert_eq!(st2.version, "1.4.1");
        assert_eq!(st2.commit, COMMIT_B);
        assert!(!skills_dir(&cfg).join("beta").exists());
        assert!(skills_dir(&cfg).join("gamma/SKILL.md").is_file());
        assert_eq!(
            fs::read_to_string(skills_dir(&cfg).join("alpha/SKILL.md")).unwrap(),
            "---\nname: alpha\ndescription: A.\n---\nBody A2\n"
        );

        let view = status(&cfg, "v1.4.0", COMMIT_A, 58);
        assert_eq!(view.source, "synced");
        assert_eq!(view.version, "1.4.1");
        assert_eq!(view.commit, COMMIT_B);
        assert_eq!(view.last_sync_ms, Some(20_000));
        assert_eq!(view.skill_count, 2);
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn bad_or_corrupt_archive_never_wipes_existing_skills() {
        let cfg = scratch("bad-keep");
        let root = format!("blackviperxiii-ui-vstack-{COMMIT_A}");
        let good = make_tar_gz(&[
            (&format!("{root}/VERSION"), b"1.4.0\n"),
            (
                &format!("{root}/skills/alpha/SKILL.md"),
                b"---\nname: alpha\ndescription: A.\n---\nKeep me\n",
            ),
        ]);
        install_archive(&cfg, &good, COMMIT_A, Some("v1.4.0".into()), 10_000).unwrap();

        // Corrupt bytes -> fails cleanly, leaves alpha intact.
        assert!(install_archive(&cfg, b"not-a-tarball", COMMIT_B, None, 15_000).is_err());
        assert_eq!(
            fs::read_to_string(skills_dir(&cfg).join("alpha/SKILL.md")).unwrap(),
            "---\nname: alpha\ndescription: A.\n---\nKeep me\n"
        );

        // Wrong plugin manifest -> rejected, leaves alpha intact.
        let spoof = make_tar_gz(&[
            (&format!("{root}/plugin.json"), br#"{"name":"other"}"#),
            (
                &format!("{root}/skills/alpha/SKILL.md"),
                b"---\nname: alpha\ndescription: A.\n---\nSpoofed\n",
            ),
        ]);
        assert!(install_archive(&cfg, &spoof, COMMIT_B, None, 16_000).is_err());
        assert_eq!(
            fs::read_to_string(skills_dir(&cfg).join("alpha/SKILL.md")).unwrap(),
            "---\nname: alpha\ndescription: A.\n---\nKeep me\n"
        );

        // Symlink inside archive -> rejected, leaves alpha intact.
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        {
            let mut ar = tar::Builder::new(&mut gz);
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(EntryType::Symlink);
            h.set_size(0);
            h.set_cksum();
            ar.append_link(&mut h, format!("{root}/skills/alpha/leak"), "/etc/passwd")
                .unwrap();
            ar.finish().unwrap();
        }
        let symlink_ar = gz.finish().unwrap();
        assert!(install_archive(&cfg, &symlink_ar, COMMIT_B, None, 17_000).is_err());
        assert_eq!(
            fs::read_to_string(skills_dir(&cfg).join("alpha/SKILL.md")).unwrap(),
            "---\nname: alpha\ndescription: A.\n---\nKeep me\n"
        );

        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn bundled_semver_newer_than_cached_disk_falls_back_to_bundled() {
        let cfg = scratch("semver");
        let root = format!("blackviperxiii-ui-vstack-{COMMIT_A}");
        let ar = make_tar_gz(&[
            (&format!("{root}/VERSION"), b"1.4.0\n"),
            (
                &format!("{root}/skills/alpha/SKILL.md"),
                b"---\nname: alpha\ndescription: A.\n---\nOld cached\n",
            ),
        ]);
        install_archive(&cfg, &ar, COMMIT_A, Some("v1.4.0".into()), 10_000).unwrap();
        assert!(active_synced_skills_dir(&cfg, "v1.4.0").is_some());
        assert!(active_synced_skills_dir(&cfg, "v1.5.0").is_none());
        let view = status(&cfg, "v1.5.0", COMMIT_B, 60);
        assert_eq!(view.source, "bundled");
        assert_eq!(view.version, "v1.5.0");
        assert_eq!(view.skill_count, 60);
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn sync_due_tracks_interval_and_check_errors() {
        let cfg = scratch("due");
        assert!(sync_due(&cfg, 10_000));
        record_check_result(&cfg, 10_000, Some("offline".into())).unwrap();
        assert!(!sync_due(&cfg, 10_000 + VSTACK_SYNC_INTERVAL_MS - 1));
        assert!(sync_due(&cfg, 10_000 + VSTACK_SYNC_INTERVAL_MS));
        let view = status(&cfg, "v1.4.0", COMMIT_A, 58);
        assert_eq!(view.source, "bundled");
        assert_eq!(view.last_check_ms, Some(10_000));
        assert_eq!(view.last_error.as_deref(), Some("offline"));
        let _ = fs::remove_dir_all(&cfg);
    }
}
