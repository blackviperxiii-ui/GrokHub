//! Settings → Connectors → Marketplace: the curated MCP catalog shipped in
//! the repo (`data/mcp_catalog.json`). Each entry carries its install
//! recipe (a stdio command or a remote URL) and how it signs in; installing
//! writes [`CatalogEntry::config_entry`] into the cabin's `mcp.json`, which
//! never holds a credential.

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

/// The catalog shipped with this build.
pub fn bundled() -> Catalog {
    parse(BUNDLED).expect("data/mcp_catalog.json parses")
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
        let filled = [&self.name, &self.publisher, &self.category, &self.description, &self.logo, &self.source, &self.permissions];
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
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
            _ => return Err(format!("MCP catalog: `{id}` needs a command (stdio) or an https url (http)")),
        }
        if self.auth == CatalogAuth::Key && self.transport == "stdio" && self.key_env.is_empty() {
            return Err(format!("MCP catalog: `{id}` takes a key but names no key_env"));
        }
        if self.auth == CatalogAuth::Oauth && self.transport != "http" {
            return Err(format!("MCP catalog: `{id}` signs in in the browser, so it must be http"));
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
            || [&self.name, &self.publisher, &self.category, &self.description, &self.id]
                .iter()
                .any(|f| f.to_lowercase().contains(&q))
            || self.capabilities.iter().any(|c| c.to_lowercase().contains(&q))
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
        for id in ["playwright", "github", "context7", "notion", "linear", "gmail", "google-drive", "google-calendar"] {
            assert!(c.get(id).is_some(), "{id} is in the catalog");
        }
    }

    #[test]
    fn config_entries_carry_the_recipe_and_no_credentials() {
        let c = bundled();
        assert_eq!(
            c.get("playwright").unwrap().config_entry(),
            json!({"command": "npx", "args": ["-y", "@playwright/mcp@latest"]})
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
        assert!(names(c.search("", Some("Google"))).iter().all(|id| c.get(id).unwrap().category == "Google"));
        assert_eq!(names(c.search("", Some("Google"))), ["google-drive", "gmail", "google-calendar"]);
        assert_eq!(names(c.search("gmail", Some("Developer"))), Vec::<String>::new());
        assert_eq!(c.search("", None).len(), c.entries.len());
        let cats = c.categories();
        assert_eq!(cats.first(), Some(&"Developer"));
        assert!(cats.contains(&"Google"));
    }

    #[test]
    fn a_broken_entry_is_refused() {
        let base = r#"{"id":"x","name":"X","publisher":"P","category":"C","description":"d","logo":"X","transport":"stdio","command":["x"],"auth":"none","source":"https://x.dev","capabilities":["a"],"permissions":"p"}"#;
        assert!(parse(&format!(r#"{{"entries":[{base}]}}"#)).is_ok());
        let bad = |from: &str, to: &str| parse(&format!(r#"{{"entries":[{}]}}"#, base.replace(from, to))).unwrap_err();
        assert_eq!(bad(r#""id":"x""#, r#""id":"X Y""#), "MCP catalog: `X Y` must be kebab-case");
        assert_eq!(bad(r#""command":["x"]"#, r#""command":[]"#), "MCP catalog: `x` needs a command (stdio) or an https url (http)");
        assert_eq!(bad(r#""auth":"none""#, r#""auth":"key""#), "MCP catalog: `x` takes a key but names no key_env");
        assert_eq!(bad(r#""auth":"none""#, r#""auth":"oauth""#), "MCP catalog: `x` signs in in the browser, so it must be http");
        assert_eq!(bad(r#""source":"https://x.dev""#, r#""source":"http://x.dev""#), "MCP catalog: `x` source must be https");
    }
}
