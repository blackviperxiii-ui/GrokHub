//! `grokhub-hub --doctor`: the cabin's auth, memory and hub checks, for
//! whichever app is installed. Auth counts the cabin's `secrets.json` and the
//! shell's keychain entries. Nothing here writes a secret or prints one.

use grokhub_core::{doctor_extras, doctor_lines, doctor_ok, hub_kind_from_health, DoctorLine};
use std::path::Path;
use std::time::Duration;

/// The shell's keychain accounts (`src-tauri/src/credential.rs` and
/// `grokhub_agent::xai_keychain`), under the service `grokhub_core::brand::INTERNAL_ID`.
pub const SHELL_KEY_ACCOUNT: &str = "shell-console-key";
pub const SHELL_SIGN_IN_ACCOUNT: &str = "shell-xai-oauth";

/// What the checks found. Gathered by [`gather`]; [`report`] only formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub authed: bool,
    pub memory_ok: bool,
    /// The body of `GET /v1/health` on the local hub, if it answered.
    pub hub_health: Option<String>,
    pub skills: usize,
    /// How this machine confines model-written commands (informational).
    pub sandbox: String,
}

/// The lines `--doctor` prints and whether all of them are ok.
pub fn report(facts: &Facts) -> (String, bool) {
    let kind = hub_kind_from_health(facts.hub_health.as_deref());
    let mut lines: Vec<DoctorLine> = doctor_lines(facts.authed, facts.memory_ok, &kind);
    lines.extend(doctor_extras(None, facts.skills));
    let mut out = String::new();
    for l in &lines {
        out.push_str(if l.ok { "ok  " } else { "ERR " });
        out.push_str(&l.text);
        out.push('\n');
    }
    out.push_str("    ");
    out.push_str(&facts.sandbox);
    out.push('\n');
    (out, doctor_ok(&lines))
}

/// A console key or Grok sign-in the cabin saved in `secrets.json` (or a
/// leftover key in `app.json`).
pub fn cabin_auth(dir: &Path) -> bool {
    let read = |name: &str| -> serde_json::Value {
        std::fs::read_to_string(dir.join(name))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or(serde_json::Value::Null)
    };
    let filled = |v: Option<&serde_json::Value>| {
        v.and_then(|s| s.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    };
    let secrets = read("secrets.json");
    filled(secrets.get("apiKey"))
        || filled(secrets.get("oauth").and_then(|o| o.get("accessToken")))
        || filled(read("app.json").get("apiKey"))
}

/// The shell's console key or Grok sign-in in the system keychain. A
/// keychain that can't be reached counts as no entry.
fn shell_auth() -> bool {
    [SHELL_KEY_ACCOUNT, SHELL_SIGN_IN_ACCOUNT]
        .iter()
        .any(|account| {
            keyring::Entry::new(grokhub_core::brand::INTERNAL_ID, account)
                .and_then(|e| e.get_password())
                .is_ok_and(|v| !v.trim().is_empty())
        })
}

/// Skill folders with a `SKILL.md` that aren't switched off.
pub fn skill_count(dir: &Path) -> usize {
    let skills = dir.join("skills");
    let Ok(rd) = std::fs::read_dir(&skills) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| e.path().join("SKILL.md").is_file())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            !grokhub_agent::skill_is_off_in(&skills, &name)
        })
        .count()
}

/// `GET /v1/health` on this computer's hub, 400 ms in all. Loopback only,
/// through the egress guard as the cabin's probe (it allows loopback and
/// logs nothing).
fn hub_health(dir: &Path, port: u16) -> Option<String> {
    use grokhub_agent::harness as hx;
    let url = format!("http://127.0.0.1:{port}/v1/health");
    hx::guard_quiet(dir, &hx::EgressReq::new(&url, &[])).ok()?;
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        overall: Some(Duration::from_millis(400)),
        ..Default::default()
    });
    agent
        .get(&url)
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
}

pub fn gather(dir: &Path, port: u16) -> Facts {
    Facts {
        // The cabin file first, so the keychain is only asked when needed.
        authed: cabin_auth(dir) || shell_auth(),
        memory_ok: std::fs::create_dir_all(dir.join("memory")).is_ok(),
        hub_health: hub_health(dir, port),
        skills: skill_count(dir),
        sandbox: grokhub_agent::sandbox::status(dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hub-doctor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn no_auth_and_no_hub_are_errs() {
        let (text, ok) = report(&Facts {
            authed: false,
            memory_ok: true,
            hub_health: None,
            skills: 0,
            sandbox: "Shell sandbox: off (shell_sandbox.toml)".into(),
        });
        assert!(!ok);
        assert_eq!(
            text,
            "ERR xAI auth missing — Connect Grok OAuth in Settings\n\
             ok  memory dir readable\n\
             ERR hub kind unreachable\n\
             ok  no host receipt yet\n\
             ok  0 skills on disk\n    \
             Shell sandbox: off (shell_sandbox.toml)\n"
        );
    }

    #[test]
    fn all_ok_with_a_live_hub() {
        let health = format!(r#"{{"ok":true,"kind":"{}"}}"#, grokhub_core::HUB_KIND);
        let (text, ok) = report(&Facts {
            authed: true,
            memory_ok: true,
            hub_health: Some(health),
            skills: 2,
            sandbox: "sb".into(),
        });
        assert!(ok, "{text}");
        assert!(text.starts_with("ok  xAI auth present\n"), "{text}");
        assert!(
            text.contains(&format!("ok  hub kind {}\n", grokhub_core::HUB_KIND)),
            "{text}"
        );
        assert!(text.contains("ok  2 skills on disk\n"), "{text}");
    }

    #[test]
    fn cabin_auth_reads_secrets_and_the_leftover_key() {
        let d = temp("auth");
        assert!(!cabin_auth(&d));
        std::fs::write(d.join("secrets.json"), r#"{"apiKey":"  "}"#).unwrap();
        assert!(!cabin_auth(&d));
        std::fs::write(d.join("secrets.json"), r#"{"oauth":{"accessToken":"tok"}}"#).unwrap();
        assert!(cabin_auth(&d));
        std::fs::write(d.join("secrets.json"), "{}").unwrap();
        std::fs::write(d.join("app.json"), r#"{"apiKey":"xai-1"}"#).unwrap();
        assert!(cabin_auth(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn skills_count_folders_with_a_skill_md() {
        let d = temp("skills");
        assert_eq!(skill_count(&d), 0);
        for name in ["a", "b"] {
            std::fs::create_dir_all(d.join("skills").join(name)).unwrap();
            std::fs::write(d.join("skills").join(name).join("SKILL.md"), "# x").unwrap();
        }
        std::fs::create_dir_all(d.join("skills/empty")).unwrap();
        assert_eq!(skill_count(&d), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn keychain_accounts_match_the_shell() {
        let cred = include_str!("../../../src-tauri/src/credential.rs");
        assert!(cred.contains(&format!(
            "pub const ACCOUNT: &str = \"{SHELL_KEY_ACCOUNT}\";"
        )));
        assert!(cred.contains("pub const SERVICE: &str = grokhub_core::brand::INTERNAL_ID;"));
        assert_eq!(grokhub_agent::xai_keychain::ACCOUNT, SHELL_SIGN_IN_ACCOUNT);
    }
}
