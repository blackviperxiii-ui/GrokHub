//! Imagine's own xAI sign-in. Tokens stay in the OS keychain, never a file.
//! The cabin OAuth flow in `oauth` is a different scope and is not used here.

use grokhub_core::OAuthAccountStore;
use grokhub_core::{
    imagine_authorize_url, imagine_code_form, imagine_device_form, imagine_device_poll_form,
    imagine_needs_refresh, imagine_refresh_form, imagine_tokens_from_xai, keychain_unavailable_message,
    merge_imagine_refresh, next_oauth_poll_secs, parse_device_start, parse_imagine_discovery,
    parse_loopback_callback, parse_poll_result, parse_token_json, pkce_challenge, pkce_verifier,
    ImagineAuthorize, ImagineEndpoints, ImagineTokens, PollStatus, XAI_OAUTH_DISCOVERY,
    IMAGINE_SIGN_IN_AGAIN, TEXT_FILE_CAP,
};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

const UA: &str = concat!("GrokHub/", env!("CARGO_PKG_VERSION"), " (xAI OAuth; Linux)");
const SERVICE: &str = "GrokHub";
const LOOPBACK_SECS: u64 = 300;
const READ_CAP: usize = 16 * 1024;

pub trait TokenStore {
    fn load(&self) -> Result<Option<ImagineTokens>, String>;
    fn save(&self, tokens: &ImagineTokens) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

pub struct KeyringStore;

struct KeyringAccounts;

fn keyring_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account).map_err(|e| keychain_unavailable_message(&e.to_string()))
}

impl grokhub_core::OAuthAccountStore for KeyringAccounts {
    fn load_account(&self, account: &str) -> Result<Option<ImagineTokens>, String> {
        let entry = keyring_entry(account)?;
        match entry.get_password() {
            Ok(raw) => Ok(Some(serde_json::from_str(&raw).map_err(|e| e.to_string())?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_unavailable_message(&e.to_string())),
        }
    }

    fn save_account(&self, account: &str, tokens: &ImagineTokens) -> Result<(), String> {
        let entry = keyring_entry(account)?;
        let raw = serde_json::to_string(tokens).map_err(|e| e.to_string())?;
        entry
            .set_password(&raw)
            .map_err(|e| keychain_unavailable_message(&e.to_string()))
    }

    fn delete_account(&self, account: &str) -> Result<(), String> {
        let entry = keyring_entry(account)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain_unavailable_message(&e.to_string())),
        }
    }
}

impl TokenStore for KeyringStore {
    fn load(&self) -> Result<Option<ImagineTokens>, String> {
        grokhub_core::load_xai_oauth(&KeyringAccounts)
    }

    fn save(&self, tokens: &ImagineTokens) -> Result<(), String> {
        KeyringAccounts.save_account(grokhub_core::XAI_OAUTH_ACCOUNT, tokens)
    }

    fn delete(&self) -> Result<(), String> {
        grokhub_core::delete_xai_oauth(&KeyringAccounts)
    }
}

/// Test double. Holds the JSON value in memory and never touches the disk.
#[cfg(test)]
pub struct MemoryTokenStore {
    inner: Mutex<Option<ImagineTokens>>,
}

#[cfg(test)]
impl MemoryTokenStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

#[cfg(test)]
impl TokenStore for MemoryTokenStore {
    fn load(&self) -> Result<Option<ImagineTokens>, String> {
        Ok(self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone())
    }

    fn save(&self, tokens: &ImagineTokens) -> Result<(), String> {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = Some(tokens.clone());
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
}

pub enum ImagineAuthEvent {
    Loaded(Result<Option<ImagineTokens>, String>),
    SignedIn(ImagineTokens),
    DeviceReady {
        user_code: String,
        verify_uri: String,
        device_code: String,
        interval: u64,
    },
    Failed(String),
    SignedOut,
}

pub enum DevicePoll {
    Wait { secs: u64 },
    Ready(ImagineTokens),
    Stop(String),
}

enum PkceStep {
    Tokens(ImagineTokens),
    NeedDevice,
    Failed(String),
}

fn read_json_capped(resp: ureq::Response) -> Result<Value, String> {
    let mut buf = Vec::new();
    resp.into_reader()
        .take(TEXT_FILE_CAP as u64)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

fn post_form(url: &str, body: &str) -> Result<(bool, Value), String> {
    let resp = ureq::post(url)
        .set("content-type", "application/x-www-form-urlencoded")
        .set("accept", "application/json")
        .set("user-agent", UA)
        .timeout(Duration::from_secs(20))
        .send_string(body);
    match resp {
        Ok(r) => Ok((true, read_json_capped(r)?)),
        Err(ureq::Error::Status(_code, r)) => Ok((false, read_json_capped(r).unwrap_or(Value::Null))),
        Err(e) => Err(e.to_string()),
    }
}

fn imagine_discovery() -> Result<ImagineEndpoints, String> {
    let resp = ureq::get(XAI_OAUTH_DISCOVERY)
        .set("accept", "application/json")
        .set("user-agent", UA)
        .timeout(Duration::from_secs(20))
        .call()
        .map_err(|e| e.to_string())?;
    parse_imagine_discovery(&read_json_capped(resp)?)
}

fn form_error(v: &Value, fallback: &str) -> String {
    v.get("error_description")
        .or_else(|| v.get("error"))
        .and_then(|x| x.as_str())
        .unwrap_or(fallback)
        .to_string()
}

fn enrich(mut tokens: ImagineTokens) -> ImagineTokens {
    let missing = tokens
        .email
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty();
    if missing {
        if let Ok(profile) = crate::oauth::fetch_userinfo(&tokens.access_token) {
            if let Some(email) = profile.email.filter(|s| !s.trim().is_empty()) {
                tokens.email = Some(email);
            }
        }
    }
    tokens
}

fn exchange(
    ends: &ImagineEndpoints,
    code: &str,
    verifier: &str,
    redirect: &str,
) -> Result<ImagineTokens, String> {
    let (ok, v) = post_form(&ends.token, &imagine_code_form(code, verifier, redirect))?;
    if !ok {
        return Err(form_error(&v, "code exchange failed"));
    }
    let parsed = parse_token_json(&v, grokhub_core::now_ms())?;
    let tokens = enrich(imagine_tokens_from_xai(&parsed));
    KeyringStore.save(&tokens)?;
    Ok(tokens)
}

fn close_page() -> Vec<u8> {
    let body = b"<p>You can close this tab.</p>";
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut out = header.into_bytes();
    out.extend_from_slice(body);
    out
}

fn pkce_blocking() -> PkceStep {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(_) => return PkceStep::NeedDevice,
    };
    if listener.set_nonblocking(true).is_err() {
        return PkceStep::NeedDevice;
    }
    let port = match listener.local_addr() {
        Ok(addr) => addr.port(),
        Err(_) => return PkceStep::NeedDevice,
    };
    let ends = match imagine_discovery() {
        Ok(ends) => ends,
        Err(e) => return PkceStep::Failed(e),
    };
    let verifier = pkce_verifier();
    let challenge = pkce_challenge(&verifier);
    let state = grokhub_core::oauth_nonce();
    let nonce = grokhub_core::oauth_nonce();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let url = imagine_authorize_url(&ImagineAuthorize {
        authorization_endpoint: &ends.authorization,
        redirect_uri: &redirect,
        code_challenge: &challenge,
        state: &state,
        nonce: &nonce,
    });
    if crate::oauth::open_browser(&url).is_err() {
        return PkceStep::NeedDevice;
    }
    let deadline = Instant::now() + Duration::from_secs(LOOPBACK_SECS);
    // Answer stray requests (a favicon, a prefetch) and keep waiting for /callback.
    let code = loop {
        if Instant::now() >= deadline {
            return PkceStep::Failed("Imagine sign-in timed out".into());
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(40));
                continue;
            }
            Err(e) => return PkceStep::Failed(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; READ_CAP];
        let n = stream.read(&mut buf).unwrap_or(0).min(READ_CAP);
        let head = String::from_utf8_lossy(&buf[..n]);
        if !head.starts_with("GET /callback") {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            continue;
        }
        let _ = stream.write_all(&close_page());
        match parse_loopback_callback(&head, &state) {
            Ok(code) => break code,
            Err(e) => return PkceStep::Failed(e),
        }
    };
    match exchange(&ends, &code, &verifier, &redirect) {
        Ok(tokens) => PkceStep::Tokens(tokens),
        Err(e) => PkceStep::Failed(e),
    }
}

fn device_start() -> Result<ImagineAuthEvent, String> {
    let ends = imagine_discovery()?;
    let (ok, v) = post_form(&ends.device, &imagine_device_form())?;
    if !ok {
        return Err(form_error(&v, "device code failed"));
    }
    let start = parse_device_start(&v)?;
    let verify = start
        .verification_uri_complete
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| start.verification_uri.clone());
    let _ = crate::oauth::open_browser(&verify);
    let show = if start.verification_uri.trim().is_empty() {
        verify
    } else {
        start.verification_uri.clone()
    };
    Ok(ImagineAuthEvent::DeviceReady {
        user_code: start.user_code,
        verify_uri: show,
        device_code: start.device_code,
        interval: start.interval.max(1),
    })
}

pub fn begin_load() -> mpsc::Receiver<ImagineAuthEvent> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(ImagineAuthEvent::Loaded(KeyringStore.load()));
    });
    rx
}

pub fn begin_pkce() -> mpsc::Receiver<ImagineAuthEvent> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let ev = match pkce_blocking() {
            PkceStep::Tokens(tokens) => ImagineAuthEvent::SignedIn(tokens),
            PkceStep::NeedDevice => match device_start() {
                Ok(ev) => ev,
                Err(e) => ImagineAuthEvent::Failed(e),
            },
            PkceStep::Failed(e) => ImagineAuthEvent::Failed(e),
        };
        let _ = tx.send(ev);
    });
    rx
}

pub fn begin_device() -> mpsc::Receiver<ImagineAuthEvent> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let ev = match device_start() {
            Ok(ev) => ev,
            Err(e) => ImagineAuthEvent::Failed(e),
        };
        let _ = tx.send(ev);
    });
    rx
}

pub fn begin_sign_out() -> mpsc::Receiver<ImagineAuthEvent> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let ev = match KeyringStore.delete() {
            Ok(()) => ImagineAuthEvent::SignedOut,
            Err(e) => ImagineAuthEvent::Failed(e),
        };
        let _ = tx.send(ev);
    });
    rx
}

/// One device-code poll. `slow_down` adds five seconds via `next_oauth_poll_secs`.
pub fn poll_device_once(device_code: &str, interval: u64) -> Result<DevicePoll, String> {
    let ends = imagine_discovery()?;
    let (ok, v) = post_form(&ends.token, &imagine_device_poll_form(device_code))?;
    let parsed = parse_poll_result(ok, &v, grokhub_core::now_ms());
    match parsed.status {
        PollStatus::Ready => {
            let xai = parsed
                .tokens
                .ok_or_else(|| "Imagine sign-in returned no token".to_string())?;
            let tokens = enrich(imagine_tokens_from_xai(&xai));
            KeyringStore.save(&tokens)?;
            Ok(DevicePoll::Ready(tokens))
        }
        PollStatus::Pending | PollStatus::SlowDown => {
            let secs = next_oauth_poll_secs(interval, parsed.status).unwrap_or(interval.max(1));
            Ok(DevicePoll::Wait { secs })
        }
        PollStatus::Expired => Ok(DevicePoll::Stop(
            "The sign-in code expired. Sign in again.".into(),
        )),
        PollStatus::Denied => Ok(DevicePoll::Stop(
            parsed
                .error
                .unwrap_or_else(|| "Imagine sign-in was denied".into()),
        )),
    }
}

/// Refresh when the access token is inside the 30 minute window. A failed refresh
/// leaves the stored token alone and reports [`IMAGINE_SIGN_IN_AGAIN`].
pub fn access_for_job(tokens: &ImagineTokens) -> Result<(String, Option<ImagineTokens>), String> {
    if !imagine_needs_refresh(tokens, grokhub_core::now_ms()) {
        return Ok((tokens.access_token.clone(), None));
    }
    // One refresh at a time: a wall paint and a user job holding the same refresh
    // token must not both spend it, or the loser is told to sign in again.
    let _gate = REFRESH_GATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(done) = refreshed_from(tokens) {
        return Ok((done.access_token.clone(), Some(done)));
    }
    let refresh = tokens
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(IMAGINE_SIGN_IN_AGAIN)?;
    let ends = imagine_discovery().map_err(|_| IMAGINE_SIGN_IN_AGAIN.to_string())?;
    let (ok, v) = post_form(&ends.token, &imagine_refresh_form(refresh))
        .map_err(|_| IMAGINE_SIGN_IN_AGAIN.to_string())?;
    if !ok {
        return Err(IMAGINE_SIGN_IN_AGAIN.into());
    }
    let next = parse_token_json(&v, grokhub_core::now_ms())
        .map_err(|_| IMAGINE_SIGN_IN_AGAIN.to_string())?;
    let merged = merge_imagine_refresh(tokens, imagine_tokens_from_xai(&next));
    KeyringStore.save(&merged)?;
    note_refreshed(tokens, &merged);
    Ok((merged.access_token.clone(), Some(merged)))
}

/// The last refresh a job made, kept so the UI picks it up even when the job
/// itself fails, and so a second job holding the old refresh token reuses it.
struct Refreshed {
    from_refresh: Option<String>,
    tokens: ImagineTokens,
    unseen: bool,
}

static REFRESH_GATE: Mutex<()> = Mutex::new(());
static REFRESHED: Mutex<Option<Refreshed>> = Mutex::new(None);

fn note_refreshed(old: &ImagineTokens, merged: &ImagineTokens) {
    *REFRESHED.lock().unwrap_or_else(|e| e.into_inner()) = Some(Refreshed {
        from_refresh: old.refresh_token.clone(),
        tokens: merged.clone(),
        unseen: true,
    });
}

fn refreshed_from(old: &ImagineTokens) -> Option<ImagineTokens> {
    let slot = REFRESHED.lock().unwrap_or_else(|e| e.into_inner());
    let r = slot.as_ref()?;
    (r.from_refresh.is_some()
        && r.from_refresh == old.refresh_token
        && !imagine_needs_refresh(&r.tokens, grokhub_core::now_ms()))
    .then(|| r.tokens.clone())
}

/// Tokens a background job refreshed that the UI has not taken yet, only when
/// they came from the refresh token the UI holds now (not an earlier account).
/// Never blocks.
pub fn take_refreshed(current_refresh: Option<&str>) -> Option<ImagineTokens> {
    let mut slot = REFRESHED.try_lock().ok()?;
    let r = slot
        .as_mut()
        .filter(|r| r.unseen && r.from_refresh.is_some() && r.from_refresh.as_deref() == current_refresh)?;
    r.unseen = false;
    Some(r.tokens.clone())
}

/// Forget any refreshed tokens, so a sign-out or a new sign-in is not undone by a job's leftovers.
pub fn forget_refreshed() {
    *REFRESHED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// One source image per pick, through the cabin's own picker (no extra GUI toolkit).
pub fn pick_image_data_uris(max: usize) -> Result<Vec<String>, String> {
    if max == 0 {
        return Ok(Vec::new());
    }
    let Some(path) = crate::desktop::pick_file() else {
        return Ok(Vec::new());
    };
    let buf = std::fs::read(&path).map_err(|e| e.to_string())?;
    Ok(vec![grokhub_core::image_bytes_data_uri(&buf)?])
}

pub fn save_copy_dialog(src: &str) -> Result<String, String> {
    let Some(dest) = crate::desktop::save_file_dialog(src) else {
        return Err("Save cancelled".into());
    };
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::copy(src, &dest).map_err(|e| e.to_string())?;
    Ok(dest.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imagine_memory_store_round_trip_writes_no_file() {
        let dir = std::env::temp_dir().join(format!("imagine-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryTokenStore::new();
        let tokens = ImagineTokens {
            access_token: "secret-access".into(),
            refresh_token: Some("secret-refresh".into()),
            id_token: Some("secret-id".into()),
            email: Some("ada@example.com".into()),
            expires_at: Some(90_000),
            connected_at: 10,
        };
        store.save(&tokens).unwrap();
        let back = store.load().unwrap().expect("stored");
        assert_eq!(back, tokens);
        let dbg = format!("{tokens:?}");
        assert!(!dbg.contains("secret-access"), "{dbg}");
        assert!(!dbg.contains("secret-refresh"), "{dbg}");
        assert!(!dbg.contains("secret-id"), "{dbg}");
        assert!(dbg.contains("redacted"), "{dbg}");
        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());
        assert!(
            std::fs::read_dir(&dir).unwrap().next().is_none(),
            "the mock store must not create a file"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let src = include_str!("imagine_auth.rs");
        assert!(!src.contains(concat!("File::", "create")));
        assert!(!src.contains(concat!("secrets", ".json")));
        for banned in [
            concat!("grok_cli_", "key"),
            concat!("auth", ".json"),
            concat!("refresh_grok_", "login"),
            concat!("cli-chat-", "proxy"),
        ] {
            assert!(!src.contains(banned), "{banned}");
        }
        let page = String::from_utf8(close_page()).unwrap();
        assert!(page.contains("You can close this tab"), "{page}");
    }
}
