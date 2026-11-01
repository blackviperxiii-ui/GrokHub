//! GrokHub's Tauri desktop shell: chat, settings and tray (cross-platform
//! recode, Phase 2). The egui cabin in `crates/grokhub-app` keeps shipping
//! until this shell reaches parity.

#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod agent;
pub mod attach;
pub mod automations;
pub mod background;
#[cfg(test)]
mod brand_guard;
pub mod capabilities;
pub mod connectors;
pub mod credential;
pub mod history;
pub mod imagine;
pub mod interests;
pub mod journal;
pub mod library;
pub mod local_ai;
pub mod local_engine;
#[cfg(test)]
mod md_fixtures;
pub mod memory;
pub mod models;
pub mod nyx;
pub mod oauth;
pub mod opener;
#[cfg(desktop)]
mod os_extras;
pub mod permissions;
pub mod personality;
pub mod playbooks;
pub mod plugins;
pub mod privacy;
pub mod pulse;
pub mod pulse_feed;
pub mod queue;
pub mod remote;
pub mod remote_devices;
pub mod remote_tls;
#[cfg(desktop)]
pub mod scheduler;
pub mod settings;
pub mod shell_update;
pub mod skills;
pub mod slash;
pub mod voice;
pub mod workboard;

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

struct ShellState {
    config_dir: PathBuf,
    settings: Mutex<ShellSettings>,
    chats: Mutex<HashMap<String, Arc<agent::Session>>>,
    /// Files waiting on each chat's composer.
    trays: attach::Trays,
    sign_in: Arc<oauth::SignIn>,
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
    /// Signed in with Grok: who, when xAI said.
    pub signed_in: Option<oauth::SignedIn>,
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
        signed_in: oauth::signed_in(&oauth::Keychain),
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

/// Settings → Defaults and the composer chip: the models to pick from.
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

/// Sign in with Grok, step one: the code and the page to type it at.
#[tauri::command]
async fn start_grok_sign_in(state: State<'_, ShellState>) -> Result<oauth::Started, String> {
    capabilities::needs(capabilities::current().grok_sign_in, "Sign in with Grok")?;
    let flow = Arc::clone(&state.sign_in);
    off_main(move || flow.start()).await?
}

/// Step two, asked every few seconds by Settings → Account.
#[tauri::command]
async fn poll_grok_sign_in(state: State<'_, ShellState>) -> Result<oauth::Poll, String> {
    let flow = Arc::clone(&state.sign_in);
    off_main(move || flow.poll(&oauth::Keychain)).await?
}

#[tauri::command]
fn grok_sign_out(state: State<'_, ShellState>) -> Result<SettingsView, String> {
    state.sign_in.cancel();
    oauth::sign_out(&oauth::Keychain)?;
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

/// The model client: the loopback mock in tests, else Grok sign-in first,
/// else the saved API key. The kind says which meter the turn uses.
fn model_client() -> Result<(XaiClient, AuthKind), String> {
    let idle = Duration::from_secs(120);
    if let Some(url) = credential::test_responses_url() {
        let client = XaiClient::new(credential::LOOPBACK_TEST_BEARER, AuthKind::ApiKey, idle)
            .with_loopback_url(&url)?;
        return Ok((client, AuthKind::ApiKey));
    }
    signed_client(oauth::bearer(&oauth::Keychain), credential::load_key, idle)
}

/// Picks the bearer: a live Grok sign-in wins over a key. A sign-in that
/// ended falls back to the key when there is one.
fn signed_client(
    oauth: Result<Option<String>, String>,
    key: impl FnOnce() -> Result<String, String>,
    idle: Duration,
) -> Result<(XaiClient, AuthKind), String> {
    match oauth {
        Ok(Some(bearer)) => Ok((
            XaiClient::new(bearer, AuthKind::OAuth, idle),
            AuthKind::OAuth,
        )),
        Ok(None) => Ok((
            XaiClient::new(key()?, AuthKind::ApiKey, idle),
            AuthKind::ApiKey,
        )),
        Err(ended) => match key() {
            Ok(k) => Ok((XaiClient::new(k, AuthKind::ApiKey, idle), AuthKind::ApiKey)),
            Err(_) => Err(ended),
        },
    }
}

fn route(
    settings: &ShellSettings,
    session: agent::SessionMode,
    config_dir: &Path,
) -> Result<agent::Route, String> {
    let (client, auth_kind) = model_client()?;
    Ok(agent::Route {
        client: Arc::new(client),
        auth_kind,
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
    attachments: Option<Vec<String>>,
) -> Result<String, String> {
    let mode = mode.unwrap_or_default();
    set_last_chat(&state, &conversation_id);
    let turn = Turn {
        text,
        mode,
        attachments: attachments.unwrap_or_default(),
    };
    chat_turn(app, &state, conversation_id, turn, current(&state)).await
}

/// One message from the composer: its text, chip and attached files (paths
/// from `stage_attachment`).
struct Turn {
    text: String,
    mode: agent::SessionMode,
    attachments: Vec<String>,
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
    turn: Turn,
    settings: ShellSettings,
) -> Result<String, String> {
    let Turn {
        text,
        mode,
        attachments,
    } = turn;
    let files = state.trays.take(&conversation_id, &attachments)?;
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
            files,
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

/// Push to talk: one recording (a WAV, as base64) to text for the draft.
/// Nothing is sent to chat; the person presses Send.
#[tauri::command]
async fn transcribe(state: State<'_, ShellState>, wav: String) -> Result<String, String> {
    use base64::Engine as _;
    capabilities::needs(capabilities::current().voice, "Voice")?;
    // Base64 is 4 bytes per 3: refuse a body past the cap unread.
    if wav.len() / 4 * 3 > voice::WAV_MAX + 3 {
        return Err(voice::TOO_BIG.into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(wav.trim())
        .map_err(|_| voice::NOT_WAV.to_string())?;
    let config_dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || voice::transcribe(&config_dir, &bytes))
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
    state.trays.forget(conversation_id);
}

/// The chat model's input kinds, for the chips' notes.
fn model_mods(state: &ShellState) -> Vec<String> {
    attach::modalities(&state.config_dir, &current(state).model)
}

/// A pasted or dropped file from the webview or the web app, as base64.
#[tauri::command]
fn stage_attachment(
    state: State<'_, ShellState>,
    conversation_id: String,
    name: String,
    data: String,
) -> Result<attach::Staged, String> {
    let mods = model_mods(&state);
    let mods: Vec<&str> = mods.iter().map(String::as_str).collect();
    state
        .trays
        .stage_base64(&state.config_dir, &conversation_id, &name, &data, &mods)
}

/// Files dropped on the window, by path. The window only: the web app sends
/// bytes.
#[tauri::command]
fn stage_attachment_paths(
    state: State<'_, ShellState>,
    conversation_id: String,
    paths: Vec<String>,
) -> Vec<attach::Outcome> {
    stage_paths(
        &state,
        &conversation_id,
        paths.into_iter().map(PathBuf::from),
    )
}

fn stage_paths(
    state: &ShellState,
    conversation_id: &str,
    paths: impl Iterator<Item = PathBuf>,
) -> Vec<attach::Outcome> {
    let mods = model_mods(state);
    let mods: Vec<&str> = mods.iter().map(String::as_str).collect();
    paths
        .map(|path| {
            state
                .trays
                .stage_path(&state.config_dir, conversation_id, &path, &mods)
                .into()
        })
        .collect()
}

/// The composer's + button: the system picker, then each file is staged.
#[tauri::command]
async fn pick_attachments(
    app: AppHandle,
    conversation_id: String,
) -> Result<Vec<attach::Outcome>, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        let picked = app
            .dialog()
            .file()
            .set_title("Add to chat")
            .blocking_pick_files()
            .unwrap_or_default();
        let paths = picked.into_iter().filter_map(|p| p.into_path().ok());
        let state = app.state::<ShellState>();
        Ok(stage_paths(&state, &conversation_id, paths))
    }
    #[cfg(mobile)]
    {
        let _ = (app, conversation_id);
        Err(attach::NO_PICKER.into())
    }
}

/// The chip's ×.
#[tauri::command]
fn remove_attachment(state: State<'_, ShellState>, conversation_id: String, path: String) {
    state.trays.remove(&conversation_id, &path);
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

/// Agents: the open chat's to-dos and background tasks.
#[tauri::command]
async fn get_queue(
    state: State<'_, ShellState>,
    conversation_id: String,
) -> Result<queue::Queue, String> {
    let dir = state.config_dir.clone();
    off_main(move || queue::queue(&dir, &conversation_id)).await
}

/// Agents → Stop: one task by id, or every task of the chat without one.
#[tauri::command]
async fn stop_task(
    state: State<'_, ShellState>,
    conversation_id: String,
    task_id: Option<String>,
) -> Result<queue::Queue, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        queue::stop(&conversation_id, task_id.as_deref())?;
        Ok(queue::queue(&dir, &conversation_id))
    })
    .await?
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

/// Edit in place from the row's Edit box. Words with no time are refused.
#[tauri::command]
fn edit_automation(
    state: State<'_, ShellState>,
    id: String,
    text: String,
    clock: automations::Clock,
) -> Result<Vec<automations::Row>, String> {
    let clock = now_clock(clock);
    let list = automations::edit(&state.config_dir, &id, &text, clock)?;
    Ok(automations::rows(&list, clock))
}

/// The window's wall clock, so the scheduler fires jobs at local times. Only
/// this window reports it: a paired browser may be in another time zone.
#[tauri::command]
fn set_scheduler_clock(clock: automations::Clock) {
    #[cfg(desktop)]
    scheduler::note_ui_clock(clock, grokhub_core::now_ms());
    #[cfg(not(desktop))]
    let _ = clock;
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

/// The Add sheet's catalog; `refresh` only from the Refresh click.
#[tauri::command]
async fn connector_catalog(
    state: State<'_, ShellState>,
    refresh: bool,
) -> Result<connectors::CatalogView, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::catalog(&workspace, refresh)).await
}

#[tauri::command]
async fn add_catalog_connector(
    state: State<'_, ShellState>,
    id: String,
) -> Result<connectors::Changed, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::add_catalog(&workspace, &id)).await?
}

#[tauri::command]
async fn add_connector_config(
    state: State<'_, ShellState>,
    text: String,
) -> Result<connectors::Changed, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::add_config(&workspace, &text)).await?
}

#[tauri::command]
async fn set_connector_enabled(
    state: State<'_, ShellState>,
    name: String,
    on: bool,
) -> Result<connectors::Changed, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::set_enabled(&workspace, &name, on)).await?
}

/// Opens the server's https sign-in page and waits for the browser to come back.
#[tauri::command]
async fn sign_in_connector(
    state: State<'_, ShellState>,
    name: String,
) -> Result<connectors::Changed, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::sign_in(&workspace, &name, &opener::open)).await?
}

/// Posts the key's approval card; the page answers it with `answer_connector_key`.
#[tauri::command]
async fn save_connector_key(
    state: State<'_, ShellState>,
    name: String,
    key: String,
) -> Result<connectors::KeyCard, String> {
    let workspace = workspace(&state);
    off_main(move || connectors::start_key(&workspace, &name, key)).await?
}

#[tauri::command]
async fn answer_connector_key(id: String, approve: bool) -> Result<connectors::Changed, String> {
    off_main(move || connectors::answer_key(&id, approve)).await?
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

/// The board file's mtime; the page reloads only when it changes.
#[tauri::command]
async fn workboard_stamp(state: State<'_, ShellState>) -> Result<u64, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::stamp(&dir)).await
}

/// A card's progress log tail, read when its drawer opens.
#[tauri::command]
async fn workboard_log(
    state: State<'_, ShellState>,
    id: String,
) -> Result<Vec<grokhub_core::WorkLogEvent>, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::log_tail(&dir, &id)).await
}

/// Resume a paused run. A stub until KB-03 fills in the real resume.
#[tauri::command]
async fn workboard_resume(
    state: State<'_, ShellState>,
    id: String,
    conversation_id: String,
) -> Result<workboard::Resume, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::resume(&dir, &id, &conversation_id)).await?
}

/// Workboard: the cabin's work cards from the same file, through the shared
/// board lock and merge.
#[tauri::command]
async fn get_workboard(state: State<'_, ShellState>) -> Result<workboard::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::load(&dir)).await?
}

#[tauri::command]
async fn workboard_add(
    state: State<'_, ShellState>,
    title: String,
    detail: String,
) -> Result<workboard::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::add(&dir, &title, &detail)).await?
}

#[tauri::command]
async fn workboard_act(
    state: State<'_, ShellState>,
    id: String,
    act: workboard::Act,
) -> Result<workboard::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::act(&dir, &id, act)).await?
}

#[tauri::command]
async fn workboard_work(
    state: State<'_, ShellState>,
    id: String,
) -> Result<workboard::Work, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::work(&dir, &id)).await?
}

/// Delete every archived card archived `days` ago or more (0: all of them),
/// with their logs. The page asks first with the count.
#[tauri::command]
async fn workboard_delete_archived(
    state: State<'_, ShellState>,
    days: u32,
) -> Result<workboard::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::delete_archived(&dir, days)).await?
}

/// Undo on the sweep notice: its cards go back to Todo.
#[tauri::command]
async fn workboard_sweep_undo(state: State<'_, ShellState>) -> Result<workboard::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::undo_sweep(&dir)).await?
}

/// Dismiss the sweep notice; the cards stay archived.
#[tauri::command]
async fn workboard_sweep_dismiss(state: State<'_, ShellState>) -> Result<workboard::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::dismiss_sweep(&dir)).await?
}

/// Settings → Workboard tidy: auto-archive age and the auto-delete choice.
#[tauri::command]
async fn get_workboard_tidy(state: State<'_, ShellState>) -> Result<workboard::Tidy, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::tidy(&dir)).await
}

#[tauri::command]
async fn set_workboard_tidy(
    state: State<'_, ShellState>,
    auto_delete_days: u32,
) -> Result<workboard::Tidy, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::set_auto_delete(&dir, auto_delete_days)).await?
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

/// The chat's Nyx for the voice in use (or `voice`) and what the chat is
/// doing: a running tool's title, else `mood`. Explicit art only with
/// Adults-only personalities on.
#[tauri::command]
fn nyx_look(
    state: State<'_, ShellState>,
    voice: Option<String>,
    mood: Option<grokhub_core::companion::Mood>,
    tool: Option<String>,
) -> Option<nyx::NyxLook> {
    let now = current(&state);
    nyx::look(
        &state.config_dir,
        voice.as_deref().unwrap_or(&now.personality),
        now.explicit_personalities,
        nyx::activity(mood, tool.as_deref()),
    )
}

/// Settings → Personality's pictures, by preset id.
#[tauri::command]
fn nyx_tiles(state: State<'_, ShellState>, ids: Vec<String>) -> HashMap<String, String> {
    nyx::tiles(&ids, current(&state).explicit_personalities)
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
async fn get_skills(state: State<'_, ShellState>) -> Result<skills::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    let home = grokhub_core::user_home();
    off_main(move || skills::load(&dir, &workspace, home.as_deref())).await
}

/// The page names the skill; the shell finds its folder itself.
#[tauri::command]
async fn open_skill_folder(
    state: State<'_, ShellState>,
    source: skills::Source,
    name: String,
) -> Result<(), String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    let home = grokhub_core::user_home();
    off_main(move || {
        skills::folder(&dir, &workspace, home.as_deref(), source, &name)
            .and_then(|folder| skills::open_folder(&folder))
    })
    .await?
}

#[tauri::command]
async fn get_library() -> Result<library::Page, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(|| library::page(grokhub_core::steam::detect_root().as_deref())).await
}

#[tauri::command]
async fn library_cover(
    state: State<'_, ShellState>,
    app_id: u32,
    shape: library::Shape,
) -> Result<Option<String>, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        let root = grokhub_core::steam::detect_root();
        let net = library::LiveNet {
            config_dir: dir.clone(),
        };
        let now = grokhub_core::now_ms() / 1000;
        library::cover(&dir, root.as_deref(), app_id, shape, &net, now)
    })
    .await
}

#[tauri::command]
async fn library_play(app_id: u32) -> Result<String, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(move || library::play(grokhub_core::steam::detect_root().as_deref(), app_id)).await?
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

/// A PC health card's options window. Read only.
#[tauri::command]
async fn pulse_fix(state: State<'_, ShellState>, id: String) -> Result<pulse::FixView, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::fix(&dir, &id)).await?
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

/// The Imagine page: the gallery (the cabin's wall) and whether chat can sign a call.
#[tauri::command]
async fn get_imagine(state: State<'_, ShellState>) -> Result<imagine::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || imagine::load(&dir, imagine::ready())).await?
}

/// Makes images, or one video, through chat's guarded media tool. Resolves
/// once the results are saved and pinned; Stop ends it early.
#[tauri::command]
async fn imagine_generate(
    state: State<'_, ShellState>,
    prompt: String,
    aspect: String,
    count: u32,
    kind: imagine::Kind,
) -> Result<imagine::Made, String> {
    let dir = state.config_dir.clone();
    let req = imagine::Request {
        prompt,
        aspect,
        count,
        kind,
    };
    off_main(move || {
        let bearer = imagine::bearer()?;
        let run = imagine::start()?;
        let api = grokhub_agent::imagine_client(&bearer);
        imagine::generate(
            &dir,
            &req,
            api.as_ref(),
            &|| run.stopped(),
            grokhub_core::now_ms(),
        )
    })
    .await?
}

#[tauri::command]
fn imagine_cancel() -> bool {
    imagine::cancel()
}

#[tauri::command]
async fn imagine_image(state: State<'_, ShellState>, name: String) -> Result<String, String> {
    let dir = state.config_dir.clone();
    off_main(move || imagine::image(&dir, &name)).await?
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

/// The Devices page (desktop only; not a web command): this computer, web
/// access, and the browsers paired right now. Kept in memory only.
#[tauri::command]
fn get_devices(web: State<'_, Arc<remote::WebAccess>>) -> remote_devices::DevicesView {
    web.devices()
}

/// Devices → Sign out all browsers: a new pairing code ends every pairing.
#[tauri::command]
fn forget_all_devices(web: State<'_, Arc<remote::WebAccess>>) -> remote_devices::DevicesView {
    web.forget_all()
}

/// Settings → Behavior: the tray, the composer and the limits on scheduled work.
#[tauri::command]
fn get_behavior(state: State<'_, ShellState>) -> settings::BehaviorView {
    settings::behavior_view(&current(&state))
}

#[tauri::command]
fn save_behavior(
    state: State<'_, ShellState>,
    close_to_tray: bool,
    behavior: settings::Behavior,
) -> Result<settings::BehaviorView, String> {
    let next = ShellSettings {
        close_to_tray: close_to_tray && capabilities::current().tray,
        behavior: behavior.checked()?,
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(settings::behavior_view(&next))
}

/// Settings → Defaults: the work folder and how chat handles changes.
#[tauri::command]
fn save_defaults(
    state: State<'_, ShellState>,
    workspace: String,
    approvals: settings::Approvals,
) -> Result<SettingsView, String> {
    let next = ShellSettings {
        workspace: workspace.trim().to_string(),
        approvals,
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(view(&next))
}

/// Settings → Privacy & data. Reads `offline.toml` again and may wait on the
/// keyring, so it runs off the main thread.
#[tauri::command]
async fn get_privacy(state: State<'_, ShellState>) -> Result<privacy::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || privacy::page(&dir, privacy::keyring(&dir), grokhub_core::now_ms())).await
}

#[tauri::command]
async fn set_offline_only(state: State<'_, ShellState>, on: bool) -> Result<privacy::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        privacy::set_offline(&dir, on)?;
        Ok(privacy::page(
            &dir,
            privacy::keyring(&dir),
            grokhub_core::now_ms(),
        ))
    })
    .await?
}

#[tauri::command]
fn open_data_folder(state: State<'_, ShellState>) -> Result<(), String> {
    privacy::open_data_folder(&state.config_dir)
}

/// Settings → Background: who fires automations, and when each runs.
#[tauri::command]
fn get_background(state: State<'_, ShellState>) -> Result<background::Page, String> {
    let list = automations::load(&state.config_dir)?;
    Ok(background::page(
        &list,
        &current(&state).behavior,
        grokhub_core::now_ms(),
    ))
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
            let turn = Turn {
                text: arg(args, "text")?,
                mode: arg::<Option<agent::SessionMode>>(args, "mode")?.unwrap_or_default(),
                attachments: arg::<Option<Vec<String>>>(args, "attachments")?.unwrap_or_default(),
            };
            out(block_on(chat_turn(
                app.clone(),
                &state,
                conversation_id,
                turn,
                settings,
            ))?)
        }
        // A paste or drop in the web app; the body cap keeps it under 1 MB.
        "stage_attachment" => {
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
            out(stage_attachment(
                state,
                conversation_id,
                arg(args, "name")?,
                arg(args, "data")?,
            )?)
        }
        "remove_attachment" => {
            remove_attachment(state, own(args)?, arg(args, "path")?);
            Ok(Value::Null)
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
        "get_queue" => out(block_on(get_queue(state, own(args)?))?),
        "stop_task" => out(block_on(stop_task(
            state,
            own(args)?,
            arg(args, "taskId")?,
        ))?),
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
        "edit_automation" => out(edit_automation(
            state,
            arg(args, "id")?,
            arg(args, "text")?,
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
        "pulse_fix" => out(block_on(pulse_fix(state, arg(args, "id")?))?),
        "get_feed_prompt" => out(block_on(get_feed_prompt(state))?),
        "save_feed_prompt" => out(block_on(save_feed_prompt(state, arg(args, "text")?))?),
        "pulse_image" => out(block_on(pulse_image(state, arg(args, "name")?))?),
        "get_imagine" => out(block_on(get_imagine(state))?),
        "imagine_generate" => out(block_on(imagine_generate(
            state,
            arg(args, "prompt")?,
            arg(args, "aspect")?,
            arg(args, "count")?,
            arg(args, "kind")?,
        ))?),
        "imagine_cancel" => out(imagine_cancel()),
        "imagine_image" => out(block_on(imagine_image(state, arg(args, "name")?))?),
        "get_skills" => out(block_on(get_skills(state))?),
        "nyx_look" => out(nyx_look(
            state,
            arg(args, "voice")?,
            arg(args, "mood")?,
            arg(args, "tool")?,
        )),
        "get_workboard" => out(block_on(get_workboard(state))?),
        "workboard_add" => out(block_on(workboard_add(
            state,
            arg(args, "title")?,
            arg(args, "detail")?,
        ))?),
        "workboard_act" => out(block_on(workboard_act(
            state,
            arg(args, "id")?,
            arg(args, "act")?,
        ))?),
        "workboard_work" => out(block_on(workboard_work(state, arg(args, "id")?))?),
        "workboard_stamp" => out(block_on(workboard_stamp(state))?),
        "workboard_log" => out(block_on(workboard_log(state, arg(args, "id")?))?),
        "workboard_delete_archived" => out(block_on(workboard_delete_archived(
            state,
            arg(args, "days")?,
        ))?),
        "workboard_sweep_undo" => out(block_on(workboard_sweep_undo(state))?),
        "workboard_sweep_dismiss" => out(block_on(workboard_sweep_dismiss(state))?),
        "get_behavior" => out(get_behavior(state)),
        "get_privacy" => out(block_on(get_privacy(state))?),
        "get_background" => out(get_background(state)?),
        _ => Err(remote::refused()),
    }
}

/// Scheduled automations fire from here while the cabin is closed, with the
/// settings as they are at each run.
#[cfg(desktop)]
fn start_scheduler(app: &tauri::App) {
    let config_dir = app.state::<ShellState>().config_dir.clone();
    let handle = app.handle().clone();
    scheduler::start(
        config_dir.clone(),
        move || {
            let settings = settings::load(&config_dir);
            let route = route(&settings, agent::SessionMode::Chat, &config_dir)?;
            Ok((route.client, route.model, route.workspace))
        },
        move |fired| {
            let _ = handle.emit(scheduler::RAN_EVENT, fired);
        },
    );
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
    let local = MenuItem::with_id(app, "local", local_ai::TRAY_CHECKING, false, None::<&str>)?;
    let menu = Menu::with_items(app, &[&local, &show, &quit])?;
    local_ai::start_tray(local, app.state::<ShellState>().config_dir.clone());
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip(grokhub_core::brand::product_name())
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
    let window = builder.build()?;
    #[cfg(target_os = "linux")]
    allow_microphone(&window);
    #[cfg(not(target_os = "linux"))]
    drop(window);
    Ok(())
}

/// WebKitGTK denies a page's mic unless the app says yes. The window shows
/// only the app's own UI, so its push-to-talk gets the mic; a camera ask
/// still gets the default no.
#[cfg(target_os = "linux")]
fn allow_microphone(window: &tauri::WebviewWindow) {
    use webkit2gtk::glib::prelude::{Cast, ObjectExt};
    use webkit2gtk::{PermissionRequestExt, UserMediaPermissionRequest, WebViewExt};
    let _ = window.with_webview(|webview| {
        webview.inner().connect_permission_request(|_, request| {
            let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() else {
                return false;
            };
            let audio: bool = media.property("is-for-audio-device");
            let video: bool = media.property("is-for-video-device");
            if audio && !video {
                request.allow();
                true
            } else {
                false
            }
        });
    });
}

#[cfg(test)]
mod sign_in_tests {
    use super::*;

    #[test]
    fn a_grok_sign_in_wins_over_a_key_and_an_ended_one_falls_back() {
        let idle = Duration::from_secs(1);
        let key = || Ok("xai-key".to_string());
        let none = || Err(grokhub_core::XAI_NEED_SIGNIN.to_string());
        let kind = |r: Result<(XaiClient, AuthKind), String>| r.map(|(_, k)| k);
        assert_eq!(
            kind(signed_client(Ok(Some("at".into())), key, idle)),
            Ok(AuthKind::OAuth)
        );
        assert_eq!(
            kind(signed_client(Ok(None), key, idle)),
            Ok(AuthKind::ApiKey)
        );
        assert_eq!(
            kind(signed_client(Err(oauth::EXPIRED.into()), key, idle)),
            Ok(AuthKind::ApiKey)
        );
        assert_eq!(
            kind(signed_client(Err(oauth::EXPIRED.into()), none, idle)),
            Err(oauth::EXPIRED.to_string())
        );
        assert_eq!(
            kind(signed_client(Ok(None), none, idle)),
            Err(grokhub_core::XAI_NEED_SIGNIN.to_string())
        );
    }
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
    let builder = builder.plugin(tauri_plugin_dialog::init());
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
                    let client: Box<dyn grokhub_agent::ModelClient> = Box::new(model_client()?.0);
                    Ok((client, settings::load(&config_dir).model))
                }
            });
            #[cfg(desktop)]
            os_extras::setup(app.handle(), &loaded);
            app.manage(ShellState {
                config_dir,
                settings: Mutex::new(loaded),
                chats: Mutex::new(HashMap::new()),
                trays: attach::Trays::default(),
                sign_in: Arc::default(),
                #[cfg(desktop)]
                notified: os_extras::Pending::default(),
            });
            let web = Arc::new(remote::WebAccess::default());
            app.manage(Arc::clone(&web));
            // Chat events reach paired browsers too; each page keeps its own
            // conversation's.
            app.listen_any(CHAT_EVENT, move |event| {
                web.broadcast(CHAT_EVENT, event.payload())
            });
            #[cfg(desktop)]
            start_scheduler(app);
            // The Workboard's local tidy, while the cabin is closed. No model.
            #[cfg(desktop)]
            workboard::start_sweeper(app.state::<ShellState>().config_dir.clone());
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
            start_grok_sign_in,
            poll_grok_sign_in,
            grok_sign_out,
            send_chat,
            transcribe,
            answer_permission,
            answer_elicit,
            cancel_chat,
            close_chat,
            list_chats,
            search_chats,
            load_chat,
            get_last_chat,
            rename_chat,
            delete_chat,
            stage_attachment,
            stage_attachment_paths,
            pick_attachments,
            remove_attachment,
            list_automations,
            add_automation,
            set_automation_enabled,
            remove_automation,
            edit_automation,
            set_scheduler_clock,
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
            connector_catalog,
            add_catalog_connector,
            add_connector_config,
            set_connector_enabled,
            sign_in_connector,
            save_connector_key,
            answer_connector_key,
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
            get_skills,
            open_skill_folder,
            get_interests,
            add_interest,
            interest_act,
            get_pulse,
            pulse_act,
            pulse_discuss,
            pulse_why,
            pulse_fix,
            get_feed_prompt,
            save_feed_prompt,
            pulse_image,
            get_imagine,
            imagine_generate,
            imagine_cancel,
            imagine_image,
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
            get_devices,
            forget_all_devices,
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
            os_extras::set_notifications,
            nyx_look,
            nyx_tiles,
            get_queue,
            stop_task,
            get_workboard,
            workboard_add,
            workboard_act,
            workboard_work,
            workboard_stamp,
            workboard_log,
            workboard_resume,
            workboard_delete_archived,
            workboard_sweep_undo,
            workboard_sweep_dismiss,
            get_workboard_tidy,
            set_workboard_tidy,
            get_behavior,
            save_behavior,
            save_defaults,
            get_privacy,
            set_offline_only,
            open_data_folder,
            get_background,
            get_library,
            library_cover,
            library_play,
            open_url
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
