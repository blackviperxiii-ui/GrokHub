//! Per-job tool allowlists (`ToolSet`). Filter only; the gate still decides.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::gate::Gate;

use super::schemas_for;

/// Named refusal when a call is outside the job's [`ToolSet`].
pub const TOOL_SET_OUTSIDE: &str = "tool_set_outside";

/// Alias tokens a job brief may use for common tools (`NEED_SHELL`, `TOOLS:shell`).
const SHELL_ALIASES: &[&str] = &["shell", "run_terminal_command"];
const MCP_ALIASES: &[&str] = &["mcp", "search_tool", "use_tool"];

/// Filter applied on top of [`schemas_for`]. Removes tools only; never adds any
/// the gate would hide. [`ToolSet::Full`] leaves the list unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSet {
    /// `None` = unrestricted (full gate list). `Some` = allow those names only.
    allow: Option<BTreeSet<String>>,
    /// When filtering, keep MCP discovery / qualified MCP tools.
    mcp: bool,
}

impl ToolSet {
    /// No filter — same payload as [`schemas_for`].
    pub const FULL: Self = Self {
        allow: None,
        mcp: true,
    };

    /// Whether this set leaves schemas and dispatch unrestricted.
    pub fn is_full(&self) -> bool {
        self.allow.is_none()
    }

    /// Explicit allowlist. MCP tools are excluded unless [`ToolSet::with_mcp`].
    pub fn only(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut allow = BTreeSet::new();
        for name in names {
            insert_alias(&mut allow, name.into());
        }
        Self {
            allow: Some(allow),
            mcp: false,
        }
    }

    /// Background / unattended default: workspace read/write, shell, fetch,
    /// control, session helpers, and self-manage. No desktop, Imagine, or MCP.
    pub fn background() -> Self {
        Self::only(BACKGROUND_TOOLS.iter().copied())
    }

    /// Scheduled / headless default: same lean set as [`ToolSet::background`].
    pub fn unattended() -> Self {
        Self::background()
    }

    /// Allow MCP `search_tool` / `use_tool` / `server__tool` names through the filter.
    pub fn with_mcp(mut self) -> Self {
        self.mcp = true;
        if let Some(allow) = self.allow.as_mut() {
            allow.insert("search_tool".into());
            allow.insert("use_tool".into());
        }
        self
    }

    /// Add one tool (or alias such as `shell`) to an allowlist set.
    pub fn with_tool(mut self, name: &str) -> Self {
        let name = name.trim();
        if name.is_empty() {
            return self;
        }
        match self.allow.as_mut() {
            Some(allow) => insert_alias(allow, name.to_string()),
            None => {
                // Full stays full — declaring a tool on Full is a no-op.
            }
        }
        if MCP_ALIASES.iter().any(|a| a.eq_ignore_ascii_case(name)) {
            self.mcp = true;
        }
        self
    }

    /// True when `name` may be offered and dispatched under this set.
    pub fn allows(&self, name: &str) -> bool {
        let Some(allow) = self.allow.as_ref() else {
            return true;
        };
        if allow.contains(name) {
            return true;
        }
        if name == super::registry::LOAD_TOOL_SCHEMA {
            return true;
        }
        if self.mcp && is_mcp_name(name) {
            return true;
        }
        false
    }

    /// Snapshot of allowlisted names for tests. `None` means unrestricted.
    pub fn names(&self) -> Option<&BTreeSet<String>> {
        self.allow.as_ref()
    }
}

/// Background job kinds used by snapshots and SP2 callers.
pub fn tool_set_for_kind(kind: &str) -> ToolSet {
    match kind.trim().to_ascii_lowercase().as_str() {
        "background" | "user" | "agent" | "detached" => ToolSet::background(),
        "unattended" | "scheduled" => ToolSet::unattended(),
        "chat" | "review" | "" => ToolSet::FULL,
        other => ToolSet::only(std::iter::once(other.to_string())),
    }
}

/// Filter [`schemas_for`] by `set`. Order is preserved (stable prefix stays stable).
pub fn schemas_for_job(gate: &Gate, set: &ToolSet) -> Vec<Value> {
    let all = schemas_for(gate);
    if set.is_full() {
        return all;
    }
    all.into_iter()
        .filter(|tool| {
            let name = tool["name"].as_str().unwrap_or("");
            set.allows(name)
        })
        .collect()
}

/// Refusal text when dispatch blocks a name outside the job set.
pub fn tool_set_refusal(name: &str) -> String {
    format!("{TOOL_SET_OUTSIDE}: `{name}` is not in this job's tool set.")
}

fn is_mcp_name(name: &str) -> bool {
    name == "search_tool" || name == "use_tool" || name.contains("__")
}

fn insert_alias(allow: &mut BTreeSet<String>, raw: String) {
    let name = raw.trim();
    if name.is_empty() {
        return;
    }
    if SHELL_ALIASES.iter().any(|a| a.eq_ignore_ascii_case(name)) {
        allow.insert("run_terminal_command".into());
        allow.insert("monitor".into());
        allow.insert("kill_command_or_subagent".into());
        allow.insert("get_command_or_subagent_output".into());
        return;
    }
    if MCP_ALIASES.iter().any(|a| a.eq_ignore_ascii_case(name)) {
        allow.insert("search_tool".into());
        allow.insert("use_tool".into());
        return;
    }
    allow.insert(name.to_string());
}

/// Lean native set for background and unattended jobs (no desktop / Imagine / MCP /
/// plan-mode / mutate-scheduler / full self-manage write surface). Jobs opt back in
/// with `TOOLS:` / `NEED_SHELL` / `NEED_MCP` via [`crate::prompt::tool_set_from_job`].
const BACKGROUND_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "grep",
    "glob",
    "get_command_or_subagent_output",
    "scheduler_list",
    "skill",
    "diagnose",
    "todo_write",
    "report_findings",
    "spawn_subagent",
    "write",
    "search_replace",
    "run_terminal_command",
    "web_fetch",
    "web_search",
    "kill_command_or_subagent",
    "monitor",
    // Self-manage reads only; writes stay behind TOOLS: declarations.
    "skill_list",
    "connection_list",
    "automation_list",
    "know_me_tags",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{Gate, PermMode};
    use crate::tools::{dispatch_for_job, ToolCtx, ToolOutput};
    use serde_json::json;

    fn chat_gate() -> Gate {
        Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: false,
        }
    }

    fn names(tools: &[Value]) -> Vec<&str> {
        tools.iter().filter_map(|t| t["name"].as_str()).collect()
    }

    #[test]
    fn full_matches_schemas_for() {
        let gate = chat_gate();
        let full = schemas_for(&gate);
        let via = schemas_for_job(&gate, &ToolSet::FULL);
        assert_eq!(names(&full), names(&via));
    }

    #[test]
    fn background_is_subset_of_chat_and_at_least_30_percent_smaller() {
        // Full tool list and per-turn system rewrite: the mode this test checks.
        let _legacy = crate::tools::registry::LegacyPromptGuard::set();
        let gate = chat_gate();
        let chat = schemas_for_job(&gate, &ToolSet::FULL);
        let bg = schemas_for_job(&gate, &ToolSet::background());
        let chat_names: BTreeSet<_> = names(&chat).into_iter().collect();
        for name in names(&bg) {
            assert!(
                chat_names.contains(name),
                "background tool `{name}` missing from chat"
            );
        }
        let chat_bytes = serde_json::to_vec(&chat).unwrap().len();
        let bg_bytes = serde_json::to_vec(&bg).unwrap().len();
        assert!(
            bg_bytes * 10 <= chat_bytes * 7,
            "background payload should be ≥30% smaller: chat={chat_bytes} bg={bg_bytes}"
        );
        assert!(
            !names(&bg).contains(&"image_generate"),
            "Imagine stays out of the background set"
        );
        assert!(
            !names(&bg).iter().any(|n| n.contains("__")),
            "MCP qualified tools stay out unless declared"
        );
    }

    #[test]
    fn with_lazy_tools_background_stays_a_smaller_subset_of_chat() {
        crate::tools::registry::set_enabled_for_tests(Some(true));
        let gate = chat_gate();
        let chat = schemas_for_job(&gate, &ToolSet::FULL);
        let bg = schemas_for_job(&gate, &ToolSet::background());
        crate::tools::registry::set_enabled_for_tests(None);
        let chat_names: BTreeSet<_> = names(&chat).into_iter().collect();
        for name in names(&bg) {
            assert!(
                chat_names.contains(name),
                "background `{name}` missing from chat"
            );
        }
        let chat_bytes = serde_json::to_vec(&chat).unwrap().len();
        let bg_bytes = serde_json::to_vec(&bg).unwrap().len();
        assert!(bg_bytes < chat_bytes, "chat={chat_bytes} bg={bg_bytes}");
    }

    #[test]
    fn unattended_kind_matches_background_subset() {
        let gate = chat_gate();
        let chat = schemas_for_job(&gate, &ToolSet::FULL);
        for kind in ["background", "user", "agent", "unattended", "scheduled"] {
            let set = tool_set_for_kind(kind);
            let job = schemas_for_job(&gate, &set);
            let chat_names: BTreeSet<_> = names(&chat).into_iter().collect();
            for name in names(&job) {
                assert!(chat_names.contains(name), "{kind}: `{name}` not in chat");
            }
            assert!(
                names(&job).contains(&"run_terminal_command"),
                "{kind} keeps shell"
            );
        }
    }

    #[test]
    fn job_declaring_shell_keeps_it_and_outside_is_refused() {
        let gate = chat_gate();
        let set = ToolSet::only(["read_file"]).with_tool("shell");
        let tools = schemas_for_job(&gate, &set);
        let n = names(&tools);
        assert!(n.contains(&"run_terminal_command"), "{n:?}");
        assert!(n.contains(&"read_file"), "{n:?}");
        assert!(!n.contains(&"write"), "{n:?}");
        assert!(!set.allows("write"));
        assert!(set.allows("run_terminal_command"));
        assert_eq!(
            tool_set_refusal("write"),
            format!("{TOOL_SET_OUTSIDE}: `write` is not in this job's tool set.")
        );
    }

    #[test]
    fn undeclared_job_gets_full_list() {
        let gate = chat_gate();
        let set = crate::prompt::tool_set_from_job("plain weekly report");
        assert!(set.is_full());
        assert_eq!(
            names(&schemas_for_job(&gate, &set)),
            names(&schemas_for(&gate))
        );
    }

    #[test]
    fn dispatch_refuses_outside_set_with_named_error() {
        let dir = crate::harness::test_dir("tool-set-dispatch");
        let stop = || false;
        let set = ToolSet::only(["read_file"]);
        let ctx = ToolCtx {
            workspace: &dir,
            desktop: None,
            stop: &stop,
            tasks: None,
            owner: None,
            session: None,
        };
        let out = dispatch_for_job(
            &ctx,
            "write",
            r#"{"path":"x.txt","content":"no"}"#,
            Some(&set),
        );
        assert!(out.failed);
        assert!(out.text.contains(TOOL_SET_OUTSIDE), "{}", out.text);
        let ok = dispatch_for_job(
            &ctx,
            "read_file",
            r#"{"target_file":"missing.txt"}"#,
            Some(&set),
        );
        // May fail on missing file, but must not be tool_set_outside.
        assert!(!ok.text.contains(TOOL_SET_OUTSIDE), "{}", ok.text);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = ok;
    }

    #[test]
    fn tool_set_outside_constant_is_stable() {
        let _: ToolOutput = ToolOutput::err(tool_set_refusal("x"));
        assert_eq!(TOOL_SET_OUTSIDE, "tool_set_outside");
        let _ = json!({"ok": true});
    }
}
