use grokhub_core::XaiOAuthTokens;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::config;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Secrets {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub oauth: Option<XaiOAuthTokens>,
    #[serde(default)]
    pub github_token: String,
    #[serde(default)]
    pub sso_cookie: String,
}

pub fn secrets_path() -> PathBuf {
    config::config_dir().join("secrets.json")
}

pub fn load() -> Secrets {
    let raw = fs::read_to_string(secrets_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(s: &Secrets) -> Result<(), String> {
    let dir = config::config_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = secrets_path();
    let body = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    fs::write(&path, body).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn access_token(s: &Secrets) -> String {
    s.oauth
        .as_ref()
        .map(|t| t.access_token.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TEST_CONFIG_LOCK;

    #[test]
    fn secrets_roundtrip_mode() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-sec-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut s = Secrets::default();
        s.oauth = Some(XaiOAuthTokens {
            access_token: "tok".into(),
            refresh_token: Some("ref".into()),
            connected_at: 1,
            ..Default::default()
        });
        save(&s).expect("save");
        let loaded = load();
        assert_eq!(access_token(&loaded), "tok");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(secrets_path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
