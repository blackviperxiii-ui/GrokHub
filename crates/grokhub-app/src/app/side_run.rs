//! Side answers: a question typed while a reply runs gets its own read-only
//! run on a copy of the chat, beside the work. The main reply, its history and
//! its session are never touched. The answer shows under the chat as an
//! "On the side" card and never enters the engine history; **Tell the main
//! task** feeds it in when the user wants that. A side run that fails or ends
//! with no answer puts the question in line as its own turn, so it is still
//! answered in the chat. A casual chat never answers on the side unasked
//! (`intent::TurnCx::casual`).

use super::*;
use grokhub_agent::route::intent::TurnRoute;

/// Finished side answers kept on screen per chat. Older ones drop off.
pub(super) const SIDE_KEEP: usize = 3;
pub(super) struct SideRun {
    pub id: u64,
    pub thread_id: String,
    /// The forked copy the run works on; deleted once it ends, so side
    /// answers never show in History.
    pub session: String,
    pub question: String,
    pub answer: String,
    /// Some while the run is going.
    pub rx: Option<mpsc::Receiver<grokhub_core::wire::GrokPEvent>>,
    pub cancel: grokhub_agent::CancelToken,
    pub failed: Option<String>,
}

impl SideRun {
    pub fn live(&self) -> bool {
        self.rx.is_some()
    }
}

#[derive(Default)]
pub(super) struct SideRuns {
    pub runs: Vec<SideRun>,
    pub next_id: u64,
}

impl SideRuns {
    pub fn live_count(&self) -> usize {
        self.runs.iter().filter(|r| r.live()).count()
    }
}

/// What a side card offers once its answer is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SideAct {
    Tell(u64),
    Dismiss(u64),
    Stop(u64),
}

use grokhub_agent::route::intent::clip_start;
pub(super) use grokhub_agent::route::intent::{side_prompt, tell_main_text};

impl Cabin {
    /// Start a read-only side run for `question` on a copy of the chat on screen.
    pub(super) fn start_side_run(&mut self, question: String) -> Result<(), String> {
        let thread_id = self.visible_thread_id();
        let (cwd, parent) = {
            let thread = self.threads.iter().find(|t| t.id == thread_id);
            (
                thread
                    .and_then(|t| t.grok_cwd.clone())
                    .filter(|s| !s.trim().is_empty()),
                thread
                    .and_then(|t| t.grok_session.clone())
                    .filter(|s| !s.trim().is_empty()),
            )
        };
        let workspace = cwd
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.grok_cwd());
        let fresh = || format!("native-{}", grokhub_core::uid("side"));
        // A fork copies the saved chat; the main session is never written.
        let session = match parent.as_deref() {
            Some(parent) => grokhub_agent::fork_session(parent)
                .map(|info| info.id)
                .unwrap_or_else(|_| fresh()),
            None => fresh(),
        };
        let (client, auth_kind, bearer) = super::background::native_bg_model(self)?;
        let effort =
            grokhub_agent::route::live::start_effort(grokhub_agent::route::live::DEFAULT_CLASS);
        let (stable, volatile) = self.chat_prompt_parts();
        let need = grokhub_agent::prompt_need_from_job(&question);
        let system = grokhub_agent::system_prompt_with(
            grokhub_agent::PromptProfile::Background,
            &stable,
            &volatile,
            &workspace,
            need,
        );
        let ask = last_user_scan(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())))
            .unwrap_or_default();
        let so_far = self
            .messages
            .last()
            .filter(|m| m.0 == "assistant")
            .map(|m| m.1.clone())
            .unwrap_or_default();
        let prompt = side_prompt(&question, &ask, &so_far);
        let mode = match self.permission_mode {
            PermissionMode::Ask => grokhub_agent::PermMode::Ask,
            PermissionMode::Auto => grokhub_agent::PermMode::Auto,
            PermissionMode::AlwaysApprove => grokhub_agent::PermMode::Always,
        };
        // Reads run without a prompt; anything that would change something is refused.
        let gate = grokhub_agent::Gate {
            mode,
            readonly_session: true,
            attended: false,
            desktop: false,
        };
        let cancel = grokhub_agent::CancelToken::new();
        let _ = grokhub_agent::hub_for(&session);
        grokhub_agent::watch_cancel(&session, cancel.clone());
        let (tx, rx) = mpsc::channel();
        let run_cancel = cancel.clone();
        let run_session = session.clone();
        let model = grokhub_agent::DEFAULT_MODEL.to_string();
        std::thread::spawn(move || {
            let _origin =
                grokhub_agent::harness::OriginScope::enter(grokhub_agent::harness::Origin::User);
            super::background::run_native_bg(
                run_session,
                workspace,
                client,
                auth_kind,
                bearer,
                gate,
                model,
                effort,
                system,
                prompt,
                run_cancel,
                tx,
            );
        });
        self.side.next_id += 1;
        self.side.runs.push(SideRun {
            id: self.side.next_id,
            thread_id,
            session,
            question,
            answer: String::new(),
            rx: Some(rx),
            cancel,
            failed: None,
        });
        Ok(())
    }

    /// Read what each side run said since the last frame. Their answers post
    /// at once: they never wait for the main reply, which they don't touch.
    pub(super) fn poll_side_runs(&mut self) -> bool {
        use grokhub_core::wire::GrokPEvent;
        let mut moved = false;
        let mut unanswered = Vec::new();
        for run in &mut self.side.runs {
            let Some(rx) = run.rx.as_ref() else {
                continue;
            };
            let mut ended = false;
            loop {
                match rx.try_recv() {
                    Ok(GrokPEvent::Text(text)) => {
                        run.answer.push_str(&text);
                        moved = true;
                    }
                    Ok(GrokPEvent::End(turn)) => {
                        if run.answer.trim().is_empty() {
                            run.answer = turn.text;
                        }
                        ended = true;
                    }
                    Ok(GrokPEvent::Err(err)) => {
                        run.failed = Some(err);
                        ended = true;
                    }
                    Ok(_) => {}
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        ended = true;
                        break;
                    }
                }
                if ended {
                    break;
                }
            }
            if ended {
                run.rx = None;
                let _ = grokhub_agent::delete_session(&run.session);
                moved = true;
                if run.answer.trim().is_empty() {
                    unanswered.push(run.id);
                }
            }
        }
        if !unanswered.is_empty() {
            self.requeue_unanswered_side(&unanswered);
        }
        if moved {
            self.trim_side_answers();
        }
        moved
    }

    /// Side runs that ended with no answer: each question leaves its card
    /// and waits in its chat's line as its own turn, word for word.
    fn requeue_unanswered_side(&mut self, ids: &[u64]) {
        let (gone, kept): (Vec<SideRun>, Vec<SideRun>) = std::mem::take(&mut self.side.runs)
            .into_iter()
            .partition(|r| ids.contains(&r.id));
        self.side.runs = kept;
        for run in gone {
            self.followup_queue.push(&run.thread_id, run.question);
        }
        self.status = "Couldn't answer that on the side, so it's next in line".into();
    }

    /// Keep the newest finished answers per chat; live runs always stay.
    fn trim_side_answers(&mut self) {
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut keep = vec![true; self.side.runs.len()];
        for (i, run) in self.side.runs.iter().enumerate().rev() {
            if run.live() {
                continue;
            }
            let n = seen.entry(run.thread_id.clone()).or_default();
            *n += 1;
            if *n > SIDE_KEEP {
                keep[i] = false;
            }
        }
        let mut i = 0;
        self.side.runs.retain(|_| {
            let k = keep[i];
            i += 1;
            k
        });
    }

    pub(super) fn stop_side_run(&mut self, id: u64) {
        if let Some(run) = self.side.runs.iter_mut().find(|r| r.id == id) {
            run.cancel.cancel();
            run.rx = None;
            let _ = grokhub_agent::delete_session(&run.session);
            if run.answer.trim().is_empty() {
                run.failed = Some("Stopped".into());
            }
        }
        self.status = "Stopped the side answer".into();
    }

    pub(super) fn apply_side_act(&mut self, act: SideAct) {
        match act {
            SideAct::Stop(id) => self.stop_side_run(id),
            SideAct::Dismiss(id) => self.side.runs.retain(|r| r.id != id),
            SideAct::Tell(id) => {
                let Some(pos) = self.side.runs.iter().position(|r| r.id == id) else {
                    return;
                };
                let run = self.side.runs.remove(pos);
                // Next time a message shaped like this one steers the work.
                grokhub_agent::route::intent::note_correction(
                    &crate::config::config_dir(),
                    &run.question,
                    true,
                    TurnRoute::Steer,
                );
                let text = tell_main_text(&run.question, &run.answer);
                if self.running && self.can_steer_live_turn() {
                    self.steer_native_live(text);
                } else {
                    self.forced_route = Some(TurnRoute::Chat);
                    self.send_chat(text);
                }
            }
        }
    }

    /// "On the side" cards for the chat on screen, right above the box.
    pub(super) fn paint_side_answers(&mut self, ui: &mut egui::Ui) {
        let here = self.visible_thread_id();
        let mut act = None;
        for run in self.side.runs.iter().filter(|r| r.thread_id == here) {
            crate::ui_kit::card_frame(ui, crate::ui_kit::CardVariant::Outlined, false, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("On the side")
                            .size(crate::theme::FONT_META)
                            .strong()
                            .color(crate::theme::muted()),
                    );
                    ui.label(
                        RichText::new(clip_start(&run.question, 120))
                            .size(crate::theme::FONT_META)
                            .color(crate::theme::muted()),
                    );
                });
                let body = match (&run.failed, run.answer.trim().is_empty(), run.live()) {
                    (Some(err), true, _) if err == "Stopped" => "Stopped.".to_string(),
                    (Some(_), true, _) => "Couldn't answer that on the side.".to_string(),
                    (_, true, true) => "Answering…".to_string(),
                    _ => run.answer.trim().to_string(),
                };
                ui.label(
                    RichText::new(body)
                        .size(crate::theme::FONT_BODY)
                        .color(crate::theme::fg()),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if run.live() {
                        if crate::cards::ghost_pill(ui, "Stop") {
                            act = Some(SideAct::Stop(run.id));
                        }
                    } else {
                        if !run.answer.trim().is_empty()
                            && crate::cards::ghost_pill(ui, "Tell the main task")
                        {
                            act = Some(SideAct::Tell(run.id));
                        }
                        if crate::cards::ghost_pill(ui, "Dismiss") {
                            act = Some(SideAct::Dismiss(run.id));
                        }
                    }
                });
            });
            ui.add_space(4.0);
        }
        if let Some(act) = act {
            self.apply_side_act(act);
        }
    }
}
