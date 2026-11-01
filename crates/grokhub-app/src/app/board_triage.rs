//! Assign, flows and triage on a workboard card: "Agent: do this" now or
//! queued, a playbook or recipe started on the card, Make it recurring, the
//! priority chip, Block with a reason and Unblock, your own edit of the steps,
//! and a drop on a card that places it above that card. The rules live in
//! `grokhub_core::work_triage`, which the shell shares.
//!
//! The queue: once a second the cabin looks at the oldest queued card. It
//! starts as a background run on its card only when no chat turn of yours is
//! running, no other queued run is live, there is room under `BG_TASK_MAX`,
//! the agent is ready, and the game guard is clear. The guard must read clear
//! for a few seconds first, so a stale reading never starts one mid-game.

use super::background::BgSession;
use super::pages::BoardAct;
use super::work_log::log_line;
use super::*;
use grokhub_core::work_resume as res;
use grokhub_core::work_run as wr;
use grokhub_core::work_triage::{self as wt, CardFlow, FlowKind, QueueGate};
use grokhub_core::{BgOrigin, KanbanColumn};

/// How often the queue looks at the board.
const QUEUE_TICK_MS: u64 = 1_000;
/// How long everything must read clear before a queued card starts.
pub(super) const QUEUE_SETTLE_MS: u64 = 3_000;
/// After a start fails (no model, a refused run), wait this long to try again.
const QUEUE_RETRY_MS: u64 = 30_000;
/// Most recipes or playbooks the Start a flow menu lists.
const FLOW_MENU_MAX: usize = 12;

/// The queue and the open triage editors. Memory only.
#[derive(Clone, Debug, Default)]
pub(super) struct TriageView {
    /// `(run id, card id)`: the background run the queue started.
    pub queued_run: Option<(u64, String)>,
    /// Why the next queued card waits, shown on queued cards.
    pub wait: Option<String>,
    /// When the queue last read everything clear; a start waits for
    /// [`QUEUE_SETTLE_MS`] of it.
    pub clear_since: Option<u64>,
    /// The next time the queue looks.
    pub next_ms: u64,
    /// A card whose blocker you are typing, and the text so far.
    pub blocking: Option<(String, String)>,
    /// A card whose steps you are editing, and the text so far.
    pub steps: Option<(String, String)>,
    /// Saved recipes and playbooks for Start a flow, read when the menu first
    /// opens: (recipe ids, (playbook id, name)).
    pub flows: Option<(Vec<String>, Vec<(String, String)>)>,
}

/// The game guard's word for the queue: wait while an anti-cheat or a game
/// runs. Nothing else in the guard holds the queue back.
pub(super) fn queue_game_wait(g: &grokhub_core::game_guard::GameGuard) -> Option<String> {
    if let Some(ac) = g.anticheat {
        return Some(format!(
            "Queued work waits while {ac} is running so it doesn't flag this PC."
        ));
    }
    g.gaming
        .then(|| "Queued work waits while you're gaming.".to_string())
}

/// Saved recipe ids, newest name order, without the "last" replay slot.
fn saved_recipes() -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(crate::recipes::dir()) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = dir
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".json").map(str::to_string)
        })
        .filter(|id| id != "last" && !id.is_empty())
        .collect();
    ids.sort();
    ids.truncate(FLOW_MENU_MAX);
    ids
}

impl Cabin {
    /// Each frame: the queue's look, at most once a second.
    pub(super) fn tick_board_queue(&mut self, now: u64) {
        if now < self.board_view.triage.next_ms {
            return;
        }
        self.board_view.triage.next_ms = now + QUEUE_TICK_MS;
        if wt::next_queued(&self.board).is_none() && self.board_view.triage.queued_run.is_none() {
            self.board_view.triage.wait = None;
            self.board_view.triage.clear_since = None;
            return;
        }
        let game = queue_game_wait(&grokhub_desktop::game_guard::peek());
        let ready = self.agent_ready();
        if self.step_board_queue(now, game, ready, |cabin, card| {
            cabin.start_queued_card(card)
        }) == Some(false)
        {
            self.board_view.triage.next_ms = now + QUEUE_RETRY_MS;
        }
    }

    /// One look at the queue. `start` starts the card and returns its run id.
    /// `None`: nothing started; `Some(true)`: a card started; `Some(false)`:
    /// the start failed.
    pub(super) fn step_board_queue(
        &mut self,
        now: u64,
        game: Option<String>,
        agent_ready: bool,
        start: impl FnOnce(&mut Self, &BoardCard) -> Result<u64, String>,
    ) -> Option<bool> {
        if let Some((run, _)) = self.board_view.triage.queued_run.clone() {
            if !self.bg.runs.iter().any(|r| r.id == run && r.live()) {
                self.board_view.triage.queued_run = None;
            }
        }
        let card = wt::next_queued(&self.board).cloned()?;
        let gate = QueueGate {
            foreground: self.running,
            queued_live: self.board_view.triage.queued_run.is_some(),
            bg_live: self.bg.live_count(),
            game,
            agent_ready,
        };
        let t = &mut self.board_view.triage;
        if let Some(why) = wt::queue_wait(&gate) {
            t.wait = Some(why);
            t.clear_since = None;
            return None;
        }
        let since = *t.clear_since.get_or_insert(now);
        if now.saturating_sub(since) < QUEUE_SETTLE_MS {
            t.wait = Some("Starts in a moment.".into());
            return None;
        }
        t.clear_since = None;
        match start(self, &card) {
            Ok(run) => {
                let t = &mut self.board_view.triage;
                t.queued_run = Some((run, card.id.clone()));
                t.wait = None;
                Some(true)
            }
            Err(why) => {
                self.board_view.triage.wait = Some(why);
                Some(false)
            }
        }
    }

    /// Start a queued card as a background run on its own card and chat.
    fn start_queued_card(&mut self, card: &BoardCard) -> Result<u64, String> {
        let thread_id = match card
            .thread_id
            .clone()
            .filter(|tid| self.threads.iter().any(|t| &t.id == tid))
        {
            Some(tid) => tid,
            None => self.make_card_thread(&card.title, false),
        };
        let task = grokhub_core::card_work_prompt(card);
        let gate = self.native_bg_gate();
        let (run_id, _, native) =
            self.start_native_bg_on(&task, &thread_id, BgOrigin::User, BgSession::Fork, gate)?;
        if let Some(run) = self.bg.runs.iter_mut().find(|r| r.id == run_id) {
            run.title = card.title.clone();
        }
        let model = self.run_model();
        let now = now_ms();
        wt::start_queued(&mut self.board, &card.id);
        let mode = self.permission_mode;
        if let Some(c) = self.board.iter_mut().find(|c| c.id == card.id) {
            c.thread_id = Some(thread_id);
            c.notes_sent = grokhub_core::card_notes_hash(&c.notes);
            let kind = c.kind;
            wr::begin_run(c, kind, &native, &model, now);
            res::claim_run(c, res::HOST_CABIN, mode);
        }
        self.bg.cards.push((run_id, card.id.clone()));
        log_line(&card.id, wr::LOG_START, "started from the queue");
        self.flush_board();
        self.persist();
        Ok(run_id)
    }

    /// Agent: do this. Now is Work on it; Queue waits for the queue.
    pub(super) fn assign_card(&mut self, id: &str, queue: bool) -> bool {
        let now = now_ms();
        let done = if queue {
            wt::queue_card(&mut self.board, id, now).map(|_| ())
        } else {
            wt::assign_now(&mut self.board, id)
        };
        if let Err(why) = done {
            self.board_view.note = Some((id.to_string(), why));
            return false;
        }
        if queue {
            self.board_view.triage.next_ms = 0;
            log_line(id, wr::LOG_NOTE, "queued for the agent");
            return true;
        }
        self.board_view.open = Some(id.to_string());
        self.say_on_card(id, "");
        false
    }

    /// Block with the reason typed under the card.
    pub(super) fn block_board_card(&mut self, id: &str, reason: &str) -> bool {
        match wt::block_card(&mut self.board, id, reason, now_ms()) {
            Ok(changed) => {
                self.board_view.triage.blocking = None;
                if changed {
                    log_line(id, wr::LOG_NOTE, &format!("blocked: {}", reason.trim()));
                }
                changed
            }
            Err(why) => {
                self.board_view.note = Some((id.to_string(), why));
                false
            }
        }
    }

    /// Save your edit of the steps.
    pub(super) fn save_board_steps(&mut self, id: &str, text: &str) -> bool {
        self.board_view.triage.steps = None;
        let now = now_ms();
        match self.board.iter_mut().find(|c| c.id == id) {
            Some(c) => wt::edit_steps(c, text, now),
            None => false,
        }
    }

    /// Start a recipe or a playbook on the card, linked to it. A recipe drives
    /// the desktop, so it asks the game guard first. A playbook is picked by
    /// the agent from the work, so it starts as Work on it with the link kept.
    pub(super) fn start_card_flow(&mut self, id: &str, flow: CardFlow) -> bool {
        if let Err(why) = wt::assign_now(&mut self.board, id) {
            self.board_view.note = Some((id.to_string(), why));
            return false;
        }
        match flow.kind {
            FlowKind::Recipe => {
                if let Some(why) = crate::desktop_mcp::game_guard()
                    .blocks(grokhub_desktop::game_guard::Guarded::DesktopControl)
                {
                    self.board_view.note = Some((id.to_string(), why));
                    return false;
                }
                if !self.replay_saved_recipe(&flow.id) {
                    self.board_view.note = Some((id.to_string(), self.status.clone()));
                    return false;
                }
                let now = now_ms();
                wt::link_flow(&mut self.board, id, flow.clone());
                wt::place_card(&mut self.board, id, KanbanColumn::Doing, None, now);
                log_line(id, wr::LOG_START, &format!("started {}", flow.line()));
                true
            }
            FlowKind::Playbook => {
                wt::link_flow(&mut self.board, id, flow.clone());
                log_line(id, wr::LOG_NOTE, &format!("linked {}", flow.line()));
                self.board_view.open = Some(id.to_string());
                self.say_on_card(id, "");
                false
            }
            FlowKind::Automation => false,
        }
    }

    /// Make it recurring: a new automation from the card's title, which then
    /// owns the card. Its runs report back here.
    pub(super) fn make_card_recurring(&mut self, id: &str, choice: usize) -> bool {
        let Some(card) = self.board.iter().find(|c| c.id == id) else {
            return false;
        };
        let Some(seed) = wt::recurring_seed(choice, card) else {
            return false;
        };
        let before = self.automations.len();
        let status = self
            .save_schedule(&seed)
            .unwrap_or_else(|| "That schedule did not read".into());
        let added = (self.automations.len() > before)
            .then(|| self.automations.last())
            .flatten()
            .map(|a| (a.id.clone(), a.name.clone()));
        match added {
            Some((auto_id, name)) => {
                wt::own_by_automation(&mut self.board, id, &auto_id, &name);
                log_line(id, wr::LOG_NOTE, &status);
                self.board_view.note = Some((id.to_string(), status));
                true
            }
            None => {
                self.board_view.note = Some((id.to_string(), status));
                false
            }
        }
    }

    fn flow_choices(&mut self) -> (Vec<String>, Vec<(String, String)>) {
        if let Some(f) = self.board_view.triage.flows.clone() {
            return f;
        }
        let (all, _) = grokhub_agent::brain::playbook::load(&crate::config::config_dir());
        let playbooks: Vec<(String, String)> = all
            .into_iter()
            .take(FLOW_MENU_MAX)
            .map(|l| (l.playbook.id, l.playbook.name))
            .collect();
        let f = (saved_recipes(), playbooks);
        self.board_view.triage.flows = Some(f.clone());
        f
    }

    /// The card menu's assign, flow and triage rows.
    pub(super) fn board_triage_menu(
        &mut self,
        ui: &mut egui::Ui,
        card: &BoardCard,
        chosen: &mut Option<BoardAct>,
    ) {
        let id = card.id.clone();
        let open = !matches!(card.status, BoardStatus::Done | BoardStatus::Blocked);
        if open {
            ui.menu_button("Agent: do this", |ui| {
                if ui.button("Now").clicked() {
                    *chosen = Some(BoardAct::Assign {
                        id: id.clone(),
                        queue: false,
                    });
                }
                if wt::is_queued(card) {
                    if ui.button("Take off the queue").clicked() {
                        *chosen = Some(BoardAct::Unqueue(id.clone()));
                    }
                } else if card.status != BoardStatus::InProgress
                    && ui
                        .button("Queue")
                        .on_hover_text(
                            "Starts on its own once nothing else is running and no game is up",
                        )
                        .clicked()
                {
                    *chosen = Some(BoardAct::Assign {
                        id: id.clone(),
                        queue: true,
                    });
                }
            });
            ui.menu_button("Start a flow", |ui| {
                let (recipes, playbooks) = self.flow_choices();
                if recipes.is_empty() && playbooks.is_empty() {
                    ui.label(
                        RichText::new("No recipes or playbooks saved yet.")
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::muted()),
                    );
                }
                for r in &recipes {
                    if ui.button(format!("Recipe · {r}")).clicked() {
                        *chosen = Some(BoardAct::Flow {
                            id: id.clone(),
                            flow: CardFlow::new(FlowKind::Recipe, r, r),
                        });
                    }
                }
                for (pid, name) in &playbooks {
                    if ui.button(format!("Playbook · {name}")).clicked() {
                        *chosen = Some(BoardAct::Flow {
                            id: id.clone(),
                            flow: CardFlow::new(FlowKind::Playbook, pid, name),
                        });
                    }
                }
            });
            if card.automation.is_none() {
                ui.menu_button("Make it recurring", |ui| {
                    for (i, (label, _)) in wt::RECUR_CHOICES.iter().enumerate() {
                        if ui.button(*label).clicked() {
                            *chosen = Some(BoardAct::Recur {
                                id: id.clone(),
                                choice: i,
                            });
                        }
                    }
                });
            }
        }
        ui.menu_button(
            format!("Priority: {}", wt::priority_label(&card.priority)),
            |ui| {
                for p in wt::PRIORITIES {
                    let label = wt::priority_label(p);
                    if ui.selectable_label(card.priority == p, label).clicked() {
                        *chosen = Some(BoardAct::Priority {
                            id: id.clone(),
                            priority: p,
                        });
                    }
                }
            },
        );
        if card.status == BoardStatus::Blocked {
            if ui.button("Unblock").clicked() {
                *chosen = Some(BoardAct::Unblock(id.clone()));
            }
        } else if card.status != BoardStatus::Dismissed && ui.button("Block…").clicked() {
            *chosen = Some(BoardAct::AskBlock(id.clone()));
        }
        let steps_label = if card.steps.is_empty() {
            "Add steps"
        } else {
            "Edit steps"
        };
        if ui.button(steps_label).clicked() {
            *chosen = Some(BoardAct::EditSteps(id.clone()));
        }
    }

    /// Under an open card: the queue line, the flow, and the block or steps
    /// editor while one is open.
    pub(super) fn paint_card_triage(
        &mut self,
        ui: &mut egui::Ui,
        card: &BoardCard,
        act: &mut Option<BoardAct>,
    ) {
        let id = card.id.clone();
        let tip = |ui: &mut egui::Ui, text: &str| {
            ui.add(
                egui::Label::new(
                    RichText::new(text)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::muted()),
                )
                .wrap(),
            );
        };
        if let Some(place) = wt::queue_place(&self.board, &id) {
            let mut line = wt::queue_label(place);
            if let Some(why) = self.board_view.triage.wait.as_ref().filter(|_| place == 1) {
                line = format!("{line} · {why}");
            }
            tip(ui, &line);
        }
        if let Some(flow) = card.flow.as_ref() {
            tip(ui, &flow.line());
        }
        let blocking = self
            .board_view
            .triage
            .blocking
            .as_ref()
            .filter(|(b, _)| *b == id)
            .map(|(_, t)| t.clone());
        if let Some(mut text) = blocking {
            tip(ui, "What is it waiting on?");
            let field = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .id(egui::Id::new(("board-block", id.as_str())))
                    .hint_text("Waiting on…")
                    .desired_width(f32::INFINITY),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            self.board_view.triage.blocking = Some((id.clone(), text.clone()));
            ui.horizontal(|ui| {
                if (crate::cards::white_pill(ui, "Block") || enter) && !text.trim().is_empty() {
                    *act = Some(BoardAct::Block {
                        id: id.clone(),
                        reason: text.clone(),
                    });
                }
                if crate::cards::ghost_pill(ui, "Cancel") {
                    *act = Some(BoardAct::CancelTriage);
                }
            });
        }
        let steps = self
            .board_view
            .triage
            .steps
            .as_ref()
            .filter(|(s, _)| *s == id)
            .map(|(_, t)| t.clone());
        if let Some(mut text) = steps {
            tip(
                ui,
                "One step a line. [x] done, [>] doing, [-] skipped. The agent works from your version.",
            );
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .id(egui::Id::new(("board-steps", id.as_str())))
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            self.board_view.triage.steps = Some((id.clone(), text.clone()));
            ui.horizontal(|ui| {
                if crate::cards::white_pill(ui, "Save steps") {
                    *act = Some(BoardAct::SaveSteps {
                        id: id.clone(),
                        text: text.clone(),
                    });
                }
                if crate::cards::ghost_pill(ui, "Cancel") {
                    *act = Some(BoardAct::CancelTriage);
                }
            });
        }
    }
}
