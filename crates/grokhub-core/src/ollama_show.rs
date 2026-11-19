//! Ollama `/api/show` context probe. When an Ollama HTTP server is up, the
//! effective `num_ctx` (Modelfile override, else GGUF training max) feeds the
//! local route's context cap. Clean-room (Hermes `agent_init_ollama` /
//! `query_ollama_num_ctx` shape only).

/// Default loopback root when `OLLAMA_HOST` is unset.
pub const DEFAULT_ROOT: &str = "http://127.0.0.1:11434";

/// Root URL: `OLLAMA_HOST` when set, else [`DEFAULT_ROOT`]. Trailing slash trimmed.
pub fn root_from_env(ollama_host: Option<&str>) -> String {
    match ollama_host.map(str::trim).filter(|s| !s.is_empty()) {
        Some(host) if host.contains("://") => host.trim_end_matches('/').to_string(),
        Some(host) => format!("http://{}", host.trim_end_matches('/')),
        None => DEFAULT_ROOT.into(),
    }
}

pub fn tags_url(root: &str) -> String {
    format!("{}/api/tags", root.trim_end_matches('/'))
}

pub fn version_url(root: &str) -> String {
    format!("{}/api/version", root.trim_end_matches('/'))
}

pub fn show_url(root: &str) -> String {
    format!("{}/api/show", root.trim_end_matches('/'))
}

/// POST body for `/api/show`.
pub fn show_body(name: &str) -> String {
    serde_json::json!({ "name": name.trim() }).to_string()
}

/// True when a `/api/tags` or `/api/version` body looks like Ollama answered.
pub fn looks_like_ollama(body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    v.get("models").and_then(|m| m.as_array()).is_some()
        || v.get("version").and_then(|m| m.as_str()).is_some()
}

/// Model names from a `/api/tags` body, in list order.
pub fn parse_tags(body: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    v.get("models")
        .and_then(|m| m.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| {
            m.get("name")
                .or_else(|| m.get("model"))
                .and_then(|n| n.as_str())
                .map(str::to_string)
        })
        .collect()
}

/// Context length from an `/api/show` payload.
///
/// `parameters.num_ctx` is the runtime window (Modelfile); `model_info.*.context_length`
/// is the GGUF training max. Local users control `num_ctx`, so prefer it unless
/// `gguf_first` is set (hosted operators may cap `num_ctx` arbitrarily).
pub fn show_context(body: &str, gguf_first: bool) -> Option<u64> {
    let Ok(data) = serde_json::from_str::<serde_json::Value>(body) else {
        return None;
    };
    let num_ctx = || num_ctx_from_parameters(&data);
    let gguf = || gguf_context_length(&data);
    if gguf_first {
        gguf().or_else(num_ctx)
    } else {
        num_ctx().or_else(gguf)
    }
}

fn num_ctx_from_parameters(data: &serde_json::Value) -> Option<u64> {
    let params = data.get("parameters")?.as_str()?;
    for line in params.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts.contains(&"num_ctx") {
            if let Ok(n) = parts.last()?.parse::<u64>() {
                if n > 0 {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn gguf_context_length(data: &serde_json::Value) -> Option<u64> {
    let info = data.get("model_info")?.as_object()?;
    info.iter()
        .filter(|(k, _)| k.contains("context_length"))
        .filter_map(|(_, v)| {
            v.as_u64()
                .or_else(|| v.as_f64().map(|f| f as u64))
                .filter(|n| *n > 0)
        })
        .next()
}

/// Cap a probed window to an optional config ceiling (never raise above GGUF max
/// when the operator set a smaller budget).
pub fn capped(detected: u64, ceiling: Option<u64>) -> u64 {
    match ceiling {
        Some(c) if c > 0 && detected > c => c,
        _ => detected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_from_env_accepts_host_or_url() {
        assert_eq!(root_from_env(None), DEFAULT_ROOT);
        assert_eq!(
            root_from_env(Some("192.168.1.10:11434")),
            "http://192.168.1.10:11434"
        );
        assert_eq!(
            root_from_env(Some("http://ollama.local:11434/")),
            "http://ollama.local:11434"
        );
    }

    #[test]
    fn tags_and_version_mark_ollama() {
        assert!(looks_like_ollama(r#"{"models":[{"name":"llama3:latest"}]}"#));
        assert!(looks_like_ollama(r#"{"version":"0.6.0"}"#));
        assert!(!looks_like_ollama(r#"{"ok":true}"#));
        assert_eq!(
            parse_tags(r#"{"models":[{"name":"gemma4:e4b"},{"model":"qwen:7b"}]}"#),
            ["gemma4:e4b", "qwen:7b"]
        );
    }

    #[test]
    fn show_prefers_num_ctx_then_gguf() {
        let body = r#"{
          "parameters": "num_ctx 8192\nstop \"<|eot|>\"",
          "model_info": {"llama.context_length": 131072}
        }"#;
        assert_eq!(show_context(body, false), Some(8192));
        assert_eq!(show_context(body, true), Some(131072));
        let gguf_only = r#"{"model_info":{"gemma3.context_length":32768}}"#;
        assert_eq!(show_context(gguf_only, false), Some(32768));
        assert_eq!(capped(131072, Some(8192)), 8192);
        assert_eq!(capped(4096, Some(8192)), 4096);
    }

    #[test]
    fn show_body_names_the_model() {
        assert_eq!(show_body(" gemma4:e4b "), r#"{"name":"gemma4:e4b"}"#);
    }
}
