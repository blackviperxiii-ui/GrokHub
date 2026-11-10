//! Activity Dock in the cabin chrome: running turns, approvals waiting, and
//! scheduled automation runs — same model as `ui/src/activity.ts` / ActivityDock.tsx.

use super::*;
use grokhub_core::BgOrigin;
use std::collections::HashMap;

/// Done and failed items stay this long, then leave the dock.
pub(super) const DONE_TTL_MS: u64 = 10 * 60 * 1000;
/// The dock lists at most this many items; the rest are counted.
pub(super) const DOCK_MAX: usize = 6;
pub(super) const CHAT_TITLE: &str = "Chat reply";
const DETAIL_MAX: usize = 90;
/// A queued message's first line in its row.
const QUEUED_TITLE_MAX: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivityStatus {
    Running,
    Waiting,
    Done,
    Failed,
    /// Lined up behind the live reply: a queued message or side question.
    Queued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivityKind {
    Chat,
    Routine,
    /// A `/bg` run you started; key `bg:{run id}`.
    Background,
    /// A message typed while a reply ran; key `queue:{index}`.
    QueuedMessage,
    /// A side question waiting for the reply; key `side:{index}`.
    SideQuestion,
    /// A local model download; key `dl:model`.
    Download,
    /// The screen recording; key `rec:screen`.
    Recording,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActivityItem {
    /// `chat:<thread>` or `run:<automation id>`.
    pub key: String,
    pub kind: ActivityKind,
    pub title: String,
    pub status: ActivityStatus,
    pub detail: String,
    pub steps: usize,
    /// When the item last changed (ms).
    pub at: u64,
}

#[derive(Default)]
pub(super) struct DockState {
    pub open: bool,
    /// Finished rows kept for [`DONE_TTL_MS`].
    pub recent: Vec<ActivityItem>,
}

fn clip(text: &str) -> String {
    clip_to(text, DETAIL_MAX)
}

fn clip_to(text: &str, max: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > max {
        let cut: String = one.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    } else {
        one
    }
}

fn status_word(status: ActivityStatus) -> &'static str {
    match status {
        ActivityStatus::Running => "Running",
        ActivityStatus::Waiting => "Waiting on you",
        ActivityStatus::Done => "Done",
        ActivityStatus::Failed => "Failed",
        ActivityStatus::Queued => "Queued",
    }
}

fn status_rank(status: ActivityStatus) -> u8 {
    match status {
        ActivityStatus::Waiting => 0,
        ActivityStatus::Running => 1,
        ActivityStatus::Queued => 2,
        ActivityStatus::Failed => 3,
        ActivityStatus::Done => 4,
    }
}

/// What the dock shows: waiting first, then running, then recent results;
/// finished items older than `DONE_TTL_MS` drop.
pub(super) fn dock_items(items: &[ActivityItem], now: u64) -> Vec<ActivityItem> {
    let mut out: Vec<ActivityItem> = items
        .iter()
        .filter(|i| {
            !matches!(i.status, ActivityStatus::Done | ActivityStatus::Failed)
                || now.saturating_sub(i.at) < DONE_TTL_MS
        })
        .cloned()
        .collect();
    out.sort_by(|a, b| {
        status_rank(a.status)
            .cmp(&status_rank(b.status))
            .then(b.at.cmp(&a.at))
    });
    out
}

/// The collapsed dock's line, e.g. "1 waiting · 2 running". With anything
/// queued the running part counts the line: "1 of 3 running".
pub(super) fn dock_summary(items: &[ActivityItem]) -> String {
    let count = |s: ActivityStatus| items.iter().filter(|i| i.status == s).count();
    let running = count(ActivityStatus::Running);
    let queued = count(ActivityStatus::Queued);
    let line = match (running, queued) {
        (0, 0) => String::new(),
        (n, 0) => format!("{n} running"),
        (0, q) => format!("{q} queued"),
        (n, q) => format!("{n} of {} running", n + q),
    };
    let waiting = count(ActivityStatus::Waiting);
    let failed = count(ActivityStatus::Failed);
    let done = count(ActivityStatus::Done);
    [
        (waiting > 0).then(|| format!("{waiting} waiting")),
        (!line.is_empty()).then_some(line),
        (failed > 0).then(|| format!("{failed} failed")),
        (done > 0).then(|| format!("{done} done")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

/// A queued row's place in line, in plain words.
pub(super) fn queue_place(i: usize) -> String {
    match i {
        0 => "Next up".into(),
        1 => "2nd in line".into(),
        2 => "3rd in line".into(),
        n => format!("{}th in line", n + 1),
    }
}

/// The detail of a queued row typed in a chat other than the running one.
pub(super) fn waits_for_line(title: Option<&str>) -> String {
    match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => format!("Waits for {}", clip_to(t, QUEUED_TITLE_MAX)),
        None => "Waits for its chat".into(),
    }
}

/// The rows for messages typed during a reply, in order. `at` falls by one
/// per place so newest-first sorting keeps the line in order.
pub(super) fn queued_items(
    queue: &[String],
    kind: ActivityKind,
    prefix: &str,
    now: u64,
) -> Vec<ActivityItem> {
    queue
        .iter()
        .enumerate()
        .map(|(i, text)| ActivityItem {
            key: format!("{prefix}:{i}"),
            kind,
            title: clip_to(text.lines().next().unwrap_or(""), QUEUED_TITLE_MAX),
            status: ActivityStatus::Queued,
            detail: queue_place(i),
            steps: 0,
            at: now.saturating_sub(i as u64),
        })
        .collect()
}

/// Live side answers, keyed by run id.
pub(super) fn side_items(runs: &[super::side_run::SideRun], now: u64) -> Vec<ActivityItem> {
    runs.iter()
        .filter(|r| r.live())
        .map(|r| ActivityItem {
            key: format!("side:{}", r.id),
            kind: ActivityKind::SideQuestion,
            title: clip_to(r.question.lines().next().unwrap_or(""), QUEUED_TITLE_MAX),
            status: ActivityStatus::Running,
            detail: "Answering on the side".into(),
            steps: 0,
            at: now,
        })
        .collect()
}

/// The progress line under a running chat.
pub(super) fn steps_line(item: &ActivityItem) -> String {
    if item.kind != ActivityKind::Chat || item.steps == 0 {
        return String::new();
    }
    if item.steps == 1 {
        "1 step done".into()
    } else {
        format!("{} steps done", item.steps)
    }
}

pub(super) fn routine_started(id: &str, name: &str, now: u64) -> ActivityItem {
    ActivityItem {
        key: format!("run:{id}"),
        kind: ActivityKind::Routine,
        title: name.to_string(),
        status: ActivityStatus::Running,
        detail: "Running now…".into(),
        steps: 0,
        at: now,
    }
}

pub(super) fn routine_finished(item: ActivityItem, ok: bool, text: &str, now: u64) -> ActivityItem {
    ActivityItem {
        status: if ok {
            ActivityStatus::Done
        } else {
            ActivityStatus::Failed
        },
        detail: clip(text),
        at: now,
        ..item
    }
}

/// The chat an item opens: `chat:{id}` items name it, a background run's
/// item finds its run.
fn item_chat_id(runs: &[super::background::BgRun], item: &ActivityItem) -> String {
    if let Some(id) = item.key.strip_prefix("chat:") {
        return id.to_string();
    }
    item.key
        .strip_prefix("bg:")
        .and_then(|id| runs.iter().find(|r| r.id.to_string() == id))
        .map(|r| r.thread_id.clone())
        .unwrap_or_default()
}

/// The card a `bg:` row's run works, if it has one.
fn item_run_card(bg: &super::background::BgWork, item: &ActivityItem) -> Option<String> {
    let run: u64 = item.key.strip_prefix("bg:")?.parse().ok()?;
    bg.cards
        .iter()
        .find(|(r, _)| *r == run)
        .map(|(_, id)| id.clone())
}

/// What a click in the Activity list asks for. Applied after painting.
enum DockAct {
    Open {
        kind: ActivityKind,
        chat_id: String,
        /// The card a background run works; Open goes there.
        card: Option<String>,
        waiting: bool,
    },
    SteerQueued(usize),
    DropQueued(usize),
    DropSide(usize),
    StopRun(u64),
    StopRecording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowPick {
    Open,
    SteerNow,
    Remove,
    Stop,
}

/// A queued row's place from its `queue:{i}` / `side:{i}` key.
fn queue_index(key: &str) -> usize {
    key.split_once(':')
        .and_then(|(_, i)| i.parse().ok())
        .unwrap_or(0)
}

/// The buttons under one Activity row, left to right.
pub(super) fn row_buttons(item: &ActivityItem, steerable: bool) -> Vec<(&'static str, RowPick)> {
    match (item.kind, item.status) {
        (ActivityKind::QueuedMessage, _) if steerable => {
            vec![
                ("Steer now", RowPick::SteerNow),
                ("Remove", RowPick::Remove),
            ]
        }
        (ActivityKind::QueuedMessage, _) => vec![("Remove", RowPick::Remove)],
        (ActivityKind::SideQuestion, _) => vec![("Stop", RowPick::Remove)],
        (ActivityKind::Recording, _) => vec![("Stop", RowPick::Stop)],
        (ActivityKind::Download, _) => vec![("Open", RowPick::Open)],
        (ActivityKind::Chat, ActivityStatus::Waiting) => vec![("Review", RowPick::Open)],
        (ActivityKind::Chat, _) => vec![("Open chat", RowPick::Open)],
        (ActivityKind::Routine, _) => vec![("Open result", RowPick::Open)],
        (ActivityKind::Background, ActivityStatus::Running) => {
            vec![("Open card", RowPick::Open), ("Stop", RowPick::Stop)]
        }
        (ActivityKind::Background, _) => vec![("Open card", RowPick::Open)],
    }
}

fn upsert(map: &mut HashMap<String, ActivityItem>, item: ActivityItem) {
    map.insert(item.key.clone(), item);
}

fn activity_chat_title(cabin: &Cabin, chat_id: &str) -> String {
    cabin
        .threads
        .iter()
        .find(|t| t.id == chat_id)
        .map(|t| t.title.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "this chat".into())
}

impl Cabin {
    /// Remember a finished chat turn for the dock TTL window.
    pub(super) fn note_activity_chat_end(&mut self, ok: bool, detail: &str) {
        let Some(id) = self.chat_job_thread.clone() else {
            return;
        };
        let now = now_ms();
        let steps = self
            .tool_cards
            .iter()
            .filter(|c| c.status == "completed" || c.status == "failed")
            .count();
        let item = ActivityItem {
            key: format!("chat:{id}"),
            kind: ActivityKind::Chat,
            title: CHAT_TITLE.into(),
            status: if ok {
                ActivityStatus::Done
            } else {
                ActivityStatus::Failed
            },
            detail: if ok {
                "Reply finished".into()
            } else {
                clip(detail)
            },
            steps,
            at: now,
        };
        self.activity
            .recent
            .retain(|i| i.key != item.key || now.saturating_sub(i.at) < DONE_TTL_MS);
        self.activity.recent.push(item);
    }

    /// Remember a finished scheduled automation for the dock.
    pub(super) fn note_activity_routine_end(
        &mut self,
        id: &str,
        title: &str,
        ok: bool,
        detail: &str,
    ) {
        let now = now_ms();
        let start = routine_started(id, title, now);
        let item = routine_finished(start, ok, detail, now);
        self.activity.recent.retain(|i| i.key != item.key);
        self.activity.recent.push(item);
    }

    /// Live + recent items for the dock, sorted like the Tauri shell.
    pub(super) fn activity_items(&self) -> Vec<ActivityItem> {
        let now = now_ms();
        let mut map: HashMap<String, ActivityItem> = HashMap::new();
        for item in &self.activity.recent {
            if matches!(item.status, ActivityStatus::Done | ActivityStatus::Failed)
                && now.saturating_sub(item.at) < DONE_TTL_MS
            {
                upsert(&mut map, item.clone());
            }
        }

        let turn_id = self
            .chat_job_thread
            .clone()
            .unwrap_or_else(|| self.visible_thread_id());
        let live_turn = self.running
            || self.perm_ask.is_some()
            || self.elicit_ask.is_some()
            || !self.perm_queue.is_empty();
        if live_turn {
            let waiting =
                self.perm_ask.is_some() || self.elicit_ask.is_some() || !self.perm_queue.is_empty();
            let steps = self
                .tool_cards
                .iter()
                .filter(|c| c.status == "completed" || c.status == "failed")
                .count();
            let detail = if let Some(p) = &self.perm_ask {
                clip(&format!("Needs your OK: {}", p.title))
            } else if let Some(e) = &self.elicit_ask {
                if e.server_name.eq_ignore_ascii_case("Plan") {
                    "Plan ready for your OK".into()
                } else {
                    clip(&format!("Asks: {}", e.message))
                }
            } else if let Some(p) = self.perm_queue.front() {
                clip(&format!("Needs your OK: {}", p.title))
            } else if let Some(c) = self.tool_cards.iter().rev().find(|c| !c.title.is_empty()) {
                clip(&c.title)
            } else if !self.stream_buf.trim().is_empty() {
                "Writing a reply…".into()
            } else {
                "Thinking…".into()
            };
            upsert(
                &mut map,
                ActivityItem {
                    key: format!("chat:{turn_id}"),
                    kind: ActivityKind::Chat,
                    title: CHAT_TITLE.into(),
                    status: if waiting {
                        ActivityStatus::Waiting
                    } else {
                        ActivityStatus::Running
                    },
                    detail,
                    steps,
                    at: now,
                },
            );
        }

        for p in self.harness.park.iter().chain(self.harness.queue.iter()) {
            let key = format!("chat:{}", p.chat_id);
            if map
                .get(&key)
                .is_some_and(|i| i.status == ActivityStatus::Waiting)
            {
                continue;
            }
            upsert(
                &mut map,
                ActivityItem {
                    key,
                    kind: ActivityKind::Chat,
                    title: activity_chat_title(self, &p.chat_id),
                    status: ActivityStatus::Waiting,
                    detail: clip(&format!("Needs your OK: {}", p.tool)),
                    steps: 0,
                    at: now,
                },
            );
        }
        for p in &self.harness.soft_parks {
            let key = format!("chat:{}", p.chat_id);
            if map
                .get(&key)
                .is_some_and(|i| i.status == ActivityStatus::Waiting)
            {
                continue;
            }
            upsert(
                &mut map,
                ActivityItem {
                    key,
                    kind: ActivityKind::Chat,
                    title: activity_chat_title(self, &p.chat_id),
                    status: ActivityStatus::Waiting,
                    detail: clip(&format!("Needs your OK: {}", p.reason)),
                    steps: 0,
                    at: now,
                },
            );
        }

        for run in &self.bg.runs {
            if run.origin == BgOrigin::Scheduled {
                continue;
            }
            let key = format!("bg:{}", run.id);
            let item = |status, detail| ActivityItem {
                key: key.clone(),
                kind: ActivityKind::Background,
                title: run.title.clone(),
                status,
                detail,
                steps: 0,
                at: now,
            };
            match &run.end {
                None => {
                    let elapsed =
                        grokhub_core::bg_elapsed_label(run.started.elapsed().as_millis() as u64);
                    let detail = if !run.action.is_empty() {
                        clip(&format!("{elapsed} · {}", run.action))
                    } else {
                        format!("{elapsed} · Running now…")
                    };
                    upsert(&mut map, item(ActivityStatus::Running, detail));
                }
                Some(grokhub_core::BgEnd::Failed(why)) => {
                    upsert(&mut map, item(ActivityStatus::Failed, clip(why)));
                }
                Some(grokhub_core::BgEnd::Done | grokhub_core::BgEnd::Stopped) => {}
            }
        }

        for run in &self.bg.runs {
            if run.origin != BgOrigin::Scheduled {
                continue;
            }
            let id = run.automation.clone().unwrap_or_else(|| run.id.to_string());
            let key = format!("run:{id}");
            match &run.end {
                None => {
                    let detail = if !run.action.is_empty() {
                        clip(&run.action)
                    } else if !run.say.is_empty() {
                        clip(&run.say)
                    } else {
                        "Running now…".into()
                    };
                    upsert(
                        &mut map,
                        ActivityItem {
                            key,
                            kind: ActivityKind::Routine,
                            title: run.title.clone(),
                            status: ActivityStatus::Running,
                            detail,
                            steps: 0,
                            at: now,
                        },
                    );
                }
                Some(grokhub_core::BgEnd::Done) => {
                    // settle already pushed recent; keep live only while settling.
                }
                Some(grokhub_core::BgEnd::Failed(why)) => {
                    upsert(
                        &mut map,
                        ActivityItem {
                            key,
                            kind: ActivityKind::Routine,
                            title: run.title.clone(),
                            status: ActivityStatus::Failed,
                            detail: clip(why),
                            steps: 0,
                            at: now,
                        },
                    );
                }
                Some(grokhub_core::BgEnd::Stopped) => {}
            }
        }

        // Messages and side questions typed during a reply wait their turn
        // here, and only here: the chat box above shows none of them.
        // A message typed in another chat names the chat it waits for, and
        // stays listed after the reply ends until that chat is opened.
        if self.running || !self.followup_queue.is_empty() {
            let texts = self.followup_queue.texts();
            let mut queued = queued_items(&texts, ActivityKind::QueuedMessage, "queue", now);
            let here = self
                .chat_job_thread
                .clone()
                .filter(|_| self.running)
                .unwrap_or_else(|| self.visible_thread_id());
            for (item, q) in queued.iter_mut().zip(self.followup_queue.iter()) {
                if !q.fits(&here) {
                    item.detail = waits_for_line(self.thread_title(&q.thread).as_deref());
                }
            }
            for item in queued {
                upsert(&mut map, item);
            }
        }
        // A side answer being written shows here with Stop until it lands.
        for item in side_items(&self.side.runs, now) {
            upsert(&mut map, item);
        }

        if let Some(dl) = &self.setup.dl {
            let detail = if dl.verifying {
                "Checking the file…".to_string()
            } else {
                super::model_download_ui::dock_progress_line(dl.done, dl.total)
            };
            upsert(
                &mut map,
                ActivityItem {
                    key: "dl:model".into(),
                    kind: ActivityKind::Download,
                    title: format!("Downloading {}", dl.model.name),
                    status: ActivityStatus::Running,
                    detail,
                    steps: 0,
                    at: now,
                },
            );
        }

        if let Some(live) = &self.screen_rec {
            upsert(
                &mut map,
                ActivityItem {
                    key: "rec:screen".into(),
                    kind: ActivityKind::Recording,
                    title: "Screen recording".into(),
                    status: ActivityStatus::Running,
                    detail: super::screen_record_ui::recording_dock_line(live),
                    steps: 0,
                    at: now,
                },
            );
        }

        dock_items(&map.into_values().collect::<Vec<_>>(), now)
    }

    /// Titlebar Activity control: collapsed summary, expandable list.
    /// The list opens below the Chat tab strip, never over it.
    fn activity_strip_gap(&self) -> f32 {
        if self.page_nav() == Nav::Chat {
            super::chat_tabs::CHAT_TABS_H
        } else {
            0.0
        }
    }

    /// A decision waits and the Activity chip already counts it.
    pub(super) fn activity_shows_waiting(&self) -> bool {
        self.activity_items()
            .iter()
            .any(|i| i.status == ActivityStatus::Waiting)
    }

    /// Always drawn, so nothing left of it moves when work starts. Empty is a
    /// quiet "Activity" with no dot.
    pub(super) fn ui_activity_dock(&mut self, ui: &mut egui::Ui) {
        let items = self.activity_items();
        let waiting = items.iter().any(|i| i.status == ActivityStatus::Waiting);
        let summary = dock_summary(&items);
        let loadout = crate::theme::is_loadout_paint();
        let look = crate::ui_kit::recipe::look(
            crate::ui_kit::recipe::Part::Button,
            crate::ui_kit::recipe::State::Default,
        );
        let stroke = if waiting {
            egui::Stroke::new(crate::ui_kit::recipe::STROKE_W, crate::theme::signal_red())
        } else {
            look.stroke
        };
        let label = if summary.is_empty() {
            "Activity".to_string()
        } else {
            format!("Activity  {summary}")
        };
        let font = if loadout {
            crate::theme::loadout_font(11.0)
        } else {
            crate::theme::title_font(crate::theme::FONT_CHROME)
        };
        let toggle = ui.add(
            egui::Button::new(RichText::new(label).font(font).color(crate::theme::fg()))
                .fill(egui::Color32::TRANSPARENT)
                .stroke(stroke)
                .corner_radius(crate::ui_kit::recipe::BUTTON_RADIUS),
        );
        let top = toggle.rect;
        let toggle_rect = toggle.rect;
        if toggle.clicked() {
            self.activity.open = !self.activity.open;
        }
        // Status dot on the left edge of the toggle.
        if let Some(first) = items.first() {
            let color = match first.status {
                ActivityStatus::Waiting | ActivityStatus::Failed => crate::theme::signal_red(),
                ActivityStatus::Running => crate::theme::amber(),
                ActivityStatus::Done | ActivityStatus::Queued => crate::theme::muted(),
            };
            let c = egui::pos2(top.left() + 8.0, top.center().y);
            ui.painter().circle_filled(c, 3.5, color);
        }

        if !self.activity.open {
            return;
        }
        let total = items.len();
        let listed: Vec<ActivityItem> = items.into_iter().take(DOCK_MAX).collect();
        let more = total.saturating_sub(listed.len());
        let mut act: Option<DockAct> = None;
        let steerable = self.can_steer_live_turn();
        // Steer now only for a message typed in the running chat.
        let steer_rows: Vec<bool> = (0..self.followup_queue.len())
            .map(|i| self.queued_steers_here(i))
            .collect();
        let mut open_card: Option<String> = None;
        let list = egui::Area::new(egui::Id::new("activity-dock-list"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(
                top.right() - 340.0,
                top.bottom() + 6.0 + self.activity_strip_gap(),
            ))
            .constrain_to(ui.ctx().content_rect())
            .show(ui.ctx(), |ui| {
                let card = crate::ui_kit::recipe::look(
                    crate::ui_kit::recipe::Part::Card,
                    crate::ui_kit::recipe::State::Default,
                );
                egui::Frame::NONE
                    .fill(card.fill)
                    .stroke(card.stroke)
                    .corner_radius(card.radius)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width(316.0);
                        ui.spacing_mut().item_spacing.y = 8.0;
                        if listed.is_empty() {
                            ui.label(
                                RichText::new("Nothing running right now")
                                    .size(crate::theme::TYPE_CAPTION)
                                    .color(crate::theme::muted()),
                            );
                        }
                        for (i, item) in listed.iter().enumerate() {
                            if i > 0 {
                                ui.painter().hline(
                                    ui.max_rect().x_range(),
                                    ui.cursor().top(),
                                    egui::Stroke::new(1.0, crate::theme::border()),
                                );
                                ui.add_space(4.0);
                            }
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&item.title)
                                        .strong()
                                        .size(crate::theme::TYPE_BODY)
                                        .color(crate::theme::fg()),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let tone = match item.status {
                                            ActivityStatus::Waiting | ActivityStatus::Failed => {
                                                crate::theme::signal_red()
                                            }
                                            _ => crate::theme::muted(),
                                        };
                                        ui.label(
                                            RichText::new(status_word(item.status))
                                                .size(crate::theme::TYPE_CAPTION)
                                                .color(tone),
                                        );
                                    },
                                );
                            });
                            ui.label(
                                RichText::new(&item.detail)
                                    .size(crate::theme::TYPE_CAPTION)
                                    .color(crate::theme::muted()),
                            );
                            let steps = steps_line(item);
                            if !steps.is_empty() {
                                ui.label(
                                    RichText::new(steps)
                                        .size(crate::theme::TYPE_CAPTION)
                                        .color(crate::theme::muted()),
                                );
                            }
                            if item.status == ActivityStatus::Running {
                                let (bar, _) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 3.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    bar,
                                    2.0,
                                    crate::theme::amber().gamma_multiply(0.35),
                                );
                            }
                            let chat_id = item_chat_id(&self.bg.runs, item);
                            // A background run lives on its card; a chat turn
                            // that works a Workboard card can open it too.
                            let run_card = item_run_card(&self.bg, item);
                            let card = run_card.clone().or_else(|| {
                                super::board_ui::board_card_for_chat(&self.board, &chat_id)
                            });
                            let row_btn = |ui: &mut egui::Ui, text: &str| {
                                ui.add(
                                    egui::Button::new(
                                        RichText::new(text)
                                            .size(crate::theme::TYPE_CAPTION)
                                            .color(crate::theme::fg()),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::new(1.0, crate::theme::border())),
                                )
                                .clicked()
                            };
                            let place = queue_index(&item.key);
                            ui.horizontal(|ui| {
                                let steers = item.kind != ActivityKind::QueuedMessage
                                    || steer_rows.get(place).copied().unwrap_or(false);
                                for (label, pick) in row_buttons(item, steerable && steers) {
                                    if row_btn(ui, label) {
                                        act = Some(match pick {
                                            RowPick::Open => DockAct::Open {
                                                kind: item.kind,
                                                chat_id: chat_id.clone(),
                                                card: run_card.clone(),
                                                waiting: item.status == ActivityStatus::Waiting,
                                            },
                                            RowPick::SteerNow => DockAct::SteerQueued(place),
                                            RowPick::Remove
                                                if item.kind == ActivityKind::SideQuestion =>
                                            {
                                                DockAct::DropSide(place)
                                            }
                                            RowPick::Remove => DockAct::DropQueued(place),
                                            RowPick::Stop
                                                if item.kind == ActivityKind::Recording =>
                                            {
                                                DockAct::StopRecording
                                            }
                                            RowPick::Stop => DockAct::StopRun(
                                                item.key
                                                    .strip_prefix("bg:")
                                                    .and_then(|id| id.parse().ok())
                                                    .unwrap_or(0),
                                            ),
                                        });
                                    }
                                }
                                if let Some(card) = card.filter(|_| run_card.is_none()) {
                                    if row_btn(ui, "Open card") {
                                        open_card = Some(card);
                                    }
                                }
                            });
                        }
                        if more > 0 {
                            ui.label(
                                RichText::new(format!("{more} more"))
                                    .size(crate::theme::TYPE_CAPTION)
                                    .color(crate::theme::muted()),
                            );
                        }
                    });
            });
        let shut = ui.input(|i| {
            i.key_pressed(egui::Key::Escape)
                || (i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| {
                        !list.response.rect.contains(p) && !toggle_rect.contains(p)
                    }))
        });
        if shut {
            self.activity.open = false;
        }
        if let Some(id) = open_card {
            self.activity.open = false;
            self.open_board_card(&id);
        }
        match act {
            None => {}
            Some(DockAct::Open {
                kind,
                chat_id,
                card,
                waiting,
            }) => self.open_dock_row(kind, &chat_id, card.as_deref(), waiting),
            // Queue edits keep the list open so the next row is one click away.
            Some(DockAct::SteerQueued(i)) => self.steer_queued(i),
            Some(DockAct::DropQueued(i)) => self.drop_queued(i),
            Some(DockAct::DropSide(id)) => self.stop_side_run(id as u64),
            Some(DockAct::StopRun(id)) => self.stop_bg_run(id),
            Some(DockAct::StopRecording) => self.stop_screen_recording(),
        }
    }

    /// Open on an Activity row: a background run's card, a chat, a routine's
    /// page or the model downloads.
    fn open_dock_row(
        &mut self,
        kind: ActivityKind,
        chat_id: &str,
        card: Option<&str>,
        waiting: bool,
    ) {
        self.activity.open = false;
        match kind {
            // A background run is its card: open that, not its chat.
            ActivityKind::Background if card.is_some() => {
                self.open_board_card(card.unwrap_or_default());
            }
            ActivityKind::Chat | ActivityKind::Background => {
                self.select_main_nav("chat");
                if !chat_id.is_empty() && chat_id != self.visible_thread_id() {
                    if let Some(idx) = self.threads.iter().position(|t| t.id == chat_id) {
                        self.switch_thread(idx);
                    }
                }
                if waiting {
                    self.harness.jump = Some("ask");
                    // The chat no longer prints a count line; open the
                    // rows when there is more than the one card.
                    self.harness.inbox_open = self.decisions_waiting() > 1;
                }
            }
            ActivityKind::Routine => {
                self.select_main_nav("automations");
            }
            ActivityKind::Download => {
                self.nav = Nav::Settings;
                self.settings_sec = SettingsSec::AiModels;
            }
            ActivityKind::QueuedMessage | ActivityKind::SideQuestion | ActivityKind::Recording => {}
        }
    }

    /// Deck-shot seed: plant a waiting approval so the dock paints on slides 02/20.
    /// Home Mission Control shots (`GROKHUB_DECK_NAV=home`) keep the popover closed —
    /// slide 09 must show the grid, not Activity over Home.
    pub(super) fn seed_activity_dock_for_deck(&mut self) {
        let now = now_ms();
        let id = self.visible_thread_id();
        let home_shot = super::loadout_home::deck_nav_home_pinned();
        self.activity.open = !home_shot;
        self.activity.recent = vec![
            ActivityItem {
                key: format!("chat:{id}"),
                kind: ActivityKind::Chat,
                title: CHAT_TITLE.into(),
                status: ActivityStatus::Waiting,
                detail: "Needs your OK: Read project notes".into(),
                steps: 2,
                at: now,
            },
            routine_finished(
                routine_started(
                    "deck-pulse",
                    "Pulse morning brief",
                    now.saturating_sub(5_000),
                ),
                true,
                "3 new items from your library.",
                now.saturating_sub(2_000),
            ),
        ];
        // A live Ask keeps the waiting row tied to real approval paint.
        // Skip on Home shots so the Activity bell stays quiet over Mission Control.
        if !home_shot && self.perm_ask.is_none() {
            self.perm_ask = Some(grokhub_core::wire::PermissionAsk {
                rpc_id: serde_json::json!(1),
                session_id: id.clone(),
                title: "Read project notes".into(),
                tool_call_id: "deck-ask".into(),
                action: "read notes".into(),
                reason: String::new(),
                reject_option: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, status: ActivityStatus, ago: u64, now: u64) -> ActivityItem {
        ActivityItem {
            key: key.into(),
            kind: ActivityKind::Routine,
            title: key.into(),
            status,
            detail: String::new(),
            steps: 0,
            at: now.saturating_sub(ago),
        }
    }

    #[test]
    fn dock_items_orders_waiting_then_running_then_results() {
        let now = 100 * DONE_TTL_MS;
        let items = [
            item("old-done", ActivityStatus::Done, DONE_TTL_MS + 1, now),
            item("done", ActivityStatus::Done, 5, now),
            item("run-a", ActivityStatus::Running, 50, now),
            item("run-b", ActivityStatus::Running, 10, now),
            item("wait", ActivityStatus::Waiting, 99, now),
            item("fail", ActivityStatus::Failed, 3, now),
        ];
        let shown = dock_items(&items, now);
        assert_eq!(
            shown.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(),
            ["wait", "run-b", "run-a", "fail", "done"]
        );
        assert_eq!(
            dock_summary(&shown),
            "1 waiting · 2 running · 1 failed · 1 done"
        );
    }

    #[test]
    fn steps_line_only_for_chat_with_steps() {
        let chat = ActivityItem {
            key: "chat:1".into(),
            kind: ActivityKind::Chat,
            title: CHAT_TITLE.into(),
            status: ActivityStatus::Running,
            detail: "Read".into(),
            steps: 1,
            at: 0,
        };
        assert_eq!(steps_line(&chat), "1 step done");
        let many = ActivityItem { steps: 3, ..chat };
        assert_eq!(steps_line(&many), "3 steps done");
    }

    #[test]
    fn quiet_cabin_hides_empty_dock() {
        let cabin = Cabin::quiet_for_test();
        assert!(cabin.activity_items().is_empty());
        assert!(!cabin.activity.open);
    }

    #[test]
    fn the_chip_is_always_drawn_and_closes_on_esc_and_outside_click() {
        let src = include_str!("activity_dock.rs").replace("\r\n", "\n");
        let dock = src
            .split("pub(super) fn ui_activity_dock(")
            .nth(1)
            .and_then(|s| s.split("pub(super) fn seed_activity_dock_for_deck(").next())
            .expect("dock");
        assert!(
            !dock.contains("if items.is_empty() {"),
            "an empty dock still draws its chip: {dock}"
        );
        assert!(dock.contains("Key::Escape") && dock.contains("any_pressed"));
        let mut cabin = Cabin::quiet_for_test();
        cabin.activity.open = true;
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let mut esc = egui::RawInput::default();
        esc.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        });
        let _ = crate::theme::test_pass(&ctx, esc, |ui| cabin.ui_activity_dock(ui));
        assert!(!cabin.activity.open, "Esc closes the list");
    }

    #[test]
    fn open_on_a_background_row_goes_to_its_card_not_its_chat() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.board.clear();
        let chat = cabin.visible_thread_id();
        cabin.open_bg_card(7, "tidy the notes", "", true);
        let card = cabin.bg_card_id(7).expect("the run's card");
        let item = ActivityItem {
            key: "bg:7".into(),
            kind: ActivityKind::Background,
            title: "tidy the notes".into(),
            status: ActivityStatus::Running,
            detail: String::new(),
            steps: 0,
            at: 0,
        };
        assert_eq!(
            item_run_card(&cabin.bg, &item).as_deref(),
            Some(card.as_str())
        );
        cabin.nav = Nav::Chat;
        cabin.open_dock_row(ActivityKind::Background, &chat, Some(&card), false);
        assert_eq!(cabin.nav, Nav::Workboard);
        assert_eq!(cabin.board_view.open.as_deref(), Some(card.as_str()));
        // A run with no card still opens its chat.
        cabin.open_dock_row(ActivityKind::Background, &chat, None, false);
        assert_eq!(cabin.nav, Nav::Chat);
    }

    #[test]
    fn a_background_run_you_started_shows_in_activity() {
        let mut cabin = Cabin::quiet_for_test();
        let thread_id = cabin.visible_thread_id();
        cabin.bg.runs.push(super::super::background::BgRun {
            id: 7,
            thread_id,
            title: "tidy the notes".into(),
            origin: BgOrigin::User,
            rx: None,
            say: String::new(),
            action: "Reading notes.md".into(),
            started: std::time::Instant::now(),
            end: None,
            session: String::new(),
            resumed: None,
            fork_hold: false,
            native_session: None,
            automation: None,
        });
        let shown = cabin.activity_items();
        let run = &cabin.bg.runs[0];
        let item = shown
            .iter()
            .find(|i| i.kind == ActivityKind::Background)
            .expect("the run is listed");
        assert_eq!(item.key, format!("bg:{}", run.id));
        assert_eq!(item.status, ActivityStatus::Running);
        assert_eq!(item.detail, "0s · Reading notes.md");
        assert_eq!(item_chat_id(&cabin.bg.runs, item), run.thread_id);
        cabin.bg.runs[0].end = Some(grokhub_core::BgEnd::Done);
        assert!(
            cabin
                .activity_items()
                .iter()
                .all(|i| i.kind != ActivityKind::Background),
            "a finished run leaves Activity; its reply posts in the chat"
        );
    }

    #[test]
    fn waiting_perm_ask_shows_in_dock() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.running = true;
        cabin.chat_job_thread = Some(cabin.visible_thread_id());
        cabin.perm_ask = Some(grokhub_core::wire::PermissionAsk {
            rpc_id: serde_json::json!(7),
            session_id: "s".into(),
            title: "Run npm test".into(),
            tool_call_id: "t".into(),
            action: "npm test".into(),
            reason: String::new(),
            reject_option: None,
        });
        let shown = cabin.activity_items();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].status, ActivityStatus::Waiting);
        assert!(shown[0].detail.contains("Run npm test"));
        assert_eq!(dock_summary(&shown), "1 waiting");
    }

    #[test]
    fn a_queue_reads_as_x_of_y_running() {
        let now = 100 * DONE_TTL_MS;
        let mut items = vec![item("run", ActivityStatus::Running, 1, now)];
        items.extend(queued_items(
            &["then deploy".into()],
            ActivityKind::QueuedMessage,
            "queue",
            now,
        ));
        assert_eq!(dock_summary(&items), "1 of 2 running");
        items.push(item("wait", ActivityStatus::Waiting, 1, now));
        items.push(item("done", ActivityStatus::Done, 1, now));
        items.extend(queued_items(
            &["a side question".into()],
            ActivityKind::SideQuestion,
            "side",
            now,
        ));
        assert_eq!(dock_summary(&items), "1 waiting · 1 of 3 running · 1 done");
        let only_queued = queued_items(&["x".into()], ActivityKind::QueuedMessage, "queue", now);
        assert_eq!(dock_summary(&only_queued), "1 queued");
        assert_eq!(dock_summary(&[]), "");
    }

    #[test]
    fn queued_rows_keep_their_order_and_place() {
        let now = 100 * DONE_TTL_MS;
        let queue = vec![
            "first thing\nmore".to_string(),
            "second".into(),
            "third".into(),
            "fourth".into(),
        ];
        let mut items = queued_items(&queue, ActivityKind::QueuedMessage, "queue", now);
        items.push(item("run", ActivityStatus::Running, 5, now));
        let shown = dock_items(&items, now);
        assert_eq!(
            shown.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(),
            ["run", "queue:0", "queue:1", "queue:2", "queue:3"]
        );
        assert_eq!(shown[1].title, "first thing");
        assert_eq!(
            shown[1..]
                .iter()
                .map(|i| i.detail.as_str())
                .collect::<Vec<_>>(),
            ["Next up", "2nd in line", "3rd in line", "4th in line"]
        );
        assert_eq!(status_word(ActivityStatus::Queued), "Queued");
    }

    #[test]
    fn queued_rows_offer_steer_now_only_when_the_reply_can_take_it() {
        let now = 1_000;
        let q = &queued_items(&["x".into()], ActivityKind::QueuedMessage, "queue", now)[0];
        let labels = |steer| {
            row_buttons(q, steer)
                .iter()
                .map(|(l, _)| *l)
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(true), ["Steer now", "Remove"]);
        assert_eq!(labels(false), ["Remove"]);
        let side = &queued_items(&["y".into()], ActivityKind::SideQuestion, "side", now)[0];
        assert_eq!(row_buttons(side, true), [("Stop", RowPick::Remove)]);
        assert_eq!(queue_index("queue:3"), 3);
        assert_eq!(queue_index("side:0"), 0);
    }

    #[test]
    fn the_cabin_lists_queued_messages_while_they_wait() {
        let mut cabin = Cabin::quiet_for_test();
        assert!(cabin.activity_items().is_empty(), "nothing runs or waits");
        // A message waits for its own chat even while nothing runs.
        cabin
            .followup_queue
            .push("other-chat", "for the other chat".into());
        let idle = cabin.activity_items();
        assert_eq!(idle.len(), 1, "{idle:?}");
        assert_eq!(idle[0].key, "queue:0");
        assert_eq!(idle[0].detail, waits_for_line(None));
        cabin.followup_queue =
            Vec::<String>::from(["then deploy".into(), "and tell me".into()]).into();
        cabin.running = true;
        cabin.chat_job_thread = Some(cabin.visible_thread_id());
        let shown = cabin.activity_items();
        assert_eq!(
            shown.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(),
            [
                "chat:".to_string() + &cabin.visible_thread_id(),
                "queue:0".into(),
                "queue:1".into()
            ]
        );
        assert_eq!(dock_summary(&shown), "1 of 3 running");
    }

    #[test]
    fn remove_puts_the_text_back_in_an_empty_chat_box() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.followup_queue =
            Vec::<String>::from(["then deploy".into(), "and tell me".into()]).into();
        cabin.drop_queued(0);
        assert_eq!(cabin.followup_queue, vec!["and tell me".to_string()]);
        assert_eq!(cabin.composer, "then deploy");
        assert_eq!(
            cabin.status,
            "Removed from the queue and put back in the chat box"
        );
        // A box with your words in it keeps them; the removed text just goes.
        cabin.drop_queued(0);
        assert!(cabin.followup_queue.is_empty());
        assert_eq!(cabin.composer, "then deploy");
        assert_eq!(cabin.status, "Removed from the queue");
        cabin.composer = "btw what time is it".into();
        cabin.drop_queued(5);
        assert_eq!(
            cabin.composer, "btw what time is it",
            "a stale index changes nothing"
        );
    }
}
