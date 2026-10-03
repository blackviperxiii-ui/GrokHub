//! Parse `GET /v1/models`. The engine never calls the network from tests.

use serde_json::Value;

pub const XAI_MODELS_URL: &str = "https://api.x.ai/v1/models";

/// Context window for `grok-4.7` when `/v1/models` omits `context_length`.
/// This is the CLI fallback used when a model entry has no window.
pub const GROK_47_CONTEXT_LENGTH: u64 = 256_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedModel {
    pub id: String,
    pub context_length: Option<u64>,
}

pub fn parse_xai_models(body: &str) -> Result<Vec<String>, String> {
    Ok(parse_listed_models(body)?
        .into_iter()
        .map(|row| row.id)
        .collect())
}

pub fn parse_listed_models(body: &str) -> Result<Vec<ListedModel>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let rows = v
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| v.as_array())
        .ok_or_else(|| "models payload has no data array".to_string())?;
    let mut listed = Vec::new();
    for row in rows {
        if let Some(id) = row.get("id").and_then(|s| s.as_str()).map(str::trim) {
            if id.is_empty() {
                continue;
            }
            let context_length = row.get("context_length").and_then(|value| value.as_u64());
            listed.push(ListedModel {
                id: id.to_string(),
                context_length,
            });
        }
    }
    Ok(listed)
}

/// `context_length` from a parsed `/v1/models` row.
/// `grok-4.7` (and any other id with no positive length) uses [`GROK_47_CONTEXT_LENGTH`].
pub fn context_length_for(model: &str, listed: &[ListedModel]) -> u64 {
    let model = model.trim();
    let key = if model.is_empty() {
        crate::DEFAULT_MODEL
    } else {
        model
    };
    if let Some(len) = listed
        .iter()
        .find(|row| row.id == key)
        .and_then(|row| row.context_length)
        .filter(|len| *len > 0)
    {
        return len;
    }
    GROK_47_CONTEXT_LENGTH
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

    #[test]
    fn context_length_parses_and_defaults_grok_4_7() {
        let body = r#"{"data":[
            {"id":"grok-4.7","context_length":128000},
            {"id":"grok-4","context_length":0},
            {"id":"other"}
        ]}"#;
        let listed = parse_listed_models(body).unwrap();
        assert_eq!(context_length_for("grok-4.7", &listed), 128_000);
        assert_eq!(context_length_for("grok-4.7", &[]), GROK_47_CONTEXT_LENGTH);
        assert_eq!(
            context_length_for("grok-4", &listed),
            GROK_47_CONTEXT_LENGTH
        );
        assert_eq!(context_length_for("other", &listed), GROK_47_CONTEXT_LENGTH);
        assert_eq!(context_length_for("", &[]), GROK_47_CONTEXT_LENGTH);
        assert_eq!(
            parse_xai_models(body).unwrap(),
            vec!["grok-4.7", "grok-4", "other"]
        );
    }
}
