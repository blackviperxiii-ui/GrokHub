//! Browser sign-in for remote MCP servers (MCP authorization, 2025-06-18):
//! protected resource metadata (RFC 9728), authorization server metadata
//! (RFC 8414 / OpenID discovery), dynamic client registration (RFC 7591),
//! authorization code + PKCE S256 (RFC 7636) on a loopback redirect, refresh,
//! and the `resource` parameter (RFC 8707). Pure: the agent does the HTTP and
//! seals what [`McpSignIn`] holds.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::oauth::XaiOAuthTokens;
use crate::pkce::form;

/// Refresh this long before the access token runs out.
pub const REFRESH_SKEW_MS: u64 = 60_000;

/// `https://host:port` and `/path` of a URL; query and fragment dropped.
pub fn split_origin(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if host.is_empty() {
        return None;
    }
    let path = path.trim_end_matches('/');
    Some((format!("{scheme}://{host}"), path.to_string()))
}

/// The RFC 8707 resource: the server URL without query or fragment.
pub fn resource_for(server_url: &str) -> String {
    match split_origin(server_url) {
        Some((origin, path)) => format!("{origin}{path}"),
        None => server_url.trim().to_string(),
    }
}

/// One `key="value"` (or bare `key=value`) parameter from a `WWW-Authenticate` challenge.
fn challenge_param(header: &str, key: &str) -> Option<String> {
    let lower = header.to_ascii_lowercase();
    let needle = format!("{}=", key.to_ascii_lowercase());
    let mut from = 0;
    while let Some(at) = lower[from..].find(&needle) {
        let start = from + at;
        let boundary = start == 0 || matches!(lower.as_bytes()[start - 1], b' ' | b',' | b'\t');
        let after = start + needle.len();
        if boundary {
            let rest = &header[after..];
            let value = if let Some(q) = rest.strip_prefix('"') {
                q.split('"').next().unwrap_or("")
            } else {
                rest.split([',', ' ']).next().unwrap_or("")
            };
            let value = value.trim();
            return (!value.is_empty()).then(|| value.to_string());
        }
        from = after;
    }
    None
}

/// `resource_metadata` from a 401's `WWW-Authenticate: Bearer resource_metadata="…"`.
pub fn challenge_resource_metadata(www_authenticate: &str) -> Option<String> {
    challenge_param(www_authenticate, "resource_metadata")
}

/// `scope` from the same challenge, when the server names one.
pub fn challenge_scope(www_authenticate: &str) -> Option<String> {
    challenge_param(www_authenticate, "scope")
}

/// Where to look for protected resource metadata when the 401 named none:
/// the path-aware well-known URL, then the root one.
pub fn resource_metadata_urls(server_url: &str) -> Vec<String> {
    let Some((origin, path)) = split_origin(server_url) else {
        return Vec::new();
    };
    let root = format!("{origin}/.well-known/oauth-protected-resource");
    if path.is_empty() {
        vec![root]
    } else {
        vec![format!("{root}{path}"), root]
    }
}

/// The first authorization server and the supported scopes from protected resource metadata.
pub fn parse_resource_metadata(v: &Value) -> Result<(String, Vec<String>), String> {
    let issuer = v
        .get("authorization_servers")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().filter_map(Value::as_str).map(str::trim).find(|s| !s.is_empty()))
        .ok_or("the server's sign-in metadata names no authorization server")?;
    Ok((issuer.to_string(), strings(v.get("scopes_supported"))))
}

/// Authorization server metadata URLs to try, in the order the MCP spec gives.
pub fn auth_server_metadata_urls(issuer: &str) -> Vec<String> {
    let Some((origin, path)) = split_origin(issuer) else {
        return Vec::new();
    };
    if path.is_empty() {
        vec![
            format!("{origin}/.well-known/oauth-authorization-server"),
            format!("{origin}/.well-known/openid-configuration"),
        ]
    } else {
        vec![
            format!("{origin}/.well-known/oauth-authorization-server{path}"),
            format!("{origin}/.well-known/openid-configuration{path}"),
            format!("{origin}{path}/.well-known/openid-configuration"),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthServer {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub scopes_supported: Vec<String>,
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// An endpoint the sign-in may use: https, or http only when the MCP server itself is http
/// (a server on this machine or the LAN).
pub fn endpoint_allowed(endpoint: &str, server_url: &str) -> bool {
    let endpoint = endpoint.trim();
    endpoint.starts_with("https://")
        || (endpoint.starts_with("http://") && server_url.trim().starts_with("http://"))
}

pub fn parse_auth_server(v: &Value, server_url: &str) -> Result<AuthServer, String> {
    let take = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let authorization_endpoint = take("authorization_endpoint").ok_or("the sign-in server has no authorization endpoint")?;
    let token_endpoint = take("token_endpoint").ok_or("the sign-in server has no token endpoint")?;
    let registration_endpoint = take("registration_endpoint");
    for url in [Some(&authorization_endpoint), Some(&token_endpoint), registration_endpoint.as_ref()].into_iter().flatten() {
        if !endpoint_allowed(url, server_url) {
            return Err(format!("the sign-in server named an endpoint that isn't https: {url}"));
        }
    }
    let methods = strings(v.get("code_challenge_methods_supported"));
    if !methods.is_empty() && !methods.iter().any(|m| m == "S256") {
        return Err("the sign-in server doesn't support PKCE S256".into());
    }
    Ok(AuthServer {
        authorization_endpoint,
        token_endpoint,
        registration_endpoint,
        scopes_supported: strings(v.get("scopes_supported")),
    })
}

/// RFC 7591 registration for a public native client on the loopback redirect.
pub fn registration_request(redirect_uri: &str) -> Value {
    json!({
        "client_name": "GrokHub",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    })
}

#[derive(Clone, PartialEq, Eq)]
pub struct Client {
    pub client_id: String,
    pub client_secret: Option<String>,
}

pub fn parse_registration(v: &Value) -> Result<Client, String> {
    let client_id = v
        .get("client_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("the sign-in server registered no client id")?;
    let client_secret = v
        .get("client_secret")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(Client { client_id: client_id.to_string(), client_secret })
}

/// The scope to ask for: the 401's, else the resource's, else the server's, else none.
pub fn pick_scope(challenge: Option<&str>, resource: &[String], server: &[String]) -> Option<String> {
    if let Some(s) = challenge.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(s.to_string());
    }
    let list = if resource.is_empty() { server } else { resource };
    (!list.is_empty()).then(|| list.join(" "))
}

pub struct Authorize<'a> {
    pub authorization_endpoint: &'a str,
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub code_challenge: &'a str,
    pub state: &'a str,
    pub scope: Option<&'a str>,
    pub resource: &'a str,
}

pub fn authorize_url(p: &Authorize<'_>) -> String {
    let mut pairs = vec![
        ("response_type", "code"),
        ("client_id", p.client_id),
        ("redirect_uri", p.redirect_uri),
        ("code_challenge", p.code_challenge),
        ("code_challenge_method", "S256"),
        ("state", p.state),
    ];
    if let Some(scope) = p.scope {
        pairs.push(("scope", scope));
    }
    pairs.push(("resource", p.resource));
    let endpoint = p.authorization_endpoint.trim().trim_end_matches(['?', '&']);
    let sep = if endpoint.contains('?') { '&' } else { '?' };
    format!("{endpoint}{sep}{}", form(&pairs))
}

fn with_client<'a>(mut pairs: Vec<(&'a str, &'a str)>, client: &'a Client) -> String {
    pairs.push(("client_id", &client.client_id));
    if let Some(secret) = &client.client_secret {
        pairs.push(("client_secret", secret));
    }
    form(&pairs)
}

pub fn code_form(code: &str, verifier: &str, redirect_uri: &str, client: &Client, resource: &str) -> String {
    with_client(
        vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
            ("resource", resource),
        ],
        client,
    )
}

pub fn refresh_form(refresh_token: &str, client: &Client, resource: &str) -> String {
    with_client(
        vec![("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("resource", resource)],
        client,
    )
}

/// What a sign-in keeps for one server. Sealed at rest; `Debug` prints no secret.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpSignIn {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    pub token_endpoint: String,
    pub resource: String,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
    /// The account the token's id token names, when it has one.
    #[serde(default)]
    pub account: Option<String>,
}

impl std::fmt::Debug for McpSignIn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpSignIn")
            .field("client_id", &self.client_id)
            .field("token_endpoint", &self.token_endpoint)
            .field("resource", &self.resource)
            .field("expires_at", &self.expires_at)
            .field("account", &self.account)
            .field("tokens", &"[redacted]")
            .finish()
    }
}

impl McpSignIn {
    pub fn new(client: &Client, token_endpoint: &str, resource: &str, tokens: XaiOAuthTokens) -> Self {
        let mut out = Self {
            client_id: client.client_id.clone(),
            client_secret: client.client_secret.clone(),
            token_endpoint: token_endpoint.to_string(),
            resource: resource.to_string(),
            ..Self::default()
        };
        out.take_tokens(tokens);
        out
    }

    pub fn client(&self) -> Client {
        Client { client_id: self.client_id.clone(), client_secret: self.client_secret.clone() }
    }

    /// Store a token response. A refresh that sends no new refresh token or
    /// account keeps the old ones.
    pub fn take_tokens(&mut self, t: XaiOAuthTokens) {
        self.access_token = t.access_token;
        if t.refresh_token.is_some() {
            self.refresh_token = t.refresh_token;
        }
        self.expires_at = t.expires_at;
        if let Some(who) = t.email.or(t.name).filter(|s| !s.trim().is_empty()) {
            self.account = Some(who);
        }
    }

    /// Refresh now: the access token is within a minute of expiring and a refresh token exists.
    pub fn needs_refresh(&self, now_ms: u64) -> bool {
        self.refresh_token.is_some() && self.expires_at.is_some_and(|e| now_ms.saturating_add(REFRESH_SKEW_MS) >= e)
    }
}

/// "Signed in to Linear as ada@example.com", or without the account when the server sent none.
pub fn signed_in_line(server: &str, account: Option<&str>) -> String {
    match account.map(str::trim).filter(|s| !s.is_empty()) {
        Some(who) => format!("Signed in to {server} as {who}"),
        None => format!("Signed in to {server}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_resources_and_well_known_urls_follow_the_spec() {
        assert_eq!(
            split_origin("https://mcp.linear.app/mcp?x=1#f"),
            Some(("https://mcp.linear.app".into(), "/mcp".into()))
        );
        assert_eq!(split_origin("https://a.example:8443"), Some(("https://a.example:8443".into(), "".into())));
        assert_eq!(split_origin("ftp://a.example/x"), None);
        assert_eq!(resource_for("https://mcp.linear.app/mcp/?x=1"), "https://mcp.linear.app/mcp");
        assert_eq!(
            resource_metadata_urls("https://mcp.linear.app/mcp"),
            vec![
                "https://mcp.linear.app/.well-known/oauth-protected-resource/mcp".to_string(),
                "https://mcp.linear.app/.well-known/oauth-protected-resource".to_string(),
            ]
        );
        assert_eq!(
            auth_server_metadata_urls("https://auth.example.com"),
            vec![
                "https://auth.example.com/.well-known/oauth-authorization-server".to_string(),
                "https://auth.example.com/.well-known/openid-configuration".to_string(),
            ]
        );
        assert_eq!(
            auth_server_metadata_urls("https://auth.example.com/tenant1"),
            vec![
                "https://auth.example.com/.well-known/oauth-authorization-server/tenant1".to_string(),
                "https://auth.example.com/.well-known/openid-configuration/tenant1".to_string(),
                "https://auth.example.com/tenant1/.well-known/openid-configuration".to_string(),
            ]
        );
    }

    #[test]
    fn the_401_challenge_names_the_metadata_and_scope() {
        let h = r#"Bearer error="invalid_token", resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource", scope="read write""#;
        assert_eq!(
            challenge_resource_metadata(h).as_deref(),
            Some("https://mcp.example.com/.well-known/oauth-protected-resource")
        );
        assert_eq!(challenge_scope(h).as_deref(), Some("read write"));
        assert_eq!(challenge_resource_metadata("Bearer realm=\"x\""), None);
        assert_eq!(challenge_scope("Bearer Scope=tools"), Some("tools".into()));
        // `scope` inside another parameter's name is not a match.
        assert_eq!(challenge_scope(r#"Bearer xscope="no""#), None);
    }

    #[test]
    fn metadata_parsing_requires_s256_and_https_endpoints() {
        let prm = json!({"resource": "https://mcp.example.com/mcp", "authorization_servers": ["https://auth.example.com"], "scopes_supported": ["mcp"]});
        assert_eq!(parse_resource_metadata(&prm), Ok(("https://auth.example.com".into(), vec!["mcp".into()])));
        assert!(parse_resource_metadata(&json!({"authorization_servers": []})).is_err());
        let server = "https://mcp.example.com/mcp";
        let meta = json!({
            "authorization_endpoint": "https://auth.example.com/authorize",
            "token_endpoint": "https://auth.example.com/token",
            "registration_endpoint": "https://auth.example.com/register",
            "code_challenge_methods_supported": ["S256"],
        });
        assert_eq!(
            parse_auth_server(&meta, server),
            Ok(AuthServer {
                authorization_endpoint: "https://auth.example.com/authorize".into(),
                token_endpoint: "https://auth.example.com/token".into(),
                registration_endpoint: Some("https://auth.example.com/register".into()),
                scopes_supported: vec![],
            })
        );
        let plain = json!({"authorization_endpoint": "https://a/x", "token_endpoint": "https://a/t", "code_challenge_methods_supported": ["plain"]});
        assert_eq!(parse_auth_server(&plain, server), Err("the sign-in server doesn't support PKCE S256".into()));
        let http = json!({"authorization_endpoint": "http://a/x", "token_endpoint": "https://a/t"});
        assert_eq!(
            parse_auth_server(&http, server),
            Err("the sign-in server named an endpoint that isn't https: http://a/x".into())
        );
        // An http MCP server (this machine or the LAN) may use an http sign-in server.
        assert!(parse_auth_server(&http, "http://localhost:8080/mcp").is_ok());
    }

    #[test]
    fn registration_authorize_and_token_forms_are_exact() {
        assert_eq!(
            registration_request("http://127.0.0.1:5000/callback"),
            json!({
                "client_name": "GrokHub",
                "redirect_uris": ["http://127.0.0.1:5000/callback"],
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"],
                "token_endpoint_auth_method": "none",
            })
        );
        let client = parse_registration(&json!({"client_id": "c-1"})).unwrap();
        assert_eq!((client.client_id.as_str(), client.client_secret.as_deref()), ("c-1", None));
        assert!(parse_registration(&json!({"client_id": " "})).is_err());
        let url = authorize_url(&Authorize {
            authorization_endpoint: "https://auth.example.com/authorize",
            client_id: "c-1",
            redirect_uri: "http://127.0.0.1:5000/callback",
            code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
            state: "st",
            scope: Some("mcp read"),
            resource: "https://mcp.example.com/mcp",
        });
        assert_eq!(
            url,
            "https://auth.example.com/authorize?response_type=code&client_id=c-1&redirect_uri=http%3A%2F%2F127.0.0.1%3A5000%2Fcallback&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256&state=st&scope=mcp%20read&resource=https%3A%2F%2Fmcp.example.com%2Fmcp"
        );
        assert_eq!(
            code_form("the-code", "the-verifier", "http://127.0.0.1:5000/callback", &client, "https://mcp.example.com/mcp"),
            "grant_type=authorization_code&code=the-code&code_verifier=the-verifier&redirect_uri=http%3A%2F%2F127.0.0.1%3A5000%2Fcallback&resource=https%3A%2F%2Fmcp.example.com%2Fmcp&client_id=c-1"
        );
        let secret = Client { client_id: "c-2".into(), client_secret: Some("s-2".into()) };
        assert_eq!(
            refresh_form("rt-1", &secret, "https://mcp.example.com/mcp"),
            "grant_type=refresh_token&refresh_token=rt-1&resource=https%3A%2F%2Fmcp.example.com%2Fmcp&client_id=c-2&client_secret=s-2"
        );
        assert_eq!(pick_scope(Some("a"), &["b".into()], &["c".into()]).as_deref(), Some("a"));
        assert_eq!(pick_scope(None, &[], &["c".into(), "d".into()]).as_deref(), Some("c d"));
        assert_eq!(pick_scope(None, &[], &[]), None);
    }

    #[test]
    fn a_sign_in_refreshes_near_expiry_keeps_its_refresh_token_and_hides_secrets() {
        let client = Client { client_id: "c-1".into(), client_secret: None };
        let first = XaiOAuthTokens {
            access_token: "at-1".into(),
            refresh_token: Some("rt-1".into()),
            expires_at: Some(100_000),
            email: Some("ada@example.com".into()),
            ..XaiOAuthTokens::default()
        };
        let mut s = McpSignIn::new(&client, "https://auth.example.com/token", "https://mcp.example.com/mcp", first);
        assert!(!s.needs_refresh(39_999));
        assert!(s.needs_refresh(40_000), "a minute before expiry");
        s.take_tokens(XaiOAuthTokens { access_token: "at-2".into(), expires_at: Some(200_000), ..XaiOAuthTokens::default() });
        assert_eq!(s.access_token, "at-2");
        assert_eq!(s.refresh_token.as_deref(), Some("rt-1"));
        assert_eq!(s.account.as_deref(), Some("ada@example.com"));
        let dbg = format!("{s:?}");
        assert!(!dbg.contains("at-2") && !dbg.contains("rt-1"), "{dbg}");
        let no_refresh = McpSignIn { refresh_token: None, ..s.clone() };
        assert!(!no_refresh.needs_refresh(u64::MAX), "nothing to refresh with");
        assert_eq!(signed_in_line("Linear", s.account.as_deref()), "Signed in to Linear as ada@example.com");
        assert_eq!(signed_in_line("Linear", None), "Signed in to Linear");
    }
}
