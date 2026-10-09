//! Browser sign-in for remote MCP servers: discovery from the server's 401,
//! dynamic client registration, authorization code + PKCE S256 on a loopback
//! redirect, and refresh. The sign-in is sealed with the keyring key next to
//! the connection tokens (`harness::seal_mcp_signin`), never written plain.
//! Every request is a handshake-class send (no user data) behind the
//! EgressGuard, like `http::connect`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use grokhub_core::mcp_oauth::{
    auth_server_metadata_urls, authorize_url, challenge_resource_metadata, challenge_scope, code_form,
    endpoint_allowed, parse_auth_server, parse_registration, parse_resource_metadata, pick_scope,
    refresh_form, registration_request, resource_for, resource_metadata_urls, signed_in_line, split_origin,
    AuthServer, Authorize, McpSignIn,
};
use serde_json::Value;

use super::http::{agent, guard_handshake};
use super::rpc;

/// How long the loopback waits for the browser to come back.
pub(super) const LOOPBACK_SECS: u64 = 300;
const READ_CAP: usize = 16 * 1024;
const BODY_CAP: u64 = 256 * 1024;
const REQUEST_SECS: u64 = 20;

fn read_capped(resp: ureq::Response) -> Value {
    let mut buf = Vec::new();
    let _ = resp.into_reader().take(BODY_CAP).read_to_end(&mut buf);
    serde_json::from_slice(&buf).unwrap_or(Value::Null)
}

/// A metadata document, or `None` when the server has none at this URL.
fn get_json(url: &str) -> Result<Option<Value>, String> {
    guard_handshake(url)?;
    let resp = agent(Duration::from_secs(REQUEST_SECS))
        .get(url)
        .set("Accept", "application/json")
        .timeout(Duration::from_secs(REQUEST_SECS))
        .call();
    match resp {
        Ok(r) => Ok(Some(read_capped(r)).filter(Value::is_object)),
        Err(ureq::Error::Status(..)) => Ok(None),
        Err(ureq::Error::Transport(err)) => Err(err.to_string()),
    }
}

fn post(url: &str, ctype: &str, body: &str) -> Result<(bool, Value), String> {
    guard_handshake(url)?;
    let resp = agent(Duration::from_secs(REQUEST_SECS))
        .post(url)
        .set("Content-Type", ctype)
        .set("Accept", "application/json")
        .timeout(Duration::from_secs(REQUEST_SECS))
        .send_string(body);
    match resp {
        Ok(r) => Ok((true, read_capped(r))),
        Err(ureq::Error::Status(_, r)) => Ok((false, read_capped(r))),
        Err(ureq::Error::Transport(err)) => Err(err.to_string()),
    }
}

fn form_error(v: &Value, fallback: &str) -> String {
    v.get("error_description")
        .or_else(|| v.get("error"))
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

/// The `WWW-Authenticate` of the server's 401 to an unsigned `initialize`.
fn probe(url: &str) -> Option<String> {
    guard_handshake(url).ok()?;
    let body = rpc::request(0, "initialize", rpc::initialize_params()).to_string();
    let resp = agent(Duration::from_secs(REQUEST_SECS))
        .post(url)
        .set("Content-Type", "application/json")
        .set("Accept", "application/json, text/event-stream")
        .set("MCP-Protocol-Version", rpc::PROTOCOL)
        .timeout(Duration::from_secs(REQUEST_SECS))
        .send_string(&body);
    match resp {
        Err(ureq::Error::Status(401, r)) => r.header("www-authenticate").map(str::to_string),
        _ => None,
    }
}

/// The authorization server and the scope to ask for. A server with no
/// protected resource metadata (the 2025-03-26 spec) is its own sign-in server.
fn discover(server: &str, url: &str) -> Result<(AuthServer, Option<String>), String> {
    let challenge = probe(url);
    let mut prm: Vec<String> = challenge.as_deref().and_then(challenge_resource_metadata).into_iter().collect();
    prm.extend(resource_metadata_urls(url));
    let mut issuer = None;
    let mut resource_scopes = Vec::new();
    for at in prm.iter().filter(|u| endpoint_allowed(u, url)) {
        if let Some(doc) = get_json(at)? {
            let (found, scopes) = parse_resource_metadata(&doc)?;
            issuer = Some(found);
            resource_scopes = scopes;
            break;
        }
    }
    let issuer = match issuer {
        Some(found) => found,
        None => split_origin(url).map(|(origin, _)| origin).ok_or("its url is not http or https")?,
    };
    for at in auth_server_metadata_urls(&issuer).iter().filter(|u| endpoint_allowed(u, url)) {
        if let Some(doc) = get_json(at)? {
            let found = parse_auth_server(&doc, url)?;
            let scope = pick_scope(challenge.as_deref().and_then(challenge_scope).as_deref(), &resource_scopes, &found.scopes_supported);
            return Ok((found, scope));
        }
    }
    Err(format!("{server} doesn't offer a browser sign-in"))
}

fn close_page() -> Vec<u8> {
    let body = b"<p>Signed in. You can close this tab.</p>";
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

/// Wait on the loopback for `/callback`, answering strays (a favicon, a prefetch) with 404.
fn wait_code(listener: &TcpListener, state: &str, who: &str, wait: Duration) -> Result<String, String> {
    let deadline = Instant::now() + wait;
    loop {
        if Instant::now() >= deadline {
            return Err(format!("{who} timed out"));
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(40));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; READ_CAP];
        let n = stream.read(&mut buf).unwrap_or(0).min(READ_CAP);
        let head = String::from_utf8_lossy(&buf[..n]);
        if !head.starts_with("GET /callback") {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        }
        let _ = stream.write_all(&close_page());
        return grokhub_core::pkce::loopback_code(&head, state, who);
    }
}

fn save(server: &str, signin: &McpSignIn) -> Result<(), String> {
    let json = serde_json::to_string(signin).map_err(|e| e.to_string())?;
    crate::harness::seal_mcp_signin(&crate::perm::config_dir(), server, &json)
}

/// The sign-in sealed for `server`, if any.
pub(super) fn load(server: &str) -> Option<McpSignIn> {
    let json = crate::harness::open_mcp_signin(&crate::perm::config_dir(), server)?;
    serde_json::from_str(&json).ok()
}

pub(super) fn forget(server: &str) {
    crate::harness::forget_mcp_signin(&crate::perm::config_dir(), server);
}

/// Sign in to `server` at `url` in the browser `open` launches. Returns the
/// line naming the account ("Signed in to Linear as ada@example.com").
pub(super) fn sign_in(
    server: &str,
    url: &str,
    open: &dyn Fn(&str) -> Result<(), String>,
    wait: Duration,
) -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let (auth, scope) = discover(server, url)?;
    let register = auth
        .registration_endpoint
        .as_deref()
        .ok_or_else(|| format!("{server} needs a client id that GrokHub can't register on its own"))?;
    let (ok, v) = post(register, "application/json", &registration_request(&redirect).to_string())?;
    if !ok {
        return Err(format!("{server} refused to register GrokHub: {}", form_error(&v, "no reason given")));
    }
    let client = parse_registration(&v)?;
    let resource = resource_for(url);
    let verifier = grokhub_core::pkce::pkce_verifier();
    let challenge = grokhub_core::pkce::pkce_challenge(&verifier);
    let state = grokhub_core::pkce::oauth_nonce();
    let link = authorize_url(&Authorize {
        authorization_endpoint: &auth.authorization_endpoint,
        client_id: &client.client_id,
        redirect_uri: &redirect,
        code_challenge: &challenge,
        state: &state,
        scope: scope.as_deref(),
        resource: &resource,
    });
    open(&link)?;
    let code = wait_code(&listener, &state, &format!("{server} sign-in"), wait)?;
    let (ok, v) = post(
        &auth.token_endpoint,
        "application/x-www-form-urlencoded",
        &code_form(&code, &verifier, &redirect, &client, &resource),
    )?;
    if !ok {
        return Err(format!("{server} sign-in failed: {}", form_error(&v, "the code was refused")));
    }
    let tokens = grokhub_core::oauth::parse_token_json(&v, grokhub_core::now_ms())?;
    let signin = McpSignIn::new(&client, &auth.token_endpoint, &resource, tokens);
    save(server, &signin)?;
    Ok(signed_in_line(server, signin.account.as_deref()))
}

fn refresh(server: &str, signin: &mut McpSignIn) -> Result<(), String> {
    let token = signin.refresh_token.clone().ok_or_else(|| format!("{server}: sign in again"))?;
    let (ok, v) = post(
        &signin.token_endpoint,
        "application/x-www-form-urlencoded",
        &refresh_form(&token, &signin.client(), &signin.resource),
    )?;
    if !ok {
        return Err(format!("{server}: sign in again ({})", form_error(&v, "the refresh was refused")));
    }
    signin.take_tokens(grokhub_core::oauth::parse_token_json(&v, grokhub_core::now_ms())?);
    save(server, signin)
}

/// The bearer token for `server` at `url`, refreshed first when it is about to
/// expire. `None` when there is no sign-in for this URL.
pub(super) fn bearer(server: &str, url: &str) -> Option<String> {
    let mut signin = load(server)?;
    if signin.resource != resource_for(url) {
        return None;
    }
    if signin.needs_refresh(grokhub_core::now_ms()) {
        // A failed refresh keeps the old token; the server's 401 then asks for a new sign-in.
        let _ = refresh(server, &mut signin);
    }
    Some(signin.access_token)
}

/// After a 401: refresh now and hand back the new token.
pub(super) fn refresh_now(server: &str, url: &str) -> Result<String, String> {
    let mut signin = load(server).filter(|s| s.resource == resource_for(url)).ok_or_else(|| format!("{server}: sign in again"))?;
    refresh(server, &mut signin)?;
    Ok(signin.access_token)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::harness as hx;
    use serde_json::json;
    use std::io::{BufRead, BufReader};
    use std::net::TcpStream;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// A remote MCP server that is its own OAuth server (RFC 9728 metadata,
    /// RFC 8414 metadata, registration, token). `/mcp` wants `Bearer <live>`.
    pub(crate) struct AuthMcp {
        pub port: u16,
        /// The access token `/mcp` accepts now.
        pub live: Arc<Mutex<String>>,
        pub refreshes: Arc<AtomicUsize>,
        pub tool_calls: Arc<AtomicUsize>,
        pub seen: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }

    impl AuthMcp {
        pub fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let me = Self {
                port,
                live: Arc::new(Mutex::new("at-1".into())),
                refreshes: Arc::new(AtomicUsize::new(0)),
                tool_calls: Arc::new(AtomicUsize::new(0)),
                seen: Arc::new(Mutex::new(Vec::new())),
                stop: Arc::new(AtomicBool::new(false)),
            };
            let (live, refreshes, calls, seen, stop) =
                (me.live.clone(), me.refreshes.clone(), me.tool_calls.clone(), me.seen.clone(), me.stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((sock, _)) => serve(sock, port, &live, &refreshes, &calls, &seen),
                        Err(_) => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
            });
            me
        }

        pub fn url(&self) -> String {
            format!("http://auth-mcp.example.test:{}/mcp", self.port)
        }
    }

    impl Drop for AuthMcp {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    fn reply(sock: &mut TcpStream, status: &str, extra: &str, body: &str) {
        let _ = write!(
            sock,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    }

    fn serve(
        mut sock: TcpStream,
        port: u16,
        live: &Mutex<String>,
        refreshes: &AtomicUsize,
        calls: &AtomicUsize,
        seen: &Mutex<Vec<String>>,
    ) {
        let _ = sock.set_nonblocking(false);
        let _ = sock.set_read_timeout(Some(Duration::from_secs(3)));
        let mut reader = BufReader::new(sock.try_clone().unwrap());
        let mut first = String::new();
        if reader.read_line(&mut first).unwrap_or(0) == 0 {
            return;
        }
        let mut len = 0usize;
        let mut auth = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
            if lower.starts_with("authorization:") {
                auth = line["authorization:".len()..].trim().to_string();
            }
        }
        let mut body = vec![0u8; len];
        let _ = reader.read_exact(&mut body);
        let body = String::from_utf8_lossy(&body).into_owned();
        let mut parts = first.split_whitespace();
        let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        seen.lock().unwrap().push(format!("{method} {path}"));
        let base = format!("http://auth-mcp.example.test:{port}");
        match (method, path) {
            ("GET", "/.well-known/oauth-protected-resource/mcp") => reply(
                &mut sock,
                "200 OK",
                "",
                &json!({"resource": format!("{base}/mcp"), "authorization_servers": [base], "scopes_supported": ["mcp:tools"]}).to_string(),
            ),
            ("GET", "/.well-known/oauth-authorization-server") => reply(
                &mut sock,
                "200 OK",
                "",
                &json!({
                    "issuer": base,
                    "authorization_endpoint": format!("{base}/authorize"),
                    "token_endpoint": format!("{base}/token"),
                    "registration_endpoint": format!("{base}/register"),
                    "code_challenge_methods_supported": ["S256"],
                })
                .to_string(),
            ),
            ("POST", "/register") => {
                let req: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                assert_eq!(req["token_endpoint_auth_method"], "none");
                reply(&mut sock, "201 Created", "", &json!({"client_id": "dcr-client-1"}).to_string())
            }
            ("POST", "/token") => {
                let resource = format!("resource={}", grokhub_core::pkce::urlencode(&format!("{base}/mcp")));
                if body.contains("grant_type=authorization_code") && body.contains("code=good-code") && body.contains("client_id=dcr-client-1") && body.contains(&resource) {
                    *live.lock().unwrap() = "at-1".into();
                    reply(&mut sock, "200 OK", "", &json!({"access_token": "at-1", "refresh_token": "rt-1", "expires_in": 3600}).to_string())
                } else if body.contains("grant_type=refresh_token") && body.contains("refresh_token=rt-1") && body.contains(&resource) {
                    refreshes.fetch_add(1, Ordering::SeqCst);
                    *live.lock().unwrap() = "at-2".into();
                    reply(&mut sock, "200 OK", "", &json!({"access_token": "at-2", "expires_in": 3600}).to_string())
                } else {
                    reply(&mut sock, "400 Bad Request", "", &json!({"error": "invalid_grant"}).to_string())
                }
            }
            ("POST", "/mcp") => {
                if auth != format!("Bearer {}", live.lock().unwrap()) {
                    let challenge = format!(
                        "WWW-Authenticate: Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\", scope=\"mcp:tools\"\r\n"
                    );
                    return reply(&mut sock, "401 Unauthorized", &challenge, "");
                }
                let msg: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let id = msg.get("id").cloned().unwrap_or(Value::Null);
                let result = match msg["method"].as_str().unwrap_or("") {
                    "initialize" => json!({"protocolVersion": rpc::PROTOCOL, "capabilities": {}, "serverInfo": {"name": "auth", "version": "0"}}),
                    "tools/list" => json!({"tools": [{"name": "echo", "description": "Echo", "inputSchema": {"type": "object"}}]}),
                    "tools/call" => {
                        calls.fetch_add(1, Ordering::SeqCst);
                        json!({"content": [{"type": "text", "text": "signed call"}]})
                    }
                    _ => json!({}),
                };
                if id.is_null() {
                    reply(&mut sock, "202 Accepted", "", "")
                } else {
                    reply(&mut sock, "200 OK", "", &json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string())
                }
            }
            _ => reply(&mut sock, "404 Not Found", "", ""),
        }
    }

    fn query_param(url: &str, key: &str) -> String {
        let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == key {
                    return v.replace("%3A", ":").replace("%2F", "/");
                }
            }
        }
        String::new()
    }

    /// The browser: approve at once by calling the loopback with `code`.
    pub(crate) fn approving_browser(code: &'static str) -> impl Fn(&str) -> Result<(), String> {
        move |link: &str| {
            let redirect = query_param(link, "redirect_uri");
            let state = query_param(link, "state");
            let target = redirect.trim_start_matches("http://").to_string();
            let (host, path) = target.split_once('/').unwrap();
            let (host, path) = (host.to_string(), path.to_string());
            std::thread::spawn(move || {
                let mut sock = TcpStream::connect(&host).unwrap();
                let _ = write!(sock, "GET /{path}?code={code}&state={state} HTTP/1.1\r\nHost: {host}\r\n\r\n");
                let mut sink = String::new();
                let _ = sock.read_to_string(&mut sink);
            });
            Ok(())
        }
    }

    #[test]
    fn a_401_server_signs_in_with_registration_pkce_and_a_sealed_token() {
        let dir = hx::test_dir("mcp-oauth-signin");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = AuthMcp::start();
        let opened = Arc::new(Mutex::new(String::new()));
        let seen_link = opened.clone();
        let approve = approving_browser("good-code");
        let line = sign_in(
            "linear",
            &server.url(),
            &move |link| {
                *seen_link.lock().unwrap() = link.to_string();
                approve(link)
            },
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(line, "Signed in to linear");
        let link = opened.lock().unwrap().clone();
        let base = format!("http://auth-mcp.example.test:{}", server.port);
        assert!(link.starts_with(&format!("{base}/authorize?response_type=code&client_id=dcr-client-1&redirect_uri=http%3A%2F%2F127.0.0.1%3A")), "{link}");
        assert!(link.contains("&code_challenge_method=S256&"), "{link}");
        assert!(link.contains("&scope=mcp%3Atools&"), "the 401's scope: {link}");
        assert!(link.ends_with(&format!("&resource={}", grokhub_core::pkce::urlencode(&format!("{base}/mcp")))), "{link}");
        assert_eq!(
            server.seen.lock().unwrap().clone(),
            vec![
                "POST /mcp".to_string(),
                "GET /.well-known/oauth-protected-resource/mcp".into(),
                "GET /.well-known/oauth-authorization-server".into(),
                "POST /register".into(),
                "POST /token".into(),
            ]
        );
        let signin = load("linear").expect("sealed sign-in opens");
        assert_eq!((signin.client_id.as_str(), signin.access_token.as_str(), signin.refresh_token.as_deref()), ("dcr-client-1", "at-1", Some("rt-1")));
        assert_eq!(bearer("linear", &server.url()).as_deref(), Some("at-1"));
        assert_eq!(bearer("linear", "http://elsewhere.example.test/mcp"), None, "a sign-in is for its own URL");
        // Nothing of the token is on disk in the clear.
        let sealed = std::fs::read_to_string(dir.join("connection-tokens").join("linear.signin.sealed")).unwrap();
        assert!(!sealed.contains("at-1") && !sealed.contains("rt-1") && !sealed.contains("dcr-client-1"));
        // Five handshake-class lines, no user data.
        let log = hx::read_egress(&dir);
        assert_eq!(log.len(), 5, "{log:?}");
        assert!(log.iter().all(|l| l.dest == "auth-mcp.example.test" && l.data_classes.is_empty()), "{log:?}");
        forget("linear");
        assert!(load("linear").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wrong_state_or_a_refused_code_signs_nothing_in() {
        let dir = hx::test_dir("mcp-oauth-refused");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = AuthMcp::start();
        let err = sign_in("linear", &server.url(), &approving_browser("bad-code"), Duration::from_secs(10)).unwrap_err();
        assert_eq!(err, "linear sign-in failed: invalid_grant");
        assert!(load("linear").is_none());
        let forged = |link: &str| {
            let forged = link.replace("state=", "state=x");
            approving_browser("good-code")(&forged)
        };
        let err = sign_in("linear", &server.url(), &forged, Duration::from_secs(10)).unwrap_err();
        assert_eq!(err, "linear sign-in state did not match");
        assert!(load("linear").is_none());
        let err = sign_in("linear", &server.url(), &|_: &str| Ok(()), Duration::from_millis(200)).unwrap_err();
        assert_eq!(err, "linear sign-in timed out");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn refresh_swaps_the_token_and_keeps_the_refresh_token() {
        let dir = hx::test_dir("mcp-oauth-refresh");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = AuthMcp::start();
        sign_in("linear", &server.url(), &approving_browser("good-code"), Duration::from_secs(10)).unwrap();
        // Due to expire: the next bearer refreshes first.
        let mut signin = load("linear").unwrap();
        signin.expires_at = Some(grokhub_core::now_ms());
        save("linear", &signin).unwrap();
        assert_eq!(bearer("linear", &server.url()).as_deref(), Some("at-2"));
        assert_eq!(server.refreshes.load(Ordering::SeqCst), 1);
        let after = load("linear").unwrap();
        assert_eq!((after.access_token.as_str(), after.refresh_token.as_deref()), ("at-2", Some("rt-1")));
        assert_eq!(bearer("linear", &server.url()).as_deref(), Some("at-2"), "fresh: no second refresh");
        assert_eq!(server.refreshes.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
