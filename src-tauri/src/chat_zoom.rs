//! Semantic zoom: a saved chat folded into topic groups, so zooming out of a
//! long chat shows a card per topic instead of every message
//! (docs/design/chat-semantic-zoom).
//!
//! A turn is one user message and the lines after it, up to the next user
//! message (lines before the first one are a turn of their own). A group is a
//! run of turns. A new group starts after a quiet gap, when a turn's words
//! drift away from the open group's, or when the group is full. Turn vectors
//! come from the local static model memory recall uses, or from a hashed bag
//! of words when that model isn't on disk. Titles and summaries are the
//! group's own words: no model call, so it works offline and costs nothing.
//!
//! Groups cover the lines the chat view can show ([`history::lines`]), so a
//! group's line range is the row range the view scrolls to. Each chat's turn
//! vectors are kept in `{config}/zoom/<id>.json` so a reopened chat only
//! embeds the turns that changed; scratch chats are grouped in memory only.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use grokhub_agent::perm::ConfigGuard;
use grokhub_core::static_embed::{cosine, StaticModel};
use serde::{Deserialize, Serialize};

use crate::history::{self, Line, Role};

/// A new group starts when this long passes between two turns.
pub const GAP_MS: u64 = 30 * 60 * 1000;
/// A group holds at least this many turns before a topic shift can close it.
pub const MIN_TURNS: usize = 3;
/// A group closes at this many turns, whatever its topic.
pub const MAX_TURNS: usize = 40;
/// Below this cosine to the open group's centroid, a model-embedded turn
/// starts a new topic.
pub const MODEL_FLOOR: f32 = 0.35;
/// The same floor for bag-of-words vectors, which share fewer dimensions.
pub const WORDS_FLOOR: f32 = 0.08;
/// A group's one-line summary is clipped to this many characters.
pub const SUMMARY_MAX: usize = 90;
/// A turn is embedded from at most this many characters of its text.
const EMBED_CHARS: usize = 2_000;
const BUCKETS: usize = 1024;
const CACHE_VERSION: u32 = 1;
const ZOOM_DIR: &str = "zoom";
/// The title of a group with no words to name it by.
const UNTITLED: &str = "Messages";

/// How turn vectors were made; a cache made the other way is not reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// The local static model (`potion-base-8M`).
    Model,
    /// Hashed bag of stemmed words.
    Words,
}

impl Kind {
    fn floor(self) -> f32 {
        match self {
            Kind::Model => MODEL_FLOOR,
            Kind::Words => WORDS_FLOOR,
        }
    }
}

/// One topic group, as the zoomed-out chat shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    /// The group's first and last line in the chat's lines, oldest first.
    pub first_line: usize,
    pub last_line: usize,
    pub title: String,
    pub summary: String,
    /// Unix ms of the group's first and last timed turn; `None` when no
    /// turn in it carries a time (saved before turns had one).
    pub first_at: Option<u64>,
    pub last_at: Option<u64>,
    /// User and assistant messages in the group (tool lines aren't counted).
    pub messages: usize,
}

/// A chat folded into groups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Zoom {
    pub id: String,
    /// How many lines the chat has in all (the view loads the newest ones).
    pub lines: usize,
    pub groups: Vec<Group>,
    /// Messages the last compaction took out of view; they aren't grouped.
    pub compacted: usize,
    /// When the chat started and when its file last changed (Unix ms), the
    /// range for groups whose turns have no time.
    pub created: u64,
    pub updated: u64,
}

/// A turn: its line range, the text it is embedded from, and its time.
#[derive(Debug, Clone, PartialEq)]
struct Turn {
    first: usize,
    last: usize,
    at: Option<u64>,
    hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedTurn {
    hash: String,
    /// The unit vector as int8 (×127), base64.
    vec: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cache {
    version: u32,
    kind: Kind,
    turns: Vec<CachedTurn>,
}

/// Splits the chat's lines into turns. `times` holds one entry per user
/// line, in order ([`grokhub_agent::user_turn_times`]).
fn turns(lines: &[Line], times: &[Option<u64>]) -> Vec<Turn> {
    let mut out: Vec<Turn> = Vec::new();
    let mut users = 0;
    for (i, line) in lines.iter().enumerate() {
        let starts = line.role == Role::User || out.is_empty();
        if starts {
            let at = if line.role == Role::User {
                users += 1;
                times.get(users - 1).copied().flatten()
            } else {
                None
            };
            out.push(Turn {
                first: i,
                last: i,
                at,
                hash: String::new(),
            });
        } else if let Some(turn) = out.last_mut() {
            turn.last = i;
        }
    }
    for turn in &mut out {
        turn.hash = turn_hash(&lines[turn.first..=turn.last]);
    }
    out
}

/// FNV-1a over each line's role and text: stable across runs and builds.
fn turn_hash(lines: &[Line]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for line in lines {
        let role = match line.role {
            Role::User => "u",
            Role::Assistant => "a",
            Role::Tools => "t",
        };
        for b in role.bytes().chain([0]).chain(line.text.bytes()).chain([0]) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// What a turn is embedded from: its user message, or for a turn with none
/// (lines before the first ask), its first line.
fn turn_text<'a>(lines: &'a [Line], turn: &Turn) -> &'a str {
    let text = lines[turn.first].text.as_str();
    match text.char_indices().nth(EMBED_CHARS) {
        Some((cut, _)) => &text[..cut],
        None => text,
    }
}

/// A rough English stem: "files", "filed" and "file" all give "fil"; a
/// double "s" stays ("pass", "class").
fn stem(word: &str) -> &str {
    let root = ["ing", "ed", "es", "s"]
        .iter()
        .find_map(|suffix| {
            let root = word.strip_suffix(suffix)?;
            (root.len() >= 3 && !(*suffix == "s" && root.ends_with('s'))).then_some(root)
        })
        .unwrap_or(word);
    match root.strip_suffix('e') {
        Some(base) if base.len() >= 3 => base,
        _ => root,
    }
}

/// Part of a word: letters and digits, plus the combining signs of Indic and
/// Southeast Asian scripts (viramas, vowel signs, tone marks), which aren't
/// alphanumeric but sit inside words. The Devanagari full stops aren't.
fn word_char(c: char) -> bool {
    c.is_alphanumeric()
        || (matches!(u32::from(c),
            0x0300..=0x036F | 0x0900..=0x0DFF | 0x0E00..=0x0FFF | 0x1000..=0x109F
                | 0x1780..=0x17FF)
            && !matches!(c, '\u{0964}' | '\u{0965}'))
}

/// The text's words, lowercased, each run of an unspaced script apart from
/// the letters around it ("用rust写" gives "用", "rust" and "写").
fn words(text: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    for c in text.to_lowercase().chars() {
        if !word_char(c) {
            out.push((false, String::new()));
            continue;
        }
        let kind = unspaced(c);
        match out.last_mut() {
            Some((last, word)) if *last == kind || word.is_empty() => {
                *last = kind;
                word.push(c);
            }
            _ => out.push((kind, c.to_string())),
        }
    }
    out.retain(|(_, word)| !word.is_empty());
    out
}

/// Scripts written without spaces between words (Thai, Lao, Myanmar, Khmer,
/// kana and CJK ideographs): their runs are split into character pairs.
fn unspaced(c: char) -> bool {
    matches!(u32::from(c),
        0x0E00..=0x0EFF | 0x1000..=0x109F | 0x1780..=0x17FF | 0x3040..=0x30FF
            | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn bucket(word: &str) -> usize {
    let mut h: u32 = 0x811c_9dc5;
    for b in word.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h as usize % BUCKETS
}

/// A unit vector of stemmed content words hashed into [`BUCKETS`] buckets.
/// A run of an unspaced script counts each pair of characters as a word.
fn word_vector(text: &str) -> Option<Vec<f32>> {
    let mut v = vec![0f32; BUCKETS];
    let mut any = false;
    for (pairs_only, raw) in words(text) {
        let raw = raw.as_str();
        if pairs_only {
            let chars: Vec<char> = raw.chars().collect();
            let pairs: Vec<String> = match chars.len() {
                1 => vec![raw.to_string()],
                _ => chars.windows(2).map(|w| w.iter().collect()).collect(),
            };
            for pair in pairs {
                v[bucket(&pair)] += 1.0;
                any = true;
            }
            continue;
        }
        if raw.chars().count() < 3 || grokhub_core::is_title_stopword(raw) {
            continue;
        }
        v[bucket(stem(raw))] += 1.0;
        any = true;
    }
    any.then(|| unit(v)).flatten()
}

fn unit(mut v: Vec<f32>) -> Option<Vec<f32>> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return None;
    }
    v.iter_mut().for_each(|x| *x /= norm);
    Some(v)
}

fn pack(v: &[f32]) -> String {
    let bytes: Vec<u8> = v
        .iter()
        .map(|x| (x * 127.0).round().clamp(-127.0, 127.0) as i8 as u8)
        .collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unpack(s: &str) -> Option<Vec<f32>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(s).ok()?;
    unit(
        bytes
            .into_iter()
            .map(|b| f32::from(b as i8) / 127.0)
            .collect(),
    )
}

/// Where turns break into groups: each group's turn range, in order.
fn segment(turns: &[Turn], vecs: &[Option<Vec<f32>>], floor: f32) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut centroid: Option<Vec<f32>> = None;
    let mut last_at: Option<u64> = None;
    for (i, turn) in turns.iter().enumerate() {
        let len = i - start;
        let vec = vecs.get(i).and_then(Option::as_ref);
        let gap = matches!((last_at, turn.at), (Some(a), Some(b)) if b.saturating_sub(a) >= GAP_MS);
        let full = len >= MAX_TURNS;
        let drift = len >= MIN_TURNS
            && match (vec, centroid.as_ref().and_then(|c| unit(c.clone()))) {
                (Some(v), Some(c)) => cosine(v, &c) < floor,
                _ => false,
            };
        if len > 0 && (gap || full || drift) {
            out.push(start..i);
            start = i;
            centroid = None;
        }
        if let Some(v) = vec {
            match centroid.as_mut() {
                Some(c) if c.len() == v.len() => c.iter_mut().zip(v).for_each(|(c, x)| *c += x),
                _ => centroid = Some(v.clone()),
            }
        }
        last_at = turn.at.or(last_at);
    }
    if start < turns.len() {
        out.push(start..turns.len());
    }
    out
}

fn first_line_of(text: &str) -> String {
    text.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .find(|l| !l.is_empty())
        .unwrap_or_default()
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A group's card from its lines.
fn group_of(lines: &[Line], turns: &[Turn]) -> Group {
    let (first, last) = (turns[0].first, turns[turns.len() - 1].last);
    let span = &lines[first..=last];
    let asks: Vec<(String, String)> = span
        .iter()
        .filter(|l| l.role == Role::User)
        .map(|l| ("user".to_string(), l.text.clone()))
        .collect();
    let first_ask = asks
        .first()
        .map(|(_, text)| first_line_of(text))
        .unwrap_or_else(|| first_line_of(&span[0].text));
    let words = grokhub_core::topics_from_turns(&asks).join(" ");
    let title = grokhub_core::short_auto_title(&words)
        .or_else(|| (!first_ask.is_empty()).then(|| clip(&first_ask, grokhub_core::AUTO_TITLE_MAX)))
        .map(|t| capitalized(&t))
        .unwrap_or_else(|| UNTITLED.to_string());
    let times: Vec<u64> = turns.iter().filter_map(|t| t.at).collect();
    Group {
        first_line: first,
        last_line: last,
        title,
        summary: clip(&first_ask, SUMMARY_MAX),
        first_at: times.iter().min().copied(),
        last_at: times.iter().max().copied(),
        messages: span.iter().filter(|l| l.role != Role::Tools).count(),
    }
}

/// Folds lines into groups. `embed` gives a turn's unit vector; turns whose
/// hash is in `reuse` at the same position take the cached vector instead.
/// Returns the groups and every turn's vector, for the cache.
fn fold(
    lines: &[Line],
    times: &[Option<u64>],
    kind: Kind,
    reuse: &[CachedTurn],
    embed: &dyn Fn(&str) -> Option<Vec<f32>>,
) -> (Vec<Group>, Vec<CachedTurn>) {
    let turns = turns(lines, times);
    let kept = reuse
        .iter()
        .zip(&turns)
        .take_while(|(cached, turn)| cached.hash == turn.hash)
        .count();
    // Vectors are kept as their int8 form: a reused one is stored again as
    // it was read, and a fresh one is grouped from its stored form, so a
    // reopen groups exactly as the first open did.
    let packed: Vec<Option<String>> = turns
        .iter()
        .enumerate()
        .map(|(i, turn)| match reuse.get(i) {
            Some(cached) if i < kept => cached.vec.clone(),
            _ => embed(turn_text(lines, turn)).map(|v| pack(&v)),
        })
        .collect();
    let vecs: Vec<Option<Vec<f32>>> = packed
        .iter()
        .map(|vec| vec.as_deref().and_then(unpack))
        .collect();
    let groups = segment(&turns, &vecs, kind.floor())
        .into_iter()
        .map(|range| group_of(lines, &turns[range]))
        .collect();
    let cache = turns
        .iter()
        .zip(packed)
        .map(|(turn, vec)| CachedTurn {
            hash: turn.hash.clone(),
            vec,
        })
        .collect();
    (groups, cache)
}

fn cache_path(config_dir: &Path, id: &str) -> PathBuf {
    config_dir.join(ZOOM_DIR).join(format!("{id}.json"))
}

fn read_cache(config_dir: &Path, id: &str, kind: Kind) -> Vec<CachedTurn> {
    std::fs::read_to_string(cache_path(config_dir, id))
        .ok()
        .and_then(|raw| serde_json::from_str::<Cache>(&raw).ok())
        .filter(|cache| cache.version == CACHE_VERSION && cache.kind == kind)
        .map(|cache| cache.turns)
        .unwrap_or_default()
}

fn write_cache(config_dir: &Path, id: &str, kind: Kind, turns: Vec<CachedTurn>) {
    let path = cache_path(config_dir, id);
    let cache = Cache {
        version: CACHE_VERSION,
        kind,
        turns,
    };
    let Ok(raw) = serde_json::to_vec(&cache) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, raw).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Writes the cache, unless the chat is gone: a delete that ran while this
/// zoom was embedding has already forgotten it, so none is left behind.
fn keep_cache(config_dir: &Path, id: &str, kind: Kind, turns: Vec<CachedTurn>) {
    write_cache(config_dir, id, kind, turns);
    let _config = ConfigGuard::set(config_dir);
    if !grokhub_agent::session_file(id).is_ok_and(|file| file.exists()) {
        forget(config_dir, id);
    }
}

/// Removes a chat's zoom cache; deleting the chat calls it.
pub fn forget(config_dir: &Path, id: &str) {
    let _ = std::fs::remove_file(cache_path(config_dir, id));
}

/// The chat folded into groups, embedding with `model` when there is one.
pub fn zoom_with(config_dir: &Path, id: &str, model: Option<&StaticModel>) -> Result<Zoom, String> {
    let info = history::saved(config_dir, id)?;
    let lines = history::lines(&info);
    let times = grokhub_agent::user_turn_times(&info);
    let kind = if model.is_some() {
        Kind::Model
    } else {
        Kind::Words
    };
    let scratch = grokhub_agent::scratch::is_scratch(&info.id);
    let reuse = if scratch {
        Vec::new()
    } else {
        read_cache(config_dir, &info.id, kind)
    };
    let embed = |text: &str| match model {
        Some(model) => model.encode(text),
        None => word_vector(text),
    };
    let (groups, turns) = fold(&lines, &times, kind, &reuse, &embed);
    if !scratch {
        keep_cache(config_dir, &info.id, kind, turns);
    }
    let _config = ConfigGuard::set(config_dir);
    Ok(Zoom {
        id: info.id.clone(),
        lines: lines.len(),
        groups,
        compacted: grokhub_agent::compacted_messages(&info),
        created: info.created,
        updated: history::updated_ms(&info),
    })
}

/// The chat folded into groups with the memory-recall model when it is on
/// disk (and not switched off), else with words.
pub fn zoom(config_dir: &Path, id: &str) -> Result<Zoom, String> {
    let model = {
        let _config = ConfigGuard::set(config_dir);
        grokhub_agent::text_embedder()
    };
    zoom_with(config_dir, id, model.as_deref())
}

#[cfg(test)]
#[path = "chat_zoom_tests.rs"]
mod tests;
