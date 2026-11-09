//! `local_ai_status` (read-only) and `local_ai_selftest` (asks first): the
//! agent can answer "is my local model working?" from facts instead of
//! saying it has no access. The self-test talks to the on-device runtime
//! directly, so it never needs the network, and it only runs when called.

use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

use crate::route::local_health::{self as health, Event};
use crate::route::{connectivity, local, local_doctor, local_sidecar};

use super::ToolOutput;

pub const STATUS: &str = "local_ai_status";
pub const SELFTEST: &str = "local_ai_selftest";
/// Health lines the status shows.
const RECENT: usize = 5;
const SELFTEST_PROMPT: &str = "Reply with the single word OK.";

pub fn status_schema() -> Value {
    json!({
        "type": "function",
        "name": STATUS,
        "description": "Check the on-device (local) model: why it is or isn't ready, read from its model folders, plus engine, pick, drafter, loaded or idle, network and recent health. Use it when asked whether the local model works; never ask the user to check files.",
        "parameters": {
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false
        }
    })
}

pub fn selftest_schema() -> Value {
    json!({
        "type": "function",
        "name": SELFTEST,
        "description": "Load the on-device model and ask it one short question, without the network. Run only when the user asks to test Local AI.",
        "parameters": {
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false
        }
    })
}

fn event_line(e: &Event, now_ms: u64) -> String {
    let ago = now_ms.saturating_sub(e.ts_ms) / 60_000;
    let mut line = format!("- {}m ago: {}", ago, e.event);
    if let Some(d) = &e.detail {
        line.push_str(&format!(" ({d})"));
    }
    if let Some(ms) = e.ms {
        line.push_str(&format!(", {ms} ms"));
    }
    line
}

/// The status report for `config_dir`.
pub fn status_text(config_dir: &Path, now_ms: u64) -> String {
    let runtime = local_sidecar::runtime(config_dir);
    let path_or = |p: Option<std::path::PathBuf>, none: &str| {
        p.map(|p| p.display().to_string())
            .unwrap_or_else(|| none.to_string())
    };
    let loaded = match local_sidecar::global_pid() {
        Some(pid) => format!("loaded (pid {pid})"),
        None => "idle (starts on the next local request)".into(),
    };
    let events = health::read(config_dir);
    let check = local_doctor::diagnose(config_dir);
    let mut out = vec![
        format!("Check: {}", check.summary()),
        format!(
            "Local AI: {}",
            if local::switched_on() { "on" } else { "off" }
        ),
        format!("Runtime: {}", local_sidecar::runtime_label(&runtime)),
        format!(
            "Engine: {}",
            path_or(local_sidecar::find_binary(config_dir), "not found")
        ),
        format!(
            "Model: {}",
            path_or(local_sidecar::find_model(config_dir), "not found")
        ),
        format!("Loaded: {loaded}"),
        format!("Network: {}", connectivity::state().as_str()),
    ];
    // The folders, the pick and the drafter; the switch and engine are above.
    out.extend(
        check
            .facts
            .iter()
            .filter(|f| !f.starts_with("Local AI switch:") && !f.starts_with("Engine:"))
            .map(|f| format!("- {f}")),
    );
    let recent: Vec<String> = events
        .iter()
        .rev()
        .take(RECENT)
        .rev()
        .map(|e| event_line(e, now_ms))
        .collect();
    if recent.is_empty() {
        out.push(format!(
            "Recent health: none yet ({})",
            health::path(config_dir).display()
        ));
    } else {
        out.push("Recent health:".into());
        out.extend(recent);
    }
    let offline = events.iter().rev().find(|e| {
        e.event == "offline_turn" || (e.event == "net" && e.detail.as_deref() != Some("online"))
    });
    out.push(match offline {
        Some(e) => format!("Last offline event:\n{}", event_line(e, now_ms)),
        None => "Last offline event: none".into(),
    });
    out.join("\n")
}

pub fn status(_args: &Value) -> ToolOutput {
    let dir = crate::perm::config_dir();
    let mut text = status_text(&dir, grokhub_core::now_ms());
    // The reason is read: the next message starts the model again instead
    // of waiting out the retry pause.
    if local::start_failure().is_some() {
        local::clear_start_error();
        text.push_str("\nCleared the failed start, so the next message starts the model again.");
    }
    ToolOutput::ok(text)
}

/// Ask `rt` the one question; the report names the time and the answer.
pub fn selftest_with(rt: &dyn local::LocalRuntime) -> Result<String, String> {
    let began = Instant::now();
    let answer = rt.chat_stream(
        &[local::ChatMessage {
            role: "user".into(),
            content: SELFTEST_PROMPT.into(),
        }],
        &mut |_| {},
    )?;
    let ms = began.elapsed().as_millis();
    let answer: String = answer.trim().chars().take(120).collect();
    if answer.is_empty() {
        return Err(format!("the local model answered nothing after {ms} ms"));
    }
    Ok(format!(
        "Self-test passed: the local model answered in {ms} ms without the network: \"{answer}\""
    ))
}

pub fn selftest(_args: &Value) -> ToolOutput {
    let result = match local::runtime() {
        Some(rt) => selftest_with(rt.as_ref()),
        None => Err(local::unavailable_reason()),
    };
    match result {
        Ok(report) => {
            health::log("selftest", Some("passed"));
            ToolOutput::ok(report)
        }
        Err(why) => {
            health::log("selftest", Some(&why));
            ToolOutput::err(format!("Self-test failed: {why}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Says(&'static str);

    impl local::LocalRuntime for Says {
        fn classify(&self, _: &str, _: &[&str]) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn summarize(&self, _: &str, _: u32) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn difficulty(&self, _: &str) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn chat_stream(
            &self,
            messages: &[local::ChatMessage],
            _on_delta: &mut dyn FnMut(&str),
        ) -> Result<String, String> {
            assert_eq!(messages[0].content, SELFTEST_PROMPT);
            Ok(self.0.into())
        }
    }

    #[test]
    fn the_status_names_paths_state_health_and_the_last_offline_event() {
        let dir = std::env::temp_dir().join(format!("gh-local-ai-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        local::set_enabled(false);
        connectivity::set_state(connectivity::NetState::Online);
        health::log_to(&dir, "net", Some("offline"), None);
        health::log_to(
            &dir,
            "offline_turn",
            Some("chat:default → offline:local_unavailable"),
            None,
        );
        // Two minutes after the newest line.
        let now = health::read(&dir).last().map(|e| e.ts_ms).unwrap();
        let text = status_text(&dir, now + 120_000);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "Check: Local AI is switched off, so the model on disk isn't used."
        );
        assert_eq!(lines[1], "Local AI: off");
        assert!(lines[2].starts_with("Runtime: "), "{text}");
        assert!(lines[3].starts_with("Engine: "), "{text}");
        assert!(lines[4].starts_with("Model: "), "{text}");
        assert_eq!(lines[5], "Loaded: idle (starts on the next local request)");
        assert_eq!(lines[6], "Network: online");
        let health = lines
            .iter()
            .position(|l| *l == "Recent health:")
            .expect("health lines");
        assert!(
            lines[7..health].iter().all(|l| l.starts_with("- ")),
            "{text}"
        );
        assert!(
            lines[7..health]
                .iter()
                .any(|l| l.starts_with("- The engine ")),
            "{text}"
        );
        assert_eq!(
            &lines[health..],
            [
                "Recent health:",
                "- 2m ago: net (offline)",
                "- 2m ago: offline_turn (chat:default → offline:local_unavailable)",
                "Last offline event:",
                "- 2m ago: offline_turn (chat:default → offline:local_unavailable)",
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_selftest_reports_the_answer_or_says_what_failed() {
        let ok = selftest_with(&Says(" OK \n")).unwrap();
        assert!(
            ok.starts_with("Self-test passed: the local model answered in ")
                && ok.ends_with(" ms without the network: \"OK\""),
            "{ok}"
        );
        let err = selftest_with(&Says("  ")).unwrap_err();
        assert!(
            err.starts_with("the local model answered nothing after "),
            "{err}"
        );
        // No runtime: the failure names why.
        local::set_enabled(false);
        let out = selftest(&json!({}));
        assert_eq!(
            (out.failed, out.text.as_str()),
            (true, "Self-test failed: Local AI is off in Settings")
        );
    }
}
