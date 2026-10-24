//! GrokHub's Tauri desktop shell: chat, settings and tray (cross-platform
//! recode, Phase 2). The egui cabin in `crates/grokhub-app` keeps shipping
//! until this shell reaches parity.

#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod chat;
pub mod credential;
pub mod settings;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use grokhub_agent::{AuthKind, CancelToken, XaiClient};
use serde::Serialize;
use settings::ShellSettings;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

pub const DELTA_EVENT: &str = "chat://delta";

struct ShellState {
    config_dir: PathBuf,
    settings: Mutex<ShellSettings>,
    chats: Mutex<HashMap<String, CancelToken>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub model: String,
    pub close_to_tray: bool,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatDelta<'a> {
    conversation_id: &'a str,
    text: &'a str,
}

fn view(settings: &ShellSettings) -> SettingsView {
    SettingsView {
        model: settings.model.clone(),
        close_to_tray: settings.close_to_tray,
        has_api_key: credential::has_key(),
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
) -> Result<SettingsView, String> {
    let next = ShellSettings {
        model: model.trim().to_string(),
        close_to_tray,
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

#[tauri::command]
async fn send_chat(
    app: AppHandle,
    state: State<'_, ShellState>,
    conversation_id: String,
    messages: Vec<chat::Turn>,
) -> Result<String, String> {
    let model = current(&state).model;
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
    let cancel = CancelToken::new();
    state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(conversation_id.clone(), cancel.clone());
    let id = conversation_id.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        chat::run_chat(&client, &model, &id, &messages, &cancel, &mut |text| {
            let _ = app.emit(
                DELTA_EVENT,
                ChatDelta {
                    conversation_id: &id,
                    text,
                },
            );
        })
    })
    .await;
    state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&conversation_id);
    worker.map_err(|e| e.to_string())?
}

#[tauri::command]
fn cancel_chat(state: State<'_, ShellState>, conversation_id: String) {
    if let Some(cancel) = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
    {
        cancel.cancel();
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
            cancel_chat
        ])
        .run(tauri::generate_context!());
    if let Err(err) = built {
        eprintln!("GrokHub shell failed to start: {err}");
        std::process::exit(1);
    }
}
