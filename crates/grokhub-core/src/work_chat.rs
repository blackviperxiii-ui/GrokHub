//! A workboard card's own chat and the queue that starts queued cards, as
//! rules any host can run: Say (a line to the card's agent), the start of a
//! queued card as a turn in its own chat, the end of that turn on the card,
//! and the queue's look (gate, settle, retry). The cabin keeps its own copy
//! while it is retired; the shell runs these.
//!
//! Everything here edits the card list in memory; the caller saves it through
//! `work_file`, under the board lock.

use crate::game_guard::GameGuard;
use crate::wire::PermissionMode;
use crate::work_resume::claim_run;
use crate::work_run::{begin_run, end_run, RunEnd};
use crate::work_triage::{self as wt, end_rank, next_queued, queue_wait, QueueGate};
use crate::workboard::{
    card_notes_hash, card_work_prompt, resume_inflight_card, take_card_notes_block, BoardCard,
    BoardStatus, KanbanColumn,
};

/// How long the queue's gate must read clear before a queued card starts, so
/// a stale game reading never starts one mid-game.
pub const QUEUE_SETTLE_MS: u64 = 3_000;
/// After a start fails (no model, a refused run), wait this long to try again.
pub const QUEUE_RETRY_MS: u64 = 30_000;
/// The wait line while the gate settles.
pub const QUEUE_SETTLING: &str = "Starts in a moment.";
pub const NO_CARD: &str = "That card is gone. It may have been removed.";
pub const SAY_EMPTY: &str = "Type a message first.";

/// What Say sends, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardSay {
    /// The card's chat: its own when it has one this host can open, else the
    /// new one passed in.
    pub chat: String,
    /// The message for the chat, with the card and its notes the first time.
    pub message: String,
}

/// Say on card `id`. A card with no chat this host can open (`ours`) gets
/// `new_chat`, and its first message is the card's work prompt (plus what you
/// typed). A plain card becomes that chat's run card in Doing, so the turn's
/// end settles it. A run card a Stop left in Blocked goes back to Doing. Notes
/// changed since the chat last saw them ride along once.
pub fn say_on_card(
    cards: &mut [BoardCard],
    id: &str,
    text: &str,
    new_chat: &str,
    ours: impl Fn(&str) -> bool,
) -> Result<CardSay, String> {
    let text = text.trim();
    let i = cards.iter().position(|c| c.id == id).ok_or(NO_CARD)?;
    let linked = cards[i].thread_id.clone().filter(|t| ours(t));
    if linked.is_some() && text.is_empty() {
        return Err(SAY_EMPTY.into());
    }
    let chat = match linked.clone() {
        Some(chat) => chat,
        None => {
            let chat = new_chat.trim();
            if chat.is_empty() {
                return Err("No chat to send it to.".into());
            }
            chat.to_string()
        }
    };
    let mut message = if linked.is_some() {
        text.to_string()
    } else if text.is_empty() || cards[i].automation.is_some() {
        card_work_prompt(&cards[i])
    } else {
        format!("{}\n\n{text}", card_work_prompt(&cards[i]))
    };
    if linked.is_none() {
        let rank = end_rank(cards, KanbanColumn::Doing);
        let c = &mut cards[i];
        c.thread_id = Some(chat.clone());
        // The work prompt carries the notes, so they count as seen.
        c.notes_sent = card_notes_hash(&c.notes);
        if c.automation.is_none() {
            c.run = true;
            if !matches!(c.status, BoardStatus::Done | BoardStatus::InProgress) {
                c.status = BoardStatus::InProgress;
                c.rank = rank;
            }
        }
        c.queued_ms = 0;
    } else if cards[i].run && cards[i].status == BoardStatus::Blocked {
        resume_inflight_card(cards, id, &chat);
    }
    if let Some(block) = take_card_notes_block(cards, &chat) {
        message = format!("{block}\n\n{message}");
    }
    let c = &mut cards[i];
    c.notes_sent = card_notes_hash(&c.notes);
    c.fresh = false;
    Ok(CardSay { chat, message })
}

/// The queue starts card `id` in `chat` (its own chat when `ours`, else
/// `new_chat`): off the queue, into Doing as that chat's run card, the run
/// begun and claimed by `host`. Returns the chat and the first message.
pub fn start_queued_chat(
    cards: &mut [BoardCard],
    id: &str,
    new_chat: &str,
    ours: impl Fn(&str) -> bool,
    host: &str,
    mode: PermissionMode,
    now: u64,
) -> Result<CardSay, String> {
    if !wt::start_queued(cards, id) {
        return Err("That card is no longer queued.".into());
    }
    let c = cards.iter_mut().find(|c| c.id == id).ok_or(NO_CARD)?;
    let chat = c
        .thread_id
        .clone()
        .filter(|t| ours(t))
        .unwrap_or_else(|| new_chat.trim().to_string());
    if chat.is_empty() {
        return Err("No chat to start it in.".into());
    }
    let message = card_work_prompt(c);
    c.thread_id = Some(chat.clone());
    c.run = true;
    c.notes_sent = card_notes_hash(&c.notes);
    c.fresh = false;
    let kind = c.kind;
    begin_run(c, kind, &chat, "", now);
    claim_run(c, host, mode);
    Ok(CardSay { chat, message })
}

/// How a card chat's turn ended, from the engine's stop reason: a halt or a
/// cancel is a Stop, an error or `Err` a failure, anything else done.
pub fn turn_end(stop: &Result<String, String>) -> RunEnd {
    match stop {
        Ok(s) if s == "halted" || s == "cancelled" => RunEnd::Stopped,
        Ok(s) if s == "error" => RunEnd::Failed("the turn hit an error".into()),
        Ok(_) => RunEnd::Done,
        Err(why) => RunEnd::Failed(why.clone()),
    }
}

/// The turn in `chat` ended: its run card in Doing settles. Done moves to
/// Done (the sweep archives it later); a Stop or a failure goes to Blocked
/// with its checkpoint, ready to Resume or Say again. The card's id when one
/// settled.
pub fn settle_card_chat(
    cards: &mut [BoardCard],
    chat: &str,
    end: &RunEnd,
    now: u64,
) -> Option<String> {
    let chat = chat.trim();
    if chat.is_empty() {
        return None;
    }
    let c = cards.iter_mut().rev().find(|c| {
        c.run && c.thread_id.as_deref() == Some(chat) && c.status == BoardStatus::InProgress
    })?;
    c.clear_inflight_undo();
    end_run(c, end, now);
    c.updated_ms = now;
    c.status = match end {
        RunEnd::Done => BoardStatus::Done,
        RunEnd::Stopped | RunEnd::Failed(_) => BoardStatus::Blocked,
    };
    Some(c.id.clone())
}

/// The game guard's word for the queue: wait while an anti-cheat or a game
/// runs. Nothing else in the guard holds the queue back.
pub fn queue_game_wait(g: &GameGuard) -> Option<String> {
    if let Some(ac) = g.anticheat {
        return Some(format!(
            "Queued work waits while {ac} is running so it doesn't flag this PC."
        ));
    }
    g.gaming
        .then(|| "Queued work waits while you're gaming.".to_string())
}

/// What one look at the queue decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueLook {
    /// Nothing is queued.
    Idle,
    /// The next card waits, and why (shown on the board).
    Wait(String),
    /// Start this card now.
    Start(String),
}

/// The queue's memory between looks: the chat of the card it started, when
/// the gate first read clear, and a retry pause after a failed start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueRunner {
    /// The chat the queue's last start runs in, while it runs.
    pub live: Option<String>,
    clear_since: Option<u64>,
    retry_at: u64,
    /// Why the next queued card waits; `None` when nothing waits.
    pub wait: Option<String>,
}

impl QueueRunner {
    /// One look. `gate` is everything but the queue's own live run, which
    /// `live_busy` says (the chat it started is still running a turn).
    /// `ready` is asked last, only when everything else reads clear.
    pub fn look(
        &mut self,
        now: u64,
        cards: &[BoardCard],
        mut gate: QueueGate,
        live_busy: bool,
        ready: impl FnOnce() -> bool,
    ) -> QueueLook {
        if !live_busy {
            self.live = None;
        }
        let Some(card) = next_queued(cards) else {
            self.wait = None;
            self.clear_since = None;
            return QueueLook::Idle;
        };
        if now < self.retry_at {
            return self.waiting(self.wait.clone().unwrap_or_default());
        }
        gate.queued_live = self.live.is_some();
        gate.agent_ready = true;
        let why = queue_wait(&gate).or_else(|| {
            (!ready()).then(|| {
                queue_wait(&QueueGate {
                    agent_ready: false,
                    ..QueueGate::default()
                })
                .unwrap_or_default()
            })
        });
        if let Some(why) = why {
            self.clear_since = None;
            return self.waiting(why);
        }
        let since = *self.clear_since.get_or_insert(now);
        if now.saturating_sub(since) < QUEUE_SETTLE_MS {
            return self.waiting(QUEUE_SETTLING.into());
        }
        self.clear_since = None;
        self.wait = None;
        QueueLook::Start(card.id.clone())
    }

    fn waiting(&mut self, why: String) -> QueueLook {
        self.wait = Some(why.clone());
        QueueLook::Wait(why)
    }

    /// The card started in `chat`.
    pub fn started(&mut self, chat: &str) {
        self.live = Some(chat.to_string());
        self.wait = None;
    }

    /// The start failed: say why and look again after [`QUEUE_RETRY_MS`].
    pub fn failed(&mut self, now: u64, why: &str) {
        self.retry_at = now + QUEUE_RETRY_MS;
        self.wait = Some(why.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_resume::HOST_SHELL;
    use crate::workboard::{BoardCard, BoardStatus};

    fn todo(id: &str, title: &str) -> BoardCard {
        let mut c = BoardCard::new(title, "", "");
        c.id = id.into();
        c.status = BoardStatus::Todo;
        c
    }

    fn shell(t: &str) -> bool {
        t.starts_with("shell-")
    }

    #[test]
    fn say_on_a_new_card_links_a_chat_and_sends_the_card_first() {
        let mut cards = vec![todo("a", "Fix the printer")];
        cards[0].notes = "Use the USB port".into();
        let say =
            say_on_card(&mut cards, "a", "start with the driver", "shell-1", shell).expect("say");
        assert_eq!(say.chat, "shell-1");
        assert_eq!(
            say.message,
            "Work on this workboard card: Fix the printer\n\nMy notes for this card (follow \
             them):\nUse the USB port\n\nWhen it is done, say so in one line.\n\nstart with the \
             driver"
        );
        let c = &cards[0];
        assert_eq!(c.thread_id.as_deref(), Some("shell-1"));
        assert!(c.run);
        assert_eq!(c.status, BoardStatus::InProgress);
        assert!(!c.fresh);
    }

    #[test]
    fn say_on_a_linked_card_sends_only_what_you_typed_and_new_notes_once() {
        let mut cards = vec![todo("a", "Fix the printer")];
        say_on_card(&mut cards, "a", "", "shell-1", shell).expect("first");
        assert_eq!(
            say_on_card(&mut cards, "a", "  ", "shell-2", shell),
            Err(SAY_EMPTY.to_string())
        );
        let next = say_on_card(&mut cards, "a", "how is it going?", "shell-2", shell).expect("say");
        assert_eq!(next.chat, "shell-1");
        assert_eq!(next.message, "how is it going?");
        cards[0].notes = "Try the other tray".into();
        let noted = say_on_card(&mut cards, "a", "go on", "shell-2", shell).expect("noted");
        assert_eq!(
            noted.message,
            "Notes on workboard card \"Fix the printer\":\nTry the other tray\n\nUse these notes \
             for this work.\n\ngo on"
        );
        let again = say_on_card(&mut cards, "a", "and?", "shell-2", shell).expect("again");
        assert_eq!(again.message, "and?");
    }

    #[test]
    fn say_on_a_card_linked_to_a_chat_this_app_cannot_open_starts_its_own() {
        let mut cards = vec![todo("a", "Old card")];
        cards[0].thread_id = Some("cabin-thread".into());
        let say = say_on_card(&mut cards, "a", "", "shell-9", shell).expect("say");
        assert_eq!(say.chat, "shell-9");
        assert_eq!(
            say.message,
            "Work on this workboard card: Old card\n\nWhen it is done, say so in one line."
        );
        assert_eq!(
            say_on_card(&mut cards, "gone", "hi", "shell-9", shell),
            Err(NO_CARD.to_string())
        );
    }

    #[test]
    fn a_stop_blocks_the_card_and_the_next_say_brings_it_back_to_doing() {
        let mut cards = vec![todo("a", "Fix the printer")];
        say_on_card(&mut cards, "a", "", "shell-1", shell).expect("say");
        let end = turn_end(&Ok("cancelled".into()));
        assert_eq!(end, RunEnd::Stopped);
        assert_eq!(
            settle_card_chat(&mut cards, "shell-1", &end, 5),
            Some("a".to_string())
        );
        assert_eq!(cards[0].status, BoardStatus::Blocked);
        assert_eq!(
            cards[0].checkpoint.as_ref().map(|c| c.blocker.as_str()),
            Some(crate::work_run::STOPPED_BLOCKER)
        );
        say_on_card(&mut cards, "a", "carry on", "shell-2", shell).expect("again");
        assert_eq!(cards[0].status, BoardStatus::InProgress);
        let done = turn_end(&Ok("end_turn".into()));
        assert_eq!(
            settle_card_chat(&mut cards, "shell-1", &done, 9),
            Some("a".into())
        );
        assert_eq!(cards[0].status, BoardStatus::Done);
        assert_eq!(settle_card_chat(&mut cards, "shell-1", &done, 9), None);
        assert_eq!(
            turn_end(&Err("no model".into())),
            RunEnd::Failed("no model".into())
        );
    }

    #[test]
    fn a_queued_card_starts_in_its_own_chat_as_a_run() {
        let mut cards = vec![todo("a", "Clean the cache")];
        wt::queue_card(&mut cards, "a", 10).expect("queue");
        let start = start_queued_chat(
            &mut cards,
            "a",
            "shell-q",
            shell,
            HOST_SHELL,
            PermissionMode::Ask,
            20,
        )
        .expect("start");
        assert_eq!(start.chat, "shell-q");
        assert_eq!(
            start.message,
            "Work on this workboard card: Clean the cache\n\nWhen it is done, say so in one line."
        );
        let c = &cards[0];
        assert_eq!(c.status, BoardStatus::InProgress);
        assert_eq!(c.queued_ms, 0);
        assert!(c.run);
        assert_eq!(c.run_state.host, "shell");
        assert_eq!(c.checkpoint.as_ref().map(|c| c.attempt), Some(1));
        assert_eq!(
            start_queued_chat(
                &mut cards,
                "a",
                "shell-q",
                shell,
                HOST_SHELL,
                PermissionMode::Ask,
                30
            ),
            Err("That card is no longer queued.".into())
        );
    }

    #[test]
    fn the_queue_waits_settles_then_starts_one_at_a_time() {
        let mut cards = vec![todo("a", "A"), todo("b", "B")];
        wt::queue_card(&mut cards, "a", 1).expect("a");
        wt::queue_card(&mut cards, "b", 2).expect("b");
        let mut q = QueueRunner::default();
        let busy = QueueGate {
            foreground: true,
            ..QueueGate::default()
        };
        assert_eq!(
            q.look(0, &cards, busy, false, || true),
            QueueLook::Wait("Waits for the open chat to finish.".into())
        );
        assert_eq!(
            q.look(1_000, &cards, QueueGate::default(), false, || false),
            QueueLook::Wait("Waits for the agent to be ready.".into())
        );
        assert_eq!(
            q.look(2_000, &cards, QueueGate::default(), false, || true),
            QueueLook::Wait(QUEUE_SETTLING.into())
        );
        assert_eq!(
            q.look(5_000, &cards, QueueGate::default(), false, || true),
            QueueLook::Start("a".into())
        );
        q.started("shell-a");
        wt::start_queued(&mut cards, "a");
        assert_eq!(
            q.look(6_000, &cards, QueueGate::default(), true, || true),
            QueueLook::Wait("A queued card is running. The next one waits for it.".into())
        );
        // The run ended: b settles, then starts.
        assert_eq!(
            q.look(7_000, &cards, QueueGate::default(), false, || true),
            QueueLook::Wait(QUEUE_SETTLING.into())
        );
        assert_eq!(
            q.look(10_000, &cards, QueueGate::default(), false, || true),
            QueueLook::Start("b".into())
        );
        q.failed(10_000, "No model");
        assert_eq!(
            q.look(20_000, &cards, QueueGate::default(), false, || true),
            QueueLook::Wait("No model".into())
        );
        assert_eq!(
            q.look(41_000, &cards, QueueGate::default(), false, || true),
            QueueLook::Wait(QUEUE_SETTLING.into())
        );
        wt::unqueue_card(&mut cards, "b");
        assert_eq!(
            q.look(42_000, &cards, QueueGate::default(), false, || true),
            QueueLook::Idle
        );
        assert_eq!(q.wait, None);
    }

    #[test]
    fn a_game_holds_the_queue() {
        let g = GameGuard {
            anticheat: Some("Vanguard"),
            gaming: true,
            idle_anticheat: None,
        };
        assert_eq!(
            queue_game_wait(&g).as_deref(),
            Some("Queued work waits while Vanguard is running so it doesn't flag this PC.")
        );
        let playing = GameGuard {
            anticheat: None,
            gaming: true,
            idle_anticheat: None,
        };
        assert_eq!(
            queue_game_wait(&playing).as_deref(),
            Some("Queued work waits while you're gaming.")
        );
        assert_eq!(queue_game_wait(&GameGuard::default()), None);
    }
}
