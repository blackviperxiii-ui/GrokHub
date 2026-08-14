use serde_json::{json, Value};

pub const DEFAULT_IMAGINE_MODEL: &str = "grok-2-image";

/// Imagine never shares the chat model. Only an explicit *image* model wins.
pub fn dedicated_imagine_model(user: &str) -> String {
    let u = user.trim();
    if u.contains("image") {
        u.to_string()
    } else {
        DEFAULT_IMAGINE_MODEL.to_string()
    }
}

pub fn imagine_request_body(prompt: &str, model: &str) -> Value {
    json!({
        "model": dedicated_imagine_model(model),
        "prompt": prompt,
        "n": 1,
        "response_format": "b64_json",
    })
}

pub fn imagine_slug(prompt: &str) -> String {
    let s: String = prompt
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "imagine".into()
    } else {
        s
    }
}

pub fn parse_imagine_url(body: &Value) -> Option<String> {
    let data = body.get("data")?.as_array()?.first()?;
    data.get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            data.get("b64_json")
                .and_then(|v| v.as_str())
                .map(|s| format!("data:image/png;base64,{s}"))
        })
}

pub fn imagine_dest(project: Option<&str>) -> String {
    match project.filter(|s| !s.is_empty()) {
        Some(p) => format!("{p}/imagine"),
        None => "GrokHub-Work/imagine".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_and_url() {
        assert_eq!(dedicated_imagine_model("grok-3-mini-fast"), DEFAULT_IMAGINE_MODEL);
        assert_eq!(dedicated_imagine_model("grok-2-image-1212"), "grok-2-image-1212");
        let b = imagine_request_body("a cabin at night", "grok-3-mini-fast");
        assert_eq!(b["model"], DEFAULT_IMAGINE_MODEL);
        assert_eq!(b["response_format"], "b64_json");
        assert_eq!(dedicated_imagine_model(""), DEFAULT_IMAGINE_MODEL);
        assert_eq!(dedicated_imagine_model("grok-imagine"), DEFAULT_IMAGINE_MODEL);
        let reply = json!({ "data": [{ "url": "https://img/x.png" }] });
        assert_eq!(parse_imagine_url(&reply).as_deref(), Some("https://img/x.png"));
        assert_eq!(imagine_dest(None), "GrokHub-Work/imagine");
    }
}
