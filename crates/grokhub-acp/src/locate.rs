use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// Resolve the Grok Build CLI. `GROKHUB_GROK` wins, then PATH, then common install dirs.
pub fn find_grok() -> Option<PathBuf> {
    let key = format!(
        "{:?}|{:?}",
        std::env::var_os("GROKHUB_GROK"),
        std::env::var_os("PATH")
    );
    if let Ok(held) = grok_bin_cache().lock() {
        if let Some((k, at, path, inflight)) = held.as_ref() {
            if *k == key {
                let hit = path.clone();
                let fresh = at.elapsed() < Duration::from_secs(2);
                let busy = *inflight;
                drop(held);
                if !fresh && !busy {
                    kick_find_grok(key);
                }
                return hit;
            }
        }
    }
    let path = find_grok_scan();
    if let Ok(mut held) = grok_bin_cache().lock() {
        *held = Some((key, Instant::now(), path.clone(), false));
    }
    path
}

fn grok_bin_cache() -> &'static Mutex<Option<(String, Instant, Option<PathBuf>, bool)>> {
    static C: OnceLock<Mutex<Option<(String, Instant, Option<PathBuf>, bool)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

fn find_grok_scan() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("GROKHUB_GROK") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(p) = which("grok") {
        return Some(p);
    }
    let home = std::env::var("HOME").ok()?;
    for rel in ["/.local/bin/grok", "/.grok/bin/grok"] {
        let p = PathBuf::from(format!("{home}{rel}"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn kick_find_grok(key: String) {
    if let Ok(mut held) = grok_bin_cache().lock() {
        if let Some(slot) = held.as_mut() {
            if slot.0 == key {
                if slot.3 {
                    return;
                }
                slot.3 = true;
            }
        }
    }
    thread::spawn(move || {
        let path = find_grok_scan();
        if let Ok(mut held) = grok_bin_cache().lock() {
            *held = Some((key, Instant::now(), path, false));
        }
    });
}

pub fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn grok_home() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".grok"))
}

pub fn grok_auth_path() -> Option<PathBuf> {
    Some(grok_home()?.join("auth.json"))
}

/// Cached `grok login` bearer from `~/.grok/auth.json`. Never logs the secret.
pub fn grok_cli_key() -> Option<String> {
    let path = grok_auth_path()?;
    if let Ok(held) = grok_key_cache().lock() {
        if let Some((p, _, at, key, inflight)) = held.as_ref() {
            if *p == path {
                let hit = key.clone();
                let fresh = at.elapsed() < Duration::from_secs(2);
                let busy = *inflight;
                drop(held);
                if !fresh && !busy {
                    kick_grok_cli_key(path);
                }
                return hit;
            }
        }
    }
    let (modified, key) = grok_cli_key_now(&path);
    if let Ok(mut held) = grok_key_cache().lock() {
        *held = Some((path, modified, Instant::now(), key.clone(), false));
    }
    key
}

fn grok_key_cache() -> &'static Mutex<Option<(PathBuf, Option<std::time::SystemTime>, Instant, Option<String>, bool)>> {
    static C: OnceLock<Mutex<Option<(PathBuf, Option<std::time::SystemTime>, Instant, Option<String>, bool)>>> =
        OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

fn grok_cli_key_now(path: &Path) -> (Option<std::time::SystemTime>, Option<String>) {
    let modified = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    let raw = read_file_capped(path, 64 * 1024);
    (modified, parse_grok_auth_key(&raw))
}

fn kick_grok_cli_key(path: PathBuf) {
    if let Ok(mut held) = grok_key_cache().lock() {
        if let Some(slot) = held.as_mut() {
            if slot.0 == path {
                if slot.4 {
                    return;
                }
                slot.4 = true;
            }
        }
    }
    thread::spawn(move || {
        let (modified, key) = grok_cli_key_now(&path);
        if let Ok(mut held) = grok_key_cache().lock() {
            *held = Some((path, modified, Instant::now(), key, false));
        }
    });
}

fn read_file_capped(path: &Path, cap: usize) -> String {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut buf = vec![0u8; cap];
    let n = match std::io::Read::read(&mut f, &mut buf) {
        Ok(n) => n,
        Err(_) => return String::new(),
    };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

pub fn parse_grok_auth_key(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if let Some(key) = grok_key_from_value(&v) {
        return Some(key);
    }
    let obj = v.as_object()?;
    let mut best: Option<(String, String)> = None;
    for rec in obj.values() {
        let Some(key) = grok_key_from_value(rec) else {
            continue;
        };
        let exp = rec
            .get("expires_at")
            .and_then(|x| x.as_str())
            .or_else(|| rec.get("expiresAt").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string();
        let take = match &best {
            None => true,
            Some((prev, _)) => exp > *prev,
        };
        if take {
            best = Some((exp, key));
        }
    }
    best.map(|(_, k)| k)
}

fn grok_key_from_value(v: &serde_json::Value) -> Option<String> {
    for field in ["key", "access_token", "accessToken", "token"] {
        if let Some(k) = v.get(field).and_then(|x| x.as_str()).map(str::trim) {
            if !k.is_empty() {
                return Some(k.to_string());
            }
        }
    }
    None
}

pub fn grok_version(bin: &Path) -> Result<String, String> {
    let cwd = std::env::temp_dir();
    let text = grok_stdout_timeout(bin, &cwd, &["--version"], 3)?;
    let line = text.lines().next().unwrap_or(text.trim()).trim();
    if line.is_empty() {
        return Err("grok --version empty".into());
    }
    Ok(line.to_string())
}

fn doctor_line_cache() -> &'static Mutex<Option<(Option<PathBuf>, Instant, bool, String, bool)>> {
    static C: OnceLock<Mutex<Option<(Option<PathBuf>, Instant, bool, String, bool)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

/// True while a background `grok --version` is in flight.
pub fn doctor_line_busy() -> bool {
    doctor_line_cache()
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|c| c.4))
        .unwrap_or(false)
}

pub fn doctor_grok_line(bin: Option<&Path>) -> (bool, String) {
    if bin.is_none() {
        return (false, "Grok Build CLI missing — install from x.ai/cli".into());
    }
    if let Ok(held) = doctor_line_cache().lock() {
        if let Some((path, at, ok, text, inflight)) = held.as_ref() {
            if path.as_deref() == bin {
                if *inflight || at.elapsed() < Duration::from_secs(8) {
                    return (*ok, text.clone());
                }
            }
        }
    }
    let path = bin.map(|p| p.to_path_buf());
    let last = if let Ok(mut held) = doctor_line_cache().lock() {
        let last = match held.as_ref() {
            Some((p, _, ok, text, _)) if p == &path => (*ok, text.clone()),
            _ => (true, "Grok Build CLI".into()),
        };
        *held = Some((path.clone(), Instant::now(), last.0, last.1.clone(), true));
        last
    } else {
        (true, "Grok Build CLI".into())
    };
    thread::spawn(move || {
        let (ok, text) = match &path {
            None => (false, "Grok Build CLI missing — install from x.ai/cli".into()),
            Some(p) => match grok_version(p) {
                Ok(v) => (true, format!("Grok Build {v}")),
                Err(e) => (false, format!("Grok Build present but unreadable: {e}")),
            },
        };
        if let Ok(mut held) = doctor_line_cache().lock() {
            *held = Some((path, Instant::now(), ok, text, false));
        }
    });
    last
}

pub fn grok_stdout(bin: &Path, cwd: &Path, args: &[&str]) -> Result<String, String> {
    grok_stdout_timeout(bin, cwd, args, 60)
}

/// Run `grok` and cap how long we wait so History cannot freeze the cabin.
pub fn grok_stdout_timeout(bin: &Path, cwd: &Path, args: &[&str], secs: u64) -> Result<String, String> {
    let bin = bin.to_path_buf();
    let cwd = cwd.to_path_buf();
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let child = Command::new(&bin)
        .args(&owned)
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let out = child.wait_with_output();
        let _ = tx.send(out);
    });
    let out = match rx.recv_timeout(Duration::from_secs(secs.max(1))) {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(e.to_string()),
        Err(_) => {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
            thread::sleep(Duration::from_millis(80));
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
            return Err(format!("grok {} timed out", args.join(" ")));
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() && stdout.is_empty() {
        return Err(if stderr.is_empty() {
            format!("grok {} failed", args.join(" "))
        } else {
            stderr
        });
    }
    if stdout.is_empty() {
        Ok(stderr)
    } else {
        Ok(stdout)
    }
}

pub fn agent_args(always_approve: bool) -> Vec<String> {
    let mut a = vec!["--no-auto-update".into(), "agent".into()];
    if always_approve {
        a.push("--always-approve".into());
    }
    a.push("stdio".into());
    a
}

pub fn agent_args_resume(always_approve: bool, resume: Option<&str>) -> Vec<String> {
    let _ = resume;
    agent_args(always_approve)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_missing_is_none() {
        let prev = std::env::var_os("GROKHUB_GROK");
        std::env::set_var("GROKHUB_GROK", "/no/such/grok-binary-xyz");
        let hit = find_grok();
        if let Some(p) = prev {
            std::env::set_var("GROKHUB_GROK", p);
        } else {
            std::env::remove_var("GROKHUB_GROK");
        }
        if let Some(p) = hit {
            assert_ne!(p, PathBuf::from("/no/such/grok-binary-xyz"));
        }
    }

    #[test]
    fn doctor_missing() {
        let (ok, text) = doctor_grok_line(None);
        assert!(!ok);
        assert!(text.contains("x.ai/cli"));
        let src = include_str!("locate.rs");
        let ver = src
            .split("pub fn grok_version(")
            .nth(1)
            .and_then(|s| s.split("pub fn doctor_grok_line(").next())
            .expect("grok_version");
        assert!(
            ver.contains("grok_stdout_timeout"),
            "grok --version must not hang the settings overlay: {ver}"
        );
        let doc = src
            .split("pub fn doctor_grok_line(")
            .nth(1)
            .and_then(|s| s.split("pub fn grok_stdout(").next())
            .expect("doctor_grok_line");
        assert!(
            doc.contains("elapsed"),
            "Settings must not spawn grok --version every frame: {doc}"
        );
        assert!(
            doc.contains("thread::spawn") && doc.contains("inflight"),
            "Settings must not freeze on grok --version: {doc}"
        );
        let find = src
            .split("pub fn find_grok(")
            .nth(1)
            .and_then(|s| s.split("pub fn which(").next())
            .expect("find_grok");
        assert!(
            find.contains("elapsed"),
            "the composer must not walk PATH every frame: {find}"
        );
        assert!(
            find.contains("thread::spawn") && find.contains("inflight"),
            "a stale grok PATH cache must refresh off the UI thread: {find}"
        );
        let key = src
            .split("pub fn grok_cli_key(")
            .nth(1)
            .and_then(|s| s.split("pub fn parse_grok_auth_key(").next())
            .expect("grok_cli_key");
        assert!(
            key.contains("read_file_capped") && key.contains("elapsed") && !key.contains("read_to_string"),
            "grok login must not slurp auth.json every paint: {key}"
        );
        assert!(
            key.contains("thread::spawn") && key.contains("inflight"),
            "stale grok login must refresh auth.json off the UI thread: {key}"
        );
    }

    #[test]
    fn agent_args_yolo() {
        assert_eq!(
            agent_args(true),
            vec!["--no-auto-update", "agent", "--always-approve", "stdio"]
        );
        assert_eq!(
            agent_args(false),
            vec!["--no-auto-update", "agent", "stdio"]
        );
        assert_eq!(
            agent_args_resume(false, Some("abc-123")),
            vec!["--no-auto-update", "agent", "stdio"]
        );
        assert!(
            !agent_args_resume(true, Some("abc-123"))
                .iter()
                .any(|a| a == "--resume"),
            "CLI --resume plus session/new mixed sessions"
        );
    }

    #[test]
    fn grok_auth_key_picks_the_login_token() {
        let raw = r#"{
            "https://auth.x.ai::one": {
                "auth_mode": "oidc",
                "expires_at": "2026-01-01T00:00:00Z",
                "key": "old-token"
            },
            "https://auth.x.ai::two": {
                "auth_mode": "oidc",
                "expires_at": "2026-12-01T00:00:00Z",
                "key": "fresh-token"
            }
        }"#;
        assert_eq!(parse_grok_auth_key(raw).as_deref(), Some("fresh-token"));
        assert!(parse_grok_auth_key("{}").is_none());
        assert!(parse_grok_auth_key("not-json").is_none());
        assert_eq!(
            parse_grok_auth_key(r#"{"access_token":"top-level"}"#).as_deref(),
            Some("top-level")
        );
    }
}