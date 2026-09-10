//! First-run Grok Build CLI install. Windows only — Linux uses install-grok-cli.sh.

use crate::locate::find_grok;
#[cfg(any(windows, test))]
use crate::locate::invalidate_grok_bin_cache;
#[cfg(windows)]
use crate::locate::hide_windows_console;
#[cfg(windows)]
use std::io::Read;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
const OFFICIAL_PS: &str = "$env:GROK_CHANNEL='alpha'; irm https://x.ai/cli/install.ps1 | iex";
const OFFICIAL_SH: &str = "curl -fsSL https://x.ai/cli/install.sh | bash";

/// Platform one-liner shown in Settings.
pub fn grok_cli_install_cmd() -> &'static str {
    if cfg!(windows) {
        OFFICIAL_PS
    } else {
        OFFICIAL_SH
    }
}

/// Background install. Completes immediately if `grok` is already on disk.
pub fn begin_grok_install() -> Receiver<Result<PathBuf, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(install_grok_blocking());
    });
    rx
}

pub fn install_grok_blocking() -> Result<PathBuf, String> {
    if let Some(p) = find_grok() {
        return Ok(p);
    }
    #[cfg(windows)]
    {
        run_official_powershell().or_else(|ps_err| {
            run_direct_download().map_err(|dl_err| {
                format!("{ps_err}; fallback download failed: {dl_err}")
            })
        })?;
        prepend_grok_bin_to_process_path();
        invalidate_grok_bin_cache();
        find_grok().ok_or_else(|| {
            "Grok Build CLI install finished but grok.exe was not found — run: $env:GROK_CHANNEL='alpha'; irm https://x.ai/cli/install.ps1 | iex".into()
        })
    }
    #[cfg(not(windows))]
    {
        Err(crate::doctor_missing_hint().into())
    }
}

fn grok_bin_dir() -> Option<PathBuf> {
    Some(grokhub_core::user_home()?.join(".grok").join("bin"))
}

pub fn prepend_grok_bin_to_process_path() {
    let Some(dir) = grok_bin_dir() else {
        return;
    };
    prepend_dir_to_path(&dir);
}

pub fn prepend_dir_to_path(dir: &Path) {
    let dir_s = dir.to_string_lossy();
    let cur = std::env::var_os("PATH").unwrap_or_default();
    let already = std::env::split_paths(&cur).any(|p| {
        if cfg!(windows) {
            p.to_string_lossy().eq_ignore_ascii_case(&dir_s)
        } else {
            p == dir
        }
    });
    if already {
        return;
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    let cur_s = cur.to_string_lossy();
    let new = if cur_s.is_empty() {
        dir_s.into_owned()
    } else {
        format!("{dir_s}{sep}{cur_s}")
    };
    std::env::set_var("PATH", new);
}

#[cfg(windows)]
fn run_official_powershell() -> Result<(), String> {
    run_hidden_powershell(OFFICIAL_PS)
}

#[cfg(windows)]
fn run_direct_download() -> Result<(), String> {
    let script = r#"
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
$ver = $null
foreach ($u in @('https://x.ai/cli/alpha','https://storage.googleapis.com/grok-build-public-artifacts/cli/alpha')) {
  try { $ver = (Invoke-WebRequest -Uri $u -UseBasicParsing).Content.Trim(); if ($ver -match '^\d+\.\d+\.\d+') { break } } catch {}
}
if (-not $ver) { throw 'could not resolve Grok Build version' }
$dir = Join-Path $env:USERPROFILE '.grok\bin'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$out = Join-Path $dir 'grok.exe'
$ok = $false
foreach ($base in @("https://x.ai/cli/grok-$ver-windows-x86_64.exe","https://storage.googleapis.com/grok-build-public-artifacts/cli/grok-$ver-windows-x86_64.exe")) {
  try { Invoke-WebRequest -Uri $base -OutFile $out -UseBasicParsing; $ok = $true; break } catch {}
}
if (-not $ok) { throw 'binary download failed' }
Copy-Item $out (Join-Path $dir 'agent.exe') -Force
"#;
    run_hidden_powershell(script)
}

#[cfg(windows)]
fn run_hidden_powershell(command: &str) -> Result<(), String> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-NoLogo",
        "-WindowStyle",
        "Hidden",
        "-Command",
        command,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    hide_windows_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn powershell: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut err = String::new();
                if let Some(mut se) = child.stderr.take() {
                    let _ = se.read_to_string(&mut err);
                }
                if status.success() {
                    return Ok(());
                }
                let err = err.trim();
                return Err(if err.is_empty() {
                    format!("powershell install failed (exit {})", status.code().unwrap_or(-1))
                } else {
                    err.to_string()
                });
            }
            Ok(None) if started.elapsed() > INSTALL_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Grok Build CLI install timed out".into());
            }
            Ok(None) => thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(format!("wait powershell: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_is_platform_correct() {
        let cmd = grok_cli_install_cmd();
        #[cfg(windows)]
        assert!(cmd.contains("install.ps1"), "{cmd}");
        #[cfg(not(windows))]
        assert!(cmd.contains("install.sh"), "{cmd}");
        assert!(cmd.contains("x.ai/cli"), "{cmd}");
        #[cfg(windows)]
        assert!(cmd.contains("alpha"), "{cmd}");
        #[cfg(not(windows))]
        assert!(!cmd.contains("alpha"), "{cmd}");
    }

    #[test]
    fn skip_when_grok_already_on_disk() {
        let _lock = crate::locate::grok_env_test_lock();
        let dir = std::env::temp_dir().join(format!("grokhub-install-skip-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let bin = dir.join(if cfg!(windows) { "grok.exe" } else { "grok" });
        // Windows locate rejects ELF/unix leftovers; a skip hit must look native.
        std::fs::write(&bin, if cfg!(windows) { &b"MZ\0\0"[..] } else { &b"x"[..] }).unwrap();
        let prev = std::env::var_os("GROKHUB_GROK");
        std::env::set_var("GROKHUB_GROK", &bin);
        invalidate_grok_bin_cache();
        let hit = install_grok_blocking();
        match prev {
            Some(v) => std::env::set_var("GROKHUB_GROK", v),
            None => std::env::remove_var("GROKHUB_GROK"),
        }
        invalidate_grok_bin_cache();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(hit.ok(), Some(bin));
    }

    #[test]
    fn prepend_dir_to_path_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("grokhub-path-{}", std::process::id()));
        let old = std::env::var_os("PATH");
        std::env::set_var("PATH", "/no/such/grokhub-path");
        prepend_dir_to_path(&dir);
        prepend_dir_to_path(&dir);
        let path = std::env::var("PATH").unwrap_or_default();
        match old {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        let name = dir.file_name().unwrap().to_string_lossy();
        assert_eq!(
            path.matches(name.as_ref()).count(),
            1,
            "PATH must list the grok bin once: {path}"
        );
        assert!(
            path.starts_with(&*dir.to_string_lossy()) || path.to_lowercase().starts_with(&dir.to_string_lossy().to_lowercase()),
            "grok bin must be first on PATH: {path}"
        );
    }

    #[test]
    fn windows_install_is_hidden() {
        let src = include_str!("install.rs");
        assert!(src.contains("hide_windows_console"), "{src}");
        assert!(src.contains("install.ps1"), "{src}");
        assert!(src.contains("WindowStyle") && src.contains("Hidden"), "{src}");
        assert!(src.contains("storage.googleapis.com/grok-build-public-artifacts"), "{src}");
        assert!(src.contains("x.ai/cli/alpha"), "{src}");
        let pointer = format!("x.ai/cli/{}", "stable");
        assert!(
            !src.contains(&pointer),
            "Windows first-run must download alpha, not {pointer}"
        );
    }

}
