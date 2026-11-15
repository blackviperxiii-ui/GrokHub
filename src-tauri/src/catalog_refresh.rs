//! Newer local models without an app update: once a day, save a newer signed
//! `local_catalog.toml` from this channel's newest release
//! (`grokhub_core::catalog_update`). Nothing unsigned is used: the catalog
//! and its `SHA256SUMS` are checked against the release key before anything
//! is written, and again every time the app starts. A saved catalog is used
//! from the next start.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use grokhub_agent::harness as hx;
use grokhub_core::catalog_update as cu;
use grokhub_core::release_verify::{Release, RELEASE_PUBKEY};
use grokhub_core::Channel;

/// How often the clock wakes; [`cu::due`] says whether a day has passed.
const TICK: Duration = Duration::from_secs(60 * 60);
/// GitHub's release download redirects once to its asset host.
const REDIRECTS_MAX: usize = 4;

static RUNNING: AtomicBool = AtomicBool::new(false);

/// The newest release on a channel (`shell_update::find_release` live).
pub(crate) type Find<'a> = &'a dyn Fn(&Path, Channel) -> Result<Release, String>;

/// Look once a day while the app runs, starting two minutes in.
pub fn start(config_dir: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(120));
        loop {
            tick(&config_dir, grokhub_core::now_ms());
            std::thread::sleep(TICK);
        }
    });
}

/// One look when a day has passed, one at a time, only where local models run.
pub fn tick(dir: &Path, now: u64) {
    if !crate::capabilities::current().local_ai || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let find = |dir: &Path, channel: Channel| crate::shell_update::find_release(dir, channel);
    let mut get = |url: &str, cap: u64| download(dir, url, cap);
    match run(dir, now, &find, &mut get) {
        Ok(Some(v)) => eprintln!("model catalog: saved version {v}, used from the next start"),
        Ok(None) => {}
        Err(why) => eprintln!("model catalog: not updated: {why}"),
    }
    RUNNING.store(false, Ordering::SeqCst);
}

/// [`tick`] with the network passed in. `Ok(Some(v))` when catalog `v` was
/// saved.
pub(crate) fn run(dir: &Path, now: u64, find: Find, get: cu::Get) -> Result<Option<u32>, String> {
    if !cu::due(dir, now) {
        return Ok(None);
    }
    // Stamped before any request, so a failed look also waits a day.
    cu::stamp(dir, now);
    // A build without the release key uses only its own catalog.
    let Some(pubkey) = RELEASE_PUBKEY else {
        return Ok(None);
    };
    let rel = find(dir, crate::shell_update::channel(dir))?;
    cu::refresh(pubkey, &rel, dir, get)
}

/// One release file: HTTPS to GitHub's release hosts only, behind the egress
/// guard on every hop, and no more than `cap` bytes.
fn download(dir: &Path, url: &str, cap: u64) -> Result<Vec<u8>, String> {
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(20),
        overall: Some(Duration::from_secs(60)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    });
    let check = |hop: &str| {
        grokhub_core::ssrf::runtime_download_url_ok(hop)?;
        hx::guard_quiet(dir, &hx::EgressReq::new(hop, &[]))
    };
    let resp = grokhub_core::http::get_following(
        &agent,
        url,
        &[("user-agent", grokhub_agent::USER_AGENT)],
        REDIRECTS_MAX,
        "while downloading the model catalog",
        &check,
    )?;
    if resp.status() != 200 {
        return Err(format!("GitHub answered HTTP {}", resp.status()));
    }
    let mut body = Vec::new();
    resp.into_reader()
        .take(cap + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() as u64 > cap {
        return Err("the file is larger than expected".into());
    }
    Ok(body)
}

#[cfg(test)]
#[path = "catalog_refresh_tests.rs"]
mod tests;
