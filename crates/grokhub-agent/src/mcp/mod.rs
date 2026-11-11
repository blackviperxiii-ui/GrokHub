//! MCP client. Sync JSON-RPC over stdio, streamable HTTP, and legacy SSE.
//! One worker thread per stdio child or HTTP read. No tokio.

mod browser;
mod config;
mod elicit;
mod headers;
mod http;
mod idem;
mod names;
mod oauth;
mod rpc;
mod stdio;

pub use config::{
    config_file, import_documents, import_paths, is_desktop_server, load_servers, load_servers_for,
    read_mcp_text, target_label, ImportReport, ServerDef,
};
pub use elicit::{
    alias_elicit, attach_elicit, detach_elicit, unalias_elicit, ElicitInbox, ElicitNote, ElicitView,
};
pub(crate) use elicit::{ask_secret, wait_elicit, ElicitAnswer};

pub(crate) use elicit::with_elicit;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::gate::{self, Decision, Gate, PermMode};
use crate::perm::{self, Action, Policy};
use crate::tools::{fence, ToolOutput};

use config::TransportDef;
use rpc::RawTool;
pub use rpc::{ToolHints, ToolIcon};

/// Individual tool schemas at this count. Above it, `search_tool` and
/// `use_tool`. Spike-2b: counts the native desktop tools and every MCP tool
/// (Cua included) together.
const DEFER_AFTER: usize = 40;

static GEN: AtomicU64 = AtomicU64::new(0);

pub fn invalidate() {
    GEN.fetch_add(1, Ordering::Relaxed);
}

fn cabin_cua() -> &'static Mutex<Option<PathBuf>> {
    static CUA: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    CUA.get_or_init(|| Mutex::new(None))
}

/// The cabin's own Cua gate (`<exe> --mcp-cua`) as a native MCP server while
/// the cabin wants it (Linux, desktop switch and `cuaDriver` flag on); `None`
/// takes it away. That process runs every call through `harness::decide`
/// before the pinned `cua-driver` child sees it. It is never written to
/// `mcp.json`, so user config can neither add nor route around it.
pub fn set_cabin_cua(exe: Option<PathBuf>) {
    let mut held = cabin_cua().lock().unwrap_or_else(|err| err.into_inner());
    if *held != exe {
        *held = exe;
        invalidate();
    }
}

fn cabin_cua_def() -> Option<ServerDef> {
    let exe = cabin_cua()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()?;
    Some(ServerDef {
        transport: TransportDef::Stdio {
            command: exe.display().to_string(),
            args: vec!["--mcp-cua".into()],
            env: BTreeMap::new(),
            cwd: None,
        },
        enabled: true,
        startup_timeout: std::time::Duration::from_secs(15),
        tool_timeout: std::time::Duration::from_secs(60),
        headers: BTreeMap::new(),
        token_ref: None,
        token_env: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorRow {
    pub name: String,
    pub status: String,
    pub tool_count: usize,
    pub last_error: String,
    pub detail: String,
    pub sign_in: SignIn,
}

/// A server's browser sign-in, for its row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    /// A stdio server, or one whose entry already carries its credentials.
    NotOffered,
    SignedOut,
    /// "Signed in to Linear as ada@example.com".
    SignedIn(String),
}

fn has_auth_header(headers: &BTreeMap<String, String>) -> bool {
    headers
        .keys()
        .any(|k| k.eq_ignore_ascii_case("authorization"))
}

fn sign_in_state(name: &str, def: &ServerDef) -> SignIn {
    let TransportDef::Http { url, .. } = &def.transport else {
        return SignIn::NotOffered;
    };
    if def.token_ref.is_some() || has_auth_header(&def.headers) {
        return SignIn::NotOffered;
    }
    match oauth::load(name).filter(|s| s.resource == grokhub_core::mcp_oauth::resource_for(url)) {
        Some(s) => SignIn::SignedIn(grokhub_core::mcp_oauth::signed_in_line(
            name,
            s.account.as_deref(),
        )),
        None => SignIn::SignedOut,
    }
}

#[derive(Clone)]
struct ToolRec {
    server: String,
    raw: String,
    qualified: String,
    title: Option<String>,
    description: String,
    schema: Value,
    output_schema: Option<Value>,
    hints: ToolHints,
    icons: Vec<ToolIcon>,
}

/// One connected MCP tool as its server's `tools/list` describes it, for a
/// connectors view. Every field is the server's word, so treat it as untrusted.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolInfo {
    pub server: String,
    /// The server's own tool name.
    pub name: String,
    /// The `server__tool` name the model calls.
    pub qualified: String,
    pub title: Option<String>,
    pub description: String,
    pub hints: ToolHints,
    pub icons: Vec<ToolIcon>,
    pub output_schema: Option<Value>,
}

impl ToolInfo {
    /// The tool's `title` when it has one, else its name.
    pub fn display_name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// Every connected MCP tool (connecting servers that are not yet).
pub fn tool_list() -> Vec<ToolInfo> {
    current_tools()
        .into_iter()
        .map(|tool| ToolInfo {
            server: tool.server,
            name: tool.raw,
            qualified: tool.qualified,
            title: tool.title,
            description: tool.description,
            hints: tool.hints,
            icons: tool.icons,
            output_schema: tool.output_schema,
        })
        .collect()
}

struct SlotInner {
    name: String,
    def: ServerDef,
    raw: Vec<RawTool>,
    tools: Vec<ToolRec>,
    last_error: Option<String>,
    tried: bool,
    /// A connect is running with the slot unlocked; its tools join when it ends.
    connecting: bool,
    /// Bumped per connect, so one that a restart or sign-out overtook is dropped.
    attempt: u64,
    conn: Option<Conn>,
    /// When `raw` was last listed, for the server's `ttlMs`.
    listed_at: Option<std::time::Instant>,
}

impl SlotInner {
    fn new(name: String, def: ServerDef) -> Self {
        Self {
            name,
            def,
            raw: Vec::new(),
            tools: Vec::new(),
            last_error: None,
            tried: false,
            connecting: false,
            attempt: 0,
            conn: None,
            listed_at: None,
        }
    }

    /// The server's `ttlMs` ran out since its tools were listed.
    fn list_expired(&self, now: std::time::Instant) -> bool {
        let (Some(conn), Some(at)) = (&self.conn, self.listed_at) else {
            return false;
        };
        conn.list_ttl()
            .is_some_and(|ttl| now.saturating_duration_since(at) >= ttl)
    }
}

enum Conn {
    Stdio(stdio::StdioConn),
    Http(http::HttpConn),
}

impl Conn {
    fn call(
        &mut self,
        name: &str,
        args: &Value,
        key: Option<&str>,
        timeout: std::time::Duration,
        stop: &dyn Fn() -> bool,
        retry_safe: bool,
    ) -> Result<(String, bool), String> {
        match self {
            Conn::Stdio(conn) => conn.call(name, args, key, timeout),
            Conn::Http(conn) => conn.call(name, args, key, timeout, stop, retry_safe),
        }
    }

    fn ping(&mut self, timeout: std::time::Duration) -> Result<(), String> {
        match self {
            Conn::Stdio(conn) => conn.ping(timeout),
            Conn::Http(conn) => conn.ping(timeout),
        }
    }

    fn list_ttl(&self) -> Option<std::time::Duration> {
        match self {
            Conn::Stdio(conn) => conn.list_ttl,
            Conn::Http(conn) => conn.list_ttl,
        }
    }

    fn relist(&mut self, timeout: std::time::Duration) -> Result<Vec<RawTool>, String> {
        match self {
            Conn::Stdio(conn) => stdio::relist(conn, timeout),
            Conn::Http(conn) => http::relist(conn, timeout),
        }
    }
}

/// Longest wait for a re-list after a server's `ttlMs` ran out.
const RELIST_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// List again the tools of each connected server whose `ttlMs` ran out. A
/// failed re-list keeps the old tools and waits out another `ttlMs`.
fn relist_expired(slots: &BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>>) {
    let now = std::time::Instant::now();
    for slot in slots.values() {
        let mut inner = lock_slot(slot);
        if !inner.list_expired(now) {
            continue;
        }
        let wait = inner.def.startup_timeout.min(RELIST_WAIT);
        let Some(conn) = inner.conn.as_mut() else {
            continue;
        };
        if let Ok(raw) = conn.relist(wait) {
            inner.raw = raw;
        }
        inner.listed_at = Some(std::time::Instant::now());
    }
}

struct State {
    path: PathBuf,
    r#gen: u64,
    mtime: Option<SystemTime>,
    /// Hash of enabled plugins. A disable or content change drops plugin servers.
    server_stamp: String,
    /// Offline only when the slots were built: a flip drops every connection,
    /// so a third-party stdio server stops and the rest reconnect.
    offline: bool,
    loaded: bool,
    workspace: PathBuf,
    slots: BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>>,
}

impl State {
    fn empty() -> Self {
        Self {
            path: PathBuf::new(),
            r#gen: 0,
            mtime: None,
            server_stamp: String::new(),
            offline: false,
            loaded: false,
            workspace: PathBuf::new(),
            slots: BTreeMap::new(),
        }
    }
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::empty()))
}

fn lock_state() -> MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|err| err.into_inner())
}

fn lock_slot(slot: &Mutex<SlotInner>) -> MutexGuard<'_, SlotInner> {
    slot.lock().unwrap_or_else(|err| err.into_inner())
}

pub fn set_workspace(path: &Path) {
    lock_state().workspace = path.to_path_buf();
}

/// Start every enabled server's connection on background threads at launch,
/// so a chat's first request doesn't wait on them. A server still connecting
/// when a chat starts joins that chat's tools at its next user turn.
pub fn connect_in_background(workspace: &Path) {
    set_workspace(workspace);
    std::thread::spawn(|| {
        let _ = current_tools();
    });
}

pub fn shutdown_all() {
    let mut held = lock_state();
    for slot in held.slots.values() {
        lock_slot(slot).conn.take();
    }
    held.slots.clear();
    held.loaded = false;
}

pub fn configured() -> Vec<DoctorRow> {
    let workspace = {
        let held = lock_state();
        workspace_of(&held)
    };
    config::load_servers_for(&workspace)
        .into_iter()
        .map(|(name, def)| DoctorRow {
            sign_in: sign_in_state(&name, &def),
            name,
            status: if def.enabled {
                "not checked"
            } else {
                "disabled"
            }
            .into(),
            tool_count: 0,
            last_error: String::new(),
            detail: config::target_label(&def),
        })
        .collect()
}

pub fn doctor() -> Vec<DoctorRow> {
    let _ = current_tools();
    let slots = slots_now();
    // A launch connect still running: give it a moment so the row is real.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < until && slots.values().any(|slot| lock_slot(slot).connecting)
    {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    requalify(&slots);
    let mut rows = Vec::new();
    for (name, slot) in slots {
        let mut inner = lock_slot(&slot);
        if !inner.def.enabled {
            rows.push(DoctorRow {
                sign_in: sign_in_state(&name, &inner.def),
                name,
                status: "disabled".into(),
                tool_count: 0,
                last_error: String::new(),
                detail: config::target_label(&inner.def),
            });
            continue;
        }
        let ping_for = std::time::Duration::from_secs(5).min(inner.def.tool_timeout);
        if let Some(conn) = inner.conn.as_mut() {
            if let Err(err) = conn.ping(ping_for) {
                inner.last_error = Some(clip_err(&err));
                inner.conn.take();
            }
        }
        let status = if inner.conn.is_some() && inner.last_error.is_none() {
            "connected"
        } else if inner.connecting {
            "connecting"
        } else if inner
            .last_error
            .as_deref()
            .is_some_and(grokhub_core::offline::is_refusal)
        {
            "paused"
        } else if inner.last_error.is_some() {
            "error"
        } else {
            "stopped"
        };
        rows.push(DoctorRow {
            sign_in: sign_in_state(&name, &inner.def),
            name,
            status: status.into(),
            tool_count: inner.tools.len(),
            last_error: inner.last_error.clone().unwrap_or_default(),
            detail: config::target_label(&inner.def),
        });
    }
    rows
}

pub fn restart(name: &str) -> Result<(), String> {
    let (slots, workspace) = load_slots();
    let Some(slot) = slots.get(name).cloned() else {
        return Err(format!("MCP server `{name}` is not configured"));
    };
    {
        let mut inner = lock_slot(&slot);
        if !inner.def.enabled {
            return Err(format!("MCP server `{name}` is disabled"));
        }
        inner.conn.take();
        inner.tried = false;
        inner.last_error = None;
        inner.raw.clear();
        inner.tools.clear();
    }
    connect_if_needed(&slot, &workspace);
    requalify(&slots);
    let inner = lock_slot(&slot);
    if inner.conn.is_none() {
        return Err(inner
            .last_error
            .clone()
            .unwrap_or_else(|| format!("MCP server `{name}` stopped")));
    }
    Ok(())
}

/// Sign in to an HTTP server in the browser `open` launches, then reconnect
/// it with the new token. Returns the line naming the account.
pub fn sign_in(name: &str, open: &dyn Fn(&str) -> Result<(), String>) -> Result<String, String> {
    let (slots, _) = load_slots();
    let slot = slots
        .get(name)
        .cloned()
        .ok_or_else(|| format!("MCP server `{name}` is not configured"))?;
    let url = match &lock_slot(&slot).def.transport {
        TransportDef::Http { url, .. } => url.clone(),
        TransportDef::Stdio { .. } => {
            return Err(format!(
                "MCP server `{name}` runs on this machine and has no sign-in"
            ))
        }
    };
    let line = oauth::sign_in(
        name,
        &url,
        open,
        std::time::Duration::from_secs(oauth::LOOPBACK_SECS),
    )?;
    match restart(name) {
        Ok(()) => Ok(line),
        Err(err) => Ok(format!("{line}, but it didn't connect: {err}")),
    }
}

/// Forget a server's sign-in and drop its connection.
pub fn sign_out(name: &str) -> String {
    oauth::forget(name);
    if let Some(slot) = slots_now().get(name) {
        let mut inner = lock_slot(slot);
        inner.conn.take();
        inner.tried = false;
        inner.last_error = None;
        inner.raw.clear();
        inner.tools.clear();
    }
    format!("Signed out of {name}")
}

/// Settings → Connectors Disconnect, after the user confirmed it. The entry
/// leaves `mcp.json` through the ChangeLedger (origin user, so `/connections`
/// can undo it), and its sign-in and sealed token go too. A plugin's server
/// is not in `mcp.json` and is refused.
pub fn disconnect(name: &str) -> Result<String, String> {
    let config = crate::perm::config_dir();
    let path = config::config_file();
    let target = crate::harness::McpFile { path: &path, name };
    let done = crate::harness::record_change(
        &config,
        &target,
        crate::harness::Origin::User,
        "disconnected in Settings",
        || crate::harness::ChangeTarget::put(&target, None),
    )?;
    if done.is_none() {
        return Err(format!(
            "{name} isn't one of {}'s connections",
            grokhub_core::brand::product_name()
        ));
    }
    crate::harness::forget_connection_token(&config, name);
    invalidate();
    shutdown_all();
    Ok(format!("Disconnected {name}"))
}

/// Settings → Connectors switch: turn a server off (`enabled: false` in its
/// `mcp.json` entry) or back on (the flag is dropped), through the
/// ChangeLedger (origin user, so `/connections` can undo it). A server that
/// is off is listed but never started. Desktop servers keep their lock.
pub fn set_enabled(name: &str, on: bool) -> Result<String, String> {
    if is_desktop_server(name) {
        return Err(format!("{name} belongs to the desktop and stays on."));
    }
    let config = crate::perm::config_dir();
    let path = config::config_file();
    let target = crate::harness::McpFile { path: &path, name };
    let cur = crate::harness::ChangeTarget::read(&target)?
        .ok_or_else(|| format!("{name} isn't one of this cabin's connections"))?;
    let mut entry: serde_json::Map<String, Value> =
        serde_json::from_slice(&cur).map_err(|e| e.to_string())?;
    let was_on = entry
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let line = if on {
        format!("Turned on {name}")
    } else {
        format!("Turned off {name}")
    };
    if was_on == on {
        return Ok(line);
    }
    if on {
        entry.remove("enabled");
    } else {
        entry.insert("enabled".into(), json!(false));
    }
    let bytes = serde_json::to_vec_pretty(&entry).map_err(|e| e.to_string())?;
    let why = if on {
        "turned on in Settings"
    } else {
        "turned off in Settings"
    };
    crate::harness::record_change(&config, &target, crate::harness::Origin::User, why, || {
        crate::harness::ChangeTarget::put(&target, Some(&bytes))
    })?;
    invalidate();
    shutdown_all();
    Ok(line)
}

/// What [`install`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    Added,
    /// The same entry was already there; nothing was written.
    AlreadyThere,
}

fn normalize_install_entry(entry: &Value) -> Value {
    let mut bare = entry.as_object().cloned().unwrap_or_default();
    bare.remove("tokenRef");
    let has_url = bare
        .get("url")
        .and_then(|v| v.as_str())
        .is_some_and(|url| !url.trim().is_empty());
    if !has_url && bare.contains_key("command") && !bare.contains_key("type") {
        bare.insert("type".into(), json!("stdio"));
    }
    if bare
        .get("args")
        .and_then(|v| v.as_array())
        .is_some_and(|items| items.is_empty())
    {
        bare.remove("args");
    }
    Value::Object(bare)
}

/// Marketplace Install, from the user's own click: `entry` (an `mcpServers`
/// value with no credentials) lands in `mcp.json` through the ChangeLedger
/// (origin user, so `/connections` can undo it). Installing the same entry
/// again writes nothing; a different entry under the name is refused, so a
/// connector the user set up is never overwritten.
pub fn install(name: &str, entry: &Value) -> Result<Installed, String> {
    let obj = entry
        .as_object()
        .ok_or("a connector entry must be a JSON object")?;
    if ["headers", "env", "bearerToken", "bearer_token", "tokenRef"]
        .iter()
        .any(|k| obj.contains_key(*k))
    {
        return Err(format!(
            "{name}'s entry carries credentials; a key is saved separately"
        ));
    }
    let config = crate::perm::config_dir();
    let path = config::config_file();
    let target = crate::harness::McpFile { path: &path, name };
    if let Some(cur) = crate::harness::ChangeTarget::read(&target)? {
        let cur: Value = serde_json::from_slice(&cur).map_err(|e| e.to_string())?;
        if normalize_install_entry(&cur) == normalize_install_entry(entry) {
            return Ok(Installed::AlreadyThere);
        }
        return Err(format!(
            "A connector named {name} is already set up differently; disconnect it first"
        ));
    }
    let bytes = serde_json::to_vec_pretty(entry).map_err(|e| e.to_string())?;
    crate::harness::record_change(
        &config,
        &target,
        crate::harness::Origin::User,
        "installed from Marketplace",
        || crate::harness::ChangeTarget::put(&target, Some(&bytes)),
    )?;
    invalidate();
    shutdown_all();
    Ok(Installed::Added)
}

/// Save a key the user typed for an installed connector, after they approved
/// it. The key is sealed with the keyring key and the entry only names it
/// (`tokenRef`): an HTTP server sends it as a bearer header, a stdio server
/// gets it in its `tokenEnv` variable.
pub fn save_key(name: &str, key: &str) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("No key was typed, so nothing was saved".into());
    }
    let config = crate::perm::config_dir();
    let path = config::config_file();
    let target = crate::harness::McpFile { path: &path, name };
    let cur = crate::harness::ChangeTarget::read(&target)?
        .ok_or_else(|| format!("{name} isn't installed, so it has nowhere to keep a key"))?;
    let mut entry: serde_json::Map<String, Value> =
        serde_json::from_slice(&cur).map_err(|e| e.to_string())?;
    crate::harness::seal_connection_token(&config, name, key)?;
    entry.insert("tokenRef".into(), json!(name));
    let bytes = serde_json::to_vec_pretty(&entry).map_err(|e| e.to_string())?;
    crate::harness::record_change(
        &config,
        &target,
        crate::harness::Origin::User,
        "key saved in Settings",
        || crate::harness::ChangeTarget::put(&target, Some(&bytes)),
    )?;
    invalidate();
    Ok(format!("Saved the key for {name}"))
}

/// MCP schemas for the native engine. `native` is how many desktop tools
/// it registers beside them; past [`DEFER_AFTER`] in all, MCP tools hide
/// behind `search_tool` and `use_tool`.
pub(crate) fn schema_tools(native: usize) -> Vec<Value> {
    let tools = current_tools();
    if tools.len() + native > DEFER_AFTER {
        return vec![search_schema(), use_schema()];
    }
    tools.iter().map(one_schema).collect()
}

/// The MCP schemas each chat was offered, so a server that connects mid-turn
/// never rewrites the chat's tool list (a full prompt-cache miss). `refresh`
/// is set at a user-turn boundary, the only time the list may change.
struct Frozen {
    schemas: Vec<Value>,
    refresh: bool,
    /// Tools a provider refused over their schema: sent with no parameters
    /// for the rest of the chat, refresh included.
    bare: std::collections::HashSet<String>,
}

/// Keep the record small: a long-lived cabin opens many chats. The least
/// recently used one goes first, so the open chat keeps its list.
const MAX_FROZEN_CHATS: usize = 256;

fn frozen_chats() -> MutexGuard<'static, crate::lru_map::LruMap<Frozen>> {
    static FROZEN: Mutex<crate::lru_map::LruMap<Frozen>> =
        Mutex::new(crate::lru_map::LruMap::new(MAX_FROZEN_CHATS));
    FROZEN.lock().unwrap_or_else(|err| err.into_inner())
}

/// A new user turn in `session`: its next request may take in servers that
/// connected since, at the end of its MCP tools.
pub(crate) fn begin_turn(session: &str) {
    if let Some(chat) = frozen_chats().get_mut(session) {
        chat.refresh = true;
    }
}

/// [`schema_tools`] frozen per chat: the first request's list holds for the
/// rest of that user turn. At the next turn, tools the chat already had keep
/// their place, gone ones leave and new ones join at the end. An empty
/// `session` gets the live list.
pub(crate) fn chat_schema_tools(session: &str, native: usize) -> Vec<Value> {
    if session.is_empty() {
        return schema_tools(native);
    }
    if let Some(chat) = frozen_chats().get(session) {
        if !chat.refresh {
            return chat.schemas.clone();
        }
    }
    let now = schema_tools(native);
    let mut chats = frozen_chats();
    let chat = chats.get_or_insert_with(session, || Frozen {
        schemas: Vec::new(),
        refresh: true,
        bare: Default::default(),
    });
    if chat.refresh {
        chat.schemas = merge_frozen(&chat.schemas, now);
        for tool in &mut chat.schemas {
            if tool["name"].as_str().is_some_and(|n| chat.bare.contains(n)) {
                tool["parameters"] = bare_parameters();
            }
        }
        chat.refresh = false;
    }
    chat.schemas.clone()
}

fn bare_parameters() -> Value {
    json!({"type": "object", "properties": {}})
}

/// The provider refused a request with `error`. When it names an MCP tool
/// this chat offers with parameters, that tool is offered bare from now on
/// (its server still checks the arguments) and this returns true, so the
/// step can be sent once more instead of every turn failing on one schema.
pub(crate) fn bare_schema_named_in(session: &str, error: &str) -> bool {
    let mut chats = frozen_chats();
    let Some(chat) = chats.get_mut(session) else {
        return false;
    };
    let mut changed = false;
    for tool in &mut chat.schemas {
        let Some(name) = tool["name"].as_str().map(str::to_string) else {
            continue;
        };
        if !name.contains("__") || !error.contains(&name) || chat.bare.contains(&name) {
            continue;
        }
        tool["parameters"] = bare_parameters();
        chat.bare.insert(name);
        changed = true;
    }
    changed
}

/// `old`'s tools that are still offered, in `old`'s order with today's
/// schema, then the new ones in the order `now` lists them.
fn merge_frozen(old: &[Value], now: Vec<Value>) -> Vec<Value> {
    let name = |tool: &Value| tool["name"].as_str().unwrap_or_default().to_string();
    let mut merged: Vec<Value> = old
        .iter()
        .filter_map(|kept| now.iter().find(|tool| name(tool) == name(kept)).cloned())
        .collect();
    for tool in now {
        if !merged.iter().any(|have| name(have) == name(&tool)) {
            merged.push(tool);
        }
    }
    merged
}

/// `stop` is the run's Stop / Halt: an egress park waiting on a card gives up
/// when it turns true.
pub(crate) fn try_dispatch(
    name: &str,
    args: &Value,
    stop: &dyn Fn() -> bool,
) -> Option<ToolOutput> {
    if name == "search_tool" {
        return Some(search_output(args));
    }
    if name != "use_tool" && !name.contains("__") {
        return None;
    }
    let tools = current_tools();
    if name == "use_tool" {
        return Some(use_output(&tools, args, stop));
    }
    let rec = tools.into_iter().find(|tool| tool.qualified == name)?;
    #[cfg(test)]
    if test_snapshot().is_some() {
        return Some(ToolOutput::ok(format!("test-call {}", rec.qualified)));
    }
    Some(call_qualified(&rec, args, stop))
}

pub(crate) fn search_output(args: &Value) -> ToolOutput {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let tools = current_tools();
    let mut lines = Vec::new();
    for tool in &tools {
        let about = model_description(tool, usize::MAX);
        let hay = format!("{} {about}", tool.qualified).to_ascii_lowercase();
        if query.is_empty() || hay.contains(&query) {
            lines.push(format!("{} — {about}", tool.qualified));
        }
        if lines.len() == 50 {
            break;
        }
    }
    if lines.is_empty() {
        ToolOutput::ok("No matching MCP tools")
    } else {
        ToolOutput::ok(lines.join("\n"))
    }
}

enum Resolved {
    Known { name: String, destructive: bool },
    Missing(String),
}

pub(crate) fn permission(
    gate: &Gate,
    name: &str,
    arguments: &str,
    latched_always: bool,
    policy: Option<&Policy>,
) -> Option<Decision> {
    match resolve(name, arguments)? {
        Resolved::Missing(target) => Some(Decision::Refuse(format!("unknown tool `{target}`"))),
        Resolved::Known { name, destructive } => {
            Some(decide_mcp(gate, &name, destructive, latched_always, policy))
        }
    }
}

pub(crate) fn is_mcp_call(name: &str, arguments: &str) -> bool {
    matches!(resolve(name, arguments), Some(Resolved::Known { .. }))
}

/// An MCP call whose server marks the tool `destructiveHint: true`. The
/// auto-review judge leaves its card to the user.
pub(crate) fn is_destructive_call(name: &str, arguments: &str) -> bool {
    matches!(
        resolve(name, arguments),
        Some(Resolved::Known {
            destructive: true,
            ..
        })
    )
}

pub(crate) fn has_ask_rule(policy: &Policy, name: &str, arguments: &str) -> bool {
    let Some(Resolved::Known { name: target, .. }) = resolve(name, arguments) else {
        return false;
    };
    rule(policy, &target, Action::Ask)
}

/// Why a destructive MCP tool did not run while nobody was there to approve it.
fn destructive_unattended(name: &str) -> String {
    format!(
        "Tool `{name}` was not executed: its server marks it destructive, and nobody is here to approve it"
    )
}

fn decide_mcp(
    gate: &Gate,
    name: &str,
    destructive: bool,
    latched_always: bool,
    policy: Option<&Policy>,
) -> Decision {
    // A server `readOnlyHint` does not grant a run. An allow rule does.
    if gate.readonly_session {
        return Decision::Refuse(gate::readonly_refusal(name));
    }
    // Always (the mode or the card's latch) skips the card, except for a tool
    // its server marks destructive: that one asks every time.
    let always = !destructive && (gate.mode == PermMode::Always || latched_always);
    let away = || {
        if destructive {
            Decision::Refuse(destructive_unattended(name))
        } else {
            Decision::Refuse(gate::mcp_policy_deny(name))
        }
    };
    if let Some(policy) = policy {
        if rule(policy, name, Action::Deny) {
            return Decision::Refuse(gate::mcp_policy_deny(name));
        }
        if rule(policy, name, Action::Ask) {
            // Always Allow: an ask rule is not a hard class, so it runs.
            return if gate.attended && always {
                Decision::Run
            } else if gate.attended {
                Decision::Ask
            } else {
                away()
            };
        }
        // The user's own allow rule for this tool is their standing approval.
        if rule(policy, name, Action::Allow) {
            return Decision::Run;
        }
    }
    if always {
        return Decision::Run;
    }
    if gate.attended {
        return Decision::Ask;
    }
    away()
}

fn rule(policy: &Policy, name: &str, action: Action) -> bool {
    policy
        .rules
        .iter()
        .any(|item| item.action == action && perm::mcp_matches(item, name))
}

fn resolve(name: &str, arguments: &str) -> Option<Resolved> {
    if name != "use_tool" && !name.contains("__") {
        return None;
    }
    let tools = current_tools();
    if name == "use_tool" {
        let target = arg_name(arguments);
        if target.is_empty() {
            return Some(Resolved::Missing("use_tool".into()));
        }
        return Some(match tools.iter().find(|tool| tool.qualified == target) {
            Some(tool) => Resolved::Known {
                destructive: tool.hints.is_destructive(),
                name: target,
            },
            None => Resolved::Missing(target),
        });
    }
    tools
        .iter()
        .find(|tool| tool.qualified == name)
        .map(|tool| Resolved::Known {
            name: name.to_string(),
            destructive: tool.hints.is_destructive(),
        })
}

fn arg_name(arguments: &str) -> String {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|v| {
            v.get("name")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn use_output(tools: &[ToolRec], args: &Value, stop: &dyn Fn() -> bool) -> ToolOutput {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return ToolOutput::err("unknown tool `use_tool`");
    }
    let Some(rec) = tools.iter().find(|tool| tool.qualified == name) else {
        return ToolOutput::err(format!("unknown tool `{name}`"));
    };
    let call_args = normalize_args(args.get("arguments"));
    #[cfg(test)]
    if test_snapshot().is_some() {
        return ToolOutput::ok(format!("test-call {}", rec.qualified));
    }
    call_qualified(rec, &call_args, stop)
}

fn normalize_args(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return json!({});
    };
    if let Some(text) = value.as_str() {
        return serde_json::from_str(text).unwrap_or_else(|_| json!({}));
    }
    if value.is_object() {
        value.clone()
    } else {
        json!({})
    }
}

fn call_qualified(rec: &ToolRec, args: &Value, stop: &dyn Fn() -> bool) -> ToolOutput {
    let slots = slots_now();
    let Some(slot) = slots.get(&rec.server).cloned() else {
        return ToolOutput::err(format!("MCP server `{}` stopped", rec.server));
    };
    let mut inner = lock_slot(&slot);
    let timeout = inner.def.tool_timeout;
    let Some(conn) = inner.conn.as_mut() else {
        return match inner.last_error.as_deref() {
            Some(err) => fenced_error(err),
            None => ToolOutput::err(format!("MCP server `{}` stopped", rec.server)),
        };
    };
    // Item 24: a call that can change things carries an idempotency key,
    // the same one when it is resent after an answer that never came.
    let keyed = idem::needs_key(&rec.hints).then(|| idem::fingerprint(&rec.server, &rec.raw, args));
    let key = keyed.as_deref().map(idem::begin);
    // A dropped HTTP call is sent once more only when running it twice is
    // harmless: the server says the tool is read-only or idempotent.
    let retry_safe = rec.hints.read_only || rec.hints.idempotent == Some(true);
    let result = conn.call(&rec.raw, args, key.as_deref(), timeout, stop, retry_safe);
    if let Some(print) = &keyed {
        idem::end(
            print,
            result
                .as_ref()
                .map_or_else(|err| !idem::maybe_ran(err), |_| true),
        );
    }
    // A browser that died at launch gets one isolated relaunch, then the
    // model is pointed at web_fetch instead of a dead tool (LO-51).
    let died = match &result {
        Ok((text, true)) => browser::launch_died(text).then(|| text.clone()),
        Err(err) => browser::launch_died(err).then(|| err.clone()),
        Ok(_) => None,
    };
    let playwright = matches!(
        &inner.def.transport,
        TransportDef::Stdio { command, args, .. } if browser::is_playwright(command, args)
    );
    if let Some(text) = died.filter(|_| playwright) {
        eprintln!("grokhub mcp `{}` browser launch died: {text}", rec.server);
        inner.conn.take();
        inner.last_error = Some(clip_err(&text));
        drop(inner);
        if browser::isolate(&rec.server) && restart(&rec.server).is_ok() {
            return call_qualified(rec, args, stop);
        }
        let mut out = fenced_error(&text);
        out.text = format!("{}\n{}", browser::CANT_START, out.text);
        return out;
    }
    match result {
        Ok((text, true)) => fenced_error(&text),
        Ok((text, false)) => ToolOutput::ok(fence::fence("mcp", &text)),
        Err(err) => {
            inner.conn.take();
            inner.last_error = Some(clip_err(&err));
            // An expired sign-in: the next use reconnects, refreshing the token.
            // Checked on the raw text, before it is fenced.
            if http::is_unauthorized(&err) {
                inner.tried = false;
            }
            let mut out = fenced_error(&err);
            if keyed.is_some() && idem::maybe_ran(&err) {
                out.text.push_str(idem::MAYBE_RAN);
            }
            out
        }
    }
}

/// A failed MCP call for the model. The text can be the server's own words
/// (an `isError` result, a JSON-RPC error message, a saved error), so it goes
/// inside the untrusted fence behind a trusted lead, like a successful result.
fn fenced_error(text: &str) -> ToolOutput {
    ToolOutput::err(format!(
        "{}\n{}",
        fence::ERROR_LEAD,
        fence::fence("mcp", text)
    ))
}

/// `Title: description (server hint: …)` for the model, the description
/// clipped to `max` chars.
fn model_description(tool: &ToolRec, max: usize) -> String {
    let body = clip_chars(&tool.description, max);
    let mut description = match tool.title.as_deref() {
        Some(title) if body.is_empty() => title.to_string(),
        Some(title) if !body.starts_with(title) => format!("{title}: {body}"),
        _ => body,
    };
    let mut hint = |note: &str| {
        if !description.is_empty() {
            description.push(' ');
        }
        description.push_str(note);
    };
    if tool.hints.read_only {
        hint("(server hint: read-only)");
    }
    if tool.hints.is_destructive() {
        hint("(server hint: destructive; asks you every time)");
    }
    description
}

fn one_schema(tool: &ToolRec) -> Value {
    let description = model_description(tool, 1024);
    let parameters = if tool.schema.is_object() {
        tool.schema.clone()
    } else {
        json!({"type": "object", "properties": {}})
    };
    json!({
        "type": "function",
        "name": tool.qualified,
        "description": description,
        "parameters": parameters,
    })
}

fn search_schema() -> Value {
    json!({
        "type": "function",
        "name": "search_tool",
        "description": "Search connected MCP tools by name or description. Returns matching server__tool names. Call use_tool with one name.",
        "parameters": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Case-insensitive substring."}
            },
            "required": ["query"],
            "additionalProperties": false
        }
    })
}

fn use_schema() -> Value {
    json!({
        "type": "function",
        "name": "use_tool",
        "description": "Call one MCP tool by its server__tool name. arguments is a JSON object or a JSON string.",
        "parameters": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "arguments": {"description": "JSON object, or a string of JSON."}
            },
            "required": ["name"],
            "additionalProperties": false
        }
    })
}

fn current_tools() -> Vec<ToolRec> {
    #[cfg(test)]
    if let Some(tools) = test_snapshot() {
        return tools;
    }
    let (slots, workspace) = load_slots();
    let pending: Vec<_> = slots
        .values()
        .filter(|slot| {
            let inner = lock_slot(slot);
            inner.def.enabled && !inner.tried && inner.conn.is_none()
        })
        .collect();
    // Servers nobody has started connect side by side, so the wait is the
    // slowest one, not their sum. One a launch connect is still opening is
    // not waited on.
    // Tests pin config and Offline only per thread, so they connect in line.
    let side_by_side = !cfg!(test) && pending.len() > 1;
    match pending.as_slice() {
        many if side_by_side => std::thread::scope(|scope| {
            for slot in many {
                let workspace = &workspace;
                scope.spawn(move || connect_if_needed(slot, workspace));
            }
        }),
        each => {
            for slot in each {
                connect_if_needed(slot, &workspace);
            }
        }
    }
    relist_expired(&slots);
    requalify(&slots);
    flatten(&slots)
}

fn slots_now() -> BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>> {
    let (slots, _) = load_slots();
    slots
}

fn workspace_of(held: &State) -> PathBuf {
    if held.workspace.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir())
    } else {
        held.workspace.clone()
    }
}

fn load_slots() -> (BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>>, PathBuf) {
    let mut held = lock_state();
    let workspace = workspace_of(&held);
    // Plugin state is read while this lock is held. Plugin code must not take it.
    let stamp = crate::plugins::listing_stamp(&workspace);
    let path = config::config_file();
    let (r#gen, mtime) = fingerprint(&path);
    let offline = grokhub_core::offline::enabled();
    if !held.loaded
        || held.offline != offline
        || held.path != path
        || held.r#gen != r#gen
        || held.mtime != mtime
        || held.server_stamp != stamp
    {
        for slot in held.slots.values() {
            lock_slot(slot).conn.take();
        }
        held.slots.clear();
        let mut servers = config::load_servers_for(&workspace);
        if let Some(def) = cabin_cua_def() {
            servers.insert(grokhub_core::CUA_MCP_SERVER.to_string(), def);
        }
        for (name, def) in servers {
            held.slots.insert(
                name.clone(),
                std::sync::Arc::new(Mutex::new(SlotInner::new(name, def))),
            );
        }
        held.loaded = true;
        held.path = path;
        held.r#gen = r#gen;
        held.mtime = mtime;
        held.server_stamp = stamp;
        held.offline = offline;
    }
    (held.slots.clone(), workspace)
}

fn fingerprint(path: &Path) -> (u64, Option<SystemTime>) {
    let r#gen = GEN.load(Ordering::Relaxed);
    let mtime = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok();
    (r#gen, mtime)
}

fn connect_if_needed(slot: &Mutex<SlotInner>, workspace: &Path) {
    let (name, def, attempt) = {
        let mut inner = lock_slot(slot);
        if !inner.def.enabled || inner.tried || inner.conn.is_some() {
            return;
        }
        inner.tried = true;
        inner.connecting = true;
        inner.attempt = inner.attempt.wrapping_add(1);
        (inner.name.clone(), inner.def.clone(), inner.attempt)
    };
    // Open without holding the slot: a reader sees no tools from it until it
    // is up, instead of waiting on a slow server.
    let opened = open_def(&name, &def, workspace);
    let mut inner = lock_slot(slot);
    if inner.attempt != attempt {
        return;
    }
    inner.connecting = false;
    if !inner.tried {
        // Signed out or restarted while it opened: this connection is stale.
        return;
    }
    match opened {
        Ok((conn, raw)) => {
            inner.raw = raw;
            inner.conn = Some(conn);
            inner.listed_at = Some(std::time::Instant::now());
            inner.last_error = None;
        }
        Err(err) => {
            inner.raw.clear();
            inner.conn = None;
            inner.last_error = Some(clip_err(&err));
        }
    }
}

fn open_def(name: &str, def: &ServerDef, workspace: &Path) -> Result<(Conn, Vec<RawTool>), String> {
    // A key is saved under its connector's own name, so an entry that names
    // any other key is asking for one it was never given.
    if def.token_ref.as_deref().is_some_and(|key| key != name) {
        return Err(format!(
            "{name} names another connector's key, so it was not used: save a key for {name} in Settings, Connectors"
        ));
    }
    match &def.transport {
        TransportDef::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let exe = std::env::current_exe().ok();
            if let Some(why) = offline_paused(
                name,
                command,
                args,
                exe.as_deref(),
                grokhub_core::offline::enabled(),
            ) {
                return Err(why);
            }
            let dir = stdio::workspace_or(cwd.clone(), workspace);
            let mut env = env.clone();
            if let (Some(token_name), Some(var)) = (&def.token_ref, &def.token_env) {
                let token = crate::harness::open_connection_token(&perm::config_dir(), token_name)
                    .ok_or("its key is missing or locked: save it again in Settings, Connectors")?;
                env.insert(var.clone(), token);
            }
            let (conn, tools) =
                stdio::connect(name, command, args, &env, &dir, def.startup_timeout)?;
            Ok((Conn::Stdio(conn), tools))
        }
        TransportDef::Http { url, sse } => {
            let mut headers = def.headers.clone();
            if let Some(token_name) = &def.token_ref {
                let token = crate::harness::open_connection_token(&perm::config_dir(), token_name)
                    .ok_or("its token is missing or locked: add the connection again")?;
                headers
                    .entry("Authorization".into())
                    .or_insert_with(|| format!("Bearer {token}"));
            }
            let signed = !has_auth_header(&headers);
            if signed {
                if let Some(token) = oauth::bearer(name, url) {
                    headers.insert("Authorization".into(), format!("Bearer {token}"));
                }
            }
            let open = |headers: &BTreeMap<String, String>| {
                http::connect(
                    name,
                    url,
                    *sse,
                    headers,
                    def.startup_timeout,
                    def.tool_timeout,
                )
            };
            let (conn, tools) = match open(&headers) {
                Err(err) if signed && http::is_unauthorized(&err) => {
                    if !has_auth_header(&headers) {
                        return Err(format!("{err}. Sign in on its row under Native MCP"));
                    }
                    // The token was refused: refresh once and try again.
                    let token = oauth::refresh_now(name, url).map_err(|e| format!("{err}. {e}"))?;
                    headers.insert("Authorization".into(), format!("Bearer {token}"));
                    open(&headers)?
                }
                other => other?,
            };
            Ok((Conn::Http(conn), tools))
        }
    }
}

/// The app's own stdio servers (`<this exe> --mcp-cua`, `--mcp-self`). They
/// never reach the internet, so Offline only leaves them running.
fn own_server(name: &str, command: &str, args: &[String], exe: Option<&Path>) -> bool {
    [grokhub_core::CUA_MCP_SERVER, grokhub_core::SELF_MCP_SERVER].contains(&name)
        && args.first().is_some_and(|a| a.starts_with("--mcp-"))
        && exe.is_some_and(|exe| Path::new(command) == exe)
}

/// Offline only pauses a third-party stdio server: it can reach the internet
/// on its own, so it doesn't start while the switch is on. Its row reads
/// `paused` with [`grokhub_core::offline::OFFLINE_MSG`].
fn offline_paused(
    name: &str,
    command: &str,
    args: &[String],
    exe: Option<&Path>,
    on: bool,
) -> Option<String> {
    (on && !own_server(name, command, args, exe))
        .then(|| grokhub_core::offline::OFFLINE_MSG.to_string())
}

fn requalify(slots: &BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>>) {
    let mut guards = Vec::new();
    let mut pairs = Vec::new();
    for slot in slots.values() {
        let guard = lock_slot(slot);
        for tool in &guard.raw {
            pairs.push((guard.name.clone(), tool.name.clone()));
        }
        guards.push(guard);
    }
    let qualified = names::qualify_all(&pairs);
    let mut map = std::collections::HashMap::new();
    for item in qualified {
        map.insert((item.server, item.tool), item.name);
    }
    for guard in &mut guards {
        let server = guard.name.clone();
        let tools: Vec<ToolRec> = guard
            .raw
            .iter()
            .filter_map(|tool| {
                let qname = map.get(&(server.clone(), tool.name.clone()))?;
                Some(ToolRec {
                    server: server.clone(),
                    raw: tool.name.clone(),
                    qualified: qname.clone(),
                    title: tool.title.clone(),
                    description: clip_chars(&tool.description, 4000),
                    schema: tool.input_schema.clone(),
                    output_schema: tool.output_schema.clone(),
                    hints: tool.hints.clone(),
                    icons: tool.icons.clone(),
                })
            })
            .collect();
        guard.tools = tools;
    }
}

fn flatten(slots: &BTreeMap<String, std::sync::Arc<Mutex<SlotInner>>>) -> Vec<ToolRec> {
    let mut out = Vec::new();
    for slot in slots.values() {
        out.extend(lock_slot(slot).tools.iter().cloned());
    }
    out
}

use crate::skills::clip_chars;

fn clip_err(err: &str) -> String {
    clip_chars(err.trim(), 300)
}

#[cfg(test)]
thread_local! {
    static TEST_TOOLS: std::cell::RefCell<Option<Vec<ToolRec>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_snapshot() -> Option<Vec<ToolRec>> {
    TEST_TOOLS.with(|slot| slot.borrow().clone())
}

#[cfg(test)]
pub(crate) struct TestGuard {
    prev: Option<Vec<ToolRec>>,
}

#[cfg(test)]
impl Drop for TestGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        TEST_TOOLS.with(|slot| *slot.borrow_mut() = prev);
    }
}

#[cfg(test)]
pub(crate) fn install_test_tools(rows: &[(&str, &str, &str, bool)]) -> TestGuard {
    let pairs: Vec<(String, String)> = rows
        .iter()
        .map(|(server, tool, _, _)| ((*server).to_string(), (*tool).to_string()))
        .collect();
    let qualified = names::qualify_all(&pairs);
    let mut tools = Vec::new();
    for item in qualified {
        let read_only = rows
            .iter()
            .find(|(server, tool, _, _)| *server == item.server && *tool == item.tool)
            .map(|(_, _, _, flag)| *flag)
            .unwrap_or(false);
        let description = rows
            .iter()
            .find(|(server, tool, _, _)| *server == item.server && *tool == item.tool)
            .map(|(_, _, description, _)| (*description).to_string())
            .unwrap_or_default();
        tools.push(ToolRec {
            server: item.server,
            raw: item.tool,
            qualified: item.name,
            title: None,
            description,
            schema: json!({"type": "object", "properties": {}}),
            output_schema: None,
            hints: ToolHints {
                read_only,
                ..ToolHints::default()
            },
            icons: Vec::new(),
        });
    }
    install_test_recs(tools)
}

/// Test tools straight from one server's `tools/list` result.
#[cfg(test)]
pub(crate) fn install_test_listing(server: &str, listing: &Value) -> TestGuard {
    let (raw, _) = rpc::parse_tools(listing);
    let pairs: Vec<(String, String)> = raw
        .iter()
        .map(|tool| (server.to_string(), tool.name.clone()))
        .collect();
    let tools = names::qualify_all(&pairs)
        .into_iter()
        .filter_map(|item| {
            let tool = raw.iter().find(|tool| tool.name == item.tool)?.clone();
            Some(ToolRec {
                server: item.server,
                raw: tool.name,
                qualified: item.name,
                title: tool.title,
                description: tool.description,
                schema: tool.input_schema,
                output_schema: tool.output_schema,
                hints: tool.hints,
                icons: tool.icons,
            })
        })
        .collect();
    install_test_recs(tools)
}

#[cfg(test)]
fn install_test_recs(tools: Vec<ToolRec>) -> TestGuard {
    let prev = TEST_TOOLS.with(|slot| slot.borrow_mut().replace(tools));
    TestGuard { prev }
}

#[cfg(test)]
mod tests {
    use super::{
        bare_schema_named_in, begin_turn, chat_schema_tools, import_documents, install_test_tools,
        read_mcp_text, schema_tools, search_output, try_dispatch,
    };
    use crate::gate::{decide, decide_with, mcp_policy_deny, Decision, Gate, PermMode};
    use crate::perm::ConfigGuard;
    use crate::perm::{parse_rule, Action, Policy};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    fn gate(mode: PermMode, readonly: bool, attended: bool) -> Gate {
        Gate {
            mode,
            readonly_session: readonly,
            attended,
            desktop: false,
        }
    }

    fn policy(rules: &[(&str, Action)]) -> Policy {
        Policy {
            rules: rules
                .iter()
                .map(|(text, action)| parse_rule(text, *action).unwrap())
                .collect(),
            grants: Vec::new(),
        }
    }

    fn temp_cfg(label: &str) -> (PathBuf, ConfigGuard) {
        let dir = std::env::temp_dir().join(format!("gh-mcp-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let guard = ConfigGuard::set(&dir);
        (dir, guard)
    }

    #[test]
    fn set_enabled_writes_the_flag_and_doctor_skips_it() {
        let (dir, _guard) = temp_cfg("set-enabled");
        std::fs::write(
            dir.join("mcp.json"),
            r#"{"mcpServers":{"box":{"command":"definitely-not-a-real-binary-xyz"}}}"#,
        )
        .unwrap();
        super::invalidate();
        assert_eq!(
            super::set_enabled("box", false),
            Ok("Turned off box".into())
        );
        let text = std::fs::read_to_string(dir.join("mcp.json")).unwrap();
        assert!(text.contains("\"enabled\": false"), "{text}");
        let rows = super::doctor();
        let row = rows.iter().find(|r| r.name == "box").expect("box row");
        assert_eq!(row.status, "disabled");
        assert_eq!(super::set_enabled("box", true), Ok("Turned on box".into()));
        let text = std::fs::read_to_string(dir.join("mcp.json")).unwrap();
        assert!(!text.contains("enabled"), "{text}");
        assert_eq!(
            super::configured()
                .iter()
                .find(|r| r.name == "box")
                .map(|r| r.status.clone()),
            Some("not checked".into())
        );
        assert_eq!(
            super::set_enabled("nope", false),
            Err("nope isn't one of this cabin's connections".into())
        );
        super::shutdown_all();
        super::invalidate();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn qualify_sanitizes_caps_and_dedupes() {
        let rows = super::names::qualify_all(&[
            ("a b".into(), "t".into()),
            ("a_b".into(), "t".into()),
            ("9box".into(), "echo".into()),
            ("box".into(), "1abc".into()),
            ("read".into(), "file".into()),
            ("x".repeat(80), "y".repeat(80)),
        ]);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert!(names
            .iter()
            .all(|name| name.len() <= super::names::NAME_MAX));
        assert!(names.iter().all(|name| name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')));
        assert!(names.iter().all(|name| name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')));
        assert_eq!(
            names.iter().filter(|name| name.contains("__")).count(),
            names.len()
        );
        assert!(names.contains(&"s9box__echo"));
        assert!(names.contains(&"box__1abc"));
        assert!(names.contains(&"read__file"));
        assert!(names
            .iter()
            .any(|name| *name == "a_b__t" || name.starts_with("a_b__t_")));
        assert_eq!(
            names.len(),
            names.iter().collect::<std::collections::HashSet<_>>().len()
        );
        assert!(!names
            .iter()
            .any(|name| *name == "search_tool" || *name == "use_tool" || *name == "read_file"));
    }

    #[test]
    fn import_copies_server_definitions_only() {
        let (dir, _guard) = temp_cfg("import");
        let doc = r#"{
            "token": "super-secret-token",
            "access_token": "nope",
            "mcpServers": {
                "echo": {"command":"echo","args":["hi"],"env":{"VISIBLE":"ok","API_TOKEN":"in-entry"},"access_token":"drop-me"},
                "web": {"url":"http://127.0.0.1:9/mcp","headers":{"Authorization":"Bearer in-entry"}}
            }
        }"#;
        let report = import_documents(&[doc.to_string()]).unwrap();
        assert_eq!(report.added, vec!["echo".to_string(), "web".to_string()]);
        let saved = std::fs::read_to_string(dir.join("mcp.json")).unwrap();
        assert!(saved.contains("echo"));
        assert!(saved.contains("VISIBLE"));
        assert!(saved.contains("API_TOKEN"));
        assert!(saved.contains("in-entry"));
        assert!(saved.contains("Authorization"));
        assert!(!saved.contains("super-secret-token"));
        assert!(!saved.contains("nope"));
        assert!(!saved.contains("drop-me"));
        let again = import_documents(&[doc.to_string()]).unwrap();
        assert!(again.added.is_empty());
        assert_eq!(again.skipped.len(), 2);
        let cred = dir.join(concat!("auth", ".json"));
        std::fs::write(&cred, "marker").unwrap();
        let err = read_mcp_text(&cred).unwrap_err();
        assert!(err.contains("refusing"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    const FAKE_KEY: &str = "sk-abcdefghijklmnopqrstuv";

    /// A loopback server that keeps each request's `Authorization` value and
    /// answers 500.
    struct AuthSeen {
        port: u16,
        seen: std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl AuthSeen {
        fn start() -> Self {
            use std::io::{BufRead, Read, Write};
            use std::sync::atomic::Ordering;
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (seen2, stop2) = (seen.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop2.load(Ordering::SeqCst) {
                    let Ok((mut sock, _)) = listener.accept() else {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    };
                    let _ = sock.set_nonblocking(false);
                    let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(3)));
                    let mut reader = std::io::BufReader::new(sock.try_clone().unwrap());
                    let (mut auth, mut len) = (None, 0usize);
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                            break;
                        }
                        if let Some((name, value)) = line.split_once(':') {
                            match name.trim().to_ascii_lowercase().as_str() {
                                "authorization" => auth = Some(value.trim().to_string()),
                                "content-length" => len = value.trim().parse().unwrap_or(0),
                                _ => {}
                            }
                        }
                    }
                    let mut body = vec![0u8; len];
                    let _ = reader.read_exact(&mut body);
                    seen2.lock().unwrap().push(auth);
                    let _ = sock.write_all(
                        b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
            });
            Self { port, seen, stop }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}/mcp", self.port)
        }

        /// Each different `Authorization` value seen, `None` for a request without one.
        fn auth_values(&self) -> Vec<Option<String>> {
            let mut all = self.seen.lock().unwrap().clone();
            all.sort();
            all.dedup();
            all
        }
    }

    impl Drop for AuthSeen {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn sealed_cfg(label: &str) -> (PathBuf, ConfigGuard) {
        let dir = crate::harness::test_dir(label);
        let guard = ConfigGuard::set(&dir);
        crate::harness::seal_connection_token(&dir, "github", FAKE_KEY).unwrap();
        super::invalidate();
        (dir, guard)
    }

    #[test]
    fn an_imported_entry_cannot_name_a_saved_key() {
        let (dir, _guard) = sealed_cfg("mcp-import-token-ref");
        let server = AuthSeen::start();
        let doc = json!({"mcpServers": {
            "weather": {"url": server.url(), "tokenRef": "github"},
            "notes": {"command": "notes-mcp", "tokenRef": "github", "tokenEnv": "NOTES_KEY", "enabled": false}
        }});
        let report = import_documents(&[doc.to_string()]).unwrap();
        assert_eq!(
            report.added,
            vec!["notes".to_string(), "weather".to_string()]
        );
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("mcp.json")).unwrap()).unwrap();
        assert_eq!(
            saved["mcpServers"]["weather"],
            json!({"url": server.url(), "type": "http"})
        );
        assert_eq!(
            saved["mcpServers"]["notes"],
            json!({"command": "notes-mcp", "enabled": false})
        );
        super::doctor();
        assert_eq!(server.auth_values(), vec![None]);
        super::shutdown_all();
        super::invalidate();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entry_that_names_another_connectors_key_is_not_opened() {
        let (dir, _guard) = sealed_cfg("mcp-other-token-ref");
        let server = AuthSeen::start();
        let doc = json!({"mcpServers": {
            "weather": {"url": server.url(), "tokenRef": "github"},
            "notes": {"command": "definitely-not-a-real-binary-xyz", "tokenRef": "github", "tokenEnv": "NOTES_KEY"}
        }});
        std::fs::write(dir.join("mcp.json"), doc.to_string()).unwrap();
        super::invalidate();
        let rows = super::doctor();
        let row = |name: &str| rows.iter().find(|r| r.name == name).expect("row");
        assert_eq!(row("weather").status, "error");
        assert_eq!(
            row("weather").last_error,
            "weather names another connector's key, so it was not used: save a key for weather in Settings, Connectors"
        );
        assert_eq!(
            row("notes").last_error,
            "notes names another connector's key, so it was not used: save a key for notes in Settings, Connectors"
        );
        assert_eq!(server.auth_values(), Vec::<Option<String>>::new());
        super::shutdown_all();
        super::invalidate();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entry_sends_the_key_saved_under_its_own_name() {
        let (dir, _guard) = sealed_cfg("mcp-own-token-ref");
        let server = AuthSeen::start();
        let doc = json!({"mcpServers": {"github": {"url": server.url(), "tokenRef": "github"}}});
        std::fs::write(dir.join("mcp.json"), doc.to_string()).unwrap();
        super::invalidate();
        super::doctor();
        assert_eq!(
            server.auth_values(),
            vec![Some("Bearer sk-abcdefghijklmnopqrstuv".to_string())]
        );
        super::shutdown_all();
        super::invalidate();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcptool_rules_deny_ask_allow_and_unattended_denies() {
        let _tools = install_test_tools(&[
            ("box", "echo", "Echo", false),
            ("box", "peek", "Peek", true),
        ]);
        let ws = Path::new(".");
        let deny = policy(&[
            ("MCPTool(box__echo)", Action::Deny),
            ("MCPTool(box__*)", Action::Allow),
        ]);
        let decision = decide_with(
            &gate(PermMode::Ask, false, true),
            "box__echo",
            "{}",
            false,
            None,
            ws,
            Some(&deny),
        );
        assert!(matches!(decision, Decision::Refuse(ref msg) if msg.contains("deny rule on mcp")));

        let ask = policy(&[("MCPTool(box__echo)", Action::Ask)]);
        // Always Allow runs past an ask rule (only hard classes stop it); Supervised asks.
        assert_eq!(
            decide_with(
                &gate(PermMode::Always, false, true),
                "box__echo",
                "{}",
                true,
                None,
                ws,
                Some(&ask)
            ),
            Decision::Run
        );
        assert_eq!(
            decide_with(
                &gate(PermMode::Ask, false, true),
                "box__echo",
                "{}",
                false,
                None,
                ws,
                Some(&ask)
            ),
            Decision::Ask
        );
        let away = decide_with(
            &gate(PermMode::Ask, false, false),
            "box__echo",
            "{}",
            false,
            None,
            ws,
            Some(&ask),
        );
        assert!(
            matches!(away, Decision::Refuse(ref msg) if msg.contains("deny rule on mcp") && !msg.contains("deny rule on edit"))
        );

        let allow = policy(&[("MCPTool(box__echo)", Action::Allow)]);
        assert_eq!(
            decide_with(
                &gate(PermMode::Ask, false, false),
                "box__echo",
                "{}",
                false,
                None,
                ws,
                Some(&allow)
            ),
            Decision::Run
        );

        let open = decide_with(
            &gate(PermMode::Ask, false, true),
            "box__echo",
            "{}",
            false,
            None,
            ws,
            None,
        );
        assert_eq!(open, Decision::Ask);
        let unattended = decide_with(
            &gate(PermMode::Ask, false, false),
            "box__echo",
            "{}",
            false,
            None,
            ws,
            None,
        );
        assert!(
            matches!(unattended, Decision::Refuse(ref msg) if msg.contains("deny rule on mcp"))
        );
        assert_eq!(
            decide_with(
                &gate(PermMode::Always, false, false),
                "box__echo",
                "{}",
                false,
                None,
                ws,
                None
            ),
            Decision::Run
        );
        let plan = decide_with(
            &gate(PermMode::Ask, true, true),
            "box__echo",
            "{}",
            false,
            None,
            ws,
            Some(&allow),
        );
        assert!(matches!(plan, Decision::Refuse(ref msg) if msg.contains("read-only")));

        assert_eq!(
            decide(
                &gate(PermMode::Ask, false, false),
                "search_tool",
                false,
                None
            ),
            Decision::Run
        );
        let (dir, _guard) = temp_cfg("empty-use");
        let unknown = decide_with(
            &gate(PermMode::Ask, false, true),
            "use_tool",
            "{}",
            false,
            None,
            ws,
            None,
        );
        assert!(matches!(unknown, Decision::Refuse(ref msg) if msg.contains("unknown tool")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn rich_listing() -> serde_json::Value {
        json!({"tools": [
            {
                "name": "rebuild",
                "title": "Rebuild Index",
                "description": "Rebuild the search index",
                "inputSchema": {"type": "object"},
                "annotations": {"destructiveHint": true, "idempotentHint": false, "openWorldHint": false},
                "icons": [{"src": "https://example.com/rebuild.png", "mimeType": "image/png"}, {"src": "http://example.com/x.png"}]
            },
            {
                "name": "list",
                "description": "List indexes",
                "inputSchema": {"type": "object"},
                "outputSchema": {"type": "object"},
                "annotations": {"readOnlyHint": true, "destructiveHint": false}
            }
        ]})
    }

    #[test]
    fn destructive_hint_asks_under_always_and_refuses_unattended() {
        let _tools = super::install_test_listing("box", &rich_listing());
        let ws = Path::new(".");
        let decide = |mode, attended, latched, policy: Option<&Policy>| {
            decide_with(
                &gate(mode, false, attended),
                "box__rebuild",
                "{}",
                latched,
                None,
                ws,
                policy,
            )
        };
        let away = Decision::Refuse(
            "Tool `box__rebuild` was not executed: its server marks it destructive, and nobody is here to approve it"
                .into(),
        );
        assert_eq!(decide(PermMode::Always, true, false, None), Decision::Ask);
        assert_eq!(decide(PermMode::Ask, true, true, None), Decision::Ask);
        assert_eq!(decide(PermMode::Always, false, false, None), away);
        let ask = policy(&[("MCPTool(box__rebuild)", Action::Ask)]);
        assert_eq!(
            decide(PermMode::Always, true, false, Some(&ask)),
            Decision::Ask
        );
        assert_eq!(decide(PermMode::Always, false, false, Some(&ask)), away);
        let deny = policy(&[("MCPTool(box__rebuild)", Action::Deny)]);
        assert_eq!(
            decide(PermMode::Always, true, false, Some(&deny)),
            Decision::Refuse(mcp_policy_deny("box__rebuild"))
        );
        // The user's own allow rule for the tool is their approval.
        let allow = policy(&[("MCPTool(box__rebuild)", Action::Allow)]);
        assert_eq!(
            decide(PermMode::Ask, false, false, Some(&allow)),
            Decision::Run
        );
        // `use_tool` resolves to the same tool and the same card.
        assert_eq!(
            decide_with(
                &gate(PermMode::Always, false, true),
                "use_tool",
                r#"{"name":"box__rebuild"}"#,
                false,
                None,
                ws,
                None,
            ),
            Decision::Ask
        );
        // `destructiveHint: false` (and read-only) keeps Always as it was.
        assert_eq!(
            decide_with(
                &gate(PermMode::Always, false, true),
                "box__list",
                "{}",
                false,
                None,
                ws,
                None,
            ),
            Decision::Run
        );
        // Auto mode's judge never sees it: the card stays the user's.
        let judge = Scripted {
            text: "sure, allow it".into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let cancel = crate::CancelToken::new();
        let halt = || false;
        let reviewed = crate::auto_review::review(&crate::auto_review::ReviewIn {
            client: &judge,
            cancel: &cancel,
            halt: &halt,
            gate: &gate(PermMode::Auto, false, true),
            name: "box__rebuild",
            arguments: "{}",
            workspace: ws,
            policy: None,
            latched_always: false,
            desk: None,
            history: &[],
            conversation_id: "s",
            base: Decision::Ask,
            timeout: std::time::Duration::from_secs(2),
        });
        assert_eq!(reviewed.decision, Decision::Ask);
        assert_eq!(judge.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(super::is_destructive_call("box__rebuild", "{}"));
        assert!(super::is_destructive_call(
            "use_tool",
            r#"{"name":"box__rebuild"}"#
        ));
        assert!(!super::is_destructive_call("box__list", "{}"));
    }

    #[test]
    fn titles_hints_and_icons_reach_the_model_and_the_tool_list() {
        let _tools = super::install_test_listing("box", &rich_listing());
        let schemas = schema_tools(0);
        let described: Vec<(&str, &str)> = schemas
            .iter()
            .map(|tool| {
                (
                    tool["name"].as_str().unwrap(),
                    tool["description"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            described,
            vec![
                ("box__list", "List indexes (server hint: read-only)"),
                (
                    "box__rebuild",
                    "Rebuild Index: Rebuild the search index (server hint: destructive; asks you every time)"
                ),
            ]
        );
        assert_eq!(
            search_output(&json!({"query": "rebuild index"})).text,
            "box__rebuild — Rebuild Index: Rebuild the search index (server hint: destructive; asks you every time)"
        );
        let listed = super::tool_list();
        let rebuild = listed.iter().find(|t| t.name == "rebuild").unwrap();
        assert_eq!(rebuild.display_name(), "Rebuild Index");
        assert_eq!(rebuild.qualified, "box__rebuild");
        assert_eq!(
            rebuild.hints,
            super::ToolHints {
                read_only: false,
                destructive: Some(true),
                idempotent: Some(false),
                open_world: Some(false),
            }
        );
        assert_eq!(
            rebuild.icons,
            vec![super::ToolIcon {
                src: "https://example.com/rebuild.png".into(),
                mime_type: Some("image/png".into()),
                sizes: Vec::new(),
            }]
        );
        let list = listed.iter().find(|t| t.name == "list").unwrap();
        assert_eq!(list.display_name(), "list");
        assert_eq!(list.output_schema, Some(json!({"type": "object"})));
    }

    #[test]
    fn a_chats_mcp_tools_hold_for_the_turn_and_new_ones_join_at_the_end() {
        let names = |tools: &[serde_json::Value]| -> Vec<String> {
            tools
                .iter()
                .map(|tool| tool["name"].as_str().unwrap_or_default().to_string())
                .collect()
        };
        let session = format!("frozen-{}", std::process::id());
        let first = install_test_tools(&[
            ("box", "peek", "Peek", true),
            ("box", "poke", "Poke", false),
        ]);
        let turn_one = chat_schema_tools(&session, 0);
        assert_eq!(names(&turn_one), ["box__peek", "box__poke"]);
        drop(first);
        // A server connects mid-turn and one tool goes: the list holds.
        let _later = install_test_tools(&[
            ("aaa", "ask", "Ask", true),
            ("box", "peek", "Peek again", true),
        ]);
        assert_eq!(chat_schema_tools(&session, 0), turn_one);
        // Next user turn: kept tools keep their place, the new one joins last.
        begin_turn(&session);
        let turn_two = chat_schema_tools(&session, 0);
        assert_eq!(names(&turn_two), ["box__peek", "aaa__ask"]);
        assert!(turn_two[0]["description"]
            .as_str()
            .is_some_and(|d| d.contains("Peek again")));
        // No chat: the live list.
        assert_eq!(names(&chat_schema_tools("", 0)), ["aaa__ask", "box__peek"]);
    }

    #[test]
    fn a_tool_the_provider_refuses_is_offered_bare_for_the_rest_of_the_chat() {
        let session = format!("bare-{}", std::process::id());
        let _tools = install_test_tools(&[
            ("box", "peek", "Peek", true),
            ("box", "poke", "Poke", false),
        ]);
        chat_schema_tools(&session, 0);
        let strict = json!({"type": "object", "properties": {"n": {"type": "integer"}}});
        for tool in &mut super::frozen_chats().get_mut(&session).unwrap().schemas {
            tool["parameters"] = strict.clone();
        }
        assert!(!bare_schema_named_in(&session, "HTTP 400: bad request"));
        assert!(bare_schema_named_in(
            &session,
            "HTTP 400: invalid parameters schema for function box__poke"
        ));
        // Named again: nothing new to strip, so no second retry.
        assert!(!bare_schema_named_in(&session, "HTTP 400: box__poke"));
        let params = |tools: &[serde_json::Value], name: &str| {
            tools
                .iter()
                .find(|t| t["name"] == name)
                .map(|t| t["parameters"].clone())
                .unwrap()
        };
        let now = chat_schema_tools(&session, 0);
        assert_eq!(params(&now, "box__peek"), strict);
        assert_eq!(
            params(&now, "box__poke"),
            json!({"type": "object", "properties": {}})
        );
        // A new user turn rebuilds the list; the refused tool stays bare.
        begin_turn(&session);
        let next = chat_schema_tools(&session, 0);
        assert_eq!(
            params(&next, "box__poke"),
            json!({"type": "object", "properties": {}})
        );
        assert!(!bare_schema_named_in("no-such-chat", "box__poke"));
    }

    #[test]
    fn read_only_hint_does_not_skip_the_gate() {
        let _tools = install_test_tools(&[("box", "peek", "Peek a file", true)]);
        let ws = Path::new(".");
        let asked = decide_with(
            &gate(PermMode::Ask, false, true),
            "box__peek",
            "{}",
            false,
            None,
            ws,
            None,
        );
        assert_eq!(asked, Decision::Ask);
        let allow = policy(&[("MCPTool(box__peek)", Action::Allow)]);
        assert_eq!(
            decide_with(
                &gate(PermMode::Ask, false, false),
                "box__peek",
                "{}",
                false,
                None,
                ws,
                Some(&allow)
            ),
            Decision::Run
        );
        let schemas = schema_tools(0);
        let hint = schemas
            .iter()
            .find(|tool| tool["name"] == "box__peek")
            .unwrap();
        assert!(hint["description"].as_str().unwrap().contains("read-only"));
    }

    #[test]
    fn deferred_discovery_switches_above_forty() {
        let mut rows = Vec::new();
        for n in 0..40 {
            rows.push((
                "box",
                format!("t{n}"),
                if n == 0 { "widget alpha" } else { "other" },
                false,
            ));
        }
        let owned: Vec<(&str, String, &str, bool)> = rows
            .iter()
            .map(|(server, tool, description, flag)| (*server, tool.clone(), *description, *flag))
            .collect();
        // install_test_tools wants &str. Leak the formatted names for the test process.
        let leaked: Vec<(&str, &str, &str, bool)> = owned
            .iter()
            .map(|(server, tool, description, flag)| (*server, tool.as_str(), *description, *flag))
            .collect();
        {
            let _guard = install_test_tools(&leaked);
            let schemas = schema_tools(0);
            assert!(schemas.iter().any(|tool| tool["name"] == "box__t0"));
            assert!(schemas
                .iter()
                .all(|tool| tool["name"] != "search_tool" && tool["name"] != "use_tool"));
            assert_eq!(schemas.len(), 40);
            let found = search_output(&json!({"query": "widget"}));
            assert!(found.text.contains("box__t0"));
            assert!(found.text.contains("widget alpha"));
        }
        let mut more = leaked.clone();
        let extra = "t40".to_string();
        more.push(("box", extra.as_str(), "widget beta", false));
        let _guard = install_test_tools(&more);
        let schemas = schema_tools(0);
        assert_eq!(schemas.len(), 2);
        assert!(schemas.iter().any(|tool| tool["name"] == "search_tool"));
        assert!(schemas.iter().any(|tool| tool["name"] == "use_tool"));
        assert!(schemas.iter().all(|tool| tool["name"] != "box__t0"));
        let found = search_output(&json!({"query": "beta"}));
        assert!(found.text.contains("box__t40"));
        let ws = Path::new(".");
        let missing = decide_with(
            &gate(PermMode::Ask, false, true),
            "use_tool",
            r#"{"name":"box__missing"}"#,
            false,
            None,
            ws,
            None,
        );
        assert!(
            matches!(missing, Decision::Refuse(ref msg) if msg.contains("unknown tool `box__missing`"))
        );
        let allow = policy(&[("MCPTool(box__t0)", Action::Allow)]);
        assert_eq!(
            decide_with(
                &gate(PermMode::Ask, false, false),
                "use_tool",
                r#"{"name":"box__t0","arguments":{"q":"1"}}"#,
                false,
                None,
                ws,
                Some(&allow),
            ),
            Decision::Run
        );
        let called = try_dispatch(
            "use_tool",
            &json!({"name": "box__t0", "arguments": {"q": "1"}}),
            &|| false,
        )
        .unwrap();
        assert_eq!(called.text, "test-call box__t0");
        let denied = decide_with(
            &gate(PermMode::Ask, false, false),
            "use_tool",
            r#"{"name":"box__t40"}"#,
            false,
            None,
            ws,
            None,
        );
        assert!(matches!(denied, Decision::Refuse(ref msg) if msg.contains("deny rule on mcp")));
    }

    /// Spike-2b: the 40 counts the native desktop tools and MCP tools together,
    /// and a deferred hard tool still parks through `harness::decide`.
    #[test]
    fn native_desktop_and_mcp_tools_pass_forty_together_and_hard_use_tool_parks() {
        // Full tool list: the mode this test checks.
        let _legacy = crate::tools::registry::LegacyPromptGuard::set();
        let desk_on = Gate {
            desktop: true,
            ..gate(PermMode::Always, false, true)
        };
        let desk_off = gate(PermMode::Always, false, true);
        let native = {
            let _none = install_test_tools(&[]);
            crate::tools::schemas_for(&desk_on).len() - crate::tools::schemas_for(&desk_off).len()
        };
        assert_eq!(native, 7, "desktop tools the native engine registers");
        let names: Vec<String> = (0..41 - native).map(|n| format!("t{n}")).collect();
        let mut rows: Vec<(&str, &str, &str, bool)> = names
            .iter()
            .map(|n| ("box", n.as_str(), "other", false))
            .collect();
        rows[0] = ("mail", "send_message", "Send an email", false);
        let _guard = install_test_tools(&rows);
        assert_eq!(rows.len() + native, 41);
        let on = crate::tools::schemas_for(&desk_on);
        assert!(
            on.iter().any(|t| t["name"] == "search_tool")
                && on.iter().any(|t| t["name"] == "use_tool")
        );
        assert!(on.iter().all(|t| t["name"] != "box__t1"));
        assert!(
            on.iter().any(|t| t["name"] == "click"),
            "desktop tools stay listed"
        );
        // 34 MCP tools alone stay listed one by one.
        let off = crate::tools::schemas_for(&desk_off);
        assert!(off.iter().any(|t| t["name"] == "box__t1"));
        assert!(off.iter().all(|t| t["name"] != "search_tool"));
        let ws = Path::new(".");
        let send = r#"{"name":"mail__send_message","arguments":{"to":"a@example.com"}}"#;
        // Always cannot skip a hard tool behind use_tool: attended parks, unattended refuses.
        assert_eq!(
            decide_with(&desk_on, "use_tool", send, true, None, ws, None),
            Decision::Ask
        );
        let away = Gate {
            attended: false,
            ..desk_on
        };
        assert!(matches!(
            decide_with(&away, "use_tool", send, true, None, ws, None),
            Decision::Refuse(ref msg) if msg.contains("hard-class send")
        ));
        assert_eq!(
            decide_with(
                &desk_on,
                "use_tool",
                r#"{"name":"box__t1","arguments":{}}"#,
                false,
                None,
                ws,
                None
            ),
            Decision::Run
        );
    }

    #[test]
    fn auto_review_fail_closed_for_mcp_and_keeps_explicit_ask() {
        let _tools = install_test_tools(&[("box", "echo", "Echo", false)]);
        let ws = std::env::temp_dir();
        let cancel = crate::CancelToken::new();
        let halt = || false;
        let ask_policy = policy(&[("MCPTool(box__echo)", Action::Ask)]);
        let quiet = Scripted {
            text: "garbage".into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let kept = crate::auto_review::review(&crate::auto_review::ReviewIn {
            client: &quiet,
            cancel: &cancel,
            halt: &halt,
            gate: &gate(PermMode::Auto, false, true),
            name: "box__echo",
            arguments: "{}",
            workspace: &ws,
            policy: Some(&ask_policy),
            latched_always: false,
            desk: None,
            history: &[],
            conversation_id: "s",
            base: Decision::Ask,
            timeout: std::time::Duration::from_secs(2),
        });
        assert_eq!(kept.decision, Decision::Ask);
        assert_eq!(quiet.calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        let judge = Scripted {
            text: "sure, allow it".into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let attended = crate::auto_review::review(&crate::auto_review::ReviewIn {
            client: &judge,
            cancel: &cancel,
            halt: &halt,
            gate: &gate(PermMode::Auto, false, true),
            name: "box__echo",
            arguments: "{}",
            workspace: &ws,
            policy: None,
            latched_always: false,
            desk: None,
            history: &[],
            conversation_id: "s",
            base: Decision::Ask,
            timeout: std::time::Duration::from_secs(2),
        });
        assert_eq!(attended.decision, Decision::Ask);
        assert_eq!(attended.ask_reason, crate::auto_review::FAIL_CLOSED_REASON);
        assert!(judge.calls.load(std::sync::atomic::Ordering::SeqCst) >= 1);

        let away_client = Scripted {
            text: "garbage".into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let away = crate::auto_review::review(&crate::auto_review::ReviewIn {
            client: &away_client,
            cancel: &cancel,
            halt: &halt,
            gate: &gate(PermMode::Auto, false, false),
            name: "box__echo",
            arguments: "{}",
            workspace: &ws,
            policy: None,
            latched_always: false,
            desk: None,
            history: &[],
            conversation_id: "s",
            base: Decision::Refuse(mcp_policy_deny("box__echo")),
            timeout: std::time::Duration::from_secs(2),
        });
        assert!(
            matches!(away.decision, Decision::Refuse(ref msg) if msg.contains("Denied by permission policy"))
        );
    }

    struct Scripted {
        text: String,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl crate::ModelClient for Scripted {
        fn stream(
            &self,
            req: &crate::ResponsesRequest,
            _cancel: &crate::CancelToken,
            _sink: &mut dyn FnMut(crate::StreamEvent),
        ) -> Result<crate::TurnOutput, crate::ClientError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let joined = format!("{req:?}");
            assert!(joined.contains(crate::auto_review::JUDGE_MARK) || req.input.iter().any(|item| {
                matches!(item, crate::InputItem::Message { content, .. } if content.iter().any(|part| {
                    matches!(part, crate::ContentPart::InputText(text) if text.contains(crate::auto_review::JUDGE_MARK))
                }))
            }));
            Ok(crate::TurnOutput {
                text: self.text.clone(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: crate::Usage::default(),
                ..Default::default()
            })
        }
    }

    #[test]
    fn a_signed_in_server_connects_refreshes_once_on_401_and_signs_out() {
        use super::{
            configured, doctor, invalidate, oauth, restart, shutdown_all, sign_in, sign_out, SignIn,
        };
        let dir = crate::harness::test_dir("mcp-oauth-slots");
        let _guard = ConfigGuard::set(&dir);
        let server = oauth::tests::AuthMcp::start();
        let doc = format!(
            r#"{{"mcpServers": {{"linear": {{"url": "{}", "type": "http"}}, "kept": {{"url": "{}", "bearerToken": "sk-abcdefghijklmnopqrstuv"}}}}}}"#,
            server.url(),
            server.url()
        );
        std::fs::write(dir.join("mcp.json"), doc).unwrap();
        invalidate();
        let state = |name: &str| {
            configured()
                .into_iter()
                .find(|r| r.name == name)
                .map(|r| r.sign_in)
        };
        assert_eq!(state("linear"), Some(SignIn::SignedOut));
        assert_eq!(
            state("kept"),
            Some(SignIn::NotOffered),
            "its entry carries its own credentials"
        );
        let err = restart("linear").unwrap_err();
        assert!(
            err.contains("HTTP 401") && err.ends_with("Sign in on its row under Native MCP"),
            "{err}"
        );

        let line = sign_in("linear", &oauth::tests::approving_browser("good-code")).unwrap();
        assert_eq!(line, "Signed in to linear");
        assert_eq!(
            state("linear"),
            Some(SignIn::SignedIn("Signed in to linear".into()))
        );
        let row = doctor().into_iter().find(|r| r.name == "linear").unwrap();
        assert_eq!((row.status.as_str(), row.tool_count), ("connected", 1));

        // The server stops taking at-1: one refresh, and the reconnect succeeds.
        *server.live.lock().unwrap() = "at-2".into();
        restart("linear").unwrap();
        assert_eq!(
            server.refreshes.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(oauth::load("linear").unwrap().access_token, "at-2");

        assert_eq!(sign_out("linear"), "Signed out of linear");
        assert_eq!(state("linear"), Some(SignIn::SignedOut));
        assert!(restart("linear")
            .unwrap_err()
            .ends_with("Sign in on its row under Native MCP"));
        shutdown_all();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn offline_only_pauses_third_party_servers_and_keeps_the_apps_own() {
        use super::{doctor, invalidate, offline_paused, shutdown_all};
        const MSG: &str = "Offline only is on, so Loadout can't reach the internet. Turn it off in Settings → AI models to use this.";
        let exe = Path::new("/opt/loadout/grokhub");
        let cua = vec!["--mcp-cua".to_string()];
        let npx = vec!["-y".to_string(), "weather-mcp".to_string()];
        assert_eq!(
            offline_paused("grokhub-cua", "/opt/loadout/grokhub", &cua, Some(exe), true),
            None
        );
        assert_eq!(
            offline_paused("weather", "npx", &npx, Some(exe), true),
            Some(MSG.to_string())
        );
        assert_eq!(
            offline_paused("grokhub-cua", "/tmp/other", &cua, Some(exe), true),
            Some(MSG.to_string()),
            "the name alone doesn't make a server the app's own"
        );
        assert_eq!(
            offline_paused("weather", "npx", &npx, Some(exe), false),
            None
        );

        let dir = crate::harness::test_dir("mcp-offline-only");
        let _guard = ConfigGuard::set(&dir);
        let doc = r#"{"mcpServers": {
            "weather": {"command": "definitely-not-a-real-mcp-binary", "args": ["--stdio"]},
            "remote": {"url": "https://mcp.example.com/mcp", "type": "http"}
        }}"#;
        std::fs::write(dir.join("mcp.json"), doc).unwrap();
        invalidate();
        let _on = grokhub_core::offline::force_for_thread(true);
        let rows: Vec<(String, String, String)> = doctor()
            .into_iter()
            .map(|r| (r.name, r.status, r.last_error))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("remote".to_string(), "paused".to_string(), MSG.to_string()),
                ("weather".to_string(), "paused".to_string(), MSG.to_string()),
            ]
        );
        shutdown_all();
        let _ = std::fs::remove_dir_all(dir);
    }
}
