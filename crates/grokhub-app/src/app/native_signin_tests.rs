//! Lab mode (native engine) picks up the Grok sign-in GrokHub already has.
//! Every credential here is a fake string in a temp HOME / GROKHUB_CONFIG, and
//! every request goes to a loopback fake server. Nothing reaches xAI.

use super::native_engine::{set_responses_url_for_test, NATIVE_NEEDS_CABIN_SIGNIN};
use super::*;
use grokhub_agent::AuthKind;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const REPLY: &str = "Fake reply from the test server";

#[derive(Debug, Clone)]
struct Seen {
    path: String,
    authorization: String,
    body: String,
}

/// One loopback server for the Responses stream and the OAuth token endpoint.
struct FakeXai {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    token_calls: Arc<AtomicUsize>,
}

impl FakeXai {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let token_calls = Arc::new(AtomicUsize::new(0));
        let (seen_t, calls_t) = (seen.clone(), token_calls.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let mut authorization = String::new();
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                        break;
                    }
                    let (name, value) = h.split_once(':').unwrap_or((&h, ""));
                    match name.trim().to_ascii_lowercase().as_str() {
                        "authorization" => authorization = value.trim().to_string(),
                        "content-length" => len = value.trim().parse().unwrap_or(0),
                        _ => {}
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let body = String::from_utf8_lossy(&body).into_owned();
                seen_t.lock().unwrap().push(Seen {
                    path: path.clone(),
                    authorization,
                    body,
                });
                let (kind, payload) = if path == "/oauth2/token" {
                    calls_t.fetch_add(1, Ordering::SeqCst);
                    (
                        "application/json",
                        r#"{"access_token":"test-refreshed-token","refresh_token":"test-refresh-token-2","expires_in":3600,"token_type":"Bearer"}"#.to_string(),
                    )
                } else {
                    (
                        "text/event-stream",
                        format!(
                            "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"{REPLY}\"}}\n\n\
                             data: {{\"type\":\"response.completed\",\"response\":{{\"usage\":{{\"input_tokens\":3,\"output_tokens\":5}},\"output\":[]}}}}\n\n"
                        ),
                    )
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(payload.as_bytes());
                let _ = stream.flush();
            }
        });
        Self {
            base,
            seen,
            token_calls,
        }
    }

    fn responses(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path == "/v1/responses")
            .cloned()
            .collect()
    }
}

/// Temp HOME, GROK_HOME and GROKHUB_CONFIG, restored on drop.
struct Sandbox {
    _lock: std::sync::MutexGuard<'static, ()>,
    root: std::path::PathBuf,
    prev: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let lock = crate::config::hold_test_config();
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["home", "config", "work"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let mut prev = Vec::new();
        for (key, dir) in [
            ("HOME", "home"),
            ("GROKHUB_CONFIG", "config"),
            (concat!("GROK", "_HOME"), "home/.grok-env"),
        ] {
            prev.push((key, std::env::var_os(key)));
            std::env::set_var(key, root.join(dir));
        }
        grokhub_acp::invalidate_grok_key_cache();
        crate::oauth::set_token_url_for_test(None);
        set_responses_url_for_test(None);
        Self {
            _lock: lock,
            root,
            prev,
        }
    }

    /// A fake GrokHub sign-in, written where Settings → Account saves it.
    fn write_signin(&self, oauth: &str) {
        let body = format!(r#"{{"apiKey":"","oauth":{oauth}}}"#);
        std::fs::write(crate::secrets::secrets_path(), body).unwrap();
    }

    fn cabin(&self) -> Cabin {
        let mut cabin = Cabin::quiet_for_test();
        cabin.secrets = crate::secrets::load();
        cabin.cfg.grok_build_engine = false;
        let mut thread = crate::threads::ChatThread::new("Lab chat", false);
        thread.native = true;
        thread.grok_cwd = Some(self.root.join("work").display().to_string());
        cabin.threads.push(thread);
        cabin.thread_idx = cabin.threads.len() - 1;
        cabin
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        crate::oauth::set_token_url_for_test(None);
        set_responses_url_for_test(None);
        for (key, value) in self.prev.drain(..) {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        grokhub_acp::invalidate_grok_key_cache();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// One chat message through the native engine thread. Returns the reply text.
fn run_turn(cabin: &mut Cabin, prompt: &str) -> String {
    cabin.ensure_native_engine().expect("native engine starts");
    let handle = cabin.acp.as_ref().expect("native handle");
    handle.prompt(prompt).expect("prompt queued");
    let mut text = String::new();
    let start = std::time::Instant::now();
    loop {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "no Done; text so far: {text}"
        );
        match handle.try_recv() {
            Ok(grokhub_acp::AcpEvent::Text(t)) => text.push_str(&t),
            Ok(grokhub_acp::AcpEvent::Err(e)) => panic!("native turn failed: {e}"),
            Ok(grokhub_acp::AcpEvent::Done { .. }) => break,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    cabin.acp = None;
    text
}

fn future_ms() -> u64 {
    grokhub_core::now_ms() + 6 * 60 * 60 * 1000
}

#[test]
fn native_signin_grok_oauth_without_api_key_sends_bearer() {
    let sb = Sandbox::new("native-signin-live");
    let server = FakeXai::start();
    set_responses_url_for_test(Some(&format!("{}/v1/responses", server.base)));
    sb.write_signin(&format!(
        r#"{{"accessToken":"test-access-token","refreshToken":"test-refresh-token","expiresAt":{},"connectedAt":1}}"#,
        future_ms()
    ));
    let mut cabin = sb.cabin();
    assert_eq!(cabin.console_key(), "");
    assert!(cabin.imagine_native.tokens.is_none(), "no Imagine sign-in");
    let (bearer, kind) = cabin.native_cred().expect("signed in via Grok");
    assert_eq!(bearer, "test-access-token");
    assert_eq!(kind, AuthKind::OAuth);

    let text = run_turn(
        &mut cabin,
        "take what you know about me and update the feed instructions",
    );
    assert_eq!(text, REPLY);
    let sent = server.responses();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].authorization, "Bearer test-access-token");
    assert!(
        sent[0].body.contains("update the feed instructions"),
        "{}",
        sent[0].body
    );
    assert_eq!(
        server.token_calls.load(Ordering::SeqCst),
        0,
        "a live token is not refreshed"
    );
    eprintln!(
        "PROOF live: POST {} Authorization: {} -> {:?}",
        sent[0].path, sent[0].authorization, text
    );
}

#[test]
fn native_signin_expired_grok_oauth_refreshes_once() {
    let sb = Sandbox::new("native-signin-refresh");
    let server = FakeXai::start();
    set_responses_url_for_test(Some(&format!("{}/v1/responses", server.base)));
    crate::oauth::set_token_url_for_test(Some(&format!("{}/oauth2/token", server.base)));
    sb.write_signin(
        r#"{"accessToken":"test-expired-token","refreshToken":"test-refresh-token","expiresAt":1000,"connectedAt":1,"email":"ada@example.com"}"#,
    );
    let mut cabin = sb.cabin();
    let (bearer, kind) = cabin.native_cred().expect("refreshed");
    assert_eq!(bearer, "test-refreshed-token");
    assert_eq!(kind, AuthKind::OAuth);
    assert_eq!(server.token_calls.load(Ordering::SeqCst), 1);
    let token_req = server
        .seen
        .lock()
        .unwrap()
        .iter()
        .find(|s| s.path == "/oauth2/token")
        .cloned()
        .expect("refresh request");
    assert!(
        token_req.body.contains("grant_type=refresh_token"),
        "{}",
        token_req.body
    );
    assert!(
        token_req.body.contains("refresh_token=test-refresh-token"),
        "{}",
        token_req.body
    );
    assert!(
        token_req
            .body
            .contains(&format!("client_id={}", grokhub_core::XAI_OAUTH_CLIENT_ID)),
        "{}",
        token_req.body
    );

    let text = run_turn(&mut cabin, "hello");
    assert_eq!(text, REPLY);
    assert_eq!(
        server.token_calls.load(Ordering::SeqCst),
        1,
        "refresh is spent once"
    );
    let sent = server.responses();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].authorization, "Bearer test-refreshed-token");

    let kept = cabin.secrets.oauth.clone().expect("kept");
    assert_eq!(kept.access_token, "test-refreshed-token");
    assert_eq!(kept.refresh_token.as_deref(), Some("test-refresh-token-2"));
    assert_eq!(kept.email.as_deref(), Some("ada@example.com"));
    let start = std::time::Instant::now();
    let saved = loop {
        let saved = crate::secrets::load()
            .oauth
            .map(|t| t.access_token)
            .unwrap_or_default();
        if saved == "test-refreshed-token" || start.elapsed() > Duration::from_secs(3) {
            break saved;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        saved, "test-refreshed-token",
        "the refresh is saved to GrokHub's own store"
    );
    assert!(
        !sb.root.join("home/.grok").exists(),
        "Lab mode never writes the Grok CLI's home"
    );
    eprintln!(
        "PROOF refresh: token endpoint calls={} then POST {} Authorization: {} -> {:?}",
        server.token_calls.load(Ordering::SeqCst),
        sent[0].path,
        sent[0].authorization,
        text
    );
}

#[test]
fn native_signin_missing_gives_exact_message() {
    let sb = Sandbox::new("native-signin-none");
    let server = FakeXai::start();
    set_responses_url_for_test(Some(&format!("{}/v1/responses", server.base)));
    crate::oauth::set_token_url_for_test(Some(&format!("{}/oauth2/token", server.base)));
    let mut cabin = sb.cabin();
    assert_eq!(
        cabin.native_cred().unwrap_err(),
        "Sign in with Grok or add an API key."
    );
    assert_eq!(
        cabin.ensure_native_engine().unwrap_err(),
        "Sign in with Grok or add an API key."
    );

    // Expired with no refresh token is no sign-in either.
    sb.write_signin(r#"{"accessToken":"test-expired-token","expiresAt":1000,"connectedAt":1}"#);
    cabin.secrets = crate::secrets::load();
    assert_eq!(
        cabin.native_cred().unwrap_err(),
        "Sign in with Grok or add an API key."
    );
    assert_eq!(server.token_calls.load(Ordering::SeqCst), 0);

    // Only the Grok CLI is signed in ($HOME/.grok): say which sign-in Lab mode needs.
    // The CLI's token is never used or echoed. GROK_HOME points elsewhere and changes nothing.
    std::fs::remove_file(crate::secrets::secrets_path()).unwrap();
    cabin.secrets = crate::secrets::load();
    let cli = sb.root.join("home/.grok");
    std::fs::create_dir_all(&cli).unwrap();
    std::fs::write(
        cli.join(concat!("auth", ".json")),
        r#"{"https://auth.x.ai::fake":{"key":"test-cli-token"}}"#,
    )
    .unwrap();
    grokhub_acp::invalidate_grok_key_cache();
    let err = cabin.native_cred().unwrap_err();
    assert_eq!(
        err,
        "GrokHub signs in on its own now, not through the Grok CLI. Sign in with Grok in Settings → Account, or add an API key."
    );
    assert_eq!(err, NATIVE_NEEDS_CABIN_SIGNIN);
    assert!(!err.contains("test-cli-token"));

    // An API key stays a working alternative.
    cabin.secrets.api_key = "test-console-key".into();
    let (bearer, kind) = cabin.native_cred().expect("api key");
    assert_eq!(bearer, "test-console-key");
    assert_eq!(kind, AuthKind::ApiKey);
    assert!(
        server.seen.lock().unwrap().is_empty(),
        "no request was sent"
    );
    eprintln!("PROOF none: {:?}", "Sign in with Grok or add an API key.");
    eprintln!("PROOF cli-only: {err:?}");
}

#[test]
fn native_signin_failed_refresh_is_not_retried_every_turn() {
    let sb = Sandbox::new("native-signin-refresh-fails");
    let server = FakeXai::start();
    // Not the token path: the server answers with an event stream, so the refresh fails.
    crate::oauth::set_token_url_for_test(Some(&format!("{}/oauth2/broken", server.base)));
    sb.write_signin(
        r#"{"accessToken":"test-expired-token","refreshToken":"test-refresh-token","expiresAt":1000,"connectedAt":1}"#,
    );
    let mut cabin = sb.cabin();
    let tries = || {
        server
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path == "/oauth2/broken")
            .count()
    };
    assert_eq!(
        cabin.native_cred().unwrap_err(),
        "Sign in with Grok or add an API key."
    );
    assert_eq!(tries(), 1);
    // The next turns inside the backoff do not wait on the network again.
    for _ in 0..3 {
        assert_eq!(
            cabin.native_cred().unwrap_err(),
            "Sign in with Grok or add an API key."
        );
    }
    assert_eq!(tries(), 1, "a failed refresh is not retried every turn");
    assert_eq!(
        cabin.secrets.oauth.as_ref().map(|t| t.access_token.as_str()),
        Some("test-expired-token"),
        "a failed refresh keeps the saved sign-in"
    );
}

#[test]
fn native_signin_source_never_uses_cli_login_token() {
    let engine = include_str!("native_engine.rs");
    let cred = engine
        .split("pub(super) fn native_cred(")
        .nth(1)
        .and_then(|s| s.split("fn keep_account_tokens(").next())
        .expect("native_cred");
    assert!(cred.contains("self.secrets.oauth"), "{cred}");
    assert!(cred.contains("ensure_access"), "{cred}");
    assert!(!cred.contains("grok_cli_key"), "{cred}");
    assert!(!cred.contains("refresh_grok_login"), "{cred}");
    assert!(
        !engine.contains(concat!("eprintln!(\"", "{bearer")),
        "never log a token"
    );
}

/// Imagine must reuse Settings → Account the same way Lab mode does.
/// Fake tokens only; no live xAI calls.
#[test]
fn imagine_cred_reuses_account_oauth_without_imagine_keychain() {
    let sb = Sandbox::new("imagine-reuse-account");
    sb.write_signin(&format!(
        r#"{{"accessToken":"test-account-access","refreshToken":"test-refresh","expiresAt":{},"connectedAt":1,"email":"ada@example.com"}}"#,
        future_ms()
    ));
    let mut cabin = sb.cabin();
    cabin.cfg.grok_build_engine = true;
    assert!(
        cabin.imagine_native.tokens.is_none(),
        "Imagine keychain empty"
    );
    assert_eq!(cabin.console_key(), "");
    assert!(cabin.account_oauth_present());
    assert!(cabin.imagine_ui_ready(), "Account alone must count as signed in");

    let cred = cabin.imagine_cred().expect("Account covers Imagine");
    assert_eq!(cred.secret, "test-account-access");
    assert_eq!(cred.kind, grokhub_core::ImagineCredKind::OAuth);

    // Second call must not re-prompt / fail — same live Account token.
    let again = cabin.imagine_cred().expect("no re-prompt");
    assert_eq!(again.secret, "test-account-access");
    assert_eq!(again.kind, grokhub_core::ImagineCredKind::OAuth);
    assert!(cabin.imagine_native.tokens.is_none(), "must not invent Imagine keychain tokens");
}

#[test]
fn imagine_cred_prefers_imagine_keychain_over_account() {
    let sb = Sandbox::new("imagine-prefer-keychain");
    sb.write_signin(&format!(
        r#"{{"accessToken":"test-account-access","refreshToken":"test-refresh","expiresAt":{},"connectedAt":1}}"#,
        future_ms()
    ));
    let mut cabin = sb.cabin();
    cabin.imagine_native.tokens = Some(grokhub_core::ImagineTokens {
        access_token: "test-imagine-access".into(),
        refresh_token: Some("test-imagine-refresh".into()),
        expires_at: Some(future_ms()),
        id_token: None,
        email: Some("imagine@example.com".into()),
        connected_at: 1,
    });
    let cred = cabin.imagine_cred().expect("Imagine keychain wins");
    assert_eq!(cred.secret, "test-imagine-access");
    assert_eq!(cred.kind, grokhub_core::ImagineCredKind::OAuth);
}

#[test]
fn kick_imagine_with_account_oauth_starts_without_second_signin() {
    let sb = Sandbox::new("imagine-kick-account");
    sb.write_signin(&format!(
        r#"{{"accessToken":"test-account-access","refreshToken":"test-refresh","expiresAt":{},"connectedAt":1,"email":"ada@example.com"}}"#,
        future_ms()
    ));
    let mut cabin = sb.cabin();
    cabin.cfg.grok_build_engine = true;
    cabin.imagine_native.tokens = None;
    cabin.imagine_prompt = "harbor at dusk".into();
    // Force-key path off; no console key. Account alone must be enough to leave
    // the need-signin path (job may still fail later on a fake token — we only
    // prove it does not refuse with IMAGINE_NEED_SIGNIN).
    cabin.kick_imagine();
    assert_ne!(
        cabin.imagine_error.as_str(),
        grokhub_core::IMAGINE_NEED_SIGNIN,
        "Account sign-in must not show the need-signin wall"
    );
    assert_ne!(cabin.status.as_str(), grokhub_core::IMAGINE_NEED_SIGNIN);
    // Either running (job spawned) or some other non-signin status.
    assert!(
        cabin.running || cabin.rx.is_some() || !cabin.imagine_error.is_empty(),
        "expected a started job or a non-signin error, got status={:?} err={:?}",
        cabin.status,
        cabin.imagine_error
    );
    // Clean up any spawned job thread so Drop is quiet.
    if let Some(rx) = cabin.rx.take() {
        cabin.running = false;
        cabin.imagine_pending = false;
        let _ = rx.recv_timeout(std::time::Duration::from_millis(200));
    }
}

#[test]
fn imagine_need_signin_message_points_at_account_settings() {
    assert_eq!(
        grokhub_core::IMAGINE_NEED_SIGNIN,
        "Sign in with Grok in Settings → Account, or add a console API key."
    );
    assert!(
        !grokhub_core::IMAGINE_NEED_SIGNIN.contains("for Imagine"),
        "must not push a second Imagine-only sign-in"
    );
}

fn wait_done(cabin: &Cabin) {
    let handle = cabin.acp.as_ref().expect("native handle");
    let start = std::time::Instant::now();
    loop {
        assert!(start.elapsed() < Duration::from_secs(20), "no Done");
        match handle.try_recv() {
            Ok(grokhub_acp::AcpEvent::Err(e)) => panic!("native turn failed: {e}"),
            Ok(grokhub_acp::AcpEvent::Done { .. }) => return,
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

#[test]
fn a_cli_era_chat_moves_to_the_native_engine_with_a_recap_once() {
    let sb = Sandbox::new("native-adopt");
    let server = FakeXai::start();
    set_responses_url_for_test(Some(&format!("{}/v1/responses", server.base)));
    sb.write_signin(&format!(
        r#"{{"accessToken":"test-access-token","refreshToken":"test-refresh-token","expiresAt":{},"connectedAt":1}}"#,
        future_ms()
    ));
    let mut cabin = sb.cabin();
    let turns: Vec<(String, String)> = [
        ("user", "plan the harbor walk"),
        ("assistant", "Start at the north pier."),
        ("user", "what about lunch"),
    ]
    .iter()
    .map(|(r, t)| (r.to_string(), t.to_string()))
    .collect();
    {
        // A chat from the CLI days: a CLI session id and no native flag.
        let thread = cabin.threads.last_mut().unwrap();
        thread.native = false;
        thread.grok_session = Some("cli-sess-7".into());
        thread.messages = Arc::new(turns.clone());
    }
    cabin.messages = Arc::new(turns);
    assert!(cabin.native_engine_for_current());

    assert!(cabin.kick_native_turn("what about lunch", None, "what about lunch", "Lab chat"));
    wait_done(&cabin);
    let thread = cabin.threads.last().unwrap();
    assert!(thread.native);
    assert!(!thread.native_carry);
    assert_eq!(thread.retired_sessions, vec!["cli-sess-7".to_string()]);
    let sid = thread.grok_session.clone().unwrap();
    assert!(sid.starts_with("native-"), "{sid}");
    let sent = server.responses();
    assert_eq!(sent.len(), 1, "{sent:?}");
    let body = &sent[0].body;
    assert!(body.contains("Earlier in this chat (before GrokHub ran it natively):"), "{body}");
    assert!(body.contains("User: plan the harbor walk"), "{body}");
    assert!(body.contains("You: Start at the north pier."), "{body}");
    assert_eq!(body.matches("what about lunch").count(), 1, "{body}");

    assert!(cabin.kick_native_turn("and dinner", None, "and dinner", "Lab chat"));
    wait_done(&cabin);
    let sent = server.responses();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(sent[1].body.contains("and dinner"), "{}", sent[1].body);
    // The second request replays the first turn from the session, recap included,
    // but no new recap is added.
    assert_eq!(sent[1].body.matches("Earlier in this chat").count(), 1, "{}", sent[1].body);
    assert_eq!(cabin.threads.last().unwrap().grok_session.as_deref(), Some(sid.as_str()));
}
