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
    let github = json!({ "name": "github", "url": "https://api.example.com/mcp", "needs_token": true });
    assert_eq!(self_class("connection_add", &github), Some(SelfClass::Credentials));
    assert_eq!(self_class("connection_modify", &github), Some(SelfClass::Credentials));
    assert_eq!(self_class("connection_add", &json!({ "name": "fs", "command": "fs-mcp" })), Some(SelfClass::Soft));
    assert_eq!(self_class("connection_add", &json!({ "name": "fs", "needs_token": false })), Some(SelfClass::Soft));
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
    let cred = json!({ "name": "github", "url": "https://api.example.com/mcp", "needs_token": true, "reason": "PRs" });
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

/// `run` with no token card (none of these calls needs one).
fn run_with(ctx: &SelfCtx<'_>, name: &str, args: &serde_json::Value) -> crate::tools::ToolOutput {
    run(ctx, name, args, &mut |_| None)
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
        let out = run_with(&ctx, "skill_create", &mk(n));
        assert!(!out.failed, "{}", out.text);
        assert_eq!(out.text, format!("created skill skill-{n} (change #{}; the user can Undo it)", n + 1));
    }
    let ledger = ChangeLedger::load(&root);
    assert_eq!(ledger.all().len(), 5);
    let c = &ledger.all()[0];
    assert_eq!((c.id.as_str(), c.op.as_str(), c.origin.as_str(), c.reason.as_str()), ("skill-0", "create", "self_manage", "seen twice"));
    assert_eq!(skill_creates_today(&root, ctx.now_ms), 5);
    let capped = run_with(&ctx, "skill_create", &mk(5));
    assert!(capped.failed);
    assert_eq!(capped.text, "skill_create is capped at 5 new skills a day; ask the user before adding more");
    assert!(!root.join("skills").join("skill-5").exists());
    // A day later the cap has room again.
    let tomorrow = SelfCtx { now_ms: ctx.now_ms + 24 * 60 * 60 * 1000 + 60_000, ..SelfCtx::new(&root) };
    assert_eq!(skill_creates_today(&root, tomorrow.now_ms), 0);
    assert!(!run_with(&tomorrow, "skill_create", &mk(5)).failed);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn skill_modify_and_delete_keep_the_prior_version() {
    let root = dir("skill-mod");
    let ctx = SelfCtx::new(&root);
    assert!(!run_with(&ctx, "skill_create", &json!({ "name": "Weekly Report", "instructions": "open report.md" })).failed);
    let path = root.join("skills").join("weekly-report").join("SKILL.md");
    let v1 = std::fs::read(&path).unwrap();
    let out = run_with(&ctx, "skill_modify", &json!({ "name": "weekly-report", "instructions": "open report.md, then save a PDF", "reason": "user asked for PDF" }));
    assert_eq!(out.text, "changed skill weekly-report (change #2; the user can Undo it)");
    assert!(std::fs::read_to_string(&path).unwrap().contains("then save a PDF"));
    let out = run_with(&ctx, "skill_delete", &json!({ "name": "weekly-report", "reason": "stale" }));
    assert_eq!(out.text, "deleted skill weekly-report (change #3; its last version is kept)");
    assert!(!path.exists());
    let ledger = ChangeLedger::load(&root);
    let ops: Vec<&str> = ledger.all().iter().map(|c| c.op.as_str()).collect();
    assert_eq!(ops, vec!["create", "modify", "delete"]);
    assert_eq!(ledger.all()[0].after_hash, crate::harness::content_hash(&v1));
    assert!(run_with(&ctx, "skill_modify", &json!({ "name": "weekly-report" })).failed);
    // Secrets never land in a skill.
    let leak = run_with(&ctx, "skill_create", &json!({ "name": "leak", "instructions": "use sk-abcdefghijklmnopqrstuv" }));
    assert_eq!((leak.failed, leak.text.as_str()), (true, "Secrets never in markdown"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn skill_disable_and_enable_move_the_folder_and_log_each_move() {
    let root = dir("skill-off");
    let ctx = SelfCtx::new(&root);
    assert!(!run_with(&ctx, "skill_create", &json!({ "name": "board", "instructions": "open the board" })).failed);
    let on = root.join("skills").join("board").join("SKILL.md");
    let v1 = std::fs::read(&on).unwrap();
    let off = run_with(&ctx, "skill_disable", &json!({ "name": "board", "reason": "noisy" }));
    assert_eq!(off.text, "turned off skill board (change #2; the user can Undo it)");
    assert!(!on.exists());
    assert_eq!(std::fs::read(root.join("skills").join(".disabled").join("board").join("SKILL.md")).unwrap(), v1);
    assert_eq!(run_with(&ctx, "skill_list", &json!({})).text, "no skills");
    let back = run_with(&ctx, "skill_enable", &json!({ "name": "board" }));
    assert_eq!(back.text, "turned on skill board (change #3; the user can Undo it)");
    assert_eq!(std::fs::read(&on).unwrap(), v1);
    assert_eq!(run_with(&ctx, "skill_enable", &json!({ "name": "board" })).text, "skill board is already on");
    let ledger = ChangeLedger::load(&root);
    let ops: Vec<(&str, &str)> = ledger.all().iter().map(|c| (c.op.as_str(), c.reason.as_str())).collect();
    assert_eq!(ops, vec![("create", "created by Grok"), ("delete", "noisy"), ("create", "turned back on by Grok")]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn connection_tools_use_the_spike5b_writers_and_never_touch_dot_grok() {
    use crate::harness::ChangeKind;
    let root = dir("conn");
    let cabin = root.join("GrokHub");
    let grok = root.join("home").join(".grok");
    std::fs::create_dir_all(&grok).unwrap();
    std::fs::write(grok.join("mcp.json"), r#"{"mcpServers":{"cli":{"command":"cli-mcp"}}}"#).unwrap();
    std::fs::create_dir_all(&cabin).unwrap();
    let grok_before = std::fs::read(grok.join("mcp.json")).unwrap();
    let _guard = crate::perm::ConfigGuard::set(&cabin);
    let ctx = SelfCtx::new(&cabin);
    let add = run_with(&ctx, "connection_add", &json!({ "name": "files", "command": "files-mcp", "args": ["--stdio"], "reason": "read notes" }));
    assert_eq!(add.text, "added connection files (version 1 kept). The user can undo it from the Work tree.");
    assert_eq!(run_with(&ctx, "connection_list", &json!({})).text, "files: files-mcp");
    let modify = run_with(&ctx, "connection_modify", &json!({ "name": "files", "args": ["--stdio", "--ro"], "reason": "read only" }));
    assert_eq!(modify.text, "updated connection files (version 2 kept). The user can undo it from the Work tree.");
    let saved = std::fs::read_to_string(cabin.join("mcp.json")).unwrap();
    assert!(saved.contains("files-mcp") && saved.contains("--ro"), "{saved}");
    let off = run_with(&ctx, "connection_disable", &json!({ "name": "files", "reason": "flaky" }));
    assert_eq!(off.text, "turned off connection files (version 3 kept). The user can undo it.");
    assert_eq!(run_with(&ctx, "connection_list", &json!({})).text, "files: files-mcp (off)");
    let gone = run_with(&ctx, "connection_remove", &json!({ "name": "files", "reason": "unused" }));
    assert_eq!(gone.text, "removed connection files (version 4 kept). The user can undo it.");
    let ledger = ChangeLedger::load_kind(&cabin, ChangeKind::Connection);
    let ops: Vec<(&str, &str)> = ledger
        .all()
        .iter()
        .map(|c| (c.op.as_str(), c.origin.as_str()))
        .collect();
    assert_eq!(
        ops,
        vec![("create", "self_manage"), ("modify", "self_manage"), ("modify", "self_manage"), ("delete", "self_manage")]
    );
    // Without needs_token no card shows; with it, a declined card adds nothing.
    let declined = run_with(&ctx, "connection_add", &json!({ "name": "crm", "url": "http://127.0.0.1:9/mcp", "needs_token": true }));
    assert_eq!((declined.failed, declined.text.as_str()), (true, "No token was given, so the connection was not added."));
    assert_eq!(std::fs::read(grok.join("mcp.json")).unwrap(), grok_before, "~/.grok is untouched");
    let _ = crate::harness::take_self_changes(&cabin);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn automation_tools_use_the_scheduler_writer_and_its_weekly_cap() {
    use crate::harness::ChangeKind;
    let root = dir("auto");
    let _guard = crate::perm::ConfigGuard::set(&root);
    let ctx = SelfCtx::new(&root);
    let a = run_with(&ctx, "automation_create", &json!({ "instructions": "sweep the inbox", "every": "1h", "reason": "you asked" }));
    assert!(!a.failed, "{}", a.text);
    let id = a.text.lines().next().unwrap().trim_start_matches("created ").to_string();
    assert!(id.starts_with("auto"), "{id}");
    assert!(!run_with(&ctx, "automation_create", &json!({ "instructions": "check the build", "every": "10m" })).failed);
    let third = run_with(&ctx, "automation_create", &json!({ "instructions": "water the plants", "every": "1d" }));
    assert_eq!(
        (third.failed, third.text.as_str()),
        (
            true,
            "Not added: GrokHub already made 2 automations on its own this week. Keep one from its Work-tree row, or add this one yourself on the Automations page."
        )
    );
    let changed = run_with(&ctx, "automation_modify", &json!({ "id": id, "every": "30m" }));
    assert_eq!(changed.text, format!("updated {id}\nevery 30 min"));
    assert!(run_with(&ctx, "automation_list", &json!({})).text.contains("every 30 min\tsweep the inbox"));
    assert_eq!(run_with(&ctx, "automation_disable", &json!({ "id": id })).text, format!("turned off {id}"));
    assert_eq!(run_with(&ctx, "automation_delete", &json!({ "id": id })).text, format!("deleted {id}"));
    let ledger = ChangeLedger::load_kind(&root, ChangeKind::Automation);
    let ops: Vec<&str> = ledger.all().iter().map(|c| c.op.as_str()).collect();
    assert_eq!(ops, vec!["create", "create", "modify", "modify", "delete"]);
    assert!(run_with(&ctx, "automation_modify", &json!({ "id": "auto-none" })).failed);
    let _ = crate::tools::control::take_automation_changes();
    let _ = crate::harness::take_self_changes(&root);
    let _ = std::fs::remove_dir_all(root);
}
