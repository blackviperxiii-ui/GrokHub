//! Downloading an on-device model into the models folder: progress, pause,
//! cancel, resume (after a restart too), and a SHA-256 check against the hash
//! the catalog pins for the file (the publisher's listing must still agree). The network sits behind [`Fetch`], so the
//! cabin plugs in HTTP and tests plug in a fake server.
//!
//! On disk, under the models folder:
//! - `<file>.part` while it downloads (kept on pause or a failed connection, so it resumes);
//! - `<file>` once the hash matches;
//! - `installed.json`, the [`Installed`] record of the last model that finished.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::local_setup::LocalModel;

pub const INSTALLED_FILE: &str = "installed.json";

/// An open download: the reader, the offset the server actually started at
/// (0 when it ignored the range) and the full size if known.
pub type Opened = (Box<dyn Read + Send>, u64, Option<u64>);

/// Where the bytes come from.
pub trait Fetch {
    /// The file from `offset` on.
    fn open(&self, url: &str, offset: u64) -> Result<Opened, String>;
    /// The publisher's SHA-256 for `file` in `repo`, lowercase hex.
    fn sha256(&self, repo: &str, file: &str) -> Result<String, String>;
}

pub fn download_url(m: &LocalModel) -> String {
    format!("https://huggingface.co/{}/resolve/main/{}", m.repo, m.file)
}

/// The Hugging Face tree listing that carries each file's LFS SHA-256.
pub fn tree_url(repo: &str) -> String {
    format!("https://huggingface.co/api/models/{repo}/tree/main")
}

/// `file`'s `lfs.oid` in a Hugging Face tree listing.
pub fn sha256_from_tree(json: &str, file: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let oid = v
        .as_array()?
        .iter()
        .find(|e| e.get("path").and_then(|p| p.as_str()) == Some(file))?
        .get("lfs")?
        .get("oid")?
        .as_str()?;
    (oid.len() == 64 && oid.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| oid.to_ascii_lowercase())
}

pub fn part_path(dir: &Path, m: &LocalModel) -> PathBuf {
    dir.join(format!("{}.part", m.file))
}

pub fn final_path(dir: &Path, m: &LocalModel) -> PathBuf {
    dir.join(m.file)
}

/// Bytes already on disk from an unfinished download.
pub fn partial_bytes(dir: &Path, m: &LocalModel) -> Option<u64> {
    fs::metadata(part_path(dir, m))
        .ok()
        .map(|md| md.len())
        .filter(|n| *n > 0)
}

/// The model that finished and passed its check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    pub id: String,
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
}

/// The installed record, only while its file is still there at the recorded size.
pub fn installed(dir: &Path) -> Option<Installed> {
    let rec: Installed =
        serde_json::from_str(&fs::read_to_string(dir.join(INSTALLED_FILE)).ok()?).ok()?;
    let len = fs::metadata(dir.join(&rec.file)).ok()?.len();
    (len == rec.bytes).then_some(rec)
}

/// Catalog models whose finished file is in `dir`, in catalog order.
pub fn downloaded(dir: &Path) -> Vec<&'static LocalModel> {
    crate::local_setup::local_models()
        .iter()
        .filter(|m| !m.is_drafter && final_path(dir, m).is_file())
        .collect()
}

/// Switch to `m`, already downloaded into `dir`: it becomes the installed
/// model with no new download. The file passed its check when it finished,
/// so the record carries the catalog's pinned hash.
pub fn activate(dir: &Path, m: &LocalModel) -> Result<Installed, String> {
    let path = final_path(dir, m);
    let bytes = fs::metadata(&path)
        .map_err(|_| format!("{} isn't downloaded.", m.name))?
        .len();
    let rec = Installed {
        id: m.id.into(),
        file: m.file.into(),
        sha256: m.sha256.into(),
        bytes,
    };
    let json = serde_json::to_string_pretty(&rec).map_err(|e| e.to_string())?;
    fs::write(dir.join(INSTALLED_FILE), json)
        .map_err(|e| format!("Couldn't record {}: {e}", m.name))?;
    Ok(rec)
}

/// Delete `m`'s file (and any `.part`) from `dir`. The installed model is
/// refused: switch to another one first.
pub fn remove(dir: &Path, m: &LocalModel) -> Result<(), String> {
    if installed(dir).is_some_and(|rec| rec.file == m.file) {
        return Err(format!(
            "{} is the model GrokHub runs. Switch to another one first.",
            m.name
        ));
    }
    let _ = fs::remove_file(part_path(dir, m));
    match fs::remove_file(final_path(dir, m)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Couldn't remove {}: {e}", m.name)),
    }
}

/// Run (the default), pause or cancel, set from the UI while a download runs.
#[derive(Debug, Default)]
pub struct Control(AtomicU8);

const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

impl Control {
    pub fn pause(&self) {
        self.0.store(PAUSE, Ordering::SeqCst);
    }
    pub fn cancel(&self) {
        self.0.store(CANCEL, Ordering::SeqCst);
    }
    pub fn paused(&self) -> bool {
        self.state() == PAUSE
    }
    pub fn cancelled(&self) -> bool {
        self.state() == CANCEL
    }
    fn state(&self) -> u8 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// Bytes on disk so far, and the full size when known.
    Bytes {
        done: u64,
        total: Option<u64>,
    },
    Verifying,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done(Installed),
    /// Stopped by Pause; the `.part` stays so it resumes.
    Paused {
        done: u64,
    },
    /// Stopped by Cancel; the `.part` is gone.
    Cancelled,
    /// The hash didn't match; the `.part` is gone so the next try starts clean.
    BadChecksum,
    /// Network or disk trouble; the `.part` stays so it resumes.
    Failed(String),
}

const CHUNK: usize = 256 * 1024;

/// Download `m` into `dir`, resuming any `.part`, then check its SHA-256.
pub fn download(
    dir: &Path,
    m: &LocalModel,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Outcome {
    if let Err(e) = fs::create_dir_all(dir) {
        return Outcome::Failed(format!("Couldn't create {}: {e}", dir.display()));
    }
    // A listing that drifted from the pin means the file was swapped upstream:
    // stop before pulling gigabytes that would fail the check anyway.
    match fetch.sha256(m.repo, m.file) {
        Ok(h) if h == m.sha256 => {}
        Ok(_) => {
            return Outcome::Failed(format!(
                "Hugging Face now lists a different file for {} than the one GrokHub pinned, so it wasn't downloaded.",
                m.name
            ))
        }
        Err(e) => {
            return Outcome::Failed(format!("Couldn't read the checksum for {}: {e}", m.name))
        }
    }
    let want = m.sha256.to_string();
    let part = part_path(dir, m);
    let have = partial_bytes(dir, m).unwrap_or(0);
    let (mut body, start, total) = match fetch.open(&download_url(m), have) {
        Ok(x) => x,
        Err(e) => return Outcome::Failed(format!("Couldn't download {}: {e}", m.name)),
    };
    // The server ignored the range: start the file over.
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
        match ctl.state() {
            PAUSE => return Outcome::Paused { done },
            CANCEL => {
                drop(out);
                let _ = fs::remove_file(&part);
                return Outcome::Cancelled;
            }
            _ => {}
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Outcome::Failed(format!("The download of {} stopped: {e}", m.name)),
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
            "The download of {} ended early ({done} of {} bytes).",
            m.name,
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
    let fin = final_path(dir, m);
    if let Err(e) = fs::rename(&part, &fin) {
        return Outcome::Failed(format!("Couldn't finish {}: {e}", fin.display()));
    }
    let rec = Installed {
        id: m.id.into(),
        file: m.file.into(),
        sha256: want,
        bytes: done,
    };
    let json = serde_json::to_string_pretty(&rec).unwrap_or_default();
    if let Err(e) = fs::write(dir.join(INSTALLED_FILE), json) {
        return Outcome::Failed(format!("Couldn't record {}: {e}", m.name));
    }
    Outcome::Done(rec)
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Lowercase hex SHA-256 of a file on disk (streaming).
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// The full size in a `Content-Range: bytes 20-49/50` header.
pub fn content_range_total(header: &str) -> Option<u64> {
    header
        .trim()
        .strip_prefix("bytes ")?
        .rsplit_once('/')?
        .1
        .trim()
        .parse()
        .ok()
}

/// "2.1 GB of 4.6 GB" for the progress row.
pub fn progress_line(done: u64, total: Option<u64>) -> String {
    let mb = |b: u64| b / (1_024 * 1_024);
    match total {
        Some(t) if t > 0 => format!(
            "{} of {}",
            crate::local_setup::size_label(mb(done)),
            crate::local_setup::size_label(mb(t))
        ),
        _ => crate::local_setup::size_label(mb(done)),
    }
}

/// 0..=100.
pub fn percent(done: u64, total: Option<u64>) -> u8 {
    match total {
        Some(t) if t > 0 => (done.min(t) * 100 / t) as u8,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Mutex;

    const BODY: &[u8] = b"pretend this is a gguf file with a few bytes in it";

    fn model() -> LocalModel {
        LocalModel {
            id: "test-model",
            name: "Test Model",
            repo: "acme/test-GGUF",
            file: "test.gguf",
            sha256: sha(BODY).leak(),
            download_mb: 1,
            quant: "Q4_K_M",
            license: "Apache-2.0",
            badges: &[],
            tier: "T1",
            route: "local:tiny",
            ctx_default: 4_096,
            run_mb: 1,
            needs_avx2: false,
            min_llama_build: None,
            optional: false,
            is_drafter: false,
            drafter: None,
            min_budget: Default::default(),
            tok_s: Default::default(),
        }
    }

    fn sha(b: &[u8]) -> String {
        sha256_hex(b)
    }

    /// Serves `BODY`, honoring ranges unless told not to, cutting off after `cut` bytes once.
    struct FakeServer {
        body: Vec<u8>,
        hash: String,
        ranges: bool,
        cut: Mutex<Option<usize>>,
        opens: Mutex<Vec<u64>>,
    }

    impl FakeServer {
        fn new() -> Self {
            Self {
                body: BODY.to_vec(),
                hash: sha(BODY),
                ranges: true,
                cut: Mutex::new(None),
                opens: Mutex::new(Vec::new()),
            }
        }
    }

    struct Broken(Cursor<Vec<u8>>);
    impl Read for Broken {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.0.read(buf)? {
                0 => Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "connection reset",
                )),
                n => Ok(n),
            }
        }
    }

    impl Fetch for FakeServer {
        fn open(&self, url: &str, offset: u64) -> Result<Opened, String> {
            assert_eq!(
                url,
                "https://huggingface.co/acme/test-GGUF/resolve/main/test.gguf"
            );
            self.opens.lock().unwrap().push(offset);
            let start = if self.ranges { offset as usize } else { 0 };
            let total = Some(self.body.len() as u64);
            if let Some(n) = self.cut.lock().unwrap().take() {
                return Ok((
                    Box::new(Broken(Cursor::new(self.body[start..n].to_vec()))),
                    start as u64,
                    total,
                ));
            }
            Ok((
                Box::new(Cursor::new(self.body[start..].to_vec())),
                start as u64,
                total,
            ))
        }
        fn sha256(&self, repo: &str, file: &str) -> Result<String, String> {
            assert_eq!((repo, file), ("acme/test-GGUF", "test.gguf"));
            Ok(self.hash.clone())
        }
    }

    fn tmp(label: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("grokhub-model-dl-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_clean_download_verifies_renames_and_records_the_model() {
        let dir = tmp("clean");
        let srv = FakeServer::new();
        let mut seen = Vec::new();
        let out = download(&dir, &model(), &srv, &Control::default(), &mut |p| {
            seen.push(p)
        });
        let rec = Installed {
            id: "test-model".into(),
            file: "test.gguf".into(),
            sha256: sha(BODY),
            bytes: BODY.len() as u64,
        };
        assert_eq!(out, Outcome::Done(rec.clone()));
        assert_eq!(fs::read(dir.join("test.gguf")).unwrap(), BODY);
        assert!(!dir.join("test.gguf.part").exists());
        assert_eq!(installed(&dir), Some(rec));
        assert_eq!(
            seen.first(),
            Some(&Progress::Bytes {
                done: 0,
                total: Some(50)
            })
        );
        assert_eq!(
            &seen[seen.len() - 2..],
            &[
                Progress::Bytes {
                    done: 50,
                    total: Some(50)
                },
                Progress::Verifying
            ]
        );
        // The record only counts while the file is there at its size.
        fs::write(dir.join("test.gguf"), b"short").unwrap();
        assert_eq!(installed(&dir), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_dropped_connection_keeps_the_part_and_the_next_run_resumes_from_it() {
        let dir = tmp("resume");
        let srv = FakeServer::new();
        *srv.cut.lock().unwrap() = Some(20);
        let out = download(&dir, &model(), &srv, &Control::default(), &mut |_| {});
        assert_eq!(
            out,
            Outcome::Failed("The download of Test Model stopped: connection reset".into())
        );
        assert_eq!(partial_bytes(&dir, &model()), Some(20));
        // A restart: a fresh call picks up at byte 20.
        let out = download(&dir, &model(), &srv, &Control::default(), &mut |_| {});
        assert!(
            matches!(out, Outcome::Done(ref r) if r.bytes == 50),
            "{out:?}"
        );
        assert_eq!(*srv.opens.lock().unwrap(), vec![0, 20]);
        assert_eq!(fs::read(dir.join("test.gguf")).unwrap(), BODY);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_server_that_ignores_the_range_starts_the_file_over() {
        let dir = tmp("norange");
        fs::create_dir_all(&dir).unwrap();
        fs::write(part_path(&dir, &model()), b"stale bytes from last time").unwrap();
        let mut srv = FakeServer::new();
        srv.ranges = false;
        let out = download(&dir, &model(), &srv, &Control::default(), &mut |_| {});
        assert!(matches!(out, Outcome::Done(_)), "{out:?}");
        assert_eq!(*srv.opens.lock().unwrap(), vec![26]);
        assert_eq!(fs::read(dir.join("test.gguf")).unwrap(), BODY);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bad_checksum_deletes_the_part_and_installs_nothing() {
        let dir = tmp("badsum");
        let mut srv = FakeServer::new();
        srv.body = b"some other file".to_vec();
        let out = download(&dir, &model(), &srv, &Control::default(), &mut |_| {});
        assert_eq!(out, Outcome::BadChecksum);
        assert!(!dir.join("test.gguf").exists() && partial_bytes(&dir, &model()).is_none());
        assert_eq!(installed(&dir), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_listing_that_drifted_from_the_pin_downloads_nothing() {
        let dir = tmp("drift");
        let mut srv = FakeServer::new();
        srv.hash = sha(b"some other file");
        let out = download(&dir, &model(), &srv, &Control::default(), &mut |_| {});
        assert_eq!(
            out,
            Outcome::Failed(
                "Hugging Face now lists a different file for Test Model than the one GrokHub pinned, so it wasn't downloaded.".into()
            )
        );
        assert!(srv.opens.lock().unwrap().is_empty());
        assert!(partial_bytes(&dir, &model()).is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn pause_keeps_the_part_and_cancel_removes_it() {
        let dir = tmp("pause");
        let srv = FakeServer::new();
        let ctl = Control::default();
        ctl.pause();
        let out = download(&dir, &model(), &srv, &ctl, &mut |_| {});
        assert_eq!(out, Outcome::Paused { done: 0 });
        assert!(part_path(&dir, &model()).exists());
        let ctl = Control::default();
        ctl.cancel();
        assert_eq!(
            download(&dir, &model(), &srv, &ctl, &mut |_| {}),
            Outcome::Cancelled
        );
        assert!(!part_path(&dir, &model()).exists() && !dir.join("test.gguf").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_tree_listing_gives_the_files_sha256_and_urls_point_at_hugging_face() {
        let tree = r#"[{"type":"file","path":"README.md","size":10},
            {"type":"file","path":"gemma-4-E4B-it-Q4_K_M.gguf","size":5335291936,
             "lfs":{"oid":"ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789","size":5335291936}}]"#;
        assert_eq!(
            sha256_from_tree(tree, "gemma-4-E4B-it-Q4_K_M.gguf").as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert_eq!(sha256_from_tree(tree, "README.md"), None, "no lfs entry");
        assert_eq!(sha256_from_tree(tree, "missing.gguf"), None);
        assert_eq!(sha256_from_tree("not json", "x"), None);
        let m = crate::local_setup::model_by_id("gemma-4-e4b-it-q4_k_m").unwrap();
        assert_eq!(download_url(m), "https://huggingface.co/lmstudio-community/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_K_M.gguf");
        assert_eq!(
            tree_url(m.repo),
            "https://huggingface.co/api/models/lmstudio-community/gemma-4-E4B-it-GGUF/tree/main"
        );
        assert_eq!(
            progress_line(2_202_009_600, Some(4_907_335_680)),
            "2.1 GB of 4.6 GB"
        );
        assert_eq!(progress_line(1_048_576 * 300, None), "300 MB");
        assert_eq!(
            (
                percent(25, Some(50)),
                percent(60, Some(50)),
                percent(5, None)
            ),
            (50, 100, 0)
        );
        assert_eq!(content_range_total("bytes 20-49/50"), Some(50));
        assert_eq!(content_range_total("bytes 0-0/*"), None);
        assert_eq!(content_range_total("50"), None);
    }

    #[test]
    fn a_downloaded_model_can_be_switched_to_and_an_unused_one_removed() {
        let dir = tmp("switch");
        fs::create_dir_all(&dir).unwrap();
        let ids = |d: &Path| downloaded(d).iter().map(|m| m.id).collect::<Vec<_>>();
        let all: Vec<&LocalModel> = crate::local_setup::local_models()
            .iter()
            .filter(|m| !m.is_drafter)
            .collect();
        let (a, b) = (all[0], all[1]);
        assert_eq!(ids(&dir), Vec::<&str>::new());
        fs::write(final_path(&dir, a), b"aaaa").unwrap();
        fs::write(final_path(&dir, b), b"bbbbbb").unwrap();
        assert_eq!(ids(&dir), vec![a.id, b.id]);

        let rec = activate(&dir, b).unwrap();
        assert_eq!((rec.id.as_str(), rec.bytes), (b.id, 6));
        assert_eq!(installed(&dir).map(|r| r.id), Some(b.id.to_string()));
        assert_eq!(
            remove(&dir, b),
            Err(format!(
                "{} is the model GrokHub runs. Switch to another one first.",
                b.name
            ))
        );
        remove(&dir, a).unwrap();
        assert_eq!(ids(&dir), vec![b.id]);
        assert_eq!(
            activate(&dir, a),
            Err(format!("{} isn't downloaded.", a.name))
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
