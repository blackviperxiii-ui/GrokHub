//! Auto setup in the desktop app. The shared settings list
//! (`grokhub_core::autotune`) maps onto `shell.json` here: the first-run
//! questions and summary, a pass at launch, back from a break and when the PC
//! changes, one note at a time (what changed, with Undo, or an offer), the
//! Settings → Auto setup page, and chat asks like "make the text bigger".
//!
//! It never picks an 18+ voice or a voice the person chose, never raises
//! approvals past their own answer and leaves "Always approve" alone. Local
//! models and their launch tuning are not settings here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use grokhub_core::autotune::chat::{self as auto_chat, SettingAsk};
use grokhub_core::autotune::plan::{self, Change, Moment};
use grokhub_core::autotune::questions::{self, Pace, Qid};
use grokhub_core::autotune::rules::Signals;
use grokhub_core::autotune::settings::{
    self as auto_settings, describe, lower_first, Key, Target, Value,
};
use grokhub_core::autotune::store::{self as auto_store, AutoStore, JournalLine};
use grokhub_core::autotune::DeviceProfile;
use serde::{Deserialize, Serialize};

use crate::settings::{Approvals, ShellSettings, Theme};

/// The desktop app's own record of what it learned and changed. Not the
/// cabin's: the two apps hold different settings.
pub fn dir(config_dir: &Path) -> PathBuf {
    config_dir.join("desktop-auto-setup")
}

/// What the page asks for.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "act", rename_all = "camelCase")]
pub enum Act {
    /// Just the current state.
    View,
    /// A look at the PC: at launch, every few minutes, and on the way back
    /// to the window (`back` after a long break).
    Pass {
        back: bool,
    },
    /// First run: one text-size step up.
    Bigger,
    /// First run: "Just set it up" or "Tailor it".
    Start {
        pace: Pace,
    },
    /// First run: a choice, or Skip (`None`).
    Answer {
        id: Qid,
        value: Option<String>,
    },
    /// First run: Done or Skip setup.
    Finish,
    Undo {
        key: Key,
    },
    /// Yes to an offer.
    Accept {
        key: Key,
    },
    /// Not now to an offer.
    Dismiss {
        key: Key,
    },
    /// A pick in Settings: the person's from then on.
    Set {
        key: Key,
        value: Value,
    },
    /// Settings → Auto setup: tune this one again.
    HandBack {
        key: Key,
    },
    HandBackAll,
    Forget,
    /// A chat line, answered here when it asks for a setting.
    Chat {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub key: Key,
    /// "Text size: 150%" or "Turn on reduce motion?".
    pub text: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub line: Line,
    /// An offer (Yes, Not now); otherwise a change (Undo, OK).
    pub offer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    pub label: &'static str,
    pub value: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Question {
    pub id: Qid,
    pub prompt: &'static str,
    pub choices: Vec<Choice>,
}

/// One row of Settings → Auto setup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub key: Key,
    pub label: &'static str,
    /// What it is and why: "150%. Windows shows text at 150%."
    pub text: String,
    /// Undo puts the old value back; otherwise the person set it, and Hand
    /// back lets auto setup tune it again.
    pub undo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    /// The first-run window shows: a new profile with no chats.
    pub first_run: bool,
    pub text_size: u16,
    pub reduce_motion: bool,
    pub theme: Theme,
    /// First run: the open question.
    pub question: Option<Question>,
    /// First run: the questions are done and the summary shows.
    pub summary: bool,
    pub made: Vec<Line>,
    pub offered: Vec<Line>,
    pub note: Option<Note>,
    pub rows: Vec<Row>,
    /// What auto setup says back to a chat ask or a pick.
    pub said: Option<String>,
}

/// Auto setup's state for this launch.
#[derive(Debug, Default)]
pub struct Session {
    store: Option<AutoStore>,
    /// The profile had chats when this launch first looked.
    had_chats: Option<bool>,
    launched: bool,
    generation: u64,
    started: bool,
    asked: Vec<Qid>,
    question: Option<Qid>,
    shown_at: Option<Instant>,
    summary: bool,
    made: Vec<Change>,
    offered: Vec<Change>,
    note: Option<Change>,
    note_offer: bool,
}

/// One act's view of the app: its settings, this launch, and the PC.
pub struct Tuner<'a> {
    pub settings: &'a mut ShellSettings,
    pub session: &'a mut Session,
    pub dir: PathBuf,
    /// `history::list` has chats; read once per launch.
    pub has_chats: &'a dyn Fn() -> bool,
    /// The tray icon exists on this platform.
    pub tray: bool,
    pub device: Option<DeviceProfile>,
    /// `device::generation()`: it goes up when the PC changes.
    pub generation: u64,
}

fn hour_of(hm: &str) -> Option<u8> {
    hm.split(':')
        .next()?
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|h| *h < 24)
}

fn theme_word(theme: Theme) -> &'static str {
    match theme {
        Theme::Light => "light",
        Theme::System => "system",
        Theme::Dark | Theme::OpsDark | Theme::OledBlack | Theme::SoftCharcoal => "dark",
    }
}

/// The three voices auto setup may pick between; any other id is the person's.
fn talk_word(personality: &str) -> &str {
    match personality {
        "" | "nyx" | "default" => "friendly",
        "concise-pro" => "direct",
        "sarcastic-sidekick" => "playful",
        other => other,
    }
}

fn talk_id(word: &str) -> Option<&'static str> {
    match word {
        "friendly" => Some(""),
        "direct" => Some("concise-pro"),
        "playful" => Some("sarcastic-sidekick"),
        _ => None,
    }
}

impl Target for Tuner<'_> {
    fn get(&self, key: Key) -> Option<Value> {
        let s = &self.settings;
        let b = &s.behavior;
        Some(match key {
            Key::TextSize => Value::Pct(s.text_size),
            Key::ReduceMotion => Value::Bool(s.reduce_motion),
            Key::Theme => Value::pick(theme_word(s.theme)),
            Key::CloseToTray if self.tray => Value::Bool(s.close_to_tray),
            Key::QuietHours => Value::Hours(hour_of(&b.quiet_start)?, hour_of(&b.quiet_end)?),
            Key::Notifications => Value::pick(match (s.notifications, s.notify_important) {
                (false, _) => "quiet",
                (true, true) => "important",
                (true, false) => "all",
            }),
            Key::Suggestions => Value::Bool(b.suggestions),
            Key::Personality => Value::pick(talk_word(&s.personality)),
            Key::Approvals => Value::pick(match s.approvals {
                Approvals::Ask => "ask",
                Approvals::Auto => "risky",
                Approvals::Always => "always",
            }),
            _ => return None,
        })
    }

    fn set(&mut self, key: Key, value: &Value) -> bool {
        let s = &mut *self.settings;
        match (key, value) {
            (Key::TextSize, Value::Pct(p)) => s.text_size = auto_settings::text_step_for(*p),
            (Key::ReduceMotion, Value::Bool(on)) => s.reduce_motion = *on,
            (Key::Theme, Value::Pick(t)) => match t.as_str() {
                "light" => s.theme = Theme::Light,
                "system" => s.theme = Theme::System,
                // Any dark variant already counts as dark.
                "dark" if theme_word(s.theme) == "dark" => {}
                "dark" => s.theme = Theme::OpsDark,
                _ => return false,
            },
            (Key::CloseToTray, Value::Bool(on)) if self.tray => s.close_to_tray = *on,
            (Key::QuietHours, Value::Hours(a, b)) if *a < 24 && *b < 24 && a != b => {
                s.behavior.quiet_start = format!("{a:02}:00");
                s.behavior.quiet_end = format!("{b:02}:00");
            }
            (Key::Notifications, Value::Pick(n)) => {
                let (on, important) = match n.as_str() {
                    "all" => (true, false),
                    "important" => (true, true),
                    "quiet" => (false, false),
                    _ => return false,
                };
                s.notifications = on;
                s.notify_important = important;
            }
            (Key::Suggestions, Value::Bool(on)) => s.behavior.suggestions = *on,
            (Key::Personality, Value::Pick(word)) => {
                // A voice the person picked (an 18+ one included) stays theirs.
                if talk_id(talk_word(&s.personality)).is_none() {
                    return false;
                }
                let Some(id) = talk_id(word) else {
                    return false;
                };
                s.personality = id.to_string();
            }
            (Key::Approvals, Value::Pick(a)) => {
                // "Always approve" was the person's own pick: leave it.
                if s.approvals == Approvals::Always {
                    return false;
                }
                s.approvals = match a.as_str() {
                    "ask" => Approvals::Ask,
                    "risky" => Approvals::Auto,
                    _ => return false,
                };
            }
            _ => return false,
        }
        true
    }
}

impl Tuner<'_> {
    fn clock_12h(&self) -> bool {
        self.device.as_ref().is_none_or(|d| d.clock_12h)
    }

    fn store(&mut self) -> &mut AutoStore {
        let dir = self.dir.clone();
        self.session
            .store
            .get_or_insert_with(|| auto_store::load(&dir))
    }

    /// Keep the store and its history lines.
    fn keep(&mut self, lines: &[JournalLine]) {
        let copy = self.store().clone();
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = auto_store::save(&self.dir, &copy);
        if !lines.is_empty() {
            let _ = auto_store::append_journal(&self.dir, lines);
        }
    }

    /// The first-run window shows until it's done, unless the profile
    /// already had chats when this launch started.
    pub fn first_run(&mut self) -> bool {
        let had = match self.session.had_chats {
            Some(had) => had,
            None => *self.session.had_chats.insert((self.has_chats)()),
        };
        !had && !self.store().setup_done
    }

    fn signals(&mut self, device: DeviceProfile) -> Signals {
        let current: BTreeMap<Key, Value> = auto_settings::all()
            .iter()
            .filter_map(|s| self.get(s.key).map(|v| (s.key, v)))
            .collect();
        Signals {
            device,
            answers: self.store().answers.clone(),
            // What the learning engine saw of how the app is used.
            usage: grokhub_core::learn::hub::auto_usage(grokhub_core::now_ms()),
            current,
        }
    }

    /// One pass of the engine.
    pub fn pass_on(&mut self, device: DeviceProfile, moment: Moment) {
        let signals = self.signals(device);
        let now = grokhub_core::now_ms();
        let before = self.store().clone();
        let p = plan::plan_pass(&signals, self.store(), moment, now);
        let made: Vec<Change> = p
            .apply
            .iter()
            .filter(|c| self.set(c.key, &c.to))
            .cloned()
            .collect();
        let mut lines = p.took_over;
        lines.extend(plan::apply(self.store(), &made, moment, now));
        // A pass that changed nothing writes nothing.
        if !lines.is_empty() || *self.store() != before {
            self.keep(&lines);
        }
        if moment == Moment::FirstRun {
            self.session.made = made;
            self.session.offered = p.suggest;
            return;
        }
        let changed = made.into_iter().find(|c| c.key.spec().visible);
        let offer = changed.is_none();
        if let Some(c) = changed.or_else(|| p.suggest.into_iter().next()) {
            self.session.note = Some(c);
            self.session.note_offer = offer;
        }
    }

    pub fn act(&mut self, act: Act) -> Option<String> {
        match act {
            Act::View => {}
            Act::Pass { back } => self.look(back),
            Act::Bigger => self.bigger(),
            Act::Start { pace } => self.start(pace),
            Act::Answer { id, value } => self.answer(id, value.as_deref()),
            Act::Finish => self.finish(),
            Act::Undo { key } => {
                self.undo(key);
            }
            Act::Accept { key } => self.accept(key),
            Act::Dismiss { key } => self.dismiss(key),
            Act::Set { key, value } => return Some(self.user_set(key, value)),
            Act::HandBack { key } => {
                self.store().manage_again(key);
                self.keep(&[]);
            }
            Act::HandBackAll => {
                self.store().manage_all_again();
                self.keep(&[]);
            }
            Act::Forget => {
                let _ = auto_store::forget(&self.dir);
                self.session.store = Some(AutoStore {
                    // Forgetting doesn't bring the first-run window back.
                    setup_done: true,
                    ..AutoStore::default()
                });
                self.keep(&[]);
            }
            Act::Chat { text } => return self.chat(&text),
        }
        None
    }

    /// A pass at launch, when the PC changed, or back from a long break.
    /// None while the first-run window is open: it runs its own.
    fn look(&mut self, back: bool) {
        if self.first_run() {
            return;
        }
        let Some(device) = self.device.clone() else {
            return;
        };
        if !self.session.launched {
            self.session.launched = true;
            self.session.generation = self.generation;
            self.pass_on(device, Moment::Launch);
        } else if self.generation != self.session.generation {
            self.session.generation = self.generation;
            self.pass_on(device, Moment::Event);
        } else if back {
            self.pass_on(device, Moment::BackFromIdle);
        }
    }

    // ---- First run --------------------------------------------------------

    fn bigger(&mut self) {
        let from = self.get(Key::TextSize);
        let to = from
            .as_ref()
            .and_then(|v| auto_chat::step_text(v, 1))
            .unwrap_or(Value::Pct(auto_settings::TEXT_STEPS[0]));
        if from.as_ref() == Some(&to) || !self.set(Key::TextSize, &to) {
            return;
        }
        let line = plan::user_set(
            self.store(),
            Key::TextSize,
            from,
            to,
            grokhub_core::now_ms(),
        );
        self.keep(&[line]);
    }

    fn start(&mut self, pace: Pace) {
        let store = self.store();
        store.pace = pace;
        if pace == Pace::Detailed {
            // Tailoring again asks again.
            store.answers.clear();
        }
        self.session.started = true;
        self.session.summary = false;
        self.session.asked.clear();
        self.next_question();
    }

    fn next_question(&mut self) {
        let device = self.device.clone().unwrap_or_default();
        let pace = self.store().pace;
        let signals = self.signals(device.clone());
        match plan::next_question(&signals, &self.session.asked, pace) {
            Some(q) => {
                self.session.question = Some(q.id);
                self.session.shown_at = Some(Instant::now());
            }
            None => {
                self.session.question = None;
                self.pass_on(device, Moment::FirstRun);
                self.session.summary = true;
            }
        }
    }

    fn answer(&mut self, id: Qid, value: Option<&str>) {
        if self.session.question != Some(id) {
            return;
        }
        let ms = self
            .session
            .shown_at
            .map_or(0, |t| t.elapsed().as_millis() as u64);
        let first = self.session.asked.is_empty();
        let store = self.store();
        if first && store.pace == Pace::Detailed {
            store.pace = Pace::from_first(ms, value.is_none());
        }
        if let Some(v) = value {
            store.answers.insert(id, v.to_string());
        }
        self.session.asked.push(id);
        self.session.question = None;
        self.next_question();
    }

    fn finish(&mut self) {
        self.store().setup_done = true;
        self.keep(&[]);
        let s = &mut *self.session;
        s.started = false;
        s.summary = false;
        s.question = None;
        s.made.clear();
        s.offered.clear();
        // The first-run pass counts as this launch's.
        s.launched = true;
        s.generation = self.generation;
    }

    // ---- Undo, yes and not now -------------------------------------------

    /// Put a setting back; it is the person's from then on.
    fn undo(&mut self, key: Key) -> Option<Value> {
        let (back, line) = self.store().undo(key, grokhub_core::now_ms())?;
        self.set(key, &back);
        self.keep(&[line]);
        self.session.made.retain(|c| c.key != key);
        if self.session.note.as_ref().is_some_and(|c| c.key == key) {
            self.session.note = None;
        }
        Some(back)
    }

    fn offer(&mut self, key: Key) -> Option<Change> {
        let s = &mut *self.session;
        if let Some(i) = s.offered.iter().position(|c| c.key == key) {
            return Some(s.offered.remove(i));
        }
        if s.note_offer && s.note.as_ref().is_some_and(|c| c.key == key) {
            return s.note.take();
        }
        None
    }

    fn accept(&mut self, key: Key) {
        let Some(c) = self.offer(key) else {
            return;
        };
        if !self.set(c.key, &c.to) {
            return;
        }
        let line = plan::accept(self.store(), &c, grokhub_core::now_ms());
        self.keep(&[line]);
        self.session.made.push(c);
    }

    fn dismiss(&mut self, key: Key) {
        if let Some(c) = self.offer(key) {
            self.store().dismiss(c.rule, grokhub_core::now_ms());
            self.keep(&[]);
        } else if self.session.note.as_ref().is_some_and(|c| c.key == key) {
            // OK on a change: it stays.
            self.session.note = None;
        }
    }

    // ---- Chat and Settings ------------------------------------------------

    /// "make the text bigger", "dark mode", "undo that", "why is the text so
    /// big?": answered here without a model call.
    fn chat(&mut self, text: &str) -> Option<String> {
        if self.first_run() {
            return None;
        }
        let clock = self.clock_12h();
        Some(match auto_chat::read_setting_ask(text)? {
            SettingAsk::Set(key, to) => self.user_set(key, to),
            SettingAsk::Step(key, dir) => {
                let Some(cur) = self.get(key) else {
                    return Some(cant(key));
                };
                match auto_chat::step_text(&cur, dir) {
                    Some(to) => self.user_set(key, to),
                    None => already(key, &cur, clock),
                }
            }
            SettingAsk::Undo => {
                let key = self.store().last_undoable();
                match key.and_then(|k| self.undo(k).map(|v| (k, v))) {
                    Some((k, back)) => auto_chat::done_reply(k, &back, clock),
                    None => "There's nothing for me to undo.".into(),
                }
            }
            SettingAsk::Why(key) => {
                let now = self.get(key);
                auto_chat::why_reply(self.store(), key, now.as_ref(), clock)
            }
        })
    }

    /// A setting the person asked for: theirs from now on.
    fn user_set(&mut self, key: Key, to: Value) -> String {
        let clock = self.clock_12h();
        let from = self.get(key);
        if from.is_none() {
            return cant(key);
        }
        if from.as_ref() == Some(&to) {
            return already(key, &to, clock);
        }
        if !self.set(key, &to) {
            return cant(key);
        }
        let line = plan::user_set(self.store(), key, from, to.clone(), grokhub_core::now_ms());
        self.keep(&[line]);
        auto_chat::done_reply(key, &to, clock)
    }

    pub fn reply(&mut self, said: Option<String>) -> Reply {
        let clock = self.clock_12h();
        let first_run = self.first_run();
        let line = |c: &Change, offer: bool| Line {
            key: c.key,
            text: if offer {
                auto_settings::offer_line(c.key, &c.to, clock)
            } else {
                auto_settings::change_line(c.key, &c.to, clock)
            },
            reason: c.reason.clone(),
        };
        let question = self.session.question.map(|id| {
            let q = questions::question(id);
            Question {
                id,
                prompt: q.prompt,
                choices: q
                    .choices
                    .iter()
                    .map(|c| Choice {
                        label: c.label,
                        value: c.value,
                    })
                    .collect(),
            }
        });
        // A note shows once; the page keeps it until it's answered.
        let note_offer = self.session.note_offer;
        let note = self.session.note.take().map(|c| {
            if note_offer {
                // Kept for Yes or Not now.
                self.session.note = Some(c.clone());
            }
            Note {
                line: line(&c, note_offer),
                offer: note_offer,
            }
        });
        let mut rows: Vec<Row> = Vec::new();
        let entries: Vec<_> = self
            .store()
            .entries
            .iter()
            .map(|(k, e)| (*k, e.clone()))
            .collect();
        for (key, e) in entries {
            if self.get(key).is_none() {
                continue;
            }
            let why = if e.locked() {
                "You set this.".to_string()
            } else {
                e.reason.clone()
            };
            rows.push(Row {
                key,
                label: key.label(),
                text: format!("{}. {why}", describe(key, &e.applied, clock)),
                undo: e.before.is_some(),
            });
        }
        Reply {
            first_run,
            text_size: self.settings.text_size,
            reduce_motion: self.settings.reduce_motion,
            theme: self.settings.theme,
            question: if first_run { question } else { None },
            summary: first_run && self.session.summary,
            made: self.session.made.iter().map(|c| line(c, false)).collect(),
            offered: self.session.offered.iter().map(|c| line(c, true)).collect(),
            note,
            rows,
            said,
        }
    }
}

fn cant(key: Key) -> String {
    format!("I can't change {} in this app.", lower_first(key.label()))
}

fn already(key: Key, v: &Value, clock: bool) -> String {
    format!(
        "{} is already {}.",
        key.label(),
        lower_first(&describe(key, v, clock))
    )
}

#[cfg(test)]
#[path = "auto_setup_tests.rs"]
mod tests;
