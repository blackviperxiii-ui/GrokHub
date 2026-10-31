//! Pinned llama.cpp `llama-server` download for this OS/arch into `{config}/bin`.
//! Starts when the user turns Local AI on (or presses Get engine): resumable
//! `.part` files, SHA-256 check, safe archive extract. x86_64 gets the Vulkan
//! build, which uses an AMD, Intel or NVIDIA GPU and falls back to the CPU
//! backend when no Vulkan driver loads.

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
    pub asset: PlatformAsset,
    pub platform_key: String,
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
    })
}

/// Every platform entry must be HTTPS and on the GitHub allowlist.
pub fn validate_manifest_urls() -> Result<(), String> {
    let file = parse_manifest()?;
    for (key, asset) in &file.platforms {
        runtime_download_url_ok(&asset.url).map_err(|e| format!("{key}: {e}"))?;
        if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("{key}: sha256 must be 64 hex digits"));
        }
        if asset.archive_format().is_none() {
            return Err(format!("{key}: unknown archive format {}", asset.format));
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
    fn open(&self, offset: u64) -> Result<model_download::Opened, String>;
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

/// Download the pinned archive, verify, extract `llama-server` (+ libs) into `config_dir/bin`.
pub fn download_engine(
    config_dir: &Path,
    manifest: &Manifest,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Outcome {
    let bin = bin_dir(config_dir);
    if let Err(e) = fs::create_dir_all(&bin) {
        return Outcome::Failed(format!("Couldn't create {}: {e}", bin.display()));
    }
    let want = manifest.asset.sha256.to_ascii_lowercase();
    let part = part_path_for_asset(&bin, &manifest.asset);
    let have = partial_bytes(&bin, &manifest.asset).unwrap_or(0);
    let (mut body, start, total) = match fetch.open(have) {
        Ok(x) => x,
        Err(e) => return Outcome::Failed(format!("Couldn't download the engine: {e}")),
    };
    let mut out = match if start == 0 {
        File::create(&part)
    } else {
        OpenOptions::new().append(true).open(&part)
    } {
        Ok(f) => f,
        Err(e) => return Outcome::Failed(format!("Couldn't write {}: {e}", part.display())),
    };
    let mut done = start;
    on(Progress::Bytes { done, total });
    let mut buf = vec![0u8; CHUNK];
    loop {
        if ctl.paused() {
            return Outcome::Paused { done };
        }
        if ctl.cancelled() {
            drop(out);
            let _ = fs::remove_file(&part);
            return Outcome::Cancelled;
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Outcome::Failed(format!("The engine download stopped: {e}")),
        };
        if let Err(e) = out.write_all(&buf[..n]) {
            return Outcome::Failed(format!("Couldn't write {}: {e}", part.display()));
        }
        done += n as u64;
        on(Progress::Bytes { done, total });
    }
    if let Err(e) = out.flush() {
        return Outcome::Failed(format!("Couldn't write {}: {e}", part.display()));
    }
    drop(out);
    if total.is_some_and(|t| done < t) {
        return Outcome::Failed(format!(
            "The engine download ended early ({done} of {} bytes).",
            total.unwrap_or(0)
        ));
    }
    on(Progress::Verifying);
    match sha256_file(&part) {
        Ok(got) if got == want => {}
        Ok(_) => {
            let _ = fs::remove_file(&part);
            return Outcome::BadChecksum;
        }
        Err(e) => return Outcome::Failed(format!("Couldn't check {}: {e}", part.display())),
    }
    let fmt = match manifest.asset.archive_format() {
        Some(f) => f,
        None => return Outcome::Failed("unknown engine archive format".into()),
    };
    let staging = bin.join(format!(".engine-extract-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    if let Err(e) = extract_archive(&part, fmt, &staging) {
        let _ = fs::remove_file(&part);
        let _ = fs::remove_dir_all(&staging);
        return Outcome::Failed(e);
    }
    let server_src = find_server_in_tree(&staging, server_binary_name())
        .ok_or_else(|| "The engine archive did not contain llama-server.".to_string());
    let server_src = match server_src {
        Ok(p) => p,
        Err(e) => {
            let _ = fs::remove_file(&part);
            let _ = fs::remove_dir_all(&staging);
            return Outcome::Failed(e);
        }
    };
    let dest = server_path(config_dir);
    let dest_new = bin.join(format!("{}.new", server_binary_name()));
    if let Err(e) = install_server_and_libs(&server_src, &dest_new) {
        let _ = fs::remove_file(&part);
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_file(&dest_new);
        return Outcome::Failed(e);
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
        let _ = fs::remove_file(&part);
        let _ = fs::remove_dir_all(&staging);
        return Outcome::Failed(format!("Couldn't install {}: {e}", dest.display()));
    }
    let _ = fs::remove_file(&part);
    let _ = fs::remove_dir_all(&staging);
    Outcome::Done {
        bytes: done,
        server: dest,
    }
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
    if cfg!(windows) {
        name.ends_with(".dll")
    } else {
        name.contains(".so")
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
        assert_eq!(m.version, "b11602");
        assert!(m.asset.url.starts_with("https://github.com/"));
        assert_eq!(m.asset.size, 30_359_972);
        // x86_64 gets the Vulkan build (GPU when a driver loads, CPU otherwise).
        assert_eq!(
            m.asset.archive_file_name(),
            "llama-b11602-bin-ubuntu-vulkan-x64.tar.gz"
        );
        let w = manifest_for_key("windows-x86_64").expect("windows asset");
        assert_eq!(
            w.asset.archive_file_name(),
            "llama-b11602-bin-win-vulkan-x64.zip"
        );
        assert_eq!(w.asset.size, 32_127_294);
    }

    struct FakeServer {
        body: Vec<u8>,
        ranges: bool,
        opens: Mutex<Vec<u64>>,
    }

    impl FakeServer {
        fn new(body: &[u8]) -> Self {
            Self {
                body: body.to_vec(),
                ranges: true,
                opens: Mutex::new(Vec::new()),
            }
        }
    }

    impl Fetch for FakeServer {
        fn open(&self, offset: u64) -> Result<model_download::Opened, String> {
            self.opens.lock().unwrap().push(offset);
            let start = if self.ranges { offset as usize } else { 0 };
            let total = Some(self.body.len() as u64);
            Ok((
                Box::new(Cursor::new(self.body[start..].to_vec())),
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
