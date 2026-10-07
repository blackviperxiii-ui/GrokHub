//! Spike-1c path D: Grok Build's own (built-in) computer use.
//!
//! These calls never reach the cabin's `grokhub-desktop` MCP (path A). Under
//! Ask they come as a permission ask (path B classifies it). Under Auto or
//! Always no ask reaches the cabin, so two layers apply:
//! - The lock: `--deny` rules on every spawn (`grokhub_acp::BUILTIN_CU_DENY`)
//!   steer desktop work to the gated path A tools. GB rules can only name a
//!   tool by an MCP name, so a built-in tool is a `GB_DENY_GAPS` entry.
//! - The belt: the cabin watches each computer-use frame it made no decision
//!   for and runs it through [`decide`] here. A fast step can finish before
//!   its first frame arrives, so the belt can stop the turn after the step,
//!   never before it. It is a check on what slipped through, not the gate.

use crate::harness::access::AccessMode;
use crate::harness::approval::{decide, GateOutcome, Step};
use crate::harness::hard::HardClass;

/// Name words of a tool that drives the screen, mouse, or keyboard.
const CU_WORDS: &[&str] = &["computer", "screenshot", "click", "mouse", "keyboard", "desktop"];

/// Lowercase words of a tool name, split on anything not alphanumeric.
fn name_words(name: &str) -> Vec<String> {
    name.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// A Grok Build computer-use tool that drives the screen, mouse, or keyboard
/// and is not the cabin's own `grokhub-desktop` or `grokhub-cua` (path A gates those). `name`
/// must be a bare tool name (`computer_click`, `computer-use__type`); a card
/// title like ``Read `click.rs` `` is a file tool, not computer use. Browser
/// tools (`browser_tab`, …) drive a page, not the desktop, and stay untouched.
pub fn builtin_cu(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || name.contains(|c: char| c.is_whitespace() || matches!(c, '`' | '/' | '\\')) {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    if grokhub_core::CABIN_CU_SERVERS.iter().any(|s| lower.contains(s)) {
        return false;
    }
    let words = name_words(&lower);
    if words.iter().any(|w| w == "browser") {
        return false;
    }
    words.iter().any(|w| CU_WORDS.contains(&w.as_str()))
}

/// Look-only steps (a screenshot or an accessibility snapshot). Like path A,
/// they write no span when allowed, so a few looks never read as a loop.
pub fn cu_look_only(name: &str) -> bool {
    let words = name_words(name);
    let acts = ["click", "type", "key", "press", "scroll", "drag", "move", "open", "focus", "launch", "delete"];
    words.iter().any(|w| w == "screenshot" || w == "snapshot") && !words.iter().any(|w| acts.contains(&w.as_str()))
}

/// The `grokhub-desktop` tool shape a built-in step matches, so the same
/// path A classifier reads it (typing, key chords, app names, delete).
fn desk_shape(name: &str) -> &'static str {
    let words = name_words(name);
    let has = |w: &[&str]| words.iter().any(|x| w.contains(&x.as_str()));
    if has(&["delete", "trash"]) {
        "delete_files"
    } else if has(&["type", "typing", "fill", "input", "write", "text"]) {
        "type"
    } else if has(&["key", "keys", "press", "hotkey", "shortcut", "keyboard"]) {
        "key"
    } else if has(&["open", "launch"]) {
        "open_app"
    } else if has(&["focus", "activate"]) {
        "focus_window"
    } else if has(&["drag"]) {
        "drag"
    } else if has(&["scroll"]) {
        "scroll"
    } else if has(&["screenshot", "snapshot"]) {
        "screenshot"
    } else if has(&["move"]) {
        "move"
    } else {
        "click"
    }
}

/// Path D verdict for one built-in computer-use step the cabin was never
/// asked about. `args` holds only what the frame kept (no typed text).
///
/// - Readonly: refuse. Desktop control is off, so no step runs, and no card.
/// - Floor: refuse, no bypass.
/// - Hard class: park (the cabin cancels the turn and posts a hard card).
/// - Otherwise allow.
///
/// Both classifiers go through [`decide`]: the step as its path A shape,
/// then the raw tool name (send, pay, delete, and credential name words).
pub fn decide_unasked(name: &str, args: &serde_json::Value, access: AccessMode) -> GateOutcome {
    if !access.allows_computer() {
        return GateOutcome::Refuse {
            reason: format!(
                "Access is {} — Grok Build's own computer use ran with desktop control off",
                access.label()
            ),
        };
    }
    let shape = decide(Step::Desk { tool: desk_shape(name), args });
    if !shape.is_allow() {
        return shape;
    }
    decide(Step::Tool { name, arguments: &args.to_string() })
}

/// What Grok tried, in the words of a hard card title.
pub fn unasked_title(class: HardClass) -> String {
    let what = match class {
        HardClass::Money => "spend money",
        HardClass::Send => "send something",
        HardClass::Delete => "delete something",
        HardClass::Credentials => "type into a credential field",
        HardClass::IrreversibleOs => "do an irreversible OS action",
    };
    format!("Grok tried to {what} without asking")
}

/// The card and span line for an unasked step: the tool and what the frame
/// shows about its target. A credential field never shows a value (the
/// frame keeps none).
pub fn unasked_action(name: &str, args: &serde_json::Value, class: HardClass) -> String {
    if class == HardClass::Credentials {
        return format!("{name}: type into a credential field (value hidden)");
    }
    let detail: Vec<String> = ["keys", "key", "app", "window", "label", "target", "element"]
        .iter()
        .filter_map(|k| args.get(*k).and_then(|v| v.as_str()).map(|v| format!("{k}={v}")))
        .collect();
    if detail.is_empty() {
        name.to_string()
    } else {
        format!("{name} {}", detail.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builtin_cu_is_a_bare_tool_name_off_path_a_and_off_the_browser() {
        for name in ["computer_screenshot", "computer_click", "computer-use__type", "left_click", "mouse_move", "desktop_key"] {
            assert!(builtin_cu(name), "{name}");
        }
        for name in [
            "grokhub-desktop__click",
            "grokhub-desktop__screenshot",
            "browser_tab",
            "browser_click",
            "Read `src/click.rs`",
            "run_terminal_cmd",
            "web_fetch",
            "",
        ] {
            assert!(!builtin_cu(name), "{name}");
        }
        assert!(cu_look_only("computer_screenshot"));
        assert!(cu_look_only("computer_snapshot"));
        assert!(!cu_look_only("computer_click"));
    }

    #[test]
    fn readonly_refuses_every_unasked_step_and_soft_steps_pass_under_supervised() {
        assert_eq!(
            decide_unasked("computer_screenshot", &json!({}), AccessMode::Readonly),
            GateOutcome::Refuse {
                reason: "Access is Readonly — Grok Build's own computer use ran with desktop control off".into()
            }
        );
        assert_eq!(decide_unasked("computer_click", &json!({ "x": 4, "y": 9 }), AccessMode::Supervised), GateOutcome::Allow);
        assert_eq!(decide_unasked("computer_type", &json!({ "label": "Search" }), AccessMode::Full), GateOutcome::Allow);
    }

    #[test]
    fn hard_steps_park_from_the_field_the_key_chord_or_the_name() {
        let park = |class: HardClass| GateOutcome::Park {
            reason: format!("hard-class {}: {} — Always cannot skip", class.as_str(), class.label()),
            hard: Some(class),
            needs_jeremy: true,
        };
        let full = AccessMode::Full;
        assert_eq!(decide_unasked("computer_type", &json!({ "label": "Password" }), full), park(HardClass::Credentials));
        assert_eq!(
            decide_unasked("computer_key", &json!({ "keys": "ctrl+alt+delete" }), full),
            park(HardClass::IrreversibleOs)
        );
        assert_eq!(
            decide_unasked("computer_key", &json!({ "keys": "Delete", "window": "Dolphin" }), full),
            park(HardClass::Delete)
        );
        assert_eq!(decide_unasked("computer_send_message", &json!({}), full), park(HardClass::Send));
        assert_eq!(decide_unasked("computer_checkout", &json!({}), full), park(HardClass::Money));
        assert_eq!(decide_unasked("computer_open", &json!({ "app": "shutdown" }), full), park(HardClass::IrreversibleOs));
    }

    #[test]
    fn card_words_name_the_step_never_a_value() {
        assert_eq!(unasked_title(HardClass::Send), "Grok tried to send something without asking");
        assert_eq!(
            unasked_title(HardClass::Credentials),
            "Grok tried to type into a credential field without asking"
        );
        assert_eq!(
            unasked_action("computer_type", &json!({ "label": "Password" }), HardClass::Credentials),
            "computer_type: type into a credential field (value hidden)"
        );
        assert_eq!(
            unasked_action("computer_key", &json!({ "keys": "ctrl+alt+delete" }), HardClass::IrreversibleOs),
            "computer_key keys=ctrl+alt+delete"
        );
        assert_eq!(unasked_action("computer_send_message", &json!({}), HardClass::Send), "computer_send_message");
    }
}
