//! GrokHub's Tauri desktop app: chat, settings and tray. It replaced the egui
//! cabin.

#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod agent;
pub mod ai_models;
#[cfg(desktop)]
mod app_guard;
pub mod attach;
pub mod audio_check;
pub mod auto_fixes;
pub mod auto_setup;
pub mod automations;
pub mod background;
pub mod bg_runs;
pub mod bot_screen;
#[cfg(test)]
mod brand_guard;
pub mod browser_attach;
pub mod capabilities;
pub mod card_chat;
mod catalog_refresh;
pub mod change_rows;
pub mod changes;
pub mod channel_asks;
pub mod channel_inbox;
pub mod channels;
#[cfg(desktop)]
pub mod chat_review;
pub mod chat_tabs;
pub mod chat_trials;
pub mod chat_zoom;
pub mod chips;
pub mod claude_chat;
mod code_agent;
mod code_keep;
mod code_rollback;
mod code_sessions;
pub mod command_box;
// The Code tab's logic lives in grokhub-code (shared with the cabin); the
// old module names stay so the commands below and settings read the same.
pub use grokhub_code::activity as code_activity;
pub use grokhub_code::commit as code_commit;
pub use grokhub_code::deploy as code_deploy;
pub use grokhub_code::files as code_files;
pub use grokhub_code::push as code_push;
pub use grokhub_code::repo as code;
pub use grokhub_code::runs as code_runs;
pub use grokhub_code::secrets as code_secrets;
pub use grokhub_code::ship as code_ship;
pub use grokhub_code::term as code_term;
pub mod battlenet;
pub mod connectors;
pub mod credential;
#[cfg(desktop)]
pub mod desktop_nyx;
pub mod diagnostics;
pub mod done_for_you;
pub mod episode;
pub mod export;
pub mod fleet;
pub mod game_companion;
pub mod game_watch;
pub mod games;
pub mod goals;
#[cfg(desktop)]
pub mod headless_service;
mod helper_asks;
#[cfg(desktop)]
mod helper_loop;
pub mod history;
pub mod home;
#[cfg(desktop)]
mod home_fps;
pub mod home_tiles;
mod home_updates;
#[cfg(desktop)]
pub mod hub_join;
#[cfg(desktop)]
pub mod hub_send;
#[cfg(desktop)]
pub mod hub_share;
pub mod imagine;
pub mod inbox;
#[cfg(desktop)]
mod indexers;
pub mod interests;
pub mod journal;
pub mod know_me;
mod lead;
pub mod library;
#[cfg(desktop)]
mod living_wall;
pub mod local_ai;
pub mod local_engine;
#[cfg(desktop)]
mod local_governor;
mod local_model_checks;
mod local_use;
#[cfg(test)]
mod md_fixtures;
pub mod meetings;
pub mod memory;
mod mission;
pub mod model_library;
pub mod model_wait;
pub mod mods;
pub mod mods_agent;
pub mod mods_install;
#[cfg(desktop)]
mod net_watch;
#[cfg(desktop)]
mod night;
mod nudges;
pub mod nyx;
mod profile;
mod prompt_tidy;
pub use grokhub_agent::xai_keychain as oauth;
mod composer_ghost;
#[cfg(desktop)]
mod focus_help;
pub mod github;
pub mod home_assistant;
pub mod hook_command;
pub mod onboard;
pub mod opener;
pub mod openrouter_sign_in;
#[cfg(desktop)]
mod os_extras;
pub mod pc_files;
#[cfg(desktop)]
pub mod pc_files_run;
pub mod pc_health;
pub mod pc_helpers;
pub mod permissions;
pub mod personality;
pub mod playbooks;
pub mod plugins;
pub mod privacy;
pub mod proactive;
pub mod project_hooks;
pub mod projects;
pub mod pulse;
pub mod pulse_digests;
pub mod pulse_feed;
pub mod pulse_live;
pub mod queue;
pub mod remote;
pub mod remote_devices;
pub mod remote_tls;
#[cfg(test)]
mod reply_sources_fixtures;
pub mod review_cards;
mod router_upkeep;
#[cfg(desktop)]
pub mod routine_runs;
#[cfg(desktop)]
pub mod routines_page;
pub mod run_resume;
#[cfg(desktop)]
pub mod scheduler;
pub mod scopes;
pub mod screen_record;
pub mod screen_report;
pub mod screen_skill;
pub mod screenshot_offer;
pub mod self_improve;
#[cfg(desktop)]
pub mod self_review;
pub mod sessions_board;
pub mod settings;
pub mod shell_update;
pub mod ship_gate;
pub mod skills;
pub mod slash;
pub mod slash_ops;
pub mod spend_cards;
mod steam_checks;
pub mod storage;
mod stt_stream;
pub mod suggested;
mod task_feed;
mod turns;
pub mod unattended;
#[cfg(desktop)]
mod update_rollback;
pub mod voice;
mod voice_live;
mod vstack_sync;
pub mod wake_word;
pub mod web_chats;
pub mod workboard;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_agent::{AuthKind, XaiClient};
use grokhub_core::review_pass::SuggestedTile;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use settings::ShellSettings;
#[cfg(desktop)]
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
#[cfg(desktop)]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Listener, Manager, State, WindowEvent};

pub const CHAT_EVENT: &str = "chat://event";
/// Sent when a chat's to-dos or background tasks changed: read the queue again.
pub const TASKS_EVENT: &str = "tasks://changed";
/// Sent after every Halt, with the chats it stopped mid-reply.
pub const HALT_EVENT: &str = "halt://done";

struct ShellState {
    config_dir: PathBuf,
    settings: Mutex<ShellSettings>,
    chats: Mutex<HashMap<String, Arc<agent::Session>>>,
    /// Super+Shift+Esc was registered system-wide.
    global_halt: std::sync::atomic::AtomicBool,
    /// Files waiting on each chat's composer.
    trays: attach::Trays,
    /// The chip rows the page was shown, for the tap that follows.
    chip_rows: Mutex<chips::Rows>,
    sign_in: Arc<oauth::SignIn>,
    /// Chat exports this window wrote; Open and Show in folder take their tokens.
    saves: export::Saves,
    /// Each chat's last read message, for a "no, just answer" right after.
    turn_reads: turns::Reads,
    /// Asks whose notifications are out (os_extras.rs).
    #[cfg(desktop)]
    notified: os_extras::Pending,
    /// Auto setup's state for this launch.
    auto: Mutex<auto_setup::Session>,
    /// A Pulse card's report, by its chat, for that chat's next message to
    /// carry unseen.
    card_carry: Mutex<HashMap<String, String>>,
    /// The folder each Code tab session works in.
    code_sessions: code_sessions::Folders,
    /// Focus help: the app the user was just in (chip + one context line).
    #[cfg(desktop)]
    focus_help: Mutex<focus_help::FocusHelp>,
    /// The game launch companion's running and just-closed game.
    game_session: Mutex<game_companion::Session>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub close_to_tray: bool,
    pub has_api_key: bool,
    /// Signed in with Grok: who, when xAI said.
    pub signed_in: Option<oauth::SignedIn>,
    /// The folder picked in Settings, empty for the default.
    pub workspace: String,
    /// The folder the agent actually works in.
    pub workspace_path: String,
    pub approvals: settings::Approvals,
    /// Settings → Defaults → Always collapse: thoughts start folded.
    pub always_collapse_thoughts: bool,
    pub desktop_control: bool,
    pub theme: settings::Theme,
    /// Settings → About.
    pub version: String,
    /// Settings → General → the summon hotkey.
    pub summon_hotkey: bool,
    /// Settings → General → notifications while the window is in the background.
    pub notifications: bool,
    /// Settings → Appearance → Objectives beside chat: Chat draws the
    /// objective and recent chats on its left and a PC snapshot on its right.
    pub chat_objectives: bool,
    /// Settings → Account under a live Grok sign-in: the SuperGrok pool's
    /// room, or that xAI's plan gate refused the sign-in.
    pub plan_note: Option<String>,
    /// Settings → Behavior → Dockable panels: the main pages as tabs in panes.
    pub dock_panels: bool,
    /// The dock's pane layout, kept in `shell.json`.
    pub dock_layout: settings::DockLayout,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatEvent<'a> {
    conversation_id: &'a str,
    event: &'a agent::UiEvent,
}

fn view(settings: &ShellSettings) -> SettingsView {
    let signed_in = oauth::signed_in(&oauth::Keychain);
    SettingsView {
        close_to_tray: settings.close_to_tray,
        has_api_key: credential::has_key(),
        plan_note: signed_in
            .as_ref()
            .and_then(|_| spend_cards::plan_note_now()),
        signed_in,
        workspace: settings.workspace.clone(),
        workspace_path: agent::workspace_for(&settings.workspace)
            .display()
            .to_string(),
        approvals: settings.approvals,
        always_collapse_thoughts: settings.always_collapse_thoughts,
        desktop_control: settings.desktop_control,
        theme: settings.theme,
        version: shell_update::version(),
        summon_hotkey: settings.summon_hotkey,
        notifications: settings.notifications,
        chat_objectives: settings.chat_objectives,
        dock_panels: settings.dock_panels,
        dock_layout: settings.dock_layout.clone(),
    }
}

fn current(state: &ShellState) -> ShellSettings {
    state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// On the blocking pool, like the other keychain reads: `view` reads the
/// keychain twice, and the window asks for settings three times as it first
/// paints. Not a plain async command: Linux's Secret Service client blocks on
/// its own tokio runtime, which panics on an async worker thread.
#[tauri::command]
async fn get_settings(state: State<'_, ShellState>) -> Result<SettingsView, String> {
    let settings = current(&state);
    off_main(move || view(&settings)).await
}

/// Settings → Account → Profile and the rail's avatar. On the blocking pool:
/// the Grok name comes from the keychain, like `get_settings`.
#[tauri::command]
async fn get_profile(state: State<'_, ShellState>) -> Result<profile::Profile, String> {
    let dir = state.config_dir.clone();
    off_main(move || profile_now(&dir)).await
}

fn profile_now(dir: &std::path::Path) -> profile::Profile {
    let grok = oauth::signed_in(&oauth::Keychain).and_then(|s| s.name);
    profile::load(dir, grok.as_deref())
}

/// Settings → Account → Name; blank goes back to the Grok name.
#[tauri::command]
async fn set_profile_name(
    state: State<'_, ShellState>,
    name: String,
) -> Result<profile::Profile, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        profile::set_name(&dir, &name)?;
        Ok(profile_now(&dir))
    })
    .await?
}

/// Settings → Account → Profile picture → Choose. `None` when the picker
/// was closed without a pick.
#[tauri::command]
async fn pick_profile_picture(app: AppHandle) -> Result<Option<profile::Profile>, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        let dir = app.state::<ShellState>().config_dir.clone();
        off_main(move || {
            let Some(path) = app
                .dialog()
                .file()
                .set_title("Choose a profile picture")
                // Linux filters match case: a camera's .JPG must show too.
                .add_filter("Images", &["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"])
                .blocking_pick_file()
                .and_then(|p| p.into_path().ok())
            else {
                return Ok(None);
            };
            profile::install_picture(&dir, &path)?;
            Ok(Some(profile_now(&dir)))
        })
        .await?
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err(attach::NO_PICKER.into())
    }
}

/// Settings → Account → Remove picture.
#[tauri::command]
async fn clear_profile_picture(state: State<'_, ShellState>) -> Result<profile::Profile, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        profile::clear_picture(&dir)?;
        Ok(profile_now(&dir))
    })
    .await?
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, ShellState>,
    close_to_tray: bool,
    workspace: String,
    approvals: settings::Approvals,
    desktop_control: bool,
) -> Result<SettingsView, String> {
    let caps = capabilities::current();
    let desktop_control = desktop_control && caps.desktop_control;
    let close_to_tray = close_to_tray && caps.tray;
    let next = ShellSettings {
        close_to_tray,
        workspace: workspace.trim().to_string(),
        approvals,
        desktop_control,
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    #[cfg(desktop)]
    {
        if current(&state).desktop_control != desktop_control {
            grokhub_desktop::set_desktop_enabled(desktop_control);
            desktop_switched(&app, &state, desktop_control);
        }
        episode::sync_cua(
            desktop_control,
            episode::cua_flag(&state.config_dir),
            episode::hub_exe(),
        );
    }
    #[cfg(mobile)]
    let _ = &app;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(view(&next))
}

/// Settings → Bot Desktop.
#[tauri::command]
fn get_bot_desktop(state: State<'_, ShellState>) -> bot_screen::BotDesktopView {
    bot_screen::view(current(&state).bot_desktop, None)
}

/// Connectors → Phone and chat apps.
#[tauri::command]
fn get_channels(state: State<'_, ShellState>) -> channels::ChannelsView {
    channels::view(
        &state.config_dir,
        &grokhub_agent::channels_net::os_keychain(),
        None,
    )
}

/// Checks a bot token with Telegram or Discord, saves it in the keychain and
/// starts listening.
#[tauri::command]
async fn connect_channel(
    state: State<'_, ShellState>,
    kind: grokhub_core::msg_channels::ChannelKind,
    token: String,
) -> Result<channels::ChannelsView, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        channels::connect(
            &dir,
            &grokhub_agent::channels_net::os_keychain(),
            kind,
            &token,
        )
    })
    .await?
}

#[tauri::command]
async fn disconnect_channel(
    state: State<'_, ShellState>,
    kind: grokhub_core::msg_channels::ChannelKind,
) -> Result<channels::ChannelsView, String> {
    let dir = state.config_dir.clone();
    off_main(move || channels::disconnect(&dir, &grokhub_agent::channels_net::os_keychain(), kind))
        .await?
}

/// The code a stranger was sent: they count as paired from now on.
#[tauri::command]
fn approve_channel_code(
    state: State<'_, ShellState>,
    code: String,
) -> Result<channels::ChannelsView, String> {
    channels::approve(
        &state.config_dir,
        &grokhub_agent::channels_net::os_keychain(),
        &code,
    )
}

/// A signed webhook with a fresh secret, shown in this answer only.
#[tauri::command]
fn add_channel_webhook(
    state: State<'_, ShellState>,
    name: String,
) -> Result<channels::ChannelsView, String> {
    channels::add_webhook(
        &state.config_dir,
        &grokhub_agent::channels_net::os_keychain(),
        &name,
    )
}

#[tauri::command]
fn remove_channel_webhook(
    state: State<'_, ShellState>,
    name: String,
) -> Result<channels::ChannelsView, String> {
    channels::remove_webhook(
        &state.config_dir,
        &grokhub_agent::channels_net::os_keychain(),
        &name,
    )
}

#[tauri::command]
fn set_channel_asks(
    state: State<'_, ShellState>,
    kind: grokhub_core::msg_channels::ChannelKind,
    sender: String,
    on: bool,
) -> Result<channels::ChannelsView, String> {
    channels::set_asks(
        &state.config_dir,
        &grokhub_agent::channels_net::os_keychain(),
        kind,
        &sender,
        on,
    )
}

#[tauri::command]
fn unpair_channel_sender(
    state: State<'_, ShellState>,
    kind: grokhub_core::msg_channels::ChannelKind,
    sender: String,
) -> Result<channels::ChannelsView, String> {
    channels::unpair(
        &state.config_dir,
        &grokhub_agent::channels_net::os_keychain(),
        kind,
        &sender,
    )
}

/// Settings → General → Your browser.
#[tauri::command]
fn get_browser_attach(state: State<'_, ShellState>) -> browser_attach::AttachView {
    browser_attach::load(&state.config_dir)
}

/// Saves the attached browser and its allowed sites, then restarts a running
/// browser tool so its next step uses them.
#[tauri::command]
async fn save_browser_attach(
    state: State<'_, ShellState>,
    endpoint: String,
    sites: Vec<String>,
) -> Result<browser_attach::AttachView, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        let saved = browser_attach::save(&dir, &endpoint, &sites)?;
        grokhub_agent::mcp::browser_attach_changed();
        Ok(saved)
    })
    .await?
}

/// Settings → Bot Desktop's preview: the screen now, or nothing when it
/// isn't running.
#[tauri::command]
async fn bot_desktop_frame() -> Result<Option<String>, String> {
    off_main(bot_screen::frame).await
}

/// Settings → Bot Desktop, on or off. It applies at once, and the next
/// desktop step of a running chat goes to the screen now in use. A screen
/// that can't start leaves the switch off and says why.
#[tauri::command]
async fn set_bot_desktop(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<bot_screen::BotDesktopView, String> {
    let dir = state.config_dir.clone();
    // Starting waits for the X server, so it runs off the main thread.
    let started = off_main(move || bot_screen::apply(&dir, on)).await?;
    let on = on && started.is_ok();
    let next = ShellSettings {
        bot_desktop: on,
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next;
    Ok(bot_screen::view(on, started.err()))
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

/// Settings → Behavior → Dockable panels, on or off. The layout is kept
/// either way, so turning it back on restores the panes as they were.
#[tauri::command]
fn set_dock_panels(state: State<'_, ShellState>, on: bool) -> Result<SettingsView, String> {
    let next = ShellSettings {
        dock_panels: on,
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(view(&next))
}

/// The dock's layout as the person left it (a drag, a resize, a split). It is
/// cleaned on the way in, so what loads back always obeys the dock's rules.
#[tauri::command]
fn save_dock_layout(
    state: State<'_, ShellState>,
    layout: settings::DockLayout,
) -> Result<SettingsView, String> {
    let next = ShellSettings {
        dock_layout: layout.cleaned(),
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(view(&next))
}

/// Auto setup: the first-run window, the corner note, Settings → Auto setup
/// and chat asks for a setting. Saves `shell.json` when a setting changed and
/// zooms the window to the text size.
#[tauri::command]
fn auto_setup(
    state: State<'_, ShellState>,
    window: tauri::WebviewWindow,
    act: auto_setup::Act,
) -> Result<auto_setup::Reply, String> {
    capabilities::needs(capabilities::current().auto_setup, "Auto setup")?;
    if matches!(act, auto_setup::Act::Pass { .. }) {
        tell_display(&window);
    }
    let before = current(&state);
    let mut settings = before.clone();
    let mut session = state.auto.lock().unwrap_or_else(|e| e.into_inner());
    let has_chats = || !history::list(&state.config_dir).is_empty();
    let mut tuner = auto_setup::Tuner {
        settings: &mut settings,
        session: &mut session,
        dir: auto_setup::dir(&state.config_dir),
        has_chats: &has_chats,
        tray: capabilities::current().tray,
        device: grokhub_agent::device::current(),
        generation: grokhub_agent::device::generation(),
    };
    let said = tuner.act(act);
    let reply = tuner.reply(said);
    if settings != before {
        settings::save(&state.config_dir, &settings)?;
        *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
    }
    if settings.text_size != before.text_size {
        zoom(&window, settings.text_size);
    }
    // Her desk window stops breathing and floating with reduce motion.
    #[cfg(desktop)]
    if settings.reduce_motion != before.reduce_motion {
        desktop_nyx::publish(window.app_handle(), &settings);
    }
    Ok(reply)
}

/// Get to know you: the stage after Auto Setup's first run, and Settings →
/// Personal → Get to know you. It reads the memory files and an OpenClaw
/// workspace and Save writes them, so it runs off the main thread.
#[tauri::command]
async fn onboard_identity(
    state: State<'_, ShellState>,
    act: onboard::Act,
) -> Result<onboard::Reply, String> {
    capabilities::needs(capabilities::current().auto_setup, "Get to know you")?;
    let personality = current(&state).personality;
    off_main(move || {
        onboard::act(
            &grokhub_agent::perm::config_dir(),
            grokhub_core::paths::user_home().as_deref(),
            &personality,
            act,
            grokhub_core::now_ms(),
        )
    })
    .await?
}

/// The window's zoom follows Settings → Appearance → Text size.
fn zoom(window: &tauri::WebviewWindow, text_size: u16) {
    let _ = window.set_zoom(f64::from(text_size) / 100.0);
}

/// The screen the window is on, for the device profile (its readers can't see it).
fn tell_display(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let monitors = window
        .available_monitors()
        .map_or(1, |all| all.len().max(1));
    grokhub_agent::device::set_display(grokhub_core::autotune::Display {
        scale_pct: (monitor.scale_factor() * 100.0).round() as u16,
        width_px: monitor.size().width,
        monitors: u8::try_from(monitors).unwrap_or(u8::MAX),
    });
}

/// The composer's command list for a draft that starts with `/`.
#[tauri::command]
fn slash_hits(draft: String) -> Vec<slash::Hit> {
    slash::hits(&draft, false)
}

/// Runs a slash line instead of sending it. Nothing reaches the model.
/// `conversation_id` is the chat the line was typed in.
#[tauri::command]
async fn run_slash(
    app: AppHandle,
    state: State<'_, ShellState>,
    line: String,
    conversation_id: Option<String>,
) -> Result<slash::Effect, String> {
    run_slash_line(
        &state,
        &line,
        false,
        conversation_id.as_deref(),
        None,
        Some(&app),
    )
}

/// `app` reaches this computer's hub for `/sync` and `/inhabit`; a paired
/// browser has none.
fn run_slash_line(
    state: &ShellState,
    line: &str,
    remote: bool,
    conversation_id: Option<&str>,
    owned: Option<&dyn Fn(&str) -> bool>,
    app: Option<&AppHandle>,
) -> Result<slash::Effect, String> {
    // Screen recording and the audio check run on this computer only.
    let reporter = || match app {
        Some(app) => report_into_chat(state, app, conversation_id),
        None => screen_record::Reporter::local(),
    };
    if let Some(done) =
        screen_record::slash(line, &state.config_dir, remote, conversation_id, reporter)
    {
        return done;
    }
    let action = slash::parse(line, remote)?;
    if conversation_id.is_some_and(grokhub_agent::scratch::is_scratch)
        && slash::writes_memory(&action)
    {
        return Err(grokhub_agent::scratch::NO_WRITES.into());
    }
    if let slash::Action::Shell(line) = &action {
        // `parse` refuses it for a browser already; the line runs here only.
        if remote {
            return Err("/sh works on your computer.".into());
        }
        let check = command_box::check(
            &state.config_dir,
            &workspace_of(state, conversation_id),
            line,
            current(state).code_ship,
        );
        return Ok(slash::Effect::Shell { check });
    }
    if let slash::Action::Hub(send) = action {
        return hub_slash(&state.config_dir, remote, app, send);
    }
    let op = match action {
        slash::Action::Op(op) => op,
        other => return slash::apply(other, &workspace_of(state, conversation_id)),
    };
    if let Some(done) = engine_op(state, &op, conversation_id)? {
        return Ok(done);
    }
    let bound = current(state).workspace;
    let done = slash_ops::run(
        op,
        &slash_ops::Ctx {
            config_dir: &state.config_dir,
            conversation_id,
            bound: &bound,
            owned,
        },
    )?;
    if let Some(path) = done.workspace {
        let next = ShellSettings {
            workspace: path,
            ..current(state)
        };
        settings::save(&state.config_dir, &next)?;
        *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next;
    }
    if let slash::Effect::Deleted { id, .. } = &done.effect {
        end_chat(state, id);
    }
    Ok(done.effect)
}

/// The slash commands that run the model on their own: `/send`, `/bg`,
/// `/consult`, and `/host`. Each is read only and unattended.
fn engine_op(
    state: &ShellState,
    op: &slash::Op,
    conversation_id: Option<&str>,
) -> Result<Option<slash::Effect>, String> {
    let notice = |text: String| Ok(Some(slash::Effect::Notice { text }));
    // Typed in a Code tab session, the work runs in that session's folder.
    let own_folder = conversation_id.and_then(|id| state.code_sessions.of(id));
    // Work started from a scratch chat runs as scratch too: no memory in, none out.
    let scratch = conversation_id.is_some_and(grokhub_agent::scratch::is_scratch);
    match op {
        slash::Op::Bg(arg) if arg.trim().is_empty() => notice(bg_runs::status()),
        slash::Op::Bg(arg) if arg.trim().eq_ignore_ascii_case("stop") => {
            notice(bg_runs::stop_text())
        }
        slash::Op::Bg(task) | slash::Op::Send(task) => {
            let settings = current(state);
            let route = route(&settings, agent::SessionMode::Chat, &state.config_dir)?;
            let title = bg_runs::start(
                state.config_dir.clone(),
                route.client,
                grokhub_agent::route::host::chat_start().0,
                own_folder.unwrap_or(route.workspace),
                task,
                scratch,
            )?;
            let sent = matches!(op, slash::Op::Send(_));
            notice(bg_runs::started_text(sent, &title))
        }
        slash::Op::Consult(question) => {
            let settings = current(state);
            let route = route(&settings, agent::SessionMode::Chat, &state.config_dir)?;
            let _config = grokhub_agent::perm::ConfigGuard::set(&state.config_dir);
            let done = bg_runs::ask_once(
                route.client,
                grokhub_core::model_for_mode("fast"),
                own_folder.unwrap_or(route.workspace),
                grokhub_agent::scratch::shell_id(
                    scratch,
                    &format!("consult-{}", grokhub_core::now_ms()),
                ),
                question.clone(),
                grokhub_agent::CancelToken::new(),
            );
            let _ = grokhub_agent::delete_session(&done.session_id);
            if !done.error.is_empty() && done.text.trim().is_empty() {
                return Err(done.error);
            }
            notice(grokhub_core::format_consult_reply(question, &done.text))
        }
        slash::Op::HostStatus => notice(match chat_client(&state.config_dir) {
            Ok(_) => format!(
                "{} is ready on this computer",
                grokhub_core::brand::product_name()
            ),
            Err(e) => e,
        }),
        _ => Ok(None),
    }
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

/// [`model_client`] for a chat turn. With no usable sign-in or key and either
/// Local AI or an added provider ready, the client has no xAI bearer and the
/// router answers on that route instead.
fn chat_client(config_dir: &Path) -> Result<(XaiClient, AuthKind), String> {
    match model_client() {
        Ok(found) => Ok(found),
        Err(_)
            if grokhub_agent::route::local::available()
                || !grokhub_agent::route::providers::usable(
                    config_dir,
                    grokhub_agent::route::providers::CALL_DATA,
                )
                .is_empty() =>
        {
            Ok((
                XaiClient::new(String::new(), AuthKind::ApiKey, Duration::from_secs(120)),
                AuthKind::ApiKey,
            ))
        }
        Err(err) => Err(err),
    }
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

/// One turn's route. The router alone picks each step's provider (xAI, a
/// provider you added, or Local AI), model and effort; this hands it the
/// cabin's Local AI switch, OpenRouter policy and spend limits first.
fn route(
    settings: &ShellSettings,
    session: agent::SessionMode,
    config_dir: &Path,
) -> Result<agent::Route, String> {
    grokhub_agent::route::host::sync(config_dir);
    let (client, auth_kind) = chat_client(config_dir)?;
    Ok(agent::Route {
        client: Arc::new(client),
        auth_kind,
        workspace: agent::workspace_for(&settings.workspace),
        mode: settings.approvals.mode(),
        desktop: settings.desktop_control && capabilities::current().desktop_control,
        session,
        voice: personality::voice(config_dir, settings),
        // A chat turn adds its voice settings (`chat_turn`); automations have none.
        voice_session: None,
        suggestions: settings.behavior.suggestions,
        // Only a session the agent window started leads (`chat_turn`).
        lead: false,
    })
}

/// An unattended run's gate from a turn's route: your Approvals, and the
/// desktop while control is on and no game holds it.
fn run_gate(route: &agent::Route) -> unattended::RunGate {
    let game = route.desktop.then(unattended::game_hold).flatten();
    unattended::RunGate::scheduled(route.mode, route.desktop, game.as_deref())
}

/// Runs one prompt through the agent loop. Text, tool cards and permission
/// asks arrive as `chat://event`; this resolves with the stop reason.
fn maybe_handle_bench_turn_to_disk(
    config_dir: &Path,
    conversation_id: &str,
    text: &str,
    cwd: &str,
) -> Result<Option<String>, String> {
    let (reg, _) = grokhub_agent::route::live::snapshot(config_dir);
    let known: Vec<&str> = reg.models.keys().map(String::as_str).collect();
    let Some(action) =
        grokhub_agent::route::user_bench::parse_chat_command_with_known(text, &known)
    else {
        return Ok(None);
    };
    let outcome = grokhub_agent::route::user_bench::apply_and_save(
        config_dir,
        action,
        grokhub_core::now_ms(),
    )?;
    agent::save_bench_exchange(conversation_id, cwd, text, &outcome.message)?;
    Ok(Some(outcome.message))
}

#[tauri::command]
async fn send_chat(
    app: AppHandle,
    state: State<'_, ShellState>,
    conversation_id: String,
    text: String,
    mode: Option<agent::SessionMode>,
    attachments: Option<Vec<String>>,
    clock: Option<automations::Clock>,
) -> Result<String, String> {
    set_last_chat(&state, &conversation_id);
    // A message you send ends a Halt's hold on the background passes.
    #[cfg(desktop)]
    night::note_send(&state.config_dir, grokhub_core::now_ms());
    // How long the composer's chip row stood before this message; off the
    // send's path.
    let timing = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = timing.state::<ShellState>();
        chips::next_step(
            &state.config_dir,
            &state.chip_rows,
            chips::COMPOSER,
            grokhub_core::now_ms(),
        );
    });
    // A chat opened from a Pulse card: its first message carries the card.
    let carry = state
        .card_carry
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&conversation_id);
    let text = match carry {
        Some(context) => format!("{}{text}", grokhub_core::card_report::card_carry(&context)),
        None => text,
    };
    // Focus help: one context line on the next turn after returning from an app.
    // A `/deep-research` line keeps its command first, so the line waits.
    #[cfg(desktop)]
    let text = {
        let on = pc_helpers::read(&state.config_dir).focus_help
            && !grokhub_agent::research::is_command(&text)
            && grokhub_agent::route::user_bench::parse_chat_command(&text).is_none();
        let focus = state
            .focus_help
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take_context(on);
        match focus {
            Some(line) => format!("{line}\n{text}"),
            None => text,
        }
    };
    let turn = Turn {
        text,
        mode,
        attachments: attachments.unwrap_or_default(),
        clock,
    };
    chat_turn(app, &state, conversation_id, turn, current(&state)).await
}

/// One message from the composer: its text, how it runs and attached files
/// (paths from `stage_attachment`).
struct Turn {
    text: String,
    /// `/plan` or a follow-up chip names it; otherwise the app reads the
    /// message and picks Chat or Plan (`turns::pick_mode`).
    mode: Option<agent::SessionMode>,
    attachments: Vec<String>,
    /// The page's local clock, for the day a finished step's XP lands on.
    clock: Option<automations::Clock>,
}

/// The chat the window has open, reopened at the next launch; empty for none.
/// A failed write only loses that, so chat goes on.
fn set_last_chat(state: &ShellState, conversation_id: &str) {
    // The Chat tab's own: a session in the Code tab's agent window is not
    // what Chat reopens.
    if state.code_sessions.holds(conversation_id) {
        return;
    }
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
        clock,
    } = turn;
    let files = state.trays.take(&conversation_id, &attachments)?;
    let own_folder = state.code_sessions.of(&conversation_id);
    if files.is_empty() {
        let bench_cwd = own_folder
            .clone()
            .unwrap_or_else(|| agent::workspace_for(&settings.workspace))
            .to_string_lossy()
            .into_owned();
        if let Some(reply) =
            maybe_handle_bench_turn_to_disk(&state.config_dir, &conversation_id, &text, &bench_cwd)?
        {
            let text_ev = agent::UiEvent::Text { text: reply };
            inbox::note(&conversation_id, &text_ev);
            let _ = app.emit(
                CHAT_EVENT,
                ChatEvent {
                    conversation_id: &conversation_id,
                    event: &text_ev,
                },
            );
            let stop = "end_turn".to_string();
            let done_ev = agent::UiEvent::Done { stop: stop.clone() };
            inbox::note(&conversation_id, &done_ev);
            let _ = app.emit(
                CHAT_EVENT,
                ChatEvent {
                    conversation_id: &conversation_id,
                    event: &done_ev,
                },
            );
            return Ok(stop);
        }
    }
    let asked = text.clone();
    // A card's chat shows Stop on its card while this turn runs.
    card_chat::turn_began(&conversation_id);
    let (ended_dir, ended_chat) = (state.config_dir.clone(), conversation_id.clone());
    // A session the agent window started, while it shows it, has a lead: it
    // plans first, until its first plan is approved.
    let lead = state.code_sessions.holds(&conversation_id)
        && lead::is_lead(&state.config_dir, &conversation_id);
    let approved = lead && lead::approved(&state.config_dir, &conversation_id);
    let mode = lead::first_mode(mode, lead, approved)
        .unwrap_or_else(|| turns::pick_mode(state, &conversation_id, &text));
    let session = session(state, &conversation_id);
    let config_dir = state.config_dir.clone();
    // A Code tab session works in its own project's folder, not the app's.
    // Its agent pushes through the Code tab's push guard and codes with the
    // bundled skills always on.
    let coding = own_folder.is_some();
    code_agent::bind(&state.config_dir, &conversation_id, coding);
    // A scratch chat feeds nothing that outlives it: no taste note, no
    // repeated-ask offers, no task outcomes.
    let scratch = grokhub_agent::scratch::is_scratch(&conversation_id);
    if capabilities::current().pulse && !scratch {
        // What the person says in chat steers the Feed writer's taste note.
        pulse_feed::note_chat(&config_dir, &text, grokhub_core::now_ms());
        // Typed asks also feed the repeated-ask automate offers.
        pulse_digests::note_ask(&config_dir, &text, grokhub_core::now_ms());
    }
    // "That was wrong" fails the chat's last task before this turn runs; the
    // turn's own outcome is written when it ends (as the cabin does).
    let started = grokhub_core::now_ms();
    if !scratch {
        chat_trials::said(&config_dir, &conversation_id, &text, started);
    }
    let stop = tauri::async_runtime::spawn_blocking(move || {
        let mut route = route(&settings, mode, &config_dir)?;
        // A resumed card's turn runs no looser than its run started.
        route.mode = card_chat::turn_mode(&conversation_id, route.mode);
        if let Some(dir) = own_folder {
            route.workspace = dir;
        }
        route.lead = lead;
        // Dynamic chat and quips step per turn from this chat's voice settings,
        // at the page's local hour (UTC when the page sent no clock).
        let offset = clock.map_or(0, |c| c.at(grokhub_core::now_ms()).utc_offset_min());
        route.voice_session = Some(personality::voice_config(&config_dir, &settings, offset));
        let id = conversation_id.clone();
        // The episode panel follows the chat's open episode while it runs.
        let halt_watch = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watcher = route.desktop.then(|| {
            let (app, session, stop) = (app.clone(), Arc::clone(&session), Arc::clone(&halt_watch));
            let (config_dir, chat) = (config_dir.clone(), conversation_id.clone());
            std::thread::spawn(move || {
                episode::watch(
                    &stop,
                    &config_dir,
                    &chat,
                    &|| session.episode(),
                    &|| session.frame(),
                    &mut |panel| {
                        let _ = app.emit(episode::EPISODE_EVENT, panel);
                    },
                );
            })
        });
        let resume_to = app.clone();
        let stop = session.prompt(
            &text,
            files,
            route,
            Box::new(move |event| {
                inbox::note(&id, &event);
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
        );
        halt_watch.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(watcher) = watcher {
            let _ = watcher.join();
        }
        // An Allow that landed after the session's last look at its cards:
        // the page sends the resume, so the step runs now and not with
        // whatever message comes next.
        if let Some(open) = session.episode().filter(|_| session.take_resume_owed()) {
            let panel =
                episode::resume_panel(&config_dir, &conversation_id, &open, session.frame());
            let _ = resume_to.emit(episode::EPISODE_EVENT, &panel);
        }
        if stop
            .as_deref()
            .is_ok_and(|s| s != agent::HALTED && s != "error")
        {
            // A chat's finished checklist moves the XP bar here too.
            let now = grokhub_core::now_ms();
            let offset = clock.map_or(0, |c| c.at(now).utc_offset_min());
            home::turn_done_for(&config_dir, &conversation_id, now, offset);
            // The turn's task outcome and run metrics, for the weekly
            // self-review and skill trials.
            if !scratch {
                chat_trials::turn_end(&config_dir, &conversation_id, &text, started, now);
            }
        }
        stop
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|stop| stop);
    // A card's run settles on its card, however the turn ended.
    let card = card_chat::turn_ended(&ended_dir, &ended_chat, &stop);
    // No model in your plan answered: the chat waits and sends again on its
    // own. A card's chat does not: its card settled already.
    if let (Err(err), false) = (&stop, card) {
        model_wait::turn_failed(&ended_dir, &ended_chat, &asked, err, grokhub_core::now_ms());
    }
    if coding {
        let why = match &stop {
            Err(err) => Some(err.as_str()),
            Ok(s) if s == "error" => Some(code_keep::TURN_ERROR),
            Ok(_) => None,
        };
        code_keep::record(&ended_dir, &ended_chat, why);
    }
    stop
}

/// A chat that paused because no model in your plan answered, once one is
/// back: its ask to send again and the quiet note, at most once. The page
/// asks when the chat is open and idle, and when the router says one is back.
#[tauri::command]
fn model_resume(conversation_id: String) -> Option<model_wait::Resume> {
    model_wait::take_resume(&conversation_id)
}

/// The budget card over the chat box, if one is up: 80% of the week, the
/// ask at 100% or the upgrade nudge.
#[tauri::command]
fn budget_card() -> Option<spend_cards::CardView> {
    spend_cards::current()
}

/// A click on the budget card.
#[tauri::command]
async fn answer_budget_card(
    state: State<'_, ShellState>,
    key: String,
    primary: bool,
) -> Result<spend_cards::Answered, String> {
    let dir = state.config_dir.clone();
    off_main(move || spend_cards::answer(&dir, &key, primary, grokhub_core::now_ms())).await?
}

/// Every minute: this week's spend against the cap and the nudge's facts,
/// for the budget card.
fn start_budget_tally(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(
            grokhub_agent::route::budget::BUDGET_EVERY_MS,
        ));
        let state = app.state::<ShellState>();
        let now = grokhub_core::now_ms();
        let in_task = state
            .chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .any(|s| s.busy());
        let config_dir = state.config_dir.clone();
        let gate = grokhub_agent::route::budget::NudgeGate {
            in_task,
            quiet_hours: quiet_now(&config_dir, now),
            throttled: proactive::load_budget(&config_dir).room(now) == 0,
        };
        spend_cards::tally(&config_dir, gate, now);
    });
}

/// Behavior's quiet hours at the window's clock; never quiet before it sent one.
fn quiet_now(config_dir: &Path, now: u64) -> bool {
    #[cfg(desktop)]
    if let Some(offset) = scheduler::utc_offset() {
        let clock = scheduler::local_clock(now, offset);
        let behavior = settings::load(config_dir).behavior;
        return proactive::is_quiet(&behavior, clock.hour, clock.minute);
    }
    let _ = (config_dir, now);
    false
}

/// Push to talk: one recording (a WAV, as base64) made in `conversation_id`
/// to text for the draft, with the line naming the engine that heard it.
/// Nothing is sent to chat; the person presses Send.
#[tauri::command]
async fn transcribe(
    app: AppHandle,
    state: State<'_, ShellState>,
    wav: String,
    conversation_id: Option<String>,
) -> Result<voice::Transcript, String> {
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
    let chat = conversation_id.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        voice::transcribe_with(Some(&app), &config_dir, &chat, &bytes)
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
    inbox::answered(&conversation_id, &call_id);
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
    inbox::answered(&conversation_id, &id);
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

/// Helper questions a chat still waits on, for when the chat opens.
#[tauri::command]
fn helper_questions(conversation_id: String) -> Vec<agent::UiEvent> {
    helper_asks::waiting(&conversation_id)
}

/// Every decision waiting on the person, in one list: the decision inbox.
#[tauri::command]
fn decision_inbox(state: State<'_, ShellState>) -> Vec<inbox::Decision> {
    inbox::decisions(&state.config_dir)
}

/// Answers one inbox decision through the same path as its card. A
/// permission takes Allow or Deny (the inbox never offers Always); an
/// elicit's Approve opens its chat instead — the page returns
/// `open:<chat id>` and answers nothing — and its Deny cancels the ask; a
/// park is answered in the harness park store.
#[tauri::command]
fn answer_inbox(
    app: AppHandle,
    state: State<'_, ShellState>,
    conversation_id: String,
    id: String,
    kind: String,
    approve: bool,
) -> Result<String, String> {
    match kind.as_str() {
        "park" => {
            grokhub_agent::harness::answer_park(&state.config_dir, &id, approve)?;
            Ok("answered".to_string())
        }
        "permission" => {
            let answer = if approve {
                grokhub_agent::PermAnswer::Allow
            } else {
                grokhub_agent::PermAnswer::Deny
            };
            inbox::answered(&conversation_id, &id);
            let session = state
                .chats
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&conversation_id)
                .cloned()
                .ok_or_else(|| "That chat is closed.".to_string())?;
            #[cfg(desktop)]
            os_extras::answered_in_window(&app, &conversation_id, &id, || {
                session.answer(&id, answer)
            });
            #[cfg(mobile)]
            {
                let _ = app;
                session.answer(&id, answer);
            }
            Ok("answered".to_string())
        }
        "elicit" => {
            if approve {
                return Ok(format!("open:{conversation_id}"));
            }
            inbox::answered(&conversation_id, &id);
            let chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
            let session = chats
                .get(&conversation_id)
                .ok_or_else(|| "That chat is closed.".to_string())?;
            session.answer_elicit(&id, "cancel", None, "");
            Ok("answered".to_string())
        }
        _ => Err(format!("unknown decision kind: {kind}")),
    }
}

/// Remembers a picked reply chip, so the next waiting card ranks it higher.
#[tauri::command]
fn reply_pick(state: State<'_, ShellState>, key: String) {
    agent::note_reply_pick(&state.config_dir, &key);
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

/// Steer now from the Activity menu: the queued words join the running
/// reply at its next step. False when that chat has no reply running.
#[tauri::command]
fn steer_chat(state: State<'_, ShellState>, conversation_id: String, text: String) -> bool {
    // A research line needs a turn of its own, so it stays in line.
    if grokhub_agent::research::is_command(&text) {
        return false;
    }
    state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
        .is_some_and(|session| session.steer(&text))
}

/// Background work waits while a chat is running or a decision waits for
/// you.
#[cfg(desktop)]
fn background_busy(app: &tauri::AppHandle) -> bool {
    let state = app.state::<ShellState>();
    let chatting = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .any(|s| s.busy());
    chatting || !inbox::decisions(&state.config_dir).is_empty()
}

/// Halt from the top bar, Ctrl+Alt+H, the tray or Super+Shift+Esc: stops
/// desktop control and every running prompt (`agent::halt`), then tells the
/// window, which shows it.
#[cfg(desktop)]
fn halt_now(app: &AppHandle) -> Vec<String> {
    let state = app.state::<ShellState>();
    // Halt ends a screen recording too, and its report sends nothing more.
    screen_record::halt();
    let (busy, open) = {
        let chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
        // Halt ends an idle chat's desktop session itself; the page is told.
        let open = open_episodes(&chats);
        (agent::halt(&state.config_dir, chats.values()), open)
    };
    let _ = app.emit(HALT_EVENT, &busy);
    emit_ended(app, open, "halt");
    busy
}

/// A chat's open desktop session, with the chat it is on.
#[cfg(desktop)]
type OpenEpisode = (String, Arc<agent::Session>, episode::Open);

/// The chats with a desktop session open right now.
#[cfg(desktop)]
fn open_episodes(chats: &HashMap<String, Arc<agent::Session>>) -> Vec<OpenEpisode> {
    chats
        .iter()
        .filter_map(|(id, session)| Some((id.clone(), Arc::clone(session), session.episode()?)))
        .collect()
}

/// Tells the page about the sessions among `open` that were ended with no
/// loop running (a running loop's watcher says so itself), as `why`. The
/// spans are read off the caller's thread.
#[cfg(desktop)]
fn emit_ended(app: &AppHandle, open: Vec<OpenEpisode>, why: &'static str) {
    if open.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<ShellState>();
        for (chat, session, open) in open {
            if session.episode().is_some() {
                continue;
            }
            let panel = episode::ended_panel(&state.config_dir, &chat, &open, session.frame(), why);
            let _ = app.emit(episode::EPISODE_EVENT, &panel);
        }
    });
}

/// Settings turned desktop control on or off. Off stops every chat's desktop
/// session (a running one is cancelled, an idle one is closed and its cards
/// denied), disarms the chats, and refuses the later desktop calls of a turn
/// that started with the switch on.
#[cfg(desktop)]
fn desktop_switched(app: &AppHandle, state: &ShellState, on: bool) {
    let chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
    if on {
        chats.values().for_each(|session| session.desktop_on());
        return;
    }
    let open = open_episodes(&chats);
    chats.values().for_each(|session| {
        session.desktop_off();
    });
    drop(chats);
    emit_ended(app, open, "stop");
}

/// The top bar's recording indicator, the audio check's line and any run that
/// just finished (each handed over once). The window asks every second.
#[tauri::command]
fn record_status() -> screen_record::Status {
    screen_record::status()
}

/// The indicator's Stop. True when a recording was running.
#[tauri::command]
fn record_stop() -> bool {
    screen_record::stop()
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

/// The chat's open desktop episode as the panel draws it now, or `None`.
/// Desktop only, like Halt: a browser never sees the computer's screen.
/// The spans are read off the main thread.
#[tauri::command]
async fn episode_state(
    state: State<'_, ShellState>,
    conversation_id: String,
) -> Result<Option<episode::Panel>, String> {
    let session = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
        .cloned();
    let Some(open) = session.as_ref().and_then(|session| session.episode()) else {
        return Ok(None);
    };
    let frame = session.and_then(|session| session.frame());
    let config_dir = state.config_dir.clone();
    off_main(move || {
        Some(episode::panel(
            &config_dir,
            &conversation_id,
            &open.id,
            open.started_ms,
            grokhub_core::now_ms(),
            frame,
            open.paused,
            open.cua,
        ))
    })
    .await
}

/// Arms or disarms the supervised desktop session for a chat: its next
/// messages, sent with desktop control on, run as an episode. Returns whether
/// it is armed now.
#[tauri::command]
fn episode_arm(state: State<'_, ShellState>, conversation_id: String, on: bool) -> bool {
    let session = session(&state, &conversation_id);
    session.arm_episode(on);
    session.episode_armed()
}

/// Whether the chat's supervised desktop session is armed or open.
#[tauri::command]
fn episode_armed(state: State<'_, ShellState>, conversation_id: String) -> bool {
    state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
        .is_some_and(|s| s.episode_armed() || s.episode().is_some())
}

/// OK or Deny on a card the episode parked. True when the window should
/// send the resume ([`agent::RESUME_TEXT`]) now: an Allow with no reply
/// running, which no loop would pick up until the next message. The page is
/// told where the episode stands (the watcher only runs with a prompt).
#[tauri::command]
async fn episode_answer(
    app: AppHandle,
    state: State<'_, ShellState>,
    conversation_id: String,
    park_id: String,
    approve: bool,
) -> Result<bool, String> {
    let session = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
        .cloned()
        .ok_or_else(|| "That chat is closed.".to_string())?;
    let config_dir = state.config_dir.clone();
    off_main(move || {
        let resume = session.answer_episode(&park_id, approve)?;
        if let Some(open) = session.episode() {
            let panel = episode::panel(
                &config_dir,
                &conversation_id,
                &open.id,
                open.started_ms,
                grokhub_core::now_ms(),
                session.frame(),
                open.paused,
                open.cua,
            );
            let _ = app.emit(episode::EPISODE_EVENT, &panel);
        }
        Ok(resume)
    })
    .await?
}

/// Pause the open episode after the step it is on; the next message runs it
/// again. False when none is running.
#[tauri::command]
fn episode_pause(state: State<'_, ShellState>, conversation_id: String) -> bool {
    state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
        .is_some_and(|session| session.pause_episode())
}

/// Stop the open episode: nothing more runs and every waiting card is denied.
#[tauri::command]
async fn episode_stop(
    app: AppHandle,
    state: State<'_, ShellState>,
    conversation_id: String,
) -> Result<bool, String> {
    let session = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
        .cloned();
    let Some(session) = session else {
        return Ok(false);
    };
    let Some(open) = session.episode() else {
        return Ok(false);
    };
    let config_dir = state.config_dir.clone();
    off_main(move || {
        let stopped = session.stop_episode();
        // A running loop ends it and its watcher says so; with none running, say it here.
        if stopped {
            let panel = episode::ended_panel(
                &config_dir,
                &conversation_id,
                &open,
                session.frame(),
                "stop",
            );
            let _ = app.emit(episode::EPISODE_EVENT, &panel);
        }
        stopped
    })
    .await
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
    drop_worker(state, conversation_id);
    // Drop the saved folder first, so a session with no turns can leave an
    // empty directory for `closed` to remove.
    let coding = state.code_sessions.holds(conversation_id);
    state.code_sessions.forget(conversation_id);
    if coding {
        lead::closed(&state.config_dir, conversation_id);
    }
    code_agent::bind(&state.config_dir, conversation_id, false);
    if current(state).last_chat == conversation_id {
        set_last_chat(state, "");
    }
    state.trays.forget(conversation_id);
}

/// Ends the chat's worker only: the chat stays open and keeps its tray, and
/// its next message starts a worker that resumes from the saved file.
fn drop_worker(state: &ShellState, conversation_id: &str) {
    let removed = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(conversation_id);
    if let Some(session) = removed {
        session.cancel();
    }
    helper_asks::chat_ended(conversation_id);
    inbox::chat_ended(conversation_id);
}

/// The input kinds of the model a chat starts at, for the chips' notes.
fn model_mods(state: &ShellState) -> Vec<String> {
    attach::modalities(
        &state.config_dir,
        &grokhub_agent::route::host::chat_start().0,
    )
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

/// A saved chat folded into topic groups for the zoomed-out view.
#[tauri::command]
async fn chat_zoom(state: State<'_, ShellState>, id: String) -> Result<chat_zoom::Zoom, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_zoom::zoom(&dir, &id)).await?
}

/// The chat the Chat tab opens at launch: Main, made first if there is none.
#[tauri::command]
async fn get_start_chat(state: State<'_, ShellState>) -> Result<history::Transcript, String> {
    let dir = state.config_dir.clone();
    let shown = off_main(move || {
        let id = chat_tabs::ensure_main(&dir, grokhub_core::now_ms())?;
        history::load(&dir, &id, history::LOAD_CAP)
    })
    .await??;
    set_last_chat(&state, &shown.id);
    Ok(shown)
}

/// Clear chat: a running reply stops and the chat's saved history goes.
/// Main always stays, so clearing it swaps in an empty Main and returns it;
/// any other chat returns nothing.
#[tauri::command]
async fn clear_chat(
    state: State<'_, ShellState>,
    id: String,
) -> Result<Option<history::Transcript>, String> {
    let dir = state.config_dir.clone();
    let doomed = id.clone();
    let fresh = off_main(move || {
        if chat_tabs::main_id(&dir).as_deref() == Some(doomed.trim()) {
            let main = chat_tabs::clear_main(&dir, grokhub_core::now_ms())?;
            history::load(&dir, &main, history::LOAD_CAP).map(Some)
        } else {
            history::delete(&dir, &doomed).map(|()| None)
        }
    })
    .await??;
    end_chat(&state, &id);
    if let Some(shown) = &fresh {
        set_last_chat(&state, &shown.id);
    }
    Ok(fresh)
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

/// Tab strip: live chats (not archived / not soft-deleted).
#[tauri::command]
async fn list_chat_tabs(state: State<'_, ShellState>) -> Result<Vec<chat_tabs::ChatTab>, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::list_tabs(&dir)).await
}

#[tauri::command]
async fn list_archived_chats(
    state: State<'_, ShellState>,
) -> Result<Vec<chat_tabs::ChatTab>, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::list_archived(&dir)).await
}

#[tauri::command]
async fn list_deleted_chats(
    state: State<'_, ShellState>,
) -> Result<Vec<chat_tabs::ChatTab>, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::list_deleted(&dir)).await
}

#[tauri::command]
async fn chat_tab_switch(
    state: State<'_, ShellState>,
    id: String,
) -> Result<chat_tabs::ChatTab, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::switch_tab(&dir, &id)).await?
}

#[tauri::command]
async fn chat_tab_pin(state: State<'_, ShellState>, id: String) -> Result<bool, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::pin_tab(&dir, &id)).await?
}

#[tauri::command]
async fn chat_tab_rename(
    state: State<'_, ShellState>,
    id: String,
    title: String,
) -> Result<(), String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::rename_tab(&dir, &id, &title)).await?
}

#[tauri::command]
async fn chat_tab_archive(
    state: State<'_, ShellState>,
    id: String,
) -> Result<chat_tabs::ChatTab, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::archive_tab(&dir, &id, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn chat_tab_unarchive(
    state: State<'_, ShellState>,
    id: String,
) -> Result<chat_tabs::ChatTab, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::unarchive_tab(&dir, &id)).await?
}

#[tauri::command]
async fn chat_tab_soft_delete(
    state: State<'_, ShellState>,
    id: String,
) -> Result<chat_tabs::ChatTab, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::soft_delete_tab(&dir, &id, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn chat_tab_restore_deleted(
    state: State<'_, ShellState>,
    id: String,
) -> Result<chat_tabs::ChatTab, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::restore_deleted_tab(&dir, &id)).await?
}

#[tauri::command]
async fn get_hangout_mode(state: State<'_, ShellState>) -> Result<chat_tabs::HangoutMode, String> {
    let dir = state.config_dir.clone();
    off_main(move || chat_tabs::get_hangout_mode(&dir)).await
}

#[tauri::command]
async fn set_hangout_mode(
    state: State<'_, ShellState>,
    mode: String,
) -> Result<chat_tabs::HangoutMode, String> {
    let dir = state.config_dir.clone();
    let parsed = chat_tabs::HangoutMode::parse(&mode)?;
    off_main(move || chat_tabs::set_hangout_mode(&dir, parsed)).await?
}

/// Where History's Delete all is armed; the second click inside its window
/// deletes (`history::delete_all`).
static DELETE_ALL_ARM: Mutex<history::DeleteArm> = Mutex::new(history::DeleteArm::new());

/// Delete all: the first click arms it, a second within 5 s (and not within
/// half a second, a double-click's second half) deletes the chats of this
/// app the first click counted, and answers with their ids. Replies still
/// running stop. When a chat can't be deleted, the ones before it are gone
/// and still named, with the reason in `error`, so their replies stop too.
#[tauri::command]
async fn delete_all_chats(state: State<'_, ShellState>) -> Result<history::DeleteAll, String> {
    let dir = state.config_dir.clone();
    let done =
        off_main(move || history::delete_all(&dir, &DELETE_ALL_ARM, std::time::Instant::now()))
            .await??;
    for id in &done.ids {
        end_chat(&state, id);
    }
    Ok(done)
}

/// Cancel on the armed Delete all, or its window lapsing on the page.
#[tauri::command]
fn cancel_delete_all() {
    DELETE_ALL_ARM
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .cancel();
}

/// The sidebar's projects and folders, for History's Projects tree.
#[tauri::command]
async fn list_project_nodes(
    state: State<'_, ShellState>,
) -> Result<Vec<grokhub_core::ProjectNode>, String> {
    let dir = state.config_dir.clone();
    off_main(move || history::project_nodes(&dir)).await?
}

/// Pins the chat, or unpins it when it is pinned; true when pinned now.
#[tauri::command]
async fn pin_chat(state: State<'_, ShellState>, id: String) -> Result<bool, String> {
    let dir = state.config_dir.clone();
    off_main(move || history::toggle_pin(&dir, &id)).await?
}

/// Grok CLI sessions and the cabin's chats, read only.
#[tauri::command]
async fn list_read_only_chats(
    state: State<'_, ShellState>,
    query: String,
) -> Result<Vec<history::ReadOnlyRow>, String> {
    let dir = state.config_dir.clone();
    off_main(move || history::read_only(&dir, None, &query)).await
}

/// One of those chats' newest lines, shown in place; never resumed.
#[tauri::command]
async fn view_read_only_chat(
    state: State<'_, ShellState>,
    id: String,
) -> Result<history::Transcript, String> {
    let dir = state.config_dir.clone();
    off_main(move || history::view_read_only(&dir, None, &id, history::LOAD_CAP)).await?
}

/// Tool steps a History search matched, over this app's chats.
#[tauri::command]
async fn search_steps(
    state: State<'_, ShellState>,
    query: String,
) -> Result<Vec<history::StepRow>, String> {
    let dir = state.config_dir.clone();
    off_main(move || history::step_search(&dir, &query)).await
}

/// A bubble's Delete message…, after the page asked. The ask `text` goes
/// alone; with `reply` its reply goes whole. A reply still running refuses.
/// The idle worker ends, so the next message resumes from the trimmed file.
#[tauri::command]
async fn forget_message(
    state: State<'_, ShellState>,
    id: String,
    reply: bool,
    text: Option<String>,
    later: usize,
) -> Result<bool, String> {
    let dir = state.config_dir.clone();
    let key = id.clone();
    let removed =
        off_main(move || history::forget(&dir, &key, reply, text.as_deref(), later)).await??;
    // A chat on the Claude plan: Claude Code's own session still has it.
    if removed {
        claude_chat::forget_session(&state.config_dir, &id)?;
    }
    drop_worker(&state, &id);
    Ok(removed)
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

/// Tasks → the checks on the session's change before it can be committed.
/// Read only: nothing is committed, staged or pushed. `message` and
/// `allowed` are what the Commit card holds: the confirm text is for that
/// message, and the flagged lines the person allowed do not block. Desktop
/// only: it reads the project folder and the keychain, and is not a web
/// command.
#[tauri::command]
async fn ship_status(
    state: State<'_, ShellState>,
    conversation_id: String,
    message: Option<String>,
    allowed: Option<Vec<String>>,
) -> Result<code_commit::ShipStatus, String> {
    let dir = workspace_of(&state, Some(&conversation_id));
    let config = state.config_dir.clone();
    let cfg = current(&state).code_ship;
    let draft = code_commit::Draft {
        message,
        allowed: allowed.unwrap_or_default(),
    };
    off_main(move || ship_gate::status(&config, &dir, &conversation_id, &cfg, &draft)).await
}

/// Tasks → Commit: the session's change becomes one commit on its branch.
/// Every check runs again; `confirmed` must be true and `command` must be
/// the exact text the confirm showed. Desktop only: it is not a web command.
#[tauri::command]
async fn ship_commit(
    state: State<'_, ShellState>,
    conversation_id: String,
    message: String,
    allowed: Vec<String>,
    confirmed: bool,
    command: Option<String>,
) -> Result<code_commit::Committed, String> {
    let dir = workspace_of(&state, Some(&conversation_id));
    let config = state.config_dir.clone();
    let cfg = current(&state).code_ship;
    let draft = code_commit::Draft {
        message: Some(message),
        allowed,
    };
    off_main(move || {
        ship_gate::commit(
            &config,
            &dir,
            &conversation_id,
            &cfg,
            &draft,
            confirmed,
            command.as_deref(),
        )
    })
    .await?
}

/// Tasks → Waive: the person ships with one review blocker open. `tree` and
/// `at_ms` are the tree id and the time of the review the page showed.
/// Desktop only.
#[tauri::command]
async fn ship_waive(
    state: State<'_, ShellState>,
    conversation_id: String,
    tree: String,
    at_ms: u64,
    index: u32,
) -> Result<queue::Queue, String> {
    let dir = workspace_of(&state, Some(&conversation_id));
    let config = state.config_dir.clone();
    let cfg = current(&state).code_ship;
    off_main(move || {
        ship_gate::waive(&config, &dir, &conversation_id, &cfg, &tree, at_ms, index)?;
        Ok(queue::queue(&config, &conversation_id))
    })
    .await?
}

/// Tasks → Merge: a helper's work goes into the project folder. Desktop
/// only: it is not a web command.
#[tauri::command]
async fn merge_task_work(
    state: State<'_, ShellState>,
    conversation_id: String,
    key: String,
) -> Result<queue::Queue, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        queue::merge_work(&dir, &conversation_id, &key)?;
        Ok(queue::queue(&dir, &conversation_id))
    })
    .await?
}

/// Tasks → Finish merge (`finish`) or Put files back. Desktop only.
#[tauri::command]
async fn resume_task_work(
    state: State<'_, ShellState>,
    conversation_id: String,
    key: String,
    finish: bool,
) -> Result<queue::Queue, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        queue::resume_work(&dir, &conversation_id, &key, finish)?;
        Ok(queue::queue(&dir, &conversation_id))
    })
    .await?
}

/// Tasks → Discard or Delete worktree. Desktop only.
#[tauri::command]
async fn discard_task_work(
    state: State<'_, ShellState>,
    conversation_id: String,
    key: String,
) -> Result<queue::Queue, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        queue::discard_work(&dir, &conversation_id, &key)?;
        Ok(queue::queue(&dir, &conversation_id))
    })
    .await?
}

/// Tasks → Details: a helper's steps so far and its result. Reading it
/// leaves the result waiting for the session that started the helper.
#[tauri::command]
async fn get_task_detail(
    conversation_id: String,
    task_id: String,
) -> Result<queue::TaskDetail, String> {
    off_main(move || queue::detail(&conversation_id, &task_id)).await?
}

fn now_clock(clock: automations::Clock) -> grokhub_core::LocalClock {
    clock.at(grokhub_core::now_ms())
}

/// The top bar's "Quiet until HH:MM" chip while Settings → Behavior's quiet
/// hours hold right now on the page's clock; `None` outside them or with none set.
#[tauri::command]
fn quiet_chip(state: State<'_, ShellState>, clock: automations::Clock) -> Option<String> {
    quiet_chip_at(&current(&state).behavior, &now_clock(clock).hm())
}

fn quiet_chip_at(behavior: &settings::Behavior, now_hm: &str) -> Option<String> {
    if behavior.quiet_start.is_empty() {
        return None;
    }
    grokhub_core::quiet_until_chip(now_hm, &behavior.quiet_start, &behavior.quiet_end)
}

/// The chat's transcript (without its chips blocks) and its follow-ups
/// while they still belong to its last reply, which rank in its row (a chat
/// opened from History, or the last chat after a restart, keeps them). A
/// chat with no file has none.
fn chip_chat(dir: &Path, id: &str) -> (Vec<(String, String)>, Option<chips::FollowUps>) {
    let _config = grokhub_agent::perm::ConfigGuard::set(dir);
    let Ok(info) = grokhub_agent::load_session(id) else {
        return (Vec::new(), None);
    };
    let pairs = chips::chat_pairs(&info);
    let ups = chips::follow_ups(dir, id, chips::user_turns(&pairs));
    (pairs, ups)
}

/// The quick chips for the composer (`conversation_id` the open chat, with
/// the box's `draft`), for Home, or for the empty palette. The cabin's engine, memory and ranker;
/// empty with Settings → Behavior → Suggestions off.
fn chip_row(
    state: &ShellState,
    surface: &str,
    conversation_id: Option<String>,
    draft: &str,
    mode: agent::SessionMode,
    clock: automations::Clock,
    remote: bool,
) -> Vec<chips::Chip> {
    let settings = current(state);
    if !settings.behavior.suggestions {
        return Vec::new();
    }
    let surface = match surface {
        chips::HOME => chips::HOME,
        chips::PALETTE => chips::PALETTE,
        chips::CODE => chips::CODE,
        _ => chips::COMPOSER,
    };
    let dir = &state.config_dir;
    let (chat, ups) = match (&conversation_id, surface) {
        (Some(id), chips::COMPOSER | chips::CODE) => chip_chat(dir, id),
        _ => (Vec::new(), None),
    };
    let follow = ups.map(|u| u.chips).unwrap_or_default();
    let local = now_clock(clock);
    let quiet = !settings.behavior.quiet_start.is_empty()
        && grokhub_core::quiet_hours_active(
            &local.hm(),
            &settings.behavior.quiet_start,
            &settings.behavior.quiet_end,
        );
    let first_run = chat.is_empty() && history::list(dir).is_empty();
    #[cfg(desktop)]
    let focus_extra: Vec<_> = {
        let on = pc_helpers::read(dir).focus_help;
        state
            .focus_help
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .chip(on)
            .into_iter()
            .collect()
    };
    #[cfg(not(desktop))]
    let focus_extra: Vec<grokhub_core::QuickChip> = Vec::new();
    let game_extra = game_companion::row_extra(
        &game_companion::lock(&state.game_session),
        surface,
        remote,
        local.now_ms,
    );
    // The Code agent's row gets no game or focus chips.
    let extra: Vec<grokhub_core::QuickChip> = match surface {
        chips::COMPOSER => game_extra.into_iter().chain(focus_extra).collect(),
        chips::CODE => Vec::new(),
        _ => game_extra,
    };
    chips::row(
        dir,
        &state.chip_rows,
        chips::RowAsk {
            surface,
            chat: &chat,
            draft: if matches!(surface, chips::COMPOSER | chips::CODE) {
                draft
            } else {
                ""
            },
            follow_ups: &follow,
            mode: mode.as_str(),
            ai_ready: ai_ready(dir),
            first_run,
            hour: local.hour as u8,
            weekday: local.weekday,
            quiet_hours: quiet,
            now_ms: local.now_ms,
            extra: &extra,
            lane: settings.behavior.lane,
            remote,
        },
    )
}

#[tauri::command]
async fn quick_chips(
    app: AppHandle,
    surface: String,
    conversation_id: Option<String>,
    draft: Option<String>,
    mode: Option<agent::SessionMode>,
    clock: automations::Clock,
) -> Result<Vec<chips::Chip>, String> {
    off_main(move || {
        let state = app.state::<ShellState>();
        chip_row(
            &state,
            &surface,
            conversation_id,
            &draft.unwrap_or_default(),
            mode.unwrap_or_default(),
            clock,
            false,
        )
    })
    .await
}

/// A tap (`picked`) or a hide on a chip of the row `surface` last showed.
/// Teaches the ranker and the habits; the page acts on the tap itself.
#[tauri::command]
async fn chip_outcome(
    app: AppHandle,
    surface: String,
    id: String,
    picked: bool,
    clock: automations::Clock,
) -> Result<bool, String> {
    chip_outcome_from(app, surface, id, picked, clock, false).await
}

/// [`chip_outcome`] for the desktop or, `remote`, a paired browser: each
/// acts on the row it was shown.
async fn chip_outcome_from(
    app: AppHandle,
    surface: String,
    id: String,
    picked: bool,
    clock: automations::Clock,
    remote: bool,
) -> Result<bool, String> {
    off_main(move || {
        let state = app.state::<ShellState>();
        let local = now_clock(clock);
        chips::outcome(
            &state.config_dir,
            &state.chip_rows,
            &surface,
            &id,
            picked,
            (local.hour as u8, local.weekday, local.now_ms),
            remote,
        )
        .is_some()
    })
    .await
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

/// The Automations page's Routines list.
#[cfg(desktop)]
#[tauri::command]
fn list_routines(state: State<'_, ShellState>) -> Vec<routines_page::Row> {
    routines_page::rows(&state.config_dir)
}

#[cfg(desktop)]
#[tauri::command]
fn set_routine_on(
    state: State<'_, ShellState>,
    name: String,
    on: bool,
) -> Result<Vec<routines_page::Row>, String> {
    grokhub_agent::routine_triggers::set_on(&state.config_dir, &name, on)?;
    Ok(routines_page::rows(&state.config_dir))
}

/// Allow on a routine's row: its change steps, as the row showed them, may
/// run unattended.
#[cfg(desktop)]
#[tauri::command]
fn allow_routine(
    state: State<'_, ShellState>,
    name: String,
    hash: String,
) -> Result<Vec<routines_page::Row>, String> {
    routines_page::allow(&state.config_dir, &name, &hash)?;
    Ok(routines_page::rows(&state.config_dir))
}

#[cfg(desktop)]
#[tauri::command]
fn revoke_routine(
    state: State<'_, ShellState>,
    name: String,
) -> Result<Vec<routines_page::Row>, String> {
    routines_page::revoke(&state.config_dir, &name)?;
    Ok(routines_page::rows(&state.config_dir))
}

/// Apply on a routine's suggested fix.
#[cfg(desktop)]
#[tauri::command]
fn apply_routine_fix(state: State<'_, ShellState>, name: String) -> Result<String, String> {
    routines_page::apply_fix(&state.config_dir, &name)
}

#[cfg(desktop)]
#[tauri::command]
fn dismiss_routine_fix(
    state: State<'_, ShellState>,
    name: String,
) -> Result<Vec<routines_page::Row>, String> {
    routines_page::dismiss_fix(&state.config_dir, &name)?;
    Ok(routines_page::rows(&state.config_dir))
}

/// Undo on a routine's row: the newest saved change to it goes back.
#[cfg(desktop)]
#[tauri::command]
fn undo_routine(state: State<'_, ShellState>, name: String) -> Result<String, String> {
    routines_page::undo(&state.config_dir, &name)
}

#[cfg(desktop)]
#[tauri::command]
async fn run_routine_now(
    state: State<'_, ShellState>,
    name: String,
) -> Result<automations::RunResult, String> {
    let config_dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        routines_page::run_now(&config_dir, &name, &|| scheduled_route(&config_dir))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The Automations page's Suggested tiles.
#[tauri::command]
fn list_suggested_automations(state: State<'_, ShellState>) -> Vec<SuggestedTile> {
    suggested::suggested(&state.config_dir)
}

/// Slot-filled starter Blueprints for the Automations gallery.
#[tauri::command]
fn list_automation_blueprints() -> Vec<grokhub_core::Blueprint> {
    grokhub_core::blueprint_catalog()
}

/// Add a Suggested tile: its seed becomes an automation.
#[tauri::command]
fn add_suggested_automation(
    state: State<'_, ShellState>,
    seed: String,
    clock: automations::Clock,
) -> Result<Vec<automations::Row>, String> {
    let clock = now_clock(clock);
    let list = suggested::add_suggested(&state.config_dir, &seed, clock)?;
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
    let settings = current(&state);
    let result = run_automation_now(state, id.clone(), settings).await;
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

/// [`run_automation`] without the notification, with the settings of who
/// started it: the window's own, or [`web_settings`] for a paired browser.
async fn run_automation_now(
    state: State<'_, ShellState>,
    id: String,
    settings: ShellSettings,
) -> Result<automations::RunResult, String> {
    let config_dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let route = route(&settings, agent::SessionMode::Chat, &config_dir)?;
        let gate = run_gate(&route);
        automations::run_now(
            &config_dir,
            &id,
            route.client,
            &grokhub_agent::route::host::chat_start().0,
            route.workspace,
            gate,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn workspace(state: &ShellState) -> PathBuf {
    agent::workspace_for(&current(state).workspace)
}

/// The folder a line typed in `conversation_id` works in: a Code tab
/// session's own, else the app's.
/// `/sync` and `/inhabit` ask this computer's hub, which a browser can't reach.
#[cfg(desktop)]
fn hub_slash(
    config_dir: &Path,
    remote: bool,
    app: Option<&AppHandle>,
    send: slash::HubSend,
) -> Result<slash::Effect, String> {
    let share = app
        .filter(|_| !remote)
        .and_then(|app| app.try_state::<Arc<hub_share::HubShare>>())
        .ok_or_else(|| "That works on your computer.".to_string())?;
    hub_send::check(config_dir, &share, send)
}

#[cfg(not(desktop))]
fn hub_slash(
    _config_dir: &Path,
    _remote: bool,
    _app: Option<&AppHandle>,
    _send: slash::HubSend,
) -> Result<slash::Effect, String> {
    Err("That works on your computer.".into())
}

/// How a screen recording's report reaches the chat that asked: Grok
/// through the usual model client, then the report saved into the chat as
/// an assistant message and shown at once if the chat is open.
fn report_into_chat(
    state: &ShellState,
    app: &AppHandle,
    conversation_id: Option<&str>,
) -> screen_record::Reporter {
    let (app, asks) = (app.clone(), app.clone());
    let cwd = workspace_of(state, conversation_id);
    let ask_cwd = cwd.clone();
    screen_record::Reporter {
        client: Box::new(|| {
            model_client()
                .map(|(client, _)| Box::new(client) as Box<dyn grokhub_agent::ModelClient>)
        }),
        deliver: Box::new(move |chat, text| deliver_report(&app, &cwd, chat, text)),
        confirm: Box::new(move |chat, prompt, yes| {
            // Kept in the chat too, so a closed or reloaded chat still has
            // the draft and the line that saves it. The open chat shows the card.
            let kept = screen_skill::kept_ask(prompt, yes);
            if agent::save_report(chat, &ask_cwd.display().to_string(), &kept).is_err() {
                return;
            }
            let _ = asks.emit(
                CHAT_EVENT,
                serde_json::json!({
                    "conversationId": chat,
                    "event": agent::UiEvent::Confirm {
                        command: "/record --skill".into(),
                        prompt: prompt.into(),
                        yes: yes.into(),
                    },
                    "helper": true,
                }),
            );
        }),
    }
}

/// Saves `text` into `chat` as Grok's message and tells the page. Grok
/// reads it with the chat's next message (`agent::save_report`).
fn deliver_report(app: &AppHandle, cwd: &Path, chat: &str, text: &str) {
    if agent::save_report(chat, &cwd.display().to_string(), text).is_err() {
        return;
    }
    let _ = app.emit(
        CHAT_EVENT,
        serde_json::json!({
            "conversationId": chat,
            "event": agent::UiEvent::Report { text: text.to_string() },
            "helper": true,
        }),
    );
}

fn workspace_of(state: &ShellState, conversation_id: Option<&str>) -> PathBuf {
    conversation_id
        .and_then(|id| state.code_sessions.of(id))
        .unwrap_or_else(|| workspace(state))
}

/// What a Home or Objectives read takes from the window or a browser.
struct HomeArgs {
    config_dir: PathBuf,
    workspace: PathBuf,
    settings: ShellSettings,
    clock: grokhub_core::LocalClock,
    conversation_id: Option<String>,
    /// A paired browser, which sees only the chats it started.
    web: Option<Arc<remote::WebAccess>>,
}

fn home_args(
    state: &ShellState,
    clock: automations::Clock,
    conversation_id: Option<String>,
    web: Option<Arc<remote::WebAccess>>,
) -> HomeArgs {
    HomeArgs {
        config_dir: state.config_dir.clone(),
        workspace: workspace(state),
        settings: current(state),
        clock: now_clock(clock),
        conversation_id,
        web,
    }
}

/// Runs `read` off the main thread with the sources `args` names.
async fn home_read<T: Send + 'static>(
    args: HomeArgs,
    read: impl FnOnce(&home::Sources<'_>) -> T + Send + 'static,
) -> Result<T, String> {
    off_main(move || {
        let owns = |id: &str| args.web.as_ref().is_some_and(|web| web.owns(id));
        read(&home::Sources {
            config_dir: &args.config_dir,
            workspace: &args.workspace,
            settings: &args.settings,
            caps: capabilities::current(),
            clock: args.clock,
            viewer: match args.web {
                Some(_) => home::Viewer::Web(&owns),
                None => home::Viewer::Desktop,
            },
            conversation_id: args.conversation_id.clone(),
        })
    })
    .await
}

/// Edit Home: today's slots, every module and the presets.
#[tauri::command]
async fn home_catalog(state: State<'_, ShellState>) -> Result<home::Catalog, String> {
    let dir = state.config_dir.clone();
    off_main(move || home::catalog(&dir)).await
}

/// Edit Home → Save. The same `home_layout.json` the cabin reads.
#[tauri::command]
async fn set_home_layout(
    state: State<'_, ShellState>,
    pick: grokhub_core::home::HomePick,
) -> Result<(), String> {
    let dir = state.config_dir.clone();
    off_main(move || home::save_pick(&dir, &pick)).await?
}

/// Home: the cabin's tiles from `home_layout.json`, filled where the shell
/// can.
#[tauri::command]
async fn get_home(
    state: State<'_, ShellState>,
    clock: automations::Clock,
    conversation_id: Option<String>,
) -> Result<home::Page, String> {
    home_read(home_args(&state, clock, conversation_id, None), home::page).await
}

/// Chat's side column: the PC Loadout tile, without reading the rest of Home.
#[tauri::command]
async fn get_pc_snapshot(
    state: State<'_, ShellState>,
) -> Result<grokhub_core::home::TileView, String> {
    let dir = state.config_dir.clone();
    off_main(move || home_tiles::pc_tile(&dir)).await
}

/// Objectives: the open chat's checklist, level, XP and weekly streak.
#[tauri::command]
async fn get_mission(
    state: State<'_, ShellState>,
    clock: automations::Clock,
    conversation_id: Option<String>,
) -> Result<home::MissionPage, String> {
    let args = home_args(&state, clock, conversation_id, None);
    home_read(args, |src| home::mission_for(src, src.clock.now_ms)).await
}

/// Objectives → Accept as done, or Undo. Not a web command.
#[tauri::command]
async fn set_mission_done(
    state: State<'_, ShellState>,
    clock: automations::Clock,
    conversation_id: Option<String>,
    done: bool,
) -> Result<home::MissionPage, String> {
    let args = home_args(&state, clock, conversation_id, None);
    home_read(args, move |src| {
        home::toggle_for(src, done, src.clock.now_ms)
    })
    .await?
}

/// Objectives → My goals, Today's picks and This season.
#[tauri::command]
async fn get_goals(
    state: State<'_, ShellState>,
    clock: automations::Clock,
) -> Result<grokhub_core::objectives::Board, String> {
    let dir = state.config_dir.clone();
    let now = grokhub_core::now_ms();
    off_main(move || goals::board(&dir, now, clock.at(now).utc_offset_min())).await
}

/// Objectives → add a goal, +1, Done, Drop, Track or Not now. A paired
/// phone may do these too: goals are the player's own list.
#[tauri::command]
async fn goal_act(
    state: State<'_, ShellState>,
    clock: automations::Clock,
    acts: Vec<grokhub_core::objectives::GoalAct>,
) -> Result<goals::Saved, String> {
    let dir = state.config_dir.clone();
    let now = grokhub_core::now_ms();
    off_main(move || {
        let library = if acts
            .iter()
            .any(|a| matches!(a, grokhub_core::objectives::GoalAct::Add { .. }))
        {
            goals::library()
        } else {
            Vec::new()
        };
        goals::act(&dir, &acts, &library, now, clock.at(now).utc_offset_min())
    })
    .await?
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

#[tauri::command]
fn pause_memory_row(
    state: State<'_, ShellState>,
    id: String,
    paused: bool,
) -> Result<memory::Notes, String> {
    memory::pause_row(&workspace(&state), &id, paused)
}

#[tauri::command]
fn add_personal_fact(
    state: State<'_, ShellState>,
    text: String,
    group: String,
) -> Result<memory::Notes, String> {
    memory::add_fact(&workspace(&state), &text, &group)
}

/// Saves the readable copy in Downloads on this computer. Desktop only.
#[tauri::command]
fn export_personal() -> Result<String, String> {
    let dir = grokhub_core::diagnostics::downloads_dir()
        .ok_or_else(|| "Couldn't find your Downloads folder.".to_string())?;
    memory::export_to(&dir)
}

/// Updates the Obsidian-friendly wiki of the plain memory. Desktop only.
#[tauri::command]
fn export_memory_wiki() -> Result<String, String> {
    memory::export_wiki()
}

/// Deletes every learned line for good. Desktop only; the page asks first.
#[tauri::command]
fn wipe_personal(state: State<'_, ShellState>) -> Result<memory::Wiped, String> {
    memory::wipe(&workspace(&state))
}

#[tauri::command]
fn get_memory_files() -> Result<memory::MemoryFiles, String> {
    memory::load_files()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryFileSaved {
    files: memory::MemoryFiles,
    redacted: bool,
}

#[tauri::command]
fn save_memory_file(name: String, text: String, base: String) -> Result<MemoryFileSaved, String> {
    let (files, redacted) = memory::save_file(&name, &text, &base)?;
    Ok(MemoryFileSaved { files, redacted })
}

#[tauri::command]
fn restore_memory_file(name: String) -> Result<memory::MemoryFiles, String> {
    memory::restore_file(&name)
}

#[tauri::command]
fn reflect_memory(
    state: State<'_, ShellState>,
    conversation_id: Option<String>,
) -> Result<memory::ReflectResult, String> {
    memory::reflect(&state.config_dir, conversation_id.as_deref())
}

#[tauri::command]
fn tidy_memory_ledger() -> Result<memory::MemoryFiles, String> {
    memory::tidy_ledger()
}

#[tauri::command]
fn apply_memory_raw_tidy() -> Result<memory::MemoryFiles, String> {
    memory::apply_raw_tidy()
}

#[tauri::command]
fn dismiss_memory_raw_tidy() -> Result<memory::MemoryFiles, String> {
    memory::dismiss_raw_tidy()
}

#[tauri::command]
fn get_memory_retention() -> memory::RetentionSetting {
    memory::retention_setting()
}

#[tauri::command]
fn set_memory_retention(days: u32) -> Result<memory::RetentionSetting, String> {
    memory::set_retention(days)
}

#[tauri::command]
fn get_chat_archive() -> memory::ArchiveSetting {
    memory::archive_setting()
}

#[tauri::command]
fn set_chat_archive(days: u32) -> Result<memory::ArchiveSetting, String> {
    memory::set_archive(days)
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

/// Host payload for an MCP App iframe's `ui/initialize`.
#[tauri::command]
fn mcp_app_initialize() -> serde_json::Value {
    grokhub_agent::app_host_initialize()
}

/// Proxy an app-visible `tools/call` from an MCP App iframe. Rejects tools
/// that are not app-visible on that server.
#[tauri::command]
async fn mcp_app_call_tool(
    server: String,
    name: String,
    #[allow(non_snake_case)] arguments: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let args = arguments.unwrap_or_else(|| serde_json::json!({}));
    off_main(move || grokhub_agent::app_call_tool(&server, &name, &args)).await?
}

/// What this platform runs; the UI hides the rest.
#[tauri::command]
fn get_capabilities() -> capabilities::Capabilities {
    capabilities::current()
}

/// Code tab: the project list and which project is open.
#[tauri::command]
async fn code_projects(state: State<'_, ShellState>) -> Result<projects::CodeProjects, String> {
    let (dir, bound) = (state.config_dir.clone(), current(&state).workspace);
    off_main(move || projects::listing(&dir, &bound)).await?
}

const CODE_SWITCH_BUSY: &str =
    "A test run or deploy is still going. Stop it or wait for it to end, then switch projects.";

/// Opens `folder` as the project the app works in, as `/project bind` does,
/// and answers with the list again.
fn open_code_project(state: &ShellState, folder: &str) -> Result<projects::CodeProjects, String> {
    if folder.trim().is_empty() {
        return Err("Pick a folder to open.".into());
    }
    let bound = current(state).workspace;
    if code_runs::running() && !projects::is_open(folder, &bound) {
        return Err(CODE_SWITCH_BUSY.into());
    }
    let old_dir = workspace(state);
    let done = projects::run(
        &state.config_dir,
        &bound,
        projects::Op::Bind(folder.to_string()),
    )?;
    let path = done.workspace.unwrap_or_else(|| bound.clone());
    let next = ShellSettings {
        workspace: path.clone(),
        ..current(state)
    };
    settings::save(&state.config_dir, &next)?;
    let keep = next.code_ship.activity_keep;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next;
    // The terminal's shell was started in the old folder: it ends with the
    // switch, as it does in the cabin.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if !projects::is_open(&path, &bound) && code_term::close(code_term::global()) {
        code_term::log(&state.config_dir, &old_dir, "Closed the terminal.", keep);
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let _ = (old_dir, keep);
    // Local open: pick up origin URL and preferred branch; never check out.
    // Do not overwrite a user branch override (`force: false`).
    let linked = grokhub_code::link::inspect_local(Path::new(&path), None);
    let _ = projects::apply_link(
        &state.config_dir,
        &path,
        linked.remote_url.as_deref(),
        Some(linked.preferred_branch.as_str()),
        false,
    );
    projects::listing(&state.config_dir, &path)
}

/// Code tab: switch to a project from the list, or open a folder by path.
#[tauri::command]
async fn code_project_open(app: AppHandle, path: String) -> Result<projects::CodeProjects, String> {
    off_main(move || open_code_project(&app.state::<ShellState>(), &path)).await?
}

/// Code tab: Open folder. The system picker, then that folder opens; `None`
/// when the picker was closed.
#[tauri::command]
async fn code_project_pick(app: AppHandle) -> Result<Option<projects::CodeProjects>, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        off_main(move || {
            let picked = app
                .dialog()
                .file()
                .set_title("Open a project folder")
                .blocking_pick_folder()
                .and_then(|p| p.into_path().ok());
            match picked {
                Some(dir) => {
                    open_code_project(&app.state::<ShellState>(), &dir.to_string_lossy()).map(Some)
                }
                None => Ok(None),
            }
        })
        .await?
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err(attach::NO_PICKER.into())
    }
}

/// Code tab: New project. Makes it under the work root and opens it.
#[tauri::command]
async fn code_project_new(app: AppHandle, name: String) -> Result<projects::CodeProjects, String> {
    off_main(move || {
        let state = app.state::<ShellState>();
        let dir = projects::create(&state.config_dir, &name)?;
        open_code_project(&state, &dir)
    })
    .await?
}

/// Code tab: link a remote git URL (clone under the work root) and open it.
/// Uses the saved GitHub PAT for HTTPS github.com clones only; never returns
/// the token. Existing local folders are reused, never wiped.
#[tauri::command]
async fn code_project_link_remote(
    app: AppHandle,
    url: String,
    preferred_branch: Option<String>,
) -> Result<projects::CodeProjects, String> {
    off_main(move || {
        let state = app.state::<ShellState>();
        let token = grokhub_agent::route::providers::named_key(
            &state.config_dir,
            grokhub_agent::route::providers::NamedKey::GithubToken,
        )
        .ok()
        .flatten();
        let work_root = PathBuf::from(grokhub_core::cabin_work_root(
            cfg!(windows),
            std::env::var("HOME").ok().as_deref(),
            std::env::var("USERPROFILE").ok().as_deref(),
        ));
        let linked = grokhub_code::link::link_remote(
            &url,
            &work_root,
            token.as_deref().map(String::as_str),
            preferred_branch.as_deref(),
        )?;
        drop(token);
        let _ = open_code_project(&state, &linked.path)?;
        projects::apply_link(
            &state.config_dir,
            &linked.path,
            linked.remote_url.as_deref(),
            Some(linked.preferred_branch.as_str()),
            true,
        )?;
        let _ = linked.note;
        projects::listing(&state.config_dir, &linked.path)
    })
    .await?
}

/// Code tab: set or clear the open project's preferred working branch.
#[tauri::command]
async fn code_project_set_branch(
    app: AppHandle,
    project_id: String,
    branch: Option<String>,
) -> Result<projects::CodeProjects, String> {
    off_main(move || {
        let state = app.state::<ShellState>();
        projects::set_preferred_branch(&state.config_dir, &project_id, branch.as_deref())?;
        let bound = current(&state).workspace;
        projects::listing(&state.config_dir, &bound)
    })
    .await?
}

/// Code tab: discovered repo rules for the open project (918.5 handoff shape).
#[tauri::command]
async fn code_project_rules(
    state: State<'_, ShellState>,
) -> Result<grokhub_agent::RepoRulesBundle, String> {
    let (dir, bound) = (state.config_dir.clone(), current(&state).workspace);
    off_main(move || {
        if bound.trim().is_empty() {
            return Err("Open a project first.".into());
        }
        let path = PathBuf::from(&bound);
        let nodes = projects::load(&dir).unwrap_or_default();
        let node = nodes.iter().find(|n| {
            n.kind == grokhub_core::ProjectKind::Project && projects::is_open(&n.path, &bound)
        });
        let project_id = node.map(|n| n.id.as_str()).unwrap_or("");
        let override_branch = node.and_then(|n| n.preferred_branch.as_deref());
        let remote = node
            .and_then(|n| n.remote_url.clone())
            .or_else(|| grokhub_code::link::remote_url(&path));
        let has_beta = grokhub_code::link::has_beta(&path);
        let default_branch = grokhub_code::ship::default_branch(&path);
        let repo = grokhub_code::repo::repo(&path);
        Ok(grokhub_agent::discover_repo_rules(
            &path,
            grokhub_agent::RepoRulesInput {
                project_id,
                remote_url: remote.as_deref(),
                preferred_branch: override_branch,
                active_branch: repo.branch.as_deref(),
                default_branch: default_branch.as_deref(),
                has_beta,
                sync: repo.is_repo.then_some(grokhub_agent::RepoSync {
                    ahead: repo.ahead,
                    behind: repo.behind,
                    upstream: repo.upstream.clone(),
                }),
            },
        ))
    })
    .await?
}

/// Code tab: the agent window shows this session. It takes the open
/// project's folder unless it has one already; the answer is its folder.
/// A session with no saved turns yet started here, so it has a lead.
#[tauri::command]
fn code_session_bind(state: State<'_, ShellState>, conversation_id: String) -> String {
    lead::shown(&state.config_dir, &conversation_id);
    state
        .code_sessions
        .bind(&conversation_id, &workspace(&state))
        .to_string_lossy()
        .into_owned()
}

/// Code tab: whether the session started in the agent window, so it has a
/// lead and its plan can be edited there.
#[tauri::command]
fn code_session_lead(state: State<'_, ShellState>, conversation_id: String) -> bool {
    lead::is_lead(&state.config_dir, &conversation_id)
}

/// Code tab: the session's saved plan, as last shown or as the person
/// edited it. `None` for a session with no plan.
#[tauri::command]
fn code_session_plan(state: State<'_, ShellState>, conversation_id: String) -> Option<String> {
    lead::read_plan(&state.config_dir, &conversation_id)
}

/// Code tab: where this session left off, or nothing when it has no checkpoint.
#[tauri::command]
fn code_session_left(
    state: State<'_, ShellState>,
    conversation_id: String,
) -> Option<code_keep::LeftOff> {
    code_keep::read(&state.config_dir, &conversation_id)
}

/// Code tab: the message Resume sends, from the saved checkpoint.
#[tauri::command]
fn code_session_resume(
    state: State<'_, ShellState>,
    conversation_id: String,
) -> Result<String, String> {
    let left = code_keep::read(&state.config_dir, &conversation_id)
        .ok_or("This session has no checkpoint to resume.")?;
    Ok(code_keep::resume_prompt(&left))
}

/// Code tab: the workspace's repo, read only.
#[tauri::command]
async fn code_repo(state: State<'_, ShellState>) -> Result<code::Repo, String> {
    let dir = workspace(&state);
    off_main(move || code::repo(&dir)).await
}

/// Code tab: one changed file's diff, read only.
#[tauri::command]
async fn code_diff(state: State<'_, ShellState>, path: String) -> Result<code::Diff, String> {
    let dir = workspace(&state);
    off_main(move || code::diff(&dir, &path)).await?
}

/// Code tab: the app's built-in skills, read only.
#[tauri::command]
async fn code_builtins(state: State<'_, ShellState>) -> Result<Vec<code::Builtin>, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    let home = grokhub_core::user_home();
    off_main(move || code::builtins(&dir, home.as_deref(), &config)).await
}

/// Code tab: the test command to show, and whether it still needs a confirm.
/// With `conversation_id`, for the folder that session works in.
#[tauri::command]
async fn code_test_plan(
    state: State<'_, ShellState>,
    command: Option<String>,
    conversation_id: Option<String>,
) -> Result<code_runs::Plan, String> {
    let dir = workspace_of(&state, conversation_id.as_deref());
    let config = state.config_dir.clone();
    off_main(move || code_runs::plan(&config, &dir, command.as_deref(), code_runs::Mode::Auto))
        .await?
}

/// Code tab: run the test command. `confirmed` must be true when the store
/// still wants a confirm for this command. With `conversation_id`, in the
/// folder that session works in. A run in a git repository that leaves the
/// files as it found them is kept as the project's last test run.
#[tauri::command]
async fn code_test_run(
    app: AppHandle,
    state: State<'_, ShellState>,
    command: String,
    confirmed: bool,
    conversation_id: Option<String>,
) -> Result<code_runs::RunResult, String> {
    let dir = workspace_of(&state, conversation_id.as_deref());
    let config = state.config_dir.clone();
    off_main(move || {
        let result = code_runs::run(
            &config,
            &dir,
            &command,
            confirmed,
            code_runs::Mode::Auto,
            |chunk| {
                let _ = app.emit(
                    code_runs::RUN_EVENT,
                    code_runs::RunChunk {
                        text: chunk.to_string(),
                        done: false,
                        exit: None,
                        cut: false,
                    },
                );
            },
        );
        if let Ok(done) = &result {
            let _ = app.emit(
                code_runs::RUN_EVENT,
                code_runs::RunChunk {
                    text: String::new(),
                    done: true,
                    exit: Some(done.exit),
                    cut: done.cut,
                },
            );
        }
        result
    })
    .await?
}

/// Code tab: stop the test run or deploy in progress, and everything it
/// started. False when nothing was running. Desktop only.
#[tauri::command]
async fn code_run_stop() -> Result<bool, String> {
    off_main(code_runs::stop).await
}

/// Settings → Code: the remembered test command for this project, if any.
#[tauri::command]
async fn code_test_remembered(
    state: State<'_, ShellState>,
) -> Result<code_runs::RememberedView, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    off_main(move || code_runs::remembered_view(&config, &dir)).await?
}

/// Settings → Code: forget the remembered test command so the next run asks.
#[tauri::command]
async fn code_test_forget(
    state: State<'_, ShellState>,
) -> Result<code_runs::RememberedView, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    off_main(move || code_runs::forget_test(&config, &dir)).await?
}

/// Command page: the project folder's name and the lines it remembers.
#[tauri::command]
async fn command_box_view(state: State<'_, ShellState>) -> Result<command_box::View, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    off_main(move || command_box::view(&config, &dir)).await
}

/// Command page: whether a line goes to the agent now, asks first, or is
/// refused (forbidden, outside the project, or stopped by the push guard).
#[tauri::command]
async fn command_box_check(
    state: State<'_, ShellState>,
    line: String,
    conversation_id: Option<String>,
) -> Result<command_box::Verdict, String> {
    let dir = workspace_of(&state, conversation_id.as_deref());
    let config = state.config_dir.clone();
    let ship = current(&state).code_ship;
    off_main(move || command_box::check(&config, &dir, &line, ship)).await
}

/// Command page, and Yes on a `/sh` card: the checks run again, the line is
/// remembered for this repo, and the agent's message comes back to queue.
/// A chat's line is checked in that chat's folder (a Code tab session's own).
#[tauri::command]
async fn command_box_approve(
    state: State<'_, ShellState>,
    line: String,
    conversation_id: Option<String>,
) -> Result<String, String> {
    let dir = workspace_of(&state, conversation_id.as_deref());
    let config = state.config_dir.clone();
    let ship = current(&state).code_ship;
    off_main(move || command_box::approve(&config, &dir, &line, ship)).await?
}

/// Command page: forget this repo's remembered lines, so each asks again.
#[tauri::command]
async fn command_box_forget(state: State<'_, ShellState>) -> Result<command_box::View, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    off_main(move || {
        command_box::forget(&config, &command_box::repo_key(&dir))?;
        Ok(command_box::view(&config, &dir))
    })
    .await?
}

/// Code tab: what a push or "Open PR into" would run, and whether it asks first.
/// With `conversation_id`, the agent window's Tasks panel: the push of the
/// commit made there, in the session's folder, after every check of the
/// push, and always asked.
#[tauri::command]
async fn code_ship_plan(
    state: State<'_, ShellState>,
    action: code_ship::Action,
    conversation_id: Option<String>,
) -> Result<code_ship::Plan, String> {
    let dir = workspace_of(&state, conversation_id.as_deref());
    let config = state.config_dir.clone();
    let cfg = current(&state).code_ship;
    off_main(move || match conversation_id {
        Some(id) => ship_gate::push_plan(&config, &dir, &id, &cfg, action),
        None => code_ship::plan(&config, &dir, &cfg, action),
    })
    .await
}

/// Code tab: push the current branch or open its PR, through the push guard.
/// Unless the guard said no confirm is needed, `confirmed` must be true and
/// `command` must be the command the confirm showed. With
/// `conversation_id`, the Tasks panel's push of the commit made there, or
/// its PR: every check runs again, and the confirm is always needed.
#[tauri::command]
async fn code_ship_run(
    state: State<'_, ShellState>,
    action: code_ship::Action,
    confirmed: bool,
    command: Option<String>,
    conversation_id: Option<String>,
) -> Result<code_ship::Shipped, String> {
    let dir = workspace_of(&state, conversation_id.as_deref());
    let config = state.config_dir.clone();
    let cfg = current(&state).code_ship;
    off_main(move || match conversation_id {
        Some(id) => ship_gate::push(
            &config,
            &dir,
            &id,
            &cfg,
            action,
            confirmed,
            command.as_deref(),
        ),
        None => code_ship::ship(&config, &dir, &cfg, action, confirmed, command.as_deref()),
    })
    .await?
}

/// Code tab: make and check out a branch the guard counts as the app's own.
/// With `conversation_id`, in that session's folder.
#[tauri::command]
async fn code_new_branch(
    state: State<'_, ShellState>,
    name: String,
    conversation_id: Option<String>,
) -> Result<String, String> {
    let dir = workspace_of(&state, conversation_id.as_deref());
    let config = state.config_dir.clone();
    let cfg = current(&state).code_ship;
    off_main(move || code_ship::new_branch(&config, &dir, &cfg, &name)).await?
}

/// Code tab: this project's newest pushes, PRs, branches and blocked attempts.
/// With `conversation_id`, of the project that session works in.
#[tauri::command]
async fn code_activity_list(
    state: State<'_, ShellState>,
    conversation_id: Option<String>,
) -> Result<Vec<code_activity::Entry>, String> {
    let dir = workspace_of(&state, conversation_id.as_deref());
    let config = state.config_dir.clone();
    off_main(move || code_activity::list_for(&config, &dir, code_activity::SHOW)).await
}

/// Settings → Code → Secrets: this project's secret names (never the values).
#[tauri::command]
async fn code_secret_list(
    state: State<'_, ShellState>,
) -> Result<code_secrets::SecretList, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    off_main(move || code_secrets::list_for(&config, &dir)).await
}

/// Settings → Code → Secrets: add a secret or replace its value. The value goes
/// to the OS keychain; only the name is written to disk.
#[tauri::command]
async fn code_secret_set(
    state: State<'_, ShellState>,
    name: String,
    value: String,
) -> Result<code_secrets::SecretList, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    let keep = current(&state).code_ship.activity_keep;
    off_main(move || code_secrets::set_for(&config, &dir, &name, &value, keep)).await?
}

/// Settings → Code → Secrets: delete one secret. Refused unless `confirmed`.
#[tauri::command]
async fn code_secret_delete(
    state: State<'_, ShellState>,
    name: String,
    confirmed: bool,
) -> Result<code_secrets::SecretList, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    let keep = current(&state).code_ship.activity_keep;
    off_main(move || code_secrets::delete_for(&config, &dir, &name, confirmed, keep)).await?
}

/// Code tab and Settings → Code: this project's deploy command and whether it asks first.
#[tauri::command]
async fn code_deploy_plan(state: State<'_, ShellState>) -> Result<code_deploy::Plan, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    let ship = current(&state).code_ship;
    off_main(move || code_deploy::plan(&config, &dir, &ship)).await?
}

/// Settings → Code: this project's deploy switch and command.
#[tauri::command]
async fn code_deploy_save(
    state: State<'_, ShellState>,
    deploy_enabled: bool,
    command: String,
) -> Result<code_deploy::Settings, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    off_main(move || {
        code_deploy::save_settings_of(
            &config,
            &dir,
            code_deploy::Settings {
                deploy_enabled,
                command,
            },
        )
    })
    .await?
}

/// Settings → Code: forget the remembered deploy command so the next deploy asks.
#[tauri::command]
async fn code_deploy_forget(state: State<'_, ShellState>) -> Result<code_deploy::Plan, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    let ship = current(&state).code_ship;
    off_main(move || code_deploy::forget(&config, &dir, &ship)).await?
}

/// Code tab: run the deploy. `command` is the text the person was shown and
/// `confirmed` must be true unless the mode and memory say no confirm is needed.
#[tauri::command]
async fn code_deploy_run(
    app: AppHandle,
    state: State<'_, ShellState>,
    command: String,
    confirmed: bool,
) -> Result<code_runs::RunResult, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    let ship = current(&state).code_ship;
    off_main(move || {
        let result = code_deploy::run(&config, &dir, &ship, &command, confirmed, |chunk| {
            let _ = app.emit(
                code_deploy::RUN_EVENT,
                code_runs::RunChunk {
                    text: chunk.to_string(),
                    done: false,
                    exit: None,
                    cut: false,
                },
            );
        });
        if let Ok(done) = &result {
            let _ = app.emit(
                code_deploy::RUN_EVENT,
                code_runs::RunChunk {
                    text: String::new(),
                    done: true,
                    exit: Some(done.exit),
                    cut: done.cut,
                },
            );
        }
        result
    })
    .await?
}
/// Code tab editor: one folder of the project.
#[tauri::command]
async fn code_files_list(
    state: State<'_, ShellState>,
    path: String,
) -> Result<code_files::Listing, String> {
    let dir = workspace(&state);
    off_main(move || code_files::list(&dir, &path)).await?
}

/// Code tab editor: open a text file of the project.
#[tauri::command]
async fn code_file_read(
    state: State<'_, ShellState>,
    path: String,
) -> Result<code_files::FileText, String> {
    let dir = workspace(&state);
    off_main(move || code_files::read(&dir, &path)).await?
}

/// Code tab editor: save a text file of the project. The save is logged.
/// `root` is the project folder `code_file_read` returned with the file; the
/// save is refused when the project folder is a different one now.
#[tauri::command]
async fn code_file_write(
    state: State<'_, ShellState>,
    path: String,
    text: String,
    root: String,
) -> Result<code_files::FileText, String> {
    let dir = workspace(&state);
    let config = state.config_dir.clone();
    let keep = current(&state).code_ship.activity_keep;
    off_main(move || code_files::write_logged(&config, &dir, &path, &text, &root, keep)).await?
}

/// Code tab terminal: open the user's shell in the project folder.
#[tauri::command]
async fn code_term_open(
    app: AppHandle,
    state: State<'_, ShellState>,
    rows: u16,
    cols: u16,
) -> Result<u32, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = (&app, &state, rows, cols);
        Err(code_term::NONE.to_string())
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let dir = workspace(&state);
        let config = state.config_dir.clone();
        let keep = current(&state).code_ship.activity_keep;
        off_main(move || {
            let held = code_secrets::held_for(&config, &dir);
            let id = code_term::open(code_term::global(), &dir, held, rows, cols, move |chunk| {
                let _ = app.emit(code_term::EVENT, chunk);
            })?;
            // The shell is already printing and the window needs the id to
            // show it, so the id goes back first and the log is written after.
            std::thread::spawn(move || {
                code_term::log(
                    &config,
                    &dir,
                    "Opened a terminal in the project folder.",
                    keep,
                );
            });
            Ok(id)
        })
        .await?
    }
}

/// Code tab terminal: send typed text to the shell.
#[tauri::command]
fn code_term_write(id: u32, data: String) -> Result<(), String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = (id, data);
        Err(code_term::NONE.to_string())
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    code_term::write(code_term::global(), id, &data)
}

/// Code tab terminal: the window changed size.
#[tauri::command]
fn code_term_resize(id: u32, rows: u16, cols: u16) -> Result<(), String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = (id, rows, cols);
        Ok(())
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    code_term::resize(code_term::global(), id, rows, cols)
}

/// Code tab terminal: end the shell (the tab closing or the Close button).
#[tauri::command]
async fn code_term_close(state: State<'_, ShellState>) -> Result<(), String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = &state;
        Ok(())
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let dir = workspace(&state);
        let config = state.config_dir.clone();
        let keep = current(&state).code_ship.activity_keep;
        off_main(move || {
            if code_term::close(code_term::global()) {
                code_term::log(&config, &dir, "Closed the terminal.", keep);
            }
        })
        .await
    }
}

/// Settings → Code → Pushes and PRs.
#[tauri::command]
fn get_code_ship(state: State<'_, ShellState>) -> code_ship::CodeShip {
    current(&state).code_ship
}

#[tauri::command]
fn save_code_ship(
    state: State<'_, ShellState>,
    ship: code_ship::CodeShip,
) -> Result<code_ship::CodeShip, String> {
    let ship = code_ship::checked(ship)?;
    let next = ShellSettings {
        code_ship: ship.clone(),
        ..current(&state)
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next;
    Ok(ship)
}

/// Settings → Code → vstack skills: active version/commit, last sync time,
/// and manual Sync now trigger.
#[tauri::command]
fn vstack_sync_status(state: State<'_, ShellState>) -> grokhub_core::vstack_sync::VstackStatus {
    vstack_sync::status(&state.config_dir)
}

#[tauri::command]
async fn vstack_sync_now(
    state: State<'_, ShellState>,
) -> Result<grokhub_core::vstack_sync::VstackStatus, String> {
    let dir = state.config_dir.clone();
    off_main(move || vstack_sync::sync_now(&dir)).await?
}

#[tauri::command]
async fn get_journal(state: State<'_, ShellState>) -> Result<journal::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || journal::load(&dir)).await?
}

#[tauri::command]
async fn get_meetings(
    state: State<'_, ShellState>,
    query: Option<String>,
) -> Result<meetings::Page, String> {
    let dir = state.config_dir.clone();
    let q = query.unwrap_or_default();
    off_main(move || meetings::load(&dir, &q)).await?
}

#[tauri::command]
async fn meetings_act(
    state: State<'_, ShellState>,
    act: meetings::Act,
    query: Option<String>,
) -> Result<meetings::Page, String> {
    let dir = state.config_dir.clone();
    let q = query.unwrap_or_default();
    off_main(move || meetings::act(&dir, act, &q)).await?
}

#[tauri::command]
async fn get_wake_word(state: State<'_, ShellState>) -> Result<wake_word::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || wake_word::load(&dir)).await?
}

#[tauri::command]
async fn wake_word_act(
    state: State<'_, ShellState>,
    act: wake_word::Act,
) -> Result<wake_word::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || wake_word::act(&dir, act)).await?
}

/// Feed one on-device transcript snippet into the wake matcher. Must not be
/// cloud STT — always-listen stays local.
#[tauri::command]
async fn wake_word_feed_local(
    state: State<'_, ShellState>,
    text: String,
) -> Result<wake_word::FeedResult, String> {
    let dir = state.config_dir.clone();
    off_main(move || wake_word::feed_local(&dir, &text)).await?
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

/// Resume a paused or interrupted run from its checkpoint (KB-03). An
/// automation's or a background run's card resumes as an unattended run on
/// that card; any other card sends its resume block once in its chat. A
/// resumed step that may drive the desktop asks the game guard first.
#[tauri::command]
async fn workboard_resume(
    state: State<'_, ShellState>,
    id: String,
    conversation_id: String,
) -> Result<workboard::Resume, String> {
    let settings = current(&state);
    let dir = state.config_dir.clone();
    // The chats the page sends into: the card's, and the open one.
    let busy: std::collections::HashSet<String> = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(_, s)| s.busy())
        .map(|(id, _)| id.clone())
        .collect();
    off_main(move || {
        let card =
            grokhub_core::work_run::read_board_file(&grokhub_core::work_run::board_path(&dir))?
                .into_iter()
                .find(|c| c.id == id)
                .ok_or_else(|| workboard::GONE.to_string())?;
        if grokhub_core::work_resume::resume_touches_desktop(&card, settings.desktop_control) {
            if let Some(why) = unattended::game_hold() {
                return Err(why);
            }
        }
        if !run_resume::unattended_card(&card) {
            // The page sends the block into the chat; a busy chat would drop it.
            let into = [
                card.thread_id.as_deref().unwrap_or_default(),
                conversation_id.as_str(),
                settings.last_chat.as_str(),
            ];
            if into.iter().any(|c| busy.contains(*c)) {
                return Err("Wait for the chat to finish its answer first.".into());
            }
            let mode =
                unattended::RunGate::scheduled(settings.approvals.mode(), false, None).wire();
            return workboard::resume(&dir, &id, &conversation_id, mode);
        }
        resume_in_background(&dir, &settings, &id, false)?;
        Ok(workboard::Resume {
            conversation_id: String::new(),
            prompt: String::new(),
            background: true,
            page: workboard::load(&dir)?,
        })
    })
    .await?
}

/// Start an unattended resume of card `id`: the card goes to Doing now, and
/// the run goes on its own thread.
fn resume_in_background(
    dir: &Path,
    settings: &ShellSettings,
    id: &str,
    auto: bool,
) -> Result<(), String> {
    let route = route(settings, agent::SessionMode::Chat, dir)?;
    let (model, _) = grokhub_agent::route::host::chat_start();
    let gate = run_gate(&route);
    let job = run_resume::prepare(
        dir,
        id,
        &model,
        gate,
        auto,
        &grokhub_agent::session_intact,
        bg_runs::live_count(),
        grokhub_core::now_ms(),
    )?;
    let (dir, client, workspace) = (dir.to_path_buf(), route.client, route.workspace);
    std::thread::spawn(move || {
        run_resume::run(&dir, job, client, &model, workspace);
    });
    Ok(())
}

/// Every five seconds: the agent's own interrupted automation and background
/// cards resume once per interruption, twice a day at most; past that they
/// wait in Blocked and the bell says so. While runs are full, the agent is
/// signed out or a game holds the desktop, a card waits, its try unspent.
#[cfg(desktop)]
fn start_auto_resume(app: AppHandle) {
    use grokhub_core::work_resume::AutoResume;
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        let state = app.state::<ShellState>();
        let dir = state.config_dir.clone();
        let now = grokhub_core::now_ms();
        for (card, decision) in run_resume::auto_picks(&dir, now) {
            match decision {
                AutoResume::GiveUp => {
                    if let Some(stopped) = run_resume::give_up(&dir, &card.id, now) {
                        os_extras::auto_resume_stopped(&app, &stopped.text);
                        let _ = app.emit(run_resume::AUTO_RESUME_EVENT, &stopped);
                    }
                }
                AutoResume::Go => {
                    let settings = current(&state);
                    let game = unattended::game_hold();
                    let waits = run_resume::waits(
                        &card,
                        bg_runs::live_count(),
                        settings.desktop_control,
                        game.as_deref(),
                    );
                    if waits || chat_client(&dir).is_err() {
                        continue;
                    }
                    if let Err(why) = resume_in_background(&dir, &settings, &card.id, true) {
                        run_resume::auto_failed(&dir, &card.id, &why, now);
                    }
                }
            }
        }
    });
}

/// Workboard → a card's own chat: Say gives the card its chat (the page's
/// new id when it has none here) and the message, which the page then sends
/// with `send_chat`, so it streams like any chat. Desktop only.
#[tauri::command]
async fn workboard_say(
    state: State<'_, ShellState>,
    id: String,
    text: String,
    conversation_id: String,
) -> Result<card_chat::Say, String> {
    let dir = state.config_dir.clone();
    off_main(move || card_chat::say(&dir, &id, &text, &conversation_id)).await?
}

/// The Workboard's queue, while the cabin is closed: once a second, the
/// oldest queued card starts as a turn in its own chat when no chat of yours
/// runs, no other queued card runs, there is room for background runs, no
/// game holds it and the agent is ready.
#[cfg(desktop)]
fn start_card_queue(app: AppHandle) {
    std::thread::spawn(move || {
        let config_dir = app.state::<ShellState>().config_dir.clone();
        card_chat::seed(&config_dir);
        let mut queue = card_chat::Queue::default();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let state = app.state::<ShellState>();
            let foreground = state
                .chats
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .any(|s| s.busy());
            let gate = grokhub_core::work_triage::QueueGate {
                foreground,
                bg_live: bg_runs::live_count() + run_resume::live_count(),
                game: grokhub_core::work_chat::queue_game_wait(&grokhub_desktop::game_guard::peek()),
                ..Default::default()
            };
            let now = grokhub_core::now_ms();
            let mode = match current(&state).approvals {
                settings::Approvals::Ask => grokhub_core::wire::PermissionMode::Ask,
                settings::Approvals::Auto => grokhub_core::wire::PermissionMode::Auto,
                settings::Approvals::Always => grokhub_core::wire::PermissionMode::AlwaysApprove,
            };
            let ready = || chat_client(&config_dir).is_ok();
            let new_chat = || card_chat::new_chat_id(now);
            let look = card_chat::Look {
                now,
                gate,
                ready: &ready,
                new_chat: &new_chat,
                mode,
            };
            let Some(start) = queue.tick(&config_dir, look) else {
                continue;
            };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<ShellState>();
                let turn = Turn {
                    text: start.message,
                    mode: None,
                    attachments: Vec::new(),
                    clock: None,
                };
                let settings = current(&state);
                let _ = chat_turn(app.clone(), &state, start.chat, turn, settings).await;
            });
        }
    });
}

/// Sessions board facts from live chats, inbox waits, and transcript PR links.
/// A tile shows its chat's title and last line, so a paired browser (`web`)
/// gets only the chats a browser started, never the window's.
fn sessions_board_facts(state: &ShellState, web: bool) -> Vec<sessions_board::SessionFact> {
    use sessions_board::{RunStatus, SessionFact};
    let dir = &state.config_dir;
    let mut rows = history::list(dir);
    if web {
        let started = web_chats::all(dir);
        rows.retain(|row| started.contains(&row.id));
    }
    let waiting: std::collections::HashSet<String> = {
        let mut set: std::collections::HashSet<String> = inbox::decisions(dir)
            .into_iter()
            .filter(|d| !d.chat_id.is_empty())
            .map(|d| d.chat_id)
            .collect();
        for row in &rows {
            if helper_asks::needs_you(&row.id) {
                set.insert(row.id.clone());
            }
        }
        set
    };
    let chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
    let mut facts = Vec::with_capacity(rows.len());
    for row in rows {
        let busy = chats.get(&row.id).is_some_and(|s| s.busy()) || helper_asks::in_turn(&row.id);
        let failed = session_recently_failed(dir, &row.id);
        let run = if waiting.contains(&row.id) {
            RunStatus::Waiting
        } else if failed {
            RunStatus::Failed
        } else if busy {
            RunStatus::Running
        } else {
            RunStatus::Idle
        };
        let (preview, prs, involving_me) = session_tile_bits(dir, &row.id);
        facts.push(SessionFact {
            id: row.id,
            title: row.title,
            updated_ms: row.updated,
            preview,
            run,
            prs,
            involving_me,
        });
    }
    facts
}

fn session_recently_failed(config_dir: &Path, id: &str) -> bool {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    match grokhub_agent::work_for(id) {
        Ok(works) => works.iter().any(|w| {
            matches!(w.ended, Some(grokhub_agent::Ended::Failed(_)))
                && !matches!(
                    w.state,
                    grokhub_agent::WorkState::Merged | grokhub_agent::WorkState::Discarded
                )
        }),
        Err(_) => false,
    }
}

fn session_tile_bits(config_dir: &Path, id: &str) -> (String, Vec<sessions_board::PrLink>, bool) {
    match history::load(config_dir, id, 40) {
        Ok(t) => {
            let mut involving_me = false;
            let mut blob = String::new();
            for line in &t.lines {
                if matches!(line.role, history::Role::User) {
                    involving_me = true;
                }
                if !blob.is_empty() {
                    blob.push(' ');
                }
                blob.push_str(&line.text);
            }
            let preview = t
                .lines
                .iter()
                .rev()
                .find(|l| !l.text.trim().is_empty())
                .map(|l| l.text.clone())
                .unwrap_or_default();
            (preview, sessions_board::extract_prs(&blob), involving_me)
        }
        Err(_) => (String::new(), Vec::new(), true),
    }
}

/// Sessions board: conversations in columns by rules and pins.
#[tauri::command]
async fn get_sessions_board(
    state: State<'_, ShellState>,
    filter: Option<String>,
    query: Option<String>,
) -> Result<sessions_board::Page, String> {
    sessions_board_get(&state, false, filter, query).await
}

/// [`get_sessions_board`], for a paired browser when `web`.
async fn sessions_board_get(
    state: &ShellState,
    web: bool,
    filter: Option<String>,
    query: Option<String>,
) -> Result<sessions_board::Page, String> {
    let filter = sessions_board::PersonalFilter::parse(filter.as_deref().unwrap_or("everyone"));
    let query = query.unwrap_or_default();
    let dir = state.config_dir.clone();
    let facts = sessions_board_facts(state, web);
    off_main(move || Ok(sessions_board::page_from(&dir, facts, filter, &query))).await?
}

#[tauri::command]
async fn sessions_board_stamp(state: State<'_, ShellState>) -> Result<u64, String> {
    let dir = state.config_dir.clone();
    off_main(move || Ok(sessions_board::stamp(&dir))).await?
}

#[tauri::command]
async fn sessions_board_pin(
    state: State<'_, ShellState>,
    id: String,
    column_id: String,
    filter: Option<String>,
    query: Option<String>,
) -> Result<sessions_board::Page, String> {
    sessions_board_pin_for(&state, false, id, column_id, filter, query).await
}

/// [`sessions_board_pin`], for a paired browser when `web`.
async fn sessions_board_pin_for(
    state: &ShellState,
    web: bool,
    id: String,
    column_id: String,
    filter: Option<String>,
    query: Option<String>,
) -> Result<sessions_board::Page, String> {
    let filter = sessions_board::PersonalFilter::parse(filter.as_deref().unwrap_or("everyone"));
    let query = query.unwrap_or_default();
    let dir = state.config_dir.clone();
    let facts = sessions_board_facts(state, web);
    off_main(move || {
        let mut spec = sessions_board::load(&dir);
        sessions_board::set_pin(&mut spec, &id, &column_id)?;
        sessions_board::save(&dir, &spec)?;
        Ok(sessions_board::page_from(&dir, facts, filter, &query))
    })
    .await?
}

/// Board agent reshape: deterministic column edits (Auto only; no model picker).
#[tauri::command]
async fn sessions_board_reshape(
    state: State<'_, ShellState>,
    instruction: String,
    filter: Option<String>,
    query: Option<String>,
) -> Result<sessions_board::ReshapeResult, String> {
    sessions_board_reshape_for(&state, false, instruction, filter, query).await
}

/// [`sessions_board_reshape`], for a paired browser when `web`.
async fn sessions_board_reshape_for(
    state: &ShellState,
    web: bool,
    instruction: String,
    filter: Option<String>,
    query: Option<String>,
) -> Result<sessions_board::ReshapeResult, String> {
    let filter = sessions_board::PersonalFilter::parse(filter.as_deref().unwrap_or("everyone"));
    let query = query.unwrap_or_default();
    let dir = state.config_dir.clone();
    let facts = sessions_board_facts(state, web);
    off_main(move || {
        let mut spec = sessions_board::load(&dir);
        let note = sessions_board::reshape(&mut spec, &instruction)?;
        sessions_board::save(&dir, &spec)?;
        Ok(sessions_board::ReshapeResult {
            note,
            page: sessions_board::page_from(&dir, facts, filter, &query),
        })
    })
    .await?
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
/// Start a playbook on a card: linked, then started as Work.
#[tauri::command]
async fn workboard_playbook(
    state: State<'_, ShellState>,
    id: String,
    playbook: String,
    name: String,
) -> Result<workboard::Work, String> {
    let dir = state.config_dir.clone();
    off_main(move || workboard::start_playbook(&dir, &id, &playbook, &name)).await?
}

/// Home's Playbooks row: start that playbook on a new Workboard card.
#[tauri::command]
async fn home_run_playbook(
    state: State<'_, ShellState>,
    id: String,
) -> Result<workboard::Work, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        let page = playbooks::load(&dir);
        let row = page
            .rows
            .iter()
            .find(|r| r.id == id)
            .ok_or("That playbook is gone. Open Playbooks to pick another.")?;
        workboard::start_playbook_card(&dir, &row.id, &row.name, &row.summary)
    })
    .await?
}

/// Make it recurring: an automation from the card that then owns it.
#[tauri::command]
async fn workboard_recur(
    state: State<'_, ShellState>,
    id: String,
    choice: usize,
    clock: automations::Clock,
) -> Result<workboard::Page, String> {
    let dir = state.config_dir.clone();
    let clock = now_clock(clock);
    off_main(move || workboard::make_recurring(&dir, &id, choice, clock, automations::Author::Here))
        .await?
}

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

/// Connectors → GitHub: whether a token is saved (never the token).
#[tauri::command]
async fn get_github(state: State<'_, ShellState>) -> Result<github::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || github::view(&dir)).await
}

/// Save the GitHub token to the keyring; empty removes it.
#[tauri::command]
async fn save_github_token(
    state: State<'_, ShellState>,
    token: String,
) -> Result<github::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || github::save_token(&dir, &token)).await?
}

/// Who am I or List repos: one read-only call to api.github.com.
#[tauri::command]
async fn run_github(
    state: State<'_, ShellState>,
    tool: github::Tool,
) -> Result<github::Ran, String> {
    let dir = state.config_dir.clone();
    off_main(move || github::run(&dir, tool)).await?
}

#[tauri::command]
async fn get_hooks(state: State<'_, ShellState>) -> Result<project_hooks::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || project_hooks::load(&dir, &workspace, grokhub_core::user_home().as_deref()))
        .await
}

#[tauri::command]
async fn set_hooks_trust(
    state: State<'_, ShellState>,
    trusted: bool,
) -> Result<project_hooks::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    off_main(move || {
        project_hooks::set_trust(
            &dir,
            &workspace,
            grokhub_core::user_home().as_deref(),
            trusted,
        )
    })
    .await?
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
    off_main(move || personality::load(&dir, &now)).await
}

/// The default voice for every chat; applies to the next message.
#[tauri::command]
fn set_personality(state: State<'_, ShellState>, id: String) -> Result<personality::Page, String> {
    let now = current(&state);
    let picked = personality::allowed(personality::pick(&state.config_dir, &id)?, now.adults_on())?;
    let next = ShellSettings {
        personality: personality::stored_id(&picked.id),
        ..now
    };
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(personalities(&state))
}

/// Settings → Personality → Adults-only personalities. The first time it
/// turns on, nothing changes and the page comes back with `ageCheck`, so the
/// one-time 18+ check comes first ([`confirm_adult`]).
#[tauri::command]
fn set_explicit_personalities(
    app: AppHandle,
    state: State<'_, ShellState>,
    on: bool,
) -> Result<personality::Page, String> {
    match current(&state).adults_only(on) {
        Some(next) => {
            let page = save_shell(&state, next)?;
            adults_changed(&app, &state);
            Ok(page)
        }
        None => Ok(personality::Page {
            age_check: true,
            ..personalities(&state)
        }),
    }
}

/// The 18+ check's yes: remembered for good, with when it was given (never a
/// birth date), and adults-only turns on.
#[tauri::command]
fn confirm_adult(
    app: AppHandle,
    state: State<'_, ShellState>,
) -> Result<personality::Page, String> {
    let page = save_shell(
        &state,
        current(&state).confirm_adult(grokhub_core::now_ms()),
    )?;
    adults_changed(&app, &state);
    Ok(page)
}

/// Adults-only decides which outfits the desktop companion may wear, so her
/// window, the dock's wardrobe and Labs hear it.
fn adults_changed(app: &AppHandle, state: &ShellState) {
    #[cfg(desktop)]
    desktop_nyx::publish(app, &current(state));
    #[cfg(mobile)]
    let _ = (app, state);
}

/// Settings → Personality → Heat. Clean is not a stop on the dial, so it reads
/// as Cheeky. Applies to the next message.
#[tauri::command]
fn set_quip_heat(
    state: State<'_, ShellState>,
    heat: grokhub_core::personality::Heat,
) -> Result<personality::Page, String> {
    save_shell(
        &state,
        ShellSettings {
            quip_heat: settings::dial_heat(heat),
            ..current(&state)
        },
    )
}

/// Settings → Personality → Dynamic chat. Applies to the next message.
#[tauri::command]
fn set_dynamic_chat(state: State<'_, ShellState>, on: bool) -> Result<personality::Page, String> {
    save_shell(
        &state,
        ShellSettings {
            dynamic_chat: on,
            ..current(&state)
        },
    )
}

/// Saves the shell's settings and returns the Personality page they make.
fn save_shell(state: &ShellState, next: ShellSettings) -> Result<personality::Page, String> {
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next;
    Ok(personalities(state))
}

fn personalities(state: &ShellState) -> personality::Page {
    personality::load(&state.config_dir, &current(state))
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
        now.adults_on(),
        nyx::activity(mood, tool.as_deref()),
    )
}

/// Settings → Personality's pictures, by preset id.
#[tauri::command]
fn nyx_tiles(state: State<'_, ShellState>, ids: Vec<String>) -> HashMap<String, String> {
    nyx::tiles(&ids, current(&state).adults_on())
}

/// Labs / dock: desktop companion state (switch, undock, wardrobe, effects).
#[cfg(desktop)]
#[tauri::command]
fn desktop_nyx_get(state: State<'_, ShellState>) -> desktop_nyx::State {
    desktop_nyx::state_now(&current(&state))
}

/// What the chat on screen is doing: her voice look, and the activity (a
/// running tool's title, else `mood`) whose outfit "Pick for me" wears.
/// Every page and her desk hear it when it changed.
#[cfg(desktop)]
#[tauri::command]
fn desktop_nyx_look(
    app: AppHandle,
    state: State<'_, ShellState>,
    look: desktop_nyx::VoiceLook,
    mood: Option<grokhub_core::companion::Mood>,
    tool: Option<String>,
) {
    let mood = Some(nyx::activity(mood, tool.as_deref()));
    if desktop_nyx::set_life(desktop_nyx::Life { look, mood }) {
        desktop_nyx::publish(&app, &current(&state));
    }
}

/// The chat's spoken reply, for her talk hop: her desk alone hears it.
#[cfg(desktop)]
#[tauri::command]
fn desktop_nyx_talk(app: AppHandle, talk: Option<desktop_nyx::Talk>) {
    use tauri::Emitter;
    let talk = talk.and_then(desktop_nyx::Talk::checked);
    let _ = app.emit_to(desktop_nyx::LABEL, desktop_nyx::TALK_EVENT, talk);
}

/// Every desktop-companion write: save, store, apply the window when asked
/// (`show_window` hides it while docked), then the tray and every page hear
/// it, even when the window couldn't be applied.
#[cfg(desktop)]
fn desktop_nyx_commit(
    app: &AppHandle,
    state: &ShellState,
    next: ShellSettings,
    window: bool,
) -> Result<desktop_nyx::State, String> {
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    let shown = if window {
        desktop_nyx::show_window(app, &next).map(|()| {
            let _ = desktop_nyx::apply_behind(app, next.desktop_nyx_behind);
        })
    } else {
        Ok(())
    };
    let told = desktop_nyx::publish(app, &next);
    shown.map(|()| told)
}

/// One desktop-companion write at a time, from the read to the window: two
/// pop-outs at once (a double click, or the tray during the dock's) would
/// each build her window, and a read taken outside could undo a newer
/// write. Only async callers take it; the main thread never waits on it,
/// because building her window needs the main thread.
#[cfg(desktop)]
static DESKTOP_NYX_WRITES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(desktop)]
async fn desktop_nyx_write(
    app: &AppHandle,
    state: &ShellState,
    window: bool,
    change: impl FnOnce(ShellSettings) -> Result<ShellSettings, String>,
) -> Result<desktop_nyx::State, String> {
    let _one = DESKTOP_NYX_WRITES.lock().await;
    let next = change(current(state))?;
    desktop_nyx_commit(app, state, next, window)
}

/// The desktop-companion writers are async: building her webview from a
/// sync command (the main thread) can hang on Windows, and the write lock
/// must never block the main thread.
#[cfg(desktop)]
#[tauri::command]
async fn desktop_nyx_set(
    app: AppHandle,
    state: State<'_, ShellState>,
    patch: desktop_nyx::Patch,
) -> Result<desktop_nyx::State, String> {
    desktop_nyx_write(&app, &state, true, |now| {
        Ok(desktop_nyx::apply_patch(now, patch))
    })
    .await
}

#[cfg(desktop)]
#[tauri::command]
async fn desktop_nyx_undock(
    app: AppHandle,
    state: State<'_, ShellState>,
) -> Result<desktop_nyx::State, String> {
    desktop_nyx_write(&app, &state, true, |mut next| {
        next.desktop_nyx = true;
        next.desktop_nyx_undocked = true;
        Ok(next)
    })
    .await
}

#[cfg(desktop)]
#[tauri::command]
async fn desktop_nyx_dock(
    app: AppHandle,
    state: State<'_, ShellState>,
) -> Result<desktop_nyx::State, String> {
    desktop_nyx_write(&app, &state, true, |now| {
        Ok(desktop_nyx::apply_patch(
            now,
            desktop_nyx::Patch {
                undocked: Some(false),
                ..desktop_nyx::Patch::default()
            },
        ))
    })
    .await
}

#[cfg(desktop)]
#[tauri::command]
async fn desktop_nyx_set_outfit(
    app: AppHandle,
    state: State<'_, ShellState>,
    id: Option<String>,
) -> Result<desktop_nyx::State, String> {
    desktop_nyx_write(&app, &state, false, |now| desktop_nyx::set_outfit(now, id)).await
}

/// The tray's "Nyx on desktop". The settings decide, never the item's own
/// tick, and the window work runs off the menu callback.
#[cfg(desktop)]
fn desktop_nyx_from_tray(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<ShellState>();
        let toggled = desktop_nyx_write(&app, &state, true, |now| {
            desktop_nyx::tray_toggle(now).ok_or_else(|| "Labs is off".to_string())
        })
        .await;
        // A click flips the tick itself: a click that changed nothing (Labs
        // off, or the save failed) must put the settings' answer back.
        if toggled.is_err() {
            desktop_nyx::refresh_tray(&current(&state));
        }
    });
}

/// Her page's parts that take the mouse; the desk watch lets clicks
/// through everywhere else.
#[cfg(desktop)]
#[tauri::command]
fn desktop_nyx_set_hit(boxes: Vec<desktop_nyx::HitBox>) {
    desktop_nyx::set_hit(boxes);
}

/// A press on her: the system drags her window, and the desk watch saves
/// where she lands. Sync, so it runs on the main thread, as dragging needs.
#[cfg(desktop)]
#[tauri::command]
fn desktop_nyx_drag(app: AppHandle) -> Result<(), String> {
    desktop_nyx::start_drag(&app)
}

/// The desk watch: every frame, click-through follows the cursor, and a
/// dropped window saves its spot through the one desktop-companion write.
#[cfg(desktop)]
fn desktop_nyx_watch(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("desktop-nyx".into())
        .spawn(move || loop {
            std::thread::sleep(desktop_nyx::CURSOR_FRAME);
            let frame = desktop_nyx::tick(&app);
            let state = app.state::<ShellState>();
            if let Some(drop) = frame.landing {
                // The write publishes, and the state it sends carries the
                // game hiding too.
                let _ =
                    tauri::async_runtime::block_on(desktop_nyx_write(&app, &state, false, |now| {
                        Ok(desktop_nyx::save_pos(now, drop.pos, drop.origin, drop.size))
                    }));
            } else if frame.game_changed {
                desktop_nyx::publish(&app, &current(&state));
            }
        });
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

/// Switch a listed skill on or off for chat. Desktop only.
#[tauri::command]
async fn set_skill_on(
    state: State<'_, ShellState>,
    name: String,
    on: bool,
) -> Result<skills::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    let home = grokhub_core::user_home();
    off_main(move || skills::set_on(&dir, &workspace, home.as_deref(), &name, on)).await?
}

/// Make a cabin skill from a name and what it should do. Desktop only.
#[tauri::command]
async fn create_skill(
    state: State<'_, ShellState>,
    name: String,
    what: String,
) -> Result<skills::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    let home = grokhub_core::user_home();
    off_main(move || skills::create(&dir, &workspace, home.as_deref(), &name, &what)).await?
}

/// The fields Edit shows for a cabin skill. Desktop only.
#[tauri::command]
async fn get_skill_detail(
    state: State<'_, ShellState>,
    name: String,
) -> Result<skills::Detail, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    let home = grokhub_core::user_home();
    off_main(move || skills::detail(&dir, &workspace, home.as_deref(), &name)).await?
}

/// Edit a cabin skill's description, trigger and steps. Desktop only.
#[tauri::command]
async fn edit_skill(
    state: State<'_, ShellState>,
    name: String,
    description: String,
    trigger: String,
    instructions: String,
) -> Result<skills::Page, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    let home = grokhub_core::user_home();
    off_main(move || {
        skills::edit(
            &dir,
            &workspace,
            home.as_deref(),
            &name,
            &description,
            &trigger,
            &instructions,
        )
    })
    .await?
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

/// Skills Hub search across ClawHub, SkillsSH, GitHub, and well-known hosts.
#[tauri::command]
async fn search_skills_hub(query: String) -> Result<Vec<skills::HubHit>, String> {
    off_main(move || skills::hub_search(&query)).await?
}

/// Install a Skills Hub hit into cabin skills (959 scanner + quarantine).
#[tauri::command]
async fn install_skills_hub(
    state: State<'_, ShellState>,
    source: String,
    id: String,
    name: String,
    description: String,
) -> Result<skills::HubInstall, String> {
    let (dir, workspace) = (state.config_dir.clone(), workspace(&state));
    let home = grokhub_core::user_home();
    off_main(move || {
        skills::hub_install(
            &dir,
            &workspace,
            home.as_deref(),
            &source,
            &id,
            &name,
            &description,
        )
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
async fn get_battlenet() -> Result<battlenet::Page, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(battlenet::page).await
}

#[tauri::command]
async fn battlenet_play(product: String) -> Result<String, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(move || {
        let games = grokhub_core::battlenet::detect();
        battlenet::play(&games, &product)
    })
    .await?
}

#[tauri::command]
async fn battlenet_open_folder(product: String) -> Result<String, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(move || {
        let games = grokhub_core::battlenet::detect();
        battlenet::open_folder(&games, &product)
    })
    .await?
}

#[tauri::command]
async fn companion_frame(
    state: State<'_, ShellState>,
    note_id: u32,
    last_played: u64,
    steam_app_id: Option<u32>,
) -> Result<game_companion::Frame, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        let news = steam_app_id.and_then(|id| game_companion::steam_news(id, last_played));
        game_companion::frame(&dir, note_id, steam_app_id, news)
    })
    .await
}

/// The note chips come from the game companion (Pulse) as well as the
/// Library's game page, so either one lets a note be read and saved.
fn notes_allowed() -> Result<(), String> {
    let caps = capabilities::current();
    capabilities::needs(caps.library || caps.pulse, "Library")
}

#[tauri::command]
async fn companion_set_note(
    app: AppHandle,
    state: State<'_, ShellState>,
    app_id: u32,
    text: String,
) -> Result<String, String> {
    notes_allowed()?;
    let dir = state.config_dir.clone();
    let saved = off_main(move || game_companion::set_note(&dir, app_id, &text)).await??;
    // A saved note answers the closed game's offer.
    if game_companion::lock(&state.game_session).noted(app_id) {
        let _ = app.emit(game_companion::EVENT, ());
    }
    Ok(saved)
}

/// The note dialog's game name and saved note.
#[tauri::command]
async fn companion_note(
    state: State<'_, ShellState>,
    app_id: u32,
) -> Result<game_companion::NoteFor, String> {
    notes_allowed()?;
    let dir = state.config_dir.clone();
    let session = game_companion::lock(&state.game_session).clone();
    off_main(move || {
        game_companion::note_for(&dir, &session, app_id, || {
            grokhub_core::steam::detect_root()
                .map(|r| grokhub_core::steam::read_library(&r))
                .unwrap_or_default()
        })
    })
    .await
}

/// Settings → Behavior → Game launch companion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompanionSwitch {
    on: bool,
    ping: bool,
    /// What the last change did; empty on a read.
    note: String,
}

fn companion_switch(dir: &std::path::Path, note: String) -> CompanionSwitch {
    let s = game_companion::load(dir);
    CompanionSwitch {
        on: s.on,
        ping: s.ping,
        note,
    }
}

#[tauri::command]
async fn get_game_companion(state: State<'_, ShellState>) -> Result<CompanionSwitch, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        // The old desktop app's switch carries over before the first read.
        let _ = game_companion::migrate_legacy(&dir);
        companion_switch(&dir, String::new())
    })
    .await
}

/// Turns the companion on or off. Off forgets the session at once, so its
/// note chips leave the rows without waiting for the next sample.
fn turn_companion(app: &AppHandle, state: &ShellState, on: bool) -> Result<String, String> {
    let note = game_companion::set(&state.config_dir, on)?;
    if !on && game_companion::lock(&state.game_session).clear() {
        let _ = app.emit(game_companion::EVENT, ());
    }
    Ok(note)
}

#[tauri::command]
async fn set_game_companion(
    app: AppHandle,
    state: State<'_, ShellState>,
    on: bool,
) -> Result<CompanionSwitch, String> {
    let note = turn_companion(&app, &state, on)?;
    Ok(companion_switch(&state.config_dir, note))
}

#[tauri::command]
async fn set_game_companion_ping(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<CompanionSwitch, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        let note = game_companion::set_ping(&dir, on)?;
        Ok(companion_switch(&dir, note))
    })
    .await?
}

#[tauri::command]
async fn get_game_watch(state: State<'_, ShellState>) -> Result<game_watch::GameWatch, String> {
    let dir = state.config_dir.clone();
    off_main(move || game_watch::read(&dir)).await
}

#[tauri::command]
async fn set_game_watch(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<game_watch::GameWatch, String> {
    let dir = state.config_dir.clone();
    off_main(move || game_watch::set(&dir, on, None, grokhub_core::game_watch_gsi::GSI_PORT))
        .await?
}

/// Settings → Behavior → Living wall: off until switched on.
#[cfg(desktop)]
#[tauri::command]
async fn get_living_wall(state: State<'_, ShellState>) -> Result<living_wall::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || living_wall::view(&dir)).await
}

#[cfg(desktop)]
#[tauri::command]
async fn set_living_wall(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<living_wall::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || living_wall::set(&dir, on)).await?
}

/// Settings → Pulse: PC health while gaming, off until switched on.
#[tauri::command]
async fn get_pc_health(state: State<'_, ShellState>) -> Result<pc_health::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_health::view(&dir)).await
}

#[tauri::command]
async fn set_pc_health(state: State<'_, ShellState>, on: bool) -> Result<pc_health::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_health::set_enabled(&dir, on)).await?
}

/// Settings → Pulse: fixes that run on their own and reminder notifications.
#[tauri::command]
async fn get_pulse_auto(state: State<'_, ShellState>) -> Result<auto_fixes::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || auto_fixes::view(&dir)).await
}

#[tauri::command]
async fn set_pulse_auto_fix(
    state: State<'_, ShellState>,
    id: String,
    on: bool,
) -> Result<auto_fixes::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || auto_fixes::set_fix(&dir, &id, on)).await?
}

#[tauri::command]
async fn set_pulse_auto_pause(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<auto_fixes::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || auto_fixes::set_pause(&dir, on)).await?
}

#[tauri::command]
async fn set_proactive_reminders(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<auto_fixes::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || auto_fixes::set_reminders(&dir, on)).await?
}

#[tauri::command]
async fn get_steam_pulse(state: State<'_, ShellState>) -> Result<steam_checks::SteamPulse, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || steam_checks::pulse_status(&dir)).await
}

#[tauri::command]
async fn set_steam_achievements(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<steam_checks::SteamPulse, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || steam_checks::set_achievements(&dir, on)).await?
}

#[tauri::command]
async fn set_steam_wishlist(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<steam_checks::SteamPulse, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || steam_checks::set_wishlist(&dir, on)).await?
}

#[tauri::command]
async fn set_steam_wishlist_threshold(
    state: State<'_, ShellState>,
    threshold: String,
) -> Result<steam_checks::SteamPulse, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || steam_checks::set_threshold(&dir, &threshold)).await?
}

#[tauri::command]
async fn save_steam_id(
    state: State<'_, ShellState>,
    draft: String,
    use_local: bool,
) -> Result<steam_checks::SteamPulse, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || steam_checks::save_id(&dir, &draft, use_local)).await?
}

#[tauri::command]
async fn forget_steam_id(state: State<'_, ShellState>) -> Result<steam_checks::SteamPulse, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || steam_checks::forget_id(&dir)).await?
}

#[tauri::command]
async fn steam_check_now(
    state: State<'_, ShellState>,
    module: String,
) -> Result<steam_checks::SteamPulse, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || steam_checks::check_now(&dir, &module)).await?
}

#[tauri::command]
async fn get_local_model_news(
    state: State<'_, ShellState>,
) -> Result<local_model_checks::LocalModelNews, String> {
    let caps = capabilities::current();
    capabilities::needs(caps.pulse, "Pulse")?;
    capabilities::needs(caps.local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_model_checks::status(&dir)).await
}

#[tauri::command]
async fn set_local_model_news(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<local_model_checks::LocalModelNews, String> {
    let caps = capabilities::current();
    capabilities::needs(caps.pulse, "Pulse")?;
    capabilities::needs(caps.local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_model_checks::set_on(&dir, on)).await?
}

#[tauri::command]
async fn local_model_news_check_now(
    state: State<'_, ShellState>,
) -> Result<local_model_checks::LocalModelNews, String> {
    let caps = capabilities::current();
    capabilities::needs(caps.pulse, "Pulse")?;
    capabilities::needs(caps.local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_model_checks::check_now(&dir)).await?
}

#[tauri::command]
async fn game_wow(state: State<'_, ShellState>) -> Result<games::WowView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::wow_view(game_watch::is_on(&dir))).await
}

#[tauri::command]
async fn game_wow_addon(
    install: String,
    flavor: String,
    action: String,
) -> Result<games::WowView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(move || games::wow_addon_action(Path::new(&install), &flavor, &action)).await?
}

#[tauri::command]
async fn game_gw2(state: State<'_, ShellState>) -> Result<games::Gw2View, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::gw2_view(&dir, grokhub_core::now_ms())).await
}

#[tauri::command]
async fn game_gw2_refresh(state: State<'_, ShellState>) -> Result<games::Gw2View, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::gw2_refresh(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn game_gw2_start_key(
    state: State<'_, ShellState>,
    key: String,
) -> Result<games::Gw2KeyCard, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::gw2_start_key(&dir, key)).await?
}

#[tauri::command]
async fn game_gw2_answer_key(
    state: State<'_, ShellState>,
    id: String,
    approve: bool,
) -> Result<games::Gw2View, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::gw2_answer_key(&dir, &id, approve, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn game_gw2_remove_key(state: State<'_, ShellState>) -> Result<games::Gw2View, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::gw2_remove_key(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn game_eso() -> Result<games::EsoView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(|| games::eso_view(grokhub_core::steam::detect_root().as_deref())).await
}

#[tauri::command]
async fn game_eso_addon(dir: String, install: bool) -> Result<games::EsoView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(move || games::eso_addon_action(Path::new(&dir), install)).await?
}

#[tauri::command]
async fn game_osrs(state: State<'_, ShellState>) -> Result<games::OsrsView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::osrs_view(&dir, grokhub_core::now_ms())).await
}

#[tauri::command]
async fn game_osrs_save(
    state: State<'_, ShellState>,
    name: String,
    kind: String,
) -> Result<games::OsrsView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::osrs_save(&dir, &name, &kind, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn game_osrs_forget(state: State<'_, ShellState>) -> Result<games::OsrsView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::osrs_forget(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn game_osrs_refresh(state: State<'_, ShellState>) -> Result<games::OsrsView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::osrs_refresh(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn game_destiny(state: State<'_, ShellState>) -> Result<games::DestinyView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::destiny_view(&dir, grokhub_core::now_ms())).await
}

#[tauri::command]
async fn game_destiny_refresh(state: State<'_, ShellState>) -> Result<games::DestinyView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::destiny_refresh(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn game_destiny_start_key(
    state: State<'_, ShellState>,
    key: String,
) -> Result<games::DestinyKeyCard, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::destiny_start_key(&dir, key)).await?
}

#[tauri::command]
async fn game_destiny_answer_key(
    state: State<'_, ShellState>,
    id: String,
    approve: bool,
) -> Result<games::DestinyView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::destiny_answer_key(&dir, &id, approve, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn game_destiny_save_name(
    state: State<'_, ShellState>,
    name: String,
) -> Result<games::DestinyView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::destiny_save_name(&dir, &name, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn game_destiny_remove(state: State<'_, ShellState>) -> Result<games::DestinyView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    let dir = state.config_dir.clone();
    off_main(move || games::destiny_remove(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn game_poe(app_id: u32) -> Result<games::PoeView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(move || games::poe_view(app_id)).await
}

#[tauri::command]
async fn game_elite() -> Result<games::EliteView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(games::elite_view).await
}

#[tauri::command]
async fn game_factorio() -> Result<games::FactorioView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(games::factorio_view).await
}

#[tauri::command]
async fn game_factorio_mod(dir: String, install: bool) -> Result<games::FactorioView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(move || games::factorio_mod_action(Path::new(&dir), install)).await?
}

#[tauri::command]
async fn game_stardew() -> Result<games::StardewView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(games::stardew_view).await
}

#[tauri::command]
async fn game_minecraft() -> Result<games::MinecraftView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(games::minecraft_view).await
}

#[tauri::command]
async fn game_rl() -> Result<games::RocketLeagueView, String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(games::rocket_league_view).await
}

/// A Home tile's Turn on, for a switch that needs nothing else. The rest
/// open their Settings row instead.
#[tauri::command]
async fn home_turn_on(state: State<'_, ShellState>, feature: String) -> Result<(), String> {
    let dir = state.config_dir.clone();
    match feature.as_str() {
        #[cfg(desktop)]
        "net_log" => off_main(move || net_watch::turn_on(&dir)).await?,
        _ => {
            let _ = dir;
            Err("That switch is in Settings.".into())
        }
    }
}

/// A Home tile's Set up (or Undo setup): the module's one-time setup behind
/// the OS's own password or admin prompt. Answers with one plain sentence on
/// how it went; the tile shows the rest on its next read.
#[tauri::command]
async fn home_setup(
    app: AppHandle,
    state: State<'_, ShellState>,
    module: String,
    undo: bool,
) -> Result<String, String> {
    match module.as_str() {
        #[cfg(desktop)]
        "frame_rate" => off_main(move || home_fps::run_setup(undo).sentence(undo)).await,
        // Achievements and Deals: the Steam account on this PC, then a check.
        "achievements" | "deals" => {
            let dir = state.config_dir.clone();
            off_main(move || steam_checks::turn(&dir, &module, !undo)).await?
        }
        // Patch Notes: the launch companion's own switch on this app.
        "patch_notes" => turn_companion(&app, &state, !undo),
        _ => {
            let _ = undo;
            Err("That module has no setup here.".into())
        }
    }
}

/// Home's Updates & Drivers page. Asks the app's own updater first (when this
/// install updates itself), then reads every other source off the main thread.
#[tauri::command]
async fn updates_page(
    app: AppHandle,
    state: State<'_, ShellState>,
) -> Result<grokhub_core::home::updates::UpdatesPage, String> {
    let dir = state.config_dir.clone();
    let updatable = shell_update::UpdateView::now(&dir).updatable;
    #[cfg(desktop)]
    let newer = if updatable {
        // Offline or a failed check: the page still lists the rest.
        shell_update::plugin::check(&app, &dir)
            .await
            .ok()
            .flatten()
            .map(|u| u.version)
    } else {
        None
    };
    #[cfg(not(desktop))]
    let newer = {
        let _ = &app;
        None
    };
    off_main(move || home_updates::page(&home_updates::read(&dir, newer, updatable))).await
}

/// One offer on the Updates page: opens its fixed page, or runs the system or
/// app update (behind the OS prompt, never while a game runs).
#[tauri::command]
async fn run_update_offer(
    app: AppHandle,
    state: State<'_, ShellState>,
    offer: grokhub_core::home::updates::Offer,
) -> Result<String, String> {
    if let Some(page) = home_updates::offer_page(offer) {
        use tauri_plugin_opener::OpenerExt;
        return app
            .opener()
            .open_url(page, None::<&str>)
            .map(|_| String::new())
            .map_err(|e| e.to_string());
    }
    let dir = state.config_dir.clone();
    off_main(move || home_updates::run_offer(offer, &dir)).await?
}

/// A Home tile's Copy setup: the setup's steps, to paste by hand.
#[tauri::command]
fn home_setup_steps(module: String) -> Result<String, String> {
    match module.as_str() {
        #[cfg(desktop)]
        "frame_rate" => Ok(grokhub_agent::indexers::display_fps::setup::copy_text(
            false,
        )),
        _ => Err("That module has no setup here.".into()),
    }
}

/// Home's Squad tile: open Steam's own friends list.
#[tauri::command]
async fn library_friends() -> Result<(), String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(move || library::open_friends(grokhub_core::steam::detect_root().as_deref())).await?
}

/// Home's Clips tile: Steam's Settings, where Game Recording turns on.
#[tauri::command]
async fn library_steam_settings() -> Result<(), String> {
    capabilities::needs(capabilities::current().library, "Library")?;
    off_main(move || library::open_settings(grokhub_core::steam::detect_root().as_deref())).await?
}

/// The Mods rail. Starts a discovery sweep and For you's due fetches in the
/// background when they are due and Offline only is off.
#[tauri::command]
async fn mods_games(state: State<'_, ShellState>) -> Result<mods::GamesView, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    let adult_ok = current(&state).adults_on();
    off_main(move || {
        let now_ms = grokhub_core::now_ms();
        let rail = mods::rail_now(now_ms);
        let offline = grokhub_core::offline::enabled();
        if !offline {
            let client = mods::client(&dir);
            mods::sweep_if_due(dir.clone(), client.clone(), &rail, now_ms);
            mods::refresh_pools(dir.clone(), client, &rail, now_ms);
        }
        let store = grokhub_core::mods::discover::Store::load(&dir);
        let prefs = grokhub_core::mods::suggest::Prefs::load(&dir);
        mods::games_view(
            &rail,
            &store,
            &prefs,
            adult_ok,
            offline,
            mods::sweeping(),
            now_ms / 1000,
        )
    })
    .await
}

/// One page of a game's Popular, Trending or Updated list on one source.
#[tauri::command]
async fn mods_browse(
    state: State<'_, ShellState>,
    game: String,
    source: String,
    list: String,
    page: u32,
) -> Result<mods::PageView, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    let adult_ok = current(&state).adults_on();
    off_main(move || {
        let now_ms = grokhub_core::now_ms();
        let (_, src) = mods::game_source(&mods::rail_now(now_ms), &game, &source)?;
        let mut q = grokhub_core::mods::Query::browse(mods::browse_of(&list)?);
        q.page = page;
        let offline = grokhub_core::offline::enabled();
        Ok(mods::search(
            &mods::client(&dir),
            &game,
            src,
            &q,
            adult_ok,
            offline,
            now_ms,
        ))
    })
    .await?
}

/// One page of a search for one game on one source.
#[tauri::command]
async fn mods_search(
    state: State<'_, ShellState>,
    game: String,
    source: String,
    text: String,
    page: u32,
) -> Result<mods::PageView, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    let adult_ok = current(&state).adults_on();
    off_main(move || {
        let now_ms = grokhub_core::now_ms();
        let (_, src) = mods::game_source(&mods::rail_now(now_ms), &game, &source)?;
        let mut q = grokhub_core::mods::Query::search(text.trim());
        q.page = page;
        let offline = grokhub_core::offline::enabled();
        Ok(mods::search(
            &mods::client(&dir),
            &game,
            src,
            &q,
            adult_ok,
            offline,
            now_ms,
        ))
    })
    .await?
}

/// One page of a game's New finds; opening it marks them seen.
#[tauri::command]
async fn mods_new_finds(
    state: State<'_, ShellState>,
    game: String,
    source: String,
    page: u32,
) -> Result<mods::PageView, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    let adult_ok = current(&state).adults_on();
    off_main(move || {
        let now_ms = grokhub_core::now_ms();
        let (_, src) = mods::game_source(&mods::rail_now(now_ms), &game, &source)?;
        Ok(mods::new_finds(
            &dir,
            &game,
            src,
            page,
            adult_ok,
            now_ms / 1000,
        ))
    })
    .await?
}

/// The For you cards.
#[tauri::command]
async fn mods_for_you(state: State<'_, ShellState>) -> Result<Vec<mods::ForYouRow>, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    let adult_ok = current(&state).adults_on();
    off_main(move || {
        let now_ms = grokhub_core::now_ms();
        let rail = mods::rail_now(now_ms);
        let root = grokhub_core::steam::detect_root();
        let client = mods::client(&dir);
        let picked = mods::for_you(
            &dir,
            &client,
            &rail,
            adult_ok,
            root.as_deref(),
            now_ms / 1000,
        );
        mods::for_you_rows(&rail, &picked)
    })
    .await
}

/// "Not for me" on a For you card: that mod never shows there again.
#[tauri::command]
async fn mods_not_for_me(
    state: State<'_, ShellState>,
    source: String,
    id: String,
) -> Result<(), String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        let src = grokhub_core::mods::Source::from_id(&source)
            .ok_or_else(|| format!("No source {source}."))?;
        mods::change_prefs(&dir, |p| {
            p.not_interested
                .insert(grokhub_core::mods::suggest::key(src, &id));
        })
    })
    .await?
}

/// Hide or show For you's suggestions for one game.
#[tauri::command]
async fn mods_hide_game(
    state: State<'_, ShellState>,
    game: String,
    hidden: bool,
) -> Result<(), String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        mods::change_prefs(&dir, |p| {
            if hidden {
                p.hidden_games.insert(game);
            } else {
                p.hidden_games.remove(&game);
            }
        })
    })
    .await?
}

/// Home's Clips tile: play the clip at this place in its list.
#[tauri::command]
async fn home_open_clip(index: usize) -> Result<(), String> {
    off_main(move || home_tiles::open_clip(index)).await?
}

/// Home's PC Loadout → Details: the hardware read as rows, or None until the
/// launch read lands.
#[cfg(desktop)]
#[tauri::command]
async fn home_pc_details() -> Result<Option<Vec<grokhub_core::home::TileRow>>, String> {
    off_main(home_tiles::pc_details).await
}

/// Settings → Privacy & data → What the app learned: the learning engine's
/// beliefs as lines (`grokhub_core::learn::view`), the same ones the cabin's
/// `/learned` prints.
#[tauri::command]
async fn get_learned() -> Result<grokhub_core::learn::view::View, String> {
    off_main(|| grokhub_core::learn::hub::view(grokhub_core::now_ms())).await
}

/// Any button on the page, a trust dial included. Saved at once; the list comes
/// back as it is after.
#[tauri::command]
async fn learned_act(
    act: grokhub_core::learn::view::Act,
) -> Result<grokhub_core::learn::view::View, String> {
    off_main(move || grokhub_core::learn::view::act(act, grokhub_core::now_ms())).await?
}

/// The fault report, and backup and move: one of the page's four `/learned`
/// lines, answered with the text the cabin's chat shows. Nothing is sent.
#[tauri::command]
async fn learned_say(line: String) -> Result<String, String> {
    off_main(move || grokhub_core::learn::view::page_say(&line, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn get_pulse_sources(
    state: State<'_, ShellState>,
) -> Result<pulse_digests::SourcesView, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || Ok(pulse_digests::sources_view(&dir))).await?
}

#[tauri::command]
async fn save_pulse_sources(
    state: State<'_, ShellState>,
    view: pulse_digests::SourcesView,
) -> Result<pulse_digests::SourcesView, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse_digests::save_sources(&dir, view)).await?
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

/// Keeps a card's report for its chat's next message and hands back the rest.
fn card_seed(state: &ShellState, seed: pulse::Seed) -> pulse::Seed {
    if let Some(carry) = seed.carry.clone() {
        state
            .card_carry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(seed.conversation_id.clone(), carry);
    }
    seed
}

/// Discuss: the card's own chat and the words for its box, carrying the card.
#[tauri::command]
async fn pulse_discuss(state: State<'_, ShellState>, id: String) -> Result<pulse::Seed, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    let seed = off_main(move || pulse::discuss(&dir, &id, grokhub_core::now_ms())).await??;
    Ok(card_seed(&state, seed))
}

/// A card's fix chip: its words in the card's own chat. Nothing runs until sent.
/// Live offers open the checklist dialog via `pulse_live_open` instead.
#[tauri::command]
async fn pulse_offer(
    state: State<'_, ShellState>,
    id: String,
    index: usize,
) -> Result<pulse::Seed, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    let seed = off_main(move || pulse::offer(&dir, &id, index, grokhub_core::now_ms())).await??;
    Ok(card_seed(&state, seed))
}

/// Open a Live offer's checklist dialog.
#[tauri::command]
async fn pulse_live_open(
    state: State<'_, ShellState>,
    id: String,
    index: usize,
) -> Result<pulse_live::LiveView, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse_live::open(&dir, &id, index, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn pulse_live_current() -> Result<Option<pulse_live::LiveView>, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    pulse_live::current()
}

/// Start the live checklist, or Allow the step waiting for an OK.
#[tauri::command]
async fn pulse_live_allow(state: State<'_, ShellState>) -> Result<pulse_live::LiveView, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse_live::start_or_allow(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn pulse_live_deny(state: State<'_, ShellState>) -> Result<pulse_live::LiveView, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse_live::deny(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn pulse_live_stop(state: State<'_, ShellState>) -> Result<pulse_live::LiveView, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse_live::stop(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn pulse_live_undo(state: State<'_, ShellState>) -> Result<pulse_live::LiveView, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse_live::undo(&dir, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn pulse_live_close() -> Result<(), String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    pulse_live::close()
}

/// Remind me later: hidden until a time that fits the day.
#[tauri::command]
async fn pulse_remind(
    state: State<'_, ShellState>,
    id: String,
    day: pulse::Day,
) -> Result<pulse::Reminded, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::remind(&dir, &id, &day, grokhub_core::now_ms())).await?
}

#[tauri::command]
async fn pulse_why(state: State<'_, ShellState>, id: String) -> Result<String, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::why(&dir, &id)).await?
}

/// The Pulse Saved page: every saved find, newest first.
#[tauri::command]
async fn pulse_saved(state: State<'_, ShellState>) -> Result<Vec<pulse::Saved>, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::saved(&dir)).await?
}

/// Remove from saved: the one action a saved find has. Returns what is left.
#[tauri::command]
async fn pulse_unsave(
    state: State<'_, ShellState>,
    url: String,
) -> Result<Vec<pulse::Saved>, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || pulse::unsave(&dir, &url)).await?
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

/// What a weekly self-review card's Apply did, and the page after it.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewDone {
    line: String,
    page: pulse::Page,
}

/// A weekly self-review card's Apply change, Revert or Save idea: noted like
/// the card's other accepts, then applied the way the cabin applies it.
#[tauri::command]
async fn pulse_review_apply(
    state: State<'_, ShellState>,
    id: String,
    day: pulse::Day,
) -> Result<ReviewDone, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        let now = grokhub_core::now_ms();
        // A no-route wait's Retry now: check again now; the card stays.
        if model_wait::is_wait_card(&dir, &id)? {
            let line = if model_wait::retry_now(now) {
                model_wait::RETRYING.to_string()
            } else {
                review_cards::GONE.to_string()
            };
            let page = pulse::load(&dir, Some(&day), now)?;
            return Ok(ReviewDone { line, page });
        }
        pulse::act(&dir, &id, pulse::Act::Accept, &day, now)?;
        let line = review_cards::apply(&dir, &id, now)?;
        let page = pulse::load(&dir, Some(&day), now)?;
        Ok(ReviewDone { line, page })
    })
    .await?
}

/// A failing automation card's Pause it: the tap pauses that job once.
#[tauri::command]
async fn pulse_pause_automation(
    state: State<'_, ShellState>,
    id: String,
    day: pulse::Day,
) -> Result<ReviewDone, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        let now = grokhub_core::now_ms();
        let line = done_for_you::pause_from_card(&dir, &id, now)?;
        let page = pulse::load(&dir, Some(&day), now)?;
        Ok(ReviewDone { line, page })
    })
    .await?
}

/// A Done for you card's Undo: turns the paused job back on.
#[tauri::command]
async fn pulse_done_undo(
    state: State<'_, ShellState>,
    id: String,
    day: pulse::Day,
) -> Result<ReviewDone, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        let line = done_for_you::undo(&dir, &id)?;
        let page = pulse::load(&dir, Some(&day), grokhub_core::now_ms())?;
        Ok(ReviewDone { line, page })
    })
    .await?
}

/// A Done for you card's Stop doing this: the switch goes off.
#[tauri::command]
async fn pulse_done_stop(
    state: State<'_, ShellState>,
    day: pulse::Day,
) -> Result<ReviewDone, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        let line = done_for_you::stop(&dir)?;
        let page = pulse::load(&dir, Some(&day), grokhub_core::now_ms())?;
        Ok(ReviewDone { line, page })
    })
    .await?
}

/// A code-idea card's Copy as GitHub issue: the text, redacted, for the
/// page to put on the clipboard. The card stays.
#[tauri::command]
async fn pulse_review_issue(
    state: State<'_, ShellState>,
    id: String,
) -> Result<self_improve::IssueText, String> {
    capabilities::needs(capabilities::current().pulse, "Pulse")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        let home = grokhub_core::user_home().map(|h| h.to_string_lossy().into_owned());
        Ok(self_improve::IssueText {
            text: review_cards::issue(&dir, &id, home.as_deref())?,
            copied: self_improve::issue_copied(),
        })
    })
    .await?
}

/// The Imagine page: the gallery (the cabin's wall) and whether chat can sign a call.
#[tauri::command]
async fn get_imagine(state: State<'_, ShellState>) -> Result<imagine::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || imagine::load(&dir, imagine::ready()).map(imagine::with_account)).await?
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
        let run = imagine::start()?;
        imagine::make(&dir, &req, &|| run.stopped(), grokhub_core::now_ms()).map(|mut made| {
            made.page = imagine::with_account(made.page);
            made
        })
    })
    .await?
}

/// Imagine's own Sign in with Grok, step one: the code to type at x.ai.
#[tauri::command]
async fn imagine_sign_in_start() -> Result<oauth::Started, String> {
    capabilities::needs(capabilities::current().grok_sign_in, "Sign in with Grok")?;
    off_main(imagine::start_sign_in).await?
}

/// Step two, asked every few seconds by the Imagine page.
#[tauri::command]
async fn imagine_sign_in_poll() -> Result<oauth::Poll, String> {
    off_main(imagine::poll_sign_in).await?
}

/// Signs out of Imagine only, and hands back the page.
#[tauri::command]
async fn imagine_sign_out(state: State<'_, ShellState>) -> Result<imagine::Page, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        imagine::sign_out()?;
        imagine::load(&dir, imagine::ready()).map(imagine::with_account)
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

/// A picture a chat's tool made, as a data URL for its tool card. Only the
/// app's media folders are read (`imagine::chat_picture`).
#[tauri::command]
async fn chat_picture(state: State<'_, ShellState>, path: String) -> Result<String, String> {
    let dir = state.config_dir.clone();
    off_main(move || imagine::chat_picture(&dir, &path, &|_| true)).await?
}

#[tauri::command]
async fn get_ai_models(state: State<'_, ShellState>) -> Result<ai_models::AiModelsView, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || ai_models::view(&dir, "")).await
}

/// Settings → AI models → Claude (your plan): whether Claude Code is on this
/// computer and how it's signed in. Only the sign-in kind is read, never a
/// credential. Desktop only; the browser never reaches it.
#[cfg(desktop)]
#[tauri::command]
async fn claude_plan_status() -> Result<grokhub_agent::claude_plan::ClaudePlan, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    off_main(grokhub_agent::claude_plan::detect).await
}

/// Settings' Allow for chats on the Claude plan, and which chats use it.
#[cfg(desktop)]
#[tauri::command]
async fn claude_plan_chats(
    state: State<'_, ShellState>,
) -> Result<claude_chat::ClaudeChats, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || claude_chat::read(&dir)).await
}

/// Settings' Allow (or revoke) for chats on the Claude plan.
#[cfg(desktop)]
#[tauri::command]
async fn set_claude_plan_allowed(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<claude_chat::ClaudeChats, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || claude_chat::set_allowed(&dir, on)).await?
}

/// Posts the key's approval card; nothing is saved until `answer_ai_models_card`.
#[tauri::command]
async fn add_ai_provider(
    state: State<'_, ShellState>,
    base_url: String,
    key: String,
) -> Result<ai_models::Card, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || ai_models::start_add(&dir, &base_url, key)).await?
}

/// Opens OpenRouter's sign-in page in the browser, or hands back the link of
/// the one that waits. The key it brings back lands on a key card
/// (`wait_openrouter_sign_in`); nothing is saved yet.
#[tauri::command]
async fn start_openrouter_sign_in(
    state: State<'_, ShellState>,
) -> Result<openrouter_sign_in::SignInStarted, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || openrouter_sign_in::start(&dir, opener::open)).await?
}

/// Waits for the browser sign-in to end: the key card, or why there is none.
#[tauri::command]
async fn wait_openrouter_sign_in() -> Result<ai_models::Card, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    off_main(openrouter_sign_in::wait).await?
}

/// A browser sign-in a page that just loaded picks up: one that waits, or its
/// unanswered key card.
#[tauri::command]
async fn openrouter_sign_in_status() -> Result<openrouter_sign_in::SignInStatus, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    off_main(openrouter_sign_in::status).await
}

/// Stops a browser sign-in that waits: its port closes and no card is posted.
#[tauri::command]
async fn stop_openrouter_sign_in() -> Result<(), String> {
    off_main(openrouter_sign_in::stop).await
}

#[tauri::command]
async fn allow_ai_provider(
    state: State<'_, ShellState>,
    id: String,
) -> Result<ai_models::Card, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || ai_models::start_allow(&dir, &id)).await?
}

#[tauri::command]
async fn answer_ai_models_card(
    id: String,
    approve: bool,
) -> Result<ai_models::AiModelsView, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    off_main(move || ai_models::answer(&id, approve)).await?
}

#[tauri::command]
async fn remove_ai_provider(
    state: State<'_, ShellState>,
    id: String,
) -> Result<ai_models::AiModelsView, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || ai_models::remove_provider(&dir, &id)).await?
}

#[tauri::command]
async fn save_ai_spend(
    state: State<'_, ShellState>,
    cap: u32,
    ceiling: u32,
) -> Result<ai_models::AiModelsView, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        ai_models::save_spend(&dir, cap, ceiling)?;
        Ok::<_, String>(ai_models::view(&dir, "Saved."))
    })
    .await?
}

#[tauri::command]
async fn bench_ai_model(
    state: State<'_, ShellState>,
    target: String,
) -> Result<ai_models::AiModelsView, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || ai_models::bench_model(&dir, &target)).await?
}

#[tauri::command]
async fn unbench_ai_model(
    state: State<'_, ShellState>,
    id: String,
) -> Result<ai_models::AiModelsView, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || ai_models::unbench_model(&dir, &id)).await?
}

/// Settings → AI models → Web search: the backend and SearXNG URL.
#[tauri::command]
async fn save_web_search(
    state: State<'_, ShellState>,
    backend: String,
    searxng_url: String,
) -> Result<ai_models::AiModelsView, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || ai_models::save_web_search(&dir, &backend, &searxng_url)).await?
}

/// Settings → AI models → Web search: the Brave Search key, to the keyring.
#[tauri::command]
async fn save_brave_key(
    state: State<'_, ShellState>,
    key: String,
) -> Result<ai_models::AiModelsView, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || ai_models::save_brave_key(&dir, &key)).await?
}

#[tauri::command]
async fn revoke_ai_premium(
    state: State<'_, ShellState>,
    id: String,
) -> Result<ai_models::AiModelsView, String> {
    capabilities::needs(capabilities::current().local_ai, "AI models")?;
    let dir = state.config_dir.clone();
    off_main(move || ai_models::revoke_premium(&dir, &id)).await?
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
async fn set_local_dynamic(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<local_ai::LocalAiView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_ai::set_dynamic(&dir, on)).await?
}

/// Settings → Local AI → Use Local AI on this computer.
#[tauri::command]
async fn set_local_use(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<local_ai::LocalAiView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        local_use::set(&dir, on)?;
        Ok(local_ai::refreshed(&dir))
    })
    .await?
}

/// Settings → Local AI → Tidy typos on this computer.
#[tauri::command]
async fn set_prompt_tidy(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<local_ai::LocalAiView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        prompt_tidy::set(&dir, on)?;
        Ok(local_ai::refreshed(&dir))
    })
    .await?
}

#[tauri::command]
async fn set_local_keep_loaded(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<local_ai::LocalAiView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_ai::set_keep_loaded(&dir, on)).await
}

#[tauri::command]
async fn unload_local_model(state: State<'_, ShellState>) -> Result<local_ai::LocalAiView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || local_ai::unload_now(&dir)).await
}

#[tauri::command]
async fn get_model_library(
    state: State<'_, ShellState>,
) -> Result<model_library::LibraryView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || model_library::view(&dir)).await
}

#[tauri::command]
async fn download_models(
    app: AppHandle,
    state: State<'_, ShellState>,
    ids: Vec<String>,
) -> Result<model_library::LibraryView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || model_library::download(app, dir, ids)).await?
}

#[tauri::command]
fn pause_model_downloads() -> Result<(), String> {
    model_library::pause()
}

#[tauri::command]
async fn remove_library_model(
    state: State<'_, ShellState>,
    id: String,
) -> Result<model_library::LibraryView, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || model_library::remove(&dir, &id)).await?
}

#[tauri::command]
async fn search_hf_gguf(
    query: String,
    limit: Option<usize>,
) -> Result<Vec<model_library::HfHit>, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    off_main(move || model_library::search_hf(&query, limit.unwrap_or(20))).await?
}

#[tauri::command]
async fn list_hf_gguf_files(
    state: State<'_, ShellState>,
    repo: String,
) -> Result<Vec<model_library::HfFileRow>, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || model_library::hf_files(&dir, &repo)).await?
}

#[tauri::command]
async fn download_hf_gguf(
    state: State<'_, ShellState>,
    repo: String,
    path: String,
) -> Result<String, String> {
    capabilities::needs(capabilities::current().local_ai, "Local AI")?;
    let dir = state.config_dir.clone();
    off_main(move || model_library::download_hf(&dir, &repo, &path)).await?
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

/// The composer's screenshot offer: one look at the watched folders, off
/// the main thread. Nothing is listed while Screenshot explain is off.
#[tauri::command]
async fn get_screenshot_offer(
    state: State<'_, ShellState>,
) -> Result<screenshot_offer::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        let dirs = screenshot_offer::this_pc_dirs();
        screenshot_offer::check(&dir, &dirs, grokhub_core::now_ms())
    })
    .await
}

/// Settings → Behavior → Screenshot explain.
#[tauri::command]
async fn set_screenshot_explain(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<screenshot_offer::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || screenshot_offer::set_enabled(&dir, on, grokhub_core::now_ms())).await?
}

/// The screenshot chip's Not now.
#[tauri::command]
async fn dismiss_screenshot_offer(
    state: State<'_, ShellState>,
) -> Result<screenshot_offer::View, String> {
    let dir = state.config_dir.clone();
    off_main(move || screenshot_offer::dismiss(&dir)).await
}

/// The screenshot chip's Explain: the offered picture is read and attached
/// off the main thread. Nothing is sent.
#[tauri::command]
async fn explain_screenshot_offer(
    app: AppHandle,
    conversation_id: String,
) -> Result<attach::Outcome, String> {
    off_main(move || {
        let state = app.state::<ShellState>();
        let dir = state.config_dir.clone();
        screenshot_offer::explain(&dir, grokhub_core::now_ms(), |path| {
            stage_paths(
                &state,
                &conversation_id,
                std::iter::once(path.to_path_buf()),
            )
            .pop()
            .unwrap_or(attach::Outcome {
                staged: None,
                error: Some(screenshot_offer::GONE.into()),
            })
        })
    })
    .await?
}

/// Settings → Your PC → PC helpers: the wellbeing and focus switches.
#[tauri::command]
async fn get_pc_helpers(state: State<'_, ShellState>) -> Result<pc_helpers::PcHelpers, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_helpers::read(&dir)).await
}

/// Sentence autocomplete: a short continuation for the composer draft.
#[tauri::command]
async fn composer_ghost_suggest(
    state: State<'_, ShellState>,
    draft: String,
) -> Result<String, String> {
    let dir = state.config_dir.clone();
    off_main(move || {
        let (client, _) = model_client()?;
        let model = grokhub_agent::DEFAULT_MODEL.to_string();
        composer_ghost::suggest(&dir, &draft, &client, &model)
    })
    .await?
}

/// Turn one PC helper switch on or off (`focusHelp`, `windDown`, `rhythm`,
/// `rhythmGames`).
#[tauri::command]
async fn set_pc_helper(
    state: State<'_, ShellState>,
    key: String,
    on: bool,
) -> Result<pc_helpers::PcHelpers, String> {
    let dir = state.config_dir.clone();
    let key_for_clear = key.clone();
    let page = off_main(move || pc_helpers::set_switch(&dir, &key, on)).await?;
    #[cfg(desktop)]
    if key_for_clear == "focusHelp" && !on {
        state
            .focus_help
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
    page
}

/// Wind-down's idle time, one of the offered choices.
#[tauri::command]
async fn set_pc_auto_sleep(
    state: State<'_, ShellState>,
    minutes: u32,
) -> Result<pc_helpers::PcHelpers, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_helpers::set_auto_sleep(&dir, minutes)).await?
}

/// Forget the learned daily rhythm.
#[tauri::command]
async fn forget_pc_rhythm(state: State<'_, ShellState>) -> Result<pc_helpers::PcHelpers, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_helpers::forget_rhythm(&dir)).await?
}

/// Settings → Your PC → PC helpers, files, system and printers.
#[tauri::command]
async fn get_pc_files(state: State<'_, ShellState>) -> Result<pc_files::FilesHelpers, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_files::read(&dir)).await
}

/// Settings → Your PC → PC helpers → Home Assistant (consent, URL; token never returned).
#[tauri::command]
async fn get_home_assistant(
    state: State<'_, ShellState>,
) -> Result<home_assistant::HomeAssistantView, String> {
    let dir = state.config_dir.clone();
    off_main(move || home_assistant::view(&dir)).await
}

/// Flip consent and optionally set the Home Assistant base URL.
#[tauri::command]
async fn set_home_assistant(
    state: State<'_, ShellState>,
    enabled: bool,
    base_url: Option<String>,
) -> Result<home_assistant::HomeAssistantView, String> {
    let dir = state.config_dir.clone();
    off_main(move || home_assistant::set_config(&dir, enabled, base_url)).await?
}

/// Save or clear the Home Assistant long-lived access token (keychain).
#[tauri::command]
async fn set_home_assistant_token(
    state: State<'_, ShellState>,
    token: String,
) -> Result<home_assistant::HomeAssistantView, String> {
    let dir = state.config_dir.clone();
    off_main(move || home_assistant::set_token(&dir, token)).await?
}

/// Probe Home Assistant `/api/` with the saved URL and token.
#[tauri::command]
async fn test_home_assistant(state: State<'_, ShellState>) -> Result<String, String> {
    let dir = state.config_dir.clone();
    off_main(move || home_assistant::test_connection(&dir)).await?
}

/// Turn one of those switches on or off (`downloadsTidy`, `modelDrop`,
/// `backupNudge`, `resumeCode`, `netLog`, `printAlerts`, `printCamera`).
#[tauri::command]
async fn set_pc_files_switch(
    state: State<'_, ShellState>,
    key: String,
    on: bool,
) -> Result<pc_files::FilesHelpers, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_files::set_switch(&dir, &key, on)).await?
}

/// Save the code folders or the important folders.
#[tauri::command]
async fn set_pc_folders(
    state: State<'_, ShellState>,
    key: String,
    folders: Vec<String>,
) -> Result<pc_files::FilesHelpers, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_files::set_folders(&dir, &key, &folders)).await?
}

/// Where "Move to models" puts a model; empty means the default.
#[tauri::command]
async fn set_pc_models_folder(
    state: State<'_, ShellState>,
    folder: String,
) -> Result<pc_files::FilesHelpers, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_files::set_models_folder(&dir, &folder)).await?
}

/// Save a LAN printer after asking its address what it is.
#[cfg(desktop)]
#[tauri::command]
async fn add_pc_printer(
    state: State<'_, ShellState>,
    endpoint: String,
) -> Result<pc_files::FilesHelpers, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_files::save_printer(&dir, pc_files_run::identify_printer(&endpoint)?))
        .await?
}

#[tauri::command]
async fn remove_pc_printer(
    state: State<'_, ShellState>,
    key: String,
) -> Result<pc_files::FilesHelpers, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_files::remove_printer(&dir, &key)).await?
}

#[tauri::command]
async fn set_pc_printer_on(
    state: State<'_, ShellState>,
    key: String,
    on: bool,
) -> Result<pc_files::FilesHelpers, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_files::set_printer_enabled(&dir, &key, on)).await?
}

/// What could go from Downloads. Nothing is moved.
#[cfg(desktop)]
#[tauri::command]
async fn pc_tidy_review() -> Result<Vec<pc_files_run::TidyRow>, String> {
    off_main(pc_files_run::tidy_review).await
}

/// Move the ticked suggestions to the Trash; the page asks for approval first.
#[cfg(desktop)]
#[tauri::command]
async fn pc_tidy_trash(names: Vec<String>) -> Result<pc_files_run::TidyDone, String> {
    off_main(move || pc_files_run::tidy_trash(&names)).await?
}

/// Where the person left off in the code folders they picked.
#[cfg(desktop)]
#[tauri::command]
async fn pc_resume_rows(
    state: State<'_, ShellState>,
) -> Result<Vec<pc_files_run::ResumeRow>, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_files_run::resume_rows(&dir)).await
}

/// Time a download and log it; returns Mbit/s.
#[cfg(desktop)]
#[tauri::command]
async fn pc_speed_test(state: State<'_, ShellState>) -> Result<f64, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_files_run::speed_test(&dir)).await?
}

/// Save the internet log as a CSV; returns where it went.
#[tauri::command]
async fn pc_net_csv(state: State<'_, ShellState>) -> Result<String, String> {
    let dir = state.config_dir.clone();
    off_main(move || pc_files::export_net_csv(&dir, grokhub_core::now_ms())).await?
}

/// Devices → Start share: whether this computer's LAN hub is on, its pairing
/// code and the computers paired with it. Desktop only. Each of the four
/// can write `hub-state.json`, so none runs on the main thread.
#[cfg(desktop)]
#[tauri::command]
async fn get_share(
    share: State<'_, Arc<hub_share::HubShare>>,
) -> Result<hub_share::ShareView, String> {
    let share = Arc::clone(&share);
    off_main(move || share.view()).await
}

#[cfg(desktop)]
#[tauri::command]
async fn start_share(
    share: State<'_, Arc<hub_share::HubShare>>,
) -> Result<hub_share::ShareView, String> {
    let share = Arc::clone(&share);
    off_main(move || share.start()).await?
}

#[cfg(desktop)]
#[tauri::command]
async fn stop_share(
    share: State<'_, Arc<hub_share::HubShare>>,
) -> Result<hub_share::ShareView, String> {
    let share = Arc::clone(&share);
    off_main(move || share.stop()).await
}

/// The Fleet page's machines: this computer, read now, then the last card
/// each paired computer sent to this computer's hub. Reads `/proc`, so off
/// the main thread. Desktop only.
#[cfg(desktop)]
#[tauri::command]
async fn fleet_machines(
    share: State<'_, Arc<hub_share::HubShare>>,
) -> Result<Vec<fleet::Machine>, String> {
    let share = Arc::clone(&share);
    off_main(move || {
        let now = grokhub_core::now_ms();
        let (id, name, held) = share.fleet();
        fleet::machines(fleet::local_card(&id, &name, now), held, now)
    })
    .await
}

/// The Fleet page's subscription rows this computer can read: Claude Code's
/// local transcripts and the last Grok plan room. Reads files, so off the
/// main thread. Desktop only.
#[cfg(desktop)]
#[tauri::command]
async fn fleet_usage() -> Result<fleet::Usage, String> {
    off_main(|| fleet::usage(fleet::claude_dir().as_deref(), grokhub_core::now_ms())).await
}

/// The Fleet page's rows the user fills in (Cursor, Antigravity). Desktop only.
#[cfg(desktop)]
#[tauri::command]
async fn get_fleet_rows(state: State<'_, ShellState>) -> Result<Vec<fleet::ManualRow>, String> {
    let config_dir = state.config_dir.clone();
    off_main(move || fleet::manual_rows(&config_dir)).await
}

#[cfg(desktop)]
#[tauri::command]
async fn set_fleet_row(
    state: State<'_, ShellState>,
    row: fleet::ManualRow,
) -> Result<Vec<fleet::ManualRow>, String> {
    let config_dir = state.config_dir.clone();
    off_main(move || fleet::set_manual_row(&config_dir, row, grokhub_core::now_ms())).await?
}

/// Devices → Join another computer: the hub this computer sends its machine
/// card to, if any. Desktop only; the browser never reaches these.
#[cfg(desktop)]
#[tauri::command]
async fn get_hub_join(state: State<'_, ShellState>) -> Result<hub_join::JoinView, String> {
    let config_dir = state.config_dir.clone();
    off_main(move || hub_join::view(&config_dir)).await
}

/// Pair with another computer's hub and send this computer's card right
/// away. Dials the LAN, so off the main thread.
#[cfg(desktop)]
#[tauri::command]
async fn join_hub(
    state: State<'_, ShellState>,
    share: State<'_, Arc<hub_share::HubShare>>,
    address: String,
    code: String,
) -> Result<hub_join::JoinView, String> {
    let config_dir = state.config_dir.clone();
    let share = Arc::clone(&share);
    off_main(move || {
        let (id, name, _) = share.fleet();
        hub_join::join(
            &config_dir,
            &address,
            &code,
            (&id, &name),
            grokhub_core::now_ms(),
        )?;
        hub_join::send_now(&config_dir, &share);
        Ok(hub_join::view(&config_dir))
    })
    .await?
}

#[cfg(desktop)]
#[tauri::command]
async fn leave_hub(state: State<'_, ShellState>) -> Result<hub_join::JoinView, String> {
    let config_dir = state.config_dir.clone();
    off_main(move || hub_join::leave(&config_dir)).await?
}

/// Send on a `/sync` or `/inhabit` card. Its own command, not a slash line,
/// so typing can't stand in for the click; the browser never reaches it.
/// Reads chats and memory and writes the send log, so off the main thread.
#[cfg(desktop)]
#[tauri::command]
async fn hub_send_approve(
    state: State<'_, ShellState>,
    share: State<'_, Arc<hub_share::HubShare>>,
    send: slash::HubSend,
) -> Result<String, String> {
    let share = Arc::clone(&share);
    let config_dir = state.config_dir.clone();
    off_main(move || hub_send::approve(&config_dir, &share, send)).await?
}

/// Settings → Permissions → Paired computers: standing hub send grant.
#[cfg(desktop)]
#[tauri::command]
async fn get_hub_send_grant(state: State<'_, ShellState>) -> Result<hub_send::HubGrantRow, String> {
    let dir = state.config_dir.clone();
    off_main(move || hub_send::grant_row(&dir, "")).await
}

#[cfg(desktop)]
#[tauri::command]
async fn allow_hub_send_every_time(
    state: State<'_, ShellState>,
) -> Result<hub_send::HubGrantRow, String> {
    let dir = state.config_dir.clone();
    off_main(move || hub_send::allow_every_time(&dir)).await?
}

#[cfg(desktop)]
#[tauri::command]
async fn ask_hub_send_each_time(
    state: State<'_, ShellState>,
    share: State<'_, Arc<hub_share::HubShare>>,
) -> Result<hub_send::HubGrantRow, String> {
    let share = Arc::clone(&share);
    let dir = state.config_dir.clone();
    off_main(move || hub_send::ask_each_time(&dir, &share)).await?
}

#[cfg(desktop)]
#[tauri::command]
async fn new_share_code(
    share: State<'_, Arc<hub_share::HubShare>>,
) -> Result<hub_share::ShareView, String> {
    let share = Arc::clone(&share);
    off_main(move || share.new_code()).await
}

/// The chat's device glance: "Sharing" while this computer's hub is on, or
/// the newest screenshot, from desktop control in any chat or from a paired
/// computer. Memory only; `None` hides it. Desktop only. Copying and
/// checking frames of up to 400 KB runs off the main thread.
#[cfg(desktop)]
#[tauri::command]
async fn get_device_glance(
    state: State<'_, ShellState>,
    share: State<'_, Arc<hub_share::HubShare>>,
) -> Result<Option<hub_share::DeviceGlance>, String> {
    let chats: Vec<Arc<agent::Session>> = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .cloned()
        .collect();
    let share = Arc::clone(&share);
    off_main(move || {
        let (sharing, theirs) = share.glance();
        let frames = chats
            .iter()
            .filter_map(|chat| chat.stamped_frame())
            .chain(theirs);
        hub_share::device_glance(sharing, frames)
    })
    .await
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

/// Settings → Defaults: the work folder, how chat handles changes, and
/// whether thoughts start folded.
#[tauri::command]
fn save_defaults(
    state: State<'_, ShellState>,
    workspace: String,
    approvals: settings::Approvals,
    always_collapse_thoughts: bool,
) -> Result<SettingsView, String> {
    let next = with_defaults(
        current(&state),
        &workspace,
        approvals,
        always_collapse_thoughts,
    );
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(view(&next))
}

/// The settings with the Defaults form's three answers in place.
fn with_defaults(
    settings: ShellSettings,
    workspace: &str,
    approvals: settings::Approvals,
    always_collapse_thoughts: bool,
) -> ShellSettings {
    ShellSettings {
        workspace: workspace.trim().to_string(),
        approvals,
        always_collapse_thoughts,
        ..settings
    }
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

/// Settings → Labs → Diagnostics mode: the two switches and what a Save holds.
#[tauri::command]
fn get_diagnostics(state: State<'_, ShellState>) -> diagnostics::Page {
    diagnostics::page(&state.config_dir)
}

#[tauri::command]
fn set_diagnostics(
    state: State<'_, ShellState>,
    mode: bool,
    include_logs: bool,
) -> Result<diagnostics::Page, String> {
    capabilities::needs(capabilities::current().plugins, "Diagnostics")?;
    diagnostics::set(&state.config_dir, mode, include_logs)
}

/// Some AI source can answer a chat: a Grok sign-in or key, a provider with
/// a key the user allowed, or a model on this PC. xAI is one of these, not a
/// requirement.
pub(crate) fn ai_ready(dir: &Path) -> bool {
    credential::has_key()
        || oauth::signed_in(&oauth::Keychain).is_some()
        || grokhub_agent::route::local::installed()
        || !grokhub_agent::route::providers::usable(
            dir,
            grokhub_agent::route::providers::call_data(false),
        )
        .is_empty()
}

/// Builds the redacted bundle and keeps it for Save; nothing is written yet.
/// `chat` is the one chat whose text goes in (none: no chat text at all).
#[tauri::command]
async fn diagnostics_preview(
    state: State<'_, ShellState>,
    chat: Option<String>,
) -> Result<diagnostics::Preview, String> {
    capabilities::needs(capabilities::current().plugins, "Diagnostics")?;
    let dir = state.config_dir.clone();
    off_main(move || {
        diagnostics::preview(
            &dir,
            env!("CARGO_PKG_VERSION"),
            credential::has_key(),
            chat.as_deref(),
        )
    })
    .await?
}

/// Writes exactly the bundle the preview showed.
#[tauri::command]
fn diagnostics_save(state: State<'_, ShellState>) -> Result<diagnostics::Saved, String> {
    capabilities::needs(capabilities::current().plugins, "Diagnostics")?;
    diagnostics::save(&state.config_dir)
}

#[tauri::command]
fn open_data_folder(state: State<'_, ShellState>) -> Result<(), String> {
    privacy::open_data_folder(&state.config_dir)
}

#[tauri::command]
fn open_files_folder(state: State<'_, ShellState>) -> Result<(), String> {
    privacy::open_files_folder(&state.config_dir)
}

/// Settings → Privacy & data → Your files → Change…: the system folder
/// picker, then the page again. Nothing changes when the picker is closed.
#[tauri::command]
async fn change_files_folder(app: AppHandle) -> Result<privacy::Page, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        let picked = app
            .dialog()
            .file()
            .set_title("Where should your files go?")
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok());
        let dir = app.state::<ShellState>().config_dir.clone();
        off_main(move || {
            if let Some(folder) = picked {
                privacy::set_files_folder(&dir, &folder)?;
            }
            Ok(privacy::page(
                &dir,
                privacy::keyring(&dir),
                grokhub_core::now_ms(),
            ))
        })
        .await?
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err("Changing where your files go works on your computer.".into())
    }
}

/// `/export` and History's Export: the chat as Markdown in your Chats
/// folder. The card that follows opens the file by its token only.
#[tauri::command]
async fn export_chat(
    state: State<'_, ShellState>,
    id: String,
    clock: automations::Clock,
    format: Option<String>,
) -> Result<export::Saved, String> {
    capabilities::needs(cfg!(desktop), "Saving a chat")?;
    let format = grokhub_core::ChatExport::parse(format.as_deref().unwrap_or(""))
        .ok_or(grokhub_core::EXPORT_FORMATS_HINT)?;
    let dir = state.config_dir.clone();
    let now = grokhub_core::now_ms();
    let offset = clock.at(now).utc_offset_min();
    let path =
        off_main(move || export::export_to_chats_as(&dir, &id, now, offset, format)).await??;
    Ok(export::saved(&state.saves, path))
}

#[tauri::command]
fn open_saved(state: State<'_, ShellState>, token: String) -> Result<(), String> {
    export::open(&state.saves, &token)
}

#[tauri::command]
fn reveal_saved(state: State<'_, ShellState>, token: String) -> Result<(), String> {
    export::reveal(&state.saves, &token)
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

/// The settings a paired browser's chat or run goes with: the window's, with
/// desktop control off. Web chats and the runs a browser starts never get
/// the desktop tools, even when the window has them on.
fn web_settings(settings: ShellSettings) -> ShellSettings {
    ShellSettings {
        desktop_control: false,
        ..settings
    }
}

/// Whether a paired browser may take chat `id`: one it holds already, or one
/// the window doesn't, which is neither open here (`open_here`) nor started
/// in the window (`web_chats`), open or closed.
fn web_may_take(
    web: &remote::WebAccess,
    config_dir: &Path,
    open_here: impl Fn(&str) -> bool,
    id: &str,
) -> Result<(), String> {
    if web.owns(id) {
        return Ok(());
    }
    if open_here(id) || web_chats::the_windows(config_dir, id) {
        return Err(remote::NOT_YOURS.into());
    }
    Ok(())
}

/// The browser takes chat `id`, kept on file as a browser's so it stays the
/// web's once this pairing ends. A job the chat's agent writes from now on
/// is the browser's too, so it never runs with the desktop.
fn web_take(web: &remote::WebAccess, config_dir: &Path, id: &str) -> Result<(), String> {
    if !web.adopt(id) {
        return Err(remote::NOT_YOURS.into());
    }
    grokhub_agent::web_jobs::set_web_chat(id);
    web_chats::note(config_dir, id)
}

/// Reflect from a browser with no chat named: the window's last chat is
/// not the browser's to read.
const WEB_REFLECT_NEEDS_CHAT: &str =
    "A browser reflects only its own chats. Reflect the window's chats on your computer.";

/// The chat a browser's Reflect reads: one it holds or a browser started,
/// never one the window started, open or closed, and never the window's
/// last chat for want of an id. Reflect hands back the chat's lines.
fn web_reflect_chat(
    web: &remote::WebAccess,
    config_dir: &Path,
    id: Option<String>,
) -> Result<String, String> {
    let id = id
        .filter(|id| !id.is_empty())
        .ok_or(WEB_REFLECT_NEEDS_CHAT)?;
    if web.owns(&id) || web_chats::started_on_web(config_dir, &id) {
        Ok(id)
    } else {
        Err(remote::NOT_YOURS.into())
    }
}

/// A browser's send, read in full before it takes the chat, so a send that
/// is refused keeps no chat id and starts no worker. `open_here` says
/// whether a chat with that id is open on this computer.
fn web_send(
    web: &remote::WebAccess,
    config_dir: &Path,
    open_here: impl Fn(&str) -> bool,
    args: &Value,
) -> Result<(String, Turn), String> {
    let conversation_id: String = arg(args, "conversationId")?;
    web_may_take(web, config_dir, open_here, &conversation_id)?;
    let turn = Turn {
        text: arg(args, "text")?,
        mode: arg::<Option<agent::SessionMode>>(args, "mode")?,
        attachments: arg::<Option<Vec<String>>>(args, "attachments")?.unwrap_or_default(),
        clock: arg::<Option<automations::Clock>>(args, "clock")?,
    };
    // The same refusal as `agent::Session::prompt`, before a worker starts.
    if turn.text.trim().is_empty() && turn.attachments.is_empty() {
        return Err("Type a message first.".into());
    }
    web_take(web, config_dir, &conversation_id)?;
    Ok((conversation_id, turn))
}

/// The chat a browser may steer or answer: one it started, and not while the
/// computer runs a desktop turn or a supervised desktop session in it.
fn web_drive(
    web: &remote::WebAccess,
    on_desktop: impl Fn(&str) -> bool,
    args: &Value,
) -> Result<String, String> {
    let id: String = arg(args, "conversationId")?;
    if !web.owns(&id) || on_desktop(&id) {
        return Err(remote::NOT_YOURS.into());
    }
    Ok(id)
}

/// What a paired browser may answer from the decision inbox: its own chats'
/// asks. A park (a hard card, such as a key or a command waiting on the
/// computer) is the desktop's alone, so a browser that names one by id is
/// refused, as the inbox list already leaves parks out.
fn web_inbox_kind(kind: &str) -> Result<(), String> {
    if kind == "park" {
        return Err(remote::refused());
    }
    Ok(())
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
    // A chat the window has open now.
    let open_here = |id: &str| {
        let state = app.state::<ShellState>();
        let chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
        chats.contains_key(id)
    };
    // Steering a reply or answering its asks: not while the computer drives
    // the desktop in that chat.
    let drive = |args: &Value| -> Result<String, String> {
        let on_desktop = |id: &str| {
            let state = app.state::<ShellState>();
            let chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
            chats.get(id).is_some_and(|session| session.on_desktop())
        };
        web_drive(&web, on_desktop, args)
    };
    match command {
        "get_settings" => out(view(&current(&state))),
        "get_profile" => out(profile_now(&state.config_dir)),
        "send_chat" => {
            let (conversation_id, turn) = web_send(&web, &state.config_dir, open_here, args)?;
            let settings = web_settings(current(&state));
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
            web_may_take(&web, &state.config_dir, open_here, &conversation_id)?;
            let config_dir = state.config_dir.clone();
            let staged = stage_attachment(
                state,
                conversation_id.clone(),
                arg(args, "name")?,
                arg(args, "data")?,
            )?;
            web_take(&web, &config_dir, &conversation_id)?;
            out(staged)
        }
        "remove_attachment" => {
            remove_attachment(state, own(args)?, arg(args, "path")?);
            Ok(Value::Null)
        }
        "answer_elicit" => {
            answer_elicit(
                state,
                drive(args)?,
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
                drive(args)?,
                arg(args, "callId")?,
                arg(args, "answer")?,
            )?;
            Ok(Value::Null)
        }
        "cancel_chat" => {
            cancel_chat(state, own(args)?);
            Ok(Value::Null)
        }
        "steer_chat" => out(steer_chat(state, drive(args)?, arg(args, "text")?)),
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
        // A job a browser writes never gets the desktop tools.
        "add_automation" => {
            let clock = now_clock(arg(args, "clock")?);
            let text: String = arg(args, "text")?;
            let by = automations::Author::Web;
            let list = automations::add_by(&state.config_dir, &text, clock, by)?;
            out(automations::rows(&list, clock))
        }
        "list_suggested_automations" => out(list_suggested_automations(state)),
        "list_automation_blueprints" => out(list_automation_blueprints()),
        "add_suggested_automation" => {
            let clock = now_clock(arg(args, "clock")?);
            let seed: String = arg(args, "seed")?;
            let by = automations::Author::Web;
            let list = suggested::add_suggested_by(&state.config_dir, &seed, clock, by)?;
            out(automations::rows(&list, clock))
        }
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
        "edit_automation" => {
            let clock = now_clock(arg(args, "clock")?);
            let (id, text): (String, String) = (arg(args, "id")?, arg(args, "text")?);
            let by = automations::Author::Web;
            let list = automations::edit_by(&state.config_dir, &id, &text, clock, by)?;
            out(automations::rows(&list, clock))
        }
        "run_automation" => {
            let (id, settings) = (arg(args, "id")?, web_settings(current(&state)));
            out(block_on(run_automation_now(state, id, settings))?)
        }
        "get_memory" => out(get_memory(state)?),
        "save_memory" => out(save_memory(
            state,
            arg(args, "global")?,
            arg(args, "text")?,
            arg(args, "base")?,
        )?),
        "add_memory" => out(add_memory(state, arg(args, "text")?, arg(args, "global")?)?),
        "slash_hits" => out(slash::hits(&arg::<String>(args, "draft")?, true)),
        "run_slash" => {
            let id: Option<String> = arg(args, "conversationId")?;
            let owned = |id: &str| web.owns(id);
            out(run_slash_line(
                &state,
                &arg::<String>(args, "line")?,
                true,
                id.as_deref(),
                Some(&owned as &dyn Fn(&str) -> bool),
                None,
            )?)
        }
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
        "pause_memory_row" => out(pause_memory_row(
            state,
            arg(args, "id")?,
            arg(args, "paused")?,
        )?),
        "add_personal_fact" => out(add_personal_fact(
            state,
            arg(args, "text")?,
            arg(args, "group")?,
        )?),
        "get_memory_files" => out(get_memory_files()?),
        "save_memory_file" => out(save_memory_file(
            arg(args, "name")?,
            arg(args, "text")?,
            arg(args, "base")?,
        )?),
        "restore_memory_file" => out(restore_memory_file(arg(args, "name")?)?),
        "reflect_memory" => {
            let id = web_reflect_chat(&web, &state.config_dir, arg(args, "conversationId")?)?;
            out(reflect_memory(state, Some(id))?)
        }
        "tidy_memory_ledger" => out(tidy_memory_ledger()?),
        "apply_memory_raw_tidy" => out(apply_memory_raw_tidy()?),
        "dismiss_memory_raw_tidy" => out(dismiss_memory_raw_tidy()?),
        "get_memory_retention" => out(get_memory_retention()),
        "set_memory_retention" => out(set_memory_retention(arg(args, "days")?)?),
        "get_chat_archive" => out(get_chat_archive()),
        "set_chat_archive" => out(set_chat_archive(arg(args, "days")?)?),
        "list_connectors" => out(block_on(list_connectors(state))?),
        "check_connectors" => out(block_on(check_connectors(state))?),
        "restart_connector" => out(block_on(restart_connector(state, arg(args, "name")?))?),
        "sign_out_connector" => out(block_on(sign_out_connector(state, arg(args, "name")?))?),
        "disconnect_connector" => out(block_on(disconnect_connector(state, arg(args, "name")?))?),
        "get_pulse" => out(block_on(get_pulse(state, arg(args, "day")?))?),
        "pulse_act" => {
            let act: pulse::Act = arg(args, "act")?;
            if !remote::web_may_pulse(&act) {
                return Err(remote::refused());
            }
            out(block_on(pulse_act(
                state,
                arg(args, "id")?,
                act,
                arg(args, "day")?,
            ))?)
        }
        "pulse_discuss" => out(block_on(pulse_discuss(state, arg(args, "id")?))?),
        "pulse_offer" => out(block_on(pulse_offer(
            state,
            arg(args, "id")?,
            arg(args, "index")?,
        ))?),
        "pulse_live_open" => out(block_on(pulse_live_open(
            state,
            arg(args, "id")?,
            arg(args, "index")?,
        ))?),
        "pulse_live_current" => out(block_on(pulse_live_current())?),
        "pulse_live_allow" => out(block_on(pulse_live_allow(state))?),
        "pulse_live_deny" => out(block_on(pulse_live_deny(state))?),
        "pulse_live_stop" => out(block_on(pulse_live_stop(state))?),
        "pulse_live_undo" => out(block_on(pulse_live_undo(state))?),
        "pulse_live_close" => out(block_on(pulse_live_close())?),
        "pulse_remind" => out(block_on(pulse_remind(
            state,
            arg(args, "id")?,
            arg(args, "day")?,
        ))?),
        "pulse_why" => out(block_on(pulse_why(state, arg(args, "id")?))?),
        "pulse_saved" => out(block_on(pulse_saved(state))?),
        "pulse_unsave" => out(block_on(pulse_unsave(state, arg(args, "url")?))?),
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
        // A browser sees only the pictures of the chats it started.
        "chat_picture" => {
            let path: String = arg(args, "path")?;
            out(imagine::chat_picture(&state.config_dir, &path, &|chat| {
                chat.is_some_and(|id| web.owns(id))
            })?)
        }
        "get_skills" => out(block_on(get_skills(state))?),
        "nyx_look" => out(nyx_look(
            state,
            arg(args, "voice")?,
            arg(args, "mood")?,
            arg(args, "tool")?,
        )),
        "get_home" | "get_mission" => {
            let args = home_args(
                &state,
                arg(args, "clock")?,
                arg(args, "conversationId")?,
                Some(Arc::clone(&web)),
            );
            if command == "get_home" {
                out(block_on(home_read(args, home::page))?)
            } else {
                out(block_on(home_read(args, |src| {
                    home::mission_for(src, src.clock.now_ms)
                }))?)
            }
        }
        "get_goals" => out(block_on(get_goals(state, arg(args, "clock")?))?),
        "goal_act" => out(block_on(goal_act(
            state,
            arg(args, "clock")?,
            arg(args, "acts")?,
        ))?),
        "home_catalog" => out(block_on(home_catalog(state))?),
        "set_home_layout" => out(block_on(set_home_layout(state, arg(args, "pick")?))?),
        // A browser's board shows only the chats a browser started.
        "get_sessions_board" => out(block_on(sessions_board_get(
            &state,
            true,
            arg(args, "filter")?,
            arg(args, "query")?,
        ))?),
        "sessions_board_stamp" => out(block_on(sessions_board_stamp(state))?),
        "sessions_board_pin" => out(block_on(sessions_board_pin_for(
            &state,
            true,
            arg(args, "id")?,
            arg(args, "columnId")?,
            arg(args, "filter")?,
            arg(args, "query")?,
        ))?),
        "sessions_board_reshape" => out(block_on(sessions_board_reshape_for(
            &state,
            true,
            arg(args, "instruction")?,
            arg(args, "filter")?,
            arg(args, "query")?,
        ))?),
        "get_workboard" => out(block_on(get_workboard(state))?),
        "workboard_add" => out(block_on(workboard_add(
            state,
            arg(args, "title")?,
            arg(args, "detail")?,
        ))?),
        "workboard_act" => {
            let act: workboard::Act = arg(args, "act")?;
            if !remote::web_may_act(&act) {
                return Err(remote::refused());
            }
            out(block_on(workboard_act(state, arg(args, "id")?, act))?)
        }
        "workboard_work" => out(block_on(workboard_work(state, arg(args, "id")?))?),
        "workboard_recur" => {
            let (dir, clock) = (state.config_dir.clone(), now_clock(arg(args, "clock")?));
            let (id, choice): (String, usize) = (arg(args, "id")?, arg(args, "choice")?);
            let by = automations::Author::Web;
            out(block_on(off_main(move || {
                workboard::make_recurring(&dir, &id, choice, clock, by)
            }))??)
        }
        "workboard_stamp" => out(block_on(workboard_stamp(state))?),
        "workboard_log" => out(block_on(workboard_log(state, arg(args, "id")?))?),
        "workboard_delete_archived" => out(block_on(workboard_delete_archived(
            state,
            arg(args, "days")?,
        ))?),
        "workboard_sweep_undo" => out(block_on(workboard_sweep_undo(state))?),
        "workboard_sweep_dismiss" => out(block_on(workboard_sweep_dismiss(state))?),
        "quick_chips" => {
            // A browser sees chips only for the chats it started.
            let id: Option<String> = arg(args, "conversationId")?;
            if id.as_deref().is_some_and(|id| !web.owns(id)) {
                return Err(remote::NOT_YOURS.into());
            }
            out(chip_row(
                &state,
                &arg::<String>(args, "surface")?,
                id,
                &arg::<Option<String>>(args, "draft")?.unwrap_or_default(),
                arg::<Option<agent::SessionMode>>(args, "mode")?.unwrap_or_default(),
                arg(args, "clock")?,
                true,
            ))
        }
        "chip_outcome" => out(block_on(chip_outcome_from(
            app.clone(),
            arg(args, "surface")?,
            arg(args, "id")?,
            arg(args, "picked")?,
            arg(args, "clock")?,
            true,
        ))?),
        "get_behavior" => out(get_behavior(state)),
        "get_privacy" => out(block_on(get_privacy(state))?),
        "get_background" => out(get_background(state)?),
        // A browser sees inbox rows only for the chats it started; parks
        // belong to the desktop and never cross.
        "decision_inbox" => {
            let rows: Vec<inbox::Decision> = inbox::decisions(&state.config_dir)
                .into_iter()
                .filter(|row| !row.chat_id.is_empty() && web.owns(&row.chat_id))
                .collect();
            out(rows)
        }
        "answer_inbox" => {
            let kind: String = arg(args, "kind")?;
            web_inbox_kind(&kind)?;
            out(answer_inbox(
                app.clone(),
                state,
                drive(args)?,
                arg(args, "id")?,
                kind,
                arg(args, "approve")?,
            )?)
        }
        "reply_pick" => {
            reply_pick(state, arg(args, "key")?);
            Ok(Value::Null)
        }
        _ => Err(remote::refused()),
    }
}

/// A scheduled run's route, with the settings as they are at each run.
#[cfg(desktop)]
fn scheduled_route(config_dir: &Path) -> Result<scheduler::Route, String> {
    let settings = settings::load(config_dir);
    let route = route(&settings, agent::SessionMode::Chat, config_dir)?;
    let (model, _) = grokhub_agent::route::host::chat_start();
    let gate = run_gate(&route);
    Ok((route.client, model, route.workspace, gate))
}

/// Scheduled automations fire from here while the cabin is closed.
#[cfg(desktop)]
fn start_scheduler(app: &tauri::App) {
    let config_dir = app.state::<ShellState>().config_dir.clone();
    let handle = app.handle().clone();
    let dir = config_dir.clone();
    scheduler::start(
        config_dir,
        move || scheduled_route(&dir),
        move |fired| {
            let _ = handle.emit(scheduler::RAN_EVENT, fired);
        },
    );
}

/// Messages from paired chats and signed webhooks are answered from here:
/// one background run each (read-only, no desktop, nobody to approve a
/// card), and a chat's answer goes back to it. Waiting decisions go out to
/// the senders the user chose, and their "yes CODE" / "no CODE" replies are
/// answered through `answer_inbox`, like the inbox row. A webhook a saved
/// routine listens for runs that routine instead ([`routine_runs`]).
#[cfg(desktop)]
fn start_channel_inbox(app: AppHandle) {
    let config_dir = app.state::<ShellState>().config_dir.clone();
    channel_inbox::recover(&config_dir, grokhub_core::now_ms());
    std::thread::spawn(move || {
        let mut asks = channel_asks::RemoteAsks::default();
        loop {
            std::thread::sleep(Duration::from_secs(3));
            let dir = config_dir.as_path();
            let store = grokhub_agent::channels_net::os_keychain();
            let waiting = inbox::decisions(dir);
            let texts = asks.sync(&waiting, &mut grokhub_core::fill_random);
            if !texts.is_empty() {
                for to in channel_asks::targets(dir) {
                    for text in &texts {
                        let _ = channels::send_answer(dir, &store, &to, text);
                    }
                }
            }
            channel_inbox::drain_with(
                dir,
                &mut |to, text| {
                    channel_asks::remember_reach(dir, to);
                    if !channel_asks::is_approver(dir, to.channel, &to.sender) {
                        return None;
                    }
                    Some(match asks.reply(text)? {
                        channel_asks::Reply::Said(said) => said,
                        channel_asks::Reply::Answer { ask, approve } => {
                            match answer_inbox(
                                app.clone(),
                                app.state::<ShellState>(),
                                ask.chat_id,
                                ask.id,
                                ask.kind,
                                approve,
                            ) {
                                Ok(_) if approve => format!("Allowed once: {}", ask.what),
                                Ok(_) => format!("Denied: {}", ask.what),
                                Err(why) => why,
                            }
                        }
                    })
                },
                // A webhook a routine listens for runs that routine instead.
                &mut |hook, body| {
                    let fired = routine_runs::webhook(dir, hook, body, &|| scheduled_route(dir))?;
                    let _ = app.emit(scheduler::RAN_EVENT, &fired);
                    Some(if fired.result.failed() {
                        Err(fired.result.error)
                    } else {
                        Ok(fired.result.text)
                    })
                },
                &mut |prompt, session| {
                    let (client, model, workspace, _) = scheduled_route(dir)?;
                    let _config = grokhub_agent::perm::ConfigGuard::set(dir);
                    let done = unattended::run(unattended::Job {
                        client,
                        model,
                        workspace,
                        session_id: session.to_string(),
                        prompt: prompt.to_string(),
                        // Fixed, so text in a message can't name the tool set.
                        tools_from: Some(String::new()),
                        gate: unattended::RunGate::BACKGROUND,
                        cancel: grokhub_agent::CancelToken::new(),
                        resume: false,
                    });
                    if done.error.is_empty() {
                        Ok(done.text)
                    } else {
                        Err(done.error)
                    }
                },
                &mut |to, text| channels::send_answer(dir, &store, to, text),
            );
        }
    });
}

/// The flag that runs scheduled automations with no window.
pub const AUTOMATIONS_ARG: &str = "--automations";

/// `grokhub-shell --automations`: scheduled automations with no window, for
/// a login service while the app is closed. Sets up what a scheduled run
/// reads (the router's settings, provider keys and the learned-tier key from
/// the OS keyring, the device profile), then ticks until stopped. An open
/// app takes the jobs over, and this waits for it to close.
///
/// Each fire still settles on the row in `automations.json` (health, count,
/// check gate). The `eprintln!` below is a console hint only: on Windows
/// release builds (`windows_subsystem = "windows"`) it goes nowhere.
#[cfg(desktop)]
pub fn run_automations() -> ! {
    let config_dir = grokhub_agent::perm::config_dir();
    grokhub_agent::route::host::sync(&config_dir);
    grokhub_agent::route::providers::use_os_vault();
    grokhub_agent::harness::use_os_keyring();
    grokhub_agent::device::start(
        grokhub_core::paths::user_home().unwrap_or_default(),
        config_dir.clone(),
    );
    grokhub_core::learn::hub::start(&config_dir);
    let dir = config_dir.clone();
    scheduler::run_headless(
        &config_dir,
        move || scheduled_route(&dir),
        |fired| {
            let how = if fired.result.failed() {
                format!("failed: {}", fired.result.error)
            } else {
                "ok".to_string()
            };
            // Console only; Windows release has no console. Row results still settle.
            eprintln!("automations: {} {how}", fired.name);
        },
    )
}

/// Automations page: whether the login service keeps jobs running with the
/// app closed.
#[cfg(desktop)]
#[tauri::command]
fn get_automations_service() -> Result<bool, String> {
    headless_service::installed()
}

/// Automations page switch: install or remove the login service.
#[cfg(desktop)]
#[tauri::command]
fn set_automations_service(on: bool) -> Result<bool, String> {
    if on {
        headless_service::install()?;
    } else {
        headless_service::remove()?;
    }
    headless_service::installed()
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
    let t = desktop_nyx::tray_item(&current(&app.state::<ShellState>()));
    let nyx = CheckMenuItem::with_id(
        app,
        desktop_nyx::TRAY_ID,
        t.text,
        t.enabled,
        t.checked,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    // Always on; harmless when nothing runs.
    let halt = MenuItem::with_id(app, "halt", "Halt", true, None::<&str>)?;
    let local = MenuItem::with_id(app, "local", local_ai::TRAY_CHECKING, false, None::<&str>)?;
    let menu = Menu::with_items(app, &[&local, &halt, &show, &nyx, &quit])?;
    local_ai::start_tray(local, app.state::<ShellState>().config_dir.clone());
    desktop_nyx::start_tray(nyx);
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
            id if id == desktop_nyx::TRAY_ID => desktop_nyx_from_tray(app),
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
    // On Linux and Windows the top bar is the title bar: it drags the window
    // and holds the window buttons (capabilities/main-window.json allows them).
    #[cfg(desktop)]
    let builder = builder.decorations(!capabilities::current().window_controls);
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

/// The desktop tools' host: the shell's config folder and its switch. Cua
/// follows the `cuaDriver` flag the cabin saved (Linux and macOS).
#[cfg(desktop)]
fn install_desktop_host() {
    // Pin the start time now, as the cabin does: a Halt after it keeps desktop
    // steps off until the next start, even if none ran before the Halt.
    grokhub_desktop::process_started_ms();
    grokhub_desktop::host::install(grokhub_desktop::host::Host {
        config_dir: grokhub_agent::perm::config_dir,
        settings: || {
            let dir = grokhub_agent::perm::config_dir();
            let saved = grokhub_desktop::host::saved_settings(&dir);
            grokhub_desktop::host::Settings {
                desktop_control: settings::load(&dir).desktop_control
                    && capabilities::current().desktop_control,
                // The shell has no Cua switch; it reads the cabin's saved one.
                cua_driver: saved.cua_driver,
                cua_driver_path: saved.cua_driver_path,
            }
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
    // SAFETY: first thing in setup, before any of the app's own threads
    // start or anything reads HOME / GROKHUB_CONFIG.
    unsafe {
        std::env::set_var("HOME", &data);
        std::env::set_var("GROKHUB_CONFIG", data.join("config"));
    }
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
            // One app per profile: the egui cabin may hold this config dir.
            #[cfg(desktop)]
            if !app_guard::take_profile(app.handle(), &config_dir) {
                return Ok(());
            }
            // A self-update that keeps ending early is put back to the old
            // version, which then runs instead of this one.
            #[cfg(desktop)]
            {
                let running = shell_update::version();
                if let Some(old) = update_rollback::on_launch(&config_dir, &running) {
                    let mut cmd = std::process::Command::new(&old);
                    cmd.args(std::env::args_os().skip(1));
                    let _ = cmd.spawn();
                    std::process::exit(0);
                }
                let dir = config_dir.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(update_rollback::HEALTHY_AFTER);
                    update_rollback::mark_healthy(&dir, &running);
                });
            }
            // Before anything reads the local model catalog.
            catalog_refresh::tidy(&config_dir);
            let loaded = settings::load(&config_dir);
            #[cfg(desktop)]
            {
                install_desktop_host();
                episode::start_askpass(episode::hub_exe());
                if loaded.desktop_control && capabilities::current().desktop_control {
                    grokhub_desktop::set_desktop_enabled(true);
                }
                // The Bot Desktop comes back with the app; a failed start
                // shows in Settings when the person looks.
                if loaded.bot_desktop {
                    let dir = config_dir.clone();
                    std::thread::spawn(move || {
                        let _ = bot_screen::apply(&dir, true);
                    });
                }
                // Connected Telegram and Discord bots listen again.
                {
                    let dir = config_dir.clone();
                    std::thread::spawn(move || {
                        channels::start_saved(&dir, &grokhub_agent::channels_net::os_keychain());
                    });
                }
                episode::sync_cua(
                    loaded.desktop_control && capabilities::current().desktop_control,
                    episode::cua_flag(&config_dir),
                    episode::hub_exe(),
                );
            }
            // The router takes Local AI's switch, OpenRouter's policy and the
            // spend limits from the cabin's settings from the start.
            grokhub_agent::route::host::sync(&config_dir);
            // Provider keys are read from the OS keyring from the first chat,
            // not only once Settings → AI models has been opened.
            grokhub_agent::route::providers::use_os_vault();
            // Android: install the OS Keystore store before the first read
            // (keyring's v1 facade has no Android backend).
            #[cfg(target_os = "android")]
            if let Err(e) = grokhub_agent::keyring_blob::ensure_android_store() {
                eprintln!("android keyring: {e}");
            }
            // The learned-tier key (sealed memory, scope grants and what the
            // indexers learn) lives in the OS keyring too, as in the cabin.
            grokhub_agent::harness::use_os_keyring();
            // The full device profile, shared with the cabin: sizes local
            // model launches and sets the router's battery and low-end tier.
            #[cfg(desktop)]
            grokhub_agent::device::start(
                grokhub_core::paths::user_home().unwrap_or_default(),
                config_dir.clone(),
            );
            #[cfg(desktop)]
            local_governor::start(config_dir.clone());
            pulse_feed::start(config_dir.clone(), || {
                let client: Box<dyn grokhub_agent::ModelClient> = Box::new(model_client()?.0);
                Ok((client, grokhub_agent::route::host::chat_start().0))
            });
            // The Sunday self-review, while the cabin is closed.
            #[cfg(desktop)]
            self_review::start(config_dir.clone(), || {
                let client: Box<dyn grokhub_agent::ModelClient> = Box::new(model_client()?.0);
                Ok(client)
            });
            steam_checks::start(config_dir.clone());
            catalog_refresh::start(config_dir.clone());
            router_upkeep::start(config_dir.clone());
            vstack_sync::start(config_dir.clone());
            local_model_checks::start(config_dir.clone());
            pc_health::start(config_dir.clone());
            // The learning engine: chips, cards and runs record to it from
            // here on. Memory only, with one small write when one is due.
            grokhub_core::learn::hub::start(&config_dir);
            #[cfg(desktop)]
            os_extras::setup(app.handle(), &loaded);
            let text_size = loaded.text_size;
            let code_sessions = code_sessions::Folders::open(config_dir.clone());
            app.manage(ShellState {
                config_dir,
                settings: Mutex::new(loaded),
                chats: Mutex::new(HashMap::new()),
                global_halt: std::sync::atomic::AtomicBool::new(false),
                trays: attach::Trays::default(),
                chip_rows: Mutex::new(chips::Rows::default()),
                sign_in: Arc::default(),
                saves: export::Saves::default(),
                turn_reads: turns::Reads::default(),
                #[cfg(desktop)]
                notified: os_extras::Pending::default(),
                auto: Mutex::default(),
                card_carry: Mutex::default(),
                code_sessions,
                #[cfg(desktop)]
                focus_help: Mutex::new(focus_help::FocusHelp::default()),
                game_session: Mutex::default(),
            });
            if let Some(window) = app.get_webview_window("main") {
                if text_size != 100 {
                    zoom(&window, text_size);
                }
            }
            #[cfg(desktop)]
            {
                let global = register_halt_keys(app.handle());
                app.state::<ShellState>()
                    .global_halt
                    .store(global, std::sync::atomic::Ordering::SeqCst);
            }
            #[cfg(desktop)]
            app.manage(Arc::new(hub_share::HubShare::open(
                &app.state::<ShellState>().config_dir,
            )));
            #[cfg(desktop)]
            hub_join::start(
                app.state::<ShellState>().config_dir.clone(),
                Arc::clone(&app.state::<Arc<hub_share::HubShare>>()),
            );
            let web = Arc::new(remote::WebAccess::default());
            app.manage(Arc::clone(&web));
            // Chat events reach paired browsers too; each page keeps its own
            // conversation's.
            app.listen_any(CHAT_EVENT, move |event| {
                web.broadcast(CHAT_EVENT, event.payload())
            });
            // The chat's mod tools, on the Mods page's own client. Setting up
            // a mod puts an Install card on the Mods page and nothing more.
            #[cfg(desktop)]
            if capabilities::current().mods {
                let asked = app.handle().clone();
                let told = app.handle().clone();
                mods_agent::register(mods_agent::Host::new(
                    app.state::<ShellState>().config_dir.clone(),
                    Box::new(move || {
                        asked
                            .try_state::<ShellState>()
                            .is_some_and(|s| current(&s).adults_on())
                    }),
                    Box::new(move || {
                        let _ = told.emit(mods_agent::REQUESTED_EVENT, ());
                    }),
                ));
            }
            // Home Assistant tools when the user has consented and configured them.
            home_assistant::register(app.state::<ShellState>().config_dir.clone());
            #[cfg(desktop)]
            start_scheduler(app);
            start_channel_inbox(app.handle().clone());
            // Questions from helpers that outlive their chat's reply.
            // Only this shell watches task changes; the cabin leaves it off.
            grokhub_agent::set_task_watch(true);
            helper_asks::start(app.handle().clone());
            // The internet log, when it's on and the cabin isn't running.
            #[cfg(desktop)]
            net_watch::start(app.state::<ShellState>().config_dir.clone());
            // Game launch cards, the ping and the note offer after a game.
            game_companion::start(app.handle().clone());
            // The wellbeing helpers' minute tick, while the cabin is closed.
            #[cfg(desktop)]
            helper_loop::start(
                app.state::<ShellState>().config_dir.clone(),
                app.handle().clone(),
            );
            // Downloads tidy, model drop, backup reminders and printer alerts.
            #[cfg(desktop)]
            pc_files_run::start(app.state::<ShellState>().config_dir.clone());
            // The Workboard's local tidy, while the cabin is closed. No model.
            #[cfg(desktop)]
            workboard::start_sweeper(app.state::<ShellState>().config_dir.clone());
            // Queued Workboard cards start on their own, while the cabin is closed.
            #[cfg(desktop)]
            start_card_queue(app.handle().clone());
            // The agent's own interrupted runs resume from their checkpoints.
            #[cfg(desktop)]
            start_auto_resume(app.handle().clone());
            // The proactive engine and nudge watchers, while the cabin is closed.
            #[cfg(desktop)]
            {
                let pinged = app.handle().clone();
                let watched = app.handle().clone();
                let proactive_dir = app.state::<ShellState>().config_dir.clone();
                proactive::start(
                    app.state::<ShellState>().config_dir.clone(),
                    move |title: &str| os_extras::proactive_reminder(&pinged, title),
                    // A running chat or a waiting decision holds Done for you.
                    move || background_busy(&watched),
                    move || chat_client(&proactive_dir).is_ok(),
                );
            }
            // The local indexers read the scopes allowed in Permissions.
            #[cfg(desktop)]
            {
                let watched = app.handle().clone();
                let told = app.handle().clone();
                indexers::start(
                    app.state::<ShellState>().config_dir.clone(),
                    // A fresh game reading: this thread can wait for it.
                    move || {
                        background_busy(&watched) || grokhub_desktop::game_guard::current().gaming
                    },
                    move || {
                        let _ = told.emit(scopes::SCOPES_EVENT, ());
                    },
                );
            }
            // The nightly lesson tidy and memory dream, while the cabin is closed.
            #[cfg(desktop)]
            {
                let watched = app.handle().clone();
                let config_dir = app.state::<ShellState>().config_dir.clone();
                let batch_dir = config_dir.clone();
                night::start(
                    config_dir,
                    move || background_busy(&watched),
                    move || {
                        // The dream can wait: on an API key it goes through the Batch API.
                        let client: Box<dyn grokhub_agent::ModelClient> =
                            Box::new(model_client()?.0.deferrable(&batch_dir));
                        Ok(client)
                    },
                );
            }
            // The living wall's covers, while the cabin is closed.
            #[cfg(desktop)]
            {
                let watched = app.handle().clone();
                let told = app.handle().clone();
                living_wall::start(
                    app.state::<ShellState>().config_dir.clone(),
                    move || {
                        background_busy(&watched) || grokhub_desktop::game_guard::current().gaming
                    },
                    move |news| {
                        let _ = told.emit(living_wall::EVENT, news);
                    },
                );
            }
            start_budget_tally(app.handle().clone());
            // A no-route pause re-checks on a backoff and resumes its chat.
            let told = app.handle().clone();
            model_wait::start_watch(
                app.state::<ShellState>().config_dir.clone(),
                move |resume| {
                    let _ = told.emit(model_wait::MODEL_BACK_EVENT, resume);
                },
            );
            build_main_window(app)?;
            #[cfg(desktop)]
            {
                let settings = current(&app.state::<ShellState>());
                let _ = desktop_nyx::show_window(app.handle(), &settings);
            }
            #[cfg(desktop)]
            build_tray(app)?;
            #[cfg(desktop)]
            desktop_nyx_watch(app.handle().clone());
            #[cfg(desktop)]
            watch_for_updates(app.handle().clone(), grokhub_agent::perm::config_dir());
            Ok(())
        })
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            if let WindowEvent::Focused(focused) = event {
                if window.label() == "main" {
                    let state = window.state::<ShellState>();
                    let on = pc_helpers::read(&state.config_dir).focus_help;
                    state
                        .focus_help
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .tick(*focused, on);
                }
            }
            #[cfg(desktop)]
            if let WindowEvent::Moved(at) = event {
                if window.label() == desktop_nyx::LABEL {
                    desktop_nyx::note_moved(window, *at);
                }
            }
            // The chats go with the main window: her desk settles.
            #[cfg(desktop)]
            if let WindowEvent::Destroyed = event {
                if window.label() == "main" && desktop_nyx::set_life(desktop_nyx::Life::default()) {
                    desktop_nyx::publish(
                        window.app_handle(),
                        &current(&window.state::<ShellState>()),
                    );
                }
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                #[cfg(desktop)]
                if window.label() == desktop_nyx::LABEL {
                    let state = window.state::<ShellState>();
                    let next = desktop_nyx::apply_patch(
                        current(&state),
                        desktop_nyx::Patch {
                            undocked: Some(false),
                            ..desktop_nyx::Patch::default()
                        },
                    );
                    let _ = settings::save(&state.config_dir, &next);
                    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
                    // Alt+F4 on her: the tray and every page hear it too.
                    desktop_nyx::publish(window.app_handle(), &next);
                    return;
                }
                let state = window.state::<ShellState>();
                if current(&state).close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            phone_smoke_report,
            #[cfg(desktop)]
            list_routines,
            #[cfg(desktop)]
            set_routine_on,
            #[cfg(desktop)]
            allow_routine,
            #[cfg(desktop)]
            revoke_routine,
            #[cfg(desktop)]
            undo_routine,
            #[cfg(desktop)]
            run_routine_now,
            #[cfg(desktop)]
            apply_routine_fix,
            #[cfg(desktop)]
            dismiss_routine_fix,
            auto_setup,
            onboard_identity,
            quick_chips,
            chip_outcome,
            get_capabilities,
            get_settings,
            get_profile,
            set_profile_name,
            pick_profile_picture,
            clear_profile_picture,
            save_settings,
            set_theme,
            set_dock_panels,
            save_dock_layout,
            open_external,
            set_api_key,
            slash_hits,
            run_slash,
            record_status,
            record_stop,
            clear_api_key,
            start_grok_sign_in,
            poll_grok_sign_in,
            grok_sign_out,
            send_chat,
            transcribe,
            voice_live::voice_live_start,
            voice_live::voice_live_chat,
            voice_live::voice_live_stop,
            voice_live::voice_live_push,
            voice_live::voice_live_listen,
            voice_live::voice_live_playing,
            voice_live::voice_live_finish,
            voice_live::voice_live_hear,
            voice_live::voice_reply_feed,
            voice_live::voice_reply_finish,
            voice_live::voice_reply_hush,
            voice_live::voice_speak,
            voice_live::get_voice_route,
            voice_live::set_voice_route,
            answer_permission,
            answer_elicit,
            helper_questions,
            decision_inbox,
            answer_inbox,
            reply_pick,
            cancel_chat,
            steer_chat,
            turns::read_turn,
            turns::side_chat,
            turns::tell_main,
            halt_desktop,
            episode_state,
            episode_arm,
            episode_armed,
            episode_answer,
            episode_pause,
            episode_stop,
            get_halt_keys,
            close_chat,
            list_chats,
            search_chats,
            load_chat,
            chat_zoom,
            get_start_chat,
            rename_chat,
            delete_all_chats,
            cancel_delete_all,
            list_project_nodes,
            pin_chat,
            list_read_only_chats,
            view_read_only_chat,
            search_steps,
            delete_chat,
            list_chat_tabs,
            list_archived_chats,
            list_deleted_chats,
            chat_tab_switch,
            chat_tab_pin,
            chat_tab_rename,
            chat_tab_archive,
            chat_tab_unarchive,
            chat_tab_soft_delete,
            chat_tab_restore_deleted,
            clear_chat,
            get_hangout_mode,
            set_hangout_mode,
            forget_message,
            stage_attachment,
            stage_attachment_paths,
            pick_attachments,
            remove_attachment,
            list_automations,
            add_automation,
            list_suggested_automations,
            list_automation_blueprints,
            add_suggested_automation,
            set_automation_enabled,
            remove_automation,
            edit_automation,
            set_scheduler_clock,
            run_automation,
            #[cfg(desktop)]
            get_automations_service,
            #[cfg(desktop)]
            set_automations_service,
            get_memory,
            save_memory,
            add_memory,
            search_memory,
            edit_memory_row,
            forget_memory_row,
            approve_memory_row,
            reject_memory_row,
            set_memory_writes,
            pause_memory_row,
            add_personal_fact,
            export_personal,
            export_memory_wiki,
            wipe_personal,
            get_memory_files,
            save_memory_file,
            restore_memory_file,
            reflect_memory,
            tidy_memory_ledger,
            apply_memory_raw_tidy,
            dismiss_memory_raw_tidy,
            get_memory_retention,
            set_memory_retention,
            get_chat_archive,
            set_chat_archive,
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
            mcp_app_initialize,
            mcp_app_call_tool,
            code_repo,
            code_diff,
            code_builtins,
            code_test_plan,
            code_test_run,
            code_run_stop,
            code_test_remembered,
            code_test_forget,
            command_box_view,
            command_box_check,
            command_box_approve,
            command_box_forget,
            code_ship_plan,
            code_ship_run,
            code_new_branch,
            code_activity_list,
            code_rollback::code_revert_list,
            code_rollback::code_revert_plan,
            code_rollback::code_revert_run,
            code_secret_list,
            code_secret_set,
            code_secret_delete,
            code_deploy_plan,
            code_deploy_save,
            code_deploy_forget,
            code_deploy_run,
            code_files_list,
            code_file_read,
            code_file_write,
            code_term_open,
            code_term_write,
            code_term_resize,
            code_term_close,
            code_projects,
            code_project_open,
            code_project_pick,
            code_project_new,
            code_project_link_remote,
            code_project_set_branch,
            code_project_rules,
            code_session_bind,
            code_session_lead,
            code_session_plan,
            code_session_left,
            code_session_resume,
            get_code_ship,
            save_code_ship,
            vstack_sync_status,
            vstack_sync_now,
            get_journal,
            get_meetings,
            meetings_act,
            get_wake_word,
            wake_word_act,
            wake_word_feed_local,
            journal_add,
            journal_act,
            journal_to_mission,
            get_permissions,
            get_github,
            save_github_token,
            run_github,
            get_hooks,
            set_hooks_trust,
            scopes::get_scopes,
            scopes::allow_scope,
            scopes::revoke_scope,
            scopes::pick_scope_folder,
            scopes::forget_scope_facts,
            add_permission_rule,
            remove_permission_rule,
            remember_grant,
            forget_grant,
            get_personalities,
            set_personality,
            set_explicit_personalities,
            confirm_adult,
            set_quip_heat,
            set_dynamic_chat,
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
            set_skill_on,
            create_skill,
            get_skill_detail,
            edit_skill,
            search_skills_hub,
            install_skills_hub,
            get_learned,
            learned_act,
            learned_say,
            get_pulse_sources,
            save_pulse_sources,
            get_interests,
            add_interest,
            interest_act,
            know_me::get_know_me,
            know_me::set_know_me_on,
            know_me::set_know_me_signal,
            know_me::know_me_scan,
            know_me::know_me_delete,
            know_me::know_me_wipe,
            get_pulse,
            pulse_act,
            pulse_review_apply,
            pulse_pause_automation,
            pulse_done_undo,
            pulse_done_stop,
            model_resume,
            budget_card,
            answer_budget_card,
            pulse_review_issue,
            pulse_discuss,
            pulse_offer,
            pulse_live_open,
            pulse_live_current,
            pulse_live_allow,
            pulse_live_deny,
            pulse_live_stop,
            pulse_live_undo,
            pulse_live_close,
            pulse_remind,
            pulse_why,
            pulse_saved,
            pulse_unsave,
            pulse_fix,
            get_feed_prompt,
            save_feed_prompt,
            pulse_image,
            get_imagine,
            imagine_generate,
            imagine_cancel,
            imagine_image,
            chat_picture,
            imagine_sign_in_start,
            imagine_sign_in_poll,
            imagine_sign_out,
            get_ai_models,
            #[cfg(desktop)]
            claude_plan_status,
            #[cfg(desktop)]
            claude_plan_chats,
            #[cfg(desktop)]
            set_claude_plan_allowed,
            add_ai_provider,
            start_openrouter_sign_in,
            wait_openrouter_sign_in,
            stop_openrouter_sign_in,
            openrouter_sign_in_status,
            allow_ai_provider,
            answer_ai_models_card,
            remove_ai_provider,
            save_ai_spend,
            bench_ai_model,
            unbench_ai_model,
            save_web_search,
            save_brave_key,
            revoke_ai_premium,
            get_local_ai,
            save_local_ai,
            start_local_engine_download,
            cancel_local_engine_download,
            start_local_drafter_download,
            cancel_local_drafter_download,
            reset_local_ai,
            check_model_file,
            run_local_speed_test,
            set_local_keep_loaded,
            set_local_dynamic,
            set_local_use,
            set_prompt_tidy,
            unload_local_model,
            get_model_library,
            download_models,
            pause_model_downloads,
            remove_library_model,
            search_hf_gguf,
            list_hf_gguf_files,
            download_hf_gguf,
            get_web_access,
            set_web_access,
            get_devices,
            forget_all_devices,
            #[cfg(desktop)]
            get_share,
            #[cfg(desktop)]
            get_device_glance,
            #[cfg(desktop)]
            start_share,
            #[cfg(desktop)]
            stop_share,
            #[cfg(desktop)]
            new_share_code,
            #[cfg(desktop)]
            fleet_machines,
            #[cfg(desktop)]
            get_hub_join,
            #[cfg(desktop)]
            join_hub,
            #[cfg(desktop)]
            leave_hub,
            #[cfg(desktop)]
            fleet_usage,
            #[cfg(desktop)]
            get_fleet_rows,
            #[cfg(desktop)]
            set_fleet_row,
            #[cfg(desktop)]
            hub_send_approve,
            #[cfg(desktop)]
            get_hub_send_grant,
            #[cfg(desktop)]
            allow_hub_send_every_time,
            #[cfg(desktop)]
            ask_hub_send_each_time,
            get_update_status,
            set_update_channel,
            check_shell_update,
            install_shell_update,
            #[cfg(desktop)]
            os_extras::get_autostart,
            #[cfg(desktop)]
            os_extras::set_autostart,
            get_bot_desktop,
            set_bot_desktop,
            bot_desktop_frame,
            get_browser_attach,
            get_channels,
            connect_channel,
            disconnect_channel,
            approve_channel_code,
            unpair_channel_sender,
            set_channel_asks,
            add_channel_webhook,
            remove_channel_webhook,
            save_browser_attach,
            #[cfg(desktop)]
            os_extras::set_summon_hotkey,
            #[cfg(desktop)]
            os_extras::set_notifications,
            nyx_look,
            nyx_tiles,
            #[cfg(desktop)]
            desktop_nyx_get,
            #[cfg(desktop)]
            desktop_nyx_set,
            #[cfg(desktop)]
            desktop_nyx_undock,
            #[cfg(desktop)]
            desktop_nyx_dock,
            #[cfg(desktop)]
            desktop_nyx_set_outfit,
            #[cfg(desktop)]
            desktop_nyx_set_hit,
            #[cfg(desktop)]
            desktop_nyx_drag,
            #[cfg(desktop)]
            desktop_nyx_look,
            #[cfg(desktop)]
            desktop_nyx_talk,
            get_queue,
            stop_task,
            ship_status,
            ship_commit,
            ship_waive,
            merge_task_work,
            resume_task_work,
            discard_task_work,
            get_task_detail,
            get_workboard,
            workboard_add,
            workboard_act,
            workboard_say,
            workboard_work,
            workboard_playbook,
            home_run_playbook,
            workboard_recur,
            workboard_stamp,
            workboard_log,
            workboard_resume,
            workboard_delete_archived,
            workboard_sweep_undo,
            workboard_sweep_dismiss,
            get_workboard_tidy,
            set_workboard_tidy,
            get_sessions_board,
            sessions_board_stamp,
            sessions_board_pin,
            sessions_board_reshape,
            get_behavior,
            get_pc_helpers,
            composer_ghost_suggest,
            get_screenshot_offer,
            set_screenshot_explain,
            dismiss_screenshot_offer,
            explain_screenshot_offer,
            set_pc_helper,
            set_pc_auto_sleep,
            forget_pc_rhythm,
            get_pc_files,
            get_home_assistant,
            set_home_assistant,
            set_home_assistant_token,
            test_home_assistant,
            set_pc_files_switch,
            set_pc_folders,
            set_pc_models_folder,
            #[cfg(desktop)]
            add_pc_printer,
            remove_pc_printer,
            set_pc_printer_on,
            #[cfg(desktop)]
            pc_tidy_review,
            #[cfg(desktop)]
            pc_tidy_trash,
            #[cfg(desktop)]
            pc_resume_rows,
            #[cfg(desktop)]
            pc_speed_test,
            pc_net_csv,
            save_behavior,
            self_improve::get_self_improve,
            self_improve::set_self_improve_quiet,
            self_improve::self_improve_revert,
            self_improve::self_improve_issue,
            change_rows::get_change_rows,
            change_rows::change_row_undo,
            change_rows::change_row_keep,
            change_rows::change_row_restore,
            save_defaults,
            get_privacy,
            quiet_chip,
            get_diagnostics,
            set_diagnostics,
            diagnostics_preview,
            diagnostics_save,
            set_offline_only,
            open_data_folder,
            open_files_folder,
            change_files_folder,
            storage::get_storage,
            storage::storage_change,
            storage::storage_reset,
            storage::storage_bring_over,
            storage::storage_open,
            export_chat,
            open_saved,
            reveal_saved,
            get_background,
            get_home,
            get_pc_snapshot,
            get_mission,
            set_mission_done,
            get_goals,
            goal_act,
            get_library,
            library_cover,
            library_play,
            get_battlenet,
            battlenet_play,
            battlenet_open_folder,
            companion_frame,
            companion_set_note,
            companion_note,
            get_game_companion,
            set_game_companion,
            set_game_companion_ping,
            get_game_watch,
            #[cfg(desktop)]
            get_living_wall,
            #[cfg(desktop)]
            set_living_wall,
            set_game_watch,
            get_pulse_auto,
            get_pc_health,
            set_pc_health,
            set_pulse_auto_fix,
            set_proactive_reminders,
            set_pulse_auto_pause,
            get_steam_pulse,
            set_steam_achievements,
            set_steam_wishlist,
            set_steam_wishlist_threshold,
            save_steam_id,
            forget_steam_id,
            steam_check_now,
            get_local_model_news,
            set_local_model_news,
            local_model_news_check_now,
            game_wow,
            game_wow_addon,
            game_gw2,
            game_gw2_refresh,
            game_gw2_start_key,
            game_gw2_answer_key,
            game_gw2_remove_key,
            game_eso,
            game_eso_addon,
            game_osrs,
            game_osrs_save,
            game_osrs_forget,
            game_osrs_refresh,
            game_destiny,
            game_destiny_refresh,
            game_destiny_start_key,
            game_destiny_answer_key,
            game_destiny_save_name,
            game_destiny_remove,
            game_poe,
            game_elite,
            game_factorio,
            game_factorio_mod,
            game_stardew,
            game_minecraft,
            game_rl,
            home_open_clip,
            #[cfg(desktop)]
            home_pc_details,
            library_friends,
            library_steam_settings,
            mods_games,
            mods_browse,
            mods_search,
            mods_new_finds,
            mods_for_you,
            mods_not_for_me,
            mods_hide_game,
            mods_install::mods_files,
            mods_install::mods_requested,
            mods_install::mods_drop_request,
            mods_install::mods_plan_install,
            mods_install::mods_wait_for_browser,
            mods_install::mods_pickup_download,
            mods_install::mods_plan_uninstall,
            mods_install::mods_confirm,
            mods_install::mods_cancel,
            mods_install::mods_installed,
            home_turn_on,
            home_setup,
            home_setup_steps,
            updates_page,
            run_update_offer,
            home_catalog,
            set_home_layout,
            open_url
        ])
        .build(tauri::generate_context!());
    match built {
        Ok(app) => app.run(|app, event| {
            // What the learning engine holds in memory is written on the way out.
            if let tauri::RunEvent::Exit = event {
                quit_steps();
                // A computer that paired since Devices last looked stays paired.
                #[cfg(desktop)]
                if let Some(share) = app.try_state::<Arc<hub_share::HubShare>>() {
                    share.save();
                }
            }
            #[cfg(not(desktop))]
            let _ = app;
        }),
        Err(err) => {
            eprintln!(
                "{} failed to start: {err}",
                grokhub_core::brand::product_name()
            );
            std::process::exit(1);
        }
    }
}

/// What every way out runs before the process ends: the learning engine's
/// memory is written, a test run or deploy is stopped, and the on-device
/// model server is stopped so it doesn't outlive the app.
fn quit_steps() {
    grokhub_core::learn::hub::flush();
    code_runs::stop();
    grokhub_agent::route::local_sidecar::stop_global();
    // Its X server would outlive the app otherwise.
    bot_screen::stop();
    channels::stop_all();
}

/// A Windows update ends the process from inside the updater (it starts the
/// installer, then exits), so `RunEvent::Exit` never runs. This runs the same
/// steps first, so nothing the app started holds a file the installer
/// replaces, then Tauri's own cleanup, which the updater would run alone.
#[cfg(desktop)]
pub(crate) fn before_update_exit(app: &AppHandle) {
    quit_steps();
    if let Some(share) = app.try_state::<Arc<hub_share::HubShare>>() {
        share.save();
    }
    app.cleanup_before_exit();
}

#[cfg(test)]
mod command_tests {
    /// The text from `start` to the end of that item (the next line that is
    /// a lone `}` at the margin), with all whitespace taken out so a
    /// reformat doesn't break the checks.
    fn item_after(src: &str, start: &str) -> String {
        src.split(start)
            .nth(1)
            .and_then(|s| s.split("\n}\n").next())
            .unwrap_or_else(|| panic!("{start} is in lib.rs"))
            .split_whitespace()
            .collect()
    }

    fn squeeze(code: &str) -> String {
        code.split_whitespace().collect()
    }

    #[test]
    fn the_tray_lists_nyx_on_desktop_after_show() {
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        let tray = item_after(&src, "fn build_tray(");
        for want in [
            "CheckMenuItem::with_id(app, desktop_nyx::TRAY_ID,",
            "&[&local, &halt, &show, &nyx, &quit]",
            "desktop_nyx::start_tray(nyx)",
            "id if id == desktop_nyx::TRAY_ID => desktop_nyx_from_tray(app)",
        ] {
            assert!(tray.contains(&squeeze(want)), "build_tray: {want}");
        }
        let click = item_after(&src, "fn desktop_nyx_from_tray(");
        assert!(click.contains("tauri::async_runtime::spawn"));
        assert!(
            click.contains(&squeeze(
                "if toggled.is_err() { desktop_nyx::refresh_tray(&current(&state)); }"
            )),
            "a click that changed nothing puts the tick back"
        );
        assert!(
            !click.contains("is_checked"),
            "the settings decide, not the item's own tick"
        );
    }

    #[test]
    fn every_desktop_nyx_write_tells_the_tray_and_every_page() {
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        for name in [
            "async fn desktop_nyx_set(",
            "async fn desktop_nyx_undock(",
            "async fn desktop_nyx_dock(",
            "async fn desktop_nyx_set_outfit(",
            "fn desktop_nyx_watch(",
            "fn desktop_nyx_from_tray(",
        ] {
            let body = item_after(&src, name);
            assert!(body.contains("desktop_nyx_write(&app,&state,"), "{name}");
            assert!(
                !body.contains("settings::save("),
                "{name} saves through the write"
            );
        }
        // One at a time, the read inside the lock.
        let write = item_after(&src, "async fn desktop_nyx_write(");
        assert!(write.contains(&squeeze(concat!(
            "let _one = DESKTOP_NYX_WRITES.lock().await; let next = change(current(state))?; ",
            "desktop_nyx_",
            "commit(app, state, next, window)"
        ))));
        assert_eq!(
            src.matches(concat!("desktop_nyx_", "commit(app,")).count(),
            1,
            "only desktop_nyx_write commits"
        );
        // A window that fails still leaves the tray and pages right.
        let commit = item_after(&src, "fn desktop_nyx_commit(");
        assert!(commit.contains(&squeeze(
            "let told = desktop_nyx::publish(app, &next); shown.map(|()| told)"
        )));
        assert!(!commit.contains("show_window(app,&next)?"));
        let close = src
            .split("if let WindowEvent::CloseRequested")
            .nth(1)
            .and_then(|s| s.split("if window.label() == desktop_nyx::LABEL {").nth(1))
            .and_then(|s| s.split("return;").next())
            .expect("the desk's close arm");
        assert!(close.contains("desktop_nyx::publish(window.app_handle(), &next)"));
        // The main window going settles her desk, since its chats are gone.
        let gone = src
            .split("if let WindowEvent::Destroyed = event {")
            .nth(1)
            .and_then(|s| s.split("if let WindowEvent::").next())
            .map(squeeze)
            .expect("the main window's destroyed arm");
        assert!(gone.contains(&squeeze(
            r#"window.label() == "main" && desktop_nyx::set_life(desktop_nyx::Life::default())"#
        )));
        assert!(gone.contains("desktop_nyx::publish("));
        assert_eq!(
            src.matches(concat!("desktop_nyx_", "undocked = ")).count(),
            1,
            "only desktop_nyx_undock writes the field directly"
        );
        // Adults-only changes her wardrobe.
        for name in ["fn set_explicit_personalities(", "fn confirm_adult("] {
            assert!(
                item_after(&src, name).contains("adults_changed(&app,&state);"),
                "{name}"
            );
        }
    }

    #[test]
    fn settings_load_on_the_blocking_pool() {
        // `view` reads the keychain. A plain command runs on the main thread
        // and holds up the window while it first paints; an async worker
        // panics in the Secret Service client. Only the blocking pool fits.
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        let body = src
            .split("async fn get_settings(")
            .nth(1)
            .and_then(|s| s.split("\n}\n").next())
            .expect("get_settings is an async fn");
        assert!(
            body.contains("off_main(move || view(&settings)).await"),
            "get_settings must read the keychain through off_main: {body}"
        );
        assert!(
            !src.contains("#[tauri::command(async)]\nfn get_settings("),
            "a sync body on an async worker panics in zbus"
        );
    }

    #[test]
    fn get_to_know_you_runs_on_the_blocking_pool() {
        // It reads an OpenClaw workspace and writes three files and the rows
        // store: disk work that must not hold up the window.
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        let body = src
            .split(concat!("async fn ", "onboard_identity("))
            .nth(1)
            .and_then(|s| s.split("\n}\n").next())
            .expect("onboard_identity is an async fn");
        assert!(
            body.contains("off_main(move || {\n        onboard::act("),
            "onboard::act must run through off_main: {body}"
        );
    }

    #[test]
    fn the_capability_file_claims_a_manifest_only_if_the_build_has_one() {
        // With no app manifest in build.rs, Tauri allows every app command
        // from the app's own page. The capability file must not say otherwise.
        let has = include_str!("../build.rs").contains("app_manifest");
        let says = include_str!("../capabilities/default.json").contains("build manifest");
        assert_eq!(says, has);
    }

    #[test]
    fn the_share_commands_run_on_the_blocking_pool() {
        // Stop share writes and fsyncs hub-state.json, and the page's poll
        // saves a new pairing: neither may hold up the window.
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        for (name, call) in [
            ("get_share", "off_main(move || share.view()).await"),
            ("start_share", "off_main(move || share.start()).await?"),
            ("stop_share", "off_main(move || share.stop()).await"),
            ("new_share_code", "off_main(move || share.new_code()).await"),
            // The glance copies and checks frames of up to 400 KB.
            ("get_device_glance", "off_main(move || {"),
        ] {
            let body = src
                .split(&format!("async fn {name}("))
                .nth(1)
                .and_then(|s| s.split("\n}\n").next())
                .unwrap_or_else(|| panic!("{name} must be an async command"));
            assert!(
                body.contains(call),
                "{name} must run through off_main: {body}"
            );
        }
    }

    /// True when the `RunEvent::Exit` arm of `run` in `src` saves the share,
    /// looked up as the type it is managed as.
    fn quitting_saves_the_share(src: &str) -> bool {
        let app = src.split("\n#[cfg(test)]\nmod command_tests {").next();
        let arm = app
            .and_then(|s| s.split("if let tauri::RunEvent::Exit = event {").nth(1))
            .and_then(|s| s.split("\n            }\n").next())
            .expect("run must have an Exit arm");
        arm.contains("if let Some(share) = app.try_state::<Arc<hub_share::HubShare>>() {")
            && arm.contains("share.save();")
    }

    #[test]
    fn a_windows_update_runs_the_quit_steps_before_the_installer_starts() {
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        let arm = src
            .split("if let tauri::RunEvent::Exit = event {")
            .nth(1)
            .and_then(|s| s.split("\n            }\n").next())
            .expect("Exit arm");
        assert!(arm.contains("quit_steps();"), "{arm}");
        let hook = src
            .split("pub(crate) fn before_update_exit(app: &AppHandle) {")
            .nth(1)
            .and_then(|s| s.split("\n}\n").next())
            .expect("update hook");
        for step in [
            "quit_steps();",
            "app.try_state::<Arc<hub_share::HubShare>>()",
            "share.save();",
            "app.cleanup_before_exit();",
        ] {
            assert!(hook.contains(step), "{step} in {hook}");
        }
        let steps = src
            .split("fn quit_steps() {")
            .nth(1)
            .and_then(|s| s.split("\n}\n").next())
            .expect("quit_steps");
        for step in [
            "grokhub_core::learn::hub::flush();",
            "code_runs::stop();",
            "grokhub_agent::route::local_sidecar::stop_global();",
            "bot_screen::stop();",
            "channels::stop_all();",
        ] {
            assert!(steps.contains(step), "{step} in {steps}");
        }
        let update = include_str!("shell_update.rs").replace("\r\n", "\n");
        assert!(
            update.contains(".on_before_exit(move || crate::before_update_exit(&handle))"),
            "the updater must run the app's quit steps"
        );
    }

    #[test]
    fn quitting_saves_a_pairing_made_since_the_page_last_looked() {
        // A computer pairs inside the hub server, and the page's poll is
        // what writes it down. Quit before the next poll and only this hook
        // keeps the pairing.
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        assert!(
            src.contains("app.manage(Arc::new(hub_share::HubShare::open("),
            "the share is managed as Arc<hub_share::HubShare>"
        );
        assert!(quitting_saves_the_share(&src));
        // The check tells a hook that is gone, or that asks for a type the
        // app never manages (try_state then finds nothing, and nothing is
        // saved), from the real one.
        let hook = "                if let Some(share) = app.try_state::<Arc<hub_share::HubShare>>() {\n                    share.save();\n                }\n";
        assert_eq!(src.matches(hook).count(), 1);
        assert!(!quitting_saves_the_share(&src.replace(hook, "")));
        assert!(!quitting_saves_the_share(&src.replace(
            "app.try_state::<Arc<hub_share::HubShare>>()",
            "app.try_state::<hub_share::HubShare>()"
        )));
        assert!(!quitting_saves_the_share(
            &src.replace("                    share.save();\n", "")
        ));
    }
}

#[cfg(test)]
mod defaults_tests {
    use super::*;

    #[test]
    fn defaults_save_the_folder_approvals_and_thought_fold_only() {
        let before = ShellSettings {
            personality: "pirate".into(),
            ..ShellSettings::default()
        };
        let next = with_defaults(before, "  /work/notes ", settings::Approvals::Auto, true);
        assert_eq!(next.workspace, "/work/notes");
        assert_eq!(next.approvals, settings::Approvals::Auto);
        assert!(next.always_collapse_thoughts);
        assert_eq!(next.personality, "pirate");
        let back = with_defaults(next, "", settings::Approvals::Ask, false);
        assert!(!back.always_collapse_thoughts);
        assert_eq!(back.workspace, "");
    }
}

#[cfg(test)]
mod quiet_chip_tests {
    use crate::settings::Behavior;

    #[test]
    fn the_chip_names_the_end_only_inside_quiet_hours() {
        let night = Behavior {
            quiet_start: "22:00".into(),
            quiet_end: "07:00".into(),
            ..Behavior::default()
        };
        assert_eq!(
            super::quiet_chip_at(&night, "23:30"),
            Some("Quiet until 07:00".to_string())
        );
        assert_eq!(super::quiet_chip_at(&night, "12:00"), None);
        // The default has no quiet hours, so no chip at any time.
        assert_eq!(super::quiet_chip_at(&Behavior::default(), "03:00"), None);
    }
}

#[cfg(test)]
mod web_command_tests {
    /// The text of `remote_call`.
    fn remote_call_src() -> String {
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        let start = src.find("fn remote_call(").expect("remote_call");
        let body = &src[start..];
        body[..body.find("\n}\n").expect("end of remote_call")].to_string()
    }

    /// One command's arm in `remote_call`, up to the next arm.
    fn arm<'a>(body: &'a str, name: &str) -> &'a str {
        let from = body
            .find(&format!("\n        \"{name}\" =>"))
            .unwrap_or_else(|| panic!("no {name} arm"));
        let rest = &body[from + 1..];
        &rest[..rest.find("\n        \"").unwrap_or(rest.len())]
    }

    /// A name on the list with no arm in `remote_call` is refused at run
    /// time, so the web app's button for it never works.
    #[test]
    fn every_web_command_has_an_arm_in_remote_call() {
        let body = remote_call_src();
        let missing: Vec<&str> = crate::remote::WEB_COMMANDS
            .iter()
            .copied()
            .filter(|name| !body.contains(&format!("\"{name}\"")))
            .collect();
        assert_eq!(missing, Vec::<&str>::new());
    }

    /// A browser has no game to note, so its chip row must be asked for as
    /// remote and the desktop's as local; a swapped flag shows a browser the
    /// game chips, or hides them from the desktop.
    #[test]
    fn a_browsers_chip_row_is_asked_for_as_remote_and_the_desktops_as_local() {
        let body = remote_call_src();
        let web = arm(&body, "quick_chips");
        assert!(
            web.contains("arg(args, \"clock\")?,\n                true,\n"),
            "{web}"
        );
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        let start = src.find("async fn quick_chips(").expect("quick_chips");
        let desk = &src[start..start + src[start..].find("\n}\n").expect("end")];
        assert!(
            desk.contains("            clock,\n            false,\n"),
            "{desk}"
        );
        // A tap or hide acts on the row that client was shown.
        let web_tap = arm(&body, "chip_outcome");
        assert!(
            web_tap.contains("chip_outcome_from(")
                && web_tap.contains("arg(args, \"clock\")?,\n            true,\n"),
            "{web_tap}"
        );
        let start = src.find("async fn chip_outcome(").expect("chip_outcome");
        let desk_tap = &src[start..start + src[start..].find("\n}\n").expect("end")];
        assert!(
            desk_tap.contains("chip_outcome_from(app, surface, id, picked, clock, false)"),
            "{desk_tap}"
        );
    }

    /// The server refuses a name that isn't on the list before `remote_call`
    /// sees it, so an arm for one never runs.
    #[test]
    fn every_arm_in_remote_call_is_a_web_command() {
        let body = remote_call_src();
        let arms: Vec<&str> = body
            .lines()
            .filter_map(|line| line.strip_prefix("        \""))
            .filter_map(|line| line.split_once(" =>"))
            .flat_map(|(names, _)| names.split(" | "))
            .map(|name| name.trim_matches('"'))
            .collect();
        let unlisted: Vec<&str> = arms
            .iter()
            .copied()
            .filter(|name| !crate::remote::web_command(name))
            .collect();
        assert_eq!(unlisted, Vec::<&str>::new());
        assert_eq!(arms.len(), crate::remote::WEB_COMMANDS.len());
    }

    /// A browser never answers a hard card: the inbox it lists has no
    /// parks, and naming one by id is refused before anything is answered.
    #[test]
    fn a_browser_cannot_answer_a_park() {
        assert_eq!(super::web_inbox_kind("park"), Err(crate::remote::refused()));
        assert_eq!(super::web_inbox_kind("permission"), Ok(()));
        let body = remote_call_src();
        let answer = arm(&body, "answer_inbox");
        let check = answer.find("web_inbox_kind(&kind)?").expect("checked");
        let call = answer.find("out(answer_inbox(").expect("answered");
        assert!(check < call, "{answer}");
    }

    /// The phone that started a chat can't steer it, or answer its asks,
    /// while the computer drives the desktop in it.
    #[test]
    fn a_browser_cannot_steer_or_answer_a_chat_running_a_desktop_session() {
        use serde_json::json;
        let web = crate::remote::WebAccess::default();
        assert!(web.adopt("a1"));
        let mine = json!({ "conversationId": "a1", "text": "click Send now" });
        let idle = |_: &str| false;
        let on_desktop = |id: &str| id == "a1";
        assert_eq!(super::web_drive(&web, idle, &mine), Ok("a1".to_string()));
        assert_eq!(
            super::web_drive(&web, on_desktop, &mine),
            Err("That chat isn't open in this browser.".to_string())
        );
        let other = json!({ "conversationId": "b2", "text": "hello" });
        assert_eq!(
            super::web_drive(&web, idle, &other),
            Err("That chat isn't open in this browser.".to_string())
        );
        // The arms that steer a reply or answer its asks go through it;
        // stopping a reply stays open to the browser that started the chat.
        let body = remote_call_src();
        for name in ["answer_elicit", "answer_permission", "steer_chat"] {
            assert!(arm(&body, name).contains("drive(args)?"), "{name}");
            assert!(!arm(&body, name).contains("own(args)?"), "{name}");
        }
        for name in ["cancel_chat", "close_chat"] {
            assert!(arm(&body, name).contains("own(args)?"), "{name}");
        }
    }

    /// A send that is refused takes no chat: the browser's chats grow only
    /// by sends that go on to run.
    #[test]
    fn a_refused_send_takes_no_chat() {
        use serde_json::json;
        let dir = temp_dir("refused-send");
        let dir = dir.as_path();
        let web = crate::remote::WebAccess::default();
        let free = |_: &str| false;
        let open_here = |_: &str| true;
        let refused = |taken: &dyn Fn(&str) -> bool, args: serde_json::Value| {
            super::web_send(&web, dir, taken, &args)
                .map(|_| ())
                .unwrap_err()
        };
        // Nothing to send.
        assert_eq!(
            refused(&free, json!({ "conversationId": "a1", "text": "  " })),
            "Type a message first."
        );
        assert_eq!(
            refused(
                &free,
                json!({ "conversationId": "a1", "text": "", "attachments": [] })
            ),
            "Type a message first."
        );
        // A call the page never makes.
        assert_eq!(
            refused(&free, json!({ "conversationId": "a1" })),
            "text: invalid type: null, expected a string"
        );
        assert!(!web.owns("a1"));
        // An id no chat can have, and a chat the window started.
        assert_eq!(
            refused(&free, json!({ "conversationId": "../x", "text": "hi" })),
            "That chat isn't open in this browser."
        );
        assert!(!web.owns("../x"));
        assert_eq!(
            refused(
                &open_here,
                json!({ "conversationId": "desk-1", "text": "hi" })
            ),
            "That chat isn't open in this browser."
        );
        assert!(!web.owns("desk-1"));

        let (id, turn) = super::web_send(
            &web,
            dir,
            free,
            &json!({ "conversationId": "a1", "text": "hi" }),
        )
        .expect("a message sends");
        assert_eq!(id, "a1");
        assert_eq!(turn.text, "hi");
        assert_eq!(turn.attachments, Vec::<String>::new());
        assert!(web.owns("a1"));
        // Files alone send, and the browser's own chat stays its own once
        // it is open here.
        let (id, turn) = super::web_send(
            &web,
            dir,
            free,
            &json!({ "conversationId": "a2", "text": "", "attachments": ["/tmp/a.txt"] }),
        )
        .expect("files alone send");
        assert_eq!(id, "a2");
        assert_eq!(turn.attachments, vec!["/tmp/a.txt".to_string()]);
        assert!(web.owns("a2"));
        let again = super::web_send(
            &web,
            dir,
            open_here,
            &json!({ "conversationId": "a1", "text": "more" }),
        );
        assert_eq!(
            again.map(|(id, turn)| (id, turn.text)),
            Ok(("a1".to_string(), "more".to_string()))
        );
    }

    /// A file that was refused claims no chat. A source check: the arm
    /// needs the running app, so this only reads the order of its calls: the
    /// same check as a send first, then the file, then the chat is taken.
    #[test]
    fn the_stage_arm_takes_the_chat_only_after_the_file_is_staged() {
        let body = remote_call_src();
        let stage = arm(&body, "stage_attachment");
        let checked = stage.find("web_may_take(").expect("checks the chat");
        let staged = stage.find("stage_attachment(").expect("the staging call");
        let adopted = stage.find("web_take(").expect("takes the chat");
        assert!(checked < staged && staged < adopted, "{stage}");
        let between = &stage[staged..adopted];
        assert!(between.trim_end().ends_with(")?;"), "{stage}");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-web-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// The app's state over `dir`, as `run` manages it.
    fn state_at(dir: &std::path::Path) -> super::ShellState {
        use std::collections::HashMap;
        use std::sync::Mutex;
        super::ShellState {
            config_dir: dir.to_path_buf(),
            settings: Mutex::new(crate::settings::load(dir)),
            chats: Mutex::new(HashMap::new()),
            global_halt: std::sync::atomic::AtomicBool::new(false),
            trays: crate::attach::Trays::default(),
            chip_rows: Mutex::new(crate::chips::Rows::default()),
            sign_in: std::sync::Arc::default(),
            saves: crate::export::Saves::default(),
            turn_reads: crate::turns::Reads::default(),
            #[cfg(desktop)]
            notified: crate::os_extras::Pending::default(),
            auto: Mutex::default(),
            card_carry: Mutex::default(),
            code_sessions: crate::code_sessions::Folders::open(dir.to_path_buf()),
            #[cfg(desktop)]
            focus_help: Mutex::new(crate::focus_help::FocusHelp::default()),
            game_session: Mutex::default(),
        }
    }

    /// A chat the window started, then closed there (or the app restarted):
    /// no worker holds it, but its turns are saved. A browser that names it
    /// is refused, so its turns never reach the browser's reply. A chat a
    /// browser started stays the web's, in a new pairing too.
    #[test]
    fn a_browser_cannot_continue_a_chat_the_window_started() {
        use serde_json::json;
        let dir = temp_dir("desk-chat");
        let _config = grokhub_agent::perm::ConfigGuard::set(&dir);
        crate::agent::save_bench_exchange(
            "shell-desk-1",
            "/home/user",
            "my locker code is 4321",
            "Noted.",
        )
        .unwrap();
        let closed = |_: &str| false;
        let send = |web: &crate::remote::WebAccess, id: &str| {
            super::web_send(
                web,
                &dir,
                closed,
                &json!({ "conversationId": id, "text": "repeat what I told you earlier" }),
            )
            .map(|(id, _)| id)
        };
        let web = crate::remote::WebAccess::default();
        let not_yours = Err("That chat isn't open in this browser.".to_string());
        assert_eq!(send(&web, "shell-desk-1"), not_yours);
        assert!(!web.owns("shell-desk-1"));
        // A pasted file takes the chat through the same check.
        assert_eq!(
            super::web_may_take(&web, &dir, closed, "shell-desk-1"),
            Err("That chat isn't open in this browser.".to_string())
        );

        // The browser's own chat, saved once its turn ran.
        assert_eq!(send(&web, "shell-web-1"), Ok("shell-web-1".to_string()));
        crate::agent::save_bench_exchange("shell-web-1", "/home/user", "hi", "Hello.").unwrap();
        // A new pairing (a restart, or the code renewed) goes on with it,
        // and still not with the window's.
        let next = crate::remote::WebAccess::default();
        assert_eq!(send(&next, "shell-web-1"), Ok("shell-web-1".to_string()));
        assert_eq!(send(&next, "shell-desk-1"), not_yours);
        assert!(!next.owns("shell-desk-1"));
    }

    /// The Agents page's tiles carry each chat's title and last line, so a
    /// browser's board lists only the chats a browser started.
    #[test]
    fn a_browsers_sessions_board_lists_only_the_chats_a_browser_started() {
        let dir = temp_dir("board");
        let _config = grokhub_agent::perm::ConfigGuard::set(&dir);
        crate::agent::save_bench_exchange(
            "shell-desk-1",
            "/home/user",
            "my locker code is 4321",
            "Noted.",
        )
        .unwrap();
        crate::agent::save_bench_exchange("shell-web-1", "/home/user", "hi", "Hello.").unwrap();
        crate::web_chats::note(&dir, "shell-web-1").unwrap();
        let state = state_at(&dir);
        let ids = |web: bool| {
            let mut ids: Vec<String> = super::sessions_board_facts(&state, web)
                .into_iter()
                .map(|fact| fact.id)
                .collect();
            ids.sort();
            ids
        };
        assert_eq!(ids(true), ["shell-web-1"]);
        assert_eq!(ids(false), ["shell-desk-1", "shell-web-1"]);
        // The web's arms ask for the browser's board.
        let body = remote_call_src();
        for name in [
            "get_sessions_board",
            "sessions_board_pin",
            "sessions_board_reshape",
        ] {
            assert!(
                arm(&body, name).contains("(\n            &state,\n            true,\n"),
                "{name}"
            );
        }
    }

    /// Asks for one screenshot on its first turn with tools, then is done.
    #[derive(Default)]
    struct Screenshots {
        asked: std::sync::Mutex<Vec<grokhub_agent::ResponsesRequest>>,
    }

    impl grokhub_agent::ModelClient for Screenshots {
        fn stream(
            &self,
            req: &grokhub_agent::ResponsesRequest,
            _cancel: &grokhub_agent::CancelToken,
            sink: &mut dyn FnMut(grokhub_agent::StreamEvent),
        ) -> Result<grokhub_agent::TurnOutput, grokhub_agent::ClientError> {
            let main = |r: &grokhub_agent::ResponsesRequest| {
                r.tools.iter().any(|t| t["name"] == "read_file")
            };
            let mut asked = self.asked.lock().unwrap();
            let first = main(req) && !asked.iter().any(main);
            asked.push(req.clone());
            if first {
                return Ok(grokhub_agent::TurnOutput {
                    calls: vec![grokhub_agent::FunctionCall {
                        call_id: "shot".into(),
                        name: "screenshot".into(),
                        arguments: "{}".into(),
                    }],
                    ..grokhub_agent::TurnOutput::default()
                });
            }
            sink(grokhub_agent::StreamEvent::TextDelta("done".into()));
            Ok(grokhub_agent::TurnOutput {
                text: "done".into(),
                ..grokhub_agent::TurnOutput::default()
            })
        }
    }

    /// What a Run now of `id` behind `gate` got back from its screenshot.
    fn screenshot_result(
        dir: &std::path::Path,
        id: &str,
        gate: crate::unattended::RunGate,
    ) -> String {
        let model = std::sync::Arc::new(Screenshots::default());
        crate::automations::run_now(dir, id, model.clone(), "grok-4.7", dir.to_path_buf(), gate)
            .unwrap();
        let asked = model.asked.lock().unwrap();
        asked
            .iter()
            .flat_map(|r| r.input.iter())
            .find_map(|item| match item {
                grokhub_agent::InputItem::FunctionCallOutput { call_id, output }
                    if call_id == "shot" =>
                {
                    Some(output.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| "<no screenshot>".into())
    }

    /// Points chat at a loopback model while it lives, so `route` builds a
    /// client; nothing is sent to it.
    struct LoopbackModel;

    impl LoopbackModel {
        fn set() -> Self {
            // SAFETY: tests run one per process (nextest) or one at a time
            // (RUST_TEST_THREADS=1), and the var goes again on drop.
            unsafe {
                std::env::set_var(
                    crate::credential::TEST_URL_ENV,
                    "http://127.0.0.1:9/v1/responses",
                )
            };
            Self
        }
    }

    impl Drop for LoopbackModel {
        fn drop(&mut self) {
            // SAFETY: as in `set`.
            unsafe { std::env::remove_var(crate::credential::TEST_URL_ENV) };
        }
    }

    /// A paired browser's Run now goes with the window's settings minus
    /// desktop control, as its chats do, so its run gets no desktop tools.
    #[test]
    fn a_browsers_run_now_gets_no_desktop_tools() {
        use crate::unattended::RunGate;
        use grokhub_agent::PermMode;
        let dir = temp_dir("run-now");
        grokhub_agent::harness::use_key_store_for(
            &dir,
            std::sync::Arc::new(grokhub_agent::harness::MemoryKeyStore::new()),
        );
        let _model = LoopbackModel::set();
        let window = crate::settings::ShellSettings {
            desktop_control: true,
            approvals: crate::settings::Approvals::Always,
            workspace: dir.to_string_lossy().into_owned(),
            ..Default::default()
        };
        // What `run_automation_now` builds from the settings it is handed.
        let gate = |settings: &crate::settings::ShellSettings| {
            super::run_gate(&super::route(settings, super::agent::SessionMode::Chat, &dir).unwrap())
        };
        assert_eq!(
            gate(&window),
            RunGate {
                mode: PermMode::Always,
                desktop: true
            }
        );
        let web = gate(&super::web_settings(window.clone()));
        assert_eq!(
            web,
            RunGate {
                mode: PermMode::Always,
                desktop: false
            }
        );
        let clock = grokhub_core::LocalClock::at_offset(grokhub_core::now_ms(), 0);
        let id = crate::automations::add(&dir, "every day at 9am take a screenshot", clock)
            .unwrap()[0]
            .id
            .clone();
        assert_eq!(
            screenshot_result(&dir, &id, web),
            *grokhub_core::desktop_mcp::OFF_MSG
        );

        // The web's arm hands it those settings; the window's hands its own.
        let body = remote_call_src();
        let run = arm(&body, "run_automation");
        assert!(
            run.contains("web_settings(current(&state))")
                && run.contains("run_automation_now(state, id, settings)"),
            "{run}"
        );
        let send = arm(&body, "send_chat");
        assert!(send.contains("web_settings(current(&state))"), "{send}");
        let src = include_str!("lib.rs").replace("\r\n", "\n");
        let start = src
            .find("async fn run_automation_now(")
            .expect("run_automation_now");
        let now = &src[start..start + src[start..].find("\n}\n").expect("end")];
        assert!(!now.contains("current("), "{now}");
        let start = src
            .find("async fn run_automation(")
            .expect("run_automation");
        let window_run = &src[start..start + src[start..].find("\n}\n").expect("end")];
        assert!(
            window_run.contains("let settings = current(&state);")
                && window_run.contains("run_automation_now(state, id.clone(), settings)"),
            "{window_run}"
        );
    }

    /// A job a browser writes gets no desktop tools when it runs later on
    /// its own: each web arm that writes a job's words says so.
    #[test]
    fn the_web_arms_that_write_a_job_mark_it_as_the_browsers() {
        let body = remote_call_src();
        for name in [
            "add_automation",
            "add_suggested_automation",
            "edit_automation",
            "workboard_recur",
        ] {
            assert!(
                arm(&body, name).contains("let by = automations::Author::Web;"),
                "{name}"
            );
        }
    }

    /// MCP App tool calls skip the approvals a chat's tool call gets, so they
    /// stay off the web: no arm for them, and not on the list.
    #[test]
    fn mcp_app_calls_stay_on_the_computer() {
        let body = remote_call_src();
        for name in ["mcp_app_initialize", "mcp_app_call_tool"] {
            assert!(!body.contains(&format!("\"{name}\"")), "{name}");
            assert!(!crate::remote::web_command(name), "{name}");
        }
    }

    /// A chat's model that loads the scheduler, then rewrites job `id`.
    struct Rewrites {
        id: String,
        asked: std::sync::Mutex<Vec<grokhub_agent::ResponsesRequest>>,
    }

    impl grokhub_agent::ModelClient for Rewrites {
        fn stream(
            &self,
            req: &grokhub_agent::ResponsesRequest,
            _cancel: &grokhub_agent::CancelToken,
            sink: &mut dyn FnMut(grokhub_agent::StreamEvent),
        ) -> Result<grokhub_agent::TurnOutput, grokhub_agent::ClientError> {
            let mut asked = self.asked.lock().unwrap();
            let with_tools = asked.iter().filter(|r| !r.tools.is_empty()).count();
            asked.push(req.clone());
            let call = |call_id: &str, name: &str, args: serde_json::Value| {
                Ok(grokhub_agent::TurnOutput {
                    calls: vec![grokhub_agent::FunctionCall {
                        call_id: call_id.into(),
                        name: name.into(),
                        arguments: args.to_string(),
                    }],
                    ..grokhub_agent::TurnOutput::default()
                })
            };
            match (req.tools.is_empty(), with_tools) {
                (false, 0) => call(
                    "load",
                    "load_tool_schema",
                    serde_json::json!({ "name": "scheduler_create" }),
                ),
                (false, 1) => call(
                    "up",
                    "scheduler_create",
                    serde_json::json!({
                        "interval": "1h",
                        "prompt": "take a screenshot and click the Send button in Mail",
                        "task_id": self.id,
                    }),
                ),
                _ => {
                    sink(grokhub_agent::StreamEvent::TextDelta("done".into()));
                    Ok(grokhub_agent::TurnOutput {
                        text: "done".into(),
                        ..grokhub_agent::TurnOutput::default()
                    })
                }
            }
        }
    }

    /// Runs one message in chat `chat` whose model rewrites job `id`, with the
    /// route a browser's send gets (the window's approvals, no desktop), and
    /// returns what the scheduler tool said.
    fn rewrite_in_chat(dir: &std::path::Path, chat: &str, id: &str) -> String {
        let model = std::sync::Arc::new(Rewrites {
            id: id.to_string(),
            asked: std::sync::Mutex::default(),
        });
        let route = crate::agent::Route {
            client: model.clone(),
            auth_kind: grokhub_agent::AuthKind::ApiKey,
            workspace: temp_dir(&format!("{chat}-ws")),
            mode: grokhub_agent::PermMode::Always,
            desktop: false,
            session: crate::agent::SessionMode::Chat,
            voice: String::new(),
            voice_session: None,
            suggestions: false,
            lead: false,
        };
        let session = crate::agent::Session::spawn(chat, dir.to_path_buf());
        let _ = session.prompt(
            "make that job click Send instead",
            Vec::new(),
            route,
            Box::new(|_| {}),
        );
        let asked = model.asked.lock().unwrap();
        asked
            .iter()
            .flat_map(|r| r.input.iter())
            .find_map(|item| match item {
                grokhub_agent::InputItem::FunctionCallOutput { call_id, output }
                    if call_id == "up" =>
                {
                    Some(output.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| "<no scheduler call>".into())
    }

    /// A browser that asks its chat to rewrite a job gets the same job as
    /// one it edits on the Automations page: one that never runs with the
    /// desktop. A window chat's rewrite leaves its job as it was.
    #[test]
    fn a_job_a_browsers_chat_rewrites_runs_without_the_desktop() {
        use crate::unattended::RunGate;
        use grokhub_agent::PermMode;
        let dir = temp_dir("chat-rewrite");
        let _config = grokhub_agent::perm::ConfigGuard::set(&dir);
        let clock = grokhub_core::LocalClock::at_offset(grokhub_core::now_ms(), 0);
        crate::automations::add(&dir, "every day at 9am summarize my inbox", clock).unwrap();
        let list =
            crate::automations::add(&dir, "every day at 10am summarize the board", clock).unwrap();
        let (web_job, desk_job) = (list[0].id.clone(), list[1].id.clone());
        let on = RunGate::scheduled(PermMode::Always, true, None);
        let off = RunGate::scheduled(PermMode::Always, false, None);

        // What send_chat does before the browser's turn runs.
        let web = crate::remote::WebAccess::default();
        super::web_take(&web, &dir, "shell-web-rw-1").unwrap();
        assert_eq!(
            rewrite_in_chat(&dir, "shell-web-rw-1", &web_job),
            format!("updated {web_job}\nevery 60 min")
        );
        assert_eq!(
            rewrite_in_chat(&dir, "shell-desk-rw-1", &desk_job),
            format!("updated {desk_job}\nevery 60 min")
        );
        let jobs = crate::automations::load(&dir).unwrap();
        assert_eq!(
            [jobs[0].instructions.as_str(), jobs[1].instructions.as_str()],
            ["take a screenshot and click the Send button in Mail"; 2]
        );
        assert_eq!(crate::automations::gate_for(&dir, &web_job, on), off);
        assert_eq!(crate::automations::gate_for(&dir, &desk_job, on), on);

        // The scheduler's run of the browser's job, with the window's
        // desktop control saved on, never reaches the desktop.
        crate::settings::save(
            &dir,
            &crate::settings::ShellSettings {
                desktop_control: true,
                ..crate::settings::ShellSettings::default()
            },
        )
        .unwrap();
        let model = std::sync::Arc::new(Screenshots::default());
        let ws = temp_dir("chat-rewrite-run");
        crate::automations::execute(&dir, &jobs[0], model.clone(), "grok-4.7", ws, on);
        let shot = model
            .asked
            .lock()
            .unwrap()
            .iter()
            .flat_map(|r| r.input.iter())
            .find_map(|item| match item {
                grokhub_agent::InputItem::FunctionCallOutput { call_id, output }
                    if call_id == "shot" =>
                {
                    Some(output.clone())
                }
                _ => None,
            });
        assert_eq!(shot.as_deref(), Some(*grokhub_core::desktop_mcp::OFF_MSG));
    }

    /// Reflect hands back a chat's lines and saves them to shared memory, so
    /// a browser reflects only a chat it holds or a browser started: never a
    /// saved chat the window started, and never the window's last chat when
    /// it names none.
    #[test]
    fn a_browser_reflects_only_its_own_chats() {
        let dir = temp_dir("reflect");
        let _config = grokhub_agent::perm::ConfigGuard::set(&dir);
        crate::agent::save_bench_exchange(
            "shell-desk-1",
            "/home/user",
            "my locker code is 4321",
            "Noted.",
        )
        .unwrap();
        let web = crate::remote::WebAccess::default();
        let refused = Err("That chat isn't open in this browser.".to_string());
        let no_chat = Err(
            "A browser reflects only its own chats. Reflect the window's chats on your computer."
                .to_string(),
        );
        let reflect = |web: &crate::remote::WebAccess, id: Option<&str>| {
            super::web_reflect_chat(web, &dir, id.map(str::to_string))
        };
        assert_eq!(reflect(&web, Some("shell-desk-1")), refused);
        assert_eq!(reflect(&web, None), no_chat);
        assert_eq!(reflect(&web, Some("")), no_chat);

        super::web_take(&web, &dir, "shell-web-1").unwrap();
        assert_eq!(reflect(&web, Some("shell-web-1")), Ok("shell-web-1".into()));
        // A new pairing still reflects a chat a browser started.
        let next = crate::remote::WebAccess::default();
        assert_eq!(
            reflect(&next, Some("shell-web-1")),
            Ok("shell-web-1".into())
        );
        assert_eq!(reflect(&next, Some("shell-desk-1")), refused);

        // The arm reflects only the chat the check handed back.
        let body = remote_call_src();
        let arm = arm(&body, "reflect_memory");
        let checked = arm.find("web_reflect_chat(").expect("checks the chat");
        let reflected = arm
            .find("reflect_memory(state, Some(id))")
            .expect("reflects that chat");
        assert!(checked < reflected, "{arm}");
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

#[cfg(test)]
mod chat_bench_tests {
    use super::maybe_handle_bench_turn_to_disk;

    #[test]
    fn chat_turn_bench_command_persists_and_unbenches_without_normal_chat_interference() {
        let dir = std::env::temp_dir().join(format!("loadout-chat-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _cfg = grokhub_agent::perm::ConfigGuard::set(&dir);

        assert_eq!(
            maybe_handle_bench_turn_to_disk(&dir, "bench-chat-1", "don't use tabs", "/tmp")
                .unwrap(),
            None
        );

        let reply =
            maybe_handle_bench_turn_to_disk(&dir, "bench-chat-1", "stop using Grok", "/tmp")
                .unwrap()
                .expect("should confirm bench");
        assert!(reply.contains("Benched Grok (xAI)"), "{reply}");
        assert!(grokhub_agent::route::user_bench::load(&dir).matches_model("grok-4.7"));

        let reply2 =
            maybe_handle_bench_turn_to_disk(&dir, "bench-chat-1", "use Grok again", "/tmp")
                .unwrap()
                .expect("should confirm unbench");
        assert!(reply2.contains("Unbenched Grok (xAI)"), "{reply2}");
        assert!(!grokhub_agent::route::user_bench::load(&dir).matches_model("grokhub-nonexistent"));
        assert!(!grokhub_agent::route::user_bench::load(&dir).matches_model("grok-4.7"));

        let _ = std::fs::remove_dir_all(&dir);
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
