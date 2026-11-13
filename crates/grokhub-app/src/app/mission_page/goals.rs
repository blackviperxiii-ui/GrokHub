//! The Objectives page's goal cards: Today (the chat's next step and the
//! goals worth a look), My goals (the add box, tracked goals with +1, Done
//! and Drop, and the suggested tray) and This season (goals done, weeks in a
//! row and the trophy shelf). Goals live in `objectives.json`, shared with
//! the shell, web and phone.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_core::mission::{MissionCheckStatus, MissionView};
use grokhub_core::objectives::{
    self, goal_detail, goal_title, steps_by_one, Goal, GoalAct, KnownGame, Objectives, Pick,
    SHELF_MAX,
};

use super::super::loadout_home::{caption, card, diamond, header, paint_weeks};
use super::super::*;

/// How often the goals are read again from disk (the other app may write).
const GOALS_REFRESH: Duration = Duration::from_secs(30);
const GOALS_ID: &str = "loadout-goals";
const WEEK_ID: &str = "objectives-your-week";
/// The caption over Monday's recap on Today.
pub(in crate::app) const YOUR_WEEK: &str = "Your week";
const DRAFT_ID: &str = "objectives-add-draft";
const ADD_ERROR_ID: &str = "objectives-add-error";
pub(in crate::app) const ADD_HINT: &str = "What do you want to do?";
pub(in crate::app) const TODAY_EMPTY: &str =
    "Nothing for today yet. Add a goal below and it shows up here.";
const MINE_EMPTY: &str =
    "No goals yet. Type one above, like \"99 Fishing in OSRS\" or \"finish Elden Ring\".";
const SHELF_EMPTY: &str = "Goals you finish land here.";
/// The button under a follow-up on Today: asks for help in chat.
pub(in crate::app) const PLAN_IT: &str = "Plan it";
/// Set when a finished chat turn changed the goals, so the page and the XP
/// bar read them again on the next frame instead of in up to 30s.
static GOALS_CHANGED: AtomicBool = AtomicBool::new(false);
/// The "Track goal" one-tap for the goal the last reply noticed, with the
/// chat it belongs to. Gone once tapped or after the chat's next turn.
static ASK: Mutex<Option<(String, QuickChip)>> = Mutex::new(None);

fn muted_line(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(crate::theme::TYPE_BODY)
            .color(crate::theme::muted()),
    );
}

/// A thin amber bar filled to `frac`.
pub(in crate::app) fn goal_bar(ui: &mut egui::Ui, frac: f32) {
    let (bar, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
    ui.painter().rect_filled(bar, 3.0, crate::theme::selected());
    let mut fill = bar;
    fill.set_width(bar.width() * frac.clamp(0.0, 1.0));
    ui.painter().rect_filled(fill, 3.0, crate::theme::amber());
}

/// The goal's title, bar and detail line.
pub(in crate::app) fn goal_summary(ui: &mut egui::Ui, g: &Goal) {
    ui.label(
        RichText::new(goal_title(g))
            .size(crate::theme::TYPE_LABEL)
            .color(crate::theme::fg()),
    );
    ui.add_space(4.0);
    goal_bar(ui, g.frac());
    ui.add_space(2.0);
    ui.label(
        RichText::new(goal_detail(g))
            .size(crate::theme::TYPE_CAPTION)
            .color(crate::theme::muted()),
    );
}

fn goal_row(ui: &mut egui::Ui, g: &Goal, acts: &mut Vec<GoalAct>) {
    egui::Frame::NONE
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            goal_summary(ui, g);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if steps_by_one(g) && crate::cards::ghost_pill(ui, "+1") {
                    acts.push(GoalAct::Bump { id: g.id.clone() });
                }
                if crate::cards::ghost_pill(ui, "Done") {
                    acts.push(GoalAct::Done { id: g.id.clone() });
                }
                if crate::cards::ghost_pill(ui, "Drop") {
                    acts.push(GoalAct::Drop { id: g.id.clone() });
                }
            });
        });
}

fn suggestion_row(ui: &mut egui::Ui, g: &Goal, acts: &mut Vec<GoalAct>) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Track goal: {}?", goal_title(g)))
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::fg()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::cards::ghost_pill(ui, "Not now") {
                acts.push(GoalAct::NotNow { id: g.id.clone() });
            }
            if crate::cards::ghost_pill(ui, "Track") {
                acts.push(GoalAct::Track { id: g.id.clone() });
            }
        });
    });
}

fn known_games(games: &[(String, Option<u32>)]) -> Vec<KnownGame<'_>> {
    games
        .iter()
        .map(|(name, app_id)| KnownGame {
            name,
            steam_app_id: *app_id,
            source: None,
        })
        .collect()
}

type CachedWeek = (Instant, Option<objectives::YourWeek>);

/// Monday's "Your week", read again every [`GOALS_REFRESH`].
fn dock_your_week(ctx: &egui::Context) -> Option<objectives::YourWeek> {
    let id = egui::Id::unique(WEEK_ID);
    if let Some((at, w)) = ctx.data(|d| d.get_temp::<CachedWeek>(id)) {
        if at.elapsed() < GOALS_REFRESH {
            return w;
        }
    }
    let clock = Cabin::local_clock();
    let w = objectives::week_from_disk(
        &crate::config::config_dir(),
        clock.now_ms,
        clock.utc_offset_min(),
    );
    ctx.data_mut(|d| d.insert_temp::<CachedWeek>(id, (Instant::now(), w.clone())));
    w
}

impl Cabin {
    /// The saved goals, read again every [`GOALS_REFRESH`] or after a change.
    pub(in crate::app) fn dock_goals(&self, ctx: &egui::Context) -> Objectives {
        type Cached = (Instant, Objectives);
        let id = egui::Id::unique(GOALS_ID);
        let changed = GOALS_CHANGED.swap(false, Ordering::Relaxed);
        if changed {
            super::super::loadout_home::forget_xp(ctx);
        } else if let Some((at, o)) = ctx.data(|d| d.get_temp::<Cached>(id)) {
            if at.elapsed() < GOALS_REFRESH {
                return o;
            }
        }
        let dir = crate::config::config_dir();
        // Game data the player allowed (OSRS hiscores, Steam achievements)
        // moves goals before they are shown.
        if !objectives::refresh_from_games(&dir, now_ms()).is_empty() {
            super::super::loadout_home::forget_xp(ctx);
        }
        let o = objectives::load(&dir);
        ctx.data_mut(|d| d.insert_temp::<Cached>(id, (Instant::now(), o.clone())));
        o
    }

    /// Follow-ups due now as proactive cards. A last "I'll stop asking" line
    /// is no offer: it is written as gone out and shows on Today instead.
    pub(in crate::app) fn goal_follow_ups(
        &self,
        now: u64,
    ) -> Vec<grokhub_core::proactive::Candidate> {
        let dir = crate::config::config_dir();
        let nudges =
            |o: &objectives::Objectives| -> usize { o.goals.iter().map(|g| g.nudges.len()).sum() };
        let (_, (out, wrote)) = objectives::change(&dir, |o| {
            let before = nudges(o);
            let out = objectives::follow_up_candidates(o, now);
            let wrote = nudges(o) != before;
            (out, wrote)
        })
        .unwrap_or_default();
        if wrote {
            GOALS_CHANGED.store(true, Ordering::Relaxed);
        }
        out
    }

    /// A follow-up card about the goal titled `topic` went up.
    pub(in crate::app) fn note_goal_follow_up(&self, topic: &str, now: u64) {
        let dir = crate::config::config_dir();
        let wrote = objectives::change(&dir, |o| objectives::posted_follow_up(o, topic, now));
        if wrote.is_ok_and(|(_, w)| w) {
            GOALS_CHANGED.store(true, Ordering::Relaxed);
        }
    }

    /// Goals from a finished chat turn, as in the shell: a goal the player
    /// asked to track, one the reply noticed (it waits in the tray) and
    /// progress the player said. Most turns stop at the first look.
    pub(in crate::app) fn read_turn_goals(&mut self, said: &str, reply: &str) {
        let chat = self
            .chat_job_thread
            .clone()
            .unwrap_or_else(|| self.trace_id());
        let mut ask = ASK.lock().unwrap_or_else(|e| e.into_inner());
        if ask.as_ref().is_some_and(|(c, _)| *c == chat) {
            *ask = None;
        }
        drop(ask);
        if !objectives::worth_a_look(said, reply) {
            return;
        }
        let now = now_ms();
        let games = self.library_names(now);
        let known = known_games(&games);
        let read = objectives::change(&crate::config::config_dir(), |o| {
            objectives::read_turn(o, &chat, said, reply, &known, now)
        });
        let Ok((o, t)) = read else { return };
        if t.changed() {
            GOALS_CHANGED.store(true, Ordering::Relaxed);
        }
        let asked = t
            .ask
            .and_then(|id| o.goals.into_iter().find(|g| g.id == id));
        if let Some(g) = asked {
            let chip = objectives::track_chip(&g);
            *ASK.lock().unwrap_or_else(|e| e.into_inner()) = Some((chat, chip));
        }
    }

    /// The "Track goal" one-tap under this chat's reply, when the reply
    /// noticed a goal.
    pub(in crate::app) fn goal_chips(&self) -> Vec<QuickChip> {
        let chat = self.trace_id();
        ASK.lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|(c, chip)| *c == chat && !self.chip_dismissed.iter().any(|d| d == &chip.id))
            .map(|(_, chip)| vec![chip.clone()])
            .unwrap_or_default()
    }

    /// The "Track goal" one-tap: the goal moves to My goals. True when
    /// `chip` was one.
    pub(in crate::app) fn take_goal_chip(&mut self, chip: &QuickChip) -> bool {
        let Some(id) = chip.id.strip_prefix(objectives::TRACK_CHIP_PREFIX) else {
            return false;
        };
        *ASK.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let track = [GoalAct::Track { id: id.to_string() }];
        let saved = objectives::change(&crate::config::config_dir(), |o| {
            objectives::apply(o, &track, &[], now_ms())
        });
        self.status = match saved {
            Ok(_) => "Tracking it. It's on Objectives under My goals.".into(),
            Err(e) => format!("Couldn't save your goals: {e}"),
        };
        GOALS_CHANGED.store(true, Ordering::Relaxed);
        self.chip_paint_key.clear();
        true
    }

    /// The Steam library's names and app ids, for linking a goal to its game.
    fn library_names(&mut self, now: u64) -> Vec<(String, Option<u32>)> {
        self.steam_library(now)
            .1
            .iter()
            .map(|g| (g.name.clone(), Some(g.app_id)))
            .collect()
    }

    /// Applies the presses from this frame and refreshes the goals and XP.
    pub(in crate::app) fn apply_goal_acts(&mut self, ctx: &egui::Context, acts: Vec<GoalAct>) {
        if acts.is_empty() {
            return;
        }
        let now = now_ms();
        let games = self.library_names(now);
        let known = known_games(&games);
        let result = objectives::change(&crate::config::config_dir(), |o| {
            objectives::apply(o, &acts, &known, now)
        });
        let error = match result {
            Ok((o, refused)) => {
                ctx.data_mut(|d| {
                    d.insert_temp::<(Instant, Objectives)>(
                        egui::Id::unique(GOALS_ID),
                        (Instant::now(), o),
                    )
                });
                super::super::loadout_home::forget_xp(ctx);
                refused.map(str::to_string)
            }
            Err(e) => Some(format!("Couldn't save your goals: {e}")),
        };
        ctx.data_mut(|d| match error {
            Some(e) => {
                d.insert_temp(egui::Id::unique(ADD_ERROR_ID), e);
            }
            None => d.remove::<String>(egui::Id::unique(ADD_ERROR_ID)),
        });
    }

    /// Today: the chat's next step and up to two goal rows.
    pub(super) fn objectives_today(
        &mut self,
        ui: &mut egui::Ui,
        mission: Option<&MissionView>,
        goals: &Objectives,
    ) {
        let now = now_ms();
        let mut plan = None;
        let mut track = None;
        let week = dock_your_week(ui.ctx());
        card(ui, |ui| {
            header(ui, true, "Today", "", crate::theme::amber());
            let mut any = false;
            if let Some(w) = &week {
                any = true;
                ui.label(caption(YOUR_WEEK));
                for line in &w.lines {
                    ui.horizontal(|ui| {
                        diamond(ui, false);
                        ui.label(
                            RichText::new(line)
                                .size(crate::theme::TYPE_BODY)
                                .color(crate::theme::fg()),
                        );
                    });
                }
                if let Some(g) = &w.suggestion {
                    ui.horizontal(|ui| {
                        muted_line(ui, &format!("Try next: {}", g.title));
                        if crate::cards::ghost_pill(ui, "Track") {
                            track = Some(g.id.clone());
                        }
                    });
                }
                ui.add_space(8.0);
            }
            if let Some(f) = objectives::shown_follow_up(goals, now) {
                any = true;
                ui.label(
                    RichText::new(&f.line)
                        .size(crate::theme::TYPE_LABEL)
                        .color(crate::theme::fg()),
                );
                if !f.stop && crate::cards::ghost_pill(ui, PLAN_IT) {
                    plan = Some(f.prompt);
                }
                ui.add_space(8.0);
            }
            let next = mission.and_then(|m| {
                m.checklist.iter().find(|i| {
                    i.status != MissionCheckStatus::Verified
                        && i.status != MissionCheckStatus::Cancelled
                })
            });
            if let Some(step) = next {
                any = true;
                ui.horizontal(|ui| {
                    diamond(ui, true);
                    ui.label(
                        RichText::new(&step.text)
                            .size(crate::theme::TYPE_LABEL)
                            .color(crate::theme::fg()),
                    );
                });
                ui.label(caption("This chat's next step"));
                ui.add_space(8.0);
            }
            for pick in objectives::today_picks(goals, now) {
                let Some(g) = goals.goals.iter().find(|g| g.id == pick.id()) else {
                    continue;
                };
                any = true;
                match pick {
                    Pick::Closest(_) => goal_summary(ui, g),
                    Pick::PickUp(_) => {
                        ui.label(
                            RichText::new(objectives::pick_up_line(g))
                                .size(crate::theme::TYPE_LABEL)
                                .color(crate::theme::fg()),
                        );
                        ui.label(
                            RichText::new(goal_detail(g))
                                .size(crate::theme::TYPE_CAPTION)
                                .color(crate::theme::muted()),
                        );
                    }
                }
                ui.add_space(8.0);
            }
            if !any {
                muted_line(ui, TODAY_EMPTY);
            }
            if let Some(m) = mission.filter(|m| !m.checklist.is_empty()) {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(caption("This chat's steps"));
                super::objective_rows(ui, m);
            }
        });
        if let Some(prompt) = plan {
            self.nav = Nav::Chat;
            self.send_chat(prompt);
        }
        if let Some(id) = track {
            self.apply_goal_acts(ui.ctx(), vec![GoalAct::Track { id }]);
            ui.ctx()
                .data_mut(|d| d.remove::<CachedWeek>(egui::Id::unique(WEEK_ID)));
        }
    }

    /// My goals: the add box, tracked goals and the suggested tray.
    pub(super) fn objectives_mine(&mut self, ui: &mut egui::Ui, goals: &Objectives) {
        let now = now_ms();
        let mut acts = Vec::new();
        let tracking = goals.tracking();
        let suggested = goals.suggestions(now);
        card(ui, |ui| {
            let count = if tracking.is_empty() {
                String::new()
            } else {
                tracking.len().to_string()
            };
            header(ui, true, "My goals", &count, crate::theme::amber());
            let draft_id = egui::Id::unique(DRAFT_ID);
            let mut draft: String = ui.ctx().data(|d| d.get_temp(draft_id)).unwrap_or_default();
            let mut submit = false;
            ui.horizontal(|ui| {
                let add_w = 64.0;
                let edit = ui.add_sized(
                    egui::vec2((ui.available_width() - add_w - 8.0).max(120.0), 28.0),
                    egui::TextEdit::singleline(&mut draft).hint_text(ADD_HINT),
                );
                submit |= edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                submit |= crate::cards::ghost_pill(ui, "Add");
            });
            if submit && !draft.trim().is_empty() {
                acts.push(GoalAct::Add {
                    text: std::mem::take(&mut draft),
                });
            }
            ui.ctx().data_mut(|d| d.insert_temp(draft_id, draft));
            if let Some(e) = ui
                .ctx()
                .data(|d| d.get_temp::<String>(egui::Id::unique(ADD_ERROR_ID)))
            {
                ui.label(
                    RichText::new(e)
                        .size(crate::theme::TYPE_CAPTION)
                        .color(crate::theme::signal_red()),
                );
            }
            ui.add_space(8.0);
            if tracking.is_empty() {
                muted_line(ui, MINE_EMPTY);
            }
            for g in &tracking {
                goal_row(ui, g, &mut acts);
            }
            if !suggested.is_empty() {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(caption("Suggested"));
                for g in &suggested {
                    suggestion_row(ui, g, &mut acts);
                }
            }
        });
        self.apply_goal_acts(ui.ctx(), acts);
    }

    /// This season: goals done, weeks in a row and the trophy shelf.
    pub(super) fn objectives_season(&mut self, ui: &mut egui::Ui, goals: &Objectives) {
        let now = now_ms();
        let offset = Self::local_clock().utc_offset_min();
        let week = self.dock_xp(ui.ctx()).week;
        card(ui, |ui| {
            header(
                ui,
                true,
                "This season",
                &objectives::season_label(now, offset),
                crate::theme::amber(),
            );
            let label = objectives::season_label(now, offset);
            let done = objectives::done_this_season(goals, now, offset);
            let s = if done == 1 { "" } else { "s" };
            muted_line(ui, &format!("{done} goal{s} done this season"));
            if let Some(line) = objectives::badge_line(done, &label) {
                muted_line(ui, &line);
            }
            if let Some(bonus) = objectives::season_bonus_note(goals, now, offset) {
                ui.label(
                    RichText::new(bonus)
                        .size(crate::theme::TYPE_LABEL)
                        .color(crate::theme::amber()),
                );
            }
            ui.add_space(8.0);
            paint_weeks(ui, &week);
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.label(caption("Trophy shelf"));
            for badge in objectives::badges(goals, offset) {
                ui.horizontal(|ui| {
                    diamond(ui, true);
                    ui.label(
                        RichText::new(format!("{badge} badge"))
                            .size(crate::theme::TYPE_BODY)
                            .color(crate::theme::amber()),
                    );
                });
            }
            let trophies = goals.trophies();
            if trophies.is_empty() {
                muted_line(ui, SHELF_EMPTY);
            }
            for g in trophies.iter().take(SHELF_MAX) {
                ui.horizontal(|ui| {
                    diamond(ui, false);
                    ui.label(
                        RichText::new(goal_title(g))
                            .size(crate::theme::TYPE_BODY)
                            .color(crate::theme::fg()),
                    );
                    if let Some(at) = g.done_ms {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(caption(&objectives::short_date(at, offset)));
                        });
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::objectives::{GameRef, GoalKind, GoalState, NewGoal, Origin};

    fn goal(kind: GoalKind, game: &str, current: u32, target: u32) -> Goal {
        let mut o = Objectives::default();
        o.add(
            NewGoal {
                game: GameRef {
                    name: game.into(),
                    ..GameRef::default()
                },
                text: "99 Fishing".into(),
                kind,
                current,
                target,
                origin: Origin::Typed,
                chat: None,
            },
            1,
        )
        .unwrap();
        o.goals.remove(0)
    }

    #[test]
    fn rows_name_the_game_and_say_what_is_left() {
        let mut g = goal(GoalKind::Skill, "Old School RuneScape", 93, 99);
        assert_eq!(goal_title(&g), "Old School RuneScape · 99 Fishing");
        assert_eq!(goal_detail(&g), "93 / 99 · 6 levels to 99");
        assert!(steps_by_one(&g));
        g.checked = true;
        assert_eq!(goal_detail(&g), "93 / 99 · 6 levels to 99 · Checked");
        let g = goal(GoalKind::Finish, "", 0, 1);
        assert_eq!(goal_title(&g), "99 Fishing");
        assert_eq!(goal_detail(&g), "Not finished yet");
        assert!(!steps_by_one(&g), "a finish goal has no +1");
    }

    #[test]
    fn a_finished_turn_tracks_an_asked_goal_and_skips_small_talk() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("goals-turn");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        cabin.read_turn_goals("what's the weather like", "Sunny all day.");
        assert!(!root.join(objectives::OBJECTIVES_FILE).exists());
        cabin.read_turn_goals("add this goal: 99 fishing in osrs", "On it.");
        let o = objectives::load(&root);
        assert_eq!(o.goals.len(), 1);
        assert_eq!(o.goals[0].state, GoalState::Tracking);
        assert_eq!(goal_title(&o.goals[0]), "Old School RuneScape · 99 fishing");
        assert!(GOALS_CHANGED.swap(false, Ordering::Relaxed));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_noticed_goal_leaves_a_track_chip_that_tracks_it() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("goals-ask");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut cabin = Cabin::quiet_for_test();
        let reply = "Nice.\nGAME_GOAL: osrs | 99 fishing | skill | 99";
        cabin.read_turn_goals("fishing is relaxing", reply);
        let chips = cabin.goal_chips();
        assert_eq!(chips.len(), 1);
        assert_eq!(chips[0].label, "Track goal: 99 fishing");
        assert!(cabin.take_goal_chip(&chips[0]));
        assert!(cabin.goal_chips().is_empty());
        let o = objectives::load(&root);
        assert_eq!(o.goals[0].state, GoalState::Tracking);
        // The same goal is not asked about again in this chat.
        cabin.read_turn_goals("still fishing", reply);
        assert!(cabin.goal_chips().is_empty());
        GOALS_CHANGED.store(false, Ordering::Relaxed);
        let _ = std::fs::remove_dir_all(&root);
    }
}
