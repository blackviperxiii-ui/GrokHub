//! Per-task model picking: what kind of task a turn is, what the router has
//! learned about each model's misses on each kind, and the Dynamic switch.
//!
//! The router ([`super::local_swap`]) runs the Settings pick for everyday
//! chat and, with Dynamic on, a model on disk that is clearly better at the
//! task in hand (code, reasoning, writing, a long paste, 18+ chat). With
//! Dynamic off the pick answers every task; only the swaps that protect the
//! computer (a game, a low battery, too little memory) still happen.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use grokhub_core::local_skills::TaskKind;
use serde::{Deserialize, Serialize};

/// Classes that are coding work.
const CODE_CLASSES: &[&str] = &["debug", "repair:diagnose"];
/// Classes that are reasoning and planning work.
const REASON_CLASSES: &[&str] = &["chat:reason", "plan", "design", "prepare:hard"];
/// A message this long (a big paste) is a long-context task.
pub const LONG_TOKENS: u64 = 6_000;

/// Asks for long-form or creative writing.
const WRITE_ASKS: &[&str] = &[
    "write a story",
    "write me a story",
    "short story",
    "write a poem",
    "write me a poem",
    "a poem about",
    "write an essay",
    "essay on",
    "essay about",
    "blog post",
    "cover letter",
    "write a letter",
    "write me a letter",
    "song lyrics",
    "write lyrics",
    "next chapter",
    "write a chapter",
    "rewrite this",
    "rewrite my",
    "proofread",
    "edit my essay",
    "write a speech",
    "write a scene",
];

/// The kind of task a turn is, or `None` for the app's own background work.
pub fn kind(class: &str, ctx_tokens: u64, text: &str) -> Option<TaskKind> {
    let class = super::policy::class_row(class).map_or(class.trim(), |r| r.class);
    if class.starts_with("background:") {
        return None;
    }
    if grokhub_core::adult_guard::check(text) == grokhub_core::adult_guard::Risk::Explicit {
        return Some(TaskKind::Adult);
    }
    if class.starts_with("code:") || CODE_CLASSES.contains(&class) || text.contains("```") {
        return Some(TaskKind::Code);
    }
    if REASON_CLASSES.contains(&class) {
        return Some(TaskKind::Reason);
    }
    if ctx_tokens > LONG_TOKENS {
        return Some(TaskKind::Long);
    }
    let lower = text.to_lowercase();
    if WRITE_ASKS.iter().any(|a| lower.contains(a)) {
        return Some(TaskKind::Write);
    }
    Some(TaskKind::Chat)
}

thread_local! {
    static KIND: Cell<Option<TaskKind>> = const { Cell::new(None) };
}

/// Sets this thread's task kind until dropped (like
/// [`super::local_scale::NeedGuard`]).
pub struct KindGuard(Option<TaskKind>);

impl KindGuard {
    pub fn set(kind: Option<TaskKind>) -> Self {
        KindGuard(KIND.with(|k| k.replace(kind)))
    }
}

impl Drop for KindGuard {
    fn drop(&mut self) {
        KIND.with(|k| k.set(self.0));
    }
}

/// The task kind of the request this thread serves.
pub fn current_kind() -> Option<TaskKind> {
    KIND.with(Cell::get)
}

/// A miss (a refusal, an error) on a task kind lowers that model's score for
/// it by this much.
pub const MISS_MARK: u8 = 10;
/// The most a model's score for one kind is lowered.
pub const MAX_MARK: u8 = 40;
/// A good reply eases a mark by this much.
pub const OK_EASE: u8 = 2;

/// What the router learned: per model id, per kind key, how far to lower
/// the catalog score.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marks(BTreeMap<String, BTreeMap<String, u8>>);

impl Marks {
    pub const fn new() -> Self {
        Marks(BTreeMap::new())
    }

    pub fn penalty(&self, id: &str, kind: TaskKind) -> u8 {
        self.0
            .get(id)
            .and_then(|k| k.get(kind.key()))
            .copied()
            .unwrap_or(0)
    }

    /// Record how a reply on `id` went for `kind`. True when a mark changed.
    pub fn note(&mut self, id: &str, kind: TaskKind, ok: bool) -> bool {
        let now = self.penalty(id, kind);
        let next = if ok {
            now.saturating_sub(OK_EASE)
        } else {
            now.saturating_add(MISS_MARK).min(MAX_MARK)
        };
        if next == now {
            return false;
        }
        let row = self.0.entry(id.to_string()).or_default();
        if next == 0 {
            row.remove(kind.key());
            if row.is_empty() {
                self.0.remove(id);
            }
        } else {
            row.insert(kind.key().to_string(), next);
        }
        true
    }
}

const MARKS_FILE: &str = "local_task_marks.json";
const ROUTING_FILE: &str = "local_routing.json";

static MARKS: Mutex<Option<(PathBuf, Marks)>> = Mutex::new(None);

/// The learned marks under `config_dir` (read once, then kept).
pub fn marks(config_dir: &Path) -> Marks {
    let mut slot = MARKS.lock().unwrap_or_else(|e| e.into_inner());
    match slot.as_ref() {
        Some((dir, m)) if dir == config_dir => m.clone(),
        _ => {
            let m = read_marks(config_dir);
            *slot = Some((config_dir.to_path_buf(), m.clone()));
            m
        }
    }
}

fn read_marks(config_dir: &Path) -> Marks {
    std::fs::read_to_string(config_dir.join(MARKS_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// A reply on the running model for `kind` finished: `ok` when it answered,
/// not when it refused or failed. The mark is saved when it changed.
pub fn note_reply(config_dir: &Path, model: &Path, kind: TaskKind, ok: bool) {
    let Some(m) = super::local_swap::catalog_for(model) else {
        return;
    };
    let mut all = marks(config_dir);
    if !all.note(m.id, kind, ok) {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(&all) {
        let _ = std::fs::write(config_dir.join(MARKS_FILE), format!("{json}\n"));
    }
    *MARKS.lock().unwrap_or_else(|e| e.into_inner()) = Some((config_dir.to_path_buf(), all));
}

/// A reply's text answered the ask (not a refusal, not empty).
pub fn answered(text: &str, has_calls: bool) -> bool {
    has_calls || (!text.trim().is_empty() && !grokhub_core::local_refusal::is_refusal(text))
}

/// The Dynamic row's hint and button, by whether it is on.
pub fn dynamic_row(on: bool) -> (&'static str, &'static str) {
    if on {
        (
            "On: each task runs on the model on disk that's best at it, like a coder for code. Your pick answers everyday chat.",
            "Turn off",
        )
    } else {
        (
            "Off: your pick answers every task. A game, a low battery or too little memory still swaps to a lighter one.",
            "Turn on",
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Routing {
    dynamic: bool,
}

impl Default for Routing {
    fn default() -> Self {
        Routing { dynamic: true }
    }
}

/// 0: not read yet, 1: on, 2: off.
static DYNAMIC: AtomicU8 = AtomicU8::new(0);

/// Dynamic is on: the router picks the best model on disk per task. On by
/// default.
pub fn dynamic(config_dir: &Path) -> bool {
    match DYNAMIC.load(Ordering::SeqCst) {
        1 => true,
        2 => false,
        _ => {
            let on = std::fs::read_to_string(config_dir.join(ROUTING_FILE))
                .ok()
                .and_then(|s| serde_json::from_str::<Routing>(&s).ok())
                .unwrap_or_default()
                .dynamic;
            DYNAMIC.store(if on { 1 } else { 2 }, Ordering::SeqCst);
            on
        }
    }
}

/// Turn Dynamic on or off and save it.
pub fn set_dynamic(config_dir: &Path, on: bool) -> Result<(), String> {
    let json = serde_json::to_string_pretty(&Routing { dynamic: on }).map_err(|e| e.to_string())?;
    std::fs::write(config_dir.join(ROUTING_FILE), format!("{json}\n"))
        .map_err(|e| format!("couldn't save the Dynamic setting: {e}"))?;
    DYNAMIC.store(if on { 1 } else { 2 }, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_get_the_kind_of_task_they_are() {
        assert_eq!(kind("background:summarize", 10, "hi"), None);
        assert_eq!(
            kind("code:edit-small", 10, "rename this"),
            Some(TaskKind::Code)
        );
        assert_eq!(kind("debug", 10, "why does it crash"), Some(TaskKind::Code));
        assert_eq!(
            kind("chat:default", 10, "what does ```x = 1``` do"),
            Some(TaskKind::Code)
        );
        assert_eq!(kind("plan", 10, "plan my week"), Some(TaskKind::Reason));
        assert_eq!(
            kind("chat:default", 9_000, "summarize this"),
            Some(TaskKind::Long)
        );
        assert_eq!(
            kind("chat:default", 20, "Write me a story about a dragon"),
            Some(TaskKind::Write)
        );
        assert_eq!(
            kind("chat:quick", 5, "what time is it in Tokyo"),
            Some(TaskKind::Chat)
        );
    }

    #[test]
    fn a_miss_marks_a_model_down_and_good_replies_ease_it_back() {
        let mut m = Marks::default();
        assert_eq!(m.penalty("a", TaskKind::Code), 0);
        assert!(m.note("a", TaskKind::Code, false));
        assert_eq!(m.penalty("a", TaskKind::Code), MISS_MARK);
        for _ in 0..10 {
            m.note("a", TaskKind::Code, false);
        }
        assert_eq!(m.penalty("a", TaskKind::Code), MAX_MARK);
        assert_eq!(m.penalty("a", TaskKind::Write), 0, "only that kind");
        for _ in 0..20 {
            m.note("a", TaskKind::Code, true);
        }
        assert_eq!(
            m,
            Marks::default(),
            "an eased-out mark leaves nothing behind"
        );
        assert!(!m.note("a", TaskKind::Code, true), "nothing to ease");
    }

    #[test]
    fn a_reply_counts_unless_it_refuses_or_is_empty() {
        assert!(answered("Here's the fix.", false));
        assert!(!answered("I can't help with that.", false));
        assert!(!answered("  ", false));
        assert!(answered("", true));
    }

    #[test]
    fn the_dynamic_button_offers_the_other_state() {
        assert_eq!(dynamic_row(true).1, "Turn off");
        assert_eq!(dynamic_row(false).1, "Turn on");
        assert!(dynamic_row(false).0.contains("game"));
    }

    #[test]
    fn dynamic_is_on_by_default_and_saves() {
        let dir = std::env::temp_dir().join(format!("grokhub-dyn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        DYNAMIC.store(0, Ordering::SeqCst);
        assert!(dynamic(&dir));
        set_dynamic(&dir, false).expect("save");
        DYNAMIC.store(0, Ordering::SeqCst);
        assert!(!dynamic(&dir), "read back from the file");
        set_dynamic(&dir, true).expect("save");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
