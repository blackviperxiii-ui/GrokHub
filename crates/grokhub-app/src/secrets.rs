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
    config::load_json(&secrets_path(), config::JSON_STORE_CAP)
}

pub fn save(s: &Secrets) -> Result<(), String> {
    let path = secrets_path();
    let body = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    config::atomic_write(&path, body.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Boot: rewrite a leftover world-readable `secrets.json` through `atomic_write`.
/// Same bytes — do not go through load/save, which would wipe a torn file.
pub fn ensure_private() {
    let _ = config::rewrite_if_world_readable(&secrets_path());
}

pub fn access_token(s: &Secrets) -> String {
    s.oauth
        .as_ref()
        .map(|t| t.access_token.clone())
        .unwrap_or_default()
}

/// Console key lives in secrets.json. `cfg_key` is only a leftover app.json value.
pub fn console_key<'a>(secrets: &'a Secrets, cfg_key: &'a str) -> &'a str {
    if !secrets.api_key.trim().is_empty() {
        secrets.api_key.as_str()
    } else {
        cfg_key
    }
}

/// Move a leftover app.json console key into secrets.json and wipe it from config.
pub fn migrate_console_key(cfg: &mut crate::config::AppConfig, secrets: &mut Secrets) {
    if cfg.api_key.trim().is_empty() {
        return;
    }
    if secrets.api_key.trim().is_empty() {
        secrets.api_key = std::mem::take(&mut cfg.api_key);
        let _ = save(secrets);
    } else {
        cfg.api_key.clear();
    }
    let _ = crate::config::save(cfg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_roundtrip_mode() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("sec");
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let s = Secrets {
            oauth: Some(XaiOAuthTokens {
                access_token: "tok".into(),
                refresh_token: Some("ref".into()),
                connected_at: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        save(&s).expect("save");
        let loaded = load();
        assert_eq!(access_token(&loaded), "tok");
        assert!(loaded.oauth.as_ref().unwrap().picture.is_none());
        let old = r#"{"apiKey":"","oauth":{"accessToken":"legacy"}}"#;
        let parsed: Secrets = serde_json::from_str(old).unwrap();
        assert_eq!(access_token(&parsed), "legacy");
        assert!(parsed.oauth.unwrap().picture.is_none());
        let mut cfg = crate::config::AppConfig {
            api_key: "xai-from-app".into(),
            ..Default::default()
        };
        let mut migrated = Secrets::default();
        migrate_console_key(&mut cfg, &mut migrated);
        assert_eq!(migrated.api_key, "xai-from-app");
        assert!(cfg.api_key.is_empty());
        assert_eq!(console_key(&migrated, ""), "xai-from-app");
        let disk = load();
        assert_eq!(disk.api_key, "xai-from-app");
        let app = crate::config::load();
        assert!(
            app.api_key.is_empty(),
            "migrate must wipe the leftover app.json key: {}",
            app.api_key
        );
        let mut keep = Secrets {
            api_key: "xai-secrets".into(),
            ..Default::default()
        };
        let mut leftover = crate::config::AppConfig {
            api_key: "xai-stale".into(),
            ..Default::default()
        };
        migrate_console_key(&mut leftover, &mut keep);
        assert_eq!(keep.api_key, "xai-secrets");
        assert!(leftover.api_key.is_empty());
        assert_eq!(console_key(&keep, "xai-stale"), "xai-secrets");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(secrets_path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        #[cfg(windows)]
        {
            let sddl = crate::win_acl::file_sddl(&secrets_path()).expect("sddl");
            assert!(
                !crate::config::sddl_allows_world(&sddl),
                "save must write user-only DACL, got {sddl}"
            );
        }
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[cfg(windows)]
    #[test]
    fn ensure_private_rewrites_world_readable_leftover() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("sec-win");
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        fs::create_dir_all(&root).expect("root");
        let path = secrets_path();
        let body = br#"{"apiKey":"xai-world","oauth":null}"#;
        fs::write(&path, body).expect("write");
        crate::win_acl::apply_sddl(&path, crate::win_acl::WORLD_READ_SDDL).expect("world");
        assert!(
            crate::config::sddl_allows_world(&crate::win_acl::file_sddl(&path).unwrap()),
            "precondition"
        );
        ensure_private();
        let sddl = crate::win_acl::file_sddl(&path).expect("fixed");
        assert!(
            !crate::config::sddl_allows_world(&sddl),
            "ensure_private must rewrite to user-only DACL: {sddl}"
        );
        assert_eq!(fs::read(&path).expect("bytes"), body);
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn ensure_private_rewrites_bytes_not_parsed_json() {
        let src = include_str!("secrets.rs");
        let ens = src
            .split("pub fn ensure_private(")
            .nth(1)
            .and_then(|s| s.split("pub fn access_token(").next())
            .expect("ensure_private");
        assert!(
            ens.contains("rewrite_if_world_readable")
                && !ens.contains("load()")
                && !ens.contains("save("),
            "boot rewrite must keep leftover bytes, not load/save JSON: {ens}"
        );
    }

    #[test]
    fn secrets_load_does_not_slurp_a_huge_file() {
        let src = include_str!("secrets.rs");
        let load = src
            .split("pub fn load(")
            .nth(1)
            .and_then(|s| s.split("pub fn save(").next())
            .expect("secrets load");
        assert!(
            load.contains("load_json") && !load.contains("read_to_string"),
            "boot must not slurp an unbounded secrets.json: {load}"
        );
        assert!(
            !load.contains("MEMORY_FILE_CAP"),
            "a capped read severs secrets.json mid-token, so load returns empty and the \
             next save writes that over the console key and OAuth tokens: {load}"
        );
    }
}
