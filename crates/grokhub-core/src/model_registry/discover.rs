//! Catalog sources and their parsers. The trait is here; the real sources (the
//! only code that touches the network or runs `grok`) live in grokhub-agent's
//! `route::sources`, so every test here is offline.

use std::collections::BTreeMap;

use serde_json::Value;

use super::record::{ModelMeta, Notice, Prices, SourceKind};

/// One source's fresh list. Only a source that answered gives a listing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub kind: Option<SourceKind>,
    pub models: Vec<ModelMeta>,
    /// The plan tier the source reported, if it reports one.
    pub tier: Option<String>,
    /// `grok --version` for the Grok Build source.
    pub gb_version: Option<String>,
}

impl Listing {
    pub fn new(kind: SourceKind, models: Vec<ModelMeta>) -> Self {
        Self { kind: Some(kind), models, tier: None, gb_version: None }
    }
}

/// A place the registry learns which models exist.
pub trait CatalogSource {
    fn kind(&self) -> SourceKind;
    /// The fresh list, or why it couldn't be read (a failed source changes nothing).
    fn fetch(&self) -> Result<Listing, String>;
}

fn u64_of(row: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|k| row.get(*k).and_then(Value::as_u64))
}

fn str_of(row: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| row.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn strings(v: Option<&Value>) -> Option<Vec<String>> {
    v.and_then(Value::as_array).map(|a| {
        a.iter()
            .filter_map(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    })
}

fn bool_of(row: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|k| row.get(*k).and_then(Value::as_bool))
}

/// One `/v1/models` or `/v1/language-models` row. Unknown fields stay `None`.
pub fn parse_model_row(row: &Value) -> Option<ModelMeta> {
    let id = row.get("id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())?;
    let caps = row.get("capabilities").cloned().unwrap_or(Value::Null);
    let efforts = strings(caps.get("reasoning_effort"))
        .or_else(|| strings(row.get("reasoning_effort")))
        .or_else(|| strings(row.get("reasoning_efforts")));
    let endpoints = strings(row.get("endpoints")).or_else(|| strings(row.get("supported_endpoints")));
    let api_shape = endpoints.as_ref().and_then(|eps| {
        if eps.iter().any(|e| e.contains("responses")) {
            Some("responses".to_string())
        } else if eps.iter().any(|e| e.contains("chat")) {
            Some("chat_completions".to_string())
        } else {
            None
        }
    });
    let notice = row.get("notice").and_then(|n| {
        Some(Notice { severity: str_of(n, &["severity"])?, text: str_of(n, &["text", "message"]).unwrap_or_default() })
    });
    Some(ModelMeta {
        id: id.to_string(),
        aliases: strings(row.get("aliases")).unwrap_or_default(),
        context_length: u64_of(row, &["context_length", "context_window", "max_prompt_length"]),
        max_output: u64_of(row, &["max_output_tokens", "max_completion_tokens"]),
        prices: Prices {
            prompt: u64_of(row, &["prompt_text_token_price"]),
            cached: u64_of(row, &["cached_prompt_text_token_price", "cached_prompt_token_price"]),
            completion: u64_of(row, &["completion_text_token_price"]),
            prompt_long: u64_of(row, &["prompt_text_token_price_long_context", "long_context_prompt_text_token_price"]),
            completion_long: u64_of(
                row,
                &["completion_text_token_price_long_context", "long_context_completion_text_token_price"],
            ),
            long_context_threshold: u64_of(row, &["long_context_threshold"]),
        },
        efforts,
        default_effort: str_of(&caps, &["default_reasoning_effort"]).or_else(|| str_of(row, &["default_reasoning_effort"])),
        fingerprint: str_of(row, &["fingerprint", "system_fingerprint"]),
        version: str_of(row, &["version"]),
        input_modalities: strings(row.get("input_modalities")),
        output_modalities: strings(row.get("output_modalities")),
        tool_calling: bool_of(&caps, &["function_calling", "tools", "tool_calling"]),
        structured_output: bool_of(&caps, &["structured_outputs", "structured_output", "json_schema"]),
        api_shape,
        deprecated: bool_of(row, &["deprecated"]),
        notice,
    })
}

fn rows(body: &str) -> Result<Vec<Value>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let list = v
        .get("data")
        .or_else(|| v.get("models"))
        .and_then(Value::as_array)
        .or_else(|| v.as_array())
        .ok_or_else(|| "models payload has no data array".to_string())?;
    Ok(list.clone())
}

/// `/v1/models` plus `/v1/language-models` (either may be missing), merged by id.
/// The language-models row wins on any field both send; order is by id.
pub fn parse_xai_catalog(models: Option<&str>, language: Option<&str>) -> Result<Vec<ModelMeta>, String> {
    let mut by_id: BTreeMap<String, ModelMeta> = BTreeMap::new();
    let mut any = false;
    for body in [language, models].into_iter().flatten() {
        any = true;
        for row in rows(body)? {
            if let Some(meta) = parse_model_row(&row) {
                match by_id.get_mut(&meta.id) {
                    Some(have) => have.merge_from(&meta),
                    None => {
                        by_id.insert(meta.id.clone(), meta);
                    }
                }
            }
        }
    }
    if !any {
        return Err("no models payload".into());
    }
    Ok(by_id.into_values().collect())
}

/// Ids from `grok models` (already parsed by `grokhub_acp::parse_models_list`).
/// The CLI lists ids only, so every other field stays unknown.
pub fn gb_listing(ids: &[String], gb_version: Option<&str>) -> Listing {
    let mut listing = Listing::new(SourceKind::GrokBuild, ids.iter().map(|id| ModelMeta::bare(id)).collect());
    listing.gb_version = gb_version.map(str::to_string);
    listing
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const MODELS: &str = r#"{"data":[
        {"id":"grok-4.7","object":"model"},
        {"id":"grok-3-mini-fast","object":"model"}
    ]}"#;
    pub(crate) const LANGUAGE: &str = r#"{"models":[{
        "id":"grok-4.7","aliases":["grok-4.7-latest"],"fingerprint":"fp_47","version":"1.0",
        "input_modalities":["text","image"],"output_modalities":["text"],
        "prompt_text_token_price":30000,"cached_prompt_text_token_price":7500,
        "completion_text_token_price":150000,"long_context_threshold":200000,
        "prompt_text_token_price_long_context":60000,"completion_text_token_price_long_context":300000,
        "context_length":500000,"max_output_tokens":64000,
        "endpoints":["/v1/responses","/v1/chat/completions"],
        "capabilities":{"reasoning_effort":["low","medium","high","xhigh"],"default_reasoning_effort":"medium",
                        "function_calling":true,"structured_outputs":true}
    }]}"#;

    #[test]
    fn xai_catalog_merges_both_endpoints_and_keeps_unknowns_null() {
        let got = parse_xai_catalog(Some(MODELS), Some(LANGUAGE)).unwrap();
        assert_eq!(got.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["grok-3-mini-fast", "grok-4.7"]);
        let ghost = &got[0];
        assert!(ghost.is_placeholder());
        assert_eq!(ghost.context_length, None);
        assert_eq!(ghost.efforts, None);
        let m = &got[1];
        assert_eq!(m.aliases, vec!["grok-4.7-latest".to_string()]);
        assert_eq!(m.context_length, Some(500_000));
        assert_eq!(m.max_output, Some(64_000));
        assert_eq!(
            m.prices,
            Prices {
                prompt: Some(30_000),
                cached: Some(7_500),
                completion: Some(150_000),
                prompt_long: Some(60_000),
                completion_long: Some(300_000),
                long_context_threshold: Some(200_000)
            }
        );
        assert_eq!(m.efforts.as_deref(), Some(&["low".to_string(), "medium".into(), "high".into(), "xhigh".into()][..]));
        assert_eq!(m.default_effort.as_deref(), Some("medium"));
        assert_eq!((m.fingerprint.as_deref(), m.version.as_deref()), (Some("fp_47"), Some("1.0")));
        assert_eq!(m.input_modalities.as_deref(), Some(&["text".to_string(), "image".into()][..]));
        assert_eq!((m.tool_calling, m.structured_output), (Some(true), Some(true)));
        assert_eq!(m.api_shape.as_deref(), Some("responses"));
        assert!(!m.is_placeholder());
    }

    #[test]
    fn a_bad_payload_is_an_error_not_an_empty_list() {
        assert_eq!(parse_xai_catalog(Some("{}"), None).unwrap_err(), "models payload has no data array");
        assert_eq!(parse_xai_catalog(None, None).unwrap_err(), "no models payload");
    }
}
