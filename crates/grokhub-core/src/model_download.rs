//! Downloading an on-device model into the models folder: progress, pause,
//! cancel, resume (after a restart too), and a SHA-256 check against the hash
//! the catalog pins for the file (the publisher's listing must still agree). The network sits behind [`Fetch`], so the
//! cabin plugs in HTTP and tests plug in a fake server.
//!
//! On disk, under the models folder:
//! - `<file>.part` while it downloads (kept on pause or a failed connection, so it resumes);
//! - `<file>` once the hash matches;
//! - `installed.json`, the [`Installed`] record of the last chat model (or
//!   drafter, in its own folder) that finished.
//!
//! A voice or image model downloads several files (its `parts`) into a folder
//! of its own, each checked the same way; it is only on disk once all are.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::local_setup::{FileCheck, LocalModel, ModelKind};

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
    resolve_url(m.repo, m.remote_name())
}

/// Where `remote` in `repo` downloads from.
pub fn resolve_url(repo: &str, remote: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{remote}")
}

/// The Hugging Face tree listing of `file`'s own folder, which carries its
/// LFS SHA-256. A listing shows one folder only, so a file under
/// `split_files/vae/` is never in the top one. Each folder name is
/// percent-encoded on its own; the slashes between them stay.
pub fn tree_url(repo: &str, file: &str) -> String {
    let base = format!("https://huggingface.co/api/models/{repo}/tree/main");
    match file.rsplit_once('/') {
        Some((folder, _)) if !folder.is_empty() => {
            let path: Vec<String> = folder.split('/').map(encode_segment).collect();
            format!("{base}/{}", path.join("/"))
        }
        _ => base,
    }
}

/// RFC 3986 unreserved characters stay; every other byte is `%XX`.
fn encode_segment(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for b in seg.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The `rel="next"` page in a tree listing's `Link` header. The Hub pages a
/// large folder this way; only a page on the Hub itself is followed.
pub fn next_page(link: &str) -> Option<String> {
    link.split(',').find_map(|part| {
        let (target, params) = part.trim().split_once(';')?;
        let next = params
            .split(';')
            .any(|p| p.trim().eq_ignore_ascii_case("rel=\"next\"") || p.trim() == "rel=next");
        let url = target.trim().strip_prefix('<')?.strip_suffix('>')?;
        (next && url.starts_with("https://huggingface.co/")).then(|| url.to_string())
    })
}

/// One page of a tree listing: its body and its `Link` header.
pub type Page = (String, Option<String>);

/// The most listing pages one lookup reads. The Hub lists 1,000 entries a
/// page, so this covers any folder a model lives in.
const MAX_PAGES: usize = 50;

/// The publisher's SHA-256 for `file` in `repo`: reads `file`'s folder
/// listing through `get`, following its next pages, until the file shows.
/// Every [`Fetch`] uses this, so the shell and the cabin read the same pages.
pub fn sha256_lookup(
    repo: &str,
    file: &str,
    get: &mut dyn FnMut(&str) -> Result<Page, String>,
) -> Result<String, String> {
    let mut url = tree_url(repo, file);
    let mut seen = Vec::new();
    for _ in 0..MAX_PAGES {
        let (body, link) = get(&url)?;
        if let Some(h) = sha256_from_tree(&body, file) {
            return Ok(h);
        }
        seen.push(url);
        match link.as_deref().and_then(next_page) {
            Some(next) if !seen.contains(&next) => url = next,
            _ => break,
        }
    }
    Err(format!(
        "Hugging Face lists no checksum for {file} in {repo}."
    ))
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

/// The folder under the models folder a kind downloads into. Speech, image
/// and other specialty models keep out of the chat folder, so nothing that
/// looks for chat GGUFs there ever finds them. Drafters have their own
/// folder outside this one, so they map to the models folder itself here.
pub fn kind_dir(models_dir: &Path, kind: ModelKind) -> PathBuf {
    match kind {
        ModelKind::Chat | ModelKind::Drafter | ModelKind::VisionProjector => models_dir.into(),
        ModelKind::Image | ModelKind::ImageGuard => models_dir.join("image"),
        ModelKind::SpeechToText | ModelKind::TextToSpeech => models_dir.join("voice"),
        ModelKind::Embed | ModelKind::Rerank | ModelKind::Ocr | ModelKind::Guard => {
            models_dir.join("other")
        }
    }
}

/// Every file of `m` finished and checked in `dir`.
pub fn complete(dir: &Path, m: &LocalModel) -> bool {
    final_path(dir, m).is_file() && m.parts.iter().all(|p| dir.join(p.file).is_file())
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
        .filter(|m| m.is_chat() && final_path(dir, m).is_file())
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
            "{} is the model {product} runs. Switch to another one first.",
            m.name,
            product = crate::brand::product_name()
        ));
    }
    let files = std::iter::once(m.file).chain(m.parts.iter().map(|p| p.file));
    for file in files {
        let fin = dir.join(file);
        let _ = fs::remove_file(part_of(&fin));
        match fs::remove_file(&fin) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Couldn't remove {}: {e}", m.name)),
        }
        // A model's own folder goes once empty; the models folder never does.
        if let Some(parent) = fin.parent().filter(|p| *p != dir) {
            let _ = fs::remove_dir(parent);
        }
    }
    Ok(())
}

fn part_of(fin: &Path) -> PathBuf {
    let mut s = fin.as_os_str().to_owned();
    s.push(".part");
    s.into()
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
    /// Network or disk trouble at `step`; the `.part` stays so it resumes.
    Failed {
        step: Step,
        why: String,
    },
}

/// Where a model download stopped, for the line under its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The drive hasn't room for it.
    Space,
    /// Reading the publisher's checksum before the download.
    Checksum,
    /// Pulling or writing the bytes.
    Download,
    /// Checking the finished file.
    Verify,
    /// The speech or picture engine that runs it.
    Engine,
}

impl Step {
    pub fn label(self) -> &'static str {
        match self {
            Step::Space => "free space",
            Step::Checksum => "checksum read",
            Step::Download => "download",
            Step::Verify => "verify",
            Step::Engine => "engine",
        }
    }
}

/// Bytes of `m` already in `dir`: finished files and any `.part`.
pub fn bytes_on_disk(dir: &Path, m: &LocalModel) -> u64 {
    std::iter::once(m.file)
        .chain(m.parts.iter().map(|p| p.file))
        .map(|f| {
            let fin = dir.join(f);
            fs::metadata(&fin)
                .or_else(|_| fs::metadata(part_of(&fin)))
                .map(|md| md.len())
                .unwrap_or(0)
        })
        .sum()
}

/// Free disk `m` still needs in `dir`: what is left to download plus the
/// headroom. A resumed download only needs room for the rest.
pub fn space_needed_mb(dir: &Path, m: &LocalModel) -> u64 {
    crate::local_setup::disk_needed_mb(m).saturating_sub(bytes_on_disk(dir, m) / (1_024 * 1_024))
}

const CHUNK: usize = 256 * 1024;

/// One file of a model's download.
struct Item {
    repo: &'static str,
    remote: &'static str,
    file: &'static str,
    check: FileCheck,
}

/// Download `m` into `dir`, resuming any `.part`, then check each file: the
/// main file, then its parts. A part already finished (it was checked before
/// its rename) is kept.
pub fn download(
    dir: &Path,
    m: &LocalModel,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Outcome {
    let failed = |step, why| Outcome::Failed { step, why };
    if let Err(e) = crate::offline::refuse() {
        return failed(Step::Download, e);
    }
    if let Err(e) = fs::create_dir_all(dir) {
        return failed(
            Step::Download,
            format!("Couldn't create {}: {e}", dir.display()),
        );
    }
    let items: Vec<Item> = std::iter::once(Item {
        repo: m.repo,
        remote: m.remote_name(),
        file: m.file,
        check: FileCheck::Sha256(m.sha256),
    })
    .chain(m.parts.iter().map(|p| Item {
        repo: p.repo,
        remote: p.remote_file,
        file: p.file,
        check: p.check,
    }))
    .collect();
    // A listing that drifted from the pin means a file was swapped upstream:
    // stop before pulling gigabytes that would fail the check anyway. A small
    // git file has no SHA-256 listed; its pinned oid is checked after.
    for it in &items {
        let FileCheck::Sha256(pin) = it.check else {
            continue;
        };
        match fetch.sha256(it.repo, it.remote) {
            Ok(h) if h == pin => {}
            Ok(_) => {
                return failed(Step::Checksum, format!(
                    "Hugging Face now lists a different file for {} than the one {product} pinned, so it wasn't downloaded.",
                    m.name, product = crate::brand::product_name()
                ))
            }
            Err(e) => {
                return failed(
                    Step::Checksum,
                    format!("Couldn't read the checksum for {}: {e}", m.name),
                )
            }
        }
    }
    let mut after: u64 = m.parts.iter().map(|p| p.bytes).sum();
    let mut base = 0;
    let mut main_bytes = 0;
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            after -= m.parts[i - 1].bytes;
        }
        let fin = dir.join(it.file);
        // A multi-file model's main file is kept too: its folder holds
        // nothing else, and it was checked before its rename.
        if (i > 0 || !m.parts.is_empty())
            && let Ok(md) = fs::metadata(&fin)
        {
            if i == 0 {
                main_bytes = md.len();
            }
            base += md.len();
            continue;
        }
        let mut sub = |p: Progress| {
            on(match p {
                Progress::Bytes { done, total } => Progress::Bytes {
                    done: base + done,
                    total: total.map(|t| base + t + after),
                },
                v => v,
            })
        };
        match fetch_file(&fin, it, m.name, fetch, ctl, &mut sub) {
            Ok(n) => {
                if i == 0 {
                    main_bytes = n;
                }
                base += n;
            }
            Err(Outcome::Paused { done }) => return Outcome::Paused { done: base + done },
            Err(o) => return o,
        }
    }
    let rec = Installed {
        id: m.id.into(),
        file: m.file.into(),
        sha256: m.sha256.into(),
        bytes: main_bytes,
    };
    // Only a chat model (or a drafter, in its own folder) is the one the
    // engine runs; a voice or image model never takes that record.
    if matches!(m.kind, ModelKind::Chat | ModelKind::Drafter) {
        let json = serde_json::to_string_pretty(&rec).unwrap_or_default();
        if let Err(e) = fs::write(dir.join(INSTALLED_FILE), json) {
            return failed(Step::Download, format!("Couldn't record {}: {e}", m.name));
        }
    }
    Outcome::Done(rec)
}

/// One file: resume its `.part`, check it, rename it into place. Ok is its
/// size; Err is how it stopped (a pause carries this file's bytes).
fn fetch_file(
    fin: &Path,
    it: &Item,
    name: &str,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Result<u64, Outcome> {
    let failed = |why: String| {
        Err(Outcome::Failed {
            step: Step::Download,
            why,
        })
    };
    if let Some(parent) = fin.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return failed(format!("Couldn't create {}: {e}", parent.display()));
    }
    let part = part_of(fin);
    let have = fs::metadata(&part).map(|md| md.len()).unwrap_or(0);
    let (mut body, start, total) = match fetch.open(&resolve_url(it.repo, it.remote), have) {
        Ok(x) => x,
        Err(e) => return failed(format!("Couldn't download {name}: {e}")),
    };
    // The server ignored the range: start the file over.
    let mut out = match if start == 0 {
        File::create(&part)
    } else {
        OpenOptions::new().append(true).open(&part)
    } {
        Ok(f) => f,
        Err(e) => return failed(format!("Couldn't write {}: {e}", part.display())),
    };
    let mut done = start;
    on(Progress::Bytes { done, total });
    let mut buf = vec![0u8; CHUNK];
    loop {
        match ctl.state() {
            PAUSE => return Err(Outcome::Paused { done }),
            CANCEL => {
                drop(out);
                let _ = fs::remove_file(&part);
                return Err(Outcome::Cancelled);
            }
            _ => {}
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return failed(format!("The download of {name} stopped: {e}")),
        };
        if let Err(e) = out.write_all(&buf[..n]) {
            return failed(format!("Couldn't write {}: {e}", part.display()));
        }
        done += n as u64;
        on(Progress::Bytes { done, total });
    }
    if let Err(e) = out.flush() {
        return failed(format!("Couldn't write {}: {e}", part.display()));
    }
    drop(out);
    if total.is_some_and(|t| done < t) {
        return failed(format!(
            "The download of {name} ended early ({done} of {} bytes).",
            total.unwrap_or(0)
        ));
    }
    on(Progress::Verifying);
    let (got, want) = match it.check {
        FileCheck::Sha256(h) => (sha256_file(&part), h),
        FileCheck::GitOid(o) => (git_oid_file(&part), o),
    };
    match got {
        Ok(got) if got == want => {}
        Ok(_) => {
            let _ = fs::remove_file(&part);
            return Err(Outcome::BadChecksum);
        }
        Err(e) => {
            return Err(Outcome::Failed {
                step: Step::Verify,
                why: format!("Couldn't check {}: {e}", part.display()),
            });
        }
    }
    if let Err(e) = fs::rename(&part, fin) {
        return failed(format!("Couldn't finish {}: {e}", fin.display()));
    }
    Ok(done)
}

/// A file's git blob oid: lowercase hex SHA-1 of `blob <len>\0` and its bytes.
pub fn git_oid_file(path: &Path) -> std::io::Result<String> {
    use sha1::{Digest as _, Sha1};
    let len = fs::metadata(path)?.len();
    let mut f = File::open(path)?;
    let mut h = Sha1::new();
    h.update(format!("blob {len}\0").as_bytes());
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
            remote_file: None,
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
            kind: crate::local_setup::ModelKind::Chat,
            pairs_with: None,
            abliterated: None,
            refusal_score: None,
            drafter: None,
            min_budget: Default::default(),
            tok_s: Default::default(),
            parts: &[],
            skills: Default::default(),
            profile: Default::default(),
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
            Outcome::Failed {
                step: Step::Download,
                why: "The download of Test Model stopped: connection reset".into()
            }
        );
        assert_eq!(partial_bytes(&dir, &model()), Some(20));
        assert_eq!(bytes_on_disk(&dir, &model()), 20);
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
            Outcome::Failed {
                step: Step::Checksum,
                why: "Hugging Face now lists a different file for Test Model than the one Loadout pinned, so it wasn't downloaded.".into()
            }
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
        let m = crate::local_setup::model_by_id("gemma-4-e4b-it-abliterated-q4_k").unwrap();
        assert_eq!(download_url(m), "https://huggingface.co/huihui-ai/Huihui-gemma-4-E4B-it-qat-q4_0-unquantized-abliterated-GGUF/resolve/main/Huihui-gemma-4-E4B-it-qat-q4_0-unquantized-abliterated-Q4_K.gguf");
        assert_eq!(
            tree_url(m.repo, m.remote_name()),
            "https://huggingface.co/api/models/huihui-ai/Huihui-gemma-4-E4B-it-qat-q4_0-unquantized-abliterated-GGUF/tree/main"
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
    fn a_nested_file_reads_its_own_folder_and_follows_the_next_page() {
        // The picture models' VAE sits two folders down; the top listing never has it.
        let repo = "Comfy-Org/z_image_turbo";
        let file = "split_files/vae/ae.safetensors";
        let first =
            "https://huggingface.co/api/models/Comfy-Org/z_image_turbo/tree/main/split_files/vae";
        assert_eq!(tree_url(repo, file), first);
        // Each folder name is encoded on its own; the slashes between stay.
        assert_eq!(
            tree_url("acme/x", "my dir/a#b/f.gguf"),
            "https://huggingface.co/api/models/acme/x/tree/main/my%20dir/a%23b"
        );
        let second = format!("{first}?cursor=abc");
        let hash = "afc8e28272cd15db3919bacdb6918ce9c1ed22e96cb12c4d5ed0fba823529e38";
        let mut asked = Vec::new();
        let got = sha256_lookup(repo, file, &mut |url| {
            asked.push(url.to_string());
            if url == first {
                Ok((
                    r#"[{"type":"file","path":"split_files/vae/other.safetensors","lfs":{"oid":"0000000000000000000000000000000000000000000000000000000000000000"}}]"#.into(),
                    Some(format!("<{second}>; rel=\"next\"")),
                ))
            } else {
                Ok((
                    format!(r#"[{{"type":"file","path":"{file}","lfs":{{"oid":"{hash}"}}}}]"#),
                    None,
                ))
            }
        });
        assert_eq!(got.as_deref(), Ok(hash));
        assert_eq!(asked, [first.to_string(), second.clone()]);
        // Only a next page on the Hub is followed; a previous page isn't next.
        assert_eq!(
            next_page(&format!("<{second}>; rel=\"next\"")),
            Some(second.clone())
        );
        assert_eq!(
            next_page(&format!(
                "<{first}>; rel=\"prev\", <{second}>; rel=\"next\""
            )),
            Some(second)
        );
        assert_eq!(next_page(&format!("<{first}>; rel=\"prev\"")), None);
        assert_eq!(next_page("<https://example.com/x>; rel=\"next\""), None);
        // Not listed on any page: the error names the file and the repo.
        let none = sha256_lookup(repo, file, &mut |_| Ok(("[]".into(), None)));
        assert_eq!(
            none,
            Err("Hugging Face lists no checksum for split_files/vae/ae.safetensors in Comfy-Org/z_image_turbo.".into())
        );
        // A Link that points back at a page already read ends the walk.
        let mut n = 0;
        let looped = sha256_lookup(repo, file, &mut |_| {
            n += 1;
            Ok(("[]".into(), Some(format!("<{first}>; rel=\"next\""))))
        });
        assert!(looped.is_err());
        assert_eq!(n, 1);
    }

    #[test]
    fn a_resumed_download_only_needs_room_for_what_is_left() {
        let dir = tmp("space");
        let m = LocalModel {
            download_mb: 10,
            ..model()
        };
        assert_eq!(space_needed_mb(&dir, &m), 10 + 2_048);
        fs::create_dir_all(&dir).unwrap();
        File::create(part_path(&dir, &m))
            .unwrap()
            .set_len(3 * 1_024 * 1_024)
            .unwrap();
        assert_eq!(bytes_on_disk(&dir, &m), 3 * 1_024 * 1_024);
        assert_eq!(space_needed_mb(&dir, &m), 7 + 2_048);
        assert_eq!(
            (
                Step::Space.label(),
                Step::Checksum.label(),
                Step::Engine.label()
            ),
            ("free space", "checksum read", "engine")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_at_the_top_of_the_repo_still_reads_the_top_listing() {
        let tree = r#"[{"type":"file","path":"test.gguf","lfs":{"oid":"ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789"}}]"#;
        let mut asked = Vec::new();
        let got = sha256_lookup("acme/test-GGUF", "test.gguf", &mut |url| {
            asked.push(url.to_string());
            Ok((tree.into(), None))
        });
        assert_eq!(
            got.as_deref(),
            Ok("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert_eq!(
            asked,
            ["https://huggingface.co/api/models/acme/test-GGUF/tree/main"]
        );
        // A failed read says why and stops there.
        assert_eq!(
            sha256_lookup("acme/test-GGUF", "test.gguf", &mut |_| Err(
                "timed out".into()
            )),
            Err("timed out".into())
        );
    }

    #[test]
    fn a_downloaded_model_can_be_switched_to_and_an_unused_one_removed() {
        let dir = tmp("switch");
        fs::create_dir_all(&dir).unwrap();
        let ids = |d: &Path| downloaded(d).iter().map(|m| m.id).collect::<Vec<_>>();
        let all: Vec<&LocalModel> = crate::local_setup::local_models()
            .iter()
            .filter(|m| m.is_chat())
            .collect();
        let (a, b) = (all[0], all[1]);
        assert_eq!(ids(&dir), Vec::<&str>::new());
        // A stock Gemma 4 file from an older catalog is not a catalog download.
        fs::write(dir.join("gemma-4-31B-it-Q4_K_M.gguf"), b"stock").unwrap();
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
                "{} is the model Loadout runs. Switch to another one first.",
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

    /// Serves several files by name; the token list has no LFS hash listed.
    struct Files(Vec<(&'static str, &'static [u8])>, Mutex<Vec<String>>);

    impl Fetch for Files {
        fn open(&self, url: &str, offset: u64) -> Result<Opened, String> {
            self.1.lock().unwrap().push(url.into());
            let (_, b) = self.0.iter().find(|(n, _)| url.ends_with(n)).unwrap();
            let start = offset as usize;
            Ok((
                Box::new(Cursor::new(b[start..].to_vec())),
                offset,
                Some(b.len() as u64),
            ))
        }
        fn sha256(&self, _repo: &str, file: &str) -> Result<String, String> {
            let (_, b) = self.0.iter().find(|(n, _)| *n == file).unwrap();
            Ok(sha(b))
        }
    }

    fn git_oid(b: &[u8]) -> String {
        use sha1::{Digest as _, Sha1};
        let mut h = Sha1::new();
        h.update(format!("blob {}\0", b.len()).as_bytes());
        h.update(b);
        hex::encode(h.finalize())
    }

    const ENC: &[u8] = b"encoder weights";
    const TOK: &[u8] = b"<blk> 0\nhello 1\n";

    fn speech(tok_check: FileCheck) -> LocalModel {
        let parts = vec![crate::local_setup::ModelPart {
            repo: "acme/stt",
            remote_file: "tokens.txt",
            file: "stt/tokens.txt",
            check: tok_check,
            bytes: TOK.len() as u64,
        }];
        LocalModel {
            id: "test-stt",
            name: "Test Speech",
            repo: "acme/stt",
            file: "stt/encoder.onnx",
            remote_file: Some("encoder.onnx"),
            sha256: sha(ENC).leak(),
            kind: ModelKind::SpeechToText,
            parts: parts.leak(),
            ..model()
        }
    }

    fn files() -> Files {
        Files(
            vec![("encoder.onnx", ENC), ("tokens.txt", TOK)],
            Mutex::new(Vec::new()),
        )
    }

    #[test]
    fn a_speech_model_downloads_every_part_into_its_own_folder_and_never_takes_the_chat_record() {
        let models = tmp("parts");
        let dir = kind_dir(&models, ModelKind::SpeechToText);
        assert_eq!(dir, models.join("voice"));
        assert_eq!(kind_dir(&models, ModelKind::Chat), models);
        assert_eq!(
            kind_dir(&models, ModelKind::ImageGuard),
            models.join("image")
        );
        let m = speech(FileCheck::GitOid(git_oid(TOK).leak()));
        let mut seen = Vec::new();
        let out = download(&dir, &m, &files(), &Control::default(), &mut |p| {
            seen.push(p)
        });
        assert!(
            matches!(out, Outcome::Done(ref r) if r.bytes == ENC.len() as u64),
            "{out:?}"
        );
        assert!(complete(&dir, &m));
        assert_eq!(fs::read(dir.join("stt/tokens.txt")).unwrap(), TOK);
        assert!(!dir.join(INSTALLED_FILE).exists(), "the chat model stays");
        // Progress runs across both files to their sum.
        let all = (ENC.len() + TOK.len()) as u64;
        assert_eq!(
            seen.iter()
                .rev()
                .find(|p| matches!(p, Progress::Bytes { .. })),
            Some(&Progress::Bytes {
                done: all,
                total: Some(all)
            })
        );
        assert!(downloaded(&dir).is_empty(), "never listed as a chat model");
        // A finished part is kept on the next run; removing clears the folder.
        let f = files();
        let _ = fs::remove_file(dir.join("stt/encoder.onnx"));
        download(&dir, &m, &f, &Control::default(), &mut |_| {});
        assert_eq!(f.1.lock().unwrap().len(), 1, "only the missing file");
        let again = files();
        let out = download(&dir, &m, &again, &Control::default(), &mut |_| {});
        assert!(matches!(out, Outcome::Done(ref r) if r.bytes == ENC.len() as u64));
        assert!(
            again.1.lock().unwrap().is_empty(),
            "all kept, nothing fetched"
        );
        remove(&dir, &m).unwrap();
        assert!(!dir.join("stt").exists());
        let _ = fs::remove_dir_all(&models);
    }

    #[test]
    fn a_part_that_fails_its_pinned_git_oid_is_deleted_and_the_model_is_not_on_disk() {
        let dir = tmp("bad-part");
        let m = speech(FileCheck::GitOid("0000"));
        let out = download(&dir, &m, &files(), &Control::default(), &mut |_| {});
        assert_eq!(out, Outcome::BadChecksum);
        assert!(!complete(&dir, &m));
        assert!(!dir.join("stt/tokens.txt.part").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
