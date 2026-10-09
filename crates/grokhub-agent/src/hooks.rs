// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Hook discovery and the sync runner.
//!
//! A hook can deny or ask. It cannot turn a gate deny or ask into allow.
//! A timeout kills the process tree and is no decision, so a PreToolUse
//! timeout leaves the gate's choice in place.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

use crate::gate::Decision;
use crate::skills::clip_chars;

const CONTEXT_CAP: usize = 2_000;
const REASON_CAP: usize = 500;
const MAX_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookOrigin {
    User,
    Project,
    Plugin,
}

impl HookOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookInfo {
    pub event: String,
    pub matcher: String,
    pub command: String,
    pub kind: String,
    pub origin: HookOrigin,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Command,
    Http,
}

#[derive(Debug, Clone)]
struct Loaded {
    event: String,
    matcher: String,
    command: String,
    kind: Kind,
    timeout: Duration,
    origin: HookOrigin,
    path: PathBuf,
    source_dir: PathBuf,
    env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    None,
    Allow { context: String },
    Ask { reason: String, context: String },
    Deny { reason: String },
}

pub struct ToolHook<'a> {
    pub session: &'a str,
    pub workspace: &'a Path,
    pub attended: bool,
    pub decision: Decision,
    pub ask_reason: String,
    pub name: &'a str,
    pub arguments: &'a str,
    pub tool_use_id: &'a str,
}

pub struct ToolConstraint {
    pub decision: Decision,
    pub ask_reason: String,
    pub context: String,
}

pub enum PromptHook {
    Continue { context: String },
    Block { reason: String },
}

struct Installed {
    hooks: Vec<Loaded>,
}

pub struct HookGuard {
    session: String,
}

impl Drop for HookGuard {
    fn drop(&mut self) {
        if self.session.is_empty() {
            return;
        }
        sessions().remove(&self.session);
    }
}

pub fn discover_hooks(
    workspace: &Path,
    home: Option<&Path>,
    plugin_dirs: &[PathBuf],
) -> Vec<HookInfo> {
    load_all(workspace, home, plugin_dirs)
        .into_iter()
        .map(|hook| HookInfo {
            event: hook.event,
            matcher: hook.matcher,
            command: hook.command,
            kind: match hook.kind {
                Kind::Command => "command".to_string(),
                Kind::Http => "http".to_string(),
            },
            origin: hook.origin,
            path: hook.path,
        })
        .collect()
}

pub fn attach(session: &str, workspace: &Path, _attended: bool) -> HookGuard {
    let hooks = load_all(
        workspace,
        grokhub_core::user_home().as_deref(),
        &crate::plugins::hook_paths(workspace),
    );
    install(session, runnable(hooks, folder_trusted(workspace)))
}

/// Project hooks come from the repository, so they only run once this folder is
/// trusted (as the Grok CLI gates repo-local config behind folder trust). User
/// hooks always run.
fn runnable(hooks: Vec<Loaded>, trusted: bool) -> Vec<Loaded> {
    hooks
        .into_iter()
        .filter(|hook| trusted || hook.origin != HookOrigin::Project)
        .collect()
}

const TRUST_FILE: &str = "trusted_folders.json";

fn trust_key(workspace: &Path) -> String {
    fs::canonicalize(workspace)
        .unwrap_or_else(|_| workspace.to_path_buf())
        .display()
        .to_string()
}

fn trusted_list() -> Vec<String> {
    let path = crate::perm::config_dir().join(TRUST_FILE);
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .unwrap_or_default()
}

fn save_trusted(list: &[String]) -> Result<(), String> {
    let dir = crate::perm::config_dir();
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let body = serde_json::to_string_pretty(list).map_err(|err| err.to_string())?;
    let tmp = dir.join(format!("{TRUST_FILE}.tmp"));
    fs::write(&tmp, body).map_err(|err| err.to_string())?;
    fs::rename(&tmp, dir.join(TRUST_FILE)).map_err(|err| err.to_string())
}

/// `true` when this folder's project hooks may run on the native engine.
pub fn folder_trusted(workspace: &Path) -> bool {
    let key = trust_key(workspace);
    trusted_list().contains(&key)
}

/// Record (or drop) trust for this folder's project hooks. The user's home and the
/// filesystem root are refused: trusting them would trust every repository below.
pub fn set_folder_trust(workspace: &Path, trusted: bool) -> Result<(), String> {
    let key = trust_key(workspace);
    let path = Path::new(&key);
    if trusted {
        let home = grokhub_core::user_home()
            .map(|home| trust_key(&home))
            .unwrap_or_default();
        if path.parent().is_none() || key == home || !path.is_absolute() {
            return Err("refusing to trust the home folder or the filesystem root".into());
        }
    }
    let mut list = trusted_list();
    list.retain(|item| *item != key);
    if trusted {
        list.push(key);
    }
    save_trusted(&list)
}

/// A test hook command that prints `json` (and exits with `exit`), in the shell the
/// hook runner uses on this platform: `sh -c` on Unix, PowerShell on Windows.
#[cfg(test)]
pub(crate) fn test_echo(json: &str, exit: Option<i32>) -> String {
    let tail = exit
        .map(|code| format!("; exit {code}"))
        .unwrap_or_default();
    if cfg!(windows) {
        format!("Write-Output '{json}'{tail}")
    } else {
        format!("printf '%s\\n' '{json}'{tail}")
    }
}

#[cfg(test)]
pub struct TestHook {
    pub event: String,
    pub matcher: String,
    pub command: String,
    pub timeout: Duration,
    pub source_dir: PathBuf,
}

#[cfg(test)]
pub fn install_test_hooks(session: &str, hooks: Vec<TestHook>) -> HookGuard {
    let loaded = hooks
        .into_iter()
        .map(|hook| Loaded {
            event: canonical_event(&hook.event)
                .unwrap_or("PreToolUse")
                .to_string(),
            matcher: hook.matcher,
            command: hook.command,
            kind: Kind::Command,
            timeout: hook.timeout,
            origin: HookOrigin::Project,
            path: hook.source_dir.join("test.json"),
            source_dir: hook.source_dir,
            env: Vec::new(),
        })
        .collect();
    install(session, loaded)
}

pub fn constrain_tool(hook: ToolHook<'_>) -> ToolConstraint {
    let subjects = tool_subjects(hook.name);
    let refs: Vec<&str> = subjects.iter().map(String::as_str).collect();
    let found = hooks_matching(hook.session, "PreToolUse", &refs);
    let cwd = hook.workspace.display().to_string();
    let body = payload(
        "PreToolUse",
        hook.session,
        &cwd,
        hook.name,
        hook.arguments,
        hook.tool_use_id,
        false,
        "",
    );
    let env = identity_env("PreToolUse", hook.session, &cwd, hook.name, hook.arguments);
    let verdict = run_chain(&found, hook.workspace, body.as_bytes(), &env, false);
    apply_tighten(hook.decision, hook.ask_reason, hook.attended, verdict)
}

pub fn on_user_prompt(session: &str, workspace: &Path, text: &str) -> PromptHook {
    let found = hooks_matching(session, "UserPromptSubmit", &[text]);
    if found.is_empty() {
        return PromptHook::Continue {
            context: String::new(),
        };
    }
    let cwd = workspace.display().to_string();
    let body = payload("UserPromptSubmit", session, &cwd, "", "", "", false, text);
    let env = identity_env("UserPromptSubmit", session, &cwd, "", "");
    match run_chain(&found, workspace, body.as_bytes(), &env, true) {
        Verdict::Deny { reason } => PromptHook::Block {
            reason: fallback(reason),
        },
        Verdict::Allow { context } | Verdict::Ask { context, .. } => {
            PromptHook::Continue { context }
        }
        Verdict::None => PromptHook::Continue {
            context: String::new(),
        },
    }
}

pub fn on_session_start(session: &str, workspace: &Path, source: &str) {
    observe(session, workspace, "SessionStart", source, "", "", false);
}

pub fn on_subagent_start(session: &str, workspace: &Path, name: &str) {
    observe(session, workspace, "SubagentStart", name, "", "", false);
}

pub fn on_subagent_stop(session: &str, workspace: &Path, name: &str) -> Option<String> {
    fire_stop(session, workspace, "SubagentStop", false, name)
}

pub fn on_stop(
    session: &str,
    workspace: &Path,
    event: &str,
    stop_hook_active: bool,
) -> Option<String> {
    fire_stop(session, workspace, event, stop_hook_active, "")
}

pub fn on_compact(session: &str, workspace: &Path, pre: bool) {
    let event = if pre { "PreCompact" } else { "PostCompact" };
    observe(session, workspace, event, "", "", "", false);
}

pub fn on_notification(session: &str, workspace: &Path, kind: &str) {
    observe(session, workspace, "Notification", kind, "", "", false);
}

pub fn on_permission_denied(
    session: &str,
    workspace: &Path,
    name: &str,
    arguments: &str,
    tool_use_id: &str,
) {
    let subjects = tool_subjects(name);
    let refs: Vec<&str> = subjects.iter().map(String::as_str).collect();
    let found = hooks_matching(session, "PermissionDenied", &refs);
    if found.is_empty() {
        return;
    }
    let cwd = workspace.display().to_string();
    let body = payload(
        "PermissionDenied",
        session,
        &cwd,
        name,
        arguments,
        tool_use_id,
        false,
        "",
    );
    let env = identity_env("PermissionDenied", session, &cwd, name, arguments);
    let _ = run_chain(&found, workspace, body.as_bytes(), &env, false);
}

pub fn after_tool(
    session: &str,
    workspace: &Path,
    name: &str,
    arguments: &str,
    tool_use_id: &str,
    failed: bool,
    _output: &str,
) -> String {
    let event = if failed {
        "PostToolUseFailure"
    } else {
        "PostToolUse"
    };
    let subjects = tool_subjects(name);
    let refs: Vec<&str> = subjects.iter().map(String::as_str).collect();
    let found = hooks_matching(session, event, &refs);
    if found.is_empty() {
        return String::new();
    }
    let cwd = workspace.display().to_string();
    let body = payload(
        event,
        session,
        &cwd,
        name,
        arguments,
        tool_use_id,
        false,
        "",
    );
    let env = identity_env(event, session, &cwd, name, arguments);
    let mut extra = String::new();
    for hook in &found {
        if hook.kind != Kind::Command {
            continue;
        }
        let merged = merge_env(&hook.env, &env);
        let proc = match crate::tools::shell::run_hook_command(
            workspace,
            &hook.command,
            body.as_bytes(),
            &merged,
            hook.timeout,
            &hook.source_dir,
        ) {
            Ok(proc) => proc,
            Err(_) => continue,
        };
        if proc.timed_out {
            continue;
        }
        let parsed = extract_json(&proc.stdout);
        if proc.exit_code == Some(2) {
            let reason = parsed
                .as_ref()
                .and_then(json_deny_reason)
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| clip_chars(proc.stderr.trim(), REASON_CAP));
            push_note(&mut extra, &fallback(reason));
            continue;
        }
        if let Some(value) = parsed.as_ref() {
            if matches!(decision_kind(value), Some(Choice::Deny)) {
                push_note(&mut extra, &fallback(reason_of(value)));
            } else {
                push_note(&mut extra, &context_of(value));
            }
        }
    }
    extra
}

fn fire_stop(
    session: &str,
    workspace: &Path,
    event: &str,
    stop_hook_active: bool,
    subject: &str,
) -> Option<String> {
    let found = hooks_matching(session, event, &[subject]);
    if found.is_empty() {
        return None;
    }
    let cwd = workspace.display().to_string();
    let body = payload(event, session, &cwd, "", "", "", stop_hook_active, "");
    let env = identity_env(event, session, &cwd, "", "");
    let verdict = run_chain(&found, workspace, body.as_bytes(), &env, true);
    if stop_hook_active {
        return None;
    }
    match verdict {
        Verdict::Deny { reason } if event == "Stop" || event == "SubagentStop" => {
            Some(fallback(reason))
        }
        _ => None,
    }
}

fn observe(
    session: &str,
    workspace: &Path,
    event: &str,
    subject: &str,
    tool: &str,
    arguments: &str,
    stop_active: bool,
) {
    let found = hooks_matching(session, event, &[subject]);
    if found.is_empty() {
        return;
    }
    let cwd = workspace.display().to_string();
    let body = payload(
        event,
        session,
        &cwd,
        tool,
        arguments,
        "",
        stop_active,
        subject,
    );
    let env = identity_env(event, session, &cwd, tool, arguments);
    let _ = run_chain(&found, workspace, body.as_bytes(), &env, false);
}

fn apply_tighten(
    decision: Decision,
    ask_reason: String,
    attended: bool,
    verdict: Verdict,
) -> ToolConstraint {
    match decision {
        Decision::Refuse(text) => ToolConstraint {
            decision: Decision::Refuse(text),
            ask_reason,
            context: String::new(),
        },
        Decision::Ask => {
            let decision = match verdict {
                Verdict::Deny { reason } => Decision::Refuse(fallback(reason)),
                _ => Decision::Ask,
            };
            ToolConstraint {
                decision,
                ask_reason,
                context: String::new(),
            }
        }
        Decision::Run => match verdict {
            Verdict::Deny { reason } => ToolConstraint {
                decision: Decision::Refuse(fallback(reason)),
                ask_reason,
                context: String::new(),
            },
            Verdict::Ask { reason, context } if attended => ToolConstraint {
                decision: Decision::Ask,
                ask_reason: if reason.is_empty() {
                    ask_reason
                } else {
                    reason
                },
                context,
            },
            Verdict::Ask { reason, .. } => ToolConstraint {
                decision: Decision::Refuse(fallback(reason)),
                ask_reason,
                context: String::new(),
            },
            Verdict::Allow { context } => ToolConstraint {
                decision: Decision::Run,
                ask_reason,
                context,
            },
            Verdict::None => ToolConstraint {
                decision: Decision::Run,
                ask_reason,
                context: String::new(),
            },
        },
    }
}

fn run_chain(
    hooks: &[Loaded],
    workspace: &Path,
    stdin: &[u8],
    env: &[(String, String)],
    prompt_or_stop: bool,
) -> Verdict {
    let mut ask: Option<Verdict> = None;
    let mut allow: Option<String> = None;
    for hook in hooks {
        if hook.kind != Kind::Command {
            continue;
        }
        let merged = merge_env(&hook.env, env);
        let proc = match crate::tools::shell::run_hook_command(
            workspace,
            &hook.command,
            stdin,
            &merged,
            hook.timeout,
            &hook.source_dir,
        ) {
            Ok(proc) => proc,
            Err(_) => continue,
        };
        match interpret(&proc, prompt_or_stop) {
            Verdict::Deny { reason } => return Verdict::Deny { reason },
            Verdict::Ask { reason, context } => {
                if ask.is_none() {
                    ask = Some(Verdict::Ask { reason, context });
                }
            }
            Verdict::Allow { context } => {
                if allow.is_none() {
                    allow = Some(context);
                }
            }
            Verdict::None => {}
        }
    }
    if let Some(ask) = ask {
        return ask;
    }
    if let Some(context) = allow {
        return Verdict::Allow { context };
    }
    Verdict::None
}

fn interpret(proc: &crate::tools::shell::HookProc, prompt_or_stop: bool) -> Verdict {
    if proc.timed_out {
        return Verdict::None;
    }
    let code = proc.exit_code.unwrap_or(1);
    let parsed = extract_json(&proc.stdout);
    if code == 2 {
        let reason = parsed
            .as_ref()
            .and_then(json_deny_reason)
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| clip_chars(proc.stderr.trim(), REASON_CAP));
        return Verdict::Deny {
            reason: fallback(reason),
        };
    }
    let Some(value) = parsed else {
        return Verdict::None;
    };
    let Some(choice) = decision_kind(&value) else {
        if code == 0 {
            let context = context_of(&value);
            if !context.is_empty() {
                return Verdict::Allow { context };
            }
        }
        return Verdict::None;
    };
    if choice == Choice::Deny {
        return Verdict::Deny {
            reason: fallback(reason_of(&value)),
        };
    }
    if code != 0 {
        return Verdict::None;
    }
    let context = context_of(&value);
    if choice == Choice::Allow {
        return Verdict::Allow { context };
    }
    if choice == Choice::Ask && !prompt_or_stop {
        return Verdict::Ask {
            reason: reason_of(&value),
            context,
        };
    }
    Verdict::None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Allow,
    Ask,
    Deny,
}

fn decision_kind(value: &Value) -> Option<Choice> {
    let specific = value.get("hookSpecificOutput");
    let perm = specific
        .and_then(|item| item.get("permissionDecision"))
        .and_then(|item| item.as_str())
        .unwrap_or("");
    let top = value
        .get("decision")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    let token = if perm.is_empty() { top } else { perm };
    match token.to_ascii_lowercase().as_str() {
        "allow" | "approve" => Some(Choice::Allow),
        "deny" | "block" => Some(Choice::Deny),
        "ask" => Some(Choice::Ask),
        _ => None,
    }
}

fn json_deny_reason(value: &Value) -> Option<String> {
    if decision_kind(value) == Some(Choice::Deny) {
        Some(reason_of(value))
    } else {
        None
    }
}

fn reason_of(value: &Value) -> String {
    let specific = value.get("hookSpecificOutput");
    let text = specific
        .and_then(|item| item.get("permissionDecisionReason"))
        .and_then(|item| item.as_str())
        .filter(|text| !text.is_empty())
        .or_else(|| value.get("reason").and_then(|item| item.as_str()))
        .unwrap_or("");
    clip_chars(text, REASON_CAP)
}

fn context_of(value: &Value) -> String {
    let specific = value
        .get("hookSpecificOutput")
        .and_then(|item| item.get("additionalContext"));
    let raw = specific.or_else(|| value.get("additionalContext"));
    match raw {
        Some(Value::String(text)) => clip_chars(text, CONTEXT_CAP),
        _ => String::new(),
    }
}

fn payload(
    event: &str,
    session: &str,
    cwd: &str,
    tool_name: &str,
    tool_input: &str,
    tool_use_id: &str,
    stop_hook_active: bool,
    prompt: &str,
) -> String {
    let input = if tool_input.is_empty() {
        Value::Null
    } else {
        serde_json::from_str::<Value>(tool_input)
            .unwrap_or_else(|_| Value::String(tool_input.to_string()))
    };
    json!({
        "hookEventName": event,
        "hook_event_name": event,
        "sessionId": session,
        "session_id": session,
        "cwd": cwd,
        "workspace_root": cwd,
        "toolName": tool_name,
        "tool_name": tool_name,
        "toolInput": input,
        "tool_input": input,
        "toolUseId": tool_use_id,
        "tool_use_id": tool_use_id,
        "stopHookActive": stop_hook_active,
        "stop_hook_active": stop_hook_active,
        "prompt": prompt,
    })
    .to_string()
}

fn identity_env(
    event: &str,
    session: &str,
    cwd: &str,
    tool_name: &str,
    tool_input: &str,
) -> Vec<(String, String)> {
    let mut env = vec![
        ("GROK_HOOK_EVENT".to_string(), event.to_string()),
        ("GROK_SESSION_ID".to_string(), session.to_string()),
        ("GROK_WORKSPACE_ROOT".to_string(), cwd.to_string()),
        ("CLAUDE_PROJECT_DIR".to_string(), cwd.to_string()),
        ("GROK_CWD".to_string(), cwd.to_string()),
    ];
    if !tool_name.is_empty() {
        env.push(("GROK_TOOL_NAME".to_string(), tool_name.to_string()));
    }
    if !tool_input.is_empty() && tool_input.len() <= 4096 {
        env.push(("GROK_TOOL_INPUT".to_string(), tool_input.to_string()));
    }
    env
}

fn merge_env(extra: &[(String, String)], identity: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = extra
        .iter()
        .filter(|(key, _)| !reserved_env(key))
        .cloned()
        .collect();
    out.extend(identity.iter().cloned());
    out
}

fn reserved_env(key: &str) -> bool {
    matches!(
        key,
        "GROK_HOOK_EVENT"
            | "GROK_SESSION_ID"
            | "GROK_WORKSPACE_ROOT"
            | "CLAUDE_PROJECT_DIR"
            | "GROK_CWD"
            | "GROK_TOOL_NAME"
            | "GROK_TOOL_INPUT"
    )
}

fn fallback(reason: String) -> String {
    let reason = reason.trim();
    if reason.is_empty() {
        "Denied by hook".to_string()
    } else {
        reason.to_string()
    }
}

fn push_note(out: &mut String, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(text);
}

fn extract_json(stdout: &str) -> Option<Value> {
    let start = stdout.find('{')?;
    let end = stdout.rfind('}')?;
    if end < start {
        return None;
    }
    serde_json::from_str(&stdout[start..=end]).ok()
}

fn sessions() -> std::sync::MutexGuard<'static, HashMap<String, Installed>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Installed>>> = OnceLock::new();
    SESSIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

fn install(session: &str, hooks: Vec<Loaded>) -> HookGuard {
    sessions().insert(session.to_string(), Installed { hooks });
    HookGuard {
        session: session.to_string(),
    }
}

fn hooks_matching(session: &str, event: &str, subjects: &[&str]) -> Vec<Loaded> {
    let map = sessions();
    let Some(installed) = map.get(session) else {
        return Vec::new();
    };
    installed
        .hooks
        .iter()
        .filter(|hook| hook.event == event && matcher_ok(hook, event, subjects))
        .cloned()
        .collect()
}

fn matcher_ok(hook: &Loaded, event: &str, subjects: &[&str]) -> bool {
    if matches!(event, "UserPromptSubmit" | "Stop") || hook.matcher.trim().is_empty() {
        return true;
    }
    let Ok(re) = regex::Regex::new(&hook.matcher) else {
        return false;
    };
    subjects.iter().any(|subject| re.is_match(subject))
}

fn tool_subjects(name: &str) -> Vec<String> {
    let mut out = vec![name.to_string()];
    let extra: &[&str] = match name {
        "run_terminal_command" => &["Bash", "bash"],
        "read_file" => &["Read"],
        "search_replace" => &["Edit", "Write", "MultiEdit"],
        "write" => &["Write", "Edit"],
        "grep" => &["Grep"],
        "list_dir" => &["ListDir", "Glob"],
        "glob" => &["Glob", "ListDir"],
        _ => &[],
    };
    for item in extra {
        out.push((*item).to_string());
    }
    out
}

fn load_all(workspace: &Path, home: Option<&Path>, plugin_dirs: &[PathBuf]) -> Vec<Loaded> {
    let mut raw = Vec::new();
    if let Some(home) = home {
        let grok = home.join(".grok");
        push_dir(&mut raw, &grok.join("hooks"), HookOrigin::User);
        push_registry(&mut raw, &grok.join("hooks-paths"), HookOrigin::User);
        let claude = home.join(".claude");
        push_settings(&mut raw, &claude.join("settings.json"), HookOrigin::User);
        push_settings(
            &mut raw,
            &claude.join("settings.local.json"),
            HookOrigin::User,
        );
    }
    for dir in crate::skills::project_chain(workspace) {
        push_dir(
            &mut raw,
            &dir.join(".grok").join("hooks"),
            HookOrigin::Project,
        );
        push_settings(
            &mut raw,
            &dir.join(".claude").join("settings.json"),
            HookOrigin::Project,
        );
        push_settings(
            &mut raw,
            &dir.join(".claude").join("settings.local.json"),
            HookOrigin::Project,
        );
    }
    for path in plugin_dirs {
        if regular_file(path) {
            push_settings(&mut raw, path, HookOrigin::Plugin);
        } else {
            push_dir(&mut raw, path, HookOrigin::Plugin);
        }
    }
    // Inline manifest hooks are only merged when the caller already asked for
    // plugin paths. An empty list stays a pure project/user scan.
    if !plugin_dirs.is_empty() {
        for (path, text) in crate::plugins::inline_hook_documents(workspace) {
            raw.extend(parse_hooks(&text, &path, HookOrigin::Plugin));
        }
    }
    dedupe(raw)
}

fn dedupe(hooks: Vec<Loaded>) -> Vec<Loaded> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for hook in hooks {
        let key = format!("{}\n{}\n{}", hook.event, hook.command, hook.matcher);
        if seen.insert(key) {
            out.push(hook);
        }
    }
    out
}

fn push_dir(out: &mut Vec<Loaded>, dir: &Path, origin: HookOrigin) {
    if !real_dir(dir) {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|ent| ent.ok())
        .map(|ent| ent.path())
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("json")
                && regular_file(path)
                && !credential_name(path)
        })
        .collect();
    files.sort();
    for path in files {
        push_settings(out, &path, origin);
    }
}

fn push_registry(out: &mut Vec<Loaded>, path: &Path, origin: HookOrigin) {
    if !regular_file(path) || credential_name(path) {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let target = PathBuf::from(line);
        if !target.is_absolute() || credential_name(&target) {
            continue;
        }
        if real_dir(&target) {
            push_dir(out, &target, origin);
        } else if regular_file(&target) {
            push_settings(out, &target, origin);
        }
    }
}

fn push_settings(out: &mut Vec<Loaded>, path: &Path, origin: HookOrigin) {
    if !regular_file(path) || credential_name(path) {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    out.extend(parse_hooks(&text, path, origin));
}

fn parse_hooks(text: &str, path: &Path, origin: HookOrigin) -> Vec<Loaded> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(map) = value.get("hooks").and_then(|item| item.as_object()) else {
        return Vec::new();
    };
    let source_dir = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut out = Vec::new();
    for (event_key, groups) in map {
        let Some(event) = canonical_event(event_key) else {
            continue;
        };
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for group in groups {
            let matcher = group
                .get("matcher")
                .and_then(|item| item.as_str())
                .unwrap_or("")
                .to_string();
            let Some(handlers) = group.get("hooks").and_then(|item| item.as_array()) else {
                continue;
            };
            for handler in handlers {
                let kind_name = handler
                    .get("type")
                    .and_then(|item| item.as_str())
                    .unwrap_or("command");
                let (kind, command) = if kind_name == "http" {
                    let url = handler
                        .get("url")
                        .and_then(|item| item.as_str())
                        .unwrap_or("")
                        .trim();
                    if url.is_empty() {
                        continue;
                    }
                    (Kind::Http, url.to_string())
                } else if kind_name == "command" {
                    let command = handler
                        .get("command")
                        .and_then(|item| item.as_str())
                        .unwrap_or("")
                        .trim();
                    if command.is_empty() {
                        continue;
                    }
                    (Kind::Command, command.to_string())
                } else {
                    continue;
                };
                out.push(Loaded {
                    event: event.to_string(),
                    matcher: matcher.clone(),
                    command,
                    kind,
                    timeout: timeout_of(event, handler.get("timeout")),
                    origin,
                    path: path.to_path_buf(),
                    source_dir: source_dir.clone(),
                    env: read_env(handler.get("env")),
                });
            }
        }
    }
    out
}

fn timeout_of(event: &str, value: Option<&Value>) -> Duration {
    let default = default_timeout(event);
    let Some(secs) = value
        .and_then(|item| item.as_u64())
        .filter(|secs| *secs > 0)
    else {
        return default;
    };
    let configured = Duration::from_secs(secs);
    if configured > MAX_TIMEOUT {
        MAX_TIMEOUT
    } else {
        configured
    }
}

fn default_timeout(event: &str) -> Duration {
    match event {
        "Stop" | "SubagentStop" | "PostToolUse" | "PostToolUseFailure" => Duration::from_secs(600),
        "UserPromptSubmit" => Duration::from_secs(30),
        "SessionEnd" => Duration::from_millis(1_500),
        _ => Duration::from_secs(5),
    }
}

fn read_env(value: Option<&Value>) -> Vec<(String, String)> {
    let Some(map) = value.and_then(|item| item.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, item) in map {
        let Some(text) = item.as_str() else {
            continue;
        };
        if key.is_empty() || reserved_env(key) {
            continue;
        }
        out.push((key.clone(), text.to_string()));
    }
    out
}

fn canonical_event(key: &str) -> Option<&'static str> {
    Some(match key {
        "SessionStart" | "session_start" | "sessionStart" => "SessionStart",
        "UserPromptSubmit" | "user_prompt_submit" | "beforeSubmitPrompt" => "UserPromptSubmit",
        "PreToolUse"
        | "pre_tool_use"
        | "preToolUse"
        | "beforeShellExecution"
        | "beforeMCPExecution"
        | "beforeReadFile" => "PreToolUse",
        "PostToolUse"
        | "post_tool_use"
        | "postToolUse"
        | "afterShellExecution"
        | "afterMCPExecution"
        | "afterFileEdit"
        | "afterAgentResponse"
        | "afterAgentThought" => "PostToolUse",
        "PostToolUseFailure" | "post_tool_use_failure" | "postToolUseFailure" => {
            "PostToolUseFailure"
        }
        "PermissionDenied" | "permission_denied" | "permissionDenied" => "PermissionDenied",
        "Stop" | "stop" => "Stop",
        "StopFailure" | "stop_failure" | "stopFailure" => "StopFailure",
        "StopCancelled" | "stop_cancelled" | "stopCancelled" => "StopCancelled",
        "Notification" | "notification" => "Notification",
        "SubagentStart" | "subagent_start" | "subagentStart" => "SubagentStart",
        "SubagentStop" | "subagent_stop" | "subagentStop" | "SubagentEnd" | "subagent_end"
        | "subagentEnd" => "SubagentStop",
        "PreCompact" | "pre_compact" | "preCompact" => "PreCompact",
        "PostCompact" | "post_compact" | "postCompact" => "PostCompact",
        "SessionEnd" | "session_end" | "sessionEnd" => "SessionEnd",
        _ => return None,
    })
}

fn real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_dir())
        .unwrap_or(false)
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_file())
        .unwrap_or(false)
}

fn credential_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    lower.contains("secret")
        || lower.contains("token")
        || lower.contains("credential")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".toml")
        || (lower.contains("auth") && lower.ends_with(".json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::Decision;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-hook-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_hook(dir: &Path, name: &str, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn settings_rows_include_source_path_event_matcher_and_command() {
        let root = scratch("list");
        let home = root.join("home");
        let repo = root.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        write_hook(
            &home.join(".grok").join("hooks"),
            "user.json",
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"echo user"}]}]}}"#,
        );
        write_hook(
            &repo.join(".grok").join("hooks"),
            "project.json",
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"echo user"}]},{"matcher":"Read","hooks":[{"type":"command","command":"echo project"}]}],"SessionEnd":[{"hooks":[{"type":"http","url":"http://127.0.0.1:9/nope"}]}]}}"#,
        );
        let rows = discover_hooks(&repo, Some(&home), &[]);
        let user = rows.iter().find(|row| row.command == "echo user").unwrap();
        assert_eq!(user.event, "PreToolUse");
        assert_eq!(user.matcher, "Bash");
        assert_eq!(user.origin, HookOrigin::User);
        assert!(user.path.ends_with("user.json"));
        let project = rows
            .iter()
            .find(|row| row.command == "echo project")
            .unwrap();
        assert_eq!(project.event, "PreToolUse");
        assert_eq!(project.matcher, "Read");
        assert_eq!(project.origin, HookOrigin::Project);
        assert!(project.path.ends_with("project.json"));
        assert_eq!(
            rows.iter().filter(|row| row.command == "echo user").count(),
            1
        );
        let http = rows.iter().find(|row| row.kind == "http").unwrap();
        assert_eq!(http.event, "SessionEnd");
        assert!(http.command.contains("127.0.0.1"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hook_deny_ask_and_allow_do_not_loosen_the_gate() {
        let dir = scratch("gate");
        let session = format!("gate-{}", std::process::id());
        let allow = install_test_hooks(
            &session,
            vec![TestHook {
                event: "PreToolUse".into(),
                matcher: String::new(),
                command: test_echo(r#"{"decision":"allow"}"#, None),
                timeout: Duration::from_secs(5),
                source_dir: dir.clone(),
            }],
        );
        let allowed_ask = constrain_tool(ToolHook {
            session: &session,
            workspace: &dir,
            attended: true,
            decision: Decision::Ask,
            ask_reason: "gate".into(),
            name: "write",
            arguments: r#"{"path":"out.txt"}"#,
            tool_use_id: "t1",
        });
        assert!(
            matches!(allowed_ask.decision, Decision::Ask),
            "{:?}",
            allowed_ask.decision
        );
        let allowed_deny = constrain_tool(ToolHook {
            session: &session,
            workspace: &dir,
            attended: true,
            decision: Decision::Refuse("kept".into()),
            ask_reason: String::new(),
            name: "write",
            arguments: "{}",
            tool_use_id: "t2",
        });
        match allowed_deny.decision {
            Decision::Refuse(text) => assert_eq!(text, "kept"),
            other => panic!("allow loosened a deny: {other:?}"),
        }
        drop(allow);
        let deny = install_test_hooks(
            &session,
            vec![TestHook {
                event: "PreToolUse".into(),
                matcher: "Write".into(),
                command: test_echo(r#"{"decision":"deny","reason":"nope"}"#, None),
                timeout: Duration::from_secs(5),
                source_dir: dir.clone(),
            }],
        );
        let denied = constrain_tool(ToolHook {
            session: &session,
            workspace: &dir,
            attended: true,
            decision: Decision::Run,
            ask_reason: String::new(),
            name: "write",
            arguments: "{}",
            tool_use_id: "t3",
        });
        match denied.decision {
            Decision::Refuse(text) => assert!(text.contains("nope"), "{text}"),
            other => panic!("deny did not stick: {other:?}"),
        }
        let exit2 = install_test_hooks(
            &format!("{session}-exit"),
            vec![TestHook {
                event: "PreToolUse".into(),
                matcher: String::new(),
                command: test_echo(r#"{"decision":"allow"}"#, Some(2)),
                timeout: Duration::from_secs(5),
                source_dir: dir.clone(),
            }],
        );
        let blocked = constrain_tool(ToolHook {
            session: &format!("{session}-exit"),
            workspace: &dir,
            attended: false,
            decision: Decision::Run,
            ask_reason: String::new(),
            name: "write",
            arguments: "{}",
            tool_use_id: "t4",
        });
        assert!(
            matches!(blocked.decision, Decision::Refuse(_)),
            "{:?}",
            blocked.decision
        );
        drop(deny);
        drop(exit2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hook_receives_session_cwd_event_and_tool_payload() {
        let dir = scratch("env");
        let session = format!("env-{}-{}", std::process::id(), "p10");
        let event_path = dir.join("event");
        let sid_path = dir.join("sid");
        let tool_path = dir.join("tool");
        let stdin_path = dir.join("stdin");
        let command = if cfg!(windows) {
            format!(
                "[IO.File]::WriteAllText('{}', $env:GROK_HOOK_EVENT); [IO.File]::WriteAllText('{}', $env:GROK_SESSION_ID); [IO.File]::WriteAllText('{}', $env:GROK_TOOL_NAME); [IO.File]::WriteAllText('{}', [Console]::In.ReadToEnd())",
                event_path.display(),
                sid_path.display(),
                tool_path.display(),
                stdin_path.display()
            )
        } else {
            format!(
                "printf '%s' \"$GROK_HOOK_EVENT\" > '{}'; printf '%s' \"$GROK_SESSION_ID\" > '{}'; printf '%s' \"$GROK_TOOL_NAME\" > '{}'; cat > '{}'",
                event_path.display(),
                sid_path.display(),
                tool_path.display(),
                stdin_path.display()
            )
        };
        let guard = install_test_hooks(
            &session,
            vec![TestHook {
                event: "PreToolUse".into(),
                matcher: String::new(),
                command,
                timeout: Duration::from_secs(5),
                source_dir: dir.clone(),
            }],
        );
        let out = constrain_tool(ToolHook {
            session: &session,
            workspace: &dir,
            attended: true,
            decision: Decision::Run,
            ask_reason: String::new(),
            name: "read_file",
            arguments: r#"{"target_file":"note.txt"}"#,
            tool_use_id: "call-9",
        });
        assert!(matches!(out.decision, Decision::Run));
        assert_eq!(fs::read_to_string(&event_path).unwrap(), "PreToolUse");
        assert_eq!(fs::read_to_string(&sid_path).unwrap(), session);
        assert_eq!(fs::read_to_string(&tool_path).unwrap(), "read_file");
        let stdin = fs::read_to_string(&stdin_path).unwrap();
        assert!(
            stdin.contains("\"hookEventName\":\"PreToolUse\""),
            "{stdin}"
        );
        assert!(stdin.contains(&session), "{stdin}");
        assert!(stdin.contains("note.txt"), "{stdin}");
        assert!(stdin.contains("call-9"), "{stdin}");
        drop(guard);
        let _ = fs::remove_dir_all(&dir);
    }

    fn sample_loaded() -> Loaded {
        Loaded {
            event: "PreToolUse".into(),
            matcher: String::new(),
            command: "true".into(),
            kind: Kind::Command,
            timeout: Duration::from_secs(1),
            origin: HookOrigin::User,
            path: PathBuf::new(),
            source_dir: PathBuf::new(),
            env: Vec::new(),
        }
    }

    #[test]
    fn project_hooks_run_only_in_a_trusted_folder() {
        let dir = scratch("trust");
        let _cfg = crate::perm::ConfigGuard::set(dir.join("cfg"));
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let hook = |origin: HookOrigin| Loaded {
            origin,
            ..sample_loaded()
        };
        let all = vec![hook(HookOrigin::User), hook(HookOrigin::Project)];
        assert!(!folder_trusted(&repo));
        let untrusted = runnable(all.clone(), folder_trusted(&repo));
        assert_eq!(untrusted.len(), 1);
        assert_eq!(untrusted[0].origin, HookOrigin::User);
        set_folder_trust(&repo, true).unwrap();
        assert!(folder_trusted(&repo));
        assert_eq!(runnable(all.clone(), folder_trusted(&repo)).len(), 2);
        set_folder_trust(&repo, false).unwrap();
        assert!(!folder_trusted(&repo));
        assert!(set_folder_trust(Path::new("/"), true).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_hook_that_never_reads_a_large_payload_still_times_out() {
        let dir = scratch("bigin");
        let session = format!("bigin-{}", std::process::id());
        let guard = install_test_hooks(
            &session,
            vec![TestHook {
                event: "PreToolUse".into(),
                matcher: String::new(),
                command: "sleep 30".into(),
                timeout: Duration::from_millis(800),
                source_dir: dir.clone(),
            }],
        );
        // Far larger than a pipe buffer: writing it inline would block before the timeout.
        let arguments = serde_json::json!({ "content": "x".repeat(4 * 1024 * 1024) }).to_string();
        let started = std::time::Instant::now();
        let out = constrain_tool(ToolHook {
            session: &session,
            workspace: &dir,
            attended: true,
            decision: Decision::Ask,
            ask_reason: "gate".into(),
            name: "write",
            arguments: &arguments,
            tool_use_id: "big",
        });
        assert!(matches!(out.decision, Decision::Ask), "{:?}", out.decision);
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "hook stdin blocked: {:?}",
            started.elapsed()
        );
        drop(guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn hook_timeout_kills_the_process_tree_and_does_not_allow() {
        let dir = scratch("time");
        let session = format!("time-{}", std::process::id());
        let pidfile = dir.join("child.pid");
        let command = format!("sleep 30 & echo $! > '{}'; wait", pidfile.display());
        let guard = install_test_hooks(
            &session,
            vec![TestHook {
                event: "PreToolUse".into(),
                matcher: String::new(),
                command,
                timeout: Duration::from_millis(800),
                source_dir: dir.clone(),
            }],
        );
        let out = constrain_tool(ToolHook {
            session: &session,
            workspace: &dir,
            attended: true,
            decision: Decision::Ask,
            ask_reason: "gate".into(),
            name: "write",
            arguments: "{}",
            tool_use_id: "slow",
        });
        assert!(
            matches!(out.decision, Decision::Ask),
            "timeout must not allow: {:?}",
            out.decision
        );
        let refused = constrain_tool(ToolHook {
            session: &session,
            workspace: &dir,
            attended: true,
            decision: Decision::Refuse("stays".into()),
            ask_reason: String::new(),
            name: "write",
            arguments: "{}",
            tool_use_id: "slow2",
        });
        match refused.decision {
            Decision::Refuse(text) => assert_eq!(text, "stays"),
            other => panic!("timeout loosened a deny: {other:?}"),
        }
        let pid_text = fs::read_to_string(&pidfile).unwrap_or_default();
        let pid: i32 = pid_text.trim().parse().unwrap_or(0);
        assert!(pid > 1, "child pid missing: {pid_text}");
        let own = unsafe { libc::getpgrp() };
        assert_ne!(own, pid, "refusing to signal the test process group");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while process_alive(pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!process_alive(pid), "grandchild {pid} still alive");
        assert!(process_alive(own), "test process group was killed");
        drop(guard);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    fn process_alive(pid: i32) -> bool {
        if pid <= 0 {
            return false;
        }
        let rc = unsafe { libc::kill(pid, 0) };
        if rc == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}
