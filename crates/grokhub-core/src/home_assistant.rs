//! First-class Home Assistant / local device control.
//!
//! Opt-in: Settings stores consent + base URL in `app.json`; the long-lived
//! access token stays in the OS keychain. The chat only gets the tools when
//! consent is on, the URL is set, and a token is present. Dangerous HA service
//! domains are refused before any POST. Dialing uses [`crate::http`] with
//! `saved_host` so traffic stays on the configured server.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::http::{self, Agent, Opts};

/// `app.json` key for the nested settings object.
pub const SETTINGS_KEY: &str = "homeAssistant";
/// Keychain account for the long-lived access token (never in `app.json`).
pub const KEYRING_ACCOUNT: &str = "home-assistant-token";

/// Default base URL when the user has not set one yet (hint only).
pub const DEFAULT_URL_HINT: &str = "http://homeassistant.local:8123";

/// Settings → Your PC → PC helpers. Off by default; control needs consent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeAssistantSettings {
    /// Explicit user consent to let the chat list, read, and control devices.
    #[serde(default)]
    pub enabled: bool,
    /// Home Assistant base URL (`http://…` or `https://…`), no trailing slash required.
    #[serde(default)]
    pub base_url: String,
}

impl HomeAssistantSettings {
    pub fn is_default(&self) -> bool {
        !self.enabled && self.base_url.trim().is_empty()
    }

    /// True when consent is on and a usable base URL is configured.
    pub fn ready_for_tools(&self) -> bool {
        self.enabled && normalize_base_url(&self.base_url).is_ok()
    }
}

/// Service domains that must never be called through Loadout.
pub const BLOCKED_DOMAINS: &[&str] = &[
    "shell_command",
    "command_line",
    "python_script",
    "pyscript",
    "hassio",
    "rest_command",
];

/// Normalize and validate the Home Assistant base URL.
pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("Add your Home Assistant address first.".into());
    }
    let url = url::Url::parse(raw).map_err(|_| {
        "Home Assistant address must be a full URL, like http://192.168.1.10:8123.".to_string()
    })?;
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "Home Assistant address must use http or https, not {other}."
            ))
        }
    }
    if url.host_str().is_none() {
        return Err("Home Assistant address needs a host.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Put the access token in the keychain field, not in the URL.".into());
    }
    // Drop path/query/fragment so callers always hit `/api/…` on the root.
    let mut clean = url.clone();
    clean.set_path("");
    clean.set_query(None);
    clean.set_fragment(None);
    let mut s = clean.to_string();
    while s.ends_with('/') {
        s.pop();
    }
    Ok(s)
}

fn is_slug_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
}

/// Domain or service name: lowercase letter, then letters/digits/underscore.
pub fn valid_slug(name: &str) -> bool {
    let name = name.trim();
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => chars.all(is_slug_char),
        _ => false,
    }
}

/// Entity id shape: `domain.object_id` with safe characters only.
pub fn valid_entity_id(id: &str) -> bool {
    let id = id.trim();
    let Some((domain, object)) = id.split_once('.') else {
        return false;
    };
    if object.contains('.') {
        return false;
    }
    valid_slug(domain) && !object.is_empty() && object.chars().all(is_slug_char)
}

pub fn domain_blocked(domain: &str) -> bool {
    BLOCKED_DOMAINS.contains(&domain.trim())
}

/// One entity row for the model (and UI test connection summaries).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitySummary {
    pub entity_id: String,
    pub state: String,
    pub friendly_name: String,
}

/// Parse `/api/states` JSON into summaries, optionally filtered by domain and
/// a case-insensitive area/name needle.
pub fn summarize_states(
    body: &str,
    domain: Option<&str>,
    area: Option<&str>,
) -> Result<Vec<EntitySummary>, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| format!("Home Assistant states were not JSON: {e}"))?;
    let rows = value
        .as_array()
        .ok_or_else(|| "Home Assistant states were not a list.".to_string())?;
    let domain = domain.map(str::trim).filter(|s| !s.is_empty());
    let area_needle = area
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    let mut out = Vec::new();
    for row in rows {
        let entity_id = row
            .get("entity_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if entity_id.is_empty() {
            continue;
        }
        if let Some(d) = domain {
            let prefix = format!("{d}.");
            if !entity_id.starts_with(&prefix) {
                continue;
            }
        }
        let state = row
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let friendly = row
            .get("attributes")
            .and_then(|a| a.get("friendly_name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if let Some(needle) = &area_needle {
            let hay = format!("{friendly} {entity_id}").to_ascii_lowercase();
            if !hay.contains(needle) {
                continue;
            }
        }
        out.push(EntitySummary {
            entity_id: entity_id.to_string(),
            state,
            friendly_name: friendly,
        });
    }
    out.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
    Ok(out)
}

/// Format entity summaries for the chat model.
pub fn format_entities(rows: &[EntitySummary]) -> String {
    if rows.is_empty() {
        return "No matching Home Assistant entities.".into();
    }
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(format!("{} entities:", rows.len()));
    for row in rows {
        let name = if row.friendly_name.is_empty() {
            "—"
        } else {
            row.friendly_name.as_str()
        };
        lines.push(format!("- {} [{}] {}", row.entity_id, row.state, name));
    }
    lines.join("\n")
}

/// Format one `/api/states/<id>` object for the model.
pub fn format_state(body: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| format!("Home Assistant state was not JSON: {e}"))?;
    let entity_id = v
        .get("entity_id")
        .and_then(Value::as_str)
        .unwrap_or("(unknown)");
    let state = v.get("state").and_then(Value::as_str).unwrap_or("");
    let attrs = v.get("attributes").cloned().unwrap_or(json!({}));
    Ok(format!(
        "entity_id: {entity_id}\nstate: {state}\nattributes:\n{}",
        serde_json::to_string_pretty(&attrs).unwrap_or_else(|_| "{}".into())
    ))
}

/// Format `/api/services` JSON, optionally filtered by domain.
pub fn format_services(body: &str, domain: Option<&str>) -> Result<String, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| format!("Home Assistant services were not JSON: {e}"))?;
    let rows = value
        .as_array()
        .ok_or_else(|| "Home Assistant services were not a list.".to_string())?;
    let domain = domain.map(str::trim).filter(|s| !s.is_empty());
    let mut lines = Vec::new();
    for row in rows {
        let d = row.get("domain").and_then(Value::as_str).unwrap_or("");
        if d.is_empty() {
            continue;
        }
        if domain.is_some_and(|want| want != d) {
            continue;
        }
        if domain_blocked(d) {
            continue;
        }
        let services = row.get("services").unwrap_or(&Value::Null);
        let names: Vec<&str> = match services {
            Value::Object(map) => map.keys().map(|k| k.as_str()).collect(),
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if names.is_empty() {
            lines.push(format!("- {d}: (no listed services)"));
        } else {
            let mut names = names;
            names.sort_unstable();
            lines.push(format!("- {d}: {}", names.join(", ")));
        }
    }
    if lines.is_empty() {
        return Ok("No matching Home Assistant services.".into());
    }
    lines.sort();
    Ok(format!("{} domains:\n{}", lines.len(), lines.join("\n")))
}

/// Gate a service call before any network I/O.
pub fn gate_call(domain: &str, service: &str, entity_id: Option<&str>) -> Result<(), String> {
    let domain = domain.trim();
    let service = service.trim();
    if !valid_slug(domain) {
        return Err("Service domain must look like `light` or `switch`.".into());
    }
    if domain_blocked(domain) {
        return Err(format!(
            "Refused: `{domain}` services are blocked for safety."
        ));
    }
    if !valid_slug(service) {
        return Err("Service name must look like `turn_on` or `set_temperature`.".into());
    }
    if let Some(id) = entity_id.map(str::trim).filter(|s| !s.is_empty()) {
        if !valid_entity_id(id) {
            return Err(format!(
                "Entity id `{id}` is not valid (expected domain.object_id)."
            ));
        }
    }
    Ok(())
}

fn agent() -> Agent {
    http::agent(Opts {
        connect: Duration::from_secs(5),
        overall: Some(Duration::from_secs(20)),
        redirects: 0,
        saved_host: true,
        ..Opts::default()
    })
}

fn auth_header(token: &str) -> String {
    format!("Bearer {}", token.trim())
}

fn get_json(base: &str, token: &str, path: &str) -> Result<String, String> {
    let url = format!("{base}{path}");
    http::with_saved_host(base, || {
        agent()
            .get(&url)
            .header("Authorization", &auth_header(token))
            .header("Content-Type", "application/json")
            .call()
            .map_err(|e| e.to_string())?
            .into_string()
            .map_err(|e| e.to_string())
    })
}

fn post_json(base: &str, token: &str, path: &str, body: &Value) -> Result<String, String> {
    let url = format!("{base}{path}");
    http::with_saved_host(base, || {
        agent()
            .post(&url)
            .header("Authorization", &auth_header(token))
            .send_json(body)
            .map_err(|e| e.to_string())?
            .into_string()
            .map_err(|e| e.to_string())
    })
}

/// Probe `/api/` — used by Settings → Test connection.
pub fn ping(base_url: &str, token: &str) -> Result<String, String> {
    let base = normalize_base_url(base_url)?;
    if token.trim().is_empty() {
        return Err("Add a Home Assistant long-lived access token first.".into());
    }
    let body = get_json(&base, token, "/api/")?;
    Ok(body.trim().to_string())
}

pub fn list_entities(
    base_url: &str,
    token: &str,
    domain: Option<&str>,
    area: Option<&str>,
) -> Result<String, String> {
    let base = normalize_base_url(base_url)?;
    if let Some(d) = domain.map(str::trim).filter(|s| !s.is_empty()) {
        if !valid_slug(d) {
            return Err("Domain filter must look like `light` or `sensor`.".into());
        }
    }
    let body = get_json(&base, token, "/api/states")?;
    let rows = summarize_states(&body, domain, area)?;
    Ok(format_entities(&rows))
}

pub fn get_state(base_url: &str, token: &str, entity_id: &str) -> Result<String, String> {
    let base = normalize_base_url(base_url)?;
    let id = entity_id.trim();
    if !valid_entity_id(id) {
        return Err(format!(
            "Entity id `{id}` is not valid (expected domain.object_id)."
        ));
    }
    let body = get_json(&base, token, &format!("/api/states/{id}"))?;
    format_state(&body)
}

pub fn list_services(base_url: &str, token: &str, domain: Option<&str>) -> Result<String, String> {
    let base = normalize_base_url(base_url)?;
    if let Some(d) = domain.map(str::trim).filter(|s| !s.is_empty()) {
        if !valid_slug(d) {
            return Err("Domain filter must look like `light` or `climate`.".into());
        }
        if domain_blocked(d) {
            return Err(format!("Refused: `{d}` services are blocked for safety."));
        }
    }
    let body = get_json(&base, token, "/api/services")?;
    format_services(&body, domain)
}

pub fn call_service(
    base_url: &str,
    token: &str,
    domain: &str,
    service: &str,
    entity_id: Option<&str>,
    data: Option<&Map<String, Value>>,
) -> Result<String, String> {
    let base = normalize_base_url(base_url)?;
    gate_call(domain, service, entity_id)?;
    let mut body = data.cloned().unwrap_or_default();
    if let Some(id) = entity_id.map(str::trim).filter(|s| !s.is_empty()) {
        body.insert("entity_id".into(), Value::String(id.to_string()));
    }
    let path = format!("/api/services/{}/{}", domain.trim(), service.trim());
    let resp = post_json(&base, token, &path, &Value::Object(body))?;
    if resp.trim().is_empty() {
        Ok(format!(
            "Called {}.{} successfully.",
            domain.trim(),
            service.trim()
        ))
    } else {
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_http_and_strips_path() {
        assert_eq!(
            normalize_base_url("http://192.168.1.10:8123/lovelace/").unwrap(),
            "http://192.168.1.10:8123"
        );
        assert!(normalize_base_url("ftp://x").is_err());
        assert!(normalize_base_url("http://user:pass@host:8123").is_err());
        assert!(normalize_base_url("").is_err());
    }

    #[test]
    fn entity_and_slug_validation() {
        assert!(valid_entity_id("light.living_room"));
        assert!(!valid_entity_id("Light.Living"));
        assert!(!valid_entity_id("light/../shell"));
        assert!(valid_slug("turn_on"));
        assert!(!valid_slug("../x"));
    }

    #[test]
    fn blocked_domains_are_refused() {
        for d in BLOCKED_DOMAINS {
            assert!(domain_blocked(d));
            assert!(gate_call(d, "turn_on", None).is_err());
        }
        assert!(gate_call("light", "turn_on", Some("light.kitchen")).is_ok());
        assert!(gate_call("light", "turn_on", Some("bad")).is_err());
    }

    #[test]
    fn summarize_filters_domain_and_area() {
        let body = r#"[
          {"entity_id":"light.kitchen","state":"on","attributes":{"friendly_name":"Kitchen"}},
          {"entity_id":"switch.fan","state":"off","attributes":{"friendly_name":"Hall fan"}},
          {"entity_id":"light.hall","state":"off","attributes":{"friendly_name":"Hall light"}}
        ]"#;
        let lights = summarize_states(body, Some("light"), None).unwrap();
        assert_eq!(lights.len(), 2);
        assert_eq!(lights[0].entity_id, "light.hall");
        let hall = summarize_states(body, None, Some("hall")).unwrap();
        assert_eq!(hall.len(), 2);
        let text = format_entities(&lights);
        assert!(text.contains("light.kitchen"));
        assert!(text.contains("[on]"));
    }

    #[test]
    fn format_services_skips_blocked_domains() {
        let body = r#"[
          {"domain":"light","services":{"turn_on":{},"turn_off":{}}},
          {"domain":"shell_command","services":{"run":{}}},
          {"domain":"switch","services":["toggle"]}
        ]"#;
        let text = format_services(body, None).unwrap();
        assert!(text.contains("light:"));
        assert!(text.contains("switch:"));
        assert!(!text.contains("shell_command"));
        let only = format_services(body, Some("light")).unwrap();
        assert!(only.contains("turn_off"));
        assert!(!only.contains("switch"));
    }

    #[test]
    fn settings_ready_needs_consent_and_url() {
        let mut s = HomeAssistantSettings::default();
        assert!(s.is_default());
        assert!(!s.ready_for_tools());
        s.enabled = true;
        assert!(!s.ready_for_tools());
        s.base_url = "http://127.0.0.1:8123".into();
        assert!(s.ready_for_tools());
    }

    #[test]
    fn format_state_shows_attributes() {
        let body = r#"{"entity_id":"climate.main","state":"heat","attributes":{"temperature":21}}"#;
        let text = format_state(body).unwrap();
        assert!(text.contains("climate.main"));
        assert!(text.contains("heat"));
        assert!(text.contains("21"));
    }
}
