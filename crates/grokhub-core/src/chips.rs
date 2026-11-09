//! Predictive quick chips — stage, draft, habits, fast-mode parse.
//! Secrets never persist in chip memory. Chip keys are opaque hashes;
//! free-form chat lines are never stored as keys, labels, or habit values.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::is_plain_text;
use crate::recipe::user_asks_gui_help;
use crate::skill::{match_skill, skill_use_in_chat_prompt, SkillMd};

pub const CHIP_VISIBLE_MAX: usize = 5;
pub const CHIP_HARD_MAX: usize = 8;
/// On-disk `chips.json` schema. v1 embedded raw chat in keys; v2 uses opaque hashes.
/// 3: retired mode kind dropped, replay-unsafe text scrubbed, local chip log.
pub const CHIP_MEMORY_VERSION: u32 = 3;
const CHIP_SCAN_CAP: usize = 4096;
const MAX_HITS: usize = 80;
const MAX_TRANSITIONS: usize = 12;
/// Hex chars kept from the sha256 of a normalized value (`kind:v1:{hex}`).
const VALUE_KEY_HASH_HEX: usize = 16;
/// Visible habit labels stay short; never the full typed line.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChipKind {
    Chat,
    Shell,
    /// `mode` is the retired model/effort chip kind. Chip memory written
    /// before still reads (as a nav hit) and the load prune drops it: no chip
    /// changes the model or effort, the router alone picks them.
    #[serde(alias = "mode")]
    Nav,
}

/// What a tap does. One rule per kind, the same on every surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipAction {
    /// Sends a message when the box is empty; with a draft typed, it is
    /// added to the draft and nothing is sent.
    Ask,
    /// Opens a page, a guide or a link.
    Go,
    /// Runs a small app action right away, with an Undo after it.
    Do,
    /// Shows what would happen first (a command, a cleanup, a note) and
    /// waits for the person to run it.
    Review,
}

/// The id of the Undo chip that follows a `Do` chip for a short while.
pub const UNDO_CHIP_ID: &str = "undo";

/// The tap rule for a chip. Feature one-taps are told apart by their id.
pub fn chip_action(chip: &QuickChip) -> ChipAction {
    let id = chip.id.as_str();
    if id == crate::backup_nudge::SNOOZE_ID
        || id == UNDO_CHIP_ID
        || id.starts_with(crate::release_radar::REMIND_PREFIX)
        || id.starts_with(crate::objectives::TRACK_CHIP_PREFIX)
    {
        return ChipAction::Do;
    }
    if id.starts_with(crate::game_companion::GUIDES_PREFIX) {
        return ChipAction::Go;
    }
    if id == crate::downloads_tidy::REVIEW_CHIP
        || id.starts_with(crate::game_companion::NOTE_PREFIX)
    {
        return ChipAction::Review;
    }
    match chip.kind {
        ChipKind::Chat => ChipAction::Ask,
        ChipKind::Nav => ChipAction::Go,
        ChipKind::Shell => ChipAction::Review,
    }
}

/// A chip value from the retired model/effort chips (`__mode:think`).
pub fn retired_mode_value(value: &str) -> bool {
    value.trim_start().starts_with("__mode:")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipStage {
    Empty,
    Mid,
    Error,
    Tools,
    Long,
    Default,
}

impl ChipStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Mid => "mid",
            Self::Error => "error",
            Self::Tools => "tools",
            Self::Long => "long",
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PredictedIntent {
    Finish,
    Fix,
    Ship,
    Explain,
    Host,
    Decide,
    Create,
    Continue,
    Review,
    Chat,
}

impl PredictedIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Finish => "finish",
            Self::Fix => "fix",
            Self::Ship => "ship",
            Self::Explain => "explain",
            Self::Host => "host",
            Self::Decide => "decide",
            Self::Create => "create",
            Self::Continue => "continue",
            Self::Review => "review",
            Self::Chat => "chat",
        }
    }

    fn all() -> [Self; 10] {
        [
            Self::Finish,
            Self::Fix,
            Self::Ship,
            Self::Explain,
            Self::Host,
            Self::Decide,
            Self::Create,
            Self::Continue,
            Self::Review,
            Self::Chat,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickChip {
    pub id: String,
    pub label: String,
    pub value: String,
    pub kind: ChipKind,
    pub score: f32,
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipHit {
    pub key: String,
    pub label: String,
    pub value: String,
    pub kind: ChipKind,
    pub uses: u32,
    #[serde(default)]
    pub typed_uses: u32,
    /// Times this chip was picked from the row. Similar prompts bump `uses`, not this.
    #[serde(default)]
    pub picks: u32,
    #[serde(default)]
    pub last_used_at: u64,
    #[serde(default)]
    pub hour_hits: Vec<u32>,
    #[serde(default)]
    pub successes: u32,
    #[serde(default)]
    pub failures: u32,
    #[serde(default)]
    pub context_tags: Vec<String>,
    #[serde(default)]
    pub dismisses: u32,
    /// Dismissals by hour of day. Separate from uses, so a 9pm no is not a 9pm habit.
    #[serde(default)]
    pub dismiss_hours: Vec<u32>,
    #[serde(default)]
    pub last_dismissed_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipMemory {
    #[serde(default = "chip_memory_version")]
    pub version: u32,
    #[serde(default)]
    pub hits: Vec<ChipHit>,
    #[serde(default)]
    pub transitions: BTreeMap<String, BTreeMap<String, u32>>,
    #[serde(default)]
    pub last_chip_key: Option<String>,
    #[serde(default)]
    pub last_slash: Option<String>,
    #[serde(default)]
    pub last_surface: Option<String>,
    #[serde(default)]
    pub total_events: u32,
    #[serde(default)]
    pub updated_at: u64,
    /// Habits by what a chip or typed ask is for ([`chip_intent`]), never
    /// by its text, so chat-built chips and typed asks teach too (#1057).
    #[serde(default)]
    pub intents: BTreeMap<String, IntentHit>,
    /// Local log of shown rows and what happened (no chip text), newest last.
    /// See [`crate::chip_store`].
    #[serde(default)]
    pub events: Vec<crate::chip_store::ChipEvent>,
    /// The learned chip ranker. See [`crate::chip_rank`].
    #[serde(default)]
    pub ranker: crate::chip_rank::ChipRanker,
    /// When the reply's own chips last lost and the rule asking for them left
    /// the prompt; 0 while it is in. See [`crate::chip_trailer`].
    #[serde(default)]
    pub trailer_off_at: u64,
    /// How often each predicted reply on a plan or question card was picked,
    /// by reply kind (`plan:go`). See [`crate::reply_chips`].
    #[serde(default)]
    pub reply_picks: BTreeMap<String, u32>,
}

/// How often one intent was used, by local hour and weekday.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentHit {
    #[serde(default)]
    pub uses: u32,
    #[serde(default)]
    pub hour_hits: Vec<u32>,
    /// 0 = Sunday.
    #[serde(default)]
    pub day_hits: Vec<u32>,
    #[serde(default)]
    pub last_used_at: u64,
}

/// Serde default when `version` is missing: treat as legacy v1 so load scrubs.
fn chip_memory_version() -> u32 {
    1
}

impl Default for ChipMemory {
    fn default() -> Self {
        Self {
            version: CHIP_MEMORY_VERSION,
            hits: vec![],
            transitions: BTreeMap::new(),
            last_chip_key: None,
            last_slash: None,
            last_surface: None,
            total_events: 0,
            updated_at: 0,
            intents: BTreeMap::new(),
            events: Vec::new(),
            ranker: crate::chip_rank::ChipRanker::default(),
            trailer_off_at: 0,
            reply_picks: BTreeMap::new(),
        }
    }
}

/// What a chip or typed ask is for, from a fixed catalog, first match wins.
/// Game and rig words come before code words, so "Fix stutter in …" is a
/// game ask, not a code fix.
const INTENT_CATALOG: &[(&str, &[&str])] = &[
    (
        "game.perf",
        &[
            "stutter",
            "fps",
            "frame rate",
            "framerate",
            "lag",
            "ping",
            "hitch",
        ],
    ),
    (
        "game.squad",
        &["duo", "squad", "teammate", "lfg", "party up"],
    ),
    ("game.build", &["roast my build", "my build", "loadout"]),
    (
        "game.session",
        &["how'd", "match go", "ranked", "just finished playing"],
    ),
    (
        "rig.storage",
        &["storage", "disk", "drive space", "free up"],
    ),
    ("rig.update", &["update", "driver"]),
    ("day.plan", &["plan my", "weekend", "my week", "schedule"]),
    ("create.image", &["imagine", "image", "draw", "logo"]),
    (
        "code.fix",
        &["fix", "error", "bug", "crash", "failing", "root cause"],
    ),
    (
        "code.ship",
        &["implement", "ship", "deploy", "release", "merge"],
    ),
    ("code.review", &["review", "refactor", "tests"]),
    ("explain", &["explain", "what does", "how does"]),
];

/// The catalog intent of a chip or typed ask, or none.
pub fn chip_intent(label: &str, value: &str) -> Option<&'static str> {
    let hay = format!(" {} {} ", label, value).to_ascii_lowercase();
    // A word must start the match: "fix" is not in "prefix", "lag" not in "flag".
    let starts_word = |w: &str| {
        hay.match_indices(w).any(|(i, _)| {
            !hay[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric())
        })
    };
    INTENT_CATALOG
        .iter()
        .find(|(_, words)| words.iter().any(|w| starts_word(w)))
        .map(|(key, _)| *key)
}

/// Count one use of what `label`/`value` is for at this hour and weekday.
/// Only the catalog key is kept; neither text reaches chips.json.
pub fn remember_intent(
    memory: &mut ChipMemory,
    label: &str,
    value: &str,
    now_ms: u64,
    hour: u8,
    weekday: u8,
) {
    let Some(key) = chip_intent(label, value) else {
        return;
    };
    let hit = memory.intents.entry(key.to_string()).or_default();
    hit.uses = hit.uses.saturating_add(1);
    hit.hour_hits = bump_hour(&hit.hour_hits, hour);
    let mut days = if hit.day_hits.len() == 7 {
        hit.day_hits.clone()
    } else {
        vec![0; 7]
    };
    let d = (weekday as usize).min(6);
    days[d] = days[d].saturating_add(1);
    hit.day_hits = days;
    hit.last_used_at = now_ms;
    memory.updated_at = now_ms;
}

/// Lift for a chip whose intent you use at this hour (or the next) and on
/// this weekday. A habit at 9pm does nothing at 2pm.
fn intent_boost(memory: &ChipMemory, chip: &QuickChip, hour: u8, weekday: u8) -> f32 {
    let Some(hit) = chip_intent(&chip.label, &chip.value).and_then(|k| memory.intents.get(k))
    else {
        return 0.0;
    };
    let at = |hours: &[u32], i: usize| hours.get(i).copied().unwrap_or(0);
    let h = (hour as usize).min(23);
    let near = at(&hit.hour_hits, h) * 2 + at(&hit.hour_hits, (h + 1) % 24);
    let day = at(&hit.day_hits, (weekday as usize).min(6));
    (near as f32 * 3.0).min(24.0) + (day as f32 * 2.0).min(8.0)
}

#[derive(Debug, Clone, Default)]
pub struct ChipContext {
    pub code: bool,
    pub app: bool,
    pub host: bool,
    pub imagine: bool,
    pub error: bool,
    pub ui: bool,
    pub decide: bool,
    pub implement: bool,
    pub incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipThread {
    pub title: String,
    pub last_user: String,
    pub last_assistant: String,
}

#[derive(Debug, Clone)]
pub struct ChipInput<'a> {
    pub chat: &'a [(String, String)],
    pub draft: &'a str,
    pub grok_connected: bool,
    pub mode: &'a str,
    pub thread_title: &'a str,
    pub usage_messages: u32,
    pub memory: &'a ChipMemory,
    pub dismissed: &'a [String],
    pub reply_chips: &'a [QuickChip],
    pub last_failed: bool,
    pub hour: u8,
    pub now_ms: u64,
    pub max: usize,
    pub other_threads: &'a [ChipThread],
    pub first_run: bool,
    pub last_slash: &'a str,
    pub last_surface: &'a str,
    pub last_project: &'a str,
    pub skill_count: u32,
    pub session_mode: &'a str,
    pub signals: ChipSignals<'a>,
    /// Timely chips from the app's own features (model drop, code resume,
    /// Downloads tidy, backup, release radar, game companion, focus help,
    /// skill offer). They rank with every other chip instead of jumping the
    /// row, and obey the same hides and vetoes.
    pub extra: &'a [QuickChip],
}

/// What the app already knows about this PC and the day, beyond the chat.
/// Filled from cached state only: no new process, file or network reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChipSignals<'a> {
    /// The library game the last process sample saw running.
    pub game: Option<&'a str>,
    /// A library game that closed a few minutes ago.
    pub game_ended: Option<&'a str>,
    /// A local drive is nearly full (see [`disk_is_low`]).
    pub low_disk: bool,
    /// Local weekday, 0 = Sunday, like `date +%w`.
    pub weekday: u8,
    /// A new app build is waiting to install.
    pub update_waiting: bool,
    /// The user's quiet hours are on.
    pub quiet_hours: bool,
    /// The person is gaming, or a fullscreen app owns the screen. Nudges
    /// wait; the chat's own chips and game chips stay.
    pub gaming: bool,
    /// A protected anti-cheat is running. Chips that touch the game, read
    /// other windows or run on the host are hidden so nothing gets flagged.
    pub anticheat: bool,
    /// The name of a file waiting in the attach tray (name only, no path).
    pub attachment: Option<&'a str>,
}

/// Under this share free, a drive counts as nearly full.
const LOW_DISK_SHARE: u64 = 10;
/// Under this many bytes free, a drive counts as nearly full whatever its size.
const LOW_DISK_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// True when a drive with `total` bytes has under 10% or 5 GiB left.
pub fn disk_is_low(free: u64, total: u64) -> bool {
    total > 0 && (free < LOW_DISK_BYTES || free.saturating_mul(100) < total * LOW_DISK_SHARE)
}

pub fn empty_chip_memory() -> ChipMemory {
    ChipMemory::default()
}

pub fn chip_memory_key(chip: &QuickChip) -> String {
    // A reply's chips reuse `reply-0..2` for every reply, and older model
    // chips put their label in the id (`llm-0-Fix the blen`): key by value.
    if chip.id.starts_with("learn-")
        || chip.id.starts_with("recent-")
        || chip.id.starts_with("pred-habit")
        || chip.id.starts_with("llm-")
        || chip.id.starts_with(crate::chip_trailer::REPLY_CHIP_PREFIX)
    {
        value_key(&chip.value, chip.kind)
    } else {
        format!("id:{}", chip.id)
    }
}

fn kind_key_prefix(kind: ChipKind) -> &'static str {
    match kind {
        ChipKind::Chat => "chat",
        ChipKind::Shell => "shell",
        ChipKind::Nav => "nav",
    }
}

/// Normalize a chip value the same way legacy keys did (lowercase, collapse space).
fn normalize_chip_value(value: &str) -> String {
    let v = value.trim().to_ascii_lowercase();
    v.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Opaque habit key: `chat:v1:{16 hex}` — never the raw ask or reply text.
fn value_key(value: &str, kind: ChipKind) -> String {
    let v = normalize_chip_value(value);
    let digest = Sha256::digest(v.as_bytes());
    let hex = hex::encode(digest);
    format!(
        "{}:v1:{}",
        kind_key_prefix(kind),
        &hex[..VALUE_KEY_HASH_HEX]
    )
}

fn is_opaque_value_key(key: &str) -> bool {
    for prefix in ["chat:v1:", "shell:v1:", "nav:v1:", "mode:v1:"] {
        if let Some(rest) = key.strip_prefix(prefix) {
            return rest.len() == VALUE_KEY_HASH_HEX && rest.chars().all(|c| c.is_ascii_hexdigit());
        }
    }
    false
}

/// True when a stored key was built by embedding normalized chat/shell text
/// (pre-v2 `chat:the whole ask…`). Catalog `id:…` and opaque `*:v1:hex` keys pass.
pub fn chip_key_embeds_chat_text(key: &str) -> bool {
    let key = key.trim();
    if key.is_empty() || key.starts_with("id:") || is_opaque_value_key(key) {
        return false;
    }
    for prefix in ["chat:", "shell:", "nav:", "mode:"] {
        if let Some(rest) = key.strip_prefix(prefix) {
            // Legacy: kind + raw normalized text. Opaque keys already returned above.
            return !rest.is_empty();
        }
    }
    // Bare prose keys (defensive): spaces mean a line, not an id.
    key.contains(' ')
}

pub fn chip_scan(s: &str) -> &str {
    if s.len() <= CHIP_SCAN_CAP {
        return s;
    }
    let mut end = CHIP_SCAN_CAP;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn last_of(chat: &[(String, String)], role: &str) -> String {
    chat.iter()
        .rev()
        .find(|(r, c)| r == role && !c.trim().is_empty())
        .map(|(_, c)| {
            // Cap before decoding. A timeline's replies are what the chips read;
            // tool rows and TURN_ markers are not the reply.
            let c = chip_scan(c);
            if role == "assistant" {
                crate::turn_timeline::strip_turn_labels(c)
            } else {
                c.to_string()
            }
        })
        .unwrap_or_default()
}

fn recent_user(chat: &[(String, String)], n: usize) -> Vec<String> {
    chat.iter()
        .rev()
        .filter(|(r, c)| r == "user" && !c.trim().is_empty())
        .take(n)
        .map(|(_, c)| chip_scan(c.trim()).to_string())
        .collect()
}

pub fn detect_chip_context(chat: &[(String, String)]) -> ChipContext {
    let users = recent_user(chat, 6).join("\n");
    let asst = last_of(chat, "assistant");
    let blob = format!("{users}\n{asst}").to_ascii_lowercase();
    let has_fence = format!("{users}\n{asst}").contains("```");
    let code = has_fence
        || regexish(
            &blob,
            &[
                "function",
                "const ",
                "let ",
                "class ",
                "import ",
                "export ",
                "def ",
                "fn ",
                "typescript",
                "python",
                "review this code",
                "refactor",
            ],
        );
    ChipContext {
        code,
        app: regexish(
            &blob,
            &[
                "grokhub",
                "this app",
                "the app",
                "native cabin",
                "desktop app",
                "sidebar",
                "composer",
                "improve the ui",
                "fix this bug",
            ],
        ),
        host: regexish(
            &blob,
            &[
                "host_cmd",
                "host_result",
                "desktop host",
                "shell",
                "uname",
                "ls -",
                "journalctl",
                "ps aux",
            ],
        ),
        imagine: regexish(
            &blob,
            &["imagine", "generate a pic", "generate an image", "draw "],
        ),
        error: regexish(
            &blob,
            &[
                "error",
                "bug",
                "fail",
                "broken",
                "crash",
                "exception",
                "doesn't work",
                "not working",
            ],
        ),
        ui: regexish(
            &blob,
            &[
                "ui",
                "layout",
                "button",
                "sidebar",
                "theme",
                "dark mode",
                "chip",
            ],
        ),
        decide: regexish(
            &blob,
            &[
                "should i",
                "which",
                "options",
                "tradeoff",
                "recommend",
                "compare",
            ],
        ),
        implement: regexish(
            &users.to_ascii_lowercase(),
            &[
                "implement",
                "add ",
                "build ",
                "create ",
                "wire ",
                "ship ",
                "patch",
            ],
        ),
        incomplete: regexish(
            &asst.to_ascii_lowercase(),
            &[
                "i'll",
                "i will",
                "let me",
                "next i",
                "continuing",
                "still need",
                "want me to",
                "shall i",
            ],
        ) || (!asst.is_empty()
            && asst.len() < 600
            && regexish(
                &asst.to_ascii_lowercase(),
                &["check", "probe", "investigate"],
            )
            && !asst.to_ascii_lowercase().contains("host_cmd")),
    }
}

fn regexish(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

pub fn detect_chip_stage(chat: &[(String, String)], last_failed: bool) -> ChipStage {
    let msgs: Vec<_> = chat
        .iter()
        .filter(|(r, _)| r == "user" || r == "assistant")
        .collect();
    if msgs.is_empty() {
        return ChipStage::Empty;
    }
    let asst = last_of(chat, "assistant");
    let low = asst.to_ascii_lowercase();
    if last_failed
        || regexish(
            &low,
            &[
                "error",
                "fail",
                "broken",
                "crash",
                "couldn't complete",
                "not a function",
            ],
        )
    {
        return ChipStage::Error;
    }
    if regexish(&asst, &["HOST_RESULT", "CONNECTOR_RESULT", "```host"]) {
        return ChipStage::Tools;
    }
    if asst.len() > 900 || msgs.len() >= 8 {
        return ChipStage::Long;
    }
    if msgs.len() >= 2 {
        return ChipStage::Mid;
    }
    ChipStage::Default
}

/// A context tag as stored on a habit: the draft part dropped. Tags are
/// matched against the draft-free fingerprint, and older tags carried up to
/// 24 raw draft characters (which may hold `+`) up to the `h{n}` hour bit.
fn tag_without_draft(tag: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut in_draft = false;
    for part in tag.split('+') {
        if part.starts_with("d:") {
            in_draft = true;
            continue;
        }
        let hour_bit =
            part.len() == 2 && part.starts_with('h') && part.as_bytes()[1].is_ascii_digit();
        if in_draft && !hour_bit {
            continue;
        }
        in_draft = false;
        out.push(part);
    }
    out.join("+")
}

pub fn context_fingerprint(
    chat: &[(String, String)],
    draft: &str,
    last_failed: bool,
    hour: u8,
    mode: &str,
) -> String {
    let c = detect_chip_context(chat);
    let stage = detect_chip_stage(chat, last_failed);
    let mut bits = vec![stage.as_str().to_string()];
    if c.code {
        bits.push("code".into());
    }
    if c.app {
        bits.push("app".into());
    }
    if c.host {
        bits.push("host".into());
    }
    if c.imagine {
        bits.push("imagine".into());
    }
    if c.error {
        bits.push("error".into());
    }
    if c.ui {
        bits.push("ui".into());
    }
    if c.decide {
        bits.push("decide".into());
    }
    if c.implement {
        bits.push("impl".into());
    }
    if c.incomplete {
        bits.push("todo".into());
    }
    // An opaque draft digest: the fingerprint still changes as the draft
    // does, but no draft text rides along into a stored context tag.
    let draft = normalize_chip_value(draft);
    if !draft.is_empty() {
        let hex = hex::encode(Sha256::digest(draft.as_bytes()));
        bits.push(format!("d:{}", &hex[..8]));
    }
    bits.push(format!("h{}", hour / 6));
    let mode_bit = mode.trim().to_ascii_lowercase();
    if !mode_bit.is_empty() {
        bits.push(format!("m:{mode_bit}"));
    }
    bits.join("+")
}

/// How the cabin should sit with this person. Local only. No model call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CabinPace {
    /// Just arrived. A few plain steps, then stop talking.
    Guide,
    /// Working together. Chips follow the chat and what they actually do.
    With,
    /// Already fluent. Stay quiet. Offer only a repeated action.
    Quiet,
}

pub fn cabin_pace(input: &ChipInput<'_>) -> CabinPace {
    let uses: u32 = input.memory.hits.iter().map(|h| h.uses).sum();
    let mode = input.session_mode.trim().to_ascii_lowercase();
    let familiar = !input.last_slash.trim().is_empty()
        || !input.last_surface.trim().is_empty()
        || !input.last_project.trim().is_empty()
        || uses >= 2
        || input.skill_count > 0
        || input.usage_messages >= 20
        || (!mode.is_empty() && mode != "chat" && mode != "auto");
    let touring = input.chat.iter().all(|(role, _)| role != "user");
    let drafting = !input.draft.trim().is_empty();
    let waved_off = guide_dismisses(input.memory) >= 1;
    if touring
        && !familiar
        && !waved_off
        && !drafting
        && (input.first_run || input.usage_messages < 8)
    {
        return CabinPace::Guide;
    }
    let advanced = input.skill_count >= 2
        || input.usage_messages >= 40
        || uses >= 8
        || (waved_off && familiar);
    if advanced {
        CabinPace::Quiet
    } else {
        CabinPace::With
    }
}

fn guide_dismisses(memory: &ChipMemory) -> u32 {
    memory
        .hits
        .iter()
        .filter(|h| h.dismisses > 0 && chip_family(&h.key, &h.value, &h.label) == "guide")
        .map(|h| h.dismisses)
        .sum()
}

fn chip_family(id: &str, value: &str, label: &str) -> &'static str {
    let blob = format!("{id} {value} {label}").to_ascii_lowercase();
    if blob.contains("guide-") || blob.contains("start here") || blob.contains("what you can do") {
        return "guide";
    }
    if blob.contains("imagine") || blob.contains("draw a logo") || blob.contains("logo") {
        return "imagine";
    }
    if blob.contains("night") {
        return "night";
    }
    if blob.contains("history") || blob.contains("recent chat") {
        return "history";
    }
    ""
}

fn family_quiet_this_hour(
    memory: &ChipMemory,
    id: &str,
    value: &str,
    label: &str,
    hour: u8,
) -> bool {
    let family = chip_family(id, value, label);
    if family.is_empty() {
        return false;
    }
    let hour_i = (hour as usize).min(23);
    memory.hits.iter().any(|hit| {
        chip_family(&hit.key, &hit.value, &hit.label) == family
            && hit.dismiss_hours.get(hour_i).copied().unwrap_or(0) > 0
    })
}

/// Id prefix of the starter chips below.
pub const SEED_PREFIX: &str = "seed-";

/// Loadout's starter chips (the deck's Hangout four plus Home's picks). They
/// rank below anything the chat, history or signals suggest, so they only fill
/// a thin row, and they dismiss and learn like any catalog chip (the label is
/// fixed copy, never chat text).
pub const SEED_CHIPS: &[(&str, &str)] = &[
    ("stutter", "Fix my stutter"),
    ("duo", "Find a duo"),
    ("roast", "Roast my build"),
    ("weekend", "Plan my weekend"),
    ("storage", "Free up storage"),
    ("week", "Plan my week"),
];

const SEED_SCORE: f32 = 30.0;

fn seed_chips(stage: ChipStage) -> Vec<QuickChip> {
    // Mid-chat they sink further so the conversation's own chips lead.
    let base = if stage == ChipStage::Empty {
        SEED_SCORE
    } else {
        SEED_SCORE - 15.0
    };
    SEED_CHIPS
        .iter()
        .enumerate()
        .map(|(i, (key, label))| {
            chip(
                &format!("{SEED_PREFIX}{key}"),
                label,
                label,
                ChipKind::Chat,
                // Keep the listed order among equals.
                base - i as f32 * 0.1,
                "",
            )
        })
        .collect()
}

fn guide_chips(connected: bool) -> Vec<QuickChip> {
    let mut out = Vec::new();
    if !connected {
        out.push(chip(
            "guide-connect",
            "Connect",
            "__nav:settings",
            ChipKind::Nav,
            130.0,
            "Sign in first",
        ));
    }
    out.push(chip(
        "guide-start",
        "Start here",
        "Walk me through this app in a few short steps. Plain words. One step at a time.",
        ChipKind::Chat,
        120.0,
        "New here",
    ));
    out.push(chip(
        "guide-can",
        "What you can do",
        "Tell me three things you can do on this computer for me. Short and plain.",
        ChipKind::Chat,
        110.0,
        "New here",
    ));
    out.push(chip(
        "guide-ideas",
        "Show Pulse",
        "Explain the Pulse page (For You and Feed) in two sentences, then tell me how to open it.",
        ChipKind::Chat,
        100.0,
        "New here",
    ));
    out
}

pub fn predict_intents(chat: &[(String, String)], draft: &str) -> BTreeMap<PredictedIntent, f32> {
    let mut scores = BTreeMap::new();
    for i in PredictedIntent::all() {
        scores.insert(
            i,
            if i == PredictedIntent::Chat {
                0.15
            } else {
                0.0
            },
        );
    }
    let asst = last_of(chat, "assistant");
    let user = last_of(chat, "user");
    let draft_l = draft.to_ascii_lowercase();
    let blob = format!("{user}\n{asst}\n{draft_l}").to_ascii_lowercase();

    if regexish(
        &asst.to_ascii_lowercase(),
        &[
            "let me",
            "i'll",
            "i will",
            "looking into",
            "running checks",
            "continuing",
        ],
    ) || (regexish(
        &asst.to_ascii_lowercase(),
        &["check", "probe", "investigate"],
    ) && !asst.to_ascii_lowercase().contains("host_cmd"))
    {
        bump(&mut scores, PredictedIntent::Finish, 0.55);
        bump(&mut scores, PredictedIntent::Host, 0.25);
    }
    if regexish(&blob, &["error", "fail", "crash", "broken", "exception"]) {
        bump(&mut scores, PredictedIntent::Fix, 0.5);
    }
    if asst.contains("```")
        || user.contains("```")
        || regexish(
            &blob,
            &["function", "const ", "import ", "typescript", "refactor"],
        )
    {
        bump(&mut scores, PredictedIntent::Review, 0.4);
        bump(&mut scores, PredictedIntent::Ship, 0.2);
    }
    if regexish(
        &format!("{user} {draft_l}"),
        &[
            "add ",
            "implement",
            "build ",
            "create ",
            "make ",
            "wire ",
            "patch",
            "fix ",
        ],
    ) {
        bump(&mut scores, PredictedIntent::Ship, 0.45);
    }
    if regexish(
        &format!("{user} {draft_l}"),
        &["explain", "how does", "what is", "why ", "walk me"],
    ) {
        bump(&mut scores, PredictedIntent::Explain, 0.45);
    }
    if regexish(
        &blob,
        &[
            "host_",
            "desktop host",
            "$ ",
            "journalctl",
            "process",
            "system",
        ],
    ) {
        bump(&mut scores, PredictedIntent::Host, 0.4);
    }
    if regexish(
        &blob,
        &["should i", "which", "options", "recommend", "tradeoff"],
    ) {
        bump(&mut scores, PredictedIntent::Decide, 0.4);
    }
    if regexish(
        &blob,
        &["imagine", "image", "draw", "logo", "generate an image"],
    ) {
        bump(&mut scores, PredictedIntent::Create, 0.5);
    }
    if regexish(
        &format!("{user} {draft_l}"),
        &["continue", "keep going", "go on", "resume", "next"],
    ) {
        bump(&mut scores, PredictedIntent::Continue, 0.5);
        bump(&mut scores, PredictedIntent::Finish, 0.2);
    }
    if draft_l.len() >= 2 {
        if draft_l.starts_with("fix")
            || draft_l.starts_with("bug")
            || draft_l.starts_with("error")
            || draft_l.starts_with("crash")
        {
            bump(&mut scores, PredictedIntent::Fix, 0.35);
        }
        if ["add", "implement", "build", "make", "create"]
            .iter()
            .any(|p| draft_l.starts_with(p))
        {
            bump(&mut scores, PredictedIntent::Ship, 0.35);
        }
        if ["explain", "how", "why", "what"]
            .iter()
            .any(|p| draft_l.starts_with(p))
        {
            bump(&mut scores, PredictedIntent::Explain, 0.35);
        }
        if ["check", "scan", "look", "ps ", "find ", "ls "]
            .iter()
            .any(|p| draft_l.starts_with(p))
        {
            bump(&mut scores, PredictedIntent::Host, 0.35);
        }
        if ["continue", "keep", "finish"]
            .iter()
            .any(|p| draft_l.starts_with(p))
        {
            bump(&mut scores, PredictedIntent::Finish, 0.4);
        }
        if ["imagine", "draw", "image"]
            .iter()
            .any(|p| draft_l.starts_with(p))
        {
            bump(&mut scores, PredictedIntent::Create, 0.4);
        }
        if draft_l.starts_with('$') || draft_l.starts_with("/sh") {
            bump(&mut scores, PredictedIntent::Host, 0.5);
        }
    }
    scores
}

fn bump(scores: &mut BTreeMap<PredictedIntent, f32>, intent: PredictedIntent, by: f32) {
    *scores.entry(intent).or_insert(0.0) += by;
}

pub fn top_intent_label(intents: &BTreeMap<PredictedIntent, f32>) -> Option<&'static str> {
    let (intent, score) = intents
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))?;
    if *score < 0.35 {
        return None;
    }
    Some(match intent {
        PredictedIntent::Finish => "finish work",
        PredictedIntent::Fix => "fix something",
        PredictedIntent::Ship => "build/ship",
        PredictedIntent::Explain => "explain",
        PredictedIntent::Host => "use desktop host",
        PredictedIntent::Decide => "decide",
        PredictedIntent::Create => "create media",
        PredictedIntent::Continue => "continue",
        PredictedIntent::Review => "review code",
        PredictedIntent::Chat => "chat",
    })
}

fn chip(id: &str, label: &str, value: &str, kind: ChipKind, score: f32, hint: &str) -> QuickChip {
    QuickChip {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        kind,
        score,
        hint: hint.into(),
        primary: false,
    }
}

/// Visible chip label: collapse whitespace, drop mid-phrase `…`. Paint wraps.
pub(crate) fn chip_label(s: &str) -> String {
    s.split_whitespace()
        .flat_map(|w| w.split('…'))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn topic_from_text(text: &str) -> String {
    let plain = text
        .replace("```", " ")
        .replace(['#', '>', '*', '_', '-'], " ");
    let sentence = plain
        .split(['.', '!', '?', '\n'])
        .map(str::trim)
        .find(|s| s.len() > 16)
        .unwrap_or(plain.trim());
    let stop = [
        "a", "an", "the", "and", "or", "but", "if", "then", "so", "to", "of", "in", "on", "for",
        "with", "from", "at", "by", "as", "is", "are", "was", "were", "be", "i", "me", "my", "we",
        "you", "your", "it", "its", "this", "that",
    ];
    sentence
        .split_whitespace()
        .filter(|w| w.len() > 2 && !stop.contains(&w.to_ascii_lowercase().as_str()))
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
}

fn last_user_wants_gui(chat: &[(String, String)]) -> bool {
    user_asks_gui_help(&last_of(chat, "user"))
}

fn finish_tools_prompt(chat: &[(String, String)]) -> &'static str {
    if last_user_wants_gui(chat) {
        "Don't just plan — drive the desktop now with computer-use. Summarize when done."
    } else {
        "Don't just plan — actually investigate my machine now with your tools. Summarize when done."
    }
}

fn finish_job_prompt(chat: &[(String, String)]) -> &'static str {
    if last_user_wants_gui(chat) {
        "Finish the incomplete work from your last reply. Act now (computer-use if needed). End with status."
    } else {
        "Finish the incomplete work from your last reply. Act now with your tools if needed. End with status."
    }
}

fn chips_from_last_assistant(chat: &[(String, String)]) -> Vec<QuickChip> {
    let asst = last_of(chat, "assistant");
    if asst.len() < 24 {
        return vec![];
    }
    let topic = topic_from_text(&asst);
    let topic_bit = if topic.is_empty() {
        String::new()
    } else {
        format!(" ({topic})")
    };
    let has_code = asst.contains("```");
    let has_error = regexish(
        &asst.to_ascii_lowercase(),
        &[
            "error",
            "fail",
            "exception",
            "couldn't complete",
            "not a function",
        ],
    );
    let has_host = regexish(&asst, &["HOST_CMD", "HOST_RESULT", "desktop host"]);
    let is_plan = regexish(
        &asst.to_ascii_lowercase(),
        &["i'll", "i will", "let me", "would you like me to"],
    ) && regexish(
        &asst.to_ascii_lowercase(),
        &["check", "probe", "investigate", "run"],
    );
    let mut out = vec![];
    if has_error {
        out.push(chip(
            "last-diagnose",
            "Explain & fix error",
            &format!(
                "Looking at your last reply, diagnose the error in plain English. Give: (1) root cause, (2) the exact fix, (3) how to verify it worked.{}",
                if topic.is_empty() { String::new() } else { format!(" Focus on: {topic}.") }
            ),
            ChipKind::Chat,
            96.0,
            "Because the last reply had an error",
        ));
    }
    if has_code {
        out.push(chip(
            "last-code-bugs",
            &chip_label(&format!("Find bugs{topic_bit}")),
            "Review the code in your last message for bugs and edge cases. List issues by severity with concrete fixes.",
            ChipKind::Chat,
            94.0,
            "Because the last reply included code",
        ));
        out.push(chip(
            "last-code-tighten",
            "3 concrete improvements",
            "From the code in your last message, list exactly 3 high-impact improvements. For each: what to change, where, and how we'll know it worked.",
            ChipKind::Chat,
            91.0,
            "Code follow-up",
        ));
    }
    if has_host || is_plan {
        out.push(chip(
            "last-run-host",
            if is_plan {
                "Finish — run tools now"
            } else {
                "Run diagnostics now"
            },
            finish_tools_prompt(chat),
            ChipKind::Chat,
            97.0,
            "Predicted: you need action, not another plan",
        ));
    }
    if !has_code && !has_error && asst.len() > 80 {
        out.push(chip(
            "last-shorter",
            "Shorter version",
            "Rewrite your last answer in half the length. Keep only decisions, commands, and next steps.",
            ChipKind::Chat,
            78.0,
            "Compress the last reply",
        ));
        out.push(chip(
            "last-checklist",
            "Make a checklist",
            "Turn your last answer into a short checklist I can execute.",
            ChipKind::Chat,
            76.0,
            "Action list",
        ));
    }
    out
}

pub fn chip_thread_from_messages(title: &str, messages: &[(String, String)]) -> Option<ChipThread> {
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    let last_user_raw = last_of(messages, "user");
    let last_assistant_raw = last_of(messages, "assistant");
    let last_user = if is_plain_text(&last_user_raw) {
        last_user_raw
    } else {
        String::new()
    };
    let last_assistant = if is_plain_text(&last_assistant_raw) {
        last_assistant_raw
    } else {
        String::new()
    };
    if last_user.trim().is_empty() && last_assistant.trim().is_empty() {
        return None;
    }
    Some(ChipThread {
        title: title.to_string(),
        last_user,
        last_assistant,
    })
}

fn skip_other_thread_title(title: &str) -> bool {
    let t = title.trim();
    t.is_empty() || t.eq_ignore_ascii_case("chat") || t.eq_ignore_ascii_case("scratch")
}

fn chips_from_other_threads(threads: &[ChipThread]) -> Vec<QuickChip> {
    let mut out = vec![];
    for (i, t) in threads.iter().enumerate() {
        if skip_other_thread_title(&t.title) || !is_plain_text(&t.title) {
            continue;
        }
        let last_user = t.last_user.trim();
        let last_asst = t.last_assistant.trim();
        if last_user.is_empty() && last_asst.is_empty() {
            continue;
        }
        if !last_user.is_empty() && !is_plain_text(last_user) {
            continue;
        }
        let label = chip_label(&format!("Continue {}", t.title.trim()));
        let value = format!(
            "Continue the work from the chat \"{}\". Pick up the unfinished step and act now.",
            t.title.trim()
        );
        if !is_plain_text(&value) {
            continue;
        }
        out.push(chip(
            &format!("prev-{i}"),
            &label,
            &value,
            ChipKind::Chat,
            86.0 - (i as f32 * 3.0),
            "From a previous chat",
        ));
        if i == 0 && last_asst.len() >= 24 && is_plain_text(last_asst) {
            let follow = chips_from_last_assistant(&[("assistant".into(), last_asst.to_string())]);
            if let Some(mut c) = follow.into_iter().next() {
                c.id = format!("prev-act-{i}");
                c.score = 84.0 - (i as f32);
                c.value = format!(
                    "In the previous chat \"{}\", take the next step. {}",
                    t.title.trim(),
                    c.value
                );
                c.hint = format!("From a previous reply in {}", t.title.trim());
                if is_plain_text(&c.value) && is_plain_text(&c.label) {
                    out.push(c);
                }
            }
        }
        if out.len() >= 2 {
            break;
        }
    }
    out
}

/// `(trigger word, label, prompt builder, score)` for a draft-predicted chip.
type DraftTemplate<'a> = (&'a str, &'a str, fn(&str) -> (String, ChipKind), f32);

fn draft_prediction_chips(draft_raw: &str) -> Vec<QuickChip> {
    let draft = draft_raw.trim();
    if draft.len() < 2 {
        return vec![];
    }
    let lower = draft.to_ascii_lowercase();
    if draft.starts_with('$') || lower.starts_with("/sh ") {
        // Keep the `$` / `/sh ` prefix: Shell chips strip it when they run.
        return vec![chip(
            "pred-shell-run",
            "Run this on host",
            draft,
            ChipKind::Shell,
            130.0,
            "Predicted from your draft",
        )];
    }
    let templates: &[DraftTemplate] = &[
        (
            "fix",
            "Debug with evidence",
            |d| {
                (format!("{d}. Investigate with your tools for real evidence. Root cause, fix, verify."), ChipKind::Chat)
            },
            125.0,
        ),
        (
            "debug",
            "Debug with evidence",
            |d| {
                (format!("{d}. Investigate with your tools for real evidence. Root cause, fix, verify."), ChipKind::Chat)
            },
            125.0,
        ),
        (
            "add",
            "Implement this now",
            |d| {
                (
                    format!(
                        "{d}. Ship a minimal solid slice. Inspect files if needed, then apply."
                    ),
                    ChipKind::Chat,
                )
            },
            124.0,
        ),
        (
            "implement",
            "Implement this now",
            |d| (format!("{d}. Ship a minimal solid slice."), ChipKind::Chat),
            124.0,
        ),
        (
            "build",
            "Implement this now",
            |d| (format!("{d}. Ship a minimal solid slice."), ChipKind::Chat),
            124.0,
        ),
        (
            "explain",
            "Explain simply",
            |d| {
                (
                    format!("{d}. Plain language, one example, one practical takeaway."),
                    ChipKind::Chat,
                )
            },
            120.0,
        ),
        (
            "how",
            "Explain simply",
            |d| {
                (
                    format!("{d}. Plain language, one example, one practical takeaway."),
                    ChipKind::Chat,
                )
            },
            120.0,
        ),
        (
            "check",
            "Check it",
            |d| {
                (
                    format!("{d}. Inspect the tree and summarize results clearly."),
                    ChipKind::Chat,
                )
            },
            123.0,
        ),
        (
            "continue",
            "Continue until done",
            |d| {
                (
                    format!("{d}. Complete the goal fully. Act now — no planning-only replies."),
                    ChipKind::Chat,
                )
            },
            126.0,
        ),
        (
            "finish",
            "Continue until done",
            |d| (format!("{d}. Complete the goal fully."), ChipKind::Chat),
            126.0,
        ),
        (
            "imagine",
            "Open Imagine for this",
            |_| ("__nav:imagine".into(), ChipKind::Nav),
            115.0,
        ),
        (
            "draw",
            "Open Imagine for this",
            |_| ("__nav:imagine".into(), ChipKind::Nav),
            115.0,
        ),
    ];
    for (prefix, label, build, score) in templates {
        if lower.starts_with(prefix) {
            let (value, kind) = build(draft);
            return vec![chip(
                &format!("pred-draft-{prefix}"),
                label,
                &value,
                kind,
                *score,
                "Predicted from what you're typing",
            )];
        }
    }
    let completions: &[(&str, &str, &str)] = &[
        (
            "fix the",
            "Fix the bug we hit",
            "Diagnose and fix the latest bug. Root cause, exact fix, verify steps.",
        ),
        (
            "fix o",
            "Fix OAuth / session",
            "Fix Grok OAuth or session issues. Check tokens, refresh, and reconnect path.",
        ),
        (
            "add ",
            "Add a feature",
            "Implement the feature I'm describing as a minimal solid slice.",
        ),
        (
            "check c",
            "Check CPU / processes",
            "Read top CPU and memory processes and summarize what's hot.",
        ),
        (
            "how d",
            "How does this work?",
            "Explain how this works simply with one example.",
        ),
        (
            "contin",
            "Continue the task",
            "Continue until the current goal is fully done. Use tools when needed.",
        ),
        (
            "summar",
            "Summarize the thread",
            "Summarize this thread: goals, decisions, open questions, next steps.",
        ),
    ];
    let mut out = vec![];
    if draft.len() >= 3 && draft.len() <= 40 {
        for (prefix, label, value) in completions {
            if lower.starts_with(prefix) || prefix.starts_with(&lower) {
                out.push(chip(
                    &format!("pred-prefix-{}", prefix.trim()),
                    label,
                    value,
                    ChipKind::Chat,
                    112.0,
                    "Predicted completion",
                ));
                if out.len() >= 2 {
                    break;
                }
            }
        }
    }
    if draft.len() >= 8 && draft.len() <= 60 && out.len() < 2 {
        out.push(chip(
            "pred-expand-draft",
            "Expand & send",
            &format!("{draft}. Be concrete and complete the goal. Use your tools if you need machine data."),
            ChipKind::Chat,
            100.0,
            "Predicted expansion of your draft",
        ));
    }
    out
}

fn bump_hour(hour_hits: &[u32], hour: u8) -> Vec<u32> {
    let mut arr = if hour_hits.len() == 24 {
        hour_hits.to_vec()
    } else {
        vec![0; 24]
    };
    let i = (hour as usize).min(23);
    arr[i] = arr[i].saturating_add(1);
    arr
}

fn upsert_hit(
    memory: &mut ChipMemory,
    key: String,
    label: &str,
    value: &str,
    kind: ChipKind,
    uses_delta: u32,
    typed_delta: u32,
    dismiss_delta: u32,
    context_tag: Option<&str>,
    now_ms: u64,
    hour: u8,
) {
    if !is_plain_text(value) || !is_plain_text(label) {
        return;
    }
    if let Some(h) = memory.hits.iter_mut().find(|h| h.key == key) {
        h.label = chip_label(label);
        h.value = value.chars().take(400).collect();
        h.kind = kind;
        h.uses = h.uses.saturating_add(uses_delta);
        h.typed_uses = h.typed_uses.saturating_add(typed_delta);
        if uses_delta > 0 && typed_delta == 0 {
            h.picks = h.picks.saturating_add(uses_delta);
        }
        if uses_delta > 0 || typed_delta > 0 {
            h.last_used_at = now_ms;
            h.hour_hits = bump_hour(&h.hour_hits, hour);
        }
        if dismiss_delta > 0 {
            h.dismiss_hours = bump_hour(&h.dismiss_hours, hour);
            h.last_dismissed_at = now_ms;
        }
        h.dismisses = h.dismisses.saturating_add(dismiss_delta);
        if let Some(tag) = context_tag.map(tag_without_draft) {
            if !h.context_tags.contains(&tag) {
                h.context_tags.insert(0, tag);
                h.context_tags.truncate(12);
            }
        }
    } else {
        memory.hits.insert(
            0,
            ChipHit {
                key,
                label: chip_label(label),
                value: value.chars().take(400).collect(),
                kind,
                uses: uses_delta,
                typed_uses: typed_delta,
                picks: if typed_delta == 0 { uses_delta } else { 0 },
                last_used_at: if uses_delta > 0 || typed_delta > 0 {
                    now_ms
                } else {
                    0
                },
                hour_hits: if uses_delta > 0 || typed_delta > 0 {
                    bump_hour(&[], hour)
                } else {
                    vec![0; 24]
                },
                successes: 0,
                failures: 0,
                context_tags: context_tag
                    .map(|t| vec![tag_without_draft(t)])
                    .unwrap_or_default(),
                dismisses: dismiss_delta,
                dismiss_hours: if dismiss_delta > 0 {
                    bump_hour(&[], hour)
                } else {
                    vec![0; 24]
                },
                last_dismissed_at: if dismiss_delta > 0 { now_ms } else { 0 },
            },
        );
    }
    memory.hits.sort_by(|a, b| {
        let sa = a.uses * 2 + a.typed_uses;
        let sb = b.uses * 2 + b.typed_uses;
        sb.cmp(&sa)
    });
    memory.hits.truncate(MAX_HITS);
    memory.total_events = memory.total_events.saturating_add(1);
    memory.updated_at = now_ms;
}

fn record_transition(memory: &mut ChipMemory, from: Option<String>, to: String) {
    if let Some(from) = from {
        if from != to {
            let row = memory.transitions.entry(from).or_default();
            *row.entry(to.clone()).or_insert(0) += 1;
            if row.len() > MAX_TRANSITIONS {
                let mut pairs: Vec<_> = row.iter().map(|(k, v)| (k.clone(), *v)).collect();
                pairs.sort_by_key(|p| std::cmp::Reverse(p.1));
                row.clear();
                for (k, v) in pairs.into_iter().take(MAX_TRANSITIONS) {
                    row.insert(k, v);
                }
            }
        }
    }
    memory.last_chip_key = Some(to);
}

/// Chat chips whose text is built from the chat, the draft, the thread
/// title or a model reply. Catalog chips (`empty-brief`, `ctx-draw`) are
/// fixed strings and keep theirs.
const CHAT_TEXT_CHIP_IDS: &[&str] = &[
    "llm-", "reply-", "learn-", "recent-", "prev-", "pred-", "last-", "topic-",
];

fn chip_text_from_chat(id: &str) -> bool {
    CHAT_TEXT_CHIP_IDS.iter().any(|p| id.starts_with(p))
}

/// Chips a feature producer builds from live state (a file that landed, a
/// repo, a game, the app in focus). Their text names things that may be
/// gone tomorrow, so a habit never replays it.
const LIVE_STATE_CHIP_IDS: &[&str] = &[
    "model:",
    "resume:",
    "backup:",
    "tidy:",
    "radar:",
    "game:",
    "focus:",
    "sig-attach",
];

/// Chips offered unasked: the PC's and the day's signals, other threads and
/// the app's feature offers. They wait while the person is gaming.
const NUDGE_CHIP_IDS: &[&str] = &[
    "sig-",
    "prev-",
    "home-continue",
    "model:",
    "resume:",
    "backup:",
    "tidy:",
    "radar:",
    "focus:",
];

/// True for a nudge that waits while the person is gaming. Game chips stay:
/// the game is what they are doing.
fn waits_while_gaming(c: &QuickChip) -> bool {
    NUDGE_CHIP_IDS.iter().any(|p| c.id.starts_with(p))
        && !c.id.starts_with("sig-attach")
        && !touches_the_game(c)
}

/// True for a chip an anti-cheat could object to: it is about the game,
/// reads the window in front, or runs a command on the host.
fn touches_the_game(c: &QuickChip) -> bool {
    c.kind == ChipKind::Shell
        || c.id.starts_with("game:")
        || c.id.starts_with("sig-game")
        || c.id.starts_with("focus:")
}

/// True when text names a full path: `/home/x`, `~/x`, `C:\x`.
fn names_full_path(text: &str) -> bool {
    text.split(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        .any(|w| {
            let w = w.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | ';'));
            w.starts_with("~/")
                || (w.starts_with('/') && w[1..].contains('/'))
                || (w.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                    && w.get(1..3) == Some(":\\"))
        })
}

/// A chip whose text must never come back as a habit: shell lines run on
/// the host, and live-state chips or full paths go stale. Its opaque key
/// still carries the habit for ranking.
fn replay_unsafe(kind: ChipKind, id: &str, label: &str, value: &str) -> bool {
    kind == ChipKind::Shell
        || LIVE_STATE_CHIP_IDS.iter().any(|p| id.starts_with(p))
        || names_full_path(label)
        || names_full_path(value)
}

/// The label and value a chip leaves in chips.json. A Chat chip built from
/// chat text keeps neither; its opaque key carries the habit. Shell lines,
/// live-state chips and full paths keep neither too (see [`replay_unsafe`]).
fn stored_text(chip: &QuickChip) -> (&str, &str) {
    if (chip.kind == ChipKind::Chat && chip_text_from_chat(&chip.id))
        || replay_unsafe(chip.kind, &chip.id, &chip.label, &chip.value)
    {
        ("", "")
    } else {
        (&chip.label, &chip.value)
    }
}

pub fn remember_chip_click(
    memory: &mut ChipMemory,
    chip: &QuickChip,
    context_tag: Option<&str>,
    now_ms: u64,
    hour: u8,
) {
    let key = chip_memory_key(chip);
    let from = memory.last_chip_key.clone();
    let (label, value) = stored_text(chip);
    upsert_hit(
        memory,
        key.clone(),
        label,
        value,
        chip.kind,
        1,
        0,
        0,
        context_tag,
        now_ms,
        hour,
    );
    record_transition(memory, from, key);
}

/// One dismiss is enough. The chip stays out of later rows, including after a restart.
pub fn chip_dismissed_for_good(memory: &ChipMemory, chip: &QuickChip) -> bool {
    let key = chip_memory_key(chip);
    let value = chip.value.trim();
    memory.hits.iter().any(|hit| {
        hit.dismisses >= 1
            && (hit.key == key
                || (!value.is_empty() && hit.value.trim().eq_ignore_ascii_case(value)))
    })
}

pub fn remember_chip_dismiss(memory: &mut ChipMemory, chip: &QuickChip, now_ms: u64, hour: u8) {
    let key = chip_memory_key(chip);
    let (label, value) = stored_text(chip);
    upsert_hit(
        memory, key, label, value, chip.kind, 0, 0, 1, None, now_ms, hour,
    );
}

pub fn remember_typed_prompt(memory: &mut ChipMemory, text: &str, now_ms: u64, hour: u8) {
    let raw = text.trim();
    if raw.len() < 2 || raw.starts_with("[Automation:") || raw.starts_with('/') {
        return;
    }
    if !is_plain_text(raw) {
        return;
    }
    let kind = if raw.starts_with('$') || raw.starts_with("/sh ") {
        ChipKind::Shell
    } else {
        ChipKind::Chat
    };
    // Free-form chat asks must never land in chips.json as keys, labels, or
    // habit values. Only reinforce chips the person already picked.
    if kind == ChipKind::Chat {
        boost_matching(memory, raw, now_ms);
        return;
    }
    let key = value_key(raw, kind);
    debug_assert!(
        !chip_key_embeds_chat_text(&key),
        "value_key must stay opaque: {key}"
    );
    // A typed shell line keeps only its opaque key: replayed later as a
    // chip it could run against a folder or file that is gone.
    let from = memory.last_chip_key.clone();
    upsert_hit(
        memory,
        key.clone(),
        "",
        "",
        kind,
        1,
        1,
        0,
        None,
        now_ms,
        hour,
    );
    record_transition(memory, from, key);
    boost_matching(memory, raw, now_ms);
}

fn boost_matching(memory: &mut ChipMemory, text: &str, now_ms: u64) {
    let lower = text.to_ascii_lowercase();
    let tokens: Vec<_> = lower
        .split_whitespace()
        .filter(|t| t.len() > 3)
        .take(8)
        .collect();
    if tokens.is_empty() {
        return;
    }
    for h in &mut memory.hits {
        let hay = format!("{} {}", h.label, h.value).to_ascii_lowercase();
        let match_n = tokens.iter().filter(|t| hay.contains(*t)).count();
        if match_n == 0 {
            continue;
        }
        h.uses = h.uses.saturating_add(if match_n >= 2 { 1 } else { 0 });
    }
    memory.updated_at = now_ms;
}

/// Cabin verbs that are worth remembering as a next-chip. Not /update, /clear, /new.
pub fn home_slash_cmd(kind: &str) -> Option<&'static str> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "plan" | "/plan" => Some("/plan"),
        "reflect" | "/learn" | "learn" => Some("/learn"),
        "imagine" | "/imagine" => Some("/imagine"),
        "skill" | "grok_skills" | "/skill" | "/skills" | "skills" => Some("/skills"),
        "board" | "/board" => Some("/board"),
        "dream" | "/dream" => Some("/dream"),
        "project_show" | "project_bind" | "/project" | "project" => Some("/project"),
        _ => None,
    }
}

pub fn remember_home_slash(memory: &mut ChipMemory, cmd: &str, now_ms: u64) {
    let Some(verb) = home_slash_cmd(cmd) else {
        return;
    };
    memory.last_slash = Some(verb.to_string());
    memory.updated_at = now_ms;
}

pub fn remember_home_surface(memory: &mut ChipMemory, surface: &str, now_ms: u64) {
    let s = surface.trim().to_ascii_lowercase();
    if !matches!(s.as_str(), "chat" | "imagine" | "skills") {
        return;
    }
    memory.last_surface = Some(s);
    memory.updated_at = now_ms;
}

/// Sidebar / palette / slash page ids that should rank home chips after a visit.
pub fn home_surface_from_nav(id: &str) -> Option<&'static str> {
    match id.trim().to_ascii_lowercase().as_str() {
        "imagine" => Some("imagine"),
        "skills" => Some("skills"),
        _ => None,
    }
}

fn home_project_title(raw: &str) -> String {
    let t = raw.trim();
    let t = t
        .strip_prefix("Continue ")
        .or_else(|| t.strip_prefix("continue "))
        .unwrap_or(t)
        .trim();
    if skip_other_thread_title(t) || !is_plain_text(t) {
        return String::new();
    }
    let low = t.to_ascii_lowercase();
    if low.starts_with("failed:") || low.contains("grokhub") {
        return String::new();
    }
    chip_label(t)
}

fn slash_home_chip(cmd: &str, score: f32) -> Option<QuickChip> {
    match home_slash_cmd(cmd)? {
        "/plan" => Some(chip(
            "home-slash-plan",
            "/plan",
            "/plan",
            ChipKind::Chat,
            score,
            "Last slash",
        )),
        "/learn" => Some(chip(
            "home-slash-learn",
            "/learn",
            "/learn",
            ChipKind::Chat,
            score,
            "Last slash",
        )),
        "/dream" => Some(chip(
            "home-slash-dream",
            "/dream",
            "/dream",
            ChipKind::Chat,
            score,
            "Last slash",
        )),
        "/project" => Some(chip(
            "home-slash-project",
            "/project",
            "/project",
            ChipKind::Chat,
            score,
            "Last slash",
        )),
        _ => None,
    }
}

fn home_signal_chips(input: &ChipInput<'_>) -> Vec<QuickChip> {
    let mut out = vec![];
    let project = home_project_title(input.last_project);
    if !project.is_empty() {
        out.push(chip(
            "home-continue",
            &format!("Continue {project}"),
            &format!("Continue the work on {project}. Pick up where we left off and act now."),
            ChipKind::Chat,
            108.0,
            "Last project",
        ));
    }
    if let Some(c) = slash_home_chip(input.last_slash, 106.0) {
        out.push(c);
    } else if input.session_mode.eq_ignore_ascii_case("plan") {
        if let Some(c) = slash_home_chip("/plan", 100.0) {
            out.push(c);
        }
    }
    if input.skill_count > 0 || input.last_surface.trim().eq_ignore_ascii_case("skills") {
        out.push(chip(
            "home-skills",
            "Skills",
            "__nav:skills",
            ChipKind::Nav,
            90.0,
            "Skills you have",
        ));
    }
    out
}

/// Longest game name kept whole in a chip label.
const SIGNAL_GAME_LABEL_MAX: usize = 22;

fn signal_game_label(name: &str) -> String {
    let name = chip_label(name);
    if name.chars().count() <= SIGNAL_GAME_LABEL_MAX {
        name
    } else {
        // Whole words only, without a dangling ":" or "-".
        let cut: String = name.chars().take(SIGNAL_GAME_LABEL_MAX + 1).collect();
        let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
        cut.trim_end_matches([' ', ':', '-', ',']).to_string()
    }
}

/// Chips from what the PC and the day are doing: a game running or just
/// closed, a full drive, an update waiting, a Friday evening. They lead an
/// empty chat and sit under the chat's own chips once it has turns. Quiet
/// hours drop the social one.
fn signal_chips(input: &ChipInput<'_>) -> Vec<QuickChip> {
    let sig = &input.signals;
    let lift = if input.chat.is_empty() { 0.0 } else { -30.0 };
    let mut out = vec![];
    if let Some(game) = sig.game.map(str::trim).filter(|g| !g.is_empty()) {
        out.push(chip(
            "sig-game-stutter",
            &format!("Fix stutter in {}", signal_game_label(game)),
            &format!(
                "{game} is stuttering. Check what else is using this PC right now and tell me how to smooth it out. Don't touch the game or its files."
            ),
            ChipKind::Chat,
            98.0 + lift,
            "Game running",
        ));
    } else if let Some(game) = sig.game_ended.map(str::trim).filter(|g| !g.is_empty()) {
        out.push(chip(
            "sig-game-ended",
            &format!("How'd {} go?", signal_game_label(game)),
            &format!(
                "I just finished playing {game}. Ask me how it went, then give me one thing to try next time."
            ),
            ChipKind::Chat,
            94.0 + lift,
            "Game just closed",
        ));
    }
    if sig.low_disk {
        out.push(chip(
            "sig-low-disk",
            "Free up storage",
            "My disk is almost full. Find what's taking up the most space and tell me what's safe to clear before deleting anything.",
            ChipKind::Chat,
            96.0 + lift,
            "Drive almost full",
        ));
    }
    if sig.update_waiting {
        out.push(chip(
            "sig-update",
            "See the update",
            "__nav:update",
            ChipKind::Nav,
            92.0 + lift,
            "Update waiting",
        ));
    }
    if let Some(name) = sig.attachment.map(str::trim).filter(|n| !n.is_empty()) {
        out.push(chip(
            "sig-attach-explain",
            &chip_label(&format!("Explain {name}")),
            &format!("Explain the file I attached, {name}: what it is and what matters in it."),
            ChipKind::Chat,
            // A file in the tray is about to be sent: it leads either way.
            104.0,
            "File ready to send",
        ));
    }
    if sig.weekday == 5 && input.hour >= 17 && !sig.quiet_hours {
        out.push(chip(
            "sig-weekend",
            "Plan my weekend",
            "Help me plan my weekend. Ask what I'm in the mood for first.",
            ChipKind::Chat,
            90.0 + lift,
            "Friday evening",
        ));
    }
    out
}

fn apply_home_signal_boost(chips: &mut [QuickChip], input: &ChipInput<'_>) {
    let slash = home_slash_cmd(input.last_slash).unwrap_or("");
    let surface = input.last_surface.trim().to_ascii_lowercase();
    let session = input.session_mode.trim().to_ascii_lowercase();
    for c in chips.iter_mut() {
        let id = c.id.to_ascii_lowercase();
        let label = c.label.to_ascii_lowercase();
        let val = c.value.to_ascii_lowercase();
        if surface == "imagine"
            && (val.contains("__nav:imagine")
                || label.contains("imagine")
                || id.contains("imagine"))
        {
            c.score += 22.0;
            if c.hint.is_empty() || c.hint == "Create images" || c.hint == "Images" {
                c.hint = "Last surface".into();
            }
        }
        if (slash == "/imagine" || surface == "imagine")
            && (val.contains("__nav:imagine") || label.contains("imagine"))
        {
            c.score += 8.0;
        }
        if (slash == "/skills" || input.skill_count > 0 || surface == "skills")
            && (val.contains("__nav:skills") || label == "skills")
        {
            c.score += if input.skill_count > 0 { 56.0 } else { 28.0 };
            if c.hint == "Default" {
                c.hint = "Skills you have".into();
            }
        }
        if slash == "/board" && (val.contains("__nav:workboard") || label.contains("workboard")) {
            c.score += 16.0;
        }
        if input.first_run {
            if val.contains("__nav:imagine") || label.contains("imagine") {
                c.score += 18.0;
            }
            if val.contains("__nav:skills") || label == "skills" {
                c.score += 20.0;
            }
            if id.contains("night") || label.contains("night job") || id.contains("github") {
                c.score -= 40.0;
            }
        }
        if session == "plan" && val == "/plan" {
            c.score += 8.0;
        }
        if !home_project_title(input.last_project).is_empty()
            && (id.starts_with("prev-") || id == "home-continue" || label.starts_with("continue "))
        {
            c.score += 12.0;
        }
    }
}

/// Credit how a turn went to the chip that started it, by its memory key.
/// A typed turn has no chip and credits nothing.
pub fn remember_chip_turn_outcome(memory: &mut ChipMemory, key: &str, success: bool, now_ms: u64) {
    if let Some(h) = memory.hits.iter_mut().find(|h| h.key == key) {
        if success {
            h.successes = h.successes.saturating_add(1);
        } else {
            h.failures = h.failures.saturating_add(1);
        }
        h.last_used_at = now_ms;
        memory.updated_at = now_ms;
    }
}

fn memory_boost_for_chip(memory: &ChipMemory, chip: &QuickChip, now_ms: u64, hour: u8) -> f32 {
    if memory.hits.is_empty() {
        return 0.0;
    }
    let key = chip_memory_key(chip);
    let hit = memory.hits.iter().find(|h| h.key == key).or_else(|| {
        memory
            .hits
            .iter()
            .find(|h| h.value.trim().eq_ignore_ascii_case(chip.value.trim()))
    });
    let mut boost = 0.0;
    if let Some(hit) = hit {
        boost += ((1.0 + hit.uses as f32).log2() * 12.0).min(48.0);
        boost += (hit.typed_uses as f32 * 1.5).min(12.0);
        let ok = hit.successes;
        let bad = hit.failures;
        if ok + bad > 0 {
            let rate = ok as f32 / (ok + bad) as f32;
            boost += (rate - 0.5) * 24.0;
            boost += (ok as f32 * 2.0).min(16.0);
            boost -= (bad as f32 * 3.0).min(18.0);
        }
        let days = now_ms.saturating_sub(hit.last_used_at) as f32 / 86_400_000.0;
        boost += (22.0 - days * 3.0).max(0.0);
        let hour_i = (hour as usize).min(23);
        if hit.hour_hits.get(hour_i).copied().unwrap_or(0) > 0 {
            boost += (hit.hour_hits[hour_i] as f32 * 2.0).min(10.0);
        }
        if hit.dismisses > 0 {
            boost -= (hit.dismisses as f32 * 12.0).min(40.0);
        }
        if let Some(last) = &memory.last_chip_key {
            if let Some(n) = memory.transitions.get(last).and_then(|r| r.get(&hit.key)) {
                boost += (*n as f32 * 4.0).min(18.0);
            }
        }
    }
    boost
}

/// Typing something once is not a habit. A typed prompt earns a chip only after
/// it came back `TYPED_HABIT_MIN` times. A chip you picked earns it right away.
pub const TYPED_HABIT_MIN: u32 = 3;

fn learned_hit_earned(h: &ChipHit) -> bool {
    h.picks > 0 || h.typed_uses >= TYPED_HABIT_MIN
}

fn learned_chips_from_memory(memory: &ChipMemory, now_ms: u64) -> Vec<QuickChip> {
    let mut out: Vec<QuickChip> = memory
        .hits
        .iter()
        .filter(|h| h.uses >= 1 && !h.value.trim().is_empty())
        .filter(|h| matches!(h.kind, ChipKind::Chat | ChipKind::Shell))
        .filter(|h| h.dismisses == 0)
        .filter(|h| learned_hit_earned(h))
        .filter(|h| !retired_host_copy(&h.key, &h.label, &h.value))
        .map(|h| {
            let age_days = now_ms.saturating_sub(h.last_used_at) as f32 / 86_400_000.0;
            let score = 25.0
                + ((1.0 + h.uses as f32).log2() * 10.0).min(45.0)
                + (15.0 - age_days * 2.0).max(0.0)
                + if h.typed_uses > 0 { 5.0 } else { 0.0 };
            chip(
                &format!("learn-{}", h.key.chars().take(40).collect::<String>()),
                &h.label,
                &h.value,
                h.kind,
                score,
                if h.uses >= 5 { "habit" } else { "learned" },
            )
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(8);
    out
}

pub fn top_habit_labels(memory: &ChipMemory, n: usize) -> Vec<String> {
    let mut hits = memory.hits.clone();
    hits.retain(|h| h.uses >= 1 && h.dismisses == 0);
    hits.sort_by(|a, b| {
        let sa = a.uses * 2 + a.successes * 3 - a.failures * 2 - a.dismisses * 4;
        let sb = b.uses * 2 + b.successes * 3 - b.failures * 2 - b.dismisses * 4;
        sb.cmp(&sa)
    });
    hits.into_iter().take(n).map(|h| h.label).collect()
}

fn apply_intent_boost(chips: &mut [QuickChip], intents: &BTreeMap<PredictedIntent, f32>) {
    let mut top: Vec<_> = intents.iter().collect();
    top.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    top.truncate(4);
    for c in chips.iter_mut() {
        let hay = format!("{} {} {} {}", c.id, c.label, c.value, c.hint).to_ascii_lowercase();
        let mut boost = 0.0;
        let mut matched = None;
        for (intent, score) in &top {
            if **score < 0.2 {
                continue;
            }
            let hit = match intent {
                PredictedIntent::Finish => regexish(
                    &hay,
                    &["finish", "complete", "continue", "host_cmd", "act now"],
                ),
                PredictedIntent::Fix => {
                    regexish(&hay, &["fix", "diagnos", "error", "root cause", "bug"])
                }
                PredictedIntent::Ship => regexish(
                    &hay,
                    &["implement", "ship", "apply", "patch", "add ", "build"],
                ),
                PredictedIntent::Explain => {
                    regexish(&hay, &["explain", "simple", "plain", "takeaway"])
                }
                PredictedIntent::Host => regexish(
                    &hay,
                    &["host", "system", "process", "shell", "machine", "scan"],
                ),
                PredictedIntent::Decide => {
                    regexish(&hay, &["recommend", "option", "pick", "best", "decide"])
                }
                PredictedIntent::Create => regexish(&hay, &["imagine", "image", "draw", "logo"]),
                PredictedIntent::Continue => {
                    regexish(&hay, &["continue", "keep", "resume", "next step"])
                }
                PredictedIntent::Review => {
                    regexish(&hay, &["review", "bugs", "improve", "refactor", "test"])
                }
                PredictedIntent::Chat => regexish(&hay, &["help", "what can"]),
            };
            if hit {
                boost += **score * 48.0;
                matched = Some(**intent);
            }
        }
        if boost > 0.0 {
            c.score += boost;
            if let Some(m) = matched {
                if intents.get(&m).copied().unwrap_or(0.0) >= 0.35 {
                    c.hint = format!(
                        "Predicted · {}{}",
                        m.as_str(),
                        if c.hint.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", c.hint)
                        }
                    );
                }
            }
        }
    }
}

fn apply_draft_boost(
    chips: &mut [QuickChip],
    draft_raw: &str,
    intents: &BTreeMap<PredictedIntent, f32>,
) {
    let draft = draft_raw.trim().to_ascii_lowercase();
    if draft.is_empty() {
        return;
    }
    for c in chips.iter_mut() {
        let hay = format!("{} {} {}", c.label, c.value, c.hint).to_ascii_lowercase();
        let mut boost = 0.0;
        if c.label.to_ascii_lowercase().starts_with(&draft) {
            boost += 55.0;
        } else if hay.starts_with(&draft) {
            boost += 48.0;
        } else if c.value.to_ascii_lowercase().starts_with(&draft) {
            boost += 42.0;
        } else if hay.contains(&draft) {
            boost += 22.0;
        }
        let tokens: Vec<_> = draft.split_whitespace().filter(|t| t.len() > 2).collect();
        let hits = tokens.iter().filter(|t| hay.contains(**t)).count();
        boost += hits as f32 * 11.0;
        if tokens.len() >= 2 && hits == tokens.len() {
            boost += 28.0;
        }
        if draft.starts_with('$') && c.kind == ChipKind::Shell {
            boost += 50.0;
        }
        if regexish(&draft, &["imagine", "draw", "image"])
            && regexish(&hay, &["imagine", "image", "__nav:imagine"])
        {
            boost += 45.0;
        }
        if regexish(&draft, &["bug", "error", "fix", "crash"])
            && regexish(&hay, &["fix", "diagnos", "error", "bug"])
        {
            boost += 32.0;
        }
        for (intent, score) in intents {
            if *score >= 0.3 {
                let kw = intent.as_str();
                if hay.contains(kw) {
                    boost += *score * 20.0;
                }
            }
        }
        c.score += boost;
        if boost >= 40.0 && !c.hint.contains("Predicted match") {
            c.hint = format!(
                "Predicted match{}",
                if c.hint.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", c.hint)
                }
            );
        }
    }
}

fn stage_chips(stage: ChipStage) -> Vec<QuickChip> {
    match stage {
        ChipStage::Empty => vec![
            chip(
                "empty-imagine",
                "Open Imagine",
                "__nav:imagine",
                ChipKind::Nav,
                70.0,
                "Create images",
            ),
            chip(
                "empty-brief",
                "Quick brief",
                "Give me a short brief: bound project, recent chats, and the next useful step.",
                ChipKind::Chat,
                68.0,
                "New chat",
            ),
            chip(
                "empty-history",
                "Recent chats",
                "__nav:history",
                ChipKind::Nav,
                66.0,
                "History",
            ),
            chip(
                "empty-night",
                "Set a night job",
                "Help me set a night automation. Ask one question, then propose a real schedule I can save.",
                ChipKind::Chat,
                64.0,
                "New chat",
            ),
        ],
        ChipStage::Error => vec![
            chip(
                "err-diagnose",
                "Root cause + fix",
                "Diagnose the error we hit — root cause, exact fix, and how to verify.",
                ChipKind::Chat,
                93.0,
                "Error mentioned in chat",
            ),
            chip(
                "err-retry",
                "Try Again",
                "/retry",
                ChipKind::Chat,
                88.0,
                "Last attempt failed",
            ),
        ],
        ChipStage::Tools => vec![
            chip(
                "tools-sum",
                "Summarize tool results",
                "Summarize the latest tool results in plain language. Call out failures or timeouts.",
                ChipKind::Chat,
                88.0,
                "After tools",
            ),
        ],
        ChipStage::Long => vec![
            chip(
                "long-sum",
                "Summarize the thread",
                "Summarize this thread: goals, decisions, open questions, next steps.",
                ChipKind::Chat,
                80.0,
                "Long thread",
            ),
        ],
        ChipStage::Mid | ChipStage::Default => vec![
            chip(
                "mid-continue",
                "Continue",
                "Continue until the current goal is fully done. Act now — no planning-only replies.",
                ChipKind::Chat,
                74.0,
                "Mid-thread",
            ),
        ],
    }
}

/// Suggestions for one idea pop-out. They follow the card and that talk, not the home row.
pub fn idea_talk_chips(title: &str, body: &str, chat: &[(String, String)]) -> Vec<QuickChip> {
    let blob = format!("{title} {body}").to_ascii_lowercase();
    let mut out = vec![
        chip(
            "idea-skill",
            "Save a skill",
            "Save a skill for this situation. Use the why in the note. Do not copy my earlier words.",
            ChipKind::Chat,
            99.0,
            "This idea",
        ),
        chip(
            "idea-auto",
            "Automate this",
            "Turn this into an automation so I do not have to ask again. Keep the time I write in the note.",
            ChipKind::Chat,
            98.0,
            "This idea",
        ),
        chip(
            "idea-sit",
            "When I sit down",
            crate::brand::filled("Run this when I open {product}."),
            ChipKind::Chat,
            96.0,
            "This idea",
        ),
        chip(
            "idea-weekday",
            "Weekday morning",
            "Set this for weekday mornings and save the time I give you.",
            ChipKind::Chat,
            94.0,
            "This idea",
        ),
        chip(
            "idea-done",
            "Tell me when it's done",
            "When this runs, post the result on the home feed.",
            ChipKind::Chat,
            90.0,
            "This idea",
        ),
    ];
    if blob.contains("night") || blob.contains("wrap") {
        out.push(chip(
            "idea-night",
            "After I stop",
            "Run this once I am done for the day.",
            ChipKind::Chat,
            92.0,
            "This idea",
        ));
    }
    if blob.contains("remind") || blob.contains("ping") || blob.contains("later") {
        out.push(chip(
            "idea-once",
            "Remind me once",
            "Remind me once, at the time I name.",
            ChipKind::Chat,
            93.0,
            "This idea",
        ));
    }
    if let Some((_, last)) = chat.iter().rev().find(|(role, _)| role == "assistant") {
        let lower = last.to_ascii_lowercase();
        if lower.contains("time") || lower.contains("when") || lower.contains("often") {
            out.push(chip(
                "idea-eight",
                "Weekdays at 8",
                "Use 8:00 on weekdays.",
                ChipKind::Chat,
                98.0,
                "This conversation",
            ));
        }
    }
    out.truncate(4);
    out
}

fn is_stale_connect_chip(c: &QuickChip) -> bool {
    c.id == "ctx-connect"
        || c.id.contains("ctx-connect")
        || c.label.eq_ignore_ascii_case("Connect Grok")
        || (c.value == "__nav:settings" && c.label.to_ascii_lowercase().contains("connect"))
}

fn is_desk_takeover_chip(c: &QuickChip) -> bool {
    retired_host_copy(&c.id, &c.label, &c.value)
}

/// Old empty-home chips (Take over / Scan the desk / Check the machine) plus
/// HOST_CMD habits stored in chips.json. Grok Build already has computer-use.
fn retired_host_copy(id: &str, label: &str, value: &str) -> bool {
    let hay = format!("{id} {label} {value}").to_ascii_lowercase();
    hay.contains("take over")
        || hay.contains("this desktop")
        || hay.contains("turn host on")
        || hay.contains("/host on")
        || hay.contains("scan the desk")
        || hay.contains("on my desk")
        || hay.contains("what's on my desk")
        || hay.contains("whats on my desk")
        || hay.contains("check the machine")
        || hay.contains("host_cmd")
        || hay.contains("host_result")
        || label.eq_ignore_ascii_case("voice")
        || label.eq_ignore_ascii_case("hey grok")
        || id == "ctx-host"
        || id == "empty-host"
        || id.starts_with("host-")
        || label.to_ascii_lowercase().contains("desk")
}

/// Drop leftover desk/host habits so chips.json cannot resurrect them.
pub fn prune_retired_chip_memory(mem: &mut ChipMemory) -> bool {
    let modes = prune_retired_mode_hits(mem);
    let retired = prune_retired_host_hits(mem) || modes;
    let scrubbed = scrub_chat_text_chip_keys(mem);
    let unsafe_text = scrub_replay_unsafe_text(mem);
    retired || scrubbed || unsafe_text
}

/// Idempotent scrub: clear the stored text of shell lines, live-state chips
/// and full paths that older builds kept, so no habit chip replays them.
/// The hits stay, so their counts still rank. True when anything changed.
fn scrub_replay_unsafe_text(mem: &mut ChipMemory) -> bool {
    let mut changed = false;
    for h in mem.hits.iter_mut() {
        let id = h.key.strip_prefix("id:").unwrap_or("");
        if (!h.label.is_empty() || !h.value.is_empty())
            && replay_unsafe(h.kind, id, &h.label, &h.value)
        {
            h.label.clear();
            h.value.clear();
            changed = true;
        }
    }
    changed
}

/// Drop old model/effort chip habits. Their keys were `mode:…` (learned) or
/// `id:mode-…`/`id:route-…` (row chips); their values `__mode:…`.
fn prune_retired_mode_hits(mem: &mut ChipMemory) -> bool {
    let gone =
        |k: &str| k.starts_with("mode:") || k.starts_with("id:mode-") || k.starts_with("id:route-");
    let before = mem.hits.len();
    mem.hits
        .retain(|h| !gone(&h.key) && !retired_mode_value(&h.value));
    mem.transitions.retain(|from, dests| {
        dests.retain(|to, _| !gone(to));
        !gone(from) && !dests.is_empty()
    });
    if mem.last_chip_key.as_deref().is_some_and(gone) {
        mem.last_chip_key = None;
    }
    mem.hits.len() != before
}

fn prune_retired_host_hits(mem: &mut ChipMemory) -> bool {
    let before = mem.hits.len();
    mem.hits
        .retain(|h| !retired_host_copy(&h.key, &h.label, &h.value));
    let keys: std::collections::HashSet<String> = mem.hits.iter().map(|h| h.key.clone()).collect();
    mem.transitions.retain(|from, dests| {
        if !keys.contains(from) && retired_host_copy(from, "", "") {
            return false;
        }
        dests.retain(|to, _| keys.contains(to) || !retired_host_copy(to, "", ""));
        !dests.is_empty()
    });
    if mem
        .last_chip_key
        .as_ref()
        .is_some_and(|k| retired_host_copy(k, "", ""))
    {
        mem.last_chip_key = None;
    }
    mem.hits.len() != before
}

/// Idempotent scrub: drop any habit whose key was built from raw chat/shell
/// text or a model chip's label (`id:llm-…`), and clear the text older Chat
/// chip clicks stored. Catalog `id:…` and opaque `*:v1:hex` keys stay.
/// True when anything changed, nested transition rows included.
pub fn scrub_chat_text_chip_keys(mem: &mut ChipMemory) -> bool {
    let before = mem.clone();
    let embeds = |k: &str| chip_key_embeds_chat_text(k) || k.starts_with("id:llm-");

    mem.hits.retain(|h| !embeds(&h.key));
    // Older clicks stored chat-built chip text; hashed Chat keys only ever
    // came from such chips.
    for h in mem.hits.iter_mut().filter(|h| {
        h.kind == ChipKind::Chat && h.key.strip_prefix("id:").is_none_or(chip_text_from_chat)
    }) {
        h.label.clear();
        h.value.clear();
    }
    // Older context tags carried raw draft text.
    for h in &mut mem.hits {
        let mut tags: Vec<String> = Vec::new();
        for tag in h.context_tags.iter().map(|t| tag_without_draft(t)) {
            if !tag.is_empty() && !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        h.context_tags = tags;
    }
    let keys: std::collections::HashSet<String> = mem.hits.iter().map(|h| h.key.clone()).collect();
    mem.transitions.retain(|from, dests| {
        if embeds(from) {
            return false;
        }
        dests.retain(|to, _| keys.contains(to) && !embeds(to));
        !dests.is_empty()
    });
    if mem.last_chip_key.as_deref().is_some_and(embeds) {
        mem.last_chip_key = None;
    }
    if mem.version < CHIP_MEMORY_VERSION {
        mem.version = CHIP_MEMORY_VERSION;
    }

    *mem != before
}

fn uniq_by_value(chips: Vec<QuickChip>) -> Vec<QuickChip> {
    let mut seen_value = std::collections::HashSet::new();
    let mut seen_label = std::collections::HashSet::new();
    let mut out = vec![];
    for c in chips {
        let value = c.value.to_ascii_lowercase();
        let label = c.label.to_ascii_lowercase();
        if seen_value.contains(&value) || seen_label.contains(&label) {
            continue;
        }
        seen_value.insert(value);
        seen_label.insert(label);
        out.push(c);
    }
    out
}

/// Composer chip for a cabin skill the typed ask already matches.
pub fn skill_offer_chip(draft: &str, skills: &[SkillMd]) -> Option<QuickChip> {
    let draft = draft.trim();
    if draft.is_empty() {
        return None;
    }
    let hit = match_skill(draft, skills)?;
    let raw = if hit.name.trim().is_empty() {
        "Saved skill".to_string()
    } else {
        hit.name.replace('-', " ")
    };
    let label = chip_label(&raw);
    let value = skill_use_in_chat_prompt(&hit.slash, &hit.name);
    Some(chip(
        &format!("skill-{}", hit.name),
        &label,
        &value,
        ChipKind::Chat,
        80.0,
        "Saved skill",
    ))
}

pub fn build_quick_chips(input: ChipInput<'_>) -> Vec<QuickChip> {
    let max = input.max.clamp(1, CHIP_HARD_MAX);
    let dismissed: std::collections::HashSet<String> = input
        .dismissed
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let pace = cabin_pace(&input);
    if pace == CabinPace::Guide {
        // A feature offer is about what the PC is doing now, so it shows
        // even while the first-run guide holds the row.
        let mut chips: Vec<QuickChip> = input.extra.to_vec();
        chips.extend(guide_chips(input.grok_connected));
        chips.retain(|c| {
            let id = c.id.to_ascii_lowercase();
            let val = c.value.trim().to_ascii_lowercase();
            !dismissed.contains(&id)
                && !dismissed.contains(&val)
                && !chip_dismissed_for_good(input.memory, c)
                && !family_quiet_this_hour(input.memory, &c.id, &c.value, &c.label, input.hour)
                && !(input.signals.anticheat && touches_the_game(c))
                && !(input.signals.gaming && waits_while_gaming(c))
        });
        chips.truncate(3);
        if let Some(first) = chips.first_mut() {
            first.primary = true;
        }
        return chips;
    }
    let stage = detect_chip_stage(input.chat, input.last_failed);
    let ctx = detect_chip_context(input.chat);
    let intents = predict_intents(input.chat, input.draft);
    let said = last_of(input.chat, "user");
    let heard = last_of(input.chat, "assistant");
    let mut chips = vec![];
    // A plain hand-off ("Take the next step") names no action, so it is not a chip.
    if let Some(learned) = crate::situation::learn_from_turns(&said, &heard)
        .filter(|l| l.kind != crate::situation::MoveKind::Next)
    {
        if !crate::situation::echoes_source(&learned.chip_value, &said)
            && !crate::situation::echoes_source(&learned.chip_label, &said)
        {
            chips.push(chip(
                &format!("learn-{}", learned.kind.as_str()),
                &learned.chip_label,
                &learned.chip_value,
                ChipKind::Chat,
                94.0,
                &learned.chip_hint,
            ));
        }
    }

    if !input.grok_connected {
        chips.push(chip(
            "ctx-connect",
            "Connect Grok",
            "__nav:settings",
            ChipKind::Nav,
            120.0,
            "You're not connected — this is the best next step",
        ));
    }
    // Feature offers come first so a plain chip with the same text yields.
    chips.extend(input.extra.iter().cloned());
    chips.extend(home_signal_chips(&input));
    chips.extend(signal_chips(&input));
    chips.extend(draft_prediction_chips(input.draft));
    chips.extend(
        learned_chips_from_memory(input.memory, input.now_ms)
            .into_iter()
            .map(|mut c| {
                if let Some(hit) = input.memory.hits.iter().find(|h| h.value == c.value) {
                    let hour_i = (input.hour as usize).min(23);
                    if hit.hour_hits.get(hour_i).copied().unwrap_or(0) > 0 {
                        c.score += (hit.hour_hits[hour_i] as f32 * 3.0).min(15.0);
                        c.hint = "Predicted for this time of day".into();
                    }
                }
                c.id = format!("pred-tod-{}", c.id);
                c
            }),
    );
    if let Some(last) = &input.memory.last_chip_key {
        if let Some(row) = input.memory.transitions.get(last) {
            let mut ranked: Vec<_> = row.iter().collect();
            ranked.sort_by(|a, b| b.1.cmp(a.1));
            for (key, n) in ranked.into_iter().take(4) {
                if *n < 1 {
                    continue;
                }
                if let Some(hit) = input.memory.hits.iter().find(|h| &h.key == key) {
                    if hit.dismisses >= 1 || hit.value.is_empty() {
                        continue;
                    }
                    chips.push(chip(
                        &format!("pred-tx-{}", key.chars().take(24).collect::<String>()),
                        &hit.label,
                        &hit.value,
                        hit.kind,
                        70.0 + (*n as f32 * 8.0).min(30.0),
                        &format!("Usually next after your last action (×{n})"),
                    ));
                }
            }
        }
    }
    if input.last_failed && recent_user(input.chat, 1).into_iter().next().is_some() {
        chips.push(chip(
            "pred-act-retry",
            "Try Again",
            "/retry",
            ChipKind::Chat,
            105.0,
            "Predicted — last attempt failed",
        ));
    }

    chips.extend(chips_from_last_assistant(input.chat));
    chips.extend(chips_from_other_threads(input.other_threads));
    if ctx.incomplete {
        chips.push(chip(
            "ctx-incomplete",
            "Finish the job",
            finish_job_prompt(input.chat),
            ChipKind::Chat,
            96.0,
            "Predicted incomplete turn",
        ));
    }
    if ctx.decide {
        chips.push(chip(
            "ctx-decide",
            "Recommend & do",
            "Recommend the best option for my setup and take the first concrete step.",
            ChipKind::Chat,
            89.0,
            "Decision context",
        ));
    }
    if ctx.implement {
        chips.push(chip(
            "ctx-implement",
            "Implement it",
            "Implement the requested change as a minimal solid slice. Inspect files if needed, then apply.",
            ChipKind::Chat,
            91.0,
            "Implement context",
        ));
    }
    chips.extend(stage_chips(stage));
    if input.first_run {
        chips.retain(|c| {
            let label = c.label.to_ascii_lowercase();
            !c.id.contains("night") && !label.contains("night job")
        });
    }
    if ctx.imagine {
        chips.push(chip(
            "ctx-imagine",
            "Open Imagine",
            "__nav:imagine",
            ChipKind::Nav,
            88.0,
            "Conversation mentioned images",
        ));
    }
    if !input.thread_title.trim().is_empty()
        && input.thread_title != "Chat"
        && input.thread_title != "Scratch"
    {
        chips.push(chip(
            "topic-title",
            &chip_label(&format!("Continue {}", input.thread_title)),
            &format!("Continue the work on {}.", input.thread_title),
            ChipKind::Chat,
            60.0,
            "Thread topic",
        ));
    }

    for c in input.reply_chips {
        if !is_plain_text(&c.value) || !is_plain_text(&c.label) {
            continue;
        }
        if crate::situation::echoes_source(&c.label, &said)
            || crate::situation::echoes_source(&c.value, &said)
        {
            continue;
        }
        let mut copy = c.clone();
        copy.score = copy.score.max(95.0);
        if copy.hint.is_empty() {
            copy.hint = "Suggested for this chat".into();
        }
        chips.push(copy);
    }

    chips.extend(seed_chips(stage));
    lead_with_reply_follow_ups(&mut chips, &input, &said, &heard);

    for c in &mut chips {
        c.score += memory_boost_for_chip(input.memory, c, input.now_ms, input.hour);
        c.score += intent_boost(input.memory, c, input.hour, input.signals.weekday);
        if let Some(hit) = input
            .memory
            .hits
            .iter()
            .find(|h| h.key == chip_memory_key(c))
        {
            if hit.context_tags.iter().any(|t| {
                t == &context_fingerprint(input.chat, "", input.last_failed, input.hour, input.mode)
            }) {
                c.score += 14.0;
            }
        }
    }

    chips.retain(|c| {
        let id = c.id.to_ascii_lowercase();
        let val = c.value.trim().to_ascii_lowercase();
        !dismissed.contains(&id)
            && !dismissed.contains(&val)
            && !chip_dismissed_for_good(input.memory, c)
            && !family_quiet_this_hour(input.memory, &c.id, &c.value, &c.label, input.hour)
            && is_plain_text(&c.value)
            && !is_desk_takeover_chip(c)
            && !retired_mode_value(&c.value)
            && !(input.signals.anticheat && touches_the_game(c))
            && !(input.signals.gaming && waits_while_gaming(c))
    });
    if input.grok_connected {
        chips.retain(|c| !is_stale_connect_chip(c));
    }
    let draft = input.draft.trim().to_ascii_lowercase();
    chips.retain(|c| c.value.trim().to_ascii_lowercase() != draft);

    apply_intent_boost(&mut chips, &intents);
    apply_home_signal_boost(&mut chips, &input);
    if !draft.is_empty() {
        apply_draft_boost(&mut chips, input.draft, &intents);
    }

    if stage != ChipStage::Empty && input.chat.len() >= 2 {
        for c in &mut chips {
            if c.kind == ChipKind::Nav {
                c.score -= 16.0;
            }
        }
    }

    chips = uniq_by_value(chips);
    chips.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if draft.len() >= 2 {
        let filtered: Vec<_> = chips
            .iter()
            .filter(|c| {
                let hay = format!("{} {} {}", c.label, c.value, c.hint).to_ascii_lowercase();
                c.score >= 85.0
                    || c.id.starts_with("pred-")
                    || hay.contains(&draft)
                    || draft
                        .split_whitespace()
                        .any(|t| t.len() > 2 && hay.contains(t))
                    || matches!(c.kind, ChipKind::Nav | ChipKind::Shell)
            })
            .cloned()
            .collect();
        if !filtered.is_empty() {
            chips = filtered;
        }
    }

    let mut picked = vec![];
    let mut kind_count: BTreeMap<u8, usize> = BTreeMap::new();
    let kind_id = |k: ChipKind| match k {
        ChipKind::Chat => 0,
        ChipKind::Shell => 1,
        ChipKind::Nav => 2,
    };
    for c in &chips {
        if picked.len() >= max {
            break;
        }
        let n = *kind_count.get(&kind_id(c.kind)).unwrap_or(&0);
        if c.kind == ChipKind::Shell && n >= 1 && max <= 5 {
            continue;
        }
        if c.kind == ChipKind::Nav && n >= 1 && max <= 5 && stage != ChipStage::Empty {
            continue;
        }
        picked.push(c.clone());
        *kind_count.entry(kind_id(c.kind)).or_insert(0) += 1;
    }
    if picked.len() < max {
        for c in &chips {
            if picked.len() >= max {
                break;
            }
            if picked
                .iter()
                .any(|p| p.id == c.id || p.value.eq_ignore_ascii_case(&c.value))
            {
                continue;
            }
            picked.push(c.clone());
        }
    }
    picked.truncate(max);
    if pace == CabinPace::Quiet && stage == ChipStage::Empty && input.draft.trim().is_empty() {
        picked.retain(|c| {
            let id = c.id.as_str();
            id.starts_with("home-")
                || id.starts_with(SEED_PREFIX)
                || id.starts_with("pred-tod")
                || id.starts_with("pred-tx")
                || id.starts_with("pred-habit")
                || id.starts_with("learn-")
                || id.starts_with("skill-")
                || c.value.starts_with('/')
                || input.extra.iter().any(|x| x.id == c.id)
        });
        picked.truncate(2);
    }
    if let Some(first) = picked.first_mut() {
        first.primary = true;
        if first.hint.is_empty() {
            first.hint = top_intent_label(&intents)
                .map(|s| format!("Predicted next: {s}"))
                .unwrap_or_else(|| "Highest-ranked next action".into());
        }
    }
    picked
}

/// Score that puts a reply's follow-ups at the front of the row, under only
/// "Connect Grok".
const FOLLOW_UP_SCORE: f32 = 112.0;

/// Right after a reply, its one-tap follow-ups ([`reply_suggestions`]) lead
/// the row. A chip already in the pool with the same label is lifted rather
/// than doubled. Typing a draft, or a reply that names no next move, leaves
/// the row as it was.
fn lead_with_reply_follow_ups(
    chips: &mut Vec<QuickChip>,
    input: &ChipInput<'_>,
    said: &str,
    heard: &str,
) {
    let replied = input
        .chat
        .last()
        .is_some_and(|(role, _)| role == "assistant");
    if !replied || !input.draft.trim().is_empty() {
        return;
    }
    for follow in reply_suggestions(said, heard, input.last_failed) {
        let hint = "Follow-up to that reply";
        match chips
            .iter_mut()
            .find(|c| c.label.eq_ignore_ascii_case(&follow.label))
        {
            Some(c) => {
                c.score = c.score.max(FOLLOW_UP_SCORE);
                c.hint = hint.into();
            }
            None => chips.push(QuickChip {
                score: FOLLOW_UP_SCORE,
                hint: hint.into(),
                ..follow
            }),
        }
    }
}

/// Up to three one-tap follow-ups for the reply that just finished, or none.
/// Only a failed turn or a move that names an action ("Fix the cause") earns
/// them, so most replies end with no suggestions at all.
pub fn reply_suggestions(said: &str, heard: &str, failed: bool) -> Vec<QuickChip> {
    let chat = [
        ("user".to_string(), said.to_string()),
        ("assistant".to_string(), heard.to_string()),
    ];
    let mut out = vec![];
    if let Some(learned) = crate::situation::learn_from_turns(said, heard)
        .filter(|l| l.kind != crate::situation::MoveKind::Next)
        .filter(|l| {
            !crate::situation::echoes_source(&l.chip_value, said)
                && !crate::situation::echoes_source(&l.chip_label, said)
        })
    {
        out.push(chip(
            &format!("learn-{}", learned.kind.as_str()),
            &learned.chip_label,
            &learned.chip_value,
            ChipKind::Chat,
            94.0,
            &learned.chip_hint,
        ));
    }
    if detect_chip_stage(&chat, failed) == ChipStage::Error {
        out.extend(stage_chips(ChipStage::Error));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|c| {
        c.kind == ChipKind::Chat
            && !c.value.starts_with('/')
            && !c.value.starts_with("__")
            && !generic_follow_up(&c.label)
            && seen.insert(c.label.to_ascii_lowercase())
    });
    out.truncate(3);
    out
}

/// A follow-up that names no action ("What's next", "Next step", "Anything else").
/// Replies lead with the result, so these are never shown as chips.
fn generic_follow_up(label: &str) -> bool {
    let words: String = label
        .to_ascii_lowercase()
        .replace('\u{2019}', "'")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '\'')
        .collect();
    matches!(
        words
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .as_str(),
        "what's next"
            | "whats next"
            | "what next"
            | "next step"
            | "next steps"
            | "the next step"
            | "take the next step"
            | "anything else"
            | "what now"
    )
}

pub(crate) fn cabin_chip_copy_ok(label: &str, value: &str, hint: &str) -> bool {
    if retired_host_copy("", label, value) || retired_host_copy("", hint, "") {
        return false;
    }
    if generic_follow_up(label) {
        return false;
    }
    let blob = format!("{label} {value} {hint}").to_ascii_lowercase();
    for w in [
        "video",
        "supergrok",
        "subscription",
        "outlook",
        "gmail",
        "google drive",
        "office",
        "stock",
    ] {
        if blob.contains(w) {
            return false;
        }
    }
    true
}

pub fn nav_from_chip_value(value: &str) -> Option<&'static str> {
    match value.trim() {
        "__nav:settings" => Some("settings"),
        "__nav:update" => Some("update"),
        "__nav:imagine" => Some("imagine"),
        "__nav:history" => Some("history"),
        "__nav:workboard" => Some("workboard"),
        "__nav:ideas" => Some("ideas"),
        "__nav:pulse" => Some("pulse"),
        "__nav:skills" => Some("skills"),
        "__nav:automations" | "__nav:night" => Some("night"),
        "__nav:command" => Some("command"),
        "__nav:queue" | "__nav:agents" => Some("agents"),
        "__nav:chat" | "__nav:agent" => Some("chat"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_chip_kind_gets_one_tap_rule() {
        let c = |id: &str, kind: ChipKind| chip(id, "Label here", "value", kind, 1.0, "");
        assert_eq!(chip_action(&c("chat-x", ChipKind::Chat)), ChipAction::Ask);
        assert_eq!(chip_action(&c("nav-x", ChipKind::Nav)), ChipAction::Go);
        // A shell line is never run on tap: it is shown first.
        assert_eq!(chip_action(&c("sh-x", ChipKind::Shell)), ChipAction::Review);
        let snooze = c(crate::backup_nudge::SNOOZE_ID, ChipKind::Chat);
        assert_eq!(chip_action(&snooze), ChipAction::Do);
        let remind = format!("{}42", crate::release_radar::REMIND_PREFIX);
        assert_eq!(chip_action(&c(&remind, ChipKind::Chat)), ChipAction::Do);
        let guides = format!("{}42", crate::game_companion::GUIDES_PREFIX);
        assert_eq!(chip_action(&c(&guides, ChipKind::Chat)), ChipAction::Go);
        let note = format!("{}42", crate::game_companion::NOTE_PREFIX);
        assert_eq!(chip_action(&c(&note, ChipKind::Chat)), ChipAction::Review);
        let tidy = c(crate::downloads_tidy::REVIEW_CHIP, ChipKind::Chat);
        assert_eq!(chip_action(&tidy), ChipAction::Review);
    }

    fn msg(role: &str, content: &str) -> (String, String) {
        (role.into(), content.into())
    }

    fn input<'a>(
        chat: &'a [(String, String)],
        draft: &'a str,
        memory: &'a ChipMemory,
        dismissed: &'a [String],
        llm: &'a [QuickChip],
    ) -> ChipInput<'a> {
        ChipInput {
            chat,
            draft,
            grok_connected: true,
            mode: "auto",
            thread_title: "Chat",
            usage_messages: 1,
            memory,
            dismissed,
            reply_chips: llm,
            last_failed: false,
            hour: 21,
            now_ms: 1_000_000,
            max: 5,
            other_threads: &[],
            first_run: false,
            last_slash: "",
            last_surface: "",
            last_project: "",
            skill_count: 0,
            session_mode: "chat",
            signals: ChipSignals::default(),
            extra: &[],
        }
    }

    fn ids(chips: &[QuickChip]) -> Vec<&str> {
        chips.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn seeds_fill_a_thin_row_and_rank_below_real_chips() {
        let memory = ChipMemory::default();
        // A returning user (the first-run tour owns a brand new chat).
        let familiar = |mut i: ChipInput<'_>| {
            i.usage_messages = 20;
            build_quick_chips(i)
        };
        let empty = familiar(input(&[], "", &memory, &[], &[]));
        assert!(
            empty.iter().any(|c| c.id.starts_with(SEED_PREFIX)),
            "a fresh chat is never dev-only or empty: {:?}",
            ids(&empty)
        );
        let chat = [
            msg("user", "the build fails with a linker error"),
            msg(
                "assistant",
                "Error: linker `cc` not found. Install build-essential.",
            ),
        ];
        let mid = familiar(input(&chat, "", &memory, &[], &[]));
        let first_seed = mid.iter().position(|c| c.id.starts_with(SEED_PREFIX));
        let last_real = mid.iter().rposition(|c| !c.id.starts_with(SEED_PREFIX));
        if let (Some(seed), Some(real)) = (first_seed, last_real) {
            assert!(
                seed > real,
                "seeds trail the chat's own chips: {:?}",
                ids(&mid)
            );
        }
    }

    #[test]
    fn a_dismissed_seed_stays_gone() {
        let memory = ChipMemory::default();
        let mut fresh = input(&[], "", &memory, &[], &[]);
        fresh.usage_messages = 20;
        let before = build_quick_chips(fresh);
        let seed = before
            .iter()
            .find(|c| c.id.starts_with(SEED_PREFIX))
            .expect("a seed")
            .clone();
        let dismissed = vec![seed.id.clone()];
        let mut again = input(&[], "", &memory, &dismissed, &[]);
        again.usage_messages = 20;
        let after = build_quick_chips(again);
        assert!(
            after.iter().all(|c| c.id != seed.id),
            "{} came back: {:?}",
            seed.id,
            ids(&after)
        );
    }

    #[test]
    fn seed_clicks_are_remembered_by_their_fixed_label() {
        let seed = seed_chips(ChipStage::Empty)
            .into_iter()
            .next()
            .expect("seed");
        assert_eq!(
            seed.value, seed.label,
            "seed copy is fixed text, never chat"
        );
        assert!(SEED_CHIPS.iter().any(|(_, l)| *l == "Find a duo"));
        assert!(SEED_CHIPS.iter().any(|(_, l)| *l == "Fix my stutter"));
    }

    /// Every chip any row offers, clicked and dismissed, twice over (the
    /// second round includes learned and predicted chips): no word from the
    /// chat, the draft, the title or a model chip reaches chips.json.
    #[test]
    fn clicking_and_dismissing_every_chip_stores_no_chat_text() {
        const MARK: &str = "zephyrquill";
        let chats = [
            vec![
                msg("user", "fix the zephyrquill parser"),
                msg(
                    "assistant",
                    "Error: zephyrquill panicked at src/main.rs\n```rust\nfn zephyrquill() {}\n```",
                ),
            ],
            vec![
                msg("user", "plan the zephyrquill launch"),
                msg(
                    "assistant",
                    "Here is a plan for zephyrquill. Want me to start?",
                ),
                msg("user", "draft the zephyrquill announcement"),
                msg("assistant", "Done. I'll check the zephyrquill page next."),
            ],
        ];
        let drafts = [
            "",
            "fix zephyrquill login",
            "add zephyrquill export",
            "zephyrquill",
        ];
        let llm = vec![chip(
            "llm-0-Zephyrquill",
            "Zephyrquill next",
            "Do the zephyrquill step next.",
            ChipKind::Chat,
            90.0,
            "Suggested from this chat",
        )];
        let mut mem = ChipMemory::default();
        let mut seen = 0;
        for round in 0..2 {
            for chat in &chats {
                for draft in drafts {
                    let snapshot = mem.clone();
                    let mut inp = input(chat, draft, &snapshot, &[], &llm);
                    inp.thread_title = "zephyrquill notes";
                    inp.max = 30;
                    let chips = build_quick_chips(inp);
                    // The tag the app passes on a real click.
                    let tag = context_fingerprint(chat, draft, false, 21, "auto");
                    for c in chips.iter().filter(|c| c.kind == ChipKind::Chat) {
                        seen += 1;
                        remember_chip_click(&mut mem, c, Some(&tag), 1_000 + round, 21);
                        if round == 1 {
                            remember_chip_dismiss(&mut mem, c, 2_000, 21);
                        }
                    }
                }
            }
        }
        assert!(seen > 20, "only {seen} chat chips offered");
        let json = serde_json::to_string(&mem).unwrap().to_ascii_lowercase();
        assert!(!json.contains(MARK), "{json}");
    }

    #[test]
    fn empty_stage_offers_cabin_chips() {
        let mem = ChipMemory::default();
        let chips = build_quick_chips(input(&[], "", &mem, &[], &[]));
        let labels: Vec<_> = chips.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels.contains(&"Start here"),
            "a new empty home teaches instead of touring the menus: {:?}",
            labels
        );
        assert!(
            labels
                .iter()
                .all(|l| *l != "Open Imagine" && !l.to_ascii_lowercase().contains("night")),
            "empty {:?}",
            labels
        );
        assert!(
            !labels.contains(&"Voice"),
            "Voice is the mic, not a duplicate chip: {:?}",
            labels
        );
        assert!(chips.len() <= CHIP_VISIBLE_MAX);
        assert!(chips[0].primary);
    }

    #[test]
    fn no_stage_offers_a_mode_chip() {
        let mem = ChipMemory::default();
        let chat = [
            msg("user", "hi"),
            msg(
                "assistant",
                "Hello — what should we work on in the cabin tonight?",
            ),
        ];
        // The router alone picks the model and effort: whatever the mode, no
        // chip offers to change it.
        for mode in ["auto", "think", "max", "fast"] {
            let mut i = input(&chat, "", &mem, &[], &[]);
            i.mode = mode;
            let chips = build_quick_chips(i);
            assert!(!chips.is_empty(), "{mode}");
            assert!(chips.len() <= CHIP_VISIBLE_MAX);
            assert!(
                chips.iter().all(|c| !c.value.starts_with("__mode:")),
                "{mode}: {:?}",
                labels(&chips)
            );
            for gone in ["Think Harder", "Go Max", "Use Adaptive"] {
                assert!(!chips.iter().any(|c| c.label == gone), "{mode}: {gone}");
            }
        }
        // A mode chip from an older model reply is dropped too.
        let old = chip(
            "llm-0-Think",
            "Think Harder",
            "__mode:think",
            ChipKind::Nav,
            99.0,
            "",
        );
        let chips = build_quick_chips(input(&chat, "", &mem, &[], std::slice::from_ref(&old)));
        assert!(!chips.iter().any(|c| c.value.starts_with("__mode:")));
    }

    #[test]
    fn duplicate_chip_labels_collapse() {
        let mem = ChipMemory::default();
        let llm = vec![
            chip(
                "llm-voice-a",
                "Voice",
                "Start a voice session with Hey Grok.",
                ChipKind::Chat,
                200.0,
                "Suggested",
            ),
            chip(
                "llm-voice-b",
                "Voice",
                "Open duplex voice on this cabin.",
                ChipKind::Chat,
                199.0,
                "Suggested",
            ),
        ];
        let chips = build_quick_chips(input(&[], "", &mem, &[], &llm));
        let voices = chips.iter().filter(|c| c.label == "Voice").count();
        assert_eq!(
            voices,
            0,
            "Voice is the mic, not a chip: {:?}",
            labels(&chips)
        );
    }

    #[test]
    fn disconnected_surfaces_connect_grok() {
        let mem = ChipMemory::default();
        let mut inp = input(&[], "", &mem, &[], &[]);
        inp.grok_connected = false;
        // A brand-new empty home is the guide, which has its own connect chip.
        inp.usage_messages = 12;
        let chips = build_quick_chips(inp);
        assert_eq!(chips[0].id, "ctx-connect");
        assert_eq!(chips[0].value, "__nav:settings");
        assert_eq!(nav_from_chip_value(&chips[0].value), Some("settings"));
    }

    #[test]
    fn connected_hides_stale_connect_grok_habit() {
        let mut mem = ChipMemory::default();
        let connect = QuickChip {
            id: "ctx-connect".into(),
            label: "Connect Grok".into(),
            value: "__nav:settings".into(),
            kind: ChipKind::Nav,
            score: 120.0,
            hint: String::new(),
            primary: false,
        };
        remember_chip_click(&mut mem, &connect, Some("empty+h2+m:auto"), 1, 16);
        remember_typed_prompt(&mut mem, "hi how are you", 2, 16);
        mem.last_chip_key = Some("id:ctx-connect".into());
        let llm = [QuickChip {
            id: "llm-0-ConnectGrok".into(),
            label: "Connect Grok".into(),
            value: "__nav:settings".into(),
            kind: ChipKind::Nav,
            score: 99.0,
            hint: "Suggested from this chat".into(),
            primary: true,
        }];
        let mut inp = input(&[], "", &mem, &[], &llm);
        inp.grok_connected = true;
        let chips = build_quick_chips(inp);
        assert!(
            chips
                .iter()
                .all(|c| !c.label.eq_ignore_ascii_case("Connect Grok") && c.id != "ctx-connect"),
            "OAuth session must not keep offering Connect Grok: {:?}",
            chips
                .iter()
                .map(|c| format!("{}:{}", c.id, c.label))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn error_stage_and_fix_intent() {
        let chat = [
            msg("user", "the build failed"),
            msg(
                "assistant",
                "TypeError: foo is not a function. The crash is in render.",
            ),
        ];
        assert_eq!(detect_chip_stage(&chat, false), ChipStage::Error);
        let intents = predict_intents(&chat, "");
        assert!(intents[&PredictedIntent::Fix] >= 0.5);
        let mem = ChipMemory::default();
        let chips = build_quick_chips(input(&chat, "", &mem, &[], &[]));
        assert!(chips.iter().any(|c| c.id.contains("diagnose")
            || c.label.contains("fix")
            || c.label.contains("Root")));
    }

    #[test]
    fn timeline_failed_tool_does_not_flip_chips_to_error() {
        use crate::turn_timeline::{append_say, append_tool, encode_turn};
        let mut blocks = Vec::new();
        append_tool(
            &mut blocks,
            "t1",
            "run_terminal_command",
            "failed",
            "exit 1",
        );
        append_say(&mut blocks, "The window is back.");
        let enc = encode_turn(&blocks);
        assert!(
            enc.contains("failed"),
            "precondition: the tool row records the failure"
        );
        assert!(enc.contains("TURN_TOOL"), "precondition: timeline marker");
        let chat = vec![msg("user", "check the window"), ("assistant".into(), enc)];
        assert_ne!(detect_chip_stage(&chat, false), ChipStage::Error);
    }

    #[test]
    fn draft_fix_predicts_debug_chip() {
        let chat = [
            msg("user", "auth is broken"),
            msg("assistant", "I can look at the oauth path next."),
        ];
        let mem = ChipMemory::default();
        let chips = build_quick_chips(input(&chat, "fix the", &mem, &[], &[]));
        assert!(
            chips.iter().any(|c| c.id.starts_with("pred-")
                && (c.label.contains("Debug") || c.label.contains("Fix"))),
            "{:?}",
            chips
                .iter()
                .map(|c| format!("{}:{}", c.id, c.label))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn dismiss_and_habit_rerank() {
        let mem0 = ChipMemory::default();
        let chat = [
            msg("user", "hi"),
            msg(
                "assistant",
                "Hello — what should we work on in the cabin tonight?",
            ),
        ];
        let base = build_quick_chips(input(&chat, "", &mem0, &[], &[]));
        let think = base
            .iter()
            .find(|c| c.id == "mid-continue")
            .cloned()
            .expect("continue chip");
        let mut mem = ChipMemory::default();
        remember_chip_dismiss(&mut mem, &think, 2_000, 21);
        let after = build_quick_chips(input(
            &chat,
            "",
            &mem,
            &[think.id.clone(), think.value.clone()],
            &[],
        ));
        assert!(!after.iter().any(|c| c.id == think.id));

        let habit = chip(
            "empty-brief",
            "Cabin brief",
            "Give me a short cabin brief: bound project, recent chats, and the next useful step.",
            ChipKind::Chat,
            64.0,
            "New chat",
        );
        remember_chip_click(&mut mem, &habit, Some("mid"), 3_000, 21);
        remember_chip_click(&mut mem, &habit, Some("mid"), 4_000, 21);
        remember_chip_click(&mut mem, &habit, Some("mid"), 5_000, 21);
        let ranked = build_quick_chips(input(&chat, "", &mem, &[], &[]));
        assert!(
            ranked
                .iter()
                .any(|c| c.label.contains("Cabin brief") || c.value.contains("cabin brief")),
            "{:?}",
            ranked.iter().map(|c| c.label.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn secrets_never_enter_memory_or_reply_chips() {
        let mut mem = ChipMemory::default();
        remember_typed_prompt(&mut mem, "token sk-abcdefghijklmnopqrstuv", 1, 3);
        assert!(mem.hits.is_empty());
        let parsed = crate::chip_trailer::parse_chip_trailer(
            "ask|use token sk-abcdefghijklmnopqrstuv now|Leak",
        );
        assert!(parsed.is_empty());
    }

    #[test]
    fn fingerprint_follows_the_context() {
        let empty: [(String, String); 0] = [];
        let err = [msg("user", "crash"), msg("assistant", "error: boom failed")];
        let a = context_fingerprint(&empty, "", false, 21, "auto");
        let b = context_fingerprint(&err, "fix", false, 21, "auto");
        assert_ne!(a, b);
        let think = context_fingerprint(&empty, "", false, 21, "think");
        assert_ne!(a, think, "mode change should refresh Fast chips");
    }

    #[test]
    fn llm_chips_merge_and_cap() {
        let mem = ChipMemory::default();
        let llm = vec![chip(
            "llm-0-Next",
            "Ship the patch",
            "Apply the auth patch now and verify with a focused test.",
            ChipKind::Chat,
            40.0,
            "Suggested from this chat",
        )];
        let chat = [
            msg("user", "implement oauth"),
            msg("assistant", "Here is a plan to wire the device code flow."),
        ];
        let chips = build_quick_chips(input(&chat, "", &mem, &[], &llm));
        assert!(chips.iter().any(|c| c.label == "Ship the patch"));
        assert!(chips.len() <= 5);
    }

    #[test]
    fn a_dismiss_at_this_hour_quiets_that_family() {
        let mut mem = ChipMemory::default();
        let draw = chip(
            "ctx-draw",
            "Draw a logo",
            "Draw a logo for the project.",
            ChipKind::Chat,
            10.0,
            "",
        );
        remember_chip_dismiss(&mut mem, &draw, 1_000, 21);
        let chat = [
            msg("user", "imagine a logo"),
            msg("assistant", "I can do that."),
        ];
        let mut night = input(&chat, "", &mem, &[], &[]);
        night.hour = 21;
        let hidden = build_quick_chips(night);
        assert!(
            hidden.iter().all(|c| c.id != "ctx-imagine"),
            "9pm dismiss hides imagine chips at 9pm: {:?}",
            labels(&hidden)
        );
        let mut morning = input(&chat, "", &mem, &[], &[]);
        morning.hour = 9;
        morning.usage_messages = 12;
        let shown = build_quick_chips(morning);
        assert!(
            shown.iter().any(|c| c.id == "ctx-imagine"),
            "the same dismiss does not block imagine chips in the morning: {:?}",
            labels(&shown)
        );
    }

    #[test]
    fn quiet_users_get_no_tour() {
        let mem = ChipMemory::default();
        let mut inp = input(&[], "", &mem, &[], &[]);
        inp.usage_messages = 80;
        inp.skill_count = 3;
        assert_eq!(cabin_pace(&inp), CabinPace::Quiet);
        let chips = build_quick_chips(inp);
        assert!(chips.len() <= 2, "{:?}", labels(&chips));
        assert!(
            chips
                .iter()
                .all(|c| !c.id.starts_with("guide-") && !c.id.starts_with("empty-")),
            "{:?}",
            labels(&chips)
        );
    }

    #[test]
    fn a_paid_review_waits_for_enough_new_turns() {
        assert!(!crate::learning::review_worth_tokens(3, 0));
        assert!(crate::learning::review_worth_tokens(8, 0));
        assert!(!crate::learning::review_worth_tokens(10, 8));
    }

    #[test]
    fn nav_values() {
        assert_eq!(nav_from_chip_value("__nav:imagine"), Some("imagine"));
    }

    #[test]
    fn reply_chips_drop_missing_products() {
        use crate::chip_trailer::parse_chip_trailer;
        let chips = parse_chip_trailer("ask|Generate a video of the cabin.|Make a video");
        assert!(chips.is_empty());
        let office = parse_chip_trailer("ask|Draft the weekly office memo.|Office mail");
        assert!(office.is_empty(), "{office:?}");
        let stock = parse_chip_trailer("ask|Check the stock ticker.|Market move");
        assert!(stock.is_empty(), "{stock:?}");
    }

    #[test]
    fn chips_do_not_advertise_missing_products() {
        let mem = ChipMemory::default();
        let mut inp = input(&[], "", &mem, &[], &[]);
        inp.usage_messages = 40;
        let chips = build_quick_chips(inp);
        assert!(!chips.iter().any(|c| c.id == "ctx-quota"));
        for c in &chips {
            let blob = format!("{} {} {}", c.label, c.value, c.hint).to_ascii_lowercase();
            assert!(!blob.contains("video"), "{}", c.id);
            assert!(!blob.contains("supergrok"), "{}", c.id);
            assert!(!blob.contains("subscription"), "{}", c.id);
        }
    }

    #[test]
    fn chat_text_never_becomes_a_chip_key() {
        let mut mem = ChipMemory::default();
        // Synthetic private-sounding line — never a real user quote in fixtures.
        let ask = "draft a private cabin story about the fireplace tonight";
        for i in 0..5u64 {
            remember_typed_prompt(&mut mem, ask, 10 + i * 10, 8);
        }
        assert!(
            mem.hits.is_empty(),
            "typed chat must not create habit hits: {:?}",
            mem.hits
        );
        assert!(learned_chips_from_memory(&mem, 100).is_empty());
        for h in &mem.hits {
            assert!(
                !h.key.contains("fireplace") && !h.key.contains("private"),
                "key leaked chat text: {}",
                h.key
            );
            assert!(
                !chip_key_embeds_chat_text(&h.key),
                "key must stay opaque: {}",
                h.key
            );
        }
        let learn = QuickChip {
            id: "learn-x".into(),
            label: "Cabin brief".into(),
            value: ask.into(),
            kind: ChipKind::Chat,
            score: 1.0,
            hint: String::new(),
            primary: false,
        };
        let key = chip_memory_key(&learn);
        assert!(
            key.starts_with("chat:v1:"),
            "learn keys must be opaque hashes: {key}"
        );
        assert!(
            !key.contains("fireplace") && !key.contains("private") && !key.contains(' '),
            "chip key must not embed the ask: {key}"
        );
        assert!(!chip_key_embeds_chat_text(&key));
    }

    #[test]
    fn scrub_removes_legacy_chat_text_chip_keys() {
        let mut mem = ChipMemory {
            version: 1,
            ..ChipMemory::default()
        };
        mem.hits.push(ChipHit {
            key: "chat:draft a private cabin story about the fireplace".into(),
            label: "draft a private cabin story about the fireplace".into(),
            value: "draft a private cabin story about the fireplace".into(),
            kind: ChipKind::Chat,
            uses: 3,
            typed_uses: 3,
            picks: 0,
            last_used_at: 1,
            hour_hits: vec![0; 24],
            successes: 0,
            failures: 0,
            context_tags: vec![],
            dismisses: 0,
            dismiss_hours: vec![0; 24],
            last_dismissed_at: 0,
        });
        mem.hits.push(ChipHit {
            key: "id:ctx-connect".into(),
            label: "Connect Grok".into(),
            value: "__nav:settings".into(),
            kind: ChipKind::Nav,
            uses: 2,
            typed_uses: 0,
            picks: 2,
            last_used_at: 2,
            hour_hits: vec![0; 24],
            successes: 1,
            failures: 0,
            context_tags: vec![],
            dismisses: 0,
            dismiss_hours: vec![0; 24],
            last_dismissed_at: 0,
        });
        let dirty = "chat:draft a private cabin story about the fireplace".to_string();
        mem.last_chip_key = Some(dirty.clone());
        mem.transitions
            .entry(dirty.clone())
            .or_default()
            .insert("id:ctx-connect".into(), 1);
        mem.transitions
            .entry("id:ctx-connect".into())
            .or_default()
            .insert(dirty, 1);

        assert!(scrub_chat_text_chip_keys(&mut mem));
        assert_eq!(mem.hits.len(), 1);
        assert_eq!(mem.hits[0].key, "id:ctx-connect");
        assert!(mem.last_chip_key.is_none());
        assert!(!mem.transitions.keys().any(|k| chip_key_embeds_chat_text(k)));
        assert!(mem
            .transitions
            .values()
            .all(|d| d.keys().all(|k| !chip_key_embeds_chat_text(k))));
        assert_eq!(mem.version, CHIP_MEMORY_VERSION);
        assert!(!scrub_chat_text_chip_keys(&mut mem), "idempotent");
    }

    /// A scrub that only empties a row's inner destinations, or only clears
    /// stored chip text, still reports a change so `load_chips` saves it.
    #[test]
    fn context_tags_keep_no_draft_text() {
        let fp = context_fingerprint(&[], "fix zephyrquill login", false, 21, "auto");
        assert_eq!(fp, "empty+d:a665f15e+h3+m:auto");
        assert_eq!(tag_without_draft(&fp), "empty+h3+m:auto");
        assert_eq!(
            tag_without_draft("code+d:fix zephyr+quill h3x+h3+m:chat"),
            "code+h3+m:chat"
        );

        let mut mem = ChipMemory {
            version: CHIP_MEMORY_VERSION,
            ..ChipMemory::default()
        };
        let brief = chip(
            "empty-brief",
            "Cabin brief",
            "Give me a short cabin brief.",
            ChipKind::Chat,
            10.0,
            "",
        );
        remember_chip_click(&mut mem, &brief, Some(&fp), 1_000, 21);
        assert_eq!(
            mem.hits[0].context_tags,
            vec!["empty+h3+m:auto".to_string()]
        );

        mem.hits[0].context_tags = vec![
            "code+d:fix zephyrquill+h3+m:chat".into(),
            "code+h3+m:chat".into(),
            "d:zephyrquill".into(),
        ];
        assert!(scrub_chat_text_chip_keys(&mut mem));
        assert_eq!(mem.hits[0].context_tags, vec!["code+h3+m:chat".to_string()]);
        assert!(!scrub_chat_text_chip_keys(&mut mem), "idempotent");
    }

    #[test]
    fn scrub_reports_inner_row_and_text_only_changes() {
        let hit = |key: &str, label: &str, value: &str, kind: ChipKind| ChipHit {
            key: key.into(),
            label: label.into(),
            value: value.into(),
            kind,
            uses: 1,
            typed_uses: 0,
            picks: 1,
            last_used_at: 1,
            hour_hits: vec![0; 24],
            successes: 0,
            failures: 0,
            context_tags: vec![],
            dismisses: 0,
            dismiss_hours: vec![0; 24],
            last_dismissed_at: 0,
        };
        let mut mem = ChipMemory {
            version: CHIP_MEMORY_VERSION,
            ..ChipMemory::default()
        };
        mem.hits = vec![
            hit(
                "id:ctx-connect",
                "Connect Grok",
                "__nav:settings",
                ChipKind::Nav,
            ),
            hit(
                "id:empty-brief",
                "Cabin brief",
                "Give me a short cabin brief.",
                ChipKind::Chat,
            ),
            hit(
                "id:ctx-draw",
                "Draw a logo",
                "Draw a logo for the project.",
                ChipKind::Chat,
            ),
        ];
        let row = mem.transitions.entry("id:ctx-connect".into()).or_default();
        row.insert("id:empty-brief".into(), 2);
        row.insert("chat:fix the harbor parser".into(), 1);
        assert!(scrub_chat_text_chip_keys(&mut mem), "inner row change");
        assert_eq!(
            mem.transitions["id:ctx-connect"].keys().collect::<Vec<_>>(),
            vec!["id:empty-brief"]
        );
        assert!(!scrub_chat_text_chip_keys(&mut mem), "idempotent");

        mem.hits.push(hit(
            "id:prev-0",
            "Fix the harbor",
            "fix the harbor parser",
            ChipKind::Chat,
        ));
        mem.hits.push(hit(
            "chat:v1:0123456789abcdef",
            "Harbor",
            "plan the harbor launch",
            ChipKind::Chat,
        ));
        mem.hits.push(hit(
            "id:llm-0-Harbornext",
            "Harbor next",
            "do the harbor step",
            ChipKind::Chat,
        ));
        assert!(scrub_chat_text_chip_keys(&mut mem), "text-only change");
        let rows: Vec<_> = mem
            .hits
            .iter()
            .map(|h| (h.key.as_str(), h.label.as_str(), h.value.as_str()))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("id:ctx-connect", "Connect Grok", "__nav:settings"),
                (
                    "id:empty-brief",
                    "Cabin brief",
                    "Give me a short cabin brief."
                ),
                ("id:ctx-draw", "Draw a logo", "Draw a logo for the project."),
                ("id:prev-0", "", ""),
                ("chat:v1:0123456789abcdef", "", ""),
            ]
        );
        assert!(!scrub_chat_text_chip_keys(&mut mem), "idempotent");
    }

    #[test]
    fn shell_typed_prompt_keeps_only_an_opaque_key() {
        let mut mem = ChipMemory::default();
        remember_typed_prompt(&mut mem, "$ uname -a && whoami", 10, 8);
        remember_typed_prompt(&mut mem, "$ uname -a && whoami", 20, 8);
        remember_typed_prompt(&mut mem, "/clear", 30, 8);
        assert_eq!(mem.hits.len(), 1);
        assert!(mem.hits[0].typed_uses >= 2);
        assert!(
            mem.hits[0].key.starts_with("shell:v1:"),
            "shell key must be opaque: {}",
            mem.hits[0].key
        );
        assert!(!chip_key_embeds_chat_text(&mem.hits[0].key));
        assert_eq!(mem.hits[0].label, "");
        assert_eq!(mem.hits[0].value, "");
        // Nothing to replay, so no habit chip comes back from it.
        assert!(learned_chips_from_memory(&mem, 30).is_empty());
    }

    fn feature_chip(id: &str, label: &str, value: &str, kind: ChipKind) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: label.into(),
            value: value.into(),
            kind,
            score: 1.0,
            hint: String::new(),
            primary: false,
        }
    }

    #[test]
    fn live_state_and_shell_chips_never_come_back_as_habits() {
        let mut mem = ChipMemory::default();
        let picks = [
            feature_chip(
                "model:move:tiny.gguf",
                "Move to models",
                "mv \"/home/user/Downloads/tiny.gguf\" \"/home/user/models\"",
                ChipKind::Shell,
            ),
            feature_chip(
                "backup:push:my-app",
                "Push my-app",
                "Push /home/user/code/my-app to its remote",
                ChipKind::Chat,
            ),
            feature_chip(
                "resume:continue:my-app",
                "Continue main",
                "Continue main",
                ChipKind::Chat,
            ),
            feature_chip("x-shell", "Status", "git status", ChipKind::Shell),
            feature_chip(
                "x-path",
                "Open notes",
                "Open C:\\notes\\todo.txt",
                ChipKind::Chat,
            ),
        ];
        for (i, c) in picks.iter().enumerate() {
            remember_chip_click(&mut mem, c, None, 10 + i as u64, 8);
            remember_chip_click(&mut mem, c, None, 20 + i as u64, 8);
        }
        assert_eq!(mem.hits.len(), 5);
        for h in &mem.hits {
            assert_eq!((h.label.as_str(), h.value.as_str()), ("", ""), "{}", h.key);
        }
        assert!(learned_chips_from_memory(&mem, 100).is_empty());

        // A plain catalog chat chip still keeps its text.
        let plain = feature_chip(
            "empty-brief",
            "Quick brief",
            "Give me a quick brief.",
            ChipKind::Chat,
        );
        remember_chip_click(&mut mem, &plain, None, 50, 8);
        let hit = mem.hits.iter().find(|h| h.key == "id:empty-brief").unwrap();
        assert_eq!(hit.value, "Give me a quick brief.");
    }

    #[test]
    fn load_scrub_clears_old_shell_and_path_habits() {
        let mut mem = ChipMemory::default();
        let mut push = |key: &str, label: &str, value: &str, kind: ChipKind| {
            mem.hits.push(ChipHit {
                key: key.into(),
                label: label.into(),
                value: value.into(),
                kind,
                uses: 3,
                typed_uses: 0,
                picks: 3,
                last_used_at: 1,
                hour_hits: Vec::new(),
                successes: 0,
                failures: 0,
                context_tags: Vec::new(),
                dismisses: 0,
                dismiss_hours: Vec::new(),
                last_dismissed_at: 0,
            });
        };
        push(
            "id:model:move:a.gguf",
            "Move a.gguf",
            "mv a.gguf models",
            ChipKind::Shell,
        );
        push(
            "shell:v1:0123456789abcdef",
            "uname",
            "$ uname -a",
            ChipKind::Shell,
        );
        push(
            "id:backup:push:my-app",
            "Push my-app",
            "Push /home/user/my-app",
            ChipKind::Chat,
        );
        push(
            "id:empty-brief",
            "Quick brief",
            "Give me a quick brief.",
            ChipKind::Chat,
        );
        assert!(prune_retired_chip_memory(&mut mem));
        let text: Vec<(&str, &str)> = mem
            .hits
            .iter()
            .map(|h| (h.label.as_str(), h.value.as_str()))
            .collect();
        assert_eq!(
            text,
            vec![
                ("", ""),
                ("", ""),
                ("", ""),
                ("Quick brief", "Give me a quick brief.")
            ]
        );
        assert_eq!(mem.hits.len(), 4, "counts stay for ranking");
        assert!(!prune_retired_chip_memory(&mut mem), "idempotent");
    }

    #[test]
    fn full_path_detection() {
        assert!(names_full_path("open /home/user/x"));
        assert!(names_full_path("~/notes.md"));
        assert!(names_full_path("\"C:\\Users\\user\""));
        assert!(!names_full_path("/learn"));
        assert!(!names_full_path("Make a checklist"));
        assert!(!names_full_path("and/or"));
    }

    fn labels(chips: &[QuickChip]) -> Vec<String> {
        chips.iter().map(|c| c.label.clone()).collect()
    }

    #[test]
    fn visible_chips_cap_five_and_skip_filler() {
        let mem = ChipMemory::default();
        let empty = build_quick_chips(input(&[], "", &mem, &[], &[]));
        assert!(!empty.is_empty(), "empty {:?}", labels(&empty));
        assert!(
            empty.len() <= CHIP_VISIBLE_MAX,
            "empty {:?}",
            labels(&empty)
        );
        assert!(empty[0].primary);
        assert_eq!(empty.iter().filter(|c| c.primary).count(), 1);

        let chat = [
            msg("user", "hi"),
            msg(
                "assistant",
                "Hello — what should we work on in the cabin tonight?",
            ),
        ];
        let mid = build_quick_chips(input(&chat, "", &mem, &[], &[]));
        assert!(!mid.is_empty(), "mid {:?}", labels(&mid));
        assert!(mid.len() <= CHIP_VISIBLE_MAX, "mid {:?}", labels(&mid));

        let mut dismissed: Vec<String> = empty
            .iter()
            .flat_map(|c| [c.id.clone(), c.value.clone()])
            .collect();
        let after = build_quick_chips(input(&[], "", &mem, &dismissed, &[]));
        assert!(
            after.len() <= CHIP_VISIBLE_MAX,
            "cap 5 after dismiss: {:?}",
            labels(&after)
        );
        let mut rest = after;
        let mut guard = 0;
        while !rest.is_empty() && guard < 8 {
            dismissed.extend(rest.iter().flat_map(|c| [c.id.clone(), c.value.clone()]));
            rest = build_quick_chips(input(&[], "", &mem, &dismissed, &[]));
            guard += 1;
        }
        assert!(
            rest.is_empty(),
            "no filler chips if ranking yields none: {:?}",
            labels(&rest)
        );
    }

    #[test]
    fn a_dismissed_chip_stays_gone_without_the_session_list() {
        let mut mem = ChipMemory::default();
        let first = build_quick_chips(input(&[], "", &mem, &[], &[]));
        let gone = first[0].clone();
        remember_chip_dismiss(&mut mem, &gone, 5, 8);
        let again = build_quick_chips(input(&[], "", &mem, &[], &[]));
        assert!(
            again
                .iter()
                .all(|c| c.id != gone.id && !c.value.eq_ignore_ascii_case(&gone.value)),
            "dismissed chip came back: {:?} still in {:?}",
            gone.label,
            labels(&again)
        );
        let hit = mem
            .hits
            .iter()
            .find(|h| h.key == chip_memory_key(&gone))
            .unwrap();
        assert_eq!(hit.dismiss_hours.get(8).copied().unwrap_or(0), 1);
        assert_eq!(
            hit.hour_hits.get(8).copied().unwrap_or(0),
            0,
            "a dismiss is not a use"
        );
        let talk = idea_talk_chips("Morning reminder", "weekday", &[]);
        assert!(talk
            .iter()
            .all(|c| c.hint == "This idea" || c.hint == "This conversation"));
        assert!(talk.iter().all(|c| !c.value.contains("__nav:")));
    }

    #[test]
    fn chips_drop_desk_takeover() {
        let mem = ChipMemory::default();
        let llm = [
            QuickChip {
                id: "llm-desk".into(),
                label: "Take over this desktop".into(),
                value: "Take over this desktop and drive it.".into(),
                kind: ChipKind::Chat,
                score: 200.0,
                hint: "Suggested".into(),
                primary: true,
            },
            QuickChip {
                id: "llm-scan".into(),
                label: "Scan the desk".into(),
                value: "Look at my desk and tell me what you see.".into(),
                kind: ChipKind::Chat,
                score: 199.0,
                hint: "Suggested".into(),
                primary: false,
            },
            QuickChip {
                id: "llm-host".into(),
                label: "Turn host on".into(),
                value: "/host on".into(),
                kind: ChipKind::Chat,
                score: 198.0,
                hint: "Suggested".into(),
                primary: false,
            },
            QuickChip {
                id: "llm-machine".into(),
                label: "Check the machine".into(),
                value: "Run a read-only HOST_CMD snapshot.".into(),
                kind: ChipKind::Chat,
                score: 197.0,
                hint: "Suggested".into(),
                primary: false,
            },
        ];
        let chips = build_quick_chips(input(&[], "", &mem, &[], &llm));
        assert!(
            chips
                .iter()
                .all(|c| !retired_host_copy(&c.id, &c.label, &c.value)),
            "desk/host chips must not sit on the composer: {:?}",
            labels(&chips)
        );
        assert!(
            chips.iter().all(|c| c.id != "ctx-host"),
            "Turn host on is gone: {:?}",
            labels(&chips)
        );
        let empty = build_quick_chips(input(&[], "", &mem, &[], &[]));
        assert!(
            empty
                .iter()
                .all(|c| !retired_host_copy(&c.id, &c.label, &c.value)),
            "empty home must not paint leftover desk chips: {:?}",
            labels(&empty)
        );
        assert!(crate::chip_trailer::parse_chip_trailer(
            "ask|Run HOST_CMD now.|Check the machine\nask|Look at my desk.|Scan the desk"
        )
        .is_empty());
    }

    #[test]
    fn chip_scan_caps_before_lowercase() {
        let src = include_str!("chips.rs");
        let last = src
            .split("fn last_of(")
            .nth(1)
            .and_then(|s| s.split("fn recent_user(").next())
            .expect("last_of");
        assert!(
            last.contains("chip_scan")
                || last.contains("TEXT_FILE_CAP")
                || last.contains("CHIP_SCAN"),
            "last_of must not clone an 8MB assistant for chips: {last}"
        );
    }

    #[test]
    fn a_chip_that_copies_the_user_is_dropped() {
        let mem = ChipMemory::default();
        let chat = [
            msg("user", "paint the wall tonight please"),
            msg("assistant", "I can start the first coat."),
        ];
        let llm = [QuickChip {
            id: "llm-echo".into(),
            label: "paint the wall tonight please".into(),
            value: "paint the wall tonight please and keep going".into(),
            kind: ChipKind::Chat,
            score: 200.0,
            hint: "Suggested".into(),
            primary: true,
        }];
        let chips = build_quick_chips(input(&chat, "", &mem, &[], &llm));
        let blob = chips
            .iter()
            .map(|c| format!("{} {}", c.label, c.value))
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();
        assert!(
            !blob.contains("paint the wall tonight"),
            "chips must not quote the ask: {blob}"
        );
        assert!(
            chips.iter().all(|c| c.id != "learn-next"),
            "a plain hand-off gets no generic next-step chip: {:?}",
            labels(&chips)
        );
    }

    #[test]
    fn reply_chips_keep_full_labels() {
        use crate::chip_trailer::parse_chip_trailer;
        let chips = parse_chip_trailer(
            "ask|Confirm the existing MCP connector is wired.|you should already have mcp configured",
        );
        assert_eq!(chips[0].label, "you should already have mcp configured");
        assert!(!chips[0].label.contains('…'));
        let clipped =
            parse_chip_trailer("ask|Look up Doosan in this chat.|check doosan for information on…");
        assert_eq!(clipped[0].label, "check doosan for information on");
        assert!(!clipped[0].label.contains('…'));
        let src = include_str!("chips.rs");
        let upsert = src
            .split("fn upsert_hit(")
            .nth(1)
            .and_then(|s| s.split("fn record_transition(").next())
            .expect("upsert_hit");
        assert!(
            !upsert.contains("take(48)") && upsert.contains("chip_label"),
            "habit labels must not mid-cut at 48: {upsert}"
        );
    }

    #[test]
    fn other_threads_seed_continue_chips() {
        let mem = ChipMemory::default();
        let others = [ChipThread {
            title: "Night cabin".into(),
            last_user: "paint the wall".into(),
            last_assistant: "I can sketch the first coat next.".into(),
        }];
        let mut inp = input(&[], "", &mem, &[], &[]);
        inp.usage_messages = 12;
        inp.other_threads = &others;
        let chips = build_quick_chips(inp);
        assert!(
            chips
                .iter()
                .any(|c| { c.label.contains("Night cabin") || c.value.contains("Night cabin") }),
            "expected a continue chip from the other chat, got {:?}",
            labels(&chips)
        );
    }

    #[test]
    fn previous_reply_followup_keeps_short_label() {
        let mem = ChipMemory::default();
        let others = [ChipThread {
            title: "Night cabin".into(),
            last_user: "paint the wall".into(),
            last_assistant: "First coat is ready. I'll run HOST_CMD diagnostics on the wall next."
                .into(),
        }];
        let mut inp = input(&[], "", &mem, &[], &[]);
        inp.usage_messages = 12;
        inp.other_threads = &others;
        let chips = build_quick_chips(inp);
        let prev_act = chips
            .iter()
            .find(|c| c.id.starts_with("prev-act"))
            .expect("previous-chat last-reply action");
        assert!(
            !prev_act.label.contains("Night cabin"),
            "follow-up label should stay the action, not the thread title: {}",
            prev_act.label
        );
        assert!(
            prev_act.hint.contains("Night cabin"),
            "hint should name the previous chat: {}",
            prev_act.hint
        );
        assert!(
            prev_act.value.contains("Night cabin"),
            "value should stay grounded in the previous chat: {}",
            prev_act.value
        );
    }

    #[test]
    fn scratch_and_empty_other_threads_are_skipped() {
        let mem = ChipMemory::default();
        let others = [
            ChipThread {
                title: "Scratch".into(),
                last_user: "ignore me".into(),
                last_assistant: String::new(),
            },
            ChipThread {
                title: "Chat".into(),
                last_user: "also ignore".into(),
                last_assistant: String::new(),
            },
        ];
        let mut inp = input(&[], "", &mem, &[], &[]);
        inp.other_threads = &others;
        let chips = build_quick_chips(inp);
        let blob = chips
            .iter()
            .map(|c| format!("{} {}", c.label, c.value))
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();
        assert!(!blob.contains("ignore me"), "{blob}");
        assert!(!blob.contains("also ignore"), "{blob}");
    }

    #[test]
    fn cabin_fast_model_is_pinned() {
        assert_eq!(crate::CABIN_FAST_MODEL, "grok-4.7");
    }

    #[test]
    fn gui_plan_finish_chip_asks_for_computer_cmd() {
        let mem = ChipMemory::default();
        let chat = [
            msg("user", "use my mouse and select a new tab in firefox"),
            msg(
                "assistant",
                "I'll locate Firefox on the desktop and run a click on the new tab.",
            ),
        ];
        let chips = build_quick_chips(input(&chat, "", &mem, &[], &[]));
        let finish = chips.iter().find(|c| {
            c.id == "last-run-host" || c.id == "ctx-incomplete" || c.label.contains("Finish")
        });
        let finish = finish.unwrap_or_else(|| panic!("finish chip: {:?}", labels(&chips)));
        assert!(
            finish.value.contains("computer-use") || finish.value.contains("desktop"),
            "GUI finish chip must ask Grok to drive the desktop: {}",
            finish.value
        );
    }

    #[test]
    fn previous_reply_outcome_boosts_the_habit() {
        let mut mem = ChipMemory::default();
        let habit = chip(
            "empty-brief",
            "Cabin brief",
            "Give me a short cabin brief: bound project, recent chats, and the next useful step.",
            ChipKind::Chat,
            64.0,
            "New chat",
        );
        remember_chip_click(&mut mem, &habit, Some("mid"), 3_000, 21);
        let key = mem.last_chip_key.clone().unwrap();
        remember_chip_turn_outcome(&mut mem, &key, true, 4_000);
        remember_chip_turn_outcome(&mut mem, &key, true, 5_000);
        let chat = [
            msg("user", "hi"),
            msg(
                "assistant",
                "Hello — what should we work on in the cabin tonight?",
            ),
        ];
        let ranked = build_quick_chips(input(&chat, "", &mem, &[], &[]));
        assert!(
            ranked
                .iter()
                .any(|c| c.label.contains("Cabin brief") || c.value.contains("cabin brief")),
            "{:?}",
            labels(&ranked)
        );
        prune_retired_chip_memory(&mut mem);
        let desk = chip(
            "empty-host",
            "Check the machine",
            "Run a quick read-only system snapshot via HOST_CMD (uname, whoami, pwd). Summarize.",
            ChipKind::Chat,
            64.0,
            "Desktop host",
        );
        remember_chip_click(&mut mem, &desk, Some("empty"), 6_000, 21);
        assert!(prune_retired_chip_memory(&mut mem));
        let cleaned = build_quick_chips(input(&[], "", &mem, &[], &[]));
        assert!(
            cleaned
                .iter()
                .all(|c| !c.label.contains("machine")
                    && !c.label.to_ascii_lowercase().contains("desk")),
            "{:?}",
            labels(&cleaned)
        );
    }

    #[test]
    fn home_slash_and_surface_rank_next_chips() {
        let mut mem = ChipMemory::default();
        remember_home_slash(&mut mem, "plan", 10);
        remember_home_surface(&mut mem, "imagine", 11);
        assert_eq!(mem.last_slash.as_deref(), Some("/plan"));
        assert_eq!(mem.last_surface.as_deref(), Some("imagine"));
        remember_home_slash(&mut mem, "/update", 12);
        assert_eq!(
            mem.last_slash.as_deref(),
            Some("/plan"),
            "junk slashes stay out"
        );

        let mut inp = input(&[], "", &mem, &[], &[]);
        inp.last_slash = "/plan";
        inp.last_surface = "imagine";
        let chips = build_quick_chips(inp);
        let shown = labels(&chips);
        assert!(
            chips
                .iter()
                .any(|c| c.value == "/plan" && c.label == "/plan"),
            "last slash /plan must surface: {shown:?}"
        );
        assert!(
            chips.iter().any(|c| c.label == "Open Imagine"),
            "last Imagine surface must keep Imagine: {shown:?}"
        );
        assert!(
            chips.iter().find(|c| c.value == "/plan").map(|c| c.score)
                > chips
                    .iter()
                    .find(|c| c.label == "Cabin brief")
                    .map(|c| c.score),
            "slash outranks generic empty-home: {shown:?}"
        );
        for c in &chips {
            assert!(
                c.label.chars().count() <= 28,
                "ADHD labels stay short: {}",
                c.label
            );
        }
        assert!(chips.len() <= CHIP_VISIBLE_MAX);
    }

    #[test]
    fn last_project_and_first_run_rank() {
        let mem = ChipMemory::default();
        let mut returning = input(&[], "", &mem, &[], &[]);
        returning.last_project = "Night cabin";
        let chips = build_quick_chips(returning);
        assert!(
            chips
                .iter()
                .any(|c| c.id == "home-continue" && c.label.contains("Night cabin")),
            "{:?}",
            labels(&chips)
        );
        assert_eq!(chips[0].id, "home-continue", "{:?}", labels(&chips));

        let mut first = input(&[], "", &mem, &[], &[]);
        first.first_run = true;
        let chips = build_quick_chips(first);
        let shown = labels(&chips);
        assert!(shown.iter().any(|l| *l == "Start here"), "{shown:?}");
        assert!(
            shown
                .iter()
                .all(|l| *l != "Open Imagine" && !l.to_ascii_lowercase().contains("night")),
            "first-run teaches, it does not open the tool menu: {shown:?}"
        );

        let mut skills = input(&[], "", &mem, &[], &[]);
        skills.skill_count = 3;
        skills.last_surface = "skills";
        let chips = build_quick_chips(skills);
        assert!(
            chips.iter().any(|c| c.label == "Skills"),
            "skills you have must rank: {:?}",
            labels(&chips)
        );

        let mut plan = input(&[], "", &mem, &[], &[]);
        plan.session_mode = "plan";
        let chips = build_quick_chips(plan);
        assert!(
            chips
                .iter()
                .any(|c| c.label == "/plan" && c.value == "/plan"),
            "last session Plan offers /plan: {:?}",
            labels(&chips)
        );
    }

    #[test]
    fn home_slash_cmd_keeps_cabin_verbs() {
        assert_eq!(home_slash_cmd("plan"), Some("/plan"));
        assert_eq!(home_slash_cmd("reflect"), Some("/learn"));
        assert_eq!(home_slash_cmd("/learn"), Some("/learn"));
        assert_eq!(home_slash_cmd("imagine"), Some("/imagine"));
        assert_eq!(home_slash_cmd("/update"), None);
        assert_eq!(home_slash_cmd("new"), None);
        assert_eq!(home_slash_cmd("clear"), None);
    }

    #[test]
    fn home_surface_from_nav_covers_sidebar_pages() {
        assert_eq!(home_surface_from_nav("imagine"), Some("imagine"));
        assert_eq!(home_surface_from_nav("skills"), Some("skills"));
        assert_eq!(home_surface_from_nav("chat"), None);
        assert_eq!(home_surface_from_nav("settings"), None);
        let mut mem = ChipMemory::default();
        remember_home_surface(&mut mem, home_surface_from_nav("imagine").unwrap(), 9);
        assert_eq!(mem.last_surface.as_deref(), Some("imagine"));
    }

    #[test]
    fn saved_skill_is_a_chip_when_the_ask_matches() {
        let flash = SkillMd {
            name: "flash-pi".into(),
            description: "write an image".into(),
            slash: "/flash".into(),
            trigger: "flash the pi".into(),
            instructions: "dd".into(),
            pitfalls: "boot disk".into(),
            verify: "lsblk".into(),
            runs: 0,
        };
        let chip = skill_offer_chip("flash the pi", std::slice::from_ref(&flash)).expect("match");
        assert_eq!(chip.value, "/flash");
        assert_eq!(chip.kind, ChipKind::Chat);
        assert!(chip.label.contains("flash"));
        assert!(skill_offer_chip("", std::slice::from_ref(&flash)).is_none());
        assert!(skill_offer_chip("what is rust", std::slice::from_ref(&flash)).is_none());
        let patched = crate::patch_skill(
            &flash,
            &SkillMd {
                name: "other".into(),
                description: String::new(),
                slash: "/other".into(),
                trigger: "flash the pi again".into(),
                instructions: "dd the newer image".into(),
                pitfalls: String::new(),
                verify: String::new(),
                runs: 1,
            },
        );
        assert_eq!(patched.name, "flash-pi");
        assert!(patched.instructions.contains("newer"));
    }

    #[test]
    fn no_generic_next_chip_in_any_stage() {
        let mem = ChipMemory::default();
        let tools = [
            msg("user", "list the repo"),
            msg("assistant", "HOST_RESULT: 12 files"),
        ];
        let mid = [msg("user", "hi"), msg("assistant", "Hello there.")];
        let failed = [
            msg("user", "build it"),
            msg("assistant", "The build hit an error."),
        ];
        let chats: [&[(String, String)]; 4] = [&[], &tools, &mid, &failed];
        for chat in chats {
            for (first_run, session) in [(false, "chat"), (true, "ask"), (false, "ask")] {
                let mut i = input(chat, "", &mem, &[], &[]);
                i.first_run = first_run;
                i.session_mode = session;
                let chips = build_quick_chips(i);
                assert!(!chips.is_empty(), "stage still offers chips");
                for c in &chips {
                    assert!(
                        !generic_follow_up(&c.label),
                        "generic chip {:?} for chat {:?} first_run={first_run} session={session}",
                        c.label,
                        chat
                    );
                    assert!(!c.id.ends_with("-next"), "retired chip id {}", c.id);
                }
            }
        }
    }

    #[test]
    fn reply_chips_drop_generic_follow_ups_and_may_be_empty() {
        use crate::chip_trailer::parse_chip_trailer;
        let raw = "ask|What should we do next?|What's next\n\
                   ask|Take the next step.|Next step\n\
                   ask|Add unit tests for parse_row in rows.rs.|Add tests for parse_row";
        let labels: Vec<_> = parse_chip_trailer(raw)
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert_eq!(labels, vec!["Add tests for parse_row".to_string()]);
        assert_eq!(parse_chip_trailer(""), Vec::<QuickChip>::new());
        assert_eq!(
            parse_chip_trailer("ask|What should we do next?|What’s next"),
            Vec::<QuickChip>::new()
        );
    }

    #[test]
    fn reply_suggestions_only_follow_a_failure_or_a_named_move() {
        let labels = |chips: Vec<QuickChip>| chips.into_iter().map(|c| c.label).collect::<Vec<_>>();
        assert_eq!(
            labels(reply_suggestions(
                "list the files in src",
                "There are 12 files.",
                false
            )),
            Vec::<String>::new(),
            "a plain hand-off gets no card"
        );
        assert_eq!(
            labels(reply_suggestions("hi", "Hello.", false)),
            Vec::<String>::new()
        );
        assert_eq!(
            labels(reply_suggestions(
                "the build failed after the last change",
                "TypeError: foo is not a function.",
                false
            )),
            vec!["Fix the cause".to_string(), "Root cause + fix".to_string()]
        );
        let failed = reply_suggestions("run the tests", "Done.", true);
        assert_eq!(labels(failed.clone()), vec!["Root cause + fix".to_string()]);
        assert!(failed.iter().all(|c| c.kind == ChipKind::Chat));
        assert!(
            failed.iter().all(|c| !c.value.starts_with('/')),
            "no /retry in the shell"
        );
    }

    #[test]
    fn generic_follow_up_matches_only_empty_labels() {
        for label in [
            "What's next",
            "whats next?",
            "Next step",
            "Take the next step",
            "Anything else?",
            "What now?",
        ] {
            assert!(generic_follow_up(label), "{label}");
        }
        for label in [
            "Continue GrokHub",
            "Fix the next-step test",
            "Add tests",
            "Try Again",
        ] {
            assert!(!generic_follow_up(label), "{label}");
        }
    }

    #[test]
    fn a_failed_turn_leads_the_row_with_its_follow_up() {
        let mem = ChipMemory::default();
        let chat = [msg("user", "run the tests"), msg("assistant", "Done.")];
        let mut i = input(&chat, "", &mem, &[], &[]);
        i.usage_messages = 20;
        i.last_failed = true;
        let chips = build_quick_chips(i);
        assert_eq!(chips[0].label, "Root cause + fix", "{:?}", ids(&chips));
        assert_eq!(chips[0].hint, "Follow-up to that reply");
        assert_eq!(
            chips
                .iter()
                .filter(|c| c.label == "Root cause + fix")
                .count(),
            1,
            "lifted, not doubled"
        );
    }

    #[test]
    fn a_named_move_leads_and_a_plain_reply_or_a_draft_gets_none() {
        let mem = ChipMemory::default();
        let broke = [
            msg("user", "the build failed after the last change"),
            msg("assistant", "TypeError: foo is not a function."),
        ];
        let mut i = input(&broke, "", &mem, &[], &[]);
        i.usage_messages = 20;
        let chips = build_quick_chips(i.clone());
        assert_eq!(chips[0].label, "Fix the cause", "{:?}", ids(&chips));
        i.draft = "also check";
        let typing = build_quick_chips(i);
        assert!(typing.iter().all(|c| c.hint != "Follow-up to that reply"));
        let plain = [msg("user", "hi"), msg("assistant", "Hello.")];
        let mut i = input(&plain, "", &mem, &[], &[]);
        i.usage_messages = 20;
        let chips = build_quick_chips(i);
        assert!(chips.iter().all(|c| c.hint != "Follow-up to that reply"));
    }

    fn with_signals(signals: ChipSignals<'_>, hour: u8) -> Vec<QuickChip> {
        let memory = ChipMemory::default();
        let mut i = input(&[], "", &memory, &[], &[]);
        i.usage_messages = 20;
        i.hour = hour;
        i.max = CHIP_HARD_MAX;
        i.signals = signals;
        build_quick_chips(i)
    }

    fn find<'c>(chips: &'c [QuickChip], id: &str) -> Option<&'c QuickChip> {
        chips.iter().find(|c| c.id == id)
    }

    #[test]
    fn a_running_game_offers_a_stutter_fix_named_for_it() {
        let chips = with_signals(
            ChipSignals {
                game: Some("Cyberpunk 2077"),
                weekday: 2,
                ..Default::default()
            },
            14,
        );
        let c = find(&chips, "sig-game-stutter").expect("stutter chip");
        assert_eq!(c.label, "Fix stutter in Cyberpunk 2077");
        assert!(c.value.contains("Cyberpunk 2077"));
        assert!(c.value.contains("Don't touch the game"));
        assert!(find(&chips, "sig-game-ended").is_none());
        let off = with_signals(ChipSignals::default(), 14);
        assert!(
            off.iter().all(|c| !c.id.starts_with("sig-")),
            "{:?}",
            ids(&off)
        );
    }

    #[test]
    fn a_long_game_name_is_cut_in_the_label_but_whole_in_the_ask() {
        let name = "The Elder Scrolls V: Skyrim Special Edition";
        let chips = with_signals(
            ChipSignals {
                game: Some(name),
                ..Default::default()
            },
            14,
        );
        let c = find(&chips, "sig-game-stutter").expect("stutter chip");
        assert_eq!(c.label, "Fix stutter in The Elder Scrolls V");
        assert!(c.value.contains(name));
    }

    #[test]
    fn a_closed_game_asks_how_it_went() {
        let chips = with_signals(
            ChipSignals {
                game_ended: Some("Hades"),
                ..Default::default()
            },
            14,
        );
        let c = find(&chips, "sig-game-ended").expect("ended chip");
        assert_eq!(c.label, "How'd Hades go?");
    }

    #[test]
    fn a_full_drive_offers_to_free_space_without_deleting_first() {
        let chips = with_signals(
            ChipSignals {
                low_disk: true,
                ..Default::default()
            },
            14,
        );
        let c = find(&chips, "sig-low-disk").expect("disk chip");
        assert_eq!(c.label, "Free up storage");
        assert!(c.value.contains("before deleting anything"));
    }

    #[test]
    fn disk_is_low_under_ten_percent_or_five_gib() {
        const GIB: u64 = 1024 * 1024 * 1024;
        assert!(disk_is_low(4 * GIB, 100 * GIB));
        assert!(disk_is_low(90 * GIB, 1000 * GIB));
        assert!(!disk_is_low(20 * GIB, 100 * GIB));
        assert!(!disk_is_low(150 * GIB, 1000 * GIB));
        assert!(!disk_is_low(0, 0), "an unknown drive is not low");
    }

    #[test]
    fn a_waiting_update_opens_the_update_page() {
        let chips = with_signals(
            ChipSignals {
                update_waiting: true,
                ..Default::default()
            },
            14,
        );
        let c = find(&chips, "sig-update").expect("update chip");
        assert_eq!(c.kind, ChipKind::Nav);
        assert_eq!(nav_from_chip_value(&c.value), Some("update"));
    }

    #[test]
    fn friday_evening_offers_weekend_plans_and_quiet_hours_drop_them() {
        let friday = |hour, quiet_hours| {
            with_signals(
                ChipSignals {
                    weekday: 5,
                    quiet_hours,
                    ..Default::default()
                },
                hour,
            )
        };
        assert!(find(&friday(19, false), "sig-weekend").is_some());
        assert!(
            find(&friday(12, false), "sig-weekend").is_none(),
            "Friday lunch"
        );
        assert!(
            find(&friday(19, true), "sig-weekend").is_none(),
            "quiet hours"
        );
        let thursday = with_signals(
            ChipSignals {
                weekday: 4,
                ..Default::default()
            },
            19,
        );
        assert!(find(&thursday, "sig-weekend").is_none());
    }

    #[test]
    fn signal_chips_sit_under_the_chats_own_once_it_has_turns() {
        let memory = ChipMemory::default();
        let chat = vec![msg("user", "fix the build"), msg("assistant", "Done.")];
        let base = |chat: &[(String, String)]| {
            let mut i = input(chat, "", &memory, &[], &[]);
            i.signals = ChipSignals {
                low_disk: true,
                ..Default::default()
            };
            signal_chips(&i)[0].score
        };
        assert!(base(&chat) < base(&[]));
    }

    #[test]
    fn intents_come_from_the_catalog_and_whole_words() {
        assert_eq!(chip_intent("Fix stutter in Hades", ""), Some("game.perf"));
        assert_eq!(
            chip_intent("Fix my stutter", "Fix my stutter"),
            Some("game.perf")
        );
        assert_eq!(chip_intent("Free up storage", ""), Some("rig.storage"));
        assert_eq!(chip_intent("Find a duo tonight", ""), Some("game.squad"));
        assert_eq!(chip_intent("Plan my weekend", ""), Some("day.plan"));
        assert_eq!(chip_intent("", "the build is failing"), Some("code.fix"));
        assert_eq!(
            chip_intent("", "add a prefix to the flag while typing"),
            None
        );
        assert_eq!(chip_intent("hi", "how are you"), None);
    }

    fn stutter_chips(mem: &ChipMemory, hour: u8) -> f32 {
        let mut i = input(&[], "", mem, &[], &[]);
        i.usage_messages = 20;
        i.hour = hour;
        i.max = CHIP_HARD_MAX;
        i.signals = ChipSignals {
            game: Some("Hades"),
            weekday: 3,
            ..Default::default()
        };
        build_quick_chips(i)
            .into_iter()
            .find(|c| c.id == "sig-game-stutter")
            .map(|c| c.score)
            .expect("stutter chip")
    }

    #[test]
    fn a_game_chip_used_at_nine_three_nights_lifts_at_nine_only() {
        let mut mem = ChipMemory::default();
        let fresh_9pm = stutter_chips(&mem, 21);
        // Three nights at 9pm, through the composer chip or by typing.
        remember_intent(&mut mem, "Fix stutter in Hades", "", 1_000, 21, 1);
        remember_intent(&mut mem, "", "my game stutters again", 2_000, 21, 2);
        remember_intent(&mut mem, "Fix my stutter", "Fix my stutter", 3_000, 21, 3);
        assert!(stutter_chips(&mem, 21) > fresh_9pm + 10.0);
        assert!(
            stutter_chips(&mem, 14) < stutter_chips(&mem, 21),
            "a 9pm habit does little at 2pm"
        );
        // Only catalog keys reach chips.json, never the typed words.
        let json = serde_json::to_string(&mem).unwrap();
        assert!(json.contains("game.perf"));
        assert!(!json.contains("stutters again") && !json.contains("Hades"));
    }

    #[test]
    fn a_turn_credits_only_the_chip_that_started_it() {
        let mut mem = ChipMemory::default();
        let fix = chip("ctx-fix", "Fix it", "Fix it", ChipKind::Chat, 50.0, "");
        remember_chip_click(&mut mem, &fix, None, 1_000, 10);
        let key = chip_memory_key(&fix);
        remember_chip_turn_outcome(&mut mem, &key, true, 2_000);
        remember_chip_turn_outcome(&mut mem, "chat:v1:0000000000000000", false, 3_000);
        let hit = mem.hits.iter().find(|h| h.key == key).unwrap();
        assert_eq!((hit.successes, hit.failures), (1, 0));
    }

    fn offer(id: &str, label: &str, score: f32) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: label.into(),
            value: label.into(),
            kind: ChipKind::Chat,
            score,
            hint: "Feature offer".into(),
            primary: false,
        }
    }

    fn with_pace<'a>(chat: &'a [(String, String)], memory: &'a ChipMemory) -> ChipInput<'a> {
        let mut i = input(chat, "", memory, &[], &[]);
        // Familiar but not advanced: the ordinary paced row.
        i.usage_messages = 25;
        i.max = CHIP_HARD_MAX;
        i
    }

    #[test]
    fn feature_offers_rank_with_the_rest_instead_of_jumping_the_row() {
        let memory = ChipMemory::default();
        let chat = vec![msg("user", "fix the build"), msg("assistant", "Done.")];
        let extra = [offer("backup:due", "Back up now", 100.0)];
        let mut i = with_pace(&chat, &memory);
        i.extra = &extra;
        let row = build_quick_chips(i);
        let at = row
            .iter()
            .position(|c| c.id == "backup:due")
            .expect("offer in pool");
        assert!(
            row[..at].iter().all(|c| c.score >= row[at].score),
            "only stronger chips lead it: {:?}",
            ids(&row)
        );
        // A hidden offer stays hidden like any chip.
        let gone = ["backup:due".to_string()];
        let mut i = with_pace(&chat, &memory);
        i.extra = &extra;
        i.dismissed = &gone;
        assert!(build_quick_chips(i).iter().all(|c| c.id != "backup:due"));
    }

    #[test]
    fn gaming_holds_nudges_but_keeps_game_and_chat_chips() {
        let memory = ChipMemory::default();
        let chat = vec![
            msg("user", "how do I fix this crash"),
            msg("assistant", "Try verifying the files."),
        ];
        let extra = [
            offer("tidy:review", "Review Downloads cleanup", 100.0),
            offer("game:guides:1", "Top guides for Hades", 100.0),
        ];
        let mut i = with_pace(&chat, &memory);
        i.extra = &extra;
        i.signals = ChipSignals {
            low_disk: true,
            gaming: true,
            ..Default::default()
        };
        let row = build_quick_chips(i);
        let got = ids(&row);
        assert!(!got.contains(&"tidy:review"), "{got:?}");
        assert!(!got.contains(&"sig-low-disk"), "{got:?}");
        assert!(got.contains(&"game:guides:1"), "{got:?}");
        assert!(
            !row.is_empty() && row.iter().any(|c| !c.id.contains(':')),
            "the chat's own chips stay: {got:?}"
        );
    }

    #[test]
    fn an_anticheat_hides_chips_that_touch_the_game() {
        let memory = ChipMemory::default();
        let chat = vec![
            msg("user", "my fps dropped"),
            msg("assistant", "Close the overlay."),
        ];
        let mut shell = offer("pred-shell", "Show processes", 100.0);
        shell.kind = ChipKind::Shell;
        shell.value = "ps aux".into();
        let extra = [
            offer("game:guides:1", "Top guides for Hades", 100.0),
            offer("focus:game:hades", "Help with Hades", 100.0),
            shell,
        ];
        let mut i = with_pace(&chat, &memory);
        i.extra = &extra;
        i.signals = ChipSignals {
            game: Some("Hades"),
            anticheat: true,
            gaming: true,
            ..Default::default()
        };
        let row = build_quick_chips(i);
        assert!(row.iter().all(|c| !touches_the_game(c)), "{:?}", ids(&row));
    }

    #[test]
    fn a_file_in_the_tray_offers_an_explain_chip_that_is_never_kept() {
        let memory = ChipMemory::default();
        let chat = vec![msg("user", "hi"), msg("assistant", "Hey!")];
        let mut i = with_pace(&chat, &memory);
        i.signals = ChipSignals {
            attachment: Some("report.pdf"),
            gaming: true,
            ..Default::default()
        };
        let row = build_quick_chips(i);
        let chip = row
            .iter()
            .find(|c| c.id == "sig-attach-explain")
            .expect("explain chip, even while gaming");
        assert_eq!(chip.label, "Explain report.pdf");
        let mut mem = ChipMemory::default();
        remember_chip_click(&mut mem, chip, None, 1_000, 9);
        assert!(
            mem.hits
                .iter()
                .all(|h| !h.label.contains("report") && !h.value.contains("report")),
            "the file name never becomes a habit"
        );
    }

    #[test]
    fn the_first_run_guide_still_shows_a_feature_offer() {
        let memory = ChipMemory::default();
        let extra = [offer("focus:coding:cursor", "Next coding step", 110.0)];
        let mut i = input(&[], "", &memory, &[], &[]);
        i.first_run = true;
        i.extra = &extra;
        assert_eq!(cabin_pace(&i), CabinPace::Guide);
        assert!(ids(&build_quick_chips(i)).contains(&"focus:coding:cursor"));
    }
}
