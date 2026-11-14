//! A scratch chat (`/scratch`) never reads the person's memory into the
//! model and never writes memory from what is said in it. The chat is
//! marked by its id, so the mark survives a restart, History and helpers
//! (a helper's id is `{parent}/{n}`).

use serde_json::Value;

/// The start of a scratch chat's id. It is a shell chat id too.
pub const PREFIX: &str = "shell-scratch-";

/// What a memory write answers in a scratch chat.
pub const NO_WRITES: &str = "Scratch — no memory writes";

/// What a memory read answers in a scratch chat.
pub const NO_READS: &str = "Scratch — this chat doesn't read your memory";

/// A shell chat id ending in `rest`: a scratch one when `scratch`, so work
/// a scratch chat starts (`/bg`, `/send`, `/consult`) stays scratch.
pub fn shell_id(scratch: bool, rest: &str) -> String {
    if scratch {
        format!("{PREFIX}{rest}")
    } else {
        format!("shell-{rest}")
    }
}

/// Whether `conversation_id` is a scratch chat or one of its helpers.
pub fn is_scratch(conversation_id: &str) -> bool {
    conversation_id.trim_start().starts_with(PREFIX)
}

/// Why a scratch chat refuses this tool call, or `None` to let it run.
/// Know me tags are personal, and a `MEMORY.md` is every chat's memory.
pub(crate) fn refuse_tool(session: &str, name: &str, args: &Value) -> Option<&'static str> {
    if !is_scratch(session) {
        return None;
    }
    let on_memory = || {
        ["path", "file_path", "target_file"]
            .iter()
            .filter_map(|key| args.get(key).and_then(Value::as_str))
            .any(|raw| crate::memory::is_memory_file(std::path::Path::new(raw.trim())))
    };
    // A self tool also answers to its server-prefixed name.
    match crate::self_manage::self_tool(name).unwrap_or(name) {
        "know_me_tags" => Some(NO_READS),
        "read_file" if on_memory() => Some(NO_READS),
        "write" | "search_replace" if on_memory() => Some(NO_WRITES),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_scratch_turn_reads_no_memory_and_a_normal_one_does() {
        let fact = |ws: &std::path::Path| {
            std::fs::write(
                crate::memory::workspace_memory_path(ws),
                "The harbor ferry leaves at seven fifteen.\n",
            )
            .unwrap();
        };
        let ask = ["when does the harbor ferry leave today"];
        let (notes, words) =
            crate::research::tests::newest_turn_in("shell-scratch-a-b", &ask, "scratch", fact);
        assert!(!notes.contains("seven fifteen"), "{notes}");
        assert_eq!(words, ask[0]);
        let (notes, _) = crate::research::tests::newest_turn_in("shell-a-b", &ask, "normal", fact);
        assert!(notes.contains("seven fifteen"), "{notes}");
    }

    #[test]
    fn only_scratch_ids_and_their_helpers_are_scratch() {
        assert!(is_scratch("shell-scratch-m3x9-ab12cd"));
        assert!(is_scratch("shell-scratch-m3x9-ab12cd/2"));
        assert!(!is_scratch("shell-m3x9-ab12cd"));
        assert!(!is_scratch("shell-m3x9-scratch-"));
        assert!(!is_scratch(""));
        assert_eq!(shell_id(true, "bg-3-17"), "shell-scratch-bg-3-17");
        assert_eq!(shell_id(false, "bg-3-17"), "shell-bg-3-17");
        assert!(is_scratch(&shell_id(true, "consult-17")));
        assert!(!is_scratch(&shell_id(false, "consult-17")));
    }

    #[test]
    fn a_scratch_chat_refuses_memory_and_know_me_but_not_other_files() {
        let id = "shell-scratch-a-b";
        let memory = json!({"path": "notes/MEMORY.md", "content": "x"});
        assert_eq!(refuse_tool(id, "write", &memory), Some(NO_WRITES));
        assert_eq!(
            refuse_tool(
                id,
                "search_replace",
                &json!({"file_path": "/home/user/.config/GrokHub/memory/memory.md"})
            ),
            Some(NO_WRITES)
        );
        assert_eq!(
            refuse_tool(id, "read_file", &json!({"target_file": "MEMORY.md"})),
            Some(NO_READS)
        );
        assert_eq!(refuse_tool(id, "know_me_tags", &json!({})), Some(NO_READS));
        assert_eq!(
            refuse_tool(id, "grokhub-self__know_me_tags", &json!({})),
            Some(NO_READS)
        );
        assert_eq!(
            refuse_tool(id, "write", &json!({"path": "README.md"})),
            None
        );
        assert_eq!(
            refuse_tool(id, "read_file", &json!({"target_file": "a.txt"})),
            None
        );
        assert_eq!(refuse_tool(id, "web_fetch", &memory), None);
        // A normal chat is never refused.
        assert_eq!(refuse_tool("shell-a-b", "write", &memory), None);
        assert_eq!(refuse_tool("shell-a-b", "know_me_tags", &json!({})), None);
    }

    #[test]
    fn a_scratch_chat_searches_past_every_memory_file() {
        let ws = std::env::temp_dir().join(format!(
            "grokhub-scratch-grep-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(ws.join("notes")).unwrap();
        std::fs::write(
            ws.join("MEMORY.md"),
            "The harbor ferry leaves at seven fifteen.\n",
        )
        .unwrap();
        std::fs::write(ws.join("notes/memory.md"), "ferry pass in the drawer\n").unwrap();
        std::fs::write(ws.join("plan.txt"), "take the ferry home\n").unwrap();
        let search = |session: &str, args: Value| {
            let stop = || false;
            let ctx = crate::tools::ToolCtx {
                workspace: &ws,
                desktop: None,
                stop: &stop,
                tasks: None,
                owner: None,
                session: Some(session),
            };
            crate::tools::dispatch(&ctx, "grep", &args.to_string()).text
        };
        let ferry = json!({"pattern": "ferry"});
        let scratch = search("shell-scratch-a-b", ferry.clone());
        assert!(scratch.contains("take the ferry home"), "{scratch}");
        assert!(!scratch.contains("seven fifteen"), "{scratch}");
        assert!(!scratch.contains("drawer"), "{scratch}");
        // Naming the file finds nothing either.
        let named = search(
            "shell-scratch-a-b",
            json!({"pattern": ".", "path": "MEMORY.md"}),
        );
        assert!(!named.contains("seven fifteen"), "{named}");
        // The off-thread read path refuses it too.
        let fanned = crate::tools::execute(
            &ws,
            "shell-scratch-a-b",
            "read_file",
            r#"{"target_file":"MEMORY.md"}"#,
        );
        assert!(fanned.failed, "{}", fanned.text);
        assert_eq!(fanned.text, NO_READS);
        // A normal chat finds all three.
        let normal = search("shell-a-b", ferry);
        assert!(normal.contains("seven fifteen"), "{normal}");
        assert!(normal.contains("drawer"), "{normal}");
        assert!(normal.contains("take the ferry home"), "{normal}");
        let _ = std::fs::remove_dir_all(&ws);
    }
}
