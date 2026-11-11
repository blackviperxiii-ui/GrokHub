//! The slash commands that act on a saved chat or on the project list:
//! `/rename`, `/pin`, `/delete`, `/sessions`, `/context`, `/usage`, `/rewind`
//! and `/project`. `slash.rs` reads the line; this runs it. The destructive
//! ones answer with a question first and change nothing until the same line
//! comes back with `yes`.

use std::path::Path;

use grokhub_agent::perm::ConfigGuard;

use crate::slash::{Effect, Op};
use crate::{history, projects};

/// The cabin's words for `/rewind` with nothing after it.
pub const REWIND_NA: &str = "N/A on native threads: the session is append-only.";
const NO_CHAT: &str = "Send a message first. This chat isn't saved yet.";

/// What a command needs to know about where it was typed.
pub struct Ctx<'a> {
    pub config_dir: &'a Path,
    /// The chat the line was typed in, when the page says which.
    pub conversation_id: Option<&'a str>,
    /// The folder the chat works in now.
    pub bound: &'a str,
    /// Which chats a paired browser may see; `None` on this computer.
    pub owned: Option<&'a dyn Fn(&str) -> bool>,
}

/// An effect, and the folder the chat works in afterwards when it changed
/// (`Some("")` is unbound).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub effect: Effect,
    pub workspace: Option<String>,
}

fn notice(text: impl Into<String>) -> Result<Applied, String> {
    Ok(Applied {
        effect: Effect::Notice { text: text.into() },
        workspace: None,
    })
}

fn confirm(prompt: String, yes: &str) -> Result<Applied, String> {
    Ok(Applied {
        effect: Effect::Confirm {
            prompt,
            yes: yes.into(),
        },
        workspace: None,
    })
}

fn chat<'a>(cx: &Ctx<'a>) -> Result<&'a str, String> {
    let id = cx.conversation_id.ok_or(NO_CHAT)?;
    if cx.owned.is_some_and(|owned| !owned(id)) {
        return Err(crate::remote::NOT_YOURS.into());
    }
    Ok(id)
}

fn saved(cx: &Ctx, id: &str) -> Result<grokhub_agent::SessionInfo, String> {
    if !id.starts_with(history::PREFIX) {
        return Err(history::NOT_SHELL.into());
    }
    let _config = ConfigGuard::set(cx.config_dir);
    grokhub_agent::load_session(id).map_err(|_| NO_CHAT.to_string())
}

pub fn run(op: Op, cx: &Ctx) -> Result<Applied, String> {
    match op {
        Op::Rename(title) => {
            let id = chat(cx)?;
            saved(cx, id)?;
            history::rename(cx.config_dir, id, &title)?;
            notice(format!("Renamed to {}", title.trim()))
        }
        Op::Pin => {
            let id = chat(cx)?;
            let info = saved(cx, id)?;
            let pinned = history::toggle_pin(cx.config_dir, id)?;
            notice(format!(
                "{} {}",
                if pinned { "Pinned" } else { "Unpinned" },
                info.title
            ))
        }
        Op::Delete { yes } => {
            let id = chat(cx)?;
            let info = saved(cx, id)?;
            if !yes {
                return confirm(
                    format!(
                        "Delete {}? The chat and its files are removed for good.",
                        info.title
                    ),
                    "/delete yes",
                );
            }
            history::delete(cx.config_dir, id)?;
            Ok(Applied {
                effect: Effect::Deleted {
                    id: id.to_string(),
                    title: info.title,
                },
                workspace: None,
            })
        }
        Op::Sessions => match cx.owned {
            None => Ok(Applied {
                effect: Effect::Open { view: "history" },
                workspace: None,
            }),
            Some(owned) => {
                let rows: Vec<String> = history::list(cx.config_dir)
                    .into_iter()
                    .filter(|r| owned(&r.id))
                    .take(10)
                    .map(|r| format!("{}{}", if r.pinned { "📌 " } else { "" }, r.title))
                    .collect();
                notice(if rows.is_empty() {
                    "No saved chats from this browser yet.".to_string()
                } else {
                    rows.join("\n")
                })
            }
        },
        Op::Context => {
            let info = saved(cx, chat(cx)?)?;
            let pairs = grokhub_agent::transcript_pairs(&info);
            let turns = grokhub_core::visible_turn_count(&pairs);
            let tokens = grokhub_core::estimate_messages(&pairs);
            notice(format!(
                "{turns} {} · {tokens} tokens · {}%",
                if turns == 1 { "turn" } else { "turns" },
                grokhub_core::context_percent(tokens, grokhub_core::CONTEXT_BUDGET_TOKENS)
            ))
        }
        Op::Usage => {
            let info = saved(cx, chat(cx)?)?;
            let u = &info.usage;
            let line = grokhub_agent::usage_label(
                u.input_tokens,
                u.output_tokens,
                u.reasoning_tokens,
                u.cost_in_usd_ticks,
                &info.meter,
            );
            notice(if line.is_empty() {
                "No usage recorded for this chat yet.".to_string()
            } else {
                line
            })
        }
        Op::RewindNone => notice(REWIND_NA),
        Op::RewindList => {
            let id = chat(cx)?;
            let _config = ConfigGuard::set(cx.config_dir);
            notice(grokhub_agent::format_steps(id))
        }
        Op::RewindStep(n) => {
            let id = chat(cx)?;
            let _config = ConfigGuard::set(cx.config_dir);
            notice(grokhub_agent::restore_step(id, n)?)
        }
        Op::RewindFiles { yes } => {
            let id = chat(cx)?;
            let _config = ConfigGuard::set(cx.config_dir);
            let steps = grokhub_agent::list_steps(id);
            let last = steps
                .last()
                .ok_or("No agent file checkpoints yet. Edit a file in this chat first.")?;
            if !yes {
                return confirm(
                    format!(
                        "Put back {} as they were before step {}? Today's versions are saved first, so /rewind step can undo it.",
                        last.files.join(", "),
                        last.step
                    ),
                    "/rewind files yes",
                );
            }
            notice(grokhub_agent::restore_step(id, last.step)?)
        }
        Op::Project(op) => {
            let done = projects::run(cx.config_dir, cx.bound, op)?;
            if let Some(prompt) = done.ask {
                return confirm(prompt, "/project delete yes");
            }
            Ok(Applied {
                effect: Effect::Notice { text: done.text },
                workspace: done.workspace,
            })
        }
    }
}
