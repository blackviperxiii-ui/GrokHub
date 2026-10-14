//! "Record my screen" (card 11, Phase A). `/record [what's wrong]` or the
//! palette starts a consented recording: a still every 2 seconds into
//! `~/GrokHub/recordings/<stamp>`, a red indicator with Stop on top of every
//! page, and a 2-minute cap. On stop a few keyframes and the note go to Grok
//! once; the report lands in the chat that asked and on a feed card. Nothing
//! on the computer is changed. Delete recording asks first.
//!
//! SD2: after a recording finishes, a **Save as skill** offer (same floating
//! chrome as the Stop indicator) runs the watch-and-learn vision call, shows
//! the draft with hard steps flagged, and saves only after the user confirms.

use super::*;
use grokhub_core::fix_plan::ApprovalNeed;
use grokhub_core::screen_record as sr;
use grokhub_core::watch_learn;
use grokhub_core::SkillMd;
use std::sync::Mutex;

/// What the capture thread hands back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RecordDone {
    pub secs: u64,
    pub frames: Vec<PathBuf>,
    pub dir: PathBuf,
    /// Why capture stopped early, if it did.
    pub err: Option<String>,
}

pub(super) struct LiveRecording {
    stop: Arc<AtomicBool>,
    started: Instant,
    note: String,
    chat: String,
    rx: mpsc::Receiver<RecordDone>,
}

/// `(title, folder, chat, reply or error)` from the diagnosis thread.
pub(super) type DiagDone = (String, PathBuf, String, Result<String, String>);

/// Pure helper for tests and the vision-reply path: fixture model text → draft.
pub(super) fn draft_from_reply(note: &str, raw: &str) -> Option<SkillMd> {
    let steps = watch_learn::parse_steps(raw);
    watch_learn::skill_draft(note, &steps)
}

/// A finished recording the user may turn into a skill.
#[derive(Debug, Clone)]
struct WatchOffer {
    dir: PathBuf,
    note: String,
    secs: u64,
    chat: String,
    title: String,
}

#[derive(Debug)]
enum WatchPhase {
    /// Floating **Save as skill** after the recording card posts.
    Offer(WatchOffer),
    /// Vision call in flight.
    Extracting {
        offer: WatchOffer,
        rx: mpsc::Receiver<Result<String, String>>,
    },
    /// Draft ready: Save / Cancel.
    Draft {
        offer: WatchOffer,
        draft: SkillMd,
        hard_lines: Vec<String>,
    },
    /// Vision returned nothing usable.
    Empty(WatchOffer),
}

/// Cabin has no extra field for SD2 (mod.rs owned elsewhere); keep the offer
/// here and drive it from `paint_record_indicator` / `poll_screen_diagnosis`,
/// which mod.rs already calls every frame.
static WATCH_LEARN: Mutex<Option<WatchPhase>> = Mutex::new(None);

/// Tests that touch [`WATCH_LEARN`] hold this so parallel cargo-test threads
/// (and any in-process callers) cannot clobber each other's phase.
#[cfg(test)]
static WATCH_LEARN_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
fn watch_learn_test_guard() -> std::sync::MutexGuard<'static, ()> {
    WATCH_LEARN_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn watch_take() -> Option<WatchPhase> {
    WATCH_LEARN.lock().unwrap_or_else(|p| p.into_inner()).take()
}

fn watch_set(phase: Option<WatchPhase>) {
    *WATCH_LEARN.lock().unwrap_or_else(|p| p.into_inner()) = phase;
}

fn watch_with_mut<R>(f: impl FnOnce(&mut Option<WatchPhase>) -> R) -> R {
    let mut g = WATCH_LEARN.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut g)
}

/// Drop any Save-as-skill chrome tied to a recording folder (e.g. after delete).
fn clear_watch_learn_for_dir(dir: &Path) {
    watch_with_mut(|slot| {
        let same = match slot.as_ref() {
            Some(
                WatchPhase::Offer(o)
                | WatchPhase::Extracting { offer: o, .. }
                | WatchPhase::Draft { offer: o, .. }
                | WatchPhase::Empty(o),
            ) => o.dir == dir,
            None => false,
        };
        if same {
            *slot = None;
        }
    });
}

fn list_frame_paths(dir: &Path) -> Vec<PathBuf> {
    let mut frames = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return frames;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.starts_with("frame-")
            && (name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg"))
        {
            frames.push(p);
        }
    }
    frames.sort();
    frames
}

fn hard_step_lines(draft: &SkillMd) -> Vec<String> {
    watch_learn::parse_steps(&draft.instructions)
        .into_iter()
        .filter(|s| matches!(s.approval, ApprovalNeed::Hard(_)))
        .map(|s| s.description)
        .collect()
}

fn still_ext(mime: &str) -> &'static str {
    if mime.contains("jpeg") || mime.contains("jpg") {
        "jpg"
    } else {
        "png"
    }
}

/// The capture loop, off the UI thread. Takes a still whenever the recorder
/// says one is due, until Stop or the 2-minute cap. A still that cannot be
/// taken or saved ends the recording with what it has.
pub(super) fn record_loop(
    dir: &Path,
    stop: &AtomicBool,
    mut grab: impl FnMut() -> Result<(Vec<u8>, String), String>,
    mut now_ms: impl FnMut() -> u64,
    mut wait: impl FnMut(),
) -> RecordDone {
    let mut done = RecordDone {
        secs: 0,
        frames: Vec::new(),
        dir: dir.to_path_buf(),
        err: None,
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        done.err = Some(format!("could not make {}: {e}", dir.display()));
        return done;
    }
    let mut rec = sr::Recorder::default();
    let _ = rec.start(true, now_ms());
    loop {
        let now = now_ms();
        if stop.load(Ordering::Relaxed) || rec.should_stop(now) {
            break;
        }
        if rec.frame_due(now) {
            let saved = grab().and_then(|(bytes, mime)| {
                let path = dir.join(sr::frame_name(done.frames.len(), still_ext(&mime)));
                std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(path)
            });
            match saved {
                Ok(path) => {
                    done.frames.push(path);
                    rec.note_frame(now);
                }
                Err(e) => {
                    done.err = Some(e);
                    break;
                }
            }
        }
        wait();
    }
    done.secs = rec.stop(now_ms()).map(|(secs, _)| secs).unwrap_or(0);
    done
}

/// One line for the feed card under the recording's title.
pub(super) fn report_summary(report: &sr::ScreenReport) -> String {
    if !report.cause.is_empty() {
        format!("Likely cause: {}", report.cause)
    } else if !report.seen.is_empty() {
        report.seen.clone()
    } else {
        "Nothing stood out in the stills.".into()
    }
}

impl Cabin {
    /// `/record [note]`: needs Settings → Screen recording on.
    pub(super) fn start_screen_recording(&mut self, note: String) {
        if self.screen_rec.is_some() {
            self.status = sr::ALREADY_RECORDING.into();
            return;
        }
        if let Err(why) = sr::Recorder::default().start(self.cfg.screen_record, now_ms()) {
            self.status = why.into();
            self.live_mut()
                .push(("assistant".into(), mark_slash_result(why)));
            self.persist();
            return;
        }
        let Some(home) = grokhub_core::paths::user_home() else {
            self.status = "No home folder to save the recording in.".into();
            return;
        };
        let stamp = format!("{}-{}", Self::local_day(), now_ms() / 1000);
        let dir = sr::recording_dir(&home, &stamp);
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let flag = stop.clone();
        let into = dir.clone();
        std::thread::spawn(move || {
            let grab = crate::desktop_mcp::screen_grabber();
            let done = record_loop(&into, &flag, grab, now_ms, || {
                std::thread::sleep(Duration::from_millis(250))
            });
            let _ = tx.send(done);
        });
        let chat = self.visible_thread_id();
        self.screen_rec = Some(LiveRecording {
            stop,
            started: Instant::now(),
            note: note.trim().to_string(),
            chat,
            rx,
        });
        let line = format!(
            "Recording your screen into {}. Show the problem, then press Stop or send /record stop. It stops on its own at {}.",
            dir.display(),
            sr::clock(sr::MAX_SECS)
        );
        self.live_mut()
            .push(("assistant".into(), mark_slash_result(&line)));
        self.persist();
        self.status = sr::indicator_line(0);
    }

    pub(super) fn stop_screen_recording(&mut self) {
        match &self.screen_rec {
            Some(live) => {
                live.stop.store(true, Ordering::Relaxed);
                self.status = "Stopping the recording…".into();
            }
            None => self.status = "Not recording.".into(),
        }
    }

    /// Frame loop: the capture thread finished (Stop, the cap, or an error).
    pub(super) fn poll_screen_recording(&mut self, ctx: &egui::Context) {
        let Some(live) = self.screen_rec.as_ref() else {
            return;
        };
        let done = match live.rx.try_recv() {
            Ok(done) => done,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(500));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => RecordDone {
                secs: live.started.elapsed().as_secs().min(sr::MAX_SECS),
                frames: Vec::new(),
                dir: PathBuf::new(),
                err: Some("the recorder stopped unexpectedly".into()),
            },
        };
        let Some(live) = self.screen_rec.take() else {
            return;
        };
        let key = if done.frames.is_empty() {
            String::new()
        } else {
            self.bearer()
        };
        self.finish_screen_recording(done, &live.note, &live.chat, key);
    }

    /// No key, or stills that can't be read back: the recording stays local
    /// and nothing is sent.
    pub(super) fn finish_screen_recording(
        &mut self,
        done: RecordDone,
        note: &str,
        chat: &str,
        key: String,
    ) {
        let title = sr::recording_title(done.secs, note);
        if done.frames.is_empty() {
            let why = done.err.unwrap_or_else(|| "no stills were taken".into());
            self.post_into_chat(
                chat,
                format!("{title}\n\nThe recording didn't start: {why}."),
            );
            self.status = format!("Screen recording failed: {why}");
            return;
        }
        // SD2: offer Save as skill once stills exist (vision needs a key on tap).
        // Do not clobber an in-flight Extracting/Draft — a second finish would drop
        // the vision rx and the first skill draft with no way back from chrome.
        let watch_busy = watch_with_mut(|slot| {
            matches!(
                slot.as_ref(),
                Some(WatchPhase::Extracting { .. } | WatchPhase::Draft { .. })
            )
        });
        if !watch_busy {
            watch_set(Some(WatchPhase::Offer(WatchOffer {
                dir: done.dir.clone(),
                note: note.to_string(),
                secs: done.secs,
                chat: chat.to_string(),
                title: title.clone(),
            })));
        }
        let picks = sr::sample_keyframes(done.frames.len(), sr::KEYFRAMES);
        let stills: Vec<String> = picks
            .iter()
            .filter_map(|&i| crate::desktop::load_image_data_url(&done.frames[i]).ok())
            .collect();
        if key.trim().is_empty() || stills.is_empty() {
            let why = if stills.is_empty() {
                "the stills could not be read back"
            } else {
                "Connect Grok in Settings"
            };
            self.post_screen_done(&title, &done.dir, chat, Err(why.to_string()));
            if watch_busy {
                self.status = "Save as skill still in progress — finish or cancel it before a new offer"
                    .into();
            }
            return;
        }
        let messages = sr::diagnosis_messages(note, done.secs, stills.len());
        let model = model_for_mode("fast").to_string();
        let (tx, rx) = mpsc::channel();
        self.screen_diag_rx = Some(rx);
        let (t, dir, c) = (title.clone(), done.dir.clone(), chat.to_string());
        std::thread::spawn(move || {
            let reply = crate::xai::grok_chat_images(&key, &model, &messages, &stills);
            let _ = tx.send((t, dir, c, reply));
        });
        self.status = if watch_busy {
            "Save as skill still in progress — finish or cancel it before a new offer".into()
        } else {
            format!("{title} · looking at {} stills…", picks.len())
        };
    }

    pub(super) fn poll_screen_diagnosis(&mut self) {
        if let Some(rx) = self.screen_diag_rx.take() {
            match rx.try_recv() {
                Ok((title, dir, chat, reply)) => self.post_screen_done(&title, &dir, &chat, reply),
                Err(mpsc::TryRecvError::Empty) => self.screen_diag_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        self.poll_watch_learn();
    }

    /// Advance the watch-and-learn vision thread, if one is running.
    pub(super) fn poll_watch_learn(&mut self) {
        let extracting =
            watch_with_mut(|slot| matches!(slot.as_ref(), Some(WatchPhase::Extracting { .. })));
        if !extracting {
            return;
        }
        let Some(WatchPhase::Extracting { offer, rx }) = watch_take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(raw)) => match draft_from_reply(&offer.note, &raw) {
                Some(draft) => {
                    let hard_lines = hard_step_lines(&draft);
                    self.status = format!("Skill draft ready: {}", draft.name);
                    watch_set(Some(WatchPhase::Draft {
                        offer,
                        draft,
                        hard_lines,
                    }));
                }
                None => {
                    self.status = "No clear steps found".into();
                    self.post_into_chat(
                        &offer.chat,
                        format!(
                            "{}\n\nNo clear steps found in the recording. Nothing was saved.",
                            offer.title
                        ),
                    );
                    watch_set(Some(WatchPhase::Empty(offer)));
                }
            },
            Ok(Err(why)) => {
                self.status = format!("Could not read steps: {why}");
                self.post_into_chat(
                    &offer.chat,
                    format!(
                        "{}\n\nCould not turn this recording into a skill: {why}",
                        offer.title
                    ),
                );
                watch_set(Some(WatchPhase::Offer(offer)));
            }
            Err(mpsc::TryRecvError::Empty) => {
                watch_set(Some(WatchPhase::Extracting { offer, rx }));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                watch_set(Some(WatchPhase::Offer(offer)));
            }
        }
    }

    /// Start the watch-and-learn vision call for the current offer.
    pub(super) fn start_watch_learn_extract(&mut self) {
        let Some(WatchPhase::Offer(offer)) = watch_take() else {
            self.status = "No recording ready to save as a skill.".into();
            return;
        };
        let key = self.bearer();
        if key.trim().is_empty() {
            self.status = "Connect Grok in Settings to save a skill from a recording.".into();
            watch_set(Some(WatchPhase::Offer(offer)));
            return;
        }
        let frames = list_frame_paths(&offer.dir);
        if frames.is_empty() {
            self.status = "No stills left in that recording folder.".into();
            watch_set(Some(WatchPhase::Offer(offer)));
            return;
        }
        let picks = sr::sample_keyframes(frames.len(), sr::KEYFRAMES);
        let stills: Vec<String> = picks
            .iter()
            .filter_map(|&i| crate::desktop::load_image_data_url(&frames[i]).ok())
            .collect();
        if stills.is_empty() {
            self.status = "The stills could not be read back.".into();
            watch_set(Some(WatchPhase::Offer(offer)));
            return;
        }
        let messages = watch_learn::steps_messages(&offer.note, offer.secs, stills.len());
        let model = model_for_mode("fast").to_string();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let reply = crate::xai::grok_chat_images(&key, &model, &messages, &stills);
            let _ = tx.send(reply);
        });
        self.status = format!(
            "{} · reading steps from {} stills…",
            offer.title,
            picks.len()
        );
        watch_set(Some(WatchPhase::Extracting { offer, rx }));
    }

    /// User confirmed Save on the draft.
    pub(super) fn confirm_watch_learn_save(&mut self) {
        let Some(WatchPhase::Draft {
            offer,
            draft,
            hard_lines: _,
        }) = watch_take()
        else {
            return;
        };
        let skills = skills::skills_dir();
        match super::skill_undo::save_watched_skill_for_undo(&config::config_dir(), &skills, &draft)
        {
            Ok(path) => {
                self.remember_skill(draft.clone());
                let hard_note = if draft.pitfalls.contains("asks for approval") {
                    " Hard steps will ask again on every replay."
                } else {
                    ""
                };
                let line = format!(
                    "Saved skill {} at {}. Undo from /skills changes removes it.{}",
                    draft.name,
                    path.display(),
                    hard_note
                );
                self.post_into_chat(&offer.chat, line.clone());
                self.status = format!("Saved skill {}", draft.name);
                watch_set(None);
            }
            Err(e) => {
                self.status = format!("Could not save skill: {e}");
                self.post_into_chat(&offer.chat, format!("Could not save skill: {e}"));
                let hard_lines = hard_step_lines(&draft);
                watch_set(Some(WatchPhase::Draft {
                    offer,
                    draft,
                    hard_lines,
                }));
            }
        }
    }

    pub(super) fn cancel_watch_learn(&mut self) {
        if let Some(phase) = watch_take() {
            let chat = match &phase {
                WatchPhase::Offer(o)
                | WatchPhase::Extracting { offer: o, .. }
                | WatchPhase::Draft { offer: o, .. }
                | WatchPhase::Empty(o) => o.chat.clone(),
            };
            if matches!(phase, WatchPhase::Draft { .. }) {
                self.post_into_chat(
                    &chat,
                    "Skill draft discarded. The recording is still local.".into(),
                );
            }
            self.status = "Skill draft cancelled.".into();
        }
        watch_set(None);
    }

    /// The report in its chat and one feed card naming the recording.
    pub(super) fn post_screen_done(
        &mut self,
        title: &str,
        dir: &Path,
        chat: &str,
        reply: Result<String, String>,
    ) {
        let (text, summary, stored) = match reply {
            Ok(raw) => {
                let report = sr::parse_report(&raw);
                (
                    sr::report_text(title, &report, dir),
                    report_summary(&report),
                    Some(super::fix_it::StoredDiagnosis::Screen(report)),
                )
            }
            Err(why) => (
                sr::local_only_text(title, dir, &why),
                format!("Saved on this computer, not diagnosed: {}", why.trim()),
                None,
            ),
        };
        self.post_into_chat(chat, text);
        let card = grokhub_core::screen_recording_card(
            &dir.display().to_string(),
            title,
            &summary,
            chat,
            now_ms(),
        );
        if let Some(stored) = stored {
            self.remember_diagnosis(card.id.clone(), stored);
        }
        self.post_feed_card(card);
        self.status = title.to_string();
    }

    /// Into the chat that asked, even after a switch.
    pub(super) fn post_into_chat(&mut self, chat: &str, body: String) {
        if chat.is_empty() || chat == self.visible_thread_id() {
            self.live_mut().push(("assistant".into(), body));
            self.stamp_current_access();
        } else if let Some(t) = self.threads.iter_mut().find(|t| t.id == chat) {
            Arc::make_mut(&mut t.messages).push(("assistant".into(), body));
        } else {
            return;
        }
        self.persist();
    }

    /// Delete recording on a feed card: a delete always asks first.
    pub(super) fn arm_delete_recording(&mut self, card_id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == card_id) else {
            return;
        };
        let Some(dir) = grokhub_core::screen_recording_dir(card) else {
            return;
        };
        self.confirm = Some(confirm::ConfirmKind::DeleteRecording {
            card_id: card.id.clone(),
            dir: dir.to_string(),
            title: card.title.clone(),
        });
    }

    /// After the confirm: only a folder directly under ~/GrokHub/recordings.
    pub(super) fn delete_recording(&mut self, card_id: &str, dir: &str, title: &str) {
        let dir = PathBuf::from(dir);
        let home = grokhub_core::paths::user_home().unwrap_or_default();
        if !sr::is_recording_dir(&home, &dir) {
            self.status = format!(
                "Not deleted: {} is not a GrokHub recording folder.",
                dir.display()
            );
            return;
        }
        // Drop Save-as-skill chrome for this folder before (or even if) remove fails.
        clear_watch_learn_for_dir(&dir);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {
                if grokhub_core::dismiss_update_at(&mut self.updates, card_id, now_ms()) {
                    self.persist_updates();
                }
                self.status = format!("Deleted {title}");
            }
            Err(e) => self.status = format!("Couldn't delete {title}: {e}"),
        }
    }

    /// Red indicator with the clock and Stop, above every page while recording,
    /// under the title bar so the window buttons stay reachable. When idle,
    /// paints the SD2 Save as skill / draft chrome for the latest recording.
    pub(super) fn paint_record_indicator(&mut self, ctx: &egui::Context) {
        if let Some(live) = self.screen_rec.as_ref() {
            let line = sr::indicator_line(live.started.elapsed().as_secs().min(sr::MAX_SECS));
            let mut stop = false;
            egui::Area::new(egui::Id::new("screen-record-indicator"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_TOP, [-16.0, 52.0])
                .show(ctx, |ui| {
                    egui::Frame::NONE
                        .fill(crate::theme::elevated())
                        .corner_radius(crate::theme::CHROME_RADIUS)
                        .stroke(egui::Stroke::new(2.0_f32, crate::theme::offline()))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&line).size(13.0).color(crate::theme::fg()));
                                stop = crate::cards::danger_pill(ui, "Stop");
                            });
                        });
                });
            if stop {
                self.stop_screen_recording();
            }
            return;
        }
        self.paint_watch_learn_chrome(ctx);
    }

    fn paint_watch_learn_chrome(&mut self, ctx: &egui::Context) {
        let kind = watch_with_mut(|slot| {
            slot.as_ref().map(|p| match p {
                WatchPhase::Offer(_) => 0u8,
                WatchPhase::Extracting { .. } => 1,
                WatchPhase::Draft { .. } => 2,
                WatchPhase::Empty(_) => 3,
            })
        });
        let Some(kind) = kind else {
            return;
        };
        let mut save_as = false;
        let mut save_draft = false;
        let mut cancel = false;
        let mut dismiss = false;
        let (title, body, hard): (String, String, Vec<String>) =
            watch_with_mut(|slot| match slot.as_ref() {
                Some(WatchPhase::Offer(o)) => (
                    o.title.clone(),
                    "Save the steps you showed as a skill?".into(),
                    Vec::new(),
                ),
                Some(WatchPhase::Extracting { offer, .. }) => (
                    offer.title.clone(),
                    "Reading steps from the stills…".into(),
                    Vec::new(),
                ),
                Some(WatchPhase::Draft {
                    offer,
                    draft,
                    hard_lines,
                }) => (
                    format!("Skill draft: {}", draft.name),
                    {
                        let mut lines = vec![offer.title.clone(), String::new()];
                        for (i, line) in draft.instructions.lines().enumerate() {
                            let bare = line.trim();
                            if bare.is_empty() {
                                continue;
                            }
                            let flagged = hard_lines.iter().any(|h| bare.contains(h.as_str()));
                            if flagged {
                                lines.push(format!("{bare}  (asks for approval)"));
                            } else {
                                lines.push(bare.to_string());
                            }
                            if i > 14 {
                                lines.push("…".into());
                                break;
                            }
                        }
                        lines.join("\n")
                    },
                    hard_lines.clone(),
                ),
                Some(WatchPhase::Empty(o)) => {
                    (o.title.clone(), "No clear steps found".into(), Vec::new())
                }
                None => (String::new(), String::new(), Vec::new()),
            });
        let _ = hard;
        egui::Area::new(egui::Id::new("watch-learn-chrome"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, [-16.0, 52.0])
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .corner_radius(crate::theme::CHROME_RADIUS)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.label(RichText::new(&title).size(13.0).color(crate::theme::fg()));
                        ui.add_space(4.0);
                        ui.label(RichText::new(&body).size(12.0).color(crate::theme::muted()));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            match kind {
                                0 => {
                                    if super::change_undo::click_pill(ui, "Save as skill") {
                                        save_as = true;
                                    }
                                    if super::change_undo::click_pill(ui, "Dismiss") {
                                        dismiss = true;
                                    }
                                }
                                1 => {
                                    ui.label(
                                        RichText::new("Working…")
                                            .size(12.0)
                                            .color(crate::theme::muted()),
                                    );
                                    if super::change_undo::click_pill(ui, "Cancel") {
                                        cancel = true;
                                    }
                                }
                                2 => {
                                    if super::change_undo::click_pill(ui, "Save") {
                                        save_draft = true;
                                    }
                                    if super::change_undo::click_pill(ui, "Cancel") {
                                        cancel = true;
                                    }
                                }
                                _ => {
                                    if super::change_undo::click_pill(ui, "Dismiss") {
                                        dismiss = true;
                                    }
                                }
                            }
                        });
                    });
            });
        if save_as {
            self.start_watch_learn_extract();
        } else if save_draft {
            self.confirm_watch_learn_save();
        } else if cancel || dismiss {
            self.cancel_watch_learn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-screenrec-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_capture_loop_saves_a_still_every_two_seconds_until_stop() {
        let dir = scratch("stop");
        let stop = AtomicBool::new(false);
        let clock = Cell::new(0u64);
        let grabs = Cell::new(0usize);
        let done = record_loop(
            &dir,
            &stop,
            || {
                grabs.set(grabs.get() + 1);
                Ok((vec![grabs.get() as u8; 4], "image/png".into()))
            },
            || clock.get(),
            || {
                clock.set(clock.get() + 500);
                if clock.get() >= 9_000 {
                    stop.store(true, Ordering::Relaxed);
                }
            },
        );
        assert_eq!(done.secs, 9);
        assert_eq!(done.err, None);
        assert_eq!(
            done.frames,
            (1..=5)
                .map(|i| dir.join(format!("frame-00{i}.png")))
                .collect::<Vec<_>>(),
            "stills at 0, 2, 4, 6 and 8 seconds"
        );
        assert_eq!(
            std::fs::read(dir.join("frame-003.png")).unwrap(),
            vec![3u8; 4]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_capture_loop_stops_itself_at_two_minutes_and_on_a_failed_still() {
        let dir = scratch("cap");
        let stop = AtomicBool::new(false);
        let clock = Cell::new(0u64);
        let done = record_loop(
            &dir,
            &stop,
            || Ok((vec![1], "image/jpeg".into())),
            || clock.get(),
            || clock.set(clock.get() + 1_000),
        );
        assert_eq!(done.secs, sr::MAX_SECS);
        assert_eq!(done.frames.len(), 60);
        assert_eq!(done.frames[0], dir.join("frame-001.jpg"));
        let _ = std::fs::remove_dir_all(&dir);

        let dir = scratch("fail");
        let clock = Cell::new(0u64);
        let calls = Cell::new(0usize);
        let done = record_loop(
            &dir,
            &AtomicBool::new(false),
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 3 {
                    Ok((vec![1], "image/png".into()))
                } else {
                    Err("portal refused".into())
                }
            },
            || clock.get(),
            || clock.set(clock.get() + 1_000),
        );
        assert_eq!(done.frames.len(), 2);
        assert_eq!(done.err.as_deref(), Some("portal refused"));
        assert_eq!(done.secs, 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn png(path: &Path) {
        image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 30, 30, 255]))
            .save(path)
            .unwrap();
    }

    fn pinned(
        label: &str,
    ) -> (
        std::sync::MutexGuard<'static, ()>,
        PathBuf,
        Option<std::ffi::OsString>,
    ) {
        let g = crate::config::hold_test_config();
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var("GROKHUB_CONFIG", &root);
        let home = std::env::var_os("HOME");
        std::env::set_var("HOME", &root);
        (g, root, home)
    }

    fn unpin(home: Option<std::ffi::OsString>) {
        std::env::remove_var("GROKHUB_CONFIG");
        match home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn record_without_consent_says_where_to_turn_it_on_and_records_nothing() {
        let (_g, root, home) = pinned("screenrec-consent");
        let mut cabin = Cabin::quiet_for_test();
        assert!(
            !cabin.cfg.screen_record,
            "screen recording is off until Settings turns it on"
        );
        cabin.run_slash_line("/record video stutters");
        assert!(cabin.screen_rec.is_none());
        assert_eq!(cabin.status, sr::CONSENT_OFF);
        assert!(cabin
            .messages
            .last()
            .is_some_and(|m| m.1.ends_with(sr::CONSENT_OFF)));
        assert!(
            !root.join("GrokHub").join("recordings").exists(),
            "no folder is made without consent"
        );
        cabin.run_slash_line("/record stop");
        assert_eq!(cabin.status, "Not recording.");
        unpin(home);
    }

    #[test]
    fn with_no_key_the_recording_stays_local_and_gets_a_named_card_with_delete() {
        let (_g, root, home) = pinned("screenrec-local");
        let mut cabin = Cabin::quiet_for_test();
        let dir = sr::recording_dir(&root, "2026-10-09-1");
        std::fs::create_dir_all(&dir).unwrap();
        let frames: Vec<PathBuf> = (0..3).map(|i| dir.join(sr::frame_name(i, "png"))).collect();
        frames.iter().for_each(|f| png(f));
        let done = RecordDone {
            secs: 42,
            frames,
            dir: dir.clone(),
            err: None,
        };
        cabin.finish_screen_recording(done, "video stutters on 4K YouTube", "", String::new());
        assert!(
            cabin.screen_diag_rx.is_none(),
            "nothing is sent without a key"
        );
        let title = "Screen recording 0:42: video stutters on 4K YouTube";
        assert_eq!(
            cabin.messages.last().map(|m| m.1.clone()),
            Some(format!(
                "{title}\n\nSaved on this computer only, not diagnosed: Connect Grok in Settings. The recording is in {}.",
                dir.display()
            ))
        );
        let card = cabin
            .updates
            .iter()
            .find(|c| grokhub_core::screen_recording_dir(c).is_some())
            .cloned()
            .expect("one screen recording card");
        assert_eq!(card.title, title);
        assert_eq!(
            card.body.as_deref(),
            Some("Saved on this computer, not diagnosed: Connect Grok in Settings")
        );

        cabin.arm_delete_recording(&card.id);
        assert_eq!(
            cabin.confirm,
            Some(confirm::ConfirmKind::DeleteRecording {
                card_id: card.id.clone(),
                dir: dir.display().to_string(),
                title: title.into(),
            })
        );
        assert!(dir.exists(), "arming the confirm deletes nothing");
        cabin.confirm = None;
        cabin.delete_recording(&card.id, &dir.display().to_string(), title);
        assert!(!dir.exists());
        assert_eq!(cabin.status, format!("Deleted {title}"));
        assert!(cabin
            .updates
            .iter()
            .find(|c| c.id == card.id)
            .is_none_or(|c| c.status == grokhub_core::UpdateStatus::Dismissed));

        let outside = root.join("Documents");
        std::fs::create_dir_all(&outside).unwrap();
        cabin.delete_recording("x", &outside.display().to_string(), "Docs");
        assert!(
            outside.exists(),
            "only a GrokHub recording folder can be deleted"
        );
        assert!(
            cabin.status.starts_with("Not deleted: "),
            "{}",
            cabin.status
        );
        unpin(home);
    }

    #[test]
    fn a_diagnosis_lands_in_the_chat_and_on_the_card() {
        let (_g, root, home) = pinned("screenrec-report");
        let mut cabin = Cabin::quiet_for_test();
        let dir = sr::recording_dir(&root, "r2");
        let raw = "Seen: Firefox plays a 4K video that freezes.\nLikely cause: Hardware decoding is off.\nFix steps:\n1. Turn on hardware decoding.";
        cabin.post_screen_done("Screen recording 0:12: freezes", &dir, "", Ok(raw.into()));
        let text = cabin
            .messages
            .last()
            .map(|m| m.1.clone())
            .unwrap_or_default();
        assert!(text.starts_with("Screen recording 0:12: freezes\n\nWhat I saw: Firefox plays a 4K video that freezes.\nLikely cause: Hardware decoding is off.\nSuggested fix steps:\n1. Turn on hardware decoding.\n"), "{text}");
        assert!(
            text.contains("Nothing was changed on this computer."),
            "{text}"
        );
        let card = cabin
            .updates
            .iter()
            .find(|c| grokhub_core::screen_recording_dir(c).is_some())
            .unwrap();
        assert_eq!(
            card.body.as_deref(),
            Some("Likely cause: Hardware decoding is off.")
        );
        unpin(home);
    }

    #[test]
    fn a_summary_prefers_the_cause_then_what_was_seen() {
        let mut r = sr::ScreenReport {
            seen: "Firefox freezes.".into(),
            cause: "Decoding is off.".into(),
            steps: vec![],
        };
        assert_eq!(report_summary(&r), "Likely cause: Decoding is off.");
        r.cause.clear();
        assert_eq!(report_summary(&r), "Firefox freezes.");
        r.seen.clear();
        assert_eq!(report_summary(&r), "Nothing stood out in the stills.");
    }

    const OPEN_SETTINGS_REPLY: &str = r#"
**Steps:**

1. Open Settings
2) Click Display in the sidebar
- Choose the built-in display
3. Confirm the resolution looks right
"#;

    #[test]
    fn draft_from_reply_builds_expected_draft_and_garbage_is_none() {
        let _wl = watch_learn_test_guard();
        let draft = draft_from_reply("open Settings → Display", OPEN_SETTINGS_REPLY)
            .expect("fixture reply");
        assert_eq!(draft.name, "open-settings-display");
        assert_eq!(draft.runs, 0);
        assert!(draft.instructions.contains("1. Open Settings"));
        assert!(draft
            .instructions
            .contains("2. Click Display in the sidebar"));
        assert!(draft
            .instructions
            .contains("3. Choose the built-in display"));
        assert!(draft
            .instructions
            .contains("4. Confirm the resolution looks right"));
        assert!(draft_from_reply("open Settings → Display", "asdf no list markers").is_none());
        assert!(draft_from_reply("open Settings → Display", "").is_none());
        assert!(draft_from_reply("open Settings → Display", "**Summary:**\nNope.\n").is_none());
    }

    #[test]
    fn finish_recording_arms_save_as_skill_offer_without_calling_vision() {
        let _wl = watch_learn_test_guard();
        watch_set(None);
        let (_g, root, home) = pinned("screenrec-watch-offer");
        let mut cabin = Cabin::quiet_for_test();
        let dir = sr::recording_dir(&root, "2026-10-14-learn");
        std::fs::create_dir_all(&dir).unwrap();
        let frames: Vec<PathBuf> = (0..3).map(|i| dir.join(sr::frame_name(i, "png"))).collect();
        frames.iter().for_each(|f| png(f));
        let done = RecordDone {
            secs: 18,
            frames,
            dir: dir.clone(),
            err: None,
        };
        cabin.finish_screen_recording(done, "open Settings → Display", "", String::new());
        assert!(
            cabin.screen_diag_rx.is_none(),
            "no diagnosis call without a key"
        );
        let armed = watch_with_mut(|s| {
            matches!(
                s.as_ref(),
                Some(WatchPhase::Offer(o)) if o.dir == dir && o.note == "open Settings → Display"
            )
        });
        assert!(armed, "Save as skill offer armed after recording");
        // Confirming a fixture reply (no network) saves via the undo path.
        let draft = draft_from_reply("open Settings → Display", OPEN_SETTINGS_REPLY).unwrap();
        watch_set(Some(WatchPhase::Draft {
            offer: WatchOffer {
                dir: dir.clone(),
                note: "open Settings → Display".into(),
                secs: 18,
                chat: String::new(),
                title: sr::recording_title(18, "open Settings → Display"),
            },
            hard_lines: hard_step_lines(&draft),
            draft,
        }));
        cabin.confirm_watch_learn_save();
        assert!(skills::skills_dir()
            .join("open-settings-display")
            .join("SKILL.md")
            .is_file());
        assert!(
            cabin
                .skill_list
                .iter()
                .any(|s| s.name == "open-settings-display"),
            "saved skill is remembered in the cabin list"
        );
        watch_set(None);
        unpin(home);
    }

    #[test]
    fn finish_recording_does_not_clobber_in_flight_save_as_skill_draft() {
        let _wl = watch_learn_test_guard();
        watch_set(None);
        let (_g, root, home) = pinned("screenrec-watch-noclobber");
        let mut cabin = Cabin::quiet_for_test();
        let dir_a = sr::recording_dir(&root, "2026-10-14-a");
        let dir_b = sr::recording_dir(&root, "2026-10-14-b");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        png(&dir_a.join(sr::frame_name(0, "png")));
        png(&dir_b.join(sr::frame_name(0, "png")));
        let draft = draft_from_reply("open Settings → Display", OPEN_SETTINGS_REPLY).unwrap();
        watch_set(Some(WatchPhase::Draft {
            offer: WatchOffer {
                dir: dir_a.clone(),
                note: "open Settings → Display".into(),
                secs: 12,
                chat: String::new(),
                title: sr::recording_title(12, "open Settings → Display"),
            },
            hard_lines: hard_step_lines(&draft),
            draft,
        }));
        let done_b = RecordDone {
            secs: 9,
            frames: vec![dir_b.join(sr::frame_name(0, "png"))],
            dir: dir_b,
            err: None,
        };
        cabin.finish_screen_recording(done_b, "other capture", "", String::new());
        let kept = watch_with_mut(|s| {
            matches!(
                s.as_ref(),
                Some(WatchPhase::Draft { offer, .. }) if offer.dir == dir_a
            )
        });
        assert!(kept, "in-flight Draft must survive a second recording finish");
        assert!(
            cabin.status.contains("Save as skill still in progress"),
            "status explains why the new offer was refused: {}",
            cabin.status
        );
        watch_set(None);
        unpin(home);
    }

    #[test]
    fn delete_recording_clears_save_as_skill_offer() {
        let _wl = watch_learn_test_guard();
        watch_set(None);
        let (_g, root, home) = pinned("screenrec-watch-delete");
        let mut cabin = Cabin::quiet_for_test();
        let dir = sr::recording_dir(&root, "2026-10-14-delete");
        std::fs::create_dir_all(&dir).unwrap();
        png(&dir.join(sr::frame_name(0, "png")));
        watch_set(Some(WatchPhase::Offer(WatchOffer {
            dir: dir.clone(),
            note: "open Settings".into(),
            secs: 6,
            chat: String::new(),
            title: sr::recording_title(6, "open Settings"),
        })));
        cabin.delete_recording("card-x", &dir.display().to_string(), "Screen recording");
        assert!(!dir.exists());
        assert!(
            watch_with_mut(|s| s.is_none()),
            "deleting the recording must clear the Save as skill offer"
        );
        unpin(home);
    }
}
