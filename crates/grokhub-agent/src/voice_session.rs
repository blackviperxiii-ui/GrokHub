//! Per-chat voice state: which voice speaks this turn (dynamic chat), whether
//! the reply gets a quip slot, and the chat's heat. The cabin and the shell
//! register each chat's voice settings; the Brief step ([`step`]) works out the
//! turn and leaves one note for the turn's volatile block ([`turn_block`]), so
//! the cached system prefix never changes from turn to turn.
//!
//! Background runs never move the voice or quip.

use std::collections::HashMap;
use std::sync::Mutex;

use grokhub_core::conversation::{self, Tell};
use grokhub_core::personality::{Heat, Personality};
use grokhub_core::quips::{self, Moment, Planner, Trigger};
use grokhub_core::voice_flow::{self, Flow, Signals};

/// A chat's voice settings, as its app publishes them.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceConfig {
    /// The chat's own voice (its system prompt already carries it).
    pub home: Personality,
    /// Voices dynamic chat may hand the conversation to (the visible presets).
    pub roster: Vec<Personality>,
    /// Dynamic chat on (Settings → Personality).
    pub dynamic: bool,
    /// The chat's voice was picked by hand for this chat: it never moves.
    pub pinned: bool,
    /// Adults-only on, at this heat; `None` with it off.
    pub adult: Option<Heat>,
    /// Minutes east of UTC on this computer, for late-night quips.
    pub utc_offset_min: i32,
}

impl VoiceConfig {
    /// Local hour (0-23) at `now_ms`.
    pub fn local_hour(&self, now_ms: u64) -> u8 {
        let min = (now_ms / 60_000) as i64 + i64::from(self.utc_offset_min);
        (min.rem_euclid(1440) / 60) as u8
    }
}

struct Session {
    cfg: VoiceConfig,
    flow: Flow,
    quips: Planner,
    heat: Option<Heat>,
    /// "Talk like a pirate" style asks pin the voice for the rest of the chat.
    asked_style: bool,
}

/// Chats kept at once; the oldest registration goes first past this.
const CAP: usize = 64;

static SESSIONS: Mutex<Vec<(String, Session)>> = Mutex::new(Vec::new());
static TURN: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Per chat, for the conversation core: whether this turn's ask was casual,
/// what the last finished reply slipped on ([`note_reply`]), and whether the
/// last turn came up empty.
#[derive(Debug, Default)]
struct Talk {
    casual: bool,
    tells: Vec<Tell>,
    /// The last finished turn found nothing for the ask.
    empty: bool,
    /// This turn follows one that came up empty (set by [`step`]).
    after_empty: bool,
}

static TALK: Mutex<Option<HashMap<String, Talk>>> = Mutex::new(None);

/// The last reply's tells, taken (each is named once), and this turn's shape
/// kept. Also whether the last turn came up empty.
fn take_tells(conversation_id: &str, casual: bool) -> (Vec<Tell>, bool) {
    let mut talk = TALK.lock().unwrap_or_else(|e| e.into_inner());
    let m = talk.get_or_insert_with(HashMap::new);
    if m.len() >= CAP && !m.contains_key(conversation_id) {
        m.clear();
    }
    let entry = m.entry(conversation_id.to_string()).or_default();
    entry.casual = casual;
    entry.after_empty = entry.empty;
    (std::mem::take(&mut entry.tells), entry.after_empty)
}

/// Reads a chat's finished reply for the conversation core's tells; the next
/// turn's note names them. `came_up_empty` is whether the turn found nothing
/// for the ask; a turn that did clears it. Never edits the reply, never calls
/// a model. Only a chat whose turn [`step`] saw in the foreground is read.
pub fn note_reply(conversation_id: &str, reply: &str, came_up_empty: bool) {
    let mut talk = TALK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = talk.as_mut().and_then(|m| m.get_mut(conversation_id)) {
        entry.tells = conversation::tells(reply, entry.casual);
        if entry.after_empty && conversation::takes_credit(reply) {
            entry.tells.push(Tell::TakesCredit);
        }
        entry.empty = came_up_empty;
    }
}

/// This turn follows one that came up empty, so thanks for a result are off.
pub fn after_empty(conversation_id: &str) -> bool {
    TALK.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(conversation_id))
        .is_some_and(|t| t.after_empty)
}

/// What the chat's last finished reply slipped on, not yet named in a note.
pub fn pending_tells(conversation_id: &str) -> Vec<Tell> {
    TALK.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(conversation_id))
        .map(|t| t.tells.clone())
        .unwrap_or_default()
}

fn seed_of(id: &str) -> u64 {
    // FNV-1a: stable across runs, no dependency.
    id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// Registers or updates a chat's voice settings. Flow and quip timing carry
/// over while the home voice stays the same; a new home voice starts fresh.
pub fn register(conversation_id: &str, cfg: VoiceConfig) {
    if conversation_id.is_empty() {
        return;
    }
    let mut all = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, s)) = all.iter_mut().find(|(id, _)| id == conversation_id) {
        if s.cfg.home.id != cfg.home.id {
            s.flow = Flow::new(&cfg.home.id);
            s.asked_style = false;
        }
        if s.cfg.adult != cfg.adult {
            s.heat = cfg.adult;
        }
        s.cfg = cfg;
        return;
    }
    if all.len() >= CAP {
        all.remove(0);
    }
    let session = Session {
        flow: Flow::new(&cfg.home.id),
        quips: Planner::new(seed_of(conversation_id)),
        heat: cfg.adult,
        asked_style: false,
        cfg,
    };
    all.push((conversation_id.to_string(), session));
}

/// Drops a chat's voice state (the chat closed or was deleted).
pub fn forget(conversation_id: &str) {
    SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(id, _)| id != conversation_id);
    if let Some(m) = TURN.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        m.remove(conversation_id);
    }
    if let Some(m) = TALK.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        m.remove(conversation_id);
    }
}

/// The voice speaking in a chat right now, for the chat's chip and avatar.
pub fn active_voice(conversation_id: &str) -> Option<String> {
    SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(id, _)| id == conversation_id)
        .map(|(_, s)| s.flow.voice().to_string())
}

/// The chat's heat now ("roast me harder" moves it inside the dial).
pub fn heat(conversation_id: &str) -> Option<Heat> {
    SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(id, _)| id == conversation_id)
        .and_then(|(_, s)| s.heat)
}

/// What the Brief says about the turn, in plain values.
#[derive(Debug, Clone, Copy)]
pub struct TurnView<'a> {
    pub text: &'a str,
    /// The Brief's intent (`chat`, `question`, `task`, `repair`, ...).
    pub intent: &'a str,
    /// The Brief's care (`casual`, `careful`, `critical`).
    pub care: &'a str,
    /// The Brief's mood (`calm`, `urgent`, `frustrated`, `low`).
    pub mood: &'a str,
    pub background: bool,
    /// Local hour, 0-23; `None` reads it from the chat's clock.
    pub hour: Option<u8>,
}

/// "Talk like a pirate", "be more like Gandalf": the user picked a style
/// in words, so the voice holds still.
fn asks_for_a_style(text: &str) -> bool {
    let low = text.to_lowercase();
    [
        "talk like",
        "speak like",
        "act like",
        "pretend to be",
        "be more like",
        "sound like",
    ]
    .iter()
    .any(|w| low.contains(w))
}

/// Steps the chat's voice for one user turn and stores the turn's note for
/// [`turn_block`]. Returns the note (empty for a background run). Every
/// foreground chat gets the conversation core's cue, registered or not, so
/// the core holds in both shells; a registered chat's voice lines come first.
pub fn step(conversation_id: &str, view: TurnView) -> String {
    let block = if view.background || conversation_id.is_empty() {
        String::new()
    } else {
        let talk = conversation::Turn {
            text: view.text,
            intent: view.intent,
            care: view.care,
            mood: view.mood,
        };
        let (last, after_empty) = take_tells(conversation_id, talk.casual());
        let mut cue = conversation::cue(&talk, &last);
        if after_empty {
            cue.push(' ');
            cue.push_str(conversation::CAME_UP_EMPTY);
        }
        let mut all = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
        let voice = match all.iter_mut().find(|(id, _)| id == conversation_id) {
            Some((_, s)) => s.step(view),
            None => String::new(),
        };
        if voice.is_empty() {
            cue
        } else {
            format!("{voice}\n{cue}")
        }
    };
    let mut turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let m = turn.get_or_insert_with(HashMap::new);
    if block.is_empty() {
        m.remove(conversation_id);
    } else {
        if m.len() >= CAP && !m.contains_key(conversation_id) {
            m.clear();
        }
        m.insert(conversation_id.to_string(), block.clone());
    }
    block
}

/// The note [`step`] left for this chat's current turn.
pub fn turn_block(conversation_id: &str) -> Option<String> {
    TURN.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(conversation_id).cloned())
}

impl Session {
    fn step(&mut self, view: TurnView) -> String {
        let hour = view
            .hour
            .unwrap_or_else(|| self.cfg.local_hour(grokhub_core::now_ms()));
        let mut lines: Vec<String> = Vec::new();
        // Heat moves only inside the dial, and only with adults-only on.
        if let (Some(heat), Some(nudge)) = (self.heat, quips::nudge(view.text)) {
            let next = match nudge {
                quips::Nudge::Hotter => heat.hotter(),
                quips::Nudge::Milder => heat.milder(),
            };
            if next != heat {
                self.heat = Some(next);
                lines.push(format!(
                    "Heat for this chat is now {}.",
                    next.label().to_lowercase()
                ));
            }
        }
        if asks_for_a_style(view.text) {
            self.asked_style = true;
        }
        let low_mood = view.mood == "low";
        let frustrated = view.mood == "frustrated";
        let signals = Signals {
            text: view.text,
            repair: view.intent == "repair",
            task: matches!(view.intent, "task" | "research" | "repair"),
            question: view.intent == "question",
            creative: view.intent == "creative",
            low_mood,
            frustrated,
            adult: self.heat.is_some()
                && grokhub_core::adult_guard::check(view.text)
                    == grokhub_core::adult_guard::Risk::Explicit,
            hour,
        };
        let pinned = self.cfg.pinned || !self.cfg.dynamic || self.asked_style;
        let moved = self.flow.step(&signals, &self.cfg.roster, pinned);
        let home = moved.voice == self.cfg.home.id;
        let voice = if home {
            self.cfg.home.clone()
        } else {
            self.cfg
                .roster
                .iter()
                .find(|p| p.id == moved.voice)
                .cloned()
                .unwrap_or_else(|| self.cfg.home.clone())
        };
        if !home {
            lines.push(format!(
                "For this part of the chat speak as {} instead of {}. {}",
                voice.name.trim(),
                self.cfg.home.name.trim(),
                voice.prompt_block_for(self.heat)
            ));
        } else if let Some(h) = self.heat.filter(|h| Some(*h) != self.cfg.adult) {
            // Moved in chat off the setting the system prompt carries: this
            // turn's note has the heat now. Heat alone never tags a chat 18+.
            lines.push(h.line().to_string());
        }
        if moved.switched {
            lines.push(voice_flow::handoff_line(voice.name.trim()));
        }
        lines.push(voice_flow::depth_note(&voice));
        let serious = view.intent == "repair"
            || view.care == "critical"
            || low_mood
            || frustrated
            || view.mood == "urgent";
        let moment = Moment {
            serious,
            casual: view.intent == "chat",
            trigger: Trigger::detect(view.text, hour),
            landed: quips::landed(view.text),
        };
        if let Some(slot) = self
            .quips
            .step(moment, self.heat.unwrap_or(Heat::Clean), voice.roast)
        {
            lines.push(slot.note());
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::personality::{builtin, builtins};

    fn cfg(home: &str, adult: Option<Heat>) -> VoiceConfig {
        VoiceConfig {
            home: builtin(home),
            roster: builtins().into_iter().filter(|p| !p.explicit).collect(),
            dynamic: true,
            pinned: false,
            adult,
            utc_offset_min: 0,
        }
    }

    fn view<'a>(text: &'a str, intent: &'a str, mood: &'a str) -> TurnView<'a> {
        TurnView {
            text,
            intent,
            care: "casual",
            mood,
            background: false,
            hour: Some(20),
        }
    }

    #[test]
    fn an_unregistered_chat_gets_only_the_talk_cue_and_a_background_run_no_note() {
        assert_eq!(turn_block("vs-none"), None);
        assert_eq!(
            step("vs-none", view("hey", "chat", "calm")),
            grokhub_core::conversation::SHORT
        );
        assert_eq!(
            turn_block("vs-none").as_deref(),
            Some(grokhub_core::conversation::SHORT)
        );
        forget("vs-none");
        assert_eq!(turn_block("vs-none"), None);
        register("vs-bg", cfg("nyx", None));
        let mut bg = view("still broken error crash", "repair", "frustrated");
        bg.background = true;
        assert_eq!(step("vs-bg", bg), "");
        forget("vs-bg");
    }

    #[test]
    fn every_turn_carries_the_depth_note_and_a_switch_brings_the_guest_voice() {
        let id = "vs-flow";
        register(id, cfg("nyx", None));
        let first = step(id, view("lol what's up", "chat", "calm"));
        assert!(first.contains("Stay in voice:"), "{first}");
        assert_eq!(turn_block(id).as_deref(), Some(first.as_str()));
        step(id, view("game crashed with an error", "repair", "calm"));
        let fix = step(
            id,
            view("still broken, driver error again", "repair", "calm"),
        );
        assert_eq!(active_voice(id).as_deref(), Some("chief-engineer"));
        assert!(fix.contains("speak as Chief Engineer instead of"), "{fix}");
        assert!(fix.contains("no announcement"), "{fix}");
        assert!(!fix.contains("Quip slot"), "no jokes while fixing: {fix}");
        forget(id);
        assert_eq!(active_voice(id), None);
    }

    #[test]
    fn a_pin_or_dynamic_off_or_a_style_ask_holds_the_voice() {
        for (id, pinned, dynamic, text) in [
            ("vs-pin", true, true, "error crash broken"),
            ("vs-off", false, false, "error crash broken"),
            (
                "vs-style",
                false,
                true,
                "talk like a pirate, the build has an error",
            ),
        ] {
            let mut c = cfg("nyx", None);
            c.pinned = pinned;
            c.dynamic = dynamic;
            register(id, c);
            for _ in 0..5 {
                step(id, view(text, "repair", "calm"));
            }
            assert_eq!(active_voice(id).as_deref(), Some("nyx"), "{id}");
            forget(id);
        }
    }

    #[test]
    fn roast_me_harder_moves_heat_inside_the_dial_only_with_adults_only_on() {
        let id = "vs-heat";
        register(id, cfg("nyx", Some(Heat::Cheeky)));
        let note = step(id, view("roast me harder", "chat", "calm"));
        assert_eq!(heat(id), Some(Heat::Spicy));
        assert!(note.contains("Heat for this chat is now spicy."), "{note}");
        assert!(note.contains(Heat::Spicy.line()), "{note}");
        assert!(
            !note.contains(grokhub_core::adult_guard::ADULT_VOICE),
            "heat alone never tags a chat 18+: {note}"
        );
        step(id, view("go harder", "chat", "calm"));
        step(id, view("go harder", "chat", "calm"));
        assert_eq!(heat(id), Some(Heat::Filthy));
        step(id, view("ok ease off", "chat", "calm"));
        assert_eq!(heat(id), Some(Heat::Spicy));
        forget(id);

        let id = "vs-heat-off";
        register(id, cfg("nyx", None));
        let note = step(id, view("roast me harder", "chat", "calm"));
        assert_eq!(heat(id), None);
        assert!(!note.contains("Heat for this chat"));
        assert!(!note.contains(grokhub_core::adult_guard::ADULT_VOICE));
        forget(id);
    }

    #[test]
    fn quips_come_in_casual_chat_and_never_on_a_frustrated_turn() {
        let id = "vs-quips";
        register(id, cfg("nyx", Some(Heat::Spicy)));
        let mut slots = 0;
        for _ in 0..20 {
            if step(id, view("lol we died again", "chat", "calm")).contains("Quip slot") {
                slots += 1;
            }
        }
        assert!((2..=5).contains(&slots), "{slots}");
        for _ in 0..20 {
            let note = step(id, view("ugh this keeps failing", "chat", "frustrated"));
            assert!(!note.contains("Quip slot"), "{note}");
        }
        forget(id);
    }

    #[test]
    fn a_new_home_voice_starts_the_flow_fresh() {
        let id = "vs-home";
        register(id, cfg("nyx", None));
        step(id, view("error", "repair", "calm"));
        step(id, view("error again", "repair", "calm"));
        assert_eq!(active_voice(id).as_deref(), Some("chief-engineer"));
        register(id, cfg("pirate", None));
        assert_eq!(active_voice(id).as_deref(), Some("pirate"));
        forget(id);
    }

    #[test]
    fn the_local_hour_follows_the_offset() {
        let mut c = cfg("nyx", None);
        // 2026-01-01 00:30 UTC.
        let now = 1_767_227_400_000;
        assert_eq!(c.local_hour(now), 0);
        c.utc_offset_min = -6 * 60;
        assert_eq!(c.local_hour(now), 18);
        c.utc_offset_min = 5 * 60 + 30;
        assert_eq!(c.local_hour(now), 6);
    }

    #[test]
    fn an_18_plus_turn_with_adults_only_on_hands_off_to_the_adults_only_voice() {
        let with_adult = |adult: Option<Heat>| VoiceConfig {
            roster: builtins(),
            ..cfg("nyx", adult)
        };
        let id = "vs-adult";
        register(id, with_adult(Some(Heat::Cheeky)));
        step(id, view("hey", "chat", "calm"));
        let note = step(id, view("talk dirty to me, I'm horny", "chat", "calm"));
        assert_eq!(active_voice(id).as_deref(), Some("whore"));
        assert!(note.contains("speak as Whore instead of Nyx"), "{note}");
        // The guest note tags the turn 18+, so it runs on the local model.
        let turn = crate::prefix_cache::user_message_text(&note, "hey");
        assert!(grokhub_core::adult_guard::explicit_conversation([(
            "user",
            turn.as_str()
        )]));
        forget(id);
        // Adults-only off: no hand-off.
        let id = "vs-adult-off";
        register(id, with_adult(None));
        step(id, view("talk dirty to me, I'm horny", "chat", "calm"));
        assert_eq!(active_voice(id).as_deref(), Some("nyx"));
        forget(id);
    }

    #[test]
    fn a_chat_turned_up_in_chat_still_routes_anywhere() {
        let id = "vs-route";
        register(id, cfg("nyx", Some(Heat::Cheeky)));
        for text in ["hey", "roast me harder", "go harder"] {
            let note = step(id, view(text, "chat", "calm"));
            let turn = crate::prefix_cache::user_message_text(&note, text);
            assert!(
                !grokhub_core::adult_guard::explicit_conversation([("user", turn.as_str())]),
                "{text}: heat alone keeps the chat on its usual model"
            );
        }
        assert_eq!(heat(id), Some(Heat::Filthy));
        forget(id);
    }

    #[test]
    fn a_slipped_reply_is_named_once_in_the_next_turns_note() {
        let id = "vs-talk";
        register(id, cfg("nyx", None));
        let first = step(id, view("yo", "chat", "calm"));
        assert!(
            first.ends_with(grokhub_core::conversation::SHORT),
            "{first}"
        );
        note_reply(id, "Certainly! Task complete.", false);
        assert_eq!(
            pending_tells(id),
            vec![Tell::StockOpener, Tell::ProcessTalk]
        );
        let next = step(id, view("cool, thanks", "chat", "calm"));
        assert!(next.contains("Stay in voice:"), "{next}");
        assert!(
            next.ends_with("Your last reply slipped: it opened with a stock phrase, so start with the answer; it narrated checklists, tools, cards or limits, so say what happened in plain words."),
            "{next}"
        );
        assert_eq!(pending_tells(id), Vec::<Tell>::new());
        let third = step(id, view("nice", "chat", "calm"));
        assert!(!third.contains("slipped"), "named once: {third}");
        // A reply to a chat no foreground turn saw is not read.
        note_reply("vs-never-stepped", "Certainly!", true);
        assert_eq!(pending_tells("vs-never-stepped"), Vec::<Tell>::new());
        forget(id);
        assert_eq!(pending_tells(id), Vec::<Tell>::new());
    }

    #[test]
    fn a_turn_after_one_that_came_up_empty_is_told_and_cleared_by_a_success() {
        use grokhub_core::conversation::CAME_UP_EMPTY;
        let id = "vs-empty";
        register(id, cfg("nyx", None));
        step(id, view("find the patch notes for 2.1", "research", "calm"));
        note_reply(id, "I couldn't find patch notes for 2.1 anywhere.", true);
        let thanks = step(id, view("ah ok thanks", "chat", "calm"));
        assert!(thanks.ends_with(CAME_UP_EMPTY), "{thanks}");
        assert!(after_empty(id));
        // Taking the thanks anyway is named next turn.
        note_reply(id, "Cool, glad it helped!", false);
        assert_eq!(pending_tells(id), vec![Tell::TakesCredit]);
        let next = step(id, view("lol", "chat", "calm"));
        assert!(
            next.ends_with("Your last reply slipped: it took thanks for a result it never found, so own that it came up empty."),
            "{next}"
        );
        assert!(!next.contains(CAME_UP_EMPTY), "a success clears it: {next}");
        assert!(!after_empty(id));
        // Without an empty turn before it, "glad it helped" is fine.
        note_reply(id, "Glad it helped!", false);
        assert_eq!(pending_tells(id), Vec::<Tell>::new());
        forget(id);
        assert!(!after_empty(id));
    }
}
