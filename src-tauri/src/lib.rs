//! GrokHub's Tauri desktop shell: chat, settings and tray (cross-platform
//! recode, Phase 2). The egui cabin in `crates/grokhub-app` keeps shipping
//! until this shell reaches parity.

#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod agent;
pub mod automations;
pub mod capabilities;
pub mod connectors;
pub mod credential;
pub mod journal;
pub mod local_ai;
pub mod memory;
pub mod pulse;
pub mod remote;
pub mod settings;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_agent::{AuthKind, XaiClient};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use settings::ShellSettings;
#[cfg(desktop)]
use tauri::menu::{Menu, MenuItem};
#[cfg(desktop)]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Listener, Manager, State, WindowEvent};

pub const CHAT_EVENT: &str = "chat://event";

struct ShellState {
    config_dir: PathBuf,
    settings: Mutex<ShellSettings>,
    chats: Mutex<HashMap<String, Arc<agent::Session>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub model: String,
    pub close_to_tray: bool,
    pub has_api_key: bool,
    /// The folder picked in Settings, empty for the default.
    pub workspace: String,
    /// The folder the agent actually works in.
    pub workspace_path: String,
    pub approvals: settings::Approvals,
    pub desktop_control: bool,
    /// Settings → About.
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatEvent<'a> {
    conversation_id: &'a str,
    event: &'a agent::UiEvent,
}

fn view(settings: &ShellSettings) -> SettingsView {
    SettingsView {
        model: settings.model.clone(),
        close_to_tray: settings.close_to_tray,
        has_api_key: credential::has_key(),
        workspace: settings.workspace.clone(),
        workspace_path: agent::workspace_for(&settings.workspace)
            .display()
            .to_string(),
        approvals: settings.approvals,
        desktop_control: settings.desktop_control,
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn current(state: &ShellState) -> ShellSettings {
    state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
fn get_settings(state: State<'_, ShellState>) -> SettingsView {
    view(&current(&state))
}

#[tauri::command]
fn save_settings(
    state: State<'_, ShellState>,
    model: String,
    close_to_tray: bool,
    workspace: String,
    approvals: settings::Approvals,
    desktop_control: bool,
) -> Result<SettingsView, String> {
    let caps = capabilities::current();
    let desktop_control = desktop_control && caps.desktop_control;
    let close_to_tray = close_to_tray && caps.tray;
    let next = ShellSettings {
        model: model.trim().to_string(),
        close_to_tray,
        workspace: workspace.trim().to_string(),
        approvals,
        desktop_control,
    };
    settings::save(&state.config_dir, &next)?;
    #[cfg(desktop)]
    if current(&state).desktop_control != desktop_control {
        grokhub_desktop::set_desktop_enabled(desktop_control);
    }
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(view(&next))
}

#[tauri::command]
fn set_api_key(state: State<'_, ShellState>, key: String) -> Result<SettingsView, String> {
    credential::save_key(&key)?;
    Ok(view(&current(&state)))
}

#[tauri::command]
fn clear_api_key(state: State<'_, ShellState>) -> Result<SettingsView, String> {
    credential::delete_key()?;
    Ok(view(&current(&state)))
}

fn session(state: &ShellState, conversation_id: &str) -> Arc<agent::Session> {
    let mut chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
    let session = chats.entry(conversation_id.to_string()).or_insert_with(|| {
        Arc::new(agent::Session::spawn(
            conversation_id,
            state.config_dir.clone(),
        ))
    });
    Arc::clone(session)
}

fn route(settings: &ShellSettings, session: agent::SessionMode) -> Result<agent::Route, String> {
    let client = match credential::test_responses_url() {
        Some(url) => XaiClient::new(
            credential::LOOPBACK_TEST_BEARER,
            AuthKind::ApiKey,
            Duration::from_secs(120),
        )
        .with_loopback_url(&url)?,
        None => XaiClient::new(
            credential::load_key()?,
            AuthKind::ApiKey,
            Duration::from_secs(120),
        ),
    };
    Ok(agent::Route {
        client: Arc::new(client),
        auth_kind: AuthKind::ApiKey,
        model: settings.model.clone(),
        workspace: agent::workspace_for(&settings.workspace),
        mode: settings.approvals.mode(),
        desktop: settings.desktop_control && capabilities::current().desktop_control,
        session,
    })
}

/// Runs one prompt through the agent loop. Text, tool cards and permission
/// asks arrive as `chat://event`; this resolves with the stop reason.
#[tauri::command]
async fn send_chat(
    app: AppHandle,
    state: State<'_, ShellState>,
    conversation_id: String,
    text: String,
    mode: Option<agent::SessionMode>,
) -> Result<String, String> {
    let mode = mode.unwrap_or_default();
    chat_turn(app, &state, conversation_id, text, mode, current(&state)).await
}

async fn chat_turn(
    app: AppHandle,
    state: &ShellState,
    conversation_id: String,
    text: String,
    mode: agent::SessionMode,
    settings: ShellSettings,
) -> Result<String, String> {
    let session = session(state, &conversation_id);
    tauri::async_runtime::spawn_blocking(move || {
        let route = route(&settings, mode)?;
        let id = conversation_id.clone();
        session.prompt(
            &text,
            route,
            Box::new(move |event| {
                let _ = app.emit(
                    CHAT_EVENT,
                    ChatEvent {
                        conversation_id: &id,
                        event: &event,
                    },
                );
            }),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn answer_permission(
    state: State<'_, ShellState>,
    conversation_id: String,
    call_id: String,
    answer: String,
) -> Result<(), String> {
    let answer = agent::parse_answer(&answer)?;
    let chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
    let session = chats
        .get(&conversation_id)
        .ok_or_else(|| "That chat is closed.".to_string())?;
    session.answer(&call_id, answer);
    Ok(())
}

/// Answers a plan card, a question or a connector form in an open chat.
#[tauri::command]
fn answer_elicit(
    state: State<'_, ShellState>,
    conversation_id: String,
    id: String,
    action: String,
    field: Option<String>,
    value: Option<String>,
) -> Result<(), String> {
    if !matches!(action.as_str(), "accept" | "decline" | "cancel") {
        return Err(format!("unknown answer: {action}"));
    }
    let chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
    let session = chats
        .get(&conversation_id)
        .ok_or_else(|| "That chat is closed.".to_string())?;
    session.answer_elicit(
        &id,
        &action,
        field.as_deref(),
        value.as_deref().unwrap_or(""),
    );
    Ok(())
}

#[tauri::command]
fn cancel_chat(state: State<'_, ShellState>, conversation_id: String) {
    if let Some(session) = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
    {
        session.cancel();
    }
}

/// New chat: the old conversation's worker ends.
#[tauri::command]
fn close_chat(state: State<'_, ShellState>, conversation_id: String) {
    let removed = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&conversation_id);
    if let Some(session) = removed {
        session.cancel();
    }
}

fn now_clock(clock: automations::Clock) -> grokhub_core::LocalClock {
    clock.at(grokhub_core::now_ms())
}

#[tauri::command]
fn list_automations(
    state: State<'_, ShellState>,
    clock: automations::Clock,
) -> Result<Vec<automations::Row>, String> {
    let list = automations::load(&state.config_dir)?;
    Ok(automations::rows(&list, now_clock(clock)))
}

#[tauri::command]
fn add_automation(
    state: State<'_, ShellState>,
    text: String,
    clock: automations::Clock,
) -> Result<Vec<automations::Row>, String> {
    let clock = now_clock(clock);
    let list = automations::add(&state.config_dir, &text, clock)?;
    Ok(automations::rows(&list, clock))
}

#[tauri::command]
fn set_automation_enabled(
    state: State<'_, ShellState>,
    id: String,
    enabled: bool,
    clock: automations::Clock,
) -> Result<Vec<automations::Row>, String> {
    let clock = now_clock(clock);
    let list = automations::set_enabled(&state.config_dir, &id, enabled, clock)?;
    Ok(automations::rows(&list, clock))
}

#[tauri::command]
fn remove_automation(
    state: State<'_, ShellState>,
    id: String,
    clock: automations::Clock,
) -> Result<Vec<automations::Row>, String> {
    let list = automations::remove(&state.config_dir, &id)?;
    Ok(automations::rows(&list, now_clock(clock)))
}

/// Runs one automation now, unattended: it can read, and every tool that
/// would need an OK is refused.
#[tauri::command]
async fn run_automation(
    state: State<'_, ShellState>,
    id: String,
) -> Result<automations::RunResult, String> {
    let settings = current(&state);
    let config_dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let route = route(&settings, agent::SessionMode::Chat)?;
        automations::run_now(
            &config_dir,
            &id,
            route.client,
            &route.model,
            route.workspace,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn workspace(state: &ShellState) -> PathBuf {
    agent::workspace_for(&current(state).workspace)
}

#[tauri::command]
fn get_memory(state: State<'_, ShellState>) -> Result<memory::Notes, String> {
    memory::load(&workspace(&state))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemorySaved {
    notes: memory::Notes,
    redacted: bool,
}

#[tauri::command]
fn save_memory(
    state: State<'_, ShellState>,
    global: bool,
    text: String,
    base: String,
) -> Result<MemorySaved, String> {
    let (notes, redacted) = memory::save(&workspace(&state), global, &text, &base)?;
    Ok(MemorySaved { notes, redacted })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryAdded {
    status: String,
    notes: memory::Notes,
}

#[tauri::command]
fn add_memory(
    state: State<'_, ShellState>,
    text: String,
    global: bool,
) -> Result<MemoryAdded, String> {
    let workspace = workspace(&state);
    let status = memory::add(&workspace, &text, global)?;
    Ok(MemoryAdded {
        status,
        notes: memory::load(&workspace)?,
    })
}

#[tauri::command]
fn search_memory(state: State<'_, ShellState>, query: String) -> Result<Vec<memory::Hit>, String> {
    memory::search(&workspace(&state), &query)
}

/// Every connector call can wait on a server's slot while chat or a check
/// holds it for up to the startup or tool timeout, so all of them run off
/// the main thread.
async fn off_main<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_connectors(state: State<'_, ShellState>) -> Result<Vec<connectors::Row>, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::list(&workspace)).await
}

#[tauri::command]
async fn check_connectors(state: State<'_, ShellState>) -> Result<Vec<connectors::Row>, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::check(&workspace)).await
}

#[tauri::command]
async fn restart_connector(
    state: State<'_, ShellState>,
    name: String,
) -> Result<connectors::Changed, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::restart(&workspace, &name)).await
}

#[tauri::command]
async fn sign_out_connector(
    state: State<'_, ShellState>,
    name: String,
) -> Result<connectors::Changed, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::sign_out(&workspace, &name)).await
}

/// The page asks "Disconnect {name}?" first.
#[tauri::command]
async fn disconnect_connector(
    state: State<'_, ShellState>,
    name: String,
) -> Result<connectors::Changed, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::disconnect(&workspace, &name)).await?
}

/// What this platform runs; the UI hides the rest.
#[tauri::command]
fn get_capabilities() -> capabilities::Capabilities {
    capabilities::current()
}

#[tauri::command]
async fn get_journal(state: State<'_, ShellState>) -> Result<journal::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || journal::load(&dir)).await?
}

#[tauri::command]
async fn journal_add(
    state: State<'_, ShellState>,
    question: String,
) -> Result<journal::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || journal::add(&dir, &question)).await?
}

#[tauri::command]
async fn journal_act(
    state: State<'_, ShellState>,
    id: String,
    act: journal::Act,
) -> Result<journal::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || journal::act(&dir, &id, act)).await?
}

#[tauri::command]
async fn journal_to_mission(
    state: State<'_, ShellState>,
    id: String,
    conversation_id: String,
) -> Result<journal::Mission, String> {
    let dir = state.config_dir.clone();
    off_main(move || journal::to_mission(&dir, &id, &conversation_id)).await?
}

#[tauri::command]
async fn get_pulse(state: State<'_, ShellState>) -> Result<pulse::Page, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::load(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn pulse_act(
    state: State<'_, ShellState>,
    id: String,
    act: pulse::Act,
    day: pulse::Day,
) -> Result<pulse::Page, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::act(&dir, &id, act, &day, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn get_local_ai(state: State<'_, ShellState>) -> Result<local_ai::LocalAiView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_ai::refreshed(&dir)).await
}

#[tauri::command]
async fn save_local_ai(
    state: State<'_, ShellState>,
    config: local_ai::LocalAiConfig,
) -> Result<local_ai::LocalAiView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_ai::save_view(&dir, config)).await?
}

#[tauri::command]
async fn reset_local_ai(state: State<'_, ShellState>) -> Result<local_ai::LocalAiView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_ai::reset(&dir)).await?
}

#[tauri::command]
async fn check_model_file(
    state: State<'_, ShellState>,
    path: String,
) -> Result<local_ai::FileRow, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_ai::check_file(&dir, &path)).await?
}

#[tauri::command]
async fn run_local_speed_test(state: State<'_, ShellState>) -> Result<String, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_ai::speed_test(&dir)).await?
}

#[tauri::command]
fn get_web_access(web: State<'_, Arc<remote::WebAccess>>) -> remote::WebAccessView {
    web.view()
}

/// Settings → Web access. On: serve the web app on the LAN with a new
/// pairing code. Off: every paired browser is dropped.
#[tauri::command]
fn set_web_access(
    app: AppHandle,
    web: State<'_, Arc<remote::WebAccess>>,
    on: bool,
) -> Result<remote::WebAccessView, String> {
    capabilities::needs(capabilities::current().web_access, "Web access")?;
    if !on {
        web.stop();
        return Ok(web.view());
    }
    let dispatch_app = app.clone();
    let dispatch: remote::Dispatch =
        Arc::new(move |command, args| remote_call(&dispatch_app, command, &args));
    let assets: remote::Assets = Arc::new(move |path| {
        app.asset_resolver()
            .get(path.to_string())
            .map(|asset| (asset.bytes, asset.mime_type))
    });
    web.start(
        std::net::Ipv4Addr::UNSPECIFIED.into(),
        remote::PORT,
        dispatch,
        assets,
    )
}

fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    serde_json::from_value(args.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|err| format!("{name}: {err}"))
}

fn out<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|err| err.to_string())
}

/// One of `remote::WEB_COMMANDS` from a paired browser, run as the window
/// would run it. A web chat never gets the desktop tools.
fn remote_call(app: &AppHandle, command: &str, args: &Value) -> Result<Value, String> {
    use tauri::async_runtime::block_on;
    let state = app.state::<ShellState>();
    let web = app.state::<Arc<remote::WebAccess>>();
    // A browser steers only the chats it started.
    let own = |args: &Value| -> Result<String, String> {
        let id: String = arg(args, "conversationId")?;
        if web.owns(&id) {
            Ok(id)
        } else {
            Err(remote::NOT_YOURS.into())
        }
    };
    match command {
        "get_settings" => out(get_settings(state)),
        "send_chat" => {
            let conversation_id: String = arg(args, "conversationId")?;
            let taken = state
                .chats
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(&conversation_id);
            if taken && !web.owns(&conversation_id) {
                return Err(remote::NOT_YOURS.into());
            }
            web.adopt(&conversation_id);
            let settings = ShellSettings {
                desktop_control: false,
                ..current(&state)
            };
            out(block_on(chat_turn(
                app.clone(),
                &state,
                conversation_id,
                arg(args, "text")?,
                arg::<Option<agent::SessionMode>>(args, "mode")?.unwrap_or_default(),
                settings,
            ))?)
        }
        "answer_elicit" => {
            answer_elicit(
                state,
                own(args)?,
                arg(args, "id")?,
                arg(args, "action")?,
                arg(args, "field")?,
                arg(args, "value")?,
            )?;
            Ok(Value::Null)
        }
        "answer_permission" => {
            answer_permission(
                state,
                own(args)?,
                arg(args, "callId")?,
                arg(args, "answer")?,
            )?;
            Ok(Value::Null)
        }
        "cancel_chat" => {
            cancel_chat(state, own(args)?);
            Ok(Value::Null)
        }
        "close_chat" => {
            close_chat(state, own(args)?);
            Ok(Value::Null)
        }
        "list_automations" => out(list_automations(state, arg(args, "clock")?)?),
        "add_automation" => out(add_automation(
            state,
            arg(args, "text")?,
            arg(args, "clock")?,
        )?),
        "set_automation_enabled" => out(set_automation_enabled(
            state,
            arg(args, "id")?,
            arg(args, "enabled")?,
            arg(args, "clock")?,
        )?),
        "remove_automation" => out(remove_automation(
            state,
            arg(args, "id")?,
            arg(args, "clock")?,
        )?),
        "run_automation" => out(block_on(run_automation(state, arg(args, "id")?))?),
        "get_memory" => out(get_memory(state)?),
        "save_memory" => out(save_memory(
            state,
            arg(args, "global")?,
            arg(args, "text")?,
            arg(args, "base")?,
        )?),
        "add_memory" => out(add_memory(state, arg(args, "text")?, arg(args, "global")?)?),
        "search_memory" => out(search_memory(state, arg(args, "query")?)?),
        "list_connectors" => out(block_on(list_connectors(state))?),
        "check_connectors" => out(block_on(check_connectors(state))?),
        "restart_connector" => out(block_on(restart_connector(state, arg(args, "name")?))?),
        "sign_out_connector" => out(block_on(sign_out_connector(state, arg(args, "name")?))?),
        "disconnect_connector" => out(block_on(disconnect_connector(state, arg(args, "name")?))?),
        _ => Err(remote::REFUSED.into()),
    }
}

#[cfg(desktop)]
fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(desktop)]
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show GrokHub", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let local = MenuItem::with_id(app, "local", local_ai::TRAY_CHECKING, false, None::<&str>)?;
    let menu = Menu::with_items(app, &[&local, &show, &quit])?;
    local_ai::start_tray(local, app.state::<ShellState>().config_dir.clone());
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("GrokHub")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// wry's WebView2 defaults (wry `create_environment`). Passing our own
/// arguments replaces them, so they are repeated here.
#[cfg(windows)]
const WEBVIEW2_DEFAULT_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// msedgedriver hands WebView2 its DevTools port and profile folder through
/// these two environment variables, but wry passes its own arguments and data
/// folder, which win over them. Carry them over so tauri-driver can attach.
/// Neither is set outside a WebDriver session.
#[cfg(windows)]
fn webview2_driver_args(extra: Option<&str>) -> Option<String> {
    let extra = extra?.trim();
    (!extra.is_empty()).then(|| format!("{WEBVIEW2_DEFAULT_ARGS} {extra}"))
}

#[cfg(windows)]
fn webview2_driver_overrides<'a>(
    mut builder: tauri::WebviewWindowBuilder<'a, tauri::Wry, tauri::AppHandle>,
) -> tauri::WebviewWindowBuilder<'a, tauri::Wry, tauri::AppHandle> {
    let extra = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").ok();
    if let Some(args) = webview2_driver_args(extra.as_deref()) {
        builder = builder.additional_browser_args(&args);
    }
    if let Some(dir) = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER") {
        builder = builder.data_directory(PathBuf::from(dir));
    }
    builder
}

fn build_main_window(app: &tauri::App) -> tauri::Result<()> {
    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
    else {
        return Ok(());
    };
    let builder = tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?;
    #[cfg(windows)]
    let builder = webview2_driver_overrides(builder);
    builder.build()?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn driver_args_keep_the_wry_defaults() {
        assert_eq!(webview2_driver_args(None), None);
        assert_eq!(webview2_driver_args(Some("  ")), None);
        assert_eq!(
            webview2_driver_args(Some("--remote-debugging-port=0")),
            Some(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port=0"
                    .to_string()
            )
        );
    }
}

/// The desktop tools' host: the shell's config folder and its switch. Cua is a
/// cabin-only spike, so it stays off here.
#[cfg(desktop)]
fn install_desktop_host() {
    grokhub_desktop::host::install(grokhub_desktop::host::Host {
        config_dir: grokhub_agent::perm::config_dir,
        settings: || grokhub_desktop::host::Settings {
            desktop_control: settings::load(&grokhub_agent::perm::config_dir()).desktop_control
                && capabilities::current().desktop_control,
            ..Default::default()
        },
        #[cfg(target_os = "linux")]
        lock_titles: grokhub_desktop::screen::lock_titles,
        #[cfg(not(target_os = "linux"))]
        lock_titles: Vec::new,
        #[cfg(target_os = "linux")]
        target_at: grokhub_desktop::screen::atspi_target_at,
    });
}

/// A phone app has no usable home folder of its own (Android sets none), so
/// the config and the default work folder live in the app's data folder.
/// This runs before anything reads either.
#[cfg(mobile)]
fn phone_home(app: &tauri::App) -> tauri::Result<()> {
    let data = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data)?;
    std::env::set_var("HOME", &data);
    std::env::set_var("GROKHUB_CONFIG", data.join("config"));
    Ok(())
}

/// Phone smoke builds (`--features phone-smoke`) report the scripted chat
/// round-trip here, and CI reads the file back from the device. Other builds
/// refuse.
#[tauri::command]
fn phone_smoke_report(app: AppHandle, text: String) -> Result<(), String> {
    if !cfg!(feature = "phone-smoke") {
        return Err("This build has no smoke mode.".into());
    }
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("phone-smoke.txt"), &text).map_err(|e| e.to_string())?;
    println!("phone smoke: {text}");
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let built = tauri::Builder::default()
        .setup(|app| {
            #[cfg(mobile)]
            phone_home(app)?;
            let config_dir = grokhub_agent::perm::config_dir();
            let loaded = settings::load(&config_dir);
            #[cfg(desktop)]
            {
                install_desktop_host();
                if loaded.desktop_control && capabilities::current().desktop_control {
                    grokhub_desktop::set_desktop_enabled(true);
                }
            }
            app.manage(ShellState {
                config_dir,
                settings: Mutex::new(loaded),
                chats: Mutex::new(HashMap::new()),
            });
            let web = Arc::new(remote::WebAccess::default());
            app.manage(Arc::clone(&web));
            // Chat events reach paired browsers too; each page keeps its own
            // conversation's.
            app.listen_any(CHAT_EVENT, move |event| {
                web.broadcast(CHAT_EVENT, event.payload())
            });
            build_main_window(app)?;
            #[cfg(desktop)]
            build_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<ShellState>();
                if current(&state).close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            phone_smoke_report,
            get_capabilities,
            get_settings,
            save_settings,
            set_api_key,
            clear_api_key,
            send_chat,
            answer_permission,
            answer_elicit,
            cancel_chat,
            close_chat,
            list_automations,
            add_automation,
            set_automation_enabled,
            remove_automation,
            run_automation,
            get_memory,
            save_memory,
            add_memory,
            search_memory,
            list_connectors,
            check_connectors,
            restart_connector,
            sign_out_connector,
            disconnect_connector,
            get_journal,
            journal_add,
            journal_act,
            journal_to_mission,
            get_pulse,
            pulse_act,
            get_local_ai,
            save_local_ai,
            reset_local_ai,
            check_model_file,
            run_local_speed_test,
            get_web_access,
            set_web_access
        ])
        .run(tauri::generate_context!());
    if let Err(err) = built {
        eprintln!("GrokHub shell failed to start: {err}");
        std::process::exit(1);
    }
}
