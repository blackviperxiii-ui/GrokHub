//! Quick chips in the shell: the cabin's engine, chip memory, ranker and log
//! (`grokhub_core::chips`), so both apps suggest the same next steps.
//!
//! Only what the page can do crosses to it. A chip asks (its text goes in the
//! box or is sent) or goes (it opens a page). The cabin's one-tap chips
//! (backup snooze, radar remind), its review chips (a `/sh` draft) and the
//! retired model and effort chips stay on the Rust side.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use grokhub_core::{ChipAction, ChipInput, ChipMemory, ChipSignals, QuickChip, RankCtx};
use serde::{Deserialize, Serialize};

use crate::settings::Lane;

/// The composer's row and Home's row, as the cabin names them in the log.
pub const COMPOSER: &str = "composer";
pub const HOME: &str = "home";
/// The command palette before anything is typed.
pub const PALETTE: &str = "palette";
/// The reply's chips, saved per chat (`FollowUps`).
const FOLLOW_UPS_FILE: &str = "shell_follow_ups.json";
/// Chats whose follow-ups are kept, newest first.
const FOLLOW_UPS_KEPT: usize = 64;

/// What a tap does on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tap {
    /// `value` goes in the box, or is sent when the box is empty.
    Ask,
    /// Opens `view`.
    Go,
}

/// A chip as the page gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chip {
    pub id: String,
    pub label: String,
    pub value: String,
    pub action: Tap,
    /// Why this chip is here, for the chip's "Why this?" tip.
    pub hint: String,
    pub primary: bool,
    /// The page a Go chip opens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<&'static str>,
    /// For a follow-up: the mode its reply was written in (`chat` or
    /// `plan`), which the tap puts back.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mode: String,
}

/// The shell page for a cabin page a chip opens, if the shell has one.
pub fn shell_view(nav: &str) -> Option<&'static str> {
    Some(match nav {
        "settings" => "settings",
        "imagine" => "imagine",
        "history" => "history",
        "workboard" => "workboard",
        "pulse" => "pulse",
        "skills" => "skills",
        "night" => "automations",
        "agents" => "agents",
        "chat" => "chat",
        _ => return None,
    })
}

/// The chip as the page gets it, or `None` when the page can't do it: a
/// retired model or effort chip, the cabin's Undo, a one-tap Do chip, a
/// review chip (the shell has no `/sh`), or a page the shell doesn't have.
pub fn wire(chip: &QuickChip, mode: &str) -> Option<Chip> {
    if grokhub_core::retired_mode_value(&chip.value) || chip.id == grokhub_core::UNDO_CHIP_ID {
        return None;
    }
    let (action, view) = match grokhub_core::chip_action(chip) {
        ChipAction::Ask => (Tap::Ask, None),
        ChipAction::Go => (
            Tap::Go,
            Some(grokhub_core::nav_from_chip_value(&chip.value).and_then(shell_view)?),
        ),
        ChipAction::Do | ChipAction::Review => return None,
    };
    let follow_up = chip
        .id
        .starts_with(grokhub_core::chip_trailer::REPLY_CHIP_PREFIX);
    Some(Chip {
        id: chip.id.clone(),
        label: chip.label.clone(),
        value: chip.value.clone(),
        action,
        hint: chip.hint.clone(),
        primary: chip.primary,
        view,
        mode: if follow_up && action == Tap::Ask {
            mode.to_string()
        } else {
            String::new()
        },
    })
}

/// The chips the page can show, in order.
fn wired(chips: &[QuickChip], mode: &str) -> Vec<Chip> {
    chips.iter().filter_map(|c| wire(c, mode)).collect()
}

/// A reply's chips, saved with the chat until it moves on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowUps {
    pub chips: Vec<QuickChip>,
    /// The chat's user turns when the reply ended; another turn retires them.
    pub at: usize,
    /// The mode the reply was written in.
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub saved_ms: u64,
}

/// Follow-ups go through one lock, so two chats ending at once keep both.
static FOLLOW_UPS_IO: Mutex<()> = Mutex::new(());

fn read_follow_ups(dir: &Path) -> HashMap<String, FollowUps> {
    std::fs::read_to_string(dir.join(FOLLOW_UPS_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Saves `ups` as the chat's follow-ups, or clears them when there are none.
pub fn save_follow_ups(dir: &Path, chat: &str, ups: Option<FollowUps>) -> Result<(), String> {
    let _io = FOLLOW_UPS_IO.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = read_follow_ups(dir);
    match ups {
        Some(ups) => {
            all.insert(chat.to_string(), ups);
        }
        None => {
            if all.remove(chat).is_none() {
                return Ok(());
            }
        }
    }
    if all.len() > FOLLOW_UPS_KEPT {
        let mut by_age: Vec<(u64, String)> =
            all.iter().map(|(k, v)| (v.saved_ms, k.clone())).collect();
        by_age.sort();
        for (_, old) in by_age.iter().take(all.len() - FOLLOW_UPS_KEPT) {
            all.remove(old);
        }
    }
    let raw = serde_json::to_string(&all).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&dir.join(FOLLOW_UPS_FILE), &raw)
}

/// The chat's follow-ups while they still belong to its last reply.
pub fn follow_ups(dir: &Path, chat: &str, turns: usize) -> Option<FollowUps> {
    let _io = FOLLOW_UPS_IO.lock().unwrap_or_else(|e| e.into_inner());
    read_follow_ups(dir)
        .remove(chat)
        .filter(|ups| ups.at == turns)
}

/// The person's turns in a transcript.
pub fn user_turns(pairs: &[(String, String)]) -> usize {
    pairs.iter().filter(|(role, _)| role == "user").count()
}

/// A chat's transcript for the chip engine: the chips block a reply ended
/// with is not part of what was said.
pub fn chat_pairs(info: &grokhub_agent::SessionInfo) -> Vec<(String, String)> {
    grokhub_agent::transcript_pairs(info)
        .into_iter()
        .map(|(role, text)| {
            if role == "assistant" {
                let (shown, _) = grokhub_core::split_chip_trailer(&text);
                (role, shown)
            } else {
                (role, text)
            }
        })
        .collect()
}

/// The reply's chips, from its chips block, or the local reading of the turn
/// when the block is off or gave none.
pub fn reply_chips(said: &str, reply: &str, block: &str, failed: bool) -> Vec<QuickChip> {
    let chips = grokhub_core::parse_chip_trailer(block);
    if !chips.is_empty() {
        return chips;
    }
    grokhub_core::chips::reply_suggestions(said, reply, failed)
}

/// Whether a finished turn gets follow-ups. Btw asks only look, and a
/// stopped or halted turn is not a reply to build on.
pub fn follow_ups_wanted(btw: bool, stop: &str) -> bool {
    !btw && stop != "cancelled" && stop != crate::agent::HALTED
}

/// What the page asks a row for.
pub struct RowAsk<'a> {
    pub surface: &'a str,
    /// The open chat's transcript and draft; empty for Home.
    pub chat: &'a [(String, String)],
    pub draft: &'a str,
    pub follow_ups: &'a [QuickChip],
    pub mode: &'a str,
    pub signed_in: bool,
    pub first_run: bool,
    pub hour: u8,
    pub weekday: u8,
    pub quiet_hours: bool,
    pub now_ms: u64,
    /// Feature chips (focus help) prepended into the pool.
    pub extra: &'a [QuickChip],
    /// Settings → Behavior: the composer's near ties lean this way.
    pub lane: Lane,
}

/// What a row keeps in this process between showing it and the tap: the
/// ranker's notes (never saved) and the chips the page was given.
#[derive(Default)]
pub struct Rows {
    ranked: Vec<grokhub_core::chip_rank::RankedChip>,
    shown: HashMap<String, Vec<QuickChip>>,
}

/// The ranked row for one surface, logged as shown. Chip memory is read
/// fresh each time, so the cabin's own picks count here too.
pub fn row(dir: &Path, rows: &Mutex<Rows>, ask: RowAsk<'_>) -> Vec<Chip> {
    let mut held = rows.lock().unwrap_or_else(|e| e.into_inner());
    let mut mem = grokhub_core::load_chip_memory(dir);
    mem.ranker.last = std::mem::take(&mut held.ranked);
    let mut pool = pool(&mem, &ask);
    // Only the open chat's row leans; Home and the palette stay as ranked.
    if ask.surface == COMPOSER {
        lean(&mut pool, ask.lane);
    }
    let seed = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (ask.surface, ask.chat.len(), ask.hour, ask.follow_ups.len()).hash(&mut h);
        h.finish()
    };
    let ranked = grokhub_core::rank_quick_chips(
        &mut mem,
        pool,
        RankCtx {
            surface: ask.surface,
            hour: ask.hour,
            now_ms: ask.now_ms,
            seed,
            max: grokhub_core::CHIP_ROW_MAX,
        },
    );
    // Only chips the page can do are shown, logged and kept for the tap.
    let shown: Vec<QuickChip> = ranked
        .into_iter()
        .filter(|c| wire(c, ask.mode).is_some())
        .collect();
    grokhub_core::log_chips_shown(&mut mem, ask.surface, &shown, ask.now_ms);
    held.ranked = std::mem::take(&mut mem.ranker.last);
    let out = wired(&shown, ask.mode);
    held.shown.insert(ask.surface.to_string(), shown);
    // Saved under the lock: a tap and a row never write over each other.
    let _ = grokhub_core::save_chip_memory(dir, &mem);
    drop(held);
    out
}

fn pool(mem: &ChipMemory, ask: &RowAsk<'_>) -> Vec<QuickChip> {
    let last_slash = mem.last_slash.clone().unwrap_or_default();
    let last_surface = mem.last_surface.clone().unwrap_or_default();
    grokhub_core::build_quick_chips(ChipInput {
        chat: ask.chat,
        draft: ask.draft,
        grok_connected: ask.signed_in,
        // The router alone picks the model: chips always see Auto.
        mode: "auto",
        thread_title: "",
        usage_messages: 0,
        memory: mem,
        dismissed: &[],
        reply_chips: ask.follow_ups,
        last_failed: false,
        hour: ask.hour,
        now_ms: ask.now_ms,
        max: grokhub_core::CHIP_HARD_MAX,
        other_threads: &[],
        first_run: ask.first_run,
        last_slash: &last_slash,
        last_surface: &last_surface,
        last_project: "",
        skill_count: 0,
        session_mode: ask.mode,
        signals: ChipSignals {
            weekday: ask.weekday,
            quiet_hours: ask.quiet_hours,
            ..ChipSignals::default()
        },
        extra: ask.extra,
    })
}

/// Lean the pool toward the lane before the ranker sees it, as the cabin
/// did. The lane only breaks near ties: an on-lane chip gains at most two
/// thirds of a tenth of the top score, so a much stronger suggestion is never
/// pushed down for being off-lane, and the ranker weighs the leaned score
/// like any other, so the row it logs is the row that shows.
pub fn lean(chips: &mut [QuickChip], lane: Lane) {
    let top = chips.iter().map(|c| c.score).fold(0.0_f32, f32::max);
    let step = (top * 0.1).max(f32::EPSILON) / 3.0;
    for c in chips.iter_mut() {
        c.score += step * f32::from(2 - lane_rank(c, lane));
    }
    chips.sort_by(|a, b| b.score.total_cmp(&a.score));
}

/// A feature's own offers (code resume, focus help, backup and the rest)
/// keep the order the feature gave them.
const FEATURE_OFFER_IDS: &[&str] = &[
    "model:", "resume:", "backup:", "tidy:", "radar:", "game:", "focus:",
];

/// 0 on the lane, 1 neither way, 2 the other lane's.
fn lane_rank(c: &QuickChip, lane: Lane) -> u8 {
    if FEATURE_OFFER_IDS.iter().any(|p| c.id.starts_with(p)) {
        return 0;
    }
    let (on, off) = match lane {
        Lane::Coding => (leans_coding(c), leans_life(c)),
        Lane::Life => (leans_life(c), leans_coding(c)),
    };
    if on {
        0
    } else if off {
        2
    } else {
        1
    }
}

fn chip_blob(c: &QuickChip) -> String {
    format!("{} {} {}", c.id, c.label, c.value).to_ascii_lowercase()
}

/// Life is Imagine only: the lane doesn't rebrand any other chip.
fn leans_life(c: &QuickChip) -> bool {
    chip_blob(c).contains("imagine")
}

fn leans_coding(c: &QuickChip) -> bool {
    let blob = chip_blob(c);
    c.kind == grokhub_core::ChipKind::Shell
        || ["ship", "fix", "skill", "/sh"]
            .iter()
            .any(|w| blob.contains(w))
}

/// The person sent a message while `surface` was up: the row there notes
/// how long it stood (`chip_health`). Rows-locked like a tap, so the two
/// never write over each other.
pub fn next_step(dir: &Path, rows: &Mutex<Rows>, surface: &str, now: u64) {
    let held = rows.lock().unwrap_or_else(|e| e.into_inner());
    let mut mem = grokhub_core::load_chip_memory(dir);
    if mem.events.is_empty() {
        return;
    }
    mem.ranker.last = held.ranked.clone();
    grokhub_core::log_chip_next_action(&mut mem, surface, now);
    // Saved under the lock: a tap and a row never write over each other.
    let _ = grokhub_core::save_chip_memory(dir, &mem);
    drop(held);
}

/// A tap or a hide on a chip the page was shown. Only a chip from that
/// surface's last row counts: the page names it, it can't make one up.
/// Returns the chip that was picked, for the tap to act on.
pub fn outcome(
    dir: &Path,
    rows: &Mutex<Rows>,
    surface: &str,
    id: &str,
    picked: bool,
    clock: (u8, u8, u64),
) -> Option<QuickChip> {
    let (hour, weekday, now) = clock;
    let mut held = rows.lock().unwrap_or_else(|e| e.into_inner());
    let chip = held
        .shown
        .get(surface)
        .and_then(|row| row.iter().find(|c| c.id == id))
        .cloned()?;
    let mut mem = grokhub_core::load_chip_memory(dir);
    mem.ranker.last = held.ranked.clone();
    grokhub_core::log_chip_outcome(&mut mem, surface, &chip, picked, now);
    if picked {
        grokhub_core::remember_chip_click(&mut mem, &chip, None, now, hour);
        grokhub_core::remember_intent(&mut mem, &chip.label, &chip.value, now, hour, weekday);
    } else {
        grokhub_core::remember_chip_dismiss(&mut mem, &chip, now, hour);
    }
    // The row was acted on: the next showing is a new one.
    held.shown.remove(surface);
    // Saved under the lock: a tap and a row never write over each other.
    let _ = grokhub_core::save_chip_memory(dir, &mem);
    drop(held);
    Some(chip)
}

/// Whether the reply's chips block goes in the chat's rules now: Settings
/// → Suggestions is on and the block hasn't been losing to the local chips.
pub fn trailer_on(dir: &Path, suggestions: bool, now_ms: u64) -> bool {
    suggestions && grokhub_core::chip_trailer_on(&grokhub_core::load_chip_memory(dir), now_ms)
}

/// After a reply: takes the block out of the rules for a while when its
/// chips keep losing.
pub fn review_trailer(dir: &Path, now_ms: u64) {
    let mut mem = grokhub_core::load_chip_memory(dir);
    if grokhub_core::review_chip_trailer(&mut mem, now_ms) {
        let _ = grokhub_core::save_chip_memory(dir, &mem);
    }
}

/// The follow-ups as the page gets them.
pub fn wire_follow_ups(ups: &FollowUps) -> Vec<Chip> {
    wired(&ups.chips, &ups.mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::ChipKind;

    fn chip(id: &str, value: &str, kind: ChipKind) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: "Next step".into(),
            value: value.into(),
            kind,
            score: 1.0,
            hint: "Why".into(),
            primary: false,
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-chips-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn only_what_the_page_can_do_crosses() {
        let ask = wire(&chip("pred-1", "Summarize this", ChipKind::Chat), "chat").unwrap();
        assert_eq!(
            (ask.action, ask.view, ask.mode.as_str()),
            (Tap::Ask, None, "")
        );
        let go = wire(&chip("nav-1", "__nav:night", ChipKind::Nav), "chat").unwrap();
        assert_eq!((go.action, go.view), (Tap::Go, Some("automations")));
        // A page the shell doesn't have, a review chip, a one-tap and a
        // retired model chip never reach the page.
        assert_eq!(
            wire(&chip("nav-2", "__nav:command", ChipKind::Nav), "chat"),
            None
        );
        assert_eq!(wire(&chip("sh-1", "$ ls", ChipKind::Shell), "chat"), None);
        assert_eq!(
            wire(
                &chip(grokhub_core::UNDO_CHIP_ID, "x", ChipKind::Chat),
                "chat"
            ),
            None
        );
        assert_eq!(
            wire(&chip("m-1", "__mode:fast", ChipKind::Nav), "chat"),
            None
        );
        // A follow-up carries its reply's mode back to the tap.
        let reply = wire(&chip("reply-1", "Go deeper", ChipKind::Chat), "plan").unwrap();
        assert_eq!(reply.mode, "plan");
        let json = serde_json::to_value(&go).unwrap();
        assert_eq!(json["action"], "go");
        assert_eq!(json["view"], "automations");
        assert!(json.get("mode").is_none());
    }

    #[test]
    fn follow_ups_need_a_reply_to_build_on() {
        assert!(follow_ups_wanted(false, "end_turn"));
        assert!(follow_ups_wanted(false, "error"));
        assert!(!follow_ups_wanted(true, "end_turn"), "btw only looks");
        assert!(!follow_ups_wanted(false, "cancelled"));
        assert!(!follow_ups_wanted(false, crate::agent::HALTED));
    }

    #[test]
    fn follow_ups_last_until_the_chat_moves_on() {
        let dir = temp_dir("follow-ups");
        let ups = FollowUps {
            chips: vec![chip("reply-1", "Go deeper", ChipKind::Chat)],
            at: 2,
            mode: "plan".into(),
            saved_ms: 1,
        };
        save_follow_ups(&dir, "shell-a", Some(ups.clone())).unwrap();
        assert_eq!(follow_ups(&dir, "shell-a", 2), Some(ups));
        assert_eq!(
            follow_ups(&dir, "shell-a", 3),
            None,
            "a new turn retires them"
        );
        assert_eq!(follow_ups(&dir, "shell-b", 2), None);
        save_follow_ups(&dir, "shell-a", None).unwrap();
        assert_eq!(follow_ups(&dir, "shell-a", 2), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_reply_block_wins_and_the_local_reading_fills_in() {
        let block = "ask|Explain the second step in more detail|Explain step two";
        let chips = reply_chips("how do I do this", "First... Second...", block, false);
        assert_eq!(chips.len(), 1);
        assert!(chips[0]
            .id
            .starts_with(grokhub_core::chip_trailer::REPLY_CHIP_PREFIX));
        // No block: the same local reading the shell used before.
        let local = reply_chips("hi", "Hello!", "", false);
        assert_eq!(
            local,
            grokhub_core::chips::reply_suggestions("hi", "Hello!", false)
        );
    }

    #[test]
    fn a_row_logs_what_it_showed_and_a_tap_on_it_teaches() {
        let dir = temp_dir("row");
        let rows = Mutex::new(Rows::default());
        let follow = [chip("reply-1", "Explain the second step", ChipKind::Chat)];
        let chat = [
            (
                "user".to_string(),
                "How do I set up the backup drive?".to_string(),
            ),
            (
                "assistant".to_string(),
                "First plug it in. Second, pick it in Settings.".to_string(),
            ),
        ];
        let ask = || RowAsk {
            surface: COMPOSER,
            chat: &chat,
            draft: "",
            follow_ups: &follow,
            mode: "chat",
            signed_in: true,
            first_run: false,
            hour: 10,
            weekday: 2,
            quiet_hours: false,
            now_ms: 1_000,
            extra: &[],
            lane: Lane::Coding,
        };
        let shown = row(&dir, &rows, ask());
        assert!(
            shown.iter().any(|c| c.id == "reply-1"),
            "the reply's chip ranks in the row: {shown:?}"
        );
        let mem = grokhub_core::load_chip_memory(&dir);
        assert_eq!(mem.events.len(), 1);
        assert_eq!(mem.events[0].surface, COMPOSER);
        // A chip the page never got is refused.
        assert_eq!(
            outcome(&dir, &rows, COMPOSER, "made-up", true, (10, 2, 2_000)),
            None
        );
        let picked = outcome(&dir, &rows, COMPOSER, "reply-1", true, (10, 2, 2_000)).unwrap();
        assert_eq!(picked.id, "reply-1");
        let mem = grokhub_core::load_chip_memory(&dir);
        assert!(matches!(
            mem.events[0].outcome,
            Some(grokhub_core::ChipOutcome::Picked { .. })
        ));
        // The next message notes how long the row stood, once.
        next_step(&dir, &rows, COMPOSER, 2_500);
        next_step(&dir, &rows, COMPOSER, 9_000);
        let mem = grokhub_core::load_chip_memory(&dir);
        assert_eq!(mem.events[0].next_at, Some(2_500));
        // The row was acted on, so the same id is not taken twice.
        assert_eq!(
            outcome(&dir, &rows, COMPOSER, "reply-1", true, (10, 2, 3_000)),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scored(id: &str, label: &str, value: &str, kind: ChipKind, score: f32) -> QuickChip {
        QuickChip {
            label: label.into(),
            score,
            ..chip(id, value, kind)
        }
    }

    #[test]
    fn the_lane_flips_a_tie_and_life_is_only_imagine() {
        let mut chips = vec![
            scored(
                "imagine",
                "Open Imagine",
                "__nav:imagine",
                ChipKind::Nav,
                1.0,
            ),
            scored(
                "ship",
                "Ship it",
                "Ship a minimal solid slice.",
                ChipKind::Chat,
                1.0,
            ),
            scored(
                "plan",
                "Plan my weekend",
                "Plan my weekend",
                ChipKind::Chat,
                1.0,
            ),
        ];
        lean(&mut chips, Lane::Coding);
        let ids: Vec<_> = chips.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["ship", "plan", "imagine"]);
        lean(&mut chips, Lane::Life);
        let ids: Vec<_> = chips.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["imagine", "plan", "ship"]);
        // Leaning moves chips, it never relabels one.
        assert_eq!(chips[0].label, "Open Imagine");
        assert_eq!(chips[0].value, "__nav:imagine");
    }

    #[test]
    fn the_lane_only_breaks_near_ties() {
        let mut chips = vec![
            scored(
                "imagine",
                "Open Imagine",
                "__nav:imagine",
                ChipKind::Nav,
                100.0,
            ),
            scored(
                "ship",
                "Ship it",
                "Ship a minimal solid slice.",
                ChipKind::Chat,
                40.0,
            ),
        ];
        lean(&mut chips, Lane::Coding);
        assert_eq!(chips[0].id, "imagine", "a far stronger chip keeps its spot");
        // The off-lane chip gains nothing; the on-lane one two steps of 10/3.
        assert_eq!(chips[0].score, 100.0);
        chips[1].score = 95.0;
        lean(&mut chips, Lane::Coding);
        assert_eq!(chips[0].id, "ship", "inside one band the lane decides");
        // The band is a tenth of the top: 8 behind stays behind.
        let mut chips = vec![
            scored(
                "imagine",
                "Open Imagine",
                "__nav:imagine",
                ChipKind::Nav,
                100.0,
            ),
            scored(
                "ship",
                "Ship it",
                "Ship a minimal solid slice.",
                ChipKind::Chat,
                92.0,
            ),
        ];
        lean(&mut chips, Lane::Coding);
        assert_eq!(chips[0].id, "imagine");
    }

    #[test]
    fn each_coding_word_leads_a_plain_chip_on_the_coding_lane() {
        for (id, label) in [
            ("ship", "Ship it"),
            ("fix", "Fix the build"),
            ("skill", "Make a skill"),
            ("sh", "Run /sh ls"),
        ] {
            let mut chips = vec![
                scored(
                    "plan",
                    "Plan my weekend",
                    "Plan my weekend",
                    ChipKind::Chat,
                    1.0,
                ),
                scored(id, label, label, ChipKind::Chat, 1.0),
            ];
            lean(&mut chips, Lane::Coding);
            assert_eq!(chips[0].id, id);
        }
    }

    #[test]
    fn a_features_own_offer_counts_as_on_lane_both_ways() {
        for lane in [Lane::Coding, Lane::Life] {
            let mut chips = vec![
                scored(
                    "plan",
                    "Plan my weekend",
                    "Plan my weekend",
                    ChipKind::Chat,
                    1.0,
                ),
                scored(
                    "focus:help",
                    "Help me focus",
                    "Help me focus",
                    ChipKind::Chat,
                    1.0,
                ),
            ];
            lean(&mut chips, lane);
            assert_eq!(chips[0].id, "focus:help", "{lane:?}");
        }
    }

    #[test]
    fn the_open_chats_row_leans_with_the_setting() {
        let follow = [
            QuickChip {
                label: "Imagine a cover".into(),
                ..chip("reply-art", "Imagine a cover for it", ChipKind::Chat)
            },
            QuickChip {
                label: "Fix the build".into(),
                ..chip("reply-fix", "Fix the failing build", ChipKind::Chat)
            },
        ];
        let chat = [
            ("user".to_string(), "Draft the release post".to_string()),
            ("assistant".to_string(), "Here is a draft.".to_string()),
        ];
        let extra = [
            QuickChip {
                label: "Imagine a poster".into(),
                ..chip("home-art", "Imagine a poster", ChipKind::Chat)
            },
            QuickChip {
                label: "Fix my setup".into(),
                ..chip("home-fix", "Fix my setup", ChipKind::Chat)
            },
        ];
        let order = |surface: &'static str, lane: Lane| {
            let dir = temp_dir(&format!("lane-{surface}-{lane:?}"));
            let rows = Mutex::new(Rows::default());
            let shown = row(
                &dir,
                &rows,
                RowAsk {
                    surface,
                    chat: &chat,
                    draft: "",
                    follow_ups: &follow,
                    mode: "chat",
                    signed_in: true,
                    first_run: false,
                    hour: 10,
                    weekday: 2,
                    quiet_hours: false,
                    now_ms: 1_000,
                    extra: if surface == HOME { &extra } else { &[] },
                    lane,
                },
            );
            let _ = std::fs::remove_dir_all(&dir);
            shown
                .into_iter()
                .map(|c| c.id)
                .filter(|id| id.starts_with("reply-") || id.starts_with("home-"))
                .collect::<Vec<_>>()
        };
        assert_eq!(order(COMPOSER, Lane::Coding), ["reply-fix", "reply-art"]);
        assert_eq!(order(COMPOSER, Lane::Life), ["reply-art", "reply-fix"]);
        // Home ranks as it always has, whichever way the lane points.
        let home = order(HOME, Lane::Life);
        assert_eq!(home.len(), 2, "{home:?}");
        assert_eq!(order(HOME, Lane::Coding), home);
    }
}
