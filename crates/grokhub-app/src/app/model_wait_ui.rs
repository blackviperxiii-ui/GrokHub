//! Router: a no-route pause heals itself ([`grokhub_agent::route::wait`]).
//! The step that hit "no model in your plan is answering" waits. The
//! heartbeat re-checks the models on a backoff and, once one answers,
//! resumes the paused chat turn on its own with one quiet note naming the
//! model and the step. The Home card only informs: it names the models and
//! the step, with Retry now. Nothing outside your plan is ever tried, and
//! the budget pause is not this: it still waits for you.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use grokhub_agent::route::live::NO_ROUTE_MSG;
use grokhub_agent::route::wait::{self, NoRouteWait};
use grokhub_agent::AuthKind;
use grokhub_core::model_registry::ModelMeta;

use super::*;

/// The same model pausing again this soon after a resume keeps its backoff place.
const CARRY_FOR: Duration = Duration::from_secs(600);

#[derive(Debug, Default)]
pub(super) struct ModelWaitUi {
    pub wait: Option<NoRouteWait>,
    /// A check out now: the model that answers, or `None`.
    pub rx: Option<mpsc::Receiver<Option<String>>>,
    /// The waiting card on Home.
    pub card: Option<String>,
    /// The chat whose turn paused on no route.
    pub paused_turn: Option<String>,
    /// A model came back (model, step): resume `paused_turn` once that chat is open and idle.
    pub resume: Option<(String, String)>,
    /// (model, attempts, when the wait ended).
    carry: Option<(String, u32, Instant)>,
}

impl Cabin {
    /// A job failed: remember the chat when it was a no-route pause.
    pub(super) fn note_no_route_turn(&mut self, err: &str) {
        if err.contains(NO_ROUTE_MSG) {
            self.harness.router.wait.paused_turn = Some(self.chat_job_thread.clone().unwrap_or_else(|| self.visible_thread_id()));
        }
    }

    /// The newest ask in `thread` (the open chat when `None` or open).
    fn last_ask_in(&self, thread: Option<&str>) -> String {
        let vis = self.visible_thread_id();
        let ask = |msgs: &[(String, String)]| msgs.iter().rev().find(|(role, text)| role == "user" && !is_workload_user(text)).map(|(_, text)| text.clone()).unwrap_or_default();
        match thread.filter(|id| *id != vis) {
            None => ask(&self.messages),
            Some(id) => self.threads.iter().find(|t| t.id == id).map(|t| ask(&t.messages)).unwrap_or_default(),
        }
    }

    /// No model in your plan answered `class`'s call to `model`: wait, post the card.
    pub(super) fn start_model_wait(&mut self, class: &str, model: &str, now: u64) {
        let model = model.trim();
        if self.harness.router.wait.wait.is_some() {
            return;
        }
        let ask = if class.starts_with("chat:") { self.last_ask_in(self.harness.router.wait.paused_turn.as_deref()) } else { String::new() };
        let step = wait::step_name(class, &ask);
        let attempts = match &self.harness.router.wait.carry {
            Some((m, a, at)) if m == model && at.elapsed() < CARRY_FOR => *a,
            _ => 0,
        };
        let (reg, _) = grokhub_agent::route::live::snapshot(&crate::config::config_dir());
        let clock = Self::local_clock();
        let since = grokhub_core::automation::format_ampm(clock.hour * 60 + clock.minute);
        let card = model_wait_card(&wait::waiting_models(&reg, model), model, &since, &step, now);
        let id = card.id.clone();
        self.status = card.title.clone();
        self.post_feed_card(card);
        let w = &mut self.harness.router.wait;
        w.card = self.updates.iter().any(|c| c.id == id).then_some(id);
        w.wait = Some(NoRouteWait::new(class, model, &step, now, attempts));
    }

    /// Retry now on the waiting card.
    pub(super) fn retry_model_wait_now(&mut self) {
        if let Some(w) = self.harness.router.wait.wait.as_mut() {
            w.retry_now(now_ms());
            self.status = "Checking the models now…".into();
        }
    }

    /// Heartbeat: read a check that came back, resume a paused turn, start the next check when due.
    pub(super) fn poll_model_wait(&mut self, halted: bool) {
        let now = now_ms();
        if let Some(rx) = self.harness.router.wait.rx.take() {
            match rx.try_recv() {
                Ok(Some(back)) => self.end_model_wait(&back),
                Ok(None) | Err(mpsc::TryRecvError::Disconnected) => {
                    if let Some(w) = self.harness.router.wait.wait.as_mut() {
                        w.missed(now);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => self.harness.router.wait.rx = Some(rx),
            }
        }
        self.resume_paused_turn();
        if halted || self.harness.router.wait.rx.is_some() {
            return;
        }
        let Some(model) = self.harness.router.wait.wait.as_ref().filter(|w| w.due(now)).map(|w| w.model.clone()) else {
            return;
        };
        // Pings go on the plan sign-in only: an included call, never money.
        let bearer = match self.native_cred() {
            Ok((b, AuthKind::OAuth)) => Some(b),
            _ => None,
        };
        let dir = crate::config::config_dir();
        let (tx, rx) = mpsc::channel();
        self.harness.router.wait.rx = Some(rx);
        std::thread::spawn(move || {
            let mut ping = |m: &ModelMeta| bearer.as_deref().is_some_and(|b| grokhub_agent::route::sources::ping_model(&dir, b, m));
            let _ = tx.send(wait::recheck(&dir, &model, now, &mut ping));
        });
    }

    /// `back` answers: the card goes, and the paused turn resumes (or the note says it's back).
    fn end_model_wait(&mut self, back: &str) {
        let w = &mut self.harness.router.wait;
        let Some(done) = w.wait.take() else {
            return;
        };
        w.carry = Some((done.model.clone(), done.attempts.saturating_add(1), Instant::now()));
        if let Some(id) = w.card.take() {
            self.updates.retain(|c| c.id != id);
            self.persist_updates();
        }
        if self.harness.router.wait.paused_turn.is_some() {
            self.harness.router.wait.resume = Some((back.to_string(), done.step));
            self.resume_paused_turn();
        } else {
            self.quiet_router_note(wait::back_note(back, &done.step, false));
        }
    }

    /// Resume the paused chat turn once a model is back and that chat is open and idle.
    fn resume_paused_turn(&mut self) {
        let w = &self.harness.router.wait;
        let (Some((model, step)), Some(tid)) = (w.resume.clone(), w.paused_turn.clone()) else {
            return;
        };
        if self.running || tid != self.visible_thread_id() {
            return;
        }
        self.harness.router.wait.resume = None;
        self.harness.router.wait.paused_turn = None;
        let paused = self.messages.last().is_some_and(|m| m.0 == "assistant" && m.1.contains(NO_ROUTE_MSG));
        let ask = self.last_ask_in(None);
        if paused && !ask.is_empty() {
            self.kick_model_retry(ask);
            self.quiet_router_note(wait::back_note(&model, &step, true));
        } else {
            // You moved on in that chat: nothing to resume.
            self.quiet_router_note(wait::back_note(&model, &step, false));
        }
    }

    /// One quiet Work-tree row and the status line; no card, no ask.
    fn quiet_router_note(&mut self, note: String) {
        let rows = &mut self.harness.router.rows;
        rows.retain(|(k, _)| k != "model-back");
        rows.insert(0, ("model-back".into(), note.clone()));
        rows.truncate(super::router_ui::ROUTER_ROWS_MAX);
        self.status = note;
    }
}

/// The waiting card for `model`'s wait.
pub(super) fn model_wait_card(models: &[String], model: &str, since: &str, step: &str, now: u64) -> grokhub_core::UpdateCard {
    let (title, text) = wait::waiting_card(models, since, step);
    grokhub_core::router_update_card(&format!("{}{model}", grokhub_core::MODEL_WAIT_SOURCE_PREFIX), &title, &text, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_cards(app: &Cabin) -> Vec<&grokhub_core::UpdateCard> {
        app.updates.iter().filter(|c| grokhub_core::is_model_wait_card(c)).collect()
    }

    #[test]
    fn a_no_route_turn_waits_on_a_named_card_and_resumes_when_a_model_answers() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("router-wait");
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut app = Cabin::quiet_for_test();
        app.live_mut().push(("user".into(), "Summarize inbox".into()));
        // A budget pause is not a wait: nothing is remembered to resume.
        app.apply_job_fail(grokhub_agent::route::budget::BUDGET_PAUSE_MSG);
        assert_eq!(app.harness.router.wait.paused_turn, None);
        app.chat_job_thread = Some(app.visible_thread_id());
        app.apply_job_fail(&format!("protocol error: {NO_ROUTE_MSG}"));
        assert_eq!(app.harness.router.wait.paused_turn, Some(app.visible_thread_id()));
        app.start_model_wait("chat:default", "grok-4.7", 1_000);
        let cards = wait_cards(&app);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "Waiting for a model: grok-4.7");
        let body = cards[0].body.clone().unwrap();
        assert!(body.starts_with("grok-4.7 isn't answering since ") && body.ends_with("; retrying. “Summarize inbox” picks up on its own once one answers."), "{body}");
        assert_eq!(app.status, "Waiting for a model: grok-4.7");
        // The same pause again keeps the one card and the one wait.
        app.start_model_wait("chat:default", "grok-4.7", 2_000);
        assert_eq!(wait_cards(&app).len(), 1);
        let w = app.harness.router.wait.wait.clone().unwrap();
        assert_eq!((w.step.as_str(), w.attempts, w.next_ms), ("Summarize inbox", 0, 16_000));
        app.retry_model_wait_now();
        assert!(app.harness.router.wait.wait.as_ref().unwrap().due(now_ms()), "Retry now checks at once");
        // grok-4.6 answers: the card goes, the turn goes out again, one quiet note.
        app.end_model_wait("grok-4.6");
        assert!(wait_cards(&app).is_empty());
        let w = &app.harness.router.wait;
        assert_eq!((w.wait.is_none(), w.paused_turn.is_none(), w.resume.is_none()), (true, true, true));
        assert_eq!(app.harness.router.rows.first(), Some(&("model-back".to_string(), "Back on grok-4.6; resumed: Summarize inbox".to_string())));
        // Pausing again right away keeps the backoff's place.
        app.start_model_wait("chat:default", "grok-4.7", 50_000);
        assert_eq!(app.harness.router.wait.wait.as_ref().map(|w| (w.attempts, w.next_ms)), Some((1, 80_000)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_background_wait_ends_with_a_note_and_resumes_no_chat() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("router-wait-bg");
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let mut app = Cabin::quiet_for_test();
        app.start_model_wait("background:dream", "grok-4.7", 1_000);
        let step = grokhub_agent::route::policy::class_row("background:dream").unwrap().plain;
        assert_eq!(app.harness.router.wait.wait.as_ref().map(|w| w.step.as_str()), Some(step));
        app.end_model_wait("grok-4.7");
        assert_eq!(app.status, format!("Back on grok-4.7 for {step}"));
        assert!(wait_cards(&app).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
