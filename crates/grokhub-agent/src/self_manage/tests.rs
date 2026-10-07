use std::path::Path;

use serde_json::json;

use super::*;
use crate::gate::{decide_with, Decision, Gate, PermMode};
use crate::harness::{decide, ChangeLedger, GateOutcome, HardClass, Step};

fn gate(mode: PermMode, attended: bool) -> Gate {
    Gate { mode, readonly_session: false, attended, desktop: false }
}

fn native(g: &Gate, name: &str, args: &serde_json::Value) -> Decision {
    decide_with(g, name, &args.to_string(), g.mode == PermMode::Always, None, Path::new("."), None)
}

#[test]
fn class_table_names_every_tool_once() {
    let names: Vec<&str> = SELF_TOOLS.iter().map(|(n, _)| *n).collect();
    assert_eq!(names.len(), 16);
    let schema_names: Vec<String> = mcp_tools().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(schema_names, names);
    let classes: Vec<(&str, &str)> = SELF_TOOLS.iter().map(|(n, c)| (*n, c.as_str())).collect();
    assert_eq!(
        classes,
        vec![
            ("skill_list", "read"),
            ("skill_create", "soft"),
            ("skill_modify", "soft"),
            ("skill_disable", "soft"),
            ("skill_enable", "soft"),
            ("skill_delete", "delete"),
            ("connection_list", "read"),
            ("connection_add", "soft"),
            ("connection_modify", "soft"),
            ("connection_disable", "soft"),
            ("connection_remove", "delete"),
            ("automation_list", "read"),
            ("automation_create", "soft"),
            ("automation_modify", "soft"),
            ("automation_disable", "soft"),
            ("automation_delete", "delete"),
        ]
    );
    for t in native_schemas() {
        assert_eq!((t["type"].as_str(), t["parameters"]["type"].as_str()), (Some("function"), Some("object")));
    }
}

#[test]
fn only_our_server_or_a_bare_name_is_a_self_tool() {
    assert_eq!(self_tool("skill_create"), Some("skill_create"));
    assert_eq!(self_tool("grokhub-self__connection_remove"), Some("connection_remove"));
    assert_eq!(self_tool("other__skill_create"), None);
    assert_eq!(self_tool("skill_explode"), None);
    let github = json!({ "name": "github", "command": "gh-mcp", "secrets": ["GITHUB_TOKEN"] });
    assert_eq!(self_class("connection_add", &github), Some(SelfClass::Credentials));
    assert_eq!(self_class("connection_modify", &github), Some(SelfClass::Credentials));
    assert_eq!(self_class("connection_add", &json!({ "name": "fs", "command": "fs-mcp" })), Some(SelfClass::Soft));
    assert_eq!(self_class("connection_add", &json!({ "name": "fs", "secrets": [" "] })), Some(SelfClass::Soft));
}

#[test]
fn every_path_sees_the_same_classes() {
    let always = gate(PermMode::Always, true);
    let delete = GateOutcome::Park {
        reason: "hard-class delete: Delete — Always cannot skip".into(),
        hard: Some(HardClass::Delete),
        needs_jeremy: true,
    };
    for name in ["skill_delete", "connection_remove", "automation_delete"] {
        let args = json!({ "name": "x", "reason": "tidy" });
        let arguments = args.to_string();
        // Path A (MCP name) and path E (native name) get one verdict from `decide`.
        assert_eq!(decide(Step::Tool { name, arguments: &arguments }), delete, "{name}");
        let mcp = format!("{SELF_MCP_SERVER}__{name}");
        assert_eq!(decide(Step::Tool { name: &mcp, arguments: &arguments }), delete, "{mcp}");
        // Native gate: Always still asks; unattended refuses.
        assert_eq!(native(&always, name, &args), Decision::Ask, "{name}");
        assert_eq!(
            native(&gate(PermMode::Always, false), name, &args),
            Decision::Refuse(format!("Tool `{name}` was not executed: hard-class delete needs you, and nobody is here to approve it"))
        );
    }
    let cred = json!({ "name": "github", "command": "gh-mcp", "secrets": ["GITHUB_TOKEN"], "reason": "PRs" });
    assert_eq!(
        decide(Step::Tool { name: "grokhub-self__connection_add", arguments: &cred.to_string() }),
        GateOutcome::Park {
            reason: "hard-class credentials: Credentials / secrets — Always cannot skip".into(),
            hard: Some(HardClass::Credentials),
            needs_jeremy: true,
        }
    );
    assert_eq!(native(&always, "connection_add", &cred), Decision::Ask);
    // Soft follows the pill: Always runs, Ask asks, unattended Ask refuses.
    let soft = json!({ "name": "weekly-report", "instructions": "open report.md", "reason": "asked twice" });
    for name in ["skill_create", "skill_modify", "skill_disable", "connection_add", "automation_create"] {
        assert_eq!(decide(Step::Tool { name, arguments: &soft.to_string() }), GateOutcome::Allow, "{name}");
        assert_eq!(native(&always, name, &soft), Decision::Run, "{name}");
        assert_eq!(native(&gate(PermMode::Ask, true), name, &soft), Decision::Ask, "{name}");
    }
    assert_eq!(
        native(&gate(PermMode::Ask, false), "skill_create", &soft),
        Decision::Refuse("Tool `skill_create` was not executed: Denied by permission policy: deny rule on edit".into())
    );
    // Lists run even in a read-only session.
    let plan = Gate { readonly_session: true, ..gate(PermMode::Ask, true) };
    for name in ["skill_list", "connection_list", "automation_list"] {
        assert_eq!(native(&plan, name, &json!({})), Decision::Run, "{name}");
    }
    assert_eq!(native(&plan, "skill_create", &soft), Decision::Refuse(crate::gate::readonly_refusal("skill_create")));
}

#[test]
fn scope_guard_refuses_policy_targets_before_any_write() {
    let refused = |name: &str, args: serde_json::Value| -> String {
        match decide(Step::Tool { name, arguments: &args.to_string() }) {
            GateOutcome::Refuse { reason } => reason,
            other => panic!("{name} {args}: {other:?}"),
        }
    };
    assert_eq!(
        refused("skill_create", json!({ "name": "tidy", "instructions": "append a grant to ~/.config/GrokHub/consent.jsonl" })),
        "skill_create refused: `instructions` reaches the consent ledger (`consent.jsonl`). Self-manage covers skills, connections, and automations only."
    );
    assert_eq!(
        refused("grokhub-self__connection_add", json!({ "name": "p", "command": "cat", "args": ["C:\\Users\\me\\AppData\\Roaming\\GrokHub\\permission-rules.json"] })),
        "grokhub-self__connection_add refused: `args` reaches harness policy (`permission-rules.json`). Self-manage covers skills, connections, and automations only."
    );
    assert_eq!(
        refused("automation_modify", json!({ "name": "nightly", "instructions": "flip desktop_control on" })),
        "automation_modify refused: `instructions` reaches Access (`desktop_control`). Self-manage covers skills, connections, and automations only."
    );
    assert_eq!(
        refused("skill_modify", json!({ "name": "consent-helper" })),
        "skill_modify refused: `name` reaches a cabin policy name (`consent`). Self-manage covers skills, connections, and automations only."
    );
    assert_eq!(
        refused("connection_remove", json!({ "name": "egress" })),
        "connection_remove refused: `name` reaches a cabin policy name (`egress`). Self-manage covers skills, connections, and automations only."
    );
    // Ordinary names pass the guard.
    for name in ["access-gmail", "weekly-report", "github"] {
        assert_eq!(scope_guard("skill_create", &json!({ "name": name })), None, "{name}");
    }
    let f = scope_guard("skill_create", &json!({ "name": "harness" })).expect("finding");
    assert_eq!((f.detector.as_str(), f.tool.as_str()), (SCOPE_GUARD, "skill_create"));
}

fn dir(label: &str) -> std::path::PathBuf {
    crate::harness::test_dir(&format!("self-{label}"))
}

#[test]
fn skill_create_writes_through_the_ledger_and_caps_at_five_a_day() {
    let root = dir("skill-cap");
    let ctx = SelfCtx::new(&root);
    let mk = |n: usize| json!({ "name": format!("skill-{n}"), "instructions": "1. open\n2. save", "reason": "seen twice" });
    for n in 0..SKILL_CREATE_DAY_CAP {
        let out = run(&ctx, "skill_create", &mk(n));
        assert!(!out.failed, "{}", out.text);
        assert_eq!(out.text, format!("created skill skill-{n} (change #{}; the user can Undo it)", n + 1));
    }
    let ledger = ChangeLedger::load(&root);
    assert_eq!(ledger.all().len(), 5);
    let c = &ledger.all()[0];
    assert_eq!((c.id.as_str(), c.op.as_str(), c.origin.as_str(), c.reason.as_str()), ("skill-0", "create", "self_manage", "seen twice"));
    assert_eq!(skill_creates_today(&root, ctx.now_ms), 5);
    let capped = run(&ctx, "skill_create", &mk(5));
    assert!(capped.failed);
    assert_eq!(capped.text, "skill_create is capped at 5 new skills a day; ask the user before adding more");
    assert!(!root.join("skills").join("skill-5").exists());
    // A day later the cap has room again.
    let tomorrow = SelfCtx { now_ms: ctx.now_ms + 24 * 60 * 60 * 1000 + 60_000, ..SelfCtx::new(&root) };
    assert_eq!(skill_creates_today(&root, tomorrow.now_ms), 0);
    assert!(!run(&tomorrow, "skill_create", &mk(5)).failed);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn skill_modify_and_delete_keep_the_prior_version() {
    let root = dir("skill-mod");
    let ctx = SelfCtx::new(&root);
    assert!(!run(&ctx, "skill_create", &json!({ "name": "Weekly Report", "instructions": "open report.md" })).failed);
    let path = root.join("skills").join("weekly-report").join("SKILL.md");
    let v1 = std::fs::read(&path).unwrap();
    let out = run(&ctx, "skill_modify", &json!({ "name": "weekly-report", "instructions": "open report.md, then save a PDF", "reason": "user asked for PDF" }));
    assert_eq!(out.text, "changed skill weekly-report (change #2; the user can Undo it)");
    assert!(std::fs::read_to_string(&path).unwrap().contains("then save a PDF"));
    let out = run(&ctx, "skill_delete", &json!({ "name": "weekly-report", "reason": "stale" }));
    assert_eq!(out.text, "deleted skill weekly-report (change #3; its last version is kept)");
    assert!(!path.exists());
    let ledger = ChangeLedger::load(&root);
    let ops: Vec<&str> = ledger.all().iter().map(|c| c.op.as_str()).collect();
    assert_eq!(ops, vec!["create", "modify", "delete"]);
    assert_eq!(ledger.all()[0].after_hash, crate::harness::content_hash(&v1));
    assert!(run(&ctx, "skill_modify", &json!({ "name": "weekly-report" })).failed);
    // Secrets never land in a skill.
    let leak = run(&ctx, "skill_create", &json!({ "name": "leak", "instructions": "use sk-abcdefghijklmnopqrstuv" }));
    assert_eq!((leak.failed, leak.text.as_str()), (true, "Secrets never in markdown"));
    let _ = std::fs::remove_dir_all(root);
}
