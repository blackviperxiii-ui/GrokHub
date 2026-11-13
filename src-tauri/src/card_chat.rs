//! Workboard in-card chat and the queue, in this app (the cabin's parity):
//! Say sends a line to the card's agent in the card's own chat, Stop ends that
//! chat's turn, and a queued card starts on its own once nothing else runs.
//! The rules are `grokhub_core::work_chat`; this file keeps which chats belong
//! to cards, which of them run a turn now, and the queue's last word.
//!
//! A card's chat is an ordinary chat of this app (an id under
//! [`crate::history::PREFIX`]), so it is in History and its asks reach the
//! decision inbox. A card linked to a cabin chat gets a chat of its own here.
//! While the cabin is open it starts queued cards itself, so this queue waits.
//!
//! Token budget: the queue's look is local (the board file and a few flags);
//! a card's turn is the same one turn the person would have sent from Chat.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use grokhub_core::wire::PermissionMode;
use grokhub_core::work_chat::{self as wc, CardSay, QueueLook, QueueRunner};
use grokhub_core::work_file::{board_stamp, read_board};
use grokhub_core::work_resume::HOST_SHELL;
use grokhub_core::work_run as wr;
use grokhub_core::work_triage::{self as wt, QueueGate};
use serde::Serialize;

use crate::workboard::{self, Page};

/// Chats that belong to a card, the ones running a turn now, and the mode a
/// resumed card's next turn may not run looser than.
#[derive(Default)]
struct Chats {
    cards: HashSet<String>,
    running: HashSet<String>,
    kept: HashMap<String, PermissionMode>,
}

static CHATS: Mutex<Option<Chats>> = Mutex::new(None);
/// Why the next queued card waits, for the board's note.
static WAIT: Mutex<Option<String>> = Mutex::new(None);

fn with_chats<R>(f: impl FnOnce(&mut Chats) -> R) -> R {
    let mut guard = CHATS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Chats::default))
}

/// A chat this app can open and run.
pub fn ours(chat: &str) -> bool {
    chat.starts_with(crate::history::PREFIX)
}

/// The chat runs a turn now.
pub fn running(chat: &str) -> bool {
    with_chats(|c| c.running.contains(chat))
}

/// The queue's word for the board, while cards wait.
pub fn queue_wait() -> Option<String> {
    WAIT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn set_wait(why: Option<String>) {
    *WAIT.lock().unwrap_or_else(|e| e.into_inner()) = why;
}

pub(crate) fn note_card_chat(chat: &str) {
    with_chats(|c| {
        c.cards.insert(chat.to_string());
    });
}

/// A card resumed in `chat` keeps the mode its run started in: the chat's next
/// turn runs no looser than `mode`.
pub(crate) fn keep_mode(chat: &str, mode: PermissionMode) {
    with_chats(|c| {
        c.kept.insert(chat.to_string(), mode);
    });
}

/// `mode` for a turn in `chat`, no looser than a resumed card's kept mode.
pub fn turn_mode(chat: &str, mode: grokhub_agent::PermMode) -> grokhub_agent::PermMode {
    use grokhub_agent::PermMode as P;
    let rank = |m: P| match m {
        P::Ask => 0,
        P::Auto => 1,
        P::Always => 2,
    };
    let kept = match with_chats(|c| c.kept.get(chat).copied()) {
        Some(PermissionMode::Ask) => P::Ask,
        Some(PermissionMode::Auto) => P::Auto,
        Some(PermissionMode::AlwaysApprove) | None => return mode,
    };
    if rank(kept) < rank(mode) {
        kept
    } else {
        mode
    }
}

/// A turn starts in `chat`: a card's chat counts as running until it ends.
pub fn turn_began(chat: &str) {
    with_chats(|c| {
        if c.cards.contains(chat) {
            c.running.insert(chat.to_string());
        }
    });
}

/// A turn in `chat` ended with `stop`: a card's run settles on its card
/// (Done, or Blocked after a Stop or a failure) and the end is logged. True
/// when `chat` is a card's: its failed ask is not sent again on its own, since
/// the card already settled; Resume picks it up.
pub fn turn_ended(config_dir: &Path, chat: &str, stop: &Result<String, String>) -> bool {
    let ours = with_chats(|c| {
        c.running.remove(chat);
        c.kept.remove(chat);
        c.cards.contains(chat)
    });
    if !ours {
        return false;
    }
    let end = wc::turn_end(stop);
    let now = grokhub_core::now_ms();
    let settled = grokhub_core::work_file::update_board(&workboard::path(config_dir), |cards| {
        Ok(wc::settle_card_chat(cards, chat, &end, now))
    });
    if let Ok((Some(id), _)) = settled {
        let _ = grokhub_core::work_item::append_work_log(
            &wr::work_dir(config_dir),
            &id,
            now,
            wr::LOG_END,
            &end.log_text(),
        );
    }
    true
}

/// What Say sends, the chat to send it in, and the board after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Say {
    pub conversation_id: String,
    pub message: String,
    pub page: Page,
}

/// Say on card `id`: the card gets its chat (`new_chat` when it has none this
/// app can open) and the message to send there. The page sends it, so the
/// turn streams into the card like any chat.
pub fn say(config_dir: &Path, id: &str, text: &str, new_chat: &str) -> Result<Say, String> {
    if !ours(new_chat) {
        return Err("That chat isn't one of this app's chats.".into());
    }
    let (said, page) = workboard::change(config_dir, |cards| {
        wc::say_on_card(cards, id, text, new_chat, ours)
    })?;
    note_card_chat(&said.chat);
    Ok(Say {
        conversation_id: said.chat,
        message: said.message,
        page,
    })
}

/// Cards whose run in Doing is in a chat of this app, from a launch before:
/// their next turn still settles them.
pub fn seed(config_dir: &Path) {
    let Ok(cards) = read_board(&workboard::path(config_dir)) else {
        return;
    };
    with_chats(|c| {
        for card in cards.iter().filter(|card| card.run) {
            if let Some(chat) = card.thread_id.as_deref().filter(|t| ours(t)) {
                c.cards.insert(chat.to_string());
            }
        }
    });
}

/// The queue's state between looks.
#[derive(Default)]
pub struct Queue {
    runner: QueueRunner,
    /// The board's stamp when it last had nothing queued: until it changes
    /// there is nothing to read.
    idle_at: Option<u64>,
}

/// What the host tells one look.
pub struct Look<'a> {
    pub now: u64,
    /// Your chats, background runs and the game guard; `queued_live` and
    /// `agent_ready` are the queue's own to fill.
    pub gate: QueueGate,
    pub ready: &'a dyn Fn() -> bool,
    pub new_chat: &'a dyn Fn() -> String,
    pub mode: PermissionMode,
}

impl Queue {
    /// One look at the board. While the cabin is open it starts queued cards,
    /// so this does nothing. A start comes back as the chat and its first
    /// message, already on the card and counted as running; the host sends it.
    pub fn tick(&mut self, config_dir: &Path, look: Look<'_>) -> Option<CardSay> {
        if workboard::cabin_open(config_dir, look.now) {
            self.idle_at = None;
            set_wait(None);
            return None;
        }
        let path = workboard::path(config_dir);
        let stamp = board_stamp(&path);
        let live_busy = self.runner.live.as_deref().is_some_and(running);
        if self.idle_at == Some(stamp) && self.runner.live.is_none() {
            return None;
        }
        let cards = read_board(&path).ok()?;
        let outcome = self
            .runner
            .look(look.now, &cards, look.gate, live_busy, look.ready);
        let id = match outcome {
            QueueLook::Idle => {
                self.idle_at = Some(stamp);
                set_wait(None);
                return None;
            }
            QueueLook::Wait(why) => {
                self.idle_at = None;
                set_wait(Some(why));
                return None;
            }
            QueueLook::Start(id) => id,
        };
        self.idle_at = None;
        let new_chat = (look.new_chat)();
        let started = workboard::change(config_dir, |cards| {
            wc::start_queued_chat(cards, &id, &new_chat, ours, HOST_SHELL, look.mode, look.now)
        });
        match started {
            Ok((say, _)) => {
                with_chats(|c| {
                    c.cards.insert(say.chat.clone());
                    c.running.insert(say.chat.clone());
                });
                self.runner.started(&say.chat);
                set_wait(None);
                let _ = grokhub_core::work_item::append_work_log(
                    &wr::work_dir(config_dir),
                    &id,
                    look.now,
                    wr::LOG_START,
                    "started from the queue",
                );
                Some(say)
            }
            Err(why) => {
                self.runner.failed(look.now, &why);
                set_wait(Some(why));
                None
            }
        }
    }
}

/// A new chat id for a card, as the page makes them.
pub fn new_chat_id(now: u64) -> String {
    let salt: u32 = std::process::id() ^ (now as u32).rotate_left(7);
    format!("{}{:x}-q{:x}", crate::history::PREFIX, now, salt)
}

/// Any card waits in the queue (for the board's note).
pub fn any_queued(cards: &[grokhub_core::BoardCard]) -> bool {
    cards.iter().any(wt::is_queued)
}

#[cfg(test)]
pub fn clear_for_test() {
    *CHATS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    set_wait(None);
}

#[cfg(test)]
#[path = "card_chat_tests.rs"]
mod tests;
