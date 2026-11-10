//! Live check of every catalog download against the real Hugging Face Hub,
//! for the owner's own machine. It never runs in CI (it is `#[ignore]`d and
//! needs the network):
//!
//! ```sh
//! cargo test -p grokhub-core --test model_download_live -- --ignored --nocapture
//! ```
//!
//! For every catalog model and each of its parts it reads the checksum
//! through the real lookup ([`md::sha256_lookup`], the folder listing with
//! its next pages) and pulls the first 1 MB through the same request the
//! desktop app's `HfFetch` makes: the app's HTTP client, public addresses
//! only, redirects followed by hand and checked on every hop. It prints one
//! line per file, ok or the step that failed and why, then the free space in
//! the models folder beside what each model needs.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_core::local_setup::{self as ls, FileCheck};
use grokhub_core::model_download::{self as md, Fetch};

const MIB: u64 = 1_024 * 1_024;

/// The Hub and the CDN it redirects downloads to.
fn hub_only(url: &str) -> Result<(), String> {
    let host = url
        .strip_prefix("https://")
        .and_then(|r| r.split(['/', '?', '#']).next())
        .ok_or_else(|| format!("not an https address: {url}"))?
        .to_ascii_lowercase();
    let ok = ["huggingface.co", "hf.co"]
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")));
    ok.then_some(())
        .ok_or_else(|| format!("{host} is not on the Hub"))
}

fn get(url: &str, range: Option<&str>) -> Result<grokhub_core::http::Response, String> {
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(15),
        read: Some(Duration::from_secs(60)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    });
    let mut headers = vec![("user-agent", "model-download-live-check")];
    if let Some(r) = range {
        headers.push(("range", r));
    }
    grokhub_core::http::get_following(
        &agent,
        url,
        &headers,
        10,
        "while checking a model",
        &hub_only,
    )
}

struct LiveFetch;

impl Fetch for LiveFetch {
    fn open(&self, url: &str, offset: u64) -> Result<md::Opened, String> {
        let range = (offset > 0).then(|| format!("bytes={offset}-"));
        let resp = get(url, range.as_deref())?;
        let total = resp
            .header("content-length")
            .and_then(|v| v.trim().parse().ok());
        Ok((Box::new(resp.into_reader()), 0, total))
    }

    fn sha256(&self, repo: &str, file: &str) -> Result<String, String> {
        md::sha256_lookup(repo, file, &mut |url| {
            let resp = get(url, None)?;
            let link = resp.header("link").map(str::to_owned);
            let mut body = String::new();
            resp.into_reader()
                .take(4 * MIB)
                .read_to_string(&mut body)
                .map_err(|e| e.to_string())?;
            Ok((body, link))
        })
    }
}

/// One file's check: `Ok` with what was seen, or the failing step and why.
fn check(repo: &str, remote: &str, pin: FileCheck) -> Result<String, String> {
    let listed = match pin {
        FileCheck::Sha256(want) => match LiveFetch.sha256(repo, remote) {
            Ok(got) if got == want => "checksum matches".to_string(),
            Ok(got) => {
                return Err(format!(
                    "checksum read: the Hub lists {got}, the catalog pins {want}"
                ))
            }
            Err(e) => return Err(format!("checksum read: {e}")),
        },
        FileCheck::GitOid(_) => "small git file, its oid is checked after download".into(),
    };
    let (body, _, total) = LiveFetch
        .open(&md::resolve_url(repo, remote), 0)
        .map_err(|e| format!("download: {e}"))?;
    let mut first = Vec::new();
    body.take(MIB)
        .read_to_end(&mut first)
        .map_err(|e| format!("download: {e}"))?;
    let size = total.map_or("size unknown".into(), |t| ls::size_label(t / MIB));
    Ok(format!(
        "{listed}, read {} of {size}",
        ls::size_label(first.len() as u64 / MIB)
    ))
}

fn models_dir() -> PathBuf {
    grokhub_core::paths::user_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(grokhub_core::brand::INTERNAL_ID)
        .join("models")
}

#[cfg(unix)]
fn free_mb(dir: &Path) -> Option<u64> {
    let out = std::process::Command::new("df")
        .args(["-Pk", &dir.to_string_lossy()])
        .output()
        .ok()?;
    ls::parse_df_avail_mb(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(windows)]
fn free_mb(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free: u64 = 0;
    // SAFETY: a NUL-terminated path and one out pointer; the other two may be null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free / MIB)
}

#[cfg(not(any(unix, windows)))]
fn free_mb(_dir: &Path) -> Option<u64> {
    None
}

#[test]
#[ignore = "reaches the real Hugging Face Hub; run it by hand on your own machine"]
fn model_download_live() {
    let mut failed = Vec::new();
    for m in ls::local_models() {
        let files = std::iter::once((m.repo, m.remote_name(), FileCheck::Sha256(m.sha256)))
            .chain(m.parts.iter().map(|p| (p.repo, p.remote_file, p.check)));
        for (repo, remote, pin) in files {
            match check(repo, remote, pin) {
                Ok(seen) => println!("ok      {} {repo}/{remote}: {seen}", m.id),
                Err(why) => {
                    println!("FAILED  {} {repo}/{remote}: {why}", m.id);
                    failed.push(format!("{} {remote}", m.id));
                }
            }
        }
    }
    let dir = models_dir();
    let at = dir.ancestors().find(|d| d.is_dir()).unwrap_or(&dir);
    match free_mb(at) {
        Some(free) => {
            println!("free space in {}: {}", dir.display(), ls::size_label(free));
            for m in ls::local_models() {
                let need = md::space_needed_mb(&md::kind_dir(&dir, m.kind), m);
                let mark = if free < need { "too big" } else { "fits" };
                println!("  {mark:<7} {}: needs {}", m.id, ls::size_label(need));
            }
        }
        None => println!("free space in {}: couldn't read it", dir.display()),
    }
    assert!(
        failed.is_empty(),
        "{} file(s) failed: {failed:?}",
        failed.len()
    );
}
