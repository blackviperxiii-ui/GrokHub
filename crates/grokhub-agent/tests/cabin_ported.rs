//! Copies of egui cabin tests (crates/grokhub-app) that exercise this
//! crate's shared logic through its public API, so the coverage stays
//! when the cabin is retired. One module per cabin source file; each test
//! keeps the cabin test's name, and the cabin originals are unchanged.

/// From `crates/grokhub-app/src/app/background.rs`.
mod app_background {
    use grokhub_agent::schemas_for;
    use grokhub_agent::schemas_for_job;
    use grokhub_agent::tool_set_from_job;
    use grokhub_agent::Gate;
    use grokhub_agent::PermMode;

    fn chat_gate() -> Gate {
        Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: false,
        }
    }

    fn names(tools: &[serde_json::Value]) -> Vec<&str> {
        tools.iter().filter_map(|t| t["name"].as_str()).collect()
    }

    #[test]
    fn need_shell_background_schemas_are_lean_subset_with_shell() {
        let gate = chat_gate();
        let chat = schemas_for(&gate);
        let set = tool_set_from_job("NEED_SHELL\nfinish the weekly report");
        let job = schemas_for_job(&gate, &set);
        let chat_names = names(&chat);
        let job_names = names(&job);
        assert!(
            job_names.iter().all(|n| chat_names.contains(n)),
            "job schemas must be a subset of chat: {job_names:?}"
        );
        assert!(
            job_names.contains(&"run_terminal_command"),
            "NEED_SHELL must offer run_terminal_command: {job_names:?}"
        );
        let chat_bytes = serde_json::to_vec(&chat).unwrap().len();
        let job_bytes = serde_json::to_vec(&job).unwrap().len();
        // Lazy tools already keep chat to the core list, so the job is only
        // required to be smaller here; the ≥30% cut against the full catalog is
        // checked in grokhub-agent with lazy tools pinned off.
        assert!(
            job_bytes < chat_bytes,
            "job schema payload must be smaller than chat: job={job_bytes} chat={chat_bytes}"
        );
    }

    #[test]
    fn undeclared_job_schemas_match_full_gate_list() {
        let gate = chat_gate();
        let set = tool_set_from_job("plain weekly report with no tool tokens");
        assert!(set.is_full());
        assert_eq!(
            names(&schemas_for_job(&gate, &set)),
            names(&schemas_for(&gate))
        );
    }
}

/// From `crates/grokhub-app/src/app/native_unattended.rs`.
mod app_native_unattended {

    #[test]
    fn unattended_tool_outside_set_is_named_refusal() {
        // Tight allowlist (not TOOLS: which starts from the lean background set).
        let set = grokhub_agent::ToolSet::only(["read_file"]);
        assert!(!set.is_full());
        assert!(set.allows("read_file"));
        assert!(!set.allows("write"));
        let refusal = grokhub_agent::tool_set_refusal("write");
        assert!(
            refusal.contains(grokhub_agent::TOOL_SET_OUTSIDE),
            "{refusal}"
        );
        let session = "native-unattended-sp2-outside";
        let _guard = grokhub_agent::bind_job_tool_set(session, set.clone());
        let schemas = grokhub_agent::schemas_for_job(
            &grokhub_agent::Gate {
                mode: grokhub_agent::PermMode::Always,
                readonly_session: false,
                attended: false,
                desktop: false,
            },
            &set,
        );
        let names: Vec<&str> = schemas.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"read_file"), "{names:?}");
        assert!(!names.contains(&"write"), "{names:?}");
    }
}

/// From `crates/grokhub-app/src/app/router_ui.rs`.
mod app_router_ui {
    use grokhub_agent::route::heal::joined_messages;

    #[test]
    fn several_new_models_make_one_update() {
        let msgs = joined_messages(&["b".to_string(), "a".to_string(), "b".to_string()]);
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            (msgs[0].key.as_str(), msgs[0].text.as_str()),
            (
                "new:a,b",
                "Your plan has 2 new AI models. Auto uses them where they do best."
            )
        );
        assert!(joined_messages(&[]).is_empty());
    }
}
