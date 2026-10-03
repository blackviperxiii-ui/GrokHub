//! Parse `GET /v1/models`. The engine never calls the network from tests.

use serde_json::Value;

pub const XAI_MODELS_URL: &str = "https://api.x.ai/v1/models";

pub fn parse_xai_models(body: &str) -> Result<Vec<String>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let rows = v
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| v.as_array())
        .ok_or_else(|| "models payload has no data array".to_string())?;
    let mut ids = Vec::new();
    for row in rows {
        if let Some(id) = row.get("id").and_then(|s| s.as_str()).map(str::trim) {
            if !id.is_empty() {
                ids.push(id.to_string());
            }
        }
    }
    Ok(ids)
}

/// An explicit pin wins. Otherwise prefer `grok-4.7` and skip proxy-only build ids.
pub fn pick_model(ids: &[String], pinned: &str) -> String {
    let pin = pinned.trim();
    if !pin.is_empty() {
        return pin.to_string();
    }
    if ids.iter().any(|id| id == crate::DEFAULT_MODEL) {
        return crate::DEFAULT_MODEL.to_string();
    }
    ids.iter()
        .find(|id| !id.contains("build"))
        .cloned()
        .unwrap_or_else(|| crate::DEFAULT_MODEL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "data": [
            {"id":"grok-4","object":"model"},
            {"id":"grok-4.7","object":"model"},
            {"id":"grok-4.7-build","object":"model"}
        ]
    }"#;

    #[test]
    fn models_fixture_defaults_to_grok_4_7() {
        let ids = parse_xai_models(FIXTURE).unwrap();
        assert_eq!(ids, vec!["grok-4", "grok-4.7", "grok-4.7-build"]);
        assert_eq!(pick_model(&ids, ""), "grok-4.7");
        assert_eq!(pick_model(&ids, " grok-4 "), "grok-4");
        let no_default = vec!["grok-4.7-build".into(), "grok-4".into()];
        assert_eq!(pick_model(&no_default, ""), "grok-4");
        assert_eq!(pick_model(&[], ""), crate::DEFAULT_MODEL);
    }
}
