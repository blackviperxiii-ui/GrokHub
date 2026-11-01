//! Pinned llama.cpp `llama-server` download for this OS/arch into `{config}/bin`.
//! Starts when the user turns Local AI on (or presses Get engine): resumable
//! `.part` files, SHA-256 check, safe archive extract. Linux and Windows get
//! the Vulkan build (x86_64 and ARM64), which uses the GPU and falls back to
//! the CPU backend when no Vulkan driver loads. Apple Silicon gets the Metal
//! build; Intel Macs get llama.cpp's CPU build.

use crate::model_download::{self, sha256_file, Control, Progress};
use crate::ssrf::runtime_download_url_ok;
use flate2::read::GzDecoder;
use serde::Deserialize;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

const MANIFEST_JSON: &str = include_str!("../data/runtime_engine.json");

#[derive(Debug, Clone, Deserialize)]
struct ManifestFile {
    version: String,
    platforms: std::collections::BTreeMap<String, PlatformAsset>,
    /// Faster, larger builds offered on top of the standard one (`cuda`).
    #[serde(default)]
    variants: std::collections::BTreeMap<String, VariantFile>,
}

/// One optional build for one platform: a main archive with `llama-server`,
/// then runtime archives installed beside it (the CUDA runtime).
#[derive(Debug, Clone, Deserialize)]
struct VariantFile {
    platform: String,
    backend: String,
    /// Lowest NVIDIA driver (major version) the build runs on.
    #[serde(default)]
    min_driver: u32,
    assets: Vec<PlatformAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PlatformAsset {
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    /// The archive that carries `llama-server`.
    pub asset: PlatformAsset,
    pub platform_key: String,
    /// More archives whose libraries go next to the server (CUDA runtime).
    pub extras: Vec<PlatformAsset>,
    /// `None` for the standard build, `Some("cuda")` for a faster one.
    pub variant: Option<String>,
}

impl Manifest {
    /// Every archive, the server's first.
    pub fn assets(&self) -> impl Iterator<Item = &PlatformAsset> {
        std::iter::once(&self.asset).chain(self.extras.iter())
    }

    /// Bytes for the whole install.
    pub fn total_size(&self) -> u64 {
        self.assets().map(|a| a.size).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    TarGz,
    Zip,
}

impl PlatformAsset {
    pub fn archive_format(&self) -> Option<ArchiveFormat> {
        match self.format.as_str() {
            "tar.gz" => Some(ArchiveFormat::TarGz),
            "zip" => Some(ArchiveFormat::Zip),
            _ => None,
        }
    }

    pub fn source_host(&self) -> Option<&str> {
        self.url
            .strip_prefix("https://")
            .and_then(|r| r.split(['/', '?', '#']).next())
    }

    pub fn archive_file_name(&self) -> String {
        self.url
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("engine-archive")
            .to_string()
    }
}

/// Key for this machine (`linux-x86_64`, `windows-aarch64`, …).
pub fn platform_key() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux-x86_64"),
        ("linux", "aarch64") => Some("linux-aarch64"),
        ("windows", "x86_64") => Some("windows-x86_64"),
        ("windows", "aarch64") => Some("windows-aarch64"),
        ("macos", "aarch64") => Some("macos-aarch64"),
        ("macos", "x86_64") => Some("macos-x86_64"),
        _ => None,
    }
}

fn parse_manifest() -> Result<ManifestFile, String> {
    serde_json::from_str(MANIFEST_JSON).map_err(|e| format!("engine manifest: {e}"))
}

/// Pinned release + asset for this host, when one exists in the manifest.
pub fn manifest_for_host() -> Option<Manifest> {
    let key = platform_key()?;
    manifest_for_key(key)
}

pub fn manifest_for_key(key: &str) -> Option<Manifest> {
    let file = parse_manifest().ok()?;
    let asset = file.platforms.get(key)?.clone();
    asset.archive_format()?;
    runtime_download_url_ok(&asset.url).ok()?;
    Some(Manifest {
        version: file.version,
        asset,
        platform_key: key.to_string(),
        extras: Vec::new(),
        variant: None,
    })
}

/// The faster `backend` build (`cuda`) for platform `key`, with the lowest
/// driver it runs on.
pub fn variant_for_key(key: &str, backend: &str) -> Option<(Manifest, u32)> {
    let file = parse_manifest().ok()?;
    let v = file
        .variants
        .values()
        .find(|v| v.platform == key && v.backend == backend)?;
    for a in &v.assets {
        a.archive_format()?;
        runtime_download_url_ok(&a.url).ok()?;
    }
    let (asset, extras) = v.assets.split_first()?;
    Some((
        Manifest {
            version: file.version,
            asset: asset.clone(),
            platform_key: key.to_string(),
            extras: extras.to_vec(),
            variant: Some(v.backend.clone()),
        },
        v.min_driver,
    ))
}

/// The CUDA build for this machine when it has an NVIDIA GPU whose driver
/// is new enough. Never chosen by itself: Settings offers it.
pub fn faster_engine_for(key: &str, gpus: &[crate::local_profile::Gpu]) -> Option<Manifest> {
    use crate::local_profile::GpuVendor;
    let (m, min_driver) = variant_for_key(key, "cuda")?;
    gpus.iter()
        .any(|g| g.vendor == GpuVendor::Nvidia && driver_major(&g.driver) >= Some(min_driver))
        .then_some(m)
}

pub fn faster_engine_for_host(gpus: &[crate::local_profile::Gpu]) -> Option<Manifest> {
    faster_engine_for(platform_key()?, gpus)
}

/// `560.94` → 560. `None` when the driver version is unknown.
fn driver_major(v: &str) -> Option<u32> {
    v.trim().split('.').next()?.parse().ok()
}

const ENGINE_MARK: &str = "engine.json";

#[derive(serde::Serialize, Deserialize)]
struct EngineMark {
    version: String,
    #[serde(default)]
    variant: Option<String>,
}

/// Which build sits in `{config}/bin`: `Some("cuda")` for the faster one,
/// `None` for the standard build or an install from before this was kept.
pub fn installed_variant(config_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(bin_dir(config_dir).join(ENGINE_MARK)).ok()?;
    serde_json::from_str::<EngineMark>(&text).ok()?.variant
}

/// Bytes already downloaded toward `manifest`, over all its archives.
pub fn partial_total(bin: &Path, manifest: &Manifest) -> Option<u64> {
    let n: u64 = manifest
        .assets()
        .filter_map(|a| partial_bytes(bin, a))
        .sum();
    (n > 0).then_some(n)
}

/// Every platform entry must be HTTPS and on the GitHub allowlist.
pub fn validate_manifest_urls() -> Result<(), String> {
    let file = parse_manifest()?;
    let variants = file
        .variants
        .iter()
        .flat_map(|(key, v)| v.assets.iter().map(move |a| (key, a)));
    for (key, asset) in file.platforms.iter().chain(variants) {
        runtime_download_url_ok(&asset.url).map_err(|e| format!("{key}: {e}"))?;
        if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("{key}: sha256 must be 64 hex digits"));
        }
        if asset.archive_format().is_none() {
            return Err(format!("{key}: unknown archive format {}", asset.format));
        }
    }
    for (key, v) in &file.variants {
        if !file.platforms.contains_key(&v.platform) {
            return Err(format!("{key}: no standard build for {}", v.platform));
        }
        if v.assets.is_empty() {
            return Err(format!("{key}: no archives"));
        }
    }
    Ok(())
}

pub fn server_binary_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

pub fn bin_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("bin")
}

pub fn server_path(config_dir: &Path) -> PathBuf {
    bin_dir(config_dir).join(server_binary_name())
}

pub fn part_path_for_asset(bin: &Path, asset: &PlatformAsset) -> PathBuf {
    bin.join(format!("{}.part", asset.archive_file_name()))
}

pub fn partial_bytes(bin: &Path, asset: &PlatformAsset) -> Option<u64> {
    fs::metadata(part_path_for_asset(bin, asset))
        .ok()
        .map(|md| md.len())
        .filter(|n| *n > 0)
}

pub fn engine_installed(config_dir: &Path) -> bool {
    server_path(config_dir).is_file()
}

/// `llama-server` is available from env, `{config}/bin`, or `PATH`.
pub fn engine_binary_available(config_dir: &Path) -> bool {
    std::env::var_os("GROKHUB_LLAMA_SERVER")
        .map(PathBuf::from)
        .is_some_and(|p| p.is_file())
        || engine_installed(config_dir)
        || find_on_path(server_binary_name()).is_some()
}

/// Bytes source for [`download_engine`].
pub trait Fetch {
    fn open(&self, url: &str, offset: u64) -> Result<model_download::Opened, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done { bytes: u64, server: PathBuf },
    Paused { done: u64 },
    Cancelled,
    BadChecksum,
    Failed(String),
}

const CHUNK: usize = 256 * 1024;

/// Download every pinned archive, verify each, extract `llama-server` (+ libs)
/// into `config_dir/bin`. The libraries already there are replaced, so a
/// switch between the standard and the CUDA build leaves no stale backend.
pub fn download_engine(
    config_dir: &Path,
    manifest: &Manifest,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Outcome {
    if let Err(e) = crate::offline::refuse() {
        return Outcome::Failed(e);
    }
    let bin = bin_dir(config_dir);
    if let Err(e) = fs::create_dir_all(&bin) {
        return Outcome::Failed(format!("Couldn't create {}: {e}", bin.display()));
    }
    let total = manifest.total_size();
    let mut base = 0u64;
    let mut parts = Vec::new();
    for asset in manifest.assets() {
        let mut each = |p: Progress| {
            on(match p {
                Progress::Bytes { done, .. } => Progress::Bytes {
                    done: base + done,
                    total: Some(total),
                },
                other => other,
            })
        };
        match fetch_asset(&bin, asset, fetch, ctl, &mut each) {
            Ok(part) => parts.push(part),
            Err(Outcome::Paused { done }) => return Outcome::Paused { done: base + done },
            Err(out) => return out,
        }
        base += asset.size;
    }
    let remove_parts = |parts: &[PathBuf]| {
        for p in parts {
            let _ = fs::remove_file(p);
        }
    };
    let staging = bin.join(format!(".engine-extract-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    let fail = |e: String, staging: &Path, parts: &[PathBuf]| {
        remove_parts(parts);
        let _ = fs::remove_dir_all(staging);
        Outcome::Failed(e)
    };
    let mut trees = Vec::new();
    for (i, (asset, part)) in manifest.assets().zip(&parts).enumerate() {
        let Some(fmt) = asset.archive_format() else {
            return fail("unknown engine archive format".into(), &staging, &parts);
        };
        let tree = staging.join(i.to_string());
        if let Err(e) = extract_archive(part, fmt, &tree) {
            return fail(e, &staging, &parts);
        }
        trees.push(tree);
    }
    let Some(server_src) = find_server_in_tree(&trees[0], server_binary_name()) else {
        return fail(
            "The engine archive did not contain llama-server.".into(),
            &staging,
            &parts,
        );
    };
    let dest = server_path(config_dir);
    let dest_new = bin.join(format!("{}.new", server_binary_name()));
    clear_runtime_libs(&bin);
    let installed = install_server_and_libs(&server_src, &dest_new).and_then(|()| {
        trees[1..]
            .iter()
            .try_for_each(|tree| install_libs_from_tree(tree, &bin))
    });
    if let Err(e) = installed {
        let _ = fs::remove_file(&dest_new);
        return fail(e, &staging, &parts);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(mut perms) = fs::metadata(&dest_new).map(|m| m.permissions()) {
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&dest_new, perms);
        }
    }
    if let Err(e) = fs::rename(&dest_new, &dest) {
        let _ = fs::remove_file(&dest_new);
        return fail(
            format!("Couldn't install {}: {e}", dest.display()),
            &staging,
            &parts,
        );
    }
    let mark = EngineMark {
        version: manifest.version.clone(),
        variant: manifest.variant.clone(),
    };
    if let Ok(text) = serde_json::to_string(&mark) {
        let _ = fs::write(bin.join(ENGINE_MARK), text);
    }
    remove_parts(&parts);
    let _ = fs::remove_dir_all(&staging);
    Outcome::Done {
        bytes: total,
        server: dest,
    }
}

/// Download one archive into its `.part` file and check its SHA-256. A part
/// that is already whole is only checked, never asked for again.
fn fetch_asset(
    bin: &Path,
    asset: &PlatformAsset,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Result<PathBuf, Outcome> {
    let want = asset.sha256.to_ascii_lowercase();
    let part = part_path_for_asset(bin, asset);
    let have = partial_bytes(bin, asset).unwrap_or(0);
    if asset.size > 0 && have >= asset.size {
        on(Progress::Bytes {
            done: have,
            total: Some(asset.size),
        });
    } else {
        stream_asset(asset, &part, have, fetch, ctl, on)?;
    }
    on(Progress::Verifying);
    match sha256_file(&part) {
        Ok(got) if got == want => Ok(part),
        Ok(_) => {
            let _ = fs::remove_file(&part);
            Err(Outcome::BadChecksum)
        }
        Err(e) => Err(Outcome::Failed(format!(
            "Couldn't check {}: {e}",
            part.display()
        ))),
    }
}

fn stream_asset(
    asset: &PlatformAsset,
    part: &Path,
    have: u64,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Result<u64, Outcome> {
    let (mut body, start, total) = fetch
        .open(&asset.url, have)
        .map_err(|e| Outcome::Failed(format!("Couldn't download the engine: {e}")))?;
    let mut out = if start == 0 {
        File::create(part)
    } else {
        OpenOptions::new().append(true).open(part)
    }
    .map_err(|e| Outcome::Failed(format!("Couldn't write {}: {e}", part.display())))?;
    let mut done = start;
    on(Progress::Bytes { done, total });
    let mut buf = vec![0u8; CHUNK];
    loop {
        if ctl.paused() {
            return Err(Outcome::Paused { done });
        }
        if ctl.cancelled() {
            drop(out);
            let _ = fs::remove_file(part);
            return Err(Outcome::Cancelled);
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(Outcome::Failed(format!("The engine download stopped: {e}"))),
        };
        out.write_all(&buf[..n])
            .map_err(|e| Outcome::Failed(format!("Couldn't write {}: {e}", part.display())))?;
        done += n as u64;
        on(Progress::Bytes { done, total });
    }
    out.flush()
        .map_err(|e| Outcome::Failed(format!("Couldn't write {}: {e}", part.display())))?;
    if total.is_some_and(|t| done < t) {
        return Err(Outcome::Failed(format!(
            "The engine download ended early ({done} of {} bytes).",
            total.unwrap_or(0)
        )));
    }
    Ok(done)
}

/// Remove the engine libraries in `bin` before a new build goes in.
fn clear_runtime_libs(bin: &Path) {
    for entry in fs::read_dir(bin).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_file() && is_runtime_lib(&path) {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Copy every runtime library anywhere under `tree` into `bin` (a runtime
/// archive may keep them in a top folder of its own).
fn install_libs_from_tree(tree: &Path, bin: &Path) -> Result<(), String> {
    let mut stack = vec![tree.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() && is_runtime_lib(&path) {
                let name = entry.file_name();
                fs::copy(&path, bin.join(&name))
                    .map_err(|e| format!("copy {}: {e}", name.to_string_lossy()))?;
            }
        }
    }
    Ok(())
}

fn install_server_and_libs(server_src: &Path, dest_new: &Path) -> Result<(), String> {
    let parent = server_src
        .parent()
        .ok_or_else(|| "engine layout is invalid".to_string())?;
    if let Some(dir) = dest_new.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::copy(server_src, dest_new).map_err(|e| format!("copy llama-server: {e}"))?;
    for entry in fs::read_dir(parent).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if !is_runtime_lib(&path) {
            continue;
        }
        let name = entry.file_name();
        let label = name.to_string_lossy();
        let into = dest_new
            .parent()
            .ok_or_else(|| "engine layout is invalid".to_string())?
            .join(&name);
        fs::copy(&path, &into).map_err(|e| format!("copy {label}: {e}"))?;
    }
    Ok(())
}

fn is_runtime_lib(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    lib_for_os(name, std::env::consts::OS)
}

/// The engine's shared libraries: `.dll` on Windows, `.dylib` on macOS
/// (the server finds them through `@loader_path`), `.so*` elsewhere.
fn lib_for_os(name: &str, os: &str) -> bool {
    match os {
        "windows" => name.ends_with(".dll"),
        "macos" => name.ends_with(".dylib"),
        _ => name.contains(".so"),
    }
}

fn find_server_in_tree(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == name) {
                return Some(path);
            }
        }
    }
    None
}

fn safe_join(base: &Path, entry: &Path) -> Option<PathBuf> {
    if entry.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(base.join(entry))
}

fn extract_archive(archive: &Path, fmt: ArchiveFormat, into: &Path) -> Result<(), String> {
    fs::create_dir_all(into).map_err(|e| e.to_string())?;
    match fmt {
        ArchiveFormat::TarGz => extract_tar_gz(archive, into),
        ArchiveFormat::Zip => extract_zip(archive, into),
    }
}

/// Linux builds ship `libllama.so.0 -> libllama.so.0.6.0` symlinks. Each
/// link becomes a copy of its target (a bare name in the same folder), so
/// nothing points outside the extract folder and the loader finds real files.
fn extract_tar_gz(archive: &Path, into: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let decoder = GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    let mut links: Vec<(PathBuf, String)> = Vec::new();
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        let Some(dest) = safe_join(into, path.as_ref()) else {
            return Err("engine archive path escapes the extract folder".into());
        };
        let kind = entry.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            let target = entry
                .link_name()
                .map_err(|e| e.to_string())?
                .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()));
            if let Some(name) = target {
                links.push((dest, name));
            }
        } else if kind.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        } else if !kind.is_file() {
            continue;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = File::create(&dest).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }
    copy_links(&links)
}

/// Copy each link's target over it; a chain resolves over a few passes.
fn copy_links(links: &[(PathBuf, String)]) -> Result<(), String> {
    let mut left: Vec<&(PathBuf, String)> = links.iter().collect();
    while !left.is_empty() {
        let before = left.len();
        left.retain(|(dest, name)| {
            let Some(src) = dest.parent().map(|d| d.join(name)) else {
                return false;
            };
            if !src.is_file() {
                return true;
            }
            fs::copy(&src, dest).is_err()
        });
        if left.len() == before {
            let names: Vec<String> = left.iter().map(|(_, n)| n.clone()).collect();
            return Err(format!(
                "the engine archive links to files it doesn't contain: {}",
                names.join(", ")
            ));
        }
    }
    Ok(())
}

fn extract_zip(archive: &Path, into: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| "engine archive path escapes the extract folder".to_string())?;
        let dest = into.join(enclosed);
        if entry.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = File::create(&dest).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Find `llama-server` on `PATH` when env/config installs are absent.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    find_in_dirs(std::env::split_paths(&paths), name)
}

/// The first `dir/name` that is a file. An empty or relative entry would mean
/// whatever folder Loadout was started from, so only absolute folders count.
fn find_in_dirs(dirs: impl IntoIterator<Item = PathBuf>, name: &str) -> Option<PathBuf> {
    dirs.into_iter()
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Mutex;

    #[test]
    fn manifest_parses_and_every_url_is_https_and_allowlisted() {
        validate_manifest_urls().expect("manifest");
        let m = manifest_for_key("linux-x86_64").expect("linux asset");
        assert_eq!(m.version, "b12017");
        assert!(m.asset.url.starts_with("https://github.com/"));
        assert_eq!(m.asset.size, 35_665_113);
        // x86_64 gets the Vulkan build (GPU when a driver loads, CPU otherwise).
        assert_eq!(
            m.asset.archive_file_name(),
            "llama-b12017-bin-ubuntu-vulkan-x64.tar.gz"
        );
        let w = manifest_for_key("windows-x86_64").expect("windows asset");
        assert_eq!(
            w.asset.archive_file_name(),
            "llama-b12017-bin-win-vulkan-x64.zip"
        );
        assert_eq!(w.asset.size, 37_514_219);
        // Arm gets the Vulkan builds too; Macs get Metal (Apple Silicon) or CPU (Intel).
        let names: Vec<String> = [
            "linux-aarch64",
            "windows-aarch64",
            "macos-aarch64",
            "macos-x86_64",
        ]
        .iter()
        .map(|k| manifest_for_key(k).expect(k).asset.archive_file_name())
        .collect();
        assert_eq!(
            names,
            [
                "llama-b12017-bin-ubuntu-vulkan-arm64.tar.gz",
                "llama-b12017-bin-win-vulkan-arm64.zip",
                "llama-b12017-bin-macos-arm64.tar.gz",
                "llama-b12017-bin-macos-x64.tar.gz",
            ]
        );
    }

    #[test]
    fn every_supported_os_and_arch_has_a_pinned_engine() {
        let file = parse_manifest().expect("manifest");
        for key in [
            "linux-x86_64",
            "linux-aarch64",
            "windows-x86_64",
            "windows-aarch64",
            "macos-aarch64",
            "macos-x86_64",
        ] {
            assert!(file.platforms.contains_key(key), "{key} has no engine");
            assert!(manifest_for_key(key).is_some(), "{key} is not usable");
        }
        if let Some(key) = platform_key() {
            assert!(manifest_for_key(key).is_some(), "this host: {key}");
        }
    }

    #[test]
    fn each_os_installs_its_own_kind_of_library() {
        assert!(lib_for_os("libggml-metal.0.28.0.dylib", "macos"));
        assert!(lib_for_os("libllama.0.dylib", "macos"));
        assert!(!lib_for_os("llama-server", "macos"));
        assert!(!lib_for_os("LICENSE", "macos"));
        assert!(lib_for_os("ggml-vulkan.dll", "windows"));
        assert!(!lib_for_os("libggml.dylib", "windows"));
        assert!(lib_for_os("libggml-vulkan.so", "linux"));
        assert!(lib_for_os("libllama.so.0.8.0", "linux"));
        assert!(!lib_for_os("ggml.dll", "linux"));
    }

    struct FakeServer {
        body: Vec<u8>,
        /// Other archives by URL; anything else gets `body`.
        more: Vec<(String, Vec<u8>)>,
        ranges: bool,
        opens: Mutex<Vec<u64>>,
        urls: Mutex<Vec<String>>,
    }

    impl FakeServer {
        fn new(body: &[u8]) -> Self {
            Self {
                body: body.to_vec(),
                more: Vec::new(),
                ranges: true,
                opens: Mutex::new(Vec::new()),
                urls: Mutex::new(Vec::new()),
            }
        }
    }

    impl Fetch for FakeServer {
        fn open(&self, url: &str, offset: u64) -> Result<model_download::Opened, String> {
            self.opens.lock().unwrap().push(offset);
            self.urls.lock().unwrap().push(url.to_string());
            let body = self
                .more
                .iter()
                .find(|(u, _)| u == url)
                .map(|(_, b)| b)
                .unwrap_or(&self.body);
            let start = if self.ranges { offset as usize } else { 0 };
            let total = Some(body.len() as u64);
            Ok((
                Box::new(Cursor::new(body[start..].to_vec())),
                start as u64,
                total,
            ))
        }
    }

    fn tiny_zip_with_server() -> Vec<u8> {
        let name = server_binary_name();
        let mut buf = Vec::new();
        {
            use std::io::Write;
            use zip::write::SimpleFileOptions;
            use zip::write::ZipWriter;
            use zip::CompressionMethod;
            let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file(format!("bin/{name}"), options).unwrap();
            zip.write_all(b"fake-server").unwrap();
            if cfg!(windows) {
                zip.start_file("bin/ggml.dll", options).unwrap();
                zip.write_all(b"fake-dll").unwrap();
            }
            zip.finish().unwrap();
        }
        buf
    }

    fn tiny_tgz_with_server() -> Vec<u8> {
        let mut inner = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut inner);
            let data = b"fake-server";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, server_binary_name(), &data[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let mut enc = Vec::new();
        {
            use std::io::Write;
            let mut gz = flate2::write::GzEncoder::new(&mut enc, flate2::Compression::default());
            gz.write_all(&inner).unwrap();
            gz.finish().unwrap();
        }
        enc
    }

    fn tgz_with_lib_links() -> Vec<u8> {
        let mut inner = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut inner);
            let mut file = |name: &str, data: &[u8]| {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o755);
                h.set_cksum();
                builder
                    .append_data(&mut h, format!("llama-b0/{name}"), data)
                    .unwrap();
            };
            file(server_binary_name(), b"fake-server");
            file("libllama.so.0.6.0", b"real-lib");
            // Links listed before and after their targets, as release tarballs do.
            for (link, target) in [
                ("libllama.so", "libllama.so.0"),
                ("libllama.so.0", "libllama.so.0.6.0"),
            ] {
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_size(0);
                h.set_cksum();
                builder
                    .append_link(&mut h, format!("llama-b0/{link}"), target)
                    .unwrap();
            }
            builder.finish().unwrap();
        }
        let mut enc = Vec::new();
        {
            use std::io::Write;
            let mut gz = flate2::write::GzEncoder::new(&mut enc, flate2::Compression::default());
            gz.write_all(&inner).unwrap();
            gz.finish().unwrap();
        }
        enc
    }

    #[test]
    fn linked_libraries_install_as_real_files_not_empty_ones() {
        let body = tgz_with_lib_links();
        let dir = tmp("links");
        let cfg = dir.join("cfg");
        let manifest = test_manifest(&body, ArchiveFormat::TarGz);
        let out = download_engine(
            &cfg,
            &manifest,
            &FakeServer::new(&body),
            &Control::default(),
            &mut |_| {},
        );
        assert!(matches!(out, Outcome::Done { .. }), "{out:?}");
        if !cfg!(windows) {
            for name in ["libllama.so", "libllama.so.0", "libllama.so.0.6.0"] {
                assert_eq!(
                    fs::read(bin_dir(&cfg).join(name)).unwrap(),
                    b"real-lib",
                    "{name}"
                );
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_link_to_a_missing_file_fails_the_install() {
        let dir = tmp("deadlink");
        fs::create_dir_all(&dir).unwrap();
        let err = copy_links(&[(dir.join("libx.so.0"), "libx.so.0.1".into())]).unwrap_err();
        assert_eq!(
            err,
            "the engine archive links to files it doesn't contain: libx.so.0.1"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn test_manifest(body: &[u8], fmt: ArchiveFormat) -> Manifest {
        let format = match fmt {
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::Zip => "zip",
        };
        Manifest {
            version: "test".into(),
            platform_key: "test".into(),
            asset: PlatformAsset {
                url: "https://github.com/ggml-org/llama.cpp/releases/download/b0/fake.bin".into(),
                size: body.len() as u64,
                sha256: model_download::sha256_hex(body),
                format: format.into(),
            },
            extras: Vec::new(),
            variant: None,
        }
    }

    /// A tar.gz of `(path, bytes)` files.
    fn tgz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut inner = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut inner);
            for (name, data) in files {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o755);
                h.set_cksum();
                builder.append_data(&mut h, name, *data).unwrap();
            }
            builder.finish().unwrap();
        }
        let mut enc = Vec::new();
        {
            use std::io::Write;
            let mut gz = flate2::write::GzEncoder::new(&mut enc, flate2::Compression::default());
            gz.write_all(&inner).unwrap();
            gz.finish().unwrap();
        }
        enc
    }

    fn lib(stem: &str) -> String {
        if cfg!(windows) {
            format!("{stem}.dll")
        } else if cfg!(target_os = "macos") {
            format!("lib{stem}.dylib")
        } else {
            format!("lib{stem}.so")
        }
    }

    fn asset_for(name: &str, body: &[u8]) -> PlatformAsset {
        PlatformAsset {
            url: format!("https://github.com/ggml-org/llama.cpp/releases/download/b0/{name}"),
            size: body.len() as u64,
            sha256: model_download::sha256_hex(body),
            format: "tar.gz".into(),
        }
    }

    /// The CUDA build's shape: the server archive, then a runtime archive
    /// whose libraries sit in a folder of their own.
    fn cuda_like() -> (Manifest, FakeServer) {
        let server = server_binary_name();
        let main = tgz(&[
            (&format!("llama-b0/{server}"), b"cuda-server"),
            (&format!("llama-b0/{}", lib("ggml-cuda")), b"cuda-backend"),
        ]);
        let rt = tgz(&[(&format!("cudart-b0/{}", lib("cudart")), b"cuda-runtime")]);
        let main_asset = asset_for("main.tar.gz", &main);
        let rt_asset = asset_for("cudart.tar.gz", &rt);
        let mut srv = FakeServer::new(&main);
        srv.more.push((rt_asset.url.clone(), rt));
        let m = Manifest {
            version: "b0".into(),
            platform_key: "test".into(),
            asset: main_asset,
            extras: vec![rt_asset],
            variant: Some("cuda".into()),
        };
        (m, srv)
    }

    #[test]
    fn the_cuda_build_installs_its_runtime_beside_the_server_and_drops_the_old_backend() {
        let dir = tmp("cuda");
        let cfg = dir.join("cfg");
        fs::create_dir_all(bin_dir(&cfg)).unwrap();
        let old = bin_dir(&cfg).join(lib("ggml-vulkan"));
        fs::write(&old, b"old-backend").unwrap();
        let (m, srv) = cuda_like();
        let mut seen = Vec::new();
        let out = download_engine(&cfg, &m, &srv, &Control::default(), &mut |p| {
            if let Progress::Bytes { done, total } = p {
                seen.push((done, total));
            }
        });
        assert!(matches!(out, Outcome::Done { .. }), "{out:?}");
        let bin = bin_dir(&cfg);
        assert_eq!(fs::read(server_path(&cfg)).unwrap(), b"cuda-server");
        assert_eq!(
            fs::read(bin.join(lib("ggml-cuda"))).unwrap(),
            b"cuda-backend"
        );
        assert_eq!(fs::read(bin.join(lib("cudart"))).unwrap(), b"cuda-runtime");
        assert!(!old.exists(), "a stale backend would load beside CUDA");
        assert_eq!(installed_variant(&cfg).as_deref(), Some("cuda"));
        // Progress counts the whole install, and only goes up.
        let total = m.total_size();
        assert!(seen.iter().all(|(_, t)| *t == Some(total)), "{seen:?}");
        assert_eq!(seen.last().map(|(d, _)| *d), Some(total));
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
        assert!(partial_total(&bin, &m).is_none(), "parts are cleaned up");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_whole_archive_is_not_asked_for_again_on_resume() {
        let dir = tmp("cuda-resume");
        let cfg = dir.join("cfg");
        let (m, srv) = cuda_like();
        let bin = bin_dir(&cfg);
        fs::create_dir_all(&bin).unwrap();
        // The first archive finished before a pause.
        fs::write(part_path_for_asset(&bin, &m.asset), &srv.body).unwrap();
        assert_eq!(partial_total(&bin, &m), Some(m.asset.size));
        let out = download_engine(&cfg, &m, &srv, &Control::default(), &mut |_| {});
        assert!(matches!(out, Outcome::Done { .. }), "{out:?}");
        assert_eq!(*srv.urls.lock().unwrap(), vec![m.extras[0].url.clone()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_standard_build_marks_no_variant() {
        let body = tiny_tgz_with_server();
        let dir = tmp("plain-mark");
        let cfg = dir.join("cfg");
        let m = test_manifest(&body, ArchiveFormat::TarGz);
        let out = download_engine(
            &cfg,
            &m,
            &FakeServer::new(&body),
            &Control::default(),
            &mut |_| {},
        );
        assert!(matches!(out, Outcome::Done { .. }), "{out:?}");
        assert_eq!(installed_variant(&cfg), None);
        let _ = fs::remove_dir_all(&dir);
    }

    fn gpu(vendor: crate::local_profile::GpuVendor, driver: &str) -> crate::local_profile::Gpu {
        crate::local_profile::Gpu {
            vendor,
            name: "card".into(),
            vram_mb: 8192,
            free_vram_mb: None,
            integrated: false,
            unified: false,
            driver: driver.into(),
        }
    }

    #[test]
    fn the_cuda_build_is_offered_only_for_an_nvidia_card_with_a_new_enough_driver() {
        use crate::local_profile::GpuVendor::*;
        for key in ["windows-x86_64", "linux-x86_64"] {
            let m = faster_engine_for(key, &[gpu(Nvidia, "560.94")]).expect(key);
            assert_eq!(m.variant.as_deref(), Some("cuda"));
            assert_eq!(m.extras.len(), 1, "{key}: the CUDA runtime comes with it");
            assert!(
                m.total_size() > 400 * 1024 * 1024,
                "{key}: it is a big download"
            );
            assert!(faster_engine_for(key, &[gpu(Amd, "24.10")]).is_none());
            assert!(faster_engine_for(key, &[gpu(Nvidia, "470.256")]).is_none());
            assert!(faster_engine_for(key, &[gpu(Nvidia, "")]).is_none());
            assert!(faster_engine_for(key, &[]).is_none());
        }
        // An NVIDIA card next to an iGPU still counts.
        assert!(
            faster_engine_for("linux-x86_64", &[gpu(Intel, "xe"), gpu(Nvidia, "575.64")]).is_some()
        );
        // No CUDA build is pinned for ARM or Macs.
        for key in [
            "linux-aarch64",
            "windows-aarch64",
            "macos-aarch64",
            "macos-x86_64",
        ] {
            assert!(
                faster_engine_for(key, &[gpu(Nvidia, "560.94")]).is_none(),
                "{key}"
            );
        }
    }

    fn tmp(label: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("grokhub-runtime-dl-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn bad_checksum_deletes_part_and_installs_nothing() {
        let body = tiny_tgz_with_server();
        let dir = tmp("badsum");
        let cfg = dir.join("cfg");
        let mut manifest = test_manifest(&body, ArchiveFormat::TarGz);
        manifest.asset.sha256 = "0".repeat(64);
        let srv = FakeServer::new(&body);
        let out = download_engine(&cfg, &manifest, &srv, &Control::default(), &mut |_| {});
        assert_eq!(out, Outcome::BadChecksum);
        assert!(!server_path(&cfg).exists());
        assert!(partial_bytes(&bin_dir(&cfg), &manifest.asset).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resume_from_partial_works() {
        let body = tiny_tgz_with_server();
        let dir = tmp("resume");
        let cfg = dir.join("cfg");
        let manifest = test_manifest(&body, ArchiveFormat::TarGz);
        let part = part_path_for_asset(&bin_dir(&cfg), &manifest.asset);
        fs::create_dir_all(part.parent().unwrap()).unwrap();
        fs::write(&part, &body[..20]).unwrap();
        let srv = FakeServer::new(&body);
        let out = download_engine(&cfg, &manifest, &srv, &Control::default(), &mut |_| {});
        assert!(matches!(out, Outcome::Done { .. }), "{out:?}");
        assert!(server_path(&cfg).is_file());
        assert_eq!(*srv.opens.lock().unwrap(), vec![20]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn zip_extract_installs_server_and_dll_on_windows_layout() {
        let body = tiny_zip_with_server();
        let dir = tmp("zip");
        let cfg = dir.join("cfg");
        let manifest = test_manifest(&body, ArchiveFormat::Zip);
        let out = download_engine(
            &cfg,
            &manifest,
            &FakeServer::new(&body),
            &Control::default(),
            &mut |_| {},
        );
        assert!(matches!(out, Outcome::Done { .. }), "{out:?}");
        assert!(server_path(&cfg).is_file());
        if cfg!(windows) {
            let dll = bin_dir(&cfg).join("ggml.dll");
            assert!(dll.is_file(), "dll beside server");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_allowlisted_url_is_rejected() {
        assert!(runtime_download_url_ok("https://evil.example/x").is_err());
        assert!(runtime_download_url_ok("http://github.com/x").is_err());
    }

    #[test]
    fn find_on_path_finds_an_executable_name() {
        let dir = tmp("path");
        fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("llama-server");
        fs::write(&bin, b"x").unwrap();
        let prev = std::env::var_os("PATH");
        std::env::set_var("PATH", &dir);
        assert_eq!(find_on_path("llama-server").unwrap(), bin);
        match prev {
            Some(p) => std::env::set_var("PATH", p),
            None => std::env::remove_var("PATH"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relative_or_empty_path_entry_never_finds_the_engine() {
        // Tests run from the crate folder, so `src/lib.rs` exists relative to it.
        let dir = tmp("path-rel");
        fs::create_dir_all(&dir).unwrap();
        let dirs = || [PathBuf::new(), PathBuf::from("."), PathBuf::from("src")];
        assert!(Path::new("src").join("lib.rs").is_file());
        assert_eq!(find_in_dirs(dirs(), "lib.rs"), None);
        let lib = dir.join("lib.rs");
        fs::write(&lib, b"x").unwrap();
        assert_eq!(
            find_in_dirs(dirs().into_iter().chain([dir.clone()]), "lib.rs"),
            Some(lib)
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
