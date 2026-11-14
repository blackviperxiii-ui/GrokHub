//! The app reads what each message is for, so there is no Chat / Plan / btw
//! choice to make. Nothing running: a big multi-step job plans first and
//! anything else is answered. A reply running: a side question gets its own
//! read-only answer beside the work, a correction steers the work, "stop"
//! stops it, and a new job waits its turn (the page does those last three).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use grokhub_agent::route::intent::{self, TurnCx, TurnRoute};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{agent, ShellState, CHAT_EVENT};

/// A side answer runs under `<chat>~side-<n>`.
pub const SIDE_MARK: &str = "~side-";

/// Each chat's last read message and the route it took, so "no, just
/// answer" or "make a plan first" right after it can fix it.
#[derive(Default)]
pub struct Reads(Mutex<HashMap<String, (String, TurnRoute)>>);

impl Reads {
    /// The reply running in `conversation_id` answers casual talk: its turn
    /// was read as chat (not a plan) and its ask is small talk or a quick
    /// question ([`intent::casual_turn`]).
    fn casual_running(&self, conversation_id: &str) -> bool {
        let reads = self.0.lock().unwrap_or_else(|e| e.into_inner());
        reads
            .get(conversation_id)
            .is_some_and(|(ask, took)| *took == TurnRoute::Chat && intent::casual_turn(ask))
    }
}

/// The route for a message sent while nothing runs. A plan still waiting
/// on approval keeps planning; "no, just answer" or "make a plan first"
/// right after a turn fixes that turn's read and teaches the reader, on
/// this device only.
pub fn pick_mode(state: &ShellState, conversation_id: &str, text: &str) -> agent::SessionMode {
    let mut reads = state.turn_reads.0.lock().unwrap_or_else(|e| e.into_inner());
    let fix = intent::correction_of_previous(text);
    if let (Some(fix), Some((prev, was))) = (fix, reads.remove(conversation_id)) {
        // A scratch chat teaches the reader nothing.
        if fix != was && !grokhub_agent::scratch::is_scratch(conversation_id) {
            intent::note_correction(&state.config_dir, &prev, false, fix);
        }
    }
    let route = fix.unwrap_or_else(|| {
        let cx = TurnCx {
            running: false,
            prior: None,
            casual: false,
        };
        intent::read_turn_learned(&state.config_dir, text, &cx).route
    });
    let pending = grokhub_agent::plan_on(conversation_id) && fix != Some(TurnRoute::Chat);
    let plan = route == TurnRoute::Plan || pending;
    let (mode, took) = if plan {
        (agent::SessionMode::Plan, TurnRoute::Plan)
    } else {
        (agent::SessionMode::Chat, TurnRoute::Chat)
    };
    reads.insert(conversation_id.to_string(), (text.to_string(), took));
    mode
}

/// What a message typed during a reply is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnReadView {
    /// "side", "steer", "queue" or "stop".
    pub route: &'static str,
    /// The words to act on, with a typed `/btw` or `btw` lead taken off.
    pub text: String,
    /// The line to show at once, so the message never looks dropped.
    pub ack: &'static str,
}

/// Reads a message typed while `conversation_id`'s reply runs. Pure and
/// instant: no model call. When that reply answers casual talk, every
/// message waits as its own turn (see [`TurnCx::casual`]).
pub fn read_running(state: &ShellState, conversation_id: &str, text: &str) -> TurnReadView {
    let casual = state.turn_reads.casual_running(conversation_id);
    let cx = TurnCx {
        running: true,
        prior: None,
        casual,
    };
    let read = intent::read_turn_learned(&state.config_dir, text, &cx);
    // Chat and Plan only fit an idle chat; while running, a new job waits.
    let route = if read.route.fits(true) {
        read.route
    } else {
        TurnRoute::Queue
    };
    TurnReadView {
        route: route.id(),
        text: read.text,
        ack: route.ack(),
    }
}

#[tauri::command]
pub fn read_turn(
    state: State<'_, ShellState>,
    conversation_id: Option<String>,
    text: String,
) -> TurnReadView {
    read_running(&state, conversation_id.as_deref().unwrap_or(""), &text)
}

/// True when `side_id` is a side answer of `conversation_id`.
pub fn side_of(side_id: &str, conversation_id: &str) -> bool {
    side_id
        .strip_prefix(conversation_id)
        .and_then(|rest| rest.strip_prefix(SIDE_MARK))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Answers `question` beside `conversation_id`'s running reply: its own
/// worker starts from the chat's saved turns, may read but changes nothing,
/// and streams as `chat://event` under `side_id`. Its turns are never saved,
/// so History never lists it. Resolves with the stop reason.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn side_chat(
    app: AppHandle,
    state: State<'_, ShellState>,
    conversation_id: String,
    side_id: String,
    question: String,
    ask: Option<String>,
    so_far: Option<String>,
) -> Result<String, String> {
    if !side_of(&side_id, &conversation_id) {
        return Err("That side answer isn't for this chat.".into());
    }
    if question.trim().is_empty() {
        return Err("Type a message first.".into());
    }
    // Retired before it runs: the engine saves nothing under this id.
    let _ = grokhub_agent::delete_session(&side_id);
    let session = {
        let mut chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
        let session = chats.entry(side_id.clone()).or_insert_with(|| {
            Arc::new(agent::Session::spawn_side(
                &side_id,
                &conversation_id,
                state.config_dir.clone(),
            ))
        });
        Arc::clone(session)
    };
    let settings = crate::current(&state);
    let config_dir = state.config_dir.clone();
    let prompt = intent::side_prompt(
        &question,
        ask.as_deref().unwrap_or(""),
        so_far.as_deref().unwrap_or(""),
    );
    let id = side_id.clone();
    // A side answer reads the folder its chat works in.
    let own_folder = state.code_sessions.of(&conversation_id);
    let stop = tauri::async_runtime::spawn_blocking(move || {
        let mut route = crate::route(&settings, agent::SessionMode::Btw, &config_dir)?;
        if let Some(dir) = own_folder {
            route.workspace = dir;
        }
        route.suggestions = false;
        session.prompt(
            &prompt,
            Vec::new(),
            route,
            Box::new(move |event| {
                let _ = app.emit(
                    CHAT_EVENT,
                    crate::ChatEvent {
                        conversation_id: &id,
                        event: &event,
                    },
                );
            }),
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    crate::drop_worker(&state, &side_id);
    stop
}

/// What **Tell the main task** did with a side answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Told {
    /// The words for the main task.
    pub text: String,
    /// They joined the running reply; false means send them as the next message.
    pub steered: bool,
}

/// Feeds a side answer to the main task: into the running reply at its next
/// step, or back to the page to send next. A message shaped like `question`
/// steers next time instead of going to the side.
#[tauri::command]
pub fn tell_main(
    state: State<'_, ShellState>,
    conversation_id: String,
    question: String,
    answer: String,
) -> Told {
    if !grokhub_agent::scratch::is_scratch(&conversation_id) {
        intent::note_correction(&state.config_dir, &question, true, TurnRoute::Steer);
    }
    let text = intent::tell_main_text(&question, &answer);
    let steered = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
        .is_some_and(|session| session.steer(&text));
    Told { text, steered }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_side_id_belongs_to_its_own_chat_only() {
        assert!(side_of("shell-a~side-1", "shell-a"));
        assert!(side_of("shell-a~side-k3", "shell-a"));
        assert!(!side_of("shell-a~side-", "shell-a"));
        assert!(!side_of("shell-b~side-1", "shell-a"));
        assert!(!side_of("shell-a", "shell-a"));
        assert!(!side_of("shell-a~side-1/../x", "shell-a"));
    }

    #[test]
    fn only_a_casual_chat_reply_counts_as_casual_and_only_in_its_own_chat() {
        let reads = Reads::default();
        assert!(!reads.casual_running("shell-a"));
        reads.0.lock().unwrap().insert(
            "shell-a".into(),
            ("how's your day going?".into(), TurnRoute::Chat),
        );
        reads.0.lock().unwrap().insert(
            "shell-b".into(),
            ("how's your day going?".into(), TurnRoute::Plan),
        );
        reads.0.lock().unwrap().insert(
            "shell-c".into(),
            ("fix the failing build in my repo".into(), TurnRoute::Chat),
        );
        assert!(reads.casual_running("shell-a"));
        assert!(!reads.casual_running("shell-b"));
        assert!(!reads.casual_running("shell-c"));
        assert!(!reads.casual_running("shell-d"));
    }
}
