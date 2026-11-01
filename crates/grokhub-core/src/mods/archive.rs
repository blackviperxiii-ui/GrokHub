//! A downloaded mod before it goes anywhere near a game: what kind of file
//! it is, whether it matches the hash its source published, and, for a zip,
//! every entry checked and unpacked into a staging folder.
//!
//! Loadout never runs anything from a mod. An archive with an installer
//! (FOMOD, an exe, a script) is handed to the user's mod manager instead.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha512};

use super::source::FileHash;

/// What a download is, from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    SevenZip,
    Rar,
    /// Anything else: placed as one file (a Minecraft `.jar`) or refused.
    Plain,
}

pub fn sniff(path: &Path) -> std::io::Result<Kind> {
    let mut head = [0u8; 6];
    let n = File::open(path)?.read(&mut head)?;
    let head = &head[..n];
    Ok(
        if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
            Kind::Zip
        } else if head.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
            Kind::SevenZip
        } else if head.starts_with(b"Rar!") {
            Kind::Rar
        } else {
            Kind::Plain
        },
    )
}

/// How much one archive may unpack to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub files: usize,
    pub total: u64,
    pub file: u64,
}

pub const LIMITS: Limits = Limits {
    files: 20_000,
    total: 4 << 30,
    file: 2 << 30,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    /// An entry that would land outside the staging folder, or a link.
    Unsafe(String),
    TooBig(String),
    /// Needs a mod manager: an installer, a script, or a format Loadout
    /// doesn't unpack.
    NeedsManager(String),
    Bad(String),
}

impl ArchiveError {
    pub fn line(&self) -> String {
        match self {
            ArchiveError::Unsafe(why) => format!("Loadout won't unpack this archive: {why}."),
            ArchiveError::TooBig(why) => format!("This archive is too big to unpack: {why}."),
            ArchiveError::NeedsManager(why) => {
                format!("{why} Install it with your mod manager instead.")
            }
            ArchiveError::Bad(why) => format!("This archive couldn't be read: {why}."),
        }
    }
}

/// File types a mod manager or the user runs; Loadout never places them.
const RUNS: &[&str] = &["exe", "msi", "bat", "cmd", "com", "ps1", "vbs", "scr", "sh"];

fn runs(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| RUNS.contains(&e.to_ascii_lowercase().as_str()))
}

/// An archive entry's name as a path inside the staging folder, or `None`
/// when it is absolute, has a drive, climbs with `..`, or is empty.
pub fn safe_rel(name: &str) -> Option<PathBuf> {
    if name.contains('\0') {
        return None;
    }
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.as_bytes().get(1) == Some(&b':') {
        return None;
    }
    let mut out = PathBuf::new();
    for comp in Path::new(&name).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn hex_of(path: &Path, mut h: impl FnMut(&[u8])) -> std::io::Result<()> {
    let mut f = File::open(path)?;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        h(&buf[..n]);
    }
}

/// Check a download against the hash its source published. `Ok(true)`
/// when it matched, `Ok(false)` when the source gave no hash Loadout can
/// check (none, or MD5 only), `Err` when it doesn't match.
pub fn verify(path: &Path, hash: Option<&FileHash>) -> Result<bool, String> {
    let read = |e: std::io::Error| format!("Couldn't read the download: {e}");
    let (want, got) = match hash {
        Some(FileHash::Sha512(want)) => {
            let mut h = Sha512::new();
            hex_of(path, |b| h.update(b)).map_err(read)?;
            (want, hex::encode(h.finalize()))
        }
        Some(FileHash::Sha1(want)) => {
            let mut h = <sha1::Sha1 as sha1::Digest>::new();
            hex_of(path, |b| sha1::Digest::update(&mut h, b)).map_err(read)?;
            (want, hex::encode(sha1::Digest::finalize(h)))
        }
        Some(FileHash::Md5(_)) | None => return Ok(false),
    };
    if got.eq_ignore_ascii_case(want.trim()) {
        Ok(true)
    } else {
        Err(
            "The download doesn't match the hash its source published, so nothing was installed."
                .into(),
        )
    }
}

/// Unpack a zip into `into`, checking every entry first: no paths out of
/// the folder, no links, no installers or scripts, within `limits`.
/// Returns the files, relative to `into`, sorted.
pub fn unpack_zip(
    archive: &Path,
    into: &Path,
    limits: &Limits,
) -> Result<Vec<PathBuf>, ArchiveError> {
    let bad = |e: &dyn std::fmt::Display| ArchiveError::Bad(e.to_string());
    let mut zip =
        zip::ZipArchive::new(File::open(archive).map_err(|e| bad(&e))?).map_err(|e| bad(&e))?;
    if zip.len() > limits.files {
        return Err(ArchiveError::TooBig(format!("{} files", zip.len())));
    }
    let mut entries: Vec<(usize, PathBuf)> = Vec::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index_raw(i).map_err(|e| bad(&e))?;
        let name = f.name().map_err(|e| bad(&e))?.to_string();
        let rel = safe_rel(&name)
            .ok_or_else(|| ArchiveError::Unsafe(format!("\"{name}\" points outside the mod")))?;
        if f.is_symlink() {
            return Err(ArchiveError::Unsafe(format!("\"{name}\" is a link")));
        }
        if f.is_dir() {
            continue;
        }
        let low = name.replace('\\', "/").to_ascii_lowercase();
        if low.starts_with("fomod/") || low.contains("/fomod/") {
            return Err(ArchiveError::NeedsManager(
                "This mod has its own installer (FOMOD).".into(),
            ));
        }
        if runs(&rel) {
            return Err(ArchiveError::NeedsManager(format!(
                "This mod includes a program or script ({name})."
            )));
        }
        if f.size() > limits.file {
            return Err(ArchiveError::TooBig(format!(
                "{name} is {} bytes",
                f.size()
            )));
        }
        total += f.size();
        if total > limits.total {
            return Err(ArchiveError::TooBig(format!("over {} bytes", limits.total)));
        }
        entries.push((i, rel));
    }
    let mut written = 0u64;
    for (i, rel) in &entries {
        let dest = into.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| bad(&e))?;
        }
        let f = zip.by_index(*i).map_err(|e| bad(&e))?;
        let mut out = File::create(&dest).map_err(|e| bad(&e))?;
        // Sizes in the header can lie; the reader is capped either way.
        let n = std::io::copy(&mut f.take(limits.file + 1), &mut out).map_err(|e| bad(&e))?;
        written += n;
        if n > limits.file || written > limits.total {
            return Err(ArchiveError::TooBig(
                "it unpacks bigger than it says".into(),
            ));
        }
        out.flush().map_err(|e| bad(&e))?;
    }
    let mut files: Vec<PathBuf> = entries.into_iter().map(|(_, r)| r).collect();
    files.sort();
    Ok(files)
}

/// Stage a download Loadout places as it is (a Minecraft `.jar`).
pub fn stage_plain(file: &Path, name: &str, into: &Path) -> Result<Vec<PathBuf>, ArchiveError> {
    let rel = safe_rel(name)
        .filter(|r| r.components().count() == 1)
        .ok_or_else(|| ArchiveError::Unsafe(format!("\"{name}\" isn't a plain file name")))?;
    if runs(&rel) {
        return Err(ArchiveError::NeedsManager(format!(
            "This download is a program ({name})."
        )));
    }
    std::fs::create_dir_all(into).map_err(|e| ArchiveError::Bad(e.to_string()))?;
    std::fs::copy(file, into.join(&rel)).map_err(|e| ArchiveError::Bad(e.to_string()))?;
    Ok(vec![rel])
}

/// Stage any download: unpack a zip, place a `.jar` as it is, and hand
/// 7z, rar and anything else to the user's mod manager.
pub fn stage(file: &Path, name: &str, into: &Path) -> Result<Vec<PathBuf>, ArchiveError> {
    let kind = sniff(file).map_err(|e| ArchiveError::Bad(e.to_string()))?;
    let jar = name.to_ascii_lowercase().ends_with(".jar");
    match kind {
        _ if jar => stage_plain(file, name, into),
        Kind::Zip => unpack_zip(file, into, &LIMITS),
        Kind::SevenZip => Err(ArchiveError::NeedsManager(
            "Loadout doesn't unpack 7z archives yet.".into(),
        )),
        Kind::Rar => Err(ArchiveError::NeedsManager(
            "Loadout doesn't unpack rar archives.".into(),
        )),
        Kind::Plain => Err(ArchiveError::NeedsManager(
            "This download isn't an archive Loadout knows how to place.".into(),
        )),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mods::testing::temp_config;
    use zip::write::{SimpleFileOptions, ZipWriter};

    /// A zip at `path` holding `(name, bytes)` entries.
    pub(crate) fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut z = ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in entries {
            z.start_file(*name, SimpleFileOptions::default()).unwrap();
            z.write_all(bytes).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn safe_names_stay_inside() {
        assert_eq!(safe_rel("a/b.esp"), Some(PathBuf::from("a/b.esp")));
        assert_eq!(safe_rel("a\\b.esp"), Some(PathBuf::from("a/b.esp")));
        assert_eq!(safe_rel("./a"), Some(PathBuf::from("a")));
        for bad in [
            "../x",
            "a/../../x",
            "/etc/passwd",
            "C:/Windows/x",
            "C:x",
            "",
            "a\0b",
        ] {
            assert_eq!(safe_rel(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_plain_zip_unpacks_and_bad_ones_are_refused() {
        let dir = temp_config("archive");
        let ok = dir.join("ok.zip");
        write_zip(&ok, &[("Mod/a.txt", b"one"), ("Mod/sub/b.dll", b"two")]);
        assert_eq!(sniff(&ok).unwrap(), Kind::Zip);
        let out = dir.join("out");
        let files = unpack_zip(&ok, &out, &LIMITS).unwrap();
        assert_eq!(
            files,
            [PathBuf::from("Mod/a.txt"), PathBuf::from("Mod/sub/b.dll")]
        );
        assert_eq!(std::fs::read(out.join("Mod/sub/b.dll")).unwrap(), b"two");

        let slip = dir.join("slip.zip");
        write_zip(&slip, &[("../evil.txt", b"x")]);
        assert!(matches!(
            unpack_zip(&slip, &dir.join("o2"), &LIMITS),
            Err(ArchiveError::Unsafe(_))
        ));
        assert!(!dir.join("evil.txt").exists());

        let link = dir.join("link.zip");
        let mut z = ZipWriter::new(File::create(&link).unwrap());
        z.add_symlink("Mod/link", "/etc", SimpleFileOptions::default())
            .unwrap();
        z.finish().unwrap();
        assert!(matches!(
            unpack_zip(&link, &dir.join("o3"), &LIMITS),
            Err(ArchiveError::Unsafe(_))
        ));

        let fomod = dir.join("fomod.zip");
        write_zip(
            &fomod,
            &[("fomod/ModuleConfig.xml", b"<x/>"), ("a.esp", b"e")],
        );
        let err = unpack_zip(&fomod, &dir.join("o4"), &LIMITS).unwrap_err();
        assert!(err.line().contains("mod manager"), "{}", err.line());

        let exe = dir.join("exe.zip");
        write_zip(&exe, &[("tool/setup.exe", b"MZ")]);
        assert!(matches!(
            unpack_zip(&exe, &dir.join("o5"), &LIMITS),
            Err(ArchiveError::NeedsManager(_))
        ));

        let small = Limits {
            files: 1,
            total: 4,
            file: 4,
        };
        assert!(matches!(
            unpack_zip(&ok, &dir.join("o6"), &small),
            Err(ArchiveError::TooBig(_))
        ));
        let one = Limits {
            files: 10,
            total: 4,
            file: 4,
        };
        assert!(matches!(
            unpack_zip(&ok, &dir.join("o7"), &one),
            Err(ArchiveError::TooBig(_))
        ));
    }

    #[test]
    fn hashes_match_or_refuse() {
        let dir = temp_config("hash");
        let f = dir.join("m.jar");
        std::fs::write(&f, b"abc").unwrap();
        let sha512 = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";
        let sha1 = "a9993e364706816aba3e25717850c26c9cd0d89d";
        assert_eq!(verify(&f, Some(&FileHash::Sha512(sha512.into()))), Ok(true));
        assert_eq!(
            verify(&f, Some(&FileHash::Sha1(sha1.to_uppercase()))),
            Ok(true)
        );
        assert!(verify(&f, Some(&FileHash::Sha1("00".into()))).is_err());
        assert_eq!(verify(&f, Some(&FileHash::Md5("x".into()))), Ok(false));
        assert_eq!(verify(&f, None), Ok(false));
    }

    #[test]
    fn stage_places_jars_and_hands_other_formats_on() {
        let dir = temp_config("stage");
        let jar = dir.join("dl.bin");
        write_zip(&jar, &[("META-INF/MANIFEST.MF", b"x")]);
        assert_eq!(
            stage(&jar, "sodium-0.6.jar", &dir.join("s1")).unwrap(),
            [PathBuf::from("sodium-0.6.jar")]
        );
        let seven = dir.join("a.7z");
        std::fs::write(&seven, [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0, 4]).unwrap();
        assert!(matches!(
            stage(&seven, "a.7z", &dir.join("s2")),
            Err(ArchiveError::NeedsManager(_))
        ));
        let rar = dir.join("a.rar");
        std::fs::write(&rar, b"Rar!\x1a\x07").unwrap();
        assert_eq!(sniff(&rar).unwrap(), Kind::Rar);
        assert!(stage(&rar, "a.rar", &dir.join("s3")).is_err());
        assert!(stage_plain(&jar, "../x.jar", &dir.join("s4")).is_err());
        assert!(stage_plain(&jar, "setup.exe", &dir.join("s5")).is_err());
    }
}
