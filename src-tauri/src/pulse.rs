//! Pulse (cross-platform recode, Phase 3f): the cabin's Feed and Ideas from
//! the same `updates.json`, ranked by the same rules in `grokhub_core::pulse`.
//! Not this, Love and Accept write the same MEMORY.md ledger line the cabin
//! writes, so the next ranking learns from them. Like (Love) and Not interested
//! also train card taste (`grokhub_core::card_taste`), which orders the feed
//! and steers the writer. Love and Discuss feed the writer's taste summary
//! (`pulse_feed`). Dismiss and Delete only remove: they carry no weight.
//! Ideas take Let's do it (Accept), More like this and Not interested; the
//! last two move the interest topics the idea names and steer the next idea
//! run.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use grokhub_core::card_chips::{card_chips, remind_later, ChipAct, DayShape};
use grokhub_core::card_report::{card_context, card_headline, card_summary, report_more};
use grokhub_core::card_taste::{self, Reaction};
use grokhub_core::interests::InterestSignal;
use grokhub_core::pc_health_advice as advice;
use grokhub_core::pulse::{self as pc, LedgerReason};
use grokhub_core::pulse_sources::{self as ps, SavedFind};
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

/// Saved Finds: their own file, shared with the cabin. The feed never reads it.
fn saved_path(config_dir: &Path) -> PathBuf {
    config_dir.join(ps::SAVED_FILE)
}

/// One card on the Saved page, as the page reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub title: String,
    pub url: String,
    pub source_name: String,
    pub summary: String,
    pub because: Option<String>,
    pub saved_ms: u64,
}

fn saved_view(list: Vec<SavedFind>) -> Vec<Saved> {
    list.into_iter()
        .map(|f| Saved {
            title: f.title,
            url: f.url,
            source_name: f.source_name,
            summary: f.summary,
            because: f.because,
            saved_ms: f.saved_ms,
        })
        .collect()
}

/// The Saved page: every saved find, newest first.
pub fn saved(config_dir: &Path) -> Vec<Saved> {
    let _store = store();
    saved_view(ps::load_saved(&saved_path(config_dir)))
}

/// Remove from saved, the Saved page's one action. Returns what is left.
pub fn unsave(config_dir: &Path, url: &str) -> Result<Vec<Saved>, String> {
    let _store = store();
    let path = saved_path(config_dir);
    let mut list = ps::load_saved(&path);
    if ps::remove_find(&mut list, url) {
        ps::save_saved(&path, &list)?;
    }
    Ok(saved_view(list))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub id: String,
    /// "Automation", or the source's own name.
    pub source: String,
    pub title: String,
    pub summary: String,
    /// Why a fetched post is here, in words ("You play Helldivers 2").
    /// Empty on other posts.
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
    /// One line on what the card's numbers mean, when it has one.
    pub verdict: Option<Verdict>,
    /// The full report and the facts behind it, as markdown, for the card's
    /// Show the full report. Empty when the card has nothing past its summary.
    pub more: String,
    /// A digest's updates, each with the card it opens.
    pub items: Vec<Item>,
    /// The quick chips under the card: its fixes, Discuss, Ignore for now,
    /// Remind me later.
    pub chips: Vec<Chip>,
    /// Offer Snooze in the row: the card is tied to a time, or it is unclear.
    pub snooze: bool,
    /// Offer Delete in the row: the card is stale, a repeat or a finished
    /// one-off, or it is unclear.
    pub delete: bool,
    /// Save in the row: a source post with a link (it goes to Saved).
    pub save: bool,
    /// Its link is already on the Saved page.
    pub saved: bool,
}

/// One update inside a digest card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub card_id: String,
    pub found: String,
    pub summary: String,
}

/// One chip under a card. `act` is "offer", "discuss", "ignore" or "remind";
/// `index` is the offer's place on the card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chip {
    pub label: String,
    pub act: &'static str,
    pub index: usize,
}

fn chips(card: &UpdateCard, now_ms: u64) -> Vec<Chip> {
    // Only the kinds of chip matter here; Remind me later's time is worked
    // out from the page's clock when it is tapped.
    let day = DayShape {
        now_ms,
        hour: 0,
        minute: 0,
        wind_down: None,
        start: None,
    };
    card_chips(card, &day, None)
        .into_iter()
        .map(|c| {
            let (act, index) = match c.act {
                ChipAct::Offer(i) => ("offer", i),
                ChipAct::Discuss | ChipAct::Primary => ("discuss", 0),
                ChipAct::Ignore => ("ignore", 0),
                ChipAct::RemindAt(_) => ("remind", 0),
            };
            Chip {
                label: c.label,
                act,
                index,
            }
        })
        .collect()
}

/// A card's one-line read of its own numbers, with how worried to be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    /// "fine", "watch" or "act".
    pub tone: &'static str,
    pub line: String,
}

fn verdict(card: &UpdateCard) -> Option<Verdict> {
    card.pulse.verdict.as_ref().map(|v| Verdict {
        tone: match v.tone {
            pc::VerdictTone::Fine => "fine",
            pc::VerdictTone::Watch => "watch",
            pc::VerdictTone::Act => "act",
        },
        line: v.line.clone(),
    })
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
    pub verdict: Option<Verdict>,
    /// "Fix it" or "Options" on a PC health card, whose main button opens
    /// the options window instead of Let's do it. Empty on other ideas.
    pub fix: String,
    /// A card that found something: its full report, opened under the summary.
    pub more: String,
    /// A card that found something: its chips in place of the two buttons.
    /// Empty on other ideas.
    pub chips: Vec<Chip>,
}

/// A PC health card's options window: the verdict, what to try, and the
/// last few sessions side by side. Read only: settings rows stay in the
/// desktop app, which owns them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixView {
    pub title: String,
    pub verdict: Option<Verdict>,
    pub options: Vec<advice::FixOption>,
    pub compare: Vec<advice::CompareRow>,
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
    /// How many of `feed` are today's set; "That's all for today" follows.
    pub today: usize,
    pub ideas: Vec<Group>,
    /// What the feed writer's last run did, for an empty feed.
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Act {
    /// Off the page. Carries no weight: nothing is noted or learned.
    Dismiss,
    /// Not interested: off the page, and card taste learns from its traits.
    Dislike,
    NotThis,
    /// Older pages still send "like".
    #[serde(alias = "like")]
    Love,
    /// Love pressed again: the heart goes. Writes no ledger line.
    Unlove,
    /// Takes the post away. A small "less of this" in card taste unless
    /// the card was stale or a finished one-off (then it is housekeeping).
    Delete,
    /// Hidden until 9 AM; a small "wanted, just not now" in card taste.
    Snooze,
    /// Read it: the post's source opened. Counts as engagement.
    Opened,
    /// Let's do it (or Run) took the idea to chat.
    Accept,
    /// More like this: the next idea run writes more in its spirit.
    MoreLike,
    /// Not interested: the idea goes, and so does some of its topics' weight.
    NotInterested,
    /// Ignore for now: off the page, with no "less of this".
    Ignore,
    /// Save: the post goes to the Saved page (and counts as a Like).
    Save,
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

fn post(
    config_dir: &Path,
    card: &UpdateCard,
    shown: &[UpdateCard],
    saved: &[SavedFind],
    now_ms: u64,
) -> Post {
    let save = ps::is_source_post(card) && ps::saved_from_card(card, now_ms).is_some();
    let saved = save && card.citations.first().is_some_and(|url| ps::is_saved(saved, url));
    let offers = grokhub_core::card_offers::offers(card, shown, now_ms);
    let images = pulse_feed::images_dir(config_dir);
    let source = card
        .pulse
        .source_name
        .clone()
        .unwrap_or_else(|| card.kind.label().to_string());
    Post {
        id: card.id.clone(),
        source,
        title: card_headline(card),
        summary: card_summary(card),
        because: pc::plain_why(card).unwrap_or_default(),
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
        verdict: verdict(card),
        more: report_more(card).unwrap_or_default(),
        items: card
            .pulse
            .report
            .iter()
            .flat_map(|r| r.items.iter())
            .map(|i| Item {
                card_id: i.card_id.clone(),
                found: i.found.trim().to_string(),
                summary: i.summary.trim().to_string(),
            })
            .collect(),
        chips: chips(card, now_ms),
        snooze: offers.snooze,
        delete: offers.delete,
        save,
        saved,
    }
}

fn idea(card: &UpdateCard, now_ms: u64) -> Idea {
    let action = card.idea_action();
    let reported = card.pulse.report.is_some();
    Idea {
        id: card.id.clone(),
        title: pc::i_can_title(card),
        icon: card.pulse.icon.clone().unwrap_or_default(),
        summary: if reported {
            card_summary(card)
        } else {
            card.body.clone().unwrap_or_default().trim().to_string()
        },
        kind: pc::pulse_type(card).label().to_string(),
        action: if action.is_empty() {
            card.title.trim().to_string()
        } else {
            action
        },
        verdict: verdict(card),
        fix: advice::fix_label(card).unwrap_or_default().to_string(),
        more: report_more(card).unwrap_or_default(),
        chips: if reported {
            chips(card, now_ms)
        } else {
            Vec::new()
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
    let mut learned = reactions::load(&reactions_path(config_dir));
    let before = learned.clone();
    learned.sweep_ignored(cards, now_ms);
    if learned != before {
        if let Err(e) = reactions::save(&reactions_path(config_dir), &learned) {
            eprintln!("pulse: {e}");
        }
    }
    build(config_dir, cards, now_ms, &learned)
}

/// Home's Pulse tile: the page as it stands. Unlike opening Pulse, it counts
/// no visit, expires nothing and notes no card as ignored.
pub fn peek(config_dir: &Path, now_ms: u64) -> Result<Page, String> {
    let cards = cards(config_dir)?;
    let learned = reactions::load(&reactions_path(config_dir));
    Ok(build(config_dir, &cards, now_ms, &learned))
}

fn build(
    config_dir: &Path,
    cards: &[UpdateCard],
    now_ms: u64,
    learned: &reactions::Reactions,
) -> Page {
    let ledger = ledger();
    let inputs = pc::PulseInputs {
        ledger: &ledger,
        now_ms,
        reactions: Some(learned),
        ..Default::default()
    };
    let by_id = |id: &str| cards.iter().find(|c| c.id == id);
    let (today, older) = pc::feed_day(cards, &inputs);
    // Card taste: liked kinds rise, disliked kinds sink, the worst leave.
    let weights = card_taste::cached_weights(config_dir);
    let (today, older) = (
        card_taste::rank_with(today, &weights),
        card_taste::rank_with(older, &weights),
    );
    let shown: Vec<UpdateCard> = today.iter().chain(older.iter()).cloned().collect();
    let saved_list = ps::load_saved(&saved_path(config_dir));
    Page {
        today: today.len(),
        feed: shown
            .iter()
            .map(|c| post(config_dir, c, &shown, &saved_list, now_ms))
            .collect(),
        note: pulse_feed::state(config_dir).note,
        ideas: pc::group_ideas(cards, &inputs)
            .into_iter()
            .map(|(cat, rows)| Group {
                label: cat.map(|c| c.label().to_string()).unwrap_or_default(),
                ideas: rows
                    .iter()
                    .filter_map(|r| by_id(&r.id))
                    .map(|c| idea(c, now_ms))
                    .collect(),
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

/// Not this or Dismiss on a feed post tells the writer's taste note "less
/// of this". Ideas learn through their own state instead.
fn note_less(config_dir: &Path, card: &UpdateCard, now_ms: u64) -> Result<(), String> {
    if card.kind == UpdateKind::Idea {
        return Ok(());
    }
    pulse_feed::with_state(config_dir, |s| {
        s.note_taste(TasteKind::Less, &card.title, now_ms);
    })
}

/// Note a Like or Not interested in card taste, or take a Like back.
fn learn_taste(
    config_dir: &Path,
    card: &UpdateCard,
    add: Option<Reaction>,
    undo: Option<Reaction>,
    now_ms: u64,
) -> Result<(), String> {
    let mut taste = card_taste::load(config_dir);
    if let Some(r) = undo {
        taste.undo(&card.id, r);
    }
    if let Some(r) = add {
        taste.record(&card.id, card_taste::card_traits(card), r, now_ms);
    }
    card_taste::save(config_dir, &taste)
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
    let (outcome, shift) = match act {
        // Dismiss carries no weight; Not interested trains card taste only, so
        // one or two clicks stay small.
        Act::Dismiss | Act::Dislike => (None, None),
        Act::Delete => (Some(Outcome::Dismissed), None),
        Act::NotThis | Act::NotInterested => (Some(Outcome::Dismissed), Some(false)),
        Act::Love | Act::Accept | Act::Save => (Some(Outcome::Accepted), None),
        Act::Unlove => (None, None),
        Act::MoreLike => (Some(Outcome::Accepted), Some(true)),
        Act::Snooze => (Some(Outcome::Snoozed), None),
        // Opened goes through `engaged`, which notes the tap itself.
        Act::Ignore | Act::Opened => (None, None),
    };
    let remove = |cards: &mut Vec<UpdateCard>| {
        if card.kind == UpdateKind::Idea {
            grokhub_core::dismiss_idea(cards, id)
        } else {
            grokhub_core::dismiss_update_at(cards, id, now_ms)
        }
    };
    let changed = match act {
        Act::Dismiss => remove(&mut cards),
        Act::Dislike => {
            if card.kind != UpdateKind::Idea {
                learn_taste(config_dir, &card, Some(Reaction::Dislike), None, now_ms)?;
            }
            note_less(config_dir, &card, now_ms)?;
            remove(&mut cards)
        }
        Act::NotThis => {
            note(&card.title, LedgerReason::NotThis, &day.date)?;
            note_less(config_dir, &card, now_ms)?;
            remove(&mut cards)
        }
        Act::Save => {
            let find = ps::saved_from_card(&card, now_ms)
                .ok_or_else(|| "This post has no link to save.".to_string())?;
            let path = saved_path(config_dir);
            let mut list = ps::load_saved(&path);
            if ps::save_find(&mut list, find) {
                ps::save_saved(&path, &list)?;
            }
            love(config_dir, &card, &mut cards, day, now_ms)?
        }
        Act::Love => love(config_dir, &card, &mut cards, day, now_ms)?,
        Act::Unlove => {
            let loved = cards
                .iter_mut()
                .find(|c| c.id == id && c.reaction == Some(CardReaction::Up));
            match loved {
                Some(saved) => {
                    saved.reaction = None;
                    learn_taste(config_dir, &card, None, Some(Reaction::Like), now_ms)?;
                    pulse_feed::with_state(config_dir, |s| {
                        s.taste_events
                            .retain(|e| !(e.kind == TasteKind::Love && e.text == card.title));
                    })?;
                    true
                }
                None => false,
            }
        }
        Act::Delete => {
            let housekeeping = grokhub_core::card_offers::stale(&card, now_ms)
                || grokhub_core::card_offers::one_off(&card);
            if card.kind != UpdateKind::Idea && !housekeeping {
                learn_taste(config_dir, &card, Some(Reaction::Delete), None, now_ms)?;
            }
            let before = cards.len();
            cards.retain(|c| c.id != id);
            cards.len() != before
        }
        Act::Snooze => {
            if card.kind != UpdateKind::Idea {
                learn_taste(config_dir, &card, Some(Reaction::Snooze), None, now_ms)?;
            }
            let until = pc::snooze_until(now_ms, day.hour, day.minute);
            pc::snooze_card(&mut cards, id, until)
        }
        Act::Opened => {
            engaged(config_dir, &card, now_ms)?;
            grokhub_core::mark_update_opened(&mut cards, id)
        }
        Act::Ignore => remove(&mut cards),
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
            // The one dislike button on an idea: card taste learns from it too.
            learn_taste(config_dir, &card, Some(Reaction::Dislike), None, now_ms)?;
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

/// Like: the heart, a ledger line and card taste. False when already liked.
fn love(
    config_dir: &Path,
    card: &UpdateCard,
    cards: &mut [UpdateCard],
    day: &Day,
    now_ms: u64,
) -> Result<bool, String> {
    if card.reaction == Some(CardReaction::Up) {
        return Ok(false);
    }
    note(&card.title, LedgerReason::Liked, &day.date)?;
    learn_taste(config_dir, card, Some(Reaction::Like), None, now_ms)?;
    pulse_feed::with_state(config_dir, |s| {
        s.note_engaged(now_ms);
        s.note_taste(TasteKind::Love, &card.title, now_ms);
    })?;
    if let Some(liked) = cards.iter_mut().find(|c| c.id == card.id) {
        liked.reaction = Some(CardReaction::Up);
    }
    Ok(true)
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

/// What a card puts in the chat: the card's own conversation, the words for
/// the box, and the card's report, which the chat's first message carries
/// unseen (see `card_report::card_carry`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Seed {
    pub conversation_id: String,
    pub text: String,
    #[serde(skip)]
    pub carry: Option<String>,
}

/// A card's own chat: the same conversation every time it is opened, so
/// "check into it" means this card. Ids are folder names, so only letters,
/// digits, `-` and `_` stay, 64 at most.
pub fn card_chat_id(id: &str) -> String {
    let clean: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let mut out = format!("pulse-{clean}");
    out.truncate(64);
    out
}

/// Words for the box when Discuss opens a reported card's chat.
pub const DISCUSS_ASK: &str = "Tell me more about this.";

fn engaged(config_dir: &Path, card: &UpdateCard, now_ms: u64) -> Result<(), String> {
    react(config_dir, card, Outcome::Tapped, None, now_ms);
    if card.kind != UpdateKind::Idea {
        learn_taste(config_dir, card, Some(Reaction::Engage), None, now_ms)?;
    }
    pulse_feed::with_state(config_dir, |s| {
        s.note_engaged(now_ms);
        s.note_taste(TasteKind::Discuss, &card.title, now_ms);
    })
}

/// Discuss: the card's own chat, carrying its report. A post with no report
/// puts the post itself in the box, as before. Counts as engagement and taste.
pub fn discuss(config_dir: &Path, id: &str, now_ms: u64) -> Result<Seed, String> {
    let card = find(config_dir, id)?;
    engaged(config_dir, &card, now_ms)?;
    let carry = card_context(&card);
    let text = if carry.is_some() {
        DISCUSS_ASK.to_string()
    } else {
        grokhub_core::pulse_writer::discuss_seed(&card)
    };
    Ok(Seed {
        conversation_id: card_chat_id(&card.id),
        text,
        carry,
    })
}

/// A card's offered fix: its words in the card's own chat, with the report.
/// Nothing runs until the person sends it.
pub fn offer(config_dir: &Path, id: &str, index: usize, now_ms: u64) -> Result<Seed, String> {
    let card = find(config_dir, id)?;
    let prompt = card
        .pulse
        .report
        .as_ref()
        .and_then(|r| r.offers.get(index))
        .map(|o| o.prompt.trim().to_string())
        .filter(|p| !p.is_empty())
        .ok_or("That fix is gone from the card. The page now shows what is left.")?;
    engaged(config_dir, &card, now_ms)?;
    Ok(Seed {
        conversation_id: card_chat_id(&card.id),
        text: prompt,
        carry: card_context(&card),
    })
}

/// Remind me later, tapped: what the page shows after, and when it is back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminded {
    pub page: Page,
    pub note: String,
}

/// The hour quiet hours start in the desktop app's `app.json`, the shell's
/// wind-down hour. `None` when quiet hours are off.
fn wind_down_hour(config_dir: &Path) -> Option<u32> {
    let raw = std::fs::read_to_string(config_dir.join("app.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let start = v.get("quietStart")?.as_str()?.trim().to_string();
    let end = v
        .get("quietEnd")
        .and_then(|e| e.as_str())
        .unwrap_or("")
        .trim();
    if start.is_empty() || start == end {
        return None;
    }
    grokhub_core::organs::hm_min(&start).map(|m| m / 60)
}

/// Remind me later: the card hides until tonight before wind-down when there
/// is time, else when they next sit down (9 AM until the cabin learns it).
pub fn remind(config_dir: &Path, id: &str, day: &Day, now_ms: u64) -> Result<Reminded, String> {
    let _store = store();
    let mut cards = load_unlocked(config_dir)?;
    if !cards.iter().any(|c| c.id == id) {
        return Err("That card is gone. The page now shows what is left.".into());
    }
    let later = remind_later(&DayShape {
        now_ms,
        hour: day.hour,
        minute: day.minute,
        wind_down: wind_down_hour(config_dir),
        start: None,
    });
    if let Some(card) = cards
        .iter()
        .find(|c| c.id == id && c.kind != UpdateKind::Idea)
    {
        learn_taste(config_dir, card, Some(Reaction::Snooze), None, now_ms)?;
    }
    if pc::snooze_card(&mut cards, id, later.at_ms) {
        save(config_dir, &cards)?;
    }
    let mut label = later.label;
    if let Some(first) = label.get(..1) {
        let lower = first.to_lowercase();
        label.replace_range(..1, &lower);
    }
    Ok(Reminded {
        page: page(config_dir, &cards, now_ms),
        note: format!("Back {label}."),
    })
}

/// The PC health settings the desktop app saved in its `app.json`, or the
/// defaults when it has none.
fn pc_health_settings(config_dir: &Path) -> grokhub_core::PcHealthSettings {
    std::fs::read_to_string(config_dir.join("app.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("pcHealth").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// The options window for a PC health card. Sessions come from the same
/// ledger the desktop app writes; nothing here changes the PC.
pub fn fix(config_dir: &Path, id: &str) -> Result<FixView, String> {
    let card = find(config_dir, id)?;
    if !grokhub_core::is_pc_health_card(&card) {
        return Err("That card has no options.".into());
    }
    let ledger: grokhub_core::PcHealthLedger =
        std::fs::read_to_string(config_dir.join(grokhub_core::pc_health::LEDGER_FILE))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
    let settings = pc_health_settings(config_dir);
    let stat = advice::card_session(&card.source_id, &ledger.history);
    let options = advice::sheet_options(stat, &ledger.history, &settings)
        .into_iter()
        // The fan fix runs in the desktop app only until the shell can run fixes.
        .filter(|o| !o.kind.is_setting() && o.kind != advice::FixKind::FansFull)
        .collect();
    let compare = if stat.is_some() {
        advice::compare_rows(stat, &ledger.history)
    } else {
        Vec::new()
    };
    Ok(FixView {
        title: card.title.trim().to_string(),
        verdict: verdict(&card),
        options,
        compare,
    })
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
        .or_else(|| grokhub_core::pulse::plain_why(&card).map(|w| format!("{w}.")))
        .or_else(|| {
            card.pulse
                .because
                .clone()
                .map(|b| format!("It matched your interest in {b}."))
        })
        .unwrap_or_else(|| "It came from your Pulse sources.".into());
    Ok(match learned {
        Some(line) => format!("{because} {line}"),
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

    /// A crash card with a report: two fixes, the facts behind it.
    fn reported() -> UpdateCard {
        use grokhub_core::card_report::{CardReport, FixOffer, OfferKind};
        let mut card = automation_done_card(
            "watch-bt",
            "Crash watch",
            "bluetoothd crashed",
            NOW - 60_000,
        );
        card.id = "crash-1".into();
        card.kind = UpdateKind::Suggestion;
        card.pulse.report = Some(CardReport {
            found: "bluetoothd crashed 4 times today".into(),
            summary: "Your Bluetooth service keeps restarting.".into(),
            details: "It stopped with a segfault each time.".into(),
            evidence: vec!["4 crash reports since 9 AM".into()],
            offers: vec![
                FixOffer {
                    label: "Find out why".into(),
                    prompt: "Read bluetoothd's crash history.".into(),
                    kind: OfferKind::Look,
                    fix_type: None,
                },
                FixOffer {
                    label: "Restart bluetooth".into(),
                    prompt: "Restart the bluetooth service.".into(),
                    kind: OfferKind::Chat,
                    fix_type: None,
                },
            ],
            ..Default::default()
        });
        card
    }

    #[test]
    fn a_reported_card_shows_its_report_chips_and_own_chat() {
        let d = dirs("reported", &[reported()]);
        let page = load(&d.config, None, NOW).expect("load");
        // A crash card is an idea row, not a feed post.
        assert!(page.feed.is_empty(), "{page:?}");
        let post = page
            .ideas
            .iter()
            .flat_map(|g| &g.ideas)
            .find(|i| i.id == "crash-1")
            .expect("idea row");
        assert_eq!(post.title, "bluetoothd crashed 4 times today");
        assert_eq!(post.summary, "Your Bluetooth service keeps restarting.");
        assert_eq!(
            post.more,
            "It stopped with a segfault each time.\n\n**What was checked**\n- 4 crash reports since 9 AM"
        );
        let chips: Vec<(&str, &str, usize)> = post
            .chips
            .iter()
            .map(|c| (c.label.as_str(), c.act, c.index))
            .collect();
        assert_eq!(
            chips,
            vec![
                ("Find out why", "offer", 0),
                ("Restart bluetooth", "offer", 1),
                ("Ignore for now", "ignore", 0),
                ("Remind me later", "remind", 0),
            ]
        );

        // Discuss and the fixes open the card's own chat, carrying its report.
        let seed = discuss(&d.config, "crash-1", NOW).expect("discuss");
        assert_eq!(seed.conversation_id, "pulse-crash-1");
        assert_eq!(seed.text, DISCUSS_ASK);
        let carry = seed.carry.expect("carry");
        assert!(
            carry.starts_with("Pulse card: bluetoothd crashed 4 times today"),
            "{carry}"
        );
        let fix = offer(&d.config, "crash-1", 1, NOW).expect("offer");
        assert_eq!(fix.conversation_id, "pulse-crash-1");
        assert_eq!(fix.text, "Restart the bluetooth service.");
        assert_eq!(fix.carry.as_deref(), Some(carry.as_str()));
        assert_eq!(
            offer(&d.config, "crash-1", 2, NOW).map(|s| s.text),
            Err("That fix is gone from the card. The page now shows what is left.".to_string())
        );
        assert_eq!(card_chat_id("a/b:c"), "pulse-abc");
        assert_eq!(card_chat_id(&"x".repeat(80)).len(), 64);
    }

    #[test]
    fn ignore_for_now_and_remind_me_later_leave_no_less_of_this() {
        let d = dirs("ignore", &[reported()]);
        std::fs::write(
            d.config.join("app.json"),
            r#"{"quietStart":"22:00","quietEnd":"07:00"}"#,
        )
        .expect("app.json");
        let before = memory();
        // 14:30 local, winds down at 10 PM: back tonight at 9:30.
        let r = remind(&d.config, "crash-1", &day(), NOW).expect("remind");
        assert_eq!(r.note, "Back tonight, 9:30 PM before you wind down.");
        let shown = |p: &Page| {
            p.ideas
                .iter()
                .flat_map(|g| &g.ideas)
                .any(|i| i.id == "crash-1")
        };
        assert!(!shown(&r.page), "{:?}", r.page);
        assert_eq!(memory(), before, "Remind me later writes no ledger line");

        let d = dirs("ignore2", &[reported()]);
        assert!(shown(&load(&d.config, None, NOW).expect("load")));
        let page = act(&d.config, "crash-1", Act::Ignore, &day(), NOW).expect("ignore");
        assert!(!shown(&page), "{page:?}");
        assert_eq!(memory(), before, "Ignore writes no ledger line");
        assert!(pulse_feed::state(&d.config).taste_events.is_empty());
        assert_eq!(
            remind(&d.config, "gone-1", &day(), NOW),
            Err("That card is gone. The page now shows what is left.".to_string())
        );
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
        assert_eq!(post.because, "", "an automation's post has no fetched why");
        assert_eq!(page.today, 1, "two hours old is today's set");
        assert_eq!(page.note, "");

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
        assert_eq!(learned.score("automations ideas"), 5, "{learned:?}");
        assert_eq!(
            why(&d.config, "idea-1").expect("why"),
            "It came from your Pulse sources. From Pulse ideas. You've liked ideas about automations before, so they rank higher."
        );
        act(&d.config, "idea-1", Act::Snooze, &day(), NOW).expect("snooze");
        let learned = reactions::load(&d.config.join(reactions::FILE_NAME));
        assert_eq!(learned.score("automations ideas"), 4);
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
    fn save_puts_a_source_post_on_the_saved_page_apart_from_the_feed() {
        let mut find =
            automation_done_card("src", "Rust 1.99 released", "Faster builds.", NOW - 60_000);
        find.id = "find-1".into();
        find.source_id = "source:rss:rust".into();
        find.citations = vec!["https://blog.rust-lang.org/1.99".into()];
        let d = dirs("saved", &[find]);
        let page = load(&d.config, None, NOW).expect("load");
        assert!(page.feed[0].save && !page.feed[0].saved);
        let page = act(&d.config, "find-1", Act::Save, &day(), NOW).expect("save");
        assert!(page.feed[0].saved && page.feed[0].loved, "{page:?}");
        let list = saved(&d.config);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Rust 1.99 released");
        // Feed delete never touches the Saved page.
        act(&d.config, "find-1", Act::Delete, &day(), NOW).expect("delete");
        assert_eq!(saved(&d.config).len(), 1);
        // Remove from saved is its one way out.
        let left = unsave(&d.config, "https://blog.rust-lang.org/1.99").expect("unsave");
        assert!(left.is_empty());
        assert!(saved(&d.config).is_empty());
        // A post with no source link offers no Save.
        let d = dirs("saved-none", &sample());
        let page = load(&d.config, None, NOW).expect("load");
        assert!(page.feed.iter().all(|p| !p.save));
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
        // Dismiss carries no weight: no ledger line, no taste.
        // Compare on LF: Windows checkouts may leave CRLF in the ledger.
        let mem = memory().replace("\r\n", "\n");
        assert!(mem.ends_with("reason=liked\n"), "{mem:?}");
        let taste = card_taste::load(&d.config);
        let kinds: Vec<Reaction> = taste.events.iter().map(|e| e.reaction).collect();
        assert_eq!(
            kinds,
            [Reaction::Like, Reaction::Snooze],
            "none from Dismiss"
        );
        assert_eq!(
            act(&d.config, "nope", Act::Dismiss, &day(), NOW),
            Err("That card is gone. The page now shows what is left.".to_string())
        );
    }

    #[test]
    fn like_and_dislike_train_card_taste_and_dismiss_does_not() {
        let d = dirs("taste", &sample());
        act(&d.config, "post-1", Act::Love, &day(), NOW).expect("like");
        let taste = card_taste::load(&d.config);
        assert_eq!(taste.events.len(), 1);
        assert_eq!(taste.events[0].reaction, Reaction::Like);
        act(&d.config, "post-1", Act::Unlove, &day(), NOW).expect("unlike");
        assert!(
            card_taste::load(&d.config).events.is_empty(),
            "Like taken back"
        );
        let page = act(&d.config, "post-1", Act::Dislike, &day(), NOW).expect("dislike");
        assert!(page.feed.is_empty(), "Not interested takes it off the page");
        let taste = card_taste::load(&d.config);
        assert_eq!(taste.events.len(), 1);
        assert_eq!(taste.events[0].reaction, Reaction::Dislike);
        assert!(!taste.events[0].traits.is_empty());
    }

    #[test]
    fn each_post_says_whether_its_row_offers_snooze_and_delete() {
        // A finished run is a one-off: Delete only.
        let d = dirs("offers", &sample());
        let page = load(&d.config, None, NOW).expect("load");
        let post = &page.feed[0];
        assert!(post.delete && !post.snooze, "{post:?}");
        // A schedule gets Snooze only; an unclear digest gets both.
        let mut remind = sample();
        remind[1].kind = UpdateKind::ScheduleCreated;
        let d = dirs("offers-remind", &remind);
        let post = &load(&d.config, None, NOW).expect("load").feed[0];
        assert!(post.snooze && !post.delete, "{post:?}");
        let mut plain = sample();
        plain[1].kind = UpdateKind::Digest;
        let d = dirs("offers-plain", &plain);
        let post = &load(&d.config, None, NOW).expect("load").feed[0];
        assert!(post.snooze && post.delete, "{post:?}");
    }

    #[test]
    fn open_snooze_and_delete_feed_card_taste_lightly() {
        let mut cards = sample();
        cards[1].kind = UpdateKind::Suggestion;
        let d = dirs("signals", &cards);
        act(&d.config, "post-1", Act::Opened, &day(), NOW).expect("opened");
        act(&d.config, "post-1", Act::Snooze, &day(), NOW).expect("snooze");
        let kinds: Vec<Reaction> = card_taste::load(&d.config)
            .events
            .iter()
            .map(|e| e.reaction)
            .collect();
        assert_eq!(kinds, [Reaction::Engage, Reaction::Snooze]);
        // A fresh card deleted is a small "less of this".
        let d = dirs("signals-delete", &cards);
        act(&d.config, "post-1", Act::Delete, &day(), NOW).expect("delete");
        let taste = card_taste::load(&d.config);
        assert_eq!(taste.events.len(), 1);
        assert_eq!(taste.events[0].reaction, Reaction::Delete);
        // A finished run deleted is housekeeping: nothing learned.
        let d = dirs("signals-housekeeping", &sample());
        act(&d.config, "post-1", Act::Delete, &day(), NOW).expect("delete");
        assert!(card_taste::load(&d.config).events.is_empty());
    }

    #[test]
    fn not_interested_on_an_idea_is_the_one_dislike_for_card_taste() {
        let d = dirs("idea-dislike", &sample());
        act(&d.config, "idea-1", Act::NotInterested, &day(), NOW).expect("not interested");
        let taste = card_taste::load(&d.config);
        assert_eq!(taste.events.len(), 1);
        assert_eq!(taste.events[0].reaction, Reaction::Dislike);
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
                today: 0,
                ideas: Vec::new(),
                note: String::new(),
            })
        );
    }

    #[test]
    fn love_and_discuss_feed_the_taste_summary_and_delete_does_not() {
        let d = dirs("taste", &sample());
        let page = act(&d.config, "post-1", Act::Love, &day(), NOW).expect("love");
        assert!(page.feed[0].loved);
        let seed = discuss(&d.config, "post-1", NOW + 1).expect("discuss");
        assert_eq!(seed.conversation_id, "pulse-post-1");
        assert_eq!(seed.carry, None);
        assert_eq!(
            seed.text,
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
            discuss(&d.config, "post-1", NOW).map(|s| s.text),
            Err("That card is gone. The page now shows what is left.".to_string())
        );
    }

    #[test]
    fn love_pressed_again_takes_the_heart_and_its_taste_line_away() {
        let d = dirs("unlove", &sample());
        let page = act(&d.config, "post-1", Act::Love, &day(), NOW).expect("love");
        assert!(page.feed[0].loved);
        let before = memory();
        let page = act(&d.config, "post-1", Act::Unlove, &day(), NOW + 1).expect("unlove");
        assert!(!page.feed[0].loved);
        assert_eq!(memory(), before, "Unlove writes no ledger line");
        assert!(pulse_feed::state(&d.config).taste_events.is_empty());
        // Pressed on a post that isn't loved, nothing changes.
        let again = act(&d.config, "post-1", Act::Unlove, &day(), NOW + 2).expect("again");
        assert_eq!(again, page);
    }

    #[test]
    fn not_this_on_a_post_tells_the_writer_less_of_this_and_an_idea_does_not() {
        let d = dirs("less", &sample());
        act(&d.config, "idea-1", Act::NotThis, &day(), NOW).expect("idea");
        assert!(pulse_feed::state(&d.config).taste_events.is_empty());
        act(&d.config, "post-1", Act::NotThis, &day(), NOW + 1).expect("post");
        let state = pulse_feed::state(&d.config);
        let kinds: Vec<(TasteKind, &str)> = state
            .taste_events
            .iter()
            .map(|e| (e.kind, e.text.as_str()))
            .collect();
        assert_eq!(kinds, vec![(TasteKind::Less, "Board")]);
        assert!(state.taste_stale());
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

    /// A PC health card reads its verdict on the Ideas row and opens an
    /// options window from the desktop app's session ledger. The window
    /// leaves out the settings rows, which only the desktop app changes.
    #[test]
    fn a_pc_health_card_shows_its_verdict_and_opens_its_options() {
        let at = NOW - 60_000;
        let mut card = grokhub_core::suggestion_card(
            &format!("pchealth:hades:{at}"),
            "PC health after Hades",
            "Max GPU 88°C.",
            at,
        );
        card.pulse.verdict = Some(grokhub_core::pulse::PulseVerdict {
            tone: pc::VerdictTone::Watch,
            line: "Warmer than usual: GPU peaked at 88°C.".into(),
        });
        let d = dirs("pchealth", &[card.clone()]);
        let stat = |ms: u64, gpu: u8| advice::SessionStat {
            at_ms: ms,
            game: "hades".into(),
            label: "Hades".into(),
            mins: 40,
            max_gpu_c: Some(gpu),
            max_cpu_c: Some(70),
            avg_gpu_pct: Some(90),
            bound: "gpu".into(),
            ..Default::default()
        };
        let ledger = grokhub_core::PcHealthLedger {
            history: vec![stat(at - 3_600_000, 78), stat(at, 88)],
            ..Default::default()
        };
        std::fs::write(
            d.config.join(grokhub_core::pc_health::LEDGER_FILE),
            serde_json::to_string(&ledger).expect("json"),
        )
        .expect("ledger");

        let page = load(&d.config, None, NOW).expect("load");
        let idea = page
            .ideas
            .iter()
            .flat_map(|g| &g.ideas)
            .find(|i| i.id == card.id)
            .expect("the card is an Ideas row");
        assert_eq!(idea.fix, "Fix it");
        assert_eq!(idea.verdict.as_ref().map(|v| v.tone), Some("watch"));

        let view = fix(&d.config, &card.id).expect("fix");
        let kinds: Vec<advice::FixKind> = view.options.iter().map(|o| o.kind).collect();
        assert!(kinds.contains(&advice::FixKind::Compare), "{kinds:?}");
        assert!(kinds.contains(&advice::FixKind::FpsCap), "{kinds:?}");
        assert!(kinds.contains(&advice::FixKind::Ask), "{kinds:?}");
        assert!(!kinds.iter().any(|k| k.is_setting()), "{kinds:?}");
        assert_eq!(view.compare.len(), 2);
        assert_eq!(view.compare[0].session, "This one");
        assert_eq!(view.compare[0].gpu_peak, "88°C");

        let other = &sample()[1];
        let d2 = dirs("pchealth-other", &sample());
        assert!(
            fix(&d2.config, &other.id).is_err(),
            "only PC health cards have options"
        );
    }
}
