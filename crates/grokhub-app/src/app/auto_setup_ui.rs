//! Auto setup in the cabin. The shared settings list (`autotune::settings`)
//! maps onto `AppConfig` here; the engine runs at launch, on the way back
//! from a long break and when the PC changes, and one note at a time says
//! what it changed (with Undo) or offers a change (with Yes and Not now).
//! Chat asks like "make the text bigger" or "undo that" land here too.
//!
//! It never touches the local model's launch tuning, never turns the local
//! model off on its own, never picks an 18+ voice and never raises approvals
//! past what the first-run answer said.

use std::collections::BTreeMap;
use std::time::Duration;

use grokhub_agent::route::local;
use grokhub_core::autotune::chat::{self as auto_chat, SettingAsk};
use grokhub_core::autotune::plan::{self, Change, Moment};
use grokhub_core::autotune::questions::{Pace, Qid};
use grokhub_core::autotune::rules::{Signals, Usage};
use grokhub_core::autotune::settings::{
    self as auto_settings, describe, lower_first, Key, Target, Value,
};
use grokhub_core::autotune::store::{self as auto_store, AutoStore, JournalLine};
use grokhub_core::autotune::{DeviceProfile, Display, Tier};
use grokhub_core::wire::PermissionMode;

use super::*;

/// Away this long, then back: a good moment for a visible change.
const AWAY_AFTER: Duration = Duration::from_secs(30 * 60);
/// A "changed" note goes away on its own after this.
const NOTE_FOR: Duration = Duration::from_secs(20);

/// The one note on screen.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum AutoNote {
    Changed(Change),
    Suggest(Change),
}

/// Auto setup's state for this launch (kept in the setup wizard's state).
#[derive(Default)]
pub(super) struct AutoSetup {
    /// What it learned and changed (`auto_setup.json`), read once.
    store: Option<AutoStore>,
    launch_done: bool,
    away: bool,
    generation: u64,
    zoom_for: Option<u16>,
    display_sent: bool,
    pub(super) note: Option<(AutoNote, Instant)>,
    /// First run: questions asked so far, the open one and when it showed.
    pub(super) asked: Vec<Qid>,
    pub(super) question: Option<Qid>,
    shown_at: Option<Instant>,
    /// First run: what it set and what it offers, for the summary.
    pub(super) made: Vec<Change>,
    pub(super) offered: Vec<Change>,
}

fn hour_of(hm: &str) -> Option<u8> {
    hm.split(':')
        .next()?
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|h| *h < 24)
}

/// `system`, `light`, or `dark` for every dark variant.
fn theme_word(theme: &str) -> &'static str {
    match grokhub_core::parse_theme(theme) {
        grokhub_core::ThemeChoice::Light => "light",
        grokhub_core::ThemeChoice::System => "system",
        _ => "dark",
    }
}

fn notify_word(reminders: bool, offers: bool) -> &'static str {
    match (reminders, offers) {
        (true, _) => "all",
        (false, true) => "important",
        (false, false) => "quiet",
    }
}

/// The three voices auto setup may pick between; any other id is the user's.
fn talk_word(personality: &str) -> &str {
    match personality {
        "" | "nyx" => "friendly",
        "concise-pro" => "direct",
        "sarcastic-sidekick" => "playful",
        other => other,
    }
}

fn talk_id(word: &str) -> Option<&'static str> {
    match word {
        "friendly" => Some("nyx"),
        "direct" => Some("concise-pro"),
        "playful" => Some("sarcastic-sidekick"),
        _ => None,
    }
}

impl Cabin {
    /// The tone the Talk answer (or the default voice) stands for, where Get
    /// to know you's soul step starts. Empty for a voice the person made.
    pub(super) fn auto_talk_tone(&self) -> &'static str {
        grokhub_core::onboard_identity::tone_for_personality(&self.cfg.personality)
    }
}

/// The OS shows 12-hour times (US default until the PC is read).
fn clock_12h() -> bool {
    grokhub_agent::device::current().is_none_or(|d| d.clock_12h)
}

/// "Turn on game companion?", "Set notifications to only important?".
pub(super) fn offer_line(c: &Change) -> String {
    auto_settings::offer_line(c.key, &c.to, clock_12h())
}

/// "Text size: 130%".
pub(super) fn change_line(c: &Change) -> String {
    auto_settings::change_line(c.key, &c.to, clock_12h())
}

/// The PC can run a model by itself, so the summary offers to set one up.
pub(super) fn fits_local_ai(d: &DeviceProfile) -> bool {
    d.tier() >= Tier::T2 && d.free_disk_mb >= 20_000
}

impl Target for Cabin {
    fn get(&self, key: Key) -> Option<Value> {
        let c = &self.cfg;
        Some(match key {
            Key::TextSize => Value::Pct(c.text_size),
            Key::ReduceMotion => Value::Bool(c.reduce_motion),
            Key::Theme => Value::pick(theme_word(&c.theme)),
            Key::Effects => Value::Bool(c.imagine_wall || c.composer_glow),
            Key::CloseToTray => Value::Bool(c.close_to_tray),
            Key::LocalModel => Value::Bool(c.local_model),
            Key::PromptCleanup => Value::Bool(c.prompt_cleanup),
            Key::QuietHours => Value::Hours(hour_of(&c.quiet_start)?, hour_of(&c.quiet_end)?),
            Key::DreamHour => Value::Hour(self.dream_hour() as u8),
            Key::Notifications => {
                Value::pick(notify_word(c.proactive_reminders, c.ask_offers.enabled))
            }
            Key::GameCompanion => Value::Bool(c.game_companion),
            Key::GameNudges => {
                Value::Bool(c.nudge_watchers.steam_patch && c.nudge_watchers.gpu_driver)
            }
            Key::ResumeCode => Value::Bool(c.resume_code),
            Key::ChatObjectives => Value::Bool(c.chat_objectives),
            Key::DockPanels => Value::Bool(c.dock_panels),
            Key::CollapseThoughts => Value::Bool(c.always_collapse_thoughts),
            Key::Suggestions => Value::Bool(c.suggestions),
            Key::Personality => Value::pick(talk_word(&c.personality)),
            Key::Approvals => match PermissionMode::parse(&c.permission_mode) {
                Some(PermissionMode::Auto) => Value::pick("risky"),
                Some(PermissionMode::AlwaysApprove) => Value::pick("always"),
                _ => Value::pick("ask"),
            },
        })
    }

    fn set(&mut self, key: Key, value: &Value) -> bool {
        match (key, value) {
            (Key::TextSize, Value::Pct(p)) => {
                let p = auto_settings::text_step_for(*p);
                if self.cfg.text_size != p {
                    self.cfg.text_size = p;
                    self.persist_cfg();
                }
            }
            (Key::ReduceMotion, Value::Bool(on)) => self.set_reduce_motion(*on),
            (Key::Theme, Value::Pick(t)) => match t.as_str() {
                "light" => self.choose_theme(grokhub_core::ThemeChoice::Light),
                "system" => self.choose_theme(grokhub_core::ThemeChoice::System),
                // Any dark variant already counts as dark.
                "dark" if theme_word(&self.cfg.theme) == "dark" => {}
                "dark" => self.choose_theme(grokhub_core::ThemeChoice::OpsDark),
                _ => return false,
            },
            (Key::Effects, Value::Bool(on)) => {
                self.set_living_wall(*on);
                if !on && self.cfg.composer_glow {
                    self.cfg.composer_glow = false;
                    self.persist_cfg();
                }
            }
            (Key::CloseToTray, Value::Bool(on)) => self.set_close_to_tray(*on),
            // On only, and only with a model already here: turning it on
            // without one would start a download nobody asked for.
            (Key::LocalModel, Value::Bool(true)) if local::installed() => {
                self.set_local_model(true)
            }
            (Key::PromptCleanup, Value::Bool(on)) => self.set_prompt_cleanup(*on),
            (Key::QuietHours, Value::Hours(a, b)) if *a < 24 && *b < 24 => {
                self.cfg.quiet_start = format!("{a:02}:00");
                self.cfg.quiet_end = format!("{b:02}:00");
                self.quiet_start_buf = self.cfg.quiet_start.clone();
                self.quiet_end_buf = self.cfg.quiet_end.clone();
                self.persist_cfg();
            }
            (Key::DreamHour, Value::Hour(h)) if *h < 24 => {
                self.cfg.dream_hour = u32::from(*h);
                self.persist_cfg();
            }
            (Key::Notifications, Value::Pick(n)) => {
                let (reminders, offers) = match n.as_str() {
                    "all" => (true, true),
                    "important" => (false, true),
                    "quiet" => (false, false),
                    _ => return false,
                };
                self.set_proactive_reminders(reminders);
                if self.cfg.ask_offers.enabled != offers {
                    self.cfg.ask_offers.enabled = offers;
                    self.persist_cfg();
                }
            }
            (Key::GameCompanion, Value::Bool(on)) => self.set_game_companion(*on),
            (Key::GameNudges, Value::Bool(on)) => {
                self.cfg.nudge_watchers.steam_patch = *on;
                self.cfg.nudge_watchers.gpu_driver = *on;
                self.persist_cfg();
            }
            (Key::ResumeCode, Value::Bool(on)) => self.set_resume_code(*on),
            (Key::ChatObjectives, Value::Bool(on)) => {
                self.cfg.chat_objectives = *on;
                self.persist_cfg();
            }
            (Key::DockPanels, Value::Bool(on)) => self.set_dock_panels(*on),
            (Key::CollapseThoughts, Value::Bool(on)) => {
                self.cfg.always_collapse_thoughts = *on;
                self.persist_cfg();
            }
            (Key::Suggestions, Value::Bool(on)) => {
                self.cfg.suggestions = *on;
                self.chip_paint_key.clear();
                self.persist_cfg();
            }
            (Key::Personality, Value::Pick(word)) => {
                // A voice the user picked (an 18+ one included) stays theirs.
                if talk_id(talk_word(&self.cfg.personality)).is_none() {
                    return false;
                }
                let Some(id) = talk_id(word) else {
                    return false;
                };
                self.set_default_personality(id);
            }
            (Key::Approvals, Value::Pick(a)) => {
                let mode = match a.as_str() {
                    "ask" => PermissionMode::Ask,
                    "risky" => PermissionMode::Auto,
                    _ => return false,
                };
                // "Always approve" was the user's own pick: leave it.
                if PermissionMode::parse(&self.cfg.permission_mode)
                    == Some(PermissionMode::AlwaysApprove)
                {
                    return false;
                }
                self.set_permission_mode(mode);
            }
            _ => return false,
        }
        true
    }
}

impl Cabin {
    fn auto_store(&mut self) -> &mut AutoStore {
        self.setup
            .auto
            .store
            .get_or_insert_with(|| auto_store::load(&crate::config::config_dir()))
    }

    /// Keep the store and its history lines, written off the UI thread.
    fn auto_keep(&mut self, lines: Vec<JournalLine>) {
        let copy = self.auto_store().clone();
        let dir = crate::config::config_dir();
        std::thread::spawn(move || {
            let _ = auto_store::save(&dir, &copy);
            if !lines.is_empty() {
                let _ = auto_store::append_journal(&dir, &lines);
            }
        });
    }

    fn auto_current(&self) -> BTreeMap<Key, Value> {
        auto_settings::all()
            .iter()
            .filter_map(|s| self.get(s.key).map(|v| (s.key, v)))
            .collect()
    }

    fn auto_usage(&mut self) -> Usage {
        let rhythm = self.rhythm_now();
        let rhythm_quiet = rhythm
            .enough()
            .then(|| rhythm.suggested_quiet())
            .flatten()
            .and_then(|(a, b)| Some((u8::try_from(a).ok()?, u8::try_from(b).ok()?)));
        // The rest comes from the learning engine, the same in both apps.
        Usage {
            rhythm_quiet,
            ..grokhub_core::learn::hub::auto_usage(grokhub_core::now_ms())
        }
    }

    pub(super) fn auto_signals(&mut self, device: DeviceProfile) -> Signals {
        let usage = self.auto_usage();
        let answers = self.auto_store().answers.clone();
        Signals {
            device,
            answers,
            usage,
            current: self.auto_current(),
        }
    }

    /// Make the changes this config accepts; the ones it took come back.
    fn auto_make(&mut self, changes: &[Change]) -> Vec<Change> {
        let status = self.status.clone();
        let made = changes
            .iter()
            .filter(|c| self.set(c.key, &c.to))
            .cloned()
            .collect();
        self.status = status;
        made
    }

    /// One pass of the engine on `device`.
    pub(super) fn auto_pass_on(&mut self, device: DeviceProfile, moment: Moment) {
        let signals = self.auto_signals(device);
        let now = grokhub_core::now_ms();
        let before = self.auto_store().clone();
        let p = plan::plan_pass(&signals, self.auto_store(), moment, now);
        let made = self.auto_make(&p.apply);
        let mut lines = p.took_over;
        lines.extend(plan::apply(self.auto_store(), &made, moment, now));
        // A pass that changed nothing writes nothing.
        if !lines.is_empty() || *self.auto_store() != before {
            self.auto_keep(lines);
        }
        if moment == Moment::FirstRun {
            self.setup.auto.made = made;
            self.setup.auto.offered = p.suggest;
            return;
        }
        let note = made
            .into_iter()
            .find(|c| c.key.spec().visible)
            .map(AutoNote::Changed)
            .or_else(|| p.suggest.into_iter().next().map(AutoNote::Suggest));
        if let Some(note) = note {
            self.setup.auto.note = Some((note, Instant::now()));
        }
    }

    /// Every frame, from the setup wizard's paint: text size, the screen,
    /// and a pass at launch, back from a break, or when the PC changed.
    pub(super) fn tick_auto_setup(&mut self, ctx: &egui::Context) {
        let want = self.cfg.text_size;
        if self.setup.auto.zoom_for != Some(want) {
            ctx.set_zoom_factor(f32::from(want) / 100.0);
            self.setup.auto.zoom_for = Some(want);
        }
        let Some(device) = grokhub_agent::device::current() else {
            return;
        };
        if !self.setup.auto.display_sent {
            self.setup.auto.display_sent = true;
            let (size, ppp) = ctx.input(|i| (i.viewport().monitor_size, i.pixels_per_point()));
            if let Some(size) = size {
                grokhub_agent::device::set_display(Display {
                    scale_pct: (ppp * 100.0).round() as u16,
                    width_px: (size.x * ppp).round() as u32,
                    monitors: 1,
                });
            }
        }
        // Waiting on the PC read for the first question.
        if self.setup.step == Some(super::setup_wizard::SetupStep::Questions)
            && self.setup.auto.question.is_none()
        {
            self.auto_next_question(device);
            return;
        }
        // Past first run: the launch check ran and the setup window is shut
        // (a profile with chats never sees first run, and still gets tuned).
        if !self.setup.checked || self.setup.step.is_some() {
            return;
        }
        let generation = grokhub_agent::device::generation();
        if !self.setup.auto.launch_done {
            self.setup.auto.launch_done = true;
            self.setup.auto.generation = generation;
            self.auto_pass_on(device, Moment::Launch);
        } else if generation != self.setup.auto.generation {
            self.setup.auto.generation = generation;
            self.auto_pass_on(device, Moment::Event);
        } else if self.last_activity.elapsed() > AWAY_AFTER {
            self.setup.auto.away = true;
        } else if self.setup.auto.away {
            self.setup.auto.away = false;
            self.auto_pass_on(device, Moment::BackFromIdle);
        }
        if let Some((AutoNote::Changed(_), at)) = &self.setup.auto.note {
            if at.elapsed() > NOTE_FOR {
                self.setup.auto.note = None;
            }
        }
    }

    // ---- First run --------------------------------------------------------

    /// "Just set it up" or "Tailor it": ask what's worth asking, then the summary.
    pub(super) fn auto_start_questions(&mut self, pace: Pace) {
        let store = self.auto_store();
        store.pace = pace;
        if pace == Pace::Detailed {
            // Tailoring again asks again.
            store.answers.clear();
        }
        self.setup.auto.asked.clear();
        self.setup.auto.question = None;
        self.setup.step = Some(super::setup_wizard::SetupStep::Questions);
        if let Some(device) = grokhub_agent::device::current() {
            self.auto_next_question(device);
        }
    }

    pub(super) fn auto_next_question(&mut self, device: DeviceProfile) {
        let pace = self.auto_store().pace;
        let signals = self.auto_signals(device.clone());
        match plan::next_question(&signals, &self.setup.auto.asked, pace) {
            Some(q) => {
                self.setup.auto.question = Some(q.id);
                self.setup.auto.shown_at = Some(Instant::now());
            }
            None => {
                self.setup.auto.question = None;
                self.auto_pass_on(device, Moment::FirstRun);
                self.setup.step = Some(super::setup_wizard::SetupStep::Summary);
            }
        }
    }

    /// A tap on a choice, or Skip (`None`), then the next question.
    pub(super) fn auto_answer(&mut self, id: Qid, value: Option<&str>) {
        self.auto_record_answer(id, value);
        if let Some(device) = grokhub_agent::device::current() {
            self.auto_next_question(device);
        }
    }

    pub(super) fn auto_record_answer(&mut self, id: Qid, value: Option<&str>) {
        let ms = self
            .setup
            .auto
            .shown_at
            .map_or(0, |t| t.elapsed().as_millis() as u64);
        let first = self.setup.auto.asked.is_empty();
        let store = self.auto_store();
        if first && store.pace == Pace::Detailed {
            store.pace = Pace::from_first(ms, value.is_none());
        }
        if let Some(v) = value {
            store.answers.insert(id, v.to_string());
        }
        self.setup.auto.asked.push(id);
        self.setup.auto.question = None;
    }

    /// The big first-screen button: one text-size step up per press.
    pub(super) fn auto_bigger(&mut self) {
        let from = self.get(Key::TextSize);
        let to = from
            .as_ref()
            .and_then(|v| auto_chat::step_text(v, 1))
            .unwrap_or(Value::Pct(auto_settings::TEXT_STEPS[0]));
        if from.as_ref() == Some(&to) || !self.set(Key::TextSize, &to) {
            return;
        }
        let line = plan::user_set(
            self.auto_store(),
            Key::TextSize,
            from,
            to,
            grokhub_core::now_ms(),
        );
        self.auto_keep(vec![line]);
    }

    pub(super) fn auto_finish_first_run(&mut self) {
        self.auto_store().setup_done = true;
        self.auto_keep(Vec::new());
        self.setup.auto.made.clear();
        self.setup.auto.offered.clear();
        self.setup.auto.launch_done = true;
        self.setup.auto.generation = grokhub_agent::device::generation();
    }

    // ---- Undo, yes and not now -------------------------------------------

    /// Put a setting back; it is the user's from then on.
    pub(super) fn auto_undo(&mut self, key: Key) -> Option<Value> {
        let (back, line) = self.auto_store().undo(key, grokhub_core::now_ms())?;
        let status = self.status.clone();
        // LocalModel never changes here, so an undo of it only records.
        if key != Key::LocalModel {
            self.set(key, &back);
        }
        self.status = status;
        self.auto_keep(vec![line]);
        self.setup.auto.made.retain(|c| c.key != key);
        Some(back)
    }

    /// Yes to an offer. The local model opens its setup step instead when
    /// no model is here yet.
    pub(super) fn auto_accept(&mut self, c: &Change) {
        if c.key == Key::LocalModel && !local::installed() {
            self.open_setup(super::setup_wizard::SetupStep::LocalAi);
            return;
        }
        if !self.set(c.key, &c.to) {
            return;
        }
        let line = plan::accept(self.auto_store(), c, grokhub_core::now_ms());
        self.auto_keep(vec![line]);
        self.setup.auto.offered.retain(|o| o.key != c.key);
        self.setup.auto.made.push(c.clone());
    }

    pub(super) fn auto_dismiss(&mut self, c: &Change) {
        self.auto_store().dismiss(c.rule, grokhub_core::now_ms());
        self.auto_keep(Vec::new());
        self.setup.auto.offered.retain(|o| o.key != c.key);
    }

    /// The note in the corner: what changed with Undo, or an offer.
    pub(super) fn paint_auto_note(&mut self, ctx: &egui::Context) {
        let Some((note, _)) = self.setup.auto.note.clone() else {
            return;
        };
        let mut act: Option<bool> = None;
        egui::Area::new(egui::Id::new("auto-setup-note"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border_strong()))
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_max_width(340.0);
                        let (title, reason, yes, no) = match &note {
                            AutoNote::Changed(c) => (change_line(c), &c.reason, "Undo", "OK"),
                            AutoNote::Suggest(c) => (offer_line(c), &c.reason, "Yes", "Not now"),
                        };
                        ui.label(
                            RichText::new(title)
                                .size(crate::theme::FONT_BODY)
                                .color(crate::theme::fg()),
                        );
                        ui.label(
                            RichText::new(reason.as_str())
                                .size(crate::theme::FONT_BODY)
                                .color(crate::theme::muted()),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if crate::cards::white_pill(ui, yes) {
                                act = Some(true);
                            }
                            if crate::cards::ghost_pill(ui, no) {
                                act = Some(false);
                            }
                        });
                    });
            });
        let Some(yes) = act else {
            return;
        };
        self.setup.auto.note = None;
        match (note, yes) {
            (AutoNote::Changed(c), true) => {
                self.auto_undo(c.key);
            }
            (AutoNote::Suggest(c), true) => self.auto_accept(&c),
            (AutoNote::Suggest(c), false) => self.auto_dismiss(&c),
            (AutoNote::Changed(_), false) => {}
        }
    }

    // ---- Chat -------------------------------------------------------------

    /// "make the text bigger", "dark mode", "undo that", "why is the text so
    /// big?": answered here without a model call.
    pub(super) fn try_setting_ask(&mut self, text: &str) -> bool {
        if self.running {
            return false;
        }
        let Some(ask) = auto_chat::read_setting_ask(text) else {
            return false;
        };
        let reply = self.answer_setting_ask(ask);
        self.touch();
        self.live_mut().push(("user".into(), text.to_string()));
        self.live_mut().push(("assistant".into(), reply));
        self.stamp_current_access();
        self.persist();
        true
    }

    pub(super) fn answer_setting_ask(&mut self, ask: SettingAsk) -> String {
        let clock = clock_12h();
        match ask {
            SettingAsk::Set(key, to) => self.chat_set(key, to),
            SettingAsk::Step(key, dir) => {
                let Some(cur) = self.get(key) else {
                    return format!("I can't change {} here.", lower_first(key.label()));
                };
                match auto_chat::step_text(&cur, dir) {
                    Some(to) => self.chat_set(key, to),
                    None => format!(
                        "{} is already {}.",
                        key.label(),
                        lower_first(&describe(key, &cur, clock))
                    ),
                }
            }
            SettingAsk::Undo => {
                let key = self.auto_store().last_undoable();
                match key.and_then(|k| self.auto_undo(k).map(|v| (k, v))) {
                    Some((k, back)) => auto_chat::done_reply(k, &back, clock),
                    None => "There's nothing for me to undo.".into(),
                }
            }
            SettingAsk::Why(key) => {
                let now = self.get(key);
                auto_chat::why_reply(self.auto_store(), key, now.as_ref(), clock)
            }
        }
    }

    /// A setting asked for in chat: the user's from now on.
    fn chat_set(&mut self, key: Key, to: Value) -> String {
        let clock = clock_12h();
        let from = self.get(key);
        if from.as_ref() == Some(&to) {
            return format!(
                "{} is already {}.",
                key.label(),
                lower_first(&describe(key, &to, clock))
            );
        }
        let set = match (key, &to) {
            (Key::LocalModel, Value::Bool(true)) if !local::installed() => {
                self.open_setup(super::setup_wizard::SetupStep::LocalAi);
                return "There's no model on this PC yet, so I opened its setup.".into();
            }
            // The user's own ask may turn it off; auto setup never does.
            (Key::LocalModel, Value::Bool(false)) => {
                self.set_local_model(false);
                true
            }
            _ => self.set(key, &to),
        };
        if !set {
            return format!("I can't change {} here.", lower_first(key.label()));
        }
        let line = plan::user_set(
            self.auto_store(),
            key,
            from,
            to.clone(),
            grokhub_core::now_ms(),
        );
        self.auto_keep(vec![line]);
        auto_chat::done_reply(key, &to, clock)
    }

    // ---- Settings ---------------------------------------------------------

    /// Settings → Appearance: text and controls size. A pick here is the
    /// user's, so auto setup leaves it alone from then on.
    pub(super) fn ui_text_size_row(&mut self, ui: &mut egui::Ui) {
        let steps: Vec<String> = auto_settings::TEXT_STEPS
            .iter()
            .map(|p| format!("{p}%"))
            .collect();
        let now = format!("{}%", self.cfg.text_size);
        if let Some(i) = crate::cards::settings_dropdown(
            ui,
            "Text size",
            "Makes text and buttons bigger everywhere.",
            &now,
            &steps,
        ) {
            let to = Value::Pct(auto_settings::TEXT_STEPS[i.min(steps.len() - 1)]);
            let reply = self.chat_set(Key::TextSize, to);
            self.status = reply;
        }
    }

    /// Settings → Your PC → Automations & limits: what auto setup changed,
    /// with Undo, and the ways to hand settings back or start over.
    pub(super) fn ui_auto_setup_rows(&mut self, ui: &mut egui::Ui) {
        let clock = clock_12h();
        let product = grokhub_core::brand::product_name();
        let mut rows: Vec<(Key, String, bool)> = self
            .auto_store()
            .entries
            .iter()
            .map(|(k, e)| {
                let what = describe(*k, &e.applied, clock);
                let undo = e.before.is_some();
                let why = if e.locked() {
                    "You set this.".to_string()
                } else {
                    e.reason.clone()
                };
                (*k, format!("{what}. {why}"), undo)
            })
            .collect();
        rows.sort_by_key(|(k, _, _)| *k);
        crate::cards::settings_note(
            ui,
            &format!("Auto setup: {product} sets these for this PC and you, and keeps tuning them. Undo puts one back and leaves it to you."),
        );
        let mut undo = None;
        let mut hand_back = None;
        for (key, desc, can_undo) in &rows {
            let button = if *can_undo { "Undo" } else { "Hand back" };
            if crate::cards::settings_action(ui, key.label(), desc, button) {
                if *can_undo {
                    undo = Some(*key);
                } else {
                    hand_back = Some(*key);
                }
            }
        }
        if let Some(key) = undo {
            self.auto_undo(key);
        }
        if let Some(key) = hand_back {
            self.auto_store().manage_again(key);
            self.auto_keep(Vec::new());
            self.status = format!("{} is {product}'s to tune again.", key.label());
        }
        if crate::cards::settings_action(
            ui,
            "Hand everything back",
            &format!("Let {product} tune every setting you changed yourself again."),
            "Hand back",
        ) {
            self.auto_store().manage_all_again();
            self.auto_keep(Vec::new());
            self.status = format!("{product} tunes every setting again.");
        }
        if crate::cards::settings_action(
            ui,
            "Forget what you learned",
            "Clears the answers, the change history and what you said no to. Settings stay as they are.",
            "Forget",
        ) {
            let _ = auto_store::forget(&crate::config::config_dir());
            self.setup.auto.store = Some(AutoStore::default());
            self.status = "Forgot what auto setup learned".into();
        }
    }
}

#[cfg(test)]
#[path = "auto_setup_ui_tests.rs"]
mod tests;
