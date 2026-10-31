//! Pulse (cross-platform recode, Phase 3f): the cabin's Feed and Ideas from
//! the same `updates.json`, ranked by the same rules in `grokhub_core::pulse`.
//! Dismiss, Not this, Love and Accept write the same MEMORY.md ledger line
//! the cabin writes, so the next ranking learns from them. Love and Discuss
//! also feed the writer's taste summary (`pulse_feed`); Delete only removes.
//! Ideas take Let's do it (Accept), More like this and Not interested; the
//! last two move the interest topics the idea names and steer the next idea
//! run.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use grokhub_core::interests::InterestSignal;
use grokhub_core::pulse::{self as pc, LedgerReason};
use grokhub_core::pulse_writer::TasteKind;
use grokhub_core::reactions::{self, Outcome};
use grokhub_core::{CardReaction, UpdateCard, UpdateKind};

use crate::pulse_feed;
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
    /// A written post's topic emoji. Empty on older posts.
    pub icon: String,
    /// A written post's "why you're seeing this" line, to the reader.
    pub why: String,
    /// Cached image files for the gallery, read with `pulse_image`.
    pub images: Vec<String>,
    /// Unix ms, for the menu's full date.
    pub created_at: u64,
    pub ago: String,
    /// The first source link the research step returned.
    pub link: String,
    pub loved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Idea {
    pub id: String,
    /// The bold line: a written idea's own title, else "I can …".
    pub title: String,
    /// A written idea's topic emoji. Empty on older ideas.
    pub icon: String,
    pub summary: String,
    /// Do, Automate, Learn or Watch.
    pub kind: String,
    /// What Let's do it puts in the chat box.
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
    /// Older pages still send "like".
    #[serde(alias = "like")]
    Love,
    /// Takes the post away. Not a taste signal: nothing is noted.
    Delete,
    Snooze,
    /// Let's do it (or Run) took the idea to chat.
    Accept,
    /// More like this: the next idea run writes more in its spirit.
    MoreLike,
    /// Not interested: the idea goes, and so does some of its topics' weight.
    NotInterested,
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

/// The Pulse ledger's own file beside MEMORY.md, shared with the cabin.
fn ledger_path() -> PathBuf {
    grokhub_agent::global_memory_path().with_file_name(pc::LEDGER_FILE)
}

/// The ledger file, with any lines an older build left in MEMORY.md in front.
/// The cabin moves those over; the shell only reads them.
fn ledger() -> Vec<pc::LedgerEntry> {
    let own = std::fs::read_to_string(ledger_path()).unwrap_or_default();
    let notes = std::fs::read_to_string(grokhub_agent::global_memory_path()).unwrap_or_default();
    let (_, older) = pc::split_ledger(&notes);
    pc::parse_ledger(&pc::merge_ledger(&own, &older))
}

/// Appends one ledger line through a temp file and a rename, so a crash never
/// leaves the ledger half written. A line that would carry a secret is left
/// out, as the cabin's `append_memory` does; the card action still goes on.
fn note(title: &str, reason: LedgerReason, date: &str) -> Result<(), String> {
    let line = pc::ledger_line(date, title, reason);
    if !grokhub_core::is_plain_text(&line) {
        return Ok(());
    }
    let path = ledger_path();
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!(
            "The Pulse ledger is a link to another file. {} does not follow it.",
            grokhub_core::brand::product_name()
        ));
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > NOTES_CAP) {
        return Err("The Pulse ledger is too large to add to. Nothing was changed.".into());
    }
    let mut body = std::fs::read_to_string(&path).unwrap_or_default();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(&line);
    body.push('\n');
    let parent = path.parent().ok_or("The Pulse ledger has no folder.")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(
        "{}.shell-{}.tmp",
        pc::LEDGER_FILE,
        std::process::id()
    ));
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Every card in the store, for the writer's repeat and seen checks.
pub fn cards(config_dir: &Path) -> Result<Vec<UpdateCard>, String> {
    let _store = store();
    load_unlocked(config_dir)
}

/// Add one written post.
pub fn add_post(config_dir: &Path, card: UpdateCard) -> Result<bool, String> {
    let _store = store();
    let mut cards = load_unlocked(config_dir)?;
    let added = grokhub_core::post_update(&mut cards, card);
    if added {
        save(config_dir, &cards)?;
    }
    Ok(added)
}

/// Add one idea run's ideas. Returns how many landed.
pub fn add_ideas(
    config_dir: &Path,
    ideas: &[grokhub_core::pulse_ideas::MuseIdea],
    now_ms: u64,
) -> Result<usize, String> {
    let _store = store();
    let mut cards = load_unlocked(config_dir)?;
    let added = grokhub_core::pulse_ideas::post_ideas(&mut cards, ideas, now_ms);
    if added > 0 {
        save(config_dir, &cards)?;
    }
    Ok(added)
}

fn post(config_dir: &Path, card: &UpdateCard, now_ms: u64) -> Post {
    let images = pulse_feed::images_dir(config_dir);
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
        icon: card.pulse.icon.clone().unwrap_or_default(),
        why: card.why.clone().unwrap_or_default(),
        images: card
            .pulse
            .image_urls
            .iter()
            .map(|url| pc::image_cache_name(url))
            .filter(|name| images.join(name).is_file())
            .collect(),
        created_at: card.created_at,
        ago: pc::ago_label(card.created_at, now_ms),
        link: card
            .citations
            .iter()
            .find(|url| grokhub_core::public_http_url(url))
            .cloned()
            .unwrap_or_default(),
        loved: card.reaction == Some(CardReaction::Up),
    }
}

fn idea(card: &UpdateCard) -> Idea {
    let action = card.idea_action();
    Idea {
        id: card.id.clone(),
        title: pc::i_can_title(card),
        icon: card.pulse.icon.clone().unwrap_or_default(),
        summary: card.body.clone().unwrap_or_default().trim().to_string(),
        kind: pc::pulse_type(card).label().to_string(),
        action: if action.is_empty() {
            card.title.trim().to_string()
        } else {
            action
        },
    }
}

fn reactions_path(config_dir: &Path) -> PathBuf {
    config_dir.join(reactions::FILE_NAME)
}

/// Note what the person did with a card in what Pulse learned. `shift` is
/// More (`Some(true)`) or Less (`Some(false)`) like this on top of the outcome.
fn react(config_dir: &Path, card: &UpdateCard, outcome: Outcome, shift: Option<bool>, now_ms: u64) {
    let Some((category, tag)) = reactions::card_category_tag(card) else {
        return;
    };
    let path = reactions_path(config_dir);
    let mut learned = reactions::load(&path);
    learned.record(outcome, &category, &tag, now_ms);
    match shift {
        Some(true) => learned.more(&category),
        Some(false) => learned.less(&category),
        None => {}
    }
    if let Err(e) = reactions::save(&path, &learned) {
        eprintln!("pulse: {e}");
    }
}

fn page(config_dir: &Path, cards: &[UpdateCard], now_ms: u64) -> Page {
    let ledger = ledger();
    let mut learned = reactions::load(&reactions_path(config_dir));
    let before = learned.clone();
    learned.sweep_ignored(cards, now_ms);
    if learned != before {
        if let Err(e) = reactions::save(&reactions_path(config_dir), &learned) {
            eprintln!("pulse: {e}");
        }
    }
    let inputs = pc::PulseInputs {
        ledger: &ledger,
        now_ms,
        reactions: Some(&learned),
        ..Default::default()
    };
    let by_id = |id: &str| cards.iter().find(|c| c.id == id);
    Page {
        feed: pc::feed_posts(cards, &inputs)
            .iter()
            .map(|c| post(config_dir, c, now_ms))
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

/// Opening Pulse counts as engagement for the writer's cadence; the page's
/// clock, when it sends one, places quiet hours.
pub fn load(config_dir: &Path, day: Option<&Day>, now_ms: u64) -> Result<Page, String> {
    pulse_feed::with_state(config_dir, |s| {
        s.note_engaged(now_ms);
        if let Some(day) = day {
            s.utc_offset_min = Some(grokhub_core::pulse_writer::utc_offset_min(
                day.hour, day.minute, now_ms,
            ));
        }
    })?;
    let _store = store();
    let mut cards = load_unlocked(config_dir)?;
    let collapsed = grokhub_core::collapse_feed(&mut cards);
    if grokhub_core::expire_ideas(&mut cards, now_ms) > 0 || collapsed {
        save(config_dir, &cards)?;
    }
    Ok(page(config_dir, &cards, now_ms))
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
    let dismiss = if matches!(act, Act::Dismiss) {
        Some(pc::dismiss_kind(&card, 0))
    } else {
        None
    };
    let (outcome, shift) = match act {
        Act::Dismiss if dismiss == Some(pc::DismissKind::UsedDone) => {
            // Used / liked / discussed then dismissed: no negative reaction.
            (None, None)
        }
        Act::Dismiss | Act::Delete => (Some(Outcome::Dismissed), None),
        Act::NotThis | Act::NotInterested => (Some(Outcome::Dismissed), Some(false)),
        Act::Love | Act::Accept => (Some(Outcome::Accepted), None),
        Act::MoreLike => (Some(Outcome::Accepted), Some(true)),
        Act::Snooze => (Some(Outcome::Snoozed), None),
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
            let reason = dismiss
                .unwrap_or(pc::DismissKind::NotInterested)
                .ledger_reason();
            note(&card.title, reason, &day.date)?;
            remove(&mut cards)
        }
        Act::NotThis => {
            note(&card.title, LedgerReason::NotThis, &day.date)?;
            remove(&mut cards)
        }
        Act::Love => {
            if card.reaction == Some(CardReaction::Up) {
                false
            } else {
                note(&card.title, LedgerReason::Liked, &day.date)?;
                pulse_feed::with_state(config_dir, |s| {
                    s.note_engaged(now_ms);
                    s.note_taste(TasteKind::Love, &card.title, now_ms);
                })?;
                if let Some(saved) = cards.iter_mut().find(|c| c.id == id) {
                    saved.reaction = Some(CardReaction::Up);
                }
                true
            }
        }
        Act::Delete => {
            let before = cards.len();
            cards.retain(|c| c.id != id);
            cards.len() != before
        }
        Act::Snooze => {
            let until = pc::snooze_until(now_ms, day.hour, day.minute);
            pc::snooze_card(&mut cards, id, until)
        }
        Act::Accept => {
            note(&card.title, LedgerReason::Accepted, &day.date)?;
            grokhub_core::mark_update_opened(&mut cards, id)
        }
        Act::MoreLike => {
            note(&card.title, LedgerReason::Liked, &day.date)?;
            idea_feedback(config_dir, &card, InterestSignal::MoreLikeThis, now_ms)?;
            pulse_feed::with_ideas_state(config_dir, |s| s.note_more_like(&card.title))?;
            false
        }
        Act::NotInterested => {
            note(&card.title, LedgerReason::NotThis, &day.date)?;
            idea_feedback(config_dir, &card, InterestSignal::NotInterested, now_ms)?;
            pulse_feed::with_ideas_state(config_dir, |s| s.note_not_interested(&card.title))?;
            remove(&mut cards)
        }
    };
    // After the ledger note: a refused note must leave taste learning alone
    // too, so "Nothing was changed" stays true.
    if let Some(outcome) = outcome {
        react(config_dir, &card, outcome, shift, now_ms);
    }
    if changed {
        save(config_dir, &cards)?;
    }
    Ok(page(config_dir, &cards, now_ms))
}

/// Move the interest topics an idea's text names.
fn idea_feedback(
    config_dir: &Path,
    card: &UpdateCard,
    signal: InterestSignal,
    now_ms: u64,
) -> Result<(), String> {
    let text = format!(
        "{} {} {}",
        card.title,
        card.body.as_deref().unwrap_or(""),
        card.idea_action()
    );
    crate::interests::feedback(config_dir, &text, signal, now_ms).map(|_| ())
}

fn find(config_dir: &Path, id: &str) -> Result<UpdateCard, String> {
    cards(config_dir)?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "That card is gone. The page now shows what is left.".to_string())
}

/// Discuss: the chat seed carrying the post. Counts as engagement and taste.
pub fn discuss(config_dir: &Path, id: &str, now_ms: u64) -> Result<String, String> {
    let card = find(config_dir, id)?;
    react(config_dir, &card, Outcome::Tapped, None, now_ms);
    pulse_feed::with_state(config_dir, |s| {
        s.note_engaged(now_ms);
        s.note_taste(TasteKind::Discuss, &card.title, now_ms);
    })?;
    Ok(grokhub_core::pulse_writer::discuss_seed(&card))
}

/// Why this post is in the feed: the writer's line, else the interest it
/// matched, then what Pulse learned about its kind.
pub fn why(config_dir: &Path, id: &str) -> Result<String, String> {
    let card = find(config_dir, id)?;
    let learned = reactions::card_category_tag(&card).map(|(category, tag)| {
        let state = reactions::load(&reactions_path(config_dir));
        reactions::why_line(&state, &category, &tag)
    });
    let because = card
        .why
        .clone()
        .or_else(|| {
            card.pulse
                .because
                .clone()
                .map(|b| format!("It matched your interest in {b}."))
        })
        .unwrap_or_else(|| "It came from your Pulse sources.".into());
    Ok(match learned {
        Some(line) => format!("{because} {line}."),
        None => because,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub text: String,
    /// True while the person has not written their own.
    pub is_default: bool,
    /// What the writer's last run did.
    pub note: String,
}

pub fn get_prompt(config_dir: &Path) -> Prompt {
    let state = pulse_feed::state(config_dir);
    Prompt {
        text: state.prompt_or_default().to_string(),
        is_default: state.prompt.trim().is_empty(),
        note: state.note,
    }
}

/// Save the feed prompt and owe one new post right away.
pub fn save_prompt(config_dir: &Path, text: &str) -> Result<Prompt, String> {
    pulse_feed::with_state(config_dir, |s| s.save_prompt(text))?;
    pulse_feed::wake();
    Ok(get_prompt(config_dir))
}

/// A cached gallery image as a data URL, so no page (desktop or web) ever
/// loads it from the source site. Only names the writer made are read.
pub fn image(config_dir: &Path, name: &str) -> Result<String, String> {
    let (stem, ext) = name.split_once('.').ok_or("Not a Pulse image.")?;
    let mime = match ext {
        "jpg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => return Err("Not a Pulse image.".into()),
    };
    if stem.len() != 16 || !stem.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Not a Pulse image.".into());
    }
    let bytes = std::fs::read(pulse_feed::images_dir(config_dir).join(name))
        .map_err(|_| "That image is no longer cached.".to_string())?;
    use base64::Engine as _;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
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

    /// The Pulse ledger file.
    fn memory() -> String {
        std::fs::read_to_string(ledger_path()).unwrap_or_default()
    }

    /// MEMORY.md itself.
    fn notes() -> String {
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
        let page = load(&d.config, None, NOW).expect("load");
        assert_eq!(page.feed.len(), 1, "{page:?}");
        let post = &page.feed[0];
        assert_eq!(post.id, "post-1");
        assert_eq!(post.source, "Automation");
        assert_eq!(post.ago, "2h ago");
        assert_eq!(post.link, "https://example.com/board");
        assert!(!post.loved);

        let ideas: Vec<&Idea> = page.ideas.iter().flat_map(|g| &g.ideas).collect();
        assert_eq!(ideas.len(), 1, "{page:?}");
        assert_eq!(ideas[0].id, "idea-1");
        assert!(ideas[0].title.starts_with("I can "), "{:?}", ideas[0]);
        assert_eq!(ideas[0].action, "close every done card on the workboard");
        assert_eq!(
            page.ideas[0].label, "",
            "the first three ideas lead with no header"
        );
    }

    fn rust_topic(config: &Path) {
        let mut profile = grokhub_core::interests::InterestProfile::default();
        profile.add(
            "workboard",
            grokhub_core::interests::InterestSource::Explicit,
            NOW,
        );
        grokhub_core::interests::save(&config.join(grokhub_core::interests::FILE_NAME), &profile)
            .expect("interests");
    }

    fn weight(config: &Path) -> u8 {
        grokhub_core::interests::load(&config.join(grokhub_core::interests::FILE_NAME))
            .find("workboard")
            .expect("topic")
            .weight
    }

    #[test]
    fn not_interested_removes_the_idea_and_lowers_its_topic() {
        let d = dirs("not-interested", &sample());
        rust_topic(&d.config);
        let before = weight(&d.config);
        let page = act(&d.config, "idea-1", Act::NotInterested, &day(), NOW).expect("act");
        assert!(page.ideas.is_empty(), "{page:?}");
        assert_eq!(weight(&d.config), before - 15);
        assert!(memory().contains("pulse: dismissed \"Tidy the workboard\" reason=not-this"));
        let state = pulse_feed::with_ideas_state(&d.config, |s| s.clone()).expect("state");
        assert_eq!(state.turned_down, vec!["Tidy the workboard"]);
    }

    #[test]
    fn more_like_this_keeps_the_idea_and_is_kept_for_the_next_run() {
        let d = dirs("more-like", &sample());
        rust_topic(&d.config);
        let before = weight(&d.config);
        let page = act(&d.config, "idea-1", Act::MoreLike, &day(), NOW).expect("act");
        assert_eq!(
            page.ideas.iter().flat_map(|g| &g.ideas).count(),
            1,
            "{page:?}"
        );
        assert_eq!(weight(&d.config), before + 12);
        let state = pulse_feed::with_ideas_state(&d.config, |s| s.clone()).expect("state");
        assert_eq!(state.more_like, vec!["Tidy the workboard"]);
        let acts: Vec<Act> = ["\"moreLike\"", "\"notInterested\"", "\"accept\""]
            .iter()
            .map(|raw| serde_json::from_str(raw).expect("act"))
            .collect();
        assert_eq!(acts, vec![Act::MoreLike, Act::NotInterested, Act::Accept]);
    }

    #[test]
    fn acts_teach_pulse_and_why_says_what_it_learned() {
        let d = dirs("learn", &sample());
        act(&d.config, "idea-1", Act::MoreLike, &day(), NOW).expect("more");
        let learned = reactions::load(&d.config.join(reactions::FILE_NAME));
        // Accepted (+2) and More like this (+3).
        assert_eq!(learned.score("productivity ideas"), 5, "{learned:?}");
        assert_eq!(
            why(&d.config, "idea-1").expect("why"),
            "It came from your Pulse sources. From Pulse ideas · productivity ideas +5."
        );
        act(&d.config, "idea-1", Act::Snooze, &day(), NOW).expect("snooze");
        let learned = reactions::load(&d.config.join(reactions::FILE_NAME));
        assert_eq!(learned.score("productivity ideas"), 4);
        // A post Pulse has no category for leaves nothing behind.
        act(&d.config, "post-1", Act::Love, &day(), NOW).expect("love");
        let learned = reactions::load(&d.config.join(reactions::FILE_NAME));
        assert_eq!(learned.log.len(), 2);
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
    fn a_refused_ledger_note_leaves_taste_learning_and_the_card_alone() {
        let d = dirs("refused", &sample());
        let path = ledger_path();
        std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir");
        std::fs::write(&path, vec![b'-'; NOTES_CAP as usize + 1]).expect("big ledger");
        let err = act(&d.config, "idea-1", Act::NotThis, &day(), NOW).expect_err("refused");
        assert_eq!(
            err,
            "The Pulse ledger is too large to add to. Nothing was changed."
        );
        assert!(
            !reactions_path(&d.config).exists(),
            "a refused note must not score the card"
        );
        let saved = std::fs::read_to_string(d.config.join(FILE)).expect("read");
        assert!(saved.contains("idea-1"), "{saved}");
    }

    #[test]
    fn dismiss_like_and_snooze_change_the_post() {
        let d = dirs("post", &sample());
        let loved = act(&d.config, "post-1", Act::Love, &day(), NOW).expect("love");
        assert!(loved.feed[0].loved);
        // A second Love adds no second line.
        act(&d.config, "post-1", Act::Love, &day(), NOW).expect("love again");
        assert_eq!(
            memory(),
            "- [2026-10-25] pulse: liked \"Board\" reason=liked\n"
        );

        let snoozed = act(&d.config, "post-1", Act::Snooze, &day(), NOW).expect("snooze");
        assert!(snoozed.feed.is_empty(), "{snoozed:?}");
        // Back the next morning at nine.
        let tomorrow = pc::snooze_until(NOW, 14, 30);
        assert_eq!(load(&d.config, None, tomorrow).expect("load").feed.len(), 1);

        let gone = act(&d.config, "post-1", Act::Dismiss, &day(), tomorrow).expect("dismiss");
        assert!(gone.feed.is_empty());
        // Liked then dismissed is used/done, not a bury (#969 / #972).
        // Compare on LF: Windows checkouts may leave CRLF in the ledger.
        let mem = memory().replace("\r\n", "\n");
        assert!(
            mem.ends_with("pulse: used \"Board\" reason=used\n"),
            "{mem:?}"
        );
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
        std::fs::write(
            grokhub_agent::global_memory_path(),
            "- earlier note\n- [2026-10-24] pulse: liked \"Old board\" reason=liked\n",
        )
        .expect("seed");
        let page = act(&d.config, "post-1", Act::Dismiss, &day(), NOW).expect("dismiss");
        assert!(page.feed.is_empty(), "{page:?}");
        assert_eq!(memory(), "");

        act(&d.config, "idea-1", Act::Accept, &day(), NOW).expect("accept");
        assert_eq!(
            memory(),
            "- [2026-10-25] pulse: accepted \"Tidy the workboard\" reason=accepted\n"
        );
        assert_eq!(
            notes(),
            "- earlier note\n- [2026-10-24] pulse: liked \"Old board\" reason=liked\n",
            "the shell never rewrites MEMORY.md"
        );
        let titles: Vec<String> = ledger().into_iter().map(|e| e.title).collect();
        assert_eq!(
            titles,
            ["Old board", "Tidy the workboard"],
            "a line still in MEMORY.md reads in front"
        );
        let dir = grokhub_agent::global_memory_path()
            .parent()
            .unwrap()
            .to_path_buf();
        let mut left: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec!["MEMORY.md".to_string(), pc::LEDGER_FILE.to_string()],
            "no temp file is left"
        );
    }

    #[test]
    fn no_feed_file_is_an_empty_page() {
        let d = dirs("empty", &[]);
        std::fs::remove_file(d.config.join(FILE)).expect("rm");
        assert_eq!(
            load(&d.config, None, NOW),
            Ok(Page {
                feed: Vec::new(),
                ideas: Vec::new()
            })
        );
    }

    #[test]
    fn love_and_discuss_feed_the_taste_summary_and_delete_does_not() {
        let d = dirs("taste", &sample());
        let page = act(&d.config, "post-1", Act::Love, &day(), NOW).expect("love");
        assert!(page.feed[0].loved);
        let seed = discuss(&d.config, "post-1", NOW + 1).expect("discuss");
        assert_eq!(
            seed,
            "Let's talk about this post from my Pulse feed.\n\n**Board**\nsummarized 3 cards\n\nSource: https://example.com/board\n"
        );
        let state = pulse_feed::state(&d.config);
        let kinds: Vec<(TasteKind, &str)> = state
            .taste_events
            .iter()
            .map(|e| (e.kind, e.text.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![(TasteKind::Love, "Board"), (TasteKind::Discuss, "Board")]
        );
        assert_eq!(state.last_engaged_ms, NOW + 1);

        let before = memory();
        let page = act(&d.config, "post-1", Act::Delete, &day(), NOW + 2).expect("delete");
        assert!(page.feed.is_empty(), "{page:?}");
        assert_eq!(memory(), before, "Delete writes no ledger line");
        assert_eq!(pulse_feed::state(&d.config).taste_events.len(), 2);
        let saved = std::fs::read_to_string(d.config.join(FILE)).expect("read");
        assert!(!saved.contains("post-1"), "{saved}");
        assert_eq!(
            discuss(&d.config, "post-1", NOW),
            Err("That card is gone. The page now shows what is left.".to_string())
        );
    }

    #[test]
    fn opening_pulse_counts_as_engagement_and_sets_the_clock() {
        let d = dirs("open", &sample());
        let noon = NOW - NOW % 86_400_000 + 12 * 3_600_000;
        load(&d.config, Some(&day()), noon).expect("load");
        let state = pulse_feed::state(&d.config);
        assert_eq!(state.last_engaged_ms, noon);
        // The page said 14:30 at 12:00 UTC.
        assert_eq!(state.utc_offset_min, Some(150));
    }

    #[test]
    fn the_prompt_saves_once_and_owes_one_post() {
        let d = dirs("prompt", &[]);
        let first = get_prompt(&d.config);
        assert!(first.is_default);
        assert_eq!(first.text, pc::DEFAULT_FEED_INSTRUCTIONS);
        let saved = save_prompt(&d.config, "  Only Rust news.  ").expect("save");
        assert_eq!(
            saved,
            Prompt {
                text: "Only Rust news.".into(),
                is_default: false,
                note: String::new()
            }
        );
        let state = pulse_feed::state(&d.config);
        assert!(state.queued);
    }

    #[test]
    fn why_prefers_the_writers_line() {
        let mut cards = sample();
        cards[1].why = Some("You run this board every day.".into());
        let d = dirs("why", &cards);
        assert_eq!(
            why(&d.config, "post-1"),
            Ok("You run this board every day.".to_string())
        );
        // An automation post keeps its own line.
        let d = dirs("why-run", &sample());
        assert_eq!(
            why(&d.config, "post-1"),
            Ok("Your automation “Board” finished.".to_string())
        );
        // A source post with neither line names the interest it matched.
        let mut cards = sample();
        cards[1].why = None;
        cards[1].pulse.because = Some("rust".into());
        let d = dirs("why-source", &cards);
        assert_eq!(
            why(&d.config, "post-1"),
            Ok("It matched your interest in rust.".to_string())
        );
    }

    #[test]
    fn images_are_read_from_the_cache_by_their_own_names_only() {
        let mut cards = sample();
        let url = "https://example.com/hero.jpg";
        cards[1].pulse.image_urls = vec![url.into(), "https://example.com/missing.png".into()];
        let d = dirs("images", &cards);
        let name = pc::image_cache_name(url);
        let dir = pulse_feed::images_dir(&d.config);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(&name), [0xFF, 0xD8, 0xFF]).expect("img");
        let page = load(&d.config, None, NOW).expect("load");
        assert_eq!(page.feed[0].images, vec![name.clone()], "only cached ones");
        assert_eq!(
            image(&d.config, &name),
            Ok("data:image/jpeg;base64,/9j/".to_string())
        );
        assert!(image(&d.config, "../updates.json").is_err());
        assert!(image(&d.config, "0123456789abcdef.svg").is_err());
    }
}
