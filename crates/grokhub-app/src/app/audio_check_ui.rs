//! "Check my audio" (card 11, Phase B). `/audiocheck [input]` or the palette
//! lists the outputs and inputs, listens to the chosen input for a few
//! seconds and reports what's wrong in the chat that asked and on a feed card
//! naming the device. The clip stays in memory; nothing is changed or sent.

use super::*;
use grokhub_core::audio_check as ac;

/// `(input name, report)` from a finished check.
pub(super) type CheckDone = Result<(String, ac::AudioReport), String>;

pub(super) struct LiveAudioCheck {
    started: Instant,
    chat: String,
    rx: mpsc::Receiver<CheckDone>,
}

/// Outputs and inputs from `pactl`, empty where it isn't available.
fn list_devices() -> (Vec<ac::AudioDevice>, Vec<ac::AudioDevice>) {
    let Some(info) = crate::desktop::pactl_text(&["info"]) else {
        return (Vec::new(), Vec::new());
    };
    let (sink, source) = ac::parse_pactl_defaults(&info);
    let list = |what: &str, default: Option<&str>| {
        crate::desktop::pactl_text(&["list", what])
            .map(|t| ac::parse_pactl_list(&t, default))
            .unwrap_or_default()
    };
    (list("sinks", sink.as_deref()), list("sources", source.as_deref()))
}

/// The check, off the UI thread: pick the input, record it, measure it.
/// The default input is recorded as "default" so any recorder can take it.
pub(super) fn run_check(
    want: &str,
    list: impl FnOnce() -> (Vec<ac::AudioDevice>, Vec<ac::AudioDevice>),
    record: impl FnOnce(Option<&str>) -> Result<(u32, Vec<u8>), String>,
) -> CheckDone {
    let (outputs, inputs) = list();
    let input = ac::pick_input(&inputs, want)?;
    let target = input.filter(|d| !d.is_default).map(|d| d.name.as_str());
    let label = input.map(ac::AudioDevice::label).unwrap_or("the default input");
    let (rate, pcm) = record(target).map_err(|e| format!("Couldn't record from {label}: {e}"))?;
    let levels = ac::measure(&ac::pcm_samples(&pcm), rate);
    let name = input.map(|d| d.name.clone()).unwrap_or_default();
    Ok((name, ac::audio_report(&outputs, &inputs, input, &levels)))
}

impl Cabin {
    /// `/audiocheck [input]`: asking for it is the consent; the clip is never kept.
    pub(super) fn start_audio_check(&mut self, want: String) {
        if self.audio_check.is_some() {
            self.status = ac::ALREADY_CHECKING.into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_check(&want, list_devices, crate::desktop::record_clip));
        });
        self.audio_check = Some(LiveAudioCheck {
            started: Instant::now(),
            chat: self.visible_thread_id(),
            rx,
        });
        let line = format!(
            "Checking your audio: finding the input, then listening for {} seconds. Talk or play something at your usual loudness. The clip stays in memory and isn't saved or sent.",
            ac::CLIP_SECS
        );
        self.live_mut().push(("assistant".into(), mark_slash_result(&line)));
        self.persist();
        self.status = "Checking your audio…".into();
    }

    pub(super) fn poll_audio_check(&mut self, ctx: &egui::Context) {
        let Some(live) = self.audio_check.as_ref() else {
            return;
        };
        let done = match live.rx.try_recv() {
            Ok(done) => done,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(500));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => Err("the audio check stopped unexpectedly".into()),
        };
        let Some(live) = self.audio_check.take() else {
            return;
        };
        self.finish_audio_check(&live.chat, done);
    }

    /// The report in the chat that asked, and one card naming the device.
    pub(super) fn finish_audio_check(&mut self, chat: &str, done: CheckDone) {
        match done {
            Ok((input, report)) => {
                self.post_into_chat(chat, report.text.clone());
                let card = grokhub_core::audio_check_card(&input, &report.card_title, &report.summary, chat, now_ms());
                self.post_feed_card(card);
                self.status = report.title;
            }
            Err(why) => {
                self.post_into_chat(chat, mark_slash_result(&why));
                self.status = why;
            }
        }
    }

    /// While the microphone is open, say so above every page.
    pub(super) fn paint_listen_indicator(&mut self, ctx: &egui::Context) {
        let Some(live) = self.audio_check.as_ref() else {
            return;
        };
        let secs = live.started.elapsed().as_secs().min(u64::from(ac::CLIP_SECS));
        let line = format!("● Audio check: listening 0:{secs:02} of 0:{:02}", ac::CLIP_SECS);
        egui::Area::new(egui::Id::new("audio-check-indicator"))
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
                        });
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, desc: &str, default: bool) -> ac::AudioDevice {
        ac::AudioDevice {
            name: name.into(),
            description: desc.into(),
            muted: false,
            volume_pct: Some(100),
            is_default: default,
        }
    }

    fn devices() -> (Vec<ac::AudioDevice>, Vec<ac::AudioDevice>) {
        (
            vec![dev("hdmi", "Navi 21 HDMI / DisplayPort", true)],
            vec![dev("yeti", "Yeti Stereo Microphone", true), dev("builtin", "Built-in Microphone", false)],
        )
    }

    /// One second of full-scale square wave: clipping all the way.
    fn clipped_pcm() -> Vec<u8> {
        (0..48_000)
            .flat_map(|i| if (i / 50) % 2 == 0 { 32_767i16 } else { -32_768 }.to_le_bytes())
            .collect()
    }

    #[test]
    fn the_check_records_the_default_input_as_default_and_a_named_one_by_name() {
        let mut asked = Vec::new();
        let (name, report) = run_check("", devices, |d| {
            asked.push(d.map(str::to_string));
            Ok((48_000, clipped_pcm()))
        })
        .unwrap();
        assert_eq!(asked, vec![None], "the default input is recorded as the default");
        assert_eq!(name, "yeti");
        assert_eq!(
            report.title,
            "Audio check: Yeti Stereo Microphone input clipping at 0.0 dBFS; default output is Navi 21 HDMI / DisplayPort"
        );

        let mut asked = Vec::new();
        let (name, _) = run_check("built-in", devices, |d| {
            asked.push(d.map(str::to_string));
            Ok((48_000, clipped_pcm()))
        })
        .unwrap();
        assert_eq!(asked, vec![Some("builtin".to_string())]);
        assert_eq!(name, "builtin");
    }

    #[test]
    fn a_wrong_name_or_a_missing_recorder_says_so_and_records_nothing() {
        let err = run_check("rode", devices, |_| panic!("nothing is recorded for an unknown input"));
        assert_eq!(err, Err("No input matches \"rode\". Inputs: Yeti Stereo Microphone, Built-in Microphone.".into()));
        let err = run_check("", devices, |_| Err("No recorder found".into()));
        assert_eq!(err, Err("Couldn't record from Yeti Stereo Microphone: No recorder found".into()));
        let err = run_check("", || (Vec::new(), Vec::new()), |_| Err("parecord recorded no sound data".into()));
        assert_eq!(err, Err("Couldn't record from the default input: parecord recorded no sound data".into()));
    }

    #[test]
    fn a_finished_check_lands_in_the_chat_and_on_a_card_naming_the_device() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("audio-check-report");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut cabin = Cabin::quiet_for_test();
        let done = run_check("", devices, |_| Ok((48_000, clipped_pcm())));
        let report = done.clone().unwrap().1;
        cabin.finish_audio_check("", done);
        assert_eq!(cabin.messages.last().map(|m| m.1.clone()), Some(report.text.clone()));
        assert!(report.text.contains("\nSuggested fixes:\n1. Lower Yeti Stereo Microphone's input volume or gain knob"));
        let card = cabin
            .updates
            .iter()
            .find(|c| c.source_id == "audiocheck:yeti")
            .expect("one audio check card");
        assert_eq!(card.title, "Audio check: Yeti Stereo Microphone input clipping at 0.0 dBFS");
        assert_eq!(
            card.body.as_deref(),
            Some("Lower Yeti Stereo Microphone's input volume or gain knob until loud speech peaks around -6 dBFS. Default output: Navi 21 HDMI / DisplayPort.")
        );
        assert_eq!(cabin.status, report.title);

        cabin.finish_audio_check("", Err("No input matches \"rode\".".into()));
        assert!(cabin.messages.last().is_some_and(|m| m.1.ends_with("No input matches \"rode\".")));
        assert_eq!(cabin.updates.iter().filter(|c| c.source_id.starts_with("audiocheck:")).count(), 1);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn audiocheck_starts_one_check_at_a_time() {
        let _g = crate::config::hold_test_config();
        let mut cabin = Cabin::quiet_for_test();
        let (_tx, rx) = mpsc::channel();
        cabin.audio_check = Some(LiveAudioCheck { started: Instant::now(), chat: String::new(), rx });
        let before = cabin.messages.len();
        cabin.run_slash_line("/audiocheck");
        assert_eq!(cabin.status, ac::ALREADY_CHECKING);
        assert_eq!(cabin.messages.len(), before, "a second check posts nothing");
    }
}
