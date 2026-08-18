//! Grok website / local connectors. grok.com + x.ai only unless the user adds a host.

pub const DEFAULT_CONNECTOR_HOSTS: &[&str] = &["grok.com", "x.ai", "api.x.ai"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorCmd {
    pub connector_id: String,
    pub tool: String,
    pub args: String,
}

pub fn connector_url_allowed(url: &str, extra: &[String]) -> bool {
    let host = host_of(url);
    let Some(host) = host else {
        return false;
    };
    DEFAULT_CONNECTOR_HOSTS
        .iter()
        .copied()
        .chain(extra.iter().map(|s| s.as_str()))
        .any(|h| {
            let h = h.trim().to_ascii_lowercase();
            !h.is_empty() && (host == h || host.ends_with(&format!(".{h}")))
        })
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split('/').next()?;
    if authority.contains('@') {
        return None;
    }
    let host = authority.split(':').next()?.to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

pub fn extract_connector_cmds(text: &str) -> Vec<ConnectorCmd> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("CONNECTOR_CMD:") else {
            continue;
        };
        let rest = rest.trim();
        let mut parts = rest.splitn(3, char::is_whitespace);
        let id = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        let tool = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        let args = parts.next().unwrap_or("").trim().to_string();
        if id.is_empty() || tool.is_empty() {
            continue;
        }
        out.push(ConnectorCmd {
            connector_id: if id == "gh" { "github".into() } else { id },
            tool,
            args,
        });
        if out.len() == 4 {
            break;
        }
    }
    out
}

pub fn parse_connector_args(args: &str) -> Vec<(String, String)> {
    let args = args.trim();
    if args.is_empty() {
        return vec![];
    }
    if !args.contains(':') {
        return vec![("q".into(), args.to_string()), ("query".into(), args.to_string())];
    }
    let mut out = Vec::new();
    let mut rest = args;
    while let Some(colon) = rest.find(':') {
        let key = rest[..colon]
            .split_whitespace()
            .last()
            .unwrap_or("")
            .to_ascii_lowercase();
        rest = rest[colon + 1..].trim_start();
        if key.is_empty() {
            break;
        }
        let val = if rest.starts_with('"') {
            let end = rest[1..].find('"').map(|i| i + 1).unwrap_or(rest.len());
            let v = rest[1..end].to_string();
            rest = rest.get(end + 1..).unwrap_or("");
            v
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let v = rest[..end].to_string();
            rest = rest.get(end..).unwrap_or("").trim_start();
            v
        };
        out.push((key, val));
    }
    if out.is_empty() {
        out.push(("q".into(), args.to_string()));
    }
    out
}

pub fn arg_of(args: &[(String, String)], keys: &[&str]) -> String {
    for k in keys {
        if let Some((_, v)) = args.iter().find(|(a, _)| a == k) {
            return v.clone();
        }
    }
    String::new()
}

/// GET path on api.github.com, or an error the cabin can show.
pub fn github_api_path(tool: &str, args: &str) -> Result<String, String> {
    let a = parse_connector_args(args);
    match tool {
        "user" | "me" => Ok("/user".into()),
        "list_repos" | "repos" => Ok("/user/repos?per_page=20&sort=updated".into()),
        "list_issues" | "issues" => {
            let mut repo = arg_of(&a, &["repo", "repository"]);
            if !repo.contains('/') {
                let q = arg_of(&a, &["q", "query"]);
                if q.contains('/') {
                    repo = q;
                }
            }
            if !repo.contains('/') {
                return Err(
                    "Need repo:owner/name  e.g. CONNECTOR_CMD: github list_issues repo:vercel/next.js"
                        .into(),
                );
            }
            Ok(format!("/repos/{repo}/issues?state=open&per_page=15"))
        }
        "search_code" | "code_search" => {
            let q = arg_of(&a, &["q", "query"]);
            let q = if q.is_empty() { args.trim().to_string() } else { q };
            if q.is_empty() {
                return Err("Need query:… for search_code".into());
            }
            Ok(format!("/search/code?q={}&per_page=10", urlencode(&q)))
        }
        "search_issues" => {
            let q = arg_of(&a, &["q", "query"]);
            let q = if q.is_empty() { args.trim().to_string() } else { q };
            if q.is_empty() {
                return Err("Need query:… for search_issues".into());
            }
            Ok(format!("/search/issues?q={}&per_page=10", urlencode(&q)))
        }
        "create_pr_comment" | "comment" => Err(
            "GitHub writes are not wired. Use user, list_repos, list_issues, search_code, search_issues.".into(),
        ),
        _ => Err(format!(
            "Unknown GitHub tool \"{tool}\". Try: user, list_repos, list_issues, search_code, search_issues"
        )),
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn map_website_connector_name(name: &str) -> Option<&'static str> {
    let k = name.trim().to_ascii_lowercase();
    let aliases = [
        ("github", "github"),
        ("git hub", "github"),
        ("notion", "notion"),
        ("microsoft teams", "teams"),
        ("teams", "teams"),
        ("outlook calendar", "outlook-calendar"),
        ("outlook", "outlook"),
        ("google calendar", "google-calendar"),
        ("google drive", "gdrive"),
        ("gdrive", "gdrive"),
        ("gmail", "gmail"),
        ("box", "box"),
        ("canva", "canva"),
        ("stripe", "stripe"),
        ("vercel", "vercel"),
        ("linear", "linear"),
    ];
    for (alias, id) in aliases {
        if alias_hits(&k, alias) {
            return Some(id);
        }
    }
    None
}

fn alias_hits(name: &str, alias: &str) -> bool {
    if name == alias {
        return true;
    }
    if alias.contains(' ') && name.contains(alias) {
        return true;
    }
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|tok| tok == alias)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_and_cmds() {
        assert!(connector_url_allowed("https://grok.com/rest/connectors", &[]));
        assert!(connector_url_allowed("https://api.x.ai/v1/chat", &[]));
        assert!(!connector_url_allowed("https://evil.example/x", &[]));
        assert!(
            !connector_url_allowed("http://grok.com/rest/connectors", &[]),
            "cleartext connector URLs are not allowed"
        );
        assert!(
            !connector_url_allowed("https://evil.com@grok.com/rest/connectors", &[]),
            "userinfo must not impersonate an allowlisted host"
        );
        assert_eq!(map_website_connector_name("teamspeak"), None);
        assert_eq!(map_website_connector_name("inbox"), None);
        assert_eq!(map_website_connector_name("Microsoft Teams"), Some("teams"));
        assert!(connector_url_allowed(
            "https://notes.example/x",
            &["example".into()]
        ));
        let cmds = extract_connector_cmds(
            "ok\nCONNECTOR_CMD: github search_code query:foo\nCONNECTOR_CMD: gh user\n",
        );
        assert_eq!(cmds[0].connector_id, "github");
        assert_eq!(cmds[0].tool, "search_code");
        assert_eq!(cmds[1].connector_id, "github");
        assert_eq!(map_website_connector_name("GitHub"), Some("github"));
        assert_eq!(github_api_path("user", "").as_deref(), Ok("/user"));
        assert!(github_api_path("list_issues", "repo:vercel/next.js")
            .unwrap()
            .contains("/repos/vercel/next.js/issues"));
        assert!(github_api_path("list_issues", "").is_err());
        assert!(github_api_path("list_issues", "query:owner/name").is_ok());
        assert!(github_api_path("search_issues", "").is_err());
        assert!(github_api_path("create_pr_comment", "repo:a/b issue:1 body:hi").is_err());
        assert_eq!(arg_of(&parse_connector_args("query:foo language:ts"), &["query"]), "foo");
    }
}
