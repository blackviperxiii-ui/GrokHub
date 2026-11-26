//! Workspace tools. `execute` stays read-only. `dispatch` runs the gated set.

pub(crate) mod batch;
pub(crate) mod coding;
pub(crate) mod connections;
pub(crate) mod control;
mod defs;
mod desktop;
pub(crate) mod fence;
mod glob;
mod grep;
pub(crate) mod hangout;
pub(crate) mod home_assistant;
mod html_md;
mod list_dir;
pub(crate) mod local_ai;
mod lock;
pub(crate) mod media;
mod models_check;
pub(crate) mod mods;
pub(crate) mod ports;
pub(crate) mod ptc;
mod read_file;
pub(crate) mod registry;
pub(crate) mod run_routine;
pub(crate) mod save_routine;
mod schema_check;
mod search_replace;
pub(crate) mod seen;
mod session_search;
pub(crate) mod set;
pub(crate) mod shell;
mod show_panel;
pub(crate) mod spool;
pub(crate) mod taint;
pub(crate) mod talk;
mod web_fetch;
pub(crate) mod web_search;
pub(crate) mod workspace_tools;
mod write;

pub use hangout::CANT_DO_HERE;
pub use registry::Settings as DeferredToolsSettings;
#[cfg(test)]
pub(crate) use schema_check::PLAIN_SCHEMA_ERROR;
pub(crate) use schema_check::{schema_message, schema_miss};
pub use set::{schemas_for_job, tool_set_for_kind, tool_set_refusal, ToolSet, TOOL_SET_OUTSIDE};
pub use web_fetch::public_only_resolve;
pub(crate) use web_fetch::{dial_host, parse_public_url};
#[cfg(test)]
pub(crate) use web_fetch::{FetchResp, PageFetch};
pub use web_search::{
    brave_key_present, seal_brave_key, searxng_url_problem, Backend as WebSearchBackend,
    Settings as WebSearchSettings, BACKENDS as WEB_SEARCH_BACKENDS,
};

/// Whether `{config_dir}/deferred_tools.toml` has `enabled = true`.
/// `generative_ui.toml` turns show_panel and the chat's panels on, read the one way.
pub fn generative_ui_enabled(config_dir: &Path) -> bool {
    show_panel::enabled(config_dir)
}

pub fn deferred_tools_enabled(config_dir: &Path) -> bool {
    registry::settings(config_dir).enabled
}

pub fn deferred_tools_settings(config_dir: &Path) -> DeferredToolsSettings {
    registry::settings(config_dir)
}

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

pub use crate::gate::{is_readonly, READ_ONLY_PHASE};

use crate::gate::{self, DeskFlags, Gate};
use crate::tasks::TaskHub;

#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    pub text: String,
    pub image_data_url: Option<String>,
    pub failed: bool,
    /// An MCP App view to host in chat when this MCP tool named a `ui://` resource.
    /// Boxed: the view carries whole HTML pages and would bloat every tool result.
    pub mcp_app: Option<Box<crate::mcp::McpAppView>>,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            image_data_url: None,
            failed: false,
            mcp_app: None,
        }
    }

    pub fn err(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            image_data_url: None,
            failed: true,
            mcp_app: None,
        }
    }
}

pub trait DesktopOps {
    fn halted(&self) -> bool;
    fn locked(&self) -> bool;
    fn call(&self, name: &str, args: &Value) -> ToolOutput;
    /// `args` plus what the hard gate reads beside them: the focused window
    /// and the control under a click, as path A adds. Never sent to the
    /// desktop. The default adds nothing.
    fn hint(&self, _name: &str, args: &Value) -> Value {
        args.clone()
    }
    /// [`LOOK_AT_SCREEN`]: a screenshot that only looks. In a chat it works
    /// with desktop control off, so a wrapper that switches control off must
    /// still pass it there. An unattended run has nobody to ask, so its
    /// wrapper refuses a look once the switch is off.
    fn look(&self, args: &Value) -> ToolOutput {
        self.call("screenshot", args)
    }
}

/// The look-only screenshot tool. It runs in the app, not the shell, so the
/// shell sandbox never stands between the agent and the screen.
pub const LOOK_AT_SCREEN: &str = "look_at_screen";

pub struct ToolCtx<'a> {
    pub workspace: &'a Path,
    pub desktop: Option<&'a dyn DesktopOps>,
    pub stop: &'a dyn Fn() -> bool,
    pub tasks: Option<Arc<TaskHub>>,
    /// Subagent that started this call. Its commands are killed with it.
    pub owner: Option<&'a str>,
    /// Chat session id for deferred tool schema state (R4).
    pub session: Option<&'a str>,
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
        crate::repair::schema(),
    ]
}

pub fn schemas_for(gate: &Gate) -> Vec<Value> {
    registry::schemas_for(gate)
}

/// Bind the chat session for deferred tool schemas on this thread (R4).
pub fn bind_deferred_session(session: &str) {
    registry::bind_session(session);
}

/// Reset loaded deferred schemas for a new user turn.
pub fn begin_deferred_turn(session: &str) {
    registry::begin_turn(session);
}

/// Item 33: count a chat's session in its workspace and freeze the workspace's
/// up-front tool list for it. A chat already started keeps its list.
pub fn start_workspace_session(session: &str, workspace: &Path) {
    // Tests that share a config folder would count each other's sessions.
    if cfg!(test) && crate::perm::pinned_config().is_none() {
        return;
    }
    workspace_tools::start(&crate::perm::config_dir(), session, workspace);
}

/// LO-45: give the chat's Brief to the tool router. The first Brief picks the
/// chat's tool profile; a later work Brief upgrades a Talk chat.
pub fn note_turn_brief(session: &str, brief: &crate::brain::Brief) {
    registry::note_brief(session, brief);
}

/// A read-only tool for `session`'s chat, off the loop's thread.
pub fn execute(workspace: &Path, session: &str, name: &str, arguments: &str) -> ToolOutput {
    if !is_readonly(name) {
        return ToolOutput::err(gate::readonly_refusal(name));
    }
    let args = match parse_args(arguments) {
        Ok(args) => args,
        Err(output) => return output,
    };
    if let Some(refused) = crate::scratch::refuse_tool(session, name, &args) {
        return ToolOutput::err(refused);
    }
    let stop = || false;
    let ctx = ToolCtx {
        workspace,
        desktop: None,
        stop: &stop,
        tasks: None,
        owner: None,
        session: Some(session),
    };
    dispatch_readonly(&ctx, name, &args)
}

pub fn dispatch(ctx: &ToolCtx<'_>, name: &str, arguments: &str) -> ToolOutput {
    dispatch_for_job(ctx, name, arguments, None)
}

/// Like [`dispatch`], but refuses tools outside `set` with [`TOOL_SET_OUTSIDE`].
pub fn dispatch_for_job(
    ctx: &ToolCtx<'_>,
    name: &str,
    arguments: &str,
    set: Option<&ToolSet>,
) -> ToolOutput {
    if let Some(session) = ctx.session {
        registry::bind_session(session);
    }
    // A tool that runs other tools here (a routine's steps) runs them in
    // this run, so they take its origin (`crate::web_origin`).
    let _run = crate::web_origin::RunScope::enter(ctx.session);
    if let Some(set) = set {
        if !set.allows(name) {
            return ToolOutput::err(set::tool_set_refusal(name));
        }
    }
    if let Some(session) = ctx.session {
        workspace_tools::note_use(&crate::perm::config_dir(), session, name);
    }
    let args = match parse_args(arguments) {
        Ok(args) => args,
        Err(output) => return output,
    };
    if let Some(refused) = ctx
        .session
        .and_then(|session| crate::scratch::refuse_tool(session, name, &args))
    {
        return ToolOutput::err(refused);
    }
    if name == registry::TOOL_SEARCH {
        return registry::run_tool_search(&args);
    }
    if name == registry::LOAD_TOOL_SCHEMA {
        return registry::run_load_tool_schema(&args);
    }
    if name == show_panel::SHOW_PANEL {
        return show_panel::run(&args);
    }
    if is_readonly(name) {
        return dispatch_readonly(ctx, name, &args);
    }
    if crate::self_manage::self_tool(name).is_some() {
        // Path E: the loop already asked `harness::decide` (via `decide_with`).
        return crate::self_manage::run_native(name, &args, ctx.session);
    }
    if let Some(output) = crate::mcp::try_dispatch(name, &args, ctx.stop) {
        return output;
    }
    match name {
        "write" => write::run(ctx.workspace, ctx.session, &args),
        "search_replace" => search_replace::run(ctx.workspace, ctx.session, &args),
        "run_terminal_command" => run_shell(ctx, &args),
        "kill_command_or_subagent" => control::kill(ctx.tasks.as_ref(), &args),
        control::MERGE_WORK => control::merge(ctx, &args),
        "monitor" => control::monitor(ctx, &args),
        "scheduler_create" => control::scheduler_create(&args, ctx.session),
        "scheduler_delete" => control::scheduler_delete(&args),
        run_routine::NAME => run_routine::run(ctx, &args),
        save_routine::NAME => save_routine::run(ctx, &args),
        "connection_delete" => connections::delete(&args),
        local_ai::SELFTEST => local_ai::selftest(&args),
        mods::PREPARE => mods::run(name, &args),
        home_assistant::CALL_SERVICE => home_assistant::run(name, &args),
        "web_fetch" => web_fetch::run_with_ports(&args, ctx.workspace, ctx.stop),
        "web_search" => web_search::run_with_ports(&args, ctx.stop),
        "image_generate" | "image_edit" | "video_generate" | "video_edit" | "video_extend" => {
            media::run_with_ports(name, &args, ctx.stop)
        }
        LOOK_AT_SCREEN => match run_desktop(ctx) {
            Some(desktop) => fence_desktop("screenshot", desktop.look(&args)),
            None => ToolOutput::err("There is no screen to look at here."),
        },
        "screenshot" | "click" | "move" | "drag" | "scroll" | "type" | "key" => {
            match run_desktop(ctx) {
                Some(desktop) => fence_desktop(name, desktop.call(name, &args)),
                None => ToolOutput::err(*grokhub_core::desktop_mcp::OFF_MSG),
            }
        }
        other => ToolOutput::err(gate::readonly_refusal(other)),
    }
}

/// The desktop this call may use: none in a run that started from a paired
/// browser's words, whatever desktop it was handed.
fn run_desktop<'a>(ctx: &ToolCtx<'a>) -> Option<&'a dyn DesktopOps> {
    let allowed = || crate::web_origin::desktop_allowed(crate::web_origin::of(ctx.session));
    ctx.desktop.filter(|_| allowed())
}

fn run_shell(ctx: &ToolCtx<'_>, args: &Value) -> ToolOutput {
    let typed = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if let Some(why) = shell::names_a_tool(typed) {
        return ToolOutput::err(why);
    }
    if shell::grabs_screen(typed) {
        return ToolOutput::err(shell::USE_LOOK);
    }
    let background = args
        .get("is_background")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !background {
        // Where background tasks are on, a command that outlives its timeout
        // keeps running as one instead of being killed.
        let Some(tasks) = ctx.tasks.as_ref() else {
            return shell::run_adoptable(ctx.workspace, ctx.session, args, ctx.stop, None);
        };
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut adopt = |proc, guard| tasks.adopt(proc, ctx.workspace, &command, ctx.owner, guard);
        return shell::run_adoptable(ctx.workspace, ctx.session, args, ctx.stop, Some(&mut adopt));
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
    let plan = crate::sandbox::plan_for_call("run_terminal_command", args, ctx.workspace);
    match tasks.spawn_for(ctx.workspace, command, ctx.owner, plan) {
        Ok(id) => ToolOutput::ok(format!("task id: {id}\nstatus: running")),
        Err(err) => ToolOutput::err(err),
    }
}

pub fn desk_flags(name: &str, gate: &Gate, desktop: Option<&dyn DesktopOps>) -> Option<DeskFlags> {
    // A look needs no desktop control, but Halt and a locked screen still hold it.
    let look = name == LOOK_AT_SCREEN;
    if !look && (!gate::is_desktop(name) || !gate.desktop) {
        return None;
    }
    let desktop = desktop?;
    if desktop.halted() {
        return Some(DeskFlags {
            halted: true,
            locked: false,
        });
    }
    Some(DeskFlags {
        halted: false,
        locked: desktop.locked(),
    })
}

fn dispatch_readonly(ctx: &ToolCtx<'_>, name: &str, args: &Value) -> ToolOutput {
    if name == show_panel::SHOW_PANEL {
        return show_panel::run(args);
    }
    match name {
        "read_file" => read_file::run(ctx.workspace, ctx.session, args),
        "list_dir" => list_dir::run(ctx.workspace, args),
        // A scratch chat's search skips every MEMORY.md.
        "grep" if ctx.session.is_some_and(crate::scratch::is_scratch) => {
            grep::run_in(ctx.workspace, args, true)
        }
        "grep" => grep::run(ctx.workspace, args),
        "glob" => glob::run(ctx.workspace, args),
        "get_command_or_subagent_output" => control::output(ctx.tasks.as_ref(), args),
        "scheduler_list" => control::scheduler_list(),
        "search_tool" => crate::mcp::search_output(args),
        registry::TOOL_SEARCH => registry::run_tool_search(args),
        registry::LOAD_TOOL_SCHEMA => registry::run_load_tool_schema(args),
        "skill" => crate::skills::tool_run(ctx.workspace, args),
        "diagnose" => crate::repair::tool_run(args),
        local_ai::STATUS => local_ai::status(args),
        models_check::NAME => models_check::run(args),
        session_search::NAME => session_search::run(ctx.session, args),
        ptc::NAME => ptc::run(ctx, args),
        mods::SEARCH | mods::DETAIL | mods::INSTALLED => mods::run(name, args),
        home_assistant::LIST_ENTITIES
        | home_assistant::GET_STATE
        | home_assistant::LIST_SERVICES => home_assistant::run(name, args),
        other => ToolOutput::err(format!("{READ_ONLY_PHASE}: `{other}` is not available.")),
    }
}

/// Screenshot (and other desktop reads that return screen / AX text) are
/// outside data. Short click/type acks stay plain.
fn fence_desktop(name: &str, mut out: ToolOutput) -> ToolOutput {
    if out.failed || out.text.is_empty() {
        return out;
    }
    if name == "screenshot" || out.text.len() > 80 {
        out.text = fence::fence("desktop", &out.text);
    }
    out
}

fn parse_args(arguments: &str) -> Result<Value, ToolOutput> {
    if arguments.trim().is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(value) if value.as_object().is_some() => Ok(value),
        Ok(_) => Err(ToolOutput::err(
            "arguments must be a JSON object: send the call again with them as one object",
        )),
        Err(err) => Err(ToolOutput::err(format!(
            "arguments are not JSON ({err}): send the call again with valid JSON"
        ))),
    }
}

/// Whether a call's arguments parse the way [`parse_args`] takes them.
pub fn args_parse(arguments: &str) -> bool {
    parse_args(arguments).is_ok()
}

/// File and shell tools. A chat with no folder cannot run them.
pub fn needs_workspace(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "list_dir"
            | "grep"
            | "glob"
            | "write"
            | "search_replace"
            | "run_terminal_command"
            | "run_tool_script"
    )
}

/// No folder was picked, or the path is not a directory on disk.
pub fn workspace_missing(path: &Path) -> bool {
    path.as_os_str().is_empty() || !path.is_dir()
}

/// Clear, actionable error when the workspace folder is missing or unreadable.
pub(crate) fn workspace_unavailable_error(workspace: &Path, err: &std::io::Error) -> String {
    let raw = workspace.to_string_lossy();
    let windows =
        cfg!(windows) || raw.starts_with(r"\\?\") || (raw.len() >= 2 && raw.as_bytes()[1] == b':');
    let shown = grokhub_core::canonical_session_cwd(&raw, windows);
    if shown.is_empty() {
        format!(
            "workspace is not available ({err}): no workspace folder is set. Pick a folder in Chat or Settings → Workspace, then try again."
        )
    } else {
        format!(
            "workspace is not available at `{shown}` ({err}). Create that folder or pick an existing folder in Chat or Settings → Workspace, then try again."
        )
    }
}

/// Clear, actionable message when a top-level chat tool call tries to reach
/// outside the active workspace. Preserves the `"path escapes the workspace"`
/// prefix while telling the user and model what to do.
pub fn path_escape_guidance(workspace: &Path, requested: &str) -> String {
    let raw = workspace.to_string_lossy();
    let windows =
        cfg!(windows) || raw.starts_with(r"\\?\") || (raw.len() >= 2 && raw.as_bytes()[1] == b':');
    let ws = grokhub_core::canonical_session_cwd(&raw, windows);
    let target = requested.trim();
    if target.is_empty() {
        format!(
            "path escapes the workspace (`{ws}`). File tools can only access paths inside this workspace; use a relative path inside `{ws}`, or pick a different folder in Chat or Settings → Workspace."
        )
    } else {
        format!(
            "path escapes the workspace: `{target}` is outside `{ws}`. Use a path inside `{ws}`, or pick the folder containing `{target}` in Chat or Settings → Workspace."
        )
    }
}

/// Lexical `..` and symlink targets that leave the canonical workspace are rejected.
pub fn confine(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let root_c = root
        .canonicalize()
        .map_err(|e| workspace_unavailable_error(root, &e))?;
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
        joined
            .strip_prefix(&root_c)
            .unwrap_or(Path::new(""))
            .to_path_buf()
    } else if path.is_absolute() {
        // One folder has many spellings: `C:\x` and `\\?\C:\x` or an 8.3
        // short name on Windows, `/var` for `/private/var` on macOS, a link
        // anywhere. Compare where the part of the path that exists really is.
        canonical_prefix(path)
            .as_deref()
            .and_then(|real| real.strip_prefix(&root_c).ok())
            .ok_or("path escapes the workspace")?
            .to_path_buf()
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

/// `path` with its longest existing ancestor canonicalized and the rest as
/// written. `None` when no ancestor exists.
fn canonical_prefix(path: &Path) -> Option<PathBuf> {
    let mut rest = Vec::new();
    let mut at = path;
    loop {
        if let Ok(real) = at.canonicalize() {
            return Some(rest.iter().rev().fold(real, |acc, name| acc.join(name)));
        }
        rest.push(at.file_name()?);
        at = at.parent()?;
    }
}

/// The consent ledger or the routine consent store, which only the
/// person's click writes. The write and edit tools refuse them however the
/// path is spelled, through a symlinked folder too; a file of the same name
/// in a project is fine.
pub(crate) fn person_only_file(config_dir: &Path, path: &Path) -> Option<String> {
    let real = resolved(path)?;
    let stores = [
        crate::harness::consent_path(config_dir),
        crate::routine_consent::path(config_dir),
    ];
    stores
        .iter()
        .any(|s| resolved(s).as_ref() == Some(&real) || same_file(s, path))
        .then(|| "only your click changes the consent files".to_string())
}

/// Two names for one file, as a hard link makes.
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(_: &Path, _: &Path) -> bool {
    false
}

/// `path` with its folder's symlinks resolved, in lowercase for file
/// systems that ignore case. The file itself may not exist yet.
fn resolved(path: &Path) -> Option<String> {
    let folder = path.parent()?.canonicalize().ok()?;
    Some(
        folder
            .join(path.file_name()?)
            .to_string_lossy()
            .to_lowercase(),
    )
}

/// A native tool's full schema by name, whatever the gate or the deferred
/// index say: the caller decides whether it is offered.
pub(crate) fn native_schema(name: &str) -> Option<Value> {
    defs::find_def(name).map(|d| (d.schema)())
}

/// `image_generate` request body from the existing media builder.
/// The `ImagineApi` is not called.
pub(crate) fn dry_imagine_body(args: &Value) -> Result<Value, String> {
    Ok(media::build_media_call("image_generate", args)?.body)
}

pub use media::{imagine_client, run_media_tool, ImagineApi, ImagineSource};
pub use show_panel::SHOW_PANEL;

/// `web_fetch` and `web_search` outside a turn that installed the network:
/// neither needs a sign-in, so the note names none.
pub(crate) const NO_WEB_PORT: &str = "Web access isn't set up for this run.";

/// Install fetch and Imagine clients for this thread. `web_fetch` and
/// `web_search` never send the bearer, so they work without one; each hop still
/// passes the permission gate, the public-only resolver and the egress guard.
/// `None` leaves Imagine unset, so a picture falls back to the on-device model
/// or asks to sign in. A source gets a fresh lazy client for this prompt: its
/// bearer is read on the first media call, never before. The guard restores
/// the previous ports. `session` picks the media folder (`sessions/<id>/media`).
pub(crate) fn install_network(
    imagine: Option<&Arc<dyn ImagineSource>>,
    session: &str,
) -> ports::Guard {
    let imagine = imagine.map(|source| {
        Arc::new(media::LazyImagine::new(
            Arc::clone(source),
            Box::new(media::dial_xai),
        )) as Arc<dyn ImagineApi>
    });
    ports::enter(ports::Ports {
        fetch: Some(Arc::new(web_fetch::UreqFetch::new())),
        imagine,
        media_dir: crate::session::media_dir(session).ok(),
    })
}

pub fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Path-touching tools that may surface nested folder rules.
pub(crate) fn is_path_tool(name: &str) -> bool {
    matches!(
        name,
        "read_file" | "list_dir" | "grep" | "search_replace" | "write"
    )
}

/// Raw path argument for a path tool, if any.
///
/// `list_dir` / `grep` with an empty path mean the workspace root (`"."`).
pub(crate) fn tool_path_arg(name: &str, args: &Value) -> Option<String> {
    let keys: &[&str] = match name {
        "read_file" => &["target_file", "path", "file_path"],
        "list_dir" | "grep" => &["path"],
        "search_replace" => &["file_path", "path", "target_file"],
        "write" => &["path", "file_path", "target_file"],
        _ => return None,
    };
    for key in keys {
        let raw = str_field(args, key);
        if !raw.is_empty() {
            return Some(raw);
        }
    }
    if matches!(name, "list_dir" | "grep") {
        return Some(".".into());
    }
    None
}

pub fn u32_field(v: &Value, key: &str) -> Option<u32> {
    v.get(key)
        .and_then(|x| x.as_u64())
        .map(|n| n.min(u64::from(u32::MAX)) as u32)
}

#[cfg(test)]
mod arg_roundtrip_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use web_fetch::PageFetch;

    #[test]
    fn web_fetch_works_without_a_bearer_and_imagine_waits_for_one() {
        let tmp = std::env::temp_dir().join(format!(
            "grokhub-install-network-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _cfg = crate::perm::ConfigGuard::set(&tmp);
        let _offline = grokhub_core::offline::force_for_thread(false);
        let one = Ok(vec![IpAddr::from([1, 1, 1, 1])]);
        {
            let _g = install_network(None, "s");
            let p = ports::current();
            assert_eq!(p.fetch.expect("fetch").resolve("1.1.1.1", 443), one);
            assert!(p.imagine.is_none(), "no Imagine without a sign-in");
        }
        let src = Arc::new(media::tests::FakeSource::signed_in());
        let source: Arc<dyn ImagineSource> = src.clone();
        let _g = install_network(Some(&source), "s");
        let p = ports::current();
        assert_eq!(p.fetch.expect("fetch").resolve("1.1.1.1", 443), one);
        assert!(p.imagine.is_some());
        assert!(src.calls().is_empty(), "installing reads no sign-in");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn offline_only_refuses_before_any_dns_lookup() {
        let _offline = grokhub_core::offline::force_for_thread(true);
        let fetch = web_fetch::UreqFetch::new();
        assert_eq!(
            fetch.resolve("example.com", 443),
            Err(grokhub_core::offline::OFFLINE_MSG.to_string())
        );
        assert_eq!(
            fetch.resolve("127.0.0.1", 443),
            Ok(vec![IpAddr::from([127, 0, 0, 1])]),
            "this computer still resolves"
        );
    }

    #[test]
    fn tool_path_arg_reads_each_path_tool() {
        assert_eq!(
            tool_path_arg("read_file", &json!({"target_file": "sub/a.rs"})).as_deref(),
            Some("sub/a.rs")
        );
        assert_eq!(
            tool_path_arg("list_dir", &json!({"path": "sub"})).as_deref(),
            Some("sub")
        );
        assert_eq!(tool_path_arg("list_dir", &json!({})).as_deref(), Some("."));
        assert_eq!(
            tool_path_arg("grep", &json!({"pattern": "x", "path": "crates"})).as_deref(),
            Some("crates")
        );
        assert_eq!(
            tool_path_arg(
                "search_replace",
                &json!({"file_path": "a.rs", "old_string": "a", "new_string": "b"})
            )
            .as_deref(),
            Some("a.rs")
        );
        assert_eq!(
            tool_path_arg("write", &json!({"path": "b.rs", "content": ""})).as_deref(),
            Some("b.rs")
        );
        assert!(tool_path_arg("shell", &json!({"command": "ls"})).is_none());
        assert!(is_path_tool("read_file"));
        assert!(!is_path_tool("shell"));
    }

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
                "diagnose",
            ]
        );
        for tool in &tools {
            assert_eq!(tool["type"], "function");
            assert_eq!(tool["parameters"]["type"], "object");
            assert!(tool["name"].as_str().is_some());
        }
    }

    /// The API answers 400 "Duplicate function definition provided" when two
    /// tools share a name, so chat fails before the first token.
    #[test]
    fn schemas_for_has_no_duplicate_names() {
        // Full tool list: the mode this test checks.
        let _legacy = crate::tools::registry::LegacyPromptGuard::set();
        for readonly_session in [false, true] {
            for desktop in [false, true] {
                let gate = Gate {
                    mode: gate::PermMode::Always,
                    readonly_session,
                    attended: true,
                    desktop,
                };
                let tools = schemas_for(&gate);
                let mut seen = std::collections::BTreeSet::new();
                for tool in &tools {
                    let name = tool["name"].as_str().unwrap_or_default();
                    assert!(
                        seen.insert(name),
                        "duplicate tool {name} (readonly={readonly_session}, desktop={desktop})"
                    );
                }
            }
        }
        let full = schemas_for(&Gate {
            mode: gate::PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: false,
        });
        let count = |n: &str| full.iter().filter(|t| t["name"] == n).count();
        assert_eq!(count("connection_add"), 1);
        assert_eq!(count("connection_disable"), 1);
        assert_eq!(count("connection_delete"), 1);
    }

    #[test]
    fn write_tool_is_refused_and_dotdot_is_rejected() {
        let dir = std::env::temp_dir().join(format!("gh-tools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = execute(
            &dir,
            "shell-a-b",
            "write",
            r#"{"path":"x.txt","content":"no"}"#,
        );
        assert!(out.failed);
        assert!(out.text.contains(READ_ONLY_PHASE));
        assert!(!dir.join("x.txt").exists());
        for name in [
            "edit",
            "search_replace",
            "shell",
            "bash",
            "run_terminal_command",
        ] {
            let out = execute(&dir, "shell-a-b", name, "{}");
            assert!(out.text.contains(READ_ONLY_PHASE), "{name}");
        }
        assert!(confine(&dir, "../outside").is_err());
        assert!(
            confine(&dir, "/tmp").is_err()
                || confine(&dir, "/tmp")
                    .unwrap()
                    .starts_with(dir.canonicalize().unwrap())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn an_absolute_path_into_the_workspace_by_another_spelling_is_inside() {
        // A link to the workspace stands in for Windows' `\\?\` and 8.3
        // spellings and macOS's `/private/var`.
        let base = std::env::temp_dir().join(format!("gh-alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let ws = base.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("notes.txt"), "alpha").unwrap();
        std::fs::write(base.join("outside.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(&ws, base.join("alias")).unwrap();
        let real = ws.canonicalize().unwrap();
        let at = |rel: &str| confine(&ws, &base.join(rel).to_string_lossy());
        assert_eq!(at("alias/notes.txt"), Ok(real.join("notes.txt")));
        assert_eq!(at("alias/new/file.txt"), Ok(real.join("new/file.txt")));
        assert_eq!(at("outside.txt"), Err("path escapes the workspace".into()));
        let _ = std::fs::remove_dir_all(&base);
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

    #[cfg(unix)]
    #[test]
    fn the_write_tools_never_reach_the_consent_files_however_the_path_reads() {
        let home = std::env::temp_dir().join(format!("person-only-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let config = home.join(".config/GrokHub");
        std::fs::create_dir_all(config.join("routines")).unwrap();
        std::fs::create_dir_all(home.join("src/routines")).unwrap();
        std::os::unix::fs::symlink(config.join("routines"), home.join("rr")).unwrap();
        let refused = |rel: &str| person_only_file(&config, &confine(&home, rel).unwrap());
        let why = Some("only your click changes the consent files".to_string());
        assert_eq!(refused(".config/GrokHub/routines/consent.json"), why);
        assert_eq!(refused(".config/GrokHub/routines/Consent.JSON"), why);
        assert_eq!(refused(".config/GrokHub/consent.jsonl"), why);
        // Through a symlink to the routines folder.
        assert_eq!(refused("rr/consent.json"), why);
        // Through a hard link to the store.
        std::fs::write(config.join("routines/consent.json"), "{}").unwrap();
        std::fs::hard_link(
            config.join("routines/consent.json"),
            home.join("notes.json"),
        )
        .unwrap();
        assert_eq!(refused("notes.json"), why);
        // A project's own file of the same name.
        assert_eq!(refused("src/routines/consent.json"), None);
        assert_eq!(refused(".config/GrokHub/routines/note/routine.json"), None);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Records each desktop call it is handed.
    #[derive(Default)]
    struct SpyDesk(std::sync::Mutex<Vec<String>>);

    impl DesktopOps for SpyDesk {
        fn halted(&self) -> bool {
            false
        }
        fn locked(&self) -> bool {
            false
        }
        fn call(&self, name: &str, _args: &Value) -> ToolOutput {
            self.0.lock().unwrap().push(name.to_string());
            ToolOutput::ok("done")
        }
    }

    #[test]
    fn a_browser_run_gets_no_desktop_it_was_handed() {
        use crate::web_origin::{note_chat, start_run, RunOrigin, RunScope};
        let dir = std::env::temp_dir().join(format!(
            "grokhub-run-desktop-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let _config = crate::perm::ConfigGuard::set(&dir);
        // A chat a browser started, and a run of a job a browser wrote.
        note_chat(&dir, "shell-web-desk-1").unwrap();
        start_run(&dir, "shell-auto-webjob-5-1", RunOrigin::Browser).unwrap();
        let desk = SpyDesk::default();
        let stop = || false;
        let steps = |session: Option<&str>| -> Vec<String> {
            let ctx = ToolCtx {
                workspace: &dir,
                desktop: Some(&desk),
                stop: &stop,
                tasks: None,
                owner: None,
                session,
            };
            ["click", "look_at_screen"]
                .iter()
                .map(|name| dispatch(&ctx, name, r#"{"x":1,"y":2}"#).text)
                .collect()
        };
        let none = vec![
            grokhub_core::desktop_mcp::OFF_MSG.to_string(),
            "There is no screen to look at here.".to_string(),
        ];
        // A turn typed at this computer in a browser's chat, its helper, the
        // job's run, and a step a browser run's routine runs on its thread.
        assert_eq!(steps(Some("shell-web-desk-1")), none);
        assert_eq!(steps(Some("shell-web-desk-1/helper-1")), none);
        assert_eq!(steps(Some("shell-auto-webjob-5-1")), none);
        {
            let _run = RunScope::enter(Some("shell-auto-webjob-5-1"));
            assert_eq!(steps(None), none);
        }
        assert!(desk.0.lock().unwrap().is_empty());
        // A chat started here keeps the desktop it was handed; a look comes
        // back fenced as the screen's words.
        for session in [Some("shell-desk-9"), None] {
            let got = steps(session);
            assert_eq!(got[0], "done");
            let look: Vec<&str> = got[1].lines().collect();
            assert_eq!(look.len(), 3, "{}", got[1]);
            assert!(look[0].starts_with("<<<UNTRUSTED source=desktop nonce="));
            assert_eq!(look[1], "done");
        }
        assert_eq!(
            *desk.0.lock().unwrap(),
            vec!["click", "screenshot", "click", "screenshot"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A routine the person saved and allowed here, run from a browser's
    /// run, clicks and types nothing: its desktop steps get no desktop
    /// either. Run from a chat started here, the same steps reach it.
    #[test]
    fn a_browser_runs_routine_gets_no_desktop_it_was_handed() {
        use crate::web_origin::{note_chat, RunOrigin, RunScope};
        let dir = std::env::temp_dir().join(format!(
            "grokhub-routine-desktop-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let _config = crate::perm::ConfigGuard::set(&dir);
        crate::harness::use_key_store_for(
            &dir,
            std::sync::Arc::new(crate::harness::MemoryKeyStore::new()),
        );
        let rdir = grokhub_core::routine::routines_dir(&dir).join("open-notes");
        std::fs::create_dir_all(&rdir).unwrap();
        let saved = serde_json::json!({
            "version": 1, "name": "open-notes", "about": "Opens my notes", "triggers": ["chat"],
            "steps": [{ "id": "open", "desktop": { "screen": null, "ops": [
                { "Click": { "x": 10, "y": 20 } }, { "Type": { "text": "hello" } }
            ] } }]
        });
        std::fs::write(rdir.join(grokhub_core::routine::FILE), saved.to_string()).unwrap();
        let routine =
            grokhub_core::routine::Routine::load(&rdir, &crate::routine_run::step_tool).unwrap();
        crate::routine_consent::grant(&dir, &routine, 1).unwrap();
        note_chat(&dir, "shell-web-rt-1").unwrap();
        let desk = SpyDesk::default();
        let stop = || false;
        let ran = |session: Option<&str>| -> (bool, String) {
            let ctx = ToolCtx {
                workspace: &dir,
                desktop: Some(&desk),
                stop: &stop,
                tasks: None,
                owner: None,
                session,
            };
            let out = dispatch(&ctx, run_routine::NAME, r#"{"name":"open-notes"}"#);
            let said: Vec<&str> = out.text.lines().collect();
            (out.failed, said[1..said.len() - 1].join("\n"))
        };
        let refused = (
            true,
            "Stopped at open: desktop steps need the app's desktop control".to_string(),
        );
        assert_eq!(ran(Some("shell-web-rt-1")), refused);
        assert_eq!(ran(Some("shell-web-rt-1/helper-1")), refused);
        {
            let _run = RunScope::enter(Some("shell-web-rt-1"));
            assert_eq!(ran(None), refused);
        }
        assert!(desk.0.lock().unwrap().is_empty());
        assert_eq!(
            crate::web_origin::of_in(&dir, Some("shell-web-rt-1")),
            RunOrigin::Browser
        );
        assert_eq!(
            ran(Some("shell-desk-rt-1")),
            (false, "open: done\ndone\n\nFinished.".to_string())
        );
        assert_eq!(*desk.0.lock().unwrap(), vec!["click", "type"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_errors_and_default_workspace_read_write_are_clear() {
        let root = std::env::temp_dir().join(format!("gh-ws-clear-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let home_str = root.to_string_lossy().into_owned();

        // Missing workspace produces a clear actionable message naming the folder.
        let missing = root.join("does-not-exist");
        let err = confine(&missing, "a.txt").unwrap_err();
        assert!(err.contains("workspace is not available"), "{err}");
        assert!(err.contains("does-not-exist"), "{err}");
        assert!(err.contains("Settings → Workspace"), "{err}");

        // Path escape guidance preserves the prefix and names both target and workspace.
        let guidance = path_escape_guidance(
            Path::new(r"C:\Users\alice\GrokHub-Work"),
            r"C:\Users\alice\Desktop\todo.txt",
        );
        assert!(
            guidance.contains("path escapes the workspace"),
            "{guidance}"
        );
        assert!(
            guidance.contains(r"C:\Users\alice\Desktop\todo.txt"),
            "{guidance}"
        );
        assert!(
            guidance.contains(r"C:\Users\alice\GrokHub-Work"),
            "{guidance}"
        );
        assert!(guidance.contains("Settings → Workspace"), "{guidance}");

        // Default Chat/Hangout workspace is created on disk and supports write + read_file.
        let ws = grokhub_core::paths::ensure_chat_workspace("", false, Some(&home_str), None);
        assert!(ws.is_dir());
        let stop = || false;
        let ctx = ToolCtx {
            workspace: &ws,
            desktop: None,
            stop: &stop,
            tasks: None,
            owner: None,
            session: Some("chat-ws-test"),
        };
        let wrote = dispatch(
            &ctx,
            "write",
            r#"{"path":"hello.txt","content":"hello from chat\n"}"#,
        );
        assert!(!wrote.failed, "{}", wrote.text);
        let read = dispatch(&ctx, "read_file", r#"{"target_file":"hello.txt"}"#);
        assert!(!read.failed, "{}", read.text);
        assert!(read.text.contains("hello from chat"), "{}", read.text);

        let _ = std::fs::remove_dir_all(&root);
    }
}
