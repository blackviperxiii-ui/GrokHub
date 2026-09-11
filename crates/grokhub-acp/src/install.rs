//! First-run Grok Build CLI **alpha** install (Linux + Windows).

use crate::locate::{
    clear_grok_unusable, cli_install_should_skip, doctor_broken_hint, find_grok,
    grok_cli_is_runnable, grok_marked_unusable, invalidate_grok_bin_cache,
};
#[cfg(windows)]
use crate::locate::hide_windows_console;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
const OFFICIAL_PS: &str = "$env:GROK_CHANNEL='alpha'; irm https://x.ai/cli/install.ps1 | iex";
/// Channel must apply to the installer bash, not only curl (a prefix on curl is dropped by the pipe).
const OFFICIAL_SH: &str = "curl -fsSL https://x.ai/cli/install.sh | GROK_CHANNEL=alpha bash";

/// Platform one-liner shown in Settings.
pub fn grok_cli_install_cmd() -> &'static str {
    if cfg!(windows) {
        OFFICIAL_PS
    } else {
        OFFICIAL_SH
    }
}

/// Background install. Completes immediately if `grok` is already runnable.
pub fn begin_grok_install() -> Receiver<Result<PathBuf, String>> {
    begin_grok_install_opts(false)
}

/// Get Started → Install Grok Build CLI. Re-runs the alpha installer even if a
/// leftover or broken `grok.exe` is on disk.
pub fn begin_grok_install_force() -> Receiver<Result<PathBuf, String>> {
    begin_grok_install_opts(true)
}

fn begin_grok_install_opts(force: bool) -> Receiver<Result<PathBuf, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(install_grok_blocking_opts(force));
    });
    rx
}

pub fn install_grok_blocking() -> Result<PathBuf, String> {
    install_grok_blocking_opts(false)
}

pub fn install_grok_blocking_force() -> Result<PathBuf, String> {
    install_grok_blocking_opts(true)
}

fn install_grok_blocking_opts(force: bool) -> Result<PathBuf, String> {
    if !force {
        if let Some(p) = find_grok() {
            if cli_install_should_skip(Some(&p)) {
                return Ok(p);
            }
            // Soft --version miss (timeout / AV stall): keep a present CLI.
            if !grok_marked_unusable(&p) {
                return Ok(p);
            }
        }
    }
    if let Some(staged) = grok_staged_bin() {
        if staged.is_file() {
            let _ = std::fs::remove_file(&staged);
        }
    }
    clear_grok_unusable();
    invalidate_grok_bin_cache();
    #[cfg(windows)]
    {
        run_official_powershell().or_else(|ps_err| {
            run_direct_download().map_err(|dl_err| {
                format!("{ps_err}; fallback download failed: {dl_err}")
            })
        })?;
        finish_cli_install(
            "Grok Build CLI install finished but grok.exe was not found — run: $env:GROK_CHANNEL='alpha'; irm https://x.ai/cli/install.ps1 | iex",
        )
    }
    #[cfg(not(windows))]
    {
        run_official_sh()?;
        finish_cli_install(
            "Grok Build CLI alpha install finished but grok was not found — run: curl -fsSL https://x.ai/cli/install.sh | GROK_CHANNEL=alpha bash",
        )
    }
}

fn finish_cli_install(missing: &str) -> Result<PathBuf, String> {
    prepend_grok_bin_to_process_path();
    clear_grok_unusable();
    invalidate_grok_bin_cache();
    if let Some(p) = grok_staged_bin().filter(|p| p.is_file()) {
        if grok_cli_is_runnable(&p) {
            return Ok(p);
        }
    }
    let p = find_grok().ok_or_else(|| missing.to_string())?;
    if grok_cli_is_runnable(&p) {
        Ok(p)
    } else {
        Err(doctor_broken_hint().into())
    }
}

/// Drain installer pipes while waiting so a verbose script cannot fill the OS buffer and hang.
fn wait_child_draining(mut child: Child, fail_label: &str) -> Result<(), String> {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let drain = thread::spawn(move || {
        let mut err = String::new();
        if let Some(mut so) = stdout {
            let mut buf = Vec::new();
            let _ = so.read_to_end(&mut buf);
        }
        if let Some(mut se) = stderr {
            let _ = se.read_to_string(&mut err);
        }
        err
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let err = drain.join().unwrap_or_default();
                if status.success() {
                    return Ok(());
                }
                let err = err.trim();
                return Err(if err.is_empty() {
                    format!("{fail_label} (exit {})", status.code().unwrap_or(-1))
                } else {
                    err.to_string()
                });
            }
            Ok(None) if started.elapsed() > INSTALL_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = drain.join();
                return Err(format!("{fail_label} timed out"));
            }
            Ok(None) => thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(format!("wait: {e}")),
        }
    }
}

#[cfg(not(windows))]
fn run_official_sh() -> Result<(), String> {
    let mut cmd = Command::new("bash");
    cmd.args(["-lc", OFFICIAL_SH])
        .env("GROK_CHANNEL", "alpha")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn().map_err(|e| format!("spawn bash: {e}"))?;
    wait_child_draining(child, "Grok Build CLI alpha installer failed")
}

fn grok_bin_dir() -> Option<PathBuf> {
    Some(grokhub_core::user_home()?.join(".grok").join("bin"))
}

fn grok_staged_bin() -> Option<PathBuf> {
    Some(grok_bin_dir()?.join(if cfg!(windows) { "grok.exe" } else { "grok" }))
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
    let sep = if cfg!(windows) { ';' } else { ':' };
    let rest: Vec<String> = std::env::split_paths(&cur)
        .filter(|p| {
            if cfg!(windows) {
                !p.to_string_lossy().eq_ignore_ascii_case(&dir_s)
            } else {
                p != dir
            }
        })
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let new = if rest.is_empty() {
        dir_s.into_owned()
    } else {
        format!("{dir_s}{sep}{}", rest.join(&sep.to_string()))
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
    let child = cmd
        .spawn()
        .map_err(|e| format!("spawn powershell: {e}"))?;
    wait_child_draining(child, "powershell install failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_is_platform_correct() {
        let cmd = grok_cli_install_cmd();
        assert!(cmd.contains("x.ai/cli"), "{cmd}");
        assert!(
            cmd.contains("alpha"),
            "first-run / doctor install must be CLI alpha: {cmd}"
        );
        #[cfg(windows)]
        assert!(cmd.contains("install.ps1") && cmd.contains("GROK_CHANNEL"), "{cmd}");
        #[cfg(not(windows))]
        {
            let curl_prefix = format!("GROK_CHANNEL=alpha {}", "curl");
            assert!(
                cmd.contains("install.sh")
                    && cmd.contains("GROK_CHANNEL=alpha")
                    && cmd.contains("| GROK_CHANNEL=alpha bash")
                    && !cmd.contains(&curl_prefix),
                "GROK_CHANNEL must apply to bash, not only curl: {cmd}"
            );
        }
    }

    #[test]
    fn skip_when_grok_already_on_disk() {
        let _lock = crate::locate::grok_env_test_lock();
        let dir = std::env::temp_dir().join(format!("grokhub-install-skip-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let bin = dir.join(if cfg!(windows) { "grok.exe" } else { "grok" });
        #[cfg(windows)]
        {
            std::fs::write(&bin, &b"MZ\0\0"[..]).unwrap();
            assert!(
                !cli_install_should_skip(Some(&bin)),
                "a stub grok.exe must not skip the alpha installer"
            );
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        #[cfg(not(windows))]
        {
            std::fs::write(&bin, "#!/bin/sh\necho '9.9.9-test (deadbeef)'\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&bin).unwrap().permissions();
            p.set_mode(0o755);
            std::fs::set_permissions(&bin, p).unwrap();
        }
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
    fn force_install_is_exported() {
        let src = include_str!("install.rs");
        assert!(
            src.contains("begin_grok_install_force") && src.contains("GROK_CHANNEL"),
            "Settings must be able to re-run the alpha installer: {src}"
        );
        assert!(
            src.contains("cli_install_should_skip") && src.contains("grok_cli_is_runnable"),
            "a leftover grok.exe that cannot start must not count as installed: {src}"
        );
        assert!(
            src.contains("grok_marked_unusable") && src.contains("grok_staged_bin"),
            "boot install must keep a present CLI on a soft --version miss and validate ~/.grok/bin first: {src}"
        );
    }

    #[test]
    fn prepend_dir_to_path_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("grokhub-path-{}", std::process::id()));
        let old = std::env::var_os("PATH");
        std::env::set_var("PATH", "/no/such/grokhub-path");
        prepend_dir_to_path(&dir);
        prepend_dir_to_path(&dir);
        let path = std::env::var("PATH").unwrap_or_default();
        let sep = if cfg!(windows) { ';' } else { ':' };
        std::env::set_var(
            "PATH",
            format!("/no/such/first{sep}{}{sep}/no/such/last", dir.display()),
        );
        prepend_dir_to_path(&dir);
        let moved = std::env::var("PATH").unwrap_or_default();
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
        assert!(
            moved.starts_with(&*dir.to_string_lossy())
                || moved.to_lowercase().starts_with(&dir.to_string_lossy().to_lowercase()),
            "an existing ~/.grok/bin later on PATH must move to the front: {moved}"
        );
        assert_eq!(
            moved.matches(name.as_ref()).count(),
            1,
            "moving ~/.grok/bin to the front must not duplicate it: {moved}"
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
        #[cfg(not(windows))]
        {
            assert!(src.contains("GROK_CHANNEL=alpha"), "{src}");
            assert!(
                src.contains("| GROK_CHANNEL=alpha bash"),
                "Linux first-run must put GROK_CHANNEL on bash: {src}"
            );
            assert!(
                src.contains("wait_child_draining") && src.contains("read_to_end"),
                "Linux first-run must drain installer pipes: {src}"
            );
            assert!(
                src.contains("run_official_sh") || src.contains("install.sh"),
                "Linux first-run must actually run the alpha installer: {src}"
            );
        }
    }

}
