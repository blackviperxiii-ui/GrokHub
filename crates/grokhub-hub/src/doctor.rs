//! `grokhub-hub --doctor [--fix]`: the cabin's auth, memory and hub checks,
//! plus config/key perms, TLS/remote bind exposure, broken MCP connector
//! commands, updater signature readiness and sandbox availability. Auth
//! counts the cabin's `secrets.json` and the shell's keychain entries.
//! Nothing here writes a secret or prints one. `--fix` only applies safe
//! permission repairs (0600 files / 0700 dirs on Unix; user-only ACL on
//! Windows).

use grokhub_core::{doctor_extras, doctor_lines, doctor_ok, hub_kind_from_health, DoctorLine};
use std::path::Path;
use std::time::Duration;

/// The shell's keychain accounts (`src-tauri/src/credential.rs` and
/// `grokhub_agent::xai_keychain`), under the service `grokhub_core::brand::INTERNAL_ID`.
pub const SHELL_KEY_ACCOUNT: &str = "shell-console-key";
pub const SHELL_SIGN_IN_ACCOUNT: &str = "shell-xai-oauth";

/// Secret files that must stay user-only (0600 / Windows user-only DACL).
const SECRET_FILES: &[&str] = &[
    "secrets.json",
    "hub-state.json",
    "code-secrets.json",
    "web-access/key.pem",
];

/// What the checks found. Gathered by [`gather`]; [`report`] only formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub authed: bool,
    /// A provider key or local model can answer without xAI.
    pub other_ai: bool,
    pub memory_ok: bool,
    /// The body of `GET /v1/health` on the local hub, if it answered.
    pub hub_health: Option<String>,
    pub skills: usize,
    /// How this machine confines model-written commands (informational).
    pub sandbox: String,
    /// Config dir and secret-file permission findings.
    pub perms: Vec<DoctorLine>,
    /// Hub LAN bind and web-access TLS readiness.
    pub remote: Vec<DoctorLine>,
    /// Stdio MCP connectors whose command cannot be resolved.
    pub mcp: Vec<DoctorLine>,
    /// Whether the release minisign public key is baked in.
    pub updater: DoctorLine,
}

/// The lines `--doctor` prints and whether all of them are ok.
pub fn report(facts: &Facts) -> (String, bool) {
    let kind = hub_kind_from_health(facts.hub_health.as_deref());
    let mut lines: Vec<DoctorLine> =
        doctor_lines(facts.authed, facts.other_ai, facts.memory_ok, &kind);
    lines.extend(doctor_extras(None, facts.skills));
    lines.extend(facts.perms.iter().cloned());
    lines.extend(facts.remote.iter().cloned());
    lines.extend(facts.mcp.iter().cloned());
    lines.push(facts.updater.clone());
    lines.push(DoctorLine {
        ok: sandbox_line_ok(&facts.sandbox),
        text: facts.sandbox.clone(),
    });
    let mut out = String::new();
    for l in &lines {
        out.push_str(if l.ok { "ok  " } else { "ERR " });
        out.push_str(&l.text);
        out.push('\n');
    }
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
/// keychain that can't be reached counts as no entry. Sign-in may be split
/// across numbered entries when the JSON exceeds Windows' blob limit.
fn shell_auth() -> bool {
    fn map_err(_: keyring::Error) -> String {
        String::new()
    }
    let backend =
        grokhub_agent::keyring_blob::OsBackend::new(grokhub_core::brand::INTERNAL_ID, map_err);
    [SHELL_KEY_ACCOUNT, SHELL_SIGN_IN_ACCOUNT]
        .iter()
        .any(|account| {
            grokhub_agent::keyring_blob::get(&backend, account)
                .ok()
                .flatten()
                .is_some_and(|v| !v.trim().is_empty())
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

/// Gather every check. When `fix` is true, apply safe permission repairs
/// first, then re-read.
pub fn gather(dir: &Path, port: u16, fix: bool) -> Facts {
    if fix {
        let _ = apply_safe_fixes(dir);
    }
    Facts {
        // The cabin file first, so the keychain is only asked when needed.
        authed: cabin_auth(dir) || shell_auth(),
        other_ai: other_ai(dir),
        memory_ok: std::fs::create_dir_all(dir.join("memory")).is_ok(),
        hub_health: hub_health(dir, port),
        skills: skill_count(dir),
        sandbox: grokhub_agent::sandbox::status(dir),
        perms: perm_lines(dir),
        remote: remote_lines(dir),
        mcp: mcp_lines(dir),
        updater: updater_line(),
    }
}

/// A provider key the user allowed, or a model on this PC — enough to answer
/// without xAI (same sources as the shell's `ai_ready`, minus xAI itself).
fn other_ai(dir: &Path) -> bool {
    grokhub_agent::route::local::installed()
        || !grokhub_agent::route::providers::usable(
            dir,
            grokhub_agent::route::providers::call_data(false),
        )
        .is_empty()
}

/// Safe repairs only: tighten the config dir and known secret files. Never
/// deletes, never rewrites JSON, never touches the keychain.
pub fn apply_safe_fixes(dir: &Path) -> Vec<String> {
    let mut done = Vec::new();
    if let Some(msg) = tighten_dir(dir) {
        done.push(msg);
    }
    for rel in SECRET_FILES {
        let path = dir.join(rel);
        if path.is_file()
            && let Some(msg) = tighten_file(&path)
        {
            done.push(msg);
        }
    }
    done
}

fn perm_lines(dir: &Path) -> Vec<DoctorLine> {
    let mut out = Vec::new();
    out.push(dir_perm_line(dir));
    for rel in SECRET_FILES {
        let path = dir.join(rel);
        if path.is_file() {
            out.push(file_perm_line(&path, rel));
        }
    }
    out
}

fn dir_perm_line(dir: &Path) -> DoctorLine {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(dir) {
            Ok(meta) => {
                let mode = meta.permissions().mode() & 0o777;
                if mode == 0o700 {
                    DoctorLine {
                        ok: true,
                        text: "config dir mode 0700".into(),
                    }
                } else {
                    DoctorLine {
                        ok: false,
                        text: format!(
                            "config dir mode {mode:04o} — want 0700 (grokhub-hub --doctor --fix)"
                        ),
                    }
                }
            }
            Err(e) => DoctorLine {
                ok: false,
                text: format!("config dir unreadable: {e}"),
            },
        }
    }
    #[cfg(windows)]
    {
        let _ = dir;
        DoctorLine {
            ok: true,
            text: "config dir perms (Windows user ACL)".into(),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = dir;
        DoctorLine {
            ok: true,
            text: "config dir perms skipped on this OS".into(),
        }
    }
}

fn file_perm_line(path: &Path, rel: &str) -> DoctorLine {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(meta) => {
                let mode = meta.permissions().mode() & 0o777;
                if mode == 0o600 {
                    DoctorLine {
                        ok: true,
                        text: format!("{rel} mode 0600"),
                    }
                } else {
                    DoctorLine {
                        ok: false,
                        text: format!(
                            "{rel} mode {mode:04o} — want 0600 (grokhub-hub --doctor --fix)"
                        ),
                    }
                }
            }
            Err(e) => DoctorLine {
                ok: false,
                text: format!("{rel} unreadable: {e}"),
            },
        }
    }
    #[cfg(windows)]
    {
        if grokhub_core::win_acl::is_user_only(path) {
            DoctorLine {
                ok: true,
                text: format!("{rel} user-only ACL"),
            }
        } else {
            DoctorLine {
                ok: false,
                text: format!("{rel} world-readable ACL — run grokhub-hub --doctor --fix"),
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        DoctorLine {
            ok: true,
            text: format!("{rel} perms skipped on this OS"),
        }
    }
}

fn tighten_dir(dir: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(dir).ok()?;
        let mode = meta.permissions().mode() & 0o777;
        if mode == 0o700 {
            return None;
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).ok()?;
        Some(format!("fixed {} to 0700", dir.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        None
    }
}

fn tighten_file(path: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path).ok()?;
        let mode = meta.permissions().mode() & 0o777;
        if mode == 0o600 {
            return None;
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).ok()?;
        Some(format!("fixed {} to 0600", path.display()))
    }
    #[cfg(windows)]
    {
        if grokhub_core::win_acl::is_user_only(path) {
            return None;
        }
        grokhub_core::win_acl::restrict_user_only(path);
        Some(format!("fixed {} to user-only ACL", path.display()))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        None
    }
}

fn remote_lines(dir: &Path) -> Vec<DoctorLine> {
    let mut out = Vec::new();
    // The hub listens on 0.0.0.0 by design; pairing is the gate.
    out.push(DoctorLine {
        ok: true,
        text: "hub binds 0.0.0.0 (LAN); pairing is the gate".into(),
    });
    let cert = dir.join("web-access/cert.pem");
    let key = dir.join("web-access/key.pem");
    match (cert.is_file(), key.is_file()) {
        (true, true) => out.push(DoctorLine {
            ok: true,
            text: "web-access TLS cert and key present".into(),
        }),
        (false, false) => out.push(DoctorLine {
            ok: true,
            text: "web-access TLS not configured yet".into(),
        }),
        (true, false) => out.push(DoctorLine {
            ok: false,
            text: "web-access cert.pem without key.pem — regenerate Web access TLS".into(),
        }),
        (false, true) => out.push(DoctorLine {
            ok: false,
            text: "web-access key.pem without cert.pem — regenerate Web access TLS".into(),
        }),
    }
    out
}

fn mcp_lines(dir: &Path) -> Vec<DoctorLine> {
    let broken = broken_mcp_commands(dir);
    if broken.is_empty() {
        return vec![DoctorLine {
            ok: true,
            text: "MCP connector commands resolve".into(),
        }];
    }
    broken
        .into_iter()
        .map(|(name, cmd)| DoctorLine {
            ok: false,
            text: format!("MCP connector `{name}` command missing: {cmd}"),
        })
        .collect()
}

/// Stdio MCP entries in `mcp.json` whose program is not on PATH and not an
/// existing absolute path.
pub fn broken_mcp_commands(dir: &Path) -> Vec<(String, String)> {
    let text = match std::fs::read_to_string(dir.join("mcp.json")) {
        Ok(t) if !t.trim().is_empty() => t,
        _ => return Vec::new(),
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(servers) = doc
        .get("mcpServers")
        .or_else(|| doc.get("mcp_servers"))
        .and_then(|v| v.as_object())
    else {
        return Vec::new();
    };
    let mut broken = Vec::new();
    for (name, spec) in servers {
        let Some(obj) = spec.as_object() else {
            continue;
        };
        let kind = obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let http = obj.get("url").and_then(|v| v.as_str()).is_some_and(|u| {
            let t = u.trim();
            t.starts_with("http://") || t.starts_with("https://")
        });
        if http || kind == "http" || kind == "sse" {
            continue;
        }
        let Some(cmd) = stdio_command(obj) else {
            continue;
        };
        if !command_resolves(&cmd) {
            broken.push((name.clone(), cmd));
        }
    }
    broken
}

fn stdio_command(spec: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    match spec.get("command")? {
        serde_json::Value::String(s) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
        serde_json::Value::Array(parts) => parts
            .first()
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

fn command_resolves(cmd: &str) -> bool {
    let path = Path::new(cmd);
    if path.is_absolute() || cmd.contains('/') || cmd.contains('\\') {
        return path.is_file();
    }
    #[cfg(windows)]
    let path_env = std::env::var("PATH").unwrap_or_default().replace(';', ":");
    #[cfg(not(windows))]
    let path_env = std::env::var("PATH").unwrap_or_default();
    let home = grokhub_core::user_home()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extras = grokhub_core::default_bin_extra_dirs(&home);
    let extra_refs: Vec<&str> = extras.iter().map(String::as_str).collect();
    grokhub_core::bin_on_path(cmd, &path_env, &extra_refs)
}

fn updater_line() -> DoctorLine {
    match grokhub_core::release_verify::RELEASE_PUBKEY {
        Some(pk) if !pk.trim().is_empty() => DoctorLine {
            ok: true,
            text: "updater signature key present (minisign)".into(),
        },
        _ => DoctorLine {
            ok: false,
            text: "updater signature key missing — release verifies will fail".into(),
        },
    }
}

fn sandbox_line_ok(status: &str) -> bool {
    // "off" is a deliberate choice, still ok. Missing confinement tools are
    // reported in the status text but are not a hard doctor failure unless
    // the line says the sandbox is broken.
    !status.to_ascii_lowercase().contains("broken")
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

    fn empty_facts() -> Facts {
        Facts {
            authed: false,
            other_ai: false,
            memory_ok: true,
            hub_health: None,
            skills: 0,
            sandbox: "Shell sandbox: off (shell_sandbox.toml)".into(),
            perms: vec![DoctorLine {
                ok: true,
                text: "config dir mode 0700".into(),
            }],
            remote: vec![
                DoctorLine {
                    ok: true,
                    text: "hub binds 0.0.0.0 (LAN); pairing is the gate".into(),
                },
                DoctorLine {
                    ok: true,
                    text: "web-access TLS not configured yet".into(),
                },
            ],
            mcp: vec![DoctorLine {
                ok: true,
                text: "MCP connector commands resolve".into(),
            }],
            updater: DoctorLine {
                ok: true,
                text: "updater signature key present (minisign)".into(),
            },
        }
    }

    #[test]
    fn other_ai_without_xai_is_ok_on_the_auth_row() {
        let mut facts = empty_facts();
        facts.other_ai = true;
        facts.hub_health = Some(format!(
            r#"{{"ok":true,"kind":"{}"}}"#,
            grokhub_core::HUB_KIND
        ));
        let (text, ok) = report(&facts);
        assert!(ok, "{text}");
        assert!(
            text.contains("ok  xAI auth missing — another AI source can answer"),
            "{text}"
        );
    }

    #[test]
    fn no_auth_and_no_hub_are_errs() {
        let (text, ok) = report(&empty_facts());
        assert!(!ok);
        assert!(text.contains("ERR xAI auth missing"), "{text}");
        assert!(text.contains("ERR hub kind unreachable"), "{text}");
        assert!(text.contains("ok  no host receipt yet"), "{text}");
        assert!(text.contains("ok  0 skills on disk"), "{text}");
        assert!(
            text.contains("Shell sandbox: off (shell_sandbox.toml)"),
            "{text}"
        );
    }

    #[test]
    fn all_ok_with_a_live_hub() {
        let health = format!(r#"{{"ok":true,"kind":"{}"}}"#, grokhub_core::HUB_KIND);
        let mut facts = empty_facts();
        facts.authed = true;
        facts.hub_health = Some(health);
        facts.skills = 2;
        facts.sandbox = "sb".into();
        let (text, ok) = report(&facts);
        assert!(ok, "{text}");
        assert!(text.starts_with("ok  xAI auth present\n"), "{text}");
        assert!(
            text.contains(&format!("ok  hub kind {}\n", grokhub_core::HUB_KIND)),
            "{text}"
        );
        assert!(text.contains("ok  2 skills on disk\n"), "{text}");
        assert!(text.contains("ok  updater signature key present"), "{text}");
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
        assert!((grokhub_agent::xai_keychain::ACCOUNT) == (SHELL_SIGN_IN_ACCOUNT));
    }

    #[test]
    fn broken_mcp_flags_a_missing_stdio_command() {
        let d = temp("mcp-broken");
        std::fs::write(
            d.join("mcp.json"),
            r#"{"mcpServers":{"weather":{"command":"definitely-not-a-real-mcp-binary-966","args":["--stdio"]}}}"#,
        )
        .unwrap();
        let broken = broken_mcp_commands(&d);
        assert_eq!(broken.len(), 1, "{broken:?}");
        assert_eq!(broken[0].0, "weather");
        assert_eq!(broken[0].1, "definitely-not-a-real-mcp-binary-966");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn broken_mcp_ignores_http_connectors() {
        let d = temp("mcp-http");
        std::fs::write(
            d.join("mcp.json"),
            r#"{"mcpServers":{"remote":{"url":"https://example.com/mcp"}}}"#,
        )
        .unwrap();
        assert!(broken_mcp_commands(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn doctor_fix_tightens_secret_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("fix-perms");
        let secrets = d.join("secrets.json");
        std::fs::write(&secrets, "{}").unwrap();
        std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();

        let before = perm_lines(&d);
        assert!(
            before
                .iter()
                .any(|l| !l.ok && l.text.contains("secrets.json")),
            "{before:?}"
        );

        let fixed = apply_safe_fixes(&d);
        assert!(fixed.iter().any(|m| m.contains("0600")), "fixes={fixed:?}");
        let mode = std::fs::metadata(&secrets).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(&d).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);

        let after = perm_lines(&d);
        assert!(
            after.iter().all(|l| l.ok),
            "still failing after --fix: {after:?}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn remote_lines_flag_a_lone_key() {
        let d = temp("tls-key-only");
        std::fs::create_dir_all(d.join("web-access")).unwrap();
        std::fs::write(d.join("web-access/key.pem"), "k").unwrap();
        let lines = remote_lines(&d);
        assert!(
            lines
                .iter()
                .any(|l| !l.ok && l.text.contains("key.pem without cert")),
            "{lines:?}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn updater_line_sees_the_release_key() {
        let line = updater_line();
        assert!(line.ok, "{line:?}");
        assert!(line.text.contains("minisign"), "{line:?}");
    }
}
