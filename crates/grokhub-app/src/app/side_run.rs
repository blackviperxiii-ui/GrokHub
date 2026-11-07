//! Side answers: a question typed while a reply runs gets its own read-only
//! run on a copy of the chat, beside the work. The main reply, its history and
//! its session are never touched. The answer shows under the chat as an
//! "On the side" card and never enters the engine history; **Tell the main
//! task** feeds it in when the user wants that.

use super::*;
use grokhub_agent::route::intent::TurnRoute;

/// Finished side answers kept on screen per chat. Older ones drop off.
pub(super) const SIDE_KEEP: usize = 3;
/// How much of the main task's ask the side run is shown.
const ASK_CLIP: usize = 400;
/// How much of the reply so far the side run is shown, from its end.
const SO_FAR_TAIL: usize = 600;

pub(super) struct SideRun {
    pub id: u64,
    pub thread_id: String,
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

fn clip_start(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}

fn clip_end(text: &str, max: usize) -> String {
    let text = text.trim();
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let cut: String = text.chars().skip(n - max).collect();
    format!("…{cut}")
}

/// The side run's own prompt: the question plus a snapshot of the work in
/// flight, so it can answer about it without continuing it.
pub(super) fn side_prompt(question: &str, ask: &str, so_far: &str) -> String {
    let mut out = String::from(
        "You are answering a quick side question while the main task in this chat keeps working. \
         Answer only the side question, briefly and plainly. Do not continue, change or redo the \
         main task, and do not start any work of your own. You can read files and look things up; \
         you cannot change anything.\n",
    );
    if !ask.trim().is_empty() {
        out.push_str(&format!(
            "\nThe main task is working on: {}\n",
            clip_start(ask, ASK_CLIP)
        ));
    }
    if !so_far.trim().is_empty() {
        out.push_str(&format!(
            "\nIts reply so far ends with: {}\n",
            clip_end(so_far, SO_FAR_TAIL)
        ));
    }
    out.push_str(&format!("\nSide question: {}", question.trim()));
    out
}

/// What **Tell the main task** feeds the main work: the question and the
/// answer it got, so the main task can use both.
pub(super) fn tell_main_text(question: &str, answer: &str) -> String {
    let answer = answer.trim();
    if answer.is_empty() {
        question.trim().to_string()
    } else {
        format!(
            "I asked on the side: {}\nThe answer was: {}\nTake this into account.",
            question.trim(),
            clip_start(answer, 1_200)
        )
    }
}

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
        let model = grokhub_agent::DEFAULT_MODEL.to_string();
        std::thread::spawn(move || {
            let _origin =
                grokhub_agent::harness::OriginScope::enter(grokhub_agent::harness::Origin::User);
            super::background::run_native_bg(
                session, workspace, client, auth_kind, bearer, gate, model, effort, system, prompt,
                run_cancel, tx,
            );
        });
        self.side.next_id += 1;
        self.side.runs.push(SideRun {
            id: self.side.next_id,
            thread_id,
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
                moved = true;
            }
        }
        if moved {
            self.trim_side_answers();
        }
        moved
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
                    ui.spacing_mut().item_spacing.x = 6.0;
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
