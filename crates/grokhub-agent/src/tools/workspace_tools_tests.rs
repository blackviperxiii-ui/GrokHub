use super::*;
use crate::gate::{Gate, PermMode};

fn ws_with(sessions: u32, uses: &[(&str, &[u32])]) -> Ws {
    Ws {
        sessions,
        seen: 0,
        tools: uses
            .iter()
            .map(|(name, s)| (name.to_string(), s.to_vec()))
            .collect(),
    }
}

const BUILTINS: [(&str, bool); 6] = [
    ("read_file", false),
    ("skill", false),
    ("diagnose", false),
    ("scheduler_list", true),
    ("monitor", true),
    ("image_generate", true),
];

#[test]
fn a_young_workspace_and_every_tenth_session_get_the_default_list() {
    let quiet = ws_with(30, &[]);
    for number in [1, 5, 10, 20, 30] {
        assert_eq!(
            plan_for(&quiet, number, &BUILTINS),
            Plan::default(),
            "{number}"
        );
    }
    let plan = plan_for(&quiet, 11, &BUILTINS);
    // The coding core never leaves; the unused rest go to the index.
    assert_eq!(plan.demoted, vec!["skill", "diagnose"]);
    assert!(plan.promoted.is_empty());
}

#[test]
fn use_in_the_window_keeps_a_tool_and_brings_a_busy_one_up_front() {
    let ws = ws_with(
        24,
        &[
            // Used once, 9 sessions back: still in the window.
            ("skill", &[16]),
            // Used only 11 sessions back: out of it.
            ("diagnose", &[14]),
            ("scheduler_list", &[16, 18, 20, 22, 24]),
            ("monitor", &[21, 22, 23, 24]),
            // A use in the session being started doesn't count yet.
            ("image_generate", &[20, 21, 22, 23, 25]),
        ],
    );
    let plan = plan_for(&ws, 25, &BUILTINS);
    assert_eq!(plan.demoted, vec!["diagnose"]);
    assert_eq!(plan.promoted, vec!["scheduler_list"]);
}

#[test]
fn promotions_are_capped_busiest_first() {
    let busy: &[u32] = &[11, 12, 13, 14, 15];
    let busier: &[u32] = &[11, 12, 13, 14, 15, 16];
    let builtins = [
        ("a", true),
        ("b", true),
        ("c", true),
        ("d", true),
        ("read_file", false),
    ];
    let ws = ws_with(
        16,
        &[("a", busy), ("b", busier), ("c", busy), ("d", busier)],
    );
    let plan = plan_for(&ws, 17, &builtins);
    assert_eq!(plan.promoted, vec!["a", "b", "d"]);
    assert_eq!(plan.promoted.len(), PROMOTE_CAP);
}

#[test]
fn a_session_freezes_its_list_and_uses_are_noted_once() {
    let dir = std::env::temp_dir().join(format!("gh-ws-tools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let workspace = dir.join("project");
    let key = workspace_key(&workspace);
    assert!(!key.contains("project"), "the record holds no paths");
    // Ten sessions that never used `skill`.
    for i in 0..WINDOW {
        let session = format!("ws-old-{i}");
        start(&dir, &session, &workspace);
        note_use(&dir, &session, "read_file");
        forget(&session);
    }
    let plan = start(&dir, "ws-now", &workspace);
    assert!(plan.demotes("skill"), "{plan:?}");
    assert!(!plan.demotes("read_file"));
    // Using it mid-session changes nothing until the next session.
    note_use(&dir, "ws-now", "skill");
    note_use(&dir, "ws-now", "skill");
    assert_eq!(start(&dir, "ws-now", &workspace), plan);
    let book = read_book(&dir);
    assert_eq!(book[&key].sessions, WINDOW + 1);
    assert_eq!(book[&key].tools["skill"], vec![WINDOW + 1]);
    assert_eq!(book[&key].tools["read_file"].len(), WINDOW as usize);
    // Tools that aren't built in aren't kept.
    note_use(&dir, "ws-now", "some_mcp_tool");
    assert!(!read_book(&dir)[&key].tools.contains_key("some_mcp_tool"));
    forget("ws-now");
    let next = start(&dir, "ws-next", &workspace);
    assert!(!next.demotes("skill"), "{next:?}");
    forget("ws-next");
    let _ = std::fs::remove_dir_all(dir);
}

fn names(tools: &[serde_json::Value]) -> Vec<String> {
    tools
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn a_demoted_tool_sits_in_the_index_and_loads_alone() {
    let dir = std::env::temp_dir().join(format!("gh-ws-offer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _guard = crate::perm::ConfigGuard::set(&dir);
    let workspace = dir.join("project");
    for i in 0..WINDOW {
        let session = format!("ws-offer-old-{i}");
        start(&dir, &session, &workspace);
        for _ in 0..PROMOTE_USES {
            note_use(&dir, &session, "scheduler_list");
        }
        forget(&session);
    }
    let session = "ws-offer-now";
    start(&dir, session, &workspace);
    super::super::registry::begin_turn(session);
    let gate = Gate {
        mode: PermMode::Ask,
        readonly_session: false,
        attended: true,
        desktop: false,
    };
    let tools = super::super::registry::schemas_for(&gate);
    let listed = names(&tools);
    assert!(!listed.iter().any(|n| n == "skill"), "{listed:?}");
    assert!(listed.iter().any(|n| n == "scheduler_list"), "{listed:?}");
    // A lookup always has its tools, however long since the last search.
    assert!(listed.iter().any(|n| n == "web_search"), "{listed:?}");
    let index = tools
        .iter()
        .find(|t| t["name"] == super::super::registry::LOAD_TOOL_SCHEMA)
        .map(|t| t.to_string())
        .unwrap_or_default();
    assert!(index.contains("skill"), "{index}");
    let loaded =
        super::super::registry::run_load_tool_schema(&serde_json::json!({ "name": "skill" }));
    assert!(!loaded.failed, "{}", loaded.text);
    let after = names(&super::super::registry::schemas_for(&gate));
    assert_eq!(after.last().map(String::as_str), Some("skill"));
    forget(session);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn web_lookup_tools_are_never_demoted() {
    let quiet = ws_with(40, &[]);
    let builtins = [
        ("web_search", false),
        ("web_fetch", false),
        ("skill", false),
    ];
    let plan = plan_for(&quiet, 35, &builtins);
    assert_eq!(plan.demoted, vec!["skill"]);
}
