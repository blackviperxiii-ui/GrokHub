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
        Self {
            kind: Some(kind),
            models,
            tier: None,
            gb_version: None,
        }
    }
}

/// A place the registry learns which models exist.
pub trait CatalogSource {
    fn kind(&self) -> SourceKind;
    /// The fresh list, or why it couldn't be read (a failed source changes nothing).
    fn fetch(&self) -> Result<Listing, String>;
}

fn u64_of(row: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|k| row.get(*k).and_then(Value::as_u64))
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
    keys.iter()
        .find_map(|k| row.get(*k).and_then(Value::as_bool))
}

/// One `/v1/models` or `/v1/language-models` row. Unknown fields stay `None`.
pub fn parse_model_row(row: &Value) -> Option<ModelMeta> {
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let caps = row.get("capabilities").cloned().unwrap_or(Value::Null);
    let efforts = strings(caps.get("reasoning_effort"))
        .or_else(|| strings(row.get("reasoning_effort")))
        .or_else(|| strings(row.get("reasoning_efforts")));
    let endpoints =
        strings(row.get("endpoints")).or_else(|| strings(row.get("supported_endpoints")));
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
        Some(Notice {
            severity: str_of(n, &["severity"])?,
            text: str_of(n, &["text", "message"]).unwrap_or_default(),
        })
    });
    Some(ModelMeta {
        id: id.to_string(),
        aliases: strings(row.get("aliases")).unwrap_or_default(),
        context_length: u64_of(
            row,
            &["context_length", "context_window", "max_prompt_length"],
        ),
        max_output: u64_of(row, &["max_output_tokens", "max_completion_tokens"]),
        prices: Prices {
            prompt: u64_of(row, &["prompt_text_token_price"]),
            cached: u64_of(
                row,
                &[
                    "cached_prompt_text_token_price",
                    "cached_prompt_token_price",
                ],
            ),
            cache_write: None,
            completion: u64_of(row, &["completion_text_token_price"]),
            prompt_long: u64_of(
                row,
                &[
                    "prompt_text_token_price_long_context",
                    "long_context_prompt_text_token_price",
                ],
            ),
            completion_long: u64_of(
                row,
                &[
                    "completion_text_token_price_long_context",
                    "long_context_completion_text_token_price",
                ],
            ),
            long_context_threshold: u64_of(row, &["long_context_threshold"]),
        },
        efforts,
        default_effort: str_of(&caps, &["default_reasoning_effort"])
            .or_else(|| str_of(row, &["default_reasoning_effort"])),
        fingerprint: str_of(row, &["fingerprint", "system_fingerprint"]),
        version: str_of(row, &["version"]),
        input_modalities: strings(row.get("input_modalities")),
        output_modalities: strings(row.get("output_modalities")),
        tool_calling: bool_of(&caps, &["function_calling", "tools", "tool_calling"]),
        reasoning: None,
        structured_output: bool_of(
            &caps,
            &["structured_outputs", "structured_output", "json_schema"],
        ),
        api_shape,
        deprecated: bool_of(row, &["deprecated"]),
        notice,
        card: Default::default(),
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
pub fn parse_xai_catalog(
    models: Option<&str>,
    language: Option<&str>,
) -> Result<Vec<ModelMeta>, String> {
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
    let mut listing = Listing::new(
        SourceKind::GrokBuild,
        ids.iter().map(|id| ModelMeta::bare(id)).collect(),
    );
    listing.gb_version = gb_version.map(str::to_string);
    listing
}

/// OpenRouter's own free router, a model id like any other.
pub const OPENROUTER_FREE_ROUTER: &str = "openrouter/free";

/// A free tier on OpenRouter: a `:free` variant or its free router. Takes
/// the wire model (`google/x:free`) or the registry id
/// (`openrouter/google/x:free`). This is the one definition of "free":
/// route classification ([`super::record::route_for`]), the quota ledger
/// and the provider calls all read it.
pub fn is_free_model(id: &str) -> bool {
    let id = id.trim();
    id.ends_with(":free") || id == OPENROUTER_FREE_ROUTER || id.ends_with("/openrouter/free")
}

/// The registry id of a model on a provider you added: `<provider>/<model>`.
/// xAI ids never have a `/`, so the two can't collide.
pub fn provider_model_id(provider: &str, model: &str) -> String {
    format!("{}/{}", provider.trim(), model.trim())
}

/// `(provider, model)` of a provider model id; `None` for an xAI id.
pub fn split_provider_model(id: &str) -> Option<(&str, &str)> {
    let (p, m) = id.trim().split_once('/')?;
    (!p.is_empty() && !m.is_empty()).then_some((p, m))
}

/// USD per token (`"0.000003"` or a number) as cents per 100M tokens, the
/// unit xAI reports. A missing, negative or unparsable price stays unknown.
fn usd_per_token(v: Option<&Value>) -> Option<u64> {
    let usd = match v? {
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    (usd.is_finite() && usd >= 0.0).then(|| (usd * 1e10).round() as u64)
}

/// A whole number of tokens sent as a number or a numeric string.
fn tokens_of(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// OpenRouter's `pricing.overrides` as one long-context step:
/// `(long_context_threshold, prompt_long, completion_long)`.
///
/// An entry with `min_prompt_tokens` is a long-context tier: its prices
/// apply once the prompt is *more than* that many tokens, and a price it
/// leaves out stays the base one. [`Prices::rates_at`] switches at or past
/// its threshold, so the threshold is one token past `min_prompt_tokens`.
/// A listing may name up to two tiers, and [`Prices`] holds one step: it
/// starts at the lowest tier and takes the highest rate any tier lists, so
/// a long prompt is never priced under what it may pay.
///
/// Entries this can't map are left out, and the base prices stand for them:
/// a time window (`utc_start`, `utc_end`, `utc_days`) or an entry with no
/// prompt-size threshold. A tier's `input_cache_read` is left out too:
/// `rates_at` scales the cached rate with the prompt's.
fn long_context_step(pricing: &Value) -> (Option<u64>, Option<u64>, Option<u64>) {
    let Some(tiers) = pricing.get("overrides").and_then(Value::as_array) else {
        return (None, None, None);
    };
    let mut threshold: Option<u64> = None;
    let (mut prompt, mut completion) = (None::<u64>, None::<u64>);
    for tier in tiers {
        let timed = ["utc_start", "utc_end", "utc_days"]
            .iter()
            .any(|k| tier.get(*k).is_some_and(|v| !v.is_null()));
        let Some(min) = tokens_of(tier.get("min_prompt_tokens")).filter(|_| !timed) else {
            continue;
        };
        let p = usd_per_token(tier.get("prompt"));
        let c = usd_per_token(tier.get("completion"));
        if p.is_none() && c.is_none() {
            continue;
        }
        threshold = Some(threshold.map_or(min, |t| t.min(min)));
        prompt = prompt.max(p);
        completion = completion.max(c);
    }
    match threshold {
        Some(t) => (Some(t.saturating_add(1)), prompt, completion),
        None => (None, None, None),
    }
}

/// An effort list the listing itself names. Nothing is filled from code.
fn listed_efforts(row: &Value) -> Option<Vec<String>> {
    const KEYS: &[&str] = &[
        "efforts",
        "reasoning_efforts",
        "reasoning_effort",
        "supported_efforts",
    ];
    let caps = row.get("capabilities");
    KEYS.iter()
        .find_map(|k| strings(row.get(*k)))
        .or_else(|| {
            KEYS.iter()
                .find_map(|k| strings(caps.and_then(|c| c.get(*k))))
        })
        .or_else(|| {
            strings(
                caps.and_then(|c| c.get("effort"))
                    .and_then(|e| e.get("levels").or(Some(e))),
            )
        })
}

/// An OpenAI-compatible `/models` listing (OpenRouter and the like), ids as
/// `<provider>/<model>`. Prices are USD per token there; unknown stays `None`,
/// and a long-context price step comes from `pricing.overrides`
/// (`long_context_step`).
/// `supported_parameters` says whether it takes `tools` and `reasoning`, and a
/// non-null `expiration_date` marks it deprecated.
pub fn parse_openai_compatible_catalog(
    provider: &str,
    body: &str,
) -> Result<Vec<ModelMeta>, String> {
    let mut out = Vec::new();
    for row in rows(body)? {
        let Some(id) = str_of(&row, &["id"]) else {
            continue;
        };
        let top = row.get("top_provider").cloned().unwrap_or(Value::Null);
        let pricing = row.get("pricing").cloned().unwrap_or(Value::Null);
        let params = strings(row.get("supported_parameters"));
        let (long_context_threshold, prompt_long, completion_long) = long_context_step(&pricing);
        out.push(ModelMeta {
            id: provider_model_id(provider, &id),
            context_length: u64_of(&row, &["context_length", "context_window"])
                .or_else(|| u64_of(&top, &["context_length"])),
            max_output: u64_of(&top, &["max_completion_tokens"])
                .or_else(|| u64_of(&row, &["max_output_tokens", "max_completion_tokens"])),
            prices: Prices {
                prompt: usd_per_token(pricing.get("prompt")),
                cached: usd_per_token(pricing.get("input_cache_read")),
                cache_write: usd_per_token(pricing.get("input_cache_write")),
                completion: usd_per_token(pricing.get("completion")),
                prompt_long,
                completion_long,
                long_context_threshold,
            },
            efforts: listed_efforts(&row),
            input_modalities: strings(
                row.get("architecture")
                    .and_then(|a| a.get("input_modalities")),
            ),
            output_modalities: strings(
                row.get("architecture")
                    .and_then(|a| a.get("output_modalities")),
            ),
            tool_calling: params.as_ref().map(|p| p.iter().any(|x| x == "tools")),
            reasoning: params.as_ref().map(|p| p.iter().any(|x| x == "reasoning")),
            // OpenRouter dates an endpoint it is retiring: routing skips it from then on.
            deprecated: row
                .get("expiration_date")
                .is_some_and(|d| !d.is_null())
                .then_some(true),
            ..ModelMeta::default()
        });
    }
    Ok(out)
}

/// Anthropic's `/v1/models`, ids as `<provider>/<model>`. It lists no
/// prices, so they stay unknown (ranked last, never the cheapest pick).
pub fn parse_anthropic_catalog(provider: &str, body: &str) -> Result<Vec<ModelMeta>, String> {
    let mut out = Vec::new();
    for row in rows(body)? {
        let Some(id) = str_of(&row, &["id"]) else {
            continue;
        };
        out.push(ModelMeta {
            id: provider_model_id(provider, &id),
            context_length: u64_of(
                &row,
                &["max_input_tokens", "context_window", "context_length"],
            ),
            max_output: u64_of(&row, &["max_tokens", "max_output_tokens"]),
            efforts: listed_efforts(&row),
            ..ModelMeta::default()
        });
    }
    Ok(out)
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
        assert_eq!(
            got.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["grok-3-mini-fast", "grok-4.7"]
        );
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
                cache_write: None,
                completion: Some(150_000),
                prompt_long: Some(60_000),
                completion_long: Some(300_000),
                long_context_threshold: Some(200_000)
            }
        );
        assert_eq!(
            m.efforts.as_deref(),
            Some(
                &[
                    "low".to_string(),
                    "medium".into(),
                    "high".into(),
                    "xhigh".into()
                ][..]
            )
        );
        assert_eq!(m.default_effort.as_deref(), Some("medium"));
        assert_eq!(
            (m.fingerprint.as_deref(), m.version.as_deref()),
            (Some("fp_47"), Some("1.0"))
        );
        assert_eq!(
            m.input_modalities.as_deref(),
            Some(&["text".to_string(), "image".into()][..])
        );
        assert_eq!(
            (m.tool_calling, m.structured_output),
            (Some(true), Some(true))
        );
        assert_eq!(m.api_shape.as_deref(), Some("responses"));
        assert!(!m.is_placeholder());
    }

    #[test]
    fn a_bad_payload_is_an_error_not_an_empty_list() {
        assert_eq!(
            parse_xai_catalog(Some("{}"), None).unwrap_err(),
            "models payload has no data array"
        );
        assert_eq!(
            parse_xai_catalog(None, None).unwrap_err(),
            "no models payload"
        );
    }

    #[test]
    fn openai_compatible_rows_parse_prices_per_token_and_keep_unknowns_null() {
        let body = r#"{"data":[
            {"id":"vendor/model-a","context_length":200000,
             "pricing":{"prompt":"0.000003","completion":"0.000015","input_cache_read":"0.0000003"},
             "top_provider":{"max_completion_tokens":64000},
             "architecture":{"input_modalities":["text","image"],"output_modalities":["text"]},
             "supported_parameters":["tools","reasoning"],"reasoning_efforts":["low","high"]},
            {"id":"vendor/model-b","pricing":{"prompt":"-1","completion":"abc"},"supported_parameters":["temperature"]},
            {"id":"vendor/model-c"},
            {"object":"no id"}
        ]}"#;
        let got = parse_openai_compatible_catalog("openrouter", body).unwrap();
        assert_eq!(
            got.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec![
                "openrouter/vendor/model-a",
                "openrouter/vendor/model-b",
                "openrouter/vendor/model-c"
            ]
        );
        let a = &got[0];
        // $3 / $15 / $0.30 per million is 30_000 / 150_000 / 3_000 cents per 100M, as xAI reports.
        assert_eq!(
            a.prices,
            Prices {
                prompt: Some(30_000),
                cached: Some(3_000),
                completion: Some(150_000),
                ..Prices::default()
            }
        );
        assert_eq!(
            (a.context_length, a.max_output),
            (Some(200_000), Some(64_000))
        );
        assert_eq!(
            a.efforts.as_deref(),
            Some(&["low".to_string(), "high".into()][..])
        );
        assert_eq!(a.tool_calling, Some(true));
        assert_eq!(
            a.input_modalities.as_deref(),
            Some(&["text".to_string(), "image".into()][..])
        );
        assert_eq!(a.api_shape, None);
        let b = &got[1];
        assert_eq!(
            b.prices,
            Prices::default(),
            "a negative or unparsable price is unknown"
        );
        assert_eq!(
            (b.tool_calling, b.efforts.as_ref()),
            (Some(false), None),
            "no effort list is filled from code"
        );
        let c = &got[2];
        assert_eq!(
            (c.context_length, c.tool_calling, c.prices.is_empty()),
            (None, None, true)
        );
        assert_eq!(
            split_provider_model("openrouter/vendor/model-a"),
            Some(("openrouter", "vendor/model-a"))
        );
        assert_eq!(split_provider_model("grok-4.7"), None);
        assert_eq!(split_provider_model("/x"), None);
    }

    /// Shaped like OpenRouter's `GET /api/v1/models`.
    const OPENROUTER_MODELS: &str = include_str!("../../tests/fixtures/openrouter_models.json");

    #[test]
    fn openrouter_listing_reads_tools_reasoning_and_expiry_from_the_row() {
        let got = parse_openai_compatible_catalog("openrouter", OPENROUTER_MODELS).unwrap();
        let row = |id: &str| got.iter().find(|m| m.id == id).unwrap();
        let flags = |id: &str| {
            let m = row(id);
            (m.tool_calling, m.reasoning, m.deprecated)
        };
        assert_eq!(got.len(), 10);
        assert_eq!(
            flags("openrouter/vendor/agent-pro"),
            (Some(true), Some(true), None)
        );
        assert_eq!(
            flags("openrouter/vendor/tools-only"),
            (Some(true), Some(false), None)
        );
        assert_eq!(
            flags("openrouter/vendor/chat-only"),
            (Some(false), Some(true), None)
        );
        assert_eq!(
            flags("openrouter/vendor/retiring"),
            (Some(true), Some(true), Some(true)),
            "a dated expiration_date marks it deprecated"
        );
        // OpenRouter's router lists -1 prices: unknown, never a cheap pick.
        assert_eq!(row("openrouter/openrouter/auto").prices, Prices::default());
        let pro = row("openrouter/vendor/agent-pro");
        assert_eq!(
            pro.prices,
            Prices {
                prompt: Some(20_000),
                cached: Some(5_000),
                completion: Some(80_000),
                ..Prices::default()
            }
        );
        assert_eq!(
            (pro.context_length, pro.max_output),
            (Some(400_000), Some(128_000))
        );
        assert_eq!(
            pro.efforts.as_deref(),
            Some(&["low".to_string(), "medium".into(), "high".into()][..])
        );
        assert_eq!(row("openrouter/vendor/agent-lite").efforts, None);
        assert_eq!(
            row("openrouter/vendor/image-maker")
                .output_modalities
                .as_deref(),
            Some(&["image".to_string()][..])
        );
        // An xAI row has no such field, so it serializes as before.
        assert!(!serde_json::to_string(&ModelMeta::bare("grok-4.7"))
            .unwrap()
            .contains("reasoning"));
    }

    #[test]
    fn anthropic_rows_have_no_prices_and_only_the_efforts_they_name() {
        let body = r#"{"data":[
            {"id":"model-x","type":"model","display_name":"X","max_input_tokens":200000,"max_tokens":32000,
             "capabilities":{"effort":{"levels":["low","medium","high"]}}},
            {"id":"model-y","type":"model"}
        ],"has_more":false}"#;
        let got = parse_anthropic_catalog("anthropic", body).unwrap();
        assert_eq!(got[0].id, "anthropic/model-x");
        assert_eq!(
            (got[0].context_length, got[0].max_output),
            (Some(200_000), Some(32_000))
        );
        assert_eq!(
            got[0].efforts.as_deref(),
            Some(&["low".to_string(), "medium".into(), "high".into()][..])
        );
        assert!(
            got[0].prices.is_empty(),
            "Anthropic lists no prices: unknown, never guessed"
        );
        assert_eq!(
            (got[1].efforts.as_ref(), got[1].context_length),
            (None, None)
        );
        assert_eq!(
            parse_anthropic_catalog("anthropic", "{}").unwrap_err(),
            "models payload has no data array"
        );
    }

    /// Rows shaped like OpenRouter's `GET /api/v1/models` with
    /// `pricing.overrides`: one long-context tier, two tiers, a time window,
    /// and a threshold sent as a string.
    const OPENROUTER_OVERRIDES: &str = r#"{"data":[
        {"id":"vendor/long-ctx","context_length":1000000,
         "pricing":{"prompt":"0.00000125","completion":"0.00001","input_cache_read":"0.000000125",
           "overrides":[{"min_prompt_tokens":200000,"prompt":"0.0000025","completion":"0.000015","input_cache_read":"0.00000025"}]}},
        {"id":"vendor/two-tiers",
         "pricing":{"prompt":"0.000001","completion":"0.000004",
           "overrides":[{"min_prompt_tokens":"256000","prompt":"0.000003","completion":"0.000008"},
                        {"min_prompt_tokens":128000,"prompt":"0.000002"}]}},
        {"id":"vendor/peak-hours",
         "pricing":{"prompt":"0.000001","completion":"0.000002",
           "overrides":[{"utc_start":"1400","utc_end":"2200","prompt":"0.000002","completion":"0.000004"}]}},
        {"id":"vendor/cache-only",
         "pricing":{"prompt":"0.000001","completion":"0.000002",
           "overrides":[{"min_prompt_tokens":100000,"input_cache_read":"0.0000005"}]}}
    ]}"#;

    #[test]
    fn openrouter_overrides_become_the_long_context_step() {
        let got = parse_openai_compatible_catalog("openrouter", OPENROUTER_OVERRIDES).unwrap();
        let prices = |id: &str| got.iter().find(|m| m.id == id).unwrap().prices.clone();
        let long = prices("openrouter/vendor/long-ctx");
        assert_eq!(
            long,
            Prices {
                prompt: Some(12_500),
                cached: Some(1_250),
                cache_write: None,
                completion: Some(100_000),
                prompt_long: Some(25_000),
                completion_long: Some(150_000),
                long_context_threshold: Some(200_001),
            }
        );
        // OpenRouter's step is strictly past 200,000 prompt tokens.
        assert_eq!(long.rates_at(200_000), Some((12_500, 1_250, 100_000)));
        assert_eq!(long.rates_at(200_001), Some((25_000, 2_500, 150_000)));
        // Two tiers: the step starts at the lower one, at the higher rates;
        // a price no tier names stays the base one.
        assert_eq!(
            prices("openrouter/vendor/two-tiers"),
            Prices {
                prompt: Some(10_000),
                completion: Some(40_000),
                prompt_long: Some(30_000),
                completion_long: Some(80_000),
                long_context_threshold: Some(128_001),
                ..Prices::default()
            }
        );
        // A time window and a cache-only tier don't map: base prices only.
        for id in [
            "openrouter/vendor/peak-hours",
            "openrouter/vendor/cache-only",
        ] {
            let p = prices(id);
            assert_eq!(
                (p.prompt_long, p.completion_long, p.long_context_threshold),
                (None, None, None),
                "{id}"
            );
        }
        assert_eq!(
            prices("openrouter/vendor/peak-hours").completion,
            Some(20_000)
        );
    }
}
