//! GrokHub's Tauri desktop shell: chat, settings and tray (cross-platform
//! recode, Phase 2). The egui cabin in `crates/grokhub-app` keeps shipping
//! until this shell reaches parity.

#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod agent;
pub mod automations;
#[cfg(test)]
mod brand_guard;
pub mod capabilities;
pub mod connectors;
pub mod credential;
pub mod history;
pub mod interests;
pub mod journal;
pub mod local_ai;
pub mod local_engine;
#[cfg(test)]
mod md_fixtures;
pub mod memory;
pub mod models;
pub mod opener;
#[cfg(desktop)]
mod os_extras;
pub mod permissions;
pub mod personality;
pub mod playbooks;
pub mod plugins;
pub mod pulse;
pub mod pulse_feed;
pub mod remote;
pub mod remote_tls;
pub mod settings;
pub mod shell_update;
pub mod slash;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
/// Sent after every Halt, with the chats it stopped mid-reply.
pub const HALT_EVENT: &str = "halt://done";

struct ShellState {
    config_dir: PathBuf,
    settings: Mutex<ShellSettings>,
    chats: Mutex<HashMap<String, Arc<agent::Session>>>,
    /// Super+Shift+Esc was registered system-wide.
    global_halt: std::sync::atomic::AtomicBool,
    /// Asks whose notifications are out (os_extras.rs).
    #[cfg(desktop)]
    notified: os_extras::Pending,
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
    pub theme: settings::Theme,
    /// Settings → About.
    pub version: String,
    /// Settings → General → the summon hotkey.
    pub summon_hotkey: bool,
    /// Settings → General → notifications while the window is in the background.
    pub notifications: bool,
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
        theme: settings.theme,
        version: shell_update::version(),
        summon_hotkey: settings.summon_hotkey,
        notifications: settings.notifications,
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
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    #[cfg(desktop)]
    if current(&state).desktop_control != desktop_control {
        grokhub_desktop::set_desktop_enabled(desktop_control);
    }
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(view(&next))
}

/// Settings → Appearance; the page applies it at once.
#[tauri::command]
fn set_theme(state: State<'_, ShellState>, theme: settings::Theme) -> Result<SettingsView, String> {
    let next = ShellSettings {
        theme,
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(view(&next))
}

/// Settings → General and the composer chip: the models to pick from.
/// `refresh` reads the account's list again instead of the 10-minute cache.
#[tauri::command]
async fn list_models(
    state: State<'_, ShellState>,
    refresh: Option<bool>,
) -> Result<models::ModelList, String> {
    let model = current(&state).model;
    let config_dir = state.config_dir.clone();
    off_main(move || {
        // The loopback mock model lists nothing; tests see the catalog.
        if credential::test_responses_url().is_some() {
            return models::list(&model, None);
        }
        match credential::load_key() {
            Ok(bearer) => {
                let api = grokhub_agent::route::sources::XaiApiSource {
                    config_dir,
                    bearer: bearer.clone(),
                };
                let cached = models::Cached {
                    inner: &api,
                    bearer_tag: models::bearer_tag(&bearer),
                    refresh: refresh.unwrap_or(false),
                };
                models::list(&model, Some(&cached))
            }
            Err(_) => models::list(&model, None),
        }
    })
    .await
}

/// The picker's choice, saved for the next message. Unknown ids are kept.
#[tauri::command]
fn set_model(state: State<'_, ShellState>, id: String) -> Result<SettingsView, String> {
    let next = ShellSettings {
        model: models::clean_id(&id)?,
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(view(&next))
}

/// The composer's command list for a draft that starts with `/`.
#[tauri::command]
fn slash_hits(draft: String) -> Vec<slash::Hit> {
    slash::hits(&draft, false)
}

/// Runs a slash line instead of sending it. Nothing reaches the model.
#[tauri::command]
fn run_slash(state: State<'_, ShellState>, line: String) -> Result<slash::Effect, String> {
    run_slash_line(&state, &line, false)
}

fn run_slash_line(state: &ShellState, line: &str, remote: bool) -> Result<slash::Effect, String> {
    let action = slash::parse(line, remote)?;
    let settings = current(state);
    slash::apply(action, &workspace(state), &settings.model, |id| {
        let next = ShellSettings {
            model: models::clean_id(id)?,
            ..current(state)
        };
        settings::save(&state.config_dir, &next)?;
        let id = next.model.clone();
        *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next;
        Ok(id)
    })
}

#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    opener::open(&url)
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

/// The model client for the signed-in key, or the loopback mock in tests.
fn model_client() -> Result<XaiClient, String> {
    Ok(match credential::test_responses_url() {
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
    })
}

fn route(
    settings: &ShellSettings,
    session: agent::SessionMode,
    config_dir: &Path,
) -> Result<agent::Route, String> {
    let client = model_client()?;
    Ok(agent::Route {
        client: Arc::new(client),
        auth_kind: AuthKind::ApiKey,
        model: settings.model.clone(),
        workspace: agent::workspace_for(&settings.workspace),
        mode: settings.approvals.mode(),
        desktop: settings.desktop_control && capabilities::current().desktop_control,
        session,
        voice: personality::voice(
            config_dir,
            &settings.personality,
            settings.explicit_personalities,
        ),
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
    set_last_chat(&state, &conversation_id);
    chat_turn(app, &state, conversation_id, text, mode, current(&state)).await
}

/// The chat the window has open, reopened at the next launch; empty for none.
/// A failed write only loses that, so chat goes on.
fn set_last_chat(state: &ShellState, conversation_id: &str) {
    let now = current(state);
    if now.last_chat == conversation_id {
        return;
    }
    let next = ShellSettings {
        last_chat: conversation_id.to_string(),
        ..now
    };
    if settings::save(&state.config_dir, &next).is_ok() {
        *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next;
    }
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
    let config_dir = state.config_dir.clone();
    if capabilities::current().pulse {
        // What the person says in chat steers the Feed writer's taste note.
        pulse_feed::note_chat(&config_dir, &text, grokhub_core::now_ms());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let route = route(&settings, mode, &config_dir)?;
        let id = conversation_id.clone();
        session.prompt(
            &text,
            route,
            Box::new(move |event| {
                #[cfg(desktop)]
                os_extras::on_chat_event(&app, &id, &event);
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
    app: AppHandle,
    state: State<'_, ShellState>,
    conversation_id: String,
    call_id: String,
    answer: String,
) -> Result<(), String> {
    let answer = agent::parse_answer(&answer)?;
    let session = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
        .cloned()
        .ok_or_else(|| "That chat is closed.".to_string())?;
    // A notification for this ask comes down, and can't answer it after this.
    #[cfg(desktop)]
    os_extras::answered_in_window(&app, &conversation_id, &call_id, || {
        session.answer(&call_id, answer)
    });
    #[cfg(mobile)]
    {
        let _ = app;
        session.answer(&call_id, answer);
    }
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

/// Halt from the top bar, Ctrl+Alt+H, the tray or Super+Shift+Esc: stops
/// desktop control and every running prompt (`agent::halt`), then tells the
/// window, which shows it.
#[cfg(desktop)]
fn halt_now(app: &AppHandle) -> Vec<String> {
    let state = app.state::<ShellState>();
    let busy = {
        let chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
        agent::halt(&state.config_dir, chats.values())
    };
    let _ = app.emit(HALT_EVENT, &busy);
    busy
}

/// Halt. Desktop only: phones have no desktop control, and the web app never
/// calls it (it is not in `remote::WEB_COMMANDS`).
#[tauri::command]
fn halt_desktop(app: AppHandle) -> Result<Vec<String>, String> {
    #[cfg(desktop)]
    {
        Ok(halt_now(&app))
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err("Halt stops desktop control, which runs on a computer.".into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HaltKeys {
    /// Super+Shift+Esc works from any app, not only this window.
    pub global: bool,
}

/// Settings → General's note on the Halt keys.
#[tauri::command]
fn get_halt_keys(state: State<'_, ShellState>) -> HaltKeys {
    HaltKeys {
        global: state.global_halt.load(std::sync::atomic::Ordering::SeqCst),
    }
}

/// Halt from any app, as the cabin's Halt keys. The cabin holds the same keys
/// when it runs; either app's Halt writes the shared halt stamp.
pub const HALT_HOTKEY: &str = "Super+Shift+Escape";

/// Registers [`HALT_HOTKEY`] on the global-shortcut plugin (`os_extras`).
/// Best effort: Wayland gives an app no global keys, and the cabin or another
/// app may hold them; Ctrl+Alt+H in the window, the button and the tray still
/// work. It only listens: nothing is typed or sent to other apps.
#[cfg(desktop)]
fn register_halt_keys(app: &AppHandle) -> bool {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    if !capabilities::current().summon_hotkey {
        return false;
    }
    app.global_shortcut()
        .on_shortcut(HALT_HOTKEY, |app, _keys, event| {
            if event.state == ShortcutState::Pressed {
                halt_now(app);
            }
        })
        .is_ok()
}

/// New chat: the old conversation's worker ends. Its saved turns stay, so
/// History can open it again.
#[tauri::command]
fn close_chat(state: State<'_, ShellState>, conversation_id: String) {
    end_chat(&state, &conversation_id);
}

fn end_chat(state: &ShellState, conversation_id: &str) {
    let removed = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(conversation_id);
    if let Some(session) = removed {
        session.cancel();
    }
    if current(state).last_chat == conversation_id {
        set_last_chat(state, "");
    }
}

/// History: the shell's saved chats, newest first.
#[tauri::command]
async fn list_chats(state: State<'_, ShellState>) -> Result<Vec<history::ChatRow>, String> {
    let dir = state.config_dir.clone();
    off_main(move || history::list(&dir)).await
}

#[tauri::command]
async fn search_chats(
    state: State<'_, ShellState>,
    query: String,
) -> Result<Vec<history::ChatRow>, String> {
    let dir = state.config_dir.clone();
    off_main(move || history::search(&dir, &query)).await
}

/// Opens a saved chat: its newest `limit` lines, and it becomes the chat the
/// next launch reopens. The next message resumes it (`agent::serve`).
#[tauri::command]
async fn load_chat(
    state: State<'_, ShellState>,
    id: String,
    limit: Option<usize>,
) -> Result<history::Transcript, String> {
    let dir = state.config_dir.clone();
    let shown =
        off_main(move || history::load(&dir, &id, limit.unwrap_or(history::LOAD_CAP))).await??;
    set_last_chat(&state, &shown.id);
    Ok(shown)
}

/// The chat open when the window last closed, if it is still saved.
#[tauri::command]
async fn get_last_chat(
    state: State<'_, ShellState>,
) -> Result<Option<history::Transcript>, String> {
    let (dir, id) = (state.config_dir.clone(), current(&state).last_chat);
    if id.is_empty() {
        return Ok(None);
    }
    off_main(move || history::load(&dir, &id, history::LOAD_CAP).ok()).await
}

#[tauri::command]
async fn rename_chat(
    state: State<'_, ShellState>,
    id: String,
    title: String,
) -> Result<(), String> {
    let dir = state.config_dir.clone();
    off_main(move || history::rename(&dir, &id, &title)).await?
}

/// The page asks "Delete {title}?" first. A reply still running stops.
#[tauri::command]
async fn delete_chat(state: State<'_, ShellState>, id: String) -> Result<(), String> {
    let dir = state.config_dir.clone();
    let doomed = id.clone();
    off_main(move || history::delete(&dir, &doomed)).await??;
    end_chat(&state, &id);
    Ok(())
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
    app: AppHandle,
    state: State<'_, ShellState>,
    id: String,
) -> Result<automations::RunResult, String> {
    let result = run_automation_now(state, id.clone()).await;
    #[cfg(desktop)]
    if result.is_ok() {
        let name = automations::load(&app.state::<ShellState>().config_dir)
            .ok()
            .and_then(|list| list.into_iter().find(|a| a.id == id))
            .map(|a| a.name)
            .unwrap_or_default();
        os_extras::automation_finished(&app, &name);
    }
    #[cfg(mobile)]
    let _ = app;
    result
}

/// [`run_automation`] without the notification, as a paired browser runs it.
async fn run_automation_now(
    state: State<'_, ShellState>,
    id: String,
) -> Result<automations::RunResult, String> {
    let settings = current(&state);
    let config_dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let route = route(&settings, agent::SessionMode::Chat, &config_dir)?;
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

#[tauri::command]
fn edit_memory_row(
    state: State<'_, ShellState>,
    id: String,
    text: String,
) -> Result<memory::Notes, String> {
    memory::edit_row(&workspace(&state), &id, &text)
}

#[tauri::command]
fn forget_memory_row(state: State<'_, ShellState>, id: String) -> Result<memory::Notes, String> {
    memory::forget_row(&workspace(&state), &id)
}

#[tauri::command]
fn approve_memory_row(state: State<'_, ShellState>, id: String) -> Result<memory::Notes, String> {
    memory::approve_row(&workspace(&state), &id)
}

#[tauri::command]
fn reject_memory_row(state: State<'_, ShellState>, id: String) -> Result<memory::Notes, String> {
    memory::reject_row(&workspace(&state), &id)
}

#[tauri::command]
fn set_memory_writes(state: State<'_, ShellState>, on: bool) -> Result<memory::Notes, String> {
    memory::set_writes(&workspace(&state), on)
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
async fn get_permissions(state: State<'_, ShellState>) -> Result<permissions::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || permissions::load(&dir, &workspace)).await
}

#[tauri::command]
async fn add_permission_rule(
    state: State<'_, ShellState>,
    rule: String,
    kind: permissions::Kind,
) -> Result<permissions::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || permissions::add_rule(&dir, &workspace, &rule, kind)).await?
}

#[tauri::command]
async fn remove_permission_rule(
    state: State<'_, ShellState>,
    index: usize,
    rule: String,
) -> Result<permissions::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || permissions::remove_rule(&dir, &workspace, index, &rule)).await?
}

#[tauri::command]
async fn remember_grant(
    state: State<'_, ShellState>,
    command: String,
) -> Result<permissions::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || permissions::remember(&dir, &workspace, &command)).await?
}

#[tauri::command]
async fn forget_grant(
    state: State<'_, ShellState>,
    command: String,
) -> Result<permissions::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || permissions::forget(&dir, &workspace, &command)).await?
}

#[tauri::command]
async fn get_personalities(state: State<'_, ShellState>) -> Result<personality::Page, String> {
    let (dir, now) = (state.config_dir.clone(), current(&state));
    off_main(move || personality::load(&dir, &now.personality, now.explicit_personalities)).await
}

/// The default voice for every chat; applies to the next message.
#[tauri::command]
fn set_personality(state: State<'_, ShellState>, id: String) -> Result<personality::Page, String> {
    let now = current(&state);
    let picked = personality::allowed(
        personality::pick(&state.config_dir, &id)?,
        now.explicit_personalities,
    )?;
    let next = ShellSettings {
        personality: personality::stored_id(&picked.id),
        ..now
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(personalities(&state))
}

/// Settings → Personality → Adults-only personalities.
#[tauri::command]
fn set_explicit_personalities(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<personality::Page, String> {
    let next = ShellSettings {
        explicit_personalities: on,
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next;
    Ok(personalities(&state))
}

fn personalities(state: &ShellState) -> personality::Page {
    let now = current(state);
    personality::load(
        &state.config_dir,
        &now.personality,
        now.explicit_personalities,
    )
}

/// Returns the copy's id with the page, so the editor can open it.
#[tauri::command]
fn duplicate_personality(
    state: State<'_, ShellState>,
    id: String,
) -> Result<(String, personality::Page), String> {
    let copy = personality::duplicate(&state.config_dir, &id)?;
    Ok((copy, personalities(&state)))
}

#[tauri::command]
fn save_personality(
    state: State<'_, ShellState>,
    edit: personality::Edit,
) -> Result<personality::Page, String> {
    personality::save(&state.config_dir, edit)?;
    Ok(personalities(&state))
}

/// Deleting the default voice's preset puts chats back on the default voice.
#[tauri::command]
fn delete_personality(
    state: State<'_, ShellState>,
    id: String,
) -> Result<personality::Page, String> {
    personality::delete(&state.config_dir, &id)?;
    let mut now = current(&state);
    if personality::shown_id(&now.personality) == id.trim() {
        now.personality = String::new();
        settings::save(&state.config_dir, &now)?;
        *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = now.clone();
    }
    Ok(personalities(&state))
}

#[tauri::command]
async fn get_playbooks(state: State<'_, ShellState>) -> Result<playbooks::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || playbooks::load(&dir)).await
}

/// Returns the saved playbook's name with the page.
#[tauri::command]
fn save_playbook(
    state: State<'_, ShellState>,
    text: String,
) -> Result<(String, playbooks::Page), String> {
    let name = playbooks::save(&state.config_dir, &text)?;
    Ok((name, playbooks::load(&state.config_dir)))
}

/// Returns the copy's id with the page, so the editor can open it.
#[tauri::command]
fn duplicate_playbook(
    state: State<'_, ShellState>,
    id: String,
) -> Result<(String, playbooks::Page), String> {
    let copy = playbooks::duplicate(&state.config_dir, &id)?;
    Ok((copy, playbooks::load(&state.config_dir)))
}

#[tauri::command]
fn reset_playbook(state: State<'_, ShellState>, id: String) -> Result<playbooks::Page, String> {
    playbooks::reset(&state.config_dir, &id)?;
    Ok(playbooks::load(&state.config_dir))
}

#[tauri::command]
async fn get_plugins(state: State<'_, ShellState>) -> Result<plugins::Page, String> {
    capabilities::needs(capabilities::current().plugins, "Plugins")?;
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || plugins::load(&dir, &workspace)).await
}

#[tauri::command]
async fn plugin_act(
    state: State<'_, ShellState>,
    name: String,
    act: plugins::Act,
) -> Result<plugins::Page, String> {
    capabilities::needs(capabilities::current().plugins, "Plugins")?;
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || plugins::act(&dir, &workspace, &name, act)).await
}

#[tauri::command]
async fn install_plugin(
    state: State<'_, ShellState>,
    source: String,
) -> Result<plugins::Page, String> {
    capabilities::needs(capabilities::current().plugins, "Plugins")?;
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || plugins::install(&dir, &workspace, &source)).await
}

#[tauri::command]
async fn fetch_plugin_market(
    state: State<'_, ShellState>,
    url: String,
) -> Result<plugins::Market, String> {
    capabilities::needs(capabilities::current().plugins, "Plugins")?;
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || plugins::fetch_market(&dir, &workspace, &url)).await
}

#[tauri::command]
async fn get_interests(state: State<'_, ShellState>) -> Result<interests::Page, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || interests::load(&dir)).await
}

#[tauri::command]
async fn add_interest(
    state: State<'_, ShellState>,
    text: String,
) -> Result<interests::Page, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || interests::add(&dir, &text)).await?
}

#[tauri::command]
async fn interest_act(
    state: State<'_, ShellState>,
    topic: String,
    act: interests::Act,
) -> Result<interests::Page, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || interests::act(&dir, &topic, act)).await?
}

#[tauri::command]
async fn get_pulse(
    state: State<'_, ShellState>,
    day: Option<pulse::Day>,
) -> Result<pulse::Page, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::load(&dir, day.as_ref(), grokhub_core::now_ms())).await?
}

/// Discuss: the text the chat starts from, carrying the post.
#[tauri::command]
async fn pulse_discuss(state: State<'_, ShellState>, id: String) -> Result<String, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::discuss(&dir, &id, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn pulse_why(state: State<'_, ShellState>, id: String) -> Result<String, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::why(&dir, &id)).await?
}

#[tauri::command]
async fn get_feed_prompt(state: State<'_, ShellState>) -> Result<pulse::Prompt, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::get_prompt(&dir)).await
}

/// Saving the feed prompt writes one new post right away.
#[tauri::command]
async fn save_feed_prompt(
    state: State<'_, ShellState>,
    text: String,
) -> Result<pulse::Prompt, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::save_prompt(&dir, &text)).await?
}

#[tauri::command]
async fn pulse_image(state: State<'_, ShellState>, name: String) -> Result<String, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::image(&dir, &name)).await?
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
fn start_local_engine_download(
    app: AppHandle,
    state: State<'_, ShellState>,
) -> Result<local_engine::EngineInfo, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    local_engine::start_engine(app, state.config_dir.clone())
}

#[tauri::command]
fn cancel_local_engine_download() -> Result<(), String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    local_engine::cancel_engine()
}

#[tauri::command]
fn start_local_drafter_download(
    app: AppHandle,
    state: State<'_, ShellState>,
    target_id: String,
) -> Result<Option<local_engine::DraftOffer>, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    local_engine::start_drafter(app, state.config_dir.clone(), target_id)
}

#[tauri::command]
fn cancel_local_drafter_download() -> Result<(), String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    local_engine::cancel_drafter()
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
    let config_dir = app.state::<ShellState>().config_dir.clone();
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
        Some(&config_dir),
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
                app.clone(),
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
        "run_automation" => out(block_on(run_automation_now(state, arg(args, "id")?))?),
        "get_memory" => out(get_memory(state)?),
        "save_memory" => out(save_memory(
            state,
            arg(args, "global")?,
            arg(args, "text")?,
            arg(args, "base")?,
        )?),
        "add_memory" => out(add_memory(state, arg(args, "text")?, arg(args, "global")?)?),
        "slash_hits" => out(slash::hits(&arg::<String>(args, "draft")?, true)),
        "run_slash" => out(run_slash_line(&state, &arg::<String>(args, "line")?, true)?),
        "search_memory" => out(search_memory(state, arg(args, "query")?)?),
        "edit_memory_row" => out(edit_memory_row(
            state,
            arg(args, "id")?,
            arg(args, "text")?,
        )?),
        "forget_memory_row" => out(forget_memory_row(state, arg(args, "id")?)?),
        "approve_memory_row" => out(approve_memory_row(state, arg(args, "id")?)?),
        "reject_memory_row" => out(reject_memory_row(state, arg(args, "id")?)?),
        "set_memory_writes" => out(set_memory_writes(state, arg(args, "on")?)?),
        "list_connectors" => out(block_on(list_connectors(state))?),
        "check_connectors" => out(block_on(check_connectors(state))?),
        "restart_connector" => out(block_on(restart_connector(state, arg(args, "name")?))?),
        "sign_out_connector" => out(block_on(sign_out_connector(state, arg(args, "name")?))?),
        "disconnect_connector" => out(block_on(disconnect_connector(state, arg(args, "name")?))?),
        "get_pulse" => out(block_on(get_pulse(state, arg(args, "day")?))?),
        "pulse_act" => out(block_on(pulse_act(
            state,
            arg(args, "id")?,
            arg(args, "act")?,
            arg(args, "day")?,
        ))?),
        "pulse_discuss" => out(block_on(pulse_discuss(state, arg(args, "id")?))?),
        "pulse_why" => out(block_on(pulse_why(state, arg(args, "id")?))?),
        "get_feed_prompt" => out(block_on(get_feed_prompt(state))?),
        "save_feed_prompt" => out(block_on(save_feed_prompt(state, arg(args, "text")?))?),
        "pulse_image" => out(block_on(pulse_image(state, arg(args, "name")?))?),
        _ => Err(remote::refused()),
    }
}

#[tauri::command]
fn get_update_status(state: State<'_, ShellState>) -> shell_update::UpdateView {
    shell_update::UpdateView::now(&state.config_dir)
}

#[tauri::command]
fn set_update_channel(
    state: State<'_, ShellState>,
    channel: String,
) -> Result<shell_update::UpdateView, String> {
    let channel = grokhub_core::Channel::parse(&channel).ok_or("unknown channel")?;
    shell_update::set_channel(&state.config_dir, channel)?;
    Ok(shell_update::UpdateView::now(&state.config_dir))
}

#[tauri::command]
async fn check_shell_update(
    app: AppHandle,
    state: State<'_, ShellState>,
) -> Result<shell_update::UpdateView, String> {
    let dir = state.config_dir.clone();
    #[cfg(desktop)]
    {
        let mut view = shell_update::UpdateView::now(&dir);
        if view.updatable {
            view.available = shell_update::plugin::check(&app, &dir)
                .await?
                .map(|u| u.version);
        }
        Ok(view)
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        Ok(shell_update::UpdateView::now(&dir))
    }
}

#[tauri::command]
async fn install_shell_update(app: AppHandle, state: State<'_, ShellState>) -> Result<(), String> {
    let dir = state.config_dir.clone();
    #[cfg(desktop)]
    {
        if !shell_update::UpdateView::now(&dir).updatable {
            return Err("This install doesn't update itself.".into());
        }
        shell_update::plugin::install(&app, &dir).await
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, dir);
        Err("Updates aren't available on this device.".into())
    }
}

/// Look for an update now and then while the shell runs; tell the UI, never
/// install without a click.
#[cfg(desktop)]
fn watch_for_updates(app: AppHandle, config_dir: PathBuf) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(90)).await;
        loop {
            let mut view = shell_update::UpdateView::now(&config_dir);
            if view.updatable {
                if let Ok(Some(update)) = shell_update::plugin::check(&app, &config_dir).await {
                    view.available = Some(update.version);
                    let _ = app.emit(shell_update::UPDATE_EVENT, view);
                }
            }
            tokio::time::sleep(shell_update::CHECK_EVERY).await;
        }
    });
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
    let show_label = format!("Show {}", grokhub_core::brand::product_name());
    let show = MenuItem::with_id(app, "show", show_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    // Always on; harmless when nothing runs.
    let halt = MenuItem::with_id(app, "halt", "Halt", true, None::<&str>)?;
    let local = MenuItem::with_id(app, "local", local_ai::TRAY_CHECKING, false, None::<&str>)?;
    let menu = Menu::with_items(app, &[&local, &halt, &show, &quit])?;
    local_ai::start_tray(local, app.state::<ShellState>().config_dir.clone());
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip(grokhub_core::brand::product_name())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "halt" => {
                halt_now(app);
            }
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
    let builder = tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?
        .title(grokhub_core::brand::product_name());
    // A start at login stays in the tray when closing keeps it there.
    #[cfg(desktop)]
    let builder = builder.visible(!os_extras::starts_hidden(
        std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()),
        current(&app.state::<ShellState>()).close_to_tray && capabilities::current().tray,
    ));
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
    // Pin the start time now, as the cabin does: a Halt after it keeps desktop
    // steps off until the next start, even if none ran before the Halt.
    grokhub_desktop::process_started_ms();
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

/// The reply links `open_url` will hand to the default browser: http(s) only,
/// by the same rule the cabin and the chat page use (`md_link_ok`).
fn openable_url(url: &str) -> Result<&str, String> {
    let url = url.trim();
    if grokhub_core::md_link_ok(url) {
        Ok(url)
    } else {
        Err("Only web links (https:// or http://) open from a reply.".into())
    }
}

/// A reply's link, opened in the default browser. The web app never calls
/// this (it is not in `remote::WEB_COMMANDS`); its links open in a new tab.
#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = openable_url(&url)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    #[cfg(desktop)]
    let builder = os_extras::plugins(builder);
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    let built = builder
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
            pulse_feed::start(config_dir.clone(), {
                let config_dir = config_dir.clone();
                move || {
                    let client: Box<dyn grokhub_agent::ModelClient> = Box::new(model_client()?);
                    Ok((client, settings::load(&config_dir).model))
                }
            });
            #[cfg(desktop)]
            os_extras::setup(app.handle(), &loaded);
            app.manage(ShellState {
                config_dir,
                settings: Mutex::new(loaded),
                chats: Mutex::new(HashMap::new()),
                global_halt: std::sync::atomic::AtomicBool::new(false),
                #[cfg(desktop)]
                notified: os_extras::Pending::default(),
            });
            #[cfg(desktop)]
            {
                let global = register_halt_keys(app.handle());
                app.state::<ShellState>()
                    .global_halt
                    .store(global, std::sync::atomic::Ordering::SeqCst);
            }
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
            #[cfg(desktop)]
            watch_for_updates(app.handle().clone(), grokhub_agent::perm::config_dir());
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
            set_theme,
            open_external,
            set_api_key,
            list_models,
            set_model,
            slash_hits,
            run_slash,
            clear_api_key,
            send_chat,
            answer_permission,
            answer_elicit,
            cancel_chat,
            halt_desktop,
            get_halt_keys,
            close_chat,
            list_chats,
            search_chats,
            load_chat,
            get_last_chat,
            rename_chat,
            delete_chat,
            list_automations,
            add_automation,
            set_automation_enabled,
            remove_automation,
            run_automation,
            get_memory,
            save_memory,
            add_memory,
            search_memory,
            edit_memory_row,
            forget_memory_row,
            approve_memory_row,
            reject_memory_row,
            set_memory_writes,
            list_connectors,
            check_connectors,
            restart_connector,
            sign_out_connector,
            disconnect_connector,
            get_journal,
            journal_add,
            journal_act,
            journal_to_mission,
            get_permissions,
            add_permission_rule,
            remove_permission_rule,
            remember_grant,
            forget_grant,
            get_personalities,
            set_personality,
            set_explicit_personalities,
            duplicate_personality,
            save_personality,
            delete_personality,
            get_playbooks,
            save_playbook,
            duplicate_playbook,
            reset_playbook,
            get_plugins,
            plugin_act,
            install_plugin,
            fetch_plugin_market,
            get_interests,
            add_interest,
            interest_act,
            get_pulse,
            pulse_act,
            pulse_discuss,
            pulse_why,
            get_feed_prompt,
            save_feed_prompt,
            pulse_image,
            get_local_ai,
            save_local_ai,
            start_local_engine_download,
            cancel_local_engine_download,
            start_local_drafter_download,
            cancel_local_drafter_download,
            reset_local_ai,
            check_model_file,
            run_local_speed_test,
            get_web_access,
            set_web_access,
            open_url,
            get_update_status,
            set_update_channel,
            check_shell_update,
            install_shell_update,
            #[cfg(desktop)]
            os_extras::get_autostart,
            #[cfg(desktop)]
            os_extras::set_autostart,
            #[cfg(desktop)]
            os_extras::set_summon_hotkey,
            #[cfg(desktop)]
            os_extras::set_notifications
        ])
        .run(tauri::generate_context!());
    if let Err(err) = built {
        eprintln!(
            "{} failed to start: {err}",
            grokhub_core::brand::product_name()
        );
        std::process::exit(1);
    }
}

#[cfg(test)]
mod open_url_tests {
    use super::openable_url;

    #[test]
    fn opens_only_web_links() {
        assert_eq!(openable_url(" https://x.ai/docs "), Ok("https://x.ai/docs"));
        assert_eq!(openable_url("http://example.com"), Ok("http://example.com"));
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<b>x</b>",
            "not a url",
            "",
            "https://",
            "https:///etc",
            "mailto:a@b.c",
            "https://a b",
        ] {
            assert!(openable_url(bad).is_err(), "{bad} must be refused");
        }
    }
}

#[cfg(all(test, desktop))]
mod halt_key_tests {
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

    #[test]
    fn halt_keys_are_the_cabins_super_shift_escape() {
        let parsed: Shortcut = super::HALT_HOTKEY.parse().expect("a valid shortcut");
        assert_eq!(
            parsed,
            Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Escape)
        );
    }
}
