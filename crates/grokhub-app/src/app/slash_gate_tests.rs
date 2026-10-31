//! Gate coverage for slash commands (rearch stage 10): every command core marks
//! `SlashEffect::Action` must reach its confirm, consent, or permission path,
//! and every `AgentTurn` command must hand off to a turn entry point.

use grokhub_core::{slash_effect, SlashEffect, SLASH_SAMPLES};

/// The body of `fn name(` in `src`, up to its closing brace at method indent.
fn method_body<'a>(src: &'a str, name: &str) -> &'a str {
    let needle = format!("fn {name}(");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("missing fn {name}"));
    let rest = &src[start..];
    // Windows checkouts embed CRLF via include_str, so accept `}\r\n` too.
    let end = rest
        .match_indices("\n    }")
        .map(|(i, m)| (i, &rest[i + m.len()..]))
        .find(|(_, after)| after.starts_with('\n') || after.starts_with("\r\n"))
        .map_or(rest.len(), |(i, _)| i);
    &rest[..end]
}

/// The `run_slash` match arm for `Slash::<variant>`.
fn run_slash_arm(variant: &str) -> &'static str {
    let run = include_str!("slash.rs")
        .split("fn run_slash(")
        .nth(1)
        .and_then(|s| s.split("fn run_slash_line(").next())
        .expect("run_slash");
    let needle = format!("Slash::{variant}");
    let start = run
        .match_indices(&needle)
        .map(|(i, _)| i)
        .find(|&i| {
            run[i + needle.len()..]
                .chars()
                .next()
                .is_some_and(|c| !c.is_ascii_alphanumeric())
        })
        .unwrap_or_else(|| panic!("run_slash has no arm for Slash::{variant}"));
    let rest = &run[start..];
    let end = rest[needle.len()..]
        .find("\n            Slash::")
        .map(|i| i + needle.len())
        .unwrap_or(rest.len());
    &rest[..end]
}

fn variant_of(line: &str) -> (String, SlashEffect) {
    let s = grokhub_core::parse_slash(line).unwrap_or_else(|| panic!("{line} parses"));
    let name = format!("{s:?}")
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or("")
        .to_string();
    (name, slash_effect(&s))
}

struct GateRow {
    variant: &'static str,
    /// What the `run_slash` arm must call.
    route: &'static str,
    /// (source, fn, text that fn must contain): the rail itself.
    rails: &'static [(&'static str, &'static str, &'static str)],
}

const MOD_RS: &str = include_str!("mod.rs");

const GATES: &[GateRow] = &[
    GateRow {
        variant: "Sh",
        route: "self.queue_sh(",
        rails: &[
            (MOD_RS, "queue_sh", "should_confirm_destructive_host"),
            (MOD_RS, "queue_sh", "ConfirmKind::DestructiveHost"),
        ],
    },
    GateRow {
        variant: "AlwaysApprove",
        route: "self.arm_session_always()",
        rails: &[(
            include_str!("confirm.rs"),
            "arm_session_always",
            "ConfirmKind::AlwaysSession",
        )],
    },
    GateRow {
        variant: "AutoPerm",
        route: "self.set_permission_mode(PermissionMode::Auto)",
        rails: &[(MOD_RS, "set_permission_mode", "self.permission_mode")],
    },
    GateRow {
        variant: "Record",
        route: "self.start_screen_recording(",
        rails: &[
            (
                include_str!("screen_record_ui.rs"),
                "start_screen_recording",
                "self.start_screen_recording_under(",
            ),
            (
                include_str!("screen_record_ui.rs"),
                "start_screen_recording_under",
                "start(self.cfg.screen_record",
            ),
            (
                include_str!("screen_record_ui.rs"),
                "start_screen_recording_under",
                "Guarded::ScreenRecording",
            ),
        ],
    },
    GateRow {
        variant: "Inhabit",
        route: "self.queue_inhabit(",
        rails: &[(MOD_RS, "queue_inhabit", "inhabit_ready(")],
    },
    GateRow {
        variant: "Sync",
        route: "self.sync_hub()",
        rails: &[
            (MOD_RS, "sync_hub", "self.gate_hub_sync()"),
            (
                include_str!("privacy_ui.rs"),
                "gate_hub_sync",
                "hx::decide(step)",
            ),
        ],
    },
    GateRow {
        variant: "Update",
        route: "self.queue_update()",
        rails: &[
            (MOD_RS, "queue_update", "self.queue_cabin_update()"),
            (MOD_RS, "queue_cabin_update", "update_wipes_config(&cmds)"),
        ],
    },
];

/// Entry points that start a model or agent turn. The turn's tools go through
/// the agent's own gate.
const TURN_ROUTES: &[&str] = &[
    "self.send_chat(",
    "self.kick_model_retry(",
    "self.queue_or_send(",
    "self.dispatch_send(",
    "self.run_bg_slash(",
    "self.run_consult(",
    "self.native_compact_if_current()",
    "self.kick_imagine()",
    "self.add_automation_seed(",
];

#[test]
fn every_slash_action_goes_through_its_gate() {
    let native = method_body(include_str!("slash.rs"), "dispatch_native_slash");
    let mut actions = Vec::new();
    for line in SLASH_SAMPLES {
        let (variant, effect) = variant_of(line);
        match effect {
            SlashEffect::Action => {
                let row = GATES
                    .iter()
                    .find(|r| r.variant == variant)
                    .unwrap_or_else(|| {
                        panic!("{line} is an Action with no gate row; add its confirm path here")
                    });
                let arm = run_slash_arm(&variant);
                assert!(
                    arm.contains(row.route),
                    "{line} must route through {}: {arm}",
                    row.route
                );
                for (src, func, rail) in row.rails {
                    let body = method_body(src, func);
                    assert!(body.contains(rail), "{func} lost its rail {rail}: {body}");
                }
                assert!(
                    !native.contains(&format!("Slash::{variant}")),
                    "{line} must not skip the cabin arm on native threads: {native}"
                );
                actions.push(variant);
            }
            SlashEffect::AgentTurn => {
                let arm = run_slash_arm(&variant);
                assert!(
                    TURN_ROUTES.iter().any(|r| arm.contains(r)),
                    "{line} starts a turn but calls no turn entry point: {arm}"
                );
            }
            SlashEffect::Local => {}
        }
    }
    actions.sort();
    actions.dedup();
    let mut rows: Vec<String> = GATES.iter().map(|r| r.variant.to_string()).collect();
    rows.sort();
    assert_eq!(actions, rows, "every gate row must still be an Action");
}

#[test]
fn queue_sh_confirms_before_it_runs() {
    let body = method_body(MOD_RS, "queue_sh");
    let ask = body
        .find("should_confirm_destructive_host")
        .expect("confirm check");
    let run = body.find("self.run_cmds(").expect("run_cmds");
    assert!(ask < run, "{body}");
}
