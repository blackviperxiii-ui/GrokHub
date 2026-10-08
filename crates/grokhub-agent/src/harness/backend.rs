//! Computer-use driver adapter. Spike-0: Grok Build CU. Spike-2a adds Cua
//! Driver as a second pair of hands on Linux, behind a spike flag that is off
//! by default (`cua.rs`); both go through the same `decide`.

use crate::gate::{DeskFlags, Gate};
use crate::harness::access::AccessMode;
use crate::harness::approval::{decide, decide_harness, GateOutcome, Step, APPROVAL_TTL};
use crate::harness::hard::{
    click_action, click_rule, click_target, credential_action, credential_field, delete_files_action, HardClass, TARGET_HINT,
};
use crate::harness::park::{post_park, wait_park, ParkRequest};
use crate::harness::span::{append_span, read_turn_context, redact_args, Origin, Span};
use crate::tools::ToolOutput;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerUseBackend {
    GrokBuild,
    /// Spike-2a: Cua Driver behind `grokhub --mcp-cua`. Linux only.
    CuaDriver,
}

impl ComputerUseBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GrokBuild => "grok_build",
            Self::CuaDriver => "cua",
        }
    }

    /// Cua only when the `cuaDriver` spike flag and desktop control are both
    /// on, and only on Linux. Everything else is Grok Build.
    pub fn selected(cua_flag: bool, desktop_control: bool) -> Self {
        if cua_flag && desktop_control && cfg!(target_os = "linux") {
            Self::CuaDriver
        } else {
            Self::GrokBuild
        }
    }
}

pub struct ClickRequest<'a> {
    pub session_id: &'a str,
    pub x: f64,
    pub y: f64,
    pub claim: &'a str,
    pub config_dir: &'a Path,
    pub gate: &'a Gate,
    pub access: AccessMode,
    pub desk: Option<DeskFlags>,
    pub execute: &'a dyn Fn(f64, f64) -> ToolOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickOutcome {
    Refused(String),
    Parked(String),
    Done { result: String, span_path: String },
}

pub fn grok_build_click(req: ClickRequest<'_>) -> ClickOutcome {
    let args = format!(r#"{{"x":{},"y":{}}}"#, req.x, req.y);
    let outcome = decide_harness(req.gate, "click", &args, false, req.desk, req.access);
    match outcome {
        GateOutcome::Refuse { reason } => {
            let span = Span::deny(req.session_id, "click", &args, &reason, "soft");
            let _ = append_span(req.config_dir, &span);
            ClickOutcome::Refused(reason)
        }
        GateOutcome::Park { reason, .. } => {
            let span = Span {
                session_id: req.session_id.into(),
                ts_ms: span_now(),
                tool: "click".into(),
                args_redacted: args.clone(),
                result: "parked".into(),
                claim: reason.clone(),
                access: req.access.as_str().into(),
                approval_class: "soft".into(),
                decision: "park".into(),
                driver: ComputerUseBackend::GrokBuild.as_str().into(),
                hard_approved: false,
                path: "A".into(),
                chat_id: String::new(),
                turn: 0,
                ui_changed: None,
                origin: Origin::User,
                consent_ref: String::new(),
                undo_ref: String::new(),
                target: String::new(),
                target_rule: String::new(),
                usage: None,
                episode: String::new(),
                goal_step: String::new(),
                tokens: None,
                route: None,
            };
            let _ = append_span(req.config_dir, &span);
            ClickOutcome::Parked(reason)
        }
        GateOutcome::Allow => {
            let output = (req.execute)(req.x, req.y);
            let result = if output.failed {
                format!("failed: {}", output.text)
            } else {
                output.text.clone()
            };
            let span = Span::soft_allow(
                req.session_id,
                "click",
                &args,
                &result,
                req.claim,
                req.access,
                ComputerUseBackend::GrokBuild.as_str(),
            );
            let path = append_span(req.config_dir, &span)
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            if output.failed {
                ClickOutcome::Refused(result)
            } else {
                ClickOutcome::Done {
                    result,
                    span_path: path,
                }
            }
        }
    }
}

fn span_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Span file for desktop calls made outside a known chat turn.
pub const CU_TRACE: &str = "grok-build-cu";

/// Path A verdict for one `grokhub-desktop` `tools/call`. The server keeps its
/// Settings switch, halt, and lock checks; this adds the hard floor and the
/// hard-class park in front of them. Runs under Always too.
pub fn desk_decide(tool: &str, args: &serde_json::Value) -> GateOutcome {
    decide(Step::Desk { tool, args })
}

/// Args as a desktop span stores them. Typed text keeps only its length, and
/// a credential field's `text` / `value` never leaves (Spike-1a). The cabin's
/// click hints (the AX label read at the point, the window) never leave either.
pub fn desk_args(tool: &str, args: &serde_json::Value) -> String {
    let mut args = args.clone();
    if let Some(m) = args.as_object_mut() {
        m.remove(TARGET_HINT);
        if tool == "click" || tool == "drag" {
            m.remove("window");
        }
    }
    let args = &args;
    match tool {
        "type" => {
            let n = args
                .get("text")
                .and_then(|v| v.as_str())
                .map(|s| s.chars().count())
                .unwrap_or(0);
            format!(r#"{{"chars":{n}}}"#)
        }
        _ if credential_field(args) => {
            let mut v = args.clone();
            for k in ["text", "value"] {
                if let Some(slot) = v.get_mut(k) {
                    *slot = serde_json::Value::String("%redacted%".into());
                }
            }
            redact_args(&v.to_string())
        }
        _ => redact_args(&args.to_string()),
    }
}

/// One path-A desktop input after the server answered. Screenshots, monitor
/// lists, and moves are not logged.
pub struct DeskCall<'a> {
    pub tool: &'a str,
    pub args: &'a serde_json::Value,
    pub ok: bool,
    pub result: &'a str,
    pub access: AccessMode,
    pub ui_changed: Option<bool>,
    /// Set when the call was parked: its hard class and whether Jeremy approved it.
    pub parked: Option<(HardClass, bool)>,
}

pub fn desk_span(call: &DeskCall<'_>, chat_id: &str, turn: u32) -> Option<Span> {
    if !matches!(call.tool, "click" | "drag" | "scroll" | "type" | "key" | "open_app" | "focus_window" | "delete_files") {
        return None;
    }
    let trace = if chat_id.is_empty() { CU_TRACE } else { chat_id };
    let args = desk_args(call.tool, call.args);
    let class = call.parked.map_or("soft", |(c, _)| c.as_str());
    let span = if call.ok {
        let mut s = Span::soft_allow(
            trace,
            call.tool,
            &args,
            call.result,
            &format!("grokhub-desktop {}", call.tool),
            call.access,
            ComputerUseBackend::GrokBuild.as_str(),
        );
        s.approval_class = class.into();
        s.hard_approved = call.parked.is_some_and(|(_, approved)| approved);
        s
    } else {
        let mut s = Span::deny(trace, call.tool, &args, call.result, class);
        s.access = call.access.as_str().into();
        s.driver = ComputerUseBackend::GrokBuild.as_str().into();
        s
    };
    let span = span.on_path("A").in_turn(chat_id, turn).with_ui_changed(call.ui_changed);
    Some(with_click_target(span, call.tool, call.args))
}

/// Spike-2b: a click span says where its target came from and which rule it
/// matched (`target:"unknown"` when the AX read found nothing).
fn with_click_target(span: Span, tool: &str, args: &serde_json::Value) -> Span {
    if tool != "click" {
        return span;
    }
    let rule = click_rule(args).map(|r| r.id).unwrap_or_default();
    span.with_target(click_target(args).source, &rule)
}

/// Access for a desktop call: Readonly with the switch off, else the open
/// chat's Full grant or Supervised.
pub fn desk_access(config_dir: &Path, enabled: bool) -> AccessMode {
    if !enabled {
        return AccessMode::Readonly;
    }
    match AccessMode::parse(&read_turn_context(config_dir).access) {
        Some(AccessMode::Full) => AccessMode::Full,
        _ => AccessMode::Supervised,
    }
}

/// Post a park for the cabin's hard card and wait. True only on Jeremy's
/// Approve; TTL, a halt, or a closed cabin is Deny. `tool` and `args` are the
/// `grokhub-desktop` shape (Cua calls are mapped first, `cua_as_desk`).
pub fn park_desk_call(
    config_dir: &Path,
    tool: &str,
    args: &serde_json::Value,
    class: &str,
    driver: ComputerUseBackend,
    halted: &mut dyn FnMut() -> bool,
) -> bool {
    static N: AtomicU64 = AtomicU64::new(0);
    let id = format!("{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
    // A credential field's value never reaches the park file, the card, or a span.
    let action = match tool {
        _ if class == HardClass::Credentials.as_str() => credential_action(args),
        "type" => args["text"].as_str().unwrap_or("").to_string(),
        // The file manager does not say which files are selected.
        "key" if class == HardClass::Delete.as_str() => format!(
            "press {} on the files selected in {} (the cabin can't see which)",
            args["keys"].as_str().unwrap_or(""),
            args["window"].as_str().unwrap_or("a file manager")
        ),
        "key" => args["keys"].as_str().unwrap_or("").to_string(),
        "delete_files" => delete_files_action(args),
        "click" | "drag" => match click_rule(args) {
            Some(rule) => click_action(args, &rule),
            None => desk_args(tool, args),
        },
        _ => desk_args(tool, args),
    };
    let req = ParkRequest {
        id: id.clone(),
        path: "A".into(),
        tool: tool.into(),
        action: redact_args(&action),
        class: class.into(),
        ts_ms: span_now(),
    };
    if post_park(config_dir, &req).is_err() {
        return false;
    }
    let ctx = read_turn_context(config_dir);
    let mut park_span = Span::hard_park(
        if ctx.chat_id.is_empty() { CU_TRACE } else { &ctx.chat_id },
        tool,
        &desk_args(tool, args),
        HardClass::parse(class).unwrap_or(HardClass::IrreversibleOs),
    )
    .on_path("A")
    .in_turn(&ctx.chat_id, ctx.turn)
    .in_episode(&ctx.episode);
    park_span.access = ctx.access.clone();
    let mut park_span = with_click_target(park_span, tool, args);
    if driver == ComputerUseBackend::CuaDriver {
        park_span.driver = driver.as_str().into();
    }
    let _ = append_span(config_dir, &park_span);
    wait_park(config_dir, &id, APPROVAL_TTL, Duration::from_millis(200), halted)
}

pub fn computer_tool_names(access: AccessMode) -> &'static [&'static str] {
    if access.allows_computer() {
        &["screenshot", "click", "move", "drag", "scroll", "type", "key", "open_app", "focus_window", "delete_files"]
    } else {
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::PermMode;
    use crate::harness::access::AccessMode;
    use crate::harness::span::read_spans;

    fn gate(desktop: bool) -> Gate {
        Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop,
        }
    }

    #[test]
    fn readonly_refuses_click() {
        let dir = crate::harness::test_dir("cu-ro");
        let g = gate(true);
        let out = grok_build_click(ClickRequest {
            session_id: "s",
            x: 1.0,
            y: 2.0,
            claim: "click",
            config_dir: &dir,
            gate: &g,
            access: AccessMode::Readonly,
            desk: None,
            execute: &|_, _| ToolOutput::ok("should not run"),
        });
        assert!(matches!(out, ClickOutcome::Refused(_)), "{out:?}");
        assert!(computer_tool_names(AccessMode::Readonly).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn supervised_always_runs_soft_click_and_writes_span() {
        let dir = crate::harness::test_dir("cu-ok");
        let g = gate(true);
        let desk = Some(DeskFlags {
            halted: false,
            locked: false,
        });
        let out = grok_build_click(ClickRequest {
            session_id: "s2",
            x: 10.0,
            y: 20.0,
            claim: "clicked disposable",
            config_dir: &dir,
            gate: &g,
            access: AccessMode::Supervised,
            desk,
            execute: &|x, y| ToolOutput::ok(format!("clicked {x},{y}")),
        });
        match out {
            ClickOutcome::Done { result, .. } => {
                assert!(result.contains("clicked"), "{result}");
            }
            other => panic!("expected Done, got {other:?}"),
        }
        let spans = read_spans(&dir, "s2").unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].tool, "click");
        assert_eq!(spans[0].driver, "grok_build");
        assert_eq!(spans[0].decision, "allow");
        assert!(computer_tool_names(AccessMode::Supervised).contains(&"click"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn desk_path_a_parks_hard_typing_and_logs_soft_click() {
        let typed = serde_json::json!({ "text": "rm -f disposable.txt" });
        assert_eq!(
            desk_decide("type", &typed),
            GateOutcome::Park {
                reason: "hard-class delete: Delete — Always cannot skip".into(),
                hard: Some(crate::harness::HardClass::Delete),
                needs_jeremy: true,
            }
        );
        assert_eq!(
            desk_decide("type", &serde_json::json!({ "text": "cat ~/.ssh/id_rsa" })),
            GateOutcome::Refuse { reason: "forbidden path: ssh keys".into() }
        );
        let click = serde_json::json!({ "x": 120, "y": 48 });
        assert_eq!(desk_decide("click", &click), GateOutcome::Allow);
        let s = desk_span(
            &DeskCall {
                tool: "click",
                args: &click,
                ok: true,
                result: "clicked",
                access: AccessMode::Supervised,
                ui_changed: Some(true),
                parked: None,
            },
            "chat-1",
            2,
        )
        .unwrap();
        assert_eq!(s.session_id, "chat-1");
        assert_eq!(s.args_redacted, r#"{"x":120,"y":48}"#);
        assert_eq!(s.path, "A");
        assert_eq!(s.turn, 2);
        assert_eq!(s.ui_changed, Some(true));
        assert_eq!(s.decision, "allow");
        let t = desk_span(
            &DeskCall {
                tool: "type",
                args: &serde_json::json!({ "text": "hunter2" }),
                ok: false,
                result: "Desktop control is off.",
                access: AccessMode::Readonly,
                ui_changed: None,
                parked: None,
            },
            "",
            0,
        )
        .unwrap();
        assert_eq!(t.session_id, "grok-build-cu");
        assert_eq!(t.args_redacted, r#"{"chars":7}"#);
        assert_eq!(t.decision, "deny");
        assert_eq!(t.access, "readonly");
        // A credential field's value stays out on any desktop tool.
        let pin = serde_json::json!({ "label": "PIN", "value": "4821" });
        let a = desk_args("set_value", &pin);
        assert!(a.contains("%redacted%") && !a.contains("4821"), "{a}");
        assert_eq!(desk_args("set_value", &serde_json::json!({ "label": "Name", "value": "Ada" })), r#"{"label":"Name","value":"Ada"}"#);
        let mv = DeskCall {
            tool: "move",
            args: &click,
            ok: true,
            result: "moved",
            access: AccessMode::Supervised,
            ui_changed: None,
            parked: None,
        };
        assert_eq!(desk_span(&mv, "chat-1", 2), None);
        let rm = serde_json::json!({ "text": "rm -rf ~/old" });
        let parked = |ok: bool, approved: bool| {
            desk_span(
                &DeskCall {
                    tool: "type",
                    args: &rm,
                    ok,
                    result: if ok { "typed" } else { "Denied" },
                    access: AccessMode::Supervised,
                    ui_changed: None,
                    parked: Some((crate::harness::HardClass::Delete, approved)),
                },
                "chat-1",
                2,
            )
            .unwrap()
        };
        let denied = parked(false, false);
        assert_eq!(
            (denied.decision.as_str(), denied.approval_class.as_str(), denied.args_redacted.as_str()),
            ("deny", "delete", r#"{"chars":12}"#)
        );
        let ran = parked(true, true);
        assert_eq!((ran.decision.as_str(), ran.approval_class.as_str(), ran.hard_approved), ("allow", "delete", true));
        assert!(crate::harness::approval_gate_violation(&[ran]).is_empty(), "an approved park is not a violation");
    }
}
