//! Workspace tools. `execute` stays read-only. `dispatch` runs the gated set.

mod connections;
pub(crate) mod control;
mod desktop;
mod glob;
mod grep;
mod html_md;
mod list_dir;
mod lock;
mod media;
pub(crate) mod ports;
mod read_file;
mod search_replace;
pub(crate) mod shell;
mod web_fetch;
mod write;

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

pub use crate::gate::{is_readonly, READ_ONLY_PHASE};

use crate::gate::{self, DeskFlags, Gate};
use crate::tasks::TaskHub;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub text: String,
    pub image_data_url: Option<String>,
    pub failed: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            image_data_url: None,
            failed: false,
        }
    }

    pub fn err(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            image_data_url: None,
            failed: true,
        }
    }
}

pub trait DesktopOps {
    fn halted(&self) -> bool;
    fn locked(&self) -> bool;
    fn call(&self, name: &str, args: &Value) -> ToolOutput;
}

pub struct ToolCtx<'a> {
    pub workspace: &'a Path,
    pub desktop: Option<&'a dyn DesktopOps>,
    pub stop: &'a dyn Fn() -> bool,
    pub tasks: Option<Arc<TaskHub>>,
    /// Subagent that started this call. Its commands are killed with it.
    pub owner: Option<&'a str>,
}

pub fn tool_schemas() -> Vec<Value> {
    vec![
        read_file::schema(),
        list_dir::schema(),
        grep::schema(),
        glob::schema(),
        control::output_schema(),
        control::scheduler_list_schema(),
        crate::skills::schema(),
    ]
}

pub fn schemas_for(gate: &Gate) -> Vec<Value> {
    let mut tools = tool_schemas();
    tools.extend(crate::session_tools::schemas());
    tools.push(crate::subagent::spawn_schema());
    if gate.readonly_session {
        return tools;
    }
    tools.push(crate::subagent::send_schema());
    tools.push(write::schema());
    tools.push(search_replace::schema());
    tools.push(shell::schema());
    tools.push(web_fetch::schema());
    tools.extend(media::schemas());
    tools.push(control::kill_schema());
    tools.push(control::monitor_schema());
    tools.push(control::scheduler_create_schema());
    tools.push(control::scheduler_delete_schema());
    tools.push(connections::add_schema());
    tools.push(connections::disable_schema());
    tools.push(connections::delete_schema());
    if gate.desktop {
        tools.extend(desktop::schemas());
    }
    tools.extend(crate::mcp::schema_tools());
    tools
}

pub fn execute(workspace: &Path, name: &str, arguments: &str) -> ToolOutput {
    if !is_readonly(name) {
        return ToolOutput::err(gate::readonly_refusal(name));
    }
    let args = match parse_args(arguments) {
        Ok(args) => args,
        Err(output) => return output,
    };
    let stop = || false;
    let ctx = ToolCtx {
        workspace,
        desktop: None,
        stop: &stop,
        tasks: None,
        owner: None,
    };
    dispatch_readonly(&ctx, name, &args)
}

pub fn dispatch(ctx: &ToolCtx<'_>, name: &str, arguments: &str) -> ToolOutput {
    let args = match parse_args(arguments) {
        Ok(args) => args,
        Err(output) => return output,
    };
    if is_readonly(name) {
        return dispatch_readonly(ctx, name, &args);
    }
    if let Some(output) = crate::mcp::try_dispatch(name, &args) {
        return output;
    }
    match name {
        "write" => write::run(ctx.workspace, &args),
        "search_replace" => search_replace::run(ctx.workspace, &args),
        "run_terminal_command" => run_shell(ctx, &args),
        "kill_command_or_subagent" => control::kill(ctx.tasks.as_ref(), &args),
        "monitor" => control::monitor(ctx, &args),
        "scheduler_create" => control::scheduler_create(&args),
        "scheduler_delete" => control::scheduler_delete(&args),
        "connection_add" => connections::add(&args),
        "connection_disable" => connections::disable(&args),
        "connection_delete" => connections::delete(&args),
        "web_fetch" => web_fetch::run_with_ports(&args, ctx.stop),
        "image_generate" | "image_edit" | "video_generate" | "video_edit" | "video_extend" => {
            media::run_with_ports(name, &args, ctx.stop)
        }
        "screenshot" | "click" | "move" | "drag" | "scroll" | "type" | "key" => match ctx.desktop {
            Some(desktop) => desktop.call(name, &args),
            None => ToolOutput::err(grokhub_core::desktop_mcp::OFF_MSG),
        },
        other => ToolOutput::err(gate::readonly_refusal(other)),
    }
}

fn run_shell(ctx: &ToolCtx<'_>, args: &Value) -> ToolOutput {
    let background = args
        .get("is_background")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !background {
        return shell::run(ctx.workspace, args, ctx.stop);
    }
    let Some(tasks) = ctx.tasks.as_ref() else {
        return ToolOutput::err("background tasks are not available on this run");
    };
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if command.is_empty() {
        return ToolOutput::err("command is required");
    }
    match tasks.spawn_for(ctx.workspace, command, ctx.owner) {
        Ok(id) => ToolOutput::ok(format!("task id: {id}\nstatus: running")),
        Err(err) => ToolOutput::err(err),
    }
}

pub fn desk_flags(name: &str, gate: &Gate, desktop: Option<&dyn DesktopOps>) -> Option<DeskFlags> {
    if !gate::is_desktop(name) || !gate.desktop {
        return None;
    }
    let desktop = desktop?;
    if desktop.halted() {
        return Some(DeskFlags { halted: true, locked: false });
    }
    Some(DeskFlags {
        halted: false,
        locked: desktop.locked(),
    })
}

fn dispatch_readonly(ctx: &ToolCtx<'_>, name: &str, args: &Value) -> ToolOutput {
    match name {
        "read_file" => read_file::run(ctx.workspace, args),
        "list_dir" => list_dir::run(ctx.workspace, args),
        "grep" => grep::run(ctx.workspace, args),
        "glob" => glob::run(ctx.workspace, args),
        "get_command_or_subagent_output" => control::output(ctx.tasks.as_ref(), args),
        "scheduler_list" => control::scheduler_list(),
        "search_tool" => crate::mcp::search_output(args),
        "skill" => crate::skills::tool_run(ctx.workspace, args),
        other => ToolOutput::err(format!("{READ_ONLY_PHASE}: `{other}` is not available.")),
    }
}

fn parse_args(arguments: &str) -> Result<Value, ToolOutput> {
    if arguments.trim().is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(value) if value.as_object().is_some() => Ok(value),
        Ok(_) => Err(ToolOutput::err("arguments must be a JSON object")),
        Err(err) => Err(ToolOutput::err(format!("arguments are not JSON: {err}"))),
    }
}

/// Lexical `..` and symlink targets that leave the canonical workspace are rejected.
pub fn confine(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let root_c = root
        .canonicalize()
        .map_err(|e| format!("workspace is not available: {e}"))?;
    let raw = raw.trim();
    if raw.is_empty() || raw == "." {
        return Ok(root_c);
    }
    let path = Path::new(raw);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("path escapes the workspace".into());
    }
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root_c.join(path)
    };
    let rel = if joined.starts_with(&root_c) {
        joined.strip_prefix(&root_c).unwrap_or(Path::new("")).to_path_buf()
    } else if path.is_absolute() {
        return Err("path escapes the workspace".into());
    } else {
        path.to_path_buf()
    };
    let mut acc = root_c.clone();
    for comp in rel.components() {
        match comp {
            Component::Normal(name) => {
                acc.push(name);
                match std::fs::symlink_metadata(&acc) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        let target = std::fs::canonicalize(&acc)
                            .map_err(|e| format!("symlink escapes the workspace: {e}"))?;
                        if !target.starts_with(&root_c) {
                            return Err("symlink escapes the workspace".into());
                        }
                        acc = target;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        // A missing final component stays lexical once its parents are inside.
                    }
                }
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                return Err("path escapes the workspace".into());
            }
        }
    }
    if !acc.starts_with(&root_c) {
        return Err("path escapes the workspace".into());
    }
    Ok(acc)
}

/// `image_generate` request body from the existing media builder.
/// The `ImagineApi` is not called.
pub(crate) fn dry_imagine_body(args: &Value) -> Result<Value, String> {
    Ok(media::build_media_call("image_generate", args)?.body)
}

pub(crate) use media::ImagineApi;

/// Install fetch and Imagine clients for this thread. An empty bearer installs
/// adapters that refuse without dialing. The guard restores the previous ports.
/// `session` picks the media folder (`sessions/<id>/media`).
pub(crate) fn install_network(bearer: &str, session: &str) -> ports::Guard {
    let bearer = bearer.trim();
    let media_dir = crate::session::media_dir(session).ok();
    if bearer.is_empty() {
        ports::enter(ports::Ports {
            fetch: Some(Arc::new(web_fetch::BlockedFetch)),
            imagine: Some(Arc::new(media::BlockedImagine)),
            media_dir,
        })
    } else {
        ports::enter(ports::Ports {
            fetch: Some(Arc::new(web_fetch::UreqFetch::new())),
            imagine: Some(Arc::new(media::UreqImagine::new(bearer.to_string()))),
            media_dir,
        })
    }
}

pub fn str_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").trim().to_string()
}

pub fn u32_field(v: &Value, key: &str) -> Option<u32> {
    v.get(key).and_then(|x| x.as_u64()).map(|n| n.min(u64::from(u32::MAX)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schemas_have_object_roots() {
        let tools = tool_schemas();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "read_file",
                "list_dir",
                "grep",
                "glob",
                "get_command_or_subagent_output",
                "scheduler_list",
                "skill",
            ]
        );
        for tool in &tools {
            assert_eq!(tool["type"], "function");
            assert_eq!(tool["parameters"]["type"], "object");
            assert!(tool["name"].as_str().is_some());
        }
    }

    #[test]
    fn write_tool_is_refused_and_dotdot_is_rejected() {
        let dir = std::env::temp_dir().join(format!("gh-tools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = execute(&dir, "write", r#"{"path":"x.txt","content":"no"}"#);
        assert!(out.failed);
        assert!(out.text.contains(READ_ONLY_PHASE));
        assert!(!dir.join("x.txt").exists());
        for name in ["edit", "search_replace", "shell", "bash", "run_terminal_command"] {
            let out = execute(&dir, name, "{}");
            assert!(out.text.contains(READ_ONLY_PHASE), "{name}");
        }
        assert!(confine(&dir, "../outside").is_err());
        assert!(confine(&dir, "/tmp").is_err() || confine(&dir, "/tmp").unwrap().starts_with(dir.canonicalize().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_the_workspace_is_rejected() {
        let dir = std::env::temp_dir().join(format!("gh-link-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("gh-link-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&outside);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("leak")).unwrap();
        assert!(confine(&dir, "leak").is_err());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&outside);
    }
}
