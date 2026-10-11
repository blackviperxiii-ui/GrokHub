//! The update plan's release step: download this host's asset from the
//! channel's latest GitHub release, check it against the signed
//! `SHA256SUMS` (`grokhub_core::release_verify`), unpack it, and swap the
//! binaries in. The old binaries stay as `*.old` so a failed swap rolls
//! back. Anything that doesn't verify is not installed.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_core::release_verify::{
    download_url_ok, host_asset, parse_releases, pick_release, refusal_note, verify_asset, Refusal, releases_api,
    RELEASE_PUBKEY, SIG_NAME, SUMS_NAME,
};
use grokhub_core::Channel;

/// Release list and `SHA256SUMS`/`.minisig` are small.
const TEXT_CAP: u64 = 4 * 1024 * 1024;
/// The Linux tarball and Windows zip are tens of MB.
const ASSET_CAP: u64 = 512 * 1024 * 1024;

fn fetch(url: &str, cap: u64, timeout: Duration) -> Result<Vec<u8>, String> {
    // EgressGuard: a public read, no user data.
    crate::xai::egress_ok(url, &[])?;
    let resp = ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout_connect(Duration::from_secs(25))
        .timeout(timeout)
        .build()
        .get(url)
        .set("user-agent", "GrokHub")
        .set("accept", "application/vnd.github+json, application/octet-stream")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("HTTP {code}"),
            other => other.to_string(),
        })?;
    let mut buf = Vec::new();
    resp.into_reader().take(cap + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > cap {
        return Err("response too large".into());
    }
    Ok(buf)
}

/// Where the swapped-in binaries go: the Setup folder on Windows, the
/// running binary's folder (`~/.local/bin`) elsewhere.
fn install_dir(windows: bool) -> Result<PathBuf, String> {
    if windows {
        let local = std::env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA is not set".to_string())?;
        return Ok(PathBuf::from(local).join("Programs").join("GrokHub"));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    exe.parent().map(Path::to_path_buf).ok_or_else(|| "can't find the install folder".into())
}

fn binary_names(windows: bool) -> [&'static str; 2] {
    if windows {
        ["grokhub.exe", "grokhub-hub.exe"]
    } else {
        ["grokhub", "grokhub-hub"]
    }
}

/// The first file called `name` under `root`, never outside it.
fn find_under(root: &Path, name: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == name) && path.starts_with(&root) {
                return Some(path);
            }
        }
    }
    None
}

/// Unpack with the system `tar` (bsdtar on Windows 10+ reads zip too).
fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(into);
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let out = std::process::Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .output()
        .map_err(|e| format!("tar: {e}"))?;
    if !out.status.success() {
        return Err(format!("tar: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

/// Put each `(source, name)` into `dest`. The current file is renamed to
/// `name.old` first (Windows allows renaming a running exe, not overwriting
/// it); if any step fails, every file already swapped is put back.
pub(crate) fn swap_in(dest: &Path, files: &[(PathBuf, &str)]) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut done: Vec<(&str, bool)> = Vec::new();
    for (src, name) in files {
        let target = dest.join(name);
        let old = dest.join(format!("{name}.old"));
        let fresh = dest.join(format!("{name}.new"));
        let step = (|| -> Result<bool, String> {
            std::fs::copy(src, &fresh).map_err(|e| format!("{name}: {e}"))?;
            let had = target.exists();
            if had {
                let _ = std::fs::remove_file(&old);
                std::fs::rename(&target, &old).map_err(|e| format!("{name}: {e}"))?;
            }
            if let Err(e) = std::fs::rename(&fresh, &target) {
                if had {
                    let _ = std::fs::rename(&old, &target);
                }
                return Err(format!("{name}: {e}"));
            }
            Ok(had)
        })();
        match step {
            Ok(had) => done.push((name, had)),
            Err(e) => {
                let _ = std::fs::remove_file(&fresh);
                for (n, had) in done.iter().rev() {
                    let target = dest.join(n);
                    let _ = std::fs::remove_file(&target);
                    if *had {
                        let _ = std::fs::rename(dest.join(format!("{n}.old")), &target);
                    }
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

/// Download, verify, unpack and swap in the channel's latest release.
/// `Err` is the one named line the update card shows.
pub(crate) fn install_release(channel: Channel, windows: bool) -> Result<String, String> {
    let dest = install_dir(windows).map_err(|e| refusal_note("", &Refusal::Download(e)))?;
    let work = std::env::temp_dir().join("grokhub-release-update");
    let mut get = |url: &str, cap: u64, secs: u64| fetch(url, cap, Duration::from_secs(secs));
    install_release_with(channel, windows, &mut get, &dest, &work)
}

/// `(url, byte cap, timeout seconds)` → body.
type Fetch<'a> = &'a mut dyn FnMut(&str, u64, u64) -> Result<Vec<u8>, String>;

fn fetch_text(get: Fetch, url: &str) -> Result<String, Refusal> {
    let bytes = get(url, TEXT_CAP, 30).map_err(Refusal::Download)?;
    String::from_utf8(bytes).map_err(|_| Refusal::Download("not text".into()))
}

/// [`install_release`] with the network, install folder and scratch folder
/// passed in. Nothing is written to `dest` until the asset verifies.
fn install_release_with(channel: Channel, windows: bool, get: Fetch, dest: &Path, work: &Path) -> Result<String, String> {
    let mut version = String::new();
    let run = (|| -> Result<String, Refusal> {
        let releases = parse_releases(&fetch_text(&mut *get, releases_api(channel))?);
        let rel = pick_release(&releases, channel).ok_or(Refusal::NoRelease)?;
        version = rel.version();
        let asset = host_asset(rel, windows).ok_or(Refusal::NoAsset)?;
        let sums = rel.asset(SUMS_NAME).ok_or(Refusal::NoSums)?;
        let sig = rel.asset(SIG_NAME);
        for a in [Some(asset), Some(sums), sig].into_iter().flatten() {
            if !download_url_ok(&a.url, &a.name) {
                return Err(Refusal::BadUrl);
            }
        }
        let sums_text = fetch_text(&mut *get, &sums.url)?;
        let sig_text = match sig {
            Some(s) => Some(fetch_text(&mut *get, &s.url)?),
            None => None,
        };
        let bytes = get(&asset.url, ASSET_CAP, 600).map_err(Refusal::Download)?;
        verify_asset(RELEASE_PUBKEY, &sums_text, sig_text.as_deref(), &asset.name, &bytes)?;
        let _ = std::fs::remove_dir_all(work);
        std::fs::create_dir_all(work).map_err(|e| Refusal::Download(e.to_string()))?;
        let archive = work.join(&asset.name);
        std::fs::write(&archive, &bytes).map_err(|e| Refusal::Download(e.to_string()))?;
        let stage = work.join("stage");
        unpack(&archive, &stage).map_err(Refusal::Download)?;
        let mut files = Vec::new();
        for name in binary_names(windows) {
            let found = find_under(&stage, name).ok_or_else(|| Refusal::Download(format!("{name} missing from {}", asset.name)))?;
            files.push((found, name));
        }
        swap_in(dest, &files).map_err(Refusal::Download)?;
        let _ = std::fs::remove_dir_all(work);
        Ok(format!("Installed GrokHub {} into {} (checked against SHA256SUMS).", rel.version(), dest.display()))
    })();
    run.map_err(|why| refusal_note(&version, &why))
}

/// The plan runner's output for the release step, shaped like a host
/// command's (`exit 0` / `exit 1`) so success and failure read the same way.
pub(crate) fn release_step_receipt_for(step: &str, result: Result<String, String>) -> String {
    match result {
        Ok(line) => format!("$ {step}\nexit 0\n{line}"),
        Err(line) => format!("$ {step}\nexit 1\n{line}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gh-swap-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn swap_in_replaces_both_binaries_and_keeps_the_old_ones() {
        let d = tmp("ok");
        let (src, dest) = (d.join("src"), d.join("bin"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(src.join("a"), "new a").unwrap();
        std::fs::write(src.join("b"), "new b").unwrap();
        std::fs::write(dest.join("grokhub"), "old a").unwrap();
        swap_in(&dest, &[(src.join("a"), "grokhub"), (src.join("b"), "grokhub-hub")]).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("grokhub")).unwrap(), "new a");
        assert_eq!(std::fs::read_to_string(dest.join("grokhub.old")).unwrap(), "old a");
        assert_eq!(std::fs::read_to_string(dest.join("grokhub-hub")).unwrap(), "new b");
        assert!(!dest.join("grokhub-hub.old").exists());
        assert!(!dest.join("grokhub.new").exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_failed_second_swap_puts_the_first_back() {
        let d = tmp("rollback");
        let (src, dest) = (d.join("src"), d.join("bin"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(src.join("a"), "new a").unwrap();
        std::fs::write(dest.join("grokhub"), "old a").unwrap();
        std::fs::write(dest.join("grokhub-hub"), "old b").unwrap();
        let err = swap_in(&dest, &[(src.join("a"), "grokhub"), (src.join("missing"), "grokhub-hub")]).unwrap_err();
        assert!(err.starts_with("grokhub-hub: "), "{err}");
        assert_eq!(std::fs::read_to_string(dest.join("grokhub")).unwrap(), "old a");
        assert_eq!(std::fs::read_to_string(dest.join("grokhub-hub")).unwrap(), "old b");
        assert!(!dest.join("grokhub.old").exists());
        assert!(!dest.join("grokhub-hub.new").exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn binaries_are_found_only_inside_the_stage() {
        let d = tmp("find");
        std::fs::create_dir_all(d.join("stage/grokhub-linux")).unwrap();
        std::fs::write(d.join("stage/grokhub-linux/grokhub"), "x").unwrap();
        std::fs::write(d.join("grokhub-hub"), "outside").unwrap();
        let stage = d.join("stage");
        assert_eq!(find_under(&stage, "grokhub"), Some(stage.canonicalize().unwrap().join("grokhub-linux/grokhub")));
        assert_eq!(find_under(&stage, "grokhub-hub"), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_release_step_reads_as_a_host_receipt() {
        let ok = release_step_receipt_for(grokhub_core::release_verify::RELEASE_INSTALL_STEP, Ok("Installed GrokHub 2.13.1 into /x (checked against SHA256SUMS).".into()));
        assert_eq!(ok, "$ grokhub: install the verified release\nexit 0\nInstalled GrokHub 2.13.1 into /x (checked against SHA256SUMS).");
        assert!(!crate::update::host_receipt_failed(&ok));
        let bad = release_step_receipt_for("grokhub: switch to the beta release", Err("Update 2.13.1 not installed: signature didn't verify".into()));
        assert_eq!(bad, "$ grokhub: switch to the beta release\nexit 1\nUpdate 2.13.1 not installed: signature didn't verify");
        assert!(crate::update::host_receipt_failed(&bad), "{bad}");
    }

    /// A release tarball shaped like `make-release-bundle.sh`'s.
    fn tarball(root: &Path, marker: &str) -> Vec<u8> {
        let src = root.join("pack");
        std::fs::create_dir_all(src.join("grokhub-linux")).unwrap();
        std::fs::write(src.join("grokhub-linux/grokhub"), format!("grokhub {marker}")).unwrap();
        std::fs::write(src.join("grokhub-linux/grokhub-hub"), format!("hub {marker}")).unwrap();
        let out = root.join("bundle.tar.gz");
        let st = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&out)
            .arg("-C")
            .arg(&src)
            .arg("grokhub-linux")
            .status()
            .unwrap();
        assert!(st.success());
        std::fs::read(out).unwrap()
    }

    const DL: &str = "https://github.com/blackviperxiii-ui/GrokHub/releases/download";

    fn release_json(tag: &str, pre: bool, created: &str, sums: bool) -> String {
        let mut assets = format!(r#"{{"name":"grokhub-linux-v2.13.1.tar.gz","browser_download_url":"{DL}/{tag}/grokhub-linux-v2.13.1.tar.gz"}}"#);
        if sums {
            assets.push_str(&format!(r#",{{"name":"SHA256SUMS","browser_download_url":"{DL}/{tag}/SHA256SUMS"}}"#));
        }
        format!(r#"{{"tag_name":"{tag}","prerelease":{pre},"draft":false,"created_at":"{created}","assets":[{assets}]}}"#)
    }

    /// Serves a fixed set of URLs, like GitHub would.
    fn server(pages: Vec<(String, Vec<u8>)>) -> impl FnMut(&str, u64, u64) -> Result<Vec<u8>, String> {
        move |url: &str, _cap: u64, _secs: u64| {
            pages.iter().find(|(u, _)| u == url).map(|(_, b)| b.clone()).ok_or_else(|| format!("HTTP 404 {url}"))
        }
    }

    #[test]
    fn beta_installs_the_newest_pre_release_and_a_tampered_one_is_refused_untouched() {
        let d = tmp("install-beta");
        let good = tarball(&d.join("new"), "29e8ac5");
        let old = tarball(&d.join("old"), "d2efb8d");
        let sums = format!("{}  grokhub-linux-v2.13.1.tar.gz\n", grokhub_core::model_download::sha256_hex(&good));
        let list = format!(
            "[{},{},{}]",
            release_json("beta-v2.13.1-d2efb8d", true, "2026-10-11T18:20:03Z", true),
            release_json("beta-v2.13.1-29e8ac5", true, "2026-10-11T18:33:17Z", true),
            release_json("v2.13.1", false, "2026-10-10T20:44:35Z", false)
        );
        let api = grokhub_core::release_verify::GITHUB_RELEASES_API.to_string();
        let new_url = format!("{DL}/beta-v2.13.1-29e8ac5/grokhub-linux-v2.13.1.tar.gz");
        let sums_url = format!("{DL}/beta-v2.13.1-29e8ac5/SHA256SUMS");
        let dest = d.join("bin");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("grokhub"), "running build").unwrap();
        std::fs::write(dest.join("grokhub-hub"), "running hub").unwrap();

        // Tampered: the asset isn't the one SHA256SUMS lists. Nothing is touched.
        let mut bad = server(vec![(api.clone(), list.clone().into_bytes()), (sums_url.clone(), sums.clone().into_bytes()), (new_url.clone(), old)]);
        assert_eq!(
            install_release_with(Channel::Beta, false, &mut bad, &dest, &d.join("work")),
            Err("Update 2.13.1-beta (29e8ac5) not installed: the download's checksum didn't match".into())
        );
        assert_eq!(std::fs::read_to_string(dest.join("grokhub")).unwrap(), "running build");
        assert!(!dest.join("grokhub.old").exists());

        // Good: the newest build (not the one GitHub listed first) swaps in, old kept.
        let mut ok = server(vec![(api, list.into_bytes()), (sums_url, sums.into_bytes()), (new_url, good)]);
        let line = install_release_with(Channel::Beta, false, &mut ok, &dest, &d.join("work")).unwrap();
        assert_eq!(line, format!("Installed GrokHub 2.13.1-beta (29e8ac5) into {} (checked against SHA256SUMS).", dest.display()));
        assert_eq!(std::fs::read_to_string(dest.join("grokhub")).unwrap(), "grokhub 29e8ac5");
        assert_eq!(std::fs::read_to_string(dest.join("grokhub-hub")).unwrap(), "hub 29e8ac5");
        assert_eq!(std::fs::read_to_string(dest.join("grokhub.old")).unwrap(), "running build");
        assert!(!d.join("work").exists(), "scratch is cleaned up");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn stable_without_sha256sums_is_refused_and_nothing_is_downloaded() {
        let d = tmp("install-stable");
        let latest = release_json("v2.13.1", false, "2026-10-10T20:44:35Z", false);
        let api = grokhub_core::release_verify::releases_api(Channel::Stable).to_string();
        let mut asked = Vec::new();
        let mut get = |url: &str, _cap: u64, _secs: u64| {
            asked.push(url.to_string());
            if url == api { Ok(latest.clone().into_bytes()) } else { Err("HTTP 404".to_string()) }
        };
        assert_eq!(
            install_release_with(Channel::Stable, false, &mut get, &d.join("bin"), &d.join("work")),
            Err("Update 2.13.1 not installed: the release has no SHA256SUMS".into())
        );
        assert_eq!(asked, vec!["https://api.github.com/repos/blackviperxiii-ui/GrokHub/releases/latest".to_string()]);
        assert!(!d.join("bin").exists());
        let _ = std::fs::remove_dir_all(d);
    }

}
