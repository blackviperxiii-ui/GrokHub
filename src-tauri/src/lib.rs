//! GrokHub's Tauri desktop shell: chat, settings and tray (cross-platform
//! recode, Phase 2). The egui cabin in `crates/grokhub-app` keeps shipping
//! until this shell reaches parity.

#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod agent;
pub mod credential;
pub mod settings;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_agent::{AuthKind, XaiClient};
use serde::Serialize;
use settings::ShellSettings;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

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
) -> Result<SettingsView, String> {
    let next = ShellSettings {
        model: model.trim().to_string(),
        close_to_tray,
        workspace: workspace.trim().to_string(),
    };
    settings::save(&state.config_dir, &next)?;
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

fn route(settings: &ShellSettings) -> Result<agent::Route, String> {
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
) -> Result<String, String> {
    let settings = current(&state);
    let session = session(&state, &conversation_id);
    tauri::async_runtime::spawn_blocking(move || {
        let route = route(&settings)?;
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

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show GrokHub", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
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

pub fn run() {
    let config_dir = grokhub_agent::perm::config_dir();
    let loaded = settings::load(&config_dir);
    let built = tauri::Builder::default()
        .manage(ShellState {
            config_dir,
            settings: Mutex::new(loaded),
            chats: Mutex::new(HashMap::new()),
        })
        .setup(|app| {
            build_main_window(app)?;
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
            get_settings,
            save_settings,
            set_api_key,
            clear_api_key,
            send_chat,
            answer_permission,
            cancel_chat,
            close_chat
        ])
        .run(tauri::generate_context!());
    if let Err(err) = built {
        eprintln!("GrokHub shell failed to start: {err}");
        std::process::exit(1);
    }
}
