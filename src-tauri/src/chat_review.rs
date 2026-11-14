//! The nightly chat review, moved out of the cabin. Once a local day it
//! reads your recent chats and asks the model what would help with work you
//! repeat: a Suggested automation tile, a skill idea on the Pulse page, or a
//! better version of a skill you already have. Before that, a quiet pass
//! with no model call turns a need asked twice into a tile.
//!
//! The review spends a call only after 8 new turns of yours, and a failed
//! call waits for the next night. Its state is `suggestions.json`, the file
//! the cabin wrote, changed through [`crate::suggested::with_suggestions`].

use std::io::Read;
use std::path::Path;

use grokhub_agent::harness as hx;
use grokhub_agent::harness::weekly_review as wr;
use grokhub_core::review_pass::{
    apply_reply, lasting_lines, load_suggestions, read_suggestions, recent_asks_from, said_lines,
    session_pass, ReplyGround,
};
use grokhub_core::{
    digest_line_from, parse_suggest_skill_patches, review_due, review_worth_tokens, DigestLine,
    FeedPulse, IdeaGround, LearnedSuggestion, LocalClock, SkillMd, SkillPatch,
};

use crate::suggested::with_suggestions;

/// What tonight's review did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reviewed {
    /// It ran today already, or this is the first night and the hour has not come.
    NotDue,
    /// Too few new turns to spend a call on.
    NotWorth { new_turns: u32 },
    /// The call answered: suggestions kept, skill ideas posted, skills patched.
    Ran {
        kept: usize,
        ideas: usize,
        patched: usize,
    },
    /// The call failed; the review waits for the next night.
    Held(String),
}

/// The model call: system prompt, then the digest. Returns the reply text.
pub type Call<'a> = dyn Fn(&str, &str) -> Result<String, String> + 'a;

/// The newest chat's last lines in the digest; all chats' lines in total.
const NEWEST_CHAT_LINES: usize = 24;
const DIGEST_LINES: usize = 40;
const RECEIPTS: usize = 6;
const ASKS: usize = 30;
/// USER.md, MEMORY.md and learning.json are read up to this much.
const READ_CAP: u64 = 256 * 1024;

/// What the review reads from your chats.
#[derive(Debug, Default)]
struct Chats {
    lines: Vec<DigestLine>,
    receipts: Vec<String>,
    said: Vec<String>,
    asks: Vec<String>,
    user_turns: u32,
}

/// Every saved chat but scratch ones, newest first, as `(role, text)`
/// pairs. A scratch chat is never learned from.
fn chat_pairs(config_dir: &Path) -> Vec<Vec<(String, String)>> {
    crate::history::all_saved(config_dir)
        .iter()
        .filter(|info| !grokhub_agent::scratch::is_scratch(&info.id))
        .map(crate::chips::chat_pairs)
        .collect()
}

fn chats(config_dir: &Path) -> Chats {
    let all = chat_pairs(config_dir);
    let newest_first = || {
        all.iter()
            .flat_map(|chat| chat.iter().rev())
            .map(|(role, text)| (role.as_str(), text.as_str()))
    };
    let mut lines = Vec::new();
    for (i, chat) in all.iter().enumerate() {
        let cap = if i == 0 {
            NEWEST_CHAT_LINES
        } else {
            DIGEST_LINES
        };
        for (role, text) in chat.iter().rev() {
            if lines.len() >= cap {
                break;
            }
            if let Some(line) = digest_line_from(role, text) {
                lines.push(line);
            }
        }
        if lines.len() >= DIGEST_LINES {
            break;
        }
    }
    lines.reverse();
    let mut receipts: Vec<String> = all
        .iter()
        .rev()
        .flat_map(|chat| grokhub_core::thread_host_receipts(chat))
        .collect();
    if receipts.len() > RECEIPTS {
        receipts = receipts.split_off(receipts.len() - RECEIPTS);
    }
    Chats {
        lines,
        receipts,
        said: said_lines(all.iter().flatten().map(|(r, t)| (r.as_str(), t.as_str()))),
        asks: recent_asks_from(newest_first(), ASKS),
        user_turns: all
            .iter()
            .map(|c| crate::chips::user_turns(c))
            .sum::<usize>()
            .try_into()
            .unwrap_or(u32::MAX),
    }
}

fn read_capped(path: &Path) -> String {
    let mut text = String::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(READ_CAP).read_to_string(&mut text);
    }
    text
}

fn memory_note(config_dir: &Path, name: &str) -> String {
    read_capped(&config_dir.join("memory").join(name))
}

/// The cabin's learning file, for its insight pin. The desktop app does not
/// write it.
fn insight(config_dir: &Path) -> String {
    let raw = read_capped(&config_dir.join("learning.json"));
    serde_json::from_str::<grokhub_core::LearningState>(&raw)
        .map(|s| grokhub_core::insight_pin(&s))
        .unwrap_or_default()
}

fn trajectory(config_dir: &Path, now: u64) -> String {
    let mut raw = String::new();
    if let Ok(file) = std::fs::File::open(config_dir.join("trajectory.jsonl")) {
        let _ = file
            .take(grokhub_core::TRAJECTORY_MAX_BYTES as u64)
            .read_to_string(&mut raw);
    }
    grokhub_core::summarize_trajectory(
        &grokhub_core::parse_trajectory_jsonl(&raw),
        grokhub_core::yesterday_ms(now),
        12,
    )
}

fn open_cards(config_dir: &Path) -> Vec<String> {
    grokhub_core::work_run::read_board_file(&grokhub_core::work_run::board_path(config_dir))
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.status.column().is_some() && c.status != grokhub_core::BoardStatus::Done)
        .map(|c| c.title)
        .collect()
}

fn automation_names(config_dir: &Path) -> Vec<String> {
    crate::automations::load(config_dir)
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.name)
        .collect()
}

/// What you turned down on the Pulse page, oldest first.
fn turned_down(config_dir: &Path) -> Vec<String> {
    let ideas = crate::pulse_feed::with_ideas_state(config_dir, |s| s.clone()).unwrap_or_default();
    ideas.turned_down.into_iter().rev().collect()
}

/// Post the review's skills as Learn ideas on the Pulse page, then take them
/// off the list. One already posted, on the board, or saved is not posted
/// again. Returns how many landed.
fn post_ideas(config_dir: &Path, skills: &[SkillMd]) -> usize {
    let store = load_suggestions(config_dir);
    if store.skills.is_empty() {
        return 0;
    }
    let have: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    let mut pulse = FeedPulse {
        turned_down: turned_down(config_dir),
        idea_titles: store.idea_titles.clone(),
        ..FeedPulse::default()
    };
    let now = grokhub_core::now_ms();
    let posted = crate::pulse::edit_cards(config_dir, &mut |cards| {
        store
            .skills
            .iter()
            .filter(|item| grokhub_core::post_skill_idea(cards, &mut pulse, now, item, &have))
            .count()
    });
    let posted = match posted {
        Ok(n) => n,
        Err(e) => {
            eprintln!("chat review: {e}");
            return 0;
        }
    };
    let sent: Vec<LearnedSuggestion> = store.skills;
    let titles = pulse.idea_titles;
    if let Err(e) = with_suggestions(config_dir, |s| {
        s.skills.retain(|item| !sent.contains(item));
        s.idea_titles = titles;
    }) {
        eprintln!("chat review: {e}");
    }
    posted
}

/// Write the review's better versions of skills you have, each through the
/// replay gate. A version you undid is not written again. Returns how many
/// were written.
fn apply_skill_patches(
    config_dir: &Path,
    skills: &[SkillMd],
    patches: Vec<SkillPatch>,
    now: u64,
) -> usize {
    if patches.is_empty() {
        return 0;
    }
    let ledger = hx::ChangeLedger::load(config_dir);
    let mut written = 0;
    for p in patches {
        let Some(existing) = skills.iter().find(|s| s.name.eq_ignore_ascii_case(&p.name)) else {
            continue;
        };
        let proposed = SkillMd {
            name: existing.name.clone(),
            description: existing.description.clone(),
            slash: existing.slash.clone(),
            trigger: p.trigger,
            instructions: p.instructions,
            pitfalls: String::new(),
            verify: String::new(),
            runs: existing.runs,
        };
        let patched = grokhub_core::patch_skill(existing, &proposed);
        let (old, new) = (
            grokhub_core::render_skill_md(existing),
            grokhub_core::render_skill_md(&patched),
        );
        if old == new || ledger.undone_by_user(&patched.name, &hx::content_hash(new.as_bytes())) {
            continue;
        }
        if let Err(note) = hx::replay_gate(config_dir, &patched.name, &old, &new, now) {
            eprintln!("chat review: {} held: {note}", patched.name);
            continue;
        }
        let steps: Vec<&str> = patched.instructions.split_whitespace().collect();
        let reason = format!("nightly review: {}", steps.join(" "));
        match grokhub_agent::skill_write::save_skill_logged(
            config_dir,
            &patched,
            hx::Origin::SelfManage,
            &reason,
        ) {
            Ok(()) => written += 1,
            Err(e) => eprintln!("chat review: {}: {e}", patched.name),
        }
    }
    written
}

/// One look: the quiet pass, then the review when it is due and worth a
/// call. `call` runs only then. `night_hour` is the dream hour.
pub fn review_step(
    config_dir: &Path,
    today: &str,
    clock: &LocalClock,
    night_hour: u32,
    now: u64,
    call: &Call<'_>,
) -> Reviewed {
    // A file that can't be read holds the review: no day could be stamped,
    // so it would call again on every look.
    let store = match read_suggestions(config_dir) {
        Ok(s) => s,
        Err(e) => return Reviewed::Held(e),
    };
    let skills = wr::cabin_skills(&config_dir.join("skills"));
    let skill_names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
    let autos = automation_names(config_dir);
    let mut read: Option<Chats> = None;
    if review_due(
        store.last_session_suggest_day.as_deref(),
        today,
        clock,
        night_hour,
    ) {
        let lines = &read.get_or_insert_with(|| chats(config_dir)).lines;
        if let Err(e) = with_suggestions(config_dir, |s| {
            session_pass(s, lines, &skill_names, &autos, today, clock, night_hour)
        }) {
            eprintln!("chat review: {e}");
        }
    }
    post_ideas(config_dir, &skills);
    if !review_due(store.last_review_day.as_deref(), today, clock, night_hour) {
        return Reviewed::NotDue;
    }
    let chats = read.unwrap_or_else(|| chats(config_dir));
    let total = chats.user_turns;
    let reviewed = store.reviewed_turns.min(total);
    if !review_worth_tokens(total, reviewed) {
        return Reviewed::NotWorth {
            new_turns: total - reviewed,
        };
    }
    let user_md = memory_note(config_dir, "USER.md");
    let memory_md = memory_note(config_dir, "MEMORY.md");
    let turned_down = turned_down(config_dir);
    let digest = grokhub_core::build_review_digest(&grokhub_core::ReviewDigest {
        insight_pin: insight(config_dir),
        user_md: user_md.clone(),
        memory_md: memory_md.clone(),
        skill_names: skill_names.clone(),
        automation_names: autos.clone(),
        turned_down: turned_down.iter().rev().cloned().collect(),
        github_pat: false,
        host_receipts: chats.receipts,
        chip_habits: grokhub_core::top_habit_labels(&grokhub_core::load_chip_memory(config_dir), 6),
        thread_lines: chats.lines,
        trajectory: trajectory(config_dir, now),
    });
    let reply = call(grokhub_core::review_system_prompt(), &digest);
    let patched = match &reply {
        Ok(text) => {
            apply_skill_patches(config_dir, &skills, parse_suggest_skill_patches(text), now)
        }
        Err(_) => 0,
    };
    let lasting = lasting_lines(&user_md, &memory_md, &open_cards(config_dir));
    let pulse = FeedPulse {
        turned_down,
        ..FeedPulse::default()
    };
    let ground = ReplyGround {
        skills: &skill_names,
        automations: &autos,
        said: &chats.said,
        ideas: IdeaGround {
            asks: &chats.asks,
            lasting: &lasting,
        },
        pulse: &pulse,
    };
    // Re-read under the lock, so a tile added meanwhile stays added.
    let kept = with_suggestions(config_dir, |s| {
        s.reviewed_turns = total;
        apply_reply(
            s,
            reply.as_deref().map_err(String::as_str),
            today,
            now,
            &ground,
        )
    });
    let kept = match kept {
        Ok(n) => n,
        Err(e) => return Reviewed::Held(e),
    };
    match reply {
        Ok(_) => Reviewed::Ran {
            kept,
            ideas: post_ideas(config_dir, &skills),
            patched,
        },
        Err(e) => Reviewed::Held(e),
    }
}

#[cfg(test)]
#[path = "chat_review_tests.rs"]
mod tests;
