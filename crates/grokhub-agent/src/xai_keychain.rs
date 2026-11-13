//! Sign in with Grok (device code), so a Grok subscriber can chat without an
//! xAI API key. The calls go to xAI's auth host only, with codes and tokens and
//! no chats or memory, so each one passes the quiet egress guard (Offline only
//! refuses it). Tokens live in the system keychain, never in a file, and never
//! in the Grok CLI's or the cabin's files. The desktop shell and
//! `grokhub-hub --oauth` share this one store.
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;

use crate::harness as hx;
use grokhub_core::http;
use grokhub_core::{
    apply_profile, merge_refreshed, next_oauth_poll_secs, parse_device_start, parse_poll_result,
    parse_token_json, parse_userinfo_profile, token_needs_refresh, trusted_xai_url, PollStatus,
    XaiOAuthTokens, TEXT_FILE_CAP, XAI_DEVICE_CODE_GRANT, XAI_OAUTH_CLIENT_ID, XAI_OAUTH_DISCOVERY,
    XAI_OAUTH_SCOPE, XAI_OAUTH_USERINFO,
};
use serde::Serialize;
use serde_json::Value;

/// The keychain account for the shell's Grok sign-in.
pub const ACCOUNT: &str = "shell-xai-oauth";

/// Points sign-in at a local mock auth server for end-to-end runs. Only a
/// plain-http loopback URL is taken; anything else is ignored.
pub const TEST_URL_ENV: &str = "GROKHUB_SHELL_OAUTH_URL";

pub const EXPIRED: &str = "Your Grok sign-in ended. Sign in again in Settings → Account.";
pub const NOT_STARTED: &str = "Start Sign in with Grok first.";

/// Where the tokens are kept. The keychain in the app; memory in tests.
pub trait Store: Sync {
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, value: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

/// The system keychain, under the same service as the API key.
pub struct Keychain;

fn keychain_message(err: keyring::Error) -> String {
    format!("The system keychain is not available: {err}")
}

fn backend() -> crate::keyring_blob::OsBackend {
    crate::keyring_blob::OsBackend::new(grokhub_core::brand::INTERNAL_ID, keychain_message)
}

impl Store for Keychain {
    fn get(&self) -> Result<Option<String>, String> {
        crate::keyring_blob::get(&backend(), ACCOUNT)
    }
    fn set(&self, value: &str) -> Result<(), String> {
        crate::keyring_blob::set(&backend(), ACCOUNT, value)
    }
    fn delete(&self) -> Result<(), String> {
        crate::keyring_blob::delete(&backend(), ACCOUNT)
    }
}

/// Who is signed in, for Settings → Account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedIn {
    pub name: Option<String>,
    pub email: Option<String>,
}

/// What Sign in with Grok shows: the code to type at the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub user_code: String,
    pub verification_uri: String,
    /// The page with the code filled in, when xAI gives one.
    pub verification_uri_complete: Option<String>,
    /// Seconds between polls.
    pub interval: u64,
    pub expires_in: u64,
}

/// One poll's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Poll {
    /// Not approved yet; poll again after `interval` seconds.
    Pending {
        interval: u64,
    },
    Ready {
        signed_in: SignedIn,
    },
    /// The code ran out or was refused; start again.
    Failed {
        error: String,
    },
    /// No answer came back this time ([`CallError::NoAnswer`]). The code is
    /// still held, so the same poll can work a moment later.
    NoAnswer {
        error: String,
    },
}

/// Why a sign-in call gave nothing to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// No answer came back: no network, a refused or dropped connection, a
    /// timeout, or a server error in place of xAI's answer. The same call can
    /// work a moment later.
    NoAnswer(String),
    /// Refused before the call, or an answer that can't be read. Asking again
    /// is unlikely to help.
    Other(String),
}

impl From<String> for CallError {
    fn from(text: String) -> Self {
        Self::Other(text)
    }
}

impl From<http::Error> for CallError {
    fn from(err: http::Error) -> Self {
        match err {
            http::Error::Transport { message, .. } => Self::NoAnswer(message),
            http::Error::Status { message, .. } => Self::Other(message),
        }
    }
}

impl From<CallError> for String {
    fn from(err: CallError) -> Self {
        match err {
            CallError::NoAnswer(text) | CallError::Other(text) => text,
        }
    }
}

struct Pending {
    device_code: String,
    interval: u64,
}

/// The sign-in in progress. The device code stays here, never in the UI.
#[derive(Default)]
pub struct SignIn {
    pending: Mutex<Option<Pending>>,
}

impl SignIn {
    /// No sign-in in progress; usable in a `static`.
    pub const fn new() -> Self {
        Self {
            pending: Mutex::new(None),
        }
    }
}

pub(crate) struct Endpoints {
    device: String,
    pub(crate) token: String,
    userinfo: String,
}

#[cfg(test)]
thread_local! {
    static TEST_BASE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// `raw` when it is a plain-http address on this computer. The host is read
/// the way the request reads it: user-info (`@`), `\`, `?` and `#` are refused
/// outright, the host ends at the first `/`, and a port is digits only.
fn loopback(raw: &str) -> Option<String> {
    let raw = raw.trim().trim_end_matches('/');
    let rest = raw.strip_prefix("http://")?;
    if rest.contains(['@', '\\', '?', '#']) {
        return None;
    }
    let authority = rest.split('/').next()?;
    // `[::1]` has colons of its own; its port follows the bracket.
    let (host, port) = match authority.strip_prefix("[::1]") {
        Some("") => ("[::1]", None),
        Some(after) => ("[::1]", Some(after.strip_prefix(':')?)),
        None => match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    (port_ok && matches!(host, "127.0.0.1" | "localhost" | "[::1]")).then(|| raw.to_string())
}

/// The loopback mock base, when one is set.
fn test_base() -> Option<String> {
    #[cfg(test)]
    if let Some(base) = TEST_BASE.with(|b| b.borrow().clone()) {
        return loopback(&base);
    }
    std::env::var(TEST_URL_ENV)
        .ok()
        .and_then(|raw| loopback(&raw))
}

/// Every sign-in call: 20 s for the whole request.
fn agent() -> http::Agent {
    http::agent(http::Opts {
        overall: Some(Duration::from_secs(20)),
        ..Default::default()
    })
}

fn guard(url: &str) -> Result<(), String> {
    hx::guard_quiet(&crate::perm::config_dir(), &hx::EgressReq::new(url, &[]))
}

fn read_json(resp: http::Response) -> Result<Value, CallError> {
    let mut buf = Vec::new();
    resp.into_reader()
        .take(TEXT_FILE_CAP as u64)
        .read_to_end(&mut buf)
        .map_err(|e| CallError::NoAnswer(e.to_string()))?;
    serde_json::from_slice(&buf).map_err(|e| CallError::Other(e.to_string()))
}

fn get_json(url: &str, bearer: Option<&str>) -> Result<Value, CallError> {
    guard(url)?;
    let mut call = agent()
        .get(url)
        .header("accept", "application/json")
        .header("user-agent", crate::USER_AGENT);
    if let Some(token) = bearer {
        call = call.header("authorization", &format!("Bearer {token}"));
    }
    read_json(call.call()?)
}

/// POSTs a form. `false` with the error body when xAI answers with an error.
/// A server error (5xx), or "too many requests" with no error of xAI's own,
/// says nothing about the request, so it is [`CallError::NoAnswer`].
pub(crate) fn post_form(url: &str, pairs: &[(&str, &str)]) -> Result<(bool, Value), CallError> {
    guard(url)?;
    let body = pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&");
    match agent()
        .post(url)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .header("user-agent", crate::USER_AGENT)
        .send_string(&body)
    {
        Ok(resp) => Ok((true, read_json(resp)?)),
        Err(http::Error::Status {
            code,
            body,
            message,
            ..
        }) => {
            let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            if code >= 500 || (code == 429 && v.get("error").is_none()) {
                Err(CallError::NoAnswer(message))
            } else {
                Ok((false, v))
            }
        }
        Err(e) => Err(e.into()),
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn error_text(v: &Value, fallback: &str) -> String {
    v.get("error_description")
        .or_else(|| v.get("error"))
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

/// xAI's endpoints from discovery, each checked to be on xAI's auth host.
pub(crate) fn endpoints() -> Result<Endpoints, CallError> {
    if let Some(base) = test_base() {
        return Ok(Endpoints {
            device: format!("{base}/device"),
            token: format!("{base}/token"),
            userinfo: format!("{base}/userinfo"),
        });
    }
    let v = get_json(XAI_OAUTH_DISCOVERY, None)?;
    let field = |name: &str| {
        v.get(name)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("xAI sign-in is missing its {name}."))
            .and_then(trusted_xai_url)
    };
    Ok(Endpoints {
        device: field("device_authorization_endpoint")?,
        token: field("token_endpoint")?,
        userinfo: trusted_xai_url(XAI_OAUTH_USERINFO)?,
    })
}

fn load(store: &dyn Store) -> Result<Option<XaiOAuthTokens>, String> {
    Ok(store
        .get()?
        .and_then(|raw| serde_json::from_str::<XaiOAuthTokens>(&raw).ok())
        .filter(|t| !t.access_token.trim().is_empty()))
}

fn save(store: &dyn Store, tokens: &XaiOAuthTokens) -> Result<(), String> {
    store.set(&serde_json::to_string(tokens).map_err(|e| e.to_string())?)
}

/// Who is signed in, or `None`. A keychain error reads as signed out.
pub fn signed_in(store: &dyn Store) -> Option<SignedIn> {
    load(store).ok().flatten().map(|t| SignedIn {
        name: t.name,
        email: t.email,
    })
}

impl SignIn {
    /// Asks xAI for a device code. The page and code go to the UI.
    pub fn start(&self) -> Result<Started, String> {
        self.start_with(&[("scope", XAI_OAUTH_SCOPE)])
    }

    /// [`start`](Self::start) asking for `extra` (the scope, and anything
    /// else xAI's device page takes) after the client id.
    pub(crate) fn start_with(&self, extra: &[(&str, &str)]) -> Result<Started, String> {
        let ends = endpoints()?;
        let mut pairs = vec![("client_id", XAI_OAUTH_CLIENT_ID)];
        pairs.extend_from_slice(extra);
        let (ok, v) = post_form(&ends.device, &pairs)?;
        if !ok {
            return Err(error_text(&v, "xAI didn't start the sign-in."));
        }
        let got = parse_device_start(&v)?;
        // Only xAI's own page is shown to open.
        let page = if test_base().is_some() {
            got.verification_uri.clone()
        } else {
            trusted_xai_url(&got.verification_uri)?
        };
        let complete = got
            .verification_uri_complete
            .filter(|u| test_base().is_some() || trusted_xai_url(u).is_ok());
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pending {
            device_code: got.device_code,
            interval: got.interval.max(1),
        });
        Ok(Started {
            user_code: got.user_code,
            verification_uri: page,
            verification_uri_complete: complete,
            interval: got.interval.max(1),
            expires_in: got.expires_in,
        })
    }

    /// Asks once whether the code was approved. On approval the tokens go to
    /// `store`, with the name and email when xAI gives them.
    pub fn poll(&self, store: &dyn Store) -> Result<Poll, String> {
        self.poll_with(&|tokens| save(store, tokens))
    }

    /// [`poll`](Self::poll) handing the approved tokens to `keep`.
    pub(crate) fn poll_with(
        &self,
        keep: &dyn Fn(&XaiOAuthTokens) -> Result<(), String>,
    ) -> Result<Poll, String> {
        match self.ask_with(keep) {
            Err(CallError::NoAnswer(error)) => Ok(Poll::NoAnswer { error }),
            other => Ok(other?),
        }
    }

    /// [`poll`](Self::poll) for a caller that keeps asking by itself: the
    /// error says whether the call got no answer (the code is still good) or
    /// failed some other way.
    pub fn ask(&self, store: &dyn Store) -> Result<Poll, CallError> {
        self.ask_with(&|tokens| save(store, tokens))
    }

    fn ask_with(
        &self,
        keep: &dyn Fn(&XaiOAuthTokens) -> Result<(), String>,
    ) -> Result<Poll, CallError> {
        let (device_code, interval) = {
            let held = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            let p = held.as_ref().ok_or_else(|| NOT_STARTED.to_string())?;
            (p.device_code.clone(), p.interval)
        };
        let ends = endpoints()?;
        let (ok, v) = post_form(
            &ends.token,
            &[
                ("grant_type", XAI_DEVICE_CODE_GRANT),
                ("client_id", XAI_OAUTH_CLIENT_ID),
                ("device_code", &device_code),
            ],
        )?;
        let got = parse_poll_result(ok, &v, grokhub_core::now_ms());
        match got.status {
            PollStatus::Pending | PollStatus::SlowDown => {
                let next = next_oauth_poll_secs(interval, got.status).unwrap_or(interval);
                if let Some(p) = self
                    .pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_mut()
                {
                    p.interval = next;
                }
                Ok(Poll::Pending { interval: next })
            }
            PollStatus::Expired | PollStatus::Denied => {
                self.cancel();
                Ok(Poll::Failed {
                    error: got.error.unwrap_or_else(|| {
                        if got.status == PollStatus::Expired {
                            "The code ran out. Start again.".into()
                        } else {
                            "The sign-in was refused.".into()
                        }
                    }),
                })
            }
            PollStatus::Ready => {
                let mut tokens = got
                    .tokens
                    .ok_or_else(|| "xAI said yes but sent no tokens.".to_string())?;
                // The name and email are a nicety; sign-in works without them.
                if let Ok(info) = get_json(&ends.userinfo, Some(&tokens.access_token)) {
                    apply_profile(&mut tokens, &parse_userinfo_profile(&info));
                }
                if tokens.connected_at == 0 {
                    tokens.connected_at = grokhub_core::now_ms();
                }
                keep(&tokens)?;
                self.cancel();
                Ok(Poll::Ready {
                    signed_in: SignedIn {
                        name: tokens.name,
                        email: tokens.email,
                    },
                })
            }
        }
    }

    /// Drops a sign-in in progress.
    pub fn cancel(&self) {
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Signs out: the tokens leave the keychain.
pub fn sign_out(store: &dyn Store) -> Result<(), String> {
    store.delete()
}

/// One refresh at a time, so two turns don't both spend the refresh token.
static REFRESH: Mutex<()> = Mutex::new(());

/// The access token for chat, refreshed first when it is about to run out.
/// `None` when nobody signed in with Grok.
pub fn bearer(store: &dyn Store) -> Result<Option<String>, String> {
    let Some(tokens) = load(store)? else {
        return Ok(None);
    };
    if !token_needs_refresh(&tokens, grokhub_core::now_ms()) {
        return Ok(Some(tokens.access_token));
    }
    let _one = REFRESH.lock().unwrap_or_else(|e| e.into_inner());
    // Another turn may have refreshed while this one waited.
    let tokens = load(store)?.unwrap_or(tokens);
    if !token_needs_refresh(&tokens, grokhub_core::now_ms()) {
        return Ok(Some(tokens.access_token));
    }
    let Some(refresh) = tokens
        .refresh_token
        .clone()
        .filter(|t| !t.trim().is_empty())
    else {
        return Err(EXPIRED.into());
    };
    let ends = endpoints()?;
    let (ok, v) = post_form(
        &ends.token,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", XAI_OAUTH_CLIENT_ID),
            ("refresh_token", &refresh),
        ],
    )?;
    if !ok {
        return Err(EXPIRED.into());
    }
    let next = parse_token_json(&v, grokhub_core::now_ms())?;
    let merged = merge_refreshed(&tokens, next);
    save(store, &merged)?;
    Ok(Some(merged.access_token))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    #[derive(Default)]
    pub(crate) struct Mem(pub Mutex<Option<String>>);

    impl Store for Mem {
        fn get(&self) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn set(&self, value: &str) -> Result<(), String> {
            *self.0.lock().unwrap() = Some(value.to_string());
            Ok(())
        }
        fn delete(&self) -> Result<(), String> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    /// A loopback auth server. Each request takes the next answer for its
    /// path; the bodies it got are kept.
    pub(crate) struct Mock {
        pub base: String,
        pub seen: Arc<Mutex<Vec<(String, String)>>>,
    }

    pub(crate) fn mock(answers: Vec<(&'static str, u16, &'static str)>) -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            let mut answers = answers;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                log.lock()
                    .unwrap()
                    .push((path.clone(), String::from_utf8_lossy(&body).into_owned()));
                let at = answers.iter().position(|(p, _, _)| *p == path);
                let (code, text) = match at {
                    Some(i) => {
                        let (_, code, text) = answers.remove(i);
                        (code, text)
                    }
                    None => (404, "{}"),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                    text.len()
                );
                if answers.is_empty() {
                    break;
                }
            }
        });
        Mock { base, seen }
    }

    pub(crate) struct Base(());
    impl Drop for Base {
        fn drop(&mut self) {
            TEST_BASE.with(|b| *b.borrow_mut() = None);
        }
    }
    pub(crate) fn use_base(base: &str) -> Base {
        TEST_BASE.with(|b| *b.borrow_mut() = Some(base.to_string()));
        Base(())
    }

    fn config(name: &str) -> (std::path::PathBuf, crate::perm::ConfigGuard) {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-oauth-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let guard = crate::perm::ConfigGuard::set(&dir);
        (dir, guard)
    }

    const START: &str = r#"{"device_code":"dev-1","user_code":"ABCD-EFGH","verification_uri":"https://accounts.x.ai/device","expires_in":600,"interval":5}"#;
    const PENDING: &str = r#"{"error":"authorization_pending"}"#;
    const TOKENS: &str =
        r#"{"access_token":"at-1","refresh_token":"rt-1","expires_in":3600,"token_type":"Bearer"}"#;
    const USERINFO: &str = r#"{"name":"Sam Rivera","email":"sam@example.com"}"#;

    #[test]
    fn the_device_code_shows_then_pending_then_tokens_are_stored() {
        let (_dir, _cfg) = config("flow");
        let server = mock(vec![
            ("/device", 200, START),
            ("/token", 400, PENDING),
            ("/token", 200, TOKENS),
            ("/userinfo", 200, USERINFO),
        ]);
        let _base = use_base(&server.base);
        let flow = SignIn::default();
        let store = Mem::default();
        let started = flow.start().expect("start");
        assert_eq!(started.user_code, "ABCD-EFGH");
        assert_eq!(started.verification_uri, "https://accounts.x.ai/device");
        assert_eq!(started.interval, 5);
        assert_eq!(flow.poll(&store), Ok(Poll::Pending { interval: 5 }));
        assert_eq!(store.get(), Ok(None));
        assert_eq!(
            flow.poll(&store),
            Ok(Poll::Ready {
                signed_in: SignedIn {
                    name: Some("Sam Rivera".into()),
                    email: Some("sam@example.com".into())
                }
            })
        );
        assert_eq!(bearer(&store), Ok(Some("at-1".into())));
        assert_eq!(
            signed_in(&store).and_then(|s| s.email),
            Some("sam@example.com".into())
        );
        // The device code went to xAI, never to the UI's answer.
        let seen = server.seen.lock().unwrap();
        assert!(seen[1].1.contains("device_code=dev-1"));
        assert!(seen[0].1.contains("client_id="));
        // A second poll needs a new start.
        assert_eq!(flow.poll(&store), Err(NOT_STARTED.into()));
    }

    #[test]
    fn an_expired_token_is_refreshed_before_use_and_saved() {
        let (_dir, _cfg) = config("refresh");
        let server = mock(vec![(
            "/token",
            200,
            r#"{"access_token":"at-2","expires_in":3600}"#,
        )]);
        let _base = use_base(&server.base);
        let store = Mem::default();
        let old = XaiOAuthTokens {
            access_token: "at-old".into(),
            refresh_token: Some("rt-old".into()),
            expires_at: Some(1),
            name: Some("Sam".into()),
            ..Default::default()
        };
        save(&store, &old).unwrap();
        assert_eq!(bearer(&store), Ok(Some("at-2".into())));
        let kept = load(&store).unwrap().unwrap();
        assert_eq!(kept.access_token, "at-2");
        // The refresh token and profile carry over when xAI sends none.
        assert_eq!(kept.refresh_token.as_deref(), Some("rt-old"));
        assert_eq!(kept.name.as_deref(), Some("Sam"));
        assert!(server.seen.lock().unwrap()[0]
            .1
            .contains("grant_type=refresh_token"));
    }

    #[test]
    fn a_refused_refresh_says_to_sign_in_again() {
        let (_dir, _cfg) = config("refused");
        let server = mock(vec![("/token", 400, r#"{"error":"invalid_grant"}"#)]);
        let _base = use_base(&server.base);
        let store = Mem::default();
        save(
            &store,
            &XaiOAuthTokens {
                access_token: "at-old".into(),
                refresh_token: Some("rt-old".into()),
                expires_at: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(bearer(&store), Err(EXPIRED.into()));
    }

    #[test]
    fn a_denied_code_fails_and_sign_out_deletes_the_tokens() {
        let (_dir, _cfg) = config("denied");
        let server = mock(vec![
            ("/device", 200, START),
            ("/token", 400, r#"{"error":"access_denied"}"#),
        ]);
        let _base = use_base(&server.base);
        let flow = SignIn::default();
        let store = Mem::default();
        flow.start().expect("start");
        assert!(matches!(flow.poll(&store), Ok(Poll::Failed { .. })));
        assert_eq!(store.get(), Ok(None));
        store.set(TOKENS).unwrap();
        sign_out(&store).unwrap();
        assert_eq!(bearer(&store), Ok(None));
        assert_eq!(signed_in(&store), None);
    }

    #[test]
    fn only_a_loopback_override_is_taken() {
        assert_eq!(
            loopback("http://127.0.0.1:4611/"),
            Some("http://127.0.0.1:4611".into())
        );
        assert_eq!(
            loopback("http://localhost:9"),
            Some("http://localhost:9".into())
        );
        assert_eq!(loopback("https://127.0.0.1:9"), None);
        assert_eq!(loopback("http://auth.example.com"), None);
        assert_eq!(loopback("http://127.0.0.1.evil.com"), None);
    }

    #[test]
    fn an_override_is_judged_by_the_host_the_request_would_reach() {
        for raw in [
            "http://127.0.0.1:9@127.0.0.2:1",
            "http://localhost:1@not-loopback.example",
            "http://127.0.0.1:x@h",
            "http://127.0.0.1\\@not-loopback.example",
            "http://localhost?@not-loopback.example",
            "http://localhost#@not-loopback.example",
            "http://127.0.0.1:x",
            "http://127.0.0.1:",
            "http://[::1].not-loopback.example",
            "http://[::1]x:9",
        ] {
            assert_eq!(loopback(raw), None, "{raw}");
        }
        assert_eq!(
            loopback("http://127.0.0.1:48212/"),
            Some("http://127.0.0.1:48212".into())
        );
        assert_eq!(loopback("http://[::1]:9"), Some("http://[::1]:9".into()));
        assert_eq!(
            loopback("http://localhost"),
            Some("http://localhost".into())
        );
    }

    #[test]
    fn ask_tells_a_call_with_no_answer_from_one_that_cannot_work() {
        let (_dir, _cfg) = config("ask");
        let store = Mem::default();
        assert_eq!(
            SignIn::default().ask(&store),
            Err(CallError::Other(NOT_STARTED.into()))
        );
        // The auth server hands out the code, then goes away.
        let m = mock(vec![("/device", 200, START)]);
        let _base = use_base(&m.base);
        let flow = SignIn::default();
        flow.start().expect("start");
        let err = flow.ask(&store).expect_err("nobody answers");
        let CallError::NoAnswer(text) = &err else {
            panic!("{err:?}");
        };
        assert!(text.starts_with(&format!("{}/token: ", m.base)), "{text}");
        // The code is still held, so the next ask goes out too.
        assert!(matches!(flow.ask(&store), Err(CallError::NoAnswer(_))));
        // The app's poll says so in an answer, so the page keeps the code up.
        assert!(matches!(
            flow.poll(&store),
            Ok(Poll::NoAnswer { error }) if error.starts_with(&format!("{}/token: ", m.base))
        ));
    }

    /// A store that takes nothing, as a keychain with no secret service does.
    struct Shut;

    impl Store for Shut {
        fn get(&self) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn set(&self, _value: &str) -> Result<(), String> {
            Err("The system keychain is not available.".into())
        }
        fn delete(&self) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn a_sign_in_the_store_cannot_keep_is_an_error_not_a_poll_to_repeat() {
        let (_dir, _cfg) = config("shut");
        let m = mock(vec![
            ("/device", 200, START),
            ("/token", 200, TOKENS),
            ("/userinfo", 200, r#"{"name":"Sam"}"#),
        ]);
        let _base = use_base(&m.base);
        let flow = SignIn::default();
        flow.start().expect("start");
        assert_eq!(
            flow.poll(&Shut),
            Err("The system keychain is not available.".to_string())
        );
    }

    #[test]
    fn a_server_error_is_no_answer_and_the_code_is_kept() {
        let (_dir, _cfg) = config("busy");
        let m = mock(vec![
            ("/device", 200, START),
            ("/token", 503, "<html>Service Unavailable</html>"),
            ("/token", 500, r#"{"error":"server_error"}"#),
            ("/token", 429, "Too Many Requests"),
            ("/token", 429, r#"{"error":"slow_down"}"#),
            ("/token", 400, "<html>Bad Request</html>"),
        ]);
        let _base = use_base(&m.base);
        let flow = SignIn::default();
        let store = Mem::default();
        flow.start().expect("start");
        for code in [503, 500, 429] {
            assert_eq!(
                flow.ask(&store),
                Err(CallError::NoAnswer(format!(
                    "{}/token: status code {code}",
                    m.base
                )))
            );
        }
        // xAI's own "slow down" is an answer, whatever its status.
        assert_eq!(flow.ask(&store), Ok(Poll::Pending { interval: 10 }));
        // A refusal the server did not explain still ends the sign-in.
        assert_eq!(
            flow.ask(&store),
            Ok(Poll::Failed {
                error: "oauth error (unknown)".into()
            })
        );
        assert_eq!(flow.ask(&store), Err(CallError::Other(NOT_STARTED.into())));
    }

    #[test]
    fn a_server_error_on_refresh_does_not_say_to_sign_in_again() {
        let (_dir, _cfg) = config("refresh-busy");
        let server = mock(vec![("/token", 503, "<html>Service Unavailable</html>")]);
        let _base = use_base(&server.base);
        let store = Mem::default();
        let old = XaiOAuthTokens {
            access_token: "at-old".into(),
            refresh_token: Some("rt-old".into()),
            expires_at: Some(1),
            ..Default::default()
        };
        save(&store, &old).unwrap();
        assert_eq!(
            bearer(&store),
            Err(format!("{}/token: status code 503", server.base))
        );
        // The tokens are as they were, for the next try.
        assert_eq!(load(&store), Ok(Some(old)));
    }

    #[test]
    fn offline_only_refuses_sign_in_before_any_call() {
        let (_dir, _cfg) = config("offline");
        let _on = grokhub_core::offline::force_for_thread(true);
        let err = SignIn::default().start().expect_err("refused");
        assert!(err.contains("Offline only"), "{err}");
    }

    #[test]
    fn ready_poll_serializes_signed_in_as_camel_case() {
        // `rename_all` alone only renames the variant; fields need
        // `rename_all_fields` or the UI reads `signedIn` as undefined.
        let json = serde_json::to_value(Poll::Ready {
            signed_in: SignedIn {
                name: Some("Sam".into()),
                email: Some("sam@example.com".into()),
            },
        })
        .expect("serialize");
        assert_eq!(json["status"], "ready");
        assert!(json.get("signed_in").is_none(), "{json}");
        assert_eq!(json["signedIn"]["name"], "Sam");
        assert_eq!(json["signedIn"]["email"], "sam@example.com");
    }
}
