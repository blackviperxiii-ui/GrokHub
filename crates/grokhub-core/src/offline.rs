//! Offline only (LU-13): one switch that keeps the app off the internet.
//!
//! Off by default. While it is on, every outbound call the app owns refuses a
//! host that isn't this computer or the user's own network, with
//! [`OFFLINE_MSG`], and the on-device model takes the chat.
//!
//! - Allowed while on: loopback (`127.0.0.0/8`, `::1`, `localhost`) for the
//!   local model server and the app's own desktop MCP / hub, and private LAN
//!   addresses (RFC 1918, link-local, IPv6 ULA, `.local` / `.lan` /
//!   `.home.arpa` / `.internal` names) for a printer or a paired computer.
//!   Carrier-grade NAT (`100.64.0.0/10`) is not a LAN: it is refused.
//! - The switch lives in `{config}/offline.toml` (`offline_only = true`) and is
//!   read once per process. `GROKHUB_OFFLINE_ONLY=1` forces it on (tests, CLI).
//! - Callers check with [`check_url`] / [`check_host`] (or [`refuse`] for a step
//!   with no URL, like `git fetch`). A refusal is final: never retried.

use std::cell::Cell;
use std::fmt;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// File name under the config dir.
pub const OFFLINE_FILE: &str = "offline.toml";
/// `1` (or `true`) forces Offline only on for this process.
pub const OFFLINE_ENV: &str = "GROKHUB_OFFLINE_ONLY";

/// What every refused call says (chat, MCP rows, downloads, updates).
pub static OFFLINE_MSG: LazyLock<&'static str> = LazyLock::new(|| {
    crate::brand::filled(
        "Offline only is on, so {product} can't reach the internet. Turn it off in Settings → AI models to use this.",
    )
});

const UNLOADED: u8 = 0;
const OFF: u8 = 1;
const ON: u8 = 2;
static STATE: AtomicU8 = AtomicU8::new(UNLOADED);

thread_local! {
    static FORCED: Cell<Option<bool>> = const { Cell::new(None) };
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OfflineFile {
    #[serde(default)]
    offline_only: bool,
}

/// A call Offline only refused. Its text is [`OFFLINE_MSG`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineError {
    /// The host that was refused (for logs; the message doesn't name it).
    pub host: String,
}

impl fmt::Display for OfflineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&OFFLINE_MSG)
    }
}

impl std::error::Error for OfflineError {}

impl From<OfflineError> for String {
    fn from(e: OfflineError) -> Self {
        e.to_string()
    }
}

fn env_forced() -> bool {
    std::env::var(OFFLINE_ENV)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on"))
        .unwrap_or(false)
}

/// The config dir this switch lives in: `GROKHUB_CONFIG`, else the platform
/// default (the same folder the cabin and the agent use).
pub fn config_dir() -> PathBuf {
    if let Ok(raw) = std::env::var("GROKHUB_CONFIG") {
        let raw = raw.trim();
        if !raw.is_empty() {
            return PathBuf::from(raw);
        }
    }
    if cfg!(windows) {
        if let Ok(app) = std::env::var("APPDATA") {
            if !app.trim().is_empty() {
                return PathBuf::from(app).join("GrokHub");
            }
        }
    }
    match crate::user_home() {
        Some(home) => home.join(".config").join("GrokHub"),
        None => PathBuf::from(".grokhub"),
    }
}

/// What `{dir}/offline.toml` says. A missing or broken file is off.
pub fn read_from(dir: &Path) -> bool {
    fs::read_to_string(dir.join(OFFLINE_FILE))
        .ok()
        .and_then(|text| toml::from_str::<OfflineFile>(&text).ok())
        .is_some_and(|f| f.offline_only)
}

/// Offline only is on for this process.
pub fn enabled() -> bool {
    if let Some(on) = FORCED.with(Cell::get) {
        return on;
    }
    if env_forced() {
        return true;
    }
    match STATE.load(Ordering::SeqCst) {
        ON => true,
        OFF => false,
        _ => load_from(&config_dir()),
    }
}

/// Read the switch from `dir` and cache it (a process whose config dir isn't
/// the default one calls this once at start).
pub fn load_from(dir: &Path) -> bool {
    let on = read_from(dir);
    STATE.store(if on { ON } else { OFF }, Ordering::SeqCst);
    on
}

/// Turn Offline only on or off in the default config dir.
pub fn set(on: bool) -> Result<(), String> {
    set_in(&config_dir(), on)
}

/// Save the switch to `{dir}/offline.toml`, then apply it to this process.
/// Turning it off while `GROKHUB_OFFLINE_ONLY=1` is set is refused.
pub fn set_in(dir: &Path, on: bool) -> Result<(), String> {
    if !on && env_forced() {
        return Err(format!(
            "{OFFLINE_ENV}=1 keeps Offline only on for this run. Unset it, then try again."
        ));
    }
    fs::create_dir_all(dir).map_err(|e| format!("Couldn't save Offline only: {e}"))?;
    let text = toml::to_string(&OfflineFile { offline_only: on })
        .map_err(|e| format!("Couldn't save Offline only: {e}"))?;
    let path = dir.join(OFFLINE_FILE);
    let tmp = dir.join(format!("{OFFLINE_FILE}.tmp"));
    fs::write(&tmp, text)
        .and_then(|()| fs::rename(&tmp, &path))
        .map_err(|e| format!("Couldn't save Offline only: {e}"))?;
    STATE.store(if on { ON } else { OFF }, Ordering::SeqCst);
    Ok(())
}

/// Force the switch for this thread until the guard drops (tests, a one-shot
/// CLI run). Other threads keep the saved value.
pub struct ForceGuard {
    prev: Option<bool>,
}

pub fn force_for_thread(on: bool) -> ForceGuard {
    ForceGuard {
        prev: FORCED.with(|f| f.replace(Some(on))),
    }
}

impl Drop for ForceGuard {
    fn drop(&mut self) {
        FORCED.with(|f| f.set(self.prev));
    }
}

/// This computer or the user's own network: loopback, RFC 1918, link-local,
/// IPv6 ULA, and LAN-only names. Anything else is the internet.
pub fn host_is_local(host: &str) -> bool {
    let host = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip_is_local(ip);
    }
    host == "localhost"
        || [".localhost", ".local", ".lan", ".home.arpa", ".internal"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
}

/// Loopback or a private LAN address (not CGNAT, not public).
pub fn ip_is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return ip_is_local(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                // fc00::/7 unique local
                || (first & 0xfe00) == 0xfc00
                // fe80::/10 link-local
                || (first & 0xffc0) == 0xfe80
        }
    }
}

/// The host part of a URL (`scheme://user@host:port/path`), or the input
/// itself when it has no scheme. `None` for a URL that names no host
/// (`file:///…`, `data:`).
pub fn url_host(url: &str) -> Option<String> {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once("://") else {
        let lower = url.to_ascii_lowercase();
        if ["data:", "mailto:", "about:", "blob:", "file:"]
            .iter()
            .any(|p| lower.starts_with(p))
        {
            return None;
        }
        return Some(authority_host(
            url.split(['/', '?', '#']).next().unwrap_or(""),
        ));
    };
    if scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    // WHATWG URLs end the authority at `\` too.
    let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or("");
    Some(authority_host(
        authority.rsplit('@').next().unwrap_or(authority),
    ))
}

fn authority_host(authority: &str) -> String {
    if authority.parse::<IpAddr>().is_ok() {
        return authority.to_string();
    }
    if let Some(v6) = authority.strip_prefix('[') {
        return v6.split(']').next().unwrap_or("").to_string();
    }
    authority
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(authority)
        .to_string()
}

/// Refuse `host` while Offline only is on, unless it is [`host_is_local`].
pub fn check_host(host: &str) -> Result<(), OfflineError> {
    if !enabled() || host_is_local(host) {
        return Ok(());
    }
    Err(OfflineError {
        host: host.trim().to_ascii_lowercase(),
    })
}

/// Refuse a URL whose host is the internet while Offline only is on. A URL
/// with no host to dial (`file:`) passes.
pub fn check_url(url: &str) -> Result<(), OfflineError> {
    if !enabled() {
        return Ok(());
    }
    match url_host(url) {
        None => Ok(()),
        Some(host) => check_host(&host),
    }
}

/// For a step that reaches a remote with no URL in hand (`git fetch`, an
/// update plan, a model download): refuse while Offline only is on.
pub fn refuse() -> Result<(), String> {
    if enabled() {
        Err(OFFLINE_MSG.to_string())
    } else {
        Ok(())
    }
}

/// This error text is an Offline only refusal (so retry code stops at once).
pub fn is_refusal(text: &str) -> bool {
    text.contains(*OFFLINE_MSG)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MSG: &str = "Offline only is on, so Loadout can't reach the internet. Turn it off in Settings → AI models to use this.";

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-offline-{tag}-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_message_names_the_product_and_where_to_turn_it_off() {
        assert_eq!(*OFFLINE_MSG, MSG);
        assert_eq!(
            OfflineError {
                host: "api.x.ai".into()
            }
            .to_string(),
            MSG
        );
        assert!(is_refusal(&format!("not sent: {MSG}")));
        assert!(!is_refusal("connection refused"));
    }

    #[test]
    fn loopback_and_lan_are_local_and_the_internet_is_not() {
        for host in [
            "127.0.0.1",
            "127.4.5.6",
            "localhost",
            "LOCALHOST.",
            "app.localhost",
            "[::1]",
            "::1",
            "192.168.1.20",
            "10.0.0.7",
            "172.16.4.4",
            "169.254.10.1",
            "fd12:3456::1",
            "fe80::1",
            "::ffff:192.168.1.2",
            "octopi.local",
            "nas.lan",
            "printer.home.arpa",
            "hub.internal",
        ] {
            assert!(host_is_local(host), "{host}");
        }
        for host in [
            "api.x.ai",
            "openrouter.ai",
            "huggingface.co",
            "8.8.8.8",
            "100.64.0.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
            "localhost.evil.com",
            "",
        ] {
            assert!(!host_is_local(host), "{host}");
        }
    }

    #[test]
    fn url_host_reads_the_authority_like_a_browser() {
        assert_eq!(
            url_host("https://api.x.ai/v1/responses").as_deref(),
            Some("api.x.ai")
        );
        assert_eq!(
            url_host("http://user:pw@127.0.0.1:8080/x").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(url_host("http://[::1]:9/").as_deref(), Some("::1"));
        assert_eq!(
            url_host("http://evil.com\\@127.0.0.1/").as_deref(),
            Some("evil.com")
        );
        assert_eq!(
            url_host("wss://Example.org").as_deref(),
            Some("Example.org")
        );
        assert_eq!(url_host("file:///home/user/a.txt"), None);
        assert_eq!(url_host("data:text/plain,hi"), None);
        assert_eq!(url_host("api.x.ai").as_deref(), Some("api.x.ai"));
        assert_eq!(
            url_host("evil.example:443").as_deref(),
            Some("evil.example")
        );
        assert_eq!(url_host("::1").as_deref(), Some("::1"));
    }

    #[test]
    fn while_on_only_local_hosts_pass() {
        let _on = force_for_thread(true);
        assert!(enabled());
        assert_eq!(
            check_url("https://api.x.ai/v1/responses"),
            Err(OfflineError {
                host: "api.x.ai".into()
            })
        );
        assert_eq!(
            check_url("http://evil.com\\@127.0.0.1/").map_err(|e| e.host),
            Err("evil.com".to_string())
        );
        assert_eq!(check_url("http://127.0.0.1:8080/health"), Ok(()));
        assert_eq!(check_url("http://192.168.1.50/api/printer"), Ok(()));
        assert_eq!(check_url("file:///home/user/model.gguf"), Ok(()));
        assert_eq!(check_host("localhost"), Ok(()));
        assert_eq!(refuse(), Err(MSG.to_string()));
        {
            let _off = force_for_thread(false);
            assert_eq!(check_url("https://api.x.ai/v1/responses"), Ok(()));
            assert_eq!(refuse(), Ok(()));
        }
        assert!(enabled(), "the inner guard restores the outer one");
    }

    #[test]
    fn the_switch_round_trips_through_offline_toml() {
        let dir = temp_dir("roundtrip");
        assert!(!read_from(&dir), "no file is off");
        set_in(&dir, true).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(OFFLINE_FILE)).unwrap(),
            "offline_only = true\n"
        );
        assert!(read_from(&dir));
        assert!(load_from(&dir));
        set_in(&dir, false).unwrap();
        assert!(!read_from(&dir));
        assert!(!load_from(&dir));
        fs::write(dir.join(OFFLINE_FILE), "offline_only = [").unwrap();
        assert!(!read_from(&dir), "a broken file is off");
        let _ = fs::remove_dir_all(dir);
    }
}
