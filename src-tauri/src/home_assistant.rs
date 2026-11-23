//! Home Assistant settings, keychain token, and chat tool host.
//!
//! Consent + base URL live in `app.json` under `homeAssistant`. The long-lived
//! access token stays in the OS keychain. The chat host is registered only when
//! consent is on, the URL is valid, and a token is present.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use grokhub_agent::HomeAssistantOps;
use grokhub_core::home_assistant as ha;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::credential;

/// Settings page view — never includes the raw token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HomeAssistantView {
    pub enabled: bool,
    pub base_url: String,
    pub has_token: bool,
    pub url_hint: String,
    pub note: String,
}

impl Default for HomeAssistantView {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            has_token: false,
            url_hint: ha::DEFAULT_URL_HINT.into(),
            note: String::new(),
        }
    }
}

fn keychain_message(err: keyring::Error) -> String {
    format!("The system keychain is not available: {err}")
}

fn backend() -> grokhub_agent::keyring_blob::OsBackend {
    grokhub_agent::keyring_blob::OsBackend::new(credential::SERVICE, keychain_message)
}

#[cfg(test)]
static TEST_TOKEN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn load_token() -> Option<String> {
    if cfg!(test) {
        return TEST_TOKEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
    }
    match grokhub_agent::keyring_blob::get(&backend(), ha::KEYRING_ACCOUNT) {
        Ok(Some(t)) if !t.trim().is_empty() => Some(t),
        _ => None,
    }
}

fn store_token(token: Option<&str>) -> Result<(), String> {
    if cfg!(test) {
        *TEST_TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        return Ok(());
    }
    match token {
        Some(t) if !t.trim().is_empty() => {
            grokhub_agent::keyring_blob::set(&backend(), ha::KEYRING_ACCOUNT, t.trim())
                .map_err(|e| format!("Couldn't save the Home Assistant token: {e}"))
        }
        _ => grokhub_agent::keyring_blob::delete(&backend(), ha::KEYRING_ACCOUNT)
            .map_err(|e| format!("Couldn't remove the Home Assistant token: {e}")),
    }
}

pub fn load_settings(config_dir: &Path) -> ha::HomeAssistantSettings {
    grokhub_core::app_json::read(config_dir)
        .ok()
        .and_then(|m| m.get(ha::SETTINGS_KEY).cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn save_settings(config_dir: &Path, settings: &ha::HomeAssistantSettings) -> Result<(), String> {
    if settings.is_default() {
        grokhub_core::app_json::edit(config_dir, |map| {
            map.remove(ha::SETTINGS_KEY);
        })
    } else {
        let value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
        grokhub_core::app_json::patch(config_dir, &[(ha::SETTINGS_KEY, value)])
    }
}

pub fn view(config_dir: &Path) -> HomeAssistantView {
    let settings = load_settings(config_dir);
    HomeAssistantView {
        enabled: settings.enabled,
        base_url: settings.base_url,
        has_token: load_token().is_some(),
        url_hint: ha::DEFAULT_URL_HINT.into(),
        note: String::new(),
    }
}

/// Flip consent and/or set the base URL. Empty `base_url` leaves the stored URL alone.
pub fn set_config(
    config_dir: &Path,
    enabled: bool,
    base_url: Option<String>,
) -> Result<HomeAssistantView, String> {
    let mut settings = load_settings(config_dir);
    settings.enabled = enabled;
    if let Some(raw) = base_url {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            settings.base_url.clear();
        } else {
            settings.base_url = ha::normalize_base_url(trimmed)?;
        }
    }
    save_settings(config_dir, &settings)?;
    refresh_host(config_dir);
    Ok(view(config_dir))
}

/// Save or clear the long-lived access token. Empty string clears it.
pub fn set_token(config_dir: &Path, token: String) -> Result<HomeAssistantView, String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        store_token(None)?;
    } else {
        store_token(Some(trimmed))?;
    }
    refresh_host(config_dir);
    Ok(view(config_dir))
}

/// Live probe against `/api/`.
pub fn test_connection(config_dir: &Path) -> Result<String, String> {
    let settings = load_settings(config_dir);
    if !settings.enabled {
        return Err("Turn on Home Assistant first (Settings → Your PC → PC helpers).".into());
    }
    let token = load_token()
        .ok_or_else(|| "Add a Home Assistant long-lived access token first.".to_string())?;
    let body = ha::ping(&settings.base_url, &token)?;
    if body.contains("API running") || body.contains("message") {
        Ok("Connected. Home Assistant API is reachable.".into())
    } else {
        Ok(format!("Connected: {body}"))
    }
}

struct Host {
    config: PathBuf,
}

impl Host {
    fn credentials(&self) -> Result<(ha::HomeAssistantSettings, String), String> {
        let settings = load_settings(&self.config);
        if !settings.enabled {
            return Err(grokhub_agent::HOME_ASSISTANT_OFF.into());
        }
        if !settings.ready_for_tools() {
            return Err(
                "Add your Home Assistant address under Settings → Your PC → PC helpers.".into(),
            );
        }
        let token = load_token().ok_or_else(|| {
            "Add a Home Assistant long-lived access token under Settings → Your PC → PC helpers."
                .to_string()
        })?;
        Ok((settings, token))
    }
}

impl HomeAssistantOps for Host {
    fn list_entities(&self, domain: Option<&str>, area: Option<&str>) -> Result<String, String> {
        let (settings, token) = self.credentials()?;
        ha::list_entities(&settings.base_url, &token, domain, area)
    }

    fn get_state(&self, entity_id: &str) -> Result<String, String> {
        let (settings, token) = self.credentials()?;
        ha::get_state(&settings.base_url, &token, entity_id)
    }

    fn list_services(&self, domain: Option<&str>) -> Result<String, String> {
        let (settings, token) = self.credentials()?;
        ha::list_services(&settings.base_url, &token, domain)
    }

    fn call_service(
        &self,
        domain: &str,
        service: &str,
        entity_id: Option<&str>,
        data: Option<&Map<String, Value>>,
    ) -> Result<String, String> {
        let (settings, token) = self.credentials()?;
        ha::call_service(&settings.base_url, &token, domain, service, entity_id, data)
    }
}

/// Register or clear the chat host from the current settings + token.
pub fn refresh_host(config_dir: &Path) {
    let settings = load_settings(config_dir);
    let has_token = load_token().is_some();
    if settings.ready_for_tools() && has_token {
        grokhub_agent::set_home_assistant_host(Arc::new(Host {
            config: config_dir.to_path_buf(),
        }));
    } else {
        grokhub_agent::clear_home_assistant_host();
    }
}

/// Startup: wire the host if the user already opted in.
pub fn register(config_dir: PathBuf) {
    refresh_host(&config_dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tmp() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "grokhub-ha-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn consent_off_leaves_tools_unready() {
        let dir = tmp();
        let _ = store_token(None);
        set_config(&dir, false, Some("http://127.0.0.1:8123".into())).unwrap();
        let page = view(&dir);
        assert!(!page.enabled);
        assert!(!load_settings(&dir).ready_for_tools() || load_token().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn consent_url_and_token_round_trip() {
        let dir = tmp();
        set_config(&dir, true, Some("http://127.0.0.1:8123".into())).unwrap();
        set_token(&dir, "sk-abcdefghijklmnopqrstuv".into()).unwrap();
        let page = view(&dir);
        assert!(page.enabled && page.has_token);
        assert_eq!(page.base_url, "http://127.0.0.1:8123");
        assert!(load_settings(&dir).ready_for_tools() && load_token().is_some());
        set_token(&dir, String::new()).unwrap();
        assert!(!view(&dir).has_token);
        let _ = store_token(None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_url_is_refused_before_save() {
        let dir = tmp();
        let err = set_config(&dir, true, Some("not-a-url".into())).unwrap_err();
        assert!(err.contains("full URL"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_round_trip_in_app_json() {
        let dir = tmp();
        set_config(&dir, true, Some("http://homeassistant.local:8123/".into())).unwrap();
        let raw = std::fs::read_to_string(dir.join("app.json")).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["homeAssistant"]["enabled"], json!(true));
        assert_eq!(
            v["homeAssistant"]["baseUrl"],
            json!("http://homeassistant.local:8123")
        );
        assert!(
            !raw.contains("sk-"),
            "token must never land in app.json: {raw}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
