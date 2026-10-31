//! Reasoning effort for the on-device model. llama-server's `GET /props`
//! lists, under `chat_template_caps.reasoning_effort_levels`, the
//! `reasoning_effort` values the model's chat template renders differently
//! from its own default, lowest first (b12017). The sidecar reads it once
//! after it starts ([`fetch_levels`]); a local turn's effort, a rung of
//! [`EFFORT_LADDER`], then goes out as the nearest listed level
//! ([`nearest`]), and none goes out when the list is empty or missing.
//!
//! The template's default level is never in the list: a template that
//! defaults to `medium` and also renders `low` and `high` lists
//! `["low", "high"]`. So an effort exactly between two listed levels sends
//! nothing and the template keeps its default.

use std::time::Duration;

use grokhub_core::model_registry::EFFORT_LADDER;
use serde_json::Value;

use super::ladder::rung;

/// How long the one `/props` read may take.
pub const PROPS_TIMEOUT: Duration = Duration::from_secs(2);

/// `chat_template_caps.reasoning_effort_levels` of a `/props` reply. Empty
/// when the reply has no list (an older engine) or isn't JSON.
pub fn parse_levels(props_json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(props_json) else {
        return Vec::new();
    };
    v.get("chat_template_caps")
        .and_then(|c| c.get("reasoning_effort_levels"))
        .and_then(Value::as_array)
        .map(|levels| {
            levels
                .iter()
                .filter_map(Value::as_str)
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The level of `levels` to send for the effort `want`: the same level when
/// listed, else the closest one on [`EFFORT_LADDER`]. `None` when `want` or
/// every level is off the ladder, when the list is empty, or when `want` sits
/// exactly between two listed levels (the template's own default).
pub fn nearest(want: &str, levels: &[String]) -> Option<String> {
    let want = rung(want)?;
    let mut best: Option<(usize, &String)> = None;
    let mut tie = false;
    for level in levels {
        let Some(r) = rung(level) else {
            continue;
        };
        let d = r.abs_diff(want);
        match best {
            Some((bd, _)) if d > bd => {}
            Some((bd, _)) if d == bd => tie = true,
            _ => {
                best = Some((d, level));
                tie = false;
            }
        }
    }
    let (d, level) = best?;
    if tie && d > 0 {
        return None;
    }
    Some(EFFORT_LADDER[rung(level)?].to_string())
}

/// Read the levels from a running llama-server on loopback, with its API
/// key (`/props` answers 401 without it). Empty on any failure, so a turn
/// then sends no effort.
pub fn fetch_levels(host: &str, port: u16, token: &str, timeout: Duration) -> Vec<String> {
    if !super::local_sidecar::is_loopback_host(host) {
        return Vec::new();
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .build();
    let resp = agent
        .get(&format!("http://{host}:{port}/props"))
        .set("Authorization", &format!("Bearer {token}"))
        .call();
    match resp {
        Ok(r) if r.status() == 200 => parse_levels(&r.into_string().unwrap_or_default()),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    /// The `/props` caps llama-server b12017 returned for a template that
    /// defaults to `medium` and renders `low` and `high` (trimmed to the caps).
    const PROPS_LOW_HIGH: &str = r#"{"chat_template_caps":{"supports_object_arguments":false,"supports_parallel_tool_calls":false,"supports_preserve_reasoning":false,"supports_reasoning_effort":true,"supports_string_content":true,"supports_system_role":true,"supports_tool_calls":false,"supports_tools":false,"supports_typed_content":false,"reasoning_effort_levels":["low","high"]},"total_slots":4}"#;

    #[test]
    fn props_levels_come_from_the_chat_template_caps() {
        assert_eq!(parse_levels(PROPS_LOW_HIGH), ["low", "high"]);
        // A template without effort, an older engine, and a broken reply.
        assert_eq!(
            parse_levels(r#"{"chat_template_caps":{"reasoning_effort_levels":[]}}"#),
            Vec::<String>::new()
        );
        assert_eq!(
            parse_levels(r#"{"chat_template_caps":{"supports_tools":true}}"#),
            Vec::<String>::new()
        );
        assert_eq!(parse_levels("not json"), Vec::<String>::new());
        assert_eq!(
            parse_levels(
                r#"{"chat_template_caps":{"reasoning_effort_levels":["minimal",3," max "]}}"#
            ),
            ["minimal", "max"]
        );
    }

    #[test]
    fn an_effort_goes_out_as_the_nearest_level_the_model_renders() {
        let low_high = levels(&["low", "high"]);
        assert_eq!(nearest("high", &low_high).as_deref(), Some("high"));
        assert_eq!(nearest("xhigh", &low_high).as_deref(), Some("high"));
        assert_eq!(nearest("max", &low_high).as_deref(), Some("high"));
        assert_eq!(nearest("minimal", &low_high).as_deref(), Some("low"));
        assert_eq!(nearest("none", &low_high).as_deref(), Some("low"));
        // Between low and high is the template's own default: send nothing.
        assert_eq!(nearest("medium", &low_high), None);
        let all = levels(&["minimal", "low", "medium", "high", "xhigh", "max"]);
        assert_eq!(nearest("medium", &all).as_deref(), Some("medium"));
        assert_eq!(nearest("none", &all).as_deref(), Some("minimal"));
        let two = levels(&["medium", "high"]);
        assert_eq!(nearest("low", &two).as_deref(), Some("medium"));
        assert_eq!(nearest("xhigh", &two).as_deref(), Some("high"));
        // Case and spaces don't matter; the ladder's own spelling goes out.
        assert_eq!(
            nearest(" XHigh ", &levels(&["High"])).as_deref(),
            Some("high")
        );
    }

    #[test]
    fn no_list_or_an_unknown_effort_sends_nothing() {
        assert_eq!(nearest("high", &[]), None);
        assert_eq!(nearest("high", &levels(&["turbo"])), None);
        assert_eq!(nearest("fast", &levels(&["low", "high"])), None);
    }

    #[test]
    fn levels_are_read_only_from_loopback() {
        assert_eq!(
            fetch_levels("192.0.2.1", 80, "sk-abcdefghijklmnopqrstuv", PROPS_TIMEOUT),
            Vec::<String>::new()
        );
    }
}
