//! Code tab slice 3: the agent drives a Code tab session.
//!
//! - The bundled vstack skills are always in a Code tab session's rules, so
//!   the agent follows them whenever it codes. They can't be switched off and
//!   still don't show on the Skills tab.
//! - The agent's own `git push` goes through the Code tab's push guard
//!   (`grokhub_code::chat_push`, on `ship::guard`): a card every time (a
//!   push is hard class `ship`, so Auto-beta doesn't skip it), and never
//!   main, the default branch or a force-push, in any mode.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use grokhub_agent::code_guard;
use grokhub_agent::code_guard::Decision;
use grokhub_code::chat_push::{self, Verdict};

/// The tools that run a shell command the agent wrote.
const SHELL_TOOLS: &[&str] = &["run_terminal_command", "monitor"];

pub const SKILLS_HEAD: &str = "## Coding skills (always on in the Code tab)";
pub const SKILLS_NOTE: &str = "This chat works in a Code tab project. Follow these built-in skills whenever you code here; they are always loaded and can't be switched off.";
pub const PUSH_NOTE: &str = "Push with `git push origin <branch>` (or `git push origin HEAD`) on its own. The push guard checks every push: never main or the default branch, never a force-push, and a form it can't read is refused.";

/// A skill's text without its `---` front matter.
fn body(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---") else {
        return text.trim();
    };
    match rest.split_once("\n---") {
        Some((_, after)) => after.trim_start_matches(['\r', '\n']).trim(),
        None => text.trim(),
    }
}

/// The block a Code tab session's rules carry: every bundled skill in full.
pub fn skills_block() -> String {
    let mut out = format!("{SKILLS_HEAD}\n\n{SKILLS_NOTE}\n\n{PUSH_NOTE}");
    for (name, text) in grokhub_agent::BUILTIN_SKILL_BODIES {
        out.push_str(&format!("\n\n### {name}\n\n{}", body(text)));
    }
    out
}

/// The shell command in a tool call's arguments.
fn command_of(arguments: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(arguments).ok()?;
    v.get("command")?.as_str().map(str::to_string)
}

/// What the push guard says about one tool call, `None` when it isn't a push.
pub fn check(config_dir: &Path, name: &str, arguments: &str, workspace: &Path) -> Option<Decision> {
    if !SHELL_TOOLS.contains(&name) {
        return None;
    }
    let command = command_of(arguments)?;
    let cfg = grokhub_code::ship::load_from_shell_json(config_dir);
    Some(
        match chat_push::decide(config_dir, workspace, &cfg, &command)? {
            Verdict::Run => Decision::Run,
            Verdict::Ask => Decision::Ask,
            Verdict::Refuse(why) => Decision::Refuse(why),
        },
    )
}

/// Turns the guard on for a Code tab session, or off for any other chat.
pub fn bind(config_dir: &Path, conversation_id: &str, coding: bool) {
    let guard: Option<code_guard::Guard> = coding.then(|| {
        let dir: PathBuf = config_dir.to_path_buf();
        Arc::new(move |name: &str, arguments: &str, workspace: &Path| {
            check(&dir, name, arguments, workspace)
        }) as code_guard::Guard
    });
    code_guard::set(conversation_id, guard);
}

#[cfg(test)]
#[path = "code_agent_tests.rs"]
mod tests;
