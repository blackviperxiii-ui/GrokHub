//! Settings → Connectors → Marketplace: the curated MCP catalog shipped in
//! the repo (`data/mcp_catalog.json`). Each entry carries its install
//! recipe (a stdio command or a remote URL) and how it signs in; installing
//! writes [`CatalogEntry::config_entry`] into the cabin's `mcp.json`, which
//! never holds a credential.

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Map, Value};

const BUNDLED: &str = include_str!("../data/mcp_catalog.json");

/// How a connector signs in after it is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogAuth {
    /// Works as soon as it is installed.
    None,
    /// The server's own browser sign-in.
    Oauth,
    /// An API key the user types, sealed in the cabin.
    Key,
    /// A Google service: set up through Google Cloud.
    Google,
    /// Signs in on its own the first time it runs (its own browser window
    /// or an existing CLI sign-in).
    Own,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub category: String,
    pub description: String,
    /// One or two letters for the row badge.
    pub logo: String,
    /// `stdio` or `http`.
    pub transport: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub url: String,
    pub auth: CatalogAuth,
    /// The env var a stdio server reads its key from (`auth: key`).
    #[serde(default)]
    pub key_env: String,
    /// Where the user gets the key, shown beside the key field.
    #[serde(default)]
    pub key_hint: String,
    #[serde(default)]
    pub registry_id: String,
    pub source: String,
    pub capabilities: Vec<String>,
    /// What it can reach, in one line, for the detail view.
    pub permissions: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    pub entries: Vec<CatalogEntry>,
}

/// Display metadata taken from the official MCP registry (never install recipes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryMeta {
    pub name: String,
    pub description: String,
    pub source: String,
}

const MAX_REGISTRY_BYTES: usize = 2 * 1024 * 1024;

/// The catalog shipped with this build. A test pins that it parses; if it ever
/// didn't, Marketplace shows an empty list rather than taking the app down.
pub fn bundled() -> Catalog {
    parse(BUNDLED).unwrap_or(Catalog {
        entries: Vec::new(),
    })
}

pub fn parse(text: &str) -> Result<Catalog, String> {
    let catalog: Catalog = serde_json::from_str(text).map_err(|e| format!("MCP catalog: {e}"))?;
    for e in &catalog.entries {
        e.check()?;
    }
    Ok(catalog)
}

impl CatalogEntry {
    fn check(&self) -> Result<(), String> {
        let id = &self.id;
        let filled = [
            &self.name,
            &self.publisher,
            &self.category,
            &self.description,
            &self.logo,
            &self.source,
            &self.permissions,
        ];
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(format!("MCP catalog: `{id}` must be kebab-case"));
        }
        if filled.iter().any(|f| f.trim().is_empty()) || self.capabilities.is_empty() {
            return Err(format!("MCP catalog: `{id}` is missing a field"));
        }
        if !self.source.starts_with("https://") {
            return Err(format!("MCP catalog: `{id}` source must be https"));
        }
        match self.transport.as_str() {
            "stdio" if !self.command.is_empty() && self.url.is_empty() => {}
            "http" if self.url.starts_with("https://") && self.command.is_empty() => {}
            _ => {
                return Err(format!(
                    "MCP catalog: `{id}` needs a command (stdio) or an https url (http)"
                ))
            }
        }
        if self.auth == CatalogAuth::Key && self.transport == "stdio" && self.key_env.is_empty() {
            return Err(format!(
                "MCP catalog: `{id}` takes a key but names no key_env"
            ));
        }
        if self.auth == CatalogAuth::Oauth && self.transport != "http" {
            return Err(format!(
                "MCP catalog: `{id}` signs in in the browser, so it must be http"
            ));
        }
        Ok(())
    }

    /// The `mcpServers` value Install writes. No credentials: a key is
    /// saved separately and the entry only names it.
    pub fn config_entry(&self) -> Value {
        let mut out = Map::new();
        if self.transport == "http" {
            out.insert("url".into(), json!(self.url));
            out.insert("type".into(), json!("http"));
        } else {
            out.insert("type".into(), json!("stdio"));
            out.insert("command".into(), json!(self.command[0]));
            if self.command.len() > 1 {
                out.insert("args".into(), json!(self.command[1..]));
            }
            if self.auth == CatalogAuth::Key {
                out.insert("tokenEnv".into(), json!(self.key_env));
            }
        }
        Value::Object(out)
    }

    /// What Install runs or reaches, for the detail view.
    pub fn target(&self) -> String {
        if self.transport == "http" {
            self.url.clone()
        } else {
            self.command.join(" ")
        }
    }

    fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || [
                &self.name,
                &self.publisher,
                &self.category,
                &self.description,
                &self.id,
            ]
            .iter()
            .any(|f| f.to_lowercase().contains(&q))
            || self
                .capabilities
                .iter()
                .any(|c| c.to_lowercase().contains(&q))
    }
}

impl Catalog {
    /// Categories in catalog order, each once.
    pub fn categories(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for e in &self.entries {
            if !out.contains(&e.category.as_str()) {
                out.push(&e.category);
            }
        }
        out
    }

    /// Entries matching `query` (name, publisher, description, capabilities)
    /// in `category` (`None` for all), in catalog order.
    pub fn search(&self, query: &str, category: Option<&str>) -> Vec<&CatalogEntry> {
        self.entries
            .iter()
            .filter(|e| category.is_none_or(|c| e.category == c) && e.matches(query))
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<&CatalogEntry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

fn encode_registry_server(name: &str) -> String {
    name.replace('/', "%2F")
}

fn registry_versions_url(registry_id: &str) -> Result<String, String> {
    let id = registry_id.trim();
    if id.is_empty() {
        return Err("MCP registry id is empty".into());
    }
    let url = format!(
        "https://{}/v0.1/servers/{}/versions",
        crate::ssrf::MCP_REGISTRY_HOST,
        encode_registry_server(id)
    );
    crate::ssrf::guard_mcp_registry_url(&url)?;
    Ok(url)
}

fn registry_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .redirects(0)
        .resolver(crate::ssrf::public_only_resolve)
        .build()
}

fn read_registry_body(resp: ureq::Response) -> Result<String, String> {
    if !(200..300).contains(&resp.status()) {
        return Err(format!("MCP registry: http {}", resp.status()));
    }
    let mut buf = Vec::new();
    resp.into_reader()
        .take((MAX_REGISTRY_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|err| format!("MCP registry: {err}"))?;
    if buf.len() > MAX_REGISTRY_BYTES {
        return Err("MCP registry response is too large".into());
    }
    String::from_utf8(buf).map_err(|_| "MCP registry response is not utf-8".to_string())
}

/// GET one server's version list from the official registry (https + allowlist + SSRF rail).
pub fn fetch_registry_versions(registry_id: &str) -> Result<String, String> {
    let url = registry_versions_url(registry_id)?;
    let resp = registry_agent()
        .get(&url)
        .call()
        .map_err(|err| format!("MCP registry: {err}"))?;
    read_registry_body(resp)
}

/// Parse a registry versions response and return metadata for the latest active row.
pub fn parse_registry_versions(body: &str) -> Result<Option<RegistryMeta>, String> {
    let root: Value = serde_json::from_str(body).map_err(|e| format!("MCP registry json: {e}"))?;
    let servers = root
        .get("servers")
        .and_then(Value::as_array)
        .ok_or_else(|| "MCP registry json: missing servers".to_string())?;
    let mut pick: Option<(bool, RegistryMeta)> = None;
    for row in servers {
        let server = row
            .get("server")
            .ok_or_else(|| "MCP registry json: missing server".to_string())?;
        let name = server
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "MCP registry json: missing server name".to_string())?;
        let official = row
            .get("_meta")
            .and_then(|m| m.get("io.modelcontextprotocol.registry/official"));
        let status = official
            .and_then(|m| m.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("active");
        if status == "deleted" {
            continue;
        }
        let is_latest = official
            .and_then(|m| m.get("isLatest"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let description = server
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let display = server
            .get("title")
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
            .or_else(|| {
                server
                    .get("_meta")
                    .and_then(|m| m.get("io.modelcontextprotocol.registry/publisher-provided"))
                    .and_then(|p| p.get("title"))
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| name.to_string());
        let source = registry_source_url(server).unwrap_or_default();
        let meta = RegistryMeta {
            name: display,
            description,
            source,
        };
        let replace = match &pick {
            None => true,
            Some((was_latest, _)) => is_latest || !was_latest,
        };
        if replace {
            pick = Some((is_latest, meta));
        }
        if is_latest {
            break;
        }
    }
    Ok(pick.map(|(_, meta)| meta))
}

fn registry_source_url(server: &Value) -> Option<String> {
    let from_repo = server
        .get("repository")
        .and_then(|r| r.get("url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|u| u.starts_with("https://"));
    if from_repo.is_some() {
        return from_repo.map(str::to_string);
    }
    server
        .get("websiteUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|u| u.starts_with("https://"))
        .map(str::to_string)
}

fn apply_registry_meta(entry: &mut CatalogEntry, meta: &RegistryMeta) -> bool {
    let before = (
        entry.name.clone(),
        entry.description.clone(),
        entry.source.clone(),
    );
    if !meta.name.trim().is_empty() {
        entry.name = meta.name.trim().to_string();
    }
    if !meta.description.is_empty() {
        entry.description = meta.description.clone();
    }
    if meta.source.starts_with("https://") {
        entry.source = meta.source.clone();
    }
    if entry.check().is_err() {
        entry.name = before.0;
        entry.description = before.1;
        entry.source = before.2;
        return false;
    }
    before
        != (
            entry.name.clone(),
            entry.description.clone(),
            entry.source.clone(),
        )
}

/// Merge registry metadata into a catalog copy. Only rows with a matching
/// `registry_id` are updated; install recipes are untouched. Rows in the
/// registry that are not already listed are ignored.
pub fn merge_registry_metadata(
    catalog: &Catalog,
    by_registry_id: &HashMap<String, RegistryMeta>,
) -> Catalog {
    let mut out = catalog.clone();
    for entry in &mut out.entries {
        if entry.registry_id.is_empty() {
            continue;
        }
        if let Some(meta) = by_registry_id.get(&entry.registry_id) {
            apply_registry_meta(entry, meta);
        }
    }
    out
}

/// Fetch the official registry for every bundled `registry_id` and merge metadata.
pub fn refresh_metadata_from_registry(catalog: &Catalog) -> Result<(Catalog, u32), String> {
    let agent = registry_agent();
    let mut by_id = HashMap::new();
    let mut seen = HashMap::new();
    for entry in &catalog.entries {
        let id = entry.registry_id.trim();
        if id.is_empty() || seen.contains_key(id) {
            continue;
        }
        seen.insert(id.to_string(), ());
        let url = registry_versions_url(id)?;
        let resp = agent
            .get(&url)
            .call()
            .map_err(|err| format!("MCP registry: {err}"))?;
        let text = read_registry_body(resp)?;
        if let Some(meta) = parse_registry_versions(&text)? {
            by_id.insert(id.to_string(), meta);
        }
    }
    let merged = merge_registry_metadata(catalog, &by_id);
    let mut updates = 0u32;
    for (a, b) in catalog.entries.iter().zip(&merged.entries) {
        if a.name != b.name || a.description != b.description || a.source != b.source {
            updates += 1;
        }
    }
    Ok((merged, updates))
}

/// Refresh starting from the catalog shipped in this build.
pub fn refresh_from_official_registry() -> Result<(Catalog, u32), String> {
    refresh_metadata_from_registry(&bundled())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_catalog_parses_with_every_field() {
        let c = bundled();
        assert!(c.entries.len() >= 25, "{} entries", c.entries.len());
        let mut ids: Vec<&str> = c.entries.iter().map(|e| e.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), c.entries.len(), "ids are unique");
        for id in [
            "playwright",
            "github",
            "context7",
            "notion",
            "linear",
            "gmail",
            "google-drive",
            "google-calendar",
        ] {
            assert!(c.get(id).is_some(), "{id} is in the catalog");
        }
    }

    #[test]
    fn config_entries_carry_the_recipe_and_no_credentials() {
        let c = bundled();
        assert_eq!(
            c.get("playwright").unwrap().config_entry(),
            json!({
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "@playwright/mcp@latest"]
            })
        );
        assert_eq!(
            c.get("linear").unwrap().config_entry(),
            json!({"url": "https://mcp.linear.app/mcp", "type": "http"})
        );
        for e in &c.entries {
            let v = e.config_entry();
            for k in ["headers", "env", "bearerToken", "tokenRef"] {
                assert!(v.get(k).is_none(), "{} carries {k}", e.id);
            }
            if e.auth == CatalogAuth::Key && e.transport == "stdio" {
                assert_eq!(v["tokenEnv"], json!(e.key_env), "{}", e.id);
            }
        }
    }

    #[test]
    fn search_and_category_filter() {
        let c = bundled();
        let names = |v: Vec<&CatalogEntry>| v.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(names(c.search("LINEAR", None)), ["linear"]);
        assert!(names(c.search("", Some("Google")))
            .iter()
            .all(|id| c.get(id).unwrap().category == "Google"));
        assert_eq!(
            names(c.search("", Some("Google"))),
            ["google-drive", "gmail", "google-calendar"]
        );
        assert_eq!(
            names(c.search("gmail", Some("Developer"))),
            Vec::<String>::new()
        );
        assert_eq!(c.search("", None).len(), c.entries.len());
        let cats = c.categories();
        assert_eq!(cats.first(), Some(&"Developer"));
        assert!(cats.contains(&"Google"));
    }

    #[test]
    fn registry_fixture_parses_and_merges_without_touching_install_recipes() {
        let fixture = include_str!("../tests/fixtures/mcp_registry_playwright_versions.json");
        let meta = parse_registry_versions(fixture)
            .expect("fixture parses")
            .expect("latest row");
        assert_eq!(
            meta.description,
            "Playwright Tools for MCP (registry latest)"
        );
        assert_eq!(meta.source, "https://github.com/microsoft/playwright-mcp");

        let base = bundled();
        let before = base.get("playwright").unwrap().clone();
        let map = HashMap::from([(before.registry_id.clone(), meta)]);
        let merged = merge_registry_metadata(&base, &map);
        let after = merged.get("playwright").unwrap();
        assert_eq!(
            after.description,
            "Playwright Tools for MCP (registry latest)"
        );
        assert_eq!(after.config_entry(), before.config_entry());
        assert_eq!(merged.entries.len(), base.entries.len());
        assert!(merged.get("slack").is_none());
    }

    #[test]
    fn registry_fetch_url_rejects_non_allowlisted_hosts() {
        assert!(registry_versions_url("io.github.microsoft/playwright-mcp").is_ok());
        let bad = "https://evil.example/v0.1/servers/x/versions";
        assert_eq!(
            crate::ssrf::guard_mcp_registry_url(bad).unwrap_err(),
            "MCP registry host is not allowlisted"
        );
    }

    #[test]
    fn registry_network_failure_leaves_the_bundled_catalog() {
        let base = bundled();
        let err = refresh_metadata_from_registry(&Catalog {
            entries: vec![CatalogEntry {
                id: "probe".into(),
                name: "Probe".into(),
                publisher: "P".into(),
                category: "Developer".into(),
                description: "d".into(),
                logo: "P".into(),
                transport: "http".into(),
                command: Vec::new(),
                url: "https://mcp.example.com/mcp".into(),
                auth: CatalogAuth::None,
                key_env: String::new(),
                key_hint: String::new(),
                registry_id: "io.github.example/probe".into(),
                source: "https://example.com".into(),
                capabilities: vec!["a".into()],
                permissions: "p".into(),
            }],
        })
        .unwrap_err();
        assert!(
            err.contains("MCP registry"),
            "expected registry fetch error, got {err}"
        );
        assert_eq!(
            base.get("playwright").unwrap().description,
            bundled().get("playwright").unwrap().description
        );
    }

    #[test]
    fn a_broken_entry_is_refused() {
        let base = r#"{"id":"x","name":"X","publisher":"P","category":"C","description":"d","logo":"X","transport":"stdio","command":["x"],"auth":"none","source":"https://x.dev","capabilities":["a"],"permissions":"p"}"#;
        assert!(parse(&format!(r#"{{"entries":[{base}]}}"#)).is_ok());
        let bad = |from: &str, to: &str| {
            parse(&format!(r#"{{"entries":[{}]}}"#, base.replace(from, to))).unwrap_err()
        };
        assert_eq!(
            bad(r#""id":"x""#, r#""id":"X Y""#),
            "MCP catalog: `X Y` must be kebab-case"
        );
        assert_eq!(
            bad(r#""command":["x"]"#, r#""command":[]"#),
            "MCP catalog: `x` needs a command (stdio) or an https url (http)"
        );
        assert_eq!(
            bad(r#""auth":"none""#, r#""auth":"key""#),
            "MCP catalog: `x` takes a key but names no key_env"
        );
        assert_eq!(
            bad(r#""auth":"none""#, r#""auth":"oauth""#),
            "MCP catalog: `x` signs in in the browser, so it must be http"
        );
        assert_eq!(
            bad(r#""source":"https://x.dev""#, r#""source":"http://x.dev""#),
            "MCP catalog: `x` source must be https"
        );
    }
}
