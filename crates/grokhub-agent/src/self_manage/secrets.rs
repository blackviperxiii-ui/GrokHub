//! Connection secrets, sealed at rest with the learned-tier key (OS keyring,
//! fail closed). The value comes only from the user's elicit card. It never
//! reaches the model (it reads `%secret%`), a span, the change ledger, a park
//! file, or the connection's config: the config runs the server through
//! `grokhub --mcp-secret-env <name> -- <command>`, which opens the seal and
//! hands the values to the child as env vars.

use std::path::{Path, PathBuf};

use grokhub_core::amr::Sealer;

use crate::harness::LearnedVault;

/// What the model reads in place of a value.
pub const SECRET_MARK: &str = "%secret%";

/// `--mcp-secret-env`: the launcher flag a sealed connection runs through.
pub const SECRET_ENV_FLAG: &str = "--mcp-secret-env";

fn aad(conn: &str) -> String {
    format!("grokhub:conn-secret:v1:{}", slug(conn))
}

fn slug(conn: &str) -> String {
    grokhub_core::skill_dir_name(conn)
}

/// `{config_dir}/connections/secrets/<name>.sealed`
pub fn secrets_path(config_dir: &Path, conn: &str) -> PathBuf {
    config_dir.join("connections").join("secrets").join(format!("{}.sealed", slug(conn)))
}

/// Seal `values` (env var name, value) for one connection, replacing what was there.
pub fn seal_secrets(config_dir: &Path, conn: &str, values: &[(String, String)]) -> Result<(), String> {
    if slug(conn).is_empty() {
        return Err("a connection needs a name".into());
    }
    let map: serde_json::Map<String, serde_json::Value> =
        values.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect();
    let plain = zeroize::Zeroizing::new(serde_json::Value::Object(map).to_string());
    let sealed = LearnedVault::new(config_dir).seal(&aad(conn), &plain)?;
    let path = secrets_path(config_dir, conn);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, sealed).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Open one connection's secrets. Missing file ⇒ none; a locked or wrong key ⇒ error.
pub fn open_secrets(config_dir: &Path, conn: &str) -> Result<Vec<(String, zeroize::Zeroizing<String>)>, String> {
    let Ok(sealed) = std::fs::read_to_string(secrets_path(config_dir, conn)) else {
        return Ok(Vec::new());
    };
    let plain = zeroize::Zeroizing::new(LearnedVault::new(config_dir).open(&aad(conn), sealed.trim())?);
    let v: serde_json::Value = serde_json::from_str(&plain).map_err(|_| "the sealed secrets are damaged".to_string())?;
    Ok(v.as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), zeroize::Zeroizing::new(s.to_string()))))
                .collect()
        })
        .unwrap_or_default())
}

/// Drop one connection's sealed secrets (after a remove).
pub fn remove_secrets(config_dir: &Path, conn: &str) {
    let _ = std::fs::remove_file(secrets_path(config_dir, conn));
}
