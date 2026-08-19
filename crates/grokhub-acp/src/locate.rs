use std::path::{Path, PathBuf};

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
    let out = std::process::Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| e.to_string())?;
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
    let mut a = vec![
        "--no-auto-update".into(),
        "agent".into(),
    ];
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
        // find_grok still walks PATH after a bad override? Plan: GROKHUB_GROK wins only if the file exists.
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
    }
}
