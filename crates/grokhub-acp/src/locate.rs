use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Resolve the Grok Build CLI. `GROKHUB_GROK` wins, then PATH, then common install dirs.
pub fn find_grok() -> Option<PathBuf> {
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
    let raw = std::fs::read_to_string(path).ok()?;
    parse_grok_auth_key(&raw)
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
    let out = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        text = String::from_utf8_lossy(&out.stderr).trim().to_string();
    }
    if text.is_empty() {
        return Err("grok --version empty".into());
    }
    Ok(text.lines().next().unwrap_or(&text).to_string())
}

pub fn doctor_grok_line(bin: Option<&Path>) -> (bool, String) {
    match bin {
        None => (false, "Grok Build CLI missing — install from x.ai/cli".into()),
        Some(p) => match grok_version(p) {
            Ok(v) => (true, format!("Grok Build {v}")),
            Err(e) => (false, format!("Grok Build present but unreadable: {e}")),
        },
    }
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
        let mut child = child;
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
    agent_args_resume(always_approve, None)
}

pub fn agent_args_resume(always_approve: bool, resume: Option<&str>) -> Vec<String> {
    let mut a = vec!["--no-auto-update".into()];
    if let Some(id) = resume {
        let id = id.trim();
        if !id.is_empty() {
            a.push("--resume".into());
            a.push(id.to_string());
        }
    }
    a.push("agent".into());
    if always_approve {
        a.push("--always-approve".into());
    }
    a.push("stdio".into());
    a
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
            vec!["--no-auto-update", "--resume", "abc-123", "agent", "stdio"]
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