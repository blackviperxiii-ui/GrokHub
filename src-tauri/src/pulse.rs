//! Pulse (cross-platform recode, Phase 3f): the cabin's Feed and Ideas from
//! the same `updates.json`, ranked by the same rules in `grokhub_core::pulse`.
//! Dismiss, Not this, Like and Accept write the same MEMORY.md ledger line
//! the cabin writes, so the next ranking learns from them.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use grokhub_core::pulse::{self as pc, LedgerReason};
use grokhub_core::{CardReaction, UpdateCard, UpdateKind};
use serde::{Deserialize, Serialize};

const FILE: &str = "updates.json";
/// The cabin's own cap for its JSON stores.
const STORE_CAP: u64 = 32 * 1024 * 1024;
/// The Memory page's cap for a notes file.
const NOTES_CAP: u64 = 1024 * 1024;

/// One writer at a time in this process; the temp file is this process's own.
static STORE: Mutex<()> = Mutex::new(());

fn store() -> MutexGuard<'static, ()> {
    STORE.lock().unwrap_or_else(|err| err.into_inner())
}

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub id: String,
    /// "Automation", or the source's own name.
    pub source: String,
    pub title: String,
    pub summary: String,
    /// "Why: …" when a source post matched an interest.
    pub because: String,
    pub ago: String,
    /// The first source link the research step returned.
    pub link: String,
    pub liked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Idea {
    pub id: String,
    /// The bold "I can …" line.
    pub title: String,
    pub summary: String,
    /// Do, Automate, Learn or Watch.
    pub kind: String,
    /// What Run puts in the chat box.
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    /// Empty for the unlabeled top group.
    pub label: String,
    pub ideas: Vec<Idea>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub feed: Vec<Post>,
    pub ideas: Vec<Group>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Act {
    Dismiss,
    NotThis,
    Like,
    Snooze,
    /// Run took the idea to chat.
    Accept,
}

/// The local wall clock from the page: today's date for the ledger line and
/// the time Snooze counts to 09:00 from.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Day {
    pub date: String,
    pub hour: u32,
    pub minute: u32,
}

fn load_unlocked(config_dir: &Path) -> Result<Vec<UpdateCard>, String> {
    let path = path(config_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len > STORE_CAP {
        return Err(format!("{FILE} is too large to be a feed."));
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw).map_err(|e| format!("{FILE}: {e}"))
}

/// Writes through a temp file and a rename, like the cabin. Callers hold
/// [`STORE`].
fn save(config_dir: &Path, cards: &[UpdateCard]) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(cards).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.shell-{}.tmp", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

fn ledger() -> Vec<pc::LedgerEntry> {
    let text = std::fs::read_to_string(grokhub_agent::global_memory_path()).unwrap_or_default();
    pc::parse_ledger(&text)
}

/// Appends one ledger line through a temp file and a rename, so a crash never
/// leaves MEMORY.md half written. A line that would carry a secret is left
/// out, as the cabin's `append_memory` does; the card action still goes on.
fn note(title: &str, reason: LedgerReason, date: &str) -> Result<(), String> {
    let line = pc::ledger_line(date, title, reason);
    if !grokhub_core::is_plain_text(&line) {
        return Ok(());
    }
    let path = grokhub_agent::global_memory_path();
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("MEMORY.md is a link to another file. GrokHub does not follow it.".into());
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > NOTES_CAP) {
        return Err("MEMORY.md is too large to add to. Nothing was changed.".into());
    }
    let mut body = std::fs::read_to_string(&path).unwrap_or_default();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(&line);
    body.push('\n');
    let parent = path.parent().ok_or("MEMORY.md has no folder.")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!("MEMORY.md.shell-{}.tmp", std::process::id()));
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

fn post(card: &UpdateCard, now_ms: u64) -> Post {
    let source = card
        .pulse
        .source_name
        .clone()
        .unwrap_or_else(|| card.kind.label().to_string());
    Post {
        id: card.id.clone(),
        source,
        title: card.title.trim().to_string(),
        summary: card.body.clone().unwrap_or_default().trim().to_string(),
        because: card.pulse.because.clone().unwrap_or_default(),
        ago: pc::ago_label(card.created_at, now_ms),
        link: card
            .citations
            .iter()
            .find(|url| grokhub_core::public_http_url(url))
            .cloned()
            .unwrap_or_default(),
        liked: card.reaction == Some(CardReaction::Up),
    }
}

fn idea(card: &UpdateCard) -> Idea {
    let action = card.idea_action();
    Idea {
        id: card.id.clone(),
        title: pc::i_can_title(card),
        summary: card.body.clone().unwrap_or_default().trim().to_string(),
        kind: pc::pulse_type(card).label().to_string(),
        action: if action.is_empty() {
            card.title.trim().to_string()
        } else {
            action
        },
    }
}

fn page(cards: &[UpdateCard], now_ms: u64) -> Page {
    let ledger = ledger();
    let inputs = pc::PulseInputs {
        ledger: &ledger,
        now_ms,
        ..Default::default()
    };
    let by_id = |id: &str| cards.iter().find(|c| c.id == id);
    Page {
        feed: pc::feed_posts(cards, &inputs)
            .iter()
            .map(|c| post(c, now_ms))
            .collect(),
        ideas: pc::group_ideas(cards, &inputs)
            .into_iter()
            .map(|(cat, rows)| Group {
                label: cat.map(|c| c.label().to_string()).unwrap_or_default(),
                ideas: rows.iter().filter_map(|r| by_id(&r.id)).map(idea).collect(),
            })
            .collect(),
    }
}

pub fn load(config_dir: &Path, now_ms: u64) -> Result<Page, String> {
    let _store = store();
    let mut cards = load_unlocked(config_dir)?;
    if grokhub_core::collapse_feed(&mut cards) {
        save(config_dir, &cards)?;
    }
    Ok(page(&cards, now_ms))
}

/// Dismiss and Not this take the card off the page (an idea leaves the
/// store; an event leaves a tombstone so a repeat stays hidden for a day).
/// Like marks it. Snooze hides it until the next 09:00. Accept only notes it.
pub fn act(config_dir: &Path, id: &str, act: Act, day: &Day, now_ms: u64) -> Result<Page, String> {
    let _store = store();
    let mut cards = load_unlocked(config_dir)?;
    let Some(card) = cards.iter().find(|c| c.id == id).cloned() else {
        return Err("That card is gone. The page now shows what is left.".into());
    };
    let remove = |cards: &mut Vec<UpdateCard>| {
        if card.kind == UpdateKind::Idea {
            grokhub_core::dismiss_idea(cards, id)
        } else {
            grokhub_core::dismiss_update_at(cards, id, now_ms)
        }
    };
    let changed = match act {
        Act::Dismiss => {
            note(&card.title, LedgerReason::Dismiss, &day.date)?;
            remove(&mut cards)
        }
        Act::NotThis => {
            note(&card.title, LedgerReason::NotThis, &day.date)?;
            remove(&mut cards)
        }
        Act::Like => {
            if card.reaction == Some(CardReaction::Up) {
                false
            } else {
                note(&card.title, LedgerReason::Liked, &day.date)?;
                if let Some(saved) = cards.iter_mut().find(|c| c.id == id) {
                    saved.reaction = Some(CardReaction::Up);
                }
                true
            }
        }
        Act::Snooze => {
            let until = pc::snooze_until(now_ms, day.hour, day.minute);
            pc::snooze_card(&mut cards, id, until)
        }
        Act::Accept => {
            note(&card.title, LedgerReason::Accepted, &day.date)?;
            grokhub_core::mark_update_opened(&mut cards, id)
        }
    };
    if changed {
        save(config_dir, &cards)?;
    }
    Ok(page(&cards, now_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::{automation_done_card, idea_card};

    const NOW: u64 = 1_800_000_000_000;

    struct Dirs {
        config: PathBuf,
        _guard: grokhub_agent::perm::ConfigGuard,
    }

    fn dirs(name: &str, cards: &[UpdateCard]) -> Dirs {
        let config = std::env::temp_dir().join(format!(
            "grokhub-shell-pulse-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&config);
        std::fs::create_dir_all(&config).expect("cfg");
        let guard = grokhub_agent::perm::ConfigGuard::set(&config);
        std::fs::write(
            config.join(FILE),
            serde_json::to_string(cards).expect("json"),
        )
        .expect("updates.json");
        Dirs {
            config,
            _guard: guard,
        }
    }

    fn day() -> Day {
        Day {
            date: "2026-10-25".into(),
            hour: 14,
            minute: 30,
        }
    }

    fn memory() -> String {
        std::fs::read_to_string(grokhub_agent::global_memory_path()).unwrap_or_default()
    }

    fn sample() -> Vec<UpdateCard> {
        let mut idea = idea_card(
            "idea-src",
            "Tidy the workboard",
            "Close the done cards",
            NOW - 60_000,
        );
        idea.id = "idea-1".into();
        idea.prompt = Some("close every done card on the workboard".into());
        let mut post =
            automation_done_card("loop-1", "Board", "summarized 3 cards", NOW - 7_200_000);
        post.id = "post-1".into();
        post.citations = vec!["https://example.com/board".into()];
        vec![idea, post]
    }

    #[test]
    fn the_page_splits_feed_and_ideas_like_the_cabin() {
        let d = dirs("page", &sample());
        let page = load(&d.config, NOW).expect("load");
        assert_eq!(page.feed.len(), 1, "{page:?}");
        let post = &page.feed[0];
        assert_eq!(post.id, "post-1");
        assert_eq!(post.source, "Automation");
        assert_eq!(post.ago, "2h ago");
        assert_eq!(post.link, "https://example.com/board");
        assert!(!post.liked);

        let ideas: Vec<&Idea> = page.ideas.iter().flat_map(|g| &g.ideas).collect();
        assert_eq!(ideas.len(), 1, "{page:?}");
        assert_eq!(ideas[0].id, "idea-1");
        assert!(ideas[0].title.starts_with("I can "), "{:?}", ideas[0]);
        assert_eq!(ideas[0].action, "close every done card on the workboard");
        assert_eq!(page.ideas[0].label, "Productivity");
    }

    #[test]
    fn not_this_writes_the_ledger_and_takes_the_idea_away() {
        let d = dirs("not", &sample());
        let page = act(&d.config, "idea-1", Act::NotThis, &day(), NOW).expect("act");
        assert!(page.ideas.is_empty(), "{page:?}");
        assert_eq!(
            memory(),
            "- [2026-10-25] pulse: dismissed \"Tidy the workboard\" reason=not-this\n"
        );
        let saved = std::fs::read_to_string(d.config.join(FILE)).expect("read");
        assert!(!saved.contains("idea-1"), "{saved}");
        assert!(saved.contains("post-1"), "{saved}");
    }

    #[test]
    fn dismiss_like_and_snooze_change_the_post() {
        let d = dirs("post", &sample());
        let liked = act(&d.config, "post-1", Act::Like, &day(), NOW).expect("like");
        assert!(liked.feed[0].liked);
        // A second Like adds no second line.
        act(&d.config, "post-1", Act::Like, &day(), NOW).expect("like again");
        assert_eq!(
            memory(),
            "- [2026-10-25] pulse: liked \"Board\" reason=liked\n"
        );

        let snoozed = act(&d.config, "post-1", Act::Snooze, &day(), NOW).expect("snooze");
        assert!(snoozed.feed.is_empty(), "{snoozed:?}");
        // Back the next morning at nine.
        let tomorrow = pc::snooze_until(NOW, 14, 30);
        assert_eq!(load(&d.config, tomorrow).expect("load").feed.len(), 1);

        let gone = act(&d.config, "post-1", Act::Dismiss, &day(), tomorrow).expect("dismiss");
        assert!(gone.feed.is_empty());
        assert!(memory().ends_with("pulse: dismissed \"Board\" reason=dismiss\n"));
        assert_eq!(
            act(&d.config, "nope", Act::Dismiss, &day(), NOW),
            Err("That card is gone. The page now shows what is left.".to_string())
        );
    }

    #[test]
    fn accept_notes_the_idea_and_keeps_it() {
        let d = dirs("accept", &sample());
        let page = act(&d.config, "idea-1", Act::Accept, &day(), NOW).expect("accept");
        assert_eq!(page.ideas.iter().map(|g| g.ideas.len()).sum::<usize>(), 1);
        assert_eq!(
            memory(),
            "- [2026-10-25] pulse: accepted \"Tidy the workboard\" reason=accepted\n"
        );
    }

    #[test]
    fn a_secret_in_a_title_never_reaches_the_notes() {
        let mut cards = sample();
        cards[1].title = "Rotated sk-abcdefghijklmnopqrstuv".into();
        let d = dirs("secret", &cards);
        std::fs::create_dir_all(grokhub_agent::global_memory_path().parent().unwrap())
            .expect("memory dir");
        std::fs::write(grokhub_agent::global_memory_path(), "- earlier note").expect("seed");
        let page = act(&d.config, "post-1", Act::Dismiss, &day(), NOW).expect("dismiss");
        assert!(page.feed.is_empty(), "{page:?}");
        assert_eq!(memory(), "- earlier note");

        act(&d.config, "idea-1", Act::Accept, &day(), NOW).expect("accept");
        assert_eq!(
            memory(),
            "- earlier note\n- [2026-10-25] pulse: accepted \"Tidy the workboard\" reason=accepted\n"
        );
        let dir = grokhub_agent::global_memory_path()
            .parent()
            .unwrap()
            .to_path_buf();
        let left: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["MEMORY.md".to_string()], "no temp file is left");
    }

    #[test]
    fn no_feed_file_is_an_empty_page() {
        let d = dirs("empty", &[]);
        std::fs::remove_file(d.config.join(FILE)).expect("rm");
        assert_eq!(
            load(&d.config, NOW),
            Ok(Page {
                feed: Vec::new(),
                ideas: Vec::new()
            })
        );
    }
}
