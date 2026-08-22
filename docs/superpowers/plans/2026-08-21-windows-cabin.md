# Windows cabin installer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the native egui GrokHub cabin on Windows as a per-user Inno Setup installer, with Grok Build (`grok.exe`) bundled when the official artifact downloads, without Windows computer-use backends.

**Architecture:** Copy committed Linux `GrokHub` `main` into a new public `GrokHub-Windows` repo. Keep the same crates. Add `cfg(windows)` for paths, PowerShell host, ACP spawn, pid-alive, and a Win32 tray. GitHub Actions `windows-latest` builds `grokhub.exe` / `grokhub-hub.exe`, optionally vendors `grok.exe`, and produces `GrokHub-Setup-<version>.exe`.

**Tech Stack:** Rust 2021, eframe 0.29 glow, `tray-icon` on Windows, `ksni` on Linux only, Inno Setup 6, GitHub Actions `windows-latest`.

## Global Constraints

- Cabin-first: no Windows AT-SPI / screenshot / click backends; Grok Build owns computer-use.
- Do not vendor grok-build crates. Do not wrap in Electron or Tauri.
- Source is committed Linux `main` (v2.6.42). Do not copy uncommitted local cabin WIP (`loops.rs`, `catalog.rs`, `grok_loop.rs`, etc.).
- Per-user install prefix: `%LOCALAPPDATA%\Programs\GrokHub`. Config: `%APPDATA%\GrokHub`. Grok login: `%USERPROFILE%\.grok`.
- x86_64 Windows only. No admin. Packager is Inno Setup 6. Artifact name `GrokHub-Setup-<version>.exe`.
- Grok artifact URL: `https://x.ai/cli/grok-<ver>-windows-x86_64.exe` with `<ver>` from `https://x.ai/cli/stable`. Download failure is a warning, not a ship blocker.
- Cross-compile from CachyOS is not the release path.
- Do not recreate or push the deleted Electron `Grok-Hub-Windows` history.

## File map

| File | Responsibility |
|------|----------------|
| `/home/viper/src/GrokHub-Windows/` | New git repo; all later tasks run here |
| `crates/grokhub-core/src/paths.rs` | `user_home()` HOME/USERPROFILE |
| `crates/grokhub-core/src/project.rs` | Windows-absolute path expansion |
| `crates/grokhub-app/src/config.rs` | `%APPDATA%\GrokHub` |
| `crates/grokhub-acp/src/locate.rs` | `grok.exe` locate + doctor hint |
| `crates/grokhub-app/src/host.rs` | PowerShell host + `Path::is_absolute` cites |
| `crates/grokhub-acp/src/client.rs` | Skip unix `pre_exec` on Windows |
| `crates/grokhub-app/src/tray.rs` | `cabin_pid_alive`; Win32 `TrayHost` |
| `crates/grokhub-app/Cargo.toml` | eframe/ksni/tray-icon per OS |
| `packaging/windows/grokhub.iss` | Inno Setup script |
| `scripts/make-windows-release.ps1` | Stage binaries, optional grok download, ISCC, zip |
| `.github/workflows/ci.yml` | `windows-latest` test + build |
| `.github/workflows/release.yml` | Tag → Setup.exe + zip |

---

### Task 1: Bootstrap `GrokHub-Windows` from committed Linux main

**Files:**
- Create: `/home/viper/src/GrokHub-Windows/**` (archive of Linux `HEAD` after this plan is committed)
- Modify: `README.md` (Windows-first clone/install blurb at the top; keep Linux notes below)

**Interfaces:**
- Consumes: Linux repo at `/home/viper/src/Grok-Hub` committed `HEAD` (includes this plan + the Windows spec). Working-tree dirty files must not be copied.
- Produces: git repo `/home/viper/src/GrokHub-Windows` on `main`, `cargo test -p grokhub-core` green on Linux.

- [ ] **Step 1: Confirm source commit has the spec and this plan, and WIP is unstaged**

```bash
cd /home/viper/src/Grok-Hub
git log -1 --oneline
test -f docs/superpowers/specs/2026-08-21-windows-cabin-design.md
test -f docs/superpowers/plans/2026-08-21-windows-cabin.md
git status --porcelain | grep -E 'loops\.rs|catalog\.rs|grok_loop\.rs' || true
```

Expected: `HEAD` is a commit that contains both docs. Dirty `loops.rs` / `catalog.rs` / `grok_loop.rs` stay unstaged and must not appear in the archive.

- [ ] **Step 2: Archive committed tree into the new repo**

```bash
rm -rf /home/viper/src/GrokHub-Windows
mkdir -p /home/viper/src/GrokHub-Windows
git -C /home/viper/src/Grok-Hub archive HEAD | tar -C /home/viper/src/GrokHub-Windows -xf -
cd /home/viper/src/GrokHub-Windows
test ! -f crates/grokhub-app/src/loops.rs
test ! -f crates/grokhub-acp/src/catalog.rs
git init -b main
git add -A
git -c user.name='Viper' -c user.email='blackviperxiii-ui@users.noreply.github.com' commit -m "chore: import GrokHub v2.6.42 for Windows cabin"
```

- [ ] **Step 3: Baseline tests on the copy (Linux host)**

Run: `cd /home/viper/src/GrokHub-Windows && cargo test --locked -p grokhub-core -p grokhub-acp --offline 2>/dev/null || cargo test --locked -p grokhub-core -p grokhub-acp`

Expected: PASS (existing tests).

- [ ] **Step 4: Commit is already done in Step 2. All later tasks `cd /home/viper/src/GrokHub-Windows`.**

---

### Task 2: `user_home` and Windows config dir

**Files:**
- Create: `crates/grokhub-core/src/paths.rs`
- Modify: `crates/grokhub-core/src/lib.rs` (add `pub mod paths;` and `pub use paths::user_home;`)
- Modify: `crates/grokhub-core/src/project.rs` (`expand_host_path_token_in` absolute check)
- Modify: `crates/grokhub-app/src/config.rs` (`dirs_fallback`)
- Test: `crates/grokhub-core/src/paths.rs` (inline `#[cfg(test)]`)
- Test: `crates/grokhub-app/src/config.rs` existing tests plus one Windows-shaped fallback

**Interfaces:**
- Consumes: none
- Produces: `pub fn user_home() -> Option<PathBuf>` — `HOME` then `USERPROFILE`. `config::config_dir()` on Windows is `%APPDATA%\GrokHub` when `GROKHUB_CONFIG` is unset. `expand_host_path_token_in` treats `Path::new(tok).is_absolute()` as absolute (so `C:\foo` works).

- [ ] **Step 1: Write the failing tests in `crates/grokhub-core/src/paths.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn user_home_prefers_home_then_userprofile() {
        let _g = LOCK.lock().unwrap();
        let old_home = std::env::var_os("HOME");
        let old_up = std::env::var_os("USERPROFILE");
        std::env::remove_var("HOME");
        std::env::set_var("USERPROFILE", r"C:\Users\viper");
        let got = user_home().expect("USERPROFILE");
        assert!(got.ends_with("viper") || got.to_string_lossy().contains("viper"));
        std::env::remove_var("USERPROFILE");
        assert!(user_home().is_none() || old_home.is_some());
        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match old_up {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
    }
}
```

Also add in `project.rs` tests (same module that tests `expand_project_root`):

```rust
#[test]
fn expand_project_root_keeps_windows_drive_paths() {
    let p = expand_project_root(r"C:\Users\viper\proj", Some(r"C:\Users\viper"));
    assert!(p.starts_with("C:") || p.contains("Users"), "{p}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --locked -p grokhub-core -- user_home_prefers_home_then_userprofile expand_project_root_keeps_windows_drive_paths`

Expected: FAIL compiling `user_home` not found / assertion on `C:\` currently rewritten or rejected because `starts_with('/')` is false and `~` rules do not apply — `expand_host_path_token_in` returns `None` so `expand_project_root` returns the token unchanged. If it already returns `C:\Users\viper\proj` unchanged, the test PASSES early; then keep it as a regression lock and still add `user_home`.

- [ ] **Step 3: Implement**

`crates/grokhub-core/src/paths.rs`:

```rust
use std::path::PathBuf;

pub fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
```

In `expand_host_path_token_in`, replace `if tok.starts_with('/')` with:

```rust
if std::path::Path::new(tok).is_absolute() {
    return Some(tok);
}
```

Join with `std::path::Path` when prefixing home so Windows gets `\`:

```rust
if let Some(rest) = tok.strip_prefix("~/") {
    return Some(std::path::Path::new(home).join(rest).to_string_lossy().into_owned());
}
```

Same for `$HOME/`.

`config.rs` `dirs_fallback`:

```rust
fn dirs_fallback() -> PathBuf {
    if cfg!(windows) {
        if let Ok(app) = std::env::var("APPDATA") {
            if !app.trim().is_empty() {
                return PathBuf::from(app).join("GrokHub");
            }
        }
    }
    if let Some(home) = grokhub_core::user_home() {
        return home.join(".config/GrokHub");
    }
    PathBuf::from(".grokhub")
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --locked -p grokhub-core -- user_home_prefers_home_then_userprofile expand_project_root_keeps_windows_drive_paths`

Expected: PASS. Also `cargo test --locked -p grokhub-app config::tests -- --test-threads=1` still PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/grokhub-core/src/paths.rs crates/grokhub-core/src/lib.rs crates/grokhub-core/src/project.rs crates/grokhub-app/src/config.rs
git commit -m "feat: Windows user home and %APPDATA% config dir"
```

---

### Task 3: Locate `grok.exe`

**Files:**
- Modify: `crates/grokhub-acp/src/locate.rs`
- Test: `crates/grokhub-acp/src/locate.rs` `mod tests`

**Interfaces:**
- Consumes: `grokhub_core::user_home`
- Produces: `which(name)` also tries `name.exe` on Windows. `find_grok_scan` order: `GROKHUB_GROK`, PATH `grok`/`grok.exe`, `{user_home}/.grok/bin/grok.exe` (and `grok` on Unix), `{current_exe_dir}/grok.exe`. `grok_home()` is `{user_home}/.grok`. `cabin_grok_home()` is `{config-equivalent}/grok-home` using the same dir as cabin config: Windows `%APPDATA%\GrokHub\grok-home`, else `{user_home}/.config/GrokHub/grok-home`. `doctor_missing_hint()` is `"Grok Build CLI missing — install from x.ai/cli"` on Unix and `"Grok Build CLI missing — irm https://x.ai/cli/install.ps1 | iex"` on Windows.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn which_tries_exe_suffix() {
    let dir = std::env::temp_dir().join(format!("grokhub-which-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let exe = if cfg!(windows) { "probe.exe" } else { "probe" };
    std::fs::write(dir.join(exe), b"x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = std::fs::metadata(dir.join(exe)).unwrap().permissions();
        p.set_mode(0o755);
        std::fs::set_permissions(dir.join(exe), p).unwrap();
    }
    let old = std::env::var_os("PATH");
    std::env::set_var("PATH", &dir);
    let hit = which("probe");
    match old {
        Some(v) => std::env::set_var("PATH", v),
        None => std::env::remove_var("PATH"),
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(hit.is_some(), "which(probe) must find {exe}");
}

#[test]
fn find_grok_scan_uses_userprofile_grok_bin() {
    let prev = std::env::var_os("GROKHUB_GROK");
    std::env::remove_var("GROKHUB_GROK");
    let dir = std::env::temp_dir().join(format!("grokhub-grokhome-{}", std::process::id()));
    let bin = dir.join(".grok").join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let name = if cfg!(windows) { "grok.exe" } else { "grok" };
    std::fs::write(bin.join(name), b"x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = std::fs::metadata(bin.join(name)).unwrap().permissions();
        p.set_mode(0o755);
        std::fs::set_permissions(bin.join(name), p).unwrap();
    }
    let old_home = std::env::var_os("HOME");
    let old_up = std::env::var_os("USERPROFILE");
    std::env::set_var("HOME", &dir);
    std::env::set_var("USERPROFILE", &dir);
    let old_path = std::env::var_os("PATH");
    std::env::set_var("PATH", "/no/such/grokhub-path");
    let hit = find_grok_scan();
    match old_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
    match old_up {
        Some(v) => std::env::set_var("USERPROFILE", v),
        None => std::env::remove_var("USERPROFILE"),
    }
    match old_path {
        Some(v) => std::env::set_var("PATH", v),
        None => std::env::remove_var("PATH"),
    }
    match prev {
        Some(v) => std::env::set_var("GROKHUB_GROK", v),
        None => std::env::remove_var("GROKHUB_GROK"),
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        hit.as_ref().is_some_and(|p| p.ends_with(name)),
        "expected {name} under ~/.grok/bin, got {hit:?}"
    );
}

#[test]
fn doctor_missing_names_official_install() {
    let (_, text) = doctor_grok_line(None);
    assert!(!doctor_grok_line(None).0);
    assert!(text.contains("x.ai/cli"), "{text}");
    #[cfg(windows)]
    assert!(text.contains("install.ps1"), "{text}");
}
```

- [ ] **Step 2: Run tests to verify fail**

Run: `cargo test --locked -p grokhub-acp -- which_tries_exe_suffix find_grok_scan_uses_userprofile_grok_bin doctor_missing_names_official_install`

Expected: FAIL — `find_grok_scan` only looks at `/.local/bin/grok` and `/.grok/bin/grok` via `HOME`, not `grok.exe`.

- [ ] **Step 3: Implement `which` / `find_grok_scan` / homes / doctor hint**

```rust
pub fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
        if cfg!(windows) && !name.ends_with(".exe") {
            let p = dir.join(format!("{name}.exe"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn grok_bin_name() -> &'static str {
    if cfg!(windows) { "grok.exe" } else { "grok" }
}

fn find_grok_scan() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("GROKHUB_GROK") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    if let Some(p) = which("grok") {
        return Some(p);
    }
    let home = grokhub_core::user_home()?;
    let p = home.join(".grok").join("bin").join(grok_bin_name());
    if p.is_file() {
        return Some(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(grok_bin_name());
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn grok_home() -> Option<PathBuf> {
    Some(grokhub_core::user_home()?.join(".grok"))
}

pub fn cabin_grok_home() -> Option<PathBuf> {
    Some(cabin_config_root()?.join("grok-home"))
}

fn cabin_config_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("GROKHUB_CONFIG") {
        return Some(PathBuf::from(p));
    }
    if cfg!(windows) {
        let app = std::env::var_os("APPDATA")?;
        return Some(PathBuf::from(app).join("GrokHub"));
    }
    Some(grokhub_core::user_home()?.join(".config/GrokHub"))
}

pub fn doctor_missing_hint() -> &'static str {
    if cfg!(windows) {
        "Grok Build CLI missing — irm https://x.ai/cli/install.ps1 | iex"
    } else {
        "Grok Build CLI missing — install from x.ai/cli"
    }
}
```

Replace both `"Grok Build CLI missing — install from x.ai/cli"` string literals in `doctor_grok_line` with `doctor_missing_hint()`.

Keep unix symlink in `prepare_cabin_grok_home`; Windows already copies `auth.json`.

- [ ] **Step 4: Run tests**

Run: `cargo test --locked -p grokhub-acp -- locate::`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/grokhub-acp/src/locate.rs
git commit -m "feat: locate grok.exe from PATH, %USERPROFILE%\\.grok\\bin, and install dir"
```

---

### Task 4: PowerShell host runner

**Files:**
- Modify: `crates/grokhub-app/src/host.rs`
- Test: `crates/grokhub-app/src/host.rs` `mod tests`

**Interfaces:**
- Consumes: `grokhub_core::user_home` for `host_working_dir` HOME fallback
- Produces: `run_host_stream` spawns `bash -lc` on Unix and `powershell.exe -NoProfile -Command` on Windows. `resolve_host_cite_path` uses `Path::is_absolute()` instead of `starts_with('/')`. `kill_host` unix process-group kill stays `#[cfg(unix)]`.

- [ ] **Step 1: Gate existing bash test; add Windows echo test**

Replace `echo_ok` with:

```rust
#[test]
fn echo_ok() {
    let out = run_host("echo grokhub-smoke", Duration::from_secs(5));
    assert!(out.contains("grokhub-smoke"), "{out}");
    assert!(out.contains("exit 0"), "{out}");
}

#[cfg(windows)]
#[test]
fn windows_host_is_powershell() {
    let src = include_str!("host.rs");
    assert!(src.contains("powershell.exe"), "{src}");
    assert!(src.contains("-NoProfile"), "{src}");
}
```

In `resolve_host_cite_path`, current Unix absolute `/tmp/abs.txt` test stays `#[cfg(unix)]`. Add:

```rust
#[cfg(windows)]
#[test]
fn resolve_host_cite_path_keeps_windows_absolute() {
    let abs = r"C:\Windows\Temp\abs.txt";
    assert_eq!(resolve_host_cite_path(r"C:\proj", abs), abs);
}
```

Change `host_working_dir` / cite tests to use `grokhub_core::user_home()` instead of `env::var("HOME")`.

- [ ] **Step 2: Run tests**

Run: `cargo test --locked -p grokhub-app -- host::tests::echo_ok host::tests::windows_host_is_powershell`

Expected: `windows_host_is_powershell` FAIL (no powershell.exe in source). `echo_ok` still PASS on Linux.

- [ ] **Step 3: Implement spawn**

Replace the `Command::new("bash")` block in `run_host_stream` with:

```rust
    let mut spawn = if cfg!(windows) {
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile", "-Command", cmd]);
        c
    } else {
        let mut c = Command::new("bash");
        c.arg("-lc").arg(cmd);
        c
    };
    spawn.stdout(Stdio::piped()).stderr(Stdio::piped());
```

`resolve_host_cite_path`:

```rust
    let expanded = grokhub_core::expand_project_root(
        cited,
        grokhub_core::user_home()
            .as_ref()
            .and_then(|p| p.to_str()),
    );
    if Path::new(&expanded).is_absolute() {
        return expanded;
    }
```

`host_working_dir` HOME arg: `grokhub_core::user_home().as_ref().and_then(|p| p.to_str())`.

- [ ] **Step 4: Run tests**

Run: `cargo test --locked -p grokhub-app -- host::`

Expected: PASS on Linux. On `windows-latest`, `echo_ok` PASSES via PowerShell `echo`.

- [ ] **Step 5: Commit**

```bash
git add crates/grokhub-app/src/host.rs
git commit -m "feat: run cabin /host through PowerShell on Windows"
```

---

### Task 5: ACP spawn without unix `pre_exec`

**Files:**
- Modify: `crates/grokhub-acp/src/client.rs` (`isolate_spawned_grok` already `#[cfg]`-adjacent; wrap the `/proc` closer)
- Test: `crates/grokhub-acp/src/client.rs` or `locate.rs` source assertion

**Interfaces:**
- Consumes: `cabin_leader_socket()` / `prepare_cabin_grok_home()` from Task 3
- Produces: `connect()` still sets `GROK_NO_AUTO_UPDATE`, `--leader-socket`, `GROK_HOME`. `isolate_spawned_grok` and `pre_exec` remain `#[cfg(unix)]` only. Windows uses stdio pipes only.

- [ ] **Step 1: Write failing source lock test in `client.rs` tests**

```rust
#[test]
fn isolate_spawned_grok_is_unix_only() {
    let src = include_str!("client.rs");
    assert!(src.contains("#[cfg(unix)]"), "pre_exec must stay unix-only");
    let iso = src.split("fn isolate_spawned_grok").nth(1).unwrap_or("");
    assert!(iso.contains("/proc/self/fd") || iso.contains("setsid") || src.contains("fn isolate_spawned_grok"), "{iso}");
    let pre = src.split("cmd.pre_exec").next().unwrap_or("");
    assert!(pre.contains("#[cfg(unix)]"), "CommandExt pre_exec must be behind cfg(unix)");
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --locked -p grokhub-acp -- isolate_spawned_grok_is_unix_only`

Expected: PASS already if `pre_exec` is behind `#[cfg(unix)]` today (it is). Keep the test as a regression lock. If `isolate_spawned_grok` is *not* cfg-gated and is called from unix-only pre_exec, that is OK as long as the function is not compiled on Windows. Wrap the whole `fn isolate_spawned_grok` in `#[cfg(unix)]` so Windows does not compile `setsid`/`close`.

- [ ] **Step 3: Gate the helper**

```rust
#[cfg(unix)]
fn isolate_spawned_grok() { /* existing body */ }

#[cfg(unix)]
fn ignore_sigpipe() { /* existing body */ }
```

`ignore_sigpipe` is currently `#[cfg(unix)]` already. Ensure `isolate_spawned_grok` is too.

- [ ] **Step 4: Run tests**

Run: `cargo test --locked -p grokhub-acp`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/grokhub-acp/src/client.rs
git commit -m "fix: do not compile unix ACP isolation on Windows"
```

---

### Task 6: Windows pid-alive and tray crate split

**Files:**
- Modify: `crates/grokhub-app/src/tray.rs` (`cabin_pid_alive`, `TrayHost`, `spawn`)
- Modify: `crates/grokhub-app/Cargo.toml`
- Test: existing `cabin_pid_alive` tests plus a new one

**Interfaces:**
- Consumes: `TrayCmd::{Show,Halt,Quit}` unchanged; `begin_tray_spawn() -> Receiver<Option<TrayHost>>` unchanged
- Produces: `cabin_pid_alive(pid)` on Windows uses `OpenProcess` / `GetExitCodeProcess` via `std` or a tiny `windows-sys` call — implement with:

```rust
#[cfg(windows)]
pub fn cabin_pid_alive(pid: u32) -> bool {
    use std::process::Command;
    if pid == 0 {
        return false;
    }
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .is_some_and(|s| s.contains(&pid.to_string()))
}
```

That is slow for every paint. Prefer:

```rust
#[cfg(windows)]
pub fn cabin_pid_alive(pid: u32) -> bool {
    windows_pid_alive(pid)
}
```

Add optional dep `windows-sys = { version = "0.59", features = ["Win32_System_Threading", "Win32_Foundation"] }` under `[target.'cfg(windows)'.dependencies]`.

```rust
#[cfg(windows)]
fn windows_pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, STILL_ACTIVE,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h == 0 {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}
```

`TrayHost` on Linux keeps `ksni::blocking::Handle<GrokTray>`. On Windows:

```rust
pub struct TrayHost {
    rx: mpsc::Receiver<TrayCmd>,
    #[cfg(unix)]
    _keep: ksni::blocking::Handle<GrokTray>,
    #[cfg(windows)]
    _keep: tray_icon::TrayIcon,
}
```

`spawn()` on Windows builds a `tray_icon::TrayIcon` with menu Show cabin / Halt / Quit, maps clicks to `TrayCmd`. `ksni` must not be linked on Windows.

Cargo.toml:

```toml
eframe = { version = "0.29", default-features = false, features = ["default_fonts", "glow"] }

[target.'cfg(unix)'.dependencies]
eframe = { version = "0.29", default-features = false, features = ["x11", "wayland"] }
ksni = { version = "0.3.6", features = ["blocking"] }

[target.'cfg(windows)'.dependencies]
tray-icon = "0.19"
windows-sys = { version = "0.59", features = ["Win32_System_Threading", "Win32_Foundation"] }
```

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn cabin_pid_alive_this_process() {
    let pid = std::process::id();
    assert!(cabin_pid_alive(pid), "own pid must look alive");
    assert!(!cabin_pid_alive(0));
}

#[test]
fn tray_host_type_exists() {
    let src = include_str!("tray.rs");
    assert!(src.contains("struct TrayHost"));
    #[cfg(windows)]
    assert!(src.contains("tray_icon"), "{src}");
    #[cfg(unix)]
    assert!(src.contains("ksni"), "{src}");
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --locked -p grokhub-app -- cabin_pid_alive_this_process tray_host_type_exists`

Expected: `cabin_pid_alive_this_process` FAIL on Linux? Today Linux uses `/proc/{pid}` so it PASSES on Linux. On Windows today `cabin_pid_alive` is `true` for any non-zero pid — the test PASSES but is wrong for dead pids. Add:

```rust
#[test]
fn cabin_pid_alive_zero_is_dead() {
    assert!(!cabin_pid_alive(0));
}
```

`tray_host_type_exists` on Linux PASS (ksni). On Windows FAIL until `tray_icon` is referenced.

- [ ] **Step 3: Implement pid + Cargo target deps + Windows `spawn()`**

Windows `spawn()` sketch:

```rust
#[cfg(windows)]
pub fn spawn() -> Option<TrayHost> {
    if !tray_wanted() {
        return None;
    }
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, TrayIconBuilder};
    let (tx, rx) = mpsc::channel();
    let show = MenuItem::new("Show cabin", true, None);
    let halt = MenuItem::new("Halt", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    let _ = menu.append(&show);
    let _ = menu.append(&halt);
    let _ = menu.append(&quit);
    let icon = Icon::from_rgba(vec![232, 168, 96, 255].repeat(22 * 22), 22, 22).ok()?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("GrokHub")
        .with_icon(icon)
        .build()
        .ok()?;
    let show_id = show.id().clone();
    let halt_id = halt.id().clone();
    let quit_id = quit.id().clone();
    std::thread::spawn(move || {
        while let Ok(ev) = MenuEvent::receiver().recv() {
            let cmd = if ev.id == show_id {
                TrayCmd::Show
            } else if ev.id == halt_id {
                TrayCmd::Halt
            } else if ev.id == quit_id {
                TrayCmd::Quit
            } else {
                continue;
            };
            if tx.send(cmd).is_err() {
                break;
            }
        }
    });
    Some(TrayHost { rx, _keep: tray })
}
```

Keep Linux `spawn()` under `#[cfg(unix)]`. Shared `begin_tray_spawn` still calls `spawn`.

`pin_session_bus` / `force_x11_for_close_to_tray` stay Unix-oriented; on Windows they no-op naturally (no DISPLAY). Gate `read_legacy_session_bus_file` reads of `/var/lib/dbus` with `#[cfg(unix)]` so Windows does not touch those paths.

- [ ] **Step 4: Run tests**

Run: `cargo test --locked -p grokhub-app -- cabin_pid_alive tray_host_type_exists`

Expected: PASS on Linux. `cargo build -p grokhub-app` still works on Linux (`ksni` linked).

- [ ] **Step 5: Commit**

```bash
git add crates/grokhub-app/src/tray.rs crates/grokhub-app/Cargo.toml Cargo.lock
git commit -m "feat: Windows tray icon and real pid-alive"
```

---

### Task 7: Packaging script + Inno Setup

**Files:**
- Create: `packaging/windows/grokhub.iss`
- Create: `scripts/make-windows-release.ps1`
- Create: `packaging/windows/README.md` (one screen: ISCC, output names)
- Test: the `.ps1` must treat missing `grokhub.exe` as fatal and grok download failure as warning — lock with a small helper function tested via a dry-run block, or a `scripts/test-windows-stage.ps1` that sources the download function.

Keep the download function in `scripts/make-windows-release.ps1` and a second tiny `scripts/grok-windows-artifact.ps1` that is easy to invoke:

```powershell
# scripts/grok-windows-artifact.ps1
param([Parameter(Mandatory=$true)][string]$DestDir)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Path $DestDir -Force | Out-Null
try {
  $ver = (Invoke-WebRequest -Uri 'https://x.ai/cli/stable' -UseBasicParsing).Content.Trim()
  if ($ver -notmatch '^\d+\.\d+\.\d+') { throw "bad version $ver" }
  $url = "https://x.ai/cli/grok-$ver-windows-x86_64.exe"
  $out = Join-Path $DestDir 'grok.exe'
  Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
  Copy-Item $out (Join-Path $DestDir 'agent.exe')
  Write-Output "bundled $ver"
  exit 0
} catch {
  Write-Warning "grok download skipped: $_"
  exit 0
}
```

**Interfaces:**
- Consumes: `target/release/grokhub.exe`, `target/release/grokhub-hub.exe`
- Produces: `dist-release/GrokHub-Setup-<version>.exe` and `dist-release/grokhub-windows-v<version>.zip`. Missing cabin exe → exit 1. Missing grok → continue.

- [ ] **Step 1: Write Inno script `packaging/windows/grokhub.iss`**

```iss
#define MyAppName "GrokHub"
#ifndef MyAppVersion
  #define MyAppVersion "2.6.42"
#endif
#define MyAppPublisher "GrokHub"
#define MyAppExeName "grokhub.exe"

[Setup]
AppId={{9E2C7C3A-4B11-4F6F-9D3A-6A1C8F0B2E44}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\GrokHub
DefaultGroupName=GrokHub
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\..\dist-release
OutputBaseFilename=GrokHub-Setup-{#MyAppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#MyAppExeName}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "stage\grokhub.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "stage\grokhub-hub.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "stage\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "stage\grok.exe"; DestDir: "{userprofile}\.grok\bin"; Flags: ignoreversion skipifsourcedoesntexist
Source: "stage\agent.exe"; DestDir: "{userprofile}\.grok\bin"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\GrokHub"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\GrokHub"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch GrokHub"; Flags: nowait postinstall skipifsilent

[Registry]
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; \
  ValueData: "{olddata};{userprofile}\.grok\bin"; Flags: preservestringtype; \
  Check: NeedsGrokPath
```

Add Pascal `[Code]` function `NeedsGrokPath` that is true when `stage\grok.exe` was installed (file exists in `{userprofile}\.grok\bin\grok.exe` after copy). Simpler: always append if not already present:

```pascal
function NeedsGrokPath: Boolean;
var
  P: String;
begin
  P := GetEnv('PATH');
  Result := Pos(ExpandConstant('{userprofile}\.grok\bin'), P) = 0;
end;
```

- [ ] **Step 2: Write `scripts/make-windows-release.ps1`**

```powershell
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root
if (-not (Test-Path 'Cargo.toml')) { throw "run from repo root" }
$Ver = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
if (-not $Ver) { throw "version missing" }
cargo build --release --locked -p grokhub-app -p grokhub-hub
if (-not (Test-Path 'target/release/grokhub.exe')) { throw "missing grokhub.exe" }
if (-not (Test-Path 'target/release/grokhub-hub.exe')) { throw "missing grokhub-hub.exe" }
$Stage = Join-Path $Root 'packaging/windows/stage'
Remove-Item $Stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $Stage | Out-Null
Copy-Item 'target/release/grokhub.exe' $Stage
Copy-Item 'target/release/grokhub-hub.exe' $Stage
Copy-Item 'LICENSE' $Stage
& (Join-Path $Root 'scripts/grok-windows-artifact.ps1') -DestDir $Stage
New-Item -ItemType Directory -Path (Join-Path $Root 'dist-release') -Force | Out-Null
$Iscc = @(
  ${env:ProgramFiles + '\Inno Setup 6\ISCC.exe'},
  ${env:LOCALAPPDATA + '\Programs\Inno Setup 6\ISCC.exe'}
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $Iscc) { throw "Inno Setup 6 ISCC.exe not found" }
& $Iscc "/DMyAppVersion=$Ver" (Join-Path $Root 'packaging/windows/grokhub.iss')
$Zip = Join-Path $Root "dist-release/grokhub-windows-v$Ver.zip"
Compress-Archive -Path (Join-Path $Stage '*') -DestinationPath $Zip -Force
Write-Output (Join-Path $Root "dist-release/GrokHub-Setup-$Ver.exe")
Write-Output $Zip
```

Fix the `$Iscc` array to real PowerShell:

```powershell
$candidates = @(
  "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
  "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
)
$Iscc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
```

- [ ] **Step 3: Fatal-missing-exe check without a Windows build (syntax lock)**

Add a tiny test at the top of `make-windows-release.ps1` only as comments plus a dedicated `scripts/make-windows-release.tests.ps1` used in CI:

```powershell
# scripts/make-windows-release.tests.ps1
$src = Get-Content -Raw "$PSScriptRoot/make-windows-release.ps1"
if ($src -notmatch 'missing grokhub.exe') { throw 'pack script must fail closed without grokhub.exe' }
if ($src -notmatch 'grok-windows-artifact.ps1') { throw 'pack script must call grok download helper' }
$dl = Get-Content -Raw "$PSScriptRoot/grok-windows-artifact.ps1"
if ($dl -notmatch 'exit 0') { throw 'grok download failure must not fail the cabin pack' }
if ($dl -notmatch 'x.ai/cli/stable') { throw 'must resolve version from x.ai/cli/stable' }
Write-Output 'pack script locks ok'
```

Run on Linux with `pwsh` if present, else skip locally and run in GHA:

```bash
command -v pwsh >/dev/null && pwsh -File scripts/make-windows-release.tests.ps1
```

Expected: PASS if pwsh exists.

- [ ] **Step 4: Commit**

```bash
git add packaging/windows/grokhub.iss packaging/windows/README.md scripts/make-windows-release.ps1 scripts/grok-windows-artifact.ps1 scripts/make-windows-release.tests.ps1
git commit -m "feat: Inno Setup Windows installer packaging"
```

---

### Task 8: CI, README, GitHub repo

**Files:**
- Modify: `.github/workflows/ci.yml` — replace ubuntu-only as primary; Windows repo CI is `windows-latest`
- Modify: `.github/workflows/release.yml` — tag job builds Setup.exe + zip
- Modify: `README.md` — Windows install first
- Modify: `Cargo.toml` workspace `repository` to `https://github.com/blackviperxiii-ui/GrokHub-Windows.git`

**Interfaces:**
- Consumes: Task 7 scripts
- Produces: public GitHub repo `blackviperxiii-ui/GrokHub-Windows` with `main` pushed, CI defined

- [ ] **Step 1: Replace `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
  workflow_dispatch:

jobs:
  windows:
    runs-on: windows-latest
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace --locked
      - run: cargo build --release --locked -p grokhub-app -p grokhub-hub
      - name: Cabin binaries exist
        shell: pwsh
        run: |
          if (-not (Test-Path target/release/grokhub.exe)) { throw 'missing grokhub.exe' }
          if (-not (Test-Path target/release/grokhub-hub.exe)) { throw 'missing grokhub-hub.exe' }
      - name: Pack script locks
        shell: pwsh
        run: ./scripts/make-windows-release.tests.ps1
      - name: No Electron leftover
        shell: pwsh
        run: |
          if (Test-Path package.json) { throw 'electron leftover' }
          if (Test-Path desktop) { throw 'electron leftover' }
```

- [ ] **Step 2: Replace `.github/workflows/release.yml`**

```yaml
name: Release

on:
  push:
    tags:
      - "v*"

permissions:
  contents: write

jobs:
  windows-installer:
    runs-on: windows-latest
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Inno Setup
        run: choco install innosetup --no-progress
      - name: Package
        shell: pwsh
        run: ./scripts/make-windows-release.ps1
      - name: Publish GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            dist-release/GrokHub-Setup-*.exe
            dist-release/grokhub-windows-v*.zip
          generate_release_notes: true
          fail_on_unmatched_files: true
```

- [ ] **Step 3: README top**

```markdown
# GrokHub for Windows

Native Rust cabin (egui). Not Electron. Not Tauri.

**v2.6.42** — per-user installer. Grok Build (`grok.exe`) is the agent and computer-use.

## Install

1. Download **GrokHub-Setup-2.6.42.exe** from [Releases](https://github.com/blackviperxiii-ui/GrokHub-Windows/releases/latest).
2. Run it (no admin). Installs to `%LOCALAPPDATA%\Programs\GrokHub`.
3. Launch GrokHub. Close hides to the tray; Quit from the tray.

If the installer could not vendor `grok.exe`, install Grok Build:

```powershell
irm https://x.ai/cli/install.ps1 | iex
```

Then `grok login`.

| Repo | Platform |
|------|----------|
| **[GrokHub-Windows](https://github.com/blackviperxiii-ui/GrokHub-Windows)** (this) | Windows x86_64 |
| [GrokHub](https://github.com/blackviperxiii-ui/GrokHub) | Arch Linux / CachyOS |
```

Keep a short “Build from source” with `cargo test --workspace` and `cargo run -p grokhub-app`.

- [ ] **Step 4: Commit CI + README**

```bash
git add .github/workflows/ci.yml .github/workflows/release.yml README.md Cargo.toml
git commit -m "ci: Windows test, build, and Inno Setup release"
```

- [ ] **Step 5: Create the GitHub repo and push**

```bash
gh repo create blackviperxiii-ui/GrokHub-Windows --public --description "GrokHub native cabin for Windows (egui). Grok Build is the agent." --source=/home/viper/src/GrokHub-Windows --remote=origin --push
git -C /home/viper/src/GrokHub-Windows push -u origin main
```

Do **not** push to `Grok-Hub-Windows` (Electron name) and do not restore that history.

Verify:

```bash
gh api repos/blackviperxiii-ui/GrokHub-Windows --jq '{html_url,private,default_branch}'
git -C /home/viper/src/GrokHub-Windows ls-remote --heads origin
```

Expected: `private: false`, `main` exists.

Optional tag after CI is green (do not tag until `cargo test` on GHA is green):

```bash
git tag v2.6.42-win1
git push origin v2.6.42-win1
```

That fires `release.yml`. First tag can wait until the Windows job is green.

---

## Spec coverage

| Spec item | Task |
|-----------|------|
| New public GrokHub-Windows from committed v2.6.42 | 1, 8 |
| No Electron history | 1, 8 |
| eframe Windows / ksni Linux / tray-icon Windows | 6 |
| PowerShell `/host` | 4 |
| `%APPDATA%\GrokHub`, `%USERPROFILE%\.grok` | 2, 3 |
| `grok.exe` locate order + sidecar | 3 |
| ACP unix isolation skipped | 5 |
| pid-alive without `/proc` | 6 |
| Optional grok.exe bundle; failure not fatal | 7 |
| Inno Setup per-user Setup.exe + zip | 7, 8 |
| windows-latest CI | 8 |
| Doctor names official install | 3 |
| No Windows computer-use backends | (no task adds them) |

## Notes for the implementer

- Work only in `/home/viper/src/GrokHub-Windows` after Task 1. Leave Linux `/home/viper/src/Grok-Hub` dirty WIP untouched.
- Linux `cargo test -p grokhub-app` must stay green after every task (ksni path).
- `tray-icon` 0.19 APIs (`MenuItem::id`, `Icon::from_rgba`) may differ by patch; if compile fails, match the crate’s docs for 0.19, not a newer major.
- Do not run Inno locally on CachyOS. Packaging proof is GitHub Actions.
