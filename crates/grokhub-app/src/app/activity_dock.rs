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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivityStatus {
    Running,
    Waiting,
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivityKind {
    Chat,
    Routine,
    /// A `/bg` run you started; key `bg:{run id}`.
    Background,
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
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > DETAIL_MAX {
        let cut: String = one.chars().take(DETAIL_MAX.saturating_sub(1)).collect();
        format!("{cut}…")
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
    }
}

fn status_rank(status: ActivityStatus) -> u8 {
    match status {
        ActivityStatus::Waiting => 0,
        ActivityStatus::Running => 1,
        ActivityStatus::Failed => 2,
        ActivityStatus::Done => 3,
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

/// The collapsed dock's line, e.g. "1 waiting · 2 running".
pub(super) fn dock_summary(items: &[ActivityItem]) -> String {
    let count = |s: ActivityStatus| items.iter().filter(|i| i.status == s).count();
    let parts = [
        (count(ActivityStatus::Waiting), "waiting"),
        (count(ActivityStatus::Running), "running"),
        (count(ActivityStatus::Failed), "failed"),
        (count(ActivityStatus::Done), "done"),
    ];
    parts
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, word)| format!("{n} {word}"))
        .collect::<Vec<_>>()
        .join(" · ")
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
                    let detail = if !run.action.is_empty() {
                        clip(&run.action)
                    } else {
                        "Running now…".into()
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
                ActivityStatus::Done => crate::theme::muted(),
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
        let mut act: Option<(ActivityKind, String, bool)> = None;
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
                                    .size(11.0)
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
                                                .size(11.0)
                                                .color(tone),
                                        );
                                    },
                                );
                            });
                            ui.label(
                                RichText::new(&item.detail)
                                    .size(11.0)
                                    .color(crate::theme::muted()),
                            );
                            let steps = steps_line(item);
                            if !steps.is_empty() {
                                ui.label(
                                    RichText::new(steps).size(11.0).color(crate::theme::muted()),
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
                            let btn = match (item.kind, item.status) {
                                (ActivityKind::Chat, ActivityStatus::Waiting) => "Review",
                                (ActivityKind::Chat, _) => "Open chat",
                                (ActivityKind::Routine, _) => "Open result",
                                (ActivityKind::Background, _) => "Open chat",
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(btn).size(11.0).color(crate::theme::fg()),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::new(1.0, crate::theme::border())),
                                )
                                .clicked()
                            {
                                let chat_id = item_chat_id(&self.bg.runs, item);
                                act = Some((
                                    item.kind,
                                    chat_id,
                                    item.status == ActivityStatus::Waiting,
                                ));
                            }
                        }
                        if more > 0 {
                            ui.label(
                                RichText::new(format!("{more} more"))
                                    .size(11.0)
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
        if let Some((kind, chat_id, waiting)) = act {
            self.activity.open = false;
            match kind {
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
            }
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
        assert_eq!(item.detail, "Reading notes.md");
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
}
