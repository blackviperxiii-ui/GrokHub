//! Reasoning effort for the on-device model. llama-server's `GET /props`
//! says two things about how the model thinks, and the sidecar reads both
//! once after it starts ([`fetch_thinking`]):
//!
//! - `chat_template_caps.reasoning_effort_levels`: the `reasoning_effort`
//!   values the model's chat template renders differently from its own
//!   default, lowest first (b12017).
//! - `chat_template`: the template itself. One that reads `enable_thinking`
//!   (or a bare `thinking` flag) turns thinking on and off through
//!   `chat_template_kwargs`, which llama-server takes on every request.
//!
//! A local turn's effort, a rung of [`EFFORT_LADDER`], then goes out as
//! [`Thinking::plan`] says: `none` and `minimal` turn thinking off where the
//! template has the switch; `low` and up turn it on and send the nearest
//! listed level ([`nearest`]). A template with neither gets nothing, and the
//! model thinks as it always does. A thinking budget is a launch flag
//! (`--reasoning-budget`), not a request field, so a turn never sets one.
//!
//! The template's default level is never in the list: a template that
//! defaults to `medium` and also renders `low` and `high` lists
//! `["low", "high"]`. So an effort exactly between two listed levels sends
//! no level and the template keeps its default.

use std::time::Duration;

use grokhub_core::model_registry::EFFORT_LADDER;
use serde_json::{json, Value};

use super::ladder::rung;

/// How long the one `/props` read may take.
pub const PROPS_TIMEOUT: Duration = Duration::from_secs(2);

/// How the running model's template thinks, from one `/props` read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Thinking {
    /// The `reasoning_effort` levels the template renders, lowest first.
    pub levels: Vec<String>,
    /// The template variable that turns thinking on and off, when it reads one.
    pub switch: Option<&'static str>,
}

/// What one local request sends for its effort.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sent {
    /// `reasoning_effort`: a level the template renders.
    pub effort: Option<String>,
    /// `chat_template_kwargs.{switch}`: thinking on or off.
    pub thinking: Option<(&'static str, bool)>,
}

impl Thinking {
    /// What a turn at effort `want` sends. `None` (or an effort off the
    /// ladder) sends nothing, so the template keeps its own default.
    pub fn plan(&self, want: Option<&str>) -> Sent {
        let Some(want) = want.filter(|w| rung(w).is_some()) else {
            return Sent::default();
        };
        let off = rung(want) <= rung("minimal");
        match self.switch {
            // Thinking off makes a level moot, so none goes with it.
            Some(switch) if off => Sent {
                effort: None,
                thinking: Some((switch, false)),
            },
            Some(switch) => Sent {
                effort: nearest(want, &self.levels),
                thinking: Some((switch, true)),
            },
            // No switch: the lowest listed level is as far down as it goes.
            None => Sent {
                effort: nearest(want, &self.levels),
                thinking: None,
            },
        }
    }
}

impl Sent {
    /// Put the effort on a `/v1/chat/completions` body.
    pub fn apply(&self, body: &mut Value) {
        if let Some(e) = &self.effort {
            body["reasoning_effort"] = json!(e);
        }
        if let Some((switch, on)) = self.thinking {
            body["chat_template_kwargs"] = json!({ switch: on });
        }
    }
}

/// `chat_template_caps.reasoning_effort_levels` of a `/props` reply. Empty
/// when the reply has no list (an older engine) or isn't JSON.
pub fn parse_levels(props_json: &str) -> Vec<String> {
    serde_json::from_str::<Value>(props_json)
        .map(|v| levels_of(&v))
        .unwrap_or_default()
}

fn levels_of(v: &Value) -> Vec<String> {
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

/// The thinking switch a chat template reads: `enable_thinking` (Qwen3,
/// GLM and the like), else a bare `thinking` flag tested in a Jinja
/// condition (`if thinking`, `thinking is defined`). Prose that only says
/// "thinking" is no switch.
fn template_switch(template: &str) -> Option<&'static str> {
    let words: Vec<&str> = template
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .collect();
    if words.contains(&"enable_thinking") {
        return Some("enable_thinking");
    }
    let flag = words.iter().enumerate().any(|(i, w)| {
        *w == "thinking"
            && (i
                .checked_sub(1)
                .is_some_and(|p| matches!(words[p], "if" | "elif" | "not" | "and" | "or"))
                || words.get(i + 1) == Some(&"is"))
    });
    flag.then_some("thinking")
}

/// The levels and the switch of a `/props` reply. Empty when the reply has
/// neither, or isn't JSON.
pub fn parse_thinking(props_json: &str) -> Thinking {
    let Ok(v) = serde_json::from_str::<Value>(props_json) else {
        return Thinking::default();
    };
    Thinking {
        levels: levels_of(&v),
        switch: v
            .get("chat_template")
            .and_then(Value::as_str)
            .and_then(template_switch),
    }
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

/// Read how the model thinks from a running llama-server on loopback, with
/// its API key (`/props` answers 401 without it). Empty on any failure, so a
/// turn then sends no effort.
pub fn fetch_thinking(host: &str, port: u16, token: &str, timeout: Duration) -> Thinking {
    if !super::local_sidecar::is_loopback_host(host) {
        return Thinking::default();
    }
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: timeout,
        read: Some(timeout),
        ..Default::default()
    });
    let resp = agent
        .get(&format!("http://{host}:{port}/props"))
        .header("Authorization", &format!("Bearer {token}"))
        .call();
    match resp {
        Ok(r) if r.status() == 200 => parse_thinking(&r.into_string().unwrap_or_default()),
        _ => Thinking::default(),
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
            fetch_thinking("192.0.2.1", 80, "sk-abcdefghijklmnopqrstuv", PROPS_TIMEOUT),
            Thinking::default()
        );
    }

    /// A Qwen3-style template: `enable_thinking` decides, no effort levels.
    const PROPS_QWEN: &str = r#"{"chat_template":"{%- if enable_thinking is defined and enable_thinking is false %}{{- '<think>\n\n</think>\n\n' }}{%- endif %}","chat_template_caps":{"supports_reasoning_effort":false},"total_slots":1}"#;
    /// A DeepSeek-V3.1-style template: a bare `thinking` flag.
    const PROPS_FLAG: &str = r#"{"chat_template":"{% if not thinking is defined %}{% set thinking = false %}{% endif %}{% if thinking %}<think>{% else %}</think>{% endif %}"}"#;

    #[test]
    fn the_thinking_switch_comes_from_the_chat_template() {
        assert_eq!(
            parse_thinking(PROPS_QWEN),
            Thinking {
                levels: Vec::new(),
                switch: Some("enable_thinking"),
            }
        );
        assert_eq!(parse_thinking(PROPS_FLAG).switch, Some("thinking"));
        assert_eq!(
            parse_thinking(PROPS_LOW_HIGH),
            Thinking {
                levels: levels(&["low", "high"]),
                switch: None,
            }
        );
        // Prose that only says "thinking" is no switch, nor is a longer name.
        assert_eq!(
            parse_thinking(
                r#"{"chat_template":"Show your thinking step by step. {{ thinking_budget }}"}"#
            )
            .switch,
            None
        );
        assert_eq!(parse_thinking("not json"), Thinking::default());
    }

    fn body(sent: &Sent) -> Value {
        let mut b = json!({"model": "local"});
        sent.apply(&mut b);
        b
    }

    #[test]
    fn none_and_minimal_turn_thinking_off_and_low_and_up_turn_it_on() {
        let qwen = parse_thinking(PROPS_QWEN);
        for off in ["none", "minimal"] {
            assert_eq!(
                body(&qwen.plan(Some(off))),
                json!({"model": "local", "chat_template_kwargs": {"enable_thinking": false}})
            );
        }
        for on in ["low", "medium", "high", "xhigh", "max"] {
            assert_eq!(
                body(&qwen.plan(Some(on))),
                json!({"model": "local", "chat_template_kwargs": {"enable_thinking": true}})
            );
        }
        // No effort, or one off the ladder: the template's own default.
        assert_eq!(qwen.plan(None), Sent::default());
        assert_eq!(qwen.plan(Some("fast")), Sent::default());
        assert_eq!(
            body(&parse_thinking(PROPS_FLAG).plan(Some("none"))),
            json!({"model": "local", "chat_template_kwargs": {"thinking": false}})
        );
    }

    #[test]
    fn a_switch_and_levels_together_send_both_on_and_only_the_switch_off() {
        let both = Thinking {
            levels: levels(&["low", "high"]),
            switch: Some("enable_thinking"),
        };
        assert_eq!(
            body(&both.plan(Some("xhigh"))),
            json!({
                "model": "local",
                "reasoning_effort": "high",
                "chat_template_kwargs": {"enable_thinking": true}
            })
        );
        assert_eq!(
            body(&both.plan(Some("minimal"))),
            json!({"model": "local", "chat_template_kwargs": {"enable_thinking": false}})
        );
        // Without a switch, the lowest level is as low as it goes.
        let levels_only = Thinking {
            levels: levels(&["low", "high"]),
            switch: None,
        };
        assert_eq!(
            body(&levels_only.plan(Some("none"))),
            json!({"model": "local", "reasoning_effort": "low"})
        );
        assert_eq!(
            body(&Thinking::default().plan(Some("high"))),
            json!({"model": "local"})
        );
    }
}
