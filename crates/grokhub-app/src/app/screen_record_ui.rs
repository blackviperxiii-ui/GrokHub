//! "Record my screen" (card 11, Phase A). `/record [what's wrong]` or the
//! palette starts a consented recording: a still every 2 seconds into
//! `~/GrokHub/recordings/<stamp>`, a red indicator with Stop on top of every
//! page, and a 2-minute cap. On stop a few keyframes and the note go to Grok
//! once; the report lands in the chat that asked and on a feed card. Nothing
//! on the computer is changed. Delete recording asks first.

use super::*;
use grokhub_core::screen_record as sr;

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
            self.live_mut().push(("assistant".into(), mark_slash_result(why)));
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
        self.live_mut().push(("assistant".into(), mark_slash_result(&line)));
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
        let key = if done.frames.is_empty() { String::new() } else { self.bearer() };
        self.finish_screen_recording(done, &live.note, &live.chat, key);
    }

    /// No key, or stills that can't be read back: the recording stays local
    /// and nothing is sent.
    pub(super) fn finish_screen_recording(&mut self, done: RecordDone, note: &str, chat: &str, key: String) {
        let title = sr::recording_title(done.secs, note);
        if done.frames.is_empty() {
            let why = done.err.unwrap_or_else(|| "no stills were taken".into());
            self.post_screen_report(chat, format!("{title}\n\nThe recording didn't start: {why}."));
            self.status = format!("Screen recording failed: {why}");
            return;
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
        self.status = format!("{title} · looking at {} stills…", picks.len());
    }

    pub(super) fn poll_screen_diagnosis(&mut self) {
        let Some(rx) = self.screen_diag_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((title, dir, chat, reply)) => self.post_screen_done(&title, &dir, &chat, reply),
            Err(mpsc::TryRecvError::Empty) => self.screen_diag_rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    /// The report in its chat and one feed card naming the recording.
    pub(super) fn post_screen_done(&mut self, title: &str, dir: &Path, chat: &str, reply: Result<String, String>) {
        let (text, summary) = match reply {
            Ok(raw) => {
                let report = sr::parse_report(&raw);
                (sr::report_text(title, &report, dir), report_summary(&report))
            }
            Err(why) => (
                sr::local_only_text(title, dir, &why),
                format!("Saved on this computer, not diagnosed: {}", why.trim()),
            ),
        };
        self.post_screen_report(chat, text);
        let card = grokhub_core::screen_recording_card(&dir.display().to_string(), title, &summary, chat, now_ms());
        self.post_feed_card(card);
        self.status = title.to_string();
    }

    /// Into the chat that started the recording, even after a switch.
    fn post_screen_report(&mut self, chat: &str, body: String) {
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
            self.status = format!("Not deleted: {} is not a GrokHub recording folder.", dir.display());
            return;
        }
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

    /// Red indicator with the clock and Stop, above every page while recording.
    pub(super) fn paint_record_indicator(&mut self, ctx: &egui::Context) {
        let Some(live) = self.screen_rec.as_ref() else {
            return;
        };
        let line = sr::indicator_line(live.started.elapsed().as_secs().min(sr::MAX_SECS));
        let mut stop = false;
        egui::Area::new(egui::Id::new("screen-record-indicator"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, [-16.0, 12.0])
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
            (1..=5).map(|i| dir.join(format!("frame-00{i}.png"))).collect::<Vec<_>>(),
            "stills at 0, 2, 4, 6 and 8 seconds"
        );
        assert_eq!(std::fs::read(dir.join("frame-003.png")).unwrap(), vec![3u8; 4]);
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

    fn pinned(label: &str) -> (std::sync::MutexGuard<'static, ()>, PathBuf, Option<std::ffi::OsString>) {
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
        assert!(!cabin.cfg.screen_record, "screen recording is off until Settings turns it on");
        cabin.run_slash_line("/record video stutters");
        assert!(cabin.screen_rec.is_none());
        assert_eq!(cabin.status, sr::CONSENT_OFF);
        assert!(cabin.messages.last().is_some_and(|m| m.1.ends_with(sr::CONSENT_OFF)));
        assert!(!root.join("GrokHub").join("recordings").exists(), "no folder is made without consent");
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
        let done = RecordDone { secs: 42, frames, dir: dir.clone(), err: None };
        cabin.finish_screen_recording(done, "video stutters on 4K YouTube", "", String::new());
        assert!(cabin.screen_diag_rx.is_none(), "nothing is sent without a key");
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
        assert_eq!(card.body.as_deref(), Some("Saved on this computer, not diagnosed: Connect Grok in Settings"));

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
        assert!(outside.exists(), "only a GrokHub recording folder can be deleted");
        assert!(cabin.status.starts_with("Not deleted: "), "{}", cabin.status);
        unpin(home);
    }

    #[test]
    fn a_diagnosis_lands_in_the_chat_and_on_the_card() {
        let (_g, root, home) = pinned("screenrec-report");
        let mut cabin = Cabin::quiet_for_test();
        let dir = sr::recording_dir(&root, "r2");
        let raw = "Seen: Firefox plays a 4K video that freezes.\nLikely cause: Hardware decoding is off.\nFix steps:\n1. Turn on hardware decoding.";
        cabin.post_screen_done("Screen recording 0:12: freezes", &dir, "", Ok(raw.into()));
        let text = cabin.messages.last().map(|m| m.1.clone()).unwrap_or_default();
        assert!(text.starts_with("Screen recording 0:12: freezes\n\nWhat I saw: Firefox plays a 4K video that freezes.\nLikely cause: Hardware decoding is off.\nSuggested fix steps:\n1. Turn on hardware decoding.\n"), "{text}");
        assert!(text.contains("Nothing was changed on this computer."), "{text}");
        let card = cabin.updates.iter().find(|c| grokhub_core::screen_recording_dir(c).is_some()).unwrap();
        assert_eq!(card.body.as_deref(), Some("Likely cause: Hardware decoding is off."));
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
}
