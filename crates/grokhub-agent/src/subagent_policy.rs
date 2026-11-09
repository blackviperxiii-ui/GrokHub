//! Spawn policy (subagents card, A1 and A2): the model proposes a helper and
//! the harness decides. Helpers pay off for wide reading and for keeping big
//! output out of the chat; they cost several times the tokens and multiply
//! mistakes when they write in parallel. So a small turn gets none, a
//! background helper that writes in place runs alone, each chat runs at most a wave of four, a turn's
//! helpers share one token allowance, and every decision is logged with its
//! rule id so evals can tune the numbers.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::route::sizing::TaskSize;

/// Helpers one chat may have running at once (a wave).
pub const WAVE: usize = 4;
/// Helpers running at once across the whole app.
pub const APP_RUNNING: usize = 8;
/// Tokens all of one turn's helpers may use together.
pub const TURN_TOKENS: u64 = 400_000;
/// Model turns an explore helper gets.
pub const EXPLORE_TURNS: u32 = 20;
/// Model turns a general helper gets.
pub const GENERAL_TURNS: u32 = 40;
/// How long a foreground explore helper may run before it hands back what it has.
pub const EXPLORE_WALL: Duration = Duration::from_secs(5 * 60);
/// How long a foreground general helper may run before it hands back what it has.
pub const GENERAL_WALL: Duration = Duration::from_secs(15 * 60);

/// Helpers a turn of this size may start. An unsized turn gets a medium share.
pub fn turn_cap(size: Option<TaskSize>) -> u32 {
    match size {
        Some(TaskSize::Tiny) => 0,
        Some(TaskSize::Short) => 1,
        Some(TaskSize::Medium) | None => 4,
        Some(TaskSize::Long) => 10,
    }
}

/// Model turns a helper gets: its kind's cap, or the parent's when lower.
pub fn child_turns(parent_max: u32, explore: bool) -> u32 {
    let cap = if explore { EXPLORE_TURNS } else { GENERAL_TURNS };
    if parent_max == 0 {
        cap
    } else {
        parent_max.min(cap)
    }
}

/// How long a foreground helper may run.
pub fn wall(explore: bool) -> Duration {
    if explore {
        EXPLORE_WALL
    } else {
        GENERAL_WALL
    }
}

/// What the model asked for.
pub struct Ask<'a> {
    /// The root chat (a helper's helper counts against its chat).
    pub session: &'a str,
    pub explore: bool,
    pub background: bool,
    pub worktree: bool,
    /// The model running the parent turn.
    pub lead_model: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow(&'static str),
    Refuse { rule: &'static str, text: String },
}

/// What the policy reads besides the ask: the turn's size and what already runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Facts {
    pub size: Option<TaskSize>,
    pub spawned_this_turn: u32,
    pub tokens_this_turn: u64,
    pub running_here: usize,
    /// A background helper that writes in place runs in this chat. A
    /// foreground helper pauses whoever started it, so only background ones
    /// write alongside other work.
    pub writer_running_here: bool,
    pub running_in_app: usize,
}

/// The decision itself. Pure, so every rule has a test.
pub fn decide(ask: &Ask<'_>, facts: &Facts) -> Verdict {
    let refuse = |rule: &'static str, text: &str| Verdict::Refuse {
        rule,
        text: text.to_string(),
    };
    if crate::route::local::is_local(ask.lead_model) {
        return refuse(
            "lead-on-device",
            "Helpers don't run under an on-device lead model. Do this step yourself.",
        );
    }
    let cap = turn_cap(facts.size);
    if cap == 0 {
        return refuse(
            "small-turn",
            "This is a small task. Do it yourself instead of starting a helper.",
        );
    }
    if facts.size == Some(TaskSize::Short) && !ask.explore {
        return refuse(
            "short-turn-general",
            "This is a short task. Do it yourself; a short task gets at most one explore helper, \
             for wide reading.",
        );
    }
    if facts.spawned_this_turn >= cap {
        return Verdict::Refuse {
            rule: "turn-cap",
            text: format!(
                "This turn already started {cap} helper{}, its limit. Use their results or do \
                 the rest yourself.",
                if cap == 1 { "" } else { "s" }
            ),
        };
    }
    if facts.tokens_this_turn >= TURN_TOKENS {
        return refuse(
            "turn-budget",
            "This turn's helpers have used their share of tokens. Finish with what you have.",
        );
    }
    if facts.running_here >= WAVE {
        return refuse(
            "wave-full",
            "Four helpers are already running in this chat. Wait for one to finish, or do \
             this step yourself.",
        );
    }
    if facts.running_in_app >= APP_RUNNING {
        return refuse(
            "app-full",
            "The app is already running as many helpers as it allows. Do this step yourself, \
             or try again when one finishes.",
        );
    }
    let writer = !ask.explore && !ask.worktree;
    if writer && facts.writer_running_here {
        return refuse(
            "one-writer",
            "Another helper may be changing files here, and writes stay one at a time. Wait \
             for it, use an explore helper to read, or give this helper isolation worktree.",
        );
    }
    Verdict::Allow(match (ask.explore, ask.worktree, ask.background) {
        (true, _, false) => "explore",
        (true, _, true) => "explore-background",
        (false, true, _) => "general-worktree",
        (false, false, false) => "general",
        (false, false, true) => "general-background",
    })
}

#[derive(Default)]
struct Chat {
    turn: u64,
    spawned: u32,
    tokens: u64,
    /// Running helpers: ticket number and whether it writes in place in the
    /// background.
    running: Vec<(u64, bool)>,
}

static CHATS: Mutex<BTreeMap<String, Chat>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU64 = AtomicU64::new(1);
/// Chats remembered; a full map keeps only the ones with helpers running.
const CHATS_CAP: usize = 256;

/// A running helper's place in the policy's counts. Dropping it frees the place.
pub struct Ticket {
    session: String,
    turn: u64,
    number: u64,
}

impl Ticket {
    /// What the helper used, counted against the turn that started it.
    pub fn spent(&self, tokens: u64) {
        let mut chats = CHATS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(chat) = chats.get_mut(&self.session) {
            if chat.turn == self.turn {
                chat.tokens = chat.tokens.saturating_add(tokens);
            }
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut chats = CHATS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(chat) = chats.get_mut(&self.session) {
            chat.running.retain(|(n, _)| *n != self.number);
        }
    }
}

/// Decides the ask against the chat's live counts; on allow, takes a place
/// and returns its ticket. Either way the decision is logged.
pub fn admit(ask: &Ask<'_>) -> Result<Ticket, String> {
    let sized = sized(ask.session);
    let turn = sized.map_or(0, |(turn, _)| turn);
    let mut chats = CHATS.lock().unwrap_or_else(|e| e.into_inner());
    if chats.len() >= CHATS_CAP && !chats.contains_key(ask.session) {
        chats.retain(|_, chat| !chat.running.is_empty());
    }
    let running_in_app = chats.values().map(|chat| chat.running.len()).sum();
    let chat = chats.entry(ask.session.to_string()).or_default();
    if chat.turn != turn {
        chat.turn = turn;
        chat.spawned = 0;
        chat.tokens = 0;
    }
    let facts = Facts {
        size: sized.map(|(_, size)| size),
        spawned_this_turn: chat.spawned,
        tokens_this_turn: chat.tokens,
        running_here: chat.running.len(),
        writer_running_here: chat.running.iter().any(|(_, writer)| *writer),
        running_in_app,
    };
    let verdict = decide(ask, &facts);
    log(ask, &facts, &verdict);
    match verdict {
        Verdict::Refuse { text, .. } => Err(text),
        Verdict::Allow(_) => {
            let number = NEXT.fetch_add(1, Ordering::Relaxed);
            chat.spawned += 1;
            let background_writer = ask.background && !ask.explore && !ask.worktree;
            chat.running.push((number, background_writer));
            Ok(Ticket {
                session: ask.session.to_string(),
                turn,
                number,
            })
        }
    }
}

/// The router's size for the chat's latest turn.
fn sized(session: &str) -> Option<(u64, TaskSize)> {
    let latest = crate::route::sizing::latest(session);
    #[cfg(test)]
    if let Some(size) = PINNED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session)
    {
        return Some((latest.map_or(0, |(turn, _)| turn), *size));
    }
    latest
}

/// Tests that drive helpers with short made-up prompts pin their chat's size.
#[cfg(test)]
static PINNED: Mutex<BTreeMap<String, TaskSize>> = Mutex::new(BTreeMap::new());

#[cfg(test)]
pub fn pin_size(session: &str, size: TaskSize) {
    PINNED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(session.to_string(), size);
}

/// The spawn log keeps about this much before it starts over.
const LOG_BYTES: u64 = 512 * 1024;

/// Appends one line to `<config>/subagents/spawns.jsonl`: the rule, what was
/// asked and the counts. No prompt text. Never fails the spawn.
fn log(ask: &Ask<'_>, facts: &Facts, verdict: &Verdict) {
    // This crate's tests drive made-up chats: only a pinned scratch config logs.
    if cfg!(test) && !crate::perm::config_pinned() {
        return;
    }
    let (allowed, rule) = match verdict {
        Verdict::Allow(rule) => (true, *rule),
        Verdict::Refuse { rule, .. } => (false, *rule),
    };
    let line = serde_json::json!({
        "ts_ms": grokhub_core::now_ms(),
        "session": ask.session,
        "allowed": allowed,
        "rule": rule,
        "explore": ask.explore,
        "background": ask.background,
        "worktree": ask.worktree,
        "size": facts.size.map(|size| format!("{size:?}").to_ascii_lowercase()),
        "spawned_this_turn": facts.spawned_this_turn,
        "tokens_this_turn": facts.tokens_this_turn,
        "running_here": facts.running_here,
        "running_in_app": facts.running_in_app,
    });
    let _ = append(&crate::perm::config_dir().join("subagents"), &line);
}

fn append(dir: &Path, line: &serde_json::Value) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("spawns.jsonl");
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > LOG_BYTES) {
        let _ = std::fs::rename(&path, dir.join("spawns.old.jsonl"));
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(explore: bool) -> Ask<'static> {
        Ask {
            session: "chat",
            explore,
            background: false,
            worktree: false,
            lead_model: "grok-4.7",
        }
    }

    fn facts(size: TaskSize) -> Facts {
        Facts {
            size: Some(size),
            ..Facts::default()
        }
    }

    fn rule(verdict: Verdict) -> &'static str {
        match verdict {
            Verdict::Allow(rule) | Verdict::Refuse { rule, .. } => rule,
        }
    }

    #[test]
    fn small_and_short_turns_mostly_do_the_work_themselves() {
        assert_eq!(rule(decide(&ask(true), &facts(TaskSize::Tiny))), "small-turn");
        assert_eq!(
            rule(decide(&ask(false), &facts(TaskSize::Short))),
            "short-turn-general"
        );
        assert_eq!(rule(decide(&ask(true), &facts(TaskSize::Short))), "explore");
        let one_done = Facts {
            spawned_this_turn: 1,
            ..facts(TaskSize::Short)
        };
        assert_eq!(rule(decide(&ask(true), &one_done)), "turn-cap");
    }

    #[test]
    fn an_on_device_lead_never_delegates() {
        let local = Ask {
            lead_model: "local:gemma-26b",
            ..ask(true)
        };
        assert_eq!(rule(decide(&local, &facts(TaskSize::Long))), "lead-on-device");
    }

    #[test]
    fn caps_stop_the_fifth_running_and_the_eleventh_per_turn() {
        let wave = Facts {
            running_here: 4,
            spawned_this_turn: 4,
            ..facts(TaskSize::Long)
        };
        assert_eq!(rule(decide(&ask(true), &wave)), "wave-full");
        let three = Facts {
            running_here: 3,
            ..wave
        };
        assert_eq!(rule(decide(&ask(true), &three)), "explore");
        let ten = Facts {
            spawned_this_turn: 10,
            ..facts(TaskSize::Long)
        };
        assert_eq!(rule(decide(&ask(true), &ten)), "turn-cap");
        let medium = Facts {
            spawned_this_turn: 4,
            ..facts(TaskSize::Medium)
        };
        assert_eq!(rule(decide(&ask(true), &medium)), "turn-cap");
        let app = Facts {
            running_in_app: 8,
            ..facts(TaskSize::Long)
        };
        assert_eq!(rule(decide(&ask(true), &app)), "app-full");
    }

    #[test]
    fn a_turn_past_its_token_share_starts_no_more_helpers() {
        let spent = Facts {
            tokens_this_turn: 400_000,
            ..facts(TaskSize::Long)
        };
        assert_eq!(rule(decide(&ask(true), &spent)), "turn-budget");
    }

    #[test]
    fn writes_stay_one_at_a_time_unless_isolated() {
        let writing = Facts {
            running_here: 1,
            writer_running_here: true,
            ..facts(TaskSize::Long)
        };
        assert_eq!(rule(decide(&ask(false), &writing)), "one-writer");
        assert_eq!(rule(decide(&ask(true), &writing)), "explore");
        let isolated = Ask {
            worktree: true,
            ..ask(false)
        };
        assert_eq!(rule(decide(&isolated, &writing)), "general-worktree");
    }

    #[test]
    fn an_unsized_turn_gets_a_medium_share() {
        assert_eq!(turn_cap(None), 4);
        assert_eq!(rule(decide(&ask(false), &Facts::default())), "general");
        // A foreground writer doesn't block its own foreground helper.
        let session = "policy-nested-test";
        crate::route::sizing::note_latest(session, 5, TaskSize::Medium);
        let nested = Ask {
            session,
            ..ask(false)
        };
        let outer = admit(&nested).expect("outer helper");
        assert!(admit(&nested).is_ok());
        drop(outer);
    }

    #[test]
    fn helper_turns_and_time_follow_their_kind() {
        assert_eq!(child_turns(0, true), 20);
        assert_eq!(child_turns(0, false), 40);
        assert_eq!(child_turns(12, false), 12);
        assert_eq!(child_turns(90, true), 20);
        assert_eq!(wall(true), Duration::from_secs(300));
        assert_eq!(wall(false), Duration::from_secs(900));
    }

    #[test]
    fn admit_counts_running_helpers_and_frees_them_on_drop() {
        let session = "policy-admit-test";
        crate::route::sizing::note_latest(session, 77, TaskSize::Long);
        let general = Ask {
            session,
            background: true,
            ..ask(false)
        };
        let first = admit(&general).expect("first writer runs");
        assert_eq!(
            admit(&general).err().as_deref(),
            Some(
                "Another helper may be changing files here, and writes stay one at a time. Wait \
                 for it, use an explore helper to read, or give this helper isolation worktree."
            )
        );
        first.spent(399_999);
        drop(first);
        let second = admit(&general).expect("the writer finished");
        second.spent(1);
        drop(second);
        assert_eq!(
            admit(&general).err().as_deref(),
            Some("This turn's helpers have used their share of tokens. Finish with what you have.")
        );
        // A new turn starts its own count.
        crate::route::sizing::note_latest(session, 78, TaskSize::Long);
        assert!(admit(&general).is_ok());
    }
}
