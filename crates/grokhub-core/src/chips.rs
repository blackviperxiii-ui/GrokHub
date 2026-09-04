//! Predictive quick chips — stage, draft, habits, fast-mode parse.
//! Secrets never persist in chip memory.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::is_plain_text;
use crate::recipe::user_asks_gui_help;

pub const CHIP_VISIBLE_MAX: usize = 5;
pub const CHIP_HARD_MAX: usize = 8;
pub const CHIP_LLM_DEBOUNCE_MS: u64 = 1200;
pub const CHIP_LLM_MODE: &str = "fast";
const CHIP_SCAN_CAP: usize = 4096;
const MAX_HITS: usize = 80;
const MAX_TRANSITIONS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChipKind {
    Chat,
    Shell,
    Nav,
    Mode,
}

impl ChipKind {
    fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "shell" => Self::Shell,
            "nav" => Self::Nav,
            "mode" => Self::Mode,
            _ => Self::Chat,
        }
    }
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
    pub total_events: u32,
    #[serde(default)]
    pub updated_at: u64,
}

fn chip_memory_version() -> u32 {
    1
}

impl Default for ChipMemory {
    fn default() -> Self {
        Self {
            version: 1,
            hits: vec![],
            transitions: BTreeMap::new(),
            last_chip_key: None,
            total_events: 0,
            updated_at: 0,
        }
    }
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
    pub host_on: bool,
    pub mode: &'a str,
    pub thread_title: &'a str,
    pub usage_messages: u32,
    pub usage_cap: u32,
    pub memory: &'a ChipMemory,
    pub dismissed: &'a [String],
    pub llm_chips: &'a [QuickChip],
    pub last_failed: bool,
    pub hour: u8,
    pub now_ms: u64,
    pub max: usize,
    pub other_threads: &'a [ChipThread],
}

pub fn empty_chip_memory() -> ChipMemory {
    ChipMemory::default()
}

pub fn chip_memory_key(chip: &QuickChip) -> String {
    if chip.id.starts_with("learn-") || chip.id.starts_with("recent-") || chip.id.starts_with("pred-habit")
    {
        value_key(&chip.value, chip.kind)
    } else {
        format!("id:{}", chip.id)
    }
}

fn value_key(value: &str, kind: ChipKind) -> String {
    let v = value.trim().to_ascii_lowercase();
    let v: String = v.split_whitespace().collect::<Vec<_>>().join(" ");
    let kind = match kind {
        ChipKind::Chat => "chat",
        ChipKind::Shell => "shell",
        ChipKind::Nav => "nav",
        ChipKind::Mode => "mode",
    };
    format!("{kind}:{}", v.chars().take(160).collect::<String>())
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
        .map(|(_, c)| chip_scan(c).to_string())
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
        imagine: regexish(&blob, &["imagine", "generate a pic", "generate an image", "draw "]),
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
        ui: regexish(&blob, &["ui", "layout", "button", "sidebar", "theme", "dark mode", "chip"]),
        decide: regexish(&blob, &["should i", "which", "options", "tradeoff", "recommend", "compare"]),
        implement: regexish(&users.to_ascii_lowercase(), &["implement", "add ", "build ", "create ", "wire ", "ship ", "patch"]),
        incomplete: regexish(&asst.to_ascii_lowercase(), &["i'll", "i will", "let me", "next i", "continuing", "still need", "want me to", "shall i"])
            || (!asst.is_empty()
                && asst.len() < 600
                && regexish(&asst.to_ascii_lowercase(), &["check", "probe", "investigate"])
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
        || regexish(&low, &["error", "fail", "broken", "crash", "couldn't complete", "not a function"])
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
    let draft_bit: String = draft.trim().chars().take(24).collect();
    if !draft_bit.is_empty() {
        bits.push(format!("d:{draft_bit}"));
    }
    bits.push(format!("h{}", hour / 6));
    let mode_bit = mode.trim().to_ascii_lowercase();
    if !mode_bit.is_empty() {
        bits.push(format!("m:{mode_bit}"));
    }
    bits.join("+")
}

pub fn should_refresh_llm(
    prev_fp: &str,
    next_fp: &str,
    last_llm_at: u64,
    now_ms: u64,
    has_auth: bool,
    busy: bool,
) -> bool {
    has_auth && !busy && next_fp != prev_fp && now_ms.saturating_sub(last_llm_at) >= CHIP_LLM_DEBOUNCE_MS
}

pub fn predict_intents(chat: &[(String, String)], draft: &str) -> BTreeMap<PredictedIntent, f32> {
    let mut scores = BTreeMap::new();
    for i in PredictedIntent::all() {
        scores.insert(i, if i == PredictedIntent::Chat { 0.15 } else { 0.0 });
    }
    let asst = last_of(chat, "assistant");
    let user = last_of(chat, "user");
    let draft_l = draft.to_ascii_lowercase();
    let blob = format!("{user}\n{asst}\n{draft_l}").to_ascii_lowercase();

    if regexish(&asst.to_ascii_lowercase(), &["let me", "i'll", "i will", "looking into", "running checks", "continuing"])
        || (regexish(&asst.to_ascii_lowercase(), &["check", "probe", "investigate"])
            && !asst.to_ascii_lowercase().contains("host_cmd"))
    {
        *scores.get_mut(&PredictedIntent::Finish).unwrap() += 0.55;
        *scores.get_mut(&PredictedIntent::Host).unwrap() += 0.25;
    }
    if regexish(&blob, &["error", "fail", "crash", "broken", "exception"]) {
        *scores.get_mut(&PredictedIntent::Fix).unwrap() += 0.5;
    }
    if asst.contains("```") || user.contains("```") || regexish(&blob, &["function", "const ", "import ", "typescript", "refactor"])
    {
        *scores.get_mut(&PredictedIntent::Review).unwrap() += 0.4;
        *scores.get_mut(&PredictedIntent::Ship).unwrap() += 0.2;
    }
    if regexish(&format!("{user} {draft_l}"), &["add ", "implement", "build ", "create ", "make ", "wire ", "patch", "fix "])
    {
        *scores.get_mut(&PredictedIntent::Ship).unwrap() += 0.45;
    }
    if regexish(&format!("{user} {draft_l}"), &["explain", "how does", "what is", "why ", "walk me"]) {
        *scores.get_mut(&PredictedIntent::Explain).unwrap() += 0.45;
    }
    if regexish(&blob, &["host_", "desktop host", "$ ", "journalctl", "process", "system"]) {
        *scores.get_mut(&PredictedIntent::Host).unwrap() += 0.4;
    }
    if regexish(&blob, &["should i", "which", "options", "recommend", "tradeoff"]) {
        *scores.get_mut(&PredictedIntent::Decide).unwrap() += 0.4;
    }
    if regexish(&blob, &["imagine", "image", "draw", "logo", "generate an image"]) {
        *scores.get_mut(&PredictedIntent::Create).unwrap() += 0.5;
    }
    if regexish(&format!("{user} {draft_l}"), &["continue", "keep going", "go on", "resume", "next"]) {
        *scores.get_mut(&PredictedIntent::Continue).unwrap() += 0.5;
        *scores.get_mut(&PredictedIntent::Finish).unwrap() += 0.2;
    }
    if draft_l.len() >= 2 {
        if draft_l.starts_with("fix") || draft_l.starts_with("bug") || draft_l.starts_with("error") || draft_l.starts_with("crash")
        {
            *scores.get_mut(&PredictedIntent::Fix).unwrap() += 0.35;
        }
        if ["add", "implement", "build", "make", "create"]
            .iter()
            .any(|p| draft_l.starts_with(p))
        {
            *scores.get_mut(&PredictedIntent::Ship).unwrap() += 0.35;
        }
        if ["explain", "how", "why", "what"].iter().any(|p| draft_l.starts_with(p)) {
            *scores.get_mut(&PredictedIntent::Explain).unwrap() += 0.35;
        }
        if ["check", "scan", "look", "ps ", "find ", "ls "].iter().any(|p| draft_l.starts_with(p)) {
            *scores.get_mut(&PredictedIntent::Host).unwrap() += 0.35;
        }
        if ["continue", "keep", "finish"].iter().any(|p| draft_l.starts_with(p)) {
            *scores.get_mut(&PredictedIntent::Finish).unwrap() += 0.4;
        }
        if ["imagine", "draw", "image"].iter().any(|p| draft_l.starts_with(p)) {
            *scores.get_mut(&PredictedIntent::Create).unwrap() += 0.4;
        }
        if draft_l.starts_with('$') || draft_l.starts_with("/sh") {
            *scores.get_mut(&PredictedIntent::Host).unwrap() += 0.5;
        }
    }
    scores
}

pub fn top_intent_label(intents: &BTreeMap<PredictedIntent, f32>) -> Option<&'static str> {
    let (intent, score) = intents.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))?;
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

fn chip(
    id: &str,
    label: &str,
    value: &str,
    kind: ChipKind,
    score: f32,
    hint: &str,
) -> QuickChip {
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

fn shorten(s: &str, n: usize) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= n {
        return t;
    }
    let mut out = String::new();
    for (i, ch) in t.chars().enumerate() {
        if i + 1 >= n {
            break;
        }
        out.push(ch);
    }
    format!("{}…", out.trim_end())
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
        "Don't just plan — drive the desktop now with Grok Build computer-use. Summarize when done."
    } else {
        "Don't just plan — actually investigate my machine now with Grok Build tools. Summarize when done."
    }
}

fn finish_job_prompt(chat: &[(String, String)]) -> &'static str {
    if last_user_wants_gui(chat) {
        "Finish the incomplete work from your last reply. Act now (computer-use if needed). End with status."
    } else {
        "Finish the incomplete work from your last reply. Act now with Grok Build tools if needed. End with status."
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
        &["error", "fail", "exception", "couldn't complete", "not a function"],
    );
    let has_host = regexish(&asst, &["HOST_CMD", "HOST_RESULT", "desktop host"]);
    let is_plan = regexish(&asst.to_ascii_lowercase(), &["i'll", "i will", "let me", "would you like me to"])
        && regexish(&asst.to_ascii_lowercase(), &["check", "probe", "investigate", "run"]);
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
            &shorten(&format!("Find bugs{topic_bit}"), 34),
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
            if is_plan { "Finish — run tools now" } else { "Run diagnostics now" },
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
        let label = shorten(&format!("Continue {}", t.title.trim()), 34);
        let value = if !last_user.is_empty() {
            format!(
                "Continue the work from the chat \"{}\". Last ask: {}. Pick up where we left off and act now.",
                t.title.trim(),
                shorten(last_user, 160)
            )
        } else {
            format!(
                "Continue the work from the chat \"{}\". Pick up where we left off and act now.",
                t.title.trim()
            )
        };
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
                    "In the previous chat \"{}\": {}. {}",
                    t.title.trim(),
                    shorten(if last_user.is_empty() { last_asst } else { last_user }, 80),
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
        ("fix", "Debug with evidence", |d| {
            (format!("{d}. Investigate with Grok Build tools for real evidence. Root cause, fix, verify."), ChipKind::Chat)
        }, 125.0),
        ("debug", "Debug with evidence", |d| {
            (format!("{d}. Investigate with Grok Build tools for real evidence. Root cause, fix, verify."), ChipKind::Chat)
        }, 125.0),
        ("add", "Implement this now", |d| {
            (format!("{d}. Ship a minimal solid slice. Inspect files if needed, then apply."), ChipKind::Chat)
        }, 124.0),
        ("implement", "Implement this now", |d| {
            (format!("{d}. Ship a minimal solid slice."), ChipKind::Chat)
        }, 124.0),
        ("build", "Implement this now", |d| {
            (format!("{d}. Ship a minimal solid slice."), ChipKind::Chat)
        }, 124.0),
        ("explain", "Explain simply", |d| {
            (format!("{d}. Plain language, one example, one practical takeaway."), ChipKind::Chat)
        }, 120.0),
        ("how", "Explain simply", |d| {
            (format!("{d}. Plain language, one example, one practical takeaway."), ChipKind::Chat)
        }, 120.0),
        ("check", "Check it", |d| {
            (format!("{d}. Inspect the tree and summarize results clearly."), ChipKind::Chat)
        }, 123.0),
        ("continue", "Continue until done", |d| {
            (format!("{d}. Complete the goal fully. Act now — no planning-only replies."), ChipKind::Chat)
        }, 126.0),
        ("finish", "Continue until done", |d| {
            (format!("{d}. Complete the goal fully."), ChipKind::Chat)
        }, 126.0),
        ("imagine", "Open Imagine for this", |_| ("__nav:imagine".into(), ChipKind::Nav), 115.0),
        ("draw", "Open Imagine for this", |_| ("__nav:imagine".into(), ChipKind::Nav), 115.0),
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
        ("fix the", "Fix the bug we hit", "Diagnose and fix the latest bug. Root cause, exact fix, verify steps."),
        ("fix o", "Fix OAuth / session", "Fix Grok OAuth or session issues. Check tokens, refresh, and reconnect path."),
        ("add ", "Add a feature", "Implement the feature I'm describing as a minimal solid slice."),
        ("check c", "Check CPU / processes", "Read top CPU and memory processes and summarize what's hot."),
        ("how d", "How does this work?", "Explain how this works simply with one example."),
        ("contin", "Continue the task", "Continue until the current goal is fully done. Use tools when needed."),
        ("summar", "Summarize the thread", "Summarize this thread: goals, decisions, open questions, next steps."),
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
            &format!("{draft}. Be concrete and complete the goal. Use Grok Build tools if you need machine data."),
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
        h.label = label.chars().take(48).collect();
        h.value = value.chars().take(400).collect();
        h.kind = kind;
        h.uses = h.uses.saturating_add(uses_delta);
        h.typed_uses = h.typed_uses.saturating_add(typed_delta);
        h.last_used_at = now_ms;
        h.hour_hits = bump_hour(&h.hour_hits, hour);
        h.dismisses = h.dismisses.saturating_add(dismiss_delta);
        if let Some(tag) = context_tag {
            if !h.context_tags.iter().any(|t| t == tag) {
                h.context_tags.insert(0, tag.to_string());
                h.context_tags.truncate(12);
            }
        }
    } else {
        memory.hits.insert(
            0,
            ChipHit {
                key,
                label: label.chars().take(48).collect(),
                value: value.chars().take(400).collect(),
                kind,
                uses: uses_delta,
                typed_uses: typed_delta,
                last_used_at: now_ms,
                hour_hits: bump_hour(&[], hour),
                successes: 0,
                failures: 0,
                context_tags: context_tag.map(|t| vec![t.to_string()]).unwrap_or_default(),
                dismisses: dismiss_delta,
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

pub fn remember_chip_click(
    memory: &mut ChipMemory,
    chip: &QuickChip,
    context_tag: Option<&str>,
    now_ms: u64,
    hour: u8,
) {
    let key = chip_memory_key(chip);
    let from = memory.last_chip_key.clone();
    upsert_hit(
        memory,
        key.clone(),
        &chip.label,
        &chip.value,
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

pub fn remember_chip_dismiss(memory: &mut ChipMemory, chip: &QuickChip, now_ms: u64, hour: u8) {
    let key = chip_memory_key(chip);
    upsert_hit(
        memory,
        key,
        &chip.label,
        &chip.value,
        chip.kind,
        0,
        0,
        1,
        None,
        now_ms,
        hour,
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
    if kind == ChipKind::Chat && raw.len() > 120 {
        boost_matching(memory, raw, now_ms);
        return;
    }
    let key = value_key(raw, kind);
    let label = shorten(raw, if kind == ChipKind::Shell { 28 } else { 32 });
    let from = memory.last_chip_key.clone();
    upsert_hit(memory, key.clone(), &label, raw, kind, 1, 1, 0, None, now_ms, hour);
    record_transition(memory, from, key);
    boost_matching(memory, raw, now_ms);
}

fn boost_matching(memory: &mut ChipMemory, text: &str, now_ms: u64) {
    let lower = text.to_ascii_lowercase();
    let tokens: Vec<_> = lower.split_whitespace().filter(|t| t.len() > 3).take(8).collect();
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

pub fn remember_chip_outcome(memory: &mut ChipMemory, success: bool, now_ms: u64) {
    let Some(key) = memory.last_chip_key.clone() else {
        return;
    };
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
    let hit = memory
        .hits
        .iter()
        .find(|h| h.key == key)
        .or_else(|| {
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

fn learned_chips_from_memory(memory: &ChipMemory, now_ms: u64) -> Vec<QuickChip> {
    let mut out: Vec<QuickChip> = memory
        .hits
        .iter()
        .filter(|h| h.uses >= 1 && !h.value.trim().is_empty())
        .filter(|h| matches!(h.kind, ChipKind::Chat | ChipKind::Shell))
        .filter(|h| h.dismisses < 2)
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
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(8);
    out
}

pub fn top_habit_labels(memory: &ChipMemory, n: usize) -> Vec<String> {
    let mut hits = memory.hits.clone();
    hits.retain(|h| h.uses >= 1 && h.dismisses < 2);
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
                PredictedIntent::Finish => regexish(&hay, &["finish", "complete", "continue", "host_cmd", "act now"]),
                PredictedIntent::Fix => regexish(&hay, &["fix", "diagnos", "error", "root cause", "bug"]),
                PredictedIntent::Ship => regexish(&hay, &["implement", "ship", "apply", "patch", "add ", "build"]),
                PredictedIntent::Explain => regexish(&hay, &["explain", "simple", "plain", "takeaway"]),
                PredictedIntent::Host => regexish(&hay, &["host", "system", "process", "shell", "machine", "scan"]),
                PredictedIntent::Decide => regexish(&hay, &["recommend", "option", "pick", "best", "decide"]),
                PredictedIntent::Create => regexish(&hay, &["imagine", "image", "draw", "logo"]),
                PredictedIntent::Continue => regexish(&hay, &["continue", "keep", "resume", "next step"]),
                PredictedIntent::Review => regexish(&hay, &["review", "bugs", "improve", "refactor", "test"]),
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
                    c.hint = format!("Predicted · {}{}", m.as_str(), if c.hint.is_empty() { String::new() } else { format!(" · {}", c.hint) });
                }
            }
        }
    }
}

fn apply_draft_boost(chips: &mut [QuickChip], draft_raw: &str, intents: &BTreeMap<PredictedIntent, f32>) {
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
        if regexish(&draft, &["imagine", "draw", "image"]) && regexish(&hay, &["imagine", "image", "__nav:imagine"]) {
            boost += 45.0;
        }
        if regexish(&draft, &["bug", "error", "fix", "crash"]) && regexish(&hay, &["fix", "diagnos", "error", "bug"]) {
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

fn routing_chip(id: &str, mode: &str, score: f32) -> QuickChip {
    let mode = mode.trim();
    if matches!(mode, "max" | "deep" | "heavy") {
        chip(
            id,
            "Use Adaptive",
            "__mode:auto",
            ChipKind::Mode,
            score,
            "Auto routes Fast / Balance / Think / Max",
        )
    } else if matches!(mode, "think" | "thinking" | "expert" | "build") {
        chip(
            id,
            "Go Max",
            "__mode:max",
            ChipKind::Mode,
            score,
            "Max · Grok 4.6 xhigh",
        )
    } else {
        chip(
            id,
            "Think Harder",
            "__mode:think",
            ChipKind::Mode,
            score,
            "Think · Grok 4.6 high",
        )
    }
}

fn stage_chips(stage: ChipStage, mode: &str) -> Vec<QuickChip> {
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
                "Cabin brief",
                "Give me a short cabin brief: bound project, recent chats, and the next useful step.",
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
            chip(
                "empty-next",
                "What's next",
                "What should we do next in this cabin? One concrete step.",
                ChipKind::Chat,
                62.0,
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
            chip(
                "tools-next",
                "Next step",
                "Given the last tool result, take the next safe step with Grok Build tools.",
                ChipKind::Chat,
                84.0,
                "Continue tools",
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
            routing_chip("long-think", mode, 72.0),
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
            routing_chip("mid-think", mode, 70.0),
        ],
    }
}

fn default_chips(mode: &str) -> Vec<QuickChip> {
    vec![
        chip(
            "def-help",
            "What can you help with?",
            "What can you help me with in GrokHub right now? Keep it to a short capability list.",
            ChipKind::Chat,
            24.0,
            "Default",
        ),
        routing_chip("def-route", mode, 24.0),
        chip(
            "def-imagine",
            "Open Imagine",
            "__nav:imagine",
            ChipKind::Nav,
            22.0,
            "Images",
        ),
        chip(
            "def-brief",
            "Cabin brief",
            "Give me a short cabin brief: bound project, recent chats, and the next useful step.",
            ChipKind::Chat,
            21.0,
            "Default",
        ),
        chip(
            "def-night",
            "Set a night job",
            "Help me set a night automation. Ask one question, then propose a real schedule I can save.",
            ChipKind::Chat,
            20.0,
            "Default",
        ),
        chip(
            "def-board",
            "Open workboard",
            "__nav:workboard",
            ChipKind::Nav,
            19.0,
            "Default",
        ),
        chip(
            "def-history",
            "Recent chats",
            "__nav:history",
            ChipKind::Nav,
            18.0,
            "Default",
        ),
        chip(
            "def-github",
            "GitHub whoami",
            "If a GitHub PAT is set, run CONNECTOR_CMD github user and summarize who I am. If not, tell me how to add one in Settings.",
            ChipKind::Chat,
            17.0,
            "Default",
        ),
        chip(
            "def-skills",
            "Skills",
            "__nav:skills",
            ChipKind::Nav,
            16.0,
            "Default",
        ),
    ]
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

pub fn build_quick_chips(input: ChipInput<'_>) -> Vec<QuickChip> {
    let max = input.max.clamp(1, CHIP_HARD_MAX);
    let dismissed: std::collections::HashSet<String> = input
        .dismissed
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let stage = detect_chip_stage(input.chat, input.last_failed);
    let ctx = detect_chip_context(input.chat);
    let intents = predict_intents(input.chat, input.draft);
    let mut chips = vec![];

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
    chips.extend(draft_prediction_chips(input.draft));
    chips.extend(learned_chips_from_memory(input.memory, input.now_ms).into_iter().map(|mut c| {
        if let Some(hit) = input.memory.hits.iter().find(|h| h.value == c.value) {
            let hour_i = (input.hour as usize).min(23);
            if hit.hour_hits.get(hour_i).copied().unwrap_or(0) > 0 {
                c.score += (hit.hour_hits[hour_i] as f32 * 3.0).min(15.0);
                c.hint = "Predicted for this time of day".into();
            }
        }
        c.id = format!("pred-tod-{}", c.id);
        c
    }));
    if let Some(last) = &input.memory.last_chip_key {
        if let Some(row) = input.memory.transitions.get(last) {
            let mut ranked: Vec<_> = row.iter().collect();
            ranked.sort_by(|a, b| b.1.cmp(a.1));
            for (key, n) in ranked.into_iter().take(4) {
                if *n < 1 {
                    continue;
                }
                if let Some(hit) = input.memory.hits.iter().find(|h| &h.key == key) {
                    if hit.dismisses >= 2 || hit.value.is_empty() {
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
    chips.extend(stage_chips(stage, input.mode));
    chips.extend(default_chips(input.mode));
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
    if !input.thread_title.trim().is_empty() && input.thread_title != "Chat" && input.thread_title != "Scratch" {
        chips.push(chip(
            "topic-title",
            &shorten(&format!("Continue {}", input.thread_title), 34),
            &format!("Continue the work on {}.", input.thread_title),
            ChipKind::Chat,
            60.0,
            "Thread topic",
        ));
    }
    chips.extend(default_chips(input.mode));

    for c in input.llm_chips {
        if !is_plain_text(&c.value) || !is_plain_text(&c.label) {
            continue;
        }
        let mut copy = c.clone();
        copy.score = copy.score.max(95.0);
        if copy.hint.is_empty() {
            copy.hint = "Suggested for this chat".into();
        }
        chips.push(copy);
    }

    for c in &mut chips {
        c.score += memory_boost_for_chip(input.memory, c, input.now_ms, input.hour);
        if let Some(hit) = input.memory.hits.iter().find(|h| h.key == chip_memory_key(c)) {
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
            && is_plain_text(&c.value)
            && !is_desk_takeover_chip(c)
    });
    if input.grok_connected {
        chips.retain(|c| !is_stale_connect_chip(c));
    }
    let draft = input.draft.trim().to_ascii_lowercase();
    chips.retain(|c| c.value.trim().to_ascii_lowercase() != draft);

    apply_intent_boost(&mut chips, &intents);
    if !draft.is_empty() {
        apply_draft_boost(&mut chips, input.draft, &intents);
    }

    if stage != ChipStage::Empty && input.chat.len() >= 2 {
        for c in &mut chips {
            if matches!(c.kind, ChipKind::Nav | ChipKind::Mode) {
                c.score -= 16.0;
            }
        }
    }

    chips = uniq_by_value(chips);
    chips.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    if draft.len() >= 2 {
        let filtered: Vec<_> = chips
            .iter()
            .filter(|c| {
                let hay = format!("{} {} {}", c.label, c.value, c.hint).to_ascii_lowercase();
                c.score >= 85.0
                    || c.id.starts_with("pred-")
                    || hay.contains(&draft)
                    || draft.split_whitespace().any(|t| t.len() > 2 && hay.contains(t))
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
        ChipKind::Mode => 3,
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
        if c.kind == ChipKind::Mode && n >= 1 {
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

pub fn chip_suggest_prompt(
    chat: &[(String, String)],
    thread_title: &str,
    draft: &str,
    habits: &[String],
    dismissed: &[String],
    other_threads: &[ChipThread],
) -> String {
    let ctx = detect_chip_context(chat);
    let stage = detect_chip_stage(chat, false);
    let mut flags = vec![];
    if ctx.code {
        flags.push("code");
    }
    if ctx.app {
        flags.push("app");
    }
    if ctx.host {
        flags.push("host");
    }
    if ctx.imagine {
        flags.push("imagine");
    }
    if ctx.error {
        flags.push("error");
    }
    if ctx.decide {
        flags.push("decide");
    }
    if ctx.implement {
        flags.push("impl");
    }
    if ctx.incomplete {
        flags.push("todo");
    }
    let transcript = chat
        .iter()
        .filter(|(r, _)| r == "user" || r == "assistant")
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .filter_map(|(role, content)| {
            let who = if role == "user" { "User" } else { "Asst" };
            let content = chip_scan(content);
            let body: String = content.replace("```", "[code]").split_whitespace().collect::<Vec<_>>().join(" ");
            let body: String = body.chars().take(220).collect();
            if body.is_empty() {
                None
            } else {
                Some(format!("{who}: {body}"))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut lines = vec![
        "You generate quick-action chips for a Grok Build cabin chat.".into(),
        "Return ONLY valid JSON array of up to 5 objects:".into(),
        r#"[{"label":"≤28 chars","value":"full prompt to send","kind":"chat","hint":"why useful"}]"#.into(),
        String::new(),
        "Rules:".into(),
        "- label: short, action-first (e.g. Fix this bug, Add tests, Next step, Think Harder)".into(),
        "- value: a concrete instruction the user would send (1–2 sentences max)".into(),
        "- Prefer next steps grounded in the transcript (not generic tips)".into(),
        "- Predict what the user will want NEXT, not a recap of what already happened".into(),
        "- If user is mid-typing (draft shown), align chips with that draft".into(),
        "- Also use previous chats, habits, and actions taken on earlier replies".into(),
        "- Avoid chips the user dismissed".into(),
        "- Prefer habits if they fit the current context".into(),
        "- No markdown fences, no commentary outside JSON".into(),
        "- Cabin-real only: no SuperGrok, subscription, Outlook, Gmail, Drive, or Office".into(),
        "- Never suggest Turn host on, Take over this desktop, Scan the desk, Check the machine, Voice, or Hey Grok. Voice is the mic. Grok Build already has computer-use from chat.".into(),
        "- Never emit HOST_CMD or HOST_RESULT. Grok Build runs bash itself.".into(),
        "- Imagine stills use grok-imagine-image-2.0; Video kind uses grok-imagine-video-1.5".into(),
        String::new(),
        format!("Stage: {}", stage.as_str()),
        format!("Signals: {}", if flags.is_empty() { "none".into() } else { flags.join(", ") }),
    ];
    if !thread_title.trim().is_empty() {
        lines.push(format!("Thread title: {}", thread_title.trim()));
    }
    if !draft.trim().is_empty() {
        let d: String = draft.trim().chars().take(80).collect();
        lines.push(format!("User is typing: {d}"));
    }
    if !habits.is_empty() {
        lines.push(format!("User habits: {}", habits.iter().take(6).cloned().collect::<Vec<_>>().join(" · ")));
    }
    if !dismissed.is_empty() {
        lines.push(format!("Dismissed (avoid): {}", dismissed.iter().take(8).cloned().collect::<Vec<_>>().join(" · ")));
    }
    let prev: Vec<String> = other_threads
        .iter()
        .filter(|t| !skip_other_thread_title(&t.title) && is_plain_text(&t.title))
        .take(6)
        .filter_map(|t| {
            let ask: String = t.last_user.trim().chars().take(120).collect();
            let reply: String = t.last_assistant.trim().chars().take(120).collect();
            if !is_plain_text(&ask) || !is_plain_text(&reply) {
                return None;
            }
            Some(format!(
                "- {} · last ask: {} · last reply: {}",
                t.title.trim(),
                if ask.is_empty() { "(none)".into() } else { ask },
                if reply.is_empty() { "(none)".into() } else { reply }
            ))
        })
        .collect();
    if !prev.is_empty() {
        lines.push("Previous chats:".into());
        lines.extend(prev);
    }
    lines.push(String::new());
    lines.push("Transcript:".into());
    lines.push(if transcript.is_empty() {
        "(empty chat)".into()
    } else {
        transcript
    });
    lines.join("\n")
}

pub fn parse_llm_chips(raw: &str) -> Vec<QuickChip> {
    let mut text = raw.trim().to_string();
    if let Some(rest) = text.strip_prefix("```json") {
        text = rest.to_string();
    } else if let Some(rest) = text.strip_prefix("```") {
        text = rest.to_string();
    }
    text = text.trim().trim_end_matches("```").trim().to_string();
    if let (Some(start), Some(end)) = (text.find('['), text.rfind(']')) {
        if end > start {
            text = text[start..=end].to_string();
        }
    }
    let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&text);
    let mut out = vec![];
    match parsed {
        Ok(arr) => {
            for (i, row) in arr.into_iter().enumerate() {
                if out.len() >= CHIP_VISIBLE_MAX {
                    break;
                }
                let label = row
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let value = row
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let label: String = label.chars().take(32).collect();
                let value: String = value.chars().take(400).collect();
                if label.len() < 2 || value.len() < 4 {
                    continue;
                }
                if !is_plain_text(&label) || !is_plain_text(&value) {
                    continue;
                }
                if !cabin_chip_copy_ok(&label, &value, "") {
                    continue;
                }
                let mut kind = ChipKind::from_str(row.get("kind").and_then(|v| v.as_str()).unwrap_or("chat"));
                if kind == ChipKind::Shell && !value.starts_with('$') && !value.starts_with("/sh") {
                    kind = ChipKind::Chat;
                }
                let hint = row
                    .get("hint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Suggested from this chat")
                    .chars()
                    .take(80)
                    .collect::<String>();
                out.push(QuickChip {
                    id: format!("llm-{i}-{}", label.chars().take(10).collect::<String>().replace(' ', "")),
                    label,
                    value,
                    kind,
                    score: 99.0 - i as f32 * 1.5,
                    hint,
                    primary: i == 0,
                });
            }
        }
        Err(_) => {
            for (i, line) in raw.lines().map(str::trim).filter(|l| !l.is_empty()).take(CHIP_VISIBLE_MAX).enumerate() {
                let parts: Vec<_> = line.split('|').map(str::trim).collect();
                let label = parts[0].trim_start_matches(['-', '*', '1', '2', '3', '4', '.', ')', ' ']);
                let value = if parts.len() > 1 { parts[1] } else { parts[0] };
                if label.len() < 2 || value.len() < 4 || !is_plain_text(value) {
                    continue;
                }
                if !cabin_chip_copy_ok(label, value, "") {
                    continue;
                }
                out.push(chip(
                    &format!("llm-{i}-{}", label.chars().take(12).collect::<String>()),
                    &label.chars().take(32).collect::<String>(),
                    &value.chars().take(400).collect::<String>(),
                    ChipKind::Chat,
                    98.0 - i as f32,
                    "Suggested from this chat",
                ));
            }
        }
    }
    out
}

fn cabin_chip_copy_ok(label: &str, value: &str, hint: &str) -> bool {
    if retired_host_copy("", label, value) || retired_host_copy("", hint, "") {
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
        "__nav:imagine" => Some("imagine"),
        "__nav:history" => Some("history"),
        "__nav:workboard" => Some("workboard"),
        "__nav:skills" => Some("skills"),
        "__nav:automations" | "__nav:night" => Some("night"),
        "__nav:command" => Some("command"),
        "__nav:queue" | "__nav:agents" => Some("agents"),
        "__nav:chat" | "__nav:agent" => Some("chat"),
        _ => None,
    }
}

pub fn mode_from_chip_value(value: &str) -> Option<&'static str> {
    let v = value.trim();
    let rest = v.strip_prefix("__mode:").unwrap_or(v);
    match rest {
        "max" | "deep" | "heavy" => Some("max"),
        "think" | "thinking" | "build" | "expert" => Some("think"),
        "balanced" | "balance" => Some("balanced"),
        "fast" => Some("fast"),
        "auto" => Some("auto"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            host_on: true,
            mode: "auto",
            thread_title: "Chat",
            usage_messages: 1,
            usage_cap: 40,
            memory,
            dismissed,
            llm_chips: llm,
            last_failed: false,
            hour: 21,
            now_ms: 1_000_000,
            max: 5,
            other_threads: &[],
        }
    }

    #[test]
    fn empty_stage_offers_cabin_chips() {
        let mem = ChipMemory::default();
        let chips = build_quick_chips(input(&[], "", &mem, &[], &[]));
        let labels: Vec<_> = chips.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels.contains(&"Open Imagine"),
            "empty {:?}",
            labels
        );
        assert!(
            labels.iter().any(|l| *l == "Cabin brief" || *l == "Recent chats" || *l == "What's next"),
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
    fn mid_stage_offers_think_harder() {
        let mem = ChipMemory::default();
        let chat = [
            msg("user", "hi"),
            msg("assistant", "Hello — what should we work on in the cabin tonight?"),
        ];
        let chips = build_quick_chips(input(&chat, "", &mem, &[], &[]));
        let think = chips
            .iter()
            .find(|c| c.label == "Think Harder" && c.kind == ChipKind::Mode)
            .expect("think chip");
        assert_eq!(think.value, "__mode:think");
        assert_eq!(mode_from_chip_value(&think.value), Some("think"));
        assert!(chips.len() <= CHIP_VISIBLE_MAX);
    }

    #[test]
    fn routing_chips_follow_new_modes() {
        let mem = ChipMemory::default();
        let chat = [
            msg("user", "hi"),
            msg("assistant", "Hello — what should we work on in the cabin tonight?"),
        ];
        let mut auto = input(&chat, "", &mem, &[], &[]);
        auto.mode = "auto";
        let auto_chips = build_quick_chips(auto);
        assert!(auto_chips.iter().any(|c| {
            c.label == "Think Harder" && c.value == "__mode:think" && c.kind == ChipKind::Mode
        }));

        let mut think = input(&chat, "", &mem, &[], &[]);
        think.mode = "think";
        let think_chips = build_quick_chips(think);
        assert!(think_chips.iter().any(|c| {
            (c.label == "Go Max" || c.label == "Max")
                && c.value == "__mode:max"
                && c.kind == ChipKind::Mode
        }));
        assert!(!think_chips.iter().any(|c| c.label == "Think Harder"));

        let mut max = input(&chat, "", &mem, &[], &[]);
        max.mode = "max";
        let max_chips = build_quick_chips(max);
        assert!(max_chips.iter().any(|c| {
            c.label == "Use Adaptive" && c.value == "__mode:auto" && c.kind == ChipKind::Mode
        }));
        assert!(!max_chips.iter().any(|c| c.label == "Think Harder"));
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
        assert_eq!(voices, 0, "Voice is the mic, not a chip: {:?}", labels(&chips));
    }

    #[test]
    fn disconnected_surfaces_connect_grok() {
        let mem = ChipMemory::default();
        let mut inp = input(&[], "", &mem, &[], &[]);
        inp.grok_connected = false;
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
            chips.iter().map(|c| format!("{}:{}", c.id, c.label)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn error_stage_and_fix_intent() {
        let chat = [
            msg("user", "the build failed"),
            msg("assistant", "TypeError: foo is not a function. The crash is in render."),
        ];
        assert_eq!(detect_chip_stage(&chat, false), ChipStage::Error);
        let intents = predict_intents(&chat, "");
        assert!(intents[&PredictedIntent::Fix] >= 0.5);
        let mem = ChipMemory::default();
        let chips = build_quick_chips(input(&chat, "", &mem, &[], &[]));
        assert!(chips.iter().any(|c| c.id.contains("diagnose") || c.label.contains("fix") || c.label.contains("Root")));
    }

    #[test]
    fn draft_fix_predicts_debug_chip() {
        let chat = [msg("user", "auth is broken"), msg("assistant", "I can look at the oauth path next.")];
        let mem = ChipMemory::default();
        let chips = build_quick_chips(input(&chat, "fix the", &mem, &[], &[]));
        assert!(
            chips.iter().any(|c| c.id.starts_with("pred-") && (c.label.contains("Debug") || c.label.contains("Fix"))),
            "{:?}",
            chips.iter().map(|c| format!("{}:{}", c.id, c.label)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn dismiss_and_habit_rerank() {
        let mem0 = ChipMemory::default();
        let chat = [msg("user", "hi"), msg("assistant", "Hello — what should we work on in the cabin tonight?")];
        let base = build_quick_chips(input(&chat, "", &mem0, &[], &[]));
        let think = base.iter().find(|c| c.label == "Think Harder").cloned().expect("think chip");
        let mut mem = ChipMemory::default();
        remember_chip_dismiss(&mut mem, &think, 2_000, 21);
        let after = build_quick_chips(input(&chat, "", &mem, &[think.id.clone(), think.value.clone()], &[]));
        assert!(!after.iter().any(|c| c.id == think.id));

        let habit = chip("empty-brief", "Cabin brief", "Give me a short cabin brief: bound project, recent chats, and the next useful step.", ChipKind::Chat, 64.0, "New chat");
        remember_chip_click(&mut mem, &habit, Some("mid"), 3_000, 21);
        remember_chip_click(&mut mem, &habit, Some("mid"), 4_000, 21);
        remember_chip_click(&mut mem, &habit, Some("mid"), 5_000, 21);
        let ranked = build_quick_chips(input(&chat, "", &mem, &[], &[]));
        assert!(
            ranked.iter().any(|c| c.label.contains("Cabin brief") || c.value.contains("cabin brief")),
            "{:?}",
            ranked.iter().map(|c| c.label.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn secrets_never_enter_memory_or_llm_parse() {
        let mut mem = ChipMemory::default();
        remember_typed_prompt(&mut mem, "token sk-abcdefghijklmnopqrstuv", 1, 3);
        assert!(mem.hits.is_empty());
        let parsed = parse_llm_chips(r#"[{"label":"Leak","value":"use token sk-abcdefghijklmnopqrstuv now","kind":"chat"}]"#);
        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_llm_json_and_prompt_grounding() {
        let chat = [msg("user", "add tests"), msg("assistant", "I will add coverage next.")];
        let prompt = chip_suggest_prompt(&chat, "Auth", "add tes", &["Check the machine".into()], &["Open Imagine".into()], &[]);
        assert!(prompt.contains("Stage:"));
        assert!(prompt.contains("User is typing: add tes"));
        assert!(prompt.contains("User habits:"));
        assert!(prompt.contains("Auth"));
        let chips = parse_llm_chips(
            r#"```json
[{"label":"Add unit tests","value":"Write focused unit tests for the auth path.","kind":"chat","hint":"from thread"}]
```"#,
        );
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].label, "Add unit tests");
        assert!(chips[0].score >= 95.0);
    }

    #[test]
    fn fingerprint_and_llm_debounce() {
        let empty: [(String, String); 0] = [];
        let err = [msg("user", "crash"), msg("assistant", "error: boom failed")];
        let a = context_fingerprint(&empty, "", false, 21, "auto");
        let b = context_fingerprint(&err, "fix", false, 21, "auto");
        assert_ne!(a, b);
        let think = context_fingerprint(&empty, "", false, 21, "think");
        assert_ne!(a, think, "mode change should refresh Fast chips");
        assert!(should_refresh_llm(&a, &b, 0, 2000, true, false));
        assert!(!should_refresh_llm(&b, &b, 0, 2000, true, false));
        assert!(!should_refresh_llm(&a, &b, 1800, 2000, true, false));
        assert!(!should_refresh_llm(&a, &b, 0, 2000, false, false));
        assert!(!should_refresh_llm(&a, &b, 0, 2000, true, true));
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
        let chat = [msg("user", "implement oauth"), msg("assistant", "Here is a plan to wire the device code flow.")];
        let chips = build_quick_chips(input(&chat, "", &mem, &[], &llm));
        assert!(chips.iter().any(|c| c.label == "Ship the patch"));
        assert!(chips.len() <= 5);
    }

    #[test]
    fn mode_and_nav_values() {
        assert_eq!(mode_from_chip_value("__mode:max"), Some("max"));
        assert_eq!(mode_from_chip_value("__mode:auto"), Some("auto"));
        assert_eq!(mode_from_chip_value("__mode:fast"), Some("fast"));
        assert_eq!(mode_from_chip_value("fast"), Some("fast"));
        assert_eq!(mode_from_chip_value("__mode:think"), Some("think"));
        assert_eq!(mode_from_chip_value("__mode:balanced"), Some("balanced"));
        assert_eq!(nav_from_chip_value("__nav:imagine"), Some("imagine"));
    }

    #[test]
    fn llm_chips_drop_missing_products() {
        let chips = parse_llm_chips(
            r#"[{"label":"Make a video","value":"Generate a video of the cabin.","kind":"chat","hint":"x"}]"#,
        );
        assert!(chips.is_empty());
        let office = parse_llm_chips(
            r#"[{"label":"Office mail","value":"Draft the weekly office memo.","kind":"chat"}]"#,
        );
        assert!(office.is_empty(), "{office:?}");
        let stock = parse_llm_chips(
            r#"[{"label":"Market move","value":"Check the stock ticker.","kind":"chat"}]"#,
        );
        assert!(stock.is_empty(), "{stock:?}");
    }

    #[test]
    fn chips_do_not_advertise_missing_products() {
        let mem = ChipMemory::default();
        let mut inp = input(&[], "", &mem, &[], &[]);
        inp.usage_messages = 40;
        inp.usage_cap = 40;
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
    fn typed_prompt_becomes_habit() {
        let mut mem = ChipMemory::default();
        remember_typed_prompt(&mut mem, "morning brief for the cabin", 10, 8);
        remember_typed_prompt(&mut mem, "morning brief for the cabin", 20, 8);
        remember_typed_prompt(&mut mem, "/clear", 30, 8);
        remember_typed_prompt(&mut mem, "/compact", 40, 8);
        assert_eq!(mem.hits.len(), 1);
        assert!(mem.hits[0].typed_uses >= 2);
        assert_eq!(top_habit_labels(&mem, 3)[0], shorten("morning brief for the cabin", 32));
    }

    fn labels(chips: &[QuickChip]) -> Vec<String> {
        chips.iter().map(|c| c.label.clone()).collect()
    }

    #[test]
    fn always_five_visible_chips() {
        let mem = ChipMemory::default();
        let empty = build_quick_chips(input(&[], "", &mem, &[], &[]));
        assert_eq!(empty.len(), CHIP_VISIBLE_MAX, "empty {:?}", labels(&empty));

        let chat = [
            msg("user", "hi"),
            msg("assistant", "Hello — what should we work on in the cabin tonight?"),
        ];
        let mid = build_quick_chips(input(&chat, "", &mem, &[], &[]));
        assert_eq!(mid.len(), CHIP_VISIBLE_MAX, "mid {:?}", labels(&mid));

        let dismissed: Vec<String> = empty
            .iter()
            .flat_map(|c| [c.id.clone(), c.value.clone()])
            .collect();
        let after = build_quick_chips(input(&[], "", &mem, &dismissed, &[]));
        assert_eq!(
            after.len(),
            CHIP_VISIBLE_MAX,
            "after dismissing the first row {:?}",
            labels(&after)
        );
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
            chips.iter().all(|c| !retired_host_copy(&c.id, &c.label, &c.value)),
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
            empty.iter().all(|c| !retired_host_copy(&c.id, &c.label, &c.value)),
            "empty home must not paint leftover desk chips: {:?}",
            labels(&empty)
        );
        assert!(
            parse_llm_chips(
                r#"[{"label":"Check the machine","value":"Run HOST_CMD now.","kind":"chat"},{"label":"Scan the desk","value":"Look at my desk.","kind":"chat"}]"#
            )
            .is_empty()
        );
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
            last.contains("chip_scan") || last.contains("TEXT_FILE_CAP") || last.contains("CHIP_SCAN"),
            "last_of must not clone an 8MB assistant for chips: {last}"
        );
        let suggest = src
            .split("pub fn chip_suggest_prompt(")
            .nth(1)
            .and_then(|s| s.split("pub fn ").next())
            .expect("chip_suggest_prompt");
        let replace = suggest.find("replace(\"```\"").expect("fence replace");
        assert!(
            suggest[..replace].contains("chip_scan")
                || suggest[..replace].contains("TEXT_FILE_CAP")
                || suggest[..replace].contains("CHIP_SCAN"),
            "chip prompt must not split_whitespace an 8MB body: {suggest}"
        );
    }

    #[test]
    fn chip_suggest_prompt_asks_for_five() {
        let prompt = chip_suggest_prompt(&[], "Chat", "", &[], &[], &[]);
        assert!(
            prompt.contains("up to 5") || prompt.contains("5 objects"),
            "Fast chip prompt should ask for five chips: {prompt}"
        );
        assert!(
            !prompt.contains("3 to 4"),
            "Fast chip prompt still asks for 3 to 4: {prompt}"
        );
    }

    #[test]
    fn parse_llm_accepts_five() {
        let chips = parse_llm_chips(
            r#"[{"label":"Continue the wall","value":"Continue painting the cabin wall.","kind":"chat"},{"label":"Cabin brief","value":"Give me a short cabin brief.","kind":"chat"},{"label":"Think Harder","value":"__mode:think","kind":"mode"},{"label":"Open Imagine","value":"__nav:imagine","kind":"nav"},{"label":"Make a checklist","value":"Turn the last answer into a short checklist.","kind":"chat"}]"#,
        );
        assert_eq!(chips.len(), 5, "{:?}", labels(&chips));
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
        inp.other_threads = &others;
        let chips = build_quick_chips(inp);
        assert!(
            chips.iter().any(|c| {
                c.label.contains("Night cabin") || c.value.contains("paint the wall")
            }),
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
    fn chip_suggest_prompt_includes_other_chats() {
        let others = [ChipThread {
            title: "Night cabin".into(),
            last_user: "paint the wall".into(),
            last_assistant: "First coat is ready.".into(),
        }];
        let prompt = chip_suggest_prompt(&[], "Chat", "", &["Check the machine".into()], &[], &others);
        assert!(prompt.contains("Night cabin"), "{prompt}");
        assert!(prompt.contains("paint the wall"), "{prompt}");
        assert!(prompt.contains("Previous"), "{prompt}");
    }

    #[test]
    fn chip_llm_is_fast_mode() {
        assert_eq!(CHIP_LLM_MODE, "fast");
        assert_eq!(crate::CABIN_FAST_MODEL, "grok-4.6");
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
        remember_chip_outcome(&mut mem, true, 4_000);
        remember_chip_outcome(&mut mem, true, 5_000);
        let chat = [
            msg("user", "hi"),
            msg("assistant", "Hello — what should we work on in the cabin tonight?"),
        ];
        let ranked = build_quick_chips(input(&chat, "", &mem, &[], &[]));
        assert!(
            ranked.iter().any(|c| c.label.contains("Cabin brief") || c.value.contains("cabin brief")),
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
            cleaned.iter().all(|c| !c.label.contains("machine") && !c.label.to_ascii_lowercase().contains("desk")),
            "{:?}",
            labels(&cleaned)
        );
    }
}
