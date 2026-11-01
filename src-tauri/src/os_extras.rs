//! Desktop basics: one running copy, the window's place and size, the summon
//! hotkey, notifications you can act on, and start at login. All of it runs
//! from Rust through the official plugins; the page only flips the switches,
//! so the window's capability file grants no plugin command.
//!
//! A notification never breaks chat: with no notification server (or no
//! session bus on Linux) the plugin drops it and the turn carries on.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use grokhub_agent::PermAnswer;
use tauri::{AppHandle, Emitter, Manager, State, Wry};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt as _, ShortcutState};
use tauri_plugin_notification::{Action, ActionPerformed, ActionType, NotificationExt as _};
use tauri_plugin_window_state::StateFlags;

use crate::agent::UiEvent;
use crate::settings::ShellSettings;
use crate::{capabilities, settings, ChatEvent, SettingsView, ShellState, CHAT_EVENT};

/// Shows the window from anywhere. The egui cabin holds Super+G and
/// Super+Shift+Esc, so this one stays clear of both.
pub const SUMMON_HOTKEY: &str = "CommandOrControl+Shift+Space";

/// The argument a start at login passes, so that launch can stay in the tray.
pub const AUTOSTART_ARG: &str = "--autostart";

/// The action type of a notification that asks for an OK.
const ASK_ACTIONS: &str = "permission";
const DENY: &str = "deny";
const OPEN: &str = "open";

/// How much of a step's detail a notification shows; the card has the rest.
const DETAIL_CHARS: usize = 120;

/// What a click on one of the shell's notifications does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clicked {
    /// Deny the step the notification asks about.
    Deny,
    /// Bring the window up; the card there has every answer.
    Open,
}

impl Clicked {
    /// The answer this click gives the waiting step. Allow and Always are
    /// never answered from a notification, only on the full card.
    pub fn answer(self) -> Option<PermAnswer> {
        match self {
            Self::Deny => Some(PermAnswer::Deny),
            Self::Open => None,
        }
    }
}

/// The click a notification reports: Deny or Open, or `tap` on its body,
/// which opens the window. Anything else, an `allow` or `always` included,
/// does nothing.
pub fn parse_action(id: &str) -> Option<Clicked> {
    match id {
        DENY => Some(Clicked::Deny),
        OPEN | "tap" => Some(Clicked::Open),
        _ => None,
    }
}

/// A notification goes out only while it is switched on and the window is
/// not the one in front.
pub fn should_notify(focused: bool, enabled: bool) -> bool {
    enabled && !focused
}

/// Whether this launch came from the login items.
pub fn at_login<S: AsRef<str>>(args: impl IntoIterator<Item = S>) -> bool {
    args.into_iter().any(|arg| arg.as_ref() == AUTOSTART_ARG)
}

/// A start at login stays in the tray when closing the window keeps it there.
pub fn starts_hidden<S: AsRef<str>>(
    args: impl IntoIterator<Item = S>,
    close_to_tray: bool,
) -> bool {
    close_to_tray && at_login(args)
}

/// The step a notification asks about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub conversation_id: String,
    pub call_id: String,
}

/// The asks whose notifications are still out, by notification id. The window
/// and a notification answer each ask through here, holding the map while
/// they send, so the two answers can't cross and the card shows what ran.
#[derive(Debug, Default)]
pub struct Pending {
    next: AtomicI32,
    asks: Mutex<HashMap<i32, Ask>>,
}

impl Pending {
    fn asks(&self) -> MutexGuard<'_, HashMap<i32, Ask>> {
        self.asks.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// A new notification id, never 0.
    pub fn fresh_id(&self) -> i32 {
        loop {
            let id = self.next.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
            if id != 0 {
                return id;
            }
        }
    }

    /// Records the ask a new notification stands for; returns its id.
    pub fn add(&self, ask: Ask) -> i32 {
        let id = self.fresh_id();
        self.asks().insert(id, ask);
        id
    }

    /// The notification `id` was answered: runs `send` for its ask, once.
    /// `None` when the window answered first or the turn ended.
    pub fn answer_notification(&self, id: i32, send: impl FnOnce(&Ask)) -> Option<Ask> {
        let mut asks = self.asks();
        let ask = asks.remove(&id)?;
        send(&ask);
        Some(ask)
    }

    /// The window answered `call_id`: runs `send`, and returns the
    /// notification to take down, if one is out.
    pub fn answer_window(
        &self,
        conversation_id: &str,
        call_id: &str,
        send: impl FnOnce(),
    ) -> Option<i32> {
        let mut asks = self.asks();
        let id = asks
            .iter()
            .find(|(_, ask)| ask.conversation_id == conversation_id && ask.call_id == call_id)
            .map(|(id, _)| *id);
        if let Some(id) = id {
            asks.remove(&id);
        }
        send();
        id
    }

    /// The turn ended, so its asks were retired: the notifications to take down.
    pub fn end(&self, conversation_id: &str) -> Vec<i32> {
        let mut asks = self.asks();
        let ids: Vec<i32> = asks
            .iter()
            .filter(|(_, ask)| ask.conversation_id == conversation_id)
            .map(|(id, _)| *id)
            .collect();
        for id in &ids {
            asks.remove(id);
        }
        ids
    }
}

/// `text` cut to `max` characters, on one line.
fn short(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let mut cut: String = line.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// The desktop plugins, single-instance first so a second launch hands over
/// before anything else starts.
pub fn plugins(builder: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    builder
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // A login start while the app already runs changes nothing.
            if !at_login(&args) {
                crate::show_main(app);
            }
        }))
        // Place and size come back; visibility doesn't, so a window hidden
        // in the tray never comes back hidden.
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(StateFlags::all() - StateFlags::VISIBLE)
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args([AUTOSTART_ARG])
                .app_name(grokhub_core::brand::product_name())
                .build(),
        )
}

/// The buttons on a notification that asks for an OK: Open and Deny, never
/// Allow. Open comes first: macOS folds two actions into a menu whose button
/// reports the first one, and a stray press there should open, not deny.
fn ask_actions() -> ActionType {
    ActionType::builder(ASK_ACTIONS)
        .actions(vec![
            Action::builder(OPEN, "Open").foreground(true).build(),
            Action::builder(DENY, "Deny").destructive(true).build(),
        ])
        .build()
}

/// Wires the notification clicks and the saved hotkey. A failure here only
/// costs that feature, never the app.
pub fn setup(app: &AppHandle, loaded: &ShellSettings) {
    let notification = app.notification();
    if let Err(err) = notification.register_action_types(vec![ask_actions()]) {
        eprintln!("notifications: {err}");
    }
    let handle = app.clone();
    if let Err(err) = notification.on_action(move |performed| on_click(&handle, performed)) {
        eprintln!("notifications: {err}");
    }
    if loaded.summon_hotkey && capabilities::current().summon_hotkey {
        if let Err(err) = hotkey(app, true) {
            eprintln!("summon hotkey: {err}");
        }
    }
}

fn on_click(app: &AppHandle, performed: &ActionPerformed) {
    let Some(clicked) = parse_action(performed.action_id()) else {
        return;
    };
    let Some(answer) = clicked.answer() else {
        crate::show_main(app);
        return;
    };
    let Some(id) = performed.notification().map(|shown| shown.id()) else {
        return;
    };
    let state = app.state::<ShellState>();
    let answered = state.notified.answer_notification(id, |ask| {
        let session = state
            .chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&ask.conversation_id)
            .cloned();
        if let Some(session) = session {
            session.answer(&ask.call_id, answer);
        }
    });
    if let Some(ask) = answered {
        let _ = app.emit(
            CHAT_EVENT,
            ChatEvent {
                conversation_id: &ask.conversation_id,
                event: &UiEvent::Answered {
                    id: ask.call_id,
                    answer: crate::agent::answer_word(answer).to_string(),
                },
            },
        );
    }
}

/// Notifications are on, the window is in the background, and the chat is
/// the window's own: a paired browser's chat notifies nobody here.
fn wanted(app: &AppHandle, conversation_id: &str) -> bool {
    if app
        .state::<Arc<crate::remote::WebAccess>>()
        .owns(conversation_id)
    {
        return false;
    }
    let enabled = crate::current(&app.state::<ShellState>()).notifications
        && capabilities::current().notifications;
    let focused = app
        .get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false);
    should_notify(focused, enabled)
}

fn notify(app: &AppHandle, id: i32, title: &str, body: &str, ask: bool) {
    let mut note = app.notification().builder().id(id).title(title).body(body);
    if ask {
        note = note.action_type_id(ASK_ACTIONS);
    }
    if let Err(err) = note.show() {
        eprintln!("notifications: {err}");
    }
}

/// Chat's notifications: an OK to give, with Deny and Open, and a finished
/// reply. Runs for every chat event before it reaches the page.
pub fn on_chat_event(app: &AppHandle, conversation_id: &str, event: &UiEvent) {
    let state = app.state::<ShellState>();
    match event {
        UiEvent::Permission {
            id, title, action, ..
        } => {
            if !wanted(app, conversation_id) {
                return;
            }
            let ask = state.notified.add(Ask {
                conversation_id: conversation_id.to_string(),
                call_id: id.clone(),
            });
            let product = grokhub_core::brand::product_name();
            notify(
                app,
                ask,
                &format!("{product} wants to run {}", short(title, DETAIL_CHARS)),
                &short(action, DETAIL_CHARS),
                true,
            );
        }
        UiEvent::Done { stop } => {
            let retired = state.notified.end(conversation_id);
            if !retired.is_empty() {
                let _ = app.notification().remove_active(retired);
            }
            if stop != "cancelled" && wanted(app, conversation_id) {
                notify(
                    app,
                    state.notified.fresh_id(),
                    "Reply finished",
                    "Grok's reply is ready.",
                    false,
                );
            }
        }
        _ => {}
    }
}

/// The window answered an ask: its notification, if one is out, comes down.
pub fn answered_in_window(
    app: &AppHandle,
    conversation_id: &str,
    call_id: &str,
    send: impl FnOnce(),
) {
    let state = app.state::<ShellState>();
    if let Some(id) = state.notified.answer_window(conversation_id, call_id, send) {
        let _ = app.notification().remove_active(vec![id]);
    }
}

/// A Run now finished while the window was in the background.
pub fn automation_finished(app: &AppHandle, name: &str) {
    let enabled = crate::current(&app.state::<ShellState>()).notifications
        && capabilities::current().notifications;
    let focused = app
        .get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false);
    if should_notify(focused, enabled) {
        let id = app.state::<ShellState>().notified.fresh_id();
        notify(
            app,
            id,
            "Automation finished",
            &short(name, DETAIL_CHARS),
            false,
        );
    }
}

fn hotkey(app: &AppHandle, on: bool) -> Result<(), String> {
    let shortcuts = app.global_shortcut();
    let registered = shortcuts.is_registered(SUMMON_HOTKEY);
    match (on, registered) {
        (true, false) => shortcuts
            .on_shortcut(SUMMON_HOTKEY, |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    crate::show_main(app);
                }
            })
            .map_err(|err| format!("Couldn't set the hotkey: {err}")),
        (false, true) => shortcuts
            .unregister(SUMMON_HOTKEY)
            .map_err(|err| format!("Couldn't clear the hotkey: {err}")),
        _ => Ok(()),
    }
}

fn store(state: &ShellState, next: ShellSettings) -> Result<SettingsView, String> {
    settings::save(&state.config_dir, &next)?;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = next.clone();
    Ok(crate::view(&next))
}

/// Settings → General → start at login, as the OS has it.
#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    capabilities::needs(capabilities::current().autostart, "Start at login")?;
    app.autolaunch().is_enabled().map_err(|err| err.to_string())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, on: bool) -> Result<bool, String> {
    capabilities::needs(capabilities::current().autostart, "Start at login")?;
    let login = app.autolaunch();
    if on { login.enable() } else { login.disable() }.map_err(|err| err.to_string())?;
    login.is_enabled().map_err(|err| err.to_string())
}

/// Settings → General → the summon hotkey. Saved only once it is set.
#[tauri::command]
pub fn set_summon_hotkey(
    app: AppHandle,
    state: State<'_, ShellState>,
    on: bool,
) -> Result<SettingsView, String> {
    capabilities::needs(capabilities::current().summon_hotkey, "The summon hotkey")?;
    hotkey(&app, on)?;
    store(
        &state,
        ShellSettings {
            summon_hotkey: on,
            ..crate::current(&state)
        },
    )
}

#[tauri::command]
pub fn set_notifications(state: State<'_, ShellState>, on: bool) -> Result<SettingsView, String> {
    capabilities::needs(capabilities::current().notifications, "Notifications")?;
    store(
        &state,
        ShellSettings {
            notifications: on,
            ..crate::current(&state)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_and_open_are_the_only_clicks() {
        assert_eq!(parse_action("deny"), Some(Clicked::Deny));
        assert_eq!(parse_action("open"), Some(Clicked::Open));
        assert_eq!(parse_action("tap"), Some(Clicked::Open));
        assert_eq!(parse_action("allow"), None);
        assert_eq!(parse_action("always"), None);
        assert_eq!(parse_action("Deny"), None);
        assert_eq!(parse_action(""), None);
    }

    #[test]
    fn an_ask_offers_open_and_deny_only() {
        let actions = ask_actions();
        let buttons: Vec<(&str, &str)> = actions
            .actions()
            .iter()
            .map(|action| (action.id(), action.title()))
            .collect();
        assert_eq!(actions.id(), "permission");
        assert_eq!(buttons, vec![("open", "Open"), ("deny", "Deny")]);
    }

    #[test]
    fn only_deny_answers_the_step() {
        assert_eq!(Clicked::Deny.answer(), Some(PermAnswer::Deny));
        assert_eq!(Clicked::Open.answer(), None);
        // A tap on the body opens the window and answers nothing.
        assert_eq!(parse_action("tap").and_then(Clicked::answer), None);
    }

    #[test]
    fn notifies_only_when_on_and_in_the_background() {
        assert!(!should_notify(true, true));
        assert!(should_notify(false, true));
        assert!(!should_notify(false, false));
        assert!(!should_notify(true, false));
    }

    #[test]
    fn the_hotkey_is_ctrl_shift_space_and_parses() {
        assert_eq!(SUMMON_HOTKEY, "CommandOrControl+Shift+Space");
        let parsed: tauri_plugin_global_shortcut::Shortcut =
            SUMMON_HOTKEY.parse().expect("a valid shortcut");
        assert_eq!(
            parsed,
            tauri_plugin_global_shortcut::Shortcut::new(
                Some(
                    tauri_plugin_global_shortcut::Modifiers::SHIFT
                        | if cfg!(target_os = "macos") {
                            tauri_plugin_global_shortcut::Modifiers::SUPER
                        } else {
                            tauri_plugin_global_shortcut::Modifiers::CONTROL
                        }
                ),
                tauri_plugin_global_shortcut::Code::Space,
            )
        );
    }

    #[test]
    fn a_login_start_stays_in_the_tray_only_when_closing_keeps_it_there() {
        let login = ["/usr/bin/loadout", "--autostart"];
        let opened = ["/usr/bin/loadout"];
        assert!(at_login(login));
        assert!(!at_login(opened));
        assert!(!at_login(["/usr/bin/loadout", "--autostart=no"]));
        assert!(starts_hidden(login, true));
        assert!(!starts_hidden(login, false));
        assert!(!starts_hidden(opened, true));
    }

    fn ask(conversation: &str, call: &str) -> Ask {
        Ask {
            conversation_id: conversation.into(),
            call_id: call.into(),
        }
    }

    #[test]
    fn a_notification_answers_its_ask_once() {
        let pending = Pending::default();
        let first = pending.add(ask("c1", "call-1"));
        let second = pending.add(ask("c1", "call-2"));
        assert_eq!((first, second), (1, 2));
        let mut sent = Vec::new();
        assert_eq!(
            pending.answer_notification(first, |a| sent.push(a.call_id.clone())),
            Some(ask("c1", "call-1"))
        );
        assert_eq!(
            pending.answer_notification(first, |_| panic!("twice")),
            None
        );
        assert_eq!(pending.answer_notification(99, |_| panic!("unknown")), None);
        assert_eq!(sent, vec!["call-1".to_string()]);
    }

    #[test]
    fn the_window_answering_first_disarms_the_notification() {
        let pending = Pending::default();
        let id = pending.add(ask("c1", "call-1"));
        let mut sent = 0;
        assert_eq!(
            pending.answer_window("c1", "call-1", || sent += 1),
            Some(id)
        );
        // The window's answer always goes through, notification or not.
        assert_eq!(pending.answer_window("c1", "call-1", || sent += 1), None);
        assert_eq!(sent, 2);
        assert_eq!(
            pending.answer_notification(id, |_| panic!("disarmed")),
            None
        );
    }

    #[test]
    fn a_finished_turn_takes_down_only_its_own_asks() {
        let pending = Pending::default();
        let a = pending.add(ask("c1", "x"));
        let b = pending.add(ask("c2", "y"));
        let c = pending.add(ask("c1", "z"));
        let mut ended = pending.end("c1");
        ended.sort_unstable();
        assert_eq!(ended, vec![a, c]);
        assert_eq!(pending.end("c1"), Vec::<i32>::new());
        assert_eq!(pending.answer_notification(b, |_| {}), Some(ask("c2", "y")));
    }

    #[test]
    fn ids_skip_zero_when_they_wrap() {
        let pending = Pending {
            next: AtomicI32::new(-1),
            ..Pending::default()
        };
        assert_eq!(pending.fresh_id(), 1);
        assert_eq!(pending.fresh_id(), 2);
    }

    #[test]
    fn detail_is_one_short_line() {
        assert_eq!(short("rm  -rf\n build", 120), "rm -rf build");
        assert_eq!(short("abcdef", 4), "abc…");
        assert_eq!(short("abcd", 4), "abcd");
    }

    #[test]
    fn a_notification_answer_reaches_the_page_as_answered() {
        use crate::agent::{answer_word, parse_answer};
        let answered = serde_json::to_value(UiEvent::Answered {
            id: "c".into(),
            answer: answer_word(PermAnswer::Deny).into(),
        })
        .expect("json");
        assert_eq!(
            answered,
            serde_json::json!({"kind": "answered", "id": "c", "answer": "deny"})
        );
        for answer in [PermAnswer::Allow, PermAnswer::Always, PermAnswer::Deny] {
            assert_eq!(parse_answer(answer_word(answer)), Ok(answer));
        }
        assert_eq!(answer_word(PermAnswer::Cancel), "deny");
    }
}
