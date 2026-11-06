//! A spoken reply, piece by piece.
//!
//! The reply's text is cut into speakable pieces as it streams in. One thread
//! synthesizes each piece on one warm connection while a second plays the
//! piece before it, so the first sentence is heard while the rest is still
//! being written. Dropping the reply stops it mid-sentence.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use grokhub_core::voice_turn::{timing_percentiles, SpeechChunker, VoiceTimings};
use grokhub_core::TEXT_FILE_CAP;

/// Turns kept in the timings file.
const TIMINGS_KEPT: usize = 100;
/// A heard turn older than this is not the one being answered.
const TURN_CLOCK_MAX_AGE: Duration = Duration::from_secs(120);

/// When the person's last voice turn ended, and how long its words took.
#[derive(Debug, Clone, Copy)]
pub struct TurnClock {
    pub ended: Instant,
    pub words_ms: u64,
}

static LAST_TURN: Mutex<Option<TurnClock>> = Mutex::new(None);

/// Remember the voice turn whose reply is coming.
pub fn note_turn(clock: TurnClock) {
    *LAST_TURN.lock().unwrap_or_else(|e| e.into_inner()) = Some(clock);
}

/// The voice turn being answered, once, if it is recent.
pub fn take_turn() -> Option<TurnClock> {
    LAST_TURN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .filter(|c| c.ended.elapsed() < TURN_CLOCK_MAX_AGE)
}

/// Synthesizes one piece of speech (mp3 bytes). Swapped out in tests.
pub type Synth = Box<dyn FnMut(&str) -> Result<Vec<u8>, String> + Send>;
/// Plays one mp3 piece, returning early when the flag is set.
pub type Play = Box<dyn FnMut(Vec<u8>, &AtomicBool) + Send>;

pub struct SpokenReply {
    chunker: SpeechChunker,
    text_tx: Option<Sender<String>>,
    stop: Arc<AtomicBool>,
    errors: Receiver<String>,
    done: Option<Receiver<()>>,
    /// Set once the last piece has played or the reply stopped.
    over: Arc<AtomicBool>,
    turn: Option<TurnClock>,
    sent: Instant,
    /// Bytes of reply text still allowed through.
    left: usize,
    first_token_ms: Option<u64>,
    /// Milliseconds from `sent` to the first piece playing, plus one (0 = not yet).
    first_audio: Arc<AtomicU64>,
    pieces: usize,
}

impl SpokenReply {
    /// A reply spoken with xAI speech in `voice`, mouth moving in `lips`.
    pub fn xai(
        key: String,
        voice: &'static str,
        lips: crate::lipsync::LipSlot,
        turn: Option<TurnClock>,
    ) -> Self {
        let agent = crate::xai::speech_agent();
        let synth: Synth = Box::new(move |text| crate::xai::grok_tts_on(&agent, &key, text, voice));
        let play: Play = Box::new(move |mp3, stop| {
            let _ = crate::lipsync::speak_mp3(mp3, &lips, stop);
        });
        Self::start(synth, play, turn)
    }

    /// Times count from the end of `turn` when there is one.
    pub fn start(mut synth: Synth, mut play: Play, turn: Option<TurnClock>) -> Self {
        let (text_tx, text_rx) = mpsc::channel::<String>();
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<u8>>();
        let (err_tx, errors) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let first_audio = Arc::new(AtomicU64::new(0));
        let over = Arc::new(AtomicBool::new(false));
        let sent = turn.map_or_else(Instant::now, |t| t.ended);
        let stop_synth = stop.clone();
        std::thread::spawn(move || {
            for text in text_rx {
                if stop_synth.load(Ordering::SeqCst) {
                    break;
                }
                match synth(&text) {
                    Ok(mp3) => {
                        if audio_tx.send(mp3).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = err_tx.send(e);
                    }
                }
            }
        });
        let stop_play = stop.clone();
        let first = first_audio.clone();
        let over_play = over.clone();
        std::thread::spawn(move || {
            for mp3 in audio_rx {
                if stop_play.load(Ordering::SeqCst) {
                    break;
                }
                let ms = sent.elapsed().as_millis() as u64 + 1;
                let _ = first.compare_exchange(0, ms, Ordering::SeqCst, Ordering::SeqCst);
                play(mp3, &stop_play);
            }
            over_play.store(true, Ordering::SeqCst);
            let _ = done_tx.send(());
        });
        Self {
            chunker: SpeechChunker::new(),
            text_tx: Some(text_tx),
            stop,
            errors,
            done: Some(done),
            over,
            turn,
            sent,
            left: TEXT_FILE_CAP,
            first_token_ms: None,
            first_audio,
            pieces: 0,
        }
    }

    /// More of the reply as it streams. True when this queued the first piece.
    pub fn feed(&mut self, delta: &str) -> bool {
        let delta = self.within_cap(delta);
        if self.first_token_ms.is_none() && !delta.trim().is_empty() {
            self.first_token_ms = Some(self.sent.elapsed().as_millis() as u64);
        }
        let before = self.pieces;
        for piece in self.chunker.push(delta) {
            self.say(piece);
        }
        before == 0 && self.pieces > 0
    }

    /// The reply is complete. `prose` is spoken whole when nothing streamed.
    pub fn finish(&mut self, prose: &str) {
        if !self.chunker.started() {
            if self.first_token_ms.is_none() {
                self.first_token_ms = Some(self.sent.elapsed().as_millis() as u64);
            }
            let prose = self.within_cap(prose);
            for piece in self.chunker.push(prose) {
                self.say(piece);
            }
        }
        // Ending the last line cuts it into sentences like the rest.
        for piece in self.chunker.push("\n") {
            self.say(piece);
        }
        if let Some(rest) = self.chunker.finish() {
            self.say(rest);
        }
        // Closing the text channel lets both threads drain and signal done.
        self.text_tx = None;
    }

    /// The part of `text` that still fits the reply's size cap.
    fn within_cap<'a>(&mut self, text: &'a str) -> &'a str {
        let mut end = self.left.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.left -= end;
        &text[..end]
    }

    fn say(&mut self, piece: String) {
        if let Some(tx) = &self.text_tx {
            if tx.send(piece).is_ok() {
                self.pieces += 1;
            }
        }
    }

    /// Stop talking now, and say nothing more of this reply even as it keeps
    /// streaming in. Finishing it still signals done.
    pub fn hush(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.left = 0;
    }

    /// Fires once the last piece has played (or the reply was stopped).
    pub fn take_done(&mut self) -> Option<Receiver<()>> {
        self.done.take()
    }

    /// True once every piece has played.
    pub fn finished(&self) -> bool {
        self.over.load(Ordering::SeqCst)
    }

    /// True while the reply is still streaming in.
    pub fn is_open(&self) -> bool {
        self.text_tx.is_some()
    }

    /// True once any piece has been queued.
    pub fn started(&self) -> bool {
        self.chunker.started()
    }

    /// A speech error since the last look, if any.
    pub fn error(&self) -> Option<String> {
        self.errors.try_iter().last()
    }

    /// Where the time went, for a reply to a voice turn.
    pub fn timings(&self) -> Option<VoiceTimings> {
        let turn = self.turn?;
        let audio = self.first_audio.load(Ordering::SeqCst);
        Some(VoiceTimings {
            turn_ms: None,
            transcript_ms: Some(turn.words_ms),
            first_token_ms: self.first_token_ms,
            first_audio_ms: (audio > 0).then(|| audio - 1),
        })
    }
}

impl Drop for SpokenReply {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.text_tx = None;
    }
}

static TIMINGS: Mutex<Vec<VoiceTimings>> = Mutex::new(Vec::new());

/// Keep this turn's timings and rewrite `voice-timings.txt` in the config
/// folder: p50/p95 on top, then the latest turns. Numbers only.
pub fn log_timings(t: VoiceTimings) {
    let text = {
        let mut all = TIMINGS.lock().unwrap_or_else(|e| e.into_inner());
        all.push(t);
        let over = all.len().saturating_sub(TIMINGS_KEPT);
        all.drain(..over);
        timings_text(&all)
    };
    // The folder is read here: it can be pinned per thread.
    let path = crate::config::config_dir().join("voice-timings.txt");
    std::thread::spawn(move || {
        let _ = std::fs::write(path, text);
    });
}

fn timings_text(turns: &[VoiceTimings]) -> String {
    let fmt = |v: Option<u64>| v.map_or("-".to_string(), |ms| ms.to_string());
    let names = ["turn end", "words", "first token", "first audio"];
    let mut out = format!("Voice turns: {} (ms, p50 / p95)\n", turns.len());
    for (name, (p50, p95)) in names.iter().zip(timing_percentiles(turns)) {
        out.push_str(&format!("{name}: {} / {}\n", fmt(p50), fmt(p95)));
    }
    out.push('\n');
    for t in turns.iter().rev() {
        out.push_str(&t.line());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorder() -> (Synth, Play, Arc<Mutex<Vec<String>>>) {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let log = heard.clone();
        let synth: Synth = Box::new(|text| {
            if text.contains("FAIL") {
                Err("speech failed".into())
            } else {
                Ok(text.as_bytes().to_vec())
            }
        });
        let play: Play = Box::new(move |mp3, _| {
            log.lock()
                .unwrap()
                .push(String::from_utf8(mp3).unwrap_or_default());
        });
        (synth, play, heard)
    }

    fn wait(done: Receiver<()>) {
        done.recv_timeout(Duration::from_secs(5)).expect("done");
    }

    #[test]
    fn the_first_sentence_plays_before_the_reply_finishes() {
        let (synth, play, heard) = recorder();
        let turn = TurnClock {
            ended: Instant::now(),
            words_ms: 400,
        };
        let mut reply = SpokenReply::start(synth, play, Some(turn));
        assert!(!reply.feed("Sure, the forecast"));
        assert!(reply.feed(" for tomorrow looks sunny, with a light"));
        let deadline = Instant::now() + Duration::from_secs(5);
        while heard.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            heard.lock().unwrap().first().map(String::as_str),
            Some("Sure, the forecast for tomorrow looks sunny,"),
            "spoken while the reply is still streaming"
        );
        reply.feed(" breeze. Enjoy it!");
        reply.finish("ignored: it streamed");
        assert!(!reply.is_open());
        let done = reply.take_done().unwrap();
        wait(done);
        assert!(reply.finished());
        assert_eq!(
            *heard.lock().unwrap(),
            [
                "Sure, the forecast for tomorrow looks sunny,",
                "with a light breeze.",
                "Enjoy it!"
            ]
        );
        let t = reply.timings().expect("a voice turn");
        assert_eq!(t.transcript_ms, Some(400));
        assert!(t.first_token_ms.is_some() && t.first_audio_ms.is_some());
    }

    #[test]
    fn a_reply_that_did_not_stream_is_spoken_whole_in_order() {
        let (synth, play, heard) = recorder();
        let mut reply = SpokenReply::start(synth, play, None);
        reply.finish("One. Two. Three.");
        wait(reply.take_done().unwrap());
        assert_eq!(heard.lock().unwrap().join(" "), "One. Two. Three.");
        assert!(reply.timings().is_none(), "not a voice turn");
    }

    #[test]
    fn a_huge_reply_is_cut_at_the_text_cap() {
        let (synth, play, heard) = recorder();
        let mut reply = SpokenReply::start(synth, play, None);
        let line = "Word ".repeat(TEXT_FILE_CAP / 5 + 100) + ".";
        reply.feed(&line);
        reply.feed(" Never said.");
        reply.finish("");
        wait(reply.take_done().unwrap());
        let said: usize = heard.lock().unwrap().iter().map(String::len).sum();
        assert!(said <= TEXT_FILE_CAP, "{said}");
        assert!(!heard.lock().unwrap().iter().any(|p| p.contains("Never")));
    }

    #[test]
    fn a_stale_turn_is_not_the_one_answered() {
        note_turn(TurnClock {
            ended: Instant::now() - TURN_CLOCK_MAX_AGE - Duration::from_secs(1),
            words_ms: 1,
        });
        assert!(take_turn().is_none());
        note_turn(TurnClock {
            ended: Instant::now(),
            words_ms: 2,
        });
        assert_eq!(take_turn().map(|c| c.words_ms), Some(2));
        assert!(take_turn().is_none(), "taken once");
    }

    #[test]
    fn a_failed_piece_is_reported_and_the_rest_still_plays() {
        let (synth, play, heard) = recorder();
        let mut reply = SpokenReply::start(synth, play, None);
        reply.finish("FAIL this one. Keep this one.");
        wait(reply.take_done().unwrap());
        assert_eq!(reply.error().as_deref(), Some("speech failed"));
        assert_eq!(*heard.lock().unwrap(), ["Keep this one."]);
    }

    #[test]
    fn dropping_the_reply_stops_playback() {
        let stopped = Arc::new(AtomicBool::new(false));
        let seen = stopped.clone();
        let synth: Synth = Box::new(|t| Ok(t.as_bytes().to_vec()));
        let play: Play = Box::new(move |_, stop| {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
            seen.store(true, Ordering::SeqCst);
        });
        let mut reply = SpokenReply::start(synth, play, None);
        reply.finish("A very long sentence that would play for a while.");
        let done = reply.take_done().unwrap();
        std::thread::sleep(Duration::from_millis(30));
        drop(reply);
        wait(done);
        assert!(stopped.load(Ordering::SeqCst));
    }

    #[test]
    fn timings_file_puts_percentiles_first() {
        let turns = vec![
            VoiceTimings {
                transcript_ms: Some(500),
                first_audio_ms: Some(1500),
                ..Default::default()
            },
            VoiceTimings {
                transcript_ms: Some(700),
                first_audio_ms: Some(2500),
                ..Default::default()
            },
        ];
        let text = timings_text(&turns);
        assert!(text.starts_with("Voice turns: 2"), "{text}");
        assert!(text.contains("first audio: "), "{text}");
        assert!(text.contains("voice turn:"), "{text}");
    }
}
