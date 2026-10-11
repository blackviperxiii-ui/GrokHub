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

fn fetch_text(url: &str) -> Result<String, Refusal> {
    let bytes = fetch(url, TEXT_CAP, Duration::from_secs(30)).map_err(Refusal::Download)?;
    String::from_utf8(bytes).map_err(|_| Refusal::Download("not text".into()))
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
    let mut version = String::new();
    let run = (|| -> Result<String, Refusal> {
        let releases = parse_releases(&fetch_text(releases_api(channel))?);
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
        let sums_text = fetch_text(&sums.url)?;
        let sig_text = match sig {
            Some(s) => Some(fetch_text(&s.url)?),
            None => None,
        };
        let bytes = fetch(&asset.url, ASSET_CAP, Duration::from_secs(600)).map_err(Refusal::Download)?;
        verify_asset(RELEASE_PUBKEY, &sums_text, sig_text.as_deref(), &asset.name, &bytes)?;
        let work = std::env::temp_dir().join("grokhub-release-update");
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).map_err(|e| Refusal::Download(e.to_string()))?;
        let archive = work.join(&asset.name);
        std::fs::write(&archive, &bytes).map_err(|e| Refusal::Download(e.to_string()))?;
        let stage = work.join("stage");
        unpack(&archive, &stage).map_err(Refusal::Download)?;
        let mut files = Vec::new();
        for name in binary_names(windows) {
            let found = find_under(&stage, name).ok_or_else(|| Refusal::Download(format!("{name} missing from {}", asset.name)))?;
            files.push((found, name));
        }
        let dest = install_dir(windows).map_err(Refusal::Download)?;
        swap_in(&dest, &files).map_err(Refusal::Download)?;
        let _ = std::fs::remove_dir_all(&work);
        Ok(format!("Installed GrokHub {} into {} (checked against SHA256SUMS).", rel.version(), dest.display()))
    })();
    run.map_err(|why| refusal_note(&version, &why))
}

/// The plan runner's output for the release step, shaped like a host
/// command's (`exit 0` / `exit 1`) so success and failure read the same way.
pub(crate) fn release_step_receipt(result: Result<String, String>) -> String {
    match result {
        Ok(line) => format!("$ {}\nexit 0\n{line}", grokhub_core::release_verify::RELEASE_INSTALL_STEP),
        Err(line) => format!("$ {}\nexit 1\n{line}", grokhub_core::release_verify::RELEASE_INSTALL_STEP),
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
        let ok = release_step_receipt(Ok("Installed GrokHub 2.13.1 into /x (checked against SHA256SUMS).".into()));
        assert_eq!(ok, "$ grokhub: install the verified release\nexit 0\nInstalled GrokHub 2.13.1 into /x (checked against SHA256SUMS).");
        assert!(!crate::update::host_receipt_failed(&ok));
        let bad = release_step_receipt(Err("Update 2.13.1 not installed: signature didn't verify".into()));
        assert!(crate::update::host_receipt_failed(&bad), "{bad}");
        assert!(bad.ends_with("Update 2.13.1 not installed: signature didn't verify"));
    }
}
